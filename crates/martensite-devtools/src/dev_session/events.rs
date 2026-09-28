//! Event-domain handlers: synthetic event dispatch, the event ledger,
//! and TimeMachine deterministic replay.
//!
//! - `event_dispatch` validates the wire `event_type` (spec §3.15),
//!   records an [`EventRecord`] in the session ledger, and delegates the
//!   live hit-test/routing to [`ArenaProbe`](super::ArenaProbe).
//! - `event_ledger` tails the ledger into the `EventRecord`-shaped
//!   payload consumed by `martensite-mcp` (spec §3.16).
//! - `timemachine_step` drives the `TimeMachine` (spec §3.17) when the
//!   `devtools-timemachine` feature is enabled *and* a machine is
//!   attached via [`DevSession::with_timemachine`]; otherwise the
//!   handler reports the capability gap rather than fabricating state
//!   (ADR-0039 D9).

use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

use martensite_core::WidgetId;

use crate::event_ledger::{Disposition, EventKind, EventRecord, Point};

#[cfg(feature = "devtools-timemachine")]
use crate::timemachine::TimeMachine;

use super::{DevSession, SessionResult};

/// Synthetic `event_type`s accepted by `event_dispatch` (spec §3.15).
const DISPATCH_EVENT_TYPES: &[&str] = &[
    "pointer_click",
    "pointer_move",
    "scroll",
    "key_press",
    "text_input",
];

/// Actions accepted by `timemachine_step` (spec §3.17).
const TIMEMACHINE_ACTIONS: &[&str] = &[
    "pause",
    "resume",
    "step_forward",
    "step_backward",
    "restore_checkpoint",
];

/// Default `event_ledger` page size (spec §3.16).
const LEDGER_DEFAULT_LIMIT: usize = 50;
/// Maximum `event_ledger` page size (spec §3.16).
const LEDGER_MAX_LIMIT: u64 = 200;

/// Error returned when no `TimeMachine` can service `timemachine_step`
/// (feature disabled or `with_timemachine` never called).
const TIMEMACHINE_UNAVAILABLE: &str = "timemachine unavailable: enable devtools-timemachine";

impl DevSession {
    /// `event_dispatch` — inject a synthetic input event and report the
    /// response plus resolved hit path (recorded in the ledger).
    ///
    /// The dispatch attempt is recorded *before* delegation so the
    /// ledger reflects every injected event even when the app probe
    /// cannot route synthetic input; the probe's `not_implemented:`
    /// error (or any other failure) still propagates unchanged. On
    /// success the assigned ledger `seq` is merged into the probe
    /// payload so the wire answer correlates with `event_ledger`.
    pub fn event_dispatch(&self, params: &Value) -> SessionResult {
        let event_type = params
            .get("event_type")
            .and_then(Value::as_str)
            .ok_or_else(|| "invalid param: `event_type` must be a string".to_string())?;
        if !DISPATCH_EVENT_TYPES.contains(&event_type) {
            return Err(format!("invalid param: unknown event_type `{event_type}`"));
        }

        // The lint bridge's evaluated-frame count is the session's only
        // public proxy for "current frame".
        let frame = self.lint.lock().expect("lint mutex").frames_evaluated() as u64;
        let timestamp_ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX))
            .unwrap_or(0);

        let mut record = EventRecord::new(0, frame, event_kind(event_type), Disposition::Ignored)
            .with_timestamp(timestamp_ns);
        if let Some(position) = parse_position(params) {
            record = record.with_position(position);
        }
        let seq = {
            let mut ledger = self.ledger.lock().expect("ledger mutex");
            ledger.push(record);
            ledger.newest().map_or(0, |r| r.seq)
        };

        let result = self
            .probe
            .lock()
            .expect("probe mutex")
            .probe_dispatch_event(event_type, params)?;

        Ok(match result {
            Value::Object(mut map) => {
                map.insert("seq".to_string(), json!(seq));
                map.insert("event_type".to_string(), json!(event_type));
                Value::Object(map)
            }
            other => json!({ "seq": seq, "event_type": event_type, "response": other }),
        })
    }

    /// `event_ledger` — tail the session's event ledger.
    ///
    /// `limit` defaults to 50 and clamps to 200 (spec §3.16). Emits the
    /// newest `limit` records oldest-first under `events` plus `total`,
    /// the number of records currently retained, so callers can see
    /// whether the tail was truncated.
    pub fn event_ledger(&self, params: &Value) -> SessionResult {
        let limit = params
            .get("limit")
            .or_else(|| params.get("tail_count"))
            .and_then(Value::as_u64)
            .map_or(LEDGER_DEFAULT_LIMIT, |v| v.min(LEDGER_MAX_LIMIT) as usize);

        let ledger = self.ledger.lock().expect("ledger mutex");
        let total = ledger.len();
        let events: Vec<Value> = ledger
            .iter()
            .rev()
            .take(limit)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .map(record_to_wire)
            .collect();

        Ok(json!({ "events": events, "total": total }))
    }

    /// `timemachine_step` — `pause`/`resume`/`step_forward`/`step_backward`/
    /// `restore_checkpoint` deterministic replay control.
    ///
    /// `pause` captures a `Checkpoint` at the current history node and
    /// reports its opaque `checkpoint_id` (the history `NodeId` packed as
    /// `version << 32 | idx`) which `restore_checkpoint` accepts back.
    /// `resume` reports current replay state without navigation —
    /// pausing the app's frame loop is the app's responsibility, not the
    /// `TimeMachine`'s. `step_forward`/`step_backward` map to
    /// `redo`/`undo`; `restore_checkpoint` maps to `replay_to` and is
    /// strict: the id must name a node that actually holds a checkpoint.
    pub fn timemachine_step(&self, params: &Value) -> SessionResult {
        let action = params
            .get("action")
            .and_then(Value::as_str)
            .ok_or_else(|| "invalid param: `action` must be a string".to_string())?;
        if !TIMEMACHINE_ACTIONS.contains(&action) {
            return Err(format!("invalid param: unknown action `{action}`"));
        }
        let checkpoint_id = params.get("checkpoint_id").and_then(Value::as_u64);
        if action == "restore_checkpoint" && checkpoint_id.is_none() {
            return Err("invalid param: `restore_checkpoint` requires `checkpoint_id`".to_string());
        }
        self.timemachine_apply(action, checkpoint_id)
    }

    /// Applies a validated `timemachine_step` action to the attached
    /// `TimeMachine` and reports the post-action replay state.
    #[cfg(feature = "devtools-timemachine")]
    fn timemachine_apply(&self, action: &str, checkpoint_id: Option<u64>) -> SessionResult {
        let mut guard = self.machine.lock().expect("timemachine mutex");
        let Some(tm) = guard.as_mut() else {
            return Err(TIMEMACHINE_UNAVAILABLE.to_string());
        };

        let mut checkpoint: Option<u64> = None;
        match action {
            "pause" => {
                checkpoint = node_ffi(tm.checkpoint());
            }
            "resume" => {}
            "step_forward" => {
                tm.redo().map_err(|e| format!("timemachine error: {e}"))?;
            }
            "step_backward" => {
                tm.undo().map_err(|e| format!("timemachine error: {e}"))?;
            }
            "restore_checkpoint" => {
                let Some(id) = checkpoint_id else {
                    return Err(
                        "invalid param: `restore_checkpoint` requires `checkpoint_id`".to_string(),
                    );
                };
                let node = node_from_ffi(id)
                    .ok_or_else(|| format!("invalid param: malformed checkpoint_id `{id}`"))?;
                if tm.checkpoint_at(node).is_none() {
                    return Err(format!("invalid param: no checkpoint with id `{id}`"));
                }
                tm.replay_to(node)
                    .map_err(|e| format!("timemachine error: {e}"))?;
                checkpoint = Some(id);
            }
            other => return Err(format!("invalid param: unknown action `{other}`")),
        }

        Ok(json!({
            "action": action,
            "frame_timestamp": tm.commit_count(),
            "arena_fingerprint": tm.arena_fingerprint(),
            "signal_state_hash": signal_state_hash(tm),
            "checkpoint_id": checkpoint,
            "commit_count": tm.commit_count(),
            "checkpoint_count": tm.checkpoint_count(),
            "history_nodes": tm.ledger().node_count(),
        }))
    }

    /// Without `devtools-timemachine` there is no machine at all —
    /// report the capability gap honestly (ADR-0039 D9).
    #[cfg(not(feature = "devtools-timemachine"))]
    fn timemachine_apply(&self, _action: &str, _checkpoint_id: Option<u64>) -> SessionResult {
        Err(TIMEMACHINE_UNAVAILABLE.to_string())
    }
}

/// Maps a wire `event_type` to its [`EventKind`] ledger category.
fn event_kind(event_type: &str) -> EventKind {
    match event_type {
        "scroll" => EventKind::Scroll,
        "key_press" => EventKind::Key,
        "text_input" => EventKind::Ime,
        _ => EventKind::Pointer,
    }
}

/// Parses a `[x, y]` logical position out of `event_dispatch` params.
fn parse_position(params: &Value) -> Option<Point> {
    let [x, y] = serde_json::from_value::<[f64; 2]>(params.get("position")?.clone()).ok()?;
    Some(Point::new(x as f32, y as f32))
}

/// Serializes an [`EventRecord`] into the wire shape consumed by
/// `martensite-mcp`'s `EventRecordDescriptor` (`seq`, `timestamp`,
/// `event_type`, `target_id`, `ancestry`, `response`,
/// `rejection_reason`) plus the raw ledger fields (`frame`, `kind`,
/// `disposition`, `hit_path`, `rejection`) the spec calls out.
fn record_to_wire(record: &EventRecord) -> Value {
    let hit_path: Vec<u64> = record.hit_path.iter().map(WidgetId::to_u64).collect();
    let ancestry: Vec<u64> = hit_path.iter().rev().copied().collect();
    let target = record
        .disposition
        .target_widget()
        .or_else(|| record.hit_path.last());
    json!({
        "seq": record.seq,
        "frame": record.frame,
        "kind": record.kind.to_string(),
        "event_type": event_type_name(record.kind),
        "disposition": record.disposition.to_string(),
        "response": disposition_name(record.disposition),
        "target_id": target.map(WidgetId::to_u64),
        "hit_path": hit_path,
        "ancestry": ancestry,
        "hit_path_truncated": record.hit_path.is_truncated(),
        "position": record.position.map(|p| json!([p.x, p.y])),
        "timestamp": record_timestamp(record.timestamp),
        "timestamp_ns": record.timestamp,
        "rejection": record.hit_rejection.map(|r| r.to_string()),
        "rejection_reason": record.hit_rejection.map(|r| r.to_string()),
        "focus_from": record.focus_from.map(WidgetId::to_u64),
        "focus_to": record.focus_to.map(WidgetId::to_u64),
    })
}

/// Lowercase wire-style name for an [`EventKind`]. `EventRecord` stores
/// the coarse category only — the fine-grained `event_type` (e.g.
/// `pointer_click` vs `pointer_move`) is not retained by the ledger.
fn event_type_name(kind: EventKind) -> &'static str {
    match kind {
        EventKind::Pointer => "pointer",
        EventKind::Key => "key",
        EventKind::Scroll => "scroll",
        EventKind::Ime => "ime",
        EventKind::Focus => "focus",
        EventKind::Dnd => "dnd",
    }
}

/// Bare [`Disposition`] variant name for the `response` field — the
/// full `Disposition` text (with target ids) is in `disposition`.
fn disposition_name(disposition: Disposition) -> &'static str {
    match disposition {
        Disposition::Handled(_) => "Handled",
        Disposition::Ignored => "Ignored",
        Disposition::BubbledTo(_) => "BubbledTo",
        Disposition::Captured(_) => "Captured",
    }
}

/// Formats a nanosecond unix timestamp as RFC 3339 with seconds
/// precision (`timestamp_ns` keeps the raw value). `0` — the unstamped
/// sentinel — serializes as the empty string.
fn record_timestamp(ns: u64) -> String {
    if ns == 0 {
        return String::new();
    }
    let secs = ns / 1_000_000_000;
    let days = secs / 86_400;
    let sod = secs % 86_400;
    let (y, m, d) = super::days_to_ymd(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        sod / 3600,
        (sod % 3600) / 60,
        sod % 60
    )
}

/// Packs a history `NodeId` into the opaque u64 `checkpoint_id` used on
/// the wire (`version << 32 | idx`, slotmap's ffi layout). Reached via
/// the key's serde form so this crate needs no direct slotmap dep.
#[cfg(feature = "devtools-timemachine")]
fn node_ffi(node: martensite_history::NodeId) -> Option<u64> {
    let v = serde_json::to_value(node).ok()?;
    let idx = v.get("idx")?.as_u64()?;
    let version = v.get("version")?.as_u64()?;
    Some((version << 32) | idx)
}

/// Decodes a wire `checkpoint_id` back into a history `NodeId`.
/// Slotmap's key deserializer forces the version odd, matching
/// `as_ffi`'s packing, so an id produced by [`node_ffi`] round-trips.
#[cfg(feature = "devtools-timemachine")]
fn node_from_ffi(id: u64) -> Option<martensite_history::NodeId> {
    serde_json::from_value(json!({ "idx": id & 0xffff_ffff, "version": id >> 32 })).ok()
}

/// Deterministic hash of the registered source-signal set plus the
/// journal watermark — `SignalSnapshot` values are type-erased
/// (`dyn Any`) and cannot be hashed, so this covers signal membership
/// and write volume rather than value contents. FNV-1a over
/// little-endian u64 words.
#[cfg(feature = "devtools-timemachine")]
fn signal_state_hash(tm: &TimeMachine) -> String {
    const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = FNV_OFFSET;
    let mut absorb = |v: u64| {
        for b in v.to_le_bytes() {
            hash ^= u64::from(b);
            hash = hash.wrapping_mul(FNV_PRIME);
        }
    };
    for id in tm.world().runtime().snapshot_signals().signals() {
        absorb(id.raw());
    }
    let journal = tm.source_journal();
    absorb(journal.len() as u64);
    absorb(journal.generation());
    format!("{hash:016x}")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::{ArenaProbe, NullProbe};
    use super::*;

    fn session() -> DevSession {
        DevSession::new(Box::new(NullProbe))
    }

    #[test]
    fn dispatch_rejects_missing_and_unknown_event_type() {
        let s = session();
        assert!(s.event_dispatch(&json!({})).is_err());
        assert!(s.event_dispatch(&json!({"event_type": 7})).is_err());
        let err = s
            .event_dispatch(&json!({"event_type": "quantum_flick"}))
            .expect_err("rejected");
        assert_eq!(err, "invalid param: unknown event_type `quantum_flick`");
        // Rejected input must not touch the ledger.
        let v = s.event_ledger(&json!({})).expect("ledger");
        assert_eq!(v["total"], 0);
    }

    #[test]
    fn event_ledger_empty_is_empty_page() {
        let s = session();
        let v = s.event_ledger(&json!({})).expect("event_ledger");
        assert_eq!(v["events"], json!([]));
        assert_eq!(v["total"], 0);
    }

    #[test]
    fn dispatch_records_attempt_and_propagates_probe_error() {
        let s = session(); // NullProbe → not_implemented
        let err = s
            .event_dispatch(&json!({"event_type": "pointer_click", "position": [10.0, 20.0]}))
            .expect_err("NullProbe cannot dispatch");
        assert!(err.starts_with("not_implemented:"), "{err}");
        // The attempt is still recorded in the ledger.
        let v = s.event_ledger(&json!({})).expect("ledger");
        assert_eq!(v["total"], 1);
        let rec = &v["events"][0];
        assert_eq!(rec["seq"], 1);
        assert_eq!(rec["kind"], "Pointer");
        assert_eq!(rec["event_type"], "pointer");
        assert_eq!(rec["disposition"], "Ignored");
        assert_eq!(rec["position"], json!([10.0, 20.0]));
        assert!(rec["timestamp"].is_string());
    }

    #[test]
    fn dispatch_merges_seq_into_probe_result() {
        struct Echo;
        impl ArenaProbe for Echo {
            fn probe_dispatch_event(&mut self, kind: &str, _params: &Value) -> SessionResult {
                assert_eq!(kind, "key_press");
                Ok(json!({"response": "Handled", "hit_path": [3, 7]}))
            }
        }
        let s = DevSession::new(Box::new(Echo));
        let v = s
            .event_dispatch(&json!({"event_type": "key_press", "key": "Enter"}))
            .expect("dispatch");
        assert_eq!(v["seq"], 1);
        assert_eq!(v["event_type"], "key_press");
        assert_eq!(v["response"], "Handled");
        assert_eq!(v["hit_path"], json!([3, 7]));
        assert_eq!(s.event_ledger(&json!({})).expect("ledger")["total"], 1);
    }

    #[test]
    fn event_ledger_tails_with_limit() {
        let s = session();
        for _ in 0..3 {
            let _ = s.event_dispatch(&json!({"event_type": "scroll"}));
        }
        let v = s.event_ledger(&json!({"limit": 2})).expect("ledger");
        assert_eq!(v["total"], 3);
        let ev = v["events"].as_array().expect("array");
        assert_eq!(ev.len(), 2);
        assert_eq!(ev[0]["seq"], 2);
        assert_eq!(ev[1]["seq"], 3);
    }

    #[test]
    fn timemachine_rejects_bad_params() {
        let s = session();
        assert!(s.timemachine_step(&json!({})).is_err());
        let err = s
            .timemachine_step(&json!({"action": "rewind_universe"}))
            .expect_err("rejected");
        assert!(err.starts_with("invalid param:"), "{err}");
        let err = s
            .timemachine_step(&json!({"action": "restore_checkpoint"}))
            .expect_err("rejected");
        assert!(err.starts_with("invalid param:"), "{err}");
    }

    #[test]
    fn timemachine_unavailable_without_machine() {
        let s = session();
        let err = s
            .timemachine_step(&json!({"action": "pause"}))
            .expect_err("unavailable");
        assert_eq!(err, TIMEMACHINE_UNAVAILABLE);
    }

    #[cfg(feature = "devtools-timemachine")]
    #[test]
    fn timemachine_step_roundtrips_checkpoint() {
        use crate::timemachine::{TimeMachine, World};
        use martensite_reactive::ReactiveRuntime;

        let s = DevSession::new(Box::new(NullProbe)).with_timemachine(TimeMachine::new(
            World::new(Default::default(), ReactiveRuntime::new()),
        ));

        let v = s
            .timemachine_step(&json!({"action": "pause"}))
            .expect("pause");
        assert_eq!(v["action"], "pause");
        assert_eq!(v["frame_timestamp"], 0);
        assert!(v["arena_fingerprint"].is_u64());
        assert!(v["signal_state_hash"].is_string());
        let id = v["checkpoint_id"].as_u64().expect("checkpoint id");

        // Nothing to undo at the root.
        assert!(s
            .timemachine_step(&json!({"action": "step_backward"}))
            .is_err());

        let v = s
            .timemachine_step(&json!({"action": "restore_checkpoint", "checkpoint_id": id}))
            .expect("restore");
        assert_eq!(v["action"], "restore_checkpoint");
        assert_eq!(v["checkpoint_id"], id);

        // Ids that do not name a checkpoint are rejected honestly.
        let err = s
            .timemachine_step(
                &json!({"action": "restore_checkpoint", "checkpoint_id": 0xdead_beef_u64}),
            )
            .expect_err("no such checkpoint");
        assert!(err.starts_with("invalid param:"), "{err}");
    }
}
