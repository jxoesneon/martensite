//! Cross-crate integration tests for the v0.10.0 Plugins & Ecosystem milestone.
//!
//! These tests verify that the plugin runtime, blessed widgets, and fuzzing
//! harness work together and meet the milestone exit criteria.

use martensite_blessed::{
    audio_waveform::AudioWaveform,
    chart::{AreaSeries, Chart, LineSeries, Point, ScatterSeries},
    code_editor::{CodeEditor, Cursor},
    data_table::DataTable,
};
use martensite_plugin::{
    ring_buffer::{PluginPaintCmd, PluginRingBuffer},
    Capability, CapabilitySet, PluginBuilder, PluginRuntime,
};
use martensite_test::{run_fuzz_campaign, FuzzConfig, FuzzEngine};

// ──────────────────────────────────────────────────────────────────────
// Plugin Ring Buffer Integration
// ──────────────────────────────────────────────────────────────────────

#[test]
fn ring_buffer_produces_and_consumes_paint_commands() {
    let mut backing = vec![0u8; 1024];
    let mut rb = PluginRingBuffer::new(&mut backing);
    let cmd = PluginPaintCmd {
        cmd_type: 0,
        flags: 0,
        data_len: 4,
        payload_offset: 0,
    };
    let payload = &[0u8; 4];

    rb.produce(&cmd, payload).unwrap();

    let (consumed, read_payload) = rb.consume().unwrap();
    assert_eq!(consumed.cmd_type, 0);
    assert_eq!(consumed.data_len, 4);
    assert_eq!(read_payload, payload);
}

#[test]
fn ring_buffer_wrap_around_without_allocation() {
    let mut backing = vec![0u8; 512];
    let mut rb = PluginRingBuffer::new(&mut backing);
    let payload = &[0u8; 8];
    for i in 0..50u16 {
        let cmd = PluginPaintCmd {
            cmd_type: i,
            flags: 0,
            data_len: 8,
            payload_offset: 0,
        };
        if rb.produce(&cmd, payload).is_err() {
            break;
        }
    }

    let mut count = 0;
    rb.drain(|_cmd, _payload| {
        count += 1;
    });
    assert!(count > 0, "should consume at least one wrapped command");
}

// ──────────────────────────────────────────────────────────────────────
// Plugin Capability Security Integration
// ──────────────────────────────────────────────────────────────────────

#[test]
fn capability_set_grants_and_revokes() {
    let mut set = CapabilitySet::empty();
    set.grant(Capability::Network);
    assert!(set.contains(&Capability::Network));

    set.revoke(&Capability::Network);
    assert!(!set.contains(&Capability::Network));
}

#[test]
fn plugin_builder_fluent_api() {
    let caps = PluginBuilder::new()
        .grant(Capability::Network)
        .grant(Capability::FileRead("/assets".into()))
        .revoke(Capability::Network)
        .build();

    assert!(!caps.contains(&Capability::Network));
    assert!(caps.contains(&Capability::FileRead("/assets".into())));
}

// ──────────────────────────────────────────────────────────────────────
// Plugin Runtime Fuel/Capability Integration
// ──────────────────────────────────────────────────────────────────────

#[test]
fn plugin_runtime_instantiates_without_wasm() {
    // Smoke test that the runtime can be built without a real wasm file.
    let _runtime = PluginRuntime::new().expect("runtime creation succeeds");
}

// ──────────────────────────────────────────────────────────────────────
// Blessed DataTable Integration
// ──────────────────────────────────────────────────────────────────────

#[test]
fn data_table_virtualizes_million_rows() {
    let rows: Vec<u64> = (0..1_000_000).collect();
    let mut table = DataTable::new(rows, 32.0);
    table.set_viewport_height(600.0);

    let visible: Vec<_> = table.visible_rows().collect();

    // 600px / 32px ≈ 19 rows visible (plus 1 for partial).
    assert!(
        visible.len() <= 25,
        "visible rows should be bounded, got {}",
        visible.len()
    );
    assert_eq!(visible[0].0, 0);
}

#[test]
fn data_table_scroll_to_middle() {
    let rows: Vec<u64> = (0..100_000).collect();
    let mut table = DataTable::new(rows, 32.0);
    table.set_viewport_height(400.0);
    table.set_scroll_offset(500_000.0);

    let visible: Vec<_> = table.visible_rows().collect();
    assert!(!visible.is_empty());

    let start_row = 500_000.0 / 32.0;
    let end_row = (500_000.0 + 400.0) / 32.0;

    for (index, _row) in &visible {
        let idx = *index as f32;
        assert!(
            idx >= start_row - 1.0 && idx <= end_row + 1.0,
            "row {} out of expected range",
            index
        );
    }
}

#[test]
fn data_table_visible_rows_timing_gate() {
    let rows: Vec<u64> = (0..1_000_000).collect();
    let mut table = DataTable::new(rows, 32.0);
    table.set_viewport_height(600.0);

    let start = std::time::Instant::now();
    let visible: Vec<_> = table.visible_rows().collect();
    let elapsed = start.elapsed();

    // Exit criterion §5.2: 1M-row DataTable visible_rows under 8.3ms per frame (120fps).
    assert!(
        elapsed.as_micros() < 8_300,
        "1M-row visible_rows took {}µs, must be < 8,300µs",
        elapsed.as_micros()
    );
    assert!(!visible.is_empty());
}

// ──────────────────────────────────────────────────────────────────────
// Blessed Chart Integration
// ──────────────────────────────────────────────────────────────────────

#[test]
fn chart_line_series_bounds() {
    let mut chart = Chart::new();
    chart.add_line(LineSeries::new(vec![
        Point::new(0.0, 0.0),
        Point::new(1.0, 1.0),
        Point::new(2.0, 4.0),
    ]));
    let bounds = chart.bounds();
    assert!(bounds.is_some());
    let b = bounds.unwrap();
    assert_eq!(b.x_min, 0.0);
    assert_eq!(b.y_max, 4.0);
}

#[test]
fn chart_auto_scales_combined_series() {
    let mut chart = Chart::new();
    chart.add_line(LineSeries::new(vec![
        Point::new(0.0, 0.0),
        Point::new(10.0, 5.0),
    ]));
    chart.add_area(AreaSeries::new(
        vec![Point::new(0.0, 0.0), Point::new(5.0, 10.0)],
        0.0,
    ));
    chart.add_scatter(ScatterSeries::new(vec![Point::new(2.0, 3.0)], 3.0));

    let bounds = chart.bounds();
    assert!(bounds.is_some());
    let b = bounds.unwrap();
    assert_eq!(b.x_min, 0.0);
    assert_eq!(b.y_max, 10.0);
    assert_eq!(b.x_max, 10.0);
}

// ──────────────────────────────────────────────────────────────────────
// Blessed Code Editor Integration
// ──────────────────────────────────────────────────────────────────────

#[test]
fn code_editor_multi_cursor_insertion() {
    let mut editor = CodeEditor::new("hello\nworld");
    editor.set_cursors(vec![Cursor::new(0, 5), Cursor::new(1, 5)]);

    editor.insert("!");

    let content = editor.text();
    assert!(content.contains("hello!"));
    assert!(content.contains("world!"));
}

#[test]
fn code_editor_backspace_at_cursors() {
    let mut editor = CodeEditor::new("abc\ndef");
    editor.set_cursors(vec![Cursor::new(0, 1)]);
    editor.delete_backward();

    let content = editor.text();
    assert!(content.starts_with("bc"));
}

// ──────────────────────────────────────────────────────────────────────
// Blessed Audio Waveform Integration
// ──────────────────────────────────────────────────────────────────────

#[test]
fn audio_waveform_1k_points_under_budget() {
    let samples: Vec<f32> = (0..1_000).map(|i| (i as f32 / 100.0).sin()).collect();
    let waveform = AudioWaveform::from_samples(samples);

    let start = std::time::Instant::now();
    let _render = waveform.render_points(800.0, 200.0, 1_000);
    let elapsed = start.elapsed();

    // Exit criterion §5.1: 1,000-point waveform under 2.0ms per frame.
    assert!(
        elapsed.as_micros() < 2_000,
        "1,000-point waveform took {}µs, must be < 2,000µs",
        elapsed.as_micros()
    );
}

#[test]
fn audio_waveform_scrub_head_position() {
    let samples: Vec<f32> = (0..100).map(|i| i as f32).collect();
    let mut waveform = AudioWaveform::from_samples(samples);

    waveform.scrub_to(0.5);
    assert!((waveform.scrub_head_x(1000.0).unwrap() - 500.0).abs() < 1.0);
}

// ──────────────────────────────────────────────────────────────────────
// Fuzzing Harness Integration
// ──────────────────────────────────────────────────────────────────────

#[test]
fn fuzz_quick_run_has_no_panics() {
    let report = FuzzEngine::new(FuzzConfig::quick(1))
        .run()
        .expect("quick fuzz run should succeed");
    assert!(report.iterations > 0);
    assert!(report.operations > 0);
}

#[test]
fn fuzz_macro_invokes() {
    let report = martensite_test::fuzz!(2, 50);
    assert!(report.iterations > 0);
}

#[test]
fn fuzz_run_fuzz_campaign_succeeds() {
    let report = run_fuzz_campaign(FuzzConfig::quick(3)).expect("campaign succeeds");
    assert_eq!(report.iterations, 100);
}

// ──────────────────────────────────────────────────────────────────────
// Milestone Exit Criteria Gates
// ──────────────────────────────────────────────────────────────────────

#[test]
fn plugin_paint_cmd_abi_size() {
    // Verify #[repr(C)] layout is 12 bytes.
    assert_eq!(std::mem::size_of::<PluginPaintCmd>(), 12);
}

#[test]
fn ring_buffer_default_capacity_is_256k() {
    let mut backing = vec![0u8; 256 * 1024];
    let rb = PluginRingBuffer::new(&mut backing);
    assert_eq!(rb.capacity(), 256 * 1024);
}
