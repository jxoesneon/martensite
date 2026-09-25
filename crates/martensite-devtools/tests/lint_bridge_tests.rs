//! Integration and unit tests for the Runtime Design-Lint Bridge (`LintBridge`).

use kurbo::Rect;
use martensite_core::PaintList;
use martensite_design_lint::{LintConfig, Standard};
use martensite_devtools::lint_bridge::{
    LintBadgeSummary, LintBridge, LintDump, LintDumpError, DUMP_MAGIC, DUMP_VERSION,
};

fn create_test_paint_list_with_finding() -> PaintList {
    let mut list = PaintList::new();
    list.push_scope(None, "App", Rect::new(0.0, 0.0, 800.0, 600.0));
    list.push_scope(None, "Header", Rect::new(0.0, 0.0, 800.0, 50.0));
    // 20×16px control is below the WCAG 24×24pt target-size threshold
    list.push_scope(None, "Button", Rect::new(10.0, 10.0, 30.0, 26.0));
    list.pop_scope();
    list.pop_scope();
    list.pop_scope();
    list
}

fn create_nested_test_paint_list() -> PaintList {
    let mut list = PaintList::new();
    list.push_scope(None, "App", Rect::new(0.0, 0.0, 1000.0, 800.0));
    list.push_scope(None, "Sidebar", Rect::new(0.0, 0.0, 200.0, 800.0));
    list.push_scope(None, "IconButton", Rect::new(10.0, 10.0, 25.0, 25.0)); // 15×15px < 24px
    list.pop_scope();
    list.pop_scope();
    list.push_scope(None, "Content", Rect::new(200.0, 0.0, 1000.0, 800.0));
    list.push_scope(None, "MiniToggle", Rect::new(210.0, 10.0, 225.0, 25.0)); // 15×15px < 24px
    list.pop_scope();
    list.pop_scope();
    list.pop_scope();
    list
}

#[test]
fn test_bridge_initial_state() {
    let bridge = LintBridge::default();
    assert!(bridge.is_enabled());
    assert_eq!(bridge.relint_count(), 0);
    assert_eq!(bridge.skipped_count(), 0);
    assert_eq!(bridge.frames_evaluated(), 0);
    assert!(bridge.report().is_none());
    assert!(bridge.scene().is_none());
    assert!(bridge.last_fingerprint().is_none());
    assert_eq!(bridge.badge_summary().total(), 0);
    assert_eq!(bridge.badge_summary().badge_text(), "lint: clean");
}

#[test]
fn test_on_frame_clean_report() {
    let mut bridge = LintBridge::new(LintConfig::new());
    let list = PaintList::new();

    let report = bridge.on_frame(&list);
    assert!(report.is_some());
    assert!(report.unwrap().is_clean());
    assert_eq!(bridge.relint_count(), 1);
    assert_eq!(bridge.skipped_count(), 0);
    assert_eq!(bridge.frames_evaluated(), 1);
    assert!(bridge.scene().is_some());
    assert!(bridge.report().is_some());
    assert!(bridge.last_fingerprint().is_some());
    assert!(bridge.badge_summary().is_clean());
}

#[test]
fn test_fingerprint_caching_skips_relint() {
    let mut bridge = LintBridge::new(LintConfig::new());
    let list = create_test_paint_list_with_finding();

    // Frame 1: Initial evaluation
    let report1 = bridge.on_frame(&list).expect("first frame evaluation");
    let initial_findings_count = report1.findings.len();
    assert_eq!(bridge.relint_count(), 1);
    assert_eq!(bridge.skipped_count(), 0);
    assert_eq!(bridge.frames_evaluated(), 1);

    // Frame 2: Identical paint list must skip re-evaluation (O(frame-change))
    let report2 = bridge.on_frame(&list).expect("second frame evaluation");
    assert_eq!(report2.findings.len(), initial_findings_count);
    assert_eq!(
        bridge.relint_count(),
        1,
        "relint_count must remain 1 for unchanged frame"
    );
    assert_eq!(
        bridge.skipped_count(),
        1,
        "skipped_count must increment for unchanged frame"
    );
    assert_eq!(bridge.frames_evaluated(), 2);

    // Frame 3: Still identical
    let report3 = bridge.on_frame(&list).expect("third frame evaluation");
    assert_eq!(report3.findings.len(), initial_findings_count);
    assert_eq!(bridge.relint_count(), 1);
    assert_eq!(bridge.skipped_count(), 2);
    assert_eq!(bridge.frames_evaluated(), 3);
}

#[test]
fn test_scene_mutation_triggers_relint() {
    let mut bridge = LintBridge::new(LintConfig::new());
    let list1 = create_test_paint_list_with_finding();

    bridge.on_frame(&list1);
    assert_eq!(bridge.relint_count(), 1);
    let fp1 = bridge.last_fingerprint().unwrap();

    // Create a modified paint list (e.g. Button geometry changed)
    let mut list2 = PaintList::new();
    list2.push_scope(None, "App", Rect::new(0.0, 0.0, 800.0, 600.0));
    list2.push_scope(None, "Header", Rect::new(0.0, 0.0, 800.0, 50.0));
    // Valid target size: 40×40px >= 24px
    list2.push_scope(None, "Button", Rect::new(10.0, 10.0, 50.0, 50.0));
    list2.pop_scope();
    list2.pop_scope();
    list2.pop_scope();

    let is_clean = bridge.on_frame(&list2).expect("second frame").is_clean();
    assert_eq!(
        bridge.relint_count(),
        2,
        "relint_count must increment on scene change"
    );
    let fp2 = bridge.last_fingerprint().unwrap();
    assert_ne!(fp1, fp2, "fingerprints must differ on geometry mutation");
    assert!(is_clean);
}

#[test]
fn test_zero_cost_when_disabled() {
    let mut bridge = LintBridge::new(LintConfig::new()).with_enabled(false);
    let list = create_test_paint_list_with_finding();

    let report = bridge.on_frame(&list);
    assert!(report.is_none(), "disabled bridge must return None");
    assert_eq!(bridge.relint_count(), 0);
    assert_eq!(bridge.skipped_count(), 0);
    assert_eq!(bridge.frames_evaluated(), 0);
    assert!(bridge.scene().is_none());
    assert!(bridge.report().is_none());

    // Re-enabling allows frames to be evaluated
    bridge.set_enabled(true);
    let report_enabled = bridge.on_frame(&list);
    assert!(report_enabled.is_some());
    assert_eq!(bridge.relint_count(), 1);
    assert_eq!(bridge.frames_evaluated(), 1);
}

#[test]
fn test_findings_for_path_and_subtree() {
    let mut bridge = LintBridge::new(LintConfig::new());
    let list = create_nested_test_paint_list();

    bridge.on_frame(&list);
    assert_eq!(bridge.relint_count(), 1);

    // Sidebar subtree findings
    let sidebar_findings = bridge.findings_for("App/Sidebar");
    assert_eq!(sidebar_findings.len(), 2);
    assert!(sidebar_findings
        .iter()
        .all(|f| f.path == "App/Sidebar/IconButton"));

    // Exact node findings for IconButton
    let button_findings = bridge.findings_for("App/Sidebar/IconButton");
    assert_eq!(button_findings.len(), 2);

    // Exact findings method
    let exact_hits = bridge.exact_findings_for("App/Sidebar/IconButton");
    assert_eq!(exact_hits.len(), 2);
    let exact_miss = bridge.exact_findings_for("App/Sidebar");
    assert_eq!(
        exact_miss.len(),
        0,
        "Sidebar has no direct finding, only child"
    );

    // Content subtree findings
    let content_findings = bridge.findings_for("App/Content");
    assert_eq!(content_findings.len(), 2);
    assert!(content_findings
        .iter()
        .all(|f| f.path == "App/Content/MiniToggle"));

    // Root subtree includes all findings
    let app_findings = bridge.findings_for("App");
    assert_eq!(app_findings.len(), 5);

    // Non-existent path returns empty
    let other_findings = bridge.findings_for("App/Footer");
    assert!(other_findings.is_empty());
}

#[test]
fn test_hud_badge_summary() {
    let mut bridge = LintBridge::new(LintConfig::new());
    let list = create_test_paint_list_with_finding();

    bridge.on_frame(&list);
    let summary = bridge.badge_summary();

    assert!(!summary.is_clean());
    assert!(summary.total() > 0);
    assert!(summary.warnings > 0 || summary.errors > 0);

    // Verify format logic of LintBadgeSummary
    let custom_summary = LintBadgeSummary {
        errors: 3,
        warnings: 12,
        infos: 0,
    };
    assert_eq!(custom_summary.badge_text(), "lint: 12w 3e");

    let warn_only = LintBadgeSummary {
        errors: 0,
        warnings: 5,
        infos: 1,
    };
    assert_eq!(warn_only.badge_text(), "lint: 5w");

    let error_only = LintBadgeSummary {
        errors: 2,
        warnings: 0,
        infos: 0,
    };
    assert_eq!(error_only.badge_text(), "lint: 2e");

    let info_only = LintBadgeSummary {
        errors: 0,
        warnings: 0,
        infos: 4,
    };
    assert_eq!(info_only.badge_text(), "lint: 4i");

    let clean = LintBadgeSummary::default();
    assert_eq!(clean.badge_text(), "lint: clean");
}

#[test]
fn test_config_reload_invalidates_fingerprint() {
    let mut bridge = LintBridge::new(LintConfig::new());
    let list = create_test_paint_list_with_finding();

    bridge.on_frame(&list);
    assert_eq!(bridge.relint_count(), 1);

    // Modify config to disable all standards except WCAG
    let mut new_config = LintConfig::new().only_standards(&[Standard::Wcag]);
    new_config.scale_factor = 2.0;
    bridge.set_config(new_config);

    assert!(bridge.last_fingerprint().is_none());

    // Feeding the same paint list must re-lint because config changed
    bridge.on_frame(&list);
    assert_eq!(
        bridge.relint_count(),
        2,
        "config change must force re-evaluation"
    );
}

#[test]
fn test_dump_json_roundtrip() {
    let mut bridge = LintBridge::new(LintConfig::new());
    let list = create_test_paint_list_with_finding();

    bridge.on_frame(&list);

    let json = bridge.dump_json().expect("JSON dump creation");
    assert!(json.contains("\"version\": 1"));
    assert!(json.contains("\"Button\""));

    let loaded_dump = LintDump::from_json(&json).expect("JSON dump load");
    assert_eq!(loaded_dump.version, DUMP_VERSION);

    let restored_scene = loaded_dump.to_scene();
    let restored_report = loaded_dump.to_report();

    assert_eq!(restored_scene.roots.len(), 1);
    assert_eq!(restored_scene.roots[0].name, "App");
    assert_eq!(
        restored_report.findings.len(),
        bridge.report().unwrap().findings.len()
    );
}

#[test]
fn test_dump_binary_roundtrip() {
    let mut bridge = LintBridge::new(LintConfig::new());
    let list = create_test_paint_list_with_finding();

    bridge.on_frame(&list);

    let bin = bridge.dump_binary().expect("binary dump creation");
    assert!(bin.starts_with(DUMP_MAGIC));
    assert!(bin.len() > 12);

    let loaded_dump = LintDump::from_binary(&bin).expect("binary dump load");
    assert_eq!(loaded_dump.version, DUMP_VERSION);

    let restored_report = loaded_dump.to_report();
    assert_eq!(
        restored_report.findings.len(),
        bridge.report().unwrap().findings.len()
    );

    // Corrupted header checks
    let mut bad_magic = bin.clone();
    bad_magic[0] = b'X';
    assert!(matches!(
        LintDump::from_binary(&bad_magic),
        Err(LintDumpError::InvalidMagic)
    ));

    let mut bad_version = bin.clone();
    bad_version[4] = 99;
    assert!(matches!(
        LintDump::from_binary(&bad_version),
        Err(LintDumpError::UnsupportedVersion(99))
    ));

    let truncated = &bin[0..10];
    assert!(matches!(
        LintDump::from_binary(truncated),
        Err(LintDumpError::CorruptedPayload)
    ));
}

#[test]
fn test_dump_to_file_and_load() {
    let temp_dir = std::env::temp_dir().join(format!("martensite_test_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&temp_dir);

    let json_file = temp_dir.join("dump.json");
    let bin_file = temp_dir.join("dump.bin");

    let mut bridge = LintBridge::new(LintConfig::new());
    let list = create_test_paint_list_with_finding();
    bridge.on_frame(&list);

    // 1. JSON file
    bridge.dump_to_file(&json_file).expect("dump to JSON file");
    assert!(json_file.exists());

    let (json_scene, json_report) =
        LintBridge::load_dump_file(&json_file).expect("load from JSON file");
    assert_eq!(json_scene.roots.len(), 1);
    assert_eq!(
        json_report.findings.len(),
        bridge.report().unwrap().findings.len()
    );

    // 2. Binary file
    bridge.dump_to_file(&bin_file).expect("dump to binary file");
    assert!(bin_file.exists());

    let (bin_scene, bin_report) =
        LintBridge::load_dump_file(&bin_file).expect("load from binary file");
    assert_eq!(bin_scene.roots.len(), 1);
    assert_eq!(
        bin_report.findings.len(),
        bridge.report().unwrap().findings.len()
    );

    // Cleanup
    let _ = std::fs::remove_file(json_file);
    let _ = std::fs::remove_file(bin_file);
    let _ = std::fs::remove_dir(temp_dir);
}

#[test]
fn test_env_dump_trigger() {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let temp_dir =
        std::env::temp_dir().join(format!("martensite_env_test_{}_{}", std::process::id(), ts));
    let _ = std::fs::create_dir_all(&temp_dir);
    let dump_file = temp_dir.join("env_dump.bin");

    let mut bridge = LintBridge::new(LintConfig::new()).with_dump_path(&dump_file);
    let list = create_test_paint_list_with_finding();
    bridge.on_frame(&list);

    assert!(
        dump_file.exists(),
        "dump file must be written via dump path"
    );

    let (scene, report) = LintBridge::load_dump_file(&dump_file).expect("load env dump file");
    assert_eq!(scene.roots.len(), 1);
    assert_eq!(
        report.findings.len(),
        bridge.report().unwrap().findings.len()
    );

    // Clean up
    let _ = std::fs::remove_file(dump_file);
    let _ = std::fs::remove_dir(temp_dir);
}
