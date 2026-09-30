//! `MorphViewer` — the showcase surface.
//!
//! One widget hosting every [`MorphIcon`] in the scene as internal
//! children: a hero card on the left, and the **icon grid** on the
//! right — a pixel-scrolled, zoomable wall (via
//! [`VirtualRows`]) of the pack's icons in their original
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

use std::time::{Duration, Instant};

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
use martensite::widgets::zoom_controls::ZoomAction;
use martensite::widgets::{
    MorphIcon, Segmented, TooltipBubble, VirtualRows, ZoomControls, DEFAULT_TOOLTIP_DELAY_MS,
};

use crate::icons::{self, IconDef, PackDef};

/// Hero auto-advance after this many seconds with no selection.
mod domain;
mod filter_panel;
mod selection;
#[cfg(test)]
mod tests;
mod wells;

use filter_panel::FilterPanel;
use wells::{IconButton, SlotWell, WellRole};
const HERO_IDLE_S: f32 = 3.2;
/// Base→target loop pacing: rest on the settled target, then snap
/// back to base and breathe briefly before morphing again.
const LOOP_HOLD_TARGET_S: f32 = 0.9;
const LOOP_REST_BASE_S: f32 = 0.4;
/// Zoom ladder — `(cell pitch, icon size)` in logical pt. The wall's
/// `VirtualRows` extents come from the pitch; `DEFAULT_ZOOM` is the
/// initial level.
const ZOOM_LEVELS: [(f32, f32); 4] = [(44.0, 20.0), (58.0, 27.0), (76.0, 36.0), (100.0, 48.0)];
const DEFAULT_ZOOM: u32 = 1;
/// Transport speed ladder — `speed` signal indices.
const SPEED_LEVELS: [f32; 3] = [0.5, 1.0, 2.0];
/// Height of the band above the wall holding count/funnel/zoom.
const GRID_HEAD_PT: f32 = 34.0;
/// Slot wells are square WCAG 2.5.8 targets.
const WELL_PT: f32 = 44.0;
/// Feedforward ghost ink — opaque muted slate, ≥3:1 against the
/// well background (WCAG 1.4.11 non-text) yet clearly dimmer than
/// the real thumbnail ink.
const GHOST_INK: [u8; 4] = [122, 130, 150, 255];

/// Optical stroke in icon units — thinner as the rendered icon grows
/// (baseline: grid icons at 27 pt render at stroke 1.1).
fn optical_stroke(size_pt: f32) -> f32 {
    1.1 * (27.0 / size_pt.max(1.0)).sqrt()
}

/// A `(pack, icon)` pair — selections are pack-qualified so they
/// survive pack switches and can pin foreign icons into the grid.
type Sel = (u32, u32);

/// Flattened icon id across all packs: `pack_offset + icon index`.
fn global_id(pack: u32, idx: u32) -> u32 {
    icons::all_packs()
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
    for (p, pack) in icons::all_packs().iter().enumerate() {
        let len = pack.icons.len() as u32;
        if rest < len {
            return Some((p as u32, rest));
        }
        rest -= len;
    }
    None
}

fn icon_at(pack: u32, idx: u32) -> Option<&'static IconDef> {
    icons::all_packs()
        .get(pack as usize)?
        .icons
        .get(idx as usize)
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
    /// Zoom level index into the pitch/icon ladder (clamped).
    pub zoom: Signal<u32>,
    /// Transport speed index (`0.5×`, `1×`, `2×`) — scales hero morph
    /// dt and loop/idle timers.
    pub speed: Signal<u32>,
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
    /// The `display` index this slot is bound to — `usize::MAX`
    /// when unbound. Rebinds fire `set_icon` only when it changes.
    item: usize,
}

/// Which hovered surface a tooltip describes.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TipFor {
    /// A grid cell — value is the `display` item index.
    Cell(usize),
    /// Hero base well.
    WellBase,
    /// Hero target well.
    WellTarget,
    /// The funnel filter button.
    Funnel,
    /// The search pill in the grid header band.
    FilterPill,
}

/// The showcase root widget.
pub struct MorphViewer {
    cells: Vec<Cell>,
    hero: MorphIcon,
    /// Hero slot wells + transport (internal children, in this order
    /// after `hero`).
    well_base: SlotWell,
    well_target: SlotWell,
    play_btn: IconButton,
    speed_seg: Segmented,
    /// `−`/`+` controls in the grid header (framework widget).
    zoom_ctl: ZoomControls,
    signals: ViewerSignals,
    bounds: Rect,
    scale: f32,
    cols: usize,
    /// Mounted cell count — `VirtualRows::visible_items().len()`.
    shown: usize,
    cell_rects: Vec<Rect>,
    icon_rects: Vec<Rect>,
    /// Bounds of the non-cell children, in child-index order after
    /// the hero: wells, play, segmented, zoom.
    extra_rects: [Rect; 5],
    tab_rects: Vec<Rect>,
    hero_card: Rect,
    /// Square the hero glyph sits in (a11y anchor).
    hero_icon_rect: Rect,
    /// Caption/meta line-box tops inside the display group.
    hero_caption_y: f32,
    hero_meta_y: f32,
    /// Progress bar track — spans the transport row's width, always
    /// reserved so the controls stack never jumps.
    progress_rect: Rect,
    /// Top of the pinned source label inside the hero card.
    hero_src_y: f32,
    filter_rect: Rect,
    /// Magnifier icon inside the search pill — 90% of the field's
    /// height, vertically centered at the left inner padding.
    search_icon_rect: Rect,
    funnel_rect: Rect,
    /// The right-hand column including its header band.
    grid_rect: Rect,
    /// The wall's scrolling viewport — `grid_rect` minus the band.
    wall_rect: Rect,
    /// Filter popover intent — `sync_overlay` reconciles this into a
    /// real overlay entry (`panel_id`), which paints above every
    /// window child (hero glyph included) and dismisses on outside
    /// presses / `Escape`.
    panel_open: bool,
    panel_id: Option<u64>,
    last_panel_anchor: Option<Rect>,
    /// Cell tooltip overlay entry + what it describes.
    tip_id: Option<u64>,
    /// Hover target armed for a tooltip (None = no tip).
    tip_for: Option<TipFor>,
    /// What the open bubble currently describes — a changed target
    /// closes and re-arms rather than re-anchoring stale text.
    tip_open: Option<TipFor>,
    /// When the current hover started — drives the tooltip delay.
    hover_since: Option<Instant>,
    hover_cell: Option<usize>,
    hover_tab: Option<usize>,
    hover_funnel: bool,
    hover_filter: bool,
    /// The filter pill grabs keyboard when clicked; typing while
    /// unfocused re-claims it (it's the app's only text input).
    filter_focused: bool,
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
    /// Wall scroll offset in logical pt — `VirtualRows` owns the math;
    /// the `scroll` signal carries the integer top row.
    scroll_pt: f32,
    last_scroll: u32,
    last_zoom: u32,
    last_speed: u32,
    /// Top item index captured before a zoom change — restored after
    /// the geometry relayout so zooming anchors the wall.
    zoom_anchor: Option<usize>,
    /// Previous-layout geometry fingerprint — `(cols, wall height pt,
    /// scale, zoom)`; a change rebinding the pool is how resizes and
    /// zooms keep every cell filled.
    prev_geo: (usize, f32, f32, u32),
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
        let well_icon = WELL_PT * 0.56;
        Self {
            cells: Vec::new(),
            hero: MorphIcon::new()
                .size(132.0)
                .stroke_width(optical_stroke(132.0)),
            well_base: {
                let mut w = SlotWell::new(WellRole::Base, well_icon, optical_stroke(well_icon));
                w.ghost_ink(GHOST_INK);
                w
            },
            well_target: {
                let mut w = SlotWell::new(WellRole::Target, well_icon, optical_stroke(well_icon));
                w.ghost_ink(GHOST_INK);
                w
            },
            play_btn: IconButton::new("▮▮", "pause"),
            speed_seg: Segmented::new()
                .options(["0.5×", "1×", "2×"])
                .selected(1)
                .label("morph speed"),
            zoom_ctl: ZoomControls::new()
                .fit(false)
                .reset(false)
                .horizontal(true)
                .readout(false)
                .label("grid zoom")
                .zoom_range(
                    ZOOM_LEVELS[0].0 / ZOOM_LEVELS[DEFAULT_ZOOM as usize].0,
                    ZOOM_LEVELS[ZOOM_LEVELS.len() - 1].0 / ZOOM_LEVELS[DEFAULT_ZOOM as usize].0,
                )
                .zoom(1.0),
            signals,
            bounds: Rect::default(),
            scale: 1.0,
            cols: 0,
            shown: 0,
            cell_rects: Vec::new(),
            icon_rects: Vec::new(),
            extra_rects: [Rect::default(); 5],
            tab_rects: Vec::new(),
            hero_card: Rect::default(),
            hero_icon_rect: Rect::default(),
            hero_caption_y: 0.0,
            hero_meta_y: 0.0,
            progress_rect: Rect::default(),
            hero_src_y: 0.0,
            filter_rect: Rect::default(),
            search_icon_rect: Rect::default(),
            funnel_rect: Rect::default(),
            grid_rect: Rect::default(),
            wall_rect: Rect::default(),
            panel_open: false,
            panel_id: None,
            last_panel_anchor: None,
            tip_id: None,
            tip_for: None,
            tip_open: None,
            hover_since: None,
            hover_cell: None,
            hover_tab: None,
            hover_funnel: false,
            hover_filter: false,
            filter_focused: true,
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
            last_zoom: u32::MAX,
            last_speed: u32::MAX,
            zoom_anchor: None,
            prev_geo: (0, 0.0, 0.0, u32::MAX),
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
        icons::all_packs()
            .get(self.last_pack as usize)
            .or_else(|| icons::all_packs().first())
    }
}

impl Widget for MorphViewer {
    fn measure(&mut self, _cx: &mut LayoutContext, _c: LayoutConstraints) -> Vec2 {
        Vec2::new(960.0, 640.0)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        let first_layout = self.last_pack == u32::MAX;
        // Zoom/speed signals seed geometry on first layout (and an
        // external write picked up by `tick` re-runs this).
        if first_layout {
            self.last_pack = self.signals.pack.get();
            self.last_zoom = self
                .signals
                .zoom
                .get()
                .min(ZOOM_LEVELS.len().saturating_sub(1) as u32);
            self.last_speed = self
                .signals
                .speed
                .get()
                .min(SPEED_LEVELS.len().saturating_sub(1) as u32);
            self.speed_seg.set_selected(self.last_speed as usize);
            self.zoom_ctl
                .set_zoom(ZOOM_LEVELS[self.last_zoom as usize].0 / ZOOM_LEVELS[1].0);
            // Signal state seeds the display list BEFORE the wall's
            // pool is sized — `visible_items` reads `display_len`.
            self.last_scroll = self.signals.scroll.get();
            self.last_select = self.signals.select.get();
            self.last_base = self.signals.base.get();
            self.last_target = self.signals.target.get();
            self.last_sort = self.signals.sort.get();
            self.last_category = self.signals.category.get();
            self.last_filter = self.signals.filter.get();
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
            self.recompute_filtered();
            self.rebuild_display();
        }
        let s = self.scale.max(0.01);
        let pt = |v: f32| v * s;
        let m = pt(26.0);
        let header_h = pt(64.0);
        let (pitch_pt, icon_pt) = self.pitch_icon_pt();

        // --- header: pack tabs, right-aligned (text width measured
        // through the ambient shaper; falls back to a rough advance) ---
        let cy = bounds.origin.y + header_h * 0.5;
        let mut tx = bounds.max_x() - m;
        let mut tabs: Vec<Rect> = Vec::with_capacity(icons::all_packs().len());
        for pack in icons::all_packs() {
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
        // The footer band is gone — the search pill lives in the
        // grid header, so body content runs to the bottom padding.
        let body_h = (bounds.max_y() - m * 0.5 - body_y).max(0.0);
        let hero_w = pt(300.0).min(bounds.size.x * 0.3);
        self.hero_card = Rect::new(bounds.origin.x + m, body_y, hero_w, body_h);
        // "Glyph top, controls bottom": the controls group (wells,
        // progress, transport) anchors to the card bottom just above
        // the pinned source line; the display group (glyph, caption,
        // meta) centers in the space above it. Order top→bottom:
        // glyph → caption → meta → wells → progress → transport →
        // source label.
        let well = pt(WELL_PT);
        let arrow_w = pt(18.0);
        let transport_h = pt(30.0);
        let progress_h = pt(3.0);
        self.hero_src_y = self.hero_card.max_y() - pt(22.0);
        let transport_y = self.hero_src_y - pt(20.0) - transport_h;
        let progress_y = transport_y - pt(12.0) - progress_h;
        let wells_y = progress_y - pt(16.0) - well;

        // --- transport: play/pause + speed segmented, centered —
        // the segments need real width or their labels clip. ---
        let transport_w = (hero_w - pt(40.0)).min(pt(240.0));
        let tx0 = self.hero_card.origin.x + (hero_w - transport_w) * 0.5;
        let play = Rect::new(tx0, transport_y, pt(30.0), transport_h);
        let seg = Rect::new(
            play.max_x() + pt(10.0),
            transport_y,
            transport_w - play.size.x - pt(10.0),
            transport_h,
        );
        cx.layout_child(&mut self.play_btn, play);
        cx.layout_child(&mut self.speed_seg, seg);
        self.extra_rects[2] = play;
        self.extra_rects[3] = seg;
        // The progress track spans the transport row's full width and
        // always reserves its height — the stack never jumps when a
        // morph starts or finishes.
        self.progress_rect = Rect::new(tx0, progress_y, transport_w, progress_h);

        // --- slot wells: base → chevron → target ---
        let wells_w = well * 2.0 + arrow_w + pt(16.0);
        let wells_x = self.hero_card.origin.x + (hero_w - wells_w) * 0.5;
        let wb = Rect::new(wells_x, wells_y, well, well);
        let wt = Rect::new(wells_x + wells_w - well, wells_y, well, well);
        cx.layout_child(&mut self.well_base, wb);
        cx.layout_child(&mut self.well_target, wt);
        self.extra_rects[0] = wb;
        self.extra_rects[1] = wt;

        // --- display group: glyph + caption + meta, centered in the
        // space between the card's 24pt top padding and the controls
        // group minus a 24pt minimum gap. ---
        let caption_h = pt(21.0);
        let meta_h = pt(14.0);
        let cap_block = caption_h + pt(4.0) + meta_h;
        let disp_top = body_y + pt(24.0);
        let disp_avail = (wells_y - pt(24.0) - disp_top).max(0.0);
        // The glyph absorbs all slack (cap 280pt); on short cards it
        // shrinks first and floors at the space that's actually left
        // (≤64pt) so sections never overlap.
        let fit_d = (disp_avail - cap_block - pt(12.0)).max(0.0);
        let hero_d = (hero_w - pt(48.0))
            .min(fit_d)
            .min(pt(280.0))
            .max(pt(64.0).min(fit_d));
        // Optical stroke follows the rendered size, not the constant.
        self.hero.set_size(hero_d / s);
        self.hero.set_stroke_width(optical_stroke(hero_d / s));
        let group_h = hero_d + pt(12.0) + cap_block;
        let gy = disp_top + ((disp_avail - group_h) * 0.5).max(0.0);
        self.hero_icon_rect = Rect::new(
            self.hero_card.origin.x + (hero_w - hero_d) * 0.5,
            gy,
            hero_d,
            hero_d,
        );
        cx.layout_child(&mut self.hero, self.hero_icon_rect);
        self.hero_caption_y = gy + hero_d + pt(12.0);
        self.hero_meta_y = self.hero_caption_y + caption_h + pt(4.0);

        // --- icon grid, right of the hero card. `grid_rect` holds the
        // header band + the wall; `wall_rect` is the scroll viewport. ---
        let gx = self.hero_card.max_x() + pt(24.0);
        let gw = (bounds.max_x() - m - gx).max(0.0);
        self.grid_rect = Rect::new(gx, body_y, gw, body_h);
        let head_h = pt(GRID_HEAD_PT);
        self.wall_rect = Rect::new(gx, body_y + head_h, gw, (body_h - head_h).max(0.0));

        // Grid header contents, right-aligned: (N) count text paints
        // in `paint`; the funnel + zoom controls are last.
        let head_cy = body_y + head_h * 0.5;
        // No readout — the quantized ladder makes a % meaningless.
        // Two 28pt buttons plus the horizontal 6pt gap ZoomControls
        // lays out between them.
        let zoom_w = pt(28.0) * 2.0 + pt(6.0);
        let zoom_r = Rect::new(
            (self.grid_rect.max_x() - zoom_w).max(gx),
            head_cy - pt(14.0),
            zoom_w,
            pt(28.0),
        );
        cx.layout_child(&mut self.zoom_ctl, zoom_r);
        self.extra_rects[4] = zoom_r;
        // The funnel's badge pill overhangs its top-right corner by
        // ~7pt; the 8pt clear gap is measured past the badge so it
        // never touches the `−` button.
        self.funnel_rect = Rect::new(
            zoom_r.min_x() - pt(8.0) - pt(7.0) - pt(30.0),
            head_cy - pt(15.0),
            pt(30.0),
            pt(30.0),
        );
        // The search pill leads the band, left-aligned on the same
        // center line as the (N) · funnel · − + cluster. It yields to
        // the cluster with a 24pt gap before shrinking under 160pt.
        let count = format!("({})", self.display_len());
        let count_w = cx
            .measure_text(&count, 11.0)
            .unwrap_or(count.len() as f32 * pt(6.0));
        let right_cluster = zoom_w + pt(8.0) + pt(37.0) + pt(8.0) + count_w;
        let search_avail = (gw - right_cluster - pt(24.0)).max(0.0);
        let search_w = search_avail.min(pt(320.0)).max(pt(160.0).min(search_avail));
        self.filter_rect = if search_w >= pt(60.0) {
            Rect::new(gx, head_cy - pt(15.0), search_w, pt(30.0))
        } else {
            Rect::default()
        };
        self.search_icon_rect = if self.filter_rect.width() > 0.0 {
            let side = self.filter_rect.size.y * 0.9;
            Rect::new(
                self.filter_rect.origin.x + pt(10.0),
                head_cy - side * 0.5,
                side,
                side,
            )
        } else {
            Rect::default()
        };
        // --- the wall: columns from the zoom pitch, pool from the
        // visible item range — no cap. ---
        let pitch = pt(pitch_pt);
        self.cols = (gw / pitch).floor().max(1.0) as usize;
        let wall_h_pt = self.wall_rect.size.y / s;
        let pool_len = {
            let mut v = self.vrows();
            v.set_item_count(self.display_len());
            v.visible_items().len()
        };
        self.shown = pool_len;
        // Shrink the pool too — oversized dead children would still
        // be enumerated (a11y + lint) with zero-size bounds.
        self.cells.truncate(pool_len);
        while self.cells.len() < self.shown {
            self.cells.push(Cell {
                icon: MorphIcon::new()
                    .size(icon_pt)
                    .stroke_width(optical_stroke(icon_pt))
                    .decorative(true),
                pack: 0,
                icon_idx: 0,
                filled: false,
                item: usize::MAX,
            });
        }
        // Zoom changes the icon square on pooled cells too.
        for cell in self.cells.iter_mut().take(self.shown) {
            cell.icon.set_size(icon_pt);
            cell.icon.set_stroke_width(optical_stroke(icon_pt));
        }
        self.position_cells(cx);

        // First layout seeds every icon settled — a boot full of
        // mid-flight morphs reads as a glitch, not a showcase.
        if first_layout {
            self.scroll_pt = self.last_scroll as f32 * pitch_pt;
            self.sync_wells();
            self.clamp_scroll();
            self.position_cells(cx);
            self.cascade();
            if let Some(first) = self.pack().and_then(|p| p.icons.first()) {
                set_icon_checked(&mut self.hero, first.d);
                self.hero.set_label(first.name.to_string());
            }
        } else {
            // Resize/zoom relayout: re-clamp the pixel offset and
            // rebind every visible cell — the pool mapping changes
            // with the geometry.
            let geo = (self.cols, wall_h_pt, self.scale, self.last_zoom);
            if self.prev_geo != geo {
                self.clamp_scroll();
                self.cascade();
            }
            self.sync_wells();
        }
        self.prev_geo = (self.cols, wall_h_pt, self.scale, self.last_zoom);
        // A zoom anchor pending from a `tick` relayout restores the
        // item that sat at the viewport top.
        if let Some(anchor) = self.zoom_anchor.take() {
            self.scroll_pt = (anchor / self.cols) as f32 * pitch_pt;
            self.clamp_scroll();
            self.position_cells(cx);
            self.cascade();
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        // Interactive internal children first: wells, transport,
        // segmented speed, zoom controls.
        match self.forward_controls(cx) {
            EventResponse::Ignored => {}
            r => return r,
        }
        match cx.event {
            WidgetEvent::PointerMoved { position } => {
                let cell = self.hit_cell(*position);
                let tab = self.hit_tab(*position);
                let funnel = self.funnel_rect.contains(*position);
                let filter = self.filter_rect.contains(*position);
                let wb = self.extra_rects[0].contains(*position);
                let wt = self.extra_rects[1].contains(*position);
                let cell_changed = cell != self.hover_cell;
                let changed = cell_changed
                    || tab != self.hover_tab
                    || funnel != self.hover_funnel
                    || filter != self.hover_filter;
                self.well_base.set_hovered(wb);
                self.well_target.set_hovered(wt);
                self.hover_cell = cell;
                self.hover_tab = tab;
                self.hover_funnel = funnel;
                self.hover_filter = filter;
                // Feedforward ghosts rebind only on a hovered-CELL
                // change — pointer jitter within a cell rebinds
                // nothing.
                if cell_changed {
                    self.sync_ghosts();
                }
                // The tooltip target follows whatever the pointer
                // now rests on.
                self.update_hover_target(*position);
                if changed || wb || wt {
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
                // Clicks hide the tooltip.
                self.tip_for = None;
                // Popover row presses are handled by the overlay
                // entry itself; outside presses light-dismiss it
                // before they reach us.
                let over_filter = self.filter_rect.contains(*position);
                if over_filter != self.filter_focused {
                    self.filter_focused = over_filter;
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
                let mut v = self.vrows();
                let consumed = v.scroll_by(-delta.y / self.scale.max(0.01));
                self.scroll_pt = v.offset();
                self.clamp_scroll();
                if consumed.abs() > f32::EPSILON {
                    self.tip_for = None;
                    self.reposition_and_cascade();
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
                        // selection, closes the popover/tooltip, and
                        // drops focus — the hero resumes idle morphing.
                        self.filter_focused = false;
                        self.panel_open = false;
                        self.tip_for = None;
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
                    "ArrowUp" => {
                        self.scroll_keys(-self.pitch_icon_pt().0);
                        EventResponse::Handled
                    }
                    "ArrowDown" => {
                        self.scroll_keys(self.pitch_icon_pt().0);
                        EventResponse::Handled
                    }
                    "PageUp" => {
                        self.scroll_keys(-self.wall_h_pt());
                        EventResponse::Handled
                    }
                    "PageDown" => {
                        self.scroll_keys(self.wall_h_pt());
                        EventResponse::Handled
                    }
                    "Home" => {
                        self.scroll_to_row(0);
                        EventResponse::Handled
                    }
                    "End" => {
                        self.scroll_to_row(u32::MAX);
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
                self.hover_since = None;
                self.tip_for = None;
                self.well_base.set_hovered(false);
                self.well_target.set_hovered(false);
                self.sync_ghosts();
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
            self.last_pack = pack_i.min(icons::all_packs().len().saturating_sub(1) as u32);
            if pack_changed {
                // The filter domain is per-pack — re-resolve before
                // the cascade walks it; the popover's category list
                // went stale, so it closes.
                self.recompute_filtered();
                self.rebuild_display();
                self.panel_open = false;
                self.sync_wells();
            }
            // The signal row wins only when it wasn't our own write.
            if scroll_i != self.last_scroll {
                self.scroll_pt = scroll_i as f32 * self.pitch_icon_pt().0;
                self.last_scroll = scroll_i;
            }
            self.clamp_scroll();
            self.reposition_and_cascade();
            changed = true;
        }
        // Zoom ladder — the control writes the signal; any path
        // (buttons, MCP, keyboard) lands here.
        let zoom_i = self
            .signals
            .zoom
            .get()
            .min(ZOOM_LEVELS.len().saturating_sub(1) as u32);
        if zoom_i != self.last_zoom && self.last_zoom != u32::MAX {
            // Anchor the item at the viewport top across the
            // geometry change.
            let (old_pitch, _) = self.pitch_icon_pt();
            self.zoom_anchor =
                Some((self.scroll_pt / old_pitch).floor().max(0.0) as usize * self.cols);
            self.last_zoom = zoom_i;
            self.zoom_ctl
                .set_zoom(ZOOM_LEVELS[zoom_i as usize].0 / ZOOM_LEVELS[1].0);
            // The geometry changed — re-run layout + rebind.
            let mut hot = martensite::core::HotNode::default();
            let mut lcx = LayoutContext {
                hot: &mut hot,
                scale: self.scale,
            };
            self.layout(&mut lcx, self.bounds);
            changed = true;
        } else if self.last_zoom == u32::MAX {
            self.last_zoom = zoom_i;
        }
        // Speed ladder — segmented control writes the signal.
        let speed_i = self
            .signals
            .speed
            .get()
            .min(SPEED_LEVELS.len().saturating_sub(1) as u32);
        if speed_i != self.last_speed {
            self.last_speed = speed_i;
            if self.speed_seg.selected_index() != speed_i as usize {
                self.speed_seg.set_selected(speed_i as usize);
            }
            self.hero.set_speed(SPEED_LEVELS[speed_i as usize]);
            changed = true;
        }
        if let Some(seg_i) = self.speed_seg.take_selected() {
            self.signals.speed.set(seg_i as u32);
        }
        // Zoom control actions → signal.
        while let Some(action) = self.zoom_ctl.take_action() {
            match action {
                ZoomAction::ZoomIn => self
                    .signals
                    .zoom
                    .set(zoom_i.saturating_add(1).min(ZOOM_LEVELS.len() as u32 - 1)),
                ZoomAction::ZoomOut => self.signals.zoom.set(zoom_i.saturating_sub(1)),
                _ => {}
            }
        }
        // Transport play/pause + well clicks.
        if self.play_btn.take_clicked() {
            self.signals.paused.set(!self.signals.paused.get());
        }
        let paused = self.signals.paused.get();
        let want_glyph = if paused { "▸" } else { "▮▮" };
        let want_label = if paused { "play" } else { "pause" };
        if self.play_btn.label() != want_label {
            self.play_btn.set_glyph(want_glyph, want_label);
        }
        self.play_btn.set_active(paused);
        if self.well_base.take_clicked() {
            // Base click → existing deselect semantics (target
            // promotes to base).
            if let Some(b) = self.base_sel {
                self.pick(b);
            }
        }
        if self.well_target.take_clicked() {
            if let Some(t) = self.target_sel {
                self.pick(t);
            }
        }
        // Tooltip delay: a stable hover older than the framework's
        // delay opens the bubble.
        if self.tip_id.is_none()
            && self.tip_for.is_some()
            && self
                .hover_since
                .is_some_and(|t| t.elapsed() >= Duration::from_millis(DEFAULT_TOOLTIP_DELAY_MS))
        {
            // `sync_overlay` opens the entry this pass.
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
            self.sync_wells();
            self.sync_ghosts();
            self.rebuild_display();
            self.clamp_scroll();
            self.cascade();
            changed = true;
        }

        // --- hero ---
        // Transport speed scales the loop/idle timers; the morph
        // itself scales inside `MorphIcon` via `set_speed`.
        let speed = SPEED_LEVELS[self.last_speed.min(SPEED_LEVELS.len() as u32 - 1) as usize];
        if !self.signals.paused.get() {
            if let (Some(b), Some(t)) = (self.base_sel, self.target_sel) {
                // Looping pair: settle on target, snap to base,
                // breathe, morph again.
                if !self.hero.is_animating() {
                    self.loop_rest_s -= dt.as_secs_f32() * speed;
                    if self.loop_rest_s <= 0.0 {
                        if self.loop_at_base {
                            if let Some(icon) = icon_at(t.0, t.1) {
                                morph_hero_checked(&mut self.hero, icon.d);
                            }
                            self.hero_pack_i = t.0;
                            self.hero_icon_idx = t.1;
                            self.loop_at_base = false;
                            self.loop_rest_s = LOOP_HOLD_TARGET_S;
                        } else {
                            if let Some(icon) = icon_at(b.0, b.1) {
                                set_icon_checked(&mut self.hero, icon.d);
                            }
                            self.hero_pack_i = b.0;
                            self.hero_icon_idx = b.1;
                            self.loop_at_base = true;
                            self.loop_rest_s = LOOP_REST_BASE_S;
                        }
                        changed = true;
                    }
                }
            } else if self.base_sel.is_none() && !self.filtered.is_empty() {
                // No selection — ambient idle cycling.
                self.hero_idle_s += dt.as_secs_f32() * speed;
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

        // --- tooltip bubble: armed by the hover delay in `tick`,
        // anchored to the hovered cell/well/pill. ---
        let tip_ready = self.tip_for.is_some()
            && self
                .hover_since
                .is_some_and(|t| t.elapsed() >= Duration::from_millis(DEFAULT_TOOLTIP_DELAY_MS));
        if !tip_ready || self.tip_for != self.tip_open {
            // Hover moved to another target (or left) — drop the open
            // bubble; the delay re-arms on the new target.
            if let Some(id) = self.tip_id.take() {
                overlay.close(id);
            }
            self.tip_open = None;
        }
        if tip_ready && self.tip_id.is_none() {
            if let Some(t) = self.tip_for {
                if let (Some(a), Some(txt)) = (self.tip_anchor(t), self.tip_text(t)) {
                    self.tip_id = Some(overlay.open(
                        Box::new(TooltipBubble::new(txt)),
                        OverlayAnchor::BoundsEdge {
                            rect: a,
                            edge: AnchorEdge::Bottom,
                        },
                    ));
                    self.tip_open = Some(t);
                } else {
                    self.tip_for = None;
                }
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

        // Header. The title and the right-aligned pack tabs share the
        // band — on narrow bounds (including the transient pre-resize
        // first frame) text that runs under a tab overlaps it, so the
        // run paints only while it fits left of the tabs.
        let tab_edge = self
            .tab_rects
            .first()
            .map(|r| r.min_x() - pt(12.0))
            .unwrap_or(b.max_x());
        let title_x = b.origin.x + pt(26.0);
        let title = "Morph Viewer";
        let title_w = text_w(cx, title, 26.0 * s);
        if title_x + title_w < tab_edge {
            // Line box centered on the same line as the pack tabs.
            text(
                cx,
                title_x,
                b.origin.y + pt(64.0) * 0.5 - 26.0 * s * 0.5,
                title,
                26.0 * s,
                ink,
            );
        }

        // Search pill (left of the grid header band) — focused state
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
                    [accent[0], accent[1], accent[2], 200]
                } else {
                    border
                },
            );
            // Magnifier — lucide "search" geometry (24-unit grid)
            // stroked at the header controls' optical weight; muted
            // ink normally, full text ink while the field is focused.
            if self.search_icon_rect.width() > 0.0 {
                let ib = self.search_icon_rect;
                let u = f64::from(ib.size.x / 24.0);
                let ix = |x: f64| f64::from(ib.origin.x) + x * u;
                let cy0 = f64::from(ib.origin.y) + 11.0 * u;
                let cx0 = f64::from(ib.origin.x) + 11.0 * u;
                let circ = kurbo::Circle::new(kurbo::Point::new(cx0, cy0), 7.0 * u);
                let mut icon_path = kurbo::Shape::to_path(&circ, 0.1);
                icon_path.move_to(kurbo::Point::new(ix(21.0), cy0 + 10.0 * u));
                icon_path.line_to(kurbo::Point::new(ix(16.65), cy0 + 5.65 * u));
                let icon_pt = ib.size.x / s;
                cx.list.push_stroke_path(
                    icon_path,
                    optical_stroke(icon_pt) * (ib.size.x / 24.0),
                    if focused { ink } else { dim },
                );
            }
            let f = self.signals.filter.get();
            let shown_text = if f.is_empty() { "Search" } else { f.as_str() };
            let tx = self.search_icon_rect.max_x() + pt(6.0);
            let ty = r.origin.y + pt(7.5);
            text(
                cx,
                tx,
                ty,
                shown_text,
                11.5 * s,
                if f.is_empty() { dim } else { ink },
            );
            if focused && !f.is_empty() {
                let caret_x = tx + text_w(cx, shown_text, 11.5 * s);
                text(cx, caret_x + pt(2.0), ty, "▏", 11.5 * s, accent);
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
            let tw = text_w(cx, icons::all_packs()[i].name, 12.5 * s);
            text(
                cx,
                r.origin.x + (r.size.x - tw) * 0.5,
                r.origin.y + pt(6.5),
                icons::all_packs()[i].name,
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
                    // Cross-pack selections name their packs so the
                    // loop's direction stays legible.
                    if b.0 != t.0 {
                        format!(
                            "{} ({}) → {} ({})",
                            self.sel_name(b),
                            icons::all_packs()[b.0 as usize].name,
                            self.sel_name(t),
                            icons::all_packs()[t.0 as usize].name,
                        )
                    } else {
                        format!("{} → {}", self.sel_name(b), self.sel_name(t))
                    }
                }
                (Some(b), None) => self.sel_name(b).to_string(),
                _ => icon_at(self.hero_pack_i, self.hero_icon_idx)
                    .map(|i| i.name)
                    .unwrap_or("nothing")
                    .to_string(),
            };
            let name_y = self.hero_caption_y;
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
            let meta = format!("{} · {}", pack.name, pack.license);
            let meta = fit_text(cx, &meta, 11.0 * s, max_w);
            let mw = text_w(cx, &meta, 11.0 * s);
            text(
                cx,
                self.hero_card.origin.x + (self.hero_card.size.x - mw) * 0.5,
                self.hero_meta_y,
                &meta,
                11.0 * s,
                dim,
            );
            let src = pack.source.trim_start_matches("https://github.com/");
            let sw = text_w(cx, src, 10.0 * s);
            text(
                cx,
                self.hero_card.origin.x + (self.hero_card.size.x - sw) * 0.5,
                self.hero_src_y,
                src,
                10.0 * s,
                dim,
            );
            // Progress track spanning the transport row — always
            // drawn so the controls stack holds still; the accent
            // fill only covers it while a morph is in flight.
            let bar = self.progress_rect;
            cx.list
                .push_fill_shape(k(bar), &Shape::rounded(pt(1.5)), [255, 255, 255, 18]);
            let progress = self.hero.progress().clamp(0.0, 1.0);
            if progress > 0.0 && progress < 1.0 {
                cx.list.push_fill_shape(
                    k(Rect::new(
                        bar.origin.x,
                        bar.origin.y,
                        bar.size.x * progress,
                        bar.size.y,
                    )),
                    &Shape::rounded(pt(1.5)),
                    accent,
                );
            }
            // Chevron between the wells — "base flows into target".
            {
                let wb = self.extra_rects[0];
                let wt = self.extra_rects[1];
                let cxm = f64::from((wb.max_x() + wt.min_x()) * 0.5);
                let cym = f64::from(wb.min_y() + wb.height() * 0.5);
                let d = f64::from(pt(4.0));
                let mut chev = kurbo::BezPath::new();
                chev.move_to(kurbo::Point::new(cxm - d * 0.6, cym - d));
                chev.line_to(kurbo::Point::new(cxm + d * 0.6, cym));
                chev.line_to(kurbo::Point::new(cxm - d * 0.6, cym + d));
                cx.list.push_stroke_path(chev, pt(1.5), dim);
            }
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

        // Empty-result state over the wall (no matches AND
        // nothing pinned by a selection).
        if self.display.is_empty() && self.pack().is_some() {
            let msg = format!("no icons match \"{}\"", self.last_filter);
            let mw = text_w(cx, &msg, 14.0 * s);
            text(
                cx,
                self.wall_rect.origin.x + (self.wall_rect.size.x - mw) * 0.5,
                self.wall_rect.origin.y + self.wall_rect.size.y * 0.42,
                &msg,
                14.0 * s,
                ink,
            );
            let sub = "Esc clears the filter · scroll to browse";
            let sw = text_w(cx, sub, 11.5 * s);
            text(
                cx,
                self.wall_rect.origin.x + (self.wall_rect.size.x - sw) * 0.5,
                self.wall_rect.origin.y + self.wall_rect.size.y * 0.42 + pt(24.0),
                sub,
                11.5 * s,
                dim,
            );
        }

        // Hover on filled cells only — a neutral light wash; accent is
        // reserved for the base selection (target is success-green).
        // The pool can shrink under a stale hover index (zoom level
        // drop truncates `cells` mid-hover) — index through `get`.
        if let Some(i) = self
            .hover_cell
            .filter(|&i| self.cells.get(i).is_some_and(|c| c.filled))
        {
            if let Some(&r) = self.cell_rects.get(i) {
                let ri = Rect::new(
                    r.origin.x + pt(3.5),
                    r.origin.y + pt(3.5),
                    r.size.x - pt(7.0),
                    r.size.y - pt(7.0),
                );
                cx.list
                    .push_fill_shape(k(ri), &Shape::rounded(pt(7.0)), [255, 255, 255, 20]);
            }
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
        // Grid scrollbar — a thin track on the wall's right edge.
        let max_scroll = self.max_scroll_pt();
        if max_scroll > 0.0 && self.wall_rect.width() > 0.0 {
            let track_x = self.wall_rect.max_x() - pt(4.0);
            let track = Rect::new(
                track_x,
                self.wall_rect.origin.y,
                pt(3.0),
                self.wall_rect.size.y,
            );
            cx.list
                .push_fill_shape(k(track), &Shape::rounded(pt(1.5)), [255, 255, 255, 14]);
            let frac = self.scroll_pt / max_scroll;
            let thumb_h = (self.wall_rect.size.y
                * (self.wall_rect.size.y / (self.wall_rect.size.y + max_scroll)))
                .max(pt(18.0));
            let thumb_y =
                self.wall_rect.origin.y + frac * (self.wall_rect.size.y - thumb_h).max(0.0);
            cx.list.push_fill_shape(
                k(Rect::new(track_x, thumb_y, pt(3.0), thumb_h)),
                &Shape::rounded(pt(1.5)),
                [accent[0], accent[1], accent[2], 120],
            );
        }

        // Grid header: right-aligned result count, then the funnel,
        // then the zoom control (child widget). A zero funnel rect
        // means no layout ran yet — skip, else the count lands on the
        // title block.
        if self.funnel_rect.width() > 0.0 {
            let count = format!("({})", self.display_len());
            let cw = text_w(cx, &count, 11.0 * s);
            // `text` y is the line box's TOP — center the box on the
            // header band rather than landing the baseline mid-band.
            let cy = self.grid_rect.origin.y + pt(GRID_HEAD_PT) * 0.5 - 5.5 * s;
            text(
                cx,
                self.funnel_rect.origin.x - pt(8.0) - cw,
                cy,
                &count,
                11.0 * s,
                dim,
            );
        }
        // Funnel pill — opens the filter popover; accent tint + a
        // badge count when filters are active.
        {
            let r = self.funnel_rect;
            let n_active = usize::from(!self.last_sort.is_empty())
                + usize::from(!self.last_category.is_empty());
            let hot = self.hover_funnel || self.panel_open;
            cx.list.push_fill_shape(
                k(r),
                &Shape::rounded(pt(12.0)),
                if n_active > 0 {
                    [accent[0], accent[1], accent[2], 44]
                } else if hot {
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
            let col = if hot || n_active > 0 { ink } else { dim };
            cx.list.push_stroke_path(glyph, pt(1.4), col);
            // Active-filter badge — the count of engaged facets.
            if n_active > 0 {
                let bd = pt(14.0);
                let bx = r.max_x() - bd * 0.5;
                let by = r.origin.y - bd * 0.35;
                let badge = Rect::new(bx, by, bd, bd);
                cx.list
                    .push_fill_shape(k(badge), &Shape::rounded(bd * 0.5), accent);
                let n = n_active.to_string();
                let nw = text_w(cx, &n, 9.0 * s);
                text(
                    cx,
                    badge.origin.x + (bd - nw) * 0.5,
                    badge.origin.y + (bd - 9.0 * s) * 0.5,
                    &n,
                    9.0 * s,
                    [12, 14, 26, 255],
                );
            }
        }
    }

    fn debug_name(&self) -> &'static str {
        "MorphViewer"
    }

    fn child_count(&self) -> usize {
        self.cells.len() + 6 // cells + hero + wells + play + speed + zoom
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        let n = self.cells.len();
        match index {
            i if i < n => self.cells.get(i).map(|c| &c.icon as &dyn Widget),
            i if i == n => Some(&self.hero),
            i if i == n + 1 => Some(&self.well_base),
            i if i == n + 2 => Some(&self.well_target),
            i if i == n + 3 => Some(&self.play_btn),
            i if i == n + 4 => Some(&self.speed_seg),
            i if i == n + 5 => Some(&self.zoom_ctl),
            _ => None,
        }
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        let n = self.cells.len();
        match index {
            i if i < n => self
                .cells
                .get_mut(i)
                .map(|c| &mut c.icon as &mut dyn Widget),
            i if i == n => Some(&mut self.hero),
            i if i == n + 1 => Some(&mut self.well_base),
            i if i == n + 2 => Some(&mut self.well_target),
            i if i == n + 3 => Some(&mut self.play_btn),
            i if i == n + 4 => Some(&mut self.speed_seg),
            i if i == n + 5 => Some(&mut self.zoom_ctl),
            _ => None,
        }
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        let n = self.cells.len();
        match index {
            i if i < n => self.icon_rects.get(i).copied(),
            i if i == n => Some(self.hero_icon_rect),
            i if i == n + 1 || i == n + 2 => self.extra_rects.get(i - (n + 1)).copied(),
            i if i == n + 3 => self.extra_rects.get(2).copied(),
            i if i == n + 4 => self.extra_rects.get(3).copied(),
            i if i == n + 5 => self.extra_rects.get(4).copied(),
            _ => None,
        }
    }

    fn clips_children(&self) -> bool {
        // The wall clips pooled cells; hero wells sit inside the card.
        true
    }

    fn child_clip(&self, index: usize) -> Option<Rect> {
        let n = self.cells.len();
        if index < n {
            Some(self.wall_rect)
        } else if index == n + 1 || index == n + 2 {
            Some(self.hero_card)
        } else {
            None
        }
    }
}
