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
//!
//! The `windows` crate is only pulled in when the `windows-backend`
//! feature is enabled. Without it, the controller stores the requested
//! material but reports `supported = false` (no FFI calls are made).

#![allow(unsafe_code)]

use crate::backdrop::{BackdropController, BackdropMaterial, BackdropMode, Window};
use core::ffi::c_void;

// --- DWM FFI imports (only available with the `windows-backend` feature) ---
#[cfg(feature = "windows-backend")]
use windows::Win32::Foundation::HWND;
#[cfg(feature = "windows-backend")]
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMSBT_MAINWINDOW, DWMSBT_NONE, DWMSBT_TABBEDWINDOW,
    DWMSBT_TRANSIENTWINDOW, DWMWA_SYSTEMBACKDROP_TYPE, DWMWINDOWATTRIBUTE, DWM_SYSTEMBACKDROP_TYPE,
};

// --- Additional DWM window attribute IDs (v0.13.0) ---
//
// These are defined as local `u32` constants rather than imported from the
// `windows` crate so they are available regardless of feature gates. They are
// wrapped in `DWMWINDOWATTRIBUTE(...)` when passed to `DwmSetWindowAttribute`.
// See <https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/nf-dwmapi-dwmsetwindowattribute>.
/// `DWMWA_USE_IMMERSIVE_DARK_MODE` (20) — syncs the title bar with dark mode.
const DWMWA_USE_IMMERSIVE_DARK_MODE: u32 = 20;
/// `DWMWA_WINDOW_CORNER_PREFERENCE` (33) — toggles rounded window corners.
const DWMWA_WINDOW_CORNER_PREFERENCE: u32 = 33;
/// `DWMWA_CAPTION_COLOR` (35) — custom title bar background color (`COLORREF`).
const DWMWA_CAPTION_COLOR: u32 = 35;
/// `DWMWA_TEXT_COLOR` (36) — custom title bar text color (`COLORREF`).
const DWMWA_TEXT_COLOR: u32 = 36;

// `DWM_WINDOW_CORNER_PREFERENCE` enum values.
/// `DWMWCP_DEFAULT` (0) — let the system decide corner preference.
const DWMWCP_DEFAULT: i32 = 0;
/// `DWMWCP_ROUND` (2) — force rounded corners.
const DWMWCP_ROUND: i32 = 2;

/// Style options for the Windows title bar applied via DWM attributes.
///
/// Each field is [`Option`]al; [`None`] means "leave the attribute
/// unchanged". The values are applied via `DwmSetWindowAttribute` in
/// [`WindowsBackdropController::set_title_bar_style`]. All attribute
/// applications are best-effort: failures (e.g. on pre-Windows 11 builds
/// where an attribute is unsupported) are silently ignored.
///
/// # Examples
///
/// ```
/// use martensite_shell::platform_impl::windows::TitleBarStyle;
///
/// let style = TitleBarStyle {
///     dark_mode: Some(true),
///     rounded_corners: Some(true),
///     caption_color: Some(0x00FFFFFF),
///     text_color: Some(0x00000000),
/// };
/// assert_eq!(style.dark_mode, Some(true));
/// assert_eq!(style.rounded_corners, Some(true));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TitleBarStyle {
    /// Whether to sync the title bar with dark mode
    /// (`DWMWA_USE_IMMERSIVE_DARK_MODE`, attr ID 20).
    pub dark_mode: Option<bool>,
    /// Whether to use rounded window corners (`true` → `DWMWCP_ROUND`,
    /// `false` → `DWMWCP_DEFAULT`; `DWMWA_WINDOW_CORNER_PREFERENCE`, attr
    /// ID 33).
    pub rounded_corners: Option<bool>,
    /// Custom title bar background color as a `COLORREF` (`0x00BBGGRR`)
    /// (`DWMWA_CAPTION_COLOR`, attr ID 35).
    pub caption_color: Option<u32>,
    /// Custom title bar text color as a `COLORREF` (`0x00BBGGRR`)
    /// (`DWMWA_TEXT_COLOR`, attr ID 36).
    pub text_color: Option<u32>,
}

/// Windows 11 backdrop controller using DWM system materials.
///
/// On Windows 11 build 22000+, `DwmSetWindowAttribute` with
/// `DWMWA_SYSTEMBACKDROP_TYPE` applies Mica, Mica Alt, or Acrylic to a
/// window. On Windows 10 and earlier, that attribute is unsupported and
/// the controller falls back to
/// [`BackdropMaterial::None`](crate::BackdropMaterial::None) (solid
/// color).
///
/// The controller captures the raw window handle (`HWND`) from the
/// [`Window`] argument on each [`set_material`](BackdropController::set_material)
/// call and invokes the DWM API directly. If the call fails (pre-Win11),
/// the controller reports `supported = false` and the material falls
/// back to `None`.
///
/// # Examples
///
/// ```no_run
/// use martensite_shell::platform_impl::windows::WindowsBackdropController;
/// use martensite_shell::{BackdropController, BackdropMaterial, Window};
/// use core::ffi::c_void;
///
/// # struct W;
/// # impl Window for W {
/// #     unsafe fn raw_handle(&self) -> *mut c_void { core::ptr::null_mut() }
/// # }
///
/// let mut controller = WindowsBackdropController::new();
/// controller.set_material(&W, BackdropMaterial::Mica);
/// // On pre-Win11, falls back to None.
/// ```
pub struct WindowsBackdropController {
    /// The requested backdrop material.
    material: BackdropMaterial,
    /// Whether DWM system backdrops are supported on this OS.
    supported: bool,
}

impl WindowsBackdropController {
    /// Creates a new Windows backdrop controller.
    ///
    /// The controller starts with [`BackdropMaterial::None`] and
    /// `supported = false`; support is determined lazily when a material
    /// is applied to a real window via
    /// [`set_material`](BackdropController::set_material).
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
            supported: false,
        }
    }

    /// Applies Windows 11 title bar styling via additional DWM window
    /// attributes.
    ///
    /// This sets the immersive dark mode flag (`DWMWA_USE_IMMERSIVE_DARK_MODE`,
    /// attr ID 20), the window corner preference
    /// (`DWMWA_WINDOW_CORNER_PREFERENCE`, attr ID 33), the caption color
    /// (`DWMWA_CAPTION_COLOR`, attr ID 35), and the text color
    /// (`DWMWA_TEXT_COLOR`, attr ID 36) through `DwmSetWindowAttribute`.
    ///
    /// Each attribute is applied independently and best-effort: if an
    /// attribute is unsupported (e.g. on pre-Windows 11 builds), the
    /// failure is silently ignored and the remaining attributes are
    /// still attempted. Fields set to [`None`] in [`TitleBarStyle`] are
    /// skipped entirely.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_shell::platform_impl::windows::{TitleBarStyle, WindowsBackdropController};
    /// use martensite_shell::Window;
    /// use core::ffi::c_void;
    ///
    /// # struct W;
    /// # impl Window for W {
    /// #     unsafe fn raw_handle(&self) -> *mut c_void { core::ptr::null_mut() }
    /// # }
    ///
    /// let mut controller = WindowsBackdropController::new();
    /// let style = TitleBarStyle {
    ///     dark_mode: Some(true),
    ///     rounded_corners: Some(true),
    ///     caption_color: Some(0x00FFFFFF),
    ///     text_color: Some(0x00000000),
    /// };
    /// controller.set_title_bar_style(&W, &style);
    /// ```
    pub fn set_title_bar_style(&mut self, window: &dyn Window, style: &TitleBarStyle) {
        #[cfg(feature = "windows-backend")]
        {
            // Safety: the caller guarantees the window is alive for the
            // duration of this call, so the HWND is valid to pass to DWM.
            let handle = unsafe { window.raw_handle() };
            let hwnd = HWND(handle);

            // DWMWA_USE_IMMERSIVE_DARK_MODE — BOOL (i32, 0 or 1).
            if let Some(dark) = style.dark_mode {
                let value: i32 = i32::from(dark);
                let _ = unsafe {
                    DwmSetWindowAttribute(
                        hwnd,
                        DWMWINDOWATTRIBUTE(DWMWA_USE_IMMERSIVE_DARK_MODE as i32),
                        &value as *const _ as *const c_void,
                        core::mem::size_of::<i32>() as u32,
                    )
                };
            }

            // DWMWA_WINDOW_CORNER_PREFERENCE — DWM_WINDOW_CORNER_PREFERENCE (i32).
            if let Some(rounded) = style.rounded_corners {
                let value: i32 = if rounded {
                    DWMWCP_ROUND
                } else {
                    DWMWCP_DEFAULT
                };
                let _ = unsafe {
                    DwmSetWindowAttribute(
                        hwnd,
                        DWMWINDOWATTRIBUTE(DWMWA_WINDOW_CORNER_PREFERENCE as i32),
                        &value as *const _ as *const c_void,
                        core::mem::size_of::<i32>() as u32,
                    )
                };
            }

            // DWMWA_CAPTION_COLOR — COLORREF (u32).
            if let Some(color) = style.caption_color {
                let _ = unsafe {
                    DwmSetWindowAttribute(
                        hwnd,
                        DWMWINDOWATTRIBUTE(DWMWA_CAPTION_COLOR as i32),
                        &color as *const _ as *const c_void,
                        core::mem::size_of::<u32>() as u32,
                    )
                };
            }

            // DWMWA_TEXT_COLOR — COLORREF (u32).
            if let Some(color) = style.text_color {
                let _ = unsafe {
                    DwmSetWindowAttribute(
                        hwnd,
                        DWMWINDOWATTRIBUTE(DWMWA_TEXT_COLOR as i32),
                        &color as *const _ as *const c_void,
                        core::mem::size_of::<u32>() as u32,
                    )
                };
            }
        }

        #[cfg(not(feature = "windows-backend"))]
        {
            // Without the windows-backend feature, no FFI is available.
            // Safety: stub does not dereference the handle.
            let _ = unsafe { window.raw_handle() };
        }
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
    fn set_material(&mut self, window: &dyn Window, material: BackdropMaterial) {
        // Short-circuit if the material hasn't changed.
        if self.material == material {
            return;
        }

        // Vibrancy is a macOS-only material backed by NSVisualEffectView.
        // It is not supported by the DWM system-backdrop API, so we don't
        // attempt any FFI call for it; report unsupported and fall back to
        // `None`.
        #[cfg(feature = "windows-backend")]
        if matches!(material, BackdropMaterial::Vibrancy(_)) {
            self.material = BackdropMaterial::None;
            self.supported = false;
            return;
        }

        #[cfg(feature = "windows-backend")]
        {
            // Map the cross-platform material to the DWM `DWMSBT_*` value.
            //
            // Per the Microsoft documentation for `DWM_SYSTEMBACKDROP_TYPE`:
            //   DWMSBT_MAINWINDOW      (2) — Mica
            //   DWMSBT_TRANSIENTWINDOW (3) — Acrylic
            //   DWMSBT_TABBEDWINDOW    (4) — Mica Alt
            let dwm_type: DWM_SYSTEMBACKDROP_TYPE = match material {
                BackdropMaterial::None => DWMSBT_NONE,
                BackdropMaterial::Mica => DWMSBT_MAINWINDOW,
                BackdropMaterial::MicaAlt => DWMSBT_TABBEDWINDOW,
                BackdropMaterial::Acrylic => DWMSBT_TRANSIENTWINDOW,
                BackdropMaterial::Transient => DWMSBT_TRANSIENTWINDOW,
                BackdropMaterial::Vibrancy(_) => DWMSBT_NONE, // unreachable; handled above
            };

            // Safety: the caller guarantees the window is alive for the
            // duration of this call, so the HWND is valid to pass to DWM.
            let handle = unsafe { window.raw_handle() };
            let hwnd = HWND(handle);
            let result = unsafe {
                DwmSetWindowAttribute(
                    hwnd,
                    DWMWA_SYSTEMBACKDROP_TYPE,
                    &dwm_type as *const _ as *const c_void,
                    core::mem::size_of::<DWM_SYSTEMBACKDROP_TYPE>() as u32,
                )
            };

            if result.is_ok() {
                self.material = material;
                self.supported = true;
            } else {
                // Pre-Win11: DWMWA_SYSTEMBACKDROP_TYPE is unsupported.
                self.material = BackdropMaterial::None;
                self.supported = false;
            }
        }

        #[cfg(not(feature = "windows-backend"))]
        {
            // Without the windows-backend feature, no FFI is available.
            // Safety: stub does not dereference the handle.
            let _ = unsafe { window.raw_handle() };
            self.material = material;
            self.supported = false;
        }
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
            BackdropMaterial::Mica | BackdropMaterial::MicaAlt => BackdropMode::Opaque,
            BackdropMaterial::Acrylic
            | BackdropMaterial::Transient
            | BackdropMaterial::Vibrancy(_) => BackdropMode::Transparent,
        }
    }

    fn supports_material(&self, material: BackdropMaterial) -> bool {
        // Win11 supports None (no backdrop), Mica, MicaAlt, Acrylic, and
        // Transient. Vibrancy is macOS-only. `self.supported` reflects
        // whether the DWM system-backdrop API is available on this OS
        // (i.e. Windows 11 22000+); without it none of the materials are
        // actually applied.
        self.supported
            && matches!(
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
/// maximize button with the mouse, or by pressing `Win+Z`. The real
/// Windows 11 Snap Layouts API is accessed through `IInspectable` and the
/// `ISnapLayouts` interface, which is not a standard COM class and
/// cannot be probed via `CoCreateInstance`.
///
/// Because snap layouts are only available on the same builds that
/// support the DWM system-backdrop API (`DWMWA_SYSTEMBACKDROP_TYPE`,
/// Windows 11 build 22000+), support is inferred from whether the
/// backdrop controller's DWM call succeeds rather than from a fabricated
/// COM probe. Until a real window-based probe is implemented, this type
/// reports `supported = false` by default; callers that have confirmed
/// DWM backdrop availability may enable it via
/// [`set_supported`](Self::set_supported).
///
/// # Examples
///
/// ```no_run
/// use martensite_shell::platform_impl::windows::WindowsSnapLayout;
///
/// let snap = WindowsSnapLayout::new();
/// let _ = snap.is_supported();
/// let _ = snap.max_zones();
/// ```
pub struct WindowsSnapLayout {
    supported: bool,
    max_zones: u32,
}

impl WindowsSnapLayout {
    /// Creates a new Windows snap layout handler.
    ///
    /// The handler starts with `supported = false` and `max_zones = 0`.
    /// Real probing requires a live window (the Snap Layouts API is
    /// accessed through `IInspectable`/`ISnapLayouts`, not a standard COM
    /// class), so support must be set externally via
    /// [`set_supported`](Self::set_supported) once DWM backdrop
    /// availability has been confirmed.
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
    pub fn new() -> Self {
        Self {
            supported: false,
            max_zones: 0,
        }
    }

    /// Marks snap layouts as supported or unsupported.
    ///
    /// When enabled, `max_zones` is set to 4 (the Windows 11 quadrant
    /// layout); when disabled it is reset to 0. This is intended to be
    /// driven by the result of the DWM system-backdrop probe, since the
    /// same Windows 11 builds support both features.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_shell::platform_impl::windows::WindowsSnapLayout;
    ///
    /// let mut snap = WindowsSnapLayout::new();
    /// snap.set_supported(true);
    /// assert!(snap.is_supported());
    /// assert_eq!(snap.max_zones(), 4);
    /// ```
    pub fn set_supported(&mut self, supported: bool) {
        self.supported = supported;
        self.max_zones = if supported { 4 } else { 0 };
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
    /// On Windows 11 this is 4 (quadrants). On unsupported systems this
    /// is 0.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_shell::platform_impl::windows::WindowsSnapLayout;
    ///
    /// let snap = WindowsSnapLayout::new();
    /// let _ = snap.max_zones();
    /// ```
    #[must_use]
    pub fn max_zones(&self) -> u32 {
        self.max_zones
    }

    /// Returns `true` if the given point lies within the maximize button
    /// region.
    ///
    /// The maximize button occupies the top-right corner of the title bar:
    /// 46 pixels wide and `title_bar_height` pixels tall, flush with the
    /// right edge of the window. This is used to detect mouse hover/click
    /// for triggering the Snap Layouts flyout.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_shell::platform_impl::windows::WindowsSnapLayout;
    ///
    /// let snap = WindowsSnapLayout::new();
    /// // Top-right corner of a 1000px-wide window with a 32px title bar.
    /// assert!(snap.hit_test_maximize_button(980, 16, 1000, 32));
    /// // Just outside the left edge of the button.
    /// assert!(!snap.hit_test_maximize_button(953, 16, 1000, 32));
    /// // Below the title bar.
    /// assert!(!snap.hit_test_maximize_button(980, 33, 1000, 32));
    /// ```
    #[must_use]
    pub fn hit_test_maximize_button(
        &self,
        x: i32,
        y: i32,
        window_width: i32,
        title_bar_height: i32,
    ) -> bool {
        const MAXIMIZE_BUTTON_WIDTH: i32 = 46;
        let left = window_width - MAXIMIZE_BUTTON_WIDTH;
        let right = window_width;
        let top = 0;
        let bottom = title_bar_height;
        x >= left && x < right && y >= top && y < bottom
    }

    /// Returns `true` if the snap flyout should be shown.
    ///
    /// This is `true` when the mouse is hovering over the maximize button
    /// (`hit_maximize`) **and** snap layouts are supported on this system
    /// (see [`is_supported`](Self::is_supported)).
    ///
    /// Note: The actual `ISnapLayouts` COM interface requires the Windows
    /// App SDK and cannot be probed via `CoCreateInstance`. This best-effort
    /// implementation infers support from DWM backdrop availability (set
    /// via [`set_supported`](Self::set_supported) once the DWM
    /// `DWMWA_SYSTEMBACKDROP_TYPE` probe has succeeded).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_shell::platform_impl::windows::WindowsSnapLayout;
    ///
    /// let mut snap = WindowsSnapLayout::new();
    /// snap.set_supported(true);
    /// assert!(snap.should_show_snap_flyout(true));
    /// assert!(!snap.should_show_snap_flyout(false));
    ///
    /// snap.set_supported(false);
    /// assert!(!snap.should_show_snap_flyout(true));
    /// ```
    #[must_use]
    pub fn should_show_snap_flyout(&self, hit_maximize: bool) -> bool {
        hit_maximize && self.supported
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
    /// let _ = snap.is_supported();
    /// ```
    fn default() -> Self {
        Self::new()
    }
}
