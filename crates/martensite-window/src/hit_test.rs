//! Two-stage widget hit-testing with affine transforms and non-rectangular clips.
//!
//! Hit-testing resolves which single widget sits directly beneath a given
//! screen-space point. It is the entry point for all pointer input routing:
//! mouse moves, clicks, drags, and hover tracking all begin by asking "which
//! widget is at `(x, y)`?".
//!
//! # Two-stage pipeline
//!
//! Evaluation proceeds in **reverse Z-order** (topmost widget first) so that
//! the visually front-most widget wins. For every candidate widget the tester
//! runs a two-stage pipeline:
//!
//! 1. **Broad-phase (AABB rejection):** the screen point is tested against the
//!    widget's axis-aligned [`HotNode::bounds`] rectangle. Widgets whose bounds
//!    do not contain the point are rejected in O(1) without any matrix math.
//! 2. **Narrow-phase (local-space verification):** if the widget owns an
//!    [`AffineTransform`], the screen point is inverse-transformed into the
//!    widget's local coordinate space and re-tested against the local
//!    `[0, size]` rectangle. This correctly rejects points that fall inside
//!    the screen-space AABB of a rotated/scaled widget but outside its actual
//!    (non-axis-aligned) footprint.
//!
//! # Singular matrix guard
//!
//! A degenerate transform — zero scale, edge-on shear, or any matrix with
//! `|det(M)| < 1e-6` — has no inverse. Rather than producing `NaN`/infinity by
//! dividing by (near) zero, the tester detects singularity via
//! [`AffineTransform::is_singular`] and safely reports *no hit* for that
//! widget. Any computed coordinate that is `NaN` or infinite is likewise
//! rejected so poisoned values never escape the tester.
//!
//! # Non-rectangular clips
//!
//! Some widgets are not rectangular: rounded rectangles, circles approximated
//! by paths, or arbitrary vector outlines. [`HitTester::hit_test_with_clip`]
//! accepts a per-widget [`ClipShape`] map and applies a winding-number /
//! corner-radius verification after the AABB broad-phase so points sitting in
//! the "cut off" corner of a rounded rectangle or outside a custom path are
//! correctly rejected. Path clips use the **non-zero winding-number** rule:
//! a point is inside when the signed sum of edge crossings of a ray cast from
//! the point is non-zero, which (unlike the even-odd rule) correctly fills
//! self-intersecting regions that wind around the point.
//!
//! # Boundary convention
//!
//! A point lying exactly on the **min** edge (left/top) of a rectangle is
//! considered *inside* (inclusive); a point on the **max** edge (right/bottom)
//! is considered *outside* (exclusive). This half-open convention avoids
//! double-counting adjacent tiles and matches the common `[min, max)` interval
//! semantics used throughout layout.
//!
//! # Example
//!
//! ```
//! use martensite_core::{WidgetArena, HotNode, NodeFlags, Rect};
//! use martensite_window::hit_test::{HitTester, HitTestResult};
//! use glam::Vec2;
//!
//! let mut arena = WidgetArena::new();
//! let root = arena.insert(
//!     HotNode {
//!         bounds: Rect::new(0.0, 0.0, 100.0, 100.0),
//!         flags: NodeFlags::VISIBLE | NodeFlags::HIT_TEST_ENABLED,
//!         ..HotNode::default()
//!     },
//!     martensite_core::ColdNode::default(),
//! );
//!
//! let tester = HitTester::new(&arena);
//! let hit = tester.hit_test(root, Vec2::new(50.0, 50.0));
//! assert_eq!(hit.map(|h| h.widget_id), Some(root));
//! ```
//!
//! [`HotNode::bounds`]: martensite_core::HotNode::bounds

use std::collections::HashMap;

use glam::{Affine2, Vec2};
use martensite_core::{HotNode, NodeFlags, Rect, WidgetArena, WidgetId};

/// Absolute tolerance below which a 2D affine transform is treated as singular
/// (non-invertible). Equal to `1e-6` as specified by the v0.5.0 milestone.
pub const SINGULAR_EPSILON: f32 = 1e-6;

/// The outcome of a successful hit-test: the front-most widget beneath the
/// pointer, plus the pointer location expressed in that widget's *local*
/// coordinate space.
///
/// `local_point` is the screen point after inverse-transforming by the
/// widget's [`AffineTransform`] (or unchanged if the widget has no transform).
/// Renderers and event handlers can use it directly to answer questions like
/// "which glyph did the user click?" without re-deriving the inverse.
///
/// # Examples
///
/// ```
/// use martensite_core::{ColdNode, HotNode, NodeFlags, Rect, WidgetArena};
/// use martensite_window::hit_test::{HitTester, HitTestResult};
/// use glam::Vec2;
///
/// let mut arena = WidgetArena::new();
/// let root = arena.insert(
///     HotNode {
///         bounds: Rect::new(10.0, 10.0, 110.0, 110.0),
///         flags: NodeFlags::VISIBLE | NodeFlags::HIT_TEST_ENABLED,
///         ..HotNode::default()
///     },
///     ColdNode::default(),
/// );
///
/// let tester = HitTester::new(&arena);
/// let result: HitTestResult = tester
///     .hit_test(root, Vec2::new(40.0, 40.0))
///     .expect("point is inside the root");
/// assert_eq!(result.widget_id, root);
/// // With no transform applied, the local point equals the screen point.
/// assert_eq!(result.local_point, Vec2::new(40.0, 40.0));
/// ```
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct HitTestResult {
    /// The widget handle that the pointer is hovering over.
    pub widget_id: WidgetId,
    /// The pointer position in the hit widget's local coordinate space.
    pub local_point: Vec2,
}

/// A 2D affine transform wrapping [`glam::Affine2`].
///
/// This type maps widget-local points to screen space via
/// [`AffineTransform::transform_point`]. The hit-tester uses the inverse
/// (computed via [`AffineTransform::inverse`]) to map screen points back into
/// local space for the narrow-phase verification.
///
/// The linear 2×2 part's determinant drives the singular-matrix guard: when
/// `|det| < [`SINGULAR_EPSILON`]` the transform is treated as non-invertible
/// and [`AffineTransform::inverse`] returns `None`, so the tester safely
/// reports no hit instead of dividing by (near) zero.
///
/// # Example
///
/// ```
/// use martensite_window::hit_test::AffineTransform;
/// use glam::Vec2;
///
/// let t = AffineTransform::from_translation(Vec2::new(10.0, 20.0));
/// assert_eq!(t.transform_point(Vec2::new(1.0, 2.0)), Vec2::new(11.0, 22.0));
/// assert!(t.inverse().is_some());
///
/// let degenerate = AffineTransform::from_scale(Vec2::new(0.0, 0.0));
/// assert!(degenerate.is_singular());
/// assert!(degenerate.inverse().is_none());
/// ```
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct AffineTransform {
    /// The underlying glam affine transform (local → screen).
    inner: Affine2,
}

impl AffineTransform {
    /// The identity transform — points map to themselves.
    pub const IDENTITY: Self = Self {
        inner: Affine2::IDENTITY,
    };

    /// Wrap a raw [`glam::Affine2`].
    #[inline]
    #[must_use]
    pub const fn from_affine(inner: Affine2) -> Self {
        Self { inner }
    }

    /// A pure translation transform.
    #[inline]
    #[must_use]
    pub fn from_translation(translation: Vec2) -> Self {
        Self {
            inner: Affine2::from_translation(translation),
        }
    }

    /// A uniform/non-uniform scale transform.
    ///
    /// A scale of zero in either axis produces a singular (non-invertible)
    /// transform — see [`AffineTransform::is_singular`].
    #[inline]
    #[must_use]
    pub fn from_scale(scale: Vec2) -> Self {
        Self {
            inner: Affine2::from_scale(scale),
        }
    }

    /// A counter-clockwise rotation (in radians) about the origin.
    #[inline]
    #[must_use]
    pub fn from_angle(angle: f32) -> Self {
        Self {
            inner: Affine2::from_angle(angle),
        }
    }

    /// A combined scale → rotate → translate transform (local → screen).
    #[inline]
    #[must_use]
    pub fn from_scale_angle_translation(scale: Vec2, angle: f32, translation: Vec2) -> Self {
        Self {
            inner: Affine2::from_scale_angle_translation(scale, angle, translation),
        }
    }

    /// Transform a local-space point into screen space.
    #[inline]
    #[must_use]
    pub fn transform_point(&self, p: Vec2) -> Vec2 {
        self.inner.transform_point2(p)
    }

    /// The determinant of the 2×2 linear part of this transform.
    #[inline]
    #[must_use]
    pub fn determinant(&self) -> f32 {
        self.inner.matrix2.determinant()
    }

    /// Returns `true` when the transform is (numerically) non-invertible.
    ///
    /// A transform is singular when `|det| < [`SINGULAR_EPSILON`]` or when the
    /// determinant itself is `NaN`/infinite (which can arise from overflow or
    /// poisoned inputs). Singular transforms have no meaningful inverse.
    #[inline]
    #[must_use]
    pub fn is_singular(&self) -> bool {
        let det = self.determinant();
        !det.is_finite() || det.abs() < SINGULAR_EPSILON
    }

    /// Returns the inverse transform (screen → local), or `None` if singular.
    ///
    /// Singularity is checked *before* inverting so that a degenerate matrix
    /// never produces `NaN`/infinity via glam's `inverse` implementation.
    #[inline]
    #[must_use]
    pub fn inverse(&self) -> Option<AffineTransform> {
        if self.is_singular() {
            None
        } else {
            Some(AffineTransform {
                inner: self.inner.inverse(),
            })
        }
    }

    /// Access the underlying [`glam::Affine2`].
    #[inline]
    #[must_use]
    pub const fn as_affine(&self) -> Affine2 {
        self.inner
    }
}

impl Default for AffineTransform {
    #[inline]
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl From<Affine2> for AffineTransform {
    #[inline]
    fn from(inner: Affine2) -> Self {
        Self::from_affine(inner)
    }
}

/// A rounded rectangle used as a non-rectangular clip shape.
///
/// `radius` is the corner radius applied symmetrically to all four corners.
/// A `radius` of `0.0` is equivalent to a plain [`Rect`].
///
/// # Examples
///
/// ```
/// use martensite_core::Rect;
/// use martensite_window::hit_test::{point_in_shape, ClipShape, RoundedRect};
/// use glam::Vec2;
///
/// let rr = RoundedRect::new(Rect::new(0.0, 0.0, 20.0, 20.0), 5.0);
/// // A radius of 0 behaves like a plain rectangle.
/// let plain = RoundedRect::new(Rect::new(0.0, 0.0, 20.0, 20.0), 0.0);
/// assert!(point_in_shape(&ClipShape::RoundedRect(plain), Vec2::new(1.0, 1.0)));
/// // With a 5px radius, the corner (1, 1) is cut off.
/// assert!(!point_in_shape(&ClipShape::RoundedRect(rr), Vec2::new(1.0, 1.0)));
/// ```
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct RoundedRect {
    /// The outer axis-aligned rectangle.
    pub rect: Rect,
    /// The corner radius in logical pixels. Clamped to half the shorter side
    /// when evaluating [`point_in_shape`].
    pub radius: f32,
}

impl RoundedRect {
    /// Create a new rounded rectangle from a bounding rect and corner radius.
    #[inline]
    #[must_use]
    pub fn new(rect: Rect, radius: f32) -> Self {
        Self { rect, radius }
    }
}

/// A non-rectangular clipping region applied during hit-testing.
///
/// Passed to [`HitTester::hit_test_with_clip`] as part of a per-widget map so
/// that widgets with rounded corners or custom vector outlines can reject
/// points that pass the AABB broad-phase but fall outside the true shape.
///
/// # Examples
///
/// ```
/// use martensite_core::Rect;
/// use martensite_window::hit_test::{point_in_shape, ClipShape, RoundedRect};
/// use glam::Vec2;
///
/// // A plain rectangle: the center is inside, a far-away point is not.
/// let rect = ClipShape::Rect(Rect::new(0.0, 0.0, 10.0, 10.0));
/// assert!(point_in_shape(&rect, Vec2::new(5.0, 5.0)));
/// assert!(!point_in_shape(&rect, Vec2::new(20.0, 20.0)));
///
/// // A rounded rectangle rejects points in the cut-off corners.
/// let rounded = ClipShape::RoundedRect(RoundedRect::new(
///     Rect::new(0.0, 0.0, 10.0, 10.0),
///     4.0,
/// ));
/// // The corner (1, 1) sits outside the 4px corner circle.
/// assert!(!point_in_shape(&rounded, Vec2::new(1.0, 1.0)));
/// // The center is still inside.
/// assert!(point_in_shape(&rounded, Vec2::new(5.0, 5.0)));
/// ```
#[derive(Clone, Debug, PartialEq)]
pub enum ClipShape {
    /// A plain axis-aligned rectangle.
    Rect(Rect),
    /// A rectangle with rounded corners.
    RoundedRect(RoundedRect),
    /// A closed polygonal path (list of vertices in order). Evaluated with the
    /// non-zero winding-number rule.
    Path(Vec<Vec2>),
}

/// Returns `true` when `point` lies inside the clipping region `shape`.
///
/// - [`ClipShape::Rect`] uses the same half-open `[min, max)` AABB test as the
///   broad-phase.
/// - [`ClipShape::RoundedRect`] first applies the AABB test, then rejects
///   points that sit in one of the four corner squares but outside the
///   inscribed corner circle of the given radius.
/// - [`ClipShape::Path`] uses the non-zero winding-number rule: a horizontal
///   ray cast from the point sums signed edge crossings (clockwise = +1,
///   counterclockwise = -1); a non-zero sum means *inside*. This correctly
///   fills self-intersecting regions that wind around the point, unlike the
///   even-odd rule.
///
/// Non-finite (`NaN`/infinite) points always return `false`.
#[must_use]
pub fn point_in_shape(shape: &ClipShape, point: Vec2) -> bool {
    if !point.is_finite() {
        return false;
    }
    match shape {
        ClipShape::Rect(rect) => point_in_rect(point, *rect),
        ClipShape::RoundedRect(rr) => point_in_rounded_rect(point, *rr),
        ClipShape::Path(pts) => point_in_path(point, pts),
    }
}

/// Half-open AABB containment test: `[min_x, max_x) × [min_y, max_y)`.
///
/// The min edges are *inclusive* and the max edges are *exclusive*, matching
/// the documented boundary convention. Non-finite points return `false`.
#[inline]
#[must_use]
pub fn point_in_rect(point: Vec2, rect: Rect) -> bool {
    if !point.is_finite() {
        return false;
    }
    point.x >= rect.min_x()
        && point.x < rect.max_x()
        && point.y >= rect.min_y()
        && point.y < rect.max_y()
}

/// Rounded-rectangle containment: AABB plus corner-circle rejection.
///
/// # Boundary convention
///
/// The axis-aligned bounding-box portion of this test uses the half-open
/// `[min, max)` semantics inherited from [`point_in_rect`]: the left/top edges
/// are inclusive and the right/bottom edges are exclusive. The corner-circle
/// test, however, uses `<=` (inclusive): a point lying *exactly* on a corner
/// circle's boundary is considered inside. This deliberate asymmetry matches
/// the intuition that a rounded outline is a smooth, closed curve — a point
/// resting precisely on the arc should be treated as on the shape — while the
/// rectangular straight edges keep the tile-friendly half-open convention used
/// throughout layout to avoid double-counting adjacent widgets.
fn point_in_rounded_rect(point: Vec2, rr: RoundedRect) -> bool {
    let rect = rr.rect;
    if !point_in_rect(point, rect) {
        return false;
    }
    // A zero (or negative) radius degenerates to a plain rectangle.
    if rr.radius <= 0.0 {
        return true;
    }
    // Clamp the radius to half the shorter side so the corner circles never
    // overlap; this keeps the geometry well-formed for any input.
    let max_radius = rect.width().min(rect.height()) * 0.5;
    let r = rr.radius.min(max_radius.max(0.0));
    if r <= 0.0 {
        return true;
    }

    // Identify which corner quadrant the point lies in, if any, and reject it
    // when it falls outside the inscribed corner circle.
    let left = rect.min_x();
    let right = rect.max_x();
    let top = rect.min_y();
    let bottom = rect.max_y();

    // Corner circle centers, inset by `r` from each corner.
    let cx = if point.x < left + r {
        left + r
    } else if point.x > right - r {
        right - r
    } else {
        return true; // Point is in the central band — no corner to test.
    };
    let cy = if point.y < top + r {
        top + r
    } else if point.y > bottom - r {
        bottom - r
    } else {
        return true; // Point is in the central band — no corner to test.
    };

    let dx = point.x - cx;
    let dy = point.y - cy;
    dx * dx + dy * dy <= r * r
}

/// Non-zero winding-number test for a closed polygonal path.
///
/// Casts a horizontal ray in the +x direction from `point` and sums the
/// *signed* crossings of every polygon edge: an edge crossing upward
/// (increasing y) contributes `+1`, an edge crossing downward (decreasing y)
/// contributes `-1`. The point is inside when the total winding number is
/// non-zero. This is the non-zero winding rule (not the even-odd rule): it
/// correctly fills self-intersecting regions that wind around the point, while
/// regions wound in opposite directions cancel out. Degenerate paths with
/// fewer than 3 vertices contain no area.
fn point_in_path(point: Vec2, pts: &[Vec2]) -> bool {
    if pts.len() < 3 {
        return false;
    }
    let n = pts.len();
    let mut winding = 0i32;
    let mut j = n - 1;
    for i in 0..n {
        let pi = pts[i];
        let pj = pts[j];
        // Only edges that straddle the horizontal ray at `point.y` can cross it.
        if (pi.y <= point.y) != (pj.y <= point.y) {
            // Compute the x-coordinate at which the edge intersects the ray.
            let dy = pj.y - pi.y;
            if dy.abs() > 0.0 {
                let x_intersect = pi.x + ((point.y - pi.y) / dy) * (pj.x - pi.x);
                // The ray travels in +x, so only crossings to the right of the
                // point are counted. Direction is determined by whether the
                // edge goes upward or downward.
                if point.x < x_intersect {
                    if pi.y < pj.y {
                        winding += 1;
                    } else {
                        winding -= 1;
                    }
                }
            }
        }
        j = i;
    }
    winding != 0
}

/// Hit-tester bound to a widget arena.
///
/// Borrows the arena immutably and resolves which widget sits beneath a given
/// screen-space point using the two-stage pipeline described in the module
/// docs. Cheap to construct: the tester is just a borrowed reference plus
/// scratch bookkeeping, so callers may build one per input event without
/// concern.
///
/// # Examples
///
/// ```
/// use martensite_core::{ColdNode, HotNode, NodeFlags, Rect, WidgetArena};
/// use martensite_window::hit_test::HitTester;
/// use glam::Vec2;
///
/// let mut arena = WidgetArena::new();
/// let root = arena.insert(
///     HotNode {
///         bounds: Rect::new(0.0, 0.0, 100.0, 100.0),
///         flags: NodeFlags::VISIBLE | NodeFlags::HIT_TEST_ENABLED,
///         ..HotNode::default()
///     },
///     ColdNode::default(),
/// );
///
/// let tester = HitTester::new(&arena);
/// // A point inside the root hits it.
/// let hit = tester.hit_test(root, Vec2::new(50.0, 50.0));
/// assert_eq!(hit.map(|h| h.widget_id), Some(root));
/// // A point outside misses every widget.
/// assert!(tester.hit_test(root, Vec2::new(200.0, 200.0)).is_none());
/// ```
pub struct HitTester<'a> {
    /// The widget arena being tested.
    arena: &'a WidgetArena,
}

impl<'a> HitTester<'a> {
    /// Create a new hit-tester borrowing the given arena.
    #[inline]
    #[must_use]
    pub fn new(arena: &'a WidgetArena) -> Self {
        Self { arena }
    }

    /// Perform a two-stage hit-test starting from `root` at screen `screen_point`.
    ///
    /// Walks the subtree in reverse Z-order (last child first = topmost) and
    /// returns the first widget whose AABB contains the point. No transforms
    /// or clip shapes are applied — this is the fast path for untransformed,
    /// rectangular widget trees.
    ///
    /// Returns `None` when the point misses every widget, when `root` is dead,
    /// or when `screen_point` is non-finite.
    #[must_use]
    pub fn hit_test(&self, root: WidgetId, screen_point: Vec2) -> Option<HitTestResult> {
        if !screen_point.is_finite() || !self.arena.is_alive(root) {
            return None;
        }
        self.hit_test_node(root, screen_point, None, None)
    }

    /// Two-stage hit-test with per-widget affine transforms.
    ///
    /// Like [`HitTester::hit_test`], but widgets present in `transforms` get
    /// the narrow-phase local-space verification: the screen point is
    /// inverse-transformed into the widget's local space and re-tested against
    /// the local `[0, size]` rectangle. Widgets with singular transforms
    /// (`|det| < [`SINGULAR_EPSILON`]`) are skipped without panicking.
    #[must_use]
    pub fn hit_test_with_transforms(
        &self,
        root: WidgetId,
        screen_point: Vec2,
        transforms: &HashMap<WidgetId, AffineTransform>,
    ) -> Option<HitTestResult> {
        if !screen_point.is_finite() || !self.arena.is_alive(root) {
            return None;
        }
        self.hit_test_node(root, screen_point, Some(transforms), None)
    }

    /// Two-stage hit-test with per-widget non-rectangular clip shapes.
    ///
    /// Widgets present in `clips` get an additional [`point_in_shape`]
    /// verification after the AABB broad-phase so points in cut-off corners
    /// or outside custom paths are rejected.
    #[must_use]
    pub fn hit_test_with_clip(
        &self,
        root: WidgetId,
        screen_point: Vec2,
        clips: &HashMap<WidgetId, ClipShape>,
    ) -> Option<HitTestResult> {
        if !screen_point.is_finite() || !self.arena.is_alive(root) {
            return None;
        }
        self.hit_test_node(root, screen_point, None, Some(clips))
    }

    /// Full two-stage hit-test with both transforms and clip shapes.
    ///
    /// Combines [`HitTester::hit_test_with_transforms`] and
    /// [`HitTester::hit_test_with_clip`]: the narrow-phase inverse transform
    /// runs first, then the clip shape is evaluated in *local* space.
    #[must_use]
    pub fn hit_test_full(
        &self,
        root: WidgetId,
        screen_point: Vec2,
        transforms: &HashMap<WidgetId, AffineTransform>,
        clips: &HashMap<WidgetId, ClipShape>,
    ) -> Option<HitTestResult> {
        if !screen_point.is_finite() || !self.arena.is_alive(root) {
            return None;
        }
        self.hit_test_node(root, screen_point, Some(transforms), Some(clips))
    }

    /// Recursive reverse Z-order traversal core shared by all entry points.
    ///
    /// Children are visited first (last child = topmost first), then the node
    /// itself is tested. The first hit wins.
    #[allow(clippy::too_many_lines)]
    fn hit_test_node(
        &self,
        node: WidgetId,
        screen_point: Vec2,
        transforms: Option<&HashMap<WidgetId, AffineTransform>>,
        clips: Option<&HashMap<WidgetId, ClipShape>>,
    ) -> Option<HitTestResult> {
        let hot: &HotNode = self.arena.get_hot(node)?;

        // Invisible widgets hide their entire subtree.
        if !hot.flags.contains(NodeFlags::VISIBLE) {
            return None;
        }

        // Recurse into children in reverse Z-order (topmost sibling first).
        // The arena stores children as a singly-linked list with `first_child`
        // pointing at the *earliest* (bottom-most) sibling, so iterating via
        // `Children::rev()` yields the last child first.
        let mut child = self.arena.last_child(node);
        while let Some(child_id) = child {
            if let Some(hit) = self.hit_test_node(child_id, screen_point, transforms, clips) {
                return Some(hit);
            }
            child = self.arena.prev_sibling(child_id);
        }

        // Test the node itself only if it opted into hit-testing and is not inert.
        let flags = hot.flags;
        if !flags.contains(NodeFlags::HIT_TEST_ENABLED) || flags.contains(NodeFlags::INERT) {
            return None;
        }

        // Stage 1 — broad-phase AABB rejection using screen-space bounds.
        if !point_in_rect(screen_point, hot.bounds) {
            return None;
        }

        // Stage 2 — narrow-phase local-space verification via inverse transform.
        let local_point = if let Some(map) = transforms {
            if let Some(transform) = map.get(&node) {
                // Singular matrix guard: skip without panicking or producing NaN.
                let inverse = transform.inverse()?;
                let local = inverse.transform_point(screen_point);
                // NaN / infinity guard: poisoned coordinates never escape.
                if !local.is_finite() {
                    return None;
                }
                // Re-test against the local [0, size] rectangle.
                let local_rect = Rect::new(0.0, 0.0, hot.bounds.width(), hot.bounds.height());
                if !point_in_rect(local, local_rect) {
                    return None;
                }
                local
            } else {
                screen_point
            }
        } else {
            screen_point
        };

        // Non-rectangular clip verification (evaluated in local space).
        if let Some(map) = clips {
            if let Some(shape) = map.get(&node) {
                if !point_in_shape(shape, local_point) {
                    return None;
                }
            }
        }

        Some(HitTestResult {
            widget_id: node,
            local_point,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::{ColdNode, HotNode, NodeFlags, Rect, WidgetArena};
    use std::f32::consts::{FRAC_PI_2, FRAC_PI_4};
    use std::panic::AssertUnwindSafe;

    /// Build a visible, hit-testable node with the given bounds.
    fn hot_node(x: f32, y: f32, w: f32, h: f32) -> HotNode {
        HotNode {
            bounds: Rect::new(x, y, w, h),
            flags: NodeFlags::VISIBLE | NodeFlags::HIT_TEST_ENABLED,
            ..HotNode::default()
        }
    }

    /// Insert a node and return its id.
    fn insert(arena: &mut WidgetArena, x: f32, y: f32, w: f32, h: f32) -> WidgetId {
        arena.insert(hot_node(x, y, w, h), ColdNode::default())
    }

    #[test]
    fn basic_aabb_hit_inside() {
        let mut arena = WidgetArena::new();
        let root = insert(&mut arena, 0.0, 0.0, 100.0, 100.0);
        let tester = HitTester::new(&arena);
        let hit = tester.hit_test(root, Vec2::new(50.0, 50.0));
        assert_eq!(hit.map(|h| h.widget_id), Some(root));
        assert_eq!(hit.unwrap().local_point, Vec2::new(50.0, 50.0));
    }

    #[test]
    fn basic_aabb_hit_outside() {
        let mut arena = WidgetArena::new();
        let root = insert(&mut arena, 0.0, 0.0, 100.0, 100.0);
        let tester = HitTester::new(&arena);
        assert!(tester.hit_test(root, Vec2::new(150.0, 150.0)).is_none());
    }

    #[test]
    fn nested_child_wins_over_parent() {
        let mut arena = WidgetArena::new();
        let parent = insert(&mut arena, 0.0, 0.0, 200.0, 200.0);
        let child = insert(&mut arena, 50.0, 50.0, 100.0, 100.0);
        arena.append_child(parent, child).unwrap();
        let tester = HitTester::new(&arena);
        // Point inside both parent and child → child (topmost) wins.
        let hit = tester.hit_test(parent, Vec2::new(75.0, 75.0));
        assert_eq!(hit.map(|h| h.widget_id), Some(child));
        // Point inside parent only → parent wins.
        let hit = tester.hit_test(parent, Vec2::new(10.0, 10.0));
        assert_eq!(hit.map(|h| h.widget_id), Some(parent));
    }

    #[test]
    fn reverse_z_order_topmost_sibling_wins() {
        let mut arena = WidgetArena::new();
        let root = insert(&mut arena, 0.0, 0.0, 200.0, 200.0);
        let bottom = insert(&mut arena, 0.0, 0.0, 100.0, 100.0);
        let top = insert(&mut arena, 0.0, 0.0, 100.0, 100.0);
        // Append bottom first, then top → top is the last child (topmost).
        arena.append_child(root, bottom).unwrap();
        arena.append_child(root, top).unwrap();
        let tester = HitTester::new(&arena);
        let hit = tester.hit_test(root, Vec2::new(50.0, 50.0));
        assert_eq!(hit.map(|h| h.widget_id), Some(top));
    }

    #[test]
    fn transformed_translation() {
        let mut arena = WidgetArena::new();
        // Widget's screen-space bounds are at (100,100) due to a translation,
        // but its local rect is [0,100]x[0,100].
        let root = insert(&mut arena, 100.0, 100.0, 100.0, 100.0);
        let mut transforms = HashMap::new();
        transforms.insert(
            root,
            AffineTransform::from_translation(Vec2::new(100.0, 100.0)),
        );
        let tester = HitTester::new(&arena);
        // Screen point (150,150) → local (50,50) → hit.
        let hit = tester.hit_test_with_transforms(root, Vec2::new(150.0, 150.0), &transforms);
        assert_eq!(hit.map(|h| h.widget_id), Some(root));
        assert_eq!(hit.unwrap().local_point, Vec2::new(50.0, 50.0));
        // Screen point (50,50) is outside the screen-space AABB → miss.
        assert!(tester
            .hit_test_with_transforms(root, Vec2::new(50.0, 50.0), &transforms)
            .is_none());
    }

    #[test]
    fn transformed_rotation_90_degrees() {
        let mut arena = WidgetArena::new();
        // A 100x100 widget rotated 90° about its top-left corner maps local
        // (x,y) → screen (-y, x). Its screen-space AABB (ignoring sign for the
        // test) is set so the broad-phase passes for the chosen point.
        let root = insert(&mut arena, -100.0, 0.0, 100.0, 100.0);
        let mut transforms = HashMap::new();
        transforms.insert(root, AffineTransform::from_angle(FRAC_PI_2));
        let tester = HitTester::new(&arena);
        // Local (50,50) → screen (-50, 50). The AABB [-100,0]x[0,100] contains it.
        let hit = tester.hit_test_with_transforms(root, Vec2::new(-50.0, 50.0), &transforms);
        assert_eq!(hit.map(|h| h.widget_id), Some(root));
        let local = hit.unwrap().local_point;
        assert!((local.x - 50.0).abs() < 1e-3);
        assert!((local.y - 50.0).abs() < 1e-3);
    }

    #[test]
    fn transformed_scale_2x() {
        let mut arena = WidgetArena::new();
        // Local [0,100]x[0,100] scaled 2x → screen [0,200]x[0,200].
        let root = insert(&mut arena, 0.0, 0.0, 200.0, 200.0);
        let mut transforms = HashMap::new();
        transforms.insert(root, AffineTransform::from_scale(Vec2::new(2.0, 2.0)));
        let tester = HitTester::new(&arena);
        // Screen (150,150) → local (75,75) → hit.
        let hit = tester.hit_test_with_transforms(root, Vec2::new(150.0, 150.0), &transforms);
        assert_eq!(hit.map(|h| h.widget_id), Some(root));
        assert_eq!(hit.unwrap().local_point, Vec2::new(75.0, 75.0));
    }

    #[test]
    fn singular_zero_scale_returns_no_hit_without_panic() {
        let mut arena = WidgetArena::new();
        let root = insert(&mut arena, 0.0, 0.0, 100.0, 100.0);
        let mut transforms = HashMap::new();
        transforms.insert(root, AffineTransform::from_scale(Vec2::new(0.0, 0.0)));
        let tester = HitTester::new(&arena);
        assert!(transforms.get(&root).unwrap().is_singular());
        // Must not panic; must return None.
        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            tester.hit_test_with_transforms(root, Vec2::new(50.0, 50.0), &transforms)
        }));
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn singular_edge_on_scale_returns_no_hit() {
        let mut arena = WidgetArena::new();
        let root = insert(&mut arena, 0.0, 0.0, 100.0, 100.0);
        let mut transforms = HashMap::new();
        // Near-zero scale in x → singular.
        transforms.insert(root, AffineTransform::from_scale(Vec2::new(1e-8, 1.0)));
        let tester = HitTester::new(&arena);
        assert!(transforms.get(&root).unwrap().is_singular());
        assert!(tester
            .hit_test_with_transforms(root, Vec2::new(50.0, 50.0), &transforms)
            .is_none());
    }

    #[test]
    fn nan_input_does_not_panic() {
        let mut arena = WidgetArena::new();
        let root = insert(&mut arena, 0.0, 0.0, 100.0, 100.0);
        let tester = HitTester::new(&arena);
        let nan = Vec2::new(f32::NAN, 50.0);
        let result = std::panic::catch_unwind(AssertUnwindSafe(|| tester.hit_test(root, nan)));
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
        let inf = Vec2::new(f32::INFINITY, 50.0);
        assert!(tester.hit_test(root, inf).is_none());
    }

    #[test]
    fn nan_inf_affine_matrix_components_are_singular() {
        // An AffineTransform whose matrix contains NaN or Infinity must be
        // treated as singular (no inverse, no hit) without panicking or
        // propagating NaN into the result.
        use glam::Affine2;
        let nan_matrix = Affine2::from_cols_array(&[f32::NAN, 0.0, 0.0, 1.0, 0.0, 0.0]);
        let nan_transform = AffineTransform::from_affine(nan_matrix);
        assert!(nan_transform.is_singular());
        assert!(nan_transform.inverse().is_none());

        let inf_matrix = Affine2::from_cols_array(&[f32::INFINITY, 0.0, 0.0, 1.0, 0.0, 0.0]);
        let inf_transform = AffineTransform::from_affine(inf_matrix);
        assert!(inf_transform.is_singular());
        assert!(inf_transform.inverse().is_none());

        // Hit testing through such a transform must return None, not panic.
        let mut arena = WidgetArena::new();
        let root = insert(&mut arena, 0.0, 0.0, 100.0, 100.0);
        let mut transforms = HashMap::new();
        transforms.insert(root, nan_transform);
        let tester = HitTester::new(&arena);
        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            tester.hit_test_with_transforms(root, Vec2::new(50.0, 50.0), &transforms)
        }));
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn empty_tree_no_children() {
        let mut arena = WidgetArena::new();
        let root = insert(&mut arena, 0.0, 0.0, 100.0, 100.0);
        let tester = HitTester::new(&arena);
        assert_eq!(
            tester
                .hit_test(root, Vec2::new(50.0, 50.0))
                .map(|h| h.widget_id),
            Some(root)
        );
        assert!(tester.hit_test(root, Vec2::new(150.0, 150.0)).is_none());
    }

    #[test]
    fn boundary_min_inclusive_max_exclusive() {
        let mut arena = WidgetArena::new();
        let root = insert(&mut arena, 10.0, 10.0, 100.0, 100.0);
        let tester = HitTester::new(&arena);
        // Min edge inclusive.
        assert!(tester.hit_test(root, Vec2::new(10.0, 10.0)).is_some());
        // Max edge exclusive.
        assert!(tester.hit_test(root, Vec2::new(110.0, 10.0)).is_none());
        assert!(tester.hit_test(root, Vec2::new(10.0, 110.0)).is_none());
    }

    #[test]
    fn deeply_nested_tree_10_levels() {
        let mut arena = WidgetArena::new();
        let mut current = insert(&mut arena, 0.0, 0.0, 200.0, 200.0);
        for _ in 0..10 {
            // Each child is inset by 10px so the deepest is the smallest.
            let child = insert(&mut arena, 10.0, 10.0, 180.0, 180.0);
            arena.append_child(current, child).unwrap();
            current = child;
        }
        let tester = HitTester::new(&arena);
        // Point at (15,15) is inside all nested children → deepest wins.
        let root = arena.iter_breadth_first().next().expect("root exists");
        let hit = tester.hit_test(root, Vec2::new(15.0, 15.0));
        assert_eq!(hit.map(|h| h.widget_id), Some(current));
    }

    #[test]
    fn rounded_rect_corner_miss() {
        let mut arena = WidgetArena::new();
        let root = insert(&mut arena, 0.0, 0.0, 100.0, 100.0);
        let mut clips = HashMap::new();
        clips.insert(
            root,
            ClipShape::RoundedRect(RoundedRect::new(Rect::new(0.0, 0.0, 100.0, 100.0), 20.0)),
        );
        let tester = HitTester::new(&arena);
        // Center hit.
        assert!(tester
            .hit_test_with_clip(root, Vec2::new(50.0, 50.0), &clips)
            .is_some());
        // Corner area (5,5) is inside the AABB but outside the 20px corner radius.
        assert!(tester
            .hit_test_with_clip(root, Vec2::new(5.0, 5.0), &clips)
            .is_none());
    }

    #[test]
    fn path_clip_star_shape_winding() {
        let mut arena = WidgetArena::new();
        let root = insert(&mut arena, -50.0, -50.0, 100.0, 100.0);
        // A simple diamond (rotated square) centered at origin.
        let diamond = vec![
            Vec2::new(0.0, -40.0),
            Vec2::new(40.0, 0.0),
            Vec2::new(0.0, 40.0),
            Vec2::new(-40.0, 0.0),
        ];
        let mut clips = HashMap::new();
        clips.insert(root, ClipShape::Path(diamond));
        let tester = HitTester::new(&arena);
        // Center of the diamond → inside.
        assert!(tester
            .hit_test_with_clip(root, Vec2::new(0.0, 0.0), &clips)
            .is_some());
        // A point in the AABB corner but outside the diamond → miss.
        assert!(tester
            .hit_test_with_clip(root, Vec2::new(-45.0, -45.0), &clips)
            .is_none());
    }

    #[test]
    fn path_clip_self_intersecting_winding_number() {
        // This test specifically distinguishes the winding-number rule
        // from the even-odd rule. The path below consists of two
        // overlapping squares both wound clockwise, forming a single
        // self-intersecting polygon. The overlap region in the center
        // has winding number 2, which is:
        //   - OUTSIDE under the even-odd rule (2 crossings = even)
        //   - INSIDE under the non-zero winding rule (2 ≠ 0)
        let mut arena = WidgetArena::new();
        let root = insert(&mut arena, 0.0, 0.0, 120.0, 120.0);
        // First square: (0,0)→(60,0)→(60,60)→(0,60)→(0,0) clockwise.
        // Second square: (40,40)→(100,40)→(100,100)→(40,100)→(40,40) clockwise.
        // The overlap region (40,40)–(60,60) has winding number 2.
        let double_square = vec![
            Vec2::new(0.0, 0.0),
            Vec2::new(60.0, 0.0),
            Vec2::new(60.0, 60.0),
            Vec2::new(0.0, 60.0),
            Vec2::new(0.0, 0.0),
            Vec2::new(40.0, 40.0),
            Vec2::new(100.0, 40.0),
            Vec2::new(100.0, 100.0),
            Vec2::new(40.0, 100.0),
            Vec2::new(40.0, 40.0),
        ];
        let mut clips = HashMap::new();
        clips.insert(root, ClipShape::Path(double_square));
        let tester = HitTester::new(&arena);
        // Inside the first square only (winding = 1).
        assert!(tester
            .hit_test_with_clip(root, Vec2::new(20.0, 30.0), &clips)
            .is_some());
        // Inside the second square only (winding = 1).
        assert!(tester
            .hit_test_with_clip(root, Vec2::new(80.0, 70.0), &clips)
            .is_some());
        // Inside the overlap region (winding = 2). Under even-odd this
        // would be OUTSIDE (2 crossings = even), but under the non-zero
        // winding rule it is INSIDE (2 ≠ 0). This assertion proves the
        // algorithm is winding-number, not even-odd.
        assert!(
            tester
                .hit_test_with_clip(root, Vec2::new(50.0, 50.0), &clips)
                .is_some(),
            "overlap region must be inside under winding-number rule"
        );
        // Outside both squares (winding = 0).
        assert!(tester
            .hit_test_with_clip(root, Vec2::new(110.0, 10.0), &clips)
            .is_none());
    }

    #[test]
    fn invisible_widget_skipped() {
        let mut arena = WidgetArena::new();
        let root = insert(&mut arena, 0.0, 0.0, 100.0, 100.0);
        // Make root invisible.
        if let Some(hot) = arena.get_hot_mut(root) {
            hot.flags.remove(NodeFlags::VISIBLE);
        }
        let tester = HitTester::new(&arena);
        assert!(tester.hit_test(root, Vec2::new(50.0, 50.0)).is_none());
    }

    #[test]
    fn inert_widget_skipped() {
        let mut arena = WidgetArena::new();
        let root = insert(&mut arena, 0.0, 0.0, 100.0, 100.0);
        if let Some(hot) = arena.get_hot_mut(root) {
            hot.flags |= NodeFlags::INERT;
        }
        let tester = HitTester::new(&arena);
        assert!(tester.hit_test(root, Vec2::new(50.0, 50.0)).is_none());
    }

    #[test]
    fn hit_test_enabled_flag_gated() {
        let mut arena = WidgetArena::new();
        let root = insert(&mut arena, 0.0, 0.0, 100.0, 100.0);
        if let Some(hot) = arena.get_hot_mut(root) {
            hot.flags.remove(NodeFlags::HIT_TEST_ENABLED);
        }
        let tester = HitTester::new(&arena);
        assert!(tester.hit_test(root, Vec2::new(50.0, 50.0)).is_none());
    }

    #[test]
    fn dead_root_returns_none() {
        let mut arena = WidgetArena::new();
        let root = insert(&mut arena, 0.0, 0.0, 100.0, 100.0);
        arena.remove(root);
        let tester = HitTester::new(&arena);
        assert!(tester.hit_test(root, Vec2::new(50.0, 50.0)).is_none());
    }

    #[test]
    fn stress_test_100k_synthetic_clicks() {
        let mut arena = WidgetArena::new();
        // Build a tree with collapsed, scaled, and rotated widgets.
        let root = insert(&mut arena, 0.0, 0.0, 400.0, 400.0);

        // Collapsed (zero-size) child.
        let collapsed = arena.insert(
            HotNode {
                bounds: Rect::new(200.0, 200.0, 0.0, 0.0),
                flags: NodeFlags::VISIBLE | NodeFlags::HIT_TEST_ENABLED,
                ..HotNode::default()
            },
            ColdNode::default(),
        );
        arena.append_child(root, collapsed).unwrap();

        // Scaled child.
        let scaled = insert(&mut arena, 0.0, 0.0, 200.0, 200.0);
        arena.append_child(root, scaled).unwrap();

        // Rotated child.
        let rotated = insert(&mut arena, -200.0, 0.0, 200.0, 200.0);
        arena.append_child(root, rotated).unwrap();

        let mut transforms = HashMap::new();
        transforms.insert(scaled, AffineTransform::from_scale(Vec2::new(2.0, 0.5)));
        transforms.insert(rotated, AffineTransform::from_angle(FRAC_PI_4));
        // Singular transform on the collapsed widget.
        transforms.insert(collapsed, AffineTransform::from_scale(Vec2::new(0.0, 0.0)));

        let tester = HitTester::new(&arena);

        // Deterministic pseudo-random sequence (no external rand dep needed).
        let mut state: u64 = 0x1234_5678_9abc_def0;
        let mut next = || {
            // xorshift64
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let to_f32 = |v: u64| -> f32 {
            // Map to [-1000, 1000] including occasional NaN/infinity/zero.
            let bits = (v & 0xFFFF_FFFF) as u32;
            match bits % 64 {
                0 => f32::NAN,
                1 => f32::INFINITY,
                2 => f32::NEG_INFINITY,
                _ => (bits % 2000) as f32 - 1000.0,
            }
        };

        for _ in 0..100_000 {
            let x = to_f32(next());
            let y = to_f32(next());
            let point = Vec2::new(x, y);
            // Must never panic across all three entry points.
            let _ = tester.hit_test(root, point);
            let _ = tester.hit_test_with_transforms(root, point, &transforms);
            let _ = tester.hit_test_full(root, point, &transforms, &HashMap::new());
        }
        // If we reach here without panicking, the stress test passed.
    }

    #[test]
    fn affine_transform_identity_round_trip() {
        let t = AffineTransform::IDENTITY;
        assert_eq!(t.transform_point(Vec2::new(5.0, 7.0)), Vec2::new(5.0, 7.0));
        let inv = t.inverse().unwrap();
        assert_eq!(
            inv.transform_point(Vec2::new(5.0, 7.0)),
            Vec2::new(5.0, 7.0)
        );
        assert!(!t.is_singular());
    }

    #[test]
    fn point_in_rect_non_finite() {
        let r = Rect::new(0.0, 0.0, 10.0, 10.0);
        assert!(!point_in_rect(Vec2::new(f32::NAN, 5.0), r));
        assert!(!point_in_rect(Vec2::new(5.0, f32::INFINITY), r));
    }

    #[test]
    fn point_in_path_degenerate() {
        assert!(!point_in_path(Vec2::new(1.0, 1.0), &[]));
        assert!(!point_in_path(Vec2::new(1.0, 1.0), &[Vec2::new(0.0, 0.0)]));
        assert!(!point_in_path(
            Vec2::new(1.0, 1.0),
            &[Vec2::new(0.0, 0.0), Vec2::new(2.0, 2.0)]
        ));
    }

    #[test]
    fn clip_shape_rect_evaluates() {
        let mut arena = WidgetArena::new();
        let root = insert(&mut arena, 0.0, 0.0, 100.0, 100.0);
        let mut clips = HashMap::new();
        // A clip smaller than the bounds.
        clips.insert(root, ClipShape::Rect(Rect::new(25.0, 25.0, 50.0, 50.0)));
        let tester = HitTester::new(&arena);
        assert!(tester
            .hit_test_with_clip(root, Vec2::new(40.0, 40.0), &clips)
            .is_some());
        // Inside bounds but outside the clip rect.
        assert!(tester
            .hit_test_with_clip(root, Vec2::new(10.0, 10.0), &clips)
            .is_none());
    }
}
