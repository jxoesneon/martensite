//! Cross-process frame transport between the Godot GDExtension and the
//! Martensite host.
//!
//! Godot and Martensite may live in different processes (the normal
//! GDExtension case) or in the same process (`libgodot` embedding). This
//! module defines a single framed message format ([`FrameMsg`]) and three
//! transports behind the [`FrameTransport`] trait:
//!
//! - [`TcpTransport`] — loopback TCP; the portable cross-process default
//!   (works on every OS, including Windows where Unix sockets are not
//!   available from `std`).
//! - [`UnixSocketTransport`] — Unix domain sockets; lower overhead on
//!   Unix-likes, `cfg(unix)` only.
//! - [`ChannelTransport`] — an in-process `std::sync::mpsc` channel for
//!   `libgodot`-style embedding where both sides share an address space.
//!
//! # Honesty note
//!
//! Every transport here moves a CPU-side `Vec<u8>`: by the time a
//! [`FrameMsg`] exists, the Godot frame has already been read back from
//! the GPU (`texture_get_data_async`), so the transport adds one more
//! CPU copy into the socket/channel. Nothing in this file is — or can
//! be — a shared-GPU-memory path.
//!
//! # Wire format
//!
//! Length-prefixed little-endian frames:
//!
//! ```text
//! [u64 magic][u32 width][u32 height][u32 format][u64 seq][u64 payload_len][payload]
//! ```
//!
//! Receivers validate every field before allocating: dimensions are
//! capped at [`MAX_FRAME_DIM`] and `payload_len` must equal
//! `width * height * 4` for the only defined format.

use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::sync::mpsc;

#[cfg(unix)]
use std::os::unix::net::{UnixListener, UnixStream};

use martensite_engine_bridge::MAX_FRAME_DIM;

/// Magic prefix identifying a [`FrameMsg`] on the wire (`"MTSGFRME"`).
pub const FRAME_MAGIC: u64 = 0x4D54_5347_4652_4D45;

/// Wire format tag for tightly packed RGBA8 (row-major, top-left origin).
pub const FORMAT_RGBA8_UNORM: u32 = 0;

/// Wire header size in bytes: magic + w + h + format + seq + payload_len.
pub const HEADER_LEN: usize = 8 + 4 + 4 + 4 + 8 + 8;

/// Largest legal payload: `MAX_FRAME_DIM²` pixels × 4 bytes.
pub const MAX_PAYLOAD_BYTES: u64 = MAX_FRAME_DIM as u64 * MAX_FRAME_DIM as u64 * 4;

/// One frame of Godot viewport pixels on its way to the host.
///
/// `pixels` is tightly packed RGBA8 (`width * height * 4` bytes),
/// row-major. Row order follows the producer's `flip_y` setting: when
/// `flip_y` was enabled on the Godot side, row 0 is already the image's
/// bottom row — the host always treats row 0 as top.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameMsg {
    /// Frame width in pixels (1..=16384).
    pub width: u32,
    /// Frame height in pixels (1..=16384).
    pub height: u32,
    /// Pixel format tag; currently only [`FORMAT_RGBA8_UNORM`].
    pub format: u32,
    /// Producer-side monotonically increasing sequence number.
    pub seq: u64,
    /// `width * height * 4` tightly packed RGBA8 pixels.
    pub pixels: Vec<u8>,
}

impl FrameMsg {
    /// Creates a validated `FrameMsg` with `format` set to
    /// [`FORMAT_RGBA8_UNORM`].
    ///
    /// # Errors
    ///
    /// `io::ErrorKind::InvalidInput` if a dimension is zero or exceeds
    /// [`MAX_FRAME_DIM`], or `pixels.len() != width * height * 4`.
    pub fn rgba8(width: u32, height: u32, seq: u64, pixels: Vec<u8>) -> io::Result<Self> {
        let msg = Self {
            width,
            height,
            format: FORMAT_RGBA8_UNORM,
            seq,
            pixels,
        };
        msg.validate()?;
        Ok(msg)
    }

    /// Checks dimension and payload invariants.
    ///
    /// # Errors
    ///
    /// `io::ErrorKind::InvalidInput` on any violation.
    pub fn validate(&self) -> io::Result<()> {
        if self.width == 0
            || self.height == 0
            || self.width > MAX_FRAME_DIM
            || self.height > MAX_FRAME_DIM
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "frame dimensions {}x{} exceed bounds 1..={MAX_FRAME_DIM}",
                    self.width, self.height
                ),
            ));
        }
        if self.format != FORMAT_RGBA8_UNORM {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("unknown frame format {}", self.format),
            ));
        }
        let expected = self.width as usize * self.height as usize * 4;
        if self.pixels.len() != expected {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "payload length {} != {} ({}x{}x4)",
                    self.pixels.len(),
                    expected,
                    self.width,
                    self.height
                ),
            ));
        }
        Ok(())
    }

    /// Serializes the header + payload into one contiguous buffer.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(HEADER_LEN + self.pixels.len());
        self.write_header(&mut out);
        out.extend_from_slice(&self.pixels);
        out
    }

    fn write_header(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&FRAME_MAGIC.to_le_bytes());
        out.extend_from_slice(&self.width.to_le_bytes());
        out.extend_from_slice(&self.height.to_le_bytes());
        out.extend_from_slice(&self.format.to_le_bytes());
        out.extend_from_slice(&self.seq.to_le_bytes());
        out.extend_from_slice(&(self.pixels.len() as u64).to_le_bytes());
    }

    /// Writes one framed message to `w`.
    ///
    /// # Errors
    ///
    /// Propagates `w`'s `io::Error`s; `InvalidInput` if the message fails
    /// validation (defense in depth — never produce a bad wire frame).
    pub fn write_to(&self, w: &mut impl Write) -> io::Result<()> {
        self.validate()?;
        w.write_all(&self.encode())?;
        w.flush()
    }

    /// Reads one framed message from `r`.
    ///
    /// Returns `Ok(None)` on a clean end-of-stream *before* the magic —
    /// i.e. the producer went away between frames. A mid-frame EOF is an
    /// error.
    ///
    /// # Errors
    ///
    /// `io::ErrorKind::InvalidData` on a bad magic or a header that fails
    /// validation; `UnexpectedEof` on a truncated payload; propagates
    /// `r`'s own errors.
    pub fn read_from(r: &mut impl Read) -> io::Result<Option<Self>> {
        let mut header = [0u8; HEADER_LEN];
        match read_exact_or_eof(r, &mut header) {
            Ok(true) => return Ok(None),
            Ok(false) => {}
            Err(e) => return Err(e),
        }
        let magic = u64::from_le_bytes(header[0..8].try_into().expect("8-byte slice"));
        if magic != FRAME_MAGIC {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("bad frame magic {magic:#018x}"),
            ));
        }
        let width = u32::from_le_bytes(header[8..12].try_into().expect("4-byte slice"));
        let height = u32::from_le_bytes(header[12..16].try_into().expect("4-byte slice"));
        let format = u32::from_le_bytes(header[16..20].try_into().expect("4-byte slice"));
        let seq = u64::from_le_bytes(header[20..28].try_into().expect("8-byte slice"));
        let payload_len = u64::from_le_bytes(header[28..36].try_into().expect("8-byte slice"));

        // Validate BEFORE allocating: a corrupt/malicious length must not
        // trigger a giant allocation.
        if width == 0 || height == 0 || width > MAX_FRAME_DIM || height > MAX_FRAME_DIM {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("frame dimensions {width}x{height} out of bounds"),
            ));
        }
        if format != FORMAT_RGBA8_UNORM {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unknown frame format {format}"),
            ));
        }
        let expected = width as u64 * height as u64 * 4;
        if payload_len != expected || payload_len > MAX_PAYLOAD_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("payload length {payload_len} != {expected}"),
            ));
        }
        let mut pixels = vec![0u8; payload_len as usize];
        r.read_exact(&mut pixels)?;
        Ok(Some(Self {
            width,
            height,
            format,
            seq,
            pixels,
        }))
    }
}

/// Reads `buf` fully; `Ok(true)` means "EOF before the first byte" (a
/// clean stream end), `Ok(false)` means the buffer was filled.
fn read_exact_or_eof(r: &mut impl Read, buf: &mut [u8]) -> io::Result<bool> {
    let mut filled = 0;
    while filled < buf.len() {
        match r.read(&mut buf[filled..]) {
            Ok(0) => {
                return if filled == 0 {
                    Ok(true)
                } else {
                    Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "truncated frame header",
                    ))
                };
            }
            Ok(n) => filled += n,
            Err(ref e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(false)
}

/// The Godot→host send side. Implementations must be `Send` so the
/// extension can keep the transport behind whichever synchronization
/// primitive the callback context allows.
pub trait FrameTransport: Send {
    /// Sends one frame. Errors are counted by the caller's stats; a
    /// broken channel is not retried — the next frame simply tries again.
    fn send(&mut self, msg: &FrameMsg) -> io::Result<()>;
}

/// In-process transport over `std::sync::mpsc` — used when Godot is
/// embedded via `libgodot` and the host app links this crate's `rlib`.
///
/// The receiver half is handed to the host through
/// [`take_channel_receiver`] (keyed by name), because the `mpsc`
/// receiver cannot itself cross the GDExtension API boundary.
pub struct ChannelTransport {
    tx: mpsc::Sender<FrameMsg>,
}

impl ChannelTransport {
    /// Creates a channel transport and registers its receiver under
    /// `name` in the process-global registry. The host retrieves it with
    /// [`take_channel_receiver`].
    ///
    /// Registering the same `name` twice replaces the stale receiver —
    /// the old sender's `send` then fails with `BrokenPipe` on the next
    /// frame, which the readback layer counts and moves on from.
    pub fn new_named(name: &str) -> Self {
        let (tx, rx) = mpsc::channel();
        channel_registry()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(name.to_string(), rx);
        Self { tx }
    }
}

impl FrameTransport for ChannelTransport {
    fn send(&mut self, msg: &FrameMsg) -> io::Result<()> {
        self.tx.send(msg.clone()).map_err(|_| {
            io::Error::new(io::ErrorKind::BrokenPipe, "frame channel receiver dropped")
        })
    }
}

/// Retrieves the receiving end of a [`ChannelTransport`] previously
/// registered with [`ChannelTransport::new_named`]. `None` if no
/// transport registered under `name` (yet) — hosts should poll after
/// loading the extension.
pub fn take_channel_receiver(name: &str) -> Option<mpsc::Receiver<FrameMsg>> {
    channel_registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(name)
}

fn channel_registry()
-> &'static std::sync::Mutex<std::collections::HashMap<String, mpsc::Receiver<FrameMsg>>> {
    static REGISTRY: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<String, mpsc::Receiver<FrameMsg>>>,
    > = std::sync::OnceLock::new();
    REGISTRY.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

/// Loopback-TCP transport — the portable cross-process default.
///
/// The extension connects out to the host's listener; reconnects are the
/// caller's concern (a `send` error means the connection is dead and a
/// new `TcpTransport` must be built).
pub struct TcpTransport {
    stream: TcpStream,
}

impl TcpTransport {
    /// Connects to a host listening on `addr` (anything
    /// [`ToSocketAddrs`] accepts, e.g. `"127.0.0.1:9177"`).
    ///
    /// # Errors
    ///
    /// Propagates `TcpStream::connect` errors.
    pub fn connect(addr: impl ToSocketAddrs) -> io::Result<Self> {
        let stream = TcpStream::connect(addr)?;
        // Latency over throughput: a frame is more useful fresh than
        // batched with the next one.
        stream.set_nodelay(true)?;
        Ok(Self { stream })
    }
}

impl FrameTransport for TcpTransport {
    fn send(&mut self, msg: &FrameMsg) -> io::Result<()> {
        msg.write_to(&mut self.stream)
    }
}

/// Unix-domain-socket transport — lower-overhead cross-process option on
/// Unix platforms.
#[cfg(unix)]
pub struct UnixSocketTransport {
    stream: UnixStream,
}

#[cfg(unix)]
impl UnixSocketTransport {
    /// Connects to a host listening on the socket at `path`.
    ///
    /// # Errors
    ///
    /// Propagates `UnixStream::connect` errors.
    pub fn connect(path: impl AsRef<std::path::Path>) -> io::Result<Self> {
        Ok(Self {
            stream: UnixStream::connect(path)?,
        })
    }
}

#[cfg(unix)]
impl FrameTransport for UnixSocketTransport {
    fn send(&mut self, msg: &FrameMsg) -> io::Result<()> {
        msg.write_to(&mut self.stream)
    }
}

/// The receiving counterpart of a [`FrameTransport`], used by the host
/// side (`host.rs`). Each variant drains framed messages; socket
/// variants keep the accepted producer connection across `recv` calls,
/// accept one producer at a time, and re-accept after a disconnect or a
/// corrupt stream so a Godot editor restart does not kill the host
/// pipeline.
pub enum FrameSource {
    /// In-process channel receiver from [`take_channel_receiver`].
    Channel(mpsc::Receiver<FrameMsg>),
    /// A bound TCP listener plus the currently accepted connection.
    Tcp {
        /// The listening socket.
        listener: TcpListener,
        /// The live producer connection, if any.
        conn: Option<TcpStream>,
    },
    /// A bound Unix-socket listener plus the currently accepted
    /// connection.
    #[cfg(unix)]
    Unix {
        /// The listening socket.
        listener: UnixListener,
        /// The live producer connection, if any.
        conn: Option<UnixStream>,
    },
}

impl FrameSource {
    /// Binds a loopback TCP listener (e.g. `"127.0.0.1:0"` — the actual
    /// port is retrievable via [`FrameSource::local_addr`]).
    ///
    /// # Errors
    ///
    /// Propagates `TcpListener::bind` errors.
    pub fn bind_tcp(addr: impl ToSocketAddrs) -> io::Result<Self> {
        Ok(Self::Tcp {
            listener: TcpListener::bind(addr)?,
            conn: None,
        })
    }

    /// Binds a Unix-socket listener at `path`, removing a stale socket
    /// file first (a crashed producer leaves it behind).
    ///
    /// # Errors
    ///
    /// Propagates filesystem and `UnixListener::bind` errors.
    #[cfg(unix)]
    pub fn bind_unix(path: impl AsRef<std::path::Path>) -> io::Result<Self> {
        let path = path.as_ref();
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        Ok(Self::Unix {
            listener: UnixListener::bind(path)?,
            conn: None,
        })
    }

    /// Wraps an already-obtained channel receiver.
    pub fn channel(rx: mpsc::Receiver<FrameMsg>) -> Self {
        Self::Channel(rx)
    }

    /// The bound TCP port, when this is a [`FrameSource::Tcp`].
    pub fn local_addr(&self) -> Option<io::Result<std::net::SocketAddr>> {
        match self {
            Self::Tcp { listener, .. } => Some(listener.local_addr()),
            _ => None,
        }
    }

    /// Blocks until the next frame arrives.
    ///
    /// Socket variants read from the current producer connection until
    /// it ends, then accept the next one — the loop only ends on a
    /// listener `accept` failure. A corrupt frame stream is treated as a
    /// dead producer: the connection is dropped and a fresh one
    /// re-accepted (byte alignment can never be trusted after garbage,
    /// so a *new* connection is the only safe recovery). The channel
    /// variant ends when all senders are dropped.
    ///
    /// # Errors
    ///
    /// `io::Error` on `accept` failure or a closed channel.
    pub fn recv(&mut self) -> io::Result<FrameMsg> {
        match self {
            Self::Channel(rx) => rx
                .recv()
                .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "channel closed")),
            Self::Tcp { listener, conn } => loop {
                if conn.is_none() {
                    let (stream, _peer) = listener.accept()?;
                    stream.set_nodelay(true).ok();
                    *conn = Some(stream);
                }
                let stream = conn.as_mut().expect("connection just accepted");
                match FrameMsg::read_from(stream) {
                    Ok(Some(msg)) => return Ok(msg),
                    // Clean EOF or unrecoverable garbage: drop the
                    // connection and wait for a fresh producer.
                    Ok(None) | Err(_) => *conn = None,
                }
            },
            #[cfg(unix)]
            Self::Unix { listener, conn } => loop {
                if conn.is_none() {
                    let (stream, _peer) = listener.accept()?;
                    *conn = Some(stream);
                }
                let stream = conn.as_mut().expect("connection just accepted");
                match FrameMsg::read_from(stream) {
                    Ok(Some(msg)) => return Ok(msg),
                    Ok(None) | Err(_) => *conn = None,
                }
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> FrameMsg {
        FrameMsg::rgba8(4, 2, 7, (0u8..32).collect()).unwrap()
    }

    #[test]
    fn encode_decode_round_trip() {
        let msg = sample();
        let bytes = msg.encode();
        let mut cursor = io::Cursor::new(bytes);
        let back = FrameMsg::read_from(&mut cursor).unwrap().unwrap();
        assert_eq!(back, msg);
    }

    #[test]
    fn read_returns_none_on_clean_eof() {
        let mut cursor = io::Cursor::new(Vec::<u8>::new());
        assert!(FrameMsg::read_from(&mut cursor).unwrap().is_none());
    }

    #[test]
    fn rejects_bad_magic() {
        let mut bytes = sample().encode();
        bytes[0] ^= 0xff;
        let mut cursor = io::Cursor::new(bytes);
        assert_eq!(
            FrameMsg::read_from(&mut cursor).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn rejects_oversized_dimensions_before_allocating() {
        let mut header = Vec::new();
        header.extend_from_slice(&FRAME_MAGIC.to_le_bytes());
        header.extend_from_slice(&(MAX_FRAME_DIM + 1).to_le_bytes());
        header.extend_from_slice(&1u32.to_le_bytes());
        header.extend_from_slice(&FORMAT_RGBA8_UNORM.to_le_bytes());
        header.extend_from_slice(&0u64.to_le_bytes());
        header.extend_from_slice(&u64::MAX.to_le_bytes()); // hostile length
        let mut cursor = io::Cursor::new(header);
        assert!(FrameMsg::read_from(&mut cursor).is_err());
    }

    #[test]
    fn rejects_length_mismatch() {
        assert!(FrameMsg::rgba8(4, 4, 0, vec![0u8; 10]).is_err());
        assert!(FrameMsg::rgba8(0, 4, 0, vec![]).is_err());
    }

    #[test]
    fn truncated_payload_is_error() {
        let bytes = sample().encode();
        let mut cursor = io::Cursor::new(bytes[..bytes.len() - 3].to_vec());
        assert_eq!(
            FrameMsg::read_from(&mut cursor).unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof
        );
    }

    #[test]
    fn channel_transport_round_trip() {
        let mut tx = ChannelTransport::new_named("test-channel-rt");
        let rx = take_channel_receiver("test-channel-rt").unwrap();
        tx.send(&sample()).unwrap();
        let msg = rx.recv().unwrap();
        assert_eq!(msg, sample());
    }

    #[test]
    fn channel_transport_reports_closed_receiver() {
        let mut tx = ChannelTransport::new_named("test-channel-closed");
        drop(take_channel_receiver("test-channel-closed"));
        assert_eq!(
            tx.send(&sample()).unwrap_err().kind(),
            io::ErrorKind::BrokenPipe
        );
    }

    #[test]
    fn tcp_transport_round_trip() {
        let mut source = FrameSource::bind_tcp("127.0.0.1:0").unwrap();
        let port = source.local_addr().unwrap().unwrap().port();
        let mut tx = TcpTransport::connect(("127.0.0.1", port)).unwrap();
        tx.send(&sample()).unwrap();
        let msg = source.recv().unwrap();
        assert_eq!(msg, sample());
    }

    #[test]
    fn tcp_multiple_frames_one_connection() {
        // Regression: every frame on a live connection must be
        // delivered — `recv` must not re-accept per frame.
        let mut source = FrameSource::bind_tcp("127.0.0.1:0").unwrap();
        let port = source.local_addr().unwrap().unwrap().port();
        let mut tx = TcpTransport::connect(("127.0.0.1", port)).unwrap();
        tx.send(&sample()).unwrap();
        tx.send(&FrameMsg::rgba8(2, 2, 8, vec![3u8; 16]).unwrap())
            .unwrap();
        assert_eq!(source.recv().unwrap(), sample());
        assert_eq!(source.recv().unwrap().seq, 8);
    }

    #[test]
    fn tcp_reconnect_after_disconnect() {
        let mut source = FrameSource::bind_tcp("127.0.0.1:0").unwrap();
        let port = source.local_addr().unwrap().unwrap().port();
        {
            let mut tx = TcpTransport::connect(("127.0.0.1", port)).unwrap();
            tx.send(&sample()).unwrap();
        } // drop → disconnect
        assert_eq!(source.recv().unwrap(), sample());
        let mut tx2 = TcpTransport::connect(("127.0.0.1", port)).unwrap();
        tx2.send(&sample()).unwrap();
        assert_eq!(source.recv().unwrap(), sample());
    }

    #[cfg(unix)]
    #[test]
    fn unix_transport_round_trip() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("martensite-godot-test-{}.sock", std::process::id()));
        let mut source = FrameSource::bind_unix(&path).unwrap();
        let mut tx = UnixSocketTransport::connect(&path).unwrap();
        tx.send(&sample()).unwrap();
        assert_eq!(source.recv().unwrap(), sample());
        std::fs::remove_file(&path).ok();
    }
}
