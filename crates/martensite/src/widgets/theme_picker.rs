//! `ThemePicker` — a theme gallery (GNOME Tweaks / macOS
//! Appearance idiom): each [`ThemeOption`] paints as a miniature
//! window mock — title bar, two text lines, and an accent button
//! — in the theme's colors, with the selected card ringed.
//!
//! Clicking a card parks its index in
//! [`ThemePicker::take_selected`]; `ArrowLeft`/`ArrowRight` move
//! the selection.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::theme_picker::{ThemeOption, ThemePicker};
//!
//! let t = ThemePicker::new()
//!     .option(ThemeOption::new("Light", [250; 4], [30; 4], [80, 120, 200, 255]))
//!     .option(ThemeOption::new("Dark", [30; 4], [235; 4], [120, 160, 240, 255]));
//! assert_eq!(t.option_count(), 2);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::text_paint::SharedTextPainter;

const PAD_PT: f32 = 12.0;
const GAP_PT: f32 = 12.0;
const CARD_W_PT: f32 = 110.0;
const CARD_H_PT: f32 = 84.0;
const CAPTION_PT: f32 = 18.0;
const FONT_PT: f32 = 11.0;

const FACE: [u8; 4] = [30, 32, 40, 255];
const TEXT: [u8; 4] = [235, 237, 240, 255];
const RING: [u8; 4] = [90, 140, 220, 255];

/// One selectable theme.
///
/// ```
/// use martensite::widgets::theme_picker::ThemeOption;
///
/// let o = ThemeOption::new("Dark", [30; 4], [235; 4], [90, 140, 220, 255]);
/// assert_eq!(o.name, "Dark");
/// ```
#[derive(Clone, Debug)]
pub struct ThemeOption {
    /// Display name under the preview.
    pub name: String,
    /// Window background.
    pub bg: [u8; 4],
    /// Text/foreground.
    pub fg: [u8; 4],
    /// Accent color for the mock button.
    pub accent: [u8; 4],
}

impl ThemeOption {
    /// A theme preview.
    ///
    /// ```
    /// use martensite::widgets::theme_picker::ThemeOption;
    ///
    /// assert_eq!(ThemeOption::new("T", [0; 4], [255; 4], [1; 4]).name, "T");
    /// ```
    pub fn new(name: impl Into<String>, bg: [u8; 4], fg: [u8; 4], accent: [u8; 4]) -> Self {
        Self {
            name: name.into(),
            bg,
            fg,
            accent,
        }
    }
}

/// The picker — see the module docs.
///
/// ```
/// use martensite::widgets::theme_picker::ThemePicker;
///
/// assert_eq!(ThemePicker::new().option_count(), 0);
/// ```
pub struct ThemePicker {
    /// Accessibility label.
    pub label: String,
    options: Vec<ThemeOption>,
    selected: usize,
    taken: Option<usize>,
    hovered: Option<usize>,
    cards: Vec<Rect>,
    text_painter: Option<SharedTextPainter>,
    bounds: Rect,
    scale: f32,
}

impl std::fmt::Debug for ThemePicker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ThemePicker")
            .field("options", &self.options.len())
            .field("selected", &self.selected)
            .finish()
    }
}

impl Default for ThemePicker {
    fn default() -> Self {
        Self::new()
    }
}

impl ThemePicker {
    /// Empty picker.
    ///
    /// ```
    /// use martensite::widgets::theme_picker::ThemePicker;
    ///
    /// assert_eq!(ThemePicker::new().option_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Theme".to_string(),
            options: Vec::new(),
            selected: 0,
            taken: None,
            hovered: None,
            cards: Vec::new(),
            text_painter: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Appends a theme option.
    ///
    /// ```
    /// use martensite::widgets::theme_picker::{ThemeOption, ThemePicker};
    ///
    /// assert_eq!(
    ///     ThemePicker::new().option(ThemeOption::new("T", [0; 4], [255; 4], [1; 4])).option_count(),
    ///     1
    /// );
    /// ```
    pub fn option(mut self, option: ThemeOption) -> Self {
        self.options.push(option);
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::theme_picker::ThemePicker;
    ///
    /// assert_eq!(ThemePicker::new().label("Appearance").label, "Appearance");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Shared text painter.
    ///
    /// ```no_run
    /// use martensite::widgets::theme_picker::ThemePicker;
    /// # fn example(p: martensite::text_paint::SharedTextPainter) {
    /// let _t = ThemePicker::new().with_text_painter(p);
    /// # }
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Option count.
    ///
    /// ```
    /// use martensite::widgets::theme_picker::ThemePicker;
    ///
    /// assert_eq!(ThemePicker::new().option_count(), 0);
    /// ```
    pub fn option_count(&self) -> usize {
        self.options.len()
    }

    /// Currently selected index.
    ///
    /// ```
    /// use martensite::widgets::theme_picker::ThemePicker;
    ///
    /// assert_eq!(ThemePicker::new().selection(), 0);
    /// ```
    pub fn selection(&self) -> usize {
        self.selected
    }

    /// Sets the selection host-side.
    ///
    /// ```
    /// use martensite::widgets::theme_picker::{ThemeOption, ThemePicker};
    ///
    /// let mut t = ThemePicker::new().option(ThemeOption::new("A", [0; 4], [1; 4], [2; 4]));
    /// t.set_selection(0);
    /// assert_eq!(t.selection(), 0);
    /// ```
    pub fn set_selection(&mut self, i: usize) {
        if i < self.options.len() {
            self.selected = i;
        }
    }

    /// Drains the last picked index.
    ///
    /// ```
    /// use martensite::widgets::theme_picker::ThemePicker;
    ///
    /// let mut t = ThemePicker::new();
    /// assert_eq!(t.take_selected(), None);
    /// ```
    pub fn take_selected(&mut self) -> Option<usize> {
        self.taken.take()
    }

    fn hit(&self, p: Vec2) -> Option<usize> {
        self.cards.iter().position(|r| r.contains(p))
    }
}

impl Widget for ThemePicker {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let s = cx.scale;
        let n = self.options.len().max(1) as f32;
        Vec2::new(
            ((n * (CARD_W_PT + GAP_PT) + PAD_PT * 2.0) * s).min(constraints.max_size.x.max(0.0)),
            ((CARD_H_PT + CAPTION_PT + PAD_PT * 2.0) * s).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(
            CARD_W_PT + PAD_PT * 2.0,
            CARD_H_PT + PAD_PT * 2.0,
        ))
        .with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        let s = cx.scale;
        let cw = CARD_W_PT * s;
        let ch = CARD_H_PT * s;
        let gap = GAP_PT * s;
        let total = self.options.len() as f32 * (cw + gap) - gap;
        let mut x = bounds.min_x() + (bounds.width() - total.max(cw)) / 2.0;
        self.cards.clear();
        for _ in &self.options {
            self.cards
                .push(Rect::new(x, bounds.min_y() + PAD_PT * s, cw, ch));
            x += cw + gap;
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::RadioGroup);
        node.set_label(self.label.clone());
        if let Some(o) = self.options.get(self.selected) {
            node.set_value(o.name.clone());
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerMoved { position } => {
                let h = self.hit(*position);
                if h != self.hovered {
                    self.hovered = h;
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerLeave => {
                if self.hovered.take().is_some() {
                    EventResponse::RequestRepaint
                } else {
                    EventResponse::Ignored
                }
            }
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position,
            } => {
                if let Some(i) = self.hit(*position) {
                    self.selected = i;
                    self.taken = Some(i);
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::KeyPressed { key, .. } if key.as_str() == "ArrowRight" => {
                if self.selected + 1 < self.options.len() {
                    self.selected += 1;
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::KeyPressed { key, .. } if key.as_str() == "ArrowLeft" => {
                if self.selected > 0 {
                    self.selected -= 1;
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let s = cx.scale;
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        cx.list.push_fill_rect(
            kurbo::Rect::new(
                f64::from(self.bounds.min_x()),
                f64::from(self.bounds.min_y()),
                f64::from(self.bounds.max_x()),
                f64::from(self.bounds.max_y()),
            ),
            cx.color(TokenKey::BackgroundColor, FACE),
        );
        let card_shape = martensite_core::shape::Shape::rounded(8.0 * s);
        for (i, o) in self.options.iter().enumerate() {
            let r = self.cards[i];
            let kr = kurbo::Rect::new(
                f64::from(r.min_x()),
                f64::from(r.min_y()),
                f64::from(r.max_x()),
                f64::from(r.max_y()),
            );
            // Selection ring.
            if i == self.selected {
                let w = 2.0 * s;
                cx.list.push_stroke_shape(
                    kurbo::Rect::new(
                        kr.x0 - f64::from(w),
                        kr.y0 - f64::from(w),
                        kr.x1 + f64::from(w),
                        kr.y1 + f64::from(w + CAPTION_PT * s * 0.6),
                    ),
                    &card_shape,
                    w,
                    cx.color(TokenKey::AccentColor, RING),
                );
            }
            // Mini window mock.
            cx.list.push_fill_shape(kr, &card_shape, o.bg);
            let bar_h = r.height() * 0.22;
            cx.list.push_fill_rect(
                kurbo::Rect::new(kr.x0, kr.y0, kr.x1, kr.y0 + f64::from(bar_h)),
                [o.fg[0], o.fg[1], o.fg[2], 40],
            );
            // Text lines.
            for line in 0..2 {
                let ly = kr.y0 + f64::from(bar_h) + f64::from(8.0 * s + line as f32 * 10.0 * s);
                cx.list.push_fill_rect(
                    kurbo::Rect::new(
                        kr.x0 + f64::from(8.0 * s),
                        ly,
                        kr.x0 + f64::from(r.width() * (0.7 - line as f32 * 0.2)),
                        ly + f64::from(4.0 * s),
                    ),
                    [o.fg[0], o.fg[1], o.fg[2], 160],
                );
            }
            // Accent button.
            cx.list.push_fill_shape(
                kurbo::Rect::new(
                    kr.x0 + f64::from(8.0 * s),
                    kr.y1 - f64::from(16.0 * s),
                    kr.x0 + f64::from(34.0 * s),
                    kr.y1 - f64::from(6.0 * s),
                ),
                &martensite_core::shape::Shape::rounded(3.0 * s),
                o.accent,
            );
            // Caption.
            let fs = FONT_PT * s;
            let tw = painter
                .and_then(|p| p.measure_text(&o.name, fs))
                .unwrap_or(o.name.len() as f32 * fs * 0.5);
            crate::text_paint::paint_label(
                painter,
                cx.list,
                kurbo::Point::new(
                    f64::from(r.min_x() + (r.width() - tw) / 2.0),
                    f64::from(r.max_y() + CAPTION_PT * s * 0.78),
                ),
                &o.name,
                fs,
                cx.color(TokenKey::TextColor, TEXT),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn fixture() -> ThemePicker {
        ThemePicker::new()
            .option(ThemeOption::new(
                "Light",
                [250; 4],
                [30; 4],
                [80, 120, 200, 255],
            ))
            .option(ThemeOption::new(
                "Dark",
                [30; 4],
                [235; 4],
                [120, 160, 240, 255],
            ))
            .option(ThemeOption::new(
                "Solar",
                [40, 44, 52, 255],
                [220; 4],
                [200, 160, 80, 255],
            ))
    }

    fn laid_out(t: &mut ThemePicker) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        t.layout(&mut cx, Rect::new(0.0, 0.0, 420.0, 140.0));
    }

    fn key(t: &mut ThemePicker, k: &str) {
        t.event(&mut EventContext {
            event: &WidgetEvent::KeyPressed {
                key: k.to_string(),
                repeat: false,
            },
            bounds: t.bounds,
            scale: 1.0,
        });
    }

    #[test]
    fn click_selects() {
        let mut t = fixture();
        laid_out(&mut t);
        let r = t.cards[2];
        t.event(&mut EventContext {
            event: &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: Vec2::new((r.min_x() + r.max_x()) / 2.0, (r.min_y() + r.max_y()) / 2.0),
            },
            bounds: t.bounds,
            scale: 1.0,
        });
        assert_eq!(t.selection(), 2);
        assert_eq!(t.take_selected(), Some(2));
    }

    #[test]
    fn arrows_move_selection() {
        let mut t = fixture();
        laid_out(&mut t);
        key(&mut t, "ArrowRight");
        key(&mut t, "ArrowRight");
        assert_eq!(t.selection(), 2);
        key(&mut t, "ArrowRight");
        assert_eq!(t.selection(), 2); // clamped
        key(&mut t, "ArrowLeft");
        assert_eq!(t.selection(), 1);
    }

    #[test]
    fn paint_without_painter() {
        let mut t = fixture();
        laid_out(&mut t);
        let mut list = martensite_core::PaintList::default();
        let theme = martensite_theme::Theme::new("test");
        t.paint(&mut PaintContext {
            list: &mut list,
            bounds: t.bounds,
            theme: &theme,
            scale: 1.0,
            text_painter: None,
        });
        assert!(!list.is_empty());
    }
}
