//! Zero-copy hardware video surfaces, HDR color science, and filmic tone mapping.
//!
//! `martensite-media` implements the Milestone `v0.8.0` media and graphics pipeline
//! capabilities for the Martensite framework:
//!
//! - [`surface`]: Zero-copy hardware video surface bindings (Windows DXGI NT handles,
//!   macOS `IOSurface`, Linux DRM `dma-buf`), format negotiation, and CPU dispatch
//!   telemetry ($< 1\%$ CPU utilization).
//! - [`color`]: BT.709 and BT.2020 color space definitions, SMPTE ST 2084 (PQ) EOTF
//!   pipeline, open-domain scRGB optical linear conversion, and color difference
//!   evaluation ($\Delta E < 1.0$).
//! - [`tonemap`]: Display-adaptive SDR reference white scaling to match panel physical
//!   headroom, plus Hable and Uchimura filmic tone mapping operators.
//!
//! # Safety
//!
//! This crate contains zero `unsafe` code blocks (`#![forbid(unsafe_code)]`). All platform
//! handle transfers and memory mappings are guarded by safe descriptors and checked invariants.
//!
//! # Example
//!
//! ```
//! use martensite_media::color::{pq_eotf, ScRgb};
//! use martensite_media::surface::{VideoPixelFormat, VideoSurface};
//! use martensite_media::tonemap::DisplayProfile;
//!
//! // Create a simulated 4K 10-bit HDR video surface
//! let surface = VideoSurface::new_mock(3840, 2160, VideoPixelFormat::P010);
//! assert!(surface.is_hardware_accelerated());
//!
//! // Convert peak 1000-nit highlight to scRGB relative to a 203-nit reference white
//! let nits = pq_eotf(0.75); // ~1000 nits
//! let display = DisplayProfile::default_hdr10();
//! let scrgb = ScRgb::from_nits(nits, display.effective_sdr_white());
//! assert!(scrgb.r > 1.0); // HDR highlight exceeds SDR 1.0 boundary
//! ```

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod color;
pub mod surface;
pub mod tonemap;

pub use color::{
    bt2020_to_bt709_linear, bt2020_yuv_to_rgb, bt709_yuv_to_rgb, delta_e_76, pq_eotf, pq_oetf,
    rgb_to_bt2020_yuv, rgb_to_bt709_yuv, rgb_to_xyz, xyz_to_lab, ColorSpace, ScRgb,
    TransferFunction,
};
pub use surface::{
    ColorRange, HardwareHandle, MediaError, VideoFrameMetadata, VideoPixelFormat, VideoSurface,
};
pub use tonemap::{
    hable_tonemap_scalar, uchimura_tonemap_scalar, DisplayCapabilities, DisplayProfile,
    ToneMapOperator,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn re_exports_are_constructible() {
        let surface = VideoSurface::new_mock(1920, 1080, VideoPixelFormat::Nv12);
        assert_eq!(surface.dimensions(), (1920, 1080));

        let profile = DisplayProfile::default_sdr();
        assert_eq!(profile.effective_sdr_white(), 203.0);
    }
}
