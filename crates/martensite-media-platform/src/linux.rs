//! Linux DRM `dma-buf` → `wgpu::Texture` zero-copy import via the Vulkan
//! backend.
//!
//! # Safety
//!
//! All `unsafe` blocks in this module call Vulkan external memory import
//! functions that are documented in the Vulkan specification:
//! - `vkGetMemoryFdPropertiesKHR`:
//!   <https://registry.khronos.org/vulkan/specs/latest/man/html/vkGetMemoryFdPropertiesKHR.html>

use martensite_media::surface::MediaError;

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

    // Obtain the raw Vulkan device from the wgpu hal backend.
    let hal_device_guard = device.as_hal::<wgpu::hal::api::Vulkan>().ok_or_else(|| {
        MediaError::ImportFailed("device is not backed by the Vulkan backend".to_string())
    })?;

    // The full implementation would:
    // 1. Query the Vulkan external memory properties for the dma-buf fd
    // 2. Create a VkImage with external memory import
    // 3. Import the dma-buf as Vulkan memory
    // 4. Bind the memory to the VkImage
    // 5. Wrap the VkImage as a wgpu-hal Vulkan Texture via texture_from_raw
    // 6. Wrap the hal Texture as a wgpu::Texture via create_texture_from_hal

    let wgpu_format = crate::luma_texture_format(desc.format);
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

    // Placeholder: would call device.create_texture_from_hal::<wgpu::hal::api::Vulkan>(...)
    let _ = (hal_device_guard, stride, offset, modifier, wgpu_desc);
    Err(MediaError::ImportFailed(
        "dma-buf import not yet fully implemented".to_string(),
    ))
}
