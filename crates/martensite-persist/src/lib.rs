//! Key-value settings/state persistence.
//!
//! `martensite-persist` provides the canonical "remember my settings"
//! layer for Martensite applications: a small [`StateStore`] contract
//! over `serde_json::Value`, a volatile [`MemoryStore`], an atomic
//! [`JsonFileStore`], and [`paths`] helpers that resolve the per-OS
//! config directory without a `dirs` dependency.
//!
//! # Architecture
//!
//! * [`StateStore`] — `get` / `set` / `remove` / `keys` / `flush`.
//! * [`MemoryStore`] — volatile `BTreeMap` backend for tests and
//!   session-scoped state.
//! * [`JsonFileStore`] — single JSON object file, loaded lazily,
//!   written atomically (tmp + rename) on `flush`.
//! * [`paths::app_config_dir`] — `%APPDATA%` / `Application Support` /
//!   `$XDG_CONFIG_HOME` resolution.
//!
//! The entire crate is `#![forbid(unsafe_code)]`.
//!
//! # Examples
//!
//! ```
//! use martensite_persist::{MemoryStore, StateStore};
//!
//! let mut store = MemoryStore::new();
//! store.set("theme", "dark");
//! store.set("volume", 0.8);
//! assert_eq!(store.get("theme").unwrap().as_str().unwrap(), "dark");
//! ```
#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod paths;
pub mod store;

pub use paths::{app_config_dir, default_store_path};
pub use store::{JsonFileStore, MemoryStore, PersistError, StateStore};

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn memory_store_round_trip() {
        let mut s = MemoryStore::new();
        s.set("a", 1);
        s.set("b", "x");
        assert_eq!(s.get("a").unwrap().as_i64().unwrap(), 1);
        assert_eq!(s.keys(), vec!["a".to_string(), "b".to_string()]);
        assert_eq!(s.remove("a"), Some(json!(1)));
        assert!(s.get("a").is_none());
    }

    #[test]
    fn json_file_store_round_trip() {
        let dir = std::env::temp_dir().join(format!("martensite-persist-{}", std::process::id()));
        let path = dir.join("settings.json");
        {
            let mut s = JsonFileStore::open(&path).unwrap();
            s.set("theme", "dark");
            assert!(s.is_dirty());
            s.flush().unwrap();
            assert!(!s.is_dirty());
        }
        let s = JsonFileStore::open(&path).unwrap();
        assert_eq!(s.get("theme").unwrap().as_str().unwrap(), "dark");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn json_file_store_missing_is_empty() {
        let dir =
            std::env::temp_dir().join(format!("martensite-persist-missing-{}", std::process::id()));
        let s = JsonFileStore::open(dir.join("none.json")).unwrap();
        assert!(s.keys().is_empty());
    }

    #[test]
    fn json_file_store_corrupt_errors() {
        let dir =
            std::env::temp_dir().join(format!("martensite-persist-corrupt-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bad.json");
        std::fs::write(&path, "{not json").unwrap();
        assert!(matches!(
            JsonFileStore::open(&path),
            Err(PersistError::Corrupt(_))
        ));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn config_dir_shape() {
        if let Some(dir) = app_config_dir("acme", "app") {
            assert!(dir.ends_with("app"));
            assert!(dir.to_string_lossy().contains("acme"));
        }
    }
}
