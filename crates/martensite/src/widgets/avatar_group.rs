//! `AvatarGroup` — overlapping avatar stack.
//!
//! The Ant `Avatar.Group` / Teams presence-cluster pattern: members
//! paint overlapping left-to-right (later members on top), and beyond
//! `max_count` the extras collapse into a trailing `+N` chip.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::{Avatar, AvatarGroup};
//!
//! let g = AvatarGroup::new()
//!     .member(Avatar::new("Ada Lovelace"))
//!     .member(Avatar::new("Grace Hopper"))
//!     .member(Avatar::new("Katherine Johnson"))
//!     .max_count(2);
//! assert_eq!(g.visible_count(), 2);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    LayoutConstraints, LayoutContext, PaintContext, Rect, RenderMinimum, UnderflowPolicy, Widget,
};
use martensite_theme::TokenKey;

use crate::widgets::avatar::Avatar;

/// `+N` chip fill.
const CHIP: TokenKey = TokenKey::SurfaceColor;
/// `+N` chip ink.
const TEXT: TokenKey = TokenKey::TextColor;
/// Ring separating stacked avatars.
const RING: TokenKey = TokenKey::BackgroundColor;
/// Default member size (logical points).
const SIZE: f32 = 32.0;

/// An overlapping avatar stack — see the module docs.
///
/// # Examples
///
/// ```
/// use martensite::widgets::{Avatar, AvatarGroup};
/// use martensite::core::Widget;
///
/// let mut g = AvatarGroup::new().member(Avatar::new("A B"));
/// assert_eq!(g.child_count(), 1);
/// ```
pub struct AvatarGroup {
    label: String,
    enabled: bool,
    members: Vec<Avatar>,
    max_count: Option<usize>,
    /// Overlap fraction (`0` = touching, `0.5` = half-covered).
    overlap: f32,
    size: f32,
    /// Laid-out member rects (visible members only).
    bounds: Vec<Rect>,
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl AvatarGroup {
    /// An empty stack.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::AvatarGroup;
    ///
    /// let g = AvatarGroup::new();
    /// assert_eq!(g.visible_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Members".into(),
            enabled: true,
            members: Vec::new(),
            max_count: None,
            overlap: 0.25,
            size: SIZE,
            bounds: Vec::new(),
            text_painter: None,
        }
    }

    /// Append a member avatar.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Avatar, AvatarGroup};
    ///
    /// let g = AvatarGroup::new().member(Avatar::new("Ada Lovelace"));
    /// assert_eq!(g.member_count(), 1);
    /// ```
    pub fn member(mut self, avatar: Avatar) -> Self {
        self.members.push(avatar);
        self
    }

    /// Cap visible members; the rest collapse into a `+N` chip.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Avatar, AvatarGroup};
    ///
    /// let g = AvatarGroup::new()
    ///     .member(Avatar::new("a")).member(Avatar::new("b"))
    ///     .member(Avatar::new("c")).max_count(2);
    /// assert_eq!(g.overflow_count(), 1);
    /// ```
    pub fn max_count(mut self, max: usize) -> Self {
        self.max_count = Some(max);
        self
    }

    /// Overlap fraction — `0.0` touching, `0.5` half-covered (default
    /// `0.25`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::AvatarGroup;
    ///
    /// let g = AvatarGroup::new().overlap(0.4);
    /// ```
    pub fn overlap(mut self, overlap: f32) -> Self {
        self.overlap = overlap.clamp(0.0, 0.8);
        self
    }

    /// Member diameter in logical points (default 32) — propagates to
    /// members at layout.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::AvatarGroup;
    ///
    /// let g = AvatarGroup::new().size(48.0);
    /// ```
    pub fn size(mut self, size: f32) -> Self {
        self.size = size.max(8.0);
        self
    }

    /// Set the accessibility label (default `"Members"`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::AvatarGroup;
    ///
    /// let g = AvatarGroup::new().label("Assignees");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Enable or disable (dims; default `true`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::AvatarGroup;
    ///
    /// let g = AvatarGroup::new().enabled(false);
    /// ```
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Share a text painter — forwarded to every member's initials
    /// fallback. `SharedTextPainter` is not `Default`, so this builder
    /// is exercised indirectly through `paint`.
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Total member count (including overflow).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Avatar, AvatarGroup};
    ///
    /// let g = AvatarGroup::new().member(Avatar::new("a"));
    /// assert_eq!(g.member_count(), 1);
    /// ```
    pub fn member_count(&self) -> usize {
        self.members.len()
    }

    /// Members painted (before the `+N` collapse).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Avatar, AvatarGroup};
    ///
    /// let g = AvatarGroup::new().member(Avatar::new("a")).max_count(0);
    /// assert_eq!(g.visible_count(), 0);
    /// ```
    pub fn visible_count(&self) -> usize {
        match self.max_count {
            Some(max) => self.members.len().min(max),
            None => self.members.len(),
        }
    }

    /// Members collapsed into the `+N` chip.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Avatar, AvatarGroup};
    ///
    /// let g = AvatarGroup::new().member(Avatar::new("a"));
    /// assert_eq!(g.overflow_count(), 0);
    /// ```
    pub fn overflow_count(&self) -> usize {
        self.members.len() - self.visible_count()
    }

    /// Stack width in logical points for `n` shown slots.
    fn width_for(&self, slots: usize) -> f32 {
        if slots == 0 {
            return 0.0;
        }
        self.size + (slots - 1) as f32 * self.size * (1.0 - self.overlap)
    }
}

impl Default for AvatarGroup {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for AvatarGroup {
    fn measure(&mut self, cx: &mut LayoutContext, _constraints: LayoutConstraints) -> Vec2 {
        let size = cx.pt(self.size);
        let slots = self.visible_count() + usize::from(self.overflow_count() > 0);
        let _ = size;
        Vec2::new(self.width_for(slots), self.size)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        let size = cx.pt(self.size);
        let pitch = size * (1.0 - self.overlap);
        let visible = self.visible_count();
        self.bounds = (0..visible)
            .map(|i| {
                Rect::new(
                    bounds.min_x() + i as f32 * pitch,
                    bounds.min_y(),
                    size,
                    size,
                )
            })
            .collect();
        for (i, m) in self.members.iter_mut().take(visible).enumerate() {
            cx.layout_child(m, self.bounds[i]);
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let overflow = self.overflow_count();
        if overflow == 0 {
            return;
        }
        // The +N chip trails the last visible avatar.
        let Some(last) = self.bounds.last().copied() else {
            return;
        };
        let size = cx.pt(self.size);
        let pitch = size * (1.0 - self.overlap);
        let chip = kurbo::Rect::new(
            f64::from(last.min_x() + pitch),
            f64::from(last.min_y()),
            f64::from(last.min_x() + pitch + size),
            f64::from(last.min_y() + size),
        );
        let fill = cx.color(CHIP, [235, 236, 240, 255]);
        let ring = cx.color(RING, [255, 255, 255, 255]);
        let ink = cx.color(TEXT, [60, 60, 68, 255]);
        let shape = martensite_core::shape::Shape::ELLIPSE;
        cx.list.push_fill_shape(chip, &shape, fill);
        cx.list.push_stroke_shape(chip, &shape, cx.pt(1.5), ring);
        let text = format!("+{overflow}");
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let size_px = size * 0.38;
        let w = painter
            .and_then(|p| p.measure_text(&text, size_px))
            .unwrap_or(size_px * text.len() as f32 * 0.55);
        crate::text_paint::paint_label_clipped(
            painter,
            cx.list,
            chip,
            kurbo::Point::new(
                chip.x0 + (chip.width() - f64::from(w)) * 0.5,
                chip.y0 + chip.height() * 0.72,
            ),
            &text,
            size_px,
            if self.enabled { ink } else { fill },
        );
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Group);
        node.set_label(self.label.as_str());
        node.set_value(format!("{} members", self.members.len()));
        if !self.enabled {
            node.set_disabled();
        }
    }

    fn child_count(&self) -> usize {
        self.visible_count()
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        self.members.get(index).map(|a| a as &dyn Widget)
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        self.members.get_mut(index).map(|a| a as &mut dyn Widget)
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        self.bounds.get(index).copied()
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(SIZE, SIZE)).with_policy(UnderflowPolicy::Lint)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn lay(w: &mut AvatarGroup) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        w.layout(&mut cx, Rect::new(0.0, 0.0, 400.0, 40.0));
    }

    fn group(n: usize) -> AvatarGroup {
        (0..n).fold(AvatarGroup::new(), |g, i| {
            g.member(Avatar::new(format!("User {i}")))
        })
    }

    #[test]
    fn members_overlap_by_pitch() {
        let mut g = group(3).overlap(0.25);
        lay(&mut g);
        let pitch = SIZE * 0.75;
        assert!((g.bounds[1].min_x() - pitch).abs() < 0.01);
        assert!((g.bounds[2].min_x() - pitch * 2.0).abs() < 0.01);
    }

    #[test]
    fn overflow_collapses_into_chip() {
        let mut g = group(5).max_count(3);
        lay(&mut g);
        assert_eq!(g.visible_count(), 3);
        assert_eq!(g.overflow_count(), 2);
        // Hidden members report no bounds.
        assert_eq!(g.child_count(), 3);
        assert!(g.child_bounds(3).is_none());
    }

    #[test]
    fn measure_covers_visible_plus_chip() {
        let mut g = group(5).max_count(3);
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        let size = g.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(500.0, 100.0),
            },
        );
        // 3 visible + 1 chip slot.
        let want = g.width_for(4);
        assert!((size.x - want).abs() < 0.01);
    }

    #[test]
    fn empty_group_is_zero_sized() {
        let mut g = AvatarGroup::new();
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        let size = g.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(500.0, 100.0),
            },
        );
        assert_eq!(size.x, 0.0);
    }
}
