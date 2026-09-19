//! `Breadcrumb` widget: a horizontal path strip — labelled segments
//! separated by chevrons, with an ellipsis collapse when the path
//! overflows (WinUI `BreadcrumbBar`, `NSPathControl`, Ant `Breadcrumb`).
//!
//! The last segment is the current location (non-interactive, full
//! ink); earlier segments are links. Poll
//! [`Breadcrumb::take_navigated`] for link activations and
//! [`Breadcrumb::take_ellipsis_activated`] when the collapsed "…"
//! was pressed — the host then shows the hidden segments (e.g. in a
//! [`crate::widgets::Menu`]).
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::breadcrumb::Breadcrumb;
//!
//! let b = Breadcrumb::new().segments(["Home", "Docs", "API"]);
//! assert_eq!(b.segment_count(), 3);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::widget::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Widget, WidgetEvent,
};
use martensite_core::{Rect, TokenKey};

/// Link segment ink.
const LINK_INK: [u8; 4] = [70, 110, 200, 255];
/// Current (last) segment ink.
const CURRENT_INK: [u8; 4] = [30, 31, 36, 255];
/// Separator "›" and ellipsis ink.
const SEP_INK: [u8; 4] = [140, 144, 153, 255];
/// Hover/keyboard-focus segment highlight.
const HIGHLIGHT: [u8; 4] = [70, 110, 200, 22];
/// Font size, logical points.
const FONT_PT: f32 = 13.0;
/// Padding around a segment's highlight pill, logical points.
const PAD_PT: f32 = 4.0;
/// Gap around each separator, logical points.
const SEP_PAD_PT: f32 = 6.0;
/// Strip height, logical points.
const HEIGHT_PT: f32 = 28.0;
/// Width reserved for the "…" collapse button, logical points.
const ELLIPSIS_W_PT: f32 = 20.0;

/// Layout plan computed during paint — the only pass with a text
/// painter — and read back by event hit-testing. Interior-mutable
/// because [`Widget::paint`] takes `&self`.
#[derive(Default)]
struct Plan {
    /// Visible segment indices (leaders collapse into the ellipsis).
    visible: Vec<usize>,
    /// Indices collapsed behind the ellipsis.
    hidden: Vec<usize>,
    /// Hit rects for navigable (non-current) visible segments.
    rects: Vec<Rect>,
    /// Ellipsis-button hit rect when collapsed.
    ellipsis_rect: Option<Rect>,
    /// Per-segment text widths incl. padding, device px.
    seg_widths: Vec<f32>,
    /// Width the plan was computed for — replan on change.
    width: f32,
}

/// A breadcrumb path strip.
///
/// Segments are laid out left to right; when they don't fit, leading
/// segments collapse behind an "…" button at the left edge. The
/// collapsed set is exposed via [`Breadcrumb::hidden_segments`] so the
/// host can present them.
///
/// # Examples
///
/// ```
/// use martensite::widgets::breadcrumb::Breadcrumb;
///
/// let b = Breadcrumb::new().segments(["a", "b"]);
/// assert_eq!(b.current(), Some("b"));
/// ```
pub struct Breadcrumb {
    /// Path segments, root first.
    segments: Vec<String>,
    /// Segment index under pointer hover / keyboard focus —
    /// `usize::MAX` marks the ellipsis button.
    highlighted: Option<usize>,
    /// Pending navigation target — drained by `take_navigated`.
    navigated: Option<usize>,
    /// Set when the ellipsis button was activated.
    ellipsis_activated: bool,
    /// Paint-computed layout plan (visible/hidden sets, hit rects).
    plan: parking_lot::Mutex<Plan>,
    /// Shared shaped-text painter. See [`crate::text_paint`].
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl Breadcrumb {
    /// Creates an empty breadcrumb.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::breadcrumb::Breadcrumb;
    ///
    /// let b = Breadcrumb::new();
    /// assert_eq!(b.segment_count(), 0);
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self {
            segments: Vec::new(),
            highlighted: None,
            navigated: None,
            ellipsis_activated: false,
            plan: parking_lot::Mutex::new(Plan::default()),
            text_painter: None,
        }
    }

    /// Sets the path segments (root first, current last).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::breadcrumb::Breadcrumb;
    ///
    /// let b = Breadcrumb::new().segments(["Root", "Sub"]);
    /// ```
    #[must_use]
    pub fn segments(mut self, segments: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.segments = segments.into_iter().map(Into::into).collect();
        self
    }

    /// Appends one segment.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::breadcrumb::Breadcrumb;
    ///
    /// let b = Breadcrumb::new().push("a").push("b");
    /// assert_eq!(b.current(), Some("b"));
    /// ```
    #[must_use]
    pub fn push(mut self, segment: impl Into<String>) -> Self {
        self.segments.push(segment.into());
        self
    }

    /// Number of segments.
    #[inline]
    #[must_use]
    pub fn segment_count(&self) -> usize {
        self.segments.len()
    }

    /// The current (last) segment label.
    #[must_use]
    pub fn current(&self) -> Option<&str> {
        self.segments.last().map(String::as_str)
    }

    /// Indices currently collapsed behind the ellipsis.
    #[inline]
    #[must_use]
    pub fn hidden_segments(&self) -> Vec<usize> {
        self.plan.lock().hidden.clone()
    }

    /// Drains a link activation — the index of the segment the user
    /// navigated to (always a non-last, visible segment).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::breadcrumb::Breadcrumb;
    ///
    /// let mut b = Breadcrumb::new().segments(["a", "b"]);
    /// assert_eq!(b.take_navigated(), None);
    /// ```
    pub fn take_navigated(&mut self) -> Option<usize> {
        self.navigated.take()
    }

    /// Drains the ellipsis-button activation flag — the host should
    /// present [`Breadcrumb::hidden_segments`] (e.g. in a menu).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::breadcrumb::Breadcrumb;
    ///
    /// let mut b = Breadcrumb::new();
    /// assert!(!b.take_ellipsis_activated());
    /// ```
    pub fn take_ellipsis_activated(&mut self) -> bool {
        std::mem::take(&mut self.ellipsis_activated)
    }

    /// Installs a shared shaped-text painter.
    #[must_use]
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Fallback per-segment width when no painter resolves —
    /// proportional estimate, matching other widgets' convention.
    fn est_width(&self, text: &str, size_px: f32) -> f32 {
        size_px * text.chars().count() as f32 * 0.55
    }

    /// Recomputes which segments are visible given `width` device px.
    /// Greedy: keep the trailing segments, collapse leaders into the
    /// ellipsis until the visible tail fits.
    fn plan(&self, cx: &PaintContext, width: f32) {
        let size_px = cx.pt(FONT_PT);
        let sep_w = cx.pt(SEP_PAD_PT) * 2.0 + size_px * 0.6;
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let seg_widths: Vec<f32> = self
            .segments
            .iter()
            .map(|s| {
                painter
                    .and_then(|p| p.measure_text(s, size_px))
                    .unwrap_or_else(|| self.est_width(s, size_px))
                    + cx.pt(PAD_PT) * 2.0
            })
            .collect();
        let n = self.segments.len();
        let total: f32 = seg_widths.iter().sum::<f32>() + sep_w * n.saturating_sub(1) as f32;
        let mut plan = self.plan.lock();
        plan.width = width;
        plan.seg_widths = seg_widths;
        plan.visible = (0..n).collect();
        plan.hidden.clear();
        if total <= width || n <= 2 {
            return;
        }
        // Collapse leaders until the remainder + ellipsis fits. Each
        // cut removes a segment and one separator; the ellipsis then
        // needs its own width plus the separator before the first
        // visible segment.
        let ell_w = cx.pt(ELLIPSIS_W_PT);
        let mut used = total;
        let mut cut = 0;
        while cut < n.saturating_sub(2) {
            plan.hidden.push(cut);
            used -= plan.seg_widths[cut] + sep_w;
            cut += 1;
            if used + ell_w + sep_w <= width {
                break;
            }
        }
        plan.visible = (cut..n).collect();
    }
}

impl Default for Breadcrumb {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Breadcrumb {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            constraints.max_size.x.max(cx.pt(80.0)),
            cx.pt(HEIGHT_PT).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn layout(&mut self, _cx: &mut LayoutContext, _bounds: Rect) {
        // The layout plan needs a text painter — it runs in `paint`,
        // the only pass with one, and hit-testing reads it back.
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Navigation);
        node.set_label("Breadcrumb");
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerMoved { position } => {
                let hit = self.hit(*position);
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
            } => match self.hit(*position) {
                Some(usize::MAX) => {
                    self.ellipsis_activated = true;
                    EventResponse::Handled
                }
                Some(i) => {
                    self.navigated = Some(i);
                    EventResponse::Handled
                }
                None => EventResponse::Ignored,
            },
            WidgetEvent::KeyPressed { key, .. } => match key.as_str() {
                "ArrowLeft" | "ArrowRight" => {
                    let targets = self.keyboard_targets();
                    if targets.is_empty() {
                        return EventResponse::Ignored;
                    }
                    let pos = targets
                        .iter()
                        .position(|t| Some(*t) == self.highlighted)
                        .map(|p| p as isize)
                        .unwrap_or(-1);
                    let next = match key.as_str() {
                        "ArrowRight" => (pos + 1).min(targets.len() as isize - 1),
                        _ => (pos - 1).max(0),
                    };
                    self.highlighted = Some(targets[next as usize]);
                    EventResponse::RequestRepaint
                }
                "Enter" | "Space" | " " => match self.highlighted {
                    Some(usize::MAX) => {
                        self.ellipsis_activated = true;
                        EventResponse::Handled
                    }
                    Some(i) => {
                        self.navigated = Some(i);
                        EventResponse::Handled
                    }
                    None => EventResponse::Ignored,
                },
                _ => EventResponse::Ignored,
            },
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        if b.size.x <= 0.0 {
            return;
        }
        self.plan(cx, b.size.x);
        // Rebuild hit rects in paint — the only pass with a painter.
        // Deref the guard once so field borrows split cleanly.
        let plan = &mut *self.plan.lock();
        plan.rects.clear();
        plan.ellipsis_rect = None;
        let size_px = cx.pt(FONT_PT);
        let pad = cx.pt(PAD_PT);
        let sep_w = cx.pt(SEP_PAD_PT) * 2.0 + size_px * 0.6;
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let y = b.origin.y + (b.size.y - size_px) / 2.0;
        let pill_h = size_px + pad * 2.0;
        let pill_y = b.origin.y + (b.size.y - pill_h) / 2.0;
        let mut x = b.origin.x;

        if !plan.hidden.is_empty() {
            let ew = cx.pt(ELLIPSIS_W_PT);
            let r = Rect::new(x, pill_y, ew, pill_h);
            plan.ellipsis_rect = Some(r);
            if self.highlighted == Some(usize::MAX) {
                cx.list.push_fill_rect(
                    kurbo::Rect::new(
                        f64::from(r.min_x()),
                        f64::from(r.min_y()),
                        f64::from(r.max_x()),
                        f64::from(r.max_y()),
                    ),
                    HIGHLIGHT,
                );
            }
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                kurbo::Rect::new(
                    f64::from(x),
                    f64::from(y),
                    f64::from(x + ew),
                    f64::from(y + size_px),
                ),
                kurbo::Point::new(f64::from(x + pad), f64::from(y)),
                "…",
                size_px,
                cx.color(TokenKey::TextMutedColor, SEP_INK),
            );
            x += ew + sep_w;
        }

        for (vi, &seg) in plan.visible.iter().enumerate() {
            let w = plan.seg_widths.get(seg).copied().unwrap_or(0.0);
            let r = Rect::new(x, pill_y, w, pill_h);
            // All but the last visible segment are navigable links.
            let is_current = seg == self.segments.len() - 1;
            if !is_current {
                plan.rects.push(r);
                if self.highlighted == Some(seg) {
                    cx.list.push_fill_rect(
                        kurbo::Rect::new(
                            f64::from(r.min_x()),
                            f64::from(r.min_y()),
                            f64::from(r.max_x()),
                            f64::from(r.max_y()),
                        ),
                        HIGHLIGHT,
                    );
                }
            }
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                kurbo::Rect::new(
                    f64::from(x + pad),
                    f64::from(y),
                    f64::from(x + w - pad),
                    f64::from(y + size_px),
                ),
                kurbo::Point::new(f64::from(x + pad), f64::from(y)),
                &self.segments[seg],
                size_px,
                if is_current {
                    cx.color(TokenKey::TextColor, CURRENT_INK)
                } else {
                    cx.color(TokenKey::AccentColor, LINK_INK)
                },
            );
            x += w;
            if vi + 1 < plan.visible.len() {
                crate::text_paint::paint_label_clipped(
                    painter,
                    cx.list,
                    kurbo::Rect::new(
                        f64::from(x),
                        f64::from(y),
                        f64::from(x + sep_w),
                        f64::from(y + size_px),
                    ),
                    kurbo::Point::new(f64::from(x + sep_w / 2.0 - size_px * 0.3), f64::from(y)),
                    "›",
                    size_px,
                    cx.color(TokenKey::TextMutedColor, SEP_INK),
                );
                x += sep_w;
            }
        }
    }
}

impl Breadcrumb {
    /// Hit-test — returns a segment index, `usize::MAX` for the
    /// ellipsis, or `None`. Only non-current segments are targets.
    fn hit(&self, position: Vec2) -> Option<usize> {
        let plan = self.plan.lock();
        if plan.ellipsis_rect.is_some_and(|r| r.contains(position)) {
            return Some(usize::MAX);
        }
        // rects[i] pairs with the i-th navigable visible segment.
        let navigable: Vec<usize> = plan
            .visible
            .iter()
            .copied()
            .filter(|s| *s != self.segments.len() - 1)
            .collect();
        for (i, r) in plan.rects.iter().enumerate() {
            if r.contains(position) {
                return navigable.get(i).copied();
            }
        }
        None
    }

    /// Keyboard traversal order: ellipsis first, then navigable
    /// segments left to right.
    fn keyboard_targets(&self) -> Vec<usize> {
        let plan = self.plan.lock();
        let mut t = Vec::new();
        if !plan.hidden.is_empty() {
            t.push(usize::MAX);
        }
        t.extend(
            plan.visible
                .iter()
                .copied()
                .filter(|s| *s != self.segments.len() - 1),
        );
        t
    }
}

impl std::fmt::Debug for Breadcrumb {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Breadcrumb")
            .field("segments", &self.segments)
            .field("hidden", &self.plan.lock().hidden)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::{PaintList, Theme};

    fn ev<'a>(event: &'a WidgetEvent) -> EventContext<'a> {
        EventContext {
            event,
            bounds: Rect::new(0.0, 0.0, 600.0, 28.0),
            scale: 1.0,
        }
    }

    fn paint(b: &mut Breadcrumb, w: f32) {
        let mut list = PaintList::new();
        let theme = Theme::new("test");
        let mut cx = PaintContext {
            list: &mut list,
            bounds: Rect::new(0.0, 0.0, w, 28.0),
            theme: &theme,
            scale: 1.0,
            text_painter: None,
        };
        b.paint(&mut cx);
    }

    #[test]
    fn builder_and_current() {
        let b = Breadcrumb::new().segments(["Home", "Docs", "API"]);
        assert_eq!(b.segment_count(), 3);
        assert_eq!(b.current(), Some("API"));
    }

    #[test]
    fn wide_bounds_show_all() {
        let mut b = Breadcrumb::new().segments(["a", "b", "c"]);
        paint(&mut b, 600.0);
        assert!(b.hidden_segments().is_empty());
        assert_eq!(b.plan.lock().visible.len(), 3);
    }

    #[test]
    fn narrow_bounds_collapse_leaders() {
        let mut b = Breadcrumb::new().segments(["alpha", "beta", "gamma", "delta", "epsilon"]);
        paint(&mut b, 120.0);
        assert!(!b.hidden_segments().is_empty());
        // The current segment always survives.
        assert_eq!(b.plan.lock().visible.last(), Some(&4));
        assert!(b.plan.lock().ellipsis_rect.is_some());
    }

    #[test]
    fn link_click_navigates() {
        let mut b = Breadcrumb::new().segments(["root", "mid", "leaf"]);
        paint(&mut b, 600.0);
        let r = b.plan.lock().rects[0];
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(r.origin.x + 2.0, r.origin.y + 2.0),
            button: PointerButton::Primary,
            count: 1,
        };
        assert_eq!(b.event(&mut ev(&press)), EventResponse::Handled);
        assert_eq!(b.take_navigated(), Some(0));
    }

    #[test]
    fn current_segment_not_clickable() {
        let mut b = Breadcrumb::new().segments(["root", "leaf"]);
        paint(&mut b, 600.0);
        // The last visible segment ("leaf") gets no hit rect — a press
        // anywhere inside it is ignored.
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(500.0, 14.0),
            button: PointerButton::Primary,
            count: 1,
        };
        assert_eq!(b.event(&mut ev(&press)), EventResponse::Ignored);
        assert_eq!(b.take_navigated(), None);
    }

    #[test]
    fn ellipsis_click_flags() {
        let mut b =
            Breadcrumb::new().segments(["a_long_segment", "another_long", "third_long", "current"]);
        paint(&mut b, 130.0);
        let r = b.plan.lock().ellipsis_rect.unwrap();
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(r.origin.x + 2.0, r.origin.y + 2.0),
            button: PointerButton::Primary,
            count: 1,
        };
        b.event(&mut ev(&press));
        assert!(b.take_ellipsis_activated());
    }

    #[test]
    fn keyboard_traverse_and_activate() {
        let mut b = Breadcrumb::new().segments(["x", "y", "z"]);
        paint(&mut b, 600.0);
        let right = WidgetEvent::KeyPressed {
            key: "ArrowRight".into(),
            repeat: false,
        };
        b.event(&mut ev(&right)); // highlight first target (seg 0)
        let enter = WidgetEvent::KeyPressed {
            key: "Enter".into(),
            repeat: false,
        };
        b.event(&mut ev(&enter));
        assert_eq!(b.take_navigated(), Some(0));
    }
}
