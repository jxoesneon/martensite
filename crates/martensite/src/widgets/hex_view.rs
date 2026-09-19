//! `HexView` — a hex-dump display (binary-inspection / dev-tool
//! idiom).
//!
//! Bytes render as rows of `offset  hh hh … hh  |ascii|`: a
//! muted offset column, 16 hex pairs per row (8+8 with a gap),
//! and a printable-ASCII gutter. Clicking a byte cell parks its
//! index in [`HexView::take_selected`]; mouse-wheel scrolls.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::hex_view::HexView;
//!
//! let h = HexView::new().bytes(vec![0x48, 0x69]);
//! assert_eq!(h.byte_count(), 2);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::text_paint::{paint_label_clipped, SharedTextPainter};

const WIDTH_PT: f32 = 460.0;
const HEIGHT_PT: f32 = 200.0;
const FONT_PT: f32 = 12.0;
const LINE_H: f32 = 1.5;
const PAD_PT: f32 = 8.0;
const PER_ROW: usize = 16;

const FACE: [u8; 4] = [36, 36, 42, 255];
const EDGE: [u8; 4] = [70, 70, 76, 255];
const HEX: [u8; 4] = [210, 210, 218, 255];
const ASCII: [u8; 4] = [140, 190, 230, 255];
const OFFSET: [u8; 4] = [130, 130, 140, 255];
const HOT: [u8; 4] = [80, 140, 220, 60];

/// A hex-dump byte display — see the module docs.
///
/// ```
/// use martensite::widgets::hex_view::HexView;
///
/// assert_eq!(HexView::new().byte_count(), 0);
/// ```
pub struct HexView {
    /// Accessibility label.
    pub label: String,
    /// Currently selected byte index, if any.
    pub selected: Option<usize>,
    bytes: Vec<u8>,
    scroll_back: f32,
    pending: Option<usize>,
    painter: Option<SharedTextPainter>,
    bounds: Rect,
    scale: f32,
}

impl Default for HexView {
    fn default() -> Self {
        Self::new()
    }
}

impl HexView {
    /// Creates an empty view.
    ///
    /// ```
    /// use martensite::widgets::hex_view::HexView;
    ///
    /// assert_eq!(HexView::new().byte_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Hex".to_string(),
            selected: None,
            bytes: Vec::new(),
            scroll_back: 0.0,
            pending: None,
            painter: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Replaces the byte payload.
    ///
    /// ```
    /// use martensite::widgets::hex_view::HexView;
    ///
    /// let h = HexView::new().bytes(vec![0u8; 32]);
    /// assert_eq!(h.byte_count(), 32);
    /// ```
    pub fn bytes(mut self, bytes: impl Into<Vec<u8>>) -> Self {
        self.bytes = bytes.into();
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::hex_view::HexView;
    ///
    /// assert_eq!(HexView::new().label("firmware").label, "firmware");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Shared text painter for glyph-accurate layout.
    ///
    /// ```
    /// use martensite::widgets::hex_view::HexView;
    /// use martensite::text_paint::shared_painter;
    ///
    /// let h = HexView::new().with_text_painter(shared_painter());
    /// assert_eq!(h.byte_count(), 0);
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.painter = Some(painter);
        self
    }

    /// Byte count.
    ///
    /// ```
    /// use martensite::widgets::hex_view::HexView;
    ///
    /// assert_eq!(HexView::new().bytes(vec![1, 2, 3]).byte_count(), 3);
    /// ```
    pub fn byte_count(&self) -> usize {
        self.bytes.len()
    }

    /// Row count (`16` bytes per row).
    ///
    /// ```
    /// use martensite::widgets::hex_view::HexView;
    ///
    /// assert_eq!(HexView::new().bytes(vec![0; 17]).row_count(), 2);
    /// ```
    pub fn row_count(&self) -> usize {
        self.bytes.len().div_ceil(PER_ROW)
    }

    /// The raw bytes.
    ///
    /// ```
    /// use martensite::widgets::hex_view::HexView;
    ///
    /// assert_eq!(HexView::new().bytes(vec![9, 8]).byte_list(), &[9, 8]);
    /// ```
    pub fn byte_list(&self) -> &[u8] {
        &self.bytes
    }

    /// Drains the last clicked byte index.
    ///
    /// ```
    /// use martensite::widgets::hex_view::HexView;
    ///
    /// let mut h = HexView::new();
    /// assert!(h.take_selected().is_none());
    /// ```
    pub fn take_selected(&mut self) -> Option<usize> {
        self.pending.take()
    }

    /// Rows scrolled back from the bottom.
    ///
    /// ```
    /// use martensite::widgets::hex_view::HexView;
    ///
    /// assert_eq!(HexView::new().scroll_offset(), 0.0);
    /// ```
    pub fn scroll_offset(&self) -> f32 {
        self.scroll_back
    }

    /// One byte as ASCII or `·`.
    fn ascii_of(b: u8) -> char {
        if b.is_ascii_graphic() || b == b' ' {
            b as char
        } else {
            '·'
        }
    }

    fn line_h(&self) -> f32 {
        FONT_PT * LINE_H * self.scale
    }

    fn visible(&self) -> usize {
        (self.bounds.height() / self.line_h().max(1.0)) as usize
    }

    fn first_visible(&self) -> usize {
        self.row_count()
            .saturating_sub(self.scroll_back.floor() as usize)
            .saturating_sub(self.visible())
    }

    /// Byte index under a point (hex columns only).
    fn byte_at(&self, p: Vec2) -> Option<usize> {
        if !self.bounds.contains(p) {
            return None;
        }
        let char_w = FONT_PT * 0.62 * self.scale;
        let hex_x = self.bounds.min_x() + PAD_PT * self.scale + char_w * 9.0;
        let row = ((p.y - self.bounds.min_y()) / self.line_h()) as usize;
        let line = self.first_visible() + row;
        // Column: pairs are "hh " → 3 chars, gap after byte 7.
        let rel = p.x - hex_x;
        if rel < 0.0 {
            return None;
        }
        let col = (rel / (char_w * 3.0)) as usize;
        // The extra gap after column 7 shifts columns ≥8 by ~1 char.
        let col = if rel >= char_w * 3.0 * 8.0 + char_w {
            ((rel - char_w) / (char_w * 3.0)) as usize
        } else {
            col
        };
        let idx = line * PER_ROW + col.min(PER_ROW - 1);
        (col < PER_ROW && idx < self.bytes.len()).then_some(idx)
    }
}

impl Widget for HexView {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            cx.pt(WIDTH_PT).min(constraints.max_size.x.max(0.0)),
            cx.pt(HEIGHT_PT).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(200.0, 40.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Code);
        node.set_label(format!("{} — {} bytes", self.label, self.bytes.len()));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::Scroll { delta, .. } => {
                let max = self.row_count().saturating_sub(self.visible()) as f32;
                let next = (self.scroll_back - delta.y / self.line_h()).clamp(0.0, max);
                if (next - self.scroll_back).abs() > f32::EPSILON {
                    self.scroll_back = next;
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                ..
            } => {
                if let Some(i) = self.byte_at(*position) {
                    self.selected = Some(i);
                    self.pending = Some(i);
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let f = |r: Rect| {
            kurbo::Rect::new(
                f64::from(r.min_x()),
                f64::from(r.min_y()),
                f64::from(r.max_x()),
                f64::from(r.max_y()),
            )
        };
        cx.list.push_fill_shape(
            f(self.bounds),
            &martensite_core::shape::Shape::rounded(cx.pt(4.0)),
            cx.color(TokenKey::SurfaceColor, FACE),
        );
        let line_h = self.line_h();
        let char_w = FONT_PT * 0.62 * self.scale;
        let start = self.first_visible();
        let end = (start + self.visible() + 1).min(self.row_count());
        let painter = crate::text_paint::resolve_painter(&self.painter, cx.text_painter);
        let size = FONT_PT * cx.scale;
        let pad = PAD_PT * self.scale;

        let offset_c = cx.color(TokenKey::TextMutedColor, OFFSET);
        let hex_c = cx.color(TokenKey::TextColor, HEX);
        let ascii_c = cx.color(TokenKey::AccentColor, ASCII);

        for (row, line) in (start..end).enumerate() {
            let y = self.bounds.min_y() + row as f32 * line_h;
            let ty = y + (line_h - size) * 0.5;
            let base = line * PER_ROW;
            let chunk: Vec<u8> = self.bytes[base..(base + PER_ROW).min(self.bytes.len())].to_vec();

            // Selected byte wash.
            if let Some(sel) = self.selected {
                if sel / PER_ROW == line {
                    cx.list.push_fill_rect(
                        f(Rect::new(
                            self.bounds.min_x(),
                            y,
                            self.bounds.width(),
                            line_h,
                        )),
                        cx.color(TokenKey::AccentColor, HOT),
                    );
                }
            }
            // Offset.
            paint_label_clipped(
                painter,
                cx.list,
                f(self.bounds),
                kurbo::Point::new(f64::from(self.bounds.min_x() + pad), f64::from(ty)),
                &format!("{:08x}", base),
                size,
                offset_c,
            );
            // Hex pairs, gap after byte 7.
            let mut hex = String::new();
            for (i, b) in chunk.iter().enumerate() {
                if i == 8 {
                    hex.push(' ');
                }
                hex.push_str(&format!("{b:02x} "));
            }
            let hex_x = self.bounds.min_x() + pad + char_w * 9.0;
            paint_label_clipped(
                painter,
                cx.list,
                f(self.bounds),
                kurbo::Point::new(f64::from(hex_x), f64::from(ty)),
                hex.trim_end(),
                size,
                hex_c,
            );
            // ASCII gutter.
            let ascii: String = chunk.iter().map(|&b| Self::ascii_of(b)).collect();
            paint_label_clipped(
                painter,
                cx.list,
                f(self.bounds),
                kurbo::Point::new(
                    f64::from(hex_x + char_w * (3.0 * PER_ROW as f32 + 1.0)),
                    f64::from(ty),
                ),
                &ascii,
                size,
                ascii_c,
            );
        }
        cx.list.push_stroke_shape(
            f(self.bounds),
            &martensite_core::shape::Shape::rounded(cx.pt(4.0)),
            cx.pt(0.75),
            cx.color(TokenKey::BorderColor, EDGE),
        );
    }
}

impl std::fmt::Debug for HexView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HexView")
            .field("bytes", &self.bytes.len())
            .field("selected", &self.selected)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(h: &mut HexView, w: f32, ht: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        h.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(w, ht),
            },
        );
        h.layout(&mut cx, Rect::new(0.0, 0.0, w, ht));
    }

    fn ev(h: &mut HexView, e: WidgetEvent) {
        h.event(&mut EventContext {
            event: &e,
            bounds: Rect::new(0.0, 0.0, 460.0, 200.0),
            scale: 1.0,
        });
    }

    #[test]
    fn rows_round_up() {
        assert_eq!(HexView::new().bytes(vec![0; 16]).row_count(), 1);
        assert_eq!(HexView::new().bytes(vec![0; 17]).row_count(), 2);
        assert_eq!(HexView::new().bytes(vec![0; 33]).row_count(), 3);
    }

    #[test]
    fn ascii_map() {
        assert_eq!(HexView::ascii_of(b'A'), 'A');
        assert_eq!(HexView::ascii_of(0), '·');
        assert_eq!(HexView::ascii_of(b' '), ' ');
    }

    #[test]
    fn click_selects_byte() {
        let mut h = HexView::new().bytes(vec![0u8; 32]);
        laid_out(&mut h, 460.0, 200.0);
        // First row, first hex column: x = pad + 9*char_w.
        let char_w = FONT_PT * 0.62;
        let x = 8.0 + char_w * 9.0 + 1.0;
        ev(
            &mut h,
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(x, 6.0),
                count: 1,
            },
        );
        assert_eq!(h.take_selected(), Some(0));
    }

    #[test]
    fn wheel_scrolls() {
        let mut h = HexView::new().bytes(vec![0u8; 800]); // 50 rows
        laid_out(&mut h, 460.0, 60.0);
        ev(
            &mut h,
            WidgetEvent::Scroll {
                delta: Vec2::new(0.0, -40.0),
                position: Vec2::new(100.0, 30.0),
            },
        );
        assert!(h.scroll_offset() > 0.0);
    }
}
