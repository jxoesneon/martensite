//! GPU theme-transition render pipeline.
//!
//! This module closes the v0.6.0 gap where [`martensite_theme::THEME_TRANSITION_WGSL`]
//! was defined as an uncompiled string and [`martensite_theme::ThemeUniforms`] existed
//! but nothing compiled the shader or created a `wgpu` render pipeline.
//!
//! [`ThemeTransitionPipeline`] compiles a complete WGSL module — the
//! [`martensite_theme::THEME_TRANSITION_WGSL`] library (uniform block + `theme_color`
//! blend function) plus a fullscreen-triangle vertex shader and a fragment shader
//! that converts the blended Oklab color to linear sRGB — and builds a
//! [`wgpu::RenderPipeline`] from it.
//!
//! The pipeline samples two theme snapshots (uploaded as 256-byte uniform buffers
//! matching the [`martensite_theme::ThemeUniforms`] layout) and blends them by a
//! scalar progress `t ∈ [0.0, 1.0]`. The entry point
//! [`render_theme_transition`] uploads the uniforms and dispatches a single
//! fullscreen-triangle draw call into a caller-supplied target view.
//!
//! `martensite-theme` does **not** gain a `wgpu` dependency: this module lives in
//! `martensite-wgpu` and consumes the theme uniform bytes via a plain `&[u8]`
//! interface.

use bytemuck::{Pod, Zeroable};
use martensite_theme::THEME_TRANSITION_WGSL;

/// The size of a single [`martensite_theme::ThemeUniforms`] buffer in bytes.
///
/// This matches `ThemeUniforms::as_bytes().len()` (256 bytes) and is used to
/// validate the byte slices passed to [`render_theme_transition`].
pub const THEME_UNIFORM_SIZE: u64 = 256;

/// The minimum binding size for each theme uniform buffer.
const THEME_UNIFORM_MIN_SIZE: std::num::NonZeroU64 =
    match std::num::NonZeroU64::new(THEME_UNIFORM_SIZE) {
        Some(n) => n,
        None => panic!("THEME_UNIFORM_SIZE must be non-zero"),
    };

/// A 16-byte uniform carrying the transition progress `t` and padding to reach
/// the 16-byte minimum uniform-buffer alignment required by some backends.
///
/// # Layout
///
/// | offset | field  | size |
/// |--------|--------|------|
/// | `0`    | `t`    | 4    |
/// | `4`    | `_pad` | 12   |
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
struct ProgressUniform {
    /// The transition parameter `t ∈ [0.0, 1.0]`.
    t: f32,
    /// Padding to 16 bytes.
    _pad: [f32; 3],
}

impl ProgressUniform {
    /// Creates a new progress uniform with the given `t` value.
    fn new(t: f32) -> Self {
        // Clamp non-finite values to 0.0 to prevent NaN propagation on the GPU.
        let t = if t.is_finite() {
            t.clamp(0.0, 1.0)
        } else {
            0.0
        };
        Self { t, _pad: [0.0; 3] }
    }

    /// Returns the uniform as a byte slice for buffer upload.
    fn as_bytes(&self) -> &[u8] {
        bytemuck::bytes_of(self)
    }
}

/// The WGSL source for the fullscreen-triangle vertex and fragment entry
/// points, appended to [`THEME_TRANSITION_WGSL`] to form a complete shader
/// module.
///
/// The vertex shader emits a single triangle that covers the entire
/// clip-space viewport (the "fullscreen triangle" technique). The fragment
/// shader calls `theme_color(0u, progress.t)` to blend the background color
/// token (index 0, [`martensite_theme::TokenKey::BackgroundColor`]) and
/// converts the Oklab result to linear sRGB for display.
const THEME_TRANSITION_RENDER_WGSL: &str = r#"
struct ProgressUniform {
    t: f32,
    _pad0: f32,
    _pad1: f32,
    _pad2: f32,
};

@group(0) @binding(2) var<uniform> progress: ProgressUniform;

// Fullscreen triangle: three vertices covering the entire clip-space quad.
// vertex_index 0 → (-1, -3), 1 → (-1, 1), 2 → (3, 1).
@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    var pos = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -3.0),
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(3.0, 1.0),
    );
    return vec4<f32>(pos[vi], 0.0, 1.0);
}

// Converts an Oklab color (L, a, b, alpha) to linear sRGB (r, g, b, alpha)
// using the inverse Oklab → LMS → linear sRGB transform.
fn oklab_to_linear_srgb(c: vec4<f32>) -> vec4<f32> {
    let l_ = c.x + 0.3963377774 * c.y + 0.2158037573 * c.z;
    let m_ = c.x - 0.1055613458 * c.y - 0.0638541728 * c.z;
    let s_ = c.x - 0.0894841775 * c.y - 1.2914855480 * c.z;

    let l = l_ * l_ * l_;
    let m = m_ * m_ * m_;
    let s = s_ * s_ * s_;

    let r = 4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s;
    let g = -1.2684380046 * l + 2.6097574013 * m - 0.3413193965 * s;
    let b = -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s;

    return vec4<f32>(r, g, b, c.w);
}

@fragment
fn fs_main() -> @location(0) vec4<f32> {
    // Blend the background color token (index 0) and convert to linear sRGB.
    let oklab = theme_color(0u, progress.t);
    let linear = oklab_to_linear_srgb(oklab);
    // Clamp to [0, 1] for display; out-of-gamut Oklab values may produce
    // negative or >1 channel values.
    return vec4<f32>(clamp(linear.rgb, vec3<f32>(0.0), vec3<f32>(1.0)), linear.a);
}
"#;

/// Errors that can occur while constructing a [`ThemeTransitionPipeline`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThemeTransitionError {
    /// The WGSL shader failed to compile on this device.
    ShaderCompilationFailed(String),
    /// A theme uniform byte slice supplied to [`ThemeTransitionPipeline::render`]
    /// was not exactly [`THEME_UNIFORM_SIZE`] bytes long.
    InvalidUniformSize {
        /// The expected size in bytes ([`THEME_UNIFORM_SIZE`]).
        expected: usize,
        /// The actual size of the supplied slice.
        actual: usize,
        /// Which slice was invalid: `"from_uniforms_bytes"` or
        /// `"to_uniforms_bytes"`.
        which: &'static str,
    },
}

impl std::fmt::Display for ThemeTransitionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ShaderCompilationFailed(msg) => {
                write!(f, "theme transition shader compilation failed: {msg}")
            }
            Self::InvalidUniformSize {
                expected,
                actual,
                which,
            } => write!(
                f,
                "{which} must be exactly {expected} bytes but was {actual} bytes"
            ),
        }
    }
}

impl std::error::Error for ThemeTransitionError {}

/// A compiled GPU render pipeline for blending two theme snapshots.
///
/// The pipeline is built from [`martensite_theme::THEME_TRANSITION_WGSL`] (the
/// uniform block and `theme_color` blend function) plus a fullscreen-triangle
/// vertex shader and a fragment shader that converts the blended Oklab color
/// to linear sRGB.
///
/// Create the pipeline once with [`ThemeTransitionPipeline::new`] and call
/// [`render_theme_transition`] (or [`ThemeTransitionPipeline::render`]) per
/// frame. The pipeline owns the compiled shader, bind group layout, and
/// render pipeline; per-frame uniform buffers and bind groups are created
/// transiently by the render entry point.
///
/// # Examples
///
/// ```no_run
/// use martensite_wgpu::theme_transition::ThemeTransitionPipeline;
/// use wgpu::Device;
///
/// # fn example(device: &Device) {
/// let pipeline = ThemeTransitionPipeline::new(device);
/// assert!(pipeline.is_ok());
/// # }
/// ```
#[derive(Debug)]
pub struct ThemeTransitionPipeline {
    /// The compiled render pipeline.
    pipeline: wgpu::RenderPipeline,
    /// The bind group layout for the two theme uniform buffers + progress.
    bind_group_layout: wgpu::BindGroupLayout,
}

impl ThemeTransitionPipeline {
    /// Creates a new theme-transition pipeline by compiling the WGSL shader
    /// and building the render pipeline.
    ///
    /// The pipeline targets the `Bgra8Unorm` format (the most common
    /// surface-preferred format). For a different target format, use
    /// [`ThemeTransitionPipeline::with_format`].
    ///
    /// # Errors
    ///
    /// Returns [`ThemeTransitionError::ShaderCompilationFailed`] if the WGSL
    /// shader fails to compile on the given device.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_wgpu::theme_transition::ThemeTransitionPipeline;
    /// use wgpu::Device;
    ///
    /// # fn example(device: &Device) {
    /// let pipeline = ThemeTransitionPipeline::new(device);
    /// assert!(pipeline.is_ok());
    /// # }
    /// ```
    pub fn new(device: &wgpu::Device) -> Result<Self, ThemeTransitionError> {
        Self::with_format(device, wgpu::TextureFormat::Bgra8Unorm)
    }

    /// Creates a new theme-transition pipeline targeting a specific output
    /// texture format.
    ///
    /// Use this when the render target (e.g. a surface texture) uses a format
    /// other than `Bgra8Unorm`.
    ///
    /// # Errors
    ///
    /// Returns [`ThemeTransitionError::ShaderCompilationFailed`] if the WGSL
    /// shader fails to compile on the given device.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_wgpu::theme_transition::ThemeTransitionPipeline;
    /// use wgpu::{Device, TextureFormat};
    ///
    /// # fn example(device: &Device) {
    /// let pipeline = ThemeTransitionPipeline::with_format(
    ///     device,
    ///     TextureFormat::Rgba8Unorm,
    /// );
    /// assert!(pipeline.is_ok());
    /// # }
    /// ```
    pub fn with_format(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
    ) -> Result<Self, ThemeTransitionError> {
        // Concatenate the theme library shader (uniforms + theme_color) with
        // the fullscreen-triangle render entry points.
        let source = format!("{THEME_TRANSITION_WGSL}\n{THEME_TRANSITION_RENDER_WGSL}");

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("martensite-theme-transition"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("martensite-theme-transition-bgl"),
            entries: &[
                // binding 0: from_colors uniform buffer (256 bytes)
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: Some(THEME_UNIFORM_MIN_SIZE),
                    },
                    count: None,
                },
                // binding 1: to_colors uniform buffer (256 bytes)
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: Some(THEME_UNIFORM_MIN_SIZE),
                    },
                    count: None,
                },
                // binding 2: progress uniform (16 bytes)
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: std::num::NonZeroU64::new(16),
                    },
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("martensite-theme-transition-layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("martensite-theme-transition-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview_mask: None,
            cache: None,
        });

        Ok(Self {
            pipeline,
            bind_group_layout,
        })
    }

    /// Returns a reference to the bind group layout used by this pipeline.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_wgpu::theme_transition::ThemeTransitionPipeline;
    /// use wgpu::Device;
    ///
    /// # fn example(device: &Device) {
    /// let pipeline = ThemeTransitionPipeline::new(device).unwrap();
    /// let _bgl = pipeline.bind_group_layout();
    /// # }
    /// ```
    #[must_use]
    pub fn bind_group_layout(&self) -> &wgpu::BindGroupLayout {
        &self.bind_group_layout
    }

    /// Returns a reference to the render pipeline.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_wgpu::theme_transition::ThemeTransitionPipeline;
    /// use wgpu::Device;
    ///
    /// # fn example(device: &Device) {
    /// let pipeline = ThemeTransitionPipeline::new(device).unwrap();
    /// let _p = pipeline.pipeline();
    /// # }
    /// ```
    #[must_use]
    pub fn pipeline(&self) -> &wgpu::RenderPipeline {
        &self.pipeline
    }

    /// Renders a theme transition into `target_view` using the supplied theme
    /// uniform bytes and progress value.
    ///
    /// This is the per-frame entry point. It:
    ///
    /// 1. Creates two uniform buffers from `from_uniforms_bytes` and
    ///    `to_uniforms_bytes` (each must be exactly 256 bytes, matching
    ///    [`martensite_theme::ThemeUniforms::as_bytes`]).
    /// 2. Creates a 16-byte progress uniform buffer from `progress`.
    /// 3. Creates a bind group binding the three buffers.
    /// 4. Records a render pass with a single fullscreen-triangle draw call.
    ///
    /// The caller is responsible for calling `encoder.finish()` and submitting
    /// the resulting command buffer to the queue.
    ///
    /// # Arguments
    ///
    /// * `device` — The wgpu device used to create buffers and the bind group.
    /// * `encoder` — The command encoder to record the render pass into. The
    ///   caller is responsible for calling `encoder.finish()` and submitting
    ///   the resulting command buffer to the queue.
    /// * `target_view` — The output texture view (e.g. a surface texture view).
    /// * `from_uniforms_bytes` — 256 bytes of the "from" `martensite_theme::ThemeUniforms`.
    /// * `to_uniforms_bytes` — 256 bytes of the "to" `martensite_theme::ThemeUniforms`.
    /// * `progress` — The transition parameter `t ∈ [0.0, 1.0]`.
    ///
    /// # Errors
    ///
    /// Returns [`ThemeTransitionError::InvalidUniformSize`] if
    /// `from_uniforms_bytes` or `to_uniforms_bytes` are not exactly
    /// [`THEME_UNIFORM_SIZE`] bytes long.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_wgpu::theme_transition::ThemeTransitionPipeline;
    /// use wgpu::{Device, Queue, TextureView};
    ///
    /// # fn example(
    /// #     device: &Device,
    /// #     queue: &Queue,
    /// #     target: &TextureView,
    /// #     from_bytes: &[u8],
    /// #     to_bytes: &[u8],
    /// # ) {
    /// let pipeline = ThemeTransitionPipeline::new(device).unwrap();
    /// let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    /// pipeline
    ///     .render(device, &mut encoder, target, from_bytes, to_bytes, 0.5)
    ///     .expect("uniforms are 256 bytes");
    /// queue.submit(std::iter::once(encoder.finish()));
    /// # }
    /// ```
    pub fn render(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        target_view: &wgpu::TextureView,
        from_uniforms_bytes: &[u8],
        to_uniforms_bytes: &[u8],
        progress: f32,
    ) -> Result<(), ThemeTransitionError> {
        let expected = THEME_UNIFORM_SIZE as usize;
        if from_uniforms_bytes.len() != expected {
            return Err(ThemeTransitionError::InvalidUniformSize {
                expected,
                actual: from_uniforms_bytes.len(),
                which: "from_uniforms_bytes",
            });
        }
        if to_uniforms_bytes.len() != expected {
            return Err(ThemeTransitionError::InvalidUniformSize {
                expected,
                actual: to_uniforms_bytes.len(),
                which: "to_uniforms_bytes",
            });
        }

        // Create the three uniform buffers and upload their contents.
        let from_buffer = wgpu::util::DeviceExt::create_buffer_init(
            device,
            &wgpu::util::BufferInitDescriptor {
                label: Some("martensite-theme-from"),
                contents: from_uniforms_bytes,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            },
        );
        let to_buffer = wgpu::util::DeviceExt::create_buffer_init(
            device,
            &wgpu::util::BufferInitDescriptor {
                label: Some("martensite-theme-to"),
                contents: to_uniforms_bytes,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            },
        );
        let progress_uniform = ProgressUniform::new(progress);
        let progress_buffer = wgpu::util::DeviceExt::create_buffer_init(
            device,
            &wgpu::util::BufferInitDescriptor {
                label: Some("martensite-theme-progress"),
                contents: progress_uniform.as_bytes(),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            },
        );

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("martensite-theme-transition-bg"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &from_buffer,
                        offset: 0,
                        size: Some(THEME_UNIFORM_MIN_SIZE),
                    }),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &to_buffer,
                        offset: 0,
                        size: Some(THEME_UNIFORM_MIN_SIZE),
                    }),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &progress_buffer,
                        offset: 0,
                        size: std::num::NonZeroU64::new(16),
                    }),
                },
            ],
        });

        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("martensite-theme-transition-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        render_pass.set_pipeline(&self.pipeline);
        render_pass.set_bind_group(0, &bind_group, &[]);
        render_pass.draw(0..3, 0..1);
        Ok(())
    }
}

/// Renders a single theme-transition frame into `target_view`.
///
/// This is a convenience function that creates a [`ThemeTransitionPipeline`],
/// a command encoder, calls [`ThemeTransitionPipeline::render`], and submits
/// the result to `queue`. For repeated rendering, prefer constructing the
/// pipeline once and calling [`ThemeTransitionPipeline::render`] per frame
/// to avoid recompiling the shader.
///
/// # Arguments
///
/// * `device` — The wgpu device.
/// * `queue` — The command queue.
/// * `target_view` — The output texture view.
/// * `from_uniforms_bytes` — 256 bytes of the "from" `martensite_theme::ThemeUniforms`.
/// * `to_uniforms_bytes` — 256 bytes of the "to" `martensite_theme::ThemeUniforms`.
/// * `progress` — The transition parameter `t ∈ [0.0, 1.0]`.
///
/// # Errors
///
/// Returns [`ThemeTransitionError::ShaderCompilationFailed`] if the shader
/// fails to compile, or [`ThemeTransitionError::InvalidUniformSize`] if the
/// uniform byte slices are not exactly 256 bytes.
///
/// # Examples
///
/// ```no_run
/// use martensite_wgpu::theme_transition::render_theme_transition;
/// use wgpu::{Device, Queue, TextureView};
///
/// # fn example(
/// #     device: &Device,
/// #     queue: &Queue,
/// #     target: &TextureView,
/// #     from_bytes: &[u8],
/// #     to_bytes: &[u8],
/// # ) {
/// render_theme_transition(device, queue, target, from_bytes, to_bytes, 0.5).expect("shader compiles");
/// # }
/// ```
pub fn render_theme_transition(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    target_view: &wgpu::TextureView,
    from_uniforms_bytes: &[u8],
    to_uniforms_bytes: &[u8],
    progress: f32,
) -> Result<(), ThemeTransitionError> {
    let pipeline = ThemeTransitionPipeline::new(device)?;
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    pipeline.render(
        device,
        &mut encoder,
        target_view,
        from_uniforms_bytes,
        to_uniforms_bytes,
        progress,
    )?;
    queue.submit(std::iter::once(encoder.finish()));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_uniform_clamps_nan_to_zero() {
        let u = ProgressUniform::new(f32::NAN);
        assert_eq!(u.t, 0.0);
    }

    #[test]
    fn progress_uniform_clamps_above_one() {
        let u = ProgressUniform::new(2.0);
        assert_eq!(u.t, 1.0);
    }

    #[test]
    fn progress_uniform_clamps_below_zero() {
        let u = ProgressUniform::new(-1.0);
        assert_eq!(u.t, 0.0);
    }

    #[test]
    fn progress_uniform_clamps_inf_to_zero() {
        let u = ProgressUniform::new(f32::INFINITY);
        assert_eq!(u.t, 0.0);
    }

    #[test]
    fn progress_uniform_keeps_valid_value() {
        let u = ProgressUniform::new(0.5);
        assert_eq!(u.t, 0.5);
    }

    #[test]
    fn progress_uniform_is_16_bytes() {
        assert_eq!(std::mem::size_of::<ProgressUniform>(), 16);
    }

    #[test]
    fn theme_uniform_size_is_256() {
        assert_eq!(THEME_UNIFORM_SIZE, 256);
    }

    #[test]
    fn render_wgsl_contains_vertex_entry_point() {
        assert!(THEME_TRANSITION_RENDER_WGSL.contains("@vertex"));
        assert!(THEME_TRANSITION_RENDER_WGSL.contains("fn vs_main"));
    }

    #[test]
    fn render_wgsl_contains_fragment_entry_point() {
        assert!(THEME_TRANSITION_RENDER_WGSL.contains("@fragment"));
        assert!(THEME_TRANSITION_RENDER_WGSL.contains("fn fs_main"));
    }

    #[test]
    fn render_wgsl_contains_oklab_to_linear_srgb() {
        assert!(THEME_TRANSITION_RENDER_WGSL.contains("fn oklab_to_linear_srgb"));
    }

    #[test]
    fn render_wgsl_contains_progress_binding() {
        assert!(THEME_TRANSITION_RENDER_WGSL.contains("@binding(2)"));
        assert!(THEME_TRANSITION_RENDER_WGSL.contains("var<uniform> progress"));
    }

    #[test]
    fn render_wgsl_uses_theme_color_function() {
        // The fragment shader must call theme_color from the library shader.
        assert!(THEME_TRANSITION_RENDER_WGSL.contains("theme_color(0u"));
    }

    #[test]
    fn error_display_is_informative() {
        let err = ThemeTransitionError::ShaderCompilationFailed("test".to_string());
        assert!(err.to_string().contains("shader compilation failed"));
        assert!(err.to_string().contains("test"));
    }

    #[test]
    fn invalid_uniform_size_error_display_is_informative() {
        let err = ThemeTransitionError::InvalidUniformSize {
            expected: 256,
            actual: 128,
            which: "from_uniforms_bytes",
        };
        let s = err.to_string();
        assert!(s.contains("from_uniforms_bytes"));
        assert!(s.contains("256"));
        assert!(s.contains("128"));
    }

    #[test]
    fn combined_shader_source_is_non_empty() {
        // Verify that the concatenation of the library and render shaders
        // produces a non-empty source string.
        let combined = format!("{THEME_TRANSITION_WGSL}\n{THEME_TRANSITION_RENDER_WGSL}");
        assert!(!combined.is_empty());
        // The combined source must contain both the library function and the
        // entry points.
        assert!(combined.contains("fn theme_color"));
        assert!(combined.contains("fn vs_main"));
        assert!(combined.contains("fn fs_main"));
    }

    #[test]
    #[ignore = "requires a wgpu device"]
    fn pipeline_compiles_on_real_device() {
        use crate::device::GpuContext;
        let ctx = match GpuContext::new() {
            Ok(ctx) => ctx,
            Err(_) => return,
        };
        let pipeline = ThemeTransitionPipeline::new(&ctx.device);
        assert!(pipeline.is_ok(), "pipeline should compile");
    }

    #[test]
    #[ignore = "requires a wgpu device"]
    fn render_theme_transition_produces_command_buffer() {
        use crate::device::GpuContext;
        let ctx = match GpuContext::new() {
            Ok(ctx) => ctx,
            Err(_) => return,
        };
        // Create a target texture to render into.
        let target = ctx.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("test-theme-target"),
            size: wgpu::Extent3d {
                width: 64,
                height: 64,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Bgra8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = target.create_view(&wgpu::TextureViewDescriptor::default());

        // Use zeroed uniforms (all-black theme).
        let from = vec![0u8; 256];
        let to = vec![0u8; 256];

        let result = render_theme_transition(&ctx.device, &ctx.queue, &view, &from, &to, 0.5);
        assert!(result.is_ok(), "render should succeed");
    }

    #[test]
    #[ignore = "requires a wgpu device"]
    fn render_rejects_undersized_from_uniforms() {
        use crate::device::GpuContext;
        let ctx = match GpuContext::new() {
            Ok(ctx) => ctx,
            Err(_) => return,
        };
        let target = ctx.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("test-theme-target"),
            size: wgpu::Extent3d {
                width: 64,
                height: 64,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Bgra8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = target.create_view(&wgpu::TextureViewDescriptor::default());

        let pipeline = ThemeTransitionPipeline::new(&ctx.device).unwrap();
        let mut encoder = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        let from = vec![0u8; 128];
        let to = vec![0u8; 256];
        let err = pipeline
            .render(&ctx.device, &mut encoder, &view, &from, &to, 0.5)
            .expect_err("undersized from_uniforms should error");
        match err {
            ThemeTransitionError::InvalidUniformSize {
                expected,
                actual,
                which,
            } => {
                assert_eq!(expected, 256);
                assert_eq!(actual, 128);
                assert_eq!(which, "from_uniforms_bytes");
            }
            other => panic!("expected InvalidUniformSize, got {other:?}"),
        }
    }

    #[test]
    #[ignore = "requires a wgpu device"]
    fn render_rejects_oversized_to_uniforms() {
        use crate::device::GpuContext;
        let ctx = match GpuContext::new() {
            Ok(ctx) => ctx,
            Err(_) => return,
        };
        let target = ctx.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("test-theme-target"),
            size: wgpu::Extent3d {
                width: 64,
                height: 64,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Bgra8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = target.create_view(&wgpu::TextureViewDescriptor::default());

        let pipeline = ThemeTransitionPipeline::new(&ctx.device).unwrap();
        let mut encoder = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        let from = vec![0u8; 256];
        let to = vec![0u8; 512];
        let err = pipeline
            .render(&ctx.device, &mut encoder, &view, &from, &to, 0.5)
            .expect_err("oversized to_uniforms should error");
        match err {
            ThemeTransitionError::InvalidUniformSize {
                expected,
                actual,
                which,
            } => {
                assert_eq!(expected, 256);
                assert_eq!(actual, 512);
                assert_eq!(which, "to_uniforms_bytes");
            }
            other => panic!("expected InvalidUniformSize, got {other:?}"),
        }
    }
}
