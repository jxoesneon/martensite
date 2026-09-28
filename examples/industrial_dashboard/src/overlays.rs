//! Shell overlays — the workstation's app-level popup layer.
//!
//! One invisible arena widget owns the three shell surfaces and
//! reconciles them in `sync_overlay` (the same pattern `Dropdown` and
//! the grid's context menu use):
//!
//! - **About dialog** — `Dialog` at `OverlayAnchor::Center` with
//!   `OverlayOptions::modal()`: scrim painted, input blocked, scrim
//!   clicks consumed but not dismissing — a real modal.
//! - **Inspector drawer** — `Drawer` at `OverlayAnchor::EdgeRight` with
//!   `modal().light_dismiss()`: scrim tap or the header's × closes it.
//!   Its content is rebuilt from live `PlantModel` reads and re-seated
//!   via `replace_content` whenever the model-derived signature moves —
//!   overlay entries never tick, so `Bound` pull/push can't run inside
//!   them and whole-content re-seat is the binding seam (the same
//!   idiom `sync_message_list` uses for shift-log appends).
//! - **Toast strip** — `ToastHost` at `OverlayAnchor::Viewport`
//!   bottom-right with `OverlayOptions::passthrough()`: clicks outside
//!   a card fall through to content, and an outside press never
//!   dismisses the strip. Overlay entries never see `tick`, so this
//!   widget keeps the canonical `ToastHost`, ticks it here, and
//!   replaces the entry's content when it changes.
//!
//! Requests arrive through `Signal`s (the toolbar writes them); toast
//! producers enqueue through `toast_inbox` — the shared-cell seam the
//! overlay pattern uses everywhere in this app.

use std::sync::{Arc, Mutex};

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite::core::overlay::{OverlayAnchor, OverlayLayer, OverlayOptions, ViewportAlign};
use martensite::core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect, Widget,
};
use martensite::prelude::Signal;
use martensite::widgets::{Banner, Dialog, Disclosure, Severity, Toast, ToastHost};
use martensite::widgets::{
    Battery, DigitalClock, StripChart, Terminal, Thermometer, Time, VuMeter,
};
use martensite::widgets::{BulletChart, Spectrum, Waveform, XYPad};
use martensite::widgets::{Drawer, Flex, Text};
use martensite::widgets::{LogSeverity, LogView, Status as DotStatus, StatusDot};
use martensite::widgets::{SettingsGroup, SettingsRow};

/// The toast inbox — producers `lock().push(Toast)`; the host drains
/// them on the next tick.
pub type ToastInbox = Arc<Mutex<Vec<Toast>>>;

/// Enqueue a toast — the app's side of the inbox seam.
pub fn push_toast(inbox: &ToastInbox, severity: Severity, message: impl Into<String>) {
    if let Ok(mut q) = inbox.lock() {
        q.push(Toast::new(severity, message));
    }
}

/// Invisible owner of the shell's overlay surfaces. Its own bounds are
/// a zero-size cell — everything it shows lives in the overlay layer.
pub struct ShellOverlays {
    /// Toolbar "About" button → open request.
    about_req: Signal<bool>,
    /// Toolbar "Inspector" button → open request.
    inspector_req: Signal<bool>,
    /// The app-wide `alerts_on` cell — the drawer mirrors it as a
    /// status lamp (overlay content can't drain a Switch's write-back,
    /// so the row alerts row is a readout, not a dead control).
    alerts_on: Signal<bool>,
    /// The shared plant model — the inspector drawer's content binds
    /// to it (live feeds, KPIs, acoustic, alarms) instead of seeded
    /// literals.
    model: crate::domain::PlantModel,
    /// Signature of the drawer's seated content — `sync_overlay`
    /// compares it against [`Self::drawer_signature`] each pass and
    /// re-seats the entry via `replace_content` on a difference, so
    /// the drawer stays live without any widget inside it ticking.
    drawer_sig: Option<u64>,
    /// Canonical toast state — overlay entries are never ticked, so
    /// the owner ticks and pushes snapshots via `replace_content`.
    toasts: ToastHost,
    /// Live overlay entry ids.
    dialog_id: Option<u64>,
    drawer_id: Option<u64>,
    toast_id: Option<u64>,
    /// Response cell the dialog writes on button press.
    dialog_resp: Arc<Mutex<Option<usize>>>,
    /// Cell the drawer sets on its close affordance.
    drawer_close: Arc<Mutex<bool>>,
    /// Shared inbox drained into `toasts` on tick.
    toast_inbox: ToastInbox,
}

impl ShellOverlays {
    pub fn new(
        about_req: Signal<bool>,
        inspector_req: Signal<bool>,
        alerts_on: Signal<bool>,
        toast_inbox: ToastInbox,
        model: crate::domain::PlantModel,
    ) -> Self {
        Self {
            about_req,
            inspector_req,
            alerts_on,
            model,
            drawer_sig: None,
            toasts: ToastHost::new().with_text_painter(martensite::text_paint::shared_painter()),
            dialog_id: None,
            drawer_id: None,
            toast_id: None,
            dialog_resp: Arc::new(Mutex::new(None)),
            drawer_close: Arc::new(Mutex::new(false)),
            toast_inbox,
        }
    }

    /// Builds the About dialog card fresh on each open.
    fn about_dialog(&self) -> Dialog {
        Dialog::new("Martensite Workstation")
            .body("A dogfood build of the Martensite widget toolkit — dockable panels, shaped rendering, real accessibility, and this modal dialog all run on the same arena.")
            .buttons(&["Close"])
            .response_sink(Arc::clone(&self.dialog_resp))
            .with_text_painter(martensite::text_paint::shared_painter())
    }

    /// FNV-1a mix step — the field-hash signature idiom the zones use
    /// (`crew_sig`/`log_sig` in `zones::media`); those folds are
    /// private, so the drawer carries its own copy.
    fn sig_fold(h: u64, v: u64) -> u64 {
        (h ^ v).wrapping_mul(0x100_0000_01b3)
    }

    fn bool_fold(h: u64, v: bool) -> u64 {
        Self::sig_fold(h, u64::from(v))
    }

    fn f64_fold(h: u64, v: f64) -> u64 {
        Self::sig_fold(h, v.to_bits())
    }

    fn str_fold(h: u64, s: &str) -> u64 {
        let mut h = Self::sig_fold(h, s.len() as u64);
        for b in s.bytes() {
            h = Self::sig_fold(h, u64::from(b));
        }
        h
    }

    /// Change-detection signature over every `PlantModel` value
    /// [`Self::inspector_drawer`] reads. `sync_overlay` compares it
    /// per pass and re-seats the drawer's content on a difference —
    /// a re-seat drops in-widget press state (a `Disclosure`'s open
    /// flag, a Terminal's scroll), so it must fire only when the data
    /// actually moved; folding everything displayed keeps that test
    /// strictly correct. `hist_rev` stands in for the history rings
    /// (bumped by `push_history` — domain.rs calls it the strictly
    /// correct signature) and `log_seq` for `shift_log` appends.
    fn drawer_signature(&self) -> u64 {
        let m = &self.model;
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        // Scalar flags the lamps/sections mirror.
        for v in [
            self.alerts_on.get(),
            m.line_running.get(),
            m.console_locked.get(),
            m.reduced_motion.get(),
            m.paused.get(),
            m.media_playing.get(),
        ] {
            h = Self::bool_fold(h, v);
        }
        // Selection + filters shown in the Selection disclosure.
        h = Self::sig_fold(h, m.selected_asset.get().map(u64::from).unwrap_or(u64::MAX));
        h = Self::str_fold(h, &m.filter_text.get());
        h = Self::str_fold(h, &m.site_filter.get());
        // Alarms — banner severity + the feeds' unacked lamp.
        for a in &m.alarms.get() {
            h = Self::sig_fold(h, u64::from(a.id));
            h = Self::bool_fold(h, a.active);
            h = Self::bool_fold(h, a.acked);
            h = Self::sig_fold(h, a.severity as u64);
        }
        // Assets — the KPI charts read kind/name/status/oee.
        for a in &m.assets.get() {
            h = Self::sig_fold(h, u64::from(a.id));
            h = Self::sig_fold(h, a.status as u64);
            h = Self::f64_fold(h, a.oee);
            h = Self::str_fold(h, a.name);
        }
        // Acoustic monitor — vibration + line-noise sections.
        let ac = m.acoustic.get();
        for b in ac.bands {
            h = Self::f64_fold(h, b);
        }
        h = Self::f64_fold(h, ac.level.0);
        h = Self::f64_fold(h, ac.level.1);
        h = Self::f64_fold(h, ac.dominant_hz);
        // Jog pad readout.
        let j = m.jog.get();
        h = Self::f64_fold(h, j.axis.0);
        h = Self::f64_fold(h, j.axis.1);
        h = Self::f64_fold(h, j.tilt);
        h = Self::f64_fold(h, j.zoom);
        // Board load + history rings + shift clock + log tail.
        h = Self::f64_fold(h, m.cpu.get());
        h = Self::f64_fold(h, m.mem.get());
        h = Self::sig_fold(h, m.hist_rev.get());
        h = Self::sig_fold(h, u64::from(m.shift_minute.get()));
        h = Self::sig_fold(h, m.log_seq.load(std::sync::atomic::Ordering::Relaxed));
        h
    }

    /// Builds the inspector drawer — a `Flex` column of facade widgets
    /// (banner, disclosures, settings cards) inside the drawer
    /// surface.
    ///
    /// Every section reads the shared [`PlantModel`](crate::domain::PlantModel)
    /// at build time; the overlay entry never receives `tick`, so
    /// `sync_overlay` re-seats the whole drawer whenever
    /// [`Self::drawer_signature`] moves — the model animates the
    /// content without any widget inside it ticking. Widgets whose
    /// writes could only leave through a `pull` drain (Switch,
    /// Equalizer faders, an interactive XYPad, Terminal submissions)
    /// have no drain path inside an overlay, so they mount as honest
    /// readouts (`enabled(false)`, `StatusDot` lamps) or are cut —
    /// the bind-or-cut rule bars dead controls.
    fn inspector_drawer(&self) -> Drawer {
        let painter = martensite::text_paint::shared_painter();
        let m = &self.model;
        // Shared lamp builder — the trailing state word + status +
        // pulse each flag/feed row declares.
        let dot = |text: &str, status: DotStatus, pulse: bool| {
            StatusDot::new(text)
                .status(status)
                .pulse(pulse)
                .with_text_painter(painter.clone())
        };
        let unacked = m.active_alarms();
        let crit = unacked
            .iter()
            .filter(|a| a.severity == crate::domain::AlarmSeverity::Critical)
            .count();
        // Banner doubles as the plant's alarm headline — a real
        // readout of `active_alarms`, not a fixed "attached" strip.
        let (sev, msg) = if crit > 0 {
            (
                Severity::Error,
                format!("{crit} critical · {} unacked alarms", unacked.len()),
            )
        } else if !unacked.is_empty() {
            (
                Severity::Warning,
                format!("{} unacked alarms", unacked.len()),
            )
        } else {
            (
                Severity::Info,
                "Inspector attached — plant nominal".to_string(),
            )
        };
        // Selection — the inspected asset plus any active filters.
        let mut sel = match m.selected_asset.get() {
            Some(id) => match m.asset(id) {
                Some(a) => format!(
                    "{} — {} · OEE {:.0}%",
                    a.name,
                    a.status.label(),
                    a.oee * 100.0
                ),
                None => format!("asset #{id} — not in registry"),
            },
            None => "no asset selected".to_string(),
        };
        let (flt, site) = (m.filter_text.get(), m.site_filter.get());
        if !flt.is_empty() {
            sel.push_str(&format!(" · filter \"{flt}\""));
        }
        if !site.is_empty() {
            sel.push_str(&format!(" · site {site}"));
        }
        // The "row alerts" Switch is gone: seeded from `alerts_on`
        // but undrainable, an in-drawer flip was visual-only — a dead
        // control. The lamp mirrors the shared cell honestly; the
        // toolbar owns the write path.
        let alerts = dot(
            if self.alerts_on.get() {
                "row alerts on"
            } else {
                "row alerts off"
            },
            if self.alerts_on.get() {
                DotStatus::Ok
            } else {
                DotStatus::Off
            },
            self.alerts_on.get(),
        );
        // Plant flags — the old gallery's fake prefs (Density/
        // Refresh/Confidence/Focus shortcut) had no model signal to
        // bind, so they're cut; these rows are the flags the model
        // actually carries, mirrored as lamps for the same
        // can't-drain reason as the alerts row.
        let locked = m.console_locked.get();
        let running = m.line_running.get();
        let reduced = m.reduced_motion.get();
        let prefs = SettingsGroup::new("Plant flags")
            .carded(true)
            .row(
                SettingsRow::new("Line running")
                    .subtitle("drives the acoustic monitor")
                    .trailing(dot(
                        if running { "running" } else { "stopped" },
                        if running {
                            DotStatus::Ok
                        } else {
                            DotStatus::Off
                        },
                        running,
                    )),
            )
            .row(
                SettingsRow::new("Console lock")
                    .subtitle("gates the HMI")
                    .trailing(dot(
                        if locked { "locked" } else { "unlocked" },
                        if locked {
                            DotStatus::Warning
                        } else {
                            DotStatus::Ok
                        },
                        false,
                    )),
            )
            .row(
                SettingsRow::new("Alert strip")
                    .subtitle("toolbar banner")
                    .trailing(dot(
                        if self.alerts_on.get() { "on" } else { "off" },
                        if self.alerts_on.get() {
                            DotStatus::Ok
                        } else {
                            DotStatus::Off
                        },
                        self.alerts_on.get(),
                    )),
            )
            .row(
                SettingsRow::new("Reduced motion")
                    .subtitle("accessibility")
                    .trailing(dot(
                        if reduced { "on" } else { "off" },
                        if reduced {
                            DotStatus::Ok
                        } else {
                            DotStatus::Off
                        },
                        false,
                    )),
            )
            .with_text_painter(painter.clone());
        // Feeds — the same StatusDot gallery, now lamped from real
        // model state instead of seeded "online/degraded/offline".
        let paused = m.paused.get();
        let playing = m.media_playing.get();
        let feeds = SettingsGroup::new("Feeds")
            .carded(true)
            .row(
                SettingsRow::new("Production line")
                    .subtitle("line_running")
                    .trailing(dot(
                        if running { "live" } else { "stopped" },
                        if running {
                            DotStatus::Ok
                        } else {
                            DotStatus::Off
                        },
                        running,
                    )),
            )
            .row(
                SettingsRow::new("Alarms")
                    .subtitle("unacked annunciations")
                    .trailing(if unacked.is_empty() {
                        dot("clear", DotStatus::Ok, false)
                    } else {
                        dot(
                            &format!("{} unacked", unacked.len()),
                            if crit > 0 {
                                DotStatus::Error
                            } else {
                                DotStatus::Warning
                            },
                            true,
                        )
                    }),
            )
            .row(
                SettingsRow::new("Console")
                    .subtitle("operator lock")
                    .trailing(dot(
                        if locked { "locked" } else { "unlocked" },
                        if locked {
                            DotStatus::Warning
                        } else {
                            DotStatus::Ok
                        },
                        false,
                    )),
            )
            .row(
                SettingsRow::new("Media monitor")
                    .subtitle("loopback playback")
                    .trailing(dot(
                        if playing { "playing" } else { "idle" },
                        if playing {
                            DotStatus::Ok
                        } else {
                            DotStatus::Off
                        },
                        playing,
                    )),
            )
            .row(
                SettingsRow::new("Telemetry")
                    .subtitle("frame sampler")
                    .trailing(dot(
                        if paused { "paused" } else { "live" },
                        if paused {
                            DotStatus::Off
                        } else {
                            DotStatus::Ok
                        },
                        !paused,
                    )),
            )
            .with_text_painter(painter.clone());
        // Event feed — the shift log's tail, severity by channel:
        // OPS chatter reads Info, COMMS traffic Debug, and the SYSTEM
        // annunciator gets Warning so machine noise stands out.
        let mut feed = LogView::new()
            .max_lines(200)
            .with_text_painter(painter.clone());
        for e in m.shift_log.get().iter().rev().take(40).rev() {
            let sev = match e.channel {
                crate::domain::LogChannel::Ops => LogSeverity::Info,
                crate::domain::LogChannel::Comms => LogSeverity::Debug,
                crate::domain::LogChannel::System => LogSeverity::Warning,
            };
            feed.push(
                sev,
                format!(
                    "[{}] {} {}",
                    crate::zones::grid::shift_hhmm(e.minute),
                    e.channel.label(),
                    e.text
                ),
            );
        }
        // Cell KPIs — the plant OEE rollup plus per-cell values from
        // `assets` (BulletChart's value-vs-target strip, 85% goal).
        let mut kpi_children: Vec<Box<dyn Widget>> = vec![Box::new(
            BulletChart::new()
                .label("Plant OEE")
                .value((m.plant_oee() * 100.0) as f32)
                .target(85.0)
                .ranges([60.0, 80.0, 100.0])
                .with_text_painter(painter.clone()),
        )];
        for c in m
            .assets
            .get()
            .iter()
            .filter(|a| a.kind == crate::domain::AssetKind::Cell)
            .take(3)
        {
            kpi_children.push(Box::new(
                BulletChart::new()
                    .label(c.name)
                    .value((c.oee * 100.0) as f32)
                    .target(85.0)
                    .ranges([60.0, 80.0, 100.0])
                    .with_text_painter(painter.clone()),
            ));
        }
        let kpis = Flex::column().gap(4.0).children(kpi_children);
        // Acoustic condition monitor — `acoustic.bands` drives both
        // the trace and the spectrum; the dominant frequency reads
        // out as text. `enabled(false)` on the waveform: its click is
        // a seek affordance with no drain path here.
        let ac = m.acoustic.get();
        let vibration = Flex::column().gap(4.0).children([
            Box::new(
                Waveform::new()
                    .label("Spindle vibration")
                    .peaks(ac.bands.iter().map(|b| *b as f32))
                    .enabled(false),
            ) as Box<dyn Widget>,
            Box::new(
                Spectrum::new()
                    .label("Band analysis")
                    .bands(ac.bands.iter().map(|b| *b as f32)),
            ),
            Box::new(Text::new(format!("dominant {:.0} Hz", ac.dominant_hz))),
        ]);
        // Jog readout — the pad displays the live axis target written
        // by the grid/camera jog controls (`jog.axis` is −1..1, the
        // pad's normalized frame is 0..1). `take_changed` has no
        // drain path inside an overlay, so it's a disabled readout —
        // honest position display, not a dead editor.
        let j = m.jog.get();
        let jog = Flex::column().gap(4.0).children([
            Box::new(
                XYPad::new()
                    .labels("X jog", "Y jog")
                    .value(
                        ((j.axis.0 + 1.0) * 0.5) as f32,
                        ((j.axis.1 + 1.0) * 0.5) as f32,
                    )
                    .enabled(false)
                    .with_text_painter(painter.clone()),
            ) as Box<dyn Widget>,
            Box::new(Text::new(format!(
                "axis ({:+.2}, {:+.2}) · tilt {:+.2} · zoom {:.1}×",
                j.axis.0, j.axis.1, j.tilt, j.zoom
            ))),
        ]);
        // Board load — the sim's cpu/mem gauges, labeled for what
        // they are (no cabinet-temperature or UPS-reserve fiction).
        let environment = Flex::column().gap(4.0).children([
            Box::new(
                Thermometer::new()
                    .label("line load %")
                    .range(0.0, 100.0)
                    .value((m.cpu.get() * 100.0) as f32)
                    .warning(0.7)
                    .critical(0.9),
            ) as Box<dyn Widget>,
            Box::new(
                Battery::new()
                    .label("memory pool")
                    .level(m.mem.get() as f32)
                    .charging(false),
            ),
        ]);
        // Line noise — the acoustic monitor's stereo level. The
        // Equalizer that used to sit beside it is cut: its faders are
        // editors with no drain path inside an overlay.
        let noise = VuMeter::new()
            .label("monitor level")
            .channels(2)
            .levels([ac.level.0 as f32, ac.level.1 as f32]);
        // Shift clock — `shift_minute` as time-of-day (shift starts
        // 06:00, `zones::telemetry::shift_time`'s convention).
        // `running(false)`: the widget's own tick-advance never runs
        // in an overlay — the model drives the displayed time.
        let min = m.shift_minute.get();
        let clock = Flex::column().gap(4.0).children([
            Box::new(
                DigitalClock::new()
                    .time(Time {
                        hour: (6 + min / 60) % 24,
                        minute: min % 60,
                    })
                    .running(false),
            ) as Box<dyn Widget>,
            Box::new(Text::new(format!("shift +{min:03}m"))),
        ]);
        // Load history — the shared cpu/mem rings every trend chart
        // in the app reads (`HISTORY_LEN` window, newest last).
        let mut load = StripChart::new().label("line load (cpu)").range(0.0, 1.0);
        load.extend(m.cpu_hist.get().iter().map(|v| *v as f32));
        let mut pool = StripChart::new().label("memory pool").range(0.0, 1.0);
        pool.extend(m.mem_hist.get().iter().map(|v| *v as f32));
        let telemetry = Flex::column()
            .gap(4.0)
            .children([Box::new(load) as Box<dyn Widget>, Box::new(pool)]);
        // Shift-log tail — the real entries, not a seeded scrollback.
        // Typed input still echoes locally (Terminal owns its own
        // input line) but `take_submitted` has no drain path, so the
        // disclosure is titled for what it is: a tail view.
        let mut console = Terminal::new()
            .prompt("log>")
            .label("shift log tail")
            .with_text_painter(painter.clone());
        for e in m.shift_log.get().iter().rev().take(12).rev() {
            console.write(format!(
                "[{}] {} {}",
                crate::zones::grid::shift_hhmm(e.minute),
                e.channel.label(),
                e.text
            ));
        }
        let content = Flex::column().gap(8.0).children([
            Box::new(
                Banner::new(sev, msg)
                    .dismissible(false)
                    .with_text_painter(painter.clone()),
            ) as Box<dyn Widget>,
            Box::new(
                Disclosure::new("Selection")
                    .child(Text::new(sel))
                    .with_text_painter(painter.clone()),
            ),
            Box::new(
                Disclosure::new("Alerts")
                    .child(alerts)
                    .with_text_painter(painter.clone()),
            ),
            Box::new(prefs),
            Box::new(feeds),
            Box::new(
                Disclosure::new("Cell KPIs")
                    .child(kpis)
                    .with_text_painter(painter.clone()),
            ),
            Box::new(
                Disclosure::new("Vibration")
                    .child(vibration)
                    .with_text_painter(painter.clone()),
            ),
            Box::new(
                Disclosure::new("Jog")
                    .child(jog)
                    .with_text_painter(painter.clone()),
            ),
            Box::new(
                Disclosure::new("Board load")
                    .child(environment)
                    .with_text_painter(painter.clone()),
            ),
            Box::new(
                Disclosure::new("Line noise")
                    .child(noise)
                    .with_text_painter(painter.clone()),
            ),
            Box::new(
                Disclosure::new("Shift clock")
                    .child(clock)
                    .with_text_painter(painter.clone()),
            ),
            Box::new(
                Disclosure::new("Load history")
                    .child(telemetry)
                    .with_text_painter(painter.clone()),
            ),
            Box::new(
                Disclosure::new("Shift log")
                    .child(console)
                    .with_text_painter(painter.clone()),
            ),
            Box::new(
                Disclosure::new("Event feed")
                    .child(feed)
                    .with_text_painter(painter.clone()),
            ),
        ]);
        Drawer::new("Inspector")
            .width(300.0)
            .content(content)
            .close_sink(Arc::clone(&self.drawer_close))
            .with_text_painter(painter)
    }
}

impl Widget for ShellOverlays {
    fn debug_name(&self) -> &'static str {
        "ShellOverlays"
    }

    fn measure(&mut self, _cx: &mut LayoutContext, _c: LayoutConstraints) -> Vec2 {
        Vec2::ZERO
    }

    fn layout(&mut self, _cx: &mut LayoutContext, _b: Rect) {}

    fn tick(&mut self, _dt: std::time::Duration) -> bool {
        // Drain the shared inbox + reap expired toasts — the overlay
        // entry itself is never ticked, so this owner-side pump is the
        // only driver.
        if let Ok(mut q) = self.toast_inbox.lock() {
            for t in q.drain(..) {
                self.toasts.push(t);
            }
        }
        self.toasts.tick()
    }

    fn sync_overlay(&mut self, overlay: &mut OverlayLayer) {
        // --- Toasts -------------------------------------------------
        if self.toasts.is_empty() {
            if let Some(id) = self.toast_id.take() {
                overlay.close(id);
            }
        } else {
            match self.toast_id {
                Some(id) if overlay.is_open(id) => {
                    overlay.replace_content(id, Box::new(self.toasts.clone()));
                }
                _ => {
                    self.toast_id = Some(overlay.open_with(
                        Box::new(self.toasts.clone()),
                        OverlayAnchor::Viewport {
                            h: ViewportAlign::End,
                            v: ViewportAlign::End,
                            margin: 12.0,
                        },
                        OverlayOptions::passthrough(),
                    ));
                }
            }
        }

        // --- About dialog -------------------------------------------
        // Layer-level dismissal (Escape) clears our id.
        if let Some(id) = self.dialog_id {
            if !overlay.is_open(id) {
                self.dialog_id = None;
                self.about_req.set(false);
            }
        }
        // A committed button closes the dialog.
        if self.dialog_id.is_some() {
            if let Ok(mut cell) = self.dialog_resp.lock() {
                if cell.take().is_some() {
                    if let Some(id) = self.dialog_id.take() {
                        overlay.close(id);
                    }
                    self.about_req.set(false);
                }
            }
        }
        if self.about_req.get() && self.dialog_id.is_none() {
            self.dialog_id = Some(overlay.open_with(
                Box::new(self.about_dialog()),
                OverlayAnchor::Center,
                OverlayOptions::modal(),
            ));
        }

        // --- Inspector drawer ---------------------------------------
        if let Some(id) = self.drawer_id {
            if !overlay.is_open(id) {
                self.drawer_id = None;
                self.drawer_sig = None;
                self.inspector_req.set(false);
            }
        }
        if self.drawer_id.is_some() {
            if let Ok(mut cell) = self.drawer_close.lock() {
                if std::mem::take(&mut *cell) {
                    if let Some(id) = self.drawer_id.take() {
                        overlay.close(id);
                    }
                    self.drawer_sig = None;
                    self.inspector_req.set(false);
                }
            }
        }
        if let Some(id) = self.drawer_id {
            // The entry never ticks, so the drawer's content is
            // re-seated here — rebuild from live model reads and
            // `replace_content` when the signature moved. Fires only
            // on a real change: a re-seat drops in-widget state
            // (Disclosure open flags, scroll offsets), matching the
            // `sync_message_list` re-seat tradeoff.
            let sig = self.drawer_signature();
            if self.drawer_sig != Some(sig) {
                self.drawer_sig = Some(sig);
                overlay.replace_content(id, Box::new(self.inspector_drawer()));
            }
        }
        if self.inspector_req.get() && self.drawer_id.is_none() {
            self.drawer_sig = Some(self.drawer_signature());
            self.drawer_id = Some(overlay.open_with(
                Box::new(self.inspector_drawer()),
                OverlayAnchor::EdgeRight,
                OverlayOptions::modal().light_dismiss(),
            ));
        }
    }

    fn event(&mut self, _cx: &mut EventContext) -> EventResponse {
        EventResponse::Ignored
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::GenericContainer);
        node.set_label("shell overlays");
    }

    fn paint(&self, _cx: &mut PaintContext) {}
}
