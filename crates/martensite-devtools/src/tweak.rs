//! Live property tweak registry and runtime mutation channel (WP-05b).
//!
//! Provides the runtime infrastructure for live property tweaks as specified in
//! `docs/dx/LIVE_TWEAKS.md`.
//!
//! # Architecture & Contracts
//!
//! - **D4 (State Identity Contract):** Live tweaks address the live widget arena
//!   and reactive signal graph directly. Tweaks persist and re-apply across cdylib
//!   hot-reloads by name (`name -> SignalId`).
//! - **D8 (Dev-only Compilation):** The [`TweakRegistry`] and associated data structures
//!   exist only under the `devtools` cargo feature. In production release builds, this
//!   module and all tweak symbols compile out entirely with zero overhead.
//! - **Transient Marker Contract:** Any property differing from its compiled-in default
//!   is flagged as modified/transient (`~` badge). [`TweakRegistry::reset_all`] restores
//!   every tweaked value to its compiled default.
//! - **Source Patch Write-back:** When source spans are captured (`file:line:col`),
//!   tweaks can emit source patches (e.g. `src/ui.rs:142: .padding(12.0) -> .padding(16.0)`),
//!   allowing write-back without requiring a custom DSL.
//!
//! # Examples
//!
//! ```
//! use martensite_devtools::tweak::{TweakRegistry, TweakValue, SourceSpan};
//!
//! let registry = TweakRegistry::new();
//!
//! // Register a default padding value with callsite span
//! let span = SourceSpan::new("src/ui.rs", 142, 5);
//! let padding = registry.register_or_get_with_span("button/padding", 12.0f32, span, "padding");
//! assert_eq!(padding, 12.0);
//!
//! // Live tweak the padding in dev mode
//! registry.set_value("button/padding", 16.0f32);
//! assert_eq!(registry.get::<f32>("button/padding"), Some(16.0));
//!
//! // Emit source patch for write-back
//! let patch = registry.emit_patch("button/padding").expect("patch generated");
//! assert_eq!(patch, "src/ui.rs:142: .padding(12.0) -> .padding(16.0)");
//! ```

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use martensite_reactive::{ReactiveRuntime, Signal, SignalId};
use parking_lot::RwLock;

/// A source code location (file path, 1-indexed line, and 1-indexed column).
///
/// # Examples
///
/// ```
/// use martensite_devtools::tweak::SourceSpan;
///
/// let span = SourceSpan::new("src/ui.rs", 142, 5);
/// assert_eq!(span.file_line(), "src/ui.rs:142");
/// assert_eq!(span.display(), "src/ui.rs:142:5");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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
    /// ```
    /// use martensite_devtools::tweak::SourceSpan;
    ///
    /// let span = SourceSpan::new("src/main.rs", 10, 1);
    /// assert_eq!(span.line, 10);
    /// assert_eq!(span.column, 1);
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
    /// ```
    /// use martensite_devtools::tweak::SourceSpan;
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
    /// ```
    /// use martensite_devtools::tweak::SourceSpan;
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
    /// ```
    /// use martensite_devtools::tweak::SourceSpan;
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
/// `file:line: .method(old) -> .method(new)`
///
/// # Examples
///
/// ```
/// use martensite_devtools::tweak::{SourcePatch, SourceSpan};
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
    /// ```
    /// use martensite_devtools::tweak::{SourcePatch, SourceSpan};
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

    /// Formats the patch line in canonical format:
    /// `file:line: .method(old) -> .method(new)`
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::tweak::{SourcePatch, SourceSpan};
    ///
    /// let span = SourceSpan::new("src/view.rs", 50, 12);
    /// let patch = SourcePatch::new(span, "gap", "8.0", "16.0");
    /// assert_eq!(patch.format_patch(), "src/view.rs:50: .gap(8.0) -> .gap(16.0)");
    /// ```
    pub fn format_patch(&self) -> String {
        format_source_patch(
            &self.span.file,
            self.span.line,
            &self.method_or_prop,
            &self.old_value,
            &self.new_value,
        )
    }
}

/// Formats a live-tweak source patch string in canonical format:
/// `file:line: .method(old_value) -> .method(new_value)`
///
/// # Examples
///
/// ```
/// use martensite_devtools::tweak::format_source_patch;
///
/// let patch = format_source_patch("src/ui.rs", 142, "padding", "12.0", "16.0");
/// assert_eq!(patch, "src/ui.rs:142: .padding(12.0) -> .padding(16.0)");
/// ```
pub fn format_source_patch(
    file: &str,
    line: u32,
    method: &str,
    old_value: &str,
    new_value: &str,
) -> String {
    let prop = method.trim_start_matches('.');
    format!("{file}:{line}: .{prop}({old_value}) -> .{prop}({new_value})")
}

/// Supported dynamically-typed live tweak value types.
///
/// Supports floating point (`f32`, `f64`) with optional min/max range constraints,
/// signed and unsigned integers (`i32`, `u32`), booleans (`bool`), RGBA colors (`[u8; 4]`),
/// and strings (`String`).
///
/// # Examples
///
/// ```
/// use martensite_devtools::tweak::TweakValue;
///
/// let val = TweakValue::f32_range(8.0, 0.0, 32.0);
/// assert_eq!(val.as_f32(), Some(8.0));
/// assert_eq!(val.range_f32(), Some((0.0, 32.0)));
///
/// let col = TweakValue::color_rgba(255, 0, 0, 255);
/// assert_eq!(col.as_color(), Some([255, 0, 0, 255]));
/// ```
#[derive(Debug, Clone, PartialEq)]
pub enum TweakValue {
    /// 32-bit floating point value with optional `(min, max)` range.
    F32(f32, Option<(f32, f32)>),
    /// 64-bit floating point value with optional `(min, max)` range.
    F64(f64, Option<(f64, f64)>),
    /// 32-bit signed integer value.
    I32(i32),
    /// 32-bit unsigned integer value.
    U32(u32),
    /// Boolean value.
    Bool(bool),
    /// Color representation as RGBA `[r, g, b, a]` bytes.
    Color([u8; 4]),
    /// Text string value.
    String(String),
}

impl TweakValue {
    /// Creates a 32-bit float tweak value without range bounds.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::tweak::TweakValue;
    ///
    /// let v = TweakValue::f32(4.5);
    /// assert_eq!(v.as_f32(), Some(4.5));
    /// ```
    pub fn f32(val: f32) -> Self {
        Self::F32(val, None)
    }

    /// Creates a 32-bit float tweak value with explicit `(min, max)` range bounds.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::tweak::TweakValue;
    ///
    /// let v = TweakValue::f32_range(12.0, 0.0, 100.0);
    /// assert_eq!(v.range_f32(), Some((0.0, 100.0)));
    /// ```
    pub fn f32_range(val: f32, min: f32, max: f32) -> Self {
        Self::F32(val, Some((min, max)))
    }

    /// Creates a 64-bit float tweak value without range bounds.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::tweak::TweakValue;
    ///
    /// let v = TweakValue::f64(10.25);
    /// assert_eq!(v.as_f64(), Some(10.25));
    /// ```
    pub fn f64(val: f64) -> Self {
        Self::F64(val, None)
    }

    /// Creates a 64-bit float tweak value with explicit `(min, max)` range bounds.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::tweak::TweakValue;
    ///
    /// let v = TweakValue::f64_range(50.0, 1.0, 1000.0);
    /// assert_eq!(v.range_f64(), Some((1.0, 1000.0)));
    /// ```
    pub fn f64_range(val: f64, min: f64, max: f64) -> Self {
        Self::F64(val, Some((min, max)))
    }

    /// Creates a signed 32-bit integer tweak value.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::tweak::TweakValue;
    ///
    /// let v = TweakValue::i32(-42);
    /// assert_eq!(v.as_i32(), Some(-42));
    /// ```
    pub fn i32(val: i32) -> Self {
        Self::I32(val)
    }

    /// Creates an unsigned 32-bit integer tweak value.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::tweak::TweakValue;
    ///
    /// let v = TweakValue::u32(100);
    /// assert_eq!(v.as_u32(), Some(100));
    /// ```
    pub fn u32(val: u32) -> Self {
        Self::U32(val)
    }

    /// Creates a boolean tweak value.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::tweak::TweakValue;
    ///
    /// let v = TweakValue::bool(true);
    /// assert_eq!(v.as_bool(), Some(true));
    /// ```
    pub fn bool(val: bool) -> Self {
        Self::Bool(val)
    }

    /// Creates an RGBA color tweak value from individual byte components.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::tweak::TweakValue;
    ///
    /// let v = TweakValue::color_rgba(255, 128, 0, 255);
    /// assert_eq!(v.as_color(), Some([255, 128, 0, 255]));
    /// ```
    pub fn color_rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self::Color([r, g, b, a])
    }

    /// Parses a hex color string (`#RGB`, `#RGBA`, `#RRGGBB`, or `#RRGGBBAA`) into a color tweak value.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::tweak::TweakValue;
    ///
    /// let red = TweakValue::color_hex("#ff0000").unwrap();
    /// assert_eq!(red.as_color(), Some([255, 0, 0, 255]));
    ///
    /// let semi = TweakValue::color_hex("#00ff0080").unwrap();
    /// assert_eq!(semi.as_color(), Some([0, 255, 0, 128]));
    /// ```
    pub fn color_hex(hex: &str) -> Option<Self> {
        parse_hex_color(hex).map(Self::Color)
    }

    /// Creates a string tweak value.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::tweak::TweakValue;
    ///
    /// let v = TweakValue::string("Hello Martensite");
    /// assert_eq!(v.as_str(), Some("Hello Martensite"));
    /// ```
    pub fn string(val: impl Into<String>) -> Self {
        Self::String(val.into())
    }

    /// Returns the canonical type tag string.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::tweak::TweakValue;
    ///
    /// assert_eq!(TweakValue::f32(1.0).type_name(), "f32");
    /// assert_eq!(TweakValue::bool(false).type_name(), "bool");
    /// assert_eq!(TweakValue::color_rgba(0, 0, 0, 255).type_name(), "color");
    /// ```
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::F32(..) => "f32",
            Self::F64(..) => "f64",
            Self::I32(..) => "i32",
            Self::U32(..) => "u32",
            Self::Bool(..) => "bool",
            Self::Color(..) => "color",
            Self::String(..) => "string",
        }
    }

    /// Returns the 32-bit float value, if this is an `F32` variant.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::tweak::TweakValue;
    ///
    /// assert_eq!(TweakValue::f32(8.5).as_f32(), Some(8.5));
    /// assert_eq!(TweakValue::bool(true).as_f32(), None);
    /// ```
    pub fn as_f32(&self) -> Option<f32> {
        match self {
            Self::F32(v, _) => Some(*v),
            _ => None,
        }
    }

    /// Returns the 64-bit float value, if this is an `F64` variant.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::tweak::TweakValue;
    ///
    /// assert_eq!(TweakValue::f64(123.456).as_f64(), Some(123.456));
    /// assert_eq!(TweakValue::i32(10).as_f64(), None);
    /// ```
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::F64(v, _) => Some(*v),
            _ => None,
        }
    }

    /// Returns the signed 32-bit integer value, if this is an `I32` variant.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::tweak::TweakValue;
    ///
    /// assert_eq!(TweakValue::i32(-10).as_i32(), Some(-10));
    /// assert_eq!(TweakValue::f32(1.0).as_i32(), None);
    /// ```
    pub fn as_i32(&self) -> Option<i32> {
        match self {
            Self::I32(v) => Some(*v),
            _ => None,
        }
    }

    /// Returns the unsigned 32-bit integer value, if this is a `U32` variant.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::tweak::TweakValue;
    ///
    /// assert_eq!(TweakValue::u32(42).as_u32(), Some(42));
    /// assert_eq!(TweakValue::bool(true).as_u32(), None);
    /// ```
    pub fn as_u32(&self) -> Option<u32> {
        match self {
            Self::U32(v) => Some(*v),
            _ => None,
        }
    }

    /// Returns the boolean value, if this is a `Bool` variant.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::tweak::TweakValue;
    ///
    /// assert_eq!(TweakValue::bool(true).as_bool(), Some(true));
    /// assert_eq!(TweakValue::u32(1).as_bool(), None);
    /// ```
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(v) => Some(*v),
            _ => None,
        }
    }

    /// Returns the color `[r, g, b, a]` bytes, if this is a `Color` variant.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::tweak::TweakValue;
    ///
    /// assert_eq!(TweakValue::color_rgba(10, 20, 30, 255).as_color(), Some([10, 20, 30, 255]));
    /// assert_eq!(TweakValue::bool(false).as_color(), None);
    /// ```
    pub fn as_color(&self) -> Option<[u8; 4]> {
        match self {
            Self::Color(c) => Some(*c),
            _ => None,
        }
    }

    /// Returns a string slice if this is a `String` variant.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::tweak::TweakValue;
    ///
    /// assert_eq!(TweakValue::string("abc").as_str(), Some("abc"));
    /// assert_eq!(TweakValue::i32(1).as_str(), None);
    /// ```
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(s) => Some(s.as_str()),
            _ => None,
        }
    }

    /// Returns the optional range `(min, max)` for an `F32` variant.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::tweak::TweakValue;
    ///
    /// let val = TweakValue::f32_range(5.0, 0.0, 10.0);
    /// assert_eq!(val.range_f32(), Some((0.0, 10.0)));
    /// ```
    pub fn range_f32(&self) -> Option<(f32, f32)> {
        match self {
            Self::F32(_, r) => *r,
            _ => None,
        }
    }

    /// Returns the optional range `(min, max)` for an `F64` variant.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::tweak::TweakValue;
    ///
    /// let val = TweakValue::f64_range(5.0, 0.0, 10.0);
    /// assert_eq!(val.range_f64(), Some((0.0, 10.0)));
    /// ```
    pub fn range_f64(&self) -> Option<(f64, f64)> {
        match self {
            Self::F64(_, r) => *r,
            _ => None,
        }
    }

    /// Attaches range bounds to an `F32` variant.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::tweak::TweakValue;
    ///
    /// let val = TweakValue::f32(5.0).with_range_f32(0.0, 10.0);
    /// assert_eq!(val.range_f32(), Some((0.0, 10.0)));
    /// ```
    pub fn with_range_f32(self, min: f32, max: f32) -> Self {
        match self {
            Self::F32(v, _) => Self::F32(v, Some((min, max))),
            other => other,
        }
    }

    /// Attaches range bounds to an `F64` variant.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::tweak::TweakValue;
    ///
    /// let val = TweakValue::f64(5.0).with_range_f64(0.0, 10.0);
    /// assert_eq!(val.range_f64(), Some((0.0, 10.0)));
    /// ```
    pub fn with_range_f64(self, min: f64, max: f64) -> Self {
        match self {
            Self::F64(v, _) => Self::F64(v, Some((min, max))),
            other => other,
        }
    }

    /// Returns `true` if this value represents a color.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::tweak::TweakValue;
    ///
    /// assert!(TweakValue::color_rgba(255, 0, 0, 255).is_color());
    /// assert!(!TweakValue::f32(1.0).is_color());
    /// ```
    pub fn is_color(&self) -> bool {
        matches!(self, Self::Color(..))
    }

    /// Returns `true` if this value is numeric (`F32`, `F64`, `I32`, or `U32`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::tweak::TweakValue;
    ///
    /// assert!(TweakValue::f32(1.0).is_numeric());
    /// assert!(TweakValue::i32(10).is_numeric());
    /// assert!(!TweakValue::bool(true).is_numeric());
    /// ```
    pub fn is_numeric(&self) -> bool {
        matches!(
            self,
            Self::F32(..) | Self::F64(..) | Self::I32(..) | Self::U32(..)
        )
    }

    /// Formats this tweak value as source code suitable for patch insertion and diagnostic display.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::tweak::TweakValue;
    ///
    /// assert_eq!(TweakValue::f32(12.0).format_value(), "12.0");
    /// assert_eq!(TweakValue::bool(true).format_value(), "true");
    /// assert_eq!(TweakValue::color_rgba(255, 0, 0, 255).format_value(), "#ff0000");
    /// ```
    pub fn format_value(&self) -> String {
        match self {
            Self::F32(v, _) => {
                if v.fract() == 0.0 {
                    format!("{v:.1}")
                } else {
                    format!("{v}")
                }
            }
            Self::F64(v, _) => {
                if v.fract() == 0.0 {
                    format!("{v:.1}")
                } else {
                    format!("{v}")
                }
            }
            Self::I32(v) => format!("{v}"),
            Self::U32(v) => format!("{v}"),
            Self::Bool(b) => format!("{b}"),
            Self::Color([r, g, b, a]) => {
                if *a == 255 {
                    format!("#{r:02x}{g:02x}{b:02x}")
                } else {
                    format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
                }
            }
            Self::String(s) => format!("\"{s}\""),
        }
    }
}

impl fmt::Display for TweakValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.format_value())
    }
}

// ---------------------------------------------------------------------------
// Color helper functions
// ---------------------------------------------------------------------------

fn parse_hex_byte(chars: &[u8]) -> Option<u8> {
    let s = std::str::from_utf8(chars).ok()?;
    u8::from_str_radix(s, 16).ok()
}

fn parse_hex_color(hex: &str) -> Option<[u8; 4]> {
    let s = hex.trim().trim_start_matches('#');
    let bytes = s.as_bytes();
    match bytes.len() {
        3 => {
            let r = parse_hex_byte(&[bytes[0], bytes[0]])?;
            let g = parse_hex_byte(&[bytes[1], bytes[1]])?;
            let b = parse_hex_byte(&[bytes[2], bytes[2]])?;
            Some([r, g, b, 255])
        }
        4 => {
            let r = parse_hex_byte(&[bytes[0], bytes[0]])?;
            let g = parse_hex_byte(&[bytes[1], bytes[1]])?;
            let b = parse_hex_byte(&[bytes[2], bytes[2]])?;
            let a = parse_hex_byte(&[bytes[3], bytes[3]])?;
            Some([r, g, b, a])
        }
        6 => {
            let r = parse_hex_byte(&bytes[0..2])?;
            let g = parse_hex_byte(&bytes[2..4])?;
            let b = parse_hex_byte(&bytes[4..6])?;
            Some([r, g, b, 255])
        }
        8 => {
            let r = parse_hex_byte(&bytes[0..2])?;
            let g = parse_hex_byte(&bytes[2..4])?;
            let b = parse_hex_byte(&bytes[4..6])?;
            let a = parse_hex_byte(&bytes[6..8])?;
            Some([r, g, b, a])
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// From conversions for TweakValue
// ---------------------------------------------------------------------------

impl From<f32> for TweakValue {
    fn from(v: f32) -> Self {
        Self::F32(v, None)
    }
}

impl From<f64> for TweakValue {
    fn from(v: f64) -> Self {
        Self::F64(v, None)
    }
}

impl From<i32> for TweakValue {
    fn from(v: i32) -> Self {
        Self::I32(v)
    }
}

impl From<u32> for TweakValue {
    fn from(v: u32) -> Self {
        Self::U32(v)
    }
}

impl From<bool> for TweakValue {
    fn from(v: bool) -> Self {
        Self::Bool(v)
    }
}

impl From<[u8; 4]> for TweakValue {
    fn from(v: [u8; 4]) -> Self {
        Self::Color(v)
    }
}

impl From<String> for TweakValue {
    fn from(v: String) -> Self {
        Self::String(v)
    }
}

impl From<&str> for TweakValue {
    fn from(v: &str) -> Self {
        // If string matches hex color syntax, parse as color; otherwise string.
        if let Some(col) = parse_hex_color(v) {
            Self::Color(col)
        } else {
            Self::String(v.to_string())
        }
    }
}

// ---------------------------------------------------------------------------
// Tweakable Trait
// ---------------------------------------------------------------------------

/// Trait for types that can convert bidirectionally to and from a [`TweakValue`].
///
/// Implemented for standard Rust primitive types (`f32`, `f64`, `i32`, `u32`, `bool`, `[u8; 4]`,
/// `String`, `&'static str`, and `TweakValue`).
///
/// # Examples
///
/// ```
/// use martensite_devtools::tweak::{Tweakable, TweakValue};
///
/// let val = 12.0f32.to_tweak_value();
/// assert_eq!(val, TweakValue::f32(12.0));
/// assert_eq!(f32::try_from_tweak_value(&val), Some(12.0));
/// ```
pub trait Tweakable: Clone + Send + Sync + 'static {
    /// Converts this value into a [`TweakValue`].
    fn to_tweak_value(&self) -> TweakValue;

    /// Attempts to convert a [`TweakValue`] into this type.
    fn try_from_tweak_value(val: &TweakValue) -> Option<Self>;
}

impl Tweakable for f32 {
    fn to_tweak_value(&self) -> TweakValue {
        TweakValue::f32(*self)
    }

    fn try_from_tweak_value(val: &TweakValue) -> Option<Self> {
        val.as_f32()
    }
}

impl Tweakable for f64 {
    fn to_tweak_value(&self) -> TweakValue {
        TweakValue::f64(*self)
    }

    fn try_from_tweak_value(val: &TweakValue) -> Option<Self> {
        val.as_f64()
    }
}

impl Tweakable for i32 {
    fn to_tweak_value(&self) -> TweakValue {
        TweakValue::i32(*self)
    }

    fn try_from_tweak_value(val: &TweakValue) -> Option<Self> {
        val.as_i32()
    }
}

impl Tweakable for u32 {
    fn to_tweak_value(&self) -> TweakValue {
        TweakValue::u32(*self)
    }

    fn try_from_tweak_value(val: &TweakValue) -> Option<Self> {
        val.as_u32()
    }
}

impl Tweakable for bool {
    fn to_tweak_value(&self) -> TweakValue {
        TweakValue::bool(*self)
    }

    fn try_from_tweak_value(val: &TweakValue) -> Option<Self> {
        val.as_bool()
    }
}

impl Tweakable for [u8; 4] {
    fn to_tweak_value(&self) -> TweakValue {
        TweakValue::Color(*self)
    }

    fn try_from_tweak_value(val: &TweakValue) -> Option<Self> {
        val.as_color()
    }
}

impl Tweakable for String {
    fn to_tweak_value(&self) -> TweakValue {
        TweakValue::String(self.clone())
    }

    fn try_from_tweak_value(val: &TweakValue) -> Option<Self> {
        match val {
            TweakValue::String(s) => Some(s.clone()),
            _ => Some(val.format_value()),
        }
    }
}

impl Tweakable for &'static str {
    fn to_tweak_value(&self) -> TweakValue {
        TweakValue::String((*self).to_string())
    }

    fn try_from_tweak_value(val: &TweakValue) -> Option<Self> {
        match val {
            TweakValue::String(s) => Some(Box::leak(s.clone().into_boxed_str())),
            _ => Some(Box::leak(val.format_value().into_boxed_str())),
        }
    }
}

impl Tweakable for TweakValue {
    fn to_tweak_value(&self) -> TweakValue {
        self.clone()
    }

    fn try_from_tweak_value(val: &TweakValue) -> Option<Self> {
        Some(val.clone())
    }
}

// ---------------------------------------------------------------------------
// AsSignalId Trait
// ---------------------------------------------------------------------------

/// Thread-safe closure for propagating tweak value updates to bound reactive signals.
pub type SignalUpdater = Arc<dyn Fn(&TweakValue) + Send + Sync>;

/// Thread-safe closure for receiving notifications when tweak values change in the registry.
pub type TweakListener = Arc<dyn Fn(&str, &TweakValue) + Send + Sync>;

/// Trait for types that can be linked to a live tweak as a reactive signal.
///
/// Implemented for [`SignalId`], `&SignalId`, [`Signal<T>`], and references thereof.
///
/// # Examples
///
/// ```
/// use martensite_devtools::tweak::AsSignalId;
/// use martensite_reactive::SignalId;
///
/// let id = SignalId::next();
/// assert_eq!(id.as_signal_id(), id);
/// ```
pub trait AsSignalId {
    /// Returns the unique reactive [`SignalId`].
    fn as_signal_id(&self) -> SignalId;

    /// Optional initial value to populate registry if not present.
    fn initial_tweak_value(&self) -> Option<TweakValue> {
        None
    }

    /// Constructs an owned closure that updates the bound signal.
    fn make_updater(&self) -> Option<SignalUpdater> {
        let id = self.as_signal_id();
        Some(Arc::new(move |_: &TweakValue| {
            ReactiveRuntime::current().mark_dirty(id);
        }))
    }
}

impl AsSignalId for SignalId {
    fn as_signal_id(&self) -> SignalId {
        *self
    }
}

impl AsSignalId for &SignalId {
    fn as_signal_id(&self) -> SignalId {
        **self
    }
}

impl<T: Tweakable + Send + Sync + 'static> AsSignalId for Signal<T> {
    fn as_signal_id(&self) -> SignalId {
        self.id()
    }

    fn initial_tweak_value(&self) -> Option<TweakValue> {
        Some(self.get_untracked().to_tweak_value())
    }

    fn make_updater(&self) -> Option<Arc<dyn Fn(&TweakValue) + Send + Sync>> {
        let sig = self.clone();
        Some(Arc::new(move |val| {
            if let Some(new_val) = T::try_from_tweak_value(val) {
                sig.set(new_val);
            }
        }))
    }
}

impl<T: Tweakable + Send + Sync + 'static> AsSignalId for &Signal<T> {
    fn as_signal_id(&self) -> SignalId {
        self.id()
    }

    fn initial_tweak_value(&self) -> Option<TweakValue> {
        Some(self.get_untracked().to_tweak_value())
    }

    fn make_updater(&self) -> Option<Arc<dyn Fn(&TweakValue) + Send + Sync>> {
        let sig = (*self).clone();
        Some(Arc::new(move |val| {
            if let Some(new_val) = T::try_from_tweak_value(val) {
                sig.set(new_val);
            }
        }))
    }
}

impl<T: Tweakable + Send + Sync + 'static> AsSignalId for Arc<Signal<T>> {
    fn as_signal_id(&self) -> SignalId {
        self.id()
    }

    fn initial_tweak_value(&self) -> Option<TweakValue> {
        Some(self.get_untracked().to_tweak_value())
    }

    fn make_updater(&self) -> Option<Arc<dyn Fn(&TweakValue) + Send + Sync>> {
        let sig = (**self).clone();
        Some(Arc::new(move |val| {
            if let Some(new_val) = T::try_from_tweak_value(val) {
                sig.set(new_val);
            }
        }))
    }
}

// ---------------------------------------------------------------------------
// TweakEntry
// ---------------------------------------------------------------------------

/// A registered live tweak entry containing compiled default, active live value,
/// optional reactive signal binding, and optional callsite source location for write-back.
///
/// # Examples
///
/// ```
/// use martensite_devtools::tweak::{TweakEntry, TweakValue, SourceSpan};
///
/// let entry = TweakEntry::new("button/padding", TweakValue::f32(12.0));
/// assert!(!entry.is_modified());
/// assert_eq!(entry.name, "button/padding");
/// ```
#[derive(Clone)]
pub struct TweakEntry {
    /// Unique identifier or hierarchical path (e.g. `"theme/gap-scale"`).
    pub name: String,
    /// The default/initial value registered at startup or compile-time.
    pub default_value: TweakValue,
    /// The current live value, which may be modified at runtime.
    pub current_value: TweakValue,
    /// Optional linked reactive signal ID.
    pub signal_id: Option<SignalId>,
    /// Optional callsite source location.
    pub source_span: Option<SourceSpan>,
    /// Optional builder method or property name (e.g. `"padding"`).
    pub property_name: Option<String>,
    /// Closure used to update the bound signal when value changes.
    signal_updater: Option<SignalUpdater>,
}

impl TweakEntry {
    /// Constructs a new [`TweakEntry`] with equal default and current values.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::tweak::{TweakEntry, TweakValue};
    ///
    /// let entry = TweakEntry::new("scale", TweakValue::f32(1.0));
    /// assert_eq!(entry.current_value, TweakValue::f32(1.0));
    /// ```
    pub fn new(name: impl Into<String>, default_value: TweakValue) -> Self {
        let current_value = default_value.clone();
        Self {
            name: name.into(),
            default_value,
            current_value,
            signal_id: None,
            source_span: None,
            property_name: None,
            signal_updater: None,
        }
    }

    /// Attaches callsite source location and property name for write-back patch emission.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::tweak::{TweakEntry, TweakValue, SourceSpan};
    ///
    /// let span = SourceSpan::new("src/ui.rs", 10, 1);
    /// let entry = TweakEntry::new("pad", TweakValue::f32(4.0)).with_span(span, "padding");
    /// assert_eq!(entry.property_name.as_deref(), Some("padding"));
    /// ```
    pub fn with_span(mut self, span: SourceSpan, property: impl Into<String>) -> Self {
        self.source_span = Some(span);
        self.property_name = Some(property.into().trim_start_matches('.').to_string());
        self
    }

    /// Links a reactive signal ID to this tweak entry.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::tweak::{TweakEntry, TweakValue};
    /// use martensite_reactive::SignalId;
    ///
    /// let id = SignalId::next();
    /// let entry = TweakEntry::new("sig", TweakValue::bool(false)).with_signal_id(id);
    /// assert_eq!(entry.signal_id, Some(id));
    /// ```
    pub fn with_signal_id(mut self, signal_id: SignalId) -> Self {
        self.signal_id = Some(signal_id);
        self
    }

    /// Returns `true` if the current live value differs from the compiled default.
    ///
    /// Triggers the `~` transient badge in the DevTools UI.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::tweak::{TweakEntry, TweakValue};
    ///
    /// let mut entry = TweakEntry::new("gap", TweakValue::f32(4.0));
    /// assert!(!entry.is_modified());
    /// entry.current_value = TweakValue::f32(8.0);
    /// assert!(entry.is_modified());
    /// ```
    pub fn is_modified(&self) -> bool {
        self.current_value != self.default_value
    }

    /// Returns `true` if this tweak is transient (modified from compiled default).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::tweak::{TweakEntry, TweakValue};
    ///
    /// let entry = TweakEntry::new("gap", TweakValue::f32(4.0));
    /// assert!(!entry.is_transient());
    /// ```
    pub fn is_transient(&self) -> bool {
        self.is_modified()
    }

    /// Resets the current live value back to its compiled default.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::tweak::{TweakEntry, TweakValue};
    ///
    /// let mut entry = TweakEntry::new("gap", TweakValue::f32(4.0));
    /// entry.current_value = TweakValue::f32(12.0);
    /// assert!(entry.is_modified());
    /// entry.reset();
    /// assert!(!entry.is_modified());
    /// assert_eq!(entry.current_value, TweakValue::f32(4.0));
    /// ```
    pub fn reset(&mut self) {
        self.current_value = self.default_value.clone();
    }

    /// Resolves the property name, falling back to the last segment of the tweak's hierarchical path.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::tweak::{TweakEntry, TweakValue};
    ///
    /// let entry1 = TweakEntry::new("theme/gap-scale", TweakValue::f32(4.0));
    /// assert_eq!(entry1.property_or_inferred(), "gap-scale");
    /// ```
    pub fn property_or_inferred(&self) -> &str {
        if let Some(ref prop) = self.property_name {
            prop.as_str()
        } else {
            self.name.split('/').next_back().unwrap_or(&self.name)
        }
    }

    /// Emits a source patch string if a callsite source span is available.
    ///
    /// Format: `file:line: .property(old_value) -> .property(new_value)`
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::tweak::{TweakEntry, TweakValue, SourceSpan};
    ///
    /// let span = SourceSpan::new("src/ui.rs", 142, 5);
    /// let mut entry = TweakEntry::new("button/padding", TweakValue::f32(12.0))
    ///     .with_span(span, "padding");
    /// entry.current_value = TweakValue::f32(16.0);
    ///
    /// let patch = entry.emit_patch().expect("patch available");
    /// assert_eq!(patch, "src/ui.rs:142: .padding(12.0) -> .padding(16.0)");
    /// ```
    pub fn emit_patch(&self) -> Option<String> {
        let span = self.source_span.as_ref()?;
        let prop = self.property_or_inferred();
        let old_val = self.default_value.format_value();
        let new_val = self.current_value.format_value();
        Some(format_source_patch(
            &span.file, span.line, prop, &old_val, &new_val,
        ))
    }
}

impl fmt::Debug for TweakEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TweakEntry")
            .field("name", &self.name)
            .field("default_value", &self.default_value)
            .field("current_value", &self.current_value)
            .field("signal_id", &self.signal_id)
            .field("source_span", &self.source_span)
            .field("property_name", &self.property_name)
            .field("has_signal_updater", &self.signal_updater.is_some())
            .finish()
    }
}

impl PartialEq for TweakEntry {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.default_value == other.default_value
            && self.current_value == other.current_value
            && self.signal_id == other.signal_id
            && self.source_span == other.source_span
            && self.property_name == other.property_name
    }
}

// ---------------------------------------------------------------------------
// TweakRegistry
// ---------------------------------------------------------------------------

/// Thread-safe registry for live runtime property tweaks.
///
/// Manages live tweak entries, signal bindings, value updates, and write-back source patches.
///
/// # Examples
///
/// ```
/// use martensite_devtools::tweak::{TweakRegistry, TweakValue};
///
/// let registry = TweakRegistry::new();
/// let val = registry.register_or_get("ui/gap", 8.0f32);
/// assert_eq!(val, 8.0);
///
/// registry.set_value("ui/gap", 16.0f32);
/// assert_eq!(registry.get::<f32>("ui/gap"), Some(16.0));
/// ```
pub struct TweakRegistry {
    entries: RwLock<HashMap<String, TweakEntry>>,
    listeners: RwLock<Vec<TweakListener>>,
}

impl Default for TweakRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl TweakRegistry {
    /// Constructs a new empty [`TweakRegistry`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::tweak::TweakRegistry;
    ///
    /// let registry = TweakRegistry::new();
    /// assert!(registry.is_empty());
    /// ```
    pub fn new() -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
            listeners: RwLock::new(Vec::new()),
        }
    }

    /// Constructs a new [`TweakRegistry`] preallocated with the specified capacity.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::tweak::TweakRegistry;
    ///
    /// let registry = TweakRegistry::with_capacity(64);
    /// assert_eq!(registry.len(), 0);
    /// ```
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            entries: RwLock::new(HashMap::with_capacity(capacity)),
            listeners: RwLock::new(Vec::new()),
        }
    }

    /// Gets an existing live tweak value, or registers the compiled default if absent.
    ///
    /// If the tweak was previously modified (e.g. across a hot-reload), the current live
    /// value is returned instead of the supplied default.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::tweak::TweakRegistry;
    ///
    /// let registry = TweakRegistry::new();
    /// let v1 = registry.register_or_get("padding", 12.0f32);
    /// assert_eq!(v1, 12.0);
    ///
    /// registry.set_value("padding", 16.0f32);
    /// let v2 = registry.register_or_get("padding", 12.0f32);
    /// assert_eq!(v2, 16.0);
    /// ```
    pub fn register_or_get<T: Tweakable>(&self, name: &str, default: T) -> T {
        let mut entries = self.entries.write();
        if let Some(entry) = entries.get(name) {
            T::try_from_tweak_value(&entry.current_value).unwrap_or(default)
        } else {
            let tweak_val = default.to_tweak_value();
            let entry = TweakEntry::new(name, tweak_val);
            entries.insert(name.to_string(), entry);
            default
        }
    }

    /// Raw [`TweakValue`] variant of [`register_or_get`](Self::register_or_get).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::tweak::{TweakRegistry, TweakValue};
    ///
    /// let registry = TweakRegistry::new();
    /// let val = registry.register_or_get_value("theme/color", TweakValue::color_rgba(255, 0, 0, 255));
    /// assert_eq!(val, TweakValue::color_rgba(255, 0, 0, 255));
    /// ```
    pub fn register_or_get_value(&self, name: &str, default: TweakValue) -> TweakValue {
        let mut entries = self.entries.write();
        if let Some(entry) = entries.get(name) {
            entry.current_value.clone()
        } else {
            let entry = TweakEntry::new(name, default.clone());
            entries.insert(name.to_string(), entry);
            default
        }
    }

    /// Registers a default value with callsite source span for write-back patch emission.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::tweak::{TweakRegistry, SourceSpan};
    ///
    /// let registry = TweakRegistry::new();
    /// let span = SourceSpan::new("src/ui.rs", 142, 5);
    /// let pad = registry.register_or_get_with_span("ui/pad", 12.0f32, span, "padding");
    /// assert_eq!(pad, 12.0);
    /// ```
    pub fn register_or_get_with_span<T: Tweakable>(
        &self,
        name: &str,
        default: T,
        span: SourceSpan,
        property: &str,
    ) -> T {
        let mut entries = self.entries.write();
        if let Some(entry) = entries.get_mut(name) {
            if entry.source_span.is_none() {
                entry.source_span = Some(span);
                entry.property_name = Some(property.trim_start_matches('.').to_string());
            }
            T::try_from_tweak_value(&entry.current_value).unwrap_or(default)
        } else {
            let tweak_val = default.to_tweak_value();
            let mut entry = TweakEntry::new(name, tweak_val);
            entry.source_span = Some(span);
            entry.property_name = Some(property.trim_start_matches('.').to_string());
            entries.insert(name.to_string(), entry);
            default
        }
    }

    /// Alias for [`register_or_get`](Self::register_or_get) to match macro expectations.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::tweak::TweakRegistry;
    ///
    /// let registry = TweakRegistry::new();
    /// let pad = registry.get_or_register("ui/pad", 10.0f32);
    /// assert_eq!(pad, 10.0);
    /// ```
    pub fn get_or_register<T: Tweakable>(&self, name: &str, default: T) -> T {
        self.register_or_get(name, default)
    }

    /// Alias for [`register_or_get_value`](Self::register_or_get_value).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::tweak::{TweakRegistry, TweakValue};
    ///
    /// let registry = TweakRegistry::new();
    /// let val = registry.get_or_register_value("ui/flag", TweakValue::bool(true));
    /// assert_eq!(val, TweakValue::bool(true));
    /// ```
    pub fn get_or_register_value(&self, name: &str, default: TweakValue) -> TweakValue {
        self.register_or_get_value(name, default)
    }

    /// Links a reactive signal to a named tweak so live mutations automatically update the signal.
    ///
    /// If the tweak already has a modified value in the registry (e.g. across a cdylib hot reload),
    /// the modified value is immediately re-asserted into the signal.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::tweak::TweakRegistry;
    /// use martensite_reactive::Signal;
    ///
    /// let registry = TweakRegistry::new();
    /// let sig = Signal::new(4.0f32);
    /// registry.register_signal("theme/gap", &sig);
    ///
    /// registry.set_value("theme/gap", 16.0f32);
    /// assert_eq!(sig.get_untracked(), 16.0);
    /// ```
    pub fn register_signal<S: AsSignalId>(&self, name: &str, signal: S) {
        let sig_id = signal.as_signal_id();
        let initial_val = signal.initial_tweak_value();
        let updater = signal.make_updater();

        let mut entries = self.entries.write();
        if let Some(entry) = entries.get_mut(name) {
            entry.signal_id = Some(sig_id);
            if let Some(ref u) = updater {
                entry.signal_updater = Some(u.clone());
            }
            // Re-assertion across hot reload: if entry is already modified, re-apply the tweaked value to the new signal!
            if entry.is_modified() {
                if let Some(ref u) = entry.signal_updater {
                    let current = entry.current_value.clone();
                    let u = u.clone();
                    drop(entries);
                    u(&current);
                }
            } else if let Some(init) = initial_val {
                entry.default_value = init.clone();
                entry.current_value = init;
            }
        } else {
            let def = initial_val.unwrap_or(TweakValue::Bool(false));
            let mut entry = TweakEntry::new(name, def);
            entry.signal_id = Some(sig_id);
            entry.signal_updater = updater;
            entries.insert(name.to_string(), entry);
        }
    }

    /// Links an explicit [`SignalId`] without direct value mutation closure.
    ///
    /// Modifying this tweak flags the signal dirty in the ambient reactive runtime.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::tweak::TweakRegistry;
    /// use martensite_reactive::SignalId;
    ///
    /// let registry = TweakRegistry::new();
    /// let id = SignalId::next();
    /// registry.register_signal_id("sensor/reading", id);
    /// assert!(registry.contains("sensor/reading"));
    /// ```
    pub fn register_signal_id(&self, name: &str, signal_id: SignalId) {
        self.register_signal(name, signal_id);
    }

    /// Links a [`SignalId`] with a custom updater closure.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::sync::atomic::{AtomicI32, Ordering};
    /// use std::sync::Arc;
    /// use martensite_devtools::tweak::{TweakRegistry, TweakValue};
    /// use martensite_reactive::SignalId;
    ///
    /// let registry = TweakRegistry::new();
    /// let id = SignalId::next();
    /// let counter = Arc::new(AtomicI32::new(0));
    /// let counter_clone = counter.clone();
    ///
    /// registry.register_signal_with_updater("count", id, move |val| {
    ///     if let Some(v) = val.as_i32() {
    ///         counter_clone.store(v, Ordering::SeqCst);
    ///     }
    /// });
    ///
    /// registry.set_value("count", 42i32);
    /// assert_eq!(counter.load(Ordering::SeqCst), 42);
    /// ```
    pub fn register_signal_with_updater(
        &self,
        name: &str,
        signal_id: SignalId,
        updater: impl Fn(&TweakValue) + Send + Sync + 'static,
    ) {
        let updater_arc: SignalUpdater = Arc::new(updater);
        let mut entries = self.entries.write();
        if let Some(entry) = entries.get_mut(name) {
            entry.signal_id = Some(signal_id);
            entry.signal_updater = Some(updater_arc.clone());
            if entry.is_modified() {
                let cur = entry.current_value.clone();
                drop(entries);
                updater_arc(&cur);
            }
        } else {
            let mut entry = TweakEntry::new(name, TweakValue::Bool(false));
            entry.signal_id = Some(signal_id);
            entry.signal_updater = Some(updater_arc);
            entries.insert(name.to_string(), entry);
        }
    }

    /// Updates the current live value for a named tweak.
    ///
    /// Dispatches signal updates to linked reactive signals and notifies change listeners.
    /// Returns `true` if an existing entry was updated, or `false` if registered anew.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::tweak::TweakRegistry;
    ///
    /// let registry = TweakRegistry::new();
    /// registry.register_or_get("font_size", 14.0f32);
    /// assert!(registry.set_value("font_size", 18.0f32));
    /// assert_eq!(registry.get::<f32>("font_size"), Some(18.0));
    /// ```
    pub fn set_value(&self, name: &str, val: impl Into<TweakValue>) -> bool {
        let new_val = val.into();
        let mut updater_and_sig = None;
        let is_existing;

        {
            let mut entries = self.entries.write();
            if let Some(entry) = entries.get_mut(name) {
                entry.current_value = new_val.clone();
                if let Some(ref updater) = entry.signal_updater {
                    updater_and_sig = Some((updater.clone(), entry.signal_id));
                } else if let Some(sig_id) = entry.signal_id {
                    updater_and_sig = Some((
                        Arc::new(move |_: &TweakValue| {
                            ReactiveRuntime::current().mark_dirty(sig_id);
                        }) as Arc<dyn Fn(&TweakValue) + Send + Sync>,
                        Some(sig_id),
                    ));
                }
                is_existing = true;
            } else {
                let mut entry = TweakEntry::new(name, new_val.clone());
                entry.current_value = new_val.clone();
                entries.insert(name.to_string(), entry);
                is_existing = false;
            }
        }

        if let Some((updater, _)) = updater_and_sig {
            updater(&new_val);
        }

        let listeners = self.listeners.read().clone();
        for listener in listeners {
            listener(name, &new_val);
        }

        is_existing
    }

    /// Strongly-typed convenience wrapper around [`set_value`](Self::set_value).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::tweak::TweakRegistry;
    ///
    /// let registry = TweakRegistry::new();
    /// registry.register_or_get("opacity", 1.0f32);
    /// registry.set("opacity", 0.8f32);
    /// assert_eq!(registry.get::<f32>("opacity"), Some(0.8));
    /// ```
    pub fn set<T: Tweakable>(&self, name: &str, val: T) -> bool {
        self.set_value(name, val.to_tweak_value())
    }

    /// Retrieves the current live [`TweakValue`] for a named tweak.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::tweak::{TweakRegistry, TweakValue};
    ///
    /// let registry = TweakRegistry::new();
    /// registry.register_or_get("spacing", 8.0f32);
    /// assert_eq!(registry.get_value("spacing"), Some(TweakValue::f32(8.0)));
    /// ```
    pub fn get_value(&self, name: &str) -> Option<TweakValue> {
        let entries = self.entries.read();
        entries.get(name).map(|e| e.current_value.clone())
    }

    /// Strongly-typed getter for the current live tweak value.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::tweak::TweakRegistry;
    ///
    /// let registry = TweakRegistry::new();
    /// registry.register_or_get("margin", 4.0f32);
    /// assert_eq!(registry.get::<f32>("margin"), Some(4.0));
    /// ```
    pub fn get<T: Tweakable>(&self, name: &str) -> Option<T> {
        let entries = self.entries.read();
        entries
            .get(name)
            .and_then(|e| T::try_from_tweak_value(&e.current_value))
    }

    /// Returns a clone of the [`TweakEntry`] for inspection.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::tweak::TweakRegistry;
    ///
    /// let registry = TweakRegistry::new();
    /// registry.register_or_get("speed", 100u32);
    /// let entry = registry.get_entry("speed").expect("entry exists");
    /// assert_eq!(entry.name, "speed");
    /// ```
    pub fn get_entry(&self, name: &str) -> Option<TweakEntry> {
        let entries = self.entries.read();
        entries.get(name).cloned()
    }

    /// Checks if a tweak with the given name is registered.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::tweak::TweakRegistry;
    ///
    /// let registry = TweakRegistry::new();
    /// assert!(!registry.contains("test"));
    /// registry.register_or_get("test", true);
    /// assert!(registry.contains("test"));
    /// ```
    pub fn contains(&self, name: &str) -> bool {
        let entries = self.entries.read();
        entries.contains_key(name)
    }

    /// Returns the number of registered tweaks.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::tweak::TweakRegistry;
    ///
    /// let registry = TweakRegistry::new();
    /// assert_eq!(registry.len(), 0);
    /// registry.register_or_get("a", 1.0f32);
    /// registry.register_or_get("b", 2.0f32);
    /// assert_eq!(registry.len(), 2);
    /// ```
    pub fn len(&self) -> usize {
        let entries = self.entries.read();
        entries.len()
    }

    /// Returns `true` if no tweaks are registered.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::tweak::TweakRegistry;
    ///
    /// let registry = TweakRegistry::new();
    /// assert!(registry.is_empty());
    /// ```
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the number of tweaks currently differing from their compiled default.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::tweak::TweakRegistry;
    ///
    /// let registry = TweakRegistry::new();
    /// registry.register_or_get("a", 1.0f32);
    /// assert_eq!(registry.count_modified(), 0);
    /// registry.set_value("a", 2.0f32);
    /// assert_eq!(registry.count_modified(), 1);
    /// ```
    pub fn count_modified(&self) -> usize {
        let entries = self.entries.read();
        entries.values().filter(|e| e.is_modified()).count()
    }

    /// Returns all tweak entries that currently differ from their compiled default.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::tweak::TweakRegistry;
    ///
    /// let registry = TweakRegistry::new();
    /// registry.register_or_get("pad", 10.0f32);
    /// registry.set_value("pad", 20.0f32);
    /// let modified = registry.modified_entries();
    /// assert_eq!(modified.len(), 1);
    /// assert_eq!(modified[0].name, "pad");
    /// ```
    pub fn modified_entries(&self) -> Vec<TweakEntry> {
        let entries = self.entries.read();
        entries
            .values()
            .filter(|e| e.is_modified())
            .cloned()
            .collect()
    }

    /// Returns all registered tweak entries in arbitrary order.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::tweak::TweakRegistry;
    ///
    /// let registry = TweakRegistry::new();
    /// registry.register_or_get("x", 1.0f32);
    /// registry.register_or_get("y", 2.0f32);
    /// assert_eq!(registry.all_entries().len(), 2);
    /// ```
    pub fn all_entries(&self) -> Vec<TweakEntry> {
        let entries = self.entries.read();
        entries.values().cloned().collect()
    }

    /// Emits a source patch string for the named tweak if a callsite source span is available.
    ///
    /// Returns `None` if the entry is not found or has no source span recorded.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::tweak::{TweakRegistry, SourceSpan};
    ///
    /// let registry = TweakRegistry::new();
    /// let span = SourceSpan::new("src/ui.rs", 142, 5);
    /// registry.register_or_get_with_span("button/padding", 12.0f32, span, "padding");
    /// registry.set_value("button/padding", 16.0f32);
    ///
    /// let patch = registry.emit_patch("button/padding").expect("patch generated");
    /// assert_eq!(patch, "src/ui.rs:142: .padding(12.0) -> .padding(16.0)");
    /// ```
    pub fn emit_patch(&self, name: &str) -> Option<String> {
        let entries = self.entries.read();
        entries.get(name).and_then(|e| e.emit_patch())
    }

    /// Emits source patches for all modified tweaks that have callsite source spans recorded.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::tweak::{TweakRegistry, SourceSpan};
    ///
    /// let registry = TweakRegistry::new();
    /// let span = SourceSpan::new("src/ui.rs", 142, 5);
    /// registry.register_or_get_with_span("button/padding", 12.0f32, span, "padding");
    /// registry.set_value("button/padding", 16.0f32);
    ///
    /// let patches = registry.emit_all_patches();
    /// assert_eq!(patches.len(), 1);
    /// assert_eq!(patches[0], "src/ui.rs:142: .padding(12.0) -> .padding(16.0)");
    /// ```
    pub fn emit_all_patches(&self) -> Vec<String> {
        let entries = self.entries.read();
        entries
            .values()
            .filter(|e| e.is_modified() && e.source_span.is_some())
            .filter_map(|e| e.emit_patch())
            .collect()
    }

    /// Resets a single named tweak back to its compiled default.
    ///
    /// Returns `true` if the tweak existed and was reset.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::tweak::TweakRegistry;
    ///
    /// let registry = TweakRegistry::new();
    /// registry.register_or_get("pad", 10.0f32);
    /// registry.set_value("pad", 20.0f32);
    /// assert_eq!(registry.get::<f32>("pad"), Some(20.0));
    ///
    /// assert!(registry.reset("pad"));
    /// assert_eq!(registry.get::<f32>("pad"), Some(10.0));
    /// ```
    pub fn reset(&self, name: &str) -> bool {
        let mut update = None;
        {
            let mut entries = self.entries.write();
            if let Some(entry) = entries.get_mut(name) {
                if entry.is_modified() {
                    entry.reset();
                    update = Some((
                        entry.default_value.clone(),
                        entry.signal_updater.clone(),
                        entry.signal_id,
                    ));
                }
            } else {
                return false;
            }
        }

        if let Some((def, updater, sig_id)) = update {
            if let Some(u) = updater {
                u(&def);
            } else if let Some(sig) = sig_id {
                ReactiveRuntime::current().mark_dirty(sig);
            }
            let listeners = self.listeners.read().clone();
            for listener in listeners {
                listener(name, &def);
            }
            true
        } else {
            false
        }
    }

    /// Resets all modified tweaks back to their compiled defaults.
    ///
    /// Propagates restorations to bound signals and notifies change listeners.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::tweak::TweakRegistry;
    ///
    /// let registry = TweakRegistry::new();
    /// registry.register_or_get("a", 1.0f32);
    /// registry.register_or_get("b", 2.0f32);
    /// registry.set_value("a", 10.0f32);
    /// registry.set_value("b", 20.0f32);
    /// assert_eq!(registry.count_modified(), 2);
    ///
    /// registry.reset_all();
    /// assert_eq!(registry.count_modified(), 0);
    /// assert_eq!(registry.get::<f32>("a"), Some(1.0));
    /// assert_eq!(registry.get::<f32>("b"), Some(2.0));
    /// ```
    pub fn reset_all(&self) {
        let mut updates = Vec::new();
        {
            let mut entries = self.entries.write();
            for entry in entries.values_mut() {
                if entry.is_modified() {
                    entry.reset();
                    updates.push((
                        entry.name.clone(),
                        entry.default_value.clone(),
                        entry.signal_updater.clone(),
                        entry.signal_id,
                    ));
                }
            }
        }

        let listeners = self.listeners.read().clone();
        for (name, def, updater, sig_id) in updates {
            if let Some(u) = updater {
                u(&def);
            } else if let Some(sig) = sig_id {
                ReactiveRuntime::current().mark_dirty(sig);
            }
            for listener in &listeners {
                listener(&name, &def);
            }
        }
    }

    /// Clears all registered tweaks and listeners.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::tweak::TweakRegistry;
    ///
    /// let registry = TweakRegistry::new();
    /// registry.register_or_get("temp", 123);
    /// assert_eq!(registry.len(), 1);
    /// registry.clear();
    /// assert_eq!(registry.len(), 0);
    /// ```
    pub fn clear(&self) {
        self.entries.write().clear();
        self.listeners.write().clear();
    }

    /// Registers a change listener callback invoked whenever a tweak value changes.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::sync::atomic::{AtomicBool, Ordering};
    /// use std::sync::Arc;
    /// use martensite_devtools::tweak::TweakRegistry;
    ///
    /// let registry = TweakRegistry::new();
    /// let called = Arc::new(AtomicBool::new(false));
    /// let called_clone = called.clone();
    ///
    /// registry.add_listener(move |name, _val| {
    ///     if name == "flag" {
    ///         called_clone.store(true, Ordering::SeqCst);
    ///     }
    /// });
    ///
    /// registry.set_value("flag", true);
    /// assert!(called.load(Ordering::SeqCst));
    /// ```
    pub fn add_listener(&self, listener: impl Fn(&str, &TweakValue) + Send + Sync + 'static) {
        self.listeners.write().push(Arc::new(listener));
    }

    /// Returns a list of all currently bound reactive [`SignalId`]s.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::tweak::TweakRegistry;
    /// use martensite_reactive::SignalId;
    ///
    /// let registry = TweakRegistry::new();
    /// let id = SignalId::next();
    /// registry.register_signal_id("sig", id);
    /// assert_eq!(registry.signal_ids(), vec![id]);
    /// ```
    pub fn signal_ids(&self) -> Vec<SignalId> {
        let entries = self.entries.read();
        entries.values().filter_map(|e| e.signal_id).collect()
    }

    /// Returns the names of all tweaks that have no active signal binding.
    ///
    /// Useful for reporting tweaks whose code declaration vanished during a cdylib swap.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::tweak::TweakRegistry;
    ///
    /// let registry = TweakRegistry::new();
    /// registry.register_or_get("unbound_prop", 10.0f32);
    /// assert_eq!(registry.orphaned_tweaks(), vec!["unbound_prop"]);
    /// ```
    pub fn orphaned_tweaks(&self) -> Vec<String> {
        let entries = self.entries.read();
        entries
            .values()
            .filter(|e| e.signal_id.is_none())
            .map(|e| e.name.clone())
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Global Default Registry Accessors
// ---------------------------------------------------------------------------

static GLOBAL_REGISTRY: std::sync::OnceLock<TweakRegistry> = std::sync::OnceLock::new();

/// Returns a reference to the global default [`TweakRegistry`].
///
/// # Examples
///
/// ```
/// use martensite_devtools::tweak::global;
///
/// let reg = global();
/// assert!(reg.len() >= 0);
/// ```
pub fn global() -> &'static TweakRegistry {
    GLOBAL_REGISTRY.get_or_init(TweakRegistry::new)
}

/// Convenience function querying or registering a default in the global registry.
///
/// Matches the code generated by `#[tweak]` procedural macro.
///
/// # Examples
///
/// ```
/// use martensite_devtools::tweak::register_or_get;
///
/// let val = register_or_get("global/pad", 8.0f32);
/// assert_eq!(val, 8.0);
/// ```
pub fn register_or_get<T: Tweakable>(name: &str, default: T) -> T {
    global().register_or_get(name, default)
}

/// Alias for [`register_or_get`] matching the `martensite-macros` expansion.
///
/// # Examples
///
/// ```
/// use martensite_devtools::tweak::get_or_register;
///
/// let val = get_or_register("global/spacing", 12.0f32);
/// assert_eq!(val, 12.0);
/// ```
pub fn get_or_register<T: Tweakable>(name: &str, default: T) -> T {
    global().get_or_register(name, default)
}

/// Convenience function linking a reactive signal to the global registry.
///
/// # Examples
///
/// ```
/// use martensite_devtools::tweak::register_signal;
/// use martensite_reactive::Signal;
///
/// let sig = Signal::new(5.0f32);
/// register_signal("global/scale", &sig);
/// ```
pub fn register_signal<S: AsSignalId>(name: &str, signal: S) {
    global().register_signal(name, signal);
}

/// Convenience function updating a live tweak value in the global registry.
///
/// # Examples
///
/// ```
/// use martensite_devtools::tweak::{register_or_get, set_value};
///
/// register_or_get("global/dim", 10.0f32);
/// set_value("global/dim", 20.0f32);
/// ```
pub fn set_value(name: &str, val: impl Into<TweakValue>) -> bool {
    global().set_value(name, val)
}

/// Convenience function emitting a source patch for a named tweak from the global registry.
///
/// # Examples
///
/// ```
/// use martensite_devtools::tweak::{emit_patch, global, SourceSpan};
///
/// let span = SourceSpan::new("src/ui.rs", 100, 1);
/// global().register_or_get_with_span("global/margin", 8.0f32, span, "margin");
/// global().set_value("global/margin", 16.0f32);
///
/// let patch = emit_patch("global/margin").expect("patch available");
/// assert_eq!(patch, "src/ui.rs:100: .margin(8.0) -> .margin(16.0)");
/// ```
pub fn emit_patch(name: &str) -> Option<String> {
    global().emit_patch(name)
}

/// Convenience function emitting all source patches for modified tweaks in the global registry.
///
/// # Examples
///
/// ```
/// use martensite_devtools::tweak::emit_all_patches;
///
/// let _patches = emit_all_patches();
/// ```
pub fn emit_all_patches() -> Vec<String> {
    global().emit_all_patches()
}

/// Convenience function resetting all modified tweaks in the global registry to defaults.
///
/// # Examples
///
/// ```
/// use martensite_devtools::tweak::reset_all;
///
/// reset_all();
/// ```
pub fn reset_all() {
    global().reset_all();
}

/// Clears all entries from the global registry.
///
/// Primarily used in tests to ensure clean state isolation.
///
/// # Examples
///
/// ```
/// use martensite_devtools::tweak::clear;
///
/// clear();
/// ```
pub fn clear() {
    global().clear();
}
