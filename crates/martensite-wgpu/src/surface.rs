//! Surface and swapchain management: configuration, present-mode negotiation,
//! resize re-creation, and frame acquisition.
//!
//! [`SurfaceWrapper`] manages a [`wgpu::Surface`] together with its
//! [`wgpu::SurfaceConfiguration`]. It centralizes present-mode negotiation
//! (preferring low-latency modes and falling back to the universally-supported
//! [`wgpu::PresentMode::Fifo`]), surface (re)configuration on resize, and
//! acquisition of the current frame texture for rendering.

/// Errors produced by [`SurfaceWrapper`] operations.
///
/// # Examples
///
/// ```
/// use martensite_wgpu::surface::SurfaceWrapperError;
/// use std::error::Error;
///
/// let err = SurfaceWrapperError::NotConfigured;
/// assert!(err.to_string().contains("not been configured"));
/// assert!(err.source().is_none());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceWrapperError {
    /// The wrapper has not yet been configured with a surface and configuration.
    NotConfigured,
    /// The supplied dimensions were zero in at least one axis.
    InvalidDimensions,
}

impl std::fmt::Display for SurfaceWrapperError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotConfigured => f.write_str("surface wrapper has not been configured yet"),
            Self::InvalidDimensions => f.write_str("surface dimensions must be non-zero"),
        }
    }
}

impl std::error::Error for SurfaceWrapperError {}

/// Surface alpha mode for backdrop-aware swapchain configuration.
///
/// When a system backdrop material (Mica, Acrylic, Vibrancy) is active,
/// the surface must be configured with `PreMultiplied` alpha so the
/// compositor can show the system material through the window. When no
/// system material is active, `Opaque` is preferred for performance.
///
/// # Examples
///
/// ```
/// use martensite_wgpu::surface::BackdropMode;
///
/// let opaque = BackdropMode::Opaque;
/// let transparent = BackdropMode::Transparent;
/// assert_ne!(opaque, transparent);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum BackdropMode {
    /// Opaque surface — solid background. Uses `CompositeAlphaMode::Opaque`.
    #[default]
    Opaque,
    /// Transparent surface — system material shows through. Uses
    /// `CompositeAlphaMode::PreMultiplied`.
    Transparent,
}

impl BackdropMode {
    /// Converts to the corresponding `wgpu::CompositeAlphaMode`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_wgpu::surface::BackdropMode;
    /// use wgpu::CompositeAlphaMode;
    ///
    /// assert_eq!(BackdropMode::Opaque.to_alpha_mode(), CompositeAlphaMode::Opaque);
    /// assert_eq!(BackdropMode::Transparent.to_alpha_mode(), CompositeAlphaMode::PreMultiplied);
    /// ```
    #[must_use]
    pub fn to_alpha_mode(self) -> wgpu::CompositeAlphaMode {
        match self {
            Self::Opaque => wgpu::CompositeAlphaMode::Opaque,
            Self::Transparent => wgpu::CompositeAlphaMode::PreMultiplied,
        }
    }
}

/// The present-mode preference order used during negotiation.
///
/// The wrapper tries each mode in turn and selects the first one advertised by
/// the surface's [`wgpu::SurfaceCapabilities`]. This implements the milestone
/// fallback chain `Mailbox → FifoRelaxed → Fifo`.
const PRESENT_MODE_PREFERENCE: &[wgpu::PresentMode] = &[
    wgpu::PresentMode::Mailbox,
    wgpu::PresentMode::FifoRelaxed,
    wgpu::PresentMode::Fifo,
];

/// A wrapper around a [`wgpu::Surface`] and its active configuration.
///
/// The wrapper owns the surface and the most recently applied configuration so
/// that resize and re-configuration can be performed without the caller having
/// to track either. Present-mode negotiation is performed once at
/// configuration time and re-evaluated whenever the surface is reconfigured.
///
/// # Examples
///
/// ```no_run
/// use martensite_wgpu::surface::SurfaceWrapper;
///
/// # fn example(surface: wgpu::Surface<'_>) {
/// let wrapper = SurfaceWrapper::new(surface);
/// assert!(wrapper.configuration().is_none());
/// # }
/// ```
pub struct SurfaceWrapper<'window> {
    /// The owned presentation surface.
    surface: wgpu::Surface<'window>,
    /// The most recently applied configuration, if any.
    config: Option<wgpu::SurfaceConfiguration>,
    /// The backdrop mode applied by the most recent `configure` call, reused
    /// by `resize` so the alpha mode is preserved across reconfigurations.
    backdrop_mode: BackdropMode,
}

impl<'window> SurfaceWrapper<'window> {
    /// Creates a new wrapper from an already-created surface.
    ///
    /// The surface is not configured; call [`SurfaceWrapper::configure`] before
    /// acquiring frames.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_wgpu::surface::SurfaceWrapper;
    ///
    /// # fn example(surface: wgpu::Surface<'_>) {
    /// let wrapper = SurfaceWrapper::new(surface);
    /// // A fresh wrapper has no configuration yet.
    /// assert!(wrapper.configuration().is_none());
    /// # }
    /// ```
    #[must_use]
    pub fn new(surface: wgpu::Surface<'window>) -> Self {
        Self {
            surface,
            config: None,
            backdrop_mode: BackdropMode::Opaque,
        }
    }

    /// Returns a reference to the inner surface.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_wgpu::surface::SurfaceWrapper;
    ///
    /// # fn example(wrapper: &SurfaceWrapper<'_>) {
    /// let _surface = wrapper.surface();
    /// # }
    /// ```
    #[must_use]
    pub fn surface(&self) -> &wgpu::Surface<'window> {
        &self.surface
    }

    /// Returns the active configuration, if the surface has been configured.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_wgpu::surface::SurfaceWrapper;
    ///
    /// # fn example(wrapper: &SurfaceWrapper<'_>) {
    /// // Before `configure` is called, the configuration is `None`.
    /// assert!(wrapper.configuration().is_none());
    /// # }
    /// ```
    #[must_use]
    pub fn configuration(&self) -> Option<&wgpu::SurfaceConfiguration> {
        self.config.as_ref()
    }

    /// Negotiates the best present mode supported by `adapter` for this surface.
    ///
    /// The selection tries `Mailbox`, then `FifoRelaxed`, and always falls back
    /// to [`wgpu::PresentMode::Fifo`], which is guaranteed to be supported on
    /// every backend.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_wgpu::surface::SurfaceWrapper;
    ///
    /// # fn example(wrapper: &SurfaceWrapper<'_>, adapter: &wgpu::Adapter) {
    /// let mode = wrapper.negotiate_present_mode(adapter);
    /// // `Fifo` is always supported, so the result is never an invalid mode.
    /// assert!(matches!(
    ///     mode,
    ///     wgpu::PresentMode::Mailbox
    ///         | wgpu::PresentMode::FifoRelaxed
    ///         | wgpu::PresentMode::Fifo
    /// ));
    /// # }
    /// ```
    #[must_use]
    pub fn negotiate_present_mode(&self, adapter: &wgpu::Adapter) -> wgpu::PresentMode {
        let caps = self.surface.get_capabilities(adapter);
        for preferred in PRESENT_MODE_PREFERENCE {
            if caps.present_modes.contains(preferred) {
                return *preferred;
            }
        }
        // `Fifo` is guaranteed to be supported everywhere; this is the safe
        // last-resort fallback.
        wgpu::PresentMode::Fifo
    }

    /// Configures the surface for presentation at `width` x `height`.
    ///
    /// The texture format is taken from the surface's preferred format (the
    /// first entry of [`wgpu::SurfaceCapabilities::formats`]), the present mode
    /// is negotiated via [`SurfaceWrapper::negotiate_present_mode`], and the
    /// alpha mode is derived from `backdrop` via [`BackdropMode::to_alpha_mode`].
    /// The supplied `backdrop` is stored so that a subsequent
    /// [`SurfaceWrapper::resize`] reuses the same alpha mode.
    ///
    /// # Errors
    ///
    /// Returns [`SurfaceWrapperError::InvalidDimensions`] if either dimension is zero.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_wgpu::surface::{BackdropMode, SurfaceWrapper};
    ///
    /// # fn example(wrapper: &mut SurfaceWrapper<'_>, device: &wgpu::Device, adapter: &wgpu::Adapter) {
    /// // Zero dimensions are rejected.
    /// assert!(wrapper.configure(device, adapter, 0, 100, BackdropMode::Opaque).is_err());
    /// # }
    /// ```
    pub fn configure(
        &mut self,
        device: &wgpu::Device,
        adapter: &wgpu::Adapter,
        width: u32,
        height: u32,
        backdrop: BackdropMode,
    ) -> Result<(), SurfaceWrapperError> {
        if width == 0 || height == 0 {
            return Err(SurfaceWrapperError::InvalidDimensions);
        }

        let caps = self.surface.get_capabilities(adapter);
        let format = caps
            .formats
            .first()
            .copied()
            .unwrap_or(wgpu::TextureFormat::Bgra8Unorm);
        let present_mode = self.negotiate_present_mode(adapter);

        let config = wgpu::SurfaceConfiguration {
            // `RENDER_ATTACHMENT` allows the surface to be used as a render
            // pass target (e.g. by a blit pipeline). `COPY_DST` allows the CPU
            // software rasterizer to upload its pixel buffer directly into the
            // surface texture via `Queue::write_texture` (the TinySkia fallback
            // path in `RenderOrchestrator::render_to_surface`).
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_DST,
            format,
            color_space: wgpu::SurfaceColorSpace::Auto,
            width,
            height,
            present_mode,
            desired_maximum_frame_latency: 2,
            alpha_mode: backdrop.to_alpha_mode(),
            view_formats: Vec::new(),
        };

        self.surface.configure(device, &config);
        self.config = Some(config);
        self.backdrop_mode = backdrop;
        Ok(())
    }

    /// Configures the surface with an opaque backdrop (solid background).
    ///
    /// This is equivalent to `configure(device, adapter, width, height, BackdropMode::Opaque)`.
    ///
    /// # Errors
    ///
    /// Returns [`SurfaceWrapperError::InvalidDimensions`] if either dimension is zero.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_wgpu::surface::SurfaceWrapper;
    ///
    /// # fn example(wrapper: &mut SurfaceWrapper<'_>, device: &wgpu::Device, adapter: &wgpu::Adapter) {
    /// assert!(wrapper.configure_opaque(device, adapter, 0, 100).is_err());
    /// # }
    /// ```
    pub fn configure_opaque(
        &mut self,
        device: &wgpu::Device,
        adapter: &wgpu::Adapter,
        width: u32,
        height: u32,
    ) -> Result<(), SurfaceWrapperError> {
        self.configure(device, adapter, width, height, BackdropMode::Opaque)
    }

    /// Reconfigures the surface for a new `width` x `height`, preserving the
    /// previously negotiated format, present mode, and backdrop alpha mode.
    ///
    /// This is the resize re-creation path: it updates the stored configuration
    /// in place and re-issues [`wgpu::Surface::configure`]. The alpha mode set
    /// by the most recent [`SurfaceWrapper::configure`] (via the supplied
    /// [`BackdropMode`]) is carried over unchanged.
    ///
    /// # Errors
    ///
    /// Returns [`SurfaceWrapperError::NotConfigured`] if the surface has not been
    /// configured yet, or [`SurfaceWrapperError::InvalidDimensions`] if either
    /// dimension is zero.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_wgpu::surface::SurfaceWrapper;
    ///
    /// # fn example(wrapper: &mut SurfaceWrapper<'_>, device: &wgpu::Device) {
    /// // Zero dimensions are rejected.
    /// assert!(wrapper.resize(device, 0, 100).is_err());
    /// # }
    /// ```
    pub fn resize(
        &mut self,
        device: &wgpu::Device,
        width: u32,
        height: u32,
    ) -> Result<(), SurfaceWrapperError> {
        if width == 0 || height == 0 {
            return Err(SurfaceWrapperError::InvalidDimensions);
        }
        let mut config = self
            .config
            .clone()
            .ok_or(SurfaceWrapperError::NotConfigured)?;
        config.width = width;
        config.height = height;
        self.surface.configure(device, &config);
        self.config = Some(config);
        Ok(())
    }

    /// Acquires the current frame texture for rendering.
    ///
    /// This is a thin pass-through to [`wgpu::Surface::get_current_texture`];
    /// the caller is responsible for presenting the texture and for handling
    /// the [`wgpu::CurrentSurfaceTexture`] variants (e.g. `Outdated` should
    /// trigger a [`SurfaceWrapper::resize`] or reconfigure).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_wgpu::surface::SurfaceWrapper;
    ///
    /// # fn example(wrapper: &SurfaceWrapper<'_>) {
    /// let frame = wrapper.acquire_frame();
    /// // The caller must handle the `CurrentSurfaceTexture` variants.
    /// let _ = frame;
    /// # }
    /// ```
    #[must_use]
    pub fn acquire_frame(&self) -> wgpu::CurrentSurfaceTexture {
        self.surface.get_current_texture()
    }
}

impl<'window> std::fmt::Debug for SurfaceWrapper<'window> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SurfaceWrapper")
            .field("configured", &self.config.is_some())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn present_mode_preference_starts_with_mailbox() {
        // The negotiation order must match the milestone specification:
        // Mailbox → FifoRelaxed → Fifo.
        assert_eq!(PRESENT_MODE_PREFERENCE[0], wgpu::PresentMode::Mailbox);
        assert_eq!(PRESENT_MODE_PREFERENCE[1], wgpu::PresentMode::FifoRelaxed);
        assert_eq!(PRESENT_MODE_PREFERENCE[2], wgpu::PresentMode::Fifo);
    }

    #[test]
    fn surface_error_display_is_informative() {
        let err = SurfaceWrapperError::NotConfigured;
        assert!(!err.to_string().is_empty());
        let err = SurfaceWrapperError::InvalidDimensions;
        assert!(!err.to_string().is_empty());
    }
}
