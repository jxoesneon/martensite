//! {{project_name}} — minimal Martensite entry point.

use martensite::prelude::*;

fn main() {
    // Dev channel: enable with MARTENSITE_DEV_CHANNEL=1 and the martensite
    // "dev-channel" feature.
    let _app = App::build().build();
    println!("Hello from {{project_name}}!");
}
