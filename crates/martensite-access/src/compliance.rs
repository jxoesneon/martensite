//! Automated WCAG 2.2 AA/AAA accessibility evaluation and Section 508 VPAT verification.
//!
//! This module provides:
//! - Color representations and WCAG 2.2 relative luminance / contrast calculation ([`ColorRgba`], [`relative_luminance`], [`contrast_ratio`]).
//! - WCAG 2.2 text and UI component contrast checks ([`check_text_contrast`], [`check_ui_component_contrast`]).
//! - WCAG 2.2 Target Size (Criterion 2.5.8) minimum dimensions verification ([`check_target_size`]).
//! - Focus appearance compliance checking ([`FocusAppearanceCheck`]).
//! - Section 508 VPAT accessibility report generation ([`Section508VpatReport`]).

/// RGBA color representation using 32-bit floating-point channels in the range `[0.0, 1.0]`.
///
/// # Examples
///
/// ```
/// use martensite_access::compliance::ColorRgba;
///
/// let white = ColorRgba::new(1.0, 1.0, 1.0, 1.0);
/// assert_eq!(white.r, 1.0);
/// assert_eq!(white.a, 1.0);
///
/// let black = ColorRgba::from_rgb_u8(0, 0, 0);
/// assert_eq!(black.r, 0.0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ColorRgba {
    /// Red channel component `[0.0, 1.0]`.
    pub r: f32,
    /// Green channel component `[0.0, 1.0]`.
    pub g: f32,
    /// Blue channel component `[0.0, 1.0]`.
    pub b: f32,
    /// Alpha channel opacity `[0.0, 1.0]`.
    pub a: f32,
}

impl ColorRgba {
    /// Creates a new `ColorRgba` from `(r, g, b, a)` values.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_access::compliance::ColorRgba;
    ///
    /// let color = ColorRgba::new(0.5, 0.5, 0.5, 1.0);
    /// assert_eq!(color.r, 0.5);
    /// ```
    #[inline]
    pub const fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    /// Creates an opaque `ColorRgba` (`a = 1.0`) from `(r, g, b)` floats.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_access::compliance::ColorRgba;
    ///
    /// let red = ColorRgba::rgb(1.0, 0.0, 0.0);
    /// assert_eq!(red.a, 1.0);
    /// ```
    #[inline]
    pub const fn rgb(r: f32, g: f32, b: f32) -> Self {
        Self { r, g, b, a: 1.0 }
    }

    /// Creates an opaque `ColorRgba` from 8-bit integer RGB components `[0, 255]`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_access::compliance::ColorRgba;
    ///
    /// let white = ColorRgba::from_rgb_u8(255, 255, 255);
    /// assert_eq!(white.r, 1.0);
    /// assert_eq!(white.g, 1.0);
    /// assert_eq!(white.b, 1.0);
    /// ```
    #[inline]
    pub fn from_rgb_u8(r: u8, g: u8, b: u8) -> Self {
        Self {
            r: r as f32 / 255.0,
            g: g as f32 / 255.0,
            b: b as f32 / 255.0,
            a: 1.0,
        }
    }
}

/// Computes the relative luminance of a color per the WCAG 2.2 sRGB linearization formula.
///
/// The formula converts sRGB components to linear RGB, then computes:
/// `L = 0.2126 * R + 0.7152 * G + 0.0722 * B`.
///
/// # Examples
///
/// ```
/// use martensite_access::compliance::{ColorRgba, relative_luminance};
///
/// let white = ColorRgba::rgb(1.0, 1.0, 1.0);
/// assert!((relative_luminance(white) - 1.0).abs() < 1e-4);
///
/// let black = ColorRgba::rgb(0.0, 0.0, 0.0);
/// assert!((relative_luminance(black) - 0.0).abs() < 1e-4);
/// ```
pub fn relative_luminance(c: ColorRgba) -> f32 {
    let linearize = |val: f32| -> f32 {
        let clamped = val.clamp(0.0, 1.0);
        if clamped <= 0.04045 {
            clamped / 12.92
        } else {
            ((clamped + 0.055) / 1.055).powf(2.4)
        }
    };

    let r_lin = linearize(c.r);
    let g_lin = linearize(c.g);
    let b_lin = linearize(c.b);

    0.2126 * r_lin + 0.7152 * g_lin + 0.0722 * b_lin
}

/// Calculates the WCAG 2.2 contrast ratio between two colors.
///
/// Contrast ratio is calculated as `(L1 + 0.05) / (L2 + 0.05)` where `L1` is the lighter
/// luminance and `L2` is the darker luminance. The result ranges from `1.0` (identical)
/// to `21.0` (pure black vs pure white).
///
/// # Examples
///
/// ```
/// use martensite_access::compliance::{ColorRgba, contrast_ratio};
///
/// let white = ColorRgba::rgb(1.0, 1.0, 1.0);
/// let black = ColorRgba::rgb(0.0, 0.0, 0.0);
/// assert!((contrast_ratio(white, black) - 21.0).abs() < 0.1);
/// assert_eq!(contrast_ratio(white, white), 1.0);
/// ```
pub fn contrast_ratio(c1: ColorRgba, c2: ColorRgba) -> f32 {
    let l1 = relative_luminance(c1);
    let l2 = relative_luminance(c2);

    let lighter = l1.max(l2);
    let darker = l1.min(l2);

    (lighter + 0.05) / (darker + 0.05)
}

/// Target WCAG accessibility conformance tier.
///
/// # Examples
///
/// ```
/// use martensite_access::compliance::WcagLevel;
///
/// let level = WcagLevel::Aa;
/// assert_eq!(level, WcagLevel::default());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum WcagLevel {
    /// WCAG Level AA (standard enterprise and regulatory requirement).
    #[default]
    Aa,
    /// WCAG Level AAA (enhanced accessibility standard).
    Aaa,
}

/// Typographic text size category under WCAG definitions.
///
/// WCAG defines large text as at least 18pt regular or 14pt bold (typically >= 24px regular or >= 18.66px bold).
///
/// # Examples
///
/// ```
/// use martensite_access::compliance::TextSize;
///
/// let size = TextSize::Normal;
/// assert_eq!(size, TextSize::default());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TextSize {
    /// Standard body text (< 18pt regular or < 14pt bold).
    #[default]
    Normal,
    /// Large text (>= 18pt regular or >= 14pt bold).
    Large,
}

/// Verifies whether text with foreground color `fg` against background color `bg` satisfies
/// WCAG 2.2 contrast requirements.
///
/// Required contrast ratios:
/// - `Normal` text, `Level AA`: 4.5:1
/// - `Normal` text, `Level AAA`: 7.0:1
/// - `Large` text, `Level AA`: 3.0:1
/// - `Large` text, `Level AAA`: 4.5:1
///
/// # Examples
///
/// ```
/// use martensite_access::compliance::{ColorRgba, TextSize, WcagLevel, check_text_contrast};
///
/// let white = ColorRgba::rgb(1.0, 1.0, 1.0);
/// let black = ColorRgba::rgb(0.0, 0.0, 0.0);
/// assert!(check_text_contrast(black, white, TextSize::Normal, WcagLevel::Aa));
/// assert!(check_text_contrast(black, white, TextSize::Normal, WcagLevel::Aaa));
/// ```
pub fn check_text_contrast(fg: ColorRgba, bg: ColorRgba, size: TextSize, level: WcagLevel) -> bool {
    let ratio = contrast_ratio(fg, bg);
    let threshold = match (size, level) {
        (TextSize::Normal, WcagLevel::Aa) => 4.5,
        (TextSize::Normal, WcagLevel::Aaa) => 7.0,
        (TextSize::Large, WcagLevel::Aa) => 3.0,
        (TextSize::Large, WcagLevel::Aaa) => 4.5,
    };
    ratio >= threshold
}

/// Verifies non-text user interface components and graphical objects against WCAG 2.2 Criterion 1.4.11.
///
/// Requires a minimum contrast ratio of 3.0:1 against adjacent background colors.
///
/// # Examples
///
/// ```
/// use martensite_access::compliance::{ColorRgba, check_ui_component_contrast};
///
/// let button_border = ColorRgba::rgb(0.2, 0.2, 0.2);
/// let container_bg = ColorRgba::rgb(1.0, 1.0, 1.0);
/// assert!(check_ui_component_contrast(button_border, container_bg));
/// ```
pub fn check_ui_component_contrast(fg: ColorRgba, bg: ColorRgba) -> bool {
    contrast_ratio(fg, bg) >= 3.0
}

/// Verifies touch and pointer target dimensions against WCAG 2.2 Criterion 2.5.8 (Target Size - Minimum).
///
/// Requires that interactive targets measure at least 24 by 24 CSS pixels.
///
/// # Examples
///
/// ```
/// use martensite_access::compliance::check_target_size;
///
/// assert!(check_target_size(24.0, 24.0));
/// assert!(check_target_size(48.0, 48.0));
/// assert!(!check_target_size(20.0, 30.0));
/// ```
pub fn check_target_size(width: f32, height: f32) -> bool {
    width >= 24.0 && height >= 24.0
}

/// Focus indicator appearance validation per WCAG 2.2 Criteria 2.4.11 and 2.4.13.
///
/// # Examples
///
/// ```
/// use martensite_access::compliance::FocusAppearanceCheck;
///
/// let check = FocusAppearanceCheck::new(3.5, 2.0);
/// assert!(check.is_compliant());
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FocusAppearanceCheck {
    /// Contrast ratio of the focus indicator against adjacent colors.
    pub contrast_ratio: f32,
    /// Thickness of the indicator outline around the perimeter in pixels.
    pub perimeter_thickness: f32,
}

impl FocusAppearanceCheck {
    /// Creates a new `FocusAppearanceCheck`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_access::compliance::FocusAppearanceCheck;
    ///
    /// let check = FocusAppearanceCheck::new(4.0, 2.0);
    /// assert_eq!(check.contrast_ratio, 4.0);
    /// ```
    #[inline]
    pub const fn new(contrast_ratio: f32, perimeter_thickness: f32) -> Self {
        Self {
            contrast_ratio,
            perimeter_thickness,
        }
    }

    /// Returns `true` if the focus indicator satisfies WCAG 2.2 focus appearance guidelines.
    ///
    /// Requires a contrast ratio of at least 3.0:1 and a perimeter thickness of at least 2.0 pixels.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_access::compliance::FocusAppearanceCheck;
    ///
    /// let valid = FocusAppearanceCheck::new(3.0, 2.0);
    /// assert!(valid.is_compliant());
    ///
    /// let thin = FocusAppearanceCheck::new(3.0, 1.0);
    /// assert!(!thin.is_compliant());
    /// ```
    #[inline]
    pub fn is_compliant(&self) -> bool {
        self.contrast_ratio >= 3.0 && self.perimeter_thickness >= 2.0
    }
}

/// Section 508 VPAT (Voluntary Product Accessibility Template) evaluation report.
///
/// # Examples
///
/// ```
/// use martensite_access::compliance::Section508VpatReport;
///
/// let report = Section508VpatReport::new(true, true, true);
/// assert!(report.all_compliant());
/// assert!(report.generate_summary().contains("Section 508 VPAT Report"));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Section508VpatReport {
    /// Whether text and UI component contrast pass WCAG 2.2 AA.
    pub wcag_contrast_pass: bool,
    /// Whether interactive pointer targets meet minimum size requirements.
    pub target_size_pass: bool,
    /// Whether focus appearance indicators meet contrast and thickness guidelines.
    pub focus_appearance_pass: bool,
}

impl Section508VpatReport {
    /// Creates a new `Section508VpatReport`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_access::compliance::Section508VpatReport;
    ///
    /// let report = Section508VpatReport::new(true, true, true);
    /// assert!(report.all_compliant());
    /// ```
    #[inline]
    pub const fn new(
        wcag_contrast_pass: bool,
        target_size_pass: bool,
        focus_appearance_pass: bool,
    ) -> Self {
        Self {
            wcag_contrast_pass,
            target_size_pass,
            focus_appearance_pass,
        }
    }

    /// Returns `true` if all accessibility criteria pass evaluation.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_access::compliance::Section508VpatReport;
    ///
    /// let report = Section508VpatReport::new(true, true, false);
    /// assert!(!report.all_compliant());
    /// ```
    #[inline]
    pub const fn all_compliant(&self) -> bool {
        self.wcag_contrast_pass && self.target_size_pass && self.focus_appearance_pass
    }

    /// Generates a formatted VPAT evaluation summary markdown report.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_access::compliance::Section508VpatReport;
    ///
    /// let report = Section508VpatReport::new(true, true, true);
    /// let summary = report.generate_summary();
    /// assert!(summary.contains("PASS"));
    /// ```
    pub fn generate_summary(&self) -> String {
        let pass_fail = |pass: bool| if pass { "PASS" } else { "FAIL" };
        let overall = if self.all_compliant() {
            "COMPLIANT"
        } else {
            "NON-COMPLIANT"
        };

        format!(
            "# Section 508 VPAT Report\n\n\
             | Criterion | Standard | Result |\n\
             |---|---|---|\n\
             | WCAG 2.2 Contrast | 1.4.3 / 1.4.11 AA | {} |\n\
             | Target Size (Min) | 2.5.8 (24x24 px) | {} |\n\
             | Focus Appearance | 2.4.11 / 2.4.13 | {} |\n\n\
             **Overall Status**: {}\n",
            pass_fail(self.wcag_contrast_pass),
            pass_fail(self.target_size_pass),
            pass_fail(self.focus_appearance_pass),
            overall
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_relative_luminance_extremes() {
        let white = ColorRgba::rgb(1.0, 1.0, 1.0);
        let black = ColorRgba::rgb(0.0, 0.0, 0.0);
        assert!((relative_luminance(white) - 1.0).abs() < 1e-4);
        assert!((relative_luminance(black) - 0.0).abs() < 1e-4);
    }

    #[test]
    fn test_contrast_ratio_symmetric() {
        let c1 = ColorRgba::rgb(0.1, 0.2, 0.3);
        let c2 = ColorRgba::rgb(0.9, 0.8, 0.7);
        assert_eq!(contrast_ratio(c1, c2), contrast_ratio(c2, c1));
    }

    #[test]
    fn test_check_text_contrast_thresholds() {
        let white = ColorRgba::rgb(1.0, 1.0, 1.0);
        let black = ColorRgba::rgb(0.0, 0.0, 0.0);
        let gray = ColorRgba::rgb(0.5, 0.5, 0.5);

        // Black on white (21:1) passes all
        assert!(check_text_contrast(
            black,
            white,
            TextSize::Normal,
            WcagLevel::Aa
        ));
        assert!(check_text_contrast(
            black,
            white,
            TextSize::Normal,
            WcagLevel::Aaa
        ));
        assert!(check_text_contrast(
            black,
            white,
            TextSize::Large,
            WcagLevel::Aa
        ));
        assert!(check_text_contrast(
            black,
            white,
            TextSize::Large,
            WcagLevel::Aaa
        ));

        // Gray on white (~3.9:1) fails normal AA (4.5) but passes large AA (3.0)
        assert!(!check_text_contrast(
            gray,
            white,
            TextSize::Normal,
            WcagLevel::Aa
        ));
        assert!(check_text_contrast(
            gray,
            white,
            TextSize::Large,
            WcagLevel::Aa
        ));
    }

    #[test]
    fn test_check_ui_component_contrast() {
        let white = ColorRgba::rgb(1.0, 1.0, 1.0);
        let dark = ColorRgba::rgb(0.3, 0.3, 0.3);
        assert!(check_ui_component_contrast(dark, white));

        let light_gray = ColorRgba::rgb(0.9, 0.9, 0.9);
        assert!(!check_ui_component_contrast(light_gray, white));
    }

    #[test]
    fn test_check_target_size() {
        assert!(check_target_size(24.0, 24.0));
        assert!(check_target_size(30.0, 24.0));
        assert!(!check_target_size(23.9, 24.0));
        assert!(!check_target_size(24.0, 10.0));
    }

    #[test]
    fn test_focus_appearance() {
        let pass = FocusAppearanceCheck::new(3.2, 2.0);
        assert!(pass.is_compliant());

        let low_contrast = FocusAppearanceCheck::new(2.8, 2.0);
        assert!(!low_contrast.is_compliant());

        let thin = FocusAppearanceCheck::new(4.0, 1.5);
        assert!(!thin.is_compliant());
    }

    #[test]
    fn test_vpat_report() {
        let pass_report = Section508VpatReport::new(true, true, true);
        assert!(pass_report.all_compliant());
        let summary = pass_report.generate_summary();
        assert!(summary.contains("COMPLIANT"));
        assert!(!summary.contains("NON-COMPLIANT"));

        let fail_report = Section508VpatReport::new(true, false, true);
        assert!(!fail_report.all_compliant());
        let summary_fail = fail_report.generate_summary();
        assert!(summary_fail.contains("NON-COMPLIANT"));
    }
}
