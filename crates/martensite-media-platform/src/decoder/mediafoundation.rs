//! Windows Media Foundation decoder backend (`IMFTransform` + D3D11/DXGI).
//!
//! This backend drives a synchronous video-decoder MFT discovered through
//! [`MFTEnumEx`] (category `MFT_CATEGORY_VIDEO_DECODER`), or instantiated
//! directly from the well-known in-box CLSIDs for H.264/HEVC. Decoded frames
//! are handed out as [`HardwareHandle::DxgiSharedHandle`] when a D3D11 device
//! and an [`IMFDXGIDeviceManager`] could be negotiated: the decoder's output
//! texture is copied into an `ID3D11Texture2D` created with
//! `D3D11_RESOURCE_MISC_SHARED_NTHANDLE | D3D11_RESOURCE_MISC_SHARED_KEYEDMUTEX`
//! and exported through [`IDXGIResource1::CreateSharedHandle`], producing an
//! NT handle that `import_dxgi_texture` opens on the D3D12/wgpu side.
//!
//! When the MFT cannot produce D3D11 surfaces (software MFT, missing driver
//! support, or a failed share-copy) frames fall back to
//! [`HardwareHandle::CpuMemory`], copied out of the locked `IMF2DBuffer`/
//! `IMFMediaBuffer` as NV12/P010 planes.
//!
//! ## Lifetime rule for exported handles
//!
//! Each exported shared texture is kept alive inside the decoder
//! ([`MfContext::shared`]) so the NT handle stays valid for the decoder's
//! entire lifetime. Callers must not use a `DxgiSharedHandle` after the
//! decoder is dropped.
//!
//! # Safety
//!
//! All `unsafe` blocks call Media Foundation / D3D11 / COM entry points that
//! are documented in the Windows SDK:
//! - `IMFTransform::ProcessInput`/`ProcessOutput`/`ProcessMessage`:
//!   <https://learn.microsoft.com/en-us/windows/win32/api/mftransform/nf-mftransform-imftransform-processoutput>
//! - `IMFDXGIDeviceManager::ResetDevice` (called through the raw vtable; see
//!   the note in [`create_d3d11`]):
//!   <https://learn.microsoft.com/en-us/windows/win32/api/mfobjects/nf-mfobjects-imfdxgidevicemanager-resetdevice>
//! - `IDXGIResource1::CreateSharedHandle`:
//!   <https://learn.microsoft.com/en-us/windows/win32/api/dxgi1_2/nf-dxgi1_2-idxgiresource1-createsharedhandle>

use std::collections::VecDeque;
use std::ffi::c_void;
use std::mem::ManuallyDrop;
use std::time::Instant;

use windows::core::{Interface, PCWSTR};
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP};
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
    D3D11_RESOURCE_MISC_SHARED_KEYEDMUTEX, D3D11_RESOURCE_MISC_SHARED_NTHANDLE, D3D11_SDK_VERSION,
    D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_SAMPLE_DESC;
use windows::Win32::Graphics::Dxgi::{
    IDXGIResource1, DXGI_SHARED_RESOURCE_READ, DXGI_SHARED_RESOURCE_WRITE,
};
use windows::Win32::Media::MediaFoundation::{
    CLSID_MSH264DecoderMFT, CLSID_MSH265DecoderMFT, IMF2DBuffer, IMFActivate, IMFDXGIBuffer,
    IMFDXGIDeviceManager, IMFMediaBuffer, IMFMediaType, IMFSample, IMFTransform,
    MFCreateDXGIDeviceManager, MFCreateMediaType, MFCreateMemoryBuffer, MFCreateSample,
    MFMediaType_Video, MFNominalRange_0_255, MFSampleExtension_CleanPoint, MFShutdown, MFStartup,
    MFTEnumEx, MFVideoFormat_AV1, MFVideoFormat_H264, MFVideoFormat_HEVC, MFVideoFormat_NV12,
    MFVideoFormat_P010, MFVideoFormat_VP90, MFVideoInterlace_MixedInterlaceOrProgressive,
    MFVideoInterlace_Progressive, MFVideoPrimaries_BT2020, MFVideoPrimaries_BT470_2_SysBG,
    MFVideoPrimaries_BT470_2_SysM, MFVideoPrimaries_BT709, MFVideoPrimaries_DCI_P3,
    MFVideoPrimaries_SMPTE170M, MFVideoPrimaries_SMPTE240M, MFVideoPrimaries_XYZ,
    MFVideoTransFunc_2020, MFVideoTransFunc_2020_const, MFVideoTransFunc_2084,
    MFVideoTransFunc_HLG, MFVideoTransFunc_sRGB, MFSTARTUP_NOSOCKET, MFT_CATEGORY_VIDEO_DECODER,
    MFT_ENUM_FLAG, MFT_ENUM_FLAG_HARDWARE, MFT_ENUM_FLAG_LOCALMFT, MFT_ENUM_FLAG_SORTANDFILTER,
    MFT_ENUM_FLAG_SYNCMFT, MFT_MESSAGE_COMMAND_DRAIN, MFT_MESSAGE_COMMAND_FLUSH,
    MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, MFT_MESSAGE_NOTIFY_END_STREAMING,
    MFT_MESSAGE_SET_D3D_MANAGER, MFT_OUTPUT_DATA_BUFFER, MFT_OUTPUT_STREAM_PROVIDES_SAMPLES,
    MFT_REGISTER_TYPE_INFO, MF_E_NOTACCEPTING, MF_E_TRANSFORM_NEED_MORE_INPUT,
    MF_E_TRANSFORM_STREAM_CHANGE, MF_MT_FRAME_RATE, MF_MT_FRAME_SIZE, MF_MT_INTERLACE_MODE,
    MF_MT_MAJOR_TYPE, MF_MT_MAX_FRAME_AVERAGE_LUMINANCE_LEVEL, MF_MT_MAX_LUMINANCE_LEVEL,
    MF_MT_MAX_MASTERING_LUMINANCE, MF_MT_MIN_MASTERING_LUMINANCE, MF_MT_SUBTYPE,
    MF_MT_TRANSFER_FUNCTION, MF_MT_USER_DATA, MF_MT_VIDEO_NOMINAL_RANGE, MF_MT_VIDEO_PRIMARIES,
    MF_TRANSFORM_ASYNC, MF_VERSION,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_INPROC_SERVER,
    COINIT_MULTITHREADED,
};

use crate::decoder::{
    DecodeError, DecodeStats, DecodedFrame, DecoderBackend, DecoderConfig, EncodedPacket,
    HdrSideData, VideoCodec,
};
use crate::surface::{
    ColorRange, HardwareHandle, MediaError, VideoFrameMetadata, VideoPixelFormat,
};

/// Stream ID used for the single input and single output of a decoder MFT.
const STREAM_ID: u32 = 0;

/// `MF_MT_FRAME_SIZE`/`MF_MT_FRAME_RATE` pack two `u32`s into one `u64`
/// attribute (`MFSetAttributeSize`/`MFSetAttributeRatio` are inline helpers
/// not exported by `mfplat.dll`, so pack by hand: high `u32` = width or
/// numerator).
fn pack_u32_pair(high: u32, low: u32) -> u64 {
    (u64::from(high) << 32) | u64::from(low)
}

/// Unpacks the high/low `u32` pair of a packed attribute.
fn unpack_u32_pair(packed: u64) -> (u32, u32) {
    ((packed >> 32) as u32, packed as u32)
}

/// Maps a `VideoCodec` to the MF input-subtype GUID used for both
/// `MFTEnumEx` filtering and `SetInputType`.
fn codec_subtype(codec: VideoCodec) -> windows::core::GUID {
    match codec {
        VideoCodec::H264 => MFVideoFormat_H264,
        VideoCodec::Hevc => MFVideoFormat_HEVC,
        VideoCodec::Av1 => MFVideoFormat_AV1,
        VideoCodec::Vp9 => MFVideoFormat_VP90,
    }
}

/// Well-known in-box decoder CLSID per codec, tried before the generic
/// `MFTEnumEx` enumeration. `None` for codecs without a stable CLSID (the
/// AV1/VP9 decoders ship via Store-delivered extensions without public
/// in-box CLSIDs).
fn codec_clsid(codec: VideoCodec) -> Option<windows::core::GUID> {
    match codec {
        VideoCodec::H264 => Some(CLSID_MSH264DecoderMFT),
        VideoCodec::Hevc => Some(CLSID_MSH265DecoderMFT),
        VideoCodec::Av1 | VideoCodec::Vp9 => None,
    }
}

/// Converts a `windows::core::Error` into a fatal decode error carrying the
/// failing call site and HRESULT text.
fn mf_fatal(context: &str, err: &windows::core::Error) -> MediaError {
    DecodeError::Fatal(format!("{context} failed: {err}")).into()
}

/// Maps an MFT error to a decode error, downgrading the "not accepting"
/// HRESULT to a plain fatal for callers that already retried.
fn map_mft_error(context: &str, err: &windows::core::Error) -> MediaError {
    DecodeError::Fatal(format!("{context} failed: {err}")).into()
}

/// An exported shared D3D11 texture kept alive inside the decoder so the
/// `DxgiSharedHandle` handed to the caller remains openable until drop.
struct SharedExport {
    /// The NT handle returned by `IDXGIResource1::CreateSharedHandle` (stored
    /// for debugging; the OS handle itself is owned by the caller's side).
    _handle: usize,
    /// The texture backing `_handle`.
    _texture: ID3D11Texture2D,
}

/// D3D11 device stack bound to the decoder MFT.
struct D3d11 {
    /// The D3D11 device also registered with the DXGI device manager.
    device: ID3D11Device,
    /// Immediate context used for the share-copy.
    context: ID3D11DeviceContext,
    /// DXGI device manager handed to the MFT via
    /// `MFT_MESSAGE_SET_D3D_MANAGER`.
    manager: IMFDXGIDeviceManager,
}

/// COM objects owned by the decoder. Kept in a dedicated struct inside an
/// `Option` so [`MediaFoundationDecoder::drop`] can release every MF/D3D
/// object before calling `MFShutdown`.
struct MfContext {
    /// The decoder MFT. Declared first so it is released before the device
    /// objects it may reference.
    transform: IMFTransform,
    /// Exported shared textures backing live `DxgiSharedHandle`s.
    shared: Vec<SharedExport>,
    /// D3D11 stack, present when a device could be created at all.
    d3d: Option<D3d11>,
    /// Whether the MFT allocates its own output samples
    /// (`MFT_OUTPUT_STREAM_PROVIDES_SAMPLES`).
    provides_samples: bool,
    /// `MFT_OUTPUT_STREAM_INFO::cbSize` used when this decoder must allocate
    /// output samples itself.
    output_cb_size: u32,
}

/// Result of successfully configuring a candidate MFT.
struct MftSetup {
    /// Negotiated output pixel format.
    format: VideoPixelFormat,
    /// Whether the MFT accepted the DXGI device manager.
    d3d_enabled: bool,
    /// Width reported by the output type (0 when unsignalled).
    width: u32,
    /// Height reported by the output type (0 when unsignalled).
    height: u32,
}

/// Windows Media Foundation hardware/software video decoder.
///
/// `MediaFoundationDecoder` is the Windows backend behind
/// `martensite-media`'s `VideoDecoder` impl. Packets are pushed with
/// [`send_packet`](Self::send_packet); reordered output frames are drained
/// with [`try_recv_frame`](Self::try_recv_frame), which returns `Ok(None)`
/// while the MFT still needs more input.
///
/// # Examples
///
/// ```no_run
/// use martensite_media_platform::decoder::{
///     mediafoundation::MediaFoundationDecoder, DecoderConfig, VideoCodec,
/// };
///
/// let dec = MediaFoundationDecoder::create(&DecoderConfig::new(VideoCodec::H264, 1920, 1080));
/// assert!(dec.is_ok());
/// ```
pub struct MediaFoundationDecoder {
    /// Live COM context; `Some` until `drop` releases it ahead of
    /// `MFShutdown`.
    ctx: Option<MfContext>,
    /// Frames decoded ahead of the caller's consumption (filled when the
    /// MFT's input queue was full and `send_packet` had to drain first).
    out: VecDeque<DecodedFrame>,
    /// Keyframe gate: `false` until the first IDR packet is accepted.
    seen_keyframe: bool,
    /// Pixel format currently emitted (`Nv12` or `P010`).
    negotiated: VideoPixelFormat,
    /// Current coded/display width in pixels (from `MF_MT_FRAME_SIZE` or
    /// `config`).
    width: u32,
    /// Current coded/display height in pixels.
    height: u32,
    /// Whether this thread's COM apartment was initialized by `create` and
    /// must be balanced with `CoUninitialize` on drop.
    did_coinit: bool,
    /// Rolling telemetry counters.
    stats: DecodeStats,
    /// Running index stamped on each emitted frame (starts at 1).
    frame_index: u64,
}

// SAFETY: every COM object inside `MfContext` is created on a
// `COINIT_MULTITHREADED` apartment and is exclusively owned by this decoder;
// all calls into them go through `&mut self` methods. The MF interface
// bindings in `windows` 0.61 are not marked `Send`, which is why this manual
// impl is required.
unsafe impl Send for MediaFoundationDecoder {}

// SAFETY: `&self` methods (`negotiated_format`/`hdr_side_data`/`stats`) read
// only owned fields and perform read-only `GetUINT32` attribute lookups on
// the output media type — no shared mutable FFI state is touched, so
// `&self` from multiple threads cannot race.
unsafe impl Sync for MediaFoundationDecoder {}

impl MediaFoundationDecoder {
    /// Creates a Media Foundation decoder for `config.codec`.
    ///
    /// Negotiation order: the well-known in-box CLSID (H.264/HEVC) first,
    /// then `MFTEnumEx` over hardware sync MFTs, then — when
    /// `config.allow_software` — any sync MFT. When `allow_software` is
    /// false an MFT that refuses the DXGI device manager is rejected.
    ///
    /// `config.codec_config` (e.g. an `avcC`/`hvcC` record) is attached to
    /// the input media type as an `MF_MT_USER_DATA` blob.
    ///
    /// # Errors
    ///
    /// - [`DecodeError::UnsupportedCodec`] when no MFT accepts the codec's
    ///   input type with NV12/P010 output.
    /// - [`DecodeError::Fatal`] when `MFStartup`, `D3D11CreateDevice`, or
    ///   the DXGI device manager setup fails.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_media_platform::decoder::{
    ///     mediafoundation::MediaFoundationDecoder, DecoderConfig, VideoCodec,
    /// };
    ///
    /// let dec = MediaFoundationDecoder::create(&DecoderConfig::new(VideoCodec::Hevc, 640, 480));
    /// assert!(dec.is_ok() || dec.is_err()); // depends on installed MFTs
    /// ```
    pub fn create(config: &DecoderConfig) -> Result<Self, MediaError> {
        // SAFETY: `CoInitializeEx` with `COINIT_MULTITHREADED` matches the
        // free-threaded usage of the MF objects below. S_OK and S_FALSE both
        // report success (is_ok) and must be balanced by `CoUninitialize`;
        // RPC_E_CHANGED_MODE is an error and means we did not initialize.
        let coinit_hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if coinit_hr.is_err() {
            return Err(DecodeError::Fatal(format!("CoInitializeEx failed: {coinit_hr}")).into());
        }
        let did_coinit = coinit_hr.is_ok();

        // SAFETY: `MFStartup` initializes Media Foundation for the process;
        // `MF_VERSION`/`MFSTARTUP_NOSOCKET` are the documented arguments.
        if let Err(e) = unsafe { MFStartup(MF_VERSION, MFSTARTUP_NOSOCKET) } {
            if did_coinit {
                // SAFETY: balances the successful CoInitializeEx above.
                unsafe { CoUninitialize() };
            }
            return Err(mf_fatal("MFStartup", &e));
        }

        // Rollback helper: releases COM/MF on any early exit below.
        let cleanup = |did_coinit: bool| {
            // SAFETY: balances `MFStartup`; at every call site below all MF
            // objects created so far have already been dropped.
            unsafe {
                let _ = MFShutdown();
                if did_coinit {
                    CoUninitialize();
                }
            }
        };

        let input_type = match build_input_type(config) {
            Ok(t) => t,
            Err(e) => {
                cleanup(did_coinit);
                return Err(e);
            }
        };

        // D3D11 device + DXGI device manager. Optional when software decode
        // is allowed: a software MFT produces CPU samples, and a headless
        // box may have no D3D11 device at all.
        let d3d = match create_d3d11() {
            Ok(d3d) => Some(d3d),
            Err(e) => {
                if config.allow_software {
                    None
                } else {
                    cleanup(did_coinit);
                    return Err(e);
                }
            }
        };

        let mut last_error = DecodeError::UnsupportedCodec(format!(
            "no Media Foundation MFT accepts {:?} with NV12/P010 output",
            config.codec
        ));

        // Try the well-known in-box CLSID first (cheapest path).
        if let Some(clsid) = codec_clsid(config.codec) {
            // SAFETY: `CoCreateInstance` with `CLSCTX_INPROC_SERVER` is the
            // documented way to instantiate an in-box decoder MFT.
            match unsafe { CoCreateInstance::<_, IMFTransform>(&clsid, None, CLSCTX_INPROC_SERVER) }
            {
                Ok(transform) => {
                    match configure_mft(&transform, config, &input_type, d3d.as_ref()) {
                        Ok(setup) => {
                            return Self::finish_create(config, transform, d3d, did_coinit, setup)
                        }
                        Err(e) => last_error = e,
                    }
                }
                Err(e) => {
                    last_error =
                        DecodeError::Fatal(format!("CoCreateInstance {clsid:?} failed: {e}"));
                }
            }
        }

        // Then enumerate: hardware sync MFTs first, all sync MFTs (incl.
        // software) as the `allow_software` fallback.
        let mut flag_sets =
            vec![MFT_ENUM_FLAG_HARDWARE | MFT_ENUM_FLAG_SYNCMFT | MFT_ENUM_FLAG_SORTANDFILTER];
        if config.allow_software {
            flag_sets
                .push(MFT_ENUM_FLAG_SYNCMFT | MFT_ENUM_FLAG_LOCALMFT | MFT_ENUM_FLAG_SORTANDFILTER);
        }

        for flags in flag_sets {
            for transform in enumerate_mfts(config.codec, flags) {
                match configure_mft(&transform, config, &input_type, d3d.as_ref()) {
                    Ok(setup) => {
                        return Self::finish_create(config, transform, d3d, did_coinit, setup);
                    }
                    Err(e) => last_error = e,
                }
            }
        }

        cleanup(did_coinit);
        Err(last_error.into())
    }

    /// Shared tail of `create`: wraps the configured transform into the
    /// decoder value.
    fn finish_create(
        config: &DecoderConfig,
        transform: IMFTransform,
        d3d: Option<D3d11>,
        did_coinit: bool,
        setup: MftSetup,
    ) -> Result<Self, MediaError> {
        // SAFETY: begin-streaming is advisory; failure only means the MFT
        // ignores streaming notifications.
        let _ = unsafe { transform.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0) };

        // SAFETY: stream 0 exists on a configured decoder MFT.
        let stream_info = unsafe { transform.GetOutputStreamInfo(STREAM_ID) }.unwrap_or_default();
        let provides_samples =
            stream_info.dwFlags & (MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32) != 0;

        let hardware = d3d.is_some() && setup.d3d_enabled;
        let mut stats = DecodeStats::default();
        stats.backend = Some(if hardware {
            DecoderBackend::MediaFoundation
        } else {
            DecoderBackend::Software
        });

        Ok(Self {
            ctx: Some(MfContext {
                transform,
                shared: Vec::new(),
                d3d,
                provides_samples,
                output_cb_size: stream_info.cbSize,
            }),
            out: VecDeque::new(),
            seen_keyframe: false,
            negotiated: setup.format,
            width: if setup.width != 0 {
                setup.width
            } else {
                config.width
            },
            height: if setup.height != 0 {
                setup.height
            } else {
                config.height
            },
            did_coinit,
            stats,
            frame_index: 0,
        })
    }

    /// Feeds one compressed access unit to the decoder.
    ///
    /// The first packet after construction or [`flush`](Self::flush) must be
    /// a keyframe; delta packets before that are rejected with
    /// [`DecodeError::NeedsKeyframe`]. When the MFT's input queue is full
    /// (`MF_E_NOTACCEPTING`) pending output is drained into an internal
    /// queue and the send retried once.
    ///
    /// # Errors
    ///
    /// - [`DecodeError::StreamCorrupt`] for empty or oversized packets.
    /// - [`DecodeError::NeedsKeyframe`] for a delta packet before the first
    ///   keyframe.
    /// - [`DecodeError::Fatal`] for unrecoverable MFT errors.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_media_platform::decoder::{
    ///     mediafoundation::MediaFoundationDecoder, DecoderConfig, EncodedPacket, VideoCodec,
    /// };
    /// use martensite_media_platform::surface::MediaError;
    ///
    /// let mut dec = MediaFoundationDecoder::create(&DecoderConfig::new(VideoCodec::H264, 64, 64))
    ///     .expect("H.264 MFT present");
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
        if packet.data.len() > u32::MAX as usize {
            self.stats.record_rejection();
            return Err(DecodeError::StreamCorrupt(
                "packet exceeds IMFMediaBuffer's maximum length".to_string(),
            )
            .into());
        }
        if !self.seen_keyframe && !packet.is_keyframe {
            self.stats.record_rejection();
            return Err(DecodeError::NeedsKeyframe.into());
        }

        let sample = build_input_sample(packet)?;

        // SAFETY: `sample` is a live IMFSample; stream 0 was configured in
        // `create`. `ProcessInput` may return `MF_E_NOTACCEPTING` while the
        // MFT's input queue is full.
        let first = unsafe {
            self.context()?
                .transform
                .ProcessInput(STREAM_ID, &sample, 0)
        };
        match first {
            Ok(()) => {
                self.seen_keyframe |= packet.is_keyframe;
                self.stats.record_packet(packet.data.len());
                Ok(())
            }
            Err(ref e) if e.code() == MF_E_NOTACCEPTING => {
                // Input queue full: drain ready output frames into
                // `self.out`, then retry exactly once.
                self.pump_output()?;
                // SAFETY: same as above; this is the single retry.
                let second = unsafe {
                    self.context()?
                        .transform
                        .ProcessInput(STREAM_ID, &sample, 0)
                };
                match second {
                    Ok(()) => {
                        self.seen_keyframe |= packet.is_keyframe;
                        self.stats.record_packet(packet.data.len());
                        Ok(())
                    }
                    Err(e) => {
                        self.stats.record_rejection();
                        Err(map_mft_error("ProcessInput", &e))
                    }
                }
            }
            Err(e) => {
                self.stats.record_rejection();
                Err(map_mft_error("ProcessInput", &e))
            }
        }
    }

    /// Returns the next reordered frame, or `Ok(None)` when the MFT needs
    /// more input before it can produce output.
    ///
    /// Frames first decoded inside a `send_packet` drain are buffered
    /// internally, so callers can interleave `send_packet`/`try_recv_frame`
    /// freely.
    ///
    /// # Errors
    ///
    /// - [`DecodeError::StreamCorrupt`] when a decoded buffer has an
    ///   unreadable plane layout.
    /// - [`DecodeError::Fatal`] for unrecoverable MFT errors.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_media_platform::decoder::{
    ///     mediafoundation::MediaFoundationDecoder, DecoderConfig, VideoCodec,
    /// };
    ///
    /// let mut dec = MediaFoundationDecoder::create(&DecoderConfig::new(VideoCodec::H264, 64, 64))
    ///     .expect("H.264 MFT present");
    /// assert!(dec.try_recv_frame().unwrap().is_none());
    /// ```
    pub fn try_recv_frame(&mut self) -> Result<Option<DecodedFrame>, MediaError> {
        self.pump_output()?;
        Ok(self.out.pop_front())
    }

    /// Discards all internally buffered frames and resets the stream state.
    ///
    /// Sends `MFT_MESSAGE_COMMAND_FLUSH` to the decoder MFT (a reset, not an
    /// end-of-stream drain); the next packet must be a keyframe.
    ///
    /// # Errors
    ///
    /// - [`DecodeError::Fatal`] when the MFT rejects the flush.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_media_platform::decoder::{
    ///     mediafoundation::MediaFoundationDecoder, DecoderConfig, EncodedPacket, VideoCodec,
    /// };
    ///
    /// let mut dec = MediaFoundationDecoder::create(&DecoderConfig::new(VideoCodec::H264, 64, 64))
    ///     .expect("H.264 MFT present");
    /// dec.flush().unwrap();
    /// assert!(dec
    ///     .send_packet(&EncodedPacket::new(vec![0x41], 0, 0).delta())
    ///     .is_err());
    /// ```
    pub fn flush(&mut self) -> Result<(), MediaError> {
        // SAFETY: flush is legal at any point after the media types are set.
        unsafe {
            self.context()?
                .transform
                .ProcessMessage(MFT_MESSAGE_COMMAND_FLUSH, 0)
        }
        .map_err(|e| map_mft_error("MFT_MESSAGE_COMMAND_FLUSH", &e))?;
        self.out.clear();
        self.seen_keyframe = false;
        Ok(())
    }

    /// Signals end-of-stream: `MFT_MESSAGE_COMMAND_DRAIN` makes the decoder
    /// produce every pending output frame without resetting stream state.
    ///
    /// Unlike [`flush`](Self::flush) the keyframe gate is not re-armed and
    /// drained frames remain available through
    /// [`try_recv_frame`](Self::try_recv_frame).
    ///
    /// # Errors
    ///
    /// - [`DecodeError::Fatal`] when the MFT rejects the drain command.
    pub fn end_of_stream(&mut self) -> Result<(), MediaError> {
        let ctx = self.context()?;
        // SAFETY: drain is legal at any point after the media types are set.
        unsafe { ctx.transform.ProcessMessage(MFT_MESSAGE_COMMAND_DRAIN, 0) }
            .map_err(|e| map_mft_error("MFT_MESSAGE_COMMAND_DRAIN", &e))?;
        self.pump_output()
    }

    /// The [`VideoPixelFormat`] negotiated on the MFT's output type (`Nv12`
    /// or `P010`).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_media_platform::decoder::{
    ///     mediafoundation::MediaFoundationDecoder, DecoderConfig, VideoCodec,
    /// };
    /// use martensite_media_platform::surface::VideoPixelFormat;
    ///
    /// let dec = MediaFoundationDecoder::create(&DecoderConfig::new(VideoCodec::H264, 64, 64))
    ///     .expect("H.264 MFT present");
    /// assert_eq!(dec.negotiated_format(), VideoPixelFormat::Nv12);
    /// ```
    #[must_use]
    pub fn negotiated_format(&self) -> VideoPixelFormat {
        self.negotiated
    }

    /// HDR side-data read from the MFT's current output media type.
    ///
    /// Reads `MF_MT_VIDEO_PRIMARIES`, `MF_MT_TRANSFER_FUNCTION`,
    /// `MF_MT_VIDEO_NOMINAL_RANGE` and the mastering / content luminance
    /// attributes (`MF_MT_MAX_LUMINANCE_LEVEL` = MaxCLL,
    /// `MF_MT_MAX_FRAME_AVERAGE_LUMINANCE_LEVEL` = MaxFALL,
    /// `MF_MT_MAX_MASTERING_LUMINANCE`, `MF_MT_MIN_MASTERING_LUMINANCE`).
    /// `MF_MT_VIDEO_MASTERING_DISPLAY` is not present in `windows` 0.61
    /// metadata, so the equivalent individual attributes are read instead.
    /// Returns `None` while no output type is set or when nothing beyond
    /// SDR/BT.709 was signalled.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_media_platform::decoder::{
    ///     mediafoundation::MediaFoundationDecoder, DecoderConfig, VideoCodec,
    /// };
    ///
    /// let dec = MediaFoundationDecoder::create(&DecoderConfig::new(VideoCodec::H264, 64, 64))
    ///     .expect("H.264 MFT present");
    /// assert!(dec.hdr_side_data().is_none());
    /// ```
    #[must_use]
    pub fn hdr_side_data(&self) -> Option<HdrSideData> {
        let ctx = self.ctx.as_ref()?;
        // SAFETY: stream 0 exists; GetOutputCurrentType only reads.
        let mt = unsafe { ctx.transform.GetOutputCurrentType(STREAM_ID) }.ok()?;

        // SAFETY: read-only attribute access on a live media type.
        let (primaries, transfer, nominal_range, max_cll, max_fall, max_mastering, min_mastering) = unsafe {
            (
                mt.GetUINT32(&MF_MT_VIDEO_PRIMARIES).ok(),
                mt.GetUINT32(&MF_MT_TRANSFER_FUNCTION).ok(),
                mt.GetUINT32(&MF_MT_VIDEO_NOMINAL_RANGE).ok(),
                mt.GetUINT32(&MF_MT_MAX_LUMINANCE_LEVEL)
                    .ok()
                    .and_then(|v| u16::try_from(v).ok()),
                mt.GetUINT32(&MF_MT_MAX_FRAME_AVERAGE_LUMINANCE_LEVEL)
                    .ok()
                    .and_then(|v| u16::try_from(v).ok()),
                mt.GetUINT32(&MF_MT_MAX_MASTERING_LUMINANCE)
                    .ok()
                    .map(|v| v as f32),
                // MF_MT_MIN_MASTERING_LUMINANCE is in 0.0001-nit units.
                mt.GetUINT32(&MF_MT_MIN_MASTERING_LUMINANCE)
                    .ok()
                    .map(|v| v as f32 / 10_000.0),
            )
        };

        // Only advertise HDR when something beyond the SDR defaults was
        // actually signalled.
        let hdrish = matches!(primaries, Some(p) if p == MFVideoPrimaries_BT2020.0 as u32)
            || matches!(transfer, Some(t) if t == MFVideoTransFunc_2084.0 as u32
                || t == MFVideoTransFunc_HLG.0 as u32)
            || max_mastering.is_some()
            || max_cll.is_some();
        if !hdrish {
            return None;
        }

        Some(HdrSideData {
            eotf_code: transfer_eotf(transfer),
            primaries_code: primaries_iso_code(primaries),
            full_range: matches!(nominal_range, Some(r) if r == MFNominalRange_0_255.0 as u32),
            max_luminance_nits: max_mastering,
            min_luminance_nits: min_mastering,
            max_cll,
            max_fall,
            dynamic_metadata: None,
        })
    }

    /// Rolling telemetry for this decoder instance.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_media_platform::decoder::{
    ///     mediafoundation::MediaFoundationDecoder, DecoderBackend, DecoderConfig, VideoCodec,
    /// };
    ///
    /// let dec = MediaFoundationDecoder::create(&DecoderConfig::new(VideoCodec::H264, 64, 64))
    ///     .expect("H.264 MFT present");
    /// assert_eq!(dec.stats().frames_decoded, 0);
    /// ```
    #[must_use]
    pub fn stats(&self) -> &DecodeStats {
        &self.stats
    }

    /// Returns the live COM context. `ctx` is `Some` for the decoder's
    /// entire usable lifetime (it is only taken inside `drop`); the error
    /// path exists to keep the `Option` honest without `unwrap`.
    fn context(&self) -> Result<&MfContext, MediaError> {
        self.ctx.as_ref().ok_or_else(|| {
            DecodeError::Fatal("Media Foundation context already shut down".to_string()).into()
        })
    }

    /// Mutable variant of [`context`](Self::context).
    fn context_mut(&mut self) -> Result<&mut MfContext, MediaError> {
        self.ctx.as_mut().ok_or_else(|| {
            DecodeError::Fatal("Media Foundation context already shut down".to_string()).into()
        })
    }

    /// Drains every frame the MFT currently has ready into `self.out`.
    fn pump_output(&mut self) -> Result<(), MediaError> {
        while let Some(frame) = self.process_output_once()? {
            self.out.push_back(frame);
        }
        Ok(())
    }

    /// One `ProcessOutput` round-trip; `Ok(None)` when the MFT wants more
    /// input or the output type changed mid-stream.
    fn process_output_once(&mut self) -> Result<Option<DecodedFrame>, MediaError> {
        let (provides, cb_size) = {
            let ctx = self.context()?;
            (ctx.provides_samples, ctx.output_cb_size)
        };

        // Either hand the MFT an allocated sample or let it provide its own.
        let provided: Option<IMFSample> = if provides {
            None
        } else {
            Some(new_output_sample(cb_size)?)
        };

        let mut out_buf = MFT_OUTPUT_DATA_BUFFER {
            dwStreamID: STREAM_ID,
            pSample: ManuallyDrop::new(provided),
            dwStatus: 0,
            pEvents: ManuallyDrop::new(None),
        };
        let mut status = 0u32;

        let start = Instant::now();
        // SAFETY: `out_buf` is a valid, initialized buffer descriptor for
        // stream 0; the `ManuallyDrop` fields are taken back below before
        // they could be released twice.
        let hr = unsafe {
            self.context()?.transform.ProcessOutput(
                0,
                std::slice::from_mut(&mut out_buf),
                &mut status,
            )
        };

        // SAFETY: both fields were initialized above and are only read back
        // once here; the `pEvents` collection, if any, is released by
        // dropping it.
        let sample = unsafe { ManuallyDrop::take(&mut out_buf.pSample) };
        let events = unsafe { ManuallyDrop::take(&mut out_buf.pEvents) };
        drop(events);

        match hr {
            Err(ref e) if e.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => Ok(None),
            Err(ref e) if e.code() == MF_E_TRANSFORM_STREAM_CHANGE => {
                self.renegotiate_output()?;
                Ok(None)
            }
            Err(e) => Err(map_mft_error("ProcessOutput", &e)),
            Ok(()) => match sample {
                Some(sample) => {
                    let frame = self.map_sample(&sample)?;
                    self.stats.record_frame(
                        u64::try_from(start.elapsed().as_nanos()).unwrap_or(u64::MAX),
                    );
                    Ok(Some(frame))
                }
                // The MFT signalled output but produced no sample; treat as
                // "keep polling" — the loop in `pump_output` re-enters.
                None => Ok(None),
            },
        }
    }

    /// Re-applies the output media type after `MF_E_TRANSFORM_STREAM_CHANGE`
    /// and refreshes negotiated format/dimensions.
    fn renegotiate_output(&mut self) -> Result<(), MediaError> {
        // Scoped so the immutable `self` borrow ends before field writes.
        let (mt, stream_info) = {
            let ctx = self.context()?;
            // SAFETY: stream 0 exists; this is the documented stream-change
            // sequence (GetOutputCurrentType, then SetOutputType with it),
            // plus a read of the new stream info.
            let mt = unsafe { ctx.transform.GetOutputCurrentType(STREAM_ID) }
                .map_err(|e| mf_fatal("GetOutputCurrentType", &e))?;
            unsafe { ctx.transform.SetOutputType(STREAM_ID, &mt, 0) }
                .map_err(|e| mf_fatal("SetOutputType after stream change", &e))?;
            let info = unsafe { ctx.transform.GetOutputStreamInfo(STREAM_ID) }.ok();
            (mt, info)
        };

        // SAFETY: read-only attribute access on a live media type.
        unsafe {
            if let Ok(subtype) = mt.GetGUID(&MF_MT_SUBTYPE) {
                if subtype == MFVideoFormat_NV12 {
                    self.negotiated = VideoPixelFormat::Nv12;
                } else if subtype == MFVideoFormat_P010 {
                    self.negotiated = VideoPixelFormat::P010;
                }
            }
            if let Ok(packed) = mt.GetUINT64(&MF_MT_FRAME_SIZE) {
                let (w, h) = unpack_u32_pair(packed);
                if w != 0 && h != 0 {
                    self.width = w;
                    self.height = h;
                }
            }
        }

        if let Some(info) = stream_info {
            let ctx = self.context_mut()?;
            ctx.provides_samples =
                info.dwFlags & (MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32) != 0;
            ctx.output_cb_size = info.cbSize;
        }
        Ok(())
    }

    /// Turns one output `IMFSample` into a [`DecodedFrame`], exporting a
    /// DXGI shared NT handle when the buffer is a D3D11 surface and falling
    /// back to an NV12/P010 CPU copy otherwise.
    fn map_sample(&mut self, sample: &IMFSample) -> Result<DecodedFrame, MediaError> {
        // SAFETY: `sample` is a live IMFSample; index 0 exists on every MF
        // video output sample.
        let buffer = match unsafe { sample.GetBufferByIndex(0) } {
            Ok(b) => b,
            Err(_) => unsafe { sample.ConvertToContiguousBuffer() }
                .map_err(|e| mf_fatal("ConvertToContiguousBuffer", &e))?,
        };

        let mut metadata = VideoFrameMetadata::try_new(
            self.width,
            self.height,
            self.negotiated,
            ColorRange::Limited,
        )?;
        // SAFETY: read-only timestamp/attribute access on a live sample.
        unsafe {
            metadata.pts_nanos = sample
                .GetSampleTime()
                .ok()
                .and_then(|t| u64::try_from(t).ok())
                .map_or(0, |t| t.saturating_mul(100));
            metadata.duration_nanos = sample
                .GetSampleDuration()
                .ok()
                .and_then(|d| u64::try_from(d).ok())
                .map_or(0, |d| d.saturating_mul(100));
            if let Ok(range) = sample.GetUINT32(&MF_MT_VIDEO_NOMINAL_RANGE) {
                metadata.range = if range == MFNominalRange_0_255.0 as u32 {
                    ColorRange::Full
                } else {
                    ColorRange::Limited
                };
            }
        }
        self.frame_index = self.frame_index.saturating_add(1);
        metadata.frame_index = self.frame_index;

        // SAFETY: QueryInterface cast on a live IMFMediaBuffer.
        let dxgi_buffer = buffer.cast::<IMFDXGIBuffer>().ok();

        let handle = match dxgi_buffer {
            Some(dxgi) => match self.export_dxgi(&dxgi) {
                Ok(h) => h,
                // Shared-texture export failed (no D3D device, or the MFT's
                // surfaces are not copyable): degrade to a CPU copy so the
                // frame is not dropped.
                Err(_) => self.copy_cpu_frame(&buffer)?,
            },
            None => self.copy_cpu_frame(&buffer)?,
        };

        let mut frame = DecodedFrame::new(handle, metadata);
        frame.hdr = self.hdr_side_data();
        Ok(frame)
    }

    /// Copies the decoded D3D11 texture into a shared-NTHANDLE texture and
    /// exports its NT handle.
    fn export_dxgi(&mut self, dxgi: &IMFDXGIBuffer) -> Result<HardwareHandle, MediaError> {
        let ctx = self.context_mut()?;
        let Some(d3d) = ctx.d3d.as_ref() else {
            return Err(MediaError::ImportFailed(
                "decoded DXGI buffer but no D3D11 device".to_string(),
            ));
        };

        let mut raw: *mut c_void = std::ptr::null_mut();
        // SAFETY: `dxgi` is a live IMFDXGIBuffer; `GetResource` writes an
        // AddRef'ed interface pointer for the requested IID on success.
        unsafe { dxgi.GetResource(&ID3D11Texture2D::IID, &mut raw) }.map_err(|e| {
            MediaError::ImportFailed(format!("IMFDXGIBuffer::GetResource failed: {e}"))
        })?;
        if raw.is_null() {
            return Err(MediaError::InvalidHandle);
        }
        // SAFETY: `raw` is a live, owned `ID3D11Texture2D` per the call
        // above.
        let decoded: ID3D11Texture2D = unsafe { Interface::from_raw(raw) };

        // SAFETY: `dxgi` is live; the call only reads the subresource index.
        let subresource = unsafe { dxgi.GetSubresourceIndex() }.unwrap_or(0);

        let mut desc = D3D11_TEXTURE2D_DESC::default();
        // SAFETY: `decoded` is live; `desc` is a valid out pointer.
        unsafe { decoded.GetDesc(&mut desc) };

        let shared_desc = D3D11_TEXTURE2D_DESC {
            Width: desc.Width,
            Height: desc.Height,
            MipLevels: 1,
            ArraySize: 1,
            Format: desc.Format,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: 0,
            CPUAccessFlags: 0,
            // NTHANDLE combined with KEYEDMUTEX is the documented pairing
            // required for NT-handle sharing of D3D11 resources. The D3D12
            // side opens the handle via `OpenSharedHandle` without needing
            // the keyed mutex for read-only sampling.
            MiscFlags: (D3D11_RESOURCE_MISC_SHARED_NTHANDLE | D3D11_RESOURCE_MISC_SHARED_KEYEDMUTEX)
                .0 as u32,
        };

        let mut shared_tex: Option<ID3D11Texture2D> = None;
        // SAFETY: `shared_desc` is fully initialized for a plain 2D texture
        // on this device.
        unsafe {
            d3d.device
                .CreateTexture2D(&shared_desc, None, Some(&mut shared_tex))
        }
        .map_err(|e| MediaError::ImportFailed(format!("CreateTexture2D (shared): {e}")))?;
        let shared_tex = shared_tex.ok_or(MediaError::InvalidHandle)?;

        // SAFETY: both textures live on the same device and share format and
        // extent; `subresource` selects the decoder's array slice into the
        // single slice of the shared texture. `psrcbox = None` copies the
        // whole subresource. `Flush` submits the copy so the handle is valid
        // for cross-API open even if the caller imports immediately.
        unsafe {
            d3d.context
                .CopySubresourceRegion(&shared_tex, 0, 0, 0, 0, &decoded, subresource, None);
            d3d.context.Flush();
        }

        // SAFETY: `shared_tex` is a live ID3D11Texture2D (a DXGI resource).
        let resource1 = shared_tex
            .cast::<IDXGIResource1>()
            .map_err(|e| MediaError::ImportFailed(format!("IDXGIResource1 cast: {e}")))?;

        // SAFETY: `shared_tex` was created with SHARED_NTHANDLE; null
        // security attributes, read|write access, unnamed handle.
        let handle = unsafe {
            resource1.CreateSharedHandle(
                None,
                DXGI_SHARED_RESOURCE_READ.0 | DXGI_SHARED_RESOURCE_WRITE.0,
                PCWSTR::null(),
            )
        }
        .map_err(|e| MediaError::ImportFailed(format!("CreateSharedHandle: {e}")))?;

        let raw_handle = handle.0 as usize;
        if raw_handle == 0 {
            return Err(MediaError::InvalidHandle);
        }

        ctx.shared.push(SharedExport {
            _handle: raw_handle,
            _texture: shared_tex,
        });
        Ok(HardwareHandle::DxgiSharedHandle { handle: raw_handle })
    }

    /// Copies an NV12/P010 `IMFMediaBuffer` into a `CpuMemory` handle.
    fn copy_cpu_frame(&self, buffer: &IMFMediaBuffer) -> Result<HardwareHandle, MediaError> {
        let bytes_per_pixel: u32 = match self.negotiated {
            VideoPixelFormat::P010 => 2,
            _ => 1,
        };
        let width = self.width.max(1);
        let height = self.height.max(1);

        // SAFETY: QueryInterface cast on a live IMFMediaBuffer; when the MFT
        // produced an IMF2DBuffer the plane layout is described by `Lock2D`.
        if let Ok(buf2d) = buffer.cast::<IMF2DBuffer>() {
            let mut scanline: *mut u8 = std::ptr::null_mut();
            let mut pitch: i32 = 0;
            // SAFETY: `buf2d` is live; both out pointers are valid.
            let locked = unsafe { buf2d.Lock2D(&mut scanline, &mut pitch) };
            match locked {
                Err(e) => Err(mf_fatal("IMF2DBuffer::Lock2D", &e)),
                Ok(()) => {
                    let pitch_u = pitch.unsigned_abs();
                    // SAFETY: `Lock2D` guarantees `scanline` is valid for
                    // `pitch` bytes per row over `height` luma rows followed
                    // by `height/2` interleaved-chroma rows (NV12/P010).
                    let result = if scanline.is_null() || pitch_u == 0 {
                        Err(DecodeError::StreamCorrupt(
                            "IMF2DBuffer returned a null scanline".to_string(),
                        )
                        .into())
                    } else {
                        unsafe { copy_biplanar(scanline, pitch_u, width, height, bytes_per_pixel) }
                    };
                    // SAFETY: balances the successful `Lock2D`.
                    unsafe {
                        let _ = buf2d.Unlock2D();
                    };
                    result
                }
            }
        } else {
            // Plain contiguous buffer: assume tightly packed NV12/P010 with
            // stride = width * bytes_per_pixel.
            let mut ptr: *mut u8 = std::ptr::null_mut();
            let mut current_len: u32 = 0;
            // SAFETY: `buffer` is live; all out pointers are valid.
            let locked = unsafe { buffer.Lock(&mut ptr, None, Some(&mut current_len)) };
            match locked {
                Err(e) => Err(mf_fatal("IMFMediaBuffer::Lock", &e)),
                Ok(()) => {
                    let stride = width.saturating_mul(bytes_per_pixel);
                    let y_len = stride as usize * height as usize;
                    let uv_len = stride as usize * (height as usize / 2);
                    let need = y_len + uv_len;
                    // SAFETY: `Lock` guarantees `ptr` valid for
                    // `current_len` bytes; `need <= current_len` is checked.
                    let result = if ptr.is_null() || (current_len as usize) < need {
                        Err(DecodeError::StreamCorrupt(format!(
                            "contiguous buffer too small: {current_len} < {need}"
                        ))
                        .into())
                    } else {
                        unsafe { copy_biplanar(ptr, stride, width, height, bytes_per_pixel) }
                    };
                    // SAFETY: balances the successful `Lock`.
                    unsafe {
                        let _ = buffer.Unlock();
                    };
                    result
                }
            }
        }
    }
}

impl Drop for MediaFoundationDecoder {
    fn drop(&mut self) {
        if let Some(ctx) = self.ctx.take() {
            // SAFETY: the transform is still live; end-of-streaming is the
            // documented shutdown notification and failure is harmless.
            unsafe {
                let _ = ctx
                    .transform
                    .ProcessMessage(MFT_MESSAGE_NOTIFY_END_STREAMING, 0);
            }
            // Dropping `ctx` releases the transform first (declared first),
            // then the shared textures, then the device manager/device.
            drop(ctx);
            // SAFETY: all MF objects created by this decoder have been
            // released; balances `MFStartup`.
            unsafe {
                let _ = MFShutdown();
            };
        }
        if self.did_coinit {
            // SAFETY: balances the successful `CoInitializeEx` in `create`.
            unsafe { CoUninitialize() };
        }
    }
}

/// Builds the decoder input media type for `config`.
fn build_input_type(config: &DecoderConfig) -> Result<IMFMediaType, MediaError> {
    // SAFETY: MFCreateMediaType allocates a fresh media type object.
    let mt = unsafe { MFCreateMediaType() }.map_err(|e| mf_fatal("MFCreateMediaType", &e))?;

    // SAFETY: `mt` is a live IMFMediaType (an IMFAttributes); all writes
    // target documented attribute keys.
    unsafe {
        mt.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
            .map_err(|e| mf_fatal("SetGUID(MF_MT_MAJOR_TYPE)", &e))?;
        mt.SetGUID(&MF_MT_SUBTYPE, &codec_subtype(config.codec))
            .map_err(|e| mf_fatal("SetGUID(MF_MT_SUBTYPE)", &e))?;
        mt.SetUINT32(
            &MF_MT_INTERLACE_MODE,
            MFVideoInterlace_MixedInterlaceOrProgressive.0 as u32,
        )
        .map_err(|e| mf_fatal("SetUINT32(MF_MT_INTERLACE_MODE)", &e))?;
        if let Some(extra) = &config.codec_config {
            if !extra.is_empty() {
                mt.SetBlob(&MF_MT_USER_DATA, extra)
                    .map_err(|e| mf_fatal("SetBlob(MF_MT_USER_DATA)", &e))?;
            }
        }
        if config.width != 0 && config.height != 0 {
            mt.SetUINT64(
                &MF_MT_FRAME_SIZE,
                pack_u32_pair(config.width, config.height),
            )
            .map_err(|e| mf_fatal("SetUINT64(MF_MT_FRAME_SIZE)", &e))?;
        }
        // Nominal 30 fps — a hint only; the MFT derives the real cadence
        // from the stream.
        mt.SetUINT64(&MF_MT_FRAME_RATE, pack_u32_pair(30, 1))
            .map_err(|e| mf_fatal("SetUINT64(MF_MT_FRAME_RATE)", &e))?;
    }
    Ok(mt)
}

/// Enumerates video-decoder MFTs accepting `codec`'s input type and NV12
/// output, returning activated `IMFTransform`s. Enumeration or activation
/// failures are skipped — the caller falls through to the next flag set or
/// reports `UnsupportedCodec`.
fn enumerate_mfts(codec: VideoCodec, flags: MFT_ENUM_FLAG) -> Vec<IMFTransform> {
    let in_info = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: codec_subtype(codec),
    };
    let out_info = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: MFVideoFormat_NV12,
    };

    let mut activates: *mut Option<IMFActivate> = std::ptr::null_mut();
    let mut count: u32 = 0;
    // SAFETY: both out pointers are valid; on success `activates` is a
    // `CoTaskMem` array of `count` interface pointers.
    let enum_ok = unsafe {
        MFTEnumEx(
            MFT_CATEGORY_VIDEO_DECODER,
            flags,
            Some(&in_info),
            Some(&out_info),
            &mut activates,
            &mut count,
        )
    };
    if enum_ok.is_err() || activates.is_null() || count == 0 {
        return Vec::new();
    }

    // SAFETY: `activates` points to `count` `Option<IMFActivate>` slots per
    // the successful MFTEnumEx call; taking each element transfers its
    // reference to us, and `CoTaskMemFree` releases the array itself (not
    // the objects).
    let taken: Vec<Option<IMFActivate>> = unsafe {
        let slots = std::slice::from_raw_parts_mut(activates, count as usize);
        let v = slots.iter_mut().map(Option::take).collect();
        CoTaskMemFree(Some(activates.cast::<c_void>()));
        v
    };

    taken
        .into_iter()
        .flatten()
        .filter_map(|activate| {
            // SAFETY: `activate` is a live IMFActivate owned by us;
            // `ActivateObject` creates the MFT in-process.
            unsafe { activate.ActivateObject::<IMFTransform>() }.ok()
        })
        .collect()
}

/// Configures one candidate MFT: rejects async transforms, hands over the
/// DXGI device manager, sets the input type, then tries NV12 then P010
/// output.
fn configure_mft(
    transform: &IMFTransform,
    config: &DecoderConfig,
    input_type: &IMFMediaType,
    d3d: Option<&D3d11>,
) -> Result<MftSetup, DecodeError> {
    // Async MFTs require the event-driven model, which this synchronous
    // backend does not implement — skip them.
    // SAFETY: read-only attribute access on a live transform.
    if let Ok(attrs) = unsafe { transform.GetAttributes() } {
        if unsafe { attrs.GetUINT32(&MF_TRANSFORM_ASYNC) }.unwrap_or(0) != 0 {
            return Err(DecodeError::UnsupportedCodec(
                "async MFTs need the event model; skipped".to_string(),
            ));
        }
    }

    // Offer the DXGI device manager before setting the output type so the
    // MFT can advertise D3D11 surface output.
    let d3d_enabled = match d3d {
        Some(d3d) => {
            // SAFETY: `ulParam` for MFT_MESSAGE_SET_D3D_MANAGER is the raw
            // IMFDXGIDeviceManager pointer, borrowed for the duration of the
            // call.
            unsafe {
                transform.ProcessMessage(
                    MFT_MESSAGE_SET_D3D_MANAGER,
                    Interface::as_raw(&d3d.manager) as usize,
                )
            }
            .is_ok()
        }
        None => false,
    };
    if !config.allow_software && !d3d_enabled {
        return Err(DecodeError::UnsupportedCodec(
            "MFT did not accept the DXGI device manager and software decode is disabled"
                .to_string(),
        ));
    }

    // SAFETY: `input_type` is a live media type built above.
    unsafe { transform.SetInputType(STREAM_ID, input_type, 0) }
        .map_err(|e| DecodeError::UnsupportedCodec(format!("SetInputType rejected by MFT: {e}")))?;

    for (subtype, format) in [
        (MFVideoFormat_NV12, VideoPixelFormat::Nv12),
        (MFVideoFormat_P010, VideoPixelFormat::P010),
    ] {
        // SAFETY: MFCreateMediaType allocates a fresh object; all writes are
        // to documented attribute keys.
        let out_type = unsafe { MFCreateMediaType() }
            .map_err(|e| DecodeError::Fatal(format!("MFCreateMediaType: {e}")))?;
        let built = unsafe {
            out_type
                .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
                .and_then(|()| out_type.SetGUID(&MF_MT_SUBTYPE, &subtype))
                .and_then(|()| {
                    out_type.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)
                })
                .and_then(|()| {
                    out_type.SetUINT64(
                        &MF_MT_FRAME_SIZE,
                        pack_u32_pair(config.width.max(2), config.height.max(2)),
                    )
                })
        };
        if built.is_err() {
            continue;
        }
        // SAFETY: `out_type` is a live, fully populated media type.
        if unsafe { transform.SetOutputType(STREAM_ID, &out_type, 0) }.is_ok() {
            let mut width = 0;
            let mut height = 0;
            // SAFETY: read-only attribute access on the current output type.
            if let Ok(mt) = unsafe { transform.GetOutputCurrentType(STREAM_ID) } {
                if let Ok(packed) = unsafe { mt.GetUINT64(&MF_MT_FRAME_SIZE) } {
                    (width, height) = unpack_u32_pair(packed);
                }
            }
            return Ok(MftSetup {
                format,
                d3d_enabled,
                width,
                height,
            });
        }
    }

    Err(DecodeError::UnsupportedCodec(
        "MFT accepts input but no NV12/P010 output type".to_string(),
    ))
}

/// Creates the D3D11 device + DXGI device manager stack.
fn create_d3d11() -> Result<D3d11, MediaError> {
    let flags = D3D11_CREATE_DEVICE_VIDEO_SUPPORT | D3D11_CREATE_DEVICE_BGRA_SUPPORT;
    let mut device: Option<ID3D11Device> = None;
    // SAFETY: standard device creation; null adapter lets the runtime pick
    // the default GPU; `None` feature-level array requests the default set.
    let created = unsafe {
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE::default(),
            flags,
            None,
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            None,
        )
    };
    if created.is_err() {
        // SAFETY: same call with the WARP software rasterizer as a fallback
        // for headless machines; the manager still enables CPU-side sample
        // handling.
        let _ = unsafe {
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_WARP,
                HMODULE::default(),
                flags,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                None,
            )
        };
    }
    let device = device.ok_or_else(|| {
        DecodeError::Fatal("D3D11CreateDevice failed on all driver types".to_string())
    })?;

    // SAFETY: `device` is live; the immediate context is a new reference.
    let context =
        unsafe { device.GetImmediateContext() }.map_err(|e| mf_fatal("GetImmediateContext", &e))?;

    let mut token: u32 = 0;
    let mut manager: Option<IMFDXGIDeviceManager> = None;
    // SAFETY: both out parameters are valid.
    unsafe { MFCreateDXGIDeviceManager(&mut token, &mut manager) }
        .map_err(|e| mf_fatal("MFCreateDXGIDeviceManager", &e))?;
    let manager = manager.ok_or_else(|| {
        DecodeError::Fatal("MFCreateDXGIDeviceManager returned no manager".to_string())
    })?;

    // SAFETY: `IMFDXGIDeviceManager::ResetDevice` takes the device as a raw
    // `IUnknown` at the ABI level, but the generated binding declares the
    // parameter `IDirect3DDevice9`, which an `ID3D11Device` can never
    // `QueryInterface`. Calling the vtable entry directly with the D3D11
    // device pointer is exactly what the Windows API expects.
    let hr = unsafe {
        (Interface::vtable(&manager).ResetDevice)(
            Interface::as_raw(&manager),
            Interface::as_raw(&device),
            token,
        )
    };
    hr.ok()
        .map_err(|e| mf_fatal("IMFDXGIDeviceManager::ResetDevice", &e))?;

    Ok(D3d11 {
        device,
        context,
        manager,
    })
}

/// Wraps `packet`'s bytes in an `IMFSample` with 100-ns timestamps.
fn build_input_sample(packet: &EncodedPacket) -> Result<IMFSample, MediaError> {
    let len = u32::try_from(packet.data.len())
        .map_err(|_| DecodeError::StreamCorrupt("packet too large".to_string()))?;

    // SAFETY: MFCreateMemoryBuffer allocates a fresh buffer of `len` bytes.
    let buffer =
        unsafe { MFCreateMemoryBuffer(len) }.map_err(|e| mf_fatal("MFCreateMemoryBuffer", &e))?;

    let mut dst: *mut u8 = std::ptr::null_mut();
    let mut max_len: u32 = 0;
    // SAFETY: `buffer` is live; both out pointers are valid.
    let locked = unsafe { buffer.Lock(&mut dst, Some(&mut max_len), None) };
    if let Err(e) = locked {
        return Err(mf_fatal("IMFMediaBuffer::Lock", &e));
    }
    if dst.is_null() || (max_len as usize) < packet.data.len() {
        // SAFETY: balances the successful Lock above.
        unsafe {
            let _ = buffer.Unlock();
        };
        return Err(DecodeError::Fatal("MFCreateMemoryBuffer too small".to_string()).into());
    }
    // SAFETY: `dst` is valid for `max_len` bytes and
    // `data.len() <= max_len`; source and destination do not overlap.
    unsafe { std::ptr::copy_nonoverlapping(packet.data.as_ptr(), dst, packet.data.len()) };
    // SAFETY: balances the Lock.
    unsafe { buffer.Unlock() }.map_err(|e| mf_fatal("IMFMediaBuffer::Unlock", &e))?;
    // SAFETY: `buffer` is live; exactly `len` bytes were written above.
    unsafe { buffer.SetCurrentLength(len) }.map_err(|e| mf_fatal("SetCurrentLength", &e))?;

    // SAFETY: MFCreateSample allocates a fresh sample.
    let sample = unsafe { MFCreateSample() }.map_err(|e| mf_fatal("MFCreateSample", &e))?;
    // SAFETY: `sample`/`buffer` are live; the timestamp calls take plain
    // values. MF timestamps are 100-ns units → nanos / 100.
    unsafe {
        sample
            .AddBuffer(&buffer)
            .map_err(|e| mf_fatal("IMFSample::AddBuffer", &e))?;
        sample
            .SetSampleTime((packet.pts_nanos / 100) as i64)
            .map_err(|e| mf_fatal("SetSampleTime", &e))?;
        if packet.duration_nanos != 0 {
            sample
                .SetSampleDuration((packet.duration_nanos / 100) as i64)
                .map_err(|e| mf_fatal("SetSampleDuration", &e))?;
        }
        if packet.is_keyframe {
            sample
                .SetUINT32(&MFSampleExtension_CleanPoint, 1)
                .map_err(|e| mf_fatal("SetUINT32(CleanPoint)", &e))?;
        }
    }
    Ok(sample)
}

/// Allocates an output sample of `cb_size` bytes for MFTs that do not
/// provide their own samples.
fn new_output_sample(cb_size: u32) -> Result<IMFSample, MediaError> {
    // SAFETY: both creators allocate fresh objects; `AddBuffer` attaches the
    // buffer to the sample.
    unsafe {
        let sample = MFCreateSample().map_err(|e| mf_fatal("MFCreateSample", &e))?;
        let buffer = MFCreateMemoryBuffer(cb_size.max(1))
            .map_err(|e| mf_fatal("MFCreateMemoryBuffer", &e))?;
        sample
            .AddBuffer(&buffer)
            .map_err(|e| mf_fatal("IMFSample::AddBuffer", &e))?;
        Ok(sample)
    }
}

/// Copies a locked bi-planar NV12/P010 buffer into owned `Vec`s.
///
/// # Safety
///
/// `scanline` must be valid for `pitch` bytes per row over `height` luma
/// rows followed by `height / 2` interleaved-chroma rows — the layout
/// `IMF2DBuffer::Lock2D` guarantees for NV12/P010 output.
unsafe fn copy_biplanar(
    scanline: *const u8,
    pitch: u32,
    width: u32,
    height: u32,
    bytes_per_pixel: u32,
) -> Result<HardwareHandle, MediaError> {
    let _ = (width, bytes_per_pixel);
    let pitch_us = pitch as usize;
    let y_plane_len = pitch_us
        .checked_mul(height as usize)
        .ok_or_else(|| DecodeError::StreamCorrupt("plane size overflow".to_string()))?;
    let uv_plane_len = pitch_us
        .checked_mul(height as usize / 2)
        .ok_or_else(|| DecodeError::StreamCorrupt("plane size overflow".to_string()))?;

    // SAFETY: upheld by the caller contract documented above.
    let y_plane = unsafe { std::slice::from_raw_parts(scanline, y_plane_len) }.to_vec();
    let uv_plane =
        unsafe { std::slice::from_raw_parts(scanline.add(y_plane_len), uv_plane_len) }.to_vec();

    Ok(HardwareHandle::CpuMemory {
        y_plane,
        uv_plane,
        y_stride: pitch,
        uv_stride: pitch,
    })
}

/// Maps an `MFVideoPrimaries` value to an ISO 23001-8 colour-primaries code.
fn primaries_iso_code(primaries: Option<u32>) -> u16 {
    let Some(p) = primaries else { return 1 };
    if p == MFVideoPrimaries_BT709.0 as u32 {
        1
    } else if p == MFVideoPrimaries_SMPTE170M.0 as u32
        || p == MFVideoPrimaries_BT470_2_SysM.0 as u32
        || p == MFVideoPrimaries_BT470_2_SysBG.0 as u32
    {
        6
    } else if p == MFVideoPrimaries_SMPTE240M.0 as u32 {
        7
    } else if p == MFVideoPrimaries_BT2020.0 as u32 {
        9
    } else if p == MFVideoPrimaries_XYZ.0 as u32 {
        10
    } else if p == MFVideoPrimaries_DCI_P3.0 as u32 {
        12
    } else {
        1
    }
}

/// Maps an `MFVideoTransferFunction` value to an ISO 23001-8 transfer
/// (EOTF) code.
fn transfer_eotf(transfer: Option<u32>) -> u16 {
    let Some(t) = transfer else { return 1 };
    if t == MFVideoTransFunc_2084.0 as u32 {
        16
    } else if t == MFVideoTransFunc_HLG.0 as u32 {
        18
    } else if t == MFVideoTransFunc_sRGB.0 as u32 {
        13
    } else if t == MFVideoTransFunc_2020.0 as u32 || t == MFVideoTransFunc_2020_const.0 as u32 {
        14
    } else {
        // Gamma-based SDR transfers (709/10/18/20/22/240M/26/28) collapse
        // onto the SDR/BT.709 code for our purposes.
        1
    }
}
