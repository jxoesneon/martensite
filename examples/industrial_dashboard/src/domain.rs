//! The simulated-plant domain model — the backend every dashboard
//! widget binds to. The Design Council's bind-or-cut rule (docket
//! `20260921`) requires each mounted widget to read real model state
//! and publish user interaction back into it; within the dashboard
//! fiction the simulation *is* the backend, so comms widgets loop back
//! into the shift log, the audio suite reads an acoustic-condition
//! model, security inputs guard the console lock, and the jog pad
//! drives a simulated axis.
//!
//! Everything hangs off [`PlantModel`], a bag of `Signal`-backed
//! stores seeded deterministically. Widgets follow the poll/reconcile
//! pattern the Toolbar already uses: the host panel calls
//! `reconcile()` to drain widget interaction (`take_*`/getters) into
//! signals, then `publish()` to push model state back into views.

use martensite::reactive::Signal;

// ---------------------------------------------------------------------------
// Assets — site → line → cell hierarchy with real fields. Feeds
// TreeView/Cascader/TreeSelect (navigation), PropertyGrid/Inspector/
// Descriptions (detail), QrCode/Barcode (identity), and the derived
// hierarchy datasets (Sunburst/OrgChart/GraphView).
// ---------------------------------------------------------------------------

/// A node in the plant hierarchy: site → line → cell.
#[derive(Clone, Debug, PartialEq)]
pub struct Asset {
    pub id: u32,
    /// `None` for sites; lines point at their site, cells at their line.
    pub parent: Option<u32>,
    pub kind: AssetKind,
    pub name: &'static str,
    pub status: AssetStatus,
    /// Overall equipment effectiveness for this node (0..1).
    pub oee: f64,
    /// Serial number — rendered by QrCode/Barcode in the identity tab.
    pub serial: &'static str,
    /// Install year — feeds the property grid / inspector.
    pub installed: u16,
    /// Free-text operator note — editable through InlineEdit/TextArea.
    pub note: &'static str,
    /// Health index 0..1 — sim-owned write (degraded assets trend
    /// down); the DETAIL rail's Rating displays it read-only.
    pub health: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssetKind {
    Site,
    Line,
    Cell,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssetStatus {
    Running,
    Degraded,
    Down,
    Maintenance,
}

impl AssetStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Running => "Running",
            Self::Degraded => "Degraded",
            Self::Down => "Down",
            Self::Maintenance => "Maintenance",
        }
    }
}

// ---------------------------------------------------------------------------
// Work orders — feeds Kanban (status columns), Table/TreeTable/ListView
// (registers), the editor inspector form (edits `selected_wo`),
// Checklist/Transfer (task ops), and Ticket (WO card).
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct WorkOrder {
    pub id: u32,
    pub title: &'static str,
    pub asset: u32,
    pub status: WoStatus,
    pub priority: WoPriority,
    /// Crew index into `PlantModel::crew`.
    pub assignee: usize,
    /// Checklist items as (label, done) — Checklist binds to this.
    pub checklist: Vec<(&'static str, bool)>,
    /// Scheduled day-of-week index 0=Mon — Calendar/DatePicker read it.
    pub due_day: u8,
    /// Percent complete — ProgressBar/Rating reflect it.
    pub progress: f64,
    /// Operator notes — TextArea/Markdown edit this.
    pub notes: String,
    /// Completion sign-off — the SIGN-OFF page writes it (typed name
    /// or an ink-canvas stroke record).
    pub signature: Option<Signature>,
    /// Captured evidence photo id — the CropBox capture sink.
    pub photo: Option<String>,
}

/// A work-order sign-off — typed name or ink stroke.
#[derive(Clone, Debug, PartialEq)]
pub enum Signature {
    Typed(String),
    Ink,
}

impl Signature {
    pub fn label(&self) -> &str {
        match self {
            Self::Typed(name) => name.as_str(),
            Self::Ink => "(signed)",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WoStatus {
    Queued,
    InProgress,
    Review,
    Done,
}

impl WoStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Queued => "Queued",
            Self::InProgress => "In progress",
            Self::Review => "Review",
            Self::Done => "Done",
        }
    }
    /// Column order for the Kanban board.
    pub fn columns() -> [WoStatus; 4] {
        [Self::Queued, Self::InProgress, Self::Review, Self::Done]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WoPriority {
    Low,
    Medium,
    High,
    Critical,
}

impl WoPriority {
    pub fn label(self) -> &'static str {
        match self {
            Self::Low => "Low",
            Self::Medium => "Medium",
            Self::High => "High",
            Self::Critical => "Critical",
        }
    }
}

// ---------------------------------------------------------------------------
// Maintenance schedule — feeds Gantt, WeekView, Calendar, Timeline,
// Milestone, DatePicker/TimePicker (scheduling writes back).
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct MaintTask {
    pub id: u32,
    pub title: &'static str,
    pub asset: u32,
    /// Start day offset from "today" (0..13 — a two-week window).
    pub start_day: u8,
    /// Duration in days.
    pub days: u8,
    pub done: bool,
    /// Crew index — the schedule's swimlane.
    pub crew: usize,
    /// Percent complete 0..1 — the MAINTENANCE task rail shows and
    /// the ProgressBar reflects it.
    pub progress: f64,
}

// ---------------------------------------------------------------------------
// Shift log — the comms backend. ChatInput/MessageComposer append;
// MessageList/CommentThread render; Announcements/Banner surface the
// pinned entry. Every comms widget is a real view/editor of this log.
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct LogEntry {
    /// Monotonic append id — survives `LOG_CAP` front-drains, so
    /// consumers key cursors by id rather than positional index.
    pub id: u64,
    /// Minutes since shift start (the simulated clock).
    pub minute: u32,
    /// Crew index, or `usize::MAX` for the system annunciator.
    pub author: usize,
    pub text: String,
    /// Pinned entries surface in the Announcements/Banner zone.
    pub pinned: bool,
    /// Channel the entry was posted to — COMMS filters by
    /// `channel_sel`; DIAGNOSTICS tails `System`.
    pub channel: LogChannel,
    /// Work order this entry discusses (`None` = general) — the
    /// WORK ORDERS CommentThread filters on it.
    pub wo: Option<u32>,
}

/// Shift-log channel — ops radio chatter vs. crew chat vs. system
/// annunciator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogChannel {
    Ops,
    Comms,
    System,
}

impl LogChannel {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ops => "OPS",
            Self::Comms => "COMMS",
            Self::System => "SYSTEM",
        }
    }
    /// Channel index order for `channel_sel` Segmented strips.
    pub fn all() -> [LogChannel; 3] {
        [Self::Ops, Self::Comms, Self::System]
    }
}

// ---------------------------------------------------------------------------
// Alarms — feeds AlarmPanel, Banner, ChipGroup (severity filter),
// EmptyState (all-clear), StackLight/StatusDot, and the distribution
// datasets (Pie/Treemap of alarms-by-line).
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct Alarm {
    pub id: u32,
    pub asset: u32,
    pub severity: AlarmSeverity,
    pub message: &'static str,
    pub active: bool,
    pub acked: bool,
    /// Minutes since shift start when the alarm raised.
    pub raised_min: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum AlarmSeverity {
    Info,
    Warning,
    Critical,
}

impl AlarmSeverity {
    pub fn label(self) -> &'static str {
        match self {
            Self::Info => "Info",
            Self::Warning => "Warning",
            Self::Critical => "Critical",
        }
    }
}

// ---------------------------------------------------------------------------
// Crew — feeds Avatar/AvatarGroup, Presence, AttendeeList, OrgChart,
// ConferenceGrid/WaitingRoom/BreakoutRooms, Poll, Mention targets.
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct CrewMember {
    pub name: &'static str,
    pub role: &'static str,
    /// Presence state driving the Presence/AttendeeList widgets.
    pub presence: Presence,
    /// Manager index into the same vec (OrgChart edges).
    pub reports_to: Option<usize>,
    /// Which simulated "room" they're in — ConferenceGrid/BreakoutRooms.
    pub room: Room,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Presence {
    OnShift,
    Remote,
    Break,
    OffShift,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Room {
    Floor,
    Control,
    BreakoutA,
    BreakoutB,
    Offsite,
}

// ---------------------------------------------------------------------------
// Acoustic condition monitoring — the audio suite's backend. A real
// plant function: ultrasonic/vibration signatures per line. The model
// synthesizes spectrum bands + waveform from line state; Equalizer,
// Spectrum, Waveform, VU Meter, Tuner, Dial all read/write it.
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct AcousticModel {
    /// 16-band spectrum magnitudes 0..1 — Spectrum/Equalizer read this;
    /// Equalizer writes band trims back (a real "monitor EQ" function).
    pub bands: [f64; 16],
    /// Stereo level 0..1 — VU Meter/AudioLevelMeter.
    pub level: (f64, f64),
    /// Dominant frequency Hz — Tuner displays it.
    pub dominant_hz: f64,
    /// Alarm-tone program: note index + step pattern + tempo — the
    /// PianoKeys/Fretboard/StepSequencer/Metronome suite edits the
    /// annunciator's alert sequence (real: plants program alarm tones).
    pub tone_note: u8,
    pub tone_pattern: [bool; 8],
    pub tone_bpm: u16,
}

// ---------------------------------------------------------------------------
// Jog / PTZ — the motion widgets' backend. Joystick/XYPad write the
// axis target; Tilt writes camera tilt; ZoomControls write zoom; the
// Grid's cell-camera view renders from it.
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default)]
pub struct JogState {
    /// Axis offset −1..1 on both axes (Joystick/XYPad write, view reads).
    pub axis: (f64, f64),
    /// Camera tilt −1..1 (Tilt widget).
    pub tilt: f64,
    /// Zoom factor 0.5..4 (ZoomControls).
    pub zoom: f64,
}

// ---------------------------------------------------------------------------
// Documents — the DOCUMENTS page's store. Manuals, logs, and reports
// attached to assets; the left ListView selects, viewers render.
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct DocEntry {
    pub id: u32,
    pub title: &'static str,
    /// Owning asset (`None` = site-wide document).
    pub asset: Option<u32>,
    pub kind: DocKind,
    /// Structured body — JSON viewer/tree renders it.
    pub body: &'static str,
    /// Raw payload — HexView renders it.
    pub bytes: &'static [u8],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocKind {
    Manual,
    Report,
    Firmware,
    Log,
}

impl DocKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Manual => "Manual",
            Self::Report => "Report",
            Self::Firmware => "Firmware",
            Self::Log => "Log",
        }
    }
}

// ---------------------------------------------------------------------------
// Recordings — camera-loop archive the TRANSPORT page scrubs. The
// CAMERAS record verb appends; MediaView/SeekBar/Filmstrip read.
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct Recording {
    pub id: u32,
    pub title: &'static str,
    /// Camera index in the zone's camera set.
    pub camera: usize,
    /// Length in seconds.
    pub secs: u32,
}

// ---------------------------------------------------------------------------
// Command registry — every console verb (CommandPalette, Terminal,
// launcher buttons) resolves through this one list, so CHROME can
// preview exactly what each chrome exposes.
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct CommandSpec {
    pub id: &'static str,
    pub label: &'static str,
    /// Chromes that surface it — bit flags (1=palette 2=terminal
    /// 4=context 8=menubar 16=radial 32=taskswitcher).
    pub chromes: u8,
}

// ---------------------------------------------------------------------------
// The model.
// ---------------------------------------------------------------------------

/// The whole simulated plant. Signals make every store observable:
/// widget interaction writes a signal, every other bound widget sees
/// it on the next publish pass — the loop is the dogfood. `Clone` is
/// cheap (all fields are `Signal`s) — every `Bound` widget holds a
/// copy for its pull/push closures.
#[derive(Clone)]
pub struct PlantModel {
    // --- existing live signals (shared with app/toolbar) ---
    pub cpu: Signal<f64>,
    pub mem: Signal<f64>,
    pub paused: Signal<bool>,
    pub alerts_on: Signal<bool>,
    pub filter_text: Signal<String>,

    // --- domain stores ---
    pub assets: Signal<Vec<Asset>>,
    /// Currently inspected asset — set by TreeView/Cascader/Table row,
    /// read by every detail-zone widget.
    pub selected_asset: Signal<Option<u32>>,
    /// Site filter from the command-row Dropdown ("" = all sites).
    pub site_filter: Signal<String>,

    pub work_orders: Signal<Vec<WorkOrder>>,
    pub selected_wo: Signal<Option<u32>>,

    pub schedule: Signal<Vec<MaintTask>>,

    pub shift_log: Signal<Vec<LogEntry>>,
    /// Next [`LogEntry::id`] — monotonic so consumers key cursors by
    /// id and survive `LOG_CAP` front-drains.
    pub log_seq: std::sync::Arc<std::sync::atomic::AtomicU64>,
    /// Simulated clock: minutes since shift start (ticks the model).
    pub shift_minute: Signal<u32>,

    pub alarms: Signal<Vec<Alarm>>,
    /// Severity filter from ChipGroup (bitmask: 1=info 2=warn 4=crit).
    pub alarm_filter: Signal<u8>,

    pub crew: Signal<Vec<CrewMember>>,

    // --- functional state ---
    /// Console lock — PatternLock/OTP/Keypad unlock the HMI (real).
    pub console_locked: Signal<bool>,
    /// Jog/PTZ state — Joystick/XYPad/Tilt/Zoom write, camera reads.
    pub jog: Signal<JogState>,
    /// Acoustic monitor — line-noise analysis the audio suite edits.
    pub acoustic: Signal<AcousticModel>,
    /// Editor config — font size, autosave, wrap, theme accent the
    /// inspector pickers genuinely control.
    pub editor_font_pt: Signal<f64>,
    pub editor_autosave: Signal<bool>,
    pub editor_wrap: Signal<bool>,
    pub editor_accent: Signal<[u8; 4]>,
    /// Line running state — drives the acoustic model + stack light.
    pub line_running: Signal<bool>,
    /// First-run flag — the Tour widget's real trigger (persisted).
    pub tour_seen: Signal<bool>,
    /// Reduced-motion flag — every animated widget consults it
    /// (Inclusion requirement; a real HMI accessibility setting).
    pub reduced_motion: Signal<bool>,

    // --- telemetry history ---
    /// Rolling cpu/mem history (0..1 each, newest last, capped at
    /// [`HISTORY_LEN`]) — every trend chart's series source.
    pub cpu_hist: Signal<Vec<f64>>,
    pub mem_hist: Signal<Vec<f64>>,
    /// Bumped every `push_history` — the strictly-correct signature
    /// for history-driven gates (a bit-identical tail sample would
    /// fool a `(len, last)` key on a full ring).
    pub hist_rev: Signal<u64>,
    /// Material flow between lines: (from asset, to asset, tons/hr) —
    /// Sankey/GraphView/ChordDiagram's real dataset.
    pub material_flow: Signal<Vec<(u32, u32, f64)>>,

    // --- media transport (loopback monitor) ---
    /// Playback state of the comms/camera loopback monitor — transport
    /// widgets write it, MediaView/SeekBar read it.
    pub media_playing: Signal<bool>,
    /// Playhead position 0..1 through the recorded loop.
    pub media_pos: Signal<f64>,
    /// Monitor gain 0..1 — VolumeSlider writes, VU scales.
    pub media_vol: Signal<f64>,
    /// Camera index the PTZ/jog controls target (0-based).
    pub camera_sel: Signal<usize>,

    // --- comms poll ---
    /// Votes for the standing crew poll ("approve the Saturday
    /// maintenance window?") — Poll widget binds: [yes, no, abstain].
    pub poll_votes: Signal<[u32; 3]>,

    // --- documents / selections / navigation (dashboard grammar Δ3) ---
    pub docs: Signal<Vec<DocEntry>>,
    pub selected_doc: Signal<Option<u32>>,
    /// Selected maintenance task — MAINTENANCE writes, SCHEDULE reads.
    pub selected_task: Signal<Option<u32>>,
    /// Selected alarm — ALARMS board writes, its rail details.
    pub selected_alarm: Signal<Option<u32>>,
    /// Selected crew member — CREW/ROOMS write, rails detail.
    pub selected_member: Signal<Option<usize>>,
    /// Camera-loop archive + playhead selection (TRANSPORT).
    pub recordings: Signal<Vec<Recording>>,
    pub selected_recording: Signal<Option<u32>>,
    /// Console verbs — CommandPalette/Terminal/CHROME preview share it.
    pub commands: Signal<Vec<CommandSpec>>,
    /// Cross-zone navigation requests — an artifact action writes
    /// `page_request[zone] = Some(page)`; `ZonePanel::tick` drains
    /// (take) and activates the tab. Cleared on consume so rebuilds
    /// never re-fire stale requests.
    pub page_request: Signal<[Option<u8>; 4]>,
    /// Deep-link sub-target — `(page, inner view)` a `page_request`
    /// should land on: the page's own index plus its inner
    /// Swap/Segmented index (REGISTRY's List view, DETAIL's Inspector
    /// dossier, DOCUMENTS' artifact). Written by
    /// [`PlantModel::request_page_deep`]; drained (take) by the
    /// activated page's strip binding, which only ticks while the
    /// page is visible — so a sub lands exactly once, post-activation.
    /// The page index rides along so a still-visible page can never
    /// consume a sub meant for its sibling. A plain `request_page`
    /// clears a pending sub for its zone.
    pub page_request_sub: Signal<[Option<(u8, u8)>; 4]>,
    /// Zone content width in pt — one slot per zone so a panel's
    /// publish can never bleed into a sibling zone's pages (the
    /// single shared signal was last-writer-wins across all four
    /// panels). `ZonePanel::layout` writes `zone_width[zone_index]`;
    /// `Page` reads its zone's slot for rail collapse breakpoints.
    pub zone_width: [Signal<f32>; 4],
    /// Shell toast inbox — any zone enqueues a `Toast`; the
    /// `ShellOverlays` owner drains it into the viewport-anchored
    /// `ToastHost`. Toasts are chrome, not page layout.
    pub toast_inbox: crate::overlays::ToastInbox,

    // --- page-scoped presentation selections (not persisted) ---
    /// COMMS channel Segmented — filters the message list.
    pub channel_sel: Signal<u8>,
    /// CHROME chrome Segmented — which chrome the preview details.
    pub active_chrome: Signal<u8>,
    /// APPEARANCE left-nav — which picker section the rail shows.
    pub editor_section: Signal<u8>,
}

/// Ring-buffer length for the telemetry history signals.
pub const HISTORY_LEN: usize = 240;

/// Shift-log retention — a full shift of per-minute chatter plus
/// headroom; keeps `shift_log` readers' per-tick clones bounded.
pub const LOG_CAP: usize = 480;

impl PlantModel {
    /// Seeds the whole plant deterministically — the same fictional
    /// facility every run so tests and screenshots are stable.
    pub fn seeded(
        cpu: Signal<f64>,
        mem: Signal<f64>,
        paused: Signal<bool>,
        alerts_on: Signal<bool>,
        filter_text: Signal<String>,
    ) -> Self {
        let shift_log = seed_shift_log();
        let log_seq =
            std::sync::Arc::new(std::sync::atomic::AtomicU64::new(shift_log.len() as u64));
        Self {
            cpu,
            mem,
            paused,
            alerts_on,
            filter_text,
            assets: Signal::new(seed_assets()),
            selected_asset: Signal::new(Some(7)), // Robot-Arm-K7
            site_filter: Signal::new(String::new()),
            work_orders: Signal::new(seed_work_orders()),
            selected_wo: Signal::new(Some(4471)),
            schedule: Signal::new(seed_schedule()),
            shift_log: Signal::new(shift_log),
            log_seq,
            shift_minute: Signal::new(217),
            alarms: Signal::new(seed_alarms()),
            alarm_filter: Signal::new(0b111),
            crew: Signal::new(seed_crew()),
            console_locked: Signal::new(false),
            jog: Signal::new(JogState::default()),
            acoustic: Signal::new(seed_acoustic()),
            editor_font_pt: Signal::new(13.0),
            editor_autosave: Signal::new(true),
            editor_wrap: Signal::new(false),
            editor_accent: Signal::new([96, 165, 250, 255]),
            line_running: Signal::new(true),
            tour_seen: Signal::new(false),
            reduced_motion: Signal::new(false),
            cpu_hist: Signal::new(vec![0.35; HISTORY_LEN]),
            mem_hist: Signal::new(vec![0.55; HISTORY_LEN]),
            hist_rev: Signal::new(0),
            material_flow: Signal::new(seed_flow()),
            media_playing: Signal::new(false),
            media_pos: Signal::new(0.0),
            media_vol: Signal::new(0.8),
            camera_sel: Signal::new(0),
            poll_votes: Signal::new([4, 1, 2]),
            docs: Signal::new(seed_docs()),
            selected_doc: Signal::new(Some(1)),
            selected_task: Signal::new(Some(4)),
            selected_alarm: Signal::new(Some(102)),
            selected_member: Signal::new(Some(0)),
            recordings: Signal::new(seed_recordings()),
            selected_recording: Signal::new(Some(1)),
            commands: Signal::new(seed_commands()),
            page_request: Signal::new([None; 4]),
            page_request_sub: Signal::new([None; 4]),
            zone_width: std::array::from_fn(|_| Signal::new(960.0)),
            toast_inbox: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            channel_sel: Signal::new(0),
            active_chrome: Signal::new(0),
            editor_section: Signal::new(0),
        }
    }

    /// Lookup helpers the binding layer uses constantly.
    pub fn asset(&self, id: u32) -> Option<Asset> {
        self.assets.get().into_iter().find(|a| a.id == id)
    }

    pub fn asset_name(&self, id: u32) -> &'static str {
        self.asset(id).map(|a| a.name).unwrap_or("—")
    }

    /// Children of a hierarchy node (sites have `parent: None`).
    pub fn asset_children(&self, parent: Option<u32>) -> Vec<Asset> {
        self.assets
            .get()
            .into_iter()
            .filter(|a| a.parent == parent)
            .collect()
    }

    /// Append a shift-log entry (ChatInput/MessageComposer's publish
    /// path — the loopback that makes comms real). The log is capped
    /// at [`LOG_CAP`] entries — per-tick readers clone it, so an
    /// unbounded log would grow their cost linearly with session
    /// length.
    pub fn log(&self, author: usize, text: impl Into<String>) {
        self.log_ch(author, text, LogChannel::Ops, None);
    }

    /// Channel/WO-tagged append — COMMS posts to `channel_sel`, the
    /// CommentThread posts under `selected_wo`, the Terminal echoes to
    /// `System`.
    pub fn log_ch(
        &self,
        author: usize,
        text: impl Into<String>,
        channel: LogChannel,
        wo: Option<u32>,
    ) {
        let mut log = self.shift_log.get();
        log.push(LogEntry {
            id: self
                .log_seq
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            minute: self.shift_minute.get(),
            author,
            text: text.into(),
            pinned: false,
            channel,
            wo,
        });
        let over = log.len().saturating_sub(LOG_CAP);
        if over > 0 {
            log.drain(..over);
        }
        self.shift_log.set(log);
    }

    /// Request a zone page — `page_request[zone] = Some(page)`;
    /// `ZonePanel::tick` drains the slot and activates the tab.
    /// A plain request lands on the page's default inner view, so a
    /// pending deep-link sub for the zone is cleared with it.
    pub fn request_page(&self, zone: usize, page: u8) {
        let mut req = self.page_request.get();
        req[zone] = Some(page);
        self.page_request.set(req);
        let mut sub = self.page_request_sub.get();
        if sub[zone].take().is_some() {
            self.page_request_sub.set(sub);
        }
    }

    /// Deep-link request — activate `page` and land on its inner
    /// `sub` view (the page's Swap/Segmented index). The page drains
    /// the sub via [`take_page_sub`](Self::take_page_sub) once it
    /// ticks — i.e. after activation, while visible.
    pub fn request_page_deep(&self, zone: usize, page: u8, sub: u8) {
        let mut req = self.page_request.get();
        req[zone] = Some(page);
        self.page_request.set(req);
        let mut s = self.page_request_sub.get();
        s[zone] = Some((page, sub));
        self.page_request_sub.set(s);
    }

    /// Drain this zone's pending deep-link sub *for `page`*
    /// (take semantics — a consumed sub never re-fires). Called by
    /// the activated page's strip `Segmented` pull; hidden pages
    /// don't tick, so a sub can only land on the page that was just
    /// activated — and a sub addressed to a different page is left
    /// for it.
    pub fn take_page_sub(&self, zone: usize, page: u8) -> Option<u8> {
        let mut req = self.page_request_sub.get();
        match req[zone] {
            Some((p, sub)) if p == page => {
                req[zone] = None;
                self.page_request_sub.set(req);
                Some(sub)
            }
            _ => None,
        }
    }

    /// Mutate one maintenance task — the MAINTENANCE rail's
    /// rebook/progress write path. No-op closures skip the `set`.
    pub fn update_task(&self, id: u32, f: impl FnOnce(&mut MaintTask)) {
        let mut s = self.schedule.get();
        match s.iter_mut().find(|t| t.id == id) {
            Some(t) => {
                let before = t.clone();
                f(t);
                if *t == before {
                    return;
                }
            }
            None => return,
        }
        self.schedule.set(s);
    }

    /// Append a camera-loop recording — the CAMERAS record verb's
    /// write path. Returns the new id.
    pub fn add_recording(&self, title: &'static str, camera: usize, secs: u32) -> u32 {
        let mut recs = self.recordings.get();
        let id = recs.iter().map(|r| r.id).max().unwrap_or(0) + 1;
        recs.push(Recording {
            id,
            title,
            camera,
            secs,
        });
        self.recordings.set(recs);
        id
    }

    /// Acknowledge an alarm — AlarmPanel/ContextMenu/Button write path.
    /// No-ops (already-acked, unknown id) skip the `set` so bound
    /// widgets aren't needlessly re-seated.
    pub fn ack_alarm(&self, id: u32) {
        let mut alarms = self.alarms.get();
        match alarms.iter_mut().find(|a| a.id == id) {
            Some(a) if !a.acked => a.acked = true,
            _ => return,
        }
        self.alarms.set(alarms);
    }

    /// Move a work order between columns — Kanban/Transfer write path.
    /// Same-column drops and unknown ids skip the `set`.
    pub fn move_wo(&self, id: u32, status: WoStatus) {
        let mut wos = self.work_orders.get();
        match wos.iter_mut().find(|w| w.id == id) {
            Some(w) if w.status != status => w.status = status,
            _ => return,
        }
        self.work_orders.set(wos);
    }

    /// Update a field of the selected work order — the inspector
    /// form's write path (every field widget routes here). A closure
    /// that changes nothing skips the `set`, so hover-adjacent writes
    /// don't invalidate `wos_sig` and rebuild the kanban mid-drag.
    pub fn update_wo(&self, f: impl FnOnce(&mut WorkOrder)) {
        let Some(sel) = self.selected_wo.get() else {
            return;
        };
        let mut wos = self.work_orders.get();
        let Some(i) = wos.iter().position(|w| w.id == sel) else {
            return;
        };
        let before = wos[i].clone();
        f(&mut wos[i]);
        if wos[i] == before {
            return;
        }
        self.work_orders.set(wos);
    }

    /// Active (unacked) alarms — the alarm zone's source.
    pub fn active_alarms(&self) -> Vec<Alarm> {
        self.alarms
            .get()
            .into_iter()
            .filter(|a| a.active && !a.acked)
            .collect()
    }

    /// Alarm counts by severity — ChipGroup badges + Pie/Treemap data.
    pub fn alarm_distribution(&self) -> (usize, usize, usize) {
        let mut d = (0, 0, 0);
        for a in self.alarms.get().iter().filter(|a| a.active) {
            match a.severity {
                AlarmSeverity::Info => d.0 += 1,
                AlarmSeverity::Warning => d.1 += 1,
                AlarmSeverity::Critical => d.2 += 1,
            }
        }
        d
    }

    /// OEE rollup: mean of running cells — the KPI strip's headline.
    pub fn plant_oee(&self) -> f64 {
        let cells: Vec<f64> = self
            .assets
            .get()
            .iter()
            .filter(|a| a.kind == AssetKind::Cell)
            .map(|a| a.oee)
            .collect();
        if cells.is_empty() {
            0.0
        } else {
            cells.iter().sum::<f64>() / cells.len() as f64
        }
    }

    /// Advance the simulated clock — the app calls this per real
    /// second; the acoustic model evolves with line state.
    pub fn tick_minute(&self) {
        self.shift_minute.set(self.shift_minute.get() + 1);
    }

    /// Append the current cpu/mem samples to the history rings —
    /// the app's per-frame sampler calls this once; every trend
    /// chart then reads the same series (shared ring, no per-widget
    /// buffers — the council's perf requirement).
    pub fn push_history(&self) {
        let mut cpu = self.cpu_hist.get();
        cpu.push(self.cpu.get());
        if cpu.len() > HISTORY_LEN {
            cpu.remove(0);
        }
        self.cpu_hist.set(cpu);
        let mut mem = self.mem_hist.get();
        mem.push(self.mem.get());
        if mem.len() > HISTORY_LEN {
            mem.remove(0);
        }
        self.mem_hist.set(mem);
        self.hist_rev.update(|r| *r += 1);
    }

    /// Evolve the acoustic model one frame — deterministic drift driven
    /// by `line_running` and cpu load, so Spectrum/VU/Tuner all move
    /// with the plant rather than a canned animation.
    pub fn tick_acoustic(&self, phase: f64) {
        let mut a = self.acoustic.get();
        let drive = if self.line_running.get() { 1.0 } else { 0.15 };
        let load = self.cpu.get();
        for (i, b) in a.bands.iter_mut().enumerate() {
            let f = i as f64;
            *b = (drive * (0.55 + 0.30 * (phase * (0.7 + f * 0.13)).sin() + 0.15 * load)
                / (1.0 + f * 0.18))
                .clamp(0.0, 1.0);
        }
        a.level = (
            (a.bands[2] * 0.9 + 0.05).min(1.0),
            (a.bands[5] * 0.85 + 0.05).min(1.0),
        );
        a.dominant_hz = 118.0 + 40.0 * (phase * 0.5).sin() * drive;
        self.acoustic.set(a);
    }

    /// Drive the sim to a representative OPERATING state for headless
    /// consumers — screenshot/lint sweeps (`lint_sweep::run`,
    /// `dump_zone_lints`, `dump_widget_tree`, the paint-audit tests)
    /// never run the app's frame loop, so the stores the loop feeds
    /// (`push_history` at 4 Hz, `tick_acoustic` every frame,
    /// `tick_minute` per second — see `App::redraw`) would otherwise
    /// sit at seed state and every chart/spectrum/gauge would paint
    /// the flat seed line. One call replays a full [`HISTORY_LEN`]
    /// window: cpu/mem advance through [`telemetry_waveform`] at the
    /// panel's [`TELEMETRY_PHASE_STEP`] cadence, each sample is pushed
    /// into the rings, the acoustic model ticks at the same phase,
    /// and the shift clock ticks once per four samples (the live
    /// 1 sim-minute-per-second ÷ 4 Hz cadence). Deterministic — no
    /// rand, no wall clock — and idempotent (phase restarts from 0),
    /// so golden frame dumps stay stable.
    pub fn warm_demo_state(&self) {
        let mut phase = 0.0f64;
        for i in 0..HISTORY_LEN {
            phase += TELEMETRY_PHASE_STEP;
            let (cpu, mem) = telemetry_waveform(phase);
            self.cpu.set(cpu);
            self.mem.set(mem);
            self.push_history();
            self.tick_acoustic(phase);
            if i % 4 == 3 {
                self.tick_minute();
            }
        }
    }
}

/// Per-sample phase advance of the telemetry oscillator — shared by
/// `TelemetryPanel::tick` (the live driver) and
/// [`PlantModel::warm_demo_state`] (the headless replay) so sweep
/// frames show a curve with the live app's sampling density.
pub(crate) const TELEMETRY_PHASE_STEP: f64 = 0.11;

/// The plant's cpu/mem waveform — smooth primary oscillation + a
/// harmonic + deterministic jitter, clamped to operating range.
/// Shared by `TelemetryPanel::tick` and
/// [`PlantModel::warm_demo_state`]; duplicating the constants here
/// and in the panel was the alternative, but a single definition
/// keeps the frame dump from drifting off the live curve — the
/// model owns the sim's math, the panel is only its sampler.
pub(crate) fn telemetry_waveform(t: f64) -> (f64, f64) {
    let cpu = (0.52 + 0.22 * t.sin() + 0.09 * (t * 2.7).sin() + 0.03 * (t * 13.0).cos())
        .clamp(0.02, 0.98);
    let mem = (0.61 + 0.14 * (t * 0.43 + 1.7).sin() + 0.02 * (t * 7.0).cos()).clamp(0.05, 0.97);
    (cpu, mem)
}

// ---------------------------------------------------------------------------
// Seed data — deterministic fictional facility "Plant East".
// ---------------------------------------------------------------------------

fn seed_assets() -> Vec<Asset> {
    use AssetKind::*;
    use AssetStatus::*;
    vec![
        // Sites
        a(
            1,
            None,
            Site,
            "Plant East",
            Running,
            0.84,
            "SITE-E",
            2015,
            "Primary stamping & assembly",
        ),
        a(
            2,
            None,
            Site,
            "Plant West",
            Running,
            0.79,
            "SITE-W",
            2018,
            "QA lab + packaging",
        ),
        // Plant East lines
        a(
            3,
            Some(1),
            Line,
            "Line 1 — Stamping",
            Running,
            0.88,
            "LN-E1",
            2016,
            "400t press line",
        ),
        a(
            4,
            Some(1),
            Line,
            "Line 2 — Welding",
            Degraded,
            0.71,
            "LN-E2",
            2016,
            "Bearing temp trending high",
        ),
        a(
            5,
            Some(1),
            Line,
            "Line 3 — Assembly",
            Running,
            0.91,
            "LN-E3",
            2019,
            "",
        ),
        // Plant West lines
        a(
            6,
            Some(2),
            Line,
            "Line 4 — Packaging",
            Maintenance,
            0.0,
            "LN-W1",
            2020,
            "PM window until 14:00",
        ),
        // Cells on Line 2 (the interesting ones)
        a(
            7,
            Some(4),
            Cell,
            "Robot-Arm-K7",
            Degraded,
            0.66,
            "RA-K7-2211",
            2021,
            "Torque drift on joint 3",
        ),
        a(
            8,
            Some(4),
            Cell,
            "Weld-Cell-B",
            Running,
            0.82,
            "WC-B-0944",
            2021,
            "",
        ),
        a(
            9,
            Some(3),
            Cell,
            "CNC-Mill-02",
            Running,
            0.93,
            "CM-02-7710",
            2022,
            "",
        ),
        a(
            10,
            Some(3),
            Cell,
            "Lathe-07",
            Running,
            0.89,
            "LT-07-3310",
            2020,
            "Chuck serviced last week",
        ),
        a(
            11,
            Some(5),
            Cell,
            "AGV-Dock-04",
            Running,
            0.95,
            "AGV-04-1102",
            2023,
            "",
        ),
        a(
            12,
            Some(6),
            Cell,
            "HVAC-Skid-11",
            Down,
            0.0,
            "HV-11-5509",
            2019,
            "Compressor fault — WO-4473",
        ),
    ]
}

/// Tiny asset literal helper — keeps the seed table readable. Nine
/// args mirror `Asset`'s field order exactly — a builder here would
/// be noisier than the table it constructs.
#[allow(clippy::too_many_arguments)]
fn a(
    id: u32,
    parent: Option<u32>,
    kind: AssetKind,
    name: &'static str,
    status: AssetStatus,
    oee: f64,
    serial: &'static str,
    installed: u16,
    note: &'static str,
) -> Asset {
    // Health derives from status+OEE — the sim owns it (degraded/down
    // trend lower); DETAIL's Rating renders it read-only.
    let health = match status {
        AssetStatus::Running => (0.6 + oee * 0.4).min(1.0),
        AssetStatus::Degraded => 0.35 + oee * 0.3,
        AssetStatus::Down => 0.08,
        AssetStatus::Maintenance => 0.5,
    };
    Asset {
        id,
        parent,
        kind,
        name,
        status,
        oee,
        serial,
        installed,
        note,
        health,
    }
}

fn seed_work_orders() -> Vec<WorkOrder> {
    use WoPriority::*;
    use WoStatus::*;
    vec![
        WorkOrder {
            id: 4469,
            title: "WO-4469 shaft",
            asset: 9,
            status: InProgress,
            priority: Medium,
            assignee: 1,
            checklist: vec![
                ("Inspect runout", true),
                ("Regrind", true),
                ("Balance check", false),
            ],
            due_day: 1,
            progress: 0.65,
            notes: "Runout 0.04mm before regrind.".into(),
            signature: None,
            photo: None,
        },
        WorkOrder {
            id: 4470,
            title: "WO-4470 bracket",
            asset: 10,
            status: Queued,
            priority: Low,
            assignee: 2,
            checklist: vec![("Pull fixture", false), ("Weld repair", false)],
            due_day: 3,
            progress: 0.0,
            notes: String::new(),
            signature: None,
            photo: None,
        },
        WorkOrder {
            id: 4471,
            title: "WO-4471 housing",
            asset: 7,
            status: InProgress,
            priority: Critical,
            assignee: 0,
            checklist: vec![
                ("Lock out cell", true),
                ("Swap bearing 6204", true),
                ("Torque verified", false),
                ("Sign-off", false),
            ],
            due_day: 0,
            progress: 0.5,
            notes: "Joint-3 torque drift — escalate if >5%.".into(),
            signature: None,
            photo: None,
        },
        WorkOrder {
            id: 4472,
            title: "WO-4472 seal kit",
            asset: 8,
            status: Review,
            priority: Medium,
            assignee: 3,
            checklist: vec![("Replace seals", true), ("Pressure test", true)],
            due_day: 2,
            progress: 1.0,
            notes: "Awaiting QA sign-off.".into(),
            signature: Some(Signature::Typed("I. Chen".into())),
            photo: None,
        },
        WorkOrder {
            id: 4473,
            title: "WO-4473 compressor",
            asset: 12,
            status: Queued,
            priority: High,
            assignee: 4,
            checklist: vec![("Diagnose fault", false), ("Order compressor", false)],
            due_day: 4,
            progress: 0.1,
            notes: "HVAC skid down — line 4 on hold.".into(),
            signature: None,
            photo: None,
        },
    ]
}

fn seed_schedule() -> Vec<MaintTask> {
    vec![
        MaintTask {
            id: 1,
            title: "Retool Line 3",
            asset: 5,
            start_day: 0,
            days: 2,
            done: false,
            crew: 0,
            progress: 0.0,
        },
        MaintTask {
            id: 2,
            title: "Swap worn dies",
            asset: 3,
            start_day: 1,
            days: 1,
            done: false,
            crew: 1,
            progress: 0.0,
        },
        MaintTask {
            id: 3,
            title: "Line 4 PM window",
            asset: 6,
            start_day: 0,
            days: 3,
            done: false,
            crew: 2,
            progress: 0.0,
        },
        MaintTask {
            id: 4,
            title: "K7 joint service",
            asset: 7,
            start_day: 2,
            days: 2,
            done: false,
            crew: 0,
            progress: 0.4,
        },
        MaintTask {
            id: 5,
            title: "Weld scan Weld-B",
            asset: 8,
            start_day: 4,
            days: 1,
            done: false,
            crew: 3,
            progress: 0.0,
        },
        MaintTask {
            id: 6,
            title: "HVAC compressor",
            asset: 12,
            start_day: 3,
            days: 3,
            done: false,
            crew: 4,
            progress: 0.15,
        },
        MaintTask {
            id: 7,
            title: "Changeover complete",
            asset: 3,
            start_day: 0,
            days: 1,
            done: true,
            crew: 1,
            progress: 1.0,
        },
    ]
}

fn seed_shift_log() -> Vec<LogEntry> {
    use LogChannel::*;
    [
        (
            6,
            usize::MAX,
            "Shift A started — Line 4 in PM window",
            true,
            System,
            None,
        ),
        (47, 0, "K7 torque drift ~3%, monitoring", false, Ops, None),
        (
            88,
            3,
            "WO-4472 pressure test passed",
            false,
            Comms,
            Some(4472),
        ),
        (
            140,
            usize::MAX,
            "Temp alarm cleared on Weld-B",
            false,
            System,
            None,
        ),
        (
            176,
            1,
            "Die swap done ahead of schedule",
            false,
            Comms,
            Some(4469),
        ),
        (
            210,
            4,
            "HVAC-Skid-11 compressor fault confirmed",
            false,
            Ops,
            Some(4473),
        ),
        (
            215,
            0,
            "Bearing 6204 on-site, swapping now",
            false,
            Comms,
            Some(4471),
        ),
    ]
    .into_iter()
    .enumerate()
    .map(
        |(i, (minute, author, text, pinned, channel, wo))| LogEntry {
            id: i as u64,
            minute,
            author,
            text: text.into(),
            pinned,
            channel,
            wo,
        },
    )
    .collect()
}

fn seed_alarms() -> Vec<Alarm> {
    use AlarmSeverity::*;
    vec![
        Alarm {
            id: 101,
            asset: 7,
            severity: Warning,
            message: "Torque drift on joint 3",
            active: true,
            acked: false,
            raised_min: 45,
        },
        Alarm {
            id: 102,
            asset: 12,
            severity: Critical,
            message: "Compressor fault",
            active: true,
            acked: false,
            raised_min: 210,
        },
        Alarm {
            id: 103,
            asset: 4,
            severity: Warning,
            message: "Bearing temp HI",
            active: true,
            acked: false,
            raised_min: 190,
        },
        Alarm {
            id: 104,
            asset: 8,
            severity: Info,
            message: "Weld scan due",
            active: true,
            acked: true,
            raised_min: 140,
        },
        Alarm {
            id: 105,
            asset: 11,
            severity: Info,
            message: "AGV uplink weak",
            active: true,
            acked: false,
            raised_min: 200,
        },
        Alarm {
            id: 106,
            asset: 9,
            severity: Info,
            message: "Cycle time above target",
            active: false,
            acked: true,
            raised_min: 60,
        },
    ]
}

fn seed_crew() -> Vec<CrewMember> {
    use Presence::*;
    use Room::*;
    vec![
        CrewMember {
            name: "Ana Ruiz",
            role: "Shift Lead A",
            presence: OnShift,
            reports_to: None,
            room: Floor,
        },
        CrewMember {
            name: "Tom Alvarez",
            role: "Maintenance",
            presence: OnShift,
            reports_to: Some(0),
            room: Floor,
        },
        CrewMember {
            name: "Iris Chen",
            role: "QA Lead",
            presence: OnShift,
            reports_to: Some(0),
            room: Control,
        },
        CrewMember {
            name: "Sam Wu",
            role: "Operator",
            presence: Break,
            reports_to: Some(0),
            room: BreakoutA,
        },
        CrewMember {
            name: "Priya Nair",
            role: "Reliability Eng",
            presence: Remote,
            reports_to: Some(0),
            room: Offsite,
        },
        CrewMember {
            name: "Jo Park",
            role: "Operator",
            presence: OnShift,
            reports_to: Some(0),
            room: Floor,
        },
    ]
}

fn seed_acoustic() -> AcousticModel {
    AcousticModel {
        bands: [0.4; 16],
        level: (0.4, 0.35),
        dominant_hz: 120.0,
        tone_note: 57, // A3
        tone_pattern: [true, false, true, false, true, false, true, true],
        tone_bpm: 96,
    }
}

fn seed_flow() -> Vec<(u32, u32, f64)> {
    // Material flow between production lines, tons/hr — the Sankey/
    // GraphView dataset. Asset ids come from `seed_assets`.
    vec![
        (2, 4, 18.5), // Line A → Line B
        (2, 5, 6.0),  // Line A → Line C
        (4, 5, 12.0), // Line B → Line C
        (5, 6, 9.5),  // Line C → Packaging
        (4, 6, 4.0),  // Line B → Packaging
    ]
}

fn seed_docs() -> Vec<DocEntry> {
    use DocKind::*;
    vec![
        DocEntry {
            id: 1,
            title: "Line 2 service manual",
            asset: Some(4),
            kind: Manual,
            body: "{\"line\": \"LN-E2\", \"rev\": \"C\", \"sections\": [\"safety\", \"lubrication\", \"torque spec\"], \"pages\": 148}",
            bytes: b"SERVICE MANUAL LN-E2 REV C - torque spec table, lubrication chart, bearing kit list.",
        },
        DocEntry {
            id: 2,
            title: "K7 firmware blob",
            asset: Some(7),
            kind: Firmware,
            body: "{\"device\": \"RA-K7\", \"fw\": \"4.2.1\", \"crc\": \"0x9AE3\", \"joints\": 6}",
            bytes: b"\x7fK7FW\x04\x02\x01RA-K7-2211\x9a\xe3 torque-cal joint3 +5%",
        },
        DocEntry {
            id: 3,
            title: "Shift A handover report",
            asset: None,
            kind: Report,
            body: "{\"shift\": \"A\", \"oee\": 0.84, \"alarms\": 3, \"open_wos\": 2, \"note\": \"Line 4 PM until 14:00\"}",
            bytes: b"SHIFT A HANDOVER - OEE 84%, 3 active alarms, 2 open WOs, Line 4 PM window.",
        },
        DocEntry {
            id: 4,
            title: "Weld-B scan journal",
            asset: Some(8),
            kind: Log,
            body: "{\"asset\": \"WC-B-0944\", \"scans\": 12, \"defects\": 0, \"last\": \"pass\"}",
            bytes: b"WELD SCAN JOURNAL WC-B - 12 scans, 0 defects, last result PASS.",
        },
    ]
}

fn seed_recordings() -> Vec<Recording> {
    vec![
        Recording {
            id: 1,
            title: "CAM-01 aisle loop",
            camera: 0,
            secs: 95,
        },
        Recording {
            id: 2,
            title: "CAM-02 east yard",
            camera: 1,
            secs: 140,
        },
        Recording {
            id: 3,
            title: "CAM-03 dock doors",
            camera: 2,
            secs: 60,
        },
    ]
}

fn seed_commands() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "wo.new",
            label: "New work order",
            chromes: 0b111111,
        },
        CommandSpec {
            id: "wo.advance",
            label: "Advance selected WO",
            chromes: 0b110111,
        },
        CommandSpec {
            id: "alarm.ack",
            label: "Ack all alarms",
            chromes: 0b110111,
        },
        CommandSpec {
            id: "alarm.filter",
            label: "Filter alarms…",
            chromes: 0b010001,
        },
        CommandSpec {
            id: "asset.search",
            label: "Find asset…",
            chromes: 0b111111,
        },
        CommandSpec {
            id: "nav.registry",
            label: "Go to REGISTRY",
            chromes: 0b110001,
        },
        CommandSpec {
            id: "nav.detail",
            label: "Open asset detail",
            chromes: 0b110001,
        },
        CommandSpec {
            id: "cam.record",
            label: "Record camera loop",
            chromes: 0b010101,
        },
        CommandSpec {
            id: "sys.snapshot",
            label: "Snapshot diagnostics",
            chromes: 0b010011,
        },
        CommandSpec {
            id: "lock.toggle",
            label: "Lock console",
            chromes: 0b111001,
        },
    ]
}
