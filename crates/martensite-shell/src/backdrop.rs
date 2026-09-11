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
/// use martensite_shell::{BackdropController, BackdropMaterial, BackdropMode, StubBackdropController};
///
/// let mut controller = StubBackdropController::new();
/// controller.set_material(BackdropMaterial::Mica);
/// // Stub always reports None since it has no system material support.
/// assert_eq!(controller.current_material(), BackdropMaterial::None);
/// assert_eq!(controller.mode(), BackdropMode::Opaque);
/// ```
pub trait BackdropController {
    /// Sets the requested backdrop material on the window.
    fn set_material(&mut self, material: BackdropMaterial);
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
    fn set_material(&mut self, _material: BackdropMaterial) {
        // Stub: ignore the request, always report None.
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
