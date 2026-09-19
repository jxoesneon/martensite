//! `TextInput` widget: an editable text field with an accessible label.
//!
//! The `TextInput` widget exposes `Role::TextInput`, an accessible label,
//! the `Action::Focus` and `Action::SetValue` accessibility actions, and
//! the current value. It integrates with the focus system via
//! `NodeFlags::FOCUSABLE`.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::text_input::TextInput;
//!
//! let input = TextInput::new("Search").placeholder("Type here...");
//! assert_eq!(input.label, "Search");
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::shape::Shape;
use martensite_core::widget::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Widget, WidgetEvent,
};
use martensite_core::{NodeFlags, Rect, RenderMinimum, TokenKey, UnderflowPolicy};
use unicode_segmentation::UnicodeSegmentation;

/// Field background colour.
const FACE: [u8; 4] = [255, 255, 255, 255];
/// Field border colour.
const EDGE: [u8; 4] = [140, 145, 155, 255];
/// Border colour while focused.
const EDGE_FOCUSED: [u8; 4] = [40, 110, 220, 255];
/// Text ink colour.
const INK: [u8; 4] = [20, 20, 25, 255];
/// Placeholder ink colour.
const INK_PLACEHOLDER: [u8; 4] = [150, 150, 155, 255];
/// Caret colour.
const CARET: [u8; 4] = [30, 30, 35, 255];
/// Horizontal inset for the editable text.
const TEXT_PAD_X: f32 = 8.0;
/// Font size in logical pt for the editable text.
const FONT_PT: f32 = 14.0;

/// Selection granularity for an in-progress drag — chosen by the
/// initiating press's click count: single-click drags by character,
/// double-click drags by word, triple-click (the whole line for a
/// single-line field) has nothing to extend.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
enum SelectGranularity {
    /// Extend the selection one insertion boundary at a time.
    #[default]
    Char,
    /// Extend the selection by whole words, keeping the initially
    /// clicked word fully covered on either drag direction.
    Word,
    /// Whole-line selection — a drag cannot extend further in a
    /// single-line input.
    Line,
}

/// The UAX#29 word segment containing byte offset `at` — the run of
/// letters, the whitespace run, or the punctuation run under the
/// pointer, matching platform text-field double-click behavior. An
/// offset at the very end resolves to the last segment.
fn word_span(text: &str, at: usize) -> (usize, usize) {
    let mut last = (text.len(), text.len());
    for (i, seg) in text.split_word_bound_indices() {
        let end = i + seg.len();
        if at < end {
            return (i, end);
        }
        last = (i, end);
    }
    last
}

/// A text input widget with a label and editable value.
///
/// # Examples
///
/// ```
/// use martensite::widgets::TextInput;
///
/// let input = TextInput::new("Email")
///     .value("user@example.com")
///     .placeholder("Enter your email");
/// assert_eq!(input.label, "Email");
/// assert_eq!(input.value, "user@example.com");
/// ```
pub struct TextInput {
    /// The accessible label for the text input.
    pub label: String,
    /// The current text value.
    pub value: String,
    /// Placeholder text shown when the value is empty.
    pub placeholder: String,
    /// Whether the text input is enabled.
    pub enabled: bool,
    /// Whether the text input is read-only.
    pub read_only: bool,
    /// Cached bounds from the last layout pass.
    cached_bounds: Rect,
    /// Shared shaped-text painter — when set, `paint` emits real
    /// `GlyphRun`s; without it text falls back to `DrawText`
    /// placeholder boxes. See [`crate::text_paint`].
    text_painter: Option<crate::text_paint::SharedTextPainter>,
    /// Whether the input currently holds keyboard focus. Updated by the
    /// `FocusGained`/`FocusLost` widget events.
    focused: bool,
    /// Caret position as a byte offset into `value` — always a
    /// char boundary, matching `ShapedGlyph::start`/`end` units so
    /// shaped hit-testing and caret placement agree exactly.
    cursor: usize,
    /// Selection anchor (byte offset). `Some(anchor)` with
    /// `anchor != cursor` means the byte range between them is
    /// selected.
    selection_anchor: Option<usize>,
    /// Shift state tracked from `KeyPressed`/`KeyReleased` —
    /// `WidgetEvent::KeyPressed` carries no modifier state (F17).
    shift_held: bool,
    /// Drag-select in progress; the widget holds pointer capture.
    dragging: bool,
    /// The granularity the current drag selects with — set from the
    /// initiating press's `count` (single → char, double → word).
    drag_granularity: SelectGranularity,
    /// The word selected by the double-click that opened a `Word`
    /// drag — its far edge becomes the anchor so the initial word
    /// stays fully covered whichever way the pointer moves.
    drag_word: Option<(usize, usize)>,
    /// Device pixels per logical point, cached in `layout` — pointer
    /// hit-testing needs it because `EventContext` carries no scale.
    scale: f32,
    /// Horizontal scroll of the text run (device px, f32 bits) —
    /// `paint` keeps the caret inside the field when the value is
    /// wider than the interior. Atomic interior mutation because the
    /// shaped offsets the scroll needs only exist in `PaintContext`
    /// (`event` can't see the ambient painter), and `Widget` requires
    /// `Sync`.
    scroll_x: std::sync::atomic::AtomicU32,
}

impl Clone for TextInput {
    fn clone(&self) -> Self {
        Self {
            label: self.label.clone(),
            value: self.value.clone(),
            placeholder: self.placeholder.clone(),
            enabled: self.enabled,
            read_only: self.read_only,
            cached_bounds: self.cached_bounds,
            text_painter: self.text_painter.clone(),
            focused: self.focused,
            cursor: self.cursor,
            selection_anchor: self.selection_anchor,
            shift_held: self.shift_held,
            dragging: self.dragging,
            drag_granularity: self.drag_granularity,
            drag_word: self.drag_word,
            scale: self.scale,
            scroll_x: std::sync::atomic::AtomicU32::new(
                self.scroll_x.load(std::sync::atomic::Ordering::Relaxed),
            ),
        }
    }
}

impl TextInput {
    /// Creates a new text input with the given label and empty value.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TextInput;
    ///
    /// let input = TextInput::new("Name");
    /// assert_eq!(input.label, "Name");
    /// assert!(input.value.is_empty());
    /// ```
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: String::new(),
            placeholder: String::new(),
            enabled: true,
            read_only: false,
            cached_bounds: Rect::default(),
            text_painter: None,
            focused: false,
            cursor: 0,
            selection_anchor: None,
            shift_held: false,
            dragging: false,
            drag_granularity: SelectGranularity::Char,
            drag_word: None,
            scale: 1.0,
            scroll_x: std::sync::atomic::AtomicU32::new(0),
        }
    }

    /// Sets the current text value.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TextInput;
    ///
    /// let input = TextInput::new("Username").value("alice");
    /// assert_eq!(input.value, "alice");
    /// ```
    #[inline]
    #[must_use]
    pub fn value(mut self, value: impl Into<String>) -> Self {
        self.set_value(value);
        self
    }

    /// Sets the placeholder text.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TextInput;
    ///
    /// let input = TextInput::new("Search").placeholder("Type query...");
    /// assert_eq!(input.placeholder, "Type query...");
    /// ```
    #[inline]
    #[must_use]
    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    /// Sets whether the text input is enabled.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TextInput;
    ///
    /// let input = TextInput::new("Locked").enabled(false);
    /// assert!(!input.enabled);
    /// ```
    #[inline]
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Sets whether the text input is read-only.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TextInput;
    ///
    /// let input = TextInput::new("ID").read_only(true);
    /// assert!(input.read_only);
    /// ```
    #[inline]
    #[must_use]
    pub fn read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }

    /// Sets the value (mutable version for programmatic updates).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TextInput;
    ///
    /// let mut input = TextInput::new("Name");
    /// input.set_value("Bob");
    /// assert_eq!(input.value, "Bob");
    /// ```
    #[inline]
    pub fn set_value(&mut self, value: impl Into<String>) {
        self.value = value.into();
        // Programmatic writes collapse the caret to the end — a stale
        // byte offset could land mid-char after a shorter write.
        self.cursor = self.value.len();
        self.selection_anchor = None;
    }

    /// Returns the cached bounds from the last layout pass.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TextInput;
    ///
    /// let input = TextInput::new("Address");
    /// let bounds = input.cached_bounds();
    /// assert_eq!(bounds.size.x, 0.0);
    /// ```
    #[inline]
    pub fn cached_bounds(&self) -> Rect {
        self.cached_bounds
    }

    /// Shares a [`crate::text_paint::TextPainter`] so `paint` emits real
    /// glyph runs instead of `DrawText` placeholder boxes.
    #[must_use]
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Selected byte range `(start, end)`, or `None` when the caret is
    /// collapsed.
    fn selection(&self) -> Option<(usize, usize)> {
        let anchor = self.selection_anchor?;
        (anchor != self.cursor).then(|| (anchor.min(self.cursor), anchor.max(self.cursor)))
    }

    /// The currently selected text, if any.
    fn selected_text(&self) -> Option<&str> {
        self.selection().map(|(lo, hi)| &self.value[lo..hi])
    }

    /// Removes the selected range, leaving the caret at its start.
    /// Returns whether anything was deleted.
    fn delete_selection(&mut self) -> bool {
        let Some((lo, hi)) = self.selection() else {
            return false;
        };
        self.value.drain(lo..hi);
        self.cursor = lo;
        self.selection_anchor = None;
        true
    }

    /// Inserts `text` at the caret, replacing any selection. Newlines
    /// are stripped — this is a single-line field.
    fn insert_str(&mut self, text: &str) {
        self.delete_selection();
        let clean: String = text.chars().filter(|c| *c != '\n' && *c != '\r').collect();
        self.value.insert_str(self.cursor, &clean);
        self.cursor += clean.len();
    }

    /// Moves the caret to `pos` (clamped). With `extend`, the range
    /// from the previous caret becomes the selection anchor;
    /// otherwise any selection collapses.
    fn set_caret(&mut self, pos: usize, extend: bool) {
        if extend {
            if self.selection_anchor.is_none() {
                self.selection_anchor = Some(self.cursor);
            }
        } else {
            self.selection_anchor = None;
        }
        self.cursor = pos.min(self.value.len());
    }

    fn caret_left(&mut self, extend: bool) {
        // Collapse to the selection's left edge rather than stepping.
        if !extend {
            if let Some((lo, _)) = self.selection() {
                self.selection_anchor = None;
                self.cursor = lo;
                return;
            }
        }
        let prev = self.value[..self.cursor]
            .chars()
            .next_back()
            .map_or(0, |c| self.cursor - c.len_utf8());
        self.set_caret(prev, extend);
    }

    fn caret_right(&mut self, extend: bool) {
        if !extend {
            if let Some((_, hi)) = self.selection() {
                self.selection_anchor = None;
                self.cursor = hi;
                return;
            }
        }
        let next = self.value[self.cursor..]
            .chars()
            .next()
            .map_or(self.value.len(), |c| self.cursor + c.len_utf8());
        self.set_caret(next, extend);
    }

    fn backspace(&mut self) {
        if self.delete_selection() {
            return;
        }
        if let Some(c) = self.value[..self.cursor].chars().next_back() {
            let start = self.cursor - c.len_utf8();
            self.value.drain(start..self.cursor);
            self.cursor = start;
        }
    }

    fn delete_forward(&mut self) {
        if self.delete_selection() {
            return;
        }
        if let Some(c) = self.value[self.cursor..].chars().next() {
            self.value.drain(self.cursor..self.cursor + c.len_utf8());
        }
    }

    /// Writes the selection to the OS clipboard (no-op when nothing
    /// is selected). A fresh backend is constructed per call — the
    /// platform clipboard objects are not `Send` and cannot live on
    /// the widget (`Widget: Send + Sync`).
    fn copy_selection(&self) {
        if let Some(text) = self.selected_text() {
            let mut cb = martensite_clipboard::default_platform_clipboard();
            cb.set_contents(&martensite_clipboard::ClipboardItem::new().offer_text(text));
        }
    }

    /// Inserts the OS clipboard's text payload at the caret (no-op
    /// when the clipboard holds none).
    fn paste_clipboard(&mut self) {
        let cb = martensite_clipboard::default_platform_clipboard();
        if let Some(bytes) = cb.get_contents(martensite_clipboard::clipboard::MIME_TEXT_PLAIN) {
            let text = String::from_utf8_lossy(&bytes);
            self.insert_str(&text);
        }
    }

    /// The text run's horizontal scroll in device px.
    fn scroll_x(&self) -> f32 {
        f32::from_bits(self.scroll_x.load(std::sync::atomic::Ordering::Relaxed))
    }

    /// Stores the text run's horizontal scroll in device px.
    fn set_scroll_x(&self, v: f32) {
        self.scroll_x
            .store(v.to_bits(), std::sync::atomic::Ordering::Relaxed);
    }

    /// Byte offset of the insertion boundary nearest device-pixel `x`
    /// in window space. Shapes through the widget's painter so clicks
    /// land exactly where the glyphs are — the painter is created
    /// lazily so inputs that are never clicked skip the font scan.
    fn byte_at_position(&mut self, bounds: Rect, x: f32) -> usize {
        // `scroll_x` shifts the painted run left — add it back so a
        // click lands on the glyph under the pointer.
        let text_x = bounds.origin.x + TEXT_PAD_X * self.scale - self.scroll_x();
        let painter = self
            .text_painter
            .get_or_insert_with(crate::text_paint::shared_painter);
        painter.byte_at(&self.value, FONT_PT * self.scale, x - text_x)
    }

    /// Device-pixel offset of the caret boundary at `byte_idx`, from
    /// the same painter that emits the glyphs. Falls back to the
    /// per-char estimate only when no shaped painter exists at all
    /// (placeholder `DrawText` path, which is itself approximate).
    fn offset_x(&self, cx: &PaintContext, byte_idx: usize, font_px: f32) -> f32 {
        if let Some(p) = &self.text_painter {
            return p.caret_x(&self.value, font_px, byte_idx);
        }
        if let Some(p) = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter) {
            if let Some(w) = p.measure_text(&self.value[..byte_idx], font_px) {
                return w;
            }
        }
        self.value[..byte_idx].chars().count() as f32 * cx.pt(7.0)
    }

    /// Keyboard handling for the focused input. `key` is the logical
    /// key name — plain characters arrive via `ImeCommitted`, while
    /// chords like Cmd+A arrive as the synthetic names
    /// `"SelectAll"`/`"Cut"`/`"Copy"`/`"Paste"` that the window layer
    /// dispatches (see the example's chord synthesis).
    fn key_pressed(&mut self, key: &str) -> EventResponse {
        match key {
            "Shift" => {
                self.shift_held = true;
                EventResponse::Handled
            }
            "ArrowLeft" => {
                self.caret_left(self.shift_held);
                EventResponse::RequestRepaint
            }
            "ArrowRight" => {
                self.caret_right(self.shift_held);
                EventResponse::RequestRepaint
            }
            "Home" => {
                self.set_caret(0, self.shift_held);
                EventResponse::RequestRepaint
            }
            "End" => {
                self.set_caret(self.value.len(), self.shift_held);
                EventResponse::RequestRepaint
            }
            "SelectAll" => {
                self.selection_anchor = Some(0);
                self.cursor = self.value.len();
                EventResponse::RequestRepaint
            }
            "Copy" => {
                self.copy_selection();
                EventResponse::Handled
            }
            "Cut" if !self.read_only => {
                self.copy_selection();
                self.delete_selection();
                EventResponse::RequestRepaint
            }
            "Paste" if !self.read_only => {
                self.paste_clipboard();
                EventResponse::RequestRepaint
            }
            "Backspace" if !self.read_only => {
                self.backspace();
                EventResponse::RequestRepaint
            }
            "Delete" if !self.read_only => {
                self.delete_forward();
                EventResponse::RequestRepaint
            }
            "Escape" if self.selection().is_some() => {
                self.selection_anchor = None;
                EventResponse::RequestRepaint
            }
            _ => EventResponse::Ignored,
        }
    }
}

impl Widget for TextInput {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        // A text input has a default minimum size of 120x24 logical pt.
        let min_w = cx.pt(120.0).min(constraints.max_size.x.max(0.0));
        let min_h = cx.pt(24.0).min(constraints.max_size.y.max(0.0));
        Vec2::new(min_w, min_h)
    }

    fn min_render(&self) -> RenderMinimum {
        // The same 120×24pt floor `measure` requests — declared so
        // underflow auditing and app-chosen policies can act on it.
        RenderMinimum::new(Vec2::new(120.0, 24.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.cached_bounds = bounds;
        self.scale = cx.scale;
        // Declare keyboard focusability on the arena node — standalone
        // inputs need it for `ImeCommitted` delivery.
        if self.enabled {
            cx.hot.flags |= NodeFlags::FOCUSABLE;
        } else {
            cx.hot.flags.remove(NodeFlags::FOCUSABLE);
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::TextInput);
        node.set_label(self.label.as_str());
        node.set_value(self.value.as_str());
        node.add_action(accesskit::Action::Focus);
        if !self.read_only {
            node.add_action(accesskit::Action::SetValue);
        }
        if !self.enabled {
            node.set_disabled();
        }
        if self.read_only {
            node.set_read_only();
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.enabled {
            return EventResponse::Ignored;
        }
        match cx.event {
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                count,
            } => {
                let hit = self.byte_at_position(cx.bounds, position.x);
                match count {
                    1 => {
                        if self.shift_held {
                            // Shift-click extends from the existing caret.
                            if self.selection_anchor.is_none() {
                                self.selection_anchor = Some(self.cursor);
                            }
                        } else {
                            self.selection_anchor = None;
                        }
                        self.cursor = hit;
                        self.drag_granularity = SelectGranularity::Char;
                        self.drag_word = None;
                    }
                    2 => {
                        // Double-click selects the whole word segment
                        // under the pointer — the platform-standard
                        // gesture `count` exists for.
                        let (lo, hi) = word_span(&self.value, hit);
                        self.selection_anchor = Some(lo);
                        self.cursor = hi;
                        self.drag_granularity = SelectGranularity::Word;
                        self.drag_word = Some((lo, hi));
                    }
                    _ => {
                        // Triple-click selects the line — the whole
                        // value for a single-line input.
                        self.selection_anchor = Some(0);
                        self.cursor = self.value.len();
                        self.drag_granularity = SelectGranularity::Line;
                        self.drag_word = None;
                    }
                }
                self.dragging = true;
                // Focus is implicit for FOCUSABLE nodes on handled
                // presses; capture keeps drag-select tracking outside
                // the bounds.
                EventResponse::CapturePointer
            }
            WidgetEvent::PointerMoved { position } if self.dragging => {
                let hit = self.byte_at_position(cx.bounds, position.x);
                match self.drag_granularity {
                    SelectGranularity::Char => {
                        if self.selection_anchor.is_none() {
                            // The press position becomes the anchor —
                            // cursor is still there on the first move.
                            self.selection_anchor = Some(self.cursor);
                        }
                        self.cursor = hit;
                    }
                    SelectGranularity::Word => {
                        // Extend by whole words: the edge of the
                        // double-clicked word opposite the drag stays
                        // anchored, the moving edge snaps to the far
                        // boundary of the word under the pointer.
                        let (wlo, whi) = word_span(&self.value, hit);
                        if let Some((ilo, ihi)) = self.drag_word {
                            if hit >= ilo {
                                self.selection_anchor = Some(ilo);
                                self.cursor = whi;
                            } else {
                                self.selection_anchor = Some(ihi);
                                self.cursor = wlo;
                            }
                        }
                    }
                    // Whole line already selected — nothing to extend.
                    SelectGranularity::Line => {}
                }
                EventResponse::RequestRepaint
            }
            WidgetEvent::PointerReleased { .. } if self.dragging => {
                self.dragging = false;
                self.drag_word = None;
                EventResponse::ReleasePointer
            }
            WidgetEvent::FocusGained => {
                self.focused = true;
                EventResponse::RequestRepaint
            }
            WidgetEvent::FocusLost => {
                self.focused = false;
                // The OS can swallow key releases on focus transitions —
                // don't leak a stuck Shift or drag into the next focus.
                self.shift_held = false;
                self.dragging = false;
                EventResponse::RequestRepaint
            }
            WidgetEvent::ImeCommitted { text } if !self.read_only => {
                self.insert_str(text);
                EventResponse::RequestRepaint
            }
            WidgetEvent::KeyPressed { key, .. } => self.key_pressed(key),
            WidgetEvent::KeyReleased { key } if key == "Shift" => {
                self.shift_held = false;
                EventResponse::Handled
            }
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        let rect = kurbo::Rect::new(
            f64::from(b.min_x()),
            f64::from(b.min_y()),
            f64::from(b.max_x()),
            f64::from(b.max_y()),
        );
        let face = Shape::rounded(cx.dim(TokenKey::BorderRadiusSmall, 3.0));
        cx.list
            .push_fill_shape(rect, &face, cx.color(TokenKey::SurfaceColor, FACE));
        cx.list.push_stroke_shape(
            rect,
            &face,
            cx.pt(1.0),
            if self.focused {
                cx.color(TokenKey::AccentColor, EDGE_FOCUSED)
            } else {
                cx.color(TokenKey::BorderColor, EDGE)
            },
        );

        // `DrawText` positions by the text run's top edge — centre the
        // 14 pt font box within the field.
        let font_px = cx.pt(FONT_PT);
        let pad = cx.pt(TEXT_PAD_X);
        let text_y = b.origin.y + (b.size.y - font_px) / 2.0;
        let text_x = b.origin.x + pad;

        // Caret-following horizontal scroll — when the value is wider
        // than the interior the run slides left so the caret stays
        // inside the field (standard text-field behavior). Painted
        // positions below all shift by `scroll`.
        let caret_off = self.offset_x(cx, self.cursor, font_px);
        let inner_w = (b.size.x - 2.0 * pad).max(0.0);
        let mut scroll = self.scroll_x();
        if caret_off - scroll > inner_w {
            scroll = caret_off - inner_w;
        }
        if caret_off - scroll < 0.0 {
            scroll = caret_off;
        }
        let scroll = scroll.max(0.0);
        self.set_scroll_x(scroll);
        let text_x = text_x - scroll;

        // Everything inside the border is clipped to the face — the
        // text run, selection band, and caret can never spill past
        // the field edge.
        let inset = cx.pt(1.0);
        cx.list.push_clip_shape(
            kurbo::Rect::new(
                rect.x0 + f64::from(inset),
                rect.y0 + f64::from(inset),
                rect.x1 - f64::from(inset),
                rect.y1 - f64::from(inset),
            ),
            &face,
        );

        // Selection highlight behind the text — same shaped offsets
        // the caret uses, so the band aligns with the glyphs.
        if self.focused {
            if let Some((lo, hi)) = self.selection() {
                let x0 = f64::from(text_x + self.offset_x(cx, lo, font_px));
                let x1 = f64::from(text_x + self.offset_x(cx, hi, font_px));
                let accent = cx.color(TokenKey::AccentColor, EDGE_FOCUSED);
                cx.list.push_fill_rect(
                    kurbo::Rect::new(
                        x0,
                        f64::from(b.origin.y + cx.pt(3.0)),
                        x1,
                        f64::from(b.max_y()) - cx.ptf(3.0),
                    ),
                    [accent[0], accent[1], accent[2], 96],
                );
            }
        }

        if self.value.is_empty() {
            crate::text_paint::paint_label(
                crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter),
                cx.list,
                kurbo::Point::new(f64::from(text_x), f64::from(text_y)),
                &self.placeholder,
                font_px,
                cx.color(TokenKey::TextMutedColor, INK_PLACEHOLDER),
            );
        } else {
            crate::text_paint::paint_label(
                crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter),
                cx.list,
                kurbo::Point::new(f64::from(text_x), f64::from(text_y)),
                &self.value,
                font_px,
                cx.color(TokenKey::TextColor, INK),
            );
        }

        // Caret at the shaped boundary for `self.cursor` — measured
        // through the same shape pass as the painted glyphs, so it
        // cannot drift on mixed-width text.
        if self.focused {
            let caret_x = f64::from(text_x + self.offset_x(cx, self.cursor, font_px));
            let top = f64::from(b.origin.y + cx.pt(4.0));
            let mut caret = kurbo::BezPath::new();
            caret.move_to((caret_x, top));
            caret.line_to((caret_x, f64::from(b.max_y()) - cx.ptf(4.0)));
            cx.list
                .push_stroke_path(caret, cx.pt(1.0), cx.color(TokenKey::TextColor, CARET));
        }
        cx.list.pop_clip();
    }
}

impl std::fmt::Debug for TextInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextInput")
            .field("label", &self.label)
            .field("value", &self.value)
            .field("enabled", &self.enabled)
            .field("read_only", &self.read_only)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn make_cx(hot: &mut HotNode) -> LayoutContext<'_> {
        LayoutContext { hot, scale: 1.0 }
    }

    #[test]
    fn text_input_new() {
        let input = TextInput::new("Name");
        assert_eq!(input.label, "Name");
        assert!(input.value.is_empty());
        assert!(input.enabled);
        assert!(!input.read_only);
    }

    #[test]
    fn text_input_builder_methods() {
        let input = TextInput::new("Email")
            .value("test@test.com")
            .placeholder("Enter email")
            .enabled(false)
            .read_only(true);
        assert_eq!(input.value, "test@test.com");
        assert_eq!(input.placeholder, "Enter email");
        assert!(!input.enabled);
        assert!(input.read_only);
    }

    #[test]
    fn text_input_set_value() {
        let mut input = TextInput::new("Name");
        input.set_value("John");
        assert_eq!(input.value, "John");
    }

    #[test]
    fn text_input_measure_returns_size() {
        let mut hot = HotNode::default();
        let mut cx = make_cx(&mut hot);
        let mut input = TextInput::new("Test");
        let size = input.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(300.0, 100.0),
            },
        );
        assert!(size.x >= 0.0 && size.y >= 0.0);
    }

    #[test]
    fn text_input_layout_sets_bounds() {
        let mut hot = HotNode::default();
        let mut cx = make_cx(&mut hot);
        let mut input = TextInput::new("Test");
        let bounds = Rect::new(0.0, 0.0, 120.0, 24.0);
        input.layout(&mut cx, bounds);
        assert_eq!(input.cached_bounds(), bounds);
    }

    #[test]
    fn text_input_accessibility_sets_role_label_value() {
        let input = TextInput::new("Email").value("hello@test.com");
        let mut node = accesskit::Node::new(accesskit::Role::Unknown);
        input.accessibility(&mut node);
        assert_eq!(node.role(), accesskit::Role::TextInput);
        assert_eq!(node.label(), Some("Email"));
        assert_eq!(node.value(), Some("hello@test.com"));
        assert!(node.supports_action(accesskit::Action::Focus));
        assert!(node.supports_action(accesskit::Action::SetValue));
    }

    #[test]
    fn text_input_accessibility_read_only_no_set_value() {
        let input = TextInput::new("Read").read_only(true);
        let mut node = accesskit::Node::new(accesskit::Role::Unknown);
        input.accessibility(&mut node);
        assert!(!node.supports_action(accesskit::Action::SetValue));
    }

    #[test]
    fn text_input_accessibility_disabled() {
        let input = TextInput::new("Disabled").enabled(false);
        let mut node = accesskit::Node::new(accesskit::Role::Unknown);
        input.accessibility(&mut node);
        assert!(node.is_disabled());
    }

    #[test]
    fn text_input_clone() {
        let input = TextInput::new("Test").value("hello");
        let cloned = input.clone();
        assert_eq!(input.label, cloned.label);
        assert_eq!(input.value, cloned.value);
    }

    fn ev<'a>(event: &'a WidgetEvent) -> EventContext<'a> {
        EventContext {
            event,
            bounds: Rect::new(0.0, 0.0, 200.0, 24.0),
            scale: 1.0,
        }
    }

    fn ime(text: &str) -> WidgetEvent {
        WidgetEvent::ImeCommitted {
            text: text.to_string(),
        }
    }

    fn key(name: &str) -> WidgetEvent {
        WidgetEvent::KeyPressed {
            key: name.to_string(),
            repeat: false,
        }
    }

    #[test]
    fn text_input_ime_inserts_at_caret() {
        let mut input = TextInput::new("F");
        input.event(&mut ev(&ime("abc")));
        input.event(&mut ev(&key("ArrowLeft")));
        input.event(&mut ev(&key("ArrowLeft")));
        input.event(&mut ev(&ime("X")));
        assert_eq!(input.value, "aXbc");
        assert_eq!(input.cursor, 2);
    }

    #[test]
    fn text_input_backspace_and_delete() {
        let mut input = TextInput::new("F").value("abc");
        input.event(&mut ev(&key("ArrowLeft")));
        input.event(&mut ev(&key("Backspace")));
        assert_eq!(input.value, "ac");
        input.event(&mut ev(&key("Home")));
        input.event(&mut ev(&key("Delete")));
        assert_eq!(input.value, "c");
    }

    #[test]
    fn text_input_select_all_then_type_replaces() {
        let mut input = TextInput::new("F").value("abc");
        input.event(&mut ev(&key("SelectAll")));
        assert_eq!(input.selection(), Some((0, 3)));
        input.event(&mut ev(&ime("z")));
        assert_eq!(input.value, "z");
        assert_eq!(input.selection(), None);
        assert_eq!(input.cursor, 1);
    }

    #[test]
    fn text_input_shift_arrows_extend_selection() {
        let mut input = TextInput::new("F").value("abc");
        input.event(&mut ev(&key("Shift")));
        input.event(&mut ev(&key("ArrowLeft")));
        assert_eq!(input.selection(), Some((2, 3)));
        input.event(&mut ev(&key("ArrowLeft")));
        assert_eq!(input.selection(), Some((1, 3)));
        // Typing replaces the selection.
        input.event(&mut ev(&ime("X")));
        assert_eq!(input.value, "aX");
    }

    /// Window-space x of a caret boundary — through the same shaped
    /// painter `byte_at_position` hit-tests with, so test presses land
    /// inside the intended word.
    fn click_x(input: &mut TextInput, byte: usize) -> f32 {
        let p = input
            .text_painter
            .get_or_insert_with(crate::text_paint::shared_painter);
        TEXT_PAD_X + p.caret_x(&input.value, FONT_PT, byte)
    }

    fn press(input: &mut TextInput, x: f32, count: u8) -> EventResponse {
        input.event(&mut ev(&WidgetEvent::PointerPressed {
            position: Vec2::new(x, 12.0),
            button: PointerButton::Primary,
            count,
        }))
    }

    #[test]
    fn text_input_double_click_selects_word() {
        let mut input = TextInput::new("F").value("alpha beta gamma");
        let x = click_x(&mut input, 8); // inside "beta"
        assert_eq!(press(&mut input, x, 2), EventResponse::CapturePointer);
        assert_eq!(input.selected_text(), Some("beta"));
    }

    #[test]
    fn text_input_triple_click_selects_all() {
        let mut input = TextInput::new("F").value("alpha beta gamma");
        let x = click_x(&mut input, 8);
        press(&mut input, x, 3);
        assert_eq!(input.selection(), Some((0, "alpha beta gamma".len())));
    }

    #[test]
    fn text_input_double_click_drag_extends_by_word() {
        let mut input = TextInput::new("F").value("alpha beta gamma");
        let x = click_x(&mut input, 8); // inside "beta"
        press(&mut input, x, 2);
        // Drag left into "alpha" — the whole initial word stays
        // covered and the selection snaps to word boundaries.
        let left = click_x(&mut input, 1); // inside "alpha"
        input.event(&mut ev(&WidgetEvent::PointerMoved {
            position: Vec2::new(left, 12.0),
        }));
        assert_eq!(input.selected_text(), Some("alpha beta"));
        // Drag back right into "gamma" — anchor flips to the initial
        // word's leading edge.
        let right = click_x(&mut input, 14); // inside "gamma"
        input.event(&mut ev(&WidgetEvent::PointerMoved {
            position: Vec2::new(right, 12.0),
        }));
        assert_eq!(input.selected_text(), Some("beta gamma"));
        input.event(&mut ev(&WidgetEvent::PointerReleased {
            position: Vec2::new(right, 12.0),
            button: PointerButton::Primary,
        }));
        assert!(!input.dragging);
    }

    #[test]
    fn text_input_single_click_drag_selects_chars() {
        let mut input = TextInput::new("F").value("alpha beta");
        let x = click_x(&mut input, 2);
        press(&mut input, x, 1);
        let end = click_x(&mut input, 5);
        input.event(&mut ev(&WidgetEvent::PointerMoved {
            position: Vec2::new(end, 12.0),
        }));
        // Char granularity — a mid-word range, not a snapped word.
        assert_eq!(input.selected_text(), Some("pha"));
    }

    #[test]
    fn text_input_arrows_collapse_selection() {
        let mut input = TextInput::new("F").value("abc");
        input.event(&mut ev(&key("SelectAll")));
        input.event(&mut ev(&key("ArrowLeft")));
        assert_eq!(input.cursor, 0);
        assert_eq!(input.selection(), None);
        input.event(&mut ev(&key("SelectAll")));
        input.event(&mut ev(&key("ArrowRight")));
        assert_eq!(input.cursor, 3);
        assert_eq!(input.selection(), None);
    }

    #[test]
    fn text_input_backspace_deletes_selection() {
        let mut input = TextInput::new("F").value("abc");
        input.event(&mut ev(&key("SelectAll")));
        input.event(&mut ev(&key("Backspace")));
        assert_eq!(input.value, "");
        assert_eq!(input.cursor, 0);
    }

    #[test]
    fn text_input_escape_clears_selection() {
        let mut input = TextInput::new("F").value("abc");
        input.event(&mut ev(&key("SelectAll")));
        input.event(&mut ev(&key("Escape")));
        assert_eq!(input.selection(), None);
        assert_eq!(input.cursor, 3);
    }

    #[test]
    fn text_input_home_end() {
        let mut input = TextInput::new("F").value("abc");
        input.event(&mut ev(&key("Home")));
        assert_eq!(input.cursor, 0);
        input.event(&mut ev(&key("End")));
        assert_eq!(input.cursor, 3);
    }

    #[test]
    fn text_input_set_value_resets_caret() {
        let mut input = TextInput::new("F").value("abcdef");
        input.event(&mut ev(&key("Home")));
        input.set_value("xy");
        assert_eq!(input.cursor, 2);
        assert_eq!(input.selection(), None);
    }

    #[test]
    fn text_input_read_only_blocks_edits() {
        let mut input = TextInput::new("F").value("abc").read_only(true);
        input.event(&mut ev(&ime("X")));
        input.event(&mut ev(&key("Backspace")));
        input.event(&mut ev(&key("Paste")));
        input.event(&mut ev(&key("Cut")));
        assert_eq!(input.value, "abc");
        // …but selection and cursor movement still work.
        input.event(&mut ev(&key("SelectAll")));
        assert_eq!(input.selection(), Some((0, 3)));
    }

    #[test]
    fn text_input_insert_strips_newlines() {
        let mut input = TextInput::new("F");
        input.event(&mut ev(&ime("a\nb\rc")));
        assert_eq!(input.value, "abc");
        assert_eq!(input.cursor, 3);
    }

    #[test]
    fn text_input_hit_test_accounts_for_scroll() {
        let mut hot = HotNode::default();
        let mut cx = make_cx(&mut hot);
        let mut input = TextInput::new("F").value("alpha beta gamma delta epsilon");
        let bounds = Rect::new(0.0, 0.0, 60.0, 24.0);
        input.layout(&mut cx, bounds);

        // No scroll — a click near the left edge maps near the start.
        let unscrolled = input.byte_at_position(bounds, TEXT_PAD_X + 5.0);
        // Scroll the run left 40px — the same click lands 40px further
        // in, matching an un-scrolled click at x+40.
        input.set_scroll_x(40.0);
        let scrolled = input.byte_at_position(bounds, TEXT_PAD_X + 5.0);
        assert!(scrolled > unscrolled, "scroll did not shift hit-test");
        input.set_scroll_x(0.0);
        assert_eq!(
            scrolled,
            input.byte_at_position(bounds, TEXT_PAD_X + 45.0),
            "scrolled hit-test disagrees with un-scrolled equivalent"
        );
    }

    #[test]
    fn text_input_debug_format() {
        let input = TextInput::new("Test");
        let debug = format!("{:?}", input);
        assert!(debug.contains("TextInput"));
        assert!(debug.contains("Test"));
    }
}
