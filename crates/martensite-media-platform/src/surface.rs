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

/// Color range quantization of the video signal.
///
/// # Examples
///
/// ```
/// use martensite_media_platform::surface::ColorRange;
///
/// assert_eq!(ColorRange::default(), ColorRange::Limited);
/// ```
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum ColorRange {
    /// Limited/Video range (e.g. Y: `[16, 235]`, UV: `[16, 240]` for 8-bit).
    #[default]
    Limited,
    /// Full/PC range (`[0, 255]` for 8-bit, `[0, 1023]` for 10-bit).
    Full,
}

impl ColorRange {
    /// Expands a raw normalized luma sample $Y_{\text{raw}} \in [0, 1]$ to full dynamic range $[0, 1]$.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media_platform::surface::ColorRange;
    ///
    /// let range = ColorRange::Limited;
    /// let black = 16.0 / 255.0;
    /// let white = 235.0 / 255.0;
    /// assert!((range.expand_luma_8bit(black) - 0.0).abs() < 1e-5);
    /// assert!((range.expand_luma_8bit(white) - 1.0).abs() < 1e-5);
    /// ```
    #[inline]
    #[must_use]
    pub fn expand_luma_8bit(&self, raw: f32) -> f32 {
        match self {
            Self::Full => raw.clamp(0.0, 1.0),
            Self::Limited => {
                let y_min = 16.0 / 255.0;
                let y_max = 235.0 / 255.0;
                ((raw - y_min) / (y_max - y_min)).clamp(0.0, 1.0)
            }
        }
    }

    /// Expands a 10-bit normalized luma sample $Y_{\text{raw}} \in [0, 1]$ to full dynamic range $[0, 1]$.
    ///
    /// In 10-bit limited range, black is code 64 and peak white is code 940 (range 876).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media_platform::surface::ColorRange;
    ///
    /// let range = ColorRange::Limited;
    /// let black = 64.0 / 1023.0;
    /// let white = 940.0 / 1023.0;
    /// assert!((range.expand_luma_10bit(black) - 0.0).abs() < 1e-5);
    /// assert!((range.expand_luma_10bit(white) - 1.0).abs() < 1e-5);
    /// ```
    #[inline]
    #[must_use]
    pub fn expand_luma_10bit(&self, raw: f32) -> f32 {
        match self {
            Self::Full => raw.clamp(0.0, 1.0),
            Self::Limited => {
                let y_min = 64.0 / 1023.0;
                let y_max = 940.0 / 1023.0;
                ((raw - y_min) / (y_max - y_min)).clamp(0.0, 1.0)
            }
        }
    }

    /// Expands a raw normalized chroma sample $C_{\text{raw}} \in [0, 1]$ to centered chroma $[-0.5, 0.5]$.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media_platform::surface::ColorRange;
    ///
    /// let range = ColorRange::Limited;
    /// let mid = 128.0 / 255.0;
    /// assert!((range.expand_chroma_8bit(mid) - 0.0).abs() < 1e-5);
    /// ```
    #[inline]
    #[must_use]
    pub fn expand_chroma_8bit(&self, raw: f32) -> f32 {
        match self {
            Self::Full => (raw - 0.5).clamp(-0.5, 0.5),
            Self::Limited => {
                let c_min = 16.0 / 255.0;
                let c_max = 240.0 / 255.0;
                (((raw - c_min) / (c_max - c_min)) - 0.5).clamp(-0.5, 0.5)
            }
        }
    }

    /// Expands a 10-bit normalized chroma sample $C_{\text{raw}} \in [0, 1]$ to centered chroma $[-0.5, 0.5]$.
    ///
    /// In 10-bit limited range, neutral chroma is code 512, range 896 (`[64, 960]`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media_platform::surface::ColorRange;
    ///
    /// let range = ColorRange::Limited;
    /// let mid = 512.0 / 1023.0;
    /// assert!((range.expand_chroma_10bit(mid) - 0.0).abs() < 1e-5);
    /// ```
    #[inline]
    #[must_use]
    pub fn expand_chroma_10bit(&self, raw: f32) -> f32 {
        match self {
            Self::Full => (raw - 0.5).clamp(-0.5, 0.5),
            Self::Limited => {
                let c_min = 64.0 / 1023.0;
                let c_max = 960.0 / 1023.0;
                (((raw - c_min) / (c_max - c_min)) - 0.5).clamp(-0.5, 0.5)
            }
        }
    }
}

/// Frame metadata accompanying a decoded video frame or surface.
#[derive(Clone, Debug, PartialEq)]
pub struct VideoFrameMetadata {
    /// Video width in pixels.
    pub width: u32,
    /// Video height in pixels.
    pub height: u32,
    /// Pixel format of the video frame.
    pub format: VideoPixelFormat,
    /// Color range encoding.
    pub range: ColorRange,
    /// Presentation timestamp (PTS) in nanoseconds.
    pub pts_nanos: u64,
    /// Frame duration in nanoseconds.
    pub duration_nanos: u64,
    /// Monotonically increasing frame index.
    pub frame_index: u64,
}

impl VideoFrameMetadata {
    /// Creates a new `VideoFrameMetadata` instance.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media_platform::surface::{
    ///     ColorRange, VideoFrameMetadata, VideoPixelFormat,
    /// };
    ///
    /// let meta = VideoFrameMetadata::new(3840, 2160, VideoPixelFormat::P010, ColorRange::Limited);
    /// assert_eq!(meta.width, 3840);
    /// assert_eq!(meta.height, 2160);
    /// ```
    #[inline]
    #[must_use]
    pub fn new(width: u32, height: u32, format: VideoPixelFormat, range: ColorRange) -> Self {
        Self {
            width,
            height,
            format,
            range,
            pts_nanos: 0,
            duration_nanos: 16_666_667, // default ~60 fps
            frame_index: 0,
        }
    }

    /// Creates a new `VideoFrameMetadata` instance, validating non-zero buffer dimensions.
    ///
    /// # Errors
    ///
    /// Returns [`MediaError::InvalidBufferDimensions`] if `width == 0` or `height == 0`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media_platform::surface::{
    ///     ColorRange, VideoFrameMetadata, VideoPixelFormat,
    /// };
    ///
    /// let meta = VideoFrameMetadata::try_new(1920, 1080, VideoPixelFormat::Nv12, ColorRange::Limited);
    /// assert!(meta.is_ok());
    ///
    /// let err = VideoFrameMetadata::try_new(0, 1080, VideoPixelFormat::Nv12, ColorRange::Limited);
    /// assert!(err.is_err());
    /// ```
    pub fn try_new(
        width: u32,
        height: u32,
        format: VideoPixelFormat,
        range: ColorRange,
    ) -> Result<Self, MediaError> {
        if width == 0 || height == 0 {
            return Err(MediaError::InvalidBufferDimensions { width, height });
        }
        Ok(Self::new(width, height, format, range))
    }
}
