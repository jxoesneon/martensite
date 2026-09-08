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
}
