//! Surface and swapchain management: configuration, present-mode negotiation,
//! resize re-creation, and frame acquisition.
//!
//! [`SurfaceWrapper`] manages a [`wgpu::Surface`] together with its
//! [`wgpu::SurfaceConfiguration`]. It centralizes present-mode negotiation
//! (preferring low-latency modes and falling back to the universally-supported
//! [`wgpu::PresentMode::Fifo`]), surface (re)configuration on resize, and
//! acquisition of the current frame texture for rendering.

/// Errors produced by [`SurfaceWrapper`] operations.
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
pub struct SurfaceWrapper<'window> {
    /// The owned presentation surface.
    surface: wgpu::Surface<'window>,
    /// The most recently applied configuration, if any.
    config: Option<wgpu::SurfaceConfiguration>,
}

impl<'window> SurfaceWrapper<'window> {
    /// Creates a new wrapper from an already-created surface.
    ///
    /// The surface is not configured; call [`SurfaceWrapper::configure`] before
    /// acquiring frames.
    #[must_use]
    pub fn new(surface: wgpu::Surface<'window>) -> Self {
        Self {
            surface,
            config: None,
        }
    }

    /// Returns a reference to the inner surface.
    #[must_use]
    pub fn surface(&self) -> &wgpu::Surface<'window> {
        &self.surface
    }

    /// Returns the active configuration, if the surface has been configured.
    #[must_use]
    pub fn configuration(&self) -> Option<&wgpu::SurfaceConfiguration> {
        self.config.as_ref()
    }

    /// Negotiates the best present mode supported by `adapter` for this surface.
    ///
    /// The selection tries `Mailbox`, then `FifoRelaxed`, and always falls back
    /// to [`wgpu::PresentMode::Fifo`], which is guaranteed to be supported on
    /// every backend.
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
    /// alpha mode is set to [`wgpu::CompositeAlphaMode::Auto`].
    ///
    /// # Errors
    ///
    /// Returns [`SurfaceWrapperError::InvalidDimensions`] if either dimension is zero.
    pub fn configure(
        &mut self,
        device: &wgpu::Device,
        adapter: &wgpu::Adapter,
        width: u32,
        height: u32,
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
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: Vec::new(),
        };

        self.surface.configure(device, &config);
        self.config = Some(config);
        Ok(())
    }

    /// Reconfigures the surface for a new `width` x `height`, preserving the
    /// previously negotiated format and present mode.
    ///
    /// This is the resize re-creation path: it updates the stored configuration
    /// in place and re-issues [`wgpu::Surface::configure`].
    ///
    /// # Errors
    ///
    /// Returns [`SurfaceWrapperError::NotConfigured`] if the surface has not been
    /// configured yet, or [`SurfaceWrapperError::InvalidDimensions`] if either
    /// dimension is zero.
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
