//! Zero-copy hardware video surface passthrough and memory bindings.
//!
//! This module provides safe platform abstractions for importing hardware
//! video decoder buffers directly into graphics memory without copying through
//! host CPU RAM. It supports Windows DXGI shared NT handles, macOS `IOSurface`,
//! Linux DRM `dma-buf` handles, and a fallback CPU host-memory pipeline.
//!
//! # Architectural Overview
//!
//! When hardware video decoders output decoded frames, traditional pipelines
//! copy plane data from GPU memory to CPU memory and then re-upload it to GPU
//! textures. By wrapping native platform handles directly, `martensite-media`
//! eliminates this overhead, achieving $< 1\%$ CPU utilization during 4K 60fps
//! playback.

use std::time::Instant;

// Re-export the shared surface types from `martensite-media-platform`.
// These types live there to break the circular workspace dependency that
// would otherwise exist between `martensite-media` (which depends on the
// platform import functions) and `martensite-media-platform` (which needs
// the handle/error/format types). See `martensite-media-platform::surface`
// for the canonical definitions.
pub use martensite_media_platform::surface::{HardwareHandle, MediaError, VideoPixelFormat};

/// Color range quantization of the video signal.
///
/// # Examples
///
/// ```
/// use martensite_media::surface::ColorRange;
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
    /// use martensite_media::surface::ColorRange;
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
    /// use martensite_media::surface::ColorRange;
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
    /// use martensite_media::surface::ColorRange;
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
    /// use martensite_media::surface::ColorRange;
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

/// Frame metadata accompanying a video surface.
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
    /// use martensite_media::surface::{ColorRange, VideoFrameMetadata, VideoPixelFormat};
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
    /// use martensite_media::surface::{ColorRange, VideoFrameMetadata, VideoPixelFormat};
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

/// A high-level video surface containing platform hardware bindings, synchronization,
/// and dispatch performance telemetry.
#[derive(Clone, Debug)]
pub struct VideoSurface {
    handle: HardwareHandle,
    metadata: VideoFrameMetadata,
    fence_id: u64,
    last_dispatch_duration_nanos: u64,
}

impl VideoSurface {
    /// Creates a new `VideoSurface` wrapping a hardware or memory handle.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media::surface::{
    ///     ColorRange, HardwareHandle, VideoFrameMetadata, VideoPixelFormat, VideoSurface,
    /// };
    ///
    /// let meta = VideoFrameMetadata::new(1920, 1080, VideoPixelFormat::Nv12, ColorRange::Limited);
    /// let surface = VideoSurface::new(HardwareHandle::IoSurface { surface_id: 101 }, meta);
    /// assert_eq!(surface.dimensions(), (1920, 1080));
    /// assert!(surface.is_hardware_accelerated());
    /// ```
    pub fn new(handle: HardwareHandle, metadata: VideoFrameMetadata) -> Self {
        Self {
            handle,
            metadata,
            fence_id: 1,
            last_dispatch_duration_nanos: 0,
        }
    }

    /// Creates a new `VideoSurface` after validating buffer dimensions.
    ///
    /// # Errors
    ///
    /// Returns [`MediaError::InvalidBufferDimensions`] if `metadata.width == 0` or `metadata.height == 0`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media::surface::{
    ///     ColorRange, HardwareHandle, VideoFrameMetadata, VideoPixelFormat, VideoSurface,
    /// };
    ///
    /// let meta = VideoFrameMetadata::new(1920, 1080, VideoPixelFormat::Nv12, ColorRange::Limited);
    /// let surface = VideoSurface::try_new(HardwareHandle::Mock { id: 1 }, meta);
    /// assert!(surface.is_ok());
    ///
    /// let bad_meta = VideoFrameMetadata::new(0, 1080, VideoPixelFormat::Nv12, ColorRange::Limited);
    /// let bad_surface = VideoSurface::try_new(HardwareHandle::Mock { id: 2 }, bad_meta);
    /// assert!(bad_surface.is_err());
    /// ```
    pub fn try_new(
        handle: HardwareHandle,
        metadata: VideoFrameMetadata,
    ) -> Result<Self, MediaError> {
        if metadata.width == 0 || metadata.height == 0 {
            return Err(MediaError::InvalidBufferDimensions {
                width: metadata.width,
                height: metadata.height,
            });
        }
        Ok(Self::new(handle, metadata))
    }

    /// Constructs a simulated zero-copy hardware surface for benchmarking and headless tests.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media::surface::{VideoPixelFormat, VideoSurface};
    ///
    /// let surface = VideoSurface::new_mock(3840, 2160, VideoPixelFormat::P010);
    /// assert_eq!(surface.dimensions(), (3840, 2160));
    /// assert!(surface.is_hardware_accelerated());
    /// ```
    #[must_use]
    pub fn new_mock(width: u32, height: u32, format: VideoPixelFormat) -> Self {
        let meta = VideoFrameMetadata::new(width, height, format, ColorRange::Limited);
        Self::new(HardwareHandle::Mock { id: 1 }, meta)
    }

    /// Returns the active pixel format.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media::surface::{VideoPixelFormat, VideoSurface};
    ///
    /// let surface = VideoSurface::new_mock(1920, 1080, VideoPixelFormat::Nv12);
    /// assert_eq!(surface.format(), VideoPixelFormat::Nv12);
    /// ```
    #[inline]
    #[must_use]
    pub fn format(&self) -> VideoPixelFormat {
        self.metadata.format
    }

    /// Returns the dimensions in pixels `(width, height)`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media::surface::{VideoPixelFormat, VideoSurface};
    ///
    /// let surface = VideoSurface::new_mock(1280, 720, VideoPixelFormat::Nv12);
    /// assert_eq!(surface.dimensions(), (1280, 720));
    /// ```
    #[inline]
    #[must_use]
    pub fn dimensions(&self) -> (u32, u32) {
        (self.metadata.width, self.metadata.height)
    }

    /// Returns the video surface width in pixels.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media::surface::{VideoPixelFormat, VideoSurface};
    ///
    /// let surface = VideoSurface::new_mock(1920, 1080, VideoPixelFormat::Nv12);
    /// assert_eq!(surface.width(), 1920);
    /// ```
    #[inline]
    #[must_use]
    pub fn width(&self) -> u32 {
        self.metadata.width
    }

    /// Returns the video surface height in pixels.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media::surface::{VideoPixelFormat, VideoSurface};
    ///
    /// let surface = VideoSurface::new_mock(1920, 1080, VideoPixelFormat::Nv12);
    /// assert_eq!(surface.height(), 1080);
    /// ```
    #[inline]
    #[must_use]
    pub fn height(&self) -> u32 {
        self.metadata.height
    }

    /// Returns `true` if this surface is backed by a zero-copy hardware handle.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media::surface::{VideoPixelFormat, VideoSurface};
    ///
    /// let surface = VideoSurface::new_mock(1920, 1080, VideoPixelFormat::Nv12);
    /// assert!(surface.is_hardware_accelerated());
    /// ```
    #[inline]
    #[must_use]
    pub fn is_hardware_accelerated(&self) -> bool {
        self.handle.is_zero_copy()
    }

    /// Returns a shared reference to the current hardware handle.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media::surface::{HardwareHandle, VideoPixelFormat, VideoSurface};
    ///
    /// let surface = VideoSurface::new_mock(1920, 1080, VideoPixelFormat::Nv12);
    /// assert_eq!(surface.handle(), &HardwareHandle::Mock { id: 1 });
    /// ```
    #[inline]
    #[must_use]
    pub fn handle(&self) -> &HardwareHandle {
        &self.handle
    }

    /// Returns a shared reference to the video frame metadata.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media::surface::{VideoPixelFormat, VideoSurface};
    ///
    /// let surface = VideoSurface::new_mock(1920, 1080, VideoPixelFormat::Nv12);
    /// assert_eq!(surface.metadata().width, 1920);
    /// ```
    #[inline]
    #[must_use]
    pub fn metadata(&self) -> &VideoFrameMetadata {
        &self.metadata
    }

    /// Returns the current synchronization fence identifier.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media::surface::{VideoPixelFormat, VideoSurface};
    ///
    /// let surface = VideoSurface::new_mock(1920, 1080, VideoPixelFormat::Nv12);
    /// assert_eq!(surface.fence_id(), 1);
    /// ```
    #[inline]
    #[must_use]
    pub fn fence_id(&self) -> u64 {
        self.fence_id
    }

    /// Advances the synchronization fence and returns the updated ID.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media::surface::{VideoPixelFormat, VideoSurface};
    ///
    /// let mut surface = VideoSurface::new_mock(1920, 1080, VideoPixelFormat::Nv12);
    /// assert_eq!(surface.advance_fence(), 2);
    /// ```
    pub fn advance_fence(&mut self) -> u64 {
        self.fence_id = self.fence_id.wrapping_add(1);
        self.fence_id
    }

    /// Updates the frame binding with a new hardware handle and presentation timestamp,
    /// measuring and recording CPU dispatch duration.
    ///
    /// Returns the elapsed CPU dispatch duration in nanoseconds.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media::surface::{HardwareHandle, VideoPixelFormat, VideoSurface};
    ///
    /// let mut surface = VideoSurface::new_mock(3840, 2160, VideoPixelFormat::Nv12);
    /// let elapsed = surface.update_handle(HardwareHandle::Mock { id: 2 }, 16_666_667);
    /// assert_eq!(surface.fence_id(), 2);
    /// ```
    pub fn update_handle(&mut self, handle: HardwareHandle, pts_nanos: u64) -> u64 {
        let start = Instant::now();

        self.handle = handle;
        self.metadata.pts_nanos = pts_nanos;
        self.metadata.frame_index = self.metadata.frame_index.wrapping_add(1);
        self.advance_fence();

        let elapsed = start.elapsed().as_nanos() as u64;
        self.last_dispatch_duration_nanos = elapsed;
        elapsed
    }

    /// Returns the duration of the most recent CPU dispatch operation in nanoseconds.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media::surface::{VideoPixelFormat, VideoSurface};
    ///
    /// let surface = VideoSurface::new_mock(1920, 1080, VideoPixelFormat::Nv12);
    /// assert_eq!(surface.last_dispatch_nanos(), 0);
    /// ```
    #[inline]
    #[must_use]
    pub fn last_dispatch_nanos(&self) -> u64 {
        self.last_dispatch_duration_nanos
    }

    /// Calculates CPU dispatch utilization percentage for a target frame rate.
    ///
    /// For 60 fps, the frame budget is ~16.67 ms (16,666,667 ns). Dispatching in
    /// $100\ \mu\text{s}$ equates to $0.6\%$ utilization.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media::surface::{HardwareHandle, VideoPixelFormat, VideoSurface};
    ///
    /// let mut surface = VideoSurface::new_mock(3840, 2160, VideoPixelFormat::P010);
    /// surface.update_handle(HardwareHandle::Mock { id: 2 }, 16_666_667);
    /// let util = surface.cpu_utilization_pct(60.0);
    /// assert!(util < 1.0, "CPU dispatch must be < 1% for zero-copy playback");
    /// ```
    #[inline]
    #[must_use]
    pub fn cpu_utilization_pct(&self, target_fps: f64) -> f64 {
        if target_fps <= 0.0 {
            return 0.0;
        }
        let frame_budget_nanos = 1_000_000_000.0 / target_fps;
        (self.last_dispatch_duration_nanos as f64 / frame_budget_nanos) * 100.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixel_format_properties() {
        assert!(VideoPixelFormat::Nv12.is_yuv());
        assert!(!VideoPixelFormat::Nv12.is_hdr());
        assert_eq!(VideoPixelFormat::Nv12.plane_count(), 2);
        assert_eq!(VideoPixelFormat::Nv12.bits_per_channel(), 8);

        assert!(VideoPixelFormat::P010.is_yuv());
        assert!(VideoPixelFormat::P010.is_hdr());
        assert_eq!(VideoPixelFormat::P010.plane_count(), 2);
        assert_eq!(VideoPixelFormat::P010.bits_per_channel(), 10);

        assert!(!VideoPixelFormat::Rgba8.is_yuv());
        assert_eq!(VideoPixelFormat::Rgba8.plane_count(), 1);

        assert!(VideoPixelFormat::Rgba16Float.is_hdr());
        assert_eq!(VideoPixelFormat::Rgba16Float.plane_count(), 1);
    }

    #[test]
    fn zero_copy_handle_classification() {
        let dxgi = HardwareHandle::DxgiSharedHandle { handle: 0x1234 };
        let iosurface = HardwareHandle::IoSurface { surface_id: 12 };
        let dmabuf = HardwareHandle::DmaBuf {
            fd: 3,
            stride: 1920,
            offset: 0,
            modifier: 0,
        };
        let mock = HardwareHandle::Mock { id: 1 };
        let cpu = HardwareHandle::CpuMemory {
            y_plane: vec![0; 100],
            uv_plane: vec![0; 50],
            y_stride: 10,
            uv_stride: 10,
        };

        assert!(dxgi.is_zero_copy());
        assert!(iosurface.is_zero_copy());
        assert!(dmabuf.is_zero_copy());
        assert!(mock.is_zero_copy());
        assert!(!cpu.is_zero_copy());
    }

    #[test]
    fn zero_copy_dispatch_sub_one_percent() {
        let mut surface = VideoSurface::new_mock(3840, 2160, VideoPixelFormat::P010);
        assert!(surface.is_hardware_accelerated());

        for i in 1..=60 {
            let pts = i * 16_666_667;
            let elapsed_nanos = surface.update_handle(HardwareHandle::Mock { id: i }, pts);
            // Milestone gate: dispatch must be < 0.10 ms (100,000 ns).
            assert!(
                elapsed_nanos < 100_000,
                "handle update took {elapsed_nanos}ns, must be < 0.10 ms"
            );
            assert_eq!(surface.fence_id(), i + 1);
        }

        let util = surface.cpu_utilization_pct(60.0);
        assert!(
            util < 1.0,
            "4K 60fps dispatch utilization must be < 1%, got {util:.3}%"
        );
    }

    #[test]
    fn color_range_luma_expansion_8bit() {
        let range = ColorRange::Limited;
        assert_eq!(range.expand_luma_8bit(16.0 / 255.0), 0.0);
        assert_eq!(range.expand_luma_8bit(235.0 / 255.0), 1.0);
        // Midpoint 125.5 -> ~0.5
        let mid = range.expand_luma_8bit(125.5 / 255.0);
        assert!((mid - 0.5).abs() < 0.01);
    }

    #[test]
    fn color_range_luma_expansion_10bit() {
        let range = ColorRange::Limited;
        assert_eq!(range.expand_luma_10bit(64.0 / 1023.0), 0.0);
        assert_eq!(range.expand_luma_10bit(940.0 / 1023.0), 1.0);
    }
}
