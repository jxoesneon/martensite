//! `decoder-ffmpeg` real-decode conformance test.
//!
//! Feeds a checked-in 320x240@30fps baseline H.264 Annex-B stream
//! (`fixtures/test-320x240.h264`, 30 frames, ~10 KB) through `FfmpegDecoder`
//! and validates the decoded `CpuMemory` frames end-to-end — the same path
//! `MediaView` exercises through `dyn VideoDecoder`.
//!
//! Runs wherever the system FFmpeg libraries are installed (CI installs the
//! `-dev` packages on Ubuntu); it is compiled only under
//! `--features decoder-ffmpeg`.

#![forbid(unsafe_code)]
#![cfg(feature = "decoder-ffmpeg")]

mod common;

use common::{annex_b_access_units, FRAME_NS, STREAM_320X240 as STREAM};
use martensite_media::decoder::ffmpeg::FfmpegDecoder;
use martensite_media::decoder::{DecoderConfig, EncodedPacket, VideoCodec, VideoDecoder};
use martensite_media::queue::{FrameQueue, QueueAction};
use martensite_media::surface::{HardwareHandle, VideoPixelFormat};

#[test]
fn annex_b_splitter_produces_30_aus() {
    assert_eq!(annex_b_access_units(STREAM).len(), 30);
}

#[test]
fn ffmpeg_decoder_decodes_real_h264_stream() {
    let mut dec: FfmpegDecoder =
        VideoDecoder::init(DecoderConfig::new(VideoCodec::H264, 320, 240)).unwrap();

    // Delta-before-keyframe gate is enforced even for real streams.
    assert!(dec
        .send_packet(&EncodedPacket::new(STREAM[..64].to_vec(), 0, 0).delta())
        .is_err());

    let aus = annex_b_access_units(STREAM);
    assert_eq!(aus.len(), 30);
    for (i, au) in aus.iter().enumerate() {
        dec.send_packet(&EncodedPacket::new(
            au.to_vec(),
            i as u64 * FRAME_NS,
            FRAME_NS,
        ))
        .unwrap_or_else(|e| panic!("AU {i} ({} bytes) rejected: {e:?}", au.len()));
    }
    dec.end_of_stream().unwrap();

    let mut frames = Vec::new();
    while let Some(f) = dec.try_recv_frame().unwrap() {
        frames.push(f);
    }
    assert_eq!(frames.len(), 30, "fixture encodes 30 frames");
    assert_eq!(dec.negotiated_format(), VideoPixelFormat::Nv12);
    assert!(!dec.stats().hardware_accelerated());

    let mut last_pts = None;
    for frame in &frames {
        assert_eq!(frame.metadata.width, 320);
        assert_eq!(frame.metadata.height, 240);
        if let Some(prev) = last_pts {
            assert!(frame.metadata.pts_nanos >= prev, "presentation order");
        }
        last_pts = Some(frame.metadata.pts_nanos);
        let HardwareHandle::CpuMemory {
            y_plane,
            uv_plane,
            y_stride,
            uv_stride,
        } = &frame.handle
        else {
            panic!("software decoder must emit CpuMemory handles");
        };
        assert!(*y_stride >= 320 && *uv_stride >= 320);
        assert!(y_plane.len() >= 320 * 240);
        assert!(uv_plane.len() >= 320 * 120);
    }
    assert_eq!(dec.stats().frames_decoded, 30);

    // The EOF drain is idempotent and rejects late input.
    dec.end_of_stream().unwrap();
    assert!(dec
        .send_packet(&EncodedPacket::new(vec![0x65], 0, 0))
        .is_err());
}

#[test]
fn ffmpeg_decoder_frames_drive_frame_queue() {
    let mut dec: FfmpegDecoder =
        VideoDecoder::init(DecoderConfig::new(VideoCodec::H264, 320, 240)).unwrap();
    for (i, au) in annex_b_access_units(STREAM).iter().enumerate() {
        dec.send_packet(&EncodedPacket::new(
            au.to_vec(),
            i as u64 * FRAME_NS,
            FRAME_NS,
        ))
        .unwrap();
    }
    dec.end_of_stream().unwrap();

    let mut queue = FrameQueue::new(3);
    while let Some(f) = dec.try_recv_frame().unwrap() {
        queue.push(f);
    }
    assert!(queue.len() <= 3);
    assert_eq!(queue.presented(), 0);
    assert!(queue.dropped() > 0, "capacity pressure must be counted");

    let mut presented = 0usize;
    while let QueueAction::Present(_) = queue.pop_present(29 * FRAME_NS) {
        presented += 1;
    }
    assert!(presented > 0);
    assert_eq!(presented as u64, queue.presented());
}
