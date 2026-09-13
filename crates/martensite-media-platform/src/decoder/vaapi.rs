//! Linux VAAPI decoder backend built on `cros-libva`.
//!
//! [`VaapiDecoder`] drives a `VADisplay` opened on a DRM render node
//! (`/dev/dri/renderD*`), submits `vaBeginPicture`/`vaRenderPicture`/
//! `vaEndPicture` per access unit, syncs the surface, and exports the decoded
//! frame via `vaExportSurfaceHandle` (`VASurfaceAttribMemTypeDRM_PRIME2`) as a
//! [`HardwareHandle::DmaBuf`].
//!
//! # Codec coverage
//!
//! Bitstream parsing is fully wired for **H.264** (Annex-B and `avcC`
//! length-prefixed framing; SPS/PPS/slice-header parsing into
//! `VAPictureParameterBufferH264` + `VAIQMatrixBufferH264` +
//! `VASliceParameterBufferH264` + slice data). The HEVC/VP9/AV1 *profiles* are
//! negotiated at creation, but their slice-level parameter translation is not
//! implemented yet — `send_packet` rejects those streams with
//! [`DecodeError::UnsupportedCodec`]. The profile/context plumbing is
//! codec-generic so adding a codec only requires its bitstream→`BufferType`
//! translation.
//!
//! # Documented simplifications (H.264 path)
//!
//! - **Reference management is approximate**: the DPB is modelled as "the
//!   last N decoded reference surfaces" (newest first in `RefPicList0`,
//!   oldest first in `RefPicList1`); MMCO/sliding-window and long-term
//!   reference handling from `dec_ref_pic_marking()` are parsed but not
//!   honoured. Streams whose reference lists deviate from a recency LRU
//!   (e.g. explicit reordering, long-term refs) may produce artifacts; IDR
//!   and short-term-recent-reference streams decode correctly.
//! - **Scaling matrices are not parsed**: `VAIQMatrixBufferH264` is always
//!   submitted with flat (16) defaults; streams signalling
//!   `seq_scaling_matrix_present`/`pic_scaling_matrix_present` decode with
//!   default quantization.
//! - **Weighted prediction fields are zeroed** in the slice params.
//! - **POC computation ignores wraparound** (`prevPicOrderCntMsb` tracking is
//!   omitted); `TopFieldOrderCnt` is an approximation used only for
//!   driver-side reference ordering.
//! - **FMO** (`num_slice_groups_minus1 > 0`) and **SP/SI** slices are
//!   rejected as corrupt.
//! - **No output reordering**: frames are emitted in *decode* order with the
//!   producing packet's PTS attached; `FrameQueue` in `martensite-media`
//!   handles presentation pacing.
//! - **Fixed surface pool**: `decode_ahead + DPB (17)` surfaces are recycled
//!   round-robin. An exported `dma-buf` aliases the surface memory, so a
//!   frame may be overwritten once its surface is recycled — callers must
//!   import promptly (the fd itself keeps the GEM object alive).
//! - **Mid-stream resolution changes are not supported**; the first SPS wins
//!   and a later conflicting SPS yields [`DecodeError::StreamCorrupt`].
//!
//! # `allow_software`
//!
//! VAAPI *is* hardware acceleration: when no usable render node exists,
//! [`VaapiDecoder::create`] fails regardless of `config.allow_software`.
//! The software fallback lives in the `decoder/ffmpeg.rs` backend
//! (`decoder-ffmpeg` feature); callers should construct that decoder when
//! this one returns an error.

use std::collections::{HashMap, VecDeque};
use std::os::fd::IntoRawFd;
use std::rc::Rc;
use std::time::Instant;

use cros_libva as va;

use crate::decoder::{
    DecodeError, DecodeStats, DecodedFrame, DecoderBackend, DecoderConfig, EncodedPacket,
    HdrSideData, VideoCodec,
};
use crate::surface::{
    ColorRange, DmaBufPlane, HardwareHandle, MediaError, VideoFrameMetadata, VideoPixelFormat,
};

/// Maximum packets buffered between `send_packet` and `try_recv_frame`
/// before `send_packet` starts rejecting.
const MAX_PENDING_PACKETS: usize = 64;

/// Maximum H.264 reference pictures kept in the decoded-picture buffer.
const MAX_DPB_FRAMES: usize = 16;

/// Extra pool slack beyond the DPB so an emitted frame is not recycled while
/// still plausibly referenced.
const POOL_SLACK: usize = 4;

/// Default coded size used when `DecoderConfig::{width,height}` are zero and
/// the stream has not yet signalled an SPS.
const DEFAULT_CODED_SIZE: (u32, u32) = (1920, 1080);

/// Converts a [`VideoCodec`] to the `VAProfile` candidates to try, in order.
fn profile_candidates(codec: VideoCodec) -> &'static [va::VAProfile::Type] {
    match codec {
        VideoCodec::H264 => &[
            va::VAProfile::VAProfileH264High,
            va::VAProfile::VAProfileH264Main,
            va::VAProfile::VAProfileH264ConstrainedBaseline,
        ],
        VideoCodec::Hevc => &[
            va::VAProfile::VAProfileHEVCMain,
            va::VAProfile::VAProfileHEVCMain10,
        ],
        VideoCodec::Av1 => &[va::VAProfile::VAProfileAV1Profile0],
        VideoCodec::Vp9 => &[va::VAProfile::VAProfileVP9Profile0],
    }
}

/// Whether the negotiated profile decodes to >8-bit 4:2:0 (P010 surfaces).
fn profile_is_10bit(profile: va::VAProfile::Type) -> bool {
    profile == va::VAProfile::VAProfileHEVCMain10 || profile == va::VAProfile::VAProfileH264High10
}

/// Maps an exported DRM/VAAPI fourcc to a [`VideoPixelFormat`].
fn fourcc_to_pixel_format(fourcc: u32) -> Option<VideoPixelFormat> {
    match fourcc {
        f if f == va::VA_FOURCC_NV12 => Some(VideoPixelFormat::Nv12),
        f if f == va::VA_FOURCC_P010 || f == va::VA_FOURCC_P016 => Some(VideoPixelFormat::P010),
        f if f == va::VA_FOURCC_RGBA || f == va::VA_FOURCC_BGRA => Some(VideoPixelFormat::Rgba8),
        _ => None,
    }
}

/// Wraps a [`VaError`] as a fatal decoder [`MediaError`].
fn va_fatal(context: &str, err: va::VaError) -> MediaError {
    DecodeError::Fatal(format!("{context}: {err}")).into()
}

/// A decoded reference surface tracked in the DPB.
#[derive(Clone, Copy)]
struct DpbEntry {
    /// `VASurfaceID` of the reference surface.
    surface_id: va::VASurfaceID,
    /// `frame_num` the surface was decoded with.
    frame_num: u32,
    /// Approximate picture order count used for `TopFieldOrderCnt`.
    poc: i32,
}

/// Linux VAAPI hardware decoder producing [`HardwareHandle::DmaBuf`] frames.
///
/// See the module documentation for codec coverage and the documented
/// simplifications of the H.264 path.
///
/// # Examples
///
/// ```no_run
/// use martensite_media_platform::decoder::vaapi::VaapiDecoder;
/// use martensite_media_platform::decoder::{DecoderConfig, VideoCodec};
///
/// // Fails on machines without a VAAPI-capable GPU.
/// let dec = VaapiDecoder::create(&DecoderConfig::new(VideoCodec::H264, 1920, 1080));
/// ```
pub struct VaapiDecoder {
    display: Rc<va::Display>,
    /// Kept alive so the context's config is not destroyed early.
    _config: va::Config,
    context: Rc<va::Context>,
    /// Surfaces available for the next picture (round-robin recycling).
    surface_pool: VecDeque<va::Surface<()>>,
    /// VASurfaceIDs of the most recent reference frames, oldest first.
    dpb: VecDeque<DpbEntry>,
    /// Surface-creation parameters remembered for lazy pool growth.
    rt_format: u32,
    fourcc: u32,
    coded_width: u32,
    coded_height: u32,
    /// Negotiated output format (from the selected VA profile).
    negotiated: VideoPixelFormat,
    /// HDR/colour side data recovered from SPS VUI, if any.
    hdr: Option<HdrSideData>,
    /// Colour range signalled by the stream (SPS VUI), default limited.
    range: ColorRange,
    /// Visible (crop-applied) dimensions once an SPS is known.
    visible_size: (u32, u32),
    stats: DecodeStats,
    pending: VecDeque<EncodedPacket>,
    seen_keyframe: bool,
    frame_index: u64,
    codec: VideoCodec,
    /// H.264 parameter-set store and framing state.
    h264: h264::State,
}

// SAFETY: all libva objects (`Rc<Display>`, `Rc<Context>`, `Surface`) are only
// touched through `&mut self` methods; the `&self` accessors return plain Rust
// data. Moving the decoder between threads therefore cannot create concurrent
// access to the non-`Send` `Rc` internals, and `&VaapiDecoder` sharing is safe
// because no `&self` method reaches into libva.
unsafe impl Send for VaapiDecoder {}
// SAFETY: see the `Send` impl — `&self` methods never touch FFI state.
unsafe impl Sync for VaapiDecoder {}

impl VaapiDecoder {
    /// Creates a VAAPI decoder for `config`.
    ///
    /// Probes `/dev/dri/renderD128`…`renderD191` for a device on which
    /// `vaGetDisplayDRM` + `vaInitialize` succeed, negotiates the first
    /// supported `VAProfile` for `config.codec`, and allocates a
    /// surface/context pair for `VAEntrypointVLD`.
    ///
    /// # Errors
    ///
    /// - [`MediaError::ImportFailed`] wrapping [`DecodeError::UnsupportedCodec`]
    ///   when no candidate profile/entrypoint is supported by the driver.
    /// - [`MediaError::ImportFailed`] wrapping [`DecodeError::Fatal`] on any
    ///   libva failure (no device, `vaCreateConfig`/`vaCreateSurfaces`/
    ///   `vaCreateContext` errors).
    ///
    /// VAAPI is hardware-only: there is no in-backend software path. When
    /// this fails and `config.allow_software` is set, callers should fall
    /// back to the `decoder-ffmpeg` backend.
    pub fn create(config: &DecoderConfig) -> Result<Self, MediaError> {
        let display = va::Display::open().ok_or_else(|| {
            let hint = if config.allow_software {
                "; enable the `decoder-ffmpeg` backend for the software fallback"
            } else {
                ""
            };
            MediaError::from(DecodeError::Fatal(format!(
                "no usable VAAPI device found under /dev/dri/renderD*{hint}"
            )))
        })?;

        let profiles = display
            .query_config_profiles()
            .map_err(|e| va_fatal("vaQueryConfigProfiles", e))?;

        let entrypoint = va::VAEntrypoint::VAEntrypointVLD;
        let mut chosen: Option<va::VAProfile::Type> = None;
        for &profile in profile_candidates(config.codec) {
            if !profiles.contains(&profile) {
                continue;
            }
            let entrypoints = display
                .query_config_entrypoints(profile)
                .map_err(|e| va_fatal("vaQueryConfigEntrypoints", e))?;
            if entrypoints.contains(&entrypoint) {
                chosen = Some(profile);
                break;
            }
        }
        let profile = chosen.ok_or_else(|| {
            MediaError::from(DecodeError::UnsupportedCodec(format!(
                "no supported VAAPI profile for {:?}",
                config.codec
            )))
        })?;

        let negotiated = if profile_is_10bit(profile) {
            VideoPixelFormat::P010
        } else {
            VideoPixelFormat::Nv12
        };
        let (rt_format, fourcc) = match negotiated {
            VideoPixelFormat::P010 => (va::VA_RT_FORMAT_YUV420_10BPP, va::VA_FOURCC_P010),
            _ => (va::VA_RT_FORMAT_YUV420, va::VA_FOURCC_NV12),
        };

        let mut attrs = vec![va::VAConfigAttrib {
            type_: va::VAConfigAttribType::VAConfigAttribRTFormat,
            value: 0,
        }];
        display
            .get_config_attributes(profile, entrypoint, &mut attrs)
            .map_err(|e| va_fatal("vaGetConfigAttributes", e))?;
        if attrs[0].value == va::VA_ATTRIB_NOT_SUPPORTED || attrs[0].value & rt_format == 0 {
            return Err(DecodeError::UnsupportedCodec(format!(
                "driver does not support rt_format {rt_format:#x} for profile {profile}"
            ))
            .into());
        }
        attrs[0].value = rt_format;
        let va_config = display
            .create_config(attrs, profile, entrypoint)
            .map_err(|e| va_fatal("vaCreateConfig", e))?;

        let (mut coded_width, mut coded_height) = (config.width, config.height);
        if coded_width == 0 || coded_height == 0 {
            (coded_width, coded_height) = DEFAULT_CODED_SIZE;
        }
        // VAAPI surfaces are macroblock-aligned; round the coded size up.
        let mb_width = coded_width.div_ceil(16) * 16;
        let mb_height = coded_height.div_ceil(16) * 16;

        let pool_size = config.decode_ahead + MAX_DPB_FRAMES + POOL_SLACK;
        let surfaces = display
            .create_surfaces(
                rt_format,
                Some(fourcc),
                mb_width,
                mb_height,
                Some(va::UsageHint::USAGE_HINT_DECODER | va::UsageHint::USAGE_HINT_EXPORT),
                vec![(); pool_size],
            )
            .map_err(|e| va_fatal("vaCreateSurfaces", e))?;

        let context = display
            .create_context(&va_config, mb_width, mb_height, Some(&surfaces), true)
            .map_err(|e| va_fatal("vaCreateContext", e))?;

        let mut decoder = Self {
            display,
            _config: va_config,
            context,
            surface_pool: surfaces.into(),
            dpb: VecDeque::new(),
            rt_format,
            fourcc,
            coded_width: mb_width,
            coded_height: mb_height,
            negotiated,
            hdr: None,
            range: ColorRange::Limited,
            visible_size: (coded_width, coded_height),
            stats: DecodeStats {
                backend: Some(DecoderBackend::Vaapi),
                ..DecodeStats::default()
            },
            pending: VecDeque::new(),
            seen_keyframe: false,
            frame_index: 0,
            codec: config.codec,
            h264: h264::State::new(),
        };

        // Out-of-band extradata (`avcC` record) seeds the parameter-set store
        // and switches NAL framing to length-prefixed.
        if let Some(record) = &config.codec_config {
            decoder
                .h264
                .apply_codec_config(record)
                .map_err(MediaError::from)?;
        }

        Ok(decoder)
    }

    /// Enqueues one compressed access unit for decode.
    ///
    /// Applies the decoder contract: empty packets are rejected as
    /// [`DecodeError::StreamCorrupt`], delta packets before the first
    /// keyframe (or after `flush`) are rejected with
    /// [`DecodeError::NeedsKeyframe`], and codecs without a wired
    /// bitstream→parameter translation are rejected with
    /// [`DecodeError::UnsupportedCodec`].
    pub fn send_packet(&mut self, packet: &EncodedPacket) -> Result<(), MediaError> {
        if packet.data.is_empty() {
            self.stats.record_rejection();
            return Err(DecodeError::StreamCorrupt("empty packet".to_string()).into());
        }
        if self.codec != VideoCodec::H264 {
            self.stats.record_rejection();
            return Err(DecodeError::UnsupportedCodec(format!(
                "{:?} slice-level parameter translation is not implemented in the \
                 VAAPI backend yet (H.264 only)",
                self.codec
            ))
            .into());
        }
        if !packet.is_keyframe && !self.seen_keyframe {
            self.stats.record_rejection();
            return Err(DecodeError::NeedsKeyframe.into());
        }
        if self.pending.len() >= MAX_PENDING_PACKETS {
            self.stats.record_rejection();
            return Err(
                DecodeError::Fatal("vaapi: pending packet queue is full".to_string()).into(),
            );
        }
        if packet.is_keyframe {
            self.seen_keyframe = true;
        }
        self.stats.record_packet(packet.data.len());
        self.pending.push_back(packet.clone());
        Ok(())
    }

    /// Decodes pending packets until one produces a frame.
    ///
    /// A packet containing only parameter sets (SPS/PPS) produces no frame;
    /// the loop keeps draining until a picture is decoded or the queue is
    /// empty. Decoding is synchronous: `vaSyncSurface` blocks until the
    /// picture is complete, then the surface is exported as a dma-buf.
    pub fn try_recv_frame(&mut self) -> Result<Option<DecodedFrame>, MediaError> {
        while let Some(packet) = self.pending.pop_front() {
            if let Some(frame) = self.decode_packet(&packet)? {
                return Ok(Some(frame));
            }
        }
        Ok(None)
    }

    /// Signals end-of-stream. VAAPI decode is synchronous — every packet is
    /// fully processed (`vaSyncSurface` + `vaExportSurfaceHandle`) inside
    /// [`try_recv_frame`](Self::try_recv_frame), so no frames are held in a
    /// reorder buffer and there is nothing extra to drain. Callers should
    /// keep calling `try_recv_frame` until it returns `None`.
    ///
    /// # Errors
    ///
    /// Always succeeds.
    pub fn end_of_stream(&mut self) -> Result<(), MediaError> {
        Ok(())
    }

    /// Drops all queued packets and reference state; the next packet must be
    /// a keyframe. Surfaces were synced before export, so nothing in-flight
    /// remains to sync — the packet queue and DPB are cleared.
    pub fn flush(&mut self) -> Result<(), MediaError> {
        self.pending.clear();
        self.dpb.clear();
        self.seen_keyframe = false;
        // Parameter sets survive a flush (streams may not re-send them).
        Ok(())
    }

    /// The negotiated output pixel format (`Nv12`, or `P010` when a 10-bit
    /// profile such as `VAProfileHEVCMain10` was selected).
    pub fn negotiated_format(&self) -> VideoPixelFormat {
        self.negotiated
    }

    /// HDR/colour side data recovered from the SPS VUI
    /// (`colour_description` + `video_full_range_flag`).
    ///
    /// Mastering-display and content-light-level SEI parsing is out of scope:
    /// libva surfaces do not carry parsed SEI, and `VaapiDecoder` does not
    /// decode SEI NAL units — so `max_luminance_nits`/`max_cll`/`max_fall`/
    /// `dynamic_metadata` are always `None`. Returns `None` until an SPS with
    /// VUI colour information has been seen (or for non-H.264 streams).
    pub fn hdr_side_data(&self) -> Option<HdrSideData> {
        self.hdr.clone()
    }

    /// Rolling decode-path telemetry.
    pub fn stats(&self) -> &DecodeStats {
        &self.stats
    }

    /// Takes a surface from the recycling pool, growing it lazily if empty.
    ///
    /// The popped surface is removed from the DPB: it is about to be
    /// overwritten, so it can no longer serve as a reference.
    fn take_surface(&mut self) -> Result<va::Surface<()>, MediaError> {
        if let Some(surface) = self.surface_pool.pop_front() {
            self.dpb.retain(|e| e.surface_id != surface.id());
            return Ok(surface);
        }
        let mut surfaces = self
            .display
            .create_surfaces(
                self.rt_format,
                Some(self.fourcc),
                self.coded_width,
                self.coded_height,
                Some(va::UsageHint::USAGE_HINT_DECODER | va::UsageHint::USAGE_HINT_EXPORT),
                vec![()],
            )
            .map_err(|e| va_fatal("vaCreateSurfaces", e))?;
        let surface = surfaces.pop().ok_or_else(|| {
            MediaError::from(DecodeError::Fatal(
                "vaCreateSurfaces returned no surfaces".to_string(),
            ))
        })?;
        self.dpb.retain(|e| e.surface_id != surface.id());
        Ok(surface)
    }

    /// Returns a surface to the recycling pool after its frame was exported.
    fn release_surface(&mut self, surface: va::Surface<()>) {
        self.surface_pool.push_back(surface);
    }

    /// Decodes one access unit. Returns `Ok(None)` for packets that carried
    /// no slices (parameter-only updates).
    fn decode_packet(
        &mut self,
        packet: &EncodedPacket,
    ) -> Result<Option<DecodedFrame>, MediaError> {
        match self.codec {
            VideoCodec::H264 => self.decode_h264(packet),
            other => Err(DecodeError::UnsupportedCodec(format!(
                "{other:?} decode is not implemented in the VAAPI backend"
            ))
            .into()),
        }
    }

    /// H.264 decode: NAL scan → parameter-set update → slice submission →
    /// surface export. See the module docs for simplifications.
    fn decode_h264(&mut self, packet: &EncodedPacket) -> Result<Option<DecodedFrame>, MediaError> {
        let started = Instant::now();
        let nals = self.h264.split_nals(&packet.data)?;

        let mut slices: Vec<h264::ParsedSlice> = Vec::new();
        for nal in nals {
            if nal.is_empty() {
                continue;
            }
            let nal_type = nal[0] & 0x1f;
            let nal_ref_idc = (nal[0] >> 5) & 0x03;
            match nal_type {
                // SPS / PPS update the parameter-set store.
                7 => {
                    let sps = h264::Sps::parse(&h264::unescape(&nal[1..]))?;
                    self.apply_sps(&sps);
                    self.h264.sps.insert(sps.id, sps);
                }
                8 => {
                    let pps = h264::Pps::parse(&h264::unescape(&nal[1..]))?;
                    self.h264.pps.insert(pps.id, pps);
                }
                // Coded slices (non-IDR / IDR).
                1 | 5 => slices.push((nal, nal_ref_idc, nal_type == 5)),
                // AUD, SEI, filler, …: ignored. SEI mastering-display parsing
                // is documented out of scope (see `hdr_side_data`).
                _ => {}
            }
        }

        if slices.is_empty() {
            return Ok(None);
        }

        // Parse slice headers and accumulate the slice data buffer.
        let mut slice_params = va::SliceParameterBufferH264::new_array();
        let mut slice_data: Vec<u8> = Vec::new();
        let mut first: Option<h264::SliceHeader> = None;
        for (nal, nal_ref_idc, is_idr) in slices {
            let rbsp = h264::unescape(&nal[1..]);
            let header = h264::SliceHeader::parse(&rbsp, nal_ref_idc, is_idr, &self.h264)?;
            // slice_data = NAL header byte + EPB-stripped RBSP.
            let offset = slice_data.len() as u32;
            slice_data.push(nal[0]);
            slice_data.extend_from_slice(&rbsp);
            header.fill_slice_parameter(
                &mut slice_params,
                offset,
                (1 + rbsp.len()) as u32,
                &self.dpb,
            );
            if first.is_none() {
                first = Some(header);
            }
        }

        let first = first.ok_or_else(|| {
            MediaError::from(DecodeError::StreamCorrupt(
                "h264: no parseable slice".to_string(),
            ))
        })?;
        let pps = self
            .h264
            .pps
            .get(&(first.pps_id as u8))
            .cloned()
            .ok_or_else(|| {
                MediaError::from(DecodeError::StreamCorrupt(
                    "h264: slice references unknown PPS".to_string(),
                ))
            })?;
        let sps = self.h264.sps.get(&pps.sps_id).cloned().ok_or_else(|| {
            MediaError::from(DecodeError::StreamCorrupt(
                "h264: slice references unknown SPS".to_string(),
            ))
        })?;

        let surface = self.take_surface()?;
        let result = self.submit_h264(
            &first,
            &sps,
            &pps,
            slice_params,
            slice_data,
            surface,
            packet,
        );
        self.stats.record_frame(started.elapsed().as_nanos() as u64);
        result.map(Some)
    }

    /// Builds the `VABuffer`s for one H.264 picture, submits it, syncs, and
    /// exports the decoded surface as a dma-buf frame.
    #[allow(clippy::too_many_arguments)]
    fn submit_h264(
        &mut self,
        first: &h264::SliceHeader,
        sps: &h264::Sps,
        pps: &h264::Pps,
        slice_params: va::SliceParameterBufferH264,
        slice_data: Vec<u8>,
        surface: va::Surface<()>,
        packet: &EncodedPacket,
    ) -> Result<DecodedFrame, MediaError> {
        let is_reference = first.nal_ref_idc != 0;
        let poc = first.poc(sps);

        let curr_pic = va::PictureH264::new(
            surface.id(),
            first.frame_num,
            if is_reference {
                va::VA_PICTURE_H264_SHORT_TERM_REFERENCE
            } else {
                0
            },
            poc,
            poc,
        );

        // The ReferenceFrames array lists every currently decoded reference
        // picture; unused slots are VA_PICTURE_H264_INVALID.
        let mut reference_frames: [va::PictureH264; 16] =
            std::array::from_fn(|_| h264::invalid_ref_pic());
        for (i, entry) in self.dpb.iter().take(16).enumerate() {
            reference_frames[i] = va::PictureH264::new(
                entry.surface_id,
                entry.frame_num,
                va::VA_PICTURE_H264_SHORT_TERM_REFERENCE,
                entry.poc,
                entry.poc,
            );
        }

        let seq_fields = sps.seq_fields();
        let pic_fields = pps.pic_fields(first.field_pic_flag, is_reference);
        let pic_param = va::PictureParameterBufferH264::new(
            curr_pic,
            reference_frames,
            (sps.pic_width_in_mbs - 1) as u16,
            (sps.pic_height_in_mbs - 1) as u16,
            sps.bit_depth_luma_minus8,
            sps.bit_depth_chroma_minus8,
            sps.max_num_ref_frames.min(255) as u8,
            &seq_fields,
            0, // num_slice_groups_minus1 — FMO rejected at parse time
            0, // slice_group_map_type
            0, // slice_group_change_rate_minus1
            pps.pic_init_qp_minus26,
            pps.pic_init_qs_minus26,
            pps.chroma_qp_index_offset,
            pps.second_chroma_qp_index_offset,
            &pic_fields,
            first.frame_num.min(u16::MAX as u32) as u16,
        );

        // Default (flat) H.264 scaling lists — see module docs.
        let iq_matrix = va::IQMatrixBufferH264::new([[16u8; 16]; 6], [[16u8; 64]; 2]);

        let mut picture = va::Picture::new(packet.pts_nanos, Rc::clone(&self.context), surface);
        for buffer_type in [
            va::BufferType::PictureParameter(va::PictureParameter::H264(pic_param)),
            va::BufferType::IQMatrix(va::IQMatrix::H264(iq_matrix)),
            va::BufferType::SliceParameter(va::SliceParameter::H264(slice_params)),
            va::BufferType::SliceData(slice_data),
        ] {
            let buffer = self
                .context
                .create_buffer(buffer_type)
                .map_err(|e| va_fatal("vaCreateBuffer", e))?;
            picture.add_buffer(buffer);
        }

        let picture = picture
            .begin()
            .map_err(|e| va_fatal("vaBeginPicture", e))?
            .render()
            .map_err(|e| va_fatal("vaRenderPicture", e))?
            .end()
            .map_err(|e| va_fatal("vaEndPicture", e))?;
        let picture = picture
            .sync()
            .map_err(|(e, _)| va_fatal("vaSyncSurface", e))?;

        let surface = picture.take_surface().map_err(|_| {
            MediaError::from(DecodeError::Fatal(
                "decoded surface still referenced".to_string(),
            ))
        })?;

        // Export the synced surface as a dma-buf. `export_prime` calls
        // `vaExportSurfaceHandle` with `VA_SURFACE_ATTRIB_MEM_TYPE_DRM_PRIME_2`;
        // cros-libva hardcodes `READ_ONLY | COMPOSED_LAYERS` (the task asked
        // for `SEPARATE_LAYERS | READ_WRITE`, but the public cros-libva API
        // fixes these flags — `Display::handle()` is crate-private so the
        // flags cannot be overridden. COMPOSED_LAYERS is what we want anyway:
        // an NV12 surface exports as one DRM layer with 2 planes).
        let desc = surface
            .export_prime()
            .map_err(|e| va_fatal("vaExportSurfaceHandle", e))?;
        let handle = drm_prime_to_dmabuf_handle(&desc)?;

        // Track the surface in the DPB when it is a reference frame, then
        // recycle it (the exported fd keeps the memory alive).
        if is_reference {
            self.dpb.push_back(DpbEntry {
                surface_id: surface.id(),
                frame_num: first.frame_num,
                poc,
            });
            while self.dpb.len() > MAX_DPB_FRAMES {
                self.dpb.pop_front();
            }
        }
        self.release_surface(surface);

        let format = fourcc_to_pixel_format(desc.fourcc).unwrap_or(self.negotiated);
        let mut metadata =
            VideoFrameMetadata::new(self.visible_size.0, self.visible_size.1, format, self.range);
        metadata.pts_nanos = packet.pts_nanos;
        metadata.duration_nanos = packet.duration_nanos;
        self.frame_index = self.frame_index.saturating_add(1);
        metadata.frame_index = self.frame_index;

        let mut frame = DecodedFrame::new(handle, metadata);
        if let Some(ref hdr) = self.hdr {
            frame.hdr = Some(hdr.clone());
        }
        Ok(frame)
    }

    /// Applies a parsed SPS: updates visible size, colour range, and HDR
    /// side data from the VUI when present.
    fn apply_sps(&mut self, sps: &h264::Sps) {
        let (w, h) = sps.visible_size();
        if w != 0 && h != 0 {
            self.visible_size = (w, h);
        }
        if let Some(vui) = &sps.vui {
            self.range = if vui.video_full_range_flag {
                ColorRange::Full
            } else {
                ColorRange::Limited
            };
            if let Some(colour) = &vui.colour {
                self.hdr = Some(HdrSideData {
                    eotf_code: colour.transfer_characteristics,
                    primaries_code: colour.colour_primaries,
                    full_range: vui.video_full_range_flag,
                    max_luminance_nits: None,
                    min_luminance_nits: None,
                    max_cll: None,
                    max_fall: None,
                    dynamic_metadata: None,
                });
            }
        }
    }
}

/// Translates a `VADRMPRIMESurfaceDescriptor` (composed-layers export) into a
/// [`HardwareHandle::DmaBuf`] carrying every exported object fd plus the
/// per-plane `(object_index, offset, pitch)` table.
///
/// Every layer's planes are flattened into `planes` in export order — for
/// NV12 this yields `[Y, UV]` — so `import_external_planes` can resolve
/// `plane_index` 0/1 directly. All object fds are `dup`ed so dropping `desc`
/// cannot invalidate the handle. `modifier` is taken from the first object:
/// `vaExportSurfaceHandle` applies one modifier to the whole surface, and
/// drivers that emit per-object modifiers are not currently differentiated
/// (a documented limitation — no known VAAPI driver mixes modifiers within a
/// single surface export).
fn drm_prime_to_dmabuf_handle(
    desc: &va::DrmPrimeSurfaceDescriptor,
) -> Result<HardwareHandle, MediaError> {
    if desc.objects.is_empty() {
        return Err(
            DecodeError::Fatal("vaExportSurfaceHandle returned no objects".to_string()).into(),
        );
    }

    // Dup every object fd so the `OwnedFd`s inside `desc` keep their own
    // copies — `try_clone_to_owned` is plain F_DUPFD_CLOEXEC, no unsafe.
    let mut objects = Vec::with_capacity(desc.objects.len());
    for object in &desc.objects {
        objects.push(
            object
                .fd
                .try_clone_to_owned()
                .map_err(|e| {
                    MediaError::from(DecodeError::Fatal(format!("dup of exported fd: {e}")))
                })?
                .into_raw_fd(),
        );
    }
    let modifier = desc.objects[0].drm_format_modifier;

    let mut planes = Vec::new();
    for layer in &desc.layers {
        for i in 0..layer.num_planes.min(4) as usize {
            planes.push(DmaBufPlane {
                object_index: u32::from(layer.object_index[i]),
                offset: layer.offset[i],
                stride: layer.pitch[i],
            });
        }
    }
    if planes.is_empty() {
        return Err(
            DecodeError::Fatal("vaExportSurfaceHandle returned no planes".to_string()).into(),
        );
    }
    Ok(HardwareHandle::DmaBuf {
        objects,
        modifier,
        planes,
    })
}

// ---------------------------------------------------------------------------
// H.264 bitstream parsing — Annex-B / avcC framing, EPB removal, Exp-Golomb,
// SPS/PPS/slice-header. Self-contained and hardware-independent so the
// `#[cfg(test)]` tests exercise it on any host.
// ---------------------------------------------------------------------------
mod h264 {
    use std::collections::{HashMap, VecDeque};

    use cros_libva as va;

    use crate::decoder::DecodeError;

    use super::DpbEntry;

    /// Per-decoder H.264 state: parameter sets plus NAL framing mode.
    pub struct State {
        /// Active SPS store, keyed by `seq_parameter_set_id`.
        pub sps: HashMap<u8, Sps>,
        /// Active PPS store, keyed by `pic_parameter_set_id`.
        pub pps: HashMap<u8, Pps>,
        /// `Some(n)` for length-prefixed (`avcC`) NAL framing, `None` for
        /// Annex-B start codes.
        nal_len_size: Option<usize>,
    }

    impl State {
        pub fn new() -> Self {
            Self {
                sps: HashMap::new(),
                pps: HashMap::new(),
                nal_len_size: None,
            }
        }

        /// Parses an `avcC` decoder configuration record: loads the contained
        /// SPS/PPS and selects length-prefixed NAL framing.
        pub fn apply_codec_config(&mut self, record: &[u8]) -> Result<(), DecodeError> {
            // avcC layout: version(1) profile(1) compat(1) level(1)
            // len_size_minus1|0xFC(1) num_sps|0xE0(1) sps... num_pps(1) pps...
            if record.len() < 7 || record[0] != 1 {
                return Err(DecodeError::StreamCorrupt(
                    "h264: malformed avcC record".to_string(),
                ));
            }
            let len_size = (record[4] & 0x03) + 1;
            self.nal_len_size = Some(len_size as usize);
            let num_sps = record[5] & 0x1f;
            let mut pos = 6usize;
            for _ in 0..num_sps {
                let (nal, next) = read_length_prefixed(record, pos, 2)?;
                let sps = Sps::parse(&unescape(&nal[1..]))?;
                self.sps.insert(sps.id, sps);
                pos = next;
            }
            if pos >= record.len() {
                return Ok(());
            }
            let num_pps = record[pos];
            pos += 1;
            for _ in 0..num_pps {
                let (nal, next) = read_length_prefixed(record, pos, 2)?;
                let pps = Pps::parse(&unescape(&nal[1..]))?;
                self.pps.insert(pps.id, pps);
                pos = next;
            }
            Ok(())
        }

        /// Splits one access unit into NAL units (excluding start codes /
        /// length prefixes).
        pub fn split_nals<'a>(&self, data: &'a [u8]) -> Result<Vec<&'a [u8]>, DecodeError> {
            match self.nal_len_size {
                Some(n) => split_length_prefixed(data, n),
                None => Ok(split_annex_b(data)),
            }
        }
    }

    /// Reads a `len_size`-byte big-endian length then that many payload bytes.
    fn read_length_prefixed(
        data: &[u8],
        pos: usize,
        len_size: usize,
    ) -> Result<(&[u8], usize), DecodeError> {
        if pos + len_size > data.len() {
            return Err(DecodeError::StreamCorrupt(
                "h264: truncated length prefix".to_string(),
            ));
        }
        let mut len = 0usize;
        for &b in &data[pos..pos + len_size] {
            len = (len << 8) | b as usize;
        }
        let start = pos + len_size;
        let end = start + len;
        if end > data.len() {
            return Err(DecodeError::StreamCorrupt(
                "h264: NAL length exceeds packet".to_string(),
            ));
        }
        Ok((&data[start..end], end))
    }

    fn split_length_prefixed<'a>(
        data: &'a [u8],
        len_size: usize,
    ) -> Result<Vec<&'a [u8]>, DecodeError> {
        let mut out = Vec::new();
        let mut pos = 0usize;
        while pos < data.len() {
            let (nal, next) = read_length_prefixed(data, pos, len_size)?;
            if !nal.is_empty() {
                out.push(nal);
            }
            pos = next;
        }
        Ok(out)
    }

    /// Splits an Annex-B byte stream into NAL payloads (without start codes,
    /// trailing zero bytes trimmed).
    fn split_annex_b(data: &[u8]) -> Vec<&[u8]> {
        let mut starts = Vec::new();
        let mut i = 0usize;
        while i + 3 <= data.len() {
            if data[i] == 0 && data[i + 1] == 0 && data[i + 2] == 1 {
                starts.push(i + 3);
            }
            i += 1;
        }
        let mut out = Vec::new();
        for (k, &start) in starts.iter().enumerate() {
            let end = if k + 1 < starts.len() {
                // The next start code's `00 00` prefix begins 2 bytes before
                // its recorded payload start; strip trailing zeros too.
                let mut e = starts[k + 1] - 3;
                while e > start && data[e - 1] == 0 {
                    e -= 1;
                }
                e
            } else {
                data.len()
            };
            if end > start {
                out.push(&data[start..end]);
            }
        }
        // No start codes at all: treat the whole packet as a single raw NAL.
        if out.is_empty() && !data.is_empty() && starts.is_empty() {
            out.push(data);
        }
        out
    }

    /// Removes emulation-prevention bytes (`00 00 03` → `00 00`).
    pub fn unescape(nal: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(nal.len());
        let mut zeros = 0;
        for &b in nal {
            if zeros >= 2 && b == 3 {
                zeros = 0;
                continue;
            }
            zeros = if b == 0 { zeros + 1 } else { 0 };
            out.push(b);
        }
        out
    }

    /// MSB-first bit reader over an RBSP buffer.
    struct BitReader<'a> {
        data: &'a [u8],
        bit_pos: usize,
    }

    impl<'a> BitReader<'a> {
        fn new(data: &'a [u8]) -> Self {
            Self { data, bit_pos: 0 }
        }

        fn corrupt(msg: &str) -> DecodeError {
            DecodeError::StreamCorrupt(format!("h264: {msg}"))
        }

        fn read_bit(&mut self) -> Result<u32, DecodeError> {
            let byte = self
                .data
                .get(self.bit_pos / 8)
                .ok_or_else(|| Self::corrupt("bitstream overrun"))?;
            let bit = (byte >> (7 - (self.bit_pos % 8))) & 1;
            self.bit_pos += 1;
            Ok(bit as u32)
        }

        fn read_bits(&mut self, n: u32) -> Result<u32, DecodeError> {
            if n > 32 {
                return Err(Self::corrupt("read_bits > 32"));
            }
            let mut v = 0u32;
            for _ in 0..n {
                v = (v << 1) | self.read_bit()?;
            }
            Ok(v)
        }

        /// Unsigned Exp-Golomb (`ue`).
        fn read_ue(&mut self) -> Result<u32, DecodeError> {
            let mut leading_zeros = 0u32;
            while self.read_bit()? == 0 {
                leading_zeros += 1;
                if leading_zeros > 31 {
                    return Err(Self::corrupt("exp-golomb overflow"));
                }
            }
            let suffix = self.read_bits(leading_zeros)?;
            Ok((1 << leading_zeros) - 1 + suffix)
        }

        /// Signed Exp-Golomb (`se`).
        fn read_se(&mut self) -> Result<i32, DecodeError> {
            let v = self.read_ue()? as i32;
            Ok(if v & 1 == 1 { (v + 1) / 2 } else { -(v / 2) })
        }

        /// Whether meaningful RBSP bits remain (i.e. the current position is
        /// before the `rbsp_stop_one_bit`, which is the last set bit).
        fn more_rbsp_data(&self) -> bool {
            // Find the index of the last '1' bit in the buffer.
            let mut last_one: Option<usize> = None;
            for (i, &b) in self.data.iter().enumerate() {
                if b != 0 {
                    for bit in 0..8 {
                        if b & (0x80 >> bit) != 0 {
                            last_one = Some(i * 8 + bit);
                        }
                    }
                }
            }
            match last_one {
                Some(pos) => self.bit_pos < pos,
                None => false,
            }
        }

        /// Skips an H.264 `scaling_list` (7.3.2.1.1.1) of `size` entries.
        fn skip_scaling_list(&mut self, size: usize) -> Result<(), DecodeError> {
            let mut last_scale = 8i32;
            let mut next_scale = 8i32;
            for _ in 0..size {
                if next_scale != 0 {
                    let delta = self.read_se()?;
                    next_scale = (last_scale + delta + 256) % 256;
                }
                last_scale = if next_scale == 0 {
                    last_scale
                } else {
                    next_scale
                };
            }
            Ok(())
        }
    }

    /// VUI colour information relevant to HDR/range signalling.
    pub struct VuiColour {
        pub colour_primaries: u16,
        pub transfer_characteristics: u16,
    }

    /// Parsed VUI subset.
    pub struct Vui {
        pub video_full_range_flag: bool,
        pub colour: Option<VuiColour>,
    }

    /// Parsed `seq_parameter_set` fields needed by VAAPI.
    pub struct Sps {
        pub id: u8,
        pub profile_idc: u8,
        pub level_idc: u8,
        pub chroma_format_idc: u32,
        pub separate_colour_plane_flag: bool,
        pub bit_depth_luma_minus8: u8,
        pub bit_depth_chroma_minus8: u8,
        pub log2_max_frame_num_minus4: u32,
        pub pic_order_cnt_type: u32,
        pub log2_max_pic_order_cnt_lsb_minus4: u32,
        pub delta_pic_order_always_zero_flag: bool,
        pub max_num_ref_frames: u32,
        pub gaps_in_frame_num_value_allowed_flag: bool,
        pub pic_width_in_mbs: u32,
        pub pic_height_in_map_units: u32,
        pub frame_mbs_only_flag: bool,
        pub mb_adaptive_frame_field_flag: bool,
        pub direct_8x8_inference_flag: bool,
        pub crop: (u32, u32, u32, u32),
        pub vui: Option<Vui>,
    }

    impl Sps {
        /// Frame height in macroblocks (accounts for interlaced map units).
        pub fn pic_height_in_mbs(&self) -> u32 {
            self.pic_height_in_map_units * if self.frame_mbs_only_flag { 1 } else { 2 }
        }

        /// Crop-applied visible size in pixels.
        pub fn visible_size(&self) -> (u32, u32) {
            // 4:2:0 crop unit: 2 luma samples in each direction.
            let (unit_x, unit_y) = match self.chroma_format_idc {
                0 | 3 => (1, 2 - self.frame_mbs_only_flag as u32),
                1 => (2, 2 * (2 - self.frame_mbs_only_flag as u32)),
                2 => (2, 2 - self.frame_mbs_only_flag as u32),
                _ => (0, 0),
            };
            let w = self
                .pic_width_in_mbs
                .saturating_mul(16)
                .saturating_sub((self.crop.0 + self.crop.1) * unit_x);
            let h = self
                .pic_height_in_mbs()
                .saturating_mul(16)
                .saturating_sub((self.crop.2 + self.crop.3) * unit_y);
            (w, h)
        }

        /// Builds the `seq_fields` bitfield for `VAPictureParameterBufferH264`.
        pub fn seq_fields(&self) -> va::H264SeqFields {
            va::H264SeqFields::new(
                self.chroma_format_idc,
                0, // residual_colour_transform_flag
                self.gaps_in_frame_num_value_allowed_flag as u32,
                self.frame_mbs_only_flag as u32,
                self.mb_adaptive_frame_field_flag as u32,
                self.direct_8x8_inference_flag as u32,
                1, // min_luma_bi_pred_size8x8 (default)
                self.log2_max_frame_num_minus4,
                self.pic_order_cnt_type,
                self.log2_max_pic_order_cnt_lsb_minus4,
                self.delta_pic_order_always_zero_flag as u32,
            )
        }

        /// Parses an SPS RBSP (after EPB removal).
        pub fn parse(rbsp: &[u8]) -> Result<Self, DecodeError> {
            let mut br = BitReader::new(rbsp);
            let profile_idc = br.read_bits(8)? as u8;
            let _constraint_flags = br.read_bits(8)?;
            let level_idc = br.read_bits(8)? as u8;
            let id = br.read_ue()?;
            if id > 31 {
                return Err(BitReader::corrupt("sps id out of range"));
            }

            let mut chroma_format_idc = 1u32;
            let mut separate_colour_plane_flag = false;
            let mut bit_depth_luma_minus8 = 0u8;
            let mut bit_depth_chroma_minus8 = 0u8;
            // High-family profiles carry the chroma/depth extension.
            if matches!(
                profile_idc,
                100 | 110 | 122 | 244 | 44 | 83 | 86 | 118 | 128 | 138 | 139 | 134 | 135
            ) {
                chroma_format_idc = br.read_ue()?;
                if chroma_format_idc == 3 {
                    separate_colour_plane_flag = br.read_bit()? != 0;
                }
                bit_depth_luma_minus8 = br.read_ue()? as u8;
                bit_depth_chroma_minus8 = br.read_ue()? as u8;
                let _qpprime = br.read_bit()?;
                if br.read_bit()? != 0 {
                    // seq_scaling_matrix_present_flag — consume and discard;
                    // VAAPI gets flat defaults (documented simplification).
                    let count = if chroma_format_idc == 3 { 12 } else { 8 };
                    for i in 0..count {
                        if br.read_bit()? != 0 {
                            br.skip_scaling_list(if i < 6 { 16 } else { 64 })?;
                        }
                    }
                }
            }

            let log2_max_frame_num_minus4 = br.read_ue()?;
            let pic_order_cnt_type = br.read_ue()?;
            let mut log2_max_pic_order_cnt_lsb_minus4 = 0;
            let mut delta_pic_order_always_zero_flag = false;
            match pic_order_cnt_type {
                0 => log2_max_pic_order_cnt_lsb_minus4 = br.read_ue()?,
                1 => {
                    delta_pic_order_always_zero_flag = br.read_bit()? != 0;
                    let _offset_for_non_ref_pic = br.read_se()?;
                    let _offset_for_top_to_bottom_field = br.read_se()?;
                    let n = br.read_ue()?;
                    for _ in 0..n {
                        let _ = br.read_se()?;
                    }
                }
                2 => {}
                _ => return Err(BitReader::corrupt("invalid pic_order_cnt_type")),
            }

            let max_num_ref_frames = br.read_ue()?;
            let gaps_in_frame_num_value_allowed_flag = br.read_bit()? != 0;
            let pic_width_in_mbs = br.read_ue()? + 1;
            let pic_height_in_map_units = br.read_ue()? + 1;
            let frame_mbs_only_flag = br.read_bit()? != 0;
            let mut mb_adaptive_frame_field_flag = false;
            if !frame_mbs_only_flag {
                mb_adaptive_frame_field_flag = br.read_bit()? != 0;
            }
            let direct_8x8_inference_flag = br.read_bit()? != 0;
            let mut crop = (0, 0, 0, 0);
            if br.read_bit()? != 0 {
                crop = (br.read_ue()?, br.read_ue()?, br.read_ue()?, br.read_ue()?);
            }
            let vui = if br.read_bit()? != 0 {
                Some(Self::parse_vui(&mut br)?)
            } else {
                None
            };

            Ok(Self {
                id: id as u8,
                profile_idc,
                level_idc,
                chroma_format_idc,
                separate_colour_plane_flag,
                bit_depth_luma_minus8,
                bit_depth_chroma_minus8,
                log2_max_frame_num_minus4,
                pic_order_cnt_type,
                log2_max_pic_order_cnt_lsb_minus4,
                delta_pic_order_always_zero_flag,
                max_num_ref_frames,
                gaps_in_frame_num_value_allowed_flag,
                pic_width_in_mbs,
                pic_height_in_map_units,
                frame_mbs_only_flag,
                mb_adaptive_frame_field_flag,
                direct_8x8_inference_flag,
                crop,
                vui,
            })
        }

        /// Parses the VUI subset we care about (range + colour description);
        /// trailing VUI fields are ignored.
        fn parse_vui(br: &mut BitReader<'_>) -> Result<Vui, DecodeError> {
            let mut full_range = false;
            let mut colour = None;
            if br.read_bit()? != 0 {
                // aspect_ratio_info_present_flag
                let idc = br.read_bits(8)?;
                if idc == 255 {
                    let _sar_w = br.read_bits(16)?;
                    let _sar_h = br.read_bits(16)?;
                }
            }
            if br.read_bit()? != 0 {
                // overscan_info_present_flag
                let _ = br.read_bit()?;
            }
            if br.read_bit()? != 0 {
                // video_signal_type_present_flag
                let _video_format = br.read_bits(3)?;
                full_range = br.read_bit()? != 0;
                if br.read_bit()? != 0 {
                    // colour_description_present_flag
                    colour = Some(VuiColour {
                        colour_primaries: br.read_bits(8)? as u16,
                        transfer_characteristics: br.read_bits(8)? as u16,
                    });
                    let _matrix_coefficients = br.read_bits(8)?;
                }
            }
            Ok(Vui {
                video_full_range_flag: full_range,
                colour,
            })
        }
    }

    /// Parsed `pic_parameter_set` fields needed by VAAPI.
    pub struct Pps {
        pub id: u8,
        pub sps_id: u8,
        pub entropy_coding_mode_flag: bool,
        pub pic_order_present_flag: bool,
        pub num_ref_idx_l0_default_active_minus1: u32,
        pub num_ref_idx_l1_default_active_minus1: u32,
        pub weighted_pred_flag: bool,
        pub weighted_bipred_idc: u32,
        pub pic_init_qp_minus26: i8,
        pub pic_init_qs_minus26: i8,
        pub chroma_qp_index_offset: i8,
        pub second_chroma_qp_index_offset: i8,
        pub deblocking_filter_control_present_flag: bool,
        pub constrained_intra_pred_flag: bool,
        pub redundant_pic_cnt_present_flag: bool,
        pub transform_8x8_mode_flag: bool,
    }

    impl Pps {
        /// Builds the `pic_fields` bitfield for `VAPictureParameterBufferH264`.
        pub fn pic_fields(
            &self,
            field_pic_flag: bool,
            reference_pic_flag: bool,
        ) -> va::H264PicFields {
            va::H264PicFields::new(
                self.entropy_coding_mode_flag as u32,
                self.weighted_pred_flag as u32,
                self.weighted_bipred_idc,
                self.transform_8x8_mode_flag as u32,
                field_pic_flag as u32,
                self.constrained_intra_pred_flag as u32,
                self.pic_order_present_flag as u32,
                self.deblocking_filter_control_present_flag as u32,
                self.redundant_pic_cnt_present_flag as u32,
                reference_pic_flag as u32,
            )
        }

        /// Parses a PPS RBSP (after EPB removal).
        pub fn parse(rbsp: &[u8]) -> Result<Self, DecodeError> {
            let mut br = BitReader::new(rbsp);
            let id = br.read_ue()?;
            let sps_id = br.read_ue()?;
            if id > 255 || sps_id > 31 {
                return Err(BitReader::corrupt("pps/sps id out of range"));
            }
            let entropy_coding_mode_flag = br.read_bit()? != 0;
            let pic_order_present_flag = br.read_bit()? != 0;
            let num_slice_groups_minus1 = br.read_ue()?;
            if num_slice_groups_minus1 > 0 {
                return Err(BitReader::corrupt(
                    "FMO (num_slice_groups_minus1 > 0) unsupported",
                ));
            }
            let num_ref_idx_l0_default_active_minus1 = br.read_ue()?;
            let num_ref_idx_l1_default_active_minus1 = br.read_ue()?;
            let weighted_pred_flag = br.read_bit()? != 0;
            let weighted_bipred_idc = br.read_bits(2)?;
            let pic_init_qp_minus26 = br.read_se()? as i8;
            let pic_init_qs_minus26 = br.read_se()? as i8;
            let chroma_qp_index_offset = br.read_se()? as i8;
            let deblocking_filter_control_present_flag = br.read_bit()? != 0;
            let constrained_intra_pred_flag = br.read_bit()? != 0;
            let redundant_pic_cnt_present_flag = br.read_bit()? != 0;

            let mut transform_8x8_mode_flag = false;
            let mut second_chroma_qp_index_offset = chroma_qp_index_offset;
            if br.more_rbsp_data() {
                transform_8x8_mode_flag = br.read_bit()? != 0;
                if br.read_bit()? != 0 {
                    // pic_scaling_matrix_present_flag — consume and discard.
                    let count = if transform_8x8_mode_flag { 12 } else { 8 };
                    // 6 base + up to 6 extra when transform_8x8: per spec it
                    // is 6 + (transform_8x8 ? 2..6 : 0) lists; we use the
                    // conservative 8/12 loop and tolerate short RBSPs.
                    for i in 0..count {
                        if br.read_bit()? != 0 {
                            br.skip_scaling_list(if i < 6 { 16 } else { 64 })?;
                        }
                    }
                }
                second_chroma_qp_index_offset = br.read_se()? as i8;
            }

            Ok(Self {
                id: id as u8,
                sps_id: sps_id as u8,
                entropy_coding_mode_flag,
                pic_order_present_flag,
                num_ref_idx_l0_default_active_minus1,
                num_ref_idx_l1_default_active_minus1,
                weighted_pred_flag,
                weighted_bipred_idc,
                pic_init_qp_minus26,
                pic_init_qs_minus26,
                chroma_qp_index_offset,
                second_chroma_qp_index_offset,
                deblocking_filter_control_present_flag,
                constrained_intra_pred_flag,
                redundant_pic_cnt_present_flag,
                transform_8x8_mode_flag,
            })
        }
    }

    /// An invalid `ReferenceFrames`/`RefPicList` slot.
    pub fn invalid_ref_pic() -> va::PictureH264 {
        va::PictureH264::new(va::VA_INVALID_SURFACE, 0, va::VA_PICTURE_H264_INVALID, 0, 0)
    }

    /// A coded-slice NAL: `(nal_bytes_with_header, nal_ref_idc, is_idr)`.
    pub type ParsedSlice<'a> = (&'a [u8], u8, bool);

    /// Parsed `slice_header` fields needed by `VASliceParameterBufferH264`.
    pub struct SliceHeader {
        pub first_mb_in_slice: u32,
        pub slice_type: u32,
        pub pps_id: u32,
        pub sps_id: u32,
        pub frame_num: u32,
        pub field_pic_flag: bool,
        pub bottom_field_flag: bool,
        pub nal_ref_idc: u8,
        pub idr: bool,
        pub pic_order_cnt_lsb: u32,
        pub delta_pic_order_cnt_bottom: i32,
        pub delta_pic_order_cnt0: i32,
        pub delta_pic_order_cnt1: i32,
        pub direct_spatial_mv_pred_flag: bool,
        pub num_ref_idx_l0_active_minus1: u32,
        pub num_ref_idx_l1_active_minus1: u32,
        pub cabac_init_idc: u32,
        pub slice_qp_delta: i32,
        pub disable_deblocking_filter_idc: u32,
        pub slice_alpha_c0_offset_div2: i32,
        pub slice_beta_offset_div2: i32,
        /// Bits consumed by the header in the EPB-stripped RBSP (i.e. after
        /// the NAL header byte).
        pub header_bits: usize,
    }

    /// The canonical H.264 slice classes (slice_type mod 5).
    const ST_P: u32 = 0;
    const ST_B: u32 = 1;
    const ST_I: u32 = 2;
    const ST_SP: u32 = 3;
    const ST_SI: u32 = 4;

    impl SliceHeader {
        /// Parses a slice header from the EPB-stripped RBSP of a coded-slice
        /// NAL (i.e. `nal[1..]` with emulation prevention removed).
        pub fn parse(
            rbsp: &[u8],
            nal_ref_idc: u8,
            idr: bool,
            state: &State,
        ) -> Result<Self, DecodeError> {
            let mut br = BitReader::new(rbsp);
            let first_mb_in_slice = br.read_ue()?;
            let slice_type_raw = br.read_ue()?;
            if slice_type_raw > 9 {
                return Err(BitReader::corrupt("slice_type out of range"));
            }
            let slice_type = slice_type_raw % 5;
            if slice_type == ST_SP || slice_type == ST_SI {
                return Err(BitReader::corrupt("SP/SI slices unsupported"));
            }
            let pps_id = br.read_ue()?;
            let pps = state
                .pps
                .get(&(pps_id as u8))
                .ok_or_else(|| BitReader::corrupt("slice references unknown PPS"))?;
            let sps = state
                .sps
                .get(&pps.sps_id)
                .ok_or_else(|| BitReader::corrupt("slice references unknown SPS"))?;

            if sps.separate_colour_plane_flag {
                let _colour_plane_id = br.read_bits(2)?;
            }
            let frame_num = br.read_bits(sps.log2_max_frame_num_minus4 + 4)?;

            let mut field_pic_flag = false;
            let mut bottom_field_flag = false;
            if !sps.frame_mbs_only_flag {
                field_pic_flag = br.read_bit()? != 0;
                if field_pic_flag {
                    bottom_field_flag = br.read_bit()? != 0;
                }
            }
            if idr {
                let _idr_pic_id = br.read_ue()?;
            }
            let mut pic_order_cnt_lsb = 0;
            let mut delta_pic_order_cnt_bottom = 0;
            let mut delta_pic_order_cnt0 = 0;
            let mut delta_pic_order_cnt1 = 0;
            if sps.pic_order_cnt_type == 0 {
                pic_order_cnt_lsb = br.read_bits(sps.log2_max_pic_order_cnt_lsb_minus4 + 4)?;
                if pps.pic_order_present_flag && !field_pic_flag {
                    delta_pic_order_cnt_bottom = br.read_se()?;
                }
            } else if sps.pic_order_cnt_type == 1 && !sps.delta_pic_order_always_zero_flag {
                delta_pic_order_cnt0 = br.read_se()?;
                if pps.pic_order_present_flag && !field_pic_flag {
                    delta_pic_order_cnt1 = br.read_se()?;
                }
            }
            if pps.redundant_pic_cnt_present_flag {
                let _redundant_pic_cnt = br.read_ue()?;
            }

            let mut direct_spatial_mv_pred_flag = false;
            if slice_type == ST_B {
                direct_spatial_mv_pred_flag = br.read_bit()? != 0;
            }

            let mut num_ref_idx_l0_active_minus1 = pps.num_ref_idx_l0_default_active_minus1;
            let mut num_ref_idx_l1_active_minus1 = pps.num_ref_idx_l1_default_active_minus1;
            if slice_type == ST_P || slice_type == ST_B {
                if br.read_bit()? != 0 {
                    // num_ref_idx_active_override_flag
                    num_ref_idx_l0_active_minus1 = br.read_ue()?;
                    if slice_type == ST_B {
                        num_ref_idx_l1_active_minus1 = br.read_ue()?;
                    }
                }
                // ref_pic_list_modification — parsed and discarded.
                if br.read_bit()? != 0 {
                    loop {
                        let idc = br.read_ue()?;
                        if idc == 3 {
                            break;
                        }
                        let _ = br.read_ue()?;
                    }
                }
                if slice_type == ST_B && br.read_bit()? != 0 {
                    loop {
                        let idc = br.read_ue()?;
                        if idc == 3 {
                            break;
                        }
                        let _ = br.read_ue()?;
                    }
                }
            }

            // pred_weight_table — consumed (values discarded; VAAPI receives
            // zeroed weights — documented simplification).
            if (pps.weighted_pred_flag && (slice_type == ST_P || slice_type == ST_SP))
                || (pps.weighted_bipred_idc == 1 && slice_type == ST_B)
            {
                let _luma_log2_weight_denom = br.read_ue()?;
                if sps.chroma_format_idc != 0 {
                    let _chroma_log2_weight_denom = br.read_ue()?;
                }
                let chroma = sps.chroma_format_idc != 0;
                for _ in 0..=num_ref_idx_l0_active_minus1.min(31) {
                    if br.read_bit()? != 0 {
                        let _ = br.read_se()?;
                        let _ = br.read_se()?;
                    }
                    if chroma && br.read_bit()? != 0 {
                        for _ in 0..4 {
                            let _ = br.read_se()?;
                        }
                    }
                }
                if slice_type == ST_B {
                    for _ in 0..=num_ref_idx_l1_active_minus1.min(31) {
                        if br.read_bit()? != 0 {
                            let _ = br.read_se()?;
                            let _ = br.read_se()?;
                        }
                        if chroma && br.read_bit()? != 0 {
                            for _ in 0..4 {
                                let _ = br.read_se()?;
                            }
                        }
                    }
                }
            }

            if nal_ref_idc != 0 {
                // dec_ref_pic_marking — parsed and discarded (documented
                // simplification; only IDR flags are consumed for position).
                if idr {
                    let _no_output = br.read_bit()?;
                    let _long_term_reference = br.read_bit()?;
                } else if br.read_bit()? != 0 {
                    // adaptive_ref_pic_marking_mode_flag
                    loop {
                        let mmco = br.read_ue()?;
                        if mmco == 0 {
                            break;
                        }
                        match mmco {
                            1 | 3 => {
                                let _ = br.read_ue()?;
                            }
                            2 | 4 | 6 => {
                                let _ = br.read_ue()?;
                            }
                            _ => {}
                        }
                        if mmco == 3 {
                            let _ = br.read_ue()?;
                        }
                    }
                }
            }

            let mut cabac_init_idc = 0;
            if pps.entropy_coding_mode_flag && slice_type != ST_I {
                cabac_init_idc = br.read_ue()?;
            }
            let slice_qp_delta = br.read_se()?;

            let mut disable_deblocking_filter_idc = 0;
            let mut slice_alpha_c0_offset_div2 = 0;
            let mut slice_beta_offset_div2 = 0;
            if pps.deblocking_filter_control_present_flag {
                disable_deblocking_filter_idc = br.read_ue()?;
                if disable_deblocking_filter_idc != 1 {
                    slice_alpha_c0_offset_div2 = br.read_se()?;
                    slice_beta_offset_div2 = br.read_se()?;
                }
            }

            Ok(Self {
                first_mb_in_slice,
                slice_type,
                pps_id,
                sps_id: pps.sps_id as u32,
                frame_num,
                field_pic_flag,
                bottom_field_flag,
                nal_ref_idc,
                idr,
                pic_order_cnt_lsb,
                delta_pic_order_cnt_bottom,
                delta_pic_order_cnt0,
                delta_pic_order_cnt1,
                direct_spatial_mv_pred_flag,
                num_ref_idx_l0_active_minus1,
                num_ref_idx_l1_active_minus1,
                cabac_init_idc,
                slice_qp_delta,
                disable_deblocking_filter_idc,
                slice_alpha_c0_offset_div2,
                slice_beta_offset_div2,
                header_bits: br.bit_pos,
            })
        }

        /// Approximate picture order count (wraparound tracking omitted —
        /// documented simplification).
        pub fn poc(&self, sps: &Sps) -> i32 {
            match sps.pic_order_cnt_type {
                0 => self.pic_order_cnt_lsb as i32,
                1 => 2 * self.frame_num as i32 + self.delta_pic_order_cnt0,
                _ => 2 * self.frame_num as i32,
            }
        }

        /// Builds a `RefPicList` array from the DPB: newest-first for L0,
        /// oldest-first for L1 (documented approximation). I slices get all
        /// invalid entries.
        fn ref_list(&self, dpb: &VecDeque<DpbEntry>, l0: bool) -> [va::PictureH264; 32] {
            let mut list: [va::PictureH264; 32] = std::array::from_fn(|_| invalid_ref_pic());
            if self.slice_type == ST_I {
                return list;
            }
            let iter: Box<dyn Iterator<Item = &DpbEntry> + '_> = if l0 {
                Box::new(dpb.iter().rev())
            } else {
                Box::new(dpb.iter())
            };
            for (i, e) in iter.take(32).enumerate() {
                list[i] = va::PictureH264::new(
                    e.surface_id,
                    e.frame_num,
                    va::VA_PICTURE_H264_SHORT_TERM_REFERENCE,
                    e.poc,
                    e.poc,
                );
            }
            list
        }

        /// Appends this slice's `VASliceParameterBufferH264` record to
        /// `out`.
        ///
        /// `slice_data_offset`/`slice_data_size` describe where this NAL
        /// (header byte + EPB-stripped RBSP) sits in the slice-data buffer.
        /// `slice_data_bit_offset` points past the NAL header byte + slice
        /// header to the first `slice_data()` bit — matching the FFmpeg
        /// `vaapi_h264` convention of `get_bits_count() + 8`.
        pub fn fill_slice_parameter(
            &self,
            out: &mut va::SliceParameterBufferH264,
            slice_data_offset: u32,
            slice_data_size: u32,
            dpb: &VecDeque<DpbEntry>,
        ) {
            out.add_slice_parameter(
                slice_data_size,
                slice_data_offset,
                va::VA_SLICE_DATA_FLAG_ALL,
                (8 + self.header_bits).min(u16::MAX as usize) as u16,
                self.first_mb_in_slice.min(u16::MAX as u32) as u16,
                self.slice_type as u8,
                self.direct_spatial_mv_pred_flag as u8,
                self.num_ref_idx_l0_active_minus1.min(31) as u8,
                self.num_ref_idx_l1_active_minus1.min(31) as u8,
                self.cabac_init_idc as u8,
                self.slice_qp_delta.clamp(i8::MIN as i32, i8::MAX as i32) as i8,
                self.disable_deblocking_filter_idc as u8,
                self.slice_alpha_c0_offset_div2
                    .clamp(i8::MIN as i32, i8::MAX as i32) as i8,
                self.slice_beta_offset_div2
                    .clamp(i8::MIN as i32, i8::MAX as i32) as i8,
                self.ref_list(dpb, true),
                self.ref_list(dpb, false),
                0,
                0, // luma/chroma_log2_weight_denom
                0,
                [0; 32],
                [0; 32], // luma_weight_l0
                0,
                [[0; 2]; 32],
                [[0; 2]; 32], // chroma_weight_l0
                0,
                [0; 32],
                [0; 32], // luma_weight_l1
                0,
                [[0; 2]; 32],
                [[0; 2]; 32], // chroma_weight_l1
            );
        }
    }
}
