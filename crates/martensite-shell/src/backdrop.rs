//! Cross-platform system backdrop material abstraction.
//!
//! This module defines the types and traits that platform backends implement
//! to apply system backdrop materials (Mica, Acrylic, vibrancy) to windows.
//! The [`BackdropController`] trait is the integration point; the
//! [`StubBackdropController`] provides a no-op implementation for platforms
//! without system material support.

/// System backdrop material types for window composition.
///
/// # Examples
///
/// ```
/// use martensite_shell::BackdropMaterial;
///
/// let mica = BackdropMaterial::Mica;
/// assert!(!mica.is_transparent());
/// assert!(!BackdropMaterial::None.is_transparent());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum BackdropMaterial {
    /// No system material — solid color background.
    None,
    /// Windows 11 Mica (subtle desktop wallpaper tint).
    Mica,
    /// Windows 11 Mica Alt (more saturated tint, for secondary windows).
    MicaAlt,
    /// Windows 11 / macOS Acrylic (GPU blur behind window).
    Acrylic,
    /// Windows 11 Transient (temporary acrylic, e.g. flyouts).
    Transient,
    /// macOS vibrancy material (Liquid Glass on macOS 26+, NSVisualEffectView on older).
    Vibrancy(VibrancyMaterial),
}

impl BackdropMaterial {
    /// Returns `true` if this material requires a transparent surface.
    ///
    /// Transparent materials (`Acrylic`, `Transient`, and `Vibrancy`) need a
    /// premultiplied-alpha swapchain so the system material shows through.
    /// Opaque materials (`None`, `Mica`, `MicaAlt`) use a solid surface.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_shell::BackdropMaterial;
    ///
    /// assert!(!BackdropMaterial::Mica.is_transparent());
    /// assert!(BackdropMaterial::Acrylic.is_transparent());
    /// assert!(BackdropMaterial::Transient.is_transparent());
    /// assert!(!BackdropMaterial::None.is_transparent());
    /// ```
    #[must_use]
    pub fn is_transparent(&self) -> bool {
        matches!(self, Self::Acrylic | Self::Transient | Self::Vibrancy(_))
    }
}

/// macOS-specific vibrancy material selection.
///
/// # Examples
///
/// ```
/// use martensite_shell::VibrancyMaterial;
///
/// let material = VibrancyMaterial::Sidebar;
/// assert_eq!(material.label(), "sidebar");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum VibrancyMaterial {
    /// Sidebar-style vibrancy (translucent sidebar).
    Sidebar,
    /// HUD-style vibrancy (heads-up display panel).
    HudWindow,
    /// Full-screen UI vibrancy.
    FullScreenUI,
    /// Sheet-style vibrancy (modal sheets).
    Sheet,
    /// Titlebar vibrancy.
    Titlebar,
    /// Menu vibrancy.
    Menu,
    /// Popover vibrancy.
    Popover,
    /// Tooltip vibrancy.
    Tooltip,
    /// Liquid Glass (macOS 26+).
    LiquidGlass,
}

impl VibrancyMaterial {
    /// Returns a lowercase stable string label identifying this vibrancy material.
    ///
    /// The label is suitable for logging, diagnostics, and configuration
    /// serialization. It is stable across releases.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_shell::VibrancyMaterial;
    ///
    /// assert_eq!(VibrancyMaterial::HudWindow.label(), "hud-window");
    /// assert_eq!(VibrancyMaterial::FullScreenUI.label(), "full-screen-ui");
    /// assert_eq!(VibrancyMaterial::LiquidGlass.label(), "liquid-glass");
    /// ```
    #[must_use]
    pub fn label(&self) -> &'static str {
        match self {
            Self::Sidebar => "sidebar",
            Self::HudWindow => "hud-window",
            Self::FullScreenUI => "full-screen-ui",
            Self::Sheet => "sheet",
            Self::Titlebar => "titlebar",
            Self::Menu => "menu",
            Self::Popover => "popover",
            Self::Tooltip => "tooltip",
            Self::LiquidGlass => "liquid-glass",
        }
    }
}

/// Surface alpha mode for GPU swapchain configuration.
///
/// When configuring a `martensite_wgpu` surface, map `Opaque` to
/// `CompositeAlphaMode::Opaque` and `Transparent` to
/// `CompositeAlphaMode::PreMultiplied`.
///
/// # Examples
///
/// ```
/// use martensite_shell::BackdropMode;
///
/// let opaque = BackdropMode::Opaque;
/// let transparent = BackdropMode::Transparent;
/// assert_ne!(opaque, transparent);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum BackdropMode {
    /// Opaque surface — solid background, `CompositeAlphaMode::Opaque`.
    Opaque,
    /// Transparent surface — system material shows through, `CompositeAlphaMode::PreMultiplied`.
    Transparent,
}

/// Platform-agnostic window handle abstraction.
///
/// Platform backends implement this to expose the raw native window
/// handle (e.g. `HWND` on Windows, `NSView*` on macOS) that backdrop
/// controllers require in order to apply system materials to a specific
/// window. The handle is returned as an opaque `*mut c_void` pointer so
/// the core trait stays free of platform-specific types.
///
/// # Examples
///
/// ```
/// use martensite_shell::Window;
/// use core::ffi::c_void;
///
/// struct TestWindow;
/// impl Window for TestWindow {
///     unsafe fn raw_handle(&self) -> *mut c_void {
///         core::ptr::null_mut()
///     }
/// }
/// ```
pub trait Window {
    /// Returns the raw native window handle.
    ///
    /// # Safety
    ///
    /// The returned pointer must be a valid, non-null native window handle
    /// (HWND on Windows, NSWindow* on macOS) for the duration of the
    /// `BackdropController` call that consumes it. Callers must ensure the
    /// window is still alive when platform backends dereference this handle.
    #[allow(unsafe_code)]
    unsafe fn raw_handle(&self) -> *mut core::ffi::c_void;
}

/// Controller for managing system backdrop materials on a window.
///
/// Implementations are platform-specific:
/// - Windows: DWM Mica/Acrylic via `DwmSetWindowAttribute`.
/// - macOS: NSVisualEffectView / Liquid Glass.
/// - Linux/Wayland: Stub (returns `BackdropMaterial::None`).
///
/// # Examples
///
/// ```
/// use martensite_shell::{
///     BackdropController, BackdropMaterial, BackdropMode, StubBackdropController, Window,
/// };
/// use core::ffi::c_void;
///
/// struct TestWindow;
/// impl Window for TestWindow {
///     unsafe fn raw_handle(&self) -> *mut c_void { core::ptr::null_mut() }
/// }
///
/// let mut controller = StubBackdropController::new();
/// controller.set_material(&TestWindow, BackdropMaterial::Mica);
/// // Stub always reports None since it has no system material support.
/// assert_eq!(controller.current_material(), BackdropMaterial::None);
/// assert_eq!(controller.mode(), BackdropMode::Opaque);
/// ```
pub trait BackdropController {
    /// Sets the requested backdrop material on the given window.
    fn set_material(&mut self, window: &dyn Window, material: BackdropMaterial);
    /// Returns the currently active backdrop material.
    fn current_material(&self) -> BackdropMaterial;
    /// Returns the surface alpha mode required by the current material.
    fn mode(&self) -> BackdropMode;
    /// Returns true if the platform supports the given material.
    fn supports_material(&self, material: BackdropMaterial) -> bool;
}

/// Stub backdrop controller for platforms without system material support.
///
/// Always returns `BackdropMaterial::None` and `BackdropMode::Opaque`.
///
/// # Examples
///
/// ```
/// use martensite_shell::{StubBackdropController, BackdropController, BackdropMaterial};
///
/// let controller = StubBackdropController::new();
/// assert!(!controller.supports_material(BackdropMaterial::Mica));
/// assert_eq!(controller.current_material(), BackdropMaterial::None);
/// ```
#[derive(Debug, Clone, Default)]
pub struct StubBackdropController;

impl StubBackdropController {
    /// Creates a new stub backdrop controller.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_shell::StubBackdropController;
    ///
    /// let controller = StubBackdropController::new();
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl BackdropController for StubBackdropController {
    #[allow(unsafe_code)]
    fn set_material(&mut self, _window: &dyn Window, material: BackdropMaterial) {
        // Safety: stub does not dereference the handle.
        let _ = unsafe { _window.raw_handle() };
        // Stub: ignore the request, always report None.
        let _ = material;
    }
    fn current_material(&self) -> BackdropMaterial {
        BackdropMaterial::None
    }
    fn mode(&self) -> BackdropMode {
        BackdropMode::Opaque
    }
    fn supports_material(&self, _material: BackdropMaterial) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// Theme token resolution
// ---------------------------------------------------------------------------
//
// The v0.13.0 spec requires `martensite-shell` to read the 8 shell-related
// theme tokens from a `martensite_theme::Theme` and convert them into the
// strongly-typed enums defined in this module. The theme stores material
// selections as `ThemeToken::Dimension(f32)` with a documented numeric
// encoding; the resolver converts them to `BackdropMaterial` /
// `VibrancyMaterial` enum values.

use martensite_theme::{Oklab, Theme, TokenKey};

/// Resolves the [`BackdropMaterial`] enum from a theme token value.
///
/// The theme stores `BackdropMaterial` as a `ThemeToken::Dimension(f32)`
/// with the encoding:
/// - `0.0` → [`BackdropMaterial::None`]
/// - `1.0` → [`BackdropMaterial::Mica`]
/// - `2.0` → [`BackdropMaterial::MicaAlt`]
/// - `3.0` → [`BackdropMaterial::Acrylic`]
/// - `4.0` → [`BackdropMaterial::Transient`]
/// - `5.0` → [`BackdropMaterial::Vibrancy`] (uses the `VibrancyMaterial`
///   from [`resolve_vibrancy_material`])
///
/// Any other value (including missing tokens) falls back to
/// [`BackdropMaterial::None`].
///
/// # Examples
///
/// ```
/// use martensite_shell::backdrop::resolve_backdrop_material;
/// use martensite_shell::BackdropMaterial;
/// use martensite_theme::{Theme, ThemeToken, TokenKey};
///
/// let mut theme = Theme::new("Test");
/// theme.set(TokenKey::BackdropMaterial, ThemeToken::Dimension(1.0));
/// assert_eq!(resolve_backdrop_material(&theme), BackdropMaterial::Mica);
///
/// theme.set(TokenKey::BackdropMaterial, ThemeToken::Dimension(5.0));
/// // When the material is Vibrancy, the vibrancy sub-material is
/// // resolved from VibrancyMaterial (defaults to Sidebar).
/// let material = resolve_backdrop_material(&theme);
/// assert!(matches!(material, BackdropMaterial::Vibrancy(_)));
/// ```
#[must_use]
pub fn resolve_backdrop_material(theme: &Theme) -> BackdropMaterial {
    let dim = match theme.dimension(TokenKey::BackdropMaterial) {
        Some(v) => v as i32,
        None => return BackdropMaterial::None,
    };
    match dim {
        1 => BackdropMaterial::Mica,
        2 => BackdropMaterial::MicaAlt,
        3 => BackdropMaterial::Acrylic,
        4 => BackdropMaterial::Transient,
        5 => BackdropMaterial::Vibrancy(resolve_vibrancy_material(theme)),
        _ => BackdropMaterial::None,
    }
}

/// Resolves the [`VibrancyMaterial`] enum from a theme token value.
///
/// The theme stores `VibrancyMaterial` as a `ThemeToken::Dimension(f32)`
/// with the encoding:
/// - `0.0` → [`VibrancyMaterial::Sidebar`]
/// - `1.0` → [`VibrancyMaterial::HudWindow`]
/// - `2.0` → [`VibrancyMaterial::FullScreenUI`]
/// - `3.0` → [`VibrancyMaterial::Sheet`]
/// - `4.0` → [`VibrancyMaterial::Titlebar`]
/// - `5.0` → [`VibrancyMaterial::Menu`]
/// - `6.0` → [`VibrancyMaterial::Popover`]
/// - `7.0` → [`VibrancyMaterial::Tooltip`]
/// - `8.0` → [`VibrancyMaterial::LiquidGlass`]
///
/// Any other value (including missing tokens) falls back to
/// [`VibrancyMaterial::Sidebar`].
///
/// # Examples
///
/// ```
/// use martensite_shell::backdrop::resolve_vibrancy_material;
/// use martensite_shell::VibrancyMaterial;
/// use martensite_theme::{Theme, ThemeToken, TokenKey};
///
/// let mut theme = Theme::new("Test");
/// theme.set(TokenKey::VibrancyMaterial, ThemeToken::Dimension(8.0));
/// assert_eq!(resolve_vibrancy_material(&theme), VibrancyMaterial::LiquidGlass);
///
/// // Missing token falls back to Sidebar.
/// let empty = Theme::new("Empty");
/// assert_eq!(resolve_vibrancy_material(&empty), VibrancyMaterial::Sidebar);
/// ```
#[must_use]
pub fn resolve_vibrancy_material(theme: &Theme) -> VibrancyMaterial {
    let dim = match theme.dimension(TokenKey::VibrancyMaterial) {
        Some(v) => v as i32,
        None => return VibrancyMaterial::Sidebar,
    };
    match dim {
        0 => VibrancyMaterial::Sidebar,
        1 => VibrancyMaterial::HudWindow,
        2 => VibrancyMaterial::FullScreenUI,
        3 => VibrancyMaterial::Sheet,
        4 => VibrancyMaterial::Titlebar,
        5 => VibrancyMaterial::Menu,
        6 => VibrancyMaterial::Popover,
        7 => VibrancyMaterial::Tooltip,
        8 => VibrancyMaterial::LiquidGlass,
        _ => VibrancyMaterial::Sidebar,
    }
}

/// Resolved shell theme tokens for a [`Theme`].
///
/// This struct reads all 8 v0.13.0 shell-related theme tokens at once,
/// converting them to strongly-typed values. It is the single entry
/// point for platform backends that need to apply theme-driven shell
/// configuration.
///
/// # Examples
///
/// ```
/// use martensite_shell::backdrop::ShellThemeTokens;
/// use martensite_shell::BackdropMaterial;
/// use martensite_theme::ThemeDictionary;
///
/// let dict = ThemeDictionary::new();
/// let light = dict.light_theme();
/// let tokens = ShellThemeTokens::from_theme(light);
/// // All 8 tokens resolve; the material depends on the platform
/// // (Mica on Windows, Vibrancy on macOS, None on Linux).
/// let _material = tokens.backdrop_material;
/// let _opacity = tokens.backdrop_tint_opacity;
/// let _fallback = tokens.backdrop_fallback_color;
/// let _title_bar = tokens.csd_title_bar_height;
/// let _radius = tokens.csd_button_radius;
/// let _blur = tokens.csd_shadow_blur;
/// let _shadow = tokens.csd_shadow_color;
/// let _vibrancy = tokens.vibrancy_material;
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct ShellThemeTokens {
    /// The resolved backdrop material (Mica, Acrylic, Vibrancy, etc.).
    pub backdrop_material: BackdropMaterial,
    /// The backdrop tint opacity (0.0–1.0).
    pub backdrop_tint_opacity: f32,
    /// The fallback background color when system materials are
    /// unavailable.
    pub backdrop_fallback_color: Oklab,
    /// The CSD title bar height in physical pixels.
    pub csd_title_bar_height: f32,
    /// The CSD window button corner radius in physical pixels.
    pub csd_button_radius: f32,
    /// The CSD shadow blur radius in physical pixels.
    pub csd_shadow_blur: f32,
    /// The CSD shadow color.
    pub csd_shadow_color: Oklab,
    /// The macOS vibrancy material selection.
    pub vibrancy_material: VibrancyMaterial,
}

impl ShellThemeTokens {
    /// Reads all 8 shell-related theme tokens from a [`Theme`].
    ///
    /// Missing tokens fall back to platform-appropriate defaults:
    /// - `BackdropMaterial` → [`BackdropMaterial::None`]
    /// - `VibrancyMaterial` → [`VibrancyMaterial::Sidebar`]
    /// - `BackdropTintOpacity` → `0.0`
    /// - `BackdropFallbackColor` → the theme's `BackgroundColor`
    ///   (or opaque white if absent)
    /// - CSD dimensions → `32.0` / `6.0` / `20.0`
    /// - `CsdShadowColor` → opaque black at 30% alpha
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_shell::backdrop::ShellThemeTokens;
    /// use martensite_theme::ThemeDictionary;
    ///
    /// let dict = ThemeDictionary::new();
    /// let tokens = ShellThemeTokens::from_theme(dict.light_theme());
    /// assert!(tokens.csd_title_bar_height > 0.0);
    /// ```
    #[must_use]
    pub fn from_theme(theme: &Theme) -> Self {
        Self {
            backdrop_material: resolve_backdrop_material(theme),
            backdrop_tint_opacity: theme
                .dimension(TokenKey::BackdropTintOpacity)
                .unwrap_or(0.0),
            backdrop_fallback_color: theme.color(TokenKey::BackdropFallbackColor).unwrap_or(
                Oklab {
                    l: 0.96,
                    a: 0.0,
                    b: 0.0,
                    alpha: 1.0,
                },
            ),
            csd_title_bar_height: theme.dimension(TokenKey::CsdTitleBarHeight).unwrap_or(32.0),
            csd_button_radius: theme.dimension(TokenKey::CsdButtonRadius).unwrap_or(6.0),
            csd_shadow_blur: theme.dimension(TokenKey::CsdShadowBlur).unwrap_or(20.0),
            csd_shadow_color: theme.color(TokenKey::CsdShadowColor).unwrap_or(Oklab {
                l: 0.0,
                a: 0.0,
                b: 0.0,
                alpha: 0.3,
            }),
            vibrancy_material: resolve_vibrancy_material(theme),
        }
    }
}
