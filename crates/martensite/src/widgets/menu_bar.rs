//! `MenuBar` widget: a horizontal menu strip (QMenuBar/NSMenuBar).
//!
//! The bar owns a row of menu buttons; pressing a button (or hovering
//! it while a menu is open) opens that menu's [`Menu`](crate::widgets::menu::Menu)
//! popup in the [`OverlayLayer`](martensite_core::overlay::OverlayLayer)
//! anchored to the button's bounds. While a menu is open, `Left`/`Right`
//! arrows and hover switch between menus, `Escape` or an outside press
//! closes, and an activation lands in [`MenuBar::take_activated`] as a
//! [`MenuPath`](crate::widgets::menu::MenuPath) — the same out-seam as
//! [`ContextMenu`](crate::widgets::context_menu::ContextMenu).
//!
//! Keyboard lives in the bar: the overlay offers non-`Escape` keys to
//! the topmost popup first, so in-menu navigation is handled by the
//! popup itself and only `Left`/`Right` fall through here for menu
//! switching — the `Dropdown` typeahead architecture.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::{MenuBar, MenuItem};
//!
//! let bar = MenuBar::new()
//!     .menu("File", vec![MenuItem::action("Open"), MenuItem::action("Quit")])
//!     .menu("Edit", vec![MenuItem::action("Copy")]);
//! assert_eq!(bar.menu_count(), 2);
//! assert!(!bar.is_open());
//! ```

use std::sync::{Arc, Mutex};

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::overlay::{OverlayAnchor, OverlayLayer};
use martensite_core::shape::Shape;
use martensite_core::widget::{
    A11yEmittedNode, EventContext, EventResponse, LayoutConstraints, LayoutContext, OverlayA11yRef,
    PaintContext, PointerButton, SemanticAction, Widget, WidgetEvent,
};
use martensite_core::{NodeFlags, Rect, TokenKey};

use crate::widgets::menu::{MenuItem, MenuPath, MenuStack, MenuState};

/// Bar height in logical pixels.
const BAR_H: f32 = 30.0;
/// Horizontal padding inside a menu button.
const BUTTON_PAD_X: f32 = 10.0;
/// Button label font size.
const FONT_PT: f32 = 13.0;

/// Strip background.
const BAR_BG: [u8; 4] = [245, 245, 248, 255];
/// Strip bottom hairline.
const BAR_EDGE: [u8; 4] = [205, 207, 215, 255];
/// Button label ink.
const INK: [u8; 4] = [30, 30, 36, 255];
/// Open-button background.
const OPEN_BG: [u8; 4] = [60, 110, 220, 255];
/// Open-button ink.
const OPEN_INK: [u8; 4] = [255, 255, 255, 255];
/// Hovered/focused button background.
const HOVER_BG: [u8; 4] = [225, 228, 236, 255];

/// One entry in the bar — a label plus its item tree.
struct BarMenu {
    /// The button label.
    label: String,
    /// The menu's item tree (canonical — the live copy inside the
    /// shared popup state is written back on close so checkable and
    /// radio mutations persist).
    items: Vec<MenuItem>,
}

/// A menu button inside the strip — emitted as a `Role::MenuItem`
/// with `aria-haspopup="menu"`. Painting lives in [`MenuBar`] (button
/// chrome depends on bar-level open/hover state); the button exists
/// for the accessibility tree and AT actions, which it reports into
/// `pending` for the bar to apply.
struct MenuBarButton {
    /// Index into `MenuBar::menus`.
    index: usize,
    /// The menu label (snapshot — menus are fixed at construction).
    label: String,
    /// Shared slot for AT activations (`Click`/`Expand`) — the bar
    /// drains it each `event`/`sync_overlay`.
    pending: Arc<Mutex<Option<usize>>>,
    /// Whether this button's menu is currently open — mirrored by the
    /// bar each layout so `aria-expanded` tracks.
    expanded: Arc<Mutex<Option<usize>>>,
}

impl Widget for MenuBarButton {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            (self.label.chars().count() as f32 * cx.pt(7.2) + cx.pt(2.0 * BUTTON_PAD_X))
                .min(constraints.max_size.x.max(0.0)),
            cx.pt(BAR_H).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn layout(&mut self, _cx: &mut LayoutContext, _bounds: Rect) {}

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::MenuItem);
        node.set_label(self.label.as_str());
        node.set_has_popup(accesskit::HasPopup::Menu);
        node.set_expanded(
            self.expanded.lock().expect("bar state poisoned").as_ref() == Some(&self.index),
        );
        node.add_action(accesskit::Action::Click);
        node.add_action(accesskit::Action::Expand);
        node.add_action(accesskit::Action::Collapse);
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::SemanticAction(SemanticAction::Click)
            | WidgetEvent::SemanticAction(SemanticAction::Expand) => {
                *self.pending.lock().expect("bar state poisoned") = Some(self.index);
                EventResponse::Handled
            }
            WidgetEvent::SemanticAction(SemanticAction::Collapse) => {
                *self.pending.lock().expect("bar state poisoned") = Some(self.index);
                EventResponse::Handled
            }
            _ => EventResponse::Ignored,
        }
    }
}

/// A horizontal menu strip — the desktop menu-bar pattern.
///
/// The bar is a single focusable widget; its buttons are internal
/// children emitted as `MenuItem` nodes with `aria-haspopup="menu"`
/// and `aria-expanded`/`aria-controls` wired to the popup. Menus are
/// added with [`MenuBar::menu`]; activations surface through
/// [`MenuBar::take_activated`].
///
/// # Examples
///
/// ```
/// use martensite::widgets::{MenuBar, MenuItem};
///
/// let mut bar = MenuBar::new().menu("File", vec![MenuItem::action("Open")]);
/// bar.open_menu_at(0);
/// assert!(bar.is_open());
/// assert_eq!(bar.active_menu(), Some(0));
/// ```
pub struct MenuBar {
    /// The menus in order.
    menus: Vec<BarMenu>,
    /// Buttons (internal children 0..n) — AT nodes and AT actions.
    buttons: Vec<MenuBarButton>,
    /// Currently open menu index.
    active: Option<usize>,
    /// Keyboard navigation position while no menu is open.
    focused: Option<usize>,
    /// Pointer-hovered button index.
    hovered: Option<usize>,
    /// Button rects from the last layout pass.
    button_bounds: Vec<Rect>,
    /// Bar bounds from the last layout pass.
    cached_bounds: Rect,
    /// Popup-stack controller (shared menu state + overlay entries).
    stack: MenuStack,
    /// AT activations reported by buttons, drained by the bar.
    pending: Arc<Mutex<Option<usize>>>,
    /// Open-menu index mirrored to buttons for `aria-expanded`.
    expanded: Arc<Mutex<Option<usize>>>,
    /// Shared shaped-text painter — propagates into popups.
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl Default for MenuBar {
    fn default() -> Self {
        Self::new()
    }
}

impl MenuBar {
    /// Creates an empty menu bar.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::MenuBar;
    ///
    /// let bar = MenuBar::new();
    /// assert_eq!(bar.menu_count(), 0);
    /// ```
    pub fn new() -> Self {
        let pending = Arc::new(Mutex::new(None));
        let expanded = Arc::new(Mutex::new(None));
        Self {
            menus: Vec::new(),
            buttons: Vec::new(),
            active: None,
            focused: None,
            hovered: None,
            button_bounds: Vec::new(),
            cached_bounds: Rect::default(),
            stack: MenuStack::new(Arc::new(Mutex::new(MenuState::new(Vec::new()))), None),
            pending,
            expanded,
            text_painter: None,
        }
    }

    /// Adds a menu to the bar.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{MenuBar, MenuItem};
    ///
    /// let bar = MenuBar::new().menu("File", vec![MenuItem::action("New")]);
    /// assert_eq!(bar.menu_label(0), Some("File"));
    /// ```
    #[must_use]
    pub fn menu(mut self, label: impl Into<String>, items: Vec<MenuItem>) -> Self {
        let index = self.menus.len();
        self.menus.push(BarMenu {
            label: label.into(),
            items,
        });
        self.buttons.push(MenuBarButton {
            index,
            label: self.menus[index].label.clone(),
            pending: Arc::clone(&self.pending),
            expanded: Arc::clone(&self.expanded),
        });
        self
    }

    /// Shares a [`crate::text_paint::TextPainter`] so buttons and
    /// popups emit real glyph runs.
    #[must_use]
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.stack = MenuStack::new(
            self.stack.shared(),
            Some(painter.clone()),
        );
        self.text_painter = Some(painter);
        self
    }

    /// Number of menus in the bar.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{MenuBar, MenuItem};
    ///
    /// let bar = MenuBar::new().menu("A", vec![]).menu("B", vec![]);
    /// assert_eq!(bar.menu_count(), 2);
    /// ```
    #[inline]
    pub fn menu_count(&self) -> usize {
        self.menus.len()
    }

    /// The label of menu `index`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{MenuBar, MenuItem};
    ///
    /// let bar = MenuBar::new().menu("Help", vec![]);
    /// assert_eq!(bar.menu_label(0), Some("Help"));
    /// ```
    #[inline]
    pub fn menu_label(&self, index: usize) -> Option<&str> {
        self.menus.get(index).map(|m| m.label.as_str())
    }

    /// Whether a menu popup is currently open.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::MenuBar;
    ///
    /// assert!(!MenuBar::new().is_open());
    /// ```
    #[inline]
    pub fn is_open(&self) -> bool {
        self.stack.is_open()
    }

    /// The index of the open menu, if any.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::MenuBar;
    ///
    /// assert_eq!(MenuBar::new().active_menu(), None);
    /// ```
    #[inline]
    pub fn active_menu(&self) -> Option<usize> {
        self.active
    }

    /// Opens menu `index`'s popup anchored to its button — the same
    /// path a button press takes. `index` is clamped to the bar.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{MenuBar, MenuItem};
    ///
    /// let mut bar = MenuBar::new().menu("File", vec![MenuItem::action("Open")]);
    /// bar.open_menu_at(0);
    /// assert_eq!(bar.active_menu(), Some(0));
    /// ```
    pub fn open_menu_at(&mut self, index: usize) {
        self.open_menu(index.min(self.menus.len().saturating_sub(1)), false);
    }

    /// Closes any open menu.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{MenuBar, MenuItem};
    ///
    /// let mut bar = MenuBar::new().menu("F", vec![MenuItem::action("x")]);
    /// bar.open_menu_at(0);
    /// bar.close_menus();
    /// assert!(!bar.is_open());
    /// ```
    pub fn close_menus(&mut self) {
        self.store_back();
        self.active = None;
        *self.expanded.lock().expect("bar state poisoned") = None;
        self.stack.close();
    }

    /// Drains the pending activation path — the item the user picked
    /// since the last call, as a [`MenuPath`] from the open menu's
    /// root.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{MenuBar, MenuItem};
    ///
    /// let mut bar = MenuBar::new().menu("F", vec![MenuItem::action("x")]);
    /// assert_eq!(bar.take_activated(), None);
    /// ```
    pub fn take_activated(&mut self) -> Option<MenuPath> {
        let path = self.stack.take_activated();
        if path.is_some() {
            self.store_back();
            self.active = None;
            *self.expanded.lock().expect("bar state poisoned") = None;
        }
        path
    }

    /// Writes the live (possibly toggled) item tree in shared state
    /// back into the open menu's canonical `items` — checkable and
    /// radio mutations survive the next open.
    fn store_back(&mut self) {
        if let Some(prev) = self.active {
            let items = self
                .stack
                .shared()
                .lock()
                .expect("menu state poisoned")
                .items()
                .to_vec();
            if let Some(menu) = self.menus.get_mut(prev) {
                menu.items = items;
            }
        }
    }

    /// Opens menu `index` (storing back the previously open menu's
    /// items), `via_keyboard` seeds the popup highlight.
    fn open_menu(&mut self, index: usize, via_keyboard: bool) {
        if index >= self.menus.len() {
            return;
        }
        self.store_back();
        let items = self.menus[index].items.clone();
        let anchor = self
            .button_bounds
            .get(index)
            .copied()
            .map(OverlayAnchor::Bounds)
            .unwrap_or_else(|| OverlayAnchor::Bounds(self.cached_bounds));
        self.stack.switch_items(items, anchor, via_keyboard);
        self.active = Some(index);
        self.focused = Some(index);
        *self.expanded.lock().expect("bar state poisoned") = Some(index);
    }

    /// The button index containing window-space `pos`.
    fn button_at(&self, pos: Vec2) -> Option<usize> {
        self.button_bounds.iter().position(|r| r.contains(pos))
    }

    /// Applies button activations reported by AT actions.
    fn drain_pending(&mut self) {
        let request = self.pending.lock().expect("bar state poisoned").take();
        if let Some(index) = request {
            if self.active == Some(index) {
                self.close_menus();
            } else {
                self.open_menu(index, true);
            }
        }
    }
}

impl Widget for MenuBar {
    fn debug_name(&self) -> &'static str {
        "Menu Bar"
    }

    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let w: f32 = self
            .buttons
            .iter_mut()
            .map(|b| b.measure(cx, constraints).x)
            .sum();
        Vec2::new(
            w.min(constraints.max_size.x.max(0.0)),
            cx.pt(BAR_H).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.cached_bounds = bounds;
        cx.hot.flags |= NodeFlags::FOCUSABLE;
        self.button_bounds.clear();
        let mut x = bounds.min_x();
        for (i, button) in self.buttons.iter_mut().enumerate() {
            let w = self.menus[i].label.chars().count() as f32 * cx.pt(7.2)
                + cx.pt(2.0 * BUTTON_PAD_X);
            let rect = Rect::new(x, bounds.min_y(), w, bounds.height());
            self.button_bounds.push(rect);
            cx.layout_child(button, rect);
            x += w;
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::MenuBar);
        node.set_orientation(accesskit::Orientation::Horizontal);
        node.add_action(accesskit::Action::Focus);
    }

    fn a11y_prepare(&mut self) {
        // AT activations on button nodes write into `pending`; apply
        // them so the emitted tree tracks the open menu.
        self.drain_pending();
    }

    fn a11y_fixup(
        &self,
        emitted: &mut Vec<A11yEmittedNode>,
        overlay_nodes: &[OverlayA11yRef],
        _this_node: &mut AccessKitNode,
    ) {
        let (Some(active), Some(popup)) = (self.active, self.stack.root_id()) else {
            return;
        };
        // aria-controls on the open menu's button → the popup's
        // Menu root. Buttons are emitted at path [i].
        if let Some(menu_id) = overlay_nodes
            .iter()
            .find(|r| r.entry == popup && r.path.is_empty())
            .map(|r| r.id)
        {
            if let Some(button) = emitted
                .iter_mut()
                .find(|e| e.path.as_slice() == [active as u32])
            {
                button.node.set_controls(vec![menu_id]);
            }
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        self.drain_pending();
        match cx.event {
            WidgetEvent::PointerPressed {
                position,
                button: PointerButton::Primary,
                ..
            } => {
                self.focused = None;
                if let Some(index) = self.button_at(*position) {
                    if self.active == Some(index) {
                        self.close_menus();
                    } else {
                        self.open_menu(index, false);
                    }
                    EventResponse::CaptureFocus
                } else {
                    EventResponse::Ignored
                }
            }
            WidgetEvent::PointerMoved { position } => {
                let hovered = self.button_at(*position);
                // While a menu is open, crossing to another button
                // switches menus — the QMenuBar/NSMenuBar convention.
                if self.is_open() {
                    if let Some(index) = hovered {
                        if self.active != Some(index) {
                            self.open_menu(index, false);
                            self.hovered = hovered;
                            return EventResponse::RequestRepaint;
                        }
                    }
                }
                if self.hovered != hovered {
                    self.hovered = hovered;
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Handled
            }
            WidgetEvent::KeyPressed { key, .. } => match key.as_str() {
                "ArrowRight" | "ArrowLeft" => {
                    let n = self.menus.len();
                    if n == 0 {
                        return EventResponse::Ignored;
                    }
                    let delta: i64 = if key == "ArrowRight" { 1 } else { -1 };
                    if let Some(active) = self.active {
                        // While open, arrows switch menus (the popup
                        // consumed Up/Down/Enter; Left/Right only fall
                        // through at the root level or on non-submenu
                        // rows — the menubar convention).
                        let next =
                            (active as i64 + delta).rem_euclid(n as i64) as usize;
                        self.open_menu(next, false);
                    } else {
                        let cur = self.focused.unwrap_or(0) as i64;
                        self.focused = Some((cur + delta).rem_euclid(n as i64) as usize);
                    }
                    EventResponse::RequestRepaint
                }
                "ArrowDown" | "Enter" | " " | "Space" => {
                    if !self.is_open() && !self.menus.is_empty() {
                        let index = self.focused.unwrap_or(0);
                        self.open_menu(index.min(self.menus.len() - 1), true);
                    }
                    EventResponse::RequestRepaint
                }
                "Escape" => {
                    if self.is_open() {
                        self.close_menus();
                        EventResponse::RequestRepaint
                    } else {
                        EventResponse::Ignored
                    }
                }
                _ => EventResponse::Ignored,
            },
            WidgetEvent::SemanticAction(SemanticAction::Focus) => EventResponse::CaptureFocus,
            _ => {
                // Forward to the buttons (default child-forwarding,
                // bounds-gated).
                for i in (0..self.buttons.len()).rev() {
                    let Some(child_bounds) = self.button_bounds.get(i).copied() else {
                        continue;
                    };
                    if let Some(pos) = cx.event.position() {
                        if !child_bounds.contains(pos) {
                            continue;
                        }
                    }
                    let mut child_cx = EventContext {
                        event: cx.event,
                        bounds: child_bounds,
                        scale: cx.scale,
                    };
                    match self.buttons[i].event(&mut child_cx) {
                        EventResponse::Ignored => continue,
                        response => return response,
                    }
                }
                EventResponse::Ignored
            }
        }
    }

    fn sync_overlay(&mut self, overlay: &mut OverlayLayer) {
        self.drain_pending();
        self.stack.sync(overlay);
        // The layer dismissed the open menu (outside press, Escape at
        // the root) — store back item mutations and reset.
        if self.active.is_some() && !self.stack.is_open() {
            self.store_back();
            self.active = None;
            *self.expanded.lock().expect("bar state poisoned") = None;
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        let rect = kurbo::Rect::new(
            f64::from(b.min_x()),
            f64::from(b.min_y()),
            f64::from(b.max_x()),
            f64::from(b.max_y()),
        );
        cx.list
            .push_fill_rect(rect, cx.color(TokenKey::SurfaceColor, BAR_BG));
        cx.list.push_fill_rect(
            kurbo::Rect::new(
                f64::from(b.min_x()),
                f64::from(b.max_y() - cx.pt(1.0)),
                f64::from(b.max_x()),
                f64::from(b.max_y()),
            ),
            cx.color(TokenKey::DividerColor, BAR_EDGE),
        );
        let font_px = cx.pt(FONT_PT);
        for (i, menu) in self.menus.iter().enumerate() {
            let Some(r) = self.button_bounds.get(i) else {
                continue;
            };
            let open = self.active == Some(i);
            let flagged = self.hovered == Some(i)
                || (!self.is_open() && self.focused == Some(i));
            let ink = if open {
                let pill = Shape::rounded(cx.dim(TokenKey::BorderRadiusSmall, 4.0));
                let fill = kurbo::Rect::new(
                    f64::from(r.min_x() + cx.pt(3.0)),
                    f64::from(r.min_y() + cx.pt(3.0)),
                    f64::from(r.max_x() - cx.pt(3.0)),
                    f64::from(r.max_y() - cx.pt(3.0)),
                );
                cx.list
                    .push_fill_shape(fill, &pill, cx.color(TokenKey::AccentColor, OPEN_BG));
                cx.color(TokenKey::TextInverseColor, OPEN_INK)
            } else {
                if flagged {
                    let pill = Shape::rounded(cx.dim(TokenKey::BorderRadiusSmall, 4.0));
                    let fill = kurbo::Rect::new(
                        f64::from(r.min_x() + cx.pt(3.0)),
                        f64::from(r.min_y() + cx.pt(3.0)),
                        f64::from(r.max_x() - cx.pt(3.0)),
                        f64::from(r.max_y() - cx.pt(3.0)),
                    );
                    cx.list.push_fill_shape(
                        fill,
                        &pill,
                        cx.color(TokenKey::DividerColor, HOVER_BG),
                    );
                }
                cx.color(TokenKey::TextColor, INK)
            };
            let tx = r.min_x() + cx.pt(BUTTON_PAD_X);
            crate::text_paint::paint_label_clipped(
                crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter),
                cx.list,
                kurbo::Rect::new(
                    f64::from(tx),
                    f64::from(r.min_y()),
                    f64::from(r.max_x() - cx.pt(4.0)),
                    f64::from(r.max_y()),
                ),
                kurbo::Point::new(
                    f64::from(tx),
                    f64::from(r.min_y() + (r.height() - font_px) / 2.0),
                ),
                &menu.label,
                font_px,
                ink,
            );
        }
    }

    fn child_count(&self) -> usize {
        self.buttons.len()
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        self.buttons.get(index).map(|b| b as &dyn Widget)
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        self.buttons.get_mut(index).map(|b| b as &mut dyn Widget)
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        self.button_bounds.get(index).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn bar() -> MenuBar {
        MenuBar::new()
            .menu(
                "File",
                vec![
                    MenuItem::action("New"),
                    MenuItem::action("Open"),
                    MenuItem::separator(),
                    MenuItem::action("Quit"),
                ],
            )
            .menu(
                "Edit",
                vec![MenuItem::action("Copy"), MenuItem::action("Paste")],
            )
            .menu("Help", vec![MenuItem::action("About")])
    }

    fn laid_out(bar: &mut MenuBar) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        bar.layout(&mut cx, Rect::new(0.0, 0.0, 400.0, 30.0));
    }

    fn overlay() -> OverlayLayer {
        let mut o = OverlayLayer::new();
        o.set_viewport(Rect::new(0.0, 0.0, 800.0, 600.0));
        o
    }

    fn key(k: &str) -> WidgetEvent {
        WidgetEvent::KeyPressed {
            key: k.to_string(),
            repeat: false,
        }
    }

    fn event(bar: &mut MenuBar, ev: &WidgetEvent) -> EventResponse {
        let mut cx = EventContext {
            event: ev,
            bounds: bar.cached_bounds,
            scale: 1.0,
        };
        bar.event(&mut cx)
    }

    fn press_at(bar: &mut MenuBar, x: f32) -> EventResponse {
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(x, 15.0),
            button: PointerButton::Primary,
            count: 1,
        };
        event(bar, &press)
    }

    #[test]
    fn press_opens_and_toggles() {
        let mut bar = bar();
        laid_out(&mut bar);
        // Button 0 spans the left edge.
        let r = bar.button_bounds[0];
        assert_eq!(press_at(&mut bar, r.min_x() + 4.0), EventResponse::CaptureFocus);
        assert_eq!(bar.active_menu(), Some(0));
        assert!(bar.is_open());
        // Pressing the open button closes.
        assert_eq!(press_at(&mut bar, r.min_x() + 4.0), EventResponse::CaptureFocus);
        assert!(!bar.is_open());
        assert_eq!(bar.active_menu(), None);
    }

    #[test]
    fn arrows_move_between_menus() {
        let mut bar = bar();
        laid_out(&mut bar);
        // Closed: arrows move the button focus.
        assert_eq!(event(&mut bar, &key("ArrowRight")), EventResponse::RequestRepaint);
        assert_eq!(bar.focused, Some(1));
        event(&mut bar, &key("ArrowLeft"));
        event(&mut bar, &key("ArrowLeft"));
        assert_eq!(bar.focused, Some(2)); // wraps
        // Open a menu — arrows switch between menus.
        bar.open_menu(0, true);
        event(&mut bar, &key("ArrowRight"));
        assert_eq!(bar.active_menu(), Some(1));
        event(&mut bar, &key("ArrowRight"));
        assert_eq!(bar.active_menu(), Some(2));
        event(&mut bar, &key("ArrowLeft"));
        assert_eq!(bar.active_menu(), Some(1));
    }

    #[test]
    fn hover_switches_open_menu() {
        let mut bar = bar();
        laid_out(&mut bar);
        bar.open_menu(0, false);
        let r2 = bar.button_bounds[2];
        let mv = WidgetEvent::PointerMoved {
            position: Vec2::new(r2.min_x() + 4.0, 15.0),
        };
        assert_eq!(event(&mut bar, &mv), EventResponse::RequestRepaint);
        assert_eq!(bar.active_menu(), Some(2));
    }

    #[test]
    fn escape_closes() {
        let mut bar = bar();
        laid_out(&mut bar);
        bar.open_menu(0, true);
        assert_eq!(event(&mut bar, &key("Escape")), EventResponse::RequestRepaint);
        assert!(!bar.is_open());
    }

    #[test]
    fn overlay_lifecycle() {
        let mut bar = bar();
        laid_out(&mut bar);
        let mut o = overlay();
        bar.open_menu(0, false);
        bar.sync_overlay(&mut o);
        o.layout_pass();
        assert_eq!(o.len(), 1);
        let id = bar.stack.root_id().unwrap();
        let b = o.entry_bounds(id).unwrap();
        // Popup hangs below the button.
        assert!(b.min_y() >= bar.button_bounds[0].max_y());
        // Activate through the shared state (as a row would).
        bar.stack
            .shared()
            .lock()
            .unwrap()
            .activate_for_test(1);
        bar.sync_overlay(&mut o);
        assert_eq!(bar.take_activated(), Some(vec![1]));
        assert!(!bar.is_open());
        assert_eq!(bar.active_menu(), None);
    }

    #[test]
    fn checkable_state_persists_across_open() {
        let mut bar = MenuBar::new().menu(
            "View",
            vec![MenuItem::checkable("Sidebar", false), MenuItem::action("Zoom")],
        );
        laid_out(&mut bar);
        let mut o = overlay();
        bar.open_menu(0, false);
        bar.sync_overlay(&mut o);
        o.layout_pass();
        // Toggle the checkable through the live state.
        bar.stack
            .shared()
            .lock()
            .unwrap()
            .activate_for_test(0);
        bar.sync_overlay(&mut o);
        assert_eq!(bar.take_activated(), Some(vec![0]));
        // Reopen — the toggle persisted back into the canonical items.
        bar.open_menu(0, false);
        bar.sync_overlay(&mut o);
        let state = bar.stack.shared();
        let state = state.lock().unwrap();
        assert!(matches!(
            state.items()[0],
            MenuItem::Checkable { checked: true, .. }
        ));
    }

    #[test]
    fn accessibility_role() {
        let bar = bar();
        let mut node = AccessKitNode::new(accesskit::Role::Unknown);
        bar.accessibility(&mut node);
        assert_eq!(node.role(), accesskit::Role::MenuBar);
        let mut node2 = AccessKitNode::new(accesskit::Role::Unknown);
        bar.buttons[0].accessibility(&mut node2);
        assert_eq!(node2.role(), accesskit::Role::MenuItem);
        assert_eq!(node2.has_popup(), Some(accesskit::HasPopup::Menu));
        assert_eq!(node2.label(), Some("File"));
    }
}
