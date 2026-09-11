//! CPU pixel buffer presentation via `softbuffer`.
//!
//! When the GPU device is lost or unavailable, the [`TinySkiaBackend`]
//! renders into an RGBA CPU pixel buffer. This module provides
//! [`SoftbufferPresenter`] which manages a `softbuffer::Surface` and
//! uploads that buffer to a window surface, enabling software-only
//! rendering without a GPU swapchain.
//!
//! [`TinySkiaBackend`]: crate::tinyskia_backend::TinySkiaBackend

use std::num::NonZeroU32;

/// Error returned by [`SoftbufferPresenter`] operations.
///
/// # Examples
///
/// ```
/// use martensite_render::PresentationError;
///
/// // Each variant describes a distinct presentation failure mode.
/// let err = PresentationError::NotConfigured;
/// assert_eq!(format!("{err}"), "surface has not been configured");
///
/// let err = PresentationError::EmptyBuffer;
/// assert_eq!(format!("{err}"), "pixel buffer is empty");
///
/// let err = PresentationError::ZeroDimension;
/// assert_eq!(format!("{err}"), "surface width or height is zero");
/// ```
#[derive(Debug)]
pub enum PresentationError {
    /// The surface has not been configured yet.
    NotConfigured,
    /// The provided buffer size does not match the configured surface size.
    SizeMismatch {
        /// Expected width in pixels.
        expected_width: u32,
        /// Expected height in pixels.
        expected_height: u32,
        /// Actual buffer width in pixels.
        actual_width: u32,
        /// Actual buffer height in pixels.
        actual_height: u32,
    },
    /// The pixel buffer is empty (zero length).
    EmptyBuffer,
    /// The surface width or height is zero.
    ZeroDimension,
    /// The underlying `softbuffer` operation failed.
    Softbuffer(String),
}

impl std::fmt::Display for PresentationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotConfigured => write!(f, "surface has not been configured"),
            Self::SizeMismatch {
                expected_width,
                expected_height,
                actual_width,
                actual_height,
            } => write!(
                f,
                "buffer size {actual_width}x{actual_height} does not match surface size {expected_width}x{expected_height}"
            ),
            Self::EmptyBuffer => write!(f, "pixel buffer is empty"),
            Self::ZeroDimension => write!(f, "surface width or height is zero"),
            Self::Softbuffer(msg) => write!(f, "softbuffer error: {msg}"),
        }
    }
}

impl std::error::Error for PresentationError {}

/// Converts an RGBA pixel buffer (8 bits per channel, 4 bytes per pixel)
/// into the `X8R8G8B8` (u32) format expected by `softbuffer`.
///
/// This function performs the color channel swizzle from RGBA byte order
/// to the packed `0x00RRGGBB` u32 format, discarding the alpha channel
/// (softbuffer surfaces are opaque by default).
///
/// # Errors
///
/// Returns [`PresentationError::EmptyBuffer`] if the input buffer is empty.
///
/// # Examples
///
/// ```
/// use martensite_render::rgba_to_softbuffer;
///
/// // Red, green, and blue pixels are packed as 0x00RRGGBB u32 values.
/// let rgba = [255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255];
/// let pixels = rgba_to_softbuffer(&rgba).expect("non-empty buffer");
/// assert_eq!(pixels.len(), 3);
/// assert_eq!(pixels[0], 0x00FF_0000);
/// assert_eq!(pixels[1], 0x0000_FF00);
/// assert_eq!(pixels[2], 0x0000_00FF);
/// ```
pub fn rgba_to_softbuffer(rgba: &[u8]) -> Result<Vec<u32>, PresentationError> {
    if rgba.is_empty() {
        return Err(PresentationError::EmptyBuffer);
    }
    let mut pixels = Vec::with_capacity(rgba.len() / 4);
    let (chunks, remainder) = rgba.as_chunks::<4>();
    for chunk in chunks {
        let r = u32::from(chunk[0]);
        let g = u32::from(chunk[1]);
        let b = u32::from(chunk[2]);
        // Pack as 0x00RRGGBB (alpha is discarded; softbuffer surfaces are opaque)
        pixels.push((r << 16) | (g << 8) | b);
    }
    // Ignore any trailing bytes that don't form a complete RGBA pixel.
    let _ = remainder;
    Ok(pixels)
}

/// Validates and prepares an RGBA pixel buffer for presentation to a
/// `softbuffer` surface.
///
/// This is the software fallback presentation path used when the GPU
/// device is lost or unavailable. The `rgba_buffer` must be in
/// RGBA8 format (4 bytes per pixel, row-major order) with dimensions
/// matching `width` and `height`.
///
/// # Arguments
///
/// * `rgba_buffer` — The RGBA8 pixel data from [`TinySkiaBackend::pixels()`].
/// * `width` — The width of the pixel buffer in pixels.
/// * `height` — The height of the pixel buffer in pixels.
///
/// # Errors
///
/// Returns [`PresentationError`] if the buffer is empty, dimensions are
/// zero, or the buffer length does not match `width * height * 4`.
///
/// # Examples
///
/// ```
/// use martensite_render::present_rgba_to_softbuffer;
///
/// // A 2x2 buffer of opaque grey pixels converts successfully.
/// let rgba = vec![128u8; 2 * 2 * 4];
/// let pixels = present_rgba_to_softbuffer(&rgba, 2, 2).expect("valid buffer");
/// assert_eq!(pixels.len(), 4);
///
/// // A zero-width request is rejected.
/// let err = present_rgba_to_softbuffer(&rgba, 0, 2).unwrap_err();
/// assert!(format!("{err}").contains("zero"));
/// ```
///
/// [`TinySkiaBackend::pixels()`]: crate::tinyskia_backend::TinySkiaBackend::pixels
pub fn present_rgba_to_softbuffer(
    rgba_buffer: &[u8],
    width: u32,
    height: u32,
) -> Result<Vec<u32>, PresentationError> {
    if width == 0 || height == 0 {
        return Err(PresentationError::ZeroDimension);
    }
    let expected_len = (width as usize) * (height as usize) * 4;
    if rgba_buffer.len() < expected_len {
        return Err(PresentationError::SizeMismatch {
            expected_width: width,
            expected_height: height,
            actual_width: (rgba_buffer.len() / 4) as u32,
            actual_height: if width > 0 {
                (rgba_buffer.len() / 4 / width as usize) as u32
            } else {
                0
            },
        });
    }
    rgba_to_softbuffer(rgba_buffer)
}

/// Helper to create a `NonZeroU32` from a `u32`, returning the given
/// error if the value is zero.
///
/// # Errors
///
/// Returns `err` if `value` is zero.
///
/// # Examples
///
/// ```
/// use martensite_render::presentation::nonzero;
/// use martensite_render::PresentationError;
/// use std::num::NonZeroU32;
///
/// // A non-zero value produces a valid NonZeroU32.
/// let ok = nonzero(42, PresentationError::NotConfigured);
/// assert_eq!(ok.expect("non-zero").get(), 42);
///
/// // A zero value returns the supplied error.
/// let err = nonzero(0, PresentationError::NotConfigured);
/// assert!(err.is_err());
/// ```
pub fn nonzero(value: u32, err: PresentationError) -> Result<NonZeroU32, PresentationError> {
    NonZeroU32::new(value).ok_or(err)
}

/// A presenter that manages a `softbuffer::Surface` for CPU-only rendering.
///
/// This wraps the `softbuffer` surface lifecycle, providing a simple
/// `present` API that takes an RGBA pixel buffer and uploads it to the
/// window. The presenter handles surface configuration, resize, and
/// buffer mutation.
///
/// # Type Parameters
///
/// * `D` — The display handle type (implements `HasDisplayHandle`).
/// * `W` — The window handle type (implements `HasWindowHandle`).
///
/// # Examples
///
/// Creating a presenter requires a `softbuffer::Context` and a window handle,
/// so construction is shown with `no_run`:
///
/// ```no_run
/// use martensite_render::SoftbufferPresenter;
/// # use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
/// # fn example<D, W>(context: &softbuffer::Context<D>, window: W)
/// #     -> Result<(), Box<dyn std::error::Error>>
/// # where
/// #     D: HasDisplayHandle,
/// #     W: HasWindowHandle,
/// # {
/// let presenter = SoftbufferPresenter::new(context, window)?;
/// assert_eq!(presenter.width(), 0);
/// assert_eq!(presenter.height(), 0);
/// # Ok(())
/// # }
/// ```
pub struct SoftbufferPresenter<D, W> {
    surface: softbuffer::Surface<D, W>,
    width: u32,
    height: u32,
}

impl<D, W> SoftbufferPresenter<D, W>
where
    D: raw_window_handle::HasDisplayHandle,
    W: raw_window_handle::HasWindowHandle,
{
    /// Creates a new presenter from an existing `softbuffer` context and
    /// window handle.
    ///
    /// The surface is created from the context and window handle.
    ///
    /// # Errors
    ///
    /// Returns [`PresentationError::Softbuffer`] if the surface cannot be
    /// created.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_render::SoftbufferPresenter;
    /// # use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
    /// # fn example<D, W>(context: &softbuffer::Context<D>, window: W)
    /// #     -> Result<(), Box<dyn std::error::Error>>
    /// # where
    /// #     D: HasDisplayHandle,
    /// #     W: HasWindowHandle,
    /// # {
    /// let presenter = SoftbufferPresenter::new(context, window)?;
    /// assert_eq!(presenter.width(), 0);
    /// # Ok(())
    /// # }
    /// ```
    pub fn new(context: &softbuffer::Context<D>, window: W) -> Result<Self, PresentationError> {
        let surface = softbuffer::Surface::new(context, window)
            .map_err(|e| PresentationError::Softbuffer(e.to_string()))?;
        Ok(Self {
            surface,
            width: 0,
            height: 0,
        })
    }

    /// Creates a presenter from an already-created `softbuffer::Surface`.
    ///
    /// This is useful when the caller wants to manage surface creation
    /// themselves (e.g. for testing or custom window integration).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_render::SoftbufferPresenter;
    /// # use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
    /// # fn example<D, W>(surface: softbuffer::Surface<D, W>)
    /// # where
    /// #     D: HasDisplayHandle,
    /// #     W: HasWindowHandle,
    /// # {
    /// let presenter = SoftbufferPresenter::from_surface(surface);
    /// assert_eq!(presenter.width(), 0);
    /// assert_eq!(presenter.height(), 0);
    /// # }
    /// ```
    #[must_use]
    pub fn from_surface(surface: softbuffer::Surface<D, W>) -> Self {
        Self {
            surface,
            width: 0,
            height: 0,
        }
    }

    /// Configures the surface dimensions for presentation.
    ///
    /// This should be called whenever the window is resized.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_render::SoftbufferPresenter;
    /// # use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
    /// # fn example<D, W>(mut presenter: SoftbufferPresenter<D, W>)
    /// # where
    /// #     D: HasDisplayHandle,
    /// #     W: HasWindowHandle,
    /// # {
    /// presenter.resize(800, 600);
    /// assert_eq!(presenter.width(), 800);
    /// assert_eq!(presenter.height(), 600);
    /// # }
    /// ```
    pub fn resize(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
    }

    /// Presents an RGBA pixel buffer to the softbuffer surface.
    ///
    /// The buffer must be in RGBA8 format with dimensions matching the
    /// last [`resize`](Self::resize) call.
    ///
    /// # Errors
    ///
    /// Returns [`PresentationError`] if the buffer is malformed or the
    /// softbuffer surface operation fails.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_render::SoftbufferPresenter;
    /// # use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
    /// # fn example<D, W>(mut presenter: SoftbufferPresenter<D, W>)
    /// # where
    /// #     D: HasDisplayHandle,
    /// #     W: HasWindowHandle,
    /// # {
    /// presenter.resize(2, 2);
    /// // Present a 2x2 grey RGBA buffer.
    /// let rgba = vec![128u8; 2 * 2 * 4];
    /// presenter.present(&rgba).expect("presentation succeeds");
    /// # }
    /// ```
    pub fn present(&mut self, rgba_buffer: &[u8]) -> Result<(), PresentationError> {
        if self.width == 0 || self.height == 0 {
            return Err(PresentationError::NotConfigured);
        }
        let pixels = present_rgba_to_softbuffer(rgba_buffer, self.width, self.height)?;
        self.surface
            .resize(
                NonZeroU32::new(self.width).ok_or(PresentationError::ZeroDimension)?,
                NonZeroU32::new(self.height).ok_or(PresentationError::ZeroDimension)?,
            )
            .map_err(|e| PresentationError::Softbuffer(e.to_string()))?;
        let mut buffer = self
            .surface
            .buffer_mut()
            .map_err(|e| PresentationError::Softbuffer(e.to_string()))?;
        buffer.copy_from_slice(&pixels);
        buffer
            .present()
            .map_err(|e| PresentationError::Softbuffer(e.to_string()))?;
        Ok(())
    }

    /// Returns the current configured surface width.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_render::SoftbufferPresenter;
    /// # use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
    /// # fn example<D, W>(mut presenter: SoftbufferPresenter<D, W>)
    /// # where
    /// #     D: HasDisplayHandle,
    /// #     W: HasWindowHandle,
    /// # {
    /// presenter.resize(640, 480);
    /// assert_eq!(presenter.width(), 640);
    /// # }
    /// ```
    #[must_use]
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Returns the current configured surface height.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_render::SoftbufferPresenter;
    /// # use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
    /// # fn example<D, W>(mut presenter: SoftbufferPresenter<D, W>)
    /// # where
    /// #     D: HasDisplayHandle,
    /// #     W: HasWindowHandle,
    /// # {
    /// presenter.resize(640, 480);
    /// assert_eq!(presenter.height(), 480);
    /// # }
    /// ```
    #[must_use]
    pub fn height(&self) -> u32 {
        self.height
    }
}

#[cfg(test)]
mod tests {
    use super::{nonzero, present_rgba_to_softbuffer, rgba_to_softbuffer, PresentationError};

    #[test]
    fn rgba_to_softbuffer_converts_correctly() {
        let rgba = [255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255];
        let result = rgba_to_softbuffer(&rgba).expect("conversion succeeds");
        assert_eq!(result.len(), 3);
        assert_eq!(result[0], 0x00FF_0000);
        assert_eq!(result[1], 0x0000_FF00);
        assert_eq!(result[2], 0x0000_00FF);
    }

    #[test]
    fn rgba_to_softbuffer_discards_alpha() {
        let opaque = [255, 128, 64, 255];
        let transparent = [255, 128, 64, 0];
        let r1 = rgba_to_softbuffer(&opaque).expect("opaque");
        let r2 = rgba_to_softbuffer(&transparent).expect("transparent");
        assert_eq!(r1, r2, "alpha should be discarded");
    }

    #[test]
    fn rgba_to_softbuffer_empty_returns_error() {
        let result = rgba_to_softbuffer(&[]);
        assert!(matches!(result, Err(PresentationError::EmptyBuffer)));
    }

    #[test]
    fn present_with_zero_width_returns_error() {
        let result = present_rgba_to_softbuffer(&[1, 2, 3, 4], 0, 10);
        assert!(matches!(result, Err(PresentationError::ZeroDimension)));
    }

    #[test]
    fn present_with_zero_height_returns_error() {
        let result = present_rgba_to_softbuffer(&[1, 2, 3, 4], 10, 0);
        assert!(matches!(result, Err(PresentationError::ZeroDimension)));
    }

    #[test]
    fn present_with_matching_dimensions_succeeds() {
        let rgba = vec![128u8; 4 * 4 * 4];
        let result = present_rgba_to_softbuffer(&rgba, 4, 4);
        assert!(result.is_ok());
        assert_eq!(result.expect("pixels").len(), 16);
    }

    #[test]
    fn present_with_size_mismatch_returns_error() {
        let rgba = vec![128u8; 8];
        let result = present_rgba_to_softbuffer(&rgba, 4, 4);
        assert!(matches!(
            result,
            Err(PresentationError::SizeMismatch { .. })
        ));
    }

    #[test]
    fn nonzero_returns_value_for_nonzero() {
        let result = nonzero(42, PresentationError::NotConfigured);
        assert_eq!(result.expect("nonzero").get(), 42);
    }

    #[test]
    fn nonzero_returns_error_for_zero() {
        let result = nonzero(0, PresentationError::NotConfigured);
        assert!(matches!(result, Err(PresentationError::NotConfigured)));
    }

    #[test]
    fn presentation_error_display_formats_correctly() {
        assert_eq!(
            format!("{}", PresentationError::NotConfigured),
            "surface has not been configured"
        );
        assert_eq!(
            format!("{}", PresentationError::EmptyBuffer),
            "pixel buffer is empty"
        );
        assert_eq!(
            format!("{}", PresentationError::ZeroDimension),
            "surface width or height is zero"
        );
        let err = PresentationError::SizeMismatch {
            expected_width: 100,
            expected_height: 100,
            actual_width: 50,
            actual_height: 50,
        };
        assert!(format!("{err}").contains("50x50"));
        assert!(format!("{err}").contains("100x100"));
        let err = PresentationError::Softbuffer("test error".to_string());
        assert!(format!("{err}").contains("test error"));
    }
}
