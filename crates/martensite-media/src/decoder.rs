//! Safe video decoder abstraction.
//!
//! This module defines the [`VideoDecoder`] trait every decode backend
//! implements, the [`MockDecoder`] used by the conformance suite and headless
//! CI, and re-exports the wire types that live in
//! `martensite-media-platform` (see that crate's `decoder` module for why the
//! definitions live there).
//!
//! Backend selection is feature-gated in `martensite-media-platform`:
//! `decoder-videotoolbox` (macOS), `decoder-mf` (Windows), `decoder-vaapi`
//! (Linux), `decoder-ffmpeg` (software fallback, all platforms). Each concrete
//! backend implements [`VideoDecoder`] via a `martensite-media`-local impl.

use crate::hdr::HdrMetadata;
use crate::surface::MediaError;
use martensite_media_platform::decoder::DecodeError;

// Re-export the decoder wire vocabulary so `martensite_media::decoder::*`
// matches the milestone specification exactly. `DecodedFrame` fields name
// the surface types, so they are re-exported alongside for convenience.
pub use martensite_media_platform::decoder::{
    DecodeStats, DecodedFrame, DecoderBackend, DecoderConfig, EncodedPacket, HdrSideData,
    VideoCodec,
};
pub use martensite_media_platform::surface::{
    ColorRange, HardwareHandle, VideoFrameMetadata, VideoPixelFormat,
};

#[cfg(feature = "decoder-ffmpeg")]
pub use martensite_media_platform::decoder::ffmpeg;
#[cfg(all(feature = "decoder-mf", target_os = "windows"))]
pub use martensite_media_platform::decoder::mediafoundation;
#[cfg(all(feature = "decoder-vaapi", target_os = "linux"))]
pub use martensite_media_platform::decoder::vaapi;
#[cfg(all(feature = "decoder-videotoolbox", target_os = "macos"))]
pub use martensite_media_platform::decoder::videotoolbox;

/// A hardware or software video decoder producing zero-copy frames.
///
/// The trait is push/pull: callers feed [`EncodedPacket`]s via
/// [`send_packet`](Self::send_packet) and drain [`DecodedFrame`]s via
/// [`try_recv_frame`](Self::try_recv_frame). Backends reorder frames into
/// presentation order internally.
///
/// `init` is an associated constructor so the trait remains the single
/// entry point the `martensite-media-test` conformance suite exercises; it
/// is deliberately not callable through `dyn VideoDecoder`.
///
/// The `Sync` bound exists because `MediaView` stores the decoder inside a
/// `Widget` (which requires `Send + Sync`). All mutating calls go through
/// `&mut self`, so shared `&self` access only ever reaches the read-only
/// `negotiated_format`/`hdr_metadata`/`stats` accessors — safe as long as
/// backends keep `&self` methods free of interior FFI mutation.
///
/// # Examples
///
/// ```
/// use martensite_media::decoder::{DecoderConfig, MockDecoder, VideoCodec, VideoDecoder};
///
/// let mut dec = MockDecoder::init(DecoderConfig::new(VideoCodec::H264, 640, 480)).unwrap();
/// assert!(dec.try_recv_frame().unwrap().is_none());
/// ```
pub trait VideoDecoder: Send + Sync {
    /// Constructs the decoder for the given configuration.
    ///
    /// # Errors
    ///
    /// Returns [`MediaError::ImportFailed`] (or a backend-specific error
    /// converted into it) when the backend cannot satisfy the requested
    /// codec — e.g. hardware decode unavailable and
    /// `config.allow_software == false`.
    fn init(config: DecoderConfig) -> Result<Self, MediaError>
    where
        Self: Sized;

    /// Feeds one compressed access unit to the decoder.
    ///
    /// # Errors
    ///
    /// Returns an error when the packet is rejected — e.g. a delta packet
    /// before any keyframe, a corrupt bitstream, or a fatal backend fault.
    fn send_packet(&mut self, packet: &EncodedPacket) -> Result<(), MediaError>;

    /// Attempts to take the next decoded frame in presentation order.
    ///
    /// Returns `Ok(None)` when the decoder needs more input or the reorder
    /// buffer has not yet released a frame.
    ///
    /// # Errors
    ///
    /// Returns an error on unrecoverable backend faults.
    fn try_recv_frame(&mut self) -> Result<Option<DecodedFrame>, MediaError>;

    /// Drops all queued packets and pending reorder state. The next packet
    /// after a flush must be a keyframe.
    ///
    /// # Errors
    ///
    /// Returns an error when the backend cannot reset its state.
    fn flush(&mut self) -> Result<(), MediaError> {
        Ok(())
    }

    /// The pixel format the decoder negotiated for output frames.
    fn negotiated_format(&self) -> VideoPixelFormat;

    /// HDR side-data attached to the current stream, if signalled.
    fn hdr_metadata(&self) -> Option<HdrMetadata>;

    /// Rolling decode-path telemetry.
    fn stats(&self) -> &DecodeStats;
}

/// A deterministic software `VideoDecoder` for tests and headless CI.
///
/// `MockDecoder` accepts any well-formed packet stream (enforcing only the
/// keyframe rule), synthesizes `HardwareHandle::Mock` frames with
/// monotonically increasing ids, and releases them after a configurable
/// packet delay to emulate reorder depth. Decode "time" is deterministic —
/// the stats counters advance by a fixed quantum per frame.
///
/// # Examples
///
/// ```
/// use martensite_media::decoder::{
///     DecoderConfig, EncodedPacket, MockDecoder, VideoCodec, VideoDecoder,
/// };
///
/// let mut dec = MockDecoder::init(DecoderConfig::new(VideoCodec::H264, 1920, 1080)).unwrap();
/// dec.send_packet(&EncodedPacket::new(vec![0x67], 0, 16_666_667)).unwrap();
/// let frame = dec.try_recv_frame().unwrap().expect("frame after keyframe");
/// assert!(frame.is_zero_copy());
/// ```
#[derive(Debug)]
pub struct MockDecoder {
    config: DecoderConfig,
    stats: DecodeStats,
    /// Packets received but not yet "decoded" (reorder latency).
    pending: std::collections::VecDeque<EncodedPacket>,
    /// Packet delay before a frame is emitted.
    latency: usize,
    format: VideoPixelFormat,
    hdr: Option<HdrMetadata>,
    next_frame_id: u64,
    seen_keyframe: bool,
}

impl MockDecoder {
    /// Creates a mock decoder with the given reorder latency in packets.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media::decoder::{DecoderConfig, MockDecoder, VideoCodec};
    ///
    /// let dec = MockDecoder::with_latency(DecoderConfig::new(VideoCodec::H264, 64, 64), 0);
    /// assert_eq!(dec.latency(), 0);
    /// ```
    #[must_use]
    pub fn with_latency(config: DecoderConfig, latency: usize) -> Self {
        let format = match config.codec {
            VideoCodec::H264 | VideoCodec::Vp9 => VideoPixelFormat::Nv12,
            VideoCodec::Hevc | VideoCodec::Av1 => VideoPixelFormat::P010,
        };
        Self {
            format,
            stats: DecodeStats {
                backend: Some(DecoderBackend::Mock),
                ..DecodeStats::default()
            },
            pending: std::collections::VecDeque::new(),
            latency,
            hdr: None,
            next_frame_id: 0,
            seen_keyframe: false,
            config,
        }
    }

    /// Returns the configured reorder latency in packets.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media::decoder::{DecoderConfig, MockDecoder, VideoCodec};
    ///
    /// let dec = MockDecoder::with_latency(DecoderConfig::new(VideoCodec::H264, 64, 64), 2);
    /// assert_eq!(dec.latency(), 2);
    /// ```
    #[must_use]
    pub fn latency(&self) -> usize {
        self.latency
    }

    /// Attaches HDR metadata that subsequent frames will report.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media::decoder::{DecoderConfig, MockDecoder, VideoCodec, VideoDecoder};
    /// use martensite_media::hdr::{Eotf, HdrMetadata};
    ///
    /// let mut dec = MockDecoder::with_latency(DecoderConfig::new(VideoCodec::Hevc, 64, 64), 0);
    /// dec.set_hdr(HdrMetadata::new(Eotf::Pq));
    /// assert_eq!(dec.hdr_metadata().unwrap().eotf, Eotf::Pq);
    /// ```
    pub fn set_hdr(&mut self, hdr: HdrMetadata) {
        self.hdr = Some(hdr);
    }
}

impl VideoDecoder for MockDecoder {
    fn init(config: DecoderConfig) -> Result<Self, MediaError> {
        Ok(Self::with_latency(config, 0))
    }

    fn send_packet(&mut self, packet: &EncodedPacket) -> Result<(), MediaError> {
        if packet.data.is_empty() {
            self.stats.record_rejection();
            return Err(DecodeError::StreamCorrupt("empty packet".to_string()).into());
        }
        if !packet.is_keyframe && !self.seen_keyframe {
            self.stats.record_rejection();
            return Err(DecodeError::NeedsKeyframe.into());
        }
        if packet.is_keyframe {
            self.seen_keyframe = true;
        }
        self.stats.record_packet(packet.data.len());
        self.pending.push_back(packet.clone());
        Ok(())
    }

    fn try_recv_frame(&mut self) -> Result<Option<DecodedFrame>, MediaError> {
        if self.pending.len() <= self.latency {
            return Ok(None);
        }
        let packet = self
            .pending
            .pop_front()
            .expect("pending is longer than latency");
        self.next_frame_id = self.next_frame_id.saturating_add(1);
        self.stats.record_frame(1_000_000); // deterministic 1 ms quantum
        let mut meta = VideoFrameMetadata::new(
            self.config.width.max(2),
            self.config.height.max(2),
            self.format,
            ColorRange::Limited,
        );
        meta.pts_nanos = packet.pts_nanos;
        meta.duration_nanos = packet.duration_nanos;
        meta.frame_index = self.next_frame_id;
        let mut frame = DecodedFrame::new(
            crate::surface::HardwareHandle::Mock {
                id: self.next_frame_id,
            },
            meta,
        );
        if let Some(ref hdr) = self.hdr {
            frame.hdr = Some(hdr.to_side_data());
        }
        Ok(Some(frame))
    }

    fn flush(&mut self) -> Result<(), MediaError> {
        self.pending.clear();
        self.seen_keyframe = false;
        Ok(())
    }

    fn negotiated_format(&self) -> VideoPixelFormat {
        self.format
    }

    fn hdr_metadata(&self) -> Option<HdrMetadata> {
        self.hdr.clone()
    }

    fn stats(&self) -> &DecodeStats {
        &self.stats
    }
}

// ---------------------------------------------------------------------------
// `VideoDecoder` impls for the platform backends.
//
// The concrete decoder types live in `martensite-media-platform` (the FFI
// crate); the trait impls live here where the trait is local. Each impl is a
// thin delegation — platform decoders expose the same method names as
// inherent methods.
// ---------------------------------------------------------------------------

#[cfg(all(feature = "decoder-videotoolbox", target_os = "macos"))]
impl VideoDecoder for videotoolbox::VideoToolboxDecoder {
    fn init(config: DecoderConfig) -> Result<Self, MediaError> {
        Self::create(&config)
    }

    fn send_packet(&mut self, packet: &EncodedPacket) -> Result<(), MediaError> {
        Self::send_packet(self, packet)
    }

    fn try_recv_frame(&mut self) -> Result<Option<DecodedFrame>, MediaError> {
        Self::try_recv_frame(self)
    }

    fn flush(&mut self) -> Result<(), MediaError> {
        Self::flush(self)
    }

    fn negotiated_format(&self) -> VideoPixelFormat {
        Self::negotiated_format(self)
    }

    fn hdr_metadata(&self) -> Option<HdrMetadata> {
        Self::hdr_side_data(self).as_ref().map(HdrMetadata::from)
    }

    fn stats(&self) -> &DecodeStats {
        Self::stats(self)
    }
}

#[cfg(all(feature = "decoder-mf", target_os = "windows"))]
impl VideoDecoder for mediafoundation::MediaFoundationDecoder {
    fn init(config: DecoderConfig) -> Result<Self, MediaError> {
        Self::create(&config)
    }

    fn send_packet(&mut self, packet: &EncodedPacket) -> Result<(), MediaError> {
        Self::send_packet(self, packet)
    }

    fn try_recv_frame(&mut self) -> Result<Option<DecodedFrame>, MediaError> {
        Self::try_recv_frame(self)
    }

    fn flush(&mut self) -> Result<(), MediaError> {
        Self::flush(self)
    }

    fn negotiated_format(&self) -> VideoPixelFormat {
        Self::negotiated_format(self)
    }

    fn hdr_metadata(&self) -> Option<HdrMetadata> {
        Self::hdr_side_data(self).as_ref().map(HdrMetadata::from)
    }

    fn stats(&self) -> &DecodeStats {
        Self::stats(self)
    }
}

#[cfg(all(feature = "decoder-vaapi", target_os = "linux"))]
impl VideoDecoder for vaapi::VaapiDecoder {
    fn init(config: DecoderConfig) -> Result<Self, MediaError> {
        Self::create(&config)
    }

    fn send_packet(&mut self, packet: &EncodedPacket) -> Result<(), MediaError> {
        Self::send_packet(self, packet)
    }

    fn try_recv_frame(&mut self) -> Result<Option<DecodedFrame>, MediaError> {
        Self::try_recv_frame(self)
    }

    fn flush(&mut self) -> Result<(), MediaError> {
        Self::flush(self)
    }

    fn negotiated_format(&self) -> VideoPixelFormat {
        Self::negotiated_format(self)
    }

    fn hdr_metadata(&self) -> Option<HdrMetadata> {
        Self::hdr_side_data(self).as_ref().map(HdrMetadata::from)
    }

    fn stats(&self) -> &DecodeStats {
        Self::stats(self)
    }
}

#[cfg(feature = "decoder-ffmpeg")]
impl VideoDecoder for ffmpeg::FfmpegDecoder {
    fn init(config: DecoderConfig) -> Result<Self, MediaError> {
        Self::create(&config)
    }

    fn send_packet(&mut self, packet: &EncodedPacket) -> Result<(), MediaError> {
        Self::send_packet(self, packet)
    }

    fn try_recv_frame(&mut self) -> Result<Option<DecodedFrame>, MediaError> {
        Self::try_recv_frame(self)
    }

    fn flush(&mut self) -> Result<(), MediaError> {
        Self::flush(self)
    }

    fn negotiated_format(&self) -> VideoPixelFormat {
        Self::negotiated_format(self)
    }

    fn hdr_metadata(&self) -> Option<HdrMetadata> {
        Self::hdr_side_data(self).as_ref().map(HdrMetadata::from)
    }

    fn stats(&self) -> &DecodeStats {
        Self::stats(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surface::HardwareHandle;

    fn config() -> DecoderConfig {
        DecoderConfig::new(VideoCodec::H264, 320, 240)
    }

    #[test]
    fn mock_decoder_emits_frames_for_packets() {
        let mut dec = MockDecoder::init(config()).unwrap();
        dec.send_packet(&EncodedPacket::new(vec![0x67], 0, 16_666_667))
            .unwrap();
        let frame = dec.try_recv_frame().unwrap().unwrap();
        assert_eq!(frame.metadata.pts_nanos, 0);
        assert_eq!(frame.metadata.frame_index, 1);
        assert!(matches!(frame.handle, HardwareHandle::Mock { id: 1 }));
        assert_eq!(dec.stats().frames_decoded, 1);
    }

    #[test]
    fn mock_decoder_rejects_delta_before_keyframe() {
        let mut dec = MockDecoder::init(config()).unwrap();
        let pkt = EncodedPacket::new(vec![0x41], 0, 0).delta();
        let err = dec.send_packet(&pkt).unwrap_err();
        assert_eq!(err, MediaError::InvalidHandle); // DecodeError::NeedsKeyframe
        assert_eq!(dec.stats().packets_rejected, 1);
    }

    #[test]
    fn mock_decoder_reorder_latency() {
        let mut dec = MockDecoder::with_latency(config(), 2);
        dec.send_packet(&EncodedPacket::new(vec![0x67], 0, 0))
            .unwrap();
        assert!(dec.try_recv_frame().unwrap().is_none());
        dec.send_packet(&EncodedPacket::new(vec![0x41], 1, 0).delta())
            .unwrap();
        assert!(dec.try_recv_frame().unwrap().is_none());
        dec.send_packet(&EncodedPacket::new(vec![0x41], 2, 0).delta())
            .unwrap();
        let frame = dec.try_recv_frame().unwrap().unwrap();
        assert_eq!(frame.metadata.pts_nanos, 0); // first packet's frame
    }

    #[test]
    fn mock_decoder_flush_resets_keyframe_requirement() {
        let mut dec = MockDecoder::init(config()).unwrap();
        dec.send_packet(&EncodedPacket::new(vec![0x67], 0, 0))
            .unwrap();
        dec.flush().unwrap();
        let pkt = EncodedPacket::new(vec![0x41], 1, 0).delta();
        assert!(dec.send_packet(&pkt).is_err());
        dec.send_packet(&EncodedPacket::new(vec![0x67], 1, 0))
            .unwrap();
        assert!(dec.try_recv_frame().unwrap().is_some());
    }

    #[test]
    fn mock_decoder_empty_packet_rejected() {
        let mut dec = MockDecoder::init(config()).unwrap();
        assert!(dec.send_packet(&EncodedPacket::new(vec![], 0, 0)).is_err());
    }

    #[test]
    fn mock_decoder_hdr_passthrough() {
        use crate::hdr::Eotf;
        let mut dec = MockDecoder::init(config()).unwrap();
        dec.set_hdr(HdrMetadata::new(Eotf::Pq));
        dec.send_packet(&EncodedPacket::new(vec![0x67], 0, 0))
            .unwrap();
        let frame = dec.try_recv_frame().unwrap().unwrap();
        assert_eq!(frame.hdr.unwrap().eotf_code, 16);
    }
}
