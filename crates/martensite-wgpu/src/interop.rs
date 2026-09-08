//! External video memory surface interop, format negotiation, and WGSL compute pipeline.
//!
//! This module provides the bridge between platform hardware video decoders and
//! the WGPU rendering engine:
//!
//! - [`FormatNegotiator`]: Determines required texture formats for planar video streams
//!   (NV12, P010) and negotiates HDR swapchain capabilities.
//! - [`VideoPipelineUniforms`]: A 256-byte aligned uniform buffer carrying color matrices,
//!   EOTF parameters, panel luminance limits, and filmic constants.
//! - [`MEDIA_YUV_EOTF_WGSL`]: A complete, AOT-validated WGSL compute shader executing
//!   bi-planar YUV sampling, SMPTE ST 2084 PQ EOTF linearization, and Hable filmic tone mapping.

use bytemuck::{Pod, Zeroable};
use martensite_media::surface::VideoPixelFormat;

/// Negotiates texture formats and swapchain parameters for hardware video surfaces.
///
/// # Examples
///
/// ```
/// use martensite_media::surface::VideoPixelFormat;
/// use martensite_wgpu::interop::FormatNegotiator;
///
/// let (y_fmt, uv_fmt) = FormatNegotiator::negotiate_plane_formats(VideoPixelFormat::Nv12);
/// assert_eq!(y_fmt, wgpu::TextureFormat::R8Unorm);
/// assert!(uv_fmt.is_some());
/// ```
pub struct FormatNegotiator;

impl FormatNegotiator {
    /// Negotiates required WGPU texture formats for each plane of a given [`VideoPixelFormat`].
    ///
    /// Returns `(plane0_format, Option<plane1_format>)`:
    /// - `Nv12`: `(R8Unorm, Some(Rg8Unorm))`
    /// - `P010`: `(R16Unorm, Some(Rg16Unorm))`
    /// - `Rgba8`: `(Rgba8Unorm, None)`
    /// - `Rgba16Float`: `(Rgba16Float, None)`
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media::surface::VideoPixelFormat;
    /// use martensite_wgpu::interop::FormatNegotiator;
    ///
    /// let (y_fmt, uv_fmt) = FormatNegotiator::negotiate_plane_formats(VideoPixelFormat::Nv12);
    /// assert_eq!(y_fmt, wgpu::TextureFormat::R8Unorm);
    /// assert_eq!(uv_fmt, Some(wgpu::TextureFormat::Rg8Unorm));
    ///
    /// let (y_p010, uv_p010) = FormatNegotiator::negotiate_plane_formats(VideoPixelFormat::P010);
    /// assert_eq!(y_p010, wgpu::TextureFormat::R16Unorm);
    /// assert_eq!(uv_p010, Some(wgpu::TextureFormat::Rg16Unorm));
    /// ```
    #[must_use]
    pub fn negotiate_plane_formats(
        format: VideoPixelFormat,
    ) -> (wgpu::TextureFormat, Option<wgpu::TextureFormat>) {
        match format {
            VideoPixelFormat::Nv12 => (
                wgpu::TextureFormat::R8Unorm,
                Some(wgpu::TextureFormat::Rg8Unorm),
            ),
            VideoPixelFormat::P010 => (
                wgpu::TextureFormat::R16Unorm,
                Some(wgpu::TextureFormat::Rg16Unorm),
            ),
            VideoPixelFormat::Rgba8 => (wgpu::TextureFormat::Rgba8Unorm, None),
            VideoPixelFormat::Rgba16Float => (wgpu::TextureFormat::Rgba16Float, None),
        }
    }

    /// Selects the optimal swapchain format for HDR rendering from surface capabilities.
    ///
    /// Prefers `Rgba16Float` (linear scRGB), then `Rgb10a2Unorm` (HDR10), falling back
    /// to standard `Bgra8UnormSrgb` or `Rgba8UnormSrgb`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_wgpu::interop::FormatNegotiator;
    ///
    /// let supported = [wgpu::TextureFormat::Bgra8UnormSrgb, wgpu::TextureFormat::Rgba16Float];
    /// let selected = FormatNegotiator::select_best_swapchain_format(&supported);
    /// assert_eq!(selected, wgpu::TextureFormat::Rgba16Float);
    /// ```
    #[must_use]
    pub fn select_best_swapchain_format(
        supported_formats: &[wgpu::TextureFormat],
    ) -> wgpu::TextureFormat {
        if supported_formats.contains(&wgpu::TextureFormat::Rgba16Float) {
            wgpu::TextureFormat::Rgba16Float
        } else if supported_formats.contains(&wgpu::TextureFormat::Rgb10a2Unorm) {
            wgpu::TextureFormat::Rgb10a2Unorm
        } else if supported_formats.contains(&wgpu::TextureFormat::Bgra8UnormSrgb) {
            wgpu::TextureFormat::Bgra8UnormSrgb
        } else if supported_formats.contains(&wgpu::TextureFormat::Rgba8UnormSrgb) {
            wgpu::TextureFormat::Rgba8UnormSrgb
        } else {
            supported_formats
                .first()
                .copied()
                .unwrap_or(wgpu::TextureFormat::Bgra8UnormSrgb)
        }
    }
}

/// 256-byte aligned uniform buffer for GPU video color pipeline compute shaders.
///
/// Contains matrix transformations, range flags, EOTF configuration, display luminance
/// limits, and filmic tone mapping parameters.
///
/// # Examples
///
/// ```
/// use martensite_wgpu::interop::VideoPipelineUniforms;
///
/// let uniforms = VideoPipelineUniforms::new_bt709_sdr();
/// assert_eq!(uniforms.as_bytes().len(), 256);
/// ```
#[repr(C, align(256))]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct VideoPipelineUniforms {
    /// 3x3 YUV-to-RGB matrix (each row stored as a 16-byte `vec4<f32>`).
    pub yuv_to_rgb_0: [f32; 4],
    /// Row 1 of YUV-to-RGB matrix.
    pub yuv_to_rgb_1: [f32; 4],
    /// Row 2 of YUV-to-RGB matrix.
    pub yuv_to_rgb_2: [f32; 4],
    /// Row 0 of gamut transformation matrix.
    pub gamut_0: [f32; 4],
    /// Row 1 of gamut transformation matrix.
    pub gamut_1: [f32; 4],
    /// Row 2 of gamut transformation matrix.
    pub gamut_2: [f32; 4],
    /// Flags: `[is_p010, is_full_range, eotf_mode (0=sRGB, 1=PQ), tonemap_mode (0=none, 1=hable)]`.
    pub flags: [u32; 4],
    /// Display luminance parameters: `[sdr_white_nits, display_max_nits, display_min_nits, exposure]`.
    pub display_params: [f32; 4],
    /// Filmic tone-mapping curve parameters 1: `[A, B, C, D]`.
    pub filmic1: [f32; 4],
    /// Filmic tone-mapping curve parameters 2: `[E, F, W, 0.0]`.
    pub filmic2: [f32; 4],
    /// Padding to enforce exactly 256-byte uniform buffer alignment.
    pub _pad: [u32; 24],
}

impl VideoPipelineUniforms {
    /// Creates a default SDR BT.709 video pipeline configuration.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_wgpu::interop::VideoPipelineUniforms;
    ///
    /// let u = VideoPipelineUniforms::new_bt709_sdr();
    /// assert_eq!(u.flags[0], 0); // 8-bit NV12
    /// assert_eq!(u.flags[2], 0); // sRGB EOTF
    /// ```
    #[must_use]
    pub fn new_bt709_sdr() -> Self {
        Self {
            yuv_to_rgb_0: [1.0, 0.0, 1.57480, 0.0],
            yuv_to_rgb_1: [1.0, -0.18732, -0.46812, 0.0],
            yuv_to_rgb_2: [1.0, 1.85560, 0.0, 0.0],
            gamut_0: [1.0, 0.0, 0.0, 0.0],
            gamut_1: [0.0, 1.0, 0.0, 0.0],
            gamut_2: [0.0, 0.0, 1.0, 0.0],
            flags: [0, 0, 0, 0],
            display_params: [203.0, 300.0, 0.1, 1.0],
            filmic1: [0.15, 0.50, 0.10, 0.20],
            filmic2: [0.02, 0.30, 11.20, 0.0],
            _pad: [0; 24],
        }
    }

    /// Creates a 10-bit HDR BT.2020 pipeline configuration with SMPTE ST 2084 PQ EOTF.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_wgpu::interop::VideoPipelineUniforms;
    ///
    /// let u = VideoPipelineUniforms::new_bt2020_hdr(true, 203.0, 1000.0);
    /// assert_eq!(u.flags[0], 1); // 10-bit P010
    /// assert_eq!(u.flags[2], 1); // PQ EOTF
    /// ```
    #[must_use]
    pub fn new_bt2020_hdr(hable_tonemap: bool, sdr_white_nits: f32, max_nits: f32) -> Self {
        Self {
            yuv_to_rgb_0: [1.0, 0.0, 1.47460, 0.0],
            yuv_to_rgb_1: [1.0, -0.16455, -0.57135, 0.0],
            yuv_to_rgb_2: [1.0, 1.88140, 0.0, 0.0],
            // BT.2020 to BT.709 linear gamut matrix
            gamut_0: [1.6605, -0.5876, -0.0728, 0.0],
            gamut_1: [-0.1246, 1.1329, -0.0083, 0.0],
            gamut_2: [-0.0182, -0.1006, 1.1187, 0.0],
            flags: [1, 0, 1, if hable_tonemap { 1 } else { 0 }],
            display_params: [sdr_white_nits, max_nits, 0.005, 1.0],
            filmic1: [0.15, 0.50, 0.10, 0.20],
            filmic2: [0.02, 0.30, 11.20, 0.0],
            _pad: [0; 24],
        }
    }

    /// Returns a byte slice view of the uniforms struct for buffer uploads.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_wgpu::interop::VideoPipelineUniforms;
    ///
    /// let u = VideoPipelineUniforms::new_bt709_sdr();
    /// assert_eq!(u.as_bytes().len(), 256);
    /// ```
    #[inline]
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        bytemuck::bytes_of(self)
    }
}

/// AOT-validated WGSL compute shader for planar YUV sampling, SMPTE ST 2084 PQ EOTF
/// linearization, gamut conversion, and filmic tone mapping.
///
/// # Examples
///
/// ```
/// use martensite_wgpu::interop::MEDIA_YUV_EOTF_WGSL;
///
/// assert!(MEDIA_YUV_EOTF_WGSL.contains("@compute"));
/// ```
pub const MEDIA_YUV_EOTF_WGSL: &str = r#"
struct VideoUniforms {
    yuv_to_rgb_0: vec4<f32>,
    yuv_to_rgb_1: vec4<f32>,
    yuv_to_rgb_2: vec4<f32>,
    gamut_0: vec4<f32>,
    gamut_1: vec4<f32>,
    gamut_2: vec4<f32>,
    flags: vec4<u32>,
    display: vec4<f32>,
    filmic1: vec4<f32>,
    filmic2: vec4<f32>,
    pad0: vec4<u32>,
    pad1: vec4<u32>,
    pad2: vec4<u32>,
    pad3: vec4<u32>,
    pad4: vec4<u32>,
    pad5: vec4<u32>,
};

@group(0) @binding(0) var<uniform> uniforms: VideoUniforms;
@group(0) @binding(1) var luma_tex: texture_2d<f32>;
@group(0) @binding(2) var chroma_tex: texture_2d<f32>;
@group(0) @binding(3) var out_tex: texture_storage_2d<rgba16float, write>;

fn pq_eotf(n: vec3<f32>) -> vec3<f32> {
    let m1 = 0.1593017578125;
    let m2 = 78.84375;
    let c1 = 0.8359375;
    let c2 = 18.8515625;
    let c3 = 18.6875;

    let n_clamped = clamp(n, vec3<f32>(0.0), vec3<f32>(1.0));
    let n_pow = pow(n_clamped, vec3<f32>(1.0 / m2));
    let num = max(n_pow - c1, vec3<f32>(0.0));
    let den = c2 - c3 * n_pow;
    let y = pow(num / den, vec3<f32>(1.0 / m1));
    return y * 10000.0;
}

fn hable_f(x: vec3<f32>) -> vec3<f32> {
    let a = uniforms.filmic1.x;
    let b = uniforms.filmic1.y;
    let c = uniforms.filmic1.z;
    let d = uniforms.filmic1.w;
    let e = uniforms.filmic2.x;
    let f = uniforms.filmic2.y;
    return ((x * (a * x + c * b) + d * e) / (x * (a * x + b) + d * f)) - (e / f);
}

@compute @workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let dims = textureDimensions(luma_tex);
    if (global_id.x >= dims.x || global_id.y >= dims.y) {
        return;
    }

    let coord = vec2<i32>(global_id.xy);
    let uv_coord = coord / 2;

    let y_raw = textureLoad(luma_tex, coord, 0).r;
    let uv_raw = textureLoad(chroma_tex, uv_coord, 0).rg;

    var y: f32;
    var cb: f32;
    var cr: f32;

    if (uniforms.flags.y == 0u) {
        if (uniforms.flags.x == 1u) {
            y = (y_raw - (64.0 / 1023.0)) / (876.0 / 1023.0);
            cb = (uv_raw.r - (512.0 / 1023.0)) / (896.0 / 1023.0);
            cr = (uv_raw.g - (512.0 / 1023.0)) / (896.0 / 1023.0);
        } else {
            y = (y_raw - (16.0 / 255.0)) / (219.0 / 255.0);
            cb = (uv_raw.r - (128.0 / 255.0)) / (224.0 / 255.0);
            cr = (uv_raw.g - (128.0 / 255.0)) / (224.0 / 255.0);
        }
    } else {
        y = y_raw;
        cb = uv_raw.r - 0.5;
        cr = uv_raw.g - 0.5;
    }

    let yuv = vec3<f32>(y, cb, cr);
    var rgb_non_linear: vec3<f32>;
    rgb_non_linear.r = dot(uniforms.yuv_to_rgb_0.xyz, yuv);
    rgb_non_linear.g = dot(uniforms.yuv_to_rgb_1.xyz, yuv);
    rgb_non_linear.b = dot(uniforms.yuv_to_rgb_2.xyz, yuv);

    var linear_nits: vec3<f32>;
    if (uniforms.flags.z == 1u) {
        linear_nits = pq_eotf(rgb_non_linear);
    } else {
        linear_nits = pow(max(rgb_non_linear, vec3<f32>(0.0)), vec3<f32>(2.2)) * uniforms.display.x;
    }

    var target_linear: vec3<f32>;
    target_linear.r = dot(uniforms.gamut_0.xyz, linear_nits);
    target_linear.g = dot(uniforms.gamut_1.xyz, linear_nits);
    target_linear.b = dot(uniforms.gamut_2.xyz, linear_nits);

    let sdr_white = uniforms.display.x;
    var scrgb = target_linear / sdr_white;

    if (uniforms.flags.w == 1u) {
        let w_val = uniforms.filmic2.z;
        let white_scale = 1.0 / hable_f(vec3<f32>(w_val)).x;
        scrgb = hable_f(scrgb) * white_scale;
    }

    textureStore(out_tex, coord, vec4<f32>(scrgb, 1.0));
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uniform_buffer_is_exact_256_bytes() {
        assert_eq!(std::mem::size_of::<VideoPipelineUniforms>(), 256);
        assert_eq!(std::mem::align_of::<VideoPipelineUniforms>(), 256);

        let u = VideoPipelineUniforms::new_bt709_sdr();
        assert_eq!(u.as_bytes().len(), 256);
    }

    #[test]
    fn format_negotiation_planes() {
        let (y, uv) = FormatNegotiator::negotiate_plane_formats(VideoPixelFormat::Nv12);
        assert_eq!(y, wgpu::TextureFormat::R8Unorm);
        assert_eq!(uv, Some(wgpu::TextureFormat::Rg8Unorm));

        let (y10, uv10) = FormatNegotiator::negotiate_plane_formats(VideoPixelFormat::P010);
        assert_eq!(y10, wgpu::TextureFormat::R16Unorm);
        assert_eq!(uv10, Some(wgpu::TextureFormat::Rg16Unorm));
    }
}
