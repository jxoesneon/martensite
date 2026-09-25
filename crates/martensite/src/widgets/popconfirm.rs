//! `Popconfirm` widget: a mini anchored confirmation bubble
//! (Ant `Popconfirm`).
//!
//! The `Popconfirm` itself is a zero-size marker placed in the widget
//! tree where the bubble should point — its layout bounds are the
//! anchor rect (or [`Popconfirm::anchor`] overrides it). Calling
//! [`Popconfirm::open`] reconciles a small surface — question text, a
//! Confirm/Cancel button pair, and the shared arrow tail — into the
//! [`OverlayLayer`](martensite_core::overlay::OverlayLayer) at
//! `OverlayAnchor::BoundsEdge { rect, edge }` on the next
//! [`Popconfirm::sync_overlay`], placed on
//! [`Popconfirm::preferred_edge`] with flip fallback.
//!
//! - **Dismissal**: light-dismiss semantics — an outside press or
//!   `Escape` closes the bubble and reports
//!   [`ConfirmResult::Cancel`]; presses inside it are consumed.
//! - **Result**: the button the user activated lands in
//!   [`Popconfirm::take_result`] (or the shared cell wired via
//!   [`Popconfirm::result_sink`], the overlay-host observation seam —
//!   mirrors `Dialog::response_sink`).
//! - The marker emits `Role::GenericContainer` with
//!   `aria-haspopup="dialog"` / `aria-expanded`; the surface emits
//!   `Role::Dialog` labelled by the question.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::Popconfirm;
//!
//! let mut c = Popconfirm::new().question("Delete this item?");
//! c.open();
//! assert!(c.is_open());
//! assert_eq!(c.take_result(), None);
//! ```

use std::sync::{Arc, Mutex};

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::overlay::{AnchorEdge, OverlayAnchor, OverlayLayer};
use martensite_core::shape::Shape;
use martensite_core::widget::{
    A11yEmittedNode, EventContext, EventResponse, LayoutConstraints, LayoutContext, OverlayA11yRef,
    PaintContext, PointerButton, SemanticAction, Widget, WidgetEvent,
};
use martensite_core::{NodeFlags, Rect, TokenKey};

use crate::widgets::popover::{anchor_facing_edge, tail_path};

/// Bubble chrome (logical points).
const PAD: f32 = 10.0;
/// Question strip height (logical points).
const QUESTION_H: f32 = 22.0;
/// Minimum bubble width (logical points).
const MIN_W: f32 = 140.0;
/// Footer button geometry (logical points).
const BUTTON_H: f32 = 24.0;
const BUTTON_GAP: f32 = 8.0;
/// Tail geometry (logical points) — same proportions as `Popover`.
const TAIL_TIP: f32 = 7.0;
const TAIL_HALF: f32 = 7.0;
/// Bubble chrome colours.
const SURFACE: [u8; 4] = [252, 252, 254, 255];
const EDGE: [u8; 4] = [140, 145, 155, 255];
const INK: [u8; 4] = [20, 20, 25, 255];
const ACCENT: [u8; 4] = [40, 110, 220, 255];

/// The terminal result of a [`Popconfirm`].
///
/// # Examples
///
/// ```
/// use martensite::widgets::ConfirmResult;
///
/// assert_ne!(ConfirmResult::Confirm, ConfirmResult::Cancel);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfirmResult {
    /// The confirm button was activated.
    Confirm,
    /// The cancel button was activated, or the bubble was
    /// light-dismissed (outside press, `Escape`).
    Cancel,
}

/// State shared between a [`Popconfirm`] marker and its live surface:
/// the anchor rect (owner → surface, so the tail tracks re-anchoring)
/// and the activation result (surface → owner).
#[derive(Debug, Default)]
struct PopconfirmShared {
    /// The anchor rect the tail points at (window space).
    anchor: Rect,
    /// Set by the surface when a button (or embedded `Escape`)
    /// resolves the confirmation.
    result: Option<ConfirmResult>,
}

/// The popup surface for a [`Popconfirm`] — a `Role::Dialog` mini
/// bubble with the question and a Confirm/Cancel pair.
struct PopconfirmSurface {
    /// The question text.
    question: String,
    /// Confirm button label.
    confirm_label: String,
    /// Cancel button label.
    cancel_label: String,
    /// Shared state with the owning `Popconfirm`.
    shared: Arc<Mutex<PopconfirmShared>>,
    /// The requested placement edge — the tail's flow axis.
    flow: AnchorEdge,
    /// Surface bounds from the last layout pass.
    bounds: Rect,
    /// `[cancel, confirm]` button rects from the last layout pass.
    button_rects: [Rect; 2],
    /// The silhouette painted last frame — the single source of truth
    /// for `clip_shape`/`hit_shape`.
    painted_shape: Mutex<Shape>,
    /// Shared shaped-text painter from the owning `Popconfirm`.
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl PopconfirmSurface {
    /// Records the activation into shared state — the owner drains it
    /// in `sync_overlay`.
    fn resolve(&self, result: ConfirmResult) {
        self.shared
            .lock()
            .expect("popconfirm state poisoned")
            .result = Some(result);
    }

    /// Per-button width from its label (device px).
    fn button_width(label: &str, scale_pt: impl Fn(f32) -> f32) -> f32 {
        (label.chars().count() as f32 * scale_pt(7.5) + scale_pt(24.0)).max(scale_pt(56.0))
    }
}

impl Widget for PopconfirmSurface {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let pad = cx.pt(PAD);
        let question_w = self.question.chars().count() as f32 * cx.pt(7.0);
        let buttons_w = Self::button_width(&self.confirm_label, |v| cx.pt(v))
            + Self::button_width(&self.cancel_label, |v| cx.pt(v))
            + cx.pt(BUTTON_GAP);
        let w = (question_w.max(buttons_w) + pad * 2.0).max(cx.pt(MIN_W));
        Vec2::new(
            w.min(constraints.max_size.x.max(0.0)),
            (pad + cx.pt(QUESTION_H) + pad * 0.5 + cx.pt(BUTTON_H) + pad)
                .min(constraints.max_size.y.max(0.0)),
        )
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        cx.hot.flags |= NodeFlags::FOCUSABLE;
        let pad = cx.pt(PAD);
        let button_h = cx.pt(BUTTON_H);
        let by = bounds.max_y() - pad - button_h;
        // Right-aligned pair: [confirm][gap][cancel]? No — platform
        // convention puts the primary action last (rightmost).
        let confirm_w = Self::button_width(&self.confirm_label, |v| cx.pt(v));
        let cancel_w = Self::button_width(&self.cancel_label, |v| cx.pt(v));
        let confirm_x = bounds.max_x() - pad - confirm_w;
        let cancel_x = confirm_x - cx.pt(BUTTON_GAP) - cancel_w;
        self.button_rects = [
            Rect::new(cancel_x, by, cancel_w, button_h),
            Rect::new(confirm_x, by, confirm_w, button_h),
        ];
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Dialog);
        node.set_label(self.question.as_str());
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if let WidgetEvent::PointerReleased {
            position,
            button: PointerButton::Primary,
        } = cx.event
        {
            if self.button_rects[1].contains(*position) {
                self.resolve(ConfirmResult::Confirm);
                return EventResponse::RequestRepaint;
            }
            if self.button_rects[0].contains(*position) {
                self.resolve(ConfirmResult::Cancel);
                return EventResponse::RequestRepaint;
            }
        }
        match cx.event {
            WidgetEvent::KeyPressed { key, .. } => match key.as_str() {
                // The layer eats Escape before content in arena use —
                // dismissal there maps to Cancel by the owner. This
                // path serves ownerless-embedded use.
                "Escape" => {
                    self.resolve(ConfirmResult::Cancel);
                    EventResponse::Handled
                }
                "Enter" => {
                    self.resolve(ConfirmResult::Confirm);
                    EventResponse::Handled
                }
                _ => EventResponse::Ignored,
            },
            // The bubble owns its surface — inside presses must not
            // leak to the dismissal path.
            WidgetEvent::PointerMoved { .. }
            | WidgetEvent::PointerPressed { .. }
            | WidgetEvent::PointerReleased { .. }
            | WidgetEvent::Scroll { .. } => EventResponse::Handled,
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        let face = kurbo::Rect::new(
            f64::from(b.min_x()),
            f64::from(b.min_y()),
            f64::from(b.max_x()),
            f64::from(b.max_y()),
        );
        // Elevation (spec B4): a `BlurredRect` drop shadow beneath the
        // face plus a 1px `BorderColor` keyline around it. The blurred
        // rect is a *solid-color* shape blur — a drop shadow, not a
        // live backdrop sample — so it is inherently static while the
        // bubble is open and needs no frozen-backdrop caching (nothing
        // beneath the overlay ever feeds the blur).
        cx.list.push_blurred_rect(
            [b.min_x(), b.min_y() + cx.pt(2.0), b.width(), b.height()],
            cx.pt(6.0),
            [0.0, 0.0, 0.0, 0.15],
        );
        let shape = Shape::rounded(cx.dim(TokenKey::BorderRadius, 8.0));
        *self.painted_shape.lock().expect("popup shape poisoned") = shape.clone();
        let surface = cx.color(TokenKey::SurfaceColor, SURFACE);
        let edge = cx.color(TokenKey::BorderColor, EDGE);
        cx.list.push_fill_shape(face, &shape, surface);
        // 1px `BorderColor` keyline — the WCAG 1.4.11 non-text edge: a
        // shadow alone is not a boundary. `BorderColor` is a stroke
        // token held at ≥3:1 against every surface fill — including the
        // page backdrop the bubble floats on — by the theme token
        // tests, and the per-frame paint audit re-checks it at paint
        // scale.
        cx.list.push_stroke_shape(face, &shape, cx.pt(1.0), edge);
        // Arrow tail on the anchor-facing edge (the shared helper).
        let anchor_rect = self
            .shared
            .lock()
            .expect("popconfirm state poisoned")
            .anchor;
        let anchor = kurbo::Rect::new(
            f64::from(anchor_rect.min_x()),
            f64::from(anchor_rect.min_y()),
            f64::from(anchor_rect.max_x()),
            f64::from(anchor_rect.max_y()),
        );
        let side = anchor_facing_edge(b, anchor_rect, self.flow);
        let tip = cx.ptf(f64::from(TAIL_TIP));
        let half = cx.ptf(f64::from(TAIL_HALF));
        cx.list
            .push_path(tail_path(face, anchor, side, tip, half), surface);
        cx.list.push_stroke_path(
            crate::widgets::popover::tail_stroke_path(face, anchor, side, tip, half),
            cx.pt(1.0),
            edge,
        );

        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        // Question — clipped to the strip above the buttons.
        let pad = cx.pt(PAD);
        crate::text_paint::paint_label_clipped(
            painter,
            cx.list,
            kurbo::Rect::new(
                f64::from(b.min_x() + pad),
                f64::from(b.min_y()),
                f64::from(b.max_x() - pad),
                f64::from(b.min_y() + pad + cx.pt(QUESTION_H)),
            ),
            kurbo::Point::new(f64::from(b.min_x() + pad), f64::from(b.min_y() + pad)),
            &self.question,
            cx.pt(13.0),
            cx.color(TokenKey::TextColor, INK),
        );
        // Buttons: [0] = cancel (neutral), [1] = confirm (accent).
        for (r, label, primary) in [
            (&self.button_rects[0], &self.cancel_label, false),
            (&self.button_rects[1], &self.confirm_label, true),
        ] {
            let br = kurbo::Rect::new(
                f64::from(r.min_x()),
                f64::from(r.min_y()),
                f64::from(r.max_x()),
                f64::from(r.max_y()),
            );
            let bshape = Shape::rounded(cx.dim(TokenKey::BorderRadius, 4.0));
            cx.list.push_fill_shape(
                br,
                &bshape,
                if primary {
                    cx.color(TokenKey::AccentColor, ACCENT)
                } else {
                    cx.color(TokenKey::SurfaceColor, SURFACE)
                },
            );
            cx.list.push_stroke_shape(
                br,
                &bshape,
                cx.pt(1.0),
                cx.color(TokenKey::BorderColor, EDGE),
            );
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                kurbo::Rect::new(
                    f64::from(r.min_x() + cx.pt(6.0)),
                    f64::from(r.min_y()),
                    f64::from(r.max_x() - cx.pt(6.0)),
                    f64::from(r.max_y()),
                ),
                kurbo::Point::new(
                    f64::from(r.min_x() + cx.pt(10.0)),
                    f64::from(r.min_y() + (r.height() - cx.pt(12.0)) / 2.0),
                ),
                label,
                cx.pt(12.0),
                if primary {
                    cx.color(TokenKey::TextInverseColor, [255, 255, 255, 255])
                } else {
                    cx.color(TokenKey::TextColor, INK)
                },
            );
        }
    }

    fn clips_children(&self) -> bool {
        true
    }

    fn clip_shape(&self) -> Option<Shape> {
        Some(
            self.painted_shape
                .lock()
                .expect("popup shape poisoned")
                .clone(),
        )
    }

    fn hit_shape(&self) -> Option<Shape> {
        Some(
            self.painted_shape
                .lock()
                .expect("popup shape poisoned")
                .clone(),
        )
    }
}

/// A mini anchored confirmation bubble (Ant `Popconfirm`).
///
/// `Popconfirm` is a zero-size marker: place it in the tree where the
/// bubble should point (its layout bounds are the anchor rect, or set
/// [`Popconfirm::anchor`]). The bubble — the question and a
/// Confirm/Cancel pair — lives in the overlay and is reconciled by
/// [`Popconfirm::sync_overlay`], which the arena calls once per frame
/// before `OverlayLayer::layout_pass`. Light-dismissal (outside press,
/// `Escape`) reports [`ConfirmResult::Cancel`].
///
/// # Examples
///
/// ```
/// use martensite::widgets::Popconfirm;
///
/// let mut c = Popconfirm::new().question("Discard changes?");
/// assert!(!c.is_open());
/// c.open();
/// assert!(c.is_open());
/// ```
pub struct Popconfirm {
    /// The question text.
    pub question: String,
    /// Confirm button label.
    pub confirm_label: String,
    /// Cancel button label.
    pub cancel_label: String,
    /// Preferred placement edge — flipped to the opposite side when
    /// the bubble does not fit.
    pub preferred_edge: AnchorEdge,
    /// Whether the bubble is logically open.
    open: bool,
    /// Overlay entry id of the open bubble.
    popup_id: Option<u64>,
    /// Explicit anchor rect override (window space).
    anchor_override: Option<Rect>,
    /// Marker bounds from the last layout pass — the default anchor.
    cached_bounds: Rect,
    /// The anchor the live entry was last opened/re-anchored with.
    last_anchor: Option<OverlayAnchor>,
    /// State shared with the surface (anchor + result).
    shared: Arc<Mutex<PopconfirmShared>>,
    /// Result since the last `take_result`.
    result: Option<ConfirmResult>,
    /// Shared cell also receiving the result — the overlay-host
    /// observation seam (mirrors `take_result`).
    result_sink: Option<Arc<Mutex<Option<ConfirmResult>>>>,
    /// Shared shaped-text painter — propagated into the surface.
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl Popconfirm {
    /// A popconfirm marker with no question, anchored below by default.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Popconfirm;
    /// use martensite_core::overlay::AnchorEdge;
    ///
    /// let c = Popconfirm::new();
    /// assert_eq!(c.preferred_edge, AnchorEdge::Bottom);
    /// ```
    pub fn new() -> Self {
        Self {
            question: String::new(),
            confirm_label: "OK".to_string(),
            cancel_label: "Cancel".to_string(),
            preferred_edge: AnchorEdge::Bottom,
            open: false,
            popup_id: None,
            anchor_override: None,
            cached_bounds: Rect::default(),
            last_anchor: None,
            shared: Arc::new(Mutex::new(PopconfirmShared::default())),
            result: None,
            result_sink: None,
            text_painter: None,
        }
    }

    /// Sets the question text.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Popconfirm;
    ///
    /// let c = Popconfirm::new().question("Sure?");
    /// assert_eq!(c.question, "Sure?");
    /// ```
    #[must_use]
    pub fn question(mut self, question: impl Into<String>) -> Self {
        self.question = question.into();
        self
    }

    /// Sets the confirm button label (default `"OK"`).
    #[must_use]
    pub fn confirm_label(mut self, label: impl Into<String>) -> Self {
        self.confirm_label = label.into();
        self
    }

    /// Sets the cancel button label (default `"Cancel"`).
    #[must_use]
    pub fn cancel_label(mut self, label: impl Into<String>) -> Self {
        self.cancel_label = label.into();
        self
    }

    /// Sets the preferred placement edge (with flip fallback).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Popconfirm;
    /// use martensite_core::overlay::AnchorEdge;
    ///
    /// let c = Popconfirm::new().preferred_edge(AnchorEdge::Top);
    /// assert_eq!(c.preferred_edge, AnchorEdge::Top);
    /// ```
    #[must_use]
    pub fn preferred_edge(mut self, edge: AnchorEdge) -> Self {
        self.preferred_edge = edge;
        self
    }

    /// Overrides the anchor rect (window space) the bubble points at;
    /// the marker's own layout bounds are used when unset.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Popconfirm;
    /// use martensite_core::Rect;
    ///
    /// let c = Popconfirm::new().anchor(Rect::new(10.0, 10.0, 80.0, 30.0));
    /// ```
    #[must_use]
    pub fn anchor(mut self, rect: Rect) -> Self {
        self.anchor_override = Some(rect);
        self
    }

    /// Shares a [`crate::text_paint::TextPainter`] so the bubble emits
    /// real glyph runs instead of `DrawText` placeholder boxes.
    #[must_use]
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Whether the bubble is logically open.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Popconfirm;
    ///
    /// let mut c = Popconfirm::new();
    /// assert!(!c.is_open());
    /// c.open();
    /// assert!(c.is_open());
    /// ```
    #[inline]
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// The overlay entry id of the open bubble, if any.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Popconfirm;
    ///
    /// let c = Popconfirm::new();
    /// assert_eq!(c.popup_id(), None);
    /// ```
    #[inline]
    pub fn popup_id(&self) -> Option<u64> {
        self.popup_id
    }

    /// Opens the bubble on the next [`sync_overlay`](Self::sync_overlay).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Popconfirm;
    ///
    /// let mut c = Popconfirm::new();
    /// c.open();
    /// assert!(c.is_open());
    /// ```
    pub fn open(&mut self) {
        self.open = true;
        self.shared
            .lock()
            .expect("popconfirm state poisoned")
            .result = None;
    }

    /// Closes the bubble on the next [`sync_overlay`](Self::sync_overlay).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Popconfirm;
    ///
    /// let mut c = Popconfirm::new();
    /// c.open();
    /// c.close();
    /// assert!(!c.is_open());
    /// ```
    pub fn close(&mut self) {
        self.open = false;
    }

    /// Returns the activation result once, if any.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Popconfirm;
    ///
    /// let mut c = Popconfirm::new();
    /// assert_eq!(c.take_result(), None);
    /// ```
    pub fn take_result(&mut self) -> Option<ConfirmResult> {
        self.result.take()
    }

    /// Wires a shared cell that receives the result — the overlay-host
    /// observation seam (mirrors `take_result`).
    ///
    /// # Examples
    ///
    /// ```
    /// use std::sync::{Arc, Mutex};
    /// use martensite::widgets::Popconfirm;
    ///
    /// let sink = Arc::new(Mutex::new(None));
    /// let c = Popconfirm::new().result_sink(sink.clone());
    /// assert!(sink.lock().unwrap().is_none());
    /// ```
    #[must_use]
    pub fn result_sink(mut self, sink: Arc<Mutex<Option<ConfirmResult>>>) -> Self {
        self.result_sink = Some(sink);
        self
    }

    /// The anchor the bubble would open at right now.
    fn current_anchor(&self) -> OverlayAnchor {
        OverlayAnchor::BoundsEdge {
            rect: self.anchor_override.unwrap_or(self.cached_bounds),
            edge: self.preferred_edge,
        }
    }

    /// Records a result — locally and into the observation sink.
    fn set_result(&mut self, result: ConfirmResult) {
        self.result = Some(result);
        if let Some(sink) = &self.result_sink {
            if let Ok(mut cell) = sink.lock() {
                *cell = Some(result);
            }
        }
    }

    /// Reconciles the overlay with the popconfirm's open state.
    ///
    /// Call once per frame before `OverlayLayer::layout_pass`:
    ///
    /// - applies an activation made inside the surface (a button, or
    ///   an embedded `Escape`/`Enter`);
    /// - opens/closes the bubble entry to match
    ///   [`is_open`](Self::is_open);
    /// - notices overlay-level dismissal (outside press, `Escape`) and
    ///   reports [`ConfirmResult::Cancel`];
    /// - re-anchors a live bubble whose anchor rect moved.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Popconfirm;
    /// use martensite_core::overlay::OverlayLayer;
    /// use martensite_core::{HotNode, LayoutContext, Rect, Widget};
    ///
    /// let mut c = Popconfirm::new().question("Sure?");
    /// let mut hot = HotNode::default();
    /// let mut cx = LayoutContext {
    ///     hot: &mut hot,
    ///     scale: 1.0,
    /// };
    /// c.layout(&mut cx, Rect::new(100.0, 100.0, 60.0, 24.0));
    ///
    /// let mut overlay = OverlayLayer::new();
    /// overlay.set_viewport(Rect::new(0.0, 0.0, 800.0, 600.0));
    /// c.open();
    /// c.sync_overlay(&mut overlay);
    /// overlay.layout_pass();
    /// assert_eq!(overlay.len(), 1);
    /// ```
    pub fn sync_overlay(&mut self, overlay: &mut OverlayLayer) {
        // A button inside the surface resolved the confirmation.
        let activated = self
            .shared
            .lock()
            .expect("popconfirm state poisoned")
            .result
            .take();
        if let Some(result) = activated {
            self.set_result(result);
            self.open = false;
        }
        // The layer dismissed our bubble (outside press / Escape) —
        // light-dismiss maps to Cancel.
        if let Some(id) = self.popup_id {
            if !overlay.is_open(id) {
                self.popup_id = None;
                if self.open {
                    self.set_result(ConfirmResult::Cancel);
                }
                self.open = false;
                self.last_anchor = None;
            }
        }
        // Mirror the live anchor into shared state so the surface's
        // tail tracks re-anchoring.
        self.shared
            .lock()
            .expect("popconfirm state poisoned")
            .anchor = self.anchor_override.unwrap_or(self.cached_bounds);
        if self.open && self.popup_id.is_none() {
            let anchor = self.current_anchor();
            let surface = PopconfirmSurface {
                question: self.question.clone(),
                confirm_label: self.confirm_label.clone(),
                cancel_label: self.cancel_label.clone(),
                shared: Arc::clone(&self.shared),
                flow: self.preferred_edge,
                bounds: Rect::default(),
                button_rects: [Rect::default(); 2],
                painted_shape: Mutex::new(Shape::RECT),
                text_painter: self.text_painter.clone(),
            };
            self.popup_id = Some(overlay.open(Box::new(surface), anchor.clone()));
            self.last_anchor = Some(anchor);
        } else if !self.open {
            if let Some(id) = self.popup_id.take() {
                overlay.close(id);
            }
            self.last_anchor = None;
        } else if let Some(id) = self.popup_id {
            // The anchor moved while open — re-anchor so the bubble
            // tracks instead of detaching.
            let anchor = self.current_anchor();
            if self.last_anchor.as_ref() != Some(&anchor) {
                overlay.set_anchor(id, anchor.clone());
                self.last_anchor = Some(anchor);
            }
        }
    }
}

impl Default for Popconfirm {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Popconfirm {
    fn measure(&mut self, _cx: &mut LayoutContext, _constraints: LayoutConstraints) -> Vec2 {
        // A marker — no inline extent; position it inside a sized
        // cell (or set `anchor`) to control the anchor rect.
        Vec2::ZERO
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.cached_bounds = bounds;
        // The marker carries keyboard focus so `Escape`/`Enter` work
        // for ownerless-embedded use.
        cx.hot.flags |= NodeFlags::FOCUSABLE;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::GenericContainer);
        node.set_has_popup(accesskit::HasPopup::Dialog);
        node.set_expanded(self.open);
        if !self.question.is_empty() {
            node.set_label(self.question.as_str());
        }
        node.add_action(accesskit::Action::Click);
        node.add_action(accesskit::Action::Focus);
    }

    fn a11y_fixup(
        &self,
        _emitted: &mut Vec<A11yEmittedNode>,
        overlay_nodes: &[OverlayA11yRef],
        this_node: &mut AccessKitNode,
    ) {
        // aria-controls → the bubble's Dialog root while open.
        let Some(popup) = self.popup_id else {
            return;
        };
        if let Some(bubble_id) = overlay_nodes
            .iter()
            .find(|r| r.entry == popup && r.path.is_empty())
            .map(|r| r.id)
        {
            this_node.set_controls(vec![bubble_id]);
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::KeyPressed { key, .. } => match key.as_str() {
                // The overlay consumes Escape first in arena use —
                // dismissal there maps to Cancel in `sync_overlay`.
                // This path serves ownerless-embedded use.
                "Escape" => {
                    if self.open {
                        self.set_result(ConfirmResult::Cancel);
                        self.close();
                        return EventResponse::RequestRepaint;
                    }
                    EventResponse::Ignored
                }
                "Enter" | " " | "Space" => {
                    if self.open {
                        self.set_result(ConfirmResult::Confirm);
                        self.close();
                    } else {
                        self.open();
                    }
                    EventResponse::RequestRepaint
                }
                _ => EventResponse::Ignored,
            },
            WidgetEvent::SemanticAction(SemanticAction::Click) => {
                if self.open {
                    self.set_result(ConfirmResult::Cancel);
                    self.close();
                } else {
                    self.open();
                }
                EventResponse::RequestRepaint
            }
            WidgetEvent::SemanticAction(SemanticAction::Focus) => EventResponse::CaptureFocus,
            _ => EventResponse::Ignored,
        }
    }

    fn sync_overlay(&mut self, overlay: &mut OverlayLayer) {
        // Delegate to the inherent method so `Popconfirm::sync_overlay`
        // and the `Widget` trait seam stay in lock-step.
        Popconfirm::sync_overlay(self, overlay);
    }
}

impl std::fmt::Debug for Popconfirm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Popconfirm")
            .field("question", &self.question)
            .field("preferred_edge", &self.preferred_edge)
            .field("open", &self.open)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::{HotNode, PaintCommand, PaintList, Theme};

    fn laid_out(c: &mut Popconfirm, bounds: Rect) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        c.layout(&mut cx, bounds);
    }

    fn overlay() -> OverlayLayer {
        let mut o = OverlayLayer::new();
        o.set_viewport(Rect::new(0.0, 0.0, 800.0, 600.0));
        o
    }

    fn open(c: &mut Popconfirm, o: &mut OverlayLayer) -> u64 {
        c.open();
        c.sync_overlay(o);
        o.layout_pass();
        c.popup_id().unwrap()
    }

    fn press_on(o: &mut OverlayLayer, id: u64, rect: Rect) {
        let surface = o.widget_at_mut(id, &[]).expect("surface");
        let release = WidgetEvent::PointerReleased {
            position: Vec2::new(rect.min_x() + 4.0, rect.min_y() + 4.0),
            button: PointerButton::Primary,
        };
        let mut cx = EventContext {
            event: &release,
            bounds: rect,
            scale: 1.0,
        };
        surface.event(&mut cx);
    }

    #[test]
    fn sync_overlay_opens_bounds_edge() {
        let mut c = Popconfirm::new().question("Sure?");
        laid_out(&mut c, Rect::new(100.0, 100.0, 60.0, 24.0));
        let mut o = overlay();
        let id = open(&mut c, &mut o);
        assert_eq!(
            o.entry(id).unwrap().anchor(),
            &OverlayAnchor::BoundsEdge {
                rect: Rect::new(100.0, 100.0, 60.0, 24.0),
                edge: AnchorEdge::Bottom,
            }
        );
        let b = o.entry_bounds(id).unwrap();
        assert!(b.min_y() >= 124.0);
    }

    #[test]
    fn confirm_button_reports_confirm() {
        let mut c = Popconfirm::new().question("Sure?");
        laid_out(&mut c, Rect::new(100.0, 100.0, 60.0, 24.0));
        let mut o = overlay();
        let id = open(&mut c, &mut o);
        // Reach into the surface: button_rects[1] is confirm — lay it
        // out via the overlay's layout pass then press it.
        let b = o.entry_bounds(id).unwrap();
        // Confirm is the rightmost 56px-wide-min button at the bottom.
        let confirm = Rect::new(b.max_x() - 10.0 - 40.0, b.max_y() - 34.0, 40.0, 24.0);
        press_on(&mut o, id, confirm);
        c.sync_overlay(&mut o);
        assert_eq!(c.take_result(), Some(ConfirmResult::Confirm));
        assert!(!c.is_open());
        assert_eq!(o.len(), 0);
    }

    #[test]
    fn cancel_button_reports_cancel() {
        let mut c = Popconfirm::new().question("Sure?");
        laid_out(&mut c, Rect::new(100.0, 100.0, 60.0, 24.0));
        let mut o = overlay();
        let id = open(&mut c, &mut o);
        let b = o.entry_bounds(id).unwrap();
        // Cancel sits just left of the confirm button.
        let cancel = Rect::new(
            b.max_x() - 10.0 - 56.0 - 8.0 - 40.0,
            b.max_y() - 34.0,
            40.0,
            24.0,
        );
        press_on(&mut o, id, cancel);
        c.sync_overlay(&mut o);
        assert_eq!(c.take_result(), Some(ConfirmResult::Cancel));
        assert!(!c.is_open());
    }

    #[test]
    fn light_dismiss_reports_cancel() {
        let mut c = Popconfirm::new().question("Sure?");
        laid_out(&mut c, Rect::new(100.0, 100.0, 60.0, 24.0));
        let mut o = overlay();
        open(&mut c, &mut o);
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(700.0, 500.0),
            button: PointerButton::Primary,
            count: 1,
        };
        assert_eq!(o.dispatch_event(&press), EventResponse::Ignored);
        c.sync_overlay(&mut o);
        assert!(!c.is_open());
        assert_eq!(c.take_result(), Some(ConfirmResult::Cancel));
    }

    #[test]
    fn escape_dismissal_reports_cancel() {
        let mut c = Popconfirm::new().question("Sure?");
        laid_out(&mut c, Rect::new(100.0, 100.0, 60.0, 24.0));
        let mut o = overlay();
        open(&mut c, &mut o);
        let esc = WidgetEvent::KeyPressed {
            key: "Escape".to_string(),
            repeat: false,
        };
        // The layer consumes Escape and dismisses the topmost entry.
        assert_eq!(o.dispatch_event(&esc), EventResponse::Handled);
        c.sync_overlay(&mut o);
        assert!(!c.is_open());
        assert_eq!(c.take_result(), Some(ConfirmResult::Cancel));
    }

    #[test]
    fn result_sink_observes_activation() {
        let sink = Arc::new(Mutex::new(None));
        let mut c = Popconfirm::new()
            .question("Sure?")
            .result_sink(Arc::clone(&sink));
        laid_out(&mut c, Rect::new(100.0, 100.0, 60.0, 24.0));
        let mut o = overlay();
        open(&mut c, &mut o);
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(700.0, 500.0),
            button: PointerButton::Primary,
            count: 1,
        };
        o.dispatch_event(&press);
        c.sync_overlay(&mut o);
        assert_eq!(*sink.lock().unwrap(), Some(ConfirmResult::Cancel));
    }

    #[test]
    fn marker_accessibility() {
        let mut c = Popconfirm::new().question("Sure?");
        c.open();
        let mut node = AccessKitNode::new(accesskit::Role::Unknown);
        c.accessibility(&mut node);
        assert_eq!(node.has_popup(), Some(accesskit::HasPopup::Dialog));
        assert_eq!(node.is_expanded(), Some(true));
    }

    #[test]
    fn surface_emits_dialog_role() {
        let shared = Arc::new(Mutex::new(PopconfirmShared::default()));
        let surface = PopconfirmSurface {
            question: "Sure?".to_string(),
            confirm_label: "OK".to_string(),
            cancel_label: "Cancel".to_string(),
            shared,
            flow: AnchorEdge::Bottom,
            bounds: Rect::default(),
            button_rects: [Rect::default(); 2],
            painted_shape: Mutex::new(Shape::RECT),
            text_painter: None,
        };
        let mut node = AccessKitNode::new(accesskit::Role::Unknown);
        surface.accessibility(&mut node);
        assert_eq!(node.role(), accesskit::Role::Dialog);
        assert_eq!(node.label(), Some("Sure?"));
    }

    #[test]
    fn surface_paints_shadow_then_border_keyline() {
        // Spec B4: an elevated overlay pairs its `BlurredRect` drop
        // shadow with a 1px `BorderColor` keyline — the shadow alone
        // is not a WCAG 1.4.11 edge — and the tail keeps the keyline
        // continuous around the silhouette.
        let surface = PopconfirmSurface {
            question: "Sure?".to_string(),
            confirm_label: "OK".to_string(),
            cancel_label: "Cancel".to_string(),
            shared: Arc::new(Mutex::new(PopconfirmShared::default())),
            flow: AnchorEdge::Bottom,
            bounds: Rect::new(80.0, 130.0, 140.0, 80.0),
            button_rects: [Rect::default(); 2],
            painted_shape: Mutex::new(Shape::RECT),
            text_painter: None,
        };
        let mut list = PaintList::new();
        let theme = Theme::new("test");
        surface.paint(&mut PaintContext {
            list: &mut list,
            bounds: Rect::new(80.0, 130.0, 140.0, 80.0),
            theme: &theme,
            scale: 1.0,
            text_painter: None,
        });
        // Shadow first, then face fill + face keyline, then tail fill
        // + tail slant strokes (rounded faces emit paths, not rects).
        assert!(matches!(list.commands[0], PaintCommand::BlurredRect { .. }));
        assert!(matches!(list.commands[1], PaintCommand::FillPath(..)));
        assert!(matches!(
            list.commands[2],
            PaintCommand::StrokePath(_, 1.0, EDGE)
        ));
        assert!(matches!(list.commands[3], PaintCommand::FillPath(..)));
        assert!(matches!(
            list.commands[4],
            PaintCommand::StrokePath(_, 1.0, EDGE)
        ));
    }
}
