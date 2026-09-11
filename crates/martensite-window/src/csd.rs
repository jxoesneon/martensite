//! Client-side decoration (CSD) controller for window chrome rendering.
//!
//! On Wayland, the application is responsible for drawing its own title
//! bar, window buttons, resize handles, and shadows. This module
//! provides the window-layer integration for CSD hit-testing and
//! configuration.
//!
//! On Windows and macOS, system decorations are used and CSD is
//! disabled by default.
//!
//! # Integration with the shell
//!
//! This module deliberately has **no dependency on `martensite-shell`**.
//! The [`CsdController`] is a standalone configuration type that the
//! shell layer (or any other caller) reads and writes. The shell's
//! `csd_hit_test` function consumes the values surfaced here — such as
//! [`CsdController::title_bar_height`] — when resolving which CSD region
//! (drag area, button, resize border, or client content) sits beneath a
//! pointer. See the integration note in [`crate::hit_test`] for how the
//! widget-level hit-tester relates to CSD hit-testing.

/// CSD controller for a single window.
///
/// Manages the CSD configuration and delegates hit-testing to the
/// shell's `csd_hit_test` function (when available). On platforms
/// without CSD (Windows, macOS), this controller is a no-op.
///
/// The controller stores the geometric parameters the shell needs to
/// render and hit-test client-side decorations: the title bar height,
/// the window-button corner radius, and the drop-shadow blur radius.
/// It does **not** perform any rendering itself — that is the shell's
/// responsibility — nor does it depend on `martensite-shell`, keeping
/// the dependency graph acyclic.
///
/// # Examples
///
/// ```
/// use martensite_window::csd::CsdController;
///
/// let controller = CsdController::new();
/// assert!(!controller.is_enabled());
/// ```
#[derive(Debug, Clone)]
pub struct CsdController {
    enabled: bool,
    title_bar_height: u32,
    button_radius: u32,
    shadow_blur: u32,
}

impl CsdController {
    /// Creates a new CSD controller (disabled by default).
    ///
    /// The default configuration uses a 32px title bar, a 6px button
    /// corner radius, and a 20px shadow blur — values that match the
    /// common GNOME/Adwaita CSD geometry. Call [`CsdController::enable`]
    /// to activate CSD with custom values.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_window::csd::CsdController;
    ///
    /// let controller = CsdController::new();
    /// assert!(!controller.is_enabled());
    /// assert_eq!(controller.title_bar_height(), 32);
    /// assert_eq!(controller.button_radius(), 6);
    /// assert_eq!(controller.shadow_blur(), 20);
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self {
            enabled: false,
            title_bar_height: 32,
            button_radius: 6,
            shadow_blur: 20,
        }
    }

    /// Enables CSD with the given configuration values.
    ///
    /// After this call [`CsdController::is_enabled`] returns `true` and
    /// the shell will draw and hit-test the title bar, window buttons,
    /// resize borders, and shadow using the supplied geometry.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_window::csd::CsdController;
    ///
    /// let mut controller = CsdController::new();
    /// controller.enable(32, 6, 20);
    /// assert!(controller.is_enabled());
    /// assert_eq!(controller.title_bar_height(), 32);
    /// ```
    pub fn enable(&mut self, title_bar_height: u32, button_radius: u32, shadow_blur: u32) {
        self.enabled = true;
        self.title_bar_height = title_bar_height;
        self.button_radius = button_radius;
        self.shadow_blur = shadow_blur;
    }

    /// Disables CSD (reverts to system decorations).
    ///
    /// The stored geometry values are retained so that CSD can be
    /// re-enabled with [`CsdController::enable`] without losing the
    /// previous configuration, but [`CsdController::is_enabled`] will
    /// report `false`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_window::csd::CsdController;
    ///
    /// let mut controller = CsdController::new();
    /// controller.enable(32, 6, 20);
    /// controller.disable();
    /// assert!(!controller.is_enabled());
    /// ```
    pub fn disable(&mut self) {
        self.enabled = false;
    }

    /// Returns `true` if CSD is enabled for this window.
    ///
    /// When CSD is enabled the application is responsible for drawing
    /// its own title bar, buttons, and shadow; when disabled the
    /// platform's system decorations are used instead.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_window::csd::CsdController;
    ///
    /// let controller = CsdController::new();
    /// assert!(!controller.is_enabled());
    /// ```
    #[must_use]
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Returns the title bar height in logical pixels.
    ///
    /// The shell uses this to size the draggable title-bar region and
    /// to offset client content below it. The value is in *logical*
    /// pixels and must be multiplied by the window's scale factor when
    /// converting to physical pixels for rendering.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_window::csd::CsdController;
    ///
    /// let controller = CsdController::new();
    /// assert_eq!(controller.title_bar_height(), 32);
    /// ```
    #[must_use]
    pub fn title_bar_height(&self) -> u32 {
        self.title_bar_height
    }

    /// Returns the window button corner radius in logical pixels.
    ///
    /// The shell rounds the minimize/maximize/close buttons to this
    /// radius when rendering and hit-testing them.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_window::csd::CsdController;
    ///
    /// let controller = CsdController::new();
    /// assert_eq!(controller.button_radius(), 6);
    /// ```
    #[must_use]
    pub fn button_radius(&self) -> u32 {
        self.button_radius
    }

    /// Returns the shadow blur radius in logical pixels.
    ///
    /// The shell draws a drop shadow with this blur radius around the
    /// window. On Wayland this is part of the client-drawn decoration
    /// surface; the compositor does not provide it.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_window::csd::CsdController;
    ///
    /// let controller = CsdController::new();
    /// assert_eq!(controller.shadow_blur(), 20);
    /// ```
    #[must_use]
    pub fn shadow_blur(&self) -> u32 {
        self.shadow_blur
    }
}

impl Default for CsdController {
    /// Returns a disabled [`CsdController`], equivalent to [`CsdController::new`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_window::csd::CsdController;
    ///
    /// let controller = CsdController::default();
    /// assert!(!controller.is_enabled());
    /// ```
    fn default() -> Self {
        Self::new()
    }
}

/// The result of a CSD hit-test: which decoration region sits beneath a
/// pointer, expressed in the window's local coordinate space.
///
/// This is the window-layer counterpart to the widget-level
/// [`HitTestResult`]. While [`HitTestResult`] resolves *which widget* is
/// under the pointer, [`CsdHitRegion`] resolves *which decoration
/// region* (title bar, button, resize border, or client content) is
/// under the pointer. The shell's `csd_hit_test` function produces the
/// platform-specific mapping; this type gives CSD-aware callers a
/// uniform, platform-agnostic classification they can branch on before
/// forwarding the pointer event to the widget hit-tester.
///
/// # Examples
///
/// ```
/// use martensite_window::csd::{csd_region_for_point, CsdController, CsdHitRegion};
///
/// let controller = CsdController::new();
/// // With CSD disabled every point falls through to client content.
/// let region = csd_region_for_point(&controller, 50.0, 50.0);
/// assert_eq!(region, CsdHitRegion::Client);
/// ```
///
/// [`HitTestResult`]: crate::HitTestResult
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum CsdHitRegion {
    /// The draggable title-bar area. The shell should initiate a
    /// window-move gesture on pointer press here.
    TitleBar,
    /// A window control button (minimize, maximize, or close). The shell
    /// should forward the click to the corresponding action.
    Button,
    /// A resize border. The shell should initiate a resize gesture; the
    /// specific edge is determined by the pointer's position relative to
    /// the window edges.
    ResizeBorder,
    /// The client content area. Pointer events here should be forwarded
    /// to the widget-level [`HitTester`](crate::hit_test::HitTester).
    Client,
}

/// Classifies which CSD region sits beneath a pointer at `point`.
///
/// This is a lightweight, platform-agnostic helper that CSD-aware
/// callers can use to decide whether a pointer event should start a
/// window move/resize, activate a window button, or be forwarded to the
/// widget hit-tester. When CSD is disabled (or the point falls outside
/// the title bar and resize borders) the region is
/// [`CsdHitRegion::Client`].
///
/// `point` is expressed in the window's local coordinate space (origin
/// at the top-left corner of the decorated surface), in logical pixels.
///
/// The shell's `csd_hit_test` function performs the authoritative,
/// platform-specific classification (including rounded corners and
/// button hit-testing); this helper is a fallback for callers that do
/// not have a shell backend available.
///
/// # Examples
///
/// ```
/// use martensite_window::csd::{csd_region_for_point, CsdController, CsdHitRegion};
///
/// let mut controller = CsdController::new();
/// controller.enable(32, 6, 20);
///
/// // A point in the title bar.
/// assert_eq!(csd_region_for_point(&controller, 10.0, 5.0), CsdHitRegion::TitleBar);
/// // A point below the title bar is client content.
/// assert_eq!(csd_region_for_point(&controller, 10.0, 40.0), CsdHitRegion::Client);
/// // With CSD disabled everything is client content.
/// controller.disable();
/// assert_eq!(csd_region_for_point(&controller, 10.0, 5.0), CsdHitRegion::Client);
/// ```
///
/// [`HitTester`]: crate::hit_test::HitTester
#[must_use]
pub fn csd_region_for_point(controller: &CsdController, x: f32, y: f32) -> CsdHitRegion {
    if !controller.is_enabled() {
        return CsdHitRegion::Client;
    }
    // Non-finite points fall through to the client region rather than
    // being treated as a decoration hit; this matches the NaN/Infinity
    // guard used throughout the hit-tester.
    if !x.is_finite() || !y.is_finite() {
        return CsdHitRegion::Client;
    }
    let title_bar_height = controller.title_bar_height() as f32;
    // A point within the title-bar band is classified as the title bar.
    // The shell's authoritative `csd_hit_test` further distinguishes
    // window buttons from the draggable area; this helper treats the
    // entire band as the title bar for simplicity.
    if y < title_bar_height {
        return CsdHitRegion::TitleBar;
    }
    // Points below the title bar are client content. A full
    // implementation would also classify resize borders (a thin band
    // around the window edges); that geometry is platform-specific and
    // is handled by the shell's `csd_hit_test`.
    CsdHitRegion::Client
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_controller_is_disabled() {
        let controller = CsdController::new();
        assert!(!controller.is_enabled());
    }

    #[test]
    fn default_equals_new() {
        assert!(!CsdController::default().is_enabled());
        assert_eq!(
            CsdController::default().title_bar_height(),
            CsdController::new().title_bar_height(),
        );
    }

    #[test]
    fn enable_sets_values() {
        let mut controller = CsdController::new();
        controller.enable(40, 8, 24);
        assert!(controller.is_enabled());
        assert_eq!(controller.title_bar_height(), 40);
        assert_eq!(controller.button_radius(), 8);
        assert_eq!(controller.shadow_blur(), 24);
    }

    #[test]
    fn disable_keeps_values() {
        let mut controller = CsdController::new();
        controller.enable(40, 8, 24);
        controller.disable();
        assert!(!controller.is_enabled());
        // Values are retained for re-enabling.
        assert_eq!(controller.title_bar_height(), 40);
        assert_eq!(controller.button_radius(), 8);
        assert_eq!(controller.shadow_blur(), 24);
    }

    #[test]
    fn region_disabled_is_client() {
        let controller = CsdController::new();
        assert_eq!(
            csd_region_for_point(&controller, 10.0, 5.0),
            CsdHitRegion::Client,
        );
    }

    #[test]
    fn region_title_bar_when_enabled() {
        let mut controller = CsdController::new();
        controller.enable(32, 6, 20);
        assert_eq!(
            csd_region_for_point(&controller, 10.0, 5.0),
            CsdHitRegion::TitleBar,
        );
        assert_eq!(
            csd_region_for_point(&controller, 10.0, 31.0),
            CsdHitRegion::TitleBar,
        );
    }

    #[test]
    fn region_client_below_title_bar() {
        let mut controller = CsdController::new();
        controller.enable(32, 6, 20);
        assert_eq!(
            csd_region_for_point(&controller, 10.0, 32.0),
            CsdHitRegion::Client,
        );
        assert_eq!(
            csd_region_for_point(&controller, 10.0, 100.0),
            CsdHitRegion::Client,
        );
    }

    #[test]
    fn region_nan_falls_through_to_client() {
        let mut controller = CsdController::new();
        controller.enable(32, 6, 20);
        assert_eq!(
            csd_region_for_point(&controller, f32::NAN, 5.0),
            CsdHitRegion::Client,
        );
        assert_eq!(
            csd_region_for_point(&controller, 10.0, f32::INFINITY),
            CsdHitRegion::Client,
        );
    }

    #[test]
    fn region_disabled_after_disable() {
        let mut controller = CsdController::new();
        controller.enable(32, 6, 20);
        controller.disable();
        assert_eq!(
            csd_region_for_point(&controller, 10.0, 5.0),
            CsdHitRegion::Client,
        );
    }

    #[test]
    fn csd_hit_region_variants_are_distinct() {
        assert_ne!(CsdHitRegion::TitleBar, CsdHitRegion::Button);
        assert_ne!(CsdHitRegion::Button, CsdHitRegion::ResizeBorder);
        assert_ne!(CsdHitRegion::ResizeBorder, CsdHitRegion::Client);
        assert_ne!(CsdHitRegion::Client, CsdHitRegion::TitleBar);
    }
}
