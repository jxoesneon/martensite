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

    /// Item index containing window-space `pos`, or `None`.
    fn item_at(&self, pos: Vec2) -> Option<usize> {
        self.item_bounds.iter().position(|r| {
            pos.x >= r.min_x() && pos.x < r.max_x() && pos.y >= r.min_y() && pos.y < r.max_y()
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
        let b = &self.bounds;
        let rect = PaintRect::new(
            f64::from(b.min_x()),
            f64::from(b.min_y()),
            f64::from(b.max_x()),
            f64::from(b.max_y()),
        );
        cx.list.push_fill_rect(rect, pal.raised);
        cx.list.push_stroke_rect(rect, cx.pt(1.0), pal.border);
        let state = self.shared.lock();
        let mut text = self.text.lock();
        for (i, label) in state.items.iter().enumerate() {
            let Some(r) = self.item_bounds.get(i) else {
                continue;
            };
            if i == state.highlighted {
                cx.list.push_fill_rect(
                    PaintRect::new(
                        f64::from(r.min_x()),
                        f64::from(r.min_y()),
                        f64::from(r.max_x()),
                        f64::from(r.max_y()),
                    ),
                    Palette::alpha(pal.accent, 90),
                );
            }
            text.push(
                cx.list,
                Point::new(
                    f64::from(r.min_x()) + 8.0 * sd,
                    f64::from(r.min_y()) + (f64::from(r.size.y) - f64::from(FONT_PT) * sd) * 0.5,
                ),
                label,
                FONT_PT * s,
                pal.text,
                Some(r.size.x),
            );
        }
    }
}
