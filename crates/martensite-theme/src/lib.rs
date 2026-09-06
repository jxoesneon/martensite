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

#[cfg(test)]
mod tests {
    use super::Oklab;
    use bytemuck::Zeroable;
    use std::mem::size_of;

    #[test]
    fn lerp_at_t_zero_returns_self() {
        let a = Oklab {
            l: 0.1,
            a: 0.2,
            b: 0.3,
            alpha: 0.4,
        };
        let b = Oklab {
            l: 0.5,
            a: 0.6,
            b: 0.7,
            alpha: 0.8,
        };
        let result = a.lerp(b, 0.0);
        assert_eq!(result, a);
    }

    #[test]
    fn lerp_at_t_one_returns_other() {
        let a = Oklab {
            l: 0.1,
            a: 0.2,
            b: 0.3,
            alpha: 0.4,
        };
        let b = Oklab {
            l: 0.5,
            a: 0.6,
            b: 0.7,
            alpha: 0.8,
        };
        let result = a.lerp(b, 1.0);
        assert_eq!(result, b);
    }

    #[test]
    fn lerp_at_t_half_returns_midpoint() {
        let a = Oklab {
            l: 0.0,
            a: 0.0,
            b: 0.0,
            alpha: 0.0,
        };
        let b = Oklab {
            l: 1.0,
            a: 1.0,
            b: 1.0,
            alpha: 1.0,
        };
        let result = a.lerp(b, 0.5);
        assert_eq!(
            result,
            Oklab {
                l: 0.5,
                a: 0.5,
                b: 0.5,
                alpha: 0.5,
            }
        );
    }

    #[test]
    fn lerp_interpolates_all_channels() {
        let a = Oklab {
            l: 0.0,
            a: 0.0,
            b: 0.0,
            alpha: 0.0,
        };
        let b = Oklab {
            l: 1.0,
            a: 2.0,
            b: 3.0,
            alpha: 4.0,
        };
        let result = a.lerp(b, 0.25);
        assert_eq!(result.l, 0.25);
        assert_eq!(result.a, 0.5);
        assert_eq!(result.b, 0.75);
        assert_eq!(result.alpha, 1.0);
    }

    #[test]
    fn oklab_is_copy_clone_debug_eq() {
        let a = Oklab {
            l: 0.1,
            a: 0.2,
            b: 0.3,
            alpha: 0.4,
        };
        let cloned = a;
        assert_eq!(a, cloned);
        let debug_str = format!("{:?}", a);
        assert!(debug_str.contains("Oklab"));
    }

    #[test]
    fn oklab_is_pod_and_zeroable() {
        let a = Oklab {
            l: 0.1,
            a: 0.2,
            b: 0.3,
            alpha: 0.4,
        };
        // bytes_of requires Pod; this call proves the trait is implemented.
        let bytes = bytemuck::bytes_of(&a);
        assert_eq!(bytes.len(), 16);
        // zeroed requires Zeroable; this call proves the trait is implemented.
        let zeroed = Oklab::zeroed();
        assert_eq!(zeroed.l, 0.0);
        assert_eq!(zeroed.a, 0.0);
        assert_eq!(zeroed.b, 0.0);
        assert_eq!(zeroed.alpha, 0.0);
    }

    #[test]
    fn oklab_is_16_bytes() {
        assert_eq!(size_of::<Oklab>(), 16);
    }
}
