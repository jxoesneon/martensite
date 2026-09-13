//! `MartensiteViewport` — the `GodotClass` (`Node`) a Godot scene adds to
//! stream a `SubViewport`'s rendered output to a Martensite host.
//!
//! Typical setup in a scene tree:
//!
//! ```text
//! MartensiteViewport          (this node)
//! SubViewport                 (renders the 3D/2D content)
//! ```
//!
//! then, from GDScript or the exported properties:
//!
//! ```gdscript
//! $MartensiteViewport.transport_addr = "tcp://127.0.0.1:9177"
//! $MartensiteViewport.set_subviewport("../SubViewport")
//! ```
//!
//! Every `process` frame with capacity (at most `max_in_flight`
//! outstanding reads) the node issues one
//! `RenderingDevice::texture_get_data_async` against the RD texture that
//! backs the `SubViewport`'s `ViewportTexture`; the callback ships a
//! [`FrameMsg`][crate::transport::FrameMsg] on the configured transport.
//!
//! # Y-flip
//!
//! Godot renders viewports with a top-left origin. The host composites
//! the same way, so `flip_y` defaults to `false` — set it `true` when
//! the receiving side expects a bottom-left-origin image.

use godot::builtin::{GString, NodePath, Rid, VarDictionary, vdict};
use godot::classes::{INode, Node, RenderingDevice, RenderingServer, SubViewport};
use godot::global::godot_warn;
use godot::prelude::*;

use crate::readback::Readback;
#[cfg(unix)]
use crate::transport::UnixSocketTransport;
use crate::transport::{ChannelTransport, FrameTransport, TcpTransport};

/// A Godot `Node` that streams a `SubViewport` to a Martensite host.
///
/// Exported properties:
///
/// - `subviewport_path` — `NodePath` to the `SubViewport` to stream.
/// - `transport_addr` — where frames go: `tcp://host:port`,
///   `unix:///path/to.sock` (Unix only), or `channel://name`
///   (in-process `libgodot` embedding; the host retrieves the receiver
///   via [`take_channel_receiver`][crate::transport::take_channel_receiver]).
/// - `auto_readback` — pull a frame every `_process` when `true`.
/// - `max_in_flight` — bound on outstanding async reads (default 2).
/// - `flip_y` — vertically flip shipped frames.
#[derive(GodotClass)]
#[class(init, base = Node)]
pub struct MartensiteViewport {
    #[base]
    base: Base<Node>,

    /// Node path to the `SubViewport` whose texture is streamed.
    #[export]
    subviewport_path: NodePath,

    /// Transport address (`tcp://`, `unix://`, `channel://`). Empty =
    /// frames are counted as dropped.
    #[export]
    transport_addr: GString,

    /// Pull a frame every `_process` callback.
    #[init(val = true)]
    #[export]
    auto_readback: bool,

    /// Maximum outstanding `texture_get_data_async` requests.
    #[init(val = 2)]
    #[export]
    max_in_flight: i64,

    /// Vertically flip shipped frames (see module docs).
    #[export]
    flip_y: bool,

    /// The `ViewportTexture` RID whose RD texture is read back —
    /// resolved on `set_subviewport`; mapped through
    /// `texture_get_rd_texture` per request so a fresh backing RID after
    /// a resize is picked up automatically.
    viewport_texture: Option<Rid>,

    /// The resolved `SubViewport`, kept to read its live size.
    subviewport: Option<Gd<SubViewport>>,

    /// Cached global `RenderingDevice` (absent under Compatibility/GL).
    rendering_device: Option<Gd<RenderingDevice>>,

    /// Readback bookkeeping + transport.
    readback: Readback,
}

#[godot_api]
impl MartensiteViewport {
    /// Points this node at the `SubViewport` to stream. Resolves the
    /// node, grabs its `ViewportTexture` RID, and caches the global
    /// `RenderingDevice`. Safe to call again after a scene re-parent.
    #[func]
    pub fn set_subviewport(&mut self, path: GString) {
        let path_string = path.to_string();
        let node_path = NodePath::from(path_string.as_str());
        let Some(viewport) = self.base().try_get_node_as::<SubViewport>(&node_path) else {
            godot_warn!("MartensiteViewport: no SubViewport at '{path_string}'");
            return;
        };
        let Some(texture) = viewport.get_texture() else {
            godot_warn!("MartensiteViewport: SubViewport at '{path_string}' has no texture");
            return;
        };
        self.viewport_texture = Some(texture.get_rid());
        self.subviewport = Some(viewport);
        self.subviewport_path = node_path;
        self.rendering_device = RenderingServer::singleton().get_rendering_device();
        if self.rendering_device.is_none() {
            godot_warn!(
                "MartensiteViewport: no RenderingDevice (Compatibility renderer or headless) — \
                 readback is unavailable"
            );
        }
    }

    /// Connects the transport named by `transport_addr`. Accepted
    /// schemes: `tcp://host:port`, `unix:///path`, `channel://name`.
    /// An empty or unrecognized address leaves the transport unset.
    #[func]
    pub fn connect_transport(&mut self) {
        let addr = self.transport_addr.to_string();
        let transport: Option<Box<dyn FrameTransport>> =
            if let Some(rest) = addr.strip_prefix("tcp://") {
                match TcpTransport::connect(rest) {
                    Ok(t) => Some(Box::new(t)),
                    Err(e) => {
                        godot_warn!("MartensiteViewport: TCP connect to '{rest}' failed: {e}");
                        None
                    }
                }
            } else if let Some(name) = addr.strip_prefix("channel://") {
                Some(Box::new(ChannelTransport::new_named(name)))
            } else if let Some(path) = addr.strip_prefix("unix://") {
                self.connect_unix(path)
            } else {
                if !addr.is_empty() {
                    godot_warn!("MartensiteViewport: unrecognized transport address '{addr}'");
                }
                None
            };
        self.readback.set_transport(transport);
    }

    /// Unix-socket arm of [`Self::connect_transport`] — split out so the
    /// `cfg(unix)` gate stays tidy.
    #[cfg(unix)]
    fn connect_unix(&self, path: &str) -> Option<Box<dyn FrameTransport>> {
        match UnixSocketTransport::connect(path) {
            Ok(t) => Some(Box::new(t)),
            Err(e) => {
                godot_warn!("MartensiteViewport: Unix connect to '{path}' failed: {e}");
                None
            }
        }
    }

    /// Non-Unix stub: reports the unsupported scheme and yields no
    /// transport.
    #[cfg(not(unix))]
    fn connect_unix(&self, path: &str) -> Option<Box<dyn FrameTransport>> {
        godot_warn!(
            "MartensiteViewport: 'unix://{path}' not supported on this OS — use tcp:// or channel://"
        );
        None
    }

    /// Issues one readback immediately (subject to the in-flight bound),
    /// independent of `auto_readback`.
    #[func]
    pub fn request_readback(&mut self) {
        self.issue_readback();
    }

    /// Connects `request_readback` to the `RenderingServer`'s
    /// `frame_post_draw` signal — an alternative pump to `auto_readback`
    /// that requests a readback right after each frame is drawn rather
    /// than every idle-process tick.
    #[func]
    pub fn connect_post_draw(&mut self) {
        let mut rs = RenderingServer::singleton();
        let callable = self.to_gd().callable("request_readback");
        rs.connect("frame_post_draw", &callable);
    }

    /// Honesty instrumentation: frames sent/dropped, transport errors,
    /// outstanding reads, and mean request→callback latency.
    #[func]
    pub fn stats(&self) -> VarDictionary {
        let s = self.readback.stats();
        // Godot `int` is i64 — the u64 counters are cast at the boundary
        // (they never approach i64::MAX in practice).
        vdict! {
            "sent" => s.sent as i64,
            "dropped" => s.dropped as i64,
            "errors" => s.errors as i64,
            "in_flight" => s.in_flight as i64,
            "avg_readback_us" => s.avg_readback_us as i64,
        }
    }

    fn issue_readback(&mut self) {
        let (Some(tex), Some(viewport), Some(mut rd)) = (
            self.viewport_texture,
            self.subviewport.clone(),
            self.rendering_device.clone(),
        ) else {
            return;
        };
        let size = viewport.get_size();
        let (w, h) = (size.x.max(0) as u32, size.y.max(0) as u32);
        if w == 0
            || h == 0
            || w > crate::readback::MAX_FRAME_DIM
            || h > crate::readback::MAX_FRAME_DIM
        {
            return;
        }
        // Re-resolve the RD rid every request: Godot may reallocate the
        // viewport texture's backing image on resize.
        let rd_rid = RenderingServer::singleton().texture_get_rd_texture(tex);
        self.readback.request(&mut rd, rd_rid, w, h, self.flip_y);
    }
}

#[godot_api]
impl INode for MartensiteViewport {
    fn ready(&mut self) {
        // Keep the readback in-flight bound in sync with the exported
        // property before the first request.
        self.readback
            .set_max_in_flight(self.max_in_flight.max(1) as u32);
        self.connect_transport();
        if !self.subviewport_path.is_empty() {
            let path = self.subviewport_path.to_string();
            self.set_subviewport(GString::from(path.as_str()));
        }
    }

    fn process(&mut self, _delta: f64) {
        if self.auto_readback && self.readback.has_capacity() {
            self.issue_readback();
        }
    }
}
