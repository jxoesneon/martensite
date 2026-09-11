//! Grapheme cluster break evaluation (UAX #29) for cursor positioning
//! and text selection.
//!
//! A *grapheme cluster* is a user-perceived character that may consist
//! of multiple Unicode code points. For example, the letter "é" can be
//! written as either a single precomposed code point (`U+00E9`) or as
//! the base letter "e" (`U+0065`) followed by the combining acute accent
//! (`U+0301`). Both forms are a single grapheme cluster, and a text
//! cursor must treat them as an indivisible unit.
//!
//! This module wraps the [`unicode_segmentation`] crate, which implements
//! the extended grapheme cluster boundary rules from
//! [Unicode Standard Annex #29](https://www.unicode.org/reports/tr29/).
//! The free functions here are the primary entry points for cursor
//! positioning and selection logic that must respect grapheme boundaries.
//!
//! # Examples
//!
//! ```
//! use martensite_text::grapheme::{grapheme_count, grapheme_clusters};
//!
//! // "é" written as "e" + combining acute is a single grapheme cluster.
//! let decomposed = "e\u{0301}";
//! assert_eq!(grapheme_count(decomposed), 1);
//! assert_eq!(grapheme_clusters(decomposed).len(), 1);
//!
//! // A family emoji is a single grapheme cluster despite being 7 code points.
//! let family = "👨\u{200d}👩\u{200d}👧\u{200d}👦";
//! assert_eq!(grapheme_count(family), 1);
//! ```

use unicode_segmentation::{Graphemes, UnicodeSegmentation};

/// A grapheme cluster breaker that wraps a [`unicode_segmentation::Graphemes`]
/// iterator over a borrowed string slice.
///
/// This struct is the owned, reusable form of the free functions in this
/// module. It borrows the source text for its lifetime and exposes the
/// underlying iterator via [`GraphemeBreaker::iter`].
///
/// # Examples
///
/// ```
/// use martensite_text::grapheme::GraphemeBreaker;
///
/// let breaker = GraphemeBreaker::new("hello");
/// assert_eq!(breaker.count(), 5);
/// ```
pub struct GraphemeBreaker<'a> {
    text: &'a str,
    graphemes: Graphemes<'a>,
}

impl<'a> GraphemeBreaker<'a> {
    /// Creates a new breaker over `text`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_text::grapheme::GraphemeBreaker;
    ///
    /// let b = GraphemeBreaker::new("abc");
    /// assert_eq!(b.count(), 3);
    /// ```
    pub fn new(text: &'a str) -> Self {
        Self {
            text,
            graphemes: text.graphemes(true),
        }
    }

    /// Returns the underlying text slice.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_text::grapheme::GraphemeBreaker;
    ///
    /// let breaker = GraphemeBreaker::new("hello");
    /// assert_eq!(breaker.text(), "hello");
    /// ```
    pub fn text(&self) -> &'a str {
        self.text
    }

    /// Returns a mutable reference to the underlying grapheme iterator.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_text::grapheme::GraphemeBreaker;
    ///
    /// let mut breaker = GraphemeBreaker::new("ab");
    /// let iter = breaker.iter();
    /// assert_eq!(iter.next(), Some("a"));
    /// ```
    pub fn iter(&mut self) -> &mut Graphemes<'a> {
        &mut self.graphemes
    }

    /// Splits the text into grapheme clusters, borrowing from the source.
    ///
    /// This is the owned-breaker equivalent of [`grapheme_clusters`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_text::grapheme::GraphemeBreaker;
    ///
    /// let mut breaker = GraphemeBreaker::new("e\u{0301}x");
    /// assert_eq!(breaker.clusters(), ["e\u{0301}", "x"]);
    /// ```
    pub fn clusters(&mut self) -> Vec<&'a str> {
        self.graphemes.clone().collect()
    }

    /// Counts the grapheme clusters in the text.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_text::grapheme::GraphemeBreaker;
    ///
    /// let breaker = GraphemeBreaker::new("e\u{0301}x");
    /// assert_eq!(breaker.count(), 2);
    /// ```
    pub fn count(&self) -> usize {
        self.text.graphemes(true).count()
    }
}

/// Splits `text` into extended grapheme clusters.
///
/// Each returned slice borrows directly from `text` (zero-copy). The
/// clusters follow the extended default grapheme cluster boundary rules
/// from UAX #29 (the `extended` flag is `true`).
///
/// # Examples
///
/// ```
/// use martensite_text::grapheme::grapheme_clusters;
///
/// let clusters = grapheme_clusters("ab");
/// assert_eq!(clusters, ["a", "b"]);
/// ```
pub fn grapheme_clusters(text: &str) -> Vec<&str> {
    text.graphemes(true).collect()
}

/// Counts the number of grapheme clusters in `text`.
///
/// This is the count a text cursor should use for "number of characters"
/// semantics, since it treats combining marks and emoji sequences as a
/// single unit.
///
/// # Examples
///
/// ```
/// use martensite_text::grapheme::grapheme_count;
///
/// assert_eq!(grapheme_count("hello"), 5);
/// assert_eq!(grapheme_count(""), 0);
/// ```
pub fn grapheme_count(text: &str) -> usize {
    text.graphemes(true).count()
}

/// Returns the `index`-th grapheme cluster of `text`, or `None` if the
/// index is out of bounds.
///
/// # Examples
///
/// ```
/// use martensite_text::grapheme::grapheme_at;
///
/// assert_eq!(grapheme_at("abc", 0), Some("a"));
/// assert_eq!(grapheme_at("abc", 2), Some("c"));
/// assert_eq!(grapheme_at("abc", 3), None);
/// ```
pub fn grapheme_at(text: &str, index: usize) -> Option<&str> {
    text.graphemes(true).nth(index)
}

/// Returns `true` if `byte_offset` lies on a grapheme cluster boundary
/// within `text`.
///
/// A byte offset of `0` and `text.len()` are always boundaries. Offsets
/// in the middle of a multi-code-point grapheme cluster (e.g. between a
/// base character and its combining mark) return `false`.
///
/// Out-of-range offsets (greater than `text.len()`) return `false`.
///
/// # Examples
///
/// ```
/// use martensite_text::grapheme::grapheme_boundary_before;
///
/// // "é" as "e" + combining acute: the only boundaries are at 0 and 3.
/// let s = "e\u{0301}";
/// assert!(grapheme_boundary_before(s, 0));
/// assert!(grapheme_boundary_before(s, 3));
/// assert!(!grapheme_boundary_before(s, 1)); // mid-cluster
/// assert!(!grapheme_boundary_before(s, 2)); // mid-cluster (inside combining mark)
/// ```
pub fn grapheme_boundary_before(text: &str, byte_offset: usize) -> bool {
    if byte_offset == 0 {
        return true;
    }
    if byte_offset > text.len() {
        return false;
    }
    // The end of the string is always a boundary.
    if byte_offset == text.len() {
        return true;
    }
    // A byte offset is a grapheme boundary iff it is the start of some
    // grapheme cluster. Walk the clusters and check whether any starts at
    // exactly `byte_offset`.
    text.grapheme_indices(true)
        .any(|(start, _)| start == byte_offset)
}

/// Finds the next grapheme cluster boundary strictly *after* `byte_offset`.
///
/// Returns the byte offset of the boundary, or `None` if there is no
/// boundary after `byte_offset` (i.e. `byte_offset` is at or past the end
/// of the text).
///
/// `byte_offset` is clamped to `text.len()`; offsets beyond the end
/// return `None`.
///
/// # Examples
///
/// ```
/// use martensite_text::grapheme::next_grapheme_boundary;
///
/// assert_eq!(next_grapheme_boundary("abc", 0), Some(1));
/// assert_eq!(next_grapheme_boundary("abc", 2), Some(3));
/// assert_eq!(next_grapheme_boundary("abc", 3), None);
/// ```
pub fn next_grapheme_boundary(text: &str, byte_offset: usize) -> Option<usize> {
    let offset = byte_offset.min(text.len());
    // Iterate grapheme clusters from the start of the string and find the
    // first boundary strictly greater than `byte_offset`. We cannot slice
    // at `byte_offset` directly because it may fall inside a multi-byte
    // UTF-8 sequence (e.g. inside a combining mark).
    text.grapheme_indices(true)
        .map(|(start, _)| start)
        .find(|&start| start > offset)
        .or_else(|| {
            // If no boundary was found, the end of the string is a boundary
            // only if it is strictly after `byte_offset`.
            (text.len() > offset).then_some(text.len())
        })
}

/// Finds the previous grapheme cluster boundary strictly *before*
/// `byte_offset`.
///
/// Returns the byte offset of the boundary, or `None` if there is no
/// boundary before `byte_offset` (i.e. `byte_offset` is `0`).
///
/// `byte_offset` is clamped to `text.len()`; offsets beyond the end are
/// treated as the end.
///
/// # Examples
///
/// ```
/// use martensite_text::grapheme::prev_grapheme_boundary;
///
/// assert_eq!(prev_grapheme_boundary("abc", 3), Some(2));
/// assert_eq!(prev_grapheme_boundary("abc", 1), Some(0));
/// assert_eq!(prev_grapheme_boundary("abc", 0), None);
/// ```
pub fn prev_grapheme_boundary(text: &str, byte_offset: usize) -> Option<usize> {
    let end = byte_offset.min(text.len());
    if end == 0 {
        return None;
    }
    // Collect the start offsets of all graphemes that begin strictly
    // before `end`; the largest such offset is the previous boundary.
    let mut prev: Option<usize> = None;
    for (start, _) in text.grapheme_indices(true) {
        if start >= end {
            break;
        }
        prev = Some(start);
    }
    prev
}

/// Returns the byte offset where the `index`-th grapheme cluster begins,
/// or `None` if the index is out of bounds.
///
/// This is useful for translating a grapheme index (cursor position) back
/// into a byte offset for string slicing.
///
/// # Examples
///
/// ```
/// use martensite_text::grapheme::grapheme_byte_offset;
///
/// let s = "e\u{0301}x";
/// assert_eq!(grapheme_byte_offset(s, 0), Some(0));
/// assert_eq!(grapheme_byte_offset(s, 1), Some(3));
/// assert_eq!(grapheme_byte_offset(s, 2), None);
/// ```
pub fn grapheme_byte_offset(text: &str, index: usize) -> Option<usize> {
    text.grapheme_indices(true)
        .nth(index)
        .map(|(start, _)| start)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- ASCII ----

    #[test]
    fn ascii_clusters() {
        let clusters = grapheme_clusters("hello");
        assert_eq!(clusters, ["h", "e", "l", "l", "o"]);
    }

    #[test]
    fn ascii_count() {
        assert_eq!(grapheme_count("hello"), 5);
    }

    #[test]
    fn ascii_grapheme_at() {
        assert_eq!(grapheme_at("hello", 0), Some("h"));
        assert_eq!(grapheme_at("hello", 4), Some("o"));
        assert_eq!(grapheme_at("hello", 5), None);
    }

    #[test]
    fn ascii_boundaries() {
        let s = "abc";
        assert!(grapheme_boundary_before(s, 0));
        assert!(grapheme_boundary_before(s, 1));
        assert!(grapheme_boundary_before(s, 2));
        assert!(grapheme_boundary_before(s, 3));
        assert!(!grapheme_boundary_before(s, 4));
    }

    #[test]
    fn ascii_next_boundary() {
        let s = "abc";
        assert_eq!(next_grapheme_boundary(s, 0), Some(1));
        assert_eq!(next_grapheme_boundary(s, 1), Some(2));
        assert_eq!(next_grapheme_boundary(s, 2), Some(3));
        assert_eq!(next_grapheme_boundary(s, 3), None);
    }

    #[test]
    fn ascii_prev_boundary() {
        let s = "abc";
        assert_eq!(prev_grapheme_boundary(s, 3), Some(2));
        assert_eq!(prev_grapheme_boundary(s, 2), Some(1));
        assert_eq!(prev_grapheme_boundary(s, 1), Some(0));
        assert_eq!(prev_grapheme_boundary(s, 0), None);
    }

    // ---- Combining characters ----

    #[test]
    fn combining_acute_is_single_cluster() {
        // "é" as "e" + combining acute (U+0301).
        let s = "e\u{0301}";
        assert_eq!(s.len(), 3); // 1 + 2 bytes
        assert_eq!(grapheme_count(s), 1);
        let clusters = grapheme_clusters(s);
        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0], s);
    }

    #[test]
    fn combining_acute_boundary_mid_cluster_is_false() {
        let s = "e\u{0301}";
        // "e\u{0301}" is 3 bytes: 1 for 'e' + 2 for combining acute (U+0301).
        // It's a single grapheme cluster, so boundaries are at 0 and 3.
        assert!(!grapheme_boundary_before(s, 1)); // mid-cluster (between e and combining mark bytes)
        assert!(!grapheme_boundary_before(s, 2)); // mid-cluster (inside combining mark UTF-8 bytes)
        assert!(grapheme_boundary_before(s, 0)); // start
        assert!(grapheme_boundary_before(s, 3)); // end of cluster
    }

    #[test]
    fn combining_acute_next_and_prev_boundary() {
        let s = "e\u{0301}x";
        // "e\u{0301}" is 3 bytes, "x" is 1 byte. Boundaries at 0, 3, 4.
        assert_eq!(next_grapheme_boundary(s, 0), Some(3));
        assert_eq!(next_grapheme_boundary(s, 1), Some(3));
        assert_eq!(next_grapheme_boundary(s, 2), Some(3));
        assert_eq!(next_grapheme_boundary(s, 3), Some(4));
        assert_eq!(next_grapheme_boundary(s, 4), None);
        assert_eq!(prev_grapheme_boundary(s, 4), Some(3));
        assert_eq!(prev_grapheme_boundary(s, 3), Some(0));
        assert_eq!(prev_grapheme_boundary(s, 1), Some(0));
    }

    // ---- Emoji sequences ----

    #[test]
    fn family_emoji_is_single_cluster() {
        // 👨‍👩‍👧‍👦 family: man + ZWJ + woman + ZWJ + girl + ZWJ + boy
        let family = "👨\u{200d}👩\u{200d}👧\u{200d}👦";
        assert_eq!(grapheme_count(family), 1);
        let clusters = grapheme_clusters(family);
        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0], family);
    }

    #[test]
    fn emoji_with_skin_tone_modifier_is_single_cluster() {
        // 👍🏽 (thumbs up + emoji modifier fitzpatrick type-4)
        let s = "👍\u{1f3fd}";
        assert_eq!(grapheme_count(s), 1);
    }

    #[test]
    fn multiple_emoji_clusters() {
        let s = "👋🌍"; // two emoji
        assert_eq!(grapheme_count(s), 2);
    }

    #[test]
    fn emoji_boundary_before_mid_sequence_is_false() {
        let family = "👨\u{200d}👩\u{200d}👧\u{200d}👦";
        // The ZWJ joins the code points; the only boundaries are 0 and len.
        let len = family.len();
        assert!(grapheme_boundary_before(family, 0));
        assert!(grapheme_boundary_before(family, len));
        // Any interior offset should not be a boundary.
        for i in 1..len {
            assert!(
                !grapheme_boundary_before(family, i),
                "offset {i} should not be a boundary"
            );
        }
    }

    // ---- CJK characters ----

    #[test]
    fn cjk_each_code_point_is_its_own_cluster() {
        let s = "你好";
        assert_eq!(grapheme_count(s), 2);
        let clusters = grapheme_clusters(s);
        assert_eq!(clusters, ["你", "好"]);
    }

    #[test]
    fn cjk_boundaries() {
        let s = "你好";
        // 你 is 3 bytes, 好 is 3 bytes.
        assert!(grapheme_boundary_before(s, 0));
        assert!(grapheme_boundary_before(s, 3));
        assert!(grapheme_boundary_before(s, 6));
        assert!(!grapheme_boundary_before(s, 1));
        assert!(!grapheme_boundary_before(s, 2));
    }

    // ---- Mixed scripts ----

    #[test]
    fn mixed_scripts() {
        // Latin + combining + CJK + emoji
        let s = "e\u{0301}你👋";
        // clusters: "é", "你", "👋"
        assert_eq!(grapheme_count(s), 3);
        let clusters = grapheme_clusters(s);
        assert_eq!(clusters[0], "e\u{0301}");
        assert_eq!(clusters[1], "你");
        assert_eq!(clusters[2], "👋");
    }

    // ---- Empty string ----

    #[test]
    fn empty_string() {
        assert_eq!(grapheme_count(""), 0);
        assert!(grapheme_clusters("").is_empty());
        assert_eq!(grapheme_at("", 0), None);
        assert!(grapheme_boundary_before("", 0));
        assert!(!grapheme_boundary_before("", 1));
        assert_eq!(next_grapheme_boundary("", 0), None);
        assert_eq!(prev_grapheme_boundary("", 0), None);
    }

    // ---- Cursor positioning at grapheme boundaries ----

    #[test]
    fn cursor_advances_by_grapheme_not_codepoint() {
        let s = "e\u{0301}x";
        // "e\u{0301}" is 3 bytes (single grapheme), "x" is 1 byte.
        // A grapheme-aware cursor moves 0 -> 3 -> 4.
        let mut pos = 0;
        let mut visited = vec![pos];
        while let Some(next) = next_grapheme_boundary(s, pos) {
            pos = next;
            visited.push(pos);
        }
        assert_eq!(visited, vec![0, 3, 4]);
    }

    #[test]
    fn cursor_retreats_by_grapheme() {
        let s = "e\u{0301}x";
        let mut pos = s.len();
        let mut visited = vec![pos];
        while let Some(prev) = prev_grapheme_boundary(s, pos) {
            pos = prev;
            visited.push(pos);
        }
        visited.reverse();
        assert_eq!(visited, vec![0, 3, 4]);
    }

    #[test]
    fn grapheme_byte_offset_helper() {
        let s = "e\u{0301}x";
        // Grapheme 0 starts at byte 0, grapheme 1 ("x") starts at byte 3.
        assert_eq!(grapheme_byte_offset(s, 0), Some(0));
        assert_eq!(grapheme_byte_offset(s, 1), Some(3));
        assert_eq!(grapheme_byte_offset(s, 2), None);
    }

    #[test]
    fn breaker_struct() {
        let mut b = GraphemeBreaker::new("e\u{0301}x");
        assert_eq!(b.count(), 2);
        assert_eq!(b.clusters(), ["e\u{0301}", "x"]);
        assert_eq!(b.text(), "e\u{0301}x");
    }

    #[test]
    fn out_of_range_offsets() {
        let s = "abc";
        assert!(!grapheme_boundary_before(s, 100));
        assert_eq!(next_grapheme_boundary(s, 100), None);
        // prev clamps to len, then finds the previous boundary.
        assert_eq!(prev_grapheme_boundary(s, 100), Some(2));
    }
}
