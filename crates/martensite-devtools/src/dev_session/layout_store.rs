//! Per-frame layout snapshot bridging the app-side
//! [`LayoutEngine`] to the dev channel.
//!
//! `LayoutEngine` is `!Send` (Taffy's compact-length internals carry a
//! `*const ()`), so it can never live behind the probe's socket-thread
//! mutex. `LayoutStore` is the `Send` counterpart: the app calls
//! [`update_from_engine`](LayoutStore::update_from_engine) on its own
//! thread once per layout pass — next to
//! [`DevSession::on_frame`](super::DevSession::on_frame) — and the probe
//! serves `layout_chain` from the recorded snapshot.
//!
//! ```no_run
//! use std::sync::{Arc, Mutex};
//!
//! use martensite_devtools::dev_session::layout_store::LayoutStore;
//! use martensite_layout::LayoutEngine;
//!
//! let store = Arc::new(Mutex::new(LayoutStore::new()));
//! // Hand `store` to `WidgetArenaProbe::with_layout_store`; per frame:
//! // `store.lock().unwrap().update_from_engine(&engine);`
//! # let engine = LayoutEngine::new();
//! store.lock().unwrap().update_from_engine(&engine);
//! ```

use std::collections::HashMap;

use serde_json::{json, Value};

use martensite_layout::LayoutEngine;

/// Wire form of a Taffy `CompactLength` (`Dimension` /
/// `LengthPercentageAuto` payload): lengths as numbers, percentages as
/// `{percent}`, keywords as snake_case strings.
fn compact_wire(c: martensite_layout::CompactLength) -> Value {
    use martensite_layout::CompactLength as C;
    match c.tag() {
        C::LENGTH_TAG => json!(c.value()),
        C::PERCENT_TAG => json!({ "percent": c.value() }),
        C::AUTO_TAG => json!("auto"),
        C::MIN_CONTENT_TAG => json!("min_content"),
        C::MAX_CONTENT_TAG => json!("max_content"),
        C::FIT_CONTENT_PX_TAG => json!({ "fit_content_px": c.value() }),
        C::FIT_CONTENT_PERCENT_TAG => json!({ "fit_content_percent": c.value() }),
        C::FIT_CONTENT_KEYWORD_TAG => json!("fit_content"),
        C::STRETCH_TAG => json!("stretch"),
        C::CONTENT_TAG => json!("content"),
        _ => json!(format!("{c:?}")),
    }
}

/// Wire form of one [`martensite_layout::LayoutDiagnostic`].
fn diagnostic_wire(d: &martensite_layout::LayoutDiagnostic) -> Value {
    let cause = if d.has_overflow() {
        format!(
            "resolved size exceeds offered size by {:.2}x{:.2}px",
            d.overflow_delta.x, d.overflow_delta.y
        )
    } else {
        "constraint violation recorded during layout".to_string()
    };
    json!({
        "widget_id": d.widget_id.map(|w| w.to_u64()),
        "bounds": [d.bounds.origin.x, d.bounds.origin.y, d.bounds.width(), d.bounds.height()],
        "offered_size": [d.offered_size.x, d.offered_size.y],
        "resolved_size": [d.resolved_size.x, d.resolved_size.y],
        "overflow_delta": [d.overflow_delta.x, d.overflow_delta.y],
        "cause": cause,
    })
}

/// Per-widget layout record extracted from a [`LayoutEngine`] pass.
#[derive(Debug, Clone)]
pub struct LayoutNodeRecord {
    /// Taffy-resolved `[width, height]` in logical pixels.
    pub resolved_size: [f32; 2],
    /// Taffy-resolved `[x, y]` placement inside the parent's content box.
    pub resolved_location: [f32; 2],
    /// Style constraints registered on the node (`display`, `size`,
    /// `min_size`, `max_size`) as a JSON object.
    pub constraints: Value,
    /// Widget ids of Taffy ancestors, root-first.
    pub taffy_parent_chain: Vec<u64>,
}

/// Snapshot of one layout pass, keyed by widget id.
#[derive(Debug, Default)]
pub struct LayoutStore {
    records: HashMap<u64, LayoutNodeRecord>,
    /// All engine diagnostics from the latest pass (unfiltered).
    diagnostics: Vec<Value>,
    /// Monotonic update counter — bumped on every
    /// [`update_from_engine`](Self::update_from_engine) call.
    generation: u64,
}

impl LayoutStore {
    /// Creates an empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Replaces the snapshot with the engine's current state. Call once
    /// per layout pass on the thread owning the engine.
    pub fn update_from_engine(&mut self, engine: &LayoutEngine) {
        self.generation += 1;
        self.records.clear();
        for (widget_id, node) in engine.iter_nodes() {
            let resolved = engine.layout(*node).ok();
            let constraints = engine
                .tree
                .style(*node)
                .ok()
                .map(|s| {
                    json!({
                        "display": format!("{:?}", s.display),
                        "size": {
                            "width": compact_wire(s.size.width.into_raw()),
                            "height": compact_wire(s.size.height.into_raw()),
                        },
                        "min_size": {
                            "width": compact_wire(s.min_size.width.into_raw()),
                            "height": compact_wire(s.min_size.height.into_raw()),
                        },
                        "max_size": {
                            "width": compact_wire(s.max_size.width.into_raw()),
                            "height": compact_wire(s.max_size.height.into_raw()),
                        },
                    })
                })
                .unwrap_or(Value::Null);
            let mut taffy_parent_chain = Vec::new();
            let mut parent = engine.tree.parent(*node);
            while let Some(p) = parent {
                if let Some(wid) = engine.lookup_widget(p) {
                    taffy_parent_chain.push(wid.to_u64());
                }
                parent = engine.tree.parent(p);
            }
            taffy_parent_chain.reverse();
            self.records.insert(
                widget_id.to_u64(),
                LayoutNodeRecord {
                    resolved_size: resolved.map_or([0.0; 2], |l| [l.size.width, l.size.height]),
                    resolved_location: resolved.map_or([0.0; 2], |l| [l.location.x, l.location.y]),
                    constraints,
                    taffy_parent_chain,
                },
            );
        }
        self.diagnostics = engine.diagnostics().iter().map(diagnostic_wire).collect();
    }

    /// The recorded layout of one widget, if it was registered in the
    /// last pass.
    #[must_use]
    pub fn record(&self, widget_id: u64) -> Option<&LayoutNodeRecord> {
        self.records.get(&widget_id)
    }

    /// Engine diagnostics naming `widget_id` from the last pass.
    #[must_use]
    pub fn diagnostics_for(&self, widget_id: u64) -> Vec<Value> {
        self.diagnostics
            .iter()
            .filter(|d| d.get("widget_id").and_then(Value::as_u64) == Some(widget_id))
            .cloned()
            .collect()
    }

    /// All diagnostics from the last pass.
    #[must_use]
    pub fn diagnostics(&self) -> &[Value] {
        &self.diagnostics
    }

    /// Number of widgets recorded in the last pass.
    #[must_use]
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// `true` when the last pass recorded no widgets.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// Updates applied since construction.
    #[must_use]
    pub fn generation(&self) -> u64 {
        self.generation
    }
}
