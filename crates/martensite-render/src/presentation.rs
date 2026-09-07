//! CPU pixel buffer presentation helpers for `softbuffer`.
//!
//! When the GPU device is lost or unavailable, the [`TinySkiaBackend`]
//! renders into an RGBA CPU pixel buffer. This module provides conversion
//! utilities that prepare the buffer for upload to a window surface
//! via the `softbuffer` crate, enabling software-only rendering without
//! a GPU swapchain.
//!
//! [`TinySkiaBackend`]: crate::tinyskia_backend::TinySkiaBackend

use std::num::NonZeroU32;

/// Error returned by software presentation operations.
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
        }
    }
}

impl std::error::Error for PresentationError {}

/// Converts an RGBA pixel buffer (8 bits per channel, 4 bytes per pixel)
/// into the `X8R8G8B8` (u32) format expected by `softbuffer`.
///
/// This function performs the color channel swizzle from RGBA byte order
/// to the packed 0xRRGGBB u32 format, discarding the alpha channel
/// (softbuffer surfaces are opaque by default).
///
/// # Errors
///
/// Returns [`PresentationError::EmptyBuffer`] if the input buffer is empty.
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

/// Presents an RGBA pixel buffer to a `softbuffer` surface.
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
pub fn nonzero(value: u32, err: PresentationError) -> Result<NonZeroU32, PresentationError> {
    NonZeroU32::new(value).ok_or(err)
}

#[cfg(test)]
mod tests {
    use super::{nonzero, present_rgba_to_softbuffer, rgba_to_softbuffer, PresentationError};

    #[test]
    fn rgba_to_softbuffer_converts_correctly() {
        let rgba = [255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255];
        let result = rgba_to_softbuffer(&rgba).expect("conversion succeeds");
        assert_eq!(result.len(), 3);
        // Red: 0x00FF0000
        assert_eq!(result[0], 0x00FF_0000);
        // Green: 0x0000FF00
        assert_eq!(result[1], 0x0000_FF00);
        // Blue: 0x000000FF
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
        let rgba = vec![128u8; 4 * 4 * 4]; // 4x4 RGBA
        let result = present_rgba_to_softbuffer(&rgba, 4, 4);
        assert!(result.is_ok());
        assert_eq!(result.expect("pixels").len(), 16);
    }

    #[test]
    fn present_with_size_mismatch_returns_error() {
        let rgba = vec![128u8; 8]; // only 2 pixels, not 4x4
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
    }
}
