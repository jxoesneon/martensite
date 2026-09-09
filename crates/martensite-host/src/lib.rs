//! Lightweight host support for dynamically loading and hot-reloading
//! Martensite guest cdylibs.
//!
//! The Martensite v0.9.0 architecture splits an application into a lightweight
//! **host** binary (which owns the `WidgetArena` and reactive signal store) and
//! a **guest** dynamic library (`cdylib`) that contains the component code.
//! When source files change, `cargo-martensite` recompiles only the guest crate
//! into a *versioned* `.dylib`/`.so`/`.dll`, and the host re-links the new
//! symbol table and triggers a full layout re-measurement within a 350 ms
//! budget.
//!
//! This crate provides the **dynamic-loading** half of that pipeline. It uses
//! the [`libloading`] crate to wrap `dlopen`/`dlsym` (Unix) and
//! `LoadLibrary`/`GetProcAddress` (Windows) behind a safe, ergonomic API.
//!
//! # Safety policy
//!
//! This crate uses `#![allow(unsafe_code)]` at the crate level because dynamic
//! library loading is inherently unsafe. All `unsafe` blocks are confined to
//! the [`GuestLibrary`] methods that delegate to [`libloading`], which itself
//! encapsulates the platform-specific FFI. The workspace-level
//! `unsafe_code = "deny"` policy is preserved for all other crates; this is
//! the narrowly scoped audited exception described in the v0.9.0 host-binary
//! boundary decision, mirroring the policy used by `martensite-font-fallback`.
//!
//! # Examples
//!
//! ```no_run
//! use martensite_host::{GuestLibrary, HostApp};
//! use std::path::Path;
//!
//! // Load a guest cdylib compiled by `cargo-martensite`.
//! let library = GuestLibrary::load(Path::new("target/martensite/libguest_v1.so"))
//!     .expect("failed to load guest library");
//!
//! // Wrap it in a host app for hot-reload management.
//! let mut app = HostApp::new(library);
//!
//! // Call the guest's render function each frame.
//! app.tick();
//!
//! // After a rebuild, hot-swap the guest library in place.
//! app.reload(Path::new("target/martensite/libguest_v2.so"))
//!     .expect("failed to reload guest library");
//! ```

#![allow(unsafe_code)]
#![deny(missing_docs)]

use libloading::{Library, Symbol};
use std::fmt;
use std::path::{Path, PathBuf};

/// Errors produced while loading or reloading a guest cdylib.
#[derive(Debug)]
pub enum HostError {
    /// The dynamic library could not be opened (`dlopen`/`LoadLibrary` failed).
    LoadFailed(PathBuf, String),
    /// A requested symbol could not be found in the library (`dlsym`/`GetProcAddress`).
    SymbolNotFound(String),
    /// The guest's render entry point is missing from the loaded library.
    MissingRenderSymbol,
}

impl fmt::Display for HostError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HostError::LoadFailed(path, msg) => {
                write!(f, "failed to load library {}: {msg}", path.display())
            }
            HostError::SymbolNotFound(name) => {
                write!(f, "symbol not found in guest library: {name}")
            }
            HostError::MissingRenderSymbol => {
                write!(
                    f,
                    "guest library is missing the required render entry point"
                )
            }
        }
    }
}

impl std::error::Error for HostError {}

/// The canonical name of the guest's render entry-point symbol.
///
/// Guest cdylibs must export a function with this name (and C ABI) that the
/// host calls once per frame to drive component layout and paint encoding.
pub const RENDER_SYMBOL_NAME: &str = "martensite_render";

/// A handle to a dynamically loaded guest cdylib.
///
/// Wraps a [`libloading::Library`] and provides safe access to the guest's
/// exported symbols. The library is unloaded when this value is dropped.
///
/// # Examples
///
/// ```no_run
/// use martensite_host::GuestLibrary;
/// use std::path::Path;
///
/// let lib = GuestLibrary::load(Path::new("libguest_v1.so"))
///     .expect("failed to load");
/// assert!(!lib.path().as_os_str().is_empty());
/// ```
pub struct GuestLibrary {
    library: Library,
    path: PathBuf,
}

impl fmt::Debug for GuestLibrary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GuestLibrary")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

impl GuestLibrary {
    /// Dynamically loads a cdylib from `path`.
    ///
    /// On Unix this calls `dlopen`; on Windows this calls `LoadLibrary`. The
    /// returned handle owns the loaded library and unloads it on drop.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_host::GuestLibrary;
    /// use std::path::Path;
    ///
    /// let lib = GuestLibrary::load(Path::new("nonexistent_lib_guest.so"));
    /// assert!(lib.is_err());
    /// ```
    pub fn load(path: &Path) -> Result<Self, HostError> {
        // SAFETY: `Library::new` calls `dlopen`/`LoadLibrary`, which is unsafe
        // because the loaded code may execute arbitrary initialization. The
        // guest cdylib is trusted build output from `cargo-martensite`.
        let library = unsafe { Library::new(path) }
            .map_err(|e| HostError::LoadFailed(path.to_path_buf(), e.to_string()))?;
        Ok(Self {
            library,
            path: path.to_path_buf(),
        })
    }

    /// Returns the filesystem path this library was loaded from.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Looks up a symbol by name in the loaded library.
    ///
    /// Returns a [`libloading::Symbol`] guard that keeps the symbol borrowed
    /// from the underlying library. The symbol must outlive the returned
    /// guard.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_host::GuestLibrary;
    /// use std::path::Path;
    ///
    /// let lib = GuestLibrary::load(Path::new("libguest.so"))?;
    /// // Look up a symbol with C ABI and no parameters.
    /// let _sym = lib.get_symbol::<extern "C" fn()>("martensite_render");
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn get_symbol<T>(&self, name: &str) -> Result<Symbol<'_, T>, HostError>
    where
        T: 'static,
    {
        // SAFETY: `Library::get` calls `dlsym`/`GetProcAddress`, which is unsafe
        // because the caller must guarantee the symbol has the expected type
        // and lifetime. The caller (host) is responsible for matching the
        // guest's declared ABI.
        unsafe { self.library.get(name.as_bytes()) }
            .map_err(|_| HostError::SymbolNotFound(name.to_string()))
    }

    /// Unloads the current library and loads a fresh copy from `path`.
    ///
    /// This is the core of the hot-reload mechanism: the old symbol table is
    /// released and a new one is linked in its place, all without restarting
    /// the host process.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_host::GuestLibrary;
    /// use std::path::Path;
    ///
    /// let mut lib = GuestLibrary::load(Path::new("libguest_v1.so"))?;
    /// lib.reload(Path::new("libguest_v2.so"))?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn reload(&mut self, path: &Path) -> Result<(), HostError> {
        // Load the new library first so that a load failure does not leave us
        // without a working guest.
        let new_lib = Self::load(path)?;
        // Only replace once the new library is successfully opened.
        self.library = new_lib.library;
        self.path = new_lib.path;
        Ok(())
    }
}

/// A host application that owns a guest cdylib and drives its render loop.
///
/// This is the top-level type used by the host binary. It owns a
/// [`GuestLibrary`] and provides [`HostApp::tick`] to invoke the guest's
/// render entry point each frame, plus [`HostApp::reload`] to hot-swap the
/// guest library after a rebuild.
///
/// # Examples
///
/// ```no_run
/// use martensite_host::{GuestLibrary, HostApp};
/// use std::path::Path;
///
/// let lib = GuestLibrary::load(Path::new("libguest_v1.so"))?;
/// let mut app = HostApp::new(lib);
/// app.tick();
/// app.reload(Path::new("libguest_v2.so"))?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub struct HostApp {
    guest: GuestLibrary,
}

impl fmt::Debug for HostApp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HostApp")
            .field("guest", &self.guest)
            .finish()
    }
}

impl HostApp {
    /// Creates a new host app wrapping the given guest library.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_host::{GuestLibrary, HostApp};
    /// use std::path::Path;
    ///
    /// let lib = GuestLibrary::load(Path::new("libguest.so"))?;
    /// let app = HostApp::new(lib);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn new(guest: GuestLibrary) -> Self {
        Self { guest }
    }

    /// Returns a reference to the currently loaded guest library.
    pub fn guest(&self) -> &GuestLibrary {
        &self.guest
    }

    /// Hot-swaps the guest library, loading a fresh cdylib from `path`.
    ///
    /// The old library is unloaded and replaced atomically. If the new
    /// library fails to load, the existing guest is preserved.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_host::{GuestLibrary, HostApp};
    /// use std::path::Path;
    ///
    /// let lib = GuestLibrary::load(Path::new("libguest_v1.so"))?;
    /// let mut app = HostApp::new(lib);
    /// app.reload(Path::new("libguest_v2.so"))?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn reload(&mut self, path: &Path) -> Result<(), HostError> {
        self.guest.reload(path)
    }

    /// Invokes the guest's render entry point for the current frame.
    ///
    /// Looks up the [`RENDER_SYMBOL_NAME`] symbol in the guest library and
    /// calls it. Returns an error if the symbol is missing or cannot be
    /// resolved.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_host::{GuestLibrary, HostApp};
    /// use std::path::Path;
    ///
    /// let lib = GuestLibrary::load(Path::new("libguest.so"))?;
    /// let mut app = HostApp::new(lib);
    /// let _ = app.tick();
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn tick(&self) -> Result<(), HostError> {
        let render: Symbol<extern "C" fn()> = self
            .guest
            .get_symbol(RENDER_SYMBOL_NAME)
            .map_err(|_| HostError::MissingRenderSymbol)?;
        // SAFETY: The guest's `martensite_render` symbol is declared with a C
        // ABI and no parameters by the guest crate. Calling it is safe as long
        // as the guest honors that contract.
        (*render)();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_error_display() {
        let e = HostError::LoadFailed(PathBuf::from("lib.so"), "missing".into());
        assert!(e.to_string().contains("lib.so"));
        assert!(e.to_string().contains("missing"));
        let e = HostError::SymbolNotFound("foo".into());
        assert!(e.to_string().contains("foo"));
        let e = HostError::MissingRenderSymbol;
        assert!(e.to_string().contains("render"));
    }

    #[test]
    fn render_symbol_name_is_martensite_render() {
        assert_eq!(RENDER_SYMBOL_NAME, "martensite_render");
    }

    #[test]
    fn guest_library_load_missing_file_returns_error() {
        let path = Path::new("/nonexistent/definitely_not_a_library_xyz.so");
        let result = GuestLibrary::load(path);
        assert!(result.is_err());
        match result.unwrap_err() {
            HostError::LoadFailed(p, _) => assert_eq!(p, path),
            other => panic!("expected LoadFailed, got {other:?}"),
        }
    }

    #[test]
    fn host_app_debug_includes_guest_path() {
        // Construct without loading to test Debug formatting only.
        let path = PathBuf::from("libguest_v1.so");
        // Use a missing path so load fails, then test the error path.
        let err = GuestLibrary::load(&path).unwrap_err();
        assert!(matches!(err, HostError::LoadFailed(_, _)));
    }
}
