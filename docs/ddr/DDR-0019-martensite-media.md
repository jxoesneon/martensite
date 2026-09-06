# Detailed Design Record: DDR-0019
## Title: `martensite-media` Zero-Copy Surface Protocol

### 1. Architectural Role & Invariants
`martensite-media` supports high-performance video playback and hardware camera ingestion without CPU pixel-copy overhead.
* **Invariant 1.1**: External media frames map directly into `wgpu` textures via OS-specific zero-copy APIs (NT Handles, IOSurfaces, dma-bufs).
* **Invariant 1.2**: Color space conversion (YUV -> RGB) executes exclusively on the GPU via compute/fragment shaders.
* **Invariant 1.3**: Fallback CPU copy is permitted only on highly legacy or unsupported drivers.

### 2. Platform Interoperability
- **Windows (DXGI)**: Uses `IDXGIResource1::CreateSharedHandle` (NT Handle). Bound via `wgpu::Device::create_texture_from_hal`.
- **macOS (IOSurface)**: `IOSurfaceRef` mapped to Metal `MTLTexture`.
- **Linux (Wayland/X11)**: `dma-buf` file descriptors mapped via EGL/Vulkan external memory extensions.

### 3. Format Negotiation & Color Conversion
Frames arrive primarily in `NV12` or `YUV420P` layouts.
```wgsl
// pseudo-wgsl color conversion snippet
@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let y = textureSample(y_texture, smp, in.uv).r;
    let uv = textureSample(uv_texture, smp, in.uv).rg;
    
    // BT.709 conversion matrix
    let r = y + 1.5748 * (uv.r - 0.5);
    let g = y - 0.1873 * (uv.g - 0.5) - 0.4681 * (uv.r - 0.5);
    let b = y + 1.8556 * (uv.g - 0.5);
    
    return vec4<f32>(r, g, b, 1.0);
}
```

### 4. Error Conditions
- If the GPU driver rejects the external memory handle (`wgpu` HAL error), log a critical error and downgrade to a CPU readback `wgpu::Queue::write_texture` pipeline (severe performance penalty).

### 5. Performance Invariants
- CPU time spent per 4K video frame: ~0.0ms (handle dispatch only).
- VRAM bandwidth: Optimal 1-copy read during compositing.
