//! Capability-based security sandbox for Martensite plugins.
//!
//! Plugins start with no access to host resources. Each permission must be
//! explicitly granted through a [`Capability`] before the plugin runtime will
//! allow a host call to proceed. Unauthorized calls trap the guest cleanly.

use std::collections::HashSet;
use std::path::PathBuf;

use martensite_reactive::SignalId;

/// A single host resource permission that can be granted to a plugin.
///
/// Capabilities are compared by value, so two grants for the same signal or the
/// same filesystem path are equivalent.
///
/// # Examples
///
/// ```
/// use martensite_plugin::Capability;
/// use std::path::PathBuf;
///
/// let read_asset = Capability::FileRead(PathBuf::from("/assets"));
/// let write_log = Capability::FileWrite(PathBuf::from("/tmp/plugin.log"));
/// let network = Capability::Network;
///
/// assert_ne!(read_asset, network);
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Capability {
    /// Permission to read the current value of the given reactive signal.
    SignalRead(SignalId),
    /// Permission to write a new value into the given reactive signal.
    SignalWrite(SignalId),
    /// Permission to read from the given filesystem path.
    FileRead(PathBuf),
    /// Permission to write to the given filesystem path.
    FileWrite(PathBuf),
    /// Permission to open network sockets.
    Network,
}

/// A set of capabilities held by a plugin instance.
///
/// Membership tests are `O(1)` on average.
///
/// # Examples
///
/// ```
/// use martensite_plugin::{Capability, CapabilitySet};
///
/// let mut caps = CapabilitySet::empty();
/// caps.grant(Capability::Network);
/// assert!(caps.contains(&Capability::Network));
/// caps.revoke(&Capability::Network);
/// assert!(!caps.contains(&Capability::Network));
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CapabilitySet(HashSet<Capability>);

impl CapabilitySet {
    /// Creates an empty capability set.
    pub fn empty() -> Self {
        Self(HashSet::new())
    }

    /// Returns a builder for constructing a capability set fluently.
    pub fn builder() -> PluginBuilder {
        PluginBuilder::new()
    }

    /// Returns the number of distinct capabilities in the set.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns `true` if no capabilities have been granted.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Grants a capability, returning `true` if it was newly inserted.
    pub fn grant(&mut self, cap: Capability) -> bool {
        self.0.insert(cap)
    }

    /// Revokes a capability, returning `true` if it was present.
    pub fn revoke(&mut self, cap: &Capability) -> bool {
        self.0.remove(cap)
    }

    /// Returns `true` if the capability is currently granted.
    pub fn contains(&self, cap: &Capability) -> bool {
        self.0.contains(cap)
    }

    /// Returns `true` if a `file_read` request for `requested_path` is
    /// authorized by any granted [`Capability::FileRead`] entry.
    ///
    /// Authorization is performed by canonicalizing both the granted
    /// roots and the requested path, then requiring the requested path
    /// to be equal to, or descend into, at least one granted root. This
    /// defeats path-traversal attacks (`/assets/../etc/passwd`) that
    /// exact-match checks would otherwise miss when a directory is
    /// granted and a child file is requested.
    ///
    /// When the requested file does not exist on disk (so
    /// [`std::fs::canonicalize`] fails), the path is normalized
    /// lexically via [`std::path::Path::components`] stripping of `.`
    /// and resolving `..` against the granted root, and the prefix
    /// check is applied to the normalized form. This keeps the check
    /// total (no filesystem dependency) while still rejecting `..`
    /// escapes.
    ///
    /// Granting a directory (e.g. `FileRead("/assets")`) authorizes
    /// reads of any file beneath it (e.g. `/assets/textures/foo.png`).
    /// Granting a file authorizes only that exact file.
    pub fn file_read_allowed(&self, requested_path: &std::path::Path) -> bool {
        self.file_path_allowed(requested_path, true)
    }

    /// Returns `true` if a `file_write` request for `requested_path` is
    /// authorized by any granted [`Capability::FileWrite`] entry.
    /// See [`Self::file_read_allowed`] for canonicalization semantics.
    pub fn file_write_allowed(&self, requested_path: &std::path::Path) -> bool {
        self.file_path_allowed(requested_path, false)
    }

    fn file_path_allowed(&self, requested_path: &std::path::Path, read: bool) -> bool {
        let requested_canon = std::fs::canonicalize(requested_path).ok();
        for cap in self.0.iter() {
            let granted = match cap {
                Capability::FileRead(p) if read => p,
                Capability::FileWrite(p) if !read => p,
                _ => continue,
            };
            // Try filesystem canonicalization first (strongest guarantee).
            if let (Some(req_c), Ok(grant_c)) = (&requested_canon, std::fs::canonicalize(granted)) {
                if req_c == &grant_c || req_c.starts_with(&grant_c) {
                    return true;
                }
                continue;
            }
            // Fall back to lexical normalization for paths that do not
            // exist yet (writes) or are inside a granted directory whose
            // own canonicalization also failed.
            if lexical_starts_with(requested_path, granted) {
                return true;
            }
        }
        false
    }
}

/// Lexically normalize `path` (resolving `.` and `..` components without
/// touching the filesystem) and return `true` if the normalized form is
/// equal to, or a descendant of, `root` (also lexically normalized).
///
/// This is the filesystem-independent fallback used when
/// [`std::fs::canonicalize`] cannot resolve a path (e.g. the file does
/// not yet exist). It rejects `..` escapes from a granted root while
/// permitting legitimate child paths.
fn lexical_starts_with(path: &std::path::Path, root: &std::path::Path) -> bool {
    let norm_path = lexical_normalize(path);
    let norm_root = lexical_normalize(root);
    norm_path == norm_root || norm_path.starts_with(&norm_root)
}

/// Lexically normalize a path by consuming `.` components and resolving
/// `..` components against the accumulated prefix, without touching the
/// filesystem. The result is a `PathBuf` containing only normal
/// components.
fn lexical_normalize(path: &std::path::Path) -> std::path::PathBuf {
    use std::path::Component;
    let mut out = std::path::PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    // `..` that escapes the root: keep it so a prefix
                    // check will fail rather than silently allow.
                    out.push("..");
                }
            }
            Component::RootDir | Component::Prefix(_) => {
                out.push(comp.as_os_str());
            }
            Component::Normal(s) => out.push(s),
        }
    }
    out
}

/// Fluent builder for assembling a [`CapabilitySet`].
///
/// # Examples
///
/// ```
/// use martensite_plugin::{Capability, CapabilitySet, PluginBuilder};
/// use std::path::PathBuf;
///
/// let caps = PluginBuilder::new()
///     .grant(Capability::Network)
///     .grant(Capability::FileRead(PathBuf::from("/assets")))
///     .revoke(Capability::Network)
///     .build();
///
/// assert!(!caps.contains(&Capability::Network));
/// assert!(caps.contains(&Capability::FileRead(PathBuf::from("/assets"))));
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PluginBuilder {
    caps: CapabilitySet,
}

impl PluginBuilder {
    /// Creates a new builder with no capabilities granted.
    pub fn new() -> Self {
        Self {
            caps: CapabilitySet::empty(),
        }
    }

    /// Grants the given capability and returns the builder.
    pub fn grant(mut self, cap: Capability) -> Self {
        self.caps.grant(cap);
        self
    }

    /// Revokes the given capability and returns the builder.
    pub fn revoke(mut self, cap: Capability) -> Self {
        self.caps.revoke(&cap);
        self
    }

    /// Finalizes the builder into an immutable capability set.
    pub fn build(self) -> CapabilitySet {
        self.caps
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_set_contains_nothing() {
        let caps = CapabilitySet::empty();
        assert!(caps.is_empty());
        assert_eq!(caps.len(), 0);
        assert!(!caps.contains(&Capability::Network));
    }

    #[test]
    fn grant_and_revoke_signal() {
        let mut caps = CapabilitySet::empty();
        let id = SignalId::next();
        let cap = Capability::SignalRead(id);

        assert!(caps.grant(cap.clone()));
        assert!(caps.contains(&cap));
        assert!(!caps.grant(cap.clone()));

        assert!(caps.revoke(&cap));
        assert!(!caps.contains(&cap));
        assert!(!caps.revoke(&cap));
    }

    #[test]
    fn builder_assembles_caps() {
        let path = PathBuf::from("/assets");
        let caps = PluginBuilder::new()
            .grant(Capability::Network)
            .grant(Capability::FileRead(path.clone()))
            .grant(Capability::SignalWrite(SignalId::next()))
            .revoke(Capability::Network)
            .build();

        assert_eq!(caps.len(), 2);
        assert!(!caps.contains(&Capability::Network));
        assert!(caps.contains(&Capability::FileRead(path)));
    }

    #[test]
    fn file_capabilities_are_distinct_by_path() {
        let a = Capability::FileRead(PathBuf::from("/a"));
        let b = Capability::FileRead(PathBuf::from("/b"));
        let mut caps = CapabilitySet::empty();
        caps.grant(a.clone());
        assert!(caps.contains(&a));
        assert!(!caps.contains(&b));
    }

    #[test]
    fn file_read_allowed_rejects_traversal_lexically() {
        // Grant a directory and verify that a `..` escape is rejected
        // even when the filesystem cannot canonicalize the path.
        let mut caps = CapabilitySet::empty();
        caps.grant(Capability::FileRead(PathBuf::from("/assets")));

        // Legitimate child path is allowed (lexical fallback).
        assert!(caps.file_read_allowed(std::path::Path::new("/assets/foo.txt")));
        // Traversal escape is rejected.
        assert!(!caps.file_read_allowed(std::path::Path::new("/assets/../etc/passwd")));
        // Sibling directory is rejected.
        assert!(!caps.file_read_allowed(std::path::Path::new("/etc/passwd")));
        // Exact granted root is allowed.
        assert!(caps.file_read_allowed(std::path::Path::new("/assets")));
    }

    #[test]
    fn file_read_allowed_exact_file_grant() {
        let mut caps = CapabilitySet::empty();
        caps.grant(Capability::FileRead(PathBuf::from("/assets/secret.txt")));
        assert!(caps.file_read_allowed(std::path::Path::new("/assets/secret.txt")));
        // A sibling file under the same directory is not allowed.
        assert!(!caps.file_read_allowed(std::path::Path::new("/assets/other.txt")));
    }

    #[test]
    fn file_read_allowed_real_dir_traversal() {
        // Use the tempdir crate pattern via std::env::temp_dir for a
        // real filesystem traversal test.
        let tmp = std::env::temp_dir().join("martensite_plugin_traversal_test");
        std::fs::create_dir_all(&tmp).unwrap();
        let sub = tmp.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        let secret = tmp.join("secret.txt");
        std::fs::write(&secret, b"x").unwrap();
        let child = sub.join("child.txt");
        std::fs::write(&child, b"y").unwrap();

        let mut caps = CapabilitySet::empty();
        caps.grant(Capability::FileRead(sub.clone()));

        // Child inside the granted dir is allowed.
        assert!(caps.file_read_allowed(&child));
        // Sibling outside the granted dir is rejected even with `..`.
        let escape = sub.join("..").join("secret.txt");
        assert!(!caps.file_read_allowed(&escape));

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn lexical_normalize_strips_dot_and_resolves_dotdot() {
        assert_eq!(
            lexical_normalize(std::path::Path::new("/a/b/./c")),
            std::path::PathBuf::from("/a/b/c")
        );
        assert_eq!(
            lexical_normalize(std::path::Path::new("/a/b/../c")),
            std::path::PathBuf::from("/a/c")
        );
        // `..` that escapes the root is preserved (cannot pop the root
        // component), so a prefix check against the original root fails.
        let escaped = lexical_normalize(std::path::Path::new("/a/../../etc"));
        assert!(!escaped.starts_with(std::path::Path::new("/a")));
    }
}
