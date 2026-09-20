//! Platform backend abstraction.
//!
//! This module defines the [`PlatformPrinter`] trait, which extends
//! [`PrinterService`] with a [`PlatformPrinter::platform_name`]
//! accessor, together with a [`StubPrinter`] no-op implementation and a
//! [`default_platform_printer`] factory that selects the best available
//! backend for the current target.
//!
//! # Platform mechanisms
//!
//! Native printing is driven by `martensite-print-platform` when the
//! `platform` Cargo feature is enabled. That crate shells out to the
//! platform's own print facility — no FFI is required:
//!
//! * **macOS / Linux** — CUPS: `lpstat` for enumeration, `lp` for
//!   submission (`-d` destination, `-n` copies, `-t` title, `-P` page
//!   list, `-o` media/duplex/color options).
//! * **Windows** — PowerShell: `Get-Printer` for enumeration,
//!   `Out-Printer` for text payloads, `Start-Process -Verb Print` for
//!   file payloads.
//!
//! Without the `platform` feature every backend is a **safe stub** that
//! reports [`PrintOutcome::Failed`]; this crate is
//! `#![forbid(unsafe_code)]` and stays a safe, auditable dependency
//! either way.
//!
//! # Examples
//!
//! ```no_run
//! use martensite_print::{PlatformPrinter, PrintJob, PrinterService,
//!     default_platform_printer};
//!
//! let mut p = default_platform_printer();
//! // The factory always returns a usable (possibly stub) service.
//! assert!(!p.platform_name().is_empty());
//! let _ = p.print(&PrintJob::file("/tmp/report.pdf"));
//! ```

use crate::job::{PrintJob, PrintOutcome, PrinterInfo, PrinterService};

/// A [`PrinterService`] backed by a specific platform print facility.
///
/// Implementations identify themselves via
/// [`platform_name`](PlatformPrinter::platform_name) so callers and
/// diagnostics can report which backend is active.
///
/// # Examples
///
/// ```
/// use martensite_print::{PlatformPrinter, StubPrinter};
///
/// assert_eq!(StubPrinter::new().platform_name(), "stub");
/// ```
pub trait PlatformPrinter: PrinterService {
    /// Returns a human-readable backend name, e.g. `"cups"`,
    /// `"windows-powershell"`, or `"stub"`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_print::{PlatformPrinter, StubPrinter};
    ///
    /// assert_eq!(StubPrinter::new().platform_name(), "stub");
    /// ```
    fn platform_name(&self) -> &str;
}

/// A no-op printer for environments without OS print support.
///
/// `printers()` returns an empty list and `print` reports
/// [`PrintOutcome::Failed`]. This is the fallback used by
/// [`default_platform_printer`] when no backend is compiled in, and is
/// useful for headless tests and CI.
///
/// # Examples
///
/// ```
/// use martensite_print::{PlatformPrinter, PrinterService, StubPrinter};
///
/// let mut p = StubPrinter::new();
/// assert!(p.printers().is_empty());
/// ```
#[derive(Default, Clone, Debug)]
pub struct StubPrinter;

impl StubPrinter {
    /// Creates a new [`StubPrinter`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_print::{PlatformPrinter, StubPrinter};
    ///
    /// assert_eq!(StubPrinter::new().platform_name(), "stub");
    /// ```
    #[inline]
    pub fn new() -> Self {
        Self
    }
}

impl PrinterService for StubPrinter {
    fn printers(&mut self) -> Vec<PrinterInfo> {
        Vec::new()
    }

    fn print(&mut self, _job: &PrintJob) -> PrintOutcome {
        PrintOutcome::Failed("no print backend available".to_string())
    }
}

impl PlatformPrinter for StubPrinter {
    #[inline]
    fn platform_name(&self) -> &str {
        "stub"
    }
}

/// Returns the best available [`PlatformPrinter`] for the current target.
///
/// The selection rules are:
///
/// | Target | Backend |
/// |--------|---------|
/// | `macos` / `linux` | `cups` (`lp`/`lpstat` on `PATH`) |
/// | `windows` | `windows-powershell` |
/// | other | [`StubPrinter`] |
///
/// Without the `platform` Cargo feature this returns [`StubPrinter`];
/// with it, the function delegates to
/// `martensite_print_platform::native_backend` and only falls back to
/// [`StubPrinter`] when no usable backend exists (e.g. no `lp` on
/// `PATH`). [`ScriptedPrinter`](crate::ScriptedPrinter) remains the
/// choice for tests. The returned service is always usable and never
/// panics.
///
/// # Examples
///
/// ```
/// use martensite_print::{PlatformPrinter, default_platform_printer};
///
/// let p = default_platform_printer();
/// assert!(!p.platform_name().is_empty());
/// ```
pub fn default_platform_printer() -> Box<dyn PlatformPrinter> {
    cfg_default_platform_printer()
}

#[cfg(feature = "platform")]
fn cfg_default_platform_printer() -> Box<dyn PlatformPrinter> {
    if let Some(backend) = martensite_print_platform::native_backend() {
        return Box::new(PlatformBackendAdapter(backend));
    }
    Box::new(StubPrinter::new())
}

#[cfg(not(feature = "platform"))]
fn cfg_default_platform_printer() -> Box<dyn PlatformPrinter> {
    Box::new(StubPrinter::new())
}

/// Adapter wrapping a `martensite_print_platform::PrintBackend` as a
/// [`PlatformPrinter`].
///
/// The subprocess backend trait lives in the platform crate to avoid a
/// cyclic dependency; this adapter maps the safe crate's
/// [`PrintJob`]/[`PrintOutcome`] onto the platform crate's
/// `PrintSpec`/`PrintReply` wire types.
#[cfg(feature = "platform")]
struct PlatformBackendAdapter(Box<dyn martensite_print_platform::PrintBackend>);

#[cfg(feature = "platform")]
impl PrinterService for PlatformBackendAdapter {
    fn printers(&mut self) -> Vec<PrinterInfo> {
        self.0
            .printers()
            .into_iter()
            .map(|p| PrinterInfo {
                name: p.name,
                description: p.description,
                is_default: p.is_default,
            })
            .collect()
    }

    fn print(&mut self, job: &PrintJob) -> PrintOutcome {
        use martensite_print_platform::{PrintReply, PrintSpec, SpecSides, SpecSource};
        let source = match &job.source {
            crate::PrintSource::File(p) => SpecSource::File(p.clone()),
            crate::PrintSource::Bytes { data, mime } => SpecSource::Bytes {
                data: data.clone(),
                mime: mime.clone(),
            },
        };
        let sides = match job.duplex {
            crate::Duplex::Default => SpecSides::Default,
            crate::Duplex::OneSided => SpecSides::OneSided,
            crate::Duplex::LongEdge => SpecSides::LongEdge,
            crate::Duplex::ShortEdge => SpecSides::ShortEdge,
        };
        let spec = PrintSpec {
            title: job.title.clone(),
            source,
            printer: job.printer.clone(),
            copies: job.copies,
            pages: job.pages.map(|r| (r.first, r.last)),
            media: job.media.cups_media().to_string(),
            landscape: matches!(job.orientation, crate::Orientation::Landscape),
            sides,
            color: job.color,
        };
        match self.0.print(&spec) {
            PrintReply::Submitted { job_id } => PrintOutcome::Submitted { job_id },
            PrintReply::Failed(e) => PrintOutcome::Failed(e),
        }
    }
}

#[cfg(feature = "platform")]
impl PlatformPrinter for PlatformBackendAdapter {
    fn platform_name(&self) -> &str {
        self.0.platform_name()
    }
}
