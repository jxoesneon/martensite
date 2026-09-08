// A valid property-form widget! invocation should compile.
use martensite_macros::widget;

widget! {
    PassProperty {
        label: String = String::new(),
        count: u32 = 0,
    }
}

fn main() {
    let w = PassProperty::new();
    assert_eq!(w.count(), &0u32);
    assert_eq!(w.label(), "");
}
