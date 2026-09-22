//! `TaskSwitcher` — the Alt-Tab app switcher strip: a horizontal
//! row of [`Thumbnail`] tiles with a focus ring.
//!
//! Arrows, scroll, or repeated `Tab` presses (via
//! [`TaskSwitcher::cycle`]) move the ring; `Enter`, `Space`, or a
//! tile click parks the index in
//! [`TaskSwitcher::take_selected`]; `Escape` parks
//! [`TaskSwitcher::take_cancelled`]. The host decides when the
//! switcher is visible (typically while a modifier is held).
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::task_switcher::TaskSwitcher;
//! use martensite::widgets::Thumbnail;
//!
//! let t = TaskSwitcher::new()
//!     .item(Thumbnail::new("editor", [80; 4]))
//!     .item(Thumbnail::new("terminal", [90; 4]));
//! assert_eq!(t.item_count(), 2);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::text_paint::SharedTextPainter;
use crate::widgets::Thumbnail;

const TILE_PT: f32 = 64.0;
const GAP_PT: f32 = 10.0;
const PAD_PT: f32 = 14.0;
const LABEL_PT: f32 = 11.0;
const RING_PT: f32 = 3.0;

const FACE: [u8; 4] = [38, 40, 48, 230];
const RING: [u8; 4] = [110, 140, 230, 255];
const TEXT: [u8; 4] = [235, 237, 240, 255];

/// The switcher — see the module docs.
///
/// ```
/// use martensite::widgets::task_switcher::TaskSwitcher;
///
/// assert_eq!(TaskSwitcher::new().item_count(), 0);
/// ```
pub struct TaskSwitcher {
    /// Accessibility label.
    pub label: String,
    items: Vec<Thumbnail>,
    index: usize,
    selected: Option<usize>,
    cancelled: bool,
    tiles: Vec<Rect>,
    text_painter: Option<SharedTextPainter>,
    bounds: Rect,
    scale: f32,
}

impl std::fmt::Debug for TaskSwitcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TaskSwitcher")
            .field("items", &self.items.len())
            .field("index", &self.index)
            .finish()
    }
}

impl Default for TaskSwitcher {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskSwitcher {
    /// Empty switcher.
    ///
    /// ```
    /// use martensite::widgets::task_switcher::TaskSwitcher;
    ///
    /// assert_eq!(TaskSwitcher::new().item_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Task switcher".to_string(),
            items: Vec::new(),
            index: 0,
            selected: None,
            cancelled: false,
            tiles: Vec::new(),
            text_painter: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Appends a tile.
    ///
    /// ```
    /// use martensite::widgets::task_switcher::TaskSwitcher;
    /// use martensite::widgets::Thumbnail;
    ///
    /// assert_eq!(TaskSwitcher::new().item(Thumbnail::new("a", [1; 4])).item_count(), 1);
    /// ```
    pub fn item(mut self, item: Thumbnail) -> Self {
        self.items.push(item);
        self
    }

    /// Initial index.
    ///
    /// ```
    /// use martensite::widgets::task_switcher::TaskSwitcher;
    /// use martensite::widgets::Thumbnail;
    ///
    /// assert_eq!(TaskSwitcher::new().item(Thumbnail::new("a", [1; 4])).index(0).current(), Some(0));
    /// ```
    pub fn index(mut self, index: usize) -> Self {
        self.index = index;
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::task_switcher::TaskSwitcher;
    ///
    /// assert_eq!(TaskSwitcher::new().label("Apps").label, "Apps");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Shared text painter for labels.
    ///
    /// ```no_run
    /// use martensite::widgets::task_switcher::TaskSwitcher;
    /// # fn example(p: martensite::text_paint::SharedTextPainter) {
    /// let _t = TaskSwitcher::new().with_text_painter(p);
    /// # }
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Tile count.
    ///
    /// ```
    /// use martensite::widgets::task_switcher::TaskSwitcher;
    ///
    /// assert_eq!(TaskSwitcher::new().item_count(), 0);
    /// ```
    pub fn item_count(&self) -> usize {
        self.items.len()
    }

    /// Focused index.
    ///
    /// ```
    /// use martensite::widgets::task_switcher::TaskSwitcher;
    ///
    /// assert_eq!(TaskSwitcher::new().current(), None);
    /// ```
    pub fn current(&self) -> Option<usize> {
        (!self.items.is_empty()).then(|| self.index.min(self.items.len() - 1))
    }

    /// Sets the focus index (host-driven).
    ///
    /// ```
    /// use martensite::widgets::task_switcher::TaskSwitcher;
    /// use martensite::widgets::Thumbnail;
    ///
    /// let mut t = TaskSwitcher::new().item(Thumbnail::new("a", [1; 4]));
    /// t.set_index(0);
    /// assert_eq!(t.current(), Some(0));
    /// ```
    pub fn set_index(&mut self, index: usize) {
        if !self.items.is_empty() {
            self.index = index.min(self.items.len() - 1);
        }
    }

    /// Cycles the ring by `d` tiles, wrapping — the Alt-Tab
    /// "press Tab again" gesture.
    ///
    /// ```
    /// use martensite::widgets::task_switcher::TaskSwitcher;
    /// use martensite::widgets::Thumbnail;
    ///
    /// let mut t = TaskSwitcher::new()
    ///     .item(Thumbnail::new("a", [1; 4]))
    ///     .item(Thumbnail::new("b", [1; 4]));
    /// t.cycle(1);
    /// assert_eq!(t.current(), Some(1));
    /// t.cycle(1);
    /// assert_eq!(t.current(), Some(0));
    /// ```
    pub fn cycle(&mut self, d: isize) {
        if self.items.is_empty() {
            return;
        }
        let n = self.items.len() as isize;
        self.index = (((self.index as isize + d) % n) + n) as usize % self.items.len();
    }

    /// Drains a commit (click / Enter / Space).
    ///
    /// ```
    /// use martensite::widgets::task_switcher::TaskSwitcher;
    ///
    /// let mut t = TaskSwitcher::new();
    /// assert_eq!(t.take_selected(), None);
    /// ```
    pub fn take_selected(&mut self) -> Option<usize> {
        self.selected.take()
    }

    /// Drains a cancel (Escape).
    ///
    /// ```
    /// use martensite::widgets::task_switcher::TaskSwitcher;
    ///
    /// let mut t = TaskSwitcher::new();
    /// assert!(!t.take_cancelled());
    /// ```
    pub fn take_cancelled(&mut self) -> bool {
        std::mem::take(&mut self.cancelled)
    }
}

impl Widget for TaskSwitcher {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let s = cx.scale;
        let n = self.items.len().max(1) as f32;
        let w = n * (TILE_PT + GAP_PT) * s + PAD_PT * 2.0 * s;
        let h = (TILE_PT + PAD_PT * 2.0 + LABEL_PT) * s;
        Vec2::new(
            w.min(constraints.max_size.x.max(0.0)),
            h.min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(140.0, 90.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        let s = cx.scale;
        let tile = TILE_PT * s;
        let gap = GAP_PT * s;
        let n = self.items.len();
        let strip_w = n as f32 * (tile + gap) - if n > 0 { gap } else { 0.0 };
        let mut x = bounds.min_x() + (bounds.width() - strip_w).max(0.0) / 2.0;
        let y = bounds.min_y() + (bounds.height() - tile - LABEL_PT * s).max(0.0) / 2.0;
        self.tiles.clear();
        for _ in &self.items {
            self.tiles.push(Rect::new(x, y, tile, tile));
            x += tile + gap;
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::ListBox);
        node.set_label(self.label.clone());
        if let Some(i) = self.current() {
            node.set_value(format!("{} of {}", i + 1, self.items.len()));
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::KeyPressed { key, .. } => match key.as_str() {
                "ArrowLeft" | "ArrowUp" => {
                    self.cycle(-1);
                    EventResponse::RequestRepaint
                }
                "ArrowRight" | "ArrowDown" | "Tab" => {
                    self.cycle(1);
                    EventResponse::RequestRepaint
                }
                "Enter" | " " => {
                    if let Some(i) = self.current() {
                        self.selected = Some(i);
                        return EventResponse::Handled;
                    }
                    EventResponse::Ignored
                }
                "Escape" => {
                    self.cancelled = true;
                    EventResponse::Handled
                }
                _ => EventResponse::Ignored,
            },
            WidgetEvent::Scroll { delta, .. } => {
                self.cycle(if delta.x + delta.y > 0.0 { 1 } else { -1 });
                EventResponse::RequestRepaint
            }
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position,
            } => {
                if let Some(i) = self.tiles.iter().position(|r| r.contains(*position)) {
                    self.index = i;
                    self.selected = Some(i);
                    return EventResponse::Handled;
                }
                EventResponse::Ignored
            }
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let s = cx.scale;
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let krect = |r: Rect| {
            kurbo::Rect::new(
                f64::from(r.min_x()),
                f64::from(r.min_y()),
                f64::from(r.max_x()),
                f64::from(r.max_y()),
            )
        };
        // Panel.
        cx.list.push_fill_rect(krect(self.bounds), FACE);
        for (i, tile) in self.tiles.iter().enumerate() {
            let item = &self.items[i];
            cx.list.push_fill_rect(krect(*tile), item.color);
            if Some(i) == self.current() {
                let ring = RING_PT * s;
                cx.list.push_stroke_rect(
                    kurbo::Rect::new(
                        f64::from(tile.min_x() - ring),
                        f64::from(tile.min_y() - ring),
                        f64::from(tile.max_x() + ring),
                        f64::from(tile.max_y() + ring),
                    ),
                    ring.max(1.0),
                    cx.color(TokenKey::AccentColor, RING),
                );
                // Label under the focused tile — clamped into the
                // widget so edge tiles' captions slide inward rather
                // than spilling past the panel.
                let label = &item.label;
                let lw = painter
                    .and_then(|p| p.measure_text(label, LABEL_PT * s))
                    .unwrap_or(label.len() as f32 * LABEL_PT * 0.6 * s);
                let lx = (tile.min_x() + (tile.width() - lw) / 2.0).clamp(
                    self.bounds.min_x(),
                    (self.bounds.max_x() - lw).max(self.bounds.min_x()),
                );
                crate::text_paint::paint_label_clipped(
                    painter,
                    cx.list,
                    krect(self.bounds),
                    kurbo::Point::new(f64::from(lx), f64::from(tile.max_y() + LABEL_PT * s)),
                    label,
                    LABEL_PT * s,
                    cx.color(TokenKey::TextColor, TEXT),
                );
            } else {
                // Dim unfocused tiles slightly.
                cx.list.push_fill_rect(krect(*tile), [0, 0, 0, 60]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn fixture() -> TaskSwitcher {
        TaskSwitcher::new()
            .item(Thumbnail::new("a", [255, 0, 0, 255]))
            .item(Thumbnail::new("b", [0, 255, 0, 255]))
            .item(Thumbnail::new("c", [0, 0, 255, 255]))
    }

    fn laid_out(t: &mut TaskSwitcher) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        t.layout(&mut cx, Rect::new(0.0, 0.0, 400.0, 120.0));
    }

    fn ev(t: &mut TaskSwitcher, e: &WidgetEvent) -> EventResponse {
        t.event(&mut EventContext {
            event: e,
            bounds: t.bounds,
            scale: 1.0,
        })
    }

    #[test]
    fn cycle_wraps() {
        let mut t = fixture();
        t.cycle(-1);
        assert_eq!(t.current(), Some(2));
        t.cycle(1);
        assert_eq!(t.current(), Some(0));
    }

    #[test]
    fn enter_commits_focused() {
        let mut t = fixture();
        laid_out(&mut t);
        ev(
            &mut t,
            &WidgetEvent::KeyPressed {
                key: "ArrowRight".to_string(),
                repeat: false,
            },
        );
        ev(
            &mut t,
            &WidgetEvent::KeyPressed {
                key: "Enter".to_string(),
                repeat: false,
            },
        );
        assert_eq!(t.take_selected(), Some(1));
    }

    #[test]
    fn escape_cancels() {
        let mut t = fixture();
        laid_out(&mut t);
        ev(
            &mut t,
            &WidgetEvent::KeyPressed {
                key: "Escape".to_string(),
                repeat: false,
            },
        );
        assert!(t.take_cancelled());
    }

    #[test]
    fn click_commits_tile() {
        let mut t = fixture();
        laid_out(&mut t);
        let r = t.tiles[2];
        ev(
            &mut t,
            &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: Vec2::new((r.min_x() + r.max_x()) / 2.0, (r.min_y() + r.max_y()) / 2.0),
            },
        );
        assert_eq!(t.take_selected(), Some(2));
    }

    #[test]
    fn scroll_cycles() {
        let mut t = fixture();
        laid_out(&mut t);
        ev(
            &mut t,
            &WidgetEvent::Scroll {
                delta: Vec2::new(0.0, 10.0),
                position: Vec2::new(100.0, 60.0),
            },
        );
        assert_eq!(t.current(), Some(1));
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
