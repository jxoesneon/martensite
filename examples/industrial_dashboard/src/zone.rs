//! Zone grammar + binding contract — the shared spec every panel's
//! functional surfaces are built from (Design Council docket
//! 20260921, requirements 1-4).
//!
//! ## Layout grammar
//!
//! - Page columns are `Flex::column().gap(ZONE_STACK)`; the tab label
//!   is the page title (no in-page header widget).
//! - [`row()`] mounts banded surfaces top-aligned; [`strip()`] mounts
//!   rows of intrinsic controls center-aligned. All bands in one row
//!   share a single band height.
//! - [`ZONE_GAP`]/[`ZONE_PAD`]/[`ZONE_STACK`] are the only spacing
//!   tokens inside zones — no ad-hoc gutters.
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
use martensite::widgets::aspect_frame::AspectFrame;
use martensite::widgets::flex::{CrossAxisAlignment, Flex};
use std::time::Duration;

use crate::domain::PlantModel;

/// Spacing between elements inside a zone (logical pt).
pub const ZONE_GAP: f32 = 8.0;
/// Spacing between rows in a page column (logical pt).
pub const ZONE_STACK: f32 = 12.0;
/// Padding between a zone's chrome and its content (logical pt).
pub const ZONE_PAD: f32 = 12.0;
/// Band height: sparklines, strips, progress, pickers (logical pt).
pub const BAND_S: f32 = 120.0;
/// Band height: charts, gauges, calendars, lists (logical pt).
pub const BAND_M: f32 = 200.0;
/// Band height: tables, kanban, viewers, terminals, trees (logical pt).
pub const BAND_L: f32 = 300.0;
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

/// A content row inside a page column — `ZONE_GAP` between children,
/// top-aligned so banded surfaces sit at the top of the row (bands
/// carry their own height). Every row must carry at least one
/// `child_flex` so it fills its width (layout grammar rule 1).
pub fn row() -> Flex {
    Flex::row()
        .gap(ZONE_GAP)
        .cross_axis_alignment(CrossAxisAlignment::Start)
}

/// A control strip — a `row()` for intrinsic controls (buttons,
/// switches, fields, badges, pickers, labels), center-aligned so mixed
/// control heights sit on one optical line. No banded children.
pub fn strip() -> Flex {
    Flex::row()
        .gap(ZONE_GAP)
        .cross_axis_alignment(CrossAxisAlignment::Center)
}

/// Aspect-framed mount — keeps a genuinely aspect-locked view's
/// proportions inside a weighted band (QR, barcode, sunburst,
/// avatar/photo tiles, video). Never for time-series charts, lists,
/// or tables — those take `band` + `child_flex`.
pub fn framed(ratio: f32, w: impl Widget + 'static) -> AspectFrame {
    AspectFrame::new(ratio).xalign(0.5).child(w)
}

/// A fixed-height surface mount: measures to the row's full width at
/// `height` pt and lays its child out to the full bounds. Every
/// fill-style surface (chart, viewer, table, terminal, kanban, tree,
/// list, canvas, video, map) is mounted through a band; intrinsic
/// controls never are.
pub struct Band {
    child: Box<dyn Widget>,
    height: f32,
    bounds: Rect,
}

/// Mount `w` in a [`Band`] at `height_pt` logical points.
pub fn band(height_pt: f32, w: impl Widget + 'static) -> Band {
    Band {
        child: Box::new(w),
        height: height_pt,
        bounds: Rect::default(),
    }
}

impl Widget for Band {
    fn debug_name(&self) -> &'static str {
        "Band"
    }

    fn measure(&mut self, cx: &mut LayoutContext, c: LayoutConstraints) -> glam::Vec2 {
        let w = if c.max_size.x.is_finite() && c.max_size.x <= MAX_REPORTED_DIM {
            c.max_size.x
        } else {
            FILL_FALLBACK.max(0.0)
        };
        let size = glam::Vec2::new(w, cx.pt(self.height));
        // Composite children (`Flex`, `Tabs`, `Stack`) fill their
        // layout caches in `measure` — they must see the band's
        // allotment even though the band's own size is fixed.
        self.child.measure(
            cx,
            LayoutConstraints {
                min_size: glam::Vec2::ZERO,
                max_size: size,
            },
        );
        size
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        // Weighted rows hand the band a width `measure` never saw —
        // re-measure tight so the child's caches match these bounds.
        self.child.measure(
            cx,
            LayoutConstraints {
                min_size: glam::Vec2::ZERO,
                max_size: bounds.size,
            },
        );
        self.child.layout(cx, bounds);
    }

    fn accessibility(&self, node: &mut accesskit::Node) {
        node.set_role(accesskit::Role::Group);
    }

    fn child_count(&self) -> usize {
        1
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        (index == 0).then_some(&*self.child)
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        (index == 0).then_some(&mut *self.child)
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        (index == 0).then_some(self.bounds)
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
    /// Last layout allotment — replayed onto the widget after a push
    /// so a rebuilt/replaced widget never paints with zeroed layout
    /// state (bounds, cached child rects). The arena's layout pass is
    /// resize-gated; without this a `*w = build(m)` reflect would sit
    /// unlaid-out (or panic on a stale layout cache) until the next
    /// window resize.
    last_layout: Option<(Rect, f32)>,
    /// Scratch hot node for post-push re-layouts. `Widget::layout`
    /// needs a `LayoutContext`, which borrows a `HotNode` — outside
    /// the arena's layout pass there is no shared node to borrow, so
    /// flag unions (`FOCUSABLE`) land here instead. That matches the
    /// staleness semantics any between-layouts mutation already has:
    /// the arena's own flag union only refreshes on real layout
    /// passes. Seeded from the real node on every real `layout`.
    scratch_hot: martensite::core::HotNode,
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
            last_layout: None,
            scratch_hot: martensite::core::HotNode::default(),
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
        self.last_layout = Some((bounds, cx.scale));
        // Seed the scratch from the shared node so a post-push
        // re-layout keeps the flags this subtree already unioned in.
        self.scratch_hot.flags = cx.hot.flags;
        self.scratch_hot.bounds = bounds;
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
        // A push reflect may mutate — or wholly replace — the widget
        // after `tick` captured its flag. Conservatively report dirty
        // whenever a push ran, and re-run `layout` against the last
        // allotment so replaced widgets never reach `paint` with
        // zeroed layout state (empty cached rect vectors → index
        // panics; zero bounds → invisible output). Layout is cheap
        // arithmetic over already-computed measures; pushes are
        // signature-gated, so the common case is a no-op reflect.
        if self.has_push {
            if let Some((bounds, scale)) = self.last_layout {
                self.widget.layout(
                    &mut LayoutContext {
                        hot: &mut self.scratch_hot,
                        scale,
                    },
                    bounds,
                );
            }
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use martensite::core::HotNode;
    use martensite::reactive::Signal;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// Counts layout passes and records the last allotment — proves
    /// `Bound` re-lays-out a pushed widget before the next paint.
    struct Probe {
        layouts: Arc<AtomicUsize>,
        last_bounds: Rect,
    }

    impl Widget for Probe {
        fn measure(&mut self, _cx: &mut LayoutContext, _c: LayoutConstraints) -> glam::Vec2 {
            glam::Vec2::new(10.0, 10.0)
        }
        fn layout(&mut self, _cx: &mut LayoutContext, bounds: Rect) {
            self.last_bounds = bounds;
            self.layouts.fetch_add(1, Ordering::Relaxed);
        }
        fn paint(&self, _cx: &mut PaintContext) {}
    }

    fn model() -> PlantModel {
        PlantModel::seeded(
            Signal::new(0.4),
            Signal::new(0.6),
            Signal::new(false),
            Signal::new(true),
            Signal::new(String::new()),
        )
    }

    #[test]
    fn push_relayouts_against_last_allotment() {
        let m = model();
        let layouts = Arc::new(AtomicUsize::new(0));
        let mut b = Bound::new(
            Probe {
                layouts: Arc::clone(&layouts),
                last_bounds: Rect::default(),
            },
            &m,
        )
        .push(|_w: &mut Probe, _m| {});
        let mut hot = HotNode::default();
        let bounds = Rect::new(4.0, 8.0, 100.0, 50.0);
        b.layout(
            &mut LayoutContext {
                hot: &mut hot,
                scale: 2.0,
            },
            bounds,
        );
        assert_eq!(layouts.load(Ordering::Relaxed), 1);
        // A push-bearing tick must re-run layout — a rebuilt widget
        // would otherwise paint with zeroed layout state.
        b.tick(Duration::from_millis(16));
        assert_eq!(layouts.load(Ordering::Relaxed), 2);
        assert_eq!(b.inner().last_bounds, bounds);
    }

    #[test]
    fn pull_only_mount_does_not_relayout() {
        let m = model();
        let layouts = Arc::new(AtomicUsize::new(0));
        let mut b = Bound::new(
            Probe {
                layouts: Arc::clone(&layouts),
                last_bounds: Rect::default(),
            },
            &m,
        )
        .pull(|_w: &mut Probe, _m| {});
        let mut hot = HotNode::default();
        b.layout(
            &mut LayoutContext {
                hot: &mut hot,
                scale: 1.0,
            },
            Rect::new(0.0, 0.0, 10.0, 10.0),
        );
        b.tick(Duration::from_millis(16));
        assert_eq!(
            layouts.load(Ordering::Relaxed),
            1,
            "a pull-only mount has no push-side mutation to re-layout"
        );
    }

    #[test]
    fn band_measures_height_and_lays_child_to_bounds() {
        let layouts = Arc::new(AtomicUsize::new(0));
        let mut b = band(
            BAND_M,
            Probe {
                layouts: Arc::clone(&layouts),
                last_bounds: Rect::default(),
            },
        );
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 2.0,
        };
        let size = b.measure(
            &mut cx,
            LayoutConstraints {
                min_size: glam::Vec2::ZERO,
                max_size: glam::Vec2::new(640.0, f32::MAX),
            },
        );
        assert_eq!(size, glam::Vec2::new(640.0, BAND_M * 2.0));
        let bounds = Rect::new(4.0, 8.0, 320.0, 160.0);
        b.layout(&mut cx, bounds);
        assert_eq!(layouts.load(Ordering::Relaxed), 1);
        assert_eq!(b.child_bounds(0), Some(bounds));
    }

    #[test]
    fn unlaid_out_push_mount_skips_relayout() {
        // A Bound that was never laid out (hidden tab) must not
        // conjure a layout from nothing.
        let m = model();
        let layouts = Arc::new(AtomicUsize::new(0));
        let mut b = Bound::new(
            Probe {
                layouts: Arc::clone(&layouts),
                last_bounds: Rect::default(),
            },
            &m,
        )
        .push(|_w: &mut Probe, _m| {});
        b.tick(Duration::from_millis(16));
        assert_eq!(layouts.load(Ordering::Relaxed), 0);
    }
}
