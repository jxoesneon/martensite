// widget! property missing the `:` separator should fail.
use martensite_macros::widget;

widget! {
    MyWidget {
        value i32 = 0,
    }
}

fn main() {}
