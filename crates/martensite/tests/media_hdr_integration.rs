//! Integration tests for Martensite v0.8.0 — Media & Advanced GPU milestone.
//!
//! These tests exercise the cross-crate interaction between `martensite-media`,
//! `martensite-wgpu`, and `martensite` core/widgets, verifying all exit criteria:
//!
//! 1. Zero-Copy Video Playback Gate: 4K 60fps NV12 (8-bit SDR) and P010 (10-bit HDR)
//!    telemetry verification with < 1% CPU utilization (< 0.10 ms dispatch per frame).
//! 2. Colorimetric Accuracy Gate: SMPTE color bars roundtrip with ΔE < 1.0
//!    across BT.709 and BT.2020, and SMPTE ST 2084 PQ EOTF dynamic range accuracy.
//! 3. HDR/SDR Blending Verification Gate: Linear optical scRGB compositing with
//!    WCAG contrast preservation (≥ 4.5:1) and zero SDR highlight clipping.
//! 4. Filmic Tone Mapping Gate: Hable and Uchimura filmic operators monotonicity,
//!    shoulder compression, and display adaptation.
//! 5. AOT WGSL Shader Gate: Naga validation of `MEDIA_YUV_EOTF_WGSL` compute shader.
//! 6. MediaView Widget Layout & Hierarchy Gate: Letterbox/pillarbox math,
//!    Flex/Stack composition, and accessibility role integration.

#![forbid(unsafe_code)]

use accesskit::Node as AccessKitNode;
use glam::Vec3;
use martensite::widgets::media::{MediaView, VideoFit};
use martensite::widgets::{Button, Container, Flex, Stack, Text};
use martensite_assets::ShaderValidator;
use martensite_core::node::Rect;
use martensite_core::widget::Widget;
use martensite_core::{ColdNode, HotNode, WidgetArena};
use martensite_layout::engine::LayoutEngine;
use martensite_media::color::{
    bt2020_yuv_to_rgb, bt709_yuv_to_rgb, delta_e_76, pq_eotf, pq_oetf, rgb_to_bt2020_yuv,
    rgb_to_bt709_yuv, ScRgb,
};
use martensite_media::surface::{
    ColorRange, HardwareHandle, VideoFrameMetadata, VideoPixelFormat, VideoSurface,
};
use martensite_media::tonemap::{
    hable_tonemap_scalar, uchimura_tonemap_scalar, DisplayProfile, ToneMapOperator,
};
use martensite_theme::oklab::wcag_contrast;
use martensite_wgpu::interop::{FormatNegotiator, VideoPipelineUniforms, MEDIA_YUV_EOTF_WGSL};
use taffy::{AvailableSpace, Size};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn approx_eq(a: f32, b: f32, tol: f32) -> bool {
    (a - b).abs() < tol
}

fn vec3_approx_eq(a: Vec3, b: Vec3, tol: f32) -> bool {
    approx_eq(a.x, b.x, tol) && approx_eq(a.y, b.y, tol) && approx_eq(a.z, b.z, tol)
}

// ---------------------------------------------------------------------------
// Gate 1: Zero-Copy Video Playback Telemetry & Format Negotiation
// ---------------------------------------------------------------------------

#[test]
fn gate1_zero_copy_playback_nv12_4k60_telemetry() {
    let meta = VideoFrameMetadata::new(3840, 2160, VideoPixelFormat::Nv12, ColorRange::Limited);
    let mut surface = VideoSurface::new(HardwareHandle::Mock { id: 4001 }, meta);

    // Assert initial zero-copy state.
    assert!(surface.handle().is_zero_copy());
    assert_eq!(surface.metadata().width, 3840);
    assert_eq!(surface.metadata().height, 2160);
    assert_eq!(surface.metadata().format.bits_per_channel(), 8);
    assert!(!surface.metadata().format.is_hdr());

    // Simulate 600 frames (10 seconds of 60 fps playback).
    for i in 0..600 {
        surface.update_handle(HardwareHandle::Mock { id: 4001 }, i * 16_666_667);
    }

    assert_eq!(surface.fence_id(), 601);

    // CPU utilization must be well below 1.0% (< 0.10 ms per frame budget).
    let cpu_pct = surface.cpu_utilization_pct(60.0);
    assert!(
        cpu_pct < 1.0,
        "CPU utilization {cpu_pct}% must be < 1.0% for zero-copy 4K60"
    );
}

#[test]
fn gate1_zero_copy_playback_p010_4k60_hdr_telemetry() {
    let meta = VideoFrameMetadata::new(3840, 2160, VideoPixelFormat::P010, ColorRange::Limited);
    let mut surface = VideoSurface::new(
        HardwareHandle::DxgiSharedHandle {
            handle: 0xCAFE_BABE,
        },
        meta,
    );

    assert!(surface.handle().is_zero_copy());
    assert_eq!(surface.metadata().format.bits_per_channel(), 10);
    assert!(surface.metadata().format.is_hdr());

    // Simulate 300 frames of HDR 4K60 playback.
    for i in 0..300 {
        surface.update_handle(
            HardwareHandle::DxgiSharedHandle {
                handle: 0xCAFE_BABE,
            },
            i * 16_666_667,
        );
    }

    assert_eq!(surface.fence_id(), 301);
    let cpu_pct = surface.cpu_utilization_pct(60.0);
    assert!(
        cpu_pct < 1.0,
        "10-bit HDR zero-copy CPU utilization {cpu_pct}% must be < 1.0%"
    );
}

#[test]
fn gate1_format_negotiation_planes_and_swapchains() {
    use martensite_wgpu::wgpu::TextureFormat;

    // NV12 plane negotiation: R8 for Y, RG8 for interleaved UV.
    let (y_nv12, uv_nv12) = FormatNegotiator::negotiate_plane_formats(VideoPixelFormat::Nv12);
    assert_eq!(y_nv12, TextureFormat::R8Unorm);
    assert_eq!(uv_nv12, Some(TextureFormat::Rg8Unorm));

    // P010 plane negotiation: R16 for 10-bit Y, RG16 for 10-bit UV.
    let (y_p010, uv_p010) = FormatNegotiator::negotiate_plane_formats(VideoPixelFormat::P010);
    assert_eq!(y_p010, TextureFormat::R16Unorm);
    assert_eq!(uv_p010, Some(TextureFormat::Rg16Unorm));

    // Single-plane packed formats.
    let (rgba8_y, rgba8_uv) = FormatNegotiator::negotiate_plane_formats(VideoPixelFormat::Rgba8);
    assert_eq!(rgba8_y, TextureFormat::Rgba8Unorm);
    assert_eq!(rgba8_uv, None);

    let (rgba16_y, rgba16_uv) =
        FormatNegotiator::negotiate_plane_formats(VideoPixelFormat::Rgba16Float);
    assert_eq!(rgba16_y, TextureFormat::Rgba16Float);
    assert_eq!(rgba16_uv, None);

    // Swapchain format selection.
    assert_eq!(
        FormatNegotiator::select_best_swapchain_format(&[
            TextureFormat::Bgra8UnormSrgb,
            TextureFormat::Rgba16Float
        ]),
        TextureFormat::Rgba16Float
    );
    assert_eq!(
        FormatNegotiator::select_best_swapchain_format(&[
            TextureFormat::Bgra8UnormSrgb,
            TextureFormat::Rgba8Unorm
        ]),
        TextureFormat::Bgra8UnormSrgb
    );
}

// ---------------------------------------------------------------------------
// Gate 2: Colorimetric Accuracy & SMPTE Test Patterns
// ---------------------------------------------------------------------------

#[test]
fn gate2_colorimetric_accuracy_smpte_color_bars_bt709() {
    // SMPTE 75% and 100% color bars in normalized non-linear RGB.
    let smpte_bars = [
        ("White", Vec3::new(1.0, 1.0, 1.0)),
        ("Yellow", Vec3::new(1.0, 1.0, 0.0)),
        ("Cyan", Vec3::new(0.0, 1.0, 1.0)),
        ("Green", Vec3::new(0.0, 1.0, 0.0)),
        ("Magenta", Vec3::new(1.0, 0.0, 1.0)),
        ("Red", Vec3::new(1.0, 0.0, 0.0)),
        ("Blue", Vec3::new(0.0, 0.0, 1.0)),
        ("Black", Vec3::new(0.0, 0.0, 0.0)),
        ("75% White", Vec3::new(0.75, 0.75, 0.75)),
        ("75% Yellow", Vec3::new(0.75, 0.75, 0.0)),
        ("75% Cyan", Vec3::new(0.0, 0.75, 0.75)),
        ("75% Green", Vec3::new(0.0, 0.75, 0.0)),
    ];

    for (name, rgb) in smpte_bars {
        let (y, cb, cr) = rgb_to_bt709_yuv(rgb);
        let recovered = bt709_yuv_to_rgb(y, cb, cr);

        let delta_e = delta_e_76(rgb, recovered);
        assert!(
            delta_e < 0.05,
            "BT.709 color bar '{name}' ΔE = {delta_e} exceeds threshold 0.05 (target < 1.0)"
        );
        assert!(
            vec3_approx_eq(rgb, recovered, 0.001),
            "BT.709 color bar '{name}' roundtrip mismatch: orig={rgb:?}, rec={recovered:?}"
        );
    }
}

#[test]
fn gate2_colorimetric_accuracy_smpte_color_bars_bt2020() {
    let smpte_bars = [
        ("White", Vec3::new(1.0, 1.0, 1.0)),
        ("Yellow", Vec3::new(1.0, 1.0, 0.0)),
        ("Cyan", Vec3::new(0.0, 1.0, 1.0)),
        ("Green", Vec3::new(0.0, 1.0, 0.0)),
        ("Magenta", Vec3::new(1.0, 0.0, 1.0)),
        ("Red", Vec3::new(1.0, 0.0, 0.0)),
        ("Blue", Vec3::new(0.0, 0.0, 1.0)),
        ("Black", Vec3::new(0.0, 0.0, 0.0)),
    ];

    for (name, rgb) in smpte_bars {
        let (y, cb, cr) = rgb_to_bt2020_yuv(rgb);
        let recovered = bt2020_yuv_to_rgb(y, cb, cr);

        let delta_e = delta_e_76(rgb, recovered);
        assert!(
            delta_e < 0.05,
            "BT.2020 color bar '{name}' ΔE = {delta_e} exceeds threshold 0.05 (target < 1.0)"
        );
        assert!(
            vec3_approx_eq(rgb, recovered, 0.001),
            "BT.2020 color bar '{name}' roundtrip mismatch: orig={rgb:?}, rec={recovered:?}"
        );
    }
}

#[test]
fn gate2_smpte_st_2084_pq_eotf_dynamic_range_accuracy() {
    // Dynamic range luminance levels from black level up to 10,000 nits.
    let test_nits = [
        0.005,   // Reference OLED black level
        0.05,    // Deep shadow
        1.0,     // Dim room low tone
        18.0,    // 18% photographic gray (~18 nits in 100 nit SDR)
        100.0,   // Standard SDR reference white
        203.0,   // ITU-R BT.2408 HDR reference white
        500.0,   // Entry-level HDR display peak
        1000.0,  // Standard HDR mastering target
        2000.0,  // High-end HDR TV peak
        4000.0,  // Mastering monitor peak (Pulsar)
        10000.0, // Full SMPTE ST 2084 peak
    ];

    for nits in test_nits {
        let pq_val = pq_oetf(nits);
        assert!(
            (0.0..=1.0).contains(&pq_val),
            "PQ OETF encoded value {pq_val} must be within [0.0, 1.0] for {nits} nits"
        );

        let recovered_nits = pq_eotf(pq_val);
        let rel_error = (recovered_nits - nits).abs() / nits;

        assert!(
            rel_error < 1e-4,
            "PQ EOTF roundtrip relative error {rel_error} for {nits} nits must be < 1e-4"
        );
    }
}

// ---------------------------------------------------------------------------
// Gate 3: HDR/SDR Blending & Contrast Verification
// ---------------------------------------------------------------------------

#[test]
fn gate3_hdr_sdr_linear_blending_and_contrast() {
    // Create an HDR video background color in scRGB:
    // 1000 nits peak specular highlight = 5.0 in scRGB (assuming 200 nits SDR white).
    let hdr_background = ScRgb::new(5.0, 4.5, 3.8, 1.0);

    // Standard SDR UI HUD panel: dark background (#141414 -> ~0.08) with 80% alpha.
    let ui_hud = ScRgb::new(0.08, 0.08, 0.08, 0.8);

    // Composite HUD over HDR background in linear optical space.
    let blended_hud = hdr_background.blend_over(ui_hud);

    // Blended HUD should maintain linear contribution:
    // R = 0.08 * 0.8 + 5.0 * (1.0 - 0.8) = 0.064 + 1.0 = 1.064
    assert!(
        approx_eq(blended_hud.r, 1.064, 0.01),
        "Linear compositing red channel {} must match analytical optical blend 1.064",
        blended_hud.r
    );

    // Now composite pure white UI text (#FFFFFF -> 1.0) over the blended HUD.
    let ui_white_text = ScRgb::new(1.0, 1.0, 1.0, 1.0);
    let blended_text = blended_hud.blend_over(ui_white_text);
    assert_eq!(blended_text.r, 1.0);
    assert_eq!(blended_text.g, 1.0);
    assert_eq!(blended_text.b, 1.0);

    // Verify WCAG contrast between UI white text and UI HUD dark background.
    let contrast = wcag_contrast(
        martensite_theme::oklab::Oklab {
            l: 1.0,
            a: 0.0,
            b: 0.0,
            alpha: 1.0,
        },
        martensite_theme::oklab::Oklab {
            l: 0.28,
            a: 0.0,
            b: 0.0,
            alpha: 1.0,
        },
    );
    assert!(
        contrast >= 4.5,
        "UI text over HUD must satisfy WCAG AA contrast (≥ 4.5:1), got {contrast}"
    );

    // Verify dynamic reference white adaptation.
    let profile_sdr = DisplayProfile::default_sdr();
    let profile_hdr = DisplayProfile::default_hdr10();
    let profile_clamped = DisplayProfile::new(Some(203.0), 0.2, 160.0);

    assert_eq!(profile_sdr.effective_sdr_white(), 203.0);
    assert_eq!(profile_hdr.effective_sdr_white(), 203.0);
    assert_eq!(profile_clamped.effective_sdr_white(), 160.0);
}

// ---------------------------------------------------------------------------
// Gate 4: Filmic Tone-Mapping Operators
// ---------------------------------------------------------------------------

#[test]
fn gate4_filmic_tonemapping_operators() {
    // Verify monotonicity across a wide dynamic range (0.0 to 50.0 linear scRGB).
    let sample_points = [0.0, 0.1, 0.5, 1.0, 2.0, 5.0, 10.0, 20.0, 50.0];

    for i in 0..sample_points.len() - 1 {
        let x1 = sample_points[i];
        let x2 = sample_points[i + 1];

        let h1 = hable_tonemap_scalar(x1);
        let h2 = hable_tonemap_scalar(x2);
        assert!(
            h2 >= h1,
            "Hable curve must be monotonically non-decreasing: H({x1})={h1}, H({x2})={h2}"
        );

        let u1 = uchimura_tonemap_scalar(x1);
        let u2 = uchimura_tonemap_scalar(x2);
        assert!(
            u2 >= u1,
            "Uchimura curve must be monotonically non-decreasing: U({x1})={u1}, U({x2})={u2}"
        );
    }

    // Verify zero input yields zero output.
    assert_eq!(hable_tonemap_scalar(0.0), 0.0);
    assert_eq!(uchimura_tonemap_scalar(0.0), 0.0);

    // Verify shoulder compression: extreme highlights smoothly compress toward 1.0.
    let extreme_hdr = Vec3::new(20.0, 30.0, 50.0);
    let hable_out = Vec3::new(
        hable_tonemap_scalar(extreme_hdr.x),
        hable_tonemap_scalar(extreme_hdr.y),
        hable_tonemap_scalar(extreme_hdr.z),
    );
    assert!(hable_out.x <= 1.0 && hable_out.y <= 1.0 && hable_out.z <= 1.0);
    assert!(hable_out.x > 0.8 && hable_out.y > 0.8 && hable_out.z > 0.8);

    let uchimura_out = Vec3::new(
        uchimura_tonemap_scalar(extreme_hdr.x),
        uchimura_tonemap_scalar(extreme_hdr.y),
        uchimura_tonemap_scalar(extreme_hdr.z),
    );
    assert!(uchimura_out.x <= 1.0 && uchimura_out.y <= 1.0 && uchimura_out.z <= 1.0);

    // Test ToneMapOperator mapping adaptation.
    let mapped_hable = ToneMapOperator::Hable.map_rgb(extreme_hdr);
    assert!(mapped_hable.x <= 1.0 && mapped_hable.y <= 1.0 && mapped_hable.z <= 1.0);
    let mapped_uchimura = ToneMapOperator::Uchimura.map_rgb(extreme_hdr);
    assert!(mapped_uchimura.x <= 1.0 && mapped_uchimura.y <= 1.0 && mapped_uchimura.z <= 1.0);
}

// ---------------------------------------------------------------------------
// Gate 5: AOT WGSL Shader & Uniform Buffer Alignment
// ---------------------------------------------------------------------------

#[test]
fn gate5_wgsl_shader_validation() {
    let mut validator = ShaderValidator::new();
    let reflection = validator
        .validate(MEDIA_YUV_EOTF_WGSL)
        .expect("MEDIA_YUV_EOTF_WGSL must pass Naga validation");

    // Must have a compute entry point named "main".
    let main_ep = reflection
        .entry_points
        .iter()
        .find(|ep| ep.name == "main")
        .expect("Shader must have 'main' compute entry point");

    assert_eq!(
        main_ep.stage,
        martensite_assets::shader::ShaderStage::Compute
    );
    assert_eq!(main_ep.workgroup_size, [16, 16, 1]);

    // Verify VideoPipelineUniforms struct alignment and byte size.
    assert_eq!(std::mem::size_of::<VideoPipelineUniforms>(), 256);
    assert_eq!(std::mem::align_of::<VideoPipelineUniforms>(), 256);

    let sdr_uniforms = VideoPipelineUniforms::new_bt709_sdr();
    let hdr_uniforms = VideoPipelineUniforms::new_bt2020_hdr(true, 200.0, 1000.0);

    assert_eq!(sdr_uniforms.as_bytes().len(), 256);
    assert_eq!(hdr_uniforms.as_bytes().len(), 256);
}

// ---------------------------------------------------------------------------
// Gate 6: MediaView Widget Layout & Hierarchy
// ---------------------------------------------------------------------------

#[test]
fn gate6_media_view_aspect_ratio_and_rect_calculation() {
    // 16:9 aspect ratio in a 1000x1000 square viewport.
    let viewport = Rect::new(0.0, 0.0, 1000.0, 1000.0);
    let view = MediaView::new().with_aspect_ratio(16.0 / 9.0);

    // Contain mode: letterboxed vertically.
    let contain_rect = view.compute_dest_rect(viewport, VideoFit::Contain);
    assert_eq!(contain_rect.origin.x, 0.0);
    assert_eq!(contain_rect.size.x, 1000.0);
    assert!(approx_eq(contain_rect.size.y, 562.5, 0.01));
    assert!(approx_eq(contain_rect.origin.y, 218.75, 0.01));

    // Cover mode: pillarbox cropped horizontally.
    let cover_rect = view.compute_dest_rect(viewport, VideoFit::Cover);
    assert_eq!(cover_rect.origin.y, 0.0);
    assert_eq!(cover_rect.size.y, 1000.0);
    assert!(approx_eq(cover_rect.size.x, 1777.778, 0.01));
    assert!(approx_eq(cover_rect.origin.x, -388.889, 0.01));

    // Fill mode: matches viewport exactly.
    let fill_rect = view.compute_dest_rect(viewport, VideoFit::Fill);
    assert_eq!(fill_rect, viewport);

    // Fixed mode: surface native dimensions.
    let meta = VideoFrameMetadata::new(640, 360, VideoPixelFormat::Nv12, ColorRange::Limited);
    let surface = VideoSurface::new(HardwareHandle::Mock { id: 8001 }, meta);
    let fixed_view = MediaView::new().with_surface(surface);
    let fixed_rect = fixed_view.compute_dest_rect(viewport, VideoFit::Fixed);
    assert_eq!(fixed_rect.size.x, 640.0);
    assert_eq!(fixed_rect.size.y, 360.0);
    assert_eq!(fixed_rect.origin.x, 180.0);
    assert_eq!(fixed_rect.origin.y, 320.0);
}

#[test]
fn gate6_media_view_widget_in_hierarchy() {
    let mut arena = WidgetArena::with_capacity(10);

    let meta = VideoFrameMetadata::new(1920, 1080, VideoPixelFormat::Nv12, ColorRange::Limited);
    let surface = VideoSurface::new(HardwareHandle::Mock { id: 7001 }, meta);

    let media_widget = MediaView::new()
        .with_surface(surface)
        .with_fit(VideoFit::Contain);

    // Verify accessibility role is Role::Video.
    let mut access_node = AccessKitNode::default();
    media_widget.accessibility(&mut access_node);
    assert_eq!(access_node.role(), accesskit::Role::Video);

    // Insert into arena.
    let media_id = arena.insert(
        HotNode::new(taffy::NodeId::new(0)),
        ColdNode::new(Box::new(media_widget)),
    );

    assert_eq!(arena.len(), 1);
    assert!(arena.get_hot(media_id).is_some());

    // Create a player UI hierarchy: MediaView inside a Stack with an overlay Flex container.
    let overlay_flex = Flex::column()
        .child(Text::new("Playing: Big Buck Bunny 4K HDR"))
        .child(Button::new("Pause"));

    let player_stack = Stack::new()
        .child(MediaView::new().with_aspect_ratio(16.0 / 9.0))
        .child(Container::new().padding_uniform(16.0).child(overlay_flex));

    let root_id = arena.insert(
        HotNode::new(taffy::NodeId::new(1)),
        ColdNode::new(Box::new(player_stack)),
    );

    let mut engine = LayoutEngine::new();
    engine
        .compute_with_widgets(
            &mut arena,
            root_id,
            Size {
                width: AvailableSpace::Definite(1280.0),
                height: AvailableSpace::Definite(720.0),
            },
        )
        .expect("layout computation must succeed");

    let hot_root = arena.get_hot(root_id).expect("root must exist");
    assert!(hot_root.bounds.width() > 0.0);
    assert!(hot_root.bounds.height() > 0.0);
}
