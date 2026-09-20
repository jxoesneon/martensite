//! `Image` widget: raster image display with aspect-fit modes.
//!
//! The widget carries a decoded
//! [`ImageData`](martensite_core::paint::ImageData) — a shared, cheaply
//! clonable RGBA8 buffer — and emits a single
//! [`PaintCommand::DrawImage`](martensite_core::PaintCommand::DrawImage)
//! per frame. The destination rectangle is computed from
//! [`Image::fit`]; any overflow (e.g. `Cover` cropping) is clipped to
//! the widget's own bounds so the image can never spill onto siblings.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::image::Image;
//! use martensite_core::ImageData;
//!
//! // A 2x2 opaque red square.
//! let data = ImageData::from_rgba(2, 2, [255, 0, 0, 255].repeat(4)).unwrap();
//! let img = Image::new(data).alt("a red square");
//! assert_eq!(img.alt.as_deref(), Some("a red square"));
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::widget::{LayoutConstraints, LayoutContext, PaintContext, Widget};
use martensite_core::{ImageData, Rect};

/// How an [`Image`] scales its pixels into the layout rect.
///
/// # Examples
///
/// ```
/// use martensite::widgets::ImageFit;
///
/// // `Contain` is the default — the whole image stays visible.
/// assert_eq!(ImageFit::default(), ImageFit::Contain);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ImageFit {
    /// Scale uniformly so the whole image fits inside the rect,
    /// centered — letterboxes when the aspects differ.
    #[default]
    Contain,
    /// Scale uniformly so the image covers the whole rect, centered —
    /// crops the overflowing edges (clipped to the widget bounds).
    Cover,
    /// Stretch the image to fill the rect exactly, ignoring the
    /// source aspect ratio.
    Fill,
    /// Draw at natural pixel size, centered — no scaling; overflow is
    /// clipped to the widget bounds.
    None,
}

/// A raster image display widget.
///
/// Measures to the image's natural pixel size (clamped to the layout
/// constraints), then paints scaled into the assigned bounds according
/// to [`Image::fit`]. Accessibility exposes `Role::Image` with the
/// [`alt`](Image::alt) text as the label when supplied.
///
/// # Examples
///
/// ```
/// use martensite::widgets::Image;
/// use martensite_core::ImageData;
///
/// let data = ImageData::from_rgba(8, 4, vec![0; 8 * 4 * 4]).unwrap();
/// let img = Image::new(data).alt("placeholder");
/// assert_eq!((img.image.width(), img.image.height()), (8, 4));
/// ```
#[derive(Clone)]
pub struct Image {
    /// The decoded pixels (shared via `Arc` — cloning is cheap).
    pub image: ImageData,
    /// How the image fits the layout rect.
    pub fit: ImageFit,
    /// Accessible alternative text — becomes the a11y label.
    pub alt: Option<String>,
    /// Cached bounds from the last layout pass.
    cached_bounds: Rect,
}

impl Image {
    /// Creates an image widget from decoded [`ImageData`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Image;
    /// use martensite_core::ImageData;
    ///
    /// let data = ImageData::from_rgba(1, 1, vec![0, 0, 0, 255]).unwrap();
    /// let img = Image::new(data);
    /// assert_eq!(img.fit, martensite::widgets::ImageFit::Contain);
    /// ```
    pub fn new(image: ImageData) -> Self {
        Self {
            image,
            fit: ImageFit::default(),
            alt: None,
            cached_bounds: Rect::default(),
        }
    }

    /// Creates an image widget from tightly-packed straight-alpha RGBA8
    /// pixels (`width * height * 4` bytes), or `None` when the buffer
    /// does not match the dimensions.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Image;
    ///
    /// let img = Image::from_rgba(2, 2, vec![10, 20, 30, 255].repeat(4)).unwrap();
    /// assert_eq!((img.image.width(), img.image.height()), (2, 2));
    ///
    /// // A mismatched buffer returns `None`.
    /// assert!(Image::from_rgba(2, 2, vec![0; 3]).is_none());
    /// ```
    #[must_use]
    pub fn from_rgba(width: u32, height: u32, pixels: Vec<u8>) -> Option<Self> {
        ImageData::from_rgba(width, height, pixels).map(Self::new)
    }

    /// Sets the fit mode.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Image, ImageFit};
    /// use martensite_core::ImageData;
    ///
    /// let data = ImageData::from_rgba(1, 1, vec![0; 4]).unwrap();
    /// let img = Image::new(data).fit(ImageFit::Cover);
    /// assert_eq!(img.fit, ImageFit::Cover);
    /// ```
    #[inline]
    #[must_use]
    pub fn fit(mut self, fit: ImageFit) -> Self {
        self.fit = fit;
        self
    }

    /// Sets the accessible alternative text.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Image;
    /// use martensite_core::ImageData;
    ///
    /// let data = ImageData::from_rgba(1, 1, vec![0; 4]).unwrap();
    /// let img = Image::new(data).alt("company logo");
    /// assert_eq!(img.alt.as_deref(), Some("company logo"));
    /// ```
    #[inline]
    #[must_use]
    pub fn alt(mut self, alt: impl Into<String>) -> Self {
        self.alt = Some(alt.into());
        self
    }

    /// The destination rectangle the image pixels are drawn into, in
    /// the same device-pixel space as `bounds`, for the current
    /// [`Image::fit`]. May extend beyond `bounds` (`Cover`, `None` with
    /// a larger source) — [`Image::paint`] clips those cases itself.
    fn dest_rect(&self, bounds: Rect) -> Rect {
        let iw = self.image.width() as f32;
        let ih = self.image.height() as f32;
        let (bw, bh) = (bounds.size.x.max(0.0), bounds.size.y.max(0.0));
        if iw <= 0.0 || ih <= 0.0 {
            return Rect::new(bounds.origin.x, bounds.origin.y, 0.0, 0.0);
        }
        let (w, h) = match self.fit {
            ImageFit::Fill => return bounds,
            ImageFit::None => (iw, ih),
            ImageFit::Contain => {
                let s = (bw / iw).min(bh / ih);
                (iw * s, ih * s)
            }
            ImageFit::Cover => {
                let s = (bw / iw).max(bh / ih);
                (iw * s, ih * s)
            }
        };
        Rect::new(
            bounds.origin.x + (bw - w) * 0.5,
            bounds.origin.y + (bh - h) * 0.5,
            w,
            h,
        )
    }
}

impl Widget for Image {
    fn measure(&mut self, _cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        // Natural pixel size, clamped to the available space.
        Vec2::new(
            (self.image.width() as f32).min(constraints.max_size.x.max(0.0)),
            (self.image.height() as f32).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn layout(&mut self, _cx: &mut LayoutContext, bounds: Rect) {
        self.cached_bounds = bounds;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Image);
        if let Some(alt) = &self.alt {
            node.set_label(alt.as_str());
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        let dest = self.dest_rect(b);
        if dest.size.x <= 0.0 || dest.size.y <= 0.0 {
            return;
        }
        let dest_k = kurbo::Rect::new(
            f64::from(dest.origin.x),
            f64::from(dest.origin.y),
            f64::from(dest.max_x()),
            f64::from(dest.max_y()),
        );
        // `Cover` and an oversized `None` dest extend past the widget —
        // clip to the widget's own bounds so the image cannot spill
        // onto siblings (paint chrome is not implicitly clipped).
        let bounds_k = kurbo::Rect::new(
            f64::from(b.origin.x),
            f64::from(b.origin.y),
            f64::from(b.max_x()),
            f64::from(b.max_y()),
        );
        let clip = dest_k.x0 < bounds_k.x0
            || dest_k.y0 < bounds_k.y0
            || dest_k.x1 > bounds_k.x1
            || dest_k.y1 > bounds_k.y1;
        if clip {
            cx.list.push_clip(bounds_k);
        }
        cx.list.push_image(dest_k, self.image.clone());
        if clip {
            cx.list.pop_clip();
        }
    }
}

impl std::fmt::Debug for Image {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Image")
            .field("size", &(self.image.width(), self.image.height()))
            .field("fit", &self.fit)
            .field("alt", &self.alt)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::{HotNode, PaintCommand, PaintList, Theme};

    fn red_2x2() -> ImageData {
        ImageData::from_rgba(2, 2, [255, 0, 0, 255].repeat(4)).unwrap()
    }

    #[test]
    fn image_measures_to_natural_size() {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        let mut img = Image::from_rgba(64, 32, vec![0; 64 * 32 * 4]).unwrap();
        let size = img.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(1000.0, 1000.0),
            },
        );
        assert_eq!(size, Vec2::new(64.0, 32.0));
    }

    #[test]
    fn image_measure_clamps_to_constraints() {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        let mut img = Image::from_rgba(64, 32, vec![0; 64 * 32 * 4]).unwrap();
        let size = img.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(16.0, 16.0),
            },
        );
        assert_eq!(size, Vec2::new(16.0, 16.0));
    }

    #[test]
    fn image_contain_letterboxes() {
        let img = Image::new(red_2x2()).fit(ImageFit::Contain);
        // 2x2 into 100x50 → scale 25 → 50x50 centered horizontally.
        let d = img.dest_rect(Rect::new(0.0, 0.0, 100.0, 50.0));
        assert_eq!(d, Rect::new(25.0, 0.0, 50.0, 50.0));
    }

    #[test]
    fn image_cover_overflows_and_clips() {
        let mut list = PaintList::new();
        let theme = Theme::new("test");
        let img = Image::new(red_2x2()).fit(ImageFit::Cover);
        {
            let mut cx = PaintContext {
                list: &mut list,
                bounds: Rect::new(0.0, 0.0, 100.0, 50.0),
                theme: &theme,
                scale: 1.0,
                text_painter: None,
            };
            img.paint(&mut cx);
        }
        // Cover of a square into a 2:1 rect overflows vertically, so the
        // paint stream must wrap the DrawImage in a balanced clip pair.
        assert_eq!(list.len(), 3);
        assert!(matches!(list.commands[0], PaintCommand::ClipRect(_)));
        assert!(matches!(list.commands[1], PaintCommand::DrawImage(..)));
        assert!(matches!(list.commands[2], PaintCommand::PopClip));
    }

    #[test]
    fn image_fill_emits_unclipped() {
        let mut list = PaintList::new();
        let theme = Theme::new("test");
        let img = Image::new(red_2x2()).fit(ImageFit::Fill);
        {
            let mut cx = PaintContext {
                list: &mut list,
                bounds: Rect::new(0.0, 0.0, 100.0, 50.0),
                theme: &theme,
                scale: 1.0,
                text_painter: None,
            };
            img.paint(&mut cx);
        }
        assert_eq!(list.len(), 1);
        let PaintCommand::DrawImage(dest, _) = list.commands[0] else {
            panic!("expected DrawImage");
        };
        assert_eq!(dest, kurbo::Rect::new(0.0, 0.0, 100.0, 50.0));
    }

    #[test]
    fn image_a11y_role_and_label() {
        let img = Image::new(red_2x2()).alt("a red square");
        let mut node = accesskit::Node::new(accesskit::Role::Unknown);
        img.accessibility(&mut node);
        assert_eq!(node.role(), accesskit::Role::Image);
        assert_eq!(node.label(), Some("a red square"));

        let bare = Image::new(red_2x2());
        let mut node = accesskit::Node::new(accesskit::Role::Unknown);
        bare.accessibility(&mut node);
        assert_eq!(node.role(), accesskit::Role::Image);
        assert_eq!(node.label(), None);
    }

    #[test]
    fn image_from_rgba_rejects_bad_buffer() {
        assert!(Image::from_rgba(0, 2, Vec::new()).is_none());
        assert!(Image::from_rgba(2, 2, vec![0; 3]).is_none());
    }
}
