//! `Presence` — a user-presence chip: initials disc with a status
//! dot plus an optional name/status line (Teams/Slack presence
//! idiom).
//!
//! Distinct from [`Avatar`](crate::widgets::Avatar): presence is
//! about the *state* — a [`PresenceStatus`] colored dot with a
//! contrasting ring and a text line — where Avatar is image/initial
//! chrome. No image pipeline here; the disc paints initials.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::presence::{Presence, PresenceStatus};
//!
//! let p = Presence::new("Ada Lovelace", PresenceStatus::Online);
//! assert_eq!(p.status(), PresenceStatus::Online);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::text_paint::SharedTextPainter;

const DISC_PT: f32 = 32.0;
const DOT_PT: f32 = 11.0;
const GAP_PT: f32 = 9.0;
const NAME_PT: f32 = 13.0;
const SUB_PT: f32 = 10.0;

const DISC: [u8; 4] = [88, 130, 247, 255];
const TEXT: [u8; 4] = [220, 222, 228, 255];
const MUTED: [u8; 4] = [139, 148, 158, 255];
const SURFACE: [u8; 4] = [30, 31, 35, 255];
const ONLINE: [u8; 4] = [39, 201, 63, 255];
const AWAY: [u8; 4] = [255, 189, 46, 255];
const BUSY: [u8; 4] = [255, 95, 86, 255];
const OFFLINE: [u8; 4] = [120, 124, 132, 255];

/// User availability state.
///
/// ```
/// use martensite::widgets::presence::PresenceStatus;
///
/// assert_ne!(PresenceStatus::Online, PresenceStatus::Offline);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PresenceStatus {
    /// Green — available.
    #[default]
    Online,
    /// Yellow — idle/away.
    Away,
    /// Red — do not disturb.
    Busy,
    /// Grey — signed out.
    Offline,
}

impl PresenceStatus {
    /// Fallback dot color.
    pub(crate) fn color(self) -> [u8; 4] {
        match self {
            Self::Online => ONLINE,
            Self::Away => AWAY,
            Self::Busy => BUSY,
            Self::Offline => OFFLINE,
        }
    }

    /// Theme token for the dot.
    pub(crate) fn token(self) -> TokenKey {
        match self {
            Self::Online => TokenKey::SuccessColor,
            Self::Away => TokenKey::WarningColor,
            Self::Busy => TokenKey::ErrorColor,
            Self::Offline => TokenKey::TextMutedColor,
        }
    }

    /// Human-readable label.
    fn text(self) -> &'static str {
        match self {
            Self::Online => "Online",
            Self::Away => "Away",
            Self::Busy => "Do not disturb",
            Self::Offline => "Offline",
        }
    }
}

/// A presence chip — see the module docs.
///
/// ```
/// use martensite::widgets::presence::Presence;
///
/// assert_eq!(Presence::new("A", Default::default()).initials(), "A");
/// ```
pub struct Presence {
    /// Accessibility label; defaults to `"{name} — {status}"`.
    pub label: Option<String>,
    name: String,
    status: PresenceStatus,
    /// Show the name + status text line.
    pub show_text: bool,
    /// Custom status line (defaults to the status label).
    pub status_text: Option<String>,
    clicked: bool,
    text_painter: Option<SharedTextPainter>,
    bounds: Rect,
    scale: f32,
}

impl std::fmt::Debug for Presence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Presence")
            .field("name", &self.name)
            .field("status", &self.status)
            .finish()
    }
}

impl Presence {
    /// Presence chip for `name` in `status`.
    ///
    /// ```
    /// use martensite::widgets::presence::{Presence, PresenceStatus};
    ///
    /// assert_eq!(Presence::new("Ada", PresenceStatus::Away).status(), PresenceStatus::Away);
    /// ```
    pub fn new(name: impl Into<String>, status: PresenceStatus) -> Self {
        Self {
            label: None,
            name: name.into(),
            status,
            show_text: true,
            status_text: None,
            clicked: false,
            text_painter: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Hides the text line (dot + disc only).
    ///
    /// ```
    /// use martensite::widgets::presence::Presence;
    ///
    /// assert!(!Presence::new("A", Default::default()).show_text(false).show_text);
    /// ```
    pub fn show_text(mut self, show: bool) -> Self {
        self.show_text = show;
        self
    }

    /// Overrides the status line text.
    ///
    /// ```
    /// use martensite::widgets::presence::Presence;
    ///
    /// assert_eq!(
    ///     Presence::new("A", Default::default()).status_text("In a call").status_text.as_deref(),
    ///     Some("In a call")
    /// );
    /// ```
    pub fn status_text(mut self, text: impl Into<String>) -> Self {
        self.status_text = Some(text.into());
        self
    }

    /// Accessibility label override.
    ///
    /// ```
    /// use martensite::widgets::presence::Presence;
    ///
    /// assert_eq!(Presence::new("A", Default::default()).label("Me").label.as_deref(), Some("Me"));
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Shared text painter for real glyph metrics.
    ///
    /// ```no_run
    /// use martensite::widgets::presence::Presence;
    /// # fn example(p: martensite::text_paint::SharedTextPainter) {
    /// let _p = Presence::new("A", Default::default()).with_text_painter(p);
    /// # }
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// The displayed name.
    ///
    /// ```
    /// use martensite::widgets::presence::Presence;
    ///
    /// assert_eq!(Presence::new("Ada", Default::default()).name(), "Ada");
    /// ```
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Current availability.
    ///
    /// ```
    /// use martensite::widgets::presence::{Presence, PresenceStatus};
    ///
    /// assert_eq!(Presence::new("A", PresenceStatus::Busy).status(), PresenceStatus::Busy);
    /// ```
    pub fn status(&self) -> PresenceStatus {
        self.status
    }

    /// Updates the availability.
    ///
    /// ```
    /// use martensite::widgets::presence::{Presence, PresenceStatus};
    ///
    /// let mut p = Presence::new("A", PresenceStatus::Online);
    /// p.set_status(PresenceStatus::Offline);
    /// assert_eq!(p.status(), PresenceStatus::Offline);
    /// ```
    pub fn set_status(&mut self, status: PresenceStatus) {
        self.status = status;
    }

    /// One- or two-letter initials derived from the name.
    ///
    /// ```
    /// use martensite::widgets::presence::Presence;
    ///
    /// assert_eq!(Presence::new("Ada Lovelace", Default::default()).initials(), "AL");
    /// ```
    pub fn initials(&self) -> String {
        let mut it = self
            .name
            .split_whitespace()
            .filter_map(|w| w.chars().next())
            .take(2);
        let mut s = String::new();
        s.extend(it.by_ref().map(|c| c.to_ascii_uppercase()));
        if s.is_empty() {
            s.push('?');
        }
        s
    }

    /// Drains a chip click.
    ///
    /// ```
    /// use martensite::widgets::presence::Presence;
    ///
    /// let mut p = Presence::new("A", Default::default());
    /// assert!(!p.take_clicked());
    /// ```
    pub fn take_clicked(&mut self) -> bool {
        std::mem::take(&mut self.clicked)
    }
}

impl Widget for Presence {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let s = cx.scale;
        let disc = DISC_PT * s;
        let text_w = if self.show_text {
            let name_w = self
                .text_painter
                .as_ref()
                .map(|p| p.measure(&self.name, NAME_PT * s))
                .unwrap_or_else(|| self.name.chars().count() as f32 * NAME_PT * 0.6 * s);
            let sub = self
                .status_text
                .clone()
                .unwrap_or_else(|| self.status.text().to_string());
            let sub_w = self
                .text_painter
                .as_ref()
                .map(|p| p.measure(&sub, SUB_PT * s))
                .unwrap_or_else(|| sub.chars().count() as f32 * SUB_PT * 0.6 * s);
            name_w.max(sub_w) + GAP_PT * s
        } else {
            0.0
        };
        Vec2::new(
            (disc + text_w).min(constraints.max_size.x.max(0.0)),
            disc.min(constraints.max_size.y.max(0.0)),
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
        node.set_role(accesskit::Role::ListItem);
        let label = self
            .label
            .clone()
            .unwrap_or_else(|| format!("{} — {}", self.name, self.status.text()));
        node.set_label(label);
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if let WidgetEvent::PointerReleased {
            button: martensite_core::PointerButton::Primary,
            position,
        } = cx.event
        {
            if self.bounds.contains(*position) {
                self.clicked = true;
                return EventResponse::Handled;
            }
        }
        EventResponse::Ignored
    }

    fn paint(&self, cx: &mut PaintContext) {
        let s = cx.scale;
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let d = DISC_PT * s;
        // Initials disc.
        let disc = kurbo::Rect::new(
            f64::from(self.bounds.min_x()),
            f64::from(self.bounds.min_y()),
            f64::from(self.bounds.min_x() + d),
            f64::from(self.bounds.min_y() + d),
        );
        cx.list.push_fill_shape(
            disc,
            &martensite_core::shape::Shape::ELLIPSE,
            cx.color(TokenKey::AccentColor, DISC),
        );
        // Initials centered.
        let initials = self.initials();
        let size = NAME_PT * s;
        let iw = painter
            .and_then(|p| p.measure_text(&initials, size))
            .unwrap_or(initials.len() as f32 * size * 0.6);
        let io = kurbo::Point::new(
            f64::from(self.bounds.min_x() + (d - iw) / 2.0),
            f64::from(self.bounds.min_y() + d / 2.0),
        );
        crate::text_paint::paint_label(
            painter,
            cx.list,
            io,
            &initials,
            size,
            cx.color(TokenKey::TextInverseColor, TEXT),
        );
        // Status dot, bottom-right with a surface ring.
        let dd = DOT_PT * s;
        let dx = self.bounds.min_x() + d - dd * 0.9;
        let dy = self.bounds.min_y() + d - dd * 0.9;
        let ring = kurbo::Rect::new(
            f64::from(dx - s),
            f64::from(dy - s),
            f64::from(dx + dd + s),
            f64::from(dy + dd + s),
        );
        cx.list.push_fill_shape(
            ring,
            &martensite_core::shape::Shape::ELLIPSE,
            cx.color(TokenKey::BackgroundColor, SURFACE),
        );
        let dot = kurbo::Rect::new(
            f64::from(dx),
            f64::from(dy),
            f64::from(dx + dd),
            f64::from(dy + dd),
        );
        cx.list.push_fill_shape(
            dot,
            &martensite_core::shape::Shape::ELLIPSE,
            cx.color(self.status.token(), self.status.color()),
        );
        // Name + status line — clipped to the widget so a shallow
        // allocation cuts the sub-line cleanly rather than spilling.
        if self.show_text {
            let wclip = kurbo::Rect::new(
                f64::from(self.bounds.min_x()),
                f64::from(self.bounds.min_y()),
                f64::from(self.bounds.max_x()),
                f64::from(self.bounds.max_y()),
            );
            let tx = self.bounds.min_x() + d + GAP_PT * s;
            let name_o = kurbo::Point::new(
                f64::from(tx),
                f64::from(self.bounds.min_y() + d / 2.0 - 4.0 * s),
            );
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                wclip,
                name_o,
                &self.name,
                size,
                cx.color(TokenKey::TextColor, TEXT),
            );
            let sub = self
                .status_text
                .clone()
                .unwrap_or_else(|| self.status.text().to_string());
            let sub_o = kurbo::Point::new(
                f64::from(tx),
                f64::from(self.bounds.min_y() + d / 2.0 + SUB_PT * s),
            );
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                wclip,
                sub_o,
                &sub,
                SUB_PT * s,
                cx.color(TokenKey::TextMutedColor, MUTED),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::{HotNode, PointerButton};

    fn laid_out(p: &mut Presence) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        p.layout(&mut cx, Rect::new(0.0, 0.0, 200.0, 32.0));
    }

    #[test]
    fn initials_take_two_letters() {
        assert_eq!(
            Presence::new("Ada Lovelace", PresenceStatus::Online).initials(),
            "AL"
        );
        assert_eq!(Presence::new("ada", PresenceStatus::Online).initials(), "A");
        assert_eq!(Presence::new("", PresenceStatus::Online).initials(), "?");
    }

    #[test]
    fn click_parks() {
        let mut p = Presence::new("Ada", PresenceStatus::Online);
        laid_out(&mut p);
        p.event(&mut EventContext {
            event: &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: Vec2::new(10.0, 10.0),
            },
            bounds: p.bounds,
            scale: 1.0,
        });
        assert!(p.take_clicked());
        assert!(!p.take_clicked());
    }

    #[test]
    fn paint_without_painter() {
        let mut p = Presence::new("Ada Lovelace", PresenceStatus::Busy);
        laid_out(&mut p);
        let mut list = martensite_core::PaintList::default();
        let theme = martensite_theme::Theme::new("test");
        p.paint(&mut PaintContext {
            list: &mut list,
            bounds: p.bounds,
            theme: &theme,
            scale: 1.0,
            text_painter: None,
        });
        assert!(!list.is_empty());
    }
}
