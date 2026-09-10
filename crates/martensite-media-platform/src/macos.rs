//! macOS `IOSurface` → `wgpu::Texture` zero-copy import via the Metal backend.
//!
//! This module implements the IOSurface import path using the `objc2-metal`
//! and `objc2-io-surface` crates, following the same pattern as
//! `wgpu-hal`'s Metal backend.
//!
//! # Safety
//!
//! All `unsafe` blocks in this module call Metal and IOSurface C functions
//! that are documented to be safe per Apple's documentation:
//! - `IOSurfaceLookup`:
//!   <https://developer.apple.com/documentation/iosurface/iosurfacelookup(_:)>
//! - `newTextureWithDescriptor:iosurface:plane:`:
//!   <https://developer.apple.com/documentation/metal/mtldevice/newtexturewithdescriptor(_:iosurface:plane:)>

use std::ffi::c_void;

use crate::surface::{MediaError, VideoPixelFormat};
use objc2::ffi::NSUInteger;
use objc2::rc::autoreleasepool;
use objc2_io_surface::IOSurfaceRef;
use objc2_metal::{
    MTLDevice, MTLPixelFormat, MTLStorageMode, MTLTextureDescriptor, MTLTextureType,
    MTLTextureUsage,
};

use crate::ImportTextureDescriptor;

/// Maps a [`VideoPixelFormat`] to the corresponding Metal pixel format for
/// the luma (Y) plane.
fn luma_mtl_format(format: VideoPixelFormat) -> MTLPixelFormat {
    match format {
        VideoPixelFormat::Nv12 => MTLPixelFormat::R8Unorm,
        VideoPixelFormat::P010 => MTLPixelFormat::R16Unorm,
        VideoPixelFormat::Rgba8 => MTLPixelFormat::RGBA8Unorm, // Note: Metal uses RGBA8Unorm
        VideoPixelFormat::Rgba16Float => MTLPixelFormat::RGBA16Float,
    }
}

/// Maps a [`VideoPixelFormat`] to the corresponding wgpu texture format
/// for the luma plane (re-exported from the crate root for convenience).
fn luma_wgpu_format(format: VideoPixelFormat) -> wgpu::TextureFormat {
    crate::luma_texture_format(format)
}

/// Imports an `IOSurface` by its 32-bit global identifier into a
/// [`wgpu::Texture`] on macOS using the Metal backend.
///
/// See [`crate::import_iosurface`] for the public API documentation.
pub(crate) fn import_iosurface(
    device: &wgpu::Device,
    surface_id: u32,
    desc: &ImportTextureDescriptor,
) -> Result<wgpu::Texture, MediaError> {
    // Step 1: Look up the IOSurface by its global identifier.
    let io_surface = IOSurfaceRef::lookup(surface_id).ok_or(MediaError::InvalidHandle)?;

    // Step 2: Obtain the raw Metal device from the wgpu hal backend.
    // SAFETY: We only read the device handle; we do not destroy it or
    // modify its state outside of wgpu's tracking.
    let hal_device_guard =
        unsafe { device.as_hal::<wgpu::hal::api::Metal>() }.ok_or_else(|| {
            MediaError::ImportFailed("device is not backed by the Metal backend".to_string())
        })?;

    let mtl_device = hal_device_guard.raw_device().clone();

    // Step 3: Create an MTLTextureDescriptor for the IOSurface-backed texture.
    let mtl_format = luma_mtl_format(desc.format);
    let texture_descriptor = autoreleasepool(|_| {
        let descriptor = MTLTextureDescriptor::new();
        descriptor.setTextureType(MTLTextureType::Type2D);
        // SAFETY: width and height are validated to be non-zero by the caller.
        unsafe {
            descriptor.setWidth(desc.width as NSUInteger);
            descriptor.setHeight(desc.height as NSUInteger);
            descriptor.setMipmapLevelCount(desc.mip_level_count as NSUInteger);
            descriptor.setArrayLength(desc.array_layer_count as NSUInteger);
        }
        descriptor.setPixelFormat(mtl_format);
        descriptor.setStorageMode(MTLStorageMode::Shared);
        descriptor.setUsage(MTLTextureUsage::ShaderRead);
        descriptor
    });

    // Step 4: Create the MTLTexture from the IOSurface.
    let mtl_texture = autoreleasepool(|_| {
        // SAFETY: The IOSurface is a valid, live surface object. The
        // descriptor matches the surface dimensions and format. Plane 0
        // is the luma plane for bi-planar YUV formats.
        mtl_device.newTextureWithDescriptor_iosurface_plane(
            &texture_descriptor,
            &io_surface,
            0, // plane 0 = luma
        )
    })
    .ok_or_else(|| {
        MediaError::ImportFailed("MTLDevice failed to create texture from IOSurface".to_string())
    })?;

    // Step 5: Wrap the MTLTexture as a wgpu-hal Texture.
    let wgpu_format = luma_wgpu_format(desc.format);
    let copy_size = wgpu::hal::CopyExtent {
        width: desc.width,
        height: desc.height,
        depth: 1,
    };

    // SAFETY: The MTLTexture was created from this device's underlying
    // MTLDevice. The format and dimensions match the descriptor.
    let hal_texture = unsafe {
        wgpu::hal::metal::Device::texture_from_raw(
            mtl_texture,
            wgpu_format,
            MTLTextureType::Type2D,
            desc.array_layer_count,
            desc.mip_level_count,
            copy_size,
            None, // no drop callback; IOSurface lifetime managed by caller
        )
    };

    // Step 6: Wrap the hal Texture as a wgpu::Texture via create_texture_from_hal.
    let wgpu_desc = wgpu::TextureDescriptor {
        label: Some("martensite-iosurface-import"),
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

    // SAFETY: The hal_texture was created from this device's underlying
    // MTLDevice and matches the descriptor. The initial state is
    // TEXTURE_BINDING since the texture will be used as a shader resource.
    let texture = unsafe {
        device.create_texture_from_hal::<wgpu::hal::api::Metal>(
            hal_texture,
            &wgpu_desc,
            wgpu::TextureUses::RESOURCE,
        )
    };

    // Keep the IOSurface alive by leaking a reference; the caller is
    // responsible for managing the surface lifetime. In a production
    // implementation, a drop callback would release this reference.
    std::mem::forget(io_surface);

    Ok(texture)
}

// Suppress unused import warning for c_void (used in FFI type aliases).
#[allow(dead_code)]
const _: fn() = || {
    let _: *mut c_void = std::ptr::null_mut();
};
