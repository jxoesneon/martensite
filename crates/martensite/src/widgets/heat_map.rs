//! `HeatMap` — an intensity grid (GitHub contribution graph, d3
//! heatmap, Ant `CalendarHeatmap`-style).
//!
//! A `rows × cols` matrix of `f32` intensities maps through a
//! sequential color ramp — empty cells paint the track color, rising
//! intensity deepens the accent. Cell hit-testing parks
//! `(row, col, value)` in [`HeatMap::take_hovered`] for app tooltips.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::heat_map::HeatMap;
//!
//! let hm = HeatMap::new(7, 52) // a year of weeks
//!     .set(0, 10, 3.0)
//!     .set(1, 10, 8.0);
//! assert_eq!(hm.get(1, 10), 8.0);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

const GAP_PT: f32 = 2.0;
const RADIUS_PT: f32 = 2.0;
/// Zero-cell fallback when the theme carries no `InsetColor`.
const TRACK: [u8; 4] = [235, 237, 240, 255];
/// Hot-end fallback when the theme carries no `SeriesColor1` — the
/// historical GitHub-green deep end.
const RAMP_HI: [u8; 4] = [33, 110, 57, 255];

/// An intensity grid — see the module docs.
///
/// ```
/// use martensite::widgets::heat_map::HeatMap;
///
/// let hm = HeatMap::new(3, 4);
/// assert_eq!(hm.rows, 3);
/// ```
pub struct HeatMap {
    /// Row count.
    pub rows: usize,
    /// Column count.
    pub cols: usize,
    /// When `false` the grid is inert.
    pub enabled: bool,
    cells: Vec<f32>,
    hovered_out: Option<(usize, usize, f32)>,
    hovered: Option<(usize, usize)>,
    bounds: Rect,
    scale: f32,
}

impl HeatMap {
    /// Creates a `rows × cols` zeroed grid.
    ///
    /// ```
    /// use martensite::widgets::heat_map::HeatMap;
    ///
    /// let hm = HeatMap::new(2, 3);
    /// assert_eq!(hm.cols, 3);
    /// ```
    pub fn new(rows: usize, cols: usize) -> Self {
        Self {
            rows,
            cols,
            enabled: true,
            cells: vec![0.0; rows * cols],
            hovered_out: None,
            hovered: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Sets cell `(row, col)`'s intensity (out-of-bounds ignored).
    ///
    /// ```
    /// use martensite::widgets::heat_map::HeatMap;
    ///
    /// let hm = HeatMap::new(2, 2).set(1, 1, 5.0);
    /// assert_eq!(hm.get(1, 1), 5.0);
    /// ```
    pub fn set(mut self, row: usize, col: usize, value: f32) -> Self {
        self.set_cell(row, col, value);
        self
    }

    /// Mutable set — for appending data after construction.
    ///
    /// ```
    /// use martensite::widgets::heat_map::HeatMap;
    ///
    /// let mut hm = HeatMap::new(1, 1);
    /// hm.set_cell(0, 0, 2.0);
    /// assert_eq!(hm.get(0, 0), 2.0);
    /// ```
    pub fn set_cell(&mut self, row: usize, col: usize, value: f32) {
        if row < self.rows && col < self.cols {
            self.cells[row * self.cols + col] = value;
        }
    }

    /// Cell `(row, col)`'s intensity (`0.0` out-of-bounds).
    ///
    /// ```
    /// use martensite::widgets::heat_map::HeatMap;
    ///
    /// let hm = HeatMap::new(1, 1).set(0, 0, 7.0);
    /// assert_eq!(hm.get(0, 0), 7.0);
    /// ```
    pub fn get(&self, row: usize, col: usize) -> f32 {
        if row < self.rows && col < self.cols {
            self.cells[row * self.cols + col]
        } else {
            0.0
        }
    }

    /// Enables or disables the grid.
    ///
    /// ```
    /// use martensite::widgets::heat_map::HeatMap;
    ///
    /// let hm = HeatMap::new(1, 1).enabled(false);
    /// assert!(!hm.enabled);
    /// ```
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Drains the last `(row, col, value)` the pointer entered.
    ///
    /// ```
    /// use martensite::widgets::heat_map::HeatMap;
    ///
    /// let mut hm = HeatMap::new(1, 1);
    /// assert_eq!(hm.take_hovered(), None);
    /// ```
    pub fn take_hovered(&mut self) -> Option<(usize, usize, f32)> {
        self.hovered_out.take()
    }

    /// Peak intensity (for the ramp denominator; `1.0` when flat).
    fn max_value(&self) -> f32 {
        self.cells.iter().copied().fold(0.0, f32::max).max(1.0)
    }

    /// Cell rect in device px.
    fn cell_rect(&self, row: usize, col: usize) -> Rect {
        let gap = GAP_PT * self.scale;
        let w = (self.bounds.size.x - gap * (self.cols - 1) as f32) / self.cols as f32;
        let h = (self.bounds.size.y - gap * (self.rows - 1) as f32) / self.rows as f32;
        Rect::new(
            self.bounds.origin.x + col as f32 * (w + gap),
            self.bounds.origin.y + row as f32 * (h + gap),
            w.max(0.0),
            h.max(0.0),
        )
    }

    /// Cell under `position`, or `None`.
    fn hit(&self, position: Vec2) -> Option<(usize, usize)> {
        for row in 0..self.rows {
            for col in 0..self.cols {
                if self.cell_rect(row, col).contains(position) {
                    return Some((row, col));
                }
            }
        }
        None
    }

    /// Sequential ramp for one cell: `track` at/below zero, then a
    /// `lo`→`hi` lerp for positive values. Endpoints are resolved by
    /// the caller so the ramp tracks the theme's categorical ink.
    fn ramp(&self, value: f32, max: f32, track: [u8; 4], lo: [u8; 4], hi: [u8; 4]) -> [u8; 4] {
        // NaN/±inf cells read as "no data" — the inset well — matching
        // the a11y path which skips non-finite cells; without the
        // finite check NaN survived `<= 0.0` and `as u8` cast it to
        // opaque black.
        if !value.is_finite() || value <= 0.0 || max <= 0.0 {
            return track;
        }
        let t = (value / max).clamp(0.0, 1.0);
        let mut c = [0u8; 4];
        for i in 0..3 {
            c[i] = (lo[i] as f32 + (hi[i] as f32 - lo[i] as f32) * t) as u8;
        }
        c[3] = 255;
        c
    }
}

impl Widget for HeatMap {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            cx.pt(self.cols.max(1) as f32 * 12.0)
                .min(constraints.max_size.x.max(0.0)),
            cx.pt(self.rows.max(1) as f32 * 12.0)
                .min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(24.0, 24.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Image);
        node.set_label("Heat map");
        node.set_value(format!("{} by {}", self.rows, self.cols));
        // A real value summary — dimensions, occupancy, min/max/mean —
        // so the grid's data is legible without the pixels (D2).
        let (mut lo, mut hi, mut sum, mut n, mut nonzero) =
            (f32::MAX, f32::MIN, 0.0f32, 0usize, 0usize);
        for &v in &self.cells {
            // Non-finite = "no data", matching the ramp's track fill —
            // an inf cell would otherwise poison max/mean (max=inf
            // flattens every finite cell to `lo`).
            if !v.is_finite() {
                continue;
            }
            lo = lo.min(v);
            hi = hi.max(v);
            sum += v;
            n += 1;
            if v != 0.0 {
                nonzero += 1;
            }
        }
        node.set_description(if n == 0 {
            format!("{} by {} grid, no data", self.rows, self.cols)
        } else {
            format!(
                "{} by {} grid, {} of {} cells non-zero, min {:.2}, max {:.2}, mean {:.2}",
                self.rows,
                self.cols,
                nonzero,
                self.cells.len(),
                lo,
                hi,
                sum / n as f32,
            )
        });
        if !self.enabled {
            node.set_disabled();
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.enabled {
            return EventResponse::Ignored;
        }
        match cx.event {
            WidgetEvent::PointerMoved { position } => {
                let h = self.hit(*position);
                if h != self.hovered {
                    self.hovered = h;
                    self.hovered_out = h.map(|(r, c)| (r, c, self.get(r, c)));
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerLeave => {
                if self.hovered.is_some() {
                    self.hovered = None;
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let max = self.max_value();
        let radius = cx.pt(RADIUS_PT);
        let shape = martensite_core::shape::Shape::rounded(radius);
        // Resolve the ramp once per paint: the zero state reads as the
        // inset well, the hot end is the theme's primary series ink.
        // (Resolving each cell through `AccentColor` — as this used to —
        // flattened the whole ramp to a single flat accent fill.)
        let track = cx.color(TokenKey::InsetColor, TRACK);
        let hi = crate::widgets::series_color(cx, 0, None, RAMP_HI);
        // Low floor: 35% toward the hot hue so near-zero cells stay
        // legible against `track` instead of fading into it.
        let lo = {
            let mut c = [0u8; 4];
            for i in 0..3 {
                c[i] = (track[i] as f32 * 0.65 + hi[i] as f32 * 0.35) as u8;
            }
            c[3] = 255;
            c
        };
        for row in 0..self.rows {
            for col in 0..self.cols {
                let r = self.cell_rect(row, col);
                let mut color = self.ramp(self.get(row, col), max, track, lo, hi);
                if self.hovered == Some((row, col)) {
                    // Darken slightly for the hover cue.
                    for c in color.iter_mut().take(3) {
                        *c = (*c as u16 * 4 / 5) as u8;
                    }
                }
                cx.list.push_fill_shape(
                    kurbo::Rect::new(
                        f64::from(r.min_x()),
                        f64::from(r.min_y()),
                        f64::from(r.max_x()),
                        f64::from(r.max_y()),
                    ),
                    &shape,
                    color,
                );
            }
        }
    }
}

impl std::fmt::Debug for HeatMap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeatMap")
            .field("rows", &self.rows)
            .field("cols", &self.cols)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(hm: &mut HeatMap, w: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        hm.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(w, h),
            },
        );
        hm.layout(&mut cx, Rect::new(0.0, 0.0, w, h));
    }

    #[test]
    fn set_get_roundtrip() {
        let mut hm = HeatMap::new(3, 4);
        hm.set_cell(1, 2, 5.0);
        assert_eq!(hm.get(1, 2), 5.0);
        assert_eq!(hm.get(9, 9), 0.0); // out of bounds
        hm.set_cell(9, 9, 1.0); // ignored
        assert_eq!(hm.get(9, 9), 0.0);
    }

    #[test]
    fn ramp_scales_to_max() {
        let hm = HeatMap::new(1, 2).set(0, 0, 10.0).set(0, 1, 5.0);
        let (track, lo, hi) = (TRACK, [155, 233, 168, 255], RAMP_HI);
        assert_eq!(hm.ramp(0.0, 10.0, track, lo, hi), track);
        let hot = hm.ramp(10.0, 10.0, track, lo, hi);
        assert_eq!(hot[1], hi[1]); // max → deep end
    }

    #[test]
    fn hit_finds_cell() {
        let mut hm = HeatMap::new(2, 2);
        laid_out(&mut hm, 100.0, 100.0);
        assert_eq!(hm.hit(Vec2::new(10.0, 10.0)), Some((0, 0)));
        assert_eq!(hm.hit(Vec2::new(90.0, 90.0)), Some((1, 1)));
    }

    #[test]
    fn hover_parks_cell_and_value() {
        let mut hm = HeatMap::new(2, 2).set(1, 1, 7.0);
        laid_out(&mut hm, 100.0, 100.0);
        hm.event(&mut EventContext {
            event: &WidgetEvent::PointerMoved {
                position: Vec2::new(90.0, 90.0),
            },
            bounds: Rect::new(0.0, 0.0, 100.0, 100.0),
            scale: 1.0,
        });
        assert_eq!(hm.take_hovered(), Some((1, 1, 7.0)));
        assert_eq!(hm.take_hovered(), None);
    }

    #[test]
    fn a11y_description_summarizes_values() {
        let hm = HeatMap::new(2, 2).set(0, 0, 2.0).set(1, 1, 6.0);
        let mut node = AccessKitNode::new(accesskit::Role::Unknown);
        hm.accessibility(&mut node);
        let desc = node.description().unwrap_or_default();
        assert!(desc.contains("2 by 2"), "{desc}");
        assert!(desc.contains("2 of 4"), "{desc}");
        assert!(desc.contains("min 0.00"), "{desc}");
        assert!(desc.contains("max 6.00"), "{desc}");
        assert!(desc.contains("mean 2.00"), "{desc}");
    }

    #[test]
    fn cell_rects_tile_bounds() {
        let mut hm = HeatMap::new(2, 2);
        laid_out(&mut hm, 102.0, 102.0);
        let r00 = hm.cell_rect(0, 0);
        let r11 = hm.cell_rect(1, 1);
        assert!(r11.min_x() > r00.max_x());
        assert!(r11.min_y() > r00.max_y());
    }
}
