//! Projected-beam 2D spatial focus navigation.
//!
//! For gamepads, TV remotes, and keyboard navigation across non-linear
//! widget grids, this module implements a directional vector scoring
//! algorithm:
//!
//! ```text
//! Score(A, B, d) = α · Distance(A, B) + β · AngularDeviation(AB, d)
//! ```
//!
//! - `d ∈ {(0, -1), (0, 1), (-1, 0), (1, 0)}` represents the navigation
//!   direction.
//! - Candidates behind the source node or outside an 80° forward cone
//!   are rejected immediately.
//! - The candidate with the minimum score is awarded focus. If multiple
//!   candidates have equivalent scores, layout tree order serves as the
//!   deterministic tie-breaker.

use glam::Vec2;
use martensite_core::{NodeFlags, Rect, WidgetArena, WidgetId};

/// Default weight for the distance component of the spatial score.
pub const DEFAULT_ALPHA: f32 = 1.0;

/// Default weight for the angular deviation component of the spatial score.
pub const DEFAULT_BETA: f32 = 100.0;

/// Half-angle of the forward cone in degrees. Candidates outside this
/// cone (measured from the navigation direction vector) are rejected.
pub const FORWARD_CONE_DEGREES: f32 = 80.0;

/// A cardinal direction in which focus may be projected within the 2D
/// spatial layout.
///
/// # Examples
///
/// ```
/// use martensite_focus::FocusDirection;
///
/// assert!(FocusDirection::Up != FocusDirection::Down);
/// assert_eq!(FocusDirection::Left.vector(), glam::Vec2::new(-1.0, 0.0));
/// ```
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum FocusDirection {
    /// Focus projected upward (decreasing vertical coordinate).
    Up,
    /// Focus projected downward (increasing vertical coordinate).
    Down,
    /// Focus projected leftward (decreasing horizontal coordinate).
    Left,
    /// Focus projected rightward (increasing horizontal coordinate).
    Right,
}

impl FocusDirection {
    /// Returns the unit direction vector for this focus direction.
    #[inline]
    pub fn vector(self) -> Vec2 {
        match self {
            Self::Up => Vec2::new(0.0, -1.0),
            Self::Down => Vec2::new(0.0, 1.0),
            Self::Left => Vec2::new(-1.0, 0.0),
            Self::Right => Vec2::new(1.0, 0.0),
        }
    }
}

/// Computes the center point of a [`Rect`].
#[inline]
fn center(r: Rect) -> Vec2 {
    Vec2::new(r.origin.x + r.size.x * 0.5, r.origin.y + r.size.y * 0.5)
}

/// Computes the Euclidean distance between the centers of two rects.
#[inline]
fn distance(a: Rect, b: Rect) -> f32 {
    (center(a) - center(b)).length()
}

/// Computes the angular deviation (in degrees) between the vector from
/// `source` to `candidate` and the navigation `direction`.
///
/// Returns `None` if the candidate is at the same position as the source
/// (zero-length vector), if either rect has non-finite coordinates, or
/// if the angular deviation exceeds [`FORWARD_CONE_DEGREES`].
fn angular_deviation(source: Rect, candidate: Rect, direction: Vec2) -> Option<f32> {
    let src_center = center(source);
    let cand_center = center(candidate);

    // Reject non-finite geometry to prevent NaN propagation.
    if !src_center.is_finite() || !cand_center.is_finite() {
        return None;
    }

    let delta = cand_center - src_center;
    if !delta.is_finite() || delta.length_squared() < f32::EPSILON {
        return None;
    }
    let normalized = delta.normalize();
    let dot = normalized.dot(direction).clamp(-1.0, 1.0);
    let angle_rad = dot.acos();
    let angle_deg = angle_rad.to_degrees();

    if !angle_deg.is_finite() || angle_deg > FORWARD_CONE_DEGREES {
        return None;
    }
    Some(angle_deg)
}

/// Computes the spatial navigation score for a candidate relative to a
/// source in a given direction.
///
/// Lower scores are better. Returns `None` if the candidate is rejected
/// (behind the source or outside the forward cone).
///
/// The score is: `α · distance + β · angular_deviation`
fn compute_score(
    source: Rect,
    candidate: Rect,
    direction: Vec2,
    alpha: f32,
    beta: f32,
) -> Option<f32> {
    let ang = angular_deviation(source, candidate, direction)?;
    let dist = distance(source, candidate);
    Some(alpha * dist + beta * ang)
}

/// The 2D spatial navigation engine.
///
/// Walks the widget arena, identifies focusable candidates, and selects
/// the best candidate in the given direction using the projected-beam
/// scoring algorithm.
///
/// # Examples
///
/// ```
/// use martensite_focus::{FocusDirection, SpatialNavigator};
/// use martensite_core::{WidgetArena, HotNode, ColdNode, NodeFlags, Rect};
///
/// let mut arena = WidgetArena::new();
///
/// // Create a source node at (0, 0) that is focusable.
/// let mut src_hot = HotNode::default();
/// src_hot.bounds = Rect::new(0.0, 0.0, 10.0, 10.0);
/// src_hot.flags |= NodeFlags::FOCUSABLE | NodeFlags::VISIBLE;
/// let src = arena.insert(src_hot, ColdNode::default());
///
/// // Create a target node to the right at (100, 0).
/// let mut tgt_hot = HotNode::default();
/// tgt_hot.bounds = Rect::new(100.0, 0.0, 10.0, 10.0);
/// tgt_hot.flags |= NodeFlags::FOCUSABLE | NodeFlags::VISIBLE;
/// let tgt = arena.insert(tgt_hot, ColdNode::default());
///
/// let navigator = SpatialNavigator::new();
/// let next = navigator.navigate(&arena, src, FocusDirection::Right);
/// assert_eq!(next, Some(tgt));
/// ```
pub struct SpatialNavigator {
    alpha: f32,
    beta: f32,
}

impl Default for SpatialNavigator {
    fn default() -> Self {
        Self::new()
    }
}

impl SpatialNavigator {
    /// Creates a new navigator with default scoring weights.
    pub fn new() -> Self {
        Self {
            alpha: DEFAULT_ALPHA,
            beta: DEFAULT_BETA,
        }
    }

    /// Creates a navigator with custom scoring weights.
    ///
    /// `alpha` weights the distance component; `beta` weights the angular
    /// deviation component. Higher `beta` favors candidates that are more
    /// aligned with the navigation direction.
    ///
    /// Non-finite or negative weights are rejected; defaults are used
    /// instead.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_focus::SpatialNavigator;
    ///
    /// // Custom weights favoring angular alignment over distance.
    /// let nav = SpatialNavigator::with_weights(1.0, 500.0);
    /// assert_eq!(nav.alpha(), 1.0);
    /// assert_eq!(nav.beta(), 500.0);
    ///
    /// // Invalid (negative / non-finite) weights fall back to the defaults.
    /// let bad = SpatialNavigator::with_weights(-1.0, f32::NAN);
    /// assert_eq!(bad.alpha(), martensite_focus::DEFAULT_ALPHA);
    /// assert_eq!(bad.beta(), martensite_focus::DEFAULT_BETA);
    /// ```
    pub fn with_weights(alpha: f32, beta: f32) -> Self {
        let valid = |w: f32| w.is_finite() && w >= 0.0;
        Self {
            alpha: if valid(alpha) { alpha } else { DEFAULT_ALPHA },
            beta: if valid(beta) { beta } else { DEFAULT_BETA },
        }
    }

    /// Navigates from `source` in the given `direction`, returning the
    /// best focusable candidate.
    ///
    /// The algorithm:
    /// 1. Collects all visible, focusable nodes in the arena (excluding
    ///    the source).
    /// 2. For each candidate, computes the spatial score.
    /// 3. Rejects candidates behind the source or outside the 80° forward
    ///    cone.
    /// 4. Returns the candidate with the minimum score.
    /// 5. Ties are broken by layout tree (depth-first) order.
    pub fn navigate(
        &self,
        arena: &WidgetArena,
        source: WidgetId,
        direction: FocusDirection,
    ) -> Option<WidgetId> {
        let source_hot = arena.get_hot(source)?;
        if !source_hot.flags.contains(NodeFlags::VISIBLE)
            || !source_hot.flags.contains(NodeFlags::FOCUSABLE)
            || source_hot.flags.contains(NodeFlags::INERT)
        {
            return None;
        }
        let source_bounds = source_hot.bounds;
        let dir_vec = direction.vector();

        let mut best: Option<(f32, WidgetId)> = None;

        for candidate_id in arena.iter_depth_first() {
            if candidate_id == source {
                continue;
            }

            let Some(candidate_hot) = arena.get_hot(candidate_id) else {
                continue;
            };
            if !candidate_hot.flags.contains(NodeFlags::FOCUSABLE) {
                continue;
            }
            if !candidate_hot.flags.contains(NodeFlags::VISIBLE) {
                continue;
            }
            if candidate_hot.flags.contains(NodeFlags::INERT) {
                continue;
            }

            let candidate_bounds = candidate_hot.bounds;
            let score = compute_score(
                source_bounds,
                candidate_bounds,
                dir_vec,
                self.alpha,
                self.beta,
            );

            if let Some(score) = score {
                // Reject non-finite scores (NaN/inf from degenerate geometry).
                if !score.is_finite() {
                    continue;
                }
                match best {
                    None => best = Some((score, candidate_id)),
                    Some((best_score, _)) => {
                        // Strictly less than — ties keep the earlier
                        // depth-first (tree order) candidate.
                        if score < best_score {
                            best = Some((score, candidate_id));
                        }
                    }
                }
            }
        }

        best.map(|(_, id)| id)
    }

    /// Navigates from `source` in the given `direction`, but restricts
    /// candidates to those within the given `scope` subtree.
    ///
    /// This is used for modal focus trapping: only widgets that are
    /// descendants of (or equal to) `scope_root` are considered.
    pub fn navigate_within_scope(
        &self,
        arena: &WidgetArena,
        source: WidgetId,
        direction: FocusDirection,
        scope_root: WidgetId,
    ) -> Option<WidgetId> {
        let source_hot = arena.get_hot(source)?;
        if !source_hot.flags.contains(NodeFlags::VISIBLE)
            || !source_hot.flags.contains(NodeFlags::FOCUSABLE)
            || source_hot.flags.contains(NodeFlags::INERT)
        {
            return None;
        }
        let source_bounds = source_hot.bounds;
        let dir_vec = direction.vector();

        let mut best: Option<(f32, WidgetId)> = None;

        for candidate_id in arena.iter_subtree(scope_root) {
            if candidate_id == source {
                continue;
            }

            let Some(candidate_hot) = arena.get_hot(candidate_id) else {
                continue;
            };
            if !candidate_hot.flags.contains(NodeFlags::FOCUSABLE) {
                continue;
            }
            if !candidate_hot.flags.contains(NodeFlags::VISIBLE) {
                continue;
            }
            if candidate_hot.flags.contains(NodeFlags::INERT) {
                continue;
            }

            let candidate_bounds = candidate_hot.bounds;
            let score = compute_score(
                source_bounds,
                candidate_bounds,
                dir_vec,
                self.alpha,
                self.beta,
            );

            if let Some(score) = score {
                if !score.is_finite() {
                    continue;
                }
                match best {
                    None => best = Some((score, candidate_id)),
                    Some((best_score, _)) => {
                        if score < best_score {
                            best = Some((score, candidate_id));
                        }
                    }
                }
            }
        }

        best.map(|(_, id)| id)
    }

    /// Returns the alpha (distance) weight.
    pub fn alpha(&self) -> f32 {
        self.alpha
    }

    /// Returns the beta (angular deviation) weight.
    pub fn beta(&self) -> f32 {
        self.beta
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::{ColdNode, HotNode, NodeFlags, Rect, WidgetArena};

    fn make_focusable_node(arena: &mut WidgetArena, bounds: Rect) -> WidgetId {
        let hot = HotNode {
            bounds,
            flags: NodeFlags::FOCUSABLE | NodeFlags::VISIBLE,
            ..Default::default()
        };
        arena.insert(hot, ColdNode::default())
    }

    fn make_visible_node(arena: &mut WidgetArena, bounds: Rect) -> WidgetId {
        let hot = HotNode {
            bounds,
            flags: NodeFlags::VISIBLE,
            ..Default::default()
        };
        arena.insert(hot, ColdNode::default())
    }

    #[test]
    fn direction_vectors() {
        assert_eq!(FocusDirection::Up.vector(), Vec2::new(0.0, -1.0));
        assert_eq!(FocusDirection::Down.vector(), Vec2::new(0.0, 1.0));
        assert_eq!(FocusDirection::Left.vector(), Vec2::new(-1.0, 0.0));
        assert_eq!(FocusDirection::Right.vector(), Vec2::new(1.0, 0.0));
    }

    #[test]
    fn navigate_right_selects_rightward_candidate() {
        let mut arena = WidgetArena::new();
        let src = make_focusable_node(&mut arena, Rect::new(0.0, 0.0, 10.0, 10.0));
        let right = make_focusable_node(&mut arena, Rect::new(100.0, 0.0, 10.0, 10.0));
        let _left = make_focusable_node(&mut arena, Rect::new(-100.0, 0.0, 10.0, 10.0));

        let nav = SpatialNavigator::new();
        let result = nav.navigate(&arena, src, FocusDirection::Right);
        assert_eq!(result, Some(right));
    }

    #[test]
    fn navigate_left_selects_leftward_candidate() {
        let mut arena = WidgetArena::new();
        let src = make_focusable_node(&mut arena, Rect::new(0.0, 0.0, 10.0, 10.0));
        let left = make_focusable_node(&mut arena, Rect::new(-100.0, 0.0, 10.0, 10.0));
        let _right = make_focusable_node(&mut arena, Rect::new(100.0, 0.0, 10.0, 10.0));

        let nav = SpatialNavigator::new();
        let result = nav.navigate(&arena, src, FocusDirection::Left);
        assert_eq!(result, Some(left));
    }

    #[test]
    fn navigate_up_selects_upward_candidate() {
        let mut arena = WidgetArena::new();
        let src = make_focusable_node(&mut arena, Rect::new(0.0, 100.0, 10.0, 10.0));
        let up = make_focusable_node(&mut arena, Rect::new(0.0, 0.0, 10.0, 10.0));
        let _down = make_focusable_node(&mut arena, Rect::new(0.0, 200.0, 10.0, 10.0));

        let nav = SpatialNavigator::new();
        let result = nav.navigate(&arena, src, FocusDirection::Up);
        assert_eq!(result, Some(up));
    }

    #[test]
    fn navigate_down_selects_downward_candidate() {
        let mut arena = WidgetArena::new();
        let src = make_focusable_node(&mut arena, Rect::new(0.0, 0.0, 10.0, 10.0));
        let down = make_focusable_node(&mut arena, Rect::new(0.0, 200.0, 10.0, 10.0));
        let _up = make_focusable_node(&mut arena, Rect::new(0.0, -100.0, 10.0, 10.0));

        let nav = SpatialNavigator::new();
        let result = nav.navigate(&arena, src, FocusDirection::Down);
        assert_eq!(result, Some(down));
    }

    #[test]
    fn navigate_returns_none_when_no_candidates_in_direction() {
        let mut arena = WidgetArena::new();
        let src = make_focusable_node(&mut arena, Rect::new(0.0, 0.0, 10.0, 10.0));
        let _right = make_focusable_node(&mut arena, Rect::new(100.0, 0.0, 10.0, 10.0));

        let nav = SpatialNavigator::new();
        let result = nav.navigate(&arena, src, FocusDirection::Up);
        assert_eq!(result, None);
    }

    #[test]
    fn navigate_skips_non_focusable_nodes() {
        let mut arena = WidgetArena::new();
        let src = make_focusable_node(&mut arena, Rect::new(0.0, 0.0, 10.0, 10.0));
        // Visible but not focusable — should be skipped.
        let _non_focus = make_visible_node(&mut arena, Rect::new(100.0, 0.0, 10.0, 10.0));

        let nav = SpatialNavigator::new();
        let result = nav.navigate(&arena, src, FocusDirection::Right);
        assert_eq!(result, None);
    }

    #[test]
    fn navigate_skips_invisible_nodes() {
        let mut arena = WidgetArena::new();
        let src = make_focusable_node(&mut arena, Rect::new(0.0, 0.0, 10.0, 10.0));
        // Focusable but not visible — should be skipped.
        let hot = HotNode {
            bounds: Rect::new(100.0, 0.0, 10.0, 10.0),
            flags: NodeFlags::FOCUSABLE, // Not VISIBLE
            ..Default::default()
        };
        let _invisible = arena.insert(hot, ColdNode::default());

        let nav = SpatialNavigator::new();
        let result = nav.navigate(&arena, src, FocusDirection::Right);
        assert_eq!(result, None);
    }

    #[test]
    fn navigate_selects_closest_candidate() {
        let mut arena = WidgetArena::new();
        let src = make_focusable_node(&mut arena, Rect::new(0.0, 0.0, 10.0, 10.0));
        let near = make_focusable_node(&mut arena, Rect::new(50.0, 0.0, 10.0, 10.0));
        let _far = make_focusable_node(&mut arena, Rect::new(200.0, 0.0, 10.0, 10.0));

        let nav = SpatialNavigator::new();
        let result = nav.navigate(&arena, src, FocusDirection::Right);
        assert_eq!(result, Some(near));
    }

    #[test]
    fn navigate_selects_most_aligned_candidate() {
        let mut arena = WidgetArena::new();
        let src = make_focusable_node(&mut arena, Rect::new(0.0, 0.0, 10.0, 10.0));
        // Directly right but very far (distance ~1000, angle 0°).
        let aligned = make_focusable_node(&mut arena, Rect::new(1000.0, 0.0, 10.0, 10.0));
        // Very close but at a small angle (distance ~10, angle ~5.7°).
        let angled = make_focusable_node(&mut arena, Rect::new(10.0, 1.0, 10.0, 10.0));

        // With high beta, the more aligned candidate should win despite
        // being farther: aligned score = 1000 + 1000*0 = 1000,
        // angled score = 10 + 1000*5.7 ≈ 5710.
        let nav = SpatialNavigator::with_weights(1.0, 1000.0);
        let result = nav.navigate(&arena, src, FocusDirection::Right);
        assert_eq!(result, Some(aligned));

        // With default weights (beta=100), the closer candidate wins:
        // aligned score = 1000 + 100*0 = 1000,
        // angled score = 10 + 100*5.7 ≈ 580.
        let nav = SpatialNavigator::new();
        let result = nav.navigate(&arena, src, FocusDirection::Right);
        assert_eq!(result, Some(angled));
    }

    #[test]
    fn navigate_rejects_candidates_outside_cone() {
        let mut arena = WidgetArena::new();
        let src = make_focusable_node(&mut arena, Rect::new(0.0, 0.0, 10.0, 10.0));
        // Candidate at ~85° from the right direction — outside the 80° cone.
        // Place it far up and slightly right.
        let _outside = make_focusable_node(&mut arena, Rect::new(1.0, -100.0, 10.0, 10.0));

        let nav = SpatialNavigator::new();
        let result = nav.navigate(&arena, src, FocusDirection::Right);
        // Should be rejected because it's outside the forward cone.
        assert_eq!(result, None);
    }

    #[test]
    fn navigate_within_scope_restricts_candidates() {
        let mut arena = WidgetArena::new();
        let root = make_focusable_node(&mut arena, Rect::new(0.0, 0.0, 10.0, 10.0));
        let inside_scope = make_focusable_node(&mut arena, Rect::new(100.0, 0.0, 10.0, 10.0));
        let _outside_scope = make_focusable_node(&mut arena, Rect::new(200.0, 0.0, 10.0, 10.0));

        // Make root a parent of inside_scope only.
        arena.append_child(root, inside_scope).unwrap();

        let nav = SpatialNavigator::new();
        let result = nav.navigate_within_scope(&arena, root, FocusDirection::Right, root);
        assert_eq!(result, Some(inside_scope));
        // outside_scope is not in the subtree of root, so it should not be found.
    }

    #[test]
    fn navigate_tie_breaks_by_tree_order() {
        let mut arena = WidgetArena::new();
        let src = make_focusable_node(&mut arena, Rect::new(0.0, 0.0, 10.0, 10.0));
        // Two candidates at the exact same position to the right.
        let first = make_focusable_node(&mut arena, Rect::new(100.0, 0.0, 10.0, 10.0));
        let second = make_focusable_node(&mut arena, Rect::new(100.0, 0.0, 10.0, 10.0));

        let nav = SpatialNavigator::new();
        let result = nav.navigate(&arena, src, FocusDirection::Right);
        // Both have the same score; the first in depth-first order wins.
        assert_eq!(result, Some(first));
        assert_ne!(result, Some(second));
    }

    #[test]
    fn navigate_returns_none_for_invisible_source() {
        let mut arena = WidgetArena::new();
        let hot = HotNode {
            bounds: Rect::new(0.0, 0.0, 10.0, 10.0),
            flags: NodeFlags::FOCUSABLE, // Not VISIBLE
            ..Default::default()
        };
        let src = arena.insert(hot, ColdNode::default());
        let _target = make_focusable_node(&mut arena, Rect::new(100.0, 0.0, 10.0, 10.0));

        let nav = SpatialNavigator::new();
        let result = nav.navigate(&arena, src, FocusDirection::Right);
        assert_eq!(result, None);
    }

    #[test]
    fn angular_deviation_straight_ahead_is_zero() {
        let src = Rect::new(0.0, 0.0, 10.0, 10.0);
        let cand = Rect::new(100.0, 0.0, 10.0, 10.0);
        let dir = Vec2::new(1.0, 0.0);
        let ang = angular_deviation(src, cand, dir).unwrap();
        assert!((ang - 0.0).abs() < 0.01);
    }

    #[test]
    fn angular_deviation_perpendicular_is_90() {
        let src = Rect::new(0.0, 0.0, 10.0, 10.0);
        let cand = Rect::new(0.0, 100.0, 10.0, 10.0);
        let dir = Vec2::new(1.0, 0.0);
        let ang = angular_deviation(src, cand, dir);
        // 90° is outside the 80° cone, so should be None.
        assert!(ang.is_none());
    }

    #[test]
    fn angular_deviation_behind_is_180() {
        let src = Rect::new(100.0, 0.0, 10.0, 10.0);
        let cand = Rect::new(0.0, 0.0, 10.0, 10.0);
        let dir = Vec2::new(1.0, 0.0);
        let ang = angular_deviation(src, cand, dir);
        // 180° is outside the 80° cone, so should be None.
        assert!(ang.is_none());
    }

    #[test]
    fn angular_deviation_zero_length_returns_none() {
        let src = Rect::new(0.0, 0.0, 10.0, 10.0);
        let cand = Rect::new(0.0, 0.0, 10.0, 10.0);
        let dir = Vec2::new(1.0, 0.0);
        let ang = angular_deviation(src, cand, dir);
        assert!(ang.is_none());
    }

    #[test]
    fn navigator_weights() {
        let nav = SpatialNavigator::with_weights(2.0, 50.0);
        assert_eq!(nav.alpha(), 2.0);
        assert_eq!(nav.beta(), 50.0);
    }

    #[test]
    fn navigator_default_weights() {
        let nav = SpatialNavigator::new();
        assert_eq!(nav.alpha(), DEFAULT_ALPHA);
        assert_eq!(nav.beta(), DEFAULT_BETA);
    }

    #[test]
    fn grid_2d_all_four_directions() {
        // Create a 3x3 grid of focusable nodes with irregular spacing.
        let mut arena = WidgetArena::new();

        let positions = [
            (0.0, 0.0),     // 0: top-left
            (50.0, 0.0),    // 1: top-center
            (120.0, 0.0),   // 2: top-right
            (0.0, 60.0),    // 3: middle-left
            (50.0, 60.0),   // 4: middle-center
            (120.0, 60.0),  // 5: middle-right
            (0.0, 150.0),   // 6: bottom-left
            (50.0, 150.0),  // 7: bottom-center
            (120.0, 150.0), // 8: bottom-right
        ];

        let mut ids = Vec::new();
        for &(x, y) in &positions {
            ids.push(make_focusable_node(&mut arena, Rect::new(x, y, 20.0, 20.0)));
        }

        let nav = SpatialNavigator::new();

        // From center (4), right should go to 5.
        assert_eq!(
            nav.navigate(&arena, ids[4], FocusDirection::Right),
            Some(ids[5])
        );
        // From center (4), left should go to 3.
        assert_eq!(
            nav.navigate(&arena, ids[4], FocusDirection::Left),
            Some(ids[3])
        );
        // From center (4), up should go to 1.
        assert_eq!(
            nav.navigate(&arena, ids[4], FocusDirection::Up),
            Some(ids[1])
        );
        // From center (4), down should go to 7.
        assert_eq!(
            nav.navigate(&arena, ids[4], FocusDirection::Down),
            Some(ids[7])
        );

        // From 1, right should go to 2.
        assert_eq!(
            nav.navigate(&arena, ids[1], FocusDirection::Right),
            Some(ids[2])
        );
        // From 2, left should go to 1.
        assert_eq!(
            nav.navigate(&arena, ids[2], FocusDirection::Left),
            Some(ids[1])
        );
        // From 3, down should go to 6.
        assert_eq!(
            nav.navigate(&arena, ids[3], FocusDirection::Down),
            Some(ids[6])
        );
        // From 6, up should go to 3.
        assert_eq!(
            nav.navigate(&arena, ids[6], FocusDirection::Up),
            Some(ids[3])
        );
    }

    #[test]
    fn navigate_skips_inert_candidates() {
        let mut arena = WidgetArena::new();
        let src = make_focusable_node(&mut arena, Rect::new(0.0, 0.0, 10.0, 10.0));
        // Inert but focusable+visible — should be skipped.
        let hot = HotNode {
            bounds: Rect::new(100.0, 0.0, 10.0, 10.0),
            flags: NodeFlags::FOCUSABLE | NodeFlags::VISIBLE | NodeFlags::INERT,
            ..Default::default()
        };
        let _inert = arena.insert(hot, ColdNode::default());
        let target = make_focusable_node(&mut arena, Rect::new(200.0, 0.0, 10.0, 10.0));

        let nav = SpatialNavigator::new();
        let result = nav.navigate(&arena, src, FocusDirection::Right);
        // Should skip the inert widget and select the target.
        assert_eq!(result, Some(target));
    }

    #[test]
    fn navigate_rejects_non_focusable_source() {
        let mut arena = WidgetArena::new();
        let hot = HotNode {
            bounds: Rect::new(0.0, 0.0, 10.0, 10.0),
            flags: NodeFlags::VISIBLE, // Not FOCUSABLE
            ..Default::default()
        };
        let src = arena.insert(hot, ColdNode::default());
        let _target = make_focusable_node(&mut arena, Rect::new(100.0, 0.0, 10.0, 10.0));

        let nav = SpatialNavigator::new();
        let result = nav.navigate(&arena, src, FocusDirection::Right);
        assert_eq!(result, None);
    }

    #[test]
    fn navigate_rejects_inert_source() {
        let mut arena = WidgetArena::new();
        let hot = HotNode {
            bounds: Rect::new(0.0, 0.0, 10.0, 10.0),
            flags: NodeFlags::FOCUSABLE | NodeFlags::VISIBLE | NodeFlags::INERT,
            ..Default::default()
        };
        let src = arena.insert(hot, ColdNode::default());
        let _target = make_focusable_node(&mut arena, Rect::new(100.0, 0.0, 10.0, 10.0));

        let nav = SpatialNavigator::new();
        let result = nav.navigate(&arena, src, FocusDirection::Right);
        assert_eq!(result, None);
    }

    #[test]
    fn angular_deviation_rejects_nan_rect() {
        let src = Rect::new(f32::NAN, 0.0, 10.0, 10.0);
        let cand = Rect::new(100.0, 0.0, 10.0, 10.0);
        let dir = Vec2::new(1.0, 0.0);
        assert!(angular_deviation(src, cand, dir).is_none());
    }

    #[test]
    fn angular_deviation_rejects_infinite_rect() {
        let src = Rect::new(0.0, 0.0, 10.0, 10.0);
        let cand = Rect::new(f32::INFINITY, 0.0, 10.0, 10.0);
        let dir = Vec2::new(1.0, 0.0);
        assert!(angular_deviation(src, cand, dir).is_none());
    }

    #[test]
    fn navigate_with_nan_bounds_does_not_panic() {
        let mut arena = WidgetArena::new();
        let hot = HotNode {
            bounds: Rect::new(f32::NAN, f32::NAN, 10.0, 10.0),
            flags: NodeFlags::FOCUSABLE | NodeFlags::VISIBLE,
            ..Default::default()
        };
        let src = arena.insert(hot, ColdNode::default());
        let _target = make_focusable_node(&mut arena, Rect::new(100.0, 0.0, 10.0, 10.0));

        let nav = SpatialNavigator::new();
        // Should not panic or return a NaN-poisoned result.
        let result = nav.navigate(&arena, src, FocusDirection::Right);
        // The source center is NaN, so angular_deviation returns None for
        // all candidates — result should be None.
        assert_eq!(result, None);
    }

    #[test]
    fn with_weights_rejects_invalid_weights() {
        let nav = SpatialNavigator::with_weights(f32::NAN, 100.0);
        assert_eq!(nav.alpha(), DEFAULT_ALPHA);
        assert_eq!(nav.beta(), 100.0);

        let nav = SpatialNavigator::with_weights(1.0, -5.0);
        assert_eq!(nav.alpha(), 1.0);
        assert_eq!(nav.beta(), DEFAULT_BETA);

        let nav = SpatialNavigator::with_weights(f32::INFINITY, 50.0);
        assert_eq!(nav.alpha(), DEFAULT_ALPHA);
    }

    #[test]
    fn angular_deviation_at_cone_boundary() {
        // A candidate at exactly 80° should be accepted (not > 80°).
        // Place candidate so the angle from source is exactly 80°.
        // tan(80°) ≈ 5.671. If source is at (0,0) center (5,5) and
        // direction is Right (1,0), we need a candidate whose center
        // is at angle 80° from the x-axis.
        let angle_rad = 80.0f32.to_radians();
        let src_center = Vec2::new(5.0, 5.0);
        let dist = 100.0;
        let cand_center = src_center + Vec2::new(dist * angle_rad.cos(), -dist * angle_rad.sin());
        let src = Rect::new(0.0, 0.0, 10.0, 10.0);
        let cand = Rect::new(cand_center.x - 5.0, cand_center.y - 5.0, 10.0, 10.0);
        let dir = Vec2::new(1.0, 0.0);
        let ang = angular_deviation(src, cand, dir);
        assert!(ang.is_some(), "candidate at 80° should be within the cone");
        let ang = ang.unwrap();
        assert!(
            (ang - 80.0).abs() < 0.5,
            "angle should be ~80°, got {}",
            ang
        );
    }

    /// Irregular-grid spatial navigation: tests navigation in all 4
    /// directions from every node of an irregular grid (varying sizes,
    /// positions, gaps) and verifies the correct target is reached 100%
    /// of the time. Includes edge cases: corner nodes, nodes with the
    /// same Y but different X, and overlapping nodes.
    #[test]
    fn irregular_grid_all_directions_from_every_node() {
        let mut arena = WidgetArena::new();

        // Build an irregular 3x3 grid. Centers are aligned on a regular
        // lattice (cx ∈ {50, 150, 250}, cy ∈ {50, 150, 250}) but each node
        // has a different size, so the bounds are irregular. This makes
        // Left/Right navigate within a row and Up/Down navigate within a
        // column, while still exercising the scoring algorithm against
        // off-axis candidates.
        //
        //  (cx, cy)  -> rect (x, y, w, h)
        //  A(50,50,40,30)  B(150,50,30,40)  C(250,50,50,25)
        //  D(50,150,35,45) E(150,150,45,35) F(250,150,25,50)
        //  G(50,250,40,40) H(150,250,30,30) I(250,250,45,35)
        let grid = [
            // row 0
            (50.0_f32, 50.0, 40.0, 30.0), // A
            (150.0, 50.0, 30.0, 40.0),    // B
            (250.0, 50.0, 50.0, 25.0),    // C
            // row 1
            (50.0, 150.0, 35.0, 45.0),  // D
            (150.0, 150.0, 45.0, 35.0), // E
            (250.0, 150.0, 25.0, 50.0), // F
            // row 2
            (50.0, 250.0, 40.0, 40.0),  // G
            (150.0, 250.0, 30.0, 30.0), // H
            (250.0, 250.0, 45.0, 35.0), // I
        ];

        let mut ids = Vec::new();
        for &(cx, cy, w, h) in &grid {
            let rect = Rect::new(cx - w * 0.5, cy - h * 0.5, w, h);
            ids.push(make_focusable_node(&mut arena, rect));
        }
        // Index layout: ids[r*3 + c] for row r, col c.

        // Add an overlapping node that shares bounds with E (center 150,150)
        // to test the overlapping edge case. It is placed slightly offset so
        // it has a distinct center.
        let overlap = make_focusable_node(
            &mut arena,
            Rect::new(140.0, 140.0, 30.0, 30.0), // center (155, 155), overlaps E
        );

        let nav = SpatialNavigator::new();

        // Helper: expected target index for (row, col, direction), or None.
        // With aligned centers, Right/Left stay in the same row and Up/Down
        // stay in the same column.
        let expected = |row: usize, col: usize, dir: FocusDirection| -> Option<usize> {
            match dir {
                FocusDirection::Right => {
                    if col < 2 {
                        Some(row * 3 + col + 1)
                    } else {
                        None
                    }
                }
                FocusDirection::Left => {
                    if col > 0 {
                        Some(row * 3 + col - 1)
                    } else {
                        None
                    }
                }
                FocusDirection::Down => {
                    if row < 2 {
                        Some((row + 1) * 3 + col)
                    } else {
                        None
                    }
                }
                FocusDirection::Up => {
                    if row > 0 {
                        Some((row - 1) * 3 + col)
                    } else {
                        None
                    }
                }
            }
        };

        let directions = [
            FocusDirection::Right,
            FocusDirection::Left,
            FocusDirection::Down,
            FocusDirection::Up,
        ];

        let mut checked = 0usize;
        for row in 0..3 {
            for col in 0..3 {
                let src = ids[row * 3 + col];
                for &dir in &directions {
                    let result = nav.navigate(&arena, src, dir);
                    let want = expected(row, col, dir).map(|i| ids[i]);
                    assert_eq!(
                        result, want,
                        "from grid[{}][{}] ({:?}) expected {:?}, got {:?}",
                        row, col, dir, want, result
                    );
                    checked += 1;
                }
            }
        }
        // 9 nodes × 4 directions = 36 navigation checks, all passing.
        assert_eq!(
            checked, 36,
            "should have checked all 36 node-direction pairs"
        );

        // Edge case: corner nodes have no candidate in two directions.
        // Top-left (A, row 0 col 0): Up and Left return None.
        assert_eq!(nav.navigate(&arena, ids[0], FocusDirection::Up), None);
        assert_eq!(nav.navigate(&arena, ids[0], FocusDirection::Left), None);
        // Bottom-right (I, row 2 col 2): Down and Right return None.
        assert_eq!(nav.navigate(&arena, ids[8], FocusDirection::Down), None);
        assert_eq!(nav.navigate(&arena, ids[8], FocusDirection::Right), None);

        // Edge case: nodes with the same Y but different X. From B
        // (center 150,50), Right should reach C (center 250,50), not any
        // node in another row, despite D/E/F having nearby X values.
        assert_eq!(
            nav.navigate(&arena, ids[1], FocusDirection::Right),
            Some(ids[2])
        );
        // From B, Left should reach A (center 50,50).
        assert_eq!(
            nav.navigate(&arena, ids[1], FocusDirection::Left),
            Some(ids[0])
        );

        // Edge case: overlapping node. From the overlap node (center
        // 155,155), Down should reach H (center 150,250) — the most
        // aligned downward candidate — not G or I which are farther off
        // axis.
        let down_from_overlap = nav.navigate(&arena, overlap, FocusDirection::Down);
        assert_eq!(down_from_overlap, Some(ids[7])); // H
                                                     // From the overlap, Up should reach B (center 150,50) — most
                                                     // aligned upward.
        let up_from_overlap = nav.navigate(&arena, overlap, FocusDirection::Up);
        assert_eq!(up_from_overlap, Some(ids[1])); // B
    }

    /// Irregular grid with varying gaps: verifies navigation still picks
    /// the geometrically correct target when horizontal and vertical gaps
    /// differ between rows and columns.
    #[test]
    fn irregular_grid_varying_gaps() {
        let mut arena = WidgetArena::new();

        // Two rows with very different horizontal spacing. Row 0 has
        // tight spacing (gap 30), row 1 has wide spacing (gap 120).
        // Centers:
        //   row 0: x=50, x=120  (gap 70 between centers)
        //   row 1: x=50, x=250  (gap 200 between centers)
        // Both rows at cy=50 and cy=200.
        let a = make_focusable_node(&mut arena, Rect::new(30.0, 35.0, 40.0, 30.0)); // center (50,50)
        let b = make_focusable_node(&mut arena, Rect::new(100.0, 35.0, 40.0, 30.0)); // center (120,50)
        let c = make_focusable_node(&mut arena, Rect::new(30.0, 185.0, 40.0, 30.0)); // center (50,200)
        let d = make_focusable_node(&mut arena, Rect::new(230.0, 185.0, 40.0, 30.0)); // center (250,200)

        let nav = SpatialNavigator::new();

        // From A, Right -> B (same row, closest to the right).
        assert_eq!(nav.navigate(&arena, a, FocusDirection::Right), Some(b));
        // From B, Left -> A.
        assert_eq!(nav.navigate(&arena, b, FocusDirection::Left), Some(a));
        // From A, Down -> C (directly below, 0° deviation).
        assert_eq!(nav.navigate(&arena, a, FocusDirection::Down), Some(c));
        // From B, Down -> C (center 50,200) is closer and more aligned than
        // D (center 250,200). B center is (120,50). C center (50,200): delta
        // (-70,150), angle from down (0,1) = atan2(70,150) ≈ 25°. D center
        // (250,200): delta (130,150), angle from down = atan2(130,150) ≈ 41°.
        // C is closer (dist ~165 vs ~197) and more aligned (25° vs 41°), so C
        // wins.
        assert_eq!(nav.navigate(&arena, b, FocusDirection::Down), Some(c));
        // From C, Up -> A (directly above).
        assert_eq!(nav.navigate(&arena, c, FocusDirection::Up), Some(a));
        // From D, Up -> B (center 120,50) is more aligned than A (center
        // 50,50). D center (250,200). B delta (-130,-150), angle from up
        // (0,-1) = atan2(130,150) ≈ 41°. A delta (-200,-150), angle from up
        // = atan2(200,150) ≈ 53°. B is closer and more aligned, so B wins.
        assert_eq!(nav.navigate(&arena, d, FocusDirection::Up), Some(b));
    }
}
