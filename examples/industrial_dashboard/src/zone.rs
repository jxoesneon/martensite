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
use martensite::reactive::Signal;
use martensite::widgets::aspect_frame::AspectFrame;
use martensite::widgets::disclosure::Disclosure;
use martensite::widgets::flex::{CrossAxisAlignment, Flex};
use std::time::Duration;

use crate::domain::PlantModel;

/// Spacing between elements inside a zone (logical pt).
pub const ZONE_GAP: f32 = 10.0;
/// Spacing between rows in a page column (logical pt).
pub const ZONE_STACK: f32 = 14.0;
/// Padding between a zone's chrome and its content (logical pt).
pub const ZONE_PAD: f32 = 14.0;
/// Band height: sparklines, strips, progress, pickers (logical pt).
pub const BAND_S: f32 = 120.0;
/// Band height: charts, gauges, calendars, lists (logical pt).
pub const BAND_M: f32 = 200.0;
/// Band height: tables, kanban, viewers, terminals, trees (logical pt).
pub const BAND_L: f32 = 300.0;
/// Selector strips cap at this many visible entries before overflow
/// routes to a `Dropdown`.
pub const MAX_SELECTOR_ENTRIES: usize = 8;
/// Below this content width (pt) a page's rail stacks under/above the
/// primary instead of sitting beside it (grammar Δ10).
pub const RAIL_STACK_W: f32 = 560.0;
/// Below this content width (pt) the rail collapses into a
/// user-toggleable `Disclosure` — never under/over the artifact for
/// left masters: masters stay above, detail rails below.
pub const RAIL_DISCLOSE_W: f32 = 380.0;
/// Detail-rail width as a fraction of the wide page body.
pub const RAIL_FRAC: f32 = 0.34;
/// Chooser (left-master) width as a fraction — narrower than a rail.
pub const MASTER_FRAC: f32 = 0.30;
/// Maximum width of a `Centered` variant's task surface (pt).
pub const CENTERED_W: f32 = 560.0;

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

/// A scroll mount: content taller than its allotment scrolls inside
/// the bounded region (smart bars — no chrome when it fits). Use for
/// detail columns, dossiers, and launcher stacks whose intrinsic
/// height legitimately exceeds the pane. Never mount a `fill` surface
/// inside it — weighted children see an unbounded main axis and fall
/// back to intrinsic size.
pub fn scroll(w: impl Widget + 'static) -> martensite::widgets::ScrollView {
    martensite::widgets::ScrollView::new(w)
}

/// A stretch mount: takes whatever the parent offers on both axes.
/// The page-grammar's workhorse — primary surfaces mount through
/// `fill` so a bounded `Tabs` panel distributes real space by
/// `child_flex` weight instead of fixed band heights. Inside an
/// unbounded scroller it reports `FILL_FALLBACK` (never `f32::MAX`,
/// so it can sit in a bounded or unbounded context safely — but per
/// the grammar it must not be mounted inside a `ScrollView`; the
/// surface scrolls internally when it needs to).
pub struct Fill {
    child: Box<dyn Widget>,
    bounds: Rect,
}

/// Mount `w` stretched to the offered bounds.
pub fn fill(w: impl Widget + 'static) -> Fill {
    Fill {
        child: Box::new(w),
        bounds: Rect::default(),
    }
}

impl Widget for Fill {
    fn debug_name(&self) -> &'static str {
        "Fill"
    }

    fn measure(&mut self, cx: &mut LayoutContext, c: LayoutConstraints) -> glam::Vec2 {
        let tame = |v: f32| {
            if v.is_finite() && v <= MAX_REPORTED_DIM {
                v.max(0.0)
            } else {
                FILL_FALLBACK
            }
        };
        let size = glam::Vec2::new(tame(c.max_size.x), tame(c.max_size.y));
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

/// Page layout variant — the schema's declared shape, driving how the
/// rail/master sits relative to the primary surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Variant {
    /// One dominant surface, no rail (charts theater, terminal).
    Theater,
    /// Uniform cell grid, no rail (gauges, cameras wall).
    Wall,
    /// Narrow centered task surface (console lock, sign-off).
    Centered,
    /// `primary | rail` — the rail is detail-of-selection, collapses
    /// below the primary.
    MasterDetail,
    /// `master | artifact` — the master (chooser) sits left and
    /// collapses *above* the artifact it selects.
    MasterLeft,
}

/// A zone page — the ratified presentation grammar made executable.
/// Every page declares:
///
/// ```text
/// question — the operator question it answers (doc comment)
/// variant  — Variant
/// strip    — optional context strip (filters/selectors/primary verb)
/// primary  — the dominant surface
/// rail     — optional detail-of-selection rail (Disclosure-wrapped)
/// ```
///
/// Collapse (reads `zone_width`, published by `ZonePanel::layout`):
///
/// - `≥ RAIL_STACK_W` — side by side (rail right / master left).
/// - `< RAIL_STACK_W` — stacked: detail rail *below*, master *above*.
/// - `< RAIL_DISCLOSE_W` — the rail/master becomes a user-toggleable
///   `Disclosure`; masters keep document order before the artifact.
pub struct Page {
    variant: Variant,
    strip: Option<Box<dyn Widget>>,
    primary: Box<dyn Widget>,
    /// Rail/master content — always Disclosure-wrapped so the <380pt
    /// collapse is a state flip, not a re-mount. At ≥ RAIL_DISCLOSE_W
    /// layout forces it open (a rail can't hide while there's room).
    rail: Option<Disclosure>,
    zone_width: Signal<f32>,
    bounds: Rect,
    strip_b: Rect,
    prim_b: Rect,
    rail_b: Rect,
}

impl Page {
    /// `primary` is the dominant surface — mount it through [`fill`]
    /// (or a widget that fills bounds) so it stretches.
    pub fn new(variant: Variant, primary: impl Widget + 'static, zone_width: &Signal<f32>) -> Self {
        Self {
            variant,
            strip: None,
            primary: Box::new(primary),
            rail: None,
            zone_width: zone_width.clone(),
            bounds: Rect::default(),
            strip_b: Rect::default(),
            prim_b: Rect::default(),
            rail_b: Rect::default(),
        }
    }

    /// Context strip — filters/selectors/the primary verb only. The
    /// strip is scroll-mounted so an over-wide control set scrolls
    /// horizontally instead of crushing trailing controls to 0pt.
    pub fn strip(mut self, s: impl Widget + 'static) -> Self {
        self.strip = Some(Box::new(martensite::widgets::ScrollView::horizontal(s)));
        self
    }

    /// Detail rail (`MasterDetail`) or chooser (`MasterLeft`) —
    /// `title` becomes the Disclosure's landmark label. The content
    /// is scroll-mounted: detail taller than the rail scrolls inside
    /// the disclosure rather than crushing trailing children.
    pub fn rail(mut self, title: &str, r: impl Widget + 'static) -> Self {
        self.rail = Some(Disclosure::new(title.to_string()).child(scroll(r)));
        self
    }

    fn child_count_inner(&self) -> usize {
        self.strip.is_some() as usize + 1 + self.rail.is_some() as usize
    }

    /// Document order: strip → (master?) → primary → (rail?). For
    /// `MasterLeft` the rail slot *is* the master, ordered before the
    /// artifact — selection context always precedes detail.
    fn order(&self) -> Vec<usize> {
        let mut v = Vec::new();
        if self.strip.is_some() {
            v.push(0);
        }
        match self.variant {
            Variant::MasterLeft => {
                if self.rail.is_some() {
                    v.push(2);
                }
                v.push(1);
            }
            _ => {
                v.push(1);
                if self.rail.is_some() {
                    v.push(2);
                }
            }
        }
        v
    }

    fn slot(&self, i: usize) -> Option<&dyn Widget> {
        match i {
            0 => self.strip.as_deref(),
            1 => Some(&*self.primary),
            2 => self.rail.as_ref().map(|d| d as &dyn Widget),
            _ => None,
        }
    }
    fn slot_mut(&mut self, i: usize) -> Option<&mut dyn Widget> {
        match i {
            0 => self.strip.as_deref_mut(),
            1 => Some(&mut *self.primary),
            2 => self.rail.as_mut().map(|d| d as &mut dyn Widget),
            _ => None,
        }
    }
    fn slot_bounds(&self, i: usize) -> Rect {
        match i {
            0 => self.strip_b,
            1 => self.prim_b,
            _ => self.rail_b,
        }
    }
}

impl Widget for Page {
    fn debug_name(&self) -> &'static str {
        "Page"
    }

    fn measure(&mut self, cx: &mut LayoutContext, c: LayoutConstraints) -> glam::Vec2 {
        let w = if c.max_size.x.is_finite() && c.max_size.x <= MAX_REPORTED_DIM {
            c.max_size.x.max(0.0)
        } else {
            FILL_FALLBACK
        };
        let strip_h = self
            .strip
            .as_mut()
            .map(|s| {
                s.measure(
                    cx,
                    LayoutConstraints {
                        min_size: glam::Vec2::ZERO,
                        max_size: glam::Vec2::new(w, f32::MAX),
                    },
                )
                .y
            })
            .unwrap_or(0.0);
        // Report a bounded intrinsic — the Tabs panel hands real
        // bounds at layout; measure exists so scroller-style parents
        // and tests get a sane request instead of f32::MAX.
        let h = if c.max_size.y.is_finite() && c.max_size.y <= MAX_REPORTED_DIM {
            c.max_size.y
        } else {
            strip_h + FILL_FALLBACK
        };
        self.primary.measure(
            cx,
            LayoutConstraints {
                min_size: glam::Vec2::ZERO,
                max_size: glam::Vec2::new(w, (h - strip_h).max(0.0)),
            },
        );
        if let Some(r) = self.rail.as_mut() {
            r.measure(
                cx,
                LayoutConstraints {
                    min_size: glam::Vec2::ZERO,
                    max_size: glam::Vec2::new(w * RAIL_FRAC, (h - strip_h).max(0.0)),
                },
            );
        }
        glam::Vec2::new(w, h.max(strip_h))
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        let s = cx.scale;
        let w_pt = self
            .zone_width
            .get()
            .max(bounds.width() / s.max(f32::EPSILON));
        let gap = cx.pt(ZONE_GAP);
        let pad_top = bounds.min_y();

        // Strip — intrinsic height, full width.
        let mut body_y = pad_top;
        if let Some(strip) = self.strip.as_mut() {
            let h = strip
                .measure(
                    cx,
                    LayoutConstraints {
                        min_size: glam::Vec2::ZERO,
                        max_size: glam::Vec2::new(bounds.width(), f32::MAX),
                    },
                )
                .y;
            self.strip_b = Rect::new(bounds.min_x(), body_y, bounds.width(), h);
            strip.layout(cx, self.strip_b);
            body_y += h + gap;
        } else {
            self.strip_b = Rect::default();
        }
        let body = Rect::new(
            bounds.min_x(),
            body_y,
            bounds.width(),
            (bounds.max_y() - body_y).max(0.0),
        );

        let Some(rail) = self.rail.as_mut() else {
            // Theater / Wall / Centered-without-rail: primary owns body.
            self.prim_b = if self.variant == Variant::Centered {
                let cw = body.width().min(cx.pt(CENTERED_W));
                Rect::new(
                    body.min_x() + (body.width() - cw) * 0.5,
                    body.min_y(),
                    cw,
                    body.height(),
                )
            } else {
                body
            };
            self.rail_b = Rect::default();
            self.primary.layout(cx, self.prim_b);
            return;
        };
        let wide = w_pt >= RAIL_STACK_W;
        let narrow = w_pt < RAIL_DISCLOSE_W;
        if !narrow {
            // At stacking/wide widths the rail is always open — a rail
            // only collapses into a toggleable disclosure when there
            // isn't room to show it.
            rail.set_open(true);
        }

        let centered_body = if self.variant == Variant::Centered {
            let cw = body.width().min(cx.pt(CENTERED_W));
            Rect::new(
                body.min_x() + (body.width() - cw) * 0.5,
                body.min_y(),
                cw,
                body.height(),
            )
        } else {
            body
        };
        let body = centered_body;

        match (self.variant, wide) {
            (Variant::MasterLeft, true) => {
                let mw = body.width() * MASTER_FRAC;
                self.rail_b = Rect::new(body.min_x(), body.min_y(), mw, body.height());
                self.prim_b = Rect::new(
                    body.min_x() + mw + gap,
                    body.min_y(),
                    (body.width() - mw - gap).max(0.0),
                    body.height(),
                );
            }
            (_, true) => {
                let rw = body.width() * RAIL_FRAC;
                self.prim_b = Rect::new(
                    body.min_x(),
                    body.min_y(),
                    (body.width() - rw - gap).max(0.0),
                    body.height(),
                );
                self.rail_b = Rect::new(
                    body.min_x() + body.width() - rw,
                    body.min_y(),
                    rw,
                    body.height(),
                );
            }
            (Variant::MasterLeft, false) => {
                // Master above the artifact it selects.
                let mh = body.height() * if narrow { 0.0 } else { 0.38 };
                if narrow {
                    // Disclosure header height — measure closed.
                    let was = rail.open;
                    rail.set_open(false);
                    let hh = rail
                        .measure(
                            cx,
                            LayoutConstraints {
                                min_size: glam::Vec2::ZERO,
                                max_size: glam::Vec2::new(body.width(), f32::MAX),
                            },
                        )
                        .y;
                    rail.set_open(was);
                    let dh = if rail.open { body.height() * 0.5 } else { hh };
                    self.rail_b = Rect::new(body.min_x(), body.min_y(), body.width(), dh);
                    self.prim_b = Rect::new(
                        body.min_x(),
                        body.min_y() + dh + gap,
                        body.width(),
                        (body.height() - dh - gap).max(0.0),
                    );
                } else {
                    self.rail_b = Rect::new(body.min_x(), body.min_y(), body.width(), mh);
                    self.prim_b = Rect::new(
                        body.min_x(),
                        body.min_y() + mh + gap,
                        body.width(),
                        (body.height() - mh - gap).max(0.0),
                    );
                }
            }
            (_, false) => {
                // Detail rail below the primary.
                if narrow {
                    let was = rail.open;
                    rail.set_open(false);
                    let hh = rail
                        .measure(
                            cx,
                            LayoutConstraints {
                                min_size: glam::Vec2::ZERO,
                                max_size: glam::Vec2::new(body.width(), f32::MAX),
                            },
                        )
                        .y;
                    rail.set_open(was);
                    let dh = if rail.open { body.height() * 0.42 } else { hh };
                    self.rail_b = Rect::new(body.min_x(), body.max_y() - dh, body.width(), dh);
                    self.prim_b = Rect::new(
                        body.min_x(),
                        body.min_y(),
                        body.width(),
                        (body.height() - dh - gap).max(0.0),
                    );
                } else {
                    let rh = body.height() * 0.38;
                    self.prim_b = Rect::new(
                        body.min_x(),
                        body.min_y(),
                        body.width(),
                        (body.height() - rh - gap).max(0.0),
                    );
                    self.rail_b = Rect::new(body.min_x(), body.max_y() - rh, body.width(), rh);
                }
            }
        }
        self.primary.layout(cx, self.prim_b);
        rail.layout(cx, self.rail_b);
    }

    fn paint(&self, _cx: &mut PaintContext) {}

    fn clips_children(&self) -> bool {
        true
    }

    fn accessibility(&self, node: &mut accesskit::Node) {
        node.set_role(accesskit::Role::Group);
    }

    fn child_count(&self) -> usize {
        self.child_count_inner()
    }
    fn child(&self, index: usize) -> Option<&dyn Widget> {
        self.order().get(index).and_then(|&s| self.slot(s))
    }
    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        self.order().get(index).and_then(|&s| self.slot_mut(s))
    }
    fn child_bounds(&self, index: usize) -> Option<Rect> {
        self.order().get(index).map(|&s| self.slot_bounds(s))
    }
}

/// Exclusive view swapper — exactly one child visible at a time,
/// selected by a `Signal<usize>`. All views stay mounted and laid
/// out, so selection/scroll/focus state survives a swap (the
/// grammar's "state carries across selector swaps" rule). The
/// context-strip `Segmented` writes the signal; hidden views are
/// inert (no events, no paint, no a11y children).
pub struct Swap {
    views: Vec<Box<dyn Widget>>,
    sel: Signal<usize>,
    bounds: Rect,
    active: usize,
}

impl Swap {
    /// `sel` is the view index signal — a strip `Segmented` writes it.
    pub fn new(sel: &Signal<usize>) -> Self {
        Self {
            views: Vec::new(),
            sel: sel.clone(),
            bounds: Rect::default(),
            active: 0,
        }
    }

    /// Add a view; index order matches the selector's option order.
    pub fn view(mut self, w: impl Widget + 'static) -> Self {
        self.views.push(Box::new(w));
        self
    }
}

impl Widget for Swap {
    fn debug_name(&self) -> &'static str {
        "Swap"
    }

    fn measure(&mut self, cx: &mut LayoutContext, c: LayoutConstraints) -> glam::Vec2 {
        let mut size = glam::Vec2::ZERO;
        for v in self.views.iter_mut() {
            let s = v.measure(cx, c);
            size = size.max(s);
        }
        size
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.active = self.sel.get().min(self.views.len().saturating_sub(1));
        // Every view gets real bounds — a hidden view swapped in later
        // must not surface with stale/zero geometry.
        for v in self.views.iter_mut() {
            v.layout(cx, bounds);
        }
    }

    fn paint(&self, _cx: &mut PaintContext) {}

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match self.views.get_mut(self.active) {
            Some(v) => v.event(cx),
            None => EventResponse::Ignored,
        }
    }

    fn tick(&mut self, dt: Duration) -> bool {
        let sel = self.sel.get().min(self.views.len().saturating_sub(1));
        let swapped = sel != self.active;
        self.active = sel;
        let mut dirty = swapped;
        // Only the visible view ticks — hidden views are suspended,
        // matching the Tabs pages' suspend-inactive invariant.
        if let Some(v) = self.views.get_mut(self.active) {
            dirty |= v.tick(dt);
        }
        dirty
    }

    fn clips_children(&self) -> bool {
        true
    }

    fn accessibility(&self, node: &mut accesskit::Node) {
        node.set_role(accesskit::Role::Group);
    }

    fn child_count(&self) -> usize {
        // Only the active view is an a11y/hit-test child — hidden
        // views are inert (the hidden-tab-panel rule, applied to
        // in-page swaps).
        usize::from(self.active < self.views.len())
    }
    fn child(&self, index: usize) -> Option<&dyn Widget> {
        (index == 0)
            .then(|| self.views.get(self.active))
            .flatten()
            .map(|v| &**v)
    }
    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        (index == 0)
            .then(|| self.views.get_mut(self.active))
            .flatten()
            .map(|v| &mut **v)
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
                let mut lcx = LayoutContext {
                    hot: &mut self.scratch_hot,
                    scale,
                };
                // A `*w = build(m)` re-seat produces a widget whose
                // layout caches (Flex::child_sizes, …) are empty —
                // measure first or every child lays out at ZERO.
                self.widget.measure(
                    &mut lcx,
                    LayoutConstraints {
                        min_size: glam::Vec2::ZERO,
                        max_size: bounds.size,
                    },
                );
                self.widget.layout(&mut lcx, bounds);
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

    fn child_clip(&self, index: usize) -> Option<Rect> {
        self.widget.child_clip(index)
    }

    fn paint_extent(&self) -> Option<Rect> {
        self.widget.paint_extent()
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

    /// A re-seated Flex must lay children out at real bounds — the
    /// push relayout path measures first, so a swapped-in widget
    /// never paints with zeroed caches.
    #[test]
    fn reseated_flex_children_get_real_bounds() {
        use martensite::widgets::button::Button;
        use martensite::widgets::flex::Flex;
        let m = model();
        let mut b = Bound::new(
            Flex::column()
                .gap(ZONE_GAP)
                .child_flex(Button::new("a"), 1.0)
                .child(Button::new("b")),
            &m,
        )
        .push(move |w: &mut Flex, m| {
            if m.selected_asset.get().is_some() {
                *w = Flex::column()
                    .gap(ZONE_GAP)
                    .child_flex(Button::new("x"), 1.0)
                    .child(Button::new("y"));
            }
        });
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        let bounds = Rect::new(0.0, 0.0, 300.0, 200.0);
        b.layout(&mut cx, bounds);
        m.selected_asset.set(Some(1));
        b.tick(Duration::from_millis(16));
        let h0 = b.child_bounds(0).map(|r| r.size.y).unwrap_or(-1.0);
        assert!(h0 > 100.0, "re-seated flex child 0 got height {h0}");
    }

    /// The Page + Swap + weighted-Flex stack from the zone pages:
    /// children must receive real bounds after a tick re-seat.
    #[test]
    fn page_swap_children_get_real_bounds() {
        use martensite::widgets::button::Button;
        use martensite::widgets::flex::Flex;
        let m = model();
        let sel = Signal::new(0usize);
        // Mirror the register column: Bound-wrapped weighted + plain
        // children, matching the zone-page composition exactly.
        let big = Bound::new(Button::new("big"), &m).push(|w: &mut Button, m| {
            let _ = m;
            let _ = w;
        });
        let view = Flex::column()
            .gap(ZONE_GAP)
            .child_flex(big, 1.0)
            .child(Bound::new(Button::new("small"), &m));
        let rail_col = Flex::column()
            .gap(ZONE_GAP)
            .child(Button::new("rail-a"))
            .child_flex(Button::new("rail-b"), 1.0);
        let mut page = Page::new(
            Variant::MasterDetail,
            fill(Swap::new(&sel).view(view)),
            &m.zone_width,
        )
        .strip(strip().child(Button::new("s")))
        .rail("R", rail_col);
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        // Arena order: measure, layout, tick.
        page.measure(
            &mut cx,
            LayoutConstraints {
                min_size: glam::Vec2::ZERO,
                max_size: glam::Vec2::new(926.0, 205.0),
            },
        );
        page.layout(&mut cx, Rect::new(0.0, 0.0, 926.0, 205.0));
        page.tick(Duration::from_millis(16));
        // children: strip(0), primary(1), rail(2)
        for i in 0..page.child_count() {
            eprintln!("child {i} -> {:?}", page.child_bounds(i));
        }
        let prim = page.child_bounds(1).expect("primary");
        assert!(prim.size.y > 50.0, "primary height {}", prim.size.y);
        let rail = page.child_bounds(2).expect("rail");
        assert!(rail.size.y > 50.0, "rail height {}", rail.size.y);
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
