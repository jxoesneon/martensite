//! Windows DXGI shared NT handle → `wgpu::Texture` zero-copy import via the
//! DX12 backend.
//!
//! # Safety
//!
//! All `unsafe` blocks in this module call DXGI/D3D12 functions that are
//! documented in the Windows SDK:
//! - `ID3D12Device::OpenSharedHandle`:
//!   <https://learn.microsoft.com/en-us/windows/win32/api/d3d12/nf-d3d12-id3d12device-opensharedhandle>

use crate::surface::MediaError;

use crate::ImportTextureDescriptor;

/// Imports a DXGI shared NT handle into a [`wgpu::Texture`] on Windows using
/// the DX12 backend.
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

    // Obtain the raw D3D12 device from the wgpu hal backend.
    let hal_device_guard = device.as_hal::<wgpu::hal::api::Dx12>().ok_or_else(|| {
        MediaError::ImportFailed("device is not backed by the DX12 backend".to_string())
    })?;

    // Open the shared handle as a D3D12 resource.
    // SAFETY: The handle is a valid DXGI shared NT handle created with
    // D3D12_RESOURCE_FLAG_ALLOW_SHARED_KEYEDMUTEX. The caller guarantees
    // that the handle is accessible from this process.
    let d3d12_resource = unsafe {
        // In a full implementation, this would call:
        // hal_device_guard.raw_device().OpenSharedHandle(handle, ...)
        // to obtain an ID3D12Resource.
        // For now, we return an error since the full D3D12 interop requires
        // additional COM initialization that is platform-specific.
        return Err(MediaError::ImportFailed(
            "DXGI shared handle import requires D3D12 COM initialization".to_string(),
        ));
    };
    // Suppress unused variable warning.
    let _ = d3d12_resource;

    // The full implementation would:
    // 1. Open the shared handle as an ID3D12Resource
    // 2. Wrap it as a wgpu-hal Dx12 Texture via texture_from_raw
    // 3. Wrap the hal Texture as a wgpu::Texture via create_texture_from_hal

    let wgpu_format = crate::luma_texture_format(desc.format);
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

    // Placeholder: would call device.create_texture_from_hal::<wgpu::hal::api::Dx12>(...)
    let _ = wgpu_desc;
    Err(MediaError::ImportFailed(
        "DXGI import not yet fully implemented".to_string(),
    ))
}
