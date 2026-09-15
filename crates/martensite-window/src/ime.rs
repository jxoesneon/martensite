//! Input-method-editor (IME) integration via winit's `request_ime_update`
//! path.
//!
//! On desktop platforms the IME composes text for CJK and other complex
//! input methods. On **mobile** the same winit API drives the on-screen
//! keyboard:
//!
//! - **iOS:** [`enable_ime`] calls `becomeFirstResponder` on the winit
//!   `UIView`, which summons the software keyboard; [`disable_ime`] calls
//!   `resignFirstResponder`. Text arrives through the normal
//!   [`WindowEvent::Ime`] / `KeyboardInput` event stream.
//!   [`ImeRequestData::cursor_area`] is **unsupported** on iOS (the system
//!   positions the keyboard itself); only `hint_and_purpose` and
//!   `surrounding_text` may be enabled.
//! - **Android:** the same request path toggles the soft input method.
//! - **Desktop:** the standard IME candidate-window contract applies.
//!
//! The helpers here are thin wrappers over
//! [`Window::request_ime_update`]
//! that keep callers free of the `ImeRequest`/`ImeEnableRequest`
//! boilerplate while preserving winit's capability contract.
//!
//! [`WindowEvent::Ime`]: winit::event::WindowEvent::Ime

use winit::window::{
    ImeCapabilities, ImeEnableRequest, ImeRequest, ImeRequestData, ImeRequestError, Window,
};

/// Re-export of winit's IME request types so callers can build requests
/// without a direct `winit` dependency edge for these names.
pub use winit::window::{ImeHint, ImePurpose, ImeSurroundingText};

/// Returns the [`ImeCapabilities`] the window's IME was enabled with, or
/// `None` when IME input is currently disabled for `window`.
///
/// # Examples
///
/// ```no_run
/// # fn scope(window: &dyn winit::window::Window) {
/// use martensite_window::ime;
///
/// if ime::ime_capabilities(window).is_none() {
///     // IME is not enabled — the on-screen keyboard is hidden on mobile.
/// }
/// # }
/// ```
#[must_use]
pub fn ime_capabilities(window: &dyn Window) -> Option<ImeCapabilities> {
    window.ime_capabilities()
}

/// Enables IME input on `window` with the given `capabilities` and initial
/// `data`.
///
/// On iOS this calls `becomeFirstResponder` on the window's `UIView`,
/// summoning the software keyboard; subsequent keystrokes and preedit
/// arrive as [`WindowEvent::Ime`] events. `data` must be consistent with
/// `capabilities` — every enabled capability must have its corresponding
/// field populated in `data` (see [`ImeEnableRequest::new`]).
///
/// # Errors
///
/// - [`ImeRequestError::NotSupported`] if `capabilities` and `data` are
///   inconsistent (a capability enabled without matching data, or data
///   supplied for a disabled capability).
/// - [`ImeRequestError::AlreadyEnabled`] if IME is already enabled on
///   `window` (disable it first to change capabilities).
/// - [`ImeRequestError::NotEnabled`] is not returned by this function.
///
/// [`WindowEvent::Ime`]: winit::event::WindowEvent::Ime
///
/// # Examples
///
/// ```no_run
/// # fn scope(window: &dyn winit::window::Window) {
/// use martensite_window::ime;
/// use winit::window::{ImeCapabilities, ImeHint, ImePurpose, ImeRequestData};
///
/// let caps = ImeCapabilities::new().with_hint_and_purpose();
/// let data = ImeRequestData::default()
///     .with_hint_and_purpose(ImeHint::NONE, ImePurpose::Normal);
/// ime::enable_ime(window, caps, data).expect("IME enabled");
/// # }
/// ```
pub fn enable_ime(
    window: &dyn Window,
    capabilities: ImeCapabilities,
    data: ImeRequestData,
) -> Result<(), ImeRequestError> {
    let request = ImeEnableRequest::new(capabilities, data).ok_or(ImeRequestError::NotSupported)?;
    window.request_ime_update(ImeRequest::Enable(request))
}

/// Updates the state of an already-enabled IME on `window`.
///
/// Fields left `None` in `data` keep their previously sent values. Fields
/// for capabilities that were not enabled via [`enable_ime`] are ignored
/// by the platform.
///
/// # Errors
///
/// - [`ImeRequestError::NotEnabled`] if IME has not been enabled on
///   `window` yet.
/// - [`ImeRequestError::NotSupported`] if the platform rejects the update.
///
/// # Examples
///
/// ```no_run
/// # fn scope(window: &dyn winit::window::Window) {
/// use martensite_window::ime;
/// use winit::window::ImeRequestData;
///
/// // Report the caret's surroundings so the IME can refine suggestions.
/// let data = ImeRequestData::default().with_surrounding_text(
///     winit::window::ImeSurroundingText::new("hello".to_string(), 5, 5)
///         .expect("cursor within text"),
/// );
/// let _ = ime::update_ime(window, data);
/// # }
/// ```
pub fn update_ime(window: &dyn Window, data: ImeRequestData) -> Result<(), ImeRequestError> {
    window.request_ime_update(ImeRequest::Update(data))
}

/// Disables IME input on `window`.
///
/// On iOS this calls `resignFirstResponder`, dismissing the software
/// keyboard. The disable request cannot fail, so this function returns
/// nothing; winit emits a [`WindowEvent::Ime::Disabled`] event when the
/// platform acknowledges it.
///
/// [`WindowEvent::Ime::Disabled`]: winit::event::Ime::Disabled
///
/// # Examples
///
/// ```no_run
/// # fn scope(window: &dyn winit::window::Window) {
/// martensite_window::ime::disable_ime(window);
/// # }
/// ```
pub fn disable_ime(window: &dyn Window) {
    // `ImeRequest::Disable` is documented as infallible; swallow the
    // result deliberately rather than surfacing a spurious error type.
    let _ = window.request_ime_update(ImeRequest::Disable);
}
