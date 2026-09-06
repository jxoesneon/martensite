# Detailed Design Record: DDR-0015
## Title: `martensite-assets` Dual-Mode VFS & Texture Atlas

### 1. Architectural Role & Invariants
`martensite-assets` abstracts resource loading (images, fonts, localization bundles).
* **Invariant 1.1**: Release builds must compile assets statically into the binary using `include_bytes!` (Embed mode).
* **Invariant 1.2**: Debug builds use OS filesystem watchers to provide live hot-reloading (FS mode).
* **Invariant 1.3**: Dynamic texture management relies on `etagere` for $O(1)$ GPU texture atlas sub-allocation.

### 2. Dual-Mode VFS Trait
```rust
use std::borrow::Cow;

pub trait VirtualFileSystem {
    fn read_asset(&self, path: &str) -> Option<Cow<'static, [u8]>>;
    fn watch_asset(&self, path: &str, callback: fn());
}
```

### 3. Asset ID Hashing & Etagere Atlas
Assets are addressed via 64-bit FNV-1a hashes of their logical paths.
```rust
pub struct TextureAtlas {
    allocator: etagere::AtlasAllocator,
    backing_texture: wgpu::Texture,
    hash_to_rect: std::collections::HashMap<u64, etagere::Allocation>,
}

impl TextureAtlas {
    pub fn allocate(&mut self, id: u64, width: i32, height: i32) -> Option<etagere::Allocation> {
        let size = etagere::Size::new(width, height);
        let allocation = self.allocator.allocate(size)?;
        self.hash_to_rect.insert(id, allocation.clone());
        Some(allocation)
    }
}
```

### 4. Hot-Reload Notify Integration
In Debug/FS mode, `notify` crate spins a background thread observing `assets/`. Upon `Modify` event:
1. Re-read file to memory.
2. Calculate delta.
3. Push invalidation command to `martensite-window` event loop proxy.
4. Engine marks relevant nodes `DIRTY_PAINT`.

### 5. Performance Invariants
- Zero allocation for reading embedded assets (returns `Cow::Borrowed`).
- Atlas allocation latency < 10μs via `etagere`.
