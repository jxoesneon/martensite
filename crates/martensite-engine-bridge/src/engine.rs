//! The [`Engine`] producer contract.
//!
//! An `Engine` is anything that renders frames Martensite can composite:
//! a headless Bevy app, a hardware video decoder, an offscreen web
//! compositor. The host calls [`Engine::render`] once per frame (or when
//! the producer signals new content) and hands the returned
//! [`Frame`] to the composite pass.

use crate::frame::{CpuFrame, Frame, FrameToken};

/// The viewport an engine should render into for one frame.
///
/// `size` is the widget's laid-out physical-pixel bounds; `scale_factor`
/// is the surface DPI scale so producers can render at display
/// resolution.
///
/// # Examples
///
/// ```
/// use martensite_engine_bridge::Viewport;
///
/// let vp = Viewport::new(800, 600, 2.0);
/// assert_eq!(vp.size, (800, 600));
/// assert_eq!(vp.scale_factor, 2.0);
/// ```
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Viewport {
    /// Physical pixel dimensions `(width, height)` of the widget bounds.
    pub size: (u32, u32),
    /// Display scale factor (e.g. `2.0` on Retina).
    pub scale_factor: f64,
}

impl Viewport {
    /// Creates a viewport descriptor.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_engine_bridge::Viewport;
    ///
    /// let vp = Viewport::new(320, 240, 1.0);
    /// assert_eq!(vp.size, (320, 240));
    /// ```
    pub fn new(width: u32, height: u32, scale_factor: f64) -> Self {
        Self {
            size: (width, height),
            scale_factor,
        }
    }
}

/// Context handed to [`Engine::render`] for the current frame.
///
/// For the same-device path this carries the host's `wgpu::Device` and
/// `wgpu::Queue` so the producer can create textures and submit commands
/// on the shared queue — serial submission ordering then guarantees the
/// host sees the finished frame without semaphores.
pub struct EngineContext<'a> {
    /// The host's wgpu device (shared with the composite pass).
    pub device: &'a wgpu::Device,
    /// The host's wgpu queue. Producers submit here; same-queue ordering
    /// provides the synchronization for [`FrameSync::None`](crate::FrameSync::None)
    /// frames.
    pub queue: &'a wgpu::Queue,
}

/// A producer of externally-rendered frames.
///
/// # Contract
///
/// - `render` is called at most once per produced frame. It may return
///   `None` when the producer has nothing new (the host keeps the last
///   ready frame).
/// - `release` is called by the host after a token's texture has been
///   composited exactly once — the producer may then recycle the slot.
///   Implementations that manage their own ring may ignore it.
/// - `to_pixmap` is the CPU-fallback contract: return an RGBA8 raster of
///   the given frame if the producer can rasterize without the GPU.
///
/// # Examples
///
/// ```
/// use martensite_engine_bridge::{
///     Engine, EngineContext, Frame, FrameToken, Viewport,
/// };
///
/// struct Idle;
/// impl Engine for Idle {
///     fn render(&mut self, _cx: &mut EngineContext, _vp: Viewport) -> Option<Box<dyn Frame>> {
///         None
///     }
///     fn release(&mut self, _token: FrameToken) {}
/// }
/// ```
pub trait Engine: Send + Sync {
    /// Renders the next frame for `viewport`, or `None` if nothing new is
    /// available.
    fn render(&mut self, ctx: &mut EngineContext, viewport: Viewport) -> Option<Box<dyn Frame>>;

    /// Called after the host finished compositing `token`; the producer
    /// may recycle the underlying texture/slot.
    fn release(&mut self, token: FrameToken);

    /// Returns a CPU raster of `token` for the TinySkia fallback path, or
    /// `None` if the producer cannot rasterize on the CPU.
    fn to_pixmap(&self, token: FrameToken) -> Option<CpuFrame> {
        let _ = token;
        None
    }
}
