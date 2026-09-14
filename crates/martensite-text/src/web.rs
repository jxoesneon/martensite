//! Web (`wasm32-unknown-unknown`) font loading.
//!
//! **There are no system fonts on the web.** `fontdb`'s
//! `load_system_fonts` finds nothing on `wasm32-unknown-unknown` (there is
//! no filesystem of OS font directories to scan), so
//! [`FontSystem::new()`]/[`FontManager::new()`] produce an empty font
//! database. Every glyph must come from fonts the application supplies:
//!
//! * **Bundled** — bytes compiled into the wasm binary with
//!   `include_bytes!` and registered through
//!   [`fontdb::Source::Binary`]. Use [`bundled_font_manager`] or
//!   [`load_bundled_fonts`].
//! * **Fetched** — bytes downloaded at runtime with `fetch` and then
//!   registered the same way. Use [`fetch_font`] +
//!   [`FontManager::load_font_data`], or the combined
//!   [`fetch_and_load_font`].
//!
//! Both paths funnel into [`FontManager::load_font_data`], which parses
//! via swash inside `catch_unwind`, so a malformed download cannot abort
//! the wasm module.
//!
//! COOP/COEP note: under `Cross-Origin-Embedder-Policy: require-corp`,
//! cross-origin font URLs must carry a `Cross-Origin-Resource-Policy`
//! response header or the fetch is blocked — same-origin or bundled fonts
//! avoid this entirely.
//!
//! # Examples
//!
//! ```
//! use martensite_text::web::bundled_font_manager;
//!
//! // `include_bytes!("../fonts/Inter-Regular.ttf")` in a real app.
//! let manager = bundled_font_manager(&[]);
//! let _ = manager.faces().len();
//! ```

use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

use crate::font::{FontId, FontManager};

/// Error type for web font fetching.
///
/// # Examples
///
/// ```
/// use martensite_text::web::WebFontError;
///
/// let err = WebFontError::Fetch("404".to_string());
/// assert!(err.to_string().contains("404"));
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WebFontError {
    /// No `window` object is available (e.g. running outside a DOM
    /// context).
    NoWindow,
    /// The `fetch` promise rejected, or the response was not `ok`.
    Fetch(String),
    /// Reading the response body (`arrayBuffer`) failed.
    Body(String),
}

impl std::fmt::Display for WebFontError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoWindow => write!(f, "no `window` object available for fetch"),
            Self::Fetch(msg) => write!(f, "font fetch failed: {msg}"),
            Self::Body(msg) => write!(f, "reading font response body failed: {msg}"),
        }
    }
}

impl std::error::Error for WebFontError {}

fn js_err(value: &wasm_bindgen::JsValue) -> String {
    value
        .as_string()
        .or_else(|| value.dyn_ref::<js_sys::Error>().map(|e| e.message().into()))
        .unwrap_or_else(|| format!("{value:?}"))
}

/// Creates a [`FontManager`] whose entire font database is the given
/// bundled font blobs — the web analogue of
/// [`FontManager::with_fonts`].
///
/// Each entry is registered as a [`fontdb::Source::Binary`]; a single
/// blob may contain multiple faces (TTC). Malformed blobs are skipped
/// without panicking.
///
/// # Examples
///
/// ```
/// use martensite_text::web::bundled_font_manager;
///
/// // In a real app: `bundled_font_manager(&[include_bytes!("font.ttf")])`
/// let manager = bundled_font_manager(&[]);
/// let _ = manager.faces();
/// ```
#[must_use]
pub fn bundled_font_manager(fonts: &[&'static [u8]]) -> FontManager {
    let mut manager = FontManager::with_fonts(std::iter::empty());
    load_bundled_fonts(&mut manager, fonts);
    manager
}

/// Registers each bundled font blob in `manager` via
/// [`fontdb::Source::Binary`], returning the new [`FontId`]s.
///
/// This is a convenience wrapper over [`FontManager::load_font_data`] for
/// `include_bytes!`-style `'static` slices; it shares that method's
/// `catch_unwind` hardening against malformed font data.
///
/// # Examples
///
/// ```
/// use martensite_text::{FontManager, web::load_bundled_fonts};
///
/// let mut manager = FontManager::with_fonts(std::iter::empty());
/// // Malformed data is skipped rather than panicking.
/// let ids = load_bundled_fonts(&mut manager, &[&[0u8; 4]]);
/// let _ = ids.len();
/// ```
pub fn load_bundled_fonts(manager: &mut FontManager, fonts: &[&'static [u8]]) -> Vec<FontId> {
    let mut ids = Vec::new();
    for blob in fonts {
        ids.extend(manager.load_font_data(*blob));
    }
    ids
}

/// Downloads a font file with `window.fetch` and returns its bytes.
///
/// The returned bytes are meant for [`FontManager::load_font_data`] (or
/// [`fontdb::Source::Binary`]); combine the two via
/// [`fetch_and_load_font`].
///
/// # Errors
///
/// [`WebFontError::NoWindow`] when no DOM `window` exists,
/// [`WebFontError::Fetch`] when the fetch rejects or returns a non-`ok`
/// status, [`WebFontError::Body`] when the `arrayBuffer` read fails.
///
/// # Examples
///
/// ```no_run
/// async fn example() {
///     let bytes = martensite_text::web::fetch_font("fonts/Inter.ttf").await;
///     let _ = bytes;
/// }
/// ```
pub async fn fetch_font(url: &str) -> Result<Vec<u8>, WebFontError> {
    let window = web_sys::window().ok_or(WebFontError::NoWindow)?;
    let response_value = JsFuture::from(window.fetch_with_str(url))
        .await
        .map_err(|e| WebFontError::Fetch(js_err(&e)))?;
    let response: web_sys::Response = response_value
        .dyn_into()
        .map_err(|e| WebFontError::Fetch(format!("response is not a Response: {}", js_err(&e))))?;
    if !response.ok() {
        return Err(WebFontError::Fetch(format!(
            "HTTP {} fetching {url}",
            response.status()
        )));
    }
    let buffer = JsFuture::from(
        response
            .array_buffer()
            .map_err(|e| WebFontError::Body(js_err(&e)))?,
    )
    .await
    .map_err(|e| WebFontError::Body(js_err(&e)))?;
    Ok(js_sys::Uint8Array::new(&buffer).to_vec())
}

/// Downloads a font with [`fetch_font`] and registers it in `manager`,
/// returning the new [`FontId`]s.
///
/// # Errors
///
/// Propagates [`fetch_font`]'s [`WebFontError`].
///
/// # Examples
///
/// ```no_run
/// async fn example(manager: &mut martensite_text::FontManager) {
///     let ids = martensite_text::web::fetch_and_load_font(manager, "fonts/Inter.ttf").await;
///     let _ = ids;
/// }
/// ```
pub async fn fetch_and_load_font(
    manager: &mut FontManager,
    url: &str,
) -> Result<Vec<FontId>, WebFontError> {
    let bytes = fetch_font(url).await?;
    Ok(manager.load_font_data(bytes))
}
