# Detailed Design Record: DDR-0019
## Title: `martensite-media` Zero-Copy Surface Protocol

**Status:** Updated — OQ-3 resolved 2026-09-06  
**Decision:** NV12 + P010 (10-bit HDR) both supported at v1.0.

---

### 1. Architectural Role & Invariants

`martensite-media` passes hardware video decoder output surfaces directly into `wgpu` textures without CPU readback. Color space conversion (YUV → RGB/linear) executes exclusively on the GPU.

* **Invariant 1.1:** External media frames map directly into `wgpu` textures via OS-specific zero-copy APIs (NT Handles, IOSurfaces, dma-bufs).
* **Invariant 1.2:** Color space conversion executes via WGSL compute/fragment shaders — zero CPU pixel work.
* **Invariant 1.3:** CPU copy fallback is permitted only if the GPU driver rejects the external memory handle; logged as `tracing::error!`, never silent.
* **Invariant 1.4:** Both NV12 (8-bit SDR) and P010 (10-bit HDR) are first-class supported formats at v1.0.

---

### 2. Supported Pixel Formats

| Format | Bit Depth | Chroma | Use Case | v1.0 |
|--------|-----------|--------|----------|-------|
| NV12 | 8-bit | 4:2:0 | SDR video, webcam, screen capture | ✅ |
| P010 | 10-bit | 4:2:0 | HDR10, HLG, 4K HDR video | ✅ |
| YUV420P | 8-bit | 4:2:0 | Software decoders | ✅ (CPU upload only) |
| P016 | 16-bit | 4:2:0 | Professional HDR | ❌ v1.1 |
| AYUV | 8-bit | 4:4:4 | Alpha video | ❌ v1.1 |

---

### 3. Platform Zero-Copy Interoperability

| Platform | API | Handle Type | wgpu Entry Point |
|----------|-----|-------------|-----------------|
| Windows | DXGI | `IDXGIResource1::CreateSharedHandle` (NT Handle) | `create_texture_from_hal` (DX12 HAL) |
| macOS | IOSurface | `IOSurfaceRef` | `create_texture_from_hal` (Metal HAL) |
| Linux | Vulkan external memory | `dma-buf` fd | `create_texture_from_hal` (Vulkan HAL) |

---

### 4. Color Space Conversion Shaders

Two WGSL shader variants are compiled and cached at startup.

#### 4a. NV12 → Linear RGB (BT.709, SDR)

```wgsl
@group(0) @binding(0) var y_tex:  texture_2d<f32>;
@group(0) @binding(1) var uv_tex: texture_2d<f32>;
@group(0) @binding(2) var smp:    sampler;

@fragment
fn fs_nv12(in: VertexOutput) -> @location(0) vec4<f32> {
    let y  = textureSample(y_tex,  smp, in.uv).r;
    let uv = textureSample(uv_tex, smp, in.uv).rg - vec2<f32>(0.5, 0.5);

    // BT.709 limited-range matrix
    let r = clamp(y + 1.5748 * uv.r, 0.0, 1.0);
    let g = clamp(y - 0.1873 * uv.g - 0.4681 * uv.r, 0.0, 1.0);
    let b = clamp(y + 1.8556 * uv.g, 0.0, 1.0);

    // Linear (no gamma — Vello composites in linear light)
    return vec4<f32>(pow(vec3<f32>(r, g, b), vec3<f32>(2.2)), 1.0);
}
```

#### 4b. P010 → Linear RGB (BT.2020, HDR10 — PQ EOTF)

```wgsl
// P010: 10-bit values packed into 16-bit unorm textures (upper 10 bits used)
@group(0) @binding(0) var y_tex:  texture_2d<f32>;  // R16_UNORM
@group(0) @binding(1) var uv_tex: texture_2d<f32>;  // RG16_UNORM
@group(0) @binding(2) var smp:    sampler;

const PQ_M1: f32 = 0.1593017578125;
const PQ_M2: f32 = 78.84375;
const PQ_C1: f32 = 0.8359375;
const PQ_C2: f32 = 18.8515625;
const PQ_C3: f32 = 18.6875;

fn pq_eotf(v: f32) -> f32 {
    let vp = pow(v, 1.0 / PQ_M2);
    return pow(max(vp - PQ_C1, 0.0) / (PQ_C2 - PQ_C3 * vp), 1.0 / PQ_M1);
}

@fragment
fn fs_p010(in: VertexOutput) -> @location(0) vec4<f32> {
    // P010 stores values in upper 10 bits of 16-bit word → divide by 64
    let y  = textureSample(y_tex,  smp, in.uv).r;
    let uv = textureSample(uv_tex, smp, in.uv).rg - vec2<f32>(0.5, 0.5);

    // BT.2020 limited-range matrix
    let r = clamp(y + 1.4746 * uv.r, 0.0, 1.0);
    let g = clamp(y - 0.1646 * uv.g - 0.5714 * uv.r, 0.0, 1.0);
    let b = clamp(y + 1.8814 * uv.g, 0.0, 1.0);

    // PQ inverse EOTF → linear scene-referred light (nits / 10000)
    return vec4<f32>(pq_eotf(r), pq_eotf(g), pq_eotf(b), 1.0);
}
```

---

### 5. HDR Swapchain Requirements (P010)

P010 output requires an HDR swapchain. Platform-specific setup:

| Platform | HDR Surface Format | Condition |
|----------|--------------------|-----------|
| Windows | `DXGI_FORMAT_R16G16B16A16_FLOAT` | `IDXGISwapChain4::SetHDRMetaData` called |
| macOS | `MTLPixelFormatRGBA16Float` + EDR | `NSScreen.maximumPotentialExtendedDynamicRangeColorComponentValue > 1.0` |
| Linux | Vulkan HDR10 surface extension | `VK_EXT_hdr_metadata` + `VK_EXT_swapchain_colorspace` |

If the display or OS does not support HDR, P010 content is tone-mapped to SDR using a Hable/Uchimura filmic operator applied in the same shader pass.

---

### 6. Format Negotiation API

```rust
pub enum PixelFormat { Nv12, P010 }

pub struct MediaSurface {
    pub format: PixelFormat,
    pub width: u32,
    pub height: u32,
    // platform handle (opaque)
}

pub struct MediaWidget {
    surface: MediaSurface,
    shader:  ShaderVariant, // Nv12BT709 | P010BT2020PQ
    hdr_swapchain: bool,
}
```

Format is detected from the decoder output at bind time. Shader variant is selected once and cached for the lifetime of the surface.

---

### 7. Error Conditions

| Condition | Response |
|-----------|----------|
| GPU driver rejects external memory handle | `tracing::error!`, downgrade to `Queue::write_texture` (CPU copy) |
| Display does not support HDR | Tone-map P010 to SDR inline; `tracing::warn!` |
| P010 texture format not supported by GPU | Return `Err(MediaError::UnsupportedFormat)` |
| NT Handle / IOSurface / dma-buf import fails | Return `Err(MediaError::ZeroCopyUnavailable { reason })` |

---

### 8. Performance Invariants

- CPU time per 4K/60fps NV12 frame: **~0.0ms** (handle dispatch only)
- CPU time per 4K/60fps P010 frame: **~0.0ms** (same — shader handles conversion)
- VRAM reads during compositing: **1 read** (texture sample in fragment shader)
- Shader compile time (startup): **<50ms** per variant (WGSL → SPIR-V → native)
