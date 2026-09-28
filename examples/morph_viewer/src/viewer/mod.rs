//! `MorphViewer` — the showcase surface.
//!
//! One widget hosting every [`MorphIcon`] in the scene as internal
//! children: a hero card on the left, and the **icon grid** on the
//! right — a static, paged view of the pack's icons in their original
//! state. Clicking a cell selects it as the morph *base*; a second
//! click picks the *target* and the hero loops `base → target`
//! forever. Further clicks chain (target promotes to base). Clicking
//! a selected icon deselects it; `Esc` clears everything and the hero
//! returns to idle ambient morphing.
//!
//! Remote control: `pack` / `scroll` / `paused` / `select` / `base` /
//! `target` / `filter` / `sort` / `category` signals are polled in
//! `tick`, so a dev-channel `signal_trigger` (or any MCP client's
//! `martensite_set_signal` call) drives the showcase live.

use std::time::Duration;

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite::core::overlay::{AnchorEdge, OverlayAnchor, OverlayLayer};
use martensite::core::{
    shape::Shape, EventContext, EventResponse, GradientStop, GradientStops, LayoutConstraints,
    LayoutContext, PaintContext, PointerButton, Rect, Widget, WidgetEvent,
};
use martensite::motion::SpringConfig;
use martensite::prelude::Signal;
use martensite::theme::TokenKey;
use martensite::widgets::MorphIcon;

use crate::icons::{self, IconDef, PackDef};

/// Hero auto-advance after this many seconds with no selection.
mod domain;
mod filter_panel;
mod selection;
#[cfg(test)]
mod tests;

use filter_panel::FilterPanel;
const HERO_IDLE_S: f32 = 3.2;
/// Base→target loop pacing: rest on the settled target, then snap
/// back to base and breathe briefly before morphing again.
const LOOP_HOLD_TARGET_S: f32 = 0.9;
const LOOP_REST_BASE_S: f32 = 0.4;
/// Grid geometry (logical pt): cell pitch and icon size.
const CELL: f32 = 58.0;
const ICON_PT: f32 = 27.0;
/// Footer hint line — also used to reserve layout space for the
/// filter pill (must stay in sync with the `paint` hint text).
const HINT: &str = "click two icons to morph · wheel scrolls · type to filter";
/// Pool cap — the wall virtualizes: only visible rows mount cells.
const MAX_CELLS: usize = 240;

/// A `(pack, icon)` pair — selections are pack-qualified so they
/// survive pack switches and can pin foreign icons into the grid.
type Sel = (u32, u32);

/// Flattened icon id across all packs: `pack_offset + icon index`.
fn global_id(pack: u32, idx: u32) -> u32 {
    icons::PACKS
        .iter()
        .take(pack as usize)
        .map(|p| p.icons.len() as u32)
        .sum::<u32>()
        + idx
}

/// Inverse of [`global_id`]; `None` when the id lands past the
/// catalog.
fn resolve_global(gid: u32) -> Option<Sel> {
    let mut rest = gid;
    for (p, pack) in icons::PACKS.iter().enumerate() {
        let len = pack.icons.len() as u32;
        if rest < len {
            return Some((p as u32, rest));
        }
        rest -= len;
    }
    None
}

fn icon_at(pack: u32, idx: u32) -> Option<&'static IconDef> {
    icons::PACKS.get(pack as usize)?.icons.get(idx as usize)
}

/// Bundled `d` strings are pre-validated at fetch time and by the
/// `every_bundled_icon_loads` test; a failure is a data regression.
fn set_icon_checked(icon: &mut MorphIcon, d: &str) {
    if let Err(e) = icon.set_icon(d) {
        debug_assert!(false, "bundled icon failed to load: {e}");
    }
}

fn morph_hero_checked(hero: &mut MorphIcon, d: &str) {
    if let Err(e) = hero.morph_to(d, SpringConfig::GENTLE) {
        debug_assert!(false, "bundled icon failed to morph: {e}");
    }
}

/// Signals wired to the dev session as adapters; the widget polls
/// them in `tick` so a socket write lands on the next frame.
#[derive(Clone)]
pub struct ViewerSignals {
    /// Pack index (`0..PACKS.len()`).
    pub pack: Signal<u32>,
    /// Grid scroll position in rows (the icon grid is a scrollable
    /// wall; this is the first visible row).
    pub scroll: Signal<u32>,
    /// Freezes hero animation (idle cycling and the morph loop).
    pub paused: Signal<bool>,
    /// Write a global icon id → same semantics as clicking that
    /// icon's cell (base → target → chain).
    pub select: Signal<u32>,
    /// Selected base icon as a global id; `-1` when unset.
    pub base: Signal<i64>,
    /// Selected target icon as a global id; `-1` when unset.
    pub target: Signal<i64>,
    /// Substring filter over icon names (case-insensitive); empty
    /// shows the whole pack.
    pub filter: Signal<String>,
    /// Result ordering: `""`/`"pack"` keeps pack order, `"az"` /
    /// `"za"` sort by name.
    pub sort: Signal<String>,
    /// First-token category filter (`"arrow"`, `"chart"`, …); `""`
    /// shows everything.
    pub category: Signal<String>,
}

/// One grid cell: a static icon child bound to a `(pack, icon)` pair.
struct Cell {
    icon: MorphIcon,
    /// Pack index of the icon currently shown.
    pack: u32,
    /// Index of the icon currently shown inside that pack.
    icon_idx: u32,
    /// Whether this slot currently maps to a display entry — empty
    /// cells don't take hover or clicks.
    filled: bool,
}

/// The showcase root widget.
pub struct MorphViewer {
    cells: Vec<Cell>,
    hero: MorphIcon,
    signals: ViewerSignals,
    bounds: Rect,
    scale: f32,
    cols: usize,
    rows: usize,
    shown: usize,
    cell_rects: Vec<Rect>,
    icon_rects: Vec<Rect>,
    tab_rects: Vec<Rect>,
    hero_card: Rect,
    filter_rect: Rect,
    funnel_rect: Rect,
    grid_rect: Rect,
    /// Filter popover intent — `sync_overlay` reconciles this into a
    /// real overlay entry (`panel_id`), which paints above every
    /// window child (hero glyph included) and dismisses on outside
    /// presses / `Escape`.
    panel_open: bool,
    panel_id: Option<u64>,
    last_panel_anchor: Option<Rect>,
    hover_cell: Option<usize>,
    hover_tab: Option<usize>,
    hover_funnel: bool,
    hover_filter: bool,
    /// The filter pill grabs keyboard when clicked; typing while
    /// unfocused re-claims it (it's the app's only text input).
    filter_focused: bool,
    /// Whether the footer hint text has room this layout — the filter
    /// pill outranks it when the footer gets cramped.
    show_hint: bool,
    /// Pack-icon indices matching `last_filter`; identity `0..len`
    /// when the filter is empty.
    filtered: Vec<u32>,
    /// The paged domain: pinned selections first (base, then target),
    /// then natural matches — slots walk this list.
    display: Vec<Sel>,
    last_filter: String,
    /// Morph selections: first click sets `base_sel`, second sets
    /// `target_sel`, third chains (target → base). Pack-qualified so
    /// a selection survives pack switches and filter misses.
    base_sel: Option<Sel>,
    target_sel: Option<Sel>,
    /// Loop pacing: `loop_at_base` marks the rest leg (settled on the
    /// base icon); `loop_rest_s` counts down to the next leg.
    loop_at_base: bool,
    loop_rest_s: f32,
    /// What the hero currently displays (`hero_pack_i` may differ
    /// from `last_pack` while a foreign-pack selection is looping).
    hero_pack_i: u32,
    hero_icon_idx: u32,
    hero_idle_s: f32,
    last_pack: u32,
    /// Grid scroll offset in logical pt; the `scroll` signal carries
    /// the integer top row.
    scroll_pt: f32,
    last_scroll: u32,
    last_sort: String,
    last_category: String,
    last_select: u32,
    last_base: i64,
    last_target: i64,
    /// Set when pack/page/hero-name semantics changed — the arena
    /// reads it through `tick_paint_only` on that frame only, so the
    /// a11y name refreshes once rather than at frame rate.
    semantic_dirty: bool,
    rng: u64,
}

impl MorphViewer {
    /// Builds the viewer; `signals` are the same handles the app layer
    /// registers with the dev session.
    pub fn new(signals: ViewerSignals) -> Self {
        // splitmix warm-up on a per-process seed so early picks aren't
        // clustered and two instances don't ripple in lockstep.
        let seed = 0x9E37_79B9_7F4A_7C15u64 ^ u64::from(std::process::id());
        Self {
            cells: Vec::new(),
            hero: MorphIcon::new().size(132.0).stroke_width(1.15),
            signals,
            bounds: Rect::default(),
            scale: 1.0,
            cols: 0,
            rows: 0,
            shown: 0,
            cell_rects: Vec::new(),
            icon_rects: Vec::new(),
            tab_rects: Vec::new(),
            hero_card: Rect::default(),
            filter_rect: Rect::default(),
            funnel_rect: Rect::default(),
            grid_rect: Rect::default(),
            panel_open: false,
            panel_id: None,
            last_panel_anchor: None,
            hover_cell: None,
            hover_tab: None,
            hover_funnel: false,
            hover_filter: false,
            filter_focused: true,
            show_hint: true,
            filtered: Vec::new(),
            display: Vec::new(),
            last_filter: String::new(),
            base_sel: None,
            target_sel: None,
            loop_at_base: true,
            loop_rest_s: 0.0,
            hero_pack_i: 0,
            hero_icon_idx: 0,
            hero_idle_s: 2.0,
            last_pack: u32::MAX,
            scroll_pt: 0.0,
            last_scroll: 0,
            last_sort: String::new(),
            last_category: String::new(),
            last_select: 0,
            last_base: -1,
            last_target: -1,
            semantic_dirty: true,
            rng: seed,
        }
    }

    fn rand(&mut self) -> u64 {
        // xorshift64* — decorrelated picks are all idle cycling needs.
        let mut x = self.rng;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.rng = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn pack(&self) -> Option<&'static PackDef> {
        icons::PACKS
            .get(self.last_pack as usize)
            .or_else(|| icons::PACKS.first())
    }
}

impl Widget for MorphViewer {
    fn measure(&mut self, _cx: &mut LayoutContext, _c: LayoutConstraints) -> Vec2 {
        Vec2::new(960.0, 640.0)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        let s = self.scale.max(0.01);
        let pt = |v: f32| v * s;
        let m = pt(26.0);
        let header_h = pt(64.0);
        let footer_h = pt(30.0);

        // --- header: pack tabs, right-aligned (text width measured
        // through the ambient shaper; falls back to a rough advance) ---
        let cy = bounds.origin.y + header_h * 0.5;
        let mut tx = bounds.max_x() - m;
        let mut tabs: Vec<Rect> = Vec::with_capacity(icons::PACKS.len());
        for pack in icons::PACKS {
            let tw = cx
                .measure_text(pack.name, 12.5)
                .unwrap_or(pack.name.len() as f32 * pt(6.5));
            let w = tw + pt(22.0);
            tx -= w;
            tabs.push(Rect::new(tx, cy - pt(13.0), w, pt(26.0)));
            tx -= pt(8.0);
        }
        tabs.reverse();
        self.tab_rects = tabs;

        // --- hero card, left column ---
        let body_y = bounds.origin.y + header_h + pt(10.0);
        let body_h = (bounds.max_y() - footer_h - m * 0.5 - body_y).max(0.0);
        let hero_w = pt(300.0).min(bounds.size.x * 0.3);
        self.hero_card = Rect::new(bounds.origin.x + m, body_y, hero_w, body_h);
        let hero_d = pt(132.0).min(hero_w - pt(60.0));
        let hero_icon_rect = Rect::new(
            self.hero_card.origin.x + (hero_w - hero_d) * 0.5,
            body_y + body_h * 0.40 - hero_d * 0.5,
            hero_d,
            hero_d,
        );
        cx.layout_child(&mut self.hero, hero_icon_rect);

        // --- icon grid, right of the hero card ---
        let gx = self.hero_card.max_x() + pt(24.0);
        let gw = (bounds.max_x() - m - gx).max(0.0);
        self.grid_rect = Rect::new(gx, body_y, gw, body_h);
        self.cols = (gw / pt(CELL)).floor().max(1.0) as usize;
        self.rows = (body_h / pt(CELL)).floor().max(1.0) as usize;
        self.shown = (self.cols * self.rows).min(MAX_CELLS);
        while self.cells.len() < self.shown {
            self.cells.push(Cell {
                icon: MorphIcon::new()
                    .size(ICON_PT)
                    .stroke_width(1.1)
                    .decorative(true),
                pack: 0,
                icon_idx: 0,
                filled: false,
            });
        }
        self.cell_rects.clear();
        self.icon_rects.clear();
        let pitch = pt(CELL);
        let grid_w = self.cols as f32 * pitch;
        let grid_h = self.rows as f32 * pitch;
        let ox = gx + (gw - grid_w) * 0.5;
        let oy = body_y + (body_h - grid_h) * 0.5;
        for i in 0..self.shown {
            let (col, row) = (i % self.cols, i / self.cols);
            let cell = Rect::new(
                ox + col as f32 * pitch,
                oy + row as f32 * pitch,
                pitch,
                pitch,
            );
            let inset = (pitch - pt(ICON_PT)) * 0.5;
            let icon_rect = Rect::new(
                cell.origin.x + inset,
                cell.origin.y + inset,
                pt(ICON_PT),
                pt(ICON_PT),
            );
            self.cell_rects.push(cell);
            self.icon_rects.push(icon_rect);
            cx.layout_child(&mut self.cells[i].icon, icon_rect);
        }
        self.icon_rects.push(hero_icon_rect);

        // --- footer: the search pill leads, the funnel pill hangs
        // off its right edge, and the hint is right-aligned so the
        // pill sizes to whatever room remains ---
        let fy = bounds.max_y() - footer_h;
        let fx0 = bounds.origin.x + m;
        let funnel_w = pt(30.0);
        let hint_w = cx
            .measure_text(HINT, 10.5)
            .unwrap_or(HINT.len() as f32 * 5.5)
            * s;
        // The pill is functional, the hint decorative — on a cramped
        // footer the hint yields its reservation.
        let mut fw =
            (bounds.max_x() - m - hint_w - pt(14.0) - funnel_w - pt(8.0) - fx0).min(pt(300.0));
        self.show_hint = fw >= pt(90.0);
        if !self.show_hint {
            fw = (bounds.max_x() - m - funnel_w - pt(8.0) - fx0).min(pt(300.0));
        }
        self.filter_rect = if fw >= pt(60.0) {
            Rect::new(fx0, fy - pt(3.0), fw, pt(30.0))
        } else {
            // No room even then — keyboard input still claims the
            // filter; the pill collapses instead of overlapping.
            Rect::default()
        };
        self.funnel_rect = Rect::new(
            self.filter_rect.max_x() + pt(8.0),
            fy - pt(3.0),
            funnel_w,
            pt(30.0),
        );

        // First layout seeds every icon settled — a boot full of
        // mid-flight morphs reads as a glitch, not a showcase.
        if self.last_pack == u32::MAX {
            self.last_pack = self.signals.pack.get();
            self.last_scroll = self.signals.scroll.get();
            self.scroll_pt = self.last_scroll as f32 * CELL;
            self.last_select = self.signals.select.get();
            self.last_base = self.signals.base.get();
            self.last_target = self.signals.target.get();
            self.last_sort = self.signals.sort.get();
            self.last_category = self.signals.category.get();
            self.base_sel = self
                .signals
                .base
                .get()
                .try_into()
                .ok()
                .and_then(resolve_global);
            self.target_sel = self
                .signals
                .target
                .get()
                .try_into()
                .ok()
                .and_then(resolve_global);
            self.last_filter = self.signals.filter.get();
            self.recompute_filtered();
            self.rebuild_display();
            self.clamp_scroll();
            for slot in 0..self.shown {
                if let Some((p, i)) =
                    self.slot_icon(self.last_scroll as usize * self.cols.max(1) + slot)
                {
                    let cell = &mut self.cells[slot];
                    cell.filled = true;
                    cell.pack = p;
                    cell.icon_idx = i;
                    if let Some(icon) = icon_at(p, i) {
                        set_icon_checked(&mut cell.icon, icon.d);
                    }
                }
            }
            if let Some(first) = self.pack().and_then(|p| p.icons.first()) {
                set_icon_checked(&mut self.hero, first.d);
                self.hero.set_label(first.name.to_string());
            }
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerMoved { position } => {
                let cell = self.hit_cell(*position);
                let tab = self.hit_tab(*position);
                let funnel = self.funnel_rect.contains(*position);
                let filter = self.filter_rect.contains(*position);
                let changed = cell != self.hover_cell
                    || tab != self.hover_tab
                    || funnel != self.hover_funnel
                    || filter != self.hover_filter;
                self.hover_cell = cell;
                self.hover_tab = tab;
                self.hover_funnel = funnel;
                self.hover_filter = filter;
                if changed {
                    EventResponse::RequestRepaint
                } else {
                    EventResponse::Handled
                }
            }
            WidgetEvent::PointerPressed {
                position, button, ..
            } => {
                if *button != PointerButton::Primary {
                    return EventResponse::Ignored;
                }
                // Popover row presses are handled by the overlay
                // entry itself; outside presses light-dismiss it
                // before they reach us.
                let over_filter = self.filter_rect.contains(*position);
                if over_filter != self.filter_focused {
                    self.filter_focused = over_filter;
                    if !over_filter {
                        // Clicking away commits the field visually;
                        // the filter itself stays applied.
                    }
                }
                if over_filter {
                    return EventResponse::Handled;
                }
                if self.funnel_rect.contains(*position) {
                    self.panel_open = !self.panel_open;
                    return EventResponse::Handled;
                }
                if let Some(tab) = self.hit_tab(*position) {
                    self.signals.pack.set(tab as u32);
                    self.scroll_pt = 0.0;
                    self.clamp_scroll();
                    return EventResponse::Handled;
                }
                if let Some(cell_i) = self.hit_cell(*position) {
                    let cell = &self.cells[cell_i];
                    self.pick((cell.pack, cell.icon_idx));
                    return EventResponse::Handled;
                }
                EventResponse::Ignored
            }
            WidgetEvent::Scroll { position, delta } => {
                // The wall is the only scrollable region — wheel
                // anywhere scrolls it (popups take precedence via
                // the overlay layer's first-pass dispatch).
                if !self.bounds.contains(*position) {
                    return EventResponse::Ignored;
                }
                // Wheel up (positive delta) scrolls toward the top.
                // The app layer normalizes deltas to device px
                // (trackpad pixel deltas included) — pt here.
                let before = self.scroll_pt;
                self.scroll_pt -= delta.y / self.scale.max(0.01);
                self.clamp_scroll();
                if self.scroll_pt != before {
                    self.cascade();
                }
                EventResponse::Handled
            }
            WidgetEvent::KeyPressed { key, .. } => {
                match key.as_str() {
                    "Backspace" if self.filter_focused => {
                        let mut f = self.signals.filter.get();
                        if f.pop().is_some() {
                            self.signals.filter.set(f);
                        }
                        EventResponse::Handled
                    }
                    "Escape" => {
                        // Escape clears the filter, the icon
                        // selection, closes the popover, and drops
                        // focus — the hero resumes idle morphing.
                        self.filter_focused = false;
                        self.panel_open = false;
                        if !self.signals.filter.get().is_empty() {
                            self.signals.filter.set(String::new());
                        }
                        self.clear_selection();
                        EventResponse::Handled
                    }
                    "Enter" if self.filter_focused => {
                        self.filter_focused = false;
                        EventResponse::Handled
                    }
                    "ArrowUp" | "ArrowLeft" => {
                        self.scroll_pt -= CELL;
                        self.clamp_scroll();
                        self.cascade();
                        EventResponse::Handled
                    }
                    "ArrowDown" | "ArrowRight" => {
                        self.scroll_pt += CELL;
                        self.clamp_scroll();
                        self.cascade();
                        EventResponse::Handled
                    }
                    "PageUp" => {
                        self.scroll_pt -= self.rows as f32 * CELL;
                        self.clamp_scroll();
                        self.cascade();
                        EventResponse::Handled
                    }
                    "PageDown" => {
                        self.scroll_pt += self.rows as f32 * CELL;
                        self.clamp_scroll();
                        self.cascade();
                        EventResponse::Handled
                    }
                    _ => EventResponse::Ignored,
                }
            }
            WidgetEvent::ImeCommitted { text } => {
                // Typing anywhere claims the filter — it's the only
                // text field in the app.
                self.filter_focused = true;
                let mut f = self.signals.filter.get();
                let before = f.len();
                for ch in text.chars() {
                    if !ch.is_control() {
                        f.push(ch);
                    }
                }
                if f.len() != before {
                    self.signals.filter.set(f);
                }
                EventResponse::Handled
            }
            WidgetEvent::PointerLeave => {
                let had_hover =
                    self.hover_cell.is_some() || self.hover_tab.is_some() || self.hover_funnel;
                self.hover_cell = None;
                self.hover_tab = None;
                self.hover_funnel = false;
                if had_hover {
                    EventResponse::RequestRepaint
                } else {
                    EventResponse::Ignored
                }
            }
            _ => EventResponse::Ignored,
        }
    }

    fn tick(&mut self, dt: Duration) -> bool {
        // The flag from the previous frame has already been read by
        // `tick_paint_only`; a fresh frame starts paint-only unless a
        // semantic change happens below.
        self.semantic_dirty = false;
        let mut changed = false;

        // --- signal reconciliation ---
        let filter = self.signals.filter.get();
        let sort = self.signals.sort.get();
        let category = self.signals.category.get();
        let domain_changed =
            filter != self.last_filter || sort != self.last_sort || category != self.last_category;
        if domain_changed {
            self.last_filter = filter;
            self.last_sort = sort;
            self.last_category = category;
            self.recompute_filtered();
            self.rebuild_display();
            // A narrowing domain invalidates the scroll position —
            // restart at the first match. A popover pick lands here:
            // the panel closes once its choice applies.
            self.scroll_pt = 0.0;
            self.panel_open = false;
        }
        let pack_i = self.signals.pack.get();
        let scroll_i = self.signals.scroll.get();
        if pack_i != self.last_pack || scroll_i != self.last_scroll || domain_changed {
            let pack_changed = pack_i != self.last_pack;
            self.last_pack = pack_i.min(icons::PACKS.len().saturating_sub(1) as u32);
            if pack_changed {
                // The filter domain is per-pack — re-resolve before
                // the cascade walks it; the popover's category list
                // went stale, so it closes.
                self.recompute_filtered();
                self.rebuild_display();
                self.panel_open = false;
            }
            // The signal row wins only when it wasn't our own write.
            if scroll_i != self.last_scroll {
                self.scroll_pt = scroll_i as f32 * CELL;
                self.last_scroll = scroll_i;
            }
            self.clamp_scroll();
            self.cascade();
            changed = true;
        }
        // `select` = click semantics on a global icon id.
        let select = self.signals.select.get();
        if select != self.last_select {
            self.last_select = select;
            if let Some(sel) = resolve_global(select) {
                self.pick(sel);
            }
            changed = true;
        }
        // `base`/`target` are the same selection observable (and
        // writable) as global ids; `-1` clears.
        let base_sig = self.signals.base.get();
        let target_sig = self.signals.target.get();
        if base_sig != self.last_base || target_sig != self.last_target {
            let mut nb = u32::try_from(base_sig).ok().and_then(resolve_global);
            let mut nt = u32::try_from(target_sig).ok().and_then(resolve_global);
            // A target without a base promotes to base — same rule as
            // deselecting the base by click.
            if nb.is_none() {
                nb = nt.take();
            }
            self.base_sel = nb;
            self.target_sel = nt;
            self.sync_sel_signals();
            self.apply_selection();
            self.rebuild_display();
            self.clamp_scroll();
            self.cascade();
            changed = true;
        }

        // --- hero ---
        if !self.signals.paused.get() {
            if let (Some(b), Some(t)) = (self.base_sel, self.target_sel) {
                // Looping pair: settle on target, snap to base,
                // breathe, morph again.
                if !self.hero.is_animating() {
                    self.loop_rest_s -= dt.as_secs_f32();
                    if self.loop_rest_s <= 0.0 {
                        if self.loop_at_base {
                            if let Some(icon) = icon_at(t.0, t.1) {
                                morph_hero_checked(&mut self.hero, icon.d);
                            }
                            self.loop_at_base = false;
                            self.loop_rest_s = LOOP_HOLD_TARGET_S;
                        } else {
                            if let Some(icon) = icon_at(b.0, b.1) {
                                set_icon_checked(&mut self.hero, icon.d);
                            }
                            self.loop_at_base = true;
                            self.loop_rest_s = LOOP_REST_BASE_S;
                        }
                        changed = true;
                    }
                }
            } else if self.base_sel.is_none() && !self.filtered.is_empty() {
                // No selection — ambient idle cycling.
                self.hero_idle_s += dt.as_secs_f32();
                if self.hero_idle_s >= HERO_IDLE_S {
                    let len = self.filtered_len().max(1);
                    let pos = self.rand() as usize % len;
                    let next = self.filtered.get(pos).copied().unwrap_or(0) as usize;
                    self.morph_hero_to(next);
                    changed = true;
                }
            }
        }
        changed
    }

    /// Reconciles the funnel popover with the overlay layer — the
    /// entry paints above every window child (hero glyph included)
    /// and the layer handles outside-press / `Escape` dismissal.
    fn sync_overlay(&mut self, overlay: &mut OverlayLayer) {
        // Dismissed at the layer level (outside press / Escape).
        if let Some(id) = self.panel_id {
            if !overlay.is_open(id) {
                self.panel_id = None;
                self.panel_open = false;
                self.last_panel_anchor = None;
            }
        }
        if self.panel_open && self.panel_id.is_none() {
            let panel = FilterPanel::new(self.categories(), self.signals.clone());
            let anchor = OverlayAnchor::BoundsEdge {
                rect: self.funnel_rect,
                edge: AnchorEdge::Top,
            };
            self.panel_id = Some(overlay.open(Box::new(panel), anchor));
            self.last_panel_anchor = Some(self.funnel_rect);
        } else if !self.panel_open {
            if let Some(id) = self.panel_id.take() {
                overlay.close(id);
            }
            self.last_panel_anchor = None;
        } else if let Some(id) = self.panel_id {
            // The funnel moved under an open popup (relayout) —
            // re-anchor so it tracks.
            if self.last_panel_anchor != Some(self.funnel_rect) {
                overlay.set_anchor(
                    id,
                    OverlayAnchor::BoundsEdge {
                        rect: self.funnel_rect,
                        edge: AnchorEdge::Top,
                    },
                );
                self.last_panel_anchor = Some(self.funnel_rect);
            }
        }
    }

    fn tick_paint_only(&self) -> bool {
        // Morph waves are pure geometry; pack/page/hero-name changes
        // flag one semantic frame so the a11y name follows.
        !self.semantic_dirty
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Group);
        if let Some(pack) = self.pack() {
            let hero_name = match (self.base_sel, self.target_sel) {
                (Some(b), Some(t)) => format!("{} → {}", self.sel_name(b), self.sel_name(t)),
                (Some(b), None) => self.sel_name(b).to_string(),
                _ => icon_at(self.hero_pack_i, self.hero_icon_idx)
                    .map(|i| i.name)
                    .unwrap_or("nothing")
                    .to_string(),
            };
            let filter_note = if self.last_filter.is_empty() && self.last_category.is_empty() {
                format!("{} icons", pack.icons.len())
            } else {
                let mut parts = Vec::new();
                if !self.last_filter.is_empty() {
                    parts.push(format!("matching \"{}\"", self.last_filter));
                }
                if !self.last_category.is_empty() {
                    parts.push(format!("in {}", self.last_category));
                }
                format!("{} icons {}", self.filtered_len(), parts.join(" "))
            };
            node.set_label(format!(
                "Morph viewer — {} pack, {filter_note}. Hero icon: {hero_name}.",
                pack.name,
            ));
        } else {
            node.set_label("Morph viewer — no icon packs fetched");
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let b = self.bounds;
        if b.width() <= 0.0 {
            return;
        }
        let s = self.scale;
        let pt = |v: f32| v * s;
        let k = |r: Rect| {
            kurbo::Rect::new(
                f64::from(r.min_x()),
                f64::from(r.min_y()),
                f64::from(r.max_x()),
                f64::from(r.max_y()),
            )
        };
        fn text(cx: &mut PaintContext, x: f32, y: f32, t: &str, size_px: f32, color: [u8; 4]) {
            let origin = kurbo::Point::new(f64::from(x), f64::from(y));
            if let Some(p) = cx.text_painter {
                p.paint_shaped_text(cx.list, origin, t, size_px, color);
            } else {
                cx.list.push_text(origin, t.to_string(), size_px, color);
            }
        }
        let text_w = |cx: &PaintContext, t: &str, size_px: f32| -> f32 {
            cx.text_painter
                .and_then(|p| p.measure_text(t, size_px))
                .unwrap_or(t.len() as f32 * size_px * 0.52)
        };
        // Ellipsis-truncates `t` to `max_w` at `size_px` — long icon
        // names can't overflow their card.
        let fit_text = |cx: &PaintContext, t: &str, size_px: f32, max_w: f32| -> String {
            if max_w <= 0.0 || text_w(cx, t, size_px) <= max_w {
                return t.to_string();
            }
            let ell = "…";
            let ell_w = text_w(cx, ell, size_px);
            let mut cut = t.len();
            for (i, _) in t.char_indices().rev() {
                cut = i;
                if text_w(cx, &t[..cut], size_px) + ell_w <= max_w {
                    break;
                }
            }
            format!("{}{ell}", t[..cut].trim_end())
        };

        let ink = cx.color(TokenKey::TextColor, [226, 230, 240, 255]);
        let dim = cx.color(TokenKey::TextMutedColor, [122, 130, 150, 255]);
        let accent = cx.color(TokenKey::AccentColor, [96, 165, 250, 255]);
        let surface = cx.color(TokenKey::SurfaceColor, [13, 15, 21, 255]);
        let raised = cx.color(TokenKey::RaisedColor, [22, 26, 35, 255]);
        let border = cx.color(TokenKey::BorderColor, [43, 48, 63, 255]);

        // Background + a soft accent glow behind the hero card.
        cx.list.push_fill_rect(k(b), surface);
        let glow = kurbo::Rect::new(
            f64::from(self.hero_card.min_x() - pt(70.0)),
            f64::from(self.hero_card.min_y() - pt(50.0)),
            f64::from(self.hero_card.max_x() + pt(90.0)),
            f64::from(self.hero_card.min_y() + self.hero_card.height() * 0.55),
        );
        let clip_rect = kurbo::Rect::new(
            f64::from(self.hero_card.origin.x),
            f64::from(self.hero_card.origin.y),
            f64::from(self.hero_card.max_x()),
            f64::from(self.hero_card.max_y()),
        );
        cx.list.push_clip_rounded(clip_rect, pt(16.0));
        let glow_center = [
            f64::from(self.hero_card.origin.x + self.hero_card.size.x * 0.5),
            f64::from(self.hero_card.origin.y + self.hero_card.height() * 0.32),
        ];
        cx.list.push_radial_gradient(
            glow,
            GradientStops::from_slice(&[
                GradientStop::new(0.0, [accent[0], accent[1], accent[2], 30]),
                GradientStop::new(1.0, [accent[0], accent[1], accent[2], 0]),
            ]),
            glow_center,
            f64::from(self.hero_card.size.x * 0.9),
        );
        cx.list.pop_clip();

        // Header.
        text(
            cx,
            b.origin.x + pt(26.0),
            b.origin.y + pt(4.0),
            "morph viewer",
            26.0 * s,
            ink,
        );
        text(
            cx,
            b.origin.x + pt(26.0),
            b.origin.y + pt(38.0),
            "five packs · one engine · every icon morphs",
            12.0 * s,
            dim,
        );

        // Filter pill (footer, after the pager) — focused state
        // brightens the border and shows a caret after the text.
        // A collapsed pill (cramped layout) paints nothing.
        if self.filter_rect.width() > 0.0 {
            let r = self.filter_rect;
            let focused = self.filter_focused;
            cx.list.push_fill_shape(
                k(r),
                &Shape::rounded(pt(14.0)),
                if focused || self.hover_filter {
                    [255, 255, 255, 16]
                } else {
                    [255, 255, 255, 9]
                },
            );
            cx.list.push_stroke_shape(
                k(r),
                &Shape::rounded(pt(14.0)),
                pt(1.0),
                if focused {
                    [accent[0], accent[1], accent[2], 150]
                } else {
                    border
                },
            );
            let f = self.signals.filter.get();
            let shown_text = if f.is_empty() {
                "⌕ filter icons…"
            } else {
                f.as_str()
            };
            let mut tx = r.origin.x + pt(12.0);
            let ty = r.origin.y + pt(7.5);
            text(
                cx,
                tx,
                ty,
                shown_text,
                11.5 * s,
                if f.is_empty() { dim } else { ink },
            );
            tx += text_w(cx, shown_text, 11.5 * s);
            if focused && !f.is_empty() {
                text(cx, tx + pt(2.0), ty, "▏", 11.5 * s, accent);
            }
            if !f.is_empty() {
                // Live match count sits at the pill's right edge.
                let count = format!("{}", self.filtered_len());
                let cw = text_w(cx, &count, 10.5 * s);
                text(
                    cx,
                    r.max_x() - pt(10.0) - cw,
                    ty + pt(1.0),
                    &count,
                    10.5 * s,
                    dim,
                );
            }
        }

        // Pack tabs (right-aligned pills).
        for (i, r) in self.tab_rects.iter().enumerate() {
            let active = i == self.last_pack as usize;
            let hovered = self.hover_tab == Some(i);
            if active || hovered {
                let a = if active { 38 } else { 18 };
                cx.list.push_fill_shape(
                    k(*r),
                    &Shape::rounded(pt(13.0)),
                    [accent[0], accent[1], accent[2], a],
                );
            }
            let color = if active { ink } else { dim };
            let tw = text_w(cx, icons::PACKS[i].name, 12.5 * s);
            text(
                cx,
                r.origin.x + (r.size.x - tw) * 0.5,
                r.origin.y + pt(6.5),
                icons::PACKS[i].name,
                12.5 * s,
                color,
            );
        }

        // Hero card.
        cx.list
            .push_fill_shape(k(self.hero_card), &Shape::rounded(pt(14.0)), raised);
        cx.list.push_stroke_shape(
            k(self.hero_card),
            &Shape::rounded(pt(14.0)),
            pt(1.0),
            border,
        );
        if let Some(pack) = self.pack() {
            let caption = match (self.base_sel, self.target_sel) {
                (Some(b), Some(t)) => {
                    format!("{} → {}", self.sel_name(b), self.sel_name(t))
                }
                (Some(b), None) => self.sel_name(b).to_string(),
                _ => icon_at(self.hero_pack_i, self.hero_icon_idx)
                    .map(|i| i.name)
                    .unwrap_or("nothing")
                    .to_string(),
            };
            let name_y = self.hero_card.origin.y + self.hero_card.height() * 0.66;
            let max_w = self.hero_card.size.x - pt(28.0);
            // Long names shrink once, then ellipsis-truncate.
            let (font_px, fitted) = {
                let full = 17.0 * s;
                if text_w(cx, &caption, full) <= max_w {
                    (full, caption.clone())
                } else {
                    let small = 13.5 * s;
                    (small, fit_text(cx, &caption, small, max_w))
                }
            };
            let tw = text_w(cx, &fitted, font_px);
            text(
                cx,
                self.hero_card.origin.x + (self.hero_card.size.x - tw) * 0.5,
                name_y,
                &fitted,
                font_px,
                ink,
            );
            let meta = format!(
                "{} · {} icons · {}",
                pack.name,
                pack.icons.len(),
                pack.license
            );
            let meta = fit_text(cx, &meta, 11.0 * s, max_w);
            let mw = text_w(cx, &meta, 11.0 * s);
            text(
                cx,
                self.hero_card.origin.x + (self.hero_card.size.x - mw) * 0.5,
                name_y + pt(24.0),
                &meta,
                11.0 * s,
                dim,
            );
            let total = self.display_len();
            let last_row = total.div_ceil(self.cols.max(1)).saturating_sub(1);
            let scroll_txt = if last_row == 0 {
                format!("{total} icons")
            } else {
                format!(
                    "row {} / {}",
                    self.last_scroll.min(last_row as u32) + 1,
                    last_row + 1
                )
            };
            let pw = text_w(cx, &scroll_txt, 11.0 * s);
            text(
                cx,
                self.hero_card.origin.x + (self.hero_card.size.x - pw) * 0.5,
                name_y + pt(42.0),
                &scroll_txt,
                11.0 * s,
                dim,
            );
            let src = pack.source.trim_start_matches("https://github.com/");
            let sw = text_w(cx, src, 10.0 * s);
            text(
                cx,
                self.hero_card.origin.x + (self.hero_card.size.x - sw) * 0.5,
                self.hero_card.max_y() - pt(22.0),
                src,
                10.0 * s,
                dim,
            );
        } else {
            text(
                cx,
                self.hero_card.origin.x + pt(20.0),
                self.hero_card.origin.y + pt(44.0),
                "no icon packs fetched — run:",
                12.0 * s,
                dim,
            );
            text(
                cx,
                self.hero_card.origin.x + pt(20.0),
                self.hero_card.origin.y + pt(64.0),
                "cargo run -p morph_viewer --bin fetch_icons",
                11.0 * s,
                accent,
            );
        }

        // Empty-result state over the grid area (no matches AND
        // nothing pinned by a selection).
        if self.display.is_empty() && self.pack().is_some() {
            let msg = format!("no icons match \"{}\"", self.last_filter);
            let mw = text_w(cx, &msg, 14.0 * s);
            text(
                cx,
                self.grid_rect.origin.x + (self.grid_rect.size.x - mw) * 0.5,
                self.grid_rect.origin.y + self.grid_rect.size.y * 0.42,
                &msg,
                14.0 * s,
                ink,
            );
            let sub = "Esc clears the filter · scroll to browse";
            let sw = text_w(cx, sub, 11.5 * s);
            text(
                cx,
                self.grid_rect.origin.x + (self.grid_rect.size.x - sw) * 0.5,
                self.grid_rect.origin.y + self.grid_rect.size.y * 0.42 + pt(24.0),
                sub,
                11.5 * s,
                dim,
            );
        }

        // Selection rings: base in accent, target in success — inset
        // so adjacent rings never touch.
        let success = cx.color(TokenKey::SuccessColor, [74, 222, 128, 255]);
        for (i, cell) in self.cells.iter().enumerate().take(self.shown) {
            if !cell.filled {
                continue;
            }
            let sel = (cell.pack, cell.icon_idx);
            let ring = if self.base_sel == Some(sel) {
                Some(accent)
            } else if self.target_sel == Some(sel) {
                Some(success)
            } else {
                None
            };
            if let (Some(color), Some(&r)) = (ring, self.cell_rects.get(i)) {
                let ri = Rect::new(
                    r.origin.x + pt(3.5),
                    r.origin.y + pt(3.5),
                    r.size.x - pt(7.0),
                    r.size.y - pt(7.0),
                );
                cx.list.push_fill_shape(
                    k(ri),
                    &Shape::rounded(pt(7.0)),
                    [color[0], color[1], color[2], 26],
                );
                cx.list.push_stroke_shape(
                    k(ri),
                    &Shape::rounded(pt(7.0)),
                    pt(1.2),
                    [color[0], color[1], color[2], 190],
                );
            }
        }
        // Hover ring on filled cells only — empty slots stay flat.
        if let Some(i) = self.hover_cell.filter(|&i| self.cells[i].filled) {
            if let Some(&r) = self.cell_rects.get(i) {
                let ri = Rect::new(
                    r.origin.x + pt(3.5),
                    r.origin.y + pt(3.5),
                    r.size.x - pt(7.0),
                    r.size.y - pt(7.0),
                );
                cx.list.push_fill_shape(
                    k(ri),
                    &Shape::rounded(pt(7.0)),
                    [accent[0], accent[1], accent[2], 22],
                );
                cx.list.push_stroke_shape(
                    k(ri),
                    &Shape::rounded(pt(7.0)),
                    pt(1.0),
                    [accent[0], accent[1], accent[2], 120],
                );
            }
        }

        // Grid scrollbar — a thin track on the wall's right edge.
        let max_scroll = self.max_scroll_pt();
        if max_scroll > 0.0 && self.grid_rect.width() > 0.0 {
            let track_x = self.grid_rect.max_x() - pt(4.0);
            let track = Rect::new(
                track_x,
                self.grid_rect.origin.y,
                pt(3.0),
                self.grid_rect.size.y,
            );
            cx.list
                .push_fill_shape(k(track), &Shape::rounded(pt(1.5)), [255, 255, 255, 14]);
            let frac = self.scroll_pt / max_scroll;
            let thumb_h = (self.grid_rect.size.y
                * (self.grid_rect.size.y / (self.grid_rect.size.y + max_scroll)))
                .max(pt(18.0));
            let thumb_y =
                self.grid_rect.origin.y + frac * (self.grid_rect.size.y - thumb_h).max(0.0);
            cx.list.push_fill_shape(
                k(Rect::new(track_x, thumb_y, pt(3.0), thumb_h)),
                &Shape::rounded(pt(1.5)),
                [accent[0], accent[1], accent[2], 120],
            );
        }

        // Funnel pill — opens the filter popover.
        {
            let r = self.funnel_rect;
            let hot = self.hover_funnel || self.panel_open;
            cx.list.push_fill_shape(
                k(r),
                &Shape::rounded(pt(12.0)),
                if hot {
                    [255, 255, 255, 28]
                } else {
                    [255, 255, 255, 12]
                },
            );
            // Funnel glyph: cone outline + two stem strokes.
            let cxf = f64::from(r.origin.x + r.size.x * 0.5);
            let cyf = f64::from(r.origin.y + r.size.y * 0.5);
            let w = f64::from(pt(7.0));
            let p = |x: f64, y: f64| kurbo::Point::new(cxf + x, cyf + y);
            let mut glyph = kurbo::BezPath::new();
            glyph.move_to(p(-w, -5.0 * f64::from(s)));
            glyph.line_to(p(w, -5.0 * f64::from(s)));
            glyph.line_to(p(f64::from(pt(1.0)), f64::from(pt(1.0))));
            glyph.line_to(p(f64::from(pt(1.0)), f64::from(pt(2.0))));
            glyph.move_to(p(-w, -5.0 * f64::from(s)));
            glyph.line_to(p(f64::from(-pt(1.0)), f64::from(pt(1.0))));
            glyph.line_to(p(f64::from(-pt(1.0)), f64::from(pt(5.0))));
            let col = if hot { ink } else { dim };
            cx.list.push_stroke_path(glyph, pt(1.4), col);
        }

        if self.show_hint {
            let hint = HINT;
            let hw = text_w(cx, hint, 10.5 * s);
            text(
                cx,
                b.max_x() - pt(26.0) - hw,
                b.max_y() - pt(20.0),
                hint,
                10.5 * s,
                dim,
            );
        }
    }

    fn debug_name(&self) -> &'static str {
        "MorphViewer"
    }

    fn child_count(&self) -> usize {
        self.shown + 1 // cells + hero
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        if index < self.shown {
            self.cells.get(index).map(|c| &c.icon as &dyn Widget)
        } else {
            Some(&self.hero)
        }
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        if index < self.shown {
            self.cells
                .get_mut(index)
                .map(|c| &mut c.icon as &mut dyn Widget)
        } else {
            Some(&mut self.hero)
        }
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        self.icon_rects.get(index).copied()
    }
}
