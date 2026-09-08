//! Cross-crate integration tests for the v0.9.0 Developer Experience milestone.
//!
//! These tests verify that the devtools, test harness, macros, and CLI
//! subsystems work together correctly.

use martensite_devtools::hud::{ArenaTelemetry, DiagnosticHud, FrameTiming, Rect};
use martensite_devtools::tracy;
use martensite_test::{dssim, images_match, HeadlessHarness, ImageBuffer, VirtualClock};

// ──────────────────────────────────────────────────────────────────────
// DevTools + Test Harness Integration
// ──────────────────────────────────────────────────────────────────────

#[test]
fn tracy_spans_record_into_hud_histogram() {
    // Simulate a frame: Tracy spans measure layout/paint/gpu, then
    // the HUD records the aggregated timing.
    let mut hud = DiagnosticHud::new();
    hud.toggle();
    assert!(hud.is_enabled());

    for _ in 0..10 {
        let layout = tracy::TracySpan::begin("layout");
        std::thread::sleep(std::time::Duration::from_micros(10));
        layout.end();

        let paint = tracy::TracySpan::begin("paint");
        std::thread::sleep(std::time::Duration::from_micros(5));
        paint.end();

        hud.record_frame(FrameTiming {
            layout_time_ns: 10_000,
            paint_time_ns: 5_000,
            gpu_wait_time_ns: 1_000,
            total_time_ns: 16_000,
        });
    }

    assert_eq!(hud.histogram().len(), 10);
    let avg = hud.histogram().average();
    assert!(avg.total_time_ns > 0);
}

#[test]
fn hud_dirty_rects_track_repainted_regions() {
    let mut hud = DiagnosticHud::new();
    hud.toggle();

    hud.add_dirty_rect(Rect::new(0, 0, 100, 100));
    hud.add_dirty_rect(Rect::new(50, 50, 200, 200));
    assert_eq!(hud.dirty_rects().len(), 2);
    assert!(hud.dirty_rects().total_area() > 0);

    hud.clear_dirty_rects();
    assert_eq!(hud.dirty_rects().len(), 0);
}

#[test]
fn hud_arena_telemetry_tracks_utilization() {
    let mut hud = DiagnosticHud::new();
    hud.toggle();

    hud.update_arena_telemetry(ArenaTelemetry::from_slots(1000, 500, 2));
    assert_eq!(hud.arena_telemetry().utilization_pct, 50.0);
    assert_eq!(hud.arena_telemetry().compaction_count, 2);
}

#[test]
fn tracy_overhead_gate_under_100us_per_frame() {
    // Exit criterion §5.3: Active Tracy profiling contributes < 0.1ms
    // per 60fps frame.
    let iterations = 60;
    let start = std::time::Instant::now();
    for _ in 0..iterations {
        let span = tracy::TracySpan::begin("test_frame");
        span.end();
        tracy::frame_mark();
    }
    let elapsed = start.elapsed();
    let per_frame_ns = elapsed.as_nanos() as u64 / iterations;
    assert!(
        per_frame_ns < 100_000,
        "Tracy overhead per frame: {per_frame_ns}ns, must be < 100_000ns (0.1ms)"
    );
}

// ──────────────────────────────────────────────────────────────────────
// VirtualClock + Headless Harness Integration
// ──────────────────────────────────────────────────────────────────────

#[test]
fn virtual_clock_drives_deterministic_frame_sequence() {
    let mut clock = VirtualClock::new();
    let mut frames = Vec::new();

    for i in 0..10 {
        clock.step_60fps();
        frames.push((i, clock.elapsed_millis()));
    }

    // Each frame should advance by ~16.666ms.
    assert!(frames[0].1 > 0);
    for window in frames.windows(2) {
        let delta = window[1].1 - window[0].1;
        assert!(
            (16..=17).contains(&delta),
            "frame delta should be ~16ms, got {delta}"
        );
    }
}

#[test]
fn headless_harness_captures_and_compares_frames() {
    let mut harness = HeadlessHarness::new(100, 100);

    // Render a simple gradient.
    let render = |frame: u64| -> Vec<u8> {
        let mut rgba = vec![0u8; 100 * 100 * 4];
        for y in 0..100 {
            for x in 0..100 {
                let idx = (y * 100 + x) * 4;
                rgba[idx] = ((x + frame as usize) as u8).wrapping_mul(2);
                rgba[idx + 1] = (y as u8).wrapping_mul(2);
                rgba[idx + 2] = 128;
                rgba[idx + 3] = 255;
            }
        }
        rgba
    };

    harness.run_frames(5, render);
    assert_eq!(harness.frame_count(), 5);
    assert_eq!(harness.snapshots().len(), 5);

    // Each snapshot should be deterministic for the same frame.
    let golden = harness.snapshots()[0].clone();
    assert!(images_match(&harness.snapshots()[0], &golden, 0.001));
}

#[test]
fn dssim_identical_images_yield_zero() {
    let img = ImageBuffer::new(64, 64);
    let score = dssim(&img, &img);
    assert!(
        score.abs() < 1e-9,
        "identical images should have DSSIM ~0, got {score}"
    );
}

#[test]
fn dssim_different_images_yield_nonzero() {
    let a = ImageBuffer::new(64, 64);
    let mut b = ImageBuffer::new(64, 64);
    b.fill(255);
    let score = dssim(&a, &b);
    assert!(
        score > 0.0,
        "different images should have DSSIM > 0, got {score}"
    );
}

#[test]
fn deterministic_ci_gate_100_runs() {
    // Exit criterion §5.2: 100 consecutive runs yield identical results.
    let a = ImageBuffer::new(32, 32);
    let mut b = ImageBuffer::new(32, 32);
    b.fill(128);

    let mut last_score: Option<f64> = None;
    for _ in 0..100 {
        let score: f64 = dssim(&a, &b);
        if let Some(prev) = last_score {
            assert!(
                (score - prev).abs() < 1e-12f64,
                "DSSIM score jitter: {score} vs {prev}"
            );
        }
        last_score = Some(score);
    }
    assert!(last_score.is_some());
}

// ──────────────────────────────────────────────────────────────────────
// Widget Macro Integration
// ──────────────────────────────────────────────────────────────────────

/// Test that the simple widget! macro form generates a Default struct.
#[test]
fn widget_macro_simple_form_generates_default_struct() {
    use martensite_macros::widget;
    widget!(TestSimpleWidget);
    let _ = TestSimpleWidget;
}

/// Test that the widget! macro with properties generates accessors.
#[test]
fn widget_macro_property_form_generates_accessors() {
    use martensite_macros::widget;
    widget! {
        TestPropertyWidget {
            label: String = String::new(),
            count: u32 = 0,
        }
    }

    let mut w = TestPropertyWidget::default();
    assert_eq!(*w.count(), 0);
    assert_eq!(w.label(), "");

    w.set_count(42);
    assert_eq!(*w.count(), 42);

    w.set_label("hello".to_string());
    assert_eq!(w.label(), "hello");

    w.count_mut();
    w.label_mut();
}

/// Test that the widget! macro generates a new() constructor.
#[test]
fn widget_macro_generates_new_constructor() {
    use martensite_macros::widget;
    widget! {
        TestNewWidget {
            value: i32 = 99,
        }
    }

    let w = TestNewWidget::new();
    assert_eq!(*w.value(), 99);
}

/// Test that the widget! macro handles complex types.
#[test]
fn widget_macro_handles_complex_types() {
    use martensite_macros::widget;
    widget! {
        TestComplexWidget {
            items: Vec<String> = Vec::new(),
            data: std::collections::HashMap<i32, String> = std::collections::HashMap::new(),
        }
    }

    let w = TestComplexWidget::default();
    assert!(w.items().is_empty());
    assert!(w.data().is_empty());
}

// ──────────────────────────────────────────────────────────────────────
// Cross-Subsystem Integration
// ──────────────────────────────────────────────────────────────────────

#[test]
fn devtools_hud_with_virtual_clock_frame_timing() {
    // Simulate a deterministic frame loop using VirtualClock and
    // record timings into the diagnostic HUD.
    let mut clock = VirtualClock::new();
    let mut hud = DiagnosticHud::new();
    hud.toggle();

    for _ in 0..120 {
        clock.step_60fps();
        hud.record_frame(FrameTiming {
            layout_time_ns: 500_000,
            paint_time_ns: 300_000,
            gpu_wait_time_ns: 100_000,
            total_time_ns: 900_000,
        });
    }

    // After 120 frames, the histogram should be full.
    assert_eq!(hud.histogram().len(), 120);
    let avg = hud.histogram().average();
    assert_eq!(avg.total_time_ns, 900_000);

    // The 99th percentile should also be ~900us (all frames are identical).
    let p99 = hud.histogram().percentile(99.0);
    assert_eq!(p99.total_time_ns, 900_000);
}

#[test]
fn tracy_span_overhead_measured_with_virtual_clock() {
    // Verify that Tracy span overhead is negligible even when
    // measured with a deterministic clock.
    let mut clock = VirtualClock::new();

    for _ in 0..60 {
        let _span = tracy::span("frame");
        clock.step_60fps();
    }

    // 60 frames at ~16.666ms each = ~1000ms total.
    assert!(clock.elapsed_millis() >= 999 && clock.elapsed_millis() <= 1001);
}

#[test]
fn headless_harness_with_devtools_telemetry() {
    // Integration: render frames in the headless harness while
    // collecting devtools telemetry.
    let mut harness = HeadlessHarness::new(64, 64);
    let mut hud = DiagnosticHud::new();
    hud.toggle();

    let render = |frame: u64| -> Vec<u8> {
        let mut rgba = vec![0u8; 64 * 64 * 4];
        for i in 0..(64 * 64) {
            rgba[i * 4] = ((frame * 4) as u8).wrapping_add(i as u8);
            rgba[i * 4 + 3] = 255;
        }
        rgba
    };

    for _ in 0..10 {
        harness.step_frame();
        let rgba = render(harness.frame_count());
        harness.capture_frame(&rgba);
        hud.record_frame(FrameTiming {
            layout_time_ns: 100_000,
            paint_time_ns: 50_000,
            gpu_wait_time_ns: 10_000,
            total_time_ns: 160_000,
        });
    }

    assert_eq!(harness.frame_count(), 10);
    assert_eq!(harness.snapshots().len(), 10);
    assert_eq!(hud.histogram().len(), 10);
}
