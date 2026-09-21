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
use martensite::widgets::flex::Flex;
use martensite::widgets::scrollview::ScrollView;
use martensite::widgets::tabs::Tabs;
use parking_lot::Mutex;

use crate::model::Palette;
use crate::panels::{krect, panel_border, TITLE_H};
use crate::text::TextPainter;

pub mod editor;
pub mod grid;
pub mod media;
pub mod telemetry;

/// Fraction of the panel height the operational view keeps — zone
/// tabs fill the remainder.
const OP_FRAC: f32 = 0.45;

/// A panel = operational view on top + domain-named zone pages below.
/// The operational widget is untouched (keeps its own chrome, focus,
/// events); the `Tabs` provides one-visible-surface semantics — hidden
/// pages are suspended by the arena's bounds-gated paint/tick walks.
pub struct ZonePanel {
    inner: Box<dyn Widget>,
    zones: Tabs,
    title: &'static str,
    bounds: Rect,
    inner_bounds: Rect,
    zones_bounds: Rect,
    scale: Signal<f32>,
    text: Mutex<TextPainter>,
}

impl ZonePanel {
    /// `pages` are `(domain label, page column)` pairs — the zone's
    /// tab names are domain names, never widget names.
    pub fn new(
        inner: Box<dyn Widget>,
        title: &'static str,
        scale: Signal<f32>,
        pages: Vec<(&'static str, Flex)>,
    ) -> Self {
        let mut zones = Tabs::new();
        for (label, page) in pages {
            // ZONE_PAD insets every page's content from the tab
            // chrome — the published grammar token, applied here so
            // page builders stay flush-left internally.
            let padded = martensite::widgets::container::Container::new()
                .padding_uniform(crate::zone::ZONE_PAD)
                .child(page);
            zones = zones.tab(label, ScrollView::new(padded));
        }
        Self {
            inner,
            zones,
            title,
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
        self.inner.layout(cx, self.inner_bounds);
        self.zones.layout(cx, self.zones_bounds);
    }

    // No `tick` override — `inner`/`zones` are internal children and
    // the arena's bounds-gated `tick_recursive` already ticks them
    // through `child_mut`. Ticking them here too would double their
    // rate (the TelemetryPanel's `elapsed += dt` would run at 2×).

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
