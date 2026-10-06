//! Reactive-domain handlers: signal registry introspection, gated signal
//! writes, and hot-reload status.
//!
//! `signals_list` prefers the app-provided [`ArenaProbe`](super::ArenaProbe);
//! when the probe reports `not_implemented`, the handler falls back to
//! in-process introspection of the ambient
//! [`ReactiveRuntime`](martensite_reactive::ReactiveRuntime). The runtime's
//! only public enumeration surface is `snapshot_signals`, which exists
//! solely under the `devtools-timemachine` feature — without it the
//! runtime cannot enumerate nodes at all and the fallback honestly reports
//! `not_implemented` (ADR-0039 D9: never fabricate signal state).
//!
//! Writes go through [`SignalAdapter`](super::SignalAdapter)s registered
//! via [`DevSession::register_signal_adapter`]: `Signal::set` is generic
//! over the payload type, so a JSON write needs a per-signal codec the
//! adapter supplies. Both probe- and fallback-produced enumerations are
//! enriched with adapter backing — matched entries gain a live `value`
//! and `"writable": true`; adapter signals absent from the enumeration
//! are appended as adapter-only entries.

use serde_json::{json, Value};

use super::{is_not_implemented, not_impl, DevSession, SessionResult};

impl DevSession {
    /// `signals_list` — registered signals, dirty state, dependency edges.
    ///
    /// Passthrough to [`ArenaProbe::probe_signals_list`](super::ArenaProbe::probe_signals_list).
    /// When the app does not back the capability (`not_implemented` error),
    /// the in-process fallback enumerates source signals registered with
    /// the ambient [`ReactiveRuntime`](martensite_reactive::ReactiveRuntime)
    /// and applies the `signal_id`/`only_dirty`/`limit`/`offset` filters.
    /// `node_id` scoping requires arena knowledge the fallback does not
    /// have and returns `not_implemented` rather than a silently
    /// unfiltered list.
    ///
    /// Either way the enumeration is merged with the registered
    /// [`SignalAdapter`](super::SignalAdapter)s — matched entries gain a
    /// live `value` and `"writable": true`, adapter-only signals are
    /// appended — and `signal_id`/`only_dirty`/`limit`/`offset` filters
    /// apply to the merged set.
    pub fn signals_list(&self, params: &Value) -> SessionResult {
        let probed = self
            .probe
            .lock()
            .expect("probe mutex")
            .probe_signals_list(params);
        match probed {
            Err(err) if is_not_implemented(&err) => self.signals_list_fallback(params),
            Ok(v) => Ok(self.merge_adapter_signals(params, v)),
            Err(e) => Err(e),
        }
    }

    /// `signal_trigger` — apply a type-checked signal write (tweak-gated).
    ///
    /// The only local validation is a nonempty `signal_id`; the write is
    /// type-checked against the registered schema by the app behind
    /// [`ArenaProbe::probe_signal_set`](super::ArenaProbe::probe_signal_set).
    /// When the probe reports `not_implemented` the handler falls back to
    /// the session's own mechanisms: a matching [`SignalAdapter`](super::SignalAdapter)'s
    /// `write_json` first, then — under `devtools-timemachine` — a
    /// `mark_dirty` + `flush` force re-evaluation when `signal_id`
    /// resolves to a runtime id. Everything else is an honest
    /// `not_implemented`.
    pub fn signal_trigger(&self, params: &Value) -> SessionResult {
        let signal_id = params
            .get("signal_id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if signal_id.is_empty() {
            return Err("invalid param: signal_id must be a nonempty string".to_string());
        }
        let value = params.get("value").cloned().unwrap_or(Value::Null);
        let probed = self
            .probe
            .lock()
            .expect("probe mutex")
            .probe_signal_set(signal_id, &value);
        match probed {
            Err(err) if is_not_implemented(&err) => self.signal_trigger_fallback(signal_id, &value),
            other => other,
        }
    }

    /// `signal_set` — write a JSON value through a registered
    /// [`SignalAdapter`](super::SignalAdapter). Signals without an
    /// adapter report an honest error naming the registration call —
    /// the capability is per-signal, not per-session.
    pub fn signal_set(&self, params: &Value) -> SessionResult {
        let signal_id = params
            .get("signal_id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if signal_id.is_empty() {
            return Err("invalid param: signal_id must be a nonempty string".to_string());
        }
        // `value: null` is a legal write; only an absent key is invalid.
        let Some(value) = params.get("value") else {
            return Err("invalid param: value is required".to_string());
        };
        self.write_via_adapter(signal_id, value)
    }

    /// `reload_status` — active build id, reload counters, diagnostics.
    pub fn reload_status(&self, _params: &Value) -> SessionResult {
        let stats = self.reload.lock().expect("reload mutex");
        Ok(json!({
            "active": stats.active_build_id.is_some(),
            "active_build_id": stats.active_build_id.clone(),
            "last_reload_timestamp": stats.last_reload_timestamp.clone(),
            "total_reloads": stats.total_reloads,
            "compiler_diagnostics": stats.compiler_diagnostics.clone(),
            "state_preservation_report": stats.state_preservation_report.clone(),
        }))
    }

    /// Applies a JSON write through the registered
    /// [`SignalAdapter`](super::SignalAdapter) matching `signal_id` —
    /// adapters match by `name` first, then by numeric `signal_id`
    /// (bare digits or the `SignalId(n)` display form). Adapter codec
    /// errors propagate unchanged; the absence of a matching adapter is
    /// an honest lookup error, not `not_implemented` — the method
    /// exists, the per-signal capability does not.
    fn write_via_adapter(&self, signal_id: &str, value: &Value) -> SessionResult {
        let adapters = self.signal_adapters();
        let raw = signal_id_raw(signal_id);
        let adapter = adapters
            .iter()
            .find(|a| a.name == signal_id)
            .or_else(|| raw.and_then(|n| adapters.iter().find(|a| a.signal_id == Some(n))));
        let Some(adapter) = adapter else {
            return Err(format!(
                "no signal adapter registered for '{signal_id}' \
                 (register via DevSession::register_signal_adapter)"
            ));
        };
        (adapter.write_json)(value)?;
        Ok(json!({
            "applied": true,
            "signal": adapter.name,
            "signal_id": adapter.signal_id,
            "revision": self.bump_revision(),
        }))
    }

    /// `true` when a registered adapter matches `signal_id` by name or
    /// numeric signal id.
    fn has_signal_adapter(&self, signal_id: &str) -> bool {
        let adapters = self.signal_adapters();
        let raw = signal_id_raw(signal_id);
        adapters
            .iter()
            .any(|a| a.name == signal_id || (raw.is_some() && a.signal_id == raw))
    }

    /// `signal_trigger` fallback when the app probe reports
    /// `not_implemented`: adapter-backed signals take the JSON write
    /// path; under `devtools-timemachine` a `signal_id` that parses as
    /// a [`SignalId`](martensite_reactive::SignalId) is force-dirtied
    /// and re-evaluated via the ambient runtime (`mark_dirty` pushes
    /// the dirty bit to subscribers, `evaluate_node`/`flush` run the
    /// pending queue). Anything else reports `not_implemented` — a JSON
    /// `Signal::set` is impossible without the payload's concrete type.
    fn signal_trigger_fallback(&self, signal_id: &str, value: &Value) -> SessionResult {
        if self.has_signal_adapter(signal_id) {
            return self.write_via_adapter(signal_id, value);
        }
        #[cfg(feature = "devtools-timemachine")]
        if let Some(id) = signal_id_raw(signal_id).map(martensite_reactive::SignalId) {
            let runtime = martensite_reactive::ReactiveRuntime::current();
            runtime.mark_dirty(id);
            runtime.evaluate_node(id);
            runtime.flush();
            return Ok(json!({
                "triggered": true,
                "signal_id": signal_id,
                "mechanism": "mark_dirty_flush",
            }));
        }
        Err(not_impl("signal_trigger"))
    }

    /// Merges registered [`SignalAdapter`](super::SignalAdapter)s into a
    /// `signals_list` enumeration — probe-produced or runtime-fallback
    /// alike — then applies `signal_id`/`only_dirty`/`limit`/`offset`
    /// to the merged set.
    ///
    /// Entries matching an adapter (by `name`, then raw signal id) gain
    /// the adapter's `name`, a live `value` from `read_json`, and
    /// `"writable": true`. Entries without adapter backing get
    /// `"writable": false`/`"value": null` only when the producer did
    /// not already supply them. Adapter signals absent from the
    /// enumeration are appended as `dirty: false` adapter-only entries.
    /// `total`/`offset` reflect the filtered, unpaged merged set.
    fn merge_adapter_signals(&self, params: &Value, mut base: Value) -> Value {
        /// Default page size (spec §3.5).
        const DEFAULT_LIMIT: usize = 50;
        /// Maximum page size (spec §3.5).
        const MAX_LIMIT: usize = 200;

        let Some(base_obj) = base.as_object_mut() else {
            return base;
        };

        let adapters = self.signal_adapters();
        let mut entries: Vec<Value> = base_obj
            .get("signals")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        // Enrich enumerated entries with adapter backing where present.
        let mut claimed = vec![false; adapters.len()];
        for entry in &mut entries {
            let Some(obj) = entry.as_object_mut() else {
                continue;
            };
            let id_str = obj.get("id").and_then(Value::as_str).unwrap_or_default();
            let name_str = obj.get("name").and_then(Value::as_str).unwrap_or_default();
            let raw = signal_id_raw(id_str);
            let idx = adapters.iter().position(|a| {
                a.name == name_str
                    || a.name == id_str
                    || (a.signal_id.is_some() && a.signal_id == raw)
            });
            if let Some(i) = idx {
                claimed[i] = true;
                let adapter = &adapters[i];
                obj.insert("name".to_string(), Value::String(adapter.name.clone()));
                obj.insert(
                    "value".to_string(),
                    (adapter.read_json)().unwrap_or(Value::Null),
                );
                obj.insert("writable".to_string(), Value::Bool(true));
            } else {
                obj.entry("writable".to_string())
                    .or_insert(Value::Bool(false));
                obj.entry("value".to_string()).or_insert(Value::Null);
            }
        }

        // Adapter-only signals never enumerated by probe or runtime.
        for (i, adapter) in adapters.iter().enumerate() {
            if claimed[i] {
                continue;
            }
            entries.push(json!({
                "id": adapter.signal_id.map(|raw| format!("SignalId({raw})")),
                "name": adapter.name,
                "kind": "signal",
                "dirty": false,
                "poisoned": false,
                "dependents": Value::Null,
                "dependencies": Value::Null,
                "subscriber_count": Value::Null,
                "writable": true,
                "value": (adapter.read_json)().unwrap_or(Value::Null),
            }));
        }
        drop(adapters);

        let wanted = params.get("signal_id").and_then(Value::as_str);
        let wanted_raw = wanted.and_then(signal_id_raw);
        let only_dirty = params
            .get("only_dirty")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let limit = params
            .get("limit")
            .and_then(Value::as_u64)
            .and_then(|v| usize::try_from(v.min(MAX_LIMIT as u64)).ok())
            .unwrap_or(DEFAULT_LIMIT);
        let offset = params
            .get("offset")
            .and_then(Value::as_u64)
            .and_then(|v| usize::try_from(v).ok())
            .unwrap_or(0);

        entries.retain(|entry| {
            if let Some(w) = wanted {
                let id = entry.get("id").and_then(Value::as_str).unwrap_or_default();
                let name = entry
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                // `id` may be a `"SignalId(n)"` string or a bare number.
                let entry_raw = entry
                    .get("id")
                    .and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(signal_id_raw)));
                let hit = id == w
                    || (!name.is_empty() && name == w)
                    || (wanted_raw.is_some() && entry_raw == wanted_raw);
                if !hit {
                    return false;
                }
            }
            // `only_dirty` applies to runtime state; adapter-only
            // entries report `dirty: false` and are excluded.
            if only_dirty && entry.get("dirty").and_then(Value::as_bool) != Some(true) {
                return false;
            }
            true
        });

        let total = entries.len();
        let page: Vec<Value> = entries.into_iter().skip(offset).take(limit).collect();
        base_obj.insert("signals".to_string(), Value::Array(page));
        base_obj.insert("total".to_string(), Value::from(total));
        base_obj.insert("offset".to_string(), Value::from(offset));
        base
    }

    /// In-process `signals_list` fallback over
    /// [`ReactiveRuntime::current`](martensite_reactive::ReactiveRuntime),
    /// merged with the session's [`SignalAdapter`](super::SignalAdapter)s.
    ///
    /// `snapshot_signals` enumerates source signals that registered a
    /// type-erased storage accessor — lazily, on their first
    /// `Clone`-bounded read or write — so the list can legitimately be
    /// empty even when signals exist. Graph topology (`dependents`,
    /// `dependencies`, `subscriber_count`) and human-readable names are
    /// not part of the public API, so those fields are emitted as
    /// `null` rather than fabricated; adapter metadata fills `name` and
    /// `value` where a registered adapter supplies them.
    #[cfg(feature = "devtools-timemachine")]
    fn signals_list_fallback(&self, params: &Value) -> SessionResult {
        if params.get("node_id").is_some_and(|v| !v.is_null()) {
            // Widget-node scoping requires arena knowledge only an
            // app-provided probe has; refuse instead of returning an
            // unfiltered list that pretends to be scoped (D9).
            return Err(not_impl("signals_list:node_id_scope"));
        }

        let runtime = martensite_reactive::ReactiveRuntime::current();

        // Sorted, deterministic id order from `SignalSnapshot::signals`.
        let mut entries: Vec<Value> = Vec::new();
        for id in runtime.snapshot_signals().signals() {
            entries.push(json!({
                "id": id.to_string(),
                "name": Value::Null,
                "kind": "signal",
                "dirty": runtime.is_dirty(id),
                "poisoned": runtime.is_poisoned(id),
                "dependents": Value::Null,
                "dependencies": Value::Null,
                "subscriber_count": Value::Null,
                "writable": false,
                "value": Value::Null,
            }));
        }

        let base = json!({
            "signals": entries,
            "errors": runtime
                .errors()
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<String>>(),
        });
        Ok(self.merge_adapter_signals(params, base))
    }

    /// Without the `devtools-timemachine` source-signal registry the
    /// ambient [`ReactiveRuntime`](martensite_reactive::ReactiveRuntime)
    /// exposes no node enumeration at all — the fallback can only list
    /// adapter-registered signals, and reports the capability gap
    /// honestly (D9) when none are registered.
    #[cfg(not(feature = "devtools-timemachine"))]
    fn signals_list_fallback(&self, params: &Value) -> SessionResult {
        if params.get("node_id").is_some_and(|v| !v.is_null()) {
            return Err(not_impl("signals_list:node_id_scope"));
        }
        if self.signal_adapters().is_empty() {
            return Err(not_impl("signals_list"));
        }
        Ok(self.merge_adapter_signals(params, json!({"signals": [], "errors": []})))
    }
}

/// Extracts the raw `u64` from a `signal_id` param — bare decimal
/// digits or the `SignalId(n)` display form.
fn signal_id_raw(s: &str) -> Option<u64> {
    s.strip_prefix("SignalId(")
        .and_then(|rest| rest.strip_suffix(')'))
        .unwrap_or(s)
        .parse::<u64>()
        .ok()
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use serde_json::json;

    use super::super::{ArenaProbe, NullProbe, SignalAdapter};
    use super::*;

    fn session() -> DevSession {
        DevSession::new(Box::new(NullProbe))
    }

    /// Adapter backed by a JSON cell — a stand-in for a typed signal
    /// codec: `read_json` clones the cell, `write_json` replaces it.
    fn cell_adapter(name: &str, signal_id: Option<u64>) -> (SignalAdapter, Arc<Mutex<Value>>) {
        let cell = Arc::new(Mutex::new(Value::Null));
        let read = {
            let cell = Arc::clone(&cell);
            move || Some(cell.lock().expect("cell").clone())
        };
        let write = {
            let cell = Arc::clone(&cell);
            move |v: &Value| {
                *cell.lock().expect("cell") = v.clone();
                Ok(())
            }
        };
        (
            SignalAdapter {
                name: name.to_string(),
                signal_id,
                read_json: Box::new(read),
                write_json: Box::new(write),
            },
            cell,
        )
    }

    #[test]
    fn signal_trigger_rejects_missing_or_empty_id() {
        let s = session();
        assert!(s.signal_trigger(&json!({})).is_err());
        assert!(s.signal_trigger(&json!({"value": 1})).is_err());
        assert!(s
            .signal_trigger(&json!({"signal_id": "", "value": 1}))
            .is_err());
        assert!(s
            .signal_trigger(&json!({"signal_id": 42, "value": 1}))
            .is_err());
    }

    #[test]
    fn signal_trigger_passthrough_to_probe() {
        struct Echo;
        impl ArenaProbe for Echo {
            fn probe_signal_set(&mut self, signal_id: &str, value: &Value) -> SessionResult {
                Ok(json!({"applied": signal_id, "value": value}))
            }
        }
        let s = DevSession::new(Box::new(Echo));
        let v = s
            .signal_trigger(&json!({"signal_id": "sig-1", "value": {"x": 2}}))
            .expect("trigger");
        assert_eq!(v["applied"], "sig-1");
        assert_eq!(v["value"]["x"], 2);
    }

    #[test]
    fn signal_trigger_propagates_probe_errors() {
        let s = session();
        let err = s
            .signal_trigger(&json!({"signal_id": "sig-1", "value": 0}))
            .expect_err("NullProbe is not implemented");
        assert!(err.starts_with("not_implemented:"), "{err}");
    }

    #[test]
    fn reload_status_fresh_session() {
        let s = session();
        let v = s.reload_status(&json!({})).expect("reload_status");
        assert_eq!(v["active"], false);
        assert_eq!(v["total_reloads"], 0);
        assert!(v["active_build_id"].is_null());
        assert!(v["last_reload_timestamp"].is_null());
        assert!(v["compiler_diagnostics"]
            .as_array()
            .is_some_and(Vec::is_empty));
        assert!(v["state_preservation_report"].is_null());
    }

    #[test]
    fn reload_status_after_record_reload() {
        let s = session();
        s.record_reload("b1");
        let v = s.reload_status(&json!({})).expect("reload_status");
        assert_eq!(v["active"], true);
        assert_eq!(v["active_build_id"], "b1");
        assert_eq!(v["total_reloads"], 1);
        assert!(v["last_reload_timestamp"].is_string());
    }

    #[cfg(not(feature = "devtools-timemachine"))]
    #[test]
    fn signals_list_without_probe_backing_is_honest() {
        let s = session();
        let err = s.signals_list(&json!({})).expect_err("must be honest");
        assert!(err.starts_with("not_implemented:"), "{err}");
    }

    #[cfg(feature = "devtools-timemachine")]
    #[test]
    fn signals_list_fallback_enumerates_registered_sources() {
        use martensite_reactive::ReactiveRuntime;

        let s = session();
        // Dedicated runtime so tests sharing the global singleton cannot
        // leak signals into each other's snapshots.
        let rt = ReactiveRuntime::new();
        ReactiveRuntime::with_current(&rt, || {
            let sig = rt.create_signal(7i32);
            let _ = sig.get_untracked(); // lazily registers source access
            let dirty_src = rt.create_signal(false);
            let _ = dirty_src.get_untracked();

            let v = s.signals_list(&json!({})).expect("signals_list");
            let arr = v["signals"].as_array().expect("signals array");
            assert_eq!(arr.len(), 2);
            assert_eq!(v["total"], 2);
            let entry = arr
                .iter()
                .find(|e| e["id"] == sig.id().to_string())
                .expect("signal entry");
            assert_eq!(entry["kind"], "signal");
            assert_eq!(entry["dirty"], false);
            assert!(entry["dependents"].is_null());

            // `signal_id` filter accepts the Display form or raw digits.
            let v = s
                .signals_list(&json!({"signal_id": sig.id().to_string()}))
                .expect("filtered");
            assert_eq!(v["signals"].as_array().expect("array").len(), 1);
            let v = s
                .signals_list(&json!({"signal_id": sig.id().raw().to_string()}))
                .expect("raw filtered");
            assert_eq!(v["signals"].as_array().expect("array").len(), 1);

            // Pagination.
            let v = s
                .signals_list(&json!({"limit": 1, "offset": 1}))
                .expect("paged");
            assert_eq!(v["signals"].as_array().expect("array").len(), 1);
            assert_eq!(v["total"], 2);

            // Node scoping cannot be satisfied without an app probe.
            let err = s
                .signals_list(&json!({"node_id": "n1"}))
                .expect_err("node scope unsupported");
            assert!(err.starts_with("not_implemented:"), "{err}");
        });
    }

    #[cfg(feature = "devtools-timemachine")]
    #[test]
    fn signals_list_prefers_probe_over_fallback() {
        struct AppBacked;
        impl ArenaProbe for AppBacked {
            fn probe_signals_list(&mut self, _params: &Value) -> SessionResult {
                Ok(json!({"signals": [{"id": "app-1"}], "from": "probe"}))
            }
        }
        let s = DevSession::new(Box::new(AppBacked));
        let v = s.signals_list(&json!({})).expect("probe result");
        assert_eq!(v["from"], "probe");
    }

    #[test]
    fn signal_set_writes_through_adapter_by_name_and_id() {
        let s = session();
        let (adapter, cell) = cell_adapter("counter", Some(7));
        s.register_signal_adapter(adapter);

        let v = s
            .signal_set(&json!({"signal_id": "counter", "value": 42}))
            .expect("set by name");
        assert_eq!(v["applied"], true);
        assert_eq!(v["signal"], "counter");
        assert_eq!(v["signal_id"], 7);
        assert_eq!(v["revision"], s.current_revision());
        assert_eq!(*cell.lock().expect("cell"), json!(42));

        // Numeric `signal_id` resolution (bare digits).
        s.signal_set(&json!({"signal_id": "7", "value": 9}))
            .expect("set by id");
        assert_eq!(*cell.lock().expect("cell"), json!(9));

        // Explicit `null` is a legal value, and each write bumps the
        // optimistic-lock revision.
        let first_rev = v["revision"].as_u64().expect("revision");
        let v = s
            .signal_set(&json!({"signal_id": "counter", "value": null}))
            .expect("explicit null");
        assert!(v["revision"].as_u64().expect("revision") > first_rev);
        assert_eq!(*cell.lock().expect("cell"), Value::Null);
    }

    #[test]
    fn signal_set_requires_adapter_and_value() {
        let s = session();
        // Missing `signal_id` / missing `value` key are invalid params.
        assert!(s.signal_set(&json!({"value": 1})).is_err());
        assert!(s.signal_set(&json!({"signal_id": "x"})).is_err());
        assert!(s.signal_set(&json!({"signal_id": ""})).is_err());

        // No adapter → honest per-signal capability error, not
        // `not_implemented` (the method exists).
        let err = s
            .signal_set(&json!({"signal_id": "ghost", "value": 1}))
            .expect_err("unregistered signal");
        assert!(err.contains("no signal adapter registered"), "{err}");
        assert!(err.contains("register_signal_adapter"), "{err}");
        assert!(!err.starts_with("not_implemented:"), "{err}");

        // Adapter codec errors propagate unchanged.
        s.register_signal_adapter(SignalAdapter {
            name: "typed".to_string(),
            signal_id: None,
            read_json: Box::new(|| None),
            write_json: Box::new(|_| Err("type mismatch: expected u64".to_string())),
        });
        let err = s
            .signal_set(&json!({"signal_id": "typed", "value": "nope"}))
            .expect_err("codec error");
        assert_eq!(err, "type mismatch: expected u64");
    }

    #[test]
    fn signal_trigger_falls_back_to_adapter_write() {
        let s = session(); // NullProbe → probe reports not_implemented.
        let (adapter, cell) = cell_adapter("counter", Some(7));
        s.register_signal_adapter(adapter);

        let v = s
            .signal_trigger(&json!({"signal_id": "counter", "value": 5}))
            .expect("adapter write");
        assert_eq!(v["applied"], true);
        assert_eq!(v["signal"], "counter");
        assert_eq!(*cell.lock().expect("cell"), json!(5));

        // Numeric id resolves to the same adapter.
        s.signal_trigger(&json!({"signal_id": "7", "value": 6}))
            .expect("adapter write by id");
        assert_eq!(*cell.lock().expect("cell"), json!(6));
    }

    #[cfg(not(feature = "devtools-timemachine"))]
    #[test]
    fn signal_trigger_fallback_without_runtime_or_adapter_is_honest() {
        let s = session();
        let err = s
            .signal_trigger(&json!({"signal_id": "42", "value": 1}))
            .expect_err("no adapter, no runtime registry");
        assert!(err.starts_with("not_implemented:"), "{err}");
    }

    #[test]
    fn signals_list_merges_adapters_into_probe_result() {
        struct AppBacked;
        impl ArenaProbe for AppBacked {
            fn probe_signals_list(&mut self, _params: &Value) -> SessionResult {
                Ok(json!({
                    "signals": [
                        {"id": "SignalId(7)", "name": "counter", "kind": "signal",
                         "dirty": true},
                        {"id": "app-2", "name": "other", "kind": "signal",
                         "dirty": false},
                    ],
                    "from": "probe",
                }))
            }
        }
        let s = DevSession::new(Box::new(AppBacked));
        let (adapter, cell) = cell_adapter("counter", Some(7));
        s.register_signal_adapter(adapter);
        let (extra, _extra_cell) = cell_adapter("extra", None);
        s.register_signal_adapter(extra);
        *cell.lock().expect("cell") = json!(3);

        let v = s.signals_list(&json!({})).expect("list");
        assert_eq!(v["from"], "probe"); // producer fields preserved
        let arr = v["signals"].as_array().expect("signals array");
        assert_eq!(arr.len(), 3);
        assert_eq!(v["total"], 3);

        // Adapter-backed entry gains name/value/writable, keeps dirty.
        let counter = arr
            .iter()
            .find(|e| e["name"] == "counter")
            .expect("counter entry");
        assert_eq!(counter["writable"], true);
        assert_eq!(counter["value"], 3);
        assert_eq!(counter["dirty"], true);

        // Unbacked entries are read-only through the dev channel.
        let other = arr
            .iter()
            .find(|e| e["name"] == "other")
            .expect("other entry");
        assert_eq!(other["writable"], false);
        assert!(other["value"].is_null());

        // Adapter-only signal is appended with dirty: false.
        let extra = arr
            .iter()
            .find(|e| e["name"] == "extra")
            .expect("adapter-only entry");
        assert_eq!(extra["writable"], true);
        assert_eq!(extra["dirty"], false);
        assert!(extra["id"].is_null());

        // Filters apply across the merged set.
        let v = s
            .signals_list(&json!({"signal_id": "extra"}))
            .expect("filter by name");
        assert_eq!(v["signals"].as_array().expect("array").len(), 1);
        let v = s
            .signals_list(&json!({"signal_id": "7"}))
            .expect("filter by raw id");
        let arr = v["signals"].as_array().expect("array");
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["name"], "counter");
        let v = s
            .signals_list(&json!({"only_dirty": true}))
            .expect("dirty filter");
        let arr = v["signals"].as_array().expect("array");
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["name"], "counter");
        let v = s
            .signals_list(&json!({"limit": 2, "offset": 1}))
            .expect("paged");
        assert_eq!(v["signals"].as_array().expect("array").len(), 2);
        assert_eq!(v["total"], 3);
        assert_eq!(v["offset"], 1);
    }

    #[test]
    fn signals_list_adapter_only_without_probe_backing() {
        use martensite_reactive::ReactiveRuntime;

        let s = session();
        let (adapter, cell) = cell_adapter("gauge", Some(11));
        s.register_signal_adapter(adapter);
        *cell.lock().expect("cell") = json!("on");

        // Dedicated runtime so other tests' global-singleton signals
        // cannot leak into the `devtools-timemachine` enumeration.
        let rt = ReactiveRuntime::new();
        ReactiveRuntime::with_current(&rt, || {
            let v = s.signals_list(&json!({})).expect("adapter-only list");
            let arr = v["signals"].as_array().expect("signals array");
            assert_eq!(arr.len(), 1);
            assert_eq!(arr[0]["name"], "gauge");
            assert_eq!(arr[0]["id"], "SignalId(11)");
            assert_eq!(arr[0]["value"], "on");
            assert_eq!(arr[0]["writable"], true);
            assert_eq!(v["total"], 1);
        });
    }

    #[cfg(feature = "devtools-timemachine")]
    #[test]
    fn signal_trigger_mark_dirty_flush_forces_runtime_reeval() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        use martensite_reactive::ReactiveRuntime;

        let s = session();
        let rt = ReactiveRuntime::new();
        ReactiveRuntime::with_current(&rt, || {
            let sig = rt.create_signal(1i32);
            let runs = Arc::new(AtomicUsize::new(0));
            let runs_for_effect = Arc::clone(&runs);
            let _effect = rt.create_effect({
                let sig = sig.clone();
                move || {
                    let _ = sig.get();
                    runs_for_effect.fetch_add(1, Ordering::SeqCst);
                }
            });
            let before = runs.load(Ordering::SeqCst);

            // No probe, no adapter — a resolvable runtime id takes the
            // mark_dirty + flush path.
            let v = s
                .signal_trigger(&json!({"signal_id": sig.id().to_string()}))
                .expect("runtime trigger");
            assert_eq!(v["triggered"], true);
            assert_eq!(v["mechanism"], "mark_dirty_flush");
            assert!(
                runs.load(Ordering::SeqCst) > before,
                "effect must re-run after mark_dirty flush"
            );

            // An unparseable id stays an honest not_implemented.
            let err = s
                .signal_trigger(&json!({"signal_id": "ghost", "value": 1}))
                .expect_err("unresolvable id");
            assert!(err.starts_with("not_implemented:"), "{err}");
        });
    }
}
