//! `RubberBand` — the drag-marquee selection rectangle (desktop /
//! file-manager / canvas idiom).
//!
//! A transparent overlay leaf: primary press anchors the band, drag
//! extends it, release finalizes and parks the normalized
//! [`Rect`](martensite_core::Rect) in [`RubberBand::take_selection`]
//! for the host to
//! intersect against its items. [`RubberBand::active`] reports the
//! live band while dragging so hosts can highlight incrementally.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::rubber_band::RubberBand;
//!
//! assert_eq!(RubberBand::new().active(), None);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};

const FILL: [u8; 4] = [90, 140, 220, 44];
const EDGE: [u8; 4] = [90, 140, 220, 200];
/// Bands smaller than this (px) are treated as clicks, not selects.
const MIN_PX: f32 = 3.0;

/// The marquee overlay — see the module docs.
///
/// ```
/// use martensite::widgets::rubber_band::RubberBand;
///
/// assert_eq!(RubberBand::new().active(), None);
/// ```
pub struct RubberBand {
    /// Accessibility label.
    pub label: String,
    /// Minimum drag size (px) before the band counts.
    pub threshold: f32,
    anchor: Option<Vec2>,
    current: Option<Vec2>,
    finished: Option<Rect>,
    bounds: Rect,
    scale: f32,
}

impl std::fmt::Debug for RubberBand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RubberBand")
            .field("dragging", &self.anchor.is_some())
            .finish()
    }
}

impl RubberBand {
    /// An empty marquee overlay.
    ///
    /// ```
    /// use martensite::widgets::rubber_band::RubberBand;
    ///
    /// assert!(!RubberBand::new().is_dragging());
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Marquee selection".to_string(),
            threshold: MIN_PX,
            anchor: None,
            current: None,
            finished: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Minimum drag size (px).
    ///
    /// ```
    /// use martensite::widgets::rubber_band::RubberBand;
    ///
    /// assert_eq!(RubberBand::new().threshold(8.0).threshold, 8.0);
    /// ```
    pub fn threshold(mut self, px: f32) -> Self {
        self.threshold = px.max(0.0);
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::rubber_band::RubberBand;
    ///
    /// assert_eq!(RubberBand::new().label("Select items").label, "Select items");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Whether a drag is in progress.
    ///
    /// ```
    /// use martensite::widgets::rubber_band::RubberBand;
    ///
    /// assert!(!RubberBand::new().is_dragging());
    /// ```
    pub fn is_dragging(&self) -> bool {
        self.anchor.is_some()
    }

    /// The live band rect while dragging, normalized to positive
    /// width/height; `None` otherwise.
    ///
    /// ```
    /// use martensite::widgets::rubber_band::RubberBand;
    ///
    /// assert_eq!(RubberBand::new().active(), None);
    /// ```
    pub fn active(&self) -> Option<Rect> {
        match (self.anchor, self.current) {
            (Some(a), Some(c)) => Some(band_rect(a, c)),
            _ => None,
        }
    }

    /// Drains the finalized selection rect.
    ///
    /// ```
    /// use martensite::widgets::rubber_band::RubberBand;
    ///
    /// assert_eq!(RubberBand::new().take_selection(), None);
    /// ```
    pub fn take_selection(&mut self) -> Option<Rect> {
        self.finished.take()
    }

    /// Cancels an in-progress drag.
    ///
    /// ```
    /// use martensite::widgets::rubber_band::RubberBand;
    ///
    /// let mut b = RubberBand::new();
    /// b.cancel();
    /// assert!(!b.is_dragging());
    /// ```
    pub fn cancel(&mut self) {
        self.anchor = None;
        self.current = None;
    }
}

impl Default for RubberBand {
    fn default() -> Self {
        Self::new()
    }
}

fn band_rect(a: Vec2, c: Vec2) -> Rect {
    let x = a.x.min(c.x);
    let y = a.y.min(c.y);
    Rect::new(x, y, (a.x - c.x).abs(), (a.y - c.y).abs())
}

impl Widget for RubberBand {
    fn measure(&mut self, _cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        constraints.max_size
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(16.0, 16.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Group);
        node.set_label(self.label.clone());
        node.set_value(if self.is_dragging() {
            "selecting"
        } else {
            "idle"
        });
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                ..
            } => {
                self.anchor = Some(*position);
                self.current = Some(*position);
                EventResponse::CaptureFocus
            }
            WidgetEvent::PointerMoved { position } => {
                if self.anchor.is_some() {
                    self.current = Some(*position);
                    EventResponse::RequestRepaint
                } else {
                    EventResponse::Ignored
                }
            }
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position,
            } => {
                if let Some(a) = self.anchor.take() {
                    self.current = None;
                    let r = band_rect(a, *position);
                    if r.width() >= self.threshold && r.height() >= self.threshold {
                        self.finished = Some(r);
                    }
                    EventResponse::RequestRepaint
                } else {
                    EventResponse::Ignored
                }
            }
            WidgetEvent::PointerLeave => {
                if self.anchor.is_some() {
                    EventResponse::RequestRepaint
                } else {
                    EventResponse::Ignored
                }
            }
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let Some(r) = self.active() else {
            return;
        };
        let kr = kurbo::Rect::new(
            f64::from(r.min_x()),
            f64::from(r.min_y()),
            f64::from(r.max_x()),
            f64::from(r.max_y()),
        );
        cx.list.push_fill_rect(kr, FILL);
        cx.list.push_stroke_rect(kr, 1.0, EDGE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(b: &mut RubberBand) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        b.layout(&mut cx, Rect::new(0.0, 0.0, 400.0, 300.0));
    }

    fn drag(b: &mut RubberBand, from: Vec2, to: Vec2) {
        b.event(&mut EventContext {
            event: &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: from,
                count: 1,
            },
            bounds: b.bounds,
            scale: 1.0,
        });
        b.event(&mut EventContext {
            event: &WidgetEvent::PointerMoved { position: to },
            bounds: b.bounds,
            scale: 1.0,
        });
        b.event(&mut EventContext {
            event: &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: to,
            },
            bounds: b.bounds,
            scale: 1.0,
        });
    }

    #[test]
    fn drag_parks_normalized_rect() {
        let mut b = RubberBand::new();
        laid_out(&mut b);
        // Drag bottom-right → top-left; rect must normalize.
        drag(&mut b, Vec2::new(200.0, 150.0), Vec2::new(50.0, 40.0));
        let r = b.take_selection().unwrap();
        assert_eq!(r.min_x(), 50.0);
        assert_eq!(r.min_y(), 40.0);
        assert_eq!(r.width(), 150.0);
        assert_eq!(r.height(), 110.0);
        assert_eq!(b.take_selection(), None);
    }

    #[test]
    fn tiny_drag_is_a_click() {
        let mut b = RubberBand::new();
        laid_out(&mut b);
        drag(&mut b, Vec2::new(10.0, 10.0), Vec2::new(11.0, 11.0));
        assert_eq!(b.take_selection(), None);
    }

    #[test]
    fn active_reports_live_rect() {
        let mut b = RubberBand::new();
        laid_out(&mut b);
        b.event(&mut EventContext {
            event: &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(10.0, 10.0),
                count: 1,
            },
            bounds: b.bounds,
            scale: 1.0,
        });
        b.event(&mut EventContext {
            event: &WidgetEvent::PointerMoved {
                position: Vec2::new(60.0, 50.0),
            },
            bounds: b.bounds,
            scale: 1.0,
        });
        let r = b.active().unwrap();
        assert_eq!(r.width(), 50.0);
        b.cancel();
        assert_eq!(b.active(), None);
    }

    #[test]
    fn paint_empty_is_noop() {
        let mut b = RubberBand::new();
        laid_out(&mut b);
        let mut list = martensite_core::PaintList::default();
        let theme = martensite_theme::Theme::new("test");
        b.paint(&mut PaintContext {
            list: &mut list,
            bounds: b.bounds,
            theme: &theme,
            scale: 1.0,
            text_painter: None,
        });
        assert!(list.is_empty());
        drag(&mut b, Vec2::new(5.0, 5.0), Vec2::new(50.0, 40.0));
        // After release the band clears.
        b.event(&mut EventContext {
            event: &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(5.0, 5.0),
                count: 1,
            },
            bounds: b.bounds,
            scale: 1.0,
        });
        b.event(&mut EventContext {
            event: &WidgetEvent::PointerMoved {
                position: Vec2::new(80.0, 60.0),
            },
            bounds: b.bounds,
            scale: 1.0,
        });
        b.paint(&mut PaintContext {
            list: &mut list,
            bounds: b.bounds,
            theme: &theme,
            scale: 1.0,
            text_painter: None,
        });
        assert!(!list.is_empty());
    }
}
