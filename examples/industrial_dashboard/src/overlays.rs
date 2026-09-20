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
use martensite::widgets::{Banner, Dialog, Disclosure, Severity, Switch, Toast, ToastHost};
use martensite::widgets::{
    Battery, DigitalClock, Equalizer, StripChart, Terminal, Thermometer, Time, VuMeter,
};
use martensite::widgets::{BulletChart, Spectrum, Waveform, XYPad};
use martensite::widgets::{Drawer, Flex, Text};
use martensite::widgets::{KeyCapture, Rating, Segmented, SettingsGroup, SettingsRow, SpinBox};
use martensite::widgets::{LogSeverity, LogView, Status as DotStatus, StatusDot};

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
    /// The drawer content's alert toggle — bound to the app's
    /// `alerts_on` cell so flipping it in the drawer drives the
    /// telemetry banner live.
    alerts_on: Signal<bool>,
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
    ) -> Self {
        Self {
            about_req,
            inspector_req,
            alerts_on,
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

    /// Builds the inspector drawer — a `Flex` column of facade widgets
    /// (banner, disclosures, a live switch) inside the drawer surface.
    fn inspector_drawer(&self) -> Drawer {
        let painter = martensite::text_paint::shared_painter();
        // Mirrors `alerts_on` at open time — the drawer's content tree
        // owns the switch, so in-drawer flips are visual-only.
        let alerts = Switch::new("row alerts")
            .on(self.alerts_on.get())
            .with_text_painter(painter.clone());
        // Dogfood the preferences widgets — a carded SettingsGroup
        // whose rows carry the newer facade controls as trailing
        // editors.
        let prefs = SettingsGroup::new("Inspector preferences")
            .carded(true)
            .row(
                SettingsRow::new("Density")
                    .subtitle("Row height preset")
                    .trailing(
                        Segmented::new()
                            .options(["Compact", "Normal", "Roomy"])
                            .selected(1)
                            .with_text_painter(painter.clone()),
                    ),
            )
            .row(
                SettingsRow::new("Refresh cadence")
                    .subtitle("Telemetry tick rate")
                    .trailing(
                        SpinBox::new()
                            .range(1.0, 60.0)
                            .suffix(" Hz")
                            .with_text_painter(painter.clone()),
                    ),
            )
            .row(
                SettingsRow::new("Confidence floor")
                    .subtitle("Minimum score for flags")
                    .trailing(
                        Rating::new()
                            .max(5)
                            .value(3.0)
                            .with_text_painter(painter.clone()),
                    ),
            )
            .row(
                SettingsRow::new("Focus shortcut")
                    .subtitle("Capture a key chord")
                    .trailing(
                        KeyCapture::new()
                            .placeholder("press keys…")
                            .with_text_painter(painter.clone()),
                    ),
            )
            .with_text_painter(painter.clone());
        // Dogfood StatusDot — a feeds card whose rows carry live-ish
        // status lamps (pulse on the healthy feed).
        let feeds = SettingsGroup::new("Feeds")
            .carded(true)
            .row(
                SettingsRow::new("Field bus")
                    .subtitle("Modbus heartbeat")
                    .trailing(
                        StatusDot::new("online")
                            .status(DotStatus::Ok)
                            .pulse(true)
                            .with_text_painter(painter.clone()),
                    ),
            )
            .row(
                SettingsRow::new("Cell telemetry")
                    .subtitle("RF uplink")
                    .trailing(
                        StatusDot::new("degraded")
                            .status(DotStatus::Warning)
                            .with_text_painter(painter.clone()),
                    ),
            )
            .row(
                SettingsRow::new("Remote archive")
                    .subtitle("Nightly sync target")
                    .trailing(
                        StatusDot::new("offline")
                            .status(DotStatus::Off)
                            .with_text_painter(painter.clone()),
                    ),
            )
            .with_text_painter(painter.clone());
        // Dogfood LogView — a small event feed under a disclosure.
        let mut feed = LogView::new()
            .max_lines(200)
            .with_text_painter(painter.clone());
        feed.push(LogSeverity::Info, "inspector attached");
        feed.push(LogSeverity::Info, "telemetry tick 60 Hz");
        feed.push(LogSeverity::Warning, "cell 3 uplink jitter 240 ms");
        feed.push(LogSeverity::Error, "archive sync stalled — retrying");
        feed.push(LogSeverity::Debug, "drawer opened via toolbar");
        // Dogfood BulletChart — cell KPIs reading value-vs-target
        // over qualitative bands, the standard OEE strip.
        let kpis = Flex::column().gap(4.0).children([
            Box::new(
                BulletChart::new()
                    .label("OEE")
                    .value(78.0)
                    .target(85.0)
                    .ranges([60.0, 80.0, 100.0])
                    .with_text_painter(painter.clone()),
            ) as Box<dyn Widget>,
            Box::new(
                BulletChart::new()
                    .label("Yield")
                    .value(94.0)
                    .target(92.0)
                    .ranges([70.0, 85.0, 100.0])
                    .with_text_painter(painter.clone()),
            ),
            Box::new(
                BulletChart::new()
                    .label("Throughput")
                    .value(61.0)
                    .target(75.0)
                    .ranges([50.0, 70.0, 100.0])
                    .with_text_painter(painter.clone()),
            ),
        ]);
        // Dogfood Waveform + Spectrum — a vibration monitor pairing
        // the amplitude signature with its band decomposition.
        let vibration = Flex::column().gap(4.0).children([
            Box::new(
                Waveform::new()
                    .label("Spindle vibration")
                    .peaks([
                        0.2, 0.35, 0.6, 0.4, 0.9, 0.55, 0.3, 0.7, 0.45, 0.25, 0.8, 0.5, 0.35, 0.6,
                        0.3, 0.2,
                    ])
                    .position(0.4),
            ) as Box<dyn Widget>,
            Box::new(
                Spectrum::new()
                    .label("Band analysis")
                    .bands([0.3, 0.55, 0.8, 0.65, 0.4, 0.7, 0.35, 0.2]),
            ),
        ]);
        // Dogfood XYPad — a robot-jog pad for cell positioning.
        let jog = XYPad::new()
            .labels("X jog", "Y jog")
            .value(0.5, 0.5)
            .with_text_painter(painter.clone());
        // Dogfood Thermometer + Battery — cabinet environment and
        // the UPS state in one disclosure.
        let environment = Flex::column().gap(4.0).children([
            Box::new(
                Thermometer::new()
                    .label("Cabinet temp")
                    .range(-10.0, 80.0)
                    .value(43.0)
                    .warning(0.7)
                    .critical(0.9),
            ) as Box<dyn Widget>,
            Box::new(
                Battery::new()
                    .label("UPS reserve")
                    .level(0.72)
                    .charging(true),
            ),
        ]);
        // Dogfood VuMeter + Equalizer — a channel-strip pair for
        // the cell's audio alarm bus.
        let audio_bus = Flex::column().gap(4.0).children([
            Box::new(
                VuMeter::new()
                    .label("Alarm bus")
                    .channels(2)
                    .levels([0.62, 0.45]),
            ) as Box<dyn Widget>,
            Box::new(
                Equalizer::new()
                    .faders(6)
                    .bands([0.5, 0.65, 0.4, 0.55, 0.7, 0.5]),
            ),
        ]);
        // Dogfood DigitalClock — shift-clock readout.
        let clock = DigitalClock::new()
            .time(Time {
                hour: 14,
                minute: 32,
            })
            .running(true);
        // Dogfood StripChart — the scrolling pressure trace.
        let mut telemetry = StripChart::new()
            .label("Hydraulic pressure")
            .range(0.0, 10.0);
        telemetry.extend([
            4.2, 4.4, 4.1, 4.6, 4.9, 5.2, 4.8, 5.5, 5.8, 5.4, 5.0, 5.6, 6.1, 5.7, 5.3, 5.9, 6.4,
            6.0, 5.5, 5.1,
        ]);
        // Dogfood Terminal — a diagnostics console with a seeded
        // scrollback.
        let mut console = Terminal::new()
            .prompt("cell>")
            .lines([
                "boot ok — plc v2.4.1",
                "field bus attached (modbus:502)",
                "cell> status",
                "3 axes online, pressure nominal",
            ])
            .with_text_painter(painter.clone());
        console.write("watchdog armed");
        let content = Flex::column().gap(8.0).children([
            Box::new(
                Banner::new(Severity::Info, "Inspector attached")
                    .dismissible(false)
                    .with_text_painter(painter.clone()),
            ) as Box<dyn Widget>,
            Box::new(
                Disclosure::new("Selection")
                    .child(Text::new("focused panel, sort, and filter state"))
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
                Disclosure::new("Environment")
                    .child(environment)
                    .with_text_painter(painter.clone()),
            ),
            Box::new(
                Disclosure::new("Alarm bus")
                    .child(audio_bus)
                    .with_text_painter(painter.clone()),
            ),
            Box::new(
                Disclosure::new("Shift clock")
                    .child(clock)
                    .with_text_painter(painter.clone()),
            ),
            Box::new(
                Disclosure::new("Pressure")
                    .child(telemetry)
                    .with_text_painter(painter.clone()),
            ),
            Box::new(
                Disclosure::new("Console")
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
                self.inspector_req.set(false);
            }
        }
        if self.drawer_id.is_some() {
            if let Ok(mut cell) = self.drawer_close.lock() {
                if std::mem::take(&mut *cell) {
                    if let Some(id) = self.drawer_id.take() {
                        overlay.close(id);
                    }
                    self.inspector_req.set(false);
                }
            }
        }
        if self.inspector_req.get() && self.drawer_id.is_none() {
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
