//! `Kanban` — a card board (columns of cards dragged between
//! lanes — the Trello / Ant-board / Azure-DevOps-board idiom).
//!
//! Columns are equal-width lanes; cards stack from the top.
//! Dragging a card lifts it and shows a drop slot in the lane
//! under the pointer; releasing drops it there (the board
//! moves the card itself) and parks `(from_col, card_index,
//! to_col)` in [`Kanban::take_moved`]. A plain click without
//! a column change parks the card index in
//! [`Kanban::take_selected`].
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::kanban::Kanban;
//!
//! let k = Kanban::new()
//!     .column("Todo")
//!     .card("Todo", "Task A")
//!     .column("Done");
//! assert_eq!(k.column_count(), 2);
//! assert_eq!(k.card_count(0), 1);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

const WIDTH_PT: f32 = 320.0;
const HEIGHT_PT: f32 = 180.0;
const PAD_PT: f32 = 6.0;
const GAP_PT: f32 = 8.0;
const HEAD_PT: f32 = 24.0;
const CARD_PT: f32 = 28.0;
const CARD_GAP_PT: f32 = 6.0;

const FACE: [u8; 4] = [36, 36, 42, 255];
const EDGE: [u8; 4] = [70, 70, 76, 255];
const LANE: [u8; 4] = [42, 42, 48, 255];
const CARD: [u8; 4] = [58, 58, 66, 255];
const CARD_HI: [u8; 4] = [74, 90, 130, 255];
const SLOT: [u8; 4] = [80, 130, 200, 120];
const GLYPH: [u8; 4] = [220, 220, 228, 255];
const MUTED: [u8; 4] = [150, 150, 160, 255];

#[derive(Debug)]
struct Col {
    name: String,
    cards: Vec<String>,
}

/// A drag in flight: source column, card index, pointer.
#[derive(Debug)]
struct Drag {
    from_col: usize,
    card: usize,
    pos: Vec2,
    /// Column the pointer is over.
    over_col: usize,
    /// Insert slot within `over_col`.
    slot: usize,
}

/// A columns-of-cards board — see the module docs.
///
/// ```
/// use martensite::widgets::kanban::Kanban;
///
/// assert_eq!(Kanban::new().column_count(), 0);
/// ```
#[derive(Debug)]
pub struct Kanban {
    /// Accessibility label.
    pub label: String,
    cols: Vec<Col>,
    drag: Option<Drag>,
    pressed: Option<(usize, usize, Vec2)>,
    moved: Option<(usize, usize, usize)>,
    selected: Option<usize>,
    bounds: Rect,
    scale: f32,
}

impl Default for Kanban {
    fn default() -> Self {
        Self::new()
    }
}

impl Kanban {
    /// Creates an empty board.
    ///
    /// ```
    /// use martensite::widgets::kanban::Kanban;
    ///
    /// assert_eq!(Kanban::new().column_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Kanban".to_string(),
            cols: Vec::new(),
            drag: None,
            pressed: None,
            moved: None,
            selected: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Appends a column.
    ///
    /// ```
    /// use martensite::widgets::kanban::Kanban;
    ///
    /// assert_eq!(Kanban::new().column("Todo").column_count(), 1);
    /// ```
    pub fn column(mut self, name: impl Into<String>) -> Self {
        self.cols.push(Col {
            name: name.into(),
            cards: Vec::new(),
        });
        self
    }

    /// Appends a card to a named column (unknown columns ignored).
    ///
    /// ```
    /// use martensite::widgets::kanban::Kanban;
    ///
    /// let k = Kanban::new().column("c").card("c", "t");
    /// assert_eq!(k.card_count(0), 1);
    /// ```
    pub fn card(mut self, column: &str, title: impl Into<String>) -> Self {
        if let Some(c) = self.cols.iter_mut().find(|c| c.name == column) {
            c.cards.push(title.into());
        }
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::kanban::Kanban;
    ///
    /// assert_eq!(Kanban::new().label("Sprint").label, "Sprint");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Column count.
    ///
    /// ```
    /// use martensite::widgets::kanban::Kanban;
    ///
    /// assert_eq!(Kanban::new().column("a").column("b").column_count(), 2);
    /// ```
    pub fn column_count(&self) -> usize {
        self.cols.len()
    }

    /// Card count in a column.
    ///
    /// ```
    /// use martensite::widgets::kanban::Kanban;
    ///
    /// let k = Kanban::new().column("c").card("c", "x").card("c", "y");
    /// assert_eq!(k.card_count(0), 2);
    /// ```
    pub fn card_count(&self, column: usize) -> usize {
        self.cols.get(column).map(|c| c.cards.len()).unwrap_or(0)
    }

    /// A column's name.
    ///
    /// ```
    /// use martensite::widgets::kanban::Kanban;
    ///
    /// assert_eq!(Kanban::new().column("Todo").column_name(0), "Todo");
    /// ```
    pub fn column_name(&self, index: usize) -> &str {
        self.cols.get(index).map(|c| c.name.as_str()).unwrap_or("")
    }

    /// A card's title.
    ///
    /// ```
    /// use martensite::widgets::kanban::Kanban;
    ///
    /// let k = Kanban::new().column("c").card("c", "Task");
    /// assert_eq!(k.card_title(0, 0), "Task");
    /// ```
    pub fn card_title(&self, column: usize, card: usize) -> &str {
        self.cols
            .get(column)
            .and_then(|c| c.cards.get(card))
            .map(String::as_str)
            .unwrap_or("")
    }

    /// Whether a drag is in flight.
    ///
    /// ```
    /// use martensite::widgets::kanban::Kanban;
    ///
    /// assert!(!Kanban::new().is_dragging());
    /// ```
    pub fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// Drains `(from_col, card_index, to_col)` after a move.
    ///
    /// ```
    /// use martensite::widgets::kanban::Kanban;
    ///
    /// assert_eq!(Kanban::new().take_moved(), None);
    /// ```
    pub fn take_moved(&mut self) -> Option<(usize, usize, usize)> {
        self.moved.take()
    }

    /// Drains the flat card index of the last plain click.
    ///
    /// ```
    /// use martensite::widgets::kanban::Kanban;
    ///
    /// assert_eq!(Kanban::new().take_selected(), None);
    /// ```
    pub fn take_selected(&mut self) -> Option<usize> {
        self.selected.take()
    }

    /// Column geometry: origin-x and width of each lane.
    fn lanes(&self) -> Vec<(f32, f32)> {
        let n = self.cols.len();
        if n == 0 {
            return Vec::new();
        }
        let pad = PAD_PT * self.scale;
        let gap = GAP_PT * self.scale;
        let w = (self.bounds.width() - 2.0 * pad - gap * (n - 1) as f32) / n as f32;
        (0..n)
            .map(|i| (self.bounds.min_x() + pad + i as f32 * (w + gap), w))
            .collect()
    }

    /// Card rect inside a lane.
    fn card_rect(&self, lane_x: f32, lane_w: f32, card: usize) -> Rect {
        let pad = PAD_PT * self.scale;
        let head = HEAD_PT * self.scale;
        let ch = CARD_PT * self.scale;
        let cg = CARD_GAP_PT * self.scale;
        Rect::new(
            lane_x + pad / 2.0,
            self.bounds.min_y() + pad + head + card as f32 * (ch + cg),
            lane_w - pad,
            ch,
        )
    }

    /// `(column, card)` under a point.
    fn card_at(&self, p: Vec2) -> Option<(usize, usize)> {
        for (i, (x, w)) in self.lanes().iter().enumerate() {
            for j in 0..self.cols[i].cards.len() {
                if self.card_rect(*x, *w, j).contains(p) {
                    return Some((i, j));
                }
            }
        }
        None
    }

    /// Column index under an x-coordinate.
    fn lane_at(&self, x: f32) -> Option<usize> {
        self.lanes()
            .iter()
            .position(|&(lx, lw)| x >= lx && x < lx + lw)
    }

    /// Insert slot within a lane for a pointer y.
    fn slot_at(&self, col: usize, y: f32) -> usize {
        let (x, w) = self.lanes()[col];
        let cards = self.cols[col].cards.len();
        for j in 0..cards {
            let r = self.card_rect(x, w, j);
            if y < r.min_y() + r.height() / 2.0 {
                return j;
            }
        }
        cards
    }
}

impl Widget for Kanban {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            cx.pt(WIDTH_PT).min(constraints.max_size.x.max(0.0)),
            cx.pt(HEIGHT_PT).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(160.0, 80.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Group);
        node.set_label(format!("{} — {} columns", self.label, self.cols.len()));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                ..
            } => {
                if let Some((col, card)) = self.card_at(*position) {
                    self.pressed = Some((col, card, *position));
                    return EventResponse::CapturePointer;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerMoved { position } => {
                if let Some((col, card, start)) = self.pressed {
                    // Lift into a drag once the pointer travels.
                    if self.drag.is_none() && position.distance(start) > 4.0 * cx.scale {
                        let over = self.lane_at(position.x).unwrap_or(col);
                        self.drag = Some(Drag {
                            from_col: col,
                            card,
                            pos: *position,
                            over_col: over,
                            slot: self.slot_at(over, position.y),
                        });
                    }
                    let over = self.lane_at(position.x);
                    let slot = over.map(|o| self.slot_at(o, position.y));
                    if let Some(d) = &mut self.drag {
                        d.pos = *position;
                        if let (Some(o), Some(s)) = (over, slot) {
                            d.over_col = o;
                            d.slot = s;
                        }
                        return EventResponse::RequestRepaint;
                    }
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                ..
            } => {
                if let Some(d) = self.drag.take() {
                    self.pressed = None;
                    // Move the card to the drop slot.
                    let card = self.cols[d.from_col].cards.remove(d.card);
                    let (to_col, mut slot) = (d.over_col, d.slot);
                    // Same-lane downward drops shift the slot after removal.
                    if to_col == d.from_col && slot > d.card {
                        slot -= 1;
                    }
                    slot = slot.min(self.cols[to_col].cards.len());
                    self.cols[to_col].cards.insert(slot, card);
                    self.moved = Some((d.from_col, slot, to_col));
                    return EventResponse::RequestRepaint;
                }
                if let Some((col, card, _)) = self.pressed.take() {
                    let flat: usize = self
                        .cols
                        .iter()
                        .take(col)
                        .map(|c| c.cards.len())
                        .sum::<usize>()
                        + card;
                    self.selected = Some(flat);
                    return EventResponse::Handled;
                }
                EventResponse::Ignored
            }
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let krect = |r: Rect| {
            kurbo::Rect::new(
                f64::from(r.min_x()),
                f64::from(r.min_y()),
                f64::from(r.max_x()),
                f64::from(r.max_y()),
            )
        };
        cx.list.push_fill_shape(
            krect(self.bounds),
            &martensite_core::shape::Shape::rounded(cx.pt(4.0)),
            cx.color(TokenKey::SurfaceColor, FACE),
        );
        let edge = cx.color(TokenKey::BorderColor, EDGE);
        let glyph = cx.color(TokenKey::TextColor, GLYPH);
        let muted = cx.color(TokenKey::TextMutedColor, MUTED);
        let pad = PAD_PT * self.scale;
        let head = HEAD_PT * self.scale;
        for (i, (x, w)) in self.lanes().iter().enumerate() {
            // Lane + header bar.
            let lane = Rect::new(
                *x,
                self.bounds.min_y() + pad,
                *w,
                self.bounds.height() - 2.0 * pad,
            );
            cx.list.push_fill_shape(
                krect(lane),
                &martensite_core::shape::Shape::rounded(cx.pt(3.0)),
                LANE,
            );
            let name_w =
                (self.cols[i].name.len() as f32 * 3.5 * self.scale).min(*w - 12.0 * self.scale);
            cx.list.push_fill_shape(
                kurbo::Rect::new(
                    f64::from(x + 8.0 * self.scale),
                    f64::from(self.bounds.min_y() + pad + head / 2.0 - self.scale),
                    f64::from(x + 8.0 * self.scale + name_w),
                    f64::from(self.bounds.min_y() + pad + head / 2.0 + self.scale),
                ),
                &martensite_core::shape::Shape::rounded(self.scale),
                muted,
            );
            // Cards (skip the one being dragged; it floats).
            for (j, card) in self.cols[i].cards.iter().enumerate() {
                let lifted = self
                    .drag
                    .as_ref()
                    .is_some_and(|d| d.from_col == i && d.card == j);
                if lifted {
                    continue;
                }
                let r = self.card_rect(*x, *w, j);
                cx.list.push_fill_shape(
                    krect(r),
                    &martensite_core::shape::Shape::rounded(cx.pt(3.0)),
                    CARD,
                );
                cx.list.push_stroke_shape(
                    krect(r),
                    &martensite_core::shape::Shape::rounded(cx.pt(3.0)),
                    cx.pt(0.5),
                    edge,
                );
                let tw = (card.len() as f32 * 3.5 * self.scale).min(r.width() - 10.0 * self.scale);
                cx.list.push_fill_shape(
                    kurbo::Rect::new(
                        f64::from(r.min_x() + 5.0 * self.scale),
                        f64::from(r.min_y() + r.height() / 2.0 - self.scale),
                        f64::from(r.min_x() + 5.0 * self.scale + tw),
                        f64::from(r.min_y() + r.height() / 2.0 + self.scale),
                    ),
                    &martensite_core::shape::Shape::rounded(self.scale),
                    glyph,
                );
            }
        }
        // Drop slot + floating card while dragging.
        if let Some(d) = &self.drag {
            let (x, w) = self.lanes()[d.over_col];
            let mut r = self.card_rect(x, w, d.slot.min(self.cols[d.over_col].cards.len()));
            // Slots past the last card sit at the stack's end.
            if d.slot >= self.cols[d.over_col].cards.len()
                && !self.cols[d.over_col].cards.is_empty()
            {
                let last = self.card_rect(x, w, self.cols[d.over_col].cards.len() - 1);
                r = Rect::new(
                    r.min_x(),
                    last.max_y() + CARD_GAP_PT * self.scale,
                    r.width(),
                    r.height(),
                );
            }
            cx.list.push_fill_shape(
                krect(r),
                &martensite_core::shape::Shape::rounded(cx.pt(3.0)),
                cx.color(TokenKey::AccentColor, SLOT),
            );
            // Floating card under the pointer.
            let ch = CARD_PT * self.scale;
            let fr = Rect::new(d.pos.x - w / 2.0, d.pos.y - ch / 2.0, w - pad, ch);
            cx.list.push_fill_shape(
                krect(fr),
                &martensite_core::shape::Shape::rounded(cx.pt(3.0)),
                CARD_HI,
            );
        }
        cx.list.push_stroke_shape(
            krect(self.bounds),
            &martensite_core::shape::Shape::rounded(cx.pt(4.0)),
            cx.pt(0.75),
            edge,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(k: &mut Kanban, w: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        k.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(w, h),
            },
        );
        k.layout(&mut cx, Rect::new(0.0, 0.0, w, h));
    }

    fn board() -> Kanban {
        Kanban::new()
            .column("Todo")
            .card("Todo", "A")
            .card("Todo", "B")
            .column("Done")
            .card("Done", "C")
    }

    fn ev(k: &mut Kanban, e: &WidgetEvent) {
        k.event(&mut EventContext {
            event: e,
            bounds: k.bounds,
            scale: 1.0,
        });
    }

    #[test]
    fn builds() {
        let k = board();
        assert_eq!(k.column_count(), 2);
        assert_eq!(k.card_count(0), 2);
        assert_eq!(k.card_title(0, 1), "B");
        assert_eq!(k.card_title(1, 0), "C");
    }

    #[test]
    fn card_hit_testing() {
        let mut k = board();
        laid_out(&mut k, 320.0, 180.0);
        // Card A: lane 0 starts at pad=6, head below that.
        let (x, w) = k.lanes()[0];
        let r = k.card_rect(x, w, 0);
        assert_eq!(
            k.card_at(Vec2::new(r.min_x() + 5.0, r.min_y() + 5.0)),
            Some((0, 0))
        );
        assert_eq!(k.card_at(Vec2::new(1.0, 1.0)), None);
    }

    #[test]
    fn click_selects_card() {
        let mut k = board();
        laid_out(&mut k, 320.0, 180.0);
        let (x, w) = k.lanes()[0];
        let p = k.card_rect(x, w, 1); // card B
        let mid = Vec2::new(p.min_x() + 10.0, p.min_y() + 10.0);
        ev(
            &mut k,
            &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: mid,
                count: 1,
            },
        );
        ev(
            &mut k,
            &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: mid,
            },
        );
        assert_eq!(k.take_selected(), Some(1)); // flat index: A=0, B=1
        assert_eq!(k.take_selected(), None);
    }

    #[test]
    fn drag_moves_card_between_lanes() {
        let mut k = board();
        laid_out(&mut k, 320.0, 180.0);
        let (x, w) = k.lanes()[0];
        let from = k.card_rect(x, w, 0);
        let start = Vec2::new(from.min_x() + 10.0, from.min_y() + 10.0);
        ev(
            &mut k,
            &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: start,
                count: 1,
            },
        );
        let (x1, _w1) = k.lanes()[1];
        let target = Vec2::new(x1 + 20.0, 38.0); // upper half of C → slot 0
        ev(&mut k, &WidgetEvent::PointerMoved { position: target });
        assert!(k.is_dragging());
        ev(
            &mut k,
            &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: target,
            },
        );
        assert_eq!(k.card_count(0), 1);
        assert_eq!(k.card_count(1), 2);
        assert_eq!(k.card_title(1, 0), "A"); // dropped above C
        assert_eq!(k.take_moved(), Some((0, 0, 1)));
    }

    #[test]
    fn same_lane_reorder() {
        let mut k = board();
        laid_out(&mut k, 320.0, 180.0);
        let (x, w) = k.lanes()[0];
        let from = k.card_rect(x, w, 0);
        let start = Vec2::new(from.min_x() + 10.0, from.min_y() + 10.0);
        ev(
            &mut k,
            &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: start,
                count: 1,
            },
        );
        // Drag below card B → slot 2 → post-removal slot 1.
        let below = Vec2::new(x + 20.0, k.card_rect(x, w, 1).max_y() + 4.0);
        ev(&mut k, &WidgetEvent::PointerMoved { position: below });
        ev(
            &mut k,
            &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: below,
            },
        );
        assert_eq!(k.card_title(0, 0), "B");
        assert_eq!(k.card_title(0, 1), "A");
        assert_eq!(k.take_moved(), Some((0, 1, 0)));
    }
}
