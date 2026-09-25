//! Contextual widget showcase — every facade widget mounted inside
//! the dashboard panel where it belongs. This is the comprehensive
//! dogfood: all 274 `martensite::widgets` modules are instantiated
//! from their own documented construction (their doctests) and
//! mounted through the internal-children protocol (`ScrollView` →
//! `Flex` → `FlowBox` → `Clamp` → `GroupBox` → widget), so event
//! routing, paint traversal, a11y virtual nodes, and `tick`
//! delivery are exercised the way a third-party composition hits
//! them.
//!
//! Placement is semantic, not a flat catalog:
//!
//! - **Process Grid** hosts data & navigation widgets — tables,
//!   trees, JSON/hex/log viewers, and the nav controls a process
//!   browser actually uses.
//! - **Telemetry** hosts charts & instrumentation — every gauge,
//!   dial, and indicator is monitoring content.
//! - **Editor** hosts forms, pickers & chrome — field
//!   configuration plus the actions (buttons, menus, dialogs)
//!   that operate on it.
//! - **Media** hosts comms & media widgets — transport, volume,
//!   conferencing, music.
//!
//! Each `mod` contributes `entries() -> Vec<Entry>` — `(display
//! name, live widget)` pairs. `card()` wraps each in a titled
//! `GroupBox` so the showcase is self-labeling.

use std::time::Duration;

use glam::Vec2;
use martensite::core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget,
};
use martensite::reactive::Signal;
use martensite::render::Point;
use martensite::widgets::clamp::Clamp;
use martensite::widgets::flex::Flex;
use martensite::widgets::flow_box::FlowBox;
use martensite::widgets::group_box::GroupBox;
use martensite::widgets::scrollview::ScrollView;
use martensite::widgets::text::Text;
use parking_lot::Mutex;

use crate::model::Palette;
use crate::panels::{krect, panel_border, TITLE_H};
use crate::text::TextPainter;
use crate::zone::{ZONE_GAP, ZONE_SECTION};

mod charts;
mod chrome;
mod data;
mod feedback;
mod forms;
mod nav;
mod pickers;
mod social;

/// A named live-widget demo: `(display name, widget instance)`.
pub type Entry = (&'static str, Box<dyn Widget>);

/// Fraction of the panel height the operational view keeps — the
/// showcase scroll region fills the remainder.
const OP_FRAC: f32 = 0.58;

/// The showcase host: an operational panel on top, a scrollable
/// grid of contextually-related live widgets below. The inner panel
/// is untouched — it gets a `Widget` child slot and keeps its own
/// chrome, focus, and events.
pub struct ShowcasePanel {
    inner: Box<dyn Widget>,
    view: ScrollView,
    title: &'static str,
    bounds: Rect,
    inner_bounds: Rect,
    view_bounds: Rect,
    scale: Signal<f32>,
    text: Mutex<TextPainter>,
}

impl ShowcasePanel {
    /// Wraps `inner` with a scrollable showcase built from
    /// `sections` — `(header, entries)` pairs in display order.
    pub fn new(
        inner: Box<dyn Widget>,
        title: &'static str,
        scale: Signal<f32>,
        sections: Vec<(&'static str, Vec<Entry>)>,
    ) -> Self {
        Self {
            inner,
            view: ScrollView::new(showcase_column(sections)),
            title,
            bounds: Rect::default(),
            inner_bounds: Rect::default(),
            view_bounds: Rect::default(),
            scale,
            text: Mutex::new(TextPainter::new()),
        }
    }

    fn s(&self) -> f32 {
        self.scale.get()
    }
}

/// Builds the showcase column: each section's cards under a header,
/// in one vertically scrolling `Flex`.
///
/// Section rhythm (spec B7 + the layout grammar): `ZONE_SECTION`
/// separates sections — each a quiet `group_label` caption paired
/// with its card field at the 4 pt label gap, so the pair reads as
/// one unit. `ZONE_GAP` spaces cards inside the field; the grammar
/// tokens are the only spacing values a page may use.
fn showcase_column(sections: Vec<(&'static str, Vec<Entry>)>) -> Flex {
    let mut col = Flex::column().gap(ZONE_SECTION);
    for (title, entries) in sections {
        let mut fb = FlowBox::new().gap(ZONE_GAP);
        for (name, widget) in entries {
            fb = fb.child(card(name, widget));
        }
        col = col.child(Flex::column().gap(4.0).child(group_label(title)).child(fb));
    }
    col
}

/// Quiet-tier section caption (spec B6/B1 role→tier): a 12 pt
/// caption label heading a card field — the in-page quiet idiom,
/// no raised chrome. Matches the zones' `group_label` convention.
fn group_label(text: &'static str) -> Text {
    Text::new(text).font_size(12.0)
}

/// Process Grid showcase — data browsers and the navigation controls
/// a process/site hierarchy uses.
pub fn grid_sections() -> Vec<(&'static str, Vec<Entry>)> {
    vec![
        ("DATA & DOCUMENTS", data::entries()),
        ("NAVIGATION & LAYOUT", nav::entries()),
    ]
}

/// Telemetry showcase — every chart and instrument the framework
/// ships, all monitoring content by nature.
pub fn telemetry_sections() -> Vec<(&'static str, Vec<Entry>)> {
    vec![
        ("CHARTS & DATAVIZ", charts::entries()),
        ("FEEDBACK & DISPLAY", feedback::entries()),
    ]
}

/// Editor showcase — field configuration widgets and the action
/// chrome (buttons, menus, dialogs) that operates on them.
pub fn editor_sections() -> Vec<(&'static str, Vec<Entry>)> {
    vec![
        ("FORMS & INPUT", forms::entries()),
        ("PICKERS & CANVAS", pickers::entries()),
        ("CHROME & ACTIONS", chrome::entries()),
    ]
}

/// Media showcase — comms, transport, and conferencing widgets.
pub fn media_sections() -> Vec<(&'static str, Vec<Entry>)> {
    vec![("MEDIA & SOCIAL", social::entries())]
}

/// Wraps a demo widget in a titled `GroupBox` — the card is the
/// showcase label, so no separate caption widget is needed. The
/// `Clamp` caps card width: `GroupBox` fills whatever width it is
/// offered, which would otherwise give every card a row to itself.
fn card(name: &'static str, widget: Box<dyn Widget>) -> Clamp {
    Clamp::new()
        .maximum(CARD_W_PT)
        .child(GroupBox::new(name).child(BoxedWidget(widget)))
}

/// Card width cap (logical points).
const CARD_W_PT: f32 = 360.0;

/// `GroupBox::child` takes `impl Widget` — a `Box<dyn Widget>` is not
/// itself a `Widget`, so it needs this forwarding shim. The shim also
/// caps the reported demo height: "fill"-style widgets (drawers,
/// sheets, watermarks) answer the offered ceiling verbatim, which
/// would otherwise render them as multi-thousand-pt towers.
struct BoxedWidget(Box<dyn Widget>);

/// Tallest a demo may claim inside its card (logical points).
const DEMO_H_PT: f32 = 340.0;

impl Widget for BoxedWidget {
    fn debug_name(&self) -> &'static str {
        self.0.debug_name()
    }
    fn measure(&mut self, cx: &mut LayoutContext, c: LayoutConstraints) -> Vec2 {
        let s = self.0.measure(cx, c);
        Vec2::new(s.x, s.y.min(cx.pt(DEMO_H_PT)))
    }
    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.0.layout(cx, bounds);
    }
    fn paint(&self, cx: &mut PaintContext) {
        self.0.paint(cx);
    }
    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        self.0.event(cx)
    }
    fn tick(&mut self, dt: Duration) -> bool {
        self.0.tick(dt)
    }
    fn accessibility(&self, node: &mut accesskit::Node) {
        self.0.accessibility(node);
    }
    fn child_count(&self) -> usize {
        self.0.child_count()
    }
    fn child(&self, index: usize) -> Option<&dyn Widget> {
        self.0.child(index)
    }
    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        self.0.child_mut(index)
    }
    fn child_bounds(&self, index: usize) -> Option<Rect> {
        self.0.child_bounds(index)
    }
}

/// Recursively ticks an internal subtree — containers do not forward
/// `tick` by default, so animated demos (clocks, countdowns,
/// indicators) rely on this walk to stay live.
fn tick_children(w: &mut dyn Widget, dt: Duration) -> bool {
    let mut dirty = false;
    for i in 0..w.child_count() {
        if let Some(child) = w.child_mut(i) {
            if tick_children(child, dt) {
                dirty = true;
            }
        }
    }
    if w.tick(dt) {
        dirty = true;
    }
    dirty
}

impl Widget for ShowcasePanel {
    fn debug_name(&self) -> &'static str {
        "Showcase Panel"
    }

    fn measure(&mut self, cx: &mut LayoutContext, c: LayoutConstraints) -> Vec2 {
        let inner = self.inner.measure(cx, c);
        // Ask for enough height to show the operational view and a
        // useful slice of the showcase.
        Vec2::new(inner.x, inner.y / OP_FRAC)
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(320.0, 240.0)).with_policy(UnderflowPolicy::Fallback)
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
        let msg = "SHOWCASE — enlarge to restore";
        // Sole-content placeholders stay ≥12pt — micro text under the
        // Caption floor fails legibility for the only thing shown.
        let size = 12.0 * s;
        let tw = f64::from(text.measure(msg, size));
        let x = (b.x0 + (b.width() - tw) * 0.5).max(b.x0 + 2.0);
        let y = b.y0 + (b.height() - f64::from(size)) * 0.5;
        text.push(cx.list, Point::new(x, y), msg, size, pal.text_muted, None);
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        let s = self.s();
        // Operational view on top, showcase below a 1px divider —
        // the inner panel owns its title bar inside its region.
        let inner_h = (bounds.height() * OP_FRAC).max(TITLE_H * s + 24.0);
        let inner_h = inner_h.min((bounds.height() - 48.0).max(TITLE_H * s + 24.0));
        self.inner_bounds = Rect::new(bounds.min_x(), bounds.min_y(), bounds.width(), inner_h);
        self.view_bounds = Rect::new(
            bounds.min_x(),
            bounds.min_y() + inner_h + 1.0,
            bounds.width(),
            (bounds.height() - inner_h - 1.0).max(0.0),
        );
        self.inner.layout(cx, self.inner_bounds);
        self.view.layout(cx, self.view_bounds);
    }

    fn tick(&mut self, dt: Duration) -> bool {
        self.inner.tick(dt) | tick_children(&mut self.view, dt)
    }

    fn paint(&self, cx: &mut PaintContext) {
        let pal = Palette::from_theme(cx.theme);
        let s = self.s();
        // Divider between the operational view and the showcase —
        // the inner panel and the scroll content paint themselves.
        let y = f64::from(self.view_bounds.min_y() - 0.5);
        cx.list.push_fill_rect(
            krect(
                f64::from(self.bounds.min_x()),
                y,
                f64::from(self.bounds.width()),
                1.0,
            ),
            pal.border,
        );
        panel_border(cx.list, self.bounds, &pal, s, false);
    }

    fn child_count(&self) -> usize {
        2
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        match index {
            0 => Some(&*self.inner as &dyn Widget),
            1 => Some(&self.view as &dyn Widget),
            _ => None,
        }
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        match index {
            0 => Some(&mut *self.inner as &mut dyn Widget),
            1 => Some(&mut self.view as &mut dyn Widget),
            _ => None,
        }
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        match index {
            0 => Some(self.inner_bounds),
            1 => Some(self.view_bounds),
            _ => None,
        }
    }

    fn clips_children(&self) -> bool {
        true
    }

    fn accessibility(&self, node: &mut accesskit::Node) {
        node.set_role(accesskit::Role::Group);
        node.set_label(format!(
            "{self_t} — operational view with contextual widget showcase",
            self_t = self.title
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite::core::HotNode;

    /// Every section group, in dock order — the showcase asserts
    /// coverage across all four panels, not one catalog.
    fn all_sections() -> Vec<(&'static str, Vec<Entry>)> {
        let mut v = grid_sections();
        v.extend(telemetry_sections());
        v.extend(editor_sections());
        v.extend(media_sections());
        v
    }

    /// Every widget module contributes a card — the showcase is the
    /// exhaustive reference, so count and label coverage are asserted
    /// rather than assumed.
    #[test]
    fn showcase_covers_all_widget_modules() {
        let total: usize = all_sections().iter().map(|(_, e)| e.len()).sum();
        assert_eq!(total, 274, "expected one card per widget module");
    }

    #[test]
    fn card_names_are_unique() {
        let mut names: Vec<&str> = all_sections()
            .iter()
            .flat_map(|(_, e)| e.iter().map(|(n, _)| *n))
            .collect();
        names.sort_unstable();
        names.dedup();
        let total: usize = all_sections().iter().map(|(_, e)| e.len()).sum();
        assert_eq!(names.len(), total, "duplicate card names");
    }

    /// Every mounted widget must report a finite measure under the
    /// real cell constraints — an infinite or NaN size poisons
    /// container layout and can turn a widget's own paint loop
    /// (e.g. checkerboard tiling) unbounded.
    #[test]
    fn every_card_measures_finite() {
        let mut bad = Vec::new();
        for (section, entries) in all_sections() {
            for (name, widget) in entries {
                // FlowBox measures the Clamp card, not the raw demo —
                // wrap it so the test sees the same numbers layout does.
                let mut widget = card(name, widget);
                let mut hot = HotNode::default();
                let mut cx = LayoutContext {
                    hot: &mut hot,
                    scale: 1.0,
                };
                // `FlowBox::layout` measures cells under
                // `max_size = (bounds.w.min(2048), 2048)` —
                // replicate its real constraints, or this test
                // misses what it sees. "Fill" widgets answer the
                // offer on both axes; that is valid input.
                let size = widget.measure(
                    &mut cx,
                    LayoutConstraints {
                        min_size: Vec2::ZERO,
                        max_size: Vec2::new(900.0, 2048.0),
                    },
                );
                if !size.x.is_finite()
                    || !size.y.is_finite()
                    || size.x > 10_000.0
                    || size.y > 10_000.0
                {
                    bad.push(format!("{section}/{name}: {size:?}"));
                }
            }
        }
        assert!(bad.is_empty(), "unbounded measures:\n{}", bad.join("\n"));
    }

    /// Times each card's layout + paint — catches widgets whose
    /// paint loops go unbounded at showcase bounds (e.g. a
    /// checkerboard tile loop fed an enormous rail).
    #[test]
    fn every_card_paints_promptly() {
        use martensite::theme::tokens::default_dark;
        let theme = default_dark();
        let mut slow = Vec::new();
        for (section, entries) in all_sections() {
            for (name, mut widget) in entries {
                let mut hot = HotNode::default();
                let mut lcx = LayoutContext {
                    hot: &mut hot,
                    scale: 1.0,
                };
                widget.layout(&mut lcx, Rect::new(0.0, 0.0, 300.0, 200.0));
                let mut list = martensite::render::PaintList::new();
                let mut pcx = PaintContext {
                    list: &mut list,
                    bounds: Rect::new(0.0, 0.0, 300.0, 200.0),
                    theme: &theme,
                    scale: 1.0,
                    text_painter: None,
                };
                let t = std::time::Instant::now();
                widget.paint(&mut pcx);
                let el = t.elapsed();
                if el > std::time::Duration::from_millis(250) {
                    slow.push(format!("{section}/{name}: {el:?}"));
                }
            }
        }
        assert!(slow.is_empty(), "slow paints:\n{}", slow.join("\n"));
    }

    /// Lays out a real showcase composition the way ScrollView does
    /// and asserts every recorded child rect is finite and inside a
    /// sane coordinate range — the diagnostic that caught the
    /// fill-widget `f32::MAX` cascade.
    #[test]
    fn showcase_card_bounds_are_sane() {
        let mut view = ScrollView::new(showcase_column(all_sections()));
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        view.layout(&mut cx, Rect::new(0.0, 0.0, 900.0, 400.0));
        fn walk(w: &dyn Widget, depth: usize, bad: &mut Vec<String>) {
            for i in 0..w.child_count() {
                if let Some(b) = w.child_bounds(i) {
                    let finite = b.origin.is_finite() && b.size.is_finite();
                    let sane = b.width() < 100_000.0
                        && b.height() < 100_000.0
                        && b.min_x().abs() < 1e7
                        && b.min_y().abs() < 1e7;
                    if !(finite && sane) {
                        bad.push(format!("depth {depth} child {i}: {b:?}"));
                    }
                }
                if let Some(c) = w.child(i) {
                    walk(c, depth + 1, bad);
                }
            }
        }
        let mut bad = Vec::new();
        walk(&view, 0, &mut bad);
        assert!(bad.is_empty(), "pathological bounds:\n{}", bad.join("\n"));
    }

    /// Laying out a ShowcasePanel runs `layout` on its inner panel
    /// plus every mounted showcase widget — the strongest headless
    /// exercise available.
    #[test]
    fn showcase_lays_out_all_cards() {
        let mut panel = ShowcasePanel::new(
            Box::new(martensite::widgets::container::Container::new()),
            "Test Panel",
            Signal::new(1.0),
            all_sections(),
        );
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        panel.layout(&mut cx, Rect::new(0.0, 0.0, 900.0, 600.0));
        assert!(panel.view_bounds.height() > 0.0);
        assert!(panel.inner_bounds.height() > 0.0);
        // A tick walk must reach every card without panicking —
        // animated demos (clocks, countdowns) depend on it.
        let _ = panel.tick(Duration::from_millis(16));
    }
}
