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

use std::collections::VecDeque;
use std::time::Duration;

use parking_lot::Mutex;

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite::blessed::data_table::{ColumnConfig, ColumnSort, KeyAction, RowFilter};
use martensite::blessed::{Chart, CodeEditor, DataTable, LineSeries, Point as CPoint, TokenKind};
use martensite::core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, SemanticAction, Widget, WidgetEvent,
};
use martensite::media::surface::{VideoPixelFormat, VideoSurface};
use martensite::prelude::*;
use martensite::render::{BezPath, PaintList, Point};
use martensite::widgets::media::{MediaView, VideoFit};

use crate::model::{alert_count, gen_rows, MetricRow, Palette, EDITOR_SOURCE};
use crate::text::{SpanColor, TextPainter};

/// Title-bar height in logical pt — one constant for the chrome paint
/// and every hit-zone/layout computation that subtracts it.
const TITLE_H: f32 = 28.0;
/// Grid column-header band height in logical pt.
const GRID_HEADER_H: f32 = 26.0;

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

/// Paints the shared panel chrome — surface, title bar, hairline, focus
/// ring — and returns the inner content rect (panel-local, still in
/// window coordinates). `right` is drawn muted at the title bar's
/// trailing edge (row counts, state badges).
#[allow(clippy::too_many_arguments)]
fn panel_chrome(
    painter: &mut TextPainter,
    list: &mut PaintList,
    bounds: Rect,
    title: &str,
    right: &str,
    pal: &Palette,
    scale: f32,
    focused: bool,
) -> martensite::render::Rect {
    let b = to_paint(bounds);
    let s = f64::from(scale);
    list.push_fill_rect(b, pal.surface);
    let title_h = f64::from(TITLE_H) * s;
    list.push_fill_rect(krect(b.x0, b.y0, b.width(), title_h), pal.raised);
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
    if focused {
        list.push_stroke_rect(
            krect(b.x0 + 1.0, b.y0 + 1.0, b.width() - 2.0, b.height() - 2.0),
            2.0,
            pal.accent,
        );
    } else {
        list.push_stroke_rect(b, 1.0, pal.hairline());
    }
    krect(
        b.x0,
        b.y0 + title_h + 1.0,
        b.width(),
        (b.height() - title_h - 1.0).max(0.0),
    )
}

// ---------------------------------------------------------------------------
// GridPanel — virtualized 1M-row DataTable
// ---------------------------------------------------------------------------

/// Column headers — `ColumnConfig` carries widths but no title field
/// (F6), so the renderer owns the labels, in column order.
const GRID_HEADERS: [&str; 4] = ["PID", "CPU %", "MEMORY", "STATUS"];

/// The process-metrics table: column-header sort toggling, click
/// selection (+ Shift range), wheel scrolling, full keyboard navigation,
/// an `F` alert-only filter toggle, all over the virtualized 1M-row
/// `DataTable` model.
pub struct GridPanel {
    table: DataTable<Vec<MetricRow>>,
    text: Mutex<TextPainter>,
    scale: Signal<f32>,
    pal: Palette,
    bounds: Rect,
    focused: bool,
    /// F17 — modifiers tracked through the key stream.
    shift_held: bool,
    ctrl_held: bool,
    alert_rows: usize,
}

impl GridPanel {
    pub fn new(scale: Signal<f32>, pal: Palette) -> Self {
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
        Self {
            table,
            text: Mutex::new(TextPainter::new()),
            scale,
            pal,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            focused: false,
            shift_held: false,
            ctrl_held: false,
            alert_rows,
        }
    }

    fn s(&self) -> f32 {
        self.scale.get()
    }

    fn row_h(&self) -> f32 {
        30.0 * self.s().max(1.0)
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

    /// Storage index of the row at window-y `y`, via the visible window
    /// (display position → storage through the same mapping the painter
    /// uses — `visible_rows` yields storage indices).
    fn row_at(&self, y: f32) -> Option<usize> {
        // Rows slide by the fractional scroll remainder — the painter
        // draws row n at `n·row_h - frac`, so hit-testing must add frac
        // back before dividing.
        let frac = self.table.scroll_offset() % self.row_h();
        let rel = y - self.rows_top() + frac;
        if rel < 0.0 {
            return None;
        }
        self.table
            .visible_rows()
            .nth((rel / self.row_h()) as usize)
            .map(|(idx, _)| idx)
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
}

impl Widget for GridPanel {
    fn measure(&mut self, _cx: &mut LayoutContext, _c: LayoutConstraints) -> Vec2 {
        Vec2::new(620.0, 420.0)
    }

    fn layout(&mut self, _cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        // Re-arm the model's row height from the live scale signal —
        // `set_row_height` preserves the scroll position in rows.
        self.table.set_row_height(self.row_h());
        self.table.set_viewport_height(
            (bounds.size.y - (TITLE_H + GRID_HEADER_H) * self.s() - 2.0).max(0.0),
        );
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerPressed {
                position,
                button: PointerButton::Primary,
            } => {
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
            WidgetEvent::Scroll { delta, .. } => {
                self.table.scroll_by(-delta.y);
                EventResponse::RequestRepaint
            }
            WidgetEvent::KeyPressed { key, repeat } => {
                match key.as_str() {
                    "Shift" => self.shift_held = true,
                    "Control" => self.ctrl_held = true,
                    "f" | "F" if !repeat => {
                        if self.table.filter_active() {
                            self.table.clear_filter();
                        } else {
                            self.table
                                .set_filter(RowFilter::new(|r: &MetricRow| r.alert));
                        }
                    }
                    _ => {
                        if let Some(action) =
                            KeyAction::from_key_name(key, self.shift_held, self.ctrl_held)
                        {
                            self.table.handle_key(action);
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
        let pal = &self.pal;
        let s = self.s();
        let mut text = self.text.lock();
        let right = format!(
            "{} rows · {} sel · {} alerts{}",
            fmt_count(self.table.display_row_count()),
            self.table.selection().selected_count(),
            fmt_count(self.alert_rows),
            if self.table.filter_active() {
                " · FILTERED"
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
            self.focused,
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
        for (n, (idx, row)) in self.table.visible_rows().enumerate() {
            let ry =
                rows_top + n as f64 * row_h - f64::from(self.table.scroll_offset() % self.row_h());
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
            let ty = ry + (row_h - 13.0 * sd) / 2.0;
            // A straddling row keeps its stripe/focus ring, but when its
            // text origin is already below the clip the runs produce
            // zero output — skip them (and the chip) entirely.
            if ty >= rows_bottom {
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
                let tw = text.measure(&value, 12.0 * s);
                text.push(
                    cx.list,
                    Point::new(f64::from(x0 + w) - 10.0 * sd - f64::from(tw), ty),
                    &value,
                    12.0 * s,
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
            cx.list
                .push_fill_rect(krect(chip_x, chip_y, chip_w, chip_h), chip_bg);
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
            cx.list
                .push_fill_rect(krect(track_x, thumb_y, 4.0 * sd, thumb_h), pal.text_muted);
        }
        cx.list.pop_clip();
    }
}

// ---------------------------------------------------------------------------
// TelemetryPanel — live Signal-driven chart
// ---------------------------------------------------------------------------

/// Telemetry chart: two live `Signal<f64>` feeds (the app holds clones
/// for the header KPIs) rendered through `LineSeries` +
/// `Chart::project`. `tick` advances the model — Space pauses.
pub struct TelemetryPanel {
    text: Mutex<TextPainter>,
    scale: Signal<f32>,
    pal: Palette,
    bounds: Rect,
    focused: bool,
    /// Shared with the app's header KPIs — this panel is the writer.
    pub cpu: Signal<f64>,
    pub mem: Signal<f64>,
    phase: f64,
    history: VecDeque<(f64, f64)>,
    paused: bool,
    elapsed: Duration,
}

impl TelemetryPanel {
    const CAP: usize = 240;

    pub fn new(scale: Signal<f32>, pal: Palette, cpu: Signal<f64>, mem: Signal<f64>) -> Self {
        Self {
            text: Mutex::new(TextPainter::new()),
            scale,
            pal,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            focused: false,
            cpu,
            mem,
            phase: 0.0,
            history: VecDeque::with_capacity(Self::CAP + 1),
            paused: false,
            elapsed: Duration::ZERO,
        }
    }

    fn s(&self) -> f32 {
        self.scale.get()
    }
}

impl Widget for TelemetryPanel {
    fn measure(&mut self, _cx: &mut LayoutContext, _c: LayoutConstraints) -> Vec2 {
        Vec2::new(560.0, 300.0)
    }

    fn layout(&mut self, _cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
    }

    fn tick(&mut self, dt: Duration) -> bool {
        self.elapsed += dt;
        if self.paused {
            return false;
        }
        // ~10 Hz sample cadence keeps the trace calm and legible.
        if self.elapsed < Duration::from_millis(100) {
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
        match cx.event {
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                ..
            } => EventResponse::CaptureFocus,
            WidgetEvent::KeyPressed { key, repeat } => {
                if key == " " && !repeat {
                    self.paused = !self.paused;
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
            if self.paused { ", paused" } else { "" }
        ));
    }

    fn paint(&self, cx: &mut PaintContext) {
        let pal = &self.pal;
        let s = self.s();
        let mut text = self.text.lock();
        let state = if self.paused { "PAUSED" } else { "LIVE" };
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
        let inner = panel_chrome(
            &mut text,
            cx.list,
            self.bounds,
            "TELEMETRY",
            &right,
            pal,
            s,
            self.focused,
        );
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

        // Horizontal gridlines at 0/25/50/75/100% with labels.
        for frac in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let y = plot.y1 - frac * plot.height();
            cx.list.push_fill_rect(
                krect(plot.x0, y, plot.width(), 1.0),
                Palette::alpha(pal.border, if frac == 0.0 { 255 } else { 110 }),
            );
            text.push(
                cx.list,
                Point::new(inner.x0 + 2.0 * sd, y - 6.0 * sd),
                &format!("{:.0}", frac * 100.0),
                12.0 * s,
                pal.text_muted,
                Some(label_w as f32),
            );
        }

        // Series → projected polylines (cpu filled, mem line-only).
        for (series_idx, color, fill) in [(0usize, pal.accent, true), (1usize, pal.accent2, false)]
        {
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
            if fill {
                let mut area = path.clone();
                let last = Chart::project(
                    *pts.last().expect("nonempty"),
                    bounds,
                    plot.width(),
                    plot.height(),
                );
                let first = Chart::project(pts[0], bounds, plot.width(), plot.height());
                area.line_to((plot.x0 + last.x, plot.y1));
                area.line_to((plot.x0 + first.x, plot.y1));
                area.close_path();
                cx.list.push_path(area, Palette::alpha(color, 40));
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
    }
}

// ---------------------------------------------------------------------------
// EditorPanel — CodeEditor model with real syntax highlighting
// ---------------------------------------------------------------------------

/// The `workstation.toml` editor: click-to-place caret, `ImeCommitted`
/// text input, Backspace/Enter/arrows editing, `highlight_line` spans
/// mapped to palette colors per glyph byte-offset.
pub struct EditorPanel {
    editor: CodeEditor,
    text: Mutex<TextPainter>,
    scale: Signal<f32>,
    pal: Palette,
    bounds: Rect,
    focused: bool,
    scroll_top: usize,
}

impl EditorPanel {
    pub fn new(scale: Signal<f32>, pal: Palette) -> Self {
        Self {
            editor: CodeEditor::new(EDITOR_SOURCE),
            text: Mutex::new(TextPainter::new()),
            scale,
            pal,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            focused: false,
            scroll_top: 0,
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

    fn kind_color(&self, kind: TokenKind) -> [u8; 4] {
        match kind {
            TokenKind::Keyword => self.pal.accent,
            TokenKind::String => self.pal.ok,
            TokenKind::Comment => self.pal.text_muted,
            TokenKind::Number => self.pal.warn,
            _ => self.pal.text,
        }
    }

    /// Ensures the caret's line is inside the scroll window.
    fn ensure_visible(&mut self, lines_visible: usize) {
        if let Some(cur) = self.editor.cursors().first() {
            if cur.line < self.scroll_top {
                self.scroll_top = cur.line;
            } else if lines_visible > 0 && cur.line >= self.scroll_top + lines_visible {
                self.scroll_top = cur.line + 1 - lines_visible;
            }
        }
    }
}

impl Widget for EditorPanel {
    fn measure(&mut self, _cx: &mut LayoutContext, _c: LayoutConstraints) -> Vec2 {
        Vec2::new(400.0, 260.0)
    }

    fn layout(&mut self, _cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        let s = self.s();
        // Matches the painter: title bar + hairline + content padding.
        let content_top = self.bounds.min_y() + TITLE_H * s + 1.0 + 8.0 * s;
        let line_h = self.line_h();
        let visible = ((self.bounds.max_y() - content_top - 8.0 * s) / line_h).max(1.0) as usize;
        match cx.event {
            WidgetEvent::PointerPressed {
                position,
                button: PointerButton::Primary,
            } => {
                // Clicks in the title bar focus the panel but never move
                // the caret.
                if position.y < self.bounds.min_y() + TITLE_H * s + 1.0 {
                    return EventResponse::CaptureFocus;
                }
                let line = (self.scroll_top
                    + ((position.y - content_top) / line_h).max(0.0) as usize)
                    .min(self.editor.lines().len().saturating_sub(1));
                let text_x = position.x - (self.bounds.min_x() + self.gutter_w() + 8.0 * s);
                let col = {
                    let mut t = self.text.lock();
                    let line_str = &self.editor.lines()[line];
                    t.column_at(line_str, 12.0 * s, text_x.max(0.0))
                };
                self.editor
                    .set_cursors(vec![martensite::blessed::Cursor::new(line, col)]);
                self.ensure_visible(visible);
                EventResponse::CaptureFocus
            }
            WidgetEvent::ImeCommitted { text } => {
                // Printable input only — control chars (Enter/Tab) ride
                // KeyPressed below.
                let printable: String = text.chars().filter(|c| !c.is_control()).collect();
                if !printable.is_empty() {
                    self.editor.insert(&printable);
                    self.ensure_visible(visible);
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Handled
            }
            WidgetEvent::KeyPressed { key, repeat } => {
                match key.as_str() {
                    "Backspace" => self.editor.delete_backward(),
                    "Enter" => self.editor.insert("\n"),
                    "ArrowLeft" | "ArrowRight" | "ArrowUp" | "ArrowDown" | "Home" | "End" => {
                        if let Some(cur) = self.editor.cursors().first().copied() {
                            let lines = self.editor.lines();
                            let cur = match key.as_str() {
                                "ArrowLeft" => martensite::blessed::Cursor::new(
                                    cur.line,
                                    cur.column.saturating_sub(1),
                                ),
                                "ArrowRight" => martensite::blessed::Cursor::new(
                                    cur.line,
                                    (cur.column + 1).min(lines[cur.line].chars().count()),
                                ),
                                "ArrowUp" => martensite::blessed::Cursor::new(
                                    cur.line.saturating_sub(1),
                                    cur.column
                                        .min(lines[cur.line.saturating_sub(1)].chars().count()),
                                ),
                                "ArrowDown" => martensite::blessed::Cursor::new(
                                    (cur.line + 1).min(lines.len().saturating_sub(1)),
                                    cur.column.min(
                                        lines[(cur.line + 1).min(lines.len().saturating_sub(1))]
                                            .chars()
                                            .count(),
                                    ),
                                ),
                                "Home" => martensite::blessed::Cursor::new(cur.line, 0),
                                _ => martensite::blessed::Cursor::new(
                                    cur.line,
                                    lines[cur.line].chars().count(),
                                ),
                            };
                            self.editor.set_cursors(vec![cur]);
                        }
                    }
                    _ => {}
                }
                self.ensure_visible(visible);
                let _ = repeat;
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
        node.set_role(accesskit::Role::MultilineTextInput);
        node.set_label("Editor — workstation.toml");
        node.set_value(self.editor.text());
    }

    fn paint(&self, cx: &mut PaintContext) {
        let pal = &self.pal;
        let s = self.s();
        let mut text = self.text.lock();
        let full = format!(
            "{} lines · cursor {}:{}",
            self.editor.lines().len(),
            self.editor
                .cursors()
                .first()
                .map(|c| c.line + 1)
                .unwrap_or(0),
            self.editor.cursors().first().map(|c| c.column).unwrap_or(0)
        );
        let slot = (self.bounds.width() * 0.42).max(1.0);
        let right = if text.measure(&full, 12.0 * s) <= slot {
            full
        } else {
            format!("{} lines", self.editor.lines().len())
        };
        let inner = panel_chrome(
            &mut text,
            cx.list,
            self.bounds,
            "EDITOR",
            &right,
            pal,
            s,
            self.focused,
        );
        let sd = f64::from(s);
        let pad = 8.0 * sd;
        let gutter_w = f64::from(self.gutter_w());
        let line_h = f64::from(self.line_h());
        let font = 12.0 * s;
        let content_top = inner.y0 + pad;
        cx.list
            .push_clip(krect(inner.x0, inner.y0, inner.width(), inner.height()));
        cx.list.push_fill_rect(
            krect(inner.x0, inner.y0, gutter_w, inner.height()),
            Palette::alpha(pal.raised, 90),
        );

        let caret = self.editor.cursors().first().copied();
        let mut y = content_top;
        for (li, line) in self.editor.lines().iter().enumerate().skip(self.scroll_top) {
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
            let spans: Vec<SpanColor> = self
                .editor
                .highlight_line(li)
                .into_iter()
                .map(|sp| SpanColor {
                    start: sp.start,
                    end: sp.end,
                    color: self.kind_color(sp.kind),
                })
                .collect();
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
    }
}

// ---------------------------------------------------------------------------
// MediaPanel — MediaView over a mock NV12 surface (honest empty state)
// ---------------------------------------------------------------------------

/// Media surface panel: a real `MediaView` widget (Contain fit, mock
/// 1920×1080 NV12 `VideoSurface`) composited through the widget tree.
/// No decoder is attached — the letterboxed backdrop and the "awaiting
/// frames" overlay are the honest state, not a faked video.
pub struct MediaPanel {
    view: MediaView,
    text: Mutex<TextPainter>,
    scale: Signal<f32>,
    pal: Palette,
    bounds: Rect,
    focused: bool,
    elapsed_nanos: u64,
}

impl MediaPanel {
    pub fn new(scale: Signal<f32>, pal: Palette) -> Self {
        let surface = VideoSurface::new_mock(1920, 1080, VideoPixelFormat::Nv12);
        Self {
            view: MediaView::new()
                .with_fit(VideoFit::Contain)
                .with_surface(surface),
            text: Mutex::new(TextPainter::new()),
            scale,
            pal,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            focused: false,
            elapsed_nanos: 0,
        }
    }

    fn s(&self) -> f32 {
        self.scale.get()
    }
}

impl Widget for MediaPanel {
    fn measure(&mut self, _cx: &mut LayoutContext, _c: LayoutConstraints) -> Vec2 {
        Vec2::new(320.0, 220.0)
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
        node.set_label("Media surface — 1920x1080 NV12 mock, no decoder attached");
    }

    fn paint(&self, cx: &mut PaintContext) {
        let pal = &self.pal;
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
        let inner = panel_chrome(
            &mut text,
            cx.list,
            self.bounds,
            "MEDIA",
            &right,
            pal,
            s,
            self.focused,
        );
        // Delegate to the real widget for the letterboxed backdrop.
        self.view.paint(&mut PaintContext {
            list: cx.list,
            bounds: Rect::new(
                inner.x0 as f32,
                inner.y0 as f32,
                inner.width() as f32,
                inner.height() as f32,
            ),
        });
        let sd = f64::from(s);
        let cx0 = inner.x0 + inner.width() / 2.0;
        let cy = inner.y0 + inner.height() / 2.0;
        // Width-adaptive overlay — full strings when the panel has room,
        // compact variants when narrow (still honest, never fake frames).
        let roomy = inner.width() >= 300.0 * sd;
        let l1 = if roomy {
            "NV12 1920×1080 — mock surface".to_string()
        } else {
            "NV12 · 1080p".to_string()
        };
        let l2 = if roomy {
            format!("awaiting decoder — {} queued", self.view.queued_frames())
        } else {
            "no decoder".to_string()
        };
        for (i, (line, color)) in [(l1.as_str(), pal.text), (l2.as_str(), pal.text_muted)]
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
