//! Implementation of the `#[tweak]` procedural macro for live property tweaks.
//!
//! Provides the attribute macro for annotating variables, fields, or functions
//! with a live-tweak identifier and default value, as specified in
//! `docs/dx/LIVE_TWEAKS.md`.
//!
//! When the `devtools` feature is disabled, this macro expands to direct,
//! zero-overhead compiled-in literal values without referencing any tweak
//! symbols or registries.
//!
//! When the `devtools` feature is enabled, it registers or queries the devtools
//! tweak registry (by default `martensite_devtools::tweak`).

#![allow(dead_code)]

use proc_macro::{Delimiter, Group, Ident, Literal, Punct, Spacing, Span, TokenStream, TokenTree};

/// Supported live-tweak literal value types.
///
/// Supports numeric (`f32`, `f64`, `u32`, `i32`), boolean (`bool`), and color
/// string literals (e.g. `"#ff0000"`, `"rgb(255, 0, 0)"`, `"red"`).
///
/// # Examples
///
/// ```ignore
/// use martensite_macros::tweak::TweakLiteral;
///
/// let lit = TweakLiteral::F32(4.0, "4.0f32".to_string());
/// assert_eq!(lit.type_name(), "f32");
/// assert_eq!(lit.as_code(), "4.0f32");
/// ```
#[derive(Debug, Clone, PartialEq)]
pub enum TweakLiteral {
    /// 32-bit floating point literal value.
    F32(f32, String),
    /// 64-bit floating point literal value.
    F64(f64, String),
    /// 32-bit unsigned integer literal value.
    U32(u32, String),
    /// 32-bit signed integer literal value.
    I32(i32, String),
    /// Boolean literal value (`true` or `false`).
    Bool(bool),
    /// Color string literal value (e.g. `"#ff0000"`).
    Color(String),
}

impl TweakLiteral {
    /// Returns the canonical type tag string.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use martensite_macros::tweak::TweakLiteral;
    ///
    /// assert_eq!(TweakLiteral::Bool(true).type_name(), "bool");
    /// assert_eq!(TweakLiteral::Color("#fff".into()).type_name(), "color");
    /// ```
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::F32(..) => "f32",
            Self::F64(..) => "f64",
            Self::U32(..) => "u32",
            Self::I32(..) => "i32",
            Self::Bool(..) => "bool",
            Self::Color(..) => "color",
        }
    }

    /// Emits the Rust source code representation of this literal.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use martensite_macros::tweak::TweakLiteral;
    ///
    /// assert_eq!(TweakLiteral::Bool(false).as_code(), "false");
    /// assert_eq!(TweakLiteral::Color("#ff0000".into()).as_code(), "\"#ff0000\"");
    /// ```
    pub fn as_code(&self) -> String {
        match self {
            Self::F32(_, raw) => raw.clone(),
            Self::F64(_, raw) => raw.clone(),
            Self::U32(_, raw) => raw.clone(),
            Self::I32(_, raw) => raw.clone(),
            Self::Bool(b) => {
                if *b {
                    "true".to_string()
                } else {
                    "false".to_string()
                }
            }
            Self::Color(c) => format!("\"{c}\""),
        }
    }

    /// Returns `true` if this literal is a color string.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use martensite_macros::tweak::TweakLiteral;
    ///
    /// assert!(TweakLiteral::Color("#ff0000".into()).is_color());
    /// assert!(!TweakLiteral::Bool(true).is_color());
    /// ```
    pub fn is_color(&self) -> bool {
        matches!(self, Self::Color(..))
    }
}

/// Parsed attributes passed to `#[tweak(...)]`.
///
/// # Examples
///
/// ```ignore
/// use martensite_macros::tweak::{TweakAttr, TweakLiteral};
///
/// let attr = TweakAttr {
///     id: Some("theme/gap-scale".to_string()),
///     default: Some(TweakLiteral::F32(4.0, "4.0f32".to_string())),
///     devtools: None,
///     registry: None,
/// };
/// assert_eq!(attr.id.as_deref(), Some("theme/gap-scale"));
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct TweakAttr {
    /// Live-tweak identifier (e.g. `"theme/gap-scale"`).
    pub id: Option<String>,
    /// Default literal value, if specified in the attribute.
    pub default: Option<TweakLiteral>,
    /// Explicit override for devtools mode (`true` or `false`).
    pub devtools: Option<bool>,
    /// Custom path to tweak registry module (defaults to `"martensite_devtools::tweak"`).
    pub registry: Option<String>,
}

impl TweakAttr {
    /// Resolves the effective registry path to call.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use martensite_macros::tweak::TweakAttr;
    ///
    /// let attr = TweakAttr {
    ///     id: Some("test".to_string()),
    ///     default: None,
    ///     devtools: None,
    ///     registry: None,
    /// };
    /// assert_eq!(attr.registry_path(), "martensite_devtools::tweak");
    /// ```
    pub fn registry_path(&self) -> &str {
        self.registry
            .as_deref()
            .unwrap_or("martensite_devtools::tweak")
    }

    /// Checks if devtools live-tweak registry querying is active.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use martensite_macros::tweak::TweakAttr;
    ///
    /// let attr = TweakAttr {
    ///     id: Some("test".to_string()),
    ///     default: None,
    ///     devtools: Some(true),
    ///     registry: None,
    /// };
    /// assert!(attr.is_devtools_enabled());
    /// ```
    #[allow(clippy::manual_unwrap_or_default)]
    pub fn is_devtools_enabled(&self) -> bool {
        self.devtools.unwrap_or(cfg!(feature = "devtools"))
    }
}

/// Expands the `#[tweak]` procedural attribute macro.
///
/// # Examples
///
/// In procedural macro contexts:
///
/// ```ignore
/// #[tweak("theme/gap-scale", 4.0f32)]
/// static GAP: f32 = 4.0f32;
/// ```
pub fn expand_tweak(attr: TokenStream, item: TokenStream) -> TokenStream {
    let tweak_attr = match parse_tweak_attr(attr) {
        Ok(a) => a,
        Err(err) => return err,
    };

    expand_item(&tweak_attr, item)
}

/// Parses the attribute parameters of `#[tweak(...)]`.
///
/// Supports forms:
/// - `#[tweak("id")]`
/// - `#[tweak("id", default_literal)]`
/// - `#[tweak("id", default = default_literal)]`
/// - `#[tweak(id = "id", default = default_literal)]`
/// - `#[tweak("id", 4.0, devtools = false)]`
/// - `#[tweak("id", 4.0, registry = "custom_path")]`
/// - `#[tweak]` or `#[tweak(devtools = false)]` (for enclosing structs/functions)
pub fn parse_tweak_attr(attr: TokenStream) -> Result<TweakAttr, TokenStream> {
    let mut tokens = attr.into_iter().peekable();

    let mut id: Option<String> = None;
    let mut default: Option<TweakLiteral> = None;
    let mut devtools: Option<bool> = None;
    let mut registry: Option<String> = None;

    while let Some(tt) = tokens.next() {
        match tt {
            TokenTree::Literal(lit) => {
                let lit_str = lit.to_string();
                if lit_str.starts_with('"') && lit_str.ends_with('"') && lit_str.len() >= 2 {
                    let unquoted = lit_str[1..lit_str.len() - 1].to_string();
                    if id.is_none() {
                        id = Some(unquoted);
                    } else if default.is_none() {
                        default = Some(TweakLiteral::Color(unquoted));
                    }
                } else {
                    return Err(error_at(lit.span(), "expected string literal for tweak id"));
                }
            }
            TokenTree::Ident(ident) => {
                let name = ident.to_string();
                if name == "id" {
                    expect_punct(&mut tokens, '=')?;
                    let val = parse_string_literal(&mut tokens, "tweak id")?;
                    id = Some(val);
                } else if name == "default" {
                    expect_punct(&mut tokens, '=')?;
                    let lit = parse_literal(&mut tokens)?;
                    default = Some(lit);
                } else if name == "devtools" {
                    expect_punct(&mut tokens, '=')?;
                    let val = parse_bool(&mut tokens)?;
                    devtools = Some(val);
                } else if name == "registry" {
                    expect_punct(&mut tokens, '=')?;
                    let val = parse_string_literal(&mut tokens, "registry path")?;
                    registry = Some(val);
                } else {
                    return Err(error_at(
                        ident.span(),
                        &format!("unexpected parameter `{name}` in #[tweak]"),
                    ));
                }
            }
            TokenTree::Punct(p) if p.as_char() == ',' => {
                // Peek next to see if it's a positional literal for default
                if let Some(next) = tokens.peek() {
                    match next {
                        TokenTree::Literal(_) => {
                            if default.is_none() {
                                let lit = parse_literal(&mut tokens)?;
                                default = Some(lit);
                            }
                        }
                        TokenTree::Punct(p2) if p2.as_char() == '-' => {
                            if default.is_none() {
                                let lit = parse_literal(&mut tokens)?;
                                default = Some(lit);
                            }
                        }
                        TokenTree::Ident(id_tok) => {
                            let s = id_tok.to_string();
                            if (s == "true" || s == "false") && default.is_none() {
                                let lit = parse_literal(&mut tokens)?;
                                default = Some(lit);
                            }
                        }
                        _ => {}
                    }
                }
            }
            other => {
                return Err(error_at(
                    other.span(),
                    "unexpected token in #[tweak] attribute",
                ));
            }
        }
    }

    Ok(TweakAttr {
        id,
        default,
        devtools,
        registry,
    })
}

/// Parses a literal value from token stream (handling signs, suffixes, bool, color strings).
fn parse_literal(
    tokens: &mut std::iter::Peekable<impl Iterator<Item = TokenTree>>,
) -> Result<TweakLiteral, TokenStream> {
    let mut is_negative = false;

    if let Some(TokenTree::Punct(p)) = tokens.peek() {
        if p.as_char() == '-' {
            is_negative = true;
            tokens.next(); // consume '-'
        }
    }

    match tokens.next() {
        Some(TokenTree::Literal(lit)) => {
            let s = lit.to_string();
            // String literal (color string)
            if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
                if is_negative {
                    return Err(error_at(lit.span(), "cannot negate a string literal"));
                }
                let unquoted = s[1..s.len() - 1].to_string();
                return Ok(TweakLiteral::Color(unquoted));
            }

            // Numeric literals
            if s.ends_with("f32") {
                let base = &s[..s.len() - 3];
                let mut val = base
                    .parse::<f32>()
                    .map_err(|_| error_at(lit.span(), "invalid f32 literal"))?;
                if is_negative {
                    val = -val;
                }
                let raw = if is_negative { format!("-{s}") } else { s };
                Ok(TweakLiteral::F32(val, raw))
            } else if s.ends_with("f64") {
                let base = &s[..s.len() - 3];
                let mut val = base
                    .parse::<f64>()
                    .map_err(|_| error_at(lit.span(), "invalid f64 literal"))?;
                if is_negative {
                    val = -val;
                }
                let raw = if is_negative { format!("-{s}") } else { s };
                Ok(TweakLiteral::F64(val, raw))
            } else if s.ends_with("u32") {
                if is_negative {
                    return Err(error_at(
                        lit.span(),
                        "cannot negate an unsigned u32 literal",
                    ));
                }
                let base = &s[..s.len() - 3];
                let val = base
                    .parse::<u32>()
                    .map_err(|_| error_at(lit.span(), "invalid u32 literal"))?;
                Ok(TweakLiteral::U32(val, s))
            } else if s.ends_with("i32") {
                let base = &s[..s.len() - 3];
                let mut val = base
                    .parse::<i32>()
                    .map_err(|_| error_at(lit.span(), "invalid i32 literal"))?;
                if is_negative {
                    val = -val;
                }
                let raw = if is_negative { format!("-{s}") } else { s };
                Ok(TweakLiteral::I32(val, raw))
            } else if s.contains('.') {
                // Floating point without explicit suffix defaults to f32
                let mut val = s
                    .parse::<f32>()
                    .map_err(|_| error_at(lit.span(), "invalid floating point literal"))?;
                if is_negative {
                    val = -val;
                }
                let raw = if is_negative { format!("-{s}") } else { s };
                Ok(TweakLiteral::F32(val, raw))
            } else {
                // Integer without suffix
                if is_negative {
                    let base = s
                        .parse::<i32>()
                        .map_err(|_| error_at(lit.span(), "invalid i32 literal"))?;
                    let val = -base;
                    let raw = format!("-{s}");
                    Ok(TweakLiteral::I32(val, raw))
                } else {
                    let val = s
                        .parse::<u32>()
                        .map_err(|_| error_at(lit.span(), "invalid integer literal"))?;
                    Ok(TweakLiteral::U32(val, s))
                }
            }
        }
        Some(TokenTree::Ident(ident)) => {
            let s = ident.to_string();
            if is_negative {
                return Err(error_at(ident.span(), "cannot negate boolean literal"));
            }
            if s == "true" {
                Ok(TweakLiteral::Bool(true))
            } else if s == "false" {
                Ok(TweakLiteral::Bool(false))
            } else {
                Err(error_at(
                    ident.span(),
                    "expected literal value (f32, f64, u32, i32, bool, or color string)",
                ))
            }
        }
        Some(other) => Err(error_at(
            other.span(),
            "expected literal value (f32, f64, u32, i32, bool, or color string)",
        )),
        None => Err(error_call_site(
            "unexpected end of input while parsing literal",
        )),
    }
}

/// Parses a string literal and unquotes it.
fn parse_string_literal(
    tokens: &mut std::iter::Peekable<impl Iterator<Item = TokenTree>>,
    desc: &str,
) -> Result<String, TokenStream> {
    match tokens.next() {
        Some(TokenTree::Literal(lit)) => {
            let s = lit.to_string();
            if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
                Ok(s[1..s.len() - 1].to_string())
            } else {
                Err(error_at(
                    lit.span(),
                    &format!("expected string literal for {desc}"),
                ))
            }
        }
        Some(other) => Err(error_at(
            other.span(),
            &format!("expected string literal for {desc}"),
        )),
        None => Err(error_call_site(&format!(
            "unexpected end of input while expecting {desc}"
        ))),
    }
}

/// Parses a boolean identifier (`true` or `false`).
fn parse_bool(
    tokens: &mut std::iter::Peekable<impl Iterator<Item = TokenTree>>,
) -> Result<bool, TokenStream> {
    match tokens.next() {
        Some(TokenTree::Ident(ident)) => {
            let s = ident.to_string();
            if s == "true" {
                Ok(true)
            } else if s == "false" {
                Ok(false)
            } else {
                Err(error_at(ident.span(), "expected boolean `true` or `false`"))
            }
        }
        Some(other) => Err(error_at(other.span(), "expected boolean `true` or `false`")),
        None => Err(error_call_site(
            "unexpected end of input while expecting bool",
        )),
    }
}

/// Expects a specific punctuation token.
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

/// Expands the annotated item based on whether devtools mode is enabled or disabled.
fn expand_item(attr: &TweakAttr, item: TokenStream) -> TokenStream {
    let item_str = item.to_string();
    let is_devtools = attr.is_devtools_enabled();

    if is_fn_item(&item_str) {
        expand_fn_item(attr, item, is_devtools)
    } else if is_struct_item(&item_str) {
        expand_struct_item(attr, item, is_devtools)
    } else if is_const_or_static_item(&item_str) {
        expand_const_or_static(attr, &item_str, is_devtools)
    } else if item_str.trim_start().starts_with("let ") {
        expand_let_binding(attr, &item_str, is_devtools)
    } else {
        // Fallback: if disabled, emit original; if enabled, query registry
        if !is_devtools {
            item
        } else if let Some(ref def) = attr.default {
            let reg = attr.registry_path();
            let id = attr.id.as_deref().unwrap_or("unknown");
            let code = def.as_code();
            let expanded = format!("{reg}::get_or_register(\"{id}\", {code})");
            expanded
                .parse()
                .unwrap_or_else(|_| error_call_site("failed to parse expanded tweak"))
        } else {
            item
        }
    }
}

/// Checks if token string represents a function declaration.
fn is_fn_item(s: &str) -> bool {
    let trimmed = s.trim_start();
    trimmed.starts_with("fn ")
        || trimmed.starts_with("pub fn ")
        || trimmed.starts_with("pub(crate) fn ")
        || trimmed.starts_with("async fn ")
        || trimmed.starts_with("pub async fn ")
}

/// Checks if token string represents a struct declaration.
fn is_struct_item(s: &str) -> bool {
    let trimmed = s.trim_start();
    trimmed.starts_with("struct ")
        || trimmed.starts_with("pub struct ")
        || trimmed.starts_with("pub(crate) struct ")
}

/// Checks if token string represents a const or static declaration.
fn is_const_or_static_item(s: &str) -> bool {
    let trimmed = s.trim_start();
    trimmed.starts_with("const ")
        || trimmed.starts_with("pub const ")
        || trimmed.starts_with("static ")
        || trimmed.starts_with("pub static ")
}

/// Expands a `const` or `static` item annotated with `#[tweak]`.
fn expand_const_or_static(_attr: &TweakAttr, item_str: &str, _is_devtools: bool) -> TokenStream {
    item_str
        .parse()
        .unwrap_or_else(|_| error_call_site("failed to parse const/static"))
}

/// Expands a `let` binding annotated with `#[tweak]`.
fn expand_let_binding(attr: &TweakAttr, item_str: &str, is_devtools: bool) -> TokenStream {
    let reg = attr.registry_path();
    let id = attr.id.as_deref().unwrap_or("tweak");

    // Check if the let statement is initializing a Signal:
    // e.g. `let gap = Signal::new(4.0f32);`
    let is_signal = item_str.contains("Signal :: new")
        || item_str.contains("Signal::new")
        || item_str.contains("MockSignal :: new")
        || item_str.contains("MockSignal::new");

    if !is_devtools {
        // Zero-overhead compiled-in value: return original code without modifications
        return item_str
            .parse()
            .unwrap_or_else(|_| error_call_site("failed to parse let binding"));
    }

    // DevTools mode enabled:
    if is_signal {
        if let Some((lhs, rhs)) = item_str.split_once('=') {
            let rhs_clean = rhs.trim().trim_end_matches(';').trim();
            let expanded = format!(
                "{lhs}= {{\n    let _tweak_sig = {rhs_clean};\n    {reg}::register_signal(\"{id}\", &_tweak_sig);\n    _tweak_sig\n}};"
            );
            return expanded
                .parse()
                .unwrap_or_else(|_| error_call_site("failed to parse signal tweak"));
        }
    }

    // Direct literal value let binding:
    if let Some(ref def) = attr.default {
        let code = def.as_code();
        if let Some((lhs, _)) = item_str.split_once('=') {
            let expanded = format!("{lhs}= {reg}::get_or_register(\"{id}\", {code});");
            return expanded
                .parse()
                .unwrap_or_else(|_| error_call_site("failed to parse tweaked let"));
        } else {
            let trimmed = item_str.trim().trim_end_matches(';').trim();
            let expanded = format!("{trimmed} = {reg}::get_or_register(\"{id}\", {code});");
            return expanded
                .parse()
                .unwrap_or_else(|_| error_call_site("failed to parse tweaked let"));
        }
    } else if let Some((lhs, rhs)) = item_str.split_once('=') {
        let rhs_clean = rhs.trim().trim_end_matches(';').trim();
        let expanded = format!("{lhs}= {reg}::get_or_register(\"{id}\", {rhs_clean});");
        return expanded
            .parse()
            .unwrap_or_else(|_| error_call_site("failed to parse tweaked let"));
    }

    item_str
        .parse()
        .unwrap_or_else(|_| error_call_site("failed to parse let binding"))
}

/// Expands a function item annotated with `#[tweak]`.
fn expand_fn_item(attr: &TweakAttr, item: TokenStream, is_devtools: bool) -> TokenStream {
    let mut header_tokens = TokenStream::new();
    let mut body_group: Option<Group> = None;

    for tt in item {
        if let TokenTree::Group(ref g) = tt {
            if g.delimiter() == Delimiter::Brace {
                body_group = Some(g.clone());
                break;
            }
        }
        header_tokens.extend([tt]);
    }

    let body = match body_group {
        Some(b) => b,
        None => return error_call_site("expected brace body for function in #[tweak]"),
    };

    if !is_devtools {
        let mut out = TokenStream::new();
        out.extend(header_tokens);
        out.extend([TokenTree::Group(body)]);
        return out;
    }

    let reg = attr.registry_path();
    let id = attr.id.as_deref().unwrap_or("fn_tweak");

    let body_str = body.stream().to_string();
    let is_signal = body_str.contains("Signal :: new")
        || body_str.contains("Signal::new")
        || body_str.contains("MockSignal :: new")
        || body_str.contains("MockSignal::new");

    let mut new_body_str = String::new();
    if is_signal {
        new_body_str.push_str(&format!(
            "{{\n    let _tweak_sig = {{ {body_str} }};\n    {reg}::register_signal(\"{id}\", &_tweak_sig);\n    _tweak_sig\n}}"
        ));
    } else if let Some(ref def) = attr.default {
        let code = def.as_code();
        new_body_str.push_str(&format!(
            "{{\n    {reg}::get_or_register(\"{id}\", {code})\n}}"
        ));
    } else {
        new_body_str.push_str(&format!(
            "{{\n    {reg}::get_or_register(\"{id}\", {{ {body_str} }})\n}}"
        ));
    }

    let new_body_stream: TokenStream = new_body_str
        .parse()
        .unwrap_or_else(|_| error_call_site("failed to parse tweaked fn body"));

    let mut out = TokenStream::new();
    out.extend(header_tokens);
    out.extend(new_body_stream);
    out
}

/// A parsed field with optional `#[tweak(...)]` attribute.
struct StructField {
    pub name: String,
    pub ty: TokenStream,
    pub is_pub: bool,
    pub tweak_attr: Option<TweakAttr>,
}

/// Expands a struct whose fields may be annotated with `#[tweak]`.
fn expand_struct_item(attr: &TweakAttr, item: TokenStream, is_devtools: bool) -> TokenStream {
    let mut tokens = item.into_iter().peekable();

    let mut is_pub = false;
    let mut struct_name = String::new();
    let mut body_tokens: Option<TokenStream> = None;

    while let Some(tt) = tokens.next() {
        match tt {
            TokenTree::Ident(ident) => {
                let s = ident.to_string();
                if s == "pub" {
                    is_pub = true;
                } else if s == "struct" {
                    if let Some(TokenTree::Ident(name_ident)) = tokens.next() {
                        struct_name = name_ident.to_string();
                    }
                }
            }
            TokenTree::Group(group) if group.delimiter() == Delimiter::Brace => {
                body_tokens = Some(group.stream());
                break;
            }
            _ => {}
        }
    }

    let body = match body_tokens {
        Some(b) => b,
        None => return error_call_site("expected brace body for struct in #[tweak]"),
    };

    let fields = match parse_struct_fields(body) {
        Ok(f) => f,
        Err(err) => return err,
    };

    let pub_str = if is_pub { "pub " } else { "" };
    let mut out = String::new();

    // Struct definition with stripped #[tweak] attributes on fields
    out.push_str(&format!("{pub_str}struct {struct_name} {{\n"));
    for f in &fields {
        let f_pub = if f.is_pub { "pub " } else { "" };
        let ty_code = f.ty.to_string();
        out.push_str(&format!("    {}{}: {ty_code},\n", f_pub, f.name));
    }
    out.push_str("}\n\n");

    // Default implementation
    out.push_str(&format!("impl Default for {struct_name} {{\n"));
    out.push_str("    fn default() -> Self {\n");
    out.push_str("        Self {\n");
    for f in &fields {
        if let Some(ref field_tweak) = f.tweak_attr {
            let effective_devtools = field_tweak
                .devtools
                .or(attr.devtools)
                .unwrap_or(is_devtools);
            if !effective_devtools {
                let code = field_tweak
                    .default
                    .as_ref()
                    .map(|d| d.as_code())
                    .unwrap_or_else(|| "Default::default()".to_string());
                out.push_str(&format!("            {}: {},\n", f.name, code));
            } else {
                let reg = field_tweak.registry_path();
                let id = field_tweak.id.as_deref().unwrap_or("field");
                let code = field_tweak
                    .default
                    .as_ref()
                    .map(|d| d.as_code())
                    .unwrap_or_else(|| "Default::default()".to_string());
                out.push_str(&format!(
                    "            {}: {reg}::get_or_register(\"{id}\", {code}),\n",
                    f.name
                ));
            }
        } else {
            out.push_str(&format!("            {}: Default::default(),\n", f.name));
        }
    }
    out.push_str("        }\n");
    out.push_str("    }\n");
    out.push_str("}\n");

    out.parse()
        .unwrap_or_else(|_| error_call_site("failed to generate struct for #[tweak]"))
}

/// Parses struct fields from the body token stream, extracting any `#[tweak(...)]` attributes.
fn parse_struct_fields(body: TokenStream) -> Result<Vec<StructField>, TokenStream> {
    let mut fields = Vec::new();
    let mut tokens = body.into_iter().peekable();

    while tokens.peek().is_some() {
        let mut field_tweak: Option<TweakAttr> = None;
        let mut field_pub = false;

        // Collect attributes on field
        while let Some(TokenTree::Punct(p)) = tokens.peek() {
            if p.as_char() == '#' {
                tokens.next(); // consume '#'
                if let Some(TokenTree::Group(attr_group)) = tokens.next() {
                    let mut attr_tokens = attr_group.stream().into_iter().peekable();
                    if let Some(TokenTree::Ident(attr_ident)) = attr_tokens.next() {
                        if attr_ident.to_string() == "tweak" {
                            if let Some(TokenTree::Group(inner_group)) = attr_tokens.next() {
                                field_tweak = Some(parse_tweak_attr(inner_group.stream())?);
                            }
                        }
                    }
                }
            } else {
                break;
            }
        }

        // Visibility
        if let Some(TokenTree::Ident(ident)) = tokens.peek() {
            if ident.to_string() == "pub" {
                field_pub = true;
                tokens.next(); // consume pub
            }
        }

        // Field name
        let field_name = match tokens.next() {
            Some(TokenTree::Ident(ident)) => ident.to_string(),
            Some(TokenTree::Punct(p)) if p.as_char() == ',' => continue,
            None => break,
            Some(other) => {
                return Err(error_at(other.span(), "expected field name identifier"));
            }
        };

        // Colon ':'
        expect_punct(&mut tokens, ':')?;

        // Type tokens up to ',' or end
        let mut ty_tokens = TokenStream::new();
        while let Some(tt) = tokens.peek() {
            if let TokenTree::Punct(p) = tt {
                if p.as_char() == ',' {
                    tokens.next(); // consume ','
                    break;
                }
            }
            ty_tokens.extend([tokens.next().unwrap()]);
        }

        fields.push(StructField {
            name: field_name,
            ty: ty_tokens,
            is_pub: field_pub,
            tweak_attr: field_tweak,
        });
    }

    Ok(fields)
}

/// Builds a `compile_error!("msg");` token stream.
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

    let mut semi = Punct::new(';', Spacing::Alone);
    semi.set_span(span);
    ts.extend([TokenTree::Punct(semi)]);

    ts
}

/// Convenience wrapper for [`error_at`] using [`Span::call_site`].
fn error_call_site(msg: &str) -> TokenStream {
    error_at(Span::call_site(), msg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_literal_types() {
        let f = TweakLiteral::F32(4.0, "4.0f32".to_string());
        assert_eq!(f.type_name(), "f32");
        assert_eq!(f.as_code(), "4.0f32");
        assert!(!f.is_color());

        let f64_lit = TweakLiteral::F64(4.0, "4.0f64".to_string());
        assert_eq!(f64_lit.type_name(), "f64");

        let u = TweakLiteral::U32(100, "100u32".to_string());
        assert_eq!(u.type_name(), "u32");

        let i = TweakLiteral::I32(-10, "-10i32".to_string());
        assert_eq!(i.type_name(), "i32");

        let b = TweakLiteral::Bool(true);
        assert_eq!(b.type_name(), "bool");
        assert_eq!(b.as_code(), "true");
        assert!(!b.is_color());

        let c = TweakLiteral::Color("#ff0000".to_string());
        assert_eq!(c.type_name(), "color");
        assert_eq!(c.as_code(), "\"#ff0000\"");
        assert!(c.is_color());
    }

    #[test]
    fn test_attr_methods() {
        let attr = TweakAttr {
            id: Some("test/id".to_string()),
            default: Some(TweakLiteral::F32(1.0, "1.0f32".to_string())),
            devtools: Some(true),
            registry: Some("custom::registry".to_string()),
        };
        assert_eq!(attr.registry_path(), "custom::registry");
        assert!(attr.is_devtools_enabled());

        let attr_default = TweakAttr {
            id: None,
            default: None,
            devtools: None,
            registry: None,
        };
        assert_eq!(attr_default.registry_path(), "martensite_devtools::tweak");
    }
}
