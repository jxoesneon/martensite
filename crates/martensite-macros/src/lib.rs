//! Procedural macros for Martensite.
//!
//! This crate provides the [`widget!`] declarative macro for compile-time
//! widget construction with property validation. The macro generates idiomatic
//! Rust structs with [`Default`] implementations and accessor methods, all
//! without any runtime overhead.
//!
//! The entire crate is built under `#![forbid(unsafe_code)]`; the generated
//! code contains no `unsafe` blocks.
//!
//! # Forms
//!
//! The macro supports two invocation forms:
//!
//! 1. **Simple** — a unit struct implementing [`Default`]:
//!
//!    ```ignore
//!    widget!(MyButton);
//!    ```
//!
//! 2. **With properties** — a named struct with typed fields, a [`Default`]
//!    implementation using the supplied default expressions, and accessor
//!    methods:
//!
//!    ```ignore
//!    widget! {
//!        MyButton {
//!            label: String = String::new(),
//!            enabled: bool = true,
//!        }
//!    }
//!    ```
//!
//! # Compile-time validation
//!
//! Property names are validated to be valid Rust identifiers (not keywords),
//! property types must be present, and default value expressions must be
//! present. Invalid input produces a clear `compile_error!` diagnostic.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

use proc_macro::{Delimiter, Group, Ident, Literal, Punct, Spacing, Span, TokenStream, TokenTree};

// ---------------------------------------------------------------------------
// Public macro entry point
// ---------------------------------------------------------------------------

/// Declares a Martensite widget, expanding to a struct that implements
/// [`Default`] together with convenience accessor methods.
///
/// # Examples
///
/// The simplest invocation generates a unit struct deriving [`Default`]:
///
/// ```
/// use martensite_macros::widget;
///
/// widget!(MyButton);
///
/// let b = MyButton::default();
/// ```
///
/// # Simple form
///
/// The simplest invocation generates a unit struct deriving [`Default`]:
///
/// ```ignore
/// widget!(MyButton);
/// ```
///
/// Expands to:
///
/// ```ignore
/// #[derive(Default, Debug)]
/// pub struct MyButton;
/// ```
///
/// # Property form
///
/// Supplying a brace-delimited block of `name: Type = default` declarations
/// generates a named struct, a [`Default`] implementation using the supplied
/// default expressions, and getter/setter methods for every property:
///
/// ```ignore
/// widget! {
///     MyButton {
///         label: String = String::new(),
///         enabled: bool = true,
///     }
/// }
/// ```
///
/// Expands to (abbreviated):
///
/// ```ignore
/// #[derive(Debug)]
/// pub struct MyButton {
///     pub label: String,
///     pub enabled: bool,
/// }
///
/// impl Default for MyButton {
///     fn default() -> Self {
///         Self {
///             label: String::new(),
///             enabled: true,
///         }
///     }
/// }
///
/// impl MyButton {
///     pub fn new() -> Self { Self::default() }
///     pub fn label(&self) -> &String { &self.label }
///     pub fn label_mut(&mut self) -> &mut String { &mut self.label }
///     pub fn set_label(&mut self, value: String) { self.label = value; }
///     // ...
/// }
/// ```
///
/// # Compile-time errors
///
/// The macro validates its input at compile time and emits a
/// [`compile_error!`] diagnostic for:
///
/// - Missing widget name.
/// - Widget or property names that are Rust keywords.
/// - Missing property types or default values.
/// - Malformed property syntax.
///
/// # Examples
///
/// ```ignore
/// use martensite_macros::widget;
///
/// widget!(Spacer);
///
/// widget! {
///     Card {
///         title: String = String::new(),
///         visible: bool = true,
///     }
/// }
///
/// let _spacer = Spacer::default();
/// let mut card = Card::new();
/// card.set_title("Hello".to_string());
/// assert_eq!(card.title(), "Hello");
/// assert!(card.visible());
/// ```
#[proc_macro]
pub fn widget(item: TokenStream) -> TokenStream {
    match parse_input(item) {
        Ok(WidgetDef::Simple(name)) => expand_simple(&name),
        Ok(WidgetDef::WithFields { name, fields }) => expand_with_fields(&name, &fields),
        Err(err) => err,
    }
}

// ---------------------------------------------------------------------------
// Data model
// ---------------------------------------------------------------------------

/// A parsed widget property (field) definition.
struct Field {
    /// The property name identifier.
    name: Ident,
    /// The property type as a token stream.
    ty: TokenStream,
    /// The default value expression as a token stream.
    default: TokenStream,
}

/// The parsed shape of a `widget!` invocation.
enum WidgetDef {
    /// A simple unit-struct widget: `widget!(Name)`.
    Simple(Ident),
    /// A widget with named properties: `widget! { Name { ... } }`.
    WithFields {
        /// The widget type name.
        name: Ident,
        /// The declared properties.
        fields: Vec<Field>,
    },
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

/// Parses the raw macro input into a [`WidgetDef`].
///
/// On failure, returns a `TokenStream` containing a `compile_error!`
/// invocation with a descriptive message.
fn parse_input(item: TokenStream) -> Result<WidgetDef, TokenStream> {
    let mut tokens = item.into_iter().peekable();

    // The first token must be an identifier (the widget name).
    let name = match tokens.next() {
        Some(TokenTree::Ident(ident)) => ident,
        Some(other) => {
            return Err(error_at(other.span(), "widget name must be an identifier"));
        }
        None => return Err(error_call_site("widget! requires a widget name")),
    };

    if is_keyword(&name.to_string()) {
        return Err(error_at(
            name.span(),
            "widget name cannot be a Rust keyword",
        ));
    }

    match tokens.next() {
        // `widget!(Name)` — simple form, nothing follows.
        None => Ok(WidgetDef::Simple(name)),

        // `widget!(Name;)` — simple form with a trailing semicolon.
        Some(TokenTree::Punct(p)) if p.as_char() == ';' => {
            if tokens.next().is_some() {
                Err(error_call_site("unexpected tokens after `;`"))
            } else {
                Ok(WidgetDef::Simple(name))
            }
        }

        // `widget! { Name { ... } }` — property form.
        Some(TokenTree::Group(g)) if g.delimiter() == Delimiter::Brace => {
            if tokens.next().is_some() {
                return Err(error_call_site("unexpected tokens after widget block"));
            }
            let fields = parse_fields(g.stream())?;
            Ok(WidgetDef::WithFields { name, fields })
        }

        Some(other) => Err(error_at(
            other.span(),
            "expected `{` or end of input after widget name",
        )),
    }
}

/// Parses the contents of the property block into a list of [`Field`]s.
fn parse_fields(stream: TokenStream) -> Result<Vec<Field>, TokenStream> {
    let mut fields = Vec::new();
    let mut tokens = stream.into_iter().peekable();

    // An empty block is valid — it produces a struct with no fields.
    if tokens.peek().is_none() {
        return Ok(fields);
    }

    loop {
        // --- Property name -------------------------------------------------
        let name = match tokens.next() {
            Some(TokenTree::Ident(ident)) => {
                if is_keyword(&ident.to_string()) {
                    return Err(error_at(
                        ident.span(),
                        "property name cannot be a Rust keyword",
                    ));
                }
                ident
            }
            Some(other) => {
                return Err(error_at(
                    other.span(),
                    "property name must be an identifier",
                ));
            }
            None => break, // end of input — clean termination
        };

        // --- `:` separator --------------------------------------------------
        expect_punct(&mut tokens, ':')?;

        // --- Type (tokens up to top-level `=`) ------------------------------
        let ty = collect_until_punct(&mut tokens, '=', "expected `=` before default value")?;
        if ty.is_empty() {
            return Err(error_at(name.span(), "property requires a type annotation"));
        }

        // --- Default expression (tokens up to top-level `,` or end) ---------
        let default = collect_until_comma_or_end(&mut tokens)?;
        if default.is_empty() {
            return Err(error_at(
                name.span(),
                "property requires a default value expression",
            ));
        }

        fields.push(Field { name, ty, default });
    }

    Ok(fields)
}

/// Consumes the next token, asserting it is a single-character punctuation.
fn expect_punct(
    tokens: &mut std::iter::Peekable<impl Iterator<Item = TokenTree>>,
    expected: char,
) -> Result<(), TokenStream> {
    match tokens.next() {
        Some(TokenTree::Punct(p)) if p.as_char() == expected => Ok(()),
        Some(other) => Err(error_at(other.span(), &format!("expected `{expected}`"))),
        None => Err(error_call_site(&format!(
            "expected `{expected}` but reached end of input"
        ))),
    }
}

/// Collects tokens until a top-level punctuation character (`delim`) is found,
/// consuming the delimiter. Returns an error if a comma is encountered first
/// (when `delim != ','`) or if the input ends without finding `delim`.
///
/// Angle brackets (`<` … `>`) are tracked so that commas inside generic
/// argument lists (e.g. `HashMap<K, V>`) do not prematurely terminate
/// collection. The `->` arrow in function-pointer types is handled so that its
/// `>` is not mistaken for a closing bracket.
fn collect_until_punct(
    tokens: &mut std::iter::Peekable<impl Iterator<Item = TokenTree>>,
    delim: char,
    missing_msg: &str,
) -> Result<TokenStream, TokenStream> {
    let mut collected: Vec<TokenTree> = Vec::new();
    let mut depth: i32 = 0;
    let mut prev_char: Option<char> = None;
    while let Some(tt) = tokens.peek() {
        match tt {
            TokenTree::Punct(p) if p.as_char() == delim && depth == 0 => {
                tokens.next(); // consume delimiter
                return Ok(collected.into_iter().collect());
            }
            TokenTree::Punct(p) if p.as_char() == ',' && depth == 0 => {
                return Err(error_at(p.span(), missing_msg));
            }
            TokenTree::Punct(p) if p.as_char() == '<' => {
                depth += 1;
                prev_char = Some('<');
                collected.push(tokens.next().unwrap());
            }
            TokenTree::Punct(p) if p.as_char() == '>' => {
                // `->` uses `>` but is not a closing generic bracket.
                if prev_char != Some('-') {
                    depth = depth.saturating_sub(1);
                }
                prev_char = Some('>');
                collected.push(tokens.next().unwrap());
            }
            TokenTree::Punct(p) => {
                prev_char = Some(p.as_char());
                collected.push(tokens.next().unwrap());
            }
            _ => {
                prev_char = None;
                collected.push(tokens.next().unwrap());
            }
        }
    }
    Err(error_call_site(missing_msg))
}

/// Collects tokens until a top-level comma or the end of input, consuming the
/// comma if present. Groups (`()`, `[]`, `{}`) are treated as opaque single
/// tokens so commas inside them do not terminate the collection.
///
/// Angle brackets are tracked (as in [`collect_until_punct`]) and closure
/// parameter pipes (`|…|`) are tracked so that commas inside multi-parameter
/// closures (e.g. `move |a, b| { … }`) do not terminate collection.
fn collect_until_comma_or_end(
    tokens: &mut std::iter::Peekable<impl Iterator<Item = TokenTree>>,
) -> Result<TokenStream, TokenStream> {
    let mut collected: Vec<TokenTree> = Vec::new();
    let mut depth: i32 = 0;
    let mut in_closure_params: bool = false;
    let mut prev_char: Option<char> = None;
    while let Some(tt) = tokens.peek() {
        match tt {
            TokenTree::Punct(p) if p.as_char() == ',' && depth == 0 && !in_closure_params => {
                tokens.next(); // consume comma
                return Ok(collected.into_iter().collect());
            }
            TokenTree::Punct(p) if p.as_char() == '<' => {
                depth += 1;
                prev_char = Some('<');
                collected.push(tokens.next().unwrap());
            }
            TokenTree::Punct(p) if p.as_char() == '>' => {
                if prev_char != Some('-') {
                    depth = depth.saturating_sub(1);
                }
                prev_char = Some('>');
                collected.push(tokens.next().unwrap());
            }
            TokenTree::Punct(p) if p.as_char() == '|' => {
                // Toggle closure-parameter mode. `||` toggles on then off;
                // `|a, b|` toggles on, ignores commas, toggles off.
                in_closure_params = !in_closure_params;
                prev_char = Some('|');
                collected.push(tokens.next().unwrap());
            }
            TokenTree::Punct(p) => {
                prev_char = Some(p.as_char());
                collected.push(tokens.next().unwrap());
            }
            _ => {
                prev_char = None;
                collected.push(tokens.next().unwrap());
            }
        }
    }
    // End of input without a trailing comma is fine.
    Ok(collected.into_iter().collect())
}

// ---------------------------------------------------------------------------
// Code generation
// ---------------------------------------------------------------------------

/// Generates the simple unit-struct form.
fn expand_simple(name: &Ident) -> TokenStream {
    let name = name.to_string();
    let source = format!(
        "/// Auto-generated Martensite widget struct.\n\
         #[derive(Default, Debug)]\n\
         pub struct {name};"
    );
    source
        .parse()
        .unwrap_or_else(|_| error_call_site("internal error: failed to generate widget struct"))
}

/// Generates the property form: struct, `Default` impl, and accessor methods.
fn expand_with_fields(name: &Ident, fields: &[Field]) -> TokenStream {
    let name = name.to_string();
    let mut out = String::new();

    // --- Struct definition ---------------------------------------------------
    // Note: `Debug` is intentionally NOT derived because property types such as
    // `Option<Box<dyn Fn()>>` do not implement `Debug`. Users can derive it
    // themselves when all their fields are `Debug`.
    out.push_str("/// Auto-generated Martensite widget struct.\n");
    out.push_str(&format!("pub struct {name} {{\n"));
    for f in fields {
        let ty = f.ty.to_string();
        let fname = f.name.to_string();
        out.push_str(&format!(
            "    /// The `{fname}` property.\n    pub {fname}: {ty},\n"
        ));
    }
    out.push_str("}\n\n");

    // --- Default impl --------------------------------------------------------
    out.push_str(&format!("impl Default for {name} {{\n"));
    out.push_str("    fn default() -> Self {\n");
    out.push_str("        Self {\n");
    for f in fields {
        let def = f.default.to_string();
        out.push_str(&format!("            {}: {},\n", f.name, def));
    }
    out.push_str("        }\n");
    out.push_str("    }\n");
    out.push_str("}\n\n");

    // --- Inherent impl with accessors ---------------------------------------
    out.push_str(&format!("impl {name} {{\n"));
    out.push_str("    /// Constructs a new widget initialised with default property values.\n");
    out.push_str("    pub fn new() -> Self {\n");
    out.push_str("        Self::default()\n");
    out.push_str("    }\n");
    for f in fields {
        let fname = f.name.to_string();
        let ty = f.ty.to_string();
        out.push_str(&format!(
            "    /// Returns a shared reference to the `{fname}` property.\n\
             pub fn {fname}(&self) -> &{ty} {{ &self.{fname} }}\n"
        ));
        out.push_str(&format!(
            "    /// Returns a mutable reference to the `{fname}` property.\n\
             pub fn {fname}_mut(&mut self) -> &mut {ty} {{ &mut self.{fname} }}\n"
        ));
        out.push_str(&format!(
            "    /// Sets the `{fname}` property.\n\
             pub fn set_{fname}(&mut self, value: {ty}) {{ self.{fname} = value; }}\n"
        ));
    }
    out.push_str("}\n");

    out.parse()
        .unwrap_or_else(|_| error_call_site("internal error: failed to generate widget"))
}

// ---------------------------------------------------------------------------
// Error helpers
// ---------------------------------------------------------------------------

/// Builds a `compile_error!("msg")` token stream whose span points to `span`,
/// so the compiler diagnostic highlights the offending input.
fn error_at(span: Span, msg: &str) -> TokenStream {
    let mut ts = TokenStream::new();
    ts.extend([TokenTree::Ident(Ident::new("compile_error", span))]);

    let mut bang = Punct::new('!', Spacing::Alone);
    bang.set_span(span);
    ts.extend([TokenTree::Punct(bang)]);

    let mut inner = TokenStream::new();
    let mut lit = Literal::string(msg);
    lit.set_span(span);
    inner.extend([TokenTree::Literal(lit)]);

    let mut group = Group::new(Delimiter::Parenthesis, inner);
    group.set_span(span);
    ts.extend([TokenTree::Group(group)]);

    ts
}

/// Convenience wrapper for [`error_at`] using [`Span::call_site`].
fn error_call_site(msg: &str) -> TokenStream {
    error_at(Span::call_site(), msg)
}

// ---------------------------------------------------------------------------
// Validation helpers
// ---------------------------------------------------------------------------

/// Returns `true` if `s` is a Rust keyword (strict or reserved).
///
/// Generated identifiers must not collide with keywords, otherwise the
/// produced struct or method definitions would fail to compile.
fn is_keyword(s: &str) -> bool {
    matches!(
        s,
        // Strict keywords.
        "as" | "break" | "const" | "continue" | "crate" | "else" | "enum"
            | "extern" | "false" | "fn" | "for" | "if" | "impl" | "in" | "let"
            | "loop" | "match" | "mod" | "move" | "mut" | "pub" | "ref"
            | "return" | "self" | "Self" | "static" | "struct" | "super"
            | "trait" | "true" | "type" | "unsafe" | "use" | "where" | "while"
        // Contextual keywords (treated as keywords for identifier safety).
            | "async" | "await" | "dyn"
        // Reserved keywords.
            | "abstract" | "become" | "box" | "do" | "final" | "macro"
            | "override" | "priv" | "try" | "typeof" | "unsized" | "virtual"
            | "yield"
    )
}

// ---------------------------------------------------------------------------
// Unit tests for pure helper logic
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::is_keyword;

    #[test]
    fn detects_strict_keywords() {
        assert!(is_keyword("fn"));
        assert!(is_keyword("struct"));
        assert!(is_keyword("let"));
        assert!(is_keyword("match"));
        assert!(is_keyword("return"));
        assert!(is_keyword("true"));
        assert!(is_keyword("false"));
    }

    #[test]
    fn detects_self_and_self_type() {
        assert!(is_keyword("self"));
        assert!(is_keyword("Self"));
    }

    #[test]
    fn detects_contextual_and_reserved_keywords() {
        assert!(is_keyword("async"));
        assert!(is_keyword("await"));
        assert!(is_keyword("dyn"));
        assert!(is_keyword("try"));
        assert!(is_keyword("yield"));
        assert!(is_keyword("box"));
        assert!(is_keyword("macro"));
    }

    #[test]
    fn allows_regular_identifiers() {
        assert!(!is_keyword("label"));
        assert!(!is_keyword("enabled"));
        assert!(!is_keyword("on_click"));
        assert!(!is_keyword("MyButton"));
        assert!(!is_keyword("widget"));
        assert!(!is_keyword("value2"));
        assert!(!is_keyword("is_visible"));
    }

    #[test]
    fn rejects_empty_and_non_keywords() {
        assert!(!is_keyword(""));
        assert!(!is_keyword("not_a_keyword"));
        assert!(!is_keyword("Button"));
        assert!(!is_keyword("spacing"));
    }
}
