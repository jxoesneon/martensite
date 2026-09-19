//! `MenuButton` — a button face that opens a menu popup
//! (GTK `MenuButton`, WinUI `DropDownButton`, Ant `Dropdown.Button`).
//!
//! Pressing the face (or `Enter`/`Space`/`ArrowDown`, or AT
//! `Expand`/`Click`) opens the root [`Menu`] at a
//! `Bounds` anchor below the face through the shared [`MenuStack`]
//! machinery — submenus, checkables, radios, separators, and
//! `Escape`/outside-press dismissal all behave exactly as in
//! `MenuBar`/`ContextMenu`. Activations park as [`MenuPath`]s in
//! [`MenuButton::take_activated`].
//!
//! The face emits `Role::Button` with `aria-haspopup="menu"` and
//! `aria-expanded`.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::{MenuButton, MenuItem};
//!
//! let mut b = MenuButton::new("Actions", vec![
//!     MenuItem::action("Rename"),
//!     MenuItem::separator(),
//!     MenuItem::action("Delete"),
//! ]);
//! assert_eq!(b.item_count(), 3);
//! ```

use std::sync::{Arc, Mutex};

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, NodeFlags, PaintContext,
    PointerButton, Rect, RenderMinimum, SemanticAction, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::widgets::menu::{MenuItem, MenuPath, MenuStack, MenuState};
use martensite_core::overlay::{OverlayAnchor, OverlayLayer};

const FACE_H_PT: f32 = 28.0;
const FONT_PT: f32 = 13.0;
const PAD_X_PT: f32 = 12.0;
const CHEV_W_PT: f32 = 16.0;
const RADIUS_PT: f32 = 6.0;
const FACE: [u8; 4] = [247, 247, 249, 255];
const FACE_HOVER: [u8; 4] = [238, 240, 245, 255];
const FACE_DOWN: [u8; 4] = [228, 231, 238, 255];
const EDGE: [u8; 4] = [0, 0, 0, 28];
const INK: [u8; 4] = [30, 30, 34, 255];
const MUTED: [u8; 4] = [110, 110, 118, 255];

/// A button that opens a menu — see the module docs.
///
/// ```
/// use martensite::widgets::{MenuButton, MenuItem};
///
/// let b = MenuButton::new("Menu", vec![MenuItem::action("A")]);
/// assert_eq!(b.label, "Menu");
/// ```
pub struct MenuButton {
    /// Face text.
    pub label: String,
    /// When `false` the face is dimmed and inert.
    pub enabled: bool,
    stack: MenuStack,
    hovered: bool,
    pressed: bool,
    focused: bool,
    cached_bounds: Rect,
    text_painter: Option<crate::text_paint::SharedTextPainter>,
    scale: f32,
}

impl MenuButton {
    /// Creates a button over `items`.
    ///
    /// ```
    /// use martensite::widgets::{MenuButton, MenuItem};
    ///
    /// let b = MenuButton::new("File", vec![MenuItem::action("New")]);
    /// assert_eq!(b.item_count(), 1);
    /// ```
    pub fn new(label: impl Into<String>, items: Vec<MenuItem>) -> Self {
        Self {
            label: label.into(),
            enabled: true,
            stack: MenuStack::new(Arc::new(Mutex::new(MenuState::new(items))), None),
            hovered: false,
            pressed: false,
            focused: false,
            cached_bounds: Rect::default(),
            text_painter: None,
            scale: 1.0,
        }
    }

    /// Enables or disables the button.
    ///
    /// ```
    /// use martensite::widgets::{MenuButton, MenuItem};
    ///
    /// let b = MenuButton::new("M", vec![MenuItem::action("A")]).enabled(false);
    /// assert!(!b.enabled);
    /// ```
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Shares a shaped-text painter so face and popup rows emit real
    /// glyph runs.
    ///
    /// ```
    /// use martensite::widgets::{MenuButton, MenuItem};
    ///
    /// let b = MenuButton::new("M", vec![MenuItem::action("A")]);
    /// let _ = b.label;
    /// ```
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.stack = MenuStack::new(self.stack.shared(), Some(painter.clone()));
        self.text_painter = Some(painter);
        self
    }

    /// The root menu's items (live model — checkable/radio state
    /// mutates as the user toggles).
    ///
    /// ```
    /// use martensite::widgets::{MenuButton, MenuItem};
    ///
    /// let b = MenuButton::new("M", vec![MenuItem::checkable("On", true)]);
    /// assert!(matches!(b.items()[0], MenuItem::Checkable { checked: true, .. }));
    /// ```
    pub fn items(&self) -> Vec<MenuItem> {
        self.stack
            .shared()
            .lock()
            .expect("menu state poisoned")
            .items()
            .to_vec()
    }

    /// Replaces the root menu's items.
    ///
    /// ```
    /// use martensite::widgets::{MenuButton, MenuItem};
    ///
    /// let mut b = MenuButton::new("M", vec![]);
    /// b.set_items(vec![MenuItem::action("A")]);
    /// assert_eq!(b.item_count(), 1);
    /// ```
    pub fn set_items(&mut self, items: Vec<MenuItem>) {
        self.stack
            .shared()
            .lock()
            .expect("menu state poisoned")
            .set_items(items);
    }

    /// Root item count.
    ///
    /// ```
    /// use martensite::widgets::{MenuButton, MenuItem};
    ///
    /// let b = MenuButton::new("M", vec![MenuItem::action("A"), MenuItem::action("B")]);
    /// assert_eq!(b.item_count(), 2);
    /// ```
    pub fn item_count(&self) -> usize {
        self.stack
            .shared()
            .lock()
            .expect("menu state poisoned")
            .items()
            .len()
    }

    /// Whether the menu is logically open.
    ///
    /// ```
    /// use martensite::widgets::{MenuButton, MenuItem};
    ///
    /// let mut b = MenuButton::new("M", vec![MenuItem::action("A")]);
    /// assert!(!b.is_open());
    /// b.open();
    /// assert!(b.is_open());
    /// ```
    pub fn is_open(&self) -> bool {
        self.stack.is_open()
    }

    /// Opens the menu below the face.
    ///
    /// ```
    /// use martensite::widgets::{MenuButton, MenuItem};
    ///
    /// let mut b = MenuButton::new("M", vec![MenuItem::action("A")]);
    /// b.open();
    /// assert!(b.is_open());
    /// ```
    pub fn open(&mut self) {
        if !self.enabled {
            return;
        }
        self.stack
            .open_at(OverlayAnchor::Bounds(self.cached_bounds), false);
    }

    /// Closes the menu logically; `sync_overlay` drops the entries.
    ///
    /// ```
    /// use martensite::widgets::{MenuButton, MenuItem};
    ///
    /// let mut b = MenuButton::new("M", vec![MenuItem::action("A")]);
    /// b.open();
    /// b.close();
    /// assert!(!b.is_open());
    /// ```
    pub fn close(&mut self) {
        self.stack.close();
    }

    /// Toggles the menu.
    ///
    /// ```
    /// use martensite::widgets::{MenuButton, MenuItem};
    ///
    /// let mut b = MenuButton::new("M", vec![MenuItem::action("A")]);
    /// b.toggle();
    /// assert!(b.is_open());
    /// b.toggle();
    /// assert!(!b.is_open());
    /// ```
    pub fn toggle(&mut self) {
        if self.is_open() {
            self.close();
        } else {
            self.open();
        }
    }

    /// Drains the last activation path (`[root_row, submenu_row, …]`).
    ///
    /// ```
    /// use martensite::widgets::{MenuButton, MenuItem};
    ///
    /// let mut b = MenuButton::new("M", vec![MenuItem::action("A")]);
    /// assert_eq!(b.take_activated(), None);
    /// ```
    pub fn take_activated(&mut self) -> Option<MenuPath> {
        self.stack.take_activated()
    }

    /// Reconciles the popup stack — call once per frame before
    /// `OverlayLayer::layout_pass` (the arena does this for registered
    /// widgets).
    ///
    /// ```
    /// use martensite::widgets::{MenuButton, MenuItem};
    /// use martensite_core::overlay::OverlayLayer;
    ///
    /// let mut b = MenuButton::new("M", vec![MenuItem::action("A")]);
    /// let mut o = OverlayLayer::new();
    /// b.sync_overlay(&mut o);
    /// ```
    pub fn sync_overlay(&mut self, overlay: &mut OverlayLayer) {
        self.stack.sync(overlay);
    }
}

impl Widget for MenuButton {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            cx.pt(120.0).min(constraints.max_size.x.max(0.0)),
            cx.pt(FACE_H_PT).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(48.0, FACE_H_PT)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.cached_bounds = bounds;
        self.scale = cx.scale;
        if self.enabled {
            cx.hot.flags |= NodeFlags::FOCUSABLE;
        } else {
            cx.hot.flags.remove(NodeFlags::FOCUSABLE);
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Button);
        node.set_label(self.label.as_str());
        node.set_has_popup(accesskit::HasPopup::Menu);
        node.set_expanded(self.is_open());
        node.add_action(accesskit::Action::Click);
        node.add_action(accesskit::Action::Focus);
        node.add_action(accesskit::Action::Expand);
        node.add_action(accesskit::Action::Collapse);
        if !self.enabled {
            node.set_disabled();
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.enabled {
            return EventResponse::Ignored;
        }
        match cx.event {
            WidgetEvent::PointerMoved { position } => {
                let h = self.cached_bounds.contains(*position);
                if h != self.hovered {
                    self.hovered = h;
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerLeave => {
                if self.hovered || self.pressed {
                    self.hovered = false;
                    self.pressed = false;
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                ..
            } if self.cached_bounds.contains(*position) => {
                self.pressed = true;
                EventResponse::CapturePointer
            }
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position,
            } => {
                let was = std::mem::take(&mut self.pressed);
                if was && self.cached_bounds.contains(*position) {
                    self.toggle();
                }
                EventResponse::ReleasePointer
            }
            WidgetEvent::KeyPressed { key, .. } => match key.as_str() {
                "Enter" | " " | "Space" | "ArrowDown" => {
                    self.toggle();
                    EventResponse::RequestRepaint
                }
                "Escape" if self.is_open() => {
                    // Normally unreachable — the OverlayLayer eats
                    // Escape first. Ownerless-embedded fallback.
                    self.close();
                    EventResponse::RequestRepaint
                }
                _ => EventResponse::Ignored,
            },
            WidgetEvent::SemanticAction(action) => match action {
                SemanticAction::Expand => {
                    self.open();
                    EventResponse::RequestRepaint
                }
                SemanticAction::Collapse => {
                    self.close();
                    EventResponse::RequestRepaint
                }
                SemanticAction::Click => {
                    self.toggle();
                    EventResponse::RequestRepaint
                }
                SemanticAction::Focus => EventResponse::CaptureFocus,
                _ => EventResponse::Ignored,
            },
            WidgetEvent::FocusGained => {
                self.focused = true;
                EventResponse::RequestRepaint
            }
            WidgetEvent::FocusLost => {
                self.focused = false;
                EventResponse::RequestRepaint
            }
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let r = kurbo::Rect::new(
            f64::from(self.cached_bounds.min_x()),
            f64::from(self.cached_bounds.min_y()),
            f64::from(self.cached_bounds.max_x()),
            f64::from(self.cached_bounds.max_y()),
        );
        let shape = martensite_core::shape::Shape::rounded(cx.pt(RADIUS_PT));
        let face = if !self.enabled {
            cx.color(TokenKey::SurfaceColor, FACE)
        } else if self.pressed || self.is_open() {
            cx.color(TokenKey::SecondaryColor, FACE_DOWN)
        } else if self.hovered || self.focused {
            cx.color(TokenKey::SecondaryColor, FACE_HOVER)
        } else {
            cx.color(TokenKey::SurfaceColor, FACE)
        };
        cx.list.push_fill_shape(r, &shape, face);
        cx.list.push_stroke_shape(
            r,
            &shape,
            cx.pt(1.0),
            cx.color(TokenKey::DividerColor, EDGE),
        );

        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let font = cx.pt(FONT_PT);
        let pad = cx.pt(PAD_X_PT);
        let chev_w = cx.pt(CHEV_W_PT);
        let mut ink = cx.color(TokenKey::TextColor, INK);
        if !self.enabled {
            ink[3] = 130;
        }
        let text_w = painter
            .and_then(|p| p.measure_text(&self.label, font))
            .unwrap_or(self.label.len() as f32 * font * 0.55);
        let lx = self.cached_bounds.min_x() + pad;
        let ly = self.cached_bounds.min_y() + (self.cached_bounds.height() - font) / 2.0;
        crate::text_paint::paint_label_clipped(
            painter,
            cx.list,
            kurbo::Rect::new(
                f64::from(lx),
                f64::from(self.cached_bounds.min_y()),
                f64::from((lx + text_w).min(self.cached_bounds.max_x() - chev_w)),
                f64::from(self.cached_bounds.max_y()),
            ),
            kurbo::Point::new(f64::from(lx), f64::from(ly)),
            &self.label,
            font,
            ink,
        );
        // `▾` affordance.
        crate::text_paint::paint_label(
            painter,
            cx.list,
            kurbo::Point::new(
                f64::from(self.cached_bounds.max_x() - pad - chev_w * 0.6),
                f64::from(ly),
            ),
            "▾",
            font,
            cx.color(TokenKey::TextMutedColor, MUTED),
        );
    }

    fn sync_overlay(&mut self, overlay: &mut OverlayLayer) {
        MenuButton::sync_overlay(self, overlay);
    }
}

impl std::fmt::Debug for MenuButton {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MenuButton")
            .field("label", &self.label)
            .field("open", &self.is_open())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn button() -> MenuButton {
        MenuButton::new(
            "Actions",
            vec![
                MenuItem::action("Rename"),
                MenuItem::separator(),
                MenuItem::action("Delete"),
            ],
        )
    }

    fn laid_out(b: &mut MenuButton) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        b.layout(&mut cx, Rect::new(10.0, 10.0, 120.0, 28.0));
    }

    fn overlay() -> OverlayLayer {
        let mut o = OverlayLayer::new();
        o.set_viewport(Rect::new(0.0, 0.0, 800.0, 600.0));
        o
    }

    fn ev<'a>(event: &'a WidgetEvent) -> EventContext<'a> {
        EventContext {
            event,
            bounds: Rect::new(10.0, 10.0, 120.0, 28.0),
            scale: 1.0,
        }
    }

    #[test]
    fn press_release_toggles() {
        let mut b = button();
        laid_out(&mut b);
        b.event(&mut ev(&WidgetEvent::PointerPressed {
            position: Vec2::new(30.0, 20.0),
            button: PointerButton::Primary,
            count: 1,
        }));
        b.event(&mut ev(&WidgetEvent::PointerReleased {
            position: Vec2::new(30.0, 20.0),
            button: PointerButton::Primary,
        }));
        assert!(b.is_open());
    }

    #[test]
    fn open_reconciles_popup() {
        let mut b = button();
        laid_out(&mut b);
        let mut o = overlay();
        b.open();
        b.sync_overlay(&mut o);
        o.layout_pass();
        assert_eq!(o.len(), 1);
    }

    #[test]
    fn arrow_down_opens() {
        let mut b = button();
        laid_out(&mut b);
        b.event(&mut ev(&WidgetEvent::KeyPressed {
            key: "ArrowDown".into(),
            repeat: false,
        }));
        assert!(b.is_open());
    }

    #[test]
    fn semantic_expand_collapse() {
        let mut b = button();
        laid_out(&mut b);
        b.event(&mut ev(&WidgetEvent::SemanticAction(
            SemanticAction::Expand,
        )));
        assert!(b.is_open());
        b.event(&mut ev(&WidgetEvent::SemanticAction(
            SemanticAction::Collapse,
        )));
        assert!(!b.is_open());
    }

    #[test]
    fn disabled_inert() {
        let mut b = button().enabled(false);
        laid_out(&mut b);
        assert_eq!(
            b.event(&mut ev(&WidgetEvent::KeyPressed {
                key: "Enter".into(),
                repeat: false,
            })),
            EventResponse::Ignored
        );
        assert!(!b.is_open());
    }

    #[test]
    fn items_roundtrip() {
        let mut b = button();
        assert_eq!(b.item_count(), 3);
        b.set_items(vec![MenuItem::action("Only")]);
        assert_eq!(b.items().len(), 1);
    }
}
