//! OS notification abstraction.
//!
//! `martensite-notify` provides a platform-agnostic notification model —
//! title, body, subtitle, urgency, sound — so widgets and application
//! code never touch platform APIs directly.
//!
//! # Architecture
//!
//! * [`Notification`] describes one delivery; [`Urgency`] maps to the
//!   platform's prominence levels.
//! * [`NotifyService`] is the one-shot delivery contract implemented by
//!   backends; failures surface as [`NotifyError`]. [`ScriptedNotifier`]
//!   is a recording implementation for tests and headless environments.
//! * [`PlatformNotifier`] extends [`NotifyService`] with a backend name.
//!   [`default_platform_notifier`] selects the best available backend for
//!   the current target. Because this crate is `#![forbid(unsafe_code)]`,
//!   native delivery is delegated to `martensite-notify-platform` behind
//!   the `platform` Cargo feature; without it, every backend is a safe
//!   stub that discards notifications (see the [`platform`] module docs).
//!
//! # Examples
//!
//! ```
//! use martensite_notify::{Notification, NotifyService, ScriptedNotifier,
//!     Urgency};
//!
//! let mut notifier = ScriptedNotifier::new();
//! notifier
//!     .notify(&Notification::new("Export done").urgency(Urgency::Normal))
//!     .unwrap();
//! assert_eq!(notifier.sent().len(), 1);
//! ```
#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod notify;
pub mod platform;

pub use notify::{Notification, NotifyError, NotifyService, ScriptedNotifier, Urgency};
pub use platform::{default_platform_notifier, PlatformNotifier, StubNotifier};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn re_exported_scripted_records() {
        let mut n = ScriptedNotifier::new();
        n.notify(&Notification::new("a").body("b")).unwrap();
        assert_eq!(n.sent()[0].body, "b");
    }

    #[test]
    fn re_exported_default_platform_notifier_is_usable() {
        let n = default_platform_notifier();
        assert!(!n.platform_name().is_empty());
    }

    #[test]
    fn re_exported_stub_accepts() {
        let mut n = StubNotifier::new();
        assert!(n.notify(&Notification::new("x")).is_ok());
    }

    #[test]
    fn urgency_default_normal() {
        assert_eq!(Notification::new("t").urgency, Urgency::Normal);
    }
}
