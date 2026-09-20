//! Widget gallery — one live card per facade widget module, grouped
//! into categories. This is the comprehensive dogfood: every
//! `martensite::widgets` module is instantiated and mounted through
//! the internal-children protocol (`ScrollView` → `Flex` → `FlowBox`
//! → `GroupBox` → widget), so event routing, paint traversal, a11y
//! virtual nodes, and `tick` delivery are all exercised the way a
//! third-party composition would hit them.
//!
//! Each `mod` below contributes `entries() -> Vec<Entry>` — a list of
//! `(display name, live widget)` pairs built from the widget's own
//! documented construction (its doctest). `card()` wraps each in a
//! titled `GroupBox` so the showcase is self-labeling.

use std::time::Duration;

use glam::Vec2;
use martensite::core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget,
};
use martensite::reactive::Signal;
use martensite::render::Point;
use martensite::widgets::flex::Flex;
use martensite::widgets::flow_box::FlowBox;
use martensite::widgets::group_box::GroupBox;
use martensite::widgets::scrollview::ScrollView;
use martensite::widgets::text::Text;
use parking_lot::Mutex;

use crate::model::Palette;
use crate::panels::{krect, panel_border, panel_chrome, TITLE_H};
use crate::text::TextPainter;

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

/// The gallery panel — a titled `ScrollView` of categorized cards.
pub struct GalleryPanel {
    scale: Signal<f32>,
    view: ScrollView,
    bounds: Rect,
    view_bounds: Rect,
    text: Mutex<TextPainter>,
}

impl GalleryPanel {
    /// Builds the gallery: every category's cards under a section
    /// header, in one vertically scrolling column.
    pub fn new(scale: Signal<f32>) -> Self {
        let mut col = Flex::column().gap(10.0);
        for (title, entries) in sections() {
            col = col.child(Text::new(title).font_size(13.0));
            let mut fb = FlowBox::new().gap(8.0);
            for (name, widget) in entries {
                fb = fb.child(card(name, widget));
            }
            col = col.child(fb);
        }
        Self {
            scale,
            view: ScrollView::new(col),
            bounds: Rect::default(),
            view_bounds: Rect::default(),
            text: Mutex::new(TextPainter::new()),
        }
    }

    fn s(&self) -> f32 {
        self.scale.get()
    }
}

/// Wraps a demo widget in a titled `GroupBox` — the card is the
/// showcase label, so no separate caption widget is needed.
fn card(name: &'static str, widget: Box<dyn Widget>) -> GroupBox {
    GroupBox::new(name).child(BoxedWidget(widget))
}

/// `GroupBox::child` takes `impl Widget` — a `Box<dyn Widget>` is not
/// itself a `Widget`, so it needs this forwarding shim.
struct BoxedWidget(Box<dyn Widget>);

impl Widget for BoxedWidget {
    fn measure(&mut self, cx: &mut LayoutContext, c: LayoutConstraints) -> Vec2 {
        self.0.measure(cx, c)
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

/// The category sections, in display order. Names are the visible
/// section headers.
fn sections() -> Vec<(&'static str, Vec<Entry>)> {
    vec![
        ("FORMS & INPUT", forms::entries()),
        ("PICKERS & CANVAS", pickers::entries()),
        ("CHROME & ACTIONS", chrome::entries()),
        ("NAVIGATION & LAYOUT", nav::entries()),
        ("DATA & DOCUMENTS", data::entries()),
        ("CHARTS & DATAVIZ", charts::entries()),
        ("FEEDBACK & DISPLAY", feedback::entries()),
        ("MEDIA & SOCIAL", social::entries()),
    ]
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

impl Widget for GalleryPanel {
    fn debug_name(&self) -> &'static str {
        "Widget Gallery"
    }

    fn measure(&mut self, _cx: &mut LayoutContext, _c: LayoutConstraints) -> Vec2 {
        Vec2::new(560.0, 380.0)
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(280.0, 160.0)).with_policy(UnderflowPolicy::Fallback)
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
        let msg = "WIDGET GALLERY — enlarge to restore";
        let size = 11.0 * s;
        let tw = f64::from(text.measure(msg, size));
        let x = (b.x0 + (b.width() - tw) * 0.5).max(b.x0 + 2.0);
        let y = b.y0 + (b.height() - f64::from(size)) * 0.5;
        text.push(cx.list, Point::new(x, y), msg, size, pal.text_muted, None);
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        // The ScrollView owns everything below the title bar.
        let s = self.s();
        let top = bounds.min_y() + TITLE_H * s + 1.0;
        self.view_bounds = Rect::new(
            bounds.min_x(),
            top,
            bounds.width(),
            (bounds.max_y() - top).max(0.0),
        );
        self.view.layout(cx, self.view_bounds);
    }

    fn tick(&mut self, dt: Duration) -> bool {
        tick_children(&mut self.view, dt)
    }

    fn paint(&self, cx: &mut PaintContext) {
        let pal = Palette::from_theme(cx.theme);
        let s = self.s();
        let mut text = self.text.lock();
        let inner = panel_chrome(
            &mut text,
            cx.list,
            self.bounds,
            "WIDGET GALLERY",
            "",
            &pal,
            s,
        );
        // Card bounds are view-local already — nothing extra to emit
        // here besides the scroll content, which the paint walk
        // reaches through `child_bounds`.
        let _ = inner;
        panel_border(cx.list, self.bounds, &pal, s, false);
    }

    fn child_count(&self) -> usize {
        1
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        (index == 0).then_some(&self.view as &dyn Widget)
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        (index == 0).then_some(&mut self.view as &mut dyn Widget)
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        (index == 0).then_some(self.view_bounds)
    }

    fn clips_children(&self) -> bool {
        true
    }

    fn accessibility(&self, node: &mut accesskit::Node) {
        node.set_role(accesskit::Role::Group);
        node.set_label("Widget gallery — live showcase of all facade widgets");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite::core::HotNode;

    /// Every widget module contributes a card — the gallery is the
    /// exhaustive showcase, so count and label coverage are asserted
    /// rather than assumed.
    #[test]
    fn gallery_covers_all_widget_modules() {
        let total: usize = sections().iter().map(|(_, e)| e.len()).sum();
        assert_eq!(total, 274, "expected one card per widget module");
    }

    #[test]
    fn card_names_are_unique() {
        let mut names: Vec<&str> = sections()
            .iter()
            .flat_map(|(_, e)| e.iter().map(|(n, _)| *n))
            .collect();
        names.sort_unstable();
        names.dedup();
        let total: usize = sections().iter().map(|(_, e)| e.len()).sum();
        assert_eq!(names.len(), total, "duplicate card names");
    }

    /// Laying out the panel runs `layout` on every one of the 274
    /// mounted widgets through ScrollView → Flex → FlowBox →
    /// GroupBox — the strongest headless exercise available.
    #[test]
    fn gallery_lays_out_all_cards() {
        let mut panel = GalleryPanel::new(Signal::new(1.0));
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        panel.layout(&mut cx, Rect::new(0.0, 0.0, 900.0, 400.0));
        assert!(panel.view_bounds.height() > 0.0);
        // A tick walk must reach every card without panicking —
        // animated demos (clocks, countdowns) depend on it.
        let _ = panel.tick(Duration::from_millis(16));
    }
}
