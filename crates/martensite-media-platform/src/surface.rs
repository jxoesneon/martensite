//! Shared video surface types used by both [`martensite_media`] and the
//! platform-specific import functions in this crate.
//!
//! These types live here (rather than in `martensite-media`) to break what
//! would otherwise be a circular workspace dependency:
//! `martensite-media` depends on `martensite-media-platform` for the import
//! functions, and `martensite-media-platform` needs the handle/error/format
//! types. By defining them here, `martensite-media-platform` has no
//! dependency on `martensite-media`, and `martensite-media` re-exports them
//! from its own `surface` module for backward compatibility.

use core::fmt;

/// Video surface errors.
///
/// # Examples
///
/// ```
/// use martensite_media_platform::surface::MediaError;
///
/// let err = MediaError::InvalidHandle;
/// assert!(err.to_string().contains("invalid"));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaError {
    /// The specified hardware handle is invalid, closed, or null.
    InvalidHandle,
    /// The pixel format is unsupported on the current adapter or pipeline.
    UnsupportedFormat(VideoPixelFormat),
    /// Zero-copy external memory import failed on the platform graphics driver.
    ImportFailed(String),
    /// Frame buffer dimensions or plane strides are non-positive or inconsistent.
    InvalidBufferDimensions {
        /// Buffer width in pixels.
        width: u32,
        /// Buffer height in pixels.
        height: u32,
    },
}

impl fmt::Display for MediaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidHandle => f.write_str("video hardware handle is invalid or closed"),
            Self::UnsupportedFormat(fmt) => write!(f, "pixel format {fmt:?} is unsupported"),
            Self::ImportFailed(msg) => write!(f, "hardware surface import failed: {msg}"),
            Self::InvalidBufferDimensions { width, height } => {
                write!(f, "invalid buffer dimensions: {width}x{height}")
            }
        }
    }
}

impl std::error::Error for MediaError {}

/// Pixel format of the video surface data.
///
/// # Examples
///
/// ```
/// use martensite_media_platform::surface::VideoPixelFormat;
///
/// assert_eq!(VideoPixelFormat::Nv12.plane_count(), 2);
/// assert!(VideoPixelFormat::P010.is_hdr());
/// ```
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum VideoPixelFormat {
    /// 8-bit bi-planar YUV 4:2:0 (Y plane 8-bit, interleaved UV plane 8-bit).
    Nv12,
    /// 10-bit bi-planar YUV 4:2:0 packed into 16-bit words (MSB aligned).
    P010,
    /// Standard 8-bit RGBA (32-bit unpacked).
    Rgba8,
    /// 16-bit floating point RGBA (scRGB / wide-gamut linear).
    Rgba16Float,
}

impl VideoPixelFormat {
    /// Returns `true` if this pixel format uses YUV planar encoding.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media_platform::surface::VideoPixelFormat;
    ///
    /// assert!(VideoPixelFormat::Nv12.is_yuv());
    /// assert!(VideoPixelFormat::P010.is_yuv());
    /// assert!(!VideoPixelFormat::Rgba8.is_yuv());
    /// ```
    #[inline]
    #[must_use]
    pub fn is_yuv(&self) -> bool {
        matches!(self, Self::Nv12 | Self::P010)
    }

    /// Returns `true` if this format provides high-dynamic-range (HDR) bit depth (>8 bits).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media_platform::surface::VideoPixelFormat;
    ///
    /// assert!(VideoPixelFormat::P010.is_hdr());
    /// assert!(VideoPixelFormat::Rgba16Float.is_hdr());
    /// assert!(!VideoPixelFormat::Nv12.is_hdr());
    /// ```
    #[inline]
    #[must_use]
    pub fn is_hdr(&self) -> bool {
        matches!(self, Self::P010 | Self::Rgba16Float)
    }

    /// Returns the number of color planes for this pixel format.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media_platform::surface::VideoPixelFormat;
    ///
    /// assert_eq!(VideoPixelFormat::Nv12.plane_count(), 2);
    /// assert_eq!(VideoPixelFormat::Rgba8.plane_count(), 1);
    /// ```
    #[inline]
    #[must_use]
    pub fn plane_count(&self) -> usize {
        match self {
            Self::Nv12 | Self::P010 => 2,
            Self::Rgba8 | Self::Rgba16Float => 1,
        }
    }

    /// Returns the effective bit depth per color channel.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media_platform::surface::VideoPixelFormat;
    ///
    /// assert_eq!(VideoPixelFormat::Nv12.bits_per_channel(), 8);
    /// assert_eq!(VideoPixelFormat::P010.bits_per_channel(), 10);
    /// assert_eq!(VideoPixelFormat::Rgba16Float.bits_per_channel(), 16);
    /// ```
    #[inline]
    #[must_use]
    pub fn bits_per_channel(&self) -> u32 {
        match self {
            Self::Nv12 | Self::Rgba8 => 8,
            Self::P010 => 10,
            Self::Rgba16Float => 16,
        }
    }
}

/// Safe abstraction of platform-specific hardware surface handles.
///
/// # Examples
///
/// ```
/// use martensite_media_platform::surface::HardwareHandle;
///
/// let h = HardwareHandle::Mock { id: 1 };
/// assert!(h.is_zero_copy());
/// ```
#[derive(Clone, Debug, PartialEq)]
pub enum HardwareHandle {
    /// Windows DXGI shared NT handle (`HANDLE` address stored as `usize`).
    DxgiSharedHandle {
        /// OS handle pointer address.
        handle: usize,
    },
    /// macOS `IOSurfaceRef` identifier (`IOSurfaceID` as `u32`).
    IoSurface {
        /// 32-bit global surface identifier.
        surface_id: u32,
    },
    /// Linux DRM `dma-buf` file descriptor with stride, offset, and modifier.
    DmaBuf {
        /// File descriptor number.
        fd: i32,
        /// Row stride in bytes.
        stride: u32,
        /// Plane byte offset.
        offset: u32,
        /// DRM format modifier (e.g. `DRM_FORMAT_MOD_LINEAR`).
        modifier: u64,
    },
    /// Mock hardware surface handle for headless execution, benchmarking, and CI testing.
    Mock {
        /// Synthetic handle identifier.
        id: u64,
    },
    /// Safe host-memory CPU fallback buffer for systems lacking hardware extensions.
    CpuMemory {
        /// Luminance plane byte buffer.
        y_plane: Vec<u8>,
        /// Interleaved chrominance plane byte buffer.
        uv_plane: Vec<u8>,
        /// Row stride of the luma plane.
        y_stride: u32,
        /// Row stride of the chroma plane.
        uv_stride: u32,
    },
}

impl HardwareHandle {
    /// Returns `true` if this handle represents a zero-copy hardware surface.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media_platform::surface::HardwareHandle;
    ///
    /// let h = HardwareHandle::IoSurface { surface_id: 42 };
    /// assert!(h.is_zero_copy());
    /// ```
    #[inline]
    #[must_use]
    pub fn is_zero_copy(&self) -> bool {
        matches!(
            self,
            Self::DxgiSharedHandle { .. }
                | Self::IoSurface { .. }
                | Self::DmaBuf { .. }
                | Self::Mock { .. }
        )
    }
}
