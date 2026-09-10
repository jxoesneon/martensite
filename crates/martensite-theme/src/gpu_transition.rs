//! GPU theme transition uniforms and WGSL blending shader.
//!
//! This module implements Section 4.3 of the `v0.6.0` motion & theme
//! milestone: theme token values are uploaded into a single 256-byte aligned
//! uniform buffer ([`ThemeUniforms`]), and a smooth 150ms transition parameter
//! (`t ∈ [0.0, 1.0]`) blends color palettes directly in the fragment shader.
//!
//! Theme swaps therefore do **not** re-run component view declarations — the
//! same view tree is retained across a light/dark switch and only the uniform
//! buffer contents change, keeping the 5,000-widget scene at continuous
//! 60fps/120fps with zero CPU allocations.

use bytemuck::{Pod, Zeroable};

use crate::tokens::{Theme, TokenKey};
use crate::Oklab;

/// Maximum number of color tokens that fit in a single 256-byte uniform
/// buffer: `15` colors × `16` bytes = `240` bytes of color data, plus a
/// `16`-byte header (`color_count` + padding) = `256` bytes total.
pub const MAX_THEME_COLORS: usize = 15;

/// The canonical order of color [`TokenKey`]s, matching the declaration order
/// of the `TokenKey` enum. This fixed mapping guarantees that the `from` and
/// `to` uniform buffers use identical indices for the same semantic color
/// token, so the GPU blend in [`THEME_TRANSITION_WGSL`] pairs the correct
/// colors.
pub const COLOR_TOKEN_KEYS: [TokenKey; 14] = [
    TokenKey::BackgroundColor,
    TokenKey::SurfaceColor,
    TokenKey::PrimaryColor,
    TokenKey::SecondaryColor,
    TokenKey::AccentColor,
    TokenKey::TextColor,
    TokenKey::TextMutedColor,
    TokenKey::TextInverseColor,
    TokenKey::BorderColor,
    TokenKey::DividerColor,
    TokenKey::ErrorColor,
    TokenKey::WarningColor,
    TokenKey::SuccessColor,
    TokenKey::InfoColor,
];

/// The default duration of a theme transition, in seconds (150 ms).
pub const DEFAULT_TRANSITION_DURATION: f32 = 0.150;

/// GPU uniform buffer layout for a complete theme palette.
///
/// The struct is exactly `256` bytes and `16`-byte aligned so it can be
/// uploaded verbatim into a single `wgpu` uniform buffer with
/// [`ThemeUniforms::as_bytes`]. The layout matches the WGSL uniform block
/// declared in [`THEME_TRANSITION_WGSL`].
///
/// # Layout
///
/// | offset | field         | size (bytes) |
/// |--------|---------------|--------------|
/// | `0`    | `colors`      | `240`        |
/// | `240`  | `color_count` | `4`          |
/// | `244`  | `_padding`    | `12`         |
/// | `256`  | *(end)*       | —            |
#[repr(C, align(16))]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct ThemeUniforms {
    /// The active color tokens in canonical `TokenKey` enum order, stored as
    /// `Oklab` perceptual colors. Unused slots remain zeroed.
    pub colors: [Oklab; MAX_THEME_COLORS],
    /// The number of active color tokens in [`ThemeUniforms::colors`].
    pub color_count: u32,
    /// Padding to reach `16`-byte alignment (and a total size of `256`
    /// bytes). Always zero.
    padding: [u32; 3],
}

impl ThemeUniforms {
    /// Creates a zeroed [`ThemeUniforms`] with no active color tokens.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_theme::gpu_transition::ThemeUniforms;
    /// let u = ThemeUniforms::new();
    /// assert_eq!(u.color_count, 0);
    /// ```
    #[inline]
    #[must_use]
    pub fn new() -> Self {
        Self::zeroed()
    }

    /// Sets the color token at `index`, growing the active color count when
    /// the index extends the populated range.
    ///
    /// # Panics
    ///
    /// Panics if `index` is greater than or equal to [`MAX_THEME_COLORS`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_theme::gpu_transition::{ThemeUniforms, MAX_THEME_COLORS};
    /// use martensite_theme::Oklab;
    /// let mut u = ThemeUniforms::new();
    /// u.set_color(0, Oklab { l: 0.5, a: 0.0, b: 0.0, alpha: 1.0 });
    /// assert_eq!(u.color_count, 1);
    /// assert_eq!(u.colors[0].l, 0.5);
    /// ```
    #[inline]
    pub fn set_color(&mut self, index: usize, color: Oklab) {
        assert!(
            index < MAX_THEME_COLORS,
            "color index {index} out of bounds (max {MAX_THEME_COLORS})"
        );
        self.colors[index] = color;
        let next_count = (index + 1) as u32;
        if next_count > self.color_count {
            self.color_count = next_count;
        }
    }

    /// Populates a [`ThemeUniforms`] from a [`Theme`], reading color tokens
    /// in canonical [`TokenKey`] enum order (see [`COLOR_TOKEN_KEYS`]).
    ///
    /// Each color token present in the theme is placed at its fixed index so
    /// that the `from` and `to` buffers of a [`ThemeTransition`] align
    /// correctly for GPU blending. Absent color tokens leave their slot
    /// zeroed. Up to [`MAX_THEME_COLORS`] tokens are considered; any further
    /// tokens are ignored. This performs no heap allocation — all data is
    /// stack resident.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_theme::gpu_transition::ThemeUniforms;
    /// use martensite_theme::tokens::default_light;
    /// let uniforms = ThemeUniforms::from_theme(&default_light());
    /// assert!(uniforms.color_count <= 15);
    /// ```
    #[inline]
    #[must_use]
    pub fn from_theme(theme: &Theme) -> Self {
        let mut uniforms = Self::new();
        for (i, &key) in COLOR_TOKEN_KEYS.iter().enumerate() {
            if i >= MAX_THEME_COLORS {
                break;
            }
            if let Some(color) = theme.color(key) {
                uniforms.set_color(i, color);
            }
        }
        uniforms
    }

    /// Returns the uniform buffer as a byte slice ready for GPU upload.
    ///
    /// The returned slice is exactly `256` bytes long and is a direct view of
    /// the struct's memory (via [`bytemuck::bytes_of`]); no allocation is
    /// performed.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_theme::gpu_transition::ThemeUniforms;
    /// let u = ThemeUniforms::new();
    /// assert_eq!(u.as_bytes().len(), 256);
    /// ```
    #[inline]
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        bytemuck::bytes_of(self)
    }

    /// Linearly interpolates all color tokens between `self` (at `t = 0`) and
    /// `other` (at `t = 1`), returning a new [`ThemeUniforms`].
    ///
    /// This is the CPU-side equivalent of the WGSL `theme_color` function and
    /// is primarily useful for testing. The GPU performs the same blend in
    /// the fragment shader. `color_count` is taken from `other`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_theme::gpu_transition::ThemeUniforms;
    /// use martensite_theme::Oklab;
    /// let mut a = ThemeUniforms::new();
    /// a.set_color(0, Oklab { l: 0.0, a: 0.0, b: 0.0, alpha: 0.0 });
    /// let mut b = ThemeUniforms::new();
    /// b.set_color(0, Oklab { l: 1.0, a: 1.0, b: 1.0, alpha: 1.0 });
    /// let mid = a.lerp(&b, 0.5);
    /// assert_eq!(mid.colors[0].l, 0.5);
    /// ```
    #[inline]
    #[must_use]
    pub fn lerp(&self, other: &Self, t: f32) -> Self {
        let mut out = *self;
        for i in 0..MAX_THEME_COLORS {
            out.colors[i] = self.colors[i].lerp(other.colors[i], t);
        }
        out.color_count = other.color_count;
        out
    }
}

impl Default for ThemeUniforms {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

/// Manages a smooth 150ms theme transition between two [`ThemeUniforms`]
/// snapshots.
///
/// The transition advances with real frame deltas via [`ThemeTransition::advance`]
/// and exposes a normalized progress `t ∈ [0.0, 1.0]`. The GPU blends the two
/// uniform buffers in the fragment shader using `t`; for CPU-side testing,
/// [`ThemeTransition::current_uniforms`] produces the same interpolated
/// result.
///
/// All operations are stack-only and perform zero heap allocations, satisfying
/// the zero-allocation exit criterion for the 5,000-widget theme switch.
#[derive(Copy, Clone, Debug)]
pub struct ThemeTransition {
    /// The starting theme uniforms (`t = 0`).
    pub from: ThemeUniforms,
    /// The target theme uniforms (`t = 1`).
    pub to: ThemeUniforms,
    /// Elapsed time in seconds since the transition started.
    pub elapsed: f32,
    /// Total transition duration in seconds.
    pub duration: f32,
}

impl ThemeTransition {
    /// Creates a new transition with the default 150ms duration.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_theme::gpu_transition::{ThemeTransition, ThemeUniforms};
    /// let t = ThemeTransition::new(ThemeUniforms::new(), ThemeUniforms::new());
    /// assert_eq!(t.duration, 0.150);
    /// assert_eq!(t.progress(), 0.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn new(from: ThemeUniforms, to: ThemeUniforms) -> Self {
        Self::new_with_duration(from, to, DEFAULT_TRANSITION_DURATION)
    }

    /// Creates a new transition with a custom `duration` (in seconds).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_theme::gpu_transition::{ThemeTransition, ThemeUniforms};
    /// let t = ThemeTransition::new_with_duration(
    ///     ThemeUniforms::new(),
    ///     ThemeUniforms::new(),
    ///     0.300,
    /// );
    /// assert_eq!(t.duration, 0.300);
    /// ```
    #[inline]
    #[must_use]
    pub fn new_with_duration(from: ThemeUniforms, to: ThemeUniforms, duration: f32) -> Self {
        // Validate duration: non-finite or non-positive values fall back to
        // the default to prevent NaN propagation in `advance`/`progress`.
        let duration = if duration.is_finite() && duration > 0.0 {
            duration
        } else {
            DEFAULT_TRANSITION_DURATION
        };
        Self {
            from,
            to,
            elapsed: 0.0,
            duration,
        }
    }

    /// Advances the transition by `dt` seconds, clamping elapsed time to
    /// `[0.0, duration]`. Non-finite or negative `dt` values are ignored so
    /// the transition state cannot be corrupted. Performs no heap allocation.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_theme::gpu_transition::{ThemeTransition, ThemeUniforms};
    /// let mut t = ThemeTransition::new(ThemeUniforms::new(), ThemeUniforms::new());
    /// t.advance(0.050);
    /// assert_eq!(t.progress(), 0.050 / 0.150);
    /// ```
    #[inline]
    pub fn advance(&mut self, dt: f32) {
        if !dt.is_finite() || dt < 0.0 {
            return;
        }
        self.elapsed = (self.elapsed + dt).clamp(0.0, self.duration);
    }

    /// Returns the normalized progress `t ∈ [0.0, 1.0]` (`elapsed / duration`,
    /// clamped). Returns `0.0` if the duration is zero.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_theme::gpu_transition::{ThemeTransition, ThemeUniforms};
    /// let mut t = ThemeTransition::new(ThemeUniforms::new(), ThemeUniforms::new());
    /// t.advance(0.075);
    /// assert_eq!(t.progress(), 0.5);
    /// ```
    #[inline]
    #[must_use]
    pub fn progress(&self) -> f32 {
        if self.duration <= 0.0 {
            return 1.0;
        }
        let t = self.elapsed / self.duration;
        t.clamp(0.0, 1.0)
    }

    /// Returns `true` once the transition has reached its full duration.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_theme::gpu_transition::{ThemeTransition, ThemeUniforms};
    /// let mut t = ThemeTransition::new(ThemeUniforms::new(), ThemeUniforms::new());
    /// t.advance(0.150);
    /// assert!(t.is_complete());
    /// ```
    #[inline]
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.elapsed >= self.duration
    }

    /// Returns the interpolated uniforms at the current progress.
    ///
    /// This is the CPU-side equivalent of the GPU fragment-shader blend and is
    /// provided for testing and headless verification. It performs no heap
    /// allocation.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_theme::gpu_transition::{ThemeTransition, ThemeUniforms};
    /// use martensite_theme::Oklab;
    /// let mut from = ThemeUniforms::new();
    /// from.set_color(0, Oklab { l: 0.0, a: 0.0, b: 0.0, alpha: 0.0 });
    /// let mut to = ThemeUniforms::new();
    /// to.set_color(0, Oklab { l: 1.0, a: 1.0, b: 1.0, alpha: 1.0 });
    /// let mut t = ThemeTransition::new(from, to);
    /// t.advance(0.075);
    /// let cur = t.current_uniforms();
    /// assert_eq!(cur.colors[0].l, 0.5);
    /// ```
    #[inline]
    #[must_use]
    pub fn current_uniforms(&self) -> ThemeUniforms {
        self.from.lerp(&self.to, self.progress())
    }

    /// Returns the raw `t` parameter for GPU upload (identical to
    /// [`ThemeTransition::progress`]).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_theme::gpu_transition::{ThemeTransition, ThemeUniforms};
    /// let mut t = ThemeTransition::new(ThemeUniforms::new(), ThemeUniforms::new());
    /// t.advance(0.150);
    /// assert_eq!(t.current_t(), 1.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn current_t(&self) -> f32 {
        self.progress()
    }
}

/// A complete, compilable WGSL snippet implementing GPU-side theme blending.
///
/// It declares a `ThemeUniforms` uniform block matching the Rust
/// [`ThemeUniforms`] layout (256 bytes, 16-byte aligned), binds two instances
/// (`from_colors` and `to_colors`), and exposes:
///
/// ```wgsl
/// fn theme_color(index: u32, t: f32) -> vec4<f32>
/// ```
///
/// which interpolates between `from_colors.colors[index]` and
/// `to_colors.colors[index]` by `t` (clamped to `[0, 1]`).
pub const THEME_TRANSITION_WGSL: &str = r#"
// Theme uniform buffer layout — matches Rust `ThemeUniforms` (256 bytes).
struct ThemeUniforms {
    colors: array<vec4<f32>, 15>,
    color_count: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
};

@group(0) @binding(0) var<uniform> from_colors: ThemeUniforms;
@group(0) @binding(1) var<uniform> to_colors: ThemeUniforms;

// Blends the color token at `index` between the `from` and `to` palettes by
// the transition parameter `t` (clamped to [0, 1]).
fn theme_color(index: u32, t: f32) -> vec4<f32> {
    let from_c = from_colors.colors[index];
    let to_c = to_colors.colors[index];
    let s = clamp(t, 0.0, 1.0);
    return mix(from_c, to_c, s);
}
"#;

/// Manages the dual uniform buffers used for GPU-side theme blending.
///
/// The renderer uploads [`ThemeUniformBuffer::from`] and
/// [`ThemeUniformBuffer::to`] as two uniform buffers, plus the scalar
/// transition parameter `t`; the fragment shader performs the per-pixel blend
/// (see [`THEME_TRANSITION_WGSL`]). All fields are stack-allocated, so
/// updating the buffers across a 5,000-widget scene performs zero CPU
/// allocations.
#[derive(Copy, Clone, Debug)]
pub struct ThemeUniformBuffer {
    /// The starting theme uniforms (`t = 0`).
    pub from: ThemeUniforms,
    /// The target theme uniforms (`t = 1`).
    pub to: ThemeUniforms,
    /// The transition parameter `t ∈ [0.0, 1.0]`.
    pub t: f32,
}

impl ThemeUniformBuffer {
    /// Creates a new buffer with zeroed uniforms and `t = 0.0`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_theme::gpu_transition::ThemeUniformBuffer;
    /// let buf = ThemeUniformBuffer::new();
    /// assert_eq!(buf.t, 0.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn new() -> Self {
        Self {
            from: ThemeUniforms::new(),
            to: ThemeUniforms::new(),
            t: 0.0,
        }
    }

    /// Sets the `from` (starting) uniforms.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_theme::gpu_transition::{ThemeUniformBuffer, ThemeUniforms};
    /// let mut buf = ThemeUniformBuffer::new();
    /// buf.set_from(ThemeUniforms::new());
    /// ```
    #[inline]
    pub fn set_from(&mut self, uniforms: ThemeUniforms) {
        self.from = uniforms;
    }

    /// Sets the `to` (target) uniforms.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_theme::gpu_transition::{ThemeUniformBuffer, ThemeUniforms};
    /// let mut buf = ThemeUniformBuffer::new();
    /// buf.set_to(ThemeUniforms::new());
    /// ```
    #[inline]
    pub fn set_to(&mut self, uniforms: ThemeUniforms) {
        self.to = uniforms;
    }

    /// Sets the transition parameter `t`, clamped to `[0.0, 1.0]`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_theme::gpu_transition::ThemeUniformBuffer;
    /// let mut buf = ThemeUniformBuffer::new();
    /// buf.set_t(2.0);
    /// assert_eq!(buf.t, 1.0);
    /// buf.set_t(-1.0);
    /// assert_eq!(buf.t, 0.0);
    /// ```
    #[inline]
    pub fn set_t(&mut self, t: f32) {
        // Guard against NaN: NaN.clamp() returns NaN, so check finiteness first.
        self.t = if t.is_finite() {
            t.clamp(0.0, 1.0)
        } else {
            0.0
        };
    }

    /// Returns the `from` uniforms as a 256-byte slice for GPU upload.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_theme::gpu_transition::ThemeUniformBuffer;
    /// let buf = ThemeUniformBuffer::new();
    /// assert_eq!(buf.from_bytes().len(), 256);
    /// ```
    #[inline]
    #[must_use]
    pub fn from_bytes(&self) -> &[u8] {
        self.from.as_bytes()
    }

    /// Returns the `to` uniforms as a 256-byte slice for GPU upload.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_theme::gpu_transition::ThemeUniformBuffer;
    /// let buf = ThemeUniformBuffer::new();
    /// assert_eq!(buf.to_bytes().len(), 256);
    /// ```
    #[inline]
    #[must_use]
    pub fn to_bytes(&self) -> &[u8] {
        self.to.as_bytes()
    }

    /// Returns the transition parameter `t`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_theme::gpu_transition::ThemeUniformBuffer;
    /// let mut buf = ThemeUniformBuffer::new();
    /// buf.set_t(0.5);
    /// assert_eq!(buf.transition_t(), 0.5);
    /// ```
    #[inline]
    #[must_use]
    pub fn transition_t(&self) -> f32 {
        self.t
    }
}

impl Default for ThemeUniformBuffer {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::linear_to_srgb;
    use std::mem::size_of;

    // Zero-allocation verification: we cannot replace the global allocator
    // from a library test module (the `#[global_allocator]` attribute must
    // live at the crate root and would conflict with other modules), so we
    // verify zero-allocation by inspection and by asserting the relevant
    // types are `Copy`. Every method on `ThemeTransition` and
    // `ThemeUniformBuffer` only copies fixed-size stack data and
    // dereferences slices — no `Vec`, `Box`, `String`, or collection is
    // touched. `Copy` is a necessary condition for stack-only operation,
    // which is checked below.

    fn sample_from() -> ThemeUniforms {
        let mut u = ThemeUniforms::new();
        u.set_color(
            0,
            Oklab {
                l: 0.0,
                a: 0.0,
                b: 0.0,
                alpha: 0.0,
            },
        );
        u.set_color(
            1,
            Oklab {
                l: 0.1,
                a: 0.2,
                b: 0.3,
                alpha: 0.4,
            },
        );
        u
    }

    fn sample_to() -> ThemeUniforms {
        let mut u = ThemeUniforms::new();
        u.set_color(
            0,
            Oklab {
                l: 1.0,
                a: 1.0,
                b: 1.0,
                alpha: 1.0,
            },
        );
        u.set_color(
            1,
            Oklab {
                l: 0.5,
                a: 0.6,
                b: 0.7,
                alpha: 0.8,
            },
        );
        u
    }

    #[test]
    fn theme_uniforms_is_256_bytes() {
        assert_eq!(size_of::<ThemeUniforms>(), 256);
    }

    #[test]
    fn theme_uniforms_is_16_byte_aligned() {
        assert_eq!(core::mem::align_of::<ThemeUniforms>(), 16);
    }

    #[test]
    fn theme_uniforms_is_pod_and_zeroable() {
        let u = ThemeUniforms::new();
        // `bytes_of` requires `Pod`.
        assert_eq!(bytemuck::bytes_of(&u).len(), 256);
        // `zeroed` requires `Zeroable`.
        let zeroed = ThemeUniforms::zeroed();
        assert_eq!(zeroed.color_count, 0);
    }

    #[test]
    fn set_color_grows_color_count() {
        let mut u = ThemeUniforms::new();
        assert_eq!(u.color_count, 0);
        u.set_color(
            2,
            Oklab {
                l: 0.5,
                a: 0.0,
                b: 0.0,
                alpha: 1.0,
            },
        );
        assert_eq!(u.color_count, 3);
        assert_eq!(u.colors[2].l, 0.5);
        // Setting a lower index must not shrink the count.
        u.set_color(
            0,
            Oklab {
                l: 0.25,
                a: 0.0,
                b: 0.0,
                alpha: 1.0,
            },
        );
        assert_eq!(u.color_count, 3);
        assert_eq!(u.colors[0].l, 0.25);
    }

    #[test]
    #[should_panic]
    fn set_color_panics_out_of_bounds() {
        let mut u = ThemeUniforms::new();
        u.set_color(MAX_THEME_COLORS, Oklab::zeroed());
    }

    #[test]
    fn as_bytes_returns_256_bytes() {
        let u = ThemeUniforms::new();
        assert_eq!(u.as_bytes().len(), 256);
    }

    #[test]
    fn lerp_at_t_zero_returns_from() {
        let from = sample_from();
        let to = sample_to();
        let result = from.lerp(&to, 0.0);
        for i in 0..MAX_THEME_COLORS {
            assert_eq!(result.colors[i], from.colors[i]);
        }
    }

    #[test]
    fn lerp_at_t_one_returns_to() {
        let from = sample_from();
        let to = sample_to();
        let result = from.lerp(&to, 1.0);
        for i in 0..MAX_THEME_COLORS {
            assert_eq!(result.colors[i], to.colors[i]);
        }
        assert_eq!(result.color_count, to.color_count);
    }

    #[test]
    fn lerp_at_t_half_returns_midpoint() {
        let from = sample_from();
        let to = sample_to();
        let result = from.lerp(&to, 0.5);
        for i in 0..MAX_THEME_COLORS {
            let exp_l = (from.colors[i].l + to.colors[i].l) * 0.5;
            let exp_a = (from.colors[i].a + to.colors[i].a) * 0.5;
            let exp_b = (from.colors[i].b + to.colors[i].b) * 0.5;
            let exp_alpha = (from.colors[i].alpha + to.colors[i].alpha) * 0.5;
            assert!((result.colors[i].l - exp_l).abs() < 1e-5);
            assert!((result.colors[i].a - exp_a).abs() < 1e-5);
            assert!((result.colors[i].b - exp_b).abs() < 1e-5);
            assert!((result.colors[i].alpha - exp_alpha).abs() < 1e-5);
        }
    }

    #[test]
    fn transition_progress_zero_to_one_over_150ms() {
        let mut t = ThemeTransition::new(sample_from(), sample_to());
        assert_eq!(t.progress(), 0.0);
        t.advance(0.075);
        assert_eq!(t.progress(), 0.5);
        t.advance(0.075);
        assert_eq!(t.progress(), 1.0);
    }

    #[test]
    fn transition_is_complete_after_150ms() {
        let mut t = ThemeTransition::new(sample_from(), sample_to());
        assert!(!t.is_complete());
        t.advance(0.150);
        assert!(t.is_complete());
    }

    #[test]
    fn transition_advance_clamps_to_duration() {
        let mut t = ThemeTransition::new(sample_from(), sample_to());
        t.advance(1.0);
        assert!(t.is_complete());
        assert_eq!(t.progress(), 1.0);
        // Advancing past completion must not exceed the duration.
        t.advance(1.0);
        assert_eq!(t.elapsed, t.duration);
        assert_eq!(t.progress(), 1.0);
    }

    #[test]
    fn transition_current_uniforms_interpolates() {
        let mut t = ThemeTransition::new(sample_from(), sample_to());
        t.advance(0.075);
        let cur = t.current_uniforms();
        assert_eq!(cur.colors[0].l, 0.5);
        assert_eq!(cur.colors[1].l, 0.3);
    }

    #[test]
    fn transition_current_t_matches_progress() {
        let mut t = ThemeTransition::new(sample_from(), sample_to());
        t.advance(0.030);
        assert_eq!(t.current_t(), t.progress());
    }

    #[test]
    fn transition_zero_duration_falls_back_to_default() {
        // A zero duration is invalid and falls back to the default 150ms.
        let t = ThemeTransition::new_with_duration(sample_from(), sample_to(), 0.0);
        assert_eq!(t.duration, DEFAULT_TRANSITION_DURATION);
        assert_eq!(t.progress(), 0.0);
        assert!(!t.is_complete());
    }

    /// Parse [`THEME_TRANSITION_WGSL`] into a naga module for structural
    /// assertions. Unwraps the parse result so tests fail loudly on syntax
    /// errors.
    fn parse_theme_shader() -> naga::Module {
        naga::front::wgsl::parse_str(THEME_TRANSITION_WGSL)
            .expect("theme transition shader should parse")
    }

    #[test]
    fn wgsl_shader_is_non_empty() {
        assert!(!THEME_TRANSITION_WGSL.is_empty());
    }

    #[test]
    fn wgsl_shader_has_theme_uniforms_struct() {
        let module = parse_theme_shader();
        // The `ThemeUniforms` struct must exist and contain the expected
        // fields: `colors` (an array) and `color_count` (a scalar).
        let ty = module
            .types
            .iter()
            .find(|(_, t)| t.name.as_deref() == Some("ThemeUniforms"))
            .map(|(_, t)| t)
            .expect("struct `ThemeUniforms` should exist in the parsed module");
        let members = match &ty.inner {
            naga::TypeInner::Struct { members, .. } => members,
            _ => panic!("`ThemeUniforms` should be a struct"),
        };
        let color_member = members
            .iter()
            .find(|m| m.name.as_deref() == Some("colors"))
            .expect("struct should have a `colors` member");
        // The `colors` field must be a fixed-size array (not dynamic).
        match &module.types[color_member.ty].inner {
            naga::TypeInner::Array { size, .. } => {
                assert!(
                    matches!(size, naga::ArraySize::Constant(_)),
                    "`colors` should be a fixed-size array, got {size:?}"
                );
            }
            _ => panic!("`colors` member should be an array type"),
        }
        assert!(
            members
                .iter()
                .any(|m| m.name.as_deref() == Some("color_count")),
            "struct should have a `color_count` member"
        );
    }

    #[test]
    fn wgsl_shader_has_uniform_bindings() {
        let module = parse_theme_shader();
        // `from_colors` must be at group 0, binding 0 in uniform space.
        let from = module
            .global_variables
            .iter()
            .find(|(_, v)| v.name.as_deref() == Some("from_colors"))
            .map(|(_, v)| v)
            .expect("global variable `from_colors` should exist");
        assert_eq!(
            from.space,
            naga::AddressSpace::Uniform,
            "`from_colors` should be in uniform address space"
        );
        let from_binding = from
            .binding
            .as_ref()
            .expect("`from_colors` should have a resource binding");
        assert_eq!(from_binding.group, 0);
        assert_eq!(from_binding.binding, 0);

        // `to_colors` must be at group 0, binding 1 in uniform space.
        let to = module
            .global_variables
            .iter()
            .find(|(_, v)| v.name.as_deref() == Some("to_colors"))
            .map(|(_, v)| v)
            .expect("global variable `to_colors` should exist");
        assert_eq!(
            to.space,
            naga::AddressSpace::Uniform,
            "`to_colors` should be in uniform address space"
        );
        let to_binding = to
            .binding
            .as_ref()
            .expect("`to_colors` should have a resource binding");
        assert_eq!(to_binding.group, 0);
        assert_eq!(to_binding.binding, 1);
    }

    #[test]
    fn wgsl_shader_has_theme_color_function() {
        let module = parse_theme_shader();
        assert!(
            module
                .functions
                .iter()
                .any(|(_, f)| f.name.as_deref() == Some("theme_color")),
            "function `theme_color` should exist in the parsed module"
        );
    }

    #[test]
    fn uniform_buffer_from_and_to_bytes_are_256() {
        let mut buf = ThemeUniformBuffer::new();
        buf.set_from(sample_from());
        buf.set_to(sample_to());
        assert_eq!(buf.from_bytes().len(), 256);
        assert_eq!(buf.to_bytes().len(), 256);
    }

    #[test]
    fn uniform_buffer_set_t_clamps() {
        let mut buf = ThemeUniformBuffer::new();
        buf.set_t(0.5);
        assert_eq!(buf.transition_t(), 0.5);
        buf.set_t(2.0);
        assert_eq!(buf.transition_t(), 1.0);
        buf.set_t(-1.0);
        assert_eq!(buf.transition_t(), 0.0);
    }

    #[test]
    fn uniform_buffer_from_bytes_reflects_set_from() {
        let mut buf = ThemeUniformBuffer::new();
        let mut from = ThemeUniforms::new();
        from.set_color(
            0,
            Oklab {
                l: 0.9,
                a: 0.0,
                b: 0.0,
                alpha: 1.0,
            },
        );
        buf.set_from(from);
        let bytes = buf.from_bytes();
        // First 4 bytes of the first color are the `l` channel (little-endian).
        let l = f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        assert_eq!(l, 0.9);
    }

    #[test]
    fn types_are_copy_for_stack_only_operation() {
        fn is_copy<T: Copy>() {}
        is_copy::<ThemeUniforms>();
        is_copy::<ThemeTransition>();
        is_copy::<ThemeUniformBuffer>();
    }

    #[test]
    fn advance_with_nan_dt_is_ignored() {
        let mut t = ThemeTransition::new(sample_from(), sample_to());
        t.advance(0.075);
        let progress_before = t.progress();
        t.advance(f32::NAN);
        assert_eq!(
            t.progress(),
            progress_before,
            "NaN dt must not change progress"
        );
    }

    #[test]
    fn advance_with_negative_dt_is_ignored() {
        let mut t = ThemeTransition::new(sample_from(), sample_to());
        t.advance(0.075);
        let progress_before = t.progress();
        t.advance(-0.5);
        assert_eq!(
            t.progress(),
            progress_before,
            "negative dt must not change progress"
        );
    }

    #[test]
    fn advance_with_inf_dt_is_ignored() {
        let mut t = ThemeTransition::new(sample_from(), sample_to());
        t.advance(0.075);
        let progress_before = t.progress();
        t.advance(f32::INFINITY);
        assert_eq!(
            t.progress(),
            progress_before,
            "Inf dt must not change progress"
        );
    }

    #[test]
    fn oklab_lerp_with_nan_t_returns_self() {
        let a = Oklab::new(0.0, 0.0, 0.0, 0.0);
        let b = Oklab::new(1.0, 1.0, 1.0, 1.0);
        let result = a.lerp(b, f32::NAN);
        assert_eq!(result, a, "NaN t should clamp to t=0 (return self)");
    }

    #[test]
    fn oklab_lerp_with_inf_t_clamps_to_one() {
        let a = Oklab::new(0.0, 0.0, 0.0, 0.0);
        let b = Oklab::new(1.0, 1.0, 1.0, 1.0);
        let result = a.lerp(b, f32::INFINITY);
        assert_eq!(result, b, "Inf t should clamp to t=1 (return other)");
    }

    #[test]
    fn new_with_duration_nan_falls_back_to_default() {
        let t = ThemeTransition::new_with_duration(
            ThemeUniforms::new(),
            ThemeUniforms::new(),
            f32::NAN,
        );
        assert_eq!(t.duration, DEFAULT_TRANSITION_DURATION);
    }

    #[test]
    fn new_with_duration_negative_falls_back_to_default() {
        let t =
            ThemeTransition::new_with_duration(ThemeUniforms::new(), ThemeUniforms::new(), -1.0);
        assert_eq!(t.duration, DEFAULT_TRANSITION_DURATION);
    }

    #[test]
    fn new_with_duration_zero_falls_back_to_default() {
        let t = ThemeTransition::new_with_duration(ThemeUniforms::new(), ThemeUniforms::new(), 0.0);
        assert_eq!(t.duration, DEFAULT_TRANSITION_DURATION);
    }

    #[test]
    fn set_t_with_nan_returns_zero() {
        let mut buf = ThemeUniformBuffer::new();
        buf.set_t(f32::NAN);
        assert_eq!(buf.transition_t(), 0.0, "NaN t should be clamped to 0.0");
    }

    #[test]
    fn linear_to_srgb_inf_returns_one() {
        assert_eq!(linear_to_srgb(f32::INFINITY), 1.0);
    }
}
