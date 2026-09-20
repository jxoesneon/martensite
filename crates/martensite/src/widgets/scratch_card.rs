//! `ScratchCard` — a scratch-to-reveal overlay (lottery-ticket /
//! promo-reveal idiom).
//!
//! A child widget paints underneath a metallic foil layer. The foil
//! is a tile grid; dragging the pointer scratches cells away.
//! When the scratched fraction crosses
//! [`ScratchCard::reveal_threshold`], the foil clears entirely and
//! `0.0..=1.0` progress is parked in [`ScratchCard::take_revealed`]
//! exactly once. Display progress is available via
//! [`ScratchCard::progress`].
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::scratch_card::ScratchCard;
//!
//! let c = ScratchCard::new(martensite_core::DummyWidget);
//! assert_eq!(c.progress(), 0.0);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};

const COLS: usize = 12;
const ROWS: usize = 8;
/// Scratch radius in tiles.
const BRUSH: f32 = 1.6;

const FOIL: [u8; 4] = [168, 172, 180, 255];
const FOIL_ALT: [u8; 4] = [146, 150, 158, 255];
const EDGE: [u8; 4] = [110, 114, 122, 255];

/// The scratch card — see the module docs.
///
/// ```
/// use martensite::widgets::scratch_card::ScratchCard;
/// use martensite_core::Widget;
///
/// assert_eq!(ScratchCard::new(martensite_core::DummyWidget).child_count(), 1);
/// ```
pub struct ScratchCard {
    /// Accessibility label.
    pub label: String,
    /// Fraction of scratched cells that triggers a full reveal.
    pub reveal_threshold: f32,
    /// Whether the card starts covered.
    pub covered: bool,
    content: Box<dyn Widget>,
    content_rect: Rect,
    scratched: Vec<bool>,
    scratching: bool,
    revealed: bool,
    reveal_flagged: bool,
    bounds: Rect,
    scale: f32,
}

impl std::fmt::Debug for ScratchCard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScratchCard")
            .field("progress", &self.progress())
            .field("revealed", &self.revealed)
            .finish()
    }
}

impl ScratchCard {
    /// A covered card hosting `content`.
    ///
    /// ```
    /// use martensite::widgets::scratch_card::ScratchCard;
    ///
    /// assert!(ScratchCard::new(martensite_core::DummyWidget).covered);
    /// ```
    pub fn new(content: impl Widget + 'static) -> Self {
        Self {
            label: "Scratch card".to_string(),
            reveal_threshold: 0.6,
            covered: true,
            content: Box::new(content),
            content_rect: Rect::new(0.0, 0.0, 0.0, 0.0),
            scratched: vec![false; COLS * ROWS],
            scratching: false,
            revealed: false,
            reveal_flagged: false,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::scratch_card::ScratchCard;
    ///
    /// assert_eq!(ScratchCard::new(martensite_core::DummyWidget).label("Prize").label, "Prize");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Scratched fraction that reveals the card (default 0.6).
    ///
    /// ```
    /// use martensite::widgets::scratch_card::ScratchCard;
    ///
    /// assert_eq!(ScratchCard::new(martensite_core::DummyWidget).reveal_threshold(0.5).reveal_threshold, 0.5);
    /// ```
    pub fn reveal_threshold(mut self, threshold: f32) -> Self {
        self.reveal_threshold = threshold.clamp(0.05, 1.0);
        self
    }

    /// Scratched fraction of the foil, `0.0..=1.0`.
    ///
    /// ```
    /// use martensite::widgets::scratch_card::ScratchCard;
    ///
    /// assert_eq!(ScratchCard::new(martensite_core::DummyWidget).progress(), 0.0);
    /// ```
    pub fn progress(&self) -> f32 {
        if self.revealed {
            return 1.0;
        }
        let n = self.scratched.iter().filter(|s| **s).count();
        n as f32 / self.scratched.len() as f32
    }

    /// Whether the foil has fully cleared.
    ///
    /// ```
    /// use martensite::widgets::scratch_card::ScratchCard;
    ///
    /// assert!(!ScratchCard::new(martensite_core::DummyWidget).is_revealed());
    /// ```
    pub fn is_revealed(&self) -> bool {
        self.revealed
    }

    /// `true` once when the reveal threshold is crossed.
    ///
    /// ```
    /// use martensite::widgets::scratch_card::ScratchCard;
    ///
    /// assert!(!ScratchCard::new(martensite_core::DummyWidget).take_revealed());
    /// ```
    pub fn take_revealed(&mut self) -> bool {
        if self.reveal_flagged {
            self.reveal_flagged = false;
            return true;
        }
        false
    }

    /// Re-covers the card with fresh foil.
    ///
    /// ```
    /// use martensite::widgets::scratch_card::ScratchCard;
    ///
    /// let mut c = ScratchCard::new(martensite_core::DummyWidget);
    /// c.reset();
    /// assert!(c.covered);
    /// ```
    pub fn reset(&mut self) {
        self.scratched.iter_mut().for_each(|s| *s = false);
        self.covered = true;
        self.revealed = false;
        self.reveal_flagged = false;
        self.scratching = false;
    }

    fn scratch_at(&mut self, p: Vec2) {
        let b = self.content_rect;
        if b.width() <= 0.0 || b.height() <= 0.0 {
            return;
        }
        let cw = b.width() / COLS as f32;
        let ch = b.height() / ROWS as f32;
        let tile = cw.max(ch);
        let ccx = ((p.x - b.min_x()) / cw).floor() as i32;
        let ccy = ((p.y - b.min_y()) / ch).floor() as i32;
        for y in 0..ROWS as i32 {
            for x in 0..COLS as i32 {
                let dx = (x - ccx) as f32 * cw;
                let dy = (y - ccy) as f32 * ch;
                if dx * dx + dy * dy <= (BRUSH * tile).powi(2) {
                    self.scratched[y as usize * COLS + x as usize] = true;
                }
            }
        }
        if !self.revealed && self.progress() >= self.reveal_threshold {
            self.revealed = true;
            self.covered = false;
            self.reveal_flagged = true;
        }
    }
}

impl Widget for ScratchCard {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        self.content.measure(cx, constraints)
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(80.0, 48.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        self.content_rect = bounds;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Group);
        node.set_label(self.label.clone());
        node.set_value(if self.revealed {
            "revealed".to_string()
        } else {
            format!("{:.0}% scratched", self.progress() * 100.0)
        });
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.covered {
            return EventResponse::Ignored;
        }
        match cx.event {
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                ..
            } => {
                self.scratching = true;
                self.scratch_at(*position);
                EventResponse::RequestRepaint
            }
            WidgetEvent::PointerMoved { position } => {
                if self.scratching {
                    self.scratch_at(*position);
                    EventResponse::RequestRepaint
                } else {
                    EventResponse::Ignored
                }
            }
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                ..
            } => {
                self.scratching = false;
                EventResponse::RequestRepaint
            }
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        if !self.covered {
            return;
        }
        let b = self.content_rect;
        let cw = b.width() / COLS as f32;
        let ch = b.height() / ROWS as f32;
        for y in 0..ROWS {
            for x in 0..COLS {
                if self.scratched[y * COLS + x] {
                    continue;
                }
                let r = kurbo::Rect::new(
                    f64::from(b.min_x() + x as f32 * cw),
                    f64::from(b.min_y() + y as f32 * ch),
                    f64::from(b.min_x() + (x + 1) as f32 * cw),
                    f64::from(b.min_y() + (y + 1) as f32 * ch),
                );
                let c = if (x + y) % 2 == 0 { FOIL } else { FOIL_ALT };
                cx.list.push_fill_rect(r, c);
            }
        }
        cx.list.push_stroke_rect(
            kurbo::Rect::new(
                f64::from(b.min_x()),
                f64::from(b.min_y()),
                f64::from(b.max_x()),
                f64::from(b.max_y()),
            ),
            1.0,
            EDGE,
        );
        // Foil sheen band.
        let sheen = kurbo::Rect::new(
            f64::from(b.min_x()),
            f64::from(b.min_y()),
            f64::from(b.max_x()),
            f64::from(b.min_y() + b.height() * 0.22),
        );
        cx.list.push_fill_rect(sheen, [255, 255, 255, 18]);
    }

    fn clips_children(&self) -> bool {
        true
    }

    fn child_count(&self) -> usize {
        1
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        (index == 0).then_some(&*self.content)
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        (index == 0).then_some(&mut *self.content)
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        (index == 0).then_some(self.content_rect)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::{DummyWidget, HotNode};

    fn laid_out(c: &mut ScratchCard) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        c.layout(&mut cx, Rect::new(0.0, 0.0, 240.0, 160.0));
    }

    fn drag(c: &mut ScratchCard, x: f32, y: f32) {
        c.event(&mut EventContext {
            event: &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(x, y),
                count: 1,
            },
            bounds: c.bounds,
            scale: 1.0,
        });
        c.event(&mut EventContext {
            event: &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: Vec2::new(x, y),
            },
            bounds: c.bounds,
            scale: 1.0,
        });
    }

    #[test]
    fn scratch_increases_progress() {
        let mut c = ScratchCard::new(DummyWidget);
        laid_out(&mut c);
        drag(&mut c, 20.0, 20.0);
        assert!(c.progress() > 0.0);
    }

    #[test]
    fn crossing_threshold_reveals() {
        let mut c = ScratchCard::new(DummyWidget).reveal_threshold(0.2);
        laid_out(&mut c);
        for x in (0..240).step_by(20) {
            drag(&mut c, x as f32, 80.0);
        }
        assert!(c.is_revealed());
        assert!(c.take_revealed());
        assert!(!c.take_revealed());
    }

    #[test]
    fn reset_recovers() {
        let mut c = ScratchCard::new(DummyWidget);
        laid_out(&mut c);
        drag(&mut c, 20.0, 20.0);
        c.reset();
        assert_eq!(c.progress(), 0.0);
        assert!(c.covered);
    }

    #[test]
    fn paint_without_painter() {
        let mut c = ScratchCard::new(DummyWidget);
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
