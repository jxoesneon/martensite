//! Windows 11 DWM system materials and Snap Layouts integration.
//!
//! This module uses the `windows` crate for Win32/COM FFI. All unsafe
//! code is confined to this module; the crate as a whole uses
//! `#![deny(unsafe_code)]` and this module opts in with a
//! module-level `#![allow(unsafe_code)]`.
//!
//! The types defined here implement the cross-platform abstractions in
//! [`crate::backdrop`] using the Desktop Window Manager (DWM) APIs
//! introduced in Windows 11 (build 22000+). On Windows 10 and earlier,
//! the DWM system-backdrop attribute is unsupported and the controller
//! falls back to [`BackdropMaterial::None`](crate::BackdropMaterial::None).

#![allow(unsafe_code)]

use crate::backdrop::{BackdropController, BackdropMaterial, BackdropMode};

// DWM_SYSTEMBACKDROP_TYPE values (Win32 `DWM_SYSTEMBACKDROP_TYPE` enum).
const DWMSBT_AUTO: u32 = 0;
const DWMSBT_NONE: u32 = 1;
const DWMSBT_MAINWINDOW: u32 = 2; // Mica
const DWMSBT_TRANSIENTWINDOW: u32 = 3; // Mica Alt
const DWMSBT_TABBEDWINDOW: u32 = 4; // Acrylic

// DWMWINDOWATTRIBUTE values used for backdrop configuration.
const DWMWA_SYSTEMBACKDROP_TYPE: u32 = 38;
const DWMWA_USE_IMMERSIVE_DARK_MODE: u32 = 20;
const DWMWA_WINDOW_CORNER_PREFERENCE: u32 = 33;
const DWMWA_CAPTION_COLOR: u32 = 35;
const DWMWA_TEXT_COLOR: u32 = 36;

/// Windows 11 backdrop controller using DWM system materials.
///
/// On Windows 11 build 22000+, `DwmSetWindowAttribute` with
/// `DWMWA_SYSTEMBACKDROP_TYPE` applies Mica, Mica Alt, or Acrylic to a
/// window. On Windows 10 and earlier, that attribute is unsupported and
/// the controller falls back to
/// [`BackdropMaterial::None`](crate::BackdropMaterial::None) (solid
/// color).
///
/// This stub implementation records the requested material and reports
/// support pessimistically (`supported = false`) until a real window
/// handle is wired in. The actual `DwmSetWindowAttribute` calls will be
/// added when the window integration layer provides an `HWND`.
///
/// # Examples
///
/// ```no_run
/// use martensite_shell::platform_impl::windows::WindowsBackdropController;
/// use martensite_shell::{BackdropController, BackdropMaterial};
///
/// let mut controller = WindowsBackdropController::new();
/// controller.set_material(BackdropMaterial::Mica);
/// // On non-Win11, falls back to None.
/// assert_eq!(controller.current_material(), BackdropMaterial::None);
/// ```
pub struct WindowsBackdropController {
    material: BackdropMaterial,
    supported: bool,
}

impl WindowsBackdropController {
    /// Creates a new Windows backdrop controller.
    ///
    /// The controller starts with [`BackdropMaterial::None`] and
    /// `supported = false`; support is determined lazily when a material
    /// is applied to a real window.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_shell::platform_impl::windows::WindowsBackdropController;
    /// use martensite_shell::{BackdropController, BackdropMaterial};
    ///
    /// let controller = WindowsBackdropController::new();
    /// assert_eq!(controller.current_material(), BackdropMaterial::None);
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self {
            material: BackdropMaterial::None,
            supported: false, // Determined on first `apply_to_window` call.
        }
    }

    /// Maps a [`BackdropMaterial`] to the DWM `DWMSBT_*` value.
    ///
    /// Materials not supported on Windows (e.g. macOS vibrancy) map to
    /// `DWMSBT_NONE`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_shell::platform_impl::windows::WindowsBackdropController;
    /// use martensite_shell::BackdropMaterial;
    ///
    /// // Mica maps to the DWM "main window" backdrop type.
    /// let dwm = WindowsBackdropController::material_to_dwm_type(BackdropMaterial::Mica);
    /// assert_eq!(dwm, 2);
    /// ```
    #[must_use]
    pub fn material_to_dwm_type(material: BackdropMaterial) -> u32 {
        match material {
            BackdropMaterial::None => DWMSBT_NONE,
            BackdropMaterial::Mica => DWMSBT_MAINWINDOW,
            BackdropMaterial::MicaAlt => DWMSBT_TRANSIENTWINDOW,
            BackdropMaterial::Acrylic => DWMSBT_TABBEDWINDOW,
            BackdropMaterial::Transient => DWMSBT_TRANSIENTWINDOW,
            BackdropMaterial::Vibrancy(_) => DWMSBT_NONE, // Not supported on Windows.
        }
    }

    /// Checks if the current OS supports DWM system backdrops.
    ///
    /// On Windows 11 build 22000+, `DwmSetWindowAttribute` with
    /// `DWMWA_SYSTEMBACKDROP_TYPE` succeeds. On earlier versions, it
    /// fails. This stub returns `false` because a real `HWND` is needed
    /// to perform the probe; the actual check happens when
    /// `apply_to_window` is called.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_shell::platform_impl::windows::WindowsBackdropController;
    ///
    /// // Without a real window handle, support is reported as false.
    /// assert!(!WindowsBackdropController::check_support());
    /// ```
    #[must_use]
    pub fn check_support() -> bool {
        // In a real implementation, this would call `DwmSetWindowAttribute`
        // with a test value and inspect the `HRESULT`. For now, return
        // `false` since we don't have a real `HWND`. The actual check
        // happens when `apply_to_window` is called.
        false
    }
}

impl Default for WindowsBackdropController {
    /// Returns the default controller, equivalent to [`new`](Self::new).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_shell::platform_impl::windows::WindowsBackdropController;
    ///
    /// let controller = WindowsBackdropController::default();
    /// ```
    fn default() -> Self {
        Self::new()
    }
}

impl BackdropController for WindowsBackdropController {
    fn set_material(&mut self, material: BackdropMaterial) {
        self.material = material;
        // The actual `DwmSetWindowAttribute` call would happen here with
        // a real `HWND`. Since this abstraction does not yet hold a raw
        // window handle, the caller must use `apply_to_window`. For now,
        // just store the requested material.
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
        // Win11 supports None (no backdrop), Mica, MicaAlt, Acrylic, and
        // Transient. Vibrancy is macOS-only.
        matches!(
            material,
            BackdropMaterial::None
                | BackdropMaterial::Mica
                | BackdropMaterial::MicaAlt
                | BackdropMaterial::Acrylic
                | BackdropMaterial::Transient
        )
    }
}

/// Snap layout integration for Windows 11.
///
/// On Windows 11, the snap flyout is triggered by hovering/clicking the
/// maximize button with the mouse, or by pressing `Win+Z`. The
/// `ISnapLayouts` COM interface provides the snap-zone layout. This
/// stub records support state pessimistically until the runtime probe
/// is wired in.
///
/// # Examples
///
/// ```no_run
/// use martensite_shell::platform_impl::windows::WindowsSnapLayout;
///
/// let snap = WindowsSnapLayout::new();
/// // Snap layouts require Windows 11; the stub reports unsupported.
/// assert!(!snap.is_supported());
/// assert_eq!(snap.max_zones(), 0);
/// ```
pub struct WindowsSnapLayout {
    supported: bool,
    max_zones: u32,
}

impl WindowsSnapLayout {
    /// Creates a new Windows snap layout handler.
    ///
    /// Support is determined at runtime; this stub starts unsupported
    /// until the `ISnapLayouts` probe is implemented.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_shell::platform_impl::windows::WindowsSnapLayout;
    ///
    /// let snap = WindowsSnapLayout::new();
    /// assert!(!snap.is_supported());
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self {
            supported: false, // Determined at runtime.
            max_zones: 0,
        }
    }

    /// Returns `true` if snap layouts are supported on this system.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_shell::platform_impl::windows::WindowsSnapLayout;
    ///
    /// let snap = WindowsSnapLayout::new();
    /// let _ = snap.is_supported();
    /// ```
    #[must_use]
    pub fn is_supported(&self) -> bool {
        self.supported
    }

    /// Returns the maximum number of snap zones available.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_shell::platform_impl::windows::WindowsSnapLayout;
    ///
    /// let snap = WindowsSnapLayout::new();
    /// assert_eq!(snap.max_zones(), 0);
    /// ```
    #[must_use]
    pub fn max_zones(&self) -> u32 {
        self.max_zones
    }
}

impl Default for WindowsSnapLayout {
    /// Returns the default snap layout, equivalent to [`new`](Self::new).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_shell::platform_impl::windows::WindowsSnapLayout;
    ///
    /// let snap = WindowsSnapLayout::default();
    /// assert!(!snap.is_supported());
    /// ```
    fn default() -> Self {
        Self::new()
    }
}

// Keep the DWM attribute constants referenced so they are not flagged as
// unused while the FFI calls are still stubbed out.
#[allow(dead_code)]
const _DWM_ATTRS_USED: [u32; 6] = [
    DWMWA_SYSTEMBACKDROP_TYPE,
    DWMWA_USE_IMMERSIVE_DARK_MODE,
    DWMWA_WINDOW_CORNER_PREFERENCE,
    DWMWA_CAPTION_COLOR,
    DWMWA_TEXT_COLOR,
    DWMSBT_AUTO,
];
