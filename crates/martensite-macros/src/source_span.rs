//! Source span tracking and live-tweak source patch generation.
//!
//! This module provides infrastructure for capturing callsite source locations
//! via `#[track_caller]` and generating source patches for live property tweaks
//! (e.g. `src/ui.rs:142: .padding(12.0) -> .padding(16.0)`), as specified in
//! `docs/dx/LIVE_TWEAKS.md`.
//!
//! When the `devtools-source-spans` feature is disabled, all tracking code is
//! compiled out with zero runtime overhead.

#![allow(dead_code)]

use proc_macro::{Delimiter, Group, Ident, Punct, Spacing, Span, TokenStream, TokenTree};

/// A source code location (file path, 1-indexed line, and 1-indexed column).
///
/// # Examples
///
/// ```ignore
/// use martensite_macros::source_span::SourceSpan;
///
/// let span = SourceSpan::new("src/ui.rs", 142, 5);
/// assert_eq!(span.file_line(), "src/ui.rs:142");
/// assert_eq!(span.display(), "src/ui.rs:142:5");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[allow(dead_code)]
pub struct SourceSpan {
    /// File path where the callsite or declaration occurred.
    pub file: String,
    /// 1-indexed line number.
    pub line: u32,
    /// 1-indexed column number.
    pub column: u32,
}

impl SourceSpan {
    /// Constructs a new [`SourceSpan`].
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use martensite_macros::source_span::SourceSpan;
    ///
    /// let span = SourceSpan::new("src/main.rs", 10, 1);
    /// assert_eq!(span.line, 10);
    /// ```
    pub fn new(file: impl Into<String>, line: u32, column: u32) -> Self {
        Self {
            file: file.into(),
            line,
            column,
        }
    }

    /// Formats the span as `file:line:col`.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use martensite_macros::source_span::SourceSpan;
    ///
    /// let span = SourceSpan::new("src/main.rs", 10, 4);
    /// assert_eq!(span.display(), "src/main.rs:10:4");
    /// ```
    pub fn display(&self) -> String {
        format!("{}:{}:{}", self.file, self.line, self.column)
    }

    /// Formats the span as `file:line`.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use martensite_macros::source_span::SourceSpan;
    ///
    /// let span = SourceSpan::new("src/main.rs", 10, 4);
    /// assert_eq!(span.file_line(), "src/main.rs:10");
    /// ```
    pub fn file_line(&self) -> String {
        format!("{}:{}", self.file, self.line)
    }

    /// Captures the caller's location using [`std::panic::Location::caller`].
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use martensite_macros::source_span::SourceSpan;
    ///
    /// let span = SourceSpan::from_caller();
    /// assert!(span.line > 0);
    /// ```
    #[track_caller]
    pub fn from_caller() -> Self {
        let caller = std::panic::Location::caller();
        Self::new(caller.file(), caller.line(), caller.column())
    }
}

/// A source code patch description for write-back of live tweaks.
///
/// Formats patches in the standard Martensite CLI patch convention:
/// `src/ui.rs:142: .padding(12.0) -> .padding(16.0)`
///
/// # Examples
///
/// ```ignore
/// use martensite_macros::source_span::{SourceSpan, SourcePatch};
///
/// let span = SourceSpan::new("src/ui.rs", 142, 5);
/// let patch = SourcePatch::new(span, "padding", "12.0", "16.0");
/// assert_eq!(patch.format_patch(), "src/ui.rs:142: .padding(12.0) -> .padding(16.0)");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourcePatch {
    /// Location of the callsite.
    pub span: SourceSpan,
    /// Name of the builder method or property being patched (e.g. `padding`).
    pub method_or_prop: String,
    /// The original compiled-in literal value.
    pub old_value: String,
    /// The new live-tweaked value.
    pub new_value: String,
}

impl SourcePatch {
    /// Constructs a new [`SourcePatch`].
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use martensite_macros::source_span::{SourceSpan, SourcePatch};
    ///
    /// let span = SourceSpan::new("src/ui.rs", 10, 1);
    /// let patch = SourcePatch::new(span, "margin", "4.0", "8.0");
    /// assert_eq!(patch.old_value, "4.0");
    /// ```
    pub fn new(
        span: SourceSpan,
        method_or_prop: impl Into<String>,
        old_value: impl Into<String>,
        new_value: impl Into<String>,
    ) -> Self {
        Self {
            span,
            method_or_prop: method_or_prop.into(),
            old_value: old_value.into(),
            new_value: new_value.into(),
        }
    }

    /// Formats the patch line in the canonical format:
    /// `file:line: .method(old) -> .method(new)`
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use martensite_macros::source_span::{SourceSpan, SourcePatch};
    ///
    /// let span = SourceSpan::new("src/view.rs", 50, 12);
    /// let patch = SourcePatch::new(span, "gap", "8.0", "16.0");
    /// assert_eq!(patch.format_patch(), "src/view.rs:50: .gap(8.0) -> .gap(16.0)");
    /// ```
    pub fn format_patch(&self) -> String {
        let method = self.method_or_prop.trim_start_matches('.');
        format!(
            "{}:{}: .{}({}) -> .{}({})",
            self.span.file, self.span.line, method, self.old_value, method, self.new_value
        )
    }
}

/// Convenience helper to format a source patch string directly from individual values.
///
/// # Examples
///
/// ```ignore
/// use martensite_macros::source_span::format_source_patch;
///
/// let patch_str = format_source_patch("src/ui.rs", 142, "padding", "12.0", "16.0");
/// assert_eq!(patch_str, "src/ui.rs:142: .padding(12.0) -> .padding(16.0)");
/// ```
pub fn format_source_patch(
    file: &str,
    line: u32,
    method: &str,
    old_value: &str,
    new_value: &str,
) -> String {
    let method = method.trim_start_matches('.');
    format!("{file}:{line}: .{method}({old_value}) -> .{method}({new_value})")
}

/// Returns `"#[track_caller]\n"` if source spans are enabled, or an empty string otherwise.
///
/// # Examples
///
/// ```ignore
/// use martensite_macros::source_span::track_caller_attr;
///
/// assert_eq!(track_caller_attr(true), "#[track_caller]\n");
/// assert_eq!(track_caller_attr(false), "");
/// ```
pub fn track_caller_attr(enabled: bool) -> &'static str {
    if enabled {
        "#[track_caller]\n"
    } else {
        ""
    }
}

/// Determines whether source spans tracking should be enabled.
///
/// Respects an explicit override (e.g. from macro attributes `enabled = true/false`),
/// falling back to the `devtools-source-spans` or `devtools` cargo features.
///
/// # Examples
///
/// ```ignore
/// use martensite_macros::source_span::is_source_spans_enabled;
///
/// assert!(is_source_spans_enabled(Some(true)));
/// assert!(!is_source_spans_enabled(Some(false)));
/// ```
#[allow(clippy::manual_unwrap_or_default)]
pub fn is_source_spans_enabled(explicit: Option<bool>) -> bool {
    explicit.unwrap_or(cfg!(feature = "devtools-source-spans") || cfg!(feature = "devtools"))
}

/// Expands the `#[source_span]` procedural attribute macro.
///
/// Annotates functions or methods with `#[track_caller]` when source span tracking is active.
///
/// # Examples
///
/// In procedural macro contexts:
///
/// ```ignore
/// #[source_span]
/// fn build_widget() { ... }
/// ```
pub fn expand_source_span(attr: TokenStream, item: TokenStream) -> TokenStream {
    let explicit = parse_enabled_override(attr);
    let enabled = is_source_spans_enabled(explicit);

    if !enabled {
        return item;
    }

    // Prepend #[track_caller] to the item.
    let mut out = TokenStream::new();

    // #[track_caller]
    let hash = Punct::new('#', Spacing::Alone);
    let track_caller_ident = Ident::new("track_caller", Span::call_site());
    let mut group_stream = TokenStream::new();
    group_stream.extend([TokenTree::Ident(track_caller_ident)]);
    let bracket_group = Group::new(Delimiter::Bracket, group_stream);

    out.extend([TokenTree::Punct(hash), TokenTree::Group(bracket_group)]);
    out.extend(item);

    out
}

/// Parses an optional `enabled = bool` from macro attribute tokens.
fn parse_enabled_override(attr: TokenStream) -> Option<bool> {
    let mut tokens = attr.into_iter().peekable();
    while let Some(tt) = tokens.next() {
        if let TokenTree::Ident(ident) = tt {
            if ident.to_string() == "enabled" {
                if let Some(TokenTree::Punct(p)) = tokens.next() {
                    if p.as_char() == '=' {
                        if let Some(TokenTree::Ident(val_ident)) = tokens.next() {
                            let val_str = val_ident.to_string();
                            if val_str == "true" {
                                return Some(true);
                            } else if val_str == "false" {
                                return Some(false);
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_source_span_formatting() {
        let span = SourceSpan::new("crates/martensite/src/ui.rs", 142, 10);
        assert_eq!(span.file_line(), "crates/martensite/src/ui.rs:142");
        assert_eq!(span.display(), "crates/martensite/src/ui.rs:142:10");
    }

    #[test]
    fn test_source_patch_formatting() {
        let span = SourceSpan::new("src/ui.rs", 142, 5);
        let patch = SourcePatch::new(span, "padding", "12.0", "16.0");
        assert_eq!(
            patch.format_patch(),
            "src/ui.rs:142: .padding(12.0) -> .padding(16.0)"
        );

        // Also test leading dot in method name is trimmed cleanly
        let span2 = SourceSpan::new("src/theme.rs", 24, 1);
        let patch2 = SourcePatch::new(span2, ".gap", "4.0", "8.0");
        assert_eq!(
            patch2.format_patch(),
            "src/theme.rs:24: .gap(4.0) -> .gap(8.0)"
        );
    }

    #[test]
    fn test_format_source_patch_helper() {
        let formatted = format_source_patch("src/ui.rs", 142, ".padding", "12.0", "16.0");
        assert_eq!(formatted, "src/ui.rs:142: .padding(12.0) -> .padding(16.0)");
    }

    #[test]
    fn test_track_caller_attr_generation() {
        assert_eq!(track_caller_attr(true), "#[track_caller]\n");
        assert_eq!(track_caller_attr(false), "");
    }

    #[test]
    fn test_source_spans_explicit_override() {
        assert!(is_source_spans_enabled(Some(true)));
        assert!(!is_source_spans_enabled(Some(false)));
    }
}
