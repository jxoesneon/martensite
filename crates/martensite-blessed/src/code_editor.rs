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
}

impl CodeEditor {
    /// Creates an editor containing `text` and one cursor at the origin.
    pub fn new(text: &str) -> Self {
        Self {
            lines: text.split('\n').map(str::to_owned).collect(),
            cursors: vec![Cursor::default()],
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
    }
    /// Returns the full buffer joined by newline characters.
    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    /// Inserts text at every cursor. Newlines split the line buffer.
    pub fn insert(&mut self, text: &str) {
        let mut cursors = self.cursors.clone();
        cursors.sort_unstable_by(|a, b| b.cmp(a));
        for cursor in &mut cursors {
            *cursor = self.insert_at(*cursor, text);
        }
        cursors.sort_unstable();
        self.cursors = cursors;
    }

    /// Deletes one character before every cursor, joining lines at column zero.
    pub fn delete_backward(&mut self) {
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
            let kind = if trimmed.starts_with("//") {
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
}
