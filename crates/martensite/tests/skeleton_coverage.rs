//! ADR-0040 §6 — mechanically-derived skeleton-coverage conformance.
//!
//! ADR-0040 requires the adopter set to be *derived, not hand-listed*
//! (AGENTS.md §7: enumerations are generated, never hand-maintained).
//! This test therefore names zero widget types. It derives skeleton
//! participation from the same source of truth the compiler uses —
//! the `pub mod` declarations in `src/widgets/mod.rs` — and scans every
//! registered module's source for the loading-state protocol's internal
//! consistency:
//!
//! 1. **Stored flag ⇒ trait override.** A widget struct declaring a
//!    `loading` field that implements `Widget` must override
//!    `Widget::is_loading`. The arena only ever sees the trait method
//!    (`effective_loading = NodeFlags::LOADING || widget.is_loading()`),
//!    so a stored flag without the override is dead state — exactly the
//!    "stored flag, forgotten override" drift flagged at review.
//! 2. **Override is not a stub.** Every `fn is_loading` inside an
//!    `impl Widget for _` must read a `loading`-named value —
//!    `{ false }` or `{ self.busy }` cannot satisfy the contract.
//! 3. **Setter flips what the trait reads.** Every `set_loading` on a
//!    `Widget` type must assign to a `self.*loading` path that the
//!    type's `is_loading` override reads back; otherwise the setter
//!    cannot change the observable trait state.
//! 4. **A runtime round-trip exists per adopter.** `Widget::as_any_mut`
//!    is `devtools-timemachine`-gated, so this test cannot drive
//!    `dyn Widget` downcasts for a generic round-trip. It instead
//!    *requires* every module declaring a full adopter
//!    (`set_loading` + trait `is_loading`) to contain a `#[test]` that
//!    calls `set_loading` and observes the flag through trait dispatch
//!    (`Widget::is_loading(&w)` / `<T as Widget>::is_loading(&w)`) —
//!    the runtime proof is mandatory, and its presence is checked
//!    mechanically.
//! 5. **Registry reachability.** Every `src/widgets/*.rs` file carrying
//!    a trait `is_loading` must be declared `pub mod` in `mod.rs`, and
//!    every full adopter module must be `pub use`d — a new adopter
//!    cannot exist as an unreachable orphan file.
//!
//! Non-adopters are intentionally absent from the participant set: the
//! `Widget::is_loading` default (`false`) gives every other widget
//! correct behavior — the generic shared-shimmer placeholder, subtree
//! suppression, `busy` a11y marking — with zero per-widget code. That
//! is the ADR-0040 design: "shape-accurate" participation is
//! refinement, not a migration gate.
//!
//! The scanner leans on `cargo fmt --check` being a CI gate: it reads
//! top-level items by brace depth on comment/string-stripped source.

#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Source scanner — comment/string-stripped, brace-depth item extraction.
// ---------------------------------------------------------------------------

fn widgets_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src/widgets")
}

fn blank(out: &mut [u8], i: usize) {
    if out[i] != b'\n' {
        out[i] = b' ';
    }
}

/// `src` with comments and string/char-literal contents blanked to
/// spaces, preserving byte positions and newlines so braces still
/// balance and line numbers map back to the original file.
fn strip_noncode(src: &str) -> String {
    let b = src.as_bytes();
    let mut out = b.to_vec();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            // `//` line comment (covers `///` and `//!` docs).
            b'/' if i + 1 < b.len() && b[i + 1] == b'/' => {
                while i < b.len() && b[i] != b'\n' {
                    blank(&mut out, i);
                    i += 1;
                }
            }
            // `/* ... */` block comment (nesting-capable in Rust).
            b'/' if i + 1 < b.len() && b[i + 1] == b'*' => {
                blank(&mut out, i);
                blank(&mut out, i + 1);
                i += 2;
                let mut depth = 1usize;
                while i < b.len() && depth > 0 {
                    if b[i] == b'/' && i + 1 < b.len() && b[i + 1] == b'*' {
                        depth += 1;
                        blank(&mut out, i);
                        blank(&mut out, i + 1);
                        i += 2;
                    } else if b[i] == b'*' && i + 1 < b.len() && b[i + 1] == b'/' {
                        depth -= 1;
                        blank(&mut out, i);
                        blank(&mut out, i + 1);
                        i += 2;
                    } else {
                        blank(&mut out, i);
                        i += 1;
                    }
                }
            }
            // Normal `"..."` string (also the tail of `b"..."`/`br"..."`
            // once the prefix bytes pass through).
            b'"' => {
                blank(&mut out, i);
                i += 1;
                while i < b.len() && b[i] != b'"' {
                    if b[i] == b'\\' {
                        blank(&mut out, i);
                        if i + 1 < b.len() {
                            blank(&mut out, i + 1);
                            i += 2;
                        } else {
                            i += 1;
                        }
                    } else {
                        blank(&mut out, i);
                        i += 1;
                    }
                }
                if i < b.len() {
                    blank(&mut out, i);
                    i += 1;
                }
            }
            // Raw strings: `r"..."`, `r#"..."#`, `r##"..."##`, `br"..."`,
            // `br#"..."#`. Entered only when the bytes line up.
            b'r' | b'b'
                if (b[i] == b'r' && i + 1 < b.len() && (b[i + 1] == b'"' || b[i + 1] == b'#'))
                    || (b[i] == b'b'
                        && i + 2 < b.len()
                        && b[i + 1] == b'r'
                        && (b[i + 2] == b'"' || b[i + 2] == b'#')) =>
            {
                let mut j = i + if b[i] == b'b' { 2 } else { 1 };
                let mut hashes = 0usize;
                while j < b.len() && b[j] == b'#' {
                    hashes += 1;
                    j += 1;
                }
                if j < b.len() && b[j] == b'"' {
                    while i <= j {
                        blank(&mut out, i);
                        i += 1;
                    }
                    while i < b.len() {
                        if b[i] == b'"' {
                            let mut k = i + 1;
                            let mut closed = true;
                            for _ in 0..hashes {
                                if k < b.len() && b[k] == b'#' {
                                    k += 1;
                                } else {
                                    closed = false;
                                    break;
                                }
                            }
                            if closed {
                                while i <= k && i < b.len() {
                                    blank(&mut out, i);
                                    i += 1;
                                }
                                break;
                            }
                        }
                        blank(&mut out, i);
                        i += 1;
                    }
                } else {
                    i += 1;
                }
            }
            // `'x'` char literal vs `'a` lifetime: a quote is a literal
            // iff a closing quote follows one char or one escape.
            b'\'' => {
                if i + 2 < b.len() && b[i + 1] != b'\\' && b[i + 2] == b'\'' {
                    for k in i..=i + 2 {
                        blank(&mut out, k);
                    }
                    i += 3;
                } else if i + 3 < b.len() && b[i + 1] == b'\\' && b[i + 3] == b'\'' {
                    for k in i..=i + 3 {
                        blank(&mut out, k);
                    }
                    i += 4;
                } else {
                    i += 1; // lifetime — keep it
                }
            }
            _ => i += 1,
        }
    }
    String::from_utf8(out).expect("blanking preserves UTF-8")
}

fn is_ident(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// `true` when keyword `kw` starts at `i` with identifier boundaries.
fn kw_at(b: &[u8], i: usize, kw: &str) -> bool {
    let k = kw.as_bytes();
    i + k.len() <= b.len()
        && b[i..i + k.len()] == *k
        && (i == 0 || !is_ident(b[i - 1]))
        && (i + k.len() == b.len() || !is_ident(b[i + k.len()]))
}

fn skip_ws(b: &[u8], mut i: usize) -> usize {
    while i < b.len() && b[i].is_ascii_whitespace() {
        i += 1;
    }
    i
}

fn read_ident(b: &[u8], i: usize) -> (String, usize) {
    let start = i;
    let mut j = i;
    while j < b.len() && is_ident(b[j]) {
        j += 1;
    }
    (
        String::from_utf8_lossy(&b[start..j]).into_owned(),
        j.max(start + 1),
    )
}

/// Skip a `<...>` generic-parameter list starting at `b[i] == '<'`.
/// Returns the position after the matching `>`, or `i` unchanged.
fn skip_generics(b: &[u8], i: usize) -> usize {
    if i >= b.len() || b[i] != b'<' {
        return i;
    }
    let mut depth = 0i32;
    let mut j = i;
    while j < b.len() {
        match b[j] {
            b'<' => depth += 1,
            b'>' => {
                depth -= 1;
                if depth == 0 {
                    return j + 1;
                }
            }
            b'{' | b'}' | b';' => return i,
            _ => {}
        }
        j += 1;
    }
    i
}

/// Index of the `}` matching the `{` at `open`.
fn matching_brace(b: &[u8], open: usize) -> usize {
    let mut depth = 0i32;
    let mut j = open;
    while j < b.len() {
        match b[j] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return j;
                }
            }
            _ => {}
        }
        j += 1;
    }
    b.len().saturating_sub(1)
}

fn line_of(src: &str, byte_pos: usize) -> usize {
    src[..byte_pos].bytes().filter(|&c| c == b'\n').count() + 1
}

/// A braced `struct` declaration: its 1-based line and field names.
struct StructDecl {
    fields: Vec<String>,
    line: usize,
}

/// Method name → (byte offset in `body`, fn body text) for `fn` items
/// at relative depth 0 — i.e. the direct methods of an impl block.
fn scan_methods(body: &str) -> BTreeMap<String, (usize, String)> {
    let b = body.as_bytes();
    let mut out = BTreeMap::new();
    let (mut brace, mut paren, mut bracket) = (0i32, 0i32, 0i32);
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'{' => brace += 1,
            b'}' => brace -= 1,
            b'(' => paren += 1,
            b')' => paren -= 1,
            b'[' => bracket += 1,
            b']' => bracket -= 1,
            _ if brace == 0 && paren == 0 && bracket == 0 && kw_at(b, i, "fn") => {
                let mut j = skip_ws(b, i + 2);
                let (name, after) = read_ident(b, j);
                j = after;
                // The signature ends at the body `{` or a `;` (decl).
                let (mut pd, mut bd) = (0i32, 0i32);
                let mut fbody = None;
                while j < b.len() {
                    match b[j] {
                        b'(' => pd += 1,
                        b')' => pd -= 1,
                        b'[' => bd += 1,
                        b']' => bd -= 1,
                        b'{' if pd == 0 && bd == 0 => {
                            fbody = Some(j);
                            break;
                        }
                        b';' if pd == 0 && bd == 0 => break,
                        _ => {}
                    }
                    j += 1;
                }
                if let Some(open) = fbody {
                    let close = matching_brace(b, open);
                    out.insert(name, (i, body[open + 1..close].to_string()));
                    i = close + 1;
                    continue;
                }
            }
            _ => {}
        }
        i += 1;
    }
    out
}

/// Field names of a braced struct body: the last identifier before the
/// first top-level `:` in each comma-separated field.
fn scan_fields(body: &str) -> Vec<String> {
    let b = body.as_bytes();
    let mut fields = Vec::new();
    let mut idents: Vec<String> = Vec::new();
    let mut saw_colon = false;
    let (mut angle, mut paren, mut bracket) = (0i32, 0i32, 0i32);
    let mut i = 0;
    while i < b.len() {
        let at_depth0 = angle == 0 && paren == 0 && bracket == 0;
        match b[i] {
            b'<' => angle += 1,
            b'>' => angle = (angle - 1).max(0),
            b'(' => paren += 1,
            b')' => paren -= 1,
            b'[' => bracket += 1,
            b']' => bracket -= 1,
            b':' if at_depth0 => {
                if i + 1 < b.len() && b[i + 1] == b':' {
                    i += 1; // `::` path separator, not a field colon
                } else if !saw_colon {
                    // Field name = last identifier before the colon;
                    // `pub` / `pub(crate)` visibility precedes it.
                    if let Some(name) = idents.last() {
                        fields.push(name.clone());
                    }
                    saw_colon = true;
                }
            }
            b',' if at_depth0 => {
                saw_colon = false;
                idents.clear();
            }
            _ if at_depth0
                && !saw_colon
                && is_ident(b[i])
                && !b[i].is_ascii_digit()
                && (i == 0 || !is_ident(b[i - 1])) =>
            {
                let (name, after) = read_ident(b, i);
                idents.push(name);
                i = after;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    fields
}

/// Whether the impl header contains a `for` keyword (a trait impl).
fn header_has_for(header: &str) -> bool {
    let b = header.as_bytes();
    (0..b.len()).any(|i| kw_at(b, i, "for"))
}

/// Parse an `impl` header (text between `impl` and `{`) into
/// `(is_widget_impl, target_type)`.
fn parse_impl_header(header: &str) -> (bool, String) {
    let b = header.as_bytes();
    let mut i = skip_ws(b, 0);
    if kw_at(b, i, "unsafe") {
        i = skip_ws(b, i + "unsafe".len());
    }
    i = skip_generics(b, i);
    let rest = header.as_bytes()[i.min(header.len())..].to_vec();
    // Identifier stream split at the `for` keyword.
    let mut idents = Vec::new();
    let mut for_at = None;
    let mut j = 0;
    while j < rest.len() {
        if is_ident(rest[j]) && !rest[j].is_ascii_digit() && (j == 0 || !is_ident(rest[j - 1])) {
            let (name, after) = read_ident(&rest, j);
            if name == "for" && for_at.is_none() {
                for_at = Some(idents.len());
            } else {
                idents.push(name);
            }
            j = after;
        } else {
            j += 1;
        }
    }
    match for_at {
        // `impl Trait for Type` — the last ident before `for` is the
        // trait's final path segment; the first after is the type.
        Some(k) => {
            let trait_last = k
                .checked_sub(1)
                .and_then(|p| idents.get(p))
                .cloned()
                .unwrap_or_default();
            let target = idents.get(k).cloned().unwrap_or_default();
            (trait_last == "Widget", target)
        }
        // Inherent `impl Type` — first ident is the target.
        None => (false, idents.first().cloned().unwrap_or_default()),
    }
}

struct ModuleScan {
    /// `impl Widget for T` methods, keyed by target type then method.
    widget_impls: BTreeMap<String, BTreeMap<String, (usize, String)>>,
    /// Inherent `impl T` methods, keyed by target type then method.
    inherent_impls: BTreeMap<String, BTreeMap<String, (usize, String)>>,
    /// `struct T { .. }` declarations, keyed by type name.
    structs: BTreeMap<String, StructDecl>,
    /// Bodies of every `#[test]` function in the file.
    test_bodies: Vec<String>,
    /// Stripped source — used for line-number reports.
    stripped: String,
}

fn scan_module_source(path: &Path) -> ModuleScan {
    let src =
        fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    let stripped = strip_noncode(&src);
    let b = stripped.as_bytes();

    let mut widget_impls: BTreeMap<String, BTreeMap<String, (usize, String)>> = BTreeMap::new();
    let mut inherent_impls: BTreeMap<String, BTreeMap<String, (usize, String)>> = BTreeMap::new();
    let mut structs: BTreeMap<String, StructDecl> = BTreeMap::new();
    let mut test_bodies: Vec<String> = Vec::new();

    // Top-level items sit at brace depth 0 — impls/structs nested in
    // `mod tests`, function bodies, and macros are all deeper.
    let mut depth = 0i32;
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'{' => depth += 1,
            b'}' => depth -= 1,
            _ if depth == 0 && kw_at(b, i, "struct") => {
                let mut j = skip_ws(b, i + "struct".len());
                let (name, after) = read_ident(b, j);
                j = skip_generics(b, skip_ws(b, after));
                if j < b.len() && b[j] == b'{' {
                    let close = matching_brace(b, j);
                    let fields = scan_fields(&stripped[j + 1..close]);
                    structs.insert(
                        name,
                        StructDecl {
                            fields,
                            line: line_of(&stripped, i),
                        },
                    );
                    i = close + 1;
                    continue;
                }
                // Tuple/unit structs have no named fields — skip.
            }
            _ if depth == 0 && kw_at(b, i, "impl") => {
                let mut j = i + "impl".len();
                while j < b.len() && b[j] != b'{' && b[j] != b';' {
                    j += 1;
                }
                if j < b.len() && b[j] == b'{' {
                    let header = &stripped[i + 4..j];
                    let (is_widget, target) = parse_impl_header(header);
                    let close = matching_brace(b, j);
                    // Method offsets are relative to the impl body —
                    // rebase them onto file positions for diagnostics.
                    let methods: BTreeMap<String, (usize, String)> =
                        scan_methods(&stripped[j + 1..close])
                            .into_iter()
                            .map(|(n, (p, body))| (n, (p + j + 1, body)))
                            .collect();
                    if is_widget {
                        widget_impls.entry(target).or_default().extend(methods);
                    } else if !target.is_empty() && !header_has_for(header) {
                        inherent_impls.entry(target).or_default().extend(methods);
                    }
                    i = close + 1;
                    continue;
                }
            }
            _ => {}
        }
        i += 1;
    }

    // `#[test]` functions live inside `mod tests` — deeper than the
    // top-level scan, so find them by attribute marker.
    let mut pos = 0;
    while let Some(rel) = stripped[pos..].find("#[test]") {
        let at = pos + rel;
        let mut j = at + "#[test]".len();
        while j < b.len() && !kw_at(b, j, "fn") {
            j += 1;
        }
        if j >= b.len() {
            break;
        }
        j = skip_ws(b, j + 2);
        let (_name, after) = read_ident(b, j);
        let mut k = after;
        while k < b.len() && b[k] != b'{' {
            k += 1;
        }
        if k < b.len() {
            let close = matching_brace(b, k);
            test_bodies.push(stripped[k + 1..close].to_string());
            pos = close + 1;
        } else {
            pos = j + 1;
        }
    }

    ModuleScan {
        widget_impls,
        inherent_impls,
        structs,
        test_bodies,
        stripped,
    }
}

// ---------------------------------------------------------------------------
// Registry + derived participant set
// ---------------------------------------------------------------------------

/// The widget registry: `pub mod NAME;` declarations plus `pub use
/// NAME::` re-exports in `src/widgets/mod.rs`.
fn registry() -> (BTreeSet<String>, BTreeSet<String>) {
    let path = widgets_dir().join("mod.rs");
    let src = fs::read_to_string(&path).expect("read src/widgets/mod.rs");
    let stripped = strip_noncode(&src);
    let mut modules = BTreeSet::new();
    let mut reexported = BTreeSet::new();
    for line in stripped.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("pub mod ") {
            if let Some(name) = rest.strip_suffix(';') {
                modules.insert(name.trim().to_string());
            }
        }
        if let Some(rest) = t.strip_prefix("pub use ") {
            if let Some(name) = rest.split("::").next() {
                let name = name.trim();
                if !name.is_empty() {
                    reexported.insert(name.to_string());
                }
            }
        }
    }
    (modules, reexported)
}

/// `.rs` files directly under `src/widgets/` (basenames) — the on-disk
/// set, scanned to detect orphan adopter files.
fn disk_modules() -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for entry in fs::read_dir(widgets_dir()).expect("read_dir src/widgets") {
        let p = entry.expect("dir entry").path();
        if p.extension().and_then(|e| e.to_str()) == Some("rs")
            && p.file_stem().and_then(|s| s.to_str()) != Some("mod")
        {
            out.insert(
                p.file_stem()
                    .and_then(|s| s.to_str())
                    .expect("utf8 name")
                    .to_string(),
            );
        }
    }
    out
}

fn module_path(name: &str) -> PathBuf {
    let flat = widgets_dir().join(format!("{name}.rs"));
    if flat.exists() {
        flat
    } else {
        widgets_dir().join(name).join("mod.rs")
    }
}

/// Module has ≥1 `fn is_loading` inside an `impl Widget` — the derived
/// skeleton participant marker.
fn is_participant(scan: &ModuleScan) -> bool {
    scan.widget_impls
        .values()
        .any(|m| m.contains_key("is_loading"))
}

/// Module has a full adopter type: `set_loading` (inherent) plus a
/// `Widget::is_loading` override on the same type.
fn has_adopter(scan: &ModuleScan) -> bool {
    scan.widget_impls.iter().any(|(ty, methods)| {
        methods.contains_key("is_loading")
            && scan
                .inherent_impls
                .get(ty)
                .is_some_and(|inh| inh.contains_key("set_loading"))
    })
}

/// Body contains an identifier containing `loading` — reads a flag or
/// forwards to a `loading`-named accessor; not a stub.
fn reads_loading(body: &str) -> bool {
    body.split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .any(|tok| tok.contains("loading"))
}

/// Paths assigned on the LHS of `=` in `body` that end in a `loading`
/// segment — e.g. `self.loading`, `self.shared.lock().loading`.
fn loading_lhs_paths(body: &str) -> Vec<String> {
    let b = body.as_bytes();
    let mut out = Vec::new();
    for i in 0..b.len() {
        let is_assign = b[i] == b'='
            && (i + 1 >= b.len() || (b[i + 1] != b'=' && b[i + 1] != b'>'))
            && (i == 0
                || (b[i - 1] != b'=' && b[i - 1] != b'!' && b[i - 1] != b'<' && b[i - 1] != b'>'));
        if !is_assign {
            continue;
        }
        let mut j = i;
        while j > 0 && b[j - 1].is_ascii_whitespace() {
            j -= 1;
        }
        let end = j;
        while j > 0 && (is_ident(b[j - 1]) || matches!(b[j - 1], b'.' | b'(' | b')' | b'[' | b']'))
        {
            j -= 1;
        }
        let lhs = body[j..end].to_string();
        if lhs.ends_with("loading") {
            out.push(lhs);
        }
    }
    out
}

/// `path` occurs in `body` ending on a non-identifier boundary, so
/// `self.loading` does not match `self.loading_flag`.
fn contains_path(body: &str, path: &str) -> bool {
    let mut from = 0;
    while let Some(at) = body[from..].find(path) {
        let end = from + at + path.len();
        if end == body.len() || !is_ident(body.as_bytes()[end]) {
            return true;
        }
        from += at + 1;
    }
    false
}

/// A `#[test]` body that calls `set_loading` and observes the flag
/// through trait dispatch — `Widget::is_loading(&w)` or
/// `<T as Widget>::is_loading(&w)` — not merely the inherent getter,
/// which could shadow a broken trait override.
fn is_roundtrip_test(body: &str) -> bool {
    body.contains(".set_loading(")
        && (body.contains("Widget::is_loading(") || body.contains("Widget>::is_loading("))
}

fn scan_all() -> (
    BTreeMap<String, ModuleScan>,
    BTreeSet<String>,
    BTreeSet<String>,
) {
    let (modules, reexported) = registry();
    let mut scans = BTreeMap::new();
    for name in &modules {
        scans.insert(name.clone(), scan_module_source(&module_path(name)));
    }
    (scans, modules, reexported)
}

// ---------------------------------------------------------------------------
// Conformance assertions
// ---------------------------------------------------------------------------

/// (c) Participants are reachable through the widget registry: no
/// source file may carry a trait `is_loading` without a `pub mod`
/// declaration, and every full adopter module must be re-exported at
/// the facade (`martensite::widgets::Type`).
#[test]
fn participants_are_registered_and_reexported() {
    let (scans, registered, reexported) = scan_all();
    assert!(
        registered.len() >= 100,
        "registry parse degenerate: found {} `pub mod` declarations in \
         src/widgets/mod.rs — the scanner or the module layout changed",
        registered.len()
    );

    // Orphan files: is_loading on disk but not in `pub mod`.
    for file in disk_modules() {
        if registered.contains(&file) {
            continue;
        }
        let scan = scan_module_source(&module_path(&file));
        assert!(
            !is_participant(&scan),
            "src/widgets/{file}.rs overrides Widget::is_loading but is \
             not declared `pub mod {file};` in src/widgets/mod.rs — the \
             adopter is unreachable through the widget registry"
        );
    }

    // Full adopters must be re-exported (`pub use name::`).
    let unexported: Vec<&String> = scans
        .iter()
        .filter(|(name, scan)| has_adopter(scan) && !reexported.contains(*name))
        .map(|(name, _)| name)
        .collect();
    assert!(
        unexported.is_empty(),
        "adopter modules missing `pub use` re-export in \
         src/widgets/mod.rs: {unexported:?}"
    );

    // Sanity anchor: `Skeleton` — the protocol's own widget — must be
    // derived as a participant, proving the derivation is not vacuous.
    let count = scans.values().filter(|s| is_participant(s)).count();
    assert!(
        count > 0 && scans.get("skeleton").is_some_and(is_participant),
        "mechanical derivation found {count} participant modules but \
         not `skeleton` — the scanner regressed"
    );
}

/// (a) Stored flag ⇒ trait override; plus the stub guard: every
/// `is_loading` in an `impl Widget` must read a `loading`-named value.
#[test]
fn stored_flags_override_is_loading() {
    let (scans, _, _) = scan_all();

    for (module, scan) in &scans {
        // A `loading` field on a Widget-implementing struct must be
        // surfaced through the trait — the arena has no other view of
        // it. Non-Widget state structs (e.g. shared popup state) are
        // exempt: the impls reading them are covered by the stub rule.
        for (ty, decl) in &scan.structs {
            if !decl.fields.iter().any(|f| f == "loading") {
                continue;
            }
            let Some(methods) = scan.widget_impls.get(ty) else {
                continue;
            };
            assert!(
                methods.contains_key("is_loading"),
                "{module}.rs:{line}: `{ty}` stores a `loading` field and \
                 implements Widget but does not override \
                 Widget::is_loading — the arena can never observe the \
                 flag (ADR-0040 stored-flag drift)",
                line = decl.line
            );
        }

        // Overrides must read a loading value — not return a constant.
        for (ty, methods) in &scan.widget_impls {
            if let Some((pos, body)) = methods.get("is_loading") {
                assert!(
                    reads_loading(body),
                    "{module}.rs:{line}: `impl Widget for {ty}` has an \
                     is_loading override that reads no `loading`-named \
                     value — stub or unrelated state?",
                    line = line_of(&scan.stripped, *pos)
                );
            }
        }
    }
}

/// (b, source half) `set_loading` must write a `loading` path that the
/// type's `Widget::is_loading` reads back.
#[test]
fn set_loading_wires_the_flag_is_loading_reads() {
    let (scans, _, _) = scan_all();

    for (module, scan) in &scans {
        for (ty, inherent) in &scan.inherent_impls {
            let Some((set_pos, set_body)) = inherent.get("set_loading") else {
                continue;
            };
            let Some(widget_methods) = scan.widget_impls.get(ty) else {
                continue; // set_loading on a non-Widget helper
            };
            let Some((_, is_body)) = widget_methods.get("is_loading") else {
                panic!(
                    "{module}.rs:{line}: `{ty}` defines set_loading and \
                     implements Widget but does not override \
                     Widget::is_loading — the setter cannot change the \
                     observable trait state",
                    line = line_of(&scan.stripped, *set_pos)
                );
            };
            let writes = loading_lhs_paths(set_body);
            assert!(
                !writes.is_empty(),
                "{module}.rs:{line}: `{ty}::set_loading` assigns no \
                 `self.*loading` path — it does not store the flag at all",
                line = line_of(&scan.stripped, *set_pos)
            );
            assert!(
                writes.iter().any(|p| contains_path(is_body, p)),
                "{module}.rs: `{ty}::set_loading` writes {writes:?} but \
                 `Widget::is_loading` reads none of those paths — the \
                 setter and the trait observe different state"
            );
        }
    }
}

/// (b, runtime half) Every module declaring a full adopter must carry
/// a `#[test]` proving the flag round-trips through trait dispatch.
#[test]
fn every_adopter_has_trait_observed_roundtrip_test() {
    let (scans, _, _) = scan_all();

    for (module, scan) in &scans {
        if !has_adopter(scan) {
            continue;
        }
        assert!(
            scan.test_bodies.iter().any(|b| is_roundtrip_test(b)),
            "{module}.rs: adopter declares set_loading + \
             Widget::is_loading but no #[test] calls set_loading and \
             observes Widget::is_loading — the runtime round-trip proof \
             is missing (e.g. `w.set_loading(true); \
             assert!(Widget::is_loading(&w));`)"
        );
    }
}
