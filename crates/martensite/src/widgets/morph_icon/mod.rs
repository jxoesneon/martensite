//! A stroke-icon widget that morphs between arbitrary icon shapes with
//! spring physics (ADR-0041 — a port of morphicons v1.7.1's core).
//!
//! Icons enter as SVG `d` strings ([`kurbo::BezPath::from_svg`]) or
//! `BezPath`s; the private `engine` module lowers them to arc-length–sampled
//! point sets, solves a Procrustes correspondence between the pair, and
//! interpolates the similarity in polar space — rotation and scale ride
//! the spring rather than dissolving point-to-point. Strokes only:
//! filled icons are documented as out of contract.
//!
//! ```no_run
//! use martensite::widgets::MorphIcon;
//!
//! // hamburger → close
//! let icon = MorphIcon::icon("M4 7h16M4 12h16M4 17h16").unwrap();
//! ```

// Engine port and widget body are private; only the widget surface
// (plus `MorphError`, required on method signatures) is public.
mod engine;

pub use engine::MorphError;

use std::time::Duration;

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::widget::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Widget,
};
use martensite_core::Rect;
use martensite_motion::{SpringConfig, SpringSolver};
use martensite_theme::TokenKey;

use engine::{
    bezpath_to_sampled, build_plan, d_to_sampled, sampled_to_bezpaths, MorphPlan, Sampled,
};

/// Default stroke width in logical points — matches the 2px-at-24px
/// convention of lucide/feather/tabler stroke icon sets.
const STROKE_PT: f32 = 2.0;
/// Default square extent in logical points — the 24px icon grid.
const SIZE_PT: f32 = 24.0;

/// Canonical demo icon pairs — real `d` strings for doctests, the
/// dashboard, and parity fixtures. Not a vendored icon set (ADR-0041):
/// apps supply their own paths in production.
#[doc(hidden)]
pub mod demo;

/// A stroke-icon widget that morphs between arbitrary `d`-string icons
/// under a [`SpringConfig`] (ADR-0041).
///
/// Stroke geometry only — the icon is painted as stroked polylines at
/// `STROKE_PT`-scaled width over a square `SIZE_PT`-defaulted
/// extent; filled `d` input paints the outline of the fill path, which
/// is almost never what the caller meant.
///
/// The widget is a leaf: `event` ignores input (a `MorphIcon` inside a
/// `Button` borrows the parent's semantics), `accessibility` emits
/// `Role::Image` + the semantic [`label`](Self::set_label) of the
/// *target* state — never a mid-flight description.
pub struct MorphIcon {
    /// Rest shape — sampled subpaths of the icon shown when `plan`
    /// is `None` (and the morph's `to` set while in flight).
    current: Vec<Sampled>,
    /// Active morph plan, dropped when the spring settles.
    plan: Option<MorphPlan>,
    /// Progress driver: position 0→1, may overshoot (the interpolant
    /// extrapolates — that is the spring's character).
    spring: SpringSolver,
    /// Semantic label for the current (target) state — a11y name.
    label: String,
    /// `true` → `Role::Image` hidden from AT (pure decoration).
    decorative: bool,
    /// Icon square extent in logical points.
    size_pt: f32,
    /// Stroke width in logical points.
    stroke_pt: f32,
    /// Optional explicit ink override (else theme foreground).
    ink: Option<[u8; 4]>,
    /// Pushed reduced-motion flag — `morph_to` snaps when set.
    reduced_motion: bool,
    bounds: Rect,
    scale: f32,
}

impl Default for MorphIcon {
    fn default() -> Self {
        Self::new()
    }
}

impl MorphIcon {
    /// An empty icon — paints nothing until `set_icon`/`morph_to`.
    pub fn new() -> Self {
        Self {
            current: Vec::new(),
            plan: None,
            spring: SpringSolver::new(SpringConfig::CRITICAL, 1.0, 1.0, 0.0),
            label: String::new(),
            decorative: false,
            size_pt: SIZE_PT,
            stroke_pt: STROKE_PT,
            ink: None,
            reduced_motion: false,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Builds an icon resting on `d` (SVG path data). Returns
    /// [`MorphError`] — never a panicking parse — on malformed,
    /// oversized, or non-finite input.
    pub fn icon(d: &str) -> Result<Self, MorphError> {
        let mut s = Self::new();
        s.set_icon(d)?;
        Ok(s)
    }

    /// The semantic name announced for the current icon state
    /// (`"Pause"`, `"Muted"` — the *meaning*, not the shape).
    #[must_use]
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Mutable label setter — updates the a11y name on state
    /// transitions only; the mid-flight shape is never announced.
    pub fn set_label(&mut self, label: impl Into<String>) {
        self.label = label.into();
    }

    /// Marks the icon decorative — hidden from the accessibility
    /// tree (the surrounding control owns the name).
    #[must_use]
    pub fn decorative(mut self, decorative: bool) -> Self {
        self.decorative = decorative;
        self
    }

    /// Square extent in logical points (default 24).
    #[must_use]
    pub fn size(mut self, size_pt: f32) -> Self {
        self.size_pt = size_pt.max(0.0);
        self
    }

    /// Stroke width in logical points (default 2).
    #[must_use]
    pub fn stroke_width(mut self, stroke_pt: f32) -> Self {
        self.stroke_pt = stroke_pt.max(0.0);
        self
    }

    /// Explicit ink override (else the theme foreground).
    #[must_use]
    pub fn ink(mut self, rgba: [u8; 4]) -> Self {
        self.ink = Some(rgba);
        self
    }

    /// Jumps straight to `d` — no flight.
    pub fn set_icon(&mut self, d: &str) -> Result<(), MorphError> {
        self.current = d_to_sampled(d)?;
        self.plan = None;
        Ok(())
    }

    /// Morphs toward `d` under `spring`. Called mid-flight, the
    /// *current interpolated shape* becomes the new origin (clean
    /// re-entry — the upstream `morphTo` interrupt contract), and the
    /// spring restarts with its velocity carried.
    ///
    /// Under the reduced-motion push this behaves as `set_icon`.
    pub fn morph_to(&mut self, d: &str, spring: SpringConfig) -> Result<(), MorphError> {
        let target = d_to_sampled(d)?;
        if self.reduced_motion {
            self.current = target;
            self.plan = None;
            return Ok(());
        }
        let from = match &self.plan {
            Some(plan) if !self.spring.settle_threshold() => {
                plan.eval(f64::from(self.spring.position()))
            }
            _ => self.current.clone(),
        };
        self.plan = Some(build_plan(&from, &target)?);
        let v = self.spring.velocity().clamp(-14.0, 14.0);
        self.spring = SpringSolver::new(spring, 0.0, 1.0, v);
        self.current = target;
        Ok(())
    }

    /// Morphs toward a `BezPath` (non-`d` input).
    pub fn morph_to_path(
        &mut self,
        path: &kurbo::BezPath,
        spring: SpringConfig,
    ) -> Result<(), MorphError> {
        let target = bezpath_to_sampled(path)?;
        if self.reduced_motion {
            self.current = target;
            self.plan = None;
            return Ok(());
        }
        let from = match &self.plan {
            Some(plan) if !self.spring.settle_threshold() => {
                plan.eval(f64::from(self.spring.position()))
            }
            _ => self.current.clone(),
        };
        self.plan = Some(build_plan(&from, &target)?);
        let v = self.spring.velocity().clamp(-14.0, 14.0);
        self.spring = SpringSolver::new(spring, 0.0, 1.0, v);
        self.current = target;
        Ok(())
    }

    /// Controlled mode (ADR-0041 `seek`): freezes the active plan at
    /// progress `t` — deterministic frames for tests and scrub UI.
    /// `t` outside `[0, 1]` extrapolates (same math as overshoot).
    pub fn seek(&mut self, t: f32) {
        if self.plan.is_some() {
            // A frozen spring at position t: elapsed 0, initial = t.
            self.spring = SpringSolver::new(SpringConfig::CRITICAL, t, t, 0.0);
        }
    }

    /// `true` while a morph is in flight.
    pub fn is_animating(&self) -> bool {
        self.plan.is_some() && !self.spring.settle_threshold()
    }

    /// The spring's current progress (extrapolated past 1.0 on
    /// overshoot); `1.0` at rest.
    pub fn progress(&self) -> f32 {
        if self.is_animating() {
            self.spring.position()
        } else {
            1.0
        }
    }

    /// Stroke paths to paint this frame — polyline `BezPath`s in the
    /// icon's own `d`-space coordinates (usually the 24px grid). A
    /// frozen (`seek`-ed) spring still evaluates the plan at its
    /// parked position; a settled-and-dropped plan falls back to the
    /// canonical rest shape.
    fn frame_paths(&self) -> Vec<kurbo::BezPath> {
        match &self.plan {
            Some(plan) => plan.eval_paths(f64::from(self.spring.position())),
            None => sampled_to_bezpaths(&self.current),
        }
    }
}

impl Widget for MorphIcon {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let s = cx.pt(self.size_pt);
        Vec2::new(
            s.min(constraints.max_size.x.max(0.0)),
            s.min(constraints.max_size.y.max(0.0)),
        )
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
    }

    fn tick(&mut self, dt: Duration) -> bool {
        if !self.is_animating() {
            return false;
        }
        self.spring.advance(dt.as_secs_f32());
        if self.spring.settle_threshold() {
            // Settled: drop the plan and let the canonical rest shape
            // take over (exact endpoint fidelity).
            self.plan = None;
            return true;
        }
        true
    }

    fn tick_paint_only(&self) -> bool {
        // Mid-flight frames are pure geometry — the a11y name already
        // reports the target state since `morph_to`.
        true
    }

    fn set_reduced_motion(&mut self, reduced: bool) {
        self.reduced_motion = reduced;
    }

    fn event(&mut self, _cx: &mut EventContext) -> EventResponse {
        EventResponse::Ignored
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        if self.decorative {
            node.set_hidden();
            return;
        }
        node.set_role(accesskit::Role::Image);
        if !self.label.is_empty() {
            node.set_label(self.label.clone());
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        if self.bounds.width() <= 0.0 || self.bounds.height() <= 0.0 {
            return;
        }
        // Icon space (the `d` coordinates, conventionally a 24px grid)
        // is scaled+translated to fill `bounds`; stroke width rides
        // the same scale so a 2pt-at-24 icon stays proportional.
        let ink = self
            .ink
            .unwrap_or_else(|| cx.color(TokenKey::TextColor, [230, 230, 235, 255]));
        let (bx, by, bw, bh) = (
            self.bounds.min_x(),
            self.bounds.min_y(),
            self.bounds.width(),
            self.bounds.height(),
        );
        let icon_extent = 24.0f64; // upstream grid convention
        let sc = f64::from(bw.min(bh)) / icon_extent;
        let ox = f64::from(bx) + (f64::from(bw) - icon_extent * sc) * 0.5;
        let oy = f64::from(by) + (f64::from(bh) - icon_extent * sc) * 0.5;
        let xform = kurbo::Affine::translate((ox, oy)) * kurbo::Affine::scale(sc);
        let width_px = cx.pt(self.stroke_pt) * sc as f32;
        for path in self.frame_paths() {
            cx.list
                .push_stroke_path(xform * path, width_px.max(0.5), ink);
        }
    }

    fn debug_name(&self) -> &'static str {
        "MorphIcon"
    }
}

#[cfg(test)]
mod tests;
