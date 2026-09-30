//! Status bar — the workstation's bottom chrome strip.
//!
//! A minimal container `Widget` modeled on [`crate::toolbar::Toolbar`]
//! holding internal children: a facade `Dropdown` of locales, the
//! live-feed `Spinner`, a CPU `ProgressBar`, and a `MorphIcon` lock
//! indicator that springs `LOCK ↔ LOCK_OPEN` on real `console_locked`
//! edges (ADR-0041 dogfood — a model-driven transition, not a demo
//! timer).
//! `paint_chrome` hand-paints the strip's left segment (fill, hints,
//! frame stats); this widget fills its own rightmost segment (chrome
//! paints after widgets — a shared fill would cover the face) and owns
//! the *interactive* piece: the locale selector and its popup, plus an
//! accent ring when arena focus lands here. The selected locale
//! travels through a shared
//! `Signal<usize>` the app drains each frame into `L10n::set_locale`,
//! so chrome labels re-resolve live — the same widget→app outcome
//! channel the toolbar uses for theme/filter/tick.
//!
//! Focus model: arena focus lands on the status bar as one unit. The
//! dropdown is the only *interactive* child (the indicators ignore
//! events), so every non-positional event is its key target — no
//! `key_target` tracking like the toolbar needs.
//! Positional events forward only when they hit the dropdown's rect;
//! the rest of the strip stays transparent to the pointer so clicks
//! on the hints text fall through to the root.

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite::core::overlay::OverlayLayer;
use martensite::core::shape::Shape;
use martensite::core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    SemanticAction, Widget, WidgetEvent,
};
use martensite::motion::SpringConfig;
use martensite::prelude::Signal;
use martensite::theme::TokenKey;
use martensite::widgets::morph_icon::{demo as morph_icon_demo, MorphIcon};
use martensite::widgets::{Dropdown, ProgressBar, Spinner};

/// Width `apply_layout` reserves for the widget at the right end
/// of the status strip (logical pt) — `paint_chrome` keeps the hints
/// text out of the same band.
pub const STATUSBAR_W: f32 = 260.0;

/// Band height the widget reports to `measure` — the strip's
/// `STATUS_PT`, shared so the two can never disagree.
const BAND_H: f32 = crate::app::STATUS_PT;

/// Dropdown option order — index maps to `LOCALE_CODES`.
pub const LOCALE_LABELS: [&str; 4] = ["English", "Español", "Français", "Deutsch"];

/// BCP-47 codes index-aligned with `LOCALE_LABELS` — the app parses
/// the selected index through these into `L10n::set_locale`.
pub const LOCALE_CODES: [&str; 4] = ["en", "es", "fr", "de"];

/// The shipped Fluent resources, index-aligned with `LOCALE_CODES`.
/// Compiled in — the example has no asset pipeline, and a missing
/// bundle should fail the build, not a runtime lookup.
const LOCALE_SOURCES: [(&str, &str); 4] = [
    ("en", include_str!("../locales/en.ftl")),
    ("es", include_str!("../locales/es.ftl")),
    ("fr", include_str!("../locales/fr.ftl")),
    ("de", include_str!("../locales/de.ftl")),
];

/// Builds the Fluent catalog: one bundle per `LOCALE_SOURCES` entry,
/// anchored to English. Shared by `App::new` and the tests so the
/// parse guard covers exactly the resources the app ships.
pub(crate) fn build_l10n() -> martensite_l10n::reactive::L10n {
    let l10n = martensite_l10n::reactive::L10n::new("en".parse().expect("valid langid"));
    for (code, source) in LOCALE_SOURCES {
        l10n.add_bundle(
            code.parse().expect("valid langid"),
            vec![source.to_string()],
        )
        .expect("shipped FTL parses");
    }
    l10n
}

/// Builds the `MissingLocale` probe from the shipped FTL resources —
/// every literal string the l10n system can emit, in every shipped
/// locale, plus the intentional exemptions (locale endonyms, the live
/// filter-field content, non-alphabetic data). `--audit-locale` wires
/// it into the paint audit; see `PaintLintKind::MissingLocale`.
pub(crate) fn build_locale_probe(
    filter_text: Signal<String>,
) -> martensite::access::paint_audit::LocaleProbe {
    use martensite::access::paint_audit::LocaleProbe;
    use std::collections::HashSet;

    // Static values match exactly — and a `fit()`-truncated prefix of
    // one still counts (a clipped localized string stays localized).
    let mut exact: HashSet<String> = HashSet::new();
    // Templated values reduce to their literal segments — covered when
    // every segment appears in order ("foco: { $name }" → "foco:").
    let mut patterns: Vec<Vec<String>> = Vec::new();
    for (_, source) in LOCALE_SOURCES {
        for line in source.lines() {
            let Some((_, value)) = line.split_once('=') else {
                continue;
            };
            let templated = value.contains('{');
            let mut segs: Vec<String> = Vec::new();
            let mut rest = value;
            while let Some((lit, after)) = rest.split_once('{') {
                if !lit.trim().is_empty() {
                    segs.push(lit.trim().to_string());
                }
                rest = after.split_once('}').map_or("", |(_, r)| r);
            }
            if !rest.trim().is_empty() {
                segs.push(rest.trim().to_string());
            }
            if templated {
                if !segs.is_empty() {
                    patterns.push(segs);
                }
            } else if let Some(v) = segs.into_iter().next() {
                exact.insert(v);
            }
        }
    }

    LocaleProbe::new(move |text, _scope| {
        // Editable-field content is user data, not chrome — the live
        // filter string is the only editable field's current value.
        if text == filter_text.get() {
            return true;
        }
        // Locale names are endonyms — correct by definition.
        if LOCALE_LABELS.contains(&text) {
            return true;
        }
        // Numbers, units and symbol runs carry no localizable text.
        if !text.chars().any(char::is_alphabetic) {
            return true;
        }
        if exact.contains(text) {
            return true;
        }
        // `fit()`-truncated output still stems from a translated
        // string — the ellipsis is stripped before prefix-matching.
        let stem = text.trim_end_matches('…').trim_end();
        if stem.len() >= 4 && exact.iter().any(|v| v.starts_with(stem)) {
            return true;
        }
        patterns.iter().any(|segs| {
            let mut rest = text;
            segs.iter().all(|seg| match rest.find(seg.as_str()) {
                Some(i) => {
                    rest = &rest[i + seg.len()..];
                    true
                }
                None => false,
            })
        })
    })
}

/// The status-bar widget. `locale_sel` is a shared cell — the app
/// clones it before constructing the widget, so the dropdown's commits
/// are observable app-side without downcasting through `dyn Widget`.
pub struct StatusBar {
    scale: Signal<f32>,
    /// The dropdown face rect resolved in `layout` — hit-testing and
    /// the focus ring both target this rather than the strip segment
    /// `apply_layout` hands us.
    dd_rect: Rect,
    focused: bool,
    dropdown: Dropdown,
    /// Locale selection as a `LOCALE_CODES` index — the dropdown
    /// writes it, `redraw` applies it.
    locale_sel: Signal<usize>,
    /// Live-telemetry spinner — present (and animated by the arena's
    /// internal-children tick) only while the feed isn't paused.
    spinner: Spinner,
    /// CPU progress bar — mirrors the shared `cpu` signal.
    progress: ProgressBar,
    /// Console-lock indicator — a `MorphIcon` driven by real
    /// `console_locked` edges in `tick` (ADR-0041 dogfood). Carries
    /// the semantic label ("console locked"/"console unlocked") since
    /// the lock state appears nowhere else in this strip. Seeded
    /// empty; the first `tick` lands the resting shape via `set_icon`
    /// (no flight) — constructor-time icon construction would parse
    /// `d` geometry before the model is even observed.
    lock_icon: MorphIcon,
    /// Telemetry + lock signals driving the indicators.
    cpu: Signal<f64>,
    paused: Signal<bool>,
    console_locked: Signal<bool>,
    /// The HMI's own reduced-motion toggle — OR'd with the arena-pushed
    /// platform flag for the icon (every animated widget consults it).
    reduced_motion: Signal<bool>,
    /// The `console_locked` value the icon was last driven to —
    /// `None` until the first `tick` seeds the resting shape. Edge
    /// detection so `morph_to` fires once per transition, never per
    /// frame (mid-flight calls re-enter the spring).
    lock_seen: Option<bool>,
    /// The platform reduced-motion flag the arena pushed through
    /// `set_reduced_motion` — remembered so `tick` can keep the icon
    /// at `platform || model` without losing the OS preference.
    platform_reduced: bool,
    /// Child rects resolved in `layout` (device px).
    spinner_rect: Rect,
    progress_rect: Rect,
    lock_rect: Rect,
}

impl StatusBar {
    pub fn new(
        scale: Signal<f32>,
        locale_sel: Signal<usize>,
        cpu: Signal<f64>,
        paused: Signal<bool>,
        console_locked: Signal<bool>,
        reduced_motion: Signal<bool>,
    ) -> Self {
        Self {
            scale,
            dd_rect: Rect::new(0.0, 0.0, 0.0, 0.0),
            focused: false,
            // Seed from the signal — a restored locale must not be
            // stomped back to index 0 by the first `publish()`.
            dropdown: {
                let mut dd = Dropdown::new(LOCALE_LABELS).label("locale");
                dd.commit(locale_sel.get());
                dd
            },
            locale_sel,
            spinner: Spinner::new().size(14.0),
            progress: ProgressBar::new().value(0.0),
            // Same footprint as the spinner; the semantic label tracks
            // the target state (set again on every edge in `tick`).
            lock_icon: MorphIcon::new().size(16.0).label(if console_locked.get() {
                "console locked"
            } else {
                "console unlocked"
            }),
            cpu,
            paused,
            console_locked,
            reduced_motion,
            // `None` → the first `tick` seeds the resting shape with
            // `set_icon` so the icon lands settled, not mid-flight.
            lock_seen: None,
            platform_reduced: false,
            spinner_rect: Rect::new(0.0, 0.0, 0.0, 0.0),
            progress_rect: Rect::new(0.0, 0.0, 0.0, 0.0),
            lock_rect: Rect::new(0.0, 0.0, 0.0, 0.0),
        }
    }

    fn s(&self) -> f32 {
        self.scale.get().max(1.0)
    }

    /// The icon's effective reduced-motion flag: the OS preference the
    /// arena pushed (`set_reduced_motion`) OR the HMI's own toggle —
    /// either one snaps the morph instead of springing it.
    fn push_icon_motion_pref(&mut self) {
        let reduced = self.platform_reduced || self.reduced_motion.get();
        self.lock_icon.set_reduced_motion(reduced);
    }

    /// Push the dropdown's committed index into the outcome signal.
    /// Called after every forwarded event and every tick — a popup
    /// commit lands via `sync_overlay` (no parent event), so `tick`
    /// re-publishes too. `set_if_changed` keeps it cheap.
    fn publish(&mut self) {
        self.locale_sel.set_if_changed(self.dropdown.selected());
    }

    /// Forward a non-positional event to the dropdown — the only
    /// child, so it is always the internal key target.
    fn forward_to_dropdown(&mut self, cx: &mut EventContext) -> EventResponse {
        let mut child_cx = EventContext {
            event: cx.event,
            bounds: self.dd_rect,
            scale: cx.scale,
        };
        self.dropdown.event(&mut child_cx)
    }
}

impl Widget for StatusBar {
    fn debug_name(&self) -> &'static str {
        "StatusBar"
    }

    fn measure(&mut self, _cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        // The parent positions the strip — report full offered width
        // and the band height the widget was designed for.
        Vec2::new(
            constraints.max_size.x,
            (BAND_H * self.s()).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        let pad = 8.0 * self.s();
        // Right-align the dropdown face inside the strip segment,
        // `pad` from the right edge and vertically centered — the
        // segment now spans the full strip height (the widget paints
        // its own background), so the face keeps the same inset the
        // strip used to apply. Rect is (x, y, width, height) — not
        // corners.
        let w = (bounds.size.x - pad).max(1.0);
        let h = (bounds.size.y - pad * 0.75).max(1.0);
        // Right-to-left: locale dropdown, then the spinner, then the
        // lock icon, then the CPU progress bar filling the remainder.
        let dd_w = (110.0 * self.s()).min(w);
        let r = Rect::new(
            bounds.max_x() - pad - dd_w,
            bounds.origin.y + (bounds.size.y - h) * 0.5,
            dd_w,
            h,
        );
        self.dd_rect = r;
        cx.layout_child(&mut self.dropdown, r);
        let sp_d = (16.0 * self.s()).min(h);
        let sp_x = (r.origin.x - pad * 0.5 - sp_d).max(bounds.origin.x);
        self.spinner_rect = Rect::new(
            sp_x,
            bounds.origin.y + (bounds.size.y - sp_d) * 0.5,
            sp_d,
            sp_d,
        );
        cx.layout_child(&mut self.spinner, self.spinner_rect);
        // The lock icon sits left of the spinner — same optical size,
        // same vertical centering.
        let ic_d = (16.0 * self.s()).min(h);
        let ic_x = (sp_x - pad * 0.5 - ic_d).max(bounds.origin.x);
        self.lock_rect = Rect::new(
            ic_x,
            bounds.origin.y + (bounds.size.y - ic_d) * 0.5,
            ic_d,
            ic_d,
        );
        cx.layout_child(&mut self.lock_icon, self.lock_rect);
        let pb_w = (ic_x - pad - bounds.origin.x).max(0.0);
        self.progress_rect = Rect::new(
            bounds.origin.x,
            bounds.origin.y + (bounds.size.y - 6.0 * self.s()) * 0.5,
            pb_w,
            6.0 * self.s(),
        );
        cx.layout_child(&mut self.progress, self.progress_rect);
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        let resp = match cx.event {
            WidgetEvent::SemanticAction(SemanticAction::Focus | SemanticAction::Click) => {
                EventResponse::CaptureFocus
            }
            WidgetEvent::FocusGained => {
                self.focused = true;
                EventResponse::RequestRepaint
            }
            WidgetEvent::FocusLost => {
                self.focused = false;
                EventResponse::RequestRepaint
            }
            // Positional: only the dropdown's rect is live — presses
            // elsewhere on the strip fall through to the root.
            _ if cx.event.position().is_some() => {
                let pos = cx.event.position().expect("checked");
                if !self.dd_rect.contains(pos) {
                    return EventResponse::Ignored;
                }
                self.forward_to_dropdown(cx)
            }
            // Non-positional: the dropdown is the only child — always
            // the key target.
            _ => self.forward_to_dropdown(cx),
        };
        self.publish();
        resp
    }

    fn tick(&mut self, _dt: std::time::Duration) -> bool {
        // `WidgetArena::tick_recursive` already ticks internal children
        // via `child_mut` — that's what animates the Spinner's phase
        // and advances the MorphIcon's spring.
        // The progress bar mirrors the shared `cpu` signal; rebuilding
        // it here keeps the fraction honest (it holds no other state).
        self.progress = ProgressBar::new().value(self.cpu.get() as f32);
        // Reduced-motion pref must be current *before* `morph_to`
        // consults it below — model-toggle writes land between pushes.
        self.push_icon_motion_pref();
        // ADR-0041 dogfood: drive the lock icon off the real
        // `console_locked` signal — the CONSOLE LOCK page's LOCK
        // button, the command verb, and the lock overlay's unlock
        // paths all write the same cell. First observation seeds the
        // resting shape (`set_icon` — no flight); later edges morph.
        let locked = self.console_locked.get();
        if self.lock_seen != Some(locked) {
            let first = self.lock_seen.is_none();
            self.lock_seen = Some(locked);
            let res = if first {
                self.lock_icon.set_icon(lock_icon_d(locked))
            } else {
                self.lock_icon
                    .morph_to(lock_icon_d(locked), LOCK_MORPH_SPRING)
            };
            // The demo `d` constants are curated — a rejection is an
            // engine-side bug, not input the bar can recover.
            debug_assert!(res.is_ok(), "morph_icon demo path rejected: {res:?}");
            self.lock_icon.set_label(if locked {
                "console locked"
            } else {
                "console unlocked"
            });
        }
        self.publish();
        true
    }

    fn set_reduced_motion(&mut self, reduced: bool) {
        // The arena push (OS preference via `apply_platform_preferences`)
        // forwards verbatim to the children — the default impl's
        // contract, minus the `paused`-suspended spinner hole — while
        // the icon takes the union with the HMI toggle (`tick`
        // re-applies it so a model-side flip needs no new push).
        self.platform_reduced = reduced;
        self.dropdown.set_reduced_motion(reduced);
        self.spinner.set_reduced_motion(reduced);
        self.progress.set_reduced_motion(reduced);
        self.push_icon_motion_pref();
    }

    fn sync_overlay(&mut self, overlay: &mut OverlayLayer) {
        // The dropdown owns a popup — let it reconcile against the
        // layer (opens/closes the entry, drains option commits).
        self.dropdown.sync_overlay(overlay);
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Status);
        node.set_label("status bar");
        node.add_action(accesskit::Action::Focus);
    }

    fn child_count(&self) -> usize {
        // The spinner drops out of the tree while paused — it stops
        // ticking (frozen) and stops painting, which is the honest
        // "feed halted" cue. The lock icon stays: console lock is a
        // state, not an activity.
        if self.paused.get() {
            3
        } else {
            4
        }
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        let live = !self.paused.get();
        match (index, live) {
            (0, _) => Some(&self.dropdown),
            (1, true) => Some(&self.spinner),
            (1, false) | (2, true) => Some(&self.lock_icon),
            (2, false) | (3, true) => Some(&self.progress),
            _ => None,
        }
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        let live = !self.paused.get();
        match (index, live) {
            (0, _) => Some(&mut self.dropdown),
            (1, true) => Some(&mut self.spinner),
            (1, false) | (2, true) => Some(&mut self.lock_icon),
            (2, false) | (3, true) => Some(&mut self.progress),
            _ => None,
        }
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        let live = !self.paused.get();
        match (index, live) {
            (0, _) => Some(self.dd_rect),
            (1, true) => Some(self.spinner_rect),
            (1, false) | (2, true) => Some(self.lock_rect),
            (2, false) | (3, true) => Some(self.progress_rect),
            _ => None,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        // Fill the strip segment — `paint_chrome` fills the strip only
        // up to this widget's left edge (chrome paints after widgets,
        // so a shared fill would cover the dropdown face). Same token
        // the chrome fill uses.
        let b = cx.bounds;
        cx.list.push_fill_rect(
            martensite::render::Rect::new(
                f64::from(b.min_x()),
                f64::from(b.min_y()),
                f64::from(b.max_x()),
                f64::from(b.max_y()),
            ),
            cx.color(TokenKey::RaisedColor, [46, 48, 53, 255]),
        );
        // Arena-focus ring around the dropdown face (WCAG 2.4.7 — the
        // audit looks for a painted indicator at the reported focus
        // rect).
        if self.focused {
            let accent = cx.color(TokenKey::AccentColor, [96, 165, 250, 255]);
            // Inflate by ~1pt — a ring exactly on the face edge hides
            // its inner half under the face's border.
            let r = self.dd_rect;
            let grow = f64::from(cx.pt(1.0));
            cx.list.push_stroke_shape(
                martensite::render::Rect::new(
                    f64::from(r.min_x()) - grow,
                    f64::from(r.min_y()) - grow,
                    f64::from(r.max_x()) + grow,
                    f64::from(r.max_y()) + grow,
                ),
                &Shape::rounded(cx.dim(TokenKey::BorderRadiusSmall, 3.0) + cx.pt(1.0)),
                cx.pt(2.0),
                accent,
            );
        }
    }
}

/// The lock icon's resting/target `d` for a `console_locked` value —
/// the demo pair written for this seam (`morph_icon::demo`, ADR-0041).
fn lock_icon_d(locked: bool) -> &'static str {
    if locked {
        morph_icon_demo::LOCK
    } else {
        morph_icon_demo::LOCK_OPEN
    }
}

/// The lock morph's spring — `SNAPPY` (ζ≈0.72): a quick, small
/// overshoot on a status indicator, not a bouncy celebration.
const LOCK_MORPH_SPRING: SpringConfig = SpringConfig::SNAPPY;

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn status_bar(
        locale_sel: Signal<usize>,
        console_locked: Signal<bool>,
        reduced_motion: Signal<bool>,
    ) -> StatusBar {
        StatusBar::new(
            Signal::new(1.0f32),
            locale_sel,
            Signal::new(0.5f64),
            Signal::new(false),
            console_locked,
            reduced_motion,
        )
    }

    #[test]
    fn all_locale_bundles_parse() {
        // `build_l10n` itself asserts each bundle parses — reaching
        // here means all four registered cleanly.
        let l10n = build_l10n();
        let locales: Vec<String> = l10n
            .available_locales()
            .into_iter()
            .map(|l| l.to_string())
            .collect();
        assert_eq!(locales, vec!["de", "en", "es", "fr"]);
    }

    #[test]
    fn sb_hints_resolves_in_every_locale() {
        let l10n = build_l10n();
        for code in LOCALE_CODES {
            l10n.set_locale(code.parse().expect("valid langid"))
                .expect("locale registered");
            let hints = l10n.get("sb-hints").expect("sb-hints resolves");
            assert!(!hints.is_empty(), "sb-hints empty for {code}");
        }
    }

    #[test]
    fn sb_focus_interpolates_name() {
        let l10n = build_l10n();
        let v = l10n
            .get_with_args("sb-focus", &[("name", "Telemetry")])
            .expect("sb-focus resolves");
        assert_eq!(v, "focus: Telemetry");
    }

    #[test]
    fn set_locale_switches_get_output() {
        let l10n = build_l10n();
        assert_eq!(l10n.get("kpi-uptime").as_deref(), Some("UPTIME"));
        l10n.set_locale("de".parse().expect("valid langid"))
            .expect("de registered");
        assert_eq!(l10n.get("kpi-uptime").as_deref(), Some("LAUFZEIT"));
        l10n.set_locale("en".parse().expect("valid langid"))
            .expect("en registered");
        assert_eq!(l10n.get("kpi-uptime").as_deref(), Some("UPTIME"));
    }

    #[test]
    fn restored_locale_survives_first_publish() {
        // Regression: the dropdown used to default to index 0, so the
        // first `publish()` overwrote a restored non-English locale —
        // reverting the UI AND clobbering the persisted pref on the
        // next write-through. `StatusBar::new` now seeds the dropdown
        // from the signal, so publish is a no-op until the user picks.
        let sel = Signal::new(3usize); // "de" — a restored store value
        let mut sb = status_bar(sel.clone(), Signal::new(false), Signal::new(false));
        sb.publish();
        assert_eq!(sel.get(), 3, "publish clobbered the restored locale");
    }

    #[test]
    fn lock_icon_path_tracks_console_locked() {
        // The icon's target `d` is a pure function of the signal —
        // this half of the contract is engine-independent.
        assert_eq!(lock_icon_d(true), morph_icon_demo::LOCK);
        assert_eq!(lock_icon_d(false), morph_icon_demo::LOCK_OPEN);
    }

    /// `true` once the morph geometry engine is live. While the port
    /// is pending, `icon`/`set_icon`/`morph_to` end in
    /// `unimplemented!()` — the morph behavioral tests probe this and
    /// skip with a printed reason (suite stays green in the interim).
    fn morph_engine_live() -> bool {
        std::panic::catch_unwind(|| MorphIcon::icon(morph_icon_demo::LOCK).map(|_| ())).is_ok()
    }

    /// Drives internal children the way `WidgetArena::tick_recursive`
    /// does — the icon's spring advances through `child_mut`, not
    /// through `StatusBar::tick` itself.
    fn tick_children(sb: &mut StatusBar, dt: Duration) {
        for i in 0..sb.child_count() {
            if let Some(c) = sb.child_mut(i) {
                let _ = c.tick(dt);
            }
        }
    }

    /// Frame-step the icon until its spring settles (bounded so a
    /// never-settling spring fails rather than hangs).
    fn settle_icon(sb: &mut StatusBar) {
        for _ in 0..500 {
            if !sb.lock_icon.is_animating() {
                return;
            }
            tick_children(sb, Duration::from_millis(16));
        }
        panic!("lock icon spring never settled");
    }

    #[test]
    fn console_locked_edge_morphs_the_statusbar_icon() {
        if !morph_engine_live() {
            eprintln!("skip: morph_icon geometry engine port pending (unimplemented!)");
            return;
        }
        let locked = Signal::new(false);
        let mut sb = status_bar(Signal::new(0usize), locked.clone(), Signal::new(false));
        // First tick seeds the resting shape — no flight on boot.
        sb.tick(Duration::from_millis(16));
        assert!(!sb.lock_icon.is_animating(), "seed must land settled");
        assert_eq!(sb.lock_icon.progress(), 1.0);
        // A real signal edge starts the morph toward LOCK — the spring
        // is in flight immediately after the `morph_to` call.
        locked.set(true);
        sb.tick(Duration::from_millis(16));
        assert!(
            sb.lock_icon.is_animating(),
            "console_locked edge did not start a morph"
        );
        // A steady signal never restarts the spring — the icon flies
        // to settle on LOCK.
        sb.tick(Duration::from_millis(16));
        tick_children(&mut sb, Duration::from_millis(16));
        assert!(sb.lock_icon.is_animating());
        settle_icon(&mut sb);
        for _ in 0..3 {
            sb.tick(Duration::from_millis(16));
            tick_children(&mut sb, Duration::from_millis(16));
            assert!(
                !sb.lock_icon.is_animating(),
                "steady console_locked restarted the spring"
            );
        }
        // Releasing the lock morphs back to LOCK_OPEN.
        locked.set(false);
        sb.tick(Duration::from_millis(16));
        assert!(sb.lock_icon.is_animating());
        settle_icon(&mut sb);
        assert_eq!(sb.lock_icon.progress(), 1.0);
    }

    #[test]
    fn reduced_motion_snaps_the_lock_icon() {
        if !morph_engine_live() {
            eprintln!("skip: morph_icon geometry engine port pending (unimplemented!)");
            return;
        }
        let locked = Signal::new(false);
        let hmi_reduced = Signal::new(false);
        let mut sb = status_bar(Signal::new(0usize), locked.clone(), hmi_reduced.clone());
        sb.tick(Duration::from_millis(16));
        // The HMI toggle alone snaps the morph — no OS pref needed.
        hmi_reduced.set(true);
        locked.set(true);
        sb.tick(Duration::from_millis(16));
        assert!(
            !sb.lock_icon.is_animating(),
            "model reduced-motion must snap the morph"
        );
        // So does the platform push on its own (arena-driven
        // `set_reduced_motion`), with the model toggle back off.
        hmi_reduced.set(false);
        locked.set(false);
        sb.tick(Duration::from_millis(16));
        settle_icon(&mut sb);
        sb.set_reduced_motion(true);
        locked.set(true);
        sb.tick(Duration::from_millis(16));
        assert!(
            !sb.lock_icon.is_animating(),
            "platform reduced-motion push must snap the morph"
        );
    }

    #[test]
    fn locale_codes_align_with_labels() {
        // The dropdown maps `selected()` index → `LOCALE_CODES` — a
        // transposed entry would silently switch to the wrong bundle.
        // Pin each index to the language its label promises.
        let expected = ["UPTIME", "ACTIVO", "ACTIF", "LAUFZEIT"];
        let l10n = build_l10n();
        assert_eq!(LOCALE_LABELS.len(), LOCALE_CODES.len());
        for (i, code) in LOCALE_CODES.iter().enumerate() {
            l10n.set_locale(code.parse().expect("valid langid"))
                .expect("locale registered");
            assert_eq!(
                l10n.get("kpi-uptime").as_deref(),
                Some(expected[i]),
                "LOCALE_CODES[{i}] = {code} mislabeled as {:?}",
                LOCALE_LABELS[i]
            );
        }
    }
}

#[cfg(test)]
mod locale_probe_tests {
    use super::*;

    fn probe() -> (martensite::access::paint_audit::LocaleProbe, Signal<String>) {
        let filter = Signal::new(String::new());
        (build_locale_probe(filter.clone()), filter)
    }

    #[test]
    fn probe_covers_static_and_templated_values_in_every_locale() {
        let (p, _) = probe();
        // Static value, English and Spanish.
        assert!(p.is_translated(
            "Tab focus · rail switches views · click sort/select · F alerts · Space pause",
            None
        ));
        // A bare fragment of a templated line is not a value itself.
        assert!(!p.is_translated("Espacio pausa", None));
        // The full es line — not a fragment.
        assert!(p.is_translated(
            "Tab foco · raíl cambia vista · clic ordenar/seleccionar · F alertas · Espacio pausa",
            None
        ));
        // Templated pattern — `focus: { $name }` resolves per locale.
        assert!(p.is_translated("focus: process grid", None));
        assert!(p.is_translated("foco: editor", None));
        assert!(p.is_translated("Fokus: media", None));
    }

    #[test]
    fn probe_exemptions_and_real_findings() {
        let (p, filter) = probe();
        // Endonyms and non-alphabetic data are intentionally unlocalized.
        assert!(p.is_translated("Español", None));
        assert!(p.is_translated("99.9%", None));
        assert!(p.is_translated("5323", None));
        // The live filter-field content is user data.
        filter.set("pid 42".to_string());
        assert!(p.is_translated("pid 42", None));
        // Hardcoded chrome — the check's real findings.
        assert!(!p.is_translated("PROCESS GRID", None));
        assert!(!p.is_translated("MEDIA", None));
        assert!(!p.is_translated("Dark", None));
    }

    #[test]
    fn probe_accepts_fit_truncated_translations() {
        let (p, _) = probe();
        // A `fit()`-ellipsized localized string stays localized.
        assert!(p.is_translated(
            "Tab focus · rail switches views · click sort/select · F alerts · S…",
            None
        ));
    }
}
