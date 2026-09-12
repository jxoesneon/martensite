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
//! registry.mark_ready_sized(...)  ──►   ExternalEngine::poll_frame()
//!                                       → intrinsic size updated
//!                                       → layout / repaint requested
//!                                     orchestrator composites the
//!                                     front texture at the marker
//! registry.release(surface, slot) ◄──   (after the pass)
//! engine.drain_released()               → slot recycled
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::node::Rect;
use martensite_core::widget::{LayoutConstraints, LayoutContext, PaintContext, Widget};
use martensite_engine_bridge::{BridgeHandle, FrameToken, SurfaceId};
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
        }
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
            VideoFit::Fixed => (iw.min(bw), ih.min(bh)),
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
