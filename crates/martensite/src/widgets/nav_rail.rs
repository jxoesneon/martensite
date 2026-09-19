//! `NavRail` widget: a vertical icon+label destination rail for
//! app-level navigation (Material 3 `NavigationRail`, WinUI
//! `NavigationView` in rail mode, `NSSplitViewController` sidebar
//! analogue).
//!
//! Destinations stack vertically; the selected one gets an accent
//! pill. Arrow keys move focus, `Enter`/`Space` activates — poll
//! [`NavRail::take_activated`] for the destination index. Selection
//! and activation are separate: the rail only *reports* activations,
//! the app decides whether to change `selected`.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::nav_rail::{NavDestination, NavRail};
//!
//! let r = NavRail::new()
//!     .destination("🏠", "Home")
//!     .destination("⚙", "Settings");
//! assert_eq!(r.destination_count(), 2);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::widget::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Widget, WidgetEvent,
};
use martensite_core::{Rect, TokenKey};

/// Rail width, logical points.
const WIDTH_PT: f32 = 72.0;
/// Destination cell height, logical points.
const CELL_PT: f32 = 56.0;
/// Icon glyph size, logical points.
const ICON_PT: f32 = 20.0;
/// Label font size, logical points.
const LABEL_PT: f32 = 11.0;
/// Top inset before the first destination, logical points.
const TOP_INSET_PT: f32 = 8.0;
/// Selected pill horizontal inset, logical points.
const PILL_INSET_PT: f32 = 8.0;

/// Rail face.
const FACE: [u8; 4] = [245, 246, 248, 255];
/// Label ink.
const INK: [u8; 4] = [30, 31, 36, 255];
/// Unselected ink.
const INK_DIM: [u8; 4] = [110, 114, 123, 255];
/// Hover tint.
const HOVER: [u8; 4] = [30, 31, 36, 12];

/// One rail destination.
///
/// # Examples
///
/// ```
/// use martensite::widgets::nav_rail::NavDestination;
///
/// let d = NavDestination::new("★", "Starred");
/// assert_eq!(d.label, "Starred");
/// ```
#[derive(Clone, Debug)]
pub struct NavDestination {
    /// Icon glyph (a short string — emoji or a single character).
    pub icon: String,
    /// The destination label.
    pub label: String,
}

impl NavDestination {
    /// Creates a destination.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::nav_rail::NavDestination;
    ///
    /// let d = NavDestination::new("📁", "Files");
    /// ```
    #[must_use]
    pub fn new(icon: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            icon: icon.into(),
            label: label.into(),
        }
    }
}

/// A vertical navigation rail.
///
/// # Examples
///
/// ```
/// use martensite::widgets::nav_rail::NavRail;
///
/// let r = NavRail::new().destination("🏠", "Home").selected(0);
/// assert_eq!(r.selected_index(), Some(0));
/// ```
pub struct NavRail {
    /// Destinations top-to-bottom.
    destinations: Vec<NavDestination>,
    /// Selected destination index.
    selected: Option<usize>,
    /// Hovered destination index.
    highlighted: Option<usize>,
    /// Pending activation — drained by `take_activated`.
    activated: Option<usize>,
    /// Per-destination hit rects.
    cell_rects: Vec<Rect>,
    /// Enabled flag.
    enabled: bool,
    /// Shared shaped-text painter.
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl NavRail {
    /// Creates an empty rail.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::nav_rail::NavRail;
    ///
    /// let r = NavRail::new();
    /// assert_eq!(r.destination_count(), 0);
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self {
            destinations: Vec::new(),
            selected: None,
            highlighted: None,
            activated: None,
            cell_rects: Vec::new(),
            enabled: true,
            text_painter: None,
        }
    }

    /// Appends a destination (icon glyph + label).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::nav_rail::NavRail;
    ///
    /// let r = NavRail::new().destination("🏠", "Home");
    /// ```
    #[must_use]
    pub fn destination(mut self, icon: impl Into<String>, label: impl Into<String>) -> Self {
        self.destinations.push(NavDestination::new(icon, label));
        self
    }

    /// Sets all destinations at once.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::nav_rail::{NavDestination, NavRail};
    ///
    /// let r = NavRail::new().destinations(vec![NavDestination::new("★", "Favs")]);
    /// ```
    #[must_use]
    pub fn destinations(mut self, destinations: Vec<NavDestination>) -> Self {
        self.destinations = destinations;
        self.selected = self.selected.filter(|i| *i < self.destinations.len());
        self
    }

    /// Sets the selected destination.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::nav_rail::NavRail;
    ///
    /// let r = NavRail::new().destination("🏠", "Home").selected(0);
    /// ```
    #[must_use]
    pub fn selected(mut self, index: usize) -> Self {
        self.selected = Some(index.min(self.destinations.len().saturating_sub(1)));
        self
    }

    /// Enables or disables the rail.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::nav_rail::NavRail;
    ///
    /// let r = NavRail::new().enabled(false);
    /// ```
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// The number of destinations.
    #[inline]
    #[must_use]
    pub fn destination_count(&self) -> usize {
        self.destinations.len()
    }

    /// The selected destination index.
    #[inline]
    #[must_use]
    pub fn selected_index(&self) -> Option<usize> {
        self.selected
    }

    /// Sets the selection programmatically.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::nav_rail::NavRail;
    ///
    /// let mut r = NavRail::new().destination("🏠", "Home");
    /// r.set_selected(Some(0));
    /// assert_eq!(r.selected_index(), Some(0));
    /// ```
    pub fn set_selected(&mut self, index: Option<usize>) {
        self.selected = index.filter(|i| *i < self.destinations.len());
    }

    /// Drains a destination activation — the index the user chose.
    /// The app applies it via [`NavRail::set_selected`] if desired.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::nav_rail::NavRail;
    ///
    /// let mut r = NavRail::new();
    /// assert_eq!(r.take_activated(), None);
    /// ```
    pub fn take_activated(&mut self) -> Option<usize> {
        self.activated.take()
    }

    /// Installs a shared shaped-text painter.
    #[must_use]
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }
}

impl Default for NavRail {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for NavRail {
    fn measure(&mut self, cx: &mut LayoutContext, _constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            cx.pt(WIDTH_PT),
            cx.pt(TOP_INSET_PT + CELL_PT * self.destinations.len().max(1) as f32),
        )
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.cell_rects.clear();
        let cell = cx.pt(CELL_PT);
        let top = cx.pt(TOP_INSET_PT);
        for i in 0..self.destinations.len() {
            self.cell_rects.push(Rect::new(
                bounds.origin.x,
                bounds.origin.y + top + i as f32 * cell,
                bounds.size.x.min(cx.pt(WIDTH_PT)),
                cell,
            ));
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Navigation);
        node.set_label("Navigation");
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
                let hit = self.cell_rects.iter().position(|r| r.contains(*position));
                if hit != self.highlighted {
                    self.highlighted = hit;
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Handled
            }
            WidgetEvent::PointerLeave => {
                self.highlighted = None;
                EventResponse::RequestRepaint
            }
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                ..
            } => {
                if let Some(i) = self.cell_rects.iter().position(|r| r.contains(*position)) {
                    self.activated = Some(i);
                    self.highlighted = Some(i);
                    return EventResponse::Handled;
                }
                EventResponse::Ignored
            }
            WidgetEvent::KeyPressed { key, .. } => match key.as_str() {
                "ArrowDown" | "ArrowUp" => {
                    if self.destinations.is_empty() {
                        return EventResponse::Ignored;
                    }
                    let cur = self.highlighted.or(self.selected);
                    let next = match (key.as_str(), cur) {
                        ("ArrowDown", Some(i)) => (i + 1).min(self.destinations.len() - 1),
                        ("ArrowDown", None) => 0,
                        (_, Some(i)) => i.saturating_sub(1),
                        (_, None) => self.destinations.len() - 1,
                    };
                    self.highlighted = Some(next);
                    EventResponse::RequestRepaint
                }
                "Enter" | "Space" | " " => {
                    if let Some(i) = self.highlighted.or(self.selected) {
                        self.activated = Some(i);
                        return EventResponse::Handled;
                    }
                    EventResponse::Ignored
                }
                _ => EventResponse::Ignored,
            },
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        cx.list.push_fill_rect(
            kurbo::Rect::new(
                f64::from(cx.bounds.min_x()),
                f64::from(cx.bounds.min_y()),
                f64::from(cx.bounds.max_x()),
                f64::from(cx.bounds.max_y()),
            ),
            cx.color(TokenKey::SurfaceColor, FACE),
        );
        let icon_size = cx.pt(ICON_PT);
        let label_size = cx.pt(LABEL_PT);
        for (i, d) in self.destinations.iter().enumerate() {
            let r = self.cell_rects[i];
            let selected = self.selected == Some(i);
            let hovered = self.highlighted == Some(i) && !selected;
            if selected || hovered {
                let inset = cx.pt(PILL_INSET_PT);
                let vr = if selected { cx.pt(4.0) } else { cx.pt(6.0) };
                cx.list.push_fill_shape(
                    kurbo::Rect::new(
                        f64::from(r.min_x() + inset),
                        f64::from(r.min_y() + vr),
                        f64::from(r.max_x() - inset),
                        f64::from(r.max_y() - vr),
                    ),
                    &martensite_core::shape::Shape::rounded(cx.pt(10.0)),
                    if selected {
                        cx.color(TokenKey::AccentColor, [70, 110, 200, 255])
                    } else {
                        HOVER
                    },
                );
            }
            let ink = if selected {
                [255, 255, 255, 255]
            } else if self.enabled {
                cx.color(TokenKey::TextColor, INK)
            } else {
                INK_DIM
            };
            // Icon glyph centred horizontally.
            let iw = painter
                .and_then(|p| p.measure_text(&d.icon, icon_size))
                .unwrap_or(icon_size);
            let ix = r.origin.x + (r.size.x - iw) / 2.0;
            let iy = r.origin.y + cx.pt(6.0);
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                kurbo::Rect::new(
                    f64::from(r.min_x()),
                    f64::from(r.min_y()),
                    f64::from(r.max_x()),
                    f64::from(r.max_y()),
                ),
                kurbo::Point::new(f64::from(ix), f64::from(iy)),
                &d.icon,
                icon_size,
                ink,
            );
            // Label under the icon.
            let lw = painter
                .and_then(|p| p.measure_text(&d.label, label_size))
                .unwrap_or(label_size * d.label.chars().count() as f32 * 0.55);
            let lx = r.origin.x + (r.size.x - lw) / 2.0;
            let ly = iy + icon_size + cx.pt(4.0);
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                kurbo::Rect::new(
                    f64::from(r.min_x()),
                    f64::from(ly),
                    f64::from(r.max_x()),
                    f64::from(r.max_y()),
                ),
                kurbo::Point::new(f64::from(lx), f64::from(ly)),
                &d.label,
                label_size,
                if selected {
                    ink
                } else {
                    cx.color(TokenKey::TextMutedColor, INK_DIM)
                },
            );
        }
    }
}

impl std::fmt::Debug for NavRail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NavRail")
            .field("destinations", &self.destinations.len())
            .field("selected", &self.selected)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn make_cx(hot: &mut HotNode) -> LayoutContext<'_> {
        LayoutContext { hot, scale: 1.0 }
    }

    fn ev<'a>(event: &'a WidgetEvent) -> EventContext<'a> {
        EventContext {
            event,
            bounds: Rect::new(0.0, 0.0, 72.0, 400.0),
            scale: 1.0,
        }
    }

    fn rail() -> NavRail {
        let mut r = NavRail::new()
            .destination("🏠", "Home")
            .destination("🔍", "Search")
            .destination("⚙", "Settings");
        let mut hot = HotNode::default();
        r.layout(&mut make_cx(&mut hot), Rect::new(0.0, 0.0, 72.0, 400.0));
        r
    }

    #[test]
    fn builder_counts() {
        assert_eq!(rail().destination_count(), 3);
    }

    #[test]
    fn click_activates() {
        let mut r = rail();
        let cell = r.cell_rects[1];
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(cell.origin.x + 8.0, cell.origin.y + 8.0),
            button: PointerButton::Primary,
            count: 1,
        };
        assert_eq!(r.event(&mut ev(&press)), EventResponse::Handled);
        assert_eq!(r.take_activated(), Some(1));
        // Activation ≠ selection — the app applies it.
        assert_eq!(r.selected_index(), None);
    }

    #[test]
    fn arrows_move_highlight() {
        let mut r = rail().selected(0);
        let down = WidgetEvent::KeyPressed {
            key: "ArrowDown".into(),
            repeat: false,
        };
        r.event(&mut ev(&down));
        assert_eq!(r.highlighted, Some(1));
        let enter = WidgetEvent::KeyPressed {
            key: "Enter".into(),
            repeat: false,
        };
        r.event(&mut ev(&enter));
        assert_eq!(r.take_activated(), Some(1));
    }

    #[test]
    fn selection_clamps() {
        let mut r = NavRail::new().destination("a", "A").selected(9);
        assert_eq!(r.selected_index(), Some(0));
        r.set_selected(Some(5));
        assert_eq!(r.selected_index(), None); // out of range → None
    }

    #[test]
    fn disabled_inert() {
        let mut r = rail();
        r.enabled = false;
        let cell = r.cell_rects[0];
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(cell.origin.x + 4.0, cell.origin.y + 4.0),
            button: PointerButton::Primary,
            count: 1,
        };
        assert_eq!(r.event(&mut ev(&press)), EventResponse::Ignored);
    }
}
