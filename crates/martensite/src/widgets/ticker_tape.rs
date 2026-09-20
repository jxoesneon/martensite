//! `TickerTape` — a horizontally scrolling strip of structured
//! market/news items (Bloomberg/AP ticker idiom).
//!
//! Distinct from [`Marquee`](crate::widgets::Marquee): items are
//! structured — a symbol, a price, and a signed delta colored
//! gain/loss — and clicks park the item index in
//! [`TickerTape::take_selected`]. `tick` advances the scroll;
//! hover pauses it. The strip wraps seamlessly.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::ticker_tape::{TickerItem, TickerTape};
//!
//! let t = TickerTape::new()
//!     .item(TickerItem::new("AAPL", "189.30", 0.012))
//!     .item(TickerItem::new("MSFT", "415.02", -0.004));
//! assert_eq!(t.item_count(), 2);
//! ```

use std::time::Duration;

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::text_paint::SharedTextPainter;

const FONT_PT: f32 = 12.0;
const PAD_V_PT: f32 = 7.0;
const ITEM_GAP_PT: f32 = 28.0;
const SPEED_PT_S: f32 = 48.0;

const UP: [u8; 4] = [46, 160, 67, 255];
const DOWN: [u8; 4] = [218, 54, 51, 255];
const TEXT: [u8; 4] = [220, 222, 228, 255];
const MUTED: [u8; 4] = [139, 148, 158, 255];

/// One strip entry — a label, a value, and a signed change.
///
/// ```
/// use martensite::widgets::ticker_tape::TickerItem;
///
/// let i = TickerItem::new("MSFT", "415.02", -0.004);
/// assert_eq!(i.delta, -0.004);
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct TickerItem {
    /// Symbol or headline label.
    pub symbol: String,
    /// Price or value text.
    pub price: String,
    /// Signed fractional change; colored gain/loss, `▲`/`▼` marked.
    pub delta: f32,
}

impl TickerItem {
    /// New strip entry.
    ///
    /// ```
    /// use martensite::widgets::ticker_tape::TickerItem;
    ///
    /// assert_eq!(TickerItem::new("A", "1", 0.5).symbol, "A");
    /// ```
    pub fn new(symbol: impl Into<String>, price: impl Into<String>, delta: f32) -> Self {
        Self {
            symbol: symbol.into(),
            price: price.into(),
            delta,
        }
    }
}

/// A scrolling structured ticker — see the module docs.
///
/// ```
/// use martensite::widgets::ticker_tape::TickerTape;
///
/// assert_eq!(TickerTape::new().item_count(), 0);
/// ```
pub struct TickerTape {
    /// Accessibility label.
    pub label: String,
    /// Scroll speed in points per second.
    pub speed: f32,
    /// Pause scrolling while hovered.
    pub pause_on_hover: bool,
    items: Vec<TickerItem>,
    widths: Vec<f32>,
    gap: f32,
    offset: f32,
    hovered: bool,
    selected: Option<usize>,
    text_painter: Option<SharedTextPainter>,
    bounds: Rect,
    scale: f32,
}

impl std::fmt::Debug for TickerTape {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TickerTape")
            .field("items", &self.items.len())
            .field("offset", &self.offset)
            .finish()
    }
}

impl Default for TickerTape {
    fn default() -> Self {
        Self::new()
    }
}

impl TickerTape {
    /// Empty ticker at the default speed.
    ///
    /// ```
    /// use martensite::widgets::ticker_tape::TickerTape;
    ///
    /// assert_eq!(TickerTape::new().item_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Ticker".to_string(),
            speed: SPEED_PT_S,
            pause_on_hover: true,
            items: Vec::new(),
            widths: Vec::new(),
            gap: ITEM_GAP_PT,
            offset: 0.0,
            hovered: false,
            selected: None,
            text_painter: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Appends an item.
    ///
    /// ```
    /// use martensite::widgets::ticker_tape::{TickerItem, TickerTape};
    ///
    /// assert_eq!(
    ///     TickerTape::new().item(TickerItem::new("X", "1", 0.0)).item_count(),
    ///     1
    /// );
    /// ```
    pub fn item(mut self, item: TickerItem) -> Self {
        self.items.push(item);
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::ticker_tape::TickerTape;
    ///
    /// assert_eq!(TickerTape::new().label("Market").label, "Market");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Scroll speed builder.
    ///
    /// ```
    /// use martensite::widgets::ticker_tape::TickerTape;
    ///
    /// assert_eq!(TickerTape::new().speed(80.0).speed, 80.0);
    /// ```
    pub fn speed(mut self, speed: f32) -> Self {
        self.speed = speed.max(0.0);
        self
    }

    /// Shared text painter for real glyph metrics.
    ///
    /// ```no_run
    /// use martensite::widgets::ticker_tape::TickerTape;
    /// # fn example(p: martensite::text_paint::SharedTextPainter) {
    /// let _t = TickerTape::new().with_text_painter(p);
    /// # }
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Replaces the item list and resets the scroll.
    ///
    /// ```
    /// use martensite::widgets::ticker_tape::{TickerItem, TickerTape};
    ///
    /// let mut t = TickerTape::new();
    /// t.set_items(vec![TickerItem::new("X", "1", 0.0)]);
    /// assert_eq!(t.item_count(), 1);
    /// ```
    pub fn set_items(&mut self, items: Vec<TickerItem>) {
        self.items = items;
        self.offset = 0.0;
    }

    /// Number of items.
    ///
    /// ```
    /// use martensite::widgets::ticker_tape::TickerTape;
    ///
    /// assert_eq!(TickerTape::new().item_count(), 0);
    /// ```
    pub fn item_count(&self) -> usize {
        self.items.len()
    }

    /// Current scroll offset in points.
    ///
    /// ```
    /// use martensite::widgets::ticker_tape::TickerTape;
    ///
    /// assert_eq!(TickerTape::new().offset(), 0.0);
    /// ```
    pub fn offset(&self) -> f32 {
        self.offset
    }

    /// Whether the strip is currently paused by hover.
    ///
    /// ```
    /// use martensite::widgets::ticker_tape::TickerTape;
    ///
    /// assert!(!TickerTape::new().is_paused());
    /// ```
    pub fn is_paused(&self) -> bool {
        self.hovered && self.pause_on_hover
    }

    /// Drains the last clicked item index.
    ///
    /// ```
    /// use martensite::widgets::ticker_tape::TickerTape;
    ///
    /// assert_eq!(TickerTape::new().take_selected(), None);
    /// ```
    pub fn take_selected(&mut self) -> Option<usize> {
        self.selected.take()
    }

    /// Full width of one wrap cycle (items + gaps).
    fn cycle_width(&self) -> f32 {
        if self.items.is_empty() {
            return 0.0;
        }
        self.widths.iter().sum::<f32>() + self.gap * self.items.len() as f32
    }

    /// Measures one item's row width.
    fn item_width(&self, item: &TickerItem) -> f32 {
        let s = self.scale;
        if let Some(p) = &self.text_painter {
            let size = FONT_PT * s;
            let w = [
                item.symbol.clone(),
                item.price.clone(),
                format!("{}{:.1}%", arrow(item.delta), item.delta * 100.0),
            ]
            .iter()
            .map(|t| p.measure(t, size))
            .sum::<f32>()
                + 16.0 * s;
            return w;
        }
        // Fallback estimate without a painter.
        (item.symbol.len() + item.price.len() + 6) as f32 * 7.0 * s + 16.0 * s
    }

    /// Maps a local x to an item index via the wrap cycle.
    fn item_at(&self, x: f32) -> Option<usize> {
        let cycle = self.cycle_width();
        if cycle <= 0.0 {
            return None;
        }
        let mut local = (x + self.offset).rem_euclid(cycle);
        for (i, w) in self.widths.iter().enumerate() {
            if local < *w {
                return Some(i);
            }
            local -= w + self.gap;
            if local < 0.0 {
                return None; // inside the gap
            }
        }
        None
    }
}

/// Gain/loss marker for a delta.
fn arrow(delta: f32) -> &'static str {
    if delta > 0.0 {
        "▲ "
    } else if delta < 0.0 {
        "▼ "
    } else {
        ""
    }
}

impl Widget for TickerTape {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let h = (FONT_PT + PAD_V_PT * 2.0) * cx.scale;
        Vec2::new(constraints.max_size.x.max(80.0 * cx.scale), h)
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(80.0, 20.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        self.gap = ITEM_GAP_PT * cx.scale;
        self.widths = self.items.iter().map(|i| self.item_width(i)).collect();
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Marquee);
        node.set_label(self.label.clone());
        let summary = self
            .items
            .iter()
            .map(|i| format!("{} {}", i.symbol, i.price))
            .collect::<Vec<_>>()
            .join(", ");
        node.set_value(summary);
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerMoved { position } => {
                if self.bounds.contains(*position) != self.hovered {
                    self.hovered = !self.hovered;
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerLeave => {
                if self.hovered {
                    self.hovered = false;
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position,
            } => {
                if self.bounds.contains(*position) {
                    if let Some(i) = self.item_at(position.x - self.bounds.min_x()) {
                        self.selected = Some(i);
                        return EventResponse::Handled;
                    }
                }
                EventResponse::Ignored
            }
            _ => EventResponse::Ignored,
        }
    }

    fn tick(&mut self, dt: Duration) -> bool {
        let cycle = self.cycle_width();
        if cycle <= 0.0 || self.is_paused() {
            return false;
        }
        self.offset = (self.offset + self.speed * self.scale * dt.as_secs_f32()).rem_euclid(cycle);
        true
    }

    fn paint(&self, cx: &mut PaintContext) {
        if self.items.is_empty() {
            return;
        }
        let s = cx.scale;
        let cycle = self.cycle_width();
        let clip = kurbo::Rect::new(
            f64::from(self.bounds.min_x()),
            f64::from(self.bounds.min_y()),
            f64::from(self.bounds.max_x()),
            f64::from(self.bounds.max_y()),
        );
        cx.list.push_clip(clip);
        let y = self.bounds.min_y() + self.bounds.height() / 2.0;
        let size = FONT_PT * s;
        // Paint two cycles so the wrap seam never shows a gap.
        for rep in 0..2 {
            let mut x = self.bounds.min_x() - self.offset + cycle * rep as f32;
            for (i, item) in self.items.iter().enumerate() {
                if x > self.bounds.max_x() {
                    break;
                }
                let w = self.widths.get(i).copied().unwrap_or(0.0);
                if x + w > self.bounds.min_x() {
                    let up_down = cx.color(
                        if item.delta >= 0.0 {
                            TokenKey::SuccessColor
                        } else {
                            TokenKey::ErrorColor
                        },
                        if item.delta >= 0.0 { UP } else { DOWN },
                    );
                    let text = cx.color(TokenKey::TextColor, TEXT);
                    let muted = cx.color(TokenKey::TextMutedColor, MUTED);
                    let painter =
                        crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
                    if let Some(p) = painter {
                        let mut pen = x + 6.0 * s;
                        let mut put =
                            |t: &str, c: [u8; 4], list: &mut martensite_core::PaintList| {
                                let pt = kurbo::Point::new(f64::from(pen), f64::from(y));
                                crate::text_paint::paint_label(Some(p), list, pt, t, size, c);
                                pen += p.measure_text(t, size).unwrap_or(0.0) + 6.0 * s;
                            };
                        put(&item.symbol, muted, cx.list);
                        put(&item.price, text, cx.list);
                        put(
                            &format!("{}{:+.1}%", arrow(item.delta), item.delta * 100.0),
                            up_down,
                            cx.list,
                        );
                    } else {
                        // Painterless fallback: symbol bar + delta tick.
                        let bar = kurbo::Rect::new(
                            f64::from(x),
                            f64::from(y - 3.0 * s),
                            f64::from(x + w),
                            f64::from(y + 3.0 * s),
                        );
                        cx.list.push_fill_rect(bar, muted);
                        let dot = kurbo::Rect::new(
                            f64::from(x + w - 8.0 * s),
                            f64::from(y - 3.0 * s),
                            f64::from(x + w - 2.0 * s),
                            f64::from(y + 3.0 * s),
                        );
                        cx.list.push_fill_shape(
                            dot,
                            &martensite_core::shape::Shape::ELLIPSE,
                            up_down,
                        );
                    }
                }
                x += w + self.gap;
            }
        }
        cx.list.pop_clip();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(t: &mut TickerTape) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        t.layout(&mut cx, Rect::new(0.0, 0.0, 400.0, 26.0));
    }

    fn ev(t: &mut TickerTape, e: &WidgetEvent) -> EventResponse {
        t.event(&mut EventContext {
            event: e,
            bounds: t.bounds,
            scale: 1.0,
        })
    }

    fn fixture() -> TickerTape {
        TickerTape::new()
            .item(TickerItem::new("AA", "10.0", 0.01))
            .item(TickerItem::new("BB", "20.0", -0.02))
    }

    #[test]
    fn tick_advances_and_wraps() {
        let mut t = fixture();
        laid_out(&mut t);
        assert!(t.tick(Duration::from_millis(500)));
        assert!(t.offset() > 0.0);
        // Force near-cycle and confirm wrap.
        t.offset = t.cycle_width() - 1.0;
        t.tick(Duration::from_secs(1));
        assert!(t.offset() < t.cycle_width());
    }

    #[test]
    fn hover_pauses() {
        let mut t = fixture();
        laid_out(&mut t);
        ev(
            &mut t,
            &WidgetEvent::PointerMoved {
                position: Vec2::new(10.0, 10.0),
            },
        );
        assert!(t.is_paused());
        let off = t.offset;
        assert!(!t.tick(Duration::from_millis(100)));
        assert_eq!(t.offset, off);
        ev(&mut t, &WidgetEvent::PointerLeave);
        assert!(!t.is_paused());
    }

    #[test]
    fn click_parks_index() {
        let mut t = fixture();
        laid_out(&mut t);
        // Click inside the first item's span.
        ev(
            &mut t,
            &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: Vec2::new(4.0, 13.0),
            },
        );
        assert_eq!(t.take_selected(), Some(0));
        // Gap clicks ignore.
        let gap_x = t.widths[0] + t.gap / 2.0;
        ev(
            &mut t,
            &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: Vec2::new(gap_x, 13.0),
            },
        );
        assert_eq!(t.take_selected(), None);
    }

    #[test]
    fn click_wraps_to_cycle() {
        let mut t = fixture();
        laid_out(&mut t);
        // Simulate a scrolled offset: clicking x lands on a later item.
        t.offset = t.widths[0] + t.gap + 2.0;
        ev(
            &mut t,
            &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: Vec2::new(4.0, 13.0),
            },
        );
        assert_eq!(t.take_selected(), Some(1));
    }

    #[test]
    fn paint_without_painter() {
        let mut t = fixture();
        laid_out(&mut t);
        let mut list = martensite_core::PaintList::default();
        let theme = martensite_theme::Theme::new("test");
        t.paint(&mut PaintContext {
            list: &mut list,
            bounds: t.bounds,
            theme: &theme,
            scale: 1.0,
            text_painter: None,
        });
        assert!(!list.is_empty());
    }
}
