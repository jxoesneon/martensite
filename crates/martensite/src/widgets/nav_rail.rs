//! `NavRail` widget: a vertical icon+label destination rail for
//! app-level navigation (Material 3 `NavigationRail`, WinUI
//! `NavigationView` in rail mode, `NSSplitViewController` sidebar
//! analogue).
//!
//! Destinations stack vertically; the selected one gets an accent
//! pill. Arrow keys move focus, `Enter`/`Space` activates — poll
//! [`NavRail::take_activated`] for the destination index. Selection
//! and activation are separate: the rail only *reports* activations,
//! the app decides whether to change `selected`.
//!
//! A destination's icon is either a text glyph ([`NavRail::destination`])
//! or a hosted [`MorphIcon`] stroke icon
//! ([`NavRail::destination_icon`], [`NavRail::destination_named`]) —
//! the latter is a real internal
//! child: it ticks with the arena (so `morph_to` animates), stays
//! decorative in the a11y tree (the destination's label carries the
//! name), and picks pill-contrasting ink while selected.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::nav_rail::{NavDestination, NavRail};
//!
//! let r = NavRail::new()
//!     .destination("nav.home", "Home")
//!     .destination("nav.settings", "Settings");
//! assert_eq!(r.destination_count(), 2);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::widget::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Widget, WidgetEvent,
};
use martensite_core::{Rect, TokenKey};

use crate::widgets::morph_icon::MorphIcon;

/// Narrowest rail width, logical points — the icon + pill insets
/// floor used when no destination label measures wider.
const MIN_WIDTH_PT: f32 = 72.0;
/// Widest rail width, logical points — a label longer than the clamp
/// clips inside its pill rather than the rail eating the deck.
const MAX_WIDTH_PT: f32 = 120.0;
/// Breathing room added beside the measured label, logical points.
const LABEL_PAD_PT: f32 = 8.0;
/// Destination cell height, logical points.
const CELL_PT: f32 = 56.0;
/// Icon glyph size, logical points.
const ICON_PT: f32 = 20.0;
/// Label font size, logical points.
const LABEL_PT: f32 = 12.0;
/// Top inset before the first destination, logical points.
const TOP_INSET_PT: f32 = 8.0;
/// Selected pill horizontal inset, logical points.
const PILL_INSET_PT: f32 = 8.0;

/// Rail face.
const FACE: [u8; 4] = [245, 246, 248, 255];
/// Label ink.
const INK: [u8; 4] = [30, 31, 36, 255];
/// Unselected ink.
const INK_DIM: [u8; 4] = [110, 114, 123, 255];
/// Hover tint.
const HOVER: [u8; 4] = [30, 31, 36, 12];

/// One rail destination.
///
/// # Examples
///
/// ```
/// use martensite::widgets::nav_rail::NavDestination;
///
/// let d = NavDestination::new("status.star", "Starred");
/// assert_eq!(d.label, "Starred");
/// ```
#[derive(Clone, Debug)]
pub struct NavDestination {
    /// Icon mark (a short string — a namespaced icon name like
    /// `"nav.home"`, a symbol, or a single character). Unused when
    /// `icon_d` carries a stroke icon.
    pub icon: String,
    /// The destination label.
    pub label: String,
    /// Stroke-icon `d` on the 24-unit icon grid — when set, the rail
    /// hosts a decorative [`MorphIcon`] child for this destination and
    /// ignores `icon`. Kept as data so `NavDestination` stays `Clone`.
    icon_d: Option<String>,
}

impl NavDestination {
    /// Creates a destination with a text-glyph icon — a namespaced
    /// icon name (`"nav.home"`, …) resolves through the ambient icon
    /// family when the rail hosts it; any other string paints as a
    /// text glyph.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::nav_rail::NavDestination;
    ///
    /// let d = NavDestination::new("file.folder", "Files");
    /// ```
    #[must_use]
    pub fn new(icon: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            icon: icon.into(),
            label: label.into(),
            icon_d: None,
        }
    }

    /// Creates a destination whose icon is a stroke path (`d` data on
    /// the 24-unit icon grid — the lucide/feather idiom; see
    /// [`crate::icons::builtin`](mod@crate::icons::builtin) for the native pack's constants).
    /// The rail hosts it as a real `MorphIcon` internal child, so
    /// `morph_to` animates; the icon is decorative in the a11y tree —
    /// `label` carries the name.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::icons::builtin;
    /// use martensite::widgets::nav_rail::NavDestination;
    ///
    /// let d = NavDestination::icon(builtin::data::DATA_GRID, "Grid");
    /// assert_eq!(d.icon_d(), Some(builtin::data::DATA_GRID));
    /// ```
    #[must_use]
    pub fn icon(icon_d: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            icon: String::new(),
            label: label.into(),
            icon_d: Some(icon_d.into()),
        }
    }

    /// Creates a destination whose icon resolves by *name* through
    /// the ambient icon family
    /// ([`icons::resolve_icon`](crate::icons::resolve_icon)) —
    /// `"nav.menu"`, `"data.grid"`, `"media.play"`, … The resolved `d`
    /// is stored, so the value stays plain data; an unknown name
    /// yields an iconless destination (the rail tolerates it like a
    /// rejected `d`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::icons::builtin;
    /// use martensite::widgets::nav_rail::NavDestination;
    ///
    /// let d = NavDestination::named("nav.menu", "Menu");
    /// assert_eq!(d.icon_d(), Some(builtin::nav::NAV_MENU));
    /// assert!(NavDestination::named("bogus.name", "Nope").icon_d().is_none());
    /// ```
    #[must_use]
    pub fn named(name: &str, label: impl Into<String>) -> Self {
        Self {
            icon: String::new(),
            label: label.into(),
            icon_d: crate::icons::resolve_icon(name),
        }
    }

    /// The stroke-icon `d` this destination carries, if any.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::nav_rail::NavDestination;
    ///
    /// assert_eq!(NavDestination::new("★", "Starred").icon_d(), None);
    /// ```
    #[must_use]
    pub fn icon_d(&self) -> Option<&str> {
        self.icon_d.as_deref()
    }
}

/// A vertical navigation rail.
///
/// # Examples
///
/// ```
/// use martensite::widgets::nav_rail::NavRail;
///
/// let r = NavRail::new().destination("nav.home", "Home").selected(0);
/// assert_eq!(r.selected_index(), Some(0));
/// ```
pub struct NavRail {
    /// Destinations top-to-bottom.
    destinations: Vec<NavDestination>,
    /// Stroke-icon widgets parallel to `destinations` — `Some` where
    /// the destination carries `icon_d` (`None` where the destination
    /// paints a text glyph, or the `d` failed to parse).
    icons: Vec<Option<MorphIcon>>,
    /// Per-destination icon rects resolved in `layout` (device px) —
    /// `Some` only where an icon widget is hosted.
    icon_rects: Vec<Option<Rect>>,
    /// Selected destination index.
    selected: Option<usize>,
    /// Hovered destination index.
    highlighted: Option<usize>,
    /// Pending activation — drained by `take_activated`.
    activated: Option<usize>,
    /// Per-destination hit rects.
    cell_rects: Vec<Rect>,
    /// Enabled flag.
    enabled: bool,
    /// Shared shaped-text painter.
    text_painter: Option<crate::text_paint::SharedTextPainter>,
    /// Accessible label override — unset falls back to the
    /// built-in `"Navigation"` chrome string so the host app
    /// can localize it.
    pub a11y_label: Option<String>,
}

impl NavRail {
    /// Creates an empty rail.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::nav_rail::NavRail;
    ///
    /// let r = NavRail::new();
    /// assert_eq!(r.destination_count(), 0);
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self {
            destinations: Vec::new(),
            icons: Vec::new(),
            icon_rects: Vec::new(),
            selected: None,
            highlighted: None,
            activated: None,
            cell_rects: Vec::new(),
            enabled: true,
            text_painter: None,
            a11y_label: None,
        }
    }

    /// Appends a destination (icon mark + label).
    ///
    /// An icon name (`"nav.home"`, `"nav.settings"`, …) resolves
    /// through the ambient icon family and hosts a
    /// [`MorphIcon`] exactly like
    /// [`destination_named`](Self::destination_named); any other
    /// string stays a text glyph.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::nav_rail::NavRail;
    ///
    /// let r = NavRail::new().destination("nav.home", "Home");
    /// ```
    #[must_use]
    pub fn destination(mut self, icon: impl Into<String>, label: impl Into<String>) -> Self {
        let mut d = NavDestination::new(icon, label);
        Self::resolve_glyph_icon(&mut d);
        let icon = d.icon_d().and_then(|d| Self::build_icon(d, self.enabled));
        self.destinations.push(d);
        self.icons.push(icon);
        self.sync_icon_inks();
        self
    }

    /// Appends a destination whose icon is a hosted
    /// [`MorphIcon`] stroke icon — `icon_d`
    /// is SVG path data on the 24-unit icon grid (the lucide/feather
    /// idiom; see [`crate::icons::builtin`](mod@crate::icons::builtin) for the native pack's
    /// constants). The icon is a real internal child: it ticks with
    /// the arena (so `morph_to` animates), reports `Role::Image` as
    /// decorative-hidden in the a11y tree, and picks pill-contrasting
    /// ink while the destination is selected.
    ///
    /// Drive later shapes through
    /// [`icon_widget_mut`](Self::icon_widget_mut) — e.g. morph to a
    /// selected-variant `d` (see
    /// [`icons::builtin::selected_pip`](crate::icons::builtin::selected_pip))
    /// when `set_selected` moves the pill, or to a state shape
    /// (`paused` → flatline) on signal edges.
    ///
    /// A `d` the icon engine rejects parses to no icon at all — the
    /// destination keeps its label and behaves like an iconless cell
    /// rather than failing the build.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::icons::builtin;
    /// use martensite::widgets::nav_rail::NavRail;
    ///
    /// let r = NavRail::new().destination_icon(builtin::data::DATA_GRID, "Process");
    /// assert!(r.icon_widget(0).is_some());
    /// ```
    #[must_use]
    pub fn destination_icon(mut self, icon_d: impl Into<String>, label: impl Into<String>) -> Self {
        let icon_d = icon_d.into();
        self.destinations
            .push(NavDestination::icon(icon_d.clone(), label));
        self.icons.push(Self::build_icon(&icon_d, self.enabled));
        self.sync_icon_inks();
        self
    }

    /// Appends a destination whose icon resolves by *name* through
    /// the ambient icon family
    /// ([`icons::resolve_icon`](crate::icons::resolve_icon)) —
    /// `"nav.menu"`, `"data.grid"`, `"media.play"`, … The resolved
    /// shape is hosted exactly like [`destination_icon`](Self::destination_icon)'s
    /// (internal `MorphIcon` child, decorative a11y, pill-contrast
    /// ink); an unknown name degrades to an iconless destination
    /// rather than failing the build — same contract as a rejected `d`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::nav_rail::NavRail;
    ///
    /// let r = NavRail::new().destination_named("data.grid", "Process");
    /// assert!(r.icon_widget(0).is_some());
    /// // Unknown names degrade to an iconless destination.
    /// assert!(NavRail::new()
    ///     .destination_named("bogus.name", "Nope")
    ///     .icon_widget(0)
    ///     .is_none());
    /// ```
    #[must_use]
    pub fn destination_named(mut self, name: &str, label: impl Into<String>) -> Self {
        let d = NavDestination::named(name, label);
        let icon = d.icon_d().and_then(|d| Self::build_icon(d, self.enabled));
        self.destinations.push(d);
        self.icons.push(icon);
        self.sync_icon_inks();
        self
    }

    /// When `d.icon` is a namespaced icon name that resolves through
    /// the ambient icon family, stores the resolved `d` as the
    /// destination's stroke icon — the mark then paints as a hosted
    /// [`MorphIcon`], not a text glyph.
    fn resolve_glyph_icon(d: &mut NavDestination) {
        if d.icon_d.is_none() {
            if let Some(resolved) = crate::icons::resolve_icon(&d.icon) {
                d.icon_d = Some(resolved);
            }
        }
    }

    /// Builds the hosted icon for a stroke `d` — `decorative` (the
    /// destination's label owns the a11y name). A rejected `d` yields
    /// `None`: the slot stays empty, the destination still works.
    fn build_icon(icon_d: &str, enabled: bool) -> Option<MorphIcon> {
        let mut icon = MorphIcon::icon(icon_d).ok()?.decorative(true);
        if !enabled {
            icon.set_ink(INK_DIM);
        }
        Some(icon)
    }

    /// Re-resolves hosted icon inks for the current selection —
    /// selected destinations paint pill-contrasting ink
    /// (`MorphIcon::ink_over` on the accent token), the rest theme ink.
    fn sync_icon_inks(&mut self) {
        for (i, icon) in self.icons.iter_mut().enumerate() {
            if let Some(icon) = icon {
                icon.set_ink_over(if self.enabled && self.selected == Some(i) {
                    Some(TokenKey::AccentColor)
                } else {
                    None
                });
            }
        }
    }

    /// The `MorphIcon` hosting destination `index`'s stroke icon —
    /// `None` for glyph destinations, out-of-range indices, and `d`s
    /// the icon engine rejected at build.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::nav_rail::NavRail;
    ///
    /// let r = NavRail::new()
    ///     .destination_named("data.grid", "Grid")
    ///     .destination("*", "Settings");
    /// assert!(r.icon_widget(0).is_some());
    /// assert!(r.icon_widget(1).is_none()); // glyph destination
    /// ```
    #[must_use]
    pub fn icon_widget(&self, index: usize) -> Option<&MorphIcon> {
        self.icons.get(index).and_then(Option::as_ref)
    }

    /// Mutable twin of [`icon_widget`](Self::icon_widget) — the seam
    /// for `set_icon`/`morph_to` drives (selection variants, state
    /// shapes) from binding code.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::icons::builtin;
    /// use martensite::widgets::nav_rail::NavRail;
    ///
    /// let mut r = NavRail::new().destination_named("data.grid", "Grid");
    /// r.icon_widget_mut(0)
    ///     .expect("icon destination")
    ///     .set_icon(&builtin::selected_pip(builtin::data::DATA_GRID))
    ///     .unwrap();
    /// ```
    pub fn icon_widget_mut(&mut self, index: usize) -> Option<&mut MorphIcon> {
        self.icons.get_mut(index).and_then(Option::as_mut)
    }

    /// Sets all destinations at once.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::nav_rail::{NavDestination, NavRail};
    ///
    /// let r = NavRail::new().destinations(vec![NavDestination::new("status.star", "Favs")]);
    /// ```
    #[must_use]
    pub fn destinations(mut self, destinations: Vec<NavDestination>) -> Self {
        let mut destinations = destinations;
        for d in &mut destinations {
            Self::resolve_glyph_icon(d);
        }
        self.icons = destinations
            .iter()
            .map(|d| d.icon_d().and_then(|d| Self::build_icon(d, self.enabled)))
            .collect();
        self.destinations = destinations;
        self.selected = self.selected.filter(|i| *i < self.destinations.len());
        self.sync_icon_inks();
        self
    }

    /// Sets the selected destination.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::nav_rail::NavRail;
    ///
    /// let r = NavRail::new().destination("nav.home", "Home").selected(0);
    /// ```
    #[must_use]
    pub fn selected(mut self, index: usize) -> Self {
        self.selected = Some(index.min(self.destinations.len().saturating_sub(1)));
        self.sync_icon_inks();
        self
    }

    /// Enables or disables the rail.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::nav_rail::NavRail;
    ///
    /// let r = NavRail::new().enabled(false);
    /// ```
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        if !enabled {
            for icon in self.icons.iter_mut().flatten() {
                icon.set_ink(INK_DIM);
            }
        }
        self.sync_icon_inks();
        self
    }

    /// The number of destinations.
    #[inline]
    #[must_use]
    pub fn destination_count(&self) -> usize {
        self.destinations.len()
    }

    /// The selected destination index.
    #[inline]
    #[must_use]
    pub fn selected_index(&self) -> Option<usize> {
        self.selected
    }

    /// Sets the selection programmatically.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::nav_rail::NavRail;
    ///
    /// let mut r = NavRail::new().destination("nav.home", "Home");
    /// r.set_selected(Some(0));
    /// assert_eq!(r.selected_index(), Some(0));
    /// ```
    pub fn set_selected(&mut self, index: Option<usize>) {
        self.selected = index.filter(|i| *i < self.destinations.len());
        self.sync_icon_inks();
    }

    /// Drains a destination activation — the index the user chose.
    /// The app applies it via [`NavRail::set_selected`] if desired.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::nav_rail::NavRail;
    ///
    /// let mut r = NavRail::new();
    /// assert_eq!(r.take_activated(), None);
    /// ```
    pub fn take_activated(&mut self) -> Option<usize> {
        self.activated.take()
    }

    /// Installs a shared shaped-text painter.
    #[must_use]
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }
}

impl Default for NavRail {
    fn default() -> Self {
        Self::new()
    }
}

impl NavRail {
    /// Sets the accessible label announced by assistive tech
    /// (default `"Navigation"`). Host apps localize the chrome string
    /// through this override.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::nav_rail::NavRail;
    ///
    /// let w = NavRail::new().destination("nav.home", "Home").selected(0).a11y_label("Custom name");
    /// assert_eq!(w.a11y_label.as_deref(), Some("Custom name"));
    /// ```
    #[must_use]
    pub fn a11y_label(mut self, label: impl Into<String>) -> Self {
        self.a11y_label = Some(label.into());
        self
    }
}

impl Widget for NavRail {
    fn measure(&mut self, cx: &mut LayoutContext, _constraints: LayoutConstraints) -> Vec2 {
        // The rail is the width authority for its band: the widest
        // destination content (label or icon) measured through the
        // same shaping pipeline the paint pass uses, plus the pill's
        // horizontal inset on each side and a little padding — clamped
        // so an extreme label clips inside its pill instead of
        // swallowing the deck.
        let mut w = cx.pt(MIN_WIDTH_PT);
        for d in &self.destinations {
            let lw = cx.measure_text(&d.label, LABEL_PT).unwrap_or_else(|| {
                crate::text_paint::estimate_label_width(&d.label) * cx.scale * LABEL_PT / 14.0
            });
            let iw = if d.icon_d().is_some() {
                // A stroke icon is a known square — no glyph measure
                // needed, and the (empty) fallback string has no
                // measurable width anyway.
                cx.pt(ICON_PT)
            } else {
                cx.measure_text(&d.icon, ICON_PT)
                    .unwrap_or_else(|| cx.pt(ICON_PT))
            };
            w = w.max(lw.max(iw) + cx.pt(PILL_INSET_PT * 2.0 + LABEL_PAD_PT));
        }
        Vec2::new(
            w.min(cx.pt(MAX_WIDTH_PT)),
            cx.pt(TOP_INSET_PT + CELL_PT * self.destinations.len().max(1) as f32),
        )
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.cell_rects.clear();
        self.icon_rects.clear();
        let cell = cx.pt(CELL_PT);
        let top = cx.pt(TOP_INSET_PT);
        for i in 0..self.destinations.len() {
            let r = Rect::new(
                bounds.origin.x,
                bounds.origin.y + top + i as f32 * cell,
                bounds.size.x.min(cx.pt(MAX_WIDTH_PT)),
                cell,
            );
            self.cell_rects.push(r);
            if let Some(icon) = self.icons.get_mut(i).and_then(Option::as_mut) {
                // Same slot the glyph path paints into: centred in the
                // cell, `6pt` below its top.
                let size = cx.pt(ICON_PT);
                let ir = Rect::new(
                    r.origin.x + (r.size.x - size) / 2.0,
                    r.origin.y + cx.pt(6.0),
                    size,
                    size,
                );
                // `layout_child` stamps the icon's own `bounds` and
                // forwards the rail's focusability flags.
                cx.layout_child(icon, ir);
                self.icon_rects.push(Some(ir));
            } else {
                self.icon_rects.push(None);
            }
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Navigation);
        node.set_label(self.a11y_label.as_deref().unwrap_or("Navigation"));
        if !self.enabled {
            node.set_disabled();
        }
    }

    // Hosted stroke icons are internal children — the arena ticks
    // them (morph springs), paints them after the rail's own pass,
    // and emits their (hidden, decorative) a11y nodes.
    fn child_count(&self) -> usize {
        self.icons.iter().filter(|i| i.is_some()).count()
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        self.icons
            .iter()
            .flatten()
            .nth(index)
            .map(|i| i as &dyn Widget)
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        self.icons
            .iter_mut()
            .flatten()
            .nth(index)
            .map(|i| i as &mut dyn Widget)
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        self.icon_rects.iter().flatten().nth(index).copied()
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.enabled {
            return EventResponse::Ignored;
        }
        match cx.event {
            WidgetEvent::PointerMoved { position } => {
                let hit = self.cell_rects.iter().position(|r| r.contains(*position));
                if hit != self.highlighted {
                    self.highlighted = hit;
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Handled
            }
            WidgetEvent::PointerLeave => {
                self.highlighted = None;
                EventResponse::RequestRepaint
            }
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                ..
            } => {
                if let Some(i) = self.cell_rects.iter().position(|r| r.contains(*position)) {
                    self.activated = Some(i);
                    self.highlighted = Some(i);
                    return EventResponse::Handled;
                }
                EventResponse::Ignored
            }
            WidgetEvent::KeyPressed { key, .. } => match key.as_str() {
                "ArrowDown" | "ArrowUp" => {
                    if self.destinations.is_empty() {
                        return EventResponse::Ignored;
                    }
                    let cur = self.highlighted.or(self.selected);
                    let next = match (key.as_str(), cur) {
                        ("ArrowDown", Some(i)) => (i + 1).min(self.destinations.len() - 1),
                        ("ArrowDown", None) => 0,
                        (_, Some(i)) => i.saturating_sub(1),
                        (_, None) => self.destinations.len() - 1,
                    };
                    self.highlighted = Some(next);
                    EventResponse::RequestRepaint
                }
                "Enter" | "Space" | " " => {
                    if let Some(i) = self.highlighted.or(self.selected) {
                        self.activated = Some(i);
                        return EventResponse::Handled;
                    }
                    EventResponse::Ignored
                }
                _ => EventResponse::Ignored,
            },
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        cx.list.push_fill_rect(
            kurbo::Rect::new(
                f64::from(cx.bounds.min_x()),
                f64::from(cx.bounds.min_y()),
                f64::from(cx.bounds.max_x()),
                f64::from(cx.bounds.max_y()),
            ),
            cx.color(TokenKey::SurfaceColor, FACE),
        );
        let icon_size = cx.pt(ICON_PT);
        let label_size = cx.pt(LABEL_PT);
        let accent = cx.color(TokenKey::AccentColor, [70, 110, 200, 255]);
        for (i, d) in self.destinations.iter().enumerate() {
            let r = self.cell_rects[i];
            let selected = self.selected == Some(i);
            let hovered = self.highlighted == Some(i) && !selected;
            // The selected/hover pill is the text's actual background —
            // clip the icon and label to it so a wide label can't spill
            // contrast-chosen ink past the pill onto the rail face.
            let pill = if selected || hovered {
                let inset = cx.pt(PILL_INSET_PT);
                let vr = if selected { cx.pt(4.0) } else { cx.pt(6.0) };
                let pr = kurbo::Rect::new(
                    f64::from(r.min_x() + inset),
                    f64::from(r.min_y() + vr),
                    f64::from(r.max_x() - inset),
                    f64::from(r.max_y() - vr),
                );
                cx.list.push_fill_shape(
                    pr,
                    &martensite_core::shape::Shape::squircle(cx.pt(10.0)),
                    if selected { accent } else { HOVER },
                );
                pr
            } else {
                kurbo::Rect::new(
                    f64::from(r.min_x()),
                    f64::from(r.min_y()),
                    f64::from(r.max_x()),
                    f64::from(r.max_y()),
                )
            };
            let ink = if selected {
                // Selected pill is accent-filled — pick the ink that
                // actually contrasts the accent: a light accent makes
                // inverse-white unreadable (1.1:1).
                crate::text_paint::better_ink(
                    accent,
                    cx.color(TokenKey::TextInverseColor, [255, 255, 255, 255]),
                    INK,
                )
            } else if self.enabled {
                cx.color(TokenKey::TextColor, INK)
            } else {
                INK_DIM
            };
            // Icon: a hosted MorphIcon paints itself in the internal-
            // child pass (into the rect `layout` gave it); glyph
            // destinations paint `d.icon` here, centred horizontally.
            let iy = r.origin.y + cx.pt(6.0);
            if d.icon_d().is_none() {
                let iw = painter
                    .and_then(|p| p.measure_text(&d.icon, icon_size))
                    .unwrap_or(icon_size);
                let ix = r.origin.x + (r.size.x - iw) / 2.0;
                crate::text_paint::paint_label_clipped(
                    painter,
                    cx.list,
                    pill,
                    kurbo::Point::new(f64::from(ix), f64::from(iy)),
                    &d.icon,
                    icon_size,
                    ink,
                );
            }
            // Label under the icon.
            let lw = painter
                .and_then(|p| p.measure_text(&d.label, label_size))
                .unwrap_or(label_size * d.label.chars().count() as f32 * 0.55);
            let lx = r.origin.x + (r.size.x - lw) / 2.0;
            // Centered past the pill edge the run's recorded origin
            // (and its first glyphs) land on the rail face — clamp the
            // start into the pill so text never sits dark-on-dark.
            let lx = lx
                .max(pill.x0 as f32)
                .min((pill.x1 as f32 - lw).max(pill.x0 as f32));
            let ly = iy + icon_size + cx.pt(4.0);
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                kurbo::Rect::new(pill.x0, f64::from(ly), pill.x1, pill.y1),
                kurbo::Point::new(f64::from(lx), f64::from(ly)),
                &d.label,
                label_size,
                if selected {
                    ink
                } else {
                    cx.color(TokenKey::TextMutedColor, INK_DIM)
                },
            );
        }
    }
}

impl std::fmt::Debug for NavRail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NavRail")
            .field("destinations", &self.destinations.len())
            .field("selected", &self.selected)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn make_cx(hot: &mut HotNode) -> LayoutContext<'_> {
        LayoutContext { hot, scale: 1.0 }
    }

    fn ev<'a>(event: &'a WidgetEvent) -> EventContext<'a> {
        EventContext {
            event,
            bounds: Rect::new(0.0, 0.0, 72.0, 400.0),
            scale: 1.0,
        }
    }

    fn rail() -> NavRail {
        let mut r = NavRail::new()
            .destination("nav.home", "Home")
            .destination("nav.search", "Search")
            .destination("nav.settings", "Settings");
        let mut hot = HotNode::default();
        r.layout(&mut make_cx(&mut hot), Rect::new(0.0, 0.0, 72.0, 400.0));
        r
    }

    #[test]
    fn builder_counts() {
        assert_eq!(rail().destination_count(), 3);
    }

    /// Fixed-width measurer — every char advances `em` device px so the
    /// measured rail width is a pure function of the longest label.
    struct EmWide;
    impl martensite_core::paint::TextShaper for EmWide {
        fn paint_shaped_text(
            &self,
            _: &mut martensite_core::PaintList,
            _: kurbo::Point,
            _: &str,
            _: f32,
            _: [u8; 4],
        ) {
        }
        fn measure_text(&self, text: &str, size_px: f32) -> Option<f32> {
            Some(text.chars().count() as f32 * size_px)
        }
    }

    fn constraints() -> LayoutConstraints {
        LayoutConstraints {
            min_size: Vec2::ZERO,
            max_size: Vec2::new(f32::MAX, f32::MAX),
        }
    }

    #[test]
    fn measure_fits_widest_label() {
        let _g = martensite_core::paint::install_ambient_measurer(std::sync::Arc::new(EmWide));
        let mut hot = HotNode::default();
        let mut cx = make_cx(&mut hot);
        // Short labels stay at the 72 pt floor.
        let mut r = NavRail::new().destination("a", "Ok");
        let m = r.measure(&mut cx, constraints());
        assert_eq!(m.x, MIN_WIDTH_PT);
        // "TELEMETRY" — 9 chars × 12 pt = 108 + 2×8 insets + 8 pad =
        // 132 → the 120 pt clamp engages; longer labels clip in-pill.
        let mut r = NavRail::new().destination("a", "TELEMETRY");
        let m = r.measure(&mut cx, constraints());
        assert_eq!(m.x, MAX_WIDTH_PT);
        // "EDITOR" — 6 × 12 = 72 + 24 = 96 pt, inside the clamp.
        let mut r = NavRail::new().destination("a", "EDITOR");
        let m = r.measure(&mut cx, constraints());
        assert_eq!(m.x, 96.0);
    }

    #[test]
    fn measure_estimate_fallback_without_measurer() {
        // No ambient measurer — the case-aware estimate still widens
        // past the 72 pt floor for a long cap-heavy label.
        let mut hot = HotNode::default();
        let mut cx = make_cx(&mut hot);
        let mut r = NavRail::new().destination("a", "TELEMETRYEXTRA");
        let m = r.measure(&mut cx, constraints());
        assert!(m.x > MIN_WIDTH_PT && m.x <= MAX_WIDTH_PT);
    }

    #[test]
    fn click_activates() {
        let mut r = rail();
        let cell = r.cell_rects[1];
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(cell.origin.x + 8.0, cell.origin.y + 8.0),
            button: PointerButton::Primary,
            count: 1,
        };
        assert_eq!(r.event(&mut ev(&press)), EventResponse::Handled);
        assert_eq!(r.take_activated(), Some(1));
        // Activation ≠ selection — the app applies it.
        assert_eq!(r.selected_index(), None);
    }

    #[test]
    fn arrows_move_highlight() {
        let mut r = rail().selected(0);
        let down = WidgetEvent::KeyPressed {
            key: "ArrowDown".into(),
            repeat: false,
        };
        r.event(&mut ev(&down));
        assert_eq!(r.highlighted, Some(1));
        let enter = WidgetEvent::KeyPressed {
            key: "Enter".into(),
            repeat: false,
        };
        r.event(&mut ev(&enter));
        assert_eq!(r.take_activated(), Some(1));
    }

    #[test]
    fn selection_clamps() {
        let mut r = NavRail::new().destination("a", "A").selected(9);
        assert_eq!(r.selected_index(), Some(0));
        r.set_selected(Some(5));
        assert_eq!(r.selected_index(), None); // out of range → None
    }

    #[test]
    fn disabled_inert() {
        let mut r = rail();
        r.enabled = false;
        let cell = r.cell_rects[0];
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(cell.origin.x + 4.0, cell.origin.y + 4.0),
            button: PointerButton::Primary,
            count: 1,
        };
        assert_eq!(r.event(&mut ev(&press)), EventResponse::Ignored);
    }

    /// Mixed rail: stroke-icon destinations around a glyph one.
    fn icon_rail() -> NavRail {
        let mut r = NavRail::new()
            .destination_named("data.grid", "Grid")
            .destination("S", "Settings")
            .destination_icon("M3 3h18v18H3z", "Box");
        let mut hot = HotNode::default();
        r.layout(&mut make_cx(&mut hot), Rect::new(0.0, 0.0, 72.0, 400.0));
        r
    }

    #[test]
    fn icon_destinations_host_internal_children() {
        let r = icon_rail();
        // Only the two stroke destinations contribute children; the
        // glyph destination doesn't (empty slot, not a child).
        assert_eq!(r.child_count(), 2);
        assert!(r.icon_widget(0).is_some());
        assert!(r.icon_widget(1).is_none());
        assert!(r.icon_widget(2).is_some());
        for i in 0..2 {
            assert_eq!(r.child(i).expect("child").debug_name(), "MorphIcon");
        }
        // Child bounds follow the icon sequence, not raw indices —
        // destination 2's icon sits in cell 2.
        let b0 = r.child_bounds(0).expect("icon 0 bounds");
        let b1 = r.child_bounds(1).expect("icon 1 bounds");
        assert!(b0.width() > 0.0 && b0.height() > 0.0);
        assert!(b1.min_y() > b0.min_y(), "icon order/bounds wrong");
        assert!(r.child_bounds(2).is_none());
    }

    #[test]
    fn icon_widgets_accept_seeds_and_morphs() {
        use crate::icons::builtin;
        let mut r = icon_rail();
        let selected = builtin::selected_pip(builtin::data::DATA_GRID);
        r.icon_widget_mut(0)
            .expect("icon")
            .morph_to(&selected, martensite_motion::SpringConfig::SNAPPY)
            .expect("pack d parses");
        assert!(r.icon_widget(0).expect("icon").is_animating());
        r.icon_widget_mut(2)
            .expect("icon")
            .set_icon(&selected)
            .expect("pack d parses");
    }

    #[test]
    fn rejected_icon_d_leaves_a_working_destination() {
        let mut r = NavRail::new().destination_icon("bogus d", "Bad");
        assert_eq!(r.destination_count(), 1);
        assert_eq!(r.child_count(), 0);
        assert!(r.icon_widget(0).is_none());
        let mut hot = HotNode::default();
        r.layout(&mut make_cx(&mut hot), Rect::new(0.0, 0.0, 72.0, 400.0));
        // The destination still activates — a bad icon must not take
        // the cell down with it.
        let cell = r.cell_rects[0];
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(cell.origin.x + 4.0, cell.origin.y + 4.0),
            button: PointerButton::Primary,
            count: 1,
        };
        assert_eq!(r.event(&mut ev(&press)), EventResponse::Handled);
        assert_eq!(r.take_activated(), Some(0));
    }

    #[test]
    fn destinations_bulk_build_hosts_icons() {
        let r = NavRail::new().destinations(vec![
            NavDestination::named("data.activity", "Telem"),
            NavDestination::new("★", "Favs"),
        ]);
        assert_eq!(r.child_count(), 1);
        assert!(r.icon_widget(0).is_some());
        assert!(r.icon_widget(1).is_none());
    }

    #[test]
    fn selection_moves_icon_ink_without_panicking() {
        let mut r = icon_rail().selected(0);
        r.set_selected(Some(2));
        assert_eq!(r.selected_index(), Some(2));
        r.set_selected(None);
        assert_eq!(r.selected_index(), None);
        let r = r.enabled(false);
        assert_eq!(r.destination_count(), 3);
    }
}
