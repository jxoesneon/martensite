//! `TextInput` widget: an editable text field with an accessible label.
//!
//! The `TextInput` widget exposes `Role::TextInput`, an accessible label,
//! the `Action::Focus` and `Action::SetValue` accessibility actions, and
//! the current value. It integrates with the focus system via
//! `NodeFlags::FOCUSABLE`.
//!
//! Beyond plain editing the widget supports a password echo mode
//! ([`TextInput::secure`]) with an optional reveal toggle
//! ([`TextInput::revealable`]), `prefix`/`suffix` adornments, a
//! `clearable` ✕ target,
//! [`ValidationState`](crate::widgets::text_input::ValidationState)
//! border/message visuals,
//! word-jump editing keys, and a bounded undo/redo stack.
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
use martensite_sanitize::{Phase, Sanitize, SanitizeContext, SanitizerConfig};
use std::collections::VecDeque;
use std::sync::Arc;
use unicode_segmentation::UnicodeSegmentation;

/// Field background colour.
const FACE: [u8; 4] = [255, 255, 255, 255];
/// Field border colour.
const EDGE: [u8; 4] = [140, 145, 155, 255];
/// Border colour while focused.
const EDGE_FOCUSED: [u8; 4] = [40, 110, 220, 255];
/// Border colour for [`ValidationState::Error`].
const EDGE_ERROR: [u8; 4] = [210, 60, 60, 255];
/// Border colour for [`ValidationState::Warning`].
const EDGE_WARNING: [u8; 4] = [220, 150, 40, 255];
/// Border colour for [`ValidationState::Valid`].
const EDGE_VALID: [u8; 4] = [50, 160, 90, 255];
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
/// One-line field height in logical pt — the `measure` floor and the
/// threshold above which a `validation_message` strip can fit.
const LINE_PT: f32 = 24.0;
/// Height of the validation-message strip in logical pt.
const MSG_STRIP_PT: f32 = 16.0;
/// Font size in logical pt for the validation message.
const MSG_FONT_PT: f32 = 11.0;
/// Edge-affordance column width (logical pt) for the reveal and
/// clear targets — a square finger-friendly strip inside the face.
const ZONE_PT: f32 = 18.0;
/// The masking glyph painted in `secure` mode.
const BULLET: char = '•';
/// `BULLET`'s UTF-8 length — display-byte math multiplies by it.
const BULLET_LEN: usize = BULLET.len_utf8();
/// Undo/redo depth — one `(value, caret, anchor)` snapshot per edit.
const UNDO_LIMIT: usize = 100;

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

/// The validation visual state of a [`TextInput`] — drives the field
/// border colour, the optional `validation_message` strip, and the
/// `aria-invalid` accessibility flag.
///
/// # Examples
///
/// ```
/// use martensite::widgets::text_input::{TextInput, ValidationState};
///
/// let input = TextInput::new("Email")
///     .validation(ValidationState::Error)
///     .validation_message("not a valid address");
/// assert_eq!(input.validation, Some(ValidationState::Error));
/// ```
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ValidationState {
    /// The value fails validation — the border paints `ErrorColor`
    /// and the AccessKit node is marked `Invalid::True`.
    Error,
    /// The value is accepted but suspect — the border paints
    /// `WarningColor`.
    Warning,
    /// The value passed validation — the border paints
    /// `SuccessColor`.
    Valid,
}

/// One undoable snapshot of the editable state — the value plus the
/// caret/selection that was in effect, so undo restores the editing
/// context, not just the text.
#[derive(Clone, Debug)]
struct EditSnapshot {
    /// `value` before the recorded edit.
    value: String,
    /// `cursor` before the recorded edit.
    cursor: usize,
    /// `selection_anchor` before the recorded edit.
    anchor: Option<usize>,
}

/// Caret positions a word-jump lands on: the edges of every non-
/// whitespace UAX#29 word segment plus the two extremes, sorted and
/// deduplicated. Whitespace runs are crossed, never landed inside —
/// Ctrl/Cmd+Arrow hops between word starts and ends the way macOS
/// Option+Arrow does.
fn word_edges(text: &str) -> Vec<usize> {
    let mut edges = Vec::new();
    for (i, seg) in text.split_word_bound_indices() {
        if !seg.trim().is_empty() {
            edges.push(i);
            edges.push(i + seg.len());
        }
    }
    edges.push(0);
    edges.push(text.len());
    edges.sort_unstable();
    edges.dedup();
    edges
}

/// The byte offset where the grapheme cluster containing `pos`'s
/// predecessor starts — ArrowLeft's and Backspace's target. Caret
/// motion and deletion step whole extended grapheme clusters so
/// combining marks, regional-indicator flags, and ZWJ emoji
/// sequences never split under the caret, matching platform
/// text-field behavior.
fn prev_grapheme_boundary(text: &str, pos: usize) -> usize {
    text[..pos]
        .grapheme_indices(true)
        .next_back()
        .map_or(0, |(i, _)| i)
}

/// The byte offset just past the grapheme cluster starting at `pos`
/// — ArrowRight's and Delete's target.
fn next_grapheme_boundary(text: &str, pos: usize) -> usize {
    text[pos..]
        .graphemes(true)
        .next()
        .map_or(text.len(), |g| pos + g.len())
}

/// Splits a chorded key name like `"Ctrl+Shift+ArrowLeft"` into its
/// modifier flags and base name. `WidgetEvent::KeyPressed` carries no
/// modifier state (F17), so window layers either dispatch the
/// modifier keys themselves (`"Control"`, `"Meta"`, `"Super"`,
/// `"Shift"` — tracked into `word_mod_held`/`shift_held`) or send
/// `+`-joined chord names; both encodings resolve here.
///
/// Returns `(word_modifier, shift, base_key)`.
pub(crate) fn parse_key_chord(key: &str) -> (bool, bool, &str) {
    let mut word = false;
    let mut shift = false;
    let mut rest = key;
    while let Some((head, tail)) = rest.split_once('+') {
        match head {
            "Ctrl" | "Control" | "Cmd" | "Meta" | "Super" => word = true,
            "Shift" => shift = true,
            "Alt" | "Option" => {}
            _ => break,
        }
        rest = tail;
    }
    (word, shift, rest)
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
    /// Password echo mode — the field paints `•` bullets instead of
    /// the real glyphs. Editing logic, selection, hit-testing, and
    /// clipboard all keep working on the real value (Copy/Cut copy
    /// the real text, matching platform password fields).
    pub secure: bool,
    /// Shows an eye affordance at the field's right edge while
    /// `secure` — pressing it toggles the masked/plain display (see
    /// [`is_revealed`](Self::is_revealed)).
    pub revealable: bool,
    /// Shows an ✕ clear target at the field's right edge while the
    /// value is non-empty and the field is editable — pressing it
    /// clears `value` and records the edit for undo and
    /// [`take_edited`](Self::take_edited).
    pub clearable: bool,
    /// Static text painted muted inside the face ahead of the value
    /// (e.g. `"$"`); the text run shifts right of it.
    pub prefix: Option<String>,
    /// Static text painted muted inside the face before the right
    /// edge adornments (e.g. `" px"`).
    pub suffix: Option<String>,
    /// Validation state driving the border colour — see
    /// [`ValidationState`].
    pub validation: Option<ValidationState>,
    /// Optional message painted small below the field — only when the
    /// widget's bounds leave a `MSG_STRIP_PT` strip under a one-line
    /// face; exactly-one-line bounds skip it (documented limitation).
    pub validation_message: Option<String>,
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
    /// Ctrl/Cmd/Super state tracked from `KeyPressed`/`KeyReleased`
    /// the same way `shift_held` tracks Shift — drives word-jump
    /// editing (Ctrl+Arrow, Ctrl+Backspace).
    word_mod_held: bool,
    /// `true` while a `secure` field's reveal toggle shows the real
    /// text instead of bullets.
    revealed: bool,
    /// In-flight IME composition — the preedit string and its caret
    /// byte-range, rendered underlined at the insertion caret until the
    /// matching `ImeCommitted` arrives (or an empty preedit clears it).
    preedit: Option<(String, Option<(usize, usize)>)>,
    /// Set when a user-driven edit mutates `value` — the
    /// [`take_edited`](Self::take_edited) out-seam (mirrors
    /// `Banner::take_dismissed`).
    edited: bool,
    /// The sanitization pipeline applied to every text ingestion
    /// (IME commit, paste, programmatic `set_value`). See
    /// [`TextInput::sanitize`], [`TextInput::raw`],
    /// [`TextInput::with_sanitizer`].
    sanitizer: SanitizerConfig,
    /// Bounded undo stack — newest snapshot at the back.
    undo: VecDeque<EditSnapshot>,
    /// Snapshots undone since the last edit — redo restores them,
    /// and any fresh mutation clears the branch.
    redo: VecDeque<EditSnapshot>,
}

impl Clone for TextInput {
    fn clone(&self) -> Self {
        Self {
            label: self.label.clone(),
            value: self.value.clone(),
            placeholder: self.placeholder.clone(),
            enabled: self.enabled,
            read_only: self.read_only,
            secure: self.secure,
            revealable: self.revealable,
            clearable: self.clearable,
            prefix: self.prefix.clone(),
            suffix: self.suffix.clone(),
            validation: self.validation,
            validation_message: self.validation_message.clone(),
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
            word_mod_held: self.word_mod_held,
            revealed: self.revealed,
            preedit: self.preedit.clone(),
            edited: self.edited,
            sanitizer: self.sanitizer.clone(),
            undo: self.undo.clone(),
            redo: self.redo.clone(),
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
            secure: false,
            revealable: false,
            clearable: false,
            prefix: None,
            suffix: None,
            validation: None,
            validation_message: None,
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
            word_mod_held: false,
            revealed: false,
            preedit: None,
            edited: false,
            sanitizer: SanitizerConfig::default(),
            undo: VecDeque::new(),
            redo: VecDeque::new(),
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

    /// Toggles input sanitization — `true` (the default) runs the
    /// aggressive [`martensite_sanitize`] profile on every text
    /// ingestion: NFKC normalization, stripping of controls, bidi
    /// overrides, noncharacters, and invisible format characters,
    /// confusable folding, and whitespace policy. `false` keeps only
    /// the structural control-character floor. [`raw`](Self::raw)
    /// disables the engine entirely.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TextInput;
    ///
    /// let mut input = TextInput::new("Name").sanitize(false);
    /// input.set_value("a\u{202e}b");
    /// assert_eq!(input.value, "a\u{202e}b");
    /// ```
    #[inline]
    #[must_use]
    pub fn sanitize(mut self, on: bool) -> Self {
        self.sanitizer = if on {
            SanitizerConfig::Aggressive
        } else {
            SanitizerConfig::Baseline
        };
        self
    }

    /// Fully verbatim input — nothing is removed, normalized, or
    /// rewritten, including newlines in a paste. Use for fields that
    /// must round-trip arbitrary text.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TextInput;
    ///
    /// let mut input = TextInput::new("Hex").raw();
    /// input.set_value("a\u{202e}\x00");
    /// assert_eq!(input.value, "a\u{202e}\x00");
    /// ```
    #[inline]
    #[must_use]
    pub fn raw(mut self) -> Self {
        self.sanitizer = SanitizerConfig::Raw;
        self
    }

    /// Replaces the sanitization pipeline with a caller-supplied rule
    /// — a [`Sanitize`] trait object applied at insert and commit.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TextInput;
    /// use martensite_sanitize::{Sanitize, SanitizeContext};
    /// use std::sync::Arc;
    ///
    /// struct Strip;
    /// impl Sanitize for Strip {
    ///     fn name(&self) -> &'static str { "strip" }
    ///     fn sanitize(&self, input: &str, _: &SanitizeContext) -> String {
    ///         input.chars().filter(|c| c.is_ascii()).collect()
    ///     }
    /// }
    /// let mut input = TextInput::new("Ascii").with_sanitizer(Arc::new(Strip));
    /// input.set_value("aé");
    /// assert_eq!(input.value, "a");
    /// ```
    #[inline]
    #[must_use]
    pub fn with_sanitizer(mut self, rule: Arc<dyn Sanitize>) -> Self {
        self.sanitizer = SanitizerConfig::Custom(rule);
        self
    }

    /// Replaces the sanitization configuration in place — wrapper
    /// widgets propagate their own setting to embedded fields through
    /// this.
    #[inline]
    pub fn set_sanitizer(&mut self, config: SanitizerConfig) {
        self.sanitizer = config;
    }

    /// The configured sanitization pipeline.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TextInput;
    /// use martensite_sanitize::SanitizerConfig;
    ///
    /// let input = TextInput::new("Name").raw();
    /// assert!(input.sanitizer_config().is_raw());
    /// ```
    #[inline]
    pub fn sanitizer_config(&self) -> &SanitizerConfig {
        &self.sanitizer
    }

    /// Sanitizes `text` as ingested content — IME commits, pastes,
    /// and programmatic writes all route through here.
    fn sanitize_insert(&self, text: &str) -> String {
        self.sanitizer
            .sanitize(text, &SanitizeContext::single_line(Phase::Insert))
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

    /// Sets password echo mode — the field paints `•` bullets per
    /// grapheme instead of the real glyphs while all editing logic
    /// keeps working on the real value (Qt's `Password` echo mode).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TextInput;
    ///
    /// let input = TextInput::new("Password").secure(true);
    /// assert!(input.secure);
    /// ```
    #[inline]
    #[must_use]
    pub fn secure(mut self, secure: bool) -> Self {
        self.set_secure(secure);
        self
    }

    /// Sets password echo mode (mutable version).
    ///
    /// Turning `secure` off also resets the reveal toggle so a
    /// re-secured field starts masked again.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TextInput;
    ///
    /// let mut input = TextInput::new("Password");
    /// input.set_secure(true);
    /// assert!(input.secure);
    /// ```
    #[inline]
    pub fn set_secure(&mut self, secure: bool) {
        self.secure = secure;
        if !secure {
            self.revealed = false;
        }
    }

    /// Sets whether a `secure` field shows the eye reveal toggle at
    /// its right edge — pressing the zone flips masked ↔ plain
    /// display.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TextInput;
    ///
    /// let input = TextInput::new("Password").secure(true).revealable(true);
    /// assert!(input.revealable);
    /// ```
    #[inline]
    #[must_use]
    pub fn revealable(mut self, revealable: bool) -> Self {
        self.revealable = revealable;
        self
    }

    /// Whether a `secure` field is currently displaying the real text
    /// (`true`) or bullets (`false`, the default). Toggled by the
    /// reveal affordance or [`set_revealed`](Self::set_revealed).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TextInput;
    ///
    /// let input = TextInput::new("Password").secure(true);
    /// assert!(!input.is_revealed());
    /// ```
    #[inline]
    pub fn is_revealed(&self) -> bool {
        self.revealed
    }

    /// Sets the reveal-toggle state directly — `true` paints the real
    /// text despite `secure`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TextInput;
    ///
    /// let mut input = TextInput::new("Password").secure(true);
    /// input.set_revealed(true);
    /// assert!(input.is_revealed());
    /// ```
    #[inline]
    pub fn set_revealed(&mut self, revealed: bool) {
        self.revealed = revealed;
    }

    /// The in-flight IME composition string, if one is being composed.
    ///
    /// Set by `WidgetEvent::ImePreedit` and cleared by `ImeCommitted`,
    /// an empty preedit, or focus loss. The string is rendered
    /// underlined at the insertion caret — it is not part of
    /// [`value`](Self::value) until committed.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TextInput;
    ///
    /// assert!(TextInput::new("T").preedit().is_none());
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
    /// use martensite::widgets::TextInput;
    ///
    /// assert!(TextInput::new("T").preedit_cursor().is_none());
    /// ```
    #[inline]
    pub fn preedit_cursor(&self) -> Option<(usize, usize)> {
        self.preedit.as_ref().and_then(|(_, c)| *c)
    }

    /// Sets the leading adornment — muted static text (or a short
    /// icon string) painted inside the face ahead of the value; the
    /// text run shifts right of it.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TextInput;
    ///
    /// let input = TextInput::new("Price").prefix("$");
    /// assert_eq!(input.prefix.as_deref(), Some("$"));
    /// ```
    #[inline]
    #[must_use]
    pub fn prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = Some(prefix.into());
        self
    }

    /// Sets the trailing adornment — muted static text painted inside
    /// the face before the right-edge affordance zones.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TextInput;
    ///
    /// let input = TextInput::new("Width").suffix(" px");
    /// assert_eq!(input.suffix.as_deref(), Some(" px"));
    /// ```
    #[inline]
    #[must_use]
    pub fn suffix(mut self, suffix: impl Into<String>) -> Self {
        self.suffix = Some(suffix.into());
        self
    }

    /// Sets whether an ✕ clear target shows at the right edge while
    /// the value is non-empty and the field is editable — pressing it
    /// clears `value`, records the edit for undo, and sets the
    /// [`take_edited`](Self::take_edited) flag.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TextInput;
    ///
    /// let input = TextInput::new("Search").clearable(true);
    /// assert!(input.clearable);
    /// ```
    #[inline]
    #[must_use]
    pub fn clearable(mut self, clearable: bool) -> Self {
        self.clearable = clearable;
        self
    }

    /// Sets the validation state — the border switches to
    /// `ErrorColor`/`WarningColor`/`SuccessColor`, an `Error` marks the
    /// AccessKit node `Invalid::True`, and `validation_message` can
    /// paint a small line under the field.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::text_input::{TextInput, ValidationState};
    ///
    /// let input = TextInput::new("Email").validation(ValidationState::Warning);
    /// assert_eq!(input.validation, Some(ValidationState::Warning));
    /// ```
    #[inline]
    #[must_use]
    pub fn validation(mut self, validation: ValidationState) -> Self {
        self.validation = Some(validation);
        self
    }

    /// Sets the validation state (mutable version; `None` clears it).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::text_input::{TextInput, ValidationState};
    ///
    /// let mut input = TextInput::new("Email").validation(ValidationState::Error);
    /// input.set_validation(None);
    /// assert_eq!(input.validation, None);
    /// ```
    #[inline]
    pub fn set_validation(&mut self, validation: Option<ValidationState>) {
        self.validation = validation;
    }

    /// Sets the message painted small below the field — only while
    /// the widget's bounds leave room under a one-line face (taller
    /// bounds split into face + message strip).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::text_input::{TextInput, ValidationState};
    ///
    /// let input = TextInput::new("Email")
    ///     .validation(ValidationState::Error)
    ///     .validation_message("address required");
    /// assert_eq!(input.validation_message.as_deref(), Some("address required"));
    /// ```
    #[inline]
    #[must_use]
    pub fn validation_message(mut self, message: impl Into<String>) -> Self {
        self.validation_message = Some(message.into());
        self
    }

    /// `true` when a user-driven edit mutated `value` since the last
    /// call — the widget's change out-seam (mirrors
    /// `Banner::take_dismissed`). Fires on typing, deletion, paste,
    /// cut, the ✕ clear, and undo/redo; programmatic
    /// [`set_value`](Self::set_value) writes do not set it.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TextInput;
    ///
    /// let mut input = TextInput::new("Name");
    /// assert!(!input.take_edited());
    /// ```
    #[inline]
    pub fn take_edited(&mut self) -> bool {
        std::mem::take(&mut self.edited)
    }

    /// Whether the `"Undo"` key currently has a snapshot to restore.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TextInput;
    ///
    /// let input = TextInput::new("Name");
    /// assert!(!input.can_undo());
    /// ```
    #[inline]
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    /// Whether an undone snapshot is waiting for `"Redo"`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TextInput;
    ///
    /// let input = TextInput::new("Name");
    /// assert!(!input.can_redo());
    /// ```
    #[inline]
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
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
        self.value = self.sanitize_insert(&value.into());
        // Programmatic writes collapse the caret to the end — a stale
        // byte offset could land mid-char after a shorter write.
        self.cursor = self.value.len();
        self.selection_anchor = None;
        // A programmatic write is not a user edit: it drops the undo
        // history (Qt's setText behaves the same) and does not set the
        // `take_edited` flag.
        self.undo.clear();
        self.redo.clear();
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
    /// Returns whether anything was deleted. Does NOT record undo —
    /// the caller's enclosing edit records one snapshot for the whole
    /// mutation.
    fn delete_selection(&mut self) -> bool {
        let Some((lo, hi)) = self.selection() else {
            return false;
        };
        self.value.drain(lo..hi);
        self.cursor = lo;
        self.selection_anchor = None;
        true
    }

    /// Pushes the current `(value, caret, anchor)` onto the bounded
    /// undo stack and clears the redo branch — every user-driven
    /// mutation routes through here so one logical edit is one undo
    /// step.
    fn record_undo(&mut self) {
        self.undo.push_back(EditSnapshot {
            value: self.value.clone(),
            cursor: self.cursor,
            anchor: self.selection_anchor,
        });
        if self.undo.len() > UNDO_LIMIT {
            self.undo.pop_front();
        }
        self.redo.clear();
        self.edited = true;
    }

    /// Restores the newest undo snapshot, pushing the current state
    /// onto the redo stack. Returns whether anything was undone.
    fn undo_edit(&mut self) -> bool {
        let Some(snap) = self.undo.pop_back() else {
            return false;
        };
        self.redo.push_back(EditSnapshot {
            value: std::mem::replace(&mut self.value, snap.value),
            cursor: std::mem::replace(&mut self.cursor, snap.cursor),
            anchor: std::mem::replace(&mut self.selection_anchor, snap.anchor),
        });
        self.edited = true;
        true
    }

    /// Re-applies the newest undone snapshot. Returns whether
    /// anything was redone.
    fn redo_edit(&mut self) -> bool {
        let Some(snap) = self.redo.pop_back() else {
            return false;
        };
        self.undo.push_back(EditSnapshot {
            value: std::mem::replace(&mut self.value, snap.value),
            cursor: std::mem::replace(&mut self.cursor, snap.cursor),
            anchor: std::mem::replace(&mut self.selection_anchor, snap.anchor),
        });
        self.edited = true;
        true
    }

    /// Empties the field — the ✕ clear target's action. Records the
    /// edit so `"Undo"` restores the cleared value.
    fn clear_value(&mut self) {
        if self.value.is_empty() {
            return;
        }
        self.record_undo();
        self.value.clear();
        self.cursor = 0;
        self.selection_anchor = None;
    }

    /// Inserts `text` at the caret, replacing any selection. The
    /// configured [`SanitizerConfig`] filters it first — at minimum
    /// newlines, which a single-line field never stores. One undo
    /// step covers the replace-and-insert.
    fn insert_str(&mut self, text: &str) {
        let clean = self.sanitize_insert(text);
        // No-op commits (an empty IME string with nothing selected)
        // don't earn an undo step or the edited flag.
        if clean.is_empty() && self.selection().is_none() {
            return;
        }
        self.record_undo();
        self.delete_selection();
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
        self.set_caret(prev_grapheme_boundary(&self.value, self.cursor), extend);
    }

    fn caret_right(&mut self, extend: bool) {
        if !extend {
            if let Some((_, hi)) = self.selection() {
                self.selection_anchor = None;
                self.cursor = hi;
                return;
            }
        }
        self.set_caret(next_grapheme_boundary(&self.value, self.cursor), extend);
    }

    /// The nearest word-jump boundary strictly left of `pos` —
    /// Ctrl/Cmd+ArrowLeft's target.
    fn word_left(&self, pos: usize) -> usize {
        word_edges(&self.value)
            .into_iter()
            .rev()
            .find(|&e| e < pos)
            .unwrap_or(0)
    }

    /// The nearest word-jump boundary strictly right of `pos` —
    /// Ctrl/Cmd+ArrowRight's target.
    fn word_right(&self, pos: usize) -> usize {
        word_edges(&self.value)
            .into_iter()
            .find(|&e| e > pos)
            .unwrap_or(self.value.len())
    }

    /// Ctrl/Cmd+ArrowLeft — jumps to the previous word edge rather
    /// than collapsing the selection (platform word-jump semantics
    /// don't collapse first; the whole selection moves).
    fn caret_word_left(&mut self, extend: bool) {
        self.set_caret(self.word_left(self.cursor), extend);
    }

    /// Ctrl/Cmd+ArrowRight — jumps to the next word edge.
    fn caret_word_right(&mut self, extend: bool) {
        self.set_caret(self.word_right(self.cursor), extend);
    }

    /// Ctrl/Cmd+Backspace — deletes the selection, else the run from
    /// the previous word edge to the caret.
    fn delete_word_back(&mut self) {
        if self.selection().is_some() {
            self.record_undo();
            self.delete_selection();
            return;
        }
        let start = self.word_left(self.cursor);
        if start < self.cursor {
            self.record_undo();
            self.value.drain(start..self.cursor);
            self.cursor = start;
        }
    }

    /// Ctrl/Cmd+Delete — deletes the selection, else the run from
    /// the caret to the next word edge.
    fn delete_word_forward(&mut self) {
        if self.selection().is_some() {
            self.record_undo();
            self.delete_selection();
            return;
        }
        let end = self.word_right(self.cursor);
        if end > self.cursor {
            self.record_undo();
            self.value.drain(self.cursor..end);
        }
    }

    fn backspace(&mut self) {
        // Only record when a byte will actually move — Backspace at
        // offset 0 with no selection must not mint an undo step.
        if self.selection().is_some() || self.cursor > 0 {
            self.record_undo();
        }
        if self.delete_selection() {
            return;
        }
        let start = prev_grapheme_boundary(&self.value, self.cursor);
        if start < self.cursor {
            self.value.drain(start..self.cursor);
            self.cursor = start;
        }
    }

    fn delete_forward(&mut self) {
        if self.selection().is_some() || self.cursor < self.value.len() {
            self.record_undo();
        }
        if self.delete_selection() {
            return;
        }
        let end = next_grapheme_boundary(&self.value, self.cursor);
        if end > self.cursor {
            self.value.drain(self.cursor..end);
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

    /// `true` while the painted run is the bullet mask — `secure`
    /// and not currently revealed.
    fn masked(&self) -> bool {
        self.secure && !self.revealed
    }

    /// The `•`-run a masked field paints — one bullet per grapheme of
    /// `value`, matching platform password fields' uniform echo.
    fn mask_text(&self) -> String {
        BULLET
            .to_string()
            .repeat(self.value.graphemes(true).count())
    }

    /// Byte offset into the masked display run matching real byte
    /// `b` — every grapheme before `b` is one `BULLET` (`BULLET_LEN`
    /// bytes).
    fn display_byte(&self, b: usize) -> usize {
        self.value[..b.min(self.value.len())]
            .graphemes(true)
            .count()
            * BULLET_LEN
    }

    /// The inverse of [`display_byte`](Self::display_byte): a byte
    /// offset into the painted run maps back to the real byte offset
    /// at that grapheme boundary. Identity in plain mode.
    fn real_byte(&self, display: usize) -> usize {
        if !self.masked() {
            return display.min(self.value.len());
        }
        self.value
            .grapheme_indices(true)
            .nth(display / BULLET_LEN)
            .map_or(self.value.len(), |(i, _)| i)
    }

    /// Whether the reveal affordance occupies the right edge.
    fn show_reveal(&self) -> bool {
        self.secure && self.revealable
    }

    /// Whether the ✕ clear target occupies the right edge.
    fn show_clear(&self) -> bool {
        self.clearable && !self.read_only && !self.value.is_empty()
    }

    /// Width of the right-edge affordance column in device px —
    /// `ZONE_PT` per active target, stacked reveal-outermost.
    fn zones_width(&self, scale: f32) -> f32 {
        (usize::from(self.show_reveal()) + usize::from(self.show_clear())) as f32 * ZONE_PT * scale
    }

    /// The message strip's height inside `b` in device px — `0`
    /// unless a `validation_message` is set and the bounds leave a
    /// full `MSG_STRIP_PT` under a one-line face. Exactly-one-line
    /// bounds show no message (documented on `validation_message`).
    fn message_h(&self, b: Rect, scale: f32) -> f32 {
        let fits = b.size.y >= (LINE_PT + MSG_STRIP_PT) * scale;
        if self.validation_message.is_some() && fits {
            MSG_STRIP_PT * scale
        } else {
            0.0
        }
    }

    /// The field face's height inside `b` — the bounds minus whatever
    /// message strip shows.
    fn face_height(&self, b: Rect, scale: f32) -> f32 {
        b.size.y - self.message_h(b, scale)
    }

    /// The reveal-toggle's hit zone — the rightmost `ZONE_PT` strip
    /// of the face, `None` unless a secure+revealable field shows it.
    fn reveal_zone(&self, b: Rect, scale: f32) -> Option<Rect> {
        self.show_reveal().then(|| {
            let w = ZONE_PT * scale;
            Rect::new(b.max_x() - w, b.origin.y, w, self.face_height(b, scale))
        })
    }

    /// The ✕ target's hit zone — the `ZONE_PT` strip just left of the
    /// reveal zone, `None` unless `clearable` and the field holds a
    /// clearable value.
    fn clear_zone(&self, b: Rect, scale: f32) -> Option<Rect> {
        self.show_clear().then(|| {
            let w = ZONE_PT * scale;
            let reveal_w = if self.show_reveal() { w } else { 0.0 };
            Rect::new(
                b.max_x() - w - reveal_w,
                b.origin.y,
                w,
                self.face_height(b, scale),
            )
        })
    }

    /// Byte offset of the insertion boundary nearest device-pixel `x`
    /// in window space. Shapes through the widget's painter so clicks
    /// land exactly where the glyphs are — the painter is created
    /// lazily so inputs that are never clicked skip the font scan.
    /// Hit-testing runs on the painted run (bullets while masked), so
    /// clicks land on the same cells `paint` fills, then maps back to
    /// real byte offsets.
    fn byte_at_position(&mut self, bounds: Rect, x: f32) -> usize {
        let masked = self.masked();
        let mask;
        let text: &str = if masked {
            mask = self.mask_text();
            &mask
        } else {
            &self.value
        };
        let font_px = FONT_PT * self.scale;
        let scroll_x = self.scroll_x();
        let painter = self
            .text_painter
            .get_or_insert_with(crate::text_paint::shared_painter);
        // `scroll_x` shifts the painted run left — add it back so a
        // click lands on the glyph under the pointer. The prefix
        // shifts the run's origin right.
        let prefix_w = self
            .prefix
            .as_deref()
            .map_or(0.0, |s| painter.measure(s, font_px));
        let text_x = bounds.origin.x + TEXT_PAD_X * self.scale + prefix_w - scroll_x;
        let display = painter.byte_at(text, font_px, x - text_x);
        self.real_byte(display)
    }

    /// Device-pixel offset of the caret boundary at `byte_idx`, from
    /// the same painter that emits the glyphs. Falls back to the
    /// per-char estimate only when no shaped painter exists at all
    /// (placeholder `DrawText` path, which is itself approximate).
    /// Metrics run on the painted run — bullets while masked — so
    /// the caret and selection band sit where the glyphs are.
    fn offset_x(&self, cx: &PaintContext, byte_idx: usize, font_px: f32) -> f32 {
        let masked = self.masked();
        let mask;
        let text: &str = if masked {
            mask = self.mask_text();
            &mask
        } else {
            &self.value
        };
        let idx = if masked {
            self.display_byte(byte_idx)
        } else {
            byte_idx.min(text.len())
        };
        if let Some(p) = &self.text_painter {
            return p.caret_x(text, font_px, idx);
        }
        if let Some(p) = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter) {
            if let Some(w) = p.measure_text(&text[..idx], font_px) {
                return w;
            }
        }
        text[..idx].chars().count() as f32 * cx.pt(7.0)
    }

    /// Advance width of an adornment string in device px through the
    /// resolved painter, or the per-char estimate when shaping is
    /// absent (the same fallback `offset_x` documents).
    fn adorn_width(&self, cx: &PaintContext, s: &str, font_px: f32) -> f32 {
        crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter)
            .and_then(|p| p.measure_text(s, font_px))
            .unwrap_or_else(|| s.chars().count() as f32 * cx.pt(7.0))
    }

    /// Keyboard handling for the focused input. `key` is the logical
    /// key name — plain characters arrive via `ImeCommitted`, while
    /// chords like Cmd+A arrive as the synthetic names
    /// `"SelectAll"`/`"Cut"`/`"Copy"`/`"Paste"` that the window layer
    /// dispatches (see the example's chord synthesis) or as
    /// `+`-joined chord names (`"Ctrl+ArrowLeft"`). Bare modifier
    /// keypresses are tracked into `shift_held`/`word_mod_held`
    /// because `KeyPressed` carries no modifier state (F17).
    fn key_pressed(&mut self, key: &str) -> EventResponse {
        let (wm_chord, sh_chord, base) = parse_key_chord(key);
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
                    self.caret_word_left(extend);
                } else {
                    self.caret_left(extend);
                }
                EventResponse::RequestRepaint
            }
            "ArrowRight" => {
                if word {
                    self.caret_word_right(extend);
                } else {
                    self.caret_right(extend);
                }
                EventResponse::RequestRepaint
            }
            "Home" => {
                self.set_caret(0, extend);
                EventResponse::RequestRepaint
            }
            "End" => {
                self.set_caret(self.value.len(), extend);
                EventResponse::RequestRepaint
            }
            "SelectAll" => {
                self.selection_anchor = Some(0);
                self.cursor = self.value.len();
                EventResponse::RequestRepaint
            }
            "a" | "A" if word => {
                self.selection_anchor = Some(0);
                self.cursor = self.value.len();
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
                if self.selection().is_some() {
                    self.copy_selection();
                    self.record_undo();
                    self.delete_selection();
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
            "Undo" if !self.read_only => {
                if self.undo_edit() {
                    EventResponse::RequestRepaint
                } else {
                    EventResponse::Handled
                }
            }
            "Redo" if !self.read_only => {
                if self.redo_edit() {
                    EventResponse::RequestRepaint
                } else {
                    EventResponse::Handled
                }
            }
            "z" | "Z" if word && !self.read_only => {
                let did = if extend {
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
            "y" | "Y" if word && !self.read_only => {
                if self.redo_edit() {
                    EventResponse::RequestRepaint
                } else {
                    EventResponse::Handled
                }
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
    #[cfg(feature = "devtools-timemachine")]
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }

    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        // A text input has a default minimum size of 120x24 logical pt;
        // a `validation_message` asks for the message strip's height on
        // top so the layout leaves the field room to show it.
        let min_w = cx.pt(120.0).min(constraints.max_size.x.max(0.0));
        let want_h = if self.validation_message.is_some() {
            LINE_PT + MSG_STRIP_PT
        } else {
            LINE_PT
        };
        let min_h = cx.pt(want_h).min(constraints.max_size.y.max(0.0));
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

    /// `@labeled` declares the accessible name to design-lint's
    /// `icon-only-control` rule — the paint list can't see the
    /// AccessKit label, so the scope marker carries it.
    fn debug_name(&self) -> &'static str {
        if self.label.is_empty() {
            "TextInput"
        } else {
            "TextInput@labeled"
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
        if matches!(self.validation, Some(ValidationState::Error)) {
            node.set_invalid(accesskit::Invalid::True);
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
            // Editing events belong to the focused field only. An
            // unfocused input must not claim keys — the forwarding
            // helper would otherwise deliver them to the first
            // claiming child regardless of where the user clicked.
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
                // Internal children never receive the arena's
                // `FocusGained` reliably — a claimed primary press IS
                // the focus signal. Sibling fields drop theirs via the
                // `FocusLost` the forwarding helper broadcasts.
                self.focused = true;
                // Right-edge affordance zones win over caret
                // placement — a press on the eye or ✕ is not a text
                // gesture and must not move the caret or open a drag.
                if *count == 1 {
                    if let Some(zone) = self.reveal_zone(cx.bounds, self.scale) {
                        if zone.contains(*position) {
                            self.revealed = !self.revealed;
                            return EventResponse::RequestRepaint;
                        }
                    }
                    if let Some(zone) = self.clear_zone(cx.bounds, self.scale) {
                        if zone.contains(*position) {
                            self.clear_value();
                            return EventResponse::RequestRepaint;
                        }
                    }
                }
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
                // don't leak a stuck Shift, word-modifier, or drag into
                // the next focus.
                self.shift_held = false;
                self.word_mod_held = false;
                self.dragging = false;
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
                self.preedit = None;
                self.insert_str(text);
                EventResponse::RequestRepaint
            }
            WidgetEvent::KeyPressed { key, .. } => self.key_pressed(key),
            WidgetEvent::KeyReleased { key } => match key.as_str() {
                "Shift" => {
                    self.shift_held = false;
                    EventResponse::Handled
                }
                "Control" | "Ctrl" | "Meta" | "Cmd" | "Super" | "Alt" | "Option" => {
                    self.word_mod_held = false;
                    EventResponse::Handled
                }
                _ => EventResponse::Ignored,
            },
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        let scale = cx.scale;
        // Taller-than-one-line bounds split into the field face plus a
        // `validation_message` strip below it — every face-internal
        // metric (text centreline, selection band, caret) uses `face_h`,
        // never the full bounds height.
        let face_h = self.face_height(b, scale);
        let face_bottom = b.origin.y + face_h;
        let rect = kurbo::Rect::new(
            f64::from(b.min_x()),
            f64::from(b.min_y()),
            f64::from(b.max_x()),
            f64::from(face_bottom),
        );
        let face = Shape::rounded(cx.dim(TokenKey::BorderRadiusSmall, 3.0));
        cx.list
            .push_fill_shape(rect, &face, cx.color(TokenKey::SurfaceColor, FACE));
        let edge = match self.validation {
            Some(ValidationState::Error) => cx.color(TokenKey::ErrorColor, EDGE_ERROR),
            Some(ValidationState::Warning) => cx.color(TokenKey::WarningColor, EDGE_WARNING),
            Some(ValidationState::Valid) => cx.color(TokenKey::SuccessColor, EDGE_VALID),
            None if self.focused => cx.color(TokenKey::AccentColor, EDGE_FOCUSED),
            None => cx.color(TokenKey::BorderColor, EDGE),
        };
        cx.list.push_stroke_shape(rect, &face, cx.pt(1.0), edge);

        // Vertically centre the font's line box within the face — the
        // painter treats the origin as the block top, not a baseline.
        let font_px = cx.pt(FONT_PT);
        let pad = cx.pt(TEXT_PAD_X);
        let text_y = crate::text_paint::vcenter_origin_y(
            kurbo::Rect::new(
                f64::from(b.origin.x),
                f64::from(b.origin.y),
                f64::from(b.origin.x + b.size.x),
                f64::from(b.origin.y + face_h),
            ),
            font_px,
        );

        // Interior layout: [pad][prefix][text run …][suffix][zones].
        // The suffix pins to the right edge of the text lane; the
        // reveal/clear affordance zones occupy the far-right column.
        let zones_w = self.zones_width(scale);
        let prefix_w = self
            .prefix
            .as_deref()
            .map_or(0.0, |s| self.adorn_width(cx, s, font_px));
        let suffix_w = self
            .suffix
            .as_deref()
            .map_or(0.0, |s| self.adorn_width(cx, s, font_px));
        let text_x = b.origin.x + pad + prefix_w;

        // Caret-following horizontal scroll — when the value is wider
        // than the text lane the run slides left so the caret stays
        // inside the field (standard text-field behavior). Painted
        // positions below all shift by `scroll`.
        let caret_off = self.offset_x(cx, self.cursor, font_px);
        let inner_w = (b.size.x - 2.0 * pad - prefix_w - suffix_w - zones_w).max(0.0);
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
        // text run, selection band, adornments, and caret can never
        // spill past the field edge.
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

        let muted = cx.color(TokenKey::TextMutedColor, INK_PLACEHOLDER);

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
                        f64::from(face_bottom) - cx.ptf(3.0),
                    ),
                    [accent[0], accent[1], accent[2], 96],
                );
            }
        }

        if let Some(prefix) = &self.prefix {
            crate::text_paint::paint_label(
                crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter),
                cx.list,
                kurbo::Point::new(f64::from(b.origin.x + pad), text_y),
                prefix,
                font_px,
                muted,
            );
        }

        let display;
        let run: &str = if self.masked() {
            display = self.mask_text();
            &display
        } else {
            &self.value
        };
        if self.value.is_empty() {
            crate::text_paint::paint_label(
                crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter),
                cx.list,
                kurbo::Point::new(f64::from(text_x), text_y),
                &self.placeholder,
                font_px,
                cx.color(TokenKey::TextMutedColor, INK_PLACEHOLDER),
            );
        } else {
            crate::text_paint::paint_label(
                crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter),
                cx.list,
                kurbo::Point::new(f64::from(text_x), text_y),
                run,
                font_px,
                cx.color(TokenKey::TextColor, INK),
            );
        }

        if let Some(suffix) = &self.suffix {
            let sx = b.max_x() - zones_w - pad - suffix_w;
            crate::text_paint::paint_label(
                crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter),
                cx.list,
                kurbo::Point::new(f64::from(sx), text_y),
                suffix,
                font_px,
                muted,
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
            caret.line_to((caret_x, f64::from(face_bottom) - cx.ptf(4.0)));
            cx.list
                .push_stroke_path(caret, cx.pt(1.0), cx.color(TokenKey::TextColor, CARET));
        }

        // IME preedit — the in-progress composition paints underlined
        // at the insertion caret (standard IME presentation); it is not
        // part of `value` until committed.
        if self.focused {
            if let Some((pre, _)) = &self.preedit {
                let painter =
                    crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
                let px = text_x + self.offset_x(cx, self.cursor, font_px);
                let ink = cx.color(TokenKey::TextColor, INK);
                crate::text_paint::paint_label(
                    painter,
                    cx.list,
                    kurbo::Point::new(f64::from(px), text_y),
                    pre,
                    font_px,
                    ink,
                );
                let w = painter
                    .and_then(|p| p.measure_text(pre, font_px))
                    .unwrap_or_else(|| pre.chars().count() as f32 * cx.pt(7.0));
                let uy = f64::from(face_bottom) - cx.ptf(3.0);
                let mut u = kurbo::BezPath::new();
                u.move_to((f64::from(px), uy));
                u.line_to((f64::from(px + w), uy));
                cx.list.push_stroke_path(u, cx.pt(1.0), ink);
            }
        }

        // Right-edge affordances — the eye toggles masked ↔ plain on
        // secure fields; the ✕ clears the value. Painted inside the
        // face clip so narrow bounds crop them cleanly.
        let mid_y = f64::from(b.origin.y + face_h / 2.0);
        if let Some(zone) = self.reveal_zone(b, scale) {
            let cxp = f64::from(zone.origin.x + zone.size.x / 2.0);
            let rx = cx.ptf(5.5);
            let ry = cx.ptf(3.2);
            let lens = kurbo::Rect::new(cxp - rx, mid_y - ry, cxp + rx, mid_y + ry);
            cx.list
                .push_stroke_shape(lens, &Shape::ELLIPSE, cx.pt(1.1), muted);
            let pr = cx.ptf(1.5);
            let pupil = kurbo::Rect::new(cxp - pr, mid_y - pr, cxp + pr, mid_y + pr);
            cx.list.push_fill_shape(pupil, &Shape::ELLIPSE, muted);
            if self.masked() {
                // Slashed pupil — the password is hidden; the affordance
                // offers to reveal it.
                let mut slash = kurbo::BezPath::new();
                slash.move_to((cxp - rx * 0.7, mid_y + ry * 0.7));
                slash.line_to((cxp + rx * 0.7, mid_y - ry * 0.7));
                cx.list.push_stroke_path(slash, cx.pt(1.1), muted);
            }
        }
        if let Some(zone) = self.clear_zone(b, scale) {
            let cxp = f64::from(zone.origin.x + zone.size.x / 2.0);
            let r = cx.ptf(3.5);
            let mut mark = kurbo::BezPath::new();
            mark.move_to((cxp - r, mid_y - r));
            mark.line_to((cxp + r, mid_y + r));
            mark.move_to((cxp + r, mid_y - r));
            mark.line_to((cxp - r, mid_y + r));
            cx.list.push_stroke_path(mark, cx.pt(1.3), muted);
        }
        cx.list.pop_clip();

        // Validation message strip under the face — only painted when
        // the bounds split left it room (see `message_h`).
        let msg_h = self.message_h(b, scale);
        if msg_h > 0.0 {
            if let Some(msg) = &self.validation_message {
                let color = match self.validation {
                    Some(ValidationState::Error) => cx.color(TokenKey::ErrorColor, EDGE_ERROR),
                    Some(ValidationState::Warning) => {
                        cx.color(TokenKey::WarningColor, EDGE_WARNING)
                    }
                    Some(ValidationState::Valid) => cx.color(TokenKey::SuccessColor, EDGE_VALID),
                    None => muted,
                };
                let msg_px = cx.pt(MSG_FONT_PT);
                crate::text_paint::paint_label_vcenter(
                    crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter),
                    cx.list,
                    kurbo::Rect::new(
                        f64::from(b.min_x()),
                        f64::from(face_bottom),
                        f64::from(b.max_x()),
                        f64::from(face_bottom + (msg_h)),
                    ),
                    f64::from(b.origin.x + pad),
                    msg,
                    msg_px,
                    color,
                );
            }
        }
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
        focus(&mut input);
        input.set_value("John");
        assert_eq!(input.value, "John");
    }

    #[test]
    fn text_input_measure_returns_size() {
        let mut hot = HotNode::default();
        let mut cx = make_cx(&mut hot);
        let mut input = TextInput::new("Test");
        focus(&mut input);
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
        focus(&mut input);
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

    fn preedit(text: &str, cursor: Option<(usize, usize)>) -> WidgetEvent {
        WidgetEvent::ImePreedit {
            text: text.to_string(),
            cursor,
        }
    }

    fn key(name: &str) -> WidgetEvent {
        WidgetEvent::KeyPressed {
            key: name.to_string(),
            repeat: false,
        }
    }

    /// Editing events only reach a focused field — tests simulate the
    /// arena's focus delivery the way production does.
    fn focus(input: &mut TextInput) {
        input.event(&mut ev(&WidgetEvent::FocusGained));
    }

    #[test]
    fn text_input_ime_inserts_at_caret() {
        let mut input = TextInput::new("F");
        focus(&mut input);
        input.event(&mut ev(&ime("abc")));
        input.event(&mut ev(&key("ArrowLeft")));
        input.event(&mut ev(&key("ArrowLeft")));
        input.event(&mut ev(&ime("X")));
        assert_eq!(input.value, "aXbc");
        assert_eq!(input.cursor, 2);
    }

    #[test]
    fn text_input_ime_preedit_lifecycle() {
        let mut input = TextInput::new("F");
        focus(&mut input);
        input.event(&mut ev(&preedit("nich", Some((0, 4)))));
        assert_eq!(input.preedit(), Some("nich"));
        assert_eq!(input.preedit_cursor(), Some((0, 4)));
        // Commit inserts the text and clears the composition.
        input.event(&mut ev(&ime("日本")));
        assert_eq!(input.preedit(), None);
        assert_eq!(input.value, "日本");
        // An empty preedit clears without touching the value.
        input.event(&mut ev(&preedit("x", None)));
        input.event(&mut ev(&preedit("", None)));
        assert_eq!(input.preedit(), None);
        assert_eq!(input.value, "日本");
    }

    #[test]
    fn text_input_preedit_cleared_on_focus_loss() {
        let mut input = TextInput::new("F");
        focus(&mut input);
        input.event(&mut ev(&preedit("wip", None)));
        assert_eq!(input.preedit(), Some("wip"));
        input.event(&mut ev(&WidgetEvent::FocusLost));
        assert_eq!(input.preedit(), None);
    }

    #[test]
    fn text_input_backspace_and_delete() {
        let mut input = TextInput::new("F").value("abc");
        focus(&mut input);
        input.event(&mut ev(&key("ArrowLeft")));
        input.event(&mut ev(&key("Backspace")));
        assert_eq!(input.value, "ac");
        input.event(&mut ev(&key("Home")));
        input.event(&mut ev(&key("Delete")));
        assert_eq!(input.value, "c");
    }

    #[test]
    fn text_input_backspace_deletes_whole_grapheme() {
        // "e\u{301}" is e + combining acute — one user-perceived char.
        let mut input = TextInput::new("F").sanitize(false).value("e\u{301}x");
        focus(&mut input);
        input.event(&mut ev(&key("ArrowLeft")));
        input.event(&mut ev(&key("Backspace")));
        assert_eq!(input.value, "x");
        assert_eq!(input.cursor, 0);
        // A ZWJ family emoji is one cluster — Backspace removes it whole.
        let mut input = TextInput::new("F").value("\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}!");
        focus(&mut input);
        input.event(&mut ev(&key("ArrowLeft")));
        input.event(&mut ev(&key("Backspace")));
        assert_eq!(input.value, "!");
    }

    #[test]
    fn text_input_delete_forward_removes_whole_grapheme() {
        let mut input = TextInput::new("F").sanitize(false).value("e\u{301}x");
        focus(&mut input);
        input.event(&mut ev(&key("Home")));
        input.event(&mut ev(&key("Delete")));
        assert_eq!(input.value, "x");
    }

    #[test]
    fn text_input_arrows_step_graphemes() {
        // Two clusters, three scalar values: "é" (2 chars) + "x".
        let mut input = TextInput::new("F").sanitize(false).value("e\u{301}x");
        focus(&mut input);
        input.event(&mut ev(&key("Home")));
        input.event(&mut ev(&key("ArrowRight")));
        assert_eq!(input.cursor, "e\u{301}".len());
        input.event(&mut ev(&key("ArrowRight")));
        assert_eq!(input.cursor, input.value.len());
        input.event(&mut ev(&key("ArrowLeft")));
        assert_eq!(input.cursor, "e\u{301}".len());
    }

    #[test]
    fn text_input_grapheme_delete_undoes_whole() {
        let mut input = TextInput::new("F").sanitize(false).value("e\u{301}");
        focus(&mut input);
        input.event(&mut ev(&key("Backspace")));
        assert_eq!(input.value, "");
        input.event(&mut ev(&key("Ctrl+Z")));
        assert_eq!(input.value, "e\u{301}");
    }

    #[test]
    fn text_input_unfocused_ignores_editing_keys() {
        let mut input = TextInput::new("F").value("abc");
        // No FocusGained/click — keys and IME must not edit.
        assert_eq!(
            input.event(&mut ev(&key("Backspace"))),
            EventResponse::Ignored
        );
        assert_eq!(input.event(&mut ev(&ime("z"))), EventResponse::Ignored);
        assert_eq!(input.value, "abc");
    }

    #[test]
    fn text_input_disabled_ignores_editing_keys() {
        let mut input = TextInput::new("F").value("abc").enabled(false);
        focus(&mut input);
        assert_eq!(
            input.event(&mut ev(&key("Backspace"))),
            EventResponse::Ignored
        );
        assert_eq!(input.event(&mut ev(&ime("z"))), EventResponse::Ignored);
        assert_eq!(input.value, "abc");
    }

    #[test]
    fn text_input_focus_lost_stops_editing() {
        let mut input = TextInput::new("F").value("abc");
        focus(&mut input);
        input.event(&mut ev(&ime("z")));
        assert_eq!(input.value, "abcz");
        input.event(&mut ev(&WidgetEvent::FocusLost));
        input.event(&mut ev(&key("Backspace")));
        assert_eq!(input.value, "abcz");
    }

    #[test]
    fn text_input_select_all_then_type_replaces() {
        let mut input = TextInput::new("F").value("abc");
        focus(&mut input);
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
        focus(&mut input);
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
        focus(&mut input);
        let x = click_x(&mut input, 8); // inside "beta"
        assert_eq!(press(&mut input, x, 2), EventResponse::CapturePointer);
        assert_eq!(input.selected_text(), Some("beta"));
    }

    #[test]
    fn text_input_triple_click_selects_all() {
        let mut input = TextInput::new("F").value("alpha beta gamma");
        focus(&mut input);
        let x = click_x(&mut input, 8);
        press(&mut input, x, 3);
        assert_eq!(input.selection(), Some((0, "alpha beta gamma".len())));
    }

    #[test]
    fn text_input_double_click_drag_extends_by_word() {
        let mut input = TextInput::new("F").value("alpha beta gamma");
        focus(&mut input);
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
        focus(&mut input);
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
        focus(&mut input);
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
        focus(&mut input);
        input.event(&mut ev(&key("SelectAll")));
        input.event(&mut ev(&key("Backspace")));
        assert_eq!(input.value, "");
        assert_eq!(input.cursor, 0);
    }

    #[test]
    fn text_input_escape_clears_selection() {
        let mut input = TextInput::new("F").value("abc");
        focus(&mut input);
        input.event(&mut ev(&key("SelectAll")));
        input.event(&mut ev(&key("Escape")));
        assert_eq!(input.selection(), None);
        assert_eq!(input.cursor, 3);
    }

    #[test]
    fn text_input_home_end() {
        let mut input = TextInput::new("F").value("abc");
        focus(&mut input);
        input.event(&mut ev(&key("Home")));
        assert_eq!(input.cursor, 0);
        input.event(&mut ev(&key("End")));
        assert_eq!(input.cursor, 3);
    }

    #[test]
    fn text_input_set_value_resets_caret() {
        let mut input = TextInput::new("F").value("abcdef");
        focus(&mut input);
        input.event(&mut ev(&key("Home")));
        input.set_value("xy");
        assert_eq!(input.cursor, 2);
        assert_eq!(input.selection(), None);
    }

    #[test]
    fn text_input_read_only_blocks_edits() {
        let mut input = TextInput::new("F").value("abc").read_only(true);
        focus(&mut input);
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
        focus(&mut input);
        input.event(&mut ev(&ime("a\nb\rc")));
        assert_eq!(input.value, "abc");
        assert_eq!(input.cursor, 3);
    }

    #[test]
    fn text_input_hit_test_accounts_for_scroll() {
        let mut hot = HotNode::default();
        let mut cx = make_cx(&mut hot);
        let mut input = TextInput::new("F").value("alpha beta gamma delta epsilon");
        focus(&mut input);
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

    #[test]
    fn text_input_undo_redo_restores_edits() {
        let mut input = TextInput::new("F");
        focus(&mut input);
        input.event(&mut ev(&ime("abc")));
        input.event(&mut ev(&ime("d")));
        assert_eq!(input.value, "abcd");
        assert!(input.can_undo());
        input.event(&mut ev(&key("Ctrl+Z")));
        assert_eq!(input.value, "abc");
        input.event(&mut ev(&key("Ctrl+Z")));
        assert_eq!(input.value, "");
        assert!(!input.can_undo());
        input.event(&mut ev(&key("Ctrl+Shift+Z")));
        assert_eq!(input.value, "abc");
        // A fresh edit clears the redo branch.
        input.event(&mut ev(&key("Ctrl+Z")));
        input.event(&mut ev(&ime("x")));
        assert_eq!(input.value, "x");
        assert!(!input.can_redo());
        // Undo restores the caret context, not just the text.
        input.event(&mut ev(&key("Undo")));
        assert_eq!(input.value, "");
        input.event(&mut ev(&key("Redo")));
        assert_eq!(input.value, "x");
        assert_eq!(input.cursor, 1);
    }

    #[test]
    fn text_input_word_jump_chorded_and_tracked() {
        let mut input = TextInput::new("F").value("alpha beta gamma");
        focus(&mut input);
        // Chorded name — Ctrl+ArrowLeft lands on the previous UAX#29
        // boundary (start of "gamma").
        input.event(&mut ev(&key("Ctrl+ArrowLeft")));
        assert_eq!(input.cursor, 11);
        // The live-tracked modifier path produces the same jump, and
        // releasing it restores single-char movement.
        input.event(&mut ev(&key("Control")));
        input.event(&mut ev(&key("ArrowLeft")));
        assert_eq!(input.cursor, 10);
        input.event(&mut ev(&WidgetEvent::KeyReleased {
            key: "Control".to_string(),
        }));
        input.event(&mut ev(&key("ArrowLeft")));
        assert_eq!(input.cursor, 9);
    }

    #[test]
    fn text_input_word_jump_extends_selection() {
        let mut input = TextInput::new("F").value("alpha beta gamma");
        focus(&mut input);
        input.event(&mut ev(&key("Ctrl+Shift+ArrowLeft")));
        assert_eq!(input.selection(), Some((11, 16)));
    }

    #[test]
    fn text_input_ctrl_backspace_deletes_word() {
        let mut input = TextInput::new("F").value("alpha beta");
        focus(&mut input);
        input.event(&mut ev(&key("Ctrl+Backspace")));
        assert_eq!(input.value, "alpha ");
        assert_eq!(input.cursor, 6);
        // Undoable in one step.
        input.event(&mut ev(&key("Ctrl+Z")));
        assert_eq!(input.value, "alpha beta");
    }

    #[test]
    fn text_input_masked_display_maps_real_bytes() {
        let input = TextInput::new("P").value("hello").secure(true);
        assert_eq!(input.mask_text(), "•••••");
        assert_eq!(input.display_byte(2), 2 * BULLET_LEN);
        assert_eq!(input.real_byte(2 * BULLET_LEN), 2);
    }

    #[test]
    fn text_input_reveal_zone_toggles_masked() {
        let mut input = TextInput::new("P")
            .value("secret")
            .secure(true)
            .revealable(true);
        focus(&mut input);
        assert!(input.masked());
        // Bounds are (0,0,200,24) at scale 1 — the reveal zone is the
        // rightmost ZONE_pt strip; a press there does not capture.
        assert_eq!(press(&mut input, 190.0, 1), EventResponse::RequestRepaint);
        assert!(input.is_revealed());
        assert!(!input.masked());
        press(&mut input, 190.0, 1);
        assert!(!input.is_revealed());
    }

    #[test]
    fn text_input_clear_zone_clears_and_undoes() {
        let mut input = TextInput::new("S").value("query").clearable(true);
        focus(&mut input);
        assert_eq!(press(&mut input, 190.0, 1), EventResponse::RequestRepaint);
        assert!(input.value.is_empty());
        assert!(input.take_edited());
        assert!(input.can_undo());
        input.event(&mut ev(&key("Ctrl+Z")));
        assert_eq!(input.value, "query");
    }

    #[test]
    fn text_input_zone_press_does_not_move_caret() {
        let mut input = TextInput::new("S").value("query").clearable(true);
        focus(&mut input);
        input.event(&mut ev(&key("Home")));
        // A clear-zone press is consumed by the zone, not the text —
        // the caret must not warp to the click position.
        press(&mut input, 190.0, 1);
        assert_eq!(input.cursor, 0);
    }

    #[test]
    fn text_input_shift_home_end_extend_selection() {
        let mut input = TextInput::new("F").value("abc");
        focus(&mut input);
        // Chorded Shift+Home selects from the caret to the line start.
        input.event(&mut ev(&key("Shift+Home")));
        assert_eq!(input.selection(), Some((0, 3)));
        input.event(&mut ev(&key("End"))); // collapse at end
                                           // Tracked-modifier path produces the same extension.
        input.event(&mut ev(&key("Shift")));
        input.event(&mut ev(&key("Home")));
        assert_eq!(input.selection(), Some((0, 3)));
        // Shift+End extends from a collapsed caret back to the end.
        input.event(&mut ev(&WidgetEvent::KeyReleased {
            key: "Shift".to_string(),
        }));
        input.event(&mut ev(&key("Home")));
        input.event(&mut ev(&key("Shift+End")));
        assert_eq!(input.selection(), Some((0, 3)));
    }

    #[test]
    fn text_input_home_end_collapse_open_selection() {
        let mut input = TextInput::new("F").value("abc");
        focus(&mut input);
        input.event(&mut ev(&key("SelectAll")));
        input.event(&mut ev(&key("Home")));
        assert_eq!(input.selection(), None);
        assert_eq!(input.cursor, 0);
        input.event(&mut ev(&key("SelectAll")));
        input.event(&mut ev(&key("End")));
        assert_eq!(input.selection(), None);
        assert_eq!(input.cursor, 3);
    }

    #[test]
    fn text_input_word_delete_at_boundaries_is_noop() {
        let mut input = TextInput::new("F").value("alpha beta");
        focus(&mut input);
        // Caret at end — Ctrl+Delete has nothing to eat and must not
        // mint an undo step.
        input.event(&mut ev(&key("Ctrl+Delete")));
        assert_eq!(input.value, "alpha beta");
        assert!(!input.can_undo());
        // Caret at start — Ctrl+Backspace likewise.
        input.event(&mut ev(&key("Home")));
        input.event(&mut ev(&key("Ctrl+Backspace")));
        assert_eq!(input.value, "alpha beta");
        assert!(!input.can_undo());
    }

    #[test]
    fn text_input_ctrl_delete_mid_word_eats_to_word_end() {
        let mut input = TextInput::new("F").value("alpha beta");
        focus(&mut input);
        input.event(&mut ev(&key("Home")));
        input.event(&mut ev(&key("ArrowRight"))); // inside "alpha"
        input.event(&mut ev(&key("Ctrl+Delete")));
        assert_eq!(input.value, "a beta");
        assert_eq!(input.cursor, 1);
    }

    #[test]
    fn text_input_backspace_at_start_mints_no_undo() {
        let mut input = TextInput::new("F").value("ab");
        focus(&mut input);
        input.event(&mut ev(&key("Home")));
        assert_eq!(
            input.event(&mut ev(&key("Backspace"))),
            EventResponse::RequestRepaint
        );
        assert_eq!(input.value, "ab");
        assert_eq!(input.cursor, 0);
        // A boundary no-op earns neither an undo step nor the edited
        // flag — undoing later must not replay a phantom edit.
        assert!(!input.can_undo());
        assert!(!input.take_edited());
    }

    #[test]
    fn text_input_delete_at_end_mints_no_undo() {
        let mut input = TextInput::new("F").value("ab");
        focus(&mut input);
        input.event(&mut ev(&key("Delete")));
        assert_eq!(input.value, "ab");
        assert!(!input.can_undo());
        assert!(!input.take_edited());
    }

    #[test]
    fn text_input_typing_replaces_mid_string_selection() {
        let mut input = TextInput::new("F").value("abcdef");
        focus(&mut input);
        input.event(&mut ev(&key("ArrowLeft")));
        input.event(&mut ev(&key("ArrowLeft")));
        input.event(&mut ev(&key("Shift+ArrowLeft")));
        input.event(&mut ev(&key("Shift+ArrowLeft")));
        assert_eq!(input.selected_text(), Some("cd"));
        input.event(&mut ev(&ime("XY")));
        assert_eq!(input.value, "abXYef");
        assert_eq!(input.cursor, 4);
        assert_eq!(input.selection(), None);
    }

    #[test]
    fn text_input_escape_twice_second_is_ignored() {
        let mut input = TextInput::new("F").value("abc");
        focus(&mut input);
        input.event(&mut ev(&key("SelectAll")));
        assert_eq!(
            input.event(&mut ev(&key("Escape"))),
            EventResponse::RequestRepaint
        );
        assert_eq!(input.selection(), None);
        // With nothing left to collapse, Escape bubbles up — a parent
        // dialog or overlay may still use it.
        assert_eq!(input.event(&mut ev(&key("Escape"))), EventResponse::Ignored);
        assert_eq!(input.value, "abc");
    }

    #[test]
    fn text_input_preedit_updates_and_commit_replaces() {
        let mut input = TextInput::new("F");
        focus(&mut input);
        input.event(&mut ev(&preedit("ni", Some((0, 2)))));
        assert_eq!(input.preedit(), Some("ni"));
        // A new preedit replaces the old one wholesale; nothing enters
        // the value until the commit arrives.
        input.event(&mut ev(&preedit("nihon", Some((0, 5)))));
        assert_eq!(input.preedit(), Some("nihon"));
        assert!(input.value.is_empty());
        input.event(&mut ev(&ime("日本")));
        assert_eq!(input.preedit(), None);
        assert_eq!(input.value, "日本");
    }

    #[test]
    fn text_input_empty_commit_clears_preedit_without_edit() {
        let mut input = TextInput::new("F").value("ab");
        focus(&mut input);
        input.event(&mut ev(&preedit("kan", None)));
        // A cancelled composition commits an empty string — the
        // document must not change and no undo step is earned.
        assert_eq!(
            input.event(&mut ev(&ime(""))),
            EventResponse::RequestRepaint
        );
        assert_eq!(input.preedit(), None);
        assert_eq!(input.value, "ab");
        assert!(!input.can_undo());
        assert!(!input.take_edited());
    }

    #[test]
    fn text_input_empty_commit_over_selection_removes_it() {
        // Wayland input-method-v2 semantics: placing a preedit over a
        // selection consumes the selection, so a cancelled composition
        // (empty commit) still removes it. Desktop toolkits differ —
        // GTK/macOS cancellations preserve the selection — but this is
        // the documented insert-replaces-selection path and a single
        // undo restores the removed range.
        let mut input = TextInput::new("F").value("abc");
        focus(&mut input);
        input.event(&mut ev(&key("SelectAll")));
        input.event(&mut ev(&ime("")));
        assert_eq!(input.value, "");
        input.event(&mut ev(&key("Ctrl+Z")));
        assert_eq!(input.value, "abc");
        assert_eq!(input.selection(), Some((0, 3)));
    }

    #[test]
    fn text_input_caret_never_splits_multibyte_char() {
        // 'é' (U+00E9) is a single 2-byte char — the caret must only
        // ever rest on char boundaries.
        let mut input = TextInput::new("F").value("a\u{e9}b");
        focus(&mut input);
        assert_eq!(input.cursor, 4);
        input.event(&mut ev(&key("ArrowLeft")));
        assert_eq!(input.cursor, 3);
        input.event(&mut ev(&key("ArrowLeft")));
        assert_eq!(input.cursor, 1); // stepped past 'é' whole
        assert!(input.value.is_char_boundary(input.cursor));
    }

    #[test]
    fn text_input_flag_emoji_is_one_grapheme() {
        // Two regional indicators form one flag cluster — motion and
        // deletion must never split it.
        let mut input = TextInput::new("F").value("\u{1f1fa}\u{1f1f8}x");
        focus(&mut input);
        input.event(&mut ev(&key("ArrowLeft"))); // past 'x'
        input.event(&mut ev(&key("ArrowLeft"))); // past the whole flag
        assert_eq!(input.cursor, 0);
        input.event(&mut ev(&key("ArrowLeft"))); // clamped at start
        assert_eq!(input.cursor, 0);
        input.event(&mut ev(&key("Delete")));
        assert_eq!(input.value, "x");
    }

    #[test]
    fn text_input_cut_removes_selection_one_undo_step() {
        let mut input = TextInput::new("F").value("abc");
        focus(&mut input);
        input.event(&mut ev(&key("SelectAll")));
        input.event(&mut ev(&key("Ctrl+X")));
        assert_eq!(input.value, "");
        assert_eq!(input.selection(), None);
        assert!(input.take_edited());
        // Cut is one undo step that restores the whole editing
        // context, including the select-all anchor.
        input.event(&mut ev(&key("Ctrl+Z")));
        assert_eq!(input.value, "abc");
        assert_eq!(input.selection(), Some((0, 3)));
        assert!(!input.can_undo());
    }

    #[test]
    fn text_input_undo_restores_pre_edit_selection() {
        let mut input = TextInput::new("F").value("abc");
        focus(&mut input);
        input.event(&mut ev(&key("SelectAll")));
        input.event(&mut ev(&ime("z")));
        assert_eq!(input.value, "z");
        input.event(&mut ev(&key("Ctrl+Z")));
        assert_eq!(input.value, "abc");
        // Undo restores the whole editing context — value, caret, AND
        // the select-all anchor that was in effect before the edit.
        assert_eq!(input.selection(), Some((0, 3)));
    }

    #[test]
    fn text_input_disabled_pointer_press_is_ignored() {
        let mut input = TextInput::new("F").value("abc").enabled(false);
        let x = click_x(&mut input, 1);
        assert_eq!(press(&mut input, x, 1), EventResponse::Ignored);
        assert!(!input.focused);
        assert_eq!(input.cursor, 3); // caret untouched
    }

    #[test]
    fn text_input_clicks_beyond_triple_keep_line_selection() {
        let mut input = TextInput::new("F").value("alpha beta");
        focus(&mut input);
        let x = click_x(&mut input, 3);
        // The click streak keeps counting past triple — a single-line
        // field has nothing beyond the line to select.
        press(&mut input, x, 4);
        assert_eq!(input.selection(), Some((0, "alpha beta".len())));
        press(&mut input, x, 7);
        assert_eq!(input.selection(), Some((0, "alpha beta".len())));
    }

    #[test]
    fn text_input_drag_release_outside_bounds() {
        let mut input = TextInput::new("F").value("alpha beta");
        focus(&mut input);
        let x = click_x(&mut input, 2);
        press(&mut input, x, 1);
        // Drag far past the right edge — the selection clamps to the
        // end rather than wrapping or dropping.
        input.event(&mut ev(&WidgetEvent::PointerMoved {
            position: Vec2::new(500.0, 12.0),
        }));
        assert_eq!(input.selection(), Some((2, "alpha beta".len())));
        // The release lands outside the bounds; pointer capture still
        // delivers it and ends the drag.
        assert_eq!(
            input.event(&mut ev(&WidgetEvent::PointerReleased {
                position: Vec2::new(500.0, 12.0),
                button: PointerButton::Primary,
            })),
            EventResponse::ReleasePointer
        );
        assert!(!input.dragging);
        // Post-release motion no longer mutates the selection.
        input.event(&mut ev(&WidgetEvent::PointerMoved {
            position: Vec2::new(10.0, 12.0),
        }));
        assert_eq!(input.selection(), Some((2, "alpha beta".len())));
    }

    #[test]
    fn text_input_drag_beyond_left_edge_selects_to_start() {
        let mut input = TextInput::new("F").value("alpha beta");
        focus(&mut input);
        let x = click_x(&mut input, 8);
        press(&mut input, x, 1);
        input.event(&mut ev(&WidgetEvent::PointerMoved {
            position: Vec2::new(-30.0, 12.0),
        }));
        assert_eq!(input.selection(), Some((0, 8)));
    }

    #[test]
    fn text_input_pointer_events_without_drag_are_ignored() {
        let mut input = TextInput::new("F").value("abc");
        focus(&mut input);
        assert_eq!(
            input.event(&mut ev(&WidgetEvent::PointerMoved {
                position: Vec2::new(50.0, 12.0),
            })),
            EventResponse::Ignored
        );
        assert_eq!(
            input.event(&mut ev(&WidgetEvent::PointerReleased {
                position: Vec2::new(50.0, 12.0),
                button: PointerButton::Primary,
            })),
            EventResponse::Ignored
        );
        // A non-primary press is not an editing gesture — no focus
        // claim, no caret move, no capture.
        assert_eq!(
            input.event(&mut ev(&WidgetEvent::PointerPressed {
                position: Vec2::new(50.0, 12.0),
                button: PointerButton::Secondary,
                count: 1,
            })),
            EventResponse::Ignored
        );
        assert_eq!(input.value, "abc");
        assert_eq!(input.selection(), None);
    }

    #[test]
    fn text_input_shift_click_extends_from_caret() {
        let mut input = TextInput::new("F").value("alpha beta");
        focus(&mut input);
        input.event(&mut ev(&key("Home")));
        input.event(&mut ev(&key("ArrowRight")));
        input.event(&mut ev(&key("Shift")));
        let x = click_x(&mut input, 7);
        press(&mut input, x, 1);
        // Shift-click extends from the existing caret instead of
        // collapsing the caret to the click.
        assert_eq!(input.selection(), Some((1, 7)));
    }

    #[test]
    fn text_input_plain_click_collapses_selection() {
        let mut input = TextInput::new("F").value("alpha beta");
        focus(&mut input);
        input.event(&mut ev(&key("SelectAll")));
        let x = click_x(&mut input, 4);
        press(&mut input, x, 1);
        assert_eq!(input.selection(), None);
        assert_eq!(input.cursor, 4);
    }

    #[test]
    fn text_input_focus_loss_clears_sticky_modifiers() {
        let mut input = TextInput::new("F").value("abc");
        focus(&mut input);
        input.event(&mut ev(&key("Shift")));
        input.event(&mut ev(&key("Control")));
        input.event(&mut ev(&WidgetEvent::FocusLost));
        // Refocus — a stuck Shift or word modifier must not leak into
        // the new session.
        focus(&mut input);
        input.event(&mut ev(&key("ArrowLeft")));
        assert_eq!(input.selection(), None);
        assert_eq!(input.cursor, 2); // one char, not a word jump
    }

    #[test]
    fn text_input_focus_loss_mid_drag_stops_drag() {
        let mut input = TextInput::new("F").value("alpha beta");
        focus(&mut input);
        let x = click_x(&mut input, 2);
        press(&mut input, x, 1);
        assert!(input.dragging);
        input.event(&mut ev(&WidgetEvent::FocusLost));
        assert!(!input.dragging);
        input.event(&mut ev(&WidgetEvent::PointerMoved {
            position: Vec2::new(150.0, 12.0),
        }));
        assert_eq!(input.selection(), None);
    }

    #[test]
    fn text_input_triple_click_drag_stays_line() {
        let mut input = TextInput::new("F").value("alpha beta gamma");
        focus(&mut input);
        let x = click_x(&mut input, 5);
        press(&mut input, x, 3);
        // Line granularity — a drag has nothing to extend in a
        // single-line field, so the selection stays whole.
        let left = click_x(&mut input, 2);
        input.event(&mut ev(&WidgetEvent::PointerMoved {
            position: Vec2::new(left, 12.0),
        }));
        assert_eq!(input.selection(), Some((0, "alpha beta gamma".len())));
    }

    #[test]
    fn text_input_undo_stack_is_bounded() {
        let mut input = TextInput::new("F");
        focus(&mut input);
        for _ in 0..UNDO_LIMIT + 5 {
            input.event(&mut ev(&ime("x")));
        }
        assert_eq!(input.value.len(), UNDO_LIMIT + 5);
        let mut undone = 0;
        while input.can_undo() {
            input.event(&mut ev(&key("Ctrl+Z")));
            undone += 1;
        }
        // Only the newest UNDO_LIMIT edits are reachable — the oldest
        // snapshots fell off the front of the deque.
        assert_eq!(undone, UNDO_LIMIT);
        assert_eq!(input.value.len(), 5);
    }

    #[test]
    fn text_input_undo_redo_empty_stack_returns_handled() {
        let mut input = TextInput::new("F").value("ab");
        focus(&mut input);
        // Empty stacks report Handled — the field consumed the
        // shortcut, there was just nothing to do.
        assert_eq!(input.event(&mut ev(&key("Ctrl+Z"))), EventResponse::Handled);
        assert_eq!(input.event(&mut ev(&key("Ctrl+Y"))), EventResponse::Handled);
        assert_eq!(input.event(&mut ev(&key("Undo"))), EventResponse::Handled);
        assert_eq!(input.event(&mut ev(&key("Redo"))), EventResponse::Handled);
        assert_eq!(input.value, "ab");
    }

    #[test]
    fn text_input_set_value_drops_history_quietly() {
        let mut input = TextInput::new("F");
        focus(&mut input);
        input.event(&mut ev(&ime("abc")));
        assert!(input.can_undo());
        assert!(input.take_edited()); // drain the flag typing set
        input.set_value("fresh");
        // Programmatic writes are not user edits — Qt setText
        // semantics: history drops and the edited flag stays clear.
        assert!(!input.can_undo());
        assert!(!input.can_redo());
        assert!(!input.take_edited());
        assert_eq!(input.cursor, 5);
    }

    #[test]
    fn text_input_key_release_non_modifier_ignored() {
        let mut input = TextInput::new("F").value("ab");
        focus(&mut input);
        assert_eq!(
            input.event(&mut ev(&WidgetEvent::KeyReleased {
                key: "x".to_string(),
            })),
            EventResponse::Ignored
        );
        // Modifier releases are consumed to clear the tracked state.
        input.event(&mut ev(&key("Shift")));
        assert_eq!(
            input.event(&mut ev(&WidgetEvent::KeyReleased {
                key: "Shift".to_string(),
            })),
            EventResponse::Handled
        );
        // Post-release arrows collapse rather than extend.
        input.event(&mut ev(&key("ArrowLeft")));
        assert_eq!(input.selection(), None);
        assert_eq!(input.cursor, 1);
    }

    #[test]
    fn text_input_navigation_keys_pass_through() {
        let mut input = TextInput::new("F").value("ab");
        focus(&mut input);
        // Single-line fields don't consume line/page/submit keys —
        // they bubble so parents can handle Enter-to-submit, Tab
        // traversal, and so on. Bare characters are also ignored:
        // text arrives via ImeCommitted, not KeyPressed.
        for k in [
            "Enter",
            "Return",
            "Tab",
            "PageUp",
            "PageDown",
            "ArrowUp",
            "ArrowDown",
            "a",
        ] {
            assert_eq!(input.event(&mut ev(&key(k))), EventResponse::Ignored, "{k}");
            assert_eq!(input.value, "ab");
        }
    }

    #[test]
    fn text_input_word_jump_crosses_whitespace_runs() {
        let mut input = TextInput::new("F").value("foo   bar");
        focus(&mut input);
        // Ctrl+Left hops between word edges — whitespace runs are
        // crossed, never landed inside.
        input.event(&mut ev(&key("Ctrl+ArrowLeft")));
        assert_eq!(input.cursor, 6); // start of "bar"
        input.event(&mut ev(&key("Ctrl+ArrowLeft")));
        assert_eq!(input.cursor, 3); // end of "foo"
        input.event(&mut ev(&key("Ctrl+ArrowLeft")));
        assert_eq!(input.cursor, 0); // start of "foo"
    }

    #[test]
    fn text_input_ctrl_backspace_with_selection_deletes_selection_only() {
        let mut input = TextInput::new("F").value("alpha beta");
        focus(&mut input);
        input.event(&mut ev(&key("Ctrl+Shift+ArrowLeft")));
        assert_eq!(input.selected_text(), Some("beta"));
        input.event(&mut ev(&key("Ctrl+Backspace")));
        // The open selection is deleted — the word before it survives.
        assert_eq!(input.value, "alpha ");
        assert_eq!(input.cursor, 6);
    }

    /// Window-space x of a caret boundary inside the painted bullet
    /// run of a masked field — the mirror of `click_x` for `secure`.
    fn masked_click_x(input: &mut TextInput, bullet: usize) -> f32 {
        let mask = input.mask_text();
        let p = input
            .text_painter
            .get_or_insert_with(crate::text_paint::shared_painter);
        TEXT_PAD_X + p.caret_x(&mask, FONT_PT, bullet * BULLET_LEN)
    }

    #[test]
    fn text_input_masked_field_hit_tests_bullet_run() {
        let mut input = TextInput::new("P").value("h\u{e9}llo").secure(true);
        focus(&mut input);
        // The painted run is bullets; a click past the second bullet
        // maps back to the real byte offset of the second grapheme.
        let x = masked_click_x(&mut input, 2);
        press(&mut input, x, 1);
        assert_eq!(input.cursor, 3); // 'h' + 'é' = 3 bytes
                                     // Deletion still steps real grapheme clusters.
        input.event(&mut ev(&key("Backspace")));
        assert_eq!(input.value, "hllo");
        assert_eq!(input.cursor, 1);
    }

    #[test]
    fn text_input_masked_cut_operates_on_real_text() {
        // Documented behavior: editing and clipboard act on the real
        // value while the display shows bullets.
        let mut input = TextInput::new("P").value("secret").secure(true);
        focus(&mut input);
        input.event(&mut ev(&key("SelectAll")));
        input.event(&mut ev(&key("Ctrl+X")));
        assert_eq!(input.value, "");
        input.event(&mut ev(&key("Ctrl+Z")));
        assert_eq!(input.value, "secret");
    }

    #[test]
    fn text_input_read_only_clearable_zone_inert() {
        let mut input = TextInput::new("S")
            .value("query")
            .clearable(true)
            .read_only(true);
        focus(&mut input);
        input.event(&mut ev(&key("Home")));
        // No ✕ zone exists on a read-only field — the press falls
        // through to the text and places the caret like any click.
        assert_eq!(press(&mut input, 190.0, 1), EventResponse::CapturePointer);
        assert_eq!(input.value, "query");
        assert_eq!(input.cursor, 5);
        input.event(&mut ev(&WidgetEvent::PointerReleased {
            position: Vec2::new(190.0, 12.0),
            button: PointerButton::Primary,
        }));
    }

    #[test]
    fn text_input_select_all_chorded() {
        let mut input = TextInput::new("F").value("abc");
        focus(&mut input);
        input.event(&mut ev(&key("Ctrl+A")));
        assert_eq!(input.selection(), Some((0, 3)));
    }

    #[test]
    fn text_input_select_all_on_empty_is_collapsed() {
        let mut input = TextInput::new("F");
        focus(&mut input);
        input.event(&mut ev(&key("SelectAll")));
        assert_eq!(input.selection(), None);
        input.event(&mut ev(&key("Backspace")));
        assert_eq!(input.value, "");
        assert!(!input.can_undo());
    }

    #[test]
    fn text_input_double_click_at_end_selects_last_word() {
        let mut input = TextInput::new("F").value("alpha beta");
        focus(&mut input);
        // A double-click past the text end resolves to the last
        // segment — matching platform double-click behavior.
        press(&mut input, 195.0, 2);
        assert_eq!(input.selected_text(), Some("beta"));
    }

    #[test]
    fn text_input_copy_with_no_selection_is_noop() {
        let mut input = TextInput::new("F").value("abc");
        focus(&mut input);
        // Nothing selected — Copy is consumed but mutates nothing.
        assert_eq!(input.event(&mut ev(&key("Ctrl+C"))), EventResponse::Handled);
        assert_eq!(input.value, "abc");
        assert!(!input.take_edited());
    }

    // ----- sanitization engine wiring -----

    #[test]
    fn sanitize_strips_bidi_overrides_on_insert() {
        let mut input = TextInput::new("F");
        focus(&mut input);
        // Trojan Source vector — a pasted/committed bidi override
        // never reaches the model.
        input.event(&mut ev(&ime("a\u{202e}b\u{202c}c")));
        assert_eq!(input.value, "abc");
    }

    #[test]
    fn sanitize_nfkc_folds_fullwidth_and_ligatures() {
        let mut input = TextInput::new("F");
        focus(&mut input);
        input.event(&mut ev(&ime("\u{ff21}\u{ff42}\u{fb01}")));
        assert_eq!(input.value, "Abfi");
    }

    #[test]
    fn sanitize_strips_controls_keeps_tab_strips_newline() {
        let mut input = TextInput::new("F");
        focus(&mut input);
        input.event(&mut ev(&ime("a\x00\x07b\nc\td")));
        assert_eq!(input.value, "abc\td");
    }

    #[test]
    fn sanitize_keeps_functional_invisibles() {
        // ZWJ/ZWNJ/ZWSP survive — breaking them destroys emoji and
        // Indic/Persian shaping.
        let mut input = TextInput::new("F");
        focus(&mut input);
        input.event(&mut ev(&ime("\u{1f468}\u{200d}\u{1f4bb}")));
        assert_eq!(input.value, "\u{1f468}\u{200d}\u{1f4bb}");
    }

    #[test]
    fn sanitize_false_is_structural_baseline() {
        // Baseline strips control chars but keeps format chars and
        // does not normalize.
        let mut input = TextInput::new("F").sanitize(false);
        focus(&mut input);
        input.event(&mut ev(&ime("a\u{202e}b\x00\u{fb01}")));
        assert_eq!(input.value, "a\u{202e}b\u{fb01}");
    }

    #[test]
    fn raw_passes_verbatim_including_newline() {
        let mut input = TextInput::new("F").raw();
        focus(&mut input);
        input.event(&mut ev(&ime("a\nb\x00")));
        assert_eq!(input.value, "a\nb\x00");
    }

    #[test]
    fn custom_sanitizer_replaces_profile() {
        struct Upper;
        impl Sanitize for Upper {
            fn name(&self) -> &'static str {
                "upper"
            }
            fn sanitize(&self, input: &str, _ctx: &SanitizeContext) -> String {
                input.to_uppercase()
            }
        }
        let mut input = TextInput::new("F").with_sanitizer(Arc::new(Upper));
        focus(&mut input);
        input.event(&mut ev(&ime("ab")));
        assert_eq!(input.value, "AB");
    }

    #[test]
    fn set_value_is_sanitized() {
        let mut input = TextInput::new("F");
        input.set_value("a\u{202e}b");
        assert_eq!(input.value, "ab");
        let mut raw = TextInput::new("F").raw();
        raw.set_value("a\u{202e}b");
        assert_eq!(raw.value, "a\u{202e}b");
    }

    #[test]
    fn sanitize_is_idempotent_across_undo() {
        // A sanitized insert is one undo step; undo restores the
        // pre-edit state exactly.
        let mut input = TextInput::new("F").value("x");
        focus(&mut input);
        input.event(&mut ev(&ime("\u{ff21}")));
        assert_eq!(input.value, "xA");
        input.event(&mut ev(&key("Ctrl+Z")));
        assert_eq!(input.value, "x");
    }
}
