//! `status` namespace — state feedback, toggles, affordances.

use super::super::{IconEntry, IconPair};

/// Qualified icon names (`status.*`).
pub mod names {
    /// `"status.info"` — informational notice.
    pub const INFO: &str = "status.info";
    /// `"status.warning"` — caution notice.
    pub const WARNING: &str = "status.warning";
    /// `"status.error"` — failure notice.
    pub const ERROR: &str = "status.error";
    /// `"status.check"` — affirmative mark.
    pub const CHECK: &str = "status.check";
    /// `"status.close"` — dismiss / destructive mark.
    pub const CLOSE: &str = "status.close";
    /// `"status.plus"` — add affordance.
    pub const PLUS: &str = "status.plus";
    /// `"status.minus"` — remove affordance.
    pub const MINUS: &str = "status.minus";
    /// `"status.bell"` — notifications on.
    pub const BELL: &str = "status.bell";
    /// `"status.bell-off"` — notifications silenced.
    pub const BELL_OFF: &str = "status.bell-off";
    /// `"status.lock"` — secured/locked state.
    pub const LOCK: &str = "status.lock";
    /// `"status.lock-open"` — unsecured/unlocked state.
    pub const LOCK_OPEN: &str = "status.lock-open";
    /// `"status.eye"` — visible.
    pub const EYE: &str = "status.eye";
    /// `"status.eye-off"` — hidden.
    pub const EYE_OFF: &str = "status.eye-off";
    /// `"status.check-circle"` — success notice.
    pub const STATUS_CHECK_CIRCLE: &str = "status.check-circle";
    /// `"status.plus-circle"` — add affordance, circled.
    pub const STATUS_PLUS_CIRCLE: &str = "status.plus-circle";
    /// `"status.minus-circle"` — remove affordance, circled.
    pub const STATUS_MINUS_CIRCLE: &str = "status.minus-circle";
    /// `"status.help-circle"` — help / question notice.
    pub const STATUS_HELP_CIRCLE: &str = "status.help-circle";
    /// `"status.warning-octagon"` — blocking caution notice.
    pub const STATUS_WARNING_OCTAGON: &str = "status.warning-octagon";
    /// `"status.circle-alert"` — alert notice, circled.
    pub const STATUS_CIRCLE_ALERT: &str = "status.circle-alert";
    /// `"status.badge-check"` — verified badge.
    pub const STATUS_BADGE_CHECK: &str = "status.badge-check";
    /// `"status.bell-ring"` — notifications ringing.
    pub const STATUS_BELL_RING: &str = "status.bell-ring";
    /// `"status.bell-dot"` — notifications pending.
    pub const STATUS_BELL_DOT: &str = "status.bell-dot";
    /// `"status.bell-plus"` — subscribe to notifications.
    pub const STATUS_BELL_PLUS: &str = "status.bell-plus";
    /// `"status.star"` — favorite.
    pub const STATUS_STAR: &str = "status.star";
    /// `"status.star-off"` — unfavorite / favorites disabled.
    pub const STATUS_STAR_OFF: &str = "status.star-off";
    /// `"status.heart"` — like / health.
    pub const STATUS_HEART: &str = "status.heart";
    /// `"status.heart-off"` — unlike.
    pub const STATUS_HEART_OFF: &str = "status.heart-off";
    /// `"status.bookmark"` — saved marker.
    pub const STATUS_BOOKMARK: &str = "status.bookmark";
    /// `"status.bookmark-plus"` — add to saved.
    pub const STATUS_BOOKMARK_PLUS: &str = "status.bookmark-plus";
    /// `"status.bookmark-check"` — saved marker, confirmed.
    pub const STATUS_BOOKMARK_CHECK: &str = "status.bookmark-check";
    /// `"status.pin"` — pinned.
    pub const STATUS_PIN: &str = "status.pin";
    /// `"status.pin-off"` — unpinned.
    pub const STATUS_PIN_OFF: &str = "status.pin-off";
    /// `"status.thumbs-up"` — approve.
    pub const STATUS_THUMBS_UP: &str = "status.thumbs-up";
    /// `"status.thumbs-down"` — disapprove.
    pub const STATUS_THUMBS_DOWN: &str = "status.thumbs-down";
    /// `"status.circle-dot"` — selected radio / waypoint.
    pub const STATUS_CIRCLE_DOT: &str = "status.circle-dot";
    /// `"status.loader"` — pending spinner.
    pub const STATUS_LOADER: &str = "status.loader";
    /// `"status.zap"` — energized / quick action.
    pub const STATUS_ZAP: &str = "status.zap";
    /// `"status.zap-off"` — de-energized / flash off.
    pub const STATUS_ZAP_OFF: &str = "status.zap-off";
    /// `"status.flame"` — hot / trending.
    pub const STATUS_FLAME: &str = "status.flame";
    /// `"status.target"` — goal / aim.
    pub const STATUS_TARGET: &str = "status.target";
    /// `"status.crosshair"` — precision targeting.
    pub const STATUS_CROSSHAIR: &str = "status.crosshair";
    /// `"status.signal-zero"` — no signal.
    pub const STATUS_SIGNAL_ZERO: &str = "status.signal-zero";
    /// `"status.signal-low"` — weak signal.
    pub const STATUS_SIGNAL_LOW: &str = "status.signal-low";
    /// `"status.signal-medium"` — fair signal.
    pub const STATUS_SIGNAL_MEDIUM: &str = "status.signal-medium";
    /// `"status.signal-high"` — strong signal.
    pub const STATUS_SIGNAL_HIGH: &str = "status.signal-high";
    /// `"status.flag"` — report / milestone marker.
    pub const STATUS_FLAG: &str = "status.flag";
}

/// `"status.info"` — circle, dot, stem.
pub const STATUS_INFO: &str = "M21 12a9 9 0 11-18 0 9 9 0 0118 0zM12 8h0.01M12 11v5";
/// `"status.warning"` — triangle outline, stem, dot.
pub const STATUS_WARNING: &str =
    "M21.73 18l-8-14a2 2 0 00-3.48 0l-8 14A2 2 0 004 21h16a2 2 0 001.73-3M12 9v4M12 17h0.01";
/// `"status.error"` — circle plus cross.
pub const STATUS_ERROR: &str = "M21 12a9 9 0 11-18 0 9 9 0 0118 0zM9 9l6 6M15 9l-6 6";
/// `"status.check"` — tick.
pub const STATUS_CHECK: &str = "M5 12l5 5 9-10";
/// `"status.close"` — diagonal cross.
pub const STATUS_CLOSE: &str = "M6 6l12 12M18 6L6 18";
/// `"status.plus"`.
pub const STATUS_PLUS: &str = "M12 5v14M5 12h14";
/// `"status.minus"`.
pub const STATUS_MINUS: &str = "M5 12h14";
/// `"status.bell"` — dome plus clapper.
pub const STATUS_BELL: &str = "M18 8a6 6 0 00-12 0c0 7-3 9-3 9h18s-3-2-3-9M13.7 21a2 2 0 01-3.4 0";
/// `"status.bell-off"` — broken dome, clapper, strike slash.
pub const STATUS_BELL_OFF: &str = "M8.7 3a6 6 0 019.3 5c.1 1.7.4 3.3.8 4.8M6.3 6.3C6.1 6.9 6 7.4 6 8c0 7-3 9-3 9h14.6M10.3 21a2 2 0 003.4 0M2 2l20 20";
/// `"status.lock"` — shackle plus body.
pub const STATUS_LOCK: &str = "M7 11V8a5 5 0 0110 0v3M6 11h12v9H6z";
/// `"status.lock-open"` — released shackle plus body.
pub const STATUS_LOCK_OPEN: &str = "M7 11V8a5 5 0 019.5-2M6 11h12v9H6z";
/// `"status.eye"` — eye outline plus pupil.
pub const STATUS_EYE: &str =
    "M2 12c3-5 7-7 10-7s7 2 10 7c-3 5-7 7-10 7s-7-2-10-7zM12 9a3 3 0 100 6 3 3 0 000-6z";
/// `"status.eye-off"` — strike slash over a dimmed eye.
pub const STATUS_EYE_OFF: &str =
    "M4 6l16 12M2 12c3-5 7-7 10-7 1.5 0 3 .4 4.3 1M22 12c-3 5-7 7-10 7-1.5 0-3-.4-4.3-1";
/// `"status.check-circle"` — circle plus tick.
pub const STATUS_CHECK_CIRCLE: &str = "M21 12a9 9 0 11-18 0 9 9 0 0118 0zM9 12L11 14L15 10";
/// `"status.plus-circle"` — circle plus add mark.
pub const STATUS_PLUS_CIRCLE: &str = "M21 12a9 9 0 11-18 0 9 9 0 0118 0zM12 8V16M8 12H16";
/// `"status.minus-circle"` — circle plus remove mark.
pub const STATUS_MINUS_CIRCLE: &str = "M21 12a9 9 0 11-18 0 9 9 0 0118 0zM8 12H16";
/// `"status.help-circle"` — circle plus question mark.
pub const STATUS_HELP_CIRCLE: &str =
    "M21 12a9 9 0 11-18 0 9 9 0 0118 0zM9.25 9A3 3 0 0115 10C15 12 12 13 12 14M12 17h.01";
/// `"status.warning-octagon"` — octagon, stem, dot.
pub const STATUS_WARNING_OCTAGON: &str =
    "M7.75 2H16.25L22 7.75V16.25L16.25 22H7.75L2 16.25V7.75ZM12 8V12M12 16h.01";
/// `"status.circle-alert"` — circle, stem, dot.
pub const STATUS_CIRCLE_ALERT: &str = "M21 12a9 9 0 11-18 0 9 9 0 0118 0zM12 8V12M12 16h.01";
/// `"status.badge-check"` — twelve-point seal plus tick.
pub const STATUS_BADGE_CHECK: &str = "M12 2.25L14 5L16.75 3.5L17.25 7L20.5 7.25L19 10.25L21.75 12L19 13.75L20.5 16.75L17.25 17.25L16.75 20.5L14 19L12 21.75L10 19L7.25 20.5L6.75 17.25L3.5 16.75L5 13.75L2.25 12L5 10.25L3.5 7.25L6.75 7L7.25 3.5L10 5ZM9 12L11 14L15 10";
/// `"status.bell-ring"` — dome, clapper, sound waves.
pub const STATUS_BELL_RING: &str = "M18 8a6 6 0 00-12 0c0 7-3 9-3 9h18s-3-2-3-9M13.7 21a2 2 0 01-3.4 0M4 2C3 4 2 6 2 8M20 2C21 4 22 6 22 8";
/// `"status.bell-dot"` — dome, clapper, pending dot.
pub const STATUS_BELL_DOT: &str =
    "M18 8a6 6 0 00-12 0c0 7-3 9-3 9h18s-3-2-3-9M13.7 21a2 2 0 01-3.4 0M18 20h.01";
/// `"status.bell-plus"` — dome, clapper, add mark.
pub const STATUS_BELL_PLUS: &str =
    "M18 8a6 6 0 00-12 0c0 7-3 9-3 9h18s-3-2-3-9M13.7 21a2 2 0 01-3.4 0M12 7V13M9 10H15";
/// `"status.star"` — five-point star.
pub const STATUS_STAR: &str =
    "M12 2L15.09 8.26L22 9.27L17 14.14L18.18 21.02L12 17.77L5.82 21.02L7 14.14L2 9.27L8.91 8.26Z";
/// `"status.star-off"` — star plus strike slash.
pub const STATUS_STAR_OFF: &str = "M12 2L15.09 8.26L22 9.27L17 14.14L18.18 21.02L12 17.77L5.82 21.02L7 14.14L2 9.27L8.91 8.26ZM2 2L22 22";
/// `"status.heart"` — heart outline.
pub const STATUS_HEART: &str = "M19 14C20.5 12.5 22 10.75 22 8.5A5.5 5.5 0 0016.5 3C14.75 3 13.5 3.5 12 5C10.5 3.5 9.25 3 7.5 3A5.5 5.5 0 002 8.5C2 10.75 3.5 12.5 5 14L12 21Z";
/// `"status.heart-off"` — heart plus strike slash.
pub const STATUS_HEART_OFF: &str = "M19 14C20.5 12.5 22 10.75 22 8.5A5.5 5.5 0 0016.5 3C14.75 3 13.5 3.5 12 5C10.5 3.5 9.25 3 7.5 3A5.5 5.5 0 002 8.5C2 10.75 3.5 12.5 5 14L12 21ZM2 2L22 22";
/// `"status.bookmark"` — ribbon marker.
pub const STATUS_BOOKMARK: &str = "M7 3A2 2 0 005 5V21L12 17L19 21V5A2 2 0 0017 3Z";
/// `"status.bookmark-plus"` — ribbon plus add mark.
pub const STATUS_BOOKMARK_PLUS: &str =
    "M7 3A2 2 0 005 5V21L12 17L19 21V5A2 2 0 0017 3ZM12 7V13M9 10H15";
/// `"status.bookmark-check"` — ribbon plus tick.
pub const STATUS_BOOKMARK_CHECK: &str =
    "M7 3A2 2 0 005 5V21L12 17L19 21V5A2 2 0 0017 3ZM9 10L11 12L15 8";
/// `"status.pin"` — pushpin, head plus skirt plus stem.
pub const STATUS_PIN: &str = "M9 3H15V8L19 13V15H5V13L9 8ZM12 15V21";
/// `"status.pin-off"` — pushpin plus strike slash.
pub const STATUS_PIN_OFF: &str = "M9 3H15V8L19 13V15H5V13L9 8ZM12 15V21M2 2L22 22";
/// `"status.thumbs-up"` — thumb raised, cuff seam.
pub const STATUS_THUMBS_UP: &str = "M7 10V22M15 6L14 10H20A2 2 0 0122 12.5L19.75 20.5A2 2 0 0117.5 22H4A2 2 0 012 20V12A2 2 0 014 10H6.75A2 2 0 008.5 9L12 2A3 3 0 0115 6Z";
/// `"status.thumbs-down"` — thumb lowered, cuff seam.
pub const STATUS_THUMBS_DOWN: &str = "M7 2V12M15 18L14 14H20A2 2 0 0022 11.5L19.75 3.5A2 2 0 0017.5 2H4A2 2 0 002 4V12A2 2 0 002 14H6.75A2 2 0 018.5 15L12 22A3 3 0 0015 18Z";
/// `"status.circle-dot"` — circle plus centered dot.
pub const STATUS_CIRCLE_DOT: &str = "M21 12a9 9 0 11-18 0 9 9 0 0118 0zM12 12h.01";
/// `"status.loader"` — three-arc spinner.
pub const STATUS_LOADER: &str =
    "M5.75 5.75A9 9 0 0118.25 5.75M20.75 9.75A9 9 0 0114.25 20.75M9.75 20.75A9 9 0 013.25 9.75";
/// `"status.zap"` — lightning bolt.
pub const STATUS_ZAP: &str = "M13 2L3 14H12L11 22L21 10H12L13 2Z";
/// `"status.zap-off"` — bolt plus strike slash.
pub const STATUS_ZAP_OFF: &str = "M13 2L3 14H12L11 22L21 10H12L13 2ZM2 2L22 22";
/// `"status.flame"` — flame outline with inner curl.
pub const STATUS_FLAME: &str = "M8.5 14.5A2.5 2.5 0 0011 12C11 10.5 10.5 10 10 9C9 6.75 9.75 5 12 3C12.5 5.5 14 8 16 9.5C18 11 19 13 19 15A7 7 0 015 15C5 13.75 5.5 12.5 6 12A2.5 2.5 0 008.5 14.5Z";
/// `"status.target"` — concentric rings plus center dot.
pub const STATUS_TARGET: &str =
    "M21 12a9 9 0 11-18 0 9 9 0 0118 0zM17 12a5 5 0 11-10 0 5 5 0 0110 0zM12 12h.01";
/// `"status.crosshair"` — ring plus four ticks.
pub const STATUS_CROSSHAIR: &str =
    "M20 12a8 8 0 11-16 0 8 8 0 0116 0zM12 2V6M12 18V22M2 12H6M18 12H22";
/// `"status.signal-zero"` — baseline dot only.
pub const STATUS_SIGNAL_ZERO: &str = "M2 20h.01";
/// `"status.signal-low"` — dot plus one bar.
pub const STATUS_SIGNAL_LOW: &str = "M2 20h.01M7 20V16";
/// `"status.signal-medium"` — dot plus two bars.
pub const STATUS_SIGNAL_MEDIUM: &str = "M2 20h.01M7 20V16M12 20V12";
/// `"status.signal-high"` — dot plus four bars.
pub const STATUS_SIGNAL_HIGH: &str = "M2 20h.01M7 20V16M12 20V12M17 20V8M22 4V20";
/// `"status.flag"` — pole plus waving cloth.
pub const STATUS_FLAG: &str =
    "M5 22V2M5 4C8 2.5 10 2.5 13 4C16 5.5 18 5.5 21 4V13C18 14.5 16 14.5 13 13C10 11.5 8 11.5 5 13";

/// `status` entries.
pub const ENTRIES: &[IconEntry] = &[
    IconEntry::new(names::INFO, STATUS_INFO),
    IconEntry::new(names::WARNING, STATUS_WARNING),
    IconEntry::new(names::ERROR, STATUS_ERROR),
    IconEntry::new(names::CHECK, STATUS_CHECK),
    IconEntry::new(names::CLOSE, STATUS_CLOSE),
    IconEntry::new(names::PLUS, STATUS_PLUS),
    IconEntry::new(names::MINUS, STATUS_MINUS),
    IconEntry::new(names::BELL, STATUS_BELL),
    IconEntry::new(names::BELL_OFF, STATUS_BELL_OFF),
    IconEntry::new(names::LOCK, STATUS_LOCK),
    IconEntry::new(names::LOCK_OPEN, STATUS_LOCK_OPEN),
    IconEntry::new(names::EYE, STATUS_EYE),
    IconEntry::new(names::EYE_OFF, STATUS_EYE_OFF),
    IconEntry::new(names::STATUS_CHECK_CIRCLE, STATUS_CHECK_CIRCLE),
    IconEntry::new(names::STATUS_PLUS_CIRCLE, STATUS_PLUS_CIRCLE),
    IconEntry::new(names::STATUS_MINUS_CIRCLE, STATUS_MINUS_CIRCLE),
    IconEntry::new(names::STATUS_HELP_CIRCLE, STATUS_HELP_CIRCLE),
    IconEntry::new(names::STATUS_WARNING_OCTAGON, STATUS_WARNING_OCTAGON),
    IconEntry::new(names::STATUS_CIRCLE_ALERT, STATUS_CIRCLE_ALERT),
    IconEntry::new(names::STATUS_BADGE_CHECK, STATUS_BADGE_CHECK),
    IconEntry::new(names::STATUS_BELL_RING, STATUS_BELL_RING),
    IconEntry::new(names::STATUS_BELL_DOT, STATUS_BELL_DOT),
    IconEntry::new(names::STATUS_BELL_PLUS, STATUS_BELL_PLUS),
    IconEntry::new(names::STATUS_STAR, STATUS_STAR),
    IconEntry::new(names::STATUS_STAR_OFF, STATUS_STAR_OFF),
    IconEntry::new(names::STATUS_HEART, STATUS_HEART),
    IconEntry::new(names::STATUS_HEART_OFF, STATUS_HEART_OFF),
    IconEntry::new(names::STATUS_BOOKMARK, STATUS_BOOKMARK),
    IconEntry::new(names::STATUS_BOOKMARK_PLUS, STATUS_BOOKMARK_PLUS),
    IconEntry::new(names::STATUS_BOOKMARK_CHECK, STATUS_BOOKMARK_CHECK),
    IconEntry::new(names::STATUS_PIN, STATUS_PIN),
    IconEntry::new(names::STATUS_PIN_OFF, STATUS_PIN_OFF),
    IconEntry::new(names::STATUS_THUMBS_UP, STATUS_THUMBS_UP),
    IconEntry::new(names::STATUS_THUMBS_DOWN, STATUS_THUMBS_DOWN),
    IconEntry::new(names::STATUS_CIRCLE_DOT, STATUS_CIRCLE_DOT),
    IconEntry::new(names::STATUS_LOADER, STATUS_LOADER),
    IconEntry::new(names::STATUS_ZAP, STATUS_ZAP),
    IconEntry::new(names::STATUS_ZAP_OFF, STATUS_ZAP_OFF),
    IconEntry::new(names::STATUS_FLAME, STATUS_FLAME),
    IconEntry::new(names::STATUS_TARGET, STATUS_TARGET),
    IconEntry::new(names::STATUS_CROSSHAIR, STATUS_CROSSHAIR),
    IconEntry::new(names::STATUS_SIGNAL_ZERO, STATUS_SIGNAL_ZERO),
    IconEntry::new(names::STATUS_SIGNAL_LOW, STATUS_SIGNAL_LOW),
    IconEntry::new(names::STATUS_SIGNAL_MEDIUM, STATUS_SIGNAL_MEDIUM),
    IconEntry::new(names::STATUS_SIGNAL_HIGH, STATUS_SIGNAL_HIGH),
    IconEntry::new(names::STATUS_FLAG, STATUS_FLAG),
];

/// `status` morph pairs.
pub const PAIRS: &[IconPair] = &[
    IconPair::new(names::LOCK, names::LOCK_OPEN),
    IconPair::new(names::BELL, names::BELL_OFF),
    IconPair::new(names::EYE, names::EYE_OFF),
    IconPair::new(names::PLUS, names::MINUS),
    IconPair::new(names::CHECK, names::CLOSE),
    IconPair::new(names::STATUS_CHECK_CIRCLE, names::ERROR),
    IconPair::new(names::STATUS_PLUS_CIRCLE, names::STATUS_MINUS_CIRCLE),
    IconPair::new(names::STATUS_STAR, names::STATUS_STAR_OFF),
    IconPair::new(names::STATUS_HEART, names::STATUS_HEART_OFF),
    IconPair::new(names::STATUS_PIN, names::STATUS_PIN_OFF),
    IconPair::new(names::STATUS_THUMBS_UP, names::STATUS_THUMBS_DOWN),
    IconPair::new(names::STATUS_ZAP, names::STATUS_ZAP_OFF),
    IconPair::new(names::STATUS_SIGNAL_HIGH, names::STATUS_SIGNAL_ZERO),
];
