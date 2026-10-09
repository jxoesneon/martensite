//! `runner_smoke` — the smallest real consumer of [`martensite::runner`].
//!
//! Mounts a [`Flex`] column (title, status line, counter button, drop
//! hint) behind a tiny shell widget that observes `DropHover`/`Dropped`
//! events — proving the full runner pipeline (window → GPU → arena →
//! router → widgets → drag-and-drop payload delivery) with no
//! hand-rolled `ApplicationHandler`.

use martensite::core::{
    EventContext, EventResponse, HotNode, LayoutConstraints, LayoutContext, NodeFlags,
    PaintContext, Rect, Widget, WidgetArena, WidgetEvent, WidgetId,
};
use martensite::runner::{self, RunnerConfig};
use martensite::widgets::{Button, Flex, FlexDirection, Text};

/// Root shell: wraps a `Flex` and reports drop activity on the status
/// line so a file drop is observable on-screen.
struct Shell {
    flex: Flex,
    drops: usize,
}

impl Shell {
    fn new() -> Self {
        let flex = Flex::new(FlexDirection::Column)
            .child(Text::new("martensite::runner smoke"))
            .child(Text::new("drop a file anywhere"))
            .child(Button::new("count").enabled(true));
        Self { flex, drops: 0 }
    }
}

impl Widget for Shell {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> glam::Vec2 {
        self.flex.measure(cx, constraints)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.flex.layout(cx, bounds);
    }

    fn paint(&self, cx: &mut PaintContext) {
        self.flex.paint(cx);
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::Dropped { payload, .. } => {
                self.drops += 1;
                let what = match payload {
                    martensite::core::DropPayload::Files(p) => {
                        format!("{} file(s): {}", p.len(), p[0].display())
                    }
                    martensite::core::DropPayload::Uris(u) => {
                        format!("{} uri(s)", u.len())
                    }
                    martensite::core::DropPayload::Text(t) => format!("text: {t}"),
                    martensite::core::DropPayload::Bytes(b) => format!("{} bytes", b.len()),
                    _ => "unknown payload".to_string(),
                };
                eprintln!("runner_smoke: drop #{n} — {what}", n = self.drops);
                EventResponse::Handled
            }
            WidgetEvent::DropHover { .. } => EventResponse::Handled,
            WidgetEvent::DropHoverLeave => EventResponse::Handled,
            _ => self.flex.event(cx),
        }
    }

    fn child_count(&self) -> usize {
        Widget::child_count(&self.flex)
    }

    fn child(&self, i: usize) -> Option<&dyn Widget> {
        Widget::child(&self.flex, i)
    }

    fn child_mut(&mut self, i: usize) -> Option<&mut dyn Widget> {
        Widget::child_mut(&mut self.flex, i)
    }

    fn child_bounds(&self, i: usize) -> Option<Rect> {
        Widget::child_bounds(&self.flex, i)
    }
}

fn build_root(arena: &mut WidgetArena, _scale: f32) -> WidgetId {
    let mut hot = HotNode::default();
    hot.flags |= NodeFlags::VISIBLE | NodeFlags::HIT_TEST_ENABLED;
    arena.insert_with_widget(hot, Box::new(Shell::new()))
}

fn main() {
    let cfg = RunnerConfig::new("runner_smoke").with_size(640.0, 360.0);
    if let Err(err) = runner::run(cfg, build_root) {
        eprintln!("runner_smoke: {err}");
        std::process::exit(1);
    }
}
