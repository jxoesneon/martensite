//! `QrCode` — QR barcode renderer.
//!
//! Renders a precomputed module matrix (dark/light cells) with the
//! standard quiet-zone margin — the Ant `QRCode` display side.
//! **Encoding is intentionally out of scope**: QR encoding
//! (Reed–Solomon + mask selection) lives in the app's domain — feed
//! `QrCode` a matrix from `qrcode`, `fast_qr`, or your own encoder
//! via [`QrCode::from_matrix`] / [`QrCode::from_bits`].
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::qr_code::QrCode;
//!
//! // A 21×21 version-1 matrix (all light modules here — real
//! // matrices come from an encoder).
//! let qr = QrCode::from_matrix(vec![vec![false; 21]; 21]);
//! assert_eq!(qr.module_count(), 21);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget,
};
use martensite_theme::TokenKey;

/// Module ink.
const DARK: TokenKey = TokenKey::TextColor;
/// Quiet-zone / light modules.
const LIGHT: TokenKey = TokenKey::BackgroundColor;
/// Quiet-zone width in modules (spec: 4).
const QUIET: f32 = 4.0;

/// A QR barcode renderer — see the module docs. Leaf widget, no
/// children, no interaction.
///
/// # Examples
///
/// ```
/// use martensite::widgets::qr_code::QrCode;
/// use martensite::core::Widget;
///
/// let mut qr = QrCode::from_matrix(vec![vec![true; 5]; 5]);
/// assert_eq!(qr.child_count(), 0);
/// ```
pub struct QrCode {
    /// Row-major modules: `modules[y][x] == true` paints dark.
    modules: Vec<Vec<bool>>,
    /// Side length in modules (square grid).
    size: usize,
    label: String,
    enabled: bool,
}

impl QrCode {
    /// Build from a square row-major boolean matrix (`true` = dark).
    /// Non-square input is cropped/padded to its shorter dimension.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::qr_code::QrCode;
    ///
    /// let qr = QrCode::from_matrix(vec![vec![true, false], vec![false, true]]);
    /// assert_eq!(qr.module_count(), 2);
    /// ```
    pub fn from_matrix(modules: Vec<Vec<bool>>) -> Self {
        let size = modules.len().min(
            modules
                .iter()
                .map(|r| r.len())
                .min()
                .unwrap_or(modules.len()),
        );
        Self {
            modules,
            size,
            label: "QR code".into(),
            enabled: true,
        }
    }

    /// Build from packed bits — `size` modules per side, `bits`
    /// row-major (`1` = dark). Missing bits read light.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::qr_code::QrCode;
    ///
    /// // 25×25 with only the four corners dark.
    /// let mut bits = vec![0u8; (25 * 25 + 7) / 8];
    /// let qr = QrCode::from_bits(&bits, 25);
    /// assert_eq!(qr.module_count(), 25);
    /// ```
    pub fn from_bits(bits: &[u8], size: usize) -> Self {
        let modules = (0..size)
            .map(|y| {
                (0..size)
                    .map(|x| {
                        let i = y * size + x;
                        bits.get(i / 8).is_some_and(|b| b & (0x80 >> (i % 8)) != 0)
                    })
                    .collect()
            })
            .collect();
        Self::from_matrix(modules)
    }

    /// Set the accessibility label (default `"QR code"`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::qr_code::QrCode;
    ///
    /// let qr = QrCode::from_matrix(vec![vec![false]]).label("Pairing code");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Enable or disable (dims the code; default `true`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::qr_code::QrCode;
    ///
    /// let qr = QrCode::from_matrix(vec![vec![true]]).enabled(false);
    /// ```
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// The module count per side.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::qr_code::QrCode;
    ///
    /// assert_eq!(QrCode::from_matrix(vec![vec![false; 21]; 21]).module_count(), 21);
    /// ```
    pub fn module_count(&self) -> usize {
        self.size
    }
}

impl Widget for QrCode {
    fn measure(&mut self, _cx: &mut LayoutContext, _constraints: LayoutConstraints) -> Vec2 {
        Vec2::splat(120.0)
    }

    fn layout(&mut self, _cx: &mut LayoutContext, _bounds: Rect) {}

    fn paint(&self, cx: &mut PaintContext) {
        if self.size == 0 {
            return;
        }
        let b = cx.bounds;
        let light = cx.color(LIGHT, [255, 255, 255, 255]);
        let dark = cx.color(DARK, [20, 20, 24, 255]);
        let muted = cx.color(TokenKey::TextMutedColor, [170, 170, 178, 255]);
        let ink = if self.enabled { dark } else { muted };

        // Quiet zone.
        cx.list.push_fill_rect(
            kurbo::Rect::new(
                f64::from(b.min_x()),
                f64::from(b.min_y()),
                f64::from(b.max_x()),
                f64::from(b.max_y()),
            ),
            light,
        );
        let cell = (b.width().min(b.height())) / (self.size as f32 + QUIET * 2.0);
        let ox =
            b.min_x() + (b.width() - cell * (self.size as f32 + QUIET * 2.0)) * 0.5 + cell * QUIET;
        let oy =
            b.min_y() + (b.height() - cell * (self.size as f32 + QUIET * 2.0)) * 0.5 + cell * QUIET;
        for (y, row) in self.modules.iter().take(self.size).enumerate() {
            for (x, dark_cell) in row.iter().take(self.size).enumerate() {
                if !*dark_cell {
                    continue;
                }
                cx.list.push_fill_rect(
                    kurbo::Rect::new(
                        f64::from(ox + x as f32 * cell),
                        f64::from(oy + y as f32 * cell),
                        f64::from(ox + (x + 1) as f32 * cell),
                        f64::from(oy + (y + 1) as f32 * cell),
                    ),
                    ink,
                );
            }
        }
    }

    fn event(&mut self, _cx: &mut EventContext) -> EventResponse {
        EventResponse::Ignored
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Image);
        node.set_label(self.label.as_str());
        if !self.enabled {
            node.set_disabled();
        }
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(60.0, 60.0)).with_policy(UnderflowPolicy::Lint)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_matrix_crops_to_square() {
        let qr = QrCode::from_matrix(vec![vec![true; 4]; 6]);
        assert_eq!(qr.module_count(), 4);
    }

    #[test]
    fn from_bits_reads_msb_first() {
        // First byte 0b1000_0000 → only module (0,0) dark.
        let qr = QrCode::from_bits(&[0b1000_0000], 3);
        assert!(qr.modules[0][0]);
        assert!(!qr.modules[0][1]);
        assert!(!qr.modules[1][1]);
    }

    #[test]
    fn from_bits_short_input_reads_light() {
        let qr = QrCode::from_bits(&[], 2);
        assert!(!qr.modules[1][1]);
    }

    #[test]
    fn empty_matrix_paints_nothing() {
        let qr = QrCode::from_matrix(vec![]);
        assert_eq!(qr.module_count(), 0);
    }
}
