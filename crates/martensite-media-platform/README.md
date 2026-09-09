# martensite-media-platform

Platform-specific zero-copy hardware video surface import for
[martensite](../martensite).

This crate implements the unsafe FFI boundary for importing hardware video
decoder buffers directly into `wgpu` textures without CPU copies:

- **macOS**: `IOSurface` → `MTLTexture` → `wgpu::Texture` via
  `wgpu::Device::create_texture_from_hal`.
- **Windows**: DXGI shared NT handle → `ID3D12Resource` → `wgpu::Texture`.
- **Linux**: DRM `dma-buf` file descriptor → `wgpu::Texture`.

## Safety

This crate uses `#![allow(unsafe_code)]` because it contains platform-specific
FFI. All `unsafe` blocks are confined to platform-specific modules and are
audited against the upstream API documentation. The workspace-level
`unsafe_code = "deny"` policy is preserved for all other crates.
