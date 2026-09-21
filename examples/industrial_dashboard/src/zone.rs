//! Zone grammar + binding contract — the shared spec every panel's
//! functional surfaces are built from (Design Council docket
//! 20260921, requirements 1-4).
//!
//! ## Layout grammar
//!
//! - [`zone_header`] renders the uniform zone title: 12 pt muted
//!   uppercase text over a hairline, matching `panel_chrome`'s
//!   language. Zone names are domain names ("MAINTENANCE SCHEDULE"),
//!   never widget names ("Gantt").
//! - [`ZONE_GAP`]/[`ZONE_PAD`] are the only spacing tokens inside
//!   zones — no ad-hoc gutters.
//! - One visible surface per zone: alternate views live behind
//!   selector chrome (`Tabs`, `Segmented`, `Dropdown` for >8
//!   destinations) — never a scroll wall of every option.
//! - Selector strips cap at 8 visible entries; beyond that the
//!   selector becomes a `Dropdown` (or the zone splits into two).
//!
//! ## Binding contract (bind-or-cut)
//!
//! [`Bound`] wraps a widget with two closures:
//!
//! - `pull` — drain the widget's interaction state (`take_*`/getters)
//!   into model signals. Runs before `tick`.
//! - `push` — reflect model signals back into the widget. Runs after.
//!
//! A `Bound` IS a `Widget` — it drops into `Tabs`/`Flex`/`FlowBox`
//! like any other child, and its `tick` runs the binding cycle.
//! Widgets with nothing to bind do not get mounted (that's the "cut"
//! half of bind-or-cut).

use martensite::core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect, Widget,
};
use martensite::widgets::text::Text;
use std::time::Duration;

use crate::domain::PlantModel;
use crate::model::Palette;

/// Spacing between elements inside a zone (logical pt).
pub const ZONE_GAP: f32 = 8.0;
/// Padding between a zone's chrome and its content (logical pt).
pub const ZONE_PAD: f32 = 12.0;
/// Zone header text size — the published type-ramp step for section
/// titles (12 pt muted uppercase; panel titles are 12 pt).
pub const ZONE_HEADER_PT: f32 = 12.0;
/// Selector strips cap at this many visible entries before overflow
/// routes to a `Dropdown`.
pub const MAX_SELECTOR_ENTRIES: usize = 8;

/// Fill-widget guard: a widget that reports a dimension beyond this
/// (the "echo the offered `f32::MAX`" pattern — drawers, watermarks)
/// is reined to [`FILL_FALLBACK`] instead of poisoning the parent's
/// flow. One guard at the mount point protects every zone surface.
const MAX_REPORTED_DIM: f32 = 4096.0;
/// Height assigned when a fill widget reports an astronomical size —
/// a sane content surface, not a tower.
const FILL_FALLBACK: f32 = 480.0;

/// The uniform zone header: muted uppercase title over a hairline.
/// Paints its own chrome so it drops into a `Flex` column directly.
pub struct ZoneHeader {
    text: Text,
    label: &'static str,
    bounds: Rect,
    scale: f32,
}

impl ZoneHeader {
    pub fn new(label: &'static str) -> Self {
        Self {
            text: Text::new(label).font_size(ZONE_HEADER_PT),
            label,
            bounds: Rect::default(),
            scale: 1.0,
        }
    }
}

impl Widget for ZoneHeader {
    fn debug_name(&self) -> &'static str {
        "Zone Header"
    }

    fn measure(&mut self, cx: &mut LayoutContext, c: LayoutConstraints) -> glam::Vec2 {
        let s = self.text.measure(cx, c);
        glam::Vec2::new(c.max_size.x, s.y + 8.0)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        let s = self.text.measure(
            cx,
            LayoutConstraints {
                min_size: glam::Vec2::ZERO,
                max_size: bounds.size,
            },
        );
        self.text.layout(
            cx,
            Rect::new(bounds.min_x(), bounds.min_y(), bounds.width(), s.y),
        );
    }

    fn paint(&self, cx: &mut PaintContext) {
        self.text.paint(cx);
        // Hairline under the title — the zone separator.
        let pal = Palette::from_theme(cx.theme);
        let y = f64::from(self.bounds.max_y() - 1.0);
        cx.list.push_fill_rect(
            crate::panels::krect(
                f64::from(self.bounds.min_x()),
                y,
                f64::from(self.bounds.width()),
                1.0,
            ),
            pal.hairline(),
        );
    }

    fn accessibility(&self, node: &mut accesskit::Node) {
        node.set_role(accesskit::Role::Heading);
        node.set_label(self.label);
    }
}

/// A model-bound widget: `pull` drains interaction into signals,
/// `push` reflects signals back into the view. Both run inside
/// `tick` — the only lifecycle the composition guarantees.
///
/// `Bound<W>` is generic so the closures get the concrete `&mut W` —
/// call `set_value`/`take_*`/getters directly, no downcasting:
///
/// ```ignore
/// Bound::new(Gauge::new(), &model)
///     .push(|g: &mut Gauge, m| g.set_value(m.cpu.get()))
/// ```
/// A binding adapter — the pull/push closure signature shared by
/// [`Bound`]'s two halves.
type BindFn<W> = Box<dyn FnMut(&mut W, &PlantModel) + Send + Sync>;

pub struct Bound<W: Widget> {
    widget: W,
    model: PlantModel,
    pull: BindFn<W>,
    push: BindFn<W>,
    /// Whether `.push()` installed a reflect closure — push mutations
    /// dirty the node, so `tick` reports `true` while one is present.
    has_push: bool,
}

impl<W: Widget> Bound<W> {
    /// Wrap `widget` bound to `model`. Callers must add a `.pull()`
    /// and/or `.push()` before mounting — a `Bound` with no adapters
    /// is a dead mount (widget costume without a model seam) and must
    /// not ship; either wire it or mount the bare widget instead.
    pub fn new(widget: W, model: &PlantModel) -> Self {
        Self {
            widget,
            model: model.clone(),
            pull: Box::new(|_, _| {}),
            push: Box::new(|_, _| {}),
            has_push: false,
        }
    }

    /// The widget → model drain (runs before `tick`).
    pub fn pull(mut self, f: impl FnMut(&mut W, &PlantModel) + Send + Sync + 'static) -> Self {
        self.pull = Box::new(f);
        self
    }

    /// The model → widget reflect (runs after `tick`).
    pub fn push(mut self, f: impl FnMut(&mut W, &PlantModel) + Send + Sync + 'static) -> Self {
        self.push = Box::new(f);
        self.has_push = true;
        self
    }

    /// Access the inner widget — test code asserts binding effects.
    /// Test-only: production code never needs to reach past the
    /// binding.
    #[cfg(test)]
    pub fn inner(&self) -> &W {
        &self.widget
    }
    /// Mutable twin of [`inner`](Self::inner).
    #[cfg(test)]
    pub fn inner_mut(&mut self) -> &mut W {
        &mut self.widget
    }
}

impl<W: Widget> Widget for Bound<W> {
    fn debug_name(&self) -> &'static str {
        self.widget.debug_name()
    }

    fn measure(&mut self, cx: &mut LayoutContext, c: LayoutConstraints) -> glam::Vec2 {
        let s = self.widget.measure(cx, c);
        let tame = |v: f32| {
            if !v.is_finite() || v > MAX_REPORTED_DIM {
                FILL_FALLBACK
            } else {
                v
            }
        };
        glam::Vec2::new(tame(s.x), tame(s.y))
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.widget.layout(cx, bounds);
    }

    fn paint(&self, cx: &mut PaintContext) {
        self.widget.paint(cx);
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        self.widget.event(cx)
    }

    fn tick(&mut self, dt: Duration) -> bool {
        (self.pull)(&mut self.widget, &self.model);
        let dirty = self.widget.tick(dt);
        (self.push)(&mut self.widget, &self.model);
        // A push reflect may mutate the widget after `tick` captured
        // its flag — conservatively report dirty whenever a push ran,
        // else pushed state changes would skip paint/a11y re-emission.
        dirty || self.has_push
    }

    fn sync_overlay(&mut self, overlay: &mut martensite::core::OverlayLayer) {
        self.widget.sync_overlay(overlay);
    }

    fn min_render(&self) -> martensite::core::RenderMinimum {
        self.widget.min_render()
    }

    fn paint_underflow(&self, cx: &mut PaintContext) {
        self.widget.paint_underflow(cx);
    }

    fn accessibility(&self, node: &mut accesskit::Node) {
        self.widget.accessibility(node);
    }

    fn child_count(&self) -> usize {
        self.widget.child_count()
    }
    fn child(&self, i: usize) -> Option<&dyn Widget> {
        self.widget.child(i)
    }
    fn child_mut(&mut self, i: usize) -> Option<&mut dyn Widget> {
        self.widget.child_mut(i)
    }
    fn child_bounds(&self, i: usize) -> Option<Rect> {
        self.widget.child_bounds(i)
    }
    fn clips_children(&self) -> bool {
        self.widget.clips_children()
    }

    // The wrapper must be transparent: every hook the wrapped widget
    // overrides has to reach it, or `Bound` silently degrades shaped
    // hit-testing, child clipping, and the a11y/timemachine trees.

    fn hit_shape(&self) -> Option<martensite::core::shape::Shape> {
        self.widget.hit_shape()
    }

    fn clip_shape(&self) -> Option<martensite::core::shape::Shape> {
        self.widget.clip_shape()
    }

    fn a11y_prepare(&mut self) {
        self.widget.a11y_prepare();
    }

    fn a11y_fixup(
        &self,
        emitted: &mut Vec<martensite::core::A11yEmittedNode>,
        overlay_nodes: &[martensite::core::OverlayA11yRef],
        this_node: &mut accesskit::Node,
    ) {
        self.widget.a11y_fixup(emitted, overlay_nodes, this_node);
    }

    #[cfg(feature = "devtools-timemachine")]
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        self.widget.as_any_mut()
    }

    #[cfg(feature = "devtools-timemachine")]
    fn timemachine_snapshot(&self) -> Option<Box<dyn martensite::core::TimemachineState>> {
        self.widget.timemachine_snapshot()
    }

    #[cfg(feature = "devtools-timemachine")]
    fn timemachine_restore(&mut self, state: &dyn martensite::core::TimemachineState) -> bool {
        self.widget.timemachine_restore(state)
    }
}
