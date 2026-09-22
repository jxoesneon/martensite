//! `EmojiPicker` — a categorized emoji grid (chat-composer picker
//! idiom). Sections of labeled emojis; clicking a cell parks the
//! glyph in [`EmojiPicker::take_picked`]. The host inserts the
//! glyph — the picker owns no text state.
//!
//! Ships a compact built-in set via [`EmojiPicker::standard`];
//! hosts supply their own tables with
//! [`EmojiPicker::section`].
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::emoji_picker::EmojiPicker;
//!
//! let p = EmojiPicker::standard();
//! assert!(p.emoji_count() > 0);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::text_paint::SharedTextPainter;

const CELL_PT: f32 = 30.0;
const HEADER_PT: f32 = 22.0;
const EMOJI_PT: f32 = 18.0;
const FONT_PT: f32 = 10.5;
const PAD_PT: f32 = 8.0;
const COLS: usize = 8;

const HOVER: [u8; 4] = [90, 95, 110, 110];
const TEXT: [u8; 4] = [235, 237, 240, 255];
const MUTED: [u8; 4] = [150, 155, 170, 255];

/// One emoji cell: the glyph plus a short name for accessibility.
///
/// ```
/// use martensite::widgets::emoji_picker::Emoji;
///
/// let e = Emoji::new("😀", "grinning");
/// assert_eq!(e.name, "grinning");
/// ```
#[derive(Clone, Debug)]
pub struct Emoji {
    /// The emoji glyph.
    pub glyph: String,
    /// Short name (accessibility + search).
    pub name: String,
}

impl Emoji {
    /// Pairs a glyph with a name.
    ///
    /// ```
    /// use martensite::widgets::emoji_picker::Emoji;
    ///
    /// assert_eq!(Emoji::new("🎉", "party").glyph, "🎉");
    /// ```
    pub fn new(glyph: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            glyph: glyph.into(),
            name: name.into(),
        }
    }
}

/// The picker — see the module docs.
///
/// ```
/// use martensite::widgets::emoji_picker::EmojiPicker;
///
/// assert_eq!(EmojiPicker::new().emoji_count(), 0);
/// ```
pub struct EmojiPicker {
    /// Accessibility label.
    pub label: String,
    sections: Vec<(String, Vec<Emoji>)>,
    hovered: Option<usize>,
    picked: Option<String>,
    cells: Vec<Rect>,
    text_painter: Option<SharedTextPainter>,
    bounds: Rect,
    scale: f32,
}

impl std::fmt::Debug for EmojiPicker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EmojiPicker")
            .field("sections", &self.sections.len())
            .finish()
    }
}

impl Default for EmojiPicker {
    fn default() -> Self {
        Self::standard()
    }
}

impl EmojiPicker {
    /// Empty picker — add sections with [`EmojiPicker::section`].
    ///
    /// ```
    /// use martensite::widgets::emoji_picker::EmojiPicker;
    ///
    /// assert_eq!(EmojiPicker::new().emoji_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Emoji picker".to_string(),
            sections: Vec::new(),
            hovered: None,
            picked: None,
            cells: Vec::new(),
            text_painter: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// The built-in set — a compact cross-category table.
    ///
    /// ```
    /// use martensite::widgets::emoji_picker::EmojiPicker;
    ///
    /// assert!(EmojiPicker::standard().emoji_count() >= 40);
    /// ```
    pub fn standard() -> Self {
        Self::new()
            .section(
                "Smileys",
                [
                    ("😀", "grinning"),
                    ("😄", "smile"),
                    ("😂", "joy"),
                    ("😉", "wink"),
                    ("😍", "heart eyes"),
                    ("🤔", "thinking"),
                    ("😴", "sleeping"),
                    ("😭", "sob"),
                ],
            )
            .section(
                "Gestures",
                [
                    ("👍", "thumbs up"),
                    ("👎", "thumbs down"),
                    ("👏", "clap"),
                    ("🙏", "pray"),
                    ("👋", "wave"),
                    ("✌️", "victory"),
                    ("🤝", "handshake"),
                    ("💪", "muscle"),
                ],
            )
            .section(
                "Hearts",
                [
                    ("❤️", "red heart"),
                    ("🧡", "orange heart"),
                    ("💛", "yellow heart"),
                    ("💚", "green heart"),
                    ("💙", "blue heart"),
                    ("💜", "purple heart"),
                    ("🖤", "black heart"),
                    ("💔", "broken heart"),
                ],
            )
            .section(
                "Objects",
                [
                    ("🎉", "party"),
                    ("🔥", "fire"),
                    ("⭐", "star"),
                    ("💡", "idea"),
                    ("🎁", "gift"),
                    ("📌", "pin"),
                    ("🔔", "bell"),
                    ("🚀", "rocket"),
                ],
            )
            .section(
                "Nature",
                [
                    ("🐱", "cat"),
                    ("🐶", "dog"),
                    ("🦊", "fox"),
                    ("🌵", "cactus"),
                    ("🌸", "blossom"),
                    ("🌈", "rainbow"),
                    ("☀️", "sun"),
                    ("🌙", "moon"),
                ],
            )
    }

    /// Appends a named section of `(glyph, name)` pairs.
    ///
    /// ```
    /// use martensite::widgets::emoji_picker::EmojiPicker;
    ///
    /// let p = EmojiPicker::new().section("Faces", [("😀", "grinning")]);
    /// assert_eq!(p.emoji_count(), 1);
    /// ```
    pub fn section(
        mut self,
        name: impl Into<String>,
        emojis: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
    ) -> Self {
        self.sections.push((
            name.into(),
            emojis
                .into_iter()
                .map(|(g, n)| Emoji::new(g.into(), n.into()))
                .collect(),
        ));
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::emoji_picker::EmojiPicker;
    ///
    /// assert_eq!(EmojiPicker::new().label("Pick").label, "Pick");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Shared text painter for glyphs/headers.
    ///
    /// ```no_run
    /// use martensite::widgets::emoji_picker::EmojiPicker;
    /// # fn example(p: martensite::text_paint::SharedTextPainter) {
    /// let _w = EmojiPicker::standard().with_text_painter(p);
    /// # }
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Total emoji across sections.
    ///
    /// ```
    /// use martensite::widgets::emoji_picker::EmojiPicker;
    ///
    /// assert_eq!(EmojiPicker::new().emoji_count(), 0);
    /// ```
    pub fn emoji_count(&self) -> usize {
        self.sections.iter().map(|(_, e)| e.len()).sum()
    }

    /// Drains the picked glyph.
    ///
    /// ```
    /// use martensite::widgets::emoji_picker::EmojiPicker;
    ///
    /// let mut p = EmojiPicker::standard();
    /// assert_eq!(p.take_picked(), None);
    /// ```
    pub fn take_picked(&mut self) -> Option<String> {
        self.picked.take()
    }

    /// Flat index → `(section, emoji)` lookup for hit-testing.
    fn flat(&self, i: usize) -> Option<&Emoji> {
        let mut base = 0;
        for (_, emojis) in &self.sections {
            if i < base + emojis.len() {
                return Some(&emojis[i - base]);
            }
            base += emojis.len();
        }
        None
    }

    fn cell_at(&self, pos: Vec2) -> Option<usize> {
        self.cells.iter().position(|r| r.contains(pos))
    }
}

impl Widget for EmojiPicker {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let s = cx.scale;
        let cols = COLS.max(1) as f32;
        let w = cols * CELL_PT * s + PAD_PT * 2.0 * s;
        let rows: usize = self
            .sections
            .iter()
            .map(|(_, e)| e.len().div_ceil(COLS))
            .sum();
        let h = (self.sections.len() as f32 * HEADER_PT + rows as f32 * CELL_PT + PAD_PT * 2.0) * s;
        Vec2::new(
            w.min(constraints.max_size.x.max(0.0)),
            h.min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(160.0, 100.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        let s = cx.scale;
        let cell = CELL_PT * s;
        let mut y = bounds.min_y() + PAD_PT * s;
        self.cells.clear();
        for (_, emojis) in &self.sections {
            y += HEADER_PT * s;
            for (i, _) in emojis.iter().enumerate() {
                let row = (i / COLS) as f32;
                let col = (i % COLS) as f32;
                self.cells.push(Rect::new(
                    bounds.min_x() + PAD_PT * s + col * cell,
                    y + row * cell,
                    cell,
                    cell,
                ));
            }
            y += emojis.len().div_ceil(COLS) as f32 * cell;
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::ListBox);
        node.set_label(self.label.clone());
        node.set_value(format!("{} emoji", self.emoji_count()));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerMoved { position, .. } => {
                let h = self.cell_at(*position);
                if h != self.hovered {
                    self.hovered = h;
                    EventResponse::RequestRepaint
                } else {
                    EventResponse::Ignored
                }
            }
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position,
            } => {
                if let Some(i) = self.cell_at(*position) {
                    if let Some(e) = self.flat(i) {
                        self.picked = Some(e.glyph.clone());
                        return EventResponse::Handled;
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
        // Cells scrolled outside the enclosing clip emit dead commands
        // — the picker's natural height routinely exceeds its scrollport.
        let clip = cx.list.active_clip();
        let on_clip = |r: Rect| {
            clip.is_none_or(|c| {
                c.x1 > f64::from(r.min_x())
                    && c.x0 < f64::from(r.max_x())
                    && c.y1 > f64::from(r.min_y())
                    && c.y0 < f64::from(r.max_y())
            })
        };
        // Section headers — `paint_label_clipped` intersects the row
        // band with the active clip and culls off-screen headers.
        let mut y = self.bounds.min_y() + PAD_PT * s;
        for (name, emojis) in &self.sections {
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                kurbo::Rect::new(
                    f64::from(self.bounds.min_x() + PAD_PT * s),
                    f64::from(y),
                    f64::from(self.bounds.max_x()),
                    f64::from(y + HEADER_PT * s),
                ),
                kurbo::Point::new(
                    f64::from(self.bounds.min_x() + PAD_PT * s),
                    f64::from(y + HEADER_PT * 0.65 * s),
                ),
                name,
                FONT_PT * s,
                cx.color(TokenKey::TextMutedColor, MUTED),
            );
            y += HEADER_PT * s + emojis.len().div_ceil(COLS) as f32 * CELL_PT * s;
        }
        // Cells.
        let mut idx = 0;
        for (_, emojis) in &self.sections {
            for e in emojis {
                let r = self.cells[idx];
                idx += 1;
                if !on_clip(r) {
                    continue;
                }
                let kr = kurbo::Rect::new(
                    f64::from(r.min_x()),
                    f64::from(r.min_y()),
                    f64::from(r.max_x()),
                    f64::from(r.max_y()),
                );
                if self.hovered == Some(idx - 1) {
                    cx.list
                        .push_fill_shape(kr, &martensite_core::shape::Shape::ELLIPSE, HOVER);
                }
                let gsize = EMOJI_PT * s;
                let gw = painter
                    .and_then(|p| p.measure_text(&e.glyph, gsize))
                    .unwrap_or(gsize * 0.6);
                crate::text_paint::paint_label_clipped(
                    painter,
                    cx.list,
                    kr,
                    kurbo::Point::new(
                        f64::from(r.min_x() + (r.width() - gw) / 2.0),
                        f64::from(r.min_y() + r.height() / 2.0),
                    ),
                    &e.glyph,
                    gsize,
                    cx.color(TokenKey::TextColor, TEXT),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(p: &mut EmojiPicker) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        p.layout(&mut cx, Rect::new(0.0, 0.0, 300.0, 400.0));
    }

    #[test]
    fn standard_has_sections() {
        let p = EmojiPicker::standard();
        assert_eq!(p.sections.len(), 5);
        assert_eq!(p.emoji_count(), 40);
    }

    #[test]
    fn click_picks_glyph() {
        let mut p = EmojiPicker::new().section("F", [("😀", "grin"), ("🔥", "fire")]);
        laid_out(&mut p);
        let r = p.cells[1];
        p.event(&mut EventContext {
            event: &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: Vec2::new((r.min_x() + r.max_x()) / 2.0, (r.min_y() + r.max_y()) / 2.0),
            },
            bounds: p.bounds,
            scale: 1.0,
        });
        assert_eq!(p.take_picked().as_deref(), Some("🔥"));
    }

    #[test]
    fn hover_tracks_cells() {
        let mut p = EmojiPicker::standard();
        laid_out(&mut p);
        let r = p.cells[3];
        let resp = p.event(&mut EventContext {
            event: &WidgetEvent::PointerMoved {
                position: Vec2::new((r.min_x() + r.max_x()) / 2.0, (r.min_y() + r.max_y()) / 2.0),
            },
            bounds: p.bounds,
            scale: 1.0,
        });
        assert_eq!(resp, EventResponse::RequestRepaint);
        assert_eq!(p.hovered, Some(3));
    }

    #[test]
    fn paint_without_painter() {
        let mut p = EmojiPicker::standard();
        laid_out(&mut p);
        let mut list = martensite_core::PaintList::default();
        let theme = martensite_theme::Theme::new("test");
        p.paint(&mut PaintContext {
            list: &mut list,
            bounds: p.bounds,
            theme: &theme,
            scale: 1.0,
            text_painter: None,
        });
        assert!(!list.is_empty());
    }
}
