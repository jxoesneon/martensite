//! Frame types exchanged between an [`Engine`](crate::Engine) producer and
//! the Martensite host.
//!
//! A [`Frame`] is the smallest unit of work the bridge understands: an
//! opaque handle to one externally-rendered image plus the metadata the
//! host needs to composite it safely (synchronization, size, alpha).

use std::sync::Arc;

/// Monotonic identifier assigned to every frame produced through a
/// [`SurfaceRing`](crate::SurfaceRing).
///
/// Tokens are unique per surface and strictly increasing, which lets the
/// host distinguish "new frame arrived" from "same frame still ready".
///
/// # Examples
///
/// ```
/// use martensite_engine_bridge::FrameToken;
///
/// let a = FrameToken(1);
/// let b = FrameToken(2);
/// assert!(b > a);
/// ```
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FrameToken(pub u64);

/// How the host must synchronize with the producer before sampling a frame.
///
/// `v0.14.0` exercises [`FrameSync::None`] end-to-end (same-device,
/// same-queue ordering). The remaining variants declare the cross-device
/// synchronization surface consumed by later milestones; they are
/// transport metadata only — the bridge never executes the waits itself.
///
/// # Examples
///
/// ```
/// use martensite_engine_bridge::FrameSync;
///
/// let sync = FrameSync::None;
/// assert!(matches!(sync, FrameSync::None));
/// ```
#[derive(Clone, Debug, PartialEq)]
pub enum FrameSync {
    /// Same `wgpu::Device` and `wgpu::Queue`: serial `Queue::submit`
    /// ordering guarantees the producer's commands complete before the
    /// host's composite pass. No explicit wait needed.
    None,
    /// A shared `ID3D12Fence` value (Windows cross-device/cross-process).
    /// Maps to `wgpu_hal::dx12::Queue::add_wait_fence`.
    FenceValue(u64),
    /// A Vulkan timeline semaphore exported as a file descriptor
    /// (Linux dma-buf path). `value` is `Some` for timeline semaphores,
    /// `None` for binary semaphores. Maps to
    /// `wgpu_hal::vulkan::Queue::add_wait_semaphore`.
    VkSemaphoreFd {
        /// The semaphore file descriptor (opaque-fd or sync-fd).
        fd: i32,
        /// Timeline wait value, or `None` for binary semaphore semantics.
        value: Option<u64>,
    },
    /// An `MTLSharedEvent` counter value (macOS/iOS). Maps to
    /// `wgpu_hal::metal::Queue::add_wait_event`.
    MetalSharedEvent(u64),
    /// A DXGI keyed-mutex acquire key (legacy Windows cross-process
    /// sharing via `IDXGIKeyedMutex::AcquireSync`).
    DxgiKeyedMutex {
        /// The keyed-mutex key the consumer must acquire.
        key: u64,
    },
}

/// Alpha-channel interpretation of a produced frame.
///
/// The host keeps two composite pipelines (one per variant) so straight
/// and premultiplied sources blend correctly into the scene.
///
/// # Examples
///
/// ```
/// use martensite_engine_bridge::SourceAlpha;
///
/// assert_eq!(SourceAlpha::default(), SourceAlpha::Premultiplied);
/// ```
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum SourceAlpha {
    /// Color channels are NOT premultiplied by alpha; the composite
    /// shader multiplies `rgb` by `a` before blending.
    Straight,
    /// Color channels are already premultiplied by alpha (the wgpu
    /// surface convention); composited as-is.
    #[default]
    Premultiplied,
}

/// A platform-native frame descriptor for cross-device/cross-process
/// import (v0.15.0+).
///
/// These variants mirror the `HardwareHandle` descriptors in
/// `martensite-media` and the `wgpu_hal` import paths
/// (`texture_from_dmabuf_fd`, `texture_from_raw`). The bridge only
/// transports them — conversion into a `wgpu::Texture` happens in the
/// consumer's allowed-unsafe platform crate.
///
/// # Examples
///
/// ```
/// use martensite_engine_bridge::NativeFrame;
///
/// let frame = NativeFrame::IoSurface { surface_id: 42 };
/// assert!(matches!(frame, NativeFrame::IoSurface { surface_id: 42 }));
/// ```
#[derive(Clone, Debug, PartialEq)]
pub enum NativeFrame {
    /// A Linux `dma-buf` file descriptor with DRM format modifier.
    DmaBuf {
        /// The dma-buf file descriptor.
        fd: i32,
        /// DRM fourcc-style format modifier.
        modifier: u64,
        /// Plane stride in bytes.
        stride: u32,
        /// Plane offset in bytes.
        offset: u32,
    },
    /// A macOS `IOSurface` global identifier.
    IoSurface {
        /// The 32-bit `IOSurfaceID`.
        surface_id: u32,
    },
    /// A Windows DXGI shared NT handle (from
    /// `IDXGIResource1::CreateSharedHandle`).
    SharedHandle {
        /// The NT handle value.
        handle: usize,
    },
}

/// A CPU-side pixel buffer used for the software-fallback path.
///
/// Producers that can rasterize on the CPU (or already have a readback)
/// return this from [`Engine::to_pixmap`](crate::Engine::to_pixmap) so the
/// TinySkia backend can composite something other than the placeholder.
///
/// # Examples
///
/// ```
/// use martensite_engine_bridge::CpuFrame;
///
/// let frame = CpuFrame::new(2, 2, vec![255u8; 16]);
/// assert_eq!(frame.len_bytes(), 16);
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CpuFrame {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Tightly packed RGBA8 pixels, row-major, `width * height * 4` bytes.
    pub pixels: Vec<u8>,
}

impl CpuFrame {
    /// Creates a new `CpuFrame`. `pixels` must be
    /// `width * height * 4` bytes.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_engine_bridge::CpuFrame;
    ///
    /// let frame = CpuFrame::new(4, 4, vec![0u8; 64]);
    /// assert_eq!(frame.width, 4);
    /// ```
    pub fn new(width: u32, height: u32, pixels: Vec<u8>) -> Self {
        debug_assert_eq!(pixels.len(), (width * height * 4) as usize);
        Self {
            width,
            height,
            pixels,
        }
    }

    /// Returns the expected byte length of [`Self::pixels`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_engine_bridge::CpuFrame;
    ///
    /// assert_eq!(CpuFrame::new(3, 2, vec![0; 24]).len_bytes(), 24);
    /// ```
    pub fn len_bytes(&self) -> usize {
        (self.width * self.height * 4) as usize
    }
}

/// One externally-produced frame offered to the host.
///
/// Implementations are cheap handles — the heavy resource (a
/// `wgpu::Texture` or platform handle) is owned by the producer's ring
/// slot and recycled when the host releases the [`FrameToken`].
///
/// # Examples
///
/// ```
/// use martensite_engine_bridge::{Frame, FrameSync, FrameToken, SourceAlpha};
///
/// struct Solid;
/// impl Frame for Solid {
///     fn token(&self) -> FrameToken { FrameToken(0) }
///     fn same_device_texture(&self) -> Option<&wgpu::Texture> { None }
///     fn native_handle(&self) -> Option<martensite_engine_bridge::NativeFrame> { None }
///     fn sync(&self) -> FrameSync { FrameSync::None }
///     fn size(&self) -> (u32, u32) { (640, 480) }
///     fn alpha_mode(&self) -> SourceAlpha { SourceAlpha::Premultiplied }
/// }
/// let f = Solid;
/// assert_eq!(f.size(), (640, 480));
/// ```
pub trait Frame: Send + Sync {
    /// The token assigned by the producing surface's ring.
    fn token(&self) -> FrameToken;

    /// A `wgpu::Texture` created on the **host's** device — the v0.14.0
    /// primary path. Returns `None` for cross-device frames.
    fn same_device_texture(&self) -> Option<&wgpu::Texture>;

    /// A platform-native handle for cross-device import. Returns `None`
    /// for same-device frames.
    fn native_handle(&self) -> Option<NativeFrame>;

    /// How the host must wait before sampling this frame.
    fn sync(&self) -> FrameSync;

    /// Frame dimensions in physical pixels `(width, height)`.
    fn size(&self) -> (u32, u32);

    /// Whether the frame's alpha is straight or premultiplied.
    fn alpha_mode(&self) -> SourceAlpha;
}

/// A `Frame` implementation wrapping an owned `wgpu::Texture`.
///
/// This is the standard same-device frame: the texture was created on the
/// host device, so it needs no import and no explicit synchronization
/// beyond same-queue ordering.
///
/// # Examples
///
/// ```no_run
/// use martensite_engine_bridge::{Frame, FrameToken, TextureFrame};
/// # let texture: wgpu::Texture = todo!();
/// let frame = TextureFrame::new(FrameToken(7), texture, (1920, 1080));
/// assert_eq!(frame.size(), (1920, 1080));
/// ```
pub struct TextureFrame {
    token: FrameToken,
    texture: wgpu::Texture,
    size: (u32, u32),
    alpha: SourceAlpha,
    sync: FrameSync,
    native: Option<NativeFrame>,
}

impl TextureFrame {
    /// Creates a `TextureFrame` for a same-device `wgpu::Texture` with
    /// premultiplied alpha and queue-ordering synchronization.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_engine_bridge::{FrameToken, TextureFrame};
    /// # let texture: wgpu::Texture = todo!();
    /// let frame = TextureFrame::new(FrameToken(1), texture, (64, 64));
    /// ```
    pub fn new(token: FrameToken, texture: wgpu::Texture, size: (u32, u32)) -> Self {
        Self {
            token,
            texture,
            size,
            alpha: SourceAlpha::Premultiplied,
            sync: FrameSync::None,
            native: None,
        }
    }

    /// Overrides the alpha interpretation.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_engine_bridge::{FrameToken, SourceAlpha, TextureFrame};
    /// # let texture: wgpu::Texture = todo!();
    /// let frame =
    ///     TextureFrame::new(FrameToken(1), texture, (64, 64)).with_alpha(SourceAlpha::Straight);
    /// ```
    #[must_use]
    pub fn with_alpha(mut self, alpha: SourceAlpha) -> Self {
        self.alpha = alpha;
        self
    }

    /// Attaches an explicit synchronization requirement.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_engine_bridge::{FrameSync, FrameToken, TextureFrame};
    /// # let texture: wgpu::Texture = todo!();
    /// let frame =
    ///     TextureFrame::new(FrameToken(1), texture, (64, 64)).with_sync(FrameSync::FenceValue(9));
    /// ```
    #[must_use]
    pub fn with_sync(mut self, sync: FrameSync) -> Self {
        self.sync = sync;
        self
    }
}

impl Frame for TextureFrame {
    fn token(&self) -> FrameToken {
        self.token
    }

    fn same_device_texture(&self) -> Option<&wgpu::Texture> {
        Some(&self.texture)
    }

    fn native_handle(&self) -> Option<NativeFrame> {
        self.native.clone()
    }

    fn sync(&self) -> FrameSync {
        self.sync.clone()
    }

    fn size(&self) -> (u32, u32) {
        self.size
    }

    fn alpha_mode(&self) -> SourceAlpha {
        self.alpha
    }
}

/// Shared, cheaply-clonable storage for a frame payload.
///
/// Producers returning boxed [`Frame`] trait objects usually embed this
/// when the same texture is re-uploaded in place every frame.
pub type SharedTexture = Arc<wgpu::Texture>;
