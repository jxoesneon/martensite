//! macOS VideoToolbox hardware decoder backend.
//!
//! Wraps `VTDecompressionSession` (via `objc2-video-toolbox`) and emits
//! decoded frames as [`HardwareHandle::IoSurface`] handles extracted from the
//! `IOSurface` backing each output `CVPixelBuffer`. Decoded frames are queued
//! in a bounded reorder buffer ([`DecoderConfig::decode_ahead`]); when the
//! queue overruns, the oldest frame is dropped and counted through
//! [`DecodeStats::packets_rejected`] (there is no dedicated dropped-frame
//! counter).
//!
//! # Bitstream framing
//!
//! - With [`DecoderConfig::codec_config`] the extradata is an
//!   `AVCDecoderConfigurationRecord` (`avcC`) for H.264 or an
//!   `HEVCDecoderConfigurationRecord` (`hvcC`) for HEVC; the corresponding
//!   `CMVideoFormatDescription` is built from the contained parameter sets
//!   and `send_packet` expects length-prefixed (AVCC/HVCC) access units.
//! - Without extradata a bare `CMVideoFormatDescription` is created and
//!   VideoToolbox runs in deferred mode, discovering parameter sets in-band
//!   (Annex-B). This is best-effort: hardware decoders may refuse to emit
//!   frames until they have seen VPS/SPS/PPS in the stream.
//! - AV1 has no `CMVideoFormatDescriptionCreateFromAV1ParameterSets`
//!   equivalent in `objc2-core-media` 0.3.2, so `av1C` extradata is ignored
//!   and a bare format description is used.
//! - VP9 is rejected up front: although `kCMVideoCodecType_VP9` exists, no
//!   CoreMedia helper builds a `vpcC` format description and VideoToolbox's
//!   VP9 support is not usable through this path.
//!
//! # CoreFoundation access note
//!
//! `objc2-core-foundation` is only a *transitive* dependency of this crate,
//! so its types (`CFDictionary`, `CFBoolean`, `CFData`, `CFRetained`) cannot
//! be named directly. The handful of CoreFoundation entry points this module
//! needs (`CFRetain`/`CFRelease`, `CFEqual`, `CFGetTypeID`, `CFData*`,
//! `CFDictionaryCreate`) are declared as local `extern` shims operating on
//! `*const c_void` (`CFTypeRef`) values; typed CF objects returned by the
//! objc2 crates are only ever manipulated through inferred bindings and
//! method calls.
//!
//! # Safety
//!
//! All `unsafe` blocks call CoreMedia / CoreVideo / VideoToolbox / CoreFoundation
//! C entry points that are documented to be safe under the invariants stated
//! at each site. `VTDecompressionSession` is documented by Apple as callable
//! from any thread; the asynchronous output callback may run on a
//! VideoToolbox-internal thread, so the shared output queue is `Mutex`
//! protected.

use core::ffi::c_void;
use core::ptr::NonNull;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use objc2_core_media::{
    kCMBlockBufferAssureMemoryNowFlag, kCMFormatDescriptionColorPrimaries_DCI_P3,
    kCMFormatDescriptionColorPrimaries_EBU_3213, kCMFormatDescriptionColorPrimaries_ITU_R_2020,
    kCMFormatDescriptionColorPrimaries_P3_D65, kCMFormatDescriptionColorPrimaries_SMPTE_C,
    kCMFormatDescriptionExtension_ColorPrimaries,
    kCMFormatDescriptionExtension_ContentLightLevelInfo,
    kCMFormatDescriptionExtension_FullRangeVideo,
    kCMFormatDescriptionExtension_MasteringDisplayColorVolume,
    kCMFormatDescriptionExtension_TransferFunction,
    kCMFormatDescriptionTransferFunction_ITU_R_2020,
    kCMFormatDescriptionTransferFunction_ITU_R_2100_HLG,
    kCMFormatDescriptionTransferFunction_Linear,
    kCMFormatDescriptionTransferFunction_SMPTE_240M_1995,
    kCMFormatDescriptionTransferFunction_SMPTE_ST_2084_PQ,
    kCMFormatDescriptionTransferFunction_SMPTE_ST_428_1, kCMFormatDescriptionTransferFunction_sRGB,
    kCMTimeInvalid, kCMVideoCodecType_AV1, kCMVideoCodecType_H264, kCMVideoCodecType_HEVC,
    CMBlockBuffer, CMFormatDescription, CMSampleBuffer, CMSampleTimingInfo, CMTime, CMTimeFlags,
    CMVideoFormatDescriptionCreate, CMVideoFormatDescriptionCreateFromH264ParameterSets,
    CMVideoFormatDescriptionCreateFromHEVCParameterSets,
};
use objc2_core_video::{
    kCVPixelFormatType_32ARGB, kCVPixelFormatType_32BGRA, kCVPixelFormatType_32RGBA,
    kCVPixelFormatType_420YpCbCr10BiPlanarFullRange,
    kCVPixelFormatType_420YpCbCr10BiPlanarVideoRange,
    kCVPixelFormatType_420YpCbCr8BiPlanarFullRange,
    kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange, CVImageBuffer, CVPixelBuffer,
    CVPixelBufferGetHeight, CVPixelBufferGetPixelFormatType, CVPixelBufferGetWidth,
};
use objc2_io_surface::IOSurfaceRef;
use objc2_video_toolbox::{
    kVTVideoDecoderSpecification_EnableHardwareAcceleratedVideoDecoder,
    kVTVideoDecoderSpecification_RequireHardwareAcceleratedVideoDecoder, VTDecodeFrameFlags,
    VTDecodeInfoFlags, VTDecompressionOutputCallbackRecord, VTDecompressionSession,
};

use crate::decoder::{
    DecodeError, DecodeStats, DecodedFrame, DecoderBackend, DecoderConfig, EncodedPacket,
    HdrSideData, VideoCodec,
};
use crate::surface::{
    ColorRange, HardwareHandle, MediaError, VideoFrameMetadata, VideoPixelFormat,
};

// ---------------------------------------------------------------------------
// Local CoreFoundation FFI shims.
//
// `objc2-core-foundation` is not a direct dependency of this crate (see the
// module-level note), so the CF symbols we need are declared here over
// opaque `CFTypeRef`-style `*const c_void` pointers. CoreFoundation.framework
// is already linked into any binary that links CoreMedia.
// ---------------------------------------------------------------------------

#[link(name = "CoreFoundation", kind = "framework")]
extern "C-unwind" {
    /// Increments the retain count of a CF object. Returns the object.
    fn CFRetain(cf: *const c_void) -> *const c_void;
    /// Decrements the retain count of a CF object, deallocating it at zero.
    fn CFRelease(cf: *const c_void);
    /// Generic CF object equality.
    fn CFEqual(cf1: *const c_void, cf2: *const c_void) -> u8;
    /// Returns the runtime type identifier of a CF object.
    fn CFGetTypeID(cf: *const c_void) -> usize;
    /// Returns the `CFData` runtime type identifier.
    fn CFDataGetTypeID() -> usize;
    /// Returns the byte length of a `CFData` object.
    fn CFDataGetLength(the_data: *const c_void) -> isize;
    /// Returns a pointer to the bytes of a `CFData` object.
    fn CFDataGetBytePtr(the_data: *const c_void) -> *const u8;
    /// Creates an immutable `CFDictionary`. `keys`/`values` are arrays of
    /// `CFTypeRef`; the callback tables control retain/copy semantics.
    fn CFDictionaryCreate(
        allocator: *const c_void,
        keys: *const *const c_void,
        values: *const *const c_void,
        num_values: isize,
        key_callbacks: *const c_void,
        value_callbacks: *const c_void,
    ) -> *mut c_void;
}

extern "C" {
    /// `const CFBooleanRef kCFBooleanTrue` — a global holding a CFBoolean pointer.
    static kCFBooleanTrue: *const c_void;
    /// `const CFBooleanRef kCFBooleanFalse` — a global holding a CFBoolean pointer.
    static kCFBooleanFalse: *const c_void;
    /// `const CFDictionaryKeyCallBacks kCFTypeDictionaryKeyCallBacks`.
    /// Only the address of this global is used; its declared type is opaque.
    static kCFTypeDictionaryKeyCallBacks: u8;
    /// `const CFDictionaryValueCallBacks kCFTypeDictionaryValueCallBacks`.
    /// Only the address of this global is used; its declared type is opaque.
    static kCFTypeDictionaryValueCallBacks: u8;
}

#[link(name = "CoreVideo", kind = "framework")]
extern "C-unwind" {
    /// Returns the `IOSurface` backing a pixel buffer, or null if it is not
    /// IOSurface-backed. The returned reference is borrowed (`+0`); the pixel
    /// buffer keeps the surface alive.
    ///
    /// Declared locally because `objc2-core-video`'s own binding is gated
    /// behind its `objc2-io-surface` cargo feature, which this crate's
    /// `Cargo.toml` does not enable.
    fn CVPixelBufferGetIOSurface(pixel_buffer: &CVPixelBuffer) -> *const IOSurfaceRef;
}

// ---------------------------------------------------------------------------
// Small RAII / utility plumbing.
// ---------------------------------------------------------------------------

/// An owned (`+1` retain count) CoreFoundation object released via `CFRelease`.
struct CfOwned<T> {
    ptr: NonNull<T>,
}

impl<T> CfOwned<T> {
    /// Takes ownership of a `+1` (Create-rule) CF object pointer.
    ///
    /// # Safety
    ///
    /// `ptr` must be a valid CF object reference with an owned retain count
    /// (e.g. the result of a `*Create` function written to an out-parameter).
    unsafe fn from_owned(ptr: *mut T) -> Option<Self> {
        NonNull::new(ptr).map(|ptr| Self { ptr })
    }

    /// Returns a shared reference to the owned CF object.
    fn get(&self) -> &T {
        // SAFETY: `ptr` is a valid, live CF object for the lifetime of `self`
        // (the `+1` retain is only released in `Drop`).
        unsafe { self.ptr.as_ref() }
    }
}

impl<T> Drop for CfOwned<T> {
    fn drop(&mut self) {
        // SAFETY: `ptr` is a valid CFTypeRef whose `+1` retain we own.
        unsafe { CFRelease(self.ptr.as_ptr().cast()) }
    }
}

// SAFETY: `CfOwned` is only instantiated with immutable CF objects
// (`VTDecompressionSession`, `CMFormatDescription`, opaque `c_void` image
// buffers). CF objects are reference counted; the pointers are only used
// through thread-safe CF/VT entry points and released once in `Drop`.
unsafe impl<T> Send for CfOwned<T> {}
// SAFETY: see the `Send` impl; shared access is only via `get()` handing out
// `&T` to callers that use thread-safe API entry points.
unsafe impl<T> Sync for CfOwned<T> {}

/// Retains a CF object and wraps the new reference in [`CfOwned`].
fn retain_cf_object(ptr: *mut c_void) -> Option<CfOwned<c_void>> {
    if ptr.is_null() {
        return None;
    }
    // SAFETY: caller guarantees `ptr` is a valid CFTypeRef.
    let retained = unsafe { CFRetain(ptr) };
    NonNull::new(retained.cast_mut()).map(|ptr| CfOwned { ptr })
}

/// Locks a mutex, recovering the guard from a poisoned state.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Converts a `CMTime` to nanoseconds, returning 0 for invalid, indefinite,
/// infinite, or zero-timescale values and clamping negative times at 0.
fn cmtime_to_nanos(time: CMTime) -> u64 {
    if !time.flags.contains(CMTimeFlags::Valid)
        || time.flags.intersects(CMTimeFlags::ImpliedValueFlagsMask)
        || time.timescale <= 0
        || time.value < 0
    {
        return 0;
    }
    let nanos = i128::from(time.value) * 1_000_000_000 / i128::from(time.timescale);
    u64::try_from(nanos).unwrap_or(0)
}

/// Maps a [`VideoCodec`] to a `CMVideoCodecType` four-character code.
fn cm_codec_type(codec: VideoCodec) -> Result<u32, DecodeError> {
    match codec {
        VideoCodec::H264 => Ok(kCMVideoCodecType_H264),
        VideoCodec::Hevc => Ok(kCMVideoCodecType_HEVC),
        VideoCodec::Av1 => Ok(kCMVideoCodecType_AV1),
        // `kCMVideoCodecType_VP9` exists but there is no CoreMedia helper to
        // build a usable `vpcC` format description; treat VP9 as unsupported.
        VideoCodec::Vp9 => Err(DecodeError::UnsupportedCodec(
            "VP9 is not supported by the VideoToolbox backend".to_string(),
        )),
    }
}

/// Maps a `CVPixelFormatType` to our pixel-format and color-range pair.
/// Unrecognized formats fall back to `Nv12` limited-range, which matches the
/// dominant VideoToolbox output; callers should treat the handle's surface as
/// authoritative.
fn map_pixel_format(ostype: u32) -> (VideoPixelFormat, ColorRange) {
    if ostype == kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange {
        (VideoPixelFormat::Nv12, ColorRange::Limited)
    } else if ostype == kCVPixelFormatType_420YpCbCr8BiPlanarFullRange {
        (VideoPixelFormat::Nv12, ColorRange::Full)
    } else if ostype == kCVPixelFormatType_420YpCbCr10BiPlanarVideoRange {
        (VideoPixelFormat::P010, ColorRange::Limited)
    } else if ostype == kCVPixelFormatType_420YpCbCr10BiPlanarFullRange {
        (VideoPixelFormat::P010, ColorRange::Full)
    } else if ostype == kCVPixelFormatType_32BGRA
        || ostype == kCVPixelFormatType_32RGBA
        || ostype == kCVPixelFormatType_32ARGB
    {
        (VideoPixelFormat::Rgba8, ColorRange::Full)
    } else {
        (VideoPixelFormat::Nv12, ColorRange::Limited)
    }
}

/// Reads a big-endian `u16` at `offset`, or `None` when out of bounds.
fn be_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    bytes
        .get(offset..offset + 2)
        .map(|b| u16::from_be_bytes([b[0], b[1]]))
}

/// Reads a big-endian `u32` at `offset`, or `None` when out of bounds.
fn be_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    bytes
        .get(offset..offset + 4)
        .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

/// Parses an `AVCDecoderConfigurationRecord` (`avcC`) into the parameter-set
/// NAL units (SPS/PPS order preserved) and the NAL length-prefix size.
fn parse_avcc(record: &[u8]) -> Option<(Vec<&[u8]>, i32)> {
    if record.len() < 7 {
        return None;
    }
    let nal_length_size = i32::from(record[4] & 0x03) + 1;
    let num_sps = record[5] & 0x1f;
    let mut sets = Vec::new();
    let mut pos = 6usize;
    for _ in 0..num_sps {
        let len = usize::from(be_u16(record, pos)?);
        pos += 2;
        let end = pos.checked_add(len)?;
        sets.push(record.get(pos..end)?);
        pos = end;
    }
    let num_pps = *record.get(pos)?;
    pos += 1;
    for _ in 0..num_pps {
        let len = usize::from(be_u16(record, pos)?);
        pos += 2;
        let end = pos.checked_add(len)?;
        sets.push(record.get(pos..end)?);
        pos = end;
    }
    if sets.is_empty() {
        return None;
    }
    Some((sets, nal_length_size))
}

/// Parses an `HEVCDecoderConfigurationRecord` (`hvcC`) into the parameter-set
/// NAL units (VPS/SPS/PPS/SEI types 32–34, 39–40) and the NAL length-prefix
/// size.
fn parse_hvcc(record: &[u8]) -> Option<(Vec<&[u8]>, i32)> {
    if record.len() < 23 {
        return None;
    }
    let nal_length_size = i32::from(record[21] & 0x03) + 1;
    let num_arrays = record[22];
    let mut sets = Vec::new();
    let mut pos = 23usize;
    for _ in 0..num_arrays {
        let nal_unit_type = *record.get(pos)? & 0x3f;
        pos += 1;
        let num_nalus = be_u16(record, pos)?;
        pos += 2;
        for _ in 0..num_nalus {
            let len = usize::from(be_u16(record, pos)?);
            pos += 2;
            let end = pos.checked_add(len)?;
            let nal = record.get(pos..end)?;
            pos = end;
            // Only parameter-set / SEI NAL types are accepted by
            // CMVideoFormatDescriptionCreateFromHEVCParameterSets.
            if matches!(nal_unit_type, 32 | 33 | 34 | 39 | 40) {
                sets.push(nal);
            }
        }
    }
    if sets.is_empty() {
        return None;
    }
    Some((sets, nal_length_size))
}

// ---------------------------------------------------------------------------
// HDR side-data parsing from CMFormatDescription extensions.
// ---------------------------------------------------------------------------

/// Returns the bytes of a `CFData` object as a slice.
///
/// The returned slice borrows the CF object; the caller must keep the owning
/// `CFRetained` alive for the duration of the borrow.
///
/// # Safety
///
/// `ptr` must be a valid `CFDataRef` for the lifetime of the returned slice.
unsafe fn cf_data_bytes<'a>(ptr: *const c_void) -> Option<&'a [u8]> {
    // SAFETY: upheld by the caller — `ptr` is a valid CFDataRef.
    let len = unsafe { CFDataGetLength(ptr) };
    // SAFETY: same invariant.
    let bytes = unsafe { CFDataGetBytePtr(ptr) };
    if len <= 0 || bytes.is_null() {
        return None;
    }
    // SAFETY: `bytes` points to `len` readable bytes owned by the live CFData.
    Some(unsafe { std::slice::from_raw_parts(bytes, len as usize) })
}

/// Returns `true` when `ptr` is a `CFData` object.
///
/// # Safety
///
/// `ptr` must be a valid `CFTypeRef` (or null — `CFGetTypeID` handles null
/// per its contract of returning a type id for valid objects only; callers
/// never pass null).
unsafe fn is_cf_data(ptr: *const c_void) -> bool {
    // SAFETY: `ptr` is a valid CFTypeRef per the caller's invariant.
    unsafe { CFGetTypeID(ptr) == CFDataGetTypeID() }
}

/// Returns `true` when `ptr` equals the `kCFBooleanTrue` singleton.
///
/// # Safety
///
/// `ptr` must be a valid `CFTypeRef`.
unsafe fn is_cf_true(ptr: *const c_void) -> bool {
    // SAFETY: `kCFBooleanTrue` is a global CFBooleanRef; `ptr` is a valid
    // CFTypeRef per the caller's invariant.
    unsafe { CFEqual(ptr, kCFBooleanTrue) != 0 }
}

/// Parses HDR signalling from the format description's extension dictionary
/// (mastering-display colour volume, content light level, colour primaries,
/// transfer function, full-range flag). Returns `None` when no colour-related
/// extension is present.
///
/// # Safety
///
/// `desc` must be a valid `CMFormatDescription`.
unsafe fn parse_hdr_side_data(desc: &CMFormatDescription) -> Option<HdrSideData> {
    let mut found = false;
    let mut side = HdrSideData {
        eotf_code: 1,
        primaries_code: 1,
        full_range: false,
        max_luminance_nits: None,
        min_luminance_nits: None,
        max_cll: None,
        max_fall: None,
        dynamic_metadata: None,
    };

    // Transfer function → EOTF code (ISO 23001-8 numbering).
    // SAFETY: `desc` is valid; the extension key is a global CFString. The
    // returned CFPropertyList stays alive in `plist` for all comparisons.
    let plist = unsafe { desc.extension(kCMFormatDescriptionExtension_TransferFunction) };
    if let Some(plist) = plist {
        found = true;
        let p = core::ptr::from_ref(&*plist).cast::<c_void>();
        // SAFETY: all arguments are valid CFTypeRefs — `p` is the extension
        // value and the `kCMFormatDescriptionTransferFunction_*` statics are
        // global CFStrings.
        side.eotf_code = unsafe {
            if CFEqual(
                p,
                core::ptr::from_ref(kCMFormatDescriptionTransferFunction_SMPTE_ST_2084_PQ).cast(),
            ) != 0
            {
                16 // PQ / SMPTE ST 2084
            } else if CFEqual(
                p,
                core::ptr::from_ref(kCMFormatDescriptionTransferFunction_ITU_R_2100_HLG).cast(),
            ) != 0
            {
                18 // HLG
            } else if CFEqual(
                p,
                core::ptr::from_ref(kCMFormatDescriptionTransferFunction_ITU_R_2020).cast(),
            ) != 0
            {
                14 // BT.2020 (10-bit OETF)
            } else if CFEqual(
                p,
                core::ptr::from_ref(kCMFormatDescriptionTransferFunction_sRGB).cast(),
            ) != 0
            {
                13 // sRGB
            } else if CFEqual(
                p,
                core::ptr::from_ref(kCMFormatDescriptionTransferFunction_Linear).cast(),
            ) != 0
            {
                8 // Linear
            } else if CFEqual(
                p,
                core::ptr::from_ref(kCMFormatDescriptionTransferFunction_SMPTE_240M_1995).cast(),
            ) != 0
            {
                7 // SMPTE 240M
            } else if CFEqual(
                p,
                core::ptr::from_ref(kCMFormatDescriptionTransferFunction_SMPTE_ST_428_1).cast(),
            ) != 0
            {
                17 // SMPTE ST 428-1
            } else {
                // ITU_R_709_2, UseGamma, and anything unrecognized → SDR.
                1
            }
        };
    }

    // Colour primaries → ISO 23001-8 code.
    // SAFETY: same invariants as the transfer-function extension above.
    let plist = unsafe { desc.extension(kCMFormatDescriptionExtension_ColorPrimaries) };
    if let Some(plist) = plist {
        found = true;
        let p = core::ptr::from_ref(&*plist).cast::<c_void>();
        // SAFETY: all arguments are valid CFTypeRefs.
        side.primaries_code = unsafe {
            if CFEqual(
                p,
                core::ptr::from_ref(kCMFormatDescriptionColorPrimaries_ITU_R_2020).cast(),
            ) != 0
            {
                9 // BT.2020
            } else if CFEqual(
                p,
                core::ptr::from_ref(kCMFormatDescriptionColorPrimaries_DCI_P3).cast(),
            ) != 0
                || CFEqual(
                    p,
                    core::ptr::from_ref(kCMFormatDescriptionColorPrimaries_P3_D65).cast(),
                ) != 0
            {
                12 // DCI-P3 / P3-D65
            } else if CFEqual(
                p,
                core::ptr::from_ref(kCMFormatDescriptionColorPrimaries_EBU_3213).cast(),
            ) != 0
            {
                22 // EBU Tech 3213
            } else if CFEqual(
                p,
                core::ptr::from_ref(kCMFormatDescriptionColorPrimaries_SMPTE_C).cast(),
            ) != 0
            {
                6 // SMPTE C (NTSC)
            } else {
                // ITU_R_709_2, P22, and unrecognized values → BT.709.
                1
            }
        };
    }

    // Full-range flag.
    // SAFETY: same invariants as above.
    let plist = unsafe { desc.extension(kCMFormatDescriptionExtension_FullRangeVideo) };
    if let Some(plist) = plist {
        let p = core::ptr::from_ref(&*plist).cast::<c_void>();
        // SAFETY: `p` is a valid CFTypeRef (CFBoolean/CFNumber extension value).
        side.full_range = unsafe { is_cf_true(p) };
    }

    // Mastering-display colour volume: a 24-byte big-endian
    // `MasteringDisplayColourVolume` record (SMPTE ST 2086). The last two
    // u32 fields are max/min display mastering luminance in 0.0001 nits.
    // SAFETY: same invariants as above.
    let plist =
        unsafe { desc.extension(kCMFormatDescriptionExtension_MasteringDisplayColorVolume) };
    if let Some(plist) = plist {
        let p = core::ptr::from_ref(&*plist).cast::<c_void>();
        // SAFETY: `p` is a valid CFTypeRef; `cf_data_bytes` requires it to be
        // CFData, which `is_cf_data` verifies first.
        if unsafe { is_cf_data(p) } {
            // SAFETY: `p` is a CFData that outlives `plist`'s binding.
            if let Some(bytes) = unsafe { cf_data_bytes(p) } {
                if bytes.len() >= 24 {
                    if let Some(max) = be_u32(bytes, 16) {
                        side.max_luminance_nits = Some(max as f32 * 1e-4);
                    }
                    if let Some(min) = be_u32(bytes, 20) {
                        side.min_luminance_nits = Some(min as f32 * 1e-4);
                    }
                    found = true;
                }
            }
        }
    }

    // Content light level: 4-byte big-endian {MaxCLL u16, MaxFALL u16}.
    // SAFETY: same invariants as above.
    let plist = unsafe { desc.extension(kCMFormatDescriptionExtension_ContentLightLevelInfo) };
    if let Some(plist) = plist {
        let p = core::ptr::from_ref(&*plist).cast::<c_void>();
        // SAFETY: `p` is a valid CFTypeRef; checked to be CFData before use.
        if unsafe { is_cf_data(p) } {
            // SAFETY: `p` is a CFData that outlives `plist`'s binding.
            if let Some(bytes) = unsafe { cf_data_bytes(p) } {
                if bytes.len() >= 4 {
                    side.max_cll = be_u16(bytes, 0);
                    side.max_fall = be_u16(bytes, 2);
                    found = true;
                }
            }
        }
    }

    found.then_some(side)
}

// ---------------------------------------------------------------------------
// Decoder-specification dictionary helper.
// ---------------------------------------------------------------------------

/// Builds a single-entry `CFDictionary` `{key: value}` with the standard
/// `kCFType*` callbacks (keys and values are retained).
///
/// Returns the raw `+1` dictionary pointer, or `None` on failure.
///
/// # Safety
///
/// `key` and `value` must be valid `CFTypeRef`s.
unsafe fn cf_dict_one(key: *const c_void, value: *const c_void) -> Option<NonNull<c_void>> {
    let keys = [key];
    let values = [value];
    // SAFETY: `keys`/`values` are arrays of one valid CFTypeRef each; the
    // callback-table globals are the standard kCFType* tables exported by
    // CoreFoundation. The returned pointer is a +1 object owned by us.
    let dict = unsafe {
        CFDictionaryCreate(
            std::ptr::null(),
            keys.as_ptr(),
            values.as_ptr(),
            1,
            (&raw const kCFTypeDictionaryKeyCallBacks).cast::<c_void>(),
            (&raw const kCFTypeDictionaryValueCallBacks).cast::<c_void>(),
        )
    };
    NonNull::new(dict)
}

/// A `CFTypeRef` static reference as a raw pointer.
///
/// `key` is one of the `&'static CFString` globals re-exported by
/// `objc2-video-toolbox`; taking its address yields the `CFStringRef`.
fn cf_key_ptr<T>(key: &'static T) -> *const c_void {
    core::ptr::from_ref(key).cast::<c_void>()
}

// ---------------------------------------------------------------------------
// Shared state between the decoder and the VT output callback.
// ---------------------------------------------------------------------------

/// A queued decoded frame plus the retained `CVPixelBuffer` that keeps its
/// `IOSurface` (and thus the pixels) alive until the frame is consumed.
struct PendingFrame {
    /// The frame handed to `try_recv_frame` callers.
    frame: DecodedFrame,
    /// Wall time from `send_packet` to the output callback.
    decode_nanos: u64,
    /// Retains the `CVPixelBuffer` so VideoToolbox's pool cannot recycle it
    /// (and overwrite the IOSurface contents) while queued.
    _keep_alive: CfOwned<c_void>,
}

/// Mutable state shared between [`VideoToolboxDecoder`] and the
/// `VTDecompressionOutputCallback`, always accessed under `Mutex`.
struct SharedState {
    /// Bounded queue of decoded-but-unconsumed frames.
    queue: VecDeque<PendingFrame>,
    /// Maximum queue length (`DecoderConfig::decode_ahead`, min 1).
    capacity: usize,
    /// `send_packet` submission instants, paired (approximately) with output
    /// callbacks to derive per-frame decode latency.
    submitted: VecDeque<Instant>,
    /// Frames dropped because the queue was at capacity; drained into
    /// [`DecodeStats::packets_rejected`] by `try_recv_frame`/`flush`.
    dropped: u64,
    /// First fatal `OSStatus` reported by the callback, if any.
    fatal: Option<i32>,
    /// Most recently observed output pixel format.
    format: VideoPixelFormat,
    /// Monotonic frame index.
    frame_index: u64,
    /// HDR side-data parsed at session creation, attached to every frame.
    hdr: Option<HdrSideData>,
}

/// Shared-state wrapper passed to VideoToolbox as `decompressionOutputRefCon`.
struct CallbackState {
    inner: Mutex<SharedState>,
}

/// `VTDecompressionOutputCallback` invoked (possibly on a VideoToolbox thread)
/// for each decompressed frame.
///
/// # Safety
///
/// Called by VideoToolbox with the `decompressionOutputRefCon` registered at
/// session creation and a valid `CVImageBuffer` pointer (or null on drops).
unsafe extern "C-unwind" fn decompression_output_callback(
    decompression_output_ref_con: *mut c_void,
    _source_frame_ref_con: *mut c_void,
    status: i32,
    _info_flags: VTDecodeInfoFlags,
    image_buffer: *mut CVImageBuffer,
    presentation_timestamp: CMTime,
    presentation_duration: CMTime,
) {
    // SAFETY: `decompression_output_ref_con` is the `Arc<CallbackState>`
    // pointer installed at `create`; it stays valid for as long as the
    // session can invoke callbacks because the decoder invalidates the
    // session (draining callbacks) before the `Arc` is released.
    let state = unsafe { &*(decompression_output_ref_con as *const CallbackState) };
    let mut shared = lock(&state.inner);
    let submitted_at = shared.submitted.pop_front();
    let decode_nanos = submitted_at.map_or(0, |t| t.elapsed().as_nanos() as u64);

    if status != 0 {
        shared.fatal.get_or_insert(status);
        return;
    }
    if image_buffer.is_null() {
        // Frame dropped by the decoder (e.g. kVTDecodeInfo_FrameDropped).
        shared.dropped = shared.dropped.saturating_add(1);
        return;
    }

    // SAFETY: `image_buffer` is a non-null CVImageBuffer delivered by
    // VideoToolbox. `CVPixelBuffer` is a `CVImageBuffer` subtype, so the cast
    // preserves object identity.
    let pixel_buffer: &CVPixelBuffer = unsafe { &*image_buffer.cast::<CVPixelBuffer>() };

    // SAFETY: `pixel_buffer` is a valid CVPixelBuffer delivered by VT.
    let iosurface = unsafe { CVPixelBufferGetIOSurface(pixel_buffer) };
    if iosurface.is_null() {
        // Pixel buffer is not IOSurface-backed (pure software decode path
        // without pool sharing); we cannot export it zero-copy.
        shared.dropped = shared.dropped.saturating_add(1);
        return;
    }
    // SAFETY: `iosurface` is a non-null borrowed IOSurfaceRef kept alive by
    // `pixel_buffer`, which we retain below.
    let surface_id = unsafe { (*iosurface).id() };

    let ostype = CVPixelBufferGetPixelFormatType(pixel_buffer);
    let (format, range) = map_pixel_format(ostype);
    shared.format = format;

    let width = CVPixelBufferGetWidth(pixel_buffer) as u32;
    let height = CVPixelBufferGetHeight(pixel_buffer) as u32;

    let mut metadata = VideoFrameMetadata::new(width, height, format, range);
    metadata.pts_nanos = cmtime_to_nanos(presentation_timestamp);
    metadata.duration_nanos = cmtime_to_nanos(presentation_duration);
    metadata.frame_index = shared.frame_index;
    shared.frame_index = shared.frame_index.saturating_add(1);

    let mut frame = DecodedFrame::new(HardwareHandle::IoSurface { surface_id }, metadata);
    frame.hdr = shared.hdr.clone();

    // Retain the pixel buffer so the decoder's pool cannot hand it back to
    // VideoToolbox and overwrite the IOSurface while the frame is queued.
    let Some(keep_alive) = retain_cf_object(image_buffer.cast()) else {
        shared.dropped = shared.dropped.saturating_add(1);
        return;
    };

    if shared.queue.len() >= shared.capacity {
        shared.queue.pop_front();
        shared.dropped = shared.dropped.saturating_add(1);
    }
    shared.queue.push_back(PendingFrame {
        frame,
        decode_nanos,
        _keep_alive: keep_alive,
    });
}

// ---------------------------------------------------------------------------
// VideoToolboxDecoder
// ---------------------------------------------------------------------------

/// Hardware video decoder backed by a `VTDecompressionSession`.
///
/// Decoded frames are emitted as [`HardwareHandle::IoSurface`] handles; import
/// them with [`crate::import_iosurface`]. See the module documentation for
/// bitstream-framing requirements and codec support.
///
/// # Examples
///
/// ```no_run
/// use martensite_media_platform::decoder::videotoolbox::VideoToolboxDecoder;
/// use martensite_media_platform::decoder::{DecoderConfig, VideoCodec};
///
/// let config = DecoderConfig::new(VideoCodec::H264, 1920, 1080);
/// let decoder = VideoToolboxDecoder::create(&config);
/// assert!(decoder.is_ok());
/// ```
pub struct VideoToolboxDecoder {
    /// The decompression session (`+1` CF object).
    session: CfOwned<VTDecompressionSession>,
    /// The video format description the session was created with.
    format_desc: CfOwned<CMFormatDescription>,
    /// State shared with the output callback via `decompressionOutputRefCon`.
    /// Declared before `session` matters not — `Drop` invalidates the session
    /// (draining all callbacks) before any field is released.
    state: Arc<CallbackState>,
    /// Whether a keyframe has been accepted since creation/last flush.
    seen_keyframe: bool,
    /// Decode-path telemetry.
    stats: DecodeStats,
    /// HDR side-data parsed from the format description at creation.
    hdr: Option<HdrSideData>,
}

// SAFETY: `VTDecompressionSession` is documented by Apple as callable from
// any thread (decode and teardown entry points serialize internally). All
// state shared with the output callback lives behind a `Mutex`; the raw CF
// object handles are only used through thread-safe entry points and are
// released in `Drop` after the session is invalidated, so no use-after-free
// or data race is possible when the decoder is moved across threads.
unsafe impl Send for VideoToolboxDecoder {}
// SAFETY: shared references only expose `&self` methods that take the
// `Mutex`-guarded shared state or read immutable fields (`stats` is accessed
// through `&mut self`/`&self` accessors that cannot race by Rust's rules).
unsafe impl Sync for VideoToolboxDecoder {}

impl VideoToolboxDecoder {
    /// Creates a decoder session for `config`.
    ///
    /// When `config.allow_software` is false the decoder specification
    /// requires hardware acceleration and session creation fails if none is
    /// available; when true, a failed default attempt is retried with
    /// hardware acceleration explicitly disabled (pure software VT decode).
    ///
    /// # Errors
    ///
    /// Returns [`MediaError`] (converted from [`DecodeError`]) when the codec
    /// is unsupported, the extradata is malformed, or session creation fails.
    pub fn create(config: &DecoderConfig) -> Result<Self, MediaError> {
        let format_desc = create_format_description(config)?;

        let state = Arc::new(CallbackState {
            inner: Mutex::new(SharedState {
                queue: VecDeque::new(),
                capacity: config.decode_ahead.max(1),
                submitted: VecDeque::new(),
                dropped: 0,
                fatal: None,
                format: VideoPixelFormat::Nv12,
                frame_index: 0,
                hdr: None,
            }),
        });

        let session = create_session(format_desc.get(), &state, config.allow_software)?;

        // SAFETY: `format_desc` is a valid, live CMFormatDescription.
        let hdr = unsafe { parse_hdr_side_data(format_desc.get()) };
        lock(&state.inner).hdr.clone_from(&hdr);

        let stats = DecodeStats {
            backend: Some(DecoderBackend::VideoToolbox),
            ..DecodeStats::default()
        };

        Ok(Self {
            session,
            format_desc,
            state,
            seen_keyframe: false,
            stats,
            hdr,
        })
    }

    /// Submits one compressed access unit for decoding.
    ///
    /// The packet is copied into a `CMBlockBuffer`/`CMSampleBuffer` and passed
    /// to `VTDecompressionSessionDecodeFrame` with asynchronous decompression
    /// enabled. Non-keyframe packets are rejected with
    /// [`DecodeError::NeedsKeyframe`] until a keyframe has been seen.
    ///
    /// # Errors
    ///
    /// Returns [`MediaError`] (converted from [`DecodeError`]) on empty
    /// packets, missing keyframes, or CoreMedia/VideoToolbox failures.
    pub fn send_packet(&mut self, packet: &EncodedPacket) -> Result<(), MediaError> {
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

        let len = packet.data.len();
        let block_buffer = create_block_buffer(&packet.data)?;

        // SAFETY: reading a CoreMedia global constant.
        let invalid_time = unsafe { kCMTimeInvalid };
        // SAFETY: `CMTime::new` requires a positive timescale; 1e9 > 0 and the
        // value is clamped to `i64::MAX`.
        let duration = if packet.duration_nanos > 0 {
            unsafe {
                CMTime::new(
                    packet.duration_nanos.min(i64::MAX as u64) as i64,
                    1_000_000_000,
                )
            }
        } else {
            invalid_time
        };
        // SAFETY: as above — positive timescale, clamped value.
        let pts =
            unsafe { CMTime::new(packet.pts_nanos.min(i64::MAX as u64) as i64, 1_000_000_000) };
        let timing = CMSampleTimingInfo {
            duration,
            presentationTimeStamp: pts,
            // Packets are submitted in presentation order; VideoToolbox does
            // not require a decode timestamp in that case.
            decodeTimeStamp: invalid_time,
        };
        let sample_size = len;

        let mut raw_sample: *mut CMSampleBuffer = std::ptr::null_mut();
        // SAFETY: `block_buffer` is a valid CMBlockBuffer containing `len`
        // bytes; `timing`/`sample_size` are valid single-element arrays;
        // `format_desc` matches the session's format; `raw_sample` is a valid
        // out-pointer receiving a +1 object on success.
        let status = unsafe {
            CMSampleBuffer::create(
                None,
                Some(block_buffer.get()),
                true,
                None,
                std::ptr::null_mut(),
                Some(self.format_desc.get()),
                1,
                1,
                &raw const timing,
                1,
                &raw const sample_size,
                NonNull::from(&mut raw_sample),
            )
        };
        if status != 0 {
            return Err(DecodeError::Fatal(format!(
                "CMSampleBufferCreate failed: OSStatus {status}"
            ))
            .into());
        }
        // SAFETY: on success `raw_sample` is a +1 CMSampleBuffer we own.
        let sample_buffer = unsafe { CfOwned::from_owned(raw_sample) }
            .ok_or_else(|| MediaError::ImportFailed("null CMSampleBuffer".to_string()))?;

        lock(&self.state.inner).submitted.push_back(Instant::now());

        let mut info_flags = VTDecodeInfoFlags::empty();
        // SAFETY: `sample_buffer` is a valid CMSampleBuffer with one sample;
        // the session is live. VideoToolbox retains whatever it needs for
        // asynchronous delivery, so the local buffers may be released when
        // this call returns.
        let status = unsafe {
            self.session.get().decode_frame(
                sample_buffer.get(),
                VTDecodeFrameFlags::Frame_EnableAsynchronousDecompression,
                std::ptr::null_mut(),
                &raw mut info_flags,
            )
        };
        if status != 0 {
            // No callback will arrive for this frame; keep the submit-time
            // queue paired.
            lock(&self.state.inner).submitted.pop_back();
            self.stats.record_rejection();
            return Err(DecodeError::Fatal(format!(
                "VTDecompressionSessionDecodeFrame failed: OSStatus {status}"
            ))
            .into());
        }

        self.stats.record_packet(len);
        Ok(())
    }

    /// Pops the oldest decoded frame, if one is ready.
    ///
    /// Also surfaces the first fatal error reported by the asynchronous
    /// output callback and drains the dropped-frame counter into
    /// [`DecodeStats::packets_rejected`].
    ///
    /// # Errors
    ///
    /// Returns [`MediaError`] (converted from [`DecodeError::Fatal`]) when the
    /// decoder reported an unrecoverable decode failure.
    pub fn try_recv_frame(&mut self) -> Result<Option<DecodedFrame>, MediaError> {
        let mut shared = lock(&self.state.inner);
        if let Some(status) = shared.fatal.take() {
            return Err(DecodeError::Fatal(format!(
                "decompression callback failed: OSStatus {status}"
            ))
            .into());
        }
        let dropped = std::mem::take(&mut shared.dropped);
        let pending = shared.queue.pop_front();
        drop(shared);

        for _ in 0..dropped {
            self.stats.record_rejection();
        }
        match pending {
            Some(pending) => {
                self.stats.record_frame(pending.decode_nanos);
                Ok(Some(pending.frame))
            }
            None => Ok(None),
        }
    }

    /// Drains all delayed frames and resets the decoder to pre-keyframe state.
    ///
    /// Calls `VTDecompressionSessionFinishDelayedFrames` followed by
    /// `VTDecompressionSessionWaitForAsynchronousFrames` so every outstanding
    /// callback has fired before the queue is cleared.
    ///
    /// # Errors
    ///
    /// Returns [`MediaError`] (converted from [`DecodeError::Fatal`]) when the
    /// session reports a failure while draining.
    pub fn flush(&mut self) -> Result<(), MediaError> {
        // SAFETY: the session is live; both are documented drain entry points
        // callable from any thread.
        let (finish_status, wait_status) = unsafe {
            (
                self.session.get().finish_delayed_frames(),
                self.session.get().wait_for_asynchronous_frames(),
            )
        };
        {
            let mut shared = lock(&self.state.inner);
            shared.queue.clear();
            shared.submitted.clear();
            let dropped = std::mem::take(&mut shared.dropped);
            drop(shared);
            for _ in 0..dropped {
                self.stats.record_rejection();
            }
        }
        self.seen_keyframe = false;
        let status = if finish_status != 0 {
            finish_status
        } else {
            wait_status
        };
        if status != 0 {
            return Err(DecodeError::Fatal(format!(
                "VTDecompressionSession drain failed: OSStatus {status}"
            ))
            .into());
        }
        Ok(())
    }

    /// Signals end-of-stream: emits all frames still held by the reorder
    /// buffer into the output queue without resetting stream state.
    ///
    /// Calls `VTDecompressionSessionFinishDelayedFrames` then
    /// `VTDecompressionSessionWaitForAsynchronousFrames`; unlike
    /// [`flush`](Self::flush) the decoded frames remain available through
    /// [`try_recv_frame`](Self::try_recv_frame) and the keyframe gate is not
    /// re-armed.
    ///
    /// # Errors
    ///
    /// Returns [`MediaError`] (converted from [`DecodeError::Fatal`]) when the
    /// session reports a failure while draining.
    pub fn end_of_stream(&mut self) -> Result<(), MediaError> {
        // SAFETY: the session is live; both are documented drain entry
        // points callable from any thread.
        let status = unsafe {
            self.session.get().finish_delayed_frames();
            self.session.get().wait_for_asynchronous_frames()
        };
        if status != 0 {
            return Err(DecodeError::Fatal(format!(
                "VTDecompressionSession end-of-stream drain failed: OSStatus {status}"
            ))
            .into());
        }
        Ok(())
    }

    /// The pixel format most recently observed on decoded output.
    ///
    /// Defaults to [`VideoPixelFormat::Nv12`] until the first frame arrives.
    #[must_use]
    pub fn negotiated_format(&self) -> VideoPixelFormat {
        lock(&self.state.inner).format
    }

    /// HDR side-data parsed from the format description's extensions at
    /// creation time, if any colour-related extension was present.
    #[must_use]
    pub fn hdr_side_data(&self) -> Option<HdrSideData> {
        self.hdr.clone()
    }

    /// Rolling decode-path telemetry counters.
    #[must_use]
    pub fn stats(&self) -> &DecodeStats {
        &self.stats
    }
}

impl Drop for VideoToolboxDecoder {
    fn drop(&mut self) {
        // SAFETY: the session is live. Draining + invalidating guarantees no
        // output callback can still run (and touch `state`'s refcon pointer)
        // after this point.
        unsafe {
            self.session.get().finish_delayed_frames();
            self.session.get().wait_for_asynchronous_frames();
            self.session.get().invalidate();
        }
    }
}

// ---------------------------------------------------------------------------
// Construction helpers.
// ---------------------------------------------------------------------------

/// Builds a `CMVideoFormatDescription` from `config` — from `avcC`/`hvcC`
/// extradata when present, otherwise a bare codec/dimensions description for
/// deferred (Annex-B) decoding.
fn create_format_description(
    config: &DecoderConfig,
) -> Result<CfOwned<CMFormatDescription>, DecodeError> {
    let codec_type = cm_codec_type(config.codec)?;
    let mut raw: *const CMFormatDescription = std::ptr::null();

    let status = match (config.codec, config.codec_config.as_deref()) {
        (VideoCodec::H264, Some(record)) => {
            let (sets, nal_length_size) = parse_avcc(record)
                .ok_or_else(|| DecodeError::StreamCorrupt("malformed avcC record".to_string()))?;
            let mut ptrs: Vec<NonNull<u8>> = sets
                .iter()
                .map(|set| {
                    // SAFETY: `NonNull::new_unchecked` — a slice's data pointer
                    // is never null (dangling for empty slices, still non-null).
                    unsafe { NonNull::new_unchecked(set.as_ptr().cast_mut()) }
                })
                .collect();
            let mut sizes: Vec<usize> = sets.iter().map(|set| set.len()).collect();
            // SAFETY: `ptrs`/`sizes` are parallel non-empty arrays of valid
            // NAL unit pointers/lengths; `raw` is a valid out-pointer that
            // receives a +1 CMFormatDescription on success.
            unsafe {
                CMVideoFormatDescriptionCreateFromH264ParameterSets(
                    None,
                    ptrs.len(),
                    NonNull::new_unchecked(ptrs.as_mut_ptr()),
                    NonNull::new_unchecked(sizes.as_mut_ptr()),
                    nal_length_size,
                    NonNull::from(&mut raw),
                )
            }
        }
        (VideoCodec::Hevc, Some(record)) => {
            let (sets, nal_length_size) = parse_hvcc(record)
                .ok_or_else(|| DecodeError::StreamCorrupt("malformed hvcC record".to_string()))?;
            let mut ptrs: Vec<NonNull<u8>> = sets
                .iter()
                .map(|set| {
                    // SAFETY: slice data pointers are never null.
                    unsafe { NonNull::new_unchecked(set.as_ptr().cast_mut()) }
                })
                .collect();
            let mut sizes: Vec<usize> = sets.iter().map(|set| set.len()).collect();
            // SAFETY: as for the H.264 path above; `extensions` is null.
            unsafe {
                CMVideoFormatDescriptionCreateFromHEVCParameterSets(
                    None,
                    ptrs.len(),
                    NonNull::new_unchecked(ptrs.as_mut_ptr()),
                    NonNull::new_unchecked(sizes.as_mut_ptr()),
                    nal_length_size,
                    None,
                    NonNull::from(&mut raw),
                )
            }
        }
        // AV1: `objc2-core-media` 0.3.2 has no av1C → CMFormatDescription
        // bridge, and Annex-B / deferred mode uses the bare description.
        _ => {
            // SAFETY: `raw` is a valid out-pointer; width/height of 0 mean
            // "derive from stream" and are passed through to CoreMedia.
            unsafe {
                CMVideoFormatDescriptionCreate(
                    None,
                    codec_type,
                    config.width as i32,
                    config.height as i32,
                    None,
                    NonNull::from(&mut raw),
                )
            }
        }
    };

    if status != 0 {
        return Err(DecodeError::Fatal(format!(
            "CMVideoFormatDescriptionCreate failed: OSStatus {status}"
        )));
    }
    // SAFETY: on success `raw` is a +1 CMFormatDescription we own.
    unsafe { CfOwned::from_owned(raw.cast_mut()) }
        .ok_or_else(|| DecodeError::Fatal("null CMFormatDescription".to_string()))
}

/// Builds a `CMBlockBuffer` containing a copy of `data`.
fn create_block_buffer(data: &[u8]) -> Result<CfOwned<CMBlockBuffer>, DecodeError> {
    let mut raw: *mut CMBlockBuffer = std::ptr::null_mut();
    // SAFETY: `memory_block` is null so CoreMedia allocates the block
    // (AssureMemoryNow makes that allocation happen eagerly); `raw` is a
    // valid out-pointer receiving a +1 CMBlockBuffer on success.
    let status = unsafe {
        CMBlockBuffer::create_with_memory_block(
            None,
            std::ptr::null_mut(),
            data.len(),
            None,
            std::ptr::null(),
            0,
            data.len(),
            kCMBlockBufferAssureMemoryNowFlag,
            NonNull::from(&mut raw),
        )
    };
    if status != 0 {
        return Err(DecodeError::Fatal(format!(
            "CMBlockBufferCreateWithMemoryBlock failed: OSStatus {status}"
        )));
    }
    // SAFETY: on success `raw` is a +1 CMBlockBuffer we own.
    let block_buffer = unsafe { CfOwned::from_owned(raw) }
        .ok_or_else(|| DecodeError::Fatal("null CMBlockBuffer".to_string()))?;

    let src = NonNull::new(data.as_ptr().cast_mut().cast::<c_void>())
        .ok_or_else(|| DecodeError::StreamCorrupt("null packet data".to_string()))?;
    // SAFETY: `src` points to `data.len()` readable bytes; the block buffer
    // was created with `data_length == data.len()` so the range is in bounds.
    let status =
        unsafe { CMBlockBuffer::replace_data_bytes(src, block_buffer.get(), 0, data.len()) };
    if status != 0 {
        return Err(DecodeError::Fatal(format!(
            "CMBlockBufferReplaceDataBytes failed: OSStatus {status}"
        )));
    }
    Ok(block_buffer)
}

/// One `VTDecompressionSessionCreate` attempt. `spec` is an optional `+1`
/// decoder-specification dictionary (as built by [`cf_dict_one`]) that is
/// released before returning.
fn try_create_session(
    format_desc: &CMFormatDescription,
    record: &VTDecompressionOutputCallbackRecord,
    spec: Option<NonNull<c_void>>,
) -> Result<CfOwned<VTDecompressionSession>, i32> {
    let mut raw: *mut VTDecompressionSession = std::ptr::null_mut();
    // SAFETY: `format_desc` is a valid CMFormatDescription; `spec` is a valid
    // CFDictionary borrowed for the duration of the call; `record` is a valid
    // callback record whose contents the session copies; `raw` is a valid
    // out-pointer.
    let status = unsafe {
        VTDecompressionSession::create(
            None,
            format_desc,
            spec.map(|dict| &*dict.as_ptr().cast()),
            None,
            core::ptr::from_ref(record),
            NonNull::from(&mut raw),
        )
    };
    if let Some(spec) = spec {
        // SAFETY: `spec` is a +1 CF object we own; balanced release.
        unsafe { CFRelease(spec.as_ptr()) };
    }
    if status != 0 {
        return Err(status);
    }
    // SAFETY: on success `raw` is a +1 VTDecompressionSession we own.
    unsafe { CfOwned::from_owned(raw) }.ok_or(-1)
}

/// Creates a `VTDecompressionSession` honouring `allow_software`.
fn create_session(
    format_desc: &CMFormatDescription,
    state: &Arc<CallbackState>,
    allow_software: bool,
) -> Result<CfOwned<VTDecompressionSession>, MediaError> {
    let record = VTDecompressionOutputCallbackRecord {
        decompressionOutputCallback: Some(decompression_output_callback),
        decompressionOutputRefCon: Arc::as_ptr(state).cast_mut().cast::<c_void>(),
    };

    let fail = |status: i32| {
        MediaError::ImportFailed(format!(
            "VTDecompressionSessionCreate failed: OSStatus {status}"
        ))
    };

    if allow_software {
        // Default specification: VideoToolbox picks a hardware decoder when
        // one is available and may fall back to software automatically.
        match try_create_session(format_desc, &record, None) {
            Ok(session) => return Ok(session),
            Err(first_status) => {
                // Retry with hardware acceleration explicitly disabled —
                // covers cases where the hardware decode path itself is
                // what made session creation fail.
                // SAFETY: the key static is a global CFString and
                // `kCFBooleanFalse` a global CFBoolean.
                let spec = unsafe {
                    cf_dict_one(
                        cf_key_ptr(
                            kVTVideoDecoderSpecification_EnableHardwareAcceleratedVideoDecoder,
                        ),
                        kCFBooleanFalse,
                    )
                };
                let Some(spec) = spec else {
                    return Err(fail(first_status));
                };
                return try_create_session(format_desc, &record, Some(spec)).map_err(|status| {
                    MediaError::ImportFailed(format!(
                        "VTDecompressionSessionCreate failed: OSStatus {status} \
                         (hardware attempt: OSStatus {first_status})"
                    ))
                });
            }
        }
    }

    // Software decode is disallowed: require hardware acceleration so
    // session creation fails instead of silently falling back to software.
    // SAFETY: the key static is a global CFString and `kCFBooleanTrue` a
    // global CFBoolean.
    let spec = unsafe {
        cf_dict_one(
            cf_key_ptr(kVTVideoDecoderSpecification_RequireHardwareAcceleratedVideoDecoder),
            kCFBooleanTrue,
        )
    }
    .ok_or_else(|| {
        MediaError::ImportFailed(
            "failed to build the VideoToolbox decoder specification".to_string(),
        )
    })?;
    try_create_session(format_desc, &record, Some(spec)).map_err(fail)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmtime(value: i64, timescale: i32) -> CMTime {
        CMTime {
            value,
            timescale,
            flags: CMTimeFlags::Valid,
            epoch: 0,
        }
    }

    #[test]
    fn cmtime_converts_to_nanos() {
        // 33.333 ms at a 90 kHz timescale ≈ 33_333_333 ns.
        assert_eq!(cmtime_to_nanos(cmtime(3_000, 90_000)), 33_333_333);
        assert_eq!(cmtime_to_nanos(cmtime(1, 1_000_000_000)), 1);
        assert_eq!(cmtime_to_nanos(cmtime(0, 600)), 0);
    }

    #[test]
    fn cmtime_guards_invalid_values() {
        assert_eq!(cmtime_to_nanos(cmtime(100, 0)), 0);
        assert_eq!(cmtime_to_nanos(cmtime(100, -1)), 0);
        assert_eq!(cmtime_to_nanos(cmtime(-5, 1000)), 0);
        let invalid = CMTime {
            value: 10,
            timescale: 1000,
            flags: CMTimeFlags(0),
            epoch: 0,
        };
        assert_eq!(cmtime_to_nanos(invalid), 0);
        let indefinite = CMTime {
            value: 0,
            timescale: 0,
            flags: CMTimeFlags::Valid | CMTimeFlags::Indefinite,
            epoch: 0,
        };
        assert_eq!(cmtime_to_nanos(indefinite), 0);
    }

    #[test]
    fn codec_mapping() {
        assert_eq!(cm_codec_type(VideoCodec::H264).unwrap(), 0x6176_6331);
        assert_eq!(cm_codec_type(VideoCodec::Hevc).unwrap(), 0x6876_6331);
        assert_eq!(cm_codec_type(VideoCodec::Av1).unwrap(), 0x6176_3031);
        assert!(matches!(
            cm_codec_type(VideoCodec::Vp9),
            Err(DecodeError::UnsupportedCodec(_))
        ));
    }

    #[test]
    fn avcc_parses_sps_pps() {
        let sps = [0x67, 0x64, 0x00, 0x1f];
        let pps = [0x68, 0xee];
        let mut rec = vec![0x01, 0x64, 0x00, 0x1f, 0xff, 0xe1];
        rec.extend_from_slice(&(sps.len() as u16).to_be_bytes());
        rec.extend_from_slice(&sps);
        rec.push(0x01);
        rec.extend_from_slice(&(pps.len() as u16).to_be_bytes());
        rec.extend_from_slice(&pps);

        let (sets, nal_len) = parse_avcc(&rec).unwrap();
        assert_eq!(nal_len, 4);
        assert_eq!(sets, vec![&sps[..], &pps[..]]);
    }

    #[test]
    fn avcc_rejects_truncation() {
        assert!(parse_avcc(&[0x01, 0x64, 0x00]).is_none());
        // num_sps = 1 but no SPS data follows.
        assert!(parse_avcc(&[0x01, 0x64, 0x00, 0x1f, 0xff, 0xe1]).is_none());
    }

    #[test]
    fn hvcc_parses_parameter_sets() {
        let vps = [0x40, 0x01];
        let sps = [0x42, 0x01];
        let pps = [0x44, 0x01];
        let skip = [0x26, 0x01]; // NAL type 38 → not a parameter set.
        let mut rec = vec![0x01];
        rec.extend_from_slice(&[0u8; 20]); // bytes 1..=20 header fields
        rec.push(0xff); // lengthSizeMinusOne = 3 → 4-byte NAL lengths
        rec.push(0x02); // numOfArrays
                        // Array 1: NAL type 33 (SPS), one unit.
        rec.push(0x21);
        rec.extend_from_slice(&1u16.to_be_bytes());
        rec.extend_from_slice(&(sps.len() as u16).to_be_bytes());
        rec.extend_from_slice(&sps);
        // Array 2: NAL type 38 (skipped), one unit.
        rec.push(0x26);
        rec.extend_from_slice(&1u16.to_be_bytes());
        rec.extend_from_slice(&(skip.len() as u16).to_be_bytes());
        rec.extend_from_slice(&skip);

        let (sets, nal_len) = parse_hvcc(&rec).unwrap();
        assert_eq!(nal_len, 4);
        assert_eq!(sets, vec![&sps[..]]);
        let _ = (vps, pps);
    }

    #[test]
    fn pixel_format_mapping() {
        assert_eq!(
            map_pixel_format(kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange),
            (VideoPixelFormat::Nv12, ColorRange::Limited)
        );
        assert_eq!(
            map_pixel_format(kCVPixelFormatType_420YpCbCr10BiPlanarFullRange),
            (VideoPixelFormat::P010, ColorRange::Full)
        );
        assert_eq!(
            map_pixel_format(kCVPixelFormatType_32BGRA),
            (VideoPixelFormat::Rgba8, ColorRange::Full)
        );
        assert_eq!(
            map_pixel_format(0xdead_beef),
            (VideoPixelFormat::Nv12, ColorRange::Limited)
        );
    }

    #[test]
    fn be_helpers() {
        let b = [0x12, 0x34, 0xab, 0xcd, 0xef, 0x01];
        assert_eq!(be_u16(&b, 0), Some(0x1234));
        assert_eq!(be_u16(&b, 5), None);
        assert_eq!(be_u32(&b, 0), Some(0x1234abcd));
        assert_eq!(be_u32(&b, 3), None);
    }

    /// Real-hardware smoke test: creates a live `VTDecompressionSession`.
    /// Ignored by default so headless/virtualized CI is unaffected; run with
    /// `cargo test -p martensite-media-platform --features decoder-videotoolbox -- --ignored`.
    #[test]
    #[ignore = "requires a live VideoToolbox session"]
    fn creates_real_session() {
        let config = DecoderConfig::new(VideoCodec::H264, 64, 64);
        let decoder = VideoToolboxDecoder::create(&config);
        assert!(decoder.is_ok());
        let decoder = decoder.unwrap();
        assert_eq!(decoder.negotiated_format(), VideoPixelFormat::Nv12);
        assert_eq!(decoder.stats().backend, Some(DecoderBackend::VideoToolbox));
    }

    /// `send_packet` gate: a delta packet before any keyframe must be
    /// rejected, and an empty packet is reported as stream-corrupt. Exercises
    /// the pure logic without decoding.
    #[test]
    #[ignore = "requires a live VideoToolbox session"]
    fn enforces_keyframe_gate() {
        let config = DecoderConfig::new(VideoCodec::H264, 64, 64);
        let mut decoder = VideoToolboxDecoder::create(&config).unwrap();
        let delta = EncodedPacket::new(vec![1, 2, 3], 0, 0).delta();
        assert!(matches!(
            decoder.send_packet(&delta),
            Err(MediaError::InvalidHandle) // DecodeError::NeedsKeyframe → InvalidHandle
        ));
        assert_eq!(decoder.stats().packets_rejected, 1);
        let empty = EncodedPacket::new(vec![], 0, 0);
        assert!(decoder.send_packet(&empty).is_err());
        assert_eq!(decoder.stats().packets_rejected, 2);
    }
}
