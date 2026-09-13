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

use common::{annex_b_access_units, au_to_avcc, avcc_record, FRAME_NS, STREAM_320X240};
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
