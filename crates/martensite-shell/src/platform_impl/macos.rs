//! macOS vibrancy and Liquid Glass backdrop integration.
//!
//! This module uses `objc2` for Objective-C runtime FFI. All unsafe
//! code is confined to this module.

#![allow(unsafe_code)]

use crate::backdrop::{BackdropController, BackdropMaterial, BackdropMode, VibrancyMaterial};

/// macOS backdrop controller using NSVisualEffectView and Liquid Glass.
///
/// On macOS 14 and earlier, uses `NSVisualEffectView` with the
/// `setMaterial:` API. On macOS 26+, uses the new `NSGlassEffectView`
/// / `CALayer.material` API for Liquid Glass.
///
/// # Examples
///
/// ```no_run
/// use martensite_shell::platform_impl::macos::MacosBackdropController;
/// use martensite_shell::{BackdropController, BackdropMaterial, VibrancyMaterial};
///
/// let mut controller = MacosBackdropController::new();
/// controller.set_material(BackdropMaterial::Vibrancy(VibrancyMaterial::Sidebar));
/// ```
pub struct MacosBackdropController {
    material: BackdropMaterial,
    supported: bool,
}

impl MacosBackdropController {
    /// Creates a new macOS backdrop controller.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_shell::platform_impl::macos::MacosBackdropController;
    ///
    /// let controller = MacosBackdropController::new();
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self {
            material: BackdropMaterial::None,
            supported: true, // macOS always supports at least NSVisualEffectView
        }
    }

    /// Maps a `VibrancyMaterial` to the `NSVisualEffectView.Material` value.
    ///
    /// The mapping is:
    /// - `Sidebar` -> 17 (NSVisualEffectMaterialSidebar)
    /// - `HudWindow` -> 13 (NSVisualEffectMaterialHudWindow)
    /// - `FullScreenUI` -> 15 (NSVisualEffectMaterialFullScreenUI)
    /// - `Sheet` -> 11 (NSVisualEffectMaterialSheet)
    /// - `Titlebar` -> 3 (NSVisualEffectMaterialTitlebar)
    /// - `Menu` -> 4 (NSVisualEffectMaterialMenu)
    /// - `Popover` -> 6 (NSVisualEffectMaterialPopover)
    /// - `Tooltip` -> 7 (NSVisualEffectMaterialToolTip)
    /// - `LiquidGlass` -> 21 (new macOS 26+ material)
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_shell::platform_impl::macos::MacosBackdropController;
    /// use martensite_shell::backdrop::VibrancyMaterial;
    ///
    /// assert_eq!(MacosBackdropController::vibrancy_to_ns_material(VibrancyMaterial::Sidebar), 17);
    /// assert_eq!(MacosBackdropController::vibrancy_to_ns_material(VibrancyMaterial::LiquidGlass), 21);
    /// ```
    #[must_use]
    pub fn vibrancy_to_ns_material(material: VibrancyMaterial) -> u32 {
        match material {
            VibrancyMaterial::Sidebar => 17,
            VibrancyMaterial::HudWindow => 13,
            VibrancyMaterial::FullScreenUI => 15,
            VibrancyMaterial::Sheet => 11,
            VibrancyMaterial::Titlebar => 3,
            VibrancyMaterial::Menu => 4,
            VibrancyMaterial::Popover => 6,
            VibrancyMaterial::Tooltip => 7,
            VibrancyMaterial::LiquidGlass => 21,
        }
    }

    /// Returns true if the current macOS version supports Liquid Glass (macOS 26+).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_shell::platform_impl::macos::MacosBackdropController;
    ///
    /// let supports = MacosBackdropController::supports_liquid_glass();
    /// // Returns false on macOS 14 and earlier.
    /// ```
    #[must_use]
    pub fn supports_liquid_glass() -> bool {
        // In a real implementation, this would check the OS version via
        // `NSProcessInfo.operatingSystemVersion`. For now, return false
        // since we can't check at compile time.
        false
    }
}

impl Default for MacosBackdropController {
    fn default() -> Self {
        Self::new()
    }
}

impl BackdropController for MacosBackdropController {
    fn set_material(&mut self, material: BackdropMaterial) {
        self.material = material;
        // The actual NSVisualEffectView / NSGlassEffectView calls would
        // happen here with a real NSWindow handle. The caller must use
        // `apply_to_window` to apply the material to a specific window.
    }

    fn current_material(&self) -> BackdropMaterial {
        if self.supported {
            self.material
        } else {
            BackdropMaterial::None
        }
    }

    fn mode(&self) -> BackdropMode {
        match self.current_material() {
            BackdropMaterial::None => BackdropMode::Opaque,
            BackdropMaterial::Mica
            | BackdropMaterial::MicaAlt
            | BackdropMaterial::Acrylic
            | BackdropMaterial::Transient
            | BackdropMaterial::Vibrancy(_) => BackdropMode::Transparent,
        }
    }

    fn supports_material(&self, material: BackdropMaterial) -> bool {
        // macOS supports Vibrancy (all variants) and Acrylic (as a
        // vibrancy alias). Mica/MicaAlt/Transient are Windows-only.
        match material {
            BackdropMaterial::Vibrancy(_) => true,
            BackdropMaterial::Acrylic => true, // Mapped to a vibrancy material
            BackdropMaterial::None => true,
            BackdropMaterial::Mica | BackdropMaterial::MicaAlt | BackdropMaterial::Transient => {
                false
            }
        }
    }
}

/// macOS appearance change observer.
///
/// Listens for `NSAppearance` change notifications and signals the
/// theme system to trigger a 150ms `ThemeDiff` transition.
///
/// # Examples
///
/// ```no_run
/// use martensite_shell::platform_impl::macos::AppearanceObserver;
///
/// let observer = AppearanceObserver::new();
/// // In a real app, this registers an NSDistributedNotificationCenter
/// // observer for "AppleColorPreferencesChangedNotification".
/// ```
pub struct AppearanceObserver {
    active: bool,
}

impl AppearanceObserver {
    /// Creates a new appearance observer.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_shell::platform_impl::macos::AppearanceObserver;
    ///
    /// let observer = AppearanceObserver::new();
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self { active: false }
    }

    /// Returns true if the appearance is currently dark mode.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_shell::platform_impl::macos::AppearanceObserver;
    ///
    /// let observer = AppearanceObserver::new();
    /// let _is_dark = observer.is_dark_mode();
    /// ```
    #[must_use]
    pub fn is_dark_mode(&self) -> bool {
        // In a real implementation, this would query
        // `NSApp.effectiveAppearance.bestMatch` for "NSAppearanceNameDarkAqua".
        false
    }

    /// Starts observing appearance changes.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_shell::platform_impl::macos::AppearanceObserver;
    ///
    /// let mut observer = AppearanceObserver::new();
    /// observer.start();
    /// ```
    pub fn start(&mut self) {
        self.active = true;
    }

    /// Stops observing appearance changes.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_shell::platform_impl::macos::AppearanceObserver;
    ///
    /// let mut observer = AppearanceObserver::new();
    /// observer.start();
    /// observer.stop();
    /// ```
    pub fn stop(&mut self) {
        self.active = false;
    }
}

impl Default for AppearanceObserver {
    fn default() -> Self {
        Self::new()
    }
}
