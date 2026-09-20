//! `ControlCenter` — a quick-settings panel (macOS Control Center /
//! Android QS idiom): a two-column grid of toggle tiles plus
//! full-width slider rows.
//!
//! Tile clicks toggle their state and park the item index in
//! [`ControlCenter::take_toggled`]; slider drags update the value
//! and park `(index, fraction)` in
//! [`ControlCenter::take_adjusted`].
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::control_center::ControlCenter;
//!
//! let c = ControlCenter::new().tile("📶", "Wi-Fi", true);
//! assert_eq!(c.item_count(), 1);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::text_paint::SharedTextPainter;

const PAD_PT: f32 = 10.0;
const TILE_PT: f32 = 52.0;
const ROW_PT: f32 = 44.0;
const GAP_PT: f32 = 8.0;
const GLYPH_PT: f32 = 15.0;
const CAPTION_PT: f32 = 9.5;

const FACE: [u8; 4] = [30, 32, 40, 252];
const TILE_BG: [u8; 4] = [52, 56, 66, 255];
const TRACK: [u8; 4] = [60, 63, 72, 255];
const TEXT_FG: [u8; 4] = [235, 237, 240, 255];
const MUTED_FG: [u8; 4] = [150, 154, 164, 255];

enum Item {
    Tile { on: bool },
    Slider { value: f32, dragging: bool },
}

struct Entry {
    glyph: String,
    title: String,
    item: Item,
}

/// The quick-settings panel — see the module docs.
///
/// ```
/// use martensite::widgets::control_center::ControlCenter;
///
/// assert_eq!(ControlCenter::new().item_count(), 0);
/// ```
pub struct ControlCenter {
    /// Accessibility label.
    pub label: String,
    /// Columns in the tile grid.
    pub tile_cols: usize,
    items: Vec<Entry>,
    toggled: Option<usize>,
    adjusted: Option<(usize, f32)>,
    rects: Vec<Rect>,
    text_painter: Option<SharedTextPainter>,
    bounds: Rect,
    scale: f32,
}

impl std::fmt::Debug for ControlCenter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ControlCenter")
            .field("items", &self.items.len())
            .finish()
    }
}

impl ControlCenter {
    /// An empty panel.
    ///
    /// ```
    /// use martensite::widgets::control_center::ControlCenter;
    ///
    /// assert_eq!(ControlCenter::new().item_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Control center".to_string(),
            tile_cols: 2,
            items: Vec::new(),
            toggled: None,
            adjusted: None,
            rects: Vec::new(),
            text_painter: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// A toggle tile (glyph + caption + on/off).
    ///
    /// ```
    /// use martensite::widgets::control_center::ControlCenter;
    ///
    /// assert!(ControlCenter::new().tile("📶", "Wi-Fi", true).is_on(0));
    /// ```
    pub fn tile(mut self, glyph: impl Into<String>, title: impl Into<String>, on: bool) -> Self {
        self.items.push(Entry {
            glyph: glyph.into(),
            title: title.into(),
            item: Item::Tile { on },
        });
        self
    }

    /// A slider row (glyph + caption + value).
    ///
    /// ```
    /// use martensite::widgets::control_center::ControlCenter;
    ///
    /// let c = ControlCenter::new().slider("☀", "Brightness", 0.8);
    /// assert_eq!(c.value_at(0), Some(0.8));
    /// ```
    pub fn slider(
        mut self,
        glyph: impl Into<String>,
        title: impl Into<String>,
        value: f32,
    ) -> Self {
        self.items.push(Entry {
            glyph: glyph.into(),
            title: title.into(),
            item: Item::Slider {
                value: value.clamp(0.0, 1.0),
                dragging: false,
            },
        });
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::control_center::ControlCenter;
    ///
    /// assert_eq!(ControlCenter::new().label("Settings").label, "Settings");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Shared text painter.
    ///
    /// ```no_run
    /// use martensite::widgets::control_center::ControlCenter;
    /// # fn example(p: martensite::text_paint::SharedTextPainter) {
    /// let _c = ControlCenter::new().with_text_painter(p);
    /// # }
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Item count.
    ///
    /// ```
    /// use martensite::widgets::control_center::ControlCenter;
    ///
    /// assert_eq!(ControlCenter::new().item_count(), 0);
    /// ```
    pub fn item_count(&self) -> usize {
        self.items.len()
    }

    /// Whether a tile is on (`None` for sliders / bad indices).
    ///
    /// ```
    /// use martensite::widgets::control_center::ControlCenter;
    ///
    /// assert!(!ControlCenter::new().tile("t", "T", false).is_on(0));
    /// ```
    pub fn is_on(&self, index: usize) -> bool {
        matches!(
            self.items.get(index).map(|e| &e.item),
            Some(Item::Tile { on: true })
        )
    }

    /// A slider's value.
    ///
    /// ```
    /// use martensite::widgets::control_center::ControlCenter;
    ///
    /// assert_eq!(ControlCenter::new().value_at(0), None);
    /// ```
    pub fn value_at(&self, index: usize) -> Option<f32> {
        match self.items.get(index).map(|e| &e.item) {
            Some(Item::Slider { value, .. }) => Some(*value),
            _ => None,
        }
    }

    /// Drains the last toggled tile index.
    ///
    /// ```
    /// use martensite::widgets::control_center::ControlCenter;
    ///
    /// assert_eq!(ControlCenter::new().take_toggled(), None);
    /// ```
    pub fn take_toggled(&mut self) -> Option<usize> {
        self.toggled.take()
    }

    /// Drains the last slider adjustment.
    ///
    /// ```
    /// use martensite::widgets::control_center::ControlCenter;
    ///
    /// assert_eq!(ControlCenter::new().take_adjusted(), None);
    /// ```
    pub fn take_adjusted(&mut self) -> Option<(usize, f32)> {
        self.adjusted.take()
    }

    fn hit(&self, p: Vec2) -> Option<usize> {
        self.rects.iter().position(|r| r.contains(p))
    }

    fn adjust(&mut self, i: usize, p: Vec2) {
        let r = self.rects[i];
        if let Item::Slider { value, .. } = &mut self.items[i].item {
            let track_min = r.min_x() + GLYPH_PT * self.scale + GAP_PT * self.scale;
            *value = ((p.x - track_min) / (r.max_x() - track_min)).clamp(0.0, 1.0);
            self.adjusted = Some((i, *value));
        }
    }
}

impl Default for ControlCenter {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for ControlCenter {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let s = cx.scale;
        let tiles = self
            .items
            .iter()
            .filter(|e| matches!(e.item, Item::Tile { .. }))
            .count();
        let sliders = self.items.len() - tiles;
        let tile_rows = tiles.div_ceil(self.tile_cols);
        let h = PAD_PT * 2.0
            + tile_rows as f32 * TILE_PT
            + sliders as f32 * ROW_PT
            + (self.items.len().saturating_sub(1)) as f32 * GAP_PT;
        Vec2::new(
            (300.0 * s).min(constraints.max_size.x.max(0.0)),
            (h * s).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(160.0, 60.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        let s = cx.scale;
        self.rects.clear();
        let pad = PAD_PT * s;
        let gap = GAP_PT * s;
        let cols = self.tile_cols.max(1);
        let tw = (bounds.width() - pad * 2.0 - gap * (cols - 1) as f32) / cols as f32;
        let mut y = bounds.min_y() + pad;
        let mut col = 0usize;
        for e in &self.items {
            match e.item {
                Item::Tile { .. } => {
                    let x = bounds.min_x() + pad + col as f32 * (tw + gap);
                    self.rects.push(Rect::new(x, y, tw, TILE_PT * s));
                    col += 1;
                    if col == cols {
                        col = 0;
                        y += TILE_PT * s + gap;
                    }
                }
                Item::Slider { .. } => {
                    if col != 0 {
                        col = 0;
                        y += TILE_PT * s + gap;
                    }
                    self.rects.push(Rect::new(
                        bounds.min_x() + pad,
                        y,
                        bounds.width() - pad * 2.0,
                        ROW_PT * s,
                    ));
                    y += ROW_PT * s + gap;
                }
            }
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Group);
        node.set_label(self.label.clone());
        node.set_value(format!("{} controls", self.items.len()));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                ..
            } => {
                if let Some(i) = self.hit(*position) {
                    if let Item::Slider { dragging, .. } = &mut self.items[i].item {
                        *dragging = true;
                        self.adjust(i, *position);
                    }
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerMoved { position } => {
                let dragging = self
                    .items
                    .iter()
                    .position(|e| matches!(e.item, Item::Slider { dragging: true, .. }));
                if let Some(i) = dragging {
                    self.adjust(i, *position);
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position,
            } => {
                let mut was_dragging = false;
                for e in &mut self.items {
                    if let Item::Slider { dragging, .. } = &mut e.item {
                        if *dragging {
                            *dragging = false;
                            was_dragging = true;
                        }
                    }
                }
                if was_dragging {
                    return EventResponse::RequestRepaint;
                }
                if let Some(i) = self.hit(*position) {
                    if let Item::Tile { on } = &mut self.items[i].item {
                        *on = !*on;
                        self.toggled = Some(i);
                        return EventResponse::RequestRepaint;
                    }
                }
                EventResponse::Ignored
            }
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let s = cx.scale;
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let b = self.bounds;
        let accent = cx.color(TokenKey::AccentColor, [90, 140, 220, 255]);
        cx.list.push_fill_shape(
            kurbo::Rect::new(
                f64::from(b.min_x()),
                f64::from(b.min_y()),
                f64::from(b.max_x()),
                f64::from(b.max_y()),
            ),
            &martensite_core::shape::Shape::rounded(10.0 * s),
            cx.color(TokenKey::SurfaceColor, FACE),
        );
        let gfs = GLYPH_PT * s;
        for (e, r) in self.items.iter().zip(self.rects.iter()) {
            let kr = kurbo::Rect::new(
                f64::from(r.min_x()),
                f64::from(r.min_y()),
                f64::from(r.max_x()),
                f64::from(r.max_y()),
            );
            match &e.item {
                Item::Tile { on } => {
                    cx.list.push_fill_shape(
                        kr,
                        &martensite_core::shape::Shape::rounded(8.0 * s),
                        if *on { accent } else { TILE_BG },
                    );
                    crate::text_paint::paint_label(
                        painter,
                        cx.list,
                        kurbo::Point::new(
                            f64::from(r.min_x() + GAP_PT * s),
                            f64::from(r.min_y() + gfs + GAP_PT * s * 0.5),
                        ),
                        &e.glyph,
                        gfs,
                        TEXT_FG,
                    );
                    crate::text_paint::paint_label(
                        painter,
                        cx.list,
                        kurbo::Point::new(
                            f64::from(r.min_x() + GAP_PT * s),
                            f64::from(r.max_y() - GAP_PT * s * 0.7),
                        ),
                        &e.title,
                        CAPTION_PT * s,
                        if *on { [255, 255, 255, 255] } else { MUTED_FG },
                    );
                }
                Item::Slider { value, .. } => {
                    cx.list.push_fill_shape(
                        kr,
                        &martensite_core::shape::Shape::rounded(8.0 * s),
                        TILE_BG,
                    );
                    crate::text_paint::paint_label(
                        painter,
                        cx.list,
                        kurbo::Point::new(
                            f64::from(r.min_x() + GAP_PT * s),
                            f64::from(r.min_y() + r.height() / 2.0 + gfs * 0.35),
                        ),
                        &e.glyph,
                        gfs,
                        TEXT_FG,
                    );
                    let tx = r.min_x() + GLYPH_PT * s + GAP_PT * s * 2.0;
                    let tw = r.max_x() - tx - GAP_PT * s;
                    let ty = r.min_y() + r.height() / 2.0 - 3.0 * s;
                    cx.list.push_fill_shape(
                        kurbo::Rect::new(
                            f64::from(tx),
                            f64::from(ty),
                            f64::from(tx + tw),
                            f64::from(ty + 6.0 * s),
                        ),
                        &martensite_core::shape::Shape::rounded(3.0 * s),
                        TRACK,
                    );
                    cx.list.push_fill_shape(
                        kurbo::Rect::new(
                            f64::from(tx),
                            f64::from(ty),
                            f64::from(tx + tw * value),
                            f64::from(ty + 6.0 * s),
                        ),
                        &martensite_core::shape::Shape::rounded(3.0 * s),
                        accent,
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn fixture() -> ControlCenter {
        ControlCenter::new()
            .tile("📶", "Wi-Fi", true)
            .tile("🌙", "Focus", false)
            .slider("☀", "Brightness", 0.5)
    }

    fn laid_out(c: &mut ControlCenter) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        c.layout(&mut cx, Rect::new(0.0, 0.0, 300.0, 200.0));
    }

    fn release(c: &mut ControlCenter, r: Rect) {
        c.event(&mut EventContext {
            event: &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: Vec2::new(r.min_x() + r.width() / 2.0, r.min_y() + r.height() / 2.0),
            },
            bounds: c.bounds,
            scale: 1.0,
        });
    }

    #[test]
    fn tile_toggles() {
        let mut c = fixture();
        laid_out(&mut c);
        let r = c.rects[1];
        release(&mut c, r);
        assert_eq!(c.take_toggled(), Some(1));
        assert!(c.is_on(1));
        assert!(c.is_on(0));
    }

    #[test]
    fn slider_drag_adjusts() {
        let mut c = fixture();
        laid_out(&mut c);
        let r = c.rects[2];
        c.event(&mut EventContext {
            event: &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(r.min_x() + r.width() * 0.9, r.min_y() + r.height() / 2.0),
                count: 1,
            },
            bounds: c.bounds,
            scale: 1.0,
        });
        assert!(c.value_at(2).unwrap() > 0.5);
        assert!(c.take_adjusted().is_some());
    }

    #[test]
    fn release_ends_drag_without_toggle() {
        let mut c = fixture();
        laid_out(&mut c);
        let r = c.rects[2];
        c.event(&mut EventContext {
            event: &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(r.min_x() + 40.0, r.min_y() + r.height() / 2.0),
                count: 1,
            },
            bounds: c.bounds,
            scale: 1.0,
        });
        release(&mut c, r);
        assert_eq!(c.take_toggled(), None);
    }

    #[test]
    fn paint_without_painter() {
        let mut c = fixture();
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
