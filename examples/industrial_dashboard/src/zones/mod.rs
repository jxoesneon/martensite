//! Functional zones — the contextual-dashboard composition per the
//! Design Council verdict (docket `20260921`): every panel is an
//! operational surface where each mounted widget is bound to the
//! [`PlantModel`] through [`Bound`], organised into domain-named
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

/// A panel = operational view on top + domain-named zone pages below.
/// The operational widget is untouched (keeps its own chrome, focus,
/// events); the `Tabs` provides one-visible-surface semantics — hidden
/// pages are suspended by the arena's bounds-gated paint/tick walks.
///
/// Cross-zone navigation: an artifact action writes
/// `model.page_request[zone_index] = Some(page)`; `tick` drains the
/// slot (take semantics — cleared on consume) and activates the tab.
/// `layout` publishes the content width into `model.zone_width`,
/// which `Page` reads for its rail-collapse breakpoints.
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
        for (label, page) in pages {
            // ZONE_PAD insets every page's content from the tab
            // chrome — the published grammar token, applied here so
            // page builders stay flush-left internally. No ScrollView
            // wrapper: pages are bounded-fill layouts; dense surfaces
            // scroll internally.
            let padded = martensite::widgets::container::Container::new()
                .padding_uniform(crate::zone::ZONE_PAD)
                .child(page);
            zones = zones.tab(label, padded);
        }
        Self {
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
        }
    }

    fn s(&self) -> f32 {
        self.scale.get()
    }
}

impl Widget for ZonePanel {
    fn debug_name(&self) -> &'static str {
        "Zone Panel"
    }

    fn measure(&mut self, cx: &mut LayoutContext, c: LayoutConstraints) -> Vec2 {
        let inner = self.inner.measure(cx, c);
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
        let msg = "PANEL — enlarge to restore";
        let size = 12.0 * s;
        let tw = f64::from(text.measure(msg, size));
        let x = (b.x0 + (b.width() - tw) * 0.5).max(b.x0 + 2.0);
        let y = b.y0 + (b.height() - f64::from(size)) * 0.5;
        text.push(cx.list, Point::new(x, y), msg, size, pal.text_muted, None);
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        let s = self.s();
        // `inner_h` never exceeds the panel — a degenerate-height
        // panel gives the zones region 0pt (the underflow fallback
        // paints "enlarge to restore" anyway).
        // The operational view keeps at least its own declared
        // `min_render` height (in pt, scaled) so a shorter panel
        // never pushes it into its underflow placeholder just to
        // give the zones a fixed fraction.
        let inner_min = self.inner.min_render().size.y * s;
        let inner_h = (bounds.height() * OP_FRAC)
            .max(TITLE_H * s + 24.0)
            .max(inner_min)
            .min(bounds.height())
            .max(0.0);
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
        if (self.model.zone_width.get() - w_pt).abs() > 0.5 {
            self.model.zone_width.set(w_pt);
        }
        self.inner.layout(cx, self.inner_bounds);
        self.zones.layout(cx, self.zones_bounds);
    }

    // The drain lives in `tick` — `inner`/`zones` are internal
    // children and the arena's bounds-gated `tick_recursive` already
    // ticks them through `child_mut`, so this override only handles
    // the nav request (ticking children here too would double their
    // rate — the TelemetryPanel's `elapsed += dt` would run at 2×).
    fn tick(&mut self, _dt: Duration) -> bool {
        let mut req = self.model.page_request.get();
        match req.get_mut(self.zone_index).and_then(Option::take) {
            Some(page) => {
                self.model.page_request.set(req);
                self.zones.activate(page as usize);
                true
            }
            None => false,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let pal = Palette::from_theme(cx.theme);
        let s = self.s();
        // Divider between the operational view and the zone tabs —
        // skipped when the zones region collapsed to 0pt (a degenerate
        // panel height would otherwise paint it below the panel).
        if self.zones_bounds.height() > 0.0 {
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

    fn model() -> PlantModel {
        PlantModel::seeded(
            Signal::new(0.4),
            Signal::new(0.6),
            Signal::new(false),
            Signal::new(true),
            Signal::new(String::new()),
        )
    }

    /// Toolbar chrome launchers write `page_request[zone]`; the owning
    /// ZonePanel's `tick` must drain the slot and activate the tab —
    /// exactly once.
    #[test]
    fn page_request_activates_the_zone_tab() {
        let m = model();
        let page = |m: &PlantModel| Page::new(Variant::Theater, fill(DummyWidget), &m.zone_width);
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
}
