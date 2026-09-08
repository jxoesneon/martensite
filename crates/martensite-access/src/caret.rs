//! Screen reader caret tracking and text selection updates for AccessKit.
//!
//! This module provides:
//! - Caret tracking across text nodes ([`CaretTracker`]).
//! - Selection representations with anchor, focus, and affinity ([`TextSelection`], [`TextAffinity`]).
//! - Text navigation boundary detection ([`TextBoundary`]).
//! - Updating [`accesskit::Node`] instances with current text selections.

use accesskit::{Node, NodeId, Rect, TextPosition, TextSelection as AccessKitSelection};

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

    /// Returns the normalized byte or character range `min(anchor, focus)..max(anchor, focus)`.
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
    /// Individual Unicode character or grapheme cluster boundary.
    #[default]
    Character,
    /// Word boundary delimited by whitespace and punctuation.
    Word,
    /// Line boundary delimited by line break (`\n`).
    Line,
    /// Paragraph boundary delimited by newline sequences.
    Paragraph,
    /// Entire document or text boundary (`0..text.len()`).
    Document,
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
        if self.character_bounds.is_empty() {
            return None;
        }

        let focus = self.selection.focus;
        if focus < self.character_bounds.len() {
            let r = self.character_bounds[focus];
            if self.selection.affinity == TextAffinity::Upstream && focus > 0 {
                let prev = self.character_bounds[focus - 1];
                Some(Rect::new(prev.x1, prev.y0, prev.x1, prev.y1))
            } else {
                Some(Rect::new(r.x0, r.y0, r.x0, r.y1))
            }
        } else {
            let last = self.character_bounds[self.character_bounds.len() - 1];
            Some(Rect::new(last.x1, last.y0, last.x1, last.y1))
        }
    }

    /// Finds the byte range for the specified [`TextBoundary`] surrounding `index`.
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
    pub fn find_boundary_range(text: &str, index: usize, boundary: TextBoundary) -> (usize, usize) {
        if text.is_empty() {
            return (0, 0);
        }

        let mut idx = index.min(text.len());
        while idx > 0 && !text.is_char_boundary(idx) {
            idx -= 1;
        }

        match boundary {
            TextBoundary::Character => {
                if idx >= text.len() {
                    return (text.len(), text.len());
                }
                let mut start = idx;
                while start > 0 && !text.is_char_boundary(start) {
                    start -= 1;
                }
                let mut end = start;
                if let Some(ch) = text[start..].chars().next() {
                    end += ch.len_utf8();
                }
                (start, end)
            }

            TextBoundary::Word => {
                if idx >= text.len() {
                    return (text.len(), text.len());
                }

                let char_indices: Vec<(usize, char)> = text.char_indices().collect();
                let pos = match char_indices.binary_search_by_key(&idx, |&(i, _)| i) {
                    Ok(p) => p,
                    Err(p) => p.saturating_sub(1),
                };

                let is_word_char = |c: char| c.is_alphanumeric() || c == '_';
                let target_type = is_word_char(char_indices[pos].1);

                let mut start_pos = pos;
                while start_pos > 0 && is_word_char(char_indices[start_pos - 1].1) == target_type {
                    start_pos -= 1;
                }

                let mut end_pos = pos;
                while end_pos < char_indices.len()
                    && is_word_char(char_indices[end_pos].1) == target_type
                {
                    end_pos += 1;
                }

                let start = char_indices[start_pos].0;
                let end = if end_pos < char_indices.len() {
                    char_indices[end_pos].0
                } else {
                    text.len()
                };

                (start, end)
            }

            TextBoundary::Line => {
                let start = text[..idx].rfind('\n').map_or(0, |i| i + 1);
                let end = text[idx..].find('\n').map_or(text.len(), |i| idx + i);
                (start, end)
            }

            TextBoundary::Paragraph => {
                // Paragraphs delimited by double newlines or single newlines
                let has_double = text.contains("\n\n");
                let delim = if has_double { "\n\n" } else { "\n" };

                let start = text[..idx].rfind(delim).map_or(0, |i| i + delim.len());
                let end = text[idx..].find(delim).map_or(text.len(), |i| idx + i);
                (start, end)
            }

            TextBoundary::Document => (0, text.len()),
        }
    }

    /// Applies the current selection state to the specified AccessKit [`Node`].
    ///
    /// Sets the node's `text_selection` property referencing `self.node_id` and the
    /// anchor/focus offsets.
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
    fn test_boundary_detection_multibyte_unaligned() {
        let text = "日本語\n世界";
        // Byte index 1 is inside '日' (3 bytes in UTF-8: 0xE6, 0x97, 0xA5)
        let (start, end) = CaretTracker::find_boundary_range(text, 1, TextBoundary::Line);
        assert_eq!(start, 0);
        assert_eq!(end, "日本語".len());

        let (p_start, p_end) = CaretTracker::find_boundary_range(text, 1, TextBoundary::Paragraph);
        assert_eq!(p_start, 0);
        assert_eq!(p_end, "日本語".len());
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
}
