//! Comprehensive unit and integration tests for the DevTools Live Tweak Registry (WP-05b).
//!
//! Validates:
//! - `TweakValue` representation, ranges, color parsing, formatting, and `Tweakable` conversions.
//! - `SourceSpan` and `SourcePatch` formatting (`file:line: .prop(old) -> .prop(new)`).
//! - `TweakRegistry` registration, retrieval, and default value preservation.
//! - Reactive signal bindings (`Signal<T>`, `SignalId`), automatic DAG propagation, and re-assertion across hot reload.
//! - Value updates, transient state tracking (`~` badge), and `reset` / `reset_all`.
//! - Source patch emission for literal write-back.
//! - Thread safety and concurrent modification.
//! - Global registry convenience methods and listener dispatch.

#![cfg(feature = "devtools")]
#![forbid(unsafe_code)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;

use martensite_devtools::tweak::{
    format_source_patch, AsSignalId, SourcePatch, SourceSpan, TweakEntry, TweakRegistry,
    TweakValue, Tweakable,
};
use martensite_reactive::{create_memo, Signal, SignalId};

// ---------------------------------------------------------------------------
// 1. TweakValue Unit Tests
// ---------------------------------------------------------------------------

#[test]
fn test_as_signal_id_and_tweak_entry() {
    let sig_id = SignalId::next();
    assert_eq!(sig_id.as_signal_id(), sig_id);
    let sig_ref = &sig_id;
    assert_eq!(AsSignalId::as_signal_id(&sig_ref), sig_id);

    let sig = Signal::new(100.0f32);
    assert_eq!(sig.as_signal_id(), sig.id());
    assert_eq!(sig.initial_tweak_value(), Some(TweakValue::f32(100.0)));

    let mut entry = TweakEntry::new("test/prop", TweakValue::f32(50.0)).with_signal_id(sig_id);
    assert_eq!(entry.property_or_inferred(), "prop");
    assert!(!entry.is_modified());
    entry.current_value = TweakValue::f32(60.0);
    assert!(entry.is_modified());
    entry.reset();
    assert_eq!(entry.current_value, TweakValue::f32(50.0));
}

#[test]
fn test_tweak_value_f32_and_ranges() {
    let val = TweakValue::f32(12.0);
    assert_eq!(val.as_f32(), Some(12.0));
    assert_eq!(val.type_name(), "f32");
    assert_eq!(val.format_value(), "12.0");
    assert_eq!(val.range_f32(), None);

    let bounded = val.with_range_f32(0.0, 32.0);
    assert_eq!(bounded.range_f32(), Some((0.0, 32.0)));
    assert_eq!(bounded.as_f32(), Some(12.0));

    let direct_range = TweakValue::f32_range(8.5, 4.0, 16.0);
    assert_eq!(direct_range.as_f32(), Some(8.5));
    assert_eq!(direct_range.range_f32(), Some((4.0, 16.0)));
    assert_eq!(direct_range.format_value(), "8.5");
}

#[test]
fn test_tweak_value_f64_and_ranges() {
    let val = TweakValue::f64(100.0);
    assert_eq!(val.as_f64(), Some(100.0));
    assert_eq!(val.type_name(), "f64");
    assert_eq!(val.format_value(), "100.0");
    assert_eq!(val.range_f64(), None);

    let bounded = val.with_range_f64(10.0, 500.0);
    assert_eq!(bounded.range_f64(), Some((10.0, 500.0)));

    let direct_range = TweakValue::f64_range(25.5, 0.0, 100.0);
    assert_eq!(direct_range.as_f64(), Some(25.5));
    assert_eq!(direct_range.range_f64(), Some((0.0, 100.0)));
}

#[test]
fn test_tweak_value_integers_and_booleans() {
    let i = TweakValue::i32(-42);
    assert_eq!(i.as_i32(), Some(-42));
    assert_eq!(i.type_name(), "i32");
    assert_eq!(i.format_value(), "-42");
    assert!(i.is_numeric());

    let u = TweakValue::u32(128);
    assert_eq!(u.as_u32(), Some(128));
    assert_eq!(u.type_name(), "u32");
    assert_eq!(u.format_value(), "128");
    assert!(u.is_numeric());

    let b_true = TweakValue::bool(true);
    assert_eq!(b_true.as_bool(), Some(true));
    assert_eq!(b_true.type_name(), "bool");
    assert_eq!(b_true.format_value(), "true");
    assert!(!b_true.is_numeric());

    let b_false = TweakValue::bool(false);
    assert_eq!(b_false.as_bool(), Some(false));
    assert_eq!(b_false.format_value(), "false");
}

#[test]
fn test_tweak_value_colors() {
    let rgba = TweakValue::color_rgba(255, 128, 0, 255);
    assert_eq!(rgba.as_color(), Some([255, 128, 0, 255]));
    assert_eq!(rgba.type_name(), "color");
    assert!(rgba.is_color());
    assert_eq!(rgba.format_value(), "#ff8000");

    let with_alpha = TweakValue::color_rgba(0, 255, 0, 128);
    assert_eq!(with_alpha.format_value(), "#00ff0080");

    // Hex parsing tests
    let hex3 = TweakValue::color_hex("#f00").expect("hex3 valid");
    assert_eq!(hex3.as_color(), Some([255, 0, 0, 255]));

    let hex4 = TweakValue::color_hex("#0f08").expect("hex4 valid");
    assert_eq!(hex4.as_color(), Some([0, 255, 0, 136]));

    let hex6 = TweakValue::color_hex("#0000ff").expect("hex6 valid");
    assert_eq!(hex6.as_color(), Some([0, 0, 255, 255]));

    let hex8 = TweakValue::color_hex("#11223344").expect("hex8 valid");
    assert_eq!(hex8.as_color(), Some([17, 34, 51, 68]));

    // Without leading '#'
    let no_hash = TweakValue::color_hex("ffffff").expect("no hash valid");
    assert_eq!(no_hash.as_color(), Some([255, 255, 255, 255]));

    // Invalid hex strings
    assert!(TweakValue::color_hex("not_a_hex").is_none());
    assert!(TweakValue::color_hex("#12").is_none());
    assert!(TweakValue::color_hex("#12345").is_none());
}

#[test]
fn test_tweak_value_strings() {
    let s = TweakValue::string("button_title");
    assert_eq!(s.as_str(), Some("button_title"));
    assert_eq!(s.type_name(), "string");
    assert_eq!(s.format_value(), "\"button_title\"");
    assert!(!s.is_numeric());
    assert!(!s.is_color());
}

#[test]
fn test_tweakable_bidirectional_conversions() {
    assert_eq!(16.0f32.to_tweak_value(), TweakValue::f32(16.0));
    assert_eq!(
        f32::try_from_tweak_value(&TweakValue::f32(16.0)),
        Some(16.0f32)
    );

    assert_eq!(32.5f64.to_tweak_value(), TweakValue::f64(32.5));
    assert_eq!(
        f64::try_from_tweak_value(&TweakValue::f64(32.5)),
        Some(32.5f64)
    );

    assert_eq!((-5i32).to_tweak_value(), TweakValue::i32(-5));
    assert_eq!(i32::try_from_tweak_value(&TweakValue::i32(-5)), Some(-5i32));

    assert_eq!(99u32.to_tweak_value(), TweakValue::u32(99));
    assert_eq!(u32::try_from_tweak_value(&TweakValue::u32(99)), Some(99u32));

    assert_eq!(true.to_tweak_value(), TweakValue::bool(true));
    assert_eq!(
        bool::try_from_tweak_value(&TweakValue::bool(true)),
        Some(true)
    );

    let color = [10u8, 20, 30, 255];
    assert_eq!(color.to_tweak_value(), TweakValue::Color(color));
    assert_eq!(
        <[u8; 4]>::try_from_tweak_value(&TweakValue::Color(color)),
        Some(color)
    );

    let string_val = String::from("dialog_header");
    assert_eq!(
        string_val.to_tweak_value(),
        TweakValue::String("dialog_header".to_string())
    );
    assert_eq!(
        String::try_from_tweak_value(&TweakValue::String("dialog_header".to_string())),
        Some("dialog_header".to_string())
    );
}

// ---------------------------------------------------------------------------
// 2. SourceSpan and SourcePatch Unit Tests
// ---------------------------------------------------------------------------

#[test]
fn test_source_span_and_formatting() {
    let span = SourceSpan::new("crates/martensite/src/ui.rs", 142, 5);
    assert_eq!(span.file, "crates/martensite/src/ui.rs");
    assert_eq!(span.line, 142);
    assert_eq!(span.column, 5);
    assert_eq!(span.file_line(), "crates/martensite/src/ui.rs:142");
    assert_eq!(span.display(), "crates/martensite/src/ui.rs:142:5");

    let caller_span = SourceSpan::from_caller();
    assert!(caller_span.line > 0);
    assert!(caller_span.file.ends_with("tweak_registry_tests.rs"));
}

#[test]
fn test_source_patch_formatting() {
    let span = SourceSpan::new("src/ui.rs", 142, 5);
    let patch = SourcePatch::new(span, "padding", "12.0", "16.0");
    assert_eq!(
        patch.format_patch(),
        "src/ui.rs:142: .padding(12.0) -> .padding(16.0)"
    );

    // Strips leading dot cleanly
    let span2 = SourceSpan::new("src/theme.rs", 45, 1);
    let patch2 = SourcePatch::new(span2, ".gap", "4.0", "8.0");
    assert_eq!(
        patch2.format_patch(),
        "src/theme.rs:45: .gap(4.0) -> .gap(8.0)"
    );

    // Free helper function
    let formatted = format_source_patch("src/components.rs", 88, ".margin", "10", "20");
    assert_eq!(
        formatted,
        "src/components.rs:88: .margin(10) -> .margin(20)"
    );
}

// ---------------------------------------------------------------------------
// 3. TweakRegistry Registration and Retrieval
// ---------------------------------------------------------------------------

#[test]
fn test_registry_registration_and_get() {
    let registry = TweakRegistry::new();
    assert!(registry.is_empty());
    assert_eq!(registry.len(), 0);

    // Register initial default
    let pad = registry.register_or_get("button/padding", 12.0f32);
    assert_eq!(pad, 12.0);
    assert_eq!(registry.len(), 1);
    assert!(registry.contains("button/padding"));

    // Querying again returns the existing value
    let pad2 = registry.register_or_get("button/padding", 99.0f32);
    assert_eq!(pad2, 12.0);

    // Check entry metadata
    let entry = registry.get_entry("button/padding").expect("entry present");
    assert_eq!(entry.name, "button/padding");
    assert_eq!(entry.default_value, TweakValue::f32(12.0));
    assert_eq!(entry.current_value, TweakValue::f32(12.0));
    assert!(!entry.is_modified());
    assert!(!entry.is_transient());

    // Alias get_or_register
    let gap = registry.get_or_register("theme/gap", 4.0f32);
    assert_eq!(gap, 4.0);
    assert_eq!(registry.len(), 2);
}

#[test]
fn test_registry_with_source_span() {
    let registry = TweakRegistry::new();
    let span = SourceSpan::new("src/ui.rs", 142, 5);

    let pad =
        registry.register_or_get_with_span("button/padding", 12.0f32, span.clone(), "padding");
    assert_eq!(pad, 12.0);

    let entry = registry.get_entry("button/padding").expect("entry present");
    assert_eq!(entry.source_span, Some(span));
    assert_eq!(entry.property_name.as_deref(), Some("padding"));

    // Before modification, emit_patch returns default -> default patch
    let patch = registry
        .emit_patch("button/padding")
        .expect("patch emitted");
    assert_eq!(patch, "src/ui.rs:142: .padding(12.0) -> .padding(12.0)");
}

// ---------------------------------------------------------------------------
// 4. Value Mutation, Transient Flags, and Reset All
// ---------------------------------------------------------------------------

#[test]
fn test_registry_set_value_and_transient_badge() {
    let registry = TweakRegistry::new();
    registry.register_or_get("ui/elevation", 2.0f32);
    registry.register_or_get("ui/radius", 8.0f32);

    assert_eq!(registry.count_modified(), 0);
    assert!(registry.modified_entries().is_empty());

    // Modify elevation
    let updated = registry.set_value("ui/elevation", 4.0f32);
    assert!(updated);
    assert_eq!(registry.get::<f32>("ui/elevation"), Some(4.0));
    assert_eq!(registry.count_modified(), 1);

    let mod_entries = registry.modified_entries();
    assert_eq!(mod_entries.len(), 1);
    assert_eq!(mod_entries[0].name, "ui/elevation");
    assert!(mod_entries[0].is_modified());
    assert!(mod_entries[0].is_transient());

    // Single reset
    assert!(registry.reset("ui/elevation"));
    assert_eq!(registry.get::<f32>("ui/elevation"), Some(2.0));
    assert_eq!(registry.count_modified(), 0);

    // Modify multiple and reset_all
    registry.set("ui/elevation", 6.0f32);
    registry.set("ui/radius", 16.0f32);
    assert_eq!(registry.count_modified(), 2);

    registry.reset_all();
    assert_eq!(registry.count_modified(), 0);
    assert_eq!(registry.get::<f32>("ui/elevation"), Some(2.0));
    assert_eq!(registry.get::<f32>("ui/radius"), Some(8.0));
}

// ---------------------------------------------------------------------------
// 5. Reactive Signal Integration and Hot-Reload Re-Assertion
// ---------------------------------------------------------------------------

#[test]
fn test_registry_signal_link_and_dag_propagation() {
    let registry = TweakRegistry::new();

    // Create a reactive signal
    let scale = Signal::new(2.0f32);
    let scale_id = scale.id();

    // Create a downstream derived memo
    let multiplied = create_memo({
        let scale = scale.clone();
        move || scale.get() * 10.0
    });
    assert_eq!(multiplied.get(), 20.0);

    // Link signal to tweak registry
    registry.register_signal("app/scale", &scale);
    let entry = registry.get_entry("app/scale").expect("entry present");
    assert_eq!(entry.signal_id, Some(scale_id));
    assert_eq!(entry.default_value, TweakValue::f32(2.0));
    assert_eq!(entry.current_value, TweakValue::f32(2.0));

    // Live tweak value mutation triggers signal set and DAG propagation!
    registry.set_value("app/scale", 5.0f32);
    assert_eq!(scale.get_untracked(), 5.0);
    assert_eq!(multiplied.get(), 50.0);

    // Resetting tweak restores signal to compiled default
    registry.reset("app/scale");
    assert_eq!(scale.get_untracked(), 2.0);
    assert_eq!(multiplied.get(), 20.0);
}

#[test]
fn test_registry_hot_reload_reassertion_by_name() {
    let registry = TweakRegistry::new();

    // 1. Initial app run: register signal and tweak it
    let sig1 = Signal::new(4.0f32);
    registry.register_signal("theme/gap", &sig1);
    registry.set_value("theme/gap", 16.0f32);
    assert_eq!(sig1.get_untracked(), 16.0);
    assert_eq!(registry.count_modified(), 1);

    // 2. Simulated cdylib hot reload:
    // A new module is loaded with a newly instantiated Signal (holding compiled default 4.0),
    // and calls register_signal("theme/gap", &new_sig).
    let sig2 = Signal::new(4.0f32);
    assert_eq!(sig2.get_untracked(), 4.0);

    // Registering with the existing registry re-asserts the live tweak value onto the new signal!
    registry.register_signal("theme/gap", &sig2);
    assert_eq!(sig2.get_untracked(), 16.0);
    assert_eq!(registry.get::<f32>("theme/gap"), Some(16.0));
    assert!(registry.get_entry("theme/gap").unwrap().is_modified());
}

#[test]
fn test_registry_raw_signal_id_and_orphaned_tweaks() {
    let registry = TweakRegistry::new();
    let sig_id = SignalId::next();

    registry.register_signal_id("sensor/temp", sig_id);
    assert_eq!(registry.signal_ids(), vec![sig_id]);

    registry.register_or_get("unbound/padding", 12.0f32);
    let orphaned = registry.orphaned_tweaks();
    assert_eq!(orphaned, vec!["unbound/padding"]);
}

// ---------------------------------------------------------------------------
// 6. Source Patch Emission
// ---------------------------------------------------------------------------

#[test]
fn test_registry_emit_patch() {
    let registry = TweakRegistry::new();
    let span = SourceSpan::new("src/ui.rs", 142, 5);

    // Register with span
    registry.register_or_get_with_span("button/padding", 12.0f32, span, "padding");

    // Modify value
    registry.set_value("button/padding", 16.0f32);

    // Emit patch for named tweak
    let patch = registry
        .emit_patch("button/padding")
        .expect("patch generated");
    assert_eq!(patch, "src/ui.rs:142: .padding(12.0) -> .padding(16.0)");

    // Emit all modified patches
    let all = registry.emit_all_patches();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0], "src/ui.rs:142: .padding(12.0) -> .padding(16.0)");

    // Unmodified entry without span returns None
    registry.register_or_get("no_span", 1.0f32);
    assert!(registry.emit_patch("no_span").is_none());
}

// ---------------------------------------------------------------------------
// 7. Change Listeners
// ---------------------------------------------------------------------------

#[test]
fn test_registry_listeners() {
    let registry = TweakRegistry::new();
    let counter = Arc::new(AtomicUsize::new(0));
    let counter_clone = counter.clone();

    registry.add_listener(move |_name, _val| {
        counter_clone.fetch_add(1, Ordering::SeqCst);
    });

    registry.register_or_get("test/val", 10.0f32);
    assert_eq!(counter.load(Ordering::SeqCst), 0);

    registry.set_value("test/val", 20.0f32);
    assert_eq!(counter.load(Ordering::SeqCst), 1);

    registry.reset_all();
    assert_eq!(counter.load(Ordering::SeqCst), 2);
}

// ---------------------------------------------------------------------------
// 8. Thread Safety and Concurrent Access
// ---------------------------------------------------------------------------

#[test]
fn test_registry_concurrent_access() {
    let registry = Arc::new(TweakRegistry::new());
    registry.register_or_get("shared_key", 0i32);

    let mut handles = Vec::new();
    for i in 0..10 {
        let reg = Arc::clone(&registry);
        handles.push(thread::spawn(move || {
            for j in 0..50 {
                reg.set_value("shared_key", i * 100 + j);
                let _ = reg.get::<i32>("shared_key");
            }
        }));
    }

    for h in handles {
        h.join().expect("thread finished cleanly");
    }

    assert!(registry.get::<i32>("shared_key").is_some());
}

// ---------------------------------------------------------------------------
// 9. Global Registry Convenience Functions
// ---------------------------------------------------------------------------

#[test]
fn test_global_registry_methods() {
    use martensite_devtools::tweak::{
        clear, emit_patch, get_or_register, register_or_get, register_signal, reset_all, set_value,
    };

    clear();

    let val = register_or_get("global/test_val", 5.0f32);
    assert_eq!(val, 5.0);

    let val2 = get_or_register("global/test_val", 99.0f32);
    assert_eq!(val2, 5.0);

    let sig = Signal::new(10.0f32);
    register_signal("global/sig", &sig);
    set_value("global/sig", 25.0f32);
    assert_eq!(sig.get_untracked(), 25.0);

    reset_all();
    assert_eq!(sig.get_untracked(), 10.0);

    let span = SourceSpan::new("src/main.rs", 10, 1);
    martensite_devtools::tweak::global().register_or_get_with_span(
        "global/pad",
        8.0f32,
        span,
        "padding",
    );
    set_value("global/pad", 14.0f32);
    let patch = emit_patch("global/pad").expect("patch generated");
    assert_eq!(patch, "src/main.rs:10: .padding(8.0) -> .padding(14.0)");

    clear();
}
