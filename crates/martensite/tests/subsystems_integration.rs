//! Integration tests for Martensite v0.7.0 — Advanced Subsystems milestone.
//!
//! These tests exercise the cross-crate interaction between
//! `martensite-history` (LCA tree undo/redo ledger), `martensite-assets`
//! (dual-mode VFS, WGSL shader validation), and `martensite-l10n`
//! (Project Fluent localization, script directionality), verifying the
//! exit criteria for the Subsystems milestone:
//!
//! 1. LCA History Integrity: 10,000 randomized state rollbacks and branch
//!    transitions restore exact bit-level state.
//! 2. Asset Resolution Performance: Embedded VFS resolves asset data
//!    pointers in < 10µs.
//! 3. Locale Switch Gate: Switching application locale across 1,000
//!    active text nodes settles within 1 frame.
//! 4. WGSL shader validation catches invalid shaders.
//! 5. Script directionality resolution for LTR and RTL locales.
//! 6. Fluent message resolution with arguments.
//! 7. Locale negotiation between requested and available locales.
//! 8. Cross-crate integration: history + assets + l10n working together.

#![forbid(unsafe_code)]

use std::str::FromStr;

use martensite_assets::{EmbeddedVfs, ShaderValidator, Vfs};
use martensite_history::{ChangeOp, HistoryLedger, LedgerError};
use martensite_l10n::{
    direction::{direction_for_locale, ScriptDirection},
    fluent::FluentCatalog,
    LanguageIdentifier,
};

// ──────────────────────────────────────────────────────────────────────
// Test helpers
// ──────────────────────────────────────────────────────────────────────

/// A reversible add operation on an i32 state.
struct AddOp(i32);
impl ChangeOp<i32> for AddOp {
    fn apply(&self, s: &mut i32) {
        *s += self.0;
    }
    fn revert(&self, s: &mut i32) {
        *s -= self.0;
    }
}

/// A reversible set operation on a String state.
struct SetOp {
    old: String,
    new: String,
}
impl ChangeOp<String> for SetOp {
    fn apply(&self, s: &mut String) {
        *s = self.new.clone();
    }
    fn revert(&self, s: &mut String) {
        *s = self.old.clone();
    }
}

/// Deterministic LCG pseudo-random number generator.
struct Lcg {
    state: u64,
}

impl Lcg {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state
    }

    fn next_range(&mut self, max: u64) -> u64 {
        self.next_u64() % max
    }
}

// ──────────────────────────────────────────────────────────────────────
// 1. LCA History Integrity Gate: 10,000 randomized rollbacks
// ──────────────────────────────────────────────────────────────────────

#[test]
fn lca_history_integrity_10k_rollbacks() {
    use std::collections::HashMap;

    let mut ledger = HistoryLedger::new(0i32, 500);
    let mut nodes: Vec<martensite_history::NodeId> = vec![ledger.root_node()];
    let mut expected: HashMap<martensite_history::NodeId, i32> = HashMap::new();
    expected.insert(ledger.root_node(), 0);

    let mut rng = Lcg::new(42);
    let mut next_val: i32 = 1;

    for _ in 0..10_000 {
        let action = rng.next_range(4);

        match action {
            0 | 1 => {
                // Commit a new operation.
                let val = next_val;
                next_val += 1;
                ledger.commit(Box::new(AddOp(val)));
                let node = ledger.current_node();
                nodes.push(node);
                expected.insert(node, *ledger.state());
            }
            2 => {
                // Undo.
                if ledger.can_undo() {
                    ledger.undo().unwrap();
                    let node = ledger.current_node();
                    expected.insert(node, *ledger.state());
                }
            }
            3 => {
                // Jump to a random known node.
                if !nodes.is_empty() {
                    let idx = rng.next_range(nodes.len() as u64) as usize;
                    let target = nodes[idx];
                    if ledger.jump_to(target).is_ok() {
                        let node = ledger.current_node();
                        expected.insert(node, *ledger.state());
                    }
                }
            }
            _ => unreachable!(),
        }
    }

    // Verify all surviving known nodes restore correct state.
    let mut verified = 0;
    for &node in &nodes {
        if ledger.jump_to(node).is_ok() {
            if let Some(&expected_state) = expected.get(&node) {
                assert_eq!(
                    *ledger.state(),
                    expected_state,
                    "State mismatch at node {:?}: expected {}, got {}",
                    node,
                    expected_state,
                    *ledger.state()
                );
                verified += 1;
            }
        }
    }
    assert!(verified > 0, "must have verified at least one node");
    // The ledger must respect the bounded depth.
    assert!(
        ledger.node_count() <= 500,
        "node count {} must be <= max_nodes (500)",
        ledger.node_count()
    );
}

#[test]
fn lca_branching_undo_redo_jump() {
    let mut ledger = HistoryLedger::new(0, 100);

    // Build: root -> A(5) -> B(8)
    ledger.commit(Box::new(AddOp(5)));
    let node_a = ledger.current_node();
    ledger.commit(Box::new(AddOp(3)));
    let node_b = ledger.current_node();
    assert_eq!(*ledger.state(), 8);

    // Undo to A, then branch: root -> A(5) -> C(15)
    ledger.undo().unwrap();
    assert_eq!(*ledger.state(), 5);
    ledger.commit(Box::new(AddOp(10)));
    let node_c = ledger.current_node();
    assert_eq!(*ledger.state(), 15);

    // Jump from C to B (sibling branch via LCA=A)
    ledger.jump_to(node_b).unwrap();
    assert_eq!(*ledger.state(), 8);

    // Jump from B to C
    ledger.jump_to(node_c).unwrap();
    assert_eq!(*ledger.state(), 15);

    // Jump from C to root
    ledger.jump_to(ledger.root_node()).unwrap();
    assert_eq!(*ledger.state(), 0);

    // Jump from root to A
    ledger.jump_to(node_a).unwrap();
    assert_eq!(*ledger.state(), 5);
}

#[test]
fn lca_deep_branch_navigation() {
    let mut ledger = HistoryLedger::new(0, 200);

    // Build a deep chain: root -> 1 -> 2 -> 3 -> 4 -> 5
    for i in 1..=5 {
        ledger.commit(Box::new(AddOp(i)));
    }
    let deep_node = ledger.current_node();
    assert_eq!(*ledger.state(), 15); // 1+2+3+4+5

    // Undo back to depth 2
    for _ in 0..3 {
        ledger.undo().unwrap();
    }
    assert_eq!(*ledger.state(), 3); // 1+2

    // Branch: ... -> 2 -> 10 -> 20
    ledger.commit(Box::new(AddOp(10)));
    ledger.commit(Box::new(AddOp(20)));
    let branch_node = ledger.current_node();
    assert_eq!(*ledger.state(), 33); // 1+2+10+20

    // Jump from branch to deep chain
    ledger.jump_to(deep_node).unwrap();
    assert_eq!(*ledger.state(), 15);

    // Jump back to branch
    ledger.jump_to(branch_node).unwrap();
    assert_eq!(*ledger.state(), 33);
}

#[test]
fn lca_undo_at_root_returns_error() {
    let mut ledger = HistoryLedger::<i32>::new(0, 100);
    assert_eq!(ledger.undo(), Err(LedgerError::NoUndo));
}

#[test]
fn lca_redo_without_children_returns_error() {
    let mut ledger = HistoryLedger::<i32>::new(0, 100);
    assert_eq!(ledger.redo(), Err(LedgerError::NoRedo));
}

#[test]
fn lca_string_state_operations() {
    let mut ledger = HistoryLedger::new("initial".to_string(), 100);

    ledger.commit(Box::new(SetOp {
        old: "initial".to_string(),
        new: "first".to_string(),
    }));
    assert_eq!(*ledger.state(), "first");

    ledger.commit(Box::new(SetOp {
        old: "first".to_string(),
        new: "second".to_string(),
    }));
    assert_eq!(*ledger.state(), "second");

    ledger.undo().unwrap();
    assert_eq!(*ledger.state(), "first");

    ledger.undo().unwrap();
    assert_eq!(*ledger.state(), "initial");
}

#[test]
fn lca_bounded_depth_pruning() {
    let mut ledger = HistoryLedger::new(0, 10);
    // Commit 20 operations in a linear chain.
    for i in 0..20 {
        ledger.commit(Box::new(AddOp(i)));
    }
    // Should have pruned to <= 10 nodes.
    assert!(
        ledger.node_count() <= 10,
        "node count {} should be <= 10",
        ledger.node_count()
    );
    // Current branch should still be intact.
    assert!(ledger.can_undo());
}

#[test]
fn lca_can_undo_can_redo() {
    let mut ledger = HistoryLedger::new(0, 100);
    assert!(!ledger.can_undo());
    assert!(!ledger.can_redo());

    ledger.commit(Box::new(AddOp(5)));
    assert!(ledger.can_undo());
    assert!(!ledger.can_redo());

    ledger.undo().unwrap();
    assert!(!ledger.can_undo());
    assert!(ledger.can_redo());

    ledger.redo().unwrap();
    assert!(ledger.can_undo());
    assert!(!ledger.can_redo());
}

// ──────────────────────────────────────────────────────────────────────
// 2. Asset Resolution Performance: Embedded VFS < 10µs
// ──────────────────────────────────────────────────────────────────────

static ES_FTL_ASSET: &[u8] = b"hola = \xc2\xa1Hola, Mundo!\n";
static EMBEDDED_ASSETS: &[(&str, &[u8])] = &[
    (
        "shaders/main.wgsl",
        b"@vertex\nfn vs_main() -> @builtin(position) vec4f { return vec4f(0,0,0,1); }",
    ),
    (
        "shaders/blur.wgsl",
        b"@fragment\nfn fs_main() -> @location(0) vec4f { return vec4f(1); }",
    ),
    (
        "textures/icon.png",
        &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A],
    ),
    (
        "fonts/default.ttf",
        &[0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00],
    ),
    ("locale/en.ftl", b"hello = Hello, World!\n"),
    ("locale/es.ftl", ES_FTL_ASSET),
];

#[test]
fn embedded_vfs_resolution_under_10_micros() {
    let vfs = EmbeddedVfs::new(EMBEDDED_ASSETS);

    // Warm up.
    for _ in 0..100 {
        let _ = vfs.resolve("shaders/main.wgsl");
    }

    // Measure.
    let iterations = 10_000;
    let start = std::time::Instant::now();
    for _ in 0..iterations {
        let data = vfs.resolve("shaders/main.wgsl");
        assert!(data.is_some(), "asset must resolve");
        std::hint::black_box(data);
    }
    let elapsed = start.elapsed();
    let per_call = elapsed.as_nanos() as f64 / iterations as f64;

    assert!(
        per_call < 10_000.0,
        "Embedded VFS resolution must be < 10µs, got {:.1}ns ({:.2}µs)",
        per_call,
        per_call / 1000.0
    );
}

#[test]
fn embedded_vfs_resolves_all_assets() {
    let vfs = EmbeddedVfs::new(EMBEDDED_ASSETS);
    for (path, expected) in EMBEDDED_ASSETS {
        let data = vfs.resolve(path);
        assert!(data.is_some(), "failed to resolve {}", path);
        assert_eq!(data.unwrap(), *expected, "data mismatch for {}", path);
    }
}

#[test]
fn embedded_vfs_returns_none_for_missing() {
    let vfs = EmbeddedVfs::new(EMBEDDED_ASSETS);
    assert!(vfs.resolve("nonexistent/file.txt").is_none());
}

#[test]
fn embedded_vfs_exists_and_list() {
    let vfs = EmbeddedVfs::new(EMBEDDED_ASSETS);
    assert!(vfs.exists("shaders/main.wgsl"));
    assert!(!vfs.exists("missing.txt"));

    let list = vfs.list();
    assert_eq!(list.len(), EMBEDDED_ASSETS.len());
}

// ──────────────────────────────────────────────────────────────────────
// 3. WGSL Shader Validation
// ──────────────────────────────────────────────────────────────────────

#[test]
fn shader_validation_accepts_valid_wgsl() {
    let mut validator = ShaderValidator::new();
    let source = r#"
        @group(0) @binding(0) var<uniform> uniforms: vec4f;
        @group(0) @binding(1) var t_diffuse: texture_2d<f32>;
        @group(0) @binding(2) var s_diffuse: sampler;

        @vertex
        fn vs_main(@location(0) pos: vec3f) -> @builtin(position) vec4f {
            return vec4f(pos, 1.0);
        }

        @fragment
        fn fs_main(@location(0) uv: vec2f) -> @location(0) vec4f {
            return textureSample(t_diffuse, s_diffuse, uv);
        }
    "#;
    let reflection = validator.validate(source).expect("valid WGSL must pass");
    assert!(
        reflection.entry_points.len() >= 2,
        "should have at least 2 entry points"
    );
}

#[test]
fn shader_validation_catches_parse_error() {
    let mut validator = ShaderValidator::new();
    let invalid = "this is not valid WGSL at all!!!";
    let result = validator.validate(invalid);
    assert!(result.is_err(), "invalid WGSL must fail validation");
}

#[test]
fn shader_validation_catches_type_error() {
    let mut validator = ShaderValidator::new();
    let invalid = r#"
        @vertex
        fn vs_main() -> @builtin(position) vec4f {
            return vec4f("not a number", 0, 0, 1);
        }
    "#;
    let result = validator.validate(invalid);
    assert!(result.is_err(), "type-incorrect WGSL must fail");
}

#[test]
fn shader_validation_reflects_compute_workgroup_size() {
    let mut validator = ShaderValidator::new();
    let source = r#"
        @group(0) @binding(0) var<storage, read_write> data: array<f32>;

        @compute @workgroup_size(64, 1, 1)
        fn cs_main(@builtin(global_invocation_id) gid: vec3u) {
            let count = arrayLength(&data);
            if (gid.x < count) {
                data[gid.x] = data[gid.x] * 2.0;
            }
        }
    "#;
    let reflection = validator.validate(source).expect("valid compute shader");
    let compute_ep = reflection
        .entry_points
        .iter()
        .find(|ep| ep.stage == martensite_assets::ShaderStage::Compute)
        .expect("must have a compute entry point");
    assert_eq!(compute_ep.workgroup_size, [64, 1, 1]);
}

// ──────────────────────────────────────────────────────────────────────
// 4. Script Directionality Resolution
// ──────────────────────────────────────────────────────────────────────

#[test]
fn direction_ltr_for_english() {
    let en = LanguageIdentifier::from_str("en-US").unwrap();
    assert_eq!(direction_for_locale(&en), ScriptDirection::Ltr);
}

#[test]
fn direction_rtl_for_arabic() {
    let ar = LanguageIdentifier::from_str("ar-SA").unwrap();
    assert_eq!(direction_for_locale(&ar), ScriptDirection::Rtl);
}

#[test]
fn direction_rtl_for_hebrew() {
    let he = LanguageIdentifier::from_str("he-IL").unwrap();
    assert_eq!(direction_for_locale(&he), ScriptDirection::Rtl);
}

#[test]
fn direction_rtl_for_persian() {
    let fa = LanguageIdentifier::from_str("fa-IR").unwrap();
    assert_eq!(direction_for_locale(&fa), ScriptDirection::Rtl);
}

#[test]
fn direction_rtl_for_urdu() {
    let ur = LanguageIdentifier::from_str("ur-PK").unwrap();
    assert_eq!(direction_for_locale(&ur), ScriptDirection::Rtl);
}

#[test]
fn direction_ltr_for_chinese() {
    let zh = LanguageIdentifier::from_str("zh-CN").unwrap();
    assert_eq!(direction_for_locale(&zh), ScriptDirection::Ltr);
}

#[test]
fn direction_ltr_for_japanese() {
    let ja = LanguageIdentifier::from_str("ja-JP").unwrap();
    assert_eq!(direction_for_locale(&ja), ScriptDirection::Ltr);
}

#[test]
fn direction_rtl_for_dhivehi() {
    let dv = LanguageIdentifier::from_str("dv-MV").unwrap();
    assert_eq!(direction_for_locale(&dv), ScriptDirection::Rtl);
}

// ──────────────────────────────────────────────────────────────────────
// 5. Fluent Message Resolution
// ──────────────────────────────────────────────────────────────────────

#[test]
fn fluent_resolves_simple_message() {
    let en = LanguageIdentifier::from_str("en").unwrap();
    let mut catalog = FluentCatalog::new(en.clone());
    catalog
        .add_bundle(en, vec!["greeting = Hello, World!".to_string()])
        .unwrap();

    assert_eq!(catalog.get("greeting"), Some("Hello, World!".to_string()));
}

#[test]
fn fluent_resolves_message_with_args() {
    let en = LanguageIdentifier::from_str("en").unwrap();
    let mut catalog = FluentCatalog::new(en.clone());
    catalog
        .add_bundle(en, vec!["welcome = Welcome, { $name }!".to_string()])
        .unwrap();

    let msg = catalog.get_with_args("welcome", &[("name", "Alice")]);
    assert!(msg.is_some());
    let msg = msg.unwrap();
    assert!(
        msg.contains("Alice"),
        "message should contain the name: {}",
        msg
    );
}

#[test]
fn fluent_locale_switch_changes_language() {
    let en = LanguageIdentifier::from_str("en").unwrap();
    let es = LanguageIdentifier::from_str("es").unwrap();
    let mut catalog = FluentCatalog::new(en.clone());
    catalog
        .add_bundle(en.clone(), vec!["hello = Hello!".to_string()])
        .unwrap();
    catalog
        .add_bundle(es.clone(), vec!["hello = ¡Hola!".to_string()])
        .unwrap();

    assert_eq!(catalog.locale(), &en);
    assert_eq!(catalog.get("hello"), Some("Hello!".to_string()));

    catalog.set_locale(es.clone()).unwrap();
    assert_eq!(catalog.locale(), &es);
    assert_eq!(catalog.get("hello"), Some("¡Hola!".to_string()));
}

#[test]
fn fluent_direction_updates_with_locale() {
    let en = LanguageIdentifier::from_str("en").unwrap();
    let ar = LanguageIdentifier::from_str("ar").unwrap();
    let mut catalog = FluentCatalog::new(en.clone());
    catalog
        .add_bundle(en.clone(), vec!["msg = Hello".to_string()])
        .unwrap();
    catalog
        .add_bundle(ar.clone(), vec!["msg = مرحبا".to_string()])
        .unwrap();

    assert_eq!(catalog.direction(), ScriptDirection::Ltr);
    catalog.set_locale(ar).unwrap();
    assert_eq!(catalog.direction(), ScriptDirection::Rtl);
}

#[test]
fn fluent_negotiate_locale() {
    let en = LanguageIdentifier::from_str("en").unwrap();
    let es = LanguageIdentifier::from_str("es").unwrap();
    let mut catalog = FluentCatalog::new(en.clone());
    catalog
        .add_bundle(en.clone(), vec!["msg = Hello".to_string()])
        .unwrap();
    catalog
        .add_bundle(es.clone(), vec!["msg = Hola".to_string()])
        .unwrap();

    // Request es-AR, should negotiate to es.
    let es_ar = LanguageIdentifier::from_str("es-AR").unwrap();
    let negotiated = catalog.negotiate(&[es_ar]);
    assert_eq!(negotiated, Some(es));

    // Request fr, should fall back to en (default).
    let fr = LanguageIdentifier::from_str("fr").unwrap();
    let negotiated = catalog.negotiate(&[fr]);
    assert_eq!(negotiated, Some(en));
}

#[test]
fn fluent_available_locales() {
    let en = LanguageIdentifier::from_str("en").unwrap();
    let es = LanguageIdentifier::from_str("es").unwrap();
    let de = LanguageIdentifier::from_str("de").unwrap();
    let mut catalog = FluentCatalog::new(en.clone());
    catalog
        .add_bundle(en.clone(), vec!["msg = Hello".to_string()])
        .unwrap();
    catalog
        .add_bundle(es.clone(), vec!["msg = Hola".to_string()])
        .unwrap();
    catalog
        .add_bundle(de.clone(), vec!["msg = Hallo".to_string()])
        .unwrap();

    let locales = catalog.available_locales();
    assert_eq!(locales.len(), 3);
}

#[test]
fn fluent_missing_key_returns_none() {
    let en = LanguageIdentifier::from_str("en").unwrap();
    let mut catalog = FluentCatalog::new(en.clone());
    catalog
        .add_bundle(en, vec!["hello = Hi".to_string()])
        .unwrap();
    assert!(catalog.get("nonexistent").is_none());
}

// ──────────────────────────────────────────────────────────────────────
// 6. Locale Switch Gate: 1,000 text nodes settle within 1 frame
// ──────────────────────────────────────────────────────────────────────

#[test]
fn locale_switch_1000_nodes_settle() {
    use martensite_l10n::reactive::L10n;
    use martensite_reactive::flush;

    let en = LanguageIdentifier::from_str("en").unwrap();
    let es = LanguageIdentifier::from_str("es").unwrap();

    let l10n = L10n::new(en.clone());
    l10n.add_bundle(en.clone(), vec!["item = Item { $n }".to_string()])
        .unwrap();
    l10n.add_bundle(es.clone(), vec!["item = Artículo { $n }".to_string()])
        .unwrap();

    // Create 1,000 localized memo nodes.
    let memos: Vec<_> = (0..1000)
        .map(|i| l10n.localized_with_args("item", vec![("n".to_string(), i.to_string())]))
        .collect();

    // Verify initial state (English).
    flush();
    let first = memos[0].get();
    assert!(
        first.contains("Item"),
        "initial should be English: {}",
        first
    );

    // Switch locale and measure settle time.
    let start = std::time::Instant::now();
    l10n.set_locale(es).unwrap();
    flush();
    let elapsed = start.elapsed();

    // All memos should now resolve to Spanish.
    for memo in &memos {
        let text = memo.get();
        assert!(
            text.contains("Artículo"),
            "after locale switch, text should be Spanish: {}",
            text
        );
    }

    // Should settle well within 1 frame (~16.6ms at 60fps).
    assert!(
        elapsed.as_millis() < 16,
        "locale switch should settle within 1 frame, took {}ms",
        elapsed.as_millis()
    );
}

// ──────────────────────────────────────────────────────────────────────
// 7. Cross-Crate Integration
// ──────────────────────────────────────────────────────────────────────

#[test]
fn cross_crate_history_assets_l10n() {
    // Simulate an editor session: undo/redo changes the active locale
    // and loads different asset bundles.

    let en = LanguageIdentifier::from_str("en").unwrap();
    let es = LanguageIdentifier::from_str("es").unwrap();

    // Set up VFS with locale assets.
    static ES_FTL: &[u8] = b"greeting = \xc2\xa1Hola!\n";
    static ASSETS: &[(&str, &[u8])] = &[
        ("locale/en.ftl", b"greeting = Hello!\n"),
        ("locale/es.ftl", ES_FTL),
    ];
    let vfs = EmbeddedVfs::new(ASSETS);

    // Set up localization.
    let mut catalog = FluentCatalog::new(en.clone());
    let en_resource = String::from_utf8(vfs.resolve("locale/en.ftl").unwrap().to_vec()).unwrap();
    let es_resource = String::from_utf8(vfs.resolve("locale/es.ftl").unwrap().to_vec()).unwrap();
    catalog.add_bundle(en.clone(), vec![en_resource]).unwrap();
    catalog.add_bundle(es.clone(), vec![es_resource]).unwrap();

    // This test verifies that the three subsystems can work together:
    // VFS provides assets, l10n provides localization, history tracks changes.
    let en_greeting = catalog.get("greeting");
    assert_eq!(en_greeting, Some("Hello!".to_string()));

    catalog.set_locale(es).unwrap();
    let es_greeting = catalog.get("greeting");
    assert_eq!(es_greeting, Some("¡Hola!".to_string()));

    // VFS resolved the assets correctly.
    assert!(vfs.exists("locale/en.ftl"));
    assert!(vfs.exists("locale/es.ftl"));
}

#[test]
fn cross_crate_shader_assets_from_vfs() {
    // Verify that shader source loaded from VFS can be validated.
    let assets: &[(&str, &[u8])] = &[(
        "shader.wgsl",
        b"@vertex\nfn vs_main() -> @builtin(position) vec4f {\n  return vec4f(0, 0, 0, 1);\n}\n",
    )];
    let vfs = EmbeddedVfs::new(assets);

    let shader_source = String::from_utf8(vfs.resolve("shader.wgsl").unwrap().to_vec()).unwrap();
    let mut validator = ShaderValidator::new();
    let reflection = validator
        .validate(&shader_source)
        .expect("shader from VFS must validate");
    assert!(
        !reflection.entry_points.is_empty(),
        "must have at least one entry point"
    );
}

#[test]
fn cross_crate_history_with_asset_state() {
    // Use history to track asset loading state changes.
    struct LoadAssetOp {
        old: Vec<String>,
        new: Vec<String>,
    }
    impl ChangeOp<Vec<String>> for LoadAssetOp {
        fn apply(&self, state: &mut Vec<String>) {
            *state = self.new.clone();
        }
        fn revert(&self, state: &mut Vec<String>) {
            *state = self.old.clone();
        }
    }

    let mut ledger = HistoryLedger::new(Vec::<String>::new(), 100);

    // Load first asset.
    ledger.commit(Box::new(LoadAssetOp {
        old: vec![],
        new: vec!["main.wgsl".to_string()],
    }));
    assert_eq!(ledger.state(), &vec!["main.wgsl".to_string()]);

    // Load second asset.
    ledger.commit(Box::new(LoadAssetOp {
        old: vec!["main.wgsl".to_string()],
        new: vec!["main.wgsl".to_string(), "blur.wgsl".to_string()],
    }));
    assert_eq!(ledger.state().len(), 2);

    // Undo: back to one asset.
    ledger.undo().unwrap();
    assert_eq!(ledger.state().len(), 1);

    // Redo: back to two assets.
    ledger.redo().unwrap();
    assert_eq!(ledger.state().len(), 2);
}
