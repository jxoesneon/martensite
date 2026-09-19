//! `Chip` widget: a compact pill for input, selection, or actions
//! (Material 3 chips / Ant Design `Tag`).
//!
//! Chips cover the M3 chip set via [`ChipKind`]: `Assist` and
//! `Suggestion` act like buttons, `Filter` toggles with a leading
//! check mark and `CheckBox` a11y semantics, `Input` represents an
//! entered entity and is typically [`Chip::deletable`]. Any chip can
//! show a trailing ✕ delete target — poll [`Chip::take_deleted`]
//! after dispatch to learn when it was hit, and
//! [`Chip::take_selected`] for selection changes.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::chip::{Chip, ChipKind};
//!
//! let c = Chip::new("Sci-Fi").kind(ChipKind::Filter).selected(true);
//! assert!(c.selected);
//! ```

use accesskit::{Node as AccessKitNode, Toggled};
use glam::Vec2;
use martensite_core::shape::Shape;
use martensite_core::widget::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    SemanticAction, Widget, WidgetEvent,
};
use martensite_core::{NodeFlags, Rect, TokenKey};

const INK: [u8; 4] = [20, 20, 25, 255];
const MUTED: [u8; 4] = [160, 160, 165, 255];
const EDGE: [u8; 4] = [140, 145, 155, 255];
const SURFACE: [u8; 4] = [248, 249, 251, 255];
const ACCENT: [u8; 4] = [40, 110, 220, 255];
/// Chip height in logical points (M3 spec: 32pt; 28 reads better in
/// dense tool UIs — matches this crate's other compact controls).
const HEIGHT: f32 = 28.0;
/// Horizontal padding inside the pill (logical points).
const PAD_X: f32 = 12.0;
/// Gap between the check mark / close target and the label.
const GLYPH_GAP: f32 = 6.0;
/// Width reserved for the selected filter chip's check mark.
const CHECK_W: f32 = 14.0;
/// Close-button hit box (logical points square).
const CLOSE: f32 = 18.0;
/// Label text size in logical points.
const LABEL_SIZE: f32 = 13.0;

/// Which Material 3 chip role a [`Chip`] plays.
///
/// # Examples
///
/// ```
/// use martensite::widgets::chip::ChipKind;
///
/// assert_eq!(ChipKind::default(), ChipKind::Assist);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ChipKind {
    /// Smart/action chip — behaves like a button.
    #[default]
    Assist,
    /// Toggles a filter — shows a leading check mark while selected
    /// and exposes `Role::CheckBox`.
    Filter,
    /// A user-entered entity (contact, tag) — typically deletable.
    Input,
    /// A suggested value — behaves like a button.
    Suggestion,
}

/// A compact pill-shaped chip with a label, an optional leading check
/// mark (selected [`ChipKind::Filter`]), and an optional trailing
/// delete ✕.
///
/// # Examples
///
/// ```
/// use martensite::widgets::{Chip, ChipKind};
///
/// let c = Chip::new("Tagged")
///     .kind(ChipKind::Input)
///     .deletable(true);
/// assert!(!c.is_deleted());
/// ```
#[derive(Clone)]
pub struct Chip {
    /// Which M3 chip role this chip plays.
    pub kind: ChipKind,
    /// The label text.
    pub label: String,
    /// Whether the chip is selected.
    pub selected: bool,
    /// Whether the trailing ✕ delete target is shown.
    pub deletable: bool,
    /// Whether the chip is enabled.
    pub enabled: bool,
    /// Set when the user hit the delete target (take via
    /// `take_deleted`).
    deleted: bool,
    /// New `selected` value since the last `take_selected`.
    selected_changed: Option<bool>,
    /// Cached bounds from the last layout pass.
    cached_bounds: Rect,
    /// Cached close-button rect (device px).
    close_rect: Rect,
    /// Shared shaped-text painter.
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl Chip {
    /// A chip with the given label — an [`ChipKind::Assist`] chip by
    /// default.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Chip, ChipKind};
    ///
    /// let c = Chip::new("Help");
    /// assert_eq!(c.kind, ChipKind::Assist);
    /// assert!(!c.selected);
    /// ```
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            kind: ChipKind::default(),
            label: label.into(),
            selected: false,
            deletable: false,
            enabled: true,
            deleted: false,
            selected_changed: None,
            cached_bounds: Rect::default(),
            close_rect: Rect::default(),
            text_painter: None,
        }
    }

    /// Sets the chip kind.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Chip, ChipKind};
    ///
    /// let c = Chip::new("Year: 2024").kind(ChipKind::Filter);
    /// assert_eq!(c.kind, ChipKind::Filter);
    /// ```
    #[inline]
    #[must_use]
    pub fn kind(mut self, kind: ChipKind) -> Self {
        self.kind = kind;
        self
    }

    /// Sets the selected state.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Chip;
    ///
    /// let c = Chip::new("Wi-Fi").selected(true);
    /// assert!(c.selected);
    /// ```
    #[inline]
    #[must_use]
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// Sets whether the delete ✕ is shown.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Chip;
    ///
    /// let c = Chip::new("alice@x.io").deletable(true);
    /// assert!(c.deletable);
    /// ```
    #[inline]
    #[must_use]
    pub fn deletable(mut self, deletable: bool) -> Self {
        self.deletable = deletable;
        self
    }

    /// Sets whether the chip is enabled.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Chip;
    ///
    /// let c = Chip::new("Locked").enabled(false);
    /// assert!(!c.enabled);
    /// ```
    #[inline]
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Whether the user hit the delete target.
    pub fn is_deleted(&self) -> bool {
        self.deleted
    }

    /// Clears and returns the deleted flag — the host's cue to remove
    /// the chip (mirrors [`crate::widgets::banner::Banner::take_dismissed`]).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Chip;
    ///
    /// let mut c = Chip::new("x");
    /// assert!(!c.take_deleted());
    /// ```
    pub fn take_deleted(&mut self) -> bool {
        std::mem::take(&mut self.deleted)
    }

    /// Returns the new `selected` value once after a user toggle, if
    /// any (mirrors the `take_*` out-seam pattern).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Chip;
    ///
    /// let mut c = Chip::new("x");
    /// assert_eq!(c.take_selected(), None);
    /// ```
    pub fn take_selected(&mut self) -> Option<bool> {
        self.selected_changed.take()
    }

    /// Shares a [`crate::text_paint::TextPainter`] for real glyph runs.
    #[must_use]
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Whether the leading check mark zone is reserved (selected
    /// filter chip).
    fn has_check(&self) -> bool {
        matches!(self.kind, ChipKind::Filter) && self.selected
    }

    /// The close-button rect (device px) from the last layout.
    fn close_target(&self, scale: f32) -> Rect {
        let side = CLOSE * scale;
        Rect::new(
            self.cached_bounds.max_x() - side - PAD_X * scale,
            self.cached_bounds.origin.y + (self.cached_bounds.size.y - side) / 2.0,
            side,
            side,
        )
    }
}

impl Widget for Chip {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let mut w = cx.pt(PAD_X * 2.0);
        if self.has_check() {
            w += cx.pt(CHECK_W + GLYPH_GAP);
        }
        // ~8pt per character — the same estimate the other label
        // widgets use until a TextPainter supplies real metrics.
        w += cx.pt(8.0 * self.label.chars().count() as f32);
        if self.deletable {
            w += cx.pt(GLYPH_GAP + CLOSE);
        }
        Vec2::new(
            w.min(constraints.max_size.x.max(0.0)),
            cx.pt(HEIGHT).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.cached_bounds = bounds;
        self.close_rect = if self.deletable {
            self.close_target(cx.scale)
        } else {
            Rect::default()
        };
        if self.enabled {
            cx.hot.flags |= NodeFlags::FOCUSABLE;
        } else {
            cx.hot.flags.remove(NodeFlags::FOCUSABLE);
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        // Filter chips are toggles; the rest are buttons.
        if matches!(self.kind, ChipKind::Filter) {
            node.set_role(accesskit::Role::CheckBox);
            node.set_toggled(if self.selected {
                Toggled::True
            } else {
                Toggled::False
            });
        } else {
            node.set_role(accesskit::Role::Button);
            if self.selected {
                node.set_selected(true);
            }
        }
        node.set_label(self.label.as_str());
        node.add_action(accesskit::Action::Click);
        node.add_action(accesskit::Action::Focus);
        if !self.enabled {
            node.set_disabled();
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.enabled {
            return EventResponse::Ignored;
        }
        match cx.event {
            WidgetEvent::PointerReleased {
                position,
                button: PointerButton::Primary,
            } => {
                // The ✕ delete target wins over the toggle body.
                if self.deletable && self.close_rect.contains(*position) {
                    self.deleted = true;
                    return EventResponse::RequestRepaint;
                }
                self.selected = !self.selected;
                self.selected_changed = Some(self.selected);
                EventResponse::RequestRepaint
            }
            WidgetEvent::KeyPressed { .. } | WidgetEvent::SemanticAction(SemanticAction::Click) => {
                self.selected = !self.selected;
                self.selected_changed = Some(self.selected);
                EventResponse::RequestRepaint
            }
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        let rect = kurbo::Rect::new(
            f64::from(b.origin.x),
            f64::from(b.origin.y),
            f64::from(b.max_x()),
            f64::from(b.max_y()),
        );
        let pill = Shape::PILL;

        let accent = cx.color(TokenKey::AccentColor, ACCENT);
        let (fill, ink, edge) = if !self.enabled {
            (
                cx.color(TokenKey::SurfaceColor, SURFACE),
                cx.color(TokenKey::TextMutedColor, MUTED),
                cx.color(TokenKey::DividerColor, EDGE),
            )
        } else if self.selected {
            // Accent tint fill + accent ink — the M3 selected chip.
            let mut tint = accent;
            tint[3] = 56;
            (tint, accent, accent)
        } else {
            (
                cx.color(TokenKey::SurfaceColor, SURFACE),
                cx.color(TokenKey::TextColor, INK),
                cx.color(TokenKey::BorderColor, EDGE),
            )
        };
        cx.list.push_fill_shape(rect, &pill, fill);
        cx.list.push_stroke_shape(rect, &pill, cx.pt(1.0), edge);

        // Leading check mark for a selected filter chip.
        let mut text_x = b.origin.x + cx.pt(PAD_X);
        if self.has_check() {
            let s = cx.pt(8.0);
            let cy = b.origin.y + b.size.y / 2.0;
            let mut tick = kurbo::BezPath::new();
            let x0 = f64::from(text_x);
            tick.move_to((x0, f64::from(cy)));
            tick.line_to((x0 + f64::from(s) * 0.4, f64::from(cy) + f64::from(s) * 0.4));
            tick.line_to((x0 + f64::from(s), f64::from(cy) - f64::from(s) * 0.5));
            cx.list.push_stroke_path(tick, cx.pt(1.75), ink);
            text_x += cx.pt(CHECK_W + GLYPH_GAP);
        }

        // Trailing delete ✕ — label clips before it.
        let text_right = if self.deletable {
            let cr = self.close_rect;
            let arm = 4.0 * cx.scale;
            let mid = cr.origin + cr.size / 2.0;
            let mut x = kurbo::BezPath::new();
            x.move_to((f64::from(mid.x - arm), f64::from(mid.y - arm)));
            x.line_to((f64::from(mid.x + arm), f64::from(mid.y + arm)));
            x.move_to((f64::from(mid.x + arm), f64::from(mid.y - arm)));
            x.line_to((f64::from(mid.x - arm), f64::from(mid.y + arm)));
            cx.list.push_stroke_path(x, cx.pt(1.5), ink);
            cr.origin.x - cx.pt(GLYPH_GAP)
        } else {
            b.max_x() - cx.pt(PAD_X)
        };

        crate::text_paint::paint_label_clipped(
            crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter),
            cx.list,
            kurbo::Rect::new(
                f64::from(text_x),
                f64::from(b.origin.y),
                f64::from(text_right),
                f64::from(b.max_y()),
            ),
            kurbo::Point::new(
                f64::from(text_x),
                f64::from(b.origin.y + (b.size.y - cx.pt(LABEL_SIZE)) / 2.0),
            ),
            &self.label,
            cx.pt(LABEL_SIZE),
            ink,
        );
    }

    fn hit_shape(&self) -> Option<martensite_core::shape::Shape> {
        // The painted silhouette is a stadium — accept input exactly
        // where the chip is visible.
        Some(Shape::PILL)
    }
}

impl std::fmt::Debug for Chip {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Chip")
            .field("kind", &self.kind)
            .field("label", &self.label)
            .field("selected", &self.selected)
            .field("deletable", &self.deletable)
            .field("enabled", &self.enabled)
            .field("deleted", &self.deleted)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn make_cx(hot: &mut HotNode) -> LayoutContext<'_> {
        LayoutContext { hot, scale: 1.0 }
    }

    #[test]
    fn chip_new() {
        let c = Chip::new("Tag");
        assert_eq!(c.kind, ChipKind::Assist);
        assert!(!c.selected);
        assert!(!c.deletable);
        assert!(c.enabled);
    }

    #[test]
    fn chip_builders() {
        let c = Chip::new("X")
            .kind(ChipKind::Input)
            .selected(true)
            .deletable(true)
            .enabled(false);
        assert_eq!(c.kind, ChipKind::Input);
        assert!(c.selected);
        assert!(c.deletable);
        assert!(!c.enabled);
    }

    #[test]
    fn chip_toggle_via_release() {
        let mut c = Chip::new("F").kind(ChipKind::Filter);
        let mut ecx = EventContext {
            event: &WidgetEvent::PointerReleased {
                position: Vec2::new(4.0, 4.0),
                button: PointerButton::Primary,
            },
            bounds: Rect::new(0.0, 0.0, 80.0, 28.0),
            scale: 1.0,
        };
        assert_eq!(c.event(&mut ecx), EventResponse::RequestRepaint);
        assert!(c.selected);
        assert_eq!(c.take_selected(), Some(true));
        assert_eq!(c.take_selected(), None);
    }

    #[test]
    fn chip_delete_via_close_target() {
        let mut hot = HotNode::new(taffy::NodeId::new(1));
        let mut cx = make_cx(&mut hot);
        let mut c = Chip::new("Victim").deletable(true);
        c.layout(&mut cx, Rect::new(0.0, 0.0, 100.0, 28.0));
        let cr = c.close_rect;
        let mut ecx = EventContext {
            event: &WidgetEvent::PointerReleased {
                position: Vec2::new(cr.origin.x + 2.0, cr.origin.y + 2.0),
                button: PointerButton::Primary,
            },
            bounds: Rect::new(0.0, 0.0, 100.0, 28.0),
            scale: 1.0,
        };
        assert_eq!(c.event(&mut ecx), EventResponse::RequestRepaint);
        assert!(c.take_deleted());
        assert!(!c.take_deleted());
        // Delete does not toggle selection.
        assert!(!c.selected);
    }

    #[test]
    fn chip_disabled_ignores() {
        let mut c = Chip::new("X").enabled(false);
        let mut ecx = EventContext {
            event: &WidgetEvent::PointerReleased {
                position: Vec2::new(4.0, 4.0),
                button: PointerButton::Primary,
            },
            bounds: Rect::new(0.0, 0.0, 80.0, 28.0),
            scale: 1.0,
        };
        assert_eq!(c.event(&mut ecx), EventResponse::Ignored);
    }

    #[test]
    fn chip_filter_a11y_is_checkbox() {
        let c = Chip::new("F").kind(ChipKind::Filter).selected(true);
        let mut node = accesskit::Node::new(accesskit::Role::Unknown);
        c.accessibility(&mut node);
        assert_eq!(node.role(), accesskit::Role::CheckBox);
        assert_eq!(node.toggled(), Some(Toggled::True));
    }

    #[test]
    fn chip_assist_a11y_is_button() {
        let c = Chip::new("A");
        let mut node = accesskit::Node::new(accesskit::Role::Unknown);
        c.accessibility(&mut node);
        assert_eq!(node.role(), accesskit::Role::Button);
        assert_eq!(node.label(), Some("A"));
    }

    #[test]
    fn chip_measure_respects_max() {
        let mut hot = HotNode::new(taffy::NodeId::new(1));
        let mut cx = make_cx(&mut hot);
        let mut c = Chip::new("A moderately long label");
        let size = c.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(60.0, 100.0),
            },
        );
        assert_eq!(size.x, 60.0);
        assert_eq!(size.y, 28.0);
    }

    #[test]
    fn chip_hit_shape_is_pill() {
        let c = Chip::new("x");
        assert_eq!(Widget::hit_shape(&c), Some(Shape::PILL));
    }

    #[test]
    fn chip_debug_format() {
        let c = Chip::new("D").kind(ChipKind::Input);
        let debug = format!("{:?}", c);
        assert!(debug.contains("Chip"));
        assert!(debug.contains("Input"));
    }
}
