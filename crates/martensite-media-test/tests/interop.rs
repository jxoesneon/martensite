//! Media interop integration tests.
//!
//! These tests exercise the cross-crate interaction between `martensite-media`,
//! `martensite-media-platform` (CPU upload + platform import dispatch), and
//! `martensite-wgpu` (`VideoProcessor` compute pipeline). They use the `noop`
//! wgpu backend (enabled via the `test-noop` feature) so they run in CI
//! without a real GPU adapter.
//!
//! See `WORKING_ON.md` §1.2 for the known limitation around the discarded UV
//! texture returned by `import_cpu_memory`.

#![forbid(unsafe_code)]

// All imports and helpers below are only needed when the `test-noop` feature
// is active (the noop tests and the platform stubs are both feature-gated).
// Gating the imports here keeps the test binary warning-free when it is
// compiled without the feature (e.g. by the workspace-wide
// `cargo test --workspace --tests` job, which runs with `-D warnings`).
#[cfg(feature = "test-noop")]
use martensite_media::surface::{HardwareHandle, MediaError, VideoPixelFormat};
#[cfg(feature = "test-noop")]
use martensite_media_platform::{
    import_cpu_memory, import_external_texture, ImportTextureDescriptor,
};
#[cfg(feature = "test-noop")]
use martensite_wgpu::interop::{VideoPipelineUniforms, VideoProcessor};

// ---------------------------------------------------------------------------
// Headless noop-backend tests
// ---------------------------------------------------------------------------

/// Builds a synthetic NV12 frame: a 64x64 luma plane and a 32x32 interleaved
/// chroma plane (2 bytes per pixel). Strides match the plane widths so the
/// buffers are dense.
#[cfg(feature = "test-noop")]
fn synthetic_nv12(width: u32, height: u32) -> HardwareHandle {
    let y_stride = width;
    let uv_stride = width;
    let y_plane = vec![128u8; (y_stride * height) as usize];
    let uv_plane = vec![128u8; (uv_stride * (height / 2)) as usize];
    HardwareHandle::CpuMemory {
        y_plane,
        uv_plane,
        y_stride,
        uv_stride,
    }
}

/// Tests that require the `noop` wgpu backend. These are compiled out unless
/// the `test-noop` feature is active, because `wgpu::Device::noop` only exists
/// when `wgpu`'s `noop` feature is enabled.
#[cfg(feature = "test-noop")]
mod noop {
    use super::*;

    fn noop_device() -> (wgpu::Device, wgpu::Queue) {
        wgpu::Device::noop(&wgpu::DeviceDescriptor::default())
    }

    #[test]
    fn video_processor_new_compiles_shaders() {
        let (device, _queue) = noop_device();
        let processor = VideoProcessor::new(&device);
        assert!(
            processor.is_ok(),
            "VideoProcessor::new should compile the YUV EOTF shader on the noop backend"
        );
    }

    #[test]
    fn process_frame_records_commands() {
        let (device, queue) = noop_device();
        let processor = VideoProcessor::new(&device).expect("processor builds");

        // 64x64 luma + 32x32 chroma.
        let width = 64u32;
        let height = 64u32;

        // Luma plane texture (R8Unorm).
        let luma = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("test-luma"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let y_view = luma.create_view(&wgpu::TextureViewDescriptor::default());

        // Chroma plane texture (Rg8Unorm), half resolution.
        let chroma = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("test-chroma"),
            size: wgpu::Extent3d {
                width: width / 2,
                height: height / 2,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rg8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let uv_view = chroma.create_view(&wgpu::TextureViewDescriptor::default());

        // Uniform buffer + output storage texture from the processor helpers.
        let uniforms = VideoPipelineUniforms::new_bt709_sdr();
        let uniform_buffer = processor.create_uniform_buffer(&device, &uniforms);
        let output = processor.create_output_texture(&device, width, height);
        let output_view = output.create_view(&wgpu::TextureViewDescriptor::default());

        // Record the compute dispatch into an encoder and finish it.
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("test-video-encoder"),
        });
        processor.process_frame(
            &device,
            &mut encoder,
            &y_view,
            &uv_view,
            &uniform_buffer,
            &output_view,
            (width, height),
        );
        let command_buffer = encoder.finish();

        // The noop backend does not execute the pass, but the command buffer
        // must be recorded without panicking. Submitting it exercises the
        // queue path as well.
        queue.submit(std::iter::once(command_buffer));
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
    }

    #[test]
    fn import_cpu_memory_nv12() {
        let (device, queue) = noop_device();
        let width = 64u32;
        let height = 64u32;
        let handle = synthetic_nv12(width, height);
        let desc = ImportTextureDescriptor::new(width, height, VideoPixelFormat::Nv12);

        let video_texture = import_cpu_memory(&device, &queue, &handle, &desc);
        assert!(
            video_texture.is_ok(),
            "import_cpu_memory should upload NV12 data"
        );

        let video_texture = video_texture.unwrap();

        // Luma plane: full resolution, R8Unorm.
        let luma = &video_texture.y;
        let luma_dims = luma.size();
        assert_eq!(luma_dims.width, width);
        assert_eq!(luma_dims.height, height);
        assert_eq!(luma_dims.depth_or_array_layers, 1);
        assert_eq!(luma.format(), wgpu::TextureFormat::R8Unorm);

        // Chroma plane: half resolution, Rg8Unorm.
        let chroma = video_texture
            .uv
            .as_ref()
            .expect("NV12 upload should produce a chroma plane");
        let chroma_dims = chroma.size();
        assert_eq!(chroma_dims.width, width / 2);
        assert_eq!(chroma_dims.height, height / 2);
        assert_eq!(chroma_dims.depth_or_array_layers, 1);
        assert_eq!(chroma.format(), wgpu::TextureFormat::Rg8Unorm);
    }

    #[test]
    fn import_cpu_memory_returns_video_texture_with_both_planes() {
        let (device, queue) = noop_device();
        let width = 64u32;
        let height = 64u32;
        let handle = synthetic_nv12(width, height);
        let desc = ImportTextureDescriptor::new(width, height, VideoPixelFormat::Nv12);

        let video_texture = import_cpu_memory(&device, &queue, &handle, &desc)
            .expect("import_cpu_memory should upload NV12 data");

        // The luma view should be creatable.
        let _luma_view = video_texture.luma_view();

        // The chroma view should be present for NV12.
        let chroma_view = video_texture
            .chroma_view()
            .expect("NV12 upload should expose a chroma view");
        let _ = chroma_view;

        // Dimensions match the luma plane.
        assert_eq!(video_texture.width(), width);
        assert_eq!(video_texture.height(), height);
    }

    #[test]
    fn import_cpu_memory_rejects_zero_dimensions() {
        let (device, queue) = noop_device();
        let handle = synthetic_nv12(64, 64);
        let desc = ImportTextureDescriptor::new(0, 0, VideoPixelFormat::Nv12);

        let err = import_cpu_memory(&device, &queue, &handle, &desc);
        assert!(
            matches!(
                err,
                Err(MediaError::InvalidBufferDimensions {
                    width: 0,
                    height: 0
                })
            ),
            "zero dimensions should be rejected with InvalidBufferDimensions, got {err:?}"
        );
    }

    #[test]
    fn import_external_texture_cpu_returns_error() {
        let (device, _queue) = noop_device();
        // A dma-buf handle with an invalid fd. On Linux this is rejected as an
        // invalid handle; on other platforms the platform import is
        // unavailable. Either way it must surface an error, not succeed.
        let handle = HardwareHandle::DmaBuf {
            fd: -1,
            stride: 0,
            offset: 0,
            modifier: 0,
        };
        let desc = ImportTextureDescriptor::new(64, 64, VideoPixelFormat::Nv12);
        let result = import_external_texture(&device, &handle, &desc);
        assert!(
            result.is_err(),
            "import_external_texture should reject an invalid hardware handle"
        );
    }

    #[test]
    fn create_output_texture_matches_dimensions() {
        let (device, _queue) = noop_device();
        let processor = VideoProcessor::new(&device).expect("processor builds");
        let texture = processor.create_output_texture(&device, 1920, 1080);
        let dims = texture.size();
        assert_eq!(dims.width, 1920);
        assert_eq!(dims.height, 1080);
        assert_eq!(dims.depth_or_array_layers, 1);
        assert_eq!(texture.format(), wgpu::TextureFormat::Rgba16Float);
    }
}

// ---------------------------------------------------------------------------
// Platform-specific import stubs (ignored)
// ---------------------------------------------------------------------------
//
// These document the native hardware import paths that must be exercised on
// real platform runners (macOS / Windows / Linux). They are `#[ignore]`-gated
// because the noop backend cannot perform real external-memory import, and
// are additionally gated on the `test-noop` feature so that the workspace-wide
// `cargo test --workspace --ignored` job (which does not enable `test-noop`)
// does not compile or run them. Each stub asserts that the platform test has
// not yet been implemented; flip the assertion once a real GPU-backed
// implementation lands.

#[cfg(all(feature = "test-noop", target_os = "macos"))]
mod macos_platform {
    //! `IOSurface` import via the Metal backend.

    use super::*;

    #[test]
    #[ignore = "requires a real macOS GPU + live IOSurface; not implemented on noop"]
    fn import_iosurface_not_implemented() {
        // TODO: create a real IOSurface, import it via `import_external_texture`
        // with a Metal-backed device, and assert the returned texture matches
        // the surface dimensions and NV12 luma format.
        let _ = (
            VideoPixelFormat::Nv12,
            ImportTextureDescriptor::new(0, 0, VideoPixelFormat::Nv12),
        );
        panic!("macOS IOSurface import test not yet implemented");
    }
}

#[cfg(all(feature = "test-noop", target_os = "windows"))]
mod windows_platform {
    //! DXGI shared NT-handle import via the DX12 backend.

    use super::*;

    #[test]
    #[ignore = "requires a real Windows GPU + DXGI shared handle; not implemented on noop"]
    fn import_dxgi_not_implemented() {
        // TODO: open a shared DXGI NT handle, import it via
        // `import_external_texture` with a DX12-backed device, and assert the
        // returned texture matches the shared resource dimensions.
        let _ = (
            VideoPixelFormat::Nv12,
            ImportTextureDescriptor::new(0, 0, VideoPixelFormat::Nv12),
        );
        panic!("Windows DXGI import test not yet implemented");
    }
}

#[cfg(all(feature = "test-noop", target_os = "linux"))]
mod linux_platform {
    //! DRM `dma-buf` import via the Vulkan backend.

    use super::*;

    #[test]
    #[ignore = "requires a real Linux GPU + dma-buf fd; not implemented on noop"]
    fn import_dmabuf_not_implemented() {
        // TODO: allocate a dma-buf (e.g. via gbm/DRM), import it via
        // `import_external_texture` with a Vulkan-backed device, and assert the
        // returned texture matches the buffer dimensions and format.
        let _ = (
            VideoPixelFormat::Nv12,
            ImportTextureDescriptor::new(0, 0, VideoPixelFormat::Nv12),
        );
        panic!("Linux dma-buf import test not yet implemented");
    }
}
