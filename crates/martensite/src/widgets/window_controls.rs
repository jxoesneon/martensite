//! `WindowControls` — the caption button cluster (Windows
//! minimize/maximize/close, or macOS traffic lights).
//!
//! Stateless chrome: each button parks a [`WindowAction`] in
//! [`WindowControls::take_action`] — the shell owns the actual
//! window ops. Two styles: `Windows` (wide flat buttons, close
//! highlights red) and `Mac` (three colored dots). Maximize shows
//! a restore glyph when [`WindowControls::set_maximized`] is fed.
//!
//! Slot it into a [`HeaderBar`](crate::widgets::HeaderBar) trailing
//! zone for a full custom title bar.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::window_controls::WindowControls;
//!
//! let c = WindowControls::new();
//! assert_eq!(c.button_count(), 3);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

const BTN_W_PT: f32 = 46.0;
const BTN_H_PT: f32 = 30.0;
const MAC_DOT_PT: f32 = 12.0;
const MAC_GAP_PT: f32 = 8.0;

const FACE: [u8; 4] = [30, 31, 35, 255];
const FACE_HOVER: [u8; 4] = [80, 82, 92, 255];
const CLOSE_HOVER: [u8; 4] = [196, 43, 43, 255];
const GLYPH: [u8; 4] = [220, 222, 228, 255];
const MAC_CLOSE: [u8; 4] = [255, 95, 86, 255];
const MAC_MIN: [u8; 4] = [255, 189, 46, 255];
const MAC_MAX: [u8; 4] = [39, 201, 63, 255];

/// Which caption button was pressed.
///
/// ```
/// use martensite::widgets::window_controls::WindowAction;
///
/// assert_eq!(WindowAction::Close, WindowAction::Close);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowAction {
    /// Minimize to taskbar.
    Minimize,
    /// Maximize, or restore when already maximized.
    MaximizeRestore,
    /// Close the window.
    Close,
}

/// Caption button style.
///
/// ```
/// use martensite::widgets::window_controls::CaptionStyle;
///
/// assert_ne!(CaptionStyle::Windows, CaptionStyle::Mac);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CaptionStyle {
    /// Wide flat buttons (Windows / Linux CSD).
    #[default]
    Windows,
    /// Three colored dots (macOS traffic lights).
    Mac,
}

/// A caption button cluster — see the module docs.
///
/// ```
/// use martensite::widgets::window_controls::WindowControls;
///
/// assert_eq!(WindowControls::new().take_action(), None);
/// ```
pub struct WindowControls {
    /// Accessibility label.
    pub label: String,
    style: CaptionStyle,
    maximized: bool,
    /// Show the minimize button.
    pub minimizable: bool,
    /// Show the maximize button.
    pub maximizable: bool,
    /// Show the close button.
    pub closable: bool,
    rects: Vec<(WindowAction, Rect)>,
    held: Option<WindowAction>,
    hovered: Option<WindowAction>,
    action: Option<WindowAction>,
    bounds: Rect,
    scale: f32,
}

impl std::fmt::Debug for WindowControls {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WindowControls")
            .field("style", &self.style)
            .field("maximized", &self.maximized)
            .finish()
    }
}

impl Default for WindowControls {
    fn default() -> Self {
        Self::new()
    }
}

impl WindowControls {
    /// Windows-style minimize/maximize/close.
    ///
    /// ```
    /// use martensite::widgets::window_controls::WindowControls;
    ///
    /// assert_eq!(WindowControls::new().button_count(), 3);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Window controls".to_string(),
            style: CaptionStyle::Windows,
            maximized: false,
            minimizable: true,
            maximizable: true,
            closable: true,
            rects: Vec::new(),
            held: None,
            hovered: None,
            action: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// macOS traffic-light style.
    ///
    /// ```
    /// use martensite::widgets::window_controls::{CaptionStyle, WindowControls};
    ///
    /// let c = WindowControls::mac();
    /// assert_eq!(c.caption_style(), CaptionStyle::Mac);
    /// ```
    pub fn mac() -> Self {
        Self::new().style(CaptionStyle::Mac)
    }

    /// Style builder.
    ///
    /// ```
    /// use martensite::widgets::window_controls::{CaptionStyle, WindowControls};
    ///
    /// assert_eq!(WindowControls::new().style(CaptionStyle::Mac).caption_style(), CaptionStyle::Mac);
    /// ```
    pub fn style(mut self, style: CaptionStyle) -> Self {
        self.style = style;
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::window_controls::WindowControls;
    ///
    /// assert_eq!(WindowControls::new().label("Editor").label, "Editor");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Initial maximized state (drives the restore glyph).
    ///
    /// ```
    /// use martensite::widgets::window_controls::WindowControls;
    ///
    /// assert!(WindowControls::new().maximized(true).is_maximized());
    /// ```
    pub fn maximized(mut self, maximized: bool) -> Self {
        self.maximized = maximized;
        self
    }

    /// The caption style.
    ///
    /// ```
    /// use martensite::widgets::window_controls::{CaptionStyle, WindowControls};
    ///
    /// assert_eq!(WindowControls::new().caption_style(), CaptionStyle::Windows);
    /// ```
    pub fn caption_style(&self) -> CaptionStyle {
        self.style
    }

    /// Whether the window is maximized (restore glyph shown).
    ///
    /// ```
    /// use martensite::widgets::window_controls::WindowControls;
    ///
    /// assert!(!WindowControls::new().is_maximized());
    /// ```
    pub fn is_maximized(&self) -> bool {
        self.maximized
    }

    /// Feeds the window's maximized state for the glyph.
    ///
    /// ```
    /// use martensite::widgets::window_controls::WindowControls;
    ///
    /// let mut c = WindowControls::new();
    /// c.set_maximized(true);
    /// assert!(c.is_maximized());
    /// ```
    pub fn set_maximized(&mut self, maximized: bool) {
        self.maximized = maximized;
    }

    /// Number of visible buttons.
    ///
    /// ```
    /// use martensite::widgets::window_controls::WindowControls;
    ///
    /// assert_eq!(WindowControls::new().button_count(), 3);
    /// ```
    pub fn button_count(&self) -> usize {
        usize::from(self.minimizable) + usize::from(self.maximizable) + usize::from(self.closable)
    }

    /// Drains the last requested action.
    ///
    /// ```
    /// use martensite::widgets::window_controls::WindowControls;
    ///
    /// assert_eq!(WindowControls::new().take_action(), None);
    /// ```
    pub fn take_action(&mut self) -> Option<WindowAction> {
        self.action.take()
    }

    /// Visible actions in order.
    fn actions(&self) -> Vec<WindowAction> {
        // macOS orders close-min-max left-to-right.
        let mut v = Vec::with_capacity(3);
        if self.style == CaptionStyle::Mac {
            if self.closable {
                v.push(WindowAction::Close);
            }
            if self.minimizable {
                v.push(WindowAction::Minimize);
            }
            if self.maximizable {
                v.push(WindowAction::MaximizeRestore);
            }
        } else {
            if self.minimizable {
                v.push(WindowAction::Minimize);
            }
            if self.maximizable {
                v.push(WindowAction::MaximizeRestore);
            }
            if self.closable {
                v.push(WindowAction::Close);
            }
        }
        v
    }

    /// Button hit-test.
    fn at(&self, p: Vec2) -> Option<WindowAction> {
        self.rects
            .iter()
            .find(|(_, r)| r.contains(p))
            .map(|(a, _)| *a)
    }
}

impl Widget for WindowControls {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let s = cx.scale;
        let (w, h) = match self.style {
            CaptionStyle::Windows => (BTN_W_PT * s * self.button_count() as f32, BTN_H_PT * s),
            CaptionStyle::Mac => {
                let n = self.button_count() as f32;
                (
                    n * (MAC_DOT_PT + MAC_GAP_PT) * s + MAC_GAP_PT * s,
                    BTN_H_PT * s,
                )
            }
        };
        Vec2::new(
            w.min(constraints.max_size.x.max(0.0)),
            h.min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(30.0, 20.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        self.rects.clear();
        match self.style {
            CaptionStyle::Windows => {
                let w = BTN_W_PT * cx.scale;
                let mut x = bounds.min_x();
                for action in self.actions() {
                    self.rects
                        .push((action, Rect::new(x, bounds.min_y(), w, bounds.height())));
                    x += w;
                }
            }
            CaptionStyle::Mac => {
                let d = MAC_DOT_PT * cx.scale;
                let gap = MAC_GAP_PT * cx.scale;
                let mut x = bounds.min_x() + gap;
                for action in self.actions() {
                    // Generous hitbox around each dot.
                    self.rects.push((
                        action,
                        Rect::new(x - gap / 2.0, bounds.min_y(), d + gap, bounds.height()),
                    ));
                    x += d + gap;
                }
            }
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Group);
        node.set_label(self.label.clone());
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                ..
            } => {
                if let Some(a) = self.at(*position) {
                    self.held = Some(a);
                    return EventResponse::CapturePointer;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerMoved { position } => {
                let now = self.at(*position);
                if now != self.hovered {
                    self.hovered = now;
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position,
            } => {
                if let Some(a) = self.held.take() {
                    if self.at(*position) == Some(a) {
                        self.action = Some(a);
                    }
                    return EventResponse::ReleasePointer;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerLeave => {
                if self.hovered.take().is_some() {
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let s = cx.scale;
        let pt = |p: Vec2| kurbo::Point::new(f64::from(p.x), f64::from(p.y));
        for (action, rect) in &self.rects {
            let krect = kurbo::Rect::new(
                f64::from(rect.min_x()),
                f64::from(rect.min_y()),
                f64::from(rect.max_x()),
                f64::from(rect.max_y()),
            );
            let hot = self.held == Some(*action) || self.hovered == Some(*action);
            match self.style {
                CaptionStyle::Windows => {
                    if hot {
                        let fill = if *action == WindowAction::Close {
                            cx.color(TokenKey::ErrorColor, CLOSE_HOVER)
                        } else {
                            cx.color(TokenKey::SecondaryColor, FACE_HOVER)
                        };
                        cx.list.push_fill_rect(krect, fill);
                    }
                    // Glyph: − / □ / ⧉ (restore) / ×.
                    let g = cx.color(TokenKey::TextColor, GLYPH);
                    let cxm = (rect.min_x() + rect.max_x()) / 2.0;
                    let cym = (rect.min_y() + rect.max_y()) / 2.0;
                    let gsz = 5.0 * s;
                    let lw = 1.4 * s;
                    let mut path = kurbo::BezPath::new();
                    match action {
                        WindowAction::Minimize => {
                            path.move_to(pt(Vec2::new(cxm - gsz, cym + gsz * 0.5)));
                            path.line_to(pt(Vec2::new(cxm + gsz, cym + gsz * 0.5)));
                        }
                        WindowAction::MaximizeRestore => {
                            if self.maximized {
                                // Two overlapping squares — the back
                                // one fills over the front's corner.
                                let r1 = kurbo::Rect::new(
                                    f64::from(cxm - gsz),
                                    f64::from(cym - gsz * 0.6),
                                    f64::from(cxm + gsz * 0.5),
                                    f64::from(cym + gsz),
                                );
                                let r2 = kurbo::Rect::new(
                                    f64::from(cxm - gsz * 0.5),
                                    f64::from(cym - gsz),
                                    f64::from(cxm + gsz),
                                    f64::from(cym + gsz * 0.5),
                                );
                                cx.list.push_stroke_rect(r2, lw, g);
                                let face = if hot {
                                    if *action == WindowAction::Close {
                                        cx.color(TokenKey::ErrorColor, CLOSE_HOVER)
                                    } else {
                                        cx.color(TokenKey::SecondaryColor, FACE_HOVER)
                                    }
                                } else {
                                    cx.color(TokenKey::BackgroundColor, FACE)
                                };
                                cx.list.push_fill_rect(r1, face);
                                cx.list.push_stroke_rect(r1, lw, g);
                            } else {
                                let r = kurbo::Rect::new(
                                    f64::from(cxm - gsz),
                                    f64::from(cym - gsz),
                                    f64::from(cxm + gsz),
                                    f64::from(cym + gsz),
                                );
                                cx.list.push_stroke_rect(r, lw, g);
                            }
                        }
                        WindowAction::Close => {
                            path.move_to(pt(Vec2::new(cxm - gsz, cym - gsz)));
                            path.line_to(pt(Vec2::new(cxm + gsz, cym + gsz)));
                            path.move_to(pt(Vec2::new(cxm + gsz, cym - gsz)));
                            path.line_to(pt(Vec2::new(cxm - gsz, cym + gsz)));
                        }
                    }
                    if !matches!(action, WindowAction::MaximizeRestore) {
                        cx.list.push_stroke_path(path, lw, g);
                    }
                }
                CaptionStyle::Mac => {
                    let d = MAC_DOT_PT * s;
                    let cxm = (rect.min_x() + rect.max_x()) / 2.0;
                    let cym = (rect.min_y() + rect.max_y()) / 2.0;
                    let dot = kurbo::Rect::new(
                        f64::from(cxm - d / 2.0),
                        f64::from(cym - d / 2.0),
                        f64::from(cxm + d / 2.0),
                        f64::from(cym + d / 2.0),
                    );
                    let color = match action {
                        WindowAction::Close => MAC_CLOSE,
                        WindowAction::Minimize => MAC_MIN,
                        WindowAction::MaximizeRestore => MAC_MAX,
                    };
                    cx.list
                        .push_fill_shape(dot, &martensite_core::shape::Shape::ELLIPSE, color);
                    // Glyph marks appear on hover (macOS convention).
                    if hot {
                        let gsz = 3.0 * s;
                        let mark = [40, 40, 40, 220];
                        let mut path = kurbo::BezPath::new();
                        match action {
                            WindowAction::Close => {
                                path.move_to(pt(Vec2::new(cxm - gsz, cym - gsz)));
                                path.line_to(pt(Vec2::new(cxm + gsz, cym + gsz)));
                                path.move_to(pt(Vec2::new(cxm + gsz, cym - gsz)));
                                path.line_to(pt(Vec2::new(cxm - gsz, cym + gsz)));
                            }
                            WindowAction::Minimize => {
                                path.move_to(pt(Vec2::new(cxm - gsz, cym)));
                                path.line_to(pt(Vec2::new(cxm + gsz, cym)));
                            }
                            WindowAction::MaximizeRestore => {
                                path.move_to(pt(Vec2::new(cxm, cym - gsz)));
                                path.line_to(pt(Vec2::new(cxm, cym + gsz)));
                                path.move_to(pt(Vec2::new(cxm - gsz, cym)));
                                path.line_to(pt(Vec2::new(cxm + gsz, cym)));
                            }
                        }
                        cx.list.push_stroke_path(path, 1.2 * s, mark);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(c: &mut WindowControls) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        c.layout(&mut cx, Rect::new(0.0, 0.0, 138.0, 30.0));
    }

    fn ev(c: &mut WindowControls, e: &WidgetEvent) -> EventResponse {
        c.event(&mut EventContext {
            event: e,
            bounds: c.bounds,
            scale: 1.0,
        })
    }

    fn tap(c: &mut WindowControls, action: WindowAction) {
        let rect = c
            .rects
            .iter()
            .find(|(a, _)| *a == action)
            .map(|(_, r)| *r)
            .unwrap();
        let p = Vec2::new(
            (rect.min_x() + rect.max_x()) / 2.0,
            (rect.min_y() + rect.max_y()) / 2.0,
        );
        ev(
            c,
            &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: p,
                count: 1,
            },
        );
        ev(
            c,
            &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: p,
            },
        );
    }

    #[test]
    fn buttons_park_actions() {
        let mut c = WindowControls::new();
        laid_out(&mut c);
        tap(&mut c, WindowAction::Minimize);
        assert_eq!(c.take_action(), Some(WindowAction::Minimize));
        tap(&mut c, WindowAction::Close);
        assert_eq!(c.take_action(), Some(WindowAction::Close));
    }

    #[test]
    fn release_off_cancels() {
        let mut c = WindowControls::new();
        laid_out(&mut c);
        let r = c.rects[0].1;
        ev(
            &mut c,
            &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new((r.min_x() + r.max_x()) / 2.0, (r.min_y() + r.max_y()) / 2.0),
                count: 1,
            },
        );
        ev(
            &mut c,
            &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: Vec2::new(500.0, 500.0),
            },
        );
        assert_eq!(c.take_action(), None);
    }

    #[test]
    fn mac_orders_close_first() {
        let mut c = WindowControls::mac();
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        c.layout(&mut cx, Rect::new(0.0, 0.0, 80.0, 30.0));
        assert_eq!(c.rects[0].0, WindowAction::Close);
        assert_eq!(c.rects[2].0, WindowAction::MaximizeRestore);
    }

    #[test]
    fn hidden_buttons_skip() {
        let mut c = WindowControls::new();
        c.minimizable = false;
        c.maximizable = false;
        laid_out(&mut c);
        assert_eq!(c.rects.len(), 1);
        assert_eq!(c.rects[0].0, WindowAction::Close);
    }

    #[test]
    fn paint_without_painter() {
        for mut c in [WindowControls::new(), WindowControls::mac()] {
            laid_out(&mut c);
            let mut list = martensite_core::PaintList::default();
            let theme = martensite_theme::Theme::new("test");
            c.paint(&mut PaintContext {
                list: &mut list,
                bounds: c.bounds,
                theme: &theme,
                scale: 1.0,
                text_painter: None,
            });
            assert!(!list.is_empty());
        }
    }
}
