//! Test-support producer used by the conformance suite.
//!
//! [`MockEngine`] synthesizes deterministic `wgpu::Texture` frames on the
//! host device — a solid color that increments a corner pixel per frame —
//! so tests can verify the full acquire → ready → composite → release
//! lifecycle without a real external engine.

use crate::bridge::{BridgeHandle, SurfaceId};
use crate::engine::{Engine, EngineContext, Viewport};
use crate::frame::{CpuFrame, Frame, FrameToken, SharedTexture, TextureFrame};

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
    /// Textures retained per published token — keeps each frame's
    /// `wgpu::Texture` alive until the host releases the token, and
    /// recycles them for reuse on `release`.
    pending: std::collections::HashMap<FrameToken, SharedTexture>,
    /// Freed textures available for reuse.
    pool: Vec<SharedTexture>,
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
            pending: std::collections::HashMap::new(),
            pool: Vec::new(),
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

    /// Tokens the host has released to this engine (via
    /// [`MockEngine::drain_released`]) — test accounting for the
    /// "release exactly once per composited token" gate.
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
    /// assert!(engine.released_tokens().is_empty());
    /// ```
    pub fn released_tokens(&self) -> &[FrameToken] {
        &self.released
    }

    /// Drains the bridge's released-token queue for this engine's
    /// surface and calls [`Engine::release`] for each — the producer
    /// half of the recycling contract. Call once per host frame.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_engine_bridge::testing::MockEngine;
    /// use martensite_engine_bridge::BridgeHandle;
    ///
    /// let handle = BridgeHandle::new();
    /// let surface = handle.lock().register();
    /// let mut engine = MockEngine::new(handle, surface, [0, 0, 0, 255]);
    /// engine.drain_released();
    /// ```
    pub fn drain_released(&mut self) {
        let tokens = self
            .handle
            .lock()
            .drain_released(self.surface)
            .unwrap_or_default();
        for token in tokens {
            self.release(token);
        }
    }

    /// Creates the slot texture for `size` on `device`, reusing a
    /// recycled texture when one of matching size is pooled.
    fn make_texture(&mut self, device: &wgpu::Device, size: (u32, u32)) -> SharedTexture {
        let size = (size.0.max(1), size.1.max(1));
        if let Some(pos) = self
            .pool
            .iter()
            .rposition(|t| t.width() == size.0 && t.height() == size.1)
        {
            return self.pool.remove(pos);
        }
        std::sync::Arc::new(device.create_texture(&wgpu::TextureDescriptor {
            label: Some("mock-engine-frame"),
            size: wgpu::Extent3d {
                width: size.0,
                height: size.1,
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
        }))
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
        let mut pixels = vec![0u8; w.max(1) as usize * h.max(1) as usize * 4];
        for px in pixels.as_chunks_mut::<4>().0 {
            px.copy_from_slice(&color);
        }
        ctx.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: texture.as_ref(),
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
        // Flush the write before publishing: same-queue ordering then
        // guarantees the host's composite sees the populated texture.
        ctx.queue.submit([]);

        // Publish the frame into the ring so the host consumes it via
        // `composite_front`; the CPU raster rides along for the
        // TinySkia fallback path. The engine retains the texture in
        // `pending` until `release` — the ring's drop must not be the
        // last owner.
        self.pending.insert(token, std::sync::Arc::clone(&texture));
        let cpu = self.to_pixmap(token);
        self.handle
            .lock()
            .mark_ready_full(
                self.surface,
                slot,
                Some(Box::new(TextureFrame::new(
                    token,
                    (*texture).clone(),
                    viewport.size,
                ))),
                cpu,
            )
            .ok()?;
        self.frame_count += 1;
        Some(Box::new(TextureFrame::new(
            token,
            (*texture).clone(),
            viewport.size,
        )))
    }

    fn release(&mut self, token: FrameToken) {
        self.released.push(token);
        if let Some(tex) = self.pending.remove(&token) {
            self.pool.push(tex);
        }
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
    fn release_fires_exactly_once_per_composited_token() {
        // The host composites a frame by take_front → composite →
        // release; the producer drains released tokens → Engine::release.
        let mut registry = BridgeRegistry::new();
        let id = registry.register();
        let (s0, t0) = registry.acquire(id).unwrap();
        registry.mark_ready(id, s0).unwrap();
        let (slot, token) = registry.take_front(id).unwrap().unwrap();
        assert_eq!(token, t0);
        registry.release(id, slot).unwrap();
        // Producer recycles.
        let mut seen = Vec::new();
        for tok in registry.drain_released(id).unwrap() {
            seen.push(tok);
        }
        assert_eq!(seen, vec![t0]);
        // No double-release: a second drain is empty.
        assert!(registry.drain_released(id).unwrap().is_empty());
    }

    #[test]
    fn mark_ready_full_publishes_frame_and_cpu_payload() {
        let handle = BridgeHandle::new();
        let surface = handle.lock().register();
        {
            let mut reg = handle.lock();
            let (slot, _token) = reg.acquire(surface).unwrap();
            reg.mark_ready_full(
                surface,
                slot,
                None,
                Some(CpuFrame::new(2, 2, vec![9u8; 16])),
            )
            .unwrap();
        }
        let reg = handle.lock();
        let cpu = reg.front_cpu_frame(surface).unwrap().unwrap();
        assert_eq!((cpu.width, cpu.height), (2, 2));
        assert_eq!(reg.front_size(surface).unwrap(), Some((2, 2)));
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
