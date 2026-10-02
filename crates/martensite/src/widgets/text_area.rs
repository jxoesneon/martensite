//! `TextArea` widget: a multiline plain-text editor.
//!
//! `TextArea` is the multiline counterpart of
//! [`TextInput`](crate::widgets::text_input::TextInput) — a QTextEdit-plain /
//! GtkTextView-style editable region backed by the headless
//! [`martensite_blessed::CodeEditor`] line buffer. It exposes
//! `Role::MultilineTextInput`, an accessible label, the current value, and
//! the `Focus`/`SetValue`/scroll accessibility actions, and integrates with
//! the focus system via `NodeFlags::FOCUSABLE`.
//!
//! # Editing model
//!
//! - The document lives in `CodeEditor`'s `Vec<String>` line buffer; the
//!   widget drives a single caret (the model's primary cursor, a
//!   `(line, char-column)` pair) plus one selection anchored on it.
//! - Soft-wrap is on by default ([`TextArea::wrap`]). Wrapped rows are
//!   computed at word boundaries through the same shaped painter that
//!   emits the glyphs, so hit-testing, selection bands, and the caret
//!   agree exactly with what is painted.
//! - Up/Down move between *logical* lines, not visual wrap rows — a
//!   deliberate v1 simplification. PageUp/PageDown move by the visible
//!   row count. A sticky preferred column survives vertical moves.
//! - `Tab` inserts a `\t` character — multiline editors own the key,
//!   unlike `TextInput`, which leaves it for focus traversal. `Enter`
//!   inserts a newline; `Escape` collapses an open selection.
//! - Vertical *and* horizontal scroll follow the caret (the horizontal
//!   axis only exists when `wrap` is off). `Scroll` deltas the widget
//!   cannot consume return `Ignored` so an ancestor scroll region can
//!   take them — the same nested-chaining contract `ScrollView` uses.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::text_area::TextArea;
//!
//! let area = TextArea::new()
//!     .label("Notes")
//!     .placeholder("Write here...");
//! assert_eq!(area.label, "Notes");
//! assert!(area.value().is_empty());
//! ```

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU32, Ordering};

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_access::{CaretTracker, TextAffinity, TextSelection};
use martensite_blessed::{CodeEditor, Cursor};
use martensite_core::paint::TextShaper;
use martensite_core::shape::Shape;
use martensite_core::widget::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    SemanticAction, Widget, WidgetEvent,
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
/// Scrollbar track colour.
const TRACK_COLOR: [u8; 4] = [235, 237, 240, 255];
/// Scrollbar thumb colour.
const THUMB_COLOR: [u8; 4] = [160, 166, 176, 255];
/// Scrollbar thumb colour while dragged.
const THUMB_ACTIVE: [u8; 4] = [120, 126, 138, 255];

/// Bounded undo history — `TextInput`'s depth.
const UNDO_LIMIT: usize = 100;

/// Word-jump boundaries of `line` in CHARACTER positions (the
/// `Cursor::column` unit): the edges of every non-whitespace UAX#29
/// word segment plus the two extremes — the same target set
/// `TextInput`'s `word_edges` computes in bytes.
fn char_word_edges(line: &str) -> Vec<usize> {
    let mut edges = Vec::new();
    for (i, seg) in line.split_word_bound_indices() {
        if !seg.trim().is_empty() {
            edges.push(line[..i].chars().count());
            edges.push(line[..i].chars().count() + seg.chars().count());
        }
    }
    edges.push(0);
    edges.push(line.chars().count());
    edges.sort_unstable();
    edges.dedup();
    edges
}

/// Horizontal inset for the editable text.
const TEXT_PAD_X: f32 = 8.0;
/// Vertical inset for the editable text.
const TEXT_PAD_Y: f32 = 6.0;
/// Font size in logical pt for the editable text.
const FONT_PT: f32 = 14.0;
/// Row height in logical pt — the `size * 1.25` line height the shared
/// [`crate::text_paint::TextPainter`] emits, so painted rows, caret
/// geometry, and hit-testing share one grid.
const LINE_PT: f32 = FONT_PT * 1.25;
/// Scrollbar strip thickness in logical pt.
const BAR_W: f32 = 8.0;
/// Minimum scrollbar thumb length in logical pt.
const MIN_THUMB: f32 = 24.0;
/// Width of the selection sliver painted past a covered line break.
const EOL_SLIVER: f32 = 4.0;

/// Selection granularity for an in-progress drag — chosen by the
/// initiating press's click count: single-click drags by character,
/// double-click drags by word, triple-click drags by whole lines.
/// Same vocabulary `TextInput` uses, extended to the multiline case.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
enum SelectGranularity {
    /// Extend the selection one caret position at a time.
    #[default]
    Char,
    /// Extend the selection by whole words, keeping the initially
    /// clicked word fully covered on either drag direction.
    Word,
    /// Extend the selection by whole lines from the triple-clicked
    /// line.
    Line,
}

/// One painted row: the byte range `start..end` of `lines[line]`. When
/// soft-wrap is off every row is a whole logical line; when on, a long
/// line is broken into several rows at word boundaries.
#[derive(Copy, Clone, Debug, PartialEq)]
struct VisRow {
    /// Index into the buffer's line vector.
    line: usize,
    /// First byte of the row within the line.
    start: usize,
    /// Byte after the row's last glyph within the line.
    end: usize,
}

/// The wrap-resolved layout shared by paint, hit-testing, and
/// scrollbar math — all coordinates in device pixels.
struct TextLayout {
    /// Every visual row in document order.
    rows: Vec<VisRow>,
    /// The text viewport: widget bounds minus padding and shown bars.
    view: Rect,
    /// Vertical scrollbar track rect, present when content overflows.
    vbar: Option<Rect>,
    /// Horizontal scrollbar track rect (never present while wrapping).
    hbar: Option<Rect>,
    /// Content width in device px (widest logical line; equals the
    /// wrap width while wrapping).
    content_w: f32,
    /// Content height in device px (`rows.len() * line_h`).
    content_h: f32,
    /// Row height in device px.
    line_h: f32,
}

/// Byte offset of the `col`-th char boundary in `text`, clamped to
/// `text.len()` — the inverse of [`byte_col`]. `CodeEditor` cursor
/// columns are char indices; the shaped painter works in bytes.
fn col_byte(text: &str, col: usize) -> usize {
    text.char_indices()
        .nth(col)
        .map_or(text.len(), |(index, _)| index)
}

/// Char index of `byte` in `text` — the inverse of [`col_byte`].
fn byte_col(text: &str, byte: usize) -> usize {
    text[..byte.min(text.len())].chars().count()
}

/// The char column where the grapheme cluster containing `col`'s
/// predecessor starts — ArrowLeft's and Backspace's target. Caret
/// motion and deletion step whole extended grapheme clusters so
/// combining marks and ZWJ emoji sequences never split under the
/// caret, matching platform text-field behavior.
fn prev_grapheme_col(line: &str, col: usize) -> usize {
    let byte = col_byte(line, col);
    let prev = line[..byte]
        .grapheme_indices(true)
        .next_back()
        .map_or(0, |(i, _)| i);
    byte_col(line, prev)
}

/// The char column just past the grapheme cluster starting at `col`
/// — ArrowRight's and forward-Delete's target.
fn next_grapheme_col(line: &str, col: usize) -> usize {
    let byte = col_byte(line, col);
    let next = line[byte..]
        .graphemes(true)
        .next()
        .map_or(line.len(), |g| byte + g.len());
    byte_col(line, next)
}

/// Largest byte offset `<= byte` that is a char boundary — the
/// defensive floor applied before slicing for measurement.
fn floor_boundary(text: &str, byte: usize) -> usize {
    let mut b = byte.min(text.len());
    while b > 0 && !text.is_char_boundary(b) {
        b -= 1;
    }
    b
}

/// Breaks `lines` into visual rows. With `wrap` off every line is one
/// row; with it on, each line is greedily broken at UAX#29 word
/// boundaries that fit `wrap_w` (falling back to a mid-word break when
/// a single word overruns the row), measured through `x_of` — the
/// `(line_text, byte) -> device x` mapping produced by the same
/// painter that draws the glyphs. Whitespace at a break is folded into
/// the preceding row, matching platform word-wrap.
fn visual_rows(
    lines: &[String],
    wrap: bool,
    wrap_w: f32,
    x_of: &dyn Fn(&str, usize) -> f32,
) -> Vec<VisRow> {
    let wrap_w = wrap_w.max(1.0);
    let mut rows = Vec::new();
    for (li, line) in lines.iter().enumerate() {
        if !wrap {
            rows.push(VisRow {
                line: li,
                start: 0,
                end: line.len(),
            });
            continue;
        }
        let mut start = 0usize;
        loop {
            let x0 = x_of(line, start);
            if start >= line.len() || x_of(line, line.len()) - x0 <= wrap_w {
                rows.push(VisRow {
                    line: li,
                    start,
                    end: line.len(),
                });
                break;
            }
            // Largest word-segment end that still fits the row.
            let mut fit = start;
            for (i, seg) in line[start..].split_word_bound_indices() {
                let end = start + i + seg.len();
                if x_of(line, end) - x0 <= wrap_w {
                    fit = end;
                } else {
                    break;
                }
            }
            if fit <= start {
                // The first segment alone overruns the row — break
                // mid-word at the last fitting char boundary (always
                // at least one char so the loop makes progress).
                for (b, _) in line[start..].char_indices().skip(1) {
                    if x_of(line, start + b) - x0 > wrap_w {
                        break;
                    }
                    fit = start + b;
                }
                if fit <= start {
                    fit = start + line[start..].chars().next().map_or(1, char::len_utf8);
                }
            }
            rows.push(VisRow {
                line: li,
                start,
                end: fit,
            });
            // Fold the break whitespace into the row just ended — the
            // next row starts at the first non-space char.
            start = fit;
            while start < line.len() && line[start..].starts_with(char::is_whitespace) {
                start += line[start..].chars().next().map_or(1, char::len_utf8);
            }
        }
    }
    rows
}

/// A multiline plain-text editor with an accessible label.
///
/// The document is held by a [`martensite_blessed::CodeEditor`] — the
/// same headless line-buffer model `EditorPanel` drives in the
/// industrial-dashboard example — so multi-line inserts, range
/// deletes, and word/line selection come from the tested model rather
/// than a parallel implementation. The widget uses a single caret (the
/// model's primary cursor).
///
/// # Examples
///
/// ```
/// use martensite::widgets::TextArea;
///
/// let area = TextArea::new()
///     .label("Bio")
///     .with_value("line one\nline two")
///     .placeholder("Tell us about yourself");
/// assert_eq!(area.value(), "line one\nline two");
/// ```
pub struct TextArea {
    /// The accessible label for the text area.
    pub label: String,
    /// Placeholder text shown when the value is empty.
    pub placeholder: String,
    /// Whether the text area is enabled.
    pub enabled: bool,
    /// Whether the text area is read-only.
    pub read_only: bool,
    /// Soft-wrap long lines at the viewport edge. `true` by default;
    /// when `false` long lines clip and the horizontal scrollbar and
    /// scroll axis appear instead.
    pub wrap: bool,
    /// Minimum height hint in rows, honoured by `measure`.
    pub min_lines: usize,
    /// Maximum height hint in rows, honoured by `measure` as a cap on
    /// the desired height (`None` grows with content).
    pub max_lines: Option<usize>,
    /// The line buffer + caret/selection model.
    editor: CodeEditor,
    /// Cached bounds from the last layout pass.
    cached_bounds: Rect,
    /// Shared shaped-text painter — when set, `paint` emits real
    /// `GlyphRun`s and hit-testing/caret geometry shape exactly like
    /// the glyphs; without it text falls back to `DrawText`
    /// placeholder boxes. See [`crate::text_paint`].
    text_painter: Option<crate::text_paint::SharedTextPainter>,
    /// Whether the area currently holds keyboard focus. Updated by the
    /// `FocusGained`/`FocusLost` widget events.
    focused: bool,
    /// Shift state tracked from `KeyPressed`/`KeyReleased` —
    /// `WidgetEvent::KeyPressed` carries no modifier state (F17).
    shift_held: bool,
    /// Word-modifier state (Ctrl/Cmd/Alt) tracked the same way —
    /// `parse_key_chord` covers `+`-joined chord names, this covers
    /// platforms that deliver the modifier as its own key event.
    word_mod_held: bool,
    /// Bounded undo history — one [`CodeEditor`] snapshot per user
    /// edit (text + cursors + selection), `TextInput`'s model.
    undo: VecDeque<CodeEditor>,
    /// Redo branch cleared by every new edit.
    redo: VecDeque<CodeEditor>,
    /// Drag-select in progress; the widget holds pointer capture.
    dragging: bool,
    /// The granularity the current drag selects with — set from the
    /// initiating press's `count` (single → char, double → word,
    /// triple → line).
    drag_granularity: SelectGranularity,
    /// The word selected by the double-click that opened a `Word`
    /// drag — its far edge becomes the anchor so the initial word
    /// stays fully covered whichever way the pointer moves.
    drag_word: Option<(Cursor, Cursor)>,
    /// The line triple-clicked to open a `Line` drag.
    drag_line: usize,
    /// Scrollbar-thumb drag state: `(vertical, grab offset inside the
    /// thumb)`, while the pointer drags a bar.
    bar_drag: Option<(bool, f32)>,
    /// Device pixels per logical point, cached in `layout` — pointer
    /// hit-testing needs it because `EventContext` carries no scale.
    scale: f32,
    /// Horizontal scroll of the text block (device px, f32 bits) —
    /// only nonzero while `wrap` is off. Atomic interior mutation
    /// because `paint` keeps the caret inside the field and `paint`
    /// takes `&self` (`Widget` requires `Sync`).
    scroll_x: AtomicU32,
    /// Vertical scroll of the text block (device px, f32 bits) — the
    /// caret-following counterpart of `scroll_x`.
    scroll_y: AtomicU32,
    /// Set on every buffer change — drained by [`Self::take_edited`].
    edited: bool,
    /// Optional caret/selection tracker for accessibility text
    /// selection support — the same seam [`crate::widgets::text::Text`]
    /// exposes. Kept in sync with the model's selection so
    /// `apply_to_node` reports the live caret when the area is focused.
    caret: Option<CaretTracker>,
    /// Sticky preferred column for vertical caret moves — `None` after
    /// any horizontal move, click, or edit.
    preferred_col: Option<usize>,
    /// In-flight IME composition — the preedit string and its caret
    /// byte-range, rendered underlined at the insertion caret until the
    /// matching `ImeCommitted` arrives (or an empty preedit clears it).
    preedit: Option<(String, Option<(usize, usize)>)>,
}

impl Clone for TextArea {
    fn clone(&self) -> Self {
        Self {
            label: self.label.clone(),
            placeholder: self.placeholder.clone(),
            enabled: self.enabled,
            read_only: self.read_only,
            wrap: self.wrap,
            min_lines: self.min_lines,
            max_lines: self.max_lines,
            editor: self.editor.clone(),
            cached_bounds: self.cached_bounds,
            text_painter: self.text_painter.clone(),
            focused: self.focused,
            shift_held: self.shift_held,
            word_mod_held: self.word_mod_held,
            undo: self.undo.clone(),
            redo: self.redo.clone(),
            dragging: self.dragging,
            drag_granularity: self.drag_granularity,
            drag_word: self.drag_word,
            drag_line: self.drag_line,
            bar_drag: self.bar_drag,
            scale: self.scale,
            scroll_x: AtomicU32::new(self.scroll_x.load(Ordering::Relaxed)),
            scroll_y: AtomicU32::new(self.scroll_y.load(Ordering::Relaxed)),
            edited: self.edited,
            caret: self.caret.clone(),
            preferred_col: self.preferred_col,
            preedit: self.preedit.clone(),
        }
    }
}

impl Default for TextArea {
    fn default() -> Self {
        Self::new()
    }
}

impl TextArea {
    /// Creates an empty multiline text area. Set the accessible label
    /// with [`label`](Self::label) or by assigning `area.label`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TextArea;
    ///
    /// let area = TextArea::new();
    /// assert!(area.value().is_empty());
    /// assert!(area.enabled);
    /// assert!(!area.read_only);
    /// assert!(area.wrap);
    /// ```
    pub fn new() -> Self {
        Self {
            label: String::new(),
            placeholder: String::new(),
            enabled: true,
            read_only: false,
            wrap: true,
            min_lines: 3,
            max_lines: None,
            editor: CodeEditor::new(""),
            cached_bounds: Rect::default(),
            text_painter: None,
            focused: false,
            shift_held: false,
            word_mod_held: false,
            undo: VecDeque::new(),
            redo: VecDeque::new(),
            dragging: false,
            drag_granularity: SelectGranularity::Char,
            drag_word: None,
            drag_line: 0,
            bar_drag: None,
            scale: 1.0,
            scroll_x: AtomicU32::new(0),
            scroll_y: AtomicU32::new(0),
            edited: false,
            caret: None,
            preferred_col: None,
            preedit: None,
        }
    }

    /// Sets the accessible label.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TextArea;
    ///
    /// let area = TextArea::new().label("Description");
    /// assert_eq!(area.label, "Description");
    /// ```
    #[inline]
    #[must_use]
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Sets the current text value (builder form — see
    /// [`set_value`](Self::set_value) for the mutable version).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TextArea;
    ///
    /// let area = TextArea::new().with_value("hello\nworld");
    /// assert_eq!(area.value(), "hello\nworld");
    /// ```
    #[inline]
    #[must_use]
    pub fn with_value(mut self, value: impl Into<String>) -> Self {
        self.set_value(value);
        self
    }

    /// Sets the placeholder text.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TextArea;
    ///
    /// let area = TextArea::new().placeholder("Type query...");
    /// assert_eq!(area.placeholder, "Type query...");
    /// ```
    #[inline]
    #[must_use]
    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    /// Sets whether the text area is enabled.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TextArea;
    ///
    /// let area = TextArea::new().enabled(false);
    /// assert!(!area.enabled);
    /// ```
    #[inline]
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Sets whether the text area is read-only.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TextArea;
    ///
    /// let area = TextArea::new().read_only(true);
    /// assert!(area.read_only);
    /// ```
    #[inline]
    #[must_use]
    pub fn read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }

    /// Sets soft-wrap: `true` (default) breaks long lines into visual
    /// rows at the viewport edge; `false` clips them and enables
    /// horizontal scrolling.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TextArea;
    ///
    /// let area = TextArea::new().wrap(false);
    /// assert!(!area.wrap);
    /// ```
    #[inline]
    #[must_use]
    pub fn wrap(mut self, wrap: bool) -> Self {
        self.wrap = wrap;
        self
    }

    /// Sets the minimum height hint in rows.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TextArea;
    ///
    /// let area = TextArea::new().min_lines(5);
    /// assert_eq!(area.min_lines, 5);
    /// ```
    #[inline]
    #[must_use]
    pub fn min_lines(mut self, min_lines: usize) -> Self {
        self.min_lines = min_lines;
        self
    }

    /// Sets the maximum height hint in rows — `measure` never asks for
    /// more than this many rows of text.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TextArea;
    ///
    /// let area = TextArea::new().max_lines(8);
    /// assert_eq!(area.max_lines, Some(8));
    /// ```
    #[inline]
    #[must_use]
    pub fn max_lines(mut self, max_lines: usize) -> Self {
        self.max_lines = Some(max_lines);
        self
    }

    /// Returns the full document text, lines joined by `\n`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TextArea;
    ///
    /// let area = TextArea::new().with_value("a\nb");
    /// assert_eq!(area.value(), "a\nb");
    /// ```
    #[inline]
    pub fn value(&self) -> String {
        self.editor.text()
    }

    /// Sets the value (mutable version for programmatic updates).
    /// Collapses the caret to the end of the document and resets the
    /// scroll offset — a stale position could land past the end after
    /// a shorter write. Flags the edit for [`take_edited`](Self::take_edited).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TextArea;
    ///
    /// let mut area = TextArea::new();
    /// area.set_value("one\ntwo");
    /// assert_eq!(area.value(), "one\ntwo");
    /// ```
    pub fn set_value(&mut self, value: impl Into<String>) {
        self.editor = CodeEditor::new(&value.into());
        // A programmatic write is not a user edit — it drops the undo
        // history (Qt's setText behaves the same).
        self.undo.clear();
        self.redo.clear();
        // Programmatic writes collapse the caret to the end — matching
        // `TextInput::set_value`.
        let last = self.editor.lines().len().saturating_sub(1);
        let col = self.editor.lines()[last].chars().count();
        self.editor.set_cursors(vec![Cursor::new(last, col)]);
        self.set_scroll_x(0.0);
        self.set_scroll_y(0.0);
        self.dragging = false;
        self.bar_drag = None;
        self.preferred_col = None;
        // An in-flight IME composition belonged to the old document —
        // painting it over the replacement would be a stale ghost
        // (the same reset FocusLost performs).
        self.preedit = None;
        self.mark_edited();
    }

    /// Drains the edit flag: `Some(())` once per buffer change since
    /// the last call, `None` when nothing changed — the widget's
    /// value-change seam, since `Widget::event` returns no payload.
    /// Programmatic [`set_value`](Self::set_value) writes flag the edit
    /// too.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TextArea;
    ///
    /// let mut area = TextArea::new();
    /// assert_eq!(area.take_edited(), None);
    /// area.set_value("x");
    /// assert_eq!(area.take_edited(), Some(()));
    /// assert_eq!(area.take_edited(), None);
    /// ```
    pub fn take_edited(&mut self) -> Option<()> {
        if self.edited {
            self.edited = false;
            Some(())
        } else {
            None
        }
    }

    /// The primary caret — a `(line, char-column)` [`Cursor`] into the
    /// line buffer.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TextArea;
    ///
    /// let area = TextArea::new();
    /// assert_eq!(area.cursor().line, 0);
    /// assert_eq!(area.cursor().column, 0);
    /// ```
    #[inline]
    pub fn cursor(&self) -> Cursor {
        self.primary()
    }

    /// The current scroll offset in device pixels, clamped by paint
    /// and scroll events to `0..=content - viewport` per axis.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TextArea;
    ///
    /// let area = TextArea::new();
    /// assert_eq!(area.scroll_offset(), glam::Vec2::ZERO);
    /// ```
    #[inline]
    pub fn scroll_offset(&self) -> Vec2 {
        Vec2::new(self.scroll_x(), self.scroll_y())
    }

    /// Returns the cached bounds from the last layout pass.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TextArea;
    ///
    /// let area = TextArea::new();
    /// assert_eq!(area.cached_bounds().size.x, 0.0);
    /// ```
    #[inline]
    pub fn cached_bounds(&self) -> Rect {
        self.cached_bounds
    }

    /// Shares a [`crate::text_paint::TextPainter`] so `paint` emits real
    /// glyph runs instead of `DrawText` placeholder boxes and caret
    /// geometry matches the shaped text.
    #[must_use]
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Attaches a [`CaretTracker`] so the accessibility node reports
    /// the live text selection while focused — the same seam
    /// [`crate::widgets::text::Text::with_caret_tracker`] provides. The
    /// widget keeps the tracker's selection synced to the model on
    /// every caret move.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TextArea;
    /// use martensite_access::{CaretTracker, TextAffinity, TextSelection};
    /// use accesskit::NodeId;
    ///
    /// let tracker = CaretTracker::new(NodeId(1), TextSelection::caret(0, TextAffinity::Downstream));
    /// let area = TextArea::new().with_caret_tracker(tracker);
    /// assert!(area.caret_tracker().is_some());
    /// ```
    #[inline]
    #[must_use]
    pub fn with_caret_tracker(mut self, caret: CaretTracker) -> Self {
        self.caret = Some(caret);
        self
    }

    /// Sets the caret/selection tracker mutably.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TextArea;
    /// use martensite_access::{CaretTracker, TextAffinity, TextSelection};
    /// use accesskit::NodeId;
    ///
    /// let mut area = TextArea::new();
    /// area.set_caret_tracker(CaretTracker::new(NodeId(1), TextSelection::caret(0, TextAffinity::Downstream)));
    /// assert!(area.caret_tracker().is_some());
    /// ```
    #[inline]
    pub fn set_caret_tracker(&mut self, caret: CaretTracker) {
        self.caret = Some(caret);
    }

    /// Returns the caret/selection tracker, if any.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TextArea;
    ///
    /// let area = TextArea::new();
    /// assert!(area.caret_tracker().is_none());
    /// ```
    #[inline]
    pub fn caret_tracker(&self) -> Option<&CaretTracker> {
        self.caret.as_ref()
    }

    /// The in-flight IME composition string, if one is being composed.
    ///
    /// Set by `WidgetEvent::ImePreedit` and cleared by `ImeCommitted`,
    /// an empty preedit, or focus loss. The string is rendered
    /// underlined at the caret — it is not part of
    /// [`value`](Self::value) until committed.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TextArea;
    ///
    /// assert!(TextArea::new().preedit().is_none());
    /// ```
    #[inline]
    pub fn preedit(&self) -> Option<&str> {
        self.preedit.as_ref().map(|(t, _)| t.as_str())
    }

    /// The caret byte-range inside the current [`preedit`](Self::preedit)
    /// string, if the IME reports one.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TextArea;
    ///
    /// assert!(TextArea::new().preedit_cursor().is_none());
    /// ```
    #[inline]
    pub fn preedit_cursor(&self) -> Option<(usize, usize)> {
        self.preedit.as_ref().and_then(|(_, c)| *c)
    }

    // ---- editing-model helpers --------------------------------------

    /// The primary caret — `cursors[0]`, the only cursor this widget
    /// drives.
    fn primary(&self) -> Cursor {
        self.editor.cursors().first().copied().unwrap_or_default()
    }

    /// Char length of `line`, clamped to a real line index.
    fn line_len(&self, line: usize) -> usize {
        let lines = self.editor.lines();
        lines[line.min(lines.len().saturating_sub(1))]
            .chars()
            .count()
    }

    /// Flat document char index for `c` — what `CaretTracker`'s
    /// `TextSelection` numbers positions in.
    fn flat_index(&self, c: Cursor) -> usize {
        self.editor.lines()[..c.line.min(self.editor.lines().len())]
            .iter()
            .map(|l| l.chars().count() + 1)
            .sum::<usize>()
            + c.column
    }

    /// Flags a buffer change and collapses the sticky column —
    /// caret-syncing bookkeeping shared by every mutation.
    fn mark_edited(&mut self) {
        self.edited = true;
        self.preferred_col = None;
        self.sync_caret_tracker();
    }

    /// Pushes the whole editor state (text, cursors, selection) onto
    /// the bounded undo stack and clears the redo branch — every
    /// user-driven mutation routes through here so one logical edit
    /// is one undo step, `TextInput`'s snapshot model.
    fn record_undo(&mut self) {
        self.undo.push_back(self.editor.clone());
        if self.undo.len() > UNDO_LIMIT {
            self.undo.pop_front();
        }
        self.redo.clear();
    }

    /// Restores the newest undo snapshot, pushing the current state
    /// onto the redo stack.
    fn undo_edit(&mut self) -> bool {
        let Some(snap) = self.undo.pop_back() else {
            return false;
        };
        self.redo
            .push_back(std::mem::replace(&mut self.editor, snap));
        self.mark_edited();
        true
    }

    /// Re-applies the newest undone snapshot.
    fn redo_edit(&mut self) -> bool {
        let Some(snap) = self.redo.pop_back() else {
            return false;
        };
        self.undo
            .push_back(std::mem::replace(&mut self.editor, snap));
        self.mark_edited();
        true
    }

    /// The nearest word-jump boundary strictly before `pos` —
    /// Ctrl/Cmd+ArrowLeft's target: the previous word edge on the
    /// line, else the previous line's end, else the origin.
    fn word_left(&self, pos: Cursor) -> Cursor {
        let line = &self.editor.lines()[pos.line];
        let col_char = pos.column.min(line.chars().count());
        let edges = char_word_edges(line);
        if let Some(&e) = edges.iter().rev().find(|&&e| e < col_char) {
            return Cursor::new(pos.line, e);
        }
        if pos.line > 0 {
            return Cursor::new(pos.line - 1, self.line_len(pos.line - 1));
        }
        Cursor::new(0, 0)
    }

    /// The nearest word-jump boundary strictly after `pos`.
    fn word_right(&self, pos: Cursor) -> Cursor {
        let line = &self.editor.lines()[pos.line];
        let col_char = pos.column.min(line.chars().count());
        let edges = char_word_edges(line);
        if let Some(&e) = edges.iter().find(|&&e| e > col_char) {
            return Cursor::new(pos.line, e);
        }
        let last = self.editor.lines().len().saturating_sub(1);
        if pos.line < last {
            return Cursor::new(pos.line + 1, 0);
        }
        Cursor::new(last, self.line_len(last))
    }

    /// Ctrl/Cmd+Backspace — deletes the selection, else the run from
    /// the previous word edge to the caret (crossing the line break
    /// at column 0, matching `caret_left`).
    fn delete_word_back(&mut self) {
        if self.editor.selection().is_some() {
            self.record_undo();
            self.editor.delete_selection();
            self.mark_edited();
            return;
        }
        let cur = self.primary();
        let start = self.word_left(cur);
        if start != cur {
            self.record_undo();
            self.editor.delete_range(start, cur);
            self.mark_edited();
        }
    }

    /// Ctrl/Cmd+Delete — deletes the selection, else the run from the
    /// caret to the next word edge.
    fn delete_word_forward(&mut self) {
        if self.editor.selection().is_some() {
            self.record_undo();
            self.editor.delete_selection();
            self.mark_edited();
            return;
        }
        let cur = self.primary();
        let end = self.word_right(cur);
        if end != cur {
            self.record_undo();
            self.editor.delete_range(cur, end);
            self.mark_edited();
        }
    }

    /// Pushes the model's selection into an attached `CaretTracker`
    /// (anchor first, focus = primary caret) — the a11y mirror of the
    /// editing state.
    fn sync_caret_tracker(&mut self) {
        let focus = self.primary();
        let anchor = match self.editor.selection() {
            Some((lo, hi)) => {
                if focus == hi {
                    lo
                } else {
                    hi
                }
            }
            None => focus,
        };
        let a = self.flat_index(anchor);
        let f = self.flat_index(focus);
        if let Some(tracker) = &mut self.caret {
            tracker.set_selection(TextSelection::new(a, f, TextAffinity::Downstream));
        }
    }

    /// Moves the caret to `target` — extending the selection from the
    /// current caret when `extend`, collapsing it otherwise.
    fn move_to(&mut self, target: Cursor, extend: bool) {
        if extend {
            self.editor.extend_selection_to(target);
        } else {
            self.editor.set_cursors(vec![target]);
        }
        self.sync_caret_tracker();
    }

    /// Inserts `text` at the caret, replacing any selection. Newlines
    /// are kept (this is the multiline field — `\r\n`/`\r` normalize
    /// to `\n`); other control characters except `\t` are dropped.
    fn insert_str(&mut self, text: &str) {
        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        let clean: String = normalized
            .chars()
            .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
            .collect();
        // A pure-selection delete still counts as an edit (an empty
        // paste over a selection removes it).
        if clean.is_empty() && self.editor.selection().is_none() {
            return;
        }
        self.record_undo();
        self.editor.insert(&clean);
        self.mark_edited();
    }

    fn backspace(&mut self) {
        // A no-op delete (caret at 0:0, no selection) must not flag an
        // edit — the same guard `EditorPanel` uses.
        let can_delete = self.editor.selection().is_some()
            || self
                .editor
                .cursors()
                .iter()
                .any(|c| c.line > 0 || c.column > 0);
        if can_delete {
            self.record_undo();
            self.editor.delete_backward();
            self.mark_edited();
        }
    }

    /// Forward-delete — `CodeEditor` exposes only `delete_backward`,
    /// so this is a `delete_range` of the char after the caret, or the
    /// line join when the caret sits at EOL.
    fn delete_forward(&mut self) {
        if self.editor.selection().is_some() {
            self.record_undo();
            self.editor.delete_selection();
            self.mark_edited();
            return;
        }
        let cur = self.primary();
        if cur.column < self.line_len(cur.line) {
            let end = next_grapheme_col(&self.editor.lines()[cur.line], cur.column);
            self.record_undo();
            self.editor.delete_range(cur, Cursor::new(cur.line, end));
            self.mark_edited();
        } else if cur.line + 1 < self.editor.lines().len() {
            self.record_undo();
            self.editor.delete_range(cur, Cursor::new(cur.line + 1, 0));
            self.mark_edited();
        }
    }

    /// Writes the selection to the OS clipboard (no-op when nothing is
    /// selected) — the same per-call backend seam `TextInput` uses:
    /// the platform clipboard objects are not `Send` and cannot live
    /// on the widget (`Widget: Send + Sync`).
    fn copy_selection(&self) {
        if let Some(text) = self.editor.selected_text() {
            let mut cb = martensite_clipboard::default_platform_clipboard();
            cb.set_contents(&martensite_clipboard::ClipboardItem::new().offer_text(&text));
        }
    }

    /// Inserts the OS clipboard's text payload at the caret (no-op
    /// when the clipboard holds none). Newlines are preserved.
    fn paste_clipboard(&mut self) {
        let cb = martensite_clipboard::default_platform_clipboard();
        if let Some(bytes) = cb.get_contents(martensite_clipboard::clipboard::MIME_TEXT_PLAIN) {
            let text = String::from_utf8_lossy(&bytes);
            self.insert_str(&text);
        }
    }

    // ---- caret motion -------------------------------------------------

    fn caret_left(&mut self, extend: bool) {
        self.preferred_col = None;
        // Collapse to the selection's left edge rather than stepping —
        // the platform convention `TextInput` follows.
        if !extend {
            if let Some((lo, _)) = self.editor.selection() {
                self.editor.set_cursors(vec![lo]);
                self.sync_caret_tracker();
                return;
            }
        }
        let cur = self.primary();
        let line = &self.editor.lines()[cur.line];
        let target = if cur.column > 0 {
            Cursor::new(cur.line, prev_grapheme_col(line, cur.column))
        } else if cur.line > 0 {
            // Cross the line break — prose-editor semantics, like
            // QTextEdit/GtkTextView.
            Cursor::new(cur.line - 1, self.line_len(cur.line - 1))
        } else {
            cur
        };
        self.move_to(target, extend);
    }

    fn caret_right(&mut self, extend: bool) {
        self.preferred_col = None;
        if !extend {
            if let Some((_, hi)) = self.editor.selection() {
                self.editor.set_cursors(vec![hi]);
                self.sync_caret_tracker();
                return;
            }
        }
        let cur = self.primary();
        let line = &self.editor.lines()[cur.line];
        let target = if cur.column < self.line_len(cur.line) {
            Cursor::new(cur.line, next_grapheme_col(line, cur.column))
        } else if cur.line + 1 < self.editor.lines().len() {
            Cursor::new(cur.line + 1, 0)
        } else {
            cur
        };
        self.move_to(target, extend);
    }

    /// Vertical move by `rows` logical lines (v1: logical, not visual
    /// wrap rows — see the module docs). The sticky preferred column
    /// survives consecutive vertical moves; a selection open without
    /// `extend` collapses to the edge in the direction of travel,
    /// matching Left/Right.
    fn caret_vertical(&mut self, rows: isize, extend: bool) {
        if !extend {
            if let Some((lo, hi)) = self.editor.selection() {
                let edge = if rows < 0 { lo } else { hi };
                self.editor.set_cursors(vec![edge]);
                self.sync_caret_tracker();
                return;
            }
        }
        let cur = self.primary();
        let pref = self.preferred_col.unwrap_or(cur.column);
        self.preferred_col = Some(pref);
        let last = self.editor.lines().len().saturating_sub(1);
        let line = (cur.line as isize + rows).clamp(0, last as isize) as usize;
        let col = pref.min(self.line_len(line));
        self.move_to(Cursor::new(line, col), extend);
    }

    /// Rows that fit the viewport — the PageUp/PageDown distance.
    fn page_rows(&mut self, bounds: Rect) -> usize {
        let lay = self.geometry_now(bounds);
        (lay.view.height() / lay.line_h).floor().max(1.0) as usize
    }

    // ---- scroll geometry ----------------------------------------------

    /// The text run's horizontal scroll in device px.
    fn scroll_x(&self) -> f32 {
        f32::from_bits(self.scroll_x.load(Ordering::Relaxed))
    }

    /// Stores the horizontal scroll in device px.
    fn set_scroll_x(&self, v: f32) {
        self.scroll_x.store(v.to_bits(), Ordering::Relaxed);
    }

    /// The text block's vertical scroll in device px.
    fn scroll_y(&self) -> f32 {
        f32::from_bits(self.scroll_y.load(Ordering::Relaxed))
    }

    /// Stores the vertical scroll in device px.
    fn set_scroll_y(&self, v: f32) {
        self.scroll_y.store(v.to_bits(), Ordering::Relaxed);
    }

    /// Device-pixel offset of the caret boundary at `byte_idx` inside
    /// `line`, from the same painter that emits the glyphs — the
    /// multiline version of `TextInput`'s `offset_x`. Falls back to the
    /// ambient painter's `measure_text` and then the per-char estimate
    /// only when no shaped painter exists at all. `ambient` and
    /// `char_w` arrive pre-resolved so callers can build a `cx`-free
    /// closure — `PaintContext` can't be borrowed across `cx.list`
    /// mutations inside the paint loop.
    fn offset_for(
        &self,
        ambient: Option<&(dyn TextShaper + Send + Sync)>,
        char_w: f32,
        line: &str,
        byte_idx: usize,
        font_px: f32,
    ) -> f32 {
        let b = floor_boundary(line, byte_idx);
        if let Some(p) = &self.text_painter {
            return p.caret_x(line, font_px, b);
        }
        if let Some(p) = crate::text_paint::resolve_painter(&self.text_painter, ambient) {
            if let Some(w) = p.measure_text(&line[..b], font_px) {
                return w;
            }
        }
        line[..b].chars().count() as f32 * char_w
    }

    /// Wrap-resolved geometry for `bounds` through `x_of` — the shared
    /// computation paint (shaped painter), hit-testing, and scrollbar
    /// math all derive rows and bar visibility from. When `wrap` is on
    /// the vertical bar's width is reserved up front so text never
    /// flows under the bar — which also breaks the wrap↔bar feedback
    /// loop.
    fn geometry(
        lines: &[String],
        wrap: bool,
        scale: f32,
        bounds: Rect,
        x_of: &dyn Fn(&str, usize) -> f32,
    ) -> TextLayout {
        let font_px = FONT_PT * scale;
        let line_h = font_px * 1.25;
        let pad_x = TEXT_PAD_X * scale;
        let pad_y = TEXT_PAD_Y * scale;
        let bar = BAR_W * scale;
        let inset_x = bounds.min_x() + pad_x;
        let inset_y = bounds.min_y() + pad_y;
        let inset_w = (bounds.width() - 2.0 * pad_x).max(0.0);
        let inset_h = (bounds.height() - 2.0 * pad_y).max(0.0);
        let wrap_w = if wrap {
            (inset_w - bar).max(8.0 * scale)
        } else {
            f32::INFINITY
        };
        let rows = visual_rows(lines, wrap, wrap_w, x_of);
        let content_h = rows.len() as f32 * line_h;
        let content_w = if wrap {
            wrap_w
        } else {
            lines.iter().map(|l| x_of(l, l.len())).fold(0.0, f32::max)
        };
        // Bar visibility — `ScrollView`'s two-pass: the vertical bar
        // steals width the horizontal check must see; a horizontal bar
        // then steals height that may re-trigger the vertical one.
        let mut show_v = content_h > inset_h + 0.5;
        let show_h = !wrap && content_w > (inset_w - if show_v { bar } else { 0.0 }) + 0.5;
        let view_h = (inset_h - if show_h { bar } else { 0.0 }).max(0.0);
        show_v = content_h > view_h + 0.5;
        let view_w = (inset_w - if show_v { bar } else { 0.0 }).max(0.0);
        let view = Rect::new(inset_x, inset_y, view_w, view_h);
        let vbar = show_v.then(|| Rect::new(view.max_x(), inset_y, bar, view_h));
        let hbar = show_h.then(|| Rect::new(inset_x, view.max_y(), view_w, bar));
        TextLayout {
            rows,
            view,
            vbar,
            hbar,
            content_w,
            content_h,
            line_h,
        }
    }

    /// Geometry through the widget's own painter — created lazily on
    /// first use, so areas that are never clicked or scrolled skip the
    /// font scan (the `byte_at_position` precedent in `TextInput`).
    fn geometry_now(&mut self, bounds: Rect) -> TextLayout {
        let font_px = FONT_PT * self.scale;
        let painter = self
            .text_painter
            .get_or_insert_with(crate::text_paint::shared_painter);
        Self::geometry(
            self.editor.lines(),
            self.wrap,
            self.scale,
            bounds,
            &|t: &str, b: usize| painter.caret_x(t, font_px, b),
        )
    }

    /// The `(row index, x offset within the row)` of `caret`'s
    /// insertion boundary — the visual position the caret and the
    /// caret-following scroll both need.
    fn caret_row_x(
        rows: &[VisRow],
        lines: &[String],
        caret: Cursor,
        x_of: &dyn Fn(&str, usize) -> f32,
    ) -> (usize, f32) {
        let line = &lines[caret.line.min(lines.len().saturating_sub(1))];
        let byte = col_byte(line, caret.column);
        let idx = rows
            .iter()
            .position(|r| r.line == caret.line && byte <= r.end)
            .or_else(|| rows.iter().rposition(|r| r.line == caret.line))
            .unwrap_or(0);
        let r = rows[idx.min(rows.len().saturating_sub(1))];
        let in_row = byte.clamp(r.start, r.end);
        (idx, x_of(line, in_row) - x_of(line, r.start))
    }

    /// Document position under the pointer, mapped through the shaped
    /// painter so clicks land on the glyph under them — the multiline
    /// version of `TextInput::byte_at_position`.
    fn cursor_at(&mut self, bounds: Rect, position: Vec2) -> Cursor {
        let lay = self.geometry_now(bounds);
        let row_i =
            ((position.y - lay.view.min_y() + self.scroll_y()) / lay.line_h).max(0.0) as usize;
        let row = lay.rows[row_i.min(lay.rows.len() - 1)];
        let font_px = FONT_PT * self.scale;
        let line = &self.editor.lines()[row.line];
        // `scroll_x` shifts the painted run left — add it back so the
        // click lands on the glyph under the pointer (wrap forces 0).
        let x = position.x - lay.view.min_x() + self.scroll_x();
        let painter = self
            .text_painter
            .get_or_insert_with(crate::text_paint::shared_painter);
        let byte = row.start + painter.byte_at(&line[row.start..row.end], font_px, x.max(0.0));
        Cursor::new(row.line, byte_col(line, byte.min(row.end)))
    }

    /// Scrolls by `delta`, clamped to the content's overflow — returns
    /// the actually-applied delta; `Vec2::ZERO` means nothing was
    /// consumed (the nested-chaining boundary `ScrollView` honours).
    fn scroll_by(&mut self, bounds: Rect, delta: Vec2) -> Vec2 {
        let lay = self.geometry_now(bounds);
        let max = Vec2::new(
            if self.wrap {
                0.0
            } else {
                (lay.content_w - lay.view.width()).max(0.0)
            },
            (lay.content_h - lay.view.height()).max(0.0),
        );
        let old = Vec2::new(self.scroll_x(), self.scroll_y());
        let new = (old + delta).clamp(Vec2::ZERO, max);
        self.set_scroll_x(new.x);
        self.set_scroll_y(new.y);
        new - old
    }

    /// Thumb rect inside a scrollbar `track` for the given scroll
    /// state — shared by `bar_press`, `bar_move`, and `paint`.
    fn thumb_rect(
        track: Rect,
        view_len: f32,
        content_len: f32,
        scroll: f32,
        min_thumb: f32,
        vertical: bool,
    ) -> Rect {
        let track_len = if vertical {
            track.height()
        } else {
            track.width()
        };
        let thumb_len = (track_len * view_len / content_len.max(f32::EPSILON))
            .max(min_thumb)
            .min(track_len);
        let max_scroll = (content_len - view_len).max(0.0);
        let travel = (track_len - thumb_len).max(0.0);
        let off = if max_scroll > 0.0 {
            travel * (scroll / max_scroll).clamp(0.0, 1.0)
        } else {
            0.0
        };
        if vertical {
            Rect::new(track.min_x(), track.min_y() + off, track.width(), thumb_len)
        } else {
            Rect::new(
                track.min_x() + off,
                track.min_y(),
                thumb_len,
                track.height(),
            )
        }
    }

    /// Hit-test a press against the scrollbar strips — grabbing the
    /// thumb opens a `bar_drag`, clicking the track pages. Returns
    /// whether the press was consumed by a bar.
    fn bar_press(&mut self, bounds: Rect, pos: Vec2) -> bool {
        let lay = self.geometry_now(bounds);
        let min_thumb = MIN_THUMB * self.scale;
        if let Some(track) = lay.vbar {
            if pos.x >= track.min_x() && pos.y >= track.min_y() && pos.y < track.max_y() {
                let thumb = Self::thumb_rect(
                    track,
                    lay.view.height(),
                    lay.content_h,
                    self.scroll_y(),
                    min_thumb,
                    true,
                );
                if thumb.contains(pos) {
                    self.bar_drag = Some((true, pos.y - thumb.min_y()));
                } else {
                    let dir = if pos.y < thumb.min_y() { -1.0 } else { 1.0 };
                    self.scroll_by(bounds, Vec2::new(0.0, dir * lay.view.height() * 0.9));
                }
                return true;
            }
        }
        if let Some(track) = lay.hbar {
            if pos.y >= track.min_y() && pos.x >= track.min_x() && pos.x < track.max_x() {
                let thumb = Self::thumb_rect(
                    track,
                    lay.view.width(),
                    lay.content_w,
                    self.scroll_x(),
                    min_thumb,
                    false,
                );
                if thumb.contains(pos) {
                    self.bar_drag = Some((false, pos.x - thumb.min_x()));
                } else {
                    let dir = if pos.x < thumb.min_x() { -1.0 } else { 1.0 };
                    self.scroll_by(bounds, Vec2::new(dir * lay.view.width() * 0.9, 0.0));
                }
                return true;
            }
        }
        false
    }

    /// Maps an in-progress thumb drag back to a scroll offset —
    /// `grab` preserves where inside the thumb the press landed.
    fn bar_move(&mut self, bounds: Rect, pos: Vec2) -> EventResponse {
        let Some((vertical, grab)) = self.bar_drag else {
            return EventResponse::Ignored;
        };
        let lay = self.geometry_now(bounds);
        let (track, view_len, content_len) = if vertical {
            (lay.vbar, lay.view.height(), lay.content_h)
        } else {
            (lay.hbar, lay.view.width(), lay.content_w)
        };
        if let Some(track) = track {
            let track_len = if vertical {
                track.height()
            } else {
                track.width()
            };
            let thumb_len = (track_len * view_len / content_len.max(f32::EPSILON))
                .max(MIN_THUMB * self.scale)
                .min(track_len);
            let max_scroll = (content_len - view_len).max(0.0);
            let travel = (track_len - thumb_len).max(0.0);
            let t = if vertical {
                pos.y - track.min_y() - grab
            } else {
                pos.x - track.min_x() - grab
            };
            let frac = if travel > 0.0 {
                (t / travel).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let v = frac * max_scroll;
            if vertical {
                self.set_scroll_y(v);
            } else {
                self.set_scroll_x(v);
            }
        }
        EventResponse::RequestRepaint
    }

    /// Keyboard handling for the focused area. `key` is the logical
    /// key name — plain characters arrive via `ImeCommitted`, while
    /// chords like Cmd+A arrive as the synthetic names
    /// `"SelectAll"`/`"Cut"`/`"Copy"`/`"Paste"` that the window layer
    /// dispatches or as `+`-joined chord names (`"Ctrl+ArrowLeft"`) —
    /// both resolve through `parse_key_chord`, `TextInput`'s seam.
    fn key_pressed(&mut self, key: &str, bounds: Rect) -> EventResponse {
        let (wm_chord, sh_chord, base) = crate::widgets::text_input::parse_key_chord(key);
        match base {
            "Control" | "Ctrl" | "Meta" | "Cmd" | "Super" | "Alt" | "Option" => {
                self.word_mod_held = true;
                return EventResponse::Handled;
            }
            "Shift" => {
                self.shift_held = true;
                return EventResponse::Handled;
            }
            _ => {}
        }
        let word = wm_chord || self.word_mod_held;
        let extend = sh_chord || self.shift_held;
        match base {
            "ArrowLeft" => {
                if word {
                    let target = self.word_left(self.primary());
                    self.preferred_col = None;
                    self.move_to(target, extend);
                } else {
                    self.caret_left(extend);
                }
                EventResponse::RequestRepaint
            }
            "ArrowRight" => {
                if word {
                    let target = self.word_right(self.primary());
                    self.preferred_col = None;
                    self.move_to(target, extend);
                } else {
                    self.caret_right(extend);
                }
                EventResponse::RequestRepaint
            }
            "ArrowUp" => {
                if word {
                    // Document edge — Cmd+Up on macOS.
                    self.preferred_col = None;
                    self.move_to(Cursor::new(0, 0), extend);
                } else {
                    self.caret_vertical(-1, extend);
                }
                EventResponse::RequestRepaint
            }
            "ArrowDown" => {
                if word {
                    let last = self.editor.lines().len().saturating_sub(1);
                    self.preferred_col = None;
                    self.move_to(Cursor::new(last, self.line_len(last)), extend);
                } else {
                    self.caret_vertical(1, extend);
                }
                EventResponse::RequestRepaint
            }
            "PageUp" => {
                let rows = self.page_rows(bounds) as isize;
                self.caret_vertical(-rows, extend);
                EventResponse::RequestRepaint
            }
            "PageDown" => {
                let rows = self.page_rows(bounds) as isize;
                self.caret_vertical(rows, extend);
                EventResponse::RequestRepaint
            }
            "Home" => {
                self.preferred_col = None;
                // Word-modifier Home jumps to the document start —
                // Ctrl+Home on Windows/Linux, Cmd+Up's sibling.
                let target = if word {
                    Cursor::new(0, 0)
                } else {
                    Cursor::new(self.primary().line, 0)
                };
                self.move_to(target, extend);
                EventResponse::RequestRepaint
            }
            "End" => {
                self.preferred_col = None;
                let target = if word {
                    let last = self.editor.lines().len().saturating_sub(1);
                    Cursor::new(last, self.line_len(last))
                } else {
                    let cur = self.primary();
                    Cursor::new(cur.line, self.line_len(cur.line))
                };
                self.move_to(target, extend);
                EventResponse::RequestRepaint
            }
            "SelectAll" => {
                self.editor.select_all();
                self.sync_caret_tracker();
                EventResponse::RequestRepaint
            }
            "a" | "A" if word => {
                self.editor.select_all();
                self.sync_caret_tracker();
                EventResponse::RequestRepaint
            }
            "Copy" => {
                self.copy_selection();
                EventResponse::Handled
            }
            "c" | "C" if word => {
                self.copy_selection();
                EventResponse::Handled
            }
            "Cut" | "x" | "X" if !self.read_only && (word || base == "Cut") => {
                if self.editor.selection().is_some() {
                    self.copy_selection();
                    self.record_undo();
                    self.editor.delete_selection();
                    self.mark_edited();
                }
                EventResponse::RequestRepaint
            }
            "Paste" | "v" | "V" if !self.read_only && (word || base == "Paste") => {
                self.paste_clipboard();
                EventResponse::RequestRepaint
            }
            "Backspace" if !self.read_only => {
                if word {
                    self.delete_word_back();
                } else {
                    self.backspace();
                }
                EventResponse::RequestRepaint
            }
            "Delete" if !self.read_only => {
                if word {
                    self.delete_word_forward();
                } else {
                    self.delete_forward();
                }
                EventResponse::RequestRepaint
            }
            // Multiline editors own Enter — it inserts the newline
            // `TextInput` strips.
            "Enter" if !self.read_only => {
                self.insert_str("\n");
                EventResponse::RequestRepaint
            }
            // …and Tab, which inserts a `\t` rather than moving focus
            // (documented in the module docs; `TextInput` leaves Tab
            // to focus traversal).
            "Tab" if !self.read_only => {
                self.insert_str("\t");
                EventResponse::RequestRepaint
            }
            "Undo" | "z" | "Z" if !self.read_only && (word || base == "Undo") => {
                // Cmd+Shift+Z is redo on macOS — `extend` marks it.
                let did = if base != "Undo" && extend {
                    self.redo_edit()
                } else {
                    self.undo_edit()
                };
                if did {
                    EventResponse::RequestRepaint
                } else {
                    EventResponse::Handled
                }
            }
            "Redo" | "y" | "Y" if !self.read_only && (word || base == "Redo") => {
                if self.redo_edit() {
                    EventResponse::RequestRepaint
                } else {
                    EventResponse::Handled
                }
            }
            "Escape" if self.editor.selection().is_some() => {
                // Collapse the selection onto its head — the
                // `EditorPanel` convention.
                let cur = self.primary();
                self.editor.set_cursors(vec![cur]);
                self.sync_caret_tracker();
                EventResponse::RequestRepaint
            }
            _ => EventResponse::Ignored,
        }
    }
}

impl Widget for TextArea {
    #[cfg(feature = "devtools-timemachine")]
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }

    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        // Desired width is a fixed comfortable column; desired height
        // grows with content between `min_lines` and `max_lines`.
        let min_w = cx.pt(160.0).min(constraints.max_size.x.max(0.0));
        let min = self.min_lines.max(1);
        let max = self.max_lines.unwrap_or(usize::MAX).max(min);
        let want = self.editor.lines().len().clamp(min, max);
        let h_pt = want as f32 * LINE_PT + 2.0 * TEXT_PAD_Y + 2.0;
        let min_h = cx.pt(h_pt).min(constraints.max_size.y.max(0.0));
        Vec2::new(min_w, min_h)
    }

    fn min_render(&self) -> RenderMinimum {
        // Two rows of text plus padding — a smaller slot still works
        // (the content scrolls), but the audit flags the squeeze.
        RenderMinimum::new(Vec2::new(120.0, 2.0 * LINE_PT + 2.0 * TEXT_PAD_Y + 2.0))
            .with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.cached_bounds = bounds;
        self.scale = cx.scale;
        // Declare keyboard focusability on the arena node — standalone
        // areas need it for `ImeCommitted` delivery.
        if self.enabled {
            cx.hot.flags |= NodeFlags::FOCUSABLE;
        } else {
            cx.hot.flags.remove(NodeFlags::FOCUSABLE);
        }
    }

    fn clips_children(&self) -> bool {
        // Scrollable region — content is clipped to the face.
        true
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::MultilineTextInput);
        node.set_label(self.label.as_str());
        node.set_value(self.editor.text());
        node.add_action(accesskit::Action::Focus);
        node.add_action(accesskit::Action::ScrollUp);
        node.add_action(accesskit::Action::ScrollDown);
        node.add_action(accesskit::Action::ScrollLeft);
        node.add_action(accesskit::Action::ScrollRight);
        node.add_action(accesskit::Action::SetScrollOffset);
        if !self.read_only {
            node.add_action(accesskit::Action::SetValue);
        }
        if !self.enabled {
            node.set_disabled();
        }
        if self.read_only {
            node.set_read_only();
        }
        // Live caret/selection through the same `CaretTracker` seam
        // `Text` exposes — synced by `sync_caret_tracker` on every
        // caret move.
        if self.focused {
            if let Some(caret) = &self.caret {
                caret.apply_to_node(node);
            }
        }
    }

    fn focused(&self) -> bool {
        self.focused
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.enabled {
            return EventResponse::Ignored;
        }
        match cx.event {
            // Editing events belong to the focused area only — an
            // unfocused area must not claim keys when the event is
            // broadcast down the internal-child chain.
            WidgetEvent::KeyPressed { .. }
            | WidgetEvent::KeyReleased { .. }
            | WidgetEvent::ImePreedit { .. }
            | WidgetEvent::ImeCommitted { .. }
                if !self.focused =>
            {
                return EventResponse::Ignored;
            }
            _ => {}
        }
        match cx.event {
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                count,
            } => {
                // A claimed primary press is the focus signal for
                // internal children — siblings drop theirs through the
                // helper's `FocusLost` broadcast.
                self.focused = true;
                // Scrollbar strips take precedence over text hit-testing.
                if self.bar_press(cx.bounds, *position) {
                    return EventResponse::CapturePointer;
                }
                let hit = self.cursor_at(cx.bounds, *position);
                self.preferred_col = None;
                match count {
                    1 => {
                        if self.shift_held {
                            // Shift-click extends from the existing caret.
                            self.editor.extend_selection_to(hit);
                        } else {
                            self.editor.set_cursors(vec![hit]);
                        }
                        self.drag_granularity = SelectGranularity::Char;
                        self.drag_word = None;
                    }
                    2 => {
                        // Double-click selects the whole word segment
                        // under the pointer — the platform-standard
                        // gesture `count` exists for. Its span is kept
                        // for the word-drag below.
                        let (lo, hi) = self.editor.word_span_at(hit);
                        self.editor.select_word_at(hit);
                        self.drag_granularity = SelectGranularity::Word;
                        self.drag_word = Some((lo, hi));
                    }
                    _ => {
                        // Triple-click selects the whole line —
                        // `CodeEditor::select_line` includes the
                        // newline when one exists.
                        self.editor.select_line(hit.line);
                        self.drag_granularity = SelectGranularity::Line;
                        self.drag_word = None;
                        self.drag_line = hit.line;
                    }
                }
                self.dragging = true;
                self.sync_caret_tracker();
                // Focus is implicit for FOCUSABLE nodes on handled
                // presses; capture keeps drag-select tracking outside
                // the bounds.
                EventResponse::CapturePointer
            }
            WidgetEvent::PointerMoved { position } if self.dragging => {
                let hit = self.cursor_at(cx.bounds, *position);
                match self.drag_granularity {
                    SelectGranularity::Char => {
                        self.editor.extend_selection_to(hit);
                    }
                    SelectGranularity::Word => {
                        // Extend by whole words: the edge of the
                        // double-clicked word opposite the drag stays
                        // anchored, the moving edge snaps to the far
                        // boundary of the word under the pointer — the
                        // `EditorPanel` line-drag logic, verbatim.
                        let (wlo, whi) = self.editor.word_span_at(hit);
                        if let Some((ilo, ihi)) = self.drag_word {
                            if hit >= ilo {
                                self.editor.set_selection(ilo, whi);
                            } else {
                                self.editor.set_selection(ihi, wlo);
                            }
                        }
                    }
                    SelectGranularity::Line => {
                        // Whole-line drag from the triple-clicked
                        // anchor line — `select_line` covers the
                        // newline on non-final lines.
                        let lines = self.editor.lines();
                        let end_of = |l: usize| {
                            if l + 1 < lines.len() {
                                Cursor::new(l + 1, 0)
                            } else {
                                Cursor::new(l, lines[l].chars().count())
                            }
                        };
                        let (anchor, head) = if hit.line >= self.drag_line {
                            (Cursor::new(self.drag_line, 0), end_of(hit.line))
                        } else {
                            (Cursor::new(hit.line, 0), end_of(self.drag_line))
                        };
                        self.editor.set_selection(anchor, head);
                    }
                }
                self.sync_caret_tracker();
                EventResponse::RequestRepaint
            }
            WidgetEvent::PointerMoved { position } if self.bar_drag.is_some() => {
                self.bar_move(cx.bounds, *position)
            }
            WidgetEvent::PointerReleased { .. } if self.dragging => {
                self.dragging = false;
                self.drag_word = None;
                EventResponse::ReleasePointer
            }
            WidgetEvent::PointerReleased { .. } if self.bar_drag.is_some() => {
                self.bar_drag = None;
                EventResponse::ReleasePointer
            }
            WidgetEvent::Scroll { delta, .. } => {
                // Nested chaining: an unconsumed delta returns
                // `Ignored` so an ancestor scroll region can take it.
                let applied = self.scroll_by(cx.bounds, *delta);
                if applied.length_squared() > 0.0 {
                    EventResponse::RequestRepaint
                } else {
                    EventResponse::Ignored
                }
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
                self.word_mod_held = false;
                self.dragging = false;
                self.bar_drag = None;
                self.preedit = None;
                EventResponse::RequestRepaint
            }
            WidgetEvent::ImePreedit { text, cursor } => {
                self.preedit = if text.is_empty() {
                    None
                } else {
                    Some((text.clone(), *cursor))
                };
                EventResponse::RequestRepaint
            }
            WidgetEvent::ImeCommitted { text } if !self.read_only => {
                // Unlike `TextInput`, committed newlines are kept.
                self.preedit = None;
                self.insert_str(text);
                EventResponse::RequestRepaint
            }
            WidgetEvent::KeyPressed { key, .. } => self.key_pressed(key, cx.bounds),
            WidgetEvent::KeyReleased { key }
                if matches!(
                    key.as_str(),
                    "Control" | "Ctrl" | "Meta" | "Cmd" | "Super" | "Alt" | "Option"
                ) =>
            {
                self.word_mod_held = false;
                EventResponse::Handled
            }
            WidgetEvent::KeyReleased { key } if key == "Shift" => {
                self.shift_held = false;
                EventResponse::Handled
            }
            WidgetEvent::SemanticAction(action) => match action {
                // AT focus/activation requests honour the pending-focus
                // protocol: CaptureFocus records a request the app
                // drains into the FocusManager.
                SemanticAction::Focus | SemanticAction::Click => EventResponse::CaptureFocus,
                SemanticAction::SetValue(text) if !self.read_only => {
                    self.set_value(text.clone());
                    EventResponse::RequestRepaint
                }
                SemanticAction::ScrollUp
                | SemanticAction::ScrollDown
                | SemanticAction::ScrollLeft
                | SemanticAction::ScrollRight => {
                    let step = LINE_PT * self.scale;
                    let d = match action {
                        SemanticAction::ScrollUp => Vec2::new(0.0, -step),
                        SemanticAction::ScrollDown => Vec2::new(0.0, step),
                        SemanticAction::ScrollLeft => Vec2::new(-step, 0.0),
                        _ => Vec2::new(step, 0.0),
                    };
                    if self.scroll_by(cx.bounds, d).length_squared() > 0.0 {
                        EventResponse::RequestRepaint
                    } else {
                        EventResponse::Ignored
                    }
                }
                SemanticAction::SetScrollOffset(v) => {
                    let applied = self.scroll_by(cx.bounds, *v - self.scroll_offset());
                    if applied.length_squared() > 0.0 {
                        EventResponse::RequestRepaint
                    } else {
                        EventResponse::Handled
                    }
                }
                // The caret-following scroll in `paint` keeps the
                // caret visible; the repaint is the acknowledgement.
                SemanticAction::ScrollIntoView => EventResponse::RequestRepaint,
                _ => EventResponse::Ignored,
            },
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

        let font_px = cx.pt(FONT_PT);
        let char_w = cx.pt(7.0);
        let ambient = cx.text_painter;
        let lines = self.editor.lines();
        // `x_of` must not borrow `cx` — the paint loop below mutates
        // `cx.list` while the closure stays alive.
        let x_of = |t: &str, byte: usize| self.offset_for(ambient, char_w, t, byte, font_px);
        let lay = Self::geometry(lines, self.wrap, cx.scale, b, &x_of);

        // Caret-following scroll on both axes — the multiline version
        // of `TextInput`'s `scroll_x` logic. `paint` takes `&self`, so
        // the offsets live in atomics.
        let caret = self.primary();
        let (caret_row, caret_off) = Self::caret_row_x(&lay.rows, lines, caret, &x_of);
        let mut sy = self.scroll_y();
        let caret_top = caret_row as f32 * lay.line_h;
        if caret_top - sy < 0.0 {
            sy = caret_top;
        }
        if caret_top + lay.line_h - sy > lay.view.height() {
            sy = caret_top + lay.line_h - lay.view.height();
        }
        let sy = sy.clamp(0.0, (lay.content_h - lay.view.height()).max(0.0));
        self.set_scroll_y(sy);
        let mut sx = self.scroll_x();
        if self.wrap {
            sx = 0.0;
        } else {
            if caret_off - sx > lay.view.width() {
                sx = caret_off - lay.view.width();
            }
            if caret_off - sx < 0.0 {
                sx = caret_off;
            }
            sx = sx.clamp(0.0, (lay.content_w - lay.view.width()).max(0.0));
        }
        self.set_scroll_x(sx);

        // Everything inside the border is clipped to the face — rows,
        // selection bands, and the caret can never spill past the
        // field edge.
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

        let accent = cx.color(TokenKey::AccentColor, EDGE_FOCUSED);
        let sel_ink = [accent[0], accent[1], accent[2], 96];
        let sel = self.editor.selection();
        let empty = self.editor.text().is_empty();
        let content_x = lay.view.min_x() - sx;
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let ink = cx.color(TokenKey::TextColor, INK);

        let mut y = lay.view.min_y() - sy;
        for (i, row) in lay.rows.iter().enumerate() {
            let row_bottom = y + lay.line_h;
            if row_bottom < lay.view.min_y() {
                y = row_bottom;
                continue;
            }
            if y > lay.view.max_y() {
                break;
            }
            let line = &lines[row.line];
            let row_text = &line[row.start..row.end];

            // Selection band behind the text — the same shaped offsets
            // the caret uses, so the band aligns with the glyphs. A
            // covered wrap row fills to the viewport edge; a covered
            // line break adds the EOL sliver `EditorPanel` paints.
            if self.focused {
                if let Some((sa, sb)) = sel {
                    if row.line >= sa.line && row.line <= sb.line {
                        let lo_b = if row.line == sa.line {
                            col_byte(line, sa.column)
                        } else {
                            0
                        };
                        let hi_b = if row.line == sb.line {
                            col_byte(line, sb.column)
                        } else {
                            line.len()
                        };
                        let lo = lo_b.clamp(row.start, row.end);
                        let hi = hi_b.clamp(row.start, row.end).max(lo);
                        let last_row = row.end == line.len();
                        let newline = last_row && row.line < sb.line;
                        if hi > lo || (newline && hi == row.end) {
                            let x0 = f64::from(content_x + x_of(line, lo) - x_of(line, row.start));
                            let mut x1 =
                                f64::from(content_x + x_of(line, hi) - x_of(line, row.start));
                            if !last_row && hi == row.end {
                                // Mid-wrap covered row — the band fills
                                // to the wrap edge.
                                x1 = f64::from(lay.view.max_x());
                            } else if newline {
                                x1 += cx.ptf(f64::from(EOL_SLIVER));
                            }
                            cx.list.push_fill_rect(
                                kurbo::Rect::new(x0, f64::from(y), x1, f64::from(row_bottom)),
                                sel_ink,
                            );
                        }
                    }
                }
            }

            if empty && i == 0 {
                crate::text_paint::paint_label(
                    painter,
                    cx.list,
                    kurbo::Point::new(f64::from(content_x), f64::from(y)),
                    &self.placeholder,
                    font_px,
                    cx.color(TokenKey::TextMutedColor, INK_PLACEHOLDER),
                );
            } else if !row_text.is_empty() {
                crate::text_paint::paint_label(
                    painter,
                    cx.list,
                    kurbo::Point::new(f64::from(content_x), f64::from(y)),
                    row_text,
                    font_px,
                    ink,
                );
            }

            // Caret at the shaped boundary for the primary cursor —
            // measured through the same shape pass as the painted
            // glyphs, so it cannot drift on mixed-width text. While an
            // IME composition is in flight the preedit string paints
            // underlined at the caret and the caret trails its end.
            if self.focused && i == caret_row {
                let cx_x = f64::from(content_x + caret_off);
                let mut pre_w = 0.0_f32;
                if let Some((pre, _)) = &self.preedit {
                    crate::text_paint::paint_label(
                        painter,
                        cx.list,
                        kurbo::Point::new(cx_x, f64::from(y)),
                        pre,
                        font_px,
                        ink,
                    );
                    pre_w = painter
                        .and_then(|p| p.measure_text(pre, font_px))
                        .unwrap_or_else(|| pre.chars().count() as f32 * char_w);
                    let uy = f64::from(row_bottom) - cx.ptf(2.0);
                    let mut u = kurbo::BezPath::new();
                    u.move_to((cx_x, uy));
                    u.line_to((cx_x + f64::from(pre_w), uy));
                    cx.list.push_stroke_path(u, cx.pt(1.0), ink);
                }
                let mut path = kurbo::BezPath::new();
                path.move_to((cx_x + f64::from(pre_w), f64::from(y) + cx.ptf(2.0)));
                path.line_to((cx_x + f64::from(pre_w), f64::from(row_bottom) - cx.ptf(2.0)));
                cx.list
                    .push_stroke_path(path, cx.pt(1.0), cx.color(TokenKey::TextColor, CARET));
            }
            y = row_bottom;
        }
        cx.list.pop_clip();

        // Smart scrollbars — present only when content overflows on
        // that axis, the `ScrollView` convention.
        let min_thumb = cx.pt(MIN_THUMB);
        for (track, content_len, view_len, scroll, vertical, grabbed) in [
            (lay.vbar, lay.content_h, lay.view.height(), sy, true, true),
            (lay.hbar, lay.content_w, lay.view.width(), sx, false, false),
        ] {
            let Some(track) = track else { continue };
            let tr = kurbo::Rect::new(
                f64::from(track.min_x()),
                f64::from(track.min_y()),
                f64::from(track.max_x()),
                f64::from(track.max_y()),
            );
            cx.list
                .push_fill_rect(tr, cx.color(TokenKey::DividerColor, TRACK_COLOR));
            let thumb = Self::thumb_rect(track, view_len, content_len, scroll, min_thumb, vertical);
            let t = kurbo::Rect::new(
                f64::from(thumb.min_x()),
                f64::from(thumb.min_y()),
                f64::from(thumb.max_x()),
                f64::from(thumb.max_y()),
            );
            let active = matches!(self.bar_drag, Some((v, _)) if v == grabbed);
            cx.list.push_fill_shape(
                t,
                &Shape::PILL,
                if active {
                    cx.color(TokenKey::TextMutedColor, THUMB_ACTIVE)
                } else {
                    cx.color(TokenKey::BorderColor, THUMB_COLOR)
                },
            );
        }
    }
}

impl std::fmt::Debug for TextArea {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextArea")
            .field("label", &self.label)
            .field("value", &self.editor.text())
            .field("enabled", &self.enabled)
            .field("read_only", &self.read_only)
            .field("wrap", &self.wrap)
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

    const BOUNDS: Rect = Rect {
        origin: Vec2::ZERO,
        size: Vec2 { x: 200.0, y: 100.0 },
    };

    fn ev<'a>(event: &'a WidgetEvent) -> EventContext<'a> {
        EventContext {
            event,
            bounds: BOUNDS,
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

    /// The release half of a key event — modifier tracking needs it:
    /// `word_mod_held`/`shift_held` are armed by `KeyPressed` and
    /// disarmed by `KeyReleased`.
    fn key_rel(name: &str) -> WidgetEvent {
        WidgetEvent::KeyReleased {
            key: name.to_string(),
        }
    }

    /// Editing events only reach a focused area — tests simulate the
    /// arena's focus delivery the way production does.
    fn focus(area: &mut TextArea) {
        area.event(&mut ev(&WidgetEvent::FocusGained));
    }

    #[test]
    fn text_area_new() {
        let area = TextArea::new();
        assert_eq!(area.label, "");
        assert!(area.value().is_empty());
        assert!(area.enabled);
        assert!(!area.read_only);
        assert!(area.wrap);
        assert_eq!(area.min_lines, 3);
        assert_eq!(area.max_lines, None);
    }

    #[test]
    fn text_area_builder_methods() {
        let area = TextArea::new()
            .label("Notes")
            .with_value("a\nb")
            .placeholder("Write…")
            .enabled(false)
            .read_only(true)
            .wrap(false)
            .min_lines(2)
            .max_lines(9);
        assert_eq!(area.label, "Notes");
        assert_eq!(area.value(), "a\nb");
        assert_eq!(area.placeholder, "Write…");
        assert!(!area.enabled);
        assert!(area.read_only);
        assert!(!area.wrap);
        assert_eq!(area.min_lines, 2);
        assert_eq!(area.max_lines, Some(9));
    }

    #[test]
    fn text_area_set_value_collapses_caret_to_end() {
        let mut area = TextArea::new().with_value("one\ntwo");
        focus(&mut area);
        area.event(&mut ev(&key("Home")));
        area.set_value("xy");
        assert_eq!(area.value(), "xy");
        assert_eq!(area.cursor(), Cursor::new(0, 2));
    }

    #[test]
    fn text_area_take_edited() {
        let mut area = TextArea::new();
        focus(&mut area);
        assert_eq!(area.take_edited(), None);
        area.event(&mut ev(&ime("a")));
        assert_eq!(area.take_edited(), Some(()));
        assert_eq!(area.take_edited(), None);
        // Caret motion alone is not an edit.
        area.event(&mut ev(&key("ArrowLeft")));
        assert_eq!(area.take_edited(), None);
    }

    #[test]
    fn text_area_measure_honours_line_hints() {
        let mut hot = HotNode::default();
        let mut cx = make_cx(&mut hot);
        let constraints = LayoutConstraints {
            min_size: Vec2::ZERO,
            max_size: Vec2::new(400.0, 400.0),
        };
        let mut area = TextArea::new().min_lines(3);
        focus(&mut area);
        let size = area.measure(&mut cx, constraints);
        assert!((size.y - (3.0 * LINE_PT + 2.0 * TEXT_PAD_Y + 2.0)).abs() < 0.01);
        // max_lines caps the desired height.
        let mut capped = TextArea::new()
            .min_lines(1)
            .max_lines(2)
            .with_value("a\nb\nc\nd\ne");
        let size = capped.measure(&mut cx, constraints);
        assert!((size.y - (2.0 * LINE_PT + 2.0 * TEXT_PAD_Y + 2.0)).abs() < 0.01);
    }

    #[test]
    fn text_area_layout_sets_bounds_and_focusable() {
        let mut hot = HotNode::default();
        let mut area = TextArea::new();
        focus(&mut area);
        {
            let mut cx = make_cx(&mut hot);
            area.layout(&mut cx, BOUNDS);
        }
        assert_eq!(area.cached_bounds(), BOUNDS);
        assert!(hot.flags.contains(NodeFlags::FOCUSABLE));
        let mut off = TextArea::new().enabled(false);
        {
            let mut cx = make_cx(&mut hot);
            off.layout(&mut cx, BOUNDS);
        }
        assert!(!hot.flags.contains(NodeFlags::FOCUSABLE));
    }

    #[test]
    fn text_area_accessibility_sets_role_label_value() {
        let area = TextArea::new().label("Notes").with_value("a\nb");
        let mut node = accesskit::Node::new(accesskit::Role::Unknown);
        area.accessibility(&mut node);
        assert_eq!(node.role(), accesskit::Role::MultilineTextInput);
        assert_eq!(node.label(), Some("Notes"));
        assert_eq!(node.value(), Some("a\nb"));
        assert!(node.supports_action(accesskit::Action::Focus));
        assert!(node.supports_action(accesskit::Action::SetValue));
        assert!(node.supports_action(accesskit::Action::ScrollDown));
    }

    #[test]
    fn text_area_accessibility_read_only_and_disabled() {
        let area = TextArea::new().read_only(true);
        let mut node = accesskit::Node::new(accesskit::Role::Unknown);
        area.accessibility(&mut node);
        assert!(!node.supports_action(accesskit::Action::SetValue));
        assert!(node.is_read_only());
        let area = TextArea::new().enabled(false);
        let mut node = accesskit::Node::new(accesskit::Role::Unknown);
        area.accessibility(&mut node);
        assert!(node.is_disabled());
    }

    #[test]
    fn text_area_clone_and_debug() {
        let area = TextArea::new().label("L").with_value("hello");
        let cloned = area.clone();
        assert_eq!(cloned.label, "L");
        assert_eq!(cloned.value(), "hello");
        let debug = format!("{:?}", area);
        assert!(debug.contains("TextArea"));
        assert!(debug.contains("hello"));
    }

    #[test]
    fn text_area_ime_inserts_and_keeps_newlines() {
        let mut area = TextArea::new();
        focus(&mut area);
        area.event(&mut ev(&ime("a\nb")));
        assert_eq!(area.value(), "a\nb");
        assert_eq!(area.cursor(), Cursor::new(1, 1));
        area.event(&mut ev(&key("ArrowLeft")));
        area.event(&mut ev(&ime("X")));
        assert_eq!(area.value(), "a\nXb");
    }

    #[test]
    fn text_area_ime_strips_control_chars_except_tab() {
        let mut area = TextArea::new();
        focus(&mut area);
        area.event(&mut ev(&ime("a\u{7}b\tc")));
        assert_eq!(area.value(), "ab\tc");
    }

    #[test]
    fn text_area_ime_preedit_lifecycle() {
        let mut area = TextArea::new();
        focus(&mut area);
        let pre = |text: &str, cursor: Option<(usize, usize)>| WidgetEvent::ImePreedit {
            text: text.to_string(),
            cursor,
        };
        area.event(&mut ev(&pre("wip", Some((0, 3)))));
        assert_eq!(area.preedit(), Some("wip"));
        assert_eq!(area.preedit_cursor(), Some((0, 3)));
        // Commit inserts the text and clears the composition.
        area.event(&mut ev(&ime("done")));
        assert_eq!(area.preedit(), None);
        assert_eq!(area.value(), "done");
        // An empty preedit clears without touching the value, and
        // focus loss clears any in-flight composition.
        area.event(&mut ev(&pre("x", None)));
        area.event(&mut ev(&pre("", None)));
        assert_eq!(area.preedit(), None);
        area.event(&mut ev(&pre("y", None)));
        area.event(&mut ev(&WidgetEvent::FocusLost));
        assert_eq!(area.preedit(), None);
        assert_eq!(area.value(), "done");
    }

    #[test]
    fn text_area_enter_inserts_newline() {
        let mut area = TextArea::new().with_value("ab");
        focus(&mut area);
        area.event(&mut ev(&key("ArrowLeft")));
        area.event(&mut ev(&key("Enter")));
        assert_eq!(area.value(), "a\nb");
        assert_eq!(area.cursor(), Cursor::new(1, 0));
    }

    #[test]
    fn text_area_tab_inserts_tab() {
        let mut area = TextArea::new().with_value("ab");
        focus(&mut area);
        area.event(&mut ev(&key("Tab")));
        assert_eq!(area.value(), "ab\t");
    }

    #[test]
    fn text_area_backspace_and_delete_cross_lines() {
        let mut area = TextArea::new().with_value("ab\ncd");
        focus(&mut area);
        // Caret starts at (1, 2) after set_value — Home then Delete
        // forward joins? No: Backspace at line start joins upward.
        area.event(&mut ev(&key("Home")));
        assert_eq!(area.cursor(), Cursor::new(1, 0));
        area.event(&mut ev(&key("Backspace")));
        assert_eq!(area.value(), "abcd");
        assert_eq!(area.cursor(), Cursor::new(0, 2));
        // Forward delete at EOL joins the next line.
        area.event(&mut ev(&key("End")));
        area.event(&mut ev(&ime("\nx")));
        // value is now "abcd\nx" — caret at (1,1)
        area.event(&mut ev(&key("ArrowLeft")));
        area.event(&mut ev(&key("ArrowLeft")));
        // caret (0,4) — EOL; Delete joins.
        area.event(&mut ev(&key("Delete")));
        assert_eq!(area.value(), "abcdx");
    }

    #[test]
    fn text_area_backspace_deletes_whole_grapheme() {
        let mut area = TextArea::new().with_value("e\u{301}x");
        focus(&mut area);
        area.event(&mut ev(&key("ArrowLeft")));
        area.event(&mut ev(&key("Backspace")));
        assert_eq!(area.value(), "x");
        assert_eq!(area.cursor(), Cursor::new(0, 0));
        // ZWJ family emoji — one cluster, removed atomically.
        let mut area = TextArea::new().with_value("\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}!");
        focus(&mut area);
        area.event(&mut ev(&key("ArrowLeft")));
        area.event(&mut ev(&key("Backspace")));
        assert_eq!(area.value(), "!");
    }

    #[test]
    fn text_area_delete_forward_removes_whole_grapheme() {
        let mut area = TextArea::new().with_value("e\u{301}x");
        focus(&mut area);
        area.event(&mut ev(&key("Home")));
        area.event(&mut ev(&key("Delete")));
        assert_eq!(area.value(), "x");
    }

    #[test]
    fn text_area_arrows_step_graphemes() {
        let mut area = TextArea::new().with_value("e\u{301}x");
        focus(&mut area);
        area.event(&mut ev(&key("Home")));
        area.event(&mut ev(&key("ArrowRight")));
        assert_eq!(area.cursor(), Cursor::new(0, 2));
        area.event(&mut ev(&key("ArrowRight")));
        assert_eq!(area.cursor(), Cursor::new(0, 3));
        area.event(&mut ev(&key("ArrowLeft")));
        assert_eq!(area.cursor(), Cursor::new(0, 2));
    }

    #[test]
    fn text_area_arrows_move_and_wrap_lines() {
        let mut area = TextArea::new().with_value("ab\ncd");
        focus(&mut area);
        // set_value leaves caret at end (1,2).
        area.event(&mut ev(&key("ArrowRight")));
        assert_eq!(area.cursor(), Cursor::new(1, 2)); // clamped at doc end
        area.event(&mut ev(&key("ArrowLeft")));
        assert_eq!(area.cursor(), Cursor::new(1, 1));
        area.event(&mut ev(&key("ArrowLeft")));
        assert_eq!(area.cursor(), Cursor::new(1, 0));
        area.event(&mut ev(&key("ArrowLeft"))); // crosses the line break
        assert_eq!(area.cursor(), Cursor::new(0, 2));
        area.event(&mut ev(&key("ArrowRight"))); // wraps forward
        assert_eq!(area.cursor(), Cursor::new(1, 0));
        area.event(&mut ev(&key("ArrowUp")));
        assert_eq!(area.cursor(), Cursor::new(0, 0));
    }

    #[test]
    fn text_area_vertical_moves_keep_sticky_column() {
        let mut area = TextArea::new().with_value("abcdef\nx\nabc");
        focus(&mut area);
        // `set_value` parks the caret at the document end — walk to
        // column 5 of line 0 first (Home is a line-home, not a doc-home).
        area.event(&mut ev(&key("Home")));
        area.event(&mut ev(&key("ArrowUp")));
        area.event(&mut ev(&key("ArrowUp")));
        assert_eq!(area.cursor(), Cursor::new(0, 0));
        for _ in 0..5 {
            area.event(&mut ev(&key("ArrowRight")));
        }
        area.event(&mut ev(&key("ArrowDown")));
        // Line 1 has 1 char — clamped, but the column is remembered.
        assert_eq!(area.cursor(), Cursor::new(1, 1));
        area.event(&mut ev(&key("ArrowDown")));
        assert_eq!(area.cursor(), Cursor::new(2, 3)); // 5 > len("abc")
        area.event(&mut ev(&key("ArrowUp")));
        assert_eq!(area.cursor(), Cursor::new(1, 1));
        // A horizontal move drops the sticky column.
        area.event(&mut ev(&key("ArrowLeft")));
        area.event(&mut ev(&key("ArrowUp")));
        assert_eq!(area.cursor(), Cursor::new(0, 0));
    }

    #[test]
    fn text_area_page_up_down() {
        let mut area = TextArea::new().with_value(&("l\n".repeat(29) + "l"));
        focus(&mut area);
        // Caret at end (line 29); PageUp jumps by viewport rows
        // (view ≈ 88px / 17.5px ≈ 5 rows).
        area.event(&mut ev(&key("PageUp")));
        let first = area.cursor().line;
        assert!(first < 29 && first > 20);
        area.event(&mut ev(&key("PageDown")));
        assert_eq!(area.cursor().line, 29);
    }

    #[test]
    fn text_area_home_end() {
        let mut area = TextArea::new().with_value("ab\ncd");
        focus(&mut area);
        area.event(&mut ev(&key("Home")));
        assert_eq!(area.cursor(), Cursor::new(1, 0));
        area.event(&mut ev(&key("End")));
        assert_eq!(area.cursor(), Cursor::new(1, 2));
    }

    #[test]
    fn text_area_select_all_then_type_replaces() {
        let mut area = TextArea::new().with_value("a\nb");
        focus(&mut area);
        area.event(&mut ev(&key("SelectAll")));
        assert_eq!(
            area.editor.selection(),
            Some((Cursor::new(0, 0), Cursor::new(1, 1)))
        );
        area.event(&mut ev(&ime("z")));
        assert_eq!(area.value(), "z");
        assert_eq!(area.editor.selection(), None);
    }

    #[test]
    fn text_area_shift_arrows_extend_selection() {
        let mut area = TextArea::new().with_value("ab\ncd");
        focus(&mut area);
        area.event(&mut ev(&key("Home")));
        area.event(&mut ev(&key("Shift")));
        area.event(&mut ev(&key("ArrowUp")));
        assert_eq!(
            area.editor.selection(),
            Some((Cursor::new(0, 0), Cursor::new(1, 0)))
        );
        // Typing replaces the selection.
        area.event(&mut ev(&key("Shift")));
        area.event(&mut ev(&ime("X")));
        assert_eq!(area.value(), "Xcd");
    }

    #[test]
    fn text_area_arrows_collapse_selection() {
        let mut area = TextArea::new().with_value("ab");
        focus(&mut area);
        area.event(&mut ev(&key("SelectAll")));
        area.event(&mut ev(&key("ArrowLeft")));
        assert_eq!(area.cursor(), Cursor::new(0, 0));
        area.event(&mut ev(&key("SelectAll")));
        area.event(&mut ev(&key("ArrowRight")));
        assert_eq!(area.cursor(), Cursor::new(0, 2));
        area.event(&mut ev(&key("SelectAll")));
        area.event(&mut ev(&key("ArrowDown")));
        assert_eq!(area.cursor(), Cursor::new(0, 2));
        assert_eq!(area.editor.selection(), None);
    }

    #[test]
    fn text_area_escape_collapses_selection() {
        let mut area = TextArea::new().with_value("ab");
        focus(&mut area);
        area.event(&mut ev(&key("SelectAll")));
        area.event(&mut ev(&key("Escape")));
        assert_eq!(area.editor.selection(), None);
        assert_eq!(area.cursor(), Cursor::new(0, 2));
    }

    #[test]
    fn text_area_backspace_deletes_selection() {
        let mut area = TextArea::new().with_value("a\nb");
        focus(&mut area);
        area.event(&mut ev(&key("SelectAll")));
        area.event(&mut ev(&key("Backspace")));
        assert_eq!(area.value(), "");
        assert_eq!(area.cursor(), Cursor::new(0, 0));
        // The SelectAll+Backspace edit flagged the change — drain it,
        // then a no-op backspace on the empty buffer must not flag.
        assert_eq!(area.take_edited(), Some(()));
        area.event(&mut ev(&key("Backspace")));
        assert_eq!(area.take_edited(), None);
    }

    #[test]
    fn text_area_read_only_blocks_edits() {
        let mut area = TextArea::new().with_value("a\nb").read_only(true);
        focus(&mut area);
        area.event(&mut ev(&ime("X")));
        area.event(&mut ev(&key("Backspace")));
        area.event(&mut ev(&key("Delete")));
        area.event(&mut ev(&key("Enter")));
        area.event(&mut ev(&key("Paste")));
        area.event(&mut ev(&key("Cut")));
        assert_eq!(area.value(), "a\nb");
        // …but selection and caret movement still work.
        area.event(&mut ev(&key("SelectAll")));
        assert!(area.editor.selection().is_some());
    }

    /// Window-space position of a caret boundary — through the same
    /// shaped painter `cursor_at` hit-tests with, so test presses land
    /// inside the intended word.
    fn pos_of(area: &mut TextArea, line: usize, col: usize) -> Vec2 {
        let painter = area
            .text_painter
            .get_or_insert_with(crate::text_paint::shared_painter);
        let x_of = |t: &str, b: usize| painter.caret_x(t, FONT_PT, b);
        let lay = TextArea::geometry(area.editor.lines(), area.wrap, 1.0, BOUNDS, &x_of);
        let (row_i, rx) = TextArea::caret_row_x(
            &lay.rows,
            area.editor.lines(),
            Cursor::new(line, col),
            &x_of,
        );
        Vec2::new(
            lay.view.min_x() + rx + 0.5,
            lay.view.min_y() + row_i as f32 * lay.line_h + 1.0,
        )
    }

    fn press(area: &mut TextArea, pos: Vec2, count: u8) -> EventResponse {
        area.event(&mut ev(&WidgetEvent::PointerPressed {
            position: pos,
            button: PointerButton::Primary,
            count,
        }))
    }

    fn drag_to(area: &mut TextArea, pos: Vec2) {
        area.event(&mut ev(&WidgetEvent::PointerMoved { position: pos }));
    }

    #[test]
    fn text_area_click_places_caret() {
        let mut area = TextArea::new().with_value("alpha\nbeta");
        focus(&mut area);
        let p = pos_of(&mut area, 1, 3);
        assert_eq!(press(&mut area, p, 1), EventResponse::CapturePointer);
        assert_eq!(area.cursor(), Cursor::new(1, 3));
    }

    #[test]
    fn text_area_double_click_selects_word() {
        let mut area = TextArea::new().with_value("alpha beta\ngamma");
        focus(&mut area);
        let p = pos_of(&mut area, 0, 8); // inside "beta"
        press(&mut area, p, 2);
        assert_eq!(area.editor.selected_text().as_deref(), Some("beta"));
    }

    #[test]
    fn text_area_triple_click_selects_line_with_newline() {
        let mut area = TextArea::new().with_value("one\ntwo\nthree");
        focus(&mut area);
        let p = pos_of(&mut area, 1, 1);
        press(&mut area, p, 3);
        // Non-final line selection covers its newline — same as
        // `CodeEditor::select_line`.
        assert_eq!(area.editor.selected_text().as_deref(), Some("two\n"));
    }

    #[test]
    fn text_area_double_click_drag_extends_by_word() {
        let mut area = TextArea::new().with_value("alpha beta\ngamma delta");
        focus(&mut area);
        let p = pos_of(&mut area, 0, 8); // inside "beta"
        press(&mut area, p, 2);
        // Drag into "delta" on the next line — the initial word stays
        // covered and the moving edge snaps to word boundaries.
        let end = pos_of(&mut area, 1, 8); // inside "delta"
        drag_to(&mut area, end);
        assert_eq!(
            area.editor.selected_text().as_deref(),
            Some("beta\ngamma delta")
        );
        area.event(&mut ev(&WidgetEvent::PointerReleased {
            position: end,
            button: PointerButton::Primary,
        }));
        assert!(!area.dragging);
    }

    #[test]
    fn text_area_triple_click_drag_extends_by_line() {
        let mut area = TextArea::new().with_value("one\ntwo\nthree\nfour");
        focus(&mut area);
        let p = pos_of(&mut area, 1, 1);
        press(&mut area, p, 3);
        let down = pos_of(&mut area, 2, 1);
        drag_to(&mut area, down);
        assert_eq!(area.editor.selected_text().as_deref(), Some("two\nthree\n"));
        // Dragging back up covers whole lines the other way.
        let up = pos_of(&mut area, 0, 1);
        drag_to(&mut area, up);
        assert_eq!(area.editor.selected_text().as_deref(), Some("one\ntwo\n"));
    }

    #[test]
    fn text_area_single_click_drag_selects_chars() {
        let mut area = TextArea::new().with_value("alpha\nbeta");
        focus(&mut area);
        let p = pos_of(&mut area, 0, 2);
        press(&mut area, p, 1);
        let end = pos_of(&mut area, 1, 3);
        drag_to(&mut area, end);
        assert_eq!(area.editor.selected_text().as_deref(), Some("pha\nbet"));
    }

    #[test]
    fn text_area_shift_click_extends() {
        let mut area = TextArea::new().with_value("alpha\nbeta");
        focus(&mut area);
        let p = pos_of(&mut area, 0, 2);
        press(&mut area, p, 1);
        area.event(&mut ev(&WidgetEvent::PointerReleased {
            position: p,
            button: PointerButton::Primary,
        }));
        area.event(&mut ev(&key("Shift")));
        let end = pos_of(&mut area, 1, 2);
        press(&mut area, end, 1);
        assert_eq!(area.editor.selected_text().as_deref(), Some("pha\nbe"));
    }

    #[test]
    fn text_area_wrap_breaks_long_lines() {
        let area = TextArea::new()
            .with_value("one two three four five six seven eight nine ten eleven twelve");
        let painter = crate::text_paint::shared_painter();
        let x_of = |t: &str, b: usize| painter.caret_x(t, FONT_PT, b);
        let lay = TextArea::geometry(area.editor.lines(), true, 1.0, BOUNDS, &x_of);
        // The sentence can't fit 176pt — it must break into ≥2 rows on
        // word boundaries (no mid-word split).
        assert!(lay.rows.len() >= 2);
        let line = &area.editor.lines()[0];
        for r in &lay.rows[..lay.rows.len() - 1] {
            assert_eq!(r.line, 0);
            // A break is only legal at a word boundary — never mid-word
            // (alphanumeric on both sides of the cut). Trailing
            // whitespace packs into the row, so `r.end` may sit just
            // after a space rather than before one.
            let before = line[..r.end].chars().next_back();
            let after = line[r.end..].chars().next();
            assert!(
                !(before.is_some_and(|c| c.is_alphanumeric())
                    && after.is_some_and(|c| c.is_alphanumeric())),
                "wrap split a word at byte {}",
                r.end
            );
        }
    }

    #[test]
    fn text_area_no_wrap_keeps_one_row_per_line() {
        let area = TextArea::new()
            .wrap(false)
            .with_value("a very long line that will not be wrapped at all ever");
        let lay = TextArea::geometry(area.editor.lines(), false, 1.0, BOUNDS, &|_t, _b| 0.0);
        assert_eq!(lay.rows.len(), 1);
        assert_eq!(lay.rows[0].end, area.editor.lines()[0].len());
    }

    #[test]
    fn text_area_scroll_consumes_within_bounds() {
        let mut area = TextArea::new().with_value(
            (0..30)
                .map(|i| format!("line {i}"))
                .collect::<Vec<_>>()
                .join("\n"),
        );
        focus(&mut area);
        let scroll = |d: f32| WidgetEvent::Scroll {
            position: Vec2::new(10.0, 10.0),
            delta: Vec2::new(0.0, d),
        };
        assert_eq!(
            area.event(&mut ev(&scroll(20.0))),
            EventResponse::RequestRepaint
        );
        assert!((area.scroll_offset().y - 20.0).abs() < 0.01);
        // At the clamp the remaining delta is refused — the nested
        // chaining contract.
        let max = 30.0 * LINE_PT - 88.0; // content 525 − view 88
        assert_eq!(
            area.event(&mut ev(&scroll(10_000.0))),
            EventResponse::RequestRepaint
        );
        assert!((area.scroll_offset().y - max).abs() < 0.5);
        assert_eq!(area.event(&mut ev(&scroll(10.0))), EventResponse::Ignored);
    }

    #[test]
    fn text_area_horizontal_scroll_only_when_unwrapped() {
        let mut area = TextArea::new().with_value("short\nalso short");
        focus(&mut area);
        let scroll = |d: Vec2| WidgetEvent::Scroll {
            position: Vec2::new(10.0, 10.0),
            delta: d,
        };
        // Wrapped: horizontal deltas are refused.
        assert_eq!(
            area.event(&mut ev(&scroll(Vec2::new(30.0, 0.0)))),
            EventResponse::Ignored
        );
        area.wrap = false;
        let mut wide = TextArea::new().wrap(false).with_value("x".repeat(400));
        assert_eq!(
            wide.event(&mut ev(&scroll(Vec2::new(40.0, 0.0)))),
            EventResponse::RequestRepaint
        );
        assert!(wide.scroll_offset().x > 0.0);
    }

    #[test]
    fn text_area_scrollbar_thumb_drag() {
        let mut area = TextArea::new().with_value(
            (0..30)
                .map(|i| format!("line {i}"))
                .collect::<Vec<_>>()
                .join("\n"),
        );
        focus(&mut area);
        // The vbar strip hugs the right padding edge.
        let track_x = 200.0 - TEXT_PAD_X - 1.0;
        let p = Vec2::new(track_x, 15.0);
        assert_eq!(press(&mut area, p, 1), EventResponse::CapturePointer);
        assert!(area.bar_drag.is_some() || area.scroll_offset().y > 0.0);
        // Drag the thumb down — scroll must follow (or the press
        // already paged).
        drag_to(&mut area, Vec2::new(track_x, 80.0));
        assert!(area.scroll_offset().y > 0.0);
        area.event(&mut ev(&WidgetEvent::PointerReleased {
            position: Vec2::new(track_x, 80.0),
            button: PointerButton::Primary,
        }));
        assert!(area.bar_drag.is_none());
    }

    #[test]
    fn text_area_disabled_ignores_events() {
        let mut area = TextArea::new().with_value("ab").enabled(false);
        focus(&mut area);
        assert_eq!(area.event(&mut ev(&ime("x"))), EventResponse::Ignored);
        assert_eq!(
            area.event(&mut ev(&key("SelectAll"))),
            EventResponse::Ignored
        );
        assert_eq!(area.value(), "ab");
    }

    #[test]
    fn text_area_semantic_actions() {
        let mut area = TextArea::new();
        focus(&mut area);
        let focus = WidgetEvent::SemanticAction(SemanticAction::Focus);
        assert_eq!(area.event(&mut ev(&focus)), EventResponse::CaptureFocus);
        let set = WidgetEvent::SemanticAction(SemanticAction::SetValue("hi".into()));
        assert_eq!(area.event(&mut ev(&set)), EventResponse::RequestRepaint);
        assert_eq!(area.value(), "hi");
    }

    #[test]
    fn text_area_ctrl_a_selects_all() {
        let mut area = TextArea::new().with_value("one\ntwo");
        focus(&mut area);
        area.event(&mut ev(&key("Ctrl+a")));
        // Select-all covers the whole document — typing replaces it.
        area.event(&mut ev(&ime("z")));
        assert_eq!(area.value(), "z");
    }

    #[test]
    fn text_area_ctrl_arrows_word_nav() {
        let mut area = TextArea::new().with_value("foo bar\nbaz qux");
        focus(&mut area);
        // Word edges are word starts AND ends (the `TextInput`
        // convention) — "baz qux" walks 4 → 3 → 0, then crosses the
        // line to "bar"'s end at (0,7) and start at (0,4).
        area.event(&mut ev(&key("Ctrl+ArrowLeft")));
        let c = area.cursor();
        assert_eq!(c, Cursor::new(1, 4), "word-left landed at {c:?}");
        area.event(&mut ev(&key("Ctrl+ArrowLeft")));
        assert_eq!(area.cursor(), Cursor::new(1, 3));
        area.event(&mut ev(&key("Ctrl+ArrowLeft")));
        assert_eq!(area.cursor(), Cursor::new(1, 0));
        area.event(&mut ev(&key("Ctrl+ArrowLeft")));
        assert_eq!(area.cursor(), Cursor::new(0, 7));
        area.event(&mut ev(&key("Ctrl+ArrowRight")));
        assert_eq!(area.cursor(), Cursor::new(1, 0));
    }

    #[test]
    fn text_area_ctrl_backspace_word_delete() {
        let mut area = TextArea::new().with_value("alpha beta");
        focus(&mut area);
        // Deletes back to the previous word edge — "beta" goes,
        // then the space, then "alpha".
        area.event(&mut ev(&key("Ctrl+Backspace")));
        assert_eq!(area.value(), "alpha ");
        area.event(&mut ev(&key("Ctrl+Backspace")));
        assert_eq!(area.value(), "alpha");
        area.event(&mut ev(&key("Ctrl+Backspace")));
        assert_eq!(area.value(), "");
    }

    #[test]
    fn text_area_ctrl_delete_word_forward() {
        let mut area = TextArea::new().with_value("alpha beta");
        focus(&mut area);
        area.event(&mut ev(&key("Home")));
        area.event(&mut ev(&key("Ctrl+Delete")));
        assert_eq!(area.value(), " beta");
    }

    #[test]
    fn text_area_undo_redo_roundtrip() {
        let mut area = TextArea::new();
        focus(&mut area);
        area.event(&mut ev(&ime("hello")));
        area.event(&mut ev(&ime(" world")));
        assert_eq!(area.value(), "hello world");
        area.event(&mut ev(&key("Ctrl+z")));
        assert_eq!(area.value(), "hello");
        area.event(&mut ev(&key("Ctrl+z")));
        assert_eq!(area.value(), "");
        area.event(&mut ev(&key("Ctrl+Shift+z")));
        assert_eq!(area.value(), "hello");
        area.event(&mut ev(&key("Ctrl+y")));
        assert_eq!(area.value(), "hello world");
    }

    #[test]
    fn text_area_edit_invalidates_redo() {
        let mut area = TextArea::new();
        focus(&mut area);
        area.event(&mut ev(&ime("a")));
        area.event(&mut ev(&key("Ctrl+z")));
        assert_eq!(area.value(), "");
        // A new edit clears the redo stack — redoing "a" would
        // resurrect text the user deleted.
        area.event(&mut ev(&ime("b")));
        area.event(&mut ev(&key("Ctrl+y")));
        assert_eq!(area.value(), "b");
    }

    #[test]
    fn text_area_set_value_clears_history() {
        let mut area = TextArea::new();
        focus(&mut area);
        area.event(&mut ev(&ime("typed")));
        // Programmatic replacement resets the edit session — undo
        // must not resurrect the old buffer.
        area.set_value("fresh");
        area.event(&mut ev(&key("Ctrl+z")));
        assert_eq!(area.value(), "fresh");
    }

    #[test]
    fn text_area_unfocused_ignores_editing_keys() {
        let mut area = TextArea::new().with_value("abc");
        // No FocusGained — keys and IME are gated on focus.
        assert_eq!(
            area.event(&mut ev(&key("Backspace"))),
            EventResponse::Ignored
        );
        assert_eq!(area.event(&mut ev(&ime("x"))), EventResponse::Ignored);
        assert_eq!(area.value(), "abc");
    }

    #[test]
    fn text_area_focus_lost_stops_editing() {
        let mut area = TextArea::new().with_value("abc");
        focus(&mut area);
        area.event(&mut ev(&ime("d")));
        assert_eq!(area.value(), "abcd");
        area.event(&mut ev(&WidgetEvent::FocusLost));
        assert_eq!(area.event(&mut ev(&ime("e"))), EventResponse::Ignored);
        assert_eq!(area.value(), "abcd");
    }

    #[test]
    fn text_area_modifier_key_events_toggle_word_ops() {
        let mut area = TextArea::new().with_value("foo bar");
        focus(&mut area);
        // Some platforms deliver the modifier as its own key event
        // rather than a `+`-joined chord — `word_mod_held` covers it.
        assert_eq!(area.event(&mut ev(&key("Control"))), EventResponse::Handled);
        // While held, arrows jump words and Backspace deletes words.
        area.event(&mut ev(&key("ArrowLeft")));
        assert_eq!(area.cursor(), Cursor::new(0, 4));
        area.event(&mut ev(&key("ArrowRight")));
        assert_eq!(area.cursor(), Cursor::new(0, 7));
        area.event(&mut ev(&key("Backspace")));
        assert_eq!(area.value(), "foo ");
        // Release returns to single-grapheme deletes — the state must
        // not leak into the next keypress.
        assert_eq!(
            area.event(&mut ev(&key_rel("Control"))),
            EventResponse::Handled
        );
        area.event(&mut ev(&key("Backspace")));
        assert_eq!(area.value(), "foo");
    }

    #[test]
    fn text_area_shift_release_collapses_extension() {
        let mut area = TextArea::new().with_value("abc");
        focus(&mut area);
        // Caret at (0,3). Shift held + ArrowLeft extends backward.
        area.event(&mut ev(&key("Shift")));
        area.event(&mut ev(&key("ArrowLeft")));
        assert_eq!(
            area.editor.selection(),
            Some((Cursor::new(0, 2), Cursor::new(0, 3)))
        );
        // After the release the next plain arrow collapses to the
        // selection edge rather than extending further.
        area.event(&mut ev(&key_rel("Shift")));
        area.event(&mut ev(&key("ArrowRight")));
        assert_eq!(area.editor.selection(), None);
        assert_eq!(area.cursor(), Cursor::new(0, 3));
    }

    #[test]
    fn text_area_ctrl_home_end_document_edges() {
        let mut area = TextArea::new().with_value("one\ntwo\nthree");
        focus(&mut area);
        // `set_value` parks the caret at (2,5) — the document end.
        area.event(&mut ev(&key("Ctrl+Home")));
        assert_eq!(area.cursor(), Cursor::new(0, 0));
        area.event(&mut ev(&key("Ctrl+End")));
        assert_eq!(area.cursor(), Cursor::new(2, 5));
        // The word-modifier vertical arrows are the macOS equivalents.
        area.event(&mut ev(&key("Ctrl+ArrowUp")));
        assert_eq!(area.cursor(), Cursor::new(0, 0));
        area.event(&mut ev(&key("Ctrl+ArrowDown")));
        assert_eq!(area.cursor(), Cursor::new(2, 5));
        // Ctrl+Shift+Home selects back to the document start.
        area.event(&mut ev(&key("Ctrl+Shift+Home")));
        assert_eq!(area.cursor(), Cursor::new(0, 0));
        assert_eq!(
            area.editor.selection(),
            Some((Cursor::new(0, 0), Cursor::new(2, 5)))
        );
    }

    #[test]
    fn text_area_shift_home_end_select_to_line_edges() {
        let mut area = TextArea::new().with_value("ab\ncd");
        focus(&mut area);
        // Caret at (1,2). Shift+Home selects the line back to its
        // start — the anchor stays where the caret was.
        area.event(&mut ev(&key("Shift+Home")));
        assert_eq!(
            area.editor.selection(),
            Some((Cursor::new(1, 0), Cursor::new(1, 2)))
        );
        assert_eq!(area.cursor(), Cursor::new(1, 0));
        // Extending back to the anchor collapses the selection to
        // nothing — an anchor==head pair is not a selection.
        area.event(&mut ev(&key("Shift+End")));
        assert_eq!(area.editor.selection(), None);
        assert_eq!(area.cursor(), Cursor::new(1, 2));
    }

    #[test]
    fn text_area_enter_at_line_edges() {
        let mut area = TextArea::new().with_value("ab");
        focus(&mut area);
        // Enter at the line start pushes the whole line down.
        area.event(&mut ev(&key("Home")));
        area.event(&mut ev(&key("Enter")));
        assert_eq!(area.value(), "\nab");
        assert_eq!(area.cursor(), Cursor::new(1, 0));
        // Backspace rejoins it.
        area.event(&mut ev(&key("Backspace")));
        assert_eq!(area.value(), "ab");
        // Enter at the line end appends a blank line — typing lands
        // on it.
        area.event(&mut ev(&key("End")));
        area.event(&mut ev(&key("Enter")));
        assert_eq!(area.value(), "ab\n");
        assert_eq!(area.cursor(), Cursor::new(1, 0));
        area.event(&mut ev(&ime("c")));
        assert_eq!(area.value(), "ab\nc");
    }

    #[test]
    fn text_area_enter_on_empty_buffer() {
        let mut area = TextArea::new();
        focus(&mut area);
        area.event(&mut ev(&key("Enter")));
        assert_eq!(area.value(), "\n");
        assert_eq!(area.cursor(), Cursor::new(1, 0));
        // An all-empty-lines document still navigates: Up crosses the
        // break, Right from line start crosses it again.
        area.event(&mut ev(&key("ArrowUp")));
        assert_eq!(area.cursor(), Cursor::new(0, 0));
        area.event(&mut ev(&key("ArrowRight")));
        assert_eq!(area.cursor(), Cursor::new(1, 0));
    }

    #[test]
    fn text_area_enter_replaces_selection_across_lines() {
        let mut area = TextArea::new().with_value("ab\ncd");
        focus(&mut area);
        area.editor
            .set_selection(Cursor::new(0, 1), Cursor::new(1, 1));
        area.event(&mut ev(&key("Enter")));
        // "b\nc" out, "\n" in.
        assert_eq!(area.value(), "a\nd");
        assert_eq!(area.cursor(), Cursor::new(1, 0));
        // One undo step restores the text AND the open selection.
        area.event(&mut ev(&key("Ctrl+z")));
        assert_eq!(area.value(), "ab\ncd");
        assert_eq!(
            area.editor.selection(),
            Some((Cursor::new(0, 1), Cursor::new(1, 1)))
        );
    }

    #[test]
    fn text_area_tab_replaces_selection() {
        let mut area = TextArea::new().with_value("a\nb");
        focus(&mut area);
        area.event(&mut ev(&key("SelectAll")));
        area.event(&mut ev(&key("Tab")));
        assert_eq!(area.value(), "\t");
        assert_eq!(area.cursor(), Cursor::new(0, 1));
    }

    #[test]
    fn text_area_noop_deletes_do_not_flag_edits() {
        let mut area = TextArea::new().with_value("ab");
        focus(&mut area);
        // `set_value` itself flags an edit — drain it so only the
        // no-op deletes below are observed.
        area.take_edited();
        // Forward delete at the document end is a no-op — no undo
        // snapshot, no edit flag.
        area.event(&mut ev(&key("Delete")));
        assert_eq!(area.value(), "ab");
        assert_eq!(area.take_edited(), None);
        // Same for Ctrl+Delete at the document end.
        area.event(&mut ev(&key("Ctrl+Delete")));
        assert_eq!(area.value(), "ab");
        assert_eq!(area.take_edited(), None);
        // Backspace at the document start, and its word variant.
        area.event(&mut ev(&key("Home")));
        area.event(&mut ev(&key("Backspace")));
        area.event(&mut ev(&key("Ctrl+Backspace")));
        assert_eq!(area.value(), "ab");
        assert_eq!(area.take_edited(), None);
    }

    #[test]
    fn text_area_delete_forward_removes_selection() {
        let mut area = TextArea::new().with_value("ab\ncd");
        focus(&mut area);
        area.editor
            .set_selection(Cursor::new(0, 1), Cursor::new(1, 1));
        area.event(&mut ev(&key("Delete")));
        assert_eq!(area.value(), "ad");
        assert_eq!(area.cursor(), Cursor::new(0, 1));
    }

    #[test]
    fn text_area_undo_redo_across_line_joins() {
        let mut area = TextArea::new().with_value("ab\ncd");
        focus(&mut area);
        area.event(&mut ev(&key("Home"))); // caret (1,0)
        area.event(&mut ev(&key("Backspace"))); // joins -> "abcd"
        assert_eq!(area.value(), "abcd");
        assert_eq!(area.cursor(), Cursor::new(0, 2));
        // Undo restores the split AND the pre-edit caret.
        area.event(&mut ev(&key("Ctrl+z")));
        assert_eq!(area.value(), "ab\ncd");
        assert_eq!(area.cursor(), Cursor::new(1, 0));
        // Redo rejoins.
        area.event(&mut ev(&key("Ctrl+y")));
        assert_eq!(area.value(), "abcd");
        assert_eq!(area.cursor(), Cursor::new(0, 2));
    }

    #[test]
    fn text_area_undo_restores_selection_snapshot() {
        let mut area = TextArea::new().with_value("a\nb");
        focus(&mut area);
        area.event(&mut ev(&key("SelectAll")));
        area.event(&mut ev(&ime("z")));
        assert_eq!(area.value(), "z");
        area.event(&mut ev(&key("Ctrl+z")));
        assert_eq!(area.value(), "a\nb");
        // The snapshot carries the whole editing context — the
        // select-all anchor comes back too.
        assert_eq!(
            area.editor.selection(),
            Some((Cursor::new(0, 0), Cursor::new(1, 1)))
        );
        area.event(&mut ev(&key("Ctrl+y")));
        assert_eq!(area.value(), "z");
    }

    #[test]
    fn text_area_undo_flags_edited() {
        let mut area = TextArea::new();
        focus(&mut area);
        area.event(&mut ev(&ime("x")));
        assert_eq!(area.take_edited(), Some(()));
        // Undo and redo ARE buffer changes — the flag must fire so
        // observers refresh.
        area.event(&mut ev(&key("Ctrl+z")));
        assert_eq!(area.take_edited(), Some(()));
        area.event(&mut ev(&key("Ctrl+y")));
        assert_eq!(area.take_edited(), Some(()));
    }

    #[test]
    fn text_area_undo_redo_synthetic_key_names() {
        let mut area = TextArea::new();
        focus(&mut area);
        area.event(&mut ev(&ime("ab")));
        // The window layer's dispatched names work alongside chords.
        assert_eq!(
            area.event(&mut ev(&key("Undo"))),
            EventResponse::RequestRepaint
        );
        assert_eq!(area.value(), "");
        assert_eq!(
            area.event(&mut ev(&key("Redo"))),
            EventResponse::RequestRepaint
        );
        assert_eq!(area.value(), "ab");
        // Exhausted history is consumed quietly — Handled, not
        // Ignored (a sibling must not claim the key).
        area.event(&mut ev(&key("Undo")));
        assert_eq!(area.event(&mut ev(&key("Undo"))), EventResponse::Handled);
        assert_eq!(area.value(), "");
        // The undone insert is still on the redo branch — one Redo
        // replays it, the next is the quiet Handled.
        assert_eq!(
            area.event(&mut ev(&key("Redo"))),
            EventResponse::RequestRepaint
        );
        assert_eq!(area.value(), "ab");
        assert_eq!(area.event(&mut ev(&key("Redo"))), EventResponse::Handled);
    }

    #[test]
    fn text_area_undo_history_is_bounded() {
        let mut area = TextArea::new();
        focus(&mut area);
        for _ in 0..UNDO_LIMIT + 5 {
            area.event(&mut ev(&ime("x")));
        }
        // Only the newest UNDO_LIMIT steps survive — the first five
        // inserts are gone for good.
        for _ in 0..UNDO_LIMIT {
            area.event(&mut ev(&key("Ctrl+z")));
        }
        assert_eq!(area.value(), "x".repeat(5));
        assert_eq!(area.event(&mut ev(&key("Ctrl+z"))), EventResponse::Handled);
        assert_eq!(area.value(), "x".repeat(5));
    }

    #[test]
    fn text_area_word_delete_crosses_line_boundary() {
        let mut area = TextArea::new().with_value("foo\nbar");
        focus(&mut area);
        // Caret at (1,0): the previous word edge is the previous
        // line's end, so Ctrl+Backspace eats the line break — the
        // same crossing ArrowLeft makes.
        area.event(&mut ev(&key("Home")));
        area.event(&mut ev(&key("Ctrl+Backspace")));
        assert_eq!(area.value(), "foobar");
        assert_eq!(area.cursor(), Cursor::new(0, 3));
        area.event(&mut ev(&key("Ctrl+z")));
        assert_eq!(area.value(), "foo\nbar");
        // Ctrl+Delete at EOL eats the break forward.
        area.event(&mut ev(&key("ArrowUp"))); // restored caret (1,0) -> (0,0)
        area.event(&mut ev(&key("End")));
        area.event(&mut ev(&key("Ctrl+Delete")));
        assert_eq!(area.value(), "foobar");
        assert_eq!(area.cursor(), Cursor::new(0, 3));
    }

    #[test]
    fn text_area_word_delete_with_selection_removes_only_selection() {
        let mut area = TextArea::new().with_value("alpha beta");
        focus(&mut area);
        area.editor.select_word_at(Cursor::new(0, 8)); // "beta"
                                                       // A word-delete with an open selection removes exactly the
                                                       // selection — it does not reach for the next word.
        area.event(&mut ev(&key("Ctrl+Backspace")));
        assert_eq!(area.value(), "alpha ");
        assert_eq!(area.cursor(), Cursor::new(0, 6));
        area.event(&mut ev(&key("Ctrl+Delete")));
        assert_eq!(area.value(), "alpha ");
        // Now without a selection the word ops resume.
        area.event(&mut ev(&key("Ctrl+Backspace")));
        assert_eq!(area.value(), "alpha");
    }

    #[test]
    fn text_area_ctrl_shift_arrows_select_words() {
        let mut area = TextArea::new().with_value("foo bar\nbaz");
        focus(&mut area);
        // Caret at (1,3) — the chord form carries Shift in the name.
        area.event(&mut ev(&key("Ctrl+Shift+ArrowLeft")));
        assert_eq!(area.editor.selected_text().as_deref(), Some("baz"));
        // Extending again crosses the line break to the previous word
        // edge — the end of "bar".
        area.event(&mut ev(&key("Ctrl+Shift+ArrowLeft")));
        assert_eq!(area.editor.selected_text().as_deref(), Some("\nbaz"));
        area.event(&mut ev(&key("Ctrl+Shift+ArrowRight")));
        assert_eq!(area.editor.selected_text().as_deref(), Some("baz"));
    }

    #[test]
    fn text_area_multiline_insert_splits_line() {
        let mut area = TextArea::new().with_value("ab");
        focus(&mut area);
        area.event(&mut ev(&key("ArrowLeft"))); // caret (0,1)
        area.event(&mut ev(&ime("x\ny")));
        // The suffix after the caret lands on the last inserted line.
        assert_eq!(area.value(), "ax\nyb");
        assert_eq!(area.cursor(), Cursor::new(1, 1));
        // One undo step reverts the whole multi-line commit.
        area.event(&mut ev(&key("Ctrl+z")));
        assert_eq!(area.value(), "ab");
        assert_eq!(area.cursor(), Cursor::new(0, 1));
    }

    #[test]
    fn text_area_multiline_insert_over_selection() {
        let mut area = TextArea::new().with_value("ab\ncd");
        focus(&mut area);
        area.event(&mut ev(&key("SelectAll")));
        area.event(&mut ev(&ime("x\ny\nz")));
        assert_eq!(area.value(), "x\ny\nz");
        assert_eq!(area.cursor(), Cursor::new(2, 1));
        // Still a single undo step despite spanning three lines.
        area.event(&mut ev(&key("Ctrl+z")));
        assert_eq!(area.value(), "ab\ncd");
    }

    #[test]
    fn text_area_ime_normalizes_crlf_and_cr() {
        let mut area = TextArea::new();
        focus(&mut area);
        // Committed text can carry CRLF pairs (Windows clipboards)
        // and bare CRs (classic Mac) — both become `\n`.
        area.event(&mut ev(&ime("a\r\nb\rc\nd")));
        assert_eq!(area.value(), "a\nb\nc\nd");
        assert_eq!(area.cursor(), Cursor::new(3, 1));
    }

    #[test]
    fn text_area_control_char_only_insert() {
        let mut area = TextArea::new().with_value("ab");
        focus(&mut area);
        area.take_edited(); // drain the flag `set_value` raises
                            // A payload that sanitizes to nothing is a true no-op — no
                            // undo snapshot, no edit flag.
        area.event(&mut ev(&ime("\u{0}\u{7}\u{b}\u{1f}")));
        assert_eq!(area.value(), "ab");
        assert_eq!(area.take_edited(), None);
        // Over an open selection the same payload still counts as an
        // edit: an empty paste replaces the selection.
        area.event(&mut ev(&key("SelectAll")));
        area.event(&mut ev(&ime("\u{7}")));
        assert_eq!(area.value(), "");
        assert_eq!(area.take_edited(), Some(()));
    }

    #[test]
    fn text_area_edit_resets_sticky_column() {
        let mut area = TextArea::new().with_value("abcdef\nx\nabcde");
        focus(&mut area);
        // From (2,5) up twice parks on (0,5); down clamps onto the
        // one-char middle line with the sticky column 5 remembered.
        area.event(&mut ev(&key("ArrowUp")));
        area.event(&mut ev(&key("ArrowUp")));
        assert_eq!(area.cursor(), Cursor::new(0, 5));
        area.event(&mut ev(&key("ArrowDown")));
        assert_eq!(area.cursor(), Cursor::new(1, 1));
        // An edit clears the sticky column — the next vertical move
        // anchors on the post-edit caret, not the stale preference.
        area.event(&mut ev(&ime("y")));
        assert_eq!(area.cursor(), Cursor::new(1, 2));
        area.event(&mut ev(&key("ArrowDown")));
        assert_eq!(area.cursor(), Cursor::new(2, 2));
    }

    #[test]
    fn text_area_click_resets_sticky_column() {
        let mut area = TextArea::new().with_value("abcdef\nx\nabcde");
        focus(&mut area);
        area.event(&mut ev(&key("ArrowUp")));
        area.event(&mut ev(&key("ArrowUp")));
        assert_eq!(area.cursor(), Cursor::new(0, 5));
        area.event(&mut ev(&key("ArrowDown")));
        assert_eq!(area.cursor(), Cursor::new(1, 1));
        // A click reseeds the column — the next vertical move must
        // not resurrect the stale preferred column.
        let p = pos_of(&mut area, 1, 0);
        press(&mut area, p, 1);
        assert_eq!(area.cursor(), Cursor::new(1, 0));
        area.event(&mut ev(&key("ArrowDown")));
        assert_eq!(area.cursor(), Cursor::new(2, 0));
    }

    #[test]
    fn text_area_shift_vertical_selects_with_sticky_column() {
        let mut area = TextArea::new().with_value("abcdef\nx\nabcde");
        focus(&mut area);
        // Caret (2,5); up to (0,5) first.
        area.event(&mut ev(&key("ArrowUp")));
        area.event(&mut ev(&key("ArrowUp")));
        assert_eq!(area.cursor(), Cursor::new(0, 5));
        // Shift+Down twice: the sticky column rides through the
        // one-char middle line, so the head lands back on column 5.
        area.event(&mut ev(&key("Shift")));
        area.event(&mut ev(&key("ArrowDown")));
        assert_eq!(area.cursor(), Cursor::new(1, 1));
        area.event(&mut ev(&key("ArrowDown")));
        assert_eq!(area.cursor(), Cursor::new(2, 5));
        assert_eq!(area.editor.selected_text().as_deref(), Some("f\nx\nabcde"));
    }

    #[test]
    fn text_area_page_up_down_clamp_at_document_edges() {
        let mut area = TextArea::new().with_value(&("l\n".repeat(9) + "l"));
        focus(&mut area);
        // Caret at line 9. Repeated PageUp clamps at the first line —
        // never past it.
        area.event(&mut ev(&key("PageUp")));
        assert!(area.cursor().line > 0);
        area.event(&mut ev(&key("PageUp")));
        assert_eq!(area.cursor(), Cursor::new(0, 1));
        area.event(&mut ev(&key("PageUp")));
        assert_eq!(area.cursor(), Cursor::new(0, 1));
        // …and PageDown clamps at the last.
        area.event(&mut ev(&key("PageDown")));
        area.event(&mut ev(&key("PageDown")));
        assert_eq!(area.cursor(), Cursor::new(9, 1));
        // Shift+PageUp extends by a viewport of rows.
        area.event(&mut ev(&key("Shift")));
        area.event(&mut ev(&key("PageUp")));
        let (lo, hi) = area.editor.selection().unwrap();
        assert_eq!(hi, Cursor::new(9, 1));
        assert!(hi.line - lo.line >= 1);
    }

    #[test]
    fn text_area_arrows_clamp_at_document_edges() {
        let mut area = TextArea::new().with_value("ab\ncd");
        focus(&mut area);
        // Caret (1,2) — the document end.
        area.event(&mut ev(&key("ArrowUp")));
        assert_eq!(area.cursor(), Cursor::new(0, 2));
        area.event(&mut ev(&key("ArrowUp")));
        assert_eq!(area.cursor(), Cursor::new(0, 2)); // stays
                                                      // Shift+ArrowUp on the first line is a true no-op — no
                                                      // phantom selection past the document start.
        area.event(&mut ev(&key("Shift")));
        area.event(&mut ev(&key("ArrowUp")));
        assert_eq!(area.editor.selection(), None);
        assert_eq!(area.cursor(), Cursor::new(0, 2));
        area.event(&mut ev(&key_rel("Shift")));
        // Back down clamps on the last line.
        area.event(&mut ev(&key("ArrowDown")));
        area.event(&mut ev(&key("ArrowDown")));
        assert_eq!(area.cursor(), Cursor::new(1, 2));
    }

    #[test]
    fn text_area_click_past_content_clamps() {
        let mut area = TextArea::new().with_value("ab\ncd");
        focus(&mut area);
        // Below the last row the caret clamps to the last line.
        press(&mut area, Vec2::new(10.0, 95.0), 1);
        assert_eq!(area.cursor().line, 1);
        // Far past a short line's end it lands at EOL, not inside
        // some later character.
        press(&mut area, Vec2::new(190.0, 10.0), 1);
        assert_eq!(area.cursor(), Cursor::new(0, 2));
    }

    #[test]
    fn text_area_hit_test_accounts_for_vertical_scroll() {
        let mut area = TextArea::new().with_value(
            (0..30)
                .map(|i| format!("line {i}"))
                .collect::<Vec<_>>()
                .join("\n"),
        );
        focus(&mut area);
        // Scroll five rows down, then click the first visible row —
        // it must hit line 5, not line 0.
        area.event(&mut ev(&WidgetEvent::Scroll {
            position: Vec2::new(10.0, 10.0),
            delta: Vec2::new(0.0, 5.0 * LINE_PT),
        }));
        assert!((area.scroll_offset().y - 5.0 * LINE_PT).abs() < 0.01);
        press(&mut area, Vec2::new(10.0, 7.0), 1);
        assert_eq!(area.cursor().line, 5);
    }

    #[test]
    fn text_area_hit_test_accounts_for_horizontal_scroll() {
        let mut area = TextArea::new().wrap(false).with_value("x".repeat(400));
        focus(&mut area);
        area.event(&mut ev(&WidgetEvent::Scroll {
            position: Vec2::new(10.0, 10.0),
            delta: Vec2::new(200.0, 0.0),
        }));
        assert!(area.scroll_offset().x > 0.0);
        // Clicking just inside the left padding maps mid-line — the
        // scroll offset shifts the glyph hit-test.
        press(&mut area, Vec2::new(9.0, 10.0), 1);
        assert!(area.cursor().column > 10);
    }

    #[test]
    fn text_area_click_on_wrapped_row_hit_tests() {
        let mut area = TextArea::new()
            .with_value("one two three four five six seven eight nine ten eleven twelve");
        focus(&mut area);
        // A click on the second visual row of a wrapped logical line
        // must resolve through the wrap to the clicked column — not
        // to the line start.
        let painter = crate::text_paint::shared_painter();
        let x_of = |t: &str, b: usize| painter.caret_x(t, FONT_PT, b);
        let lay = TextArea::geometry(area.editor.lines(), true, 1.0, BOUNDS, &x_of);
        assert!(lay.rows.len() >= 2);
        let row = lay.rows[1];
        let line_len = area.editor.lines()[0].chars().count();
        let col = byte_col(
            &area.editor.lines()[0],
            row.start + (row.end - row.start) / 2,
        );
        let p = pos_of(&mut area, 0, col);
        press(&mut area, p, 1);
        assert_eq!(area.cursor(), Cursor::new(0, col));
        // Home/End stay logical even mid-wrap (the documented v1
        // simplification — no visual-row Home).
        area.event(&mut ev(&key("Home")));
        assert_eq!(area.cursor(), Cursor::new(0, 0));
        area.event(&mut ev(&key("End")));
        assert_eq!(area.cursor(), Cursor::new(0, line_len));
    }

    #[test]
    fn text_area_wrap_rows_never_start_with_whitespace() {
        let area = TextArea::new()
            .with_value("one two three four five six seven eight nine ten eleven twelve");
        let painter = crate::text_paint::shared_painter();
        let x_of = |t: &str, b: usize| painter.caret_x(t, FONT_PT, b);
        let lay = TextArea::geometry(area.editor.lines(), true, 1.0, BOUNDS, &x_of);
        assert!(lay.rows.len() >= 2);
        // Break whitespace folds into the preceding row — no
        // continuation row may begin with a space.
        let line = &area.editor.lines()[0];
        for r in &lay.rows[1..] {
            assert!(
                !line[r.start..].starts_with(char::is_whitespace),
                "row {r:?} starts with whitespace"
            );
        }
    }

    #[test]
    fn text_area_disabled_ignores_pointer_and_keys() {
        let mut area = TextArea::new().with_value("ab").enabled(false);
        // The focus-grab path is gated too — a disabled area never
        // takes focus and never moves the caret.
        let p = pos_of(&mut area, 0, 1);
        assert_eq!(press(&mut area, p, 1), EventResponse::Ignored);
        assert_eq!(area.cursor(), Cursor::new(0, 2));
        assert!(!area.focused());
        assert_eq!(
            area.event(&mut ev(&WidgetEvent::FocusGained)),
            EventResponse::Ignored
        );
        assert!(!area.focused());
        // Scroll falls through so an ancestor region can take it.
        let scroll = WidgetEvent::Scroll {
            position: Vec2::new(10.0, 10.0),
            delta: Vec2::new(0.0, 10.0),
        };
        assert_eq!(area.event(&mut ev(&scroll)), EventResponse::Ignored);
    }

    #[test]
    fn text_area_read_only_navigates_and_selects_but_blocks_edits() {
        let mut area = TextArea::new().with_value("ab\ncd").read_only(true);
        focus(&mut area);
        area.take_edited(); // drain the flag `set_value` raises
                            // Caret motion and selection stay live in read-only mode —
                            // screen readers and copy need them.
        area.event(&mut ev(&key("Home")));
        assert_eq!(area.cursor(), Cursor::new(1, 0));
        area.event(&mut ev(&key("Ctrl+a")));
        assert!(area.editor.selection().is_some());
        // Every mutation path is refused, including chords, the
        // synthetic names, and undo/redo.
        assert_eq!(area.event(&mut ev(&key("Ctrl+z"))), EventResponse::Ignored);
        assert_eq!(area.event(&mut ev(&key("Undo"))), EventResponse::Ignored);
        assert_eq!(area.event(&mut ev(&key("Ctrl+x"))), EventResponse::Ignored);
        assert_eq!(area.event(&mut ev(&key("Ctrl+v"))), EventResponse::Ignored);
        area.event(&mut ev(&key("Enter")));
        area.event(&mut ev(&key("Tab")));
        assert_eq!(area.value(), "ab\ncd");
        assert_eq!(area.take_edited(), None);
        // A pointer click still places the caret.
        let p = pos_of(&mut area, 0, 1);
        assert_eq!(press(&mut area, p, 1), EventResponse::CapturePointer);
        assert_eq!(area.cursor(), Cursor::new(0, 1));
    }

    #[test]
    fn text_area_escape_without_selection_is_ignored() {
        let mut area = TextArea::new().with_value("ab");
        focus(&mut area);
        // Nothing to collapse — the key falls through for a parent
        // (e.g. an IME composition cancel) to claim.
        assert_eq!(area.event(&mut ev(&key("Escape"))), EventResponse::Ignored);
        assert_eq!(area.cursor(), Cursor::new(0, 2));
    }

    #[test]
    fn text_area_escape_collapses_backward_selection_to_head() {
        let mut area = TextArea::new().with_value("abc");
        focus(&mut area);
        // Caret (0,3); select backward over "bc" — the head sits at
        // the selection's low edge.
        area.event(&mut ev(&key("Shift")));
        area.event(&mut ev(&key("ArrowLeft")));
        area.event(&mut ev(&key("ArrowLeft")));
        assert_eq!(
            area.editor.selection(),
            Some((Cursor::new(0, 1), Cursor::new(0, 3)))
        );
        area.event(&mut ev(&key_rel("Shift")));
        // Escape collapses onto the moving end (the head) — the
        // EditorPanel convention.
        area.event(&mut ev(&key("Escape")));
        assert_eq!(area.editor.selection(), None);
        assert_eq!(area.cursor(), Cursor::new(0, 1));
    }

    #[test]
    fn text_area_backward_selection_replaced_by_typing() {
        let mut area = TextArea::new().with_value("abc");
        focus(&mut area);
        // Caret (0,3) — select backward over "bc", then type.
        area.event(&mut ev(&key("Shift")));
        area.event(&mut ev(&key("ArrowLeft")));
        area.event(&mut ev(&key("ArrowLeft")));
        area.event(&mut ev(&key_rel("Shift")));
        area.event(&mut ev(&ime("X")));
        assert_eq!(area.value(), "aX");
        assert_eq!(area.cursor(), Cursor::new(0, 2));
    }

    #[test]
    fn text_area_focus_loss_clears_modifiers_and_drag() {
        let mut area = TextArea::new().with_value("alpha\nbeta");
        focus(&mut area);
        // Hold Shift and begin a shift-click extension, then lose
        // focus mid-gesture — the OS may never deliver the matching
        // releases.
        area.event(&mut ev(&key("Shift")));
        let p = pos_of(&mut area, 0, 1);
        press(&mut area, p, 1);
        assert_eq!(
            area.editor.selection(),
            Some((Cursor::new(0, 1), Cursor::new(1, 4)))
        );
        area.event(&mut ev(&WidgetEvent::FocusLost));
        // The in-flight drag is dead: pointer motion no longer
        // extends the selection.
        let end = pos_of(&mut area, 1, 3);
        assert_eq!(
            area.event(&mut ev(&WidgetEvent::PointerMoved { position: end })),
            EventResponse::Ignored
        );
        // On refocus the leaked Shift must not turn a plain arrow
        // into an extension — it collapses to the left edge instead.
        focus(&mut area);
        area.event(&mut ev(&key("ArrowLeft")));
        assert_eq!(area.editor.selection(), None);
        assert_eq!(area.cursor(), Cursor::new(0, 1));
    }

    #[test]
    fn text_area_caret_tracker_mirrors_selection() {
        let mut area = TextArea::new()
            .with_value("a\nb")
            .with_caret_tracker(CaretTracker::new(
                accesskit::NodeId(7),
                TextSelection::caret(0, TextAffinity::Downstream),
            ));
        focus(&mut area);
        area.event(&mut ev(&key("SelectAll")));
        // Flat document offsets: "a\nb" numbers (1,1) as 1 + '\n' + 1.
        let sel = area.caret_tracker().unwrap().selection;
        assert_eq!((sel.anchor, sel.focus), (0, 3));
        // Collapsing the selection mirrors a plain caret.
        area.event(&mut ev(&key("ArrowLeft")));
        let sel = area.caret_tracker().unwrap().selection;
        assert_eq!((sel.anchor, sel.focus), (0, 0));
    }

    #[test]
    fn text_area_semantic_set_value_blocked_when_read_only() {
        let mut area = TextArea::new().with_value("keep").read_only(true);
        focus(&mut area);
        let set = WidgetEvent::SemanticAction(SemanticAction::SetValue("nope".into()));
        assert_eq!(area.event(&mut ev(&set)), EventResponse::Ignored);
        assert_eq!(area.value(), "keep");
        // ScrollIntoView acknowledges with a repaint — the
        // caret-following scroll in `paint` does the work.
        let siv = WidgetEvent::SemanticAction(SemanticAction::ScrollIntoView);
        assert_eq!(area.event(&mut ev(&siv)), EventResponse::RequestRepaint);
    }

    #[test]
    fn text_area_semantic_scroll_actions() {
        let mut area = TextArea::new().with_value(&("l\n".repeat(29) + "l"));
        focus(&mut area);
        // SetScrollOffset applies an absolute offset through the same
        // clamped `scroll_by`.
        let s = WidgetEvent::SemanticAction(SemanticAction::SetScrollOffset(Vec2::new(0.0, 50.0)));
        assert_eq!(area.event(&mut ev(&s)), EventResponse::RequestRepaint);
        assert!((area.scroll_offset().y - 50.0).abs() < 0.01);
        // Setting the offset it already holds is a quiet Handled.
        assert_eq!(area.event(&mut ev(&s)), EventResponse::Handled);
        // ScrollUp steps a line at a time.
        let up = WidgetEvent::SemanticAction(SemanticAction::ScrollUp);
        assert_eq!(area.event(&mut ev(&up)), EventResponse::RequestRepaint);
        assert!((area.scroll_offset().y - (50.0 - LINE_PT)).abs() < 0.01);
    }

    #[test]
    fn text_area_horizontal_scrollbar_track_pages() {
        let mut area = TextArea::new().wrap(false).with_value("x".repeat(400));
        focus(&mut area);
        // The hbar strip hugs the bottom padding edge when unwrapped
        // content overflows horizontally; a track click below the
        // thumb pages right.
        let p = Vec2::new(100.0, 90.0);
        assert_eq!(press(&mut area, p, 1), EventResponse::CapturePointer);
        assert!(area.scroll_offset().x > 0.0);
        area.event(&mut ev(&WidgetEvent::PointerReleased {
            position: p,
            button: PointerButton::Primary,
        }));
    }

    #[test]
    fn text_area_grapheme_ops_on_later_lines() {
        let mut area = TextArea::new().with_value("ab\ne\u{301}x");
        focus(&mut area);
        // Caret (1,3): "e\u{301}x" is three chars but two clusters.
        area.event(&mut ev(&key("Home")));
        area.event(&mut ev(&key("ArrowRight"))); // steps the cluster
        assert_eq!(area.cursor(), Cursor::new(1, 2));
        // Backspace removes the whole combining-mark cluster.
        area.event(&mut ev(&key("Backspace")));
        assert_eq!(area.value(), "ab\nx");
        // Backspace again joins the line upward.
        area.event(&mut ev(&key("Backspace")));
        assert_eq!(area.value(), "abx");
        assert_eq!(area.cursor(), Cursor::new(0, 2));
    }

    #[test]
    fn text_area_cut_removes_selection_in_one_undo_step() {
        let mut area = TextArea::new().with_value("a\nb");
        focus(&mut area);
        area.event(&mut ev(&key("SelectAll")));
        assert_eq!(
            area.event(&mut ev(&key("Ctrl+x"))),
            EventResponse::RequestRepaint
        );
        assert_eq!(area.value(), "");
        assert_eq!(area.editor.selection(), None);
        // Cut is a single undo step that restores the select-all
        // anchor too.
        area.event(&mut ev(&key("Ctrl+z")));
        assert_eq!(area.value(), "a\nb");
        assert_eq!(
            area.editor.selection(),
            Some((Cursor::new(0, 0), Cursor::new(1, 1)))
        );
    }

    #[test]
    fn text_area_cut_copy_without_selection_are_noops() {
        let mut area = TextArea::new().with_value("ab");
        focus(&mut area);
        area.take_edited(); // drain the flag `set_value` raises
                            // Nothing selected: copy is consumed quietly, cut repaints
                            // but changes nothing.
        assert_eq!(area.event(&mut ev(&key("Ctrl+c"))), EventResponse::Handled);
        assert_eq!(
            area.event(&mut ev(&key("Ctrl+x"))),
            EventResponse::RequestRepaint
        );
        assert_eq!(area.value(), "ab");
        assert_eq!(area.take_edited(), None);
    }

    #[test]
    fn text_area_set_value_clears_preedit() {
        let mut area = TextArea::new();
        focus(&mut area);
        area.event(&mut ev(&WidgetEvent::ImePreedit {
            text: "wip".to_string(),
            cursor: None,
        }));
        assert_eq!(area.preedit(), Some("wip"));
        // A programmatic write discards the in-flight composition —
        // the preedit belongs to the document it was composed into.
        area.set_value("fresh");
        assert_eq!(area.preedit(), None);
    }
}
