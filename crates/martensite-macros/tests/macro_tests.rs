//! Integration tests for the `widget!` procedural macro.

use martensite_macros::widget;

// ---------------------------------------------------------------------------
// Simple form (backward compatible)
// ---------------------------------------------------------------------------

widget!(MyTestWidget);

/// Verifies that `widget!` expands to a unit struct implementing `Default`.
#[test]
fn widget_macro_creates_unit_struct() {
    let _widget: MyTestWidget = MyTestWidget;
    let _widget2: MyTestWidget = Default::default();
}

widget!(WidgetA);
widget!(WidgetB);

/// Verifies that `widget!` creates distinct types for different names.
#[test]
fn widget_macro_creates_distinct_types() {
    let _a: WidgetA = WidgetA;
    let _b: WidgetB = WidgetB;
}

// Trailing semicolons are accepted in the simple form.
widget!(WidgetWithSemi;);

/// Verifies the simple form with a trailing semicolon.
#[test]
fn widget_macro_accepts_trailing_semicolon() {
    let _w: WidgetWithSemi = WidgetWithSemi;
}

// ---------------------------------------------------------------------------
// Property form
// ---------------------------------------------------------------------------

widget! {
    Button {
        label: String = String::new(),
        enabled: bool = true,
        count: u32 = 0,
    }
}

/// Verifies the property form generates a struct with fields, a `Default`
/// implementation using the supplied expressions, and accessor methods.
#[test]
fn widget_property_form_basic() {
    let btn = Button::default();
    assert_eq!(btn.label(), "");
    assert!(btn.enabled());
    assert_eq!(btn.count(), &0u32);
}

/// Verifies the `new()` constructor delegates to `Default`.
#[test]
fn widget_property_form_new() {
    let btn = Button::new();
    assert_eq!(btn.label(), "");
    assert!(btn.enabled());
}

/// Verifies the setter methods work.
#[test]
fn widget_property_form_setters() {
    let mut btn = Button::new();
    btn.set_label("Click me".to_string());
    assert_eq!(btn.label(), "Click me");
    btn.set_enabled(false);
    assert!(!btn.enabled());
    btn.set_count(42);
    assert_eq!(btn.count(), &42u32);
}

/// Verifies the mutable accessor methods work.
#[test]
fn widget_property_form_mut_accessors() {
    let mut btn = Button::new();
    *btn.count_mut() = 10;
    assert_eq!(btn.count(), &10u32);
    btn.label_mut().push_str("hi");
    assert_eq!(btn.label(), "hi");
}

widget! {
    Panel {
        title: String = String::from("Untitled"),
        visible: bool = true,
        children: Vec<String> = Vec::new(),
    }
}

/// Verifies the property form works with non-trivial default expressions.
#[test]
fn widget_property_form_nontrivial_defaults() {
    let panel = Panel::default();
    assert_eq!(panel.title(), "Untitled");
    assert!(panel.visible());
    assert!(panel.children().is_empty());
}

// Verifies the property form works with generic types in field types.
widget! {
    Callback {
        on_click: Option<Box<dyn Fn()>> = None,
    }
}

/// Verifies the property form handles generic type annotations.
#[test]
fn widget_property_form_generic_types() {
    let cb = Callback::default();
    assert!(cb.on_click().is_none());
}

// Verifies an empty property block produces a valid (fieldless) struct.
widget! {
    Empty {}
}

#[test]
fn widget_property_form_empty_block() {
    let _e = Empty::default();
    let _e2 = Empty::new();
}

// Verifies the property form works without a trailing comma.
widget! {
    NoTrailingComma {
        value: i32 = 1
    }
}

#[test]
fn widget_property_form_no_trailing_comma() {
    let w = NoTrailingComma::default();
    assert_eq!(w.value(), &1i32);
}

// Verifies the property form handles types with commas inside generics.
widget! {
    MapWidget {
        data: std::collections::HashMap<i32, String> = std::collections::HashMap::new(),
        nested: Vec<Vec<String>> = Vec::new(),
    }
}

#[test]
fn widget_property_form_generic_types_with_commas() {
    let w = MapWidget::default();
    assert!(w.data().is_empty());
    assert!(w.nested().is_empty());
}

// Verifies the property form handles function-pointer types with `->`.
widget! {
    Handler {
        callback: Option<Box<dyn Fn() -> i32>> = None,
    }
}

#[test]
fn widget_property_form_fn_pointer_type() {
    let h = Handler::default();
    assert!(h.callback().is_none());
}
