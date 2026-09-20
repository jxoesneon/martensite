//! Printer enumeration and print-job submission abstraction.
//!
//! `martensite-print` provides a platform-agnostic model for printing —
//! a [`PrintJob`] (payload + copies + page range + media/duplex/color
//! options) submitted through a [`PrinterService`], plus printer
//! enumeration via [`PrinterInfo`] — so widgets and application code
//! never touch platform APIs directly.
//!
//! # Architecture
//!
//! * [`PrintJob`] describes one submission: a [`PrintSource`] (file or
//!   bytes + MIME hint), destination printer, copies, [`PageRange`],
//!   [`PageSize`], [`Orientation`], [`Duplex`], and color mode.
//! * [`PrintOutcome`] is `Submitted { job_id }`, `Cancelled`, or
//!   `Failed(reason)`.
//! * [`PrinterService`] is the blocking submit/enumerate contract
//!   implemented by backends. [`ScriptedPrinter`] is a deterministic
//!   canned-response implementation for tests and headless environments.
//! * [`PlatformPrinter`] extends [`PrinterService`] with a backend name.
//!   [`default_platform_printer`] selects the best available backend for
//!   the current target. Because this crate is `#![forbid(unsafe_code)]`,
//!   native printing is delegated to `martensite-print-platform` behind
//!   the `platform` Cargo feature; without it, every backend is a safe
//!   stub (see the [`platform`] module docs).
//!
//! # Examples
//!
//! ```
//! use martensite_print::{Duplex, PageRange, PrintJob, PrinterService,
//!     ScriptedPrinter};
//!
//! let mut printer = ScriptedPrinter::new();
//! let job = PrintJob::text("Receipt", "total: $4.00\n")
//!     .copies(2)
//!     .duplex(Duplex::LongEdge);
//! assert!(printer.print(&job).is_ok());
//! assert_eq!(printer.last_job().unwrap().copies, 2);
//! ```
#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod job;
pub mod platform;

pub use job::{
    Duplex, Orientation, PageRange, PageSize, PrintJob, PrintOutcome, PrintSource, PrinterInfo,
    PrinterService, ScriptedPrinter,
};
pub use platform::{default_platform_printer, PlatformPrinter, StubPrinter};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn re_exported_scripted_round_trip() {
        let mut p = ScriptedPrinter::new();
        p.respond_with(PrintOutcome::Cancelled);
        assert!(p.print(&PrintJob::file("/a.pdf")).is_cancelled());
        assert!(p.print(&PrintJob::file("/a.pdf")).is_ok());
    }

    #[test]
    fn re_exported_default_platform_printer_is_usable() {
        let p = default_platform_printer();
        assert!(!p.platform_name().is_empty());
    }

    #[test]
    #[cfg(not(feature = "platform"))]
    fn default_platform_printer_fails_without_backend() {
        let mut p = default_platform_printer();
        assert!(matches!(
            p.print(&PrintJob::file("/a")),
            PrintOutcome::Failed(_)
        ));
    }

    #[test]
    fn re_exported_stub_fails() {
        let mut p = StubPrinter::new();
        assert!(matches!(
            p.print(&PrintJob::text("t", "b")),
            PrintOutcome::Failed(_)
        ));
    }

    #[test]
    fn page_range_clamps_and_formats() {
        assert_eq!(PageRange::new(0, 0).cups_page_list(), "1");
        assert_eq!(PageRange::new(2, 9).cups_page_list(), "2-9");
    }
}
