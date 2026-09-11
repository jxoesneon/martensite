//! Platform-specific window attribute builders.
//!
//! These types provide a safe, platform-typed builder pattern for
//! configuring platform-specific window features (backdrop materials,
//! title bar styling, vibrancy) *before* window creation. The
//! resulting configuration is applied by the shell layer after the
//! window is created, since the actual FFI lives in `martensite-shell`.
//!
//! # Examples
//!
//! ```
//! use martensite_window::window_attributes::WindowsWindowAttributes;
//!
//! let attrs = WindowsWindowAttributes::default()
//!     .with_backdrop_material(martensite_shell::BackdropMaterial::Mica)
//!     .with_dark_mode(true);
//! assert_eq!(attrs.backdrop_material, martensite_shell::BackdropMaterial::Mica);
//! assert_eq!(attrs.dark_mode, Some(true));
//! ```

use martensite_shell::BackdropMaterial;

// ---------------------------------------------------------------------------
// Windows
// ---------------------------------------------------------------------------

/// Platform-specific window attributes for Windows.
///
/// This builder configures Windows-specific window features that are
/// applied via DWM attributes after window creation. The fields mirror
/// the DWM attribute configuration in the `martensite_shell` Windows
/// platform backend.
///
/// # Examples
///
/// ```
/// use martensite_window::window_attributes::WindowsWindowAttributes;
/// use martensite_shell::BackdropMaterial;
///
/// let attrs = WindowsWindowAttributes::default()
///     .with_backdrop_material(BackdropMaterial::Mica)
///     .with_dark_mode(true)
///     .with_rounded_corners(true)
///     .with_caption_color(0x00FFFFFF)
///     .with_text_color(0x00000000);
///
/// assert_eq!(attrs.backdrop_material, BackdropMaterial::Mica);
/// assert_eq!(attrs.dark_mode, Some(true));
/// assert_eq!(attrs.rounded_corners, Some(true));
/// assert_eq!(attrs.caption_color, Some(0x00FFFFFF));
/// assert_eq!(attrs.text_color, Some(0x00000000));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsWindowAttributes {
    /// The system backdrop material to apply via
    /// `DWMWA_SYSTEMBACKDROP_TYPE`.
    pub backdrop_material: BackdropMaterial,
    /// Whether to sync the title bar with dark mode
    /// (`DWMWA_USE_IMMERSIVE_DARK_MODE`). `None` leaves the
    /// attribute unchanged.
    pub dark_mode: Option<bool>,
    /// Whether to use rounded window corners
    /// (`DWMWA_WINDOW_CORNER_PREFERENCE`). `None` leaves the
    /// attribute unchanged.
    pub rounded_corners: Option<bool>,
    /// Custom title bar background color as a `COLORREF`
    /// (`0x00BBGGRR`) (`DWMWA_CAPTION_COLOR`). `None` leaves
    /// the attribute unchanged.
    pub caption_color: Option<u32>,
    /// Custom title bar text color as a `COLORREF`
    /// (`0x00BBGGRR`) (`DWMWA_TEXT_COLOR`). `None` leaves the
    /// attribute unchanged.
    pub text_color: Option<u32>,
}

impl Default for WindowsWindowAttributes {
    /// Returns the default Windows window attributes.
    ///
    /// The default backdrop material is [`BackdropMaterial::None`]
    /// (solid color background), and all DWM title bar attributes
    /// are left unchanged (`None`).
    fn default() -> Self {
        Self {
            backdrop_material: BackdropMaterial::None,
            dark_mode: None,
            rounded_corners: None,
            caption_color: None,
            text_color: None,
        }
    }
}

impl WindowsWindowAttributes {
    /// Creates a new `WindowsWindowAttributes` with default values.
    ///
    /// The default backdrop material is [`BackdropMaterial::None`]
    /// (solid color background), and all DWM title bar attributes
    /// are left unchanged (`None`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_window::window_attributes::WindowsWindowAttributes;
    /// use martensite_shell::BackdropMaterial;
    ///
    /// let attrs = WindowsWindowAttributes::new();
    /// assert_eq!(attrs.backdrop_material, BackdropMaterial::None);
    /// assert_eq!(attrs.dark_mode, None);
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the system backdrop material.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_window::window_attributes::WindowsWindowAttributes;
    /// use martensite_shell::BackdropMaterial;
    ///
    /// let attrs = WindowsWindowAttributes::default()
    ///     .with_backdrop_material(BackdropMaterial::Acrylic);
    /// assert_eq!(attrs.backdrop_material, BackdropMaterial::Acrylic);
    /// ```
    #[must_use]
    pub fn with_backdrop_material(mut self, material: BackdropMaterial) -> Self {
        self.backdrop_material = material;
        self
    }

    /// Sets whether to sync the title bar with dark mode.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_window::window_attributes::WindowsWindowAttributes;
    ///
    /// let attrs = WindowsWindowAttributes::default().with_dark_mode(true);
    /// assert_eq!(attrs.dark_mode, Some(true));
    /// ```
    #[must_use]
    pub fn with_dark_mode(mut self, dark: bool) -> Self {
        self.dark_mode = Some(dark);
        self
    }

    /// Sets whether to use rounded window corners.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_window::window_attributes::WindowsWindowAttributes;
    ///
    /// let attrs = WindowsWindowAttributes::default().with_rounded_corners(true);
    /// assert_eq!(attrs.rounded_corners, Some(true));
    /// ```
    #[must_use]
    pub fn with_rounded_corners(mut self, rounded: bool) -> Self {
        self.rounded_corners = Some(rounded);
        self
    }

    /// Sets the custom title bar background color (`COLORREF`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_window::window_attributes::WindowsWindowAttributes;
    ///
    /// let attrs = WindowsWindowAttributes::default().with_caption_color(0x00FFFFFF);
    /// assert_eq!(attrs.caption_color, Some(0x00FFFFFF));
    /// ```
    #[must_use]
    pub fn with_caption_color(mut self, color: u32) -> Self {
        self.caption_color = Some(color);
        self
    }

    /// Sets the custom title bar text color (`COLORREF`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_window::window_attributes::WindowsWindowAttributes;
    ///
    /// let attrs = WindowsWindowAttributes::default().with_text_color(0x00000000);
    /// assert_eq!(attrs.text_color, Some(0x00000000));
    /// ```
    #[must_use]
    pub fn with_text_color(mut self, color: u32) -> Self {
        self.text_color = Some(color);
        self
    }
}

// ---------------------------------------------------------------------------
// macOS
// ---------------------------------------------------------------------------

/// Platform-specific window attributes for macOS.
///
/// This builder configures macOS-specific window features that are
/// applied via `NSVisualEffectView` / `NSGlassEffectView` after window
/// creation. The fields mirror the vibrancy configuration in the
/// `martensite_shell` macOS platform backend.
///
/// # Examples
///
/// ```
/// use martensite_window::window_attributes::MacOSWindowAttributes;
/// use martensite_shell::VibrancyMaterial;
///
/// let attrs = MacOSWindowAttributes::default()
///     .with_vibrancy_material(VibrancyMaterial::Sidebar);
/// assert_eq!(attrs.vibrancy_material, Some(VibrancyMaterial::Sidebar));
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MacOSWindowAttributes {
    /// The macOS vibrancy material to apply via
    /// `NSVisualEffectView` (or `NSGlassEffectView` on macOS 26+).
    pub vibrancy_material: Option<martensite_shell::VibrancyMaterial>,
    /// Whether the title bar should appear in translucent style.
    /// `None` leaves the system default.
    pub titlebar_appears_translucent: Option<bool>,
    /// Whether the window's style mask should include
    /// `NSFullSizeContentViewWindowMask` (content extends under the
    /// title bar). `None` leaves the system default.
    pub full_size_content_view: Option<bool>,
}

impl MacOSWindowAttributes {
    /// Creates a new `MacOSWindowAttributes` with default values.
    ///
    /// The default has no vibrancy material and leaves all title
    /// bar attributes at their system defaults.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_window::window_attributes::MacOSWindowAttributes;
    ///
    /// let attrs = MacOSWindowAttributes::new();
    /// assert_eq!(attrs.vibrancy_material, None);
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the macOS vibrancy material.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_window::window_attributes::MacOSWindowAttributes;
    /// use martensite_shell::VibrancyMaterial;
    ///
    /// let attrs = MacOSWindowAttributes::default()
    ///     .with_vibrancy_material(VibrancyMaterial::LiquidGlass);
    /// assert_eq!(attrs.vibrancy_material, Some(VibrancyMaterial::LiquidGlass));
    /// ```
    #[must_use]
    pub fn with_vibrancy_material(mut self, material: martensite_shell::VibrancyMaterial) -> Self {
        self.vibrancy_material = Some(material);
        self
    }

    /// Sets whether the title bar appears translucent.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_window::window_attributes::MacOSWindowAttributes;
    ///
    /// let attrs = MacOSWindowAttributes::default().with_titlebar_translucent(true);
    /// assert_eq!(attrs.titlebar_appears_translucent, Some(true));
    /// ```
    #[must_use]
    pub fn with_titlebar_translucent(mut self, translucent: bool) -> Self {
        self.titlebar_appears_translucent = Some(translucent);
        self
    }

    /// Sets whether the content view extends under the title bar.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_window::window_attributes::MacOSWindowAttributes;
    ///
    /// let attrs = MacOSWindowAttributes::default().with_full_size_content_view(true);
    /// assert_eq!(attrs.full_size_content_view, Some(true));
    /// ```
    #[must_use]
    pub fn with_full_size_content_view(mut self, full_size: bool) -> Self {
        self.full_size_content_view = Some(full_size);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_defaults_are_none() {
        let attrs = WindowsWindowAttributes::default();
        assert_eq!(attrs.backdrop_material, BackdropMaterial::None);
        assert_eq!(attrs.dark_mode, None);
        assert_eq!(attrs.rounded_corners, None);
        assert_eq!(attrs.caption_color, None);
        assert_eq!(attrs.text_color, None);
    }

    #[test]
    fn windows_builder_sets_all_fields() {
        let attrs = WindowsWindowAttributes::new()
            .with_backdrop_material(BackdropMaterial::MicaAlt)
            .with_dark_mode(false)
            .with_rounded_corners(false)
            .with_caption_color(0x0000FF)
            .with_text_color(0xFF0000);
        assert_eq!(attrs.backdrop_material, BackdropMaterial::MicaAlt);
        assert_eq!(attrs.dark_mode, Some(false));
        assert_eq!(attrs.rounded_corners, Some(false));
        assert_eq!(attrs.caption_color, Some(0x0000FF));
        assert_eq!(attrs.text_color, Some(0xFF0000));
    }

    #[test]
    fn macos_defaults_are_none() {
        let attrs = MacOSWindowAttributes::default();
        assert_eq!(attrs.vibrancy_material, None);
        assert_eq!(attrs.titlebar_appears_translucent, None);
        assert_eq!(attrs.full_size_content_view, None);
    }

    #[test]
    fn macos_builder_sets_all_fields() {
        let attrs = MacOSWindowAttributes::new()
            .with_vibrancy_material(martensite_shell::VibrancyMaterial::HudWindow)
            .with_titlebar_translucent(true)
            .with_full_size_content_view(true);
        assert_eq!(
            attrs.vibrancy_material,
            Some(martensite_shell::VibrancyMaterial::HudWindow)
        );
        assert_eq!(attrs.titlebar_appears_translucent, Some(true));
        assert_eq!(attrs.full_size_content_view, Some(true));
    }
}
