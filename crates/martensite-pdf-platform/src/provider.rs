//! Subprocess PDF document — `open`/`info`/`page_size`/`render_page`
//! driven by `pdfinfo`/`pdftoppm` (poppler) or `mutool` (mupdf).
//!
//! Wire types ([`CliDocInfo`], [`CliPageSize`], [`CliBitmap`],
//! [`CliSource`], [`CliError`]) are defined here rather than imported
//! from `martensite-pdf` — the platform crate cannot depend on the
//! safe crate or the `platform` feature would form a dependency
//! cycle. `martensite-pdf` adapts these onto `PdfDocument`.
//!
//! # Examples
//!
//! ```no_run
//! use martensite_pdf_platform::{open_document, CliSource};
//! use std::path::PathBuf;
//!
//! let doc = open_document(&CliSource::File(PathBuf::from("spec.pdf")))?;
//! assert!(doc.info().page_count > 0);
//! # Ok::<(), martensite_pdf_platform::CliError>(())
//! ```

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};

use crate::ppm::{decode_ppm, decode_ppm_header};

/// Document-level metadata (mirrors `martensite_pdf::PdfDocInfo`).
///
/// # Examples
///
/// ```
/// use martensite_pdf_platform::CliDocInfo;
///
/// let i = CliDocInfo::default();
/// assert_eq!(i.page_count, 0);
/// ```
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CliDocInfo {
    /// `Title` from the document info dictionary, when present.
    pub title: Option<String>,
    /// `Author` from the document info dictionary, when present.
    pub author: Option<String>,
    /// Number of pages.
    pub page_count: u32,
}

/// A page's size in PDF points (mirrors
/// `martensite_pdf::PageSize`).
///
/// # Examples
///
/// ```
/// use martensite_pdf_platform::CliPageSize;
///
/// assert_eq!(CliPageSize::LETTER.width, 612.0);
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CliPageSize {
    /// Width in points.
    pub width: f32,
    /// Height in points.
    pub height: f32,
}

impl CliPageSize {
    /// US Letter — 8.5 × 11 in.
    pub const LETTER: CliPageSize = CliPageSize {
        width: 612.0,
        height: 792.0,
    };
}

/// One rasterized page: tightly-packed RGBA8 (mirrors
/// `martensite_pdf::PdfPageBitmap`).
///
/// # Examples
///
/// ```
/// use martensite_pdf_platform::CliBitmap;
///
/// let b = CliBitmap { width: 1, height: 1, pixels: vec![0, 0, 0, 255] };
/// assert_eq!(b.pixels.len(), 4);
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct CliBitmap {
    /// Pixel width.
    pub width: u32,
    /// Pixel height.
    pub height: u32,
    /// `width * height * 4` RGBA bytes.
    pub pixels: Vec<u8>,
}

/// Where [`open_document`] reads from (mirrors
/// `martensite_pdf::PdfSource`).
///
/// # Examples
///
/// ```
/// use martensite_pdf_platform::CliSource;
///
/// let s = CliSource::Bytes(vec![37, 80, 68, 70]); // "%PDF"
/// assert!(matches!(s, CliSource::Bytes(_)));
/// ```
#[derive(Clone, Debug)]
pub enum CliSource {
    /// A file on disk.
    File(PathBuf),
    /// An in-memory PDF byte stream (materialized to a temp file).
    Bytes(Vec<u8>),
}

/// An open/parse/render failure (mirrors `martensite_pdf::PdfError`).
///
/// # Examples
///
/// ```
/// use martensite_pdf_platform::CliError;
///
/// let e = CliError::Unsupported("x".into());
/// assert!(e.to_string().contains("x"));
/// ```
#[derive(Clone, Debug, PartialEq)]
pub enum CliError {
    /// The document could not be opened or parsed.
    OpenFailed(String),
    /// No rasterizer CLI is installed.
    Unsupported(String),
}

impl std::fmt::Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CliError::OpenFailed(e) => write!(f, "open failed: {e}"),
            CliError::Unsupported(e) => write!(f, "unsupported: {e}"),
        }
    }
}

impl std::error::Error for CliError {}

/// Which rasterizer toolset is available.
///
/// # Examples
///
/// ```
/// use martensite_pdf_platform::PdfBackend;
///
/// assert_eq!(PdfBackend::Poppler.name(), "poppler");
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PdfBackend {
    /// `pdfinfo` + `pdftoppm` (poppler-utils).
    Poppler,
    /// `mutool info` + `mutool draw` (mupdf-tools).
    Mupdf,
}

impl PdfBackend {
    /// Short toolset name.
    ///
    /// ```
    /// use martensite_pdf_platform::PdfBackend;
    ///
    /// assert_eq!(PdfBackend::Mupdf.name(), "mupdf");
    /// ```
    pub fn name(self) -> &'static str {
        match self {
            PdfBackend::Poppler => "poppler",
            PdfBackend::Mupdf => "mupdf",
        }
    }
}

/// Probe `$PATH` for a usable rasterizer: poppler preferred, mupdf
/// fallback, `None` when neither is installed. The result is cached
/// — probing spawns 2–3 probe processes, so it happens once.
///
/// ```
/// use martensite_pdf_platform::probe_backend;
///
/// // CI/dev machines without poppler-utils get `None` — callers
/// // must handle that rather than unwrap.
/// let _ = probe_backend();
/// ```
pub fn probe_backend() -> Option<PdfBackend> {
    static PROBE: OnceLock<Option<PdfBackend>> = OnceLock::new();
    *PROBE.get_or_init(|| {
        if have("pdftoppm") && have("pdfinfo") {
            Some(PdfBackend::Poppler)
        } else if have("mutool") {
            Some(PdfBackend::Mupdf)
        } else {
            None
        }
    })
}

/// Open `source` with the best probed backend.
///
/// ```no_run
/// use martensite_pdf_platform::{open_document, CliSource};
///
/// let r = open_document(&CliSource::Bytes(b"%PDF-1.4".to_vec()));
/// // Errors `Unsupported` without poppler/mupdf installed.
/// let _ = r;
/// ```
pub fn open_document(source: &CliSource) -> Result<SubprocessDocument, CliError> {
    let backend = probe_backend().ok_or_else(|| {
        CliError::Unsupported("no PDF CLI found (install poppler-utils or mupdf-tools)".to_string())
    })?;
    SubprocessDocument::open(source, backend)
}

/// A document rendered by a CLI rasterizer — see the module docs.
///
/// Page sizes are discovered lazily and cached behind a `Mutex`
/// (failures cached too — a broken page doesn't re-spawn the info
/// tool per query), so all accessors are `&self` — matching
/// `martensite-pdf`'s `PdfDocument` contract.
///
/// ```no_run
/// use martensite_pdf_platform::{open_document, CliSource};
/// use std::path::PathBuf;
///
/// let doc = open_document(&CliSource::File(PathBuf::from("a.pdf")))?;
/// assert!(doc.page_size(0).is_some());
/// # Ok::<(), martensite_pdf_platform::CliError>(())
/// ```
pub struct SubprocessDocument {
    /// Path the rasterizer reads — either the caller's canonicalized
    /// file or a temp file this document owns (`temp_owned == true`).
    path: PathBuf,
    /// `true` when `path` is a temp file we must delete on drop.
    temp_owned: bool,
    backend: PdfBackend,
    info: CliDocInfo,
    /// Lazily discovered per-page sizes; presence = probed
    /// (`Some(None)` caches a failed probe too).
    sizes: Mutex<HashMap<u32, Option<CliPageSize>>>,
}

impl std::fmt::Debug for SubprocessDocument {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SubprocessDocument")
            .field("backend", &self.backend)
            .field("pages", &self.info.page_count)
            .field("path", &self.path)
            .finish()
    }
}

impl Drop for SubprocessDocument {
    fn drop(&mut self) {
        if self.temp_owned {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// Upper bound on a sane `Pages:` value — generous enough for any
/// real document (hundred-thousand-page log dumps) while rejecting
/// corrupt `u32::MAX`-style metadata.
const MAX_PAGES: u32 = 1_000_000;

impl SubprocessDocument {
    /// Open `source` with an explicit `backend` (skips probing).
    ///
    /// ```no_run
    /// use martensite_pdf_platform::{
    ///     open_document, CliSource, PdfBackend, SubprocessDocument,
    /// };
    /// use std::path::PathBuf;
    ///
    /// let doc = SubprocessDocument::open(
    ///     &CliSource::File(PathBuf::from("spec.pdf")),
    ///     PdfBackend::Poppler,
    /// );
    /// ```
    pub fn open(source: &CliSource, backend: PdfBackend) -> Result<Self, CliError> {
        let (path, temp_owned) = match source {
            // Canonicalize: a `-`-prefixed basename could otherwise be
            // consumed as a flag by the CLIs, and the tools only ever
            // see an absolute path.
            CliSource::File(p) => (
                p.canonicalize()
                    .map_err(|e| CliError::OpenFailed(format!("{}: {e}", p.display())))?,
                false,
            ),
            CliSource::Bytes(bytes) => (write_temp_pdf(bytes)?, true),
        };
        let doc = (|| {
            let info = read_info(&path, backend)?;
            // `page_count` comes from parsing tool output over
            // attacker-controlled input — bound it. The lazy HashMap
            // cache makes a huge count non-allocating, but an absurd
            // number is almost certainly a corrupt/hostile file.
            if info.page_count == 0 || info.page_count > MAX_PAGES {
                return Err(CliError::OpenFailed(format!(
                    "implausible page count {}",
                    info.page_count
                )));
            }
            Ok(SubprocessDocument {
                path: path.clone(),
                temp_owned,
                backend,
                sizes: Mutex::new(HashMap::new()),
                info,
            })
        })();
        // A failed open must not leak the temp file — Drop only runs
        // once the document exists.
        if doc.is_err() && temp_owned {
            let _ = std::fs::remove_file(&path);
        }
        doc
    }

    /// The backend serving this document.
    ///
    /// ```no_run
    /// use martensite_pdf_platform::{open_document, CliSource};
    /// use std::path::PathBuf;
    ///
    /// # fn ex() -> Result<(), martensite_pdf_platform::CliError> {
    /// let doc = open_document(&CliSource::File(PathBuf::from("a.pdf")))?;
    /// assert!(matches!(doc.backend().name(), "poppler" | "mupdf"));
    /// # Ok(())
    /// # }
    /// ```
    pub fn backend(&self) -> PdfBackend {
        self.backend
    }

    /// Document metadata.
    ///
    /// ```no_run
    /// use martensite_pdf_platform::{open_document, CliSource};
    /// use std::path::PathBuf;
    ///
    /// # fn ex() -> Result<(), martensite_pdf_platform::CliError> {
    /// let doc = open_document(&CliSource::File(PathBuf::from("a.pdf")))?;
    /// assert!(doc.info().page_count > 0);
    /// # Ok(())
    /// # }
    /// ```
    pub fn info(&self) -> CliDocInfo {
        self.info.clone()
    }

    /// The page size of `page` (0-based) — the crop box the CLIs
    /// render (the `Page size` line / the 72-dpi probe), `None`
    /// when out of range or the probe fails.
    ///
    /// ```no_run
    /// use martensite_pdf_platform::{open_document, CliSource};
    /// use std::path::PathBuf;
    ///
    /// # fn ex() -> Result<(), martensite_pdf_platform::CliError> {
    /// let doc = open_document(&CliSource::File(PathBuf::from("a.pdf")))?;
    /// assert!(doc.page_size(0).is_some());
    /// # Ok(())
    /// # }
    /// ```
    pub fn page_size(&self, page: u32) -> Option<CliPageSize> {
        if page >= self.info.page_count {
            return None;
        }
        if let Some(&cached) = self.sizes.lock().ok()?.get(&page) {
            return cached;
        }
        let size = read_page_size(&self.path, self.backend, page);
        if let Ok(mut sizes) = self.sizes.lock() {
            sizes.insert(page, size); // failed probes cached too
        }
        size
    }

    /// Rasterize `page` (0-based) so its longer edge is at most
    /// `max_px` pixels, preserving aspect. `None` when the page is out
    /// of range or the rasterizer fails.
    ///
    /// ```no_run
    /// use martensite_pdf_platform::{open_document, CliSource};
    /// use std::path::PathBuf;
    ///
    /// # fn ex() -> Result<(), martensite_pdf_platform::CliError> {
    /// let doc = open_document(&CliSource::File(PathBuf::from("a.pdf")))?;
    /// let bmp = doc.render_page(0, 512).unwrap();
    /// assert_eq!(bmp.pixels.len(), (bmp.width * bmp.height * 4) as usize);
    /// # Ok(())
    /// # }
    /// ```
    pub fn render_page(&self, page: u32, max_px: u32) -> Option<CliBitmap> {
        if max_px == 0 {
            return None;
        }
        let size = self.page_size(page)?;
        let longest = size.width.max(size.height).max(1.0);
        // PDF points are 1/72 inch; choose DPI so the long edge lands
        // at `max_px`. The floor is 1 dpi — a hard floor higher would
        // break the "at most max_px" contract for degenerate pages.
        let dpi = (max_px as f32 * 72.0 / longest).clamp(1.0, 600.0);
        let ppm = render_ppm(&self.path, self.backend, page, dpi)?;
        let (w, h, pixels) = decode_ppm(&ppm)?;
        Some(CliBitmap {
            width: w,
            height: h,
            pixels,
        })
    }
}

/// `true` when `prog` spawns (found on `$PATH`); output discarded.
/// Bounded like `run` — a hung `$PATH` impostor must not stall the
/// backend probe forever.
fn have(prog: &str) -> bool {
    let Ok(mut child) = Command::new(prog)
        .arg("-v")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    else {
        return false;
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        match child.try_wait() {
            Ok(Some(_)) | Err(_) => return true,
            Ok(None) if std::time::Instant::now() > deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return true; // spawned — a hung impostor still "exists"
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(10)),
        }
    }
}

/// Write `bytes` to a fresh temp PDF and return its path.
///
/// `create_new` + retry guards the predictable-name race: a
/// pre-planted file (or symlink) at the candidate path makes the
/// create fail rather than following the link and overwriting it.
/// Unix perms are `0o600` — the temp PDF isn't world-readable.
fn write_temp_pdf(bytes: &[u8]) -> Result<PathBuf, CliError> {
    use std::io::Write;
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    for _ in 0..32 {
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("martensite-pdf-{}-{n}.pdf", std::process::id()));
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        match opts.open(&path) {
            Ok(mut f) => {
                return match f.write_all(bytes) {
                    Ok(()) => Ok(path),
                    Err(e) => {
                        // The created file would otherwise linger —
                        // nothing owns it until `open` succeeds.
                        let _ = std::fs::remove_file(&path);
                        Err(CliError::OpenFailed(format!("temp write: {e}")))
                    }
                };
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(CliError::OpenFailed(format!("temp create: {e}"))),
        }
    }
    Err(CliError::OpenFailed(
        "temp create: exhausted path candidates".to_string(),
    ))
}

/// Claim a fresh, private output file for a rasterizer subprocess.
///
/// Created with `create_new` + `0o600` *before* the tool runs: a
/// pre-planted symlink at a predictable path can't make the tool
/// follow it and overwrite an unrelated file, and the tool happily
/// truncates our empty placeholder. Retried on `AlreadyExists`.
fn claim_out_path(ext: &str) -> Option<PathBuf> {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    for _ in 0..32 {
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "martensite-pdf-out-{}-{n}.{ext}",
            std::process::id()
        ));
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        match opts.open(&path) {
            Ok(_) => return Some(path),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return None,
        }
    }
    None
}

/// Run `prog` with `args`, returning stdout on success. The child is
/// killed when it exceeds `timeout_secs` — a pathological PDF must
/// not hang the paint path forever. stdout is drained on a reader
/// thread so a >64KiB info dump can't deadlock the poll loop.
fn run(prog: &str, args: &[&str], timeout_secs: u64) -> Option<Vec<u8>> {
    use std::io::Read;
    let mut child = Command::new(prog)
        .args(args)
        // None of the tools read stdin, but a hung `$PATH` impostor
        // could block on the tty — never inherit it.
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    // `take` can only fail if stdout wasn't piped — but without it
    // the child is unreachable, so kill+reap rather than leak it.
    let Some(stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return None;
    };
    let reader = std::thread::spawn(move || {
        let mut v = Vec::new();
        // `mutool info` dumps per-page resources — a hostile doc
        // could stream unbounded output. Cap what we buffer.
        let _ = stdout.take(16 * 1024 * 1024).read_to_end(&mut v);
        v
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let out = reader.join().unwrap_or_default();
                return status.success().then_some(out);
            }
            Ok(None) => {
                if std::time::Instant::now() > deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = reader.join(); // pipe closed at kill — drains fast
                    return None;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = reader.join();
                return None;
            }
        }
    }
}

/// Parse document metadata + page count.
fn read_info(path: &Path, backend: PdfBackend) -> Result<CliDocInfo, CliError> {
    let file = path.to_string_lossy();
    let prog_args: &[&str] = match backend {
        PdfBackend::Poppler => &[file.as_ref()],
        PdfBackend::Mupdf => &["info", file.as_ref()],
    };
    let prog = match backend {
        PdfBackend::Poppler => "pdfinfo",
        PdfBackend::Mupdf => "mutool",
    };
    let out = run(prog, prog_args, 60)
        .ok_or_else(|| CliError::OpenFailed(format!("info tool failed on {}", path.display())))?;
    let text = String::from_utf8_lossy(&out);
    let mut info = parse_info(&text);
    if backend == PdfBackend::Mupdf {
        // `mutool info` dumps the Info dict as `<< /Title (…) >>` —
        // no `Title:`/`Author:` lines. Best-effort paren extraction.
        if info.title.is_none() {
            info.title = dict_string(&text, "Title");
        }
        if info.author.is_none() {
            info.author = dict_string(&text, "Author");
        }
    }
    Ok(info)
}

/// Extract `/Name (value)` or `/Name <hex>` from a PDF dict dump
/// (mupdf `info` prints `<< /Title (…) /Author (…) >>`).
///
/// Best-effort: a `/Name` appearing *inside* another string's value
/// (e.g. `/Subject (see /Title (x))`) can still match — real dumps
/// place keys at token boundaries, and a wrong hit yields metadata
/// noise, not memory unsafety.
fn dict_string(text: &str, name: &str) -> Option<String> {
    let key = format!("/{name}");
    // Scan every `/Name` whose next byte can start a value —
    // `/TitlePage`-style prefixes are rejected, and a candidate
    // whose value turns out not to be a string doesn't stop the
    // search (a later real `/Name` may still exist).
    for (i, _) in text.match_indices(&key) {
        let after = text.as_bytes().get(i + key.len()).copied()?;
        if !(after == b'(' || after == b'<' || after.is_ascii_whitespace()) {
            continue;
        }
        if let Some(v) = dict_value(text[i + key.len()..].trim_start()) {
            return Some(v);
        }
    }
    None
}

/// Parse the `(parens)` or `<hex>` string starting at `rest`.
fn dict_value(rest: &str) -> Option<String> {
    if let Some(inner) = rest.strip_prefix('(') {
        // PDF parens strings balance nested `()` and escape `\(`,
        // `\)` — a naive `find(')')` truncates both. Scan bytes
        // tracking depth, skipping escaped bytes.
        let bytes = inner.as_bytes();
        let mut depth = 1usize;
        let mut i = 0;
        let end = loop {
            match *bytes.get(i)? {
                b'\\' => i += 2,
                b'(' => {
                    depth += 1;
                    i += 1;
                }
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        break i;
                    }
                    i += 1;
                }
                _ => i += 1,
            }
        };
        // Unescape the standard PDF escapes on the extracted slice:
        // `\n \r \t \b \f \( \) \\`, `\ddd` (1–3 octal digits), and
        // `\<EOL>` line continuation (emits nothing).
        let raw = inner[..end].trim();
        let mut v = String::with_capacity(raw.len());
        let mut chars = raw.chars().peekable();
        while let Some(c) = chars.next() {
            if c != '\\' {
                v.push(c);
                continue;
            }
            match chars.next() {
                Some('n') => v.push('\n'),
                Some('r') => v.push('\r'),
                Some('t') => v.push('\t'),
                Some('b') => v.push('\u{8}'),
                Some('f') => v.push('\u{c}'),
                Some('\n') => {} // line continuation
                Some('\r') => {
                    // \r or \r\n continuation
                    if chars.peek() == Some(&'\n') {
                        chars.next();
                    }
                }
                Some(d @ '0'..='7') => {
                    let mut oct = d.to_digit(8).unwrap_or(0);
                    for _ in 0..2 {
                        match chars.peek().copied() {
                            Some(d2 @ '0'..='7') => {
                                chars.next();
                                oct = oct * 8 + d2.to_digit(8).unwrap_or(0);
                            }
                            _ => break,
                        }
                    }
                    v.push(char::from_u32(oct).unwrap_or('\u{fffd}'));
                }
                Some(other) => v.push(other), // \( \) \\ etc.
                None => v.push('\\'),
            }
        }
        return (!v.is_empty()).then_some(v);
    }
    // `<HEX>` strings — decode best-effort as UTF-16BE nibbles.
    if let Some(inner) = rest.strip_prefix('<') {
        let end = inner.find('>')?;
        // `.get` slicing: arbitrary UTF-8 inside `<…>` must not
        // panic on a non-char-boundary — bad pairs just drop out.
        let hex: String = inner[..end]
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        let bytes: Vec<u8> = (0..hex.len() / 2)
            .filter_map(|i| {
                hex.get(i * 2..i * 2 + 2)
                    .and_then(|s| u8::from_str_radix(s, 16).ok())
            })
            .collect();
        // Skip a UTF-16BE BOM when present.
        let s = if bytes.starts_with(&[0xFE, 0xFF]) {
            String::from_utf16_lossy(
                &bytes[2..]
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|c| u16::from_be_bytes(*c))
                    .collect::<Vec<_>>(),
            )
        } else {
            String::from_utf8_lossy(&bytes).into_owned()
        };
        let v = s.trim();
        return (!v.is_empty()).then(|| v.to_string());
    }
    None
}

/// Extract `Title`/`Author`/`Pages` from `pdfinfo` or `mutool info`
/// text — both emit `Key: value` lines.
fn parse_info(text: &str) -> CliDocInfo {
    let mut info = CliDocInfo::default();
    for line in text.lines() {
        let line = line.trim();
        if let Some(v) = line.strip_prefix("Title:") {
            let v = v.trim();
            if !v.is_empty() {
                info.title = Some(v.to_string());
            }
        } else if let Some(v) = line.strip_prefix("Author:") {
            let v = v.trim();
            if !v.is_empty() {
                info.author = Some(v.to_string());
            }
        } else if let Some(v) = line.strip_prefix("Pages:") {
            // First `Pages:` wins — a hostile metadata value can
            // inject a fake `Pages: N` line (metadata may contain
            // newlines) to spoof the count.
            if info.page_count == 0 {
                info.page_count = v.trim().parse().unwrap_or(0);
            }
        }
    }
    info
}

/// Read the size of `page` (0-based).
fn read_page_size(path: &Path, backend: PdfBackend, page: u32) -> Option<CliPageSize> {
    let file = path.to_string_lossy();
    match backend {
        // `pdfinfo -f N -l N` prints "Page    N size: W x H pts".
        PdfBackend::Poppler => {
            let n = (page + 1).to_string();
            let out = run("pdfinfo", &["-f", &n, "-l", &n, file.as_ref()], 60)?;
            parse_pdfinfo_size(&String::from_utf8_lossy(&out))
        }
        // `mutool info` emits a *deduplicated* `Mediaboxes` list that
        // cannot be mapped back to individual pages, so the mupdf
        // size probe renders the page at exactly 72 dpi — where px
        // == pt — and reads just the PPM header. A hostile MediaBox
        // can rasterize to hundreds of MB; never read it all for two
        // integers. Cached per page.
        //
        // KNOWN COST: every first `page_size` call per page pays a
        // full-page rasterization (bounded by the 120s timeout) just
        // to read ~30 bytes of header. The cheaper probe is
        // `mutool pages`, which prints per-page boxes as metadata
        // with no rasterization — but its output format varies
        // across mutool versions and was not verifiable in the dev
        // environment. The render probe was kept because it is
        // *verified* correct (`.ppm` → P6 suffix mapping, 1-based
        // pages, px == pt at 72 dpi) and measures the crop box the
        // rasterizer will actually draw — writing a parser for
        // unseen `mutool pages` output would repeat the fictional-
        // parser bug the deduplicated-Mediaboxes approach had.
        // Revisit once `mutool pages` output is verified on a box
        // with mupdf-tools installed.
        PdfBackend::Mupdf => {
            let ppm = render_ppm_to(path, backend, page, 72.0)?;
            let head = read_head(&ppm, 128);
            let _ = std::fs::remove_file(&ppm);
            let (w, h) = head.and_then(|h| decode_ppm_header(&h))?;
            // Sanity bound on the decoded dims — a corrupt MediaBox
            // must not yield absurd point sizes.
            (w > 0 && h > 0 && w <= 100_000 && h <= 100_000).then_some(CliPageSize {
                width: w as f32,
                height: h as f32,
            })
        }
    }
}

/// Read at most `n` bytes of `path` — enough for a P6-PPM header.
fn read_head(path: &Path, n: usize) -> Option<Vec<u8>> {
    use std::io::Read;
    let mut buf = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(n as u64)
        .read_to_end(&mut buf)
        .ok()?;
    Some(buf)
}

/// `Page    1 size: 612 x 792 pts (letter)` → `CliPageSize`.
fn parse_pdfinfo_size(text: &str) -> Option<CliPageSize> {
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with("Page") && line.contains("size:") {
            let after = line.split("size:").nth(1)?;
            let mut it = after.split_whitespace();
            let width: f32 = it.next()?.parse().ok()?;
            it.next()?; // "x"
            let height: f32 = it.next()?.parse().ok()?;
            // Reject 0/negative/NaN/inf/astronomical values — the
            // text comes from parsing a hostile document.
            let sane = |v: f32| v.is_finite() && v > 0.0 && v <= 1_000_000.0;
            return (sane(width) && sane(height)).then_some(CliPageSize { width, height });
        }
    }
    None
}

/// Rasterize `page` (0-based) to a fresh private PPM file and
/// return its path when the rasterizer exited successfully. The
/// caller removes the file. A failed run still removes the partial
/// output before returning `None`.
fn render_ppm_to(path: &Path, backend: PdfBackend, page: u32, dpi: f32) -> Option<PathBuf> {
    let file = path.to_string_lossy();
    let n = (page + 1).to_string();
    // Floor, not round — `{:.0}` of 1.5 → "2" would render ~33%
    // over the "at most max_px" contract.
    let d = format!("{:.0}", dpi.floor().max(1.0));
    let out = claim_out_path("ppm")?;
    // Resolve string args *before* spawning — a `to_str` failure must
    // still remove the claimed file, not `?`-leak it.
    let out_s = out.to_str();
    let prefix_s = out.with_extension("");
    let (out_s, prefix_s) = match (out_s, prefix_s.to_str()) {
        (Some(o), Some(p)) => (o.to_string(), p.to_string()),
        _ => {
            let _ = std::fs::remove_file(&out);
            return None;
        }
    };
    let ok = match backend {
        PdfBackend::Poppler => {
            // `-singlefile` → writes literally "<prefix>.ppm". PPM
            // is pdftoppm's *default* format — there is no `-ppm`
            // flag; passing one makes it exit with usage.
            run(
                "pdftoppm",
                &[
                    "-f",
                    &n,
                    "-l",
                    &n,
                    "-r",
                    &d,
                    "-singlefile",
                    file.as_ref(),
                    &prefix_s,
                ],
                120,
            )
            .is_some()
        }
        PdfBackend::Mupdf => run(
            "mutool",
            &["draw", "-r", &d, "-o", &out_s, file.as_ref(), &n],
            120,
        )
        .is_some(),
    };
    if ok {
        Some(out)
    } else {
        let _ = std::fs::remove_file(&out);
        None
    }
}

/// Rasterize `page` (0-based) to PPM bytes. The output file is
/// removed whether or not the rasterizer succeeded — a failed run
/// can still leave a partial file behind.
fn render_ppm(path: &Path, backend: PdfBackend, page: u32, dpi: f32) -> Option<Vec<u8>> {
    let out = render_ppm_to(path, backend, page, dpi)?;
    let bytes = std::fs::read(&out).ok();
    let _ = std::fs::remove_file(&out);
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pdfinfo() {
        let text = "Title:        Spec Sheet\nAuthor:       Eng\nPages:        12\n";
        let info = parse_info(text);
        assert_eq!(info.title.as_deref(), Some("Spec Sheet"));
        assert_eq!(info.author.as_deref(), Some("Eng"));
        assert_eq!(info.page_count, 12);
    }

    #[test]
    fn parses_pdfinfo_size() {
        let text = "Page    3 size: 595.28 x 841.89 pts (A4)\n";
        let s = parse_pdfinfo_size(text).unwrap();
        assert!((s.width - 595.28).abs() < 0.01);
        assert!((s.height - 841.89).abs() < 0.01);
    }

    #[test]
    fn parses_mupdf_dict_metadata() {
        // Real `mutool info` shape: `<< /Title (…) /Author (…) >>`.
        let text = "Info object (12 0 R):\n<</Title(Spec Sheet)/Author(Eng)>>\nPages: 2\n";
        assert_eq!(dict_string(text, "Title").as_deref(), Some("Spec Sheet"));
        assert_eq!(dict_string(text, "Author").as_deref(), Some("Eng"));
        assert_eq!(dict_string(text, "Creator"), None);
        // `/TitlePage` must not match the `/Title` key.
        let text = "<</TitlePage(3)/Producer(X)>>";
        assert_eq!(dict_string(text, "Title"), None);
    }

    #[test]
    fn dict_string_handles_nesting_and_escapes() {
        // Balanced nested parens and escaped parens are legal inside
        // a PDF string — a naive `find(')')` truncates both.
        let text = "<</Title (Report \\(final\\))>>";
        assert_eq!(
            dict_string(text, "Title").as_deref(),
            Some("Report (final)")
        );
        let text = "<</Title (a(b)c)>>";
        assert_eq!(dict_string(text, "Title").as_deref(), Some("a(b)c"));
        // Octal escapes and `\<EOL>` continuations decode per spec.
        let text = "<</Title (a\\101b)>>"; // \101 = 'A'
        assert_eq!(dict_string(text, "Title").as_deref(), Some("aAb"));
        let text = "<</Title (a\\\nb)>>"; // line continuation
        assert_eq!(dict_string(text, "Title").as_deref(), Some("ab"));
    }

    #[test]
    fn open_missing_file_errors() {
        if probe_backend().is_none() {
            return; // no rasterizer installed — nothing to test
        }
        let err = open_document(&CliSource::File(PathBuf::from("/nonexistent.pdf")));
        assert!(matches!(err, Err(CliError::OpenFailed(_))));
    }
}
