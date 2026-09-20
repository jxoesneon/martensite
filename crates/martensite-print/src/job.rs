//! Core print-job data model and service trait.
//!
//! This module is platform-agnostic: it defines the request/response
//! wire types every backend consumes ([`PrintJob`], [`PrintSource`],
//! [`PageSize`], [`Orientation`], [`Duplex`], [`PageRange`],
//! [`PrintOutcome`], [`PrinterInfo`]), the [`PrinterService`] contract,
//! and [`ScriptedPrinter`], a deterministic canned-response
//! implementation for tests and headless environments.
//!
//! # Examples
//!
//! ```
//! use martensite_print::{PrintJob, PrinterService, ScriptedPrinter};
//!
//! let mut svc = ScriptedPrinter::new();
//! let job = PrintJob::text("Quarterly report", "hello\n");
//! assert!(svc.print(&job).is_ok());
//! ```

use std::path::PathBuf;

/// Where the printable payload comes from.
///
/// # Examples
///
/// ```
/// use martensite_print::PrintSource;
/// use std::path::PathBuf;
///
/// let f = PrintSource::File(PathBuf::from("/doc.pdf"));
/// assert!(matches!(f, PrintSource::File(_)));
/// ```
#[derive(Clone, Debug, PartialEq)]
pub enum PrintSource {
    /// An existing file on disk — the backend passes the path to the
    /// print system, which decodes it (CUPS auto-detects the format).
    File(PathBuf),
    /// Raw bytes with an optional MIME hint (e.g. `"application/pdf"`,
    /// `"text/plain"`). Backends stream or stage them as needed.
    Bytes {
        /// The payload.
        data: Vec<u8>,
        /// MIME media type, when known.
        mime: Option<String>,
    },
}

/// Standard paper sizes.
///
/// # Examples
///
/// ```
/// use martensite_print::PageSize;
///
/// assert_eq!(PageSize::A4.cups_media(), "A4");
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PageSize {
    /// ISO A4 — 210 × 297 mm.
    #[default]
    A4,
    /// ISO A5 — 148 × 210 mm.
    A5,
    /// US Letter — 8.5 × 11 in.
    Letter,
    /// US Legal — 8.5 × 14 in.
    Legal,
}

impl PageSize {
    /// The CUPS `media=` name for this size.
    ///
    /// ```
    /// use martensite_print::PageSize;
    ///
    /// assert_eq!(PageSize::Letter.cups_media(), "Letter");
    /// ```
    pub fn cups_media(self) -> &'static str {
        match self {
            PageSize::A4 => "A4",
            PageSize::A5 => "A5",
            PageSize::Letter => "Letter",
            PageSize::Legal => "Legal",
        }
    }
}

/// Page orientation.
///
/// # Examples
///
/// ```
/// use martensite_print::Orientation;
///
/// assert_eq!(Orientation::default(), Orientation::Portrait);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Orientation {
    /// Taller than wide.
    #[default]
    Portrait,
    /// Wider than tall.
    Landscape,
}

/// Two-sided printing mode.
///
/// # Examples
///
/// ```
/// use martensite_print::Duplex;
///
/// assert_eq!(Duplex::LongEdge.cups_sides(), Some("two-sided-long-edge"));
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Duplex {
    /// Let the printer/driver default decide.
    #[default]
    Default,
    /// One side only (`sides=one-sided`).
    OneSided,
    /// Flip on the long edge — book-style (`sides=two-sided-long-edge`).
    LongEdge,
    /// Flip on the short edge — calendar-style (`sides=two-sided-short-edge`).
    ShortEdge,
}

impl Duplex {
    /// The CUPS `sides=` value, or `None` to leave the driver default.
    ///
    /// ```
    /// use martensite_print::Duplex;
    ///
    /// assert_eq!(Duplex::Default.cups_sides(), None);
    /// ```
    pub fn cups_sides(self) -> Option<&'static str> {
        match self {
            Duplex::Default => None,
            Duplex::OneSided => Some("one-sided"),
            Duplex::LongEdge => Some("two-sided-long-edge"),
            Duplex::ShortEdge => Some("two-sided-short-edge"),
        }
    }
}

/// An inclusive 1-based page range.
///
/// # Examples
///
/// ```
/// use martensite_print::PageRange;
///
/// let r = PageRange::new(2, 5);
/// assert_eq!(r.cups_page_list(), "2-5");
/// assert_eq!(PageRange::new(7, 7).cups_page_list(), "7");
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PageRange {
    /// First page, 1-based.
    pub first: u32,
    /// Last page, inclusive.
    pub last: u32,
}

impl PageRange {
    /// An inclusive range; `last` is clamped to `first` when smaller.
    ///
    /// ```
    /// use martensite_print::PageRange;
    ///
    /// assert_eq!(PageRange::new(9, 3).last, 9);
    /// ```
    pub fn new(first: u32, last: u32) -> Self {
        Self {
            first: first.max(1),
            last: last.max(first.max(1)),
        }
    }

    /// The CUPS `-P` page-list token (`"2-5"` or `"7"` for a single page).
    ///
    /// ```
    /// use martensite_print::PageRange;
    ///
    /// assert_eq!(PageRange::new(1, 3).cups_page_list(), "1-3");
    /// ```
    pub fn cups_page_list(self) -> String {
        if self.first == self.last {
            self.first.to_string()
        } else {
            format!("{}-{}", self.first, self.last)
        }
    }
}

/// A print job: what to print and how.
///
/// Build with [`PrintJob::file`] or [`PrintJob::text`]/[`bytes`], then
/// refine with the builder methods.
///
/// [`bytes`]: PrintJob::bytes
///
/// # Examples
///
/// ```
/// use martensite_print::{Duplex, PageRange, PrintJob};
///
/// let job = PrintJob::file("/doc.pdf")
///     .title("Report")
///     .copies(2)
///     .pages(PageRange::new(1, 3))
///     .duplex(Duplex::LongEdge);
/// assert_eq!(job.copies, 2);
/// ```
#[derive(Clone, Debug)]
pub struct PrintJob {
    /// Human-readable job name shown in print queues.
    pub title: String,
    /// The payload.
    pub source: PrintSource,
    /// Destination printer name (`None` = system default).
    pub printer: Option<String>,
    /// Number of copies (≥ 1).
    pub copies: u32,
    /// Optional inclusive page range.
    pub pages: Option<PageRange>,
    /// Paper size.
    pub media: PageSize,
    /// Orientation.
    pub orientation: Orientation,
    /// Duplex mode.
    pub duplex: Duplex,
    /// `false` requests monochrome (`print-color-mode=monochrome`).
    pub color: bool,
}

impl PrintJob {
    /// A job printing an existing file.
    ///
    /// ```
    /// use martensite_print::PrintJob;
    ///
    /// let j = PrintJob::file("/a.pdf");
    /// assert_eq!(j.copies, 1);
    /// ```
    pub fn file(path: impl Into<PathBuf>) -> Self {
        Self {
            title: String::new(),
            source: PrintSource::File(path.into()),
            printer: None,
            copies: 1,
            pages: None,
            media: PageSize::default(),
            orientation: Orientation::default(),
            duplex: Duplex::default(),
            color: true,
        }
    }

    /// A job printing a byte payload with an optional MIME hint.
    ///
    /// ```
    /// use martensite_print::{PrintJob, PrintSource};
    ///
    /// let j = PrintJob::bytes(b"%PDF-1.4", Some("application/pdf"));
    /// assert!(matches!(j.source, PrintSource::Bytes { .. }));
    /// ```
    pub fn bytes(data: impl Into<Vec<u8>>, mime: Option<&str>) -> Self {
        Self {
            source: PrintSource::Bytes {
                data: data.into(),
                mime: mime.map(str::to_string),
            },
            ..Self::file(PathBuf::new())
        }
    }

    /// A job printing plain text (MIME `text/plain`).
    ///
    /// ```
    /// use martensite_print::PrintJob;
    ///
    /// let j = PrintJob::text("Receipt", "total: $4\n");
    /// assert_eq!(j.title, "Receipt");
    /// ```
    pub fn text(title: impl Into<String>, body: impl Into<String>) -> Self {
        let mut j = Self::bytes(body.into().into_bytes(), Some("text/plain"));
        j.title = title.into();
        j
    }

    /// Job title shown in the queue.
    ///
    /// ```
    /// use martensite_print::PrintJob;
    ///
    /// assert_eq!(PrintJob::file("/x").title("T").title, "T");
    /// ```
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    /// Destination printer (`None` = system default).
    ///
    /// ```
    /// use martensite_print::PrintJob;
    ///
    /// let j = PrintJob::file("/x").printer("Office");
    /// assert_eq!(j.printer.as_deref(), Some("Office"));
    /// ```
    pub fn printer(mut self, name: impl Into<String>) -> Self {
        self.printer = Some(name.into());
        self
    }

    /// Number of copies, clamped to ≥ 1.
    ///
    /// ```
    /// use martensite_print::PrintJob;
    ///
    /// assert_eq!(PrintJob::file("/x").copies(0).copies, 1);
    /// ```
    pub fn copies(mut self, n: u32) -> Self {
        self.copies = n.max(1);
        self
    }

    /// Inclusive page range (`None` prints all pages).
    ///
    /// ```
    /// use martensite_print::{PageRange, PrintJob};
    ///
    /// let j = PrintJob::file("/x").pages(PageRange::new(2, 4));
    /// assert_eq!(j.pages.unwrap().cups_page_list(), "2-4");
    /// ```
    pub fn pages(mut self, range: PageRange) -> Self {
        self.pages = Some(range);
        self
    }

    /// Paper size.
    ///
    /// ```
    /// use martensite_print::{PageSize, PrintJob};
    ///
    /// assert_eq!(PrintJob::file("/x").media(PageSize::Legal).media, PageSize::Legal);
    /// ```
    pub fn media(mut self, size: PageSize) -> Self {
        self.media = size;
        self
    }

    /// Landscape orientation.
    ///
    /// ```
    /// use martensite_print::{Orientation, PrintJob};
    ///
    /// let j = PrintJob::file("/x").landscape();
    /// assert_eq!(j.orientation, Orientation::Landscape);
    /// ```
    pub fn landscape(mut self) -> Self {
        self.orientation = Orientation::Landscape;
        self
    }

    /// Duplex mode.
    ///
    /// ```
    /// use martensite_print::{Duplex, PrintJob};
    ///
    /// assert_eq!(PrintJob::file("/x").duplex(Duplex::ShortEdge).duplex, Duplex::ShortEdge);
    /// ```
    pub fn duplex(mut self, duplex: Duplex) -> Self {
        self.duplex = duplex;
        self
    }

    /// `false` requests monochrome output.
    ///
    /// ```
    /// use martensite_print::PrintJob;
    ///
    /// assert!(!PrintJob::file("/x").monochrome().color);
    /// ```
    pub fn monochrome(mut self) -> Self {
        self.color = false;
        self
    }
}

/// The result of submitting a [`PrintJob`].
///
/// # Examples
///
/// ```
/// use martensite_print::PrintOutcome;
///
/// let ok = PrintOutcome::Submitted {
///     job_id: Some("Office-42".into()),
/// };
/// assert!(ok.is_ok());
/// assert!(PrintOutcome::Cancelled.is_cancelled());
/// ```
#[derive(Clone, Debug, PartialEq)]
pub enum PrintOutcome {
    /// The job was accepted by the print system; `job_id` is the
    /// spooler's identifier when it reports one (CUPS `request id`).
    Submitted {
        /// Spooler job identifier, when reported.
        job_id: Option<String>,
    },
    /// The user cancelled a pre-flight dialog (backends that submit
    /// non-interactively never produce this).
    Cancelled,
    /// The backend could not submit the job.
    Failed(String),
}

impl PrintOutcome {
    /// `true` when the job was accepted.
    ///
    /// ```
    /// use martensite_print::PrintOutcome;
    ///
    /// assert!(PrintOutcome::Submitted { job_id: None }.is_ok());
    /// ```
    pub fn is_ok(&self) -> bool {
        matches!(self, PrintOutcome::Submitted { .. })
    }

    /// `true` when the user cancelled.
    ///
    /// ```
    /// use martensite_print::PrintOutcome;
    ///
    /// assert!(PrintOutcome::Cancelled.is_cancelled());
    /// ```
    pub fn is_cancelled(&self) -> bool {
        matches!(self, PrintOutcome::Cancelled)
    }
}

/// One discovered printer.
///
/// # Examples
///
/// ```
/// use martensite_print::PrinterInfo;
///
/// let p = PrinterInfo {
///     name: "Office".into(),
///     description: "LaserJet 4th floor".into(),
///     is_default: true,
/// };
/// assert!(p.is_default);
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PrinterInfo {
    /// Spooler queue name (passed to [`PrintJob::printer`]).
    pub name: String,
    /// Human-readable description, when the backend reports one.
    pub description: String,
    /// `true` for the system default printer.
    pub is_default: bool,
}

/// The service contract every print backend implements.
///
/// Calls are **blocking**: `print` returns after the job is handed to
/// the spooler (or rejected). Hosts that must keep a UI responsive
/// should invoke it from a worker thread.
///
/// # Examples
///
/// ```
/// use martensite_print::{PrintJob, PrinterService, ScriptedPrinter};
///
/// let mut svc = ScriptedPrinter::new();
/// assert!(svc.printers().is_empty());
/// assert!(svc.print(&PrintJob::file("/a.pdf")).is_ok());
/// ```
pub trait PrinterService {
    /// The printers this backend can see (empty when none are installed
    /// or enumeration is unsupported).
    fn printers(&mut self) -> Vec<PrinterInfo>;

    /// Submit `job` and return the outcome.
    fn print(&mut self, job: &PrintJob) -> PrintOutcome;
}

/// A deterministic canned-response backend for tests and headless runs.
///
/// Outcomes are enqueued via [`ScriptedPrinter::respond_with`] and popped
/// FIFO per [`print`](PrinterService::print) call; an empty queue yields
/// [`PrintOutcome::Submitted`] with no job id. The most recent job is
/// retained for assertions via [`ScriptedPrinter::last_job`].
///
/// # Examples
///
/// ```
/// use martensite_print::{PrintJob, PrintOutcome, PrinterService,
///     ScriptedPrinter};
///
/// let mut svc = ScriptedPrinter::new();
/// svc.respond_with(PrintOutcome::Failed("offline".into()));
/// assert!(svc.print(&PrintJob::file("/a")).is_ok() == false);
/// assert_eq!(svc.last_job().unwrap().title, "");
/// ```
#[derive(Default, Debug)]
pub struct ScriptedPrinter {
    queue: std::collections::VecDeque<PrintOutcome>,
    last: Option<PrintJob>,
    fake_printers: Vec<PrinterInfo>,
}

impl ScriptedPrinter {
    /// An empty scripted printer (every `print` reports `Submitted`).
    ///
    /// ```
    /// use martensite_print::{PrintJob, PrinterService, ScriptedPrinter};
    ///
    /// let mut s = ScriptedPrinter::new();
    /// assert!(s.print(&PrintJob::file("/x")).is_ok());
    /// ```
    pub fn new() -> Self {
        Self::default()
    }

    /// Enqueue an outcome returned by the next `print` call.
    ///
    /// ```
    /// use martensite_print::{PrintJob, PrintOutcome, PrinterService,
    ///     ScriptedPrinter};
    ///
    /// let mut s = ScriptedPrinter::new();
    /// s.respond_with(PrintOutcome::Cancelled);
    /// assert!(s.print(&PrintJob::file("/x")).is_cancelled());
    /// ```
    pub fn respond_with(&mut self, outcome: PrintOutcome) {
        self.queue.push_back(outcome);
    }

    /// Set the printer list `printers()` reports.
    ///
    /// ```
    /// use martensite_print::{PrinterInfo, PrinterService, ScriptedPrinter};
    ///
    /// let mut s = ScriptedPrinter::new();
    /// s.set_printers(vec![PrinterInfo {
    ///     name: "Laser".into(),
    ///     ..Default::default()
    /// }]);
    /// assert_eq!(s.printers().len(), 1);
    /// ```
    pub fn set_printers(&mut self, printers: Vec<PrinterInfo>) {
        self.fake_printers = printers;
    }

    /// The job passed to the most recent `print` call.
    ///
    /// ```
    /// use martensite_print::{PrintJob, PrinterService, ScriptedPrinter};
    ///
    /// let mut s = ScriptedPrinter::new();
    /// assert!(s.last_job().is_none());
    /// s.print(&PrintJob::file("/x").copies(3));
    /// assert_eq!(s.last_job().unwrap().copies, 3);
    /// ```
    pub fn last_job(&self) -> Option<&PrintJob> {
        self.last.as_ref()
    }
}

impl PrinterService for ScriptedPrinter {
    fn printers(&mut self) -> Vec<PrinterInfo> {
        self.fake_printers.clone()
    }

    fn print(&mut self, job: &PrintJob) -> PrintOutcome {
        self.last = Some(job.clone());
        self.queue.pop_front().unwrap_or(PrintOutcome::Submitted {
            job_id: Some("scripted-1".to_string()),
        })
    }
}
