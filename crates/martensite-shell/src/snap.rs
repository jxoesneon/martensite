//! Cross-platform window snap-layout abstraction.
//!
//! This module defines [`SnapLayout`], the configuration type that platform
//! backends use to report window tiling / snap-zone support (e.g. Windows 11
//! snap layouts, macOS Stage Manager, GNOME edge tiling).

/// Snap layout configuration for window tiling/arrangement.
///
/// # Examples
///
/// ```
/// use martensite_shell::SnapLayout;
///
/// let snap = SnapLayout::default();
/// assert!(!snap.enabled);
/// ```
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct SnapLayout {
    /// Whether snap layouts are enabled on this platform.
    pub enabled: bool,
    /// Maximum number of snap zones (e.g. 4 quadrants on Win11).
    pub max_zones: u32,
}
