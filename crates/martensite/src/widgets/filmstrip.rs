//! `Filmstrip` — a horizontal strip of thumbnails with selection
//! (the photo-editor filmstrip / gallery picker idiom).
//!
//! Tiles show an optional [`ImageData`](martensite_core::paint::ImageData)
//! image (or a flat color
//! fallback) plus a caption. Click selects and parks the index in
//! [`Filmstrip::take_selected`]; `←`/`→` move the selection; the
//! wheel scrolls the strip when it overflows.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::filmstrip::{Filmstrip, Thumbnail};
//!
//! let mut f = Filmstrip::new()
//!     .thumb(Thumbnail::new("DSC_001", [80, 120, 200, 255]))
//!     .thumb(Thumbnail::new("DSC_002", [200, 120, 80, 255]));
//! assert_eq!(f.thumb_count(), 2);
//! f.select(1);
//! assert_eq!(f.selected(), Some(1));
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, ImageData, LayoutConstraints, LayoutContext, PaintContext,
    PointerButton, Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;
use parking_lot::Mutex;

const TILE_PT: f32 = 72.0;
const CAPTION_PT: f32 = 16.0;
const GAP_PT: f32 = 6.0;
const PAD_PT: f32 = 8.0;

const FACE: [u8; 4] = [24, 24, 28, 255];
const TILE_BG: [u8; 4] = [40, 40, 46, 255];
const TEXT: [u8; 4] = [200, 200, 206, 255];
const ACCENT: [u8; 4] = [96, 165, 250, 255];

/// One thumbnail tile — see [`Filmstrip`].
///
/// ```
/// use martensite::widgets::filmstrip::Thumbnail;
///
/// let t = Thumbnail::new("shot", [100, 100, 200, 255]);
/// assert_eq!(t.label, "shot");
/// ```
#[derive(Debug, Clone)]
pub struct Thumbnail {
    /// Caption under the tile.
    pub label: String,
    /// Fallback tile color when `image` is `None`.
    pub color: [u8; 4],
    /// Optional decoded image.
    pub image: Option<ImageData>,
}

impl Thumbnail {
    /// A color-backed tile.
    ///
    /// ```
    /// use martensite::widgets::filmstrip::Thumbnail;
    ///
    /// assert!(Thumbnail::new("x", [0, 0, 0, 255]).image.is_none());
    /// ```
    pub fn new(label: impl Into<String>, color: [u8; 4]) -> Self {
        Self {
            label: label.into(),
            color,
            image: None,
        }
    }

    /// A tile backed by a decoded image.
    ///
    /// ```
    /// use martensite::widgets::filmstrip::Thumbnail;
    /// use martensite_core::ImageData;
    ///
    /// let img = ImageData::from_rgba(1, 1, vec![0, 0, 0, 255]).unwrap();
    /// assert!(Thumbnail::new("x", [0, 0, 0, 255]).image(img).image.is_some());
    /// ```
    pub fn image(mut self, image: ImageData) -> Self {
        self.image = Some(image);
        self
    }
}

/// A thumbnail strip — see the module docs.
///
/// ```
/// use martensite::widgets::filmstrip::Filmstrip;
///
/// assert_eq!(Filmstrip::new().thumb_count(), 0);
/// ```
pub struct Filmstrip {
    /// Accessibility label.
    pub label: String,
    /// Tiles, left to right.
    thumbs: Vec<Thumbnail>,
    selected: Option<usize>,
    picked: Option<usize>,
    scroll: f32,
    bounds: Rect,
    scale: f32,
    /// Tile rects painted last frame.
    hits: Mutex<Vec<(usize, Rect)>>,
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl std::fmt::Debug for Filmstrip {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Filmstrip")
            .field("thumbs", &self.thumbs.len())
            .field("selected", &self.selected)
            .finish()
    }
}

impl Default for Filmstrip {
    fn default() -> Self {
        Self::new()
    }
}

impl Filmstrip {
    /// Creates an empty strip.
    ///
    /// ```
    /// use martensite::widgets::filmstrip::Filmstrip;
    ///
    /// assert_eq!(Filmstrip::new().thumb_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Filmstrip".to_string(),
            thumbs: Vec::new(),
            selected: None,
            picked: None,
            scroll: 0.0,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
            hits: Mutex::new(Vec::new()),
            text_painter: None,
        }
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::filmstrip::Filmstrip;
    ///
    /// assert_eq!(Filmstrip::new().label("Imports").label, "Imports");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Custom text painter.
    ///
    /// ```
    /// use martensite::widgets::filmstrip::Filmstrip;
    ///
    /// let _ = Filmstrip::new(); // painter optional
    /// ```
    pub fn with_text_painter(mut self, p: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(p);
        self
    }

    /// Appends a thumbnail.
    ///
    /// ```
    /// use martensite::widgets::filmstrip::{Filmstrip, Thumbnail};
    ///
    /// let f = Filmstrip::new().thumb(Thumbnail::new("a", [0, 0, 0, 255]));
    /// assert_eq!(f.thumb_count(), 1);
    /// ```
    pub fn thumb(mut self, t: Thumbnail) -> Self {
        self.thumbs.push(t);
        self
    }

    /// Thumbnail count.
    ///
    /// ```
    /// use martensite::widgets::filmstrip::Filmstrip;
    ///
    /// assert_eq!(Filmstrip::new().thumb_count(), 0);
    /// ```
    pub fn thumb_count(&self) -> usize {
        self.thumbs.len()
    }

    /// Selected index.
    ///
    /// ```
    /// use martensite::widgets::filmstrip::Filmstrip;
    ///
    /// assert_eq!(Filmstrip::new().selected(), None);
    /// ```
    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    /// Selects index `i` (clamped).
    ///
    /// ```
    /// use martensite::widgets::filmstrip::{Filmstrip, Thumbnail};
    ///
    /// let mut f = Filmstrip::new().thumb(Thumbnail::new("a", [0, 0, 0, 255]));
    /// f.select(0);
    /// assert_eq!(f.selected(), Some(0));
    /// ```
    pub fn select(&mut self, i: usize) {
        if i < self.thumbs.len() {
            self.selected = Some(i);
        }
    }

    /// Drains the index of the last clicked tile.
    ///
    /// ```
    /// use martensite::widgets::filmstrip::Filmstrip;
    ///
    /// assert_eq!(Filmstrip::new().take_selected(), None);
    /// ```
    pub fn take_selected(&mut self) -> Option<usize> {
        self.picked.take()
    }

    /// Content width.
    fn content_w(&self) -> f32 {
        let s = self.scale;
        self.thumbs.len() as f32 * (TILE_PT + GAP_PT) * s + PAD_PT * s
    }

    /// Max scroll offset.
    fn max_scroll(&self) -> f32 {
        (self.content_w() - self.bounds.width()).max(0.0)
    }
}

impl Widget for Filmstrip {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            cx.pt(320.0).min(constraints.max_size.x.max(0.0)),
            cx.pt(TILE_PT + CAPTION_PT + PAD_PT * 2.0)
                .min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(90.0, 60.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        self.scroll = self.scroll.clamp(0.0, self.max_scroll());
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::List);
        node.set_label(format!("{} — {} thumbnails", self.label, self.thumbs.len()));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::Scroll { position, delta } => {
                if self.bounds.contains(*position) {
                    self.scroll = (self.scroll - delta.x - delta.y).clamp(0.0, self.max_scroll());
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                ..
            } => {
                if !self.bounds.contains(*position) {
                    return EventResponse::Ignored;
                }
                let hits = self.hits.lock();
                if let Some((i, _)) = hits.iter().find(|(_, r)| r.contains(*position)) {
                    let i = *i;
                    drop(hits);
                    self.selected = Some(i);
                    self.picked = Some(i);
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Handled
            }
            WidgetEvent::KeyPressed { key, .. } => {
                let next = match key.as_str() {
                    "ArrowRight" => self
                        .selected
                        .map_or(0, |s| (s + 1).min(self.thumbs.len().saturating_sub(1))),
                    "ArrowLeft" => self.selected.map_or(0, |s| s.saturating_sub(1)),
                    _ => return EventResponse::Ignored,
                };
                if !self.thumbs.is_empty() {
                    self.selected = Some(next);
                    self.picked = Some(next);
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let krect = |r: Rect| {
            kurbo::Rect::new(
                f64::from(r.min_x()),
                f64::from(r.min_y()),
                f64::from(r.max_x()),
                f64::from(r.max_y()),
            )
        };
        cx.list.push_fill_shape(
            krect(self.bounds),
            &martensite_core::shape::Shape::RECT,
            cx.color(TokenKey::SurfaceColor, FACE),
        );
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let s = self.scale;
        let pad = PAD_PT * s;
        let tile = TILE_PT * s;
        let gap = GAP_PT * s;
        let cap_h = CAPTION_PT * s;
        let cap_sz = 9.5 * s;
        let mut hits = self.hits.lock();
        hits.clear();
        cx.list.push_clip(krect(self.bounds));
        for (i, t) in self.thumbs.iter().enumerate() {
            let x = self.bounds.min_x() + pad + i as f32 * (tile + gap) - self.scroll;
            if x + tile < self.bounds.min_x() || x > self.bounds.max_x() {
                continue;
            }
            let r = Rect::new(x, self.bounds.min_y() + pad, tile, tile);
            hits.push((i, r));
            let kr = krect(r);
            cx.list.push_fill_shape(
                kr,
                &martensite_core::shape::Shape::rounded(4.0 * s),
                t.color,
            );
            if let Some(img) = &t.image {
                cx.list.push_image(kr, img.clone());
            } else {
                cx.list.push_stroke_shape(
                    kr,
                    &martensite_core::shape::Shape::rounded(4.0 * s),
                    1.0 * s,
                    cx.color(TokenKey::BorderColor, TILE_BG),
                );
            }
            if self.selected == Some(i) {
                cx.list.push_stroke_shape(
                    kr,
                    &martensite_core::shape::Shape::rounded(4.0 * s),
                    2.0 * s,
                    cx.color(TokenKey::AccentColor, ACCENT),
                );
            }
            if !t.label.is_empty() {
                crate::text_paint::paint_label_clipped(
                    painter,
                    cx.list,
                    kr,
                    kurbo::Point::new(
                        f64::from(x + 2.0 * s),
                        f64::from(self.bounds.min_y() + pad + tile + 3.0 * s),
                    ),
                    &t.label,
                    cap_sz,
                    cx.color(TokenKey::TextMutedColor, TEXT),
                );
            }
            let _ = cap_h;
        }
        cx.list.pop_clip();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;
    use martensite_core::PaintList;

    fn laid_out(w: &mut Filmstrip, wd: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        w.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(wd, h),
            },
        );
        w.layout(&mut cx, Rect::new(0.0, 0.0, wd, h));
    }

    fn painted(w: &Filmstrip) {
        let theme = martensite_theme::Theme::new("test");
        let mut list = PaintList::new();
        let mut cx = PaintContext {
            list: &mut list,
            bounds: w.bounds,
            scale: 1.0,
            theme: &theme,
            text_painter: None,
        };
        w.paint(&mut cx);
    }

    #[test]
    fn click_selects() {
        let mut f = Filmstrip::new()
            .thumb(Thumbnail::new("a", [10, 10, 10, 255]))
            .thumb(Thumbnail::new("b", [20, 20, 20, 255]));
        laid_out(&mut f, 320.0, 96.0);
        painted(&f);
        let tile = f.hits.lock()[1].1;
        f.event(&mut EventContext {
            event: &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(
                    (tile.min_x() + tile.max_x()) / 2.0,
                    (tile.min_y() + tile.max_y()) / 2.0,
                ),
                count: 1,
            },
            bounds: f.bounds,
            scale: 1.0,
        });
        assert_eq!(f.selected(), Some(1));
        assert_eq!(f.take_selected(), Some(1));
        assert_eq!(f.take_selected(), None);
    }

    #[test]
    fn arrows_move_selection() {
        let mut f = Filmstrip::new()
            .thumb(Thumbnail::new("a", [0, 0, 0, 255]))
            .thumb(Thumbnail::new("b", [0, 0, 0, 255]));
        laid_out(&mut f, 320.0, 96.0);
        f.event(&mut EventContext {
            event: &WidgetEvent::KeyPressed {
                key: "ArrowRight".to_string(),
                repeat: false,
            },
            bounds: f.bounds,
            scale: 1.0,
        });
        assert_eq!(f.selected(), Some(0));
        f.event(&mut EventContext {
            event: &WidgetEvent::KeyPressed {
                key: "ArrowRight".to_string(),
                repeat: false,
            },
            bounds: f.bounds,
            scale: 1.0,
        });
        assert_eq!(f.selected(), Some(1));
    }

    #[test]
    fn scroll_clamps() {
        let mut f = Filmstrip::new();
        for i in 0..20 {
            f = f.thumb(Thumbnail::new(format!("t{i}"), [0, 0, 0, 255]));
        }
        laid_out(&mut f, 320.0, 96.0);
        f.event(&mut EventContext {
            event: &WidgetEvent::Scroll {
                position: Vec2::new(160.0, 48.0),
                delta: Vec2::new(0.0, -9999.0),
            },
            bounds: f.bounds,
            scale: 1.0,
        });
        assert_eq!(f.scroll, f.max_scroll());
    }
}
