//! HDR golden-frame gate (v0.16.0): decoder HDR side data →
//! [`HdrMetadata`] → [`VideoPipelineUniforms`] → tonemapped scRGB output.
//!
//! Two layers:
//!
//! - **Layer A** (always runs under `decoder-ffmpeg`): decodes the checked-in
//!   10-bit HEVC Annex-B fixtures through [`FfmpegDecoder`], asserts the
//!   [`HdrSideData`] on every emitted frame (PQ `eotf_code = 16` + mastering
//!   display / MaxCLL; HLG `eotf_code = 18`), and verifies the
//!   [`VideoPipelineUniforms::from_hdr_metadata`] mapping (EOTF flag 1 = PQ,
//!   2 = HLG).
//! - **Layer B** (needs a real GPU, skips gracefully when no adapter is
//!   available): uploads one decoded frame via
//!   [`import_cpu_memory`], runs [`VideoProcessor`]'s WGSL pipeline, reads the
//!   `Rgba16Float` output back, and compares it per-pixel against a CPU
//!   reference that mirrors [`martensite_wgpu::interop::MEDIA_YUV_EOTF_WGSL`]
//!   operation-for-operation. It also asserts the semantic invariant that the
//!   HDR path diverges meaningfully from treating the same pixels as
//!   BT.709 SDR.
//!
//! ## Fixture regeneration
//!
//! The fixtures are 320x240@30fps 10-bit HEVC Annex-B streams (~30 frames,
//! ~4 KB each) produced from a moving `gradients` source so pixel comparisons
//! are meaningful:
//!
//! ```sh
//! ffmpeg -f lavfi -i "gradients=size=320x240:duration=1:rate=30" \
//!     -pix_fmt yuv420p10le -c:v libx265 -crf 30 \
//!     -x265-params "colorprim=bt2020:transfer=smpte2084:colormatrix=bt2020nc:\
//! master-display=G(13250,3450)B(7500,3000)R(34000,16000)WP(15635,16450)L(10000000,50):\
//! max-cll=1000,400" \
//!     -f hevc crates/martensite-media-test/tests/fixtures/test-pq-320x240.hevc
//!
//! ffmpeg -f lavfi -i "gradients=size=320x240:duration=1:rate=30" \
//!     -pix_fmt yuv420p10le -c:v libx265 -crf 30 \
//!     -x265-params "colorprim=bt2020:transfer=arib-std-b67:colormatrix=bt2020nc" \
//!     -f hevc crates/martensite-media-test/tests/fixtures/test-hlg-320x240.hevc
//! ```
//!
//! Verified with:
//! `ffprobe -show_frames` → `color_transfer=smpte2084` + `Mastering display
//! metadata` (`min_luminance=50/10000`, `max_luminance=10000000/10000`) +
//! `Content light level metadata` (`max_content=1000`, `max_average=400`) for
//! PQ; `color_transfer=arib-std-b67` for HLG.
//!
//! ## Decode-format note
//!
//! FFmpeg's native HEVC decoder emits planar `yuv420p10le` for these streams;
//! [`FfmpegDecoder`] runs that through its swscale path and negotiates
//! [`VideoPixelFormat::Nv12`] (8-bit). The golden reference below therefore
//! mirrors *both* shader branches (8-bit NV12 and 10-bit P010) driven by the
//! negotiated format, so the test still validates the P010 path if a future
//! backend emits it verbatim.

#![forbid(unsafe_code)]
#![cfg(feature = "decoder-ffmpeg")]

mod common;

use martensite_media::decoder::ffmpeg::FfmpegDecoder;
use martensite_media::decoder::{
    DecodedFrame, DecoderConfig, EncodedPacket, HdrSideData, VideoCodec, VideoDecoder,
};
use martensite_media::hdr::{Eotf, HdrMetadata};
use martensite_media::surface::{ColorRange, HardwareHandle, VideoPixelFormat};
use martensite_media::tonemap::DisplayProfile;
use martensite_media_platform::{import_cpu_memory, ImportTextureDescriptor};
use martensite_wgpu::interop::{video_texture_views, VideoPipelineUniforms, VideoProcessor};

/// BT.2020/PQ HDR10 fixture: ST 2084 transfer, mastering-display volume
/// (1000/0.005 nits) and MaxCLL/MaxFALL 1000/400 signalled.
const PQ_STREAM: &[u8] = include_bytes!("fixtures/test-pq-320x240.hevc");
/// BT.2020/HLG fixture: ARIB STD-B67 transfer, no static metadata.
const HLG_STREAM: &[u8] = include_bytes!("fixtures/test-hlg-320x240.hevc");

const WIDTH: u32 = 320;
const HEIGHT: u32 = 240;
const FRAME_NS: u64 = 33_333_333; // 30 fps
const EXPECTED_FRAMES: usize = 30;

// ---------------------------------------------------------------------------
// Layer A — decode + metadata → uniforms
// ---------------------------------------------------------------------------

/// Feeds every HEVC access unit of `stream` to a fresh [`FfmpegDecoder`] and
/// drains the reordered output after `end_of_stream`.
fn decode_fixture(name: &str, stream: &[u8]) -> Vec<DecodedFrame> {
    let mut dec: FfmpegDecoder =
        VideoDecoder::init(DecoderConfig::new(VideoCodec::Hevc, WIDTH, HEIGHT))
            .unwrap_or_else(|e| panic!("{name}: HEVC decoder init failed: {e}"));

    let aus = common::hevc_access_units(stream);
    assert_eq!(
        aus.len(),
        EXPECTED_FRAMES,
        "{name}: fixture should split into {EXPECTED_FRAMES} access units"
    );
    for (i, au) in aus.iter().enumerate() {
        dec.send_packet(&EncodedPacket::new(
            au.to_vec(),
            i as u64 * FRAME_NS,
            FRAME_NS,
        ))
        .unwrap_or_else(|e| panic!("{name}: AU {i} ({} bytes) rejected: {e:?}", au.len()));
    }
    dec.end_of_stream()
        .unwrap_or_else(|e| panic!("{name}: end_of_stream failed: {e}"));

    let mut frames = Vec::new();
    while let Some(frame) = dec
        .try_recv_frame()
        .unwrap_or_else(|e| panic!("{name}: try_recv_frame failed: {e}"))
    {
        frames.push(frame);
    }
    assert_eq!(
        frames.len(),
        EXPECTED_FRAMES,
        "{name}: expected {EXPECTED_FRAMES} decoded frames"
    );
    assert!(
        frames.iter().all(|f| f.metadata.format.is_yuv()),
        "{name}: decoded frames must be bi-planar YUV"
    );
    frames
}

/// Extracts the HDR side data every frame of these fixtures must carry.
fn hdr_of<'f>(name: &str, frame: &'f DecodedFrame) -> &'f HdrSideData {
    frame
        .hdr
        .as_ref()
        .unwrap_or_else(|| panic!("{name}: decoded frame is missing HDR side data"))
}

#[test]
fn pq_fixture_decodes_with_hdr10_metadata_and_uniforms() {
    let frames = decode_fixture("PQ", PQ_STREAM);

    for frame in &frames {
        assert_eq!(frame.metadata.width, WIDTH);
        assert_eq!(frame.metadata.height, HEIGHT);
        let side = hdr_of("PQ", frame);
        assert_eq!(side.eotf_code, 16, "PQ fixture must signal ST 2084");
        assert_eq!(side.primaries_code, 9, "PQ fixture must signal BT.2020");
        assert!(!side.full_range, "fixture is encoded limited (tv) range");
        assert!(
            side.max_luminance_nits.is_some(),
            "PQ fixture must carry mastering-display luminance"
        );
        assert!(side.max_cll.is_some(), "PQ fixture must carry MaxCLL");
    }

    let side = hdr_of("PQ", &frames[0]);
    let max_lum = side.max_luminance_nits.unwrap();
    let min_lum = side.min_luminance_nits.unwrap();
    assert!(
        (max_lum - 1000.0).abs() < 0.5,
        "mastering max luminance should be 1000 nits, got {max_lum}"
    );
    assert!(
        (min_lum - 0.005).abs() < 0.001,
        "mastering min luminance should be 0.005 nits, got {min_lum}"
    );
    assert_eq!(side.max_cll, Some(1000));
    // FFmpeg 8 widened `AVContentLightMetadata` (`unsigned short` →
    // `unsigned`, so MaxFALL moved from byte offset 2 to 4);
    // `FfmpegDecoder::extract_hdr` discriminates the layout by payload
    // length, so MaxFALL = 400 (verified via ffprobe `max_average=400`)
    // must surface on both FFmpeg ≤7 and ≥8.
    assert_eq!(side.max_fall, Some(400), "MaxFALL must be decoded");

    let hdr = HdrMetadata::from(side);
    assert_eq!(hdr.eotf, Eotf::Pq);
    assert!(hdr.is_hdr10());
    assert_eq!(hdr.range, ColorRange::Limited);
    assert_eq!(hdr.mastering_display.unwrap().max_luminance_nits, max_lum);
    assert_eq!(hdr.content_light.unwrap().max_cll, 1000);

    let uniforms = VideoPipelineUniforms::from_hdr_metadata(
        &hdr,
        &DisplayProfile::default_hdr10(),
        frames[0].metadata.format,
    );
    assert_eq!(
        uniforms.flags[0],
        u32::from(frames[0].metadata.format == VideoPixelFormat::P010),
        "P010 flag must track the negotiated format"
    );
    assert_eq!(uniforms.flags[1], 0, "limited range");
    assert_eq!(uniforms.flags[2], 1, "PQ EOTF mode flag");
    assert_eq!(uniforms.flags[3], 1, "hable tonemap must engage for HDR");
    assert_eq!(uniforms.display_params[1], max_lum);
    assert!((uniforms.display_params[0] - 203.0).abs() < 1e-3);
    // BT.2020 YUV→RGB row 0 and the BT.2020→BT.709 gamut row 0.
    assert_eq!(uniforms.yuv_to_rgb_0[..3], [1.0, 0.0, 1.47460]);
    assert_eq!(uniforms.gamut_0[..3], [1.6605, -0.5876, -0.0728]);
}

#[test]
fn hlg_fixture_decodes_with_hlg_metadata_and_uniforms() {
    let frames = decode_fixture("HLG", HLG_STREAM);

    for frame in &frames {
        let side = hdr_of("HLG", frame);
        assert_eq!(side.eotf_code, 18, "HLG fixture must signal ARIB STD-B67");
        assert_eq!(side.primaries_code, 9, "HLG fixture must signal BT.2020");
    }

    let side = hdr_of("HLG", &frames[0]);
    let hdr = HdrMetadata::from(side);
    assert_eq!(hdr.eotf, Eotf::Hlg);
    assert!(hdr.is_hdr());
    assert!(!hdr.is_hdr10());
    assert_eq!(hdr.color_space, martensite_media::color::ColorSpace::Bt2020);

    let uniforms = VideoPipelineUniforms::from_hdr_metadata(
        &hdr,
        &DisplayProfile::default_hdr10(),
        frames[0].metadata.format,
    );
    assert_eq!(uniforms.flags[2], 2, "HLG EOTF mode flag");
    assert_eq!(uniforms.flags[3], 1, "hable tonemap must engage for HDR");
    // No mastering/CLL signalled → HLG peak falls back to the 10 000-nit
    // PQ reference carried in display.y (the shader scales HLG scene-linear
    // by it).
    assert_eq!(uniforms.display_params[1], 10_000.0);
}

// ---------------------------------------------------------------------------
// Layer B — golden frame on a real GPU
// ---------------------------------------------------------------------------

/// Returns a high-performance [`martensite_wgpu::device::GpuContext`], or
/// `None` (with a skip line on stderr) when no adapter is available — e.g. a
/// headless CI host without lavapipe. macOS always has Metal.
fn acquire_gpu() -> Option<martensite_wgpu::device::GpuContext> {
    match martensite_wgpu::device::GpuContext::with_power_preference(
        wgpu::PowerPreference::HighPerformance,
    ) {
        Ok(ctx) => Some(ctx),
        Err(e) => {
            eprintln!("skipping HDR golden test: no GPU adapter available ({e})");
            None
        }
    }
}

/// IEEE 754 half → single precision.
fn f16_to_f32(bits: u16) -> f32 {
    let sign = if bits & 0x8000 != 0 { -1.0f32 } else { 1.0 };
    let exp = i32::from((bits >> 10) & 0x1f);
    let frac = f32::from(bits & 0x3ff);
    match exp {
        0 => sign * frac * 2f32.powi(-24), // subnormal: frac/1024 * 2^-14
        31 => {
            if frac == 0.0 {
                sign * f32::INFINITY
            } else {
                f32::NAN
            }
        }
        e => sign * (1.0 + frac / 1024.0) * 2f32.powi(e - 15),
    }
}

/// Mirrors the WGSL `hable_f` filmic curve using the uniform-block constants
/// (no clamping — the shader stores the raw result).
fn hable_f(x: f32, u: &VideoPipelineUniforms) -> f32 {
    let (a, b, c, d) = (u.filmic1[0], u.filmic1[1], u.filmic1[2], u.filmic1[3]);
    let (e, f) = (u.filmic2[0], u.filmic2[1]);
    ((x * (a * x + c * b) + d * e) / (x * (a * x + b) + d * f)) - (e / f)
}

/// Mirrors the WGSL `hlg_eotf` (ARIB STD-B67 inverse OETF).
fn hlg_eotf(v: f32) -> f32 {
    let a = 0.178_832_77_f32;
    let b = 1.0 - 4.0 * a;
    let c = 0.5 - a * (4.0 * a).ln();
    let vc = v.clamp(0.0, 1.0);
    if vc <= 0.5 {
        vc * vc / 3.0
    } else {
        (((vc - c) / a).exp() + b) / 12.0
    }
}

fn dot3(row: [f32; 4], v: [f32; 3]) -> f32 {
    row[0] * v[0] + row[1] * v[1] + row[2] * v[2]
}

/// CPU mirror of `MEDIA_YUV_EOTF_WGSL`'s per-pixel math for one normalized
/// `(y_raw, cb_raw, cr_raw)` sample triple, driven entirely by the uniform
/// block's flags/matrices (exactly as the shader consumes them).
fn shader_reference_pixel(
    y_raw: f32,
    u_raw: f32,
    v_raw: f32,
    u: &VideoPipelineUniforms,
) -> [f32; 3] {
    let (y, cb, cr) = if u.flags[1] == 0 {
        if u.flags[0] == 1 {
            // P010 limited range (10-bit studio swing).
            (
                (y_raw - 64.0 / 1023.0) / (876.0 / 1023.0),
                (u_raw - 512.0 / 1023.0) / (896.0 / 1023.0),
                (v_raw - 512.0 / 1023.0) / (896.0 / 1023.0),
            )
        } else {
            // NV12 limited range (8-bit studio swing).
            (
                (y_raw - 16.0 / 255.0) / (219.0 / 255.0),
                (u_raw - 128.0 / 255.0) / (224.0 / 255.0),
                (v_raw - 128.0 / 255.0) / (224.0 / 255.0),
            )
        }
    } else {
        (y_raw, u_raw - 0.5, v_raw - 0.5)
    };

    let yuv = [y, cb, cr];
    let rgb_nl = [
        dot3(u.yuv_to_rgb_0, yuv),
        dot3(u.yuv_to_rgb_1, yuv),
        dot3(u.yuv_to_rgb_2, yuv),
    ];

    let linear_nits = match u.flags[2] {
        // PQ: absolute nits. `pq_eotf` saturates identically to the shader's
        // internal `clamp(n, 0, 1)`.
        1 => rgb_nl.map(martensite_media::color::pq_eotf),
        // HLG: scene-linear [0,1] scaled by the signalled content peak.
        2 => rgb_nl.map(|c| hlg_eotf(c) * u.display_params[1]),
        // SDR: pow(2.2) expansion scaled by SDR white.
        _ => rgb_nl.map(|c| c.max(0.0).powf(2.2) * u.display_params[0]),
    };

    let target = [
        dot3(u.gamut_0, linear_nits),
        dot3(u.gamut_1, linear_nits),
        dot3(u.gamut_2, linear_nits),
    ];

    let sdr_white = u.display_params[0];
    let mut scrgb = target.map(|c| c / sdr_white);

    if u.flags[2] == 2 {
        // HLG display system gamma approximation (gamma = 1.2).
        scrgb = scrgb.map(|c| c.max(0.0).powf(1.0 / 1.2));
    }

    if u.flags[3] == 1 {
        let white_scale = 1.0 / hable_f(u.filmic2[2], u);
        scrgb = scrgb.map(|c| hable_f(c, u) * white_scale);
    }

    scrgb
}

/// Reads a `u16` sample out of a plane buffer at byte offset `x` (LE, as in
/// P010LE / `R16Unorm` upload order).
fn sample_u16le(plane: &[u8], byte_offset: usize) -> f32 {
    let lo = plane[byte_offset];
    let hi = plane[byte_offset + 1];
    f32::from(u16::from_le_bytes([lo, hi])) / 65535.0
}

/// Reproduces the shader's per-pixel input sampling from a `CpuMemory`
/// bi-planar frame. The shader uses `textureLoad` with `uv_coord = coord / 2`
/// — integer-division nearest chroma, no bilinear filtering — so the
/// reference does exactly the same.
fn reference_frame(
    handle: &HardwareHandle,
    width: u32,
    height: u32,
    format: VideoPixelFormat,
    u: &VideoPipelineUniforms,
) -> Vec<[f32; 3]> {
    let HardwareHandle::CpuMemory {
        y_plane,
        uv_plane,
        y_stride,
        uv_stride,
    } = handle
    else {
        panic!("golden reference requires a CpuMemory handle");
    };
    let (y_stride, uv_stride) = (*y_stride as usize, *uv_stride as usize);
    let p010 = format == VideoPixelFormat::P010;

    let mut out = Vec::with_capacity((width * height) as usize);
    for py in 0..height as usize {
        for px in 0..width as usize {
            let (y_raw, u_raw, v_raw) = if p010 {
                let y = sample_u16le(y_plane, py * y_stride + px * 2);
                let base = (py / 2) * uv_stride + (px / 2) * 4;
                (
                    y,
                    sample_u16le(uv_plane, base),
                    sample_u16le(uv_plane, base + 2),
                )
            } else {
                let y = f32::from(y_plane[py * y_stride + px]) / 255.0;
                let base = (py / 2) * uv_stride + (px / 2) * 2;
                (
                    y,
                    f32::from(uv_plane[base]) / 255.0,
                    f32::from(uv_plane[base + 1]) / 255.0,
                )
            };
            out.push(shader_reference_pixel(y_raw, u_raw, v_raw, u));
        }
    }
    out
}

/// Output texture for the golden readback. `VideoProcessor::create_output_texture`
/// lacks `COPY_SRC`, so the test allocates its own `Rgba16Float` texture with
/// readback usage — the shader binding requirements are identical.
fn create_readback_output(device: &wgpu::Device, width: u32, height: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("hdr-golden-output"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba16Float,
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

/// Dispatches `process_frame` for `video` + `uniforms` into `encoder`, then
/// enqueues the copy of the `Rgba16Float` output into a fresh MAP_READ
/// staging buffer. Returns `(bytes_per_row, staging)`.
fn run_pipeline_to_staging(
    ctx: &martensite_wgpu::device::GpuContext,
    processor: &VideoProcessor,
    video: &martensite_media_platform::VideoTexture,
    uniforms: &VideoPipelineUniforms,
    width: u32,
    height: u32,
    encoder: &mut wgpu::CommandEncoder,
) -> (u32, wgpu::Buffer) {
    let uniform_buffer = processor.create_uniform_buffer(&ctx.device, uniforms);
    let output = create_readback_output(&ctx.device, width, height);
    let output_view = output.create_view(&wgpu::TextureViewDescriptor::default());
    let (y_view, uv_view) = video_texture_views(video);
    let uv_view = uv_view.expect("bi-planar frame must have a chroma plane");

    processor.process_frame(
        &ctx.device,
        encoder,
        &y_view,
        &uv_view,
        &uniform_buffer,
        &output_view,
        (width, height),
    );

    // Rgba16Float = 8 bytes/texel; encoder copies require 256-aligned rows.
    let bytes_per_row = (width * 8).div_ceil(256) * 256;
    let staging = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("hdr-golden-staging"),
        size: u64::from(bytes_per_row) * u64::from(height),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &output,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &staging,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    (bytes_per_row, staging)
}

/// Maps a staging buffer produced by [`run_pipeline_to_staging`] and returns
/// the decoded f32 texels as `width*height` `[r,g,b,a]` quads.
fn readback_scrgb(
    ctx: &martensite_wgpu::device::GpuContext,
    buffer: &wgpu::Buffer,
    bytes_per_row: u32,
    width: u32,
    height: u32,
) -> Vec<[f32; 4]> {
    let slice = buffer.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |res| {
        tx.send(res).expect("map callback channel");
    });
    ctx.device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("device poll for map");
    rx.recv().expect("map callback").expect("buffer map failed");

    let mapped = slice.get_mapped_range().expect("mapped range");
    // `bytes_per_row` is in bytes; each row holds `bytes_per_row / 2` f16
    // elements (4 channels/texel plus any pitch padding).
    let row_elems = bytes_per_row as usize / 2;
    let mut out = Vec::with_capacity((width * height) as usize);
    for py in 0..height as usize {
        for px in 0..width as usize {
            let base = (py * row_elems + px * 4) * 2;
            out.push([
                f16_to_f32(u16::from_le_bytes([mapped[base], mapped[base + 1]])),
                f16_to_f32(u16::from_le_bytes([mapped[base + 2], mapped[base + 3]])),
                f16_to_f32(u16::from_le_bytes([mapped[base + 4], mapped[base + 5]])),
                f16_to_f32(u16::from_le_bytes([mapped[base + 6], mapped[base + 7]])),
            ]);
        }
    }
    drop(mapped);
    buffer.unmap();
    out
}

/// Compares GPU output against the CPU shader mirror.
///
/// Tolerances (scRGB units, where 1.0 = 203 nits): the output is quantized to
/// `Rgba16Float` (≤ 2^-11 relative, ≈ 5e-4 absolute at 1.0) and Metal/Vulkan
/// `pow`/`exp`/`log` may differ from the host's by a few ULP on the steep PQ
/// shoulder, so bit-exactness is impossible across backends. The bounds are
/// expressed in 8-bit-equivalent terms: mean |err| < 2/255 and max |err| <
/// 8/255. On Apple M4/Metal the measured error is ~1e-4 mean / ~1e-3 max —
/// pure f16 output quantization — so these bounds fail loudly for any real
/// regression (wrong matrix, missing EOTF, or dropped tonemap each shifts
/// output by ≫0.1) while leaving headroom for less precise GPUs.
fn assert_golden_match(name: &str, gpu: &[[f32; 4]], reference: &[[f32; 3]]) {
    assert_eq!(gpu.len(), reference.len());
    let mut sum_abs = 0.0f32;
    let mut max_abs = 0.0f32;
    let mut worst = (0usize, [0.0; 4], [0.0; 3]);
    for (i, (g, r)) in gpu.iter().zip(reference.iter()).enumerate() {
        assert!(
            (g[3] - 1.0).abs() < 1e-3,
            "{name}: alpha must be 1.0 at pixel {i}, got {}",
            g[3]
        );
        for c in 0..3 {
            let err = (g[c] - r[c]).abs();
            sum_abs += err;
            if err > max_abs {
                max_abs = err;
                worst = (i, *g, *r);
            }
        }
    }
    let mean_abs = sum_abs / (gpu.len() * 3) as f32;
    eprintln!(
        "{name}: golden mean |err| = {mean_abs:.6} scRGB, max |err| = {max_abs:.6} \
         at pixel {} (gpu {:?} vs ref {:?})",
        worst.0, worst.1, worst.2
    );
    assert!(
        mean_abs < 2.0 / 255.0,
        "{name}: mean |err| {mean_abs:.6} exceeds 2/255 scRGB"
    );
    assert!(
        max_abs < 8.0 / 255.0,
        "{name}: max |err| {max_abs:.6} exceeds 8/255 scRGB at pixel {}",
        worst.0
    );
}

/// Mean absolute per-channel distance between two readbacks — the semantic
/// divergence metric for the SDR-vs-HDR invariant.
fn mean_abs_diff(a: &[[f32; 4]], b: &[[f32; 4]]) -> f32 {
    let sum: f32 = a
        .iter()
        .zip(b.iter())
        .flat_map(|(pa, pb)| (0..3).map(move |c| (pa[c] - pb[c]).abs()))
        .sum();
    sum / (a.len() * 3) as f32
}

/// The Layer-B driver shared by both fixtures.
fn golden_frame_check(name: &str, stream: &[u8]) {
    let Some(ctx) = acquire_gpu() else {
        return;
    };
    eprintln!(
        "{name}: golden test on adapter {:?} ({:?})",
        ctx.adapter_info.name, ctx.adapter_info.backend
    );

    let frames = decode_fixture(name, stream);
    // A mid-GOP frame exercises the inter-prediction path rather than the IDR.
    let frame = &frames[EXPECTED_FRAMES / 2];
    let side = hdr_of(name, frame);
    let hdr = HdrMetadata::from(side);
    let format = frame.metadata.format;
    let (w, h) = (frame.metadata.width, frame.metadata.height);

    let video = import_cpu_memory(
        &ctx.device,
        &ctx.queue,
        &frame.handle,
        &ImportTextureDescriptor::new(w, h, format),
    )
    .unwrap_or_else(|e| panic!("{name}: import_cpu_memory failed: {e}"));

    let processor = VideoProcessor::new(&ctx.device).unwrap_or_else(|e| panic!("{name}: {e}"));

    let hdr_uniforms =
        VideoPipelineUniforms::from_hdr_metadata(&hdr, &DisplayProfile::default_hdr10(), format);
    let sdr_uniforms = VideoPipelineUniforms::new_bt709_sdr();

    let mut encoder = ctx
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("hdr-golden-encoder"),
        });
    let (hdr_bpr, hdr_staging) =
        run_pipeline_to_staging(&ctx, &processor, &video, &hdr_uniforms, w, h, &mut encoder);
    let (sdr_bpr, sdr_staging) =
        run_pipeline_to_staging(&ctx, &processor, &video, &sdr_uniforms, w, h, &mut encoder);
    ctx.queue.submit(std::iter::once(encoder.finish()));

    let gpu_hdr = readback_scrgb(&ctx, &hdr_staging, hdr_bpr, w, h);
    let gpu_sdr = readback_scrgb(&ctx, &sdr_staging, sdr_bpr, w, h);

    let reference = reference_frame(&frame.handle, w, h, format, &hdr_uniforms);

    // Semantic invariant: HDR-aware processing must diverge meaningfully from
    // treating the same code values as BT.709 SDR — otherwise the EOTF/gamut
    // path is dead code.
    let divergence = mean_abs_diff(&gpu_hdr, &gpu_sdr);
    eprintln!("{name}: HDR-vs-SDR mean divergence = {divergence:.4} scRGB");
    assert!(
        divergence > 0.02,
        "{name}: HDR output must differ meaningfully from SDR treatment \
         (mean |diff| {divergence:.4})"
    );

    // Sanity: the tonemapped picture must have real dynamic range, not be a
    // clipped solid field.
    let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
    for p in &gpu_hdr {
        for c in &p[..3] {
            lo = lo.min(*c);
            hi = hi.max(*c);
        }
    }
    assert!(
        hi - lo > 0.3,
        "{name}: tonemapped output suspiciously flat (range {lo:.3}..{hi:.3})"
    );

    assert_golden_match(name, &gpu_hdr, &reference);
}

#[test]
fn pq_golden_frame_matches_shader_reference() {
    golden_frame_check("PQ", PQ_STREAM);
}

#[test]
fn hlg_golden_frame_matches_shader_reference() {
    golden_frame_check("HLG", HLG_STREAM);
}
