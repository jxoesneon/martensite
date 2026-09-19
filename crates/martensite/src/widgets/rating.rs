//! `Rating` widget: a star-style rating input/display (WinUI
//! `RatingControl`, KDE `KRatingWidget`, Ant `Rate`).
//!
//! Paints `max` glyph cells (★ by default); the user clicks or
//! arrow-keys to set a value. Supports half steps, read-only display
//! mode, and clear-on-repeat-click (clicking the current value resets
//! to zero, matching Ant `Rate allowClear`). Poll
//! [`Rating::take_changed`] for new values.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::rating::Rating;
//!
//! let r = Rating::new().half_steps(true).value(3.5);
//! assert_eq!(r.get_value(), 3.5);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::widget::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Widget, WidgetEvent,
};
use martensite_core::{Rect, TokenKey};

/// Glyph cell size, logical points.
const CELL_PT: f32 = 20.0;
/// Gap between cells, logical points.
const GAP_PT: f32 = 3.0;
/// Glyph font size, logical points.
const GLYPH_PT: f32 = 16.0;

/// Filled-star ink.
const STAR_ON: [u8; 4] = [240, 180, 40, 255];
/// Empty-star ink.
const STAR_OFF: [u8; 4] = [200, 203, 210, 255];
/// Hovered-star ink (preview).
const STAR_HOT: [u8; 4] = [250, 205, 90, 255];
/// Disabled ink.
const STAR_DIM: [u8; 4] = [170, 174, 183, 140];

/// A star-rating input or read-only display.
///
/// # Examples
///
/// ```
/// use martensite::widgets::rating::Rating;
///
/// let r = Rating::new().max(5).value(4.0);
/// assert_eq!(r.get_value(), 4.0);
/// ```
pub struct Rating {
    /// Number of cells.
    max: usize,
    /// Current value (0..=max, half steps when enabled).
    value: f32,
    /// Whether half-step selection is allowed.
    half_steps: bool,
    /// Read-only display mode (no interaction).
    read_only: bool,
    /// Clicking the current value clears to zero.
    allow_clear: bool,
    /// Enabled flag — mirrors the sibling-widget convention.
    enabled: bool,
    /// Hover preview value (None = not hovered).
    preview: Option<f32>,
    /// Pending change notification.
    changed: Option<f32>,
    /// Per-cell hit rects from the last layout.
    cell_rects: Vec<Rect>,
    /// Shared shaped-text painter.
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl Rating {
    /// Creates a 5-star rating.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::rating::Rating;
    ///
    /// let r = Rating::new();
    /// assert_eq!(r.get_value(), 0.0);
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self {
            max: 5,
            value: 0.0,
            half_steps: false,
            read_only: false,
            allow_clear: true,
            enabled: true,
            preview: None,
            changed: None,
            cell_rects: Vec::new(),
            text_painter: None,
        }
    }

    /// Sets the cell count.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::rating::Rating;
    ///
    /// let r = Rating::new().max(10);
    /// ```
    #[must_use]
    pub fn max(mut self, max: usize) -> Self {
        self.max = max.max(1);
        self.value = self.value.min(self.max as f32);
        self
    }

    /// Sets the current value (clamped to `0..=max`, snapped to the
    /// step granularity).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::rating::Rating;
    ///
    /// let r = Rating::new().half_steps(true).value(3.7);
    /// assert_eq!(r.get_value(), 3.5);
    /// ```
    #[must_use]
    pub fn value(mut self, value: f32) -> Self {
        self.value = self.snap(value);
        self
    }

    /// Enables half-step granularity.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::rating::Rating;
    ///
    /// let r = Rating::new().half_steps(true);
    /// ```
    #[must_use]
    pub fn half_steps(mut self, enabled: bool) -> Self {
        self.half_steps = enabled;
        self.value = self.snap(self.value);
        self
    }

    /// Sets read-only display mode.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::rating::Rating;
    ///
    /// let r = Rating::new().read_only(true);
    /// ```
    #[must_use]
    pub fn read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }

    /// Whether re-clicking the current value clears it (default true).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::rating::Rating;
    ///
    /// let r = Rating::new().allow_clear(false);
    /// ```
    #[must_use]
    pub fn allow_clear(mut self, allow: bool) -> Self {
        self.allow_clear = allow;
        self
    }

    /// Enables or disables the widget.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::rating::Rating;
    ///
    /// let r = Rating::new().enabled(false);
    /// ```
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// The current value.
    #[inline]
    #[must_use]
    pub fn get_value(&self) -> f32 {
        self.value
    }

    /// Sets the value programmatically.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::rating::Rating;
    ///
    /// let mut r = Rating::new();
    /// r.set_value(4.0);
    /// assert_eq!(r.get_value(), 4.0);
    /// ```
    pub fn set_value(&mut self, value: f32) {
        self.value = self.snap(value);
    }

    /// Drains a change notification — the new value after user input.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::rating::Rating;
    ///
    /// let mut r = Rating::new();
    /// assert_eq!(r.take_changed(), None);
    /// ```
    pub fn take_changed(&mut self) -> Option<f32> {
        self.changed.take()
    }

    /// Installs a shared shaped-text painter.
    #[must_use]
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Snaps a value to the granularity and clamps to `0..=max`.
    fn snap(&self, value: f32) -> f32 {
        let step = if self.half_steps { 0.5 } else { 1.0 };
        (value / step).round().clamp(0.0, self.max as f32 / step) * step
    }

    /// Maps a pointer x-position within cell `i` to a value.
    fn value_at(&self, index: usize, position: Vec2) -> f32 {
        if self.half_steps {
            let cell = self.cell_rects[index];
            let frac = ((position.x - cell.origin.x) / cell.size.x).clamp(0.0, 1.0);
            index as f32 + if frac <= 0.5 { 0.5 } else { 1.0 }
        } else {
            index as f32 + 1.0
        }
    }

    /// Applies a user-chosen value (clear-on-repeat, notify).
    fn commit(&mut self, value: f32) {
        let new = if self.allow_clear && (value - self.value).abs() < f32::EPSILON {
            0.0
        } else {
            value
        };
        if (new - self.value).abs() > f32::EPSILON {
            self.value = new;
            self.changed = Some(new);
        }
    }
}

impl Default for Rating {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Rating {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let w =
            self.max as f32 * cx.pt(CELL_PT) + (self.max.saturating_sub(1)) as f32 * cx.pt(GAP_PT);
        Vec2::new(w.min(constraints.max_size.x.max(0.0)), cx.pt(CELL_PT))
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.cell_rects.clear();
        let cell = cx.pt(CELL_PT);
        let gap = cx.pt(GAP_PT);
        for i in 0..self.max {
            self.cell_rects.push(Rect::new(
                bounds.origin.x + i as f32 * (cell + gap),
                bounds.origin.y,
                cell,
                cell,
            ));
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Slider);
        node.set_label("Rating");
        node.set_numeric_value(f64::from(self.value));
        node.set_min_numeric_value(0.0);
        node.set_max_numeric_value(self.max as f64);
        if self.read_only {
            node.set_read_only();
        }
        if !self.enabled {
            node.set_disabled();
        }
        if self.enabled && !self.read_only {
            node.add_action(accesskit::Action::SetValue);
            node.add_action(accesskit::Action::Increment);
            node.add_action(accesskit::Action::Decrement);
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.enabled || self.read_only {
            return EventResponse::Ignored;
        }
        match cx.event {
            WidgetEvent::PointerMoved { position } => {
                let hit = self
                    .cell_rects
                    .iter()
                    .position(|r| r.contains(*position))
                    .map(|i| self.value_at(i, *position));
                if hit != self.preview {
                    self.preview = hit;
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Handled
            }
            WidgetEvent::PointerLeave => {
                if self.preview.take().is_some() {
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                ..
            } => {
                if let Some(i) = self.cell_rects.iter().position(|r| r.contains(*position)) {
                    self.commit(self.value_at(i, *position));
                    self.preview = Some(self.value);
                    return EventResponse::Handled;
                }
                EventResponse::Ignored
            }
            WidgetEvent::KeyPressed { key, .. } => match key.as_str() {
                "ArrowRight" | "ArrowUp" => {
                    let step = if self.half_steps { 0.5 } else { 1.0 };
                    self.commit((self.value + step).min(self.max as f32));
                    EventResponse::Handled
                }
                "ArrowLeft" | "ArrowDown" => {
                    let step = if self.half_steps { 0.5 } else { 1.0 };
                    self.commit((self.value - step).max(0.0));
                    EventResponse::Handled
                }
                "Home" => {
                    self.commit(0.0);
                    EventResponse::Handled
                }
                "End" => {
                    self.commit(self.max as f32);
                    EventResponse::Handled
                }
                _ => EventResponse::Ignored,
            },
            WidgetEvent::SemanticAction(action) => match action {
                martensite_core::widget::SemanticAction::Increment => {
                    let step = if self.half_steps { 0.5 } else { 1.0 };
                    self.commit((self.value + step).min(self.max as f32));
                    EventResponse::Handled
                }
                martensite_core::widget::SemanticAction::Decrement => {
                    let step = if self.half_steps { 0.5 } else { 1.0 };
                    self.commit((self.value - step).max(0.0));
                    EventResponse::Handled
                }
                _ => EventResponse::Ignored,
            },
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let glyph = "★";
        let size = cx.pt(GLYPH_PT);
        let shown = self.preview.unwrap_or(self.value);
        for (i, r) in self.cell_rects.iter().enumerate() {
            let fill = (shown - i as f32).clamp(0.0, 1.0);
            let ink = if !self.enabled {
                STAR_DIM
            } else if self.preview.is_some() {
                STAR_HOT
            } else if fill >= 1.0 {
                STAR_ON
            } else if fill > 0.0 {
                STAR_HOT
            } else {
                cx.color(TokenKey::BorderColor, STAR_OFF)
            };
            let w = painter
                .and_then(|p| p.measure_text(glyph, size))
                .unwrap_or(size);
            let x = r.origin.x + (r.size.x - w) / 2.0;
            let y = r.origin.y + (r.size.y - size) / 2.0;
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                kurbo::Rect::new(
                    f64::from(r.min_x()),
                    f64::from(r.min_y()),
                    f64::from(r.max_x()),
                    f64::from(r.max_y()),
                ),
                kurbo::Point::new(f64::from(x), f64::from(y)),
                glyph,
                size,
                ink,
            );
        }
    }
}

impl std::fmt::Debug for Rating {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Rating")
            .field("value", &self.value)
            .field("max", &self.max)
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
            bounds: Rect::new(0.0, 0.0, 120.0, 20.0),
            scale: 1.0,
        }
    }

    fn laid_out(value: f32) -> Rating {
        let mut r = Rating::new().value(value);
        let mut hot = HotNode::default();
        r.layout(&mut make_cx(&mut hot), Rect::new(0.0, 0.0, 120.0, 20.0));
        r
    }

    #[test]
    fn builder_snaps_and_clamps() {
        assert_eq!(Rating::new().value(3.7).get_value(), 4.0);
        assert_eq!(Rating::new().half_steps(true).value(3.7).get_value(), 3.5);
        assert_eq!(Rating::new().value(9.0).get_value(), 5.0);
    }

    #[test]
    fn click_sets_value() {
        let mut r = laid_out(0.0);
        let cell = r.cell_rects[2];
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(cell.origin.x + 2.0, cell.origin.y + 2.0),
            button: PointerButton::Primary,
            count: 1,
        };
        assert_eq!(r.event(&mut ev(&press)), EventResponse::Handled);
        assert_eq!(r.get_value(), 3.0);
        assert_eq!(r.take_changed(), Some(3.0));
    }

    #[test]
    fn click_current_clears() {
        let mut r = laid_out(3.0);
        let cell = r.cell_rects[2];
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(cell.origin.x + 2.0, cell.origin.y + 2.0),
            button: PointerButton::Primary,
            count: 1,
        };
        r.event(&mut ev(&press));
        assert_eq!(r.get_value(), 0.0);
        assert_eq!(r.take_changed(), Some(0.0));
    }

    #[test]
    fn allow_clear_false_keeps() {
        let mut r = Rating::new().value(3.0).allow_clear(false);
        let mut hot = HotNode::default();
        r.layout(&mut make_cx(&mut hot), Rect::new(0.0, 0.0, 120.0, 20.0));
        let cell = r.cell_rects[2];
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(cell.origin.x + 2.0, cell.origin.y + 2.0),
            button: PointerButton::Primary,
            count: 1,
        };
        r.event(&mut ev(&press));
        assert_eq!(r.get_value(), 3.0);
        assert_eq!(r.take_changed(), None); // unchanged → no signal
    }

    #[test]
    fn arrows_step() {
        let mut r = laid_out(2.0);
        let right = WidgetEvent::KeyPressed {
            key: "ArrowRight".into(),
            repeat: false,
        };
        r.event(&mut ev(&right));
        assert_eq!(r.get_value(), 3.0);
        let left = WidgetEvent::KeyPressed {
            key: "ArrowLeft".into(),
            repeat: false,
        };
        r.event(&mut ev(&left));
        r.event(&mut ev(&left));
        assert_eq!(r.get_value(), 1.0);
    }

    #[test]
    fn read_only_ignores_input() {
        let mut r = Rating::new().read_only(true).value(2.0);
        let mut hot = HotNode::default();
        r.layout(&mut make_cx(&mut hot), Rect::new(0.0, 0.0, 120.0, 20.0));
        let cell = r.cell_rects[4];
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(cell.origin.x + 2.0, cell.origin.y + 2.0),
            button: PointerButton::Primary,
            count: 1,
        };
        assert_eq!(r.event(&mut ev(&press)), EventResponse::Ignored);
        assert_eq!(r.get_value(), 2.0);
    }

    #[test]
    fn half_step_click() {
        let mut r = Rating::new().half_steps(true);
        let mut hot = HotNode::default();
        r.layout(&mut make_cx(&mut hot), Rect::new(0.0, 0.0, 120.0, 20.0));
        let cell = r.cell_rects[1];
        // Left half of cell 1 → 1.5.
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(cell.origin.x + 1.0, cell.origin.y + 2.0),
            button: PointerButton::Primary,
            count: 1,
        };
        r.event(&mut ev(&press));
        assert_eq!(r.get_value(), 1.5);
    }

    #[test]
    fn hover_preview_repaints() {
        let mut r = laid_out(1.0);
        let cell = r.cell_rects[3];
        let mv = WidgetEvent::PointerMoved {
            position: Vec2::new(cell.origin.x + 2.0, cell.origin.y + 2.0),
        };
        assert_eq!(r.event(&mut ev(&mv)), EventResponse::RequestRepaint);
        assert_eq!(r.preview, Some(4.0));
        let leave = WidgetEvent::PointerLeave;
        r.event(&mut ev(&leave));
        assert_eq!(r.preview, None);
    }
}
