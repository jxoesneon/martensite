//! `AttendeeList` — a conference roster (Zoom/Meet participants
//! panel idiom): rows of [`Attendee`] — presence dot, name, a
//! speaking highlight, and muted / raised-hand badges.
//!
//! Clicking a row parks its index in
//! [`AttendeeList::take_selected`]; `set_muted`, `set_hand`,
//! `set_speaking`, and `set_status` update row state host-side,
//! and [`AttendeeList::raised_hands`] returns the indices with
//! hands up so the host can surface them. Companion to
//! [`VideoGrid`](crate::widgets::VideoGrid) and
//! [`CallControls`](crate::widgets::CallControls).
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::attendee_list::{Attendee, AttendeeList};
//!
//! let a = AttendeeList::new()
//!     .attendee(Attendee::new("Ana"))
//!     .attendee(Attendee::new("Ben").hand_raised(true));
//! assert_eq!(a.attendee_count(), 2);
//! assert_eq!(a.raised_hands(), &[1]);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::text_paint::SharedTextPainter;
use crate::widgets::presence::PresenceStatus;

const ROW_PT: f32 = 34.0;
const PAD_PT: f32 = 6.0;
const DOT_PT: f32 = 9.0;
const FONT_PT: f32 = 12.0;
const BADGE_PT: f32 = 20.0;

const FACE: [u8; 4] = [30, 32, 40, 255];
const ROW_HOVER: [u8; 4] = [255, 255, 255, 14];
const SPEAKING: [u8; 4] = [90, 200, 120, 60];
const TEXT: [u8; 4] = [235, 237, 240, 255];
const BADGE_BG: [u8; 4] = [60, 63, 74, 230];
const BADGE_FG: [u8; 4] = [220, 222, 226, 255];

/// One roster entry.
///
/// ```
/// use martensite::widgets::attendee_list::Attendee;
///
/// let a = Attendee::new("Ana").muted(true);
/// assert!(a.muted);
/// ```
#[derive(Clone, Debug)]
pub struct Attendee {
    /// Display name.
    pub name: String,
    /// Presence dot.
    pub status: PresenceStatus,
    /// Microphone-muted badge.
    pub muted: bool,
    /// Raised-hand badge.
    pub hand_raised: bool,
    /// Active-speaker row highlight.
    pub speaking: bool,
}

impl Attendee {
    /// An online attendee.
    ///
    /// ```
    /// use martensite::widgets::attendee_list::Attendee;
    ///
    /// assert_eq!(Attendee::new("A").name, "A");
    /// ```
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            status: PresenceStatus::Online,
            muted: false,
            hand_raised: false,
            speaking: false,
        }
    }

    /// Presence status.
    ///
    /// ```
    /// use martensite::widgets::attendee_list::Attendee;
    /// use martensite::widgets::presence::PresenceStatus;
    ///
    /// assert_eq!(Attendee::new("A").status(PresenceStatus::Busy).status, PresenceStatus::Busy);
    /// ```
    pub fn status(mut self, status: PresenceStatus) -> Self {
        self.status = status;
        self
    }

    /// Muted flag.
    ///
    /// ```
    /// use martensite::widgets::attendee_list::Attendee;
    ///
    /// assert!(Attendee::new("A").muted(true).muted);
    /// ```
    pub fn muted(mut self, muted: bool) -> Self {
        self.muted = muted;
        self
    }

    /// Raised-hand flag.
    ///
    /// ```
    /// use martensite::widgets::attendee_list::Attendee;
    ///
    /// assert!(Attendee::new("A").hand_raised(true).hand_raised);
    /// ```
    pub fn hand_raised(mut self, raised: bool) -> Self {
        self.hand_raised = raised;
        self
    }

    /// Speaking flag.
    ///
    /// ```
    /// use martensite::widgets::attendee_list::Attendee;
    ///
    /// assert!(Attendee::new("A").speaking(true).speaking);
    /// ```
    pub fn speaking(mut self, speaking: bool) -> Self {
        self.speaking = speaking;
        self
    }
}

/// The roster — see the module docs.
///
/// ```
/// use martensite::widgets::attendee_list::AttendeeList;
///
/// assert_eq!(AttendeeList::new().attendee_count(), 0);
/// ```
pub struct AttendeeList {
    /// Accessibility label.
    pub label: String,
    /// Row height in points.
    pub row_height: f32,
    attendees: Vec<Attendee>,
    selected: Option<usize>,
    hovered: Option<usize>,
    rows: Vec<Rect>,
    text_painter: Option<SharedTextPainter>,
    bounds: Rect,
    scale: f32,
}

impl std::fmt::Debug for AttendeeList {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AttendeeList")
            .field("attendees", &self.attendees.len())
            .finish()
    }
}

impl Default for AttendeeList {
    fn default() -> Self {
        Self::new()
    }
}

impl AttendeeList {
    /// Empty roster.
    ///
    /// ```
    /// use martensite::widgets::attendee_list::AttendeeList;
    ///
    /// assert_eq!(AttendeeList::new().attendee_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Attendees".to_string(),
            row_height: ROW_PT,
            attendees: Vec::new(),
            selected: None,
            hovered: None,
            rows: Vec::new(),
            text_painter: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Appends an attendee.
    ///
    /// ```
    /// use martensite::widgets::attendee_list::{Attendee, AttendeeList};
    ///
    /// assert_eq!(AttendeeList::new().attendee(Attendee::new("A")).attendee_count(), 1);
    /// ```
    pub fn attendee(mut self, a: Attendee) -> Self {
        self.attendees.push(a);
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::attendee_list::AttendeeList;
    ///
    /// assert_eq!(AttendeeList::new().label("Call roster").label, "Call roster");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Shared text painter for names and badges.
    ///
    /// ```no_run
    /// use martensite::widgets::attendee_list::AttendeeList;
    /// # fn example(p: martensite::text_paint::SharedTextPainter) {
    /// let _a = AttendeeList::new().with_text_painter(p);
    /// # }
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Attendee count.
    ///
    /// ```
    /// use martensite::widgets::attendee_list::AttendeeList;
    ///
    /// assert_eq!(AttendeeList::new().attendee_count(), 0);
    /// ```
    pub fn attendee_count(&self) -> usize {
        self.attendees.len()
    }

    /// Indices of attendees with raised hands, in roster order.
    ///
    /// ```
    /// use martensite::widgets::attendee_list::{Attendee, AttendeeList};
    ///
    /// let a = AttendeeList::new()
    ///     .attendee(Attendee::new("A"))
    ///     .attendee(Attendee::new("B").hand_raised(true));
    /// assert_eq!(a.raised_hands(), &[1]);
    /// ```
    pub fn raised_hands(&self) -> Vec<usize> {
        self.attendees
            .iter()
            .enumerate()
            .filter(|(_, a)| a.hand_raised)
            .map(|(i, _)| i)
            .collect()
    }

    /// Sets the muted flag host-side.
    ///
    /// ```
    /// use martensite::widgets::attendee_list::{Attendee, AttendeeList};
    ///
    /// let mut a = AttendeeList::new().attendee(Attendee::new("A"));
    /// a.set_muted(0, true);
    /// assert!(a.is_muted(0));
    /// ```
    pub fn set_muted(&mut self, i: usize, muted: bool) {
        if let Some(a) = self.attendees.get_mut(i) {
            a.muted = muted;
        }
    }

    /// Sets the raised-hand flag host-side.
    ///
    /// ```
    /// use martensite::widgets::attendee_list::{Attendee, AttendeeList};
    ///
    /// let mut a = AttendeeList::new().attendee(Attendee::new("A"));
    /// a.set_hand(0, true);
    /// assert_eq!(a.raised_hands(), &[0]);
    /// ```
    pub fn set_hand(&mut self, i: usize, raised: bool) {
        if let Some(a) = self.attendees.get_mut(i) {
            a.hand_raised = raised;
        }
    }

    /// Sets the speaking flag host-side.
    ///
    /// ```
    /// use martensite::widgets::attendee_list::{Attendee, AttendeeList};
    ///
    /// let mut a = AttendeeList::new().attendee(Attendee::new("A"));
    /// a.set_speaking(0, true);
    /// assert!(a.is_speaking(0));
    /// ```
    pub fn set_speaking(&mut self, i: usize, speaking: bool) {
        if let Some(a) = self.attendees.get_mut(i) {
            a.speaking = speaking;
        }
    }

    /// Sets the presence status host-side.
    ///
    /// ```
    /// use martensite::widgets::attendee_list::{Attendee, AttendeeList};
    /// use martensite::widgets::presence::PresenceStatus;
    ///
    /// let mut a = AttendeeList::new().attendee(Attendee::new("A"));
    /// a.set_status(0, PresenceStatus::Away);
    /// assert_eq!(a.status_of(0), Some(PresenceStatus::Away));
    /// ```
    pub fn set_status(&mut self, i: usize, status: PresenceStatus) {
        if let Some(a) = self.attendees.get_mut(i) {
            a.status = status;
        }
    }

    /// Flags for a row.
    ///
    /// ```
    /// use martensite::widgets::attendee_list::AttendeeList;
    ///
    /// assert!(!AttendeeList::new().is_muted(0));
    /// ```
    pub fn is_muted(&self, i: usize) -> bool {
        self.attendees.get(i).is_some_and(|a| a.muted)
    }

    /// Speaking flag for a row.
    ///
    /// ```
    /// use martensite::widgets::attendee_list::AttendeeList;
    ///
    /// assert!(!AttendeeList::new().is_speaking(0));
    /// ```
    pub fn is_speaking(&self, i: usize) -> bool {
        self.attendees.get(i).is_some_and(|a| a.speaking)
    }

    /// Presence status for a row.
    ///
    /// ```
    /// use martensite::widgets::attendee_list::AttendeeList;
    ///
    /// assert_eq!(AttendeeList::new().status_of(0), None);
    /// ```
    pub fn status_of(&self, i: usize) -> Option<PresenceStatus> {
        self.attendees.get(i).map(|a| a.status)
    }

    /// Drains the last clicked row index.
    ///
    /// ```
    /// use martensite::widgets::attendee_list::AttendeeList;
    ///
    /// let mut a = AttendeeList::new();
    /// assert_eq!(a.take_selected(), None);
    /// ```
    pub fn take_selected(&mut self) -> Option<usize> {
        self.selected.take()
    }

    fn hit(&self, p: Vec2) -> Option<usize> {
        self.rows.iter().position(|r| r.contains(p))
    }
}

impl Widget for AttendeeList {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let s = cx.scale;
        let rows = self.attendees.len().max(1) as f32;
        Vec2::new(
            (220.0 * s).min(constraints.max_size.x.max(0.0)),
            ((rows * self.row_height + PAD_PT * 2.0) * s).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(160.0, ROW_PT + PAD_PT * 2.0))
            .with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        let s = cx.scale;
        let row_h = self.row_height * s;
        self.rows.clear();
        for i in 0..self.attendees.len() {
            self.rows.push(Rect::new(
                bounds.min_x() + PAD_PT * s,
                bounds.min_y() + PAD_PT * s + i as f32 * row_h,
                (bounds.width() - PAD_PT * 2.0 * s).max(0.0),
                row_h,
            ));
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::List);
        node.set_label(self.label.clone());
        node.set_value(format!("{} attendees", self.attendees.len()));
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
                    self.selected = Some(i);
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
        let shape = martensite_core::shape::Shape::rounded(6.0 * s);
        for (i, a) in self.attendees.iter().enumerate() {
            let r = self.rows[i];
            let kr = kurbo::Rect::new(
                f64::from(r.min_x()),
                f64::from(r.min_y()),
                f64::from(r.max_x()),
                f64::from(r.max_y()),
            );
            if a.speaking {
                cx.list
                    .push_fill_shape(kr, &shape, cx.color(TokenKey::SuccessColor, SPEAKING));
            } else if self.hovered == Some(i) {
                cx.list.push_fill_shape(kr, &shape, ROW_HOVER);
            }
            // Presence dot.
            let d = DOT_PT * s;
            let dr = kurbo::Rect::new(
                f64::from(r.min_x() + 8.0 * s),
                f64::from(r.min_y() + (r.height() - d) / 2.0),
                f64::from(r.min_x() + 8.0 * s + d),
                f64::from(r.min_y() + (r.height() + d) / 2.0),
            );
            cx.list.push_fill_shape(
                dr,
                &martensite_core::shape::Shape::ELLIPSE,
                cx.color(a.status.token(), a.status.color()),
            );
            // Name.
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                kr,
                kurbo::Point::new(
                    f64::from(r.min_x() + 8.0 * s + d + 8.0 * s),
                    f64::from(r.min_y() + r.height() * 0.72),
                ),
                &a.name,
                FONT_PT * s,
                cx.color(TokenKey::TextColor, TEXT),
            );
            // Badges, right-aligned: muted "✕", hand "✋".
            let mut x = r.max_x() - 8.0 * s;
            for (show, glyph) in [(a.muted, "✕"), (a.hand_raised, "✋")] {
                if !show {
                    continue;
                }
                let w = BADGE_PT * s;
                let br = kurbo::Rect::new(
                    f64::from(x - w),
                    f64::from(r.min_y() + (r.height() - w) / 2.0),
                    f64::from(x),
                    f64::from(r.min_y() + (r.height() + w) / 2.0),
                );
                cx.list.push_fill_shape(br, &shape, BADGE_BG);
                crate::text_paint::paint_label(
                    painter,
                    cx.list,
                    kurbo::Point::new(br.x0 + f64::from(w) * 0.22, br.y0 + f64::from(w) * 0.74),
                    glyph,
                    FONT_PT * s,
                    BADGE_FG,
                );
                x -= w + 4.0 * s;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn fixture() -> AttendeeList {
        AttendeeList::new()
            .attendee(Attendee::new("Ana"))
            .attendee(Attendee::new("Ben").muted(true))
            .attendee(Attendee::new("Cat").hand_raised(true).speaking(true))
    }

    fn laid_out(a: &mut AttendeeList) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        a.layout(&mut cx, Rect::new(0.0, 0.0, 240.0, 160.0));
    }

    fn release(a: &mut AttendeeList, i: usize) {
        let r = a.rows[i];
        a.event(&mut EventContext {
            event: &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: Vec2::new((r.min_x() + r.max_x()) / 2.0, (r.min_y() + r.max_y()) / 2.0),
            },
            bounds: a.bounds,
            scale: 1.0,
        });
    }

    #[test]
    fn rows_stack() {
        let mut a = fixture();
        laid_out(&mut a);
        assert_eq!(a.rows.len(), 3);
        assert!(a.rows[1].min_y() > a.rows[0].min_y());
    }

    #[test]
    fn click_selects_row() {
        let mut a = fixture();
        laid_out(&mut a);
        release(&mut a, 2);
        assert_eq!(a.take_selected(), Some(2));
        assert_eq!(a.take_selected(), None);
    }

    #[test]
    fn raised_hands_lists_indices() {
        let a = fixture();
        assert_eq!(a.raised_hands(), &[2]);
    }

    #[test]
    fn setters_update_rows() {
        let mut a = fixture();
        a.set_muted(0, true);
        a.set_hand(0, true);
        a.set_status(0, PresenceStatus::Busy);
        assert!(a.is_muted(0));
        assert_eq!(a.status_of(0), Some(PresenceStatus::Busy));
        assert_eq!(a.raised_hands(), &[0, 2]);
    }

    #[test]
    fn hover_tracks_pointer() {
        let mut a = fixture();
        laid_out(&mut a);
        let r = a.rows[1];
        a.event(&mut EventContext {
            event: &WidgetEvent::PointerMoved {
                position: Vec2::new(r.min_x() + 4.0, r.min_y() + 4.0),
            },
            bounds: a.bounds,
            scale: 1.0,
        });
        assert_eq!(a.hovered, Some(1));
    }

    #[test]
    fn paint_without_painter() {
        let mut a = fixture();
        laid_out(&mut a);
        let mut list = martensite_core::PaintList::default();
        let theme = martensite_theme::Theme::new("test");
        a.paint(&mut PaintContext {
            list: &mut list,
            bounds: a.bounds,
            theme: &theme,
            scale: 1.0,
            text_painter: None,
        });
        assert!(!list.is_empty());
    }
}
