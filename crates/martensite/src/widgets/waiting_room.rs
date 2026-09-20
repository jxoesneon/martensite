//! `WaitingRoom` — a conference lobby (Zoom/Meet admit panel
//! idiom): queued attendees with per-row **admit** and **deny**
//! buttons plus an *admit all* affordance.
//!
//! Rows are pushed host-side via [`WaitingRoom::queue`]; clicking
//! a row's ✓ parks its index in [`WaitingRoom::take_admitted`],
//! ✕ parks it in [`WaitingRoom::take_denied`], and the header
//! button parks `usize::MAX` in `take_admitted` (admit all). The
//! host removes admitted rows with
//! [`WaitingRoom::remove`]. Companion to
//! [`AttendeeList`](crate::widgets::AttendeeList).
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::waiting_room::WaitingRoom;
//!
//! let mut w = WaitingRoom::new();
//! w.queue("Ana");
//! w.queue("Ben");
//! assert_eq!(w.waiting_count(), 2);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::text_paint::SharedTextPainter;

const ROW_PT: f32 = 32.0;
const HEAD_PT: f32 = 40.0;
const PAD_PT: f32 = 8.0;
const BTN_PT: f32 = 24.0;
const FONT_PT: f32 = 12.0;

const FACE: [u8; 4] = [30, 32, 40, 255];
const ROW_HOVER: [u8; 4] = [255, 255, 255, 14];
const ADMIT: [u8; 4] = [80, 170, 100, 255];
const DENY: [u8; 4] = [190, 80, 80, 255];
const TEXT: [u8; 4] = [235, 237, 240, 255];
const MUTED_FG: [u8; 4] = [150, 154, 164, 255];

/// The lobby — see the module docs.
///
/// ```
/// use martensite::widgets::waiting_room::WaitingRoom;
///
/// assert_eq!(WaitingRoom::new().waiting_count(), 0);
/// ```
pub struct WaitingRoom {
    /// Accessibility label.
    pub label: String,
    /// Header text above the queue.
    pub title: String,
    /// Row height in points.
    pub row_height: f32,
    waiting: Vec<String>,
    admitted: Option<usize>,
    denied: Option<usize>,
    hovered: Option<usize>,
    rows: Vec<Rect>,
    admit_rects: Vec<Rect>,
    deny_rects: Vec<Rect>,
    admit_all_rect: Rect,
    text_painter: Option<SharedTextPainter>,
    bounds: Rect,
    scale: f32,
}

impl std::fmt::Debug for WaitingRoom {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WaitingRoom")
            .field("waiting", &self.waiting.len())
            .finish()
    }
}

impl Default for WaitingRoom {
    fn default() -> Self {
        Self::new()
    }
}

impl WaitingRoom {
    /// Empty lobby.
    ///
    /// ```
    /// use martensite::widgets::waiting_room::WaitingRoom;
    ///
    /// assert_eq!(WaitingRoom::new().waiting_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Waiting room".to_string(),
            title: "Waiting to join".to_string(),
            row_height: ROW_PT,
            waiting: Vec::new(),
            admitted: None,
            denied: None,
            hovered: None,
            rows: Vec::new(),
            admit_rects: Vec::new(),
            deny_rects: Vec::new(),
            admit_all_rect: Rect::new(0.0, 0.0, 0.0, 0.0),
            text_painter: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::waiting_room::WaitingRoom;
    ///
    /// assert_eq!(WaitingRoom::new().label("Lobby").label, "Lobby");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Header text.
    ///
    /// ```
    /// use martensite::widgets::waiting_room::WaitingRoom;
    ///
    /// assert_eq!(WaitingRoom::new().title("Knock knock").title, "Knock knock");
    /// ```
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    /// Shared text painter.
    ///
    /// ```no_run
    /// use martensite::widgets::waiting_room::WaitingRoom;
    /// # fn example(p: martensite::text_paint::SharedTextPainter) {
    /// let _w = WaitingRoom::new().with_text_painter(p);
    /// # }
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Adds a waiting attendee at the end of the queue.
    ///
    /// ```
    /// use martensite::widgets::waiting_room::WaitingRoom;
    ///
    /// let mut w = WaitingRoom::new();
    /// w.queue("Ana");
    /// assert_eq!(w.waiting_at(0), Some("Ana"));
    /// ```
    pub fn queue(&mut self, name: impl Into<String>) {
        self.waiting.push(name.into());
    }

    /// Waiting count.
    ///
    /// ```
    /// use martensite::widgets::waiting_room::WaitingRoom;
    ///
    /// assert_eq!(WaitingRoom::new().waiting_count(), 0);
    /// ```
    pub fn waiting_count(&self) -> usize {
        self.waiting.len()
    }

    /// Name at queue position `i`.
    ///
    /// ```
    /// use martensite::widgets::waiting_room::WaitingRoom;
    ///
    /// assert_eq!(WaitingRoom::new().waiting_at(0), None);
    /// ```
    pub fn waiting_at(&self, i: usize) -> Option<&str> {
        self.waiting.get(i).map(String::as_str)
    }

    /// Removes a queued attendee (after admit or deny).
    ///
    /// ```
    /// use martensite::widgets::waiting_room::WaitingRoom;
    ///
    /// let mut w = WaitingRoom::new();
    /// w.queue("Ana");
    /// w.remove(0);
    /// assert_eq!(w.waiting_count(), 0);
    /// ```
    pub fn remove(&mut self, i: usize) {
        if i < self.waiting.len() {
            self.waiting.remove(i);
        }
    }

    /// Empties the queue (after admit-all).
    ///
    /// ```
    /// use martensite::widgets::waiting_room::WaitingRoom;
    ///
    /// let mut w = WaitingRoom::new();
    /// w.queue("A");
    /// w.clear();
    /// assert_eq!(w.waiting_count(), 0);
    /// ```
    pub fn clear(&mut self) {
        self.waiting.clear();
    }

    /// Drains the last admitted index — `usize::MAX` = admit all.
    ///
    /// ```
    /// use martensite::widgets::waiting_room::WaitingRoom;
    ///
    /// let mut w = WaitingRoom::new();
    /// assert_eq!(w.take_admitted(), None);
    /// ```
    pub fn take_admitted(&mut self) -> Option<usize> {
        self.admitted.take()
    }

    /// Drains the last denied index.
    ///
    /// ```
    /// use martensite::widgets::waiting_room::WaitingRoom;
    ///
    /// let mut w = WaitingRoom::new();
    /// assert_eq!(w.take_denied(), None);
    /// ```
    pub fn take_denied(&mut self) -> Option<usize> {
        self.denied.take()
    }
}

impl Widget for WaitingRoom {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let s = cx.scale;
        let rows = self.waiting.len().max(1) as f32;
        Vec2::new(
            (280.0 * s).min(constraints.max_size.x.max(0.0)),
            ((HEAD_PT + rows * self.row_height + PAD_PT) * s).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(200.0, HEAD_PT + ROW_PT)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        let s = cx.scale;
        // Header: title left, "admit all" right.
        self.admit_all_rect = Rect::new(
            bounds.max_x() - PAD_PT * s - 84.0 * s,
            bounds.min_y() + (HEAD_PT * s - BTN_PT * s) / 2.0,
            84.0 * s,
            BTN_PT * s,
        );
        let row_h = self.row_height * s;
        let btn = BTN_PT * s;
        self.rows.clear();
        self.admit_rects.clear();
        self.deny_rects.clear();
        for i in 0..self.waiting.len() {
            let r = Rect::new(
                bounds.min_x() + PAD_PT * s,
                bounds.min_y() + HEAD_PT * s + i as f32 * row_h,
                (bounds.width() - PAD_PT * 2.0 * s).max(0.0),
                row_h,
            );
            let by = r.min_y() + (r.height() - btn) / 2.0;
            self.deny_rects
                .push(Rect::new(r.max_x() - btn - 6.0 * s, by, btn, btn));
            self.admit_rects
                .push(Rect::new(r.max_x() - btn * 2.0 - 12.0 * s, by, btn, btn));
            self.rows.push(r);
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Group);
        node.set_label(self.label.clone());
        node.set_value(format!("{} waiting", self.waiting.len()));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerMoved { position } => {
                let h = self.rows.iter().position(|r| r.contains(*position));
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
                if self.admit_all_rect.contains(*position) {
                    self.admitted = Some(usize::MAX);
                    return EventResponse::RequestRepaint;
                }
                if let Some(i) = self.deny_rects.iter().position(|r| r.contains(*position)) {
                    self.denied = Some(i);
                    return EventResponse::RequestRepaint;
                }
                if let Some(i) = self.admit_rects.iter().position(|r| r.contains(*position)) {
                    self.admitted = Some(i);
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
        // Header.
        crate::text_paint::paint_label(
            painter,
            cx.list,
            kurbo::Point::new(
                f64::from(self.bounds.min_x() + PAD_PT * s),
                f64::from(self.bounds.min_y() + HEAD_PT * s * 0.65),
            ),
            &self.title,
            FONT_PT * s,
            cx.color(TokenKey::TextColor, TEXT),
        );
        let shape = martensite_core::shape::Shape::rounded(5.0 * s);
        // Admit-all button.
        let ar = self.admit_all_rect;
        let akr = kurbo::Rect::new(
            f64::from(ar.min_x()),
            f64::from(ar.min_y()),
            f64::from(ar.max_x()),
            f64::from(ar.max_y()),
        );
        cx.list
            .push_fill_shape(akr, &shape, cx.color(TokenKey::SuccessColor, ADMIT));
        crate::text_paint::paint_label_clipped(
            painter,
            cx.list,
            akr,
            kurbo::Point::new(
                akr.x0 + f64::from(8.0 * s),
                akr.y0 + f64::from(ar.height() * 0.72),
            ),
            "Admit all",
            FONT_PT * 0.85 * s,
            TEXT,
        );
        if self.waiting.is_empty() {
            crate::text_paint::paint_label(
                painter,
                cx.list,
                kurbo::Point::new(
                    f64::from(self.bounds.min_x() + PAD_PT * s),
                    f64::from(self.bounds.min_y() + (HEAD_PT + ROW_PT * 0.7) * s),
                ),
                "No one is waiting",
                FONT_PT * s,
                MUTED_FG,
            );
        }
        for (i, name) in self.waiting.iter().enumerate() {
            let r = self.rows[i];
            let kr = kurbo::Rect::new(
                f64::from(r.min_x()),
                f64::from(r.min_y()),
                f64::from(r.max_x()),
                f64::from(r.max_y()),
            );
            if self.hovered == Some(i) {
                cx.list.push_fill_shape(kr, &shape, ROW_HOVER);
            }
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                kr,
                kurbo::Point::new(
                    f64::from(r.min_x() + 8.0 * s),
                    f64::from(r.min_y() + r.height() * 0.7),
                ),
                name,
                FONT_PT * s,
                cx.color(TokenKey::TextColor, TEXT),
            );
            // Admit ✓.
            let a = self.admit_rects[i];
            cx.list.push_fill_shape(
                kurbo::Rect::new(
                    f64::from(a.min_x()),
                    f64::from(a.min_y()),
                    f64::from(a.max_x()),
                    f64::from(a.max_y()),
                ),
                &shape,
                cx.color(TokenKey::SuccessColor, ADMIT),
            );
            crate::text_paint::paint_label(
                painter,
                cx.list,
                kurbo::Point::new(
                    f64::from(a.min_x() + a.width() * 0.24),
                    f64::from(a.min_y() + a.height() * 0.74),
                ),
                "✓",
                FONT_PT * s,
                TEXT,
            );
            // Deny ✕.
            let d = self.deny_rects[i];
            cx.list.push_fill_shape(
                kurbo::Rect::new(
                    f64::from(d.min_x()),
                    f64::from(d.min_y()),
                    f64::from(d.max_x()),
                    f64::from(d.max_y()),
                ),
                &shape,
                cx.color(TokenKey::ErrorColor, DENY),
            );
            crate::text_paint::paint_label(
                painter,
                cx.list,
                kurbo::Point::new(
                    f64::from(d.min_x() + d.width() * 0.26),
                    f64::from(d.min_y() + d.height() * 0.74),
                ),
                "✕",
                FONT_PT * s,
                TEXT,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn fixture() -> WaitingRoom {
        let mut w = WaitingRoom::new();
        w.queue("Ana");
        w.queue("Ben");
        w.queue("Cat");
        w
    }

    fn laid_out(w: &mut WaitingRoom) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        w.layout(&mut cx, Rect::new(0.0, 0.0, 280.0, 180.0));
    }

    fn click(w: &mut WaitingRoom, r: Rect) {
        w.event(&mut EventContext {
            event: &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: Vec2::new((r.min_x() + r.max_x()) / 2.0, (r.min_y() + r.max_y()) / 2.0),
            },
            bounds: w.bounds,
            scale: 1.0,
        });
    }

    #[test]
    fn admit_parks_index() {
        let mut w = fixture();
        laid_out(&mut w);
        let r = w.admit_rects[1];
        click(&mut w, r);
        assert_eq!(w.take_admitted(), Some(1));
    }

    #[test]
    fn deny_parks_index() {
        let mut w = fixture();
        laid_out(&mut w);
        let r = w.deny_rects[2];
        click(&mut w, r);
        assert_eq!(w.take_denied(), Some(2));
    }

    #[test]
    fn admit_all_parks_max() {
        let mut w = fixture();
        laid_out(&mut w);
        let r = w.admit_all_rect;
        click(&mut w, r);
        assert_eq!(w.take_admitted(), Some(usize::MAX));
    }

    #[test]
    fn remove_shifts_queue() {
        let mut w = fixture();
        w.remove(0);
        assert_eq!(w.waiting_at(0), Some("Ben"));
    }

    #[test]
    fn paint_without_painter() {
        let mut w = fixture();
        laid_out(&mut w);
        let mut list = martensite_core::PaintList::default();
        let theme = martensite_theme::Theme::new("test");
        w.paint(&mut PaintContext {
            list: &mut list,
            bounds: w.bounds,
            theme: &theme,
            scale: 1.0,
            text_painter: None,
        });
        assert!(!list.is_empty());
    }
}
