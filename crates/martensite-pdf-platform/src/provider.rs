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
    /// `mutool show -g <file> pages grep` whole-document page table,
    /// probed at most once on first `page_size` (mupdf backend
    /// only). The outer `Option` caches a failed probe too — a
    /// broken `mutool` doesn't re-spawn per query; per-page `None`
    /// entries fall back to the render probe.
    mupdf_meta: OnceLock<Option<Vec<Option<MutoolPage>>>>,
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
                mupdf_meta: OnceLock::new(),
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
    /// render (poppler's `Page size` + `rot:` lines; mupdf's
    /// `mutool show` object-table metadata, 72-dpi render probe as
    /// fallback), `None` when out of range or the probe fails.
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
        let size = match self.backend {
            // Metadata-first: one `mutool show` subprocess resolves
            // every page's boxes (incl. inherited attrs) in the
            // document. Pages the metadata can't cover (no MediaBox
            // anywhere in the parent chain, missing object) fall
            // back to the 72-dpi render probe. A page whose metadata
            // resolves to an *insane* size does NOT probe — a >100k-pt
            // page would rasterize tens of GB for nothing.
            PdfBackend::Mupdf => match self.mupdf_page_size(page) {
                Ok(size) => size,
                Err(()) => read_page_size(&self.path, self.backend, page),
            },
            PdfBackend::Poppler => read_page_size(&self.path, self.backend, page),
        };
        if let Ok(mut sizes) = self.sizes.lock() {
            sizes.insert(page, size); // failed probes cached too
        }
        size
    }

    /// `page` (0-based) sized from the `mutool show` object table:
    /// effective crop box (`CropBox ∩ MediaBox`, `MediaBox` when
    /// absent — both resolved up the `/Parent` chain, matching
    /// `pdf_lookup_inherited_page_item`) scaled by the page's own
    /// `UserUnit` and rotated, matching what `mutool draw`
    /// rasterizes (verified end-to-end on mupdf-tools 1.28.4).
    ///
    /// `Ok(size)` is authoritative — `Ok(None)` means the metadata
    /// resolved but produced an insane size (do NOT probe: the
    /// fallback would rasterize an absurd page). `Err(())` means the
    /// metadata can't cover this page (subprocess failed, page
    /// object unlisted, or no `MediaBox` in its whole ancestor
    /// chain) — the render probe may still resolve it.
    fn mupdf_page_size(&self, page: u32) -> Result<Option<CliPageSize>, ()> {
        let pages = self
            .mupdf_meta
            .get_or_init(|| {
                let file = self.path.to_string_lossy();
                run(
                    "mutool",
                    &["show", "-g", file.as_ref(), "pages", "grep"],
                    60,
                )
                .map(|out| parse_mutool_show(&String::from_utf8_lossy(&out)))
            })
            .as_ref()
            .ok_or(())?;
        match pages.get(page as usize) {
            Some(Some(p)) => p.verdict(),
            _ => Err(()),
        }
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
            // LAST `Pages:` wins — a hostile Title/metadata string
            // can inject a fake `Pages: N` line (parens strings may
            // contain newlines), but every attacker-controlled field
            // prints BEFORE the real `Pages:` line in both pdfinfo
            // and `mutool info` output.
            info.page_count = v.trim().parse().unwrap_or(0);
        }
    }
    info
}

/// Read the size of `page` (0-based).
fn read_page_size(path: &Path, backend: PdfBackend, page: u32) -> Option<CliPageSize> {
    let file = path.to_string_lossy();
    match backend {
        // `pdfinfo -f N -l N` prints "Page    N size: W x H pts"
        // (the effective CropBox, *unrotated*) plus a separate
        // "Page    N rot:   R" line — `parse_pdfinfo_size` applies
        // the rotation so this returns what `-cropbox` renders.
        //
        // SEMANTIC NOTE — poppler 26.09 IGNORES `/UserUnit`
        // entirely (page-local and inherited alike, in both pdfinfo
        // and pdftoppm — verified empirically), while mupdf scales
        // by page-local UserUnit. The backends therefore disagree
        // on `/UserUnit` pages; each stays internally consistent —
        // normalizing would mean *removing* scaling mupdf applies.
        PdfBackend::Poppler => {
            let n = (page + 1).to_string();
            let out = run("pdfinfo", &["-f", &n, "-l", &n, file.as_ref()], 60)?;
            parse_pdfinfo_size(&String::from_utf8_lossy(&out))
        }
        // FALLBACK probe — `page_size` prefers `mutool show`
        // object-table metadata (see `mupdf_page_size`), which
        // needs no rasterization at all. This render probe remains
        // for pages the object table can't size (no MediaBox in the
        // ancestor chain, degenerate dims mupdf clamps, missing
        // object): it renders at exactly 72 dpi — where px == pt —
        // and reads just the PPM header. A hostile MediaBox can
        // rasterize to hundreds of MB; never read it all for two
        // integers. (`mutool info` emits a *deduplicated*
        // `Mediaboxes` list that cannot be mapped back to
        // individual pages, which is why it was never used here.)
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

/// One page's box attributes, resolved through `/Parent`-chain
/// inheritance from the `mutool show` object table.
#[derive(Clone, Copy, Debug, PartialEq)]
struct MutoolPage {
    /// `[l, b, r, t]` — the effective `MediaBox` (page or nearest
    /// ancestor). `None` when the whole ancestor chain lacks one —
    /// that page falls back to the render probe.
    media: Option<[f32; 4]>,
    /// `[l, b, r, t]` — effective `CropBox`; defaults to media.
    crop: Option<[f32; 4]>,
    /// Effective `/Rotate` (page or ancestor), raw degrees.
    rotate: f32,
    /// The page's OWN `/UserUnit` — deliberately NOT inherited:
    /// `mutool draw` applies a per-page `UserUnit` but ignores an
    /// inherited one (verified on 1.28.4: a `/Pages`-level
    /// `UserUnit 2` renders unscaled while CropBox/Rotate on the
    /// same node DO apply).
    user_unit: f32,
}

impl MutoolPage {
    /// What `mutool draw` would rasterize, as a verdict:
    ///
    /// - `Ok(Some)` — the resolved size: effective crop box
    ///   (`CropBox ∩ MediaBox`, `MediaBox` when no CropBox) scaled
    ///   by `UserUnit`, dims swapped when the snapped rotation is
    ///   90/270.
    /// - `Ok(None)` — metadata resolved but produced an absurd size
    ///   (>100k pt). Do NOT probe: the fallback would rasterize
    ///   tens of GB for a header.
    /// - `Err(())` — metadata can't decide: no `MediaBox` anywhere
    ///   in the ancestor chain, or degenerate dims (sub-1-pt boxes
    ///   mupdf clamps to a 1×1 unit rect, empty/negative
    ///   intersections, NaN/inf). The render probe reports whatever
    ///   `mutool draw` actually produces for these.
    fn verdict(&self) -> Result<Option<CliPageSize>, ()> {
        let m = self.media.ok_or(())?;
        let (l, b, r, t) = match self.crop {
            Some(c) => (
                m[0].max(c[0]),
                m[1].max(c[1]),
                m[2].min(c[2]),
                m[3].min(c[3]),
            ),
            None => (m[0], m[1], m[2], m[3]),
        };
        let (mut w, mut h) = ((r - l) * self.user_unit, (t - b) * self.user_unit);
        // mupdf quantizes /Rotate via `pdf_to_int` (floorf(x+0.5))
        // THEN snaps to the nearest multiple of 90
        // (`pdf-page.c`: `90*((rotate+45)/90)`) — a degenerate
        // `/Rotate 44.7` rounds to 45 → snaps to 90 → swapped.
        let r = (self.rotate + 0.5).floor();
        let rot = 90.0 * ((r + 45.0) / 90.0).floor().rem_euclid(4.0);
        if rot == 90.0 || rot == 270.0 {
            std::mem::swap(&mut w, &mut h);
        }
        if !w.is_finite() || !h.is_finite() || w < 1.0 || h < 1.0 {
            return Err(());
        }
        if w > 100_000.0 || h > 100_000.0 {
            return Ok(None);
        }
        Ok(Some(CliPageSize {
            width: w,
            height: h,
        }))
    }
}

/// Parse `mutool show -g <file> pages grep` output — verified
/// verbatim against mupdf-tools 1.28.4:
///
/// ```text
/// page 1 = 3 0 R
/// 1 0 obj <</Pages 2 0 R/Type/Catalog>>
/// 2 0 obj <</Type/Pages/Count 1/CropBox[0 54 540 684]/Kids[3 0 R]/Rotate 90>>
/// 3 0 obj <</MediaBox[0 0 612 792]/Parent 2 0 R/Type/Page>>
/// trailer <</Size 4/Root 1 0 R>>
/// ```
///
/// `page N = M 0 R` lines give the authoritative page order —
/// mutool resolves the page tree itself, so page indexing can't be
/// shifted by injected lines or malformed input. `N 0 obj <<…>>`
/// lines give every object's dict on one line; each page's
/// `MediaBox`/`CropBox`/`Rotate` is then resolved up its `/Parent`
/// chain, matching `pdf_lookup_inherited_page_item` (the same
/// lookup `mutool draw` uses).
///
/// One `Option<MutoolPage>` per page in document order; `None`
/// when the page's object isn't in the table.
fn parse_mutool_show(text: &str) -> Vec<Option<MutoolPage>> {
    let mut dicts: HashMap<u32, &str> = HashMap::new();
    let mut refs: Vec<(u32, u32)> = Vec::new(); // (pagenum, objnum)
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("page ") {
            // `page N = M 0 R`
            let mut it = rest.split_whitespace();
            let Some(Ok(n)) = it.next().map(|t| t.parse::<u32>()) else {
                continue;
            };
            if it.next() != Some("=") {
                continue;
            }
            let Some(Ok(m)) = it.next().map(|t| t.parse::<u32>()) else {
                continue;
            };
            refs.push((n, m));
        } else if let Some((n, body)) = parse_obj_line(line) {
            dicts.insert(n, body);
        }
    }
    // Index by the printed page number — position is not trusted.
    let max = refs.iter().map(|&(n, _)| n).max().unwrap_or(0);
    if max > MAX_PAGES {
        return Vec::new(); // absurd numbering — probe everything
    }
    let mut by_num: Vec<Option<u32>> = vec![None; max as usize];
    for (n, m) in refs {
        if n >= 1 {
            by_num[n as usize - 1] = Some(m);
        }
    }
    by_num
        .iter()
        .map(|obj| {
            let dict = obj.and_then(|o| dicts.get(&o).copied())?;
            Some(MutoolPage {
                media: resolve(&dicts, dict, "MediaBox").and_then(parse_arr4),
                crop: resolve(&dicts, dict, "CropBox").and_then(parse_arr4),
                rotate: resolve(&dicts, dict, "Rotate")
                    .and_then(|v| v.trim().parse().ok())
                    .unwrap_or(0.0),
                user_unit: dict_lookup(dict, "UserUnit")
                    .and_then(|v| v.trim().parse().ok())
                    .unwrap_or(1.0),
            })
        })
        .collect()
}

/// `N 0 obj <<…>>` (a `-g` one-line object) → `(N, dict-body)`.
/// Lines without a dict (bare values, stream stubs, `trailer`)
/// return `None`.
fn parse_obj_line(line: &str) -> Option<(u32, &str)> {
    let (num, rest) = line.split_once(" obj ")?;
    // `num` is "N G" — object number + generation; only N matters.
    let n: u32 = num.split_whitespace().next()?.parse().ok()?;
    let start = rest.find("<<")?;
    // Body ends at the matching `>>`; on unbalanced input just take
    // the rest of the line — a truncated body only fails lookups.
    let end = skip_dict(rest, start).saturating_sub(2).max(start + 2);
    Some((n, &rest[start + 2..end.min(rest.len())]))
}

/// Walk a dict's `/Parent` chain for `key` — the same inherited
/// lookup `mutool draw` performs for `MediaBox`/`CropBox`/`Rotate`.
/// Depth-capped so a `/Parent` cycle bails instead of looping.
fn resolve<'a>(dicts: &HashMap<u32, &'a str>, start: &'a str, key: &str) -> Option<&'a str> {
    let mut cur = start;
    for _ in 0..64 {
        if let Some(v) = dict_lookup(cur, key) {
            return Some(v);
        }
        cur = *dicts.get(&dict_ref(cur, "Parent")?)?;
    }
    None
}

/// Extract `key`'s TOP-LEVEL value from a `-g` dict body, skipping
/// nested `<<…>>` dicts, `[…]` arrays, and `(…)` strings wholesale
/// so a `/Key` inside them can't false-match. Returns the value
/// text verbatim (`[0 0 612 792]`, `2`, `3 0 R`, `<<…>>`…).
fn dict_lookup<'a>(body: &'a str, key: &str) -> Option<&'a str> {
    let pat = format!("/{key}");
    let b = body.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'<' if b.get(i + 1) == Some(&b'<') => i = skip_dict(body, i),
            b'[' => i = skip_balanced(body, i, b'[', b']'),
            b'(' => i = skip_parens(body, i),
            b'/' if body[i..].starts_with(&pat) => {
                let end = i + pat.len();
                // `/MediaBoxFoo` must not match `/MediaBox` — the
                // byte after the name must be a delimiter/EOL.
                if b.get(end).is_none_or(|&c| is_delim(c)) {
                    return value_text(body, end);
                }
                i += 1;
            }
            _ => i += 1,
        }
    }
    None
}

/// PDF delimiters — a name ends at any of these or whitespace.
fn is_delim(c: u8) -> bool {
    matches!(
        c,
        b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
    ) || c.is_ascii_whitespace()
}

/// The value text following a top-level `/Key`: dict, array,
/// string, name, or bare token — sliced verbatim, balanced.
fn value_text(body: &str, key_end: usize) -> Option<&str> {
    let b = body.as_bytes();
    let mut i = key_end;
    while b.get(i).is_some_and(|c| c.is_ascii_whitespace()) {
        i += 1;
    }
    let start = i;
    let end = match *b.get(i)? {
        b'<' if b.get(i + 1) == Some(&b'<') => skip_dict(body, i),
        b'[' => skip_balanced(body, i, b'[', b']'),
        b'(' => skip_parens(body, i),
        _ => {
            // Name, number, ref's first number, keyword — a bare
            // token up to the next delimiter/whitespace. (For
            // `N 0 R` refs only `N` is captured — all we need.)
            let mut j = i;
            while b.get(j).is_some_and(|&c| !is_delim(c)) {
                j += 1;
            }
            j
        }
    };
    (end > start).then_some(&body[start..end])
}

/// `[n n n n]` → `[f32; 4]` — first four numbers, extras ignored.
fn parse_arr4(value: &str) -> Option<[f32; 4]> {
    let inner = value.strip_prefix('[')?.strip_suffix(']')?;
    let mut it = inner.split_whitespace().map(|t| t.parse::<f32>());
    Some([
        it.next()?.ok()?,
        it.next()?.ok()?,
        it.next()?.ok()?,
        it.next()?.ok()?,
    ])
}

/// `key`'s object reference (`N 0 R`) → `N`. Generation ignored.
fn dict_ref(body: &str, key: &str) -> Option<u32> {
    dict_lookup(body, key)?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

/// Index past a balanced `<<…>>` starting at `start` (which points
/// at `<<`). Nested dicts and `(…)` strings are handled; unbalanced
/// input consumes the rest of the string.
fn skip_dict(body: &str, start: usize) -> usize {
    let b = body.as_bytes();
    let mut i = start + 2;
    let mut depth = 1u32;
    while i < b.len() {
        match b[i] {
            b'<' if b.get(i + 1) == Some(&b'<') => {
                depth += 1;
                i += 2;
            }
            b'>' if b.get(i + 1) == Some(&b'>') => {
                depth -= 1;
                i += 2;
                if depth == 0 {
                    return i;
                }
            }
            b'(' => i = skip_parens(body, i),
            _ => i += 1,
        }
    }
    b.len()
}

/// Index past a balanced `open…close` span (arrays). Nested dicts
/// and strings are skipped so their contents can't close the span.
fn skip_balanced(body: &str, start: usize, open: u8, close: u8) -> usize {
    let b = body.as_bytes();
    let mut i = start + 1;
    let mut depth = 1u32;
    while i < b.len() {
        match b[i] {
            b'<' if b.get(i + 1) == Some(&b'<') => i = skip_dict(body, i),
            b'(' => i = skip_parens(body, i),
            c if c == open => {
                depth += 1;
                i += 1;
            }
            c if c == close => {
                depth -= 1;
                i += 1;
                if depth == 0 {
                    return i;
                }
            }
            _ => i += 1,
        }
    }
    b.len()
}

/// Index past a balanced `(…)` PDF string starting at `start`,
/// honoring `\` escapes and nested parens.
fn skip_parens(body: &str, start: usize) -> usize {
    let b = body.as_bytes();
    let mut i = start + 1;
    let mut depth = 1u32;
    while i < b.len() {
        match b[i] {
            b'\\' => i += 2,
            b'(' => {
                depth += 1;
                i += 1;
            }
            b')' => {
                depth -= 1;
                i += 1;
                if depth == 0 {
                    return i;
                }
            }
            _ => i += 1,
        }
    }
    b.len()
}

/// `Page    1 size: 612 x 792 pts (letter)` + optional
/// `Page    1 rot:  90` → `CliPageSize`.
///
/// `pdfinfo` reports raw crop-box dims UNROTATED while `pdftoppm`
/// renders the rotation — the `rot:` line is applied so both tools
/// (and the mupdf backend) agree. Defenses against injected lines
/// (a hostile `/Title` can smuggle `Page … size:`/`rot:` text into
/// the metadata block): the real per-page lines always print AFTER
/// all metadata, so the last valid `size:` wins and `rot:` is only
/// accepted once a real `size:` line has been seen.
fn parse_pdfinfo_size(text: &str) -> Option<CliPageSize> {
    let mut size = None;
    let mut rot = 0.0f32;
    for line in text.lines() {
        let line = line.trim();
        if !line.starts_with("Page") {
            continue;
        }
        if let Some(after) = line.split("size:").nth(1) {
            let mut it = after.split_whitespace();
            let width = it.next().and_then(|t| t.parse::<f32>().ok());
            let height = it.nth(1).and_then(|t| t.parse::<f32>().ok());
            if let (Some(w), Some(h)) = (width, height) {
                let sane = |v: f32| v.is_finite() && v > 0.0 && v <= 100_000.0;
                if sane(w) && sane(h) {
                    size = Some((w, h));
                }
            }
        } else if size.is_some() {
            if let Some(after) = line.split("rot:").nth(1) {
                if let Some(r) = after
                    .split_whitespace()
                    .next()
                    .and_then(|t| t.parse::<f32>().ok())
                {
                    rot = r;
                }
            }
        }
    }
    let (mut w, mut h) = size?;
    // pdfinfo prints the raw /Rotate; apply the same snap mupdf
    // uses before rasterizing (pdf_to_int then nearest 90).
    let r = (rot + 0.5).floor();
    let snapped = 90.0 * ((r + 45.0) / 90.0).floor().rem_euclid(4.0);
    if snapped == 90.0 || snapped == 270.0 {
        std::mem::swap(&mut w, &mut h);
    }
    Some(CliPageSize {
        width: w,
        height: h,
    })
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
            // `-cropbox` is REQUIRED: pdftoppm rasterizes the
            // MediaBox by default, but `page_size` reports the
            // CropBox — without it a cropped page renders the wrong
            // region at the wrong dims (verified on poppler 26.09:
            // MediaBox 612x792 + CropBox 540x684 + /Rotate 90
            // produced 792x612 instead of 684x540).
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
                    "-cropbox",
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

    /// Verbatim `mutool show -g file pages grep` stdout
    /// (mupdf-tools 1.28.4) on a 3-page PDF, MediaBoxes
    /// 612x792 / 595x842 / 420x595.
    const SHOW_VERBATIM: &str = "\
page 1 = 3 0 R
page 2 = 4 0 R
page 3 = 5 0 R
1 0 obj <</Pages 2 0 R/Type/Catalog>>
2 0 obj <</Count 3/Kids[3 0 R 4 0 R 5 0 R]/Type/Pages>>
3 0 obj <</Contents 6 0 R/MediaBox[0 0 612 792]/Parent 2 0 R/Resources<<>>/Type/Page>>
4 0 obj <</Contents 7 0 R/MediaBox[0 0 595 842]/Parent 2 0 R/Resources<<>>/Type/Page>>
5 0 obj <</Contents 8 0 R/MediaBox[0 0 420 595]/Parent 2 0 R/Resources<<>>/Type/Page>>
6 0 obj <</Length 0>> stream
7 0 obj <</Length 0>> stream
8 0 obj <</Length 0>> stream
trailer <</Size 9/Root 1 0 R>>
";

    /// Size verdict for page `i` of a `parse_mutool_show` result.
    fn show_size(text: &str, i: usize) -> Result<Option<CliPageSize>, ()> {
        parse_mutool_show(text)
            .get(i)
            .copied()
            .flatten()
            .ok_or(())
            .and_then(|p| p.verdict())
    }

    #[test]
    fn parses_mutool_show_verbatim() {
        assert_eq!(
            show_size(SHOW_VERBATIM, 0),
            Ok(Some(CliPageSize {
                width: 612.0,
                height: 792.0
            }))
        );
        assert_eq!(
            show_size(SHOW_VERBATIM, 1),
            Ok(Some(CliPageSize {
                width: 595.0,
                height: 842.0
            }))
        );
        assert_eq!(
            show_size(SHOW_VERBATIM, 2),
            Ok(Some(CliPageSize {
                width: 420.0,
                height: 595.0
            }))
        );
    }

    #[test]
    fn mutool_show_resolves_inherited_attrs() {
        // Verbatim output for a PDF carrying
        // `/Rotate 90 /UserUnit 2 /CropBox [0 54 540 684]` on the
        // `/Pages` ROOT — `mutool draw` applies inherited
        // CropBox+Rotate (renders 630x540 at 72 dpi) but IGNORES
        // inherited UserUnit (would be 1260x1080 if applied).
        let text = "\
page 1 = 3 0 R
1 0 obj <</Pages 2 0 R/Type/Catalog>>
2 0 obj <</Type/Pages/Count 1/CropBox[0 54 540 684]/Kids[3 0 R]/Rotate 90/UserUnit 2>>
3 0 obj <</Contents 4 0 R/MediaBox[0 0 612 792]/Parent 2 0 R/Resources<<>>/Type/Page>>
4 0 obj <</Filter/FlateDecode/Length 8>> stream
trailer <</Size 5/Root 1 0 R>>
";
        assert_eq!(
            show_size(text, 0),
            Ok(Some(CliPageSize {
                width: 630.0,
                height: 540.0
            }))
        );
        // A per-page UserUnit DOES apply (draw renders 1224x1584).
        let text = "\
page 1 = 3 0 R
2 0 obj <</Type/Pages/Count 1/Kids[3 0 R]>>
3 0 obj <</MediaBox[0 0 612 792]/UserUnit 2/Parent 2 0 R/Type/Page>>
";
        assert_eq!(
            show_size(text, 0),
            Ok(Some(CliPageSize {
                width: 1224.0,
                height: 1584.0
            }))
        );
    }

    #[test]
    fn mutool_show_ignores_nested_dict_keys() {
        // `/Resources<<>>`-style nested dicts and array/string
        // values must not leak `/MediaBox`-lookalikes to the
        // top-level lookup.
        let text = "\
page 1 = 3 0 R
3 0 obj <</MediaBox[0 0 612 792]/Parent 2 0 R/Resources<</MediaBox[1 1 1 1]>>/X[(a/MediaBox[2 2 2 2])]/Y[/MediaBox]/Type/Page>>
";
        assert_eq!(
            show_size(text, 0),
            Ok(Some(CliPageSize {
                width: 612.0,
                height: 792.0
            }))
        );
    }

    #[test]
    fn mutool_page_verdict_math() {
        let page = |media, crop, rotate, user_unit| MutoolPage {
            media,
            crop,
            rotate,
            user_unit,
        };
        let size = |p: MutoolPage| p.verdict();
        // Crop ∩ media, rotated: 540x630 → swap → 630x540.
        assert_eq!(
            size(page(
                Some([0.0, 0.0, 612.0, 792.0]),
                Some([0.0, 54.0, 540.0, 684.0]),
                90.0,
                1.0
            )),
            Ok(Some(CliPageSize {
                width: 630.0,
                height: 540.0
            }))
        );
        // Non-integer rotate: mupdf rounds (pdf_to_int) then snaps —
        // 44.7 → 45 → 90 → swapped; -45.3 → -45 → 315 → 0 → unswapped.
        assert_eq!(
            size(page(Some([0.0, 0.0, 612.0, 792.0]), None, 44.7, 1.0)),
            Ok(Some(CliPageSize {
                width: 792.0,
                height: 612.0
            }))
        );
        assert_eq!(
            size(page(Some([0.0, 0.0, 612.0, 792.0]), None, -45.3, 1.0)),
            Ok(Some(CliPageSize {
                width: 612.0,
                height: 792.0
            }))
        );
        // No MediaBox anywhere → probe.
        assert_eq!(size(page(None, None, 0.0, 1.0)), Err(()));
        // Sub-1-pt box → mupdf clamps to a unit rect → probe.
        assert_eq!(
            size(page(Some([0.0, 0.0, 0.5, 0.5]), None, 0.0, 1.0)),
            Err(())
        );
        // Empty CropBox ∩ MediaBox → probe (draw normalizes).
        assert_eq!(
            size(page(
                Some([0.0, 0.0, 612.0, 792.0]),
                Some([900.0, 900.0, 1000.0, 1000.0]),
                0.0,
                1.0
            )),
            Err(())
        );
        // Absurd dims → resolved-but-rejected, NO probe.
        assert_eq!(
            size(page(Some([0.0, 0.0, 150_000.0, 150_000.0]), None, 0.0, 1.0)),
            Ok(None)
        );
        // Huge UserUnit can push a sane box over the bound → no probe.
        assert_eq!(
            size(page(Some([0.0, 0.0, 612.0, 792.0]), None, 0.0, 1_000.0)),
            Ok(None)
        );
        // NaN / negative / zero dims → degenerate → probe.
        for bad in [f32::NAN, -1.0, 0.0] {
            assert_eq!(
                size(page(Some([0.0, 0.0, bad, 792.0]), None, 0.0, 1.0)),
                Err(()),
                "bad={bad}"
            );
        }
    }

    #[test]
    fn mutool_show_edge_cases() {
        // Empty output → empty table → every page probes.
        assert!(parse_mutool_show("").is_empty());
        // A `page N` line whose object never appears → None entry.
        let pages = parse_mutool_show("page 1 = 3 0 R\npage 2 = 4 0 R\n");
        assert_eq!(pages, vec![None, None]);
        // Page numbers, not positions, index the table.
        let text = "\
page 2 = 4 0 R
page 1 = 3 0 R
3 0 obj <</MediaBox[0 0 100 200]/Type/Page>>
4 0 obj <</MediaBox[0 0 300 400]/Type/Page>>
";
        let pages = parse_mutool_show(text);
        assert_eq!(
            pages[0].and_then(|p| p.media),
            Some([0.0, 0.0, 100.0, 200.0])
        );
        assert_eq!(
            pages[1].and_then(|p| p.media),
            Some([0.0, 0.0, 300.0, 400.0])
        );
    }

    #[test]
    fn parse_info_pages_last_wins() {
        // An injected `Pages:` inside a metadata field prints
        // BEFORE the real one — last-wins keeps the real count.
        let text = "Title:        fake\nPages:        999999\nPages:        12\n";
        assert_eq!(parse_info(text).page_count, 12);
    }

    #[test]
    fn pdfinfo_size_applies_rot_and_resists_injection() {
        // Real pdfinfo per-page output: unrotated crop dims, then a
        // `rot:` line — `pdftoppm` renders rotated, so we swap.
        let text = "Page    1 size: 612 x 792 pts (letter)\nPage    1 rot:  90\n";
        assert_eq!(
            parse_pdfinfo_size(text),
            Some(CliPageSize {
                width: 792.0,
                height: 612.0
            })
        );
        // An injected rot line in the metadata block (before the
        // real `size:` line) must NOT apply — rot is only accepted
        // once a real size line has been seen.
        let text = "Title:  fake\nPage    1 rot:  90\nPage    1 size: 612 x 792 pts\n";
        assert_eq!(
            parse_pdfinfo_size(text),
            Some(CliPageSize {
                width: 612.0,
                height: 792.0
            })
        );
        // An injected size line before the real one loses (last
        // valid wins).
        let text = "Page    1 size: 9 x 9 pts\nPage    1 size: 612 x 792 pts\n";
        assert_eq!(
            parse_pdfinfo_size(text),
            Some(CliPageSize {
                width: 612.0,
                height: 792.0
            })
        );
    }

    /// Minimal valid PDF: catalog + page tree + `boxes.len()` empty
    /// content streams, with a correct xref table.
    fn test_pdf(boxes: &[(u32, u32)]) -> Vec<u8> {
        test_pdf_extra(&boxes.iter().map(|&(w, h)| (w, h, "")).collect::<Vec<_>>())
    }

    /// Like [`test_pdf`] but appends `extra` (e.g. `"/Rotate 90"`)
    /// to each page dict — exercises CropBox/Rotate/UserUnit paths.
    fn test_pdf_extra(boxes: &[(u32, u32, &str)]) -> Vec<u8> {
        test_pdf_full("", boxes)
    }

    /// Like [`test_pdf_extra`] but also appends `root_extra` to the
    /// `/Pages` root dict — exercises inherited page attributes.
    fn test_pdf_full(root_extra: &str, boxes: &[(u32, u32, &str)]) -> Vec<u8> {
        let n = boxes.len();
        let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", 3 + i)).collect();
        let mut objs = vec![
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            format!(
                "<< /Type /Pages /Kids [{}] /Count {n} {root_extra} >>",
                kids.join(" ")
            ),
        ];
        for (i, (w, h, extra)) in boxes.iter().enumerate() {
            objs.push(format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {w} {h}] {extra} \
                 /Contents {} 0 R /Resources << >> >>",
                3 + n + i
            ));
        }
        for _ in 0..n {
            objs.push("<< /Length 0 >>\nstream\n\nendstream".to_string());
        }
        let mut pdf = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (i, obj) in objs.iter().enumerate() {
            offsets.push(pdf.len());
            pdf.extend_from_slice(format!("{} 0 obj\n{obj}\nendobj\n", i + 1).as_bytes());
        }
        let xref = pdf.len();
        pdf.extend_from_slice(format!("xref\n0 {}\n", objs.len() + 1).as_bytes());
        pdf.extend_from_slice(b"0000000000 65535 f \n");
        for off in offsets {
            pdf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
        }
        pdf.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objs.len() + 1
            )
            .as_bytes(),
        );
        pdf
    }

    #[test]
    fn mupdf_end_to_end_page_sizes() {
        if !have("mutool") {
            return; // mupdf-tools not installed — nothing to test
        }
        let doc = SubprocessDocument::open(
            &CliSource::Bytes(test_pdf(&[(612, 792), (595, 842)])),
            PdfBackend::Mupdf,
        )
        .unwrap();
        assert_eq!(doc.info().page_count, 2);
        // Sizes come from `mutool show` metadata — no render.
        assert_eq!(
            doc.page_size(0),
            Some(CliPageSize {
                width: 612.0,
                height: 792.0
            })
        );
        assert_eq!(
            doc.page_size(1),
            Some(CliPageSize {
                width: 595.0,
                height: 842.0
            })
        );
        assert_eq!(doc.page_size(2), None);
    }

    /// Distinguishes the metadata path from the render fallback:
    /// Rotate/CropBox/UserUnit change the size `mutool draw` draws,
    /// and only the parser that handles them reports it — verified
    /// against real `mutool show`/`mutool draw` output (1.28.4).
    #[test]
    fn mupdf_end_to_end_rotated_scaled_sizes() {
        if !have("mutool") {
            return;
        }
        let doc = SubprocessDocument::open(
            &CliSource::Bytes(test_pdf_extra(&[
                (612, 792, "/CropBox [0 54 540 684] /Rotate 90"),
                (612, 792, "/UserUnit 2"),
            ])),
            PdfBackend::Mupdf,
        )
        .unwrap();
        // Page 1: crop 540×630, rotated → 630×540 (mutool draw
        // renders exactly this at 72 dpi).
        assert_eq!(
            doc.page_size(0),
            Some(CliPageSize {
                width: 630.0,
                height: 540.0
            })
        );
        // Page 2: 612×792 × UserUnit 2 → 1224×1584.
        assert_eq!(
            doc.page_size(1),
            Some(CliPageSize {
                width: 1224.0,
                height: 1584.0
            })
        );
    }

    /// Whole-document rotation/cropping lives on the `/Pages` root
    /// in the wild — `mutool draw` resolves inherited CropBox and
    /// Rotate (renders 630×540 at 72 dpi for this doc) but IGNORES
    /// an inherited UserUnit (would be 1260×1080 if applied).
    /// Verified against real `mutool draw` output (1.28.4).
    #[test]
    fn mupdf_end_to_end_inherited_sizes() {
        if !have("mutool") {
            return;
        }
        let doc = SubprocessDocument::open(
            &CliSource::Bytes(test_pdf_full(
                "/Rotate 90 /UserUnit 2 /CropBox [0 54 540 684]",
                &[(612, 792, "")],
            )),
            PdfBackend::Mupdf,
        )
        .unwrap();
        assert_eq!(
            doc.page_size(0),
            Some(CliPageSize {
                width: 630.0,
                height: 540.0
            })
        );
    }

    /// The cropbox contract e2e: `pdfinfo` reports the CropBox and
    /// `pdftoppm -cropbox` rasterizes it, so `render_page`'s bitmap
    /// dims must equal `page_size`. Without `-cropbox` pdftoppm
    /// renders the MediaBox — for this doc 792x612 while page_size
    /// says 684x540 (verified against poppler 26.09).
    #[test]
    fn poppler_end_to_end_cropped_rotated_render() {
        if !(have("pdfinfo") && have("pdftoppm")) {
            return; // poppler not installed — nothing to test
        }
        let doc = SubprocessDocument::open(
            &CliSource::Bytes(test_pdf_extra(&[(
                612,
                792,
                "/CropBox [36 36 576 720] /Rotate 90",
            )])),
            PdfBackend::Poppler,
        )
        .unwrap();
        // CropBox 540×684, rotated 90 → 684×540.
        assert_eq!(
            doc.page_size(0),
            Some(CliPageSize {
                width: 684.0,
                height: 540.0
            })
        );
        let bmp = doc.render_page(0, 684).expect("poppler renders");
        assert_eq!((bmp.width, bmp.height), (684, 540));
    }

    /// `/UserUnit` divergence documented in `read_page_size`:
    /// poppler ignores it (612×792 both in pdfinfo and the raster),
    /// mupdf scales (1224×1584). Asserting poppler's actual
    /// behavior so the inconsistency is a test, not folklore.
    #[test]
    fn poppler_ignores_user_unit() {
        if !(have("pdfinfo") && have("pdftoppm")) {
            return;
        }
        let doc = SubprocessDocument::open(
            &CliSource::Bytes(test_pdf_extra(&[(612, 792, "/UserUnit 2")])),
            PdfBackend::Poppler,
        )
        .unwrap();
        assert_eq!(
            doc.page_size(0),
            Some(CliPageSize {
                width: 612.0,
                height: 792.0
            })
        );
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
