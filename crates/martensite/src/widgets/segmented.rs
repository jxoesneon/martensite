//! `Segmented` widget: a single-select pill strip.
//!
//! The `Segmented` control is the joined-strip form of a radio group —
//! NSSegmentedControl, Ant Design `Segmented`, and Material 3's
//! segmented button set: one rounded strip divided into equal
//! segments, exactly one selected at a time.
//!
//! Semantics follow [`RadioGroup`](crate::widgets::RadioGroup) (the
//! same single-select contract, painted as a strip instead of a row of
//! circles):
//!
//! - The group emits `Role::RadioGroup`; each segment emits
//!   `Role::RadioButton` with `Toggled` checked state.
//! - **Roving tabindex**: the strip is a single tab stop — the
//!   selected (or, before first selection, the focused) segment
//!   carries the `Focus` action.
//! - Arrow keys move the focus indicator *and* select the newly
//!   focused segment (APG *automatic* activation); `Space`/`Enter`
//!   selects the focused segment; `Home`/`End` jump to the ends.
//! - `SemanticAction::Click` on a segment selects it.
//! - Selection changes are reported outward through
//!   [`take_selected`](Segmented::take_selected), the `take_*`
//!   signal seam used by `Dialog::take_response` and friends.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::Segmented;
//!
//! let seg = Segmented::new()
//!     .options(vec!["Day", "Week", "Month"])
//!     .selected(1);
//! assert_eq!(seg.selected_index(), 1);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::shape::{CornerRadii, CornerStyle, Shape};
use martensite_core::widget::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    SemanticAction, Widget, WidgetEvent,
};
use martensite_core::{NodeFlags, Rect, TokenKey};

use crate::widgets::FlexDirection;

/// Strip height in logical points.
const STRIP_H: f32 = 28.0;
/// Minimum segment width in logical points (a comfortable hit target).
const SEG_MIN_W: f32 = 48.0;
/// Horizontal label padding inside a segment.
const TEXT_PAD_X: f32 = 10.0;
/// Strip border colour.
const EDGE: [u8; 4] = [140, 145, 155, 255];
/// Strip track colour (the unselected background).
const TRACK: [u8; 4] = [232, 234, 238, 255];
/// Selected segment fill.
const SELECTED: [u8; 4] = [60, 110, 220, 255];
/// Label ink on the track.
const INK: [u8; 4] = [20, 20, 25, 255];
/// Label ink on the selected segment.
const INK_SELECTED: [u8; 4] = [255, 255, 255, 255];
/// Focus ring colour (translucent accent wash).
const FOCUS_RING: [u8; 4] = [60, 110, 220, 128];

/// One segment inside a [`Segmented`] strip — an internal child
/// emitted with `Role::RadioButton`.
///
/// The segment's checked/focused flags and geometry mirror group
/// state via [`Segmented::sync_options`]; pointer and AT activations
/// are parked (`activation_pending`/`focus_pending`) and applied by
/// the group so the single-checked invariant stays centralized —
/// the same pending-activation seam [`RadioOption`] uses.
///
/// [`RadioOption`]: crate::widgets::RadioOption
pub struct Segment {
    /// The segment's accessible label.
    label: String,
    /// Whether this segment is selected (mirrored from the group).
    selected: bool,
    /// Whether this segment carries the roving tabindex.
    focused: bool,
    /// 0-based position in the segment set (AccessKit convention).
    pos_in_set: usize,
    /// Total segment count.
    set_size: usize,
    /// Which outer end this segment caps, if any — drives the
    /// selected fill's per-corner radii.
    end: SegmentEnd,
    /// Strip direction (mirrored from the group).
    direction: FlexDirection,
    /// Whether the group is enabled.
    enabled: bool,
    /// A `SemanticAction::Click` or pointer press parked for the
    /// owning group to apply.
    activation_pending: bool,
    /// A `SemanticAction::Focus` parked for the owning group.
    focus_pending: bool,
    /// Shared shaped-text painter from the owning `Segmented`.
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

/// Which end of the strip a [`Segment`] caps — the selected fill and
/// hit silhouette round only the outer corners.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
enum SegmentEnd {
    /// An interior segment — all four corners square.
    #[default]
    Middle,
    /// The first segment — rounds the leading (left/top) end.
    First,
    /// The last segment — rounds the trailing (right/bottom) end.
    Last,
    /// A single-segment strip — rounds both ends.
    Only,
}

impl Segment {
    fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            selected: false,
            focused: false,
            pos_in_set: 0,
            set_size: 0,
            end: SegmentEnd::Middle,
            direction: FlexDirection::Row,
            enabled: true,
            activation_pending: false,
            focus_pending: false,
            text_painter: None,
        }
    }

    /// The fill/stroke silhouette for this segment: stadium ends on
    /// the outer caps, square on interior segments.
    fn fill_shape(&self, bounds: Rect) -> Shape {
        let radius = if self.direction.is_row() {
            bounds.height() / 2.0
        } else {
            bounds.width() / 2.0
        };
        let radii = match (self.end, self.direction.is_row()) {
            (SegmentEnd::Middle, _) => CornerRadii::ZERO,
            (SegmentEnd::First, true) => CornerRadii::left(radius),
            (SegmentEnd::Last, true) => CornerRadii::right(radius),
            (SegmentEnd::Only, true) => CornerRadii::uniform(radius),
            (SegmentEnd::First, false) => CornerRadii::top(radius),
            (SegmentEnd::Last, false) => CornerRadii::bottom(radius),
            (SegmentEnd::Only, false) => CornerRadii::uniform(radius),
        };
        Shape::corners(radii, CornerStyle::Round)
    }
}

impl Widget for Segment {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        // Approximate label width — real shaping lives in the
        // `martensite-text` pipeline; this is the same coarse
        // per-grapheme estimate `RadioOption` uses.
        let w = cx
            .pt(2.0 * TEXT_PAD_X + 8.0 * self.label.chars().count() as f32)
            .max(cx.pt(SEG_MIN_W));
        Vec2::new(
            w.min(constraints.max_size.x.max(0.0)),
            cx.pt(STRIP_H).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn layout(&mut self, _cx: &mut LayoutContext, _bounds: Rect) {}

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::RadioButton);
        node.set_label(self.label.as_str());
        node.set_toggled(accesskit::Toggled::from(self.selected));
        node.set_position_in_set(self.pos_in_set);
        node.set_size_of_set(self.set_size);
        node.add_action(accesskit::Action::Click);
        // Roving tabindex: only the segment holding the tab stop
        // advertises Focus — the strip is a single tab stop.
        if self.focused {
            node.add_action(accesskit::Action::Focus);
        }
        if !self.enabled {
            node.set_disabled();
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.enabled {
            return EventResponse::Ignored;
        }
        match cx.event {
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                ..
            } => {
                // Park the activation; the owning group applies it via
                // `poll_pending` so the single-checked invariant stays
                // centralized.
                self.activation_pending = true;
                EventResponse::CaptureFocus
            }
            WidgetEvent::SemanticAction(SemanticAction::Click) => {
                self.activation_pending = true;
                EventResponse::Handled
            }
            WidgetEvent::SemanticAction(SemanticAction::Focus) => {
                // Move the group's roving tabindex here and request
                // platform focus on the owning group node.
                self.focus_pending = true;
                EventResponse::CaptureFocus
            }
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        let rect = kurbo::Rect::new(
            f64::from(b.min_x()),
            f64::from(b.min_y()),
            f64::from(b.max_x()),
            f64::from(b.max_y()),
        );
        let shape = self.fill_shape(b);
        let accent = cx.color(TokenKey::AccentColor, SELECTED);
        if self.selected {
            // Inset the selected fill by a hair so the strip's outer
            // stroke still reads as one continuous outline.
            let inset = cx.ptf(1.5);
            let inner = kurbo::Rect::new(
                rect.x0 + inset,
                rect.y0 + inset,
                rect.x1 - inset,
                rect.y1 - inset,
            );
            cx.list.push_fill_shape(inner, &shape, accent);
        }
        if self.focused {
            // Translucent accent wash — the same focus ring RadioOption
            // paints, clipped to this segment's silhouette.
            let wash = [accent[0], accent[1], accent[2], FOCUS_RING[3]];
            cx.list.push_clip_shape(rect, &shape);
            cx.list.push_stroke_shape(rect, &shape, cx.pt(2.0), wash);
            cx.list.pop_clip();
        }

        // Centre the label in the segment, clipped to its interior —
        // a long label can't spill into the neighbouring segment.
        let font_px = cx.pt(14.0);
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let text_w = painter
            .and_then(|p| p.measure_text(&self.label, font_px))
            .unwrap_or_else(|| 8.0 * self.label.chars().count() as f32 * cx.scale);
        let text_x = b.min_x() + (b.width() - text_w).max(0.0) / 2.0;
        let ink = if self.selected {
            cx.color(TokenKey::TextInverseColor, INK_SELECTED)
        } else {
            cx.color(TokenKey::TextColor, INK)
        };
        crate::text_paint::paint_label_clipped(
            painter,
            cx.list,
            kurbo::Rect::new(
                f64::from(b.min_x() + cx.pt(4.0)),
                f64::from(b.min_y()),
                f64::from(b.max_x() - cx.pt(4.0)),
                f64::from(b.max_y()),
            ),
            kurbo::Point::new(
                f64::from(text_x.max(b.min_x() + cx.pt(4.0))),
                f64::from(b.min_y() + (b.height() - font_px) / 2.0),
            ),
            &self.label,
            font_px,
            ink,
        );
    }
}

/// A single-select segmented pill strip.
///
/// Options are laid out as equal segments inside one stadium-outline
/// track. Exactly one segment is selected at all times —
/// [`set_selected`](Self::set_selected) enforces the invariant; an
/// empty strip tolerates no-op selection until options exist (the
/// `RadioGroup`/`Dropdown` convention).
///
/// # Examples
///
/// ```
/// use martensite::widgets::Segmented;
///
/// let mut seg = Segmented::new().options(vec!["S", "M", "L"]);
/// seg.set_selected(2);
/// assert_eq!(seg.selected_index(), 2);
/// ```
pub struct Segmented {
    /// Optional accessible label for the group.
    pub label: Option<String>,
    /// Whether the group accepts input.
    pub enabled: bool,
    /// Strip direction — horizontal by default; vertical lays the
    /// segments top to bottom.
    pub direction: FlexDirection,
    /// The segments.
    segments: Vec<Segment>,
    /// Index of the selected segment.
    selected: usize,
    /// Index of the segment holding the roving tabindex.
    focused: usize,
    /// Cached bounds from the last layout pass.
    cached_bounds: Rect,
    /// Segment bounds from the last layout pass.
    segment_bounds: Vec<Rect>,
    /// The last selection change not yet drained by
    /// [`take_selected`](Self::take_selected) — the signal-out seam
    /// `Dialog::take_response`/`Banner::take_dismissed` use.
    changed: Option<usize>,
    /// Shared shaped-text painter — propagated to segments in
    /// `sync_options`. See [`crate::text_paint`].
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl Segmented {
    /// Creates an empty strip; add options with
    /// [`option`](Self::option) or [`options`](Self::options).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Segmented;
    ///
    /// let seg = Segmented::new();
    /// assert_eq!(seg.option_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: None,
            enabled: true,
            direction: FlexDirection::Row,
            segments: Vec::new(),
            selected: 0,
            focused: 0,
            cached_bounds: Rect::default(),
            segment_bounds: Vec::new(),
            changed: None,
            text_painter: None,
        }
    }

    /// Appends one option to the strip.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Segmented;
    ///
    /// let seg = Segmented::new().option("Left").option("Right");
    /// assert_eq!(seg.option_count(), 2);
    /// ```
    #[must_use]
    pub fn option(mut self, label: impl Into<String>) -> Self {
        self.segments.push(Segment::new(label));
        self.sync_options();
        self
    }

    /// Appends a batch of options.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Segmented;
    ///
    /// let seg = Segmented::new().options(vec!["Day", "Week", "Month"]);
    /// assert_eq!(seg.option_count(), 3);
    /// ```
    #[must_use]
    pub fn options(mut self, labels: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.segments.extend(labels.into_iter().map(Segment::new));
        self.sync_options();
        self
    }

    /// Sets the group's accessible label.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Segmented;
    ///
    /// let seg = Segmented::new().label("View");
    /// assert_eq!(seg.label.as_deref(), Some("View"));
    /// ```
    #[inline]
    #[must_use]
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Sets the strip direction.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{FlexDirection, Segmented};
    ///
    /// let seg = Segmented::new().direction(FlexDirection::Column);
    /// assert_eq!(seg.direction, FlexDirection::Column);
    /// ```
    #[inline]
    #[must_use]
    pub fn direction(mut self, direction: FlexDirection) -> Self {
        self.direction = direction;
        self.sync_options();
        self
    }

    /// Sets whether the group is enabled.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Segmented;
    ///
    /// let seg = Segmented::new().enabled(false);
    /// assert!(!seg.enabled);
    /// ```
    #[inline]
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self.sync_options();
        self
    }

    /// Sets the selected index (builder form of
    /// [`set_selected`](Self::set_selected)).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Segmented;
    ///
    /// let seg = Segmented::new().options(vec!["A", "B"]).selected(1);
    /// assert_eq!(seg.selected_index(), 1);
    /// ```
    #[inline]
    #[must_use]
    pub fn selected(mut self, index: usize) -> Self {
        self.set_selected(index);
        self
    }

    /// Number of options in the strip.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Segmented;
    ///
    /// assert_eq!(Segmented::new().options(vec!["A", "B"]).option_count(), 2);
    /// ```
    #[inline]
    pub fn option_count(&self) -> usize {
        self.segments.len()
    }

    /// The index of the selected segment.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Segmented;
    ///
    /// let seg = Segmented::new().options(vec!["A", "B"]);
    /// assert_eq!(seg.selected_index(), 0);
    /// ```
    #[inline]
    pub fn selected_index(&self) -> usize {
        self.selected
    }

    /// The index of the segment carrying the roving tabindex.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Segmented;
    ///
    /// let seg = Segmented::new().options(vec!["A", "B"]);
    /// assert_eq!(seg.focused_index(), 0);
    /// ```
    #[inline]
    pub fn focused_index(&self) -> usize {
        self.focused
    }

    /// Selects `index` — moves the checked state and, per APG
    /// arrow-key behaviour, the focus indicator with it. Out-of-range
    /// indices are ignored. An effective change is parked for
    /// [`take_selected`](Self::take_selected).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Segmented;
    ///
    /// let mut seg = Segmented::new().options(vec!["A", "B", "C"]);
    /// seg.set_selected(1);
    /// assert_eq!(seg.selected_index(), 1);
    /// seg.set_selected(9);
    /// assert_eq!(seg.selected_index(), 1); // out of range ignored
    /// ```
    pub fn set_selected(&mut self, index: usize) {
        if index < self.segments.len() {
            if self.selected != index {
                self.changed = Some(index);
            }
            self.selected = index;
            self.focused = index;
            self.sync_options();
        }
    }

    /// Moves the roving tabindex by `delta` positions with wraparound
    /// and selects the newly focused segment (APG automatic-activation
    /// semantics — the same contract [`RadioGroup::select`] applies).
    fn move_focus(&mut self, delta: i64) {
        let n = self.segments.len() as i64;
        if n == 0 {
            return;
        }
        let next = ((self.focused as i64 + delta).rem_euclid(n)) as usize;
        self.set_selected(next);
    }

    /// Returns the newly selected index once after each effective
    /// selection change — the widget's signal-out seam. Apps poll it
    /// per frame (or after dispatching input) to react to selection
    /// changes without a callback.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Segmented;
    ///
    /// let mut seg = Segmented::new().options(vec!["A", "B"]);
    /// seg.set_selected(1);
    /// assert_eq!(seg.take_selected(), Some(1));
    /// assert_eq!(seg.take_selected(), None); // drained
    /// ```
    #[inline]
    pub fn take_selected(&mut self) -> Option<usize> {
        self.changed.take()
    }

    /// Applies pending activations and focus moves recorded by segment
    /// children.
    ///
    /// AT actions dispatched to internal segment widgets (through
    /// `WidgetArena::internal_widget_mut`) can only mark the segment —
    /// this pulls those marks into group state. Called automatically
    /// from `event`, `layout`, and `a11y_prepare`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Segmented;
    /// use martensite_core::widget::Widget;
    ///
    /// let mut seg = Segmented::new().options(vec!["A", "B"]);
    /// // Simulate an AT Click on segment 1 through the child protocol.
    /// {
    ///     let segment = seg.child_mut(1).unwrap();
    ///     let event = martensite_core::WidgetEvent::SemanticAction(
    ///         martensite_core::SemanticAction::Click,
    ///     );
    ///     let mut cx = martensite_core::EventContext {
    ///         event: &event,
    ///         bounds: martensite_core::Rect::default(),
    ///         scale: 1.0,
    ///     };
    ///     segment.event(&mut cx);
    /// }
    /// seg.poll_pending();
    /// assert_eq!(seg.selected_index(), 1);
    /// ```
    pub fn poll_pending(&mut self) {
        let mut activate = None;
        let mut focus = None;
        for (i, segment) in self.segments.iter_mut().enumerate() {
            if segment.activation_pending {
                segment.activation_pending = false;
                activate = Some(i);
            }
            if segment.focus_pending {
                segment.focus_pending = false;
                focus = Some(i);
            }
        }
        if let Some(index) = activate {
            self.set_selected(index);
        } else if let Some(index) = focus {
            // Focus without selection (activation also moves focus).
            self.focused = index;
            self.sync_options();
        }
    }

    /// Shares a [`crate::text_paint::TextPainter`] so segment labels
    /// emit real glyph runs instead of `DrawText` placeholder boxes.
    #[must_use]
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self.sync_options();
        self
    }

    /// The strip's outer silhouette — a stadium (semicircular ends).
    fn strip_shape(&self, bounds: kurbo::Rect) -> Shape {
        Shape::corners(
            CornerRadii::uniform((bounds.height().min(bounds.width()) / 2.0) as f32),
            CornerStyle::Round,
        )
    }

    /// Mirrors group state onto the segment children so their emitted
    /// `RadioButton` nodes and selected fills are accurate.
    fn sync_options(&mut self) {
        let n = self.segments.len();
        // Keep the indices inside the set when options were appended
        // or removed around the current selection.
        if n > 0 {
            self.selected = self.selected.min(n - 1);
            self.focused = self.focused.min(n - 1);
        } else {
            self.selected = 0;
            self.focused = 0;
        }
        for (i, segment) in self.segments.iter_mut().enumerate() {
            segment.selected = i == self.selected;
            segment.focused = i == self.focused;
            // AccessKit `position_in_set` is zero-based (unlike ARIA's
            // one-based `aria-posinset`).
            segment.pos_in_set = i;
            segment.set_size = n;
            segment.end = match (i, n) {
                (0, 1) => SegmentEnd::Only,
                (0, _) => SegmentEnd::First,
                (i, n) if i == n - 1 => SegmentEnd::Last,
                _ => SegmentEnd::Middle,
            };
            segment.direction = self.direction;
            segment.enabled = self.enabled;
            segment.text_painter = self.text_painter.clone();
        }
    }
}

impl Default for Segmented {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Segmented {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let mut total = Vec2::ZERO;
        for segment in &mut self.segments {
            let size = segment.measure(cx, constraints);
            if self.direction.is_row() {
                total.x += size.x;
                total.y = total.y.max(size.y);
            } else {
                total.y += size.y;
                total.x = total.x.max(size.x);
            }
        }
        Vec2::new(
            total.x.min(constraints.max_size.x.max(0.0)),
            total.y.min(constraints.max_size.y.max(0.0)),
        )
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.poll_pending();
        self.cached_bounds = bounds;
        // Declare keyboard focusability on the arena node — the strip
        // is one tab stop.
        if self.enabled {
            cx.hot.flags |= NodeFlags::FOCUSABLE;
        } else {
            cx.hot.flags.remove(NodeFlags::FOCUSABLE);
        }
        self.segment_bounds.clear();
        let n = self.segments.len();
        if n == 0 {
            return;
        }
        for (i, segment) in self.segments.iter_mut().enumerate() {
            // Equal segments — the NSSegmentedControl/M3 contract; the
            // last segment absorbs the rounding remainder.
            let rect = if self.direction.is_row() {
                let x0 = bounds.min_x() + bounds.width() * i as f32 / n as f32;
                let x1 = bounds.min_x() + bounds.width() * (i + 1) as f32 / n as f32;
                Rect::new(x0, bounds.min_y(), (x1 - x0).max(0.0), bounds.height())
            } else {
                let y0 = bounds.min_y() + bounds.height() * i as f32 / n as f32;
                let y1 = bounds.min_y() + bounds.height() * (i + 1) as f32 / n as f32;
                Rect::new(bounds.min_x(), y0, bounds.width(), (y1 - y0).max(0.0))
            };
            self.segment_bounds.push(rect);
            cx.layout_child(segment, rect);
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::RadioGroup);
        if let Some(ref label) = self.label {
            node.set_label(label.as_str());
        }
        node.set_orientation(if self.direction.is_row() {
            accesskit::Orientation::Horizontal
        } else {
            accesskit::Orientation::Vertical
        });
        if !self.enabled {
            node.set_disabled();
        }
    }

    fn a11y_prepare(&mut self) {
        self.poll_pending();
    }

    fn hit_shape(&self) -> Option<Shape> {
        // The strip paints a stadium; accept input exactly where it is
        // visible so corner misses don't hit.
        Some(Shape::PILL)
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.enabled {
            return EventResponse::Ignored;
        }
        self.poll_pending();
        match cx.event {
            WidgetEvent::PointerPressed {
                position,
                button: PointerButton::Primary,
                ..
            } => {
                // Forward to the segment under the press via the child
                // protocol; the segment parks an activation, applied
                // below so selection stays centralized.
                let mut response = EventResponse::Ignored;
                for i in (0..self.segments.len()).rev() {
                    let Some(b) = self.child_bounds(i) else {
                        continue;
                    };
                    if !b.contains(*position) {
                        continue;
                    }
                    let mut child_cx = EventContext {
                        event: cx.event,
                        bounds: b,
                        scale: cx.scale,
                    };
                    if let Some(segment) = self.segments.get_mut(i) {
                        response = segment.event(&mut child_cx);
                    }
                    break;
                }
                self.poll_pending();
                response
            }
            WidgetEvent::KeyPressed { key, .. } => {
                // APG: all four arrows cycle the strip in either layout.
                let forward = key == "ArrowDown" || key == "ArrowRight";
                let backward = key == "ArrowUp" || key == "ArrowLeft";
                if forward {
                    self.move_focus(1);
                    EventResponse::RequestRepaint
                } else if backward {
                    self.move_focus(-1);
                    EventResponse::RequestRepaint
                } else if key == "Home" {
                    self.set_selected(0);
                    EventResponse::RequestRepaint
                } else if key == "End" {
                    let last = self.segments.len().saturating_sub(1);
                    self.set_selected(last);
                    EventResponse::RequestRepaint
                } else if key == " " || key == "Space" || key == "Enter" {
                    self.set_selected(self.focused);
                    EventResponse::RequestRepaint
                } else {
                    EventResponse::Ignored
                }
            }
            // A Click on the group selects the roving segment.
            WidgetEvent::SemanticAction(SemanticAction::Click) => {
                self.set_selected(self.focused);
                EventResponse::RequestRepaint
            }
            WidgetEvent::SemanticAction(SemanticAction::Focus) => EventResponse::CaptureFocus,
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        let rect = kurbo::Rect::new(
            f64::from(b.min_x()),
            f64::from(b.min_y()),
            f64::from(b.max_x()),
            f64::from(b.max_y()),
        );
        let shape = self.strip_shape(rect);
        let face = if self.enabled {
            cx.color(TokenKey::SurfaceColor, TRACK)
        } else {
            cx.color(TokenKey::BackgroundColor, [245, 245, 246, 255])
        };
        cx.list.push_fill_shape(rect, &shape, face);
        cx.list.push_stroke_shape(
            rect,
            &shape,
            cx.pt(1.0),
            cx.color(TokenKey::BorderColor, EDGE),
        );

        // Hairline dividers between segments, inset so they read as
        // separators rather than part of the outline — skipped where
        // they would butt against the selected fill (which already
        // separates itself visually).
        let n = self.segment_bounds.len();
        if n > 1 {
            let divider = cx.color(TokenKey::DividerColor, EDGE);
            let inset = cx.pt(6.0);
            for (i, seg_bounds) in self.segment_bounds.iter().enumerate().skip(1) {
                // Skip the divider where the selected segment's own
                // fill already separates the neighbours.
                if i == self.selected || i == self.selected + 1 {
                    continue;
                }
                let line = if self.direction.is_row() {
                    let x = f64::from(seg_bounds.min_x());
                    kurbo::Rect::new(
                        x,
                        f64::from(b.min_y()) + f64::from(inset),
                        x + cx.ptf(1.0),
                        f64::from(b.max_y()) - f64::from(inset),
                    )
                } else {
                    let y = f64::from(seg_bounds.min_y());
                    kurbo::Rect::new(
                        f64::from(b.min_x()) + f64::from(inset),
                        y,
                        f64::from(b.max_x()) - f64::from(inset),
                        y + cx.ptf(1.0),
                    )
                };
                cx.list.push_fill_rect(line, divider);
            }
        }
    }

    fn child_count(&self) -> usize {
        self.segments.len()
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        self.segments.get(index).map(|s| s as &dyn Widget)
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        self.segments.get_mut(index).map(|s| s as &mut dyn Widget)
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        self.segment_bounds.get(index).copied()
    }
}

impl std::fmt::Debug for Segmented {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Segmented")
            .field("options", &self.segments.len())
            .field("selected", &self.selected)
            .field("focused", &self.focused)
            .field("enabled", &self.enabled)
            .field("direction", &self.direction)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn strip(labels: &[&str]) -> Segmented {
        Segmented::new().options(labels.iter().copied())
    }

    fn laid_out(seg: &mut Segmented, w: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        seg.layout(&mut cx, Rect::new(0.0, 0.0, w, h));
    }

    fn key(k: &str) -> WidgetEvent {
        WidgetEvent::KeyPressed {
            key: k.to_string(),
            repeat: false,
        }
    }

    fn event(seg: &mut Segmented, ev: &WidgetEvent) -> EventResponse {
        let mut cx = EventContext {
            event: ev,
            bounds: seg.cached_bounds,
            scale: 1.0,
        };
        seg.event(&mut cx)
    }

    #[test]
    fn builder_collects_options() {
        let seg = Segmented::new()
            .option("A")
            .options(vec!["B", "C"])
            .selected(2)
            .enabled(true);
        assert_eq!(seg.option_count(), 3);
        assert_eq!(seg.selected_index(), 2);
    }

    #[test]
    fn single_checked_invariant() {
        let mut seg = strip(&["A", "B", "C"]);
        for i in 0..3 {
            seg.set_selected(i);
            let checked: Vec<bool> = (0..3)
                .map(|j| {
                    let mut n = AccessKitNode::new(accesskit::Role::Unknown);
                    seg.child(j).unwrap().accessibility(&mut n);
                    n.toggled() == Some(accesskit::Toggled::True)
                })
                .collect();
            assert_eq!(checked.iter().filter(|c| **c).count(), 1);
            assert!(checked[i]);
        }
    }

    #[test]
    fn arrows_move_and_select() {
        let mut seg = strip(&["A", "B", "C"]);
        laid_out(&mut seg, 300.0, 28.0);
        event(&mut seg, &key("ArrowRight"));
        assert_eq!(seg.focused_index(), 1);
        assert_eq!(seg.selected_index(), 1);
        event(&mut seg, &key("ArrowRight"));
        assert_eq!(seg.selected_index(), 2);
        // Wraparound.
        event(&mut seg, &key("ArrowRight"));
        assert_eq!(seg.selected_index(), 0);
        event(&mut seg, &key("ArrowLeft"));
        assert_eq!(seg.selected_index(), 2);
    }

    #[test]
    fn home_end_and_space() {
        let mut seg = strip(&["A", "B", "C"]);
        laid_out(&mut seg, 300.0, 28.0);
        event(&mut seg, &key("End"));
        assert_eq!(seg.selected_index(), 2);
        event(&mut seg, &key("Home"));
        assert_eq!(seg.selected_index(), 0);
        seg.focused = 1;
        seg.sync_options();
        event(&mut seg, &key("Space"));
        assert_eq!(seg.selected_index(), 1);
    }

    #[test]
    fn roving_tabindex_single_focus_action() {
        let seg = strip(&["A", "B", "C"]);
        let focus_actions: Vec<bool> = (0..3)
            .map(|i| {
                let mut n = AccessKitNode::new(accesskit::Role::Unknown);
                seg.child(i).unwrap().accessibility(&mut n);
                n.supports_action(accesskit::Action::Focus)
            })
            .collect();
        assert_eq!(focus_actions, vec![true, false, false]);
    }

    #[test]
    fn click_selects_segment() {
        let mut seg = strip(&["A", "B", "C"]);
        laid_out(&mut seg, 300.0, 28.0);
        // Third segment occupies the last third of the strip.
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(250.0, 14.0),
            button: PointerButton::Primary,
            count: 1,
        };
        assert_eq!(event(&mut seg, &press), EventResponse::CaptureFocus);
        assert_eq!(seg.selected_index(), 2);
    }

    #[test]
    fn pending_activation_from_child_click() {
        let mut seg = strip(&["A", "B", "C"]);
        laid_out(&mut seg, 300.0, 28.0);
        let ev = WidgetEvent::SemanticAction(SemanticAction::Click);
        let segment = seg.child_mut(2).unwrap();
        let mut cx = EventContext {
            event: &ev,
            bounds: Rect::default(),
            scale: 1.0,
        };
        assert_eq!(segment.event(&mut cx), EventResponse::Handled);
        seg.poll_pending();
        assert_eq!(seg.selected_index(), 2);
    }

    #[test]
    fn take_selected_drains_once() {
        let mut seg = strip(&["A", "B"]);
        assert_eq!(seg.take_selected(), None);
        seg.set_selected(1);
        assert_eq!(seg.take_selected(), Some(1));
        assert_eq!(seg.take_selected(), None);
        // Re-selecting the same index reports nothing.
        seg.set_selected(1);
        assert_eq!(seg.take_selected(), None);
    }

    #[test]
    fn group_accessibility_role_and_orientation() {
        let seg = strip(&["A"]).label("View").direction(FlexDirection::Column);
        let mut node = AccessKitNode::new(accesskit::Role::Unknown);
        seg.accessibility(&mut node);
        assert_eq!(node.role(), accesskit::Role::RadioGroup);
        assert_eq!(node.label(), Some("View"));
        assert_eq!(node.orientation(), Some(accesskit::Orientation::Vertical));
    }

    #[test]
    fn segment_a11y_pos_in_set() {
        let seg = strip(&["A", "B", "C"]);
        let mut node = AccessKitNode::new(accesskit::Role::Unknown);
        seg.child(1).unwrap().accessibility(&mut node);
        assert_eq!(node.role(), accesskit::Role::RadioButton);
        // AccessKit positions are zero-based.
        assert_eq!(node.position_in_set(), Some(1));
        assert_eq!(node.size_of_set(), Some(3));
    }

    #[test]
    fn disabled_ignores_events() {
        let mut seg = strip(&["A", "B"]).enabled(false);
        laid_out(&mut seg, 200.0, 28.0);
        assert_eq!(event(&mut seg, &key("ArrowRight")), EventResponse::Ignored);
        assert_eq!(seg.selected_index(), 0);
    }

    #[test]
    fn vertical_layout_divides_height() {
        let mut seg = strip(&["A", "B"]).direction(FlexDirection::Column);
        laid_out(&mut seg, 80.0, 60.0);
        assert_eq!(seg.child_bounds(0), Some(Rect::new(0.0, 0.0, 80.0, 30.0)));
        assert_eq!(seg.child_bounds(1), Some(Rect::new(0.0, 30.0, 80.0, 30.0)));
    }

    #[test]
    fn debug_format() {
        let seg = strip(&["A", "B"]);
        let debug = format!("{:?}", seg);
        assert!(debug.contains("Segmented"));
    }
}
