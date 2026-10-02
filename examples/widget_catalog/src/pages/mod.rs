//! Page registry — one [`Page`] per public widget, grouped by family
//! in rail order.
//!
//! Each page builds the staged widget from [`PropValues`], emits the
//! matching Rust expression for the live snippet, drains the widget's
//! event channels into the log, and reports observable state for the
//! diff logger.

mod charts;
mod containers;
mod controls;
mod data;
mod feedback;
mod input;
mod media;
mod misc;
mod navigation;
mod overlays;
mod visualization;

use martensite::core::Widget;

use crate::page::{PageMeta, PropValues};

/// Declares one catalog page: metadata, prop spec, live builder, and
/// the matching snippet — plus optional `poll`/`state` drains.
macro_rules! page {
    ($ident:ident {
        meta: $meta:expr,
        props: $props:expr,
        build: |$bp:ident| $build:expr,
        snippet: |$sp:ident| $snippet:expr
        $(, poll: |$pp:ident, $po:ident| $poll:block)?
        $(, state: |$dp:ident| $state:block)?
        $(,)?
    }) => {
        pub(super) struct $ident;
        impl crate::page::Page for $ident {
            fn meta(&self) -> crate::page::PageMeta {
                $meta
            }
            fn props(&self) -> &'static [crate::page::PropSpec] {
                $props
            }
            fn build(&self, $bp: &crate::page::PropValues) -> Box<dyn martensite::core::Widget> {
                $build
            }
            fn snippet(&self, $sp: &crate::page::PropValues) -> String {
                $snippet
            }
            $(
                fn poll_events(
                    &self,
                    $pp: &mut dyn martensite::core::Widget,
                    $po: &mut Vec<String>,
                ) $poll
            )?
            $(
                fn describe_state(
                    &self,
                    $dp: &mut dyn martensite::core::Widget,
                ) -> Vec<(String, String)> $state
            )?
        }
    };
}

pub(crate) use page;

/// Reference metadata shorthand.
pub(crate) fn meta(
    name: &'static str,
    family: &'static str,
    desc: &'static str,
    role: &'static str,
    aliases: &'static [(&'static str, &'static str)],
    needs_overlay: bool,
) -> PageMeta {
    PageMeta {
        name,
        family,
        description: desc,
        role,
        aliases,
        needs_overlay,
    }
}

/// Downcasts the staged widget for event/state drains.
pub(crate) fn downcast_mut<T: 'static>(w: &mut dyn Widget) -> Option<&mut T> {
    w.as_any_mut()?.downcast_mut::<T>()
}

/// `Text` prop parsed as comma-separated options.
pub(crate) fn csv(props: &PropValues, key: &str) -> Vec<String> {
    props
        .str(key)
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Maps the shared `"sanitize"` Choice prop to a `SanitizerConfig`:
/// Aggressive (default) / Baseline / Raw.
pub(crate) fn sanitize_cfg(p: &PropValues) -> martensite::sanitize::SanitizerConfig {
    use martensite::sanitize::SanitizerConfig;
    match p.choice("sanitize") {
        1 => SanitizerConfig::Baseline,
        2 => SanitizerConfig::Raw,
        _ => SanitizerConfig::Aggressive,
    }
}

/// The snippet suffix for the `"sanitize"` prop — `.sanitize(false)`
/// or `.raw()`, empty for the aggressive default.
pub(crate) fn sanitize_snippet(p: &PropValues) -> &'static str {
    match p.choice("sanitize") {
        1 => "\n    .sanitize(false)",
        2 => "\n    .raw()",
        _ => "",
    }
}

/// Every registered page, in rail order — grouped by family.
pub fn all_pages() -> Vec<Box<dyn crate::page::Page>> {
    let mut v: Vec<Box<dyn crate::page::Page>> = Vec::new();
    v.extend(controls::pages());
    v.extend(input::pages());
    v.extend(containers::pages());
    v.extend(data::pages());
    v.extend(feedback::pages());
    v.extend(navigation::pages());
    v.extend(overlays::pages());
    v.extend(media::pages());
    v.extend(charts::pages());
    v.extend(visualization::pages());
    v.extend(misc::pages());
    v
}
