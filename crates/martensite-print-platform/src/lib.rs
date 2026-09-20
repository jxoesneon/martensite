//! OS-native print backends for Martensite.
//!
//! This crate provides platform-specific printer enumeration and job
//! submission:
//!
//! - **macOS / Linux**: CUPS — `lpstat` for enumeration (`-p` for
//!   queues, `-d` for the default) and `lp` for submission (`-d`, `-n`,
//!   `-t`, `-P`, `-o` option strings)
//! - **Windows**: PowerShell — `Get-Printer` for enumeration,
//!   `Out-Printer` for text payloads, `Start-Process -Verb Print` for
//!   file payloads
//!
//! # Architecture
//!
//! This crate intentionally does **not** depend on `martensite-print` to
//! avoid a cyclic dependency. It defines its own [`PrintBackend`] trait,
//! wire types ([`PrintSpec`], [`SpecSource`], [`SpecSides`],
//! [`PrintReply`], [`BackendPrinter`]), and a [`native_backend`]
//! factory. The `martensite-print` crate wraps this crate's API behind
//! its own `PlatformPrinter` trait when the `platform` feature is
//! enabled.
//!
//! # Safety policy
//!
//! Like `martensite-dialog-platform`, this crate carries
//! `#![forbid(unsafe_code)]`: every backend drives the platform's own
//! print facility through short-lived subprocesses, so no FFI or unsafe
//! code is required at all.
//!
//! # Examples
//!
//! ```
//! use martensite_print_platform::native_backend;
//!
//! // `Some` on desktop targets with a usable print tool on PATH;
//! // `None` elsewhere (e.g. a bare Linux CI box without CUPS).
//! let _backend = native_backend();
//! ```
#![forbid(unsafe_code)]
#![forbid(missing_docs)]

use std::path::PathBuf;

#[cfg(any(target_os = "macos", target_os = "linux"))]
mod cups;
#[cfg(target_os = "windows")]
mod windows;

/// Where a [`PrintSpec`]'s payload comes from (wire type).
///
/// Mirrors `martensite_print::PrintSource`.
///
/// # Examples
///
/// ```
/// use martensite_print_platform::SpecSource;
/// use std::path::PathBuf;
///
/// let s = SpecSource::File(PathBuf::from("/a.pdf"));
/// assert!(matches!(s, SpecSource::File(_)));
/// ```
#[derive(Clone, Debug)]
pub enum SpecSource {
    /// An existing file on disk.
    File(PathBuf),
    /// Raw bytes plus an optional MIME hint.
    Bytes {
        /// The payload.
        data: Vec<u8>,
        /// MIME media type, when known.
        mime: Option<String>,
    },
}

/// Duplex mode for a [`PrintSpec`] (wire type).
///
/// # Examples
///
/// ```
/// use martensite_print_platform::SpecSides;
///
/// assert_ne!(SpecSides::LongEdge, SpecSides::ShortEdge);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SpecSides {
    /// Leave the driver default.
    #[default]
    Default,
    /// `sides=one-sided`.
    OneSided,
    /// `sides=two-sided-long-edge`.
    LongEdge,
    /// `sides=two-sided-short-edge`.
    ShortEdge,
}

/// A platform-agnostic print request (wire type).
///
/// Mirrors `martensite_print::PrintJob` with primitive fields the
/// backends translate to their own option vocabulary.
///
/// # Examples
///
/// ```
/// use martensite_print_platform::{PrintSpec, SpecSource};
/// use std::path::PathBuf;
///
/// let s = PrintSpec {
///     title: "Report".into(),
///     source: SpecSource::File(PathBuf::from("/a.pdf")),
///     ..Default::default()
/// };
/// assert_eq!(s.copies, 1);
/// ```
#[derive(Clone, Debug)]
pub struct PrintSpec {
    /// Human-readable job name.
    pub title: String,
    /// The payload.
    pub source: SpecSource,
    /// Destination queue (`None` = system default).
    pub printer: Option<String>,
    /// Number of copies.
    pub copies: u32,
    /// Inclusive 1-based page range.
    pub pages: Option<(u32, u32)>,
    /// CUPS media name, e.g. `"A4"`, `"Letter"`.
    pub media: String,
    /// Landscape orientation.
    pub landscape: bool,
    /// Duplex mode.
    pub sides: SpecSides,
    /// `false` requests monochrome.
    pub color: bool,
}

impl Default for PrintSpec {
    fn default() -> Self {
        Self {
            title: String::new(),
            source: SpecSource::Bytes {
                data: Vec::new(),
                mime: None,
            },
            printer: None,
            copies: 1,
            pages: None,
            media: "A4".to_string(),
            landscape: false,
            sides: SpecSides::Default,
            color: true,
        }
    }
}

/// The reply a backend produces for a [`PrintSpec`].
///
/// # Examples
///
/// ```
/// use martensite_print_platform::PrintReply;
///
/// let r = PrintReply::Submitted { job_id: None };
/// assert!(matches!(r, PrintReply::Submitted { .. }));
/// ```
#[derive(Clone, Debug, PartialEq)]
pub enum PrintReply {
    /// The spooler accepted the job; `job_id` is its identifier when
    /// reported (CUPS `request id is <queue>-<n>`).
    Submitted {
        /// Spooler job identifier, when reported.
        job_id: Option<String>,
    },
    /// The backend could not submit the job.
    Failed(String),
}

/// One printer queue a backend can see (wire type).
///
/// # Examples
///
/// ```
/// use martensite_print_platform::BackendPrinter;
///
/// let p = BackendPrinter {
///     name: "Office".into(),
///     description: String::new(),
///     is_default: true,
/// };
/// assert!(p.is_default);
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BackendPrinter {
    /// Queue name.
    pub name: String,
    /// Human-readable description, when reported.
    pub description: String,
    /// `true` for the system default queue.
    pub is_default: bool,
}

/// A native print backend.
///
/// Implementations block for the duration of the submission and return
/// the spooler's reply. A backend that cannot launch should not be
/// constructed (see [`native_backend`]); runtime failures map to
/// [`PrintReply::Failed`].
///
/// # Examples
///
/// ```
/// use martensite_print_platform::{PrintBackend, PrintReply, PrintSpec};
///
/// struct Null;
/// impl PrintBackend for Null {
///     fn printers(&mut self) -> Vec<martensite_print_platform::BackendPrinter> {
///         Vec::new()
///     }
///     fn print(&mut self, _s: &PrintSpec) -> PrintReply {
///         PrintReply::Failed("null".into())
///     }
///     fn platform_name(&self) -> &str { "null" }
/// }
/// assert_eq!(Null.platform_name(), "null");
/// ```
pub trait PrintBackend {
    /// The queues this backend can see.
    fn printers(&mut self) -> Vec<BackendPrinter>;

    /// Submit `spec` and return the spooler's reply.
    fn print(&mut self, spec: &PrintSpec) -> PrintReply;

    /// Human-readable backend name, e.g. `"cups"`.
    fn platform_name(&self) -> &str;
}

/// Returns the best available [`PrintBackend`] for the current target.
///
/// | Target | Backend | Availability gate |
/// |--------|---------|-------------------|
/// | `macos`, `linux` | `cups` | `lp` on `PATH` |
/// | `windows` | `windows-powershell` | always (PowerShell is in-box) |
/// | other | — | `None` |
///
/// # Examples
///
/// ```
/// use martensite_print_platform::native_backend;
///
/// if let Some(b) = native_backend() {
///     assert!(!b.platform_name().is_empty());
/// }
/// ```
pub fn native_backend() -> Option<Box<dyn PrintBackend>> {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        cups::CupsPrinter::probe()
    }
    #[cfg(target_os = "windows")]
    {
        Some(Box::new(windows::PowerShellPrinter::new()))
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        None
    }
}
