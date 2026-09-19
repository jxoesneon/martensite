//! `Markdown` — read-only rich-text display for a pragmatic Markdown
//! subset (Ant `Typography`, Qt `QTextEdit` markdown mode, SwiftUI
//! `AttributedString(markdown:)`).
//!
//! The parser is hand-rolled — no external markdown crates. Supported
//! block syntax (blocks split on blank lines):
//!
//! - `#`–`######` headings — rendered larger and faux-bold
//! - paragraphs — word-wrapped to the widget width
//! - ```` ``` ```` fenced code blocks — surface-fill panel, verbatim lines
//! - `>` blockquotes — accent bar + indent
//! - `-`/`*`/`+` bullet lists and `1.` ordered lists — hanging indent
//! - `---`/`***`/`___` thematic breaks — a hairline rule
//!
//! Inline syntax: `**bold**`, `*italic*`, `~~strike~~`, `` `code` ``,
//! and `[label](url)` links. A `\` before punctuation escapes it.
//! `_`/`__` emphasis and images are intentionally not in the subset.
//!
//! ## Rendering limitations (documented, not hidden)
//!
//! The [`crate::text_paint`] `TextShaper` seam carries only
//! `(text, size, color)` — no font-weight, slant, or family axis — so
//! styles degrade honestly:
//!
//! - **bold** (and headings) are *faux-bold*: the run is emitted twice
//!   with a small x offset.
//! - *italic* runs render in the normal face with ink blended toward
//!   the muted text color — there is no true slant.
//! - `code` runs sit on a rounded surface-fill chip; fenced blocks on
//!   a surface-fill panel. The face is the default family, not a real
//!   monospace.
//!
//! Wrapping is planned in `layout` — which has no text painter — with
//! a `size · chars · 0.5` advance heuristic; `paint` replans with real
//! `measure_text` metrics when a painter resolves, so link hit-zones
//! (kept in a `Mutex` plan, like `Breadcrumb`'s) track painted text.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::Markdown;
//!
//! let m = Markdown::new("# Title\n\nSome *emphasis* and a [link](https://a.b).");
//! assert_eq!(m.source(), "# Title\n\nSome *emphasis* and a [link](https://a.b).");
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;
use parking_lot::Mutex;

use crate::text_paint::{paint_label_clipped, resolve_painter, SharedTextPainter};

/// Default body text size, logical points.
const BASE_PT: f32 = 14.0;
/// Line height as a multiple of the run's font size.
const LINE_HEIGHT: f32 = 1.4;
/// Vertical gap between blocks, logical points.
const BLOCK_GAP_PT: f32 = 10.0;
/// Padding inside a fenced-code panel, logical points.
const CODE_PAD_PT: f32 = 8.0;
/// Inline-code size reduction (chip text reads large otherwise).
const CODE_SHRINK: f32 = 0.92;
/// Blockquote bar width, logical points.
const QUOTE_BAR_PT: f32 = 3.0;
/// Blockquote text indent past the bar, logical points.
const QUOTE_PAD_PT: f32 = 10.0;
/// Hanging indent for list item text, logical points.
const LIST_INDENT_PT: f32 = 18.0;
/// Gap between list items, logical points.
const ITEM_GAP_PT: f32 = 3.0;
/// Heading size multipliers for `#`–`######`.
const HEAD_SCALE: [f32; 6] = [1.9, 1.6, 1.35, 1.18, 1.05, 0.95];
/// Heuristic advance width: `size * chars * HEURISTIC_W` when no
/// `TextShaper` is available (layout-time wrap estimates).
const HEURISTIC_W: f32 = 0.5;

/// Style flags carried by one inline run — the parser's output and the
/// plan's per-word styling.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
struct Flags {
    bold: bool,
    italic: bool,
    strike: bool,
    code: bool,
}

/// One inline run — text plus its style flags and optional link URL.
#[derive(Clone, Debug, PartialEq)]
struct Run {
    text: String,
    flags: Flags,
    link: Option<String>,
}

/// A parsed block-level element.
#[derive(Clone, Debug, PartialEq)]
enum Block {
    /// `#`–`######` heading (`level` is 1–6).
    Heading { level: u8, runs: Vec<Run> },
    /// Wrapped paragraph.
    Paragraph(Vec<Run>),
    /// Fenced code block — verbatim lines.
    Code(Vec<String>),
    /// `>` blockquote — its lines joined into one inline run list.
    Quote(Vec<Run>),
    /// `-`/`*`/`+` or `1.` list — one run list per item.
    List { ordered: bool, items: Vec<Vec<Run>> },
    /// `---`/`***`/`___` thematic break.
    Rule,
}

/// Fallback advance estimate — `0.5 · size` per char, matching the
/// convention used by `Cascader`/`Breadcrumb` for painterless passes.
fn heuristic(text: &str, size: f32) -> f32 {
    size * text.chars().count() as f32 * HEURISTIC_W
}

/// Leading-`#` count when `t` is an ATX heading (`# `–`###### `).
fn heading_level(t: &str) -> Option<u8> {
    let n = t.bytes().take_while(|&b| b == b'#').count();
    if (1..=6).contains(&n) && (t.len() == n || t.as_bytes()[n] == b' ') {
        Some(n as u8)
    } else {
        None
    }
}

/// Whether `t` is a thematic break: ≥3 of the same `-`, `*`, or `_`,
/// with optional interleaved whitespace.
fn is_rule(t: &str) -> bool {
    let mut chars = t.chars().filter(|c| !c.is_whitespace());
    let Some(first) = chars.next() else {
        return false;
    };
    if !matches!(first, '-' | '*' | '_') {
        return false;
    }
    let mut n = 1usize;
    for c in chars {
        if c != first {
            return false;
        }
        n += 1;
    }
    n >= 3
}

/// List marker on `t` — returns `(ordered, text_after_marker)`.
fn list_marker(t: &str) -> Option<(bool, &str)> {
    let b = t.as_bytes();
    if b.len() >= 2 && matches!(b[0], b'-' | b'*' | b'+') && b[1] == b' ' {
        return Some((false, &t[2..]));
    }
    let digits = t.bytes().take_while(|b| b.is_ascii_digit()).count();
    if digits > 0
        && t.len() > digits + 1
        && matches!(b[digits], b'.' | b')')
        && b[digits + 1] == b' '
    {
        return Some((true, &t[digits + 2..]));
    }
    None
}

/// Whether `t` opens a block construct — used to end a paragraph
/// without requiring a blank line.
fn starts_block(t: &str) -> bool {
    t.starts_with("```")
        || t.starts_with('>')
        || heading_level(t).is_some()
        || is_rule(t)
        || list_marker(t).is_some()
}

/// Byte offset of the next `delim` at or after `from`.
fn find_close(s: &str, from: usize, delim: &str) -> Option<usize> {
    s[from..].find(delim).map(|e| from + e)
}

/// Byte offset of the next `c` (a `u8` delimiter) not glued to an
/// identical neighbour — so `*italic*` inside `**bold**` regions
/// doesn't close early on a `**` pair.
fn find_close_single(s: &str, from: usize, c: u8) -> Option<usize> {
    let b = s.as_bytes();
    let mut i = from;
    while i < b.len() {
        if b[i] == c {
            let prev_same = i > 0 && b[i - 1] == c;
            let next_same = b.get(i + 1) == Some(&c);
            if !prev_same && !next_same {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

/// Parses the inline layer of `s` under the ambient `flags` —
/// recursion carries bold/italic/strike into nested spans.
fn parse_runs(s: &str, flags: Flags) -> Vec<Run> {
    let mut runs: Vec<Run> = Vec::new();
    let mut buf = String::new();
    let mut i = 0usize;
    while i < s.len() {
        let rest = &s[i..];
        // `\x` escapes punctuation — emit `x` literally.
        if rest.starts_with('\\') && rest.len() > 1 {
            let c = rest[1..].chars().next().unwrap_or('\\');
            buf.push(c);
            i += 1 + c.len_utf8();
            continue;
        }
        // `` `code` `` — verbatim, no nested markup.
        if let Some(after) = rest.strip_prefix('`') {
            if let Some(end) = after.find('`') {
                if !buf.is_empty() {
                    runs.push(Run {
                        text: std::mem::take(&mut buf),
                        flags,
                        link: None,
                    });
                }
                let mut f = flags;
                f.code = true;
                runs.push(Run {
                    text: after[..end].to_string(),
                    flags: f,
                    link: None,
                });
                i += end + 2;
                continue;
            }
        }
        // `**bold**`.
        if rest.starts_with("**") {
            if let Some(end) = find_close(s, i + 2, "**") {
                if !buf.is_empty() {
                    runs.push(Run {
                        text: std::mem::take(&mut buf),
                        flags,
                        link: None,
                    });
                }
                let mut f = flags;
                f.bold = true;
                runs.extend(parse_runs(&s[i + 2..end], f));
                i = end + 2;
                continue;
            }
        }
        // `~~strike~~`.
        if rest.starts_with("~~") {
            if let Some(end) = find_close(s, i + 2, "~~") {
                if !buf.is_empty() {
                    runs.push(Run {
                        text: std::mem::take(&mut buf),
                        flags,
                        link: None,
                    });
                }
                let mut f = flags;
                f.strike = true;
                runs.extend(parse_runs(&s[i + 2..end], f));
                i = end + 2;
                continue;
            }
        }
        // `*italic*` — single `*` only; `**` was handled above.
        if rest.starts_with('*') && !rest.starts_with("**") {
            if let Some(end) = find_close_single(s, i + 1, b'*') {
                if !buf.is_empty() {
                    runs.push(Run {
                        text: std::mem::take(&mut buf),
                        flags,
                        link: None,
                    });
                }
                let mut f = flags;
                f.italic = true;
                runs.extend(parse_runs(&s[i + 1..end], f));
                i = end + 1;
                continue;
            }
        }
        // `[label](url)` — the label is itself parsed (so
        // `[**bold**](u)` keeps its emphasis) and stamped with the URL.
        if rest.starts_with('[') {
            if let Some(rb) = rest.find(']') {
                if rest[rb..].starts_with("](") {
                    if let Some(rp) = rest[rb + 2..].find(')') {
                        if !buf.is_empty() {
                            runs.push(Run {
                                text: std::mem::take(&mut buf),
                                flags,
                                link: None,
                            });
                        }
                        let url = rest[rb + 2..rb + 2 + rp].to_string();
                        for mut r in parse_runs(&rest[1..rb], flags) {
                            r.link = Some(url.clone());
                            runs.push(r);
                        }
                        i += rb + 2 + rp + 1;
                        continue;
                    }
                }
            }
        }
        let c = rest.chars().next().unwrap_or(' ');
        buf.push(c);
        i += c.len_utf8();
    }
    if !buf.is_empty() {
        runs.push(Run {
            text: buf,
            flags,
            link: None,
        });
    }
    runs
}

/// Inline parse entry point — drops empty runs and merges adjacent
/// runs with identical style (so `**a** **b**` paints as one run).
fn parse_inlines(text: &str) -> Vec<Run> {
    let runs = parse_runs(text, Flags::default());
    let mut merged: Vec<Run> = Vec::with_capacity(runs.len());
    for r in runs {
        if r.text.is_empty() {
            continue;
        }
        if let Some(last) = merged.last_mut() {
            if last.flags == r.flags && last.link == r.link {
                last.text.push_str(&r.text);
                continue;
            }
        }
        merged.push(r);
    }
    merged
}

/// Splits `src` into block-level elements.
fn parse_blocks(src: &str) -> Vec<Block> {
    let lines: Vec<&str> = src.lines().collect();
    let mut blocks = Vec::new();
    let mut i = 0usize;
    while i < lines.len() {
        let t = lines[i].trim();
        if t.is_empty() {
            i += 1;
            continue;
        }
        // Fenced code block — verbatim until the closing fence.
        if t.starts_with("```") {
            i += 1;
            let start = i;
            while i < lines.len() && !lines[i].trim_start().starts_with("```") {
                i += 1;
            }
            let code: Vec<String> = lines[start..i].iter().map(|l| (*l).to_string()).collect();
            blocks.push(Block::Code(code));
            i += 1; // skip the closing fence (or walk off the end)
            continue;
        }
        if let Some(level) = heading_level(t) {
            blocks.push(Block::Heading {
                level,
                runs: parse_inlines(t[level as usize..].trim_start()),
            });
            i += 1;
            continue;
        }
        if is_rule(t) {
            blocks.push(Block::Rule);
            i += 1;
            continue;
        }
        if t.starts_with('>') {
            let mut buf = String::new();
            while i < lines.len() && lines[i].trim_start().starts_with('>') {
                let c = lines[i].trim_start().trim_start_matches('>').trim_start();
                if !buf.is_empty() {
                    buf.push(' ');
                }
                buf.push_str(c);
                i += 1;
            }
            blocks.push(Block::Quote(parse_inlines(&buf)));
            continue;
        }
        if let Some((ordered, _)) = list_marker(t) {
            let mut items = Vec::new();
            while i < lines.len() {
                match list_marker(lines[i].trim_start()) {
                    Some((o, rest)) if o == ordered => {
                        items.push(parse_inlines(rest));
                        i += 1;
                    }
                    _ => break,
                }
            }
            blocks.push(Block::List { ordered, items });
            continue;
        }
        // Paragraph — runs until a blank line or the next block opener.
        let mut buf = String::new();
        while i < lines.len() {
            let tt = lines[i].trim();
            if tt.is_empty() || starts_block(tt) {
                break;
            }
            if !buf.is_empty() {
                buf.push(' ');
            }
            buf.push_str(tt);
            i += 1;
        }
        if buf.is_empty() {
            // Defensive: a line that is neither blank nor a block
            // opener always parses — this only guards the impossible
            // path so the loop cannot stall.
            i += 1;
        } else {
            blocks.push(Block::Paragraph(parse_inlines(&buf)));
        }
    }
    blocks
}

/// Non-text paint geometry (panels, chips, bars, rules) — emitted
/// before the text runs so backgrounds sit under their glyphs.
#[derive(Clone, Debug)]
enum Chrome {
    /// Surface-fill panel behind a fenced code block.
    Panel(Rect),
    /// Surface-fill chip behind one inline-code run.
    Chip(Rect),
    /// Accent bar on a blockquote's left edge.
    Bar(Rect),
    /// Thematic-break hairline.
    Rule(Rect),
}

/// One positioned text run in the layout plan.
#[derive(Clone, Debug)]
struct Placed {
    /// Destination rect (bounds-space device px). `min_y` is the line's
    /// top — `paint_label_clipped` origins are top-left.
    rect: Rect,
    text: String,
    /// Font size in device px (scale already applied).
    size: f32,
    flags: Flags,
    /// Index into [`Plan::links`] when this run is a link.
    link: Option<usize>,
}

/// The layout plan — block flow resolved into positioned runs and
/// link hit-zones. Computed in `layout` (heuristic measure) and
/// recomputed in `paint` when a real painter resolves or the width,
/// origin, or scale changed. Interior-mutable because
/// [`Widget::paint`] takes `&self` — the `Breadcrumb` `Mutex<Plan>`
/// pattern.
#[derive(Default)]
struct Plan {
    /// Top-left the plan was computed for (bounds-space).
    origin: Vec2,
    /// Width the plan was wrapped for, device px. `-1` forces a replan.
    width: f32,
    /// Scale the plan was computed at.
    scale: f32,
    /// Whether a real `TextShaper` fed the measurements.
    measured: bool,
    /// Total content height, device px.
    height: f32,
    chrome: Vec<Chrome>,
    runs: Vec<Placed>,
    /// Link hit-zones: `(rect, url)`.
    links: Vec<(Rect, String)>,
}

/// A styled word inside a wrapping line.
struct WItem {
    text: String,
    flags: Flags,
    link: Option<String>,
    /// Measured advance, device px.
    w: f32,
    /// Whether a space precedes this word in the source text.
    space_before: bool,
}

/// Greedy word-wrap of `runs` at `size` (device px) inside `width`,
/// emitting [`Placed`] runs (plus chips and link zones) into `plan`.
/// `x`/`y` are bounds-space device px; returns the y below the block.
/// `bold_all` forces the bold flag — headings are bold by convention.
#[allow(clippy::too_many_arguments)]
fn wrap_runs(
    plan: &mut Plan,
    runs: &[Run],
    size: f32,
    x: f32,
    width: f32,
    y: f32,
    bold_all: bool,
    measure: &dyn Fn(&str, f32) -> f32,
) -> f32 {
    let line_h = size * LINE_HEIGHT;
    let space_w = measure(" ", size).max(size * 0.2);
    // Tokenize into styled words, tracking whether whitespace preceded
    // each one so `**bold**end` reassembles without a gap.
    let mut lines: Vec<Vec<WItem>> = vec![Vec::new()];
    let mut cur_w = 0.0f32;
    let mut pending_space = false;
    for run in runs {
        let mut flags = run.flags;
        if bold_all {
            flags.bold = true;
        }
        let eff = if flags.code { size * CODE_SHRINK } else { size };
        let text = run.text.as_str();
        let mut i = 0usize;
        while i < text.len() {
            let ws_start = i;
            while i < text.len() {
                let c = text[i..].chars().next().unwrap_or(' ');
                if !c.is_whitespace() {
                    break;
                }
                i += c.len_utf8();
            }
            if i > ws_start {
                pending_space = true;
            }
            if i >= text.len() {
                break;
            }
            let wstart = i;
            while i < text.len() {
                let c = text[i..].chars().next().unwrap_or(' ');
                if c.is_whitespace() {
                    break;
                }
                i += c.len_utf8();
            }
            let word = &text[wstart..i];
            let ww = measure(word, eff);
            let cur = lines.last_mut().expect("lines is never empty");
            let need = ww
                + if cur.is_empty() || !pending_space {
                    0.0
                } else {
                    space_w
                };
            if !cur.is_empty() && cur_w + need > width {
                lines.push(Vec::new());
                cur_w = 0.0;
            }
            let cur = lines.last_mut().expect("lines is never empty");
            if !cur.is_empty() && pending_space {
                cur_w += space_w;
            }
            cur_w += ww;
            cur.push(WItem {
                text: word.to_string(),
                flags,
                link: run.link.clone(),
                w: ww,
                space_before: pending_space,
            });
            pending_space = false;
        }
    }
    // Emit lines — adjacent same-style items merge into one Placed.
    let mut y = y;
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let mut x = x;
        let mut first = true;
        for item in line {
            if !first && item.space_before {
                x += space_w;
            }
            first = false;
            // Merge into the previous Placed when it sits on this line,
            // is contiguous (within one space width), and carries the
            // same style and link — the `Breadcrumb`-style hit-zone
            // bookkeeping stays per emitted run.
            if let Some(last) = plan.runs.last_mut() {
                let same_line = (last.rect.min_y() - y).abs() < 0.01;
                let gap = x - last.rect.max_x();
                let contiguous = (-0.01..=space_w + 0.01).contains(&gap);
                let same_link = match (last.link, &item.link) {
                    (None, None) => true,
                    (Some(li), Some(u)) => plan.links[li].1 == *u,
                    _ => false,
                };
                if same_line && contiguous && same_link && last.flags == item.flags {
                    if item.space_before {
                        last.text.push(' ');
                    }
                    last.text.push_str(&item.text);
                    last.rect.size.x = x + item.w - last.rect.min_x();
                    if let Some(li) = last.link {
                        plan.links[li].0 = last.rect;
                    }
                    x += item.w;
                    continue;
                }
            }
            let rect = Rect::new(x, y, item.w, line_h);
            let link = item.link.map(|url| {
                plan.links.push((rect, url));
                plan.links.len() - 1
            });
            if item.flags.code {
                let pad = size * 0.15;
                let chip_h = size * 1.15;
                plan.chrome.push(Chrome::Chip(Rect::new(
                    rect.min_x() - pad,
                    rect.min_y() + (line_h - chip_h) / 2.0,
                    rect.width() + pad * 2.0,
                    chip_h,
                )));
            }
            plan.runs.push(Placed {
                rect,
                text: item.text,
                size: eff_size_of(item.flags, size),
                flags: item.flags,
                link,
            });
            x += item.w;
        }
        y += line_h;
    }
    y
}

/// Effective font size for a run — inline code shrinks slightly.
fn eff_size_of(flags: Flags, size: f32) -> f32 {
    if flags.code {
        size * CODE_SHRINK
    } else {
        size
    }
}

/// Read-only Markdown rich-text renderer.
///
/// Parses a pragmatic Markdown subset once per source change, flows it
/// as a single column of styled runs, and exposes link clicks through
/// [`Markdown::take_link_clicked`]. See the module docs for the subset
/// and the documented style fallbacks.
///
/// # Examples
///
/// ```
/// use martensite::widgets::Markdown;
///
/// let m = Markdown::new("## Hi\n\n- a\n- b");
/// assert!(m.content_height() >= 0.0);
/// ```
pub struct Markdown {
    /// Accessibility label override.
    pub label: Option<String>,
    /// Whether link clicks are delivered.
    pub enabled: bool,
    /// Base font size in logical points.
    base_size: f32,
    /// The raw Markdown source.
    source: String,
    /// Parsed block tree — rebuilt by [`Markdown::set_source`].
    blocks: Vec<Block>,
    bounds: Rect,
    /// Last computed layout plan — see [`Plan`].
    plan: Mutex<Plan>,
    /// Parked link activation — drained by `take_link_clicked`.
    link_clicked: Option<String>,
    /// Shared shaped-text painter. See [`crate::text_paint`].
    text_painter: Option<SharedTextPainter>,
}

impl Markdown {
    /// Creates a renderer for `source` — parses immediately.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Markdown;
    ///
    /// let m = Markdown::new("# Hello");
    /// assert_eq!(m.source(), "# Hello");
    /// ```
    pub fn new(source: impl Into<String>) -> Self {
        let source = source.into();
        Self {
            label: None,
            enabled: true,
            base_size: BASE_PT,
            blocks: parse_blocks(&source),
            source,
            bounds: Rect::default(),
            plan: Mutex::new(Plan {
                width: -1.0,
                ..Plan::default()
            }),
            link_clicked: None,
            text_painter: None,
        }
    }

    /// Sets the accessibility label.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Markdown;
    ///
    /// let m = Markdown::new("x").label("Release notes");
    /// assert_eq!(m.label.as_deref(), Some("Release notes"));
    /// ```
    #[must_use]
    pub fn label(mut self, text: impl Into<String>) -> Self {
        self.label = Some(text.into());
        self
    }

    /// Sets whether link clicks are delivered.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Markdown;
    ///
    /// assert!(!Markdown::new("x").enabled(false).enabled);
    /// ```
    #[must_use]
    pub fn enabled(mut self, flag: bool) -> Self {
        self.enabled = flag;
        self
    }

    /// Shares a [`crate::text_paint::TextPainter`] for real glyph runs
    /// and accurate wrap metrics.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::text_paint::shared_painter;
    /// use martensite::widgets::Markdown;
    ///
    /// let m = Markdown::new("x").with_text_painter(shared_painter());
    /// ```
    #[must_use]
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Sets the body text size in logical points — headings scale off
    /// it.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Markdown;
    ///
    /// let m = Markdown::new("x").base_size(16.0);
    /// ```
    #[must_use]
    pub fn base_size(mut self, pt: f32) -> Self {
        self.base_size = pt.max(1.0);
        // Sizes live in the plan — force a re-wrap.
        self.plan.lock().width = -1.0;
        self
    }

    /// The raw Markdown source.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Markdown;
    ///
    /// assert_eq!(Markdown::new("**b**").source(), "**b**");
    /// ```
    #[inline]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Replaces the source and reparses; the plan re-wraps on the next
    /// layout or paint pass.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Markdown;
    ///
    /// let mut m = Markdown::new("a");
    /// m.set_source("# Bigger");
    /// assert_eq!(m.source(), "# Bigger");
    /// ```
    pub fn set_source(&mut self, source: impl Into<String>) {
        self.source = source.into();
        self.blocks = parse_blocks(&self.source);
        self.plan.lock().width = -1.0;
    }

    /// Total flowed content height in device px — valid after the last
    /// `layout` or `paint` pass; `0.0` before either runs.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::core::{HotNode, LayoutContext, Rect, Widget};
    /// use martensite::widgets::Markdown;
    ///
    /// let mut m = Markdown::new("Hello");
    /// let mut hot = HotNode::default();
    /// let mut cx = LayoutContext { hot: &mut hot, scale: 1.0 };
    /// m.layout(&mut cx, Rect::new(0.0, 0.0, 200.0, 400.0));
    /// assert!(m.content_height() > 0.0);
    /// ```
    #[inline]
    pub fn content_height(&self) -> f32 {
        self.plan.lock().height
    }

    /// Drains a link activation — the URL of the clicked `[label](url)`
    /// run, `Some` once per click.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Markdown;
    ///
    /// let mut m = Markdown::new("[a](https://b.c)");
    /// assert_eq!(m.take_link_clicked(), None);
    /// ```
    #[inline]
    pub fn take_link_clicked(&mut self) -> Option<String> {
        self.link_clicked.take()
    }

    /// Plain-text extraction — markup stripped, block structure kept
    /// as newlines. Backs the accessibility `value`.
    fn plain_text(&self) -> String {
        let mut out = String::new();
        for block in &self.blocks {
            if !out.is_empty() {
                out.push('\n');
            }
            match block {
                Block::Heading { runs, .. } | Block::Paragraph(runs) | Block::Quote(runs) => {
                    for r in runs {
                        out.push_str(&r.text);
                    }
                }
                Block::Code(lines) => {
                    out.push_str(&lines.join("\n"));
                }
                Block::List { ordered, items } => {
                    for (i, item) in items.iter().enumerate() {
                        if i > 0 {
                            out.push('\n');
                        }
                        if *ordered {
                            out.push_str(&format!("{}. ", i + 1));
                        } else {
                            out.push_str("• ");
                        }
                        for r in item {
                            out.push_str(&r.text);
                        }
                    }
                }
                Block::Rule => {
                    out.pop(); // a rule contributes no text
                }
            }
        }
        out
    }

    /// Flows `self.blocks` into a positioned [`Plan`] for `bounds`.
    /// `measure` is the advance estimator — the real painter at paint
    /// time, the `0.5·size·chars` heuristic at layout time.
    fn build_plan(
        &self,
        bounds: Rect,
        scale: f32,
        measured: bool,
        measure: &dyn Fn(&str, f32) -> f32,
    ) -> Plan {
        let mut plan = Plan {
            origin: bounds.origin,
            width: bounds.width(),
            scale,
            measured,
            ..Plan::default()
        };
        let base = self.base_size * scale;
        let x0 = bounds.min_x();
        let w = bounds.width().max(0.0);
        let gap = BLOCK_GAP_PT * scale;
        let mut y = bounds.min_y();
        for (i, block) in self.blocks.iter().enumerate() {
            if i > 0 {
                y += gap;
            }
            match block {
                Block::Heading { level, runs } => {
                    let idx = usize::from(level.saturating_sub(1)).min(5);
                    let size = base * HEAD_SCALE[idx];
                    y = wrap_runs(&mut plan, runs, size, x0, w, y, true, measure);
                }
                Block::Paragraph(runs) => {
                    y = wrap_runs(&mut plan, runs, base, x0, w, y, false, measure);
                }
                Block::Code(lines) => {
                    let pad = CODE_PAD_PT * scale;
                    let size = base * CODE_SHRINK;
                    let line_h = size * LINE_HEIGHT;
                    let h = lines.len().max(1) as f32 * line_h + pad * 2.0;
                    plan.chrome.push(Chrome::Panel(Rect::new(x0, y, w, h)));
                    let mut ly = y + pad;
                    for l in lines {
                        plan.runs.push(Placed {
                            rect: Rect::new(x0 + pad, ly, (w - pad * 2.0).max(0.0), line_h),
                            text: l.clone(),
                            size,
                            flags: Flags {
                                code: true,
                                ..Flags::default()
                            },
                            link: None,
                        });
                        ly += line_h;
                    }
                    y += h;
                }
                Block::Quote(runs) => {
                    let bar = QUOTE_BAR_PT * scale;
                    let pad = QUOTE_PAD_PT * scale;
                    let vpad = pad * 0.5;
                    let start = y;
                    y += vpad;
                    y = wrap_runs(
                        &mut plan,
                        runs,
                        base,
                        x0 + bar + pad,
                        (w - bar - pad).max(0.0),
                        y,
                        false,
                        measure,
                    );
                    y += vpad;
                    plan.chrome.push(Chrome::Bar(Rect::new(
                        x0,
                        start,
                        bar,
                        (y - start).max(base * LINE_HEIGHT),
                    )));
                }
                Block::List { ordered, items } => {
                    let indent = LIST_INDENT_PT * scale;
                    for (n, item) in items.iter().enumerate() {
                        if n > 0 {
                            y += ITEM_GAP_PT * scale;
                        }
                        let marker = if *ordered {
                            format!("{}.", n + 1)
                        } else {
                            "•".to_string()
                        };
                        let line_start = y;
                        let mw = measure(&marker, base).max(indent * 0.5);
                        plan.runs.push(Placed {
                            rect: Rect::new(x0, y, mw, base * LINE_HEIGHT),
                            text: marker,
                            size: base,
                            flags: Flags::default(),
                            link: None,
                        });
                        y = wrap_runs(
                            &mut plan,
                            item,
                            base,
                            x0 + indent,
                            (w - indent).max(0.0),
                            y,
                            false,
                            measure,
                        );
                        // An empty item still occupies its marker line.
                        y = y.max(line_start + base * LINE_HEIGHT);
                    }
                }
                Block::Rule => {
                    let slot = base * LINE_HEIGHT * 0.6;
                    let hair = HAIRLINE * scale;
                    plan.chrome.push(Chrome::Rule(Rect::new(
                        x0,
                        y + (slot - hair) / 2.0,
                        w,
                        hair,
                    )));
                    y += slot;
                }
            }
        }
        plan.height = y - bounds.min_y();
        plan
    }
}

/// Hairline thickness, logical points.
const HAIRLINE: f32 = 1.0;

impl Default for Markdown {
    fn default() -> Self {
        Self::new("")
    }
}

impl std::fmt::Debug for Markdown {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Markdown")
            .field("blocks", &self.blocks.len())
            .field("enabled", &self.enabled)
            .finish()
    }
}

/// Alpha-blend `a` toward `b` by `t` (0 = a, 1 = b).
fn mix(a: [u8; 4], b: [u8; 4], t: f32) -> [u8; 4] {
    let lerp = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t) as u8;
    [lerp(a[0], b[0]), lerp(a[1], b[1]), lerp(a[2], b[2]), a[3]]
}

impl Widget for Markdown {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let w = if constraints.max_size.x.is_finite() {
            constraints.max_size.x
        } else {
            cx.pt(320.0)
        };
        let plan = self.build_plan(
            Rect::new(0.0, 0.0, w.max(0.0), 0.0),
            cx.scale,
            false,
            &heuristic,
        );
        Vec2::new(w, plan.height.min(constraints.max_size.y.max(0.0)))
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(160.0, 80.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        // LayoutContext carries no text painter — wrap with the
        // documented heuristic; paint replans with real metrics.
        let plan = self.build_plan(bounds, cx.scale, false, &heuristic);
        *self.plan.lock() = plan;
    }

    fn paint(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        if b.width() <= 0.0 {
            return;
        }
        let painter = resolve_painter(&self.text_painter, cx.text_painter);
        let stale = {
            let p = self.plan.lock();
            p.width != b.width()
                || p.origin != b.origin
                || (p.scale - cx.scale).abs() > f32::EPSILON
                || (painter.is_some() && !p.measured)
        };
        if stale {
            let plan = match painter {
                Some(p) => self.build_plan(b, cx.scale, true, &|t, s| {
                    p.measure_text(t, s).unwrap_or_else(|| heuristic(t, s))
                }),
                None => self.build_plan(b, cx.scale, false, &heuristic),
            };
            *self.plan.lock() = plan;
        }
        let plan = self.plan.lock();

        let fg = cx.color(TokenKey::TextColor, [220, 220, 220, 255]);
        let muted = cx.color(TokenKey::TextMutedColor, [150, 150, 150, 255]);
        let accent = cx.color(TokenKey::AccentColor, [80, 140, 240, 255]);
        let surface = cx.color(TokenKey::SurfaceColor, [45, 45, 48, 255]);
        let border = cx.color(TokenKey::BorderColor, [90, 90, 90, 255]);
        let fg = if self.enabled { fg } else { muted };

        let k = |r: Rect| {
            kurbo::Rect::new(
                f64::from(r.min_x()),
                f64::from(r.min_y()),
                f64::from(r.max_x()),
                f64::from(r.max_y()),
            )
        };
        let clip = k(b);
        let radius = cx.dim(TokenKey::BorderRadius, 4.0);
        for chrome in &plan.chrome {
            match chrome {
                Chrome::Panel(r) => {
                    cx.list.push_fill_shape(
                        k(*r),
                        &martensite_core::shape::Shape::rounded(radius),
                        surface,
                    );
                }
                Chrome::Chip(r) => {
                    cx.list.push_fill_shape(
                        k(*r),
                        &martensite_core::shape::Shape::rounded(cx.pt(2.0)),
                        surface,
                    );
                }
                Chrome::Bar(r) => {
                    cx.list.push_fill_rect(k(*r), accent);
                }
                Chrome::Rule(r) => {
                    cx.list.push_fill_rect(k(*r), border);
                }
            }
        }
        for run in &plan.runs {
            let ink = if run.link.is_some() {
                accent
            } else if run.flags.italic {
                // Documented fallback — the TextShaper seam has no slant
                // axis, so italics render as a muted-leaning ink.
                mix(fg, muted, 0.45)
            } else {
                fg
            };
            let origin =
                kurbo::Point::new(f64::from(run.rect.min_x()), f64::from(run.rect.min_y()));
            paint_label_clipped(painter, cx.list, clip, origin, &run.text, run.size, ink);
            if run.flags.bold {
                // Faux bold — no weight axis in the seam, so the run is
                // double-struck with a small offset.
                let dx = (run.size * 0.035).max(0.4);
                paint_label_clipped(
                    painter,
                    cx.list,
                    clip,
                    kurbo::Point::new(origin.x + f64::from(dx), origin.y),
                    &run.text,
                    run.size,
                    ink,
                );
            }
            if run.flags.strike {
                let hair = cx.pt(0.8).max(1.0);
                cx.list.push_fill_rect(
                    kurbo::Rect::new(
                        f64::from(run.rect.min_x()),
                        f64::from(run.rect.min_y() + run.size * 0.55),
                        f64::from(run.rect.max_x()),
                        f64::from(run.rect.min_y() + run.size * 0.55 + hair),
                    ),
                    ink,
                );
            }
            if run.link.is_some() {
                let hair = cx.pt(0.8).max(1.0);
                cx.list.push_fill_rect(
                    kurbo::Rect::new(
                        f64::from(run.rect.min_x()),
                        f64::from(run.rect.min_y() + run.size * 1.15),
                        f64::from(run.rect.max_x()),
                        f64::from(run.rect.min_y() + run.size * 1.15 + hair),
                    ),
                    accent,
                );
            }
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.enabled {
            return EventResponse::Ignored;
        }
        if let WidgetEvent::PointerReleased { position, button } = cx.event {
            if *button == martensite_core::PointerButton::Primary {
                let hit = {
                    let plan = self.plan.lock();
                    plan.links
                        .iter()
                        .find(|(r, _)| r.contains(*position))
                        .map(|(_, url)| url.clone())
                };
                if let Some(url) = hit {
                    self.link_clicked = Some(url);
                    return EventResponse::Handled;
                }
            }
        }
        EventResponse::Ignored
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Document);
        node.set_label(self.label.as_deref().unwrap_or("Markdown document"));
        let text = self.plain_text();
        if !text.is_empty() {
            node.set_value(text);
        }
        if !self.enabled {
            node.set_disabled();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::{HotNode, PointerButton};

    fn laid_out(w: &mut Markdown, width: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        w.layout(&mut cx, Rect::new(0.0, 0.0, width, h));
    }

    fn ev(w: &mut Markdown, event: &WidgetEvent) -> EventResponse {
        let mut cx = EventContext {
            event,
            bounds: w.bounds,
            scale: 1.0,
        };
        w.event(&mut cx)
    }

    #[test]
    fn headings_and_paragraph_parse() {
        let m = Markdown::new("# Title\n\n## Sub\n\nbody text");
        assert_eq!(m.blocks.len(), 3);
        assert!(matches!(&m.blocks[0], Block::Heading { level: 1, .. }));
        assert!(matches!(&m.blocks[1], Block::Heading { level: 2, .. }));
        assert!(matches!(&m.blocks[2], Block::Paragraph(_)));
    }

    #[test]
    fn bullet_and_ordered_lists_parse() {
        let m = Markdown::new("- a\n- b\n\n1. x\n2. y\n3. z");
        assert_eq!(m.blocks.len(), 2);
        match &m.blocks[0] {
            Block::List { ordered, items } => {
                assert!(!ordered);
                assert_eq!(items.len(), 2);
            }
            other => panic!("expected bullet list, got {other:?}"),
        }
        match &m.blocks[1] {
            Block::List { ordered, items } => {
                assert!(ordered);
                assert_eq!(items.len(), 3);
            }
            other => panic!("expected ordered list, got {other:?}"),
        }
    }

    #[test]
    fn code_quote_and_rule_parse() {
        let m = Markdown::new("```rust\nlet x = 1;\nlet y = 2;\n```\n\n> quoted\n\n---");
        assert_eq!(m.blocks.len(), 3);
        match &m.blocks[0] {
            Block::Code(lines) => assert_eq!(lines.len(), 2),
            other => panic!("expected code, got {other:?}"),
        }
        assert!(matches!(&m.blocks[1], Block::Quote(_)));
        assert!(matches!(&m.blocks[2], Block::Rule));
    }

    #[test]
    fn inline_styles_parse() {
        let runs = parse_inlines("a **b** *i* `c` ~~s~~ [l](u)");
        let flags_of = |needle: &str| {
            runs.iter()
                .find(|r| r.text == needle)
                .unwrap_or_else(|| panic!("missing run {needle}"))
                .flags
        };
        assert!(!flags_of("a ").bold);
        assert!(flags_of("b").bold);
        assert!(flags_of("i").italic);
        assert!(flags_of("c").code);
        assert!(flags_of("s").strike);
        let link = runs.iter().find(|r| r.link.is_some()).expect("link run");
        assert_eq!(link.text, "l");
        assert_eq!(link.link.as_deref(), Some("u"));
    }

    #[test]
    fn plain_text_strips_markup() {
        let m = Markdown::new("# T **x**\n\n- a\n- b\n\n`c` and [l](u)");
        let text = m.plain_text();
        assert!(!text.contains('#'));
        assert!(!text.contains("**"));
        assert!(text.contains("T x"));
        assert!(text.contains("• a"));
        assert!(text.contains("c and l"));
    }

    #[test]
    fn link_click_parks_url() {
        let mut m = Markdown::new("See [docs](https://example.com) now.");
        laid_out(&mut m, 400.0, 200.0);
        let rect = {
            let plan = m.plan.lock();
            assert_eq!(plan.links.len(), 1);
            plan.links[0].0
        };
        let click = WidgetEvent::PointerReleased {
            position: Vec2::new(
                (rect.min_x() + rect.max_x()) / 2.0,
                (rect.min_y() + rect.max_y()) / 2.0,
            ),
            button: PointerButton::Primary,
        };
        assert_eq!(ev(&mut m, &click), EventResponse::Handled);
        assert_eq!(
            m.take_link_clicked().as_deref(),
            Some("https://example.com")
        );
        assert!(m.take_link_clicked().is_none());
    }

    #[test]
    fn set_source_reparses() {
        let mut m = Markdown::new("one");
        assert_eq!(m.blocks.len(), 1);
        m.set_source("# a\n\n# b\n\n# c");
        assert_eq!(m.blocks.len(), 3);
        laid_out(&mut m, 200.0, 400.0);
        let h1 = m.content_height();
        m.set_source("tiny");
        laid_out(&mut m, 200.0, 400.0);
        assert!(m.content_height() < h1);
    }

    #[test]
    fn disabled_inert() {
        let mut m = Markdown::new("[a](https://b.c)").enabled(false);
        laid_out(&mut m, 400.0, 200.0);
        let rect = m.plan.lock().links[0].0;
        let click = WidgetEvent::PointerReleased {
            position: Vec2::new(
                (rect.min_x() + rect.max_x()) / 2.0,
                (rect.min_y() + rect.max_y()) / 2.0,
            ),
            button: PointerButton::Primary,
        };
        assert_eq!(ev(&mut m, &click), EventResponse::Ignored);
        assert!(m.take_link_clicked().is_none());
    }

    #[test]
    fn content_height_grows_with_content() {
        let mut small = Markdown::new("one line");
        laid_out(&mut small, 300.0, 500.0);
        let mut big = Markdown::new(
            "# Head\n\nparagraph one\n\nparagraph two\n\n- a\n- b\n- c\n\n```\ncode\n```",
        );
        laid_out(&mut big, 300.0, 500.0);
        assert!(big.content_height() > small.content_height() * 2.0);
    }

    #[test]
    fn empty_source_is_zero_height() {
        let mut m = Markdown::new("");
        laid_out(&mut m, 300.0, 500.0);
        assert_eq!(m.content_height(), 0.0);
        assert!(m.blocks.is_empty());
        assert_eq!(m.plain_text(), "");
    }

    #[test]
    fn narrow_width_wraps_taller() {
        let src = "a fairly long paragraph of text that must wrap across several lines when narrow";
        let mut wide = Markdown::new(src);
        laid_out(&mut wide, 800.0, 500.0);
        let mut narrow = Markdown::new(src);
        laid_out(&mut narrow, 120.0, 500.0);
        assert!(narrow.content_height() > wide.content_height());
    }
}
