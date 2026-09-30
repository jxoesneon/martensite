//! Functional zones — the contextual-dashboard composition per the
//! Design Council verdict (docket `20260921`): every panel is an
//! operational surface where each mounted widget is bound to the
//! [`PlantModel`] through [`Bound`](crate::zone::Bound), organised into domain-named
//! `Tabs` pages — one visible surface at a time, no exhibit walls.
//!
//! Each `zones/<panel>.rs` module returns its pages as
//! `(domain label, page column)` pairs via [`ZonePanel`]:
//!
//! ```text
//! ZonePanel
//! ├── operational view (the existing panel widget, untouched)
//! └── Tabs — domain-named zone pages, one shown at a time
//!       └── page: Flex column of Bound widgets
//! ```
//!
//! The page builders live in the sibling modules; this file owns the
//! shared panel shell.

use glam::Vec2;
use martensite::core::{
    LayoutConstraints, LayoutContext, PaintContext, Rect, RenderMinimum, UnderflowPolicy, Widget,
};
use martensite::reactive::Signal;
use martensite::render::Point;
use martensite::widgets::badge::BadgeSpec;
use martensite::widgets::tabs::Tabs;
use parking_lot::Mutex;
use std::time::Duration;

use crate::domain::PlantModel;
use crate::model::Palette;
use crate::panels::{krect, panel_border, TITLE_H};
use crate::text::TextPainter;
use crate::zone::Page;

pub mod editor;
pub mod grid;
pub mod media;
pub mod telemetry;

/// Fraction of the panel height the operational view keeps — zone
/// tabs fill the remainder (~70%, per the ratified layout grammar).
const OP_FRAC: f32 = 0.30;

/// Tombstone floor (logical pt): below ~120×80pt not even a tab strip
/// plus a content sliver fits, so `paint_underflow` takes over. The
/// full stacked layout wants ~320×240pt; everything between the two
/// is served by the compact tiers in `layout` — a tiled/compositor-
/// shrunk panel degrades, it does not blank.
const UNDERFLOW_W: f32 = 120.0;
const UNDERFLOW_H: f32 = 80.0;

/// Height the compact tiers reserve for the tab strip (logical pt) —
/// mirrors `Tabs`' private `STRIP_H` so navigation survives a short
/// panel even when the active page's own slice collapses to zero.
const TAB_STRIP_PT: f32 = 32.0;

/// A panel = operational view on top + domain-named zone pages below.
/// The operational widget is untouched (keeps its own chrome, focus,
/// events); the `Tabs` provides one-visible-surface semantics — hidden
/// pages are suspended by the arena's bounds-gated paint/tick walks.
///
/// Cross-zone navigation: an artifact action writes
/// `model.page_request[zone_index] = Some(page)`; `tick` drains the
/// slot (take semantics — cleared on consume) and activates the tab.
/// `layout` publishes the content width into
/// `model.zone_width[zone_index]` — one slot per zone so panels can't
/// contaminate each other — which `Page` reads for its rail-collapse
/// breakpoints.
pub struct ZonePanel {
    inner: Box<dyn Widget>,
    zones: Tabs,
    title: &'static str,
    zone_index: usize,
    model: PlantModel,
    bounds: Rect,
    inner_bounds: Rect,
    zones_bounds: Rect,
    scale: Signal<f32>,
    text: Mutex<TextPainter>,
    /// The tab index seen at the end of the previous tick — diffs
    /// `zones.selected()` to tell a user-driven tab click apart from
    /// a `page_request`-driven `activate` (a manual nav abandons any
    /// pending deep-link sub for this zone).
    last_selected: usize,
}

impl ZonePanel {
    /// `pages` are `(domain label, Page)` pairs — the zone's tab names
    /// are domain names, never widget names. `zone_index` is the
    /// panel's slot in `model.page_request` (0=grid, 1=telemetry,
    /// 2=editor, 3=media).
    pub fn new(
        inner: Box<dyn Widget>,
        title: &'static str,
        scale: Signal<f32>,
        model: &PlantModel,
        zone_index: usize,
        pages: Vec<(&'static str, Page)>,
    ) -> Self {
        let mut zones = Tabs::new();
        for (i, (label, page)) in pages.into_iter().enumerate() {
            // ZONE_PAD insets every page's content from the tab
            // chrome — the published grammar token, applied here so
            // page builders stay flush-left internally. No ScrollView
            // wrapper: pages are bounded-fill layouts; dense surfaces
            // scroll internally.
            let padded = martensite::widgets::container::Container::new()
                .padding_uniform(crate::zone::ZONE_PAD)
                .child(page);
            // C2-lite — a tab concealing an alarm-bearing channel
            // carries a count+worst-severity badge. The badge is
            // sourced from the model here, at the always-mounted
            // panel level — never from a hidden page's tick.
            zones = match zone_tab_badge(zone_index, model, i) {
                Some(badge) => zones.tab_with_badge(label, padded, badge),
                None => zones.tab(label, padded),
            };
        }
        let mut panel = Self {
            inner,
            zones,
            title,
            zone_index,
            model: model.clone(),
            bounds: Rect::default(),
            inner_bounds: Rect::default(),
            zones_bounds: Rect::default(),
            scale,
            text: Mutex::new(TextPainter::new()),
            last_selected: 0,
        };
        // C2-lite — the default-selected page is the one carrying
        // active unacked alarms (the annunciated surface is where
        // the operator lands).
        if let Some(tab) = zone_default_tab(zone_index, model) {
            panel.zones.activate(tab);
        }
        panel.last_selected = panel.zones.selected();
        panel
    }

    fn s(&self) -> f32 {
        self.scale.get()
    }
}

/// Telemetry tab indices — keep in sync with `telemetry::pages()`
/// order (TRENDS, INSTRUMENTS, ALARMS, …).
const TELEMETRY_ALARMS_TAB: usize = 2;

/// C2-lite — the `BadgeSpec` a zone tab conceals, resolved from the
/// model. The grid panel's per-tab alarm channels dispatch through
/// `grid::tab_badge`; telemetry's ALARMS board is the plant-wide
/// channel. Other panels conceal no alarm channels today — add a
/// `tab_badge` next to their `pages()` when they do.
fn zone_tab_badge(zone_index: usize, model: &PlantModel, tab: usize) -> Option<BadgeSpec> {
    match zone_index {
        0 => grid::tab_badge(model, tab as u8),
        1 if tab == TELEMETRY_ALARMS_TAB => grid::alarm_badge(model, None),
        _ => None,
    }
}

/// C2-lite — the panel's landing tab: the first page concealing
/// active unacked alarms, when the zone declares one. Grid computes
/// its own; telemetry lands on ALARMS whenever the plant-wide
/// channel has unacked alarms (and on TRENDS otherwise).
fn zone_default_tab(zone_index: usize, model: &PlantModel) -> Option<usize> {
    match zone_index {
        0 => Some(grid::default_tab(model)),
        1 => grid::alarm_badge(model, None).map(|_| TELEMETRY_ALARMS_TAB),
        _ => None,
    }
}

impl Widget for ZonePanel {
    fn debug_name(&self) -> &'static str {
        self.title
    }

    fn measure(&mut self, cx: &mut LayoutContext, c: LayoutConstraints) -> Vec2 {
        let inner = self.inner.measure(cx, c);
        Vec2::new(inner.x, inner.y / OP_FRAC)
    }

    fn min_render(&self) -> RenderMinimum {
        // The honest floor — below ~120×80pt no tier (op view, tab
        // strip, page slice) renders meaningfully. Between this and
        // the full ~320×240pt stack, `layout` runs compact tiers:
        // tiling WMs hand panels whatever the grid says, so a
        // shrunken panel must degrade rather than tombstone.
        RenderMinimum::new(Vec2::new(UNDERFLOW_W, UNDERFLOW_H))
            .with_policy(UnderflowPolicy::Fallback)
    }

    fn paint_underflow(&self, cx: &mut PaintContext) {
        let pal = Palette::from_theme(cx.theme);
        let s = self.s();
        let mut text = self.text.lock();
        let b = krect(
            f64::from(self.bounds.min_x()),
            f64::from(self.bounds.min_y()),
            f64::from(self.bounds.width()),
            f64::from(self.bounds.height()),
        );
        cx.list.push_fill_rect(b, pal.surface);
        cx.list.push_stroke_rect(b, 1.0, pal.border);
        // Named — a shrunken page must still identify which panel
        // starved.
        let msg = format!("{} — enlarge to restore", self.title.to_uppercase());
        let size = 12.0 * s;
        let tw = f64::from(text.measure(&msg, size));
        let x = (b.x0 + (b.width() - tw) * 0.5).max(b.x0 + 2.0);
        let y = b.y0 + (b.height() - f64::from(size)) * 0.5;
        text.push(cx.list, Point::new(x, y), &msg, size, pal.text_muted, None);
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        let s = self.s();
        let s_safe = s.max(f32::EPSILON);
        let inner_min = self.inner.min_render().size;
        let strip = TAB_STRIP_PT * s;
        // Compact-tier selection (the comparison runs in pt so the
        // declared pt floors stay honest at any scale):
        //
        // - Viable — the operational view can meet its own declared
        //   `min_render` on both axes AND leave a tab strip: it keeps
        //   the top at its usual share, and the zones take the rest
        //   (`Tabs` degrades to strip-only when the remainder is
        //   short — navigation still works, the active page's slice
        //   just shrinks).
        // - Sub-viable — the op view could only paint its own
        //   underflow chrome (or collapse outright): the zone pages
        //   take the whole panel. A working tab strip plus a clipped
        //   page slice is a better degrade than a nested tombstone —
        //   the panel's priority content is whatever surface the
        //   operator switched to.
        let viable = bounds.width() / s_safe >= inner_min.x
            && bounds.height() / s_safe >= inner_min.y + TAB_STRIP_PT;
        let inner_h = if viable {
            // The `min(height - strip)` cap is a guard — viability
            // already implies `height - strip ≥ inner_min`, so the
            // zones region never loses its strip to the op view.
            (bounds.height() * OP_FRAC)
                .max(TITLE_H * s + 24.0)
                .max(inner_min.y * s)
                .min(bounds.height() - strip)
                .max(0.0)
        } else {
            0.0
        };
        self.inner_bounds = Rect::new(bounds.min_x(), bounds.min_y(), bounds.width(), inner_h);
        self.zones_bounds = Rect::new(
            bounds.min_x(),
            bounds.min_y() + inner_h + 1.0,
            bounds.width(),
            (bounds.height() - inner_h - 1.0).max(0.0),
        );
        // Publish the content width (pt) — `Page` reads it for the
        // rail-collapse breakpoints. Guarded so hover-noise relayouts
        // don't churn the signal.
        let w_pt = bounds.width() / s.max(f32::EPSILON);
        if (self.model.zone_width[self.zone_index].get() - w_pt).abs() > 0.5 {
            self.model.zone_width[self.zone_index].set(w_pt);
        }
        // Publish the tab-strip rect (device px, window space) beside
        // it — the first-run tour reads it for its zone-tabs coach
        // mark. `Tabs` pins its strip to the top `STRIP_H` of its
        // bounds; `TAB_STRIP_PT` is this crate's mirror of it.
        self.model.zone_tabs[self.zone_index].set_if_changed(Rect::new(
            self.zones_bounds.min_x(),
            self.zones_bounds.min_y(),
            self.zones_bounds.width(),
            (TAB_STRIP_PT * s).min(self.zones_bounds.height()),
        ));
        self.inner.layout(cx, self.inner_bounds);
        self.zones.layout(cx, self.zones_bounds);
    }

    // The drain lives in `tick` — `inner`/`zones` are internal
    // children and the arena's bounds-gated `tick_recursive` already
    // ticks them through `child_mut`, so this override only handles
    // the nav request + tab-badge sync (ticking children here too
    // would double their rate — the TelemetryPanel's `elapsed += dt`
    // would run at 2×).
    fn tick(&mut self, _dt: Duration) -> bool {
        let mut dirty = false;
        // Programmatic nav wins a same-tick race with a user click —
        // a `page_request`/`request_page_deep` is a semantic intent
        // (deep link, alarm ack) posted by another surface; the user's
        // next click always lands normally.
        let mut requested = false;
        let mut req = self.model.page_request.get();
        if let Some(page) = req.get_mut(self.zone_index).and_then(Option::take) {
            self.model.page_request.set(req);
            // Only a request that lands counts as requested — an
            // out-of-range page can't activate a tab, and treating it
            // as requested would suppress the pending-sub clear and
            // orphan the deep link until manual nav.
            if (page as usize) < self.zones.tab_count() {
                self.zones.activate(page as usize);
                requested = true;
            }
            dirty = true;
        }
        // A manual tab change — selected moved without a page_request
        // drain — abandons any pending deep-link sub for this zone:
        // the user overrode the intent before the target page ticked,
        // so it must not fire stale the next time that page opens.
        if !requested && self.zones.selected() != self.last_selected {
            let mut sub = self.model.page_request_sub.get();
            if sub[self.zone_index].take().is_some() {
                self.model.page_request_sub.set(sub);
            }
        }
        self.last_selected = self.zones.selected();
        // C2-lite — tab badges re-resolve from the model every tick.
        // This panel is always mounted, so annunciation never depends
        // on a suspended page's `Bound::push` (hidden pages don't
        // tick). `set_tab_badge` only runs on a real change, keeping
        // idle ticks clean.
        for i in 0..self.zones.tab_count() {
            let want = zone_tab_badge(self.zone_index, &self.model, i);
            if self.zones.tab_badge(i) != want.as_ref() {
                self.zones.set_tab_badge(i, want);
                dirty = true;
            }
        }
        dirty
    }

    fn paint(&self, cx: &mut PaintContext) {
        let pal = Palette::from_theme(cx.theme);
        let s = self.s();
        // Divider between the operational view and the zone tabs —
        // skipped when either region collapsed (a degenerate height
        // would otherwise paint a stray hairline along an edge).
        if self.inner_bounds.height() > 0.0 && self.zones_bounds.height() > 0.0 {
            let y = f64::from(self.zones_bounds.min_y() - 0.5);
            cx.list.push_fill_rect(
                krect(
                    f64::from(self.bounds.min_x()),
                    y,
                    f64::from(self.bounds.width()),
                    1.0,
                ),
                pal.border,
            );
        }
        panel_border(cx.list, self.bounds, &pal, s, false);
    }

    fn child_count(&self) -> usize {
        2
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        match index {
            0 => Some(&*self.inner as &dyn Widget),
            1 => Some(&self.zones as &dyn Widget),
            _ => None,
        }
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        match index {
            0 => Some(&mut *self.inner as &mut dyn Widget),
            1 => Some(&mut self.zones as &mut dyn Widget),
            _ => None,
        }
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        match index {
            0 => Some(self.inner_bounds),
            1 => Some(self.zones_bounds),
            _ => None,
        }
    }

    fn clips_children(&self) -> bool {
        true
    }

    fn accessibility(&self, node: &mut accesskit::Node) {
        node.set_role(accesskit::Role::Group);
        node.set_label(format!(
            "{} — operational view with functional zones",
            self.title
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::zone::{fill, Page, Variant};
    use martensite::core::widget::DummyWidget;
    use martensite::core::HotNode;

    /// Probe inner with a declared render floor — stands in for an
    /// operational panel so the compact-tier switch is testable
    /// without dragging a real panel's widget tree into the test.
    struct Floored {
        min: Vec2,
    }

    impl Widget for Floored {
        fn debug_name(&self) -> &'static str {
            "Floored"
        }
        fn measure(&mut self, _cx: &mut LayoutContext, _c: LayoutConstraints) -> Vec2 {
            self.min
        }
        fn layout(&mut self, _cx: &mut LayoutContext, _bounds: Rect) {}
        fn paint(&self, _cx: &mut PaintContext) {}
        fn min_render(&self) -> RenderMinimum {
            RenderMinimum::new(self.min).with_policy(UnderflowPolicy::Fallback)
        }
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

    /// A zone panel over a `Floored` inner at `min`, scale 2.0 —
    /// the observed Hyprland tile ran at scale 2.
    fn floored_panel(m: &PlantModel, min: Vec2) -> ZonePanel {
        let page =
            |m: &PlantModel| Page::new(Variant::Theater, fill(DummyWidget), &m.zone_width[2]);
        ZonePanel::new(
            Box::new(Floored { min }),
            "TEST",
            Signal::new(2.0),
            m,
            2,
            vec![("A", page(m)), ("B", page(m))],
        )
    }

    /// Toolbar chrome launchers write `page_request[zone]`; the owning
    /// ZonePanel's `tick` must drain the slot and activate the tab —
    /// exactly once.
    #[test]
    fn page_request_activates_the_zone_tab() {
        let m = model();
        let page =
            |m: &PlantModel| Page::new(Variant::Theater, fill(DummyWidget), &m.zone_width[0]);
        let mut panel = ZonePanel::new(
            Box::new(DummyWidget),
            "TEST",
            Signal::new(1.0),
            &m,
            2,
            vec![
                ("A", page(&m)),
                ("B", page(&m)),
                ("C", page(&m)),
                ("CHROME", page(&m)),
            ],
        );
        m.request_page(2, 3);
        assert!(panel.tick(Duration::from_millis(16)));
        assert_eq!(panel.zones.selected(), 3);
        assert_eq!(m.page_request.get()[2], None, "request left pending");
        // A second tick must not re-fire — the slot is drained.
        assert!(!panel.tick(Duration::from_millis(16)));
        // A request for another zone leaves this panel alone.
        m.request_page(0, 1);
        assert!(!panel.tick(Duration::from_millis(16)));
        assert_eq!(panel.zones.selected(), 3);
        assert_eq!(m.page_request.get()[0], Some(1));
    }

    /// `TELEMETRY_ALARMS_TAB` is pinned to `telemetry::pages()` order
    /// by comment — this test is the mechanical check: a page reorder
    /// that drops ALARMS out of index 2 must fail here, not silently
    /// mis-badge TRENDS.
    #[test]
    fn telemetry_alarms_tab_index_matches_pages() {
        let m = model();
        let pages = crate::zones::telemetry::pages(&m);
        assert_eq!(
            pages.get(TELEMETRY_ALARMS_TAB).map(|(l, _)| *l),
            Some("ALARMS"),
            "telemetry page order changed — update TELEMETRY_ALARMS_TAB"
        );
    }

    /// The collapsed-panel regression: a compositor-tiled window once
    /// handed the panels ~315×175 logical pt (630×350px at scale
    /// 2) — under the old 320×240 floor both tombstoned on first
    /// launch. The declared floor must now sit under the allotment so
    /// the arena never engages `paint_underflow`, and `layout` must
    /// still produce a visible op-view + tab-strip split.
    #[test]
    fn compact_tier_keeps_content_at_tiled_sizes() {
        let m = model();
        let mut panel = floored_panel(&m, Vec2::new(220.0, 130.0));
        let min = panel.min_render();
        assert_eq!(min.policy, UnderflowPolicy::Fallback);
        assert!(
            min.size.x <= 315.0 && min.size.y <= 175.0,
            "floor {min:?} would still tombstone a 315×175pt panel"
        );
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 2.0,
        };
        panel.layout(&mut cx, Rect::new(0.0, 0.0, 630.0, 350.0));
        // Viable tier: the op view gets at least its declared floor and
        // the zones keep at least a full tab strip.
        assert!(
            panel.inner_bounds.height() >= 130.0 * 2.0,
            "op view shrank under its own floor: {:?}",
            panel.inner_bounds
        );
        assert!(
            panel.zones_bounds.height() >= TAB_STRIP_PT * 2.0,
            "tab strip lost: {:?}",
            panel.zones_bounds
        );
    }

    /// Below the op view's viability floor the zone pages take the
    /// whole panel — a working tab strip plus a clipped page slice is
    /// a better degrade than an inner tombstone or a collapsed slot.
    #[test]
    fn sub_viable_size_gives_zones_the_panel() {
        let m = model();
        let mut panel = floored_panel(&m, Vec2::new(220.0, 130.0));
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 2.0,
        };
        // 315×140pt — under inner_min + strip (162pt) → zones-priority.
        panel.layout(&mut cx, Rect::new(0.0, 0.0, 630.0, 280.0));
        assert_eq!(panel.inner_bounds.height(), 0.0);
        assert!(
            panel.zones_bounds.height() >= 279.0,
            "zones did not inherit the panel: {:?}",
            panel.zones_bounds
        );
        // 190×400pt — tall but under the op view's 220pt width floor →
        // the same zones-priority switch applies on the narrow axis.
        panel.layout(&mut cx, Rect::new(0.0, 0.0, 380.0, 800.0));
        assert_eq!(panel.inner_bounds.height(), 0.0);
        assert!(panel.zones_bounds.height() >= 799.0);
        // Tab navigation still works in the collapsed tier.
        panel.zones.activate(1);
        assert_eq!(panel.zones.selected(), 1);
    }

    /// Above the compact range the split is unchanged — the op view
    /// keeps its `OP_FRAC` share and the zones take the remainder.
    #[test]
    fn full_layout_unchanged_above_compact_floor() {
        let m = model();
        let mut panel = floored_panel(&m, Vec2::new(220.0, 130.0));
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 2.0,
        };
        panel.layout(&mut cx, Rect::new(0.0, 0.0, 1280.0, 960.0));
        // inner_h = max(960·0.3, TITLE_H·2+24, 130·2) = 288px.
        assert_eq!(panel.inner_bounds.height(), 288.0);
        assert_eq!(panel.zones_bounds.height(), 671.0);
    }
}
