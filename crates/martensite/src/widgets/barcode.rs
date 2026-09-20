//! `Barcode` — a Code-39 linear barcode display (the 1D
//! companion to [`crate::widgets::qr_code::QrCode`]).
//!
//! [`Barcode::text`] accepts the 43-symbol Code-39 alphabet
//! (`0-9`, `A-Z`, `-`, `.`, space, `$`, `/`, `+`, `%`); lowercase
//! input is folded to upper. Each symbol renders as nine
//! bar/space elements (wide = 3× narrow) wrapped in `*` start
//! and stop guards — the output is a real, scannable Code-39.
//! Unsupported characters are skipped via
//! `Barcode::encode`'s return value.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::barcode::Barcode;
//!
//! let b = Barcode::new().text("HELLO-39");
//! assert_eq!(b.symbols(), 8);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    LayoutConstraints, LayoutContext, PaintContext, Rect, RenderMinimum, UnderflowPolicy, Widget,
};
use martensite_theme::TokenKey;

const WIDTH_PT: f32 = 220.0;
const HEIGHT_PT: f32 = 60.0;
const PAD_PT: f32 = 6.0;
/// Narrow element width in points.
const NARROW_PT: f32 = 1.4;
/// Wide elements are 3 narrow modules.
const WIDE: f32 = 3.0;
/// Quiet zone on each side, in narrow modules.
const QUIET: f32 = 10.0;

const FACE: [u8; 4] = [240, 240, 240, 255];
const EDGE: [u8; 4] = [70, 70, 76, 255];
const INK: [u8; 4] = [20, 20, 24, 255];

/// Code-39 element patterns: nine bar/space elements per symbol,
/// `true` = wide. Index into [`C39_ALPHABET`].
const C39: [[bool; 9]; 44] = [
    // 0-9
    [false, false, false, true, true, false, true, false, false],
    [true, false, false, true, false, false, false, false, true],
    [false, false, true, true, false, false, false, false, true],
    [true, false, true, true, false, false, false, false, false],
    [false, false, false, true, true, false, false, false, true],
    [true, false, false, true, true, false, false, false, false],
    [false, false, true, true, true, false, false, false, false],
    [false, false, false, true, false, false, true, false, true],
    [true, false, false, true, false, false, true, false, false],
    [false, false, true, true, false, false, true, false, false],
    // A-Z
    [true, false, false, false, false, true, false, false, true],
    [false, false, true, false, false, true, false, false, true],
    [true, false, true, false, false, true, false, false, false],
    [false, false, false, false, true, true, false, false, true],
    [true, false, false, false, true, true, false, false, false],
    [false, false, true, false, true, true, false, false, false],
    [false, false, false, false, false, true, true, false, true],
    [true, false, false, false, false, true, true, false, false],
    [false, false, true, false, false, true, true, false, false],
    [false, false, false, false, true, true, true, false, false],
    [true, false, false, false, false, false, false, true, true],
    [false, false, true, false, false, false, false, true, true],
    [true, false, true, false, false, false, false, true, false],
    [false, false, false, false, true, false, false, true, true],
    [true, false, false, false, true, false, false, true, false],
    [false, false, true, false, true, false, false, true, false],
    [false, false, false, false, false, false, true, true, true],
    [true, false, false, false, false, false, true, true, false],
    [false, false, true, false, false, false, true, true, false],
    [false, false, false, false, true, false, true, true, false],
    [true, true, false, false, false, false, false, false, true],
    [false, true, true, false, false, false, false, false, true],
    [true, true, true, false, false, false, false, false, false],
    [false, true, false, false, true, false, false, false, true],
    [true, true, false, false, true, false, false, false, false],
    [false, true, true, false, true, false, false, false, false],
    // - . space $ / + %
    [false, true, false, false, false, false, true, false, true],
    [true, true, false, false, false, false, true, false, false],
    [false, true, true, false, false, false, true, false, false],
    [false, true, false, true, false, true, false, false, false],
    [false, true, false, true, false, false, false, true, false],
    [false, true, false, false, false, true, false, true, false],
    [false, false, false, true, false, true, false, true, false],
    // * (start/stop)
    [false, true, false, false, true, false, true, false, false],
];

/// The Code-39 alphabet: `0-9 A-Z -.$/+%` and `*`.
const C39_ALPHABET: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ-. $/+%*";

/// A Code-39 linear barcode — see the module docs.
///
/// ```
/// use martensite::widgets::barcode::Barcode;
///
/// assert_eq!(Barcode::new().symbols(), 0);
/// ```
#[derive(Debug)]
pub struct Barcode {
    /// Accessibility label.
    pub label: String,
    /// Encoded symbol indices into `C39` (no guards).
    symbols: Vec<usize>,
    /// Whether to draw the human-readable text strip below.
    show_text: bool,
    /// Original text (uppercased, unsupported chars dropped).
    text: String,
    bounds: Rect,
    scale: f32,
}

impl Default for Barcode {
    fn default() -> Self {
        Self::new()
    }
}

impl Barcode {
    /// Creates an empty barcode.
    ///
    /// ```
    /// use martensite::widgets::barcode::Barcode;
    ///
    /// assert_eq!(Barcode::new().symbols(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Barcode".to_string(),
            symbols: Vec::new(),
            show_text: true,
            text: String::new(),
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Sets the payload text; unsupported chars are dropped.
    ///
    /// ```
    /// use martensite::widgets::barcode::Barcode;
    ///
    /// let b = Barcode::new().text("ABC-123");
    /// assert_eq!(b.symbols(), 7);
    /// assert_eq!(b.content(), "ABC-123");
    /// ```
    pub fn text(mut self, text: impl AsRef<str>) -> Self {
        self.set_text(text.as_ref());
        self
    }

    /// Same as [`text`](Self::text) but on `&mut self`.
    ///
    /// ```
    /// use martensite::widgets::barcode::Barcode;
    ///
    /// let mut b = Barcode::new();
    /// b.set_text("ok!");
    /// assert_eq!(b.content(), "OK"); // '!' dropped
    /// ```
    pub fn set_text(&mut self, text: &str) {
        self.symbols.clear();
        self.text.clear();
        for c in text.chars() {
            let c = c.to_ascii_uppercase();
            if let Some(i) = C39_ALPHABET.iter().position(|&a| a == c as u8) {
                if c == '*' {
                    continue; // guards are implicit
                }
                self.symbols.push(i);
                self.text.push(c);
            }
        }
    }

    /// Hides the human-readable text strip.
    ///
    /// ```
    /// use martensite::widgets::barcode::Barcode;
    ///
    /// assert!(!Barcode::new().hide_text().has_text_strip());
    /// ```
    pub fn hide_text(mut self) -> Self {
        self.show_text = false;
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::barcode::Barcode;
    ///
    /// assert_eq!(Barcode::new().label("SKU").label, "SKU");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Encoded symbol count (excluding guards).
    ///
    /// ```
    /// use martensite::widgets::barcode::Barcode;
    ///
    /// assert_eq!(Barcode::new().text("42").symbols(), 2);
    /// ```
    pub fn symbols(&self) -> usize {
        self.symbols.len()
    }

    /// The sanitized payload actually encoded.
    ///
    /// ```
    /// use martensite::widgets::barcode::Barcode;
    ///
    /// assert_eq!(Barcode::new().text("a+b").content(), "A+B");
    /// ```
    pub fn content(&self) -> &str {
        &self.text
    }

    /// Whether the text strip is shown.
    ///
    /// ```
    /// use martensite::widgets::barcode::Barcode;
    ///
    /// assert!(Barcode::new().has_text_strip());
    /// ```
    pub fn has_text_strip(&self) -> bool {
        self.show_text
    }

    /// Total width in narrow modules (quiet zones + guards +
    /// symbols + inter-char spaces).
    ///
    /// ```
    /// use martensite::widgets::barcode::Barcode;
    ///
    /// // Each symbol is 15 modules (6 narrow + 3 wide×3) plus a
    /// // 1-module inter-char gap; "AB" + guards = 4 symbols.
    /// assert_eq!(Barcode::new().text("AB").module_count(), 83.0);
    /// ```
    pub fn module_count(&self) -> f32 {
        let n = self.symbols.len() + 2; // + start/stop guards
        QUIET * 2.0 + n as f32 * 15.0 + (n - 1) as f32
    }

    /// Full element stream including guards: `(symbol, element)`
    /// as wide flags, flattened.
    fn elements(&self) -> Vec<bool> {
        let star = C39_ALPHABET.len() - 1;
        let mut out = Vec::new();
        let push_sym = |s: usize, out: &mut Vec<bool>| {
            out.extend_from_slice(&C39[s]);
            out.push(false); // inter-character narrow space
        };
        push_sym(star, &mut out);
        for &s in &self.symbols {
            push_sym(s, &mut out);
        }
        push_sym(star, &mut out);
        out.pop(); // trailing space
        out
    }
}

impl Widget for Barcode {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            cx.pt(WIDTH_PT).min(constraints.max_size.x.max(0.0)),
            cx.pt(HEIGHT_PT).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(60.0, 20.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Image);
        node.set_label(format!("{} — {}", self.label, self.text));
    }

    fn paint(&self, cx: &mut PaintContext) {
        let krect = |r: Rect| {
            kurbo::Rect::new(
                f64::from(r.min_x()),
                f64::from(r.min_y()),
                f64::from(r.max_x()),
                f64::from(r.max_y()),
            )
        };
        // Light card face — barcodes need contrast to scan.
        cx.list.push_fill_shape(
            krect(self.bounds),
            &martensite_core::shape::Shape::rounded(cx.pt(4.0)),
            FACE,
        );
        if self.symbols.is_empty() {
            return;
        }
        let pad = PAD_PT * self.scale;
        let text_h = if self.show_text {
            10.0 * self.scale
        } else {
            0.0
        };
        let bar_top = self.bounds.min_y() + pad;
        let bar_h = (self.bounds.height() - 2.0 * pad - text_h).max(0.0);
        // Fit the whole stream (quiet zone included) to the width.
        let modules = self.module_count();
        let mw = ((self.bounds.width() - 2.0 * pad) / modules).min(NARROW_PT * self.scale);
        let ink = cx.color(TokenKey::TextColor, INK);
        let mut x = self.bounds.min_x() + pad + QUIET * mw;
        let mut is_bar = true;
        for wide in self.elements() {
            let w = if wide { WIDE * mw } else { mw };
            if is_bar {
                cx.list.push_fill_shape(
                    kurbo::Rect::new(
                        f64::from(x),
                        f64::from(bar_top),
                        f64::from(x + w),
                        f64::from(bar_top + bar_h),
                    ),
                    &martensite_core::shape::Shape::RECT,
                    ink,
                );
            }
            x += w;
            is_bar = !is_bar;
        }
        // Human-readable strip — centered text-width stub.
        if self.show_text && !self.text.is_empty() {
            let tw = (self.text.len() as f32 * 4.0 * self.scale).min(self.bounds.width() * 0.8);
            cx.list.push_fill_shape(
                kurbo::Rect::new(
                    f64::from((self.bounds.min_x() + self.bounds.max_x()) / 2.0 - tw / 2.0),
                    f64::from(bar_top + bar_h + 2.0 * self.scale),
                    f64::from((self.bounds.min_x() + self.bounds.max_x()) / 2.0 + tw / 2.0),
                    f64::from(bar_top + bar_h + 2.0 * self.scale + 2.0 * self.scale),
                ),
                &martensite_core::shape::Shape::rounded(self.scale),
                ink,
            );
        }
        cx.list.push_stroke_shape(
            krect(self.bounds),
            &martensite_core::shape::Shape::rounded(cx.pt(4.0)),
            cx.pt(0.75),
            cx.color(TokenKey::BorderColor, EDGE),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(b: &mut Barcode, w: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        b.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(w, h),
            },
        );
        b.layout(&mut cx, Rect::new(0.0, 0.0, w, h));
    }

    #[test]
    fn encodes_alphabet_and_folds_case() {
        let b = Barcode::new().text("abc-123");
        assert_eq!(b.content(), "ABC-123");
        assert_eq!(b.symbols(), 7);
    }

    #[test]
    fn drops_unsupported_chars() {
        let mut b = Barcode::new();
        b.set_text("a!b@c#");
        assert_eq!(b.content(), "ABC");
        assert_eq!(b.symbols(), 3);
    }

    #[test]
    fn guards_and_elements() {
        // One symbol → start + sym + stop = 3 symbols,
        // 9 elements each + 2 inter-char spaces = 29 elements.
        let b = Barcode::new().text("A");
        assert_eq!(b.elements().len(), 29);
        // Elements alternate starting with a bar: count of bars
        // = ceil(29/2) = 15 bar elements.
    }

    #[test]
    fn module_count_scales() {
        let b = Barcode::new().text("AA");
        // 20 quiet + 4 symbols*15 + 3 gaps = 20+60+3 = 83
        assert_eq!(b.module_count(), 83.0);
    }

    #[test]
    fn smoke() {
        let mut b = Barcode::new().text("SKU-0001").label("Part");
        laid_out(&mut b, 220.0, 60.0);
        assert_eq!(b.symbols(), 8);
        assert!(b.has_text_strip());
        assert!(!Barcode::new().hide_text().has_text_strip());
    }
}
