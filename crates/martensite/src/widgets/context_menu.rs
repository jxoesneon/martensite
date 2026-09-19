//! `ContextMenu` widget: a right-click context-menu trigger wrapper.
//!
//! Wraps a content child; a secondary-button press anywhere on the
//! child (or an AT `ShowContextMenu` action) opens a
//! [`Menu`](crate::widgets::menu::Menu) popup in the
//! [`OverlayLayer`](martensite_core::overlay::OverlayLayer) anchored at
//! the pointer. Item activation lands in
//! [`ContextMenu::take_activated`] as a
//! [`MenuPath`](crate::widgets::menu::MenuPath) — the same out-seam as
//! [`MenuBar`](crate::widgets::menu_bar::MenuBar) and the dashboard's
//! hand-rolled `ContextMenu` this supersedes.
//!
//! The child receives all other input normally. Keyboard navigation
//! inside the popup is handled by the popup itself (the overlay offers
//! non-`Escape` keys to the topmost entry first); the wrapper keeps a
//! fallback path for ownerless-embedded use.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::{ContextMenu, MenuItem, Text};
//!
//! let menu = ContextMenu::new(
//!     Text::new("right-click me"),
//!     vec![MenuItem::action("Copy"), MenuItem::action("Paste")],
//! );
//! assert!(!menu.is_open());
//! ```

use std::sync::{Arc, Mutex};

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::overlay::{OverlayAnchor, OverlayLayer};
use martensite_core::widget::{
    A11yEmittedNode, EventContext, EventResponse, LayoutConstraints, LayoutContext, OverlayA11yRef,
    PaintContext, PointerButton, SemanticAction, Widget, WidgetEvent,
};
use martensite_core::{NodeFlags, Rect};

use crate::widgets::menu::{MenuItem, MenuPath, MenuStack, MenuState};

/// A context-menu trigger wrapping a content child.
///
/// The child is `child(0)` and receives input normally; the menu
/// itself lives in the overlay and is reconciled by
/// [`ContextMenu::sync_overlay`], which the arena calls once per
/// frame before `OverlayLayer::layout_pass`.
///
/// # Examples
///
/// ```
/// use martensite::widgets::{ContextMenu, MenuItem, Text};
///
/// let menu = ContextMenu::new(Text::new("content"), vec![MenuItem::action("Act")]);
/// assert_eq!(menu.item_count(), 1);
/// ```
pub struct ContextMenu {
    /// The wrapped content widget (internal child 0).
    child: Box<dyn Widget>,
    /// Popup-stack controller (shared menu state + overlay entries).
    stack: MenuStack,
    /// Wrapper bounds from the last layout pass.
    cached_bounds: Rect,
    /// Last seen pointer position — anchors the popup and the
    /// `ShowContextMenu` semantic action.
    last_pointer: Option<Vec2>,
    /// Shared shaped-text painter — propagates into the popup.
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl ContextMenu {
    /// Wraps `child` with a context menu over `items`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{ContextMenu, MenuItem, Text};
    ///
    /// let menu = ContextMenu::new(
    ///     Text::new("area"),
    ///     vec![MenuItem::action("Rename"), MenuItem::separator()],
    /// );
    /// assert_eq!(menu.item_count(), 2);
    /// ```
    pub fn new(child: impl Widget + 'static, items: Vec<MenuItem>) -> Self {
        Self {
            child: Box::new(child),
            stack: MenuStack::new(Arc::new(Mutex::new(MenuState::new(items))), None),
            cached_bounds: Rect::default(),
            last_pointer: None,
            text_painter: None,
        }
    }

    /// Shares a [`crate::text_paint::TextPainter`] so popup rows emit
    /// real glyph runs.
    #[must_use]
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.stack = MenuStack::new(self.stack.shared(), Some(painter.clone()));
        self.text_painter = Some(painter);
        self
    }

    /// The root menu's items (the live model — checkable/radio state
    /// mutates as the user toggles items).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{ContextMenu, MenuItem, Text};
    ///
    /// let menu = ContextMenu::new(Text::new("c"), vec![MenuItem::checkable("On", true)]);
    /// assert!(matches!(menu.items()[0], MenuItem::Checkable { checked: true, .. }));
    /// ```
    pub fn items(&self) -> Vec<MenuItem> {
        self.stack
            .shared()
            .lock()
            .expect("menu state poisoned")
            .items()
            .to_vec()
    }

    /// Number of root menu items.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{ContextMenu, MenuItem, Text};
    ///
    /// let menu = ContextMenu::new(Text::new("c"), vec![MenuItem::action("A")]);
    /// assert_eq!(menu.item_count(), 1);
    /// ```
    pub fn item_count(&self) -> usize {
        self.stack
            .shared()
            .lock()
            .expect("menu state poisoned")
            .items()
            .len()
    }

    /// Whether the menu popup is logically open.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{ContextMenu, MenuItem, Text};
    ///
    /// let mut menu = ContextMenu::new(Text::new("c"), vec![MenuItem::action("A")]);
    /// menu.open_at(glam::Vec2::new(20.0, 30.0));
    /// assert!(menu.is_open());
    /// ```
    #[inline]
    pub fn is_open(&self) -> bool {
        self.stack.is_open()
    }

    /// The root popup's overlay entry id, if open.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{ContextMenu, MenuItem, Text};
    ///
    /// let menu = ContextMenu::new(Text::new("c"), vec![MenuItem::action("A")]);
    /// assert_eq!(menu.popup_id(), None);
    /// ```
    #[inline]
    pub fn popup_id(&self) -> Option<u64> {
        self.stack.root_id()
    }

    /// Opens the menu anchored at `position` (window space) — the
    /// programmatic equivalent of a secondary-button press.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{ContextMenu, MenuItem, Text};
    ///
    /// let mut menu = ContextMenu::new(Text::new("c"), vec![MenuItem::action("A")]);
    /// menu.open_at(glam::Vec2::new(10.0, 10.0));
    /// assert!(menu.is_open());
    /// ```
    pub fn open_at(&mut self, position: Vec2) {
        self.last_pointer = Some(position);
        self.stack
            .open_at(OverlayAnchor::Pointer(position), false);
    }

    /// Opens the menu at the last pointer position (keyboard/AT path),
    /// or the bounds center when no pointer has been seen.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{ContextMenu, MenuItem, Text};
    ///
    /// let mut menu = ContextMenu::new(Text::new("c"), vec![MenuItem::action("A")]);
    /// menu.open();
    /// assert!(menu.is_open());
    /// ```
    pub fn open(&mut self) {
        let position = self.last_pointer.unwrap_or_else(|| {
            Vec2::new(
                self.cached_bounds.min_x() + self.cached_bounds.width() / 2.0,
                self.cached_bounds.min_y() + self.cached_bounds.height() / 2.0,
            )
        });
        self.stack
            .open_at(OverlayAnchor::Pointer(position), true);
    }

    /// Closes the menu.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{ContextMenu, MenuItem, Text};
    ///
    /// let mut menu = ContextMenu::new(Text::new("c"), vec![MenuItem::action("A")]);
    /// menu.open();
    /// menu.close();
    /// assert!(!menu.is_open());
    /// ```
    pub fn close(&mut self) {
        self.stack.close();
    }

    /// Drains the pending activation path — the item the user picked
    /// since the last call, as a [`MenuPath`] from the menu's root.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{ContextMenu, MenuItem, Text};
    ///
    /// let mut menu = ContextMenu::new(Text::new("c"), vec![MenuItem::action("A")]);
    /// assert_eq!(menu.take_activated(), None);
    /// ```
    pub fn take_activated(&mut self) -> Option<MenuPath> {
        self.stack.take_activated()
    }
}

impl Widget for ContextMenu {
    fn debug_name(&self) -> &'static str {
        "Context Menu"
    }

    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        self.child.measure(cx, constraints)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.cached_bounds = bounds;
        // The wrapper carries keyboard focus for the menu's keyboard
        // contract — the child is an internal widget with no arena
        // node of its own.
        cx.hot.flags |= NodeFlags::FOCUSABLE;
        cx.layout_child(self.child.as_mut(), bounds);
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        // The child carries the content semantics; the wrapper
        // advertises the context-menu trigger relationship so AT can
        // open the popup directly.
        node.set_role(accesskit::Role::GenericContainer);
        node.set_has_popup(accesskit::HasPopup::Menu);
        node.set_expanded(self.is_open());
        node.add_action(accesskit::Action::ShowContextMenu);
        node.add_action(accesskit::Action::Focus);
    }

    fn a11y_fixup(
        &self,
        _emitted: &mut Vec<A11yEmittedNode>,
        overlay_nodes: &[OverlayA11yRef],
        this_node: &mut AccessKitNode,
    ) {
        // aria-controls → the popup's Menu root while open.
        let Some(popup) = self.stack.root_id() else {
            return;
        };
        if let Some(menu_id) = overlay_nodes
            .iter()
            .find(|r| r.entry == popup && r.path.is_empty())
            .map(|r| r.id)
        {
            this_node.set_controls(vec![menu_id]);
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerMoved { position } => {
                self.last_pointer = Some(*position);
            }
            WidgetEvent::PointerPressed {
                position,
                button: PointerButton::Secondary,
                ..
            } => {
                self.open_at(*position);
                return EventResponse::CaptureFocus;
            }
            WidgetEvent::KeyPressed { key, .. } => match key.as_str() {
                // The overlay consumes Escape first in arena use; this
                // is the ownerless-embedded fallback (and keyboard
                // nav for a Menu driven without overlay routing).
                "Escape" => {
                    if self.is_open() {
                        self.close();
                        return EventResponse::RequestRepaint;
                    }
                    return EventResponse::Ignored;
                }
                _ => {
                    if self.is_open() {
                        let response = self.stack.key(key);
                        if response != EventResponse::Ignored {
                            return response;
                        }
                    }
                }
            },
            WidgetEvent::SemanticAction(SemanticAction::ShowContextMenu) => {
                self.open();
                return EventResponse::RequestRepaint;
            }
            WidgetEvent::SemanticAction(SemanticAction::Focus) => {
                return EventResponse::CaptureFocus;
            }
            _ => {}
        }
        // Everything else belongs to the wrapped content.
        let mut child_cx = EventContext {
            event: cx.event,
            bounds: self.cached_bounds,
            scale: cx.scale,
        };
        self.child.event(&mut child_cx)
    }

    fn sync_overlay(&mut self, overlay: &mut OverlayLayer) {
        self.stack.sync(overlay);
    }

    fn paint(&self, _cx: &mut PaintContext) {
        // The wrapper contributes no chrome — the child paints itself.
    }

    fn child_count(&self) -> usize {
        1
    }

    fn child(&self, _index: usize) -> Option<&dyn Widget> {
        Some(self.child.as_ref())
    }

    fn child_mut(&mut self, _index: usize) -> Option<&mut dyn Widget> {
        Some(self.child.as_mut())
    }

    fn child_bounds(&self, _index: usize) -> Option<Rect> {
        Some(self.cached_bounds)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    use crate::widgets::text::Text;

    fn menu() -> ContextMenu {
        ContextMenu::new(
            Text::new("content"),
            vec![
                MenuItem::action("Cut"),
                MenuItem::checkable("Freeze", false),
                MenuItem::submenu("More", vec![MenuItem::action("Deep")]),
            ],
        )
    }

    fn laid_out(m: &mut ContextMenu) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        m.layout(&mut cx, Rect::new(0.0, 0.0, 200.0, 100.0));
    }

    fn overlay() -> OverlayLayer {
        let mut o = OverlayLayer::new();
        o.set_viewport(Rect::new(0.0, 0.0, 800.0, 600.0));
        o
    }

    fn event(m: &mut ContextMenu, ev: &WidgetEvent) -> EventResponse {
        let mut cx = EventContext {
            event: ev,
            bounds: m.cached_bounds,
            scale: 1.0,
        };
        m.event(&mut cx)
    }

    #[test]
    fn secondary_press_opens() {
        let mut m = menu();
        laid_out(&mut m);
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(50.0, 40.0),
            button: PointerButton::Secondary,
            count: 1,
        };
        assert_eq!(event(&mut m, &press), EventResponse::CaptureFocus);
        assert!(m.is_open());
    }

    #[test]
    fn primary_press_does_not_open() {
        let mut m = menu();
        laid_out(&mut m);
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(50.0, 40.0),
            button: PointerButton::Primary,
            count: 1,
        };
        // Forwarded to the child (Text ignores it) — no menu.
        assert_eq!(event(&mut m, &press), EventResponse::Ignored);
        assert!(!m.is_open());
    }

    #[test]
    fn semantic_show_context_menu() {
        let mut m = menu();
        laid_out(&mut m);
        let r = event(&mut m, &WidgetEvent::SemanticAction(SemanticAction::ShowContextMenu));
        assert_eq!(r, EventResponse::RequestRepaint);
        assert!(m.is_open());
    }

    #[test]
    fn overlay_opens_at_pointer() {
        let mut m = menu();
        laid_out(&mut m);
        let mut o = overlay();
        m.open_at(Vec2::new(100.0, 100.0));
        m.sync_overlay(&mut o);
        o.layout_pass();
        assert_eq!(o.len(), 1);
        let b = o.entry_bounds(m.popup_id().unwrap()).unwrap();
        assert!(b.min_x() >= 100.0 && b.min_y() >= 100.0);
    }

    #[test]
    fn activation_drains_and_closes() {
        let mut m = menu();
        laid_out(&mut m);
        let mut o = overlay();
        m.open_at(Vec2::new(100.0, 100.0));
        m.sync_overlay(&mut o);
        o.layout_pass();
        m.stack
            .shared()
            .lock()
            .unwrap()
            .activate_for_test(0);
        m.sync_overlay(&mut o);
        assert_eq!(m.take_activated(), Some(vec![0]));
        assert!(!m.is_open());
        assert_eq!(o.len(), 0);
    }

    #[test]
    fn outside_press_dismisses() {
        let mut m = menu();
        laid_out(&mut m);
        let mut o = overlay();
        m.open_at(Vec2::new(100.0, 100.0));
        m.sync_overlay(&mut o);
        o.layout_pass();
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(700.0, 500.0),
            button: PointerButton::Primary,
            count: 1,
        };
        assert_eq!(o.dispatch_event(&press), EventResponse::Ignored);
        m.sync_overlay(&mut o);
        assert!(!m.is_open());
        assert_eq!(m.popup_id(), None);
    }

    #[test]
    fn accessibility_role() {
        let m = menu();
        let mut node = AccessKitNode::new(accesskit::Role::Unknown);
        m.accessibility(&mut node);
        assert_eq!(node.has_popup(), Some(accesskit::HasPopup::Menu));
        assert!(node.supports_action(accesskit::Action::ShowContextMenu));
    }
}
