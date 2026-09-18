//! A line-based, multi-cursor code editor.

/// A zero-based text cursor.
///
/// # Examples
///
/// ```
/// use martensite_blessed::Cursor;
/// assert_eq!(Cursor::new(2, 4).line, 2);
/// ```
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub struct Cursor {
    /// Zero-based line index.
    pub line: usize,
    /// Zero-based character index within the line.
    pub column: usize,
}
impl Cursor {
    /// Creates a cursor.
    pub const fn new(line: usize, column: usize) -> Self {
        Self { line, column }
    }
}

/// A coarse syntax token kind.
///
/// # Examples
///
/// ```
/// use martensite_blessed::TokenKind;
/// assert_eq!(TokenKind::Keyword, TokenKind::Keyword);
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TokenKind {
    /// Language keyword.
    Keyword,
    /// Quoted string.
    String,
    /// Line comment.
    Comment,
    /// Numeric literal.
    Number,
    /// Unclassified source text.
    Plain,
}

/// A syntax-highlighted character range.
///
/// # Examples
///
/// ```
/// use martensite_blessed::{HighlightedSpan, TokenKind};
/// let span = HighlightedSpan { start: 0, end: 2, kind: TokenKind::Keyword };
/// assert_eq!(span.end, 2);
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HighlightedSpan {
    /// Start byte offset.
    pub start: usize,
    /// End byte offset.
    pub end: usize,
    /// Token classification.
    pub kind: TokenKind,
}

/// A syntax-highlighted line editor with multiple cursors.
///
/// # Examples
///
/// ```
/// use martensite_blessed::{CodeEditor, Cursor};
/// let mut editor = CodeEditor::new("one\ntwo");
/// editor.set_cursors(vec![Cursor::new(0, 0), Cursor::new(1, 0)]);
/// editor.insert("_");
/// assert_eq!(editor.text(), "_one\n_two");
/// ```
#[derive(Clone, Debug)]
pub struct CodeEditor {
    lines: Vec<String>,
    cursors: Vec<Cursor>,
    /// Selection anchor paired with the primary cursor — `Some` means
    /// the range between it and `cursors[0]` is selected. The model is
    /// single-selection: secondary cursors never carry ranges of their
    /// own, and [`set_cursors`](Self::set_cursors) collapses the
    /// selection.
    sel_anchor: Option<Cursor>,
}

impl CodeEditor {
    /// Creates an editor containing `text` and one cursor at the origin.
    pub fn new(text: &str) -> Self {
        Self {
            lines: text.split('\n').map(str::to_owned).collect(),
            cursors: vec![Cursor::default()],
            sel_anchor: None,
        }
    }
    /// Returns the line buffer.
    pub fn lines(&self) -> &[String] {
        &self.lines
    }
    /// Returns all cursors.
    pub fn cursors(&self) -> &[Cursor] {
        &self.cursors
    }
    /// Replaces cursors, deduplicating and clamping their positions.
    pub fn set_cursors(&mut self, mut cursors: Vec<Cursor>) {
        if cursors.is_empty() {
            cursors.push(Cursor::default());
        }
        for cursor in &mut cursors {
            self.clamp_cursor(cursor);
        }
        cursors.sort_unstable();
        cursors.dedup();
        self.cursors = cursors;
        // A caret move collapses the selection — the anchor belongs to
        // the cursor it was set against.
        self.sel_anchor = None;
    }

    /// The selected range as `(start, end)`, or `None` when the
    /// selection is empty or collapsed. Selection is anchored on the
    /// primary cursor; secondary cursors do not select.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::{CodeEditor, Cursor};
    /// let mut editor = CodeEditor::new("alpha");
    /// assert_eq!(editor.selection(), None);
    /// editor.extend_selection_to(Cursor::new(0, 3));
    /// assert_eq!(editor.selection(), Some((Cursor::new(0, 0), Cursor::new(0, 3))));
    /// ```
    pub fn selection(&self) -> Option<(Cursor, Cursor)> {
        let anchor = self.sel_anchor?;
        let head = *self.cursors.first()?;
        (anchor != head).then(|| (anchor.min(head), anchor.max(head)))
    }

    /// Extends the selection to `head`, seeding the anchor at the
    /// current primary caret when no selection is open — the
    /// shift-click / shift-arrow primitive.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::{CodeEditor, Cursor};
    /// let mut editor = CodeEditor::new("alpha\nbeta");
    /// editor.set_cursors(vec![Cursor::new(0, 2)]);
    /// editor.extend_selection_to(Cursor::new(1, 1));
    /// assert_eq!(editor.selected_text().as_deref(), Some("pha\nb"));
    /// ```
    pub fn extend_selection_to(&mut self, head: Cursor) {
        if self.sel_anchor.is_none() {
            self.sel_anchor = self.cursors.first().copied();
        }
        let mut head = head;
        self.clamp_cursor(&mut head);
        self.cursors = vec![head];
    }

    /// Sets an explicit selection: `anchor` fixed, `head` becomes the
    /// primary cursor. The word- and line-drag primitive — unlike
    /// [`extend_selection_to`](Self::extend_selection_to), the anchor
    /// is specified rather than seeded from the caret.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::{CodeEditor, Cursor};
    /// let mut editor = CodeEditor::new("alpha");
    /// editor.set_selection(Cursor::new(0, 1), Cursor::new(0, 4));
    /// assert_eq!(editor.selected_text().as_deref(), Some("lph"));
    /// ```
    pub fn set_selection(&mut self, mut anchor: Cursor, mut head: Cursor) {
        self.clamp_cursor(&mut anchor);
        self.clamp_cursor(&mut head);
        self.sel_anchor = Some(anchor);
        self.cursors = vec![head];
    }

    /// Selects the word span under `pos` — the maximal run of the same
    /// character class (identifier chars `[A-Za-z0-9_]`, whitespace, or
    /// punctuation), matching code-editor double-click semantics.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::{CodeEditor, Cursor};
    /// let mut editor = CodeEditor::new("let x = 1");
    /// editor.select_word_at(Cursor::new(0, 4));
    /// assert_eq!(editor.selected_text().as_deref(), Some("x"));
    /// ```
    pub fn select_word_at(&mut self, pos: Cursor) {
        let (lo, hi) = self.word_span_at(pos);
        self.sel_anchor = Some(lo);
        self.cursors = vec![hi];
    }

    /// Selects `line` end to end, including its newline when one
    /// exists — the triple-click paragraph gesture for a line-based
    /// editor.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::{CodeEditor, Cursor};
    /// let mut editor = CodeEditor::new("one\ntwo\nthree");
    /// editor.select_line(1);
    /// assert_eq!(editor.selected_text().as_deref(), Some("two\n"));
    /// ```
    pub fn select_line(&mut self, line: usize) {
        let line = line.min(self.lines.len().saturating_sub(1));
        self.sel_anchor = Some(Cursor::new(line, 0));
        self.cursors = vec![if line + 1 < self.lines.len() {
            Cursor::new(line + 1, 0)
        } else {
            Cursor::new(line, self.lines[line].chars().count())
        }];
    }

    /// Selects the entire buffer.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::CodeEditor;
    /// let mut editor = CodeEditor::new("one\ntwo");
    /// editor.select_all();
    /// assert_eq!(editor.selected_text().as_deref(), Some("one\ntwo"));
    /// ```
    pub fn select_all(&mut self) {
        self.sel_anchor = Some(Cursor::default());
        let last = self.lines.len().saturating_sub(1);
        self.cursors = vec![Cursor::new(last, self.lines[last].chars().count())];
    }

    /// The selected text, or `None` when nothing is selected.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::CodeEditor;
    /// let mut editor = CodeEditor::new("hello world");
    /// assert_eq!(editor.selected_text(), None);
    /// editor.select_all();
    /// assert_eq!(editor.selected_text().as_deref(), Some("hello world"));
    /// ```
    pub fn selected_text(&self) -> Option<String> {
        self.selection().map(|(a, b)| self.text_in_range(a, b))
    }

    /// Returns the text in `a..b` (normalized order not required —
    /// the range is sorted internally), joining crossed lines with
    /// `\n` exactly as [`text`](Self::text) does.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::{CodeEditor, Cursor};
    /// let editor = CodeEditor::new("one\ntwo\nthree");
    /// assert_eq!(
    ///     editor.text_in_range(Cursor::new(0, 1), Cursor::new(1, 2)),
    ///     "ne\ntw",
    /// );
    /// ```
    pub fn text_in_range(&self, mut a: Cursor, mut b: Cursor) -> String {
        if a > b {
            std::mem::swap(&mut a, &mut b);
        }
        if a.line == b.line {
            let line = &self.lines[a.line];
            return line[char_byte(line, a.column)..char_byte(line, b.column)].to_string();
        }
        let mut out = self.lines[a.line][char_byte(&self.lines[a.line], a.column)..].to_string();
        for line in &self.lines[a.line + 1..b.line] {
            out.push('\n');
            out.push_str(line);
        }
        out.push('\n');
        let last = &self.lines[b.line];
        out.push_str(&last[..char_byte(last, b.column)]);
        out
    }

    /// Deletes the text in `a..b`, collapsing every cursor inside the
    /// removed span onto `a` and shifting cursors past it. Clears the
    /// selection.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::{CodeEditor, Cursor};
    /// let mut editor = CodeEditor::new("one\ntwo\nthree");
    /// editor.delete_range(Cursor::new(0, 1), Cursor::new(1, 2));
    /// assert_eq!(editor.text(), "oo\nthree");
    /// ```
    pub fn delete_range(&mut self, mut a: Cursor, mut b: Cursor) {
        if a > b {
            std::mem::swap(&mut a, &mut b);
        }
        if a == b {
            return;
        }
        if a.line == b.line {
            let line = &mut self.lines[a.line];
            line.replace_range(char_byte(line, a.column)..char_byte(line, b.column), "");
        } else {
            let start_byte = char_byte(&self.lines[a.line], a.column);
            let end_byte = char_byte(&self.lines[b.line], b.column);
            let tail = self.lines[b.line][end_byte..].to_string();
            self.lines[a.line].truncate(start_byte);
            self.lines[a.line].push_str(&tail);
            self.lines.drain(a.line + 1..=b.line);
        }
        let removed_lines = b.line - a.line;
        for c in &mut self.cursors {
            if *c > a && *c <= b {
                *c = a;
            } else if *c > b {
                *c = if c.line == b.line {
                    // Same surviving line: shift by the join.
                    Cursor::new(a.line, a.column + (c.column - b.column))
                } else {
                    Cursor::new(c.line - removed_lines, c.column)
                };
            }
        }
        // Field-level borrows: `clamp_cursor` takes `&self`, which
        // conflicts with `&mut self.cursors` — clamp inline instead.
        let max_line = self.lines.len().saturating_sub(1);
        for c in &mut self.cursors {
            c.line = c.line.min(max_line);
            c.column = c.column.min(self.lines[c.line].chars().count());
        }
        self.cursors.sort_unstable();
        self.cursors.dedup();
        self.sel_anchor = None;
    }

    /// Deletes the selection if one is open. Returns whether text was
    /// removed.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::CodeEditor;
    /// let mut editor = CodeEditor::new("hello");
    /// assert!(!editor.delete_selection());
    /// editor.select_all();
    /// assert!(editor.delete_selection());
    /// assert_eq!(editor.text(), "");
    /// ```
    pub fn delete_selection(&mut self) -> bool {
        let Some((a, b)) = self.selection() else {
            return false;
        };
        self.delete_range(a, b);
        true
    }

    /// The word span containing `pos` — the maximal run of the same
    /// character class (identifier, whitespace, or punctuation). An
    /// offset past the end of the line resolves to the last span.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::{CodeEditor, Cursor};
    /// let editor = CodeEditor::new("foo bar");
    /// assert_eq!(
    ///     editor.word_span_at(Cursor::new(0, 1)),
    ///     (Cursor::new(0, 0), Cursor::new(0, 3)),
    /// );
    /// ```
    pub fn word_span_at(&self, pos: Cursor) -> (Cursor, Cursor) {
        let line = pos.line.min(self.lines.len().saturating_sub(1));
        let chars: Vec<char> = self.lines[line].chars().collect();
        if chars.is_empty() {
            return (Cursor::new(line, 0), Cursor::new(line, 0));
        }
        let col = pos.column.min(chars.len() - 1);
        let class = word_class(chars[col]);
        let mut lo = col;
        while lo > 0 && word_class(chars[lo - 1]) == class {
            lo -= 1;
        }
        let mut hi = col + 1;
        while hi < chars.len() && word_class(chars[hi]) == class {
            hi += 1;
        }
        (Cursor::new(line, lo), Cursor::new(line, hi))
    }
    /// Returns the full buffer joined by newline characters.
    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    /// Inserts text at every cursor, replacing the selection first if
    /// one is open. Newlines split the line buffer.
    pub fn insert(&mut self, text: &str) {
        self.delete_selection();
        let mut cursors = self.cursors.clone();
        cursors.sort_unstable_by(|a, b| b.cmp(a));
        for cursor in &mut cursors {
            *cursor = self.insert_at(*cursor, text);
        }
        cursors.sort_unstable();
        self.cursors = cursors;
    }

    /// Deletes one character before every cursor, joining lines at
    /// column zero — or just the selection when one is open.
    pub fn delete_backward(&mut self) {
        if self.delete_selection() {
            return;
        }
        let mut cursors = self.cursors.clone();
        cursors.sort_unstable_by(|a, b| b.cmp(a));
        for cursor in &mut cursors {
            *cursor = self.delete_at(*cursor);
        }
        cursors.sort_unstable();
        cursors.dedup();
        self.cursors = cursors;
    }

    /// Performs lightweight highlighting for common keywords, strings, numbers, and comments.
    pub fn highlight_line(&self, line: usize) -> Vec<HighlightedSpan> {
        let Some(source) = self.lines.get(line) else {
            return Vec::new();
        };
        let mut spans = Vec::new();
        let mut offset = 0;
        for token in source.split_inclusive(char::is_whitespace) {
            let trimmed = token.trim_end();
            let kind = if trimmed.starts_with("//") || trimmed.starts_with('#') {
                TokenKind::Comment
            } else if matches!(
                trimmed,
                "fn" | "let" | "pub" | "struct" | "enum" | "impl" | "use"
            ) {
                TokenKind::Keyword
            } else if trimmed.parse::<f64>().is_ok() {
                TokenKind::Number
            } else if trimmed.starts_with('"') {
                TokenKind::String
            } else {
                TokenKind::Plain
            };
            if !trimmed.is_empty() {
                spans.push(HighlightedSpan {
                    start: offset,
                    end: offset + trimmed.len(),
                    kind,
                });
            }
            offset += token.len();
        }
        spans
    }

    fn clamp_cursor(&self, cursor: &mut Cursor) {
        cursor.line = cursor.line.min(self.lines.len().saturating_sub(1));
        cursor.column = cursor.column.min(self.lines[cursor.line].chars().count());
    }
    fn insert_at(&mut self, cursor: Cursor, text: &str) -> Cursor {
        let byte = char_byte(&self.lines[cursor.line], cursor.column);
        let suffix = self.lines[cursor.line].split_off(byte);
        let parts: Vec<&str> = text.split('\n').collect();
        self.lines[cursor.line].push_str(parts[0]);
        if parts.len() == 1 {
            self.lines[cursor.line].push_str(&suffix);
            return Cursor::new(cursor.line, cursor.column + text.chars().count());
        }
        let last = parts.len() - 1;
        for (index, part) in parts[1..].iter().enumerate() {
            let mut line = (*part).to_owned();
            if index + 1 == last {
                line.push_str(&suffix);
            }
            self.lines.insert(cursor.line + index + 1, line);
        }
        Cursor::new(cursor.line + last, parts[last].chars().count())
    }
    fn delete_at(&mut self, cursor: Cursor) -> Cursor {
        if cursor.column > 0 {
            let end = char_byte(&self.lines[cursor.line], cursor.column);
            let start = char_byte(&self.lines[cursor.line], cursor.column - 1);
            self.lines[cursor.line].replace_range(start..end, "");
            Cursor::new(cursor.line, cursor.column - 1)
        } else if cursor.line > 0 {
            let removed = self.lines.remove(cursor.line);
            let column = self.lines[cursor.line - 1].chars().count();
            self.lines[cursor.line - 1].push_str(&removed);
            Cursor::new(cursor.line - 1, column)
        } else {
            cursor
        }
    }
}

/// Character class for code word spans — identifier chars group
/// together, whitespace groups together, and each run of punctuation
/// is its own selectable unit (double-clicking `::` selects `::`).
fn word_class(c: char) -> u8 {
    if c.is_alphanumeric() || c == '_' {
        0
    } else if c.is_whitespace() {
        1
    } else {
        2
    }
}

fn char_byte(text: &str, column: usize) -> usize {
    text.char_indices()
        .nth(column)
        .map_or(text.len(), |(index, _)| index)
}

#[cfg(test)]
mod tests {
    use super::{CodeEditor, Cursor, TokenKind};
    #[test]
    fn edits_with_multiple_cursors() {
        let mut editor = CodeEditor::new("alpha\nbeta");
        editor.set_cursors(vec![Cursor::new(0, 5), Cursor::new(1, 4)]);
        editor.insert("!");
        assert_eq!(editor.text(), "alpha!\nbeta!");
        editor.delete_backward();
        assert_eq!(editor.text(), "alpha\nbeta");
    }
    #[test]
    fn highlights_keyword() {
        let editor = CodeEditor::new("pub fn main");
        assert_eq!(editor.highlight_line(0)[0].kind, TokenKind::Keyword);
    }

    #[test]
    fn insert_replaces_selection() {
        let mut editor = CodeEditor::new("hello world");
        editor.select_word_at(Cursor::new(0, 7));
        editor.insert("there");
        assert_eq!(editor.text(), "hello there");
        assert_eq!(editor.selection(), None);
        assert_eq!(editor.cursors(), &[Cursor::new(0, 11)]);
    }

    #[test]
    fn backspace_deletes_selection_only() {
        let mut editor = CodeEditor::new("abc def");
        editor.select_word_at(Cursor::new(0, 5));
        editor.delete_backward();
        assert_eq!(editor.text(), "abc ");
        // The caret sits at the selection's start — the deleted
        // word's left edge.
        assert_eq!(editor.cursors(), &[Cursor::new(0, 4)]);
    }

    #[test]
    fn delete_range_adjusts_cursors_past_the_gap() {
        let mut editor = CodeEditor::new("aa\nbb\ncc");
        editor.set_cursors(vec![Cursor::new(2, 1)]);
        editor.delete_range(Cursor::new(0, 0), Cursor::new(1, 1));
        // "aa\nb" is removed — line 0 keeps only the tail "b" of
        // line 1, and "cc" moves up to line 1.
        assert_eq!(editor.text(), "b\ncc");
        assert_eq!(editor.cursors(), &[Cursor::new(1, 1)]);
    }

    #[test]
    fn word_span_groups_punctuation_and_whitespace_runs() {
        let editor = CodeEditor::new("a::b c");
        // Identifier run, punctuation run, whitespace run.
        assert_eq!(
            editor.word_span_at(Cursor::new(0, 1)),
            (Cursor::new(0, 1), Cursor::new(0, 3))
        );
        assert_eq!(
            editor.word_span_at(Cursor::new(0, 4)),
            (Cursor::new(0, 4), Cursor::new(0, 5))
        );
    }

    #[test]
    fn extend_selection_seeds_anchor_from_caret() {
        let mut editor = CodeEditor::new("one\ntwo");
        editor.set_cursors(vec![Cursor::new(0, 1)]);
        editor.extend_selection_to(Cursor::new(1, 2));
        assert_eq!(
            editor.selection(),
            Some((Cursor::new(0, 1), Cursor::new(1, 2)))
        );
        // A plain caret move collapses it.
        editor.set_cursors(vec![Cursor::new(1, 2)]);
        assert_eq!(editor.selection(), None);
    }
}
