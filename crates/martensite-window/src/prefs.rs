//! OS-level user-preference propagation into the widget arena
//! (ADR-0040 phase 2).
//!
//! [`apply_platform_preferences`] is the one-shot startup seam: call it
//! once when the `WidgetArena` is built — beside `set_theme`,
//! `set_scale_factor`, and `set_text_painter` — and it mirrors each
//! ambient platform preference onto the arena's matching flag.
//!
//! Live-change listening is intentionally not wired: the consult is a
//! snapshot taken at init. Apps that want to track changes re-call the
//! function from their own settings-change hooks (e.g. after a
//! [`WindowEventOutcome::ThemeAppearanceChanged`]-style shell event).
//!
//! [`WindowEventOutcome::ThemeAppearanceChanged`]: crate::WindowEventOutcome::ThemeAppearanceChanged

use martensite_core::WidgetArena;

/// Consults ambient platform preferences once and installs them on
/// `arena`.
///
/// Currently a single preference: **reduced motion**, mirrored into
/// [`WidgetArena::set_reduced_motion`] so every loading placeholder
/// paints statically instead of shimmering.
///
/// Sources by platform:
///
/// - `MARTENSITE_REDUCED_MOTION` (`1/true/on/yes` force on,
///   `0/false/off/no` force off) overrides the OS probe on native
///   targets — tests, CI, and probe-less platforms use it.
/// - Linux: GNOME/GTK animation settings via `gsettings` and GTK
///   `settings.ini` ([`martensite_shell::prefs::prefers_reduced_motion`]).
/// - macOS: `NSWorkspace.accessibilityDisplayShouldReduceMotion` when
///   `martensite-shell` is built with `macos-backend`.
/// - Windows: `SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION)` when
///   `martensite-shell` is built with `windows-backend`.
/// - wasm: `matchMedia("(prefers-reduced-motion: reduce)")`.
///
/// # Examples
///
/// ```
/// use martensite_core::WidgetArena;
/// use martensite_window::prefs::apply_platform_preferences;
///
/// let mut arena = WidgetArena::new();
/// apply_platform_preferences(&mut arena);
/// // `arena.reduced_motion()` now mirrors the OS preference (or the
/// // MARTENSITE_REDUCED_MOTION override).
/// ```
pub fn apply_platform_preferences(arena: &mut WidgetArena) {
    arena.set_reduced_motion(platform_prefers_reduced_motion());
}

/// Native targets delegate to the shell crate's probe (env override
/// first, then the OS settings store).
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
fn platform_prefers_reduced_motion() -> bool {
    martensite_shell::prefs::prefers_reduced_motion()
}

/// The canonical web signal: the `prefers-reduced-motion` media query.
/// Returns `false` outside a browsing context (no `window`, or a user
/// agent without `matchMedia`).
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
fn platform_prefers_reduced_motion() -> bool {
    web_sys::window()
        .and_then(|w| w.match_media("(prefers-reduced-motion: reduce)").ok())
        .flatten()
        .is_some_and(|query| query.matches())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_platform_preferences_sets_arena_flag() {
        let mut arena = WidgetArena::new();
        assert!(!arena.reduced_motion());
        apply_platform_preferences(&mut arena);
        assert_eq!(arena.reduced_motion(), platform_prefers_reduced_motion());
    }

    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    #[test]
    fn apply_platform_preferences_honors_env_override() {
        // The only test in this binary that mutates
        // MARTENSITE_REDUCED_MOTION.
        std::env::set_var("MARTENSITE_REDUCED_MOTION", "1");
        let mut arena = WidgetArena::new();
        apply_platform_preferences(&mut arena);
        assert!(arena.reduced_motion());
        std::env::remove_var("MARTENSITE_REDUCED_MOTION");
    }
}
