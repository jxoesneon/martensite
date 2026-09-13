//! The [`Engine`] producer contract.
//!
//! An `Engine` is anything that renders frames Martensite can composite:
//! a headless Bevy app, a hardware video decoder, an offscreen web
//! compositor. The engine's [`Engine::render`] implementation renders a
//! frame and **publishes it into its ring slot** via
//! [`BridgeRegistry::mark_ready_full`](crate::BridgeRegistry::mark_ready_full)
//! (or a sibling `mark_ready_*` call). The composite pass samples the
//! ring's copy; the returned [`Frame`] is the same frame as a cheap
//! handle for inspection/accounting — `None` when nothing new was
//! produced (the host keeps the last ready frame).

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

/// A pointer button carried by [`EngineEvent::PointerButton`].
///
/// The mapping follows the platform/winit convention: `Primary` is the
/// left button for right-handed users, `Secondary` the right button.
///
/// # Examples
///
/// ```
/// use martensite_engine_bridge::PointerButton;
///
/// assert_ne!(PointerButton::Primary, PointerButton::Secondary);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PointerButton {
    /// The primary pointer button (left for right-handed users).
    Primary,
    /// The secondary pointer button (right for right-handed users).
    Secondary,
    /// The middle pointer button (scroll-wheel click).
    Middle,
    /// The "back" navigation button (mouse button 4 / browser back).
    Back,
    /// The "forward" navigation button (mouse button 5 / browser forward).
    Forward,
    /// Any other platform button, identified by its raw code.
    Other(u16),
}

/// A host→engine input event, in surface-local physical pixels.
///
/// Positions are relative to the surface's origin (the `ExternalEngine`
/// widget's laid-out bounds), already converted to the physical pixels the
/// producer renders at — the host subtracts the widget origin before
/// forwarding so engines never need the window-global position.
///
/// The enum is `#[non_exhaustive]`: new event kinds may be added in minor
/// releases. Engines should ignore events they do not handle.
///
/// # Examples
///
/// ```
/// use martensite_engine_bridge::{EngineEvent, PointerButton};
///
/// let ev = EngineEvent::PointerButton {
///     position: [12.0, 34.0],
///     button: PointerButton::Primary,
///     pressed: true,
/// };
/// assert!(matches!(ev, EngineEvent::PointerButton { pressed: true, .. }));
/// ```
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum EngineEvent {
    /// Pointer moved; `position` is relative to the surface origin.
    PointerMove {
        /// Surface-local physical-pixel position.
        position: [f32; 2],
    },
    /// Pointer button press/release.
    PointerButton {
        /// Surface-local physical-pixel position.
        position: [f32; 2],
        /// Which button changed state.
        button: PointerButton,
        /// `true` on press, `false` on release.
        pressed: bool,
    },
    /// Scroll delta in surface-local space.
    Scroll {
        /// Surface-local physical-pixel position of the pointer.
        position: [f32; 2],
        /// Scroll delta `(x, y)` in lines or physical pixels, per platform.
        delta: [f32; 2],
    },
    /// Key press/release. `scancode` is the platform scancode.
    Key {
        /// Platform scancode of the key.
        scancode: u32,
        /// `true` on press, `false` on release.
        pressed: bool,
    },
    /// Composed text input (IME-resulting string).
    TextInput {
        /// The composed text delivered to the engine.
        text: String,
    },
    /// Surface focus gained/lost.
    Focus {
        /// `true` when the surface gained keyboard focus.
        focused: bool,
    },
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
/// - `render` renders one frame and **publishes it into the ring** via
///   `acquire` → render into the slot texture → `mark_ready_*`. The
///   returned `Frame` is the same frame as a cheap handle for
///   inspection; the composite pass reads the ring's copy. `None` means
///   "nothing new" — the host keeps the last ready frame.
/// - `release` is called after the host finished compositing `token` —
///   the producer may then recycle the underlying texture (e.g. move it
///   back to a free pool). Implementations that manage their own
///   lifetime may ignore it.
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
///
/// # Safety boundary
///
/// `Engine` implementations run in the host process with access to the
/// host's `wgpu::Device` — they are trusted producers. `ExternalEngines`
/// wraps each `Engine` call in `catch_unwind` and quarantines a panicking
/// engine, but under Martensite's `panic = "abort"` release profile a
/// producer panic still aborts the process. Adapters embedding untrusted
/// renderers should isolate them in a child process and transport frames
/// via [`NativeFrame`](crate::NativeFrame) handles.
pub trait Engine: Send + Sync {
    /// Renders the next frame for `viewport`, or `None` if nothing new is
    /// available.
    fn render(&mut self, ctx: &mut EngineContext, viewport: Viewport) -> Option<Box<dyn Frame>>;

    /// Called after the host finished compositing `token`; the producer
    /// may recycle the underlying texture/slot.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_engine_bridge::{
    ///     Engine, EngineContext, Frame, FrameToken, Viewport,
    /// };
    /// struct Recycled;
    /// impl Engine for Recycled {
    ///     fn render(&mut self, _c: &mut EngineContext, _v: Viewport) -> Option<Box<dyn Frame>> {
    ///         None
    ///     }
    ///     fn release(&mut self, token: FrameToken) {
    ///         assert!(token > FrameToken(0));
    ///     }
    /// }
    /// ```
    fn release(&mut self, token: FrameToken);

    /// Returns a CPU raster of `token` for the TinySkia fallback path, or
    /// `None` if the producer cannot rasterize on the CPU.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_engine_bridge::{
    ///     CpuFrame, Engine, EngineContext, Frame, FrameToken, Viewport,
    /// };
    /// struct Raster;
    /// impl Engine for Raster {
    ///     fn render(&mut self, _c: &mut EngineContext, _v: Viewport) -> Option<Box<dyn Frame>> {
    ///         None
    ///     }
    ///     fn release(&mut self, _t: FrameToken) {}
    ///     fn to_pixmap(&self, _t: FrameToken) -> Option<CpuFrame> {
    ///         Some(CpuFrame::new(1, 1, vec![255, 0, 0, 255]))
    ///     }
    /// }
    /// ```
    fn to_pixmap(&self, token: FrameToken) -> Option<CpuFrame> {
        let _ = token;
        None
    }

    /// Forwards a host input event into the engine.
    ///
    /// The host calls this when window events target the surface bound to
    /// this engine (`martensite::widgets::external::ExternalEngines::
    /// forward_event` in the `martensite` crate). Positions are
    /// surface-local physical pixels — see [`EngineEvent`].
    ///
    /// The default implementation ignores every event, so existing
    /// `Engine` implementations keep compiling unchanged.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_engine_bridge::{
    ///     Engine, EngineContext, EngineEvent, Frame, FrameToken, Viewport,
    /// };
    ///
    /// struct Keys(std::sync::Mutex<u32>);
    /// impl Engine for Keys {
    ///     fn render(&mut self, _c: &mut EngineContext, _v: Viewport) -> Option<Box<dyn Frame>> {
    ///         None
    ///     }
    ///     fn release(&mut self, _t: FrameToken) {}
    ///     fn on_event(&mut self, event: &EngineEvent) {
    ///         if let EngineEvent::Key { pressed: true, .. } = event {
    ///             *self.0.lock().unwrap() += 1;
    ///         }
    ///     }
    /// }
    /// ```
    fn on_event(&mut self, event: &EngineEvent) {
        let _ = event;
    }
}
