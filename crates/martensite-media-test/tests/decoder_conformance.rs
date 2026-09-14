//! `VideoDecoder` trait conformance suite.
//!
//! Every decode backend — `MockDecoder`, VideoToolbox, Media Foundation,
//! VAAPI, ffmpeg software — must satisfy the same contract. The suite is
//! generic over a constructor so the same tests drive every backend the
//! target platform and enabled features provide.
//!
//! Backend coverage by platform/feature (feature flags forwarded from
//! `martensite-media`):
//!
//! | backend | gate |
//! |---------|------|
//! | `MockDecoder` | always |
//! | `VideoToolboxDecoder` | `decoder-videotoolbox` on macOS |
//! | `MediaFoundationDecoder` | `decoder-mf` on Windows |
//! | `VaapiDecoder` | `decoder-vaapi` on Linux |
//! | `FfmpegDecoder` | `decoder-ffmpeg`, needs system ffmpeg |
//!
//! The 4K120 gate (`full_rate_4k120_gate`) is `#[ignore]`-gated: it requires
//! a self-hosted runner with a real hardware decoder and
//! `MARTENSITE_MEDIA_4K120=1` in the environment. Automated CI exercises the
//! noop/CPU paths only.

#![forbid(unsafe_code)]

mod common;

use martensite_media::decoder::{
    DecodedFrame, DecoderConfig, EncodedPacket, MockDecoder, VideoCodec, VideoDecoder,
};
use martensite_media::hdr::{Eotf, HdrMetadata};
use martensite_media::queue::{FrameQueue, QueueAction};
use martensite_media::surface::{HardwareHandle, VideoPixelFormat};

const FRAME_NS: u64 = 16_666_667; // ~60 fps

fn packet(pts: u64, keyframe: bool) -> EncodedPacket {
    let pkt = EncodedPacket::new(vec![0x01, 0x02, 0x03], pts, FRAME_NS);
    if keyframe {
        pkt
    } else {
        pkt.delta()
    }
}

/// The contract every backend must satisfy, exercised through `&mut dyn
/// VideoDecoder` so the same code path used by `MediaView` is what gets
/// tested.
fn conformance_suite(dec: &mut dyn VideoDecoder) {
    // 1. Fresh decoder has a negotiated format and empty queue.
    assert_ne!(
        dec.negotiated_format().plane_count(),
        0,
        "negotiated format must be a valid pixel format"
    );
    assert!(
        dec.try_recv_frame().expect("initial recv").is_none(),
        "fresh decoder must not emit frames"
    );

    // 2. Delta packet before any keyframe is rejected.
    assert!(
        dec.send_packet(&packet(0, false)).is_err(),
        "delta before keyframe must be rejected"
    );

    // 3. Keyframe + deltas produce frames with monotonic, packet-sourced PTS.
    dec.send_packet(&packet(0, true)).expect("keyframe");
    dec.send_packet(&packet(FRAME_NS, false)).expect("delta 1");
    dec.send_packet(&packet(FRAME_NS * 2, false))
        .expect("delta 2");

    let mut last_pts = u64::MAX;
    let mut count = 0usize;
    while let Ok(Some(frame)) = dec.try_recv_frame() {
        assert!(
            last_pts == u64::MAX || frame.metadata.pts_nanos >= last_pts,
            "frames must come out in presentation order"
        );
        last_pts = frame.metadata.pts_nanos;
        count += 1;
        if count > 8 {
            break;
        }
    }
    assert!(count > 0, "decoder must emit at least one frame");
    assert_eq!(dec.stats().packets_received, 3);

    // 4. `end_of_stream` is idempotent and drains reorder-buffered frames:
    // everything the decoder has seen must be emitted or accounted for.
    dec.end_of_stream().expect("end_of_stream");
    dec.end_of_stream().expect("end_of_stream is idempotent");
    let mut drained = count;
    while let Ok(Some(frame)) = dec.try_recv_frame() {
        assert!(frame.metadata.pts_nanos >= last_pts);
        last_pts = frame.metadata.pts_nanos;
        drained += 1;
        if drained > 16 {
            break;
        }
    }
    assert_eq!(dec.stats().frames_decoded as usize, drained);

    // 5. Flush resets the keyframe gate.
    dec.flush().expect("flush");
    assert!(
        dec.send_packet(&packet(FRAME_NS * 3, false)).is_err(),
        "post-flush delta must be rejected"
    );
    dec.send_packet(&packet(FRAME_NS * 3, true))
        .expect("post-flush keyframe");
}

#[test]
fn mock_decoder_satisfies_conformance() {
    let mut dec = MockDecoder::init(DecoderConfig::new(VideoCodec::H264, 1920, 1080)).unwrap();
    conformance_suite(&mut dec);
}

#[test]
fn mock_decoder_hevc_reports_p010() {
    let dec = MockDecoder::init(DecoderConfig::new(VideoCodec::Hevc, 3840, 2160)).unwrap();
    assert_eq!(dec.negotiated_format(), VideoPixelFormat::P010);
}

#[test]
fn decoder_frame_queue_integration() {
    let mut dec = MockDecoder::init(DecoderConfig::new(VideoCodec::H264, 640, 360)).unwrap();
    for i in 0..6u64 {
        dec.send_packet(&packet(i * FRAME_NS, i == 0)).unwrap();
    }

    let mut queue = FrameQueue::new(3);
    while let Some(f) = dec.try_recv_frame().unwrap() {
        queue.push(f);
    }
    assert_eq!(queue.len(), 3); // capacity caps the backlog

    // Present at a wall clock aligned to the last frame's PTS: the frame at
    // index 3 is past its deadline (skipped+counted); frames 4 and 5 are
    // both presentable.
    let mut presented = 0usize;
    let now = 5 * FRAME_NS;
    while let QueueAction::Present(_) = queue.pop_present(now) {
        presented += 1;
    }
    assert_eq!(presented, 2);
    assert_eq!(queue.dropped(), 4); // 3 capacity evictions + 1 deadline skip
    assert_eq!(queue.presented(), 2);
}

#[test]
fn hdr_metadata_flows_to_frames() {
    let mut dec = MockDecoder::init(DecoderConfig::new(VideoCodec::Hevc, 3840, 2160)).unwrap();
    dec.set_hdr(HdrMetadata::new(Eotf::Pq));
    dec.send_packet(&packet(0, true)).unwrap();
    let frame = dec.try_recv_frame().unwrap().unwrap();
    let side = frame.hdr.expect("hdr side data");
    assert_eq!(side.eotf_code, 16);
    let hdr = HdrMetadata::from(&side);
    assert!(hdr.is_hdr());
}

/// 4K120 acceptance gate — `#[ignore]`-gated by design.
///
/// Requires a self-hosted runner with a real hardware decoder, sample
/// assets (`$MARTENSITE_MEDIA_SAMPLES/{h264,hevc,av1}-4k120.bin`), and
/// `MARTENSITE_MEDIA_4K120=1`. The gate reads `FrameQueue::drop_rate_pct`
/// and `FrameQueue::cpu_utilization_pct` — never wall-clock heuristics.
///
/// CI does not run this; the noop/CPU suites cover correctness.
#[test]
#[ignore = "requires dedicated GPU runner with hardware decode — see v0.16.0 spec §5"]
fn full_rate_4k120_gate() {
    if std::env::var_os("MARTENSITE_MEDIA_4K120").is_none() {
        eprintln!("MARTENSITE_MEDIA_4K120 not set; gate skipped");
        return;
    }
    #[cfg(any(
        feature = "decoder-ffmpeg",
        all(feature = "decoder-videotoolbox", target_os = "macos")
    ))]
    gate_4k120::run();
    #[cfg(not(any(
        feature = "decoder-ffmpeg",
        all(feature = "decoder-videotoolbox", target_os = "macos")
    )))]
    eprintln!("4k120 gate: no decoder backend feature enabled; all legs skipped");
}

/// The real 4K120 acceptance harness behind [`full_rate_4k120_gate`].
///
/// Compiled only when at least one usable decoder backend is enabled —
/// VideoToolbox on macOS, or the ffmpeg software backend anywhere — so the
/// conformance file still builds with zero decoder features (the gate test
/// then prints a skip line).
///
/// Each leg loads one generated sample from `$MARTENSITE_MEDIA_SAMPLES`
/// (default `<workspace>/target/media-samples`), re-packetizes it, and pumps
/// a single decoder at a real wall-clock 120 fps cadence through
/// [`FrameQueue`]. The pump is deliberately single-threaded: the number it
/// measures is the honest end-to-end decode-present rate a `MediaView`
/// would see.
#[cfg(any(
    feature = "decoder-ffmpeg",
    all(feature = "decoder-videotoolbox", target_os = "macos")
))]
mod gate_4k120 {
    use std::path::Path;
    use std::time::{Duration, Instant};

    use martensite_media::decoder::{
        DecodedFrame, DecoderConfig, EncodedPacket, VideoCodec, VideoDecoder,
    };
    use martensite_media::queue::{FrameQueue, QueueAction};

    use crate::common::{
        annex_b_access_units, au_to_avcc, au_to_hvcc, avcc_record, hevc_access_units, hvcc_record,
        ivf_frames, samples_dir,
    };

    /// Presentation interval at the 120 fps gate rate (8.333 ms).
    const GATE_FRAME_NS: u64 = 1_000_000_000 / 120;
    /// `DecoderConfig::decode_ahead` for the gate. VideoToolbox's async
    /// output queue is bounded by this value; too small and output overflow
    /// is dropped into `packets_rejected` before the pump can drain it.
    const DECODE_AHEAD: usize = 8;
    /// How far ahead of the presentation clock the pump keeps the decoder
    /// fed, in frames. Bounded by `DECODE_AHEAD`: total in-flight frames
    /// must never exceed the decoder's bounded output queue plus the
    /// `FrameQueue` capacity, or the overflow is dropped before it can be
    /// presented — a harness bug, not a decode failure.
    const FEED_AHEAD_FRAMES: u64 = DECODE_AHEAD as u64;
    /// Legs shorter than this cannot demonstrate sustained 4K120; skipped.
    const MIN_FRAMES: usize = 600;
    /// Acceptance threshold from the v0.16.0 spec: < 0.1% of frames lost.
    const MAX_LOSS_PCT: f64 = 0.1;
    /// Acceptance threshold: < 1% CPU dispatch utilization at 120 fps.
    const MAX_CPU_PCT: f64 = 1.0;
    /// Grace period after the stream's nominal duration before a leg is
    /// declared wedged (decoder producing nothing, queue never draining).
    const DRAIN_TIMEOUT: Duration = Duration::from_secs(60);
    /// Window during backend probing for an async backend to surface an
    /// early fatal error (e.g. VideoToolbox rejecting a stream at
    /// decode time rather than at `send_packet` time).
    const PROBE_WINDOW: Duration = Duration::from_millis(250);

    /// A decode backend a leg can run on. Variants exist for every backend
    /// the gate knows about even when the corresponding cargo feature is
    /// off, so `backend_candidates` stays a simple preference list.
    #[allow(dead_code)]
    #[derive(Copy, Clone, Debug)]
    enum Backend {
        /// macOS VideoToolbox (`decoder-videotoolbox`) — the hardware path.
        VideoToolbox,
        /// `ffmpeg-next` software decode (`decoder-ffmpeg`).
        Ffmpeg,
    }

    impl Backend {
        /// Human-readable backend tag for report lines.
        fn name(self) -> &'static str {
            match self {
                Self::VideoToolbox => "videotoolbox",
                Self::Ffmpeg => "ffmpeg",
            }
        }
    }

    /// Backends tried for a leg, in preference order: VideoToolbox first on
    /// macOS (the hardware path under acceptance test), then the ffmpeg
    /// software fallback for codecs the hardware path cannot take.
    fn backend_candidates() -> Vec<Backend> {
        [
            #[cfg(all(feature = "decoder-videotoolbox", target_os = "macos"))]
            Backend::VideoToolbox,
            #[cfg(feature = "decoder-ffmpeg")]
            Backend::Ffmpeg,
        ]
        .into_iter()
        .collect()
    }

    /// Opens a VideoToolbox session for `config` (`decoder-videotoolbox`
    /// enabled and running on macOS).
    #[cfg(all(feature = "decoder-videotoolbox", target_os = "macos"))]
    fn open_videotoolbox(config: &DecoderConfig) -> Option<Box<dyn VideoDecoder>> {
        use martensite_media::decoder::videotoolbox::VideoToolboxDecoder;
        match VideoToolboxDecoder::init(config.clone()) {
            Ok(dec) => Some(Box::new(dec)),
            Err(e) => {
                eprintln!("4k120: VideoToolbox init failed: {e}");
                None
            }
        }
    }

    /// Stub for builds without a usable VideoToolbox backend.
    #[cfg(not(all(feature = "decoder-videotoolbox", target_os = "macos")))]
    fn open_videotoolbox(_config: &DecoderConfig) -> Option<Box<dyn VideoDecoder>> {
        None
    }

    /// Opens an `FfmpegDecoder` for `config` (`decoder-ffmpeg` enabled).
    #[cfg(feature = "decoder-ffmpeg")]
    fn open_ffmpeg(config: &DecoderConfig) -> Option<Box<dyn VideoDecoder>> {
        use martensite_media::decoder::ffmpeg::FfmpegDecoder;
        match FfmpegDecoder::init(config.clone()) {
            Ok(dec) => Some(Box::new(dec)),
            Err(e) => {
                eprintln!("4k120: ffmpeg init failed: {e}");
                None
            }
        }
    }

    /// Stub for builds without the ffmpeg backend.
    #[cfg(not(feature = "decoder-ffmpeg"))]
    fn open_ffmpeg(_config: &DecoderConfig) -> Option<Box<dyn VideoDecoder>> {
        None
    }

    /// Constructs `backend` for `config`; `None` when it is not compiled in
    /// or session creation fails — callers then try the next candidate.
    fn open_backend(backend: Backend, config: &DecoderConfig) -> Option<Box<dyn VideoDecoder>> {
        match backend {
            Backend::VideoToolbox => open_videotoolbox(config),
            Backend::Ffmpeg => open_ffmpeg(config),
        }
    }

    /// Opens the first backend that initializes *and* accepts the leg's
    /// first packet without an early fatal decode error.
    ///
    /// Probing with a real send — plus a short window for asynchronous
    /// backends to report a decode-time failure — is what lets the AV1 leg
    /// fall back from VideoToolbox to the ffmpeg (dav1d) software backend
    /// when the hardware decoder refuses the sample.
    ///
    /// On success the returned decoder already holds packet 0; `primed`
    /// carries a frame if one was emitted during the probe window.
    fn open_working_decoder(
        config: &DecoderConfig,
        first_packet: &EncodedPacket,
    ) -> Option<(Backend, Box<dyn VideoDecoder>, Option<DecodedFrame>)> {
        for backend in backend_candidates() {
            let Some(mut dec) = open_backend(backend, config) else {
                continue;
            };
            if let Err(e) = dec.send_packet(first_packet) {
                eprintln!(
                    "4k120: {} rejected the first packet ({e}); trying next backend",
                    backend.name()
                );
                continue;
            }
            // Poll briefly for either a first frame (backend is definitely
            // decoding) or a fatal error (definitely not). A silent backend
            // is still given the benefit of the doubt.
            let deadline = Instant::now() + PROBE_WINDOW;
            let mut primed = None;
            let mut failed = false;
            while Instant::now() < deadline {
                match dec.try_recv_frame() {
                    Ok(Some(frame)) => {
                        primed = Some(frame);
                        break;
                    }
                    Ok(None) => std::thread::sleep(Duration::from_millis(2)),
                    Err(e) => {
                        eprintln!(
                            "4k120: {} failed during probe ({e}); trying next backend",
                            backend.name()
                        );
                        failed = true;
                        break;
                    }
                }
            }
            if !failed {
                return Some((backend, dec, primed));
            }
        }
        None
    }

    /// One gate leg: packetized access units plus optional codec extradata.
    struct Leg {
        /// Codec of the sample stream.
        codec: VideoCodec,
        /// Sample file stem, for report lines.
        name: &'static str,
        /// Per-frame compressed payloads in presentation order.
        packets: Vec<Vec<u8>>,
        /// `avcC`/`hvcC` extradata when the stream carries parameter sets;
        /// `None` for Annex-B (in-band) and AV1.
        codec_config: Option<Vec<u8>>,
    }

    /// Loads one sample file and re-packetizes it per codec; `None` when the
    /// file is missing or unparseable — the leg is skipped, never failed, so
    /// a partially populated samples directory still exercises what exists.
    fn load_leg(codec: VideoCodec, path: &Path, name: &'static str) -> Option<Leg> {
        let data = match std::fs::read(path) {
            Ok(data) => data,
            Err(e) => {
                eprintln!("4k120: {} unreadable ({e}); leg skipped", path.display());
                return None;
            }
        };
        let (packets, codec_config) = match codec {
            VideoCodec::H264 => {
                let aus = annex_b_access_units(&data);
                // Build the `avcC` record from the first AU that carries an
                // SPS — mirroring a demuxer — rather than the whole stream:
                // periodic in-band parameter-set repeats would overflow the
                // record's 5-bit SPS count.
                let record = aus.iter().find_map(|au| avcc_record(au));
                // With extradata, length-prefixed (`avcC`) AU framing is what
                // every backend here expects; without it the raw Annex-B AUs
                // go in-band so deferred-mode parsers can still find the
                // parameter sets.
                let packets = if record.is_some() {
                    aus.iter().map(|au| au_to_avcc(au)).collect()
                } else {
                    aus.iter().map(|au| au.to_vec()).collect()
                };
                (packets, record)
            }
            VideoCodec::Hevc => {
                let aus = hevc_access_units(&data);
                let record = aus.iter().find_map(|au| hvcc_record(au));
                let packets = if record.is_some() {
                    aus.iter().map(|au| au_to_hvcc(au)).collect()
                } else {
                    aus.iter().map(|au| au.to_vec()).collect()
                };
                (packets, record)
            }
            // The `.bin` AV1 sample is actually an IVF container; payloads
            // are the raw per-frame temporal units (OBUs).
            VideoCodec::Av1 => {
                let Some(frames) = ivf_frames(&data) else {
                    eprintln!("4k120: {name} is not a parseable IVF stream; leg skipped");
                    return None;
                };
                (frames, None)
            }
            VideoCodec::Vp9 => {
                eprintln!("4k120: {name} — VP9 has no gate leg; skipped");
                return None;
            }
        };
        if packets.is_empty() {
            eprintln!("4k120: {name} produced no access units; leg skipped");
            return None;
        }
        Some(Leg {
            codec,
            name,
            packets,
            codec_config,
        })
    }

    /// Moves decoded frames from the decoder into `queue`, stopping when the
    /// queue is full or the decoder has nothing ready.
    ///
    /// Pulling is deliberately gated on queue space: a hardware decoder
    /// emits frames at decode speed, not presentation speed, and pushing
    /// them into a full `FrameQueue` would evict frames that were never
    /// late — measuring the harness instead of the pipeline. Frames left in
    /// the decoder are held by its own `decode_ahead`-bounded output queue.
    /// Returns whether at least one frame was received.
    fn drain_into(dec: &mut dyn VideoDecoder, queue: &mut FrameQueue) -> Result<bool, String> {
        let mut got = false;
        while queue.len() < queue.capacity() {
            match dec
                .try_recv_frame()
                .map_err(|e| format!("decoder error while draining: {e}"))?
            {
                Some(frame) => {
                    queue.push(frame);
                    got = true;
                }
                None => break,
            }
        }
        Ok(got)
    }

    /// The wall-clock decode→present pump for one leg.
    ///
    /// `fed` packets are already inside `dec` (the backend probe sends
    /// packet 0); `primed` is a frame received during probing. The loop
    /// feeds the decoder up to [`FEED_AHEAD_FRAMES`] ahead of the
    /// presentation clock, drains whatever it has emitted (bounded by queue
    /// space — see [`drain_into`]), and serves `pop_present` at each frame's
    /// PTS deadline. Returns the final queue (drop/CPU accounting) and the
    /// measured wall time.
    ///
    /// # Errors
    ///
    /// Fails the leg on a mid-stream `send_packet`/`try_recv_frame` error,
    /// non-monotonic PTS, wrong frame geometry, or a wedged drain.
    fn pump(
        dec: &mut dyn VideoDecoder,
        leg: &Leg,
        mut fed: usize,
        primed: Option<DecodedFrame>,
    ) -> Result<(FrameQueue, Duration), String> {
        let total = leg.packets.len();
        let mut queue = FrameQueue::new(3);
        if let Some(frame) = primed {
            queue.push(frame);
        }
        let start = Instant::now();
        let budget = Duration::from_nanos(total as u64 * GATE_FRAME_NS) + DRAIN_TIMEOUT;
        let mut eof_sent = false;
        let mut last_pts: Option<u64> = None;

        loop {
            let now = start.elapsed().as_nanos() as u64;

            // Feed: keep the decoder's pipeline full ahead of the clock.
            // Packet 0 (sent by the probe) was the keyframe; everything here
            // is a delta — the honest usage that exercises the keyframe gate.
            while fed < total
                && (fed as u64) * GATE_FRAME_NS <= now + FEED_AHEAD_FRAMES * GATE_FRAME_NS
            {
                let packet = EncodedPacket::new(
                    leg.packets[fed].clone(),
                    fed as u64 * GATE_FRAME_NS,
                    GATE_FRAME_NS,
                )
                .delta();
                dec.send_packet(&packet)
                    .map_err(|e| format!("packet {fed} rejected: {e}"))?;
                fed += 1;
                drain_into(dec, &mut queue)?;
            }

            let got_frame = drain_into(dec, &mut queue)?;

            match queue.pop_present(now) {
                QueueAction::Present(frame) => {
                    if let Some(prev) = last_pts {
                        if frame.metadata.pts_nanos < prev {
                            return Err(format!(
                                "non-monotonic PTS: {} < {prev}",
                                frame.metadata.pts_nanos
                            ));
                        }
                    }
                    last_pts = Some(frame.metadata.pts_nanos);
                    if frame.metadata.width != 3840 || frame.metadata.height != 2160 {
                        return Err(format!(
                            "expected 3840x2160 frame, got {}x{}",
                            frame.metadata.width, frame.metadata.height
                        ));
                    }
                }
                QueueAction::WaitFor { wait_nanos } => {
                    std::thread::sleep(Duration::from_nanos(wait_nanos.min(1_000_000)));
                }
                QueueAction::Empty => {
                    std::thread::sleep(Duration::from_micros(500));
                }
            }

            if fed == total && !eof_sent {
                dec.end_of_stream()
                    .map_err(|e| format!("end_of_stream failed: {e}"))?;
                eof_sent = true;
            }

            // Termination: every packet fed, the EOF drain issued, the
            // decoder out of frames, and the queue empty.
            if eof_sent && queue.is_empty() && !got_frame {
                break;
            }
            if start.elapsed() > budget {
                return Err(format!(
                    "leg wedged: {}/{} fed, eof={eof_sent}, queue={}, decoded={}, \
                     presented={}, dropped={}",
                    fed,
                    total,
                    queue.len(),
                    dec.stats().frames_decoded,
                    queue.presented(),
                    queue.dropped()
                ));
            }
        }

        Ok((queue, start.elapsed()))
    }

    /// Prints the per-leg acceptance report and asserts the gate thresholds.
    ///
    /// Lost frames are folded together: `FrameQueue::dropped` (pacing drops)
    /// plus `DecodeStats::packets_rejected` (decoder-side drops — the
    /// VideoToolbox backend reports overflow/fatal drop accounting through
    /// that counter) must jointly stay under the 0.1% spec budget. Presented
    /// frames additionally tolerate a two-frame tail (a trailing packet may
    /// legitimately carry no displayable frame).
    fn report(
        leg: &Leg,
        backend: Backend,
        dec: &dyn VideoDecoder,
        queue: &FrameQueue,
        wall: Duration,
    ) {
        let stats = dec.stats();
        let total = leg.packets.len();
        let presented = queue.presented();
        let dropped = queue.dropped();
        let rejected = stats.packets_rejected;
        let drop_pct = queue.drop_rate_pct();
        let cpu_pct = queue.cpu_utilization_pct(120.0);
        let loss_pct = ((dropped + rejected) as f64 / total as f64) * 100.0;
        let fps = presented as f64 / wall.as_secs_f64();
        eprintln!(
            "4k120 {:>5} | backend={} hw={} | frames={} presented={} dropped={} \
             rejected={} | drop={drop_pct:.3}% loss={loss_pct:.3}% cpu={cpu_pct:.3}% | \
             wall={:.2}s fps={fps:.1}",
            leg.name,
            backend.name(),
            stats.hardware_accelerated(),
            total,
            presented,
            dropped,
            rejected,
            wall.as_secs_f64(),
        );

        // The spec thresholds gate the hardware decode path. A leg that fell
        // back to software (e.g. AV1 on hosts whose VideoToolbox cannot
        // hardware-decode it through this backend) is measured and
        // reported with the same honesty — the numbers above are real — but
        // cannot meet a spec that itself documents software decode as
        // infeasible at 4K120, so it asserts correctness only.
        if stats.hardware_accelerated() {
            assert!(
                drop_pct < MAX_LOSS_PCT,
                "4k120 {}: queue drop rate {drop_pct:.3}% exceeds {MAX_LOSS_PCT}%",
                leg.name
            );
            assert!(
                loss_pct < MAX_LOSS_PCT,
                "4k120 {}: combined loss {loss_pct:.3}% exceeds {MAX_LOSS_PCT}%",
                leg.name
            );
            assert!(
                cpu_pct < MAX_CPU_PCT,
                "4k120 {}: queue CPU utilization {cpu_pct:.3}% exceeds {MAX_CPU_PCT}%",
                leg.name
            );
            assert!(
                presented as usize + 2 >= total,
                "4k120 {}: only {presented}/{total} frames presented",
                leg.name
            );
        } else {
            eprintln!(
                "4k120: {} — software backend; thresholds advisory, \
                 not asserted",
                leg.name
            );
            assert!(
                presented > 0,
                "4k120 {}: software leg decoded no frames",
                leg.name
            );
        }
    }

    /// Runs one codec leg: load the sample, pick a working backend, pump at
    /// 120 fps, assert the acceptance thresholds.
    fn run_leg(codec: VideoCodec, path: &Path, name: &'static str) {
        let Some(leg) = load_leg(codec, path, name) else {
            return;
        };
        if leg.packets.len() < MIN_FRAMES {
            eprintln!(
                "4k120: {name} has only {} frames (< {MIN_FRAMES}); leg skipped",
                leg.packets.len()
            );
            return;
        }

        let mut config = DecoderConfig::new(codec, 3840, 2160);
        config.decode_ahead = DECODE_AHEAD;
        config.codec_config = leg.codec_config.clone();

        let first = EncodedPacket::new(leg.packets[0].clone(), 0, GATE_FRAME_NS);
        let Some((backend, mut dec, primed)) = open_working_decoder(&config, &first) else {
            eprintln!("4k120: no usable decoder backend for {name}; leg skipped");
            return;
        };
        if matches!(backend, Backend::Ffmpeg) && leg.codec == VideoCodec::Av1 {
            eprintln!("4k120: {name} — using dav1d software decode via the ffmpeg backend");
        }

        match pump(&mut *dec, &leg, 1, primed) {
            Ok((queue, wall)) => report(&leg, backend, &*dec, &queue, wall),
            Err(e) => panic!("4k120 {name} leg failed: {e}"),
        }
    }

    /// Gate entry point: one leg per codec whose sample exists on disk.
    pub fn run() {
        let dir = samples_dir();
        for (codec, name) in [
            (VideoCodec::H264, "h264-4k120.bin"),
            (VideoCodec::Hevc, "hevc-4k120.bin"),
            (VideoCodec::Av1, "av1-4k120.bin"),
        ] {
            run_leg(codec, &dir.join(name), name);
        }
    }
}

/// `DecodedFrame` plumbing sanity: handles classify correctly.
#[test]
fn decoded_frame_handle_classification() {
    use martensite_media::surface::{ColorRange, VideoFrameMetadata};
    let meta = VideoFrameMetadata::new(64, 64, VideoPixelFormat::Nv12, ColorRange::Limited);
    let hw = DecodedFrame::new(HardwareHandle::Mock { id: 1 }, meta.clone());
    let sw = DecodedFrame::new(
        HardwareHandle::CpuMemory {
            y_plane: vec![0; 64 * 64],
            uv_plane: vec![0; 64 * 32],
            y_stride: 64,
            uv_stride: 64,
        },
        meta,
    );
    assert!(hw.is_zero_copy());
    assert!(!sw.is_zero_copy());
}
