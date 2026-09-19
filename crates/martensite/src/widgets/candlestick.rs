//! `Candlestick` — an OHLC financial chart (candle bars — Qt
//! `QCandlestickSeries`, Ant `Stock`, trading-view style).
//!
//! Each [`Candle`] paints a high–low wick plus an open–close body —
//! up-close in the success tone, down-close in the error tone. The y
//! range auto-fits the high/low extrema or pins through `y_range`;
//! pointer hover parks the nearest candle index in
//! [`Candlestick::take_hovered`] for app tooltips.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::candlestick::{Candle, Candlestick};
//!
//! let c = Candlestick::new()
//!     .candle(Candle::new(10.0, 12.0, 9.0, 11.0));
//! assert_eq!(c.candle_count(), 1);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

const W_PT: f32 = 240.0;
const H_PT: f32 = 160.0;
const PAD_PT: f32 = 8.0;
const GRID_N: usize = 4;
const HOVER_PT: f32 = 12.0;

const GRID: [u8; 4] = [215, 217, 222, 255];
const FRAME: [u8; 4] = [150, 152, 158, 255];
const SURFACE: [u8; 4] = [250, 250, 252, 255];
const UP: [u8; 4] = [46, 160, 90, 255];
const DOWN: [u8; 4] = [210, 60, 60, 255];

/// One OHLC candle.
///
/// ```
/// use martensite::widgets::candlestick::Candle;
///
/// let c = Candle::new(10.0, 12.0, 9.0, 11.0);
/// assert!(c.is_up());
/// ```
#[derive(Clone, Copy, Debug)]
pub struct Candle {
    /// Opening price.
    pub open: f32,
    /// Highest price.
    pub high: f32,
    /// Lowest price.
    pub low: f32,
    /// Closing price.
    pub close: f32,
}

impl Candle {
    /// Creates a candle (`open, high, low, close`).
    ///
    /// ```
    /// use martensite::widgets::candlestick::Candle;
    ///
    /// let c = Candle::new(5.0, 8.0, 4.0, 6.0);
    /// assert_eq!(c.high, 8.0);
    /// ```
    pub fn new(open: f32, high: f32, low: f32, close: f32) -> Self {
        Self {
            open,
            high,
            low,
            close,
        }
    }

    /// `true` when `close ≥ open` (up candle).
    ///
    /// ```
    /// use martensite::widgets::candlestick::Candle;
    ///
    /// assert!(Candle::new(1.0, 2.0, 0.5, 1.5).is_up());
    /// assert!(!Candle::new(2.0, 2.5, 1.0, 1.5).is_up());
    /// ```
    pub fn is_up(&self) -> bool {
        self.close >= self.open
    }
}

/// An OHLC candlestick chart — see the module docs.
///
/// ```
/// use martensite::widgets::candlestick::Candlestick;
///
/// let c = Candlestick::new();
/// assert_eq!(c.candle_count(), 0);
/// ```
pub struct Candlestick {
    /// Candles left-to-right.
    pub candles: Vec<Candle>,
    /// Pinned y range; `None` fits `low..high`.
    pub y_range: Option<(f32, f32)>,
    /// Whether grid lines paint.
    pub grid: bool,
    /// When `false` the chart is inert.
    pub enabled: bool,
    hovered: Option<usize>,
    hovered_out: Option<usize>,
    bounds: Rect,
    plot: Rect,
    scale: f32,
}

impl Default for Candlestick {
    fn default() -> Self {
        Self::new()
    }
}

impl Candlestick {
    /// Creates an empty chart.
    ///
    /// ```
    /// use martensite::widgets::candlestick::Candlestick;
    ///
    /// assert!(Candlestick::new().grid);
    /// ```
    pub fn new() -> Self {
        Self {
            candles: Vec::new(),
            y_range: None,
            grid: true,
            enabled: true,
            hovered: None,
            hovered_out: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            plot: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Appends a candle.
    ///
    /// ```
    /// use martensite::widgets::candlestick::{Candle, Candlestick};
    ///
    /// let c = Candlestick::new().candle(Candle::new(1.0, 2.0, 0.5, 1.5));
    /// assert_eq!(c.candle_count(), 1);
    /// ```
    pub fn candle(mut self, candle: Candle) -> Self {
        self.candles.push(candle);
        self
    }

    /// Replaces the candle list.
    ///
    /// ```
    /// use martensite::widgets::candlestick::{Candle, Candlestick};
    ///
    /// let c = Candlestick::new().candles([Candle::new(1.0, 2.0, 0.5, 1.5)]);
    /// assert_eq!(c.candle_count(), 1);
    /// ```
    pub fn candles(mut self, candles: impl IntoIterator<Item = Candle>) -> Self {
        self.candles = candles.into_iter().collect();
        self
    }

    /// Pins the y range.
    ///
    /// ```
    /// use martensite::widgets::candlestick::Candlestick;
    ///
    /// let c = Candlestick::new().y_range(0.0, 100.0);
    /// assert_eq!(c.y_range, Some((0.0, 100.0)));
    /// ```
    pub fn y_range(mut self, lo: f32, hi: f32) -> Self {
        self.y_range = Some((lo, hi));
        self
    }

    /// Toggles the grid.
    ///
    /// ```
    /// use martensite::widgets::candlestick::Candlestick;
    ///
    /// let c = Candlestick::new().grid(false);
    /// assert!(!c.grid);
    /// ```
    pub fn grid(mut self, show: bool) -> Self {
        self.grid = show;
        self
    }

    /// Enables or disables the chart.
    ///
    /// ```
    /// use martensite::widgets::candlestick::Candlestick;
    ///
    /// let c = Candlestick::new().enabled(false);
    /// assert!(!c.enabled);
    /// ```
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Candle count.
    ///
    /// ```
    /// use martensite::widgets::candlestick::Candlestick;
    ///
    /// assert_eq!(Candlestick::new().candle_count(), 0);
    /// ```
    pub fn candle_count(&self) -> usize {
        self.candles.len()
    }

    /// Drains the hovered candle index.
    ///
    /// ```
    /// use martensite::widgets::candlestick::Candlestick;
    ///
    /// let mut c = Candlestick::new();
    /// assert_eq!(c.take_hovered(), None);
    /// ```
    pub fn take_hovered(&mut self) -> Option<usize> {
        self.hovered_out.take()
    }

    /// Y range — pinned or fitted to `low..high` (`(0, 1)` empty).
    fn range(&self) -> (f32, f32) {
        self.y_range.unwrap_or_else(|| {
            let mut lo = f32::MAX;
            let mut hi = f32::MIN;
            for c in &self.candles {
                lo = lo.min(c.low);
                hi = hi.max(c.high);
            }
            if lo > hi || (hi - lo).abs() < f32::EPSILON {
                (0.0, 1.0)
            } else {
                (lo, hi)
            }
        })
    }

    /// Maps a price to plot y.
    fn map_y(&self, v: f32) -> f32 {
        let (lo, hi) = self.range();
        let f = (v - lo) / (hi - lo).max(f32::EPSILON);
        self.plot.max_y() - f * self.plot.height()
    }

    /// Slot width for candle `n` candles.
    fn slot_w(&self) -> f32 {
        self.plot.width() / self.candles.len().max(1) as f32
    }

    /// Candle index under `pos` (nearest slot center within hover).
    fn hit(&self, pos: Vec2) -> Option<usize> {
        if self.candles.is_empty() || !self.plot.contains(pos) {
            return None;
        }
        let i = ((pos.x - self.plot.min_x()) / self.slot_w().max(1.0)) as usize;
        let i = i.min(self.candles.len() - 1);
        // Reject far vertical misses so tooltips only arm on the bar.
        let c = &self.candles[i];
        if pos.y >= self.map_y(c.high) - HOVER_PT * self.scale
            && pos.y <= self.map_y(c.low) + HOVER_PT * self.scale
        {
            Some(i)
        } else {
            None
        }
    }
}

impl Widget for Candlestick {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            cx.pt(W_PT).min(constraints.max_size.x.max(0.0)),
            cx.pt(H_PT).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(80.0, 48.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        let pad = cx.pt(PAD_PT);
        self.plot = Rect::new(
            bounds.min_x() + pad,
            bounds.min_y() + pad,
            (bounds.width() - 2.0 * pad).max(0.0),
            (bounds.height() - 2.0 * pad).max(0.0),
        );
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Image);
        node.set_label("Candlestick chart");
        node.set_value(format!("{} candles", self.candles.len()));
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
                    self.hovered_out = h;
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
        let f = |r: Rect| {
            kurbo::Rect::new(
                f64::from(r.min_x()),
                f64::from(r.min_y()),
                f64::from(r.max_x()),
                f64::from(r.max_y()),
            )
        };
        cx.list.push_fill_shape(
            f(self.plot),
            &martensite_core::shape::Shape::rounded(0.0),
            cx.color(TokenKey::SurfaceColor, SURFACE),
        );
        let grid = cx.color(TokenKey::DividerColor, GRID);
        if self.grid {
            for i in 1..GRID_N {
                let fy = i as f32 / GRID_N as f32;
                let mut h = kurbo::BezPath::new();
                h.move_to((
                    f64::from(self.plot.min_x()),
                    f64::from(self.plot.min_y() + self.plot.height() * fy),
                ));
                h.line_to((
                    f64::from(self.plot.max_x()),
                    f64::from(self.plot.min_y() + self.plot.height() * fy),
                ));
                cx.list.push_stroke_path(h, cx.pt(0.5), grid);
            }
        }
        cx.list.push_stroke_shape(
            f(self.plot),
            &martensite_core::shape::Shape::rounded(0.0),
            cx.pt(0.75),
            cx.color(TokenKey::BorderColor, FRAME),
        );

        let slot = self.slot_w();
        let body_w = (slot * 0.6).max(1.0);
        for (i, c) in self.candles.iter().enumerate() {
            let cx_slot = self.plot.min_x() + (i as f32 + 0.5) * slot;
            let mut color = cx.color(
                if c.is_up() {
                    TokenKey::SuccessColor
                } else {
                    TokenKey::ErrorColor
                },
                if c.is_up() { UP } else { DOWN },
            );
            if self.hovered == Some(i) {
                color = [
                    color[0].saturating_add(30),
                    color[1].saturating_add(30),
                    color[2].saturating_add(30),
                    255,
                ];
            }
            // Wick: high–low vertical line.
            let mut wick = kurbo::BezPath::new();
            wick.move_to((f64::from(cx_slot), f64::from(self.map_y(c.high))));
            wick.line_to((f64::from(cx_slot), f64::from(self.map_y(c.low))));
            cx.list.push_stroke_path(wick, cx.pt(1.0), color);
            // Body: open–close rect (≥1px).
            let y_top = self.map_y(c.open.max(c.close));
            let y_bot = self.map_y(c.open.min(c.close));
            let body = Rect::new(
                cx_slot - body_w / 2.0,
                y_top,
                body_w,
                (y_bot - y_top).max(cx.pt(1.0)),
            );
            cx.list
                .push_fill_shape(f(body), &martensite_core::shape::Shape::rounded(0.0), color);
        }
    }
}

impl std::fmt::Debug for Candlestick {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Candlestick")
            .field("candles", &self.candles.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(c: &mut Candlestick, w: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        c.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(w, h),
            },
        );
        c.layout(&mut cx, Rect::new(0.0, 0.0, w, h));
    }

    #[test]
    fn range_fits_extrema() {
        let c = Candlestick::new().candles([
            Candle::new(10.0, 15.0, 8.0, 12.0),
            Candle::new(12.0, 20.0, 11.0, 18.0),
        ]);
        assert_eq!(c.range(), (8.0, 20.0));
        let pinned = Candlestick::new().y_range(0.0, 50.0);
        assert_eq!(pinned.range(), (0.0, 50.0));
    }

    #[test]
    fn is_up_semantics() {
        assert!(Candle::new(1.0, 2.0, 0.5, 2.0).is_up());
        assert!(Candle::new(1.0, 2.0, 0.5, 1.0).is_up()); // doji counts up
        assert!(!Candle::new(2.0, 2.5, 0.5, 1.0).is_up());
    }

    #[test]
    fn map_y_inverts() {
        let mut c = Candlestick::new().y_range(0.0, 10.0);
        laid_out(&mut c, 216.0, 216.0);
        assert!((c.map_y(0.0) - c.plot.max_y()).abs() < 0.01);
        assert!((c.map_y(10.0) - c.plot.min_y()).abs() < 0.01);
    }

    #[test]
    fn hover_parks_index() {
        let mut c = Candlestick::new().candles([
            Candle::new(0.0, 10.0, 0.0, 10.0),
            Candle::new(0.0, 10.0, 0.0, 10.0),
        ]);
        laid_out(&mut c, 216.0, 216.0);
        // Second slot center, mid-range.
        let x = c.plot.min_x() + 1.5 * c.slot_w();
        let y = c.map_y(5.0);
        c.event(&mut EventContext {
            event: &WidgetEvent::PointerMoved {
                position: Vec2::new(x, y),
            },
            bounds: Rect::new(0.0, 0.0, 216.0, 216.0),
            scale: 1.0,
        });
        assert_eq!(c.take_hovered(), Some(1));
        assert_eq!(c.take_hovered(), None);
    }

    #[test]
    fn hover_rejects_outside_plot() {
        let mut c = Candlestick::new().candles([Candle::new(0.0, 10.0, 0.0, 10.0)]);
        laid_out(&mut c, 216.0, 216.0);
        c.event(&mut EventContext {
            event: &WidgetEvent::PointerMoved {
                position: Vec2::new(1.0, 1.0),
            },
            bounds: Rect::new(0.0, 0.0, 216.0, 216.0),
            scale: 1.0,
        });
        assert_eq!(c.take_hovered(), None);
    }
}
