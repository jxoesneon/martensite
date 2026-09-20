//! Core notification data model and service trait.
//!
//! This module is platform-agnostic: it defines the wire types every
//! backend consumes ([`Notification`], [`Urgency`]), the [`NotifyService`]
//! contract, and [`ScriptedNotifier`], a recording implementation for
//! tests and headless environments.
//!
//! # Examples
//!
//! ```
//! use martensite_notify::{Notification, NotifyService, ScriptedNotifier};
//!
//! let mut svc = ScriptedNotifier::new();
//! svc.notify(&Notification::new("Build finished")).unwrap();
//! assert_eq!(svc.sent().len(), 1);
//! ```

/// How prominently the notification should be presented.
///
/// Maps to `notify-send --urgency` on Linux and to sound/critical hints
/// on other platforms.
///
/// # Examples
///
/// ```
/// use martensite_notify::Urgency;
///
/// assert_eq!(Urgency::default(), Urgency::Normal);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Urgency {
    /// Passive, auto-dismissed, no sound.
    Low,
    /// Standard notification.
    #[default]
    Normal,
    /// Important — persists and may play a sound.
    Critical,
}

/// A single OS notification.
///
/// # Examples
///
/// ```
/// use martensite_notify::{Notification, Urgency};
///
/// let n = Notification::new("Build finished")
///     .body("3 warnings")
///     .urgency(Urgency::Critical);
/// assert_eq!(n.title, "Build finished");
/// assert_eq!(n.body, "3 warnings");
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Notification {
    /// Headline.
    pub title: String,
    /// Secondary text under the title.
    pub body: String,
    /// Tertiary caption (macOS `subtitle`; ignored elsewhere).
    pub subtitle: String,
    /// Presentation priority.
    pub urgency: Urgency,
    /// Notification sound name (macOS `sound name`; Linux hint).
    pub sound: Option<String>,
}

impl Notification {
    /// A notification titled `title`.
    ///
    /// ```
    /// use martensite_notify::Notification;
    ///
    /// assert_eq!(Notification::new("Hi").title, "Hi");
    /// ```
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            ..Default::default()
        }
    }

    /// Body text.
    ///
    /// ```
    /// use martensite_notify::Notification;
    ///
    /// assert_eq!(Notification::new("T").body("b").body, "b");
    /// ```
    pub fn body(mut self, body: impl Into<String>) -> Self {
        self.body = body.into();
        self
    }

    /// Subtitle (macOS only).
    ///
    /// ```
    /// use martensite_notify::Notification;
    ///
    /// assert_eq!(Notification::new("T").subtitle("s").subtitle, "s");
    /// ```
    pub fn subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = subtitle.into();
        self
    }

    /// Urgency level.
    ///
    /// ```
    /// use martensite_notify::{Notification, Urgency};
    ///
    /// assert_eq!(Notification::new("T").urgency(Urgency::Low).urgency, Urgency::Low);
    /// ```
    pub fn urgency(mut self, urgency: Urgency) -> Self {
        self.urgency = urgency;
        self
    }

    /// Notification sound name.
    ///
    /// ```
    /// use martensite_notify::Notification;
    ///
    /// assert_eq!(Notification::new("T").sound("Ping").sound.as_deref(), Some("Ping"));
    /// ```
    pub fn sound(mut self, sound: impl Into<String>) -> Self {
        self.sound = Some(sound.into());
        self
    }
}

/// Why a notification could not be delivered.
///
/// # Examples
///
/// ```
/// use martensite_notify::NotifyError;
///
/// let e = NotifyError::Failed("spawn".into());
/// assert!(e.to_string().contains("spawn"));
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NotifyError {
    /// No notification facility exists on this system.
    Unavailable,
    /// The backend was invoked but failed (`stderr` / exit detail).
    Failed(String),
}

impl std::fmt::Display for NotifyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NotifyError::Unavailable => write!(f, "no notification facility available"),
            NotifyError::Failed(msg) => write!(f, "notification failed: {msg}"),
        }
    }
}

impl std::error::Error for NotifyError {}

/// The service contract every notifier backend implements.
///
/// Delivery is best-effort and one-shot: there is no delivery receipt and
/// no click-action callback (platform action callbacks require app-bundle
/// registration beyond this crate's scope).
///
/// # Examples
///
/// ```
/// use martensite_notify::{Notification, NotifyService, ScriptedNotifier};
///
/// let mut svc = ScriptedNotifier::new();
/// assert!(svc.notify(&Notification::new("T")).is_ok());
/// ```
pub trait NotifyService {
    /// Deliver `notification` to the OS notification facility.
    fn notify(&mut self, notification: &Notification) -> Result<(), NotifyError>;
}

/// A recording backend for tests and headless runs.
///
/// Every notification is appended to [`ScriptedNotifier::sent`] and
/// `Ok(())` is returned.
///
/// # Examples
///
/// ```
/// use martensite_notify::{Notification, NotifyService, ScriptedNotifier};
///
/// let mut svc = ScriptedNotifier::new();
/// svc.notify(&Notification::new("a")).unwrap();
/// svc.notify(&Notification::new("b")).unwrap();
/// assert_eq!(svc.sent()[1].title, "b");
/// ```
#[derive(Default, Debug)]
pub struct ScriptedNotifier {
    sent: Vec<Notification>,
}

impl ScriptedNotifier {
    /// An empty recorder.
    ///
    /// ```
    /// use martensite_notify::ScriptedNotifier;
    ///
    /// assert!(ScriptedNotifier::new().sent().is_empty());
    /// ```
    pub fn new() -> Self {
        Self::default()
    }

    /// All notifications delivered so far, in order.
    ///
    /// ```
    /// use martensite_notify::{Notification, NotifyService, ScriptedNotifier};
    ///
    /// let mut s = ScriptedNotifier::new();
    /// s.notify(&Notification::new("x")).unwrap();
    /// assert_eq!(s.sent().len(), 1);
    /// ```
    pub fn sent(&self) -> &[Notification] {
        &self.sent
    }
}

impl NotifyService for ScriptedNotifier {
    fn notify(&mut self, notification: &Notification) -> Result<(), NotifyError> {
        self.sent.push(notification.clone());
        Ok(())
    }
}
