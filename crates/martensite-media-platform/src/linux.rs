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

    // The full implementation would:
    // 1. Obtain the raw Vulkan device from the wgpu HAL backend
    // 2. Query the Vulkan external memory properties for the dma-buf fd
    // 3. Create a VkImage with external memory import
    // 4. Import the dma-buf as Vulkan memory
    // 5. Bind the memory to the VkImage
    // 6. Wrap the VkImage as a wgpu-hal Vulkan Texture via texture_from_raw
    // 7. Wrap the hal Texture as a wgpu::Texture via create_texture_from_hal
    //
    // The wgpu 30 HAL is not publicly exposed, so the Vulkan backend
    // integration requires an upstream API addition or a direct Vulkan
    // FFI path. This stub returns an error until that lands.

    let _ = (device, fd, stride, offset, modifier, desc);
    Err(MediaError::ImportFailed(
        "dma-buf import not yet fully implemented".to_string(),
    ))
}
