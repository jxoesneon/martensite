//! **Tier 2 (feature `godot-gpu-copy`, experimental): shared-texture
//! blit.**
//!
//! The host allocates a shareable GPU texture (`HEAP_FLAG_SHARED` /
//! exportable Vulkan memory / IOSurface-backed Metal), hands the native
//! image handle to [`MartensiteSharedBlit::set_shared_image`], which
//! imports it into Godot via
//! `RenderingDevice::texture_create_from_extension`. As a
//! `CompositorEffect` registered on a viewport's `Compositor`, the blit
//! then `texture_copy`s the scene's color buffer into the imported
//! texture every `POST_TRANSPARENT` callback.
//!
//! # Honesty
//!
//! This path is **one GPU→GPU copy per frame — still not zero-copy**,
//! and it is platform-unsafe by nature (the `image` handle is an opaque
//! `VkImage`/equivalent whose validity is the host's problem).
//! Synchronization is best-effort: Godot exposes no public
//! fence/semaphore API to GDExtension, so the host tolerates up to one
//! frame of delay (sample the texture only after the next `frame_post_draw`).
//!
//! Requires the `godot` crate's `experimental-godot-api` feature —
//! `CompositorEffect` is an experimental Godot API surface — which the
//! `godot-gpu-copy` feature of this crate enables.

use godot::builtin::Rid;
use godot::classes::compositor_effect::EffectCallbackType;
use godot::classes::rendering_device::{DataFormat, TextureSamples, TextureType, TextureUsageBits};
use godot::classes::{
    CompositorEffect, ICompositorEffect, RenderData, RenderSceneBuffersRd, RenderingServer,
};
use godot::global::godot_warn;
use godot::prelude::*;

/// `CompositorEffect` that blits the rendered scene color into an
/// imported shared texture. See module docs for the contract.
#[derive(GodotClass)]
#[class(base = CompositorEffect)]
pub struct MartensiteSharedBlit {
    base: Base<CompositorEffect>,

    /// RD RID of the imported shared texture, once `set_shared_image`
    /// succeeded.
    target: Option<Rid>,

    /// Pixel size of the imported texture (the copy extent).
    target_size: (u32, u32),
}

#[godot_api]
impl MartensiteSharedBlit {
    /// Imports a host-allocated native image (`VkImage` on Vulkan —
    /// `RenderingDevice::texture_get_native_handle`-style opaque value,
    /// passed as a GDScript `int` and bit-cast to `u64`) as a Godot RD
    /// texture sized `width`×`height` RGBA8.
    ///
    /// The imported texture gets `CAN_COPY_TO | SAMPLING |
    /// COLOR_ATTACHMENT` usage. The `Rid` is freed via `free_rid` when a
    /// new image is imported or on `exit`; the underlying image stays
    /// owned by the host.
    #[func]
    pub fn set_shared_image(&mut self, image: i64, width: i64, height: i64) {
        let Some(mut rd) = RenderingServer::singleton().get_rendering_device() else {
            godot_warn!("MartensiteSharedBlit: no RenderingDevice — shared blit unavailable");
            return;
        };
        let usage = TextureUsageBits::CAN_COPY_TO_BIT
            | TextureUsageBits::SAMPLING_BIT
            | TextureUsageBits::COLOR_ATTACHMENT_BIT;
        let rid = rd.texture_create_from_extension(
            TextureType::TYPE_2D,
            DataFormat::R8G8B8A8_UNORM,
            TextureSamples::SAMPLES_1,
            usage,
            image as u64,
            width.max(0) as u64,
            height.max(0) as u64,
            1, // depth
            1, // layers
        );
        if !rid.is_valid() {
            godot_warn!("MartensiteSharedBlit: texture_create_from_extension returned invalid RID");
            return;
        }
        if let Some(old) = self.target.take() {
            rd.free_rid(old);
        }
        self.target = Some(rid);
        self.target_size = (width.max(0) as u32, height.max(0) as u32);
    }

    /// Releases the imported texture RID (the host's image itself is
    /// untouched).
    #[func]
    pub fn clear_shared_image(&mut self) {
        if let (Some(rid), Some(mut rd)) = (
            self.target.take(),
            RenderingServer::singleton().get_rendering_device(),
        ) {
            rd.free_rid(rid);
        }
        self.target_size = (0, 0);
    }
}

#[godot_api]
impl ICompositorEffect for MartensiteSharedBlit {
    fn init(base: Base<CompositorEffect>) -> Self {
        // Blit after transparency so the shipped image contains the full
        // scene; earlier stages would miss transparent/overlay passes.
        // `to_init_gd` is the documented way to touch the (refcounted)
        // base object while it is still under construction.
        base.to_init_gd()
            .set_effect_callback_type(EffectCallbackType::POST_TRANSPARENT);
        Self {
            base,
            target: None,
            target_size: (0, 0),
        }
    }

    fn render_callback(&mut self, effect_callback_type: i32, render_data: Option<Gd<RenderData>>) {
        if effect_callback_type != EffectCallbackType::POST_TRANSPARENT.ord() {
            return;
        }
        let (Some(target), Some(render_data)) = (self.target, render_data) else {
            return;
        };
        let Some(buffers) = render_data.get_render_scene_buffers() else {
            return;
        };
        let Ok(buffers_rd) = buffers.try_cast::<RenderSceneBuffersRd>() else {
            return; // non-RD render path (Compatibility renderer)
        };
        let color = buffers_rd.get_color_texture();
        let Some(mut rd) = RenderingServer::singleton().get_rendering_device() else {
            return;
        };
        let (w, h) = self.target_size;
        rd.texture_copy(
            color,
            target,
            Vector3::ZERO,
            Vector3::ZERO,
            Vector3::new(w as f32, h as f32, 1.0),
            0, // src mip
            0, // dst mip
            0, // src layer
            0, // dst layer
        );
    }
}
