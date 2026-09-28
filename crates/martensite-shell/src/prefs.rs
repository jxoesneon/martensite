//! OS-level user preference probing (ADR-0040 phase 2).
//!
//! Each function is a **one-shot snapshot** — query once at app startup
//! and install the result onto the widget arena (the window layer's
//! `apply_platform_preferences` helper does exactly this beside
//! `set_theme`/`set_scale_factor`). Live-change listening is
//! intentionally out of scope: apps that want to track changes re-query
//! from their own settings-change hooks.
//!
//! Resolution order for every preference:
//!
//! 1. The `MARTENSITE_*` environment override wins — it exists so tests,
//!    CI, and users on platforms without a probe can force the flag.
//! 2. The platform probe (OS settings store / accessibility API).
//! 3. `false` — a platform with no probe yet never reports a phantom
//!    preference.

/// Environment variable consulted by [`prefers_reduced_motion`] before
/// the platform probe.
///
/// Truthy spellings (`1`, `true`, `on`, `yes` — case-insensitive) force
/// reduced motion on; falsy spellings (`0`, `false`, `off`, `no`) force
/// it off even when the OS reports the preference. Unset or
/// unrecognised values defer to the platform probe.
///
/// # Examples
///
/// ```
/// use martensite_shell::prefs::REDUCED_MOTION_ENV;
///
/// assert_eq!(REDUCED_MOTION_ENV, "MARTENSITE_REDUCED_MOTION");
/// ```
pub const REDUCED_MOTION_ENV: &str = "MARTENSITE_REDUCED_MOTION";

/// Returns `true` when the user has asked the OS for reduced motion —
/// the `prefers-reduced-motion` signal that swaps animated loading
/// shimmers for static placeholders via
/// `martensite_core::WidgetArena::set_reduced_motion`.
///
/// Resolution order:
///
/// 1. [`REDUCED_MOTION_ENV`] — explicit override, always consulted first.
/// 2. Platform probe:
///    - **Linux** — `gsettings get org.gnome.desktop.a11y.interface
///      reduce-animation` (GNOME 45+ accessibility toggle), then
///      `org.gnome.desktop.interface enable-animations` (`false` means
///      reduced), then `$XDG_CONFIG_HOME/gtk-{4,3}.0/settings.ini`'s
///      `gtk-enable-animations` (covers KDE/GTK and dconf-less systems).
///      `gsettings` is spawned once per consult; a missing binary or
///      schema is simply "no signal", never an error.
///    - **macOS** — `NSWorkspace.accessibilityDisplayShouldReduceMotion`
///      (requires the `macos-backend` feature for its `objc2` binding).
///    - **Windows** — `SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION)`
///      (requires the `windows-backend` feature).
///    - **wasm/other** — no probe yet; see
///      `martensite-window`'s web backend for `matchMedia` handling.
/// 3. `false`.
///
/// # Examples
///
/// ```
/// let _reduced = martensite_shell::prefs::prefers_reduced_motion();
/// ```
#[must_use]
pub fn prefers_reduced_motion() -> bool {
    if let Some(forced) = env_override() {
        return forced;
    }
    platform_probe::prefers_reduced_motion()
}

/// Reads the [`REDUCED_MOTION_ENV`] override: `Some` forces the
/// preference, `None` defers to the platform probe.
fn env_override() -> Option<bool> {
    let raw = std::env::var(REDUCED_MOTION_ENV).ok()?;
    parse_bool(&raw)
}

/// Shared truthy/falsy parser for environment variables and settings
/// files. Recognises `1/true/on/yes` and `0/false/off/no`
/// (case-insensitive, surrounding whitespace ignored); anything else is
/// `None` — an unparsable value must never masquerade as a preference.
fn parse_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "on" | "yes" => Some(true),
        "0" | "false" | "off" | "no" => Some(false),
        _ => None,
    }
}

/// Linux probe: GNOME/GTK settings. All sources are optional; the probe
/// returns `true` on the first explicit "animations off / reduce on"
/// signal and `false` when no source reports one.
#[cfg(target_os = "linux")]
mod platform_probe {
    use std::path::PathBuf;
    use std::process::{Command, Stdio};

    use super::parse_bool;

    /// Probes GNOME/GTK animation settings for a reduced-motion request.
    pub fn prefers_reduced_motion() -> bool {
        // GNOME 45+ "Reduce Animation" accessibility toggle —
        // `org.gnome.desktop.a11y.interface reduce-animation`.
        if gsettings_bool("org.gnome.desktop.a11y.interface", "reduce-animation") == Some(true) {
            return true;
        }
        // Classic GTK/GNOME key — what the accessibility toggle wrote
        // before the a11y schema existed and what GTK's
        // `gtk-enable-animations` setting still mirrors.
        if gsettings_bool("org.gnome.desktop.interface", "enable-animations") == Some(false) {
            return true;
        }
        // Settings-file fallback: covers dconf-less setups and non-GNOME
        // desktops that configure GTK directly (kde-gtk-config writes
        // gtk-enable-animations into these files).
        gtk_settings_disable_animations()
    }

    /// `gsettings get <schema> <key>` → `Some(true)`/`Some(false)` for a
    /// boolean key, `None` when gsettings, the schema, or the key is
    /// absent (all indistinguishable from "no preference expressed").
    pub(super) fn gsettings_bool(schema: &str, key: &str) -> Option<bool> {
        let out = Command::new("gsettings")
            .args(["get", schema, key])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        parse_bool(&String::from_utf8_lossy(&out.stdout))
    }

    /// Scans the per-user GTK `settings.ini` files for
    /// `gtk-enable-animations=0|false`.
    fn gtk_settings_disable_animations() -> bool {
        gtk_settings_paths()
            .into_iter()
            .filter_map(|path| std::fs::read_to_string(path).ok())
            .filter_map(|content| gtk_enable_animations_ini(&content))
            .any(|enabled| !enabled)
    }

    /// Candidate GTK settings files: `$XDG_CONFIG_HOME/gtk-4.0` and
    /// `gtk-3.0`, with `~/.config` as the XDG fallback root.
    fn gtk_settings_paths() -> Vec<PathBuf> {
        let Some(root) = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        else {
            return Vec::new();
        };
        vec![
            root.join("gtk-4.0").join("settings.ini"),
            root.join("gtk-3.0").join("settings.ini"),
        ]
    }

    /// Extracts `gtk-enable-animations` from a GTK `settings.ini`
    /// `[Settings]` section. Returns `Some(enabled)` when the key is
    /// present, `None` when absent — the caller treats `Some(false)` as
    /// the reduced-motion signal.
    pub(super) fn gtk_enable_animations_ini(content: &str) -> Option<bool> {
        let mut in_settings = false;
        for line in content.lines().map(str::trim) {
            if let Some(section) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
                in_settings = section.trim().eq_ignore_ascii_case("settings");
                continue;
            }
            if !in_settings {
                continue;
            }
            if let Some((key, value)) = line.split_once('=') {
                if key.trim().eq_ignore_ascii_case("gtk-enable-animations") {
                    return parse_bool(value);
                }
            }
        }
        None
    }
}

/// macOS probe: `NSWorkspace.accessibilityDisplayShouldReduceMotion`.
/// Only compiled when `macos-backend` supplies the `objc2` binding —
/// without it there is no AppKit seam and the fallback applies.
#[cfg(all(target_os = "macos", feature = "macos-backend"))]
mod platform_probe {
    pub fn prefers_reduced_motion() -> bool {
        crate::platform_impl::macos::prefers_reduced_motion()
    }
}

/// Windows probe: `SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION)`.
/// Only compiled when `windows-backend` supplies the `windows` crate.
#[cfg(all(target_os = "windows", feature = "windows-backend"))]
mod platform_probe {
    pub fn prefers_reduced_motion() -> bool {
        crate::platform_impl::windows::prefers_reduced_motion()
    }
}

/// Every other target — and macOS/Windows builds without their backend
/// features — has no probe yet. `false` keeps the preference honest:
/// [`REDUCED_MOTION_ENV`](crate::prefs::REDUCED_MOTION_ENV) is the
/// escape hatch on these platforms.
#[cfg(not(any(
    target_os = "linux",
    all(target_os = "macos", feature = "macos-backend"),
    all(target_os = "windows", feature = "windows-backend"),
)))]
mod platform_probe {
    pub fn prefers_reduced_motion() -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_bool_recognises_documented_spellings() {
        for value in ["1", "true", "on", "yes", "TRUE", " Yes ", "ON"] {
            assert_eq!(parse_bool(value), Some(true), "{value:?}");
        }
        for value in ["0", "false", "off", "no", "FALSE", " Off "] {
            assert_eq!(parse_bool(value), Some(false), "{value:?}");
        }
        for value in ["", "reduce", "2", "nope"] {
            assert_eq!(parse_bool(value), None, "{value:?}");
        }
    }

    #[test]
    fn env_override_wins_over_platform_probe() {
        // The only test in this binary that mutates
        // MARTENSITE_REDUCED_MOTION; sibling tests call the platform
        // probe directly so they never observe the override.
        std::env::set_var(REDUCED_MOTION_ENV, "1");
        assert!(prefers_reduced_motion());
        std::env::set_var(REDUCED_MOTION_ENV, "0");
        assert!(!prefers_reduced_motion());
        std::env::remove_var(REDUCED_MOTION_ENV);
    }

    #[test]
    fn platform_probe_resolves_without_panic() {
        // Whatever the host reports — real setting, absent tooling, or
        // the no-probe fallback — the call must return a bool.
        let _ = platform_probe::prefers_reduced_motion();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn gtk_ini_parser_reads_settings_section() {
        use super::platform_probe::gtk_enable_animations_ini;

        let disabled = "[Settings]\ngtk-enable-animations=0\n";
        assert_eq!(gtk_enable_animations_ini(disabled), Some(false));
        let disabled_word = "[Settings]\ngtk-enable-animations = false\n";
        assert_eq!(gtk_enable_animations_ini(disabled_word), Some(false));
        let enabled = "[Settings]\ngtk-enable-animations=1\n";
        assert_eq!(gtk_enable_animations_ini(enabled), Some(true));
        // Keys outside [Settings] must not leak in.
        let wrong_section = "[Other]\ngtk-enable-animations=0\n";
        assert_eq!(gtk_enable_animations_ini(wrong_section), None);
        let missing = "[Settings]\ngtk-theme-name=Adwaita\n";
        assert_eq!(gtk_enable_animations_ini(missing), None);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn gsettings_probe_runs_when_binary_present() {
        use super::platform_probe::gsettings_bool;
        use std::process::Command;

        if Command::new("gsettings")
            .arg("--version")
            .output()
            .map(|o| !o.status.success())
            .unwrap_or(true)
        {
            eprintln!("gsettings not installed; probe-and-skip");
            return;
        }
        // Missing schema → None (not an error); a present key resolves
        // to Some(_) — either is a valid probe outcome.
        assert_eq!(gsettings_bool("org.example.no.such.schema", "key"), None);
        let _ = gsettings_bool("org.gnome.desktop.interface", "enable-animations");
    }
}
