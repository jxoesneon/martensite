//! Context menu popup — opened by `GridPanel` on a secondary-button
//! press, hosted in the arena `OverlayLayer` under
//! `OverlayAnchor::Pointer`.
//!
//! Mirrors the `Dropdown`/`ListBoxPopup` reconcile pattern: the menu
//! writes a committed index into a shared [`MenuState`], the owning
//! panel drains it inside `sync_overlay`, maps it to a real action
//! (clipboard payload), and closes the entry. Item visuals follow the
//! theme tokens through `Palette` like every other panel.

use std::sync::Arc;

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite::core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, SemanticAction, Widget, WidgetEvent,
};
use martensite::render::{Point, Rect as PaintRect};
use martensite_motion::{SpringConfig, SpringSolver};
use parking_lot::Mutex;

use crate::model::Palette;
use crate::text::TextPainter;

/// Horizontal padding inside the menu panel (logical pt).
const PAD_X: f32 = 10.0;
/// Vertical padding inside the menu panel (logical pt).
const PAD_Y: f32 = 6.0;
/// Item row height (logical pt).
const ROW_H: f32 = 22.0;
/// Label font size (logical pt) — the paint audit's WCAG floor is
/// 12pt; menu items are readable body text, not dense-grid cells.
const FONT_PT: f32 = 12.0;
/// Minimum menu width (logical pt).
const MIN_W: f32 = 96.0;

/// Entrance spring configuration — lightly under-damped (`ζ ≈ 0.80`,
/// `ω₀ ≈ 64 rad/s`) so the 0→1 progress settles in ~180ms (the
/// solver's 1e-4 settle threshold needs ≈9.2/(ζω₀) seconds) with a
/// whisper of overshoot. Subtle on purpose: this is a motion-quality
/// showcase, not a bounce demo.
fn entrance_config() -> SpringConfig {
    SpringConfig::new(1.0, 4100.0, 102.0).expect("menu entrance spring is statically valid")
}

/// Entrance progress `t` (1.0 once settled or never armed) — sampled
/// from the shared solver the owning `GridPanel::tick` advances.
fn entrance_t(state: &MenuState) -> f32 {
    state
        .entrance
        .map(|spring| spring.sample().0)
        .unwrap_or(1.0)
}

/// State shared between `GridPanel` and its popup [`ContextMenu`].
///
/// The popup writes `committed` on activation (click, Enter, or an AT
/// `Click` action) and `highlighted` on hover; the owner drains both in
/// `sync_overlay`.
#[derive(Debug)]
pub struct MenuState {
    /// Item labels — mirrored from the owner when the menu opens.
    pub items: Vec<String>,
    /// Index under the pointer / keyboard highlight.
    pub highlighted: usize,
    /// Set by the menu when an item is activated.
    pub committed: Option<usize>,
    /// Entrance animation: a 0→1 [`SpringSolver`] armed by the owner
    /// when the popup opens. Overlay entries never receive `tick`, so
    /// the owning `GridPanel::tick` advances it each frame and clears
    /// it once [`SpringSolver::settle_threshold`] trips; `paint` reads
    /// the sampled position for a translate + fade.
    pub entrance: Option<SpringSolver>,
}

impl MenuState {
    /// Arms the entrance spring — call when the popup opens.
    pub fn arm_entrance(&mut self) {
        self.entrance = Some(SpringSolver::new(entrance_config(), 0.0, 1.0, 0.0));
    }
}

/// Right-click context menu — a compact raised panel of text items.
///
/// Pointer hover moves the highlight, a primary press commits the item
/// under the cursor, `Up`/`Down`/`Enter` drive the keyboard path, and
/// outside-press/`Escape` dismissal is handled by the `OverlayLayer`.
pub struct ContextMenu {
    /// Shared state with the owning `GridPanel`.
    shared: Arc<Mutex<MenuState>>,
    /// Shaped-text painter — same dashboard-local `TextPainter` the
    /// panels use.
    text: Mutex<TextPainter>,
    /// Cached bounds from the last layout pass.
    bounds: Rect,
    /// Per-item rects resolved in `layout` — used for hit-testing and
    /// the highlight fill.
    item_bounds: Vec<Rect>,
    /// Scale factor captured at layout — event hit-math must agree
    /// with paint.
    scale: f32,
}

impl ContextMenu {
    pub fn new(shared: Arc<Mutex<MenuState>>) -> Self {
        Self {
            shared,
            text: Mutex::new(TextPainter::new()),
            bounds: Rect::default(),
            item_bounds: Vec::new(),
            scale: 1.0,
        }
    }

    /// The entrance slide offset in device px (`0` once settled) —
    /// the same `dy` `paint` adds to every rect. Keeping `t` unclamped
    /// matches the paint path so mid-overshoot hit-tests agree.
    fn entrance_dy(&self) -> f32 {
        (1.0 - entrance_t(&self.shared.lock())) * 4.0 * self.scale
    }

    /// Item index containing window-space `pos`, or `None`.
    fn item_at(&self, pos: Vec2) -> Option<usize> {
        // `paint` slides the whole menu by `entrance_dy` during the
        // entrance — hit-test the drawn position so a mid-animation
        // click lands on the item under the cursor.
        let dy = self.entrance_dy();
        self.item_bounds.iter().position(|r| {
            pos.x >= r.min_x()
                && pos.x < r.max_x()
                && pos.y - dy >= r.min_y()
                && pos.y - dy < r.max_y()
        })
    }

    /// Commits `index` into the shared state for the owner to drain.
    fn commit(&self, index: usize) {
        let mut state = self.shared.lock();
        if index < state.items.len() {
            state.committed = Some(index);
        }
    }
}

impl Widget for ContextMenu {
    fn debug_name(&self) -> &'static str {
        "Context Menu"
    }

    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let state = self.shared.lock();
        // Content width — longest label at the menu font size plus
        // gutters — clamped to the offered constraint so the layer can
        // keep the popup inside the viewport.
        let longest = state
            .items
            .iter()
            .map(|i| i.chars().count())
            .max()
            .unwrap_or(0) as f32;
        let w = (longest * cx.pt(FONT_PT * 0.62) + 2.0 * cx.pt(PAD_X))
            .max(cx.pt(MIN_W))
            .min(constraints.max_size.x.max(0.0));
        let h = (state.items.len() as f32 * cx.pt(ROW_H) + 2.0 * cx.pt(PAD_Y))
            .min(constraints.max_size.y.max(0.0));
        Vec2::new(w, h)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.scale = cx.scale;
        self.bounds = bounds;
        let n = self.shared.lock().items.len();
        self.item_bounds.clear();
        let row_h = cx.pt(ROW_H);
        for i in 0..n {
            self.item_bounds.push(Rect::new(
                bounds.min_x() + cx.pt(PAD_X) * 0.5,
                bounds.min_y() + cx.pt(PAD_Y) + i as f32 * row_h,
                bounds.size.x - cx.pt(PAD_X),
                row_h,
            ));
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerMoved { position } => {
                if let Some(idx) = self.item_at(*position) {
                    let mut state = self.shared.lock();
                    if state.highlighted != idx {
                        state.highlighted = idx;
                        return EventResponse::RequestRepaint;
                    }
                }
                EventResponse::Handled
            }
            WidgetEvent::PointerPressed {
                position,
                button: PointerButton::Primary,
            } => {
                if let Some(idx) = self.item_at(*position) {
                    self.commit(idx);
                }
                EventResponse::Handled
            }
            WidgetEvent::KeyPressed { key, repeat } => {
                let n = self.shared.lock().items.len();
                if n == 0 {
                    return EventResponse::Handled;
                }
                match key.as_str() {
                    "ArrowDown" => {
                        let mut state = self.shared.lock();
                        state.highlighted = (state.highlighted + 1).min(n - 1);
                        EventResponse::RequestRepaint
                    }
                    "ArrowUp" => {
                        let mut state = self.shared.lock();
                        state.highlighted = state.highlighted.saturating_sub(1);
                        EventResponse::RequestRepaint
                    }
                    "Enter" if !repeat => {
                        let idx = self.shared.lock().highlighted;
                        self.commit(idx);
                        EventResponse::Handled
                    }
                    _ => EventResponse::Ignored,
                }
            }
            // AT activation commits the highlighted item.
            WidgetEvent::SemanticAction(SemanticAction::Click) => {
                let idx = self.shared.lock().highlighted;
                self.commit(idx);
                EventResponse::Handled
            }
            _ => EventResponse::Ignored,
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Menu);
        let state = self.shared.lock();
        node.set_label(format!("Row actions — {}", state.items.join(", ")));
    }

    fn paint(&self, cx: &mut PaintContext) {
        let pal = Palette::from_theme(cx.theme);
        let s = cx.scale;
        let sd = f64::from(s);
        let state = self.shared.lock();
        // Entrance progress — the owning `GridPanel::tick` advances
        // the shared solver (overlay entries never tick); `None`
        // means the animation settled or never ran. Translate-only:
        // an alpha fade would emit sub-threshold-contrast text/strokes
        // every frame (paint-audit lint spam), so the entrance is a
        // 4px slide at full opacity — `t` stays unclamped so the
        // spring's slight overshoot reads through as a dip past the
        // anchor point.
        let t = entrance_t(&state);
        let dy = f64::from((1.0 - t) * 4.0 * s);
        let b = &self.bounds;
        let rect = PaintRect::new(
            f64::from(b.min_x()),
            f64::from(b.min_y()) + dy,
            f64::from(b.max_x()),
            f64::from(b.max_y()) + dy,
        );
        cx.list.push_fill_rect(rect, pal.raised);
        cx.list.push_stroke_rect(rect, cx.pt(1.0), pal.border);
        let mut text = self.text.lock();
        for (i, label) in state.items.iter().enumerate() {
            let Some(r) = self.item_bounds.get(i) else {
                continue;
            };
            if i == state.highlighted {
                cx.list.push_fill_rect(
                    PaintRect::new(
                        f64::from(r.min_x()),
                        f64::from(r.min_y()) + dy,
                        f64::from(r.max_x()),
                        f64::from(r.max_y()) + dy,
                    ),
                    Palette::alpha(pal.accent, 90),
                );
            }
            text.push(
                cx.list,
                Point::new(
                    f64::from(r.min_x()) + 8.0 * sd,
                    f64::from(r.min_y())
                        + dy
                        + (f64::from(r.size.y) - f64::from(FONT_PT) * sd) * 0.5,
                ),
                label,
                FONT_PT * s,
                pal.text,
                Some(r.size.x),
            );
        }
    }
}
