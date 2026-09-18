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
use martensite::widgets::{Drawer, Flex, Text};

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
