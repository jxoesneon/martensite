//! `ClipboardHistory` — a clipboard manager's history panel
//! (Windows `Win+V`, Paste, CopyQ idiom): newest-first rows of
//! copied snippets, pinned entries immune to eviction, and a
//! paste seam.
//!
//! [`ClipboardHistory::push`] inserts at the top and evicts the
//! oldest *unpinned* entry past `max_entries`. Clicking a row
//! parks its index in [`ClipboardHistory::take_pasted`] for the
//! host to inject; `Ctrl+P`-style pinning is host-driven via
//! [`ClipboardHistory::set_pinned`].
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::clipboard_history::ClipboardHistory;
//!
//! let mut h = ClipboardHistory::new();
//! h.push("hello");
//! h.push("world");
//! assert_eq!(h.entry(0).unwrap().text, "world"); // newest first
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::text_paint::SharedTextPainter;

const ROW_PT: f32 = 30.0;
const PAD_PT: f32 = 6.0;
const FONT_PT: f32 = 12.0;
const PIN_PT: f32 = 14.0;

const FACE: [u8; 4] = [30, 32, 40, 255];
const ROW_HOVER: [u8; 4] = [255, 255, 255, 14];
const TEXT: [u8; 4] = [235, 237, 240, 255];
const MUTED_FG: [u8; 4] = [150, 154, 164, 255];
const PIN: [u8; 4] = [240, 190, 80, 255];

/// One history entry.
///
/// ```
/// use martensite::widgets::clipboard_history::ClipEntry;
///
/// let e = ClipEntry::new("snippet").pinned(true);
/// assert!(e.pinned);
/// ```
#[derive(Clone, Debug)]
pub struct ClipEntry {
    /// Copied text.
    pub text: String,
    /// Pinned entries are never evicted.
    pub pinned: bool,
}

impl ClipEntry {
    /// A text entry.
    ///
    /// ```
    /// use martensite::widgets::clipboard_history::ClipEntry;
    ///
    /// assert_eq!(ClipEntry::new("x").text, "x");
    /// ```
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            pinned: false,
        }
    }

    /// Pinned flag.
    ///
    /// ```
    /// use martensite::widgets::clipboard_history::ClipEntry;
    ///
    /// assert!(ClipEntry::new("x").pinned(true).pinned);
    /// ```
    pub fn pinned(mut self, pinned: bool) -> Self {
        self.pinned = pinned;
        self
    }
}

/// The history panel — see the module docs.
///
/// ```
/// use martensite::widgets::clipboard_history::ClipboardHistory;
///
/// assert_eq!(ClipboardHistory::new().entry_count(), 0);
/// ```
pub struct ClipboardHistory {
    /// Accessibility label.
    pub label: String,
    /// Maximum retained entries (pinned exempt).
    pub max_entries: usize,
    /// Row height in points.
    pub row_height: f32,
    entries: Vec<ClipEntry>,
    pasted: Option<usize>,
    hovered: Option<usize>,
    rows: Vec<Rect>,
    text_painter: Option<SharedTextPainter>,
    bounds: Rect,
    scale: f32,
}

impl std::fmt::Debug for ClipboardHistory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClipboardHistory")
            .field("entries", &self.entries.len())
            .finish()
    }
}

impl Default for ClipboardHistory {
    fn default() -> Self {
        Self::new()
    }
}

impl ClipboardHistory {
    /// Empty history.
    ///
    /// ```
    /// use martensite::widgets::clipboard_history::ClipboardHistory;
    ///
    /// assert_eq!(ClipboardHistory::new().entry_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Clipboard history".to_string(),
            max_entries: 50,
            row_height: ROW_PT,
            entries: Vec::new(),
            pasted: None,
            hovered: None,
            rows: Vec::new(),
            text_painter: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::clipboard_history::ClipboardHistory;
    ///
    /// assert_eq!(ClipboardHistory::new().label("Clips").label, "Clips");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Retention cap.
    ///
    /// ```
    /// use martensite::widgets::clipboard_history::ClipboardHistory;
    ///
    /// assert_eq!(ClipboardHistory::new().max(3).max_entries, 3);
    /// ```
    pub fn max(mut self, max_entries: usize) -> Self {
        self.max_entries = max_entries;
        self
    }

    /// Shared text painter.
    ///
    /// ```no_run
    /// use martensite::widgets::clipboard_history::ClipboardHistory;
    /// # fn example(p: martensite::text_paint::SharedTextPainter) {
    /// let _h = ClipboardHistory::new().with_text_painter(p);
    /// # }
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Pushes a snippet at the top, evicting the oldest unpinned
    /// entry past `max_entries`.
    ///
    /// ```
    /// use martensite::widgets::clipboard_history::ClipboardHistory;
    ///
    /// let mut h = ClipboardHistory::new().max(2);
    /// h.push("a");
    /// h.push("b");
    /// h.push("c");
    /// assert_eq!(h.entry_count(), 2);
    /// assert_eq!(h.entry(0).unwrap().text, "c");
    /// ```
    pub fn push(&mut self, text: impl Into<String>) {
        self.entries.insert(0, ClipEntry::new(text));
        while self.entries.len() > self.max_entries {
            if let Some(i) = self.entries.iter().rposition(|e| !e.pinned) {
                self.entries.remove(i);
            } else {
                break;
            }
        }
    }

    /// Entry count.
    ///
    /// ```
    /// use martensite::widgets::clipboard_history::ClipboardHistory;
    ///
    /// assert_eq!(ClipboardHistory::new().entry_count(), 0);
    /// ```
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// Entry at `i` (0 = newest).
    ///
    /// ```
    /// use martensite::widgets::clipboard_history::ClipboardHistory;
    ///
    /// assert!(ClipboardHistory::new().entry(0).is_none());
    /// ```
    pub fn entry(&self, i: usize) -> Option<&ClipEntry> {
        self.entries.get(i)
    }

    /// Pinned entries survive eviction.
    ///
    /// ```
    /// use martensite::widgets::clipboard_history::ClipboardHistory;
    ///
    /// let mut h = ClipboardHistory::new().max(2);
    /// h.push("keep");
    /// h.set_pinned(0, true);
    /// h.push("x");
    /// h.push("y");
    /// h.push("z");
    /// assert_eq!(h.entry_count(), 2);
    /// assert_eq!(h.entry(1).unwrap().text, "keep");
    /// ```
    pub fn set_pinned(&mut self, i: usize, pinned: bool) {
        if let Some(e) = self.entries.get_mut(i) {
            e.pinned = pinned;
        }
    }

    /// Removes an entry.
    ///
    /// ```
    /// use martensite::widgets::clipboard_history::ClipboardHistory;
    ///
    /// let mut h = ClipboardHistory::new();
    /// h.push("a");
    /// h.remove(0);
    /// assert_eq!(h.entry_count(), 0);
    /// ```
    pub fn remove(&mut self, i: usize) {
        if i < self.entries.len() {
            self.entries.remove(i);
        }
    }

    /// Clears all unpinned entries.
    ///
    /// ```
    /// use martensite::widgets::clipboard_history::ClipboardHistory;
    ///
    /// let mut h = ClipboardHistory::new();
    /// h.push("a");
    /// h.push("b");
    /// h.set_pinned(1, true);
    /// h.clear_unpinned();
    /// assert_eq!(h.entry_count(), 1);
    /// ```
    pub fn clear_unpinned(&mut self) {
        self.entries.retain(|e| e.pinned);
    }

    /// Drains the last clicked row index — the host pastes
    /// [`ClipboardHistory::entry`] at that index.
    ///
    /// ```
    /// use martensite::widgets::clipboard_history::ClipboardHistory;
    ///
    /// let mut h = ClipboardHistory::new();
    /// assert_eq!(h.take_pasted(), None);
    /// ```
    pub fn take_pasted(&mut self) -> Option<usize> {
        self.pasted.take()
    }

    fn hit(&self, p: Vec2) -> Option<usize> {
        self.rows.iter().position(|r| r.contains(p))
    }
}

impl Widget for ClipboardHistory {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let s = cx.scale;
        let rows = self.entries.len().clamp(1, 8) as f32;
        Vec2::new(
            (280.0 * s).min(constraints.max_size.x.max(0.0)),
            ((rows * self.row_height + PAD_PT * 2.0) * s).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(180.0, ROW_PT + PAD_PT * 2.0))
            .with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        let s = cx.scale;
        let row_h = self.row_height * s;
        self.rows.clear();
        for i in 0..self.entries.len() {
            self.rows.push(Rect::new(
                bounds.min_x() + PAD_PT * s,
                bounds.min_y() + PAD_PT * s + i as f32 * row_h,
                (bounds.width() - PAD_PT * 2.0 * s).max(0.0),
                row_h,
            ));
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::List);
        node.set_label(self.label.clone());
        node.set_value(format!("{} clips", self.entries.len()));
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
                    self.pasted = Some(i);
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
        let shape = martensite_core::shape::Shape::rounded(5.0 * s);
        for (i, e) in self.entries.iter().enumerate() {
            let r = self.rows[i];
            let kr = kurbo::Rect::new(
                f64::from(r.min_x()),
                f64::from(r.min_y()),
                f64::from(r.max_x()),
                f64::from(r.max_y()),
            );
            if self.hovered == Some(i) {
                cx.list.push_fill_shape(kr, &shape, ROW_HOVER);
            }
            // Row number.
            let num = format!("{}", i + 1);
            crate::text_paint::paint_label(
                painter,
                cx.list,
                kurbo::Point::new(
                    f64::from(r.min_x() + 6.0 * s),
                    f64::from(r.min_y() + r.height() * 0.7),
                ),
                &num,
                FONT_PT * 0.8 * s,
                MUTED_FG,
            );
            // Snippet — whitespace flattened to a single line.
            let snippet: String = e.text.split_whitespace().collect::<Vec<_>>().join(" ");
            let text_r = Rect::new(
                r.min_x() + 24.0 * s,
                r.min_y(),
                r.width() - 24.0 * s - PIN_PT * s - 8.0 * s,
                r.height(),
            );
            let tkr = kurbo::Rect::new(
                f64::from(text_r.min_x()),
                f64::from(text_r.min_y()),
                f64::from(text_r.max_x()),
                f64::from(text_r.max_y()),
            );
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                tkr,
                kurbo::Point::new(
                    f64::from(text_r.min_x()),
                    f64::from(r.min_y() + r.height() * 0.72),
                ),
                &snippet,
                FONT_PT * s,
                cx.color(TokenKey::TextColor, TEXT),
            );
            // Pin marker.
            if e.pinned {
                crate::text_paint::paint_label(
                    painter,
                    cx.list,
                    kurbo::Point::new(
                        f64::from(r.max_x() - PIN_PT * s - 4.0 * s),
                        f64::from(r.min_y() + r.height() * 0.72),
                    ),
                    "●",
                    FONT_PT * 0.8 * s,
                    cx.color(TokenKey::WarningColor, PIN),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn fixture() -> ClipboardHistory {
        let mut h = ClipboardHistory::new();
        h.push("first");
        h.push("second");
        h.push("third");
        h
    }

    fn laid_out(h: &mut ClipboardHistory) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        h.layout(&mut cx, Rect::new(0.0, 0.0, 280.0, 140.0));
    }

    #[test]
    fn push_prepends_and_evicts() {
        let mut h = ClipboardHistory::new().max(2);
        h.push("a");
        h.push("b");
        h.push("c");
        assert_eq!(h.entry_count(), 2);
        assert_eq!(h.entry(0).unwrap().text, "c");
        assert_eq!(h.entry(1).unwrap().text, "b");
    }

    #[test]
    fn pinned_survives_eviction() {
        let mut h = ClipboardHistory::new().max(2);
        h.push("keep");
        h.set_pinned(0, true);
        h.push("x");
        h.push("y");
        assert_eq!(h.entry_count(), 2);
        assert_eq!(h.entry(1).unwrap().text, "keep");
    }

    #[test]
    fn click_parks_pasted() {
        let mut h = fixture();
        laid_out(&mut h);
        let r = h.rows[1];
        h.event(&mut EventContext {
            event: &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: Vec2::new((r.min_x() + r.max_x()) / 2.0, (r.min_y() + r.max_y()) / 2.0),
            },
            bounds: h.bounds,
            scale: 1.0,
        });
        assert_eq!(h.take_pasted(), Some(1));
        assert_eq!(h.take_pasted(), None);
    }

    #[test]
    fn clear_keeps_pinned() {
        let mut h = fixture();
        h.set_pinned(0, true);
        h.clear_unpinned();
        assert_eq!(h.entry_count(), 1);
        assert_eq!(h.entry(0).unwrap().text, "third");
    }

    #[test]
    fn paint_without_painter() {
        let mut h = fixture();
        laid_out(&mut h);
        let mut list = martensite_core::PaintList::default();
        let theme = martensite_theme::Theme::new("test");
        h.paint(&mut PaintContext {
            list: &mut list,
            bounds: h.bounds,
            theme: &theme,
            scale: 1.0,
            text_painter: None,
        });
        assert!(!list.is_empty());
    }
}
