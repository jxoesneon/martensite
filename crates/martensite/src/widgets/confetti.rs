//! `Confetti` — a celebration particle burst (checkout-success /
//! achievement-unlocked idiom).
//!
//! [`Confetti::burst`] spawns particles at a normalized origin;
//! `tick` integrates their fall, drift, and fade with a cheap
//! deterministic pseudo-random spread. When the last particle
//! dies, [`Confetti::take_done`] parks `true` so the host can
//! dismiss the overlay. Pure decoration — no hit-testing.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::confetti::Confetti;
//!
//! let mut c = Confetti::new();
//! c.burst(0.5, 0.0);
//! assert!(c.particle_count() > 0);
//! ```

use std::time::Duration;

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget,
};

const COUNT: usize = 48;
const LIFE_S: f32 = 2.2;
const GRAVITY: f32 = 500.0;
const DRIFT: f32 = 90.0;
const SIZE_PT: f32 = 8.0;

const PALETTE: [[u8; 4]; 6] = [
    [88, 130, 247, 255],
    [46, 160, 67, 255],
    [255, 189, 46, 255],
    [255, 95, 86, 255],
    [200, 120, 255, 255],
    [90, 200, 220, 255],
];

struct Particle {
    pos: Vec2,
    vel: Vec2,
    /// Seconds remaining.
    life: f32,
    /// Total lifetime for fade.
    max_life: f32,
    color: [u8; 4],
    /// Horizontal stretch for the rectangle confetti look.
    aspect: f32,
}

/// A celebration burst — see the module docs.
///
/// ```
/// use martensite::widgets::confetti::Confetti;
///
/// assert_eq!(Confetti::new().particle_count(), 0);
/// ```
#[derive(Default)]
pub struct Confetti {
    /// Accessibility label.
    pub label: String,
    /// Particles per burst.
    pub count: usize,
    particles: Vec<Particle>,
    done: bool,
    seed: u32,
    bounds: Rect,
    scale: f32,
}

impl std::fmt::Debug for Confetti {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Confetti")
            .field("particles", &self.particles.len())
            .finish()
    }
}

impl Confetti {
    /// No particles.
    ///
    /// ```
    /// use martensite::widgets::confetti::Confetti;
    ///
    /// assert_eq!(Confetti::new().particle_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Confetti".to_string(),
            count: COUNT,
            particles: Vec::new(),
            done: false,
            seed: 0x9E3779B9,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::confetti::Confetti;
    ///
    /// assert_eq!(Confetti::new().label("Win").label, "Win");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Particle-count builder.
    ///
    /// ```
    /// use martensite::widgets::confetti::Confetti;
    ///
    /// let mut c = Confetti::new().count(10);
    /// c.burst(0.5, 0.0);
    /// assert_eq!(c.particle_count(), 10);
    /// ```
    pub fn count(mut self, count: usize) -> Self {
        self.count = count;
        self
    }

    /// Spawns a burst at normalized `(x, y)` in the bounds.
    ///
    /// ```
    /// use martensite::widgets::confetti::Confetti;
    ///
    /// let mut c = Confetti::new().count(8);
    /// c.burst(0.5, 0.2);
    /// assert_eq!(c.particle_count(), 8);
    /// ```
    pub fn burst(&mut self, nx: f32, ny: f32) {
        let origin = Vec2::new(
            self.bounds.min_x() + self.bounds.width() * nx.clamp(0.0, 1.0),
            self.bounds.min_y() + self.bounds.height() * ny.clamp(0.0, 1.0),
        );
        for _ in 0..self.count {
            let a = self.rand(); // palette pick
            let b = self.rand(); // speed
            let c = self.rand(); // drift direction
            let d = self.rand(); // life jitter
            let e = self.rand(); // aspect
            let life = LIFE_S * (0.6 + 0.4 * d);
            let particle = Particle {
                pos: origin,
                vel: Vec2::new(
                    (c - 0.5) * 2.0 * DRIFT * self.scale,
                    -(80.0 + 220.0 * b) * self.scale,
                ),
                life,
                max_life: life,
                color: PALETTE[(a * PALETTE.len() as f32) as usize % PALETTE.len()],
                aspect: 0.4 + e * 1.2,
            };
            self.particles.push(particle);
        }
    }

    /// Live particle count.
    ///
    /// ```
    /// use martensite::widgets::confetti::Confetti;
    ///
    /// assert_eq!(Confetti::new().particle_count(), 0);
    /// ```
    pub fn particle_count(&self) -> usize {
        self.particles.len()
    }

    /// Whether any particles are alive.
    ///
    /// ```
    /// use martensite::widgets::confetti::Confetti;
    ///
    /// assert!(!Confetti::new().is_active());
    /// ```
    pub fn is_active(&self) -> bool {
        !self.particles.is_empty()
    }

    /// Drains the all-particles-dead flag.
    ///
    /// ```
    /// use martensite::widgets::confetti::Confetti;
    ///
    /// let mut c = Confetti::new();
    /// assert!(!c.take_done());
    /// ```
    pub fn take_done(&mut self) -> bool {
        std::mem::take(&mut self.done)
    }

    /// Cheap xorshift for the deterministic spread.
    fn rand(&mut self) -> f32 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        (self.seed >> 8) as f32 / 16_777_216.0
    }
}

impl Widget for Confetti {
    fn measure(&mut self, _cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        constraints.max_size
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(40.0, 40.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Image);
        node.set_label(self.label.clone());
    }

    fn event(&mut self, _cx: &mut EventContext) -> EventResponse {
        EventResponse::Ignored
    }

    fn tick(&mut self, dt: Duration) -> bool {
        if self.particles.is_empty() {
            return false;
        }
        let dts = dt.as_secs_f32();
        let g = GRAVITY * self.scale;
        let mut any = false;
        for p in &mut self.particles {
            p.life -= dts;
            if p.life > 0.0 {
                any = true;
                p.vel.y += g * dts;
                p.pos += p.vel * dts;
            }
        }
        self.particles.retain(|p| p.life > 0.0);
        if self.particles.is_empty() {
            self.done = true;
        }
        any || !self.particles.is_empty()
    }

    fn paint(&self, cx: &mut PaintContext) {
        let s = cx.scale;
        for p in &self.particles {
            let fade = (p.life / p.max_life).clamp(0.0, 1.0);
            let mut color = p.color;
            color[3] = (f32::from(color[3]) * fade) as u8;
            let w = SIZE_PT * s;
            let h = w * p.aspect;
            let rect = kurbo::Rect::new(
                f64::from(p.pos.x - w / 2.0),
                f64::from(p.pos.y - h / 2.0),
                f64::from(p.pos.x + w / 2.0),
                f64::from(p.pos.y + h / 2.0),
            );
            cx.list.push_fill_rect(rect, color);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(c: &mut Confetti) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        c.layout(&mut cx, Rect::new(0.0, 0.0, 300.0, 200.0));
    }

    #[test]
    fn burst_spawns_at_origin() {
        let mut c = Confetti::new().count(5);
        laid_out(&mut c);
        c.burst(0.5, 0.5);
        assert_eq!(c.particle_count(), 5);
        // Origin is bounds center.
        assert!((c.particles[0].pos.x - 150.0).abs() < 1.0);
    }

    #[test]
    fn tick_falls_and_fades() {
        let mut c = Confetti::new().count(3);
        laid_out(&mut c);
        c.burst(0.5, 0.1);
        let y0 = c.particles[0].pos.y;
        c.tick(Duration::from_millis(100));
        assert!(c.particles[0].pos.y != y0); // moved
                                             // All die within ~2.5 s.
        for _ in 0..30 {
            c.tick(Duration::from_millis(100));
        }
        assert_eq!(c.particle_count(), 0);
        assert!(c.take_done());
        assert!(!c.take_done());
    }

    #[test]
    fn empty_tick_idles() {
        let mut c = Confetti::new();
        laid_out(&mut c);
        assert!(!c.tick(Duration::from_millis(16)));
    }

    #[test]
    fn paint_without_painter() {
        let mut c = Confetti::new().count(4);
        laid_out(&mut c);
        c.burst(0.5, 0.3);
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
