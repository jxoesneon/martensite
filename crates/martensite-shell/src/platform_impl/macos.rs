//! macOS vibrancy and Liquid Glass backdrop integration.
//!
//! This module uses `objc2` for Objective-C runtime FFI. All unsafe
//! code is confined to this module; the crate as a whole uses
//! `#![deny(unsafe_code)]` and this module opts in with a module-level
//! `#![allow(unsafe_code)]`.
//!
//! The types defined here implement the cross-platform abstractions in
//! [`crate::backdrop`] using `NSVisualEffectView` (available since macOS
//! 10.10) and the new Liquid Glass materials introduced in macOS 26.

#![allow(unsafe_code)]

use core::ptr::NonNull;

use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2::{msg_send, Encode, Encoding};

use crate::backdrop::{
    BackdropController, BackdropMaterial, BackdropMode, VibrancyMaterial, Window,
};
use crate::event::{ShellEvent, ShellEventQueue};

// ---------------------------------------------------------------------------
// NSVisualEffectMaterial / BlendingMode / State raw values.
// ---------------------------------------------------------------------------
//
// These mirror the `NSVisualEffectMaterial`, `NSVisualEffectBlendingMode`
// and `NSVisualEffectState` Objective-C enums. We use raw `i64` values
// (matching `NSInteger` on 64-bit macOS) so the FFI does not depend on
// `icrate` framework features being enabled.

/// `NSVisualEffectBlendingModeBehindWindow` — the vibrancy material blends
/// with the content *behind* the window (desktop wallpaper, other windows).
const NS_VISUAL_EFFECT_BLENDING_MODE_BEHIND_WINDOW: i64 = 0;

/// `NSVisualEffectStateActive` — the vibrancy material is always active.
const NS_VISUAL_EFFECT_STATE_ACTIVE: i64 = 1;

/// `NSWindowOrderingModeNSWindowBelow` — place the new subview below all
/// existing subviews (at the back of the z-order).
const NS_WINDOW_ORDERING_MODE_BELOW: i64 = -1;

/// `NSOperatingSystemVersion` as returned by
/// `NSProcessInfo.operatingSystemVersion`.
///
/// This is a `repr(C)` struct of three `NSInteger` (`i64` on 64-bit macOS)
/// fields. The `Encode` implementation matches the `?` struct encoding used
/// by the Objective-C runtime so that `msg_send!` can return it by value.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct NSOperatingSystemVersion {
    /// Major version (e.g. 14, 15, 26).
    major: i64,
    /// Minor version.
    minor: i64,
    /// Patch version.
    patch: i64,
}

// SAFETY: The layout and encoding match the Objective-C
// `NSOperatingSystemVersion` struct (`{?=qqq}`).
unsafe impl Encode for NSOperatingSystemVersion {
    const ENCODING: Encoding =
        Encoding::Struct("?", &[i64::ENCODING, i64::ENCODING, i64::ENCODING]);
}

/// `CGFloat` on 64-bit macOS is `double` (`f64`).
type CGFloat = f64;

/// `NSPoint` / `CGPoint` — a 2D point. The struct encoding (`_NSPoint`)
/// matches the Objective-C runtime so `msg_send!` can return/pass it by
/// value.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct NSPoint {
    /// The x-coordinate.
    x: CGFloat,
    /// The y-coordinate.
    y: CGFloat,
}

// SAFETY: Matches the Objective-C `NSPoint` struct (`{_NSPoint=dd}`).
unsafe impl Encode for NSPoint {
    const ENCODING: Encoding =
        Encoding::Struct("_NSPoint", &[CGFloat::ENCODING, CGFloat::ENCODING]);
}

/// `NSSize` / `CGSize` — a 2D size.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct NSSize {
    /// The width.
    width: CGFloat,
    /// The height.
    height: CGFloat,
}

// SAFETY: Matches the Objective-C `NSSize` struct (`{_NSSize=dd}`).
unsafe impl Encode for NSSize {
    const ENCODING: Encoding = Encoding::Struct("_NSSize", &[CGFloat::ENCODING, CGFloat::ENCODING]);
}

/// `NSRect` / `CGRect` — a 2D rectangle (origin + size).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct NSRect {
    /// The rectangle origin.
    origin: NSPoint,
    /// The rectangle dimensions.
    size: NSSize,
}

// SAFETY: Matches the Objective-C `NSRect` struct
// (`{_NSRect={_NSPoint=dd}{_NSSize=dd}}`).
unsafe impl Encode for NSRect {
    const ENCODING: Encoding = Encoding::Struct("_NSRect", &[NSPoint::ENCODING, NSSize::ENCODING]);
}

/// Looks up an Objective-C class by name, returning `None` if it is not
/// registered with the runtime.
fn class(name: &core::ffi::CStr) -> Option<&'static AnyClass> {
    AnyClass::get(name)
}

/// Creates an autoreleased `NSString` from a NUL-terminated C string.
///
/// Returns `None` if the `NSString` class is unavailable.
unsafe fn ns_string(s: &core::ffi::CStr) -> Option<Retained<AnyObject>> {
    let cls = class(c"NSString")?;
    // SAFETY: `stringWithUTF8String:` copies the bytes during the call, so
    // the borrowed `s.as_ptr()` is valid for the duration of the message
    // send. The selector is `stringWithUTF8String:` (NoneFamily), returning
    // an autoreleased (+0) string which `Retained` retains.
    let string: Option<Retained<AnyObject>> =
        unsafe { msg_send![cls, stringWithUTF8String: s.as_ptr()] };
    string
}

/// Returns `true` if the given `NSString*` is equal to the given C string
/// (compared via `isEqualToString:`).
unsafe fn ns_string_equals(ns_str: &Retained<AnyObject>, target: &core::ffi::CStr) -> bool {
    let target_str = unsafe { ns_string(target) };
    let Some(target_str) = target_str else {
        return false;
    };
    // SAFETY: `isEqualToString:` returns a `BOOL` which `msg_send!`
    // converts to a Rust `bool`. Both arguments are valid objects.
    let equal: bool = unsafe {
        msg_send![
            ns_str,
            isEqualToString: Retained::as_ptr(&target_str) as *mut AnyObject
        ]
    };
    equal
}

/// macOS backdrop controller using `NSVisualEffectView` and Liquid Glass.
///
/// On macOS 14 and earlier, uses `NSVisualEffectView` with the
/// `setMaterial:` API. On macOS 26+, the `LiquidGlass` vibrancy material
/// maps to the new under-window background material.
///
/// The controller owns the `NSVisualEffectView` instance (held as a raw
/// pointer with a manual retain/release lifecycle) and attaches it as a
/// subview of the window's content view on first use.
///
/// # Examples
///
/// ```no_run
/// use martensite_shell::platform_impl::macos::MacosBackdropController;
/// use martensite_shell::{BackdropController, BackdropMaterial, VibrancyMaterial, Window};
/// use core::ffi::c_void;
///
/// # struct W;
/// # impl Window for W {
/// #     unsafe fn raw_handle(&self) -> *mut c_void { core::ptr::null_mut() }
/// # }
/// let mut controller = MacosBackdropController::new();
/// controller.set_material(&W, BackdropMaterial::Vibrancy(VibrancyMaterial::Sidebar));
/// ```
pub struct MacosBackdropController {
    /// The most recently requested material.
    material: BackdropMaterial,
    /// Whether the platform supports system materials (always `true` on
    /// macOS 10.10+, where `NSVisualEffectView` is available).
    supported: bool,
    /// The owned `NSVisualEffectView` instance, or `None` when no material
    /// is currently applied. Stored as a raw `+1`-retained pointer; released
    /// in [`Drop`].
    effect_view: Option<*mut AnyObject>,
}

impl MacosBackdropController {
    /// Creates a new macOS backdrop controller.
    ///
    /// The controller starts with [`BackdropMaterial::None`] and no
    /// `NSVisualEffectView` allocated. The view is created lazily on the
    /// first call to [`set_material`](BackdropController::set_material)
    /// with a supported material.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_shell::platform_impl::macos::MacosBackdropController;
    ///
    /// let controller = MacosBackdropController::new();
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self {
            material: BackdropMaterial::None,
            supported: true, // macOS always supports at least NSVisualEffectView
            effect_view: None,
        }
    }

    /// Maps a `VibrancyMaterial` to the `NSVisualEffectView.Material` value.
    ///
    /// The mapping follows Apple's `NSVisualEffectMaterial` enum:
    /// - `Sidebar` -> 7 (NSVisualEffectMaterialSidebar)
    /// - `HudWindow` -> 13 (NSVisualEffectMaterialHudWindow)
    /// - `FullScreenUI` -> 15 (NSVisualEffectMaterialFullScreenUI)
    /// - `Sheet` -> 11 (NSVisualEffectMaterialSheet)
    /// - `Titlebar` -> 3 (NSVisualEffectMaterialTitlebar)
    /// - `Menu` -> 5 (NSVisualEffectMaterialMenu)
    /// - `Popover` -> 6 (NSVisualEffectMaterialPopover)
    /// - `Tooltip` -> 17 (NSVisualEffectMaterialToolTip)
    /// - `LiquidGlass` -> 13 (fallback to `HudWindow`)
    ///
    /// `LiquidGlass` is mapped to `HudWindow` (13) as a fallback. When the
    /// `NSGlassEffectView` class is available at runtime (macOS 26+, see
    /// [`detect_glass_effect_view_class`](Self::detect_glass_effect_view_class)),
    /// the controller's internal effect view creation creates an
    /// `NSGlassEffectView` instead of an `NSVisualEffectView`, and the
    /// material value returned here is irrelevant — the glass view provides
    /// the Liquid Glass effect natively. When `NSGlassEffectView` is
    /// unavailable, the `HudWindow` (13) fallback gives callers a
    /// translucent vibrancy material on `NSVisualEffectView`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_shell::platform_impl::macos::MacosBackdropController;
    /// use martensite_shell::backdrop::VibrancyMaterial;
    ///
    /// assert_eq!(MacosBackdropController::vibrancy_to_ns_material(VibrancyMaterial::Sidebar), 7);
    /// // LiquidGlass falls back to HudWindow (13) for NSVisualEffectView.
    /// // When NSGlassEffectView is available, the material value is
    /// // ignored by the glass view.
    /// assert_eq!(MacosBackdropController::vibrancy_to_ns_material(VibrancyMaterial::LiquidGlass), 13);
    /// ```
    #[must_use]
    pub fn vibrancy_to_ns_material(material: VibrancyMaterial) -> u32 {
        match material {
            VibrancyMaterial::Sidebar => 7,
            VibrancyMaterial::HudWindow => 13,
            VibrancyMaterial::FullScreenUI => 15,
            VibrancyMaterial::Sheet => 11,
            VibrancyMaterial::Titlebar => 3,
            VibrancyMaterial::Menu => 5,
            VibrancyMaterial::Popover => 6,
            VibrancyMaterial::Tooltip => 17,
            // When `NSGlassEffectView` is available (macOS 26+),
            // `create_effect_view` creates a glass view and this material
            // value is ignored. Otherwise, fall back to `HudWindow` (13) so
            // callers still get a translucent vibrancy material on
            // `NSVisualEffectView`.
            VibrancyMaterial::LiquidGlass => 13,
        }
    }

    /// Returns `true` if the current macOS version supports Liquid Glass
    /// (macOS 26+).
    ///
    /// This queries `NSProcessInfo.operatingSystemVersion` at runtime and
    /// checks whether `majorVersion >= 26`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_shell::platform_impl::macos::MacosBackdropController;
    ///
    /// let supports = MacosBackdropController::supports_liquid_glass();
    /// // Returns false on macOS 14 and earlier, true on macOS 26+.
    /// let _ = supports;
    /// ```
    #[must_use]
    pub fn supports_liquid_glass() -> bool {
        unsafe {
            let Some(cls) = class(c"NSProcessInfo") else {
                return false;
            };
            // `processInfo` returns the shared `NSProcessInfo` singleton.
            let info: Option<Retained<AnyObject>> = msg_send![cls, processInfo];
            let Some(info) = info else {
                return false;
            };
            // `operatingSystemVersion` returns an `NSOperatingSystemVersion`
            // struct by value (NoneFamily return).
            let version: NSOperatingSystemVersion = msg_send![&info, operatingSystemVersion];
            version.major >= 26
        }
    }

    /// Returns `true` if the `NSGlassEffectView` class is registered with
    /// the Objective-C runtime.
    ///
    /// `NSGlassEffectView` is the Liquid Glass backdrop view introduced in
    /// macOS 26. When this returns `true`, the controller's internal effect
    /// view creation (called by [`set_material`](BackdropController::set_material))
    /// will create an `NSGlassEffectView` instead of falling back to
    /// `NSVisualEffectView`. When the class is unavailable (macOS 25 and
    /// earlier), the controller uses `NSVisualEffectView` with the
    /// `HudWindow` material as a fallback for the [`LiquidGlass`](VibrancyMaterial::LiquidGlass)
    /// vibrancy material.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_shell::platform_impl::macos::MacosBackdropController;
    ///
    /// let has_glass = MacosBackdropController::detect_glass_effect_view_class();
    /// // Returns false on macOS 14/15, true on macOS 26+.
    /// let _ = has_glass;
    /// ```
    #[must_use]
    pub fn detect_glass_effect_view_class() -> bool {
        AnyClass::get(c"NSGlassEffectView").is_some()
    }

    /// Allocates a new effect view (via `new` = `alloc] init]`) and returns
    /// it as a `+1`-retained raw pointer, or `None` if the class is missing.
    ///
    /// On macOS 26+, when the `NSGlassEffectView` class is registered (see
    /// [`detect_glass_effect_view_class`](Self::detect_glass_effect_view_class)),
    /// an `NSGlassEffectView` is created instead of an `NSVisualEffectView`.
    /// The glass view provides the Liquid Glass material natively, so the
    /// `setMaterial:` call in [`set_material`](BackdropController::set_material)
    /// is effectively a no-op for the glass view. On older macOS versions,
    /// this falls back to `NSVisualEffectView`.
    ///
    /// # Safety
    ///
    /// Must be called on the main thread (AppKit requirement). The returned
    /// pointer is `+1`-retained and must be released (e.g. via
    /// `Retained::from_raw`).
    unsafe fn create_effect_view() -> Option<*mut AnyObject> {
        // Prefer `NSGlassEffectView` (macOS 26+) when the class is
        // registered; fall back to `NSVisualEffectView` otherwise.
        let cls = class(c"NSGlassEffectView").or_else(|| class(c"NSVisualEffectView"))?;
        // SAFETY: `new` is equivalent to `alloc] init]` and returns a +1
        // retained instance. `NSVisualEffectView` is available on macOS
        // 10.10+; `NSGlassEffectView` is available on macOS 26+.
        let view: Retained<AnyObject> = unsafe { msg_send![cls, new] };
        Some(Retained::into_raw(view))
    }

    /// Attaches the effect view as the backmost subview of the window's
    /// content view so that the GPU layer (rendered on top) can composite
    /// over the vibrancy material.
    ///
    /// # Safety
    ///
    /// `ns_window` must be a valid `NSWindow*` and `effect_view` a valid
    /// `NSVisualEffectView*`. Must be called on the main thread.
    unsafe fn attach_effect_view(ns_window: *mut AnyObject, effect_view: *mut AnyObject) {
        // `contentView` returns the window's content view (autoreleased,
        // retained by `Retained`).
        let content_view: Option<Retained<AnyObject>> =
            unsafe { msg_send![ns_window, contentView] };
        let Some(content_view) = content_view else {
            return;
        };
        // Size the effect view to fill the content view and keep it
        // aligned on resize. `NSRect` is returned by value from `bounds`;
        // we pass it straight to `setFrame:`. The autoresizing mask
        // `NSViewWidthSizable | NSViewHeightSizable` (2 | 16 = 18) makes
        // the effect view track the content view's width and height.
        let bounds: NSRect = unsafe { msg_send![&content_view, bounds] };
        let _: () = unsafe { msg_send![effect_view, setFrame: bounds] };
        let mask: u64 = 18; // NSViewWidthSizable | NSViewHeightSizable
        let _: () = unsafe { msg_send![effect_view, setAutoresizingMask: mask] };
        // Place the effect view at the back of the subview z-order so the
        // GPU swapchain layer (added on top) composites over it.
        let _: () = unsafe {
            msg_send![
                &content_view,
                addSubview: effect_view,
                positioned: NS_WINDOW_ORDERING_MODE_BELOW,
                relativeTo: core::ptr::null_mut::<AnyObject>(),
            ]
        };
    }

    /// Removes the effect view from its superview and releases the
    /// controller's `+1` retain on it.
    fn remove_effect_view(&mut self) {
        if let Some(effect_view) = self.effect_view.take() {
            // SAFETY: `removeFromSuperview` is a no-op if the view has no
            // superview. `Retained::from_raw` reclaims the +1 retain and
            // dropping the `Retained` releases it.
            unsafe {
                let _: () = msg_send![effect_view, removeFromSuperview];
                let _ = Retained::from_raw(effect_view);
            }
        }
    }
}

impl Default for MacosBackdropController {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for MacosBackdropController {
    fn drop(&mut self) {
        // Release the owned effect view (this also removes it from its
        // superview, which is safe even if already detached).
        self.remove_effect_view();
    }
}

impl BackdropController for MacosBackdropController {
    fn set_material(&mut self, window: &dyn Window, material: BackdropMaterial) {
        // Short-circuit if the material hasn't changed.
        if self.material == material {
            return;
        }

        // For unsupported Windows-only materials, reset state.
        if matches!(
            material,
            BackdropMaterial::Mica | BackdropMaterial::MicaAlt | BackdropMaterial::Transient
        ) {
            self.remove_effect_view();
            self.material = BackdropMaterial::None;
            self.supported = false;
            return;
        }

        // Removing the material tears down the effect view entirely.
        if material == BackdropMaterial::None {
            self.remove_effect_view();
            self.material = BackdropMaterial::None;
            self.supported = true;
            return;
        }

        // Only vibrancy (and Acrylic, aliased to a vibrancy material) are
        // supported on macOS.
        let vibrancy = match material {
            BackdropMaterial::Vibrancy(v) => v,
            BackdropMaterial::Acrylic => VibrancyMaterial::HudWindow,
            // Unreachable: handled by the early returns above.
            BackdropMaterial::Mica
            | BackdropMaterial::MicaAlt
            | BackdropMaterial::Transient
            | BackdropMaterial::None => return,
        };

        // Validate handle BEFORE committing state.
        // Safety: the caller guarantees the window is alive for the
        // duration of this call.
        let ns_window = unsafe { window.raw_handle() } as *mut AnyObject;
        if ns_window.is_null() {
            self.remove_effect_view();
            self.material = BackdropMaterial::None;
            self.supported = false;
            return;
        }

        let ns_material = Self::vibrancy_to_ns_material(vibrancy) as i64;

        // Lazily create and attach the `NSVisualEffectView` on first use.
        // Only commit state on success.
        if self.effect_view.is_none() {
            // SAFETY: `set_material` is invoked from the window integration
            // layer on the main thread.
            let view = unsafe { Self::create_effect_view() };
            let Some(view) = view else {
                self.remove_effect_view();
                self.material = BackdropMaterial::None;
                self.supported = false;
                return;
            };
            self.effect_view = Some(view);
            // SAFETY: `ns_window` is a valid `NSWindow*` and `view` is a
            // freshly allocated `NSVisualEffectView*`.
            unsafe {
                Self::attach_effect_view(ns_window, view);
            }
        }

        if let Some(effect_view) = self.effect_view {
            // SAFETY: `effect_view` is a valid `NSVisualEffectView*`. All
            // three setters take an `NSInteger` enum argument and return
            // `void`.
            unsafe {
                let _: () = msg_send![effect_view, setMaterial: ns_material];
                let _: () = msg_send![
                    effect_view,
                    setBlendingMode: NS_VISUAL_EFFECT_BLENDING_MODE_BEHIND_WINDOW
                ];
                let _: () = msg_send![effect_view, setState: NS_VISUAL_EFFECT_STATE_ACTIVE];
            }
            // Commit state only after the view is configured successfully.
            self.material = material;
            self.supported = true;
        } else {
            // View creation/attachment failed; do not commit state.
            self.material = BackdropMaterial::None;
            self.supported = false;
        }
    }

    fn current_material(&self) -> BackdropMaterial {
        if self.supported {
            self.material
        } else {
            BackdropMaterial::None
        }
    }

    fn mode(&self) -> BackdropMode {
        match self.current_material() {
            BackdropMaterial::None | BackdropMaterial::Mica | BackdropMaterial::MicaAlt => {
                BackdropMode::Opaque
            }
            BackdropMaterial::Acrylic
            | BackdropMaterial::Transient
            | BackdropMaterial::Vibrancy(_) => BackdropMode::Transparent,
        }
    }

    fn supports_material(&self, material: BackdropMaterial) -> bool {
        // macOS supports Vibrancy (all variants) and Acrylic (as a
        // vibrancy alias). Mica/MicaAlt/Transient are Windows-only.
        match material {
            BackdropMaterial::Vibrancy(_) => true,
            BackdropMaterial::Acrylic => true, // Mapped to a vibrancy material
            BackdropMaterial::None => true,
            BackdropMaterial::Mica | BackdropMaterial::MicaAlt | BackdropMaterial::Transient => {
                false
            }
        }
    }
}

/// macOS appearance change observer.
///
/// Listens for `AppleInterfaceThemeChangedNotification` via
/// `NSDistributedNotificationCenter` and signals the theme system to
/// trigger a 150ms `ThemeDiff` transition. The current light/dark state
/// is queried on demand via [`is_dark_mode`](Self::is_dark_mode).
///
/// # Examples
///
/// ```no_run
/// use martensite_shell::platform_impl::macos::AppearanceObserver;
///
/// let mut observer = AppearanceObserver::new();
/// // Registers an NSDistributedNotificationCenter observer for
/// // "AppleInterfaceThemeChangedNotification".
/// observer.start();
/// ```
pub struct AppearanceObserver {
    /// Whether the observer is currently registered.
    active: bool,
    /// The `NSObject` observer token returned by
    /// `addObserverForName:object:queue:usingBlock:`, held as a `+1`-retained
    /// raw pointer. Used to remove the observer in [`stop`](Self::stop) /
    /// [`Drop`].
    observer_token: Option<*mut AnyObject>,
    /// Optional event queue onto which [`ShellEvent::ThemeAppearanceChanged`]
    /// is pushed when the system appearance notification fires. Cloned into
    /// the notification block so the block can emit events after
    /// [`start`](Self::start) returns.
    event_queue: Option<ShellEventQueue>,
}

impl AppearanceObserver {
    /// Creates a new appearance observer.
    ///
    /// The observer is initially inactive; call [`start`](Self::start) to
    /// register the distributed-notification observer. No event queue is
    /// attached, so appearance changes are not emitted as [`ShellEvent`]s.
    /// Use [`with_event_queue`](Self::with_event_queue) or
    /// [`set_event_queue`](Self::set_event_queue) to enable event emission.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_shell::platform_impl::macos::AppearanceObserver;
    ///
    /// let observer = AppearanceObserver::new();
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self {
            active: false,
            observer_token: None,
            event_queue: None,
        }
    }

    /// Creates a new appearance observer with a [`ShellEventQueue`] attached.
    ///
    /// When the system appearance changes, a
    /// [`ShellEvent::ThemeAppearanceChanged`] is pushed onto the queue so
    /// the window manager can trigger a theme transition.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_shell::platform_impl::macos::AppearanceObserver;
    /// use martensite_shell::{ShellEvent, ShellEventQueue};
    ///
    /// let queue = ShellEventQueue::new();
    /// let observer = AppearanceObserver::with_event_queue(queue.clone());
    /// // After `start()`, an appearance change pushes the event onto the
    /// // queue — drain it to observe:
    /// // assert_eq!(queue.drain(), vec![ShellEvent::ThemeAppearanceChanged]);
    /// let _ = observer;
    /// ```
    #[must_use]
    pub fn with_event_queue(queue: ShellEventQueue) -> Self {
        Self {
            active: false,
            observer_token: None,
            event_queue: Some(queue),
        }
    }

    /// Attaches or replaces the [`ShellEventQueue`] for event emission.
    ///
    /// After calling this, subsequent appearance-change notifications will
    /// push [`ShellEvent::ThemeAppearanceChanged`] onto the queue. If the
    /// observer is already active, the new queue takes effect on the next
    /// notification (the block captures the queue at registration time).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_shell::platform_impl::macos::AppearanceObserver;
    /// use martensite_shell::{ShellEvent, ShellEventQueue};
    ///
    /// let queue = ShellEventQueue::new();
    /// let mut observer = AppearanceObserver::new();
    /// observer.set_event_queue(queue.clone());
    /// // After `start()`, an appearance change pushes the event onto the
    /// // queue — drain it to observe:
    /// // assert_eq!(queue.drain(), vec![ShellEvent::ThemeAppearanceChanged]);
    /// ```
    pub fn set_event_queue(&mut self, queue: ShellEventQueue) {
        self.event_queue = Some(queue);
    }

    /// Returns `true` if the appearance is currently dark mode.
    ///
    /// Queries `NSApp.effectiveAppearance.name` and checks whether it
    /// matches `NSAppearanceNameDarkAqua`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_shell::platform_impl::macos::AppearanceObserver;
    ///
    /// let observer = AppearanceObserver::new();
    /// let _is_dark = observer.is_dark_mode();
    /// ```
    #[must_use]
    pub fn is_dark_mode(&self) -> bool {
        unsafe {
            let Some(app_cls) = class(c"NSApplication") else {
                return false;
            };
            // `sharedApplication` returns the shared `NSApplication`.
            let app: Option<Retained<AnyObject>> = msg_send![app_cls, sharedApplication];
            let Some(app) = app else {
                return false;
            };
            // `effectiveAppearance` returns the app's effective appearance.
            let appearance: Option<Retained<AnyObject>> = msg_send![&app, effectiveAppearance];
            let Some(appearance) = appearance else {
                return false;
            };
            // `name` returns the appearance name (an `NSString*`).
            let name: Option<Retained<AnyObject>> = msg_send![&appearance, name];
            let Some(name) = name else {
                return false;
            };
            ns_string_equals(&name, c"NSAppearanceNameDarkAqua")
        }
    }

    /// Starts observing appearance changes.
    ///
    /// Registers an observer for
    /// `AppleInterfaceThemeChangedNotification` on the distributed
    /// notification center. The observer token is stored for later
    /// removal by [`stop`](Self::stop). Calling `start` while already
    /// active is a no-op.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_shell::platform_impl::macos::AppearanceObserver;
    ///
    /// let mut observer = AppearanceObserver::new();
    /// observer.start();
    /// ```
    pub fn start(&mut self) {
        if self.active {
            return;
        }
        // SAFETY: `start` is invoked from the main thread by the window
        // integration layer. The block is copied to the heap by the
        // notification center, so the stack-local `StackBlock` is safe to
        // drop after the message send.
        let token = unsafe { self.register_observer() };
        self.observer_token = token;
        self.active = token.is_some();
    }

    /// Stops observing appearance changes.
    ///
    /// Removes the registered observer from the distributed notification
    /// center and releases the stored observer token. Calling `stop` while
    /// inactive is a no-op.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_shell::platform_impl::macos::AppearanceObserver;
    ///
    /// let mut observer = AppearanceObserver::new();
    /// observer.start();
    /// observer.stop();
    /// ```
    pub fn stop(&mut self) {
        if let Some(token) = self.observer_token.take() {
            // SAFETY: `token` is a valid observer object previously
            // returned by `addObserverForName:object:queue:usingBlock:`.
            unsafe {
                if let Some(cls) = class(c"NSDistributedNotificationCenter") {
                    let center: Retained<AnyObject> = msg_send![cls, defaultCenter];
                    let _: () = msg_send![&center, removeObserver: token];
                }
                // Reclaim the +1 retain on the token.
                let _ = Retained::from_raw(token);
            }
        }
        self.active = false;
    }

    /// Registers the distributed-notification observer and returns the
    /// `+1`-retained observer token, or `None` on failure.
    ///
    /// # Safety
    ///
    /// Must be called on the main thread.
    unsafe fn register_observer(&self) -> Option<*mut AnyObject> {
        let cls = class(c"NSDistributedNotificationCenter")?;
        // `defaultCenter` returns the shared distributed notification
        // center (a singleton).
        let center: Retained<AnyObject> = unsafe { msg_send![cls, defaultCenter] };
        let name = unsafe { ns_string(c"AppleInterfaceThemeChangedNotification") }?;

        // The block is invoked when the system appearance preference
        // changes. It clones the attached event queue (if any) and pushes
        // a `ThemeAppearanceChanged` event so the window manager can
        // trigger a theme transition. Callers also poll `is_dark_mode` to
        // detect the new state.
        let queue = self.event_queue.clone();
        let block = block2::StackBlock::new(move |_notification: *mut AnyObject| {
            if let Some(queue) = &queue {
                queue.push(ShellEvent::ThemeAppearanceChanged);
            }
        });
        let block_ptr = NonNull::from(&*block).as_ptr();

        // SAFETY: `addObserverForName:object:queue:usingBlock:` copies the
        // block (Block_copy) and returns a +1 observer token. Passing `nil`
        // for `object` and `queue` matches the documented "any sender" and
        // "main queue" semantics.
        let token: Option<Retained<AnyObject>> = unsafe {
            msg_send![
                &center,
                addObserverForName: Retained::as_ptr(&name) as *mut AnyObject,
                object: core::ptr::null_mut::<AnyObject>(),
                queue: core::ptr::null_mut::<AnyObject>(),
                usingBlock: block_ptr,
            ]
        };
        token.map(Retained::into_raw)
    }
}

impl Default for AppearanceObserver {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for AppearanceObserver {
    fn drop(&mut self) {
        self.stop();
    }
}
