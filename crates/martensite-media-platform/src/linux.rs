//! Linux DRM `dma-buf` → `wgpu::Texture` zero-copy import via the Vulkan
//! backend.
//!
//! The import is implemented on top of wgpu-hal's
//! [`wgpu::hal::vulkan::Device::texture_from_dmabuf_fd`], which performs the
//! full `VK_EXT_external_memory_dma_buf` + `VK_EXT_image_drm_format_modifier`
//! dance internally:
//!
//! 1. `VkImageCreateInfo` with `VkExternalMemoryImageCreateInfo`
//!    (`VK_EXTERNAL_MEMORY_HANDLE_TYPE_DMA_BUF_BIT_EXT`) and
//!    `VkImageDrmFormatModifierExplicitCreateInfoEXT` carrying the modifier,
//!    plane offset, and row pitch (tiling `VK_IMAGE_TILING_DRM_FORMAT_MODIFIER_EXT`).
//! 2. `vkGetMemoryFdPropertiesKHR` on the `dma-buf` fd to obtain the allowed
//!    memory-type bits.
//! 3. `vkAllocateMemory` with `VkImportMemoryFdInfoKHR` + a
//!    `VkMemoryDedicatedAllocateInfo` chained to the image, then
//!    `vkBindImageMemory`.
//! 4. The `VkImage` is wrapped as a `wgpu::hal::vulkan::Texture` with
//!    `TextureMemory::Dedicated`, then re-wrapped as a [`wgpu::Texture`] via
//!    [`wgpu::Device::create_texture_from_hal`].
//!
//! # Requirements
//!
//! The [`wgpu::Device`] must have been requested with
//! [`wgpu::Features::VULKAN_EXTERNAL_MEMORY_DMA_BUF`]; without it the hal
//! refuses the import (the feature gates `VK_EXT_external_memory_dma_buf` and
//! `VK_EXT_image_drm_format_modifier`). The caller must also have requested
//! the Vulkan backend — the function returns an error on other backends.
//!
//! # Limitations
//!
//! `texture_from_dmabuf_fd` is single-plane: each call imports exactly one
//! plane described by `(fd, offset, stride, modifier)`. `desc.plane_index`
//! selects the pixel-format interpretation (`R8Unorm`/`R16Unorm` luma vs
//! `Rg8Unorm`/`Rg16Unorm` chroma) and the (already plane-adjusted) extent from
//! [`ImportTextureDescriptor::for_plane`]; the caller is responsible for
//! placing *that* plane's byte offset and pitch in the
//! [`HardwareHandle::DmaBuf`](crate::surface::HardwareHandle::DmaBuf) handle.
//!
//! # Safety
//!
//! All `unsafe` blocks in this module call Vulkan external memory import
//! functions that are documented in the Vulkan specification:
//! - `vkGetMemoryFdPropertiesKHR`:
//!   <https://registry.khronos.org/vulkan/specs/latest/man/html/vkGetMemoryFdPropertiesKHR.html>
//! - `VK_EXT_image_drm_format_modifier`:
//!   <https://registry.khronos.org/vulkan/specs/latest/man/html/VK_EXT_image_drm_format_modifier.html>

use std::os::fd::BorrowedFd;

use crate::surface::MediaError;
use crate::ImportTextureDescriptor;

/// Imports a DRM `dma-buf` file descriptor into a [`wgpu::Texture`] on Linux
/// using the Vulkan backend.
///
/// See [`crate::import_dmabuf`] for the public API documentation.
pub(crate) fn import_dmabuf(
    device: &wgpu::Device,
    fd: i32,
    stride: u32,
    offset: u32,
    modifier: u64,
    desc: &ImportTextureDescriptor,
) -> Result<wgpu::Texture, MediaError> {
    if fd < 0 {
        return Err(MediaError::InvalidHandle);
    }
    if desc.width == 0 || desc.height == 0 {
        return Err(MediaError::InvalidBufferDimensions {
            width: desc.width,
            height: desc.height,
        });
    }

    // `plane_index` picks which plane of a bi-planar source is being imported:
    // it determines the texture format (luma vs interleaved chroma) while
    // `desc.width`/`desc.height` already carry the plane's extent. The
    // single-plane hal import uses the `stride`/`offset`/`modifier` from the
    // handle, which must describe that plane (caller contract).
    let wgpu_format = desc
        .plane_wgpu_format(desc.plane_index)
        .ok_or(MediaError::InvalidHandle)?;

    // The hal entry point hard-fails when the device was not requested with
    // this feature; check first so the error message is actionable.
    if !device
        .features()
        .contains(wgpu::Features::VULKAN_EXTERNAL_MEMORY_DMA_BUF)
    {
        return Err(MediaError::ImportFailed(
            "dma-buf import requires wgpu::Features::VULKAN_EXTERNAL_MEMORY_DMA_BUF \
             to be requested at device creation"
                .to_string(),
        ));
    }

    // SAFETY: `as_hal` only reads the backend device; nothing wgpu tracks is
    // destroyed or mutated through the returned guard.
    let hal_device = unsafe { device.as_hal::<wgpu::hal::api::Vulkan>() }.ok_or_else(|| {
        MediaError::ImportFailed("device is not backed by the Vulkan backend".to_string())
    })?;

    // `texture_from_dmabuf_fd` consumes an `OwnedFd` (Vulkan takes ownership
    // on success and the fd is closed on failure), so duplicate the caller's
    // fd — the `HardwareHandle::DmaBuf` remains valid and owned by the caller.
    //
    // SAFETY: the caller guarantees `fd` is a valid, open dma-buf descriptor
    // (also checked non-negative above); the borrow lives only for the
    // duration of `try_clone_to_owned`, which `dup`s it.
    let owned_fd = unsafe { BorrowedFd::borrow_raw(fd) }
        .try_clone_to_owned()
        .map_err(|e| MediaError::ImportFailed(format!("dup of dma-buf fd failed: {e}")))?;

    let hal_desc = wgpu::hal::TextureDescriptor {
        label: Some("martensite-dmabuf-import"),
        size: wgpu::Extent3d {
            width: desc.width,
            height: desc.height,
            depth_or_array_layers: desc.array_layer_count,
        },
        mip_level_count: desc.mip_level_count,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu_format,
        // SAMPLED for binding in the video pipeline + COPY_SRC so test
        // readback of imported planes stays possible.
        usage: wgpu::TextureUses::RESOURCE | wgpu::TextureUses::COPY_SRC,
        memory_flags: wgpu::hal::MemoryFlags::empty(),
        view_formats: Vec::new(),
    };

    // SAFETY: `owned_fd` is a freshly duplicated, valid dma-buf fd whose
    // layout matches `hal_desc` per the caller's contract; the required
    // device feature was verified above. On success Vulkan owns the fd and
    // the returned texture owns the imported (dedicated) memory; on failure
    // wgpu-hal closes the fd and destroys the image.
    let hal_texture = unsafe {
        hal_device.texture_from_dmabuf_fd(
            owned_fd,
            &hal_desc,
            modifier,
            u64::from(stride),
            u64::from(offset),
        )
    }
    .map_err(|e| MediaError::ImportFailed(format!("vulkan dma-buf import failed: {e:?}")))?;

    let wgpu_desc = wgpu::TextureDescriptor {
        label: Some("martensite-dmabuf-import"),
        size: wgpu::Extent3d {
            width: desc.width,
            height: desc.height,
            depth_or_array_layers: desc.array_layer_count,
        },
        mip_level_count: desc.mip_level_count,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu_format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    };

    // SAFETY: `hal_texture` was created by this device's Vulkan backend and
    // matches `wgpu_desc`; its imported memory is bound for the texture's
    // whole lifetime (wgpu-hal frees it on drop). `RESOURCE` matches the
    // actual layout a freshly imported sampled image is in.
    let texture = unsafe {
        device.create_texture_from_hal::<wgpu::hal::api::Vulkan>(
            hal_texture,
            &wgpu_desc,
            wgpu::TextureUses::RESOURCE,
        )
    };

    Ok(texture)
}
