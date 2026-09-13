//! Windows DXGI shared-NT-handle → `wgpu::Texture` zero-copy import via the
//! Vulkan backend.
//!
//! The import is implemented on top of wgpu-hal's
//! [`wgpu::hal::vulkan::Device::texture_from_d3d11_shared_handle`], which
//! performs the `VK_KHR_external_memory_win32` import internally:
//!
//! 1. `VkImageCreateInfo` with `VkExternalMemoryImageCreateInfo`
//!    (`VK_EXTERNAL_MEMORY_HANDLE_TYPE_D3D11_TEXTURE_BIT`).
//! 2. `vkAllocateMemory` with `VkImportMemoryWin32HandleInfoKHR` (+ a
//!    chained `VkMemoryDedicatedAllocateInfo`), then `vkBindImageMemory`.
//! 3. The `VkImage` is wrapped as a `wgpu::hal::vulkan::Texture` with
//!    `TextureMemory::Dedicated`, then re-wrapped as a [`wgpu::Texture`]
//!    via [`wgpu::Device::create_texture_from_hal`].
//!
//! # Requirements
//!
//! - The [`wgpu::Device`] must use the **Vulkan** backend (request
//!   `wgpu::Backends::VULKAN` on `wgpu::Instance::new`) and must have been
//!   created with [`wgpu::Features::VULKAN_EXTERNAL_MEMORY_WIN32`]. The
//!   default DX12 backend cannot import D3D11 shared textures through
//!   wgpu-hal — `OpenSharedHandle` only accepts handles created by D3D12
//!   itself, and D3D11→D3D12 interop requires `D3D11On12` plumbing that
//!   wgpu-hal does not expose.
//! - The imported resource must be a D3D11 texture whose shared handle was
//!   created with `IDXGIResource1::CreateSharedHandle` (NT handle), which is
//!   exactly what `MediaFoundationDecoder::export_dxgi` produces.
//!
//! # Multi-plane limitation
//!
//! `texture_from_d3d11_shared_handle` binds the whole allocation to a
//! single-format `VkImage` — it offers no per-plane offset, so a bi-planar
//! D3D11 NV12/P010 texture cannot be split into `R8Unorm`/`Rg8Unorm` plane
//! textures. Such imports fail with
//! [`MediaError::UnsupportedFormat`]; callers should fall back to
//! [`crate::import_cpu_memory`] (the MF decoder already emits
//! [`HardwareHandle::CpuMemory`] frames when a shared handle is not
//! requested).
//!
//! # Safety
//!
//! All `unsafe` blocks call Vulkan external-memory import functions
//! documented in the Vulkan specification:
//! - `VK_KHR_external_memory_win32`:
//!   <https://registry.khronos.org/vulkan/specs/latest/man/html/VK_KHR_external_memory_win32.html>

use crate::surface::{MediaError, VideoPixelFormat};
use crate::ImportTextureDescriptor;

/// Imports a DXGI shared NT handle into a [`wgpu::Texture`] on Windows using
/// the Vulkan backend.
///
/// See [`crate::import_dxgi_texture`] for the public API documentation.
pub(crate) fn import_dxgi_texture(
    device: &wgpu::Device,
    handle: usize,
    desc: &ImportTextureDescriptor,
) -> Result<wgpu::Texture, MediaError> {
    if handle == 0 {
        return Err(MediaError::InvalidHandle);
    }
    if desc.width == 0 || desc.height == 0 {
        return Err(MediaError::InvalidBufferDimensions {
            width: desc.width,
            height: desc.height,
        });
    }

    // Single-plane formats only: the Win32 import binds the entire
    // allocation to one image format, with no per-plane offset, so bi-planar
    // NV12/P010 cannot be split into separate luma/chroma textures.
    let wgpu_format = match desc.format {
        VideoPixelFormat::Rgba8 | VideoPixelFormat::Rgba16Float => {
            desc.plane_wgpu_format(desc.plane_index)
        }
        VideoPixelFormat::Nv12 | VideoPixelFormat::P010 => {
            return Err(MediaError::UnsupportedFormat(desc.format));
        }
    }
    .ok_or(MediaError::InvalidHandle)?;

    if !device
        .features()
        .contains(wgpu::Features::VULKAN_EXTERNAL_MEMORY_WIN32)
    {
        return Err(MediaError::ImportFailed(
            "DXGI import requires wgpu::Features::VULKAN_EXTERNAL_MEMORY_WIN32 \
             (Vulkan backend) to be requested at device creation"
                .to_string(),
        ));
    }

    // SAFETY: `as_hal` only reads the backend device; nothing wgpu tracks is
    // destroyed or mutated through the returned guard.
    let hal_device = unsafe { device.as_hal::<wgpu::hal::api::Vulkan>() }.ok_or_else(|| {
        MediaError::ImportFailed(
            "DXGI import requires a Vulkan-backend device on Windows".to_string(),
        )
    })?;

    let hal_desc = wgpu::hal::TextureDescriptor {
        label: Some("martensite-dxgi-import"),
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
        // readback of imported textures stays possible.
        usage: wgpu::TextureUses::RESOURCE | wgpu::TextureUses::COPY_SRC,
        memory_flags: wgpu::hal::MemoryFlags::empty(),
        view_formats: Vec::new(),
    };

    // SAFETY: the caller guarantees `handle` is a live D3D11 shared NT
    // handle whose resource matches `hal_desc`; the required device feature
    // was verified above. Vulkan duplicates the handle internally, so the
    // caller retains ownership.
    let hal_texture = unsafe {
        hal_device.texture_from_d3d11_shared_handle(
            // wgpu-hal names the `windows` 0.62 HANDLE type; ours is 0.61.
            // Both are `#[repr(transparent)]` wrappers over `*mut c_void`.
            windows_0_62::Win32::Foundation::HANDLE(handle as _),
            &hal_desc,
        )
    }
    .map_err(|e| MediaError::ImportFailed(format!("vulkan DXGI import failed: {e:?}")))?;

    let wgpu_desc = wgpu::TextureDescriptor {
        label: Some("martensite-dxgi-import"),
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
