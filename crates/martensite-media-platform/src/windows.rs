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
    // SAFETY: `as_hal` is unsafe because it exposes the raw hal device,
    // but we only use the guard to check backend availability.
    let _hal_device_guard = unsafe {
        device.as_hal::<wgpu::hal::api::Dx12>()
    };

    // The full implementation would:
    // 1. Open the shared handle as an ID3D12Resource via
    //    `hal_device_guard.raw_device().OpenSharedHandle(handle, ...)`
    // 2. Wrap it as a wgpu-hal Dx12 Texture via `texture_from_raw`
    // 3. Wrap the hal Texture as a `wgpu::Texture` via
    //    `device.create_texture_from_hal::<wgpu::hal::api::Dx12>(...)`
    //
    // This requires D3D12 COM initialization that is platform-specific and
    // is not yet fully implemented.

    let _ = desc;
    Err(MediaError::ImportFailed(
        "DXGI shared handle import requires D3D12 COM initialization".to_string(),
    ))
}
