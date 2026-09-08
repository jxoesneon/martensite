// widget! property with no type annotation should fail.
use martensite_macros::widget;

widget! {
    MyWidget {
        value = 0,
    }
}

fn main() {}
