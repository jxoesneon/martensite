// widget! property with no default value should fail.
use martensite_macros::widget;

widget! {
    MyWidget {
        value: i32,
    }
}

fn main() {}
