//! Screen reader caret tracking and text selection updates for AccessKit.
//!
//! This module provides:
//! - Caret tracking across text nodes ([`CaretTracker`]).
//! - Selection representations with anchor, focus, and affinity ([`TextSelection`], [`TextAffinity`]).
//! - Unicode-aware text navigation boundary detection ([`TextBoundary`]).
//! - Right-to-left and vertical writing-mode aware caret geometry
//!   ([`CaretGeometry`], [`ReadingDirection`], [`WritingMode`]).
//!
//! [`CaretGeometry`]: crate::caret::CaretGeometry
//! [`ReadingDirection`]: crate::caret::ReadingDirection
//! [`WritingMode`]: crate::caret::WritingMode
//! - Updating [`accesskit::Node`] instances with current text selections.

use accesskit::{Node, NodeId, Rect, TextPosition, TextSelection as AccessKitSelection};
use unicode_segmentation::UnicodeSegmentation;

/// Text affinity for disambiguating caret placement at line and run wrapping boundaries.
///
/// # Examples
///
/// ```
/// use martensite_access::caret::TextAffinity;
///
/// let affinity = TextAffinity::Downstream;
/// assert_eq!(affinity, TextAffinity::default());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TextAffinity {
    /// Caret associates with the character preceding the offset (trailing edge of line 1).
    Upstream,
    /// Caret associates with the character following the offset (leading edge of line 2).
    #[default]
    Downstream,
}

/// Text selection or collapsed caret position within an accessible text element.
///
/// Offsets are measured in **Unicode scalar value (code point) indices**, not
/// byte offsets. This ensures that multi-byte characters (e.g., emoji,
/// CJK ideographs, combining sequences) are addressed consistently with
/// platform assistive technologies. Use [`char_index_to_byte_offset`] and
/// [`byte_offset_to_char_index`] to convert to and from byte positions in a
/// specific `&str`.
///
/// # Examples
///
/// ```
/// use martensite_access::caret::{TextAffinity, TextSelection};
///
/// let selection = TextSelection::new(0, 5, TextAffinity::Downstream);
/// assert!(!selection.is_collapsed());
/// assert_eq!(selection.range(), 0..5);
///
/// let caret = TextSelection::caret(5, TextAffinity::Downstream);
/// assert!(caret.is_collapsed());
/// assert_eq!(caret.range(), 5..5);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TextSelection {
    /// The position where the selection started, which remains fixed during selection drag.
    pub anchor: usize,
    /// The active moving end of the selection (the caret position).
    pub focus: usize,
    /// Disambiguation affinity for line wrap boundaries.
    pub affinity: TextAffinity,
}

impl TextSelection {
    /// Constructs a new text selection spanning `anchor` to `focus`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_access::caret::{TextAffinity, TextSelection};
    ///
    /// let sel = TextSelection::new(2, 8, TextAffinity::Downstream);
    /// assert_eq!(sel.anchor, 2);
    /// assert_eq!(sel.focus, 8);
    /// ```
    #[inline]
    pub const fn new(anchor: usize, focus: usize, affinity: TextAffinity) -> Self {
        Self {
            anchor,
            focus,
            affinity,
        }
    }

    /// Constructs a collapsed text selection representing a solitary caret at `index`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_access::caret::{TextAffinity, TextSelection};
    ///
    /// let caret = TextSelection::caret(3, TextAffinity::Downstream);
    /// assert!(caret.is_collapsed());
    /// ```
    #[inline]
    pub const fn caret(index: usize, affinity: TextAffinity) -> Self {
        Self {
            anchor: index,
            focus: index,
            affinity,
        }
    }

    /// Returns `true` if the selection is collapsed to a single point (caret).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_access::caret::{TextAffinity, TextSelection};
    ///
    /// let sel = TextSelection::new(4, 4, TextAffinity::Downstream);
    /// assert!(sel.is_collapsed());
    /// ```
    #[inline]
    pub const fn is_collapsed(&self) -> bool {
        self.anchor == self.focus
    }

    /// Returns the normalized character range `min(anchor, focus)..max(anchor, focus)`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_access::caret::{TextAffinity, TextSelection};
    ///
    /// let sel_forward = TextSelection::new(2, 6, TextAffinity::Downstream);
    /// assert_eq!(sel_forward.range(), 2..6);
    ///
    /// let sel_backward = TextSelection::new(6, 2, TextAffinity::Downstream);
    /// assert_eq!(sel_backward.range(), 2..6);
    /// ```
    #[inline]
    pub fn range(&self) -> std::ops::Range<usize> {
        let start = self.anchor.min(self.focus);
        let end = self.anchor.max(self.focus);
        start..end
    }
}

/// Text boundary granularity for cursor stepping and selection range computation.
///
/// # Examples
///
/// ```
/// use martensite_access::caret::TextBoundary;
///
/// let boundary = TextBoundary::Word;
/// assert_eq!(boundary, TextBoundary::Word);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TextBoundary {
    /// Individual Unicode extended grapheme cluster boundary.
    #[default]
    Character,
    /// Word boundary determined by Unicode segmentation rules.
    Word,
    /// Line boundary delimited by line feed (`\n`) or carriage return/line feed (`\r\n`).
    Line,
    /// Paragraph boundary delimited by blank lines or line break sequences.
    Paragraph,
    /// Entire document or text boundary.
    Document,
}

/// Logical reading direction of a text run.
///
/// This is used to map logical selection endpoints to visual caret rectangles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ReadingDirection {
    /// Left-to-right text (e.g., Latin, Cyrillic, CJK horizontal).
    #[default]
    Ltr,
    /// Right-to-left text (e.g., Arabic, Hebrew).
    Rtl,
}

/// Writing mode for a text run.
///
/// This is used to orient caret geometry in horizontal or vertical layouts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum WritingMode {
    /// Horizontal top-to-bottom text.
    #[default]
    Horizontal,
    /// Vertical right-to-left or left-to-right text (baseline runs vertically).
    Vertical,
}

/// Visual caret geometry for a text selection.
///
/// Carries separate rectangles for the anchor and focus endpoints so that
/// bidirectional and vertical text can be represented correctly.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CaretGeometry {
    /// Visual rectangle of the anchor endpoint.
    pub anchor_rect: Rect,
    /// Visual rectangle of the focus / caret endpoint.
    pub focus_rect: Rect,
    /// Logical reading direction used to derive the rectangles.
    pub direction: ReadingDirection,
    /// Writing mode used to derive the rectangles.
    pub writing_mode: WritingMode,
}

impl CaretGeometry {
    /// Creates a new `CaretGeometry` with the given anchor and focus rectangles.
    #[inline]
    pub const fn new(
        anchor_rect: Rect,
        focus_rect: Rect,
        direction: ReadingDirection,
        writing_mode: WritingMode,
    ) -> Self {
        Self {
            anchor_rect,
            focus_rect,
            direction,
            writing_mode,
        }
    }

    /// Returns the rectangle for the focus endpoint, i.e., the visible caret.
    #[inline]
    pub const fn caret_rect(&self) -> Rect {
        self.focus_rect
    }
}

/// Screen reader caret tracker managing cursor positions and character bounding boxes.
///
/// # Examples
///
/// ```
/// use martensite_access::caret::{CaretTracker, TextAffinity, TextSelection};
/// use accesskit::NodeId;
///
/// let tracker = CaretTracker::new(
///     NodeId(1),
///     TextSelection::caret(0, TextAffinity::Downstream),
/// );
/// assert!(tracker.selection.is_collapsed());
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct CaretTracker {
    /// Target AccessKit node ID representing the text element.
    pub node_id: NodeId,
    /// Current text selection or caret state.
    pub selection: TextSelection,
    /// Screen-space bounding rectangles for each character.
    pub character_bounds: Vec<Rect>,
}

impl CaretTracker {
    /// Creates a new `CaretTracker` with the given node ID and initial selection.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_access::caret::{CaretTracker, TextAffinity, TextSelection};
    /// use accesskit::NodeId;
    ///
    /// let tracker = CaretTracker::new(
    ///     NodeId(42),
    ///     TextSelection::caret(0, TextAffinity::Downstream),
    /// );
    /// assert_eq!(tracker.node_id, NodeId(42));
    /// ```
    pub fn new(node_id: NodeId, selection: TextSelection) -> Self {
        Self {
            node_id,
            selection,
            character_bounds: Vec::new(),
        }
    }

    /// Updates the current text selection.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_access::caret::{CaretTracker, TextAffinity, TextSelection};
    /// use accesskit::NodeId;
    ///
    /// let mut tracker = CaretTracker::new(NodeId(1), TextSelection::caret(0, TextAffinity::Downstream));
    /// tracker.set_selection(TextSelection::new(0, 5, TextAffinity::Downstream));
    /// assert_eq!(tracker.selection.range(), 0..5);
    /// ```
    #[inline]
    pub fn set_selection(&mut self, selection: TextSelection) {
        self.selection = selection;
    }

    /// Sets the character bounding boxes in screen or container space.
    ///
    /// The `i`-th rectangle corresponds to the `i`-th Unicode scalar value in
    /// the accessible text. Callers are responsible for ensuring that the
    /// number of rectangles matches the number of characters.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_access::caret::{CaretTracker, TextAffinity, TextSelection};
    /// use accesskit::{NodeId, Rect};
    ///
    /// let mut tracker = CaretTracker::new(NodeId(1), TextSelection::caret(0, TextAffinity::Downstream));
    /// tracker.set_character_bounds(vec![Rect::new(0.0, 0.0, 10.0, 20.0)]);
    /// assert_eq!(tracker.character_bounds.len(), 1);
    /// ```
    #[inline]
    pub fn set_character_bounds(&mut self, bounds: Vec<Rect>) {
        self.character_bounds = bounds;
    }

    /// Computes the bounding rectangle of the caret at its current focus position.
    ///
    /// Returns `None` if `character_bounds` is empty.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_access::caret::{CaretTracker, TextAffinity, TextSelection};
    /// use accesskit::{NodeId, Rect};
    ///
    /// let mut tracker = CaretTracker::new(NodeId(1), TextSelection::caret(0, TextAffinity::Downstream));
    /// tracker.set_character_bounds(vec![
    ///     Rect::new(0.0, 0.0, 10.0, 20.0),
    ///     Rect::new(10.0, 0.0, 20.0, 20.0),
    /// ]);
    /// let caret_rect = tracker.current_caret_rect().unwrap();
    /// assert_eq!(caret_rect.x0, 0.0);
    /// assert_eq!(caret_rect.y0, 0.0);
    /// ```
    pub fn current_caret_rect(&self) -> Option<Rect> {
        Some(
            self.caret_geometry(ReadingDirection::Ltr, WritingMode::Horizontal)?
                .caret_rect(),
        )
    }

    /// Computes a [`CaretGeometry`] for the current selection using the supplied
    /// reading direction and writing mode.
    ///
    /// For horizontal LTR text the focus rectangle is placed at the leading edge
    /// of the focus character. For RTL text the leading edge is on the right side
    /// of the character bounds. For vertical text the rectangles are interpreted
    /// with the baseline running top-to-bottom.
    ///
    /// Returns `None` if `character_bounds` is empty.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_access::caret::{CaretTracker, ReadingDirection, TextAffinity, TextSelection, WritingMode};
    /// use accesskit::{NodeId, Rect};
    ///
    /// let mut tracker = CaretTracker::new(NodeId(1), TextSelection::new(0, 2, TextAffinity::Downstream));
    /// tracker.set_character_bounds(vec![
    ///     Rect::new(0.0, 0.0, 10.0, 20.0),
    ///     Rect::new(10.0, 0.0, 20.0, 20.0),
    ///     Rect::new(20.0, 0.0, 30.0, 20.0),
    /// ]);
    ///
    /// let geo = tracker.caret_geometry(ReadingDirection::Rtl, WritingMode::Horizontal).unwrap();
    /// // RTL focus at character 2 uses the trailing (right) edge.
    /// assert_eq!(geo.focus_rect.x0, 30.0);
    /// ```
    pub fn caret_geometry(
        &self,
        direction: ReadingDirection,
        writing_mode: WritingMode,
    ) -> Option<CaretGeometry> {
        if self.character_bounds.is_empty() {
            return None;
        }

        let len = self.character_bounds.len();
        let anchor = self.selection.anchor.min(len);
        let focus = self.selection.focus.min(len);

        let anchor_rect = endpoint_caret_rect(
            &self.character_bounds,
            anchor,
            self.selection.affinity,
            direction,
            writing_mode,
        );
        let focus_rect = endpoint_caret_rect(
            &self.character_bounds,
            focus,
            self.selection.affinity,
            direction,
            writing_mode,
        );

        Some(CaretGeometry::new(
            anchor_rect,
            focus_rect,
            direction,
            writing_mode,
        ))
    }

    /// Finds the byte range for the specified [`TextBoundary`] surrounding `byte_index`.
    ///
    /// The returned byte offsets are always aligned to valid UTF-8 boundaries and,
    /// for [`TextBoundary::Character`], to Unicode extended grapheme cluster
    /// boundaries.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_access::caret::{CaretTracker, TextBoundary};
    ///
    /// let text = "Hello world!\nSecond line.";
    /// assert_eq!(CaretTracker::find_boundary_range(text, 1, TextBoundary::Word), (0, 5));
    /// assert_eq!(CaretTracker::find_boundary_range(text, 7, TextBoundary::Word), (6, 11));
    /// assert_eq!(CaretTracker::find_boundary_range(text, 3, TextBoundary::Line), (0, 12));
    /// assert_eq!(CaretTracker::find_boundary_range(text, 0, TextBoundary::Document), (0, text.len()));
    /// ```
    pub fn find_boundary_range(
        text: &str,
        byte_index: usize,
        boundary: TextBoundary,
    ) -> (usize, usize) {
        if text.is_empty() {
            return (0, 0);
        }

        let aligned = align_to_char_boundary(text, byte_index);

        match boundary {
            TextBoundary::Character => grapheme_boundary_range(text, aligned),
            TextBoundary::Word => word_boundary_range(text, aligned),
            TextBoundary::Line => line_boundary_range(text, aligned),
            TextBoundary::Paragraph => paragraph_boundary_range(text, aligned),
            TextBoundary::Document => (0, text.len()),
        }
    }

    /// Returns the previous boundary byte index before `byte_index`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_access::caret::{CaretTracker, TextBoundary};
    ///
    /// let text = "Hello world";
    /// assert_eq!(CaretTracker::prev_boundary(text, 6, TextBoundary::Word), Some(0));
    /// assert_eq!(CaretTracker::prev_boundary(text, 1, TextBoundary::Character), Some(0));
    /// ```
    pub fn prev_boundary(text: &str, byte_index: usize, boundary: TextBoundary) -> Option<usize> {
        let aligned = align_to_char_boundary(text, byte_index);
        match boundary {
            TextBoundary::Character => prev_grapheme_boundary(text, aligned),
            TextBoundary::Word => prev_word_boundary(text, aligned),
            TextBoundary::Line => prev_line_boundary(text, aligned),
            TextBoundary::Paragraph => prev_paragraph_boundary(text, aligned),
            TextBoundary::Document => Some(0),
        }
    }

    /// Returns the next boundary byte index after `byte_index`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_access::caret::{CaretTracker, TextBoundary};
    ///
    /// let text = "Hello world";
    /// assert_eq!(CaretTracker::next_boundary(text, 5, TextBoundary::Word), Some(11));
    /// assert_eq!(CaretTracker::next_boundary(text, 0, TextBoundary::Character), Some(1));
    /// ```
    pub fn next_boundary(text: &str, byte_index: usize, boundary: TextBoundary) -> Option<usize> {
        let aligned = align_to_char_boundary(text, byte_index);
        match boundary {
            TextBoundary::Character => next_grapheme_boundary(text, aligned),
            TextBoundary::Word => next_word_boundary(text, aligned),
            TextBoundary::Line => next_line_boundary(text, aligned),
            TextBoundary::Paragraph => next_paragraph_boundary(text, aligned),
            TextBoundary::Document => Some(text.len()),
        }
    }

    /// Applies the current selection state to the specified AccessKit [`Node`].
    ///
    /// Sets the node's `text_selection` property referencing `self.node_id` and the
    /// anchor/focus offsets as character indices. The caller must ensure that the
    /// `character_index` values are consistent with how the platform adapter
    /// interprets them; this implementation exposes character indices from the
    /// selection unchanged.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_access::caret::{CaretTracker, TextAffinity, TextSelection};
    /// use accesskit::{Node, NodeId, Role};
    ///
    /// let tracker = CaretTracker::new(NodeId(5), TextSelection::new(1, 4, TextAffinity::Downstream));
    /// let mut node = Node::new(Role::TextInput);
    /// tracker.apply_to_node(&mut node);
    /// assert!(node.text_selection().is_some());
    /// ```
    pub fn apply_to_node(&self, node: &mut Node) {
        let sel = AccessKitSelection {
            anchor: TextPosition {
                node: self.node_id,
                character_index: self.selection.anchor,
            },
            focus: TextPosition {
                node: self.node_id,
                character_index: self.selection.focus,
            },
        };
        node.set_text_selection(sel);
    }
}

/// Aligns an arbitrary byte offset to the nearest valid UTF-8 character boundary.
fn align_to_char_boundary(text: &str, index: usize) -> usize {
    let len = text.len();
    if index >= len {
        return len;
    }
    let mut idx = index;
    while idx > 0 && !text.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

fn endpoint_caret_rect(
    bounds: &[Rect],
    index: usize,
    affinity: TextAffinity,
    direction: ReadingDirection,
    writing_mode: WritingMode,
) -> Rect {
    let len = bounds.len();
    let index = index.min(len);

    if writing_mode == WritingMode::Vertical {
        // In vertical mode the "leading" edge is the top of the character cell.
        let r = if index < len {
            bounds[index]
        } else {
            bounds[len.saturating_sub(1)]
        };
        let y = if index < len { r.y0 } else { r.y1 };
        if affinity == TextAffinity::Upstream && index > 0 {
            let prev = bounds[index.saturating_sub(1)];
            Rect::new(prev.x0, prev.y1, prev.x1, prev.y1)
        } else {
            Rect::new(r.x0, y, r.x1, y)
        }
    } else {
        // Horizontal mode: leading edge depends on reading direction.
        if direction == ReadingDirection::Rtl {
            // RTL leading edge is the right side of the visual character cell.
            if index == 0 && bounds.is_empty() {
                // Defensive: never happens because caller checks empty.
                Rect::new(0.0, 0.0, 0.0, 0.0)
            } else if index < len {
                let r = bounds[index];
                if affinity == TextAffinity::Upstream && index > 0 {
                    let prev = bounds[index - 1];
                    Rect::new(prev.x0, prev.y0, prev.x0, prev.y1)
                } else {
                    Rect::new(r.x1, r.y0, r.x1, r.y1)
                }
            } else {
                let last = bounds[len.saturating_sub(1)];
                Rect::new(last.x0, last.y0, last.x0, last.y1)
            }
        } else {
            // LTR leading edge is the left side of the visual character cell.
            if index < len {
                let r = bounds[index];
                if affinity == TextAffinity::Upstream && index > 0 {
                    let prev = bounds[index - 1];
                    Rect::new(prev.x1, prev.y0, prev.x1, prev.y1)
                } else {
                    Rect::new(r.x0, r.y0, r.x0, r.y1)
                }
            } else {
                let last = bounds[len.saturating_sub(1)];
                Rect::new(last.x1, last.y0, last.x1, last.y1)
            }
        }
    }
}

fn grapheme_boundary_range(text: &str, byte_index: usize) -> (usize, usize) {
    let aligned = align_to_char_boundary(text, byte_index);
    for (start, grapheme) in text.grapheme_indices(true) {
        let end = start + grapheme.len();
        if aligned >= start && aligned < end {
            return (start, end);
        }
    }
    if aligned == text.len() {
        let start = text
            .grapheme_indices(true)
            .next_back()
            .map(|(s, g)| s + g.len())
            .unwrap_or(0);
        (start, text.len())
    } else {
        (0, text.len())
    }
}

fn prev_grapheme_boundary(text: &str, byte_index: usize) -> Option<usize> {
    if byte_index == 0 {
        return None;
    }
    let aligned = align_to_char_boundary(text, byte_index);
    let mut prev = 0;
    for (start, _) in text.grapheme_indices(true) {
        if start >= aligned {
            break;
        }
        prev = start;
    }
    if prev == aligned {
        None
    } else {
        Some(prev)
    }
}

fn next_grapheme_boundary(text: &str, byte_index: usize) -> Option<usize> {
    if byte_index >= text.len() {
        return None;
    }
    let aligned = align_to_char_boundary(text, byte_index);
    for (start, grapheme) in text.grapheme_indices(true) {
        let end = start + grapheme.len();
        if end > aligned {
            return Some(end);
        }
    }
    Some(text.len())
}

fn word_boundary_range(text: &str, byte_index: usize) -> (usize, usize) {
    let words: Vec<(usize, &str)> = text.split_word_bound_indices().collect();
    if words.is_empty() {
        return (0, text.len());
    }

    let mut start = words[0].0;
    let mut end = text.len();
    for window in words.windows(2) {
        let (a, _) = window[0];
        let (b, _) = window[1];
        if byte_index >= a && byte_index < b {
            start = a;
            end = b;
            break;
        }
    }
    if byte_index >= words[words.len() - 1].0 {
        start = words[words.len() - 1].0;
    }
    (start, end)
}

fn prev_word_boundary(text: &str, byte_index: usize) -> Option<usize> {
    let aligned = align_to_char_boundary(text, byte_index);
    // Start offsets of non-whitespace word segments. Whitespace runs are
    // skipped so the caret lands on the previous *word*, matching
    // platform caret semantics (Ctrl+Left on Windows/macOS).
    let starts: Vec<usize> = text
        .split_word_bound_indices()
        .filter(|(_, word)| !word.chars().all(char::is_whitespace))
        .map(|(i, _)| i)
        .collect();

    // The previous boundary is the latest word start strictly before the
    // current offset. At end-of-text this yields the start of the last
    // word; inside a word it yields that word's start only when the caret
    // is past the start.
    starts.iter().rev().copied().find(|&s| s < aligned)
}

fn next_word_boundary(text: &str, byte_index: usize) -> Option<usize> {
    let aligned = align_to_char_boundary(text, byte_index);

    // Move to the end of the current-or-next non-whitespace word segment,
    // matching platform caret semantics (Ctrl+Right on Windows/macOS):
    // whitespace runs are traversed rather than treated as stop points.
    for (start, word) in text.split_word_bound_indices() {
        if word.chars().all(char::is_whitespace) {
            continue;
        }
        let end = start + word.len();
        if end > aligned {
            return Some(end);
        }
    }
    None
}

fn line_boundary_range(text: &str, byte_index: usize) -> (usize, usize) {
    let start = text[..byte_index].rfind('\n').map_or(0, |i| i + 1);
    let end = text[byte_index..].find('\n').map_or(text.len(), |i| {
        let candidate = byte_index + i;
        // Exclude the trailing CRLF from the line content if present.
        if candidate > 0 && text.as_bytes().get(candidate - 1) == Some(&b'\r') {
            candidate - 1
        } else {
            candidate
        }
    });
    (start, end)
}

fn prev_line_boundary(text: &str, byte_index: usize) -> Option<usize> {
    if byte_index == 0 {
        return None;
    }
    let search_end = byte_index.saturating_sub(1);
    text[..search_end].rfind('\n').map(|i| i + 1).or(Some(0))
}

fn next_line_boundary(text: &str, byte_index: usize) -> Option<usize> {
    if byte_index >= text.len() {
        return None;
    }
    text[byte_index..]
        .find('\n')
        .map(|i| {
            let mut pos = byte_index + i + 1;
            if text.as_bytes().get(pos.saturating_sub(1)) == Some(&b'\r') {
                pos = pos.saturating_sub(1);
            }
            pos.min(text.len())
        })
        .or(Some(text.len()))
}

/// Length of a line-ending sequence starting at `i`.
///
/// Treats `\r\n` as a single delimiter and bare `\n` or `\r` as single
/// delimiters. Returns `0` if `i` is not at a line ending.
fn line_ending_len(text: &str, i: usize) -> usize {
    if i >= text.len() {
        return 0;
    }
    match text.as_bytes()[i] {
        b'\n' => 1,
        b'\r' if text.as_bytes().get(i + 1) == Some(&b'\n') => 2,
        b'\r' => 1,
        _ => 0,
    }
}

/// Returns the `(start, end)` byte offsets of the first paragraph
/// boundary at or after `start`.
///
/// A paragraph boundary is two consecutive line-ending sequences with
/// no content between them, e.g. `\n\n`, `\r\n\r\n`, `\n\r\n`, or `\r\r`.
fn next_paragraph_boundary_start(text: &str, start: usize) -> Option<(usize, usize)> {
    let mut i = start;
    while i < text.len() {
        let len = line_ending_len(text, i);
        if len == 0 {
            i += 1;
            continue;
        }
        let j = i + len;
        if j < text.len() {
            let len2 = line_ending_len(text, j);
            if len2 > 0 {
                return Some((i, j + len2));
            }
        }
        i += len;
    }
    None
}

fn paragraph_boundary_range(text: &str, byte_index: usize) -> (usize, usize) {
    if text.is_empty() {
        return (0, 0);
    }

    // Find the end of the previous paragraph boundary (or the start of text).
    let mut start = 0;
    let mut i = 0;
    while i < text.len() {
        let len = line_ending_len(text, i);
        if len == 0 {
            i += 1;
            continue;
        }
        let j = i + len;
        if j < text.len() {
            let len2 = line_ending_len(text, j);
            if len2 > 0 {
                let boundary_end = j + len2;
                if boundary_end <= byte_index {
                    start = boundary_end;
                    i = boundary_end;
                    continue;
                }
                break;
            }
        }
        i += len;
    }

    // Find the start of the next paragraph boundary (or the end of text).
    let end = if let Some((next_start, _)) = next_paragraph_boundary_start(text, byte_index) {
        next_start
    } else {
        text.len()
    };

    (start, end)
}

fn prev_paragraph_boundary(text: &str, byte_index: usize) -> Option<usize> {
    if byte_index == 0 {
        return None;
    }
    let mut last = None;
    let mut i = 0;
    while i < byte_index {
        let len = line_ending_len(text, i);
        if len == 0 {
            i += 1;
            continue;
        }
        let j = i + len;
        if j >= text.len() {
            break;
        }
        let len2 = line_ending_len(text, j);
        if len2 > 0 {
            let boundary_start = i;
            if boundary_start < byte_index {
                last = Some(boundary_start);
            }
            i = j + len2;
        } else {
            i += len;
        }
    }
    last.or(Some(0))
}

fn next_paragraph_boundary(text: &str, byte_index: usize) -> Option<usize> {
    if byte_index >= text.len() {
        return None;
    }
    next_paragraph_boundary_start(text, byte_index)
        .map(|(_, end)| end)
        .or(Some(text.len()))
}

/// Converts a character index to the corresponding byte offset in `text`.
///
/// Returns `None` if the character index is out of range.
pub fn char_index_to_byte_offset(text: &str, char_index: usize) -> Option<usize> {
    let mut current = 0usize;
    for (offset, _) in text.char_indices() {
        if current == char_index {
            return Some(offset);
        }
        current += 1;
    }
    if current == char_index {
        Some(text.len())
    } else {
        None
    }
}

/// Converts a byte offset in `text` to the corresponding character index.
///
/// Returns `None` if `byte_offset` is not on a character boundary.
pub fn byte_offset_to_char_index(text: &str, byte_offset: usize) -> Option<usize> {
    if byte_offset > text.len() || !text.is_char_boundary(byte_offset) {
        return None;
    }
    Some(text[..byte_offset].chars().count())
}

#[cfg(test)]
mod tests {
    use super::*;
    use accesskit::Role;

    #[test]
    fn test_text_selection_range_and_collapsed() {
        let sel = TextSelection::new(3, 7, TextAffinity::Downstream);
        assert!(!sel.is_collapsed());
        assert_eq!(sel.range(), 3..7);

        let reverse_sel = TextSelection::new(7, 3, TextAffinity::Upstream);
        assert_eq!(reverse_sel.range(), 3..7);

        let caret = TextSelection::caret(5, TextAffinity::Downstream);
        assert!(caret.is_collapsed());
        assert_eq!(caret.range(), 5..5);
    }

    #[test]
    fn test_caret_tracker_rects() {
        let mut tracker = CaretTracker::new(
            NodeId(10),
            TextSelection::caret(1, TextAffinity::Downstream),
        );
        assert!(tracker.current_caret_rect().is_none());

        tracker.set_character_bounds(vec![
            Rect::new(0.0, 0.0, 10.0, 20.0),
            Rect::new(10.0, 0.0, 20.0, 20.0),
            Rect::new(20.0, 0.0, 30.0, 20.0),
        ]);

        let rect = tracker.current_caret_rect().unwrap();
        assert_eq!(rect.x0, 10.0);
        assert_eq!(rect.y0, 0.0);

        // At end of bounds
        tracker.set_selection(TextSelection::caret(3, TextAffinity::Downstream));
        let end_rect = tracker.current_caret_rect().unwrap();
        assert_eq!(end_rect.x0, 30.0);
    }

    #[test]
    fn test_caret_geometry_rtl() {
        let mut tracker = CaretTracker::new(
            NodeId(1),
            TextSelection::new(0, 2, TextAffinity::Downstream),
        );
        tracker.set_character_bounds(vec![
            Rect::new(0.0, 0.0, 10.0, 20.0),
            Rect::new(10.0, 0.0, 20.0, 20.0),
            Rect::new(20.0, 0.0, 30.0, 20.0),
        ]);

        let ltr = tracker
            .caret_geometry(ReadingDirection::Ltr, WritingMode::Horizontal)
            .unwrap();
        assert_eq!(ltr.focus_rect.x0, 20.0); // leading edge of char 2 is x0

        let rtl = tracker
            .caret_geometry(ReadingDirection::Rtl, WritingMode::Horizontal)
            .unwrap();
        assert_eq!(rtl.focus_rect.x0, 30.0); // RTL leading edge is x1 of char 2

        let vertical = tracker
            .caret_geometry(ReadingDirection::Ltr, WritingMode::Vertical)
            .unwrap();
        assert_eq!(vertical.focus_rect.y0, 0.0);
        assert_eq!(vertical.focus_rect.y1, 0.0);
    }

    #[test]
    fn test_boundary_detection() {
        let text = "Hello world!\nSecond line.\n\nParagraph two.";

        // Word boundary
        assert_eq!(
            CaretTracker::find_boundary_range(text, 2, TextBoundary::Word),
            (0, 5)
        );
        assert_eq!(
            CaretTracker::find_boundary_range(text, 8, TextBoundary::Word),
            (6, 11)
        );

        // Line boundary
        assert_eq!(
            CaretTracker::find_boundary_range(text, 3, TextBoundary::Line),
            (0, 12)
        );

        // Paragraph boundary
        let (p_start, p_end) = CaretTracker::find_boundary_range(text, 2, TextBoundary::Paragraph);
        assert_eq!(p_start, 0);
        assert!(p_end > 0);

        // Document boundary
        assert_eq!(
            CaretTracker::find_boundary_range(text, 5, TextBoundary::Document),
            (0, text.len())
        );
    }

    #[test]
    fn test_boundary_detection_multibyte() {
        let text = "日本語\n世界";
        // Byte index 1 is inside '日' (3 bytes in UTF-8)
        let (start, end) = CaretTracker::find_boundary_range(text, 1, TextBoundary::Line);
        assert_eq!(start, 0);
        assert_eq!(end, "日本語".len());

        let (p_start, p_end) = CaretTracker::find_boundary_range(text, 1, TextBoundary::Paragraph);
        assert_eq!(p_start, 0);
        // A single line ending is not a paragraph boundary, so the whole
        // text is treated as one paragraph.
        assert_eq!(p_end, text.len());

        // Grapheme boundary for CJK returns a single character.
        let (g_start, g_end) = CaretTracker::find_boundary_range(text, 1, TextBoundary::Character);
        assert_eq!(g_start, 0);
        assert_eq!(g_end, "日".len());
    }

    #[test]
    fn test_grapheme_boundary_with_emoji() {
        // "flag" is a single extended grapheme cluster made of regional indicators.
        let text = "a\u{1f1fa}\u{1f1f8}b"; // a + US flag + b
        let (start, end) = CaretTracker::find_boundary_range(text, 1, TextBoundary::Character);
        assert_eq!(start, 1);
        assert_eq!(end, 1 + "\u{1f1fa}\u{1f1f8}".len());
    }

    #[test]
    fn test_next_prev_word_boundary() {
        let text = "Hello world";
        assert_eq!(
            CaretTracker::next_boundary(text, 0, TextBoundary::Word),
            Some(5)
        );
        assert_eq!(
            CaretTracker::next_boundary(text, 6, TextBoundary::Word),
            Some(11)
        );
        assert_eq!(
            CaretTracker::prev_boundary(text, 11, TextBoundary::Word),
            Some(6)
        );
        assert_eq!(
            CaretTracker::prev_boundary(text, 6, TextBoundary::Word),
            Some(0)
        );
    }

    #[test]
    fn test_next_prev_grapheme_boundary() {
        let text = "a\u{1f1fa}\u{1f1f8}b";
        assert_eq!(
            CaretTracker::next_boundary(text, 0, TextBoundary::Character),
            Some(1)
        );
        assert_eq!(
            CaretTracker::next_boundary(text, 1, TextBoundary::Character),
            Some(1 + "\u{1f1fa}\u{1f1f8}".len())
        );
        assert_eq!(
            CaretTracker::prev_boundary(
                text,
                1 + "\u{1f1fa}\u{1f1f8}".len(),
                TextBoundary::Character
            ),
            Some(1)
        );
    }

    #[test]
    fn test_apply_to_node() {
        let tracker = CaretTracker::new(
            NodeId(7),
            TextSelection::new(2, 5, TextAffinity::Downstream),
        );
        let mut node = Node::new(Role::TextInput);
        tracker.apply_to_node(&mut node);

        let sel = node.text_selection().expect("text selection must be set");
        assert_eq!(sel.anchor.node, NodeId(7));
        assert_eq!(sel.anchor.character_index, 2);
        assert_eq!(sel.focus.node, NodeId(7));
        assert_eq!(sel.focus.character_index, 5);
    }

    #[test]
    fn test_char_index_conversions() {
        let text = "aé日本";
        assert_eq!(char_index_to_byte_offset(text, 0), Some(0));
        assert_eq!(char_index_to_byte_offset(text, 1), Some(1));
        assert_eq!(char_index_to_byte_offset(text, 2), Some(3));
        assert_eq!(char_index_to_byte_offset(text, 3), Some(6));
        assert_eq!(char_index_to_byte_offset(text, 4), Some(9));

        assert_eq!(byte_offset_to_char_index(text, 0), Some(0));
        assert_eq!(byte_offset_to_char_index(text, 1), Some(1));
        assert_eq!(byte_offset_to_char_index(text, 3), Some(2));
        assert_eq!(byte_offset_to_char_index(text, 6), Some(3));
        assert_eq!(byte_offset_to_char_index(text, 9), Some(4));
    }

    #[test]
    fn test_paragraph_boundaries_crlf() {
        let text = "Line1\r\nLine2\r\n\r\nPara2.";
        // First paragraph runs to the first line-ending sequence.
        assert_eq!(
            CaretTracker::find_boundary_range(text, 2, TextBoundary::Paragraph),
            (0, 12)
        );
        // Second paragraph starts after the \r\n\r\n boundary.
        assert_eq!(
            CaretTracker::find_boundary_range(text, 18, TextBoundary::Paragraph),
            (16, text.len())
        );
        // Next boundary ends at 16; previous boundary starts at 12.
        assert_eq!(
            CaretTracker::next_boundary(text, 2, TextBoundary::Paragraph),
            Some(16)
        );
        assert_eq!(
            CaretTracker::prev_boundary(text, 18, TextBoundary::Paragraph),
            Some(12)
        );
    }

    #[test]
    fn test_paragraph_boundaries_cr_only() {
        let text = "Line1\rLine2\r\rPara2.";
        assert_eq!(
            CaretTracker::find_boundary_range(text, 2, TextBoundary::Paragraph),
            (0, 11)
        );
        assert_eq!(
            CaretTracker::find_boundary_range(text, 14, TextBoundary::Paragraph),
            (13, text.len())
        );
    }

    #[test]
    fn test_paragraph_boundaries_mixed_endings() {
        // \n then \r\n
        let text1 = "Line1\n\r\nLine2.";
        assert_eq!(
            CaretTracker::find_boundary_range(text1, 2, TextBoundary::Paragraph),
            (0, 5)
        );
        assert_eq!(
            CaretTracker::find_boundary_range(text1, 8, TextBoundary::Paragraph),
            (8, text1.len())
        );

        // \r\n then \n
        let text2 = "Line1\r\n\nLine2.";
        assert_eq!(
            CaretTracker::find_boundary_range(text2, 2, TextBoundary::Paragraph),
            (0, 5)
        );
        assert_eq!(
            CaretTracker::find_boundary_range(text2, 8, TextBoundary::Paragraph),
            (8, text2.len())
        );
    }

    #[test]
    fn test_paragraph_boundaries_empty_and_trailing_delim() {
        let empty = "";
        assert_eq!(
            CaretTracker::find_boundary_range(empty, 0, TextBoundary::Paragraph),
            (0, 0)
        );
        assert_eq!(
            CaretTracker::next_boundary(empty, 0, TextBoundary::Paragraph),
            None
        );

        // Text ending in a delimiter with no final paragraph.
        let trailing = "Hello\n\n";
        assert_eq!(
            CaretTracker::find_boundary_range(trailing, 2, TextBoundary::Paragraph),
            (0, 5)
        );
        assert_eq!(
            CaretTracker::next_boundary(trailing, 2, TextBoundary::Paragraph),
            Some(7)
        );
    }
}
