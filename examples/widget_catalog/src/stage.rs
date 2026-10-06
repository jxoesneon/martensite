//! The stage: a host widget that frames one staged widget inside a
//! [`Viewport`] (so zoom is a real transform, not a resized layout
//! box), scopes the app's direction/locale/theme choices to the staged
//! subtree only, and supports resizable frame presets so under- and
//! over-flow are visible.

use std::collections::VecDeque;

use glam::Vec2;
use martensite::core::intl::install_ambient_intl;
use martensite::core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect, Widget,
    WidgetEvent,
};
use martensite::core::{LayoutDirection, Locale};
use martensite::theme::Theme;
use martensite::widgets::viewport::Viewport;

/// Backdrop grid spacing in logical points.
const GRID_STEP: f32 = 24.0;
/// Fill-mode padding around the stage content box.
const FRAME_PAD: f32 = 24.0;

/// Frame sizing presets for the stage — `Fill` follows the stage's own
/// bounds; the rest fix the content box the staged widget lives in.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum FramePreset {
    /// Fill the stage area (minus padding).
    #[default]
    Fill,
    /// 360×240 — phone-ish.
    Small,
    /// 640×360 — common content box.
    Medium,
    /// 960×540 — near-window.
    Large,
    /// 200×200 — deliberately cramped, exercises underflow policies.
    Cramped,
}

impl FramePreset {
    /// All presets in toolbar order.
    pub const ALL: [FramePreset; 5] = [
        FramePreset::Fill,
        FramePreset::Small,
        FramePreset::Medium,
        FramePreset::Large,
        FramePreset::Cramped,
    ];

    /// Toolbar label.
    pub fn label(self) -> &'static str {
        match self {
            FramePreset::Fill => "Fill",
            FramePreset::Small => "360×240",
            FramePreset::Medium => "640×360",
            FramePreset::Large => "960×540",
            FramePreset::Cramped => "200×200",
        }
    }

    /// Content-box size in logical points; `None` = fill.
    pub fn size(self) -> Option<Vec2> {
        match self {
            FramePreset::Fill => None,
            FramePreset::Small => Some(Vec2::new(360.0, 240.0)),
            FramePreset::Medium => Some(Vec2::new(640.0, 360.0)),
            FramePreset::Large => Some(Vec2::new(960.0, 540.0)),
            FramePreset::Cramped => Some(Vec2::new(200.0, 200.0)),
        }
    }
}

/// Hosts the staged widget inside a [`Viewport`]. The frame rect the
/// viewport receives is centered in the stage; `Fill` gives it the
/// stage minus `FRAME_PAD` on every side.
///
/// Direction/locale/theme overrides apply to the staged subtree only:
/// `layout`/`event` install the ambient values around the child calls,
/// and the framework's paint walk picks up
/// [`Widget::intl_override`]/[`Widget::theme_override`] for paint.
pub struct StageHost {
    viewport: Viewport,
    viewport_bounds: Rect,
    frame: FramePreset,
    direction: Option<LayoutDirection>,
    locale: Option<Locale>,
    theme: Option<Theme>,
    /// Formatted lines for events delivered to the staged subtree —
    /// the catalog's event panel drains this so the log reports what
    /// happens to the *displayed* widget, not app-wide traffic.
    event_log: VecDeque<String>,
}

impl StageHost {
    /// New stage hosting `child` in a default viewport.
    pub fn new(child: Box<dyn Widget>) -> Self {
        Self {
            // `backdrop(false)` — the stage paints its own surface +
            // grid under the whole area; the viewport's built-in
            // backdrop would double the grid and seam the fill.
            viewport: Viewport::new().child_boxed(child).backdrop(false),
            viewport_bounds: Rect::default(),
            frame: FramePreset::Fill,
            direction: None,
            locale: None,
            theme: None,
            event_log: VecDeque::new(),
        }
    }

    /// Replaces the staged widget, preserving zoom/pan.
    pub fn set_child(&mut self, child: Box<dyn Widget>) {
        let zoom = self.viewport.zoom_value();
        let pan = self.viewport.pan_offset();
        self.viewport = Viewport::new()
            .child_boxed(child)
            .backdrop(false)
            .zoom(zoom)
            .pan(pan);
        // Events about the previous widget are stale by definition.
        self.event_log.clear();
    }

    /// Drains the staged subtree's event lines for the log panel.
    pub fn take_event_log(&mut self) -> Vec<String> {
        self.event_log.drain(..).collect()
    }

    /// The staged widget — inside the viewport. `Widget::child` is
    /// qualified because `Viewport` also has an inherent `child`
    /// builder method.
    pub fn staged(&self) -> Option<&dyn Widget> {
        Widget::child(&self.viewport, 0)
    }

    /// The staged widget, mutably.
    pub fn staged_mut(&mut self) -> Option<&mut dyn Widget> {
        Widget::child_mut(&mut self.viewport, 0)
    }

    /// Frame preset setter.
    pub fn set_frame(&mut self, frame: FramePreset) {
        self.frame = frame;
    }

    /// Direction override — `None` follows the arena ambient.
    pub fn set_direction(&mut self, direction: Option<LayoutDirection>) {
        self.direction = direction;
    }

    /// Locale override — `None` follows the arena ambient.
    pub fn set_locale(&mut self, locale: Option<Locale>) {
        self.locale = locale;
    }

    /// Theme override — `None` follows the arena theme.
    pub fn set_theme(&mut self, theme: Option<Theme>) {
        self.theme = theme;
    }

    /// Absolute zoom, centered on the stage.
    pub fn set_zoom(&mut self, zoom: f32) {
        let c = self.viewport_bounds.origin + self.viewport_bounds.size * 0.5;
        self.viewport.zoom_at(c, zoom.clamp(0.25, 4.0));
    }

    /// Current zoom.
    pub fn zoom(&self) -> f32 {
        self.viewport.zoom_value()
    }

    /// Resets zoom and pan.
    pub fn reset_view(&mut self) {
        self.viewport.reset_view();
    }

    /// Installs the override pair as ambient — `None` when unset.
    fn ambient_guard(&self) -> Option<martensite::core::intl::AmbientIntlGuard> {
        if self.direction.is_none() && self.locale.is_none() {
            return None;
        }
        Some(install_ambient_intl(
            self.direction.unwrap_or_default(),
            self.locale.clone().unwrap_or_default(),
        ))
    }

    /// Appends a human-readable line for an event that actually
    /// reached the staged widget. Pointer traffic is gated to the
    /// widget's content rect (a press on bare canvas is a pan, not a
    /// widget event); keys and IME only count while the staged subtree
    /// holds focus. `PointerMoved` is pure chatter and never logged.
    fn record(&mut self, event: &WidgetEvent) {
        // The viewport's content rect — the staged widget's actual
        // on-screen extent inside the pannable canvas.
        let content = Widget::child_bounds(&self.viewport, 0);
        let on_widget = |p: Vec2| content.is_some_and(|r| r.contains(p));
        let focused = self.viewport.focused() || self.viewport.has_focused_descendant();
        let line = match event {
            WidgetEvent::PointerMoved { .. } | WidgetEvent::ImePreedit { .. } => return,
            WidgetEvent::PointerPressed {
                position,
                button,
                count,
            } if on_widget(*position) => {
                format!(
                    "press {button:?}{}",
                    if *count > 1 {
                        format!(" ×{count}")
                    } else {
                        String::new()
                    }
                )
            }
            WidgetEvent::PointerReleased { position, button } if on_widget(*position) => {
                format!("release {button:?}")
            }
            WidgetEvent::Scroll { position, delta } if on_widget(*position) => {
                format!("scroll Δ{:.0},{:.0}", delta.x, delta.y)
            }
            WidgetEvent::KeyPressed { key, repeat } if focused && !repeat => {
                format!("key {key}")
            }
            WidgetEvent::ImeCommitted { text } if focused => format!("commit {text:?}"),
            WidgetEvent::FocusGained => "focus".to_string(),
            WidgetEvent::FocusLost => "blur".to_string(),
            _ => return,
        };
        // Coalesce a run of identical lines into `… ×n` — a held key
        // or a drag shouldn't bury the log.
        if let Some(back) = self.event_log.back_mut() {
            if let Some((head, n)) = back.rsplit_once(" ×") {
                if head == line {
                    if let Ok(n) = n.parse::<u32>() {
                        *back = format!("{line} ×{}", n + 1);
                        return;
                    }
                }
            } else if *back == line {
                *back = format!("{line} ×2");
                return;
            }
        }
        if self.event_log.len() >= 256 {
            self.event_log.pop_front();
        }
        self.event_log.push_back(line);
    }

    /// The rect the viewport is laid out into.
    fn content_box(&self, bounds: Rect) -> Rect {
        let padded = Rect::new(
            bounds.min_x() + FRAME_PAD,
            bounds.min_y() + FRAME_PAD,
            (bounds.width() - FRAME_PAD * 2.0).max(0.0),
            (bounds.height() - FRAME_PAD * 2.0).max(0.0),
        );
        let Some(size) = self.frame.size() else {
            return padded;
        };
        let w = size.x.min(padded.width());
        let h = size.y.min(padded.height());
        Rect::new(
            padded.min_x() + (padded.width() - w) * 0.5,
            padded.min_y() + (padded.height() - h) * 0.5,
            w,
            h,
        )
    }
}

impl Widget for StageHost {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let _g = self.ambient_guard();
        self.viewport.measure(cx, constraints)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        let _g = self.ambient_guard();
        self.viewport_bounds = self.content_box(bounds);
        cx.layout_child(&mut self.viewport, self.viewport_bounds);
        // Center content when it fits inside the viewport on an axis —
        // keeps a lone widget visually centered while still allowing
        // pan of oversized content.
        let content = self.viewport.content_size() * self.viewport.zoom_value();
        let pan = self.viewport.pan_offset();
        let center = (self.viewport_bounds.size - content) * 0.5;
        let new_pan = Vec2::new(
            if content.x <= self.viewport_bounds.width() {
                center.x
            } else {
                pan.x
            },
            if content.y <= self.viewport_bounds.height() {
                center.y
            } else {
                pan.y
            },
        );
        if new_pan != pan {
            self.viewport.set_pan(new_pan);
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        // Stage backdrop: surface fill + subtle dot grid so frame
        // extents and empty space read at a glance.
        let surface = cx.color(martensite::theme::TokenKey::SurfaceColor, [24, 26, 32, 255]);
        let dot = cx.color(
            martensite::theme::TokenKey::TextMutedColor,
            [90, 95, 110, 255],
        );
        let b = cx.bounds;
        cx.list.push_fill_rect(
            kurbo::Rect::new(
                f64::from(b.min_x()),
                f64::from(b.min_y()),
                f64::from(b.max_x()),
                f64::from(b.max_y()),
            ),
            surface,
        );
        let step = f64::from(GRID_STEP * cx.scale);
        let dot_px = f64::from(cx.scale * 1.5);
        // Dots anchor to the global grid phase (`… mod step`), not the
        // stage origin — a stage that moves a pixel between pages must
        // not shift the backdrop pattern under the user's eyes.
        let mut y = f64::from(b.min_y()) - f64::from(b.min_y()).rem_euclid(step);
        while y <= f64::from(b.max_y()) {
            let mut x = f64::from(b.min_x()) - f64::from(b.min_x()).rem_euclid(step);
            while x <= f64::from(b.max_x()) {
                cx.list.push_fill_rect(
                    kurbo::Rect::new(x, y, x + dot_px, y + dot_px),
                    dot.map(|v| (f32::from(v) * 0.14) as u8),
                );
                x += step;
            }
            y += step;
        }
        // Frame outline for fixed presets.
        if self.frame.size().is_some() {
            let vb = self.viewport_bounds;
            cx.list.push_stroke_rect(
                kurbo::Rect::new(
                    f64::from(vb.min_x()),
                    f64::from(vb.min_y()),
                    f64::from(vb.max_x()),
                    f64::from(vb.max_y()),
                ),
                1.0,
                dot,
            );
        }
    }

    fn intl_override(&self) -> Option<(LayoutDirection, Locale)> {
        if self.direction.is_none() && self.locale.is_none() {
            return None;
        }
        Some((
            self.direction.unwrap_or_default(),
            self.locale.clone().unwrap_or_default(),
        ))
    }

    fn theme_override(&self) -> Option<&Theme> {
        self.theme.as_ref()
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        let _g = self.ambient_guard();
        self.record(cx.event);
        self.forward_event_to_children(cx)
    }

    fn child_count(&self) -> usize {
        1
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        (index == 0).then_some(&self.viewport as &dyn Widget)
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        (index == 0).then_some(&mut self.viewport as &mut dyn Widget)
    }

    fn child_bounds(&self, _index: usize) -> Option<Rect> {
        Some(self.viewport_bounds)
    }

    fn clips_children(&self) -> bool {
        true
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite::core::widget::DummyWidget;
    use martensite::core::HotNode;

    fn make_cx(hot: &mut HotNode) -> LayoutContext<'_> {
        LayoutContext { hot, scale: 1.0 }
    }

    #[test]
    fn fill_frame_gives_padded_viewport() {
        let mut host = StageHost::new(Box::new(DummyWidget));
        let mut hot = HotNode::default();
        let mut cx = make_cx(&mut hot);
        host.layout(&mut cx, Rect::new(0.0, 0.0, 400.0, 300.0));
        let b = host.viewport_bounds;
        assert_eq!(b.min_x(), FRAME_PAD);
        assert_eq!(b.width(), 400.0 - FRAME_PAD * 2.0);
        assert_eq!(b.height(), 300.0 - FRAME_PAD * 2.0);
    }

    #[test]
    fn fixed_frame_centers_viewport() {
        let mut host = StageHost::new(Box::new(DummyWidget));
        host.set_frame(FramePreset::Small);
        let mut hot = HotNode::default();
        let mut cx = make_cx(&mut hot);
        host.layout(&mut cx, Rect::new(0.0, 0.0, 800.0, 600.0));
        let b = host.viewport_bounds;
        assert_eq!(b.width(), 360.0);
        assert_eq!(b.min_x(), (800.0 - 24.0 * 2.0 - 360.0) * 0.5 + 24.0);
    }

    #[test]
    fn overrides_report() {
        let mut host = StageHost::new(Box::new(DummyWidget));
        assert!(host.intl_override().is_none());
        assert!(host.theme_override().is_none());
        host.set_direction(Some(LayoutDirection::Rtl));
        host.set_theme(Some(martensite::theme::tokens::default_dark()));
        assert_eq!(
            host.intl_override().map(|(d, _)| d),
            Some(LayoutDirection::Rtl)
        );
        assert!(host.theme_override().is_some());
    }

    #[test]
    fn set_child_preserves_zoom() {
        let mut host = StageHost::new(Box::new(DummyWidget));
        host.set_zoom(2.0);
        host.set_child(Box::new(DummyWidget));
        assert_eq!(host.zoom(), 2.0);
        assert!(host.staged().is_some());
    }
}
