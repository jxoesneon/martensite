//! Split button — a primary-action zone fused with a chevron zone
//! that requests a dropdown (WinUI `SplitButton`, QToolButton's
//! MenuButtonPopup, Bootstrap `.dropdown-toggle-split`).
//!
//! The primary zone behaves exactly like a [`Button`] — it *is* one
//! internally, so activation rides the shared `take_activated` seam.
//! The chevron zone parks a separate request through
//! [`SplitButton::take_dropped`]; the consumer opens a `Menu`/`Popup`
//! anchored to [`SplitButton::chevron_bounds`].
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::SplitButton;
//!
//! let mut b = SplitButton::new("Save");
//! assert!(!b.take_dropped());
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::widgets::Button;

/// Chevron zone width in points.
const CHEV_PT: f32 = 22.0;
/// Face height in points.
const H_PT: f32 = 28.0;
/// Horizontal padding inside the primary zone (points).
const PAD_PT: f32 = 12.0;

/// Split button.
///
/// # Examples
///
/// ```
/// use martensite::core::Widget;
/// use martensite::widgets::SplitButton;
///
/// assert_eq!(SplitButton::new("x").child_count(), 1);
/// ```
pub struct SplitButton {
    /// Accessibility label override.
    pub label: Option<String>,
    /// Whether input reaches the face.
    pub enabled: bool,
    /// The primary action (painted as the left zone's content).
    button: Button,
    /// Parked chevron request.
    dropped: bool,
    /// Whether the chevron is pressed (visual).
    chevron_pressed: bool,
    bounds: Rect,
    face_bounds: Option<Rect>,
    chevron_bounds: Option<Rect>,
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl SplitButton {
    /// Creates a split button with `text` as the primary action.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::SplitButton;
    ///
    /// assert!(SplitButton::new("Save").enabled);
    /// ```
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            label: None,
            enabled: true,
            button: Button::new(text),
            dropped: false,
            chevron_pressed: false,
            bounds: Rect::default(),
            face_bounds: None,
            chevron_bounds: None,
            text_painter: None,
        }
    }

    /// Sets the accessibility label.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::SplitButton;
    ///
    /// let b = SplitButton::new("x").label("Save options");
    /// assert_eq!(b.label.as_deref(), Some("Save options"));
    /// ```
    #[must_use]
    pub fn label(mut self, text: impl Into<String>) -> Self {
        self.label = Some(text.into());
        self
    }

    /// Sets whether input reaches the face.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::SplitButton;
    ///
    /// assert!(!SplitButton::new("x").enabled(false).enabled);
    /// ```
    #[must_use]
    pub fn enabled(mut self, flag: bool) -> Self {
        self.enabled = flag;
        self.button.enabled = flag;
        self
    }

    /// Shares a [`crate::text_paint::TextPainter`] for the label.
    #[must_use]
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.button = self.button.with_text_painter(painter.clone());
        self.text_painter = Some(painter);
        self
    }

    /// The primary action's parked activation.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::SplitButton;
    ///
    /// assert!(!SplitButton::new("x").take_activated());
    /// ```
    #[inline]
    pub fn take_activated(&mut self) -> bool {
        self.button.take_activated()
    }

    /// Takes the parked chevron/dropdown request.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::SplitButton;
    ///
    /// assert!(!SplitButton::new("x").take_dropped());
    /// ```
    #[inline]
    pub fn take_dropped(&mut self) -> bool {
        std::mem::take(&mut self.dropped)
    }

    /// Chevron zone rect — anchor the consumer's popup here.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::SplitButton;
    ///
    /// assert!(SplitButton::new("x").chevron_bounds().is_none());
    /// ```
    #[inline]
    pub fn chevron_bounds(&self) -> Option<Rect> {
        self.chevron_bounds
    }

    /// Primary label text.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::SplitButton;
    ///
    /// assert_eq!(SplitButton::new("Save").text(), "Save");
    /// ```
    #[inline]
    pub fn text(&self) -> &str {
        &self.button.label
    }
}

impl std::fmt::Debug for SplitButton {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SplitButton")
            .field("text", &self.text())
            .field("enabled", &self.enabled)
            .finish()
    }
}

impl Widget for SplitButton {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let inner = self.button.measure(cx, constraints);
        Vec2::new(
            (inner.x + cx.pt(CHEV_PT + PAD_PT)).min(constraints.max_size.x.max(0.0)),
            inner.y.max(cx.pt(H_PT)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(72.0, 24.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        let chev_w = cx.pt(CHEV_PT).min(bounds.width() / 3.0);
        let face = Rect::new(
            bounds.min_x(),
            bounds.min_y(),
            (bounds.width() - chev_w).max(0.0),
            bounds.height(),
        );
        let chev = Rect::new(face.max_x(), bounds.min_y(), chev_w, bounds.height());
        self.face_bounds = Some(face);
        self.chevron_bounds = Some(chev);
        cx.layout_child(&mut self.button, face);
        self.button.enabled = self.enabled;
    }

    fn paint(&self, cx: &mut PaintContext) {
        // The Button child paints the primary zone; we paint the
        // chevron zone: separator hairline + pressed face + glyph.
        let Some(chev) = self.chevron_bounds else {
            return;
        };
        let f = |r: Rect| {
            kurbo::Rect::new(
                f64::from(r.min_x()),
                f64::from(r.min_y()),
                f64::from(r.max_x()),
                f64::from(r.max_y()),
            )
        };
        let surface = cx.color(TokenKey::SurfaceColor, [55, 55, 58, 255]);
        let border = cx.color(TokenKey::BorderColor, [90, 90, 90, 255]);
        let fg = if self.enabled {
            cx.color(TokenKey::TextColor, [220, 220, 220, 255])
        } else {
            cx.color(TokenKey::TextMutedColor, [140, 140, 140, 255])
        };
        let face = if self.chevron_pressed && self.enabled {
            [
                surface[0].saturating_sub(16),
                surface[1].saturating_sub(16),
                surface[2].saturating_sub(16),
                255,
            ]
        } else {
            surface
        };
        cx.list.push_fill_rect(f(chev), face);
        cx.list.push_fill_rect(
            f(Rect::new(chev.min_x(), chev.min_y(), 1.0, chev.height())),
            border,
        );
        // Chevron glyph — a small down-triangle centered in the zone.
        let cx_mid = chev.min_x() + chev.width() / 2.0;
        let cy_mid = chev.min_y() + chev.height() / 2.0;
        let s = cx.pt(4.0);
        let mut tri = kurbo::BezPath::new();
        tri.move_to(kurbo::Point::new(
            f64::from(cx_mid - s),
            f64::from(cy_mid - s / 2.0),
        ));
        tri.line_to(kurbo::Point::new(
            f64::from(cx_mid + s),
            f64::from(cy_mid - s / 2.0),
        ));
        tri.line_to(kurbo::Point::new(
            f64::from(cx_mid),
            f64::from(cy_mid + s / 2.0),
        ));
        tri.close_path();
        cx.list.push_path(tri, fg);
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.enabled {
            return EventResponse::Ignored;
        }
        match cx.event {
            WidgetEvent::PointerPressed {
                position, button, ..
            } => {
                if *button == martensite_core::PointerButton::Primary {
                    if self.chevron_bounds.is_some_and(|c| c.contains(*position)) {
                        self.chevron_pressed = true;
                        return EventResponse::CapturePointer;
                    }
                    if self.face_bounds.is_some_and(|f| f.contains(*position)) {
                        if let Some(b) = self.face_bounds {
                            let mut child_cx = EventContext {
                                event: cx.event,
                                bounds: b,
                                scale: cx.scale,
                            };
                            return self.button.event(&mut child_cx);
                        }
                    }
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerReleased { position, .. } => {
                if self.chevron_pressed {
                    self.chevron_pressed = false;
                    if self.chevron_bounds.is_some_and(|c| c.contains(*position)) {
                        self.dropped = true;
                    }
                    return EventResponse::ReleasePointer;
                }
                if self.face_bounds.is_some_and(|f| f.contains(*position)) {
                    let mut child_cx = EventContext {
                        event: cx.event,
                        bounds: self.face_bounds.unwrap(),
                        scale: cx.scale,
                    };
                    return self.button.event(&mut child_cx);
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerMoved { position } => {
                // Hover/press tracking for the primary zone.
                if self.face_bounds.is_some_and(|f| f.contains(*position))
                    || self.button.is_pressed()
                {
                    let mut child_cx = EventContext {
                        event: cx.event,
                        bounds: self.face_bounds.unwrap_or(self.bounds),
                        scale: cx.scale,
                    };
                    return self.button.event(&mut child_cx);
                }
                EventResponse::Ignored
            }
            _ => {
                // Keys/focus go to the primary action.
                let mut child_cx = EventContext {
                    event: cx.event,
                    bounds: self.face_bounds.unwrap_or(self.bounds),
                    scale: cx.scale,
                };
                self.button.event(&mut child_cx)
            }
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Button);
        if let Some(ref label) = self.label {
            node.set_label(label.as_str());
        } else {
            node.set_label(self.button.label.clone());
        }
        if !self.enabled {
            node.set_disabled();
        }
        node.set_has_popup(accesskit::HasPopup::Menu);
    }

    fn child_count(&self) -> usize {
        1
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        (index == 0).then_some(&self.button as &dyn Widget)
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        (index == 0).then_some(&mut self.button as &mut dyn Widget)
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        (index == 0).then_some(self.face_bounds).flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::{HotNode, PointerButton, WidgetEvent};

    fn laid_out(b: &mut SplitButton) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        b.layout(&mut cx, Rect::new(0.0, 0.0, 140.0, 28.0));
    }

    fn ev(b: &mut SplitButton, event: &WidgetEvent) -> EventResponse {
        let mut cx = EventContext {
            event,
            bounds: Rect::new(0.0, 0.0, 140.0, 28.0),
            scale: 1.0,
        };
        b.event(&mut cx)
    }

    fn click(b: &mut SplitButton, x: f32) {
        let down = WidgetEvent::PointerPressed {
            position: Vec2::new(x, 14.0),
            button: PointerButton::Primary,
            count: 1,
        };
        let up = WidgetEvent::PointerReleased {
            position: Vec2::new(x, 14.0),
            button: PointerButton::Primary,
        };
        ev(b, &down);
        ev(b, &up);
    }

    #[test]
    fn builder() {
        let mut b = SplitButton::new("Save");
        assert_eq!(b.text(), "Save");
        assert!(!b.take_activated());
        assert!(!b.take_dropped());
    }

    #[test]
    fn chevron_click_parks_drop() {
        let mut b = SplitButton::new("Save");
        laid_out(&mut b);
        let chev = b.chevron_bounds().unwrap();
        click(&mut b, chev.min_x() + chev.width() / 2.0);
        assert!(b.take_dropped());
        assert!(!b.take_activated());
    }

    #[test]
    fn face_click_activates() {
        let mut b = SplitButton::new("Save");
        laid_out(&mut b);
        click(&mut b, 20.0); // inside the primary zone
        assert!(b.take_activated());
        assert!(!b.take_dropped());
    }

    #[test]
    fn chevron_release_outside_does_not_drop() {
        let mut b = SplitButton::new("Save");
        laid_out(&mut b);
        let chev = b.chevron_bounds().unwrap();
        let down = WidgetEvent::PointerPressed {
            position: Vec2::new(chev.min_x() + 5.0, 14.0),
            button: PointerButton::Primary,
            count: 1,
        };
        let up = WidgetEvent::PointerReleased {
            position: Vec2::new(5.0, 200.0),
            button: PointerButton::Primary,
        };
        ev(&mut b, &down);
        ev(&mut b, &up);
        assert!(!b.take_dropped());
    }

    #[test]
    fn disabled_inert() {
        let mut b = SplitButton::new("Save").enabled(false);
        laid_out(&mut b);
        let chev = b.chevron_bounds().unwrap();
        click(&mut b, chev.min_x() + 5.0);
        click(&mut b, 20.0);
        assert!(!b.take_dropped());
        assert!(!b.take_activated());
    }
}
