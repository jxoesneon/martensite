//! `decoder-videotoolbox` real-decode conformance test (macOS only).
//!
//! Feeds the checked-in 320x240@30fps H.264 stream through
//! `VideoToolboxDecoder` on the host's real hardware decoder and validates
//! the emitted `IoSurface` frames end-to-end — the same path `MediaView`
//! exercises through `dyn VideoDecoder`.
//!
//! VideoToolbox requires `avcC` (length-prefixed) samples, so the Annex-B
//! fixture is converted per access unit and the extracted SPS/PPS are
//! passed as `codec_config` — mirroring how an MP4 demuxer drives the
//! decoder.
//!
//! Runs on every macOS CI runner (VideoToolbox is part of the OS); it is
//! compiled only under `--features decoder-videotoolbox` on macOS.

#![forbid(unsafe_code)]
#![cfg(all(feature = "decoder-videotoolbox", target_os = "macos"))]

mod common;

use common::{
    annex_b_access_units, au_to_avcc, avcc_record, ivf_frames, samples_dir, FRAME_NS,
    STREAM_320X240,
};
use martensite_media::decoder::videotoolbox::VideoToolboxDecoder;
use martensite_media::decoder::{DecoderConfig, EncodedPacket, VideoCodec, VideoDecoder};
use martensite_media::surface::{HardwareHandle, VideoPixelFormat};

#[test]
fn videotoolbox_decoder_decodes_real_h264_stream() {
    let avcc = avcc_record(STREAM_320X240).expect("fixture must contain SPS");
    let mut dec: VideoToolboxDecoder =
        VideoDecoder::init(DecoderConfig::new(VideoCodec::H264, 320, 240).with_codec_config(avcc))
            .unwrap();

    let aus = annex_b_access_units(STREAM_320X240);
    assert_eq!(aus.len(), 30);

    // The decoder is asynchronous and its output queue is bounded by
    // `decode_ahead` (3); a real consumer drains continuously, so poll for
    // each AU's frame before submitting the next. The fixture is baseline
    // profile (no B-frames) — one decoded frame per access unit.
    let mut frames = Vec::new();
    for (i, au) in aus.iter().enumerate() {
        dec.send_packet(&EncodedPacket::new(
            au_to_avcc(au),
            i as u64 * FRAME_NS,
            FRAME_NS,
        ))
        .unwrap_or_else(|e| panic!("AU {i} rejected: {e:?}"));

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            match dec.try_recv_frame().unwrap() {
                Some(f) => {
                    frames.push(f);
                    break;
                }
                None if std::time::Instant::now() < deadline => {
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                None => panic!("AU {i} produced no frame within 5s"),
            }
        }
    }
    dec.end_of_stream().unwrap();
    while let Some(f) = dec.try_recv_frame().unwrap() {
        frames.push(f);
    }
    assert_eq!(frames.len(), 30, "fixture encodes 30 frames");
    assert_eq!(dec.negotiated_format(), VideoPixelFormat::Nv12);

    let mut last_pts = None;
    for frame in &frames {
        assert_eq!(frame.metadata.width, 320);
        assert_eq!(frame.metadata.height, 240);
        if let Some(prev) = last_pts {
            assert!(frame.metadata.pts_nanos >= prev, "presentation order");
        }
        last_pts = Some(frame.metadata.pts_nanos);
        assert!(
            matches!(frame.handle, HardwareHandle::IoSurface { .. }),
            "VideoToolbox must emit zero-copy IoSurface handles"
        );
    }
    assert_eq!(dec.stats().frames_decoded, 30);
    assert!(dec.stats().hardware_accelerated());
}

/// Number of IVF temporal units fed to the AV1 decoder — the 4K120 sample
/// is large, so only a bounded prefix is decoded.
const AV1_UNITS: usize = 30;

/// The `av1C` record of `av1-4k120.bin` (marker/version, profile 0,
/// level_idx 14, 8-bit 4:2:0 + the sequence-header OBU in low-overhead
/// form). Matches what the decoder's deferred path synthesizes in-band.
const AV1C_4K120: &[u8] = &[
    0x81, 0x0e, 0x0c, 0x00, 0x0a, 0x0c, 0x00, 0x00, 0x00, 0x72, 0xef, 0xbf, 0xe1, 0xbc, 0x6a, 0xf9,
    0x00, 0x40,
];

/// Loads the first [`AV1_UNITS`] temporal units of the IVF sample; `None`
/// (with a skip line) when the gitignored sample asset is absent.
fn av1_temporal_units() -> Option<Vec<Vec<u8>>> {
    let path = samples_dir().join("av1-4k120.bin");
    let data = match std::fs::read(&path) {
        Ok(data) => data,
        Err(e) => {
            eprintln!("av1: {} unreadable ({e}); test skipped", path.display());
            return None;
        }
    };
    let units = ivf_frames(&data, b"AV01").expect("av1-4k120.bin is an AV1 IVF stream");
    Some(units[..AV1_UNITS.min(units.len())].to_vec())
}

/// Feeds `units` through a decoder built from `config`, draining the
/// asynchronous output after each send and once more after end-of-stream.
/// Returns the decoder (for stats) and the decoded frames, or `None` when
/// the host has no AV1 hardware decoder (init then fails by design).
fn feed_av1(
    config: DecoderConfig,
    units: &[Vec<u8>],
) -> Option<(
    VideoToolboxDecoder,
    Vec<martensite_media::decoder::DecodedFrame>,
)> {
    // Deferred init (`codec_config` absent) creates the session lazily at
    // the first temporal unit, so a missing hardware decoder surfaces as a
    // send error on unit 0 rather than an init error.
    let deferred_init = config.codec_config.is_none();
    let mut config = config;
    // Require the hardware decoder: `hardware_accelerated()` is derived
    // from the backend tag, so a silently-created VT software decoder would
    // still report true. With `allow_software` unset the spec uses
    // `RequireHardwareAcceleratedVideoDecoder` and init fails instead.
    config.allow_software = false;
    // A roomy output queue keeps the bounded `decode_ahead` buffer from
    // evicting frames mid-run, so the decoded-frame count is deterministic.
    config.decode_ahead = 16;
    let mut dec = match VideoToolboxDecoder::init(config) {
        Ok(dec) => dec,
        Err(e) => {
            eprintln!("av1: no usable hardware decoder ({e}); test skipped");
            return None;
        }
    };

    const AV1_FRAME_NS: u64 = 1_000_000_000 / 120;
    let mut frames = Vec::new();
    for (i, unit) in units.iter().enumerate() {
        let packet = EncodedPacket::new(unit.clone(), i as u64 * AV1_FRAME_NS, AV1_FRAME_NS);
        let packet = if i == 0 { packet } else { packet.delta() };
        if let Err(e) = dec.send_packet(&packet) {
            if deferred_init && i == 0 {
                eprintln!(
                    "av1: deferred session creation failed ({e}); \
                     no usable AV1 hardware decoder — test skipped"
                );
                return None;
            }
            panic!("temporal unit {i} rejected: {e:?}");
        }
        loop {
            match dec.try_recv_frame() {
                Ok(Some(f)) => frames.push(f),
                Ok(None) => break,
                Err(e) => panic!("temporal unit {i}: fatal decode error: {e:?}"),
            }
        }
    }
    dec.end_of_stream().unwrap();
    loop {
        match dec.try_recv_frame() {
            Ok(Some(f)) => frames.push(f),
            Ok(None) => break,
            Err(e) => panic!("end-of-stream drain hit a fatal decode error: {e:?}"),
        }
    }
    Some((dec, frames))
}

/// Validates decoded AV1 output: a real decoded-frame count, 4K geometry,
/// IoSurface handles, a plausible pixel format, and a hardware-accelerated
/// backend.
fn assert_av1_output(
    dec: &VideoToolboxDecoder,
    frames: &[martensite_media::decoder::DecodedFrame],
    units: &[Vec<u8>],
) {
    // Every temporal unit of this sample is displayable: measured 30/30
    // frames on the M4 host with an oversized `decode_ahead` queue. The
    // ≥27 bound (not `!is_empty`) asserts a real decode happened while
    // tolerating a couple of decoder-internal drops, which surface via
    // `packets_rejected` rather than silently shrinking the count.
    assert!(
        frames.len() >= 27,
        "expected ≥27 of {} temporal units to produce frames, got {}",
        units.len(),
        frames.len()
    );
    for frame in frames {
        assert_eq!(frame.metadata.width, 3840);
        assert_eq!(frame.metadata.height, 2160);
        assert!(
            matches!(frame.handle, HardwareHandle::IoSurface { .. }),
            "VideoToolbox must emit zero-copy IoSurface handles"
        );
    }
    assert!(
        matches!(
            dec.negotiated_format(),
            VideoPixelFormat::Nv12 | VideoPixelFormat::P010
        ),
        "unexpected AV1 output format {:?}",
        dec.negotiated_format()
    );
    assert!(dec.stats().hardware_accelerated());
}

/// AV1 through the `av1C` extension-atom bridge, deferred-init path.
///
/// The IVF sample carries no `av1C` extradata, so `VideoToolboxDecoder`
/// defers session creation until the first temporal unit's sequence-header
/// OBU, synthesizes the `AV1CodecConfigurationRecord` from it, and builds
/// the `av01` format description through
/// `kCMFormatDescriptionExtension_SampleDescriptionExtensionAtoms`. Every
/// IVF temporal unit is submitted verbatim as an `av01` sample
/// (low-overhead OBU format — no Annex-B style reframing exists for AV1).
#[test]
fn videotoolbox_decoder_decodes_real_av1_stream() {
    let Some(units) = av1_temporal_units() else {
        return;
    };
    let Some((dec, frames)) = feed_av1(DecoderConfig::new(VideoCodec::Av1, 3840, 2160), &units)
    else {
        return;
    };
    assert_av1_output(&dec, &frames, &units);
    eprintln!(
        "av1 (deferred init): {} temporal units -> {} decoded frames, \
         format={:?} rejected={}",
        units.len(),
        frames.len(),
        dec.negotiated_format(),
        dec.stats().packets_rejected,
    );
}

/// AV1 with `av1C` extradata supplied out-of-band: the session is created
/// eagerly in `init`, exactly as an MP4 demuxer would drive it.
#[test]
fn videotoolbox_decoder_decodes_av1_with_av1c_extradata() {
    let Some(units) = av1_temporal_units() else {
        return;
    };
    let config =
        DecoderConfig::new(VideoCodec::Av1, 3840, 2160).with_codec_config(AV1C_4K120.to_vec());
    let Some((dec, frames)) = feed_av1(config, &units) else {
        return;
    };
    assert_av1_output(&dec, &frames, &units);
    eprintln!(
        "av1 (av1C extradata): {} temporal units -> {} decoded frames",
        units.len(),
        frames.len()
    );
}
