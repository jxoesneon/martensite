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

use crate::page::{PageMeta, PropValue, PropValues};

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

/// A prop->snippet emission rule for [`prop_snippet`]: how one spec's
/// value renders as a builder call when it differs from default.
#[derive(Clone, Copy, Debug)]
pub(crate) enum SnipProp {
    /// Bool -- emits `.method(v)` when `v != default`.
    Bool(bool),
    /// Float -- emits `.method(v)` when `v != default`.
    Float(f64),
    /// Int -- emits `.method(v)` when `v != default`.
    Int(i64),
    /// Text -- emits `.method("v")` when `v != default`.
    Text(&'static str),
    /// Choice -- the `&'static [&'static str]` maps the selected index
    /// to the literal expression emitted (e.g. `"ChipKind::Filter"`).
    Choice(&'static [&'static str]),
}

/// Emits builder-call snippet lines for every prop that differs from
/// its default -- the uniform mechanism behind generated props. Each
/// rule is `(spec_key, method_name, spec_kind)`.
pub(crate) fn prop_snippet(
    p: &PropValues,
    rules: &[(&'static str, &'static str, SnipProp)],
) -> String {
    let mut s = String::new();
    for (key, method, kind) in rules {
        match (p.get(key), kind) {
            (Some(PropValue::Bool(v)), SnipProp::Bool(d)) if v != d => {
                s.push_str(&format!("\n    {method}({v})"));
            }
            (Some(PropValue::Float(v)), SnipProp::Float(d)) if v != d => {
                s.push_str(&format!("\n    {method}({v:?})"));
            }
            (Some(PropValue::Int(v)), SnipProp::Int(d)) if v != d => {
                s.push_str(&format!("\n    {method}({v})"));
            }
            (Some(PropValue::Text(t)), SnipProp::Text(d)) if t != d => {
                s.push_str(&format!("\n    {method}({t:?})"));
            }
            (Some(PropValue::Choice(i)), SnipProp::Choice(lits)) if *i != 0 => {
                if let Some(lit) = lits.get(*i) {
                    s.push_str(&format!("\n    {method}({lit})"));
                }
            }
            _ => {}
        }
    }
    s
}

/// Parses `"YYYY-MM-DD"` into a [`Date`] for text-driven date props.
pub(crate) fn parse_date(s: &str) -> Option<martensite::widgets::date_picker::Date> {
    let mut it = s.trim().split('-');
    let year: i32 = it.next()?.parse().ok()?;
    let month: u32 = it.next()?.parse().ok()?;
    let day: u32 = it.next()?.parse().ok()?;
    let d = martensite::widgets::date_picker::Date { year, month, day };
    d.is_valid().then_some(d)
}

/// Parses `"HH:MM"` into a [`Time`] for text-driven time props.
pub(crate) fn parse_time(s: &str) -> Option<martensite::widgets::time_picker::Time> {
    let mut it = s.trim().split(':');
    let hour: u32 = it.next()?.parse().ok()?;
    let minute: u32 = it.next()?.parse().ok()?;
    let t = martensite::widgets::time_picker::Time { hour, minute };
    t.is_valid().then_some(t)
}

/// Parses a seconds count into a [`Duration`].
pub(crate) fn parse_secs(s: &str) -> Option<std::time::Duration> {
    s.trim()
        .parse::<f64>()
        .ok()
        .map(std::time::Duration::from_secs_f64)
}

/// Parses `"a,b"` into a float pair.
pub(crate) fn parse_pair(s: &str) -> Option<(f64, f64)> {
    let mut it = s.trim().split(',');
    let a: f64 = it.next()?.trim().parse().ok()?;
    let b: f64 = it.next()?.trim().parse().ok()?;
    Some((a, b))
}

/// Parses `"a,b,c,d"` into a float quad.
pub(crate) fn parse_quad(s: &str) -> Option<(f64, f64, f64, f64)> {
    let v: Vec<f64> = s
        .trim()
        .split(',')
        .filter_map(|t| t.trim().parse().ok())
        .collect();
    match v.as_slice() {
        [a, b, c, d] => Some((*a, *b, *c, *d)),
        _ => None,
    }
}

/// Parses `"r,g,b[,a]"` (0–255 channels) into an RGBA tuple.
pub(crate) fn parse_rgba(s: &str) -> Option<[u8; 4]> {
    let v: Vec<u8> = s
        .trim()
        .split(',')
        .filter_map(|t| t.trim().parse().ok())
        .collect();
    match v.as_slice() {
        [r, g, b] => Some([*r, *g, *b, 255]),
        [r, g, b, a] => Some([*r, *g, *b, *a]),
        _ => None,
    }
}

/// Parses a csv/`;`-separated float list.
pub(crate) fn parse_f32s(s: &str) -> Vec<f32> {
    s.trim()
        .split([',', ';'])
        .filter_map(|t| t.trim().parse().ok())
        .collect()
}

/// Parses `;`-separated `"pos,r,g,b,a"` rows into gradient stops.
pub(crate) fn parse_stops(s: &str) -> Vec<martensite::widgets::gradient_editor::GradientStop> {
    s.trim()
        .split(';')
        .filter_map(|row| {
            let mut it = row.trim().splitn(2, ',');
            let pos: f32 = it.next()?.trim().parse().ok()?;
            let color = parse_rgba(it.next()?)?;
            Some(martensite::widgets::gradient_editor::GradientStop::new(
                pos, color,
            ))
        })
        .collect()
}

/// Parses `;`-separated `"o,h,l,c"` rows into candles.
pub(crate) fn parse_candles(s: &str) -> Vec<martensite::widgets::candlestick::Candle> {
    s.trim()
        .split(';')
        .filter_map(|row| {
            let v: Vec<f32> = row
                .trim()
                .split(',')
                .filter_map(|t| t.trim().parse().ok())
                .collect();
            match v.as_slice() {
                [o, h, l, c] => Some(martensite::widgets::candlestick::Candle::new(
                    *o, *h, *l, *c,
                )),
                _ => None,
            }
        })
        .collect()
}

/// Parses a csv text into a fixed `[String; N]` array; `None` when the
/// count is wrong.
pub(crate) fn str_arr<const N: usize>(s: &str) -> Option<[String; N]> {
    let v: Vec<String> = s.trim().split(',').map(|t| t.trim().to_string()).collect();
    v.try_into().ok()
}

/// Emits `\n    .method(<expr>)` when the prop differs from `default`
/// and `f` renders an expression for it — the snippet counterpart of
/// the text-driven parse props.
pub(crate) fn snip_textmap(
    p: &PropValues,
    key: &str,
    method: &str,
    default: &str,
    f: fn(&str) -> Option<String>,
) -> String {
    let v = p.str(key);
    if v == default {
        return String::new();
    }
    f(v).map(|e| format!("\n    {method}({e})"))
        .unwrap_or_default()
}

/// Snippet expr for a date prop: `Date { year: 2024, month: 1, day: 2 }`.
pub(crate) fn expr_date(s: &str) -> Option<String> {
    let d = parse_date(s)?;
    Some(format!(
        "Date {{ year: {}, month: {}, day: {} }}",
        d.year, d.month, d.day
    ))
}

/// Snippet expr for a time prop: `Time { hour: 9, minute: 30 }`.
pub(crate) fn expr_time(s: &str) -> Option<String> {
    let t = parse_time(s)?;
    Some(format!("Time {{ hour: {}, minute: {} }}", t.hour, t.minute))
}

/// Snippet expr for a seconds prop: `Duration::from_secs_f64(30.0)`.
pub(crate) fn expr_secs(s: &str) -> Option<String> {
    s.trim()
        .parse::<f64>()
        .ok()
        .map(|v| format!("Duration::from_secs_f64({v:?})"))
}

/// Snippet expr for a float pair: `0.0, 100.0`.
pub(crate) fn expr_pair(s: &str) -> Option<String> {
    let (a, b) = parse_pair(s)?;
    Some(format!("{a:?}, {b:?}"))
}

/// Snippet expr for a float quad: `0.0, 0.0, 1.0, 1.0`.
pub(crate) fn expr_quad(s: &str) -> Option<String> {
    let (a, b, c, d) = parse_quad(s)?;
    Some(format!("{a:?}, {b:?}, {c:?}, {d:?}"))
}

/// Snippet expr for an RGBA prop: `[255, 0, 0, 255]`.
pub(crate) fn expr_rgba(s: &str) -> Option<String> {
    let [r, g, b, a] = parse_rgba(s)?;
    Some(format!("[{r}, {g}, {b}, {a}]"))
}

/// Snippet expr for an sRGB→Oklab prop: `Oklab::from_srgb(1.0, 0.0, 0.0)`.
pub(crate) fn expr_oklab(s: &str) -> Option<String> {
    let [r, g, b, _] = parse_rgba(s)?;
    Some(format!(
        "Oklab::from_srgb({:?}, {:?}, {:?})",
        r as f32 / 255.0,
        g as f32 / 255.0,
        b as f32 / 255.0
    ))
}

/// Snippet expr for a float-list prop: `vec![1.0, 2.0]`.
pub(crate) fn expr_f32s(s: &str) -> Option<String> {
    let v = parse_f32s(s);
    (!v.is_empty()).then(|| format!("vec!{v:?}"))
}

/// Snippet expr for a string-list prop: `vec!["a", "b"]`.
pub(crate) fn expr_strs(s: &str) -> Option<String> {
    let v: Vec<String> = s
        .trim()
        .split(',')
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect();
    (!v.is_empty()).then(|| format!("vec!{v:?}"))
}

/// Snippet expr for a `["a", "b"]` fixed array prop.
pub(crate) fn expr_arr(s: &str) -> Option<String> {
    let v: Vec<String> = s
        .trim()
        .split(',')
        .map(|t| format!("{:?}", t.trim()))
        .filter(|t| t != "\"\"")
        .collect();
    (!v.is_empty()).then(|| format!("[{}]", v.join(", ")))
}

/// Parses `;`-separated `sender|body` rows into messages; a `>`-prefixed
/// body marks an outgoing (sent) bubble.
pub(crate) fn parse_messages(s: &str) -> Vec<martensite::widgets::message_list::Message> {
    use martensite::widgets::message_list::Message;
    s.trim()
        .split(';')
        .filter_map(|row| {
            let row = row.trim();
            if row.is_empty() {
                return None;
            }
            match row.split_once('|') {
                Some((sender, body)) => Some(Message::received(sender.trim(), body.trim())),
                None => Some(Message::sent(row.trim_start_matches('>').trim())),
            }
        })
        .collect()
}

/// Snippet expr for a messages prop: `vec![Message::received("A", "b"), …]`.
pub(crate) fn expr_messages(s: &str) -> Option<String> {
    let v = parse_messages(s);
    if v.is_empty() {
        return None;
    }
    let items: Vec<String> = v
        .iter()
        .map(|m| {
            if m.outgoing {
                format!("Message::sent({:?})", m.body)
            } else {
                format!("Message::received({:?}, {:?})", m.sender, m.body)
            }
        })
        .collect();
    Some(format!("vec![{}]", items.join(", ")))
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
