// widget! with a keyword as a property name should fail.
use martensite_macros::widget;

widget! {
    MyWidget {
        type: i32 = 0,
    }
}

fn main() {}
