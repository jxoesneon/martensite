//! `LcdNumber` — seven-segment digit display (Qt `QLCDNumber`).
//!
//! Renders an integer or decimal string in the classic segment style —
//! each glyph is seven stroked bars. Display-only; pair with a
//! `Slider`/`SpinBox` for the retro instrument look.
//!
//! Segments are indexed `a` (top) through `g` (middle) in the
//! canonical arrangement:
//!
//! ```text
//!    -- a --
//!   |       |
//!   f       b
//!   |-- g --|
//!   e       c
//!   |       |
//!    -- d --
//! ```
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::lcd_number::LcdNumber;
//!
//! let lcd = LcdNumber::new().value(42.5).digits(5);
//! assert_eq!(lcd.value, 42.5);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget,
};
use martensite_theme::TokenKey;

/// Segment bit order: a=1, b=2, c=4, d=8, e=16, f=32, g=64.
const GLYPHS: [u8; 10] = [
    0b011_1111, // 0: a b c d e f
    0b000_0110, // 1: b c
    0b101_1011, // 2: a b d e g
    0b100_1111, // 3: a b c d g
    0b110_0110, // 4: b c f g
    0b110_1101, // 5: a c d f g
    0b111_1101, // 6: a c d e f g
    0b000_0111, // 7: a b c
    0b111_1111, // 8: all
    0b110_1111, // 9: a b c d f g
];

const DIGIT_W_PT: f32 = 14.0;
const DIGIT_H_PT: f32 = 24.0;
const GAP_PT: f32 = 3.0;
const PAD_PT: f32 = 4.0;
const BAR_PT: f32 = 2.2;
const ON: [u8; 4] = [20, 20, 24, 255];
const OFF: [u8; 4] = [20, 20, 24, 28];
const FACE: [u8; 4] = [230, 234, 220, 255];

/// Seven-segment display — see the module docs.
///
/// ```
/// use martensite::widgets::lcd_number::LcdNumber;
///
/// let lcd = LcdNumber::new();
/// assert_eq!(lcd.digits, 4);
/// ```
pub struct LcdNumber {
    /// Displayed value; formatted per [`LcdNumber::decimals`].
    pub value: f64,
    /// Fixed glyph cells — the value is right-aligned and
    /// zero-free-padded on overflow (Qt `numDigits` semantics).
    pub digits: usize,
    /// Decimal places shown for `value`.
    pub decimals: usize,
    /// When `false` the display is dimmed.
    pub enabled: bool,
    bounds: Rect,
    scale: f32,
}

impl LcdNumber {
    /// Creates a 4-digit display.
    ///
    /// ```
    /// use martensite::widgets::lcd_number::LcdNumber;
    ///
    /// let lcd = LcdNumber::new();
    /// assert_eq!(lcd.value, 0.0);
    /// ```
    pub fn new() -> Self {
        Self {
            value: 0.0,
            digits: 4,
            decimals: 0,
            enabled: true,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Sets the displayed value.
    ///
    /// ```
    /// use martensite::widgets::lcd_number::LcdNumber;
    ///
    /// let lcd = LcdNumber::new().value(3.14).decimals(2);
    /// assert_eq!(lcd.value, 3.14);
    /// ```
    pub fn value(mut self, value: f64) -> Self {
        self.value = value;
        self
    }

    /// Sets the glyph cell count.
    ///
    /// ```
    /// use martensite::widgets::lcd_number::LcdNumber;
    ///
    /// let lcd = LcdNumber::new().digits(8);
    /// assert_eq!(lcd.digits, 8);
    /// ```
    pub fn digits(mut self, n: usize) -> Self {
        self.digits = n.max(1);
        self
    }

    /// Sets the decimal places shown.
    ///
    /// ```
    /// use martensite::widgets::lcd_number::LcdNumber;
    ///
    /// let lcd = LcdNumber::new().decimals(1);
    /// assert_eq!(lcd.decimals, 1);
    /// ```
    pub fn decimals(mut self, n: usize) -> Self {
        self.decimals = n;
        self
    }

    /// Enables or disables the display (dims the glass).
    ///
    /// ```
    /// use martensite::widgets::lcd_number::LcdNumber;
    ///
    /// let lcd = LcdNumber::new().enabled(false);
    /// assert!(!lcd.enabled);
    /// ```
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// The text shown — `value` formatted to `decimals`, truncated on
    /// the left when it overflows `digits` cells.
    fn display_text(&self) -> String {
        let text = format!("{:.*}", self.decimals, self.value);
        let cells = text.chars().count();
        if cells > self.digits {
            // Right-align: keep the low-order digits (Qt overflow
            // semantics show the tail, not an error).
            text.chars().skip(cells - self.digits).collect()
        } else {
            " ".repeat(self.digits - cells) + &text
        }
    }

    /// Paints one seven-segment glyph centered in `cell` — `mask`
    /// selects the lit bars.
    fn paint_glyph(&self, cx: &mut PaintContext, cell: kurbo::Rect, mask: u8, off_alpha: u8) {
        let t = BAR_PT * self.scale;
        let w = cell.width() as f32;
        let h = cell.height() as f32;
        let x0 = cell.x0 as f32;
        let y0 = cell.y0 as f32;
        let ym = y0 + h / 2.0; // middle line
        let half_w = w - t; // bar body length
                            // Segment rects: (x, y, w, h) in device px.
        let segs = [
            // a: top horizontal
            (x0 + t / 2.0, y0, half_w, t),
            // b: upper right vertical
            (x0 + w - t, y0 + t / 2.0, t, h / 2.0 - t),
            // c: lower right vertical
            (x0 + w - t, ym + t / 2.0, t, h / 2.0 - t),
            // d: bottom horizontal
            (x0 + t / 2.0, y0 + h - t, half_w, t),
            // e: lower left vertical
            (x0, ym + t / 2.0, t, h / 2.0 - t),
            // f: upper left vertical
            (x0, y0 + t / 2.0, t, h / 2.0 - t),
            // g: middle horizontal
            (x0 + t / 2.0, ym - t / 2.0, half_w, t),
        ];
        for (i, (sx, sy, sw, sh)) in segs.iter().enumerate() {
            let lit = mask & (1 << i) != 0;
            let color = if lit {
                let mut c = cx.color(TokenKey::TextColor, ON);
                c[3] = if self.enabled { 255 } else { 120 };
                c
            } else {
                let mut c = cx.color(TokenKey::TextMutedColor, OFF);
                c[3] = off_alpha;
                c
            };
            cx.list.push_fill_rect(
                kurbo::Rect::new(
                    f64::from(*sx),
                    f64::from(*sy),
                    f64::from(*sx + *sw),
                    f64::from(*sy + *sh),
                ),
                color,
            );
        }
    }
}

impl Default for LcdNumber {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for LcdNumber {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let w = self.digits as f32 * DIGIT_W_PT + (self.digits - 1) as f32 * GAP_PT + PAD_PT * 2.0;
        Vec2::new(
            cx.pt(w).min(constraints.max_size.x.max(0.0)),
            cx.pt(DIGIT_H_PT + PAD_PT * 2.0)
                .min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(30.0, 20.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Label);
        node.set_label("Display");
        node.set_value(self.display_text().trim());
        if !self.enabled {
            node.set_disabled();
        }
    }

    fn event(&mut self, _cx: &mut EventContext) -> EventResponse {
        EventResponse::Ignored
    }

    fn paint(&self, cx: &mut PaintContext) {
        let r = kurbo::Rect::new(
            f64::from(self.bounds.min_x()),
            f64::from(self.bounds.min_y()),
            f64::from(self.bounds.max_x()),
            f64::from(self.bounds.max_y()),
        );
        cx.list
            .push_fill_rect(r, cx.color(TokenKey::SurfaceColor, FACE));

        let pad = PAD_PT * self.scale;
        let gap = GAP_PT * self.scale;
        let text = self.display_text();
        let chars: Vec<char> = text.chars().collect();
        let n = chars.len().max(1);
        let cell_w = ((self.bounds.size.x - pad * 2.0 - gap * (n - 1) as f32) / n as f32).max(0.0);
        let cell_h = (self.bounds.size.y - pad * 2.0).max(0.0);

        for (i, ch) in chars.iter().enumerate() {
            let x = self.bounds.origin.x + pad + i as f32 * (cell_w + gap);
            // A '.' cell is narrow — attach the dot to the next glyph's
            // pitch anyway for simplicity (classic LCDs do the same).
            let cell = kurbo::Rect::new(
                f64::from(x),
                f64::from(self.bounds.origin.y + pad),
                f64::from(x + cell_w),
                f64::from(self.bounds.origin.y + pad + cell_h),
            );
            match ch {
                '0'..='9' => {
                    let mask = GLYPHS[*ch as usize - '0' as usize];
                    self.paint_glyph(cx, cell, mask, 28);
                }
                '.' | ',' => {
                    // Dot: small lit square near the baseline.
                    let d = BAR_PT * self.scale;
                    let mut c = cx.color(TokenKey::TextColor, ON);
                    c[3] = if self.enabled { 255 } else { 120 };
                    cx.list.push_fill_rect(
                        kurbo::Rect::new(
                            cell.x0 + cell.width() / 2.0 - f64::from(d) / 2.0,
                            cell.y1 - f64::from(d) - 1.0,
                            cell.x0 + cell.width() / 2.0 + f64::from(d) / 2.0,
                            cell.y1 - 1.0,
                        ),
                        c,
                    );
                }
                '-' => {
                    self.paint_glyph(cx, cell, 0b100_0000, 0); // g only
                }
                _ => {
                    // Blank cell — ghost segments still paint (real
                    // LCD glass shows the unlit bars).
                    self.paint_glyph(cx, cell, 0, 14);
                }
            }
        }
    }
}

impl std::fmt::Debug for LcdNumber {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LcdNumber")
            .field("value", &self.value)
            .field("digits", &self.digits)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    #[test]
    fn display_text_right_aligns_and_pads() {
        let lcd = LcdNumber::new().value(42.0).digits(5);
        assert_eq!(lcd.display_text(), "   42");
    }

    #[test]
    fn display_text_truncates_overflow_tail() {
        let lcd = LcdNumber::new().value(12345.0).digits(3);
        assert_eq!(lcd.display_text(), "345");
    }

    #[test]
    fn decimals_include_point() {
        let lcd = LcdNumber::new().value(3.5).decimals(1).digits(4);
        assert_eq!(lcd.display_text(), " 3.5");
    }

    #[test]
    fn negative_shows_minus() {
        let lcd = LcdNumber::new().value(-7.0).digits(3);
        assert_eq!(lcd.display_text(), " -7");
    }

    #[test]
    fn glyph_masks_are_canonical() {
        // 8 lights every segment; 0 lights all but g; 1 is b|c.
        assert_eq!(GLYPHS[8], 0b111_1111);
        assert_eq!(GLYPHS[0] & 0b100_0000, 0);
        assert_eq!(GLYPHS[1], 0b000_0110);
    }

    #[test]
    fn measure_scales_with_digits() {
        let mut a = LcdNumber::new().digits(2);
        let mut b = LcdNumber::new().digits(8);
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        let cons = LayoutConstraints {
            min_size: Vec2::ZERO,
            max_size: Vec2::new(1000.0, 1000.0),
        };
        let sa = a.measure(&mut cx, cons);
        let sb = b.measure(&mut cx, cons);
        assert!(sb.x > sa.x);
    }
}
