//! Dual-mode Virtual File System for Martensite assets.
//!
//! The VFS abstracts over two asset backends so that the same asset-loading
//! code works transparently in development and in shipped binaries:
//!
//! - [`DiskVfs`] resolves assets from the workspace filesystem and watches for
//!   changes on a background thread, emitting invalidation signals that drive
//!   hot-reloading in debug builds.
//! - [`EmbeddedVfs`] resolves assets from `&'static` memory slices baked into
//!   the binary at compile time (e.g. via `include_bytes!`), achieving
//!   single-digit microsecond resolution latency with zero copying via an
//!   O(log n) binary search over a path-sorted index.
//!
//! Both backends implement the common [`Vfs`] trait so callers can be written
//! once against the trait and swap backends via [`VfsBackend`].
//!
//! # Examples
//!
//! ```
//! use martensite_assets::vfs::{EmbeddedVfs, Vfs};
//!
//! static ASSETS: &[(&str, &[u8])] = &[
//!     ("shaders/triangle.wgsl", b"@vertex fn vs() -> @builtin(position) vec4<f32> { return vec4<f32>(0.0); }"),
//!     ("data/greeting.txt", b"hello"),
//! ];
//!
//! let vfs = EmbeddedVfs::new(ASSETS);
//! assert!(vfs.exists("data/greeting.txt"));
//! assert_eq!(vfs.resolve("data/greeting.txt"), Some(&b"hello"[..]));
//! ```

use core::fmt;

// ============================================================================
// Vfs trait
// ============================================================================

/// A read-only virtual file system for resolving Martensite assets.
///
/// Both the disk-backed ([`DiskVfs`]) and embedded ([`EmbeddedVfs`]) backends
/// implement this trait. Resolved data is borrowed directly from the backend's
/// internal storage, so embedded resolution is zero-copy.
///
/// # Examples
///
/// ```
/// use martensite_assets::vfs::{EmbeddedVfs, Vfs};
///
/// static TABLE: &[(&str, &[u8])] = &[("a.txt", b"alpha")];
/// let vfs = EmbeddedVfs::new(TABLE);
/// assert_eq!(vfs.resolve("a.txt"), Some(&b"alpha"[..]));
/// assert_eq!(vfs.list(), vec!["a.txt".to_string()]);
/// ```
pub trait Vfs: Send + Sync {
    /// Resolve `path` to its byte contents, returning a borrowed slice.
    ///
    /// Returns `None` if no asset is registered at `path`.
    fn resolve(&self, path: &str) -> Option<&[u8]>;

    /// Returns `true` if an asset is registered at `path`.
    fn exists(&self, path: &str) -> bool;

    /// Lists every asset path known to this backend, in unspecified order.
    fn list(&self) -> Vec<String>;

    /// Resolve `path` to an [`AssetHandle`] bundling the path and its bytes.
    ///
    /// This is a convenience wrapper over [`Vfs::resolve`]; the default
    /// implementation is provided so backends need not override it.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_assets::vfs::{EmbeddedVfs, Vfs};
    ///
    /// static TABLE: &[(&str, &[u8])] = &[("a.txt", b"alpha")];
    /// let vfs = EmbeddedVfs::new(TABLE);
    /// let handle = vfs.resolve_handle("a.txt").unwrap();
    /// assert_eq!(handle.path, "a.txt");
    /// assert_eq!(handle.data, b"alpha");
    /// ```
    fn resolve_handle<'a>(&'a self, path: &'a str) -> Option<AssetHandle<'a>> {
        self.resolve(path).map(|data| AssetHandle { path, data })
    }
}

// ============================================================================
// AssetHandle
// ============================================================================

/// A borrowed handle to a resolved asset, pairing its path with the byte slice
/// returned by the VFS.
///
/// Produced by [`Vfs::resolve_handle`]. The handle borrows both the lookup path
/// and the backing storage, so it is cheap to construct and copy.
///
/// # Examples
///
/// ```
/// use martensite_assets::vfs::{EmbeddedVfs, Vfs};
///
/// static TABLE: &[(&str, &[u8])] = &[("config.toml", b"key = 1")];
/// let vfs = EmbeddedVfs::new(TABLE);
/// let handle = vfs.resolve_handle("config.toml").unwrap();
/// assert_eq!(handle.path, "config.toml");
/// assert_eq!(handle.data, b"key = 1");
/// ```
#[derive(Debug, Clone, Copy)]
pub struct AssetHandle<'a> {
    /// The asset path that produced this handle.
    pub path: &'a str,
    /// The resolved byte contents.
    pub data: &'a [u8],
}

impl<'a> AsRef<[u8]> for AssetHandle<'a> {
    fn as_ref(&self) -> &[u8] {
        self.data
    }
}

// ============================================================================
// AssetPath
// ============================================================================

/// Error returned when an [`AssetPath`] fails validation.
///
/// # Examples
///
/// ```
/// use martensite_assets::vfs::{AssetPath, AssetPathError};
///
/// assert!(matches!(AssetPath::new("../escape"), Err(AssetPathError::ParentTraversal)));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssetPathError {
    /// The path was empty.
    Empty,
    /// The path was absolute (assets are always relative to the VFS root).
    Absolute,
    /// The path used backslashes; only forward-slash separators are accepted.
    Backslash,
    /// The path contained a `..` component, which would escape the VFS root.
    ParentTraversal,
}

impl fmt::Display for AssetPathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "asset path is empty"),
            Self::Absolute => write!(f, "asset path must be relative, not absolute"),
            Self::Backslash => write!(f, "asset path must use '/' separators, not '\\'"),
            Self::ParentTraversal => write!(f, "asset path may not contain '..' components"),
        }
    }
}

impl std::error::Error for AssetPathError {}

/// A validated, normalized, relative asset path.
///
/// Asset paths always use forward-slash separators, are never absolute, and
/// never contain `..` components, so they cannot escape the VFS root. This
/// makes [`AssetPath`] safe to use as a lookup key across both backends.
///
/// # Examples
///
/// ```
/// use martensite_assets::vfs::AssetPath;
///
/// let path = AssetPath::new("shaders/triangle.wgsl").unwrap();
/// assert_eq!(path.as_str(), "shaders/triangle.wgsl");
///
/// let joined = path.join("main.wgsl").unwrap();
/// assert_eq!(joined.as_str(), "shaders/triangle.wgsl/main.wgsl");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AssetPath {
    inner: String,
}

impl AssetPath {
    /// Create a validated asset path from a string-like input.
    ///
    /// Leading `./` prefixes are stripped. The path must be relative, use
    /// forward slashes, and contain no `..` components.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_assets::vfs::{AssetPath, AssetPathError};
    ///
    /// assert_eq!(AssetPath::new("./a/b.txt").unwrap().as_str(), "a/b.txt");
    /// assert!(matches!(AssetPath::new("/abs"), Err(AssetPathError::Absolute)));
    /// ```
    pub fn new(path: impl Into<String>) -> Result<Self, AssetPathError> {
        let mut inner = path.into();
        if inner.is_empty() {
            return Err(AssetPathError::Empty);
        }
        // Strip a leading "./" prefix for normalization.
        while inner.starts_with("./") {
            inner = inner[2..].to_string();
        }
        if inner.is_empty() {
            return Err(AssetPathError::Empty);
        }
        if inner.starts_with('/') {
            return Err(AssetPathError::Absolute);
        }
        if inner.contains('\\') {
            return Err(AssetPathError::Backslash);
        }
        for component in inner.split('/') {
            if component == ".." {
                return Err(AssetPathError::ParentTraversal);
            }
        }
        Ok(Self { inner })
    }

    /// Returns the path as a string slice.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_assets::vfs::AssetPath;
    /// let p = AssetPath::new("a/b.txt").unwrap();
    /// assert_eq!(p.as_str(), "a/b.txt");
    /// ```
    pub fn as_str(&self) -> &str {
        &self.inner
    }

    /// Append a `segment` to this path, producing a new validated [`AssetPath`].
    ///
    /// `segment` is validated with the same rules as [`AssetPath::new`].
    ///
    /// This is **slash-concatenation**, not real filesystem navigation: the
    /// segment is appended after a single `/` separator and the combined
    /// string is re-validated. It does not resolve `.`/`..` components (and
    /// `..` is rejected outright by validation), nor does it collapse
    /// duplicate separators. Use it to build child asset paths from a known
    /// parent, not to perform path algebra.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_assets::vfs::AssetPath;
    /// let p = AssetPath::new("dir").unwrap();
    /// assert_eq!(p.join("file.txt").unwrap().as_str(), "dir/file.txt");
    /// ```
    pub fn join(&self, segment: &str) -> Result<Self, AssetPathError> {
        let combined = format!("{}/{}", self.inner, segment);
        Self::new(combined)
    }

    /// Returns the parent directory of this path, or `None` if it has none.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_assets::vfs::AssetPath;
    /// let p = AssetPath::new("dir/file.txt").unwrap();
    /// assert_eq!(p.parent().unwrap().as_str(), "dir");
    /// assert!(AssetPath::new("file.txt").unwrap().parent().is_none());
    /// ```
    pub fn parent(&self) -> Option<Self> {
        let idx = self.inner.rfind('/')?;
        let parent = &self.inner[..idx];
        if parent.is_empty() {
            None
        } else {
            Some(Self {
                inner: parent.to_string(),
            })
        }
    }
}

impl fmt::Display for AssetPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.inner)
    }
}

impl AsRef<str> for AssetPath {
    fn as_ref(&self) -> &str {
        &self.inner
    }
}

// ============================================================================
// EmbeddedVfs
// ============================================================================

/// A zero-copy VFS backed by a `&'static` table of `(path, bytes)` pairs.
///
/// Intended for release builds where assets are baked into the binary via
/// `include_bytes!` and assembled into a static slice. At construction time
/// the table is indexed into a path-sorted view so that [`Vfs::resolve`] and
/// [`Vfs::exists`] perform an O(log n) binary search instead of a linear scan,
/// keeping resolution in the single-digit microsecond range even for large
/// tables. Lookups return references directly into the static data, so no
/// allocation or copying occurs after construction.
///
/// # Examples
///
/// ```
/// use martensite_assets::vfs::{EmbeddedVfs, Vfs};
///
/// static ASSETS: &[(&str, &[u8])] = &[
///     ("ui/theme.toml", b"primary = \"#0f0\""),
///     ("ui/logo.png", &[0x89, 0x50, 0x4e, 0x47]),
/// ];
///
/// let vfs = EmbeddedVfs::new(ASSETS);
/// assert_eq!(vfs.resolve("ui/theme.toml"), Some(&b"primary = \"#0f0\""[..]));
/// assert_eq!(vfs.list().len(), 2);
/// ```
pub struct EmbeddedVfs {
    table: &'static [(&'static str, &'static [u8])],
    /// Path-sorted copy of `table` used for O(log n) binary-search lookups.
    sorted: Vec<(&'static str, &'static [u8])>,
}

impl EmbeddedVfs {
    /// Create an embedded VFS from a static asset table.
    ///
    /// The table is borrowed for the lifetime of the program (`'static`), so
    /// the returned [`EmbeddedVfs`] is freely shareable and resolution is
    /// zero-copy. A path-sorted index is built once at construction time so
    /// that subsequent [`Vfs::resolve`] / [`Vfs::exists`] calls are O(log n)
    /// binary searches rather than linear scans.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_assets::vfs::{EmbeddedVfs, Vfs};
    /// static T: &[(&str, &[u8])] = &[("x", b"y")];
    /// let vfs = EmbeddedVfs::new(T);
    /// assert!(vfs.exists("x"));
    /// ```
    pub fn new(table: &'static [(&'static str, &'static [u8])]) -> Self {
        let mut sorted: Vec<(&'static str, &'static [u8])> = table.to_vec();
        // Stable sort by path so the binary-search index is deterministic.
        sorted.sort_by(|a, b| a.0.cmp(b.0));
        Self { table, sorted }
    }

    /// Returns the number of assets in the embedded table.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_assets::vfs::EmbeddedVfs;
    /// static T: &[(&str, &[u8])] = &[("a", b"1"), ("b", b"2")];
    /// assert_eq!(EmbeddedVfs::new(T).len(), 2);
    /// ```
    pub const fn len(&self) -> usize {
        self.table.len()
    }

    /// Returns `true` if the embedded table contains no assets.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_assets::vfs::EmbeddedVfs;
    /// static EMPTY: &[(&str, &[u8])] = &[];
    /// assert!(EmbeddedVfs::new(EMPTY).is_empty());
    /// ```
    pub const fn is_empty(&self) -> bool {
        self.table.is_empty()
    }
}

impl Vfs for EmbeddedVfs {
    fn resolve(&self, path: &str) -> Option<&[u8]> {
        // O(log n) binary search over the path-sorted index built in `new`.
        self.sorted
            .binary_search_by(|(p, _)| p.cmp(&path))
            .ok()
            .map(|idx| self.sorted[idx].1)
    }

    fn exists(&self, path: &str) -> bool {
        self.sorted.binary_search_by(|(p, _)| p.cmp(&path)).is_ok()
    }

    fn list(&self) -> Vec<String> {
        self.table.iter().map(|(p, _)| (*p).to_string()).collect()
    }
}

impl fmt::Debug for EmbeddedVfs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EmbeddedVfs")
            .field("asset_count", &self.table.len())
            .finish()
    }
}

// ============================================================================
// DiskVfs (behind the "disk" feature)
// ============================================================================

#[cfg(feature = "disk")]
mod disk {
    use super::Vfs;
    use notify::{event::EventKind, RecommendedWatcher, RecursiveMode, Watcher};
    use parking_lot::Mutex;
    use std::collections::{HashMap, HashSet};
    use std::fmt;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;

    /// Maximum number of invalidation paths retained in the queue before
    /// the oldest entries are dropped.
    const MAX_INVALIDATIONS: usize = 1024;

    /// A bounded, deduplicated queue of invalidated asset paths.
    ///
    /// The file watcher appends every event path here. Paths are
    /// deduplicated so repeated events for the same file do not bloat
    /// the queue, and the queue is capped at [`MAX_INVALIDATIONS`]
    /// entries. When the cap is reached, the oldest entry is dropped
    /// and the `dropped` counter is incremented.
    struct InvalidationQueue {
        paths: Vec<String>,
        seen: HashSet<String>,
        dropped: u64,
    }

    impl InvalidationQueue {
        fn new() -> Self {
            Self {
                paths: Vec::new(),
                seen: HashSet::new(),
                dropped: 0,
            }
        }

        /// Appends `path` to the queue if it is not already present.
        /// When the queue is at capacity, the oldest entry is removed
        /// (and its `seen` entry cleared) before the new one is added.
        fn push(&mut self, path: String) {
            if self.seen.contains(&path) {
                return;
            }
            if self.paths.len() >= MAX_INVALIDATIONS {
                if let Some(oldest) = self.paths.first().cloned() {
                    self.paths.remove(0);
                    self.seen.remove(&oldest);
                    self.dropped += 1;
                    tracing::warn!(
                        path = %oldest,
                        dropped_count = self.dropped,
                        "DiskVfs invalidation queue full; dropping oldest entry"
                    );
                }
            }
            self.seen.insert(path.clone());
            self.paths.push(path);
        }

        /// Drains and returns all queued paths, resetting the dedup set.
        fn drain(&mut self) -> Vec<String> {
            self.seen.clear();
            core::mem::take(&mut self.paths)
        }
    }

    /// Internal state shared between the [`DiskVfs`] and its background watcher.
    struct WatchState {
        root: Arc<PathBuf>,
        version: Arc<AtomicU64>,
        invalidated: Arc<Mutex<InvalidationQueue>>,
    }

    /// A filesystem-backed VFS for development builds.
    ///
    /// `DiskVfs` eagerly loads every file under a root directory into an
    /// immutable in-memory cache at construction time, so that [`Vfs::resolve`]
    /// can return borrowed slices without holding a lock. A background file
    /// watcher (powered by the `notify` crate) observes the root for
    /// modifications and bumps an invalidation version counter, which callers
    /// can poll to drive hot-reloading.
    ///
    /// Because the cache is immutable for the lifetime of the `DiskVfs`, pick
    /// up file changes by rebuilding the VFS (see [`DiskVfs::version`] and
    /// [`DiskVfs::take_invalidations`]).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_assets::vfs::{DiskVfs, Vfs};
    /// # fn main() -> std::io::Result<()> {
    /// let dir = tempfile::tempdir()?;
    /// std::fs::write(dir.path().join("hello.txt"), b"hi")?;
    ///
    /// let vfs = DiskVfs::new(dir.path()).unwrap();
    /// assert_eq!(vfs.resolve("hello.txt"), Some(&b"hi"[..]));
    /// # Ok(())
    /// # }
    /// ```
    pub struct DiskVfs {
        root: PathBuf,
        cache: HashMap<String, Box<[u8]>>,
        version: Arc<AtomicU64>,
        invalidated: Arc<Mutex<InvalidationQueue>>,
        // The watcher must be kept alive for the lifetime of the VFS; dropping
        // it stops the background watch thread.
        _watcher: Option<RecommendedWatcher>,
    }

    impl DiskVfs {
        /// Create a `DiskVfs` over `root`, eagerly loading all files and
        /// starting a background file watcher.
        ///
        /// Paths in the cache are stored relative to `root` using forward-slash
        /// separators. If the watcher fails to start, the VFS is still
        /// returned with watching disabled (the error is ignored), so asset
        /// resolution always works even on platforms without native watchers.
        ///
        /// # Examples
        ///
        /// ```no_run
        /// use martensite_assets::vfs::{DiskVfs, Vfs};
        /// # fn main() -> std::io::Result<()> {
        /// let dir = tempfile::tempdir()?;
        /// std::fs::write(dir.path().join("a.txt"), b"a")?;
        /// let vfs = DiskVfs::new(dir.path()).unwrap();
        /// assert!(vfs.exists("a.txt"));
        /// # Ok(())
        /// # }
        /// ```
        pub fn new(root: impl AsRef<Path>) -> std::io::Result<Self> {
            let root = root.as_ref().to_path_buf();
            let cache = load_directory(&root)?;
            let version = Arc::new(AtomicU64::new(0));
            let invalidated: Arc<Mutex<InvalidationQueue>> =
                Arc::new(Mutex::new(InvalidationQueue::new()));
            let root_arc = Arc::new(root.clone());

            let watcher = {
                let state = WatchState {
                    root: Arc::clone(&root_arc),
                    version: Arc::clone(&version),
                    invalidated: Arc::clone(&invalidated),
                };
                try_start_watcher(state)
            };

            Ok(Self {
                root,
                cache,
                version,
                invalidated,
                _watcher: watcher,
            })
        }

        /// Create a `DiskVfs` without starting a background file watcher.
        ///
        /// Useful in tests where file watching is unnecessary or would race
        /// with assertions. The cache is still eagerly populated from disk.
        ///
        /// # Examples
        ///
        /// ```no_run
        /// use martensite_assets::vfs::{DiskVfs, Vfs};
        /// # fn main() -> std::io::Result<()> {
        /// let dir = tempfile::tempdir()?;
        /// std::fs::write(dir.path().join("a.txt"), b"a")?;
        /// let vfs = DiskVfs::without_watcher(dir.path()).unwrap();
        /// assert_eq!(vfs.resolve("a.txt"), Some(&b"a"[..]));
        /// # Ok(())
        /// # }
        /// ```
        pub fn without_watcher(root: impl AsRef<Path>) -> std::io::Result<Self> {
            let root = root.as_ref().to_path_buf();
            let cache = load_directory(&root)?;
            Ok(Self {
                root,
                cache,
                version: Arc::new(AtomicU64::new(0)),
                invalidated: Arc::new(Mutex::new(InvalidationQueue::new())),
                _watcher: None,
            })
        }

        /// Returns the absolute root directory this VFS resolves against.
        ///
        /// # Examples
        ///
        /// ```no_run
        /// use martensite_assets::vfs::DiskVfs;
        /// # fn main() -> std::io::Result<()> {
        /// let dir = tempfile::tempdir()?;
        /// let vfs = DiskVfs::without_watcher(dir.path()).unwrap();
        /// assert_eq!(vfs.root(), dir.path());
        /// # Ok(())
        /// # }
        /// ```
        pub fn root(&self) -> &Path {
            &self.root
        }

        /// Returns the current invalidation version.
        ///
        /// The version starts at `0` and is bumped (by at least one) every
        /// time the background watcher observes a file modification, creation,
        /// or removal under the root. Callers can compare versions across
        /// polls to detect that assets have changed and a rebuild is needed.
        ///
        /// # Examples
        ///
        /// ```no_run
        /// use martensite_assets::vfs::DiskVfs;
        /// # fn main() -> std::io::Result<()> {
        /// let dir = tempfile::tempdir()?;
        /// let vfs = DiskVfs::without_watcher(dir.path()).unwrap();
        /// assert_eq!(vfs.version(), 0);
        /// # Ok(())
        /// # }
        /// ```
        pub fn version(&self) -> u64 {
            self.version.load(Ordering::Acquire)
        }

        /// Drain the list of asset paths invalidated since the last call.
        ///
        /// Each path is relative to the root and uses forward-slash
        /// separators. Returns an empty vector if no file watcher is active
        /// or no changes have been observed.
        ///
        /// # Examples
        ///
        /// ```no_run
        /// use martensite_assets::vfs::DiskVfs;
        /// # fn main() -> std::io::Result<()> {
        /// let dir = tempfile::tempdir()?;
        /// let vfs = DiskVfs::without_watcher(dir.path()).unwrap();
        /// assert!(vfs.take_invalidations().is_empty());
        /// # Ok(())
        /// # }
        /// ```
        pub fn take_invalidations(&self) -> Vec<String> {
            self.invalidated.lock().drain()
        }

        /// Returns the number of invalidation entries that have been
        /// dropped because the bounded queue reached its capacity.
        ///
        /// This counter is cumulative across all `take_invalidations`
        /// calls and is only reset when the [`DiskVfs`] is dropped. A
        /// non-zero value indicates the caller is not draining
        /// invalidations fast enough.
        ///
        /// # Examples
        ///
        /// ```no_run
        /// use martensite_assets::vfs::DiskVfs;
        /// # fn main() -> std::io::Result<()> {
        /// let dir = tempfile::tempdir()?;
        /// let vfs = DiskVfs::without_watcher(dir.path()).unwrap();
        /// assert_eq!(vfs.dropped_count(), 0);
        /// # Ok(())
        /// # }
        /// ```
        pub fn dropped_count(&self) -> u64 {
            self.invalidated.lock().dropped
        }

        /// Returns the number of cached assets.
        ///
        /// # Examples
        ///
        /// ```no_run
        /// use martensite_assets::vfs::DiskVfs;
        /// # fn main() -> std::io::Result<()> {
        /// let dir = tempfile::tempdir()?;
        /// std::fs::write(dir.path().join("a.txt"), b"a")?;
        /// let vfs = DiskVfs::without_watcher(dir.path()).unwrap();
        /// assert_eq!(vfs.len(), 1);
        /// # Ok(())
        /// # }
        /// ```
        pub fn len(&self) -> usize {
            self.cache.len()
        }

        /// Returns `true` if no assets were loaded.
        ///
        /// # Examples
        ///
        /// ```no_run
        /// use martensite_assets::vfs::DiskVfs;
        /// # fn main() -> std::io::Result<()> {
        /// let dir = tempfile::tempdir()?;
        /// let vfs = DiskVfs::without_watcher(dir.path()).unwrap();
        /// assert!(vfs.is_empty());
        /// # Ok(())
        /// # }
        /// ```
        pub fn is_empty(&self) -> bool {
            self.cache.is_empty()
        }
    }

    impl Vfs for DiskVfs {
        fn resolve(&self, path: &str) -> Option<&[u8]> {
            self.cache.get(path).map(|b| b.as_ref())
        }

        fn exists(&self, path: &str) -> bool {
            self.cache.contains_key(path)
        }

        fn list(&self) -> Vec<String> {
            self.cache.keys().cloned().collect()
        }
    }

    impl fmt::Debug for DiskVfs {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_struct("DiskVfs")
                .field("root", &self.root)
                .field("asset_count", &self.cache.len())
                .field("version", &self.version.load(Ordering::Relaxed))
                .field("watching", &self._watcher.is_some())
                .finish()
        }
    }

    /// Recursively load every file under `root` into a path -> bytes map.
    ///
    /// Paths are stored relative to `root` with forward-slash separators.
    /// Symlinks are not followed and directories are skipped. Errors reading
    /// an individual file propagate immediately.
    fn load_directory(root: &Path) -> std::io::Result<HashMap<String, Box<[u8]>>> {
        let mut cache = HashMap::new();
        if !root.exists() {
            return Ok(cache);
        }
        load_directory_into(root, root, &mut cache)?;
        Ok(cache)
    }

    fn load_directory_into(
        root: &Path,
        current: &Path,
        cache: &mut HashMap<String, Box<[u8]>>,
    ) -> std::io::Result<()> {
        for entry in std::fs::read_dir(current)? {
            let entry = entry?;
            let path = entry.path();
            let metadata = entry.metadata()?;
            if metadata.is_dir() {
                load_directory_into(root, &path, cache)?;
            } else if metadata.is_file() {
                let bytes = std::fs::read(&path)?;
                let rel = path.strip_prefix(root).unwrap_or(&path);
                let key = rel.to_string_lossy().replace('\\', "/");
                cache.insert(key, bytes.into_boxed_slice());
            }
        }
        Ok(())
    }

    /// Best-effort: start a recursive `notify` watcher over the root.
    ///
    /// On any create/modify/remove event the shared version counter is bumped
    /// and the affected relative path is appended to the invalidated list.
    /// Returns `None` if the watcher could not be created.
    fn try_start_watcher(state: WatchState) -> Option<RecommendedWatcher> {
        let WatchState {
            root,
            version,
            invalidated,
        } = state;

        // Clone the root Arc for the closure so the original can still be
        // passed to `watcher.watch` below.
        let closure_root = Arc::clone(&root);
        let handler = move |event: notify::Result<notify::Event>| {
            if let Ok(event) = event {
                if matches!(
                    event.kind,
                    EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
                ) {
                    version.fetch_add(1, Ordering::Release);
                    let mut invalidated = invalidated.lock();
                    for path in &event.paths {
                        // Record the path relative to the root when possible;
                        // otherwise fall back to the full path so the
                        // invalidation is never silently dropped.
                        let recorded = match path.strip_prefix(&*closure_root) {
                            Ok(rel) => rel.to_string_lossy().replace('\\', "/"),
                            Err(_) => path.to_string_lossy().replace('\\', "/"),
                        };
                        invalidated.push(recorded);
                    }
                }
            }
        };

        let mut watcher = notify::recommended_watcher(handler).ok()?;
        // Watching is best-effort: ignore errors (e.g. root removed).
        let _ = watcher.watch(&root, RecursiveMode::Recursive);
        Some(watcher)
    }
}

#[cfg(feature = "disk")]
pub use disk::DiskVfs;

// ============================================================================
// ReactiveVfsWatcher (behind the "reactive" feature)
// ============================================================================

#[cfg(feature = "reactive")]
mod reactive {
    use super::{DiskVfs, Vfs};
    use martensite_reactive::Signal;

    /// A wrapper around [`DiskVfs`] that emits a reactive [`Signal<u64>`]
    /// version counter whenever the watched filesystem changes.
    ///
    /// The disk VFS's background file watcher bumps an internal
    /// `AtomicU64` version counter on every create/modify/remove event.
    /// [`ReactiveVfsWatcher::check_for_changes`] polls that counter and,
    /// if it advanced since the last poll, updates the owned `Signal<u64>`
    /// so downstream reactive consumers (memos, effects) are notified
    /// that assets have changed and a reload is needed.
    ///
    /// The signal starts at `0` and is set to the disk VFS's current
    /// version on each detected change. Call `check_for_changes` once per
    /// frame (e.g. in the event loop) to bridge the push-based file
    /// watcher into the pull-based reactive graph.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_assets::vfs::ReactiveVfsWatcher;
    /// use martensite_assets::vfs::Vfs;
    /// # fn main() -> std::io::Result<()> {
    /// let dir = tempfile::tempdir()?;
    /// std::fs::write(dir.path().join("a.txt"), b"a")?;
    /// let watcher = ReactiveVfsWatcher::new(dir.path())?;
    /// let signal = watcher.version_signal();
    /// assert_eq!(signal.get_untracked(), 0);
    /// // After a file changes and check_for_changes is called, the signal
    /// // is updated to the new version.
    /// # Ok(())
    /// # }
    /// ```
    pub struct ReactiveVfsWatcher {
        vfs: DiskVfs,
        signal: Signal<u64>,
        last_seen: u64,
    }

    impl ReactiveVfsWatcher {
        /// Creates a new watcher over `root`, eagerly loading all files and
        /// starting a background file watcher (see [`DiskVfs::new`]).
        ///
        /// The reactive signal is initialized to `0`.
        pub fn new(root: impl AsRef<std::path::Path>) -> std::io::Result<Self> {
            Self::from_disk(DiskVfs::new(root)?)
        }

        /// Creates a new watcher from an existing [`DiskVfs`] without
        /// starting an additional background watcher.
        ///
        /// Useful in tests where file watching is unnecessary or would race
        /// with assertions (see [`DiskVfs::without_watcher`]).
        pub fn without_watcher(root: impl AsRef<std::path::Path>) -> std::io::Result<Self> {
            Self::from_disk(DiskVfs::without_watcher(root)?)
        }

        fn from_disk(vfs: DiskVfs) -> std::io::Result<Self> {
            let last_seen = vfs.version();
            Ok(Self {
                vfs,
                signal: Signal::new(0),
                last_seen,
            })
        }

        /// Returns a reference to the reactive version signal.
        ///
        /// The signal holds the last version counter observed by
        /// [`Self::check_for_changes`]. Reading it (via
        /// [`Signal::get`] / [`Signal::get_untracked`]) registers a
        /// dependency edge when called within a reactive context.
        pub fn version_signal(&self) -> &Signal<u64> {
            &self.signal
        }

        /// Polls the underlying [`DiskVfs`] version counter and, if it
        /// advanced since the last poll, updates the reactive signal and
        /// drains the invalidation list.
        ///
        /// Returns `true` if a change was detected (and the signal was
        /// updated), `false` otherwise. Call this once per frame from the
        /// application's event loop to bridge the file watcher into the
        /// reactive graph.
        ///
        /// # Examples
        ///
        /// ```no_run
        /// use martensite_assets::vfs::ReactiveVfsWatcher;
        /// # fn main() -> std::io::Result<()> {
        /// let dir = tempfile::tempdir()?;
        /// let mut watcher = ReactiveVfsWatcher::without_watcher(dir.path())?;
        /// // No changes yet.
        /// assert!(!watcher.check_for_changes());
        /// # Ok(())
        /// # }
        /// ```
        pub fn check_for_changes(&mut self) -> bool {
            let current = self.vfs.version();
            if current != self.last_seen {
                self.last_seen = current;
                // Drain the invalidation list so it doesn't grow unbounded.
                let _ = self.vfs.take_invalidations();
                self.signal.set(current);
                true
            } else {
                false
            }
        }

        /// Returns the last version observed by [`Self::check_for_changes`].
        pub fn version(&self) -> u64 {
            self.last_seen
        }

        /// Returns the list of asset paths invalidated since the last
        /// [`Self::check_for_changes`] drain.
        pub fn take_invalidations(&self) -> Vec<String> {
            self.vfs.take_invalidations()
        }
    }

    impl Vfs for ReactiveVfsWatcher {
        fn resolve(&self, path: &str) -> Option<&[u8]> {
            self.vfs.resolve(path)
        }

        fn exists(&self, path: &str) -> bool {
            self.vfs.exists(path)
        }

        fn list(&self) -> Vec<String> {
            self.vfs.list()
        }
    }

    impl std::fmt::Debug for ReactiveVfsWatcher {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("ReactiveVfsWatcher")
                .field("version", &self.last_seen)
                .field("signal", &self.signal.get_untracked())
                .finish()
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::io::Write;

        #[test]
        fn signal_starts_at_zero() {
            let dir = tempfile::tempdir().unwrap();
            std::fs::write(dir.path().join("a.txt"), b"a").unwrap();
            let watcher = ReactiveVfsWatcher::without_watcher(dir.path()).unwrap();
            assert_eq!(watcher.version_signal().get_untracked(), 0);
            assert_eq!(watcher.version(), 0);
        }

        #[test]
        fn check_for_changes_returns_false_when_unchanged() {
            let dir = tempfile::tempdir().unwrap();
            std::fs::write(dir.path().join("a.txt"), b"a").unwrap();
            let mut watcher = ReactiveVfsWatcher::without_watcher(dir.path()).unwrap();
            assert!(!watcher.check_for_changes());
            assert_eq!(watcher.version_signal().get_untracked(), 0);
        }

        #[test]
        fn signal_updates_when_file_changes() {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("watched.txt");
            std::fs::write(&path, b"v1").unwrap();
            let mut watcher = ReactiveVfsWatcher::new(dir.path()).unwrap();

            // Give the watcher a moment to register.
            std::thread::sleep(std::time::Duration::from_millis(150));

            // Modify the file.
            let mut file = std::fs::File::create(&path).unwrap();
            file.write_all(b"v2").unwrap();
            drop(file);

            // Poll for the change (best-effort timing).
            let mut detected = false;
            for _ in 0..20 {
                std::thread::sleep(std::time::Duration::from_millis(50));
                if watcher.check_for_changes() {
                    detected = true;
                    break;
                }
            }
            assert!(detected, "watcher did not detect file change");
            // The signal should now reflect the new version (> 0).
            let v = watcher.version_signal().get_untracked();
            assert!(v > 0, "signal should be > 0 after change, got {v}");
            assert_eq!(v, watcher.version());
        }

        #[test]
        fn check_for_changes_is_idempotent() {
            // After detecting a change, a second call without a new change
            // should return false and not re-update the signal.
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("f.txt");
            std::fs::write(&path, b"v1").unwrap();
            let mut watcher = ReactiveVfsWatcher::new(dir.path()).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(150));

            let mut file = std::fs::File::create(&path).unwrap();
            file.write_all(b"v2").unwrap();
            drop(file);

            // Wait for the change to be detected.
            let mut first_detected = false;
            for _ in 0..20 {
                std::thread::sleep(std::time::Duration::from_millis(50));
                if watcher.check_for_changes() {
                    first_detected = true;
                    break;
                }
            }
            assert!(first_detected);
            let v_after = watcher.version_signal().get_untracked();

            // No new changes — should return false and keep the same version.
            std::thread::sleep(std::time::Duration::from_millis(100));
            assert!(!watcher.check_for_changes());
            assert_eq!(watcher.version_signal().get_untracked(), v_after);
        }

        #[test]
        fn delegates_vfs_operations() {
            let dir = tempfile::tempdir().unwrap();
            std::fs::write(dir.path().join("a.txt"), b"alpha").unwrap();
            let watcher = ReactiveVfsWatcher::without_watcher(dir.path()).unwrap();
            assert!(watcher.exists("a.txt"));
            assert_eq!(watcher.resolve("a.txt"), Some(&b"alpha"[..]));
            assert_eq!(watcher.list().len(), 1);
        }

        #[test]
        fn debug_format() {
            let dir = tempfile::tempdir().unwrap();
            std::fs::write(dir.path().join("a.txt"), b"a").unwrap();
            let watcher = ReactiveVfsWatcher::without_watcher(dir.path()).unwrap();
            let s = format!("{:?}", watcher);
            assert!(s.contains("ReactiveVfsWatcher"));
        }
    }
}

#[cfg(feature = "reactive")]
pub use reactive::ReactiveVfsWatcher;

// ============================================================================
// VfsBackend
// ============================================================================

/// A backend-agnostic VFS selected at runtime.
///
/// Wraps either a disk-backed ([`DiskVfs`]) or embedded ([`EmbeddedVfs`]) VFS
/// behind the common [`Vfs`] trait, letting application code hold a single
/// `VfsBackend` and switch strategies without changing call sites.
///
/// The `Disk` variant is only available when the `disk` feature is enabled
/// (it is on by default).
///
/// # Examples
///
/// ```
/// use martensite_assets::vfs::{EmbeddedVfs, Vfs, VfsBackend};
///
/// static T: &[(&str, &[u8])] = &[("a.txt", b"alpha")];
/// let backend: VfsBackend = VfsBackend::Embedded(EmbeddedVfs::new(T));
/// assert_eq!(backend.resolve("a.txt"), Some(&b"alpha"[..]));
/// ```
pub enum VfsBackend {
    /// Disk-backed VFS for development hot-reloading.
    #[cfg(feature = "disk")]
    Disk(DiskVfs),
    /// Embedded, zero-copy VFS for release binaries.
    Embedded(EmbeddedVfs),
}

impl Vfs for VfsBackend {
    fn resolve(&self, path: &str) -> Option<&[u8]> {
        match self {
            #[cfg(feature = "disk")]
            Self::Disk(d) => d.resolve(path),
            Self::Embedded(e) => e.resolve(path),
        }
    }

    fn exists(&self, path: &str) -> bool {
        match self {
            #[cfg(feature = "disk")]
            Self::Disk(d) => d.exists(path),
            Self::Embedded(e) => e.exists(path),
        }
    }

    fn list(&self) -> Vec<String> {
        match self {
            #[cfg(feature = "disk")]
            Self::Disk(d) => d.list(),
            Self::Embedded(e) => e.list(),
        }
    }
}

impl fmt::Debug for VfsBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            #[cfg(feature = "disk")]
            Self::Disk(d) => f.debug_tuple("VfsBackend::Disk").field(d).finish(),
            Self::Embedded(e) => f.debug_tuple("VfsBackend::Embedded").field(e).finish(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- EmbeddedVfs ----

    static SAMPLE_TABLE: &[(&str, &[u8])] = &[
        ("a.txt", b"alpha"),
        ("dir/b.txt", b"beta"),
        ("dir/c.bin", &[0u8, 1, 2, 3]),
    ];

    #[test]
    fn embedded_resolves_existing_assets() {
        let vfs = EmbeddedVfs::new(SAMPLE_TABLE);
        assert_eq!(vfs.resolve("a.txt"), Some(&b"alpha"[..]));
        assert_eq!(vfs.resolve("dir/c.bin"), Some(&[0u8, 1, 2, 3][..]));
    }

    #[test]
    fn embedded_returns_none_for_missing() {
        let vfs = EmbeddedVfs::new(SAMPLE_TABLE);
        assert_eq!(vfs.resolve("missing.txt"), None);
    }

    #[test]
    fn embedded_exists_and_list() {
        let vfs = EmbeddedVfs::new(SAMPLE_TABLE);
        assert!(vfs.exists("a.txt"));
        assert!(!vfs.exists("nope"));
        let mut listed = vfs.list();
        listed.sort();
        assert_eq!(
            listed,
            ["a.txt", "dir/b.txt", "dir/c.bin"]
                .into_iter()
                .map(String::from)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn embedded_len_and_is_empty() {
        assert_eq!(EmbeddedVfs::new(SAMPLE_TABLE).len(), 3);
        assert!(!EmbeddedVfs::new(SAMPLE_TABLE).is_empty());
        static EMPTY: &[(&str, &[u8])] = &[];
        assert!(EmbeddedVfs::new(EMPTY).is_empty());
        assert_eq!(EmbeddedVfs::new(EMPTY).len(), 0);
    }

    #[test]
    fn embedded_resolve_handle() {
        let vfs = EmbeddedVfs::new(SAMPLE_TABLE);
        let h = vfs.resolve_handle("a.txt").unwrap();
        assert_eq!(h.path, "a.txt");
        assert_eq!(h.data, b"alpha");
        assert_eq!(h.as_ref(), b"alpha");
        assert!(vfs.resolve_handle("missing").is_none());
    }

    #[test]
    fn embedded_debug_format() {
        let vfs = EmbeddedVfs::new(SAMPLE_TABLE);
        let s = format!("{:?}", vfs);
        assert!(s.contains("EmbeddedVfs"));
        assert!(s.contains("3"));
    }

    // ---- AssetPath ----

    #[test]
    fn asset_path_accepts_relative() {
        assert_eq!(AssetPath::new("a/b.txt").unwrap().as_str(), "a/b.txt");
        assert_eq!(AssetPath::new("./a.txt").unwrap().as_str(), "a.txt");
        assert_eq!(AssetPath::new("././x.txt").unwrap().as_str(), "x.txt");
    }

    #[test]
    fn asset_path_rejects_invalid() {
        assert_eq!(AssetPath::new(""), Err(AssetPathError::Empty));
        assert_eq!(AssetPath::new("/abs"), Err(AssetPathError::Absolute));
        assert_eq!(AssetPath::new("a\\b"), Err(AssetPathError::Backslash));
        assert_eq!(AssetPath::new("../x"), Err(AssetPathError::ParentTraversal));
        assert_eq!(
            AssetPath::new("a/../b"),
            Err(AssetPathError::ParentTraversal)
        );
    }

    #[test]
    fn asset_path_join_and_parent() {
        let p = AssetPath::new("dir").unwrap();
        let joined = p.join("file.txt").unwrap();
        assert_eq!(joined.as_str(), "dir/file.txt");

        let p2 = AssetPath::new("dir/file.txt").unwrap();
        assert_eq!(p2.parent().unwrap().as_str(), "dir");
        assert!(AssetPath::new("file.txt").unwrap().parent().is_none());
    }

    #[test]
    fn asset_path_display_and_as_ref() {
        let p = AssetPath::new("a/b.txt").unwrap();
        assert_eq!(format!("{}", p), "a/b.txt");
        assert_eq!(p.as_ref(), "a/b.txt");
    }

    #[test]
    fn asset_path_error_display() {
        assert_eq!(AssetPathError::Empty.to_string(), "asset path is empty");
        assert_eq!(
            AssetPathError::Absolute.to_string(),
            "asset path must be relative, not absolute"
        );
        assert_eq!(
            AssetPathError::Backslash.to_string(),
            "asset path must use '/' separators, not '\\'"
        );
        assert_eq!(
            AssetPathError::ParentTraversal.to_string(),
            "asset path may not contain '..' components"
        );
    }

    // ---- VfsBackend (embedded variant, always available) ----

    #[test]
    fn vfs_backend_embedded_dispatches() {
        let backend = VfsBackend::Embedded(EmbeddedVfs::new(SAMPLE_TABLE));
        assert!(backend.exists("a.txt"));
        assert_eq!(backend.resolve("a.txt"), Some(&b"alpha"[..]));
        assert_eq!(backend.list().len(), 3);
        let s = format!("{:?}", backend);
        assert!(s.contains("Embedded"));
    }

    // ---- DiskVfs (only when the disk feature is enabled) ----

    #[cfg(feature = "disk")]
    mod disk_tests {
        use super::*;
        use std::io::Write;

        fn make_temp_tree() -> tempfile::TempDir {
            let dir = tempfile::tempdir().unwrap();
            std::fs::write(dir.path().join("top.txt"), b"top").unwrap();
            let sub = dir.path().join("sub");
            std::fs::create_dir(&sub).unwrap();
            std::fs::write(sub.join("nested.txt"), b"nested").unwrap();
            std::fs::write(sub.join("data.bin"), [0u8, 1, 2, 3]).unwrap();
            dir
        }

        #[test]
        fn disk_loads_files_recursively() {
            let dir = make_temp_tree();
            let vfs = DiskVfs::without_watcher(dir.path()).unwrap();
            assert_eq!(vfs.resolve("top.txt"), Some(&b"top"[..]));
            assert_eq!(vfs.resolve("sub/nested.txt"), Some(&b"nested"[..]));
            assert_eq!(vfs.resolve("sub/data.bin"), Some(&[0u8, 1, 2, 3][..]));
            assert!(!vfs.exists("missing.txt"));
            assert_eq!(vfs.len(), 3);
            assert!(!vfs.is_empty());
        }

        #[test]
        fn disk_list_and_exists() {
            let dir = make_temp_tree();
            let vfs = DiskVfs::without_watcher(dir.path()).unwrap();
            let mut listed = vfs.list();
            listed.sort();
            assert_eq!(
                listed,
                ["sub/data.bin", "sub/nested.txt", "top.txt"]
                    .into_iter()
                    .map(String::from)
                    .collect::<Vec<_>>()
            );
        }

        #[test]
        fn disk_root_and_version() {
            let dir = make_temp_tree();
            let vfs = DiskVfs::without_watcher(dir.path()).unwrap();
            assert_eq!(vfs.root(), dir.path());
            assert_eq!(vfs.version(), 0);
            assert!(vfs.take_invalidations().is_empty());
        }

        #[test]
        fn disk_empty_directory() {
            let dir = tempfile::tempdir().unwrap();
            let vfs = DiskVfs::without_watcher(dir.path()).unwrap();
            assert!(vfs.is_empty());
            assert_eq!(vfs.len(), 0);
            assert!(vfs.list().is_empty());
        }

        #[test]
        fn disk_missing_root_is_empty() {
            let dir = tempfile::tempdir().unwrap();
            let missing = dir.path().join("does_not_exist");
            let vfs = DiskVfs::without_watcher(&missing).unwrap();
            assert!(vfs.is_empty());
        }

        #[test]
        fn disk_new_starts_watcher_and_resolves() {
            let dir = make_temp_tree();
            let vfs = DiskVfs::new(dir.path()).unwrap();
            assert_eq!(vfs.resolve("top.txt"), Some(&b"top"[..]));
            assert!(format!("{:?}", vfs).contains("watching"));
        }

        #[test]
        fn disk_watcher_detects_modification() {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("watched.txt");
            std::fs::write(&path, b"v1").unwrap();
            let vfs = DiskVfs::new(dir.path()).unwrap();
            // Give the watcher a moment to register, then modify the file.
            std::thread::sleep(std::time::Duration::from_millis(150));
            let mut file = std::fs::File::create(&path).unwrap();
            file.write_all(b"v2").unwrap();
            drop(file);
            // Poll for the invalidation signal (best-effort timing).
            let mut detected = false;
            for _ in 0..20 {
                std::thread::sleep(std::time::Duration::from_millis(50));
                if vfs.version() > 0 {
                    detected = true;
                    break;
                }
            }
            assert!(detected, "watcher did not bump version on modification");
            let invalidations = vfs.take_invalidations();
            // The exact path reported by the OS watcher may vary (some
            // platforms emit directory-level events); accept any invalidation
            // that refers to the modified file.
            assert!(
                invalidations.iter().any(|p| p.ends_with("watched.txt")),
                "expected an invalidation for watched.txt, got {invalidations:?}"
            );
        }

        #[test]
        fn disk_resolve_handle() {
            let dir = make_temp_tree();
            let vfs = DiskVfs::without_watcher(dir.path()).unwrap();
            let h = vfs.resolve_handle("top.txt").unwrap();
            assert_eq!(h.path, "top.txt");
            assert_eq!(h.data, b"top");
        }

        #[test]
        fn vfs_backend_disk_dispatches() {
            let dir = make_temp_tree();
            let backend = VfsBackend::Disk(DiskVfs::without_watcher(dir.path()).unwrap());
            assert!(backend.exists("top.txt"));
            assert_eq!(backend.resolve("top.txt"), Some(&b"top"[..]));
            assert_eq!(backend.list().len(), 3);
            assert!(format!("{:?}", backend).contains("Disk"));
        }
    }

    /// Single-digit microsecond embedded resolution latency gate (exit
    /// criterion 5.2). The design target is single-digit microseconds; this
    /// gate asserts resolution stays under 10 µs.
    #[test]
    fn embedded_resolution_under_10_micros() {
        let vfs = EmbeddedVfs::new(SAMPLE_TABLE);
        // Warm up (first call may touch cold cache lines).
        let _ = vfs.resolve("dir/b.txt");
        let iterations = 1000u32;
        let start = std::time::Instant::now();
        for _ in 0..iterations {
            let _ = vfs.resolve("dir/b.txt");
        }
        let elapsed = start.elapsed();
        let per_call_ns = elapsed.as_nanos() as f64 / f64::from(iterations);
        // 10 microseconds = 10_000 nanoseconds. Use a safety margin to keep
        // the gate stable on noisy CI runners while still asserting the
        // single-digit microsecond design goal on typical hardware.
        assert!(
            per_call_ns < 10_000.0,
            "embedded resolution took {per_call_ns:.1} ns/call, expected < 10,000 ns"
        );
    }
}
