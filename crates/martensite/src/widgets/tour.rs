//! Guided tour — a step sequence of coach-mark cards over a dimmed
//! backdrop with an optional spotlight cutout around the target
//! (Ant `Tour`, driver.js, shepherd.js).
//!
//! The tour is an overlay leaf: place it in a `Stack` above the UI
//! being explained. Each [`TourStep`] carries an optional `target`
//! rectangle (in the tour widget's coordinate space); when present,
//! the backdrop is drawn around it as a spotlight and the card is
//! placed adjacent to it. Without a target the card centers.
//!
//! Footer buttons ride the shared [`Button`] activation seam:
//! Prev, Next/Finish, and an optional Skip. Escape skips the tour
//! (the universal dismiss affordance). Completion and dismissal are
//! surfaced through [`Tour::take_finished`] and
//! [`Tour::take_dismissed`] — the consumer decides what happens
//! next, exactly like the wizard's finish/cancel contract.
//!
//! # Examples
//!
//! ```
//! use martensite::prelude::Rect;
//! use martensite::widgets::{Tour, TourStep};
//!
//! let mut t = Tour::new()
//!     .step("Welcome", "This is the dashboard.", None)
//!     .step("Filter", "Type here to filter rows.", Some(Rect::new(8.0, 8.0, 160.0, 24.0)));
//! assert_eq!(t.step_count(), 2);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::text_paint::paint_label_clipped;
use crate::widgets::Button;

/// Backdrop dimming alpha (0–255). The spotlight cutout leaves the
/// target fully lit.
const DIM_ALPHA: u8 = 120;
/// Card width in logical points.
const CARD_W_PT: f32 = 240.0;
/// Card chrome — padding around the text block.
const PAD_PT: f32 = 12.0;
/// Footer button metrics (logical points, pre-scale).
const BUTTON_W_PT: f32 = 64.0;
const BUTTON_H_PT: f32 = 26.0;
const BUTTON_GAP_PT: f32 = 8.0;
/// Line pitch for wrapped body text.
const LINE_PT: f32 = 14.0;
/// Spotlight ring thickness around the target.
const RING_PT: f32 = 2.0;
/// Gap between the card and its target.
const CARD_GAP_PT: f32 = 10.0;

/// One coach-mark step.
///
/// # Examples
///
/// ```
/// use martensite::prelude::Rect;
/// use martensite::widgets::TourStep;
///
/// let s = TourStep::new("Hint", "Click here.").target(Rect::new(0.0, 0.0, 40.0, 20.0));
/// assert!(s.target.is_some());
/// ```
#[derive(Debug, Clone)]
pub struct TourStep {
    /// Card heading.
    pub title: String,
    /// Body copy.
    pub body: String,
    /// Spotlight rect in tour-widget coordinates. `None` → centered
    /// card over a full dim.
    pub target: Option<Rect>,
}

impl TourStep {
    /// Creates a step with `title` heading and `body` copy.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TourStep;
    ///
    /// assert_eq!(TourStep::new("t", "b").title, "t");
    /// ```
    pub fn new(title: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            body: body.into(),
            target: None,
        }
    }

    /// Sets the spotlight target rect.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::prelude::Rect;
    /// use martensite::widgets::TourStep;
    ///
    /// let s = TourStep::new("t", "b").target(Rect::new(1.0, 2.0, 3.0, 4.0));
    /// assert_eq!(s.target.unwrap().width(), 3.0);
    /// ```
    #[must_use]
    pub fn target(mut self, rect: Rect) -> Self {
        self.target = Some(rect);
        self
    }
}

/// Guided-tour overlay widget.
///
/// # Examples
///
/// ```
/// use martensite::widgets::{Tour, TourStep};
///
/// let t = Tour::new().step("a", "b", None);
/// assert!(t.active());
/// ```
pub struct Tour {
    /// Accessibility label override.
    pub label: Option<String>,
    /// Whether the overlay intercepts input.
    pub enabled: bool,
    /// Whether a Skip button appears.
    pub skippable: bool,
    steps: Vec<TourStep>,
    /// Whether the tour is on screen. Starts active once steps exist.
    active: bool,
    current: usize,
    finished: bool,
    dismissed: bool,
    /// Explicit text painter opt-in (sibling convention — ambient
    /// `PaintContext::text_painter` is used when unset).
    text_painter: Option<crate::text_paint::SharedTextPainter>,
    prev_button: Button,
    next_button: Button,
    skip_button: Button,
    bounds: Rect,
    card_bounds: Option<Rect>,
    prev_bounds: Option<Rect>,
    next_bounds: Option<Rect>,
    skip_bounds: Option<Rect>,
}

impl Tour {
    /// Creates an inactive empty tour.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Tour;
    ///
    /// assert!(!Tour::new().active());
    /// ```
    pub fn new() -> Self {
        Self {
            label: None,
            enabled: true,
            skippable: true,
            steps: Vec::new(),
            active: false,
            current: 0,
            finished: false,
            dismissed: false,
            text_painter: None,
            prev_button: Button::new("Prev"),
            next_button: Button::new("Next"),
            skip_button: Button::new("Skip"),
            bounds: Rect::default(),
            card_bounds: None,
            prev_bounds: None,
            next_bounds: None,
            skip_bounds: None,
        }
    }

    /// Appends a step. The first step activates the tour.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Tour;
    ///
    /// assert!(Tour::new().step("t", "b", None).active());
    /// ```
    #[must_use]
    pub fn step(
        mut self,
        title: impl Into<String>,
        body: impl Into<String>,
        target: Option<Rect>,
    ) -> Self {
        self.steps.push(TourStep {
            title: title.into(),
            body: body.into(),
            target,
        });
        if !self.steps.is_empty() {
            self.active = true;
        }
        self.sync_buttons();
        self
    }

    /// Sets the accessibility label.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Tour;
    ///
    /// let t = Tour::new().label("Feature tour");
    /// assert_eq!(t.label.as_deref(), Some("Feature tour"));
    /// ```
    #[must_use]
    pub fn label(mut self, text: impl Into<String>) -> Self {
        self.label = Some(text.into());
        self
    }

    /// Sets whether input reaches the tour.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Tour;
    ///
    /// assert!(Tour::new().enabled(false).enabled == false);
    /// ```
    #[must_use]
    pub fn enabled(mut self, flag: bool) -> Self {
        self.enabled = flag;
        self.sync_buttons();
        self
    }

    /// Sets whether the Skip affordance appears.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Tour;
    ///
    /// assert!(!Tour::new().skippable(false).skippable);
    /// ```
    #[must_use]
    pub fn skippable(mut self, flag: bool) -> Self {
        self.skippable = flag;
        self
    }

    /// Shares a [`crate::text_paint::TextPainter`] for real glyph
    /// runs in the card text.
    #[must_use]
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Sets the enabled flag post-build.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Tour;
    ///
    /// let mut t = Tour::new();
    /// t.set_enabled(false);
    /// assert!(!t.enabled);
    /// ```
    pub fn set_enabled(&mut self, flag: bool) {
        self.enabled = flag;
        self.sync_buttons();
    }

    /// Step count.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Tour;
    ///
    /// assert_eq!(Tour::new().step("a", "b", None).step_count(), 1);
    /// ```
    #[inline]
    pub fn step_count(&self) -> usize {
        self.steps.len()
    }

    /// Zero-based index of the current step.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Tour;
    ///
    /// assert_eq!(Tour::new().step("a", "b", None).current(), 0);
    /// ```
    #[inline]
    pub fn current(&self) -> usize {
        self.current
    }

    /// Whether the tour is on screen.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Tour;
    ///
    /// assert!(!Tour::new().active());
    /// ```
    #[inline]
    pub fn active(&self) -> bool {
        self.active
    }

    /// Advances to the next step; finishes past the last.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Tour;
    ///
    /// let mut t = Tour::new().step("a", "b", None);
    /// t.go_next();
    /// assert!(t.take_finished());
    /// ```
    pub fn go_next(&mut self) {
        if self.current + 1 < self.steps.len() {
            self.current += 1;
        } else {
            self.active = false;
            self.finished = true;
        }
        self.sync_buttons();
    }

    /// Returns to the previous step (clamped at zero).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Tour;
    ///
    /// let mut t = Tour::new().step("a", "b", None).step("c", "d", None);
    /// t.go_next();
    /// t.go_prev();
    /// assert_eq!(t.current(), 0);
    /// ```
    pub fn go_prev(&mut self) {
        self.current = self.current.saturating_sub(1);
        self.sync_buttons();
    }

    /// Ends the tour without completing it.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Tour;
    ///
    /// let mut t = Tour::new().step("a", "b", None);
    /// t.dismiss();
    /// assert!(t.take_dismissed());
    /// assert!(!t.take_finished());
    /// ```
    pub fn dismiss(&mut self) {
        self.active = false;
        self.dismissed = true;
    }

    /// Restarts from the first step.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Tour;
    ///
    /// let mut t = Tour::new().step("a", "b", None);
    /// t.go_next();
    /// t.restart();
    /// assert!(t.active());
    /// ```
    pub fn restart(&mut self) {
        self.current = 0;
        self.active = !self.steps.is_empty();
        self.finished = false;
        self.dismissed = false;
        self.sync_buttons();
    }

    /// Takes the parked finish flag.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Tour;
    ///
    /// assert!(!Tour::new().take_finished());
    /// ```
    #[inline]
    pub fn take_finished(&mut self) -> bool {
        std::mem::take(&mut self.finished)
    }

    /// Takes the parked dismissal flag.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Tour;
    ///
    /// assert!(!Tour::new().take_dismissed());
    /// ```
    #[inline]
    pub fn take_dismissed(&mut self) -> bool {
        std::mem::take(&mut self.dismissed)
    }

    fn sync_buttons(&mut self) {
        let last = self.current + 1 >= self.steps.len();
        self.prev_button.enabled = self.enabled && self.current > 0;
        self.next_button.enabled = self.enabled && !self.steps.is_empty();
        self.next_button.label = if last { "Done" } else { "Next" }.to_string();
        self.skip_button.enabled = self.enabled;
    }

    /// Child indices: prev (0), next (1), skip (2 when skippable).
    fn total_children(&self) -> usize {
        if self.skippable {
            3
        } else {
            2
        }
    }

    fn skip_index(&self) -> usize {
        2
    }

    /// Drains footer activations into navigation.
    fn poll_buttons(&mut self) {
        if self.prev_button.take_activated() {
            self.go_prev();
        }
        if self.next_button.take_activated() {
            self.go_next();
        }
        if self.skippable && self.skip_button.take_activated() {
            self.dismiss();
        }
    }

    /// Card placement: below the target when room, above otherwise,
    /// centered when there is no target.
    fn card_rect(&self, target: Option<Rect>, card_h: f32) -> Rect {
        let w = CARD_W_PT.min(self.bounds.width());
        let b = self.bounds;
        let x = target.map_or_else(
            || b.min_x() + (b.width() - w) / 2.0,
            |t| {
                (t.min_x() + (t.width() - w) / 2.0)
                    .max(b.min_x() + PAD_PT)
                    .min((b.max_x() - PAD_PT - w).max(b.min_x() + PAD_PT))
            },
        );
        let y = target.map_or_else(
            || b.min_y() + (b.height() - card_h) / 2.0,
            |t| {
                let below = t.max_y() + CARD_GAP_PT;
                if below + card_h <= b.max_y() - PAD_PT {
                    below
                } else {
                    (t.min_y() - CARD_GAP_PT - card_h)
                        .max(b.min_y() + PAD_PT)
                        .min((b.max_y() - PAD_PT - card_h).max(b.min_y() + PAD_PT))
                }
            },
        );
        Rect::new(x, y, w, card_h)
    }
}

impl Default for Tour {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for Tour {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tour")
            .field("steps", &self.steps.len())
            .field("current", &self.current)
            .field("active", &self.active)
            .finish()
    }
}

impl Widget for Tour {
    fn measure(&mut self, _cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        // Overlays fill whatever they are given.
        constraints.max_size.max(Vec2::ZERO)
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(160.0, 120.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        if !self.active || self.steps.is_empty() {
            self.card_bounds = None;
            self.prev_bounds = None;
            self.next_bounds = None;
            self.skip_bounds = None;
            return;
        }
        let step = &self.steps[self.current.min(self.steps.len() - 1)];
        let pad = cx.pt(PAD_PT);
        let body_w = (CARD_W_PT.min(bounds.width()) - 2.0 * pad).max(1.0);
        // No painter in `LayoutContext` — estimate wrapped lines with
        // the sibling 0.5-em-per-char heuristic, clamped to a card
        // that stays usable.
        let chars_per_line = (body_w / (cx.pt(LINE_PT) * 0.45)).max(1.0) as usize;
        let body_lines = (step.body.chars().count() / chars_per_line + 1).clamp(1, 8);
        let body_h = cx.pt(LINE_PT) * body_lines as f32;
        let title_h = cx.pt(LINE_PT + 2.0);
        let footer_h = cx.pt(BUTTON_H_PT);
        let card_h = pad + title_h + body_h + pad + footer_h + pad;
        let card = self.card_rect(step.target, card_h);
        self.card_bounds = Some(card);
        // Footer: [Skip ..... Prev] [Next]
        let bw = cx.pt(BUTTON_W_PT).min(card.width() / 3.0);
        let bh = footer_h;
        let by = card.max_y() - pad - bh;
        let mut bx = card.max_x() - pad - bw;
        let next_r = Rect::new(bx, by, bw, bh);
        bx -= bw + cx.pt(BUTTON_GAP_PT);
        let prev_r = Rect::new(bx, by, bw, bh);
        let skip_r = Rect::new(card.min_x() + pad, by, bw, bh);
        self.next_bounds = Some(next_r);
        self.prev_bounds = Some(prev_r);
        self.skip_bounds = self.skippable.then_some(skip_r);
        cx.layout_child(&mut self.next_button, next_r);
        cx.layout_child(&mut self.prev_button, prev_r);
        if self.skippable {
            cx.layout_child(&mut self.skip_button, skip_r);
        }
        self.sync_buttons();
    }

    fn paint(&self, cx: &mut PaintContext) {
        if !self.active || self.steps.is_empty() {
            return;
        }
        let Some(card) = self.card_bounds else { return };
        let step = &self.steps[self.current.min(self.steps.len() - 1)];
        let b = cx.bounds;
        let dim = [0u8, 0, 0, DIM_ALPHA];
        let surface = cx.color(TokenKey::SurfaceColor, [45, 45, 48, 255]);
        let border = cx.color(TokenKey::BorderColor, [90, 90, 90, 255]);
        let fg = cx.color(TokenKey::TextColor, [220, 220, 220, 255]);
        let accent = cx.color(TokenKey::AccentColor, [0, 122, 204, 255]);
        let muted = cx.color(TokenKey::TextMutedColor, [140, 140, 140, 255]);
        // Dim the whole bounds, carving out the spotlight target by
        // drawing the four surrounding slabs instead of a hole.
        let f = |r: Rect| {
            kurbo::Rect::new(
                f64::from(r.min_x()),
                f64::from(r.min_y()),
                f64::from(r.max_x()),
                f64::from(r.max_y()),
            )
        };
        if let Some(t) = step.target {
            let ty = t.min_y().max(b.min_y());
            let th = (t.max_y().min(b.max_y()) - ty).max(0.0);
            // Top / bottom / left / right slabs around the target.
            cx.list.push_fill_rect(
                f(Rect::new(b.min_x(), b.min_y(), b.width(), ty - b.min_y())),
                dim,
            );
            cx.list.push_fill_rect(
                f(Rect::new(
                    b.min_x(),
                    ty + th,
                    b.width(),
                    b.max_y() - ty - th,
                )),
                dim,
            );
            cx.list.push_fill_rect(
                f(Rect::new(
                    b.min_x(),
                    ty,
                    t.min_x().max(b.min_x()) - b.min_x(),
                    th,
                )),
                dim,
            );
            cx.list.push_fill_rect(
                f(Rect::new(
                    t.max_x().min(b.max_x()),
                    ty,
                    b.max_x() - t.max_x().min(b.max_x()),
                    th,
                )),
                dim,
            );
            // Spotlight ring.
            cx.list.push_stroke_rect(f(t), cx.pt(RING_PT), accent);
        } else {
            cx.list.push_fill_rect(f(b), dim);
        }
        // Card.
        let card_shape = martensite_core::shape::Shape::rounded(cx.pt(8.0));
        cx.list.push_fill_shape(f(card), &card_shape, surface);
        cx.list
            .push_stroke_shape(f(card), &card_shape, 1.0_f32.max(cx.pt(0.5)), border);
        let pad = cx.pt(PAD_PT);
        let title_h = cx.pt(LINE_PT + 2.0);
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let text_x = card.min_x() + pad;
        let title_clip = Rect::new(
            text_x,
            card.min_y() + pad,
            card.width() - 2.0 * pad,
            title_h,
        );
        let title_size = 14.0 * cx.scale;
        paint_label_clipped(
            painter,
            cx.list,
            f(title_clip),
            kurbo::Point::new(f64::from(text_x), f64::from(title_clip.min_y())),
            &step.title,
            title_size,
            fg,
        );
        let counter = format!("{} / {}", self.current + 1, self.steps.len());
        let counter_size = 10.0 * cx.scale;
        let counter_w = painter
            .and_then(|p| p.measure_text(&counter, counter_size))
            .unwrap_or(counter_size * counter.chars().count() as f32 * 0.5);
        let counter_clip = Rect::new(
            card.max_x() - pad - counter_w - cx.pt(4.0),
            card.min_y() + pad,
            counter_w + cx.pt(4.0),
            title_h,
        );
        paint_label_clipped(
            painter,
            cx.list,
            f(counter_clip),
            kurbo::Point::new(
                f64::from(counter_clip.min_x()),
                f64::from(counter_clip.min_y() + (title_h - counter_size) / 2.0),
            ),
            &counter,
            counter_size,
            muted,
        );
        // Body.
        let body_size = 12.0 * cx.scale;
        let body_clip = Rect::new(
            text_x,
            card.min_y() + pad + title_h,
            card.width() - 2.0 * pad,
            (self.skip_bounds.or(self.next_bounds))
                .map(|r| r.min_y() - pad - (card.min_y() + pad + title_h))
                .unwrap_or(card.height() - 2.0 * pad - title_h)
                .max(0.0),
        );
        paint_label_clipped(
            painter,
            cx.list,
            f(body_clip),
            kurbo::Point::new(f64::from(text_x), f64::from(body_clip.min_y())),
            &step.body,
            body_size,
            fg,
        );
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.enabled || !self.active {
            return EventResponse::Ignored;
        }
        self.poll_buttons();
        if let WidgetEvent::KeyPressed { key, .. } = cx.event {
            if key == "Escape" {
                self.dismiss();
                return EventResponse::Handled;
            }
        }
        // Modal: swallow every pointer event inside the overlay so
        // nothing bleeds to the dimmed UI below; forward the ones on
        // the buttons.
        let mut response = EventResponse::Ignored;
        if let WidgetEvent::PointerPressed { position, .. }
        | WidgetEvent::PointerReleased { position, .. }
        | WidgetEvent::PointerMoved { position, .. }
        | WidgetEvent::Scroll { position, .. } = cx.event
        {
            let pos = *position;
            for i in (0..self.child_count()).rev() {
                let Some(cb) = self.child_bounds(i) else {
                    continue;
                };
                if !cb.contains(pos) {
                    continue;
                }
                let mut child_cx = EventContext {
                    event: cx.event,
                    bounds: cb,
                    scale: cx.scale,
                };
                if let Some(child) = self.child_mut(i) {
                    response = child.event(&mut child_cx);
                }
                break;
            }
            self.poll_buttons();
            return if response == EventResponse::Ignored {
                EventResponse::Handled
            } else {
                response
            };
        }
        self.poll_buttons();
        response
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Dialog);
        if let Some(ref label) = self.label {
            node.set_label(label.as_str());
        }
        if !self.enabled {
            node.set_disabled();
        }
        if !self.active {
            node.set_hidden();
        }
        if let Some(step) = self.steps.get(self.current) {
            node.set_description(step.title.clone());
        }
    }

    fn child_count(&self) -> usize {
        self.total_children()
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        match index {
            0 => Some(&self.prev_button),
            1 => Some(&self.next_button),
            _ if self.skippable && index == self.skip_index() => Some(&self.skip_button),
            _ => None,
        }
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        match index {
            0 => Some(&mut self.prev_button),
            1 => Some(&mut self.next_button),
            _ if self.skippable && index == self.skip_index() => Some(&mut self.skip_button),
            _ => None,
        }
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        if !self.active {
            return None;
        }
        match index {
            0 => self.prev_bounds,
            1 => self.next_bounds,
            _ if self.skippable && index == self.skip_index() => self.skip_bounds,
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::{HotNode, WidgetEvent};

    fn laid_out(w: &mut Tour, width: f32, height: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        w.layout(&mut cx, Rect::new(0.0, 0.0, width, height));
    }

    fn ev(w: &mut Tour, event: &WidgetEvent) -> EventResponse {
        let mut cx = EventContext {
            event,
            bounds: Rect::new(0.0, 0.0, 400.0, 300.0),
            scale: 1.0,
        };
        w.event(&mut cx)
    }

    #[test]
    fn starts_inactive_until_first_step() {
        assert!(!Tour::new().active());
        assert!(Tour::new().step("a", "b", None).active());
    }

    #[test]
    fn next_advances_then_finishes() {
        let mut t = Tour::new().step("a", "b", None).step("c", "d", None);
        t.go_next();
        assert_eq!(t.current(), 1);
        t.go_next();
        assert!(!t.active());
        assert!(t.take_finished());
        assert!(!t.take_dismissed());
    }

    #[test]
    fn prev_clamps_at_first() {
        let mut t = Tour::new().step("a", "b", None);
        t.go_prev();
        assert_eq!(t.current(), 0);
    }

    #[test]
    fn dismiss_parks_flag() {
        let mut t = Tour::new().step("a", "b", None);
        t.dismiss();
        assert!(!t.active());
        assert!(t.take_dismissed());
        assert!(!t.take_finished());
    }

    #[test]
    fn escape_dismisses() {
        let mut t = Tour::new().step("a", "b", None);
        laid_out(&mut t, 400.0, 300.0);
        let esc = WidgetEvent::KeyPressed {
            key: "Escape".to_string(),
            repeat: false,
        };
        assert_eq!(ev(&mut t, &esc), EventResponse::Handled);
        assert!(t.take_dismissed());
    }

    #[test]
    fn next_button_activates_via_click() {
        let mut t = Tour::new().step("a", "b", None).step("c", "d", None);
        laid_out(&mut t, 400.0, 300.0);
        let nb = t.next_bounds.unwrap();
        let mid = Vec2::new(
            nb.min_x() + nb.width() / 2.0,
            nb.min_y() + nb.height() / 2.0,
        );
        let down = WidgetEvent::PointerPressed {
            position: mid,
            button: martensite_core::PointerButton::Primary,
            count: 1,
        };
        let up = WidgetEvent::PointerReleased {
            position: mid,
            button: martensite_core::PointerButton::Primary,
        };
        ev(&mut t, &down);
        ev(&mut t, &up);
        assert_eq!(t.current(), 1);
    }

    #[test]
    fn skip_button_dismisses() {
        let mut t = Tour::new().step("a", "b", None);
        laid_out(&mut t, 400.0, 300.0);
        let sb = t.skip_bounds.unwrap();
        let mid = Vec2::new(
            sb.min_x() + sb.width() / 2.0,
            sb.min_y() + sb.height() / 2.0,
        );
        let down = WidgetEvent::PointerPressed {
            position: mid,
            button: martensite_core::PointerButton::Primary,
            count: 1,
        };
        let up = WidgetEvent::PointerReleased {
            position: mid,
            button: martensite_core::PointerButton::Primary,
        };
        ev(&mut t, &down);
        ev(&mut t, &up);
        assert!(t.take_dismissed());
    }

    #[test]
    fn backdrop_swallows_clicks() {
        let mut t = Tour::new().step("a", "b", None);
        laid_out(&mut t, 400.0, 300.0);
        let down = WidgetEvent::PointerPressed {
            position: Vec2::new(5.0, 5.0),
            button: martensite_core::PointerButton::Primary,
            count: 1,
        };
        // Outside the card and all buttons — still Handled (modal).
        assert_eq!(ev(&mut t, &down), EventResponse::Handled);
        assert!(!t.take_dismissed());
        assert_eq!(t.current(), 0);
    }

    #[test]
    fn hidden_children_have_no_bounds() {
        let mut t = Tour::new().step("a", "b", None);
        t.dismiss();
        laid_out(&mut t, 400.0, 300.0);
        assert!(t.child_bounds(0).is_none());
        assert!(t.child_bounds(1).is_none());
    }

    #[test]
    fn restart_recovers() {
        let mut t = Tour::new().step("a", "b", None);
        t.dismiss();
        t.restart();
        assert!(t.active());
        assert!(!t.take_dismissed());
        assert_eq!(t.current(), 0);
    }

    #[test]
    fn disabled_inert() {
        let mut t = Tour::new().step("a", "b", None).enabled(false);
        laid_out(&mut t, 400.0, 300.0);
        let esc = WidgetEvent::KeyPressed {
            key: "Escape".to_string(),
            repeat: false,
        };
        assert_eq!(ev(&mut t, &esc), EventResponse::Ignored);
        assert!(!t.take_dismissed());
    }

    #[test]
    fn child_topology() {
        let t = Tour::new().step("a", "b", None);
        assert_eq!(t.child_count(), 3);
        assert!(t.child(0).is_some());
        assert!(t.child(3).is_none());
        let t2 = Tour::new().step("a", "b", None).skippable(false);
        assert_eq!(t2.child_count(), 2);
    }

    #[test]
    fn hotspot_nodes_present() {
        let mut t = Tour::new().step("a", "b", None);
        laid_out(&mut t, 400.0, 300.0);
        let _ = HotNode::default();
        assert!(t.card_bounds.is_some());
        assert!(t.next_bounds.is_some());
        assert!(t.skip_bounds.is_some());
    }
}
