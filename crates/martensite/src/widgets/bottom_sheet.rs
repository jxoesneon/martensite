//! `BottomSheet` widget: an edge-anchored surface with a drag handle
//! and snap-point detents (Material bottom sheet, iOS sheet
//! presentation).
//!
//! Open with `OverlayAnchor::EdgeBottom` — the sheet measures its
//! maximum detent height, then positions its card inside the entry at
//! the current [`BottomSheet::fraction`]. Dragging the handle moves the
//! fraction live; releasing snaps to the nearest detent, and dragging
//! below the lowest detent requests dismissal.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::bottom_sheet::BottomSheet;
//!
//! let s = BottomSheet::new().detents(&[0.35, 0.9]);
//! assert_eq!(s.fraction(), 0.35);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use kurbo::Shape as _;
use martensite_core::widget::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Widget, WidgetEvent,
};
use martensite_core::{Rect, TokenKey};

/// Sheet face colour.
const SURFACE: [u8; 4] = [38, 41, 48, 255];
/// Sheet border colour.
const EDGE: [u8; 4] = [110, 115, 125, 255];
/// Drag handle colour.
const HANDLE: [u8; 4] = [130, 135, 145, 255];
/// Title ink.
const INK: [u8; 4] = [235, 236, 240, 255];
/// Top corner radius, logical points.
const CORNER: f64 = 14.0;
/// Grab-zone height at the top of the card, logical points.
const GRAB_PT: f32 = 24.0;
/// Handle bar size, logical points.
const HANDLE_W_PT: f32 = 36.0;
const HANDLE_H_PT: f32 = 4.0;
/// Title strip height, logical points.
const TITLE_PT: f32 = 28.0;
/// Depth of the entry the sheet asks for — the largest detent's
/// fraction of the offered height is applied by the host; measure
/// reports the full offered height so the card can travel between
/// detents inside one stable entry.
const MIN_DEPTH_PT: f32 = 120.0;

/// A bottom sheet with snap-point detents and a drag handle.
///
/// The sheet is hosted in an `OverlayAnchor::EdgeBottom` entry sized to
/// the full offered height; the card occupies `fraction` of that entry
/// measured from its bottom edge. Everything above the card inside the
/// entry is a dismiss target (the modal scrim beneath shows through —
/// pair with `OverlayOptions::modal().light_dismiss()`).
///
/// Poll [`BottomSheet::take_close_requested`] after dispatch; the host
/// then closes the overlay entry.
///
/// # Examples
///
/// ```
/// use martensite::widgets::bottom_sheet::BottomSheet;
///
/// let s = BottomSheet::new().title("Share").detents(&[0.5, 0.9]);
/// assert_eq!(s.detents, [0.5, 0.9]);
/// ```
pub struct BottomSheet {
    /// Optional title strip under the grab zone.
    pub title: Option<String>,
    /// Snap points as fractions of the entry height (0 < d ≤ 1),
    /// sorted ascending. The card rests at `detents[0]` initially.
    pub detents: Vec<f32>,
    /// Current card height as a fraction of the entry height.
    fraction: f32,
    /// Content child filling the card below the header.
    child: Option<Box<dyn Widget>>,
    /// Pointer currently dragging the handle.
    dragging: bool,
    /// Fraction captured when the drag began — drags are deltas from
    /// it, keeping the card under the pointer.
    drag_start_fraction: f32,
    /// Pointer y when the drag began.
    drag_start_y: f32,
    /// Set when the sheet asked to be dismissed (drag below the
    /// lowest detent or a press on the void above the card).
    close_requested: bool,
    /// Bounds assigned to the content child, in widget space.
    child_rect: Rect,
    /// The card rect within the entry, in widget space.
    card_rect: Rect,
    /// Cached bounds from the last layout pass.
    cached_bounds: Rect,
    /// Shared shaped-text painter. See [`crate::text_paint`].
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl BottomSheet {
    /// Creates a sheet with the default half-and-full detents.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::bottom_sheet::BottomSheet;
    ///
    /// let s = BottomSheet::new();
    /// assert_eq!(s.detents, [0.5, 1.0]);
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self {
            title: None,
            detents: vec![0.5, 1.0],
            fraction: 0.5,
            child: None,
            dragging: false,
            drag_start_fraction: 0.0,
            drag_start_y: 0.0,
            close_requested: false,
            child_rect: Rect::default(),
            card_rect: Rect::default(),
            cached_bounds: Rect::default(),
            text_painter: None,
        }
    }

    /// Sets the title strip text.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::bottom_sheet::BottomSheet;
    ///
    /// let s = BottomSheet::new().title("Options");
    /// ```
    #[must_use]
    pub fn title(mut self, text: impl Into<String>) -> Self {
        self.title = Some(text.into());
        self
    }

    /// Sets the detents — sorted fractions of the entry height the
    /// card snaps to. The card starts at the lowest detent.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::bottom_sheet::BottomSheet;
    ///
    /// let s = BottomSheet::new().detents(&[0.25, 0.6, 1.0]);
    /// assert_eq!(s.fraction(), 0.25);
    /// ```
    #[must_use]
    pub fn detents(mut self, detents: &[f32]) -> Self {
        let mut d: Vec<f32> = detents
            .iter()
            .copied()
            .filter(|d| *d > 0.0 && *d <= 1.0)
            .collect();
        d.sort_by(f32::total_cmp);
        if d.is_empty() {
            d.push(0.5);
        }
        self.fraction = d[0];
        self.detents = d;
        self
    }

    /// Sets the content child filling the card below the grab zone
    /// and title.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::bottom_sheet::BottomSheet;
    /// use martensite::widgets::Text;
    ///
    /// let s = BottomSheet::new().child(Text::new("content"));
    /// ```
    #[must_use]
    pub fn child(mut self, child: impl Widget + 'static) -> Self {
        self.child = Some(Box::new(child));
        self
    }

    /// The current card height as a fraction of the entry height.
    #[inline]
    #[must_use]
    pub fn fraction(&self) -> f32 {
        self.fraction
    }

    /// Sets the card fraction directly (clamped to the top detent).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::bottom_sheet::BottomSheet;
    ///
    /// let mut s = BottomSheet::new();
    /// s.set_fraction(0.75);
    /// assert_eq!(s.fraction(), 0.75);
    /// ```
    pub fn set_fraction(&mut self, fraction: f32) {
        self.fraction = fraction.clamp(0.0, self.detents.last().copied().unwrap_or(1.0));
    }

    /// Drains the close-request flag — set by dragging below the
    /// lowest detent or pressing the void above the card.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::bottom_sheet::BottomSheet;
    ///
    /// let mut s = BottomSheet::new();
    /// assert!(!s.take_close_requested());
    /// ```
    pub fn take_close_requested(&mut self) -> bool {
        std::mem::take(&mut self.close_requested)
    }

    /// Installs a shared shaped-text painter for the title.
    #[must_use]
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Card rect within the entry for the current fraction.
    fn card_for(&self, bounds: Rect) -> Rect {
        let h = bounds.size.y * self.fraction;
        Rect::new(bounds.origin.x, bounds.max_y() - h, bounds.size.x, h)
    }

    /// Header height (grab zone + optional title) in device px.
    fn header_h(&self, cx_scale: f32) -> f32 {
        (GRAB_PT + if self.title.is_some() { TITLE_PT } else { 0.0 }) * cx_scale
    }

    /// Snaps `fraction` to the nearest detent; dragging below half of
    /// the lowest detent requests dismissal instead.
    fn release_drag(&mut self) {
        self.dragging = false;
        let lowest = self.detents.first().copied().unwrap_or(0.5);
        if self.fraction < lowest * 0.6 {
            self.close_requested = true;
            self.fraction = lowest;
            return;
        }
        self.fraction = self
            .detents
            .iter()
            .copied()
            .min_by(|a, b| {
                (*a - self.fraction)
                    .abs()
                    .total_cmp(&(*b - self.fraction).abs())
            })
            .unwrap_or(self.fraction);
    }
}

impl Default for BottomSheet {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for BottomSheet {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        // Fill the offered edge strip — the card positions itself
        // inside at the current fraction.
        Vec2::new(
            constraints.max_size.x.max(0.0),
            constraints.max_size.y.max(cx.pt(MIN_DEPTH_PT)),
        )
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.cached_bounds = bounds;
        self.card_rect = self.card_for(bounds);
        self.child_rect = Rect::default();
        let header = self.header_h(cx.scale);
        if let Some(child) = &mut self.child {
            self.child_rect = Rect::new(
                self.card_rect.origin.x,
                self.card_rect.origin.y + header,
                self.card_rect.size.x,
                (self.card_rect.size.y - header).max(0.0),
            );
            cx.layout_child(child.as_mut(), self.child_rect);
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Dialog);
        if let Some(title) = &self.title {
            node.set_label(title.as_str());
        } else {
            node.set_label("Bottom sheet");
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                ..
            } => {
                if self.card_rect.contains(*position) {
                    let grab_bottom = self.card_rect.origin.y + GRAB_PT * cx.scale;
                    if position.y <= grab_bottom {
                        self.dragging = true;
                        self.drag_start_fraction = self.fraction;
                        self.drag_start_y = position.y;
                        return EventResponse::CapturePointer;
                    }
                    // Inside the card below the grab zone — let the
                    // child claim it (default forwarding runs after).
                    return EventResponse::Ignored;
                }
                // Press on the void above the card — the scrim shows
                // through here; treat it as a dismiss tap.
                self.close_requested = true;
                EventResponse::Handled
            }
            WidgetEvent::PointerMoved { position } if self.dragging => {
                let entry_h = cx.bounds.size.y.max(1.0);
                let delta = (self.drag_start_y - position.y) / entry_h;
                self.set_fraction(self.drag_start_fraction + delta);
                EventResponse::RequestRepaint
            }
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                ..
            } if self.dragging => {
                self.release_drag();
                EventResponse::ReleasePointer
            }
            WidgetEvent::KeyPressed { key, .. } if key == "Escape" => {
                self.close_requested = true;
                EventResponse::Handled
            }
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let card = self.card_rect;
        if card.size.x <= 0.0 || card.size.y <= 0.0 {
            return;
        }
        let rect = kurbo::Rect::new(
            f64::from(card.min_x()),
            f64::from(card.min_y()),
            f64::from(card.max_x()),
            f64::from(card.max_y()),
        );
        // Rounded top corners only — the bottom is flush with the
        // viewport edge. Build the shape as a path: arc top-left,
        // top-right, square bottom.
        let r = cx.ptf(CORNER).min(f64::from(card.size.y));
        let mut path = kurbo::BezPath::new();
        path.move_to((rect.x0, rect.y1));
        path.line_to((rect.x0, rect.y0 + r));
        path.quad_to((rect.x0, rect.y0), (rect.x0 + r, rect.y0));
        path.line_to((rect.x1 - r, rect.y0));
        path.quad_to((rect.x1, rect.y0), (rect.x1, rect.y0 + r));
        path.line_to((rect.x1, rect.y1));
        path.close_path();
        cx.list
            .push_path(path.clone(), cx.color(TokenKey::SurfaceColor, SURFACE));
        cx.list
            .push_stroke_path(path, cx.pt(1.0), cx.color(TokenKey::BorderColor, EDGE));

        // Drag handle — centred pill in the grab zone.
        let hw = cx.pt(HANDLE_W_PT);
        let hh = cx.pt(HANDLE_H_PT);
        let hx = card.origin.x + (card.size.x - hw) / 2.0;
        let hy = card.origin.y + (GRAB_PT * cx.scale - hh) / 2.0;
        let handle = kurbo::RoundedRect::from_rect(
            kurbo::Rect::new(
                f64::from(hx),
                f64::from(hy),
                f64::from(hx + hw),
                f64::from(hy + hh),
            ),
            f64::from(hh / 2.0),
        )
        .into_path(0.1);
        cx.list
            .push_path(handle, cx.color(TokenKey::BorderColor, HANDLE));

        if let Some(title) = &self.title {
            let size_px = cx.pt(15.0);
            let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
            let w = painter
                .and_then(|p| p.measure_text(title, size_px))
                .unwrap_or(size_px * title.chars().count() as f32 * 0.55);
            let x = card.origin.x + (card.size.x - w.min(card.size.x)) / 2.0;
            let y = card.origin.y + GRAB_PT * cx.scale + (TITLE_PT * cx.scale - size_px) / 2.0;
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                kurbo::Rect::new(
                    f64::from(x),
                    f64::from(y),
                    f64::from(card.max_x()),
                    f64::from(y + size_px),
                ),
                kurbo::Point::new(f64::from(x), f64::from(y)),
                title,
                size_px,
                cx.color(TokenKey::TextColor, INK),
            );
        }
    }

    fn child_count(&self) -> usize {
        usize::from(self.child.is_some())
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        if index == 0 {
            self.child.as_deref()
        } else {
            None
        }
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        if index == 0 {
            self.child.as_deref_mut()
        } else {
            None
        }
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        if index == 0 && self.child.is_some() {
            Some(self.child_rect)
        } else {
            None
        }
    }
}

impl std::fmt::Debug for BottomSheet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BottomSheet")
            .field("fraction", &self.fraction)
            .field("detents", &self.detents)
            .field("dragging", &self.dragging)
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

    fn ev<'a>(event: &'a WidgetEvent, bounds: Rect) -> EventContext<'a> {
        EventContext {
            event,
            bounds,
            scale: 1.0,
        }
    }

    fn laid_out() -> BottomSheet {
        let mut s = BottomSheet::new().detents(&[0.5, 1.0]);
        let mut hot = HotNode::default();
        s.layout(&mut make_cx(&mut hot), Rect::new(0.0, 0.0, 400.0, 600.0));
        s
    }

    #[test]
    fn default_detents_and_fraction() {
        let s = BottomSheet::new();
        assert_eq!(s.detents, [0.5, 1.0]);
        assert_eq!(s.fraction(), 0.5);
    }

    #[test]
    fn detents_sorted_and_clamped() {
        let s = BottomSheet::new().detents(&[0.9, 0.25, 0.0, 1.5]);
        assert_eq!(s.detents, [0.25, 0.9]);
        assert_eq!(s.fraction(), 0.25);
    }

    #[test]
    fn empty_detents_fall_back() {
        let s = BottomSheet::new().detents(&[]);
        assert_eq!(s.detents, [0.5]);
    }

    #[test]
    fn card_positions_at_bottom() {
        let s = laid_out();
        // 50% of 600 → card occupies the bottom 300px.
        assert!((s.card_rect.origin.y - 300.0).abs() < 0.01);
        assert!((s.card_rect.size.y - 300.0).abs() < 0.01);
    }

    #[test]
    fn grab_drag_updates_fraction() {
        let mut s = laid_out();
        let grab = Vec2::new(200.0, s.card_rect.origin.y + 10.0);
        let press = WidgetEvent::PointerPressed {
            position: grab,
            button: PointerButton::Primary,
            count: 1,
        };
        assert_eq!(
            s.event(&mut ev(&press, Rect::new(0.0, 0.0, 400.0, 600.0))),
            EventResponse::CapturePointer
        );
        // Drag up 150px → +0.25 fraction.
        let mv = WidgetEvent::PointerMoved {
            position: Vec2::new(200.0, grab.y - 150.0),
        };
        s.event(&mut ev(&mv, Rect::new(0.0, 0.0, 400.0, 600.0)));
        assert!((s.fraction() - 0.75).abs() < 0.01);
    }

    #[test]
    fn release_snaps_to_nearest_detent() {
        let mut s = laid_out();
        s.set_fraction(0.62);
        s.dragging = true;
        let rel = WidgetEvent::PointerReleased {
            position: Vec2::new(0.0, 0.0),
            button: PointerButton::Primary,
        };
        assert_eq!(
            s.event(&mut ev(&rel, Rect::new(0.0, 0.0, 400.0, 600.0))),
            EventResponse::ReleasePointer
        );
        assert_eq!(s.fraction(), 0.5);
    }

    #[test]
    fn drag_below_lowest_requests_close() {
        let mut s = laid_out();
        s.set_fraction(0.2);
        s.dragging = true;
        let rel = WidgetEvent::PointerReleased {
            position: Vec2::new(0.0, 0.0),
            button: PointerButton::Primary,
        };
        s.event(&mut ev(&rel, Rect::new(0.0, 0.0, 400.0, 600.0)));
        assert!(s.take_close_requested());
        assert!(!s.take_close_requested());
    }

    #[test]
    fn press_above_card_dismisses() {
        let mut s = laid_out();
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(200.0, 50.0), // above the card (y<300)
            button: PointerButton::Primary,
            count: 1,
        };
        assert_eq!(
            s.event(&mut ev(&press, Rect::new(0.0, 0.0, 400.0, 600.0))),
            EventResponse::Handled
        );
        assert!(s.take_close_requested());
    }

    #[test]
    fn escape_requests_close() {
        let mut s = laid_out();
        let key = WidgetEvent::KeyPressed {
            key: "Escape".into(),
            repeat: false,
        };
        s.event(&mut ev(&key, Rect::new(0.0, 0.0, 400.0, 600.0)));
        assert!(s.take_close_requested());
    }

    #[test]
    fn set_fraction_clamps_to_top_detent() {
        let mut s = BottomSheet::new().detents(&[0.3, 0.8]);
        s.set_fraction(0.95);
        assert_eq!(s.fraction(), 0.8);
    }
}
