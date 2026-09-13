//! Software video decode via `ffmpeg-next` (`libavcodec` + `libswscale`).
//!
//! This is the all-platform CPU fallback backend: it decodes H.264, HEVC,
//! AV1 and VP9 elementary streams into [`HardwareHandle::CpuMemory`] frames.
//! Frames that arrive in `AV_PIX_FMT_NV12` or `AV_PIX_FMT_P010LE` are copied
//! out verbatim (preserving FFmpeg's plane padding); every other pixel
//! format — most commonly `AV_PIX_FMT_YUV420P` from the native H.264
//! decoder — is converted to NV12 once per unique input format through a
//! cached `swscale` context, so [`FfmpegDecoder::negotiated_format`] reports
//! the post-conversion [`VideoPixelFormat`].
//!
//! ## Software opt-in
//!
//! FFmpeg *is* software decode, so [`DecoderConfig::allow_software`] is not
//! consulted: reaching this backend at all is already an explicit opt-in —
//! the caller chose the software fallback (via the `decoder-ffmpeg` cargo
//! feature and this init path) after hardware backends were unavailable or
//! declined.
//!
//! ## Threading
//!
//! [`FfmpegDecoder::create`] enables FFmpeg frame-level threading
//! (`FF_THREAD_FRAME`) with `thread_count = 0` (libavcodec auto-detect),
//! which pipelines reorder-buffer decoding across cores.

use std::collections::VecDeque;
use std::time::Instant;

use ffmpeg::codec::{self, decoder, threading};
use ffmpeg::frame::{self, side_data};
use ffmpeg::software::scaling;
use ffmpeg::util::color;
use ffmpeg::util::format::Pixel;
use ffmpeg_next as ffmpeg;

use crate::decoder::{
    DecodeError, DecodeStats, DecodedFrame, DecoderBackend, DecoderConfig, EncodedPacket,
    HdrSideData, VideoCodec,
};
use crate::surface::{
    ColorRange, HardwareHandle, MediaError, VideoFrameMetadata, VideoPixelFormat,
};

/// Field byte offsets inside `AVMasteringDisplayMetadata`
/// (`libavutil/mastering_display_metadata.h`). `ffmpeg-sys-next`'s bindgen
/// allowlist does not generate the struct, so the side-data payload is
/// decoded by offset instead: six `AVRational` display primaries
/// (48 bytes), two `AVRational` white-point fields (16 bytes), then
/// `min_luminance`, `max_luminance`, `has_primaries`, `has_luminance` — all
/// naturally aligned `AVRational`/`int` members with no interior padding.
mod mastering_display {
    /// Offset of `AVRational min_luminance`.
    pub const MIN_LUMINANCE: usize = 64;
    /// Offset of `AVRational max_luminance`.
    pub const MAX_LUMINANCE: usize = 72;
    /// Offset of `int has_luminance`.
    pub const HAS_LUMINANCE: usize = 84;
    /// `sizeof(AVMasteringDisplayMetadata)`.
    pub const SIZE: usize = 88;
}

/// Byte offsets inside `AVContentLightMetadata`
/// (`libavutil/mastering_display_metadata.h`): two `unsigned short` fields.
mod content_light {
    /// Offset of `unsigned short MaxCLL`.
    pub const MAX_CLL: usize = 0;
    /// Offset of `unsigned short MaxFALL`.
    pub const MAX_FALL: usize = 2;
    /// `sizeof(AVContentLightMetadata)`.
    pub const SIZE: usize = 4;
}

/// `libavcodec` software decoder producing [`HardwareHandle::CpuMemory`]
/// frames.
///
/// `FfmpegDecoder` is the software fallback behind `martensite-media`'s
/// `VideoDecoder` impl. Packets are pushed with
/// [`send_packet`](Self::send_packet); reordered output frames are drained
/// with [`try_recv_frame`](Self::try_recv_frame), which returns `Ok(None)`
/// while the decoder is still chewing on input.
///
/// # Examples
///
/// ```
/// use martensite_media_platform::decoder::{
///     DecoderConfig, VideoCodec, ffmpeg::FfmpegDecoder,
/// };
/// use martensite_media_platform::surface::VideoPixelFormat;
///
/// let mut dec = FfmpegDecoder::create(&DecoderConfig::new(VideoCodec::H264, 320, 240))
///     .expect("system FFmpeg must provide an H.264 decoder");
/// assert_eq!(dec.negotiated_format(), VideoPixelFormat::Nv12);
/// assert!(dec.try_recv_frame().expect("drain").is_none());
/// ```
pub struct FfmpegDecoder {
    /// Opened `libavcodec` video decoder context.
    decoder: decoder::Video,
    /// Lazily created `swscale` converter for non-NV12/P010 input formats.
    scaler: Option<scaling::Context>,
    /// `(format, width, height)` key the cached `scaler` was built for.
    scaler_key: Option<(Pixel, u32, u32)>,
    /// Reordered output frames already pulled out of `libavcodec`.
    out: VecDeque<DecodedFrame>,
    /// Keyframe gate: `false` until the first IDR packet is accepted.
    seen_keyframe: bool,
    /// Pixel format currently being emitted (updated per decoded frame).
    negotiated: VideoPixelFormat,
    /// HDR side-data captured from the most recently decoded frame.
    hdr: Option<HdrSideData>,
    /// Rolling telemetry counters.
    stats: DecodeStats,
    /// Running index stamped on each emitted frame (starts at 1).
    frame_index: u64,
    /// Whether `avcodec_send_packet(NULL)` has already been issued.
    eof: bool,
}

// SAFETY: `FfmpegDecoder` exclusively owns every raw FFmpeg object it
// carries — the `AVCodecContext` inside `decoder::Video` and the
// `SwsContext` inside `scaling::Context` (a `*mut` that blocks auto-`Send`).
// Neither object shares state with other FFmpeg instances, and every call
// into them goes through `&mut self` methods, so transferring ownership to
// another thread cannot introduce a data race.
unsafe impl Send for FfmpegDecoder {}

// SAFETY: `&self` methods (`negotiated_format`/`hdr_side_data`/`stats`) read
// only plain owned fields — they never touch the `AVCodecContext` or
// `SwsContext`, so shared references cannot race FFI state. `VideoDecoder`
// requires `Sync` because widgets are stored in the shared arena.
unsafe impl Sync for FfmpegDecoder {}

impl FfmpegDecoder {
    /// Opens a software decoder for `config.codec`.
    ///
    /// `config.codec_config` (the container's `avcC`/`hvcC`/`av1C`/`vpcC`
    /// record) is installed as codec extradata before `avcodec_open2`.
    /// `config.decode_ahead` is honoured implicitly: frame threading plus
    /// the internal output queue give the decoder its reorder depth.
    ///
    /// # Errors
    ///
    /// - [`DecodeError::UnsupportedCodec`] when the linked FFmpeg build has
    ///   no decoder for `config.codec`.
    /// - [`DecodeError::Fatal`] when `avcodec_open2` fails or the extradata
    ///   allocation fails.
    /// - [`DecodeError::StreamCorrupt`] when `codec_config` is present but
    ///   empty.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media_platform::decoder::{
    ///     DecoderConfig, DecoderBackend, VideoCodec, ffmpeg::FfmpegDecoder,
    /// };
    ///
    /// let dec = FfmpegDecoder::create(&DecoderConfig::new(VideoCodec::H264, 64, 64)).unwrap();
    /// assert_eq!(dec.stats().backend, Some(DecoderBackend::FfmpegSoftware));
    /// ```
    pub fn create(config: &DecoderConfig) -> Result<Self, MediaError> {
        ffmpeg::init().map_err(|e| DecodeError::Fatal(format!("ffmpeg init failed: {e}")))?;

        let id = match config.codec {
            VideoCodec::H264 => codec::Id::H264,
            VideoCodec::Hevc => codec::Id::HEVC,
            VideoCodec::Av1 => codec::Id::AV1,
            VideoCodec::Vp9 => codec::Id::VP9,
        };

        let codec = decoder::find(id).ok_or_else(|| {
            DecodeError::UnsupportedCodec(format!(
                "no FFmpeg decoder for {:?} ({})",
                config.codec,
                config.codec.fourcc_hint()
            ))
        })?;

        let mut context = codec::Context::new_with_codec(codec).decoder();

        // Frame threading pipelines decode across cores; `count = 0` lets
        // libavcodec pick the thread pool size from the host topology.
        let mut threading = threading::Config::kind(threading::Type::Frame);
        threading.count = 0;
        context.set_threading(threading);

        if let Some(extradata) = &config.codec_config {
            if extradata.is_empty() {
                return Err(
                    DecodeError::StreamCorrupt("codec_config record is empty".to_string()).into(),
                );
            }
            set_extradata(&mut context, extradata)?;
        }

        let mut decoder = context
            .open_as(codec)
            .and_then(decoder::Opened::video)
            .map_err(|e| DecodeError::Fatal(format!("avcodec_open2 failed for {id:?}: {e}")))?;

        // Declare packet timestamps as nanoseconds so `frame.pts()` and
        // `frame.packet().duration` come back in nanoseconds verbatim.
        decoder.set_packet_time_base(ffmpeg::Rational::new(1, 1_000_000_000));

        let stats = DecodeStats {
            backend: Some(DecoderBackend::FfmpegSoftware),
            ..DecodeStats::default()
        };

        Ok(Self {
            decoder,
            scaler: None,
            scaler_key: None,
            out: VecDeque::new(),
            seen_keyframe: false,
            negotiated: VideoPixelFormat::Nv12,
            hdr: None,
            stats,
            frame_index: 0,
            eof: false,
        })
    }

    /// Feeds one compressed access unit to the decoder.
    ///
    /// The first packet after construction or [`flush`](Self::flush) must be
    /// a keyframe; delta packets before that are rejected with
    /// [`DecodeError::NeedsKeyframe`]. When `libavcodec`'s internal queue is
    /// full (`EAGAIN`) the decoder is drained into the output queue and the
    /// send retried once.
    ///
    /// # Errors
    ///
    /// - [`DecodeError::StreamCorrupt`] for empty packets, packets too large
    ///   for `libavcodec`, or packets the codec itself rejects as invalid.
    /// - [`DecodeError::NeedsKeyframe`] for a delta packet before the first
    ///   keyframe.
    /// - [`DecodeError::Fatal`] for unrecoverable backend errors.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media_platform::decoder::{
    ///     DecoderConfig, EncodedPacket, VideoCodec, ffmpeg::FfmpegDecoder,
    /// };
    /// use martensite_media_platform::surface::MediaError;
    ///
    /// let mut dec = FfmpegDecoder::create(&DecoderConfig::new(VideoCodec::H264, 64, 64)).unwrap();
    /// // A delta packet before any keyframe is gated.
    /// let err = dec
    ///     .send_packet(&EncodedPacket::new(vec![0x41], 0, 0).delta())
    ///     .unwrap_err();
    /// assert_eq!(err, MediaError::InvalidHandle); // DecodeError::NeedsKeyframe
    /// ```
    pub fn send_packet(&mut self, packet: &EncodedPacket) -> Result<(), MediaError> {
        if packet.data.is_empty() {
            self.stats.record_rejection();
            return Err(DecodeError::StreamCorrupt(
                "empty packet carries no access unit".to_string(),
            )
            .into());
        }
        if packet.data.len() > i32::MAX as usize {
            self.stats.record_rejection();
            return Err(DecodeError::StreamCorrupt(
                "packet exceeds libavcodec's maximum size".to_string(),
            )
            .into());
        }
        if self.eof {
            self.stats.record_rejection();
            return Err(
                DecodeError::StreamCorrupt("packet sent after end_of_stream".to_string()).into(),
            );
        }
        if !self.seen_keyframe && !packet.is_keyframe {
            self.stats.record_rejection();
            return Err(DecodeError::NeedsKeyframe.into());
        }

        let mut pkt = ffmpeg::Packet::copy(&packet.data);
        pkt.set_pts(Some(sat_i64(packet.pts_nanos)));
        pkt.set_duration(sat_i64(packet.duration_nanos));
        if packet.is_keyframe {
            pkt.set_flags(ffmpeg::packet::Flags::KEY);
        }

        match self.decoder.send_packet(&pkt) {
            Ok(()) => {
                self.seen_keyframe |= packet.is_keyframe;
                self.stats.record_packet(packet.data.len());
                // Opportunistically pull finished frames out of the
                // frame-threading workers. Older libavcodec releases (5.x)
                // can drop the final frame at `send_eof` when every frame
                // is still queued inside the worker threads because the
                // caller never received mid-stream; draining here keeps the
                // output path exercised so EOF flushes the whole stream.
                self.drain_decoder()?;
                Ok(())
            }
            Err(ref e) if is_again(e) => {
                // Internal frame queue full: drain completed frames into
                // `self.out`, then retry the send exactly once.
                self.drain_decoder()?;
                match self.decoder.send_packet(&pkt) {
                    Ok(()) => {
                        self.seen_keyframe |= packet.is_keyframe;
                        self.stats.record_packet(packet.data.len());
                        Ok(())
                    }
                    Err(e) => {
                        self.stats.record_rejection();
                        Err(map_send_error(e))
                    }
                }
            }
            Err(e) => {
                self.stats.record_rejection();
                Err(map_send_error(e))
            }
        }
    }

    /// Returns the next reordered frame, or `Ok(None)` when the decoder
    /// needs more input before it can produce output.
    ///
    /// Frames first decoded after a full send queue are buffered internally,
    /// so callers can interleave `send_packet`/`try_recv_frame` freely.
    ///
    /// # Errors
    ///
    /// - [`DecodeError::StreamCorrupt`] when `libavcodec` reports corrupt
    ///   frame data or a frame with an unreadable plane layout.
    /// - [`DecodeError::Fatal`] for unrecoverable backend errors.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media_platform::decoder::{
    ///     DecoderConfig, VideoCodec, ffmpeg::FfmpegDecoder,
    /// };
    ///
    /// let mut dec = FfmpegDecoder::create(&DecoderConfig::new(VideoCodec::H264, 64, 64)).unwrap();
    /// assert!(dec.try_recv_frame().unwrap().is_none());
    /// ```
    pub fn try_recv_frame(&mut self) -> Result<Option<DecodedFrame>, MediaError> {
        self.drain_decoder()?;
        Ok(self.out.pop_front())
    }

    /// Discards all internally buffered frames and resets the stream state.
    ///
    /// This is a *reset* (seek semantics), not an end-of-stream drain:
    /// `avcodec_flush_buffers` drops frames still inside the reorder buffer,
    /// and the next packet must be a keyframe.
    ///
    /// # Errors
    ///
    /// Always succeeds; `flush` exists on the trait for uniformity with
    /// backends whose reset can fail.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media_platform::decoder::{
    ///     DecoderConfig, EncodedPacket, VideoCodec, ffmpeg::FfmpegDecoder,
    /// };
    ///
    /// let mut dec = FfmpegDecoder::create(&DecoderConfig::new(VideoCodec::H264, 64, 64)).unwrap();
    /// dec.flush().unwrap();
    /// // Post-flush the keyframe gate is armed again.
    /// assert!(dec
    ///     .send_packet(&EncodedPacket::new(vec![0x41], 0, 0).delta())
    ///     .is_err());
    /// ```
    pub fn flush(&mut self) -> Result<(), MediaError> {
        self.decoder.flush();
        self.out.clear();
        self.seen_keyframe = false;
        self.scaler = None;
        self.scaler_key = None;
        self.hdr = None;
        self.eof = false;
        Ok(())
    }

    /// The [`VideoPixelFormat`] currently produced by
    /// [`try_recv_frame`](Self::try_recv_frame).
    ///
    /// Reports `Nv12` until the first frame is decoded, then tracks the
    /// actual output (`Nv12` or `P010`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media_platform::decoder::{
    ///     DecoderConfig, VideoCodec, ffmpeg::FfmpegDecoder,
    /// };
    /// use martensite_media_platform::surface::VideoPixelFormat;
    ///
    /// let dec = FfmpegDecoder::create(&DecoderConfig::new(VideoCodec::H264, 64, 64)).unwrap();
    /// assert_eq!(dec.negotiated_format(), VideoPixelFormat::Nv12);
    /// ```
    #[must_use]
    pub fn negotiated_format(&self) -> VideoPixelFormat {
        self.negotiated
    }

    /// HDR side-data captured from the most recently decoded frame.
    ///
    /// Combines `AV_FRAME_DATA_MASTERING_DISPLAY_METADATA` /
    /// `AV_FRAME_DATA_CONTENT_LIGHT_LEVEL` frame side-data with the frame's
    /// signalled colour attributes. Returns `None` while no frame has been
    /// decoded or when the stream signals plain SDR/BT.709.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media_platform::decoder::{
    ///     DecoderConfig, VideoCodec, ffmpeg::FfmpegDecoder,
    /// };
    ///
    /// let dec = FfmpegDecoder::create(&DecoderConfig::new(VideoCodec::H264, 64, 64)).unwrap();
    /// assert!(dec.hdr_side_data().is_none());
    /// ```
    #[must_use]
    pub fn hdr_side_data(&self) -> Option<HdrSideData> {
        self.hdr.clone()
    }

    /// Rolling telemetry for this decoder instance.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media_platform::decoder::{
    ///     DecoderConfig, DecoderBackend, VideoCodec, ffmpeg::FfmpegDecoder,
    /// };
    ///
    /// let dec = FfmpegDecoder::create(&DecoderConfig::new(VideoCodec::H264, 64, 64)).unwrap();
    /// assert_eq!(dec.stats().frames_decoded, 0);
    /// assert!(!dec.stats().hardware_accelerated());
    /// ```
    #[must_use]
    pub fn stats(&self) -> &DecodeStats {
        &self.stats
    }

    /// Signals end-of-stream and releases every frame still held in the
    /// reorder buffer (`avcodec_send_packet(NULL)` + drain).
    ///
    /// Idempotent: subsequent calls are no-ops. After `end_of_stream`,
    /// [`send_packet`](Self::send_packet) may be used again once a
    /// [`flush`](Self::flush) has re-armed the decoder.
    ///
    /// # Errors
    ///
    /// Returns [`DecodeError::Fatal`] when `libavcodec` rejects the drain.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media_platform::decoder::{
    ///     DecoderConfig, VideoCodec, ffmpeg::FfmpegDecoder,
    /// };
    ///
    /// let mut dec = FfmpegDecoder::create(&DecoderConfig::new(VideoCodec::H264, 64, 64)).unwrap();
    /// dec.end_of_stream().unwrap();
    /// ```
    pub fn end_of_stream(&mut self) -> Result<(), MediaError> {
        if self.eof {
            return Ok(());
        }
        self.eof = true;
        self.decoder
            .send_eof()
            .map_err(|e| DecodeError::Fatal(format!("send_eof failed: {e}")))?;
        self.drain_decoder()
    }

    /// Pulls every frame `libavcodec` currently has ready into `self.out`.
    fn drain_decoder(&mut self) -> Result<(), MediaError> {
        loop {
            let mut raw = frame::Video::empty();
            let start = Instant::now();
            match self.decoder.receive_frame(&mut raw) {
                Ok(()) => {
                    let decoded = self.convert(&raw)?;
                    self.stats.record_frame(sat_u64(start.elapsed().as_nanos()));
                    self.out.push_back(decoded);
                }
                Err(ref e) if is_again(e) || matches!(e, ffmpeg::Error::Eof) => {
                    return Ok(());
                }
                Err(e) => return Err(map_recv_error(e)),
            }
        }
    }

    /// Converts one decoded `AVFrame` into a [`DecodedFrame`] backed by
    /// [`HardwareHandle::CpuMemory`].
    ///
    /// NV12 and P010LE planes are copied verbatim; other formats go through
    /// the cached `swscale` converter to NV12 first.
    fn convert(&mut self, raw: &frame::Video) -> Result<DecodedFrame, MediaError> {
        let (width, height) = (raw.width(), raw.height());
        let source_format = raw.format();
        if width == 0 || height == 0 {
            return Err(MediaError::InvalidBufferDimensions { width, height });
        }

        // `converted` owns the swscale output frame for the conversion path;
        // for NV12/P010LE passthrough the decoded `raw` frame is read
        // directly. Either way `frame` is only borrowed long enough to copy
        // its planes into owned `Vec`s.
        let mut converted = frame::Video::empty();
        let frame: &frame::Video = match source_format {
            Pixel::NV12 | Pixel::P010LE => raw,
            other => {
                self.ensure_scaler(other, width, height)?;
                let Some(scaler) = self.scaler.as_mut() else {
                    return Err(DecodeError::Fatal(
                        "swscale context missing after ensure".to_string(),
                    )
                    .into());
                };
                scaler.run(raw, &mut converted).map_err(|e| {
                    DecodeError::Fatal(format!("swscale {other:?} -> NV12 failed: {e}"))
                })?;
                &converted
            }
        };

        let output_format = match frame.format() {
            Pixel::NV12 => VideoPixelFormat::Nv12,
            Pixel::P010LE => VideoPixelFormat::P010,
            other => {
                return Err(DecodeError::Fatal(format!(
                    "unexpected post-conversion pixel format {other:?}"
                ))
                .into());
            }
        };
        // `frame.data(1)` panics on a single-plane frame; treat a missing
        // chroma plane as stream corruption rather than crashing.
        if frame.planes() < 2 {
            return Err(DecodeError::StreamCorrupt(format!(
                "decoded {source_format:?} frame has fewer than 2 planes"
            ))
            .into());
        }

        self.negotiated = output_format;
        self.hdr = extract_hdr(raw);

        let range = match raw.color_range() {
            color::Range::JPEG => ColorRange::Full,
            _ => ColorRange::Limited,
        };
        let mut metadata = VideoFrameMetadata::try_new(width, height, output_format, range)?;
        // `pkt_timebase` was set to 1/1e9 at create, so `pts` and `duration`
        // arrive in nanoseconds.
        metadata.pts_nanos = raw
            .pts()
            .or_else(|| raw.timestamp())
            .map_or(0, |pts| u64::try_from(pts).unwrap_or(0));
        metadata.duration_nanos = u64::try_from(raw.packet().duration).unwrap_or(0);
        self.frame_index = self.frame_index.saturating_add(1);
        metadata.frame_index = self.frame_index;

        let handle = HardwareHandle::CpuMemory {
            // `data(i)` covers `stride * plane_height` bytes including
            // FFmpeg's row padding; the strides are reported alongside so
            // the upload path can skip it.
            y_plane: frame.data(0).to_vec(),
            uv_plane: frame.data(1).to_vec(),
            y_stride: u32::try_from(frame.stride(0)).unwrap_or(u32::MAX),
            uv_stride: u32::try_from(frame.stride(1)).unwrap_or(u32::MAX),
        };

        let mut decoded = DecodedFrame::new(handle, metadata);
        decoded.hdr = self.hdr.clone();
        Ok(decoded)
    }

    /// (Re)creates the `swscale` converter when the input format or
    /// dimensions change.
    fn ensure_scaler(&mut self, format: Pixel, width: u32, height: u32) -> Result<(), MediaError> {
        if self.scaler_key == Some((format, width, height)) && self.scaler.is_some() {
            return Ok(());
        }
        let scaler = scaling::Context::get(
            format,
            width,
            height,
            Pixel::NV12,
            width,
            height,
            scaling::flag::Flags::FAST_BILINEAR,
        )
        .map_err(|e| {
            DecodeError::UnsupportedCodec(format!(
                "no swscale conversion path from {format:?} to NV12: {e}"
            ))
        })?;
        self.scaler = Some(scaler);
        self.scaler_key = Some((format, width, height));
        Ok(())
    }
}

/// `true` when `libavcodec` reports "resource temporarily unavailable"
/// (`AVERROR(EAGAIN)`).
fn is_again(error: &ffmpeg::Error) -> bool {
    matches!(
        *error,
        ffmpeg::Error::Other { errno } if errno == ffmpeg::error::EAGAIN
    )
}

/// Saturates a `u64` nanosecond timestamp into `i64` for `AVPacket`.
fn sat_i64(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

/// Saturates an elapsed-nanos measurement into `u64`.
fn sat_u64<T: TryInto<u64>>(value: T) -> u64 {
    value.try_into().unwrap_or(u64::MAX)
}

/// Installs container-supplied codec extradata on a not-yet-opened decoder.
fn set_extradata(context: &mut decoder::Decoder, extradata: &[u8]) -> Result<(), MediaError> {
    // `ffmpeg-next` exposes no safe extradata setter, so this writes the
    // `AVCodecContext` fields directly.
    //
    // SAFETY: `context` is a live, exclusively-owned `AVCodecContext` that
    // has not been opened yet, so replacing `extradata` cannot race with an
    // active decoder. The buffer is allocated with `av_mallocz` — the same
    // allocator `avcodec_free_context` uses to release `extradata` — and is
    // padded with `AV_INPUT_BUFFER_PADDING_SIZE` trailing zero bytes as
    // `avcodec_open2` requires. Any previously installed extradata is
    // released with `av_freep` first.
    unsafe {
        let ctx = context.as_mut_ptr();
        let size = extradata
            .len()
            .saturating_add(ffmpeg::ffi::AV_INPUT_BUFFER_PADDING_SIZE as usize);
        let buffer = ffmpeg::ffi::av_mallocz(size).cast::<u8>();
        if buffer.is_null() {
            return Err(
                DecodeError::Fatal("av_mallocz failed for codec extradata".to_string()).into(),
            );
        }
        if !(*ctx).extradata.is_null() {
            ffmpeg::ffi::av_freep(
                core::ptr::addr_of_mut!((*ctx).extradata).cast::<core::ffi::c_void>(),
            );
        }
        core::ptr::copy_nonoverlapping(extradata.as_ptr(), buffer, extradata.len());
        (*ctx).extradata = buffer;
        (*ctx).extradata_size = i32::try_from(extradata.len()).unwrap_or(i32::MAX);
    }
    Ok(())
}

/// Maps an `avcodec_send_packet` failure onto the decoder error vocabulary.
fn map_send_error(error: ffmpeg::Error) -> MediaError {
    match error {
        ffmpeg::Error::InvalidData => {
            DecodeError::StreamCorrupt("libavcodec rejected the packet".to_string()).into()
        }
        other => DecodeError::Fatal(format!("avcodec_send_packet failed: {other}")).into(),
    }
}

/// Maps an `avcodec_receive_frame` failure onto the decoder error vocabulary.
fn map_recv_error(error: ffmpeg::Error) -> MediaError {
    match error {
        ffmpeg::Error::InvalidData => {
            DecodeError::StreamCorrupt("libavcodec reported corrupt frame data".to_string()).into()
        }
        other => DecodeError::Fatal(format!("avcodec_receive_frame failed: {other}")).into(),
    }
}

/// Reads a native-endian `i32` out of a side-data payload.
fn read_i32_ne(bytes: &[u8], offset: usize) -> Option<i32> {
    let raw: [u8; 4] = bytes.get(offset..offset + 4)?.try_into().ok()?;
    Some(i32::from_ne_bytes(raw))
}

/// Reads a native-endian `u16` out of a side-data payload.
fn read_u16_ne(bytes: &[u8], offset: usize) -> Option<u16> {
    let raw: [u8; 2] = bytes.get(offset..offset + 2)?.try_into().ok()?;
    Some(u16::from_ne_bytes(raw))
}

/// Reads an `AVRational` (`{ num: i32, den: i32 }`) out of a side-data
/// payload, treating non-positive fields as "not signalled".
fn read_rational_ne(bytes: &[u8], offset: usize) -> Option<f32> {
    let num = read_i32_ne(bytes, offset)?;
    let den = read_i32_ne(bytes, offset + 4)?;
    (num > 0 && den > 0).then_some(num as f32 / den as f32)
}

/// Reads HDR signalling (colour attributes + mastering-display and
/// content-light side data) off a decoded frame.
///
/// Returns `None` for plain SDR / BT.709 output so `DecodedFrame::hdr` only
/// carries a value when the signal actually deviates from the SDR default.
fn extract_hdr(frame: &frame::Video) -> Option<HdrSideData> {
    // ISO 23001-8 code points: 9 = BT.2020 primaries, 16 = PQ/ST 2084,
    // 18 = HLG/ARIB STD-B67; 1 = BT.709/SDR for everything else.
    let mut side = HdrSideData {
        eotf_code: match frame.color_transfer_characteristic() {
            color::TransferCharacteristic::SMPTE2084 => 16,
            color::TransferCharacteristic::ARIB_STD_B67 => 18,
            _ => 1,
        },
        primaries_code: match frame.color_primaries() {
            color::Primaries::BT2020 => 9,
            _ => 1,
        },
        full_range: frame.color_range() == color::Range::JPEG,
        max_luminance_nits: None,
        min_luminance_nits: None,
        max_cll: None,
        max_fall: None,
        dynamic_metadata: None,
    };

    if let Some(data) = frame.side_data(side_data::Type::MasteringDisplayMetadata) {
        let bytes = data.data();
        if bytes.len() >= mastering_display::SIZE
            && read_i32_ne(bytes, mastering_display::HAS_LUMINANCE) != Some(0)
        {
            side.max_luminance_nits = read_rational_ne(bytes, mastering_display::MAX_LUMINANCE);
            side.min_luminance_nits = read_rational_ne(bytes, mastering_display::MIN_LUMINANCE);
        }
    }

    if let Some(data) = frame.side_data(side_data::Type::ContentLightLevel) {
        let bytes = data.data();
        if bytes.len() >= content_light::SIZE {
            if let Some(max_cll) = read_u16_ne(bytes, content_light::MAX_CLL).filter(|v| *v != 0) {
                side.max_cll = Some(max_cll);
            }
            if let Some(max_fall) = read_u16_ne(bytes, content_light::MAX_FALL).filter(|v| *v != 0)
            {
                side.max_fall = Some(max_fall);
            }
        }
    }

    let signalled = side.eotf_code != 1
        || side.primaries_code == 9
        || side.max_luminance_nits.is_some()
        || side.max_cll.is_some();
    signalled.then_some(side)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> DecoderConfig {
        DecoderConfig::new(VideoCodec::H264, 320, 240)
    }

    fn decoder() -> FfmpegDecoder {
        match FfmpegDecoder::create(&config()) {
            Ok(dec) => dec,
            Err(e) => panic!("H.264 decoder unavailable in system FFmpeg: {e}"),
        }
    }

    #[test]
    fn create_reports_ffmpeg_backend() {
        let dec = decoder();
        assert_eq!(dec.stats().backend, Some(DecoderBackend::FfmpegSoftware));
        assert_eq!(dec.negotiated_format(), VideoPixelFormat::Nv12);
        assert!(dec.hdr_side_data().is_none());
    }

    #[test]
    fn other_codecs_construct_when_supported() {
        for codec in [VideoCodec::Hevc, VideoCodec::Av1, VideoCodec::Vp9] {
            // Best-effort: Homebrew FFmpeg ships all of these, but a minimal
            // build may not — absence must surface as UnsupportedCodec (a
            // `MediaError::ImportFailed` via the `DecodeError` conversion).
            match FfmpegDecoder::create(&DecoderConfig::new(codec, 64, 64)) {
                Ok(_) | Err(MediaError::ImportFailed(_)) => {}
                Err(e) => panic!("unexpected error for {codec:?}: {e}"),
            }
        }
    }

    #[test]
    fn rejects_empty_packet() {
        let mut dec = decoder();
        let err = dec
            .send_packet(&EncodedPacket::new(vec![], 0, 0))
            .expect_err("empty packet must fail");
        assert_eq!(
            err,
            MediaError::ImportFailed("empty packet carries no access unit".to_string())
        );
        assert_eq!(dec.stats().packets_rejected, 1);
    }

    #[test]
    fn rejects_delta_before_keyframe() {
        let mut dec = decoder();
        let err = dec
            .send_packet(&EncodedPacket::new(vec![0x41], 0, 0).delta())
            .expect_err("delta before keyframe must fail");
        assert_eq!(err, MediaError::InvalidHandle); // DecodeError::NeedsKeyframe
        assert_eq!(dec.stats().packets_rejected, 1);
    }

    #[test]
    fn flush_rearms_keyframe_gate() {
        let mut dec = decoder();
        dec.flush().expect("flush");
        let err = dec
            .send_packet(&EncodedPacket::new(vec![0x41], 0, 0).delta())
            .expect_err("post-flush delta must fail");
        assert_eq!(err, MediaError::InvalidHandle);
    }

    #[test]
    fn try_recv_is_none_without_input() {
        let mut dec = decoder();
        assert!(dec.try_recv_frame().expect("drain").is_none());
    }

    #[test]
    fn empty_codec_config_record_rejected() {
        let config = DecoderConfig::new(VideoCodec::H264, 64, 64).with_codec_config(vec![]);
        assert!(FfmpegDecoder::create(&config).is_err());
    }

    #[test]
    fn decoder_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<FfmpegDecoder>();
    }

    /// End-to-end decode requires a real H.264 elementary-stream fixture
    /// (Annex-B or `avcC` + `codec_config`), which is not vendored into the
    /// repo. Synthesize one with
    /// `ffmpeg -f lavfi -i testsrc2=size=320x240:rate=30 -t 1 -c:v libx264 -f h264 crates/martensite-media-platform/tests/fixtures/one_frame.h264`
    /// then run `cargo test -p martensite-media-platform --features
    /// decoder-ffmpeg -- --ignored`.
    #[test]
    #[ignore = "needs a real H.264 bitstream fixture"]
    fn decodes_real_h264_fixture() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/one_frame.h264");
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("skipping: {path} not present");
            return;
        };
        let mut dec = decoder();
        let packet = EncodedPacket::new(bytes, 0, 33_333_333);
        // Frame threading delays output until the reorder pipeline fills, so
        // the same IDR is fed repeatedly until a frame emerges.
        let mut produced = None;
        for _ in 0..16 {
            dec.send_packet(&packet).expect("keyframe accepted");
            if let Some(frame) = dec.try_recv_frame().expect("receive") {
                produced = Some(frame);
                break;
            }
        }
        let frame = produced.expect("a frame is produced");
        assert!(matches!(frame.handle, HardwareHandle::CpuMemory { .. }));
        assert_eq!(frame.metadata.frame_index, 1);
    }
}
