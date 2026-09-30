//! Canonical stroke-icon `d` constants used by doctests and fixtures
//! (ADR-0041). These are plain 24px-grid stroke paths in the
//! lucide/feather idiom — written for this project, not vendored from
//! an icon library. Application icon vocabularies live in the native
//! pack ([`crate::icons::builtin`]).

/// Hamburger menu (three strokes) → close (X).
pub const MENU: &str = "M4 7h16M4 12h16M4 17h16";
/// Close (X).
pub const CLOSE: &str = "M6 6l12 12M18 6L6 18";
/// Play (triangle) → pause (two bars).
pub const PLAY: &str = "M8 5v14l11-7z";
/// Pause (two bars).
pub const PAUSE: &str = "M7 5h4v14H7zM13 5h4v14h-4z";
/// Check mark → cross.
pub const CHECK: &str = "M5 12l5 5 9-10";
/// Speaker (volume on) → muted (speaker + X).
pub const VOLUME_ON: &str = "M4 10v4h4l5 4V6L8 10H4zM16 9c1.2 1 1.2 5 0 6";
/// Muted speaker.
pub const VOLUME_OFF: &str = "M4 10v4h4l5 4V6L8 10H4zM16 9l5 6M21 9l-5 6";
/// Eye open → eye closed (arc-heavy pair — regression fixture).
pub const EYE_OPEN: &str =
    "M2 12c3-5 7-7 10-7s7 2 10 7c-3 5-7 7-10 7s-7-2-10-7zM12 9a3 3 0 100 6 3 3 0 000-6z";
/// Eye closed.
pub const EYE_CLOSED: &str =
    "M4 6l16 12M2 12c3-5 7-7 10-7 1.5 0 3 .4 4.3 1M22 12c-3 5-7 7-10 7-1.5 0-3-.4-4.3-1";
/// Plus → minus.
pub const PLUS: &str = "M12 5v14M5 12h14";
/// Minus.
pub const MINUS: &str = "M5 12h14";
/// Chevron right → chevron down (open/close disclosure).
pub const CHEVRON_RIGHT: &str = "M9 6l6 6-6 6";
/// Chevron down.
pub const CHEVRON_DOWN: &str = "M6 9l6 6 6-6";
/// Lock closed → lock open (dashboard console-lock demo).
pub const LOCK: &str = "M7 11V8a5 5 0 0110 0v3M6 11h12v9H6z";
/// Lock open.
pub const LOCK_OPEN: &str = "M7 11V8a5 5 0 019.5-2M6 11h12v9H6z";
