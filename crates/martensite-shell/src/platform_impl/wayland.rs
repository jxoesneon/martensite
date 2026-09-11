//! Wayland client-side decorations, fractional-scale, and system tray.
//!
//! Unlike Windows and macOS backends, the Wayland backend does NOT use
//! system materials (there is no Wayland protocol for compositor-side
//! blur). Instead, CSD (client-side decorations) are rendered by the
//! application itself through the normal paint pipeline.
//!
//! This module is entirely safe Rust — no `unsafe` code is needed.

use crate::backdrop::{BackdropController, BackdropMaterial, BackdropMode};

/// Wayland backdrop controller — always returns `None` (no system materials).
///
/// Wayland compositors do not provide a system backdrop blur protocol.
/// The application must render its own background (solid color or
/// application-level blur via `PaintList::push_blurred_rect`).
///
/// # Examples
///
/// ```
/// use martensite_shell::platform_impl::wayland::WaylandBackdropController;
/// use martensite_shell::{BackdropController, BackdropMaterial};
///
/// let controller = WaylandBackdropController::new();
/// assert_eq!(controller.current_material(), BackdropMaterial::None);
/// assert!(!controller.supports_material(BackdropMaterial::Mica));
/// ```
pub struct WaylandBackdropController;

impl WaylandBackdropController {
    /// Creates a new Wayland backdrop controller.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Default for WaylandBackdropController {
    fn default() -> Self {
        Self::new()
    }
}

impl BackdropController for WaylandBackdropController {
    fn set_material(&mut self, _material: BackdropMaterial) {
        // No-op: Wayland has no system material protocol.
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

/// Fractional scale information for a Wayland surface.
///
/// `wp_fractional_scale_v1` provides a fractional scale factor (e.g. 1.5)
/// that is more precise than the integer scale factors from
/// `wl_surface::preferred_buffer_scale`.
///
/// # Examples
///
/// ```
/// use martensite_shell::platform_impl::wayland::FractionalScale;
///
/// let scale = FractionalScale::new(1.5);
/// assert_eq!(scale.scale(), 1.5);
/// assert_eq!(scale.to_physical(100), 150);
/// assert_eq!(scale.to_logical(150), 100);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FractionalScale {
    scale: f64,
}

impl FractionalScale {
    /// Creates a new fractional scale from a `wp_fractional_scale_v1` value.
    ///
    /// The scale is clamped to the range [0.25, 10.0] to reject
    /// unreasonable compositor values.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_shell::platform_impl::wayland::FractionalScale;
    ///
    /// let scale = FractionalScale::new(1.5);
    /// assert_eq!(scale.scale(), 1.5);
    ///
    /// // Unreasonable values are clamped.
    /// let clamped = FractionalScale::new(100.0);
    /// assert_eq!(clamped.scale(), 10.0);
    /// ```
    #[must_use]
    pub fn new(scale: f64) -> Self {
        // Reject NaN and infinity — clamp to a safe default of 1.0.
        let clamped = if scale.is_finite() {
            scale.clamp(0.25, 10.0)
        } else {
            1.0
        };
        Self { scale: clamped }
    }

    /// Returns the fractional scale factor.
    #[must_use]
    pub fn scale(&self) -> f64 {
        self.scale
    }

    /// Converts logical pixels to physical pixels (buffer size).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_shell::platform_impl::wayland::FractionalScale;
    ///
    /// let scale = FractionalScale::new(1.5);
    /// assert_eq!(scale.to_physical(100), 150);
    /// assert_eq!(scale.to_physical(200), 300);
    /// ```
    #[must_use]
    pub fn to_physical(&self, logical: u32) -> u32 {
        ((logical as f64) * self.scale).round() as u32
    }

    /// Converts physical pixels (buffer) to logical pixels.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_shell::platform_impl::wayland::FractionalScale;
    ///
    /// let scale = FractionalScale::new(1.5);
    /// assert_eq!(scale.to_logical(150), 100);
    /// assert_eq!(scale.to_logical(300), 200);
    /// ```
    #[must_use]
    pub fn to_logical(&self, physical: u32) -> u32 {
        ((physical as f64) / self.scale).round() as u32
    }
}

/// CSD (client-side decoration) configuration.
///
/// On Wayland, the application is responsible for drawing its own title
/// bar, window buttons, resize handles, and shadows. This struct
/// configures the CSD appearance based on the detected desktop
/// environment.
///
/// # Examples
///
/// ```
/// use martensite_shell::platform_impl::wayland::{CsdConfig, DesktopEnvironment};
///
/// let config = CsdConfig::new(DesktopEnvironment::Gnome);
/// assert_eq!(config.title_bar_height(), 32);
/// assert_eq!(config.button_radius(), 6);
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct CsdConfig {
    /// The detected desktop environment.
    env: DesktopEnvironment,
    /// Title bar height in logical pixels.
    title_bar_height: u32,
    /// Window button corner radius in logical pixels.
    button_radius: u32,
    /// Shadow blur radius in logical pixels.
    shadow_blur: u32,
}

/// Detected Linux desktop environment for CSD styling.
///
/// # Examples
///
/// ```
/// use martensite_shell::platform_impl::wayland::DesktopEnvironment;
///
/// let env = DesktopEnvironment::Gnome;
/// assert!(env != DesktopEnvironment::Kde);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DesktopEnvironment {
    /// GNOME (Adwaita-style CSD).
    Gnome,
    /// KDE Plasma (Breeze-style CSD).
    Kde,
    /// Generic wlroots compositor (minimal CSD).
    Wlroots,
    /// Unknown desktop environment (minimal CSD).
    Unknown,
}

impl CsdConfig {
    /// Creates a new CSD config for the given desktop environment.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_shell::platform_impl::wayland::{CsdConfig, DesktopEnvironment};
    ///
    /// let gnome = CsdConfig::new(DesktopEnvironment::Gnome);
    /// let kde = CsdConfig::new(DesktopEnvironment::Kde);
    /// assert_ne!(gnome, kde);
    /// ```
    #[must_use]
    pub fn new(env: DesktopEnvironment) -> Self {
        let (title_bar_height, button_radius, shadow_blur) = match env {
            DesktopEnvironment::Gnome => (32, 6, 20),
            DesktopEnvironment::Kde => (30, 4, 16),
            DesktopEnvironment::Wlroots | DesktopEnvironment::Unknown => (28, 4, 12),
        };
        Self {
            env,
            title_bar_height,
            button_radius,
            shadow_blur,
        }
    }

    /// Returns the title bar height in logical pixels.
    #[must_use]
    pub fn title_bar_height(&self) -> u32 {
        self.title_bar_height
    }

    /// Returns the window button corner radius in logical pixels.
    #[must_use]
    pub fn button_radius(&self) -> u32 {
        self.button_radius
    }

    /// Returns the shadow blur radius in logical pixels.
    #[must_use]
    pub fn shadow_blur(&self) -> u32 {
        self.shadow_blur
    }

    /// Returns the desktop environment.
    #[must_use]
    pub fn environment(&self) -> DesktopEnvironment {
        self.env
    }
}

/// CSD hit-test result for mouse interaction with client-side decorations.
///
/// # Examples
///
/// ```
/// use martensite_shell::platform_impl::wayland::CsdHitTest;
///
/// let hit = CsdHitTest::TitleBar;
/// assert!(hit.is_title_bar());
/// assert!(!hit.is_client());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CsdHitTest {
    /// The title bar drag region.
    TitleBar,
    /// The close button.
    CloseButton,
    /// The minimize button.
    MinimizeButton,
    /// The maximize/restore button.
    MaximizeButton,
    /// The top resize edge.
    ResizeTop,
    /// The bottom resize edge.
    ResizeBottom,
    /// The left resize edge.
    ResizeLeft,
    /// The right resize edge.
    ResizeRight,
    /// The top-left resize corner.
    ResizeTopLeft,
    /// The top-right resize corner.
    ResizeTopRight,
    /// The bottom-left resize corner.
    ResizeBottomLeft,
    /// The bottom-right resize corner.
    ResizeBottomRight,
    /// The client content area (not CSD).
    Client,
}

impl CsdHitTest {
    /// Returns true if this hit is in the title bar drag region.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_shell::platform_impl::wayland::CsdHitTest;
    ///
    /// assert!(CsdHitTest::TitleBar.is_title_bar());
    /// assert!(!CsdHitTest::Client.is_title_bar());
    /// ```
    #[must_use]
    pub fn is_title_bar(&self) -> bool {
        matches!(self, Self::TitleBar)
    }

    /// Returns true if this hit is a resize edge or corner.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_shell::platform_impl::wayland::CsdHitTest;
    ///
    /// assert!(CsdHitTest::ResizeTop.is_resize());
    /// assert!(CsdHitTest::ResizeTopLeft.is_resize());
    /// assert!(!CsdHitTest::Client.is_resize());
    /// ```
    #[must_use]
    pub fn is_resize(&self) -> bool {
        matches!(
            self,
            Self::ResizeTop
                | Self::ResizeBottom
                | Self::ResizeLeft
                | Self::ResizeRight
                | Self::ResizeTopLeft
                | Self::ResizeTopRight
                | Self::ResizeBottomLeft
                | Self::ResizeBottomRight
        )
    }

    /// Returns true if this hit is a window button.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_shell::platform_impl::wayland::CsdHitTest;
    ///
    /// assert!(CsdHitTest::CloseButton.is_button());
    /// assert!(CsdHitTest::MinimizeButton.is_button());
    /// assert!(CsdHitTest::MaximizeButton.is_button());
    /// assert!(!CsdHitTest::Client.is_button());
    /// ```
    #[must_use]
    pub fn is_button(&self) -> bool {
        matches!(
            self,
            Self::CloseButton | Self::MinimizeButton | Self::MaximizeButton
        )
    }

    /// Returns true if this hit is in the client content area (not CSD).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_shell::platform_impl::wayland::CsdHitTest;
    ///
    /// assert!(CsdHitTest::Client.is_client());
    /// assert!(!CsdHitTest::TitleBar.is_client());
    /// ```
    #[must_use]
    pub fn is_client(&self) -> bool {
        matches!(self, Self::Client)
    }
}

/// Performs a CSD hit-test at the given logical coordinates.
///
/// Returns the `CsdHitTest` for the given point within a window of
/// `width` x `height` logical pixels, with the given `CsdConfig`.
///
/// The hit-test regions are:
/// - Title bar: top `title_bar_height` pixels, excluding button regions.
/// - Resize edges: 8px border around the window.
/// - Buttons: 14x14px squares in the top-right (GNOME) or top-left (KDE).
///
/// # Examples
///
/// ```
/// use martensite_shell::platform_impl::wayland::{csd_hit_test, CsdConfig, CsdHitTest, DesktopEnvironment};
///
/// let config = CsdConfig::new(DesktopEnvironment::Gnome);
/// // Point at (5, 5) is in the top-left resize corner.
/// assert_eq!(csd_hit_test(5, 5, 800, 600, &config), CsdHitTest::ResizeTopLeft);
/// // Point at (400, 10) is in the title bar.
/// assert_eq!(csd_hit_test(400, 10, 800, 600, &config), CsdHitTest::TitleBar);
/// // Point at (400, 300) is in the client area.
/// assert_eq!(csd_hit_test(400, 300, 800, 600, &config), CsdHitTest::Client);
/// ```
#[must_use]
pub fn csd_hit_test(x: u32, y: u32, width: u32, height: u32, config: &CsdConfig) -> CsdHitTest {
    const RESIZE_BORDER: u32 = 8;
    const BUTTON_SIZE: u32 = 14;
    const BUTTON_PADDING: u32 = 8;

    let title_h = config.title_bar_height();

    // Check resize edges (8px border)
    let on_top = y < RESIZE_BORDER;
    let on_bottom = y >= height.saturating_sub(RESIZE_BORDER);
    let on_left = x < RESIZE_BORDER;
    let on_right = x >= width.saturating_sub(RESIZE_BORDER);

    match (on_top, on_bottom, on_left, on_right) {
        (true, _, true, _) => return CsdHitTest::ResizeTopLeft,
        (true, _, _, true) => return CsdHitTest::ResizeTopRight,
        (_, true, true, _) => return CsdHitTest::ResizeBottomLeft,
        (_, true, _, true) => return CsdHitTest::ResizeBottomRight,
        (true, _, _, _) => return CsdHitTest::ResizeTop,
        (_, true, _, _) => return CsdHitTest::ResizeBottom,
        (_, _, true, _) => return CsdHitTest::ResizeLeft,
        (_, _, _, true) => return CsdHitTest::ResizeRight,
        _ => {}
    }

    // Check title bar
    if y < title_h {
        // Check window buttons (GNOME: top-right, KDE: top-left)
        match config.environment() {
            DesktopEnvironment::Gnome
            | DesktopEnvironment::Wlroots
            | DesktopEnvironment::Unknown => {
                // Close button at top-right
                let close_x = width.saturating_sub(BUTTON_PADDING + BUTTON_SIZE);
                let close_y = (title_h - BUTTON_SIZE) / 2;
                if x >= close_x
                    && x < close_x + BUTTON_SIZE
                    && y >= close_y
                    && y < close_y + BUTTON_SIZE
                {
                    return CsdHitTest::CloseButton;
                }
                // Minimize button
                let min_x = close_x.saturating_sub(BUTTON_PADDING + BUTTON_SIZE);
                if x >= min_x
                    && x < min_x + BUTTON_SIZE
                    && y >= close_y
                    && y < close_y + BUTTON_SIZE
                {
                    return CsdHitTest::MinimizeButton;
                }
                // Maximize button
                let max_x = min_x.saturating_sub(BUTTON_PADDING + BUTTON_SIZE);
                if x >= max_x
                    && x < max_x + BUTTON_SIZE
                    && y >= close_y
                    && y < close_y + BUTTON_SIZE
                {
                    return CsdHitTest::MaximizeButton;
                }
            }
            DesktopEnvironment::Kde => {
                // KDE: buttons at top-right (same layout as GNOME).
                let close_x = width.saturating_sub(BUTTON_PADDING + BUTTON_SIZE);
                let close_y = (title_h - BUTTON_SIZE) / 2;
                if x >= close_x
                    && x < close_x + BUTTON_SIZE
                    && y >= close_y
                    && y < close_y + BUTTON_SIZE
                {
                    return CsdHitTest::CloseButton;
                }
                // Minimize button
                let min_x = close_x.saturating_sub(BUTTON_PADDING + BUTTON_SIZE);
                if x >= min_x
                    && x < min_x + BUTTON_SIZE
                    && y >= close_y
                    && y < close_y + BUTTON_SIZE
                {
                    return CsdHitTest::MinimizeButton;
                }
                // Maximize button
                let max_x = min_x.saturating_sub(BUTTON_PADDING + BUTTON_SIZE);
                if x >= max_x
                    && x < max_x + BUTTON_SIZE
                    && y >= close_y
                    && y < close_y + BUTTON_SIZE
                {
                    return CsdHitTest::MaximizeButton;
                }
            }
        }
        return CsdHitTest::TitleBar;
    }

    CsdHitTest::Client
}

/// StatusNotifierItem system tray registration.
///
/// Implements the `org.kde.StatusNotifierItem` D-Bus protocol for
/// registering an application icon on the system tray. This is a
/// stub implementation — the actual D-Bus connection requires the
/// `zbus` crate (added in Phase 4 wiring).
///
/// # Examples
///
/// ```
/// use martensite_shell::platform_impl::wayland::StatusNotifierItem;
///
/// let item = StatusNotifierItem::new("my-app", "My Application");
/// assert_eq!(item.id(), "my-app");
/// assert_eq!(item.title(), "My Application");
/// assert!(!item.is_registered());
/// ```
#[derive(Debug, Clone)]
pub struct StatusNotifierItem {
    id: String,
    title: String,
    registered: bool,
}

impl StatusNotifierItem {
    /// Creates a new StatusNotifierItem with the given D-Bus ID and title.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_shell::platform_impl::wayland::StatusNotifierItem;
    ///
    /// let item = StatusNotifierItem::new("my-app", "My Application");
    /// assert_eq!(item.id(), "my-app");
    /// ```
    #[must_use]
    pub fn new(id: &str, title: &str) -> Self {
        Self {
            id: id.to_string(),
            title: title.to_string(),
            registered: false,
        }
    }

    /// Returns the D-Bus ID of the item.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns the display title of the item.
    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Returns true if the item is registered on the session bus.
    #[must_use]
    pub fn is_registered(&self) -> bool {
        self.registered
    }

    /// Registers the item on the D-Bus session bus.
    ///
    /// In this stub implementation, this just sets `registered` to `true`.
    /// The real implementation will use `zbus` to connect to the session
    /// bus and register the `org.kde.StatusNotifierItem` interface.
    pub fn register(&mut self) {
        self.registered = true;
    }

    /// Unregisters the item from the D-Bus session bus.
    pub fn unregister(&mut self) {
        self.registered = false;
    }
}
