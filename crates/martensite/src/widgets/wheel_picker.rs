//! Wheel picker — a vertically scrollable drum of options that
//! snaps to the centered row (iOS `UIPickerView`, SwiftUI
//! `.pickerStyle(.wheel)`, Ant `PickerView`).
//!
//! The selected row sits in a fixed highlight window; wheel
//! scrolling and vertical drags rotate the drum with rubber-band
//! resistance at the ends. Release snaps to the nearest row and
//! emits through [`WheelPicker::take_selected`].
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::WheelPicker;
//!
//! let w = WheelPicker::new().items(["a", "b", "c"]);
//! assert_eq!(w.item_count(), 3);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::text_paint::paint_label_clipped;

/// Row height in points.
const ROW_PT: f32 = 32.0;
/// Rows visible above/below the selection window.
const VISIBLE_HALF: usize = 2;
/// End-stretch resistance while dragging past the bounds.
const RESIST: f32 = 0.35;

/// Wheel picker.
///
/// # Examples
///
/// ```
/// use martensite::widgets::WheelPicker;
///
/// assert_eq!(WheelPicker::new().selected(), 0);
/// ```
pub struct WheelPicker {
    /// Accessibility label override.
    pub label: Option<String>,
    /// Whether input reaches the drum.
    pub enabled: bool,
    /// Row height in points.
    pub row_height: f32,
    items: Vec<String>,
    /// Selected index (the row in the window).
    selected: usize,
    /// Continuous row position under the window (drag integration).
    drag_row: f32,
    drag_y: Option<f32>,
    drag_start_row: f32,
    /// Parked index after a snap-settle.
    picked: Option<usize>,
    bounds: Rect,
    window_bounds: Option<Rect>,
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl WheelPicker {
    /// Creates an empty picker.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::WheelPicker;
    ///
    /// assert_eq!(WheelPicker::new().item_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: None,
            enabled: true,
            row_height: ROW_PT,
            items: Vec::new(),
            selected: 0,
            drag_row: 0.0,
            drag_y: None,
            drag_start_row: 0.0,
            picked: None,
            bounds: Rect::default(),
            window_bounds: None,
            text_painter: None,
        }
    }

    /// Sets the items.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::WheelPicker;
    ///
    /// assert_eq!(WheelPicker::new().items(["a", "b"]).item_count(), 2);
    /// ```
    #[must_use]
    pub fn items(mut self, items: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.items = items.into_iter().map(Into::into).collect();
        self.selected = self.selected.min(self.items.len().saturating_sub(1));
        self.drag_row = self.selected as f32;
        self
    }

    /// Sets the initial selection (clamped).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::WheelPicker;
    ///
    /// assert_eq!(WheelPicker::new().items(["a", "b"]).selected_index(1).selected(), 1);
    /// ```
    #[must_use]
    pub fn selected_index(mut self, index: usize) -> Self {
        self.selected = index.min(self.items.len().saturating_sub(1));
        self.drag_row = self.selected as f32;
        self
    }

    /// Sets the accessibility label.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::WheelPicker;
    ///
    /// let w = WheelPicker::new().label("Size");
    /// assert_eq!(w.label.as_deref(), Some("Size"));
    /// ```
    #[must_use]
    pub fn label(mut self, text: impl Into<String>) -> Self {
        self.label = Some(text.into());
        self
    }

    /// Sets whether input reaches the drum.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::WheelPicker;
    ///
    /// assert!(!WheelPicker::new().enabled(false).enabled);
    /// ```
    #[must_use]
    pub fn enabled(mut self, flag: bool) -> Self {
        self.enabled = flag;
        self
    }

    /// Shares a [`crate::text_paint::TextPainter`] for item text.
    #[must_use]
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Item count.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::WheelPicker;
    ///
    /// assert_eq!(WheelPicker::new().item_count(), 0);
    /// ```
    #[inline]
    pub fn item_count(&self) -> usize {
        self.items.len()
    }

    /// Selected index.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::WheelPicker;
    ///
    /// assert_eq!(WheelPicker::new().selected(), 0);
    /// ```
    #[inline]
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// Selected item text, if any.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::WheelPicker;
    ///
    /// assert!(WheelPicker::new().selected_item().is_none());
    /// ```
    pub fn selected_item(&self) -> Option<&str> {
        self.items.get(self.selected).map(String::as_str)
    }

    /// Sets the selection post-build (no emit).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::WheelPicker;
    ///
    /// let mut w = WheelPicker::new().items(["a", "b"]);
    /// w.select(1);
    /// assert_eq!(w.selected(), 1);
    /// ```
    pub fn select(&mut self, index: usize) {
        self.selected = index.min(self.items.len().saturating_sub(1));
        self.drag_row = self.selected as f32;
    }

    /// Takes the parked pick after a snap-settle.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::WheelPicker;
    ///
    /// assert!(WheelPicker::new().take_selected().is_none());
    /// ```
    #[inline]
    pub fn take_selected(&mut self) -> Option<usize> {
        self.picked.take()
    }

    /// Row height in device px.
    fn row_h(&self, scale: f32) -> f32 {
        self.row_height * scale
    }

    /// Settles `drag_row` to the nearest valid row and emits.
    fn settle(&mut self) {
        if self.items.is_empty() {
            return;
        }
        let snapped = self
            .drag_row
            .round()
            .clamp(0.0, self.items.len() as f32 - 1.0);
        self.drag_row = snapped;
        let idx = snapped as usize;
        if idx != self.selected {
            self.selected = idx;
            self.picked = Some(idx);
        }
    }

    /// Row index under `position` (post-settle rows only).
    fn row_at(&self, position: Vec2, scale: f32) -> Option<usize> {
        if !self.bounds.contains(position) {
            return None;
        }
        let rh = self.row_h(scale);
        let center = self.bounds.min_y() + self.bounds.height() / 2.0;
        // The drum is continuous during drags; convert position to
        // the row under the pointer.
        let row = self.drag_row + (position.y - center) / rh;
        let i = row.round() as i64;
        (i >= 0 && (i as usize) < self.items.len()).then_some(i as usize)
    }
}

impl Default for WheelPicker {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for WheelPicker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WheelPicker")
            .field("items", &self.items.len())
            .field("selected", &self.selected)
            .finish()
    }
}

impl Widget for WheelPicker {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let h = cx.pt(self.row_height) * (VISIBLE_HALF * 2 + 1) as f32;
        Vec2::new(
            constraints
                .max_size
                .x
                .max(cx.pt(120.0).min(constraints.max_size.x)),
            h.min(constraints.max_size.y.max(h)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(80.0, 96.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        let rh = self.row_h(cx.scale);
        let center = bounds.min_y() + bounds.height() / 2.0;
        self.window_bounds = Some(Rect::new(
            bounds.min_x(),
            center - rh / 2.0,
            bounds.width(),
            rh,
        ));
    }

    fn paint(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        let f = |r: Rect| {
            kurbo::Rect::new(
                f64::from(r.min_x()),
                f64::from(r.min_y()),
                f64::from(r.max_x()),
                f64::from(r.max_y()),
            )
        };
        let surface = cx.color(TokenKey::SurfaceColor, [45, 45, 48, 255]);
        let fg = cx.color(TokenKey::TextColor, [220, 220, 220, 255]);
        let muted = cx.color(TokenKey::TextMutedColor, [130, 130, 130, 255]);
        let accent = cx.color(TokenKey::AccentColor, [0, 122, 204, 255]);
        let border = cx.color(TokenKey::BorderColor, [90, 90, 90, 255]);
        cx.list.push_fill_rect(f(b), surface);
        // Selection window.
        if let Some(w) = self.window_bounds {
            let wash = [accent[0], accent[1], accent[2], 36];
            cx.list.push_fill_rect(f(w), wash);
            cx.list
                .push_fill_rect(f(Rect::new(w.min_x(), w.min_y(), w.width(), 1.0)), border);
            cx.list
                .push_fill_rect(f(Rect::new(w.min_x(), w.max_y(), w.width(), 1.0)), border);
        }
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let rh = self.row_h(cx.scale);
        let center = b.min_y() + b.height() / 2.0;
        let clip = f(b);
        let size = 13.0 * cx.scale;
        for (i, item) in self.items.iter().enumerate() {
            // Row `i` sits `i - drag_row` rows below the window.
            let y = center + (i as f32 - self.drag_row) * rh - rh / 2.0;
            if y + rh < b.min_y() || y > b.max_y() {
                continue;
            }
            let dist = ((i as f32) - self.drag_row).abs();
            let ink = if !self.enabled {
                muted
            } else if dist < 0.5 {
                fg
            } else {
                muted
            };
            let w = painter
                .and_then(|p| p.measure_text(item, size))
                .unwrap_or(size * item.chars().count() as f32 * 0.5);
            let origin = kurbo::Point::new(
                f64::from(b.min_x() + (b.width() - w.min(b.width())) / 2.0),
                f64::from(y + (rh - size) / 2.0),
            );
            paint_label_clipped(painter, cx.list, clip, origin, item, size, ink);
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.enabled || self.items.is_empty() {
            return EventResponse::Ignored;
        }
        match cx.event {
            WidgetEvent::Scroll { position, delta } => {
                if !self.bounds.contains(*position) {
                    return EventResponse::Ignored;
                }
                let rh = self.row_h(cx.scale);
                let raw = self.drag_row + delta.y / rh;
                // Resist at the ends.
                let max = self.items.len() as f32 - 1.0;
                self.drag_row = if raw < 0.0 {
                    raw * RESIST
                } else if raw > max {
                    max + (raw - max) * RESIST
                } else {
                    raw
                };
                self.settle();
                EventResponse::Handled
            }
            WidgetEvent::PointerPressed {
                position, button, ..
            } => {
                if *button == martensite_core::PointerButton::Primary
                    && self.bounds.contains(*position)
                {
                    self.drag_y = Some(position.y);
                    self.drag_start_row = self.drag_row;
                    return EventResponse::CapturePointer;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerMoved { position } => {
                if let Some(start) = self.drag_y {
                    let rh = self.row_h(cx.scale);
                    let raw = self.drag_start_row - (position.y - start) / rh;
                    let max = self.items.len() as f32 - 1.0;
                    self.drag_row = if raw < 0.0 {
                        raw * RESIST
                    } else if raw > max {
                        max + (raw - max) * RESIST
                    } else {
                        raw
                    };
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerReleased { position, .. } => {
                if self.drag_y.take().is_some() {
                    // Tap (no motion) selects the tapped row directly.
                    if (self.drag_start_row - self.drag_row).abs() < 0.05 {
                        if let Some(row) = self.row_at(*position, cx.scale) {
                            self.drag_row = row as f32;
                        }
                    }
                    self.settle();
                    return EventResponse::ReleasePointer;
                }
                EventResponse::Ignored
            }
            WidgetEvent::KeyPressed { key, .. } => {
                let d = match key.as_str() {
                    "ArrowUp" => -1.0,
                    "ArrowDown" => 1.0,
                    "PageUp" => -3.0,
                    "PageDown" => 3.0,
                    _ => return EventResponse::Ignored,
                };
                self.drag_row = (self.drag_row + d).clamp(0.0, self.items.len() as f32 - 1.0);
                self.settle();
                EventResponse::Handled
            }
            _ => EventResponse::Ignored,
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::ListBox);
        if let Some(ref label) = self.label {
            node.set_label(label.as_str());
        }
        if !self.enabled {
            node.set_disabled();
        }
        if let Some(item) = self.selected_item() {
            node.set_value(item.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::{HotNode, PointerButton, WidgetEvent};

    fn picker() -> WheelPicker {
        WheelPicker::new().items(["S", "M", "L", "XL", "XXL"])
    }

    fn laid_out(w: &mut WheelPicker) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        w.layout(&mut cx, Rect::new(0.0, 0.0, 200.0, 160.0));
    }

    fn ev(w: &mut WheelPicker, event: &WidgetEvent) -> EventResponse {
        let mut cx = EventContext {
            event,
            bounds: Rect::new(0.0, 0.0, 200.0, 160.0),
            scale: 1.0,
        };
        w.event(&mut cx)
    }

    #[test]
    fn builder() {
        let w = picker();
        assert_eq!(w.item_count(), 5);
        assert_eq!(w.selected(), 0);
        assert_eq!(w.selected_item(), Some("S"));
    }

    #[test]
    fn wheel_scroll_snaps() {
        let mut w = picker();
        laid_out(&mut w);
        let scroll = WidgetEvent::Scroll {
            position: Vec2::new(100.0, 80.0),
            delta: Vec2::new(0.0, 32.0), // one row down
        };
        assert_eq!(ev(&mut w, &scroll), EventResponse::Handled);
        assert_eq!(w.selected(), 1);
        assert_eq!(w.take_selected(), Some(1));
    }

    #[test]
    fn drag_snaps_to_nearest() {
        let mut w = picker();
        laid_out(&mut w);
        let down = WidgetEvent::PointerPressed {
            position: Vec2::new(100.0, 80.0),
            button: PointerButton::Primary,
            count: 1,
        };
        let mv = WidgetEvent::PointerMoved {
            position: Vec2::new(100.0, 80.0 - 40.0), // drag up = scroll down 1.25 rows
        };
        let up = WidgetEvent::PointerReleased {
            position: Vec2::new(100.0, 40.0),
            button: PointerButton::Primary,
        };
        ev(&mut w, &down);
        ev(&mut w, &mv);
        ev(&mut w, &up);
        assert_eq!(w.selected(), 1);
        assert_eq!(w.take_selected(), Some(1));
    }

    #[test]
    fn drag_resists_at_ends() {
        let mut w = picker();
        laid_out(&mut w);
        let down = WidgetEvent::PointerPressed {
            position: Vec2::new(100.0, 80.0),
            button: PointerButton::Primary,
            count: 1,
        };
        let mv = WidgetEvent::PointerMoved {
            position: Vec2::new(100.0, 80.0 + 200.0), // way past top
        };
        ev(&mut w, &down);
        ev(&mut w, &mv);
        // Raw would be -200/32 = -6.25; resistance shrinks the stretch.
        assert!(w.drag_row < 0.0);
        assert!(w.drag_row > -6.25);
    }

    #[test]
    fn arrow_keys_step() {
        let mut w = picker();
        laid_out(&mut w);
        let down = WidgetEvent::KeyPressed {
            key: "ArrowDown".to_string(),
            repeat: false,
        };
        ev(&mut w, &down);
        ev(&mut w, &down);
        assert_eq!(w.selected(), 2);
        assert_eq!(w.take_selected(), Some(2));
    }

    #[test]
    fn tap_selects_row() {
        let mut w = picker();
        laid_out(&mut w);
        // Row 1 sits one row_height below center (80+32=112).
        let tap_down = WidgetEvent::PointerPressed {
            position: Vec2::new(100.0, 112.0),
            button: PointerButton::Primary,
            count: 1,
        };
        let tap_up = WidgetEvent::PointerReleased {
            position: Vec2::new(100.0, 112.0),
            button: PointerButton::Primary,
        };
        ev(&mut w, &tap_down);
        ev(&mut w, &tap_up);
        assert_eq!(w.selected(), 1);
    }

    #[test]
    fn scroll_clamps_at_last() {
        let mut w = picker();
        laid_out(&mut w);
        let scroll = WidgetEvent::Scroll {
            position: Vec2::new(100.0, 80.0),
            delta: Vec2::new(0.0, 400.0),
        };
        ev(&mut w, &scroll);
        assert_eq!(w.selected(), 4);
    }

    #[test]
    fn disabled_inert() {
        let mut w = picker().enabled(false);
        laid_out(&mut w);
        let scroll = WidgetEvent::Scroll {
            position: Vec2::new(100.0, 80.0),
            delta: Vec2::new(0.0, 32.0),
        };
        assert_eq!(ev(&mut w, &scroll), EventResponse::Ignored);
        assert_eq!(w.selected(), 0);
    }

    #[test]
    fn select_clamps() {
        let mut w = picker();
        w.select(99);
        assert_eq!(w.selected(), 4);
    }
}
