//! Dev-session state backing the ADR-0038 dev channel.
//!
//! `DevSession` aggregates the devtools subsystems a running Martensite
//! application exposes to the dev channel (event ledger, tweak registry,
//! lint bridge, TimeMachine, reload counters) plus an [`ArenaProbe`] —
//! the application-provided bridge that answers queries which need live
//! widget-tree, layout, accessibility, paint, or signal state.
//!
//! Handlers in the sibling modules translate JSON-RPC params into session
//! calls and produce `serde_json::Value` results; `martensite-host`'s
//! `dev-channel` feature wires them onto `DevChannelHandler`.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::Value;

use crate::event_ledger::EventLedger;
use crate::lint_bridge::LintBridge;
#[cfg(feature = "devtools-timemachine")]
use crate::timemachine::TimeMachine;
use crate::tweak::TweakRegistry;

pub mod arena_probe;
pub mod errors;
pub mod events;
pub mod inspect;
pub mod layout_store;
pub mod log_ring;
pub mod signals;
pub mod tweaks;

/// Result type for dev-session handlers: wire-ready JSON or a string error.
pub type SessionResult = Result<Value, String>;

/// Application-provided bridge for queries that need live runtime state
/// (widget arena, Taffy layout, AccessKit, paint, signals, theme).
///
/// The app supplies a boxed implementation at [`DevSession`] construction.
/// Methods return `Err` with a `not_implemented:` prefix when the app does
/// not back a capability; callers may translate that to `-32601`.
pub trait ArenaProbe: Send {
    /// Full or subtree widget-tree snapshot (`tree_snapshot` params).
    fn probe_tree_snapshot(&mut self, params: &Value) -> SessionResult {
        let _ = params;
        Err(not_impl("tree_snapshot"))
    }

    /// Detailed single-node inspection.
    fn probe_node_detail(&mut self, node_id: u64) -> SessionResult {
        let _ = node_id;
        Err(not_impl("tree_node"))
    }

    /// Taffy constraint chain for a node.
    fn probe_layout_chain(&mut self, node_id: u64) -> SessionResult {
        let _ = node_id;
        Err(not_impl("layout_chain"))
    }

    /// Overflow/clipping scan (subtree when `node_id` is `Some`).
    fn probe_overflow_scan(&mut self, node_id: Option<u64>) -> SessionResult {
        let _ = node_id;
        Err(not_impl("overflow_scan"))
    }

    /// AccessKit semantic tree (subtree when `root_id` is `Some`).
    fn probe_a11y_tree(&mut self, params: &Value) -> SessionResult {
        let _ = params;
        Err(not_impl("a11y_tree"))
    }

    /// Paint-order audit of the current frame.
    fn probe_audit_paint(&mut self, node_id: Option<u64>) -> SessionResult {
        let _ = node_id;
        Err(not_impl("audit_paint"))
    }

    /// Rasterize a node subtree. Implementations should return
    /// `{ "bytes_base64": ..., "format": "png"|"jpeg",
    ///   "logical_size": [w,h], "physical_size": [w,h] }`.
    fn probe_capture_node(&mut self, node_id: u64, format: &str, scale: f64) -> SessionResult {
        let _ = (node_id, format, scale);
        Err(not_impl("capture_node"))
    }

    /// Inject a synthetic input event and report its response/hit path.
    fn probe_dispatch_event(&mut self, kind: &str, params: &Value) -> SessionResult {
        let _ = (kind, params);
        Err(not_impl("event_dispatch"))
    }

    /// Apply a signal write through the app's reactive runtime
    /// (tweak-gated, type-checked by the app).
    fn probe_signal_set(&mut self, signal_id: &str, value: &Value) -> SessionResult {
        let _ = (signal_id, value);
        Err(not_impl("signal_trigger"))
    }

    /// Enumerate registered reactive signals and dirty state.
    fn probe_signals_list(&mut self, params: &Value) -> SessionResult {
        let _ = params;
        Err(not_impl("signals_list"))
    }

    /// Apply a theme mode/token override; returns the effective state.
    fn probe_theme_apply(&mut self, params: &Value) -> SessionResult {
        let _ = params;
        Err(not_impl("theme_set"))
    }

    /// Current theme tokens (`theme_get`).
    fn probe_theme_tokens(&mut self) -> SessionResult {
        Err(not_impl("theme_get"))
    }

    /// Arm inspector select mode and return the clicked node.
    fn probe_inspector_select(&mut self, params: &Value) -> SessionResult {
        let _ = params;
        Err(not_impl("inspector_select"))
    }

    /// Dispatch an accessibility `SemanticAction` to a widget node.
    /// `params` carries `node_id` (optional — falls back to focus),
    /// `action` (snake_case `SemanticAction` name), `value`, `point`.
    fn probe_a11y_action(&mut self, params: &Value) -> SessionResult {
        let _ = params;
        Err(not_impl("a11y_action"))
    }

    /// Force or clear the node's `NodeFlags::LOADING` override
    /// (`node_set_loading`, ADR-0040). Implementations should mutate
    /// the real arena and return
    /// `{ "applied": true, "node_id": <u64>, "loading": <effective> }`
    /// — `loading` reports `WidgetArena::node_loading` (override OR
    /// widget-declared), not just the requested override. The session
    /// stamps the mutation `revision`.
    fn probe_node_set_loading(&mut self, node_id: u64, loading: bool) -> SessionResult {
        let _ = (node_id, loading);
        Err(not_impl("node_set_loading"))
    }

    /// Attaches the app's `FocusManager` so `key_press`/`text_input`
    /// dispatch and `a11y_action`'s focused-node fallback can resolve
    /// targets. Default no-op — probes without a focus source decline
    /// those paths honestly.
    fn set_focus_manager(&mut self, _focus: Arc<Mutex<martensite_focus::FocusManager>>) {}
}

/// Marker on probe errors that map to JSON-RPC `-32601` (not implemented).
pub fn not_impl(method: &str) -> String {
    format!("not_implemented:{method}")
}

/// `true` when a probe/session error means "capability absent" (-32601).
pub fn is_not_implemented(err: &str) -> bool {
    err.starts_with("not_implemented:")
}

/// Default probe for sessions without app backing (tests, offline mocks).
#[derive(Debug, Default)]
pub struct NullProbe;

impl ArenaProbe for NullProbe {}

pub use arena_probe::WidgetArenaProbe;

/// Hot-reload bookkeeping surfaced by `reload_status`.
#[derive(Debug, Clone, Default)]
pub struct ReloadStats {
    /// Identifier of the currently active guest build, if any.
    pub active_build_id: Option<String>,
    /// Timestamp (RFC 3339) of the last completed reload.
    pub last_reload_timestamp: Option<String>,
    /// Number of completed reloads this session.
    pub total_reloads: u64,
    /// Diagnostics emitted by the most recent guest build.
    pub compiler_diagnostics: Vec<String>,
    /// What state survived the last reload.
    pub state_preservation_report: Option<String>,
}

/// Shared dev-session state serviced by the dev-channel handlers.
///
/// Construct once per running app (debug/dev builds only — ADR-0039 D8:
/// release builds must not link this machinery) and hand it to
/// `SessionDevChannelHandler` in `martensite-host`.
pub struct DevSession {
    /// Hybrid event ledger (raw input + resolved intents + hit rejection).
    pub ledger: Mutex<EventLedger>,
    /// Live tweak registry (parameter overrides + source write-back).
    pub tweaks: Mutex<TweakRegistry>,
    /// Per-frame lint scene/report cache produced by [`LintBridge`].
    pub lint: Mutex<LintBridge>,
    /// TimeMachine replay state (requires `devtools-timemachine`).
    #[cfg(feature = "devtools-timemachine")]
    pub machine: Mutex<Option<TimeMachine>>,
    /// Application-provided runtime bridge.
    pub probe: Mutex<Box<dyn ArenaProbe>>,
    /// Monotonic tweak revision for optimistic write-back locking.
    pub revision: AtomicU64,
    /// Hot-reload counters for `reload_status`.
    pub reload: Mutex<ReloadStats>,
    /// Highest source-ledger seq already copied by
    /// [`absorb_events`](Self::absorb_events).
    absorbed_watermark: AtomicU64,
    /// Optional attached [`crate::error_surface::ErrorSurface`] feeding
    /// `runtime_errors` with layout/lint/paint diagnostics.
    error_surface: Mutex<Option<crate::error_surface::ErrorSurface>>,
    /// Optional bounded tracing ring buffer backing `logs`.
    log_ring: Mutex<Option<Arc<log_ring::LogRing>>>,
    /// Registered JSON signal adapters making type-erased signals
    /// writable via `signal_set` / inspectable with values in
    /// `signals_list`.
    signal_adapters: Mutex<Vec<SignalAdapter>>,
    /// Path of the reload-request marker file `hot_reload` writes; the
    /// dev coordinator (`cargo martensite dev`) polls for it. Set by
    /// `serve_dev_session` when the socket is bound.
    reload_request_path: Mutex<Option<std::path::PathBuf>>,
}

/// JSON read-back codec for a [`SignalAdapter`]: returns the current
/// value, or `None` when it cannot be represented.
pub type SignalReadJson = Box<dyn Fn() -> Option<Value> + Send + Sync>;

/// JSON write codec for a [`SignalAdapter`]: validates and applies the
/// value, returning an error string on shape/type mismatch.
pub type SignalWriteJson = Box<dyn Fn(&Value) -> Result<(), String> + Send + Sync>;

/// Registered adapter giving the dev channel JSON access to a
/// type-erased reactive signal.
///
/// `Signal::set` is generic over `T`, so a running signal cannot be
/// written from a JSON payload without a per-type codec. Apps (or
/// `#[tweak]` macro expansions) register an adapter per signal they want
/// the channel to mutate; unregistered signals stay inspectable but read-
/// only.
pub struct SignalAdapter {
    /// Human-facing signal name (`"counter"`, `"theme.accent"`).
    pub name: String,
    /// Raw signal id when known (matches `SignalId` raw `u64`).
    pub signal_id: Option<u64>,
    /// Serializes the current value to JSON.
    pub read_json: SignalReadJson,
    /// Validates and applies a JSON value; returns an error string on
    /// shape/type mismatch.
    pub write_json: SignalWriteJson,
}

impl std::fmt::Debug for SignalAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SignalAdapter")
            .field("name", &self.name)
            .field("signal_id", &self.signal_id)
            .finish_non_exhaustive()
    }
}

impl DevSession {
    /// Builds a session around an app-provided [`ArenaProbe`].
    #[must_use]
    pub fn new(probe: Box<dyn ArenaProbe>) -> Self {
        Self {
            ledger: Mutex::new(EventLedger::new()),
            tweaks: Mutex::new(TweakRegistry::new()),
            lint: Mutex::new(LintBridge::default()),
            #[cfg(feature = "devtools-timemachine")]
            machine: Mutex::new(None),
            probe: Mutex::new(probe),
            revision: AtomicU64::new(0),
            reload: Mutex::new(ReloadStats::default()),
            absorbed_watermark: AtomicU64::new(0),
            error_surface: Mutex::new(None),
            log_ring: Mutex::new(None),
            signal_adapters: Mutex::new(Vec::new()),
            reload_request_path: Mutex::new(None),
        }
    }

    /// Builds a session over a shared [`martensite_core::WidgetArena`]
    /// using [`WidgetArenaProbe`] — the standard app-side wiring:
    ///
    /// ```no_run
    /// use std::sync::{Arc, Mutex};
    ///
    /// use martensite_core::WidgetArena;
    /// use martensite_devtools::dev_session::DevSession;
    ///
    /// // `arena` is the app's live arena — the probe reads/mutates it
    /// // through the shared mutex.
    /// let arena = Arc::new(Mutex::new(WidgetArena::new()));
    /// let session = Arc::new(DevSession::with_arena(Arc::clone(&arena)));
    /// // `session.serve(...)` — pass to `martensite_host::serve_dev_session`.
    /// ```
    ///
    /// The app should additionally call [`on_frame`](Self::on_frame)
    /// once per frame, [`absorb_events`](Self::absorb_events) with its
    /// `EventRouter` ledger, and [`record_reload`](Self::record_reload)
    /// after each hot-reload to keep lint/event/reload surfaces live.
    #[must_use]
    pub fn with_arena(arena: Arc<Mutex<martensite_core::WidgetArena>>) -> Self {
        Self::new(Box::new(WidgetArenaProbe::new(arena)))
    }

    /// Attaches a [`TimeMachine`] for deterministic replay tooling.
    #[cfg(feature = "devtools-timemachine")]
    pub fn with_timemachine(self, machine: TimeMachine) -> Self {
        *self.machine.lock().expect("timemachine mutex") = Some(machine);
        self
    }

    /// Current tweak revision (optimistic-lock token for `tweaks_sync`).
    pub fn current_revision(&self) -> u64 {
        self.revision.load(Ordering::Acquire)
    }

    /// Bumps the tweak revision and returns the new value.
    pub fn bump_revision(&self) -> u64 {
        self.revision.fetch_add(1, Ordering::AcqRel) + 1
    }

    /// Records a completed reload (build id + diagnostics).
    pub fn record_reload(&self, build_id: impl Into<String>) {
        let mut stats = self.reload.lock().expect("reload mutex");
        stats.active_build_id = Some(build_id.into());
        stats.total_reloads += 1;
        stats.last_reload_timestamp = Some(crate::dev_session::iso_now());
    }

    /// Feeds a finished frame into the lint bridge (call once per frame).
    pub fn on_frame(&self, list: &martensite_core::PaintList) {
        self.lint.lock().expect("lint mutex").on_frame(list);
    }

    /// Merges records from an external ledger — e.g. the `EventRouter`'s
    /// own `EventLedger` — into the session ledger, so `event_ledger`
    /// reflects real routed input as well as MCP-injected events.
    ///
    /// Call once per frame next to [`on_frame`](Self::on_frame). Records
    /// are copied with their source seqs preserved (see
    /// [`EventLedger::push`]); only records newer than the last absorbed
    /// seq are merged, so per-frame calls cost O(new events). Returns the
    /// number of records merged.
    pub fn absorb_events(&self, source: &EventLedger) -> usize {
        let watermark = self.absorbed_watermark.load(Ordering::Acquire);
        let mut max_seen = watermark;
        let mut merged = 0usize;
        {
            let mut ledger = self.ledger.lock().expect("ledger mutex");
            for record in source.iter() {
                if record.seq > watermark {
                    ledger.push(*record);
                    max_seen = max_seen.max(record.seq);
                    merged += 1;
                }
            }
        }
        self.absorbed_watermark.store(max_seen, Ordering::Release);
        merged
    }

    /// Attaches an [`crate::error_surface::ErrorSurface`] so
    /// `runtime_errors` can report live diagnostics collected on the app
    /// side (layout overflow tapes, lint findings, paint errors).
    pub fn set_error_surface(&self, surface: crate::error_surface::ErrorSurface) {
        *self.error_surface.lock().expect("error_surface mutex") = Some(surface);
    }

    /// Shared access to the attached error surface, if any.
    pub fn error_surface(
        &self,
    ) -> std::sync::MutexGuard<'_, Option<crate::error_surface::ErrorSurface>> {
        self.error_surface.lock().expect("error_surface mutex")
    }

    /// Attaches the session's bounded tracing ring buffer. Apps install
    /// the layer once at startup via
    /// [`log_ring::LogRing::layer`]-equivalent wiring and pass the shared
    /// handle here so `logs` can drain it.
    pub fn set_log_ring(&self, ring: Arc<log_ring::LogRing>) {
        *self.log_ring.lock().expect("log_ring mutex") = Some(ring);
    }

    /// Shared access to the attached log ring, if any.
    pub fn log_ring(&self) -> Option<Arc<log_ring::LogRing>> {
        self.log_ring.lock().expect("log_ring mutex").clone()
    }

    /// Attaches the app's [`martensite_focus::FocusManager`] to the
    /// probe so `key_press`/`text_input` dispatch can resolve the
    /// focused target and `a11y_action` gains its focused-node
    /// fallback. Probes without a focus source ignore this.
    pub fn attach_focus_manager(&self, focus: Arc<Mutex<martensite_focus::FocusManager>>) {
        self.probe
            .lock()
            .expect("probe mutex")
            .set_focus_manager(focus);
    }

    /// Registers a JSON [`SignalAdapter`] making a type-erased reactive
    /// signal writable via `signal_set` and value-inspectable via
    /// `signals_list`. Adapters are matched by `name` first, then
    /// `signal_id`.
    pub fn register_signal_adapter(&self, adapter: SignalAdapter) {
        self.signal_adapters
            .lock()
            .expect("signal_adapters mutex")
            .push(adapter);
    }

    /// Read access to registered signal adapters (locked guard — adapters
    /// hold boxed closures, so callers must not hold this across probe or
    /// runtime calls).
    pub fn signal_adapters(&self) -> std::sync::MutexGuard<'_, Vec<SignalAdapter>> {
        self.signal_adapters.lock().expect("signal_adapters mutex")
    }

    /// Sets the reload-request marker path the `hot_reload` handler
    /// writes. Called by `serve_dev_session` once the socket path is
    /// known (marker lives next to the socket as `<name>.reload-request`).
    pub fn set_reload_request_path(&self, path: std::path::PathBuf) {
        *self.reload_request_path.lock().expect("reload path mutex") = Some(path);
    }

    /// The reload-request marker path, if the session was served with
    /// hot-reload triggering enabled.
    pub fn reload_request_path(&self) -> Option<std::path::PathBuf> {
        self.reload_request_path
            .lock()
            .expect("reload path mutex")
            .clone()
    }
}

/// RFC 3339 UTC timestamp without adding a chrono dependency.
pub(crate) fn iso_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = secs / 86_400;
    let sod = secs % 86_400;
    let (y, m, d) = days_to_ymd(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        sod / 3600,
        (sod % 3600) / 60,
        sod % 60
    )
}

fn days_to_ymd(days_since_epoch: u64) -> (i64, u32, u32) {
    // Howard Hinnant's civil-from-days algorithm.
    let z = days_since_epoch as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}
