//! Design token dictionary, light/dark theme definitions, and theme diffing.
//!
//! This module implements the semantic design-token layer described in
//! milestone `v0.6.0` section 4.3. A [`Theme`] is a flat map of
//! [`TokenKey`] -> [`ThemeToken`] values, [`ThemeDictionary`] pairs a light
//! and dark [`Theme`], and [`ThemeDiff`] captures the color differences
//! between two themes so that a GPU transition shader can blend them over a
//! 150 ms window.

use crate::Oklab;
use std::collections::HashMap;

/// A semantic design token value.
///
/// Tokens are intentionally typed so that a color cannot be silently
/// interpreted as a spacing value (or vice-versa). The [`Theme`] map stores
/// these as a tagged enum so a single uniform dictionary can describe every
/// restyleable property of the UI.
#[derive(Clone, Debug, PartialEq)]
pub enum ThemeToken {
    /// A color token, stored in the Oklab perceptual color space.
    Color(Oklab),
    /// A spacing or sizing token, expressed in physical pixels.
    Dimension(f32),
    /// A font family token, stored as a CSS-style family name string.
    FontFamily(String),
    /// A font size token, expressed in physical pixels.
    FontSize(f32),
    /// An animation duration token, expressed in milliseconds.
    Duration(f32),
    /// An easing curve parameter token (e.g. a single cubic-bezier scalar).
    Easing(f32),
}

/// A key identifying a semantic design token.
///
/// `TokenKey` is the stable, hashable identity of a token. It is `Copy` so it
/// can be passed around freely, and `Eq + Hash` so it can be used as a
/// [`HashMap`] key.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TokenKey {
    /// The base page background color.
    BackgroundColor,
    /// The elevated surface color (cards, panels).
    SurfaceColor,
    /// The primary brand color.
    PrimaryColor,
    /// The secondary brand color.
    SecondaryColor,
    /// The accent / highlight color.
    AccentColor,
    /// The default text color.
    TextColor,
    /// The muted / secondary text color.
    TextMutedColor,
    /// The inverse text color (text on a colored surface).
    TextInverseColor,
    /// The border / outline color.
    BorderColor,
    /// The divider / separator color.
    DividerColor,
    /// The error / danger semantic color.
    ErrorColor,
    /// The warning semantic color.
    WarningColor,
    /// The success semantic color.
    SuccessColor,
    /// The informational semantic color.
    InfoColor,
    /// The base spacing unit (pixels).
    Spacing,
    /// The small spacing unit (pixels).
    SpacingSmall,
    /// The large spacing unit (pixels).
    SpacingLarge,
    /// The base border radius (pixels).
    BorderRadius,
    /// The small border radius (pixels).
    BorderRadiusSmall,
    /// The large border radius (pixels).
    BorderRadiusLarge,
    /// The small font size (pixels).
    FontSizeSmall,
    /// The medium font size (pixels).
    FontSizeMedium,
    /// The large font size (pixels).
    FontSizeLarge,
    /// The default animation duration (milliseconds).
    AnimationDuration,
    /// The default animation easing parameter.
    AnimationEasing,
    /// The system backdrop material type (Mica, Acrylic, Vibrancy, etc.).
    BackdropMaterial,
    /// The opacity tint applied over the system backdrop (0.0–1.0).
    BackdropTintOpacity,
    /// The fallback background color when the platform does not support system materials.
    BackdropFallbackColor,
    /// The CSD title bar height in physical pixels.
    CsdTitleBarHeight,
    /// The CSD window button corner radius in physical pixels.
    CsdButtonRadius,
    /// The CSD shadow blur radius in physical pixels.
    CsdShadowBlur,
    /// The CSD shadow color.
    CsdShadowColor,
    /// The macOS-specific vibrancy material selection.
    VibrancyMaterial,
}

/// A collection of design tokens describing a complete theme.
///
/// A `Theme` is a flat [`HashMap`] of [`TokenKey`] -> [`ThemeToken`] plus a
/// human-readable `name` (e.g. `"Light"` or `"Dark"`). Themes can be
/// [`Theme::merge`]d to layer overrides, and compared via [`ThemeDiff`] to
/// drive animated transitions.
///
/// # Examples
///
/// ```
/// use martensite_theme::{Oklab, Theme, ThemeToken, TokenKey};
///
/// let mut theme = Theme::new("Custom");
/// theme.set(
///     TokenKey::BackgroundColor,
///     ThemeToken::Color(Oklab { l: 0.96, a: 0.0, b: 0.0, alpha: 1.0 }),
/// );
/// assert!(matches!(
///     theme.color(TokenKey::BackgroundColor),
///     Some(_)
/// ));
/// ```
#[derive(Clone, Debug)]
pub struct Theme {
    /// The token map.
    pub tokens: HashMap<TokenKey, ThemeToken>,
    /// The human-readable theme name (e.g. `"Light"`, `"Dark"`).
    pub name: String,
}

impl Theme {
    /// Creates a new, empty theme with the given name.
    pub fn new(name: &str) -> Self {
        Self {
            tokens: HashMap::new(),
            name: name.to_string(),
        }
    }

    /// Inserts (or replaces) the token stored under `key`.
    pub fn set(&mut self, key: TokenKey, token: ThemeToken) {
        self.tokens.insert(key, token);
    }

    /// Returns the token stored under `key`, if any.
    pub fn get(&self, key: TokenKey) -> Option<&ThemeToken> {
        self.tokens.get(&key)
    }

    /// Convenience accessor that returns the [`Oklab`] value of a color token.
    ///
    /// Returns `None` if the key is absent or the stored token is not a
    /// [`ThemeToken::Color`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_theme::{Oklab, Theme, ThemeToken, TokenKey};
    ///
    /// let mut theme = Theme::new("Custom");
    /// theme.set(
    ///     TokenKey::BackgroundColor,
    ///     ThemeToken::Color(Oklab { l: 0.96, a: 0.0, b: 0.0, alpha: 1.0 }),
    /// );
    /// assert!(theme.color(TokenKey::BackgroundColor).is_some());
    /// assert!(theme.color(TokenKey::Spacing).is_none());
    /// ```
    #[inline]
    #[must_use]
    pub fn color(&self, key: TokenKey) -> Option<Oklab> {
        match self.tokens.get(&key)? {
            ThemeToken::Color(c) => Some(*c),
            _ => None,
        }
    }

    /// Backwards-compatible alias for [`color`](Self::color).
    #[inline]
    #[must_use]
    pub fn get_color(&self, key: TokenKey) -> Option<Oklab> {
        self.color(key)
    }

    /// Convenience accessor that returns the `f32` value of a dimension token.
    ///
    /// Returns `None` if the key is absent or the stored token is not a
    /// numeric token ([`ThemeToken::Dimension`], [`ThemeToken::FontSize`],
    /// [`ThemeToken::Duration`], or [`ThemeToken::Easing`]).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_theme::{Theme, ThemeToken, TokenKey};
    ///
    /// let mut theme = Theme::new("Custom");
    /// theme.set(TokenKey::Spacing, ThemeToken::Dimension(16.0));
    /// assert_eq!(theme.dimension(TokenKey::Spacing), Some(16.0));
    /// assert!(theme.dimension(TokenKey::BackgroundColor).is_none());
    /// ```
    #[inline]
    #[must_use]
    pub fn dimension(&self, key: TokenKey) -> Option<f32> {
        match self.tokens.get(&key)? {
            ThemeToken::Dimension(v)
            | ThemeToken::FontSize(v)
            | ThemeToken::Duration(v)
            | ThemeToken::Easing(v) => Some(*v),
            _ => None,
        }
    }

    /// Backwards-compatible alias for [`dimension`](Self::dimension).
    #[inline]
    #[must_use]
    pub fn get_dimension(&self, key: TokenKey) -> Option<f32> {
        self.dimension(key)
    }

    /// Merges tokens from `other` into `self`.
    ///
    /// Every token present in `other` overwrites the corresponding token in
    /// `self`; tokens only present in `self` are left untouched. This is the
    /// mechanism used to layer theme overrides.
    pub fn merge(&mut self, other: &Theme) {
        for (key, token) in &other.tokens {
            self.tokens.insert(*key, token.clone());
        }
    }
}

/// The active color-scheme mode.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ThemeMode {
    /// The light color scheme.
    Light,
    /// The dark color scheme.
    Dark,
}

/// A pair of light and dark [`Theme`]s addressable by [`ThemeMode`].
///
/// `ThemeDictionary` is the top-level container consumed by the renderer. It
/// holds the two canonical themes and lets callers resolve the active theme
/// for a given [`ThemeMode`].
///
/// # Examples
///
/// ```
/// use martensite_theme::{ThemeDictionary, ThemeMode};
///
/// let dict = ThemeDictionary::new();
/// let light = dict.theme(ThemeMode::Light);
/// assert_eq!(light.name, "Light");
/// ```
#[derive(Clone, Debug)]
pub struct ThemeDictionary {
    /// The light theme.
    pub light: Theme,
    /// The dark theme.
    pub dark: Theme,
}

impl ThemeDictionary {
    /// Creates a new dictionary populated with the default light and dark
    /// themes (see [`default_light`] and [`default_dark`]).
    pub fn new() -> Self {
        Self {
            light: default_light(),
            dark: default_dark(),
        }
    }

    /// Returns a reference to the light theme.
    pub fn light_theme(&self) -> &Theme {
        &self.light
    }

    /// Returns a reference to the dark theme.
    pub fn dark_theme(&self) -> &Theme {
        &self.dark
    }

    /// Returns a reference to the theme active for `mode`.
    pub fn theme(&self, mode: ThemeMode) -> &Theme {
        match mode {
            ThemeMode::Light => &self.light,
            ThemeMode::Dark => &self.dark,
        }
    }

    /// Replaces the light theme.
    pub fn set_light_theme(&mut self, theme: Theme) {
        self.light = theme;
    }

    /// Replaces the dark theme.
    pub fn set_dark_theme(&mut self, theme: Theme) {
        self.dark = theme;
    }
}

impl Default for ThemeDictionary {
    fn default() -> Self {
        Self::new()
    }
}

/// Constructs the default light theme.
///
/// The light theme uses a near-white background (`Oklab` lightness ≈ `0.96`),
/// dark text (lightness ≈ `0.20`), and a blue primary accent.
pub fn default_light() -> Theme {
    let mut theme = Theme::new("Light");

    // --- Colors -----------------------------------------------------------
    theme.set(
        TokenKey::BackgroundColor,
        ThemeToken::Color(Oklab {
            l: 0.96,
            a: 0.0,
            b: 0.0,
            alpha: 1.0,
        }),
    );
    theme.set(
        TokenKey::SurfaceColor,
        ThemeToken::Color(Oklab {
            l: 0.98,
            a: 0.0,
            b: 0.0,
            alpha: 1.0,
        }),
    );
    theme.set(
        TokenKey::PrimaryColor,
        ThemeToken::Color(Oklab {
            l: 0.55,
            a: -0.08,
            b: -0.10,
            alpha: 1.0,
        }),
    );
    theme.set(
        TokenKey::SecondaryColor,
        ThemeToken::Color(Oklab {
            l: 0.70,
            a: -0.05,
            b: 0.05,
            alpha: 1.0,
        }),
    );
    theme.set(
        TokenKey::AccentColor,
        ThemeToken::Color(Oklab {
            l: 0.75,
            a: 0.15,
            b: -0.05,
            alpha: 1.0,
        }),
    );
    theme.set(
        TokenKey::TextColor,
        ThemeToken::Color(Oklab {
            l: 0.20,
            a: 0.0,
            b: 0.0,
            alpha: 1.0,
        }),
    );
    theme.set(
        TokenKey::TextMutedColor,
        ThemeToken::Color(Oklab {
            l: 0.50,
            a: 0.0,
            b: 0.0,
            alpha: 1.0,
        }),
    );
    theme.set(
        TokenKey::TextInverseColor,
        ThemeToken::Color(Oklab {
            l: 0.96,
            a: 0.0,
            b: 0.0,
            alpha: 1.0,
        }),
    );
    theme.set(
        TokenKey::BorderColor,
        ThemeToken::Color(Oklab {
            l: 0.85,
            a: 0.0,
            b: 0.0,
            alpha: 1.0,
        }),
    );
    theme.set(
        TokenKey::DividerColor,
        ThemeToken::Color(Oklab {
            l: 0.90,
            a: 0.0,
            b: 0.0,
            alpha: 1.0,
        }),
    );
    theme.set(
        TokenKey::ErrorColor,
        ThemeToken::Color(Oklab {
            l: 0.60,
            a: 0.18,
            b: 0.12,
            alpha: 1.0,
        }),
    );
    theme.set(
        TokenKey::WarningColor,
        ThemeToken::Color(Oklab {
            l: 0.78,
            a: 0.10,
            b: 0.13,
            alpha: 1.0,
        }),
    );
    theme.set(
        TokenKey::SuccessColor,
        ThemeToken::Color(Oklab {
            l: 0.70,
            a: -0.18,
            b: 0.12,
            alpha: 1.0,
        }),
    );
    theme.set(
        TokenKey::InfoColor,
        ThemeToken::Color(Oklab {
            l: 0.70,
            a: -0.10,
            b: -0.12,
            alpha: 1.0,
        }),
    );

    // --- Dimensions -------------------------------------------------------
    theme.set(TokenKey::Spacing, ThemeToken::Dimension(16.0));
    theme.set(TokenKey::SpacingSmall, ThemeToken::Dimension(8.0));
    theme.set(TokenKey::SpacingLarge, ThemeToken::Dimension(24.0));
    theme.set(TokenKey::BorderRadius, ThemeToken::Dimension(6.0));
    theme.set(TokenKey::BorderRadiusSmall, ThemeToken::Dimension(3.0));
    theme.set(TokenKey::BorderRadiusLarge, ThemeToken::Dimension(12.0));

    // --- Typography -------------------------------------------------------
    theme.set(TokenKey::FontSizeSmall, ThemeToken::FontSize(12.0));
    theme.set(TokenKey::FontSizeMedium, ThemeToken::FontSize(16.0));
    theme.set(TokenKey::FontSizeLarge, ThemeToken::FontSize(24.0));

    // --- Motion -----------------------------------------------------------
    theme.set(TokenKey::AnimationDuration, ThemeToken::Duration(150.0));
    theme.set(TokenKey::AnimationEasing, ThemeToken::Easing(0.25));

    // --- Platform shell (v0.13.0) -----------------------------------------
    // Backdrop material: platform-conditional defaults.
    //   Windows  → Mica (1) with solid fallback on pre-Win11.
    //   macOS    → Vibrancy (5) with solid fallback on pre-10.10.
    //   Linux    → None (0) — no system material protocol.
    // The shell layer interprets this dimension value:
    // 0 = None, 1 = Mica, 2 = MicaAlt, 3 = Acrylic, 4 = Transient,
    // 5 = Vibrancy.
    #[cfg(target_os = "windows")]
    theme.set(TokenKey::BackdropMaterial, ThemeToken::Dimension(1.0));
    #[cfg(target_os = "macos")]
    theme.set(TokenKey::BackdropMaterial, ThemeToken::Dimension(5.0));
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    theme.set(TokenKey::BackdropMaterial, ThemeToken::Dimension(0.0));
    theme.set(TokenKey::BackdropTintOpacity, ThemeToken::Dimension(0.0));
    theme.set(
        TokenKey::BackdropFallbackColor,
        ThemeToken::Color(Oklab {
            l: 0.96,
            a: 0.0,
            b: 0.0,
            alpha: 1.0,
        }),
    );
    theme.set(TokenKey::CsdTitleBarHeight, ThemeToken::Dimension(32.0));
    theme.set(TokenKey::CsdButtonRadius, ThemeToken::Dimension(6.0));
    theme.set(TokenKey::CsdShadowBlur, ThemeToken::Dimension(20.0));
    theme.set(
        TokenKey::CsdShadowColor,
        ThemeToken::Color(Oklab {
            l: 0.0,
            a: 0.0,
            b: 0.0,
            alpha: 0.3,
        }),
    );
    // VibrancyMaterial: 0 = Sidebar (default on macOS). Not used on
    // Windows/Linux.
    theme.set(TokenKey::VibrancyMaterial, ThemeToken::Dimension(0.0));

    theme
}

/// Constructs the default dark theme.
///
/// The dark theme uses a dark background (`Oklab` lightness ≈ `0.20`), light
/// text (lightness ≈ `0.96`), and a brighter blue primary accent adjusted for
/// the dark surround.
pub fn default_dark() -> Theme {
    let mut theme = Theme::new("Dark");

    // --- Colors -----------------------------------------------------------
    theme.set(
        TokenKey::BackgroundColor,
        ThemeToken::Color(Oklab {
            l: 0.20,
            a: 0.0,
            b: 0.0,
            alpha: 1.0,
        }),
    );
    theme.set(
        TokenKey::SurfaceColor,
        ThemeToken::Color(Oklab {
            l: 0.25,
            a: 0.0,
            b: 0.0,
            alpha: 1.0,
        }),
    );
    theme.set(
        TokenKey::PrimaryColor,
        ThemeToken::Color(Oklab {
            l: 0.65,
            a: -0.08,
            b: -0.10,
            alpha: 1.0,
        }),
    );
    theme.set(
        TokenKey::SecondaryColor,
        ThemeToken::Color(Oklab {
            l: 0.60,
            a: -0.05,
            b: 0.05,
            alpha: 1.0,
        }),
    );
    theme.set(
        TokenKey::AccentColor,
        ThemeToken::Color(Oklab {
            l: 0.75,
            a: 0.15,
            b: -0.05,
            alpha: 1.0,
        }),
    );
    theme.set(
        TokenKey::TextColor,
        ThemeToken::Color(Oklab {
            l: 0.96,
            a: 0.0,
            b: 0.0,
            alpha: 1.0,
        }),
    );
    theme.set(
        TokenKey::TextMutedColor,
        ThemeToken::Color(Oklab {
            l: 0.65,
            a: 0.0,
            b: 0.0,
            alpha: 1.0,
        }),
    );
    theme.set(
        TokenKey::TextInverseColor,
        ThemeToken::Color(Oklab {
            l: 0.20,
            a: 0.0,
            b: 0.0,
            alpha: 1.0,
        }),
    );
    theme.set(
        TokenKey::BorderColor,
        ThemeToken::Color(Oklab {
            l: 0.35,
            a: 0.0,
            b: 0.0,
            alpha: 1.0,
        }),
    );
    theme.set(
        TokenKey::DividerColor,
        ThemeToken::Color(Oklab {
            l: 0.30,
            a: 0.0,
            b: 0.0,
            alpha: 1.0,
        }),
    );
    theme.set(
        TokenKey::ErrorColor,
        ThemeToken::Color(Oklab {
            l: 0.65,
            a: 0.18,
            b: 0.12,
            alpha: 1.0,
        }),
    );
    theme.set(
        TokenKey::WarningColor,
        ThemeToken::Color(Oklab {
            l: 0.78,
            a: 0.10,
            b: 0.13,
            alpha: 1.0,
        }),
    );
    theme.set(
        TokenKey::SuccessColor,
        ThemeToken::Color(Oklab {
            l: 0.72,
            a: -0.18,
            b: 0.12,
            alpha: 1.0,
        }),
    );
    theme.set(
        TokenKey::InfoColor,
        ThemeToken::Color(Oklab {
            l: 0.72,
            a: -0.10,
            b: -0.12,
            alpha: 1.0,
        }),
    );

    // --- Dimensions -------------------------------------------------------
    theme.set(TokenKey::Spacing, ThemeToken::Dimension(16.0));
    theme.set(TokenKey::SpacingSmall, ThemeToken::Dimension(8.0));
    theme.set(TokenKey::SpacingLarge, ThemeToken::Dimension(24.0));
    theme.set(TokenKey::BorderRadius, ThemeToken::Dimension(6.0));
    theme.set(TokenKey::BorderRadiusSmall, ThemeToken::Dimension(3.0));
    theme.set(TokenKey::BorderRadiusLarge, ThemeToken::Dimension(12.0));

    // --- Typography -------------------------------------------------------
    theme.set(TokenKey::FontSizeSmall, ThemeToken::FontSize(12.0));
    theme.set(TokenKey::FontSizeMedium, ThemeToken::FontSize(16.0));
    theme.set(TokenKey::FontSizeLarge, ThemeToken::FontSize(24.0));

    // --- Motion -----------------------------------------------------------
    theme.set(TokenKey::AnimationDuration, ThemeToken::Duration(150.0));
    theme.set(TokenKey::AnimationEasing, ThemeToken::Easing(0.25));

    // --- Platform shell (v0.13.0) -----------------------------------------
    // Backdrop material: platform-conditional defaults (same mapping
    // as the light theme).
    #[cfg(target_os = "windows")]
    theme.set(TokenKey::BackdropMaterial, ThemeToken::Dimension(1.0));
    #[cfg(target_os = "macos")]
    theme.set(TokenKey::BackdropMaterial, ThemeToken::Dimension(5.0));
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    theme.set(TokenKey::BackdropMaterial, ThemeToken::Dimension(0.0));
    theme.set(TokenKey::BackdropTintOpacity, ThemeToken::Dimension(0.0));
    theme.set(
        TokenKey::BackdropFallbackColor,
        ThemeToken::Color(Oklab {
            l: 0.20,
            a: 0.0,
            b: 0.0,
            alpha: 1.0,
        }),
    );
    theme.set(TokenKey::CsdTitleBarHeight, ThemeToken::Dimension(32.0));
    theme.set(TokenKey::CsdButtonRadius, ThemeToken::Dimension(6.0));
    theme.set(TokenKey::CsdShadowBlur, ThemeToken::Dimension(20.0));
    theme.set(
        TokenKey::CsdShadowColor,
        ThemeToken::Color(Oklab {
            l: 0.0,
            a: 0.0,
            b: 0.0,
            alpha: 0.3,
        }),
    );
    theme.set(TokenKey::VibrancyMaterial, ThemeToken::Dimension(0.0));

    theme
}

/// A single color difference recorded between two themes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TokenColorDelta {
    /// The token that changed.
    pub key: TokenKey,
    /// The color in the source theme.
    pub from: Oklab,
    /// The color in the target theme.
    pub to: Oklab,
}

/// The set of color-token differences between two [`Theme`]s.
///
/// `ThemeDiff` is the input to the GPU theme-transition shader: it captures
/// every color token whose value differs between the `from` and `to` themes
/// and exposes an [`interpolate`](ThemeDiff::interpolate) helper that blends
/// them at a parameter `t ∈ [0.0, 1.0]`.
///
/// # Examples
///
/// ```
/// use martensite_theme::{Oklab, Theme, ThemeDiff, ThemeToken, TokenKey};
///
/// let mut a = Theme::new("a");
/// a.set(
///     TokenKey::BackgroundColor,
///     ThemeToken::Color(Oklab { l: 0.0, a: 0.0, b: 0.0, alpha: 1.0 }),
/// );
/// let mut b = Theme::new("b");
/// b.set(
///     TokenKey::BackgroundColor,
///     ThemeToken::Color(Oklab { l: 1.0, a: 0.0, b: 0.0, alpha: 1.0 }),
/// );
/// let diff = ThemeDiff::from_themes(&a, &b);
/// let at_start = diff.interpolate(0.0);
/// assert_eq!(at_start[0].1.l, 0.0);
/// ```
#[derive(Clone, Debug)]
pub struct ThemeDiff {
    deltas: Vec<TokenColorDelta>,
}

impl ThemeDiff {
    /// Builds a diff by comparing the color tokens of `from` and `to`.
    ///
    /// A delta is recorded for every color token that exists in both themes
    /// and has a different [`Oklab`] value. Tokens present in only one theme
    /// are ignored, since the transition shader only blends tokens that have a
    /// well-defined endpoint in both themes.
    pub fn from_themes(from: &Theme, to: &Theme) -> Self {
        let mut deltas = Vec::new();
        for (key, from_token) in &from.tokens {
            let from_color = match from_token {
                ThemeToken::Color(c) => *c,
                _ => continue,
            };
            let to_color = match to.tokens.get(key) {
                Some(ThemeToken::Color(c)) => *c,
                _ => continue,
            };
            if from_color != to_color {
                deltas.push(TokenColorDelta {
                    key: *key,
                    from: from_color,
                    to: to_color,
                });
            }
        }
        // Stable ordering keyed by the discriminant so the diff is
        // deterministic regardless of HashMap iteration order.
        deltas.sort_by_key(|d| d.key as usize);
        Self { deltas }
    }

    /// Returns the recorded color deltas.
    pub fn deltas(&self) -> &[TokenColorDelta] {
        &self.deltas
    }

    /// Interpolates every recorded color delta at parameter `t`.
    ///
    /// `t = 0.0` yields the `from` colors, `t = 1.0` yields the `to` colors,
    /// and intermediate values produce a perceptually uniform Oklab blend.
    ///
    /// This method allocates a `Vec`. For zero-allocation, per-frame use, prefer
    /// [`ThemeDiff::interpolate_into`] which writes into a caller-provided
    /// buffer.
    pub fn interpolate(&self, t: f32) -> Vec<(TokenKey, Oklab)> {
        self.deltas
            .iter()
            .map(|d| (d.key, d.from.lerp(d.to, t)))
            .collect()
    }

    /// Interpolates every recorded color delta at parameter `t` into the
    /// provided `output` buffer, returning the number of entries written.
    ///
    /// This is the zero-allocation variant of [`ThemeDiff::interpolate`],
    /// suitable for per-frame use in the animation loop. The buffer is cleared
    /// before writing. If the buffer is smaller than the number of deltas, only
    /// as many entries as fit are written.
    ///
    /// `t = 0.0` yields the `from` colors, `t = 1.0` yields the `to` colors,
    /// and intermediate values produce a perceptually uniform Oklab blend.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_theme::{Oklab, Theme, ThemeDiff, ThemeToken, TokenKey};
    ///
    /// let mut a = Theme::new("a");
    /// a.set(
    ///     TokenKey::BackgroundColor,
    ///     ThemeToken::Color(Oklab { l: 0.0, a: 0.0, b: 0.0, alpha: 1.0 }),
    /// );
    /// let mut b = Theme::new("b");
    /// b.set(
    ///     TokenKey::BackgroundColor,
    ///     ThemeToken::Color(Oklab { l: 1.0, a: 0.0, b: 0.0, alpha: 1.0 }),
    /// );
    /// let diff = ThemeDiff::from_themes(&a, &b);
    ///
    /// let mut buf = Vec::with_capacity(1);
    /// let count = diff.interpolate_into(0.5, &mut buf);
    /// assert_eq!(count, 1);
    /// assert_eq!(buf[0].1.l, 0.5);
    /// ```
    pub fn interpolate_into(&self, t: f32, output: &mut Vec<(TokenKey, Oklab)>) -> usize {
        output.clear();
        let count = self.deltas.len().min(output.capacity());
        for i in 0..count {
            let d = &self.deltas[i];
            output.push((d.key, d.from.lerp(d.to, t)));
        }
        count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx_eq(a: f32, b: f32) -> bool {
        (a - b).abs() < 1.0e-5
    }

    fn oklab_approx_eq(a: Oklab, b: Oklab) -> bool {
        approx_eq(a.l, b.l)
            && approx_eq(a.a, b.a)
            && approx_eq(a.b, b.b)
            && approx_eq(a.alpha, b.alpha)
    }

    #[test]
    fn default_light_has_white_background_and_dark_text() {
        let theme = default_light();
        let bg = theme.color(TokenKey::BackgroundColor).unwrap();
        assert!(approx_eq(bg.l, 0.96));
        assert!(approx_eq(bg.alpha, 1.0));
        let text = theme.color(TokenKey::TextColor).unwrap();
        assert!(approx_eq(text.l, 0.20));
        assert_eq!(theme.name, "Light");
    }

    #[test]
    fn default_dark_has_dark_background_and_light_text() {
        let theme = default_dark();
        let bg = theme.color(TokenKey::BackgroundColor).unwrap();
        assert!(approx_eq(bg.l, 0.20));
        assert!(approx_eq(bg.alpha, 1.0));
        let text = theme.color(TokenKey::TextColor).unwrap();
        assert!(approx_eq(text.l, 0.96));
        assert_eq!(theme.name, "Dark");
    }

    #[test]
    fn theme_get_set_all_token_types() {
        let mut theme = Theme::new("test");

        let color = Oklab {
            l: 0.5,
            a: 0.1,
            b: -0.1,
            alpha: 1.0,
        };
        theme.set(TokenKey::PrimaryColor, ThemeToken::Color(color));
        assert_eq!(theme.color(TokenKey::PrimaryColor), Some(color));
        assert_eq!(theme.get_color(TokenKey::PrimaryColor), Some(color));

        theme.set(TokenKey::Spacing, ThemeToken::Dimension(32.0));
        assert_eq!(theme.dimension(TokenKey::Spacing), Some(32.0));
        assert_eq!(theme.get_dimension(TokenKey::Spacing), Some(32.0));

        theme.set(TokenKey::FontSizeMedium, ThemeToken::FontSize(18.0));
        assert_eq!(theme.dimension(TokenKey::FontSizeMedium), Some(18.0));
        assert_eq!(theme.get_dimension(TokenKey::FontSizeMedium), Some(18.0));

        theme.set(TokenKey::AnimationDuration, ThemeToken::Duration(250.0));
        assert_eq!(theme.dimension(TokenKey::AnimationDuration), Some(250.0));
        assert_eq!(
            theme.get_dimension(TokenKey::AnimationDuration),
            Some(250.0)
        );

        theme.set(TokenKey::AnimationEasing, ThemeToken::Easing(0.42));
        assert_eq!(theme.dimension(TokenKey::AnimationEasing), Some(0.42));
        assert_eq!(theme.get_dimension(TokenKey::AnimationEasing), Some(0.42));

        theme.set(
            TokenKey::FontSizeLarge,
            ThemeToken::FontFamily("Inter".to_string()),
        );
        assert!(matches!(
            theme.get(TokenKey::FontSizeLarge),
            Some(ThemeToken::FontFamily(f)) if f == "Inter"
        ));
    }

    #[test]
    fn get_color_returns_none_for_non_color_token() {
        let mut theme = Theme::new("test");
        theme.set(TokenKey::Spacing, ThemeToken::Dimension(16.0));
        assert_eq!(theme.color(TokenKey::Spacing), None);
        assert_eq!(theme.get_color(TokenKey::Spacing), None);
    }

    #[test]
    fn get_dimension_returns_none_for_color_token() {
        let mut theme = Theme::new("test");
        theme.set(
            TokenKey::PrimaryColor,
            ThemeToken::Color(Oklab {
                l: 0.5,
                a: 0.0,
                b: 0.0,
                alpha: 1.0,
            }),
        );
        assert_eq!(theme.dimension(TokenKey::PrimaryColor), None);
        assert_eq!(theme.get_dimension(TokenKey::PrimaryColor), None);
    }

    #[test]
    fn get_returns_none_for_missing_key() {
        let theme = Theme::new("empty");
        assert!(theme.get(TokenKey::PrimaryColor).is_none());
        assert!(theme.color(TokenKey::PrimaryColor).is_none());
        assert!(theme.get_color(TokenKey::PrimaryColor).is_none());
        assert!(theme.dimension(TokenKey::Spacing).is_none());
        assert!(theme.get_dimension(TokenKey::Spacing).is_none());
    }

    #[test]
    fn theme_merge_other_overrides() {
        let mut base = default_light();
        let mut override_theme = Theme::new("override");

        let new_bg = Oklab {
            l: 0.42,
            a: 0.0,
            b: 0.0,
            alpha: 1.0,
        };
        override_theme.set(TokenKey::BackgroundColor, ThemeToken::Color(new_bg));
        override_theme.set(TokenKey::Spacing, ThemeToken::Dimension(99.0));

        base.merge(&override_theme);

        assert_eq!(base.color(TokenKey::BackgroundColor), Some(new_bg));
        assert_eq!(base.get_color(TokenKey::BackgroundColor), Some(new_bg));
        assert_eq!(base.dimension(TokenKey::Spacing), Some(99.0));
        assert_eq!(base.get_dimension(TokenKey::Spacing), Some(99.0));
        // Untouched tokens remain.
        assert!(base.color(TokenKey::TextColor).is_some());
    }

    #[test]
    fn theme_dictionary_returns_correct_theme_per_mode() {
        let dict = ThemeDictionary::new();
        assert_eq!(dict.light_theme().name, "Light");
        assert_eq!(dict.dark_theme().name, "Dark");
        assert_eq!(dict.theme(ThemeMode::Light).name, "Light");
        assert_eq!(dict.theme(ThemeMode::Dark).name, "Dark");
    }

    #[test]
    fn theme_dictionary_setters_replace_themes() {
        let mut dict = ThemeDictionary::new();
        let mut custom_light = Theme::new("CustomLight");
        custom_light.set(
            TokenKey::BackgroundColor,
            ThemeToken::Color(Oklab {
                l: 0.97,
                a: 0.0,
                b: 0.0,
                alpha: 1.0,
            }),
        );
        let mut custom_dark = Theme::new("CustomDark");
        custom_dark.set(
            TokenKey::BackgroundColor,
            ThemeToken::Color(Oklab {
                l: 0.18,
                a: 0.0,
                b: 0.0,
                alpha: 1.0,
            }),
        );
        dict.set_light_theme(custom_light);
        dict.set_dark_theme(custom_dark);
        assert_eq!(dict.light_theme().name, "CustomLight");
        assert_eq!(dict.dark_theme().name, "CustomDark");
    }

    #[test]
    fn theme_diff_detects_color_differences() {
        let light = default_light();
        let dark = default_dark();
        let diff = ThemeDiff::from_themes(&light, &dark);

        // Background and text colors differ between light and dark.
        let keys: Vec<TokenKey> = diff.deltas().iter().map(|d| d.key).collect();
        assert!(keys.contains(&TokenKey::BackgroundColor));
        assert!(keys.contains(&TokenKey::TextColor));

        for d in diff.deltas() {
            assert_ne!(d.from, d.to);
        }
    }

    #[test]
    fn theme_diff_ignores_unchanged_and_non_color_tokens() {
        let light = default_light();
        let mut twin = Theme::new("twin");
        twin.merge(&light);
        // Spacing is identical -> not a delta.
        let diff = ThemeDiff::from_themes(&light, &twin);
        assert!(diff.deltas().is_empty());
    }

    #[test]
    fn theme_diff_interpolate_at_zero_returns_from_colors() {
        let light = default_light();
        let dark = default_dark();
        let diff = ThemeDiff::from_themes(&light, &dark);

        let at_zero = diff.interpolate(0.0);
        assert_eq!(at_zero.len(), diff.deltas().len());
        for (i, (key, color)) in at_zero.iter().enumerate() {
            let d = &diff.deltas()[i];
            assert_eq!(*key, d.key);
            assert!(oklab_approx_eq(*color, d.from));
        }
    }

    #[test]
    fn theme_diff_interpolate_at_one_returns_to_colors() {
        let light = default_light();
        let dark = default_dark();
        let diff = ThemeDiff::from_themes(&light, &dark);

        let at_one = diff.interpolate(1.0);
        assert_eq!(at_one.len(), diff.deltas().len());
        for (i, (key, color)) in at_one.iter().enumerate() {
            let d = &diff.deltas()[i];
            assert_eq!(*key, d.key);
            assert!(oklab_approx_eq(*color, d.to));
        }
    }

    #[test]
    fn theme_diff_interpolate_at_half_returns_midpoint() {
        let mut from = Theme::new("from");
        from.set(
            TokenKey::BackgroundColor,
            ThemeToken::Color(Oklab {
                l: 0.0,
                a: 0.0,
                b: 0.0,
                alpha: 1.0,
            }),
        );
        let mut to = Theme::new("to");
        to.set(
            TokenKey::BackgroundColor,
            ThemeToken::Color(Oklab {
                l: 1.0,
                a: 0.0,
                b: 0.0,
                alpha: 1.0,
            }),
        );
        let diff = ThemeDiff::from_themes(&from, &to);
        let at_half = diff.interpolate(0.5);
        assert_eq!(at_half.len(), 1);
        assert!(oklab_approx_eq(
            at_half[0].1,
            Oklab {
                l: 0.5,
                a: 0.0,
                b: 0.0,
                alpha: 1.0,
            }
        ));
    }

    #[test]
    fn token_key_is_copy_and_hash_compatible() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        let key = TokenKey::PrimaryColor;
        set.insert(key);
        // Copy: insert again without moving.
        assert!(set.contains(&key));
        assert!(set.contains(&TokenKey::PrimaryColor));
        assert!(!set.contains(&TokenKey::SecondaryColor));
    }

    #[test]
    fn theme_dictionary_default_matches_new() {
        let new = ThemeDictionary::new();
        let default = ThemeDictionary::default();
        assert_eq!(new.light_theme().name, default.light_theme().name);
        assert_eq!(new.dark_theme().name, default.dark_theme().name);
    }

    #[test]
    fn theme_diff_interpolate_into_is_zero_allocation_compatible() {
        let light = default_light();
        let dark = default_dark();
        let diff = ThemeDiff::from_themes(&light, &dark);

        // Pre-allocate a buffer with enough capacity.
        let mut buf = Vec::with_capacity(diff.deltas().len());
        let count = diff.interpolate_into(0.5, &mut buf);
        assert_eq!(count, diff.deltas().len());
        assert_eq!(buf.len(), diff.deltas().len());

        // Verify the midpoint values are correct.
        for (i, (key, color)) in buf.iter().enumerate() {
            let d = &diff.deltas()[i];
            assert_eq!(*key, d.key);
            assert!(oklab_approx_eq(*color, d.from.lerp(d.to, 0.5)));
        }

        // Calling again should clear and refill without growing capacity.
        let cap_before = buf.capacity();
        let _ = diff.interpolate_into(0.0, &mut buf);
        assert_eq!(
            buf.capacity(),
            cap_before,
            "interpolate_into must not grow capacity"
        );
    }

    #[test]
    fn theme_diff_interpolate_into_handles_small_buffer() {
        let light = default_light();
        let dark = default_dark();
        let diff = ThemeDiff::from_themes(&light, &dark);

        // Buffer with capacity 0 — should write 0 entries.
        let mut buf: Vec<(TokenKey, Oklab)> = Vec::with_capacity(0);
        let count = diff.interpolate_into(0.5, &mut buf);
        assert_eq!(count, 0);
        assert_eq!(buf.len(), 0);
    }
}
