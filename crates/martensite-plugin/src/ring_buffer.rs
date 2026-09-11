//! Zero-allocation shared-memory ring buffer for plugin paint commands.
//!
//! The host and the WebAssembly guest share a single linear memory region.
//! The guest writes raw `PluginPaintCmd` records followed by their payload
//! directly into the buffer, and the host consumes them by parsing in place,
//! avoiding any per-frame allocation or serialization overhead.

use std::fmt;

use tracing::error;

/// Default size of the shared linear memory ring buffer (256 KiB).
///
/// # Examples
///
/// ```
/// use martensite_plugin::DEFAULT_CAPACITY;
///
/// assert_eq!(DEFAULT_CAPACITY, 256 * 1024);
/// ```
pub const DEFAULT_CAPACITY: usize = 256 * 1024;

/// Number of leading bytes reserved for the producer/consumer cursors in a
/// shared ring region.
///
/// When the ring buffer lives inside the guest's linear memory, the first
/// [`SHARED_HEADER_SIZE`] bytes hold the `head` (consumer cursor) and `tail`
/// (producer cursor) as little-endian `u32` values so that both sides can
/// observe the buffer state without a host call. The remaining bytes are the
/// circular payload area.
///
/// # Examples
///
/// ```
/// use martensite_plugin::ring_buffer::SHARED_HEADER_SIZE;
///
/// assert_eq!(SHARED_HEADER_SIZE, 8);
/// ```
pub const SHARED_HEADER_SIZE: usize = 8;

/// Fixed header size of a [`PluginPaintCmd`] in bytes.
const CMD_SIZE: usize = std::mem::size_of::<PluginPaintCmd>();

/// A raw paint command produced by a WebAssembly plugin.
///
/// The record is laid out exactly as the guest writes it into shared memory so
/// that the host can read it directly from the ring buffer without copying.
///
/// # Examples
///
/// ```
/// use martensite_plugin::PluginPaintCmd;
///
/// let cmd = PluginPaintCmd {
///     cmd_type: 1,
///     flags: 0,
///     data_len: 12,
///     payload_offset: 100,
/// };
///
/// assert_eq!(cmd.data_len, 12);
/// ```
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct PluginPaintCmd {
    /// Discriminant of the paint operation (e.g. 0=DrawLine, 1=FillRect).
    pub cmd_type: u16,
    /// Command flags for future extensions.
    pub flags: u16,
    /// Length of the variable-length payload in bytes.
    pub data_len: u32,
    /// Byte offset of the payload relative to the start of the ring buffer.
    pub payload_offset: u32,
}

impl PluginPaintCmd {
    /// Returns the number of bytes occupied by the command header.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_plugin::PluginPaintCmd;
    ///
    /// assert_eq!(PluginPaintCmd::header_size(), 12);
    /// ```
    #[inline]
    pub const fn header_size() -> usize {
        CMD_SIZE
    }

    /// Writes the command into the supplied byte slice in little-endian order.
    ///
    /// Returns `None` if `buf` is too short.
    fn write_to(&self, buf: &mut [u8]) -> Option<()> {
        if buf.len() < CMD_SIZE {
            return None;
        }
        let (cmd_type, rest) = buf.split_at_mut(2);
        cmd_type.copy_from_slice(&self.cmd_type.to_le_bytes());
        let (flags, rest) = rest.split_at_mut(2);
        flags.copy_from_slice(&self.flags.to_le_bytes());
        let (data_len, rest) = rest.split_at_mut(4);
        data_len.copy_from_slice(&self.data_len.to_le_bytes());
        let (payload_offset, _) = rest.split_at_mut(4);
        payload_offset.copy_from_slice(&self.payload_offset.to_le_bytes());
        Some(())
    }

    /// Reads a command from the supplied byte slice in little-endian order.
    ///
    /// Returns `None` if the slice is too short.
    fn read_from(buf: &[u8]) -> Option<Self> {
        if buf.len() < CMD_SIZE {
            return None;
        }
        let cmd_type = u16::from_le_bytes([buf[0], buf[1]]);
        let flags = u16::from_le_bytes([buf[2], buf[3]]);
        let data_len = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]);
        let payload_offset = u32::from_le_bytes([buf[8], buf[9], buf[10], buf[11]]);
        Some(Self {
            cmd_type,
            flags,
            data_len,
            payload_offset,
        })
    }
}

/// Errors that can occur while writing into a [`PluginRingBuffer`].
///
/// # Examples
///
/// ```
/// use martensite_plugin::RingBufferError;
///
/// let err = RingBufferError::BufferFull;
/// assert_eq!(err.to_string(), "ring buffer is full");
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RingBufferError {
    /// The ring buffer does not have enough contiguous free space for the
    /// command and its payload.
    BufferFull,
    /// The command's `data_len` field does not match the supplied payload.
    PayloadLengthMismatch,
    /// The command's `payload_offset` or `data_len` is outside the buffer.
    InvalidPayloadOffset,
}

impl fmt::Display for RingBufferError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RingBufferError::BufferFull => write!(f, "ring buffer is full"),
            RingBufferError::PayloadLengthMismatch => {
                write!(f, "command data_len does not match payload length")
            }
            RingBufferError::InvalidPayloadOffset => {
                write!(f, "command payload offset or length is out of bounds")
            }
        }
    }
}

impl std::error::Error for RingBufferError {}

/// A circular command buffer backed by a shared linear memory slice.
///
/// The buffer is intentionally not thread-safe; it is intended for single-
/// producer/single-consumer use between the plugin guest and the host render
/// thread. All hot-path reads return borrowed slices and perform no allocation.
///
/// # Examples
///
/// ```
/// use martensite_plugin::{PluginPaintCmd, PluginRingBuffer, DEFAULT_CAPACITY};
///
/// let mut backing = vec![0u8; DEFAULT_CAPACITY];
/// let mut rb = PluginRingBuffer::new(&mut backing);
///
/// let cmd = PluginPaintCmd {
///     cmd_type: 1,
///     flags: 0,
///     data_len: 4,
///     payload_offset: 0,
/// };
/// rb.produce(&cmd, &[1, 2, 3, 4]).unwrap();
///
/// let (read_cmd, payload) = rb.consume().unwrap();
/// assert_eq!(read_cmd.cmd_type, 1);
/// assert_eq!(payload, &[1, 2, 3, 4]);
/// ```
pub struct PluginRingBuffer<'a> {
    data: &'a mut [u8],
    /// Optional leading cursor block when the buffer lives in shared memory.
    ///
    /// When present, `head` and `tail` are mirrored into this 8-byte
    /// little-endian header after every mutation so the other side of the
    /// shared region can observe the cursors without a host call.
    header: Option<&'a mut [u8]>,
    head: u32,
    tail: u32,
}

impl<'a> PluginRingBuffer<'a> {
    /// Creates a ring buffer over the supplied shared memory slice.
    ///
    /// The slice must be large enough for at least one command header plus a
    /// small payload. The buffer starts empty and the cursors are held only
    /// in this struct; use [`PluginRingBuffer::new_shared`] when the slice is
    /// a region shared with another party (e.g. guest linear memory).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_plugin::{PluginRingBuffer, DEFAULT_CAPACITY};
    ///
    /// let mut backing = vec![0u8; DEFAULT_CAPACITY];
    /// let rb = PluginRingBuffer::new(&mut backing);
    /// assert!(rb.is_empty());
    /// ```
    pub fn new(data: &'a mut [u8]) -> Self {
        Self {
            data,
            header: None,
            head: 0,
            tail: 0,
        }
    }

    /// Creates a ring buffer over a shared memory region with a persisted
    /// cursor header.
    ///
    /// The first [`SHARED_HEADER_SIZE`] bytes of `data` are interpreted as the
    /// `head`/`tail` cursor block (little-endian `u32` each); the rest is the
    /// circular payload area. Cursors are read on construction and written
    /// back after every [`produce`](Self::produce)/[`consume`](Self::consume)
    /// so a peer sharing the same region sees consistent state. Corrupt
    /// out-of-range cursors reset the buffer to empty.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_plugin::ring_buffer::SHARED_HEADER_SIZE;
    /// use martensite_plugin::{PluginPaintCmd, PluginRingBuffer};
    ///
    /// // 8-byte cursor header + 64 bytes of payload area.
    /// let mut region = vec![0u8; SHARED_HEADER_SIZE + 64];
    /// let mut rb = PluginRingBuffer::new_shared(&mut region);
    /// let cmd = PluginPaintCmd {
    ///     cmd_type: 1,
    ///     flags: 0,
    ///     data_len: 4,
    ///     payload_offset: 0,
    /// };
    /// rb.produce(&cmd, &[1, 2, 3, 4]).unwrap();
    /// // The peer can observe `tail` in the header.
    /// assert_eq!(u32::from_le_bytes(region[4..8].try_into().unwrap()), 16);
    /// ```
    pub fn new_shared(data: &'a mut [u8]) -> Self {
        if data.len() < SHARED_HEADER_SIZE {
            return Self::new(data);
        }
        let (header, payload) = data.split_at_mut(SHARED_HEADER_SIZE);
        let head = u32::from_le_bytes([header[0], header[1], header[2], header[3]]);
        let tail = u32::from_le_bytes([header[4], header[5], header[6], header[7]]);
        let capacity = payload.len() as u32;
        // Reject corrupt cursors; an empty buffer is the safe fallback.
        let (head, tail) = if head <= capacity && tail <= capacity {
            (head, tail)
        } else {
            (0, 0)
        };
        let mut rb = Self {
            data: payload,
            header: Some(header),
            head,
            tail,
        };
        rb.sync_header();
        rb
    }

    /// Mirrors the in-struct cursors into the shared header, if present.
    fn sync_header(&mut self) {
        if let Some(header) = self.header.as_deref_mut() {
            header[0..4].copy_from_slice(&self.head.to_le_bytes());
            header[4..8].copy_from_slice(&self.tail.to_le_bytes());
        }
    }

    /// Returns the total capacity of the buffer in bytes.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_plugin::{PluginRingBuffer, DEFAULT_CAPACITY};
    ///
    /// let mut backing = vec![0u8; DEFAULT_CAPACITY];
    /// let rb = PluginRingBuffer::new(&mut backing);
    /// assert_eq!(rb.capacity(), DEFAULT_CAPACITY);
    /// ```
    pub fn capacity(&self) -> usize {
        self.data.len()
    }

    fn capacity_u32(&self) -> u32 {
        self.data.len() as u32
    }

    /// Returns the number of bytes currently stored in the buffer.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_plugin::{PluginPaintCmd, PluginRingBuffer, DEFAULT_CAPACITY};
    ///
    /// let mut backing = vec![0u8; DEFAULT_CAPACITY];
    /// let mut rb = PluginRingBuffer::new(&mut backing);
    /// assert_eq!(rb.len(), 0);
    ///
    /// let cmd = PluginPaintCmd { cmd_type: 1, flags: 0, data_len: 0, payload_offset: 0 };
    /// rb.produce(&cmd, &[]).unwrap();
    /// assert_eq!(rb.len(), PluginPaintCmd::header_size());
    /// ```
    pub fn len(&self) -> usize {
        let cap = self.capacity_u32();
        if self.head == self.tail {
            0
        } else if self.tail > self.head {
            (self.tail - self.head) as usize
        } else {
            (cap - self.head + self.tail) as usize
        }
    }

    /// Returns `true` if the buffer contains no commands.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_plugin::{PluginRingBuffer, DEFAULT_CAPACITY};
    ///
    /// let mut backing = vec![0u8; DEFAULT_CAPACITY];
    /// let rb = PluginRingBuffer::new(&mut backing);
    /// assert!(rb.is_empty());
    /// ```
    pub fn is_empty(&self) -> bool {
        self.head == self.tail
    }

    /// Returns the total byte size of a record containing `payload_len` bytes.
    #[inline]
    const fn record_size(payload_len: usize) -> usize {
        CMD_SIZE.saturating_add(payload_len)
    }

    /// Writes a command and its payload into the ring buffer.
    ///
    /// The `payload_offset` field of `cmd` is ignored; it is overwritten with
    /// the actual offset of the payload in the buffer. `data_len` must match
    /// `payload.len()`. Records are never split across the buffer boundary.
    ///
    /// # Errors
    ///
    /// Returns [`RingBufferError::PayloadLengthMismatch`] if `cmd.data_len` does
    /// not equal `payload.len()`, or [`RingBufferError::BufferFull`] if the
    /// command does not fit.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_plugin::{PluginPaintCmd, PluginRingBuffer, DEFAULT_CAPACITY};
    ///
    /// let mut backing = vec![0u8; DEFAULT_CAPACITY];
    /// let mut rb = PluginRingBuffer::new(&mut backing);
    ///
    /// let cmd = PluginPaintCmd {
    ///     cmd_type: 2,
    ///     flags: 0,
    ///     data_len: 6,
    ///     payload_offset: 0,
    /// };
    /// rb.produce(&cmd, &[9; 6]).unwrap();
    /// ```
    pub fn produce(&mut self, cmd: &PluginPaintCmd, payload: &[u8]) -> Result<(), RingBufferError> {
        if cmd.data_len as usize != payload.len() {
            return Err(RingBufferError::PayloadLengthMismatch);
        }
        let payload_len = payload.len();
        let total = Self::record_size(payload_len);
        if total == 0 || self.capacity() == 0 {
            return Err(RingBufferError::BufferFull);
        }
        // Keep one byte of slack so that head == tail always means "empty".
        if self.len().saturating_add(total) >= self.capacity() {
            return Err(RingBufferError::BufferFull);
        }

        let cap_u32 = self.capacity_u32();
        let tail = self.tail as usize;

        // Decide where to write the new record. Records are never split across
        // the end of the buffer; if there is not enough contiguous space at the
        // tail, wrap to the start of the buffer and discard the trailing slack.
        // The consumer `head` is left unchanged so any unconsumed records at
        // the end of the buffer are consumed before the newly-wrapped record.
        // If the buffer is empty before the wrap, the consumer can safely start
        // from zero.
        let (write_pos, wrapped) = if self.tail < self.head {
            if tail.saturating_add(total) > self.head as usize {
                return Err(RingBufferError::BufferFull);
            }
            (tail, false)
        } else if tail.saturating_add(total) <= self.capacity() {
            (tail, false)
        } else if total < self.head as usize {
            (0, true)
        } else {
            return Err(RingBufferError::BufferFull);
        };

        if wrapped && self.tail == self.head {
            self.head = 0;
        }

        let payload_offset = write_pos + CMD_SIZE;
        let mut stored_cmd = *cmd;
        stored_cmd.payload_offset = payload_offset as u32;
        stored_cmd
            .write_to(&mut self.data[write_pos..])
            .ok_or(RingBufferError::BufferFull)?;
        self.data[payload_offset..payload_offset.saturating_add(payload_len)]
            .copy_from_slice(payload);

        self.tail = (write_pos + total) as u32;
        if self.tail >= cap_u32 {
            self.tail = 0;
        }
        self.sync_header();
        Ok(())
    }

    /// Reads and removes the next command from the ring buffer.
    ///
    /// Returns `None` when the buffer is empty or the next record is malformed.
    /// The returned payload is a borrowed view into the underlying shared
    /// memory, so no allocation occurs on the readback hot path.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_plugin::{PluginPaintCmd, PluginRingBuffer, DEFAULT_CAPACITY};
    ///
    /// let mut backing = vec![0u8; DEFAULT_CAPACITY];
    /// let mut rb = PluginRingBuffer::new(&mut backing);
    ///
    /// let cmd = PluginPaintCmd {
    ///     cmd_type: 0,
    ///     flags: 0,
    ///     data_len: 0,
    ///     payload_offset: 0,
    /// };
    /// rb.produce(&cmd, &[]).unwrap();
    ///
    /// let (read_cmd, payload) = rb.consume().unwrap();
    /// assert_eq!(payload.len(), 0);
    /// assert_eq!(read_cmd.cmd_type, 0);
    /// ```
    pub fn consume(&mut self) -> Option<(PluginPaintCmd, &[u8])> {
        if self.is_empty() {
            return None;
        }

        let cap_u32 = self.capacity_u32();
        if self.head >= cap_u32 {
            self.head = 0;
            if self.is_empty() {
                self.sync_header();
                return None;
            }
        }

        let pos = self.head as usize;
        if pos.saturating_add(CMD_SIZE) > self.data.len() {
            // Head points into the slack created by a previous wrap-around.
            self.head = 0;
            return self.consume();
        }

        let cmd = match PluginPaintCmd::read_from(&self.data[pos..]) {
            Some(cmd) => cmd,
            None => {
                // Fatal corruption: the header could not be parsed. Reset the
                // buffer so the consumer does not wedge on the same bad record
                // forever.
                error!(
                    head = self.head,
                    tail = self.tail,
                    pos,
                    "ring buffer: malformed command header; resetting cursors"
                );
                self.head = 0;
                self.tail = 0;
                self.sync_header();
                return None;
            }
        };
        let payload_len = cmd.data_len as usize;
        let expected_payload_start = pos.saturating_add(CMD_SIZE);
        let expected_payload_end = expected_payload_start.saturating_add(payload_len);

        // The guest is untrusted; reject any record whose payload is not
        // contiguously after the header within the buffer. A malformed record
        // is treated as fatal corruption: resetting the cursors prevents the
        // consumer from spinning forever on the same record.
        if cmd.payload_offset as usize != expected_payload_start
            || expected_payload_end > self.data.len()
            || expected_payload_end < expected_payload_start
        {
            error!(
                head = self.head,
                tail = self.tail,
                pos,
                payload_offset = cmd.payload_offset,
                data_len = cmd.data_len,
                expected_payload_start,
                expected_payload_end,
                buf_len = self.data.len(),
                "ring buffer: malformed record (payload not contiguous); resetting cursors"
            );
            self.head = 0;
            self.tail = 0;
            self.sync_header();
            return None;
        }

        self.head = (expected_payload_end) as u32;
        if self.head >= cap_u32 {
            self.head = 0;
        }
        // Write the cursors back before borrowing `data` for the payload.
        if let Some(header) = self.header.as_deref_mut() {
            header[0..4].copy_from_slice(&self.head.to_le_bytes());
            header[4..8].copy_from_slice(&self.tail.to_le_bytes());
        }
        let payload = &self.data[expected_payload_start..expected_payload_end];
        Some((cmd, payload))
    }

    /// Drains all currently available commands from the buffer, invoking the
    /// provided closure for each command and its borrowed payload.
    ///
    /// This is the preferred host readback API because it keeps all reads
    /// zero-allocation and bounded.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_plugin::{PluginPaintCmd, PluginRingBuffer, DEFAULT_CAPACITY};
    ///
    /// let mut backing = vec![0u8; DEFAULT_CAPACITY];
    /// let mut rb = PluginRingBuffer::new(&mut backing);
    ///
    /// let cmd = PluginPaintCmd {
    ///     cmd_type: 1,
    ///     flags: 0,
    ///     data_len: 2,
    ///     payload_offset: 0,
    /// };
    /// rb.produce(&cmd, &[10, 20]).unwrap();
    /// rb.produce(&cmd, &[30, 40]).unwrap();
    ///
    /// let mut count = 0;
    /// rb.drain(|_cmd, payload| {
    ///     count += 1;
    ///     assert_eq!(payload.len(), 2);
    /// });
    /// assert_eq!(count, 2);
    /// ```
    pub fn drain<F>(&mut self, mut f: F)
    where
        F: FnMut(&PluginPaintCmd, &[u8]),
    {
        while let Some((cmd, payload)) = self.consume() {
            f(&cmd, payload);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_buffer_returns_none() {
        let mut data = vec![0u8; DEFAULT_CAPACITY];
        let mut rb = PluginRingBuffer::new(&mut data);
        assert!(rb.is_empty());
        assert_eq!(rb.len(), 0);
        assert!(rb.consume().is_none());
    }

    #[test]
    fn produce_and_consume_single_command() {
        let mut data = vec![0u8; DEFAULT_CAPACITY];
        let mut rb = PluginRingBuffer::new(&mut data);
        let cmd = PluginPaintCmd {
            cmd_type: 1,
            flags: 0,
            data_len: 4,
            payload_offset: 0,
        };
        rb.produce(&cmd, &[1, 2, 3, 4]).unwrap();
        assert_eq!(rb.len(), CMD_SIZE + 4);

        let (read_cmd, payload) = rb.consume().unwrap();
        assert_eq!(read_cmd.cmd_type, 1);
        assert_eq!(read_cmd.data_len, 4);
        assert_eq!(payload, &[1, 2, 3, 4]);
        assert!(rb.is_empty());
    }

    #[test]
    fn payload_length_mismatch_is_rejected() {
        let mut data = vec![0u8; DEFAULT_CAPACITY];
        let mut rb = PluginRingBuffer::new(&mut data);
        let cmd = PluginPaintCmd {
            cmd_type: 1,
            flags: 0,
            data_len: 10,
            payload_offset: 0,
        };
        assert_eq!(
            rb.produce(&cmd, &[1, 2, 3, 4]),
            Err(RingBufferError::PayloadLengthMismatch)
        );
    }

    #[test]
    fn wrap_around_reuses_start_of_buffer() {
        let mut data = vec![0u8; 64];
        let mut rb = PluginRingBuffer::new(&mut data);

        // Fill most of the buffer.
        let cmd = PluginPaintCmd {
            cmd_type: 2,
            flags: 0,
            data_len: 40,
            payload_offset: 0,
        };
        rb.produce(&cmd, &[7; 40]).unwrap();
        rb.consume().unwrap();

        // A new command that does not fit at the old tail should wrap to the
        // start of the buffer.
        let cmd2 = PluginPaintCmd {
            cmd_type: 3,
            flags: 0,
            data_len: 16,
            payload_offset: 0,
        };
        rb.produce(&cmd2, &[8; 16]).unwrap();

        let (read_cmd, payload) = rb.consume().unwrap();
        assert_eq!(read_cmd.cmd_type, 3);
        assert_eq!(payload, &[8; 16]);
        assert!(rb.is_empty());
    }

    #[test]
    fn buffer_full_is_reported() {
        let mut data = vec![0u8; 64];
        let mut rb = PluginRingBuffer::new(&mut data);

        let cmd = PluginPaintCmd {
            cmd_type: 1,
            flags: 0,
            data_len: 40,
            payload_offset: 0,
        };
        rb.produce(&cmd, &[1; 40]).unwrap();
        assert_eq!(rb.produce(&cmd, &[1; 40]), Err(RingBufferError::BufferFull));
    }

    #[test]
    fn drain_visits_all_commands() {
        let mut data = vec![0u8; DEFAULT_CAPACITY];
        let mut rb = PluginRingBuffer::new(&mut data);

        let cmd = PluginPaintCmd {
            cmd_type: 1,
            flags: 0,
            data_len: 2,
            payload_offset: 0,
        };
        for i in 0u8..5 {
            rb.produce(&cmd, &[i, i + 1]).unwrap();
        }

        let mut count = 0;
        rb.drain(|_cmd, payload| {
            assert_eq!(payload.len(), 2);
            count += 1;
        });
        assert_eq!(count, 5);
        assert!(rb.is_empty());
    }

    #[test]
    fn cmd_serialization_roundtrips() {
        let original = PluginPaintCmd {
            cmd_type: 0xABCD,
            flags: 0x1234,
            data_len: 0xDEAD_BEEF,
            payload_offset: 0xCAFE_BABE,
        };
        let mut buf = [0u8; CMD_SIZE];
        original.write_to(&mut buf).unwrap();
        let parsed = PluginPaintCmd::read_from(&buf).unwrap();
        assert_eq!(original, parsed);
    }

    #[test]
    fn zero_payload_command_roundtrips() {
        let mut data = vec![0u8; 64];
        let mut rb = PluginRingBuffer::new(&mut data);

        let cmd = PluginPaintCmd {
            cmd_type: 0,
            flags: 0,
            data_len: 0,
            payload_offset: 0,
        };
        rb.produce(&cmd, &[]).unwrap();
        let (read_cmd, payload) = rb.consume().unwrap();
        assert_eq!(read_cmd.cmd_type, cmd.cmd_type);
        assert_eq!(read_cmd.flags, cmd.flags);
        assert_eq!(read_cmd.data_len, cmd.data_len);
        assert!(payload.is_empty());
    }

    #[test]
    fn malformed_record_resets_cursors_and_does_not_wedge() {
        let mut data = vec![0u8; 64];

        // Manually craft a record whose payload_offset does not match the
        // expected position right after the header. Write it directly into the
        // backing store before handing the slice to the ring buffer.
        let bad = PluginPaintCmd {
            cmd_type: 1,
            flags: 0,
            data_len: 4,
            payload_offset: 0, // wrong: should be CMD_SIZE
        };
        bad.write_to(&mut data[0..]).unwrap();
        // Fill the payload area with non-zero bytes so the record looks
        // populated.
        data[CMD_SIZE..CMD_SIZE + 4].fill(9);

        let mut rb = PluginRingBuffer::new(&mut data);
        rb.head = 0;
        rb.tail = (CMD_SIZE + 4) as u32;

        // First call detects corruption and returns None.
        assert!(rb.consume().is_none());
        // Cursors were reset, so the buffer is now empty and subsequent calls
        // do not wedge on the same bad record.
        assert!(rb.is_empty());
        assert!(rb.consume().is_none());

        // The buffer is usable again after the reset.
        let cmd = PluginPaintCmd {
            cmd_type: 2,
            flags: 0,
            data_len: 2,
            payload_offset: 0,
        };
        rb.produce(&cmd, &[3, 4]).unwrap();
        let (read_cmd, payload) = rb.consume().unwrap();
        assert_eq!(read_cmd.cmd_type, 2);
        assert_eq!(payload, &[3, 4]);
    }

    #[test]
    fn malformed_record_resets_shared_header() {
        use crate::ring_buffer::SHARED_HEADER_SIZE;
        let mut region = vec![0u8; SHARED_HEADER_SIZE + 64];

        // Write a valid record, then corrupt its payload_offset so the next
        // consume treats it as malformed. The corruption is applied directly to
        // the backing region before the ring buffer borrows it.
        {
            let mut rb = PluginRingBuffer::new_shared(&mut region);
            let cmd = PluginPaintCmd {
                cmd_type: 1,
                flags: 0,
                data_len: 4,
                payload_offset: 0,
            };
            rb.produce(&cmd, &[1, 2, 3, 4]).unwrap();
        }
        // Corrupt the payload_offset field (bytes 8..12 of the payload area,
        // i.e. region offset SHARED_HEADER_SIZE + 8).
        let off = SHARED_HEADER_SIZE + 8;
        region[off..off + 4].copy_from_slice(&99u32.to_le_bytes());

        {
            let mut rb = PluginRingBuffer::new_shared(&mut region);
            assert!(rb.consume().is_none());
        }
        // The shared header must reflect the reset cursors (head == tail == 0).
        let head = u32::from_le_bytes(region[0..4].try_into().unwrap());
        let tail = u32::from_le_bytes(region[4..8].try_into().unwrap());
        assert_eq!(head, 0);
        assert_eq!(tail, 0);
    }
}
