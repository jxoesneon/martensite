//! Wayland client-side decorations and fractional-scale.
//!
//! Unlike Windows and macOS backends, the Wayland backend does NOT use
//! system materials (there is no Wayland protocol for compositor-side
//! blur). Instead, CSD (client-side decorations) are rendered by the
//! application itself through the normal paint pipeline.
//!
//! StatusNotifierItem system tray registration has been moved to the
//! dedicated [`crate::status_notifier`] module.
//!
//! This module is entirely safe Rust — no `unsafe` code is needed.

use crate::backdrop::{BackdropController, BackdropMaterial, BackdropMode, Window};
use crate::event::{ShellEvent, ShellEventQueue};

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
/// use martensite_shell::{BackdropController, BackdropMaterial, Window};
/// use core::ffi::c_void;
///
/// # struct W;
/// # impl Window for W {
/// #     unsafe fn raw_handle(&self) -> *mut c_void { core::ptr::null_mut() }
/// # }
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
    fn set_material(&mut self, _window: &dyn Window, _material: BackdropMaterial) {
        // No-op: Wayland has no system material protocol.
    }
    fn current_material(&self) -> BackdropMaterial {
        BackdropMaterial::None
    }
    fn mode(&self) -> BackdropMode {
        BackdropMode::Opaque
    }
    fn supports_material(&self, material: BackdropMaterial) -> bool {
        // Wayland has no system material protocol, but `None` (solid opaque
        // fallback) is always supported.
        material == BackdropMaterial::None
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
        let physical = ((logical as f64) * self.scale).round();
        // Clamp to a sane maximum to avoid silent saturation to u32::MAX.
        let clamped = physical.clamp(0.0, u32::MAX as f64 - 1.0);
        clamped as u32
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
        let logical = ((physical as f64) / self.scale).round();
        // Clamp to a sane maximum to avoid silent saturation to u32::MAX.
        let clamped = logical.clamp(0.0, u32::MAX as f64 - 1.0);
        clamped as u32
    }
}

/// Mutable tracker for the fractional scale of a Wayland surface.
///
/// The compositor sends `wp_fractional_scale_v1::preferred_scale` events
/// whenever the preferred scale for a surface changes (e.g. when the
/// window is dragged between outputs with different DPRs). This type
/// holds the most recent value, validated through [`FractionalScale::new`]
/// so that unreasonable or non-finite compositor values can never reach
/// the rendering pipeline.
///
/// When a [`ShellEventQueue`] is attached via
/// [`with_event_queue`](Self::with_event_queue), each scale change also
/// pushes a [`ShellEvent::FractionalScaleChanged`] onto the queue so the
/// window manager can emit
/// `WindowEventOutcome::FractionalScaleChanged`.
///
/// # Examples
///
/// ```
/// use martensite_shell::platform_impl::wayland::FractionalScaleTracker;
/// use martensite_shell::ShellEventQueue;
///
/// let queue = ShellEventQueue::new();
/// let mut tracker = FractionalScaleTracker::with_event_queue(queue.clone());
/// assert_eq!(tracker.current().scale(), 1.0);
/// tracker.update(1.5);
/// assert_eq!(tracker.current().scale(), 1.5);
/// // The event was pushed to the queue.
/// let events = queue.drain();
/// assert_eq!(events.len(), 1);
/// // NaN and infinity are rejected, falling back to 1.0.
/// tracker.update(f64::NAN);
/// assert_eq!(tracker.current().scale(), 1.0);
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct FractionalScaleTracker {
    scale: FractionalScale,
    event_queue: Option<ShellEventQueue>,
}

impl FractionalScaleTracker {
    /// Creates a new tracker starting at the default scale of `1.0`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_shell::platform_impl::wayland::FractionalScaleTracker;
    ///
    /// let tracker = FractionalScaleTracker::new();
    /// assert_eq!(tracker.current().scale(), 1.0);
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self {
            scale: FractionalScale::new(1.0),
            event_queue: None,
        }
    }

    /// Creates a new tracker with a [`ShellEventQueue`] attached.
    ///
    /// Each call to [`update`](Self::update) that changes the scale
    /// pushes a [`ShellEvent::FractionalScaleChanged`] onto the queue.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_shell::platform_impl::wayland::FractionalScaleTracker;
    /// use martensite_shell::{ShellEvent, ShellEventQueue};
    ///
    /// let queue = ShellEventQueue::new();
    /// let mut tracker = FractionalScaleTracker::with_event_queue(queue.clone());
    /// tracker.update(2.0);
    /// assert_eq!(queue.drain(), vec![ShellEvent::FractionalScaleChanged(2.0)]);
    /// ```
    #[must_use]
    pub fn with_event_queue(queue: ShellEventQueue) -> Self {
        Self {
            scale: FractionalScale::new(1.0),
            event_queue: Some(queue),
        }
    }

    /// Attaches or replaces the [`ShellEventQueue`] for event emission.
    ///
    /// After calling this, subsequent [`update`](Self::update) calls
    /// will push `FractionalScaleChanged` events onto the queue.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_shell::platform_impl::wayland::FractionalScaleTracker;
    /// use martensite_shell::{ShellEvent, ShellEventQueue};
    ///
    /// let queue = ShellEventQueue::new();
    /// let mut tracker = FractionalScaleTracker::new();
    /// tracker.set_event_queue(queue.clone());
    /// tracker.update(1.5);
    /// assert_eq!(queue.drain(), vec![ShellEvent::FractionalScaleChanged(1.5)]);
    /// ```
    pub fn set_event_queue(&mut self, queue: ShellEventQueue) {
        self.event_queue = Some(queue);
    }

    /// Updates the tracked scale with a new compositor value.
    ///
    /// The value is validated through [`FractionalScale::new`], so NaN,
    /// infinity, and out-of-range values are clamped/rejected before
    /// being stored. If the validated scale differs from the previous
    /// value and an event queue is attached, a
    /// [`ShellEvent::FractionalScaleChanged`] is pushed.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_shell::platform_impl::wayland::FractionalScaleTracker;
    ///
    /// let mut tracker = FractionalScaleTracker::new();
    /// tracker.update(2.0);
    /// assert_eq!(tracker.current().scale(), 2.0);
    /// ```
    pub fn update(&mut self, scale: f64) {
        let new_scale = FractionalScale::new(scale);
        let changed = new_scale != self.scale;
        self.scale = new_scale;
        if changed {
            if let Some(queue) = &self.event_queue {
                queue.push(ShellEvent::FractionalScaleChanged(new_scale.scale()));
            }
        }
    }

    /// Returns the current validated fractional scale.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_shell::platform_impl::wayland::FractionalScaleTracker;
    ///
    /// let tracker = FractionalScaleTracker::new();
    /// let scale = tracker.current();
    /// assert_eq!(scale.scale(), 1.0);
    /// ```
    #[must_use]
    pub fn current(&self) -> FractionalScale {
        self.scale
    }
}

impl Default for FractionalScaleTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Computes the physical buffer size for a surface from its logical size
/// and fractional scale.
///
/// Given a logical (surface) width and height in device-independent
/// pixels and a [`FractionalScale`], this returns the physical buffer
/// dimensions the compositor expects, by applying
/// [`FractionalScale::to_physical`] to each axis independently.
///
/// # Examples
///
/// ```
/// use martensite_shell::platform_impl::wayland::{physical_buffer_size, FractionalScale};
///
/// // 800x600 logical at 1.5x -> 1200x900 physical.
/// let (w, h) = physical_buffer_size(800, 600, FractionalScale::new(1.5));
/// assert_eq!((w, h), (1200, 900));
///
/// // 100x100 logical at 1.25x -> 125x125 physical.
/// let (w, h) = physical_buffer_size(100, 100, FractionalScale::new(1.25));
/// assert_eq!((w, h), (125, 125));
/// ```
#[must_use]
pub fn physical_buffer_size(
    logical_width: u32,
    logical_height: u32,
    scale: FractionalScale,
) -> (u32, u32) {
    (
        scale.to_physical(logical_width),
        scale.to_physical(logical_height),
    )
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
/// - Buttons: 14x14px squares, positioned per desktop environment:
///   - GNOME: top-right (close/min/max, right-to-left).
///   - KDE: top-right (close/min/max, right-to-left, tighter spacing).
///   - wlroots/Unknown: top-right (minimal, close only).
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
        let button_y = (title_h - BUTTON_SIZE) / 2;

        // Button positions vary by desktop environment per the v0.13.0
        // spec: GNOME uses 8px padding, KDE uses 4px (tighter), and
        // wlroots/Unknown expose only a close button (minimal).
        let (padding, has_min_max) = match config.environment() {
            DesktopEnvironment::Gnome => (8, true),
            DesktopEnvironment::Kde => (4, true),
            DesktopEnvironment::Wlroots | DesktopEnvironment::Unknown => (6, false),
        };

        // Close button at top-right.
        let close_x = width.saturating_sub(padding + BUTTON_SIZE);
        if x >= close_x && x < close_x + BUTTON_SIZE && y >= button_y && y < button_y + BUTTON_SIZE
        {
            return CsdHitTest::CloseButton;
        }

        if has_min_max {
            // Minimize button to the left of close.
            let min_x = close_x.saturating_sub(padding + BUTTON_SIZE);
            if x >= min_x && x < min_x + BUTTON_SIZE && y >= button_y && y < button_y + BUTTON_SIZE
            {
                return CsdHitTest::MinimizeButton;
            }
            // Maximize button to the left of minimize.
            let max_x = min_x.saturating_sub(padding + BUTTON_SIZE);
            if x >= max_x && x < max_x + BUTTON_SIZE && y >= button_y && y < button_y + BUTTON_SIZE
            {
                return CsdHitTest::MaximizeButton;
            }
        }

        return CsdHitTest::TitleBar;
    }

    CsdHitTest::Client
}
