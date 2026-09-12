//! External GPU surface compositing (Milestone `v0.14.0`).
//!
//! [`WgpuHost`] is the consumer half of the `martensite-engine-bridge`
//! protocol: it samples a producer's `wgpu::Texture` and draws it into
//! the frame target with a six-vertex quad pass — the
//! `netrender::ExternalTextureComposite` pattern — with **zero GPU
//! copies** (no `queue.write_texture`, no readback, no Vello
//! `COPY_SRC` image-atlas path).
//!
//! The same pass also blits Vello segment textures into the frame
//! target: `vello::Renderer::render_to_texture` overwrites its whole
//! target with the base color (it is a compute-store, not a blend), so
//! paint-ordered interleaving is achieved by rendering each command
//! span to its own offscreen texture and compositing all textures —
//! Vello segments and external surfaces alike — inside **one** render
//! pass / encoder, in paint order. See
//! [`RenderOrchestrator`](crate::RenderOrchestrator).
//!
//! # Color contract
//!
//! External textures are expected to contain **linear-space** color
//! (the native convention of game-engine render targets such as
//! Bevy's). When the frame target is an `*Srgb` format the blend unit
//! and store path perform the sRGB encode automatically; on non-sRGB
//! targets the `*_encode` fragment entry points apply the sRGB curve
//! in-shader.
//!
//! Vello segment textures contain **straight-alpha** sRGB-encoded
//! pixels (Vello's fine shader unpremultiplies before `textureStore`).
//! They are sampled through a view whose sRGB-ness matches the target
//! and drawn with the straight→premul pipeline: on sRGB targets the
//! sample decodes, the blend unit re-encodes; on unorm targets the
//! premultiply happens in the encoded space, matching Vello's own
//! blending convention byte-for-byte.

pub use martensite_engine_bridge::SourceAlpha;
use std::cell::Cell;
use std::collections::HashMap;

/// WGSL for the composite pass.
///
/// One module, four fragment entry points. `fs_premul` /
/// `fs_straight` pass the sample through (premultiplied source, or
/// straight-alpha premultiplied in-shader) for sRGB targets where the
/// hardware performs the encode. `fs_premul_encode` /
/// `fs_straight_encode` additionally apply the sRGB curve for
/// non-sRGB targets. The vertex stage expands a unit quad into the
/// destination rect in target-pixel space via the rect uniform; the
/// uniform buffer is bound with a dynamic offset so one buffer
/// serves every composite in the frame.
const EXTERNAL_SHADER: &str = r"
struct RectUniform {
    rect: vec4<f32>,
    dst_size: vec4<f32>,
};

@group(0) @binding(0) var src_tex: texture_2d<f32>;
@group(0) @binding(1) var src_samp: sampler;
@group(1) @binding(0) var<uniform> uni: RectUniform;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

const QUAD: array<vec2<f32>, 6> = array<vec2<f32>, 6>(
    vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0),
    vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0),
);

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> VsOut {
    let p = QUAD[i];
    let px = uni.rect.xy + p * uni.rect.zw;
    let ndc = vec2<f32>(
        px.x / uni.dst_size.x * 2.0 - 1.0,
        1.0 - px.y / uni.dst_size.y * 2.0,
    );
    var out: VsOut;
    out.pos = vec4<f32>(ndc, 0.0, 1.0);
    out.uv = p;
    return out;
}

fn linear_to_srgb(c: vec3<f32>) -> vec3<f32> {
    let lo = c * 12.92;
    let hi = 1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c <= vec3<f32>(0.0031308));
}

@fragment
fn fs_premul(in: VsOut) -> @location(0) vec4<f32> {
    return textureSample(src_tex, src_samp, in.uv);
}

@fragment
fn fs_straight(in: VsOut) -> @location(0) vec4<f32> {
    let c = textureSample(src_tex, src_samp, in.uv);
    return vec4<f32>(c.rgb * c.a, c.a);
}

@fragment
fn fs_premul_encode(in: VsOut) -> @location(0) vec4<f32> {
    let c = textureSample(src_tex, src_samp, in.uv);
    return vec4<f32>(linear_to_srgb(c.rgb), c.a);
}

@fragment
fn fs_straight_encode(in: VsOut) -> @location(0) vec4<f32> {
    let c = textureSample(src_tex, src_samp, in.uv);
    return vec4<f32>(linear_to_srgb(c.rgb * c.a), c.a);
}
";

/// The frame target a [`WgpuHost::composite`] call draws into.
///
/// Bundles the encoder/queue/view/size parameters so `composite` stays
/// readable — equivalent to the `(encoder, queue, view, size)` tuple
/// passed between render stages.
///
/// # Examples
///
/// ```no_run
/// use martensite_wgpu::external::CompositeTarget;
/// # let (device, queue): (wgpu::Device, wgpu::Queue) = todo!();
/// # let view: wgpu::TextureView = todo!();
/// let mut encoder = device.create_command_encoder(&Default::default());
/// let target = CompositeTarget {
///     encoder: &mut encoder,
///     queue: &queue,
///     view: &view,
///     size: (1920, 1080),
/// };
/// ```
pub struct CompositeTarget<'a> {
    /// The command encoder to record the composite pass into.
    pub encoder: &'a mut wgpu::CommandEncoder,
    /// The queue used for uniform-buffer uploads (same-queue ordering
    /// keeps the write ahead of the draw in this pass).
    pub queue: &'a wgpu::Queue,
    /// The target texture view (the surface or offscreen frame).
    pub view: &'a wgpu::TextureView,
    /// Target dimensions in physical pixels.
    pub size: (u32, u32),
}

/// Per-surface GPU resources: the source texture's bind group and
/// geometry metadata. The rect transform is shared — see
/// [`WgpuHost`]'s `rect_buffer`.
struct ExternalEntry {
    bind_group: wgpu::BindGroup,
    alpha: SourceAlpha,
    size: (u32, u32),
}

/// Errors returned by [`WgpuHost`] operations.
#[derive(Debug)]
pub enum ExternalError {
    /// The `surface_id` is not registered with this host.
    UnknownSurface(u64),
    /// More composites were recorded in one frame than the shared
    /// rect-uniform buffer can hold. Call
    /// [`WgpuHost::begin_frame`] once per frame; the capacity is
    /// reported by [`WgpuHost::rect_capacity`].
    RectCapacityExceeded,
}

impl std::fmt::Display for ExternalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownSurface(id) => write!(f, "external surface {id} is not registered"),
            Self::RectCapacityExceeded => {
                write!(f, "rect uniform capacity exceeded for one frame")
            }
        }
    }
}

impl std::error::Error for ExternalError {}

/// Which composite pipeline a draw needs.
#[derive(Copy, Clone)]
enum PipelineKind {
    /// Pass-through premultiplied sample (Vello segments always use
    /// this; external premul sources use it on sRGB targets).
    PremulPassthrough,
    /// Premultiplied source with in-shader sRGB encode (external premul
    /// sources on non-sRGB targets).
    PremulEncode,
    /// Straight-alpha source premultiplied in-shader (sRGB targets).
    StraightPassthrough,
    /// Straight-alpha source premultiplied and sRGB-encoded in-shader
    /// (non-sRGB targets).
    StraightEncode,
}

/// Composites external GPU textures into a frame target.
///
/// One `WgpuHost` is bound to a target texture format at construction
/// (pipelines are format-specific); the orchestrator lazily creates it
/// for the configured surface format and recreates it if the surface
/// is reconfigured with a different format.
///
/// # Frame discipline
///
/// [`WgpuHost::begin_frame`] must be called before the first
/// [`composite`](WgpuHost::composite) or
/// [`composite_view`](WgpuHost::composite_view) of a frame: each call
/// uploads the destination rect into a shared dynamic-offset uniform
/// buffer, so every draw in the pass reads its own rect even when the
/// same surface is composited several times.
///
/// # Examples
///
/// ```no_run
/// use martensite_wgpu::external::WgpuHost;
/// # let device: wgpu::Device = todo!();
/// let host = WgpuHost::new(&device, wgpu::TextureFormat::Bgra8UnormSrgb);
/// host.begin_frame();
/// assert_eq!(host.surface_count(), 0);
/// ```
pub struct WgpuHost {
    texture_layout: wgpu::BindGroupLayout,
    rect_bind_group: wgpu::BindGroup,
    rect_buffer: wgpu::Buffer,
    rect_align: u64,
    rect_capacity: u32,
    rect_cursor: Cell<u32>,
    sampler: wgpu::Sampler,
    pipeline_premul_passthrough: wgpu::RenderPipeline,
    pipeline_premul_encode: wgpu::RenderPipeline,
    pipeline_straight_passthrough: wgpu::RenderPipeline,
    pipeline_straight_encode: wgpu::RenderPipeline,
    target_format: wgpu::TextureFormat,
    target_is_srgb: bool,
    entries: HashMap<u64, ExternalEntry>,
}

impl WgpuHost {
    /// Maximum composites recordable in one frame.
    const RECT_SLOTS: u32 = 128;

    /// Creates a host whose pipelines target `target_format` (the
    /// configured surface or offscreen format).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_wgpu::external::WgpuHost;
    /// # let device: wgpu::Device = todo!();
    /// let host = WgpuHost::new(&device, wgpu::TextureFormat::Bgra8UnormSrgb);
    /// assert!(host.target_is_srgb());
    /// ```
    pub fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("external-composite-shader"),
            source: wgpu::ShaderSource::Wgsl(EXTERNAL_SHADER.into()),
        });

        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("external-texture-layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        // Shared dynamic-offset uniform: one 256-aligned slot per
        // composite call, so every draw in the pass reads its own rect.
        let rect_align = u64::from(device.limits().min_uniform_buffer_offset_alignment.max(64));
        let rect_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("external-rect-uniforms"),
            size: rect_align * u64::from(Self::RECT_SLOTS),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let rect_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("external-rect-layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: wgpu::BufferSize::new(32),
                },
                count: None,
            }],
        });
        let rect_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("external-rect-bind-group"),
            layout: &rect_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &rect_buffer,
                    offset: 0,
                    size: wgpu::BufferSize::new(32),
                }),
            }],
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("external-sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("external-composite-layout"),
            bind_group_layouts: &[Some(&texture_layout), Some(&rect_layout)],
            immediate_size: 0,
        });

        let make_pipeline = |fragment: &str| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("external-composite-pipeline"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fragment),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: target_format,
                        blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            })
        };

        // sRGB targets encode on store, and float/HDR targets store
        // linear values — both take the pass-through pipelines for
        // external sources. Only unorm non-sRGB targets need the
        // in-shader sRGB encode.
        let target_is_srgb = target_format.is_srgb()
            || matches!(
                target_format,
                wgpu::TextureFormat::R16Float
                    | wgpu::TextureFormat::Rg16Float
                    | wgpu::TextureFormat::Rgba16Float
                    | wgpu::TextureFormat::R32Float
                    | wgpu::TextureFormat::Rg32Float
                    | wgpu::TextureFormat::Rgba32Float
                    | wgpu::TextureFormat::Rgb10a2Unorm
                    | wgpu::TextureFormat::Rg11b10Ufloat
            );

        Self {
            texture_layout,
            rect_bind_group,
            rect_buffer,
            rect_align,
            rect_capacity: Self::RECT_SLOTS,
            rect_cursor: Cell::new(0),
            sampler,
            pipeline_premul_passthrough: make_pipeline("fs_premul"),
            pipeline_premul_encode: make_pipeline("fs_premul_encode"),
            pipeline_straight_passthrough: make_pipeline("fs_straight"),
            pipeline_straight_encode: make_pipeline("fs_straight_encode"),
            target_format,
            target_is_srgb,
            entries: HashMap::new(),
        }
    }

    /// The texture format the pipelines were built for.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_wgpu::external::WgpuHost;
    /// # let device: wgpu::Device = todo!();
    /// let host = WgpuHost::new(&device, wgpu::TextureFormat::Bgra8UnormSrgb);
    /// assert_eq!(host.target_format(), wgpu::TextureFormat::Bgra8UnormSrgb);
    /// ```
    #[must_use]
    pub fn target_format(&self) -> wgpu::TextureFormat {
        self.target_format
    }

    /// Whether the target format stores sRGB-encoded pixels (i.e. the
    /// blend/store path performs the linear→sRGB encode).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_wgpu::external::WgpuHost;
    /// # let device: wgpu::Device = todo!();
    /// let host = WgpuHost::new(&device, wgpu::TextureFormat::Bgra8UnormSrgb);
    /// assert!(host.target_is_srgb());
    /// ```
    #[must_use]
    pub fn target_is_srgb(&self) -> bool {
        self.target_is_srgb
    }

    /// Resets the shared rect-uniform cursor. Call once before the
    /// first [`composite`](WgpuHost::composite) of each frame.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_wgpu::external::WgpuHost;
    /// # let device: wgpu::Device = todo!();
    /// let host = WgpuHost::new(&device, wgpu::TextureFormat::Bgra8UnormSrgb);
    /// host.begin_frame();
    /// ```
    pub fn begin_frame(&self) {
        self.rect_cursor.set(0);
    }

    /// The number of composites recordable per frame.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_wgpu::external::WgpuHost;
    /// # let device: wgpu::Device = todo!();
    /// let host = WgpuHost::new(&device, wgpu::TextureFormat::Bgra8UnormSrgb);
    /// assert_eq!(host.rect_capacity(), 128);
    /// ```
    #[must_use]
    pub fn rect_capacity(&self) -> u32 {
        self.rect_capacity
    }

    /// Allocates the next rect slot and uploads the uniform.
    fn write_rect(
        &self,
        queue: &wgpu::Queue,
        rect: [f32; 4],
        dst_size: (u32, u32),
    ) -> Result<u32, ExternalError> {
        let slot = self.rect_cursor.get();
        if slot >= self.rect_capacity {
            return Err(ExternalError::RectCapacityExceeded);
        }
        self.rect_cursor.set(slot + 1);
        let uniform: [f32; 8] = [
            rect[0],
            rect[1],
            rect[2],
            rect[3],
            dst_size.0 as f32,
            dst_size.1 as f32,
            0.0,
            0.0,
        ];
        queue.write_buffer(
            &self.rect_buffer,
            u64::from(slot) * self.rect_align,
            bytemuck::cast_slice(&uniform),
        );
        Ok(slot * self.rect_align as u32)
    }

    fn pipeline(&self, kind: PipelineKind) -> &wgpu::RenderPipeline {
        match kind {
            PipelineKind::PremulPassthrough => &self.pipeline_premul_passthrough,
            PipelineKind::PremulEncode => &self.pipeline_premul_encode,
            PipelineKind::StraightPassthrough => &self.pipeline_straight_passthrough,
            PipelineKind::StraightEncode => &self.pipeline_straight_encode,
        }
    }

    /// The pipeline for an external source with alpha mode `alpha`.
    fn external_pipeline(&self, alpha: SourceAlpha) -> &wgpu::RenderPipeline {
        let kind = match (alpha, self.target_is_srgb) {
            (SourceAlpha::Premultiplied, true) => PipelineKind::PremulPassthrough,
            (SourceAlpha::Premultiplied, false) => PipelineKind::PremulEncode,
            (SourceAlpha::Straight, true) => PipelineKind::StraightPassthrough,
            (SourceAlpha::Straight, false) => PipelineKind::StraightEncode,
        };
        self.pipeline(kind)
    }

    /// Registers a producer texture under `surface_id`.
    ///
    /// Re-registering the same `surface_id` replaces the previous entry
    /// (producers swap textures when the ring slot cycles). The
    /// texture must be `TEXTURE_BINDING`-capable and `Float`-
    /// filterable; it is sampled until [`WgpuHost::unregister`] or the
    /// next `register_texture` for the same id.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_wgpu::external::{WgpuHost, SourceAlpha};
    /// # let device: wgpu::Device = todo!();
    /// # let texture: wgpu::Texture = todo!();
    /// let mut host = WgpuHost::new(&device, wgpu::TextureFormat::Bgra8UnormSrgb);
    /// host.register_texture(&device, 7, &texture, SourceAlpha::Premultiplied);
    /// assert!(host.is_registered(7));
    /// ```
    pub fn register_texture(
        &mut self,
        device: &wgpu::Device,
        surface_id: u64,
        texture: &wgpu::Texture,
        alpha: SourceAlpha,
    ) {
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("external-texture-bind-group"),
            layout: &self.texture_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        self.entries.insert(
            surface_id,
            ExternalEntry {
                bind_group,
                alpha,
                size: (texture.width(), texture.height()),
            },
        );
    }

    /// Drops the surface registration.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_wgpu::external::{WgpuHost, SourceAlpha};
    /// # let device: wgpu::Device = todo!();
    /// # let texture: wgpu::Texture = todo!();
    /// let mut host = WgpuHost::new(&device, wgpu::TextureFormat::Bgra8UnormSrgb);
    /// host.register_texture(&device, 7, &texture, SourceAlpha::Premultiplied);
    /// host.unregister(7);
    /// assert!(!host.is_registered(7));
    /// ```
    pub fn unregister(&mut self, surface_id: u64) {
        self.entries.remove(&surface_id);
    }

    /// Whether `surface_id` has a registered texture.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_wgpu::external::WgpuHost;
    /// # let device: wgpu::Device = todo!();
    /// let host = WgpuHost::new(&device, wgpu::TextureFormat::Bgra8UnormSrgb);
    /// assert!(!host.is_registered(0));
    /// ```
    #[must_use]
    pub fn is_registered(&self, surface_id: u64) -> bool {
        self.entries.contains_key(&surface_id)
    }

    /// Number of registered surfaces.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_wgpu::external::WgpuHost;
    /// # let device: wgpu::Device = todo!();
    /// let host = WgpuHost::new(&device, wgpu::TextureFormat::Bgra8UnormSrgb);
    /// assert_eq!(host.surface_count(), 0);
    /// ```
    #[must_use]
    pub fn surface_count(&self) -> usize {
        self.entries.len()
    }

    /// The registered texture's size for `surface_id`, if registered.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_wgpu::external::WgpuHost;
    /// # let device: wgpu::Device = todo!();
    /// let host = WgpuHost::new(&device, wgpu::TextureFormat::Bgra8UnormSrgb);
    /// assert_eq!(host.surface_size(0), None);
    /// ```
    pub fn surface_size(&self, surface_id: u64) -> Option<(u32, u32)> {
        self.entries.get(&surface_id).map(|e| e.size)
    }

    /// Records a composite draw into `target`'s encoder.
    ///
    /// Shared body of [`composite`](WgpuHost::composite) and
    /// [`composite_view`](WgpuHost::composite_view): uploads the rect,
    /// opens a `LoadOp::Load` pass, scissors to `clip`, and draws.
    fn record_composite(
        &self,
        target: CompositeTarget<'_>,
        bind_group: &wgpu::BindGroup,
        pipeline: &wgpu::RenderPipeline,
        rect: [f32; 4],
        clip: [f32; 4],
    ) -> Result<(), ExternalError> {
        let offset = self.write_rect(target.queue, rect, target.size)?;

        let mut pass = target
            .encoder
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("external-composite-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target.view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

        // Scissor to the clip rect (physical pixels, clamped to target).
        let cx = clip[0].max(0.0) as u32;
        let cy = clip[1].max(0.0) as u32;
        let cw = (clip[2].max(0.0) as u32).min(target.size.0.saturating_sub(cx));
        let ch = (clip[3].max(0.0) as u32).min(target.size.1.saturating_sub(cy));
        if cw == 0 || ch == 0 {
            return Ok(());
        }
        pass.set_scissor_rect(cx, cy, cw, ch);
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, bind_group, &[]);
        pass.set_bind_group(1, &self.rect_bind_group, &[offset]);
        pass.draw(0..6, 0..1);
        Ok(())
    }

    /// Draws `surface_id`'s registered texture into `target` at `rect`,
    /// clipped to `clip` (all in physical pixels).
    ///
    /// Records a render pass on `target.encoder` with `LoadOp::Load`
    /// so it composites *over* whatever is already in the target — the
    /// Vello segment or external content below this marker. The caller
    /// submits the encoder after all composites are recorded.
    ///
    /// # Errors
    ///
    /// [`ExternalError::UnknownSurface`] if `surface_id` is not
    /// registered, or [`ExternalError::RectCapacityExceeded`] if more
    /// than [`WgpuHost::rect_capacity`] composites are recorded in one
    /// frame.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_wgpu::external::WgpuHost;
    /// # let (device, queue): (wgpu::Device, wgpu::Queue) = todo!();
    /// # let view: wgpu::TextureView = todo!();
    /// let mut host = WgpuHost::new(&device, wgpu::TextureFormat::Bgra8UnormSrgb);
    /// let mut encoder = device.create_command_encoder(&Default::default());
    /// // `host.composite(...)` is called by the orchestrator between
    /// // segment blits.
    /// ```
    pub fn composite(
        &self,
        target: CompositeTarget<'_>,
        surface_id: u64,
        rect: [f32; 4],
        clip: [f32; 4],
    ) -> Result<(), ExternalError> {
        let entry = self
            .entries
            .get(&surface_id)
            .ok_or(ExternalError::UnknownSurface(surface_id))?;
        self.record_composite(
            target,
            &entry.bind_group,
            self.external_pipeline(entry.alpha),
            rect,
            clip,
        )
    }

    /// Draws an arbitrary `view` into `target` — the Vello segment blit.
    ///
    /// Unlike [`composite`](WgpuHost::composite) the texture is not
    /// registered: a texture bind group is built for this call (the
    /// small CPU-side cost of compositing a per-frame offscreen
    /// segment). Vello stores *straight-alpha* pixels (its fine shader
    /// unpremultiplies before `textureStore`), so the blit uses the
    /// `fs_straight` pipeline which premultiplies in-shader; the
    /// caller picks the sample view's sRGB-ness to match the target.
    ///
    /// # Errors
    ///
    /// [`ExternalError::RectCapacityExceeded`] past the per-frame
    /// composite limit.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_wgpu::external::WgpuHost;
    /// # let device: wgpu::Device = todo!();
    /// let host = WgpuHost::new(&device, wgpu::TextureFormat::Bgra8UnormSrgb);
    /// // Called by the orchestrator for each Vello segment texture.
    /// ```
    pub fn composite_view(
        &self,
        device: &wgpu::Device,
        target: CompositeTarget<'_>,
        view: &wgpu::TextureView,
        rect: [f32; 4],
        clip: [f32; 4],
    ) -> Result<(), ExternalError> {
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("segment-blit-bind-group"),
            layout: &self.texture_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        // Vello's fine shader stores *unpremultiplied* (straight-alpha)
        // sRGB-encoded pixels, so the blit must premultiply in-shader.
        // On sRGB targets the sample view decodes first and the blend
        // unit re-encodes; on unorm targets the multiply happens in the
        // encoded space — matching Vello's own blending convention.
        self.record_composite(
            target,
            &bind_group,
            &self.pipeline_straight_passthrough,
            rect,
            clip,
        )
    }
}

#[cfg(test)]
mod tests {
    /// Tests requiring `wgpu`'s `noop` backend — `wgpu::Device::noop`
    /// only exists when the `test-noop` feature is enabled.
    #[cfg(feature = "test-noop")]
    mod noop {
        use super::super::*;

        fn noop_device() -> (wgpu::Device, wgpu::Queue) {
            wgpu::Device::noop(&wgpu::DeviceDescriptor::default())
        }

        fn make_texture(device: &wgpu::Device, label: &str, size: u32) -> wgpu::Texture {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: size,
                    height: size,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
        }

        fn make_target(device: &wgpu::Device) -> (wgpu::Texture, wgpu::TextureView) {
            let tex = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("target"),
                size: wgpu::Extent3d {
                    width: 64,
                    height: 64,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Bgra8UnormSrgb,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
            (tex, view)
        }

        #[test]
        fn host_new_builds_pipelines() {
            let (device, _queue) = noop_device();
            let host = WgpuHost::new(&device, wgpu::TextureFormat::Bgra8UnormSrgb);
            assert_eq!(host.surface_count(), 0);
            assert!(host.target_is_srgb());
            assert_eq!(host.rect_capacity(), WgpuHost::RECT_SLOTS);
        }

        #[test]
        fn non_srgb_target_detected() {
            let (device, _queue) = noop_device();
            let host = WgpuHost::new(&device, wgpu::TextureFormat::Bgra8Unorm);
            assert!(!host.target_is_srgb());
        }

        #[test]
        fn register_and_unregister_texture() {
            let (device, _queue) = noop_device();
            let mut host = WgpuHost::new(&device, wgpu::TextureFormat::Bgra8UnormSrgb);
            let texture = make_texture(&device, "test-source", 64);
            host.register_texture(&device, 1, &texture, SourceAlpha::Premultiplied);
            assert!(host.is_registered(1));
            assert_eq!(host.surface_size(1), Some((64, 64)));
            host.unregister(1);
            assert!(!host.is_registered(1));
        }

        #[test]
        fn composite_unknown_surface_errors() {
            let (device, queue) = noop_device();
            let host = WgpuHost::new(&device, wgpu::TextureFormat::Bgra8UnormSrgb);
            host.begin_frame();
            let (_tex, view) = make_target(&device);
            let mut encoder =
                device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            let result = host.composite(
                CompositeTarget {
                    encoder: &mut encoder,
                    queue: &queue,
                    view: &view,
                    size: (64, 64),
                },
                99,
                [0.0, 0.0, 64.0, 64.0],
                [0.0, 0.0, 64.0, 64.0],
            );
            assert!(matches!(result, Err(ExternalError::UnknownSurface(99))));
        }

        #[test]
        fn composite_records_pass() {
            let (device, queue) = noop_device();
            let mut host = WgpuHost::new(&device, wgpu::TextureFormat::Bgra8UnormSrgb);
            host.begin_frame();
            let texture = make_texture(&device, "src", 32);
            host.register_texture(&device, 7, &texture, SourceAlpha::Straight);
            let (_tex, view) = make_target(&device);
            let mut encoder =
                device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            host.composite(
                CompositeTarget {
                    encoder: &mut encoder,
                    queue: &queue,
                    view: &view,
                    size: (64, 64),
                },
                7,
                [0.0, 0.0, 64.0, 64.0],
                [0.0, 0.0, 64.0, 64.0],
            )
            .expect("composite succeeds");
            queue.submit([encoder.finish()]);
        }

        #[test]
        fn repeated_composites_use_distinct_rect_slots() {
            let (device, queue) = noop_device();
            let mut host = WgpuHost::new(&device, wgpu::TextureFormat::Bgra8UnormSrgb);
            host.begin_frame();
            let texture = make_texture(&device, "src", 32);
            host.register_texture(&device, 7, &texture, SourceAlpha::Premultiplied);
            let (_tex, view) = make_target(&device);
            let mut encoder =
                device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            // Same surface composited twice with different rects — the
            // dynamic-offset design gives each draw its own rect slot.
            for i in 0..2 {
                let x = i as f32 * 16.0;
                host.composite(
                    CompositeTarget {
                        encoder: &mut encoder,
                        queue: &queue,
                        view: &view,
                        size: (64, 64),
                    },
                    7,
                    [x, 0.0, 16.0, 16.0],
                    [0.0, 0.0, 64.0, 64.0],
                )
                .expect("composite succeeds");
            }
            queue.submit([encoder.finish()]);
        }

        #[test]
        fn composite_view_records_pass() {
            let (device, queue) = noop_device();
            let host = WgpuHost::new(&device, wgpu::TextureFormat::Bgra8UnormSrgb);
            host.begin_frame();
            let src = make_texture(&device, "segment", 64);
            let src_view = src.create_view(&wgpu::TextureViewDescriptor::default());
            let (_tex, view) = make_target(&device);
            let mut encoder =
                device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            host.composite_view(
                &device,
                CompositeTarget {
                    encoder: &mut encoder,
                    queue: &queue,
                    view: &view,
                    size: (64, 64),
                },
                &src_view,
                [0.0, 0.0, 64.0, 64.0],
                [0.0, 0.0, 64.0, 64.0],
            )
            .expect("segment blit succeeds");
            queue.submit([encoder.finish()]);
        }

        #[test]
        fn zero_clip_returns_early() {
            let (device, queue) = noop_device();
            let mut host = WgpuHost::new(&device, wgpu::TextureFormat::Bgra8UnormSrgb);
            host.begin_frame();
            let texture = make_texture(&device, "src", 8);
            host.register_texture(&device, 1, &texture, SourceAlpha::Premultiplied);
            let (_tex, view) = make_target(&device);
            let mut encoder =
                device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            host.composite(
                CompositeTarget {
                    encoder: &mut encoder,
                    queue: &queue,
                    view: &view,
                    size: (64, 64),
                },
                1,
                [0.0, 0.0, 64.0, 64.0],
                [0.0, 0.0, 0.0, 0.0],
            )
            .expect("degenerate clip returns Ok");
        }
    }
}
