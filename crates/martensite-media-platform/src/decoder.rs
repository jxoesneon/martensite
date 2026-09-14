//! Decoder wire types shared by every platform backend.
//!
//! These types live here (rather than in `martensite-media`) for the same
//! reason as [`crate::surface`]: `martensite-media` depends on this crate for
//! the import functions, so the decoder backends implemented here cannot name
//! types defined there without creating a circular workspace dependency.
//! `martensite-media::decoder` re-exports everything below and layers the safe
//! `VideoDecoder` trait, `MockDecoder`, and `FrameQueue` pacing on top.
//!
//! Platform backends live in `decoder/*` submodules gated behind `decoder-*`
//! cargo features:
//!
//! - `decoder/videotoolbox.rs` (macOS): `VTDecompressionSession` →
//!   `CVPixelBuffer` (IOSurface-backed) → [`HardwareHandle::IoSurface`].
//! - `decoder/mediafoundation.rs` (Windows): `IMFSourceReader` + D3D11 →
//!   shared `ID3D11Texture2D` → [`HardwareHandle::DxgiSharedHandle`].
//! - `decoder/vaapi.rs` (Linux): `cros-libva` → `vaExportSurfaceHandle` →
//!   [`HardwareHandle::DmaBuf`].
//! - `decoder/ffmpeg.rs` (all platforms): `ffmpeg-next` software fallback →
//!   [`HardwareHandle::CpuMemory`].

use crate::surface::{HardwareHandle, MediaError, VideoFrameMetadata};

#[cfg(all(feature = "decoder-videotoolbox", target_os = "macos"))]
mod av1;

#[cfg(all(feature = "decoder-videotoolbox", target_os = "macos"))]
pub mod videotoolbox;

#[cfg(all(feature = "decoder-mf", target_os = "windows"))]
pub mod mediafoundation;

#[cfg(all(feature = "decoder-vaapi", target_os = "linux"))]
pub mod vaapi;

#[cfg(feature = "decoder-ffmpeg")]
pub mod ffmpeg;

/// Video codec a decoder backend should negotiate.
///
/// # Examples
///
/// ```
/// use martensite_media_platform::decoder::VideoCodec;
///
/// assert_eq!(VideoCodec::H264.fourcc_hint(), "avc1");
/// ```
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum VideoCodec {
    /// H.264 / AVC (`avc1`/`avcC` extradata, or Annex-B byte stream).
    H264,
    /// H.265 / HEVC (`hvc1`/`hvcC` extradata).
    Hevc,
    /// AV1 (`av1C` extradata, `av01` sample entry).
    Av1,
    /// VP9 (`vpcC` extradata, `vp09` sample entry).
    Vp9,
}

impl VideoCodec {
    /// A representative codec-string hint (`codecs=` parameter style).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media_platform::decoder::VideoCodec;
    ///
    /// assert_eq!(VideoCodec::Hevc.fourcc_hint(), "hvc1");
    /// ```
    #[must_use]
    pub fn fourcc_hint(&self) -> &'static str {
        match self {
            Self::H264 => "avc1",
            Self::Hevc => "hvc1",
            Self::Av1 => "av01",
            Self::Vp9 => "vp09",
        }
    }
}

/// Which concrete backend produced a decoded frame, for telemetry.
///
/// # Examples
///
/// ```
/// use martensite_media_platform::decoder::DecoderBackend;
///
/// assert!(DecoderBackend::VideoToolbox.is_hardware());
/// assert!(!DecoderBackend::Software.is_hardware());
/// ```
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum DecoderBackend {
    /// macOS VideoToolbox (`VTDecompressionSession`).
    VideoToolbox,
    /// Windows Media Foundation (`IMFSourceReader` + D3D11).
    MediaFoundation,
    /// Linux VAAPI (`libva` / `vaExportSurfaceHandle`).
    Vaapi,
    /// `ffmpeg-next` software decode (CPU memory output).
    FfmpegSoftware,
    /// Any other software/CPU path.
    Software,
    /// Synthetic decoder used by tests.
    Mock,
}

impl DecoderBackend {
    /// Returns `true` when the backend performs decode on dedicated hardware.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media_platform::decoder::DecoderBackend;
    ///
    /// assert!(DecoderBackend::MediaFoundation.is_hardware());
    /// assert!(!DecoderBackend::Mock.is_hardware());
    /// ```
    #[must_use]
    pub fn is_hardware(&self) -> bool {
        matches!(
            self,
            Self::VideoToolbox | Self::MediaFoundation | Self::Vaapi
        )
    }
}

/// A single compressed access unit fed to a decoder.
///
/// The bitstream framing (Annex-B start codes vs length-prefixed NALs) is
/// dictated by the backend and [`DecoderConfig::codec_config`]; decoders
/// document which framing they accept.
///
/// # Examples
///
/// ```
/// use martensite_media_platform::decoder::EncodedPacket;
///
/// let pkt = EncodedPacket::new(vec![0, 0, 0, 1, 0x67], 33_333_334, 33_333_333);
/// assert!(pkt.is_keyframe);
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct EncodedPacket {
    /// Compressed elementary-stream bytes for one access unit.
    pub data: Vec<u8>,
    /// Presentation timestamp in nanoseconds.
    pub pts_nanos: u64,
    /// Packet duration in nanoseconds (0 if unknown).
    pub duration_nanos: u64,
    /// Whether this packet starts with an IDR/keyframe.
    pub is_keyframe: bool,
}

impl EncodedPacket {
    /// Creates a packet marked as a keyframe.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media_platform::decoder::EncodedPacket;
    ///
    /// let pkt = EncodedPacket::new(vec![1, 2, 3], 0, 16_666_667);
    /// assert_eq!(pkt.pts_nanos, 0);
    /// ```
    #[must_use]
    pub fn new(data: Vec<u8>, pts_nanos: u64, duration_nanos: u64) -> Self {
        Self {
            data,
            pts_nanos,
            duration_nanos,
            is_keyframe: true,
        }
    }

    /// Marks the packet as a non-keyframe (delta frame).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media_platform::decoder::EncodedPacket;
    ///
    /// let pkt = EncodedPacket::new(vec![1], 0, 0).delta();
    /// assert!(!pkt.is_keyframe);
    /// ```
    #[must_use]
    pub fn delta(mut self) -> Self {
        self.is_keyframe = false;
        self
    }
}

/// Decoder construction parameters.
///
/// # Examples
///
/// ```
/// use martensite_media_platform::decoder::{DecoderConfig, VideoCodec};
///
/// let config = DecoderConfig::new(VideoCodec::H264, 1920, 1080);
/// assert_eq!(config.decode_ahead, 3);
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct DecoderConfig {
    /// Codec to negotiate.
    pub codec: VideoCodec,
    /// Expected coded width in pixels (0 = derive from the stream).
    pub width: u32,
    /// Expected coded height in pixels (0 = derive from the stream).
    pub height: u32,
    /// Reorder buffer / decode-ahead depth in frames (2–3 recommended).
    pub decode_ahead: usize,
    /// Whether the backend may fall back to software decode when hardware
    /// is unavailable.
    pub allow_software: bool,
    /// Codec extradata (`avcC`/`hvcC`/`av1C`/`vpcC` record) when the
    /// container supplies it out-of-band; `None` for in-band Annex-B.
    pub codec_config: Option<Vec<u8>>,
}

impl DecoderConfig {
    /// Creates a config for the given codec and coded size.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media_platform::decoder::{DecoderConfig, VideoCodec};
    ///
    /// let config = DecoderConfig::new(VideoCodec::Hevc, 3840, 2160);
    /// assert!(config.allow_software);
    /// ```
    #[must_use]
    pub fn new(codec: VideoCodec, width: u32, height: u32) -> Self {
        Self {
            codec,
            width,
            height,
            decode_ahead: 3,
            allow_software: true,
            codec_config: None,
        }
    }

    /// Builder: attach codec extradata.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media_platform::decoder::{DecoderConfig, VideoCodec};
    ///
    /// let config = DecoderConfig::new(VideoCodec::H264, 640, 480)
    ///     .with_codec_config(vec![0x01, 0x64]);
    /// assert!(config.codec_config.is_some());
    /// ```
    #[must_use]
    pub fn with_codec_config(mut self, record: Vec<u8>) -> Self {
        self.codec_config = Some(record);
        self
    }
}

/// Raw HDR side-data attached to a decoded frame by a platform backend.
///
/// This is the untyped carrier produced by VideoToolbox
/// `CMFormatDescription` extensions, Media Foundation `MF_MT_*` attributes,
/// VAAPI parsed SEI messages, or FFmpeg `AV_FRAME_DATA_*` side data.
/// `martensite-media::hdr::HdrMetadata` converts it into the typed,
/// uniform-ready representation.
///
/// # Examples
///
/// ```
/// use martensite_media_platform::decoder::HdrSideData;
///
/// let side = HdrSideData::pq_bt2020(1000.0);
/// assert_eq!(side.eotf_code, 16);
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct HdrSideData {
    /// Transfer-characteristic code (SMPTE ST 2086 / ISO 23001-8 numbering):
    /// 1 = BT.709/SDR, 16 = PQ (ST 2084), 18 = HLG. Other values are passed
    /// through for forward compatibility.
    pub eotf_code: u16,
    /// Colour-primaries code (ISO 23001-8): 1 = BT.709, 9 = BT.2020.
    pub primaries_code: u16,
    /// Whether the signal uses full-range quantization.
    pub full_range: bool,
    /// Mastering-display maximum luminance in nits, if signalled.
    pub max_luminance_nits: Option<f32>,
    /// Mastering-display minimum luminance in nits, if signalled.
    pub min_luminance_nits: Option<f32>,
    /// `MaxCLL` content light level in nits, if signalled.
    pub max_cll: Option<u16>,
    /// `MaxFALL` frame-average light level in nits, if signalled.
    pub max_fall: Option<u16>,
    /// HDR10+ / dynamic metadata payload (SMPTE ST 2094-40), if present.
    pub dynamic_metadata: Option<Vec<u8>>,
}

impl HdrSideData {
    /// A PQ / BT.2020 HDR10 signal with the given mastering peak.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media_platform::decoder::HdrSideData;
    ///
    /// let side = HdrSideData::pq_bt2020(1000.0);
    /// assert_eq!(side.primaries_code, 9);
    /// assert_eq!(side.max_luminance_nits, Some(1000.0));
    /// ```
    #[must_use]
    pub fn pq_bt2020(max_luminance_nits: f32) -> Self {
        Self {
            eotf_code: 16,
            primaries_code: 9,
            full_range: false,
            max_luminance_nits: Some(max_luminance_nits),
            min_luminance_nits: Some(0.005),
            max_cll: None,
            max_fall: None,
            dynamic_metadata: None,
        }
    }

    /// An HLG / BT.2020 broadcast HDR signal.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media_platform::decoder::HdrSideData;
    ///
    /// let side = HdrSideData::hlg_bt2020();
    /// assert_eq!(side.eotf_code, 18);
    /// ```
    #[must_use]
    pub fn hlg_bt2020() -> Self {
        Self {
            eotf_code: 18,
            primaries_code: 9,
            full_range: false,
            max_luminance_nits: None,
            min_luminance_nits: None,
            max_cll: None,
            max_fall: None,
            dynamic_metadata: None,
        }
    }
}

/// One decoded output frame: a hardware (or CPU-fallback) surface handle plus
/// presentation metadata.
///
/// # Examples
///
/// ```
/// use martensite_media_platform::decoder::DecodedFrame;
/// use martensite_media_platform::surface::{
///     ColorRange, HardwareHandle, VideoFrameMetadata, VideoPixelFormat,
/// };
///
/// let meta = VideoFrameMetadata::new(1920, 1080, VideoPixelFormat::Nv12, ColorRange::Limited);
/// let frame = DecodedFrame::new(HardwareHandle::Mock { id: 7 }, meta);
/// assert_eq!(frame.metadata.width, 1920);
/// assert!(frame.hdr.is_none());
/// ```
#[derive(Clone, Debug)]
pub struct DecodedFrame {
    /// Surface handle referencing the decoded pixel planes.
    pub handle: HardwareHandle,
    /// Frame dimensions, format, range and timestamps.
    pub metadata: VideoFrameMetadata,
    /// HDR side-data signalled with this frame, if any.
    pub hdr: Option<HdrSideData>,
}

impl DecodedFrame {
    /// Creates a decoded frame without HDR side-data.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media_platform::decoder::DecodedFrame;
    /// use martensite_media_platform::surface::{
    ///     ColorRange, HardwareHandle, VideoFrameMetadata, VideoPixelFormat,
    /// };
    ///
    /// let meta = VideoFrameMetadata::new(640, 480, VideoPixelFormat::Nv12, ColorRange::Full);
    /// let frame = DecodedFrame::new(HardwareHandle::Mock { id: 1 }, meta);
    /// assert!(frame.hdr.is_none());
    /// ```
    #[must_use]
    pub fn new(handle: HardwareHandle, metadata: VideoFrameMetadata) -> Self {
        Self {
            handle,
            metadata,
            hdr: None,
        }
    }

    /// Builder: attach HDR side-data.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media_platform::decoder::{DecodedFrame, HdrSideData};
    /// use martensite_media_platform::surface::{
    ///     ColorRange, HardwareHandle, VideoFrameMetadata, VideoPixelFormat,
    /// };
    ///
    /// let meta = VideoFrameMetadata::new(3840, 2160, VideoPixelFormat::P010, ColorRange::Limited);
    /// let frame = DecodedFrame::new(HardwareHandle::Mock { id: 1 }, meta)
    ///     .with_hdr(HdrSideData::pq_bt2020(1000.0));
    /// assert_eq!(frame.hdr.as_ref().map(|h| h.eotf_code), Some(16));
    /// ```
    #[must_use]
    pub fn with_hdr(mut self, hdr: HdrSideData) -> Self {
        self.hdr = Some(hdr);
        self
    }

    /// Returns `true` when the frame is backed by a zero-copy hardware handle.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media_platform::decoder::DecodedFrame;
    /// use martensite_media_platform::surface::{
    ///     ColorRange, HardwareHandle, VideoFrameMetadata, VideoPixelFormat,
    /// };
    ///
    /// let meta = VideoFrameMetadata::new(640, 480, VideoPixelFormat::Nv12, ColorRange::Limited);
    /// let hw = DecodedFrame::new(HardwareHandle::Mock { id: 1 }, meta.clone());
    /// let sw = DecodedFrame::new(
    ///     HardwareHandle::CpuMemory {
    ///         y_plane: vec![0; 4],
    ///         uv_plane: vec![0; 2],
    ///         y_stride: 2,
    ///         uv_stride: 2,
    ///     },
    ///     meta,
    /// );
    /// assert!(hw.is_zero_copy());
    /// assert!(!sw.is_zero_copy());
    /// ```
    #[must_use]
    pub fn is_zero_copy(&self) -> bool {
        self.handle.is_zero_copy()
    }
}

/// Rolling decode-path telemetry counters.
///
/// # Examples
///
/// ```
/// use martensite_media_platform::decoder::DecodeStats;
///
/// let mut stats = DecodeStats::default();
/// stats.record_packet(1_500);
/// stats.record_frame(2_000_000);
/// assert_eq!(stats.packets_received, 1);
/// assert_eq!(stats.frames_decoded, 1);
/// ```
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DecodeStats {
    /// Total packets accepted by `send_packet`.
    pub packets_received: u64,
    /// Total packets rejected by `send_packet` (queue full / wrong state).
    pub packets_rejected: u64,
    /// Total frames emitted by `try_recv_frame`.
    pub frames_decoded: u64,
    /// Sum of per-frame decode wall time in nanoseconds (backend-reported).
    pub total_decode_nanos: u64,
    /// The backend performing the decode.
    pub backend: Option<DecoderBackend>,
}

impl DecodeStats {
    /// Records one accepted packet of `bytes` length.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media_platform::decoder::DecodeStats;
    ///
    /// let mut s = DecodeStats::default();
    /// s.record_packet(100);
    /// assert_eq!(s.packets_received, 1);
    /// ```
    pub fn record_packet(&mut self, _bytes: usize) {
        self.packets_received = self.packets_received.saturating_add(1);
    }

    /// Records one rejected packet.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media_platform::decoder::DecodeStats;
    ///
    /// let mut s = DecodeStats::default();
    /// s.record_rejection();
    /// assert_eq!(s.packets_rejected, 1);
    /// ```
    pub fn record_rejection(&mut self) {
        self.packets_rejected = self.packets_rejected.saturating_add(1);
    }

    /// Records one emitted frame with its decode time.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media_platform::decoder::DecodeStats;
    ///
    /// let mut s = DecodeStats::default();
    /// s.record_frame(4_000_000);
    /// s.record_frame(2_000_000);
    /// assert_eq!(s.average_decode_nanos(), 3_000_000);
    /// ```
    pub fn record_frame(&mut self, decode_nanos: u64) {
        self.frames_decoded = self.frames_decoded.saturating_add(1);
        self.total_decode_nanos = self.total_decode_nanos.saturating_add(decode_nanos);
    }

    /// Mean decode wall time per frame in nanoseconds (0 when no frames).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media_platform::decoder::DecodeStats;
    ///
    /// let mut s = DecodeStats::default();
    /// assert_eq!(s.average_decode_nanos(), 0);
    /// s.record_frame(10);
    /// s.record_frame(20);
    /// assert_eq!(s.average_decode_nanos(), 15);
    /// ```
    #[must_use]
    pub fn average_decode_nanos(&self) -> u64 {
        self.total_decode_nanos
            .checked_div(self.frames_decoded)
            .unwrap_or(0)
    }

    /// Returns `true` when the active backend decodes on dedicated hardware.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media_platform::decoder::{DecoderBackend, DecodeStats};
    ///
    /// let mut s = DecodeStats::default();
    /// s.backend = Some(DecoderBackend::Vaapi);
    /// assert!(s.hardware_accelerated());
    /// ```
    #[must_use]
    pub fn hardware_accelerated(&self) -> bool {
        self.backend.is_some_and(|b| b.is_hardware())
    }
}

/// Error raised when a packet stream is fed to a decoder in an illegal order
/// or after a fatal decode failure. Kept distinct from
/// [`MediaError::ImportFailed`] so callers can tell decode faults apart from
/// surface-import faults.
///
/// # Examples
///
/// ```
/// use martensite_media_platform::decoder::DecodeError;
///
/// let err = DecodeError::StreamCorrupt("missing SPS".to_string());
/// assert!(err.to_string().contains("missing SPS"));
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DecodeError {
    /// The compressed stream is malformed for the negotiated codec.
    StreamCorrupt(String),
    /// The requested codec or profile is unsupported by this backend.
    UnsupportedCodec(String),
    /// The decoder hit an unrecoverable backend failure; re-create it.
    Fatal(String),
    /// A non-keyframe packet arrived before the decoder had a reference
    /// frame (start-of-stream or post-flush).
    NeedsKeyframe,
}

impl core::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::StreamCorrupt(m) => write!(f, "corrupt stream: {m}"),
            Self::UnsupportedCodec(m) => write!(f, "unsupported codec: {m}"),
            Self::Fatal(m) => write!(f, "fatal decoder error: {m}"),
            Self::NeedsKeyframe => f.write_str("decoder requires a keyframe before delta packets"),
        }
    }
}

impl std::error::Error for DecodeError {}

impl From<DecodeError> for MediaError {
    fn from(err: DecodeError) -> Self {
        match err {
            DecodeError::StreamCorrupt(m)
            | DecodeError::UnsupportedCodec(m)
            | DecodeError::Fatal(m) => MediaError::ImportFailed(m),
            DecodeError::NeedsKeyframe => MediaError::InvalidHandle,
        }
    }
}
