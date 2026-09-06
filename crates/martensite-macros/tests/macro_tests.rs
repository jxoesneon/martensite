//! Integration tests for the `widget!` procedural macro.

use martensite_macros::widget;

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
