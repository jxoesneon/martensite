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

    /// Composites this (foreground) color over `background` using the
    /// source-over alpha blending formula.
    ///
    /// This is required before contrast checking whenever the foreground
    /// color has an alpha channel less than 1.0. The WCAG contrast ratio
    /// should be computed on the actual color visible after compositing,
    /// not on the raw transparent RGBA foreground.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_access::compliance::ColorRgba;
    ///
    /// let fg = ColorRgba::new(0.0, 0.0, 0.0, 0.5);
    /// let bg = ColorRgba::rgb(1.0, 1.0, 1.0);
    /// let c = fg.composite_over(bg);
    /// assert_eq!(c.r, 0.5);
    /// assert_eq!(c.a, 1.0);
    /// ```
    #[inline]
    pub fn composite_over(self, background: Self) -> Self {
        let a = self.a;
        let inv_a = 1.0 - a;
        Self {
            r: self.r * a + background.r * inv_a,
            g: self.g * a + background.g * inv_a,
            b: self.b * a + background.b * inv_a,
            a: a + background.a * inv_a,
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
/// This function operates on RGB values and ignores alpha. For transparent
/// foregrounds, call [`ColorRgba::composite_over`] first so contrast is
/// computed on the color that is actually visible.
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
/// The foreground is first composited over the background, so transparent
/// text is evaluated against the color that is actually rendered.
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
    let fg = fg.composite_over(bg);
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
/// The foreground is first composited over the background so transparent
/// UI components are evaluated against the rendered color.
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
    let fg = fg.composite_over(bg);
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

/// Context describing the WCAG 2.2 Criterion 2.5.8 (Target Size - Minimum)
/// exceptions for a given target.
///
/// # Examples
///
/// ```
/// use martensite_access::compliance::TargetSizeContext;
///
/// let ctx = TargetSizeContext::default();
/// assert!(!ctx.inline);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct TargetSizeContext {
    /// Spacing equivalent: a 24 CSS pixel diameter circle centered on the
    /// target does not intersect another target or its spacing circle.
    pub spacing_equivalent: bool,
    /// Inline: the target is within a sentence or text block.
    pub inline: bool,
    /// Essential: a particular target presentation is essential to the
    /// information being conveyed.
    pub essential: bool,
    /// User-agent controlled: the target size is set by the user agent and
    /// not modified by the author.
    pub user_agent_controlled: bool,
}

/// Verifies touch and pointer target dimensions against WCAG 2.2 Criterion
/// 2.5.8 (Target Size - Minimum), taking the standard exceptions into
/// account.
///
/// Targets must measure at least 24 by 24 CSS pixels unless one of the
/// documented exceptions applies.
///
/// # Examples
///
/// ```
/// use martensite_access::compliance::{TargetSizeContext, check_target_size_with_exceptions};
///
/// let mut ctx = TargetSizeContext::default();
/// ctx.inline = true;
///
/// assert!(check_target_size_with_exceptions(24.0, 24.0, ctx));
/// assert!(check_target_size_with_exceptions(20.0, 20.0, ctx));
/// ```
pub fn check_target_size_with_exceptions(
    width: f32,
    height: f32,
    context: TargetSizeContext,
) -> bool {
    width >= 24.0 && height >= 24.0
        || context.spacing_equivalent
        || context.inline
        || context.essential
        || context.user_agent_controlled
}

/// Focus Not Obscured validation per WCAG 2.2 Criterion 2.4.11 (Minimum).
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

/// Computes the minimum focus-indicator area required by WCAG 2.2
/// Criterion 2.4.13 (Focus Appearance, Level AAA).
///
/// The criterion requires the focus indicator's area to be at least as
/// large as a 2 CSS pixel thick perimeter surrounding the component's
/// bounding box. For a `width` × `height` rectangle, the perimeter path
/// length is `2 * (width + height)`, so the required indicator area is:
///
/// ```text
/// min_area = 2 px * 2 * (width + height) = 4 * (width + height) px²
/// ```
///
/// # Examples
///
/// ```
/// use martensite_access::compliance::minimum_focus_indicator_area;
///
/// // A 100x40 px button needs a focus indicator covering at least 560 px².
/// assert_eq!(minimum_focus_indicator_area(100.0, 40.0), 560.0);
/// ```
#[inline]
pub fn minimum_focus_indicator_area(width: f32, height: f32) -> f32 {
    4.0 * (width.max(0.0) + height.max(0.0))
}

/// Evaluates WCAG 2.2 Criterion 2.4.13 (Focus Appearance, Level AAA) for a
/// rendered focus indicator.
///
/// The criterion requires all of the following:
/// - **Area:** the indicator covers at least the area of a 2 CSS pixel
///   thick perimeter of the unfocused component — see
///   [`minimum_focus_indicator_area`] — *or* is at least 4 CSS pixels
///   thick along the shortest side of the component.
/// - **Contrast:** the indicator's color achieves a contrast ratio of at
///   least 3:1 against the same pixels in the unfocused state.
/// - **Visibility:** the indicator is not entirely hidden by
///   author-created content (e.g. overlapping elements or clipping).
///
/// # Examples
///
/// ```
/// use martensite_access::compliance::{FocusAreaCheck, minimum_focus_indicator_area};
///
/// // 100x40 px button, 4px solid outline (area = 2*4*(108+48) ≈ 1248 px²,
/// // well above the 560 px² minimum), 4.5:1 contrast, not obscured.
/// let check = FocusAreaCheck::new(100.0, 40.0, 1248.0, 4.5, false);
/// assert!(check.is_compliant());
///
/// // Insufficient area fails.
/// let small = FocusAreaCheck::new(100.0, 40.0, 100.0, 4.5, false);
/// assert!(!small.is_compliant());
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FocusAreaCheck {
    /// Width of the unfocused component's bounding box, in CSS pixels.
    pub component_width: f32,
    /// Height of the unfocused component's bounding box, in CSS pixels.
    pub component_height: f32,
    /// Total area of the rendered focus indicator, in CSS pixels².
    pub indicator_area: f32,
    /// Contrast ratio between the focused and unfocused indicator pixels.
    pub indicator_contrast: f32,
    /// Whether the indicator is entirely hidden by author content.
    pub obscured: bool,
}

impl FocusAreaCheck {
    /// Creates a new `FocusAreaCheck`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_access::compliance::FocusAreaCheck;
    ///
    /// let check = FocusAreaCheck::new(24.0, 24.0, 200.0, 3.0, false);
    /// assert!(check.is_compliant());
    /// ```
    #[inline]
    pub const fn new(
        component_width: f32,
        component_height: f32,
        indicator_area: f32,
        indicator_contrast: f32,
        obscured: bool,
    ) -> Self {
        Self {
            component_width,
            component_height,
            indicator_area,
            indicator_contrast,
            obscured,
        }
    }

    /// The minimum indicator area required for this component.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_access::compliance::FocusAreaCheck;
    ///
    /// let check = FocusAreaCheck::new(100.0, 40.0, 0.0, 3.0, false);
    /// assert_eq!(check.required_area(), 560.0);
    /// ```
    #[inline]
    pub fn required_area(&self) -> f32 {
        minimum_focus_indicator_area(self.component_width, self.component_height)
    }

    /// Returns `true` if the indicator satisfies the area requirement,
    /// either by covering the 2px-perimeter area or by being at least
    /// 4 CSS px thick along the component's shortest side.
    ///
    /// The 4px-along-shortest-side alternative is approximated as an
    /// indicator area of at least `4 * min(width, height)` px².
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_access::compliance::FocusAreaCheck;
    ///
    /// // Meets the 2px-perimeter rule.
    /// assert!(FocusAreaCheck::new(100.0, 40.0, 560.0, 3.0, false).area_compliant());
    /// // Meets the 4px-along-shortest-side alternative (4*40 = 160 px²).
    /// assert!(FocusAreaCheck::new(100.0, 40.0, 160.0, 3.0, false).area_compliant());
    /// assert!(!FocusAreaCheck::new(100.0, 40.0, 159.0, 3.0, false).area_compliant());
    /// ```
    #[inline]
    pub fn area_compliant(&self) -> bool {
        let shortest_side = self.component_width.min(self.component_height).max(0.0);
        self.indicator_area >= self.required_area() || self.indicator_area >= 4.0 * shortest_side
    }

    /// Returns `true` if the indicator contrast is at least 3:1.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_access::compliance::FocusAreaCheck;
    ///
    /// assert!(FocusAreaCheck::new(10.0, 10.0, 100.0, 3.0, false).contrast_compliant());
    /// assert!(!FocusAreaCheck::new(10.0, 10.0, 100.0, 2.9, false).contrast_compliant());
    /// ```
    #[inline]
    pub fn contrast_compliant(&self) -> bool {
        self.indicator_contrast >= 3.0
    }

    /// Returns `true` if all 2.4.13 sub-requirements are satisfied.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_access::compliance::FocusAreaCheck;
    ///
    /// assert!(FocusAreaCheck::new(50.0, 50.0, 400.0, 4.5, false).is_compliant());
    /// assert!(!FocusAreaCheck::new(50.0, 50.0, 400.0, 4.5, true).is_compliant());
    /// ```
    #[inline]
    pub fn is_compliant(&self) -> bool {
        !self.obscured && self.area_compliant() && self.contrast_compliant()
    }
}

/// Conformance level reported for a single criterion in a VPAT 2.5 Rev
/// accessibility conformance report.
///
/// These map to the standard VPAT conformance terms. `NotEvaluated` is
/// the default for criteria the automated checker cannot assess —
/// reporting `Supports` for unevaluated criteria would constitute a
/// false conformance claim.
///
/// # Examples
///
/// ```
/// use martensite_access::compliance::VpatConformanceLevel;
///
/// let level = VpatConformanceLevel::Supports;
/// assert_eq!(level.as_str(), "Supports");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum VpatConformanceLevel {
    /// The product fully meets the criterion.
    Supports,
    /// Some functionality meets the criterion; gaps documented in remarks.
    PartiallySupports,
    /// The product does not meet the criterion.
    DoesNotSupport,
    /// The criterion is not relevant to the product.
    NotApplicable,
    /// The criterion has not been evaluated. Default — never assume
    /// conformance.
    #[default]
    NotEvaluated,
}

impl VpatConformanceLevel {
    /// Returns the canonical VPAT term.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_access::compliance::VpatConformanceLevel;
    ///
    /// assert_eq!(VpatConformanceLevel::PartiallySupports.as_str(), "Partially Supports");
    /// ```
    #[inline]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Supports => "Supports",
            Self::PartiallySupports => "Partially Supports",
            Self::DoesNotSupport => "Does Not Support",
            Self::NotApplicable => "Not Applicable",
            Self::NotEvaluated => "Not Evaluated",
        }
    }
}

/// One evaluated row in a VPAT 2.5 Rev conformance table.
///
/// # Examples
///
/// ```
/// use martensite_access::compliance::{VpatConformanceLevel, VpatCriterion};
///
/// let c = VpatCriterion::new(
///     "WCAG 2.2",
///     "1.4.3 Contrast (Minimum)",
///     VpatConformanceLevel::Supports,
///     "Automated contrast verification passes at AA.",
/// );
/// assert_eq!(c.table, "WCAG 2.2");
/// assert!(c.is_evaluated());
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct VpatCriterion {
    /// The conformance table this row belongs to
    /// (e.g. `"WCAG 2.x"`, `"Revised Section 508"`).
    pub table: &'static str,
    /// Criterion identifier and title (e.g. `"1.4.3 Contrast (Minimum)"`).
    pub criterion: String,
    /// Reported conformance level.
    pub level: VpatConformanceLevel,
    /// Remarks and explanations (required for `PartiallySupports` and
    /// `DoesNotSupport`).
    pub remarks: String,
}

impl VpatCriterion {
    /// Creates a new criterion row.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_access::compliance::{VpatConformanceLevel, VpatCriterion};
    ///
    /// let c = VpatCriterion::new(
    ///     "Revised Section 508",
    ///     "502.3 Accessibility Services",
    ///     VpatConformanceLevel::Supports,
    ///     "AccessKit adapter exposes full UIA/AT-SPI surface.",
    /// );
    /// assert!(c.is_evaluated());
    /// ```
    #[inline]
    pub fn new(
        table: &'static str,
        criterion: impl Into<String>,
        level: VpatConformanceLevel,
        remarks: impl Into<String>,
    ) -> Self {
        Self {
            table,
            criterion: criterion.into(),
            level,
            remarks: remarks.into(),
        }
    }

    /// Returns `true` if this criterion was actually evaluated (any level
    /// other than [`VpatConformanceLevel::NotEvaluated`]).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_access::compliance::{VpatConformanceLevel, VpatCriterion};
    ///
    /// let c = VpatCriterion::new("T", "x", VpatConformanceLevel::NotEvaluated, "");
    /// assert!(!c.is_evaluated());
    /// ```
    #[inline]
    pub fn is_evaluated(&self) -> bool {
        self.level != VpatConformanceLevel::NotEvaluated
    }
}

/// A structured ICT Accessibility Conformance Report following the
/// VPAT 2.5 Rev template structure.
///
/// This is an **automated evaluation aid**, not a certification: only
/// criteria with programmatic checkers (contrast, focus appearance,
/// target size) are populated by [`VpatReport::evaluate`]; all other
/// criteria remain [`VpatConformanceLevel::NotEvaluated`] and must be
/// assessed manually before the report can claim conformance. The
/// generated markdown states this limitation explicitly.
///
/// # Examples
///
/// ```
/// use martensite_access::compliance::{VpatConformanceLevel, VpatReport};
///
/// let mut report = VpatReport::new("Martensite UI", "0.11.0", "2025-01-01");
/// report.evaluate(true, true, true);
/// let md = report.render_markdown();
/// assert!(md.contains("Accessibility Conformance Report"));
/// assert!(md.contains("1.4.3 Contrast (Minimum)"));
/// // Automated evaluation must not claim full certification.
/// assert!(md.contains("not a certification"));
/// ```
#[derive(Debug, Clone)]
pub struct VpatReport {
    /// Product name appearing in the report header.
    pub product_name: String,
    /// Product version under evaluation.
    pub product_version: String,
    /// Report date (ISO 8601 `YYYY-MM-DD` recommended).
    pub report_date: String,
    /// Evaluation methods used (e.g. `"Automated rule engine + manual review"`).
    pub evaluation_methods: String,
    /// All criterion rows, grouped by their `table` field.
    pub criteria: Vec<VpatCriterion>,
}

impl VpatReport {
    /// Creates an empty report for the given product.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_access::compliance::VpatReport;
    ///
    /// let report = VpatReport::new("App", "1.0.0", "2025-01-01");
    /// assert!(report.criteria.is_empty());
    /// ```
    pub fn new(
        product_name: impl Into<String>,
        product_version: impl Into<String>,
        report_date: impl Into<String>,
    ) -> Self {
        Self {
            product_name: product_name.into(),
            product_version: product_version.into(),
            report_date: report_date.into(),
            evaluation_methods: "Automated WCAG rule evaluation".to_string(),
            criteria: Vec::new(),
        }
    }

    /// Populates the rows covered by the automated checkers.
    ///
    /// Evaluated criteria:
    /// - WCAG `1.4.3 Contrast (Minimum)` (AA) — `contrast_pass`
    /// - WCAG `1.4.11 Non-text Contrast` (AA) — `contrast_pass`
    /// - WCAG `2.5.8 Target Size (Minimum)` (AA) — `target_size_pass`
    /// - WCAG `2.4.13 Focus Appearance` (AAA) — `focus_appearance_pass`
    /// - Revised Section 508 `502.3 Accessibility Services` — marked
    ///   `NotEvaluated` (requires manual platform testing).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_access::compliance::{VpatConformanceLevel, VpatReport};
    ///
    /// let mut report = VpatReport::new("App", "1.0", "2025-01-01");
    /// report.evaluate(true, false, true);
    /// assert_eq!(report.count_level(VpatConformanceLevel::DoesNotSupport), 1);
    /// ```
    pub fn evaluate(&mut self, contrast_pass: bool, target_size_pass: bool, focus_pass: bool) {
        let level = |pass: bool, remarks: &str| {
            if pass {
                (
                    VpatConformanceLevel::Supports,
                    "Automated evaluation passed.".to_string(),
                )
            } else {
                (
                    VpatConformanceLevel::DoesNotSupport,
                    format!("Automated evaluation failed: {remarks}"),
                )
            }
        };

        let (c_lvl, c_rem) = level(
            contrast_pass,
            "contrast ratios below WCAG 2.2 AA thresholds",
        );
        self.criteria.push(VpatCriterion::new(
            "WCAG 2.x",
            "1.4.3 Contrast (Minimum) (Level AA)",
            c_lvl,
            c_rem.clone(),
        ));
        self.criteria.push(VpatCriterion::new(
            "WCAG 2.x",
            "1.4.11 Non-text Contrast (Level AA)",
            c_lvl,
            c_rem,
        ));

        let (t_lvl, t_rem) = level(target_size_pass, "targets smaller than 24x24 CSS px");
        self.criteria.push(VpatCriterion::new(
            "WCAG 2.x",
            "2.5.8 Target Size (Minimum) (Level AA)",
            t_lvl,
            t_rem,
        ));

        let (f_lvl, f_rem) = level(
            focus_pass,
            "focus indicator below 2.4.13 area/contrast requirements",
        );
        self.criteria.push(VpatCriterion::new(
            "WCAG 2.x",
            "2.4.13 Focus Appearance (Level AAA)",
            f_lvl,
            f_rem,
        ));

        self.criteria.push(VpatCriterion::new(
            "Revised Section 508",
            "502.3 Accessibility Services",
            VpatConformanceLevel::NotEvaluated,
            "Requires manual platform assistive-technology testing.",
        ));
    }

    /// Adds a manually assessed criterion row.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_access::compliance::{VpatConformanceLevel, VpatReport};
    ///
    /// let mut report = VpatReport::new("App", "1.0", "2025-01-01");
    /// report.add_criterion(VpatConformanceLevel::Supports, "WCAG 2.x", "2.1.1 Keyboard", "All actions keyboard-reachable.");
    /// assert_eq!(report.criteria.len(), 1);
    /// ```
    pub fn add_criterion(
        &mut self,
        level: VpatConformanceLevel,
        table: &'static str,
        criterion: impl Into<String>,
        remarks: impl Into<String>,
    ) {
        self.criteria
            .push(VpatCriterion::new(table, criterion, level, remarks));
    }

    /// Counts rows at the given conformance level.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_access::compliance::{VpatConformanceLevel, VpatReport};
    ///
    /// let mut report = VpatReport::new("App", "1.0", "2025-01-01");
    /// report.evaluate(true, true, true);
    /// assert_eq!(report.count_level(VpatConformanceLevel::Supports), 4);
    /// assert_eq!(report.count_level(VpatConformanceLevel::NotEvaluated), 1);
    /// ```
    pub fn count_level(&self, level: VpatConformanceLevel) -> usize {
        self.criteria.iter().filter(|c| c.level == level).count()
    }

    /// Validates report integrity: every `PartiallySupports` or
    /// `DoesNotSupport` row must carry non-empty remarks.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_access::compliance::{VpatConformanceLevel, VpatReport};
    ///
    /// let mut report = VpatReport::new("App", "1.0", "2025-01-01");
    /// report.evaluate(true, true, true);
    /// assert!(report.validate().is_empty());
    /// report.add_criterion(VpatConformanceLevel::DoesNotSupport, "WCAG 2.x", "x.y.z Bad", "");
    /// assert_eq!(report.validate().len(), 1);
    /// ```
    pub fn validate(&self) -> Vec<String> {
        let mut issues = Vec::new();
        for c in &self.criteria {
            let needs_remarks = matches!(
                c.level,
                VpatConformanceLevel::PartiallySupports | VpatConformanceLevel::DoesNotSupport
            );
            if needs_remarks && c.remarks.trim().is_empty() {
                issues.push(format!(
                    "Criterion '{}' is '{}' but has no remarks.",
                    c.criterion,
                    c.level.as_str()
                ));
            }
        }
        issues
    }

    /// Renders the report as a VPAT 2.5 Rev-style markdown document.
    ///
    /// The output explicitly states that the report is an automated
    /// evaluation aid and **not a certification**, and lists the count of
    /// unevaluated criteria so that passing automated checks cannot be
    /// mistaken for a conformance claim over the full standard.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_access::compliance::VpatReport;
    ///
    /// let mut report = VpatReport::new("App", "1.0", "2025-01-01");
    /// report.evaluate(true, true, false);
    /// let md = report.render_markdown();
    /// assert!(md.contains("| Conformance Level | Remarks |"));
    /// assert!(md.contains("Does Not Support"));
    /// ```
    pub fn render_markdown(&self) -> String {
        use std::fmt::Write;

        let mut out = String::new();
        let _ = writeln!(out, "# Accessibility Conformance Report");
        let _ = writeln!(out, "VPAT 2.5 Rev — automated evaluation aid\n");
        let _ = writeln!(out, "- **Product**: {}", self.product_name);
        let _ = writeln!(out, "- **Version**: {}", self.product_version);
        let _ = writeln!(out, "- **Report Date**: {}", self.report_date);
        let _ = writeln!(
            out,
            "- **Evaluation Methods**: {}\n",
            self.evaluation_methods
        );
        let _ = writeln!(
            out,
            "> **Disclaimer**: This report is generated by automated rule \
             evaluation and is not a certification. Only criteria listed \
             below were evaluated; criteria marked \"Not Evaluated\" \
             require manual assessment before any conformance claim can \
             be made.\n"
        );

        // Summary counts.
        let _ = writeln!(out, "## Summary");
        for level in [
            VpatConformanceLevel::Supports,
            VpatConformanceLevel::PartiallySupports,
            VpatConformanceLevel::DoesNotSupport,
            VpatConformanceLevel::NotApplicable,
            VpatConformanceLevel::NotEvaluated,
        ] {
            let _ = writeln!(out, "- {}: {}", level.as_str(), self.count_level(level));
        }
        let _ = writeln!(out);

        // Tables grouped by table name, preserving insertion order.
        let mut tables: Vec<&'static str> = Vec::new();
        for c in &self.criteria {
            if !tables.contains(&c.table) {
                tables.push(c.table);
            }
        }
        for table in tables {
            let _ = writeln!(out, "## {table}");
            let _ = writeln!(out, "| Criteria | Conformance Level | Remarks |");
            let _ = writeln!(out, "|---|---|---|");
            for c in self.criteria.iter().filter(|c| c.table == table) {
                let _ = writeln!(
                    out,
                    "| {} | {} | {} |",
                    c.criterion,
                    c.level.as_str(),
                    c.remarks
                );
            }
            let _ = writeln!(out);
        }

        let issues = self.validate();
        if !issues.is_empty() {
            let _ = writeln!(out, "## Validation Issues");
            for issue in issues {
                let _ = writeln!(out, "- {issue}");
            }
        }

        out
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
            "PASSED"
        } else {
            "FAILED"
        };

        format!(
            "# Section 508 VPAT Report\n\n\
             | Criterion | Standard | Result |\n\
             |---|---|---|\n\
             | WCAG 2.2 Contrast | 1.4.3 / 1.4.11 AA | {} |\n\
             | Target Size (Min) | 2.5.8 (24x24 px) | {} |\n\
             | Focus Appearance | 2.4.11 | {} |\n\n\
             Automated subset: {} (3 of ~50 criteria evaluated — not a conformance claim)\n\n\
             **Disclaimer**: This report reflects only the criteria exercised by automated checks. \
             Full VPAT/Section 508 conformance requires manual evaluation of all applicable criteria.\n",
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

        let ctx = TargetSizeContext {
            inline: true,
            ..Default::default()
        };
        assert!(!check_target_size(20.0, 20.0));
        assert!(check_target_size_with_exceptions(20.0, 20.0, ctx));

        let ctx = TargetSizeContext {
            essential: true,
            ..Default::default()
        };
        assert!(check_target_size_with_exceptions(18.0, 18.0, ctx));
    }

    #[test]
    fn test_composite_over() {
        let black_50 = ColorRgba::new(0.0, 0.0, 0.0, 0.5);
        let white = ColorRgba::rgb(1.0, 1.0, 1.0);
        let gray = black_50.composite_over(white);
        assert!((gray.r - 0.5).abs() < 1e-5);
        assert!((gray.a - 1.0).abs() < 1e-5);

        // Transparent text over white should fail against the background.
        assert!(!check_text_contrast(
            black_50,
            white,
            TextSize::Normal,
            WcagLevel::Aa
        ));
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
        assert!(summary.contains("Automated subset: PASSED"));
        assert!(summary.contains("not a conformance claim"));

        let fail_report = Section508VpatReport::new(true, false, true);
        assert!(!fail_report.all_compliant());
        let summary_fail = fail_report.generate_summary();
        assert!(summary_fail.contains("Automated subset: FAILED"));
        assert!(summary_fail.contains("not a conformance claim"));
    }
}
