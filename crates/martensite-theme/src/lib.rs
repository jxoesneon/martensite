//! Design tokens and Oklab uniform blending.
#![forbid(unsafe_code)]

use bytemuck::{Pod, Zeroable};

/// A color in the Oklab perceptual color space, stored as a 16-byte
/// C-compatible struct suitable for GPU uniform buffers.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Pod, Zeroable)]
pub struct Oklab {
    /// The lightness channel, ranging from `0.0` (black) to `1.0` (white).
    pub l: f32,
    /// The green-red chromaticity axis (`a`).
    pub a: f32,
    /// The blue-yellow chromaticity axis (`b`).
    pub b: f32,
    /// The alpha channel, where `0.0` is fully transparent and `1.0` is opaque.
    pub alpha: f32,
}

impl Oklab {
    /// Linearly interpolates between `self` and `other` by the factor `t`,
    /// performing uniform blending across all four Oklab channels.
    #[inline(always)]
    pub fn lerp(self, other: Self, t: f32) -> Self {
        Self {
            l: self.l + (other.l - self.l) * t,
            a: self.a + (other.a - self.a) * t,
            b: self.b + (other.b - self.b) * t,
            alpha: self.alpha + (other.alpha - self.alpha) * t,
        }
    }
}
