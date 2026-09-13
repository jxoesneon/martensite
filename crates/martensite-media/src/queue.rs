//! Decode-ahead frame queue with PTS-deadline pacing and drop accounting.
//!
//! [`FrameQueue`] sits between a [`crate::decoder::VideoDecoder`] and the
//! presentation path. It keeps 2–3 decoded frames buffered, drops the
//! stalest frame under pressure, skips frames whose PTS deadline has already
//! passed by more than 1.5 frame intervals, and exposes the
//! `presented`/`dropped` counters the v0.16.0 4K120 gate reads
//! (`< 0.1%` drops, `< 1%` CPU dispatch utilization).
//!
//! All timing is caller-injected (`now_nanos`) so the policy is fully
//! deterministic under test; a real presentation loop feeds it a vsync or
//! monotonic clock.

use std::collections::VecDeque;
use std::time::Instant;

use crate::decoder::DecodedFrame;

/// The result of polling a [`FrameQueue`] for the next presentation step.
///
/// # Examples
///
/// ```
/// use martensite_media::queue::QueueAction;
///
/// let action = QueueAction::Empty;
/// assert!(matches!(action, QueueAction::Empty));
/// ```
#[derive(Debug)]
pub enum QueueAction {
    /// A frame whose PTS has been reached: present it now.
    Present(DecodedFrame),
    /// The head frame is early; wait `wait_nanos` until its PTS. Lets the
    /// event loop sleep until the next deadline instead of polling.
    WaitFor {
        /// Nanoseconds until the head frame's PTS.
        wait_nanos: u64,
    },
    /// No decoded frames are queued; feed the decoder more packets.
    Empty,
}

/// A bounded ring of decoded frames ordered by presentation timestamp.
///
/// `push` inserts in PTS order; when the queue is at capacity the frame
/// with the earliest (stalest) PTS is evicted and counted as dropped —
/// the "last-buffer-drop under pressure" policy from the v0.16.0 spec.
///
/// # Examples
///
/// ```
/// use martensite_media::decoder::{DecodedFrame, DecoderConfig, EncodedPacket, MockDecoder, VideoCodec, VideoDecoder};
/// use martensite_media::queue::{FrameQueue, QueueAction};
///
/// let mut dec = MockDecoder::init(DecoderConfig::new(VideoCodec::H264, 64, 64)).unwrap();
/// dec.send_packet(&EncodedPacket::new(vec![0x67], 0, 16_666_667)).unwrap();
///
/// let mut queue = FrameQueue::new(3);
/// queue.push(dec.try_recv_frame().unwrap().unwrap());
/// match queue.pop_present(0) {
///     QueueAction::Present(frame) => assert_eq!(frame.metadata.frame_index, 1),
///     _ => panic!("expected a presentable frame"),
/// }
/// ```
#[derive(Debug)]
pub struct FrameQueue {
    frames: VecDeque<DecodedFrame>,
    capacity: usize,
    received: u64,
    presented: u64,
    dropped: u64,
    /// Cumulative CPU time spent inside `push`/`pop_present` — the queue's
    /// share of the dispatch-utilization budget.
    dispatch_nanos: u64,
}

impl FrameQueue {
    /// Creates a queue holding at most `capacity` decoded frames (2–3 is the
    /// recommended decode-ahead depth).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media::queue::FrameQueue;
    ///
    /// let queue = FrameQueue::new(3);
    /// assert_eq!(queue.capacity(), 3);
    /// assert!(queue.is_empty());
    /// ```
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            frames: VecDeque::with_capacity(capacity.max(1)),
            capacity: capacity.max(1),
            received: 0,
            presented: 0,
            dropped: 0,
            dispatch_nanos: 0,
        }
    }

    /// Returns the configured decode-ahead capacity.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media::queue::FrameQueue;
    ///
    /// assert_eq!(FrameQueue::new(2).capacity(), 2);
    /// ```
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Number of frames currently queued.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media::queue::FrameQueue;
    ///
    /// assert_eq!(FrameQueue::new(3).len(), 0);
    /// ```
    #[must_use]
    pub fn len(&self) -> usize {
        self.frames.len()
    }

    /// Returns `true` when no frames are queued.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media::queue::FrameQueue;
    ///
    /// assert!(FrameQueue::new(3).is_empty());
    /// ```
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// PTS of the head-of-queue frame, if any.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media::queue::FrameQueue;
    ///
    /// assert_eq!(FrameQueue::new(3).next_pts_nanos(), None);
    /// ```
    #[must_use]
    pub fn next_pts_nanos(&self) -> Option<u64> {
        self.frames.front().map(|f| f.metadata.pts_nanos)
    }

    /// Inserts a decoded frame in PTS order.
    ///
    /// When the queue is full, the frame with the earliest PTS (incoming or
    /// queued) is evicted and counted as dropped, so the newest decode
    /// output is never blocked by stale backlog.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media::decoder::{DecodedFrame, HardwareHandle, VideoFrameMetadata, ColorRange};
    /// use martensite_media::decoder::VideoPixelFormat;
    /// use martensite_media::queue::FrameQueue;
    ///
    /// let meta = VideoFrameMetadata::new(64, 64, VideoPixelFormat::Nv12, ColorRange::Limited);
    /// let mut queue = FrameQueue::new(1);
    /// queue.push(DecodedFrame::new(HardwareHandle::Mock { id: 1 }, meta.clone()));
    /// queue.push(DecodedFrame::new(HardwareHandle::Mock { id: 2 }, meta));
    /// assert_eq!(queue.len(), 1);
    /// assert_eq!(queue.dropped(), 1); // stale frame evicted under pressure
    /// ```
    pub fn push(&mut self, frame: DecodedFrame) {
        let start = Instant::now();
        self.received = self.received.saturating_add(1);

        // Insert sorted by PTS (decode output is usually near-ordered, so a
        // reverse scan is O(1) in the common case).
        let pos = self
            .frames
            .iter()
            .rposition(|f| f.metadata.pts_nanos <= frame.metadata.pts_nanos)
            .map_or(0, |i| i + 1);
        self.frames.insert(pos, frame);

        if self.frames.len() > self.capacity {
            self.frames.pop_front();
            self.dropped = self.dropped.saturating_add(1);
        }
        self.dispatch_nanos = self
            .dispatch_nanos
            .saturating_add(start.elapsed().as_nanos() as u64);
    }

    /// Polls the queue for the next presentation step at `now_nanos`.
    ///
    /// Frames whose deadline (`pts + 1.5 × duration`) has passed are skipped
    /// and counted as dropped before the head frame is considered — this is
    /// the spec's ">1.5 frame-intervals behind" skip rule.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media::decoder::{DecodedFrame, HardwareHandle, VideoFrameMetadata, ColorRange};
    /// use martensite_media::decoder::VideoPixelFormat;
    /// use martensite_media::queue::{FrameQueue, QueueAction};
    ///
    /// let mut meta = VideoFrameMetadata::new(64, 64, VideoPixelFormat::Nv12, ColorRange::Limited);
    /// meta.pts_nanos = 1_000_000;
    /// meta.duration_nanos = 16_666_667;
    ///
    /// let mut queue = FrameQueue::new(3);
    /// queue.push(DecodedFrame::new(HardwareHandle::Mock { id: 1 }, meta));
    /// // At t=0 the frame is early: the caller should wait ~1 ms.
    /// assert!(matches!(queue.pop_present(0), QueueAction::WaitFor { .. }));
    /// // At t=2 ms the PTS has been reached.
    /// assert!(matches!(queue.pop_present(2_000_000), QueueAction::Present(_)));
    /// ```
    pub fn pop_present(&mut self, now_nanos: u64) -> QueueAction {
        let start = Instant::now();

        // Skip stale frames: deadline = pts + 1.5 * duration.
        while let Some(front) = self.frames.front() {
            let deadline = front
                .metadata
                .pts_nanos
                .saturating_add(front.metadata.duration_nanos * 3 / 2);
            if now_nanos > deadline && self.frames.len() > 1 {
                self.frames.pop_front();
                self.dropped = self.dropped.saturating_add(1);
            } else {
                break;
            }
        }

        let action = match self.frames.front() {
            None => QueueAction::Empty,
            Some(front) if now_nanos >= front.metadata.pts_nanos => {
                self.presented = self.presented.saturating_add(1);
                QueueAction::Present(self.frames.pop_front().expect("front exists"))
            }
            Some(front) => QueueAction::WaitFor {
                wait_nanos: front.metadata.pts_nanos - now_nanos,
            },
        };

        self.dispatch_nanos = self
            .dispatch_nanos
            .saturating_add(start.elapsed().as_nanos() as u64);
        action
    }

    /// Removes all queued frames without counting them as presented or
    /// dropped (seek/flush path).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media::queue::FrameQueue;
    ///
    /// let mut queue = FrameQueue::new(3);
    /// queue.clear();
    /// assert!(queue.is_empty());
    /// ```
    pub fn clear(&mut self) {
        self.frames.clear();
    }

    /// Total frames pushed into the queue.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media::queue::FrameQueue;
    ///
    /// assert_eq!(FrameQueue::new(3).received(), 0);
    /// ```
    #[must_use]
    pub fn received(&self) -> u64 {
        self.received
    }

    /// Total frames delivered to the presentation path.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media::queue::FrameQueue;
    ///
    /// assert_eq!(FrameQueue::new(3).presented(), 0);
    /// ```
    #[must_use]
    pub fn presented(&self) -> u64 {
        self.presented
    }

    /// Total frames dropped (evicted under pressure or skipped past their
    /// PTS deadline).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media::queue::FrameQueue;
    ///
    /// assert_eq!(FrameQueue::new(3).dropped(), 0);
    /// ```
    #[must_use]
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    /// Percentage of delivered-or-dropped frames that were dropped.
    ///
    /// This is the counter the 4K120 gate reads: it must stay `< 0.1%`.
    /// Returns 0 when nothing has been presented or dropped yet.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media::queue::FrameQueue;
    ///
    /// assert_eq!(FrameQueue::new(3).drop_rate_pct(), 0.0);
    /// ```
    #[must_use]
    pub fn drop_rate_pct(&self) -> f64 {
        let total = self.presented.saturating_add(self.dropped);
        if total == 0 {
            0.0
        } else {
            (self.dropped as f64 / total as f64) * 100.0
        }
    }

    /// Cumulative CPU time spent inside queue operations, in nanoseconds.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media::queue::FrameQueue;
    ///
    /// assert_eq!(FrameQueue::new(3).dispatch_nanos(), 0);
    /// ```
    #[must_use]
    pub fn dispatch_nanos(&self) -> u64 {
        self.dispatch_nanos
    }

    /// Estimated CPU utilization percentage of the queue at `target_fps`,
    /// assuming one `push` + one `pop_present` per frame.
    ///
    /// Extends `VideoSurface::cpu_utilization_pct` to the decode-ahead path;
    /// the 4K60 gate requires `< 1%`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media::queue::FrameQueue;
    ///
    /// let queue = FrameQueue::new(3);
    /// assert_eq!(queue.cpu_utilization_pct(60.0), 0.0);
    /// ```
    #[must_use]
    pub fn cpu_utilization_pct(&self, target_fps: f64) -> f64 {
        if target_fps <= 0.0 || self.received == 0 {
            return 0.0;
        }
        let budget_total = 1_000_000_000.0 / target_fps * self.received as f64;
        (self.dispatch_nanos as f64 / budget_total) * 100.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decoder::{HardwareHandle, VideoFrameMetadata, VideoPixelFormat};
    use crate::surface::ColorRange;

    fn frame(pts: u64, index: u64) -> DecodedFrame {
        let mut meta = VideoFrameMetadata::new(64, 64, VideoPixelFormat::Nv12, ColorRange::Limited);
        meta.pts_nanos = pts;
        meta.duration_nanos = 16_666_667;
        meta.frame_index = index;
        DecodedFrame::new(HardwareHandle::Mock { id: index }, meta)
    }

    #[test]
    fn pts_ordered_insert() {
        let mut q = FrameQueue::new(4);
        q.push(frame(30_000_000, 3));
        q.push(frame(10_000_000, 1));
        q.push(frame(20_000_000, 2));
        assert_eq!(q.next_pts_nanos(), Some(10_000_000));
        match q.pop_present(10_000_000) {
            QueueAction::Present(f) => assert_eq!(f.metadata.frame_index, 1),
            _ => panic!("expected present"),
        }
    }

    #[test]
    fn full_queue_drops_stalest() {
        let mut q = FrameQueue::new(2);
        q.push(frame(0, 1));
        q.push(frame(16_666_667, 2));
        q.push(frame(33_333_334, 3)); // evicts frame 1
        assert_eq!(q.len(), 2);
        assert_eq!(q.dropped(), 1);
        assert_eq!(q.next_pts_nanos(), Some(16_666_667));
    }

    #[test]
    fn deadline_skip_counts_drops() {
        let mut q = FrameQueue::new(4);
        q.push(frame(0, 1));
        q.push(frame(16_666_667, 2));
        q.push(frame(33_333_334, 3));
        // now = 100 ms: frames 1 and 2 are both >1.5 intervals late.
        let action = q.pop_present(100_000_000);
        assert!(matches!(action, QueueAction::Present(_)));
        assert_eq!(q.dropped(), 2);
        assert_eq!(q.presented(), 1);
    }

    #[test]
    fn last_frame_is_presented_late_not_dropped() {
        let mut q = FrameQueue::new(4);
        q.push(frame(0, 1));
        // A single stale frame is presented rather than dropped — dropping
        // the only frame would leave the display starved.
        let action = q.pop_present(100_000_000);
        match action {
            QueueAction::Present(f) => assert_eq!(f.metadata.frame_index, 1),
            _ => panic!("expected late frame to still present"),
        }
        assert_eq!(q.dropped(), 0);
    }

    #[test]
    fn wait_for_reports_time_to_pts() {
        let mut q = FrameQueue::new(4);
        q.push(frame(50_000_000, 1));
        match q.pop_present(10_000_000) {
            QueueAction::WaitFor { wait_nanos } => assert_eq!(wait_nanos, 40_000_000),
            _ => panic!("expected WaitFor"),
        }
    }

    #[test]
    fn drop_rate_accounting() {
        let mut q = FrameQueue::new(2);
        for i in 0..10 {
            q.push(frame(i * 16_666_667, i));
        }
        // Capacity 2 → 8 evictions on push.
        assert_eq!(q.dropped(), 8);
        assert!((q.drop_rate_pct() - 100.0).abs() < 1e-9);
    }
}
