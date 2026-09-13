//! Ring-slot texture pool and `ManualTextureViews` registration.
//!
//! The bridge ring has two slots; the engine owns one [`wgpu::Texture`] per
//! slot, created on the host's `wgpu::Device`. Before each Bevy update the
//! acquired slot's texture is registered as a
//! [`ManualTextureView`](bevy::render::texture::ManualTextureView) under the
//! fixed [`CAMERA_VIEW_HANDLE`], so the engine camera always renders into the
//! slot the ring is about to publish — the host then samples that texture
//! directly. Zero copies.

use std::sync::Arc;

use bevy::camera::ManualTextureViewHandle;
use bevy::ecs::world::World;
use bevy::math::UVec2;
use bevy::render::texture::{ManualTextureView, ManualTextureViews};
use martensite_engine_bridge::SharedTexture;

/// The [`ManualTextureViewHandle`] the engine's `Camera3d` renders into.
///
/// `0x4D415254` — ASCII `"MART"`. The value is fixed so the engine camera is
/// spawned once at construction with
/// `RenderTarget::TextureView(CAMERA_VIEW_HANDLE)`; only the view payload is
/// re-pointed at the acquired ring slot's texture each frame.
///
/// Hosts spawning additional cameras through
/// [`BevyEngine::with_app`](crate::BevyEngine::with_app) may target the same
/// handle (the last camera in render order wins) or register their own
/// handles in
/// [`ManualTextureViews`].
///
/// # Examples
///
/// ```
/// use martensite_bevy::CAMERA_VIEW_HANDLE;
///
/// assert_eq!(CAMERA_VIEW_HANDLE.0, 0x4D41_5254);
/// ```
pub const CAMERA_VIEW_HANDLE: ManualTextureViewHandle = ManualTextureViewHandle(0x4D41_5254);

/// The two ring-slot textures plus their shared creation parameters.
///
/// Textures are `Arc`-shared ([`SharedTexture`]): a clone rides inside every
/// published frame, so the ring dropping its `Box<dyn Frame>` on `release` is
/// never the texture's last owner — the slot keeps its own `Arc`.
pub(crate) struct SlotPool {
    device: wgpu::Device,
    format: wgpu::TextureFormat,
    size: (u32, u32),
    slots: [SharedTexture; 2],
}

impl SlotPool {
    /// Creates both slot textures at `size` on `device`.
    pub(crate) fn new(
        device: &wgpu::Device,
        size: (u32, u32),
        format: wgpu::TextureFormat,
    ) -> Self {
        let size = clamp_size(size);
        Self {
            slots: [
                create_texture(device, size, format),
                create_texture(device, size, format),
            ],
            device: device.clone(),
            format,
            size,
        }
    }

    /// The size the slot textures were last allocated at.
    pub(crate) fn size(&self) -> (u32, u32) {
        self.size
    }

    /// The device the slot textures are allocated on.
    pub(crate) fn device(&self) -> &wgpu::Device {
        &self.device
    }

    /// The texture bound to ring `slot` (`0` or `1`).
    pub(crate) fn slot(&self, slot: u8) -> &SharedTexture {
        &self.slots[usize::from(slot.min(1))]
    }

    /// Recreates both slot textures if `size` differs from the current
    /// allocation. Returns `true` when the pool was resized.
    ///
    /// Old textures are dropped from the pool, but any `Arc` clone already
    /// published into the ring (or being composited by the host) keeps the
    /// old allocation alive — the ring records each frame's own size, so a
    /// stale-size in-flight frame remains valid.
    pub(crate) fn ensure_size(&mut self, size: (u32, u32)) -> bool {
        let size = clamp_size(size);
        if size == self.size {
            return false;
        }
        self.slots = [
            create_texture(&self.device, size, self.format),
            create_texture(&self.device, size, self.format),
        ];
        self.size = size;
        true
    }

    /// Registers `slot`'s texture as a [`ManualTextureView`] under `handle`
    /// in `world`'s `ManualTextureViews` resource.
    ///
    /// Must run before `App::update`: `camera_system` resolves the camera's
    /// target size from this entry in `PostUpdate`, and the extract stage
    /// copies it into the render world for the render graph.
    pub(crate) fn register_view(
        &self,
        world: &mut World,
        handle: ManualTextureViewHandle,
        slot: u8,
    ) {
        let texture = self.slot(slot);
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        world
            .resource_mut::<ManualTextureViews>()
            .insert(handle, self.make_view(view));
    }

    /// Builds the [`ManualTextureView`] descriptor for a `wgpu` view of this
    /// pool's format/size.
    pub(crate) fn make_view(&self, view: wgpu::TextureView) -> ManualTextureView {
        ManualTextureView {
            texture_view: view.into(),
            size: UVec2::new(self.size.0, self.size.1),
            view_format: self.format,
        }
    }
}

/// Clamps a viewport size into the valid texture range: at least `1` (wgpu
/// rejects zero-sized textures) and at most `MAX_FRAME_DIM` per axis.
pub(crate) fn clamp_size(size: (u32, u32)) -> (u32, u32) {
    (
        size.0.clamp(1, martensite_engine_bridge::MAX_FRAME_DIM),
        size.1.clamp(1, martensite_engine_bridge::MAX_FRAME_DIM),
    )
}

/// Allocates one slot texture: `RENDER_ATTACHMENT` so Bevy's render graph can
/// draw into it, `TEXTURE_BINDING` so the host composite pass can sample it,
/// `COPY_SRC` so [`to_pixmap`](martensite_engine_bridge::Engine::to_pixmap)
/// can read it back.
fn create_texture(
    device: &wgpu::Device,
    size: (u32, u32),
    format: wgpu::TextureFormat,
) -> SharedTexture {
    Arc::new(device.create_texture(&wgpu::TextureDescriptor {
        label: Some("martensite-bevy-viewport"),
        size: wgpu::Extent3d {
            width: size.0,
            height: size.1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_size_floors_at_one_and_caps_at_max() {
        assert_eq!(clamp_size((0, 0)), (1, 1));
        assert_eq!(clamp_size((800, 600)), (800, 600));
        let max = martensite_engine_bridge::MAX_FRAME_DIM;
        assert_eq!(clamp_size((max + 1, u32::MAX)), (max, max));
    }

    #[test]
    fn camera_view_handle_is_fixed() {
        assert_eq!(CAMERA_VIEW_HANDLE.0, 0x4D41_5254);
    }
}
