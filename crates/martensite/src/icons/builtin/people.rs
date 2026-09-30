//! `people` namespace — users, groups, identity, accessibility.

use super::super::{IconEntry, IconPair};

/// Qualified icon names (`people.*`).
///
/// Every name constant in this module is prefixed `PEOPLE_`
/// (`PEOPLE_FOO` → `"people.foo"`) so the flattened
/// `builtin::names` re-export cannot collide.
pub mod names {
    /// `"people.user-plus"` — person with add badge.
    pub const USER_PLUS: &str = "people.user-plus";
    /// `"people.user-minus"` — person with remove badge.
    pub const USER_MINUS: &str = "people.user-minus";
    /// `"people.user-x"` — person with delete badge.
    pub const USER_X: &str = "people.user-x";
    /// `"people.user-check"` — person with confirm badge.
    pub const USER_CHECK: &str = "people.user-check";
    /// `"people.users"` — two people, group.
    pub const USERS: &str = "people.users";
    /// `"people.circle-user"` — person inside a circle, avatar.
    pub const CIRCLE_USER: &str = "people.circle-user";
    /// `"people.contact"` — address book card.
    pub const CONTACT: &str = "people.contact";
    /// `"people.accessibility"` — accessibility figure.
    pub const ACCESSIBILITY: &str = "people.accessibility";
    /// `"people.person-standing"` — standing figure.
    pub const PERSON_STANDING: &str = "people.person-standing";
    /// `"people.handshake"` — agreement / partnership.
    pub const HANDSHAKE: &str = "people.handshake";
}

/// `"people.user-plus"` — person plus add badge.
pub const PEOPLE_USER_PLUS: &str =
    "M16 7a4 4 0 11-8 0 4 4 0 018 0zM20 21v-2a4 4 0 00-4-4H8a4 4 0 00-4 4v2M20 9v4M18 11h4";
/// `"people.user-minus"` — person plus remove badge.
pub const PEOPLE_USER_MINUS: &str =
    "M16 7a4 4 0 11-8 0 4 4 0 018 0zM20 21v-2a4 4 0 00-4-4H8a4 4 0 00-4 4v2M18 11h4";
/// `"people.user-x"` — person plus delete badge.
pub const PEOPLE_USER_X: &str =
    "M16 7a4 4 0 11-8 0 4 4 0 018 0zM20 21v-2a4 4 0 00-4-4H8a4 4 0 00-4 4v2M18 9l4 4M22 9l-4 4";
/// `"people.user-check"` — person plus confirm badge.
pub const PEOPLE_USER_CHECK: &str =
    "M16 7a4 4 0 11-8 0 4 4 0 018 0zM20 21v-2a4 4 0 00-4-4H8a4 4 0 00-4 4v2M17 11l2 2 3-3";
/// `"people.users"` — two overlapping people.
pub const PEOPLE_USERS: &str =
    "M12.5 7a4 4 0 11-8 0 4 4 0 018 0zM16 21v-2a4 4 0 00-4-4H6a4 4 0 00-4 4v2M15 3.2a4 4 0 010 7.6M21 21v-2a4 4 0 00-3-3.9";
/// `"people.circle-user"` — person bust inside a circle.
pub const PEOPLE_CIRCLE_USER: &str =
    "M22 12a10 10 0 11-20 0 10 10 0 0120 0zM16 10a4 4 0 11-8 0 4 4 0 018 0zM18 20a6 6 0 00-12 0";
/// `"people.contact"` — address book with spine and person.
pub const PEOPLE_CONTACT: &str =
    "M5 4h14a2 2 0 012 2v12a2 2 0 01-2 2H5a2 2 0 01-2-2V6a2 2 0 012-2zM6.5 4v16M14.5 11a2 2 0 11-4 0 2 2 0 014 0zM16.5 17a4 4 0 00-8 0";
/// `"people.accessibility"` — figure with arms out, seated dynamic.
pub const PEOPLE_ACCESSIBILITY: &str =
    "M13 4a1 1 0 11-2 0 1 1 0 012 0zM5 8.5C8.5 9.8 15.5 9.8 19 8.5M12 9.5V14M9.5 21L12 14l2.5 7";
/// `"people.person-standing"` — upright figure, arms angled up.
pub const PEOPLE_PERSON_STANDING: &str =
    "M13 4.5a1 1 0 11-2 0 1 1 0 012 0zM12 7v7M6 9l6-2 6 2M9 21l3-7 3 7";
/// `"people.handshake"` — two forearms meeting in a clasp.
pub const PEOPLE_HANDSHAKE: &str =
    "M2 8l5-3 5 4M22 8l-5-3-5 4M12 9l3.5 3.5L12 16l-3.5-3.5zM9.7 13.9l2.3-1.4M14.3 13.9l-2.3-1.4";

/// `people` entries — registered in the pack in this order.
pub const ENTRIES: &[IconEntry] = &[
    IconEntry::new(names::USER_PLUS, PEOPLE_USER_PLUS),
    IconEntry::new(names::USER_MINUS, PEOPLE_USER_MINUS),
    IconEntry::new(names::USER_X, PEOPLE_USER_X),
    IconEntry::new(names::USER_CHECK, PEOPLE_USER_CHECK),
    IconEntry::new(names::USERS, PEOPLE_USERS),
    IconEntry::new(names::CIRCLE_USER, PEOPLE_CIRCLE_USER),
    IconEntry::new(names::CONTACT, PEOPLE_CONTACT),
    IconEntry::new(names::ACCESSIBILITY, PEOPLE_ACCESSIBILITY),
    IconEntry::new(names::PERSON_STANDING, PEOPLE_PERSON_STANDING),
    IconEntry::new(names::HANDSHAKE, PEOPLE_HANDSHAKE),
];

/// `people` morph pairs.
pub const PAIRS: &[IconPair] = &[];
