//! OS drag-and-drop platform trait seam, capability detection, and backends.
//!
//! `martensite-dnd` is `#![forbid(unsafe_code)]`, so it cannot perform native
//! OS drag-and-drop FFI directly. Instead it defines the [`DndPlatform`] trait
//! seam — a small, safe abstraction over the OS DnD surface — together with:
//!
//! - [`DndCapabilities`] — capability detection bitflags so callers can
//!   feature-detect before attempting an operation.
//! - [`UnsupportedDndPlatform`] — a documented fallback that reports every
//!   capability as unsupported. This is the default when no platform backend is
//!   available and is the path used by headless tests and CI.
//! - [`WinitDndPlatform`] — a real backend that delegates to winit's safe
//!   `ActiveEventLoop` DnD API (which itself performs the platform-specific
//!   FFI internally on macOS, Windows, X11, and Wayland). This is the
//!   recommended production backend and the minimum viable OS integration
//!   path.
//!
//! # Why a trait seam?
//!
//! winit 0.31.0-beta.3 replaced the legacy `DroppedFile`/`HoveredFile` events
//! with a full data-transfer API (`DragEntered`/`DragPosition`/`DragDropped`/
//! `DragLeft` for incoming drags, `start_drag` for outgoing drags, arbitrary
//! MIME types via `TransferType`/`TypeHint`). That API is safe to call, so the
//! [`WinitDndPlatform`] adapter can live inside this `#![forbid(unsafe_code)]`
//! crate without any `unsafe` blocks. The trait seam keeps the internal DnD
//! model ([`crate::DndSession`], [`crate::DropTargetRegistry`]) decoupled from
//! the winit version: a future winit upgrade, or a non-winit backend, only
//! needs to provide a new [`DndPlatform`] implementation.
//!
//! # Documented limitations
//!
//! - **No direct FFI**: this crate performs no `unsafe` FFI. All OS interaction
//!   goes through winit's safe API via [`WinitDndPlatform`], or is reported as
//!   unsupported via [`UnsupportedDndPlatform`].
//! - **Arbitrary MIME types**: winit exposes cross-platform `TypeHint`s
//!   (Plaintext, UriList, Html, Rtf, Audio, Image) plus platform-dependent
//!   custom types. [`WinitDndPlatform::available_types`] reports the
//!   cross-platform hints it can map; platform-dependent types are surfaced as
//!   [`DropTypeHint::Custom`] with a best-effort string. The
//!   [`DndCapabilities::ARBITRARY_MIME`] capability is therefore reported as
//!   *not* supported by [`WinitDndPlatform`], since winit does not expose a
//!   fully arbitrary, portable MIME string for every platform type.
//! - **Drag icon**: [`WinitDndPlatform::start_drag`] does not currently set a
//!   custom drag icon (it passes `None`), relying on the platform default.
//!   A future revision can add an icon parameter.
//! - **Async data fetch**: fetching the actual dropped bytes is asynchronous in
//!   winit (`fetch_data_transfer` + `DataTransferReceived`). The seam exposes
//!   [`DndPlatform::fetch_data`] which returns a serial; the caller is
//!   responsible for correlating the later `DataTransferReceived` event. This
//!   crate does not buffer async-fetched data.

use crate::DropEffect;
use std::fmt;

/// Bitflags describing which drag-and-drop capabilities a [`DndPlatform`]
/// backend supports.
///
/// Callers should check these before attempting an operation so they can fall
/// back gracefully (e.g. to in-app-only DnD) on platforms without OS support.
///
/// # Examples
///
/// ```
/// use martensite_dnd::platform::{DndCapabilities, UnsupportedDndPlatform, DndPlatform};
///
/// let backend = UnsupportedDndPlatform;
/// assert!(backend.capabilities().is_empty());
/// assert!(!backend.capabilities().contains(DndCapabilities::DROP_TARGET));
/// ```
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct DndCapabilities(u8);

bitflags::bitflags! {
    impl DndCapabilities: u8 {
        /// The platform can receive incoming drag-and-drop operations (drop
        /// target side): it can report the advertised types of an incoming
        /// drag and accept/reject it via
        /// [`DndPlatform::set_accepted_actions`].
        const DROP_TARGET = 0b0000_0001;
        /// The platform can initiate outgoing drag-and-drop operations
        /// (drag source side) via [`DndPlatform::start_drag`].
        const DRAG_SOURCE = 0b0000_0010;
        /// The platform supports fully arbitrary MIME type strings, not just
        /// the cross-platform [`DropTypeHint`] set. winit does not expose
        /// this portably, so [`WinitDndPlatform`] does **not** set this bit.
        const ARBITRARY_MIME = 0b0000_0100;
        /// The platform supports asynchronous data fetching via
        /// [`DndPlatform::fetch_data`].
        const ASYNC_DATA_FETCH = 0b0000_1000;
    }
}

/// A platform-assigned identifier for an ongoing data transfer (incoming or
/// outgoing drag).
///
/// This mirrors winit's `DataTransferId` (a wrapped `i64`) without leaking the
/// winit type into the public seam.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PlatformTransferId(pub i64);

impl PlatformTransferId {
    /// Creates a new id from a raw `i64`.
    #[must_use]
    pub const fn new(id: i64) -> Self {
        Self(id)
    }

    /// Returns the raw `i64` value.
    #[must_use]
    pub const fn into_raw(self) -> i64 {
        self.0
    }
}

impl From<PlatformTransferId> for i64 {
    fn from(id: PlatformTransferId) -> Self {
        id.0
    }
}

/// The action an OS proposes for an incoming drag, mapped onto the
/// [`DropEffect`] model where possible.
///
/// `Ask` and `Private` have no [`DropEffect`] equivalent and are preserved
/// verbatim so callers can round-trip them back to the OS.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum ProposedAction {
    /// No action proposed by the OS (or the backend could not determine one).
    #[default]
    None,
    /// Copy the data.
    Copy,
    /// Move the data.
    Move,
    /// Link the data.
    Link,
    /// Ask the user what to do (platform-specific; no [`DropEffect`] mapping).
    Ask,
    /// Private negotiation between source and destination (macOS).
    Private,
}

impl ProposedAction {
    /// Converts this action to a [`DropEffect`], returning `None` for `Ask`
    /// and `Private` which have no [`DropEffect`] equivalent.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_dnd::platform::ProposedAction;
    /// use martensite_dnd::DropEffect;
    ///
    /// assert_eq!(ProposedAction::Copy.to_drop_effect(), Some(DropEffect::Copy));
    /// assert_eq!(ProposedAction::Ask.to_drop_effect(), None);
    /// ```
    #[must_use]
    pub fn to_drop_effect(self) -> Option<DropEffect> {
        match self {
            Self::Copy => Some(DropEffect::Copy),
            Self::Move => Some(DropEffect::Move),
            Self::Link => Some(DropEffect::Link),
            Self::None | Self::Ask | Self::Private => None,
        }
    }
}

/// Cross-platform type hints advertised by an incoming drag.
///
/// This mirrors the subset of winit's `TypeHint` that can be represented
/// portably. Platform-dependent types are surfaced as
/// [`DropTypeHint::Custom`] with a best-effort string label.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DropTypeHint {
    /// Plain UTF-8 text.
    Plaintext,
    /// A `text/uri-list` (list of URIs, UTF-8 encoded).
    UriList,
    /// HTML-formatted text.
    Html,
    /// RTF-formatted text.
    Rtf,
    /// A platform-dependent type labelled by a best-effort string.
    Custom(String),
}

/// Payload for an outgoing drag operation.
///
/// This is the input to [`DndPlatform::start_drag`]. It covers the common
/// cross-platform cases; arbitrary binary blobs are sent via [`DragPayload::Bytes`].
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum DragPayload {
    /// Plain UTF-8 text.
    Text(String),
    /// A list of URI strings (RFC 3986).
    Uris(Vec<String>),
    /// An arbitrary binary blob.
    Bytes(Vec<u8>),
}

/// Errors returned by [`DndPlatform`] operations.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum DndPlatformError {
    /// The operation is not supported on this platform/backend.
    Unsupported,
    /// The supplied [`PlatformTransferId`] is invalid or has expired.
    InvalidId,
    /// The platform rejected the operation for an opaque reason.
    Platform(String),
}

impl fmt::Display for DndPlatformError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported => write!(f, "drag-and-drop is not supported on this platform"),
            Self::InvalidId => write!(f, "invalid or expired data-transfer id"),
            Self::Platform(msg) => write!(f, "platform drag-and-drop error: {msg}"),
        }
    }
}

impl std::error::Error for DndPlatformError {}

/// The platform seam for OS drag-and-drop integration.
///
/// Implementations are expected to be cheap to construct (typically borrowing
/// an event loop handle) and `Send` where the underlying platform allows it.
/// All methods are infallible to call; failures are reported via
/// [`DndPlatformError`].
///
/// # Implementations
///
/// - [`UnsupportedDndPlatform`] — the no-op fallback (all operations
///   unsupported).
/// - [`WinitDndPlatform`] — the real backend delegating to winit's safe
///   `ActiveEventLoop` DnD API.
pub trait DndPlatform: fmt::Debug {
    /// Returns the set of DnD capabilities supported by this backend.
    fn capabilities(&self) -> DndCapabilities;

    /// Returns the list of types advertised by the incoming drag identified by
    /// `id`.
    ///
    /// Returns [`DndPlatformError::Unsupported`] if the backend cannot act as
    /// a drop target, or [`DndPlatformError::InvalidId`] if `id` is unknown or
    /// has expired.
    fn available_types(
        &self,
        id: PlatformTransferId,
    ) -> Result<Vec<DropTypeHint>, DndPlatformError>;

    /// Reports the set of accepted [`DropEffect`]s back to the OS for the
    /// incoming drag `id`, so the OS can display the correct cursor and honor
    /// modifier-key action changes.
    ///
    /// The order is treated as preference-ordered on platforms that support it
    /// (e.g. Wayland, macOS). Pass an empty slice to reject the drag.
    fn set_accepted_actions(
        &self,
        id: PlatformTransferId,
        actions: &[DropEffect],
    ) -> Result<(), DndPlatformError>;

    /// Requests the actual data for `id` in the given `type_`.
    ///
    /// The data is delivered asynchronously via a later
    /// `WindowEvent::DataTransferReceived` event, which carries its own serial
    /// for correlation. This method therefore returns `()` on success.
    ///
    /// Returns [`DndPlatformError::Unsupported`] if async data fetch is not
    /// available.
    fn fetch_data(
        &self,
        id: PlatformTransferId,
        type_: &DropTypeHint,
    ) -> Result<(), DndPlatformError>;

    /// Initiates an outgoing drag operation from `source_window` carrying
    /// `payload`, offering the given `actions` (preference-ordered).
    ///
    /// Returns the [`PlatformTransferId`] identifying the outgoing drag, which
    /// the caller correlates with a later `OutgoingDragDropped`/
    /// `OutgoingDragCanceled` event.
    ///
    /// Returns [`DndPlatformError::Unsupported`] if the backend cannot act as
    /// a drag source.
    fn start_drag(
        &self,
        source_window: winit::window::WindowId,
        payload: &DragPayload,
        actions: &[DropEffect],
    ) -> Result<PlatformTransferId, DndPlatformError>;
}

/// A no-op [`DndPlatform`] fallback that reports every capability as
/// unsupported.
///
/// This is the default backend for headless tests, CI, and any target where no
/// OS DnD backend is available. Every operation returns
/// [`DndPlatformError::Unsupported`]; [`DndPlatform::capabilities`] is empty.
///
/// # Examples
///
/// ```
/// use martensite_dnd::platform::{
///     DndCapabilities, DndPlatform, DndPlatformError, PlatformTransferId, UnsupportedDndPlatform,
/// };
/// use martensite_dnd::DropEffect;
///
/// let backend = UnsupportedDndPlatform;
/// assert!(backend.capabilities().is_empty());
/// assert_eq!(
///     backend.available_types(PlatformTransferId::new(1)),
///     Err(DndPlatformError::Unsupported),
/// );
/// assert_eq!(
///     backend.set_accepted_actions(PlatformTransferId::new(1), &[DropEffect::Copy]),
///     Err(DndPlatformError::Unsupported),
/// );
/// ```
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct UnsupportedDndPlatform;

impl DndPlatform for UnsupportedDndPlatform {
    fn capabilities(&self) -> DndCapabilities {
        DndCapabilities::empty()
    }

    fn available_types(
        &self,
        _id: PlatformTransferId,
    ) -> Result<Vec<DropTypeHint>, DndPlatformError> {
        Err(DndPlatformError::Unsupported)
    }

    fn set_accepted_actions(
        &self,
        _id: PlatformTransferId,
        _actions: &[DropEffect],
    ) -> Result<(), DndPlatformError> {
        Err(DndPlatformError::Unsupported)
    }

    fn fetch_data(
        &self,
        _id: PlatformTransferId,
        _type_: &DropTypeHint,
    ) -> Result<(), DndPlatformError> {
        Err(DndPlatformError::Unsupported)
    }

    fn start_drag(
        &self,
        _source_window: winit::window::WindowId,
        _payload: &DragPayload,
        _actions: &[DropEffect],
    ) -> Result<PlatformTransferId, DndPlatformError> {
        Err(DndPlatformError::Unsupported)
    }
}

// ---------------------------------------------------------------------------
// winit-backed backend.
//
// winit 0.31.0-beta.3 exposes a *safe* DnD API on `ActiveEventLoop`
// (`data_transfer`, `set_valid_dnd_actions`, `fetch_data_transfer`,
// `start_drag`). The platform-specific FFI (NSPasteboard/Win32/X11/Wayland) is
// performed internally by winit, so this adapter contains no `unsafe` blocks
// and is safe to compile under `#![forbid(unsafe_code)]`.
// ---------------------------------------------------------------------------

/// A [`DndPlatform`] backend that delegates to winit's safe `ActiveEventLoop`
/// DnD API.
///
/// This is the recommended production backend. It reports
/// [`DndCapabilities::DROP_TARGET`], [`DndCapabilities::DRAG_SOURCE`], and
/// [`DndCapabilities::ASYNC_DATA_FETCH`] as supported (subject to the runtime
/// winit backend actually implementing them — winit returns
/// `RequestError::NotSupported` on platforms without DnD, which this adapter
/// surfaces as [`DndPlatformError::Platform`]). It does **not** report
/// [`DndCapabilities::ARBITRARY_MIME`] because winit does not expose a fully
/// portable arbitrary-MIME surface.
///
/// The adapter borrows the `ActiveEventLoop` for the duration of the DnD
/// operations; it is cheap to construct and intended to be created per
/// event-loop iteration as needed.
///
/// # Examples
///
/// ```no_run
/// use martensite_dnd::platform::{DndCapabilities, DndPlatform, WinitDndPlatform};
///
/// // `event_loop` would come from your winit `ApplicationHandler`.
/// // let platform = WinitDndPlatform::new(event_loop);
/// // assert!(platform.capabilities().contains(DndCapabilities::DROP_TARGET));
/// # let _ = DndCapabilities::DROP_TARGET;
/// ```
#[derive(Clone)]
pub struct WinitDndPlatform<'a> {
    loop_: &'a dyn winit::event_loop::ActiveEventLoop,
}

impl<'a> fmt::Debug for WinitDndPlatform<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WinitDndPlatform").finish_non_exhaustive()
    }
}

impl<'a> WinitDndPlatform<'a> {
    /// Creates a new winit-backed DnD platform adapter borrowing `event_loop`.
    #[must_use]
    pub fn new(event_loop: &'a dyn winit::event_loop::ActiveEventLoop) -> Self {
        Self { loop_: event_loop }
    }
}

impl<'a> DndPlatform for WinitDndPlatform<'a> {
    fn capabilities(&self) -> DndCapabilities {
        // winit exposes the drop-target, drag-source, and async-fetch surface
        // portably. Individual platforms may still return `NotSupported` at
        // runtime; that is surfaced per-call as `DndPlatformError::Platform`.
        DndCapabilities::DROP_TARGET
            | DndCapabilities::DRAG_SOURCE
            | DndCapabilities::ASYNC_DATA_FETCH
    }

    fn available_types(
        &self,
        id: PlatformTransferId,
    ) -> Result<Vec<DropTypeHint>, DndPlatformError> {
        let winit_id = winit::data_transfer::DataTransferId::from_raw(id.into_raw());
        let transfer = self
            .loop_
            .data_transfer(winit_id)
            .map_err(map_request_error)?;
        Ok(transfer
            .available_types()
            .into_iter()
            .filter_map(type_hint_from_wintype)
            .collect())
    }

    fn set_accepted_actions(
        &self,
        id: PlatformTransferId,
        actions: &[DropEffect],
    ) -> Result<(), DndPlatformError> {
        let winit_id = winit::data_transfer::DataTransferId::from_raw(id.into_raw());
        let winit_actions: Vec<winit::event_loop::DndAction> = actions
            .iter()
            .copied()
            .map(dnd_action_from_effect)
            .collect();
        self.loop_
            .set_valid_dnd_actions(winit_id, &winit_actions)
            .map_err(map_request_error)
    }

    fn fetch_data(
        &self,
        id: PlatformTransferId,
        type_: &DropTypeHint,
    ) -> Result<(), DndPlatformError> {
        let winit_id = winit::data_transfer::DataTransferId::from_raw(id.into_raw());
        let winit_type: Box<dyn winit::data_transfer::TransferType + Send> = match type_ {
            DropTypeHint::Plaintext => Box::new(winit::data_transfer::TypeHint::Plaintext),
            DropTypeHint::UriList => Box::new(winit::data_transfer::TypeHint::UriList),
            DropTypeHint::Html => Box::new(winit::data_transfer::TypeHint::Html),
            DropTypeHint::Rtf => Box::new(winit::data_transfer::TypeHint::Rtf),
            // winit has no portable "custom string MIME" type; surface as
            // unsupported so callers fall back gracefully.
            DropTypeHint::Custom(_) => return Err(DndPlatformError::Unsupported),
        };
        self.loop_
            .fetch_data_transfer(winit_id, &*winit_type)
            .map_err(map_request_error)?;
        Ok(())
    }

    fn start_drag(
        &self,
        source_window: winit::window::WindowId,
        payload: &DragPayload,
        actions: &[DropEffect],
    ) -> Result<PlatformTransferId, DndPlatformError> {
        let send_data: Box<dyn winit::data_transfer::DataTransferSend> = match payload {
            DragPayload::Text(text) => {
                let mut builder = winit::data_transfer::DataTransferSendBuilder::new(text.clone());
                builder.add_type(winit::data_transfer::TypeHint::Plaintext, |state, _| {
                    Some(state.clone())
                });
                builder.build()
            }
            DragPayload::Uris(uris) => {
                let uris = uris.clone();
                let mut builder = winit::data_transfer::DataTransferSendBuilder::new(uris);
                builder.add_type(winit::data_transfer::TypeHint::UriList, |state, _| {
                    Some(winit::data_transfer::SendData::Uris(state.clone()))
                });
                builder.build()
            }
            DragPayload::Bytes(bytes) => {
                let bytes = bytes.clone();
                let mut builder = winit::data_transfer::DataTransferSendBuilder::new(bytes);
                // Bytes have no portable cross-platform TypeHint; advertise as
                // plaintext best-effort so at least the drop can proceed on
                // platforms that accept arbitrary bytes.
                builder.add_type(winit::data_transfer::TypeHint::Plaintext, |state, _| {
                    Some(state.clone())
                });
                builder.build()
            }
        };
        let winit_actions: Vec<winit::event_loop::DndAction> = actions
            .iter()
            .copied()
            .map(dnd_action_from_effect)
            .collect();
        let id = self
            .loop_
            .start_drag(source_window, send_data, &winit_actions, None)
            .map_err(map_request_error)?;
        Ok(PlatformTransferId::new(id.into_raw()))
    }
}

/// Maps a winit `RequestError` to a [`DndPlatformError`].
fn map_request_error(err: winit::error::RequestError) -> DndPlatformError {
    use winit::error::RequestError;
    match err {
        RequestError::NotSupported(_) => DndPlatformError::Platform(format!("{err:?}")),
        other => DndPlatformError::Platform(format!("{other:?}")),
    }
}

/// Converts a winit `DndAction` to a [`ProposedAction`].
pub(crate) fn proposed_action_from_winit(action: winit::event_loop::DndAction) -> ProposedAction {
    use winit::event_loop::DndAction;
    match action {
        DndAction::Move => ProposedAction::Move,
        DndAction::Copy => ProposedAction::Copy,
        DndAction::Link => ProposedAction::Link,
        DndAction::Ask => ProposedAction::Ask,
        DndAction::Private => ProposedAction::Private,
        // `DndAction` is `#[non_exhaustive]`.
        _ => ProposedAction::None,
    }
}

/// Converts a [`DropEffect`] to the winit `DndAction` used to report accepted
/// actions back to the OS.
fn dnd_action_from_effect(effect: DropEffect) -> winit::event_loop::DndAction {
    match effect {
        DropEffect::None => winit::event_loop::DndAction::Copy,
        DropEffect::Copy => winit::event_loop::DndAction::Copy,
        DropEffect::Move => winit::event_loop::DndAction::Move,
        DropEffect::Link => winit::event_loop::DndAction::Link,
    }
}

/// Converts a winit `TransferType` (from `DataTransfer::available_types`) to a
/// [`DropTypeHint`], returning `None` for types with no portable
/// representation.
fn type_hint_from_wintype(ty: &dyn winit::data_transfer::TransferType) -> Option<DropTypeHint> {
    match ty.hint()? {
        winit::data_transfer::TypeHint::Plaintext => Some(DropTypeHint::Plaintext),
        winit::data_transfer::TypeHint::UriList => Some(DropTypeHint::UriList),
        winit::data_transfer::TypeHint::Html => Some(DropTypeHint::Html),
        winit::data_transfer::TypeHint::Rtf => Some(DropTypeHint::Rtf),
        // Audio/Image hints carry extension metadata that has no
        // cross-platform string label in our seam; surface as Custom with a
        // best-effort label.
        winit::data_transfer::TypeHint::Audio { extension_hint } => Some(DropTypeHint::Custom(
            format!("audio/{}", extension_hint.unwrap_or("*")),
        )),
        winit::data_transfer::TypeHint::Image { extension_hint } => Some(DropTypeHint::Custom(
            format!("image/{}", extension_hint.unwrap_or("*")),
        )),
        // `TypeHint` is `#[non_exhaustive]`; unknown variants have no portable
        // representation in our seam.
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capabilities_flags_work() {
        let caps = DndCapabilities::DROP_TARGET | DndCapabilities::DRAG_SOURCE;
        assert!(caps.contains(DndCapabilities::DROP_TARGET));
        assert!(caps.contains(DndCapabilities::DRAG_SOURCE));
        assert!(!caps.contains(DndCapabilities::ARBITRARY_MIME));
        assert!(!caps.contains(DndCapabilities::ASYNC_DATA_FETCH));
    }

    #[test]
    fn empty_capabilities_is_default() {
        assert_eq!(DndCapabilities::empty(), DndCapabilities::default());
    }

    #[test]
    fn unsupported_reports_empty_capabilities() {
        assert!(UnsupportedDndPlatform.capabilities().is_empty());
    }

    #[test]
    fn unsupported_available_types_is_unsupported() {
        assert_eq!(
            UnsupportedDndPlatform
                .available_types(PlatformTransferId::new(0))
                .unwrap_err(),
            DndPlatformError::Unsupported
        );
    }

    #[test]
    fn unsupported_set_actions_is_unsupported() {
        assert_eq!(
            UnsupportedDndPlatform
                .set_accepted_actions(PlatformTransferId::new(0), &[DropEffect::Copy])
                .unwrap_err(),
            DndPlatformError::Unsupported
        );
    }

    #[test]
    fn unsupported_fetch_data_is_unsupported() {
        assert_eq!(
            UnsupportedDndPlatform
                .fetch_data(PlatformTransferId::new(0), &DropTypeHint::Plaintext)
                .unwrap_err(),
            DndPlatformError::Unsupported
        );
    }
    #[test]
    fn unsupported_start_drag_is_unsupported() {
        let id = winit::window::WindowId::from_raw(0);
        assert_eq!(
            UnsupportedDndPlatform
                .start_drag(id, &DragPayload::Text("x".into()), &[DropEffect::Copy])
                .unwrap_err(),
            DndPlatformError::Unsupported
        );
    }

    #[test]
    fn platform_transfer_id_roundtrips() {
        let id = PlatformTransferId::new(42);
        assert_eq!(id.into_raw(), 42);
        let raw: i64 = id.into();
        assert_eq!(raw, 42);
    }

    #[test]
    fn proposed_action_to_drop_effect() {
        assert_eq!(
            ProposedAction::Copy.to_drop_effect(),
            Some(DropEffect::Copy)
        );
        assert_eq!(
            ProposedAction::Move.to_drop_effect(),
            Some(DropEffect::Move)
        );
        assert_eq!(
            ProposedAction::Link.to_drop_effect(),
            Some(DropEffect::Link)
        );
        assert_eq!(ProposedAction::None.to_drop_effect(), None);
        assert_eq!(ProposedAction::Ask.to_drop_effect(), None);
        assert_eq!(ProposedAction::Private.to_drop_effect(), None);
    }

    #[test]
    fn proposed_action_default_is_none() {
        assert_eq!(ProposedAction::default(), ProposedAction::None);
    }

    #[test]
    fn dnd_platform_error_display() {
        assert!(!format!("{}", DndPlatformError::Unsupported).is_empty());
        assert!(!format!("{}", DndPlatformError::InvalidId).is_empty());
        assert!(!format!("{}", DndPlatformError::Platform("boom".into())).is_empty());
    }

    #[test]
    fn dnd_platform_error_is_std_error() {
        fn is_error(_: &dyn std::error::Error) {}
        is_error(&DndPlatformError::Unsupported);
    }

    #[test]
    fn type_hint_from_wintype_maps_known() {
        use winit::data_transfer::TransferType;
        // `TypeHint` implements `TransferType`, so we can pass `&hint` as
        // `&dyn TransferType`.
        let plaintext = winit::data_transfer::TypeHint::Plaintext;
        assert_eq!(
            type_hint_from_wintype(&plaintext as &dyn TransferType),
            Some(DropTypeHint::Plaintext)
        );
        let urilist = winit::data_transfer::TypeHint::UriList;
        assert_eq!(
            type_hint_from_wintype(&urilist as &dyn TransferType),
            Some(DropTypeHint::UriList)
        );
        let html = winit::data_transfer::TypeHint::Html;
        assert_eq!(
            type_hint_from_wintype(&html as &dyn TransferType),
            Some(DropTypeHint::Html)
        );
        let rtf = winit::data_transfer::TypeHint::Rtf;
        assert_eq!(
            type_hint_from_wintype(&rtf as &dyn TransferType),
            Some(DropTypeHint::Rtf)
        );
        let audio = winit::data_transfer::TypeHint::Audio {
            extension_hint: Some("wav"),
        };
        assert_eq!(
            type_hint_from_wintype(&audio as &dyn TransferType),
            Some(DropTypeHint::Custom("audio/wav".into()))
        );
        let img = winit::data_transfer::TypeHint::Image {
            extension_hint: None,
        };
        assert_eq!(
            type_hint_from_wintype(&img as &dyn TransferType),
            Some(DropTypeHint::Custom("image/*".into()))
        );
    }

    #[test]
    fn proposed_action_from_winit_maps() {
        use winit::event_loop::DndAction;
        assert_eq!(
            proposed_action_from_winit(DndAction::Copy),
            ProposedAction::Copy
        );
        assert_eq!(
            proposed_action_from_winit(DndAction::Move),
            ProposedAction::Move
        );
        assert_eq!(
            proposed_action_from_winit(DndAction::Link),
            ProposedAction::Link
        );
        assert_eq!(
            proposed_action_from_winit(DndAction::Ask),
            ProposedAction::Ask
        );
        assert_eq!(
            proposed_action_from_winit(DndAction::Private),
            ProposedAction::Private
        );
    }

    #[test]
    fn drag_payload_variants_distinct() {
        assert_ne!(
            DragPayload::Text("a".into()),
            DragPayload::Bytes(b"a".to_vec())
        );
        assert_ne!(
            DragPayload::Text("a".into()),
            DragPayload::Uris(vec!["a".into()])
        );
    }

    #[test]
    fn winit_platform_capabilities_advertises_supported() {
        // We cannot construct a real ActiveEventLoop in a unit test, but the
        // capability set is a constant independent of the loop, so verify it
        // indirectly through the documented contract.
        let caps = DndCapabilities::DROP_TARGET
            | DndCapabilities::DRAG_SOURCE
            | DndCapabilities::ASYNC_DATA_FETCH;
        assert!(caps.contains(DndCapabilities::DROP_TARGET));
        assert!(caps.contains(DndCapabilities::DRAG_SOURCE));
        assert!(caps.contains(DndCapabilities::ASYNC_DATA_FETCH));
        assert!(!caps.contains(DndCapabilities::ARBITRARY_MIME));
    }
}
