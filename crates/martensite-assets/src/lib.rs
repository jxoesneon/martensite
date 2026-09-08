//! Virtual asset system and AOT shader validation for Martensite.
//!
//! This crate provides two subsystems used by the Martensite renderer and
//! tooling:
//!
//! - **[`vfs`]** — a dual-mode Virtual File System with a disk-backed backend
//!   for development hot-reloading and an embedded, zero-copy backend for
//!   release binaries.
//! - **[`shader`]** — ahead-of-time WGSL validation and reflection powered by
//!   the [`naga`] crate, catching invalid shaders at build time and extracting
//!   pipeline-layout metadata.
//!
//! Both subsystems are `#![forbid(unsafe_code)]` and fully documented with
//! runnable examples.
//!
//! # Examples
//!
//! ```
//! use martensite_assets::shader::ShaderValidator;
//! use martensite_assets::vfs::{EmbeddedVfs, Vfs};
//!
//! // Embedded VFS: assets baked into the binary.
//! static ASSETS: &[(&str, &[u8])] = &[("shader.wgsl", b"@vertex fn vs() -> @builtin(position) vec4<f32> { return vec4<f32>(0.0); }")];
//! let vfs = EmbeddedVfs::new(ASSETS);
//!
//! // AOT shader validation against the embedded source.
//! let mut validator = ShaderValidator::new();
//! let source = std::str::from_utf8(vfs.resolve("shader.wgsl").unwrap()).unwrap();
//! let reflection = validator.validate(source).unwrap();
//! assert_eq!(reflection.entry_points.len(), 1);
//! ```
//!
//! [`naga`]: naga
#![forbid(unsafe_code)]
#![deny(missing_docs)]

/// AOT WGSL shader validation and reflection.
pub mod shader;

/// Dual-mode Virtual File System for asset resolution.
pub mod vfs;

pub use shader::{
    BindingInfo, BindingType, EntryPoint, ShaderError, ShaderReflection, ShaderStage,
    ShaderValidator,
};
pub use vfs::{AssetHandle, AssetPath, AssetPathError, EmbeddedVfs, Vfs, VfsBackend};

#[cfg(feature = "disk")]
pub use vfs::DiskVfs;

#[cfg(test)]
mod tests {
    /// Smoke test: the crate compiles and `#![forbid(unsafe_code)]` is
    /// enforced at compile time.
    #[test]
    fn crate_compiles() {
        // Re-exported items are reachable from the crate root.
        let _ = crate::ShaderValidator::new();
    }
}
