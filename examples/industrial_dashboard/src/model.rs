//! Shared demo model: the process-metrics row type, deterministic row
//! generation, the dock-tree topology, and the theme-token palette every
//! painted surface resolves through.
//!
//! Both the headless composition (`headless.rs`) and the windowed app
//! (`app.rs` + `panels.rs`) build on these — the windowed path is a
//! presentation layer over the same model the CI dogfood exercises.

use martensite::blessed::{DockPanel, DockTree, SplitDirection};
use martensite::prelude::*;
use martensite::theme::tokens::{default_dark, default_light};

/// One row of the process-metrics table. Kept POD so 1M rows stay cheap.
#[derive(Clone, Debug)]
pub struct MetricRow {
    pub pid: u32,
    pub cpu_milli: u32,
    pub mem_kib: u32,
    pub alert: bool,
}

/// Deterministic 1M-row dataset — the same generator the headless
/// composition and its tests use.
pub fn gen_rows(n: usize) -> Vec<MetricRow> {
    (0..n)
        .map(|i| MetricRow {
            pid: 1000 + (i as u32) * 7 % 60000,
            cpu_milli: ((i * 37) % 100_000) as u32,
            mem_kib: 4096 + ((i * 53) % 2_000_000) as u32,
            alert: i % 977 == 0,
        })
        .collect()
}

/// Number of alerting rows in the generated dataset — precomputed once so
/// the header KPI doesn't scan a million rows per frame.
pub fn alert_count(rows: &[MetricRow]) -> usize {
    rows.iter().filter(|r| r.alert).count()
}

/// Builds the BSP docking tree for the four workstation panels:
///
/// ```text
/// ┌──────────────────┬───────────────┐
/// │  PROCESS GRID    │  TELEMETRY    │
/// ├──────────────────┼───────────────┤
/// │  EDITOR          │  MEDIA        │
/// └──────────────────┴───────────────┘
/// ```
///
/// `Vertical` split → left/right children; `Horizontal` → top/bottom.
/// The tree is the panel-geometry authority: `DockTree::panel_rects`
/// maps each leaf to a physical rect the arena applies verbatim.
///
/// The bottom row spans the full window width — nesting Media as a
/// corner of the right column (the old topology) left it ~16% of the
/// window, under the 320pt zone minimum, so it fell back to the
/// "enlarge to restore" placeholder at every reasonable size.
pub fn build_dock_tree(widget_ids: &[u64; 4]) -> DockTree {
    let mut tree = DockTree::with_capacity(8);
    let root = tree.insert_root(DockPanel::new(widget_ids[0], "Process Grid"));
    let (_top, bottom) = tree
        .split_leaf(
            root,
            SplitDirection::Horizontal,
            0.55,
            DockPanel::new(widget_ids[2], "Editor"),
        )
        .expect("split horizontal");
    let (_grid, _telemetry) = tree
        .split_leaf(
            _top,
            SplitDirection::Vertical,
            0.58,
            DockPanel::new(widget_ids[1], "Telemetry"),
        )
        .expect("split grid");
    // Editor gets the wider share — code needs horizontal room; Media
    // still lands ~38% of the full window width (≈575pt at 1512),
    // comfortably above the zone minimum.
    tree.split_leaf(
        bottom,
        SplitDirection::Vertical,
        0.62,
        DockPanel::new(widget_ids[3], "Media"),
    )
    .expect("split media");
    tree
}

/// The workstation palette — every painted surface resolves through
/// `default_dark()` theme tokens (Oklab → sRGB8) so the demo is a live
/// consumer of the theming pipeline rather than a bag of literals.
/// Semantic aliases keep call-sites readable.
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    /// Window background.
    pub bg: [u8; 4],
    /// Panel surface.
    pub surface: [u8; 4],
    /// Raised chrome (title bars, header strip).
    pub raised: [u8; 4],
    /// Primary accent (focus rings, sort indicators, primary data).
    pub accent: [u8; 4],
    /// Secondary data series.
    pub accent2: [u8; 4],
    /// Body text.
    pub text: [u8; 4],
    /// Secondary/muted text.
    pub text_muted: [u8; 4],
    /// Panel borders and hairlines (must hold 3:1 vs backdrop — the
    /// paint audit checks stroked borders under WCAG 1.4.11).
    pub border: [u8; 4],
    /// Selection fill.
    pub selected: [u8; 4],
    /// Error/alert.
    pub error: [u8; 4],
    /// Warning.
    pub warn: [u8; 4],
    /// Success/healthy.
    pub ok: [u8; 4],
}

impl Palette {
    /// Resolves an arbitrary theme into paint colors. Missing tokens
    /// fall back to the dark defaults so the palette is stable even if
    /// a token is renamed pre-freeze.
    pub fn from_theme(theme: &martensite::theme::Theme) -> Self {
        let c =
            |key, fallback: [u8; 4]| theme.color(key).map(|o| o.to_srgba8()).unwrap_or(fallback);
        Self {
            bg: c(TokenKey::BackgroundColor, [15, 17, 23, 255]),
            surface: c(TokenKey::SurfaceColor, [24, 27, 36, 255]),
            // `SecondaryColor` is a *foreground* token (l 0.60) — chrome
            // surfaces ride the tonal ladder: bg 0.20 → surface 0.25 →
            // raised 0.30 (DividerColor) → border 0.35.
            raised: c(TokenKey::DividerColor, [46, 48, 53, 255]),
            accent: c(TokenKey::AccentColor, [96, 165, 250, 255]),
            accent2: c(TokenKey::InfoColor, [56, 189, 248, 255]),
            text: c(TokenKey::TextColor, [226, 232, 240, 255]),
            text_muted: c(TokenKey::TextMutedColor, [148, 163, 184, 255]),
            border: c(TokenKey::BorderColor, [71, 85, 105, 255]),
            selected: c(TokenKey::PrimaryColor, [37, 99, 235, 255]),
            error: c(TokenKey::ErrorColor, [248, 113, 113, 255]),
            warn: c(TokenKey::WarningColor, [251, 191, 36, 255]),
            ok: c(TokenKey::SuccessColor, [74, 222, 128, 255]),
        }
    }

    /// The shipped dark theme resolved to a palette.
    pub fn dark() -> Self {
        Self::from_theme(&default_dark())
    }

    /// The shipped light theme resolved to a palette.
    #[allow(dead_code)] // Convenience parity with `dark()` — callers go through `from_theme`.
    pub fn light() -> Self {
        Self::from_theme(&default_light())
    }

    /// `color` at `alpha` — chart area fills and focus-ring glows derive
    /// from the same tokens instead of drifting to ad-hoc tints.
    pub fn alpha(color: [u8; 4], a: u8) -> [u8; 4] {
        [color[0], color[1], color[2], a]
    }

    /// Hairline stroke for panel frames and separators. `BorderColor`
    /// is tuned for *control outlines* (≈3.4:1 on `raised`) — for the
    /// finer 1px panel hairlines the muted-text token reads better and
    /// composites to ≈3.5:1 on `raised` under the audit (translucent
    /// variants measured only 1.9:1).
    pub fn hairline(&self) -> [u8; 4] {
        self.text_muted
    }

    /// Selected-row tint: `PrimaryColor` (l 0.65) is too bright to sit
    /// under body text (≈2:1), so selection is a token-derived wash over
    /// `surface` — the hue still reads as the primary selection color.
    pub fn selection_tint(&self) -> [u8; 4] {
        Self::alpha(self.selected, 60)
    }
}

/// The editor panel's `workstation.toml` buffer — reads like a real
/// Martensite consumer so the syntax-highlight spans exercise every
/// `TokenKind`.
const EDITOR_SOURCE: &str = "\
# workstation.toml — dogfooding consumer config
[window]
title = \"Industrial Workstation\"
scale = 2.0            # physical px per logical pt

[dock]
grid = 0.58            # BSP ratio, left leaf
telemetry = 0.55       # top of right column

[telemetry]
source = \"cpu_load\"    # Signal<f64> → LineSeries
history = 240          # samples retained
paused = false

[access]
audit = \"advisory\"     # WCAG paint lints on in debug
";

/// `pipeline.toml` — a sibling config so the editor's tab strip has a
/// second document; numbers, strings, and `#` comments still land on
/// distinct `TokenKind`s.
const PIPELINE_SOURCE: &str = "\
# pipeline.toml — telemetry ingest pipeline
[pipeline]
name = \"cpu_feed\"
rate_hz = 10           # samples per second
batch = 64

[stages]
ingest = \"ring_buffer\" # Signal<f64> source
fold = \"mean_2sigma\"   # outlier gate
sink = \"chart\"         # LineSeries consumer

[alerts]
high_watermark = 90.0  # percent of capacity
cooldown_ms = 250      # debounce window
enabled = true
";

/// `hot_path.rs` — a Rust fragment so `fn`/`let`/`pub`/`impl`/`use`
/// reach `TokenKind::Keyword` (the toml tabs can't) and `//` comments
/// stay muted.
const HOT_PATH_SOURCE: &str = "\
// hot_path.rs — the per-frame inner loop the demo profiles
use std::time::Instant;

pub struct FrameBudget {
    pub limit_ms: f64,
    pub last_ms: f64,
}

impl FrameBudget {
    pub fn new(limit_ms: f64) -> Self {
        Self { limit_ms, last_ms: 0.0 }
    }

    fn record(&mut self, start: Instant) -> bool {
        let ms = start.elapsed().as_secs_f64() * 1000.0;
        self.last_ms = ms;
        self.last_ms <= self.limit_ms
    }
}
";

/// The editor panel's document set — baked into the binary and served
/// through `martensite_assets::vfs::EmbeddedVfs`, one tab per entry.
/// Table order is tab order.
pub static SOURCES: &[(&str, &[u8])] = &[
    ("workstation.toml", EDITOR_SOURCE.as_bytes()),
    ("pipeline.toml", PIPELINE_SOURCE.as_bytes()),
    ("hot_path.rs", HOT_PATH_SOURCE.as_bytes()),
];
