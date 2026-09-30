//! `comms` namespace — mail, messaging, phone, broadcast.

use super::super::{IconEntry, IconPair};

/// Qualified icon names (`comms.*`).
///
/// Every name constant in this module is prefixed `COMMS_`
/// (`COMMS_FOO` → `"comms.foo"`) so the flattened
/// `builtin::names` re-export cannot collide.
pub mod names {
    /// `"comms.mail"` — envelope.
    pub const MAIL: &str = "comms.mail";
    /// `"comms.mail-open"` — envelope with raised flap.
    pub const MAIL_OPEN: &str = "comms.mail-open";
    /// `"comms.mail-plus"` — envelope with add badge.
    pub const MAIL_PLUS: &str = "comms.mail-plus";
    /// `"comms.mail-x"` — envelope with remove badge.
    pub const MAIL_X: &str = "comms.mail-x";
    /// `"comms.mail-check"` — envelope with confirm badge.
    pub const MAIL_CHECK: &str = "comms.mail-check";
    /// `"comms.send"` — paper plane.
    pub const SEND: &str = "comms.send";
    /// `"comms.message-circle"` — round speech bubble.
    pub const MESSAGE_CIRCLE: &str = "comms.message-circle";
    /// `"comms.message-square"` — square speech bubble.
    pub const MESSAGE_SQUARE: &str = "comms.message-square";
    /// `"comms.messages-square"` — two square speech bubbles.
    pub const MESSAGES_SQUARE: &str = "comms.messages-square";
    /// `"comms.at-sign"` — mention / address.
    pub const AT_SIGN: &str = "comms.at-sign";
    /// `"comms.rss"` — feed broadcast.
    pub const RSS: &str = "comms.rss";
    /// `"comms.phone"` — handset.
    pub const PHONE: &str = "comms.phone";
    /// `"comms.phone-off"` — handset struck through.
    pub const PHONE_OFF: &str = "comms.phone-off";
    /// `"comms.phone-call"` — ringing handset.
    pub const PHONE_CALL: &str = "comms.phone-call";
    /// `"comms.phone-incoming"` — inbound call.
    pub const PHONE_INCOMING: &str = "comms.phone-incoming";
    /// `"comms.phone-outgoing"` — outbound call.
    pub const PHONE_OUTGOING: &str = "comms.phone-outgoing";
    /// `"comms.phone-missed"` — missed call.
    pub const PHONE_MISSED: &str = "comms.phone-missed";
}

/// `"comms.mail"` — envelope body plus flap.
pub const COMMS_MAIL: &str =
    "M4 4h16a2 2 0 012 2v12a2 2 0 01-2 2H4a2 2 0 01-2-2V6a2 2 0 012-2zM22 7L12 13L2 7";
/// `"comms.mail-open"` — open envelope, flap raised.
pub const COMMS_MAIL_OPEN: &str =
    "M12 3L22 10v9a2 2 0 01-2 2H4a2 2 0 01-2-2v-9L12 3zM22 10L12 16L2 10";
/// `"comms.mail-plus"` — envelope plus add badge.
pub const COMMS_MAIL_PLUS: &str =
    "M22 13V6a2 2 0 00-2-2H4a2 2 0 00-2 2v12a2 2 0 002 2h8M22 7L12 13L2 7M19 16v6M16 19h6";
/// `"comms.mail-x"` — envelope plus remove badge.
pub const COMMS_MAIL_X: &str =
    "M22 13V6a2 2 0 00-2-2H4a2 2 0 00-2 2v12a2 2 0 002 2h8M22 7L12 13L2 7M16.5 16.5l5 5M21.5 16.5l-5 5";
/// `"comms.mail-check"` — envelope plus confirm badge.
pub const COMMS_MAIL_CHECK: &str =
    "M22 13V6a2 2 0 00-2-2H4a2 2 0 00-2 2v12a2 2 0 002 2h8M22 7L12 13L2 7M16 19l2 2 4-4";
/// `"comms.send"` — paper plane with flight line.
pub const COMMS_SEND: &str = "M22 2L11 13M22 2L15 22L11 13L2 4L22 2z";
/// `"comms.message-circle"` — round bubble with tail.
pub const COMMS_MESSAGE_CIRCLE: &str = "M8 20A9 9 0 104 16L2 22z";
/// `"comms.message-square"` — square bubble with tail.
pub const COMMS_MESSAGE_SQUARE: &str = "M21 15a2 2 0 01-2 2H7l-4 4V5a2 2 0 012-2h14a2 2 0 012 2z";
/// `"comms.messages-square"` — stacked bubbles, conversation.
pub const COMMS_MESSAGES_SQUARE: &str =
    "M14 9a2 2 0 01-2 2H6l-4 4V4a2 2 0 012-2h8a2 2 0 012 2zM18 9h2a2 2 0 012 2v10l-4-4h-6a2 2 0 01-2-2v-1";
/// `"comms.at-sign"` — inner loop plus outer ring hook.
pub const COMMS_AT_SIGN: &str =
    "M16 12a4 4 0 11-8 0 4 4 0 018 0zM16 8v5a3 3 0 006 0v-1a9 9 0 10-4.6 7.2";
/// `"comms.rss"` — dot plus two quarter arcs.
pub const COMMS_RSS: &str = "M6 19a1 1 0 11-2 0 1 1 0 012 0zM4 11a9 9 0 019 9M4 4a16 16 0 0116 16";
/// `"comms.phone"` — handset.
pub const COMMS_PHONE: &str =
    "M22 16.92v3a2 2 0 01-2.18 2 19.79 19.79 0 01-8.63-3.07 19.5 19.5 0 01-6-6 19.79 19.79 0 01-3.07-8.67A2 2 0 014.11 2h3a2 2 0 012 1.72 12.84 12.84 0 00.7 2.81 2 2 0 01-.45 2.11L8.09 9.91a16 16 0 006 6l1.27-1.27a2 2 0 012.11-.45 12.84 12.84 0 002.81.7A2 2 0 0122 16.92z";
/// `"comms.phone-off"` — handset plus strike slash.
pub const COMMS_PHONE_OFF: &str =
    "M22 16.92v3a2 2 0 01-2.18 2 19.79 19.79 0 01-8.63-3.07 19.5 19.5 0 01-6-6 19.79 19.79 0 01-3.07-8.67A2 2 0 014.11 2h3a2 2 0 012 1.72 12.84 12.84 0 00.7 2.81 2 2 0 01-.45 2.11L8.09 9.91a16 16 0 006 6l1.27-1.27a2 2 0 012.11-.45 12.84 12.84 0 002.81.7A2 2 0 0122 16.92zM2 2l20 20";
/// `"comms.phone-call"` — handset plus ring arcs.
pub const COMMS_PHONE_CALL: &str =
    "M14 2a8.5 8.5 0 018 8M14 5.5a4.5 4.5 0 014.5 4.5M22 16.92v3a2 2 0 01-2.18 2 19.79 19.79 0 01-8.63-3.07 19.5 19.5 0 01-6-6 19.79 19.79 0 01-3.07-8.67A2 2 0 014.11 2h3a2 2 0 012 1.72 12.84 12.84 0 00.7 2.81 2 2 0 01-.45 2.11L8.09 9.91a16 16 0 006 6l1.27-1.27a2 2 0 012.11-.45 12.84 12.84 0 002.81.7A2 2 0 0122 16.92z";
/// `"comms.phone-incoming"` — handset plus inbound arrow.
pub const COMMS_PHONE_INCOMING: &str =
    "M16 2v6h6M22 2l-6 6M22 16.92v3a2 2 0 01-2.18 2 19.79 19.79 0 01-8.63-3.07 19.5 19.5 0 01-6-6 19.79 19.79 0 01-3.07-8.67A2 2 0 014.11 2h3a2 2 0 012 1.72 12.84 12.84 0 00.7 2.81 2 2 0 01-.45 2.11L8.09 9.91a16 16 0 006 6l1.27-1.27a2 2 0 012.11-.45 12.84 12.84 0 002.81.7A2 2 0 0122 16.92z";
/// `"comms.phone-outgoing"` — handset plus outbound arrow.
pub const COMMS_PHONE_OUTGOING: &str =
    "M22 8V2h-6M16 8l6-6M22 16.92v3a2 2 0 01-2.18 2 19.79 19.79 0 01-8.63-3.07 19.5 19.5 0 01-6-6 19.79 19.79 0 01-3.07-8.67A2 2 0 014.11 2h3a2 2 0 012 1.72 12.84 12.84 0 00.7 2.81 2 2 0 01-.45 2.11L8.09 9.91a16 16 0 006 6l1.27-1.27a2 2 0 012.11-.45 12.84 12.84 0 002.81.7A2 2 0 0122 16.92z";
/// `"comms.phone-missed"` — handset plus top-right cross.
pub const COMMS_PHONE_MISSED: &str =
    "M16 2l6 6M22 2l-6 6M22 16.92v3a2 2 0 01-2.18 2 19.79 19.79 0 01-8.63-3.07 19.5 19.5 0 01-6-6 19.79 19.79 0 01-3.07-8.67A2 2 0 014.11 2h3a2 2 0 012 1.72 12.84 12.84 0 00.7 2.81 2 2 0 01-.45 2.11L8.09 9.91a16 16 0 006 6l1.27-1.27a2 2 0 012.11-.45 12.84 12.84 0 002.81.7A2 2 0 0122 16.92z";

/// `comms` entries — registered in the pack in this order.
pub const ENTRIES: &[IconEntry] = &[
    IconEntry::new(names::MAIL, COMMS_MAIL),
    IconEntry::new(names::MAIL_OPEN, COMMS_MAIL_OPEN),
    IconEntry::new(names::MAIL_PLUS, COMMS_MAIL_PLUS),
    IconEntry::new(names::MAIL_X, COMMS_MAIL_X),
    IconEntry::new(names::MAIL_CHECK, COMMS_MAIL_CHECK),
    IconEntry::new(names::SEND, COMMS_SEND),
    IconEntry::new(names::MESSAGE_CIRCLE, COMMS_MESSAGE_CIRCLE),
    IconEntry::new(names::MESSAGE_SQUARE, COMMS_MESSAGE_SQUARE),
    IconEntry::new(names::MESSAGES_SQUARE, COMMS_MESSAGES_SQUARE),
    IconEntry::new(names::AT_SIGN, COMMS_AT_SIGN),
    IconEntry::new(names::RSS, COMMS_RSS),
    IconEntry::new(names::PHONE, COMMS_PHONE),
    IconEntry::new(names::PHONE_OFF, COMMS_PHONE_OFF),
    IconEntry::new(names::PHONE_CALL, COMMS_PHONE_CALL),
    IconEntry::new(names::PHONE_INCOMING, COMMS_PHONE_INCOMING),
    IconEntry::new(names::PHONE_OUTGOING, COMMS_PHONE_OUTGOING),
    IconEntry::new(names::PHONE_MISSED, COMMS_PHONE_MISSED),
];

/// `comms` morph pairs.
pub const PAIRS: &[IconPair] = &[IconPair::new(names::PHONE, names::PHONE_OFF)];
