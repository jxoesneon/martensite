//! Test-support producer used by the conformance suite.
//!
//! [`MockEngine`] synthesizes deterministic `wgpu::Texture` frames on the
//! host device — a solid color that increments a corner pixel per frame —
//! so tests can verify the full acquire → ready → composite → release
//! lifecycle without a real external engine.

use crate::bridge::{BridgeHandle, SurfaceId};
use crate::engine::{Engine, EngineContext, Viewport};
use crate::frame::{CpuFrame, Frame, FrameToken, TextureFrame};

/// A deterministic same-device producer for tests and examples.
///
/// Each `render` call acquires a ring slot on the bound surface, creates
/// a `wgpu::Texture` of the viewport size, fills it with a solid color
/// whose red channel encodes the frame counter, marks it ready, and
/// returns the [`TextureFrame`]. On `wgpu`'s `noop` backend the texture
/// operations are validated but execute no GPU work, so the engine runs
/// in CI.
///
/// # Examples
///
/// ```no_run
/// use martensite_engine_bridge::testing::MockEngine;
/// use martensite_engine_bridge::{BridgeHandle, Engine, EngineContext, Viewport};
/// # let (device, queue): (wgpu::Device, wgpu::Queue) = todo!();
///
/// let handle = BridgeHandle::new();
/// let surface = handle.lock().register();
/// let mut engine = MockEngine::new(handle.clone(), surface, [200, 80, 40, 255]);
/// let mut ctx = EngineContext {
///     device: &device,
///     queue: &queue,
/// };
/// let frame = engine
///     .render(&mut ctx, Viewport::new(64, 64, 1.0))
///     .expect("a frame is produced");
/// assert_eq!(frame.size(), (64, 64));
/// ```
pub struct MockEngine {
    handle: BridgeHandle,
    surface: SurfaceId,
    base_color: [u8; 4],
    frame_count: u64,
    released: Vec<FrameToken>,
}

impl MockEngine {
    /// Creates a `MockEngine` bound to `surface` on `handle`, producing
    /// frames tinted with `base_color` (RGBA8).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_engine_bridge::testing::MockEngine;
    /// use martensite_engine_bridge::BridgeHandle;
    ///
    /// let handle = BridgeHandle::new();
    /// let surface = handle.lock().register();
    /// let engine = MockEngine::new(handle, surface, [255, 0, 0, 255]);
    /// assert_eq!(engine.frame_count(), 0);
    /// ```
    pub fn new(handle: BridgeHandle, surface: SurfaceId, base_color: [u8; 4]) -> Self {
        Self {
            handle,
            surface,
            base_color,
            frame_count: 0,
            released: Vec::new(),
        }
    }

    /// Number of frames rendered so far.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_engine_bridge::testing::MockEngine;
    /// use martensite_engine_bridge::BridgeHandle;
    ///
    /// let handle = BridgeHandle::new();
    /// let surface = handle.lock().register();
    /// let engine = MockEngine::new(handle, surface, [0, 0, 0, 255]);
    /// assert_eq!(engine.frame_count(), 0);
    /// ```
    pub fn frame_count(&self) -> u64 {
        self.frame_count
    }

    /// The surface this engine is bound to.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_engine_bridge::testing::MockEngine;
    /// use martensite_engine_bridge::BridgeHandle;
    ///
    /// let handle = BridgeHandle::new();
    /// let surface = handle.lock().register();
    /// let engine = MockEngine::new(handle, surface, [0, 0, 0, 255]);
    /// assert_eq!(engine.surface(), surface);
    /// ```
    pub fn surface(&self) -> SurfaceId {
        self.surface
    }

    /// Creates the slot texture for `size` on `device`.
    fn make_texture(&self, device: &wgpu::Device, size: (u32, u32)) -> wgpu::Texture {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some("mock-engine-frame"),
            size: wgpu::Extent3d {
                width: size.0.max(1),
                height: size.1.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        })
    }
}

impl Engine for MockEngine {
    fn render(&mut self, ctx: &mut EngineContext, viewport: Viewport) -> Option<Box<dyn Frame>> {
        let (slot, token) = self.handle.lock().acquire(self.surface).ok()?;
        let texture = self.make_texture(ctx.device, viewport.size);

        // Fill the texture with the deterministic frame color so tests
        // can distinguish successive frames by pixel content.
        let (w, h) = viewport.size;
        let mut color = self.base_color;
        color[0] = color[0].wrapping_add(self.frame_count as u8);
        let mut pixels = vec![0u8; (w.max(1) * h.max(1) * 4) as usize];
        for px in pixels.as_chunks_mut::<4>().0 {
            px.copy_from_slice(&color);
        }
        ctx.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(w.max(1) * 4),
                rows_per_image: Some(h.max(1)),
            },
            wgpu::Extent3d {
                width: w.max(1),
                height: h.max(1),
                depth_or_array_layers: 1,
            },
        );

        self.handle
            .lock()
            .mark_ready_sized(self.surface, slot, viewport.size)
            .ok()?;
        self.frame_count += 1;
        Some(Box::new(TextureFrame::new(token, texture, viewport.size)))
    }

    fn release(&mut self, token: FrameToken) {
        self.released.push(token);
    }

    fn to_pixmap(&self, _token: FrameToken) -> Option<CpuFrame> {
        // The mock rasterizes trivially: a 4x4 block of its base color.
        let mut pixels = vec![0u8; 4 * 4 * 4];
        for px in pixels.as_chunks_mut::<4>().0 {
            px.copy_from_slice(&self.base_color);
        }
        Some(CpuFrame::new(4, 4, pixels))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BridgeRegistry;

    #[test]
    fn mock_engine_registers_and_reports() {
        let handle = BridgeHandle::new();
        let surface = handle.lock().register();
        let engine = MockEngine::new(handle, surface, [10, 20, 30, 255]);
        assert_eq!(engine.surface(), surface);
        assert_eq!(engine.frame_count(), 0);
        assert!(engine.to_pixmap(FrameToken(0)).is_some());
    }

    #[test]
    fn registry_full_lifecycle() {
        let mut registry = BridgeRegistry::new();
        let id = registry.register();
        let (slot, token) = registry.acquire(id).unwrap();
        registry.mark_ready(id, slot).unwrap();
        assert_eq!(registry.drain_ready(), vec![id]);
        let (front, front_token) = registry.take_front(id).unwrap().unwrap();
        assert_eq!(token, front_token);
        registry.release(id, front).unwrap();
        assert_eq!(registry.drain_released(id).unwrap(), vec![token]);
    }

    #[test]
    fn ring_mailbox_drops_stale_ready() {
        let mut ring = crate::SurfaceRing::new();
        let (s0, _t0) = ring.acquire().unwrap();
        ring.mark_ready(s0).unwrap();
        // Produce a second frame before the host consumes the first:
        // slot 0's ready frame becomes stale and is freed.
        let (s1, t1) = ring.acquire().unwrap();
        ring.mark_ready(s1).unwrap();
        let (front, front_token) = ring.take_front().unwrap();
        assert_eq!(front, s1);
        assert_eq!(front_token, t1);
        // The stale slot was freed — a third acquire succeeds.
        assert!(ring.acquire().is_ok());
    }

    #[test]
    fn ring_exhausted_when_both_slots_busy() {
        let mut ring = crate::SurfaceRing::new();
        let _ = ring.acquire().unwrap();
        let _ = ring.acquire().unwrap();
        assert!(matches!(
            ring.acquire(),
            Err(crate::BridgeError::RingExhausted)
        ));
    }

    #[test]
    fn invalid_transitions_are_rejected() {
        let mut ring = crate::SurfaceRing::new();
        assert!(matches!(
            ring.mark_ready(0),
            Err(crate::BridgeError::InvalidTransition)
        ));
        assert!(matches!(
            ring.release(0),
            Err(crate::BridgeError::InvalidTransition)
        ));
        assert!(matches!(
            ring.mark_ready(7),
            Err(crate::BridgeError::InvalidSlot(7))
        ));
    }
}
