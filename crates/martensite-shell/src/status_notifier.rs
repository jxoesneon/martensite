//! StatusNotifierItem D-Bus system tray registration.
//!
//! Implements the `org.kde.StatusNotifierItem` D-Bus protocol for
//! registering an application icon on the system tray. This is a
//! self-contained module — it does not depend on any Wayland
//! protocol, only on D-Bus via the `zbus` crate.
//!
//! Registration connects to the D-Bus session bus, exports the
//! `org.kde.StatusNotifierItem` interface at `/StatusNotifierItem`,
//! requests the well-known name
//! `org.freedesktop.StatusNotifierItem-<pid>-<id>`, and notifies the
//! `org.kde.StatusNotifierWatcher` service by calling its
//! `RegisterStatusNotifierItem` method.
//!
//! This module is gated behind `all(target_os = "linux", feature =
//! "wayland-backend")` because `zbus` is only pulled in under that
//! feature. The protocol itself works on any D-Bus-capable desktop.

#![cfg(all(target_os = "linux", feature = "wayland-backend"))]

use std::sync::Arc;

/// D-Bus object path exported by [`StatusNotifierItem`].
///
/// The de-facto standard path for a single StatusNotifierItem
/// instance. Each instance must provide an object called
/// `StatusNotifierItem`; the canonical path is `/StatusNotifierItem`.
const SNI_OBJECT_PATH: &str = "/StatusNotifierItem";

/// D-Bus service name of the KDE StatusNotifierWatcher.
const SNI_WATCHER_SERVICE: &str = "org.kde.StatusNotifierWatcher";

/// D-Bus object path of the KDE StatusNotifierWatcher.
const SNI_WATCHER_PATH: &str = "/StatusNotifierWatcher";

/// D-Bus interface name of the KDE StatusNotifierWatcher.
const SNI_WATCHER_INTERFACE: &str = "org.kde.StatusNotifierWatcher";

/// Callback invoked when the tray icon is activated (clicked).
///
/// The `x` and `y` arguments are the screen coordinates of the click
/// relative to the tray icon. Implementations typically open the
/// application's main window or menu at these coordinates.
pub type ActivateCallback = Arc<dyn Fn(i32, i32) + Send + Sync>;

/// Callback invoked when the tray icon is secondary-activated
/// (e.g. middle-clicked).
pub type SecondaryActivateCallback = Arc<dyn Fn(i32, i32) + Send + Sync>;

/// Callback invoked when the tray icon's context menu is requested
/// (e.g. right-clicked).
pub type ContextMenuCallback = Arc<dyn Fn(i32, i32) + Send + Sync>;

/// Callback invoked when the tray icon is scrolled.
///
/// The `delta` is the scroll amount (positive = up/right, negative =
/// down/left); `orientation` is `"vertical"` or `"horizontal"`.
pub type ScrollCallback = Arc<dyn Fn(i32, &str) + Send + Sync>;

/// The `org.kde.StatusNotifierItem` D-Bus interface served by a registered
/// [`StatusNotifierItem`].
///
/// This exposes the minimal set of properties and methods that tray
/// implementations (KDE Plasma, SNI, ayatana) query after registration:
/// `Category`, `Id`, `Title`, `Status`, `WindowId`, `IconName`, and
/// `IconThemePath`, plus the `Activate`/`SecondaryActivate`/`ContextMenu`/
/// `Scroll` methods that trays invoke on user interaction. The icon
/// pixmaps and tooltip properties are omitted; trays fall back to
/// `IconName` when they are absent.
struct StatusNotifierItemIface {
    /// The application id reported via the `Id` property.
    id: String,
    /// The display title reported via the `Title` property.
    title: String,
    /// Optional callback invoked when `Activate` is called by the tray.
    activate: Option<ActivateCallback>,
    /// Optional callback invoked when `SecondaryActivate` is called.
    secondary_activate: Option<SecondaryActivateCallback>,
    /// Optional callback invoked when `ContextMenu` is called.
    context_menu: Option<ContextMenuCallback>,
    /// Optional callback invoked when `Scroll` is called.
    scroll: Option<ScrollCallback>,
}

#[zbus::interface(name = "org.kde.StatusNotifierItem")]
impl StatusNotifierItemIface {
    /// `Category` — always `ApplicationStatus` for an application icon.
    #[zbus(property)]
    fn category(&self) -> String {
        "ApplicationStatus".to_string()
    }

    /// `Id` — the application id.
    #[zbus(property)]
    fn id(&self) -> String {
        self.id.clone()
    }

    /// `Title` — the display title.
    #[zbus(property)]
    fn title(&self) -> String {
        self.title.clone()
    }

    /// `Status` — always `Active` while the application is running.
    #[zbus(property)]
    fn status(&self) -> String {
        "Active".to_string()
    }

    /// `WindowId` — the X11 window id (0 on pure Wayland).
    #[zbus(property)]
    fn window_id(&self) -> i32 {
        0
    }

    /// `IconName` — the freedesktop icon name (empty until wired up).
    #[zbus(property)]
    fn icon_name(&self) -> String {
        String::new()
    }

    /// `IconThemePath` — the icon theme path (empty string).
    #[zbus(property)]
    fn icon_theme_path(&self) -> String {
        String::new()
    }

    /// `Activate` — the tray icon was clicked at the given coordinates.
    ///
    /// If an activation callback is registered, it is invoked; otherwise
    /// this is a no-op.
    fn activate(&self, x: i32, y: i32) {
        if let Some(cb) = &self.activate {
            cb(x, y);
        }
    }

    /// `SecondaryActivate` — secondary activation (e.g. middle-click).
    ///
    /// If a secondary-activation callback is registered, it is invoked;
    /// otherwise this is a no-op.
    fn secondary_activate(&self, x: i32, y: i32) {
        if let Some(cb) = &self.secondary_activate {
            cb(x, y);
        }
    }

    /// `ContextMenu` — show the context menu at the given coordinates.
    ///
    /// If a context-menu callback is registered, it is invoked;
    /// otherwise this is a no-op.
    fn context_menu(&self, x: i32, y: i32) {
        if let Some(cb) = &self.context_menu {
            cb(x, y);
        }
    }

    /// `Scroll` — the tray icon was scrolled.
    ///
    /// If a scroll callback is registered, it is invoked; otherwise
    /// this is a no-op.
    fn scroll(&self, delta: i32, orientation: &str) {
        if let Some(cb) = &self.scroll {
            cb(delta, orientation);
        }
    }
}

/// StatusNotifierItem system tray registration.
///
/// Implements the `org.kde.StatusNotifierItem` D-Bus protocol for
/// registering an application icon on the system tray. Registration
/// connects to the D-Bus session bus, exports the
/// `org.kde.StatusNotifierItem` interface at
/// `/StatusNotifierItem`,
/// requests the well-known name
/// `org.freedesktop.StatusNotifierItem-<pid>-<id>`, and notifies the
/// `org.kde.StatusNotifierWatcher` service by calling its
/// `RegisterStatusNotifierItem` method.
///
/// If no D-Bus session bus is available (e.g. running without a desktop
/// session), [`register`](Self::register) returns an error and leaves
/// the item unregistered rather than panicking.
///
/// # Examples
///
/// ```
/// use martensite_shell::status_notifier::StatusNotifierItem;
///
/// let item = StatusNotifierItem::new("my-app", "My Application");
/// assert_eq!(item.id(), "my-app");
/// assert_eq!(item.title(), "My Application");
/// assert!(!item.is_registered());
/// ```
pub struct StatusNotifierItem {
    /// The D-Bus ID (used to build the well-known name).
    id: String,
    /// The display title reported via the `Title` property.
    title: String,
    /// Whether the item is currently registered on the session bus.
    registered: bool,
    /// The live D-Bus session bus connection, held while registered.
    /// Dropping this unregisters the item from the bus.
    connection: Option<zbus::blocking::Connection>,
    /// Callback invoked when the tray icon is activated (clicked).
    activate: Option<ActivateCallback>,
    /// Callback invoked on secondary activation (middle-click).
    secondary_activate: Option<SecondaryActivateCallback>,
    /// Callback invoked when the context menu is requested.
    context_menu: Option<ContextMenuCallback>,
    /// Callback invoked when the tray icon is scrolled.
    scroll: Option<ScrollCallback>,
}

impl StatusNotifierItem {
    /// Creates a new StatusNotifierItem with the given D-Bus ID and title.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_shell::status_notifier::StatusNotifierItem;
    ///
    /// let item = StatusNotifierItem::new("my-app", "My Application");
    /// assert_eq!(item.id(), "my-app");
    /// ```
    #[must_use]
    pub fn new(id: &str, title: &str) -> Self {
        Self {
            id: id.to_string(),
            title: title.to_string(),
            registered: false,
            connection: None,
            activate: None,
            secondary_activate: None,
            context_menu: None,
            scroll: None,
        }
    }

    /// Returns the D-Bus ID of the item.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns the display title of the item.
    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Returns true if the item is registered on the session bus.
    #[must_use]
    pub fn is_registered(&self) -> bool {
        self.registered
    }

    /// Sets the callback invoked when the tray icon is activated
    /// (clicked). The callback receives the `(x, y)` screen
    /// coordinates of the click relative to the tray icon.
    ///
    /// This must be called *before* [`register`](Self::register) so
    /// the callback is in place when the tray first queries the item.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_shell::status_notifier::StatusNotifierItem;
    /// use std::sync::atomic::{AtomicBool, Ordering};
    /// use std::sync::Arc;
    ///
    /// let mut item = StatusNotifierItem::new("my-app", "My Application");
    /// let clicked = Arc::new(AtomicBool::new(false));
    /// let clicked_clone = clicked.clone();
    /// item.on_activate(move |_x, _y| {
    ///     clicked_clone.store(true, Ordering::SeqCst);
    /// });
    /// ```
    pub fn on_activate<F>(&mut self, callback: F)
    where
        F: Fn(i32, i32) + Send + Sync + 'static,
    {
        self.activate = Some(Arc::new(callback));
    }

    /// Sets the callback invoked on secondary activation (middle-click).
    ///
    /// Must be called before [`register`](Self::register).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_shell::status_notifier::StatusNotifierItem;
    ///
    /// let mut item = StatusNotifierItem::new("my-app", "My Application");
    /// item.on_secondary_activate(|_x, _y| { /* middle-click handler */ });
    /// ```
    pub fn on_secondary_activate<F>(&mut self, callback: F)
    where
        F: Fn(i32, i32) + Send + Sync + 'static,
    {
        self.secondary_activate = Some(Arc::new(callback));
    }

    /// Sets the callback invoked when the context menu is requested
    /// (right-click).
    ///
    /// Must be called before [`register`](Self::register).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_shell::status_notifier::StatusNotifierItem;
    ///
    /// let mut item = StatusNotifierItem::new("my-app", "My Application");
    /// item.on_context_menu(|_x, _y| { /* show context menu */ });
    /// ```
    pub fn on_context_menu<F>(&mut self, callback: F)
    where
        F: Fn(i32, i32) + Send + Sync + 'static,
    {
        self.context_menu = Some(Arc::new(callback));
    }

    /// Sets the callback invoked when the tray icon is scrolled.
    ///
    /// The callback receives `(delta, orientation)` where `delta` is
    /// the scroll amount (positive = up/right) and `orientation` is
    /// `"vertical"` or `"horizontal"`.
    ///
    /// Must be called before [`register`](Self::register).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_shell::status_notifier::StatusNotifierItem;
    ///
    /// let mut item = StatusNotifierItem::new("my-app", "My Application");
    /// item.on_scroll(|_delta, _orientation| { /* scroll handler */ });
    /// ```
    pub fn on_scroll<F>(&mut self, callback: F)
    where
        F: Fn(i32, &str) + Send + Sync + 'static,
    {
        self.scroll = Some(Arc::new(callback));
    }

    /// Registers the item on the D-Bus session bus.
    ///
    /// This connects to the session bus, exports the
    /// `org.kde.StatusNotifierItem` interface at
    /// `/StatusNotifierItem`, requests the
    /// well-known name `org.freedesktop.StatusNotifierItem-<pid>-<id>`,
    /// and calls `RegisterStatusNotifierItem` on the
    /// `org.kde.StatusNotifierWatcher` service so the tray picks up the
    /// new icon.
    ///
    /// On failure (e.g. no D-Bus session bus is available, the watcher
    /// is not running, or the well-known name is already taken), the
    /// item is left unregistered and a human-readable error string is
    /// returned. This method never panics.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_shell::status_notifier::StatusNotifierItem;
    ///
    /// let mut item = StatusNotifierItem::new("my-app", "My Application");
    /// // Without a real D-Bus session, this returns an error we ignore.
    /// let _ = item.register();
    /// ```
    pub fn register(&mut self) -> Result<(), String> {
        // Connect to the D-Bus session bus.
        let connection = zbus::blocking::Connection::session().map_err(|e| {
            self.registered = false;
            format!("failed to connect to D-Bus session bus: {e}")
        })?;

        // Export the org.kde.StatusNotifierItem interface at the
        // canonical object path.
        let iface = StatusNotifierItemIface {
            id: self.id.clone(),
            title: self.title.clone(),
            activate: self.activate.clone(),
            secondary_activate: self.secondary_activate.clone(),
            context_menu: self.context_menu.clone(),
            scroll: self.scroll.clone(),
        };
        connection
            .object_server()
            .at(SNI_OBJECT_PATH, iface)
            .map_err(|e| format!("failed to export StatusNotifierItem object: {e}"))?;

        // Request the well-known name so trays can discover us.
        // The spec format is `org.freedesktop.StatusNotifierItem-<pid>-<id>`.
        let pid = std::process::id();
        let well_known = format!("org.freedesktop.StatusNotifierItem-{}-{}", pid, self.id);
        connection
            .request_name(well_known.as_str())
            .map_err(|e| format!("failed to request well-known name {well_known}: {e}"))?;

        // Tell the StatusNotifierWatcher to register this item.
        connection
            .call_method(
                Some(SNI_WATCHER_SERVICE),
                SNI_WATCHER_PATH,
                Some(SNI_WATCHER_INTERFACE),
                "RegisterStatusNotifierItem",
                &well_known,
            )
            .map_err(|e| format!("failed to register with StatusNotifierWatcher: {e}"))?;

        self.connection = Some(connection);
        self.registered = true;
        Ok(())
    }

    /// Unregisters the item from the D-Bus session bus.
    ///
    /// Dropping the connection releases the well-known name and removes
    /// the exported object from the bus, which causes the tray to drop
    /// the icon. The item is left in the unregistered state and can be
    /// re-registered with [`register`](Self::register).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_shell::status_notifier::StatusNotifierItem;
    ///
    /// let mut item = StatusNotifierItem::new("my-app", "My Application");
    /// item.unregister();
    /// assert!(!item.is_registered());
    /// ```
    pub fn unregister(&mut self) {
        // Dropping the connection unregisters us from D-Bus: the
        // well-known name is released and the object path is removed.
        self.connection = None;
        self.registered = false;
    }
}

impl Default for StatusNotifierItem {
    fn default() -> Self {
        Self::new("martensite", "Martensite")
    }
}

impl core::fmt::Debug for StatusNotifierItem {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // `zbus::blocking::Connection` is not `Clone`, so we implement
        // `Debug` manually and only report whether a connection is held.
        f.debug_struct("StatusNotifierItem")
            .field("id", &self.id)
            .field("title", &self.title)
            .field("registered", &self.registered)
            .field("connected", &self.connection.is_some())
            .field("has_activate", &self.activate.is_some())
            .field("has_secondary_activate", &self.secondary_activate.is_some())
            .field("has_context_menu", &self.context_menu.is_some())
            .field("has_scroll", &self.scroll.is_some())
            .finish()
    }
}
