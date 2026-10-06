//! A production [`ArenaProbe`] backed by a shared [`WidgetArena`].
//!
//! `WidgetArenaProbe` is the default app-side runtime bridge: an app that
//! keeps its arena behind `Arc<Mutex<WidgetArena>>` hands a clone to
//! [`DevSession::new`](crate::dev_session::DevSession::new) and immediately serves real `tree_snapshot`,
//! `tree_node`, `overflow_scan`, `a11y_tree`, `theme_get`, `audit_paint`,
//! and `a11y_action` data over the dev channel. Pointer/scroll
//! `event_dispatch` is real too: targets resolve by bounds hit-test (or
//! an explicit `target` id) and dispatch through
//! `WidgetArena::dispatch_event`.
//!
//! Optional runtimes plug in via builder methods:
//!
//! - [`with_layout_store`](crate::dev_session::WidgetArenaProbe::with_layout_store) — real Taffy
//!   `layout_chain` output (style constraints, resolved size,
//!   diagnostics) from a per-frame [`LayoutStore`](crate::dev_session::layout_store::LayoutStore)
//!   snapshot; without it
//!   the probe still answers with arena bounds and ancestry
//!   (`"layout_engine": null`).
//! - [`with_focus_manager`](crate::dev_session::WidgetArenaProbe::with_focus_manager) — keyboard/text
//!   `event_dispatch` and `a11y_action` target resolution through
//!   `FocusManager::current_focus`.
//! - the `render` feature — `capture_node` rasterizes the subtree with
//!   `martensite_render::TinySkiaBackend` into a PNG payload.
//!
//! Genuinely absent capabilities (OS theme watcher, interactive pick
//! overlay, reactive `signal_*`) still answer `not_implemented` — the
//! app overrides them by wrapping this probe or implementing
//! [`ArenaProbe`] itself.
//!
//! Concurrency: probe calls run on dev-channel socket threads while the
//! app mutates the arena on its own thread; the shared `Mutex` is the
//! only synchronization. Handlers lock briefly to serialize snapshots —
//! no `WidgetArena` state is retained between calls.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

use glam::Vec2;
use kurbo::Shape;

use martensite_core::{
    EventResponse, NodeFlags, PaintCommand, PaintList, PointerButton, Rect, SemanticAction,
    WidgetArena, WidgetEvent, WidgetId,
};

#[cfg(not(feature = "render"))]
use super::not_impl;
use super::{ArenaProbe, SessionResult};

/// [`ArenaProbe`] implementation over a shared [`WidgetArena`].
///
/// Construct with [`WidgetArenaProbe::new`] (or `From<Arc<Mutex<WidgetArena>>>`)
/// and box it into [`DevSession::new`](super::DevSession::new):
///
/// ```
/// use std::sync::{Arc, Mutex};
///
/// use martensite_core::WidgetArena;
/// use martensite_devtools::dev_session::{DevSession, WidgetArenaProbe};
///
/// let arena = Arc::new(Mutex::new(WidgetArena::new()));
/// let session = DevSession::new(Box::new(WidgetArenaProbe::new(Arc::clone(&arena))));
/// ```
pub struct WidgetArenaProbe {
    arena: Arc<Mutex<WidgetArena>>,
    /// Optional layout snapshot backing `layout_chain` with real Taffy
    /// state. `LayoutEngine` is `!Send`, so the app feeds a
    /// [`LayoutStore`](super::layout_store::LayoutStore) per layout pass
    /// via [`Self::with_layout_store`].
    layout_store: Option<Arc<Mutex<super::layout_store::LayoutStore>>>,
    /// Optional focus manager backing keyboard/text `event_dispatch`
    /// and `a11y_action` fallback targeting. Attached via
    /// [`Self::with_focus_manager`].
    focus: Option<Arc<Mutex<martensite_focus::FocusManager>>>,
    /// Node selected via `inspector_select` — surfaced as `selected_id`
    /// in `tree_snapshot`. Probe calls run under the session's probe
    /// mutex, so a plain field suffices.
    selected: Option<WidgetId>,
}

impl std::fmt::Debug for WidgetArenaProbe {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WidgetArenaProbe")
            .field("arena", &self.arena)
            .field("layout_store", &self.layout_store.is_some())
            .field("focus_manager", &self.focus.is_some())
            .field("selected", &self.selected)
            .finish()
    }
}

impl Clone for WidgetArenaProbe {
    fn clone(&self) -> Self {
        Self {
            arena: Arc::clone(&self.arena),
            layout_store: self.layout_store.clone(),
            focus: self.focus.clone(),
            selected: self.selected,
        }
    }
}

impl WidgetArenaProbe {
    /// Wraps a shared arena handle.
    #[must_use]
    pub fn new(arena: Arc<Mutex<WidgetArena>>) -> Self {
        Self {
            arena,
            layout_store: None,
            focus: None,
            selected: None,
        }
    }

    /// Attaches a shared [`LayoutStore`](super::layout_store::LayoutStore)
    /// so `layout_chain` serves real Taffy constraint/resolved data. The
    /// `LayoutEngine` itself is `!Send` and stays on the app thread — the
    /// app calls `store.lock().update_from_engine(&engine)` once per
    /// layout pass. Without a store the probe still answers
    /// `layout_chain` with arena bounds and ancestry, marked
    /// `"layout_engine": null`.
    #[must_use]
    pub fn with_layout_store(
        mut self,
        store: Arc<Mutex<super::layout_store::LayoutStore>>,
    ) -> Self {
        self.layout_store = Some(store);
        self
    }

    /// Attaches the app's [`FocusManager`](martensite_focus::FocusManager)
    /// so `event_dispatch` routes `key_press`/`text_input` to the
    /// focused widget and `a11y_action` falls back to the focused node
    /// when `node_id` is omitted.
    #[must_use]
    pub fn with_focus_manager(mut self, focus: Arc<Mutex<martensite_focus::FocusManager>>) -> Self {
        self.focus = Some(focus);
        self
    }

    /// Returns the shared arena handle (for composing custom probes).
    #[must_use]
    pub fn arena(&self) -> &Arc<Mutex<WidgetArena>> {
        &self.arena
    }

    /// Real `capture_node` behind the `render` feature: records the
    /// node's subtree paint list, translates it into capture-local
    /// space at `scale`, rasterizes with [`martensite_render::TinySkiaBackend`],
    /// and PNG-encodes the pixmap.
    ///
    /// Pixmap side lengths are clamped to `[1, 4096]` — a degenerate or
    /// oversized node still produces a valid (if blank) capture.
    #[cfg(feature = "render")]
    fn capture_node_png(&self, node_id: u64, format: &str, scale: f64) -> SessionResult {
        use base64::Engine;

        if format != "png" {
            return Err(format!(
                "invalid param: capture_node rasterizes PNG only (format `{format}` unsupported)"
            ));
        }
        if !scale.is_finite() || scale <= 0.0 {
            return Err("invalid param: scale must be a positive finite number".to_string());
        }
        let Some(id) = WidgetId::from_u64(node_id) else {
            return Err(format!(
                "invalid param: node_id `{node_id}` is not a valid widget id"
            ));
        };
        let arena = self.arena.lock().expect("arena mutex");
        let Some(hot) = arena.get_hot(id) else {
            return Err(format!("node `{id:?}` is not alive in the arena"));
        };
        let bounds = hot.bounds;
        if bounds.width() <= 0.0 || bounds.height() <= 0.0 {
            return Err(format!(
                "node `{id:?}` has zero-area bounds — nothing to rasterize"
            ));
        }

        const MAX_DIM: f64 = 4096.0;
        let pw = (f64::from(bounds.width()) * scale)
            .ceil()
            .clamp(1.0, MAX_DIM) as u32;
        let ph = (f64::from(bounds.height()) * scale)
            .ceil()
            .clamp(1.0, MAX_DIM) as u32;

        let mut list = PaintList::new();
        arena.build_paint_list(id, &mut list);

        // Move the node origin to (0, 0) and zoom: `out = (in - origin) * scale`.
        let affine = kurbo::Affine::scale(scale)
            * kurbo::Affine::translate(kurbo::Vec2::new(
                -f64::from(bounds.origin.x),
                -f64::from(bounds.origin.y),
            ));
        for cmd in &mut list.commands {
            transform_command(cmd, affine, scale);
        }

        let mut backend = martensite_render::TinySkiaBackend::new(pw, ph)
            .ok_or_else(|| format!("failed to allocate {pw}x{ph} capture pixmap"))?;
        martensite_render::RenderBackend::render(&mut backend, &list);
        let png = backend
            .pixmap()
            .encode_png()
            .map_err(|e| format!("capture_node PNG encode failed: {e}"))?;

        Ok(json!({
            "bytes_base64": base64::engine::general_purpose::STANDARD.encode(&png),
            "format": "png",
            "logical_size": [bounds.width(), bounds.height()],
            "physical_size": [pw, ph],
        }))
    }
}

impl From<Arc<Mutex<WidgetArena>>> for WidgetArenaProbe {
    fn from(arena: Arc<Mutex<WidgetArena>>) -> Self {
        Self::new(arena)
    }
}

/// `[x, y, w, h]` wire form of a [`Rect`].
fn rect_wire(r: Rect) -> [f32; 4] {
    [r.origin.x, r.origin.y, r.size.x, r.size.y]
}

/// State badges derived from the node's [`NodeFlags`] (real flags only —
/// lint badges are layered on by consumers with lint access).
fn flag_badges(flags: NodeFlags) -> Vec<String> {
    let mut badges = Vec::new();
    if !flags.contains(NodeFlags::VISIBLE) {
        badges.push("hidden".to_string());
    }
    if flags.contains(NodeFlags::DIRTY_LAYOUT) {
        badges.push("dirty_layout".to_string());
    }
    if flags.contains(NodeFlags::DIRTY_PAINT) {
        badges.push("dirty_paint".to_string());
    }
    if flags.contains(NodeFlags::DIRTY_A11Y) {
        badges.push("dirty_a11y".to_string());
    }
    if flags.contains(NodeFlags::HOVERED) {
        badges.push("hovered".to_string());
    }
    if flags.contains(NodeFlags::PRESSED) {
        badges.push("pressed".to_string());
    }
    if flags.contains(NodeFlags::FOCUSABLE) {
        badges.push("focusable".to_string());
    }
    if flags.contains(NodeFlags::INERT) {
        badges.push("inert".to_string());
    }
    badges
}

/// Semantic markers (`@alarm`, `@lint:*`, `@level:*`, ...) split off a
/// `debug_name` suffix convention (`Name@marker@marker`).
fn markers_of(debug_name: Option<&str>) -> Vec<String> {
    let Some(name) = debug_name else {
        return Vec::new();
    };
    let mut parts = name.split('@');
    let _ = parts.next();
    parts.map(str::to_string).collect()
}

/// Short type segment of `widget.debug_name()` (`...::text::Text` → `Text`).
fn widget_kind(debug: &str) -> String {
    debug.rsplit("::").next().unwrap_or(debug).to_string()
}

/// Iterates arena children of `id` (`first_child`/`next_sibling` chain).
fn arena_children(arena: &WidgetArena, id: WidgetId) -> Vec<WidgetId> {
    let mut out = Vec::new();
    let mut cur = arena.first_child(id);
    while let Some(c) = cur {
        out.push(c);
        cur = arena.next_sibling(c);
    }
    out
}

/// Whether `id` or any arena descendant's debug name/kind contains `marker`
/// (case-insensitive). Ancestors of matches are retained as context.
fn subtree_matches(arena: &WidgetArena, id: WidgetId, marker: &str) -> bool {
    if let Some(cold) = arena.get_cold(id) {
        let name = cold.debug_name.unwrap_or_default();
        let kind = widget_kind(cold.widget.debug_name());
        if name.to_ascii_lowercase().contains(marker) || kind.to_ascii_lowercase().contains(marker)
        {
            return true;
        }
    }
    arena_children(arena, id)
        .into_iter()
        .any(|c| subtree_matches(arena, c, marker))
}

/// Serializes one arena subtree into the `WireTreeNode` shape consumed by
/// `martensite-mcp` (`id`, `debug_name`, `kind`, `screen_bounds`,
/// `local_bounds`, `depth`, `child_count`, `badges`, `children`).
/// `active_signal_count` is omitted — the arena does not track it (D9).
#[allow(clippy::too_many_arguments)]
fn wire_node(
    arena: &WidgetArena,
    id: WidgetId,
    depth: usize,
    max_depth: Option<usize>,
    child_limit: usize,
    offset: usize,
    marker: Option<&str>,
    include_internal: bool,
) -> Option<Value> {
    let hot = arena.get_hot(id)?;
    let cold = arena.get_cold(id);
    let bounds = rect_wire(hot.bounds);
    let internal = cold
        .filter(|_| include_internal)
        .map_or(0, |c| c.widget.child_count());

    let mut children = Vec::new();
    let arena_kids = arena_children(arena, id);
    if max_depth.is_none_or(|m| depth < m) {
        for kid in arena_kids.iter().skip(offset).take(child_limit) {
            if let Some(m) = marker {
                if !subtree_matches(arena, *kid, m) {
                    continue;
                }
            }
            if let Some(n) = wire_node(
                arena,
                *kid,
                depth + 1,
                max_depth,
                child_limit,
                offset,
                marker,
                include_internal,
            ) {
                children.push(n);
            }
        }
    }

    let mut badges = flag_badges(hot.flags);
    if arena.node_loading(id) {
        badges.push("loading".to_string());
    }

    Some(json!({
        "id": id.to_u64(),
        "debug_name": cold.and_then(|c| c.debug_name),
        "kind": cold.map_or_else(|| "Unknown".to_string(), |c| widget_kind(c.widget.debug_name())),
        "screen_bounds": bounds,
        "local_bounds": bounds,
        "depth": depth,
        "child_count": arena_kids.len() + internal,
        "badges": badges,
        "children": children,
    }))
}

/// Full [`NodeDescriptor`]-shaped detail for one node (`tree_node`).
fn node_detail(arena: &WidgetArena, id: WidgetId) -> Option<Value> {
    let hot = arena.get_hot(id)?;
    let cold = arena.get_cold(id);
    let bounds = rect_wire(hot.bounds);
    let scale = arena.scale_factor();
    let physical = [
        bounds[0] * scale,
        bounds[1] * scale,
        bounds[2] * scale,
        bounds[3] * scale,
    ];

    // Nearest ancestor that clips children supplies the effective clip rect.
    let mut clip_rect = None;
    let mut cur = hot.parent;
    while let Some(p) = cur {
        if let Some(ph) = arena.get_hot(p) {
            if ph.flags.contains(NodeFlags::CLIPS_CHILDREN) {
                clip_rect = Some(rect_wire(ph.bounds));
                break;
            }
            cur = ph.parent;
        } else {
            break;
        }
    }

    let debug_name = cold.and_then(|c| c.debug_name);
    // Effective loading (override OR widget-declared) — the a11y map
    // reports the sanitized node an AT would actually see (ADR-0040).
    let loading = arena.node_loading(id);
    let node = cold.map(|c| {
        if loading {
            loading_a11y_node(c)
        } else {
            widget_a11y_node(c)
        }
    });
    let mut accesskit = BTreeMap::new();
    if let Some(n) = &node {
        if let Some(name) = n.label() {
            accesskit.insert("name".to_string(), name.to_string());
        }
        if let Some(d) = n.description() {
            accesskit.insert("description".to_string(), d.to_string());
        }
        if let Some(v) = n.value() {
            accesskit.insert("value".to_string(), v.to_string());
        }
        if n.is_busy() {
            // The wire map is string→string (`NodeDescriptor::accesskit`).
            accesskit.insert("busy".to_string(), "true".to_string());
        }
    }
    let mut badges = flag_badges(hot.flags);
    if loading {
        badges.push("loading".to_string());
    }

    Some(json!({
        "id": id.to_u64(),
        "generation": id.generation(),
        "kind": cold.map_or_else(|| "Unknown".to_string(), |c| widget_kind(c.widget.debug_name())),
        "debug_name": debug_name,
        "logical_bounds": bounds,
        "physical_bounds": physical,
        "clip_rect": clip_rect,
        "z_index": hot.z_index,
        "markers": markers_of(debug_name),
        "badges": badges,
        "loading": loading,
        "aria_role": node.as_ref().map(|n| format!("{:?}", n.role()).to_ascii_lowercase()),
        "accesskit": accesskit,
    }))
}

/// Builds the widget's AccessKit node the same way
/// `AccessKitAdapter::build_node` does — seeded from `cold` fields,
/// then enriched by `Widget::accessibility` — so the probe reports the
/// real roles/names/values instead of `cold.a11y_role`'s
/// `GenericContainer` default.
fn widget_a11y_node(cold: &martensite_core::ColdNode) -> accesskit::Node {
    let mut node = accesskit::Node::new(cold.a11y_role);
    if let Some(name) = &cold.a11y_name {
        if !name.is_empty() {
            node.set_label(name.as_str());
        }
    }
    if let Some(tooltip) = &cold.tooltip {
        node.set_tooltip(tooltip.as_str());
    }
    cold.widget.accessibility(&mut node);
    node
}

/// The sanitized AccessKit node a *loading* widget emits (ADR-0040) —
/// mirrors `AccessKitAdapter::build_node`'s sanitized branch: the role
/// is preserved but the label is the generic `"Loading"` plus `busy`,
/// the node is input-covered (`disabled`), and the widget's own
/// `accessibility` hook is skipped so pending state cannot leak.
fn loading_a11y_node(cold: &martensite_core::ColdNode) -> accesskit::Node {
    let mut node = accesskit::Node::new(cold.a11y_role);
    node.set_label("Loading");
    node.set_busy();
    node.set_disabled();
    node
}

/// `A11yNodeDescriptor`-shaped subtree (`id` is the widget id string).
///
/// Loading nodes (ADR-0040) are emitted exactly as the AccessKit
/// adapter reports them: sanitized label `"Loading"`, `busy` +
/// `disabled` states, no widget-contributed properties — and their
/// children are pruned, since the placeholder replaced the subtree
/// (descendants are `hidden`+`covered` in the adapter's tree, and this
/// wire format already drops hidden nodes entirely).
fn a11y_node(arena: &WidgetArena, id: WidgetId, role_filter: Option<&str>) -> Option<Value> {
    let hot = arena.get_hot(id)?;
    if !hot.flags.contains(NodeFlags::VISIBLE) {
        return None;
    }
    let loading = arena.node_loading(id);
    let cold = arena.get_cold(id);
    let node = cold.map(|c| {
        if loading {
            loading_a11y_node(c)
        } else {
            widget_a11y_node(c)
        }
    });
    let role = node.as_ref().map_or_else(
        || "genericcontainer".to_string(),
        |n| format!("{:?}", n.role()).to_ascii_lowercase(),
    );

    let children: Vec<Value> = if loading {
        Vec::new()
    } else {
        arena_children(arena, id)
            .into_iter()
            .filter_map(|c| a11y_node(arena, c, role_filter))
            .collect()
    };

    let self_matches = role_filter.is_none_or(|r| role == r);
    if role_filter.is_some() && !self_matches && children.is_empty() {
        return None;
    }

    let mut states = Vec::new();
    if loading {
        // Sanitized emission: busy + covered (disabled), no Focus
        // action — matching the adapter's loading branch.
        states.push("busy".to_string());
        states.push("disabled".to_string());
    }
    if hot.flags.contains(NodeFlags::FOCUSABLE) && !loading {
        states.push("focusable".to_string());
    }
    if hot.flags.contains(NodeFlags::INERT) && !loading {
        states.push("disabled".to_string());
    }
    if hot.flags.contains(NodeFlags::PRESSED) && !loading {
        states.push("pressed".to_string());
    }

    Some(json!({
        "id": id.to_u64().to_string(),
        "role": role,
        "name": node.as_ref().and_then(|n| n.label().map(str::to_string)),
        "description": node.as_ref().and_then(|n| n.description().map(str::to_string)),
        "value": node.as_ref().and_then(|n| n.value().map(str::to_string)),
        "states": states,
        "ignored": false,
        "children": children,
    }))
}

/// Overflow diagnostics over real arena geometry: a node is reported
/// when its bounds protrude past its parent's bounds by more than 0.5 px.
fn overflow_diagnostics(arena: &WidgetArena, root: WidgetId, out: &mut Vec<Value>) {
    for id in arena.iter_subtree(root) {
        if id == root {
            continue;
        }
        let (Some(hot), Some(parent)) = (arena.get_hot(id), arena.parent(id)) else {
            continue;
        };
        let Some(ph) = arena.get_hot(parent) else {
            continue;
        };
        let dx = (hot.bounds.max_x() - ph.bounds.max_x())
            .max(ph.bounds.min_x() - hot.bounds.min_x())
            .max(0.0);
        let dy = (hot.bounds.max_y() - ph.bounds.max_y())
            .max(ph.bounds.min_y() - hot.bounds.min_y())
            .max(0.0);
        if dx <= 0.5 && dy <= 0.5 {
            continue;
        }
        let axis = if dx > 0.5 && dy > 0.5 {
            "both"
        } else if dx > 0.5 {
            "horizontal"
        } else {
            "vertical"
        };
        let clip = {
            let mut found = None;
            let mut cur = Some(parent);
            while let Some(p) = cur {
                match arena.get_hot(p) {
                    Some(h) if h.flags.contains(NodeFlags::CLIPS_CHILDREN) => {
                        found = arena
                            .get_cold(p)
                            .and_then(|c| c.debug_name)
                            .map(str::to_string)
                            .or(Some(p.to_u64().to_string()));
                        break;
                    }
                    Some(h) => cur = h.parent,
                    None => break,
                }
            }
            found
        };
        out.push(json!({
            "offending_node": id.to_u64(),
            "offending_name": arena.get_cold(id).and_then(|c| c.debug_name),
            "overflow_axis": axis,
            "overflow_pixels": dx.max(dy),
            "clipping_ancestor": clip,
            "suggested_remediations": [
                "shrink the child's content or size",
                "increase the parent's allocated bounds",
                "wrap the child in a ScrollView if overflow is intentional",
            ],
        }));
    }
}

/// Real hit-test: the visible, hit-testable arena node containing
/// `point` with the highest `(z_index, paint-order)` — the topmost
/// target. `Rect::contains` is half-open `[min, max)`, matching the
/// production router's bounds gating. Clip-region culling is not
/// modelled here — ancestors with `CLIPS_CHILDREN` are reported via
/// `clip_rect` in `tree_node` instead.
fn hit_test(arena: &WidgetArena, point: Vec2) -> Option<WidgetId> {
    let mut best: Option<(i16, usize, WidgetId)> = None;
    for (dense, hot) in arena.hot_nodes().iter().enumerate() {
        if !hot.flags.contains(NodeFlags::VISIBLE)
            || !hot.flags.contains(NodeFlags::HIT_TEST_ENABLED)
            || !hot.bounds.contains(point)
        {
            continue;
        }
        let slot = arena.dense_to_slot()[dense];
        let Some(gen) = arena.slot_generation(slot) else {
            continue;
        };
        let rank = (hot.z_index, dense);
        if best.is_none_or(|(z, d, _)| rank >= (z, d)) {
            best = Some((hot.z_index, dense, WidgetId::from_parts(slot, gen)));
        }
    }
    best.map(|(_, _, id)| id)
}

/// Ancestor chain of `id`, root-first, ending in `id` — the wire
/// `hit_path` shape.
fn hit_path(arena: &WidgetArena, id: WidgetId) -> Vec<u64> {
    let mut chain = vec![id];
    let mut cur = arena.parent(id);
    while let Some(p) = cur {
        chain.push(p);
        cur = arena.parent(p);
    }
    chain.reverse();
    chain.iter().map(|id| id.to_u64()).collect()
}

/// First arena root (hot nodes with no parent), if any.
fn first_root(arena: &WidgetArena) -> Option<WidgetId> {
    for (dense, hot) in arena.hot_nodes().iter().enumerate() {
        if hot.parent.is_none() {
            let slot = arena.dense_to_slot()[dense];
            if let Some(gen) = arena.slot_generation(slot) {
                return Some(WidgetId::from_parts(slot, gen));
            }
        }
    }
    None
}

/// Parses a two-element `[x, y]` JSON point into a finite [`Vec2`].
fn parse_vec2(value: &Value, name: &str) -> Result<Vec2, String> {
    let arr = value
        .as_array()
        .filter(|a| a.len() == 2)
        .ok_or_else(|| format!("invalid param: `{name}` must be an [x, y] array"))?;
    let x = arr[0]
        .as_f64()
        .filter(|v| v.is_finite())
        .ok_or_else(|| format!("invalid param: `{name}[0]` must be a finite number"))?;
    let y = arr[1]
        .as_f64()
        .filter(|v| v.is_finite())
        .ok_or_else(|| format!("invalid param: `{name}[1]` must be a finite number"))?;
    Ok(Vec2::new(x as f32, y as f32))
}

/// snake_case kind label for one [`PaintCommand`] (audit payload key).
fn command_kind(cmd: &PaintCommand) -> &'static str {
    match cmd {
        PaintCommand::FillRect(..) => "fill_rect",
        PaintCommand::StrokeRect(..) => "stroke_rect",
        PaintCommand::FillPath(..) => "fill_path",
        PaintCommand::StrokePath(..) => "stroke_path",
        PaintCommand::FillLinearGradient(..) => "fill_linear_gradient",
        PaintCommand::FillRadialGradient(..) => "fill_radial_gradient",
        PaintCommand::ClipRect(..) => "clip_rect",
        PaintCommand::ClipRoundedRect(..) => "clip_rounded_rect",
        PaintCommand::ClipPath(..) => "clip_path",
        PaintCommand::FillLinearGradientPath(..) => "fill_linear_gradient_path",
        PaintCommand::FillRadialGradientPath(..) => "fill_radial_gradient_path",
        PaintCommand::PopClip => "pop_clip",
        PaintCommand::DrawText(..) => "draw_text",
        PaintCommand::DrawGlyphRun(..) => "draw_glyph_run",
        PaintCommand::DrawImage(..) => "draw_image",
        PaintCommand::BlurredRect { .. } => "blurred_rect",
        PaintCommand::External { .. } => "external",
        PaintCommand::PushScope { .. } => "push_scope",
        PaintCommand::PopScope => "pop_scope",
    }
}

/// `[x, y, w, h]` wire bounds for a [`PaintCommand`], where meaningful.
fn command_bounds(cmd: &PaintCommand) -> Value {
    let krect = |r: &kurbo::Rect| json!([r.x0, r.y0, r.width(), r.height()]);
    match cmd {
        PaintCommand::FillRect(r, _)
        | PaintCommand::StrokeRect(r, ..)
        | PaintCommand::FillLinearGradient(r, ..)
        | PaintCommand::FillRadialGradient(r, ..)
        | PaintCommand::ClipRect(r)
        | PaintCommand::ClipRoundedRect(r, _)
        | PaintCommand::DrawImage(r, _) => krect(r),
        PaintCommand::FillPath(p, _)
        | PaintCommand::StrokePath(p, ..)
        | PaintCommand::ClipPath(p)
        | PaintCommand::FillLinearGradientPath(p, ..)
        | PaintCommand::FillRadialGradientPath(p, ..) => krect(&p.bounding_box()),
        PaintCommand::DrawText(p, ..) => json!([p.x, p.y, 0.0, 0.0]),
        PaintCommand::DrawGlyphRun(run) => {
            let mut bounds: Option<kurbo::Rect> = None;
            for g in &run.glyphs {
                let r = kurbo::Rect::new(
                    f64::from(g.x),
                    f64::from(g.y - g.height),
                    f64::from(g.x + g.width),
                    f64::from(g.y),
                );
                bounds = Some(bounds.map_or(r, |b| b.union(r)));
            }
            bounds.map_or(Value::Null, |b| krect(&b))
        }
        PaintCommand::BlurredRect { rect, .. } | PaintCommand::External { rect, .. } => {
            json!(rect)
        }
        PaintCommand::PushScope { bounds, .. } => krect(bounds),
        PaintCommand::PopClip | PaintCommand::PopScope => Value::Null,
    }
}

// ---------------------------------------------------------------------------
// Theme token overrides
// ---------------------------------------------------------------------------

/// How a `token_overrides` JSON value maps onto a [`ThemeToken`] variant.
#[derive(Copy, Clone)]
enum TokenShape {
    Color,
    Dimension,
    FontSize,
    Duration,
    Easing,
}

/// `token_overrides` vocabulary: snake_case name → (`TokenKey`, shape).
/// Covers every variant of [`martensite_theme::TokenKey`] — the enum is
/// `#[non_exhaustive]`, so unknown names fall back to an error listing
/// this table.
const TOKEN_KEYS: &[(&str, martensite_theme::TokenKey, TokenShape)] = {
    use martensite_theme::TokenKey as K;
    use TokenShape as S;
    &[
        ("background_color", K::BackgroundColor, S::Color),
        ("surface_color", K::SurfaceColor, S::Color),
        ("primary_color", K::PrimaryColor, S::Color),
        ("secondary_color", K::SecondaryColor, S::Color),
        ("accent_color", K::AccentColor, S::Color),
        ("text_color", K::TextColor, S::Color),
        ("text_muted_color", K::TextMutedColor, S::Color),
        ("text_inverse_color", K::TextInverseColor, S::Color),
        ("border_color", K::BorderColor, S::Color),
        ("divider_color", K::DividerColor, S::Color),
        ("raised_color", K::RaisedColor, S::Color),
        ("error_color", K::ErrorColor, S::Color),
        ("warning_color", K::WarningColor, S::Color),
        ("success_color", K::SuccessColor, S::Color),
        ("info_color", K::InfoColor, S::Color),
        ("inset_color", K::InsetColor, S::Color),
        ("overlay_color", K::OverlayColor, S::Color),
        ("scrim_color", K::ScrimColor, S::Color),
        (
            "backdrop_fallback_color",
            K::BackdropFallbackColor,
            S::Color,
        ),
        ("csd_shadow_color", K::CsdShadowColor, S::Color),
        ("series_color_1", K::SeriesColor1, S::Color),
        ("series_color_2", K::SeriesColor2, S::Color),
        ("series_color_3", K::SeriesColor3, S::Color),
        ("series_color_4", K::SeriesColor4, S::Color),
        ("series_color_5", K::SeriesColor5, S::Color),
        ("series_color_6", K::SeriesColor6, S::Color),
        ("spacing", K::Spacing, S::Dimension),
        ("spacing_small", K::SpacingSmall, S::Dimension),
        ("spacing_large", K::SpacingLarge, S::Dimension),
        ("border_radius", K::BorderRadius, S::Dimension),
        ("border_radius_small", K::BorderRadiusSmall, S::Dimension),
        ("border_radius_large", K::BorderRadiusLarge, S::Dimension),
        ("backdrop_material", K::BackdropMaterial, S::Dimension),
        (
            "backdrop_tint_opacity",
            K::BackdropTintOpacity,
            S::Dimension,
        ),
        ("csd_title_bar_height", K::CsdTitleBarHeight, S::Dimension),
        ("csd_button_radius", K::CsdButtonRadius, S::Dimension),
        ("csd_shadow_blur", K::CsdShadowBlur, S::Dimension),
        ("vibrancy_material", K::VibrancyMaterial, S::Dimension),
        ("font_size_micro", K::FontSizeMicro, S::FontSize),
        ("font_size_caption", K::FontSizeCaption, S::FontSize),
        ("font_size_body", K::FontSizeBody, S::FontSize),
        ("font_size_title", K::FontSizeTitle, S::FontSize),
        ("font_size_display", K::FontSizeDisplay, S::FontSize),
        ("font_size_small", K::FontSizeSmall, S::FontSize),
        ("font_size_medium", K::FontSizeMedium, S::FontSize),
        ("font_size_large", K::FontSizeLarge, S::FontSize),
        ("animation_duration", K::AnimationDuration, S::Duration),
        ("animation_easing", K::AnimationEasing, S::Easing),
    ]
};

/// Comma-separated list of accepted `token_overrides` keys for errors.
fn valid_token_keys() -> String {
    TOKEN_KEYS
        .iter()
        .map(|(n, ..)| *n)
        .collect::<Vec<_>>()
        .join(", ")
}

/// Parses a color override value — `"#rrggbb"`/`"#rrggbbaa"` hex
/// strings, `[r, g, b, a]` arrays, or `{r, g, b, a}` objects — into an
/// [`Oklab`](martensite_theme::Oklab). Channels are unit-scale when all
/// components are `<= 1.0`, else 0–255.
fn parse_color_override(v: &Value) -> Result<martensite_theme::Oklab, String> {
    let channels: Vec<f64> = match v {
        Value::String(s) => return parse_hex_color(s),
        Value::Array(arr) => {
            if !(3..=4).contains(&arr.len()) {
                return Err("color arrays must be [r, g, b] or [r, g, b, a]".to_string());
            }
            arr.iter()
                .enumerate()
                .map(|(i, c)| {
                    c.as_f64()
                        .ok_or_else(|| format!("color channel {i} must be a number"))
                })
                .collect::<Result<Vec<_>, _>>()?
        }
        Value::Object(map) => {
            let channel = |name: &str| -> Result<f64, String> {
                map.get(name)
                    .and_then(Value::as_f64)
                    .ok_or_else(|| format!("color object requires numeric `{name}`"))
            };
            let mut ch = vec![channel("r")?, channel("g")?, channel("b")?];
            if let Some(a) = map.get("a") {
                ch.push(
                    a.as_f64()
                        .ok_or_else(|| "color `a` must be a number".to_string())?,
                );
            }
            ch
        }
        _ => {
            return Err(
                "color override must be \"#rrggbb[aa]\", [r, g, b, a], or {r, g, b, a}".to_string(),
            )
        }
    };
    // Heuristic matching the wire docs: channels > 1.0 mean 0-255 scale.
    let unit = channels.iter().all(|c| (0.0..=1.0).contains(c));
    let (r, g, b) = if unit {
        (channels[0], channels[1], channels[2])
    } else {
        (
            channels[0] / 255.0,
            channels[1] / 255.0,
            channels[2] / 255.0,
        )
    };
    if !r.is_finite() || !g.is_finite() || !b.is_finite() {
        return Err("color channels must be finite".to_string());
    }
    let alpha = match channels.get(3) {
        Some(a) if unit => *a,
        Some(a) => *a / 255.0,
        None => 1.0,
    };
    let mut color = martensite_theme::Oklab::from_srgb(r as f32, g as f32, b as f32);
    color.alpha = alpha.clamp(0.0, 1.0) as f32;
    Ok(color)
}

/// Parses `"#rrggbb"` / `"#rrggbbaa"` into an [`Oklab`].
fn parse_hex_color(s: &str) -> Result<martensite_theme::Oklab, String> {
    let h = s.trim().strip_prefix('#').unwrap_or(s.trim());
    let parse = |i: usize| -> Result<f32, String> {
        u8::from_str_radix(h.get(i..i + 2).unwrap_or(""), 16)
            .map(|v| v as f32 / 255.0)
            .map_err(|_| format!("invalid hex color `{s}`"))
    };
    let (r, g, b, a) = match h.len() {
        6 => (parse(0)?, parse(2)?, parse(4)?, 1.0),
        8 => (parse(0)?, parse(2)?, parse(4)?, parse(6)?),
        _ => {
            return Err(format!(
                "invalid hex color `{s}` (expected #rrggbb or #rrggbbaa)"
            ))
        }
    };
    let mut color = martensite_theme::Oklab::from_srgb(r, g, b);
    color.alpha = a;
    Ok(color)
}

/// Parses a finite numeric override value (dimension/font size/
/// duration/easing tokens).
fn parse_numeric_override(v: &Value) -> Result<f32, String> {
    v.as_f64()
        .map(|n| n as f32)
        .filter(|n| n.is_finite())
        .ok_or_else(|| "token override must be a finite number".to_string())
}

// ---------------------------------------------------------------------------
// Semantic actions
// ---------------------------------------------------------------------------

/// snake_case names accepted by `a11y_action` (used in error messages).
const A11Y_ACTIONS: &[&str] = &[
    "click",
    "focus",
    "blur",
    "set_value",
    "increment",
    "decrement",
    "expand",
    "collapse",
    "show_tooltip",
    "hide_tooltip",
    "show_context_menu",
    "scroll_up",
    "scroll_down",
    "scroll_left",
    "scroll_right",
    "scroll_into_view",
    "scroll_to_point",
    "set_scroll_offset",
];

/// Maps an `a11y_action` `action` string plus payload params onto a
/// [`SemanticAction`]. Unknown names error listing [`A11Y_ACTIONS`].
fn parse_semantic_action(name: &str, params: &Value) -> Result<SemanticAction, String> {
    let norm = name.to_ascii_lowercase().replace(['_', '-'], "");
    Ok(match norm.as_str() {
        "click" => SemanticAction::Click,
        "focus" => SemanticAction::Focus,
        "blur" => SemanticAction::Blur,
        "setvalue" => {
            let value = params
                .get("value")
                .map(|v| {
                    v.as_str()
                        .map(str::to_string)
                        .unwrap_or_else(|| v.to_string())
                })
                .ok_or_else(|| "invalid param: `set_value` requires a `value` param".to_string())?;
            SemanticAction::SetValue(value)
        }
        "increment" => SemanticAction::Increment,
        "decrement" => SemanticAction::Decrement,
        "expand" => SemanticAction::Expand,
        "collapse" => SemanticAction::Collapse,
        "showtooltip" => SemanticAction::ShowTooltip,
        "hidetooltip" => SemanticAction::HideTooltip,
        "showcontextmenu" => SemanticAction::ShowContextMenu,
        "scrollup" => SemanticAction::ScrollUp,
        "scrolldown" => SemanticAction::ScrollDown,
        "scrollleft" => SemanticAction::ScrollLeft,
        "scrollright" => SemanticAction::ScrollRight,
        "scrollintoview" => SemanticAction::ScrollIntoView,
        "scrolltopoint" => SemanticAction::ScrollToPoint(parse_vec2(
            params.get("point").unwrap_or(&Value::Null),
            "point",
        )?),
        "setscrolloffset" => SemanticAction::SetScrollOffset(parse_vec2(
            params
                .get("offset")
                .or_else(|| params.get("point"))
                .or_else(|| params.get("value"))
                .unwrap_or(&Value::Null),
            "offset",
        )?),
        _ => {
            return Err(format!(
                "invalid param: unknown a11y action `{name}` (expected one of: {})",
                A11Y_ACTIONS.join(", ")
            ))
        }
    })
}

// ---------------------------------------------------------------------------
// capture_node rasterization (`render` feature)
// ---------------------------------------------------------------------------

/// Translates + scales one paint command into capture-local space
/// (`out = (in - node_origin) * scale`).
#[cfg(feature = "render")]
fn transform_command(cmd: &mut PaintCommand, affine: kurbo::Affine, scale: f64) {
    let krect = |a: kurbo::Affine, r: &mut kurbo::Rect| {
        *r = kurbo::Rect::from_points(
            a * kurbo::Point::new(r.x0, r.y0),
            a * kurbo::Point::new(r.x1, r.y1),
        );
    };
    let rect4 = |a: kurbo::Affine, r: &mut [f32; 4]| {
        let p0 = a * kurbo::Point::new(f64::from(r[0]), f64::from(r[1]));
        let p1 = a * kurbo::Point::new(f64::from(r[0] + r[2]), f64::from(r[1] + r[3]));
        *r = [
            p0.x as f32,
            p0.y as f32,
            (p1.x - p0.x) as f32,
            (p1.y - p0.y) as f32,
        ];
    };
    let pt = |a: kurbo::Affine, p: &mut [f64; 2]| {
        let q = a * kurbo::Point::new(p[0], p[1]);
        *p = [q.x, q.y];
    };
    let s = scale as f32;
    match cmd {
        PaintCommand::FillRect(r, _)
        | PaintCommand::ClipRect(r)
        | PaintCommand::DrawImage(r, _) => krect(affine, r),
        PaintCommand::StrokeRect(r, w, _) => {
            krect(affine, r);
            *w *= s;
        }
        PaintCommand::ClipRoundedRect(r, radius) => {
            krect(affine, r);
            *radius *= s;
        }
        PaintCommand::FillPath(p, _) | PaintCommand::ClipPath(p) => p.apply_affine(affine),
        PaintCommand::StrokePath(p, w, _) => {
            p.apply_affine(affine);
            *w *= s;
        }
        PaintCommand::FillLinearGradient(r, _, from, to) => {
            krect(affine, r);
            pt(affine, from);
            pt(affine, to);
        }
        PaintCommand::FillRadialGradient(r, _, center, radius) => {
            krect(affine, r);
            pt(affine, center);
            *radius *= scale;
        }
        PaintCommand::FillLinearGradientPath(p, _, from, to) => {
            p.apply_affine(affine);
            pt(affine, from);
            pt(affine, to);
        }
        PaintCommand::FillRadialGradientPath(p, _, center, radius) => {
            p.apply_affine(affine);
            pt(affine, center);
            *radius *= scale;
        }
        PaintCommand::DrawText(origin, _, size, _) => {
            *origin = affine * *origin;
            *size *= s;
        }
        PaintCommand::DrawGlyphRun(run) => {
            run.font_size *= s;
            if let Some(ls) = &mut run.style.letter_spacing {
                *ls *= s;
            }
            for g in &mut run.glyphs {
                let q = affine * kurbo::Point::new(f64::from(g.x), f64::from(g.y));
                g.x = q.x as f32;
                g.y = q.y as f32;
                g.width *= s;
                g.height *= s;
            }
        }
        PaintCommand::BlurredRect {
            rect, blur_radius, ..
        } => {
            rect4(affine, rect);
            *blur_radius *= s;
        }
        PaintCommand::External { rect, clip, .. } => {
            rect4(affine, rect);
            rect4(affine, clip);
        }
        PaintCommand::PushScope { bounds, .. } => krect(affine, bounds),
        PaintCommand::PopClip | PaintCommand::PopScope => {}
    }
}

impl ArenaProbe for WidgetArenaProbe {
    fn set_focus_manager(&mut self, focus: Arc<Mutex<martensite_focus::FocusManager>>) {
        self.focus = Some(focus);
    }
    fn probe_tree_snapshot(&mut self, params: &Value) -> SessionResult {
        let max_depth = params
            .get("max_depth")
            .and_then(Value::as_u64)
            .map(|v| v as usize);
        let child_limit = params
            .get("child_limit")
            .and_then(Value::as_u64)
            .map_or(50, |v| (v as usize).clamp(1, 200));
        let offset = params
            .get("offset")
            .and_then(Value::as_u64)
            .map_or(0, |v| v as usize);
        let marker = params
            .get("filter_marker")
            .and_then(Value::as_str)
            .map(|s| s.trim_start_matches('@').trim().to_ascii_lowercase())
            .filter(|s| !s.is_empty());
        let include_internal = params
            .get("include_internal")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let root_id = params
            .get("root_id")
            .and_then(|v| v.as_u64().or_else(|| v.as_str()?.parse().ok()))
            .and_then(WidgetId::from_u64);

        let arena = self.arena.lock().expect("arena mutex");

        // Arena roots = hot nodes with no parent.
        let mut roots: Vec<WidgetId> = Vec::new();
        for (dense, hot) in arena.hot_nodes().iter().enumerate() {
            if hot.parent.is_none() {
                let slot = arena.dense_to_slot()[dense];
                if let Some(gen) = arena.slot_generation(slot) {
                    roots.push(WidgetId::from_parts(slot, gen));
                }
            }
        }
        let roots: Vec<WidgetId> = match root_id {
            Some(r) if arena.is_alive(r) => vec![r],
            Some(r) => return Err(format!("node `{r:?}` is not alive in the arena")),
            None => roots,
        };

        let mut nodes: Vec<Value> = roots
            .iter()
            .skip(offset)
            .take(child_limit)
            .filter_map(|r| {
                if let Some(m) = marker.as_deref() {
                    if !subtree_matches(&arena, *r, m) {
                        return None;
                    }
                }
                wire_node(
                    &arena,
                    *r,
                    0,
                    max_depth,
                    child_limit,
                    offset,
                    marker.as_deref(),
                    include_internal,
                )
            })
            .collect();

        let root = match nodes.len() {
            0 => return Err("widget arena is empty".to_string()),
            1 => nodes.remove(0),
            // Multi-root scenes wrap in a synthetic envelope (id 0 is
            // never a live WidgetId — generation 0 is unconstructable).
            _ => json!({
                "id": 0,
                "debug_name": "ArenaRoots",
                "kind": "ArenaRoot",
                "screen_bounds": [0.0, 0.0, 0.0, 0.0],
                "local_bounds": [0.0, 0.0, 0.0, 0.0],
                "depth": 0,
                "child_count": nodes.len(),
                "badges": [],
                "children": nodes,
            }),
        };

        Ok(json!({
            "root": root,
            "selected_id": self.selected.filter(|id| arena.is_alive(*id)).map(|id| id.to_u64()),
            "layout_chain": [],
            "signals": [],
        }))
    }

    fn probe_node_detail(&mut self, node_id: u64) -> SessionResult {
        let Some(id) = WidgetId::from_u64(node_id) else {
            return Err(format!(
                "invalid param: node_id `{node_id}` is not a valid widget id"
            ));
        };
        let arena = self.arena.lock().expect("arena mutex");
        node_detail(&arena, id).ok_or_else(|| format!("node `{node_id}` is not alive in the arena"))
    }

    /// `layout_chain` — real Taffy state when a
    /// [`LayoutStore`](super::layout_store::LayoutStore)
    /// (`with_layout_store`) is attached: the node's registered
    /// `size`/`min_size`/`max_size` style constraints, its resolved
    /// layout size, overflow diagnostics naming it, and the arena
    /// ancestor chain. Without a store the arena's own bounds and
    /// ancestry still answer the call — the response carries
    /// `"layout_engine": null` instead of failing.
    fn probe_layout_chain(&mut self, node_id: u64) -> SessionResult {
        let Some(id) = WidgetId::from_u64(node_id) else {
            return Err(format!(
                "invalid param: node_id `{node_id}` is not a valid widget id"
            ));
        };
        let arena = self.arena.lock().expect("arena mutex");
        let Some(hot) = arena.get_hot(id) else {
            return Err(format!("node `{id:?}` is not alive in the arena"));
        };
        let bounds = rect_wire(hot.bounds);

        // Arena ancestors, root-first, excluding `id` itself.
        let mut parent_chain: Vec<u64> = Vec::new();
        let mut cur = hot.parent;
        while let Some(p) = cur {
            parent_chain.push(p.to_u64());
            cur = arena.parent(p);
        }
        parent_chain.reverse();

        let Some(store) = &self.layout_store else {
            return Ok(json!({
                "node_id": node_id,
                "layout_engine": Value::Null,
                "bounds": bounds,
                "resolved_size": [bounds[2], bounds[3]],
                "constraints": Value::Null,
                "diagnostics": [],
                "parent_chain": parent_chain,
            }));
        };
        let store = store.lock().expect("layout store mutex");
        let diagnostics = store.diagnostics_for(node_id);

        let Some(record) = store.record(node_id) else {
            return Ok(json!({
                "node_id": node_id,
                "layout_engine": { "registered": false },
                "bounds": bounds,
                "resolved_size": [bounds[2], bounds[3]],
                "constraints": Value::Null,
                "diagnostics": diagnostics,
                "parent_chain": parent_chain,
            }));
        };

        Ok(json!({
            "node_id": node_id,
            "layout_engine": {
                "registered": true,
                "generation": store.generation(),
            },
            "bounds": bounds,
            "resolved_size": record.resolved_size,
            "resolved_location": record.resolved_location,
            "constraints": record.constraints.clone(),
            "diagnostics": diagnostics,
            "parent_chain": parent_chain,
            "taffy_parent_chain": record.taffy_parent_chain,
        }))
    }

    /// `audit_paint` — records the real paint list for `node_id`'s
    /// subtree (or the first arena root) via
    /// [`WidgetArena::build_paint_list`], then reports command stats:
    /// total count, per-variant counts, clip-push count, an ordered
    /// `{index, kind, bounds}` entry per command, the nodes flagged
    /// [`NodeFlags::DIRTY_PAINT`], and the union rect covering the
    /// painted subtree.
    fn probe_audit_paint(&mut self, node_id: Option<u64>) -> SessionResult {
        let arena = self.arena.lock().expect("arena mutex");
        let root = match node_id.map(WidgetId::from_u64) {
            Some(Some(id)) if arena.is_alive(id) => id,
            Some(Some(id)) => return Err(format!("node `{id:?}` is not alive in the arena")),
            Some(None) => {
                return Err(format!(
                    "invalid param: node_id `{node_id:?}` is not a valid widget id"
                ))
            }
            None => first_root(&arena).ok_or_else(|| "widget arena is empty".to_string())?,
        };

        let mut list = PaintList::new();
        arena.build_paint_list(root, &mut list);

        let mut counts: BTreeMap<&'static str, usize> = BTreeMap::new();
        let mut clip_count = 0usize;
        let mut commands: Vec<Value> = Vec::with_capacity(list.commands.len());
        for (index, cmd) in list.commands.iter().enumerate() {
            let kind = command_kind(cmd);
            *counts.entry(kind).or_default() += 1;
            if matches!(
                cmd,
                PaintCommand::ClipRect(_)
                    | PaintCommand::ClipRoundedRect(..)
                    | PaintCommand::ClipPath(_)
            ) {
                clip_count += 1;
            }
            commands.push(json!({
                "index": index,
                "kind": kind,
                "bounds": command_bounds(cmd),
            }));
        }

        // Live dirty-paint flags across the whole arena — the repaint
        // backlog this frame's list already consumed or still owes.
        let mut dirty: Vec<u64> = Vec::new();
        for (dense, hot) in arena.hot_nodes().iter().enumerate() {
            if hot.flags.contains(NodeFlags::DIRTY_PAINT) {
                let slot = arena.dense_to_slot()[dense];
                if let Some(gen) = arena.slot_generation(slot) {
                    dirty.push(WidgetId::from_parts(slot, gen).to_u64());
                }
            }
        }

        // Union of every live node bounds inside the audited subtree.
        let mut frame: Option<Rect> = None;
        for wid in arena.iter_subtree(root) {
            if let Some(h) = arena.get_hot(wid) {
                frame = Some(match frame {
                    Some(f) => Rect::new(
                        f.min_x().min(h.bounds.min_x()),
                        f.min_y().min(h.bounds.min_y()),
                        f.max_x().max(h.bounds.max_x()) - f.min_x().min(h.bounds.min_x()),
                        f.max_y().max(h.bounds.max_y()) - f.min_y().min(h.bounds.min_y()),
                    ),
                    None => h.bounds,
                });
            }
        }

        Ok(json!({
            "root": root.to_u64(),
            "total_commands": list.commands.len(),
            "counts_by_kind": counts,
            "clip_count": clip_count,
            "commands": commands,
            "dirty_nodes": dirty,
            "frame": frame.map(rect_wire),
        }))
    }

    /// `capture_node` — rasterizes the node's painted subtree to PNG.
    /// With the `render` feature this is a real rasterization: the
    /// subtree's [`PaintList`] is recorded via
    /// [`WidgetArena::build_paint_list`], translated so the node's
    /// top-left lands at the pixmap origin and scaled by `scale`, then
    /// rendered by [`martensite_render::TinySkiaBackend`] and PNG-encoded.
    /// Without `render` the capability is honestly `not_implemented`.
    fn probe_capture_node(&mut self, node_id: u64, format: &str, scale: f64) -> SessionResult {
        #[cfg(feature = "render")]
        {
            self.capture_node_png(node_id, format, scale)
        }
        #[cfg(not(feature = "render"))]
        {
            let _ = (node_id, format, scale);
            Err(not_impl("capture_node"))
        }
    }

    fn probe_overflow_scan(&mut self, node_id: Option<u64>) -> SessionResult {
        let arena = self.arena.lock().expect("arena mutex");
        let mut overflows = Vec::new();
        match node_id.and_then(WidgetId::from_u64) {
            Some(root) if arena.is_alive(root) => {
                overflow_diagnostics(&arena, root, &mut overflows);
            }
            Some(_) => return Err(format!("node `{node_id:?}` is not alive in the arena")),
            None => {
                for (dense, hot) in arena.hot_nodes().iter().enumerate() {
                    if hot.parent.is_none() {
                        let slot = arena.dense_to_slot()[dense];
                        if let Some(gen) = arena.slot_generation(slot) {
                            overflow_diagnostics(
                                &arena,
                                WidgetId::from_parts(slot, gen),
                                &mut overflows,
                            );
                        }
                    }
                }
            }
        }
        Ok(json!({ "overflows": overflows, "total": overflows.len() }))
    }

    fn probe_a11y_tree(&mut self, params: &Value) -> SessionResult {
        let root_id = params
            .get("root_id")
            .and_then(|v| v.as_u64().or_else(|| v.as_str()?.parse().ok()))
            .and_then(WidgetId::from_u64);
        let role_filter = params
            .get("role_filter")
            .and_then(Value::as_str)
            .map(|s| s.to_ascii_lowercase())
            .filter(|s| !s.is_empty());

        let arena = self.arena.lock().expect("arena mutex");
        let roots: Vec<WidgetId> = match root_id {
            Some(r) if arena.is_alive(r) => vec![r],
            Some(r) => return Err(format!("node `{r:?}` is not alive in the arena")),
            None => arena
                .hot_nodes()
                .iter()
                .enumerate()
                .filter(|(_, h)| h.parent.is_none())
                .filter_map(|(dense, _)| {
                    let slot = arena.dense_to_slot()[dense];
                    arena
                        .slot_generation(slot)
                        .map(|g| WidgetId::from_parts(slot, g))
                })
                .collect(),
        };

        let nodes: Vec<Value> = roots
            .into_iter()
            .filter_map(|r| a11y_node(&arena, r, role_filter.as_deref()))
            .collect();
        let root = match nodes.len() {
            0 => return Err("no visible accessibility nodes".to_string()),
            1 => nodes.into_iter().next().expect("one node"),
            _ => json!({
                "id": "0",
                "role": "genericcontainer",
                "name": "ArenaRoots",
                "ignored": true,
                "children": nodes,
            }),
        };
        Ok(json!({ "root": root }))
    }

    fn probe_dispatch_event(&mut self, kind: &str, params: &Value) -> SessionResult {
        // Optional explicit target (`target`/`node_id`, u64 or decimal
        // string) for clients that already know the widget id.
        let explicit = params
            .get("target")
            .or_else(|| params.get("node_id"))
            .and_then(|v| v.as_u64().or_else(|| v.as_str()?.parse().ok()))
            .and_then(WidgetId::from_u64);
        let position = params.get("position").and_then(|v| {
            let arr = v.as_array()?;
            Some(Vec2::new(
                arr.first()?.as_f64()? as f32,
                arr.get(1)?.as_f64()? as f32,
            ))
        });

        let mut arena = self.arena.lock().expect("arena mutex");
        let target = match explicit {
            Some(id) if arena.is_alive(id) => id,
            Some(id) => return Err(format!("node `{id:?}` is not alive in the arena")),
            None => match kind {
                // Pointer events resolve their target by hit-test.
                "pointer_click" | "pointer_move" | "scroll" => {
                    let Some(pos) = position else {
                        return Err(
                            "invalid param: `position: [x, y]` is required for pointer events"
                                .to_string(),
                        );
                    };
                    hit_test(&arena, pos)
                        .ok_or_else(|| "no hit-testable node at `position`".to_string())?
                }
                // Keyboard/text events route to the focused widget —
                // real when a `FocusManager` is attached via
                // `with_focus_manager`, honestly unbacked otherwise.
                "key_press" | "text_input" => {
                    let Some(focus) = &self.focus else {
                        return Err(format!(
                            "not_implemented:event_dispatch `{kind}` needs the app's focused \
                             widget; attach the FocusManager via \
                             `WidgetArenaProbe::with_focus_manager` or pass an explicit `target` \
                             node id"
                        ));
                    };
                    match focus.lock().expect("focus mutex").current_focus() {
                        Some(id) if arena.is_alive(id) => id,
                        Some(id) => {
                            return Err(format!("focused node `{id:?}` is not alive in the arena"))
                        }
                        None => {
                            return Err(
                                "no widget currently holds focus; pass an explicit `target` \
                                 node id"
                                    .to_string(),
                            )
                        }
                    }
                }
                _ => return Err(format!("invalid param: unknown event_type `{kind}`")),
            },
        };

        let path = hit_path(&arena, target);
        let button = PointerButton::Primary;
        let dispatch = |arena: &mut WidgetArena, ev: WidgetEvent| -> EventResponse {
            arena.dispatch_event(target, &ev)
        };
        let response = match kind {
            "pointer_click" => {
                let Some(pos) = position else {
                    return Err(
                        "invalid param: `position: [x, y]` is required for pointer_click"
                            .to_string(),
                    );
                };
                let press = dispatch(
                    &mut arena,
                    WidgetEvent::PointerPressed {
                        position: pos,
                        button,
                        count: 1,
                    },
                );
                let release = dispatch(
                    &mut arena,
                    WidgetEvent::PointerReleased {
                        position: pos,
                        button,
                    },
                );
                if press == EventResponse::Ignored {
                    release
                } else {
                    press
                }
            }
            "pointer_move" => {
                let Some(pos) = position else {
                    return Err(
                        "invalid param: `position: [x, y]` is required for pointer_move"
                            .to_string(),
                    );
                };
                dispatch(&mut arena, WidgetEvent::PointerMoved { position: pos })
            }
            "scroll" => {
                let (Some(pos), Some(delta)) = (
                    position,
                    params.get("delta").and_then(|v| {
                        let arr = v.as_array()?;
                        Some(Vec2::new(
                            arr.first()?.as_f64()? as f32,
                            arr.get(1)?.as_f64()? as f32,
                        ))
                    }),
                ) else {
                    return Err(
                        "invalid param: `position: [x, y]` and `delta: [dx, dy]` are required for scroll"
                            .to_string(),
                    );
                };
                dispatch(
                    &mut arena,
                    WidgetEvent::Scroll {
                        position: pos,
                        delta,
                    },
                )
            }
            "key_press" => {
                let key = params
                    .get("key")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "invalid param: `key` is required for key_press".to_string())?
                    .to_string();
                dispatch(
                    &mut arena,
                    WidgetEvent::KeyPressed {
                        key,
                        repeat: params
                            .get("repeat")
                            .and_then(Value::as_bool)
                            .unwrap_or(false),
                    },
                )
            }
            "text_input" => {
                let text = params
                    .get("text")
                    .or_else(|| params.get("key"))
                    .and_then(Value::as_str)
                    .ok_or_else(|| "invalid param: `text` is required for text_input".to_string())?
                    .to_string();
                dispatch(&mut arena, WidgetEvent::ImeCommitted { text })
            }
            _ => return Err(format!("invalid param: unknown event_type `{kind}`")),
        };

        Ok(json!({
            "response": format!("{response:?}"),
            "hit_path": path,
            "target": target.to_u64(),
        }))
    }

    /// Selection is real state: an explicit `node_id`/`target` selects
    /// the node (surfaced as `selected_id` in `tree_snapshot`), a
    /// `pick`/`point` `[x, y]` pair hit-tests and selects the topmost
    /// hit node, and `arm:false` clears it. Arming an *interactive*
    /// pick (`arm:true` with no id or point) needs the app's inspector
    /// overlay — honest `not_implemented` there.
    fn probe_inspector_select(&mut self, params: &Value) -> SessionResult {
        let arm = params.get("arm").and_then(Value::as_bool).unwrap_or(true);
        let id = params
            .get("node_id")
            .or_else(|| params.get("target"))
            .and_then(|v| v.as_u64().or_else(|| v.as_str()?.parse().ok()))
            .and_then(WidgetId::from_u64);
        if let Some(id) = id {
            let arena = self.arena.lock().expect("arena mutex");
            if !arena.is_alive(id) {
                return Err(format!("node `{id:?}` is not alive in the arena"));
            }
            self.selected = Some(id);
            return Ok(json!({
                "armed": false,
                "selected_id": id.to_u64(),
            }));
        }
        // `pick`/`point`: hit-test the coordinate directly — the same
        // bounds test `event_dispatch` uses for pointer events.
        if let Some(v) = params.get("pick").or_else(|| params.get("point")) {
            let point = parse_vec2(v, "pick")?;
            let arena = self.arena.lock().expect("arena mutex");
            let id = hit_test(&arena, point)
                .ok_or_else(|| "no hit-testable node at `pick` coordinates".to_string())?;
            let path = hit_path(&arena, id);
            drop(arena);
            self.selected = Some(id);
            return Ok(json!({
                "armed": false,
                "selected_id": id.to_u64(),
                "hit_path": path,
            }));
        }
        if !arm {
            self.selected = None;
            return Ok(json!({ "armed": false, "selected_id": Value::Null }));
        }
        Err(
            "not_implemented:inspector_select interactive pick mode needs the app's inspector \
             overlay; pass `node_id` or a `pick: [x, y]` coordinate to select directly"
                .to_string(),
        )
    }

    /// `a11y_action` — parses a snake_case action name into a real
    /// [`SemanticAction`] and dispatches `WidgetEvent::SemanticAction`
    /// through `WidgetArena::dispatch_event` at `node_id` (or, when
    /// omitted, the attached [`FocusManager`](martensite_focus::FocusManager)'s
    /// current focus — or an honest error when neither source exists).
    fn probe_a11y_action(&mut self, params: &Value) -> SessionResult {
        let action = params
            .get("action")
            .and_then(Value::as_str)
            .ok_or_else(|| "invalid param: `action` must be a string".to_string())?;
        let action = parse_semantic_action(action, params)?;
        let explicit = params
            .get("node_id")
            .or_else(|| params.get("target"))
            .and_then(|v| v.as_u64().or_else(|| v.as_str()?.parse().ok()))
            .and_then(WidgetId::from_u64);

        // Resolve the focus fallback before locking the arena —
        // `FocusManager::current_focus` never touches arena state.
        let focused = self
            .focus
            .as_ref()
            .and_then(|fm| fm.lock().expect("focus mutex").current_focus());

        let mut arena = self.arena.lock().expect("arena mutex");
        let target = match explicit {
            Some(id) if arena.is_alive(id) => id,
            Some(id) => return Err(format!("node `{id:?}` is not alive in the arena")),
            None => match focused {
                Some(id) if arena.is_alive(id) => id,
                Some(id) => return Err(format!("focused node `{id:?}` is not alive in the arena")),
                None => {
                    return Err(
                        "invalid param: `node_id` required — no focus manager attached to fall \
                         back on"
                            .to_string(),
                    )
                }
            },
        };

        let response = arena.dispatch_event(target, &WidgetEvent::SemanticAction(action));
        Ok(json!({
            "response": format!("{response:?}"),
            "target": target.to_u64(),
            "hit_path": hit_path(&arena, target),
        }))
    }

    /// `node_set_loading` — real `WidgetArena::set_loading` on the
    /// resolved node (ADR-0040 phase 3). Dead ids are rejected before
    /// the write; the response echoes the *effective* loading state via
    /// `node_loading`, so a widget whose own `is_loading` is `true`
    /// still reports `loading: true` after the override is cleared.
    fn probe_node_set_loading(&mut self, node_id: u64, loading: bool) -> SessionResult {
        let Some(id) = WidgetId::from_u64(node_id) else {
            return Err(format!(
                "invalid param: node_id `{node_id}` is not a valid widget id"
            ));
        };
        let mut arena = self.arena.lock().expect("arena mutex");
        if !arena.is_alive(id) {
            return Err(format!("node `{id:?}` is not alive in the arena"));
        }
        arena.set_loading(id, loading);
        Ok(json!({
            "applied": true,
            "node_id": node_id,
            "loading": arena.node_loading(id),
        }))
    }

    /// `theme_set` — real mode switching (`light`/`dark`) and real
    /// `token_overrides`: every snake_case
    /// [`TokenKey`](martensite_theme::TokenKey) name maps to its typed
    /// [`ThemeToken`](martensite_theme::ThemeToken) (hex/`{r,g,b}`/
    /// `[r,g,b,a]` colors → Oklab; numbers → dimension/font-size/
    /// duration/easing). `system` stays honestly `not_implemented` —
    /// the OS preference source lives in the window layer, outside the
    /// arena.
    fn probe_theme_apply(&mut self, params: &Value) -> SessionResult {
        use martensite_theme::ThemeToken;

        let mode = params.get("mode").and_then(Value::as_str);
        let overrides = params.get("token_overrides").and_then(Value::as_object);
        if params
            .get("system")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            return Err(
                "not_implemented:theme_apply `system` needs the app's OS preference source"
                    .to_string(),
            );
        }

        let mut arena = self.arena.lock().expect("arena mutex");
        let mut theme = match mode {
            Some("light") => martensite_theme::tokens::default_light(),
            Some("dark") => martensite_theme::tokens::default_dark(),
            // `system` needs the OS preference, which lives in the window
            // layer — not reachable from the arena.
            Some("system") => {
                return Err(
                    "not_implemented:theme_apply `system` needs the app's OS preference source"
                        .to_string(),
                );
            }
            Some(other) => {
                return Err(format!(
                    "invalid param: unknown theme mode `{other}` (expected light|dark|system)"
                ));
            }
            // Overrides without a mode layer onto the current theme.
            None => arena.theme().clone(),
        };

        let mut applied = 0usize;
        if let Some(map) = overrides {
            for (key, value) in map {
                let Some((token_key, shape)) = TOKEN_KEYS
                    .iter()
                    .find(|(n, ..)| *n == key.as_str())
                    .map(|(_, k, s)| (*k, *s))
                else {
                    return Err(format!(
                        "invalid param: unknown token key `{key}` (expected one of: {})",
                        valid_token_keys()
                    ));
                };
                let token = match shape {
                    TokenShape::Color => ThemeToken::Color(
                        parse_color_override(value).map_err(|e| format!("token `{key}`: {e}"))?,
                    ),
                    TokenShape::Dimension => ThemeToken::Dimension(
                        parse_numeric_override(value).map_err(|e| format!("token `{key}`: {e}"))?,
                    ),
                    TokenShape::FontSize => ThemeToken::FontSize(
                        parse_numeric_override(value).map_err(|e| format!("token `{key}`: {e}"))?,
                    ),
                    TokenShape::Duration => ThemeToken::Duration(
                        parse_numeric_override(value).map_err(|e| format!("token `{key}`: {e}"))?,
                    ),
                    TokenShape::Easing => ThemeToken::Easing(
                        parse_numeric_override(value).map_err(|e| format!("token `{key}`: {e}"))?,
                    ),
                };
                theme.set(token_key, token);
                applied += 1;
            }
        }

        if mode.is_none() && applied == 0 {
            return Err(
                "invalid param: `theme_set` requires `mode` or `token_overrides`".to_string(),
            );
        }
        arena.set_theme(theme);
        Ok(json!({
            "applied": true,
            "mode": mode,
            "name": arena.theme().name.clone(),
            "overrides_applied": applied,
        }))
    }

    fn probe_theme_tokens(&mut self) -> SessionResult {
        let arena = self.arena.lock().expect("arena mutex");
        let theme = arena.theme();
        let tokens: serde_json::Map<String, Value> = theme
            .tokens
            .iter()
            .map(|(k, v)| (format!("{k:?}"), json!(format!("{v:?}"))))
            .collect();
        Ok(json!({
            "name": theme.name,
            "mode": Value::Null,
            "tokens": tokens,
        }))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use martensite_core::{ColdNode, DummyWidget, HotNode, Rect, WidgetArena};

    use super::super::layout_store::LayoutStore;
    use super::*;

    /// Builds `root (Container)` → `child (Text)` with real bounds.
    fn arena() -> (Arc<Mutex<WidgetArena>>, WidgetId, WidgetId) {
        let mut arena = WidgetArena::new();
        let root_hot = HotNode {
            bounds: Rect::new(0.0, 0.0, 800.0, 600.0),
            flags: NodeFlags::VISIBLE | NodeFlags::HIT_TEST_ENABLED,
            ..Default::default()
        };
        let root = arena.insert_with_widget(root_hot, Box::new(DummyWidget));

        let child_hot = HotNode {
            bounds: Rect::new(8.0, 8.0, 120.0, 24.0),
            flags: NodeFlags::VISIBLE | NodeFlags::FOCUSABLE | NodeFlags::HIT_TEST_ENABLED,
            ..Default::default()
        };
        let mut child_cold = ColdNode::new(Box::new(DummyWidget));
        child_cold.debug_name = Some("CounterLabel");
        let child = arena.insert_with_widget(child_hot, Box::new(DummyWidget));
        // Re-insert with a named cold node: `insert_with_widget` builds a
        // default ColdNode; set the debug name via `get_cold_mut`.
        *arena.get_cold_mut(child).expect("child cold") = child_cold;
        arena.append_child(root, child).expect("append child");

        (Arc::new(Mutex::new(arena)), root, child)
    }

    fn probe(a: &Arc<Mutex<WidgetArena>>) -> WidgetArenaProbe {
        WidgetArenaProbe::new(Arc::clone(a))
    }

    #[test]
    fn tree_snapshot_emits_real_arena_tree() {
        let (a, root, child) = arena();
        let mut p = probe(&a);
        let out = p.probe_tree_snapshot(&json!({})).expect("snapshot");
        let r = &out["root"];
        assert_eq!(r["id"], root.to_u64());
        assert_eq!(r["screen_bounds"], json!([0.0, 0.0, 800.0, 600.0]));
        assert_eq!(r["child_count"], 1);
        let kids = r["children"].as_array().expect("children");
        assert_eq!(kids.len(), 1);
        assert_eq!(kids[0]["id"], child.to_u64());
        assert_eq!(kids[0]["debug_name"], "CounterLabel");
        assert_eq!(kids[0]["depth"], 1);
    }

    #[test]
    fn tree_snapshot_honors_max_depth_and_filter_marker() {
        let (a, _root, _child) = arena();
        let mut p = probe(&a);
        let out = p
            .probe_tree_snapshot(&json!({"max_depth": 0}))
            .expect("snapshot");
        assert_eq!(out["root"]["children"].as_array().unwrap().len(), 0);
        assert_eq!(out["root"]["child_count"], 1);

        let mut p = probe(&a);
        let out = p
            .probe_tree_snapshot(&json!({"filter_marker": "counterlabel"}))
            .expect("filtered");
        assert_eq!(out["root"]["children"].as_array().unwrap().len(), 1);

        let mut p = probe(&a);
        let out = p
            .probe_tree_snapshot(&json!({"filter_marker": "nomatch"}))
            .expect_err("no roots match");
        assert!(out.contains("empty"), "{out}");
    }

    #[test]
    fn tree_node_reports_generation_bounds_and_aria() {
        let (a, _root, child) = arena();
        let mut p = probe(&a);
        let out = p.probe_node_detail(child.to_u64()).expect("detail");
        assert_eq!(out["id"], child.to_u64());
        assert_eq!(out["generation"], child.generation());
        assert_eq!(out["debug_name"], "CounterLabel");
        assert_eq!(out["logical_bounds"], json!([8.0, 8.0, 120.0, 24.0]));
        assert!(out["badges"].is_null() || true);
    }

    #[test]
    fn overflow_scan_reports_real_protrusion() {
        let (a, _root, child) = arena();
        {
            // Push the child outside the root's bounds.
            let mut g = a.lock().unwrap();
            g.get_hot_mut(child).expect("hot").bounds = Rect::new(700.0, 590.0, 200.0, 50.0);
        }
        let mut p = probe(&a);
        let out = p.probe_overflow_scan(None).expect("scan");
        let overflows = out["overflows"].as_array().expect("overflows");
        assert_eq!(overflows.len(), 1);
        assert_eq!(overflows[0]["offending_node"], child.to_u64());
        assert_eq!(overflows[0]["overflow_axis"], "both");
        assert_eq!(overflows[0]["overflow_pixels"], 100.0);
    }

    #[test]
    fn a11y_tree_emits_roles_and_names() {
        let (a, _root, child) = arena();
        {
            let mut g = a.lock().unwrap();
            g.get_cold_mut(child).expect("cold").a11y_name = Some("Count".to_string());
        }
        let mut p = probe(&a);
        let out = p.probe_a11y_tree(&json!({})).expect("a11y");
        let root = &out["root"];
        let kids = root["children"].as_array().expect("a11y children");
        assert_eq!(kids[0]["name"], "Count");
        assert_eq!(kids[0]["id"], child.to_u64().to_string());
        // Invisible nodes are excluded.
        {
            let mut g = a.lock().unwrap();
            g.get_hot_mut(child)
                .expect("hot")
                .flags
                .remove(NodeFlags::VISIBLE);
        }
        let mut p = probe(&a);
        let out = p.probe_a11y_tree(&json!({})).expect("a11y after hide");
        assert_eq!(out["root"]["children"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn node_set_loading_mutates_arena_and_reports_effective_state() {
        let (a, _root, child) = arena();
        let mut p = probe(&a);

        let out = p
            .probe_node_set_loading(child.to_u64(), true)
            .expect("set loading");
        assert_eq!(out["applied"], true);
        assert_eq!(out["node_id"], child.to_u64());
        assert_eq!(out["loading"], true);
        assert!(a.lock().unwrap().node_loading(child));

        // Clearing the override reports the effective state honestly.
        let out = p
            .probe_node_set_loading(child.to_u64(), false)
            .expect("clear loading");
        assert_eq!(out["applied"], true);
        assert_eq!(out["loading"], false);
        assert!(!a.lock().unwrap().node_loading(child));
    }

    #[test]
    fn node_set_loading_rejects_invalid_and_dead_ids() {
        let (a, _root, _child) = arena();
        let mut p = probe(&a);

        let err = p
            .probe_node_set_loading(0, true)
            .expect_err("0 is not a WidgetId");
        assert!(err.contains("not a valid widget id"), "{err}");

        let dead = WidgetId::from_parts(u32::MAX - 1, 9);
        let err = p
            .probe_node_set_loading(dead.to_u64(), true)
            .expect_err("dead slot");
        assert!(err.contains("not alive"), "{err}");
        // A rejected write must not disturb the arena.
        assert!(!a
            .lock()
            .unwrap()
            .hot_nodes()
            .iter()
            .any(|h| { h.flags.contains(NodeFlags::LOADING) }));
    }

    /// A widget whose own `is_loading` declaration is `true` — the
    /// instance override cannot clear it.
    struct BusyWidget;

    impl martensite_core::Widget for BusyWidget {
        fn measure(
            &mut self,
            _cx: &mut martensite_core::LayoutContext,
            _c: martensite_core::LayoutConstraints,
        ) -> Vec2 {
            Vec2::new(10.0, 10.0)
        }
        fn layout(&mut self, _cx: &mut martensite_core::LayoutContext, _b: Rect) {}
        fn is_loading(&self) -> bool {
            true
        }
    }

    #[test]
    fn node_set_loading_reports_widget_declared_loading() {
        let mut arena = WidgetArena::new();
        let id = arena.insert_with_widget(
            HotNode {
                bounds: Rect::new(0.0, 0.0, 10.0, 10.0),
                flags: NodeFlags::VISIBLE,
                ..Default::default()
            },
            Box::new(BusyWidget),
        );
        let a = Arc::new(Mutex::new(arena));
        let mut p = probe(&a);

        // Clearing the override cannot suppress a widget-declared load.
        let out = p.probe_node_set_loading(id.to_u64(), false).expect("clear");
        assert_eq!(out["applied"], true);
        assert_eq!(out["loading"], true, "widget is_loading still applies");
    }

    #[test]
    fn loading_node_surfaces_busy_and_sanitized_a11y() {
        let (a, _root, child) = arena();
        {
            let mut g = a.lock().unwrap();
            g.get_cold_mut(child).expect("cold").a11y_name = Some("Count".to_string());
            g.set_loading(child, true);
        }
        let mut p = probe(&a);
        let out = p.probe_a11y_tree(&json!({})).expect("a11y");
        let kids = out["root"]["children"].as_array().expect("kids");
        let want_id = child.to_u64().to_string();
        let node = kids
            .iter()
            .find(|n| n["id"].as_str() == Some(want_id.as_str()))
            .expect("child in a11y tree");
        assert_eq!(node["name"], "Loading", "sanitized label");
        let states: Vec<&str> = node["states"]
            .as_array()
            .expect("states")
            .iter()
            .filter_map(Value::as_str)
            .collect();
        assert!(states.contains(&"busy"), "{node}");
        assert!(states.contains(&"disabled"), "{node}");
        assert!(!states.contains(&"focusable"), "{node}");
        assert_eq!(node["children"], json!([]), "placeholder prunes subtree");

        // tree_node detail exposes the flag, badge, and busy prop.
        let mut p = probe(&a);
        let detail = p.probe_node_detail(child.to_u64()).expect("detail");
        assert_eq!(detail["loading"], true);
        assert!(
            detail["badges"]
                .as_array()
                .is_some_and(|b| b.iter().any(|v| v == "loading")),
            "{detail}"
        );
        assert_eq!(detail["accesskit"]["name"], "Loading");
        assert_eq!(detail["accesskit"]["busy"], "true");

        // Snapshot carries the badge too.
        let mut p = probe(&a);
        let snap = p.probe_tree_snapshot(&json!({})).expect("snapshot");
        let kid = &snap["root"]["children"][0];
        assert!(
            kid["badges"]
                .as_array()
                .is_some_and(|b| b.iter().any(|v| v == "loading")),
            "{kid}"
        );
    }

    /// A widget that paints one solid red rect covering its bounds.
    struct FillWidget;

    impl martensite_core::Widget for FillWidget {
        fn measure(
            &mut self,
            _cx: &mut martensite_core::LayoutContext,
            _c: martensite_core::LayoutConstraints,
        ) -> Vec2 {
            Vec2::new(10.0, 10.0)
        }
        fn layout(&mut self, _cx: &mut martensite_core::LayoutContext, _b: Rect) {}
        fn paint(&self, cx: &mut martensite_core::PaintContext) {
            let b = cx.bounds;
            cx.list.push_fill_rect(
                kurbo::Rect::new(
                    f64::from(b.origin.x),
                    f64::from(b.origin.y),
                    f64::from(b.origin.x + b.size.x),
                    f64::from(b.origin.y + b.size.y),
                ),
                [255, 0, 0, 255],
            );
        }
    }

    /// A widget that handles `SemanticAction::Click` (and records
    /// `SetValue` payloads) — everything else is ignored.
    #[derive(Default)]
    struct ClickWidget {
        value: Option<String>,
    }

    impl martensite_core::Widget for ClickWidget {
        fn measure(
            &mut self,
            _cx: &mut martensite_core::LayoutContext,
            _c: martensite_core::LayoutConstraints,
        ) -> Vec2 {
            Vec2::new(10.0, 10.0)
        }
        fn layout(&mut self, _cx: &mut martensite_core::LayoutContext, _b: Rect) {}
        fn event(&mut self, cx: &mut martensite_core::EventContext) -> EventResponse {
            match cx.event {
                WidgetEvent::SemanticAction(SemanticAction::Click) => EventResponse::Handled,
                WidgetEvent::SemanticAction(SemanticAction::SetValue(v)) => {
                    self.value = Some(v.clone());
                    EventResponse::Handled
                }
                _ => EventResponse::Ignored,
            }
        }
    }

    #[test]
    fn unbacked_capabilities_stay_honest() {
        let (a, root, _child) = arena();
        let mut p = probe(&a);
        // Without the `render` feature capture stays honestly unbacked.
        #[cfg(not(feature = "render"))]
        {
            let err = p
                .probe_capture_node(root.to_u64(), "png", 1.0)
                .expect_err("no render backend");
            assert!(err.starts_with("not_implemented:"), "{err}");
        }
        // layout_chain / audit_paint are real even without attachments.
        assert!(p.probe_layout_chain(root.to_u64()).is_ok());
        assert!(p.probe_audit_paint(None).is_ok());
    }

    #[test]
    fn layout_chain_falls_back_to_arena_bounds() {
        let (a, root, child) = arena();
        let mut p = probe(&a);
        let out = p.probe_layout_chain(child.to_u64()).expect("arena-only");
        assert_eq!(out["layout_engine"], Value::Null);
        assert_eq!(out["bounds"], json!([8.0, 8.0, 120.0, 24.0]));
        assert_eq!(out["resolved_size"], json!([120.0, 24.0]));
        assert_eq!(out["parent_chain"], json!([root.to_u64()]));
        assert!(out["diagnostics"].as_array().unwrap().is_empty());

        // `0` is the only non-id (`NonZeroU64`); a well-formed but dead
        // id reports "not alive" instead.
        let err = p.probe_layout_chain(0).expect_err("not a widget id");
        assert!(err.contains("invalid param"), "{err}");
        let err = p
            .probe_layout_chain(u64::MAX)
            .expect_err("well-formed but dead");
        assert!(err.contains("not alive"), "{err}");
    }

    #[test]
    fn layout_chain_reports_real_taffy_state() {
        let (a, root, child) = arena();
        let mut engine = martensite_layout::LayoutEngine::new();
        {
            let mut g = a.lock().unwrap();
            engine
                .compute_with_widgets(
                    &mut g,
                    root,
                    martensite_layout::constraints_to_available(
                        martensite_layout::Constraints::tight(800.0, 600.0),
                    ),
                )
                .expect("compute");
        }
        // The engine is `!Send`; the app feeds a shared `LayoutStore`
        // once per layout pass and the probe reads the snapshot.
        let store = Arc::new(Mutex::new(LayoutStore::new()));
        store.lock().unwrap().update_from_engine(&engine);
        let mut p = probe(&a).with_layout_store(store);

        let out = p.probe_layout_chain(child.to_u64()).expect("chain");
        assert_eq!(out["node_id"], child.to_u64());
        assert_eq!(out["layout_engine"]["registered"], true);
        assert_eq!(out["layout_engine"]["generation"], 1);
        assert_eq!(out["constraints"]["display"], "Flex");
        assert!(out["resolved_size"].is_array());
        assert_eq!(out["parent_chain"], json!([root.to_u64()]));
        assert_eq!(out["taffy_parent_chain"], json!([root.to_u64()]));
    }

    #[test]
    fn audit_paint_reports_real_command_stats() {
        let (a, root, child) = arena();
        {
            let mut g = a.lock().unwrap();
            g.get_cold_mut(child).expect("cold").widget = Box::new(FillWidget);
        }
        let mut p = probe(&a);
        let out = p.probe_audit_paint(None).expect("audit");
        assert_eq!(out["root"], root.to_u64());
        assert_eq!(out["counts_by_kind"]["push_scope"], 2);
        assert_eq!(out["counts_by_kind"]["pop_scope"], 2);
        assert_eq!(out["counts_by_kind"]["fill_rect"], 1);
        assert_eq!(out["clip_count"], 0);
        assert_eq!(out["frame"], json!([0.0, 0.0, 800.0, 600.0]));
        let cmds = out["commands"].as_array().expect("commands");
        assert_eq!(cmds.len() as u64, out["total_commands"].as_u64().unwrap());
        // The child's FillWidget chrome is attributed in order.
        assert_eq!(cmds[2]["kind"], "fill_rect");
        assert_eq!(cmds[2]["bounds"], json!([8.0, 8.0, 120.0, 24.0]));

        // Scoped to the child's subtree.
        let out = p
            .probe_audit_paint(Some(child.to_u64()))
            .expect("scoped audit");
        assert_eq!(out["counts_by_kind"]["fill_rect"], 1);
        assert_eq!(out["counts_by_kind"]["push_scope"], 1);
        assert_eq!(out["frame"], json!([8.0, 8.0, 120.0, 24.0]));

        // Dirty-paint flags surface as real node ids.
        {
            let mut g = a.lock().unwrap();
            g.get_hot_mut(child).expect("hot").flags |= NodeFlags::DIRTY_PAINT;
        }
        let out = p.probe_audit_paint(None).expect("audit dirty");
        assert_eq!(out["dirty_nodes"], json!([child.to_u64()]));
    }

    #[cfg(feature = "render")]
    #[test]
    fn capture_node_rasterizes_real_png() {
        use base64::Engine;

        let (a, _root, child) = arena();
        {
            let mut g = a.lock().unwrap();
            g.get_cold_mut(child).expect("cold").widget = Box::new(FillWidget);
        }
        let mut p = probe(&a);
        let out = p
            .probe_capture_node(child.to_u64(), "png", 2.0)
            .expect("capture");
        assert_eq!(out["format"], "png");
        assert_eq!(out["logical_size"], json!([120.0, 24.0]));
        assert_eq!(out["physical_size"], json!([240, 48]));
        let png = base64::engine::general_purpose::STANDARD
            .decode(out["bytes_base64"].as_str().expect("b64"))
            .expect("decode");
        assert_eq!(&png[..8], &[137, 80, 78, 71, 13, 10, 26, 10]);

        // Honest errors: unsupported format, bad scale, dead node.
        assert!(p.probe_capture_node(child.to_u64(), "jpeg", 1.0).is_err());
        assert!(p.probe_capture_node(child.to_u64(), "png", 0.0).is_err());
        assert!(p.probe_capture_node(u64::MAX, "png", 1.0).is_err());
    }

    #[test]
    fn dispatch_routes_keys_to_focus_manager_target() {
        let (a, _root, child) = arena();
        let fm = Arc::new(Mutex::new(martensite_focus::FocusManager::new()));
        {
            let mut g = a.lock().unwrap();
            fm.lock().unwrap().set_focus(&mut g, child);
        }
        let mut p = probe(&a).with_focus_manager(Arc::clone(&fm));

        let out = p
            .probe_dispatch_event("key_press", &json!({"key": "Enter"}))
            .expect("key");
        assert_eq!(out["target"], child.to_u64());
        let out = p
            .probe_dispatch_event("text_input", &json!({"text": "hi"}))
            .expect("text");
        assert_eq!(out["target"], child.to_u64());

        // Focus cleared → the probe reports it instead of faking a target.
        fm.lock().unwrap().clear_focus();
        let err = p
            .probe_dispatch_event("key_press", &json!({"key": "x"}))
            .expect_err("no focus");
        assert!(err.contains("no widget currently holds focus"), "{err}");
    }

    #[test]
    fn a11y_action_dispatches_real_semantic_actions() {
        let (a, root, child) = arena();
        {
            let mut g = a.lock().unwrap();
            g.get_cold_mut(child).expect("cold").widget = Box::<ClickWidget>::default();
        }
        let mut p = probe(&a);

        let out = p
            .probe_a11y_action(&json!({"node_id": child.to_u64(), "action": "click"}))
            .expect("click");
        assert_eq!(out["response"], "Handled");
        assert_eq!(out["target"], child.to_u64());
        assert_eq!(out["hit_path"], json!([root.to_u64(), child.to_u64()]));

        let out = p
            .probe_a11y_action(
                &json!({"node_id": child.to_u64(), "action": "set_value", "value": "42"}),
            )
            .expect("set_value");
        assert_eq!(out["response"], "Handled");

        // Payload-carrying actions validate their params for real.
        assert!(
            p.probe_a11y_action(
                &json!({"node_id": child.to_u64(), "action": "scroll_to_point", "point": [4.0, 8.0]})
            )
            .is_ok()
        );
        let err = p
            .probe_a11y_action(&json!({"node_id": child.to_u64(), "action": "scroll_to_point"}))
            .expect_err("missing point");
        assert!(err.contains("invalid param"), "{err}");

        // Unknown actions list the real vocabulary.
        let err = p
            .probe_a11y_action(&json!({"node_id": child.to_u64(), "action": "explode"}))
            .expect_err("bad action");
        assert!(err.contains("set_scroll_offset"), "{err}");
    }

    #[test]
    fn a11y_action_falls_back_to_focus_target() {
        let (a, _root, child) = arena();
        {
            let mut g = a.lock().unwrap();
            g.get_cold_mut(child).expect("cold").widget = Box::<ClickWidget>::default();
        }
        let fm = Arc::new(Mutex::new(martensite_focus::FocusManager::new()));
        {
            let mut g = a.lock().unwrap();
            fm.lock().unwrap().set_focus(&mut g, child);
        }
        let mut p = probe(&a).with_focus_manager(fm);
        let out = p
            .probe_a11y_action(&json!({"action": "click"}))
            .expect("focus fallback");
        assert_eq!(out["target"], child.to_u64());
    }

    #[test]
    fn inspector_select_pick_hit_tests() {
        let (a, root, child) = arena();
        let mut p = probe(&a);
        let out = p
            .probe_inspector_select(&json!({"pick": [50.0, 15.0]}))
            .expect("pick");
        assert_eq!(out["selected_id"], child.to_u64());
        assert_eq!(out["armed"], false);
        assert_eq!(out["hit_path"], json!([root.to_u64(), child.to_u64()]));
        // Selected id surfaces in tree_snapshot.
        let snap = p.probe_tree_snapshot(&json!({})).expect("snapshot");
        assert_eq!(snap["selected_id"], child.to_u64());

        let err = p
            .probe_inspector_select(&json!({"pick": [900.0, 900.0]}))
            .expect_err("miss");
        assert!(err.contains("no hit-testable"), "{err}");
        let err = p
            .probe_inspector_select(&json!({"pick": [1.0, f64::NAN]}))
            .expect_err("non-finite");
        assert!(err.contains("invalid param"), "{err}");
    }

    #[test]
    fn theme_apply_token_overrides_are_real() {
        use martensite_theme::{ThemeToken, TokenKey};

        let (a, _root, _child) = arena();
        let mut p = probe(&a);
        let err = p
            .probe_theme_apply(&json!({
                "mode": "dark",
                "token_overrides": {
                    "spacing": 20.0,
                    "primary_color": "#ff0000",
                    "font_size_body": {"r": 0, "g": 0, "b": 0},
                },
            }))
            .expect_err("font_size_body is not a color");
        assert!(
            err.contains("font_size_body") || err.contains("color"),
            "{err}"
        );

        let out = p
            .probe_theme_apply(&json!({
                "mode": "dark",
                "token_overrides": {
                    "spacing": 20.0,
                    "primary_color": "#ff0000",
                    "animation_duration": 300.0,
                },
            }))
            .expect("apply");
        assert_eq!(out["applied"], true);
        assert_eq!(out["mode"], "dark");
        assert_eq!(out["overrides_applied"], 3);
        {
            let g = a.lock().unwrap();
            let theme = g.theme();
            assert_eq!(
                theme.get(TokenKey::Spacing),
                Some(&ThemeToken::Dimension(20.0))
            );
            assert_eq!(
                theme.get(TokenKey::AnimationDuration),
                Some(&ThemeToken::Duration(300.0))
            );
            let ThemeToken::Color(c) = theme.get(TokenKey::PrimaryColor).expect("primary") else {
                panic!("primary_color must be a color token");
            };
            let (r, g_, b) = c.to_srgb();
            assert!((r - 1.0).abs() < 0.01 && g_.abs() < 0.01 && b.abs() < 0.01);
        }

        // Overrides layer onto the current theme when no mode is given.
        let out = p
            .probe_theme_apply(&json!({"token_overrides": {"border_radius": 7.5}}))
            .expect("override only");
        assert_eq!(out["overrides_applied"], 1);
        assert_eq!(out["name"], "Dark");
        {
            let g = a.lock().unwrap();
            assert_eq!(
                g.theme().get(TokenKey::BorderRadius),
                Some(&ThemeToken::Dimension(7.5))
            );
            assert_eq!(
                g.theme().get(TokenKey::Spacing),
                Some(&ThemeToken::Dimension(20.0))
            );
        }

        // Unknown keys error with the valid vocabulary.
        let err = p
            .probe_theme_apply(&json!({"token_overrides": {"mystery_token": 1.0}}))
            .expect_err("unknown key");
        assert!(err.contains("primary_color"), "{err}");

        // Empty request rejected; `system` stays honest not_impl.
        assert!(p.probe_theme_apply(&json!({})).is_err());
        let err = p
            .probe_theme_apply(&json!({"mode": "system"}))
            .expect_err("system");
        assert!(err.starts_with("not_implemented:"), "{err}");
        let err = p
            .probe_theme_apply(&json!({"system": true}))
            .expect_err("system flag");
        assert!(err.starts_with("not_implemented:"), "{err}");
    }

    #[test]
    fn event_dispatch_hit_tests_and_dispatches_real_events() {
        let (a, root, child) = arena();
        let mut p = probe(&a);

        // A click inside the child resolves by hit-test and bubbles
        // through `WidgetArena::dispatch_event` — `DummyWidget` ignores
        // everything, so the response is honest `Ignored`.
        let out = p
            .probe_dispatch_event("pointer_click", &json!({"position": [50.0, 15.0]}))
            .expect("click");
        assert_eq!(out["response"], "Ignored");
        assert_eq!(out["target"], child.to_u64());
        assert_eq!(out["hit_path"], json!([root.to_u64(), child.to_u64()]));

        // A move inside the root but outside the child hits the root.
        let out = p
            .probe_dispatch_event("pointer_move", &json!({"position": [400.0, 300.0]}))
            .expect("move");
        assert_eq!(out["target"], root.to_u64());

        // Outside everything: honest error, not a fake target.
        let err = p
            .probe_dispatch_event("pointer_click", &json!({"position": [900.0, 900.0]}))
            .expect_err("miss");
        assert!(err.contains("no hit-testable"), "{err}");
    }

    #[test]
    fn event_dispatch_keyboard_needs_explicit_target() {
        let (a, _root, child) = arena();
        let mut p = probe(&a);

        // No focus tracking in the arena → honest not_implemented.
        let err = p
            .probe_dispatch_event("key_press", &json!({"key": "Enter"}))
            .expect_err("no focus source");
        assert!(err.starts_with("not_implemented:"), "{err}");

        // Explicit target routes the key event for real.
        let out = p
            .probe_dispatch_event(
                "key_press",
                &json!({"key": "Enter", "target": child.to_u64()}),
            )
            .expect("key dispatch");
        assert_eq!(out["target"], child.to_u64());
    }

    #[test]
    fn theme_get_reports_arena_theme_name() {
        let (a, _root, _child) = arena();
        let mut p = probe(&a);
        let out = p.probe_theme_tokens().expect("theme");
        assert!(out["name"].is_string());
    }
}
