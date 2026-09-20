//! Key-value state store trait and backends.
//!
//! [`StateStore`] is the read/write/remove/flush contract. Two backends
//! ship in-crate:
//!
//! * [`MemoryStore`] — volatile `BTreeMap`, for tests, sessions, and as a
//!   write-through cache.
//! * [`JsonFileStore`] — a single JSON object file on disk, loaded lazily
//!   on open and written atomically (tmp + rename) on
//!   [`StateStore::flush`].
//!
//! Values are `serde_json::Value` — the lingua franca callers already
//! serialize app state into — so `set` accepts anything `Into<Value>`.
//!
//! # Examples
//!
//! ```
//! use martensite_persist::{MemoryStore, StateStore};
//!
//! let mut s = MemoryStore::new();
//! s.set("theme", "dark");
//! assert_eq!(s.get("theme").unwrap().as_str().unwrap(), "dark");
//! ```

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

/// Why a persistence operation failed.
///
/// # Examples
///
/// ```
/// use martensite_persist::PersistError;
///
/// let e = PersistError::Io("disk full".into());
/// assert!(e.to_string().contains("disk full"));
/// ```
#[derive(Debug)]
pub enum PersistError {
    /// Underlying I/O failure (read/write/rename/create_dir).
    Io(String),
    /// The backing file was not valid JSON object data.
    Corrupt(String),
}

impl std::fmt::Display for PersistError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PersistError::Io(msg) => write!(f, "i/o error: {msg}"),
            PersistError::Corrupt(msg) => write!(f, "corrupt store: {msg}"),
        }
    }
}

impl std::error::Error for PersistError {}

impl From<std::io::Error> for PersistError {
    fn from(e: std::io::Error) -> Self {
        PersistError::Io(e.to_string())
    }
}

impl From<serde_json::Error> for PersistError {
    fn from(e: serde_json::Error) -> Self {
        PersistError::Corrupt(e.to_string())
    }
}

/// The read/write contract every state-store backend implements.
///
/// # Examples
///
/// ```
/// use martensite_persist::{MemoryStore, StateStore};
///
/// let mut s = MemoryStore::new();
/// s.set("volume", 0.8);
/// assert_eq!(s.get("volume").unwrap().as_f64().unwrap(), 0.8);
/// s.remove("volume");
/// assert!(s.get("volume").is_none());
/// ```
pub trait StateStore {
    /// The value stored under `key`, if any.
    fn get(&self, key: &str) -> Option<&Value>;
    /// Store `value` under `key`, replacing any previous value.
    fn set(&mut self, key: impl Into<String>, value: impl Into<Value>);
    /// Remove `key`, returning the evicted value if present.
    fn remove(&mut self, key: &str) -> Option<Value>;
    /// All keys, sorted.
    fn keys(&self) -> Vec<String>;
    /// Persist pending writes to the backing medium. No-op for
    /// volatile backends.
    fn flush(&mut self) -> Result<(), PersistError>;
}

/// An in-memory [`StateStore`] — volatile, for tests and session state.
///
/// # Examples
///
/// ```
/// use martensite_persist::{MemoryStore, StateStore};
///
/// let mut s = MemoryStore::new();
/// s.set("k", true);
/// assert_eq!(s.keys(), vec!["k".to_string()]);
/// ```
#[derive(Default, Debug)]
pub struct MemoryStore {
    map: BTreeMap<String, Value>,
}

impl MemoryStore {
    /// An empty store.
    ///
    /// ```
    /// use martensite_persist::{MemoryStore, StateStore};
    ///
    /// assert!(MemoryStore::new().keys().is_empty());
    /// ```
    pub fn new() -> Self {
        Self::default()
    }
}

impl StateStore for MemoryStore {
    fn get(&self, key: &str) -> Option<&Value> {
        self.map.get(key)
    }

    fn set(&mut self, key: impl Into<String>, value: impl Into<Value>) {
        self.map.insert(key.into(), value.into());
    }

    fn remove(&mut self, key: &str) -> Option<Value> {
        self.map.remove(key)
    }

    fn keys(&self) -> Vec<String> {
        self.map.keys().cloned().collect()
    }

    fn flush(&mut self) -> Result<(), PersistError> {
        Ok(())
    }
}

/// A JSON-file-backed [`StateStore`].
///
/// The file holds a single JSON object (`{"key": value, …}`). Writes are
/// buffered in memory and persisted by [`StateStore::flush`], which writes
/// a sibling `*.tmp` file and atomically renames it over the target so a
/// crash mid-write cannot corrupt the store. [`JsonFileStore::open`]
/// tolerates a missing file (treated as empty) but reports malformed JSON
/// as [`PersistError::Corrupt`].
///
/// # Examples
///
/// ```no_run
/// use martensite_persist::{JsonFileStore, StateStore};
///
/// let mut s = JsonFileStore::open("/tmp/demo-settings.json").unwrap();
/// s.set("theme", "dark");
/// s.flush().unwrap();
/// ```
#[derive(Debug)]
pub struct JsonFileStore {
    path: PathBuf,
    map: BTreeMap<String, Value>,
    dirty: bool,
}

impl JsonFileStore {
    /// Open (or create-on-flush) the store at `path`.
    ///
    /// A missing file yields an empty store; a malformed file returns
    /// [`PersistError::Corrupt`].
    ///
    /// ```no_run
    /// use martensite_persist::JsonFileStore;
    ///
    /// let s = JsonFileStore::open("/tmp/demo.json").unwrap();
    /// assert!(martensite_persist::StateStore::keys(&s).is_empty());
    /// ```
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, PersistError> {
        let path = path.into();
        let map = match std::fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<Value>(&text) {
                Ok(Value::Object(m)) => m.into_iter().collect(),
                Ok(_) => {
                    return Err(PersistError::Corrupt(
                        "top-level JSON value is not an object".into(),
                    ))
                }
                Err(e) => return Err(e.into()),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
            Err(e) => return Err(e.into()),
        };
        Ok(Self {
            path,
            map,
            dirty: false,
        })
    }

    /// The file this store writes to.
    ///
    /// ```no_run
    /// use martensite_persist::JsonFileStore;
    ///
    /// let s = JsonFileStore::open("/tmp/demo.json").unwrap();
    /// assert_eq!(s.path().to_str().unwrap(), "/tmp/demo.json");
    /// ```
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// `true` when in-memory changes have not been flushed.
    ///
    /// ```no_run
    /// use martensite_persist::{JsonFileStore, StateStore};
    ///
    /// let mut s = JsonFileStore::open("/tmp/demo2.json").unwrap();
    /// assert!(!s.is_dirty());
    /// s.set("k", 1);
    /// assert!(s.is_dirty());
    /// ```
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }
}

impl StateStore for JsonFileStore {
    fn get(&self, key: &str) -> Option<&Value> {
        self.map.get(key)
    }

    fn set(&mut self, key: impl Into<String>, value: impl Into<Value>) {
        self.map.insert(key.into(), value.into());
        self.dirty = true;
    }

    fn remove(&mut self, key: &str) -> Option<Value> {
        let v = self.map.remove(key);
        if v.is_some() {
            self.dirty = true;
        }
        v
    }

    fn keys(&self) -> Vec<String> {
        self.map.keys().cloned().collect()
    }

    fn flush(&mut self) -> Result<(), PersistError> {
        if let Some(parent) = self.path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let obj: serde_json::Map<String, Value> = self.map.clone().into_iter().collect();
        let text = serde_json::to_string_pretty(&Value::Object(obj))?;
        let tmp = self.path.with_extension("tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, &self.path)?;
        self.dirty = false;
        Ok(())
    }
}
