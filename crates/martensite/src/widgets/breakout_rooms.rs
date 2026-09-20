//! `BreakoutRooms` — the conference breakout-room panel (Zoom /
//! Teams idiom): rows of named rooms with occupancy counts and a
//! Join button; the host's current room shows a badge.
//!
//! Clicks park the room index in [`BreakoutRooms::take_joined`] —
//! the host owns actually moving the participant.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::breakout_rooms::{BreakoutRooms, Room};
//!
//! let r = BreakoutRooms::new().room(Room::new("Design", 4));
//! assert_eq!(r.room_count(), 1);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::text_paint::SharedTextPainter;

const PAD_PT: f32 = 10.0;
const ROW_PT: f32 = 34.0;
const BTN_PT_W: f32 = 56.0;
const BTN_PT_H: f32 = 22.0;
const NAME_PT: f32 = 12.0;
const COUNT_PT: f32 = 10.0;

const FACE: [u8; 4] = [34, 36, 44, 255];
const EDGE: [u8; 4] = [70, 74, 84, 255];
const TEXT_FG: [u8; 4] = [235, 237, 240, 255];
const MUTED_FG: [u8; 4] = [150, 154, 164, 255];
const BTN_FACE: [u8; 4] = [52, 55, 66, 255];
const OK: [u8; 4] = [70, 180, 100, 255];

/// One breakout room.
///
/// ```
/// use martensite::widgets::breakout_rooms::Room;
///
/// assert_eq!(Room::new("Lobby", 3).occupants, 3);
/// ```
#[derive(Clone, Debug)]
pub struct Room {
    /// Room name.
    pub name: String,
    /// Current participant count.
    pub occupants: usize,
    /// Room capacity (0 = unlimited).
    pub capacity: usize,
    /// Whether this is where the local user is.
    pub current: bool,
}

impl Room {
    /// A room named `name` with `occupants`.
    ///
    /// ```
    /// use martensite::widgets::breakout_rooms::Room;
    ///
    /// assert_eq!(Room::new("R", 2).name, "R");
    /// ```
    pub fn new(name: impl Into<String>, occupants: usize) -> Self {
        Self {
            name: name.into(),
            occupants,
            capacity: 0,
            current: false,
        }
    }

    /// Capacity cap.
    ///
    /// ```
    /// use martensite::widgets::breakout_rooms::Room;
    ///
    /// assert_eq!(Room::new("R", 0).capacity(8).capacity, 8);
    /// ```
    pub fn capacity(mut self, cap: usize) -> Self {
        self.capacity = cap;
        self
    }

    /// Marks this as the local user's room.
    ///
    /// ```
    /// use martensite::widgets::breakout_rooms::Room;
    ///
    /// assert!(Room::new("R", 0).current().current);
    /// ```
    pub fn current(mut self) -> Self {
        self.current = true;
        self
    }

    /// `true` when `capacity` is set and reached.
    ///
    /// ```
    /// use martensite::widgets::breakout_rooms::Room;
    ///
    /// assert!(Room::new("R", 4).capacity(4).is_full());
    /// ```
    pub fn is_full(&self) -> bool {
        self.capacity > 0 && self.occupants >= self.capacity
    }
}

/// The rooms panel — see the module docs.
///
/// ```
/// use martensite::widgets::breakout_rooms::BreakoutRooms;
///
/// assert_eq!(BreakoutRooms::new().room_count(), 0);
/// ```
pub struct BreakoutRooms {
    /// Accessibility label.
    pub label: String,
    rooms: Vec<Room>,
    joined: Option<usize>,
    rows: Vec<(Rect, Rect)>,
    text_painter: Option<SharedTextPainter>,
    bounds: Rect,
    scale: f32,
}

impl std::fmt::Debug for BreakoutRooms {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BreakoutRooms")
            .field("rooms", &self.rooms.len())
            .finish()
    }
}

impl BreakoutRooms {
    /// An empty panel.
    ///
    /// ```
    /// use martensite::widgets::breakout_rooms::BreakoutRooms;
    ///
    /// assert_eq!(BreakoutRooms::new().room_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Breakout rooms".to_string(),
            rooms: Vec::new(),
            joined: None,
            rows: Vec::new(),
            text_painter: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Appends a room.
    ///
    /// ```
    /// use martensite::widgets::breakout_rooms::{BreakoutRooms, Room};
    ///
    /// assert_eq!(BreakoutRooms::new().room(Room::new("R", 1)).room_count(), 1);
    /// ```
    pub fn room(mut self, room: Room) -> Self {
        self.rooms.push(room);
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::breakout_rooms::BreakoutRooms;
    ///
    /// assert_eq!(BreakoutRooms::new().label("Rooms").label, "Rooms");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Shared text painter.
    ///
    /// ```no_run
    /// use martensite::widgets::breakout_rooms::BreakoutRooms;
    /// # fn example(p: martensite::text_paint::SharedTextPainter) {
    /// let _r = BreakoutRooms::new().with_text_painter(p);
    /// # }
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Room count.
    ///
    /// ```
    /// use martensite::widgets::breakout_rooms::BreakoutRooms;
    ///
    /// assert_eq!(BreakoutRooms::new().room_count(), 0);
    /// ```
    pub fn room_count(&self) -> usize {
        self.rooms.len()
    }

    /// A room by index.
    ///
    /// ```
    /// use martensite::widgets::breakout_rooms::{BreakoutRooms, Room};
    ///
    /// let r = BreakoutRooms::new().room(Room::new("QA", 2));
    /// assert_eq!(r.room_at(0).unwrap().name, "QA");
    /// ```
    pub fn room_at(&self, index: usize) -> Option<&Room> {
        self.rooms.get(index)
    }

    /// Drains the last clicked Join index.
    ///
    /// ```
    /// use martensite::widgets::breakout_rooms::BreakoutRooms;
    ///
    /// assert_eq!(BreakoutRooms::new().take_joined(), None);
    /// ```
    pub fn take_joined(&mut self) -> Option<usize> {
        self.joined.take()
    }
}

impl Default for BreakoutRooms {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for BreakoutRooms {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let s = cx.scale;
        Vec2::new(
            (280.0 * s).min(constraints.max_size.x.max(0.0)),
            ((PAD_PT * 2.0 + self.rooms.len() as f32 * ROW_PT) * s)
                .min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(180.0, 44.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        let s = cx.scale;
        self.rows.clear();
        let pad = PAD_PT * s;
        let row = ROW_PT * s;
        let bw = BTN_PT_W * s;
        let bh = BTN_PT_H * s;
        for i in 0..self.rooms.len() {
            let y = bounds.min_y() + pad + i as f32 * row;
            self.rows.push((
                Rect::new(bounds.min_x() + pad, y, bounds.width() - pad * 2.0, row),
                Rect::new(bounds.max_x() - pad - bw, y + (row - bh) / 2.0, bw, bh),
            ));
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::List);
        node.set_label(self.label.clone());
        node.set_value(format!("{} rooms", self.rooms.len()));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if let WidgetEvent::PointerReleased {
            button: PointerButton::Primary,
            position,
        } = cx.event
        {
            for (i, (_, btn)) in self.rows.iter().enumerate() {
                if btn.contains(*position) && !self.rooms[i].is_full() && !self.rooms[i].current {
                    self.joined = Some(i);
                    return EventResponse::RequestRepaint;
                }
            }
        }
        EventResponse::Ignored
    }

    fn paint(&self, cx: &mut PaintContext) {
        let s = cx.scale;
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let b = self.bounds;
        let accent = cx.color(TokenKey::AccentColor, [90, 140, 220, 255]);
        cx.list.push_fill_shape(
            kurbo::Rect::new(
                f64::from(b.min_x()),
                f64::from(b.min_y()),
                f64::from(b.max_x()),
                f64::from(b.max_y()),
            ),
            &martensite_core::shape::Shape::rounded(6.0 * s),
            cx.color(TokenKey::SurfaceColor, FACE),
        );
        let nfs = NAME_PT * s;
        let cfs = COUNT_PT * s;
        for (i, (room, (row, btn))) in self.rooms.iter().zip(self.rows.iter()).enumerate() {
            if i > 0 {
                cx.list.push_stroke_path(
                    line_path(
                        Vec2::new(row.min_x(), row.min_y()),
                        Vec2::new(row.max_x(), row.min_y()),
                    ),
                    0.5,
                    EDGE,
                );
            }
            // Name + occupancy.
            crate::text_paint::paint_label(
                painter,
                cx.list,
                kurbo::Point::new(
                    f64::from(row.min_x() + 4.0 * s),
                    f64::from(row.min_y() + nfs + 2.0 * s),
                ),
                &room.name,
                nfs,
                cx.color(TokenKey::TextColor, TEXT_FG),
            );
            let cap = if room.capacity > 0 {
                format!("{}/{}", room.occupants, room.capacity)
            } else {
                format!("{}", room.occupants)
            };
            crate::text_paint::paint_label(
                painter,
                cx.list,
                kurbo::Point::new(
                    f64::from(row.min_x() + 4.0 * s),
                    f64::from(row.max_y() - 4.0 * s),
                ),
                &format!("{cap} · {}", if room.current { "you" } else { "joined" }),
                cfs,
                if room.current { OK } else { MUTED_FG },
            );
            // Join button (or "Full"/"Here" caption).
            let (caption, face, fg) = if room.current {
                ("Here", BTN_FACE, OK)
            } else if room.is_full() {
                ("Full", BTN_FACE, MUTED_FG)
            } else {
                ("Join", accent, [255, 255, 255, 255])
            };
            cx.list.push_fill_shape(
                kurbo::Rect::new(
                    f64::from(btn.min_x()),
                    f64::from(btn.min_y()),
                    f64::from(btn.max_x()),
                    f64::from(btn.max_y()),
                ),
                &martensite_core::shape::Shape::rounded(4.0 * s),
                face,
            );
            let w = caption.len() as f32 * cfs * 0.6;
            crate::text_paint::paint_label(
                painter,
                cx.list,
                kurbo::Point::new(
                    f64::from(btn.min_x() + (btn.width() - w) / 2.0),
                    f64::from(btn.min_y() + btn.height() / 2.0 + cfs * 0.35),
                ),
                caption,
                cfs,
                fg,
            );
        }
    }
}

fn line_path(a: Vec2, b: Vec2) -> kurbo::BezPath {
    let mut p = kurbo::BezPath::new();
    p.move_to((f64::from(a.x), f64::from(a.y)));
    p.line_to((f64::from(b.x), f64::from(b.y)));
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn fixture() -> BreakoutRooms {
        BreakoutRooms::new()
            .room(Room::new("Design", 4).capacity(8))
            .room(Room::new("Eng", 6).capacity(6))
            .room(Room::new("Lobby", 2).current())
    }

    fn laid_out(r: &mut BreakoutRooms) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        r.layout(&mut cx, Rect::new(0.0, 0.0, 280.0, 140.0));
    }

    fn click_btn(r: &mut BreakoutRooms, i: usize) {
        let (_, btn) = r.rows[i];
        r.event(&mut EventContext {
            event: &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: Vec2::new(btn.min_x() + 4.0, btn.min_y() + 4.0),
            },
            bounds: r.bounds,
            scale: 1.0,
        });
    }

    #[test]
    fn join_parks_index() {
        let mut r = fixture();
        laid_out(&mut r);
        click_btn(&mut r, 0);
        assert_eq!(r.take_joined(), Some(0));
        assert_eq!(r.take_joined(), None);
    }

    #[test]
    fn full_room_wont_join() {
        let mut r = fixture();
        laid_out(&mut r);
        click_btn(&mut r, 1);
        assert_eq!(r.take_joined(), None);
    }

    #[test]
    fn current_room_wont_join() {
        let mut r = fixture();
        laid_out(&mut r);
        click_btn(&mut r, 2);
        assert_eq!(r.take_joined(), None);
    }

    #[test]
    fn paint_without_painter() {
        let mut r = fixture();
        laid_out(&mut r);
        let mut list = martensite_core::PaintList::default();
        let theme = martensite_theme::Theme::new("test");
        r.paint(&mut PaintContext {
            list: &mut list,
            bounds: r.bounds,
            theme: &theme,
            scale: 1.0,
            text_painter: None,
        });
        assert!(!list.is_empty());
    }
}
