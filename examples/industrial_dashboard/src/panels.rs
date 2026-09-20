//! Workstation panels — real `Widget` implementations over the blessed
//! headless models (`DataTable`, `Chart`, `CodeEditor`, `MediaView`).
//!
//! This is the file that makes the windowed demo an honest consumer
//! rather than a paint mock-up: every panel exercises the actual
//! `measure`/`layout`/`paint`/`event`/`accessibility`/`tick` contract a
//! third-party widget author would implement. F-log notes extend the
//! headless friction log with findings unique to the windowed path.
//!
//! F17. `WidgetEvent::KeyPressed` carries no modifier state — a widget
//!      that wants Shift+Down must track `Shift` press/release through
//!      the ordinary key stream (`GridPanel::shift_held`). A `modifiers`
//!      field is a pre-freeze fix candidate.
//! F18. There is no DPI channel into widgets: `HotNode` is a fixed
//!      64-byte struct and `LayoutContext` only carries `&mut HotNode`.
//!      The demo passes scale as a `Signal<f32>` — the app writes it on
//!      `ScaleFactorChanged` and panels read it in `layout`/`paint`,
//!      which also dogfoods signal delivery inside the widget lifecycle.
//! F19. `Widget::paint` takes `&self` but shaping needs `&mut
//!      FontManager` — `Text` only copes because its content is static
//!      between `measure` calls. Panels whose content changes per frame
//!      (scroll, telemetry) must shape inside `paint`, so each keeps its
//!      `TextPainter` behind a `parking_lot::Mutex` (poison-free, same
//!      as `MartensiteAccessBridge`). Interior mutability for paint-time
//!      shaping is a real consumer need — a shared painter in
//!      `PaintContext` is a pre-freeze API candidate.
//! F20. `blessed::Rect` is (x, y, width, height) while `core::Rect`/
//!      `kurbo::Rect` are (x0, y0, x1, y1) — passing a dock rect's
//!      `bottom` as `height` silently doubled every panel's extent
//!      under the status bar. The two conventions existing side by side
//!      is a real trap for consumers mixing the layers.
//! F21. `HotNode::default()` has empty flags — no `VISIBLE`, no
//!      `HIT_TEST_ENABLED`. Without VISIBLE the paint walker skips the
//!      whole subtree (empty window, no error); without HIT_TEST the
//!      router never delivers pointer or scroll events (dead UI, no
//!      error). Both defaults fail silently — flag-inclusion would be
//!      a kinder default for interactive nodes.
//! F22. `FocusManager::try_set_focus` and `tab` update focus state but
//!      do not dispatch `FocusLost`/`FocusGained` — only
//!      `apply_focus_request` does. Focus rings and editor carets stay
//!      dark when the app uses the cheaper calls; the dispatch-vs-set
//!      split is easy to miss.
//! F23. Paint findings used to name a coordinate, not a component —
//!      `@ (738, 302)` meant hunting panels by geometry. The fix is
//!      provenance in the stream: `PushScope { id, name, bounds }` /
//!      `PopScope` around every widget (a deliberate breaking change to
//!      the public `PaintCommand` enum, taken now because this is
//!      exactly what the pre-1.0 freeze window exists for), plus
//!      `Widget::debug_name()` — panels override it with friendly
//!      labels so lints read `in Process Grid`, not
//!      `…::panels::GridPanel`.
//! F24. `CodeEditor` exposes no `set_text`/`undo` — restoring a
//!      snapshot means rebuilding the whole editor (`CodeEditor::new`
//!      + `set_cursors`), so undo/redo history lives consumer-side in
//!      `EditorTab`. Cheap at demo sizes; a real editor wants a
//!      piece-table model with `replace_text`. Pre-freeze candidate.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite::blessed::data_table::{ColumnConfig, ColumnSort, KeyAction, RowFilter};
use martensite::blessed::{
    AreaSeries, Chart, CodeEditor, Cursor, DataTable, LineSeries, Point as CPoint, ScatterSeries,
    TokenKind,
};
use martensite::core::overlay::{OverlayAnchor, OverlayLayer};
use martensite::core::shape::{CornerRadii, CornerStyle, Shape};
use martensite::core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, SemanticAction, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite::media::surface::{VideoPixelFormat, VideoSurface};
use martensite::prelude::*;
use martensite::render::{BezPath, PaintList, Point};
use martensite::widgets::media::{MediaView, VideoFit};
use martensite::widgets::{Banner, Severity};
use martensite_assets::vfs::{EmbeddedVfs, Vfs};
use martensite_motion::RubberBandScroller;

use crate::menu::{ContextMenu, MenuState};
use crate::model::{alert_count, gen_rows, MetricRow, Palette, SOURCES};
use crate::text::{SpanColor, TextPainter};

/// Title-bar height in logical pt — one constant for the chrome paint
/// and every hit-zone/layout computation that subtracts it (including
/// the app's title-bar drag-to-dock hit test).
pub(crate) const TITLE_H: f32 = 28.0;
/// Grid column-header band height in logical pt.
const GRID_HEADER_H: f32 = 26.0;
/// Editor tab-strip row height in logical pt — a second header row
/// under the title bar.
const TAB_H: f32 = 24.0;

fn to_paint(rect: Rect) -> martensite::render::Rect {
    martensite::render::Rect::new(
        f64::from(rect.min_x()),
        f64::from(rect.min_y()),
        f64::from(rect.max_x()),
        f64::from(rect.max_y()),
    )
}

fn krect(x: f64, y: f64, w: f64, h: f64) -> martensite::render::Rect {
    martensite::render::Rect::new(x, y, x + w, y + h)
}

/// Paints the shared panel chrome — surface and title bar — and
/// returns the inner content rect (panel-local, still in window
/// coordinates). `right` is drawn muted at the title bar's trailing
/// edge (row counts, state badges). The hairline/focus ring is NOT
/// emitted here: it must come last via [`panel_border`], after all
/// panel content, or full-width content bands (header rows, selection
/// stripes, scrollbars) paint over the outline's edge segments.
#[allow(clippy::too_many_arguments)]
fn panel_chrome(
    painter: &mut TextPainter,
    list: &mut PaintList,
    bounds: Rect,
    title: &str,
    right: &str,
    pal: &Palette,
    scale: f32,
) -> martensite::render::Rect {
    let b = to_paint(bounds);
    let s = f64::from(scale);
    let corner = Shape::rounded((6.0 * s) as f32);
    list.push_fill_shape(b, &corner, pal.surface);
    let title_h = f64::from(TITLE_H) * s;
    // The title bar rounds only its top corners so it meets the panel
    // silhouette flush; its radius shrinks by the 1px border inset.
    let title_shape = Shape::corners(
        CornerRadii::top(((6.0 * s - 1.0).max(0.0)) as f32),
        CornerStyle::Round,
    );
    list.push_fill_shape(
        krect(b.x0, b.y0, b.width(), title_h),
        &title_shape,
        pal.raised,
    );
    // Below ~120pt the right label is dropped — two ellipsized strings
    // butted together read worse than one clean title.
    let show_right = !right.is_empty() && b.width() >= 120.0 * s;
    let title_slot = if show_right {
        b.width() * 0.52
    } else {
        b.width() - 24.0 * s
    };
    let title_fit = painter.fit(title, 12.0 * scale, title_slot.max(1.0) as f32);
    painter.push(
        list,
        Point::new(b.x0 + 12.0 * s, b.y0 + 6.0 * s),
        &title_fit,
        12.0 * scale,
        pal.text,
        None,
    );
    if show_right {
        let right_fit = painter.fit(right, 12.0 * scale, (b.width() * 0.42).max(1.0) as f32);
        let w = painter.measure(&right_fit, 12.0 * scale);
        painter.push(
            list,
            Point::new(b.x1 - 12.0 * s - f64::from(w), b.y0 + 7.0 * s),
            &right_fit,
            12.0 * scale,
            pal.text,
            None,
        );
    }
    list.push_fill_rect(krect(b.x0, b.y0 + title_h, b.width(), 1.0), pal.border);
    krect(
        b.x0,
        b.y0 + title_h + 1.0,
        b.width(),
        (b.height() - title_h - 1.0).max(0.0),
    )
}

/// Paints the panel outline — accent focus ring when focused, hairline
/// otherwise — matching [`panel_chrome`]'s silhouette. Call this at the
/// END of a panel's `paint`, after every content band: headers, rows,
/// and scrollbars all span the full inner width and would otherwise
/// cover the outline's left/right/bottom segments.
fn panel_border(list: &mut PaintList, bounds: Rect, pal: &Palette, scale: f32, focused: bool) {
    let b = to_paint(bounds);
    let s = f64::from(scale);
    if focused {
        list.push_stroke_shape(
            krect(b.x0 + 1.0, b.y0 + 1.0, b.width() - 2.0, b.height() - 2.0),
            &Shape::rounded(((6.0 * s - 1.0).max(0.0)) as f32),
            2.0,
            pal.accent,
        );
    } else {
        list.push_stroke_shape(b, &Shape::rounded((6.0 * s) as f32), 1.0, pal.hairline());
    }
}

// ---------------------------------------------------------------------------
// GridPanel — virtualized 1M-row DataTable
// ---------------------------------------------------------------------------

/// Column headers — `ColumnConfig` carries widths but no title field
/// (F6), so the renderer owns the labels, in column order.
const GRID_HEADERS: [&str; 4] = ["PID", "CPU %", "MEMORY", "STATUS"];
/// Logical-pt floor for a dragged column — matches the stretch
/// column's "at least ~40pt" collapse floor in `column_layout`.
const GRID_MIN_COL_W: f32 = 40.0;
/// Logical-pt slop either side of a divider that grabs a resize drag.
const GRID_DIVIDER_GRAB: f32 = 4.0;

/// The process-metrics table: column-header sort toggling, click
/// selection (+ Shift range), wheel scrolling, full keyboard navigation,
/// an `F` alert-only filter toggle, all over the virtualized 1M-row
/// `DataTable` model.
pub struct GridPanel {
    table: DataTable<Vec<MetricRow>>,
    text: Mutex<TextPainter>,
    scale: Signal<f32>,
    bounds: Rect,
    focused: bool,
    /// F17 — modifiers tracked through the key stream.
    shift_held: bool,
    ctrl_held: bool,
    alert_rows: usize,
    /// Toolbar filter input — polled in `tick`; folded into the
    /// `RowFilter` together with the `F` alert-only toggle.
    filter_text: Signal<String>,
    /// The text currently baked into the table's filter — change
    /// detection so `set_filter` doesn't re-scan 1M rows per frame.
    applied_filter: String,
    /// `F` alert-only toggle — composes with the text filter.
    alert_only: bool,
    /// Clipboard payload sink — the app drains it into the OS
    /// clipboard each frame (the platform backend isn't `Send`, so it
    /// can't live in the widget).
    clipboard_out: Signal<Option<String>>,
    /// State shared with the `ContextMenu` popup (committed item).
    menu_shared: Arc<Mutex<MenuState>>,
    /// Whether a context menu is requested — reconciled by
    /// `sync_overlay` against the arena `OverlayLayer`.
    menu_open: bool,
    /// Live overlay entry id while the menu is up.
    menu_id: Option<u64>,
    /// Window-space position of the opening secondary press.
    menu_anchor: Vec2,
    /// Snapshot of the row the menu targets — committed actions read
    /// its cells (the table has no storage-index getter; the pressed
    /// row is always visible, so capture it at press time).
    context_row: Option<MetricRow>,
    /// "· copied" title flash after a successful clipboard write.
    copied_flash: Option<Instant>,
    /// Rubber-band overscroll driver (vertical, device px) — the
    /// `ScrollView::scroller` analogue. The table's `scroll_offset`
    /// stays the authoritative clamped position; the band adds
    /// transient stretch visuals past the boundaries.
    band: RubberBandScroller,
    /// Live column-resize drag: `(configured column, grab offset)` —
    /// the offset is `divider_x - press_x` so the line doesn't jump
    /// when the press lands off-center inside the grab zone.
    resizing: Option<(usize, f32)>,
    /// The shipped column widths — the double-click reset target.
    default_widths: Vec<f32>,
}

impl GridPanel {
    pub fn new(
        scale: Signal<f32>,
        filter_text: Signal<String>,
        clipboard_out: Signal<Option<String>>,
    ) -> Self {
        let rows = gen_rows(1_000_000);
        let alert_rows = alert_count(&rows);
        // 26px rows keep the WCAG 2.5.8 24px target floor at 1x.
        let mut table = DataTable::new(rows, 30.0 * scale.get().max(1.0));
        let mut c_pid = ColumnConfig::new(96.0);
        c_pid.set_sortable(true);
        let mut c_cpu = ColumnConfig::new(110.0);
        c_cpu.set_sortable(true);
        let mut c_mem = ColumnConfig::new(130.0);
        c_mem.set_sortable(true);
        let c_st = ColumnConfig::new(90.0);
        table.set_columns(vec![c_pid, c_cpu, c_mem, c_st]);
        table.sort_by(1, ColumnSort::Descending, |a, b| {
            a.cpu_milli.cmp(&b.cpu_milli)
        });
        let default_widths = table.columns().iter().map(|c| c.width()).collect();
        Self {
            table,
            text: Mutex::new(TextPainter::new()),
            scale,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            focused: false,
            shift_held: false,
            ctrl_held: false,
            alert_rows,
            filter_text,
            applied_filter: String::new(),
            alert_only: false,
            clipboard_out,
            menu_shared: Arc::new(Mutex::new(MenuState {
                items: Vec::new(),
                highlighted: 0,
                committed: None,
                entrance: None,
            })),
            menu_open: false,
            menu_id: None,
            menu_anchor: Vec2::ZERO,
            context_row: None,
            copied_flash: None,
            band: RubberBandScroller::new(0.0, 0.0),
            resizing: None,
            default_widths,
        }
    }

    fn s(&self) -> f32 {
        self.scale.get()
    }

    /// Rebuilds the table's `RowFilter` from the two live filter axes:
    /// the `F` alert-only toggle and the toolbar's text field. The text
    /// needle is case-insensitive and matches pid, status word
    /// (`alert`/`ok`), or the raw memory figure.
    fn apply_filter(&mut self) {
        let needle = self.applied_filter.to_lowercase();
        let alert_only = self.alert_only;
        if needle.is_empty() && !alert_only {
            if self.table.filter_active() {
                self.table.clear_filter();
                // `clear_filter` clamps the scroll offset — re-sync
                // the band (same reason as `set_filter` below).
                self.sync_band();
            }
            return;
        }
        self.table.set_filter(RowFilter::new(move |r: &MetricRow| {
            if alert_only && !r.alert {
                return false;
            }
            if needle.is_empty() {
                return true;
            }
            r.pid.to_string().contains(&needle)
                || r.mem_kib.to_string().contains(&needle)
                || (if r.alert { "alert" } else { "ok" }).contains(&needle)
        }));
        // `set_filter` clamps the scroll offset — re-sync the band so
        // it can't fight the table's clamped position.
        self.sync_band();
    }

    fn row_h(&self) -> f32 {
        30.0 * self.s().max(1.0)
    }

    /// Row viewport height in device px — the value pushed into
    /// `DataTable::set_viewport_height` in `layout` and the band's
    /// viewport size. One formula keeps the two in lockstep.
    fn rows_viewport_h(&self) -> f32 {
        (self.bounds.size.y - (TITLE_H + GRID_HEADER_H) * self.s() - 2.0).max(0.0)
    }

    /// Aligns the rubber-band scroller's sizes and offset with the
    /// table's authoritative scroll state — the `ScrollView::
    /// sync_scroller` analogue. Runs from `layout` and after every
    /// scroll change the band didn't originate (keyboard `handle_key`,
    /// filter clamps, scale-driven row-height rescales).
    fn sync_band(&mut self) {
        self.band
            .set_content_size(self.table.display_row_count() as f32 * self.row_h());
        self.band.set_viewport_size(self.rows_viewport_h());
        // `drag` doubles as the offset writer — it interrupts a live
        // spring from its current position, so an external scroll
        // change can't leave the band animating against stale state.
        let diff = self.table.scroll_offset() - self.band.content_offset();
        if diff != 0.0 {
            self.band.drag(diff);
            // Re-arm after the drag: `release` is a no-op in bounds,
            // but if the drag left residual overshoot (the new target
            // didn't absorb the interrupted spring's OOB position)
            // this starts the spring-back — otherwise the band sits
            // out-of-bounds with no spring, `is_settled` never trips,
            // `tick` spins dirty, and `visible_range` clamps to an
            // empty row area. It must run only inside this branch:
            // called while a healthy spring is in flight, `release`
            // would restart it from the stale raw offset — a visible
            // snap-back.
            self.band.release(0.0);
        }
    }

    /// Column (x, width) in device px; the last column stretches.
    /// Columns that can't fit inside the panel are dropped rightmost-
    /// first rather than painted past the clip — a real table collapses
    /// under pressure instead of lying about its extent.
    fn column_layout(&self) -> Vec<(f32, f32)> {
        let pad = 12.0 * self.s();
        let mut x = self.bounds.min_x() + pad;
        let inner_right = self.bounds.max_x() - pad;
        let widths: Vec<f32> = self
            .table
            .columns()
            .iter()
            .map(|c| c.width() * self.s())
            .collect();
        let mut cols = Vec::with_capacity(widths.len());
        for (i, w) in widths.iter().enumerate() {
            if i == widths.len() - 1 {
                // Stretch column — only if at least ~40pt remains.
                let w = inner_right - x;
                if w < 40.0 * self.s() {
                    break;
                }
                cols.push((x, w));
            } else {
                if x + w > inner_right {
                    break;
                }
                cols.push((x, *w));
                x += w;
            }
        }
        cols
    }

    /// The column-header band: below the title bar + hairline, above
    /// `rows_top`. Sorting clicks are only meaningful inside this band —
    /// the title bar above it must not sort.
    fn header_band(&self) -> (f32, f32) {
        let s = self.s();
        let top = self.bounds.min_y() + TITLE_H * s + 1.0;
        (top, top + GRID_HEADER_H * s + 1.0)
    }

    /// `rows_top` in window coordinates — matches the painter exactly
    /// (title + hairline + column-header band + hairline).
    fn rows_top(&self) -> f32 {
        let s = self.s();
        self.bounds.min_y() + TITLE_H * s + 1.0 + GRID_HEADER_H * s + 1.0
    }

    /// The px offset the row window is currently rendered at — the
    /// rubber-banded visible offset while the band is unsettled, else
    /// the table's clamped offset. Paint AND hit-testing must share
    /// this or clicks during the spring-back land on the wrong row.
    fn render_offset(&self) -> f32 {
        if self.band.is_settled() {
            self.table.scroll_offset()
        } else {
            self.band.visible_offset()
        }
    }

    /// Storage index of the row at window-y `y`, via the visible window
    /// (display position → storage through the same mapping the painter
    /// uses — `visible_rows` yields storage indices).
    fn row_at(&self, y: f32) -> Option<usize> {
        self.row_snapshot(y).map(|(idx, _)| idx)
    }

    /// Like [`row_at`](Self::row_at) but also clones the row's data —
    /// the context menu snapshots cells at press time.
    fn row_snapshot(&self, y: f32) -> Option<(usize, MetricRow)> {
        // Above `rows_top` a press lands on the column header — with a
        // fractional scroll offset the clipped top sliver of the first
        // visible row still hides there, and must not answer the hit.
        if y < self.rows_top() {
            return None;
        }
        // Rows sit at `(first + n)·row_h − render_off` — the same
        // mapping the painter uses, including mid-spring stretch.
        let first = self.table.visible_range().start;
        let row = ((y - self.rows_top() + self.render_offset()) / self.row_h()).floor();
        if row < first as f32 {
            return None;
        }
        self.table
            .visible_rows()
            .nth(row as usize - first)
            .map(|(idx, r)| (idx, r.clone()))
    }

    fn toggle_sort(&mut self, col: usize) {
        let dir = if self.table.sort_column() == Some(col)
            && self.table.sort_direction() == ColumnSort::Ascending
        {
            ColumnSort::Descending
        } else {
            ColumnSort::Ascending
        };
        match col {
            0 => self.table.sort_by(col, dir, |a, b| a.pid.cmp(&b.pid)),
            1 => self
                .table
                .sort_by(col, dir, |a, b| a.cpu_milli.cmp(&b.cpu_milli)),
            2 => self
                .table
                .sort_by(col, dir, |a, b| a.mem_kib.cmp(&b.mem_kib)),
            _ => {}
        }
    }

    /// Live divider positions — `(divider x, configured column)` for
    /// every visible column's right edge, except the stretch column's:
    /// its edge is the panel boundary, so there's nothing to drag. A
    /// column whose right-side siblings all collapsed keeps its
    /// divider — shrinking it is how they come back. Because
    /// `column_layout` walks configured columns in order and *breaks*
    /// on the first that doesn't fit, the visible index IS the
    /// configured index.
    fn divider_edges(&self) -> Vec<(f32, usize)> {
        let Some(last) = self.table.column_count().checked_sub(1) else {
            return Vec::new();
        };
        self.column_layout()
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != last && self.table.columns()[*i].resizable())
            .map(|(i, (x0, w))| (*x0 + *w, i))
            .collect()
    }

    /// The divider under `position` inside the header band —
    /// `(configured column, grab offset)` where the offset keeps the
    /// drag from jumping when the press lands off-center in the zone.
    fn divider_at(&self, position: Vec2) -> Option<(usize, f32)> {
        let (top, bottom) = self.header_band();
        if position.y < top || position.y >= bottom {
            return None;
        }
        let grab = GRID_DIVIDER_GRAB * self.s();
        self.divider_edges()
            .into_iter()
            .find(|(x, _)| (position.x - *x).abs() <= grab)
            .map(|(x, col)| (col, x - position.x))
    }

    /// Sets `col`'s logical width from a drag position, clamped to the
    /// floor and to the panel's inner right edge — past that the
    /// column would collapse out of `column_layout` entirely and the
    /// drag would lose its anchor.
    fn resize_to(&mut self, col: usize, grab_off: f32, x: f32) {
        let s = self.s();
        let Some(&(x0, _)) = self.column_layout().get(col) else {
            return;
        };
        let max_w = (self.bounds.max_x() - 12.0 * s - x0) / s;
        let w = ((x + grab_off - x0) / s).clamp(GRID_MIN_COL_W, max_w.max(GRID_MIN_COL_W));
        self.table.set_column_width(col, w);
    }
}

impl Widget for GridPanel {
    fn debug_name(&self) -> &'static str {
        "Process Grid"
    }
    fn measure(&mut self, _cx: &mut LayoutContext, _c: LayoutConstraints) -> Vec2 {
        Vec2::new(620.0, 420.0)
    }

    fn min_render(&self) -> RenderMinimum {
        // Below ~300×170pt the table cannot show headers plus a row
        // meaningfully — degrade to a placeholder badge instead of a
        // half-rendered grid.
        RenderMinimum::new(Vec2::new(300.0, 170.0)).with_policy(UnderflowPolicy::Fallback)
    }

    fn paint_underflow(&self, cx: &mut PaintContext) {
        let pal = Palette::from_theme(cx.theme);
        let s = self.s();
        let mut text = self.text.lock();
        let b = krect(
            f64::from(self.bounds.min_x()),
            f64::from(self.bounds.min_y()),
            f64::from(self.bounds.width()),
            f64::from(self.bounds.height()),
        );
        cx.list.push_fill_rect(b, pal.surface);
        cx.list.push_stroke_rect(b, 1.0, pal.border);
        let msg = "PROCESS GRID — enlarge to restore";
        let size = 11.0 * s;
        let tw = f64::from(text.measure(msg, size));
        let x = (b.x0 + (b.width() - tw) * 0.5).max(b.x0 + 2.0);
        let y = b.y0 + (b.height() - f64::from(size)) * 0.5;
        text.push(cx.list, Point::new(x, y), msg, size, pal.text_muted, None);
    }

    fn layout(&mut self, _cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        // Re-arm the model's row height from the live scale signal —
        // `set_row_height` preserves the scroll position in rows.
        self.table.set_row_height(self.row_h());
        self.table.set_viewport_height(self.rows_viewport_h());
        // Row-height rescales and viewport clamps move the table's
        // offset — keep the band aligned (`ScrollView::layout` ends
        // the same way).
        self.sync_band();
    }

    fn tick(&mut self, dt: Duration) -> bool {
        let mut dirty = false;
        // Toolbar filter input — only re-apply on change; `set_filter`
        // re-scans the backing store so per-frame churn is real work.
        let text = self.filter_text.get();
        if text != self.applied_filter {
            self.applied_filter = text;
            self.apply_filter();
            dirty = true;
        }
        if self
            .copied_flash
            .is_some_and(|t| t.elapsed() > Duration::from_millis(1500))
        {
            self.copied_flash = None;
            dirty = true;
        }
        // Context-menu entrance spring — overlay entries never see
        // `tick`, so the owner advances the shared solver each frame
        // while the popup is animating. When the entry is gone
        // (commit/dismissal mid-entrance) clear the dead spring so it
        // can't keep this widget dirty.
        {
            let mut state = self.menu_shared.lock();
            if self.menu_id.is_none() {
                state.entrance = None;
            } else if let Some(spring) = state.entrance.as_mut() {
                spring.advance(dt.as_secs_f32());
                if spring.settle_threshold() {
                    state.entrance = None;
                }
                dirty = true;
            }
        }
        // Rubber-band spring-back — repaint while animating; landing
        // writes the boundary back so band and table stay consistent.
        if !self.band.is_settled() {
            self.band.update(dt.as_secs_f32());
            self.table.set_scroll_offset(self.band.content_offset());
            dirty = true;
        }
        dirty
    }

    /// Reconciles the context-menu overlay entry with the requested
    /// state — same pattern as `Dropdown::sync_overlay`: open when
    /// armed, notice layer-level dismissal, drain committed items into
    /// real actions (clipboard payloads), close when disarmed.
    fn sync_overlay(&mut self, overlay: &mut OverlayLayer) {
        // The layer dismissed the menu (outside press / Escape).
        if let Some(id) = self.menu_id {
            if !overlay.is_open(id) {
                self.menu_id = None;
                self.menu_open = false;
            }
        }
        // A committed item becomes a clipboard payload for the target
        // row; the app performs the OS write from the signal.
        if let Some(action) = self.menu_shared.lock().committed.take() {
            if let Some(row) = &self.context_row {
                let payload = match action {
                    0 => format!("{}", row.pid),
                    _ => format!(
                        "{},{:.1},{},{}",
                        row.pid,
                        f64::from(row.cpu_milli) / 1000.0,
                        fmt_mem(row.mem_kib),
                        if row.alert { "ALERT" } else { "OK" }
                    ),
                };
                self.clipboard_out.set(Some(payload));
                self.copied_flash = Some(Instant::now());
            }
            self.menu_open = false;
        }
        if self.menu_open && self.menu_id.is_none() {
            {
                let mut state = self.menu_shared.lock();
                state.items = vec!["Copy PID".into(), "Copy row (CSV)".into()];
                state.highlighted = 0;
                state.committed = None;
                // Entrance spring — `tick` advances it; the popup's
                // `paint` samples it for the translate + fade.
                state.arm_entrance();
            }
            let menu = ContextMenu::new(Arc::clone(&self.menu_shared));
            self.menu_id =
                Some(overlay.open(Box::new(menu), OverlayAnchor::Pointer(self.menu_anchor)));
        } else if !self.menu_open {
            if let Some(id) = self.menu_id.take() {
                overlay.close(id);
            }
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerPressed {
                position,
                button: PointerButton::Primary,
                count,
                ..
            } => {
                // Divider grab wins over the sort click — a press in
                // the slop zone is a resize (or, on double-click, a
                // reset to the shipped width), never a sort.
                if let Some((col, grab_off)) = self.divider_at(*position) {
                    if *count >= 2 {
                        if let Some(&w) = self.default_widths.get(col) {
                            self.table.set_column_width(col, w);
                        }
                    } else {
                        self.resizing = Some((col, grab_off));
                    }
                    return EventResponse::CapturePointer;
                }
                let (hdr_top, hdr_bottom) = self.header_band();
                if position.y >= hdr_top && position.y < hdr_bottom {
                    for (i, (x0, w)) in self.column_layout().iter().enumerate() {
                        if position.x >= *x0 && position.x < x0 + w {
                            self.toggle_sort(i);
                            return EventResponse::CaptureFocus;
                        }
                    }
                } else if let Some(idx) = self.row_at(position.y) {
                    self.table.set_focused_row(Some(idx));
                    if self.shift_held {
                        self.table.selection_mut().extend_to(idx);
                    } else {
                        let sel = self.table.selection_mut();
                        sel.clear();
                        sel.select(idx);
                    }
                }
                EventResponse::CaptureFocus
            }
            WidgetEvent::PointerPressed {
                position,
                button: PointerButton::Secondary,
                ..
            } => {
                // Right-click selects the row under the cursor (standard
                // context-menu UX) and arms the popup; `sync_overlay`
                // opens it at the press position.
                if let Some((idx, row)) = self.row_snapshot(position.y) {
                    self.table.set_focused_row(Some(idx));
                    let sel = self.table.selection_mut();
                    sel.clear();
                    sel.select(idx);
                    self.context_row = Some(row);
                    self.menu_anchor = *position;
                    self.menu_open = true;
                }
                EventResponse::CaptureFocus
            }
            WidgetEvent::PointerMoved { position } => {
                if let Some((col, grab_off)) = self.resizing {
                    self.resize_to(col, grab_off, position.x);
                    EventResponse::RequestRepaint
                } else {
                    EventResponse::Ignored
                }
            }
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                ..
            } => {
                if self.resizing.take().is_some() {
                    EventResponse::ReleasePointer
                } else {
                    EventResponse::Ignored
                }
            }
            WidgetEvent::Scroll { delta, .. } => {
                self.sync_band();
                // Wheel input through the band: `drag` accumulates the
                // unclamped offset (stretch past the boundaries), then
                // `release` re-arms the spring-back — a no-op while in
                // bounds, so every event is safe. Note the spring
                // starts from the raw accumulated offset: the 0.55
                // stretch coefficient only applies while dragging, so
                // a wheel fling shows the full accumulated stretch.
                // The velocity is the coarse one-tick-per-frame
                // estimate.
                self.band.drag(-delta.y);
                self.band.release(-delta.y * 60.0);
                // The table keeps the clamped offset — out-of-bounds
                // stretch is render-only via `band.visible_offset`.
                self.table.set_scroll_offset(self.band.content_offset());
                EventResponse::RequestRepaint
            }
            WidgetEvent::KeyPressed { key, repeat } => {
                match key.as_str() {
                    "Shift" => self.shift_held = true,
                    "Control" => self.ctrl_held = true,
                    "f" | "F" if !repeat => {
                        self.alert_only = !self.alert_only;
                        self.apply_filter();
                    }
                    _ => {
                        if let Some(action) =
                            KeyAction::from_key_name(key, self.shift_held, self.ctrl_held)
                        {
                            self.table.handle_key(action);
                            // `handle_key` scrolls to keep the focused
                            // row visible — re-sync so the band never
                            // fights keyboard navigation.
                            self.sync_band();
                        }
                    }
                }
                EventResponse::Handled
            }
            WidgetEvent::KeyReleased { key } => {
                match key.as_str() {
                    "Shift" => self.shift_held = false,
                    "Control" => self.ctrl_held = false,
                    _ => {}
                }
                EventResponse::Handled
            }
            WidgetEvent::FocusGained => {
                self.focused = true;
                EventResponse::RequestRepaint
            }
            WidgetEvent::FocusLost => {
                self.focused = false;
                // The OS can swallow the KeyReleased while we're
                // unfocused — clear tracked modifiers so a stuck flag
                // doesn't ghost into the next KeyAction.
                self.shift_held = false;
                self.ctrl_held = false;
                EventResponse::RequestRepaint
            }
            // AT focus/activation requests honour the pending-focus
            // protocol: CaptureFocus records a request the app drains
            // into the FocusManager.
            WidgetEvent::SemanticAction(SemanticAction::Focus | SemanticAction::Click) => {
                EventResponse::CaptureFocus
            }
            _ => EventResponse::Ignored,
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Table);
        node.set_label(format!(
            "Process Grid — {} of 1,000,000 rows, {} selected",
            self.table.display_row_count(),
            self.table.selection().selected_count()
        ));
    }

    fn paint(&self, cx: &mut PaintContext) {
        let pal = Palette::from_theme(cx.theme);
        let pal = &pal;
        let s = self.s();
        let mut text = self.text.lock();
        let right = format!(
            "{} rows · {} sel · {} alerts{}{}",
            fmt_count(self.table.display_row_count()),
            self.table.selection().selected_count(),
            fmt_count(self.alert_rows),
            if self.table.filter_active() {
                " · FILTERED"
            } else {
                ""
            },
            if self.copied_flash.is_some() {
                " · copied"
            } else {
                ""
            }
        );
        let inner = panel_chrome(
            &mut text,
            cx.list,
            self.bounds,
            "PROCESS GRID",
            &right,
            pal,
            s,
        );
        let sd = f64::from(s);

        // Column header row.
        let header_h = f64::from(GRID_HEADER_H) * sd;
        cx.list.push_fill_rect(
            krect(inner.x0, inner.y0, inner.width(), header_h),
            pal.raised,
        );
        for (i, (x0, w)) in self.column_layout().iter().enumerate() {
            let hdr = text.fit(GRID_HEADERS[i], 12.0 * s, (*w - 16.0 * s).max(1.0));
            text.push(
                cx.list,
                Point::new(f64::from(*x0) + 4.0 * sd, inner.y0 + 6.0 * sd),
                &hdr,
                12.0 * s,
                pal.text,
                None,
            );
            if self.table.sort_column() == Some(i) {
                // Sort arrow — filled triangle, accent.
                let ssd = f64::from(s);
                let (aw, ah) = (7.0 * ssd, 5.0 * ssd);
                let ax = f64::from(*x0) + f64::from(*w) - 14.0 * ssd;
                let ay = inner.y0 + (header_h - ah) / 2.0;
                let mut path = BezPath::new();
                if self.table.sort_direction() == ColumnSort::Ascending {
                    path.move_to((ax, ay + ah));
                    path.line_to((ax + aw / 2.0, ay));
                    path.line_to((ax + aw, ay + ah));
                } else {
                    path.move_to((ax, ay));
                    path.line_to((ax + aw / 2.0, ay + ah));
                    path.line_to((ax + aw, ay));
                }
                path.close_path();
                cx.list.push_path(path, pal.accent);
            }
        }
        cx.list.push_fill_rect(
            krect(inner.x0, inner.y0 + header_h, inner.width(), 1.0),
            pal.border,
        );

        // Rows — clipped to the region below the header.
        let rows_top = inner.y0 + header_h + 1.0;
        let rows_bottom = inner.y1;
        cx.list.push_clip(krect(
            inner.x0,
            rows_top,
            inner.width(),
            rows_bottom - rows_top,
        ));
        let row_h = f64::from(self.row_h());
        let cols = self.column_layout();
        let focused_row = self.table.focused_row();
        // While the band is out of bounds / springing, rows render at
        // the rubber-banded visible offset (device px — may be negative
        // or past the max, which is the stretch). Settled this is
        // exactly `scroll_offset % row_h` against the first visible row.
        // `row_snapshot` uses the same helper so clicks agree with paint.
        let render_off = f64::from(self.render_offset());
        let first = self.table.visible_range().start;
        for (n, (idx, row)) in self.table.visible_rows().enumerate() {
            let ry = rows_top + (first + n) as f64 * row_h - render_off;
            if ry + row_h < rows_top || ry > rows_bottom {
                continue;
            }
            let selected = self.table.selection().is_selected(idx);
            if selected {
                cx.list.push_fill_rect(
                    krect(inner.x0, ry, inner.width(), row_h),
                    pal.selection_tint(),
                );
            } else if n % 2 == 1 {
                cx.list.push_fill_rect(
                    krect(inner.x0, ry, inner.width(), row_h),
                    Palette::alpha(pal.raised, 110),
                );
            }
            if focused_row == Some(idx) {
                cx.list.push_stroke_rect(
                    krect(inner.x0 + 1.0, ry, inner.width() - 2.0, row_h),
                    1.0,
                    pal.accent,
                );
            }
            // Row separator — under the cell text, which follows.
            cx.list.push_fill_rect(
                krect(inner.x0, ry + row_h - 1.0, inner.width(), 1.0),
                Palette::alpha(pal.border, 50),
            );
            let ty = ry + (row_h - 13.0 * sd) / 2.0;
            // A straddling row keeps its stripe/focus ring, but its text
            // can still paint zero pixels: `TextPainter` places the first
            // baseline ≈1·font_px below the block top (line_height =
            // 1.25·font_px → 0.8·lh), and the audit's probe puts the
            // glyph-box top another 0.8·font_px above the baseline — so
            // the audit's probe box spans `baseline − 0.8·fp ..
            // baseline + 0.25·fp`, i.e. `ty + 0.2·font_px ..
            // ty + 1.25·font_px`. When it sits fully past either clip
            // edge the runs produce nothing — and a record left in the
            // list also earns a spurious contrast check against
            // whatever fill it overlaps.
            let font_px = 12.0 * s;
            if ty + 0.2 * f64::from(font_px) >= rows_bottom
                || ty + 1.25 * f64::from(font_px) <= rows_top
            {
                continue;
            }
            // Cells: numeric columns right-aligned; status is a chip.
            let pid = format!("{}", row.pid);
            let cpu = format!("{:.1}", f64::from(row.cpu_milli) / 1000.0);
            let mem = fmt_mem(row.mem_kib);
            let cells: [(usize, String, [u8; 4]); 3] = [
                (0, pid, if selected { pal.text } else { pal.text_muted }),
                (
                    1,
                    cpu,
                    if row.alert {
                        pal.error
                    } else if row.cpu_milli > 70_000 {
                        pal.warn
                    } else {
                        pal.text
                    },
                ),
                (2, mem, pal.text),
            ];
            for (ci, value, color) in cells {
                let Some(&(x0, w)) = cols.get(ci) else {
                    continue;
                };
                let tw = text.measure(&value, font_px);
                text.push(
                    cx.list,
                    Point::new(f64::from(x0 + w) - 10.0 * sd - f64::from(tw), ty),
                    &value,
                    font_px,
                    color,
                    Some(w),
                );
            }
            // Status chip — only when the column survived the collapse.
            let Some(&(x0, w)) = cols.get(3) else {
                continue;
            };
            let label = if row.alert { "ALERT" } else { "OK" };
            let chip_w = 52.0 * sd;
            let chip_h = 18.0 * sd;
            let chip_x = f64::from(x0 + w) - 10.0 * sd - chip_w;
            let chip_y = ry + (row_h - chip_h) / 2.0;
            let (chip_bg, chip_fg) = if row.alert {
                (Palette::alpha(pal.error, 60), pal.text)
            } else {
                (Palette::alpha(pal.ok, 40), pal.text)
            };
            cx.list.push_fill_shape(
                krect(chip_x, chip_y, chip_w, chip_h),
                &Shape::squircle((chip_h * 0.35) as f32),
                chip_bg,
            );
            let label_fit = text.fit(label, 12.0 * s, chip_w as f32);
            let lw = text.measure(&label_fit, 12.0 * s);
            text.push(
                cx.list,
                Point::new(chip_x + (chip_w - f64::from(lw)) / 2.0, chip_y + 2.0 * sd),
                &label_fit,
                12.0 * s,
                chip_fg,
                None,
            );
        }

        // Scrollbar — track + proportional thumb.
        let total_h = self.table.display_row_count() as f64 * row_h;
        let view_h = rows_bottom - rows_top;
        if total_h > view_h {
            let track_x = inner.x1 - 6.0 * sd;
            cx.list.push_fill_rect(
                krect(track_x, rows_top, 4.0 * sd, rows_bottom - rows_top),
                Palette::alpha(pal.raised, 140),
            );
            let thumb_h = ((view_h / total_h) * (rows_bottom - rows_top)).max(20.0 * sd);
            let max_off = (total_h - view_h).max(1.0);
            let frac = f64::from(self.table.scroll_offset()) / max_off;
            let thumb_y = rows_top + frac * (rows_bottom - rows_top - thumb_h);
            cx.list.push_fill_shape(
                krect(track_x, thumb_y, 4.0 * sd, thumb_h),
                &Shape::PILL,
                pal.text_muted,
            );
        }
        cx.list.pop_clip();
        // Column separators — solid through the header band, faint
        // through the rows; the live-drag divider turns accent.
        for (x, col) in self.divider_edges() {
            let dx = f64::from(x);
            let active = self.resizing.is_some_and(|(c, _)| c == col);
            cx.list.push_fill_rect(
                krect(dx, inner.y0 + 3.0 * sd, 1.0, header_h - 6.0 * sd),
                if active { pal.accent } else { pal.border },
            );
            cx.list.push_fill_rect(
                krect(dx, rows_top, 1.0, rows_bottom - rows_top),
                if active {
                    pal.accent
                } else {
                    Palette::alpha(pal.border, 70)
                },
            );
        }
        // Outline last — the header band and row fills span the full
        // inner width and would cover its edge segments.
        panel_border(cx.list, self.bounds, pal, s, self.focused);
    }
}

// ---------------------------------------------------------------------------
// TelemetryPanel — live Signal-driven chart
// ---------------------------------------------------------------------------

/// Telemetry chart: two live `Signal<f64>` feeds (the app holds clones
/// for the header KPIs) rendered through the blessed `Chart` model —
/// `AreaSeries` under-fill (the toolbar's glow toggle), `LineSeries`
/// strokes, and `ScatterSeries` outlier markers, all projected with
/// `Chart::project`. `tick` advances the model — Space pauses.
pub struct TelemetryPanel {
    text: Mutex<TextPainter>,
    scale: Signal<f32>,
    bounds: Rect,
    focused: bool,
    /// Shared with the app's header KPIs — this panel is the writer.
    pub cpu: Signal<f64>,
    pub mem: Signal<f64>,
    /// Toolbar-driven controls — shared cells: the toolbar's Pause
    /// button and sample-rate slider write them, Space writes `paused`
    /// too so both controls stay in sync.
    paused: Signal<bool>,
    glow: Signal<bool>,
    tick_ms: Signal<f64>,
    /// Toolbar "alerts" switch → the inline `Banner` strip. The
    /// banner's × writes the cell off (the toolbar mirrors it back).
    alerts: Signal<bool>,
    banner: Banner,
    /// The banner's strip rect — zero when alerts are off.
    banner_rect: Rect,
    phase: f64,
    history: VecDeque<(f64, f64)>,
    elapsed: Duration,
}

impl TelemetryPanel {
    const CAP: usize = 240;
    /// Cpu% at which an outlier marker escalates from warn to error.
    const ALERT_PCT: f64 = 90.0;

    pub fn new(
        scale: Signal<f32>,
        cpu: Signal<f64>,
        mem: Signal<f64>,
        paused: Signal<bool>,
        glow: Signal<bool>,
        tick_ms: Signal<f64>,
        alerts: Signal<bool>,
    ) -> Self {
        Self {
            text: Mutex::new(TextPainter::new()),
            scale,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            focused: false,
            cpu,
            mem,
            paused,
            glow,
            tick_ms,
            alerts,
            banner: Banner::new(
                Severity::Warning,
                "Row alerts live — outliers flagged in the grid",
            )
            .with_text_painter(martensite::text_paint::shared_painter()),
            banner_rect: Rect::new(0.0, 0.0, 0.0, 0.0),
            phase: 0.0,
            history: VecDeque::with_capacity(Self::CAP + 1),
            elapsed: Duration::ZERO,
        }
    }

    fn s(&self) -> f32 {
        self.scale.get()
    }
}

impl Widget for TelemetryPanel {
    fn debug_name(&self) -> &'static str {
        "Telemetry"
    }
    fn measure(&mut self, _cx: &mut LayoutContext, _c: LayoutConstraints) -> Vec2 {
        Vec2::new(560.0, 300.0)
    }

    fn min_render(&self) -> RenderMinimum {
        // Below ~260×150pt the chart is illegible even after the
        // widget's own label-thinning — veil the region rather than
        // paint noise.
        RenderMinimum::new(Vec2::new(260.0, 150.0)).with_policy(UnderflowPolicy::Scrim)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        // The banner strips the top of the content area while the
        // alerts switch is on; the plot shrinks under it.
        let s = self.s();
        if self.alerts.get() {
            let top = bounds.origin.y + TITLE_H * s + 6.0 * s;
            self.banner_rect = Rect::new(
                bounds.origin.x + 8.0 * s,
                top,
                (bounds.size.x - 16.0 * s).max(0.0),
                36.0 * s,
            );
            cx.layout_child(&mut self.banner, self.banner_rect);
        } else {
            self.banner_rect = Rect::new(bounds.origin.x, bounds.origin.y, 0.0, 0.0);
        }
    }

    fn tick(&mut self, dt: Duration) -> bool {
        // Banner × → write the shared cell off; the toolbar switch
        // mirrors it back on its next tick.
        if self.banner.take_dismissed() {
            self.alerts.set(false);
        }
        self.elapsed += dt;
        if self.paused.get() {
            return false;
        }
        // Toolbar slider drives the cadence — 20 ms frantic to 500 ms
        // glacial; 100 ms is the default legible trace.
        if self.elapsed < Duration::from_millis(self.tick_ms.get() as u64) {
            return true;
        }
        self.elapsed = Duration::ZERO;
        self.phase += 0.11;
        let t = self.phase;
        // Smooth primary oscillation + a harmonic + deterministic jitter.
        let cpu = (0.52 + 0.22 * t.sin() + 0.09 * (t * 2.7).sin() + 0.03 * (t * 13.0).cos())
            .clamp(0.02, 0.98);
        let mem = (0.61 + 0.14 * (t * 0.43 + 1.7).sin() + 0.02 * (t * 7.0).cos()).clamp(0.05, 0.97);
        self.cpu.set(cpu);
        self.mem.set(mem);
        self.history.push_back((cpu, mem));
        if self.history.len() > Self::CAP {
            self.history.pop_front();
        }
        true
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        // The banner's × is the only interactive element — forward
        // positional events that land inside its rect.
        if self.alerts.get() {
            if let Some(pos) = cx.event.position() {
                if self.banner_rect.contains(pos) {
                    return self.banner.event(cx);
                }
            }
        }
        match cx.event {
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                ..
            } => EventResponse::CaptureFocus,
            WidgetEvent::KeyPressed { key, repeat } => {
                if key == " " && !repeat {
                    self.paused.set(!self.paused.get());
                }
                EventResponse::Handled
            }
            WidgetEvent::FocusGained => {
                self.focused = true;
                EventResponse::RequestRepaint
            }
            WidgetEvent::FocusLost => {
                self.focused = false;
                EventResponse::RequestRepaint
            }
            // AT focus/activation requests honour the pending-focus
            // protocol: CaptureFocus records a request the app drains
            // into the FocusManager.
            WidgetEvent::SemanticAction(SemanticAction::Focus | SemanticAction::Click) => {
                EventResponse::CaptureFocus
            }
            _ => EventResponse::Ignored,
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Image);
        node.set_label(format!(
            "Telemetry — CPU {:.0}%, memory {:.0}%, {} samples{}",
            self.cpu.get() * 100.0,
            self.mem.get() * 100.0,
            self.history.len(),
            if self.paused.get() { ", paused" } else { "" }
        ));
    }

    fn child_count(&self) -> usize {
        usize::from(self.alerts.get())
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        if index == 0 && self.alerts.get() {
            Some(&self.banner)
        } else {
            None
        }
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        if index == 0 && self.alerts.get() {
            Some(&mut self.banner)
        } else {
            None
        }
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        if index == 0 && self.alerts.get() {
            Some(self.banner_rect)
        } else {
            None
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let pal = Palette::from_theme(cx.theme);
        let pal = &pal;
        let s = self.s();
        let mut text = self.text.lock();
        let state = if self.paused.get() { "PAUSED" } else { "LIVE" };
        let full = format!(
            "cpu {:>3.0}% · mem {:>3.0}% · {state}",
            self.cpu.get() * 100.0,
            self.mem.get() * 100.0
        );
        // Degrade to the compact form when the title-bar slot can't
        // fit the full readout — never a mid-word ellipsis.
        let slot = (self.bounds.width() * 0.42).max(1.0);
        let right = if text.measure(&full, 12.0 * s) <= slot {
            full
        } else {
            format!(
                "{:>3.0}% · {:>3.0}%",
                self.cpu.get() * 100.0,
                self.mem.get() * 100.0
            )
        };
        let inner = panel_chrome(&mut text, cx.list, self.bounds, "TELEMETRY", &right, pal, s);
        let sd = f64::from(s);
        let pad = 14.0 * sd;
        let label_w = 34.0 * sd;
        let legend_h = 20.0 * sd;
        let plot = krect(
            inner.x0 + label_w,
            inner.y0 + pad * 0.6,
            (inner.width() - label_w - pad).max(1.0),
            (inner.height() - pad * 0.6 - legend_h - pad * 0.4).max(1.0),
        );

        // Auto-bounds across both series — Chart::bounds() is the model
        // doing its own min/max scan; y pinned to the 0..100% domain so
        // the gridlines stay meaningful.
        let mut chart = Chart::new();
        let cpu_pts: Vec<CPoint> = self
            .history
            .iter()
            .enumerate()
            .map(|(i, (c, _))| CPoint {
                x: i as f64,
                y: c * 100.0,
            })
            .collect();
        let mem_pts: Vec<CPoint> = self
            .history
            .iter()
            .enumerate()
            .map(|(i, (_, m))| CPoint {
                x: i as f64,
                y: m * 100.0,
            })
            .collect();
        // The toolbar's glow toggle rides the blessed area series now —
        // the under-fill lives in the chart model instead of a
        // hand-closed copy of the stroke path.
        if self.glow.get() {
            chart.add_area(AreaSeries::new(cpu_pts.clone(), 0.0));
        }
        // Cpu outliers — samples beyond mean + 2σ of the visible
        // window — get scatter markers so spikes stay identifiable
        // once they scroll off the line's leading edge.
        let outlier_pts: Vec<CPoint> = if cpu_pts.is_empty() {
            Vec::new()
        } else {
            let n = cpu_pts.len() as f64;
            let mean = cpu_pts.iter().map(|p| p.y).sum::<f64>() / n;
            let stddev = (cpu_pts
                .iter()
                .map(|p| (p.y - mean) * (p.y - mean))
                .sum::<f64>()
                / n)
                .sqrt();
            let cutoff = mean + 2.0 * stddev;
            cpu_pts.iter().copied().filter(|p| p.y > cutoff).collect()
        };
        if !outlier_pts.is_empty() {
            chart.add_scatter(ScatterSeries::new(outlier_pts, 2.5 * sd));
        }
        chart.add_line(LineSeries::new(cpu_pts));
        chart.add_line(LineSeries::new(mem_pts));
        let auto = chart.bounds().unwrap_or(martensite::blessed::ChartBounds {
            x_min: 0.0,
            x_max: 1.0,
            y_min: 0.0,
            y_max: 100.0,
        });
        let bounds = martensite::blessed::ChartBounds {
            x_min: auto.x_min,
            x_max: auto.x_max.max(auto.x_min + 1.0),
            y_min: 0.0,
            y_max: 100.0,
        };

        // Horizontal gridlines at 0/25/50/75/100% with labels. Labels
        // thin out when the plot is squeezed (a docked panel can be
        // dragged to any height): keep a label only if its text band
        // clears the previous kept label's band — the gridlines still
        // all paint.
        let label_gap = 16.0 * sd;
        let mut last_label_y = f64::INFINITY;
        for frac in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let y = plot.y1 - frac * plot.height();
            cx.list.push_fill_rect(
                krect(plot.x0, y, plot.width(), 1.0),
                Palette::alpha(pal.border, if frac == 0.0 { 255 } else { 110 }),
            );
            if last_label_y - y < label_gap {
                continue;
            }
            last_label_y = y;
            text.push(
                cx.list,
                Point::new(inner.x0 + 2.0 * sd, y - 6.0 * sd),
                &format!("{:.0}", frac * 100.0),
                12.0 * s,
                pal.text_muted,
                Some(label_w as f32),
            );
        }

        // Area fills paint first — the stroke lines stay crisp on top.
        for area in chart.areas() {
            let pts = &area.points;
            if pts.len() < 2 {
                continue;
            }
            let mut path = BezPath::new();
            for (i, p) in pts.iter().enumerate() {
                // `Chart::project` already returns screen-space y
                // (data max → 0/top) — add the plot origin, don't re-flip.
                let q = Chart::project(*p, bounds, plot.width(), plot.height());
                let (qx, qy) = (plot.x0 + q.x, plot.y0 + q.y);
                if i == 0 {
                    path.move_to((qx, qy));
                } else {
                    path.line_to((qx, qy));
                }
            }
            // Close along the series' data-space baseline (0% → plot
            // bottom) — the model owns where the fill ends.
            let base_y = Chart::project(
                CPoint {
                    x: pts[0].x,
                    y: area.baseline,
                },
                bounds,
                plot.width(),
                plot.height(),
            )
            .y;
            let last = Chart::project(
                *pts.last().expect("nonempty"),
                bounds,
                plot.width(),
                plot.height(),
            );
            let first = Chart::project(pts[0], bounds, plot.width(), plot.height());
            path.line_to((plot.x0 + last.x, plot.y0 + base_y));
            path.line_to((plot.x0 + first.x, plot.y0 + base_y));
            path.close_path();
            cx.list.push_path(path, Palette::alpha(pal.accent, 56));
        }

        // Series → projected polylines.
        for (series_idx, color) in [(0usize, pal.accent), (1usize, pal.accent2)] {
            let pts = &chart.lines()[series_idx].points;
            if pts.len() < 2 {
                continue;
            }
            let mut path = BezPath::new();
            for (i, p) in pts.iter().enumerate() {
                // `Chart::project` already returns screen-space y
                // (data max → 0/top) — add the plot origin, don't re-flip.
                let q = Chart::project(*p, bounds, plot.width(), plot.height());
                let (qx, qy) = (plot.x0 + q.x, plot.y0 + q.y);
                if i == 0 {
                    path.move_to((qx, qy));
                } else {
                    path.line_to((qx, qy));
                }
            }
            cx.list.push_stroke_path(path, 1.6, color);
            // Latest-point dot.
            let q = Chart::project(
                *pts.last().expect("nonempty"),
                bounds,
                plot.width(),
                plot.height(),
            );
            let (qx, qy) = (plot.x0 + q.x, plot.y0 + q.y);
            cx.list
                .push_fill_rect(krect(qx - 2.5, qy - 2.5, 5.0, 5.0), color);
        }

        // Outlier markers last so they sit on top of both strokes —
        // `error` when the sample is also in alert territory, `warn`
        // otherwise. Radius is device px, matching the scale math.
        for scatter in chart.scatters() {
            let r = scatter.radius;
            for p in &scatter.points {
                let q = Chart::project(*p, bounds, plot.width(), plot.height());
                let (qx, qy) = (plot.x0 + q.x, plot.y0 + q.y);
                let color = if p.y >= Self::ALERT_PCT {
                    pal.error
                } else {
                    pal.warn
                };
                cx.list
                    .push_fill_rect(krect(qx - r, qy - r, 2.0 * r, 2.0 * r), color);
            }
        }

        // Legend.
        let legend_y = plot.y1 + 8.0 * sd;
        let mut lx = plot.x0;
        for (label, color) in [("cpu_load", pal.accent), ("mem_pressure", pal.accent2)] {
            cx.list
                .push_fill_rect(krect(lx, legend_y + 4.0 * sd, 12.0 * sd, 3.0 * sd), color);
            let w = text.measure(label, 12.0 * s);
            text.push(
                cx.list,
                Point::new(lx + 16.0 * sd, legend_y),
                label,
                12.0 * s,
                pal.text_muted,
                None,
            );
            lx += 16.0 * sd + f64::from(w) + 20.0 * sd;
        }
        // Outlier marker — a square matching the scatter glyph, shown
        // only while outliers are actually in the window. The chip
        // escalates to the error color when every visible outlier does.
        if !chart.scatters().is_empty() {
            let chip = if chart
                .scatters()
                .iter()
                .all(|s| s.points.iter().all(|p| p.y >= Self::ALERT_PCT))
            {
                pal.error
            } else {
                pal.warn
            };
            cx.list.push_fill_rect(
                krect(lx + 3.5 * sd, legend_y + 3.0 * sd, 5.0 * sd, 5.0 * sd),
                chip,
            );
            text.push(
                cx.list,
                Point::new(lx + 16.0 * sd, legend_y),
                "outliers",
                12.0 * s,
                pal.text_muted,
                None,
            );
        }
        panel_border(cx.list, self.bounds, pal, s, self.focused);
    }
}

// ---------------------------------------------------------------------------
// EditorPanel — tabbed CodeEditor documents over an embedded VFS
// ---------------------------------------------------------------------------

/// A restorable buffer state for undo — `CodeEditor` has no `set_text`
/// (F24), so restore rebuilds the editor and re-clamps the cursors.
/// Bespoke snapshots rather than `martensite-history`'s ledger: the
/// model exposes no diff/op API a ledger could record.
struct Snap {
    text: String,
    cursors: Vec<Cursor>,
}

/// One open document: the editor model plus its private undo/redo
/// stacks, scroll position, and the insert-run coalescing flag.
struct EditorTab {
    /// Display name — the VFS path for shipped documents, the file
    /// name for documents picked in the OS open dialog.
    name: String,
    /// The bytes `tab_dirty` compares the live buffer against — VFS
    /// bytes for shipped documents, file bytes for opened ones.
    baseline: Vec<u8>,
    editor: CodeEditor,
    undo: VecDeque<Snap>,
    redo: Vec<Snap>,
    scroll_top: usize,
    /// `true` while a run of consecutive `ImeCommitted` inserts is
    /// open — the run is a single undo step; any other event closes it.
    insert_run: bool,
}

/// Per-tab undo-history cap — bounds memory on the snapshot model.
const UNDO_CAP: usize = 100;

/// Drag-select granularity chosen by the initiating press's click
/// count — the same convention `TextInput` uses: single-click drags by
/// character, double by word, triple by line.
#[derive(Copy, Clone)]
enum DragGran {
    /// Extend one cursor position at a time.
    Char,
    /// Extend by whole words; the double-clicked word's span is kept
    /// so its far edge anchors whichever way the pointer moves.
    Word(Cursor, Cursor),
    /// Extend by whole lines from the triple-clicked line index.
    Line(usize),
}

/// Tab-strip action-chip labels — right-aligned, they publish the
/// request signals the app drains into OS file dialogs.
const OPEN_LABEL: &str = "open…";
const SAVE_LABEL: &str = "save as…";

/// Outcome signals shared between the editor panel and the app — the
/// panel owns the buffers but the app owns the OS services, so
/// requests and payloads travel through cells (the same seam the
/// toolbar and `clipboard_out` use).
#[derive(Clone)]
pub struct EditorSignals {
    /// Active tab index — seeded from the persisted preference at
    /// construction; the panel writes it back on every switch so the
    /// app can persist the last-active document.
    pub tab_sel: Signal<usize>,
    /// App → panel: a document picked in the OS open dialog, drained
    /// in `tick` into a new tab as `(name, contents)`.
    pub open_in: Signal<Option<(String, String)>>,
    /// Panel → app: the active tab's `(name, buffer)` — the save
    /// dialog's payload, republished on change in `tick`.
    pub doc_out: Signal<Option<(String, String)>>,
    /// Tab-strip "open…" chip → request an OS open-file dialog.
    pub open_req: Signal<bool>,
    /// Tab-strip "save as…" chip → request an OS save-file dialog.
    pub export_req: Signal<bool>,
}

/// Detached cells for tests — `Signal` has no `Default`, so tests and
/// headless harnesses get a freestanding set rather than wiring app
/// state.
impl Default for EditorSignals {
    fn default() -> Self {
        Self {
            tab_sel: Signal::new(0),
            open_in: Signal::new(None),
            doc_out: Signal::new(None),
            open_req: Signal::new(false),
            export_req: Signal::new(false),
        }
    }
}

/// The editor panel: one tab per `SOURCES` document resolved through
/// `EmbeddedVfs`, a chip strip under the title bar (dirty dot when the
/// buffer diverges from the VFS bytes, accent underline on the active
/// tab), click-to-place caret, `ImeCommitted` input, Backspace/Enter/
/// arrows editing, per-tab undo/redo on synthetic `Undo`/`Redo` keys,
/// and `highlight_line` spans mapped to palette colors per glyph
/// byte-offset.
pub struct EditorPanel {
    tabs: Vec<EditorTab>,
    active: usize,
    text: Mutex<TextPainter>,
    scale: Signal<f32>,
    bounds: Rect,
    focused: bool,
    /// Shift state for shift-click / shift-arrow selection extension —
    /// `KeyPressed` carries no modifier field (F17), tracked like
    /// `TextInput` does.
    shift_held: bool,
    /// In-progress drag-select; the panel holds pointer capture.
    drag: Option<DragGran>,
    /// Shared outcome cells — see [`EditorSignals`].
    tab_sel: Signal<usize>,
    open_in: Signal<Option<(String, String)>>,
    doc_out: Signal<Option<(String, String)>>,
    open_req: Signal<bool>,
    export_req: Signal<bool>,
    /// Set by any event that could mutate the buffer or the active
    /// tab — `tick` republishes `doc_out` only while dirty so an idle
    /// frame doesn't clone the whole document at 60 Hz.
    doc_dirty: bool,
}

impl EditorPanel {
    pub fn new(scale: Signal<f32>, signals: EditorSignals) -> Self {
        let vfs = EmbeddedVfs::new(SOURCES);
        // One tab per VFS document — buffers are seeded from
        // `vfs.resolve`, not the static table directly.
        let tabs = SOURCES
            .iter()
            .map(|&(path, _)| {
                let bytes = vfs.resolve(path).unwrap_or(b"");
                EditorTab {
                    name: path.to_string(),
                    baseline: bytes.to_vec(),
                    editor: CodeEditor::new(std::str::from_utf8(bytes).unwrap_or("")),
                    undo: VecDeque::new(),
                    redo: Vec::new(),
                    scroll_top: 0,
                    insert_run: false,
                }
            })
            .collect();
        // Restore the persisted active tab — the signal carries the
        // app's stored preference, clamped into the shipped set.
        let active = signals.tab_sel.get().min(SOURCES.len().saturating_sub(1));
        Self {
            tabs,
            active,
            text: Mutex::new(TextPainter::new()),
            scale,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            focused: false,
            shift_held: false,
            drag: None,
            tab_sel: signals.tab_sel,
            open_in: signals.open_in,
            doc_out: signals.doc_out,
            open_req: signals.open_req,
            export_req: signals.export_req,
            doc_dirty: true,
        }
    }

    fn s(&self) -> f32 {
        self.scale.get()
    }

    fn line_h(&self) -> f32 {
        12.0 * self.s() * 1.4
    }

    fn gutter_w(&self) -> f32 {
        46.0 * self.s()
    }

    fn tab(&self) -> &EditorTab {
        &self.tabs[self.active]
    }

    fn tab_mut(&mut self) -> &mut EditorTab {
        &mut self.tabs[self.active]
    }

    /// Maps a window-space point to the document cursor under it —
    /// the shared geometry for `PointerPressed` caret placement and
    /// drag-select extension. `content_top`/`line_h` come from the
    /// event handler so click geometry and paint geometry agree.
    fn cursor_at(
        &self,
        text: &mut TextPainter,
        position: Vec2,
        content_top: f32,
        line_h: f32,
    ) -> Cursor {
        let s = self.s();
        let line = (self.tab().scroll_top
            + ((position.y - content_top) / line_h).max(0.0) as usize)
            .min(self.tab().editor.lines().len().saturating_sub(1));
        let text_x = position.x - (self.bounds.min_x() + self.gutter_w() + 8.0 * s);
        let col = text.column_at(&self.tab().editor.lines()[line], 12.0 * s, text_x.max(0.0));
        Cursor::new(line, col)
    }

    /// Copies the open selection to the OS clipboard — the same
    /// `default_platform_clipboard` seam `TextInput` uses. Returns
    /// whether text was written.
    fn copy_selection(&self) -> bool {
        let Some(text) = self.tab().editor.selected_text() else {
            return false;
        };
        let mut cb = martensite::clipboard::default_platform_clipboard();
        cb.set_contents(&martensite::clipboard::ClipboardItem::new().offer_text(text));
        true
    }

    /// Inserts the OS clipboard's text payload at the caret, replacing
    /// any open selection (the model's `insert` consumes it). One undo
    /// step, its own run — pastes never coalesce into a typing run.
    fn paste_clipboard(&mut self, visible: usize) {
        let cb = martensite::clipboard::default_platform_clipboard();
        if let Some(bytes) = cb.get_contents(martensite::clipboard::clipboard::MIME_TEXT_PLAIN) {
            if let Ok(text) = String::from_utf8(bytes) {
                self.push_undo(false);
                self.tab_mut().editor.insert(&text);
                self.ensure_visible(visible);
            }
        }
    }

    fn kind_color(&self, pal: &Palette, kind: TokenKind) -> [u8; 4] {
        match kind {
            TokenKind::Keyword => pal.accent,
            TokenKind::String => pal.ok,
            TokenKind::Comment => pal.text_muted,
            TokenKind::Number => pal.warn,
            _ => pal.text,
        }
    }

    /// `true` when the tab's buffer diverges from its baseline bytes
    /// (VFS contents for shipped docs, file contents for opened ones)
    /// — the chip's dirty dot.
    fn tab_dirty(&self, index: usize) -> bool {
        let tab = &self.tabs[index];
        tab.editor.text().as_bytes() != tab.baseline.as_slice()
    }

    /// The chip label — name plus a dirty marker.
    fn chip_label(&self, index: usize) -> String {
        if self.tab_dirty(index) {
            format!("{} •", self.tabs[index].name)
        } else {
            self.tabs[index].name.clone()
        }
    }

    /// Chip (x, width) rects in window coordinates — the painter and
    /// `PointerPressed` hit-testing share this so clicks land on the
    /// painted chip. Takes the already-held `TextPainter` guard, same
    /// as `panel_chrome` (the painter is behind a non-reentrant
    /// `Mutex`, so locking inside would deadlock from `paint`).
    fn chip_rects(&self, text: &mut TextPainter) -> Vec<(f32, f32)> {
        let s = self.s();
        let mut x = self.bounds.min_x() + 6.0 * s;
        (0..self.tabs.len())
            .map(|i| {
                let w = text.measure(&self.chip_label(i), 12.0 * s) + 16.0 * s;
                let r = (x, w);
                x += w + 4.0 * s;
                r
            })
            .collect()
    }

    /// Right-aligned action-chip `(x, width)` rects in window
    /// coordinates — `[open, save as]`, the same shared-geometry
    /// contract as `chip_rects` (painter and hit-test share it).
    fn action_rects(&self, text: &mut TextPainter) -> [(f32, f32); 2] {
        let s = self.s();
        let w_open = text.measure(OPEN_LABEL, 12.0 * s) + 14.0 * s;
        let w_save = text.measure(SAVE_LABEL, 12.0 * s) + 14.0 * s;
        let right = self.bounds.max_x() - 6.0 * s;
        let save_x = right - w_save;
        let open_x = save_x - 4.0 * s - w_open;
        [(open_x, w_open), (save_x, w_save)]
    }

    /// The tab-strip band in window coordinates: (top, bottom) — under
    /// the title bar + hairline, above the code content.
    fn strip_band(&self) -> (f32, f32) {
        let s = self.s();
        let top = self.bounds.min_y() + TITLE_H * s + 1.0;
        (top, top + TAB_H * s + 1.0)
    }

    /// Ensures the caret's line is inside the scroll window.
    fn ensure_visible(&mut self, lines_visible: usize) {
        let tab = self.tab_mut();
        if let Some(cur) = tab.editor.cursors().first() {
            if cur.line < tab.scroll_top {
                tab.scroll_top = cur.line;
            } else if lines_visible > 0 && cur.line >= tab.scroll_top + lines_visible {
                tab.scroll_top = cur.line + 1 - lines_visible;
            }
        }
    }

    /// Snapshots the active tab's state onto its undo stack ahead of a
    /// mutation. `insert` marks an `ImeCommitted` run: consecutive
    /// inserts coalesce into one step (per-typing-run granularity, not
    /// per keystroke); anything else closes the run and pushes a fresh
    /// snapshot. Any new mutation clears redo.
    fn push_undo(&mut self, insert: bool) {
        let tab = self.tab_mut();
        if insert && tab.insert_run {
            return;
        }
        tab.undo.push_back(Snap {
            text: tab.editor.text(),
            cursors: tab.editor.cursors().to_vec(),
        });
        while tab.undo.len() > UNDO_CAP {
            tab.undo.pop_front();
        }
        tab.redo.clear();
        tab.insert_run = insert;
    }

    /// Pops the active tab's undo stack, pushes current state to redo,
    /// and restores the snapshot by rebuilding the editor (F24).
    fn undo(&mut self, visible: usize) {
        let tab = self.tab_mut();
        if let Some(snap) = tab.undo.pop_back() {
            tab.redo.push(Snap {
                text: tab.editor.text(),
                cursors: tab.editor.cursors().to_vec(),
            });
            tab.editor = CodeEditor::new(&snap.text);
            tab.editor.set_cursors(snap.cursors);
        }
        tab.insert_run = false;
        self.ensure_visible(visible);
    }

    /// `undo`'s mirror — pops redo, pushes current state to undo.
    fn redo(&mut self, visible: usize) {
        let tab = self.tab_mut();
        if let Some(snap) = tab.redo.pop() {
            tab.undo.push_back(Snap {
                text: tab.editor.text(),
                cursors: tab.editor.cursors().to_vec(),
            });
            tab.editor = CodeEditor::new(&snap.text);
            tab.editor.set_cursors(snap.cursors);
        }
        tab.insert_run = false;
        self.ensure_visible(visible);
    }
}

impl Widget for EditorPanel {
    fn debug_name(&self) -> &'static str {
        "Editor"
    }
    fn measure(&mut self, _cx: &mut LayoutContext, _c: LayoutConstraints) -> Vec2 {
        Vec2::new(400.0, 260.0)
    }

    fn min_render(&self) -> RenderMinimum {
        // The editor stays live when squeezed but clips its content to
        // the slot — no half-painted glyphs past the panel edge.
        RenderMinimum::new(Vec2::new(240.0, 140.0)).with_policy(UnderflowPolicy::Clip)
    }

    fn layout(&mut self, _cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
    }

    fn tick(&mut self, _dt: Duration) -> bool {
        let mut dirty = false;
        // A document the app picked in the OS open dialog lands here
        // as a new active tab — the file read and the dialog stay
        // app-side; the panel only receives the payload.
        if let Some((name, text)) = self.open_in.get() {
            self.open_in.set(None);
            self.tab_mut().insert_run = false;
            self.tabs.push(EditorTab {
                baseline: text.as_bytes().to_vec(),
                editor: CodeEditor::new(&text),
                name,
                undo: VecDeque::new(),
                redo: Vec::new(),
                scroll_top: 0,
                insert_run: false,
            });
            self.active = self.tabs.len() - 1;
            self.doc_dirty = true;
            dirty = true;
        }
        // Publish the active tab for the app's save dialog, and the
        // selection index for the persisted preference. `doc_out`
        // republishes only after an event that could have changed the
        // buffer — cloning a full document every idle frame is waste.
        if self.doc_dirty {
            self.doc_dirty = false;
            self.doc_out
                .set_if_changed(Some((self.tab().name.clone(), self.tab().editor.text())));
        }
        self.tab_sel.set_if_changed(self.active);
        dirty
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        let s = self.s();
        // Any event could mutate the buffer or switch tabs — flag the
        // next tick to republish `doc_out`.
        self.doc_dirty = true;
        // Matches the painter: title bar + hairline + tab strip +
        // hairline + content padding.
        let (strip_top, strip_bottom) = self.strip_band();
        let content_top = strip_bottom + 8.0 * s;
        let line_h = self.line_h();
        let visible = ((self.bounds.max_y() - content_top - 8.0 * s) / line_h).max(1.0) as usize;
        match cx.event {
            WidgetEvent::PointerPressed {
                position,
                button: PointerButton::Primary,
                count,
            } => {
                // Clicks in the title bar focus the panel but never move
                // the caret.
                if position.y < strip_top {
                    return EventResponse::CaptureFocus;
                }
                // Tab-strip clicks switch documents — chips are
                // hit-tested against the same rects the painter emits.
                if position.y < strip_bottom {
                    // Action chips win the strip's right edge — they
                    // publish request signals the app drains into OS
                    // file dialogs (the panel stays OS-free).
                    let acts = {
                        let mut t = self.text.lock();
                        self.action_rects(&mut t)
                    };
                    for (i, (ax, aw)) in acts.iter().enumerate() {
                        if position.x >= *ax && position.x < ax + aw {
                            if i == 0 {
                                self.open_req.set(true);
                            } else {
                                self.export_req.set(true);
                            }
                            return EventResponse::CaptureFocus;
                        }
                    }
                    // Scoped lock — the guard borrows `self.text` until
                    // its drop point, which would conflict with the
                    // `&mut self` uses below.
                    let chips = {
                        let mut t = self.text.lock();
                        self.chip_rects(&mut t)
                    };
                    // Same drop rule the painter uses — chips past
                    // `min(panel-right, action-chips-left)` aren't
                    // painted, so they mustn't be clickable either.
                    let tab_limit = acts[0].0 - 4.0 * s;
                    let limit = tab_limit.min(self.bounds.max_x());
                    if let Some(i) = chips
                        .iter()
                        .take_while(|(x, w)| x + w <= limit)
                        .position(|(x, w)| position.x >= *x && position.x < x + w)
                    {
                        if i != self.active {
                            // Switching documents ends any open insert
                            // run on the *departing* tab — the next
                            // edit there starts a fresh step.
                            self.tab_mut().insert_run = false;
                            self.active = i;
                        }
                    }
                    return EventResponse::CaptureFocus;
                }
                let cur = {
                    let mut t = self.text.lock();
                    self.cursor_at(&mut t, *position, content_top, line_h)
                };
                let shift = self.shift_held;
                let tab = self.tab_mut();
                // A caret move ends the insert run — the next typed
                // character opens a new undo step.
                tab.insert_run = false;
                match count {
                    1 => {
                        if shift {
                            // Shift-click extends from the anchor.
                            tab.editor.extend_selection_to(cur);
                        } else {
                            tab.editor.set_cursors(vec![cur]);
                        }
                        self.drag = Some(DragGran::Char);
                    }
                    2 => {
                        // Double-click selects the word under the
                        // pointer; its span is kept for word-drag.
                        let (lo, hi) = tab.editor.word_span_at(cur);
                        tab.editor.select_word_at(cur);
                        self.drag = Some(DragGran::Word(lo, hi));
                    }
                    _ => {
                        // Triple-click selects the whole line — the
                        // paragraph gesture for a line-based editor.
                        tab.editor.select_line(cur.line);
                        self.drag = Some(DragGran::Line(cur.line));
                    }
                }
                self.ensure_visible(visible);
                EventResponse::CapturePointer
            }
            WidgetEvent::PointerMoved { position } if self.drag.is_some() => {
                let cur = {
                    let mut t = self.text.lock();
                    self.cursor_at(&mut t, *position, content_top, line_h)
                };
                match self.drag {
                    Some(DragGran::Char) => {
                        self.tab_mut().editor.extend_selection_to(cur);
                    }
                    Some(DragGran::Word(ilo, ihi)) => {
                        // Word-drag: the initial word stays covered —
                        // anchor on its far edge relative to the drag.
                        let (wlo, whi) = self.tab().editor.word_span_at(cur);
                        let tab = self.tab_mut();
                        if cur >= ilo {
                            tab.editor.set_selection(ilo, whi);
                        } else {
                            tab.editor.set_selection(ihi, wlo);
                        }
                    }
                    Some(DragGran::Line(anchor_line)) => {
                        // Line-drag covers whole lines from the
                        // triple-clicked line to the pointer's line.
                        let ed = &mut self.tab_mut().editor;
                        let end_of = |ed: &CodeEditor, l: usize| {
                            if l + 1 < ed.lines().len() {
                                Cursor::new(l + 1, 0)
                            } else {
                                Cursor::new(l, ed.lines()[l].chars().count())
                            }
                        };
                        if cur.line >= anchor_line {
                            let end = end_of(ed, cur.line);
                            ed.set_selection(Cursor::new(anchor_line, 0), end);
                        } else {
                            let end = end_of(ed, anchor_line);
                            ed.set_selection(Cursor::new(cur.line, 0), end);
                        }
                    }
                    None => {}
                }
                self.ensure_visible(visible);
                EventResponse::RequestRepaint
            }
            WidgetEvent::PointerReleased { .. } if self.drag.is_some() => {
                self.drag = None;
                EventResponse::ReleasePointer
            }
            WidgetEvent::ImeCommitted { text } => {
                // Printable input only — control chars (Enter/Tab) ride
                // KeyPressed below.
                let printable: String = text.chars().filter(|c| !c.is_control()).collect();
                if !printable.is_empty() {
                    self.push_undo(true);
                    self.tab_mut().editor.insert(&printable);
                    self.ensure_visible(visible);
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Handled
            }
            WidgetEvent::KeyPressed { key, .. } => {
                match key.as_str() {
                    "Shift" => self.shift_held = true,
                    "SelectAll" => self.tab_mut().editor.select_all(),
                    "Copy" => {
                        self.copy_selection();
                    }
                    "Cut" => {
                        if self.copy_selection() {
                            self.push_undo(false);
                            self.tab_mut().editor.delete_selection();
                        }
                    }
                    "Paste" => self.paste_clipboard(visible),
                    "Escape" => {
                        // Collapse an open selection onto its head.
                        if let Some(cur) = self.tab().editor.cursors().first().copied() {
                            self.tab_mut().editor.set_cursors(vec![cur]);
                        }
                    }
                    "Backspace" => {
                        // A no-op delete (every cursor at 0:0) must not
                        // push a snapshot — and above all must not
                        // clear the redo stack. An open selection is
                        // always deletable.
                        let can_delete = self.tab().editor.selection().is_some()
                            || self
                                .tab()
                                .editor
                                .cursors()
                                .iter()
                                .any(|c| c.line > 0 || c.column > 0);
                        if can_delete {
                            self.push_undo(false);
                            self.tab_mut().editor.delete_backward();
                        }
                    }
                    "Enter" => {
                        self.push_undo(false);
                        self.tab_mut().editor.insert("\n");
                    }
                    // Synthetic names dispatched by the app's Cmd/Ctrl
                    // chord handling — KeyPressed has no modifiers
                    // field (F17).
                    "Undo" => self.undo(visible),
                    "Redo" => self.redo(visible),
                    "ArrowLeft" | "ArrowRight" | "ArrowUp" | "ArrowDown" | "Home" | "End" => {
                        self.tab_mut().insert_run = false;
                        if let Some(cur) = self.tab().editor.cursors().first().copied() {
                            let lines = self.tab().editor.lines();
                            let target = match key.as_str() {
                                "ArrowLeft" => Cursor::new(cur.line, cur.column.saturating_sub(1)),
                                "ArrowRight" => Cursor::new(
                                    cur.line,
                                    (cur.column + 1).min(lines[cur.line].chars().count()),
                                ),
                                "ArrowUp" => Cursor::new(
                                    cur.line.saturating_sub(1),
                                    cur.column
                                        .min(lines[cur.line.saturating_sub(1)].chars().count()),
                                ),
                                "ArrowDown" => Cursor::new(
                                    (cur.line + 1).min(lines.len().saturating_sub(1)),
                                    cur.column.min(
                                        lines[(cur.line + 1).min(lines.len().saturating_sub(1))]
                                            .chars()
                                            .count(),
                                    ),
                                ),
                                "Home" => Cursor::new(cur.line, 0),
                                _ => Cursor::new(cur.line, lines[cur.line].chars().count()),
                            };
                            let extend = self.shift_held;
                            let tab = self.tab_mut();
                            if extend {
                                // Shift+arrow extends the selection.
                                tab.editor.extend_selection_to(target);
                            } else if let Some((lo, hi)) = tab.editor.selection() {
                                // Plain Left/Right collapse to the
                                // selection edge instead of stepping —
                                // the platform convention.
                                tab.editor.set_cursors(vec![match key.as_str() {
                                    "ArrowLeft" => lo,
                                    "ArrowRight" => hi,
                                    _ => target,
                                }]);
                            } else {
                                tab.editor.set_cursors(vec![target]);
                            }
                        }
                    }
                    _ => {}
                }
                self.ensure_visible(visible);
                EventResponse::Handled
            }
            WidgetEvent::KeyReleased { key } if key == "Shift" => {
                self.shift_held = false;
                EventResponse::Handled
            }
            WidgetEvent::FocusGained => {
                self.focused = true;
                EventResponse::RequestRepaint
            }
            WidgetEvent::FocusLost => {
                self.focused = false;
                // A focus round-trip ends the insert run — typing after
                // clicking back in is a new undo step. Shift and any
                // in-progress drag must not leak past the transition.
                self.tab_mut().insert_run = false;
                self.shift_held = false;
                self.drag = None;
                EventResponse::RequestRepaint
            }
            // AT focus/activation requests honour the pending-focus
            // protocol: CaptureFocus records a request the app drains
            // into the FocusManager.
            WidgetEvent::SemanticAction(SemanticAction::Focus | SemanticAction::Click) => {
                EventResponse::CaptureFocus
            }
            _ => EventResponse::Ignored,
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::MultilineTextInput);
        node.set_label(format!("Editor — {}", self.tab().name));
        node.set_value(self.tab().editor.text());
    }

    fn paint(&self, cx: &mut PaintContext) {
        let pal = Palette::from_theme(cx.theme);
        let pal = &pal;
        let s = self.s();
        let mut text = self.text.lock();
        let tab = self.tab();
        let full = format!(
            "{} lines · cursor {}:{} · ↶{}",
            tab.editor.lines().len(),
            tab.editor
                .cursors()
                .first()
                .map(|c| c.line + 1)
                .unwrap_or(0),
            tab.editor.cursors().first().map(|c| c.column).unwrap_or(0),
            tab.undo.len()
        );
        let slot = (self.bounds.width() * 0.42).max(1.0);
        let right = if text.measure(&full, 12.0 * s) <= slot {
            full
        } else {
            format!("{} lines", tab.editor.lines().len())
        };
        let title = format!("EDITOR · {}", tab.name);
        let inner = panel_chrome(&mut text, cx.list, self.bounds, &title, &right, pal, s);
        let sd = f64::from(s);
        let pad = 8.0 * sd;
        let gutter_w = f64::from(self.gutter_w());
        let line_h = f64::from(self.line_h());
        let font = 12.0 * s;
        cx.list
            .push_clip(krect(inner.x0, inner.y0, inner.width(), inner.height()));

        // Tab strip — the second header row: raised band, one chip per
        // VFS path (dirty `•` when the buffer diverges from the VFS
        // bytes), accent underline on the active chip.
        let strip_h = f64::from(TAB_H) * sd;
        let strip_top = inner.y0;
        cx.list.push_fill_rect(
            krect(inner.x0, strip_top, inner.width(), strip_h),
            pal.raised,
        );
        // `chip_rects` is the single geometry source — PointerPressed
        // hit-tests the same rects painted here. Chips that can't fit
        // are dropped rather than painted past the panel clip — same
        // convention as the grid's rightmost-first column drop. The
        // action chips own the strip's right edge, so tab chips stop
        // short of it.
        let acts = self.action_rects(&mut text);
        let tab_limit = f64::from(acts[0].0) - 4.0 * sd;
        for (i, (chip_x, chip_w)) in self.chip_rects(&mut text).iter().enumerate() {
            let chip_x = f64::from(*chip_x);
            let chip_w = f64::from(*chip_w);
            if chip_x + chip_w > inner.x1.min(tab_limit) {
                break;
            }
            let label = self.chip_label(i);
            if i == self.active {
                // Active tab rounds its top edge — it visually meets the
                // raised strip at the bottom.
                cx.list.push_fill_shape(
                    krect(chip_x, strip_top, chip_w, strip_h),
                    &Shape::corners(CornerRadii::top((4.0 * sd) as f32), CornerStyle::Round),
                    pal.surface,
                );
                cx.list.push_fill_rect(
                    krect(
                        chip_x + 4.0 * sd,
                        strip_top + strip_h - 2.0 * sd,
                        chip_w - 8.0 * sd,
                        2.0 * sd,
                    ),
                    pal.accent,
                );
            }
            text.push(
                cx.list,
                Point::new(chip_x + 8.0 * sd, strip_top + 5.0 * sd),
                &label,
                12.0 * s,
                // `text_muted` on the raised strip resolves to 4.20:1 —
                // under the 4.5:1 AA floor. Active/inactive reads via
                // the surface fill + accent underline instead.
                pal.text,
                None,
            );
        }
        // Action chips — right edge of the strip; pressing them
        // publishes the request signals the app drains into OS file
        // dialogs (the panel itself stays OS-free).
        for (i, label) in [OPEN_LABEL, SAVE_LABEL].iter().enumerate() {
            let (ax, aw) = (f64::from(acts[i].0), f64::from(acts[i].1));
            if ax < inner.x0 || ax + aw > inner.x1 {
                continue;
            }
            let r = krect(ax, strip_top + 3.0 * sd, aw, strip_h - 6.0 * sd);
            cx.list
                .push_stroke_shape(r, &Shape::rounded(3.0 * s), 1.0, pal.border);
            text.push(
                cx.list,
                Point::new(ax + 7.0 * sd, strip_top + 5.0 * sd),
                label,
                12.0 * s,
                pal.text_muted,
                None,
            );
        }
        cx.list.push_fill_rect(
            krect(inner.x0, strip_top + strip_h, inner.width(), 1.0),
            pal.border,
        );

        let content_top = strip_top + strip_h + 1.0 + pad;
        cx.list.push_fill_rect(
            krect(
                inner.x0,
                content_top - pad,
                gutter_w,
                inner.y1 - content_top + pad,
            ),
            Palette::alpha(pal.raised, 90),
        );

        let caret = tab.editor.cursors().first().copied();
        let sel = tab.editor.selection();
        let mut y = content_top;
        for (li, line) in tab.editor.lines().iter().enumerate().skip(tab.scroll_top) {
            if y + line_h > inner.y1 {
                break;
            }
            // Line number.
            let num = format!("{}", li + 1);
            let nw = text.measure(&num, 12.0 * s);
            text.push(
                cx.list,
                Point::new(inner.x0 + gutter_w - 8.0 * sd - f64::from(nw), y + 1.5 * sd),
                &num,
                12.0 * s,
                if caret.is_some_and(|c| c.line == li) {
                    pal.text
                } else {
                    pal.text_muted
                },
                Some(gutter_w as f32),
            );
            // Highlighted text — spans carry byte offsets, glyphs carry
            // byte offsets; SpanColor maps one onto the other.
            let spans: Vec<SpanColor> = tab
                .editor
                .highlight_line(li)
                .into_iter()
                .map(|sp| SpanColor {
                    start: sp.start,
                    end: sp.end,
                    color: self.kind_color(pal, sp.kind),
                })
                .collect();
            // Selection band behind the text — measured with the same
            // painter and font so the highlight aligns with glyphs.
            if let Some((sa, sb)) = sel {
                if li >= sa.line && li <= sb.line {
                    let len = line.chars().count();
                    let c0 = if li == sa.line { sa.column.min(len) } else { 0 };
                    let c1 = if li == sb.line {
                        sb.column.min(len)
                    } else {
                        len
                    };
                    if c0 < c1 || li < sb.line {
                        let p0: String = line.chars().take(c0).collect();
                        let p1: String = line.chars().take(c1).collect();
                        let x0 =
                            inner.x0 + gutter_w + 8.0 * sd + f64::from(text.measure(&p0, font));
                        let mut x1 =
                            inner.x0 + gutter_w + 8.0 * sd + f64::from(text.measure(&p1, font));
                        // The selection continues past EOL (newline
                        // selected) — the band extends a sliver, the
                        // platform convention.
                        if li < sb.line {
                            x1 = x1.max(x0) + 4.0 * sd;
                        }
                        cx.list
                            .push_fill_rect(krect(x0, y, x1 - x0, line_h), pal.selection_tint());
                    }
                }
            }
            // No wrap — code lines clip at the panel edge like a real
            // editor (horizontal scroll is out of scope for the demo).
            text.push_colored(
                cx.list,
                Point::new(inner.x0 + gutter_w + 8.0 * sd, y),
                line,
                font,
                pal.text,
                &spans,
                None,
            );
            // Caret.
            if self.focused && caret.is_some_and(|c| c.line == li) {
                let col = caret.expect("checked").column;
                let prefix: String = line.chars().take(col).collect();
                let caret_x =
                    inner.x0 + gutter_w + 8.0 * sd + f64::from(text.measure(&prefix, font));
                cx.list
                    .push_fill_rect(krect(caret_x, y, 2.0, line_h.min(16.0 * sd)), pal.accent);
            }
            y += line_h;
        }
        cx.list.pop_clip();
        panel_border(cx.list, self.bounds, pal, s, self.focused);
    }
}

// ---------------------------------------------------------------------------
// MediaPanel — MediaView over the platform decoder (or honest fallback)
// ---------------------------------------------------------------------------

/// Media surface panel: a real `MediaView` widget (Contain fit) decoding
/// the checked-in 320×240 H.264 fixture through the platform hardware
/// decoder (VideoToolbox / Media Foundation / VAAPI, per target). When
/// no backend is available the mock-surface fallback keeps the honest
/// "no decoder" overlay — never faked frames.
pub struct MediaPanel {
    view: MediaView,
    text: Mutex<TextPainter>,
    scale: Signal<f32>,
    bounds: Rect,
    focused: bool,
    elapsed_nanos: u64,
    /// Packetized fixture stream; `None` only when it fails to parse.
    clip: Option<crate::media_stream::ClipStream>,
    /// Next access-unit index to feed this pass.
    feed_idx: usize,
    /// Pacing budget — access units are fed at the stream's 30fps
    /// cadence so the decoder's bounded output queue never drops.
    feed_budget_ns: u64,
    /// Running output frame index — keeps `pts` monotonic across loops.
    frame_index: u64,
    /// `end_of_stream` signalled for the current pass.
    eos_sent: bool,
    /// A persistent `send_packet` failure disables feeding (the overlay
    /// shows the stalled queue honestly rather than spinning).
    feed_failed: bool,
}

impl MediaPanel {
    pub fn new(scale: Signal<f32>) -> Self {
        let clip = crate::media_stream::ClipStream::load();
        let mut view = MediaView::new().with_fit(VideoFit::Contain);
        if let Some(clip) = clip.as_ref() {
            if let Some(dec) = crate::media_stream::platform_decoder(clip) {
                view.set_decoder(dec);
            }
        }
        if view.decoder().is_none() {
            view = view.with_surface(VideoSurface::new_mock(1920, 1080, VideoPixelFormat::Nv12));
        }
        Self {
            view,
            text: Mutex::new(TextPainter::new()),
            scale,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            focused: false,
            elapsed_nanos: 0,
            clip,
            feed_idx: 0,
            feed_budget_ns: 0,
            frame_index: 0,
            eos_sent: false,
            feed_failed: false,
        }
    }

    fn s(&self) -> f32 {
        self.scale.get()
    }

    /// Feeds the next access units due under the 30fps pacing budget.
    fn feed_decoder(&mut self, dt_ns: u64) {
        let Some(clip) = self.clip.as_ref() else {
            return;
        };
        if self.view.decoder().is_none() || self.feed_failed {
            return;
        }
        self.feed_budget_ns = self.feed_budget_ns.saturating_add(dt_ns);
        while self.feed_idx < clip.len() && self.feed_budget_ns >= crate::media_stream::FRAME_NS {
            let pts = self.frame_index * crate::media_stream::FRAME_NS;
            if self
                .view
                .feed_packet(&clip.packet(self.feed_idx, pts))
                .is_err()
            {
                self.feed_failed = true;
                return;
            }
            self.feed_idx += 1;
            self.frame_index += 1;
            self.feed_budget_ns -= crate::media_stream::FRAME_NS;
        }
        if self.feed_idx == clip.len() && !self.eos_sent {
            let _ = self.view.end_of_stream();
            self.eos_sent = true;
        }
        if self.eos_sent && self.view.queued_frames() == 0 {
            // Pass drained — re-arm the keyframe gate and loop the clip.
            if let Some(dec) = self.view.decoder_mut() {
                let _ = dec.flush();
            }
            self.feed_idx = 0;
            self.eos_sent = false;
        }
    }
}

impl Widget for MediaPanel {
    fn debug_name(&self) -> &'static str {
        "Media"
    }
    fn measure(&mut self, _cx: &mut LayoutContext, _c: LayoutConstraints) -> Vec2 {
        Vec2::new(320.0, 220.0)
    }

    fn min_render(&self) -> RenderMinimum {
        // The dock's manual layout cannot honor `display:none`
        // semantics, so Collapse degrades to hide-like behavior here —
        // the slot is retained and the panel simply stops painting.
        RenderMinimum::new(Vec2::new(220.0, 130.0)).with_policy(UnderflowPolicy::Collapse)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        // MediaView owns its dest-rect math (fit + letterbox). Content
        // starts below the title bar + hairline (TITLE_H·s + 1).
        let inner = Rect::new(
            bounds.origin.x,
            bounds.origin.y + TITLE_H * self.s() + 1.0,
            bounds.size.x,
            (bounds.size.y - TITLE_H * self.s() - 1.0).max(0.0),
        );
        self.view.layout(cx, inner);
    }

    fn tick(&mut self, dt: Duration) -> bool {
        self.elapsed_nanos += dt.as_nanos() as u64;
        self.feed_decoder(dt.as_nanos() as u64);
        self.view.advance(self.elapsed_nanos)
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                ..
            } => EventResponse::CaptureFocus,
            WidgetEvent::FocusGained => {
                self.focused = true;
                EventResponse::RequestRepaint
            }
            WidgetEvent::FocusLost => {
                self.focused = false;
                EventResponse::RequestRepaint
            }
            // AT focus/activation requests honour the pending-focus
            // protocol: CaptureFocus records a request the app drains
            // into the FocusManager.
            WidgetEvent::SemanticAction(SemanticAction::Focus | SemanticAction::Click) => {
                EventResponse::CaptureFocus
            }
            _ => EventResponse::Ignored,
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        self.view.accessibility(node);
        let label = match self.view.decoder().and_then(|d| d.stats().backend) {
            Some(backend) => {
                format!("Media surface — H.264 320x240 NV12, {backend:?} decoder")
            }
            None => "Media surface — 1920x1080 NV12 mock, no decoder attached".to_string(),
        };
        node.set_label(&label);
    }

    fn paint(&self, cx: &mut PaintContext) {
        let pal = Palette::from_theme(cx.theme);
        let pal = &pal;
        let s = self.s();
        let mut text = self.text.lock();
        // Compact status — tiered to the actual panel width so the
        // title bar never shows a mid-word ellipsis.
        let right = if self.bounds.width() >= 170.0 * s {
            format!("drop {:.1}%", self.view.drop_rate_pct())
        } else if self.bounds.width() >= 120.0 * s {
            format!("{:.1}%", self.view.drop_rate_pct())
        } else {
            String::new()
        };
        let inner = panel_chrome(&mut text, cx.list, self.bounds, "MEDIA", &right, pal, s);
        // Delegate to the real widget for the letterboxed backdrop.
        self.view.paint(&mut PaintContext {
            list: cx.list,
            bounds: Rect::new(
                inner.x0 as f32,
                inner.y0 as f32,
                inner.width() as f32,
                inner.height() as f32,
            ),
            theme: cx.theme,
            scale: cx.scale,
            text_painter: cx.text_painter,
        });
        let sd = f64::from(s);
        let cx0 = inner.x0 + inner.width() / 2.0;
        let cy = inner.y0 + inner.height() / 2.0;
        // Width-adaptive overlay — full strings when the panel has room,
        // compact variants when narrow (still honest, never fake frames).
        let roomy = inner.width() >= 300.0 * sd;
        let (l1, l2) = match self.view.decoder() {
            Some(dec) => {
                let stats = dec.stats();
                // Once real frames are flowing the decoded video is the
                // content — the center overlay only shows during warm-up.
                if stats.frames_decoded > 0 {
                    (String::new(), String::new())
                } else {
                    let backend = stats
                        .backend
                        .map_or("decoder".to_string(), |b| format!("{b:?}"));
                    let fmt = format!("{:?}", dec.negotiated_format());
                    if roomy {
                        (
                            format!("H.264 320×240 → {fmt} · {backend}"),
                            "awaiting frames".to_string(),
                        )
                    } else {
                        (format!("{fmt} · {backend}"), String::new())
                    }
                }
            }
            None => {
                if roomy {
                    (
                        "NV12 1920×1080 — mock surface".to_string(),
                        "no decoder on this platform".to_string(),
                    )
                } else {
                    ("NV12 · 1080p".to_string(), "no decoder".to_string())
                }
            }
        };
        // The letterbox is black in every theme — overlay text stays
        // light regardless of the palette (on-video convention).
        const VIDEO_INK: [u8; 4] = [226, 232, 240, 255];
        const VIDEO_MUTED: [u8; 4] = [160, 170, 185, 255];
        for (i, (line, color)) in [(l1.as_str(), VIDEO_INK), (l2.as_str(), VIDEO_MUTED)]
            .iter()
            .enumerate()
        {
            let line_fit = text.fit(line, 12.0 * s, (inner.width() - 20.0 * sd).max(1.0) as f32);
            let w = text.measure(&line_fit, 12.0 * s);
            text.push(
                cx.list,
                Point::new(
                    cx0 - f64::from(w) / 2.0,
                    cy - 12.0 * sd + i as f64 * 18.0 * sd,
                ),
                &line_fit,
                12.0 * s,
                *color,
                None,
            );
        }
        panel_border(cx.list, self.bounds, pal, s, self.focused);
    }
}

/// Formats a byte count in KiB as a compact `X.X MiB`/`X.X GiB` string.
fn fmt_mem(kib: u32) -> String {
    if kib >= 1_048_576 {
        format!("{:.1}G", f64::from(kib) / 1_048_576.0)
    } else if kib >= 1024 {
        format!("{:.0}M", f64::from(kib) / 1024.0)
    } else {
        format!("{kib}K")
    }
}

/// `1234567` → `"1,234,567"`.
pub fn fmt_count(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite::core::HotNode;

    fn grid() -> GridPanel {
        GridPanel::new(
            Signal::new(1.0),
            Signal::new(String::new()),
            Signal::new(None),
        )
    }

    fn layout_at(w: &mut impl Widget, bounds: Rect) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        w.layout(&mut cx, bounds);
    }

    fn send(w: &mut impl Widget, bounds: Rect, ev: &WidgetEvent) -> EventResponse {
        let mut cx = EventContext {
            event: ev,
            bounds,
            scale: 1.0,
        };
        w.event(&mut cx)
    }

    /// The paint/hit-test invariant: while the band stretches past the
    /// boundary the table keeps the clamped offset and `render_offset`
    /// carries the stretch — and the two converge once the spring
    /// lands.
    #[test]
    fn rubber_band_stretches_while_table_stays_clamped() {
        let mut p = grid();
        let bounds = Rect::new(0.0, 0.0, 600.0, 300.0);
        layout_at(&mut p, bounds);
        // A wheel fling far past the bottom (negative delta.y scrolls
        // down — `band.drag(-delta.y)` grows the offset).
        send(
            &mut p,
            bounds,
            &WidgetEvent::Scroll {
                position: Vec2::new(10.0, 200.0),
                delta: Vec2::new(0.0, -40_000_000.0),
            },
        );
        assert!(!p.band.is_settled());
        // The visible offset carries the stretch past the table's
        // clamped offset — paint and hit-testing agree on it.
        assert!(p.render_offset() > p.table.scroll_offset());
        // The spring lands back on the boundary and the offsets
        // converge again.
        for _ in 0..180 {
            p.tick(Duration::from_millis(16));
        }
        assert!(p.band.is_settled());
        assert_eq!(p.render_offset(), p.table.scroll_offset());
    }

    /// A press inside a divider's grab zone starts a resize drag — it
    /// must NOT toggle the column's sort — and a double-click on the
    /// divider resets the column to its shipped width.
    #[test]
    fn column_divider_drag_resizes_and_double_click_resets() {
        let mut p = grid();
        let bounds = Rect::new(0.0, 0.0, 600.0, 300.0);
        layout_at(&mut p, bounds);

        // First divider = PID's right edge (12pt pad + 96pt column).
        let (dx, col) = p.divider_edges()[0];
        assert_eq!(col, 0);
        let (hdr_top, hdr_bottom) = p.header_band();
        let y = (hdr_top + hdr_bottom) * 0.5;
        let sort_before = p.table.sort_column();

        let r = send(
            &mut p,
            bounds,
            &WidgetEvent::PointerPressed {
                position: Vec2::new(dx, y),
                button: PointerButton::Primary,
                count: 1,
            },
        );
        assert_eq!(r, EventResponse::CapturePointer);
        // A divider press is a resize grab, not a sort toggle.
        assert_eq!(p.table.sort_column(), sort_before);

        // Dragging +40px grows the column 96 → 136 logical pt.
        send(
            &mut p,
            bounds,
            &WidgetEvent::PointerMoved {
                position: Vec2::new(dx + 40.0, y),
            },
        );
        assert_eq!(p.table.columns()[0].width(), 136.0);

        // Release frees the capture and ends the drag.
        let r = send(
            &mut p,
            bounds,
            &WidgetEvent::PointerReleased {
                position: Vec2::new(dx + 40.0, y),
                button: PointerButton::Primary,
            },
        );
        assert_eq!(r, EventResponse::ReleasePointer);
        assert!(p.resizing.is_none());

        // Double-click on the (moved) divider resets to 96pt.
        send(
            &mut p,
            bounds,
            &WidgetEvent::PointerPressed {
                position: Vec2::new(dx + 40.0, y),
                button: PointerButton::Primary,
                count: 2,
            },
        );
        assert_eq!(p.table.columns()[0].width(), 96.0);
        assert!(p.resizing.is_none());
    }

    /// The drag clamps at the floor and at the panel's inner edge —
    /// past the edge the column would collapse out of the layout and
    /// the drag would lose its anchor.
    #[test]
    fn column_resize_clamps() {
        let mut p = grid();
        let bounds = Rect::new(0.0, 0.0, 600.0, 300.0);
        layout_at(&mut p, bounds);
        let (dx, _) = p.divider_edges()[0];
        let (hdr_top, hdr_bottom) = p.header_band();
        let y = (hdr_top + hdr_bottom) * 0.5;
        send(
            &mut p,
            bounds,
            &WidgetEvent::PointerPressed {
                position: Vec2::new(dx, y),
                button: PointerButton::Primary,
                count: 1,
            },
        );
        // Far left → floor; far right → panel edge.
        send(
            &mut p,
            bounds,
            &WidgetEvent::PointerMoved {
                position: Vec2::new(0.0, y),
            },
        );
        assert_eq!(p.table.columns()[0].width(), GRID_MIN_COL_W);
        send(
            &mut p,
            bounds,
            &WidgetEvent::PointerMoved {
                position: Vec2::new(10_000.0, y),
            },
        );
        let max_w = bounds.max_x() - 12.0 - p.column_layout()[0].0;
        assert_eq!(p.table.columns()[0].width(), max_w);
    }

    /// Overlay entries never receive `tick` — the menu's entrance
    /// spring is advanced by the owning `GridPanel::tick` and cleared
    /// once it settles.
    #[test]
    fn menu_entrance_advances_via_owner_tick() {
        let mut p = grid();
        layout_at(&mut p, Rect::new(0.0, 0.0, 600.0, 300.0));
        // Arm the menu as a secondary press does.
        p.menu_open = true;
        let mut overlay = OverlayLayer::new();
        overlay.set_viewport(Rect::new(0.0, 0.0, 800.0, 600.0));
        p.sync_overlay(&mut overlay);
        assert!(p.menu_id.is_some());
        assert!(p.menu_shared.lock().entrance.is_some());
        // ~180ms settle → well inside 60 16ms frames.
        for _ in 0..60 {
            p.tick(Duration::from_millis(16));
        }
        assert!(p.menu_shared.lock().entrance.is_none());
    }

    /// Undo stacks are per-tab: undoing in one tab must not consume
    /// the other tab's history.
    #[test]
    fn undo_history_is_per_tab() {
        let mut p = EditorPanel::new(Signal::new(1.0), EditorSignals::default());
        let bounds = Rect::new(0.0, 0.0, 800.0, 400.0);
        layout_at(&mut p, bounds);

        let type_text = |p: &mut EditorPanel, text: &str| {
            send(
                p,
                bounds,
                &WidgetEvent::ImeCommitted {
                    text: text.to_string(),
                },
            );
        };
        let click_chip = |p: &mut EditorPanel, i: usize| {
            let (top, bottom) = p.strip_band();
            let chips = {
                let mut t = p.text.lock();
                p.chip_rects(&mut t)
            };
            let x = chips[i].0 + chips[i].1 * 0.5;
            send(
                p,
                bounds,
                &WidgetEvent::PointerPressed {
                    position: Vec2::new(x, (top + bottom) * 0.5),
                    button: PointerButton::Primary,
                    count: 1,
                },
            );
        };
        let undo = |p: &mut EditorPanel| {
            send(
                p,
                bounds,
                &WidgetEvent::KeyPressed {
                    key: "Undo".to_string(),
                    repeat: false,
                },
            );
        };

        type_text(&mut p, "X");
        assert!(p.tab_dirty(0));
        click_chip(&mut p, 1);
        type_text(&mut p, "Y");
        assert!(p.tab_dirty(1));

        // Undo on tab 1 restores only that buffer — tab 0's edit is
        // untouched by a stack it doesn't own.
        undo(&mut p);
        assert!(!p.tab_dirty(1));
        assert!(p.tab_dirty(0));

        click_chip(&mut p, 0);
        undo(&mut p);
        assert!(!p.tab_dirty(0));
    }

    /// Double-click selects the word under the pointer, triple-click
    /// selects the line, and a word-drag keeps the initial word
    /// covered on either side — the platform-standard multi-click
    /// contract the framework's `count` field exists for.
    #[test]
    fn editor_multi_click_selects_word_and_line() {
        let mut p = EditorPanel::new(Signal::new(1.0), EditorSignals::default());
        let bounds = Rect::new(0.0, 0.0, 800.0, 400.0);
        layout_at(&mut p, bounds);
        let (_, strip_bottom) = p.strip_band();
        let content_top = strip_bottom + 8.0;
        let line_h = p.line_h();
        // Window-space x of a column on a line — the same painter the
        // panel hit-tests with.
        let col_x = |p: &mut EditorPanel, line: usize, col: usize| {
            let mut t = p.text.lock();
            let prefix: String = p.tab().editor.lines()[line].chars().take(col).collect();
            bounds.min_x() + p.gutter_w() + 8.0 + t.measure(&prefix, 12.0)
        };
        let press = |p: &mut EditorPanel, x: f32, y: f32, count: u8| {
            send(
                p,
                bounds,
                &WidgetEvent::PointerPressed {
                    position: Vec2::new(x, y),
                    button: PointerButton::Primary,
                    count,
                },
            )
        };

        // Line 2 is `title = "Industrial Workstation"` — double-click
        // inside "title".
        let x = col_x(&mut p, 2, 3);
        let y = content_top + line_h * 2.0 + 2.0;
        assert_eq!(press(&mut p, x, y, 2), EventResponse::CapturePointer);
        assert_eq!(p.tab().editor.selected_text().as_deref(), Some("title"));

        // Triple-click on line 1 (`[window]`) selects the line plus
        // its newline — paragraph semantics.
        let y1 = content_top + line_h + 2.0;
        press(&mut p, x, y1, 3);
        assert_eq!(
            p.tab().editor.selected_text().as_deref(),
            Some("[window]\n")
        );

        // Double-click "title" again, then drag into "Workstation" —
        // the selection snaps to word boundaries and keeps "title".
        press(&mut p, x, y, 2);
        let xw = col_x(&mut p, 2, 20); // inside "Workstation"
        send(
            &mut p,
            bounds,
            &WidgetEvent::PointerMoved {
                position: Vec2::new(xw, y),
            },
        );
        assert_eq!(
            p.tab().editor.selected_text().as_deref(),
            // "Workstation" is a word run; the closing quote is a
            // punctuation span of its own — not part of the selection.
            Some("title = \"Industrial Workstation"),
        );
        send(
            &mut p,
            bounds,
            &WidgetEvent::PointerReleased {
                position: Vec2::new(xw, y),
                button: PointerButton::Primary,
            },
        );
        assert!(p.drag.is_none());
    }
}

#[cfg(test)]
mod decoder_tests {
    use super::*;
    use std::time::Instant;

    /// macOS: `MediaPanel` must attach the real VideoToolbox decoder and
    /// produce NV12 frames from the checked-in H.264 fixture — never the
    /// "no decoder" fallback. Skips gracefully where no backend exists.
    #[test]
    fn media_panel_decodes_fixture_through_platform_decoder() {
        let mut panel = MediaPanel::new(Signal::new(1.0));
        if panel.view.decoder().is_none() {
            eprintln!("no platform decoder on this target — skipping");
            return;
        }
        // Ticks advance the 30fps pacing budget and drain the decoder;
        // sleeps give the backend's decode thread wall-clock time. The
        // clip is one pass of 30 access units — `frame_index` crossing
        // that boundary proves EOS drain → flush → loop restart works.
        let pass_len = panel.clip.as_ref().map_or(0, |c| c.len() as u64);
        assert_eq!(pass_len, 30);
        let deadline = Instant::now() + Duration::from_secs(15);
        while panel.frame_index <= pass_len && Instant::now() < deadline {
            panel.tick(Duration::from_millis(16));
            std::thread::sleep(Duration::from_millis(2));
        }
        let dec = panel.view.decoder().expect("decoder attached");
        assert!(
            dec.stats().frames_decoded > 0,
            "platform decoder produced no frames"
        );
        assert_eq!(
            dec.negotiated_format(),
            VideoPixelFormat::Nv12,
            "NV12 was the negotiated output"
        );
        assert!(
            panel.frame_index > pass_len,
            "clip did not loop after end_of_stream"
        );
    }

    /// The dock panels declare their render floors with the intended
    /// degradation policies — the contract `apply_dock_layout`'s
    /// `update_underflow_all` consumes.
    #[test]
    fn panels_declare_min_render_policies() {
        let grid = GridPanel::new(
            Signal::new(1.0),
            Signal::new(String::new()),
            Signal::new(None),
        );
        let min = grid.min_render();
        assert_eq!(min.policy, UnderflowPolicy::Fallback);
        assert_eq!(min.size, Vec2::new(300.0, 170.0));

        let telemetry = TelemetryPanel::new(
            Signal::new(1.0),
            Signal::new(0.0),
            Signal::new(0.0),
            Signal::new(false),
            Signal::new(false),
            Signal::new(16.0),
            Signal::new(false),
        );
        assert_eq!(telemetry.min_render().policy, UnderflowPolicy::Scrim);

        let editor = EditorPanel::new(Signal::new(1.0), EditorSignals::default());
        assert_eq!(editor.min_render().policy, UnderflowPolicy::Clip);

        let media = MediaPanel::new(Signal::new(1.0));
        assert_eq!(media.min_render().policy, UnderflowPolicy::Collapse);
    }
}
