//! Integration tests for `#[tweak]` procedural macro and source-span infrastructure.
//!
//! Validates:
//! 1. `#[tweak]` attribute macro on variables, fields, and functions.
//! 2. Numeric (`f32`, `f64`, `u32`, `i32`), boolean (`bool`), and color string literals.
//! 3. Zero-overhead direct literal expansion when devtools is disabled.
//! 4. Devtools tweak registry integration (queries and registrations) when devtools is enabled.
//! 5. Source-span tracking and source patch formatting (`src/ui.rs:142: .padding(12.0) -> .padding(16.0)`).

#![allow(clippy::assertions_on_constants)]

use std::collections::HashMap;
use std::sync::Mutex;

use martensite_macros::{source_span, tweak, widget};

// ---------------------------------------------------------------------------
// Mock devtools registry for integration testing
// ---------------------------------------------------------------------------

static QUERIED_TWEAKS: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);
static REGISTERED_SIGNALS: Mutex<Option<Vec<String>>> = Mutex::new(None);

/// Mock devtools crate for integration testing.
pub mod martensite_devtools {
    /// Mock tweak module.
    pub mod tweak {
        use super::super::{QUERIED_TWEAKS, REGISTERED_SIGNALS};
        use std::collections::HashMap;

        /// Mock get_or_register function.
        pub fn get_or_register<T: std::fmt::Display + Clone>(name: &str, default: T) -> T {
            let mut guard = QUERIED_TWEAKS.lock().unwrap();
            if guard.is_none() {
                *guard = Some(HashMap::new());
            }
            guard
                .as_mut()
                .unwrap()
                .insert(name.to_string(), format!("{default}"));
            default
        }

        /// Mock register_signal function.
        pub fn register_signal<S>(name: &str, _sig: &S) {
            let mut guard = REGISTERED_SIGNALS.lock().unwrap();
            if guard.is_none() {
                *guard = Some(Vec::new());
            }
            guard.as_mut().unwrap().push(name.to_string());
        }
    }
}

/// Mock reactive Signal for testing signal tweak registration.
struct MockSignal<T>(T);

impl<T> MockSignal<T> {
    pub fn new(val: T) -> Self {
        Self(val)
    }

    pub fn get(&self) -> &T {
        &self.0
    }
}

// ---------------------------------------------------------------------------
// 1. Literal type support on variables (zero-overhead when devtools disabled)
// ---------------------------------------------------------------------------

#[tweak("ui/scale", 1.5f32, devtools = false)]
const SCALE: f32 = 1.5f32;

#[test]
fn test_numeric_f32_tweak_disabled() {
    assert_eq!(SCALE, 1.5f32);
}

#[tweak("math/factor", 42.5849625f64, devtools = false)]
const FACTOR: f64 = 42.5849625f64;

#[test]
fn test_numeric_f64_tweak_disabled() {
    assert_eq!(FACTOR, 42.5849625f64);
}

#[tweak("limit/max_items", 100u32, devtools = false)]
const MAX_ITEMS: u32 = 100u32;

#[test]
fn test_numeric_u32_tweak_disabled() {
    assert_eq!(MAX_ITEMS, 100u32);
}

#[tweak("offset/x", -10i32, devtools = false)]
const OFFSET_X: i32 = -10i32;

#[test]
fn test_numeric_i32_tweak_disabled() {
    assert_eq!(OFFSET_X, -10i32);
}

#[tweak("feature/active", true, devtools = false)]
const ACTIVE: bool = true;

#[tweak("feature/dark_mode", false, devtools = false)]
const DARK_MODE: bool = false;

#[test]
fn test_bool_tweak_disabled() {
    assert!(ACTIVE);
    assert!(!DARK_MODE);
}

#[tweak("theme/accent", "#3498db", devtools = false)]
const ACCENT: &'static str = "#3498db";

#[tweak("theme/danger", "rgb(231, 76, 60)", devtools = false)]
const DANGER: &'static str = "rgb(231, 76, 60)";

#[test]
fn test_color_string_tweak_disabled() {
    assert_eq!(ACCENT, "#3498db");
    assert_eq!(DANGER, "rgb(231, 76, 60)");
}

#[tweak("config/static_padding", 16.0f32, devtools = false)]
static STATIC_PADDING: f32 = 16.0f32;

#[test]
fn test_static_tweak_disabled() {
    assert_eq!(STATIC_PADDING, 16.0f32);
}

// ---------------------------------------------------------------------------
// 2. Functions with tweaks
// ---------------------------------------------------------------------------

#[tweak("layout/offset", 8.0f32, devtools = false)]
fn get_offset_disabled() -> f32 {
    8.0f32
}

#[test]
fn test_function_tweak_disabled() {
    assert_eq!(get_offset_disabled(), 8.0f32);
}

#[tweak("layout/dynamic_gap", 16.0f32, devtools = true)]
fn get_dynamic_gap_enabled() -> f32 {
    16.0f32
}

#[test]
fn test_function_tweak_enabled() {
    let val = get_dynamic_gap_enabled();
    assert_eq!(val, 16.0f32);

    let guard = QUERIED_TWEAKS.lock().unwrap();
    let map = guard.as_ref().expect("registry should have been queried");
    assert_eq!(map.get("layout/dynamic_gap"), Some(&"16".to_string()));
}

// ---------------------------------------------------------------------------
// 3. Structs with field tweaks
// ---------------------------------------------------------------------------

#[tweak(devtools = false)]
struct DisabledStyleConfig {
    #[tweak("style/padding", 12.0f32)]
    padding: f32,
    #[tweak("style/color", "#ff0000")]
    color: &'static str,
    #[tweak("style/active", true)]
    active: bool,
}

#[test]
fn test_struct_fields_tweak_disabled() {
    let style = DisabledStyleConfig::default();
    assert_eq!(style.padding, 12.0f32);
    assert_eq!(style.color, "#ff0000");
    assert!(style.active);
}

#[tweak(devtools = true)]
struct EnabledStyleConfig {
    #[tweak("style/enabled_padding", 24.0f32)]
    padding: f32,
    #[tweak("style/enabled_color", "#00ffcc")]
    color: &'static str,
}

#[test]
fn test_struct_fields_tweak_enabled() {
    let style = EnabledStyleConfig::default();
    assert_eq!(style.padding, 24.0f32);
    assert_eq!(style.color, "#00ffcc");

    let guard = QUERIED_TWEAKS.lock().unwrap();
    let map = guard.as_ref().expect("registry should have been queried");
    assert_eq!(map.get("style/enabled_padding"), Some(&"24".to_string()));
    assert_eq!(map.get("style/enabled_color"), Some(&"#00ffcc".to_string()));
}

// ---------------------------------------------------------------------------
// 4. Devtools registry integration (queries and signal registration)
// ---------------------------------------------------------------------------

#[tweak("button/min_width", 120u32, devtools = true)]
fn get_min_width() -> u32 {
    120u32
}

#[test]
fn test_variable_registry_query_when_devtools_enabled() {
    let min_width = get_min_width();
    assert_eq!(min_width, 120u32);

    let guard = QUERIED_TWEAKS.lock().unwrap();
    let map = guard.as_ref().expect("registry should have been queried");
    assert_eq!(map.get("button/min_width"), Some(&"120".to_string()));
}

#[tweak("theme/gap-scale", devtools = true)]
fn create_gap_signal() -> MockSignal<f32> {
    MockSignal::new(4.0f32)
}

#[test]
fn test_signal_registration_when_devtools_enabled() {
    let gap = create_gap_signal();
    assert_eq!(*gap.get(), 4.0f32);

    let guard = REGISTERED_SIGNALS.lock().unwrap();
    let list = guard.as_ref().expect("signal should have been registered");
    assert!(list.contains(&"theme/gap-scale".to_string()));
}

// ---------------------------------------------------------------------------
// 5. Source-span infrastructure & patch formatting
// ---------------------------------------------------------------------------

/// Source span structure for test assertions.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SourceSpan {
    pub file: String,
    pub line: u32,
    pub column: u32,
}

impl SourceSpan {
    pub fn new(file: &str, line: u32, column: u32) -> Self {
        Self {
            file: file.to_string(),
            line,
            column,
        }
    }

    pub fn file_line(&self) -> String {
        format!("{}:{}", self.file, self.line)
    }

    pub fn display(&self) -> String {
        format!("{}:{}:{}", self.file, self.line, self.column)
    }
}

/// Source patch structure for test assertions.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SourcePatch {
    pub span: SourceSpan,
    pub method: String,
    pub old_val: String,
    pub new_val: String,
}

impl SourcePatch {
    pub fn new(span: SourceSpan, method: &str, old_val: &str, new_val: &str) -> Self {
        Self {
            span,
            method: method.trim_start_matches('.').to_string(),
            old_val: old_val.to_string(),
            new_val: new_val.to_string(),
        }
    }

    pub fn format_patch(&self) -> String {
        format!(
            "{}:{}: .{}({}) -> .{}({})",
            self.span.file, self.span.line, self.method, self.old_val, self.method, self.new_val
        )
    }
}

#[test]
fn test_source_span_representation() {
    let span = SourceSpan::new("src/ui.rs", 142, 5);
    assert_eq!(span.file_line(), "src/ui.rs:142");
    assert_eq!(span.display(), "src/ui.rs:142:5");
}

#[test]
fn test_source_patch_formatting() {
    let span = SourceSpan::new("src/ui.rs", 142, 5);
    let patch = SourcePatch::new(span, "padding", "12.0", "16.0");
    assert_eq!(
        patch.format_patch(),
        "src/ui.rs:142: .padding(12.0) -> .padding(16.0)"
    );
}

#[source_span]
fn sample_tracked_function() -> &'static str {
    "ok"
}

#[test]
fn test_source_span_attribute_macro() {
    assert_eq!(sample_tracked_function(), "ok");
}

// ---------------------------------------------------------------------------
// 6. Widget macro integration with builder methods and source span patches
// ---------------------------------------------------------------------------

widget! {
    TweakableBox {
        padding: f32 = 12.0,
        margin: f32 = 8.0,
    }
}

#[test]
fn test_widget_macro_with_builder_and_patch() {
    let b = TweakableBox::new().with_padding(16.0).with_margin(4.0);
    assert_eq!(b.padding(), &16.0f32);
    assert_eq!(b.margin(), &4.0f32);

    // Verify source patch generator
    let patch = TweakableBox::emit_patch("padding", "12.0", "16.0", "src/ui.rs", 142);
    assert_eq!(patch, "src/ui.rs:142: .padding(12.0) -> .padding(16.0)");
}
