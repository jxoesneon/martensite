//! `AppGrid` — a paginated launcher icon grid (GNOME apps view /
//! iOS home-screen idiom): [`AppEntry`] tiles — a color icon
//! swatch with a caption — flowing left-to-right across fixed-size
//! pages with a dot indicator.
//!
//! Clicking an icon parks its global index in
//! [`AppGrid::take_activated`]; `ArrowLeft`/`ArrowRight` (or a
//! `Page`) change pages. Companion to [`Dock`](crate::widgets::Dock).
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::app_grid::{AppEntry, AppGrid};
//!
//! let g = AppGrid::new().app(AppEntry::new("Files", [90, 140, 200, 255]));
//! assert_eq!(g.app_count(), 1);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::text_paint::SharedTextPainter;

const PAD_PT: f32 = 14.0;
const GAP_PT: f32 = 12.0;
const ICON_PT: f32 = 56.0;
const CAPTION_PT: f32 = 16.0;
const FONT_PT: f32 = 11.0;
const DOT_PT: f32 = 6.0;
const DOTS_PT: f32 = 16.0;
const COLS: usize = 4;
const ROWS: usize = 3;

const FACE: [u8; 4] = [30, 32, 40, 255];
const TEXT: [u8; 4] = [235, 237, 240, 255];
const HOVER: [u8; 4] = [255, 255, 255, 16];
const DOT: [u8; 4] = [120, 124, 134, 255];

/// One launcher tile.
///
/// ```
/// use martensite::widgets::app_grid::AppEntry;
///
/// assert_eq!(AppEntry::new("Files", [1; 4]).name, "Files");
/// ```
#[derive(Clone, Debug)]
pub struct AppEntry {
    /// Display name under the icon.
    pub name: String,
    /// Icon swatch color (stands in for the icon image).
    pub color: [u8; 4],
    /// Optional status lamp painted as a glyph-marked chip on the
    /// icon's top-right corner — pair the state with a mark, never
    /// with the swatch color alone.
    pub status: Option<crate::widgets::status_dot::Status>,
}

impl AppEntry {
    /// A tile with a name and icon color.
    ///
    /// ```
    /// use martensite::widgets::app_grid::AppEntry;
    ///
    /// assert_eq!(AppEntry::new("A", [1; 4]).name, "A");
    /// ```
    pub fn new(name: impl Into<String>, color: [u8; 4]) -> Self {
        Self {
            name: name.into(),
            color,
            status: None,
        }
    }

    /// Attaches a status lamp chip to the tile — the redundant
    /// color+mark channel for callers that used to encode status in
    /// the swatch color alone.
    ///
    /// ```
    /// use martensite::widgets::app_grid::AppEntry;
    /// use martensite::widgets::status_dot::Status;
    ///
    /// let e = AppEntry::new("Pump A", [1; 4]).status(Status::Warning);
    /// assert_eq!(e.status, Some(Status::Warning));
    /// ```
    pub fn status(mut self, status: crate::widgets::status_dot::Status) -> Self {
        self.status = Some(status);
        self
    }
}

/// The grid — see the module docs.
///
/// ```
/// use martensite::widgets::app_grid::AppGrid;
///
/// assert_eq!(AppGrid::new().app_count(), 0);
/// ```
pub struct AppGrid {
    /// Accessibility label.
    pub label: String,
    /// Columns per page.
    pub columns: usize,
    /// Rows per page.
    pub rows: usize,
    apps: Vec<AppEntry>,
    page: usize,
    activated: Option<usize>,
    hovered: Option<usize>,
    cells: Vec<(Rect, usize)>, // cell rect + global index
    text_painter: Option<SharedTextPainter>,
    bounds: Rect,
    scale: f32,
}

impl std::fmt::Debug for AppGrid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppGrid")
            .field("apps", &self.apps.len())
            .field("page", &self.page)
            .finish()
    }
}

impl Default for AppGrid {
    fn default() -> Self {
        Self::new()
    }
}

impl AppGrid {
    /// Empty grid.
    ///
    /// ```
    /// use martensite::widgets::app_grid::AppGrid;
    ///
    /// assert_eq!(AppGrid::new().app_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Applications".to_string(),
            columns: COLS,
            rows: ROWS,
            apps: Vec::new(),
            page: 0,
            activated: None,
            hovered: None,
            cells: Vec::new(),
            text_painter: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Appends an app tile.
    ///
    /// ```
    /// use martensite::widgets::app_grid::{AppEntry, AppGrid};
    ///
    /// assert_eq!(AppGrid::new().app(AppEntry::new("A", [1; 4])).app_count(), 1);
    /// ```
    pub fn app(mut self, entry: AppEntry) -> Self {
        self.apps.push(entry);
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::app_grid::AppGrid;
    ///
    /// assert_eq!(AppGrid::new().label("Apps").label, "Apps");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Page geometry.
    ///
    /// ```
    /// use martensite::widgets::app_grid::AppGrid;
    ///
    /// assert_eq!(AppGrid::new().page_size(6, 3).apps_per_page(), 18);
    /// ```
    pub fn page_size(mut self, columns: usize, rows: usize) -> Self {
        self.columns = columns.max(1);
        self.rows = rows.max(1);
        self
    }

    /// Shared text painter.
    ///
    /// ```no_run
    /// use martensite::widgets::app_grid::AppGrid;
    /// # fn example(p: martensite::text_paint::SharedTextPainter) {
    /// let _g = AppGrid::new().with_text_painter(p);
    /// # }
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// App count.
    ///
    /// ```
    /// use martensite::widgets::app_grid::AppGrid;
    ///
    /// assert_eq!(AppGrid::new().app_count(), 0);
    /// ```
    pub fn app_count(&self) -> usize {
        self.apps.len()
    }

    /// Entries per page.
    ///
    /// ```
    /// use martensite::widgets::app_grid::AppGrid;
    ///
    /// assert_eq!(AppGrid::new().page_size(4, 3).apps_per_page(), 12);
    /// ```
    pub fn apps_per_page(&self) -> usize {
        self.columns * self.rows
    }

    /// Page count (1+ when empty).
    ///
    /// ```
    /// use martensite::widgets::app_grid::{AppEntry, AppGrid};
    ///
    /// let mut g = AppGrid::new().page_size(2, 2);
    /// for _ in 0..5 {
    ///     g = g.app(AppEntry::new("A", [1; 4]));
    /// }
    /// assert_eq!(g.page_count(), 2);
    /// ```
    pub fn page_count(&self) -> usize {
        self.apps.len().div_ceil(self.apps_per_page()).max(1)
    }

    /// Current page index.
    ///
    /// ```
    /// use martensite::widgets::app_grid::AppGrid;
    ///
    /// assert_eq!(AppGrid::new().current_page(), 0);
    /// ```
    pub fn current_page(&self) -> usize {
        self.page
    }

    /// Clamps to a valid page.
    ///
    /// ```
    /// use martensite::widgets::app_grid::{AppEntry, AppGrid};
    ///
    /// let mut g = AppGrid::new().page_size(1, 1).app(AppEntry::new("A", [1; 4]));
    /// g.set_page(9);
    /// assert_eq!(g.current_page(), 0);
    /// ```
    pub fn set_page(&mut self, page: usize) {
        self.page = page.min(self.page_count() - 1);
        self.rebuild_cells();
    }

    /// Drains the last activated app index.
    ///
    /// ```
    /// use martensite::widgets::app_grid::AppGrid;
    ///
    /// let mut g = AppGrid::new();
    /// assert_eq!(g.take_activated(), None);
    /// ```
    pub fn take_activated(&mut self) -> Option<usize> {
        self.activated.take()
    }

    fn hit(&self, p: Vec2) -> Option<usize> {
        self.cells
            .iter()
            .find(|(r, _)| r.contains(p))
            .map(|(_, i)| *i)
    }

    fn rebuild_cells(&mut self) {
        let s = self.scale;
        let bounds = self.bounds;
        self.page = self.page.min(self.page_count() - 1);
        let cols = self.columns.max(1);
        let cell_h = (ICON_PT + CAPTION_PT + GAP_PT) * s;
        let cell_w =
            (bounds.width() - PAD_PT * 2.0 * s - (cols - 1) as f32 * GAP_PT * s) / cols as f32;
        self.cells.clear();
        let base = self.page * self.apps_per_page();
        for slot in 0..self.apps_per_page() {
            let i = base + slot;
            if i >= self.apps.len() {
                break;
            }
            let row = (slot / cols) as f32;
            let col = (slot % cols) as f32;
            self.cells.push((
                Rect::new(
                    bounds.min_x() + PAD_PT * s + col * (cell_w + GAP_PT * s),
                    bounds.min_y() + PAD_PT * s + row * cell_h,
                    cell_w,
                    cell_h - GAP_PT * s,
                ),
                i,
            ));
        }
    }
}

impl Widget for AppGrid {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let s = cx.scale;
        let cell = ICON_PT + CAPTION_PT + GAP_PT;
        Vec2::new(
            ((self.columns as f32 * cell + PAD_PT * 2.0) * s).min(constraints.max_size.x.max(0.0)),
            ((self.rows as f32 * cell + DOTS_PT + PAD_PT * 2.0) * s)
                .min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(160.0, 160.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        self.rebuild_cells();
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Grid);
        node.set_label(self.label.clone());
        node.set_value(format!(
            "page {} of {}, {} apps",
            self.page + 1,
            self.page_count(),
            self.apps.len()
        ));
        // Per-item status chips are paint-only — fold them into the
        // description so AT hears the same state the chip marks.
        let statuses = self
            .apps
            .iter()
            .filter_map(|a| a.status.map(|s| format!("{} {}", a.name, s.label())))
            .collect::<Vec<_>>();
        if !statuses.is_empty() {
            node.set_description(statuses.join(", "));
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerMoved { position } => {
                let h = self.hit(*position);
                if h != self.hovered {
                    self.hovered = h;
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerLeave => {
                if self.hovered.take().is_some() {
                    EventResponse::RequestRepaint
                } else {
                    EventResponse::Ignored
                }
            }
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position,
            } => {
                if let Some(i) = self.hit(*position) {
                    self.activated = Some(i);
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::KeyPressed { key, .. } if key.as_str() == "ArrowRight" => {
                if self.page + 1 < self.page_count() {
                    self.page += 1;
                    self.rebuild_cells();
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::KeyPressed { key, .. } if key.as_str() == "ArrowLeft" => {
                if self.page > 0 {
                    self.page -= 1;
                    self.rebuild_cells();
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let s = cx.scale;
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        cx.list.push_fill_rect(
            kurbo::Rect::new(
                f64::from(self.bounds.min_x()),
                f64::from(self.bounds.min_y()),
                f64::from(self.bounds.max_x()),
                f64::from(self.bounds.max_y()),
            ),
            cx.color(TokenKey::BackgroundColor, FACE),
        );
        let shape = martensite_core::shape::Shape::rounded(10.0 * s);
        for (cell, i) in &self.cells {
            let app = &self.apps[*i];
            let icon = Rect::new(
                cell.min_x() + (cell.width() - ICON_PT * s) / 2.0,
                cell.min_y(),
                ICON_PT * s,
                ICON_PT * s,
            );
            let kr = kurbo::Rect::new(
                f64::from(icon.min_x()),
                f64::from(icon.min_y()),
                f64::from(icon.max_x()),
                f64::from(icon.max_y()),
            );
            if self.hovered == Some(*i) {
                let hr = kurbo::Rect::new(
                    kr.x0 - f64::from(6.0 * s),
                    kr.y0 - f64::from(6.0 * s),
                    kr.x1 + f64::from(6.0 * s),
                    kr.y1 + f64::from(6.0 * s + CAPTION_PT * s),
                );
                cx.list.push_fill_shape(hr, &shape, HOVER);
            }
            cx.list.push_fill_shape(kr, &shape, app.color);
            // Status chip straddling the icon's top-right corner —
            // glyph-marked so the state is never color-only.
            if let Some(status) = app.status {
                crate::widgets::status_dot::paint_status_chip(
                    cx,
                    Vec2::new(icon.max_x(), icon.min_y()),
                    7.0 * s,
                    status,
                );
            }
            // Caption.
            let fs = FONT_PT * s;
            let tw = painter
                .and_then(|p| p.measure_text(&app.name, fs))
                .unwrap_or(app.name.len() as f32 * fs * 0.5);
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                kurbo::Rect::new(
                    f64::from(cell.min_x()),
                    f64::from(cell.min_y()),
                    f64::from(cell.max_x()),
                    f64::from(cell.max_y()),
                ),
                kurbo::Point::new(
                    f64::from(cell.min_x() + (cell.width() - tw) / 2.0),
                    f64::from(icon.max_y() + CAPTION_PT * s * 0.85),
                ),
                &app.name,
                fs,
                cx.color(TokenKey::TextColor, TEXT),
            );
        }
        // Page dots.
        let pages = self.page_count();
        if pages > 1 {
            let d = DOT_PT * s;
            let gap = d * 1.8;
            let total = pages as f32 * gap;
            let mut x = self.bounds.min_x() + (self.bounds.width() - total) / 2.0;
            let y = self.bounds.max_y() - DOTS_PT * s;
            for p in 0..pages {
                let color = if p == self.page {
                    cx.color(TokenKey::AccentColor, [90, 140, 220, 255])
                } else {
                    DOT
                };
                cx.list.push_fill_shape(
                    kurbo::Rect::new(
                        f64::from(x),
                        f64::from(y),
                        f64::from(x + d),
                        f64::from(y + d),
                    ),
                    &martensite_core::shape::Shape::ELLIPSE,
                    color,
                );
                x += gap;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn fixture() -> AppGrid {
        let mut g = AppGrid::new().page_size(2, 2);
        for i in 0..6 {
            g = g.app(AppEntry::new(format!("App{i}"), [100, 100, 100, 255]));
        }
        g
    }

    fn laid_out(g: &mut AppGrid) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        g.layout(&mut cx, Rect::new(0.0, 0.0, 320.0, 260.0));
    }

    fn key(g: &mut AppGrid, k: &str) {
        g.event(&mut EventContext {
            event: &WidgetEvent::KeyPressed {
                key: k.to_string(),
                repeat: false,
            },
            bounds: g.bounds,
            scale: 1.0,
        });
    }

    #[test]
    fn pages_split_apps() {
        let g = fixture();
        assert_eq!(g.page_count(), 2);
        assert_eq!(g.apps_per_page(), 4);
    }

    #[test]
    fn arrows_page() {
        let mut g = fixture();
        laid_out(&mut g);
        key(&mut g, "ArrowRight");
        assert_eq!(g.current_page(), 1);
        key(&mut g, "ArrowRight");
        assert_eq!(g.current_page(), 1); // clamped
        key(&mut g, "ArrowLeft");
        assert_eq!(g.current_page(), 0);
    }

    #[test]
    fn click_activates_global_index() {
        let mut g = fixture();
        g.set_page(1);
        laid_out(&mut g);
        let (r, i) = g.cells[0];
        g.event(&mut EventContext {
            event: &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: Vec2::new((r.min_x() + r.max_x()) / 2.0, (r.min_y() + r.max_y()) / 2.0),
            },
            bounds: g.bounds,
            scale: 1.0,
        });
        assert_eq!(i, 4); // page 2 starts at global index 4
        assert_eq!(g.take_activated(), Some(4));
    }

    #[test]
    fn paint_without_painter() {
        let mut g = fixture();
        laid_out(&mut g);
        let mut list = martensite_core::PaintList::default();
        let theme = martensite_theme::Theme::new("test");
        g.paint(&mut PaintContext {
            list: &mut list,
            bounds: g.bounds,
            theme: &theme,
            scale: 1.0,
            text_painter: None,
        });
        assert!(!list.is_empty());
    }
}
