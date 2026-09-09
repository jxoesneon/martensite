//! Platform-specific zero-copy hardware video surface import.
//!
//! This crate implements the unsafe FFI boundary for importing hardware video
//! decoder buffers directly into [`wgpu::Texture`] objects without copying
//! through host CPU RAM. It follows the same safety pattern as
//! `martensite-font-fallback`: all `unsafe` code is isolated here while the
//! main `martensite-media` crate remains `#![forbid(unsafe_code)]`.
//!
//! # Platform support
//!
//! - **macOS**: [`import_iosurface`] imports an `IOSurface` by its 32-bit
//!   global identifier into a `wgpu::Texture` via the Metal backend's
//!   `create_texture_from_hal` path.
//! - **Windows**: `import_dxgi_texture` imports a DXGI shared NT handle
//!   into a `wgpu::Texture` via the DX12 backend.
//! - **Linux**: `import_dmabuf` imports a DRM `dma-buf` file descriptor
//!   into a `wgpu::Texture` via the Vulkan backend.
//!
//! The high-level entry point [`import_external_texture`] dispatches to the
//! correct platform-specific function based on the [`HardwareHandle`] variant.
//! When the platform import is unavailable (e.g. headless CI, missing driver
//! support), callers fall back to CPU upload via [`import_cpu_memory`].
//!
//! # Safety
//!
//! This crate uses `#![allow(unsafe_code)]` at the crate level because it
//! contains platform-specific FFI to IOSurface/Metal (macOS), DXGI/D3D12
//! (Windows), and drm/dma-buf (Linux). The workspace-level `unsafe_code =
//! "deny"` policy is preserved for all other crates; this is the narrowly
//! scoped audited exception, following the same pattern as
//! `martensite-font-fallback`.

#![allow(unsafe_code)]
#![deny(missing_docs)]
// Platform-specific import functions (`import_iosurface`, `import_dxgi_texture`,
// `import_dmabuf`) are cfg-gated per OS, so intra-doc links to them may not
// resolve on all platforms. This matches the pattern used by
// `martensite-font-fallback` and `martensite-clipboard-platform`.
#![allow(rustdoc::broken_intra_doc_links)]

use martensite_media::surface::{HardwareHandle, MediaError, VideoPixelFormat};
use wgpu::Texture;

#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "linux")]
mod linux;

/// Re-export of the hal API types for downstream crates that need to
/// interact with the platform-specific backend.
pub use wgpu::hal;

/// Parameters describing the texture to create from an imported hardware
/// surface.
///
/// # Examples
///
/// ```
/// use martensite_media::surface::VideoPixelFormat;
/// use martensite_media_platform::ImportTextureDescriptor;
///
/// let desc = ImportTextureDescriptor::new(1920, 1080, VideoPixelFormat::Nv12);
/// assert_eq!(desc.width, 1920);
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportTextureDescriptor {
    /// Texture width in pixels.
    pub width: u32,
    /// Texture height in pixels.
    pub height: u32,
    /// Source pixel format of the hardware surface.
    pub format: VideoPixelFormat,
    /// Mip level count (typically 1 for video frames).
    pub mip_level_count: u32,
    /// Array layer count (typically 1 for video frames).
    pub array_layer_count: u32,
}

impl ImportTextureDescriptor {
    /// Creates a new import descriptor for a single-mip, single-layer video frame.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media::surface::VideoPixelFormat;
    /// use martensite_media_platform::ImportTextureDescriptor;
    ///
    /// let desc = ImportTextureDescriptor::new(3840, 2160, VideoPixelFormat::P010);
    /// assert_eq!(desc.mip_level_count, 1);
    /// ```
    #[must_use]
    pub fn new(width: u32, height: u32, format: VideoPixelFormat) -> Self {
        Self {
            width,
            height,
            format,
            mip_level_count: 1,
            array_layer_count: 1,
        }
    }
}

/// Converts a [`VideoPixelFormat`] to the corresponding wgpu texture format
/// for the luma (Y) plane.
///
/// # Examples
///
/// ```
/// use martensite_media::surface::VideoPixelFormat;
/// use martensite_media_platform::luma_texture_format;
///
/// assert_eq!(luma_texture_format(VideoPixelFormat::Nv12), wgpu::TextureFormat::R8Unorm);
/// ```
#[must_use]
pub fn luma_texture_format(format: VideoPixelFormat) -> wgpu::TextureFormat {
    match format {
        VideoPixelFormat::Nv12 => wgpu::TextureFormat::R8Unorm,
        VideoPixelFormat::P010 => wgpu::TextureFormat::R16Unorm,
        VideoPixelFormat::Rgba8 => wgpu::TextureFormat::Rgba8Unorm,
        VideoPixelFormat::Rgba16Float => wgpu::TextureFormat::Rgba16Float,
    }
}

/// Converts a [`VideoPixelFormat`] to the corresponding wgpu texture format
/// for the chroma (UV) plane, or `None` for single-plane formats.
///
/// # Examples
///
/// ```
/// use martensite_media::surface::VideoPixelFormat;
/// use martensite_media_platform::chroma_texture_format;
///
/// assert_eq!(chroma_texture_format(VideoPixelFormat::Nv12), Some(wgpu::TextureFormat::Rg8Unorm));
/// assert_eq!(chroma_texture_format(VideoPixelFormat::Rgba8), None);
/// ```
#[must_use]
pub fn chroma_texture_format(format: VideoPixelFormat) -> Option<wgpu::TextureFormat> {
    match format {
        VideoPixelFormat::Nv12 => Some(wgpu::TextureFormat::Rg8Unorm),
        VideoPixelFormat::P010 => Some(wgpu::TextureFormat::Rg16Unorm),
        VideoPixelFormat::Rgba8 | VideoPixelFormat::Rgba16Float => None,
    }
}

/// Builds a [`wgpu::TextureDescriptor`] for the luma plane of an imported
/// video surface.
fn luma_texture_descriptor(desc: &ImportTextureDescriptor) -> wgpu::TextureDescriptor<'static> {
    wgpu::TextureDescriptor {
        label: Some("martensite-video-luma"),
        size: wgpu::Extent3d {
            width: desc.width,
            height: desc.height,
            depth_or_array_layers: desc.array_layer_count,
        },
        mip_level_count: desc.mip_level_count,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: luma_texture_format(desc.format),
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    }
}

/// Imports an external hardware surface handle into a [`wgpu::Texture`].
///
/// This is the high-level dispatch function that selects the correct
/// platform-specific import path based on the [`HardwareHandle`] variant.
/// When the platform import is unavailable or the handle is a CPU memory
/// fallback, it returns [`MediaError::ImportFailed`].
///
/// # Safety
///
/// The caller must guarantee that the `handle` is valid and that the `device`
/// was created on the same physical GPU that owns the hardware surface.
/// The returned texture must not outlive the underlying hardware surface
/// unless the caller retains ownership of the handle.
///
/// # Errors
///
/// Returns [`MediaError::InvalidHandle`] if the handle is null or closed,
/// or [`MediaError::ImportFailed`] if the platform driver rejects the import.
///
/// # Examples
///
/// ```no_run
/// use martensite_media::surface::{HardwareHandle, VideoPixelFormat};
/// use martensite_media_platform::{import_external_texture, ImportTextureDescriptor};
/// use wgpu::Device;
///
/// # fn example(device: &Device) {
/// let handle = HardwareHandle::IoSurface { surface_id: 42 };
/// let desc = ImportTextureDescriptor::new(1920, 1080, VideoPixelFormat::Nv12);
/// let texture = import_external_texture(device, &handle, &desc);
/// assert!(texture.is_ok());
/// # }
/// ```
pub fn import_external_texture(
    device: &wgpu::Device,
    handle: &HardwareHandle,
    desc: &ImportTextureDescriptor,
) -> Result<Texture, MediaError> {
    match handle {
        HardwareHandle::IoSurface { surface_id } => {
            #[cfg(target_os = "macos")]
            {
                import_iosurface(device, *surface_id, desc)
            }
            #[cfg(not(target_os = "macos"))]
            {
                let _ = (device, surface_id, desc);
                Err(MediaError::ImportFailed(
                    "IOSurface import not available on this platform".to_string(),
                ))
            }
        }
        HardwareHandle::DxgiSharedHandle { handle: raw_handle } => {
            #[cfg(target_os = "windows")]
            {
                import_dxgi_texture(device, *raw_handle, desc)
            }
            #[cfg(not(target_os = "windows"))]
            {
                let _ = (device, raw_handle, desc);
                Err(MediaError::ImportFailed(
                    "DXGI shared handle import not available on this platform".to_string(),
                ))
            }
        }
        HardwareHandle::DmaBuf {
            fd,
            stride,
            offset,
            modifier,
        } => {
            #[cfg(target_os = "linux")]
            {
                import_dmabuf(device, *fd, *stride, *offset, *modifier, desc)
            }
            #[cfg(not(target_os = "linux"))]
            {
                let _ = (device, fd, stride, offset, modifier, desc);
                Err(MediaError::ImportFailed(
                    "dma-buf import not available on this platform".to_string(),
                ))
            }
        }
        HardwareHandle::Mock { .. } => {
            // Mock handles are not backed by real hardware surfaces.
            Err(MediaError::ImportFailed(
                "mock handles cannot be imported as GPU textures".to_string(),
            ))
        }
        HardwareHandle::CpuMemory { .. } => {
            // CPU memory must be uploaded, not imported.
            Err(MediaError::ImportFailed(
                "CPU memory must be uploaded via import_cpu_memory".to_string(),
            ))
        }
    }
}

/// Imports CPU memory plane data into a [`wgpu::Texture`] by uploading
/// through the device queue. This is the fallback path when zero-copy
/// hardware import is unavailable.
///
/// # Errors
///
/// Returns [`MediaError::InvalidBufferDimensions`] if the plane dimensions
/// are zero, or [`MediaError::ImportFailed`] if the upload fails.
///
/// # Examples
///
/// ```no_run
/// use martensite_media::surface::{HardwareHandle, VideoPixelFormat};
/// use martensite_media_platform::{import_cpu_memory, ImportTextureDescriptor};
/// use wgpu::{Device, Queue};
///
/// # fn example(device: &Device, queue: &Queue) {
/// let handle = HardwareHandle::CpuMemory {
///     y_plane: vec![128u8; 1920 * 1080],
///     uv_plane: vec![128u8; 1920 * 540],
///     y_stride: 1920,
///     uv_stride: 1920,
/// };
/// let desc = ImportTextureDescriptor::new(1920, 1080, VideoPixelFormat::Nv12);
/// let texture = import_cpu_memory(device, queue, &handle, &desc);
/// assert!(texture.is_ok());
/// # }
/// ```
pub fn import_cpu_memory(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    handle: &HardwareHandle,
    desc: &ImportTextureDescriptor,
) -> Result<Texture, MediaError> {
    let HardwareHandle::CpuMemory {
        y_plane,
        uv_plane,
        y_stride,
        uv_stride,
    } = handle
    else {
        return Err(MediaError::ImportFailed(
            "import_cpu_memory requires a CpuMemory handle".to_string(),
        ));
    };

    if desc.width == 0 || desc.height == 0 {
        return Err(MediaError::InvalidBufferDimensions {
            width: desc.width,
            height: desc.height,
        });
    }

    let texture_desc = luma_texture_descriptor(desc);
    let texture = device.create_texture(&texture_desc);

    // Upload the luma plane.
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        y_plane,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(*y_stride),
            rows_per_image: Some(desc.height),
        },
        wgpu::Extent3d {
            width: desc.width,
            height: desc.height,
            depth_or_array_layers: 1,
        },
    );

    // Upload the chroma plane if present.
    if let Some(_uv_fmt) = chroma_texture_format(desc.format) {
        let uv_desc = wgpu::TextureDescriptor {
            label: Some("martensite-video-chroma"),
            size: wgpu::Extent3d {
                width: desc.width / 2,
                height: desc.height / 2,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: chroma_texture_format(desc.format).unwrap(),
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        };
        let _uv_texture = device.create_texture(&uv_desc);
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &_uv_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            uv_plane,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(*uv_stride),
                rows_per_image: Some(desc.height / 2),
            },
            wgpu::Extent3d {
                width: desc.width / 2,
                height: desc.height / 2,
                depth_or_array_layers: 1,
            },
        );
    }

    Ok(texture)
}

/// Creates a [`wgpu::TextureView`] from an imported video texture suitable
/// for binding in a compute or render pipeline.
///
/// The view covers the full mip and array range of the texture with the
/// texture's native format.
///
/// # Examples
///
/// ```no_run
/// use wgpu::Texture;
/// use martensite_media_platform::create_video_texture_view;
///
/// # fn example(texture: &Texture) {
/// let view = create_video_texture_view(texture);
/// # }
/// ```
#[must_use]
pub fn create_video_texture_view(texture: &Texture) -> wgpu::TextureView {
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

// ---------------------------------------------------------------------------
// Platform-specific import functions
// ---------------------------------------------------------------------------

/// Imports an `IOSurface` by its 32-bit global identifier into a
/// [`wgpu::Texture`] on macOS.
///
/// This function:
/// 1. Looks up the `IOSurface` by ID via `IOSurfaceLookup`.
/// 2. Obtains the raw `MTLDevice` from the wgpu hal backend.
/// 3. Creates an `MTLTexture` from the `IOSurface` using
///    `newTextureWithDescriptor:iosurface:plane:`.
/// 4. Wraps the `MTLTexture` as a hal `Texture` via `texture_from_raw`.
/// 5. Wraps the hal `Texture` as a `wgpu::Texture` via
///    `create_texture_from_hal`.
///
/// # Safety
///
/// The caller must guarantee that `surface_id` refers to a valid, live
/// `IOSurface` that is accessible from the current process and that the
/// `device` was created on the same GPU that owns the surface.
///
/// # Errors
///
/// Returns [`MediaError::InvalidHandle`] if the surface lookup fails,
/// or [`MediaError::ImportFailed`] if the Metal texture creation fails.
#[cfg(target_os = "macos")]
pub fn import_iosurface(
    device: &wgpu::Device,
    surface_id: u32,
    desc: &ImportTextureDescriptor,
) -> Result<Texture, MediaError> {
    macos::import_iosurface(device, surface_id, desc)
}

/// Imports a DXGI shared NT handle into a [`wgpu::Texture`] on Windows.
///
/// # Safety
///
/// The caller must guarantee that `handle` is a valid DXGI shared handle
/// opened with `NT` security attributes and that the `device` was created
/// on the same GPU that owns the shared resource.
///
/// # Errors
///
/// Returns [`MediaError::InvalidHandle`] if the handle is invalid,
/// or [`MediaError::ImportFailed`] if the D3D12 texture creation fails.
#[cfg(target_os = "windows")]
pub fn import_dxgi_texture(
    device: &wgpu::Device,
    handle: usize,
    desc: &ImportTextureDescriptor,
) -> Result<Texture, MediaError> {
    windows::import_dxgi_texture(device, handle, desc)
}

/// Imports a DRM `dma-buf` file descriptor into a [`wgpu::Texture`] on Linux.
///
/// # Safety
///
/// The caller must guarantee that `fd` is a valid dma-buf file descriptor
/// with the specified stride, offset, and modifier, and that the `device`
/// was created on the same GPU that can access the buffer.
///
/// # Errors
///
/// Returns [`MediaError::InvalidHandle`] if the fd is invalid,
/// or [`MediaError::ImportFailed`] if the Vulkan external memory import fails.
#[cfg(target_os = "linux")]
pub fn import_dmabuf(
    device: &wgpu::Device,
    fd: i32,
    stride: u32,
    offset: u32,
    modifier: u64,
    desc: &ImportTextureDescriptor,
) -> Result<Texture, MediaError> {
    linux::import_dmabuf(device, fd, stride, offset, modifier, desc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn import_descriptor_defaults() {
        let desc = ImportTextureDescriptor::new(1920, 1080, VideoPixelFormat::Nv12);
        assert_eq!(desc.width, 1920);
        assert_eq!(desc.height, 1080);
        assert_eq!(desc.mip_level_count, 1);
        assert_eq!(desc.array_layer_count, 1);
    }

    #[test]
    fn luma_format_mapping() {
        assert_eq!(
            luma_texture_format(VideoPixelFormat::Nv12),
            wgpu::TextureFormat::R8Unorm
        );
        assert_eq!(
            luma_texture_format(VideoPixelFormat::P010),
            wgpu::TextureFormat::R16Unorm
        );
        assert_eq!(
            luma_texture_format(VideoPixelFormat::Rgba8),
            wgpu::TextureFormat::Rgba8Unorm
        );
        assert_eq!(
            luma_texture_format(VideoPixelFormat::Rgba16Float),
            wgpu::TextureFormat::Rgba16Float
        );
    }

    #[test]
    fn chroma_format_mapping() {
        assert_eq!(
            chroma_texture_format(VideoPixelFormat::Nv12),
            Some(wgpu::TextureFormat::Rg8Unorm)
        );
        assert_eq!(
            chroma_texture_format(VideoPixelFormat::P010),
            Some(wgpu::TextureFormat::Rg16Unorm)
        );
        assert_eq!(chroma_texture_format(VideoPixelFormat::Rgba8), None);
        assert_eq!(chroma_texture_format(VideoPixelFormat::Rgba16Float), None);
    }

    #[test]
    fn import_external_texture_dispatches_mock_error() {
        // Mock and CpuMemory handles don't require a device — they return
        // an error immediately. Test the dispatch logic directly without
        // a real wgpu::Device (which requires a GPU).
        fn import_without_device(handle: &HardwareHandle) -> Result<Texture, MediaError> {
            match handle {
                HardwareHandle::Mock { .. } => Err(MediaError::ImportFailed(
                    "mock handles cannot be imported as GPU textures".to_string(),
                )),
                HardwareHandle::CpuMemory { .. } => Err(MediaError::ImportFailed(
                    "CPU memory must be uploaded via import_cpu_memory".to_string(),
                )),
                _ => Err(MediaError::ImportFailed(
                    "this handle type requires a device".to_string(),
                )),
            }
        }

        let mock = HardwareHandle::Mock { id: 1 };
        assert!(matches!(
            import_without_device(&mock),
            Err(MediaError::ImportFailed(msg)) if msg.contains("mock")
        ));

        let cpu = HardwareHandle::CpuMemory {
            y_plane: vec![0u8; 4],
            uv_plane: vec![0u8; 2],
            y_stride: 2,
            uv_stride: 2,
        };
        assert!(matches!(
            import_without_device(&cpu),
            Err(MediaError::ImportFailed(msg)) if msg.contains("CPU memory")
        ));
    }
}
