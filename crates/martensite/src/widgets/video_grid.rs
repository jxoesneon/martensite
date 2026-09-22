//! `VideoGrid` — a conference participant grid (Zoom/Meet idiom):
//! equal tiles laid out in columns, each a [`Participant`] —
//! color swatch, name caption, accent speaking ring, and a muted
//! badge.
//!
//! Clicking a tile parks its index in
//! [`VideoGrid::take_selected`]; `set_speaking`/`set_muted` update
//! tile state host-side. Companion to [`CallControls`].
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::video_grid::{Participant, VideoGrid};
//!
//! let v = VideoGrid::new()
//!     .participant(Participant::new("Ana", [90, 140, 200, 255]))
//!     .participant(Participant::new("Ben", [200, 140, 90, 255]));
//! assert_eq!(v.participant_count(), 2);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::text_paint::SharedTextPainter;

const GAP_PT: f32 = 8.0;
const PAD_PT: f32 = 10.0;
const TILE_MIN_PT: f32 = 120.0;
const CAPTION_PT: f32 = 20.0;
const FONT_PT: f32 = 11.0;
const INITIAL_PT: f32 = 28.0;

const FACE: [u8; 4] = [30, 32, 40, 255];
const SPEAKING: [u8; 4] = [90, 200, 120, 255];
const BADGE: [u8; 4] = [60, 63, 74, 230];
const TEXT: [u8; 4] = [235, 237, 240, 255];
const CAPTION_BG: [u8; 4] = [0, 0, 0, 110];

/// One participant tile.
///
/// ```
/// use martensite::widgets::video_grid::Participant;
///
/// let p = Participant::new("Ana", [1; 4]).muted(true);
/// assert!(p.muted);
/// ```
#[derive(Clone, Debug)]
pub struct Participant {
    /// Display name.
    pub name: String,
    /// Tile color (stands in for the video feed).
    pub color: [u8; 4],
    /// Active-speaker ring.
    pub speaking: bool,
    /// Muted badge.
    pub muted: bool,
}

impl Participant {
    /// A participant with a tile color.
    ///
    /// ```
    /// use martensite::widgets::video_grid::Participant;
    ///
    /// assert_eq!(Participant::new("A", [1; 4]).name, "A");
    /// ```
    pub fn new(name: impl Into<String>, color: [u8; 4]) -> Self {
        Self {
            name: name.into(),
            color,
            speaking: false,
            muted: false,
        }
    }

    /// Muted flag.
    ///
    /// ```
    /// use martensite::widgets::video_grid::Participant;
    ///
    /// assert!(Participant::new("A", [1; 4]).muted(true).muted);
    /// ```
    pub fn muted(mut self, muted: bool) -> Self {
        self.muted = muted;
        self
    }

    /// Speaking flag.
    ///
    /// ```
    /// use martensite::widgets::video_grid::Participant;
    ///
    /// assert!(Participant::new("A", [1; 4]).speaking(true).speaking);
    /// ```
    pub fn speaking(mut self, speaking: bool) -> Self {
        self.speaking = speaking;
        self
    }
}

/// The grid — see the module docs.
///
/// ```
/// use martensite::widgets::video_grid::VideoGrid;
///
/// assert_eq!(VideoGrid::new().participant_count(), 0);
/// ```
pub struct VideoGrid {
    /// Accessibility label.
    pub label: String,
    /// Preferred tile width (columns derive from it).
    pub tile_min: f32,
    participants: Vec<Participant>,
    selected: Option<usize>,
    tiles: Vec<Rect>,
    /// Uniform scale applied when the natural grid is taller than the
    /// allotted bounds — keeps every tile inside the widget instead of
    /// overflowing into siblings below.
    fit: f32,
    text_painter: Option<SharedTextPainter>,
    bounds: Rect,
    scale: f32,
}

impl std::fmt::Debug for VideoGrid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VideoGrid")
            .field("participants", &self.participants.len())
            .finish()
    }
}

impl Default for VideoGrid {
    fn default() -> Self {
        Self::new()
    }
}

impl VideoGrid {
    /// Empty grid.
    ///
    /// ```
    /// use martensite::widgets::video_grid::VideoGrid;
    ///
    /// assert_eq!(VideoGrid::new().participant_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Participants".to_string(),
            tile_min: TILE_MIN_PT,
            participants: Vec::new(),
            selected: None,
            tiles: Vec::new(),
            fit: 1.0,
            text_painter: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Appends a participant.
    ///
    /// ```
    /// use martensite::widgets::video_grid::{Participant, VideoGrid};
    ///
    /// assert_eq!(VideoGrid::new().participant(Participant::new("A", [1; 4])).participant_count(), 1);
    /// ```
    pub fn participant(mut self, p: Participant) -> Self {
        self.participants.push(p);
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::video_grid::VideoGrid;
    ///
    /// assert_eq!(VideoGrid::new().label("Call").label, "Call");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Shared text painter for captions.
    ///
    /// ```no_run
    /// use martensite::widgets::video_grid::VideoGrid;
    /// # fn example(p: martensite::text_paint::SharedTextPainter) {
    /// let _v = VideoGrid::new().with_text_painter(p);
    /// # }
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Participant count.
    ///
    /// ```
    /// use martensite::widgets::video_grid::VideoGrid;
    ///
    /// assert_eq!(VideoGrid::new().participant_count(), 0);
    /// ```
    pub fn participant_count(&self) -> usize {
        self.participants.len()
    }

    /// Column count for the current width.
    ///
    /// ```
    /// use martensite::widgets::video_grid::VideoGrid;
    ///
    /// assert!(VideoGrid::new().columns() >= 1);
    /// ```
    pub fn columns(&self) -> usize {
        let w = self.bounds.width() - PAD_PT * 2.0 * self.scale;
        let cell = (self.tile_min + GAP_PT) * self.scale;
        ((w / cell.max(1.0)) as usize).max(1)
    }

    /// Sets the speaking flag host-side.
    ///
    /// ```
    /// use martensite::widgets::video_grid::{Participant, VideoGrid};
    ///
    /// let mut v = VideoGrid::new().participant(Participant::new("A", [1; 4]));
    /// v.set_speaking(0, true);
    /// assert!(v.is_speaking(0));
    /// ```
    pub fn set_speaking(&mut self, i: usize, speaking: bool) {
        if let Some(p) = self.participants.get_mut(i) {
            p.speaking = speaking;
        }
    }

    /// Sets the muted flag host-side.
    ///
    /// ```
    /// use martensite::widgets::video_grid::{Participant, VideoGrid};
    ///
    /// let mut v = VideoGrid::new().participant(Participant::new("A", [1; 4]));
    /// v.set_muted(0, true);
    /// assert!(v.is_muted(0));
    /// ```
    pub fn set_muted(&mut self, i: usize, muted: bool) {
        if let Some(p) = self.participants.get_mut(i) {
            p.muted = muted;
        }
    }

    /// Speaking flag for a tile.
    ///
    /// ```
    /// use martensite::widgets::video_grid::VideoGrid;
    ///
    /// assert!(!VideoGrid::new().is_speaking(0));
    /// ```
    pub fn is_speaking(&self, i: usize) -> bool {
        self.participants.get(i).is_some_and(|p| p.speaking)
    }

    /// Muted flag for a tile.
    ///
    /// ```
    /// use martensite::widgets::video_grid::VideoGrid;
    ///
    /// assert!(!VideoGrid::new().is_muted(0));
    /// ```
    pub fn is_muted(&self, i: usize) -> bool {
        self.participants.get(i).is_some_and(|p| p.muted)
    }

    /// Drains the last clicked tile index.
    ///
    /// ```
    /// use martensite::widgets::video_grid::VideoGrid;
    ///
    /// let mut v = VideoGrid::new();
    /// assert_eq!(v.take_selected(), None);
    /// ```
    pub fn take_selected(&mut self) -> Option<usize> {
        self.selected.take()
    }
}

impl Widget for VideoGrid {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let s = cx.scale;
        let cols = self.columns().max(1);
        let rows = self.participants.len().div_ceil(cols).max(1) as f32;
        let h = (rows * (self.tile_min * 0.75 + CAPTION_PT)
            + (rows - 1.0).max(0.0) * GAP_PT
            + PAD_PT * 2.0)
            * s;
        Vec2::new(
            (self.tile_min * 2.0 * s).min(constraints.max_size.x.max(0.0)),
            h.min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(160.0, 120.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        let s = cx.scale;
        let gap = GAP_PT * s;
        let cols = self.columns();
        let cell_w =
            ((bounds.width() - PAD_PT * 2.0 * s - (cols - 1) as f32 * gap) / cols as f32).max(0.0);
        let cell_h = cell_w * 0.75 + CAPTION_PT * s;
        let rows = self.participants.len().div_ceil(cols).max(1) as f32;
        let natural_h = PAD_PT * 2.0 * s + rows * cell_h + (rows - 1.0).max(0.0) * gap;
        // Tiles derive height from width (16:9 + caption), so a tall
        // roster overflows a short band. Shrink the whole grid —
        // centered horizontally — rather than letting tiles paint into
        // the sibling below.
        self.fit = if natural_h > bounds.height() && natural_h > 0.0 {
            (bounds.height() / natural_h).max(0.0)
        } else {
            1.0
        };
        let fit = self.fit;
        let grid_w = (PAD_PT * 2.0 * s + cols as f32 * cell_w + (cols - 1) as f32 * gap) * fit;
        let x0 = bounds.min_x() + (bounds.width() - grid_w).max(0.0) / 2.0;
        let y0 = bounds.min_y() + PAD_PT * s * fit;
        self.tiles.clear();
        for (i, _) in self.participants.iter().enumerate() {
            let row = (i / cols) as f32;
            let col = (i % cols) as f32;
            self.tiles.push(Rect::new(
                x0 + col * (cell_w + gap) * fit,
                y0 + row * (cell_h + gap) * fit,
                cell_w * fit,
                cell_h * fit,
            ));
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Grid);
        node.set_label(self.label.clone());
        node.set_value(format!("{} participants", self.participants.len()));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if let WidgetEvent::PointerReleased {
            button: PointerButton::Primary,
            position,
        } = cx.event
        {
            if let Some(i) = self.tiles.iter().position(|r| r.contains(*position)) {
                self.selected = Some(i);
                return EventResponse::RequestRepaint;
            }
        }
        EventResponse::Ignored
    }

    fn paint(&self, cx: &mut PaintContext) {
        let s = cx.scale;
        let fit = self.fit;
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let wb = kurbo::Rect::new(
            f64::from(self.bounds.min_x()),
            f64::from(self.bounds.min_y()),
            f64::from(self.bounds.max_x()),
            f64::from(self.bounds.max_y()),
        );
        cx.list
            .push_fill_rect(wb, cx.color(TokenKey::BackgroundColor, FACE));
        // Speaking rings bleed 2.5pt past the tile edge — clip the
        // whole grid to bounds so nothing reaches a sibling's rect.
        cx.list.push_clip(wb);
        let shape = martensite_core::shape::Shape::rounded(8.0 * s * fit);
        for (i, p) in self.participants.iter().enumerate() {
            let r = self.tiles[i];
            let kr = kurbo::Rect::new(
                f64::from(r.min_x()),
                f64::from(r.min_y()),
                f64::from(r.max_x()),
                f64::from(r.max_y()),
            );
            // Tile.
            cx.list.push_fill_shape(kr, &shape, p.color);
            // Speaking ring.
            if p.speaking {
                let w = 2.5 * s * fit;
                cx.list.push_stroke_shape(
                    kurbo::Rect::new(
                        kr.x0 - f64::from(w),
                        kr.y0 - f64::from(w),
                        kr.x1 + f64::from(w),
                        kr.y1 + f64::from(w),
                    ),
                    &shape,
                    w,
                    cx.color(TokenKey::SuccessColor, SPEAKING),
                );
            }
            // Center initial.
            let initial: String = p.name.chars().take(1).collect();
            let fs = INITIAL_PT * s * fit;
            let iw = painter
                .and_then(|pt| pt.measure_text(&initial, fs))
                .unwrap_or(fs * 0.5);
            let cap = CAPTION_PT * s * fit;
            crate::text_paint::paint_label(
                painter,
                cx.list,
                kurbo::Point::new(
                    f64::from(r.min_x() + (r.width() - iw) / 2.0),
                    f64::from(r.min_y() + (r.height() - cap) / 2.0),
                ),
                &initial,
                fs,
                TEXT,
            );
            // Caption band.
            let cr = kurbo::Rect::new(kr.x0, kr.y1 - f64::from(cap), kr.x1, kr.y1);
            cx.list.push_fill_rect(cr, CAPTION_BG);
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                cr,
                kurbo::Point::new(
                    f64::from(r.min_x() + 6.0 * s * fit),
                    f64::from(r.max_y() - cap * 0.3),
                ),
                &p.name,
                FONT_PT * s * fit,
                cx.color(TokenKey::TextColor, TEXT),
            );
            // Muted badge.
            if p.muted {
                let d = 18.0 * s * fit;
                let br = kurbo::Rect::new(
                    f64::from(r.max_x() - d - 5.0 * s * fit),
                    f64::from(r.max_y() - cap - d - 5.0 * s * fit),
                    f64::from(r.max_x() - 5.0 * s * fit),
                    f64::from(r.max_y() - cap - 5.0 * s * fit),
                );
                cx.list
                    .push_fill_shape(br, &martensite_core::shape::Shape::ELLIPSE, BADGE);
                crate::text_paint::paint_label(
                    painter,
                    cx.list,
                    kurbo::Point::new(br.x0 + f64::from(d) * 0.28, br.y0 + f64::from(d) * 0.72),
                    "✕",
                    FONT_PT * s * fit,
                    TEXT,
                );
            }
        }
        cx.list.pop_clip();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn fixture() -> VideoGrid {
        VideoGrid::new()
            .participant(Participant::new("Ana", [90, 140, 200, 255]))
            .participant(Participant::new("Ben", [200, 140, 90, 255]).muted(true))
            .participant(Participant::new("Cat", [140, 200, 90, 255]).speaking(true))
    }

    fn laid_out(v: &mut VideoGrid) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        v.layout(&mut cx, Rect::new(0.0, 0.0, 480.0, 240.0));
    }

    #[test]
    fn grid_flows_rows() {
        let mut v = fixture();
        laid_out(&mut v);
        assert!(v.tiles[1].min_x() > v.tiles[0].min_x());
        assert_eq!(v.tiles.len(), 3);
    }

    #[test]
    fn click_selects_tile() {
        let mut v = fixture();
        laid_out(&mut v);
        let r = v.tiles[2];
        v.event(&mut EventContext {
            event: &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: Vec2::new((r.min_x() + r.max_x()) / 2.0, (r.min_y() + r.max_y()) / 2.0),
            },
            bounds: v.bounds,
            scale: 1.0,
        });
        assert_eq!(v.take_selected(), Some(2));
    }

    #[test]
    fn state_setters() {
        let mut v = fixture();
        v.set_muted(0, true);
        v.set_speaking(0, true);
        assert!(v.is_muted(0));
        assert!(v.is_speaking(0));
        assert!(v.is_muted(1));
        assert!(!v.is_speaking(1));
    }

    #[test]
    fn paint_without_painter() {
        let mut v = fixture();
        laid_out(&mut v);
        let mut list = martensite_core::PaintList::default();
        let theme = martensite_theme::Theme::new("test");
        v.paint(&mut PaintContext {
            list: &mut list,
            bounds: v.bounds,
            theme: &theme,
            scale: 1.0,
            text_painter: None,
        });
        assert!(!list.is_empty());
    }
}
