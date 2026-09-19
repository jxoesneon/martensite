//! `Popover` widget: an anchored bubble with an arrow tail
//! (GtkPopover / NSPopover).
//!
//! The `Popover` itself is a zero-size marker placed in the widget
//! tree where the bubble should point — its layout bounds are the
//! anchor rect (or [`Popover::anchor`] overrides it). Calling
//! [`Popover::open`] reconciles a [`PopoverSurface`] into the
//! [`OverlayLayer`](martensite_core::overlay::OverlayLayer) at
//! `OverlayAnchor::BoundsEdge { rect, edge }` on the next
//! [`Popover::sync_overlay`]: placed on [`Popover::preferred_edge`]
//! when it fits, flipped to the opposite edge when it does not, and
//! clamped into the viewport.
//!
//! - **Dismissal** (the light-dismiss contract, like `Dropdown`): an
//!   outside press or `Escape` closes the bubble; presses inside it
//!   are consumed.
//! - **`autohide(false)`** (GtkPopover semantics): the entry opens
//!   with [`OverlayOptions::passthrough`] so outside presses fall
//!   through to the content beneath without closing the bubble.
//!   Passthrough entries are never the layer's `Escape` target, so the
//!   surface handles `Escape` itself — `Escape` and explicit
//!   [`Popover::close`] still dismiss it.
//! - **Arrow tail**: a small filled triangle on the bubble edge facing
//!   the anchor. The `BoundsEdge` flip isn't observable from the
//!   surface, so the tail side is resolved geometrically — comparing
//!   the popup rect's centre against the anchor rect's centre on the
//!   requested edge's flow axis — and therefore always points back at
//!   the anchor even after a flip.
//! - **Content child**: [`Popover::child`] moves into the surface for
//!   the lifetime of the popup and is handed back to the marker when
//!   the entry closes (via a shared reclaim slot filled by the
//!   surface's `Drop`), so close/re-open cycles keep the same child.
//!
//! The marker emits `Role::GenericContainer` with
//! `aria-haspopup="dialog"` / `aria-expanded`; the surface emits
//! `Role::Dialog` — announced when the overlay emits the popup on
//! open.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::{Popover, Text};
//! use martensite_core::overlay::AnchorEdge;
//!
//! let mut p = Popover::new()
//!     .title("Details")
//!     .child(Text::new("content"))
//!     .preferred_edge(AnchorEdge::Bottom);
//! p.open();
//! assert!(p.is_open());
//! ```

use std::sync::{Arc, Mutex};

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::overlay::{AnchorEdge, OverlayAnchor, OverlayLayer, OverlayOptions};
use martensite_core::shape::Shape;
use martensite_core::widget::{
    A11yEmittedNode, EventContext, EventResponse, LayoutConstraints, LayoutContext, OverlayA11yRef,
    PaintContext, SemanticAction, Widget, WidgetEvent,
};
use martensite_core::{NodeFlags, Rect, TokenKey};

/// Bubble chrome (logical points).
const PAD: f32 = 12.0;
/// Title strip height (logical points).
const TITLE_H: f32 = 24.0;
/// Minimum bubble width (logical points).
const MIN_W: f32 = 120.0;
/// Tail geometry: apex distance beyond the face edge / base half-width
/// (logical points).
const TAIL_TIP: f32 = 7.0;
/// Tail base half-width (logical points).
const TAIL_HALF: f32 = 7.0;
/// Bubble background.
const SURFACE: [u8; 4] = [252, 252, 254, 255];
/// Bubble border.
const EDGE: [u8; 4] = [140, 145, 155, 255];
/// Title ink.
const INK: [u8; 4] = [20, 20, 25, 255];

/// Which edge of a placed `popup` rect faces its `anchor` along
/// `flow`'s axis — the side the arrow tail belongs on.
///
/// `flow` is the *requested* [`AnchorEdge`] (`Top`/`Bottom` flow
/// vertically, `Left`/`Right` horizontally); comparing rect centres
/// absorbs the `BoundsEdge` flip for free: a `Bottom`-anchored popup
/// flipped above its anchor has the anchor below it, so the tail
/// resolves to the bottom edge.
///
/// `pub(crate)` — shared with `Popconfirm`'s surface.
pub(crate) fn anchor_facing_edge(popup: Rect, anchor: Rect, flow: AnchorEdge) -> AnchorEdge {
    let pc = popup.origin + popup.size / 2.0;
    let ac = anchor.origin + anchor.size / 2.0;
    match flow {
        AnchorEdge::Top | AnchorEdge::Bottom => {
            if ac.y > pc.y {
                AnchorEdge::Bottom
            } else {
                AnchorEdge::Top
            }
        }
        AnchorEdge::Left | AnchorEdge::Right => {
            if ac.x > pc.x {
                AnchorEdge::Right
            } else {
                AnchorEdge::Left
            }
        }
    }
}

/// The arrow-tail triangle for a bubble face: base `2*half` wide on
/// `side`'s edge of `face`, apex `tip` px outward, centred on
/// `anchor`'s cross-axis centre clamped to the edge (so it still
/// points at the anchor when the popup was clamped off-centre).
///
/// `pub(crate)` — shared with `Popconfirm`'s surface.
pub(crate) fn tail_path(
    face: kurbo::Rect,
    anchor: kurbo::Rect,
    side: AnchorEdge,
    tip: f64,
    half: f64,
) -> kurbo::BezPath {
    let anchor_cx = anchor.x0 + anchor.width() / 2.0;
    let anchor_cy = anchor.y0 + anchor.height() / 2.0;
    let (a, b, c) = match side {
        AnchorEdge::Top => {
            let cx = anchor_cx.clamp(face.x0 + half, face.x1 - half);
            (
                kurbo::Point::new(cx - half, face.y0),
                kurbo::Point::new(cx + half, face.y0),
                kurbo::Point::new(cx, face.y0 - tip),
            )
        }
        AnchorEdge::Bottom => {
            let cx = anchor_cx.clamp(face.x0 + half, face.x1 - half);
            (
                kurbo::Point::new(cx - half, face.y1),
                kurbo::Point::new(cx + half, face.y1),
                kurbo::Point::new(cx, face.y1 + tip),
            )
        }
        AnchorEdge::Left => {
            let cy = anchor_cy.clamp(face.y0 + half, face.y1 - half);
            (
                kurbo::Point::new(face.x0, cy - half),
                kurbo::Point::new(face.x0, cy + half),
                kurbo::Point::new(face.x0 - tip, cy),
            )
        }
        AnchorEdge::Right => {
            let cy = anchor_cy.clamp(face.y0 + half, face.y1 - half);
            (
                kurbo::Point::new(face.x1, cy - half),
                kurbo::Point::new(face.x1, cy + half),
                kurbo::Point::new(face.x1 + tip, cy),
            )
        }
    };
    kurbo::BezPath::from_vec(vec![
        kurbo::PathEl::MoveTo(a),
        kurbo::PathEl::LineTo(b),
        kurbo::PathEl::LineTo(c),
        kurbo::PathEl::ClosePath,
    ])
}

/// The two slant edges of the tail triangle — stroked separately so
/// the face border reads as continuous where the tail attaches (the
/// base edge is covered by the tail's own fill).
///
/// `pub(crate)` — shared with `Popconfirm`'s surface.
pub(crate) fn tail_stroke_path(
    face: kurbo::Rect,
    anchor: kurbo::Rect,
    side: AnchorEdge,
    tip: f64,
    half: f64,
) -> kurbo::BezPath {
    let path = tail_path(face, anchor, side, tip, half);
    // tail_path emits base-a, base-b, apex — re-stitch as a→apex→b.
    let els: Vec<kurbo::PathEl> = path.elements().to_vec();
    if els.len() == 4 {
        if let (
            kurbo::PathEl::MoveTo(a),
            kurbo::PathEl::LineTo(b),
            kurbo::PathEl::LineTo(c),
            kurbo::PathEl::ClosePath,
        ) = (els[0], els[1], els[2], els[3])
        {
            return kurbo::BezPath::from_vec(vec![
                kurbo::PathEl::MoveTo(a),
                kurbo::PathEl::LineTo(c),
                kurbo::PathEl::LineTo(b),
            ]);
        }
    }
    path
}

/// State shared between a [`Popover`] marker and its live
/// [`PopoverSurface`]: the anchor rect (owner → surface, so the tail
/// tracks re-anchoring) and the surface's close request (surface →
/// owner, the `Escape` path for `autohide(false)` passthrough
/// entries).
#[derive(Debug, Default)]
struct PopoverShared {
    /// The anchor rect the tail points at (window space).
    anchor: Rect,
    /// The surface asked to be closed (Escape on a passthrough entry).
    close_requested: bool,
}

/// The popup surface for a [`Popover`] — a `Role::Dialog` bubble with
/// a title strip, a content child, and an arrow tail aimed at the
/// anchor rect.
struct PopoverSurface {
    /// Title shown at the top of the bubble (empty = no strip).
    title: String,
    /// The content child — reclaimed by the owner on `Drop`.
    content: Option<Box<dyn Widget>>,
    /// Shared state with the owning `Popover`.
    shared: Arc<Mutex<PopoverShared>>,
    /// Slot the content child is returned to when this surface is
    /// dropped — the owner drains it in `sync_overlay`, surviving even
    /// layer-initiated dismissal where the entry is simply removed.
    reclaim: Arc<Mutex<Option<Box<dyn Widget>>>>,
    /// The requested placement edge — the tail's flow axis.
    flow: AnchorEdge,
    /// Surface bounds from the last layout pass.
    bounds: Rect,
    /// Content rect inside the bubble.
    content_rect: Rect,
    /// The silhouette painted last frame — the single source of truth
    /// for `clip_shape`/`hit_shape` (mirrors `ListBoxPopup`).
    painted_shape: Mutex<Shape>,
    /// Shared shaped-text painter from the owning `Popover`.
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl Drop for PopoverSurface {
    fn drop(&mut self) {
        // Hand the content child back to the owner — close,
        // light-dismissal, and `clear` all route through here.
        if let Ok(mut slot) = self.reclaim.lock() {
            *slot = self.content.take();
        }
    }
}

impl Widget for PopoverSurface {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let pad = cx.pt(PAD);
        let title_h = if self.title.is_empty() {
            0.0
        } else {
            cx.pt(TITLE_H)
        };
        let content = if let Some(child) = &mut self.content {
            child.measure(
                cx,
                LayoutConstraints {
                    min_size: Vec2::ZERO,
                    max_size: Vec2::new(
                        (constraints.max_size.x - pad * 2.0).max(0.0),
                        (constraints.max_size.y - pad * 2.0 - title_h).max(0.0),
                    ),
                },
            )
        } else {
            Vec2::ZERO
        };
        let title_w = self.title.chars().count() as f32 * cx.pt(8.0);
        let w = (content.x.max(title_w) + pad * 2.0).max(cx.pt(MIN_W));
        Vec2::new(
            w.min(constraints.max_size.x.max(0.0)),
            (pad + title_h + content.y + pad).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        cx.hot.flags |= NodeFlags::FOCUSABLE;
        let pad = cx.pt(PAD);
        let title_h = if self.title.is_empty() {
            0.0
        } else {
            cx.pt(TITLE_H)
        };
        self.content_rect = Rect::new(
            bounds.min_x() + pad,
            bounds.min_y() + pad + title_h,
            (bounds.width() - pad * 2.0).max(0.0),
            (bounds.height() - pad * 2.0 - title_h).max(0.0),
        );
        if let Some(child) = &mut self.content {
            cx.layout_child(child.as_mut(), self.content_rect);
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Dialog);
        if !self.title.is_empty() {
            node.set_label(self.title.as_str());
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        // Content child first — it owns its rect.
        if let Some(pos) = cx.event.position() {
            if let Some(child) = &mut self.content {
                if self.content_rect.contains(pos) {
                    let r = child.event(cx);
                    if r != EventResponse::Ignored {
                        return r;
                    }
                }
            }
        }
        match cx.event {
            WidgetEvent::KeyPressed { key, .. } if key == "Escape" => {
                // The layer eats Escape before content for regular
                // entries — this path serves `autohide(false)`
                // passthrough entries (Escape skips them at the
                // layer) and ownerless-embedded use.
                self.shared
                    .lock()
                    .expect("popover state poisoned")
                    .close_requested = true;
                EventResponse::Handled
            }
            // The bubble owns its surface — inside presses must not
            // leak to the dismissal path (or through a passthrough
            // entry to the content beneath).
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
        // Subtle offset shadow beneath the elevated surface.
        cx.list.push_blurred_rect(
            [b.min_x(), b.min_y() + cx.pt(2.0), b.width(), b.height()],
            cx.pt(8.0),
            [0.0, 0.0, 0.0, 0.18],
        );
        let shape = Shape::rounded(cx.dim(TokenKey::BorderRadiusLarge, 10.0));
        *self.painted_shape.lock().expect("popup shape poisoned") = shape.clone();
        let surface = cx.color(TokenKey::SurfaceColor, SURFACE);
        let edge = cx.color(TokenKey::BorderColor, EDGE);
        cx.list.push_fill_shape(face, &shape, surface);
        cx.list.push_stroke_shape(face, &shape, cx.pt(1.0), edge);
        // Arrow tail on the anchor-facing edge — resolved from the
        // placed rect vs the anchor rect so it follows the BoundsEdge
        // flip automatically. Fill covers the border segment where it
        // attaches; the slants re-stroke the outline.
        let anchor_rect = self.shared.lock().expect("popover state poisoned").anchor;
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
            tail_stroke_path(face, anchor, side, tip, half),
            cx.pt(1.0),
            edge,
        );

        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        if !self.title.is_empty() {
            // Clip the title to the strip — an over-long title can't
            // spill past the rounded chrome.
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                kurbo::Rect::new(
                    f64::from(b.min_x() + cx.pt(PAD)),
                    f64::from(b.min_y()),
                    f64::from(b.max_x() - cx.pt(PAD)),
                    f64::from(b.min_y() + cx.pt(PAD) + cx.pt(TITLE_H)),
                ),
                kurbo::Point::new(
                    f64::from(b.min_x() + cx.pt(PAD)),
                    f64::from(b.min_y() + cx.pt(PAD)),
                ),
                &self.title,
                cx.pt(14.0),
                cx.color(TokenKey::TextColor, INK),
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

    fn child_count(&self) -> usize {
        usize::from(self.content.is_some())
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        if index == 0 {
            self.content.as_deref()
        } else {
            None
        }
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        if index == 0 {
            self.content.as_deref_mut()
        } else {
            None
        }
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        if index == 0 && self.content.is_some() {
            Some(self.content_rect)
        } else {
            None
        }
    }
}

/// An anchored bubble with an arrow tail (GtkPopover / NSPopover).
///
/// `Popover` is a zero-size marker: place it in the tree where the
/// bubble should point (its layout bounds are the anchor rect — put it
/// in a `Stack` cell over the trigger, or override with
/// [`Popover::anchor`]). The bubble — an optional title plus the
/// single content child — lives in the overlay and is reconciled by
/// [`Popover::sync_overlay`], which the arena calls once per frame
/// before `OverlayLayer::layout_pass`.
///
/// # Examples
///
/// ```
/// use martensite::widgets::{Popover, Text};
///
/// let mut p = Popover::new().title("Info").child(Text::new("body"));
/// assert!(!p.is_open());
/// p.open();
/// assert!(p.is_open());
/// ```
pub struct Popover {
    /// Optional title at the top of the bubble.
    pub title: String,
    /// Preferred placement edge — flipped to the opposite side when
    /// the bubble does not fit.
    pub preferred_edge: AnchorEdge,
    /// GtkPopover `autohide`: when `true` (default) an outside press
    /// light-dismisses the bubble; when `false` outside presses fall
    /// through to the content beneath and only `Escape` / explicit
    /// [`Popover::close`] dismiss it.
    pub autohide: bool,
    /// The content child — parked here while closed, living in the
    /// surface while open.
    content: Option<Box<dyn Widget>>,
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
    /// State shared with the surface (anchor + close request).
    shared: Arc<Mutex<PopoverShared>>,
    /// Reclaim slot the surface's `Drop` returns the content child to.
    reclaim: Arc<Mutex<Option<Box<dyn Widget>>>>,
    /// Shared shaped-text painter — propagated into the surface.
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl Popover {
    /// A popover marker with no title, anchored below by default.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Popover;
    /// use martensite_core::overlay::AnchorEdge;
    ///
    /// let p = Popover::new();
    /// assert_eq!(p.preferred_edge, AnchorEdge::Bottom);
    /// assert!(p.autohide);
    /// ```
    pub fn new() -> Self {
        Self {
            title: String::new(),
            preferred_edge: AnchorEdge::Bottom,
            autohide: true,
            content: None,
            open: false,
            popup_id: None,
            anchor_override: None,
            cached_bounds: Rect::default(),
            last_anchor: None,
            shared: Arc::new(Mutex::new(PopoverShared::default())),
            reclaim: Arc::new(Mutex::new(None)),
            text_painter: None,
        }
    }

    /// Sets the bubble title.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Popover;
    ///
    /// let p = Popover::new().title("Filters");
    /// assert_eq!(p.title, "Filters");
    /// ```
    #[must_use]
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    /// Sets the single content child hosted inside the bubble.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Popover, Text};
    ///
    /// let p = Popover::new().child(Text::new("hello"));
    /// ```
    #[must_use]
    pub fn child(mut self, child: impl Widget + 'static) -> Self {
        self.content = Some(Box::new(child));
        self
    }

    /// Sets the preferred placement edge (with flip fallback).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Popover;
    /// use martensite_core::overlay::AnchorEdge;
    ///
    /// let p = Popover::new().preferred_edge(AnchorEdge::Right);
    /// assert_eq!(p.preferred_edge, AnchorEdge::Right);
    /// ```
    #[must_use]
    pub fn preferred_edge(mut self, edge: AnchorEdge) -> Self {
        self.preferred_edge = edge;
        self
    }

    /// Sets `autohide` — see [`Popover::autohide`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Popover;
    ///
    /// let p = Popover::new().autohide(false);
    /// assert!(!p.autohide);
    /// ```
    #[must_use]
    pub fn autohide(mut self, autohide: bool) -> Self {
        self.autohide = autohide;
        self
    }

    /// Overrides the anchor rect (window space) the bubble points at;
    /// the marker's own layout bounds are used when unset.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Popover;
    /// use martensite_core::Rect;
    ///
    /// let p = Popover::new().anchor(Rect::new(10.0, 10.0, 80.0, 30.0));
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
    /// use martensite::widgets::Popover;
    ///
    /// let mut p = Popover::new();
    /// assert!(!p.is_open());
    /// p.open();
    /// assert!(p.is_open());
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
    /// use martensite::widgets::Popover;
    ///
    /// let p = Popover::new();
    /// assert_eq!(p.popup_id(), None);
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
    /// use martensite::widgets::Popover;
    ///
    /// let mut p = Popover::new();
    /// p.open();
    /// assert!(p.is_open());
    /// ```
    pub fn open(&mut self) {
        self.open = true;
        self.shared
            .lock()
            .expect("popover state poisoned")
            .close_requested = false;
    }

    /// Closes the bubble on the next [`sync_overlay`](Self::sync_overlay).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Popover;
    ///
    /// let mut p = Popover::new();
    /// p.open();
    /// p.close();
    /// assert!(!p.is_open());
    /// ```
    pub fn close(&mut self) {
        self.open = false;
    }

    /// The anchor the bubble would open at right now — the explicit
    /// override when set, the marker's layout bounds otherwise.
    fn current_anchor(&self) -> OverlayAnchor {
        OverlayAnchor::BoundsEdge {
            rect: self.anchor_override.unwrap_or(self.cached_bounds),
            edge: self.preferred_edge,
        }
    }

    /// The anchor rect in window space.
    fn anchor_rect(&self) -> Rect {
        self.anchor_override.unwrap_or(self.cached_bounds)
    }

    /// Drains the reclaim slot — the content child a dropped surface
    /// handed back.
    fn drain_reclaim(&mut self) {
        if self.content.is_none() {
            if let Some(child) = self
                .reclaim
                .lock()
                .expect("popover reclaim poisoned")
                .take()
            {
                self.content = Some(child);
            }
        }
    }

    /// Reconciles the overlay with the popover's open state.
    ///
    /// Call once per frame before `OverlayLayer::layout_pass`:
    ///
    /// - applies a close request from the surface (`Escape` on an
    ///   `autohide(false)` entry);
    /// - opens/closes the bubble entry to match [`is_open`](Self::is_open);
    /// - notices overlay-level dismissal (outside press, `Escape` on a
    ///   regular entry) and resets `open`/`popup_id`;
    /// - re-anchors a live bubble whose anchor rect moved.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Popover;
    /// use martensite_core::overlay::OverlayLayer;
    /// use martensite_core::{HotNode, LayoutContext, Rect, Widget};
    ///
    /// let mut p = Popover::new();
    /// let mut hot = HotNode::default();
    /// let mut cx = LayoutContext {
    ///     hot: &mut hot,
    ///     scale: 1.0,
    /// };
    /// p.layout(&mut cx, Rect::new(100.0, 100.0, 60.0, 24.0));
    ///
    /// let mut overlay = OverlayLayer::new();
    /// overlay.set_viewport(Rect::new(0.0, 0.0, 800.0, 600.0));
    /// p.open();
    /// p.sync_overlay(&mut overlay);
    /// overlay.layout_pass();
    /// assert_eq!(overlay.len(), 1);
    /// ```
    pub fn sync_overlay(&mut self, overlay: &mut OverlayLayer) {
        self.drain_reclaim();
        // The surface asked to close (Escape on a passthrough entry).
        if self
            .shared
            .lock()
            .expect("popover state poisoned")
            .close_requested
        {
            self.open = false;
            self.shared
                .lock()
                .expect("popover state poisoned")
                .close_requested = false;
        }
        // The layer dismissed our bubble (outside press / Escape).
        if let Some(id) = self.popup_id {
            if !overlay.is_open(id) {
                self.popup_id = None;
                self.open = false;
                self.last_anchor = None;
            }
        }
        // Mirror the live anchor into shared state so the surface's
        // tail tracks re-anchoring.
        self.shared.lock().expect("popover state poisoned").anchor = self.anchor_rect();
        if self.open && self.popup_id.is_none() {
            let anchor = self.current_anchor();
            let surface = PopoverSurface {
                title: self.title.clone(),
                content: self.content.take(),
                shared: Arc::clone(&self.shared),
                reclaim: Arc::clone(&self.reclaim),
                flow: self.preferred_edge,
                bounds: Rect::default(),
                content_rect: Rect::default(),
                painted_shape: Mutex::new(Shape::RECT),
                text_painter: self.text_painter.clone(),
            };
            let options = if self.autohide {
                OverlayOptions::default()
            } else {
                // autohide(false): outside presses skip this entry and
                // reach the content beneath (GtkPopover semantics);
                // Escape skips it too, so the surface handles Escape
                // itself through `close_requested`.
                OverlayOptions::passthrough()
            };
            self.popup_id = Some(overlay.open_with(Box::new(surface), anchor.clone(), options));
            self.last_anchor = Some(anchor);
        } else if !self.open {
            if let Some(id) = self.popup_id.take() {
                overlay.close(id);
            }
            self.last_anchor = None;
            // `close` dropped the surface — the child came home.
            self.drain_reclaim();
        } else if let Some(id) = self.popup_id {
            // The anchor moved while open (resize, relayout) —
            // re-anchor so the bubble tracks instead of detaching.
            let anchor = self.current_anchor();
            if self.last_anchor.as_ref() != Some(&anchor) {
                overlay.set_anchor(id, anchor.clone());
                self.last_anchor = Some(anchor);
            }
        }
    }
}

impl Default for Popover {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Popover {
    fn measure(&mut self, _cx: &mut LayoutContext, _constraints: LayoutConstraints) -> Vec2 {
        // A marker — no inline extent; position it inside a sized
        // cell (or set `anchor`) to control the anchor rect.
        Vec2::ZERO
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.cached_bounds = bounds;
        // The marker carries keyboard focus so `Escape`/`Enter` work
        // for ownerless-embedded use; the bubble handles its own keys
        // while hosted in the overlay.
        cx.hot.flags |= NodeFlags::FOCUSABLE;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::GenericContainer);
        node.set_has_popup(accesskit::HasPopup::Dialog);
        node.set_expanded(self.open);
        if !self.title.is_empty() {
            node.set_label(self.title.as_str());
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
                // The overlay consumes Escape first in arena use; this
                // is the ownerless-embedded fallback.
                "Escape" => {
                    if self.open {
                        self.close();
                        return EventResponse::RequestRepaint;
                    }
                    EventResponse::Ignored
                }
                "Enter" | " " | "Space" => {
                    if self.open {
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
        // Delegate to the inherent method so `Popover::sync_overlay`
        // and the `Widget` trait seam stay in lock-step.
        Popover::sync_overlay(self, overlay);
    }

    fn child_count(&self) -> usize {
        usize::from(self.content.is_some())
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        if index == 0 {
            self.content.as_deref()
        } else {
            None
        }
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        if index == 0 {
            self.content.as_deref_mut()
        } else {
            None
        }
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        if index == 0 && self.content.is_some() {
            Some(self.cached_bounds)
        } else {
            None
        }
    }
}

impl std::fmt::Debug for Popover {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Popover")
            .field("title", &self.title)
            .field("preferred_edge", &self.preferred_edge)
            .field("open", &self.open)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::Text;
    use martensite_core::{HotNode, PointerButton};

    fn laid_out(p: &mut Popover, bounds: Rect) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        p.layout(&mut cx, bounds);
    }

    fn overlay() -> OverlayLayer {
        let mut o = OverlayLayer::new();
        o.set_viewport(Rect::new(0.0, 0.0, 800.0, 600.0));
        o
    }

    #[test]
    fn facing_edge_tracks_anchor_side() {
        let anchor = Rect::new(100.0, 100.0, 40.0, 20.0);
        // Popup below the anchor → tail on the top edge.
        let below = Rect::new(80.0, 130.0, 120.0, 60.0);
        assert_eq!(
            anchor_facing_edge(below, anchor, AnchorEdge::Bottom),
            AnchorEdge::Top
        );
        // Flipped above → tail on the bottom edge.
        let above = Rect::new(80.0, 20.0, 120.0, 60.0);
        assert_eq!(
            anchor_facing_edge(above, anchor, AnchorEdge::Bottom),
            AnchorEdge::Bottom
        );
        // Right of the anchor → tail on the left edge.
        let right = Rect::new(160.0, 80.0, 100.0, 60.0);
        assert_eq!(
            anchor_facing_edge(right, anchor, AnchorEdge::Right),
            AnchorEdge::Left
        );
        // Flipped left → tail on the right edge.
        let left = Rect::new(0.0, 80.0, 90.0, 60.0);
        assert_eq!(
            anchor_facing_edge(left, anchor, AnchorEdge::Right),
            AnchorEdge::Right
        );
    }

    #[test]
    fn tail_points_at_anchor() {
        let face = kurbo::Rect::new(80.0, 130.0, 200.0, 190.0);
        // kurbo coords are (x0, y0, x1, y1) — a 40×20 anchor at 100,100.
        let anchor = kurbo::Rect::new(100.0, 100.0, 140.0, 120.0);
        let path = tail_path(face, anchor, AnchorEdge::Top, 7.0, 7.0);
        let els = path.elements();
        // Triangle: base on the top edge, apex above it, centred on
        // the anchor's horizontal centre (120).
        assert_eq!(els.len(), 4);
        if let kurbo::PathEl::MoveTo(p) = els[0] {
            assert_eq!(p.y, 130.0);
            assert_eq!(p.x, 113.0);
        } else {
            panic!("expected MoveTo");
        }
        if let kurbo::PathEl::LineTo(p) = els[2] {
            assert_eq!((p.x, p.y), (120.0, 123.0));
        } else {
            panic!("expected LineTo apex");
        }
    }

    #[test]
    fn sync_overlay_opens_bounds_edge() {
        let mut p = Popover::new().preferred_edge(AnchorEdge::Bottom);
        laid_out(&mut p, Rect::new(100.0, 100.0, 60.0, 24.0));
        let mut o = overlay();
        p.open();
        p.sync_overlay(&mut o);
        o.layout_pass();
        assert_eq!(o.len(), 1);
        let id = p.popup_id().unwrap();
        assert_eq!(
            o.entry(id).unwrap().anchor(),
            &OverlayAnchor::BoundsEdge {
                rect: Rect::new(100.0, 100.0, 60.0, 24.0),
                edge: AnchorEdge::Bottom,
            }
        );
        // Placed below the anchor.
        let b = o.entry_bounds(id).unwrap();
        assert!(b.min_y() >= 124.0);
    }

    #[test]
    fn outside_press_dismisses_and_reconciles() {
        let mut p = Popover::new();
        laid_out(&mut p, Rect::new(100.0, 100.0, 60.0, 24.0));
        let mut o = overlay();
        p.open();
        p.sync_overlay(&mut o);
        o.layout_pass();
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(700.0, 500.0),
            button: PointerButton::Primary,
            count: 1,
        };
        assert_eq!(o.dispatch_event(&press), EventResponse::Ignored);
        p.sync_overlay(&mut o);
        assert!(!p.is_open());
        assert_eq!(p.popup_id(), None);
    }

    #[test]
    fn autohide_false_survives_outside_press() {
        let mut p = Popover::new().autohide(false);
        laid_out(&mut p, Rect::new(100.0, 100.0, 60.0, 24.0));
        let mut o = overlay();
        p.open();
        p.sync_overlay(&mut o);
        o.layout_pass();
        assert!(
            o.entry(p.popup_id().unwrap())
                .unwrap()
                .options()
                .passthrough
        );
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(700.0, 500.0),
            button: PointerButton::Primary,
            count: 1,
        };
        // The press falls through to content; the bubble stays open.
        assert_eq!(o.dispatch_event(&press), EventResponse::Ignored);
        assert!(o.is_open(p.popup_id().unwrap()));
        p.sync_overlay(&mut o);
        assert!(p.is_open());
        // Escape reaches the passthrough surface, which asks to close.
        let esc = WidgetEvent::KeyPressed {
            key: "Escape".to_string(),
            repeat: false,
        };
        assert_eq!(o.dispatch_event(&esc), EventResponse::Handled);
        p.sync_overlay(&mut o);
        assert!(!p.is_open());
    }

    #[test]
    fn child_round_trips_through_surface() {
        let mut p = Popover::new().child(Text::new("content"));
        laid_out(&mut p, Rect::new(100.0, 100.0, 60.0, 24.0));
        let mut o = overlay();
        assert_eq!(p.child_count(), 1);
        p.open();
        p.sync_overlay(&mut o);
        // The child moved into the surface.
        assert_eq!(p.child_count(), 0);
        o.layout_pass();
        let surface = o.widget_at_mut(p.popup_id().unwrap(), &[]).unwrap();
        assert_eq!(surface.child_count(), 1);
        // Explicit close → the child comes home via the reclaim slot.
        p.close();
        p.sync_overlay(&mut o);
        assert_eq!(p.child_count(), 1);
        // Re-open and let the layer dismiss it — the child still
        // survives through `Drop`.
        p.open();
        p.sync_overlay(&mut o);
        o.layout_pass();
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(700.0, 500.0),
            button: PointerButton::Primary,
            count: 1,
        };
        o.dispatch_event(&press);
        p.sync_overlay(&mut o);
        assert!(!p.is_open());
        assert_eq!(p.child_count(), 1);
    }

    #[test]
    fn open_bubble_reanchors_when_marker_moves() {
        let mut p = Popover::new();
        laid_out(&mut p, Rect::new(100.0, 100.0, 60.0, 24.0));
        let mut o = overlay();
        p.open();
        p.sync_overlay(&mut o);
        o.layout_pass();
        let id = p.popup_id().unwrap();
        let moved = Rect::new(200.0, 150.0, 60.0, 24.0);
        laid_out(&mut p, moved);
        p.sync_overlay(&mut o);
        assert_eq!(
            o.entry(id).unwrap().anchor(),
            &OverlayAnchor::BoundsEdge {
                rect: moved,
                edge: AnchorEdge::Bottom,
            }
        );
    }

    #[test]
    fn marker_accessibility() {
        let mut p = Popover::new().title("Details");
        p.open();
        let mut node = AccessKitNode::new(accesskit::Role::Unknown);
        p.accessibility(&mut node);
        assert_eq!(node.has_popup(), Some(accesskit::HasPopup::Dialog));
        assert_eq!(node.is_expanded(), Some(true));
    }

    #[test]
    fn surface_emits_dialog_role() {
        let shared = Arc::new(Mutex::new(PopoverShared::default()));
        let surface = PopoverSurface {
            title: "Title".to_string(),
            content: None,
            shared,
            reclaim: Arc::new(Mutex::new(None)),
            flow: AnchorEdge::Bottom,
            bounds: Rect::default(),
            content_rect: Rect::default(),
            painted_shape: Mutex::new(Shape::RECT),
            text_painter: None,
        };
        let mut node = AccessKitNode::new(accesskit::Role::Unknown);
        surface.accessibility(&mut node);
        assert_eq!(node.role(), accesskit::Role::Dialog);
        assert_eq!(node.label(), Some("Title"));
    }
}
