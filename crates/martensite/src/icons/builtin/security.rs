//! `security` namespace — locks, keys, shields, credentials.

use super::super::{IconEntry, IconPair};

/// Qualified icon names (`security.*`).
///
/// Every name constant in this module is prefixed `SECURITY_`
/// (`SECURITY_FOO` → `"security.foo"`) so the flattened
/// `builtin::names` re-export cannot collide.
pub mod names {
    /// `"security.key"` — skeleton key, diagonal.
    pub const SECURITY_KEY: &str = "security.key";
    /// `"security.key-round"` — key with round bow, vertical.
    pub const SECURITY_KEY_ROUND: &str = "security.key-round";
    /// `"security.fingerprint"` — biometric ridges.
    pub const SECURITY_FINGERPRINT: &str = "security.fingerprint";
    /// `"security.scan-face"` — face-recognition frame.
    pub const SECURITY_SCAN_FACE: &str = "security.scan-face";
    /// `"security.scan-line"` — scanning frame.
    pub const SECURITY_SCAN_LINE: &str = "security.scan-line";
    /// `"security.shield"` — protection.
    pub const SECURITY_SHIELD: &str = "security.shield";
    /// `"security.shield-check"` — protected / verified.
    pub const SECURITY_SHIELD_CHECK: &str = "security.shield-check";
    /// `"security.shield-off"` — protection disabled.
    pub const SECURITY_SHIELD_OFF: &str = "security.shield-off";
    /// `"security.shield-alert"` — protection warning.
    pub const SECURITY_SHIELD_ALERT: &str = "security.shield-alert";
    /// `"security.shield-x"` — protection failed.
    pub const SECURITY_SHIELD_X: &str = "security.shield-x";
    /// `"security.shield-half"` — partial protection.
    pub const SECURITY_SHIELD_HALF: &str = "security.shield-half";
    /// `"security.lock-keyhole"` — secured, keyed.
    pub const SECURITY_LOCK_KEYHOLE: &str = "security.lock-keyhole";
    /// `"security.lock-keyhole-open"` — unsecured, keyed.
    pub const SECURITY_LOCK_KEYHOLE_OPEN: &str = "security.lock-keyhole-open";
    /// `"security.door-open"` — open entry.
    pub const SECURITY_DOOR_OPEN: &str = "security.door-open";
    /// `"security.door-closed"` — closed entry.
    pub const SECURITY_DOOR_CLOSED: &str = "security.door-closed";
}

/// `"security.key"` — bow circle, shaft, two teeth.
pub const SECURITY_KEY: &str =
    "M12.5 16A4.5 4.5 0 113.5 16A4.5 4.5 0 0112.5 16ZM11 13L21 3M17.5 6.5L20 9M20 4L22 6";
/// `"security.key-round"` — round bow, shaft, teeth.
pub const SECURITY_KEY_ROUND: &str = "M16 18A4 4 0 118 18A4 4 0 0116 18ZM12 2V14M12 5H16M12 8H15";
/// `"security.fingerprint"` — stacked ridge arcs.
pub const SECURITY_FINGERPRINT: &str = "M12 10A2 2 0 0010 12C10 13.5 9.75 15.5 9.25 17.5M12 7A5 5 0 007 12C7 14 6.5 16 6 17.5M12 7A5 5 0 0117 12C17 14 17.5 16 18 18M5 9.5A7 7 0 0119 9.5M4.25 12C4.25 14.25 4.75 16.5 5.75 18.5M19.75 12C19.75 14.25 19.25 16.5 18.25 18.5";
/// `"security.scan-face"` — corner brackets, eyes, smile.
pub const SECURITY_SCAN_FACE: &str = "M3 7V5A2 2 0 015 3H7M17 3H19A2 2 0 0121 5V7M21 17V19A2 2 0 0119 21H17M7 21H5A2 2 0 013 19V17M9 10h.01M15 10h.01M9 15C10 16 11 16.5 12 16.5C13 16.5 14 16 15 15";
/// `"security.scan-line"` — corner brackets, scan bar.
pub const SECURITY_SCAN_LINE: &str = "M3 7V5A2 2 0 015 3H7M17 3H19A2 2 0 0121 5V7M21 17V19A2 2 0 0119 21H17M7 21H5A2 2 0 013 19V17M7 12H17";
/// `"security.shield"` — shield outline.
pub const SECURITY_SHIELD: &str = "M12 22C12 22 20 18 20 12V5L12 2L4 5V12C4 18 12 22 12 22Z";
/// `"security.shield-check"` — shield plus tick.
pub const SECURITY_SHIELD_CHECK: &str =
    "M12 22C12 22 20 18 20 12V5L12 2L4 5V12C4 18 12 22 12 22ZM9 12L11 14L15 10";
/// `"security.shield-off"` — shield plus strike slash.
pub const SECURITY_SHIELD_OFF: &str =
    "M12 22C12 22 20 18 20 12V5L12 2L4 5V12C4 18 12 22 12 22ZM2 2L22 22";
/// `"security.shield-alert"` — shield, stem, dot.
pub const SECURITY_SHIELD_ALERT: &str =
    "M12 22C12 22 20 18 20 12V5L12 2L4 5V12C4 18 12 22 12 22ZM12 8V12M12 16h.01";
/// `"security.shield-x"` — shield plus cross.
pub const SECURITY_SHIELD_X: &str =
    "M12 22C12 22 20 18 20 12V5L12 2L4 5V12C4 18 12 22 12 22ZM9.5 9.5L14.5 14.5M14.5 9.5L9.5 14.5";
/// `"security.shield-half"` — shield plus center split.
pub const SECURITY_SHIELD_HALF: &str =
    "M12 22C12 22 20 18 20 12V5L12 2L4 5V12C4 18 12 22 12 22ZM12 2V22";
/// `"security.lock-keyhole"` — shackle, body, keyhole.
pub const SECURITY_LOCK_KEYHOLE: &str =
    "M6 11H18V20H6ZM7 11V8A5 5 0 0117 8V11M12 14.5h.01M12 16.5V18";
/// `"security.lock-keyhole-open"` — released shackle, body, keyhole.
pub const SECURITY_LOCK_KEYHOLE_OPEN: &str =
    "M6 11H18V20H6ZM7 11V8A5 5 0 0116.5 6M12 14.5h.01M12 16.5V18";
/// `"security.door-open"` — frame plus swung slab.
pub const SECURITY_DOOR_OPEN: &str =
    "M19 21V5A2 2 0 0017 3H7A2 2 0 005 5V21M5 5L13 7V19L5 17ZM3 21H21";
/// `"security.door-closed"` — frame plus handle dot.
pub const SECURITY_DOOR_CLOSED: &str = "M5 21V5A2 2 0 017 3H17A2 2 0 0119 5V21M3 21H21M14.5 12h.01";

/// `security` entries.
pub const ENTRIES: &[IconEntry] = &[
    IconEntry::new(names::SECURITY_KEY, SECURITY_KEY),
    IconEntry::new(names::SECURITY_KEY_ROUND, SECURITY_KEY_ROUND),
    IconEntry::new(names::SECURITY_FINGERPRINT, SECURITY_FINGERPRINT),
    IconEntry::new(names::SECURITY_SCAN_FACE, SECURITY_SCAN_FACE),
    IconEntry::new(names::SECURITY_SCAN_LINE, SECURITY_SCAN_LINE),
    IconEntry::new(names::SECURITY_SHIELD, SECURITY_SHIELD),
    IconEntry::new(names::SECURITY_SHIELD_CHECK, SECURITY_SHIELD_CHECK),
    IconEntry::new(names::SECURITY_SHIELD_OFF, SECURITY_SHIELD_OFF),
    IconEntry::new(names::SECURITY_SHIELD_ALERT, SECURITY_SHIELD_ALERT),
    IconEntry::new(names::SECURITY_SHIELD_X, SECURITY_SHIELD_X),
    IconEntry::new(names::SECURITY_SHIELD_HALF, SECURITY_SHIELD_HALF),
    IconEntry::new(names::SECURITY_LOCK_KEYHOLE, SECURITY_LOCK_KEYHOLE),
    IconEntry::new(
        names::SECURITY_LOCK_KEYHOLE_OPEN,
        SECURITY_LOCK_KEYHOLE_OPEN,
    ),
    IconEntry::new(names::SECURITY_DOOR_OPEN, SECURITY_DOOR_OPEN),
    IconEntry::new(names::SECURITY_DOOR_CLOSED, SECURITY_DOOR_CLOSED),
];

/// `security` morph pairs.
pub const PAIRS: &[IconPair] = &[
    IconPair::new(names::SECURITY_SHIELD, names::SECURITY_SHIELD_OFF),
    IconPair::new(names::SECURITY_SHIELD_CHECK, names::SECURITY_SHIELD_X),
    IconPair::new(
        names::SECURITY_LOCK_KEYHOLE,
        names::SECURITY_LOCK_KEYHOLE_OPEN,
    ),
    IconPair::new(names::SECURITY_DOOR_OPEN, names::SECURITY_DOOR_CLOSED),
];
