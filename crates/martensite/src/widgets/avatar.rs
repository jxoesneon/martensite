//! `Avatar` widget: a circular (or rounded) user avatar — image
//! content clipped to the silhouette, or initials on an accent-tinted
//! disc when no image is supplied.
//!
//! The image variant embeds an [`Image`] internal child set to
//! [`ImageFit::Cover`] and relies on the framework's child-clip
//! machinery ([`Widget::clips_children`] + [`Widget::clip_shape`]) to
//! crop it to the ellipse — so the same clip governs painting,
//! hit-testing, and accessibility geometry.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::avatar::Avatar;
//!
//! // Initials avatar — no image.
//! let a = Avatar::new("Ada Lovelace");
//! assert_eq!(a.name, "Ada Lovelace");
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::shape::Shape;
use martensite_core::widget::{LayoutConstraints, LayoutContext, PaintContext, Widget};
use martensite_core::{ImageData, Rect, TokenKey};

use crate::widgets::image::{Image, ImageFit};

const ACCENT: [u8; 4] = [40, 110, 220, 255];
const INVERSE_INK: [u8; 4] = [255, 255, 255, 255];
const EDGE: [u8; 4] = [110, 115, 125, 255];
/// Default avatar diameter in logical points.
const DEFAULT_SIZE: f32 = 32.0;

/// A circular user avatar.
///
/// With an [`ImageData`] set via [`Avatar::image`] the content is an
/// internal [`Image`] child clipped to the silhouette; without one the
/// avatar paints the first letters of the first and last words of
/// [`Avatar::name`] (e.g. "Ada Lovelace" → "AL") centered on an
/// accent-tinted disc. [`Avatar::rounded`] switches the silhouette from
/// a full circle to a rounded square.
///
/// # Examples
///
/// ```
/// use martensite::widgets::Avatar;
/// use martensite_core::{ImageData, Widget};
///
/// let a = Avatar::new("Ada Lovelace").size(40.0);
/// assert_eq!(a.size, 40.0);
///
/// let data = ImageData::from_rgba(4, 4, vec![0; 64]).unwrap();
/// let b = Avatar::new("Ada Lovelace").image(data);
/// assert_eq!(b.child_count(), 1);
/// ```
pub struct Avatar {
    /// Display name — the a11y label and the initials source.
    pub name: String,
    /// Diameter in logical points (default `32`).
    pub size: f32,
    /// `None` for a full circle; `Some(radius)` in logical points for
    /// a rounded-square silhouette.
    pub corner: Option<f32>,
    /// Image content; `None` paints the initials fallback.
    image: Option<Image>,
    /// Silhouette resolved against the last layout's scale —
    /// [`Widget::clip_shape`] has no context, so `layout` bakes it.
    clip: Shape,
    /// Cached widget bounds handed to the image child.
    cached_bounds: Rect,
    /// Shared shaped-text painter (initials fallback).
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl Avatar {
    /// Creates an initials avatar for `name` at the default size.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Avatar;
    ///
    /// let a = Avatar::new("Grace Hopper");
    /// assert_eq!(a.name, "Grace Hopper");
    /// assert_eq!(a.size, 32.0);
    /// ```
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            size: DEFAULT_SIZE,
            corner: None,
            image: None,
            clip: Shape::ELLIPSE,
            cached_bounds: Rect::default(),
            text_painter: None,
        }
    }

    /// Sets image content (an internal [`Image`] child cropped to the
    /// silhouette with [`ImageFit::Cover`]).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Avatar;
    /// use martensite_core::{ImageData, Widget};
    ///
    /// let data = ImageData::from_rgba(4, 4, vec![0; 64]).unwrap();
    /// let a = Avatar::new("Grace Hopper").image(data);
    /// assert_eq!(a.child_count(), 1);
    /// ```
    #[must_use]
    pub fn image(mut self, image: ImageData) -> Self {
        self.image = Some(Image::new(image).fit(ImageFit::Cover).alt(self.name.clone()));
        self
    }

    /// Sets the diameter in logical points.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Avatar;
    ///
    /// let a = Avatar::new("Grace Hopper").size(48.0);
    /// assert_eq!(a.size, 48.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn size(mut self, size: f32) -> Self {
        self.size = size.max(0.0);
        self
    }

    /// Switches the silhouette from a full circle to a rounded square
    /// with the given corner radius in logical points.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Avatar;
    ///
    /// let a = Avatar::new("Grace Hopper").rounded(8.0);
    /// assert_eq!(a.corner, Some(8.0));
    /// ```
    #[inline]
    #[must_use]
    pub fn rounded(mut self, radius: f32) -> Self {
        self.corner = Some(radius.max(0.0));
        self
    }

    /// Shares a [`crate::text_paint::TextPainter`] for real glyph runs
    /// in the initials fallback.
    #[must_use]
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// The avatar's silhouette as a [`Shape`] resolved at `scale`
    /// (logical pt → device px for the rounded variant).
    fn silhouette(&self, scale: f32) -> Shape {
        match self.corner {
            Some(r) => Shape::rounded(r * scale),
            None => Shape::ELLIPSE,
        }
    }

    /// First letters of the first and last whitespace-separated words
    /// of [`Avatar::name`], uppercased — "Ada Lovelace" → "AL",
    /// "madonna" → "M", "" → "".
    fn initials(&self) -> String {
        let mut firsts = self
            .name
            .split_whitespace()
            .filter_map(|word| word.chars().next());
        let first = firsts.next();
        // `None` for a single-word name — the lone initial is not
        // duplicated ("madonna" → "M", not "MM").
        let last = firsts.next_back();
        [first, last]
            .into_iter()
            .flatten()
            .flat_map(char::to_uppercase)
            .collect()
    }
}

impl Widget for Avatar {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let d = cx.pt(self.size);
        Vec2::new(
            d.min(constraints.max_size.x.max(0.0)),
            d.min(constraints.max_size.y.max(0.0)),
        )
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.cached_bounds = bounds;
        // Bake the silhouette at the layout scale — `clip_shape` /
        // `hit_shape` receive no context to resolve pt→px themselves.
        self.clip = self.silhouette(cx.scale);
        if let Some(image) = &mut self.image {
            cx.layout_child(image, bounds);
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Image);
        node.set_label(self.name.as_str());
    }

    fn paint(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        let rect = kurbo::Rect::new(
            f64::from(b.origin.x),
            f64::from(b.origin.y),
            f64::from(b.max_x()),
            f64::from(b.max_y()),
        );
        let shape = self.silhouette(cx.scale);
        if self.image.is_some() {
            // Backing disc under the image child — visible through any
            // transparent pixels. The child itself paints on top under
            // the ellipse clip.
            cx.list
                .push_fill_shape(rect, &shape, cx.color(TokenKey::AccentColor, ACCENT));
            return;
        }
        // Initials fallback: accent disc + centered initials.
        cx.list
            .push_fill_shape(rect, &shape, cx.color(TokenKey::AccentColor, ACCENT));
        cx.list.push_stroke_shape(
            rect,
            &shape,
            cx.pt(1.0),
            cx.color(TokenKey::BorderColor, EDGE),
        );
        let initials = self.initials();
        if initials.is_empty() {
            return;
        }
        let size_px = cx.pt(self.size * 0.4);
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let w = painter
            .and_then(|p| p.measure_text(&initials, size_px))
            .unwrap_or_else(|| initials.chars().count() as f32 * size_px * 0.6);
        crate::text_paint::paint_label_clipped(
            painter,
            cx.list,
            rect,
            kurbo::Point::new(
                f64::from(b.origin.x + (b.size.x - w) * 0.5),
                f64::from(b.origin.y + (b.size.y - size_px) * 0.5),
            ),
            &initials,
            size_px,
            cx.color(TokenKey::TextInverseColor, INVERSE_INK),
        );
    }

    fn clips_children(&self) -> bool {
        self.image.is_some()
    }

    fn clip_shape(&self) -> Option<Shape> {
        Some(self.clip.clone())
    }

    fn hit_shape(&self) -> Option<Shape> {
        Some(self.clip.clone())
    }

    fn child_count(&self) -> usize {
        usize::from(self.image.is_some())
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        if index == 0 {
            self.image.as_ref().map(|i| i as &dyn Widget)
        } else {
            None
        }
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        if index == 0 {
            self.image.as_mut().map(|i| i as &mut dyn Widget)
        } else {
            None
        }
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        if index == 0 && self.image.is_some() {
            Some(self.cached_bounds)
        } else {
            None
        }
    }
}

impl std::fmt::Debug for Avatar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Avatar")
            .field("name", &self.name)
            .field("size", &self.size)
            .field("corner", &self.corner)
            .field("image", &self.image.is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::{HotNode, PaintCommand, PaintList, Theme};

    fn image_4x4() -> ImageData {
        ImageData::from_rgba(4, 4, [10, 20, 30, 255].repeat(16)).unwrap()
    }

    #[test]
    fn avatar_initials_first_and_last_word() {
        assert_eq!(Avatar::new("Ada Lovelace").initials(), "AL");
        assert_eq!(Avatar::new("madonna").initials(), "M");
        assert_eq!(Avatar::new("Ada Byron King").initials(), "AK");
        assert_eq!(Avatar::new("").initials(), "");
        assert_eq!(Avatar::new("  ").initials(), "");
    }

    #[test]
    fn avatar_measures_to_diameter() {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 2.0,
        };
        let mut a = Avatar::new("Ada").size(24.0);
        let size = a.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(1000.0, 1000.0),
            },
        );
        assert_eq!(size, Vec2::splat(48.0));
    }

    #[test]
    fn avatar_initials_paints_disc_and_text() {
        let mut list = PaintList::new();
        let theme = Theme::new("test");
        let a = Avatar::new("Ada Lovelace");
        {
            let mut cx = PaintContext {
                list: &mut list,
                bounds: Rect::new(0.0, 0.0, 32.0, 32.0),
                theme: &theme,
                scale: 1.0,
                text_painter: None,
            };
            a.paint(&mut cx);
        }
        // Disc fill + ring stroke + clipped initials (clip, text, pop).
        assert_eq!(list.len(), 5);
        assert!(matches!(list.commands[0], PaintCommand::FillPath(..)));
        assert!(matches!(list.commands[1], PaintCommand::StrokePath(..)));
        assert!(matches!(list.commands[2], PaintCommand::ClipRect(_)));
        assert!(matches!(list.commands[3], PaintCommand::DrawText(..)));
        assert!(matches!(list.commands[4], PaintCommand::PopClip));
    }

    #[test]
    fn avatar_image_variant_exposes_child_and_ellipse_clip() {
        let mut a = Avatar::new("Ada").image(image_4x4());
        assert_eq!(a.child_count(), 1);
        assert!(a.clips_children());
        assert_eq!(a.clip_shape(), Some(Shape::ELLIPSE));
        assert_eq!(a.hit_shape(), Some(Shape::ELLIPSE));

        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        a.layout(&mut cx, Rect::new(4.0, 6.0, 32.0, 32.0));
        assert_eq!(a.child_bounds(0), Some(Rect::new(4.0, 6.0, 32.0, 32.0)));
        assert!(a.child(0).is_some());
    }

    #[test]
    fn avatar_rounded_updates_silhouette() {
        let mut a = Avatar::new("Ada").rounded(8.0).image(image_4x4());
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 2.0,
        };
        a.layout(&mut cx, Rect::new(0.0, 0.0, 32.0, 32.0));
        // Corner radius baked at the layout scale (8pt × 2 = 16px).
        assert_eq!(a.clip_shape(), Some(Shape::rounded(16.0)));
    }

    #[test]
    fn avatar_a11y_role_and_label() {
        let a = Avatar::new("Ada Lovelace");
        let mut node = accesskit::Node::new(accesskit::Role::Unknown);
        a.accessibility(&mut node);
        assert_eq!(node.role(), accesskit::Role::Image);
        assert_eq!(node.label(), Some("Ada Lovelace"));
    }
}
