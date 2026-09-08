// A valid simple widget! invocation should compile.
use martensite_macros::widget;

widget!(PassSimple);

fn main() {
    let _w = PassSimple::default();
}
