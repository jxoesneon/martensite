//! `ExternalEngine` widget — the `PaintCallback` primitive for embedding
//! external GPU renderers (Milestone `v0.14.0`, ADR-0033).
//!
//! The widget is a retained leaf in the widget tree: it occupies layout
//! space like any other widget, but its paint output is a
//! [`PaintCommand::External`](martensite_render::PaintCommand::External)
//! marker resolved by `martensite-wgpu`'s `WgpuHost` into a direct
//! `wgpu::Texture` sample — zero GPU copies on the same-device path.
//!
//! # Frame flow
//!
//! ```text
//! producer (Engine impl)                widget / host
//! ───────────────────────               ────────────────
//! registry.acquire(surface)
//! render into slot texture
//! registry.mark_ready_full(...)   ──►   ExternalEngine::poll_frame()
//!                                       → intrinsic size updated
//!                                       → layout / repaint requested
//!                                     dispatch: take_front →
//!                                     composite_front samples the
//!                                     slot's texture (zero-copy)
//! registry.release(surface, slot) ◄──   after queue.submit
//! engines.drain_released()              → Engine::release → recycled
//! ```
//!
//! [`ExternalEngines`] is the app-loop integration point: call
//! `render_frame` (pull engines), `drain_ready` (→ dirty + redraw), and
//! `drain_released` (→ `Engine::release`) once per frame.

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::node::Rect;
use martensite_core::widget::{LayoutConstraints, LayoutContext, PaintContext, Widget};
use martensite_engine_bridge::{
    BridgeHandle, Engine, EngineContext, FrameToken, SurfaceId, Viewport,
};
use martensite_render::PaintList;

use super::media::VideoFit;

/// What [`ExternalEngine::poll_frame`] observed for the widget's surface.
///
/// # Examples
///
/// ```
/// use martensite::widgets::external::FramePoll;
///
/// assert_ne!(FramePoll::NewFrame, FramePoll::None);
/// ```
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum FramePoll {
    /// No new frame since the last poll.
    None,
    /// A new frame is ready; repaint is required.
    NewFrame,
    /// The front frame's size changed; layout must be re-run.
    Resized,
}

/// A retained leaf widget displaying frames produced by an external GPU
/// engine via `martensite-engine-bridge`.
///
/// `ExternalEngine` plays the role egui's `PaintCallback`, iced's
/// `widget::shader`, and Slint's `Image::try_from(wgpu::Texture)` play in
/// their frameworks: the single primitive every engine adapter (Bevy,
/// Godot, decoder, compositor) targets.
///
/// # Examples
///
/// ```
/// use martensite::widgets::external::ExternalEngine;
/// use martensite_engine_bridge::BridgeHandle;
///
/// let handle = BridgeHandle::new();
/// let surface = handle.lock().register();
/// let widget = ExternalEngine::new(handle, surface);
/// assert_eq!(widget.surface_id(), surface);
/// ```
pub struct ExternalEngine {
    surface_id: SurfaceId,
    handle: BridgeHandle,
    intrinsic_size: Vec2,
    fit: VideoFit,
    cached_bounds: Rect,
    cached_dest_rect: Rect,
    last_token: Option<FrameToken>,
    label: String,
    scale_factor: f64,
}

impl ExternalEngine {
    /// Creates a widget bound to `surface` on `handle`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::external::ExternalEngine;
    /// use martensite_engine_bridge::BridgeHandle;
    ///
    /// let handle = BridgeHandle::new();
    /// let surface = handle.lock().register();
    /// let mut widget = ExternalEngine::new(handle, surface);
    /// assert_eq!(widget.poll_frame(), martensite::widgets::external::FramePoll::None);
    /// ```
    pub fn new(handle: BridgeHandle, surface_id: SurfaceId) -> Self {
        Self {
            surface_id,
            handle,
            intrinsic_size: Vec2::ZERO,
            fit: VideoFit::default(),
            cached_bounds: Rect::default(),
            cached_dest_rect: Rect::default(),
            last_token: None,
            label: String::from("External content"),
            scale_factor: 1.0,
        }
    }

    /// Sets the display scale factor forwarded to producers via the
    /// bridge viewport (e.g. `2.0` on Retina). The paint loop should
    /// call this when the window's `scale_factor` changes.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::external::ExternalEngine;
    /// use martensite_engine_bridge::BridgeHandle;
    ///
    /// let handle = BridgeHandle::new();
    /// let surface = handle.lock().register();
    /// let widget = ExternalEngine::new(handle, surface).with_scale_factor(2.0);
    /// ```
    #[must_use]
    pub fn with_scale_factor(mut self, scale_factor: f64) -> Self {
        self.scale_factor = scale_factor;
        self
    }

    /// Sets the content-fit mode (letterbox, crop, stretch, or native
    /// size). Reuses [`VideoFit`] — the semantics are identical.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::external::ExternalEngine;
    /// use martensite::widgets::media::VideoFit;
    /// use martensite_engine_bridge::BridgeHandle;
    ///
    /// let handle = BridgeHandle::new();
    /// let surface = handle.lock().register();
    /// let widget = ExternalEngine::new(handle, surface).with_fit(VideoFit::Cover);
    /// ```
    #[must_use]
    pub fn with_fit(mut self, fit: VideoFit) -> Self {
        self.fit = fit;
        self
    }

    /// Sets the accessible label announced for the region.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::external::ExternalEngine;
    /// use martensite_engine_bridge::BridgeHandle;
    ///
    /// let handle = BridgeHandle::new();
    /// let surface = handle.lock().register();
    /// let widget = ExternalEngine::new(handle, surface).with_label("3D viewport");
    /// ```
    #[must_use]
    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// The bridge surface this widget displays.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::external::ExternalEngine;
    /// use martensite_engine_bridge::BridgeHandle;
    ///
    /// let handle = BridgeHandle::new();
    /// let surface = handle.lock().register();
    /// let widget = ExternalEngine::new(handle, surface);
    /// assert_eq!(widget.surface_id(), surface);
    /// ```
    #[inline]
    #[must_use]
    pub fn surface_id(&self) -> SurfaceId {
        self.surface_id
    }

    /// The current intrinsic (frame-native) size in physical pixels.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::external::ExternalEngine;
    /// use martensite_engine_bridge::BridgeHandle;
    /// use glam::Vec2;
    ///
    /// let handle = BridgeHandle::new();
    /// let surface = handle.lock().register();
    /// let widget = ExternalEngine::new(handle, surface);
    /// assert_eq!(widget.intrinsic_size(), Vec2::ZERO);
    /// ```
    #[inline]
    #[must_use]
    pub fn intrinsic_size(&self) -> Vec2 {
        self.intrinsic_size
    }

    /// The rectangle the external content occupies within the widget
    /// bounds after fit scaling.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::external::ExternalEngine;
    /// use martensite_engine_bridge::BridgeHandle;
    ///
    /// let handle = BridgeHandle::new();
    /// let surface = handle.lock().register();
    /// let widget = ExternalEngine::new(handle, surface);
    /// assert_eq!(widget.dest_rect().size.x, 0.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn dest_rect(&self) -> Rect {
        self.cached_dest_rect
    }

    /// Observes the front frame for this surface and updates the
    /// intrinsic size. Returns what changed — the paint loop maps
    /// [`FramePoll::NewFrame`] to a repaint request and
    /// [`FramePoll::Resized`] to a relayout.
    ///
    /// The slot, [`FrameToken`], and size are read atomically via
    /// [`BridgeRegistry::front_with_token`](martensite_engine_bridge::BridgeRegistry::front_with_token),
    /// so a new frame arriving mid-poll cannot mix fields from two
    /// frames.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::external::{ExternalEngine, FramePoll};
    /// use martensite_engine_bridge::BridgeHandle;
    ///
    /// let handle = BridgeHandle::new();
    /// let surface = handle.lock().register();
    /// let mut widget = ExternalEngine::new(handle.clone(), surface);
    /// {
    ///     let mut reg = handle.lock();
    ///     let (slot, _) = reg.acquire(surface).unwrap();
    ///     reg.mark_ready_sized(surface, slot, (320, 240)).unwrap();
    /// }
    /// // Ready events are drained centrally; the widget observes the front.
    /// handle.lock().drain_ready();
    /// assert_eq!(widget.poll_frame(), FramePoll::Resized);
    /// ```
    pub fn poll_frame(&mut self) -> FramePoll {
        let Ok(front) = self.handle.lock().front_with_token(self.surface_id) else {
            return FramePoll::None;
        };
        let Some(front) = front else {
            return FramePoll::None;
        };
        let new_size = front.size.map(|(w, h)| Vec2::new(w as f32, h as f32));
        let changed_size = matches!(new_size, Some(s) if s != self.intrinsic_size);
        if changed_size {
            self.intrinsic_size = new_size.unwrap_or(self.intrinsic_size);
        }
        // Tokens are unique per frame — a recycled slot still yields a
        // fresh token, so this correctly detects every new frame.
        let is_new = Some(front.token) != self.last_token;
        self.last_token = Some(front.token);
        if changed_size {
            FramePoll::Resized
        } else if is_new {
            FramePoll::NewFrame
        } else {
            FramePoll::None
        }
    }

    /// Emits the widget's paint output into `list`: a
    /// `PaintCommand::External` marker at this widget's position in the
    /// paint order.
    ///
    /// The trait `Widget::paint` remains a no-op (the production paint
    /// loop is not yet wired); this method is the real emission path and
    /// is what the paint recorder calls once the loop lands.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::external::ExternalEngine;
    /// use martensite_engine_bridge::BridgeHandle;
    /// use martensite_render::{PaintCommand, PaintList};
    ///
    /// let handle = BridgeHandle::new();
    /// let surface = handle.lock().register();
    /// let widget = ExternalEngine::new(handle, surface);
    /// let mut list = PaintList::new();
    /// widget.record_paint(&mut list);
    /// assert!(matches!(
    ///     list.commands[0],
    ///     PaintCommand::External { surface_id, .. } if surface_id == surface.0
    /// ));
    /// ```
    pub fn record_paint(&self, list: &mut PaintList) {
        let r = self.cached_dest_rect;
        let b = self.cached_bounds;
        list.push_external(
            self.surface_id.0,
            [r.origin.x, r.origin.y, r.size.x, r.size.y],
            [b.origin.x, b.origin.y, b.size.x, b.size.y],
        );
    }

    /// Computes the destination rectangle for `fit` inside `bounds`,
    /// matching [`MediaView`](super::media::MediaView)'s behavior.
    fn compute_dest_rect(&self, bounds: Rect, fit: VideoFit) -> Rect {
        let iw = self.intrinsic_size.x.max(1.0);
        let ih = self.intrinsic_size.y.max(1.0);
        let bw = bounds.size.x.max(0.0);
        let bh = bounds.size.y.max(0.0);
        if bw <= 0.0 || bh <= 0.0 {
            return bounds;
        }
        let (w, h) = match fit {
            VideoFit::Fill => (bw, bh),
            // Native size unclamped — the clip rect confines overflow,
            // matching `MediaView::compute_dest_rect`.
            VideoFit::Fixed => (iw, ih),
            VideoFit::Contain => {
                let scale = (bw / iw).min(bh / ih);
                (iw * scale, ih * scale)
            }
            // Cover overflows the bounds intentionally — the paint
            // command's clip rect confines it.
            VideoFit::Cover => {
                let scale = (bw / iw).max(bh / ih);
                (iw * scale, ih * scale)
            }
        };
        Rect::new(
            bounds.origin.x + (bw - w) * 0.5,
            bounds.origin.y + (bh - h) * 0.5,
            w,
            h,
        )
    }
}

/// A bound collection of external [`Engine`] producers — the single
/// integration point the application frame loop calls once per frame.
///
/// `ExternalEngines` closes the bridge lifecycle in production code:
///
/// ```text
/// once per frame:
///   engines.render_frame(&mut ctx)      — pull-driven engines render
///                                         into their ring slots using
///                                         the stored Viewport
///   engines.drain_ready()               — surfaces with fresh frames
///     → mark the owning widget dirty + window.request_redraw()
///   ... widget tree paints, orchestrator composites, queue submits ...
///   engines.drain_released()            — Engine::release per token,
///                                         recycling the producer's
///                                         slots
/// ```
///
/// Streaming producers (video decoders, engine threads) may skip
/// [`render_frame`](Self::render_frame) and call `mark_ready_*` from
/// their own cadence; `drain_ready`/`drain_released` still apply.
///
/// # Examples
///
/// ```
/// use martensite::widgets::external::ExternalEngines;
/// use martensite_engine_bridge::BridgeHandle;
///
/// let handle = BridgeHandle::new();
/// let mut engines = ExternalEngines::new();
/// // engines.bind(handle, surface, Box::new(my_engine));
/// assert!(engines.drain_ready().is_empty());
/// ```
#[derive(Default)]
pub struct ExternalEngines {
    /// (bridge handle, surface, engine) triples — each engine owns one
    /// surface on one registry.
    bound: Vec<(BridgeHandle, SurfaceId, Box<dyn Engine>)>,
}

impl ExternalEngines {
    /// Creates an empty collection.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::external::ExternalEngines;
    ///
    /// let engines = ExternalEngines::new();
    /// assert_eq!(engines.len(), 0);
    /// ```
    pub fn new() -> Self {
        Self::default()
    }

    /// Binds `engine` to `surface` on `handle`. The engine is expected
    /// to publish frames into that surface's ring.
    ///
    /// # Errors
    ///
    /// [`BindError::DuplicateBinding`] if an engine is already bound to
    /// the same `(registry, surface)` pair — the second engine would
    /// never see `release` callbacks (the per-surface released queue is
    /// drained by the first).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::external::ExternalEngines;
    /// use martensite_engine_bridge::{
    ///     BridgeHandle, Engine, EngineContext, Frame, FrameToken, Viewport,
    /// };
    ///
    /// struct Idle;
    /// impl Engine for Idle {
    ///     fn render(&mut self, _c: &mut EngineContext, _v: Viewport) -> Option<Box<dyn Frame>> {
    ///         None
    ///     }
    ///     fn release(&mut self, _t: FrameToken) {}
    /// }
    ///
    /// let handle = BridgeHandle::new();
    /// let surface = handle.lock().register();
    /// let mut engines = ExternalEngines::new();
    /// engines.bind(handle, surface, Box::new(Idle)).unwrap();
    /// assert_eq!(engines.len(), 1);
    /// ```
    pub fn bind(
        &mut self,
        handle: BridgeHandle,
        surface: SurfaceId,
        engine: Box<dyn Engine>,
    ) -> Result<(), BindError> {
        if self
            .bound
            .iter()
            .any(|(h, s, _)| *s == surface && h.same_registry(&handle))
        {
            return Err(BindError::DuplicateBinding);
        }
        self.bound.push((handle, surface, engine));
        Ok(())
    }

    /// Number of bound engines.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::external::ExternalEngines;
    ///
    /// assert_eq!(ExternalEngines::new().len(), 0);
    /// ```
    pub fn len(&self) -> usize {
        self.bound.len()
    }

    /// Whether no engines are bound.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::external::ExternalEngines;
    ///
    /// assert!(ExternalEngines::new().is_empty());
    /// ```
    pub fn is_empty(&self) -> bool {
        self.bound.is_empty()
    }

    /// Drives every bound pull-style engine once, passing the
    /// [`Viewport`] its widget last laid out into (via
    /// [`BridgeRegistry::set_viewport`]). Engines without a stored
    /// viewport are skipped — the widget hasn't laid out yet.
    ///
    /// `BridgeRegistry::set_viewport` is pushed by
    /// [`ExternalEngine::layout`]; this is how producers learn the
    /// physical size + DPI to render at.
    ///
    /// Engines publish into their ring inside `render` (the
    /// `mark_ready_*` family) — the returned `Vec` carries each produced
    /// [`Frame`](martensite_engine_bridge::Frame) alongside its surface
    /// for inspection; the composite path consumes the ring's copy, so
    /// callers may drop the result.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite::widgets::external::ExternalEngines;
    /// use martensite_engine_bridge::{BridgeHandle, EngineContext};
    /// # let mut ctx: EngineContext = todo!();
    /// # let handle = BridgeHandle::new();
    /// let mut engines = ExternalEngines::new();
    /// let produced = engines.render_frame(&mut ctx);
    /// ```
    ///
    /// [`BridgeRegistry::set_viewport`]: martensite_engine_bridge::BridgeRegistry::set_viewport
    pub fn render_frame(
        &mut self,
        ctx: &mut EngineContext,
    ) -> Vec<(SurfaceId, Box<dyn martensite_engine_bridge::Frame>)> {
        let mut produced = Vec::new();
        for (handle, surface, engine) in &mut self.bound {
            let viewport = match handle.lock().viewport(*surface) {
                Ok(Some(vp)) => vp,
                _ => continue,
            };
            if let Some(frame) = engine.render(ctx, viewport) {
                produced.push((*surface, frame));
            }
        }
        produced
    }

    /// Drains the ready-event queues of every bound registry and
    /// returns the surfaces with fresh frames. The frame loop maps each
    /// to its owning widget's dirty flag + `window.request_redraw()`.
    ///
    /// Registries shared by several bound engines are drained once —
    /// the ready queue is per-registry, so each unique
    /// [`BridgeHandle::same_registry`] group contributes every ready
    /// surface bound to it.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::external::ExternalEngines;
    /// use martensite_engine_bridge::BridgeHandle;
    ///
    /// let handle = BridgeHandle::new();
    /// let surface = handle.lock().register();
    /// {
    ///     let mut reg = handle.lock();
    ///     let (slot, _) = reg.acquire(surface).unwrap();
    ///     reg.mark_ready(surface, slot).unwrap();
    /// }
    /// let mut engines = ExternalEngines::new();
    /// struct Idle;
    /// impl martensite_engine_bridge::Engine for Idle {
    ///     fn render(&mut self, _c: &mut martensite_engine_bridge::EngineContext, _v: martensite_engine_bridge::Viewport) -> Option<Box<dyn martensite_engine_bridge::Frame>> { None }
    ///     fn release(&mut self, _t: martensite_engine_bridge::FrameToken) {}
    /// }
    /// engines.bind(handle, surface, Box::new(Idle)).unwrap();
    /// assert_eq!(engines.drain_ready(), vec![surface]);
    /// ```
    pub fn drain_ready(&mut self) -> Vec<SurfaceId> {
        let mut out = Vec::new();
        // Group bindings by registry once — the ready queue is
        // per-registry, so each unique registry is drained exactly once.
        let mut seen: std::collections::HashSet<usize> = std::collections::HashSet::new();
        for i in 0..self.bound.len() {
            if !seen.insert(self.bound[i].0.registry_id()) {
                continue;
            }
            // Keep only surfaces bound on THIS registry — both so
            // unbound surfaces' events survive in the queue and so
            // `SurfaceId`s from different registries can't collide.
            let here: std::collections::HashSet<SurfaceId> = self
                .bound
                .iter()
                .filter(|(h, _, _)| h.same_registry(&self.bound[i].0))
                .map(|(_, s, _)| *s)
                .collect();
            out.extend(
                self.bound[i]
                    .0
                    .lock()
                    .drain_ready_matching(|s| here.contains(&s)),
            );
        }
        out
    }

    /// Drains each registry's released-token queue for the bound
    /// surfaces and calls [`Engine::release`] once per token — the
    /// producer-side recycling half of the bridge lifecycle.
    ///
    /// Call after the frame's composite encoder has been submitted.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::external::ExternalEngines;
    /// use martensite_engine_bridge::BridgeHandle;
    ///
    /// let handle = BridgeHandle::new();
    /// let surface = handle.lock().register();
    /// {
    ///     let mut reg = handle.lock();
    ///     let (slot, token) = reg.acquire(surface).unwrap();
    ///     reg.mark_ready(surface, slot).unwrap();
    ///     let (s, _) = reg.take_front(surface).unwrap().unwrap();
    ///     reg.release(surface, s).unwrap();
    /// }
    /// let mut engines = ExternalEngines::new();
    /// struct Rec(std::sync::Mutex<Vec<martensite_engine_bridge::FrameToken>>);
    /// impl martensite_engine_bridge::Engine for Rec {
    ///     fn render(&mut self, _c: &mut martensite_engine_bridge::EngineContext, _v: martensite_engine_bridge::Viewport) -> Option<Box<dyn martensite_engine_bridge::Frame>> { None }
    ///     fn release(&mut self, t: martensite_engine_bridge::FrameToken) { self.0.lock().unwrap().push(t); }
    /// }
    /// engines.bind(handle, surface, Box::new(Rec(std::sync::Mutex::new(Vec::new())))).unwrap();
    /// engines.drain_released(); // engine.release called once for token
    /// ```
    pub fn drain_released(&mut self) {
        for (handle, surface, engine) in &mut self.bound {
            let tokens = handle.lock().drain_released(*surface).unwrap_or_default();
            for token in tokens {
                engine.release(token);
            }
        }
    }
}

/// Error returned by [`ExternalEngines::bind`].
///
/// # Examples
///
/// ```
/// use martensite::widgets::external::BindError;
///
/// assert_eq!(BindError::DuplicateBinding, BindError::DuplicateBinding);
/// ```
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum BindError {
    /// An engine is already bound to this `(registry, surface)` pair.
    DuplicateBinding,
}

impl std::fmt::Display for BindError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateBinding => f.write_str("an engine is already bound to this surface"),
        }
    }
}

impl std::error::Error for BindError {}

impl std::fmt::Debug for ExternalEngines {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExternalEngines")
            .field("bound", &self.bound.len())
            .finish_non_exhaustive()
    }
}

impl Widget for ExternalEngine {
    fn measure(&mut self, _cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        // A zero intrinsic size (no frame yet) yields a flexible zero so
        // the viewport can appear lazily without reserving space.
        self.intrinsic_size
            .clamp(constraints.min_size, constraints.max_size)
    }

    fn layout(&mut self, _cx: &mut LayoutContext, bounds: Rect) {
        self.cached_bounds = bounds;
        self.cached_dest_rect = self.compute_dest_rect(bounds, self.fit);
        // Forward the physical-pixel bounds + DPI to the bridge so
        // producers render at the widget's actual size and scale.
        let _ = self.handle.lock().set_viewport(
            self.surface_id,
            Viewport::new(
                bounds.size.x.max(0.0) as u32,
                bounds.size.y.max(0.0) as u32,
                self.scale_factor,
            ),
        );
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Image);
        node.set_label(self.label.as_str());
    }

    fn paint(&self, _cx: &mut PaintContext) {
        // Emission path is `record_paint` — see its docs.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;
    use martensite_render::PaintCommand;

    fn widget_with_frame(size: (u32, u32)) -> (ExternalEngine, BridgeHandle, SurfaceId) {
        let handle = BridgeHandle::new();
        let surface = handle.lock().register();
        let mut w = ExternalEngine::new(handle.clone(), surface);
        {
            let mut reg = handle.lock();
            let (slot, _) = reg.acquire(surface).unwrap();
            reg.mark_ready_sized(surface, slot, size).unwrap();
            reg.drain_ready();
        }
        w.poll_frame();
        (w, handle, surface)
    }

    #[test]
    fn measure_reports_intrinsic_size() {
        let (mut w, _h, _s) = widget_with_frame((640, 360));
        let mut hot = HotNode::new(taffy::NodeId::new(1));
        let mut cx = LayoutContext { hot: &mut hot };
        let size = w.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(1000.0, 1000.0),
            },
        );
        assert_eq!(size, Vec2::new(640.0, 360.0));
    }

    #[test]
    fn contain_fit_letterboxes() {
        let (mut w, _h, _s) = widget_with_frame((1920, 1080));
        let mut hot = HotNode::new(taffy::NodeId::new(1));
        let mut cx = LayoutContext { hot: &mut hot };
        w.layout(&mut cx, Rect::new(0.0, 0.0, 1000.0, 1000.0));
        let dest = w.dest_rect();
        assert!((dest.size.x - 1000.0).abs() < 1e-3);
        assert!((dest.size.y - 562.5).abs() < 1e-3);
        assert!((dest.origin.y - 218.75).abs() < 1e-3);
    }

    #[test]
    fn record_paint_emits_external_marker() {
        let (mut w, _h, surface) = widget_with_frame((64, 64));
        let mut hot = HotNode::new(taffy::NodeId::new(1));
        let mut cx = LayoutContext { hot: &mut hot };
        w.layout(&mut cx, Rect::new(10.0, 20.0, 100.0, 100.0));
        let mut list = PaintList::new();
        w.record_paint(&mut list);
        assert_eq!(list.len(), 1);
        match list.commands[0] {
            PaintCommand::External {
                surface_id,
                rect,
                clip,
            } => {
                assert_eq!(surface_id, surface.0);
                assert_eq!(clip, [10.0, 20.0, 100.0, 100.0]);
                assert_eq!(rect[2], 100.0);
            }
            _ => panic!("expected PaintCommand::External"),
        }
    }

    #[test]
    fn poll_frame_reports_resize_then_new() {
        let (mut w, handle, surface) = widget_with_frame((64, 64));
        assert_eq!(w.poll_frame(), FramePoll::None);
        // New frame, same size → NewFrame.
        {
            let mut reg = handle.lock();
            let (slot, _) = reg.acquire(surface).unwrap();
            reg.mark_ready_sized(surface, slot, (64, 64)).unwrap();
            reg.drain_ready();
        }
        assert_eq!(w.poll_frame(), FramePoll::NewFrame);
        // New frame, different size → Resized.
        {
            let mut reg = handle.lock();
            let (slot, _) = reg.acquire(surface).unwrap();
            reg.mark_ready_sized(surface, slot, (128, 128)).unwrap();
            reg.drain_ready();
        }
        assert_eq!(w.poll_frame(), FramePoll::Resized);
        assert_eq!(w.intrinsic_size(), Vec2::new(128.0, 128.0));
    }

    #[test]
    fn layout_forwards_viewport_to_bridge() {
        let (w, handle, surface) = widget_with_frame((64, 64));
        // Non-1.0 scale factor proves the forwarding end-to-end.
        let mut w = w.with_scale_factor(2.0);
        let mut hot = HotNode::new(taffy::NodeId::new(1));
        let mut cx = LayoutContext { hot: &mut hot };
        w.layout(&mut cx, Rect::new(0.0, 0.0, 320.0, 240.0));
        let vp = handle.lock().viewport(surface).unwrap().unwrap();
        assert_eq!(vp.size, (320, 240));
        assert_eq!(vp.scale_factor, 2.0);
    }

    #[test]
    fn external_engines_drives_release_lifecycle() {
        use std::sync::{Arc, Mutex as StdMutex};
        struct Rec(Arc<StdMutex<Vec<FrameToken>>>);
        impl Engine for Rec {
            fn render(
                &mut self,
                _c: &mut martensite_engine_bridge::EngineContext,
                _v: Viewport,
            ) -> Option<Box<dyn martensite_engine_bridge::Frame>> {
                None
            }
            fn release(&mut self, t: FrameToken) {
                self.0.lock().unwrap().push(t);
            }
        }
        let handle = BridgeHandle::new();
        let surface = handle.lock().register();
        // Host composites a frame: take → release → released queue.
        let expected_token = {
            let mut reg = handle.lock();
            let (slot, token) = reg.acquire(surface).unwrap();
            reg.mark_ready(surface, slot).unwrap();
            let (s, _) = reg.take_front(surface).unwrap().unwrap();
            reg.release(surface, s).unwrap();
            token
        };
        let seen = Arc::new(StdMutex::new(Vec::new()));
        let mut engines = ExternalEngines::new();
        engines
            .bind(handle, surface, Box::new(Rec(Arc::clone(&seen))))
            .unwrap();
        engines.drain_released();
        // The bound engine saw exactly one release for the composited token.
        assert_eq!(*seen.lock().unwrap(), vec![expected_token]);
        // Re-drain: no double-release.
        engines.drain_released();
        assert_eq!(seen.lock().unwrap().len(), 1);
    }

    #[test]
    fn external_engines_drain_ready_reports_surfaces() {
        struct Idle;
        impl Engine for Idle {
            fn render(
                &mut self,
                _c: &mut martensite_engine_bridge::EngineContext,
                _v: Viewport,
            ) -> Option<Box<dyn martensite_engine_bridge::Frame>> {
                None
            }
            fn release(&mut self, _t: FrameToken) {}
        }
        let handle = BridgeHandle::new();
        let surface = handle.lock().register();
        {
            let mut reg = handle.lock();
            let (slot, _) = reg.acquire(surface).unwrap();
            reg.mark_ready(surface, slot).unwrap();
        }
        let mut engines = ExternalEngines::new();
        engines.bind(handle, surface, Box::new(Idle)).unwrap();
        assert_eq!(engines.drain_ready(), vec![surface]);
        assert!(engines.drain_ready().is_empty());
    }

    #[test]
    fn drain_ready_shared_registry_reports_all_bound_surfaces() {
        // Regression: the ready queue is per-registry — draining it per
        // binding would consume events for the sibling surface.
        struct Idle;
        impl Engine for Idle {
            fn render(
                &mut self,
                _c: &mut martensite_engine_bridge::EngineContext,
                _v: Viewport,
            ) -> Option<Box<dyn martensite_engine_bridge::Frame>> {
                None
            }
            fn release(&mut self, _t: FrameToken) {}
        }
        let handle = BridgeHandle::new();
        let (s1, s2) = {
            let mut reg = handle.lock();
            (reg.register(), reg.register())
        };
        {
            let mut reg = handle.lock();
            let (a, _) = reg.acquire(s1).unwrap();
            reg.mark_ready(s1, a).unwrap();
            let (b, _) = reg.acquire(s2).unwrap();
            reg.mark_ready(s2, b).unwrap();
        }
        let mut engines = ExternalEngines::new();
        engines.bind(handle.clone(), s1, Box::new(Idle)).unwrap();
        engines.bind(handle, s2, Box::new(Idle)).unwrap();
        let ready = engines.drain_ready();
        assert!(ready.contains(&s1) && ready.contains(&s2), "got {ready:?}");
    }

    #[test]
    fn duplicate_bind_rejected() {
        struct Idle;
        impl Engine for Idle {
            fn render(
                &mut self,
                _c: &mut martensite_engine_bridge::EngineContext,
                _v: Viewport,
            ) -> Option<Box<dyn martensite_engine_bridge::Frame>> {
                None
            }
            fn release(&mut self, _t: FrameToken) {}
        }
        let handle = BridgeHandle::new();
        let surface = handle.lock().register();
        let mut engines = ExternalEngines::new();
        engines
            .bind(handle.clone(), surface, Box::new(Idle))
            .unwrap();
        assert!(matches!(
            engines.bind(handle, surface, Box::new(Idle)),
            Err(BindError::DuplicateBinding)
        ));
        assert_eq!(engines.len(), 1);
    }

    #[test]
    fn poll_frame_detects_recycled_slot_frame() {
        // Regression: when the mailbox recycles the same slot index,
        // the fresh FrameToken must still report a new frame.
        let (mut w, handle, surface) = widget_with_frame((64, 64));
        assert_eq!(w.poll_frame(), FramePoll::None);
        {
            let mut reg = handle.lock();
            // Simulate host consumption: take + release the front.
            let (slot, _token) = reg.take_front(surface).unwrap().unwrap();
            reg.release(surface, slot).unwrap();
        }
        assert_eq!(w.poll_frame(), FramePoll::None);
        {
            let mut reg = handle.lock();
            let released = reg.drain_released(surface).unwrap();
            assert_eq!(released.len(), 1);
            // Producer reuses the same slot for the next frame.
            let (slot, _) = reg.acquire(surface).unwrap();
            reg.mark_ready_sized(surface, slot, (64, 64)).unwrap();
            reg.drain_ready();
        }
        assert_eq!(w.poll_frame(), FramePoll::NewFrame);
    }
}
