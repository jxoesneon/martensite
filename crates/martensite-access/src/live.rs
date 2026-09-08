//! Dynamic live region change detection and platform-agnostic announcement queueing.
//!
//! This module implements the announcement pipeline for
//! [`accesskit::Live::Polite`] and [`accesskit::Live::Assertive`] live regions:
//!
//! - [`LiveRegionMonitor`] watches live region nodes for value or content
//!   changes and produces [`LiveAnnouncement`] events.
//! - `Assertive` announcements are dispatched **immediately** — they
//!   interrupt the current speech queue on every platform.
//! - `Polite` announcements are coalesced over a deterministic
//!   [`POLITE_COALESCE_MS`] (80 ms) window: rapid successive updates to the
//!   same node produce a single announcement carrying the latest message,
//!   preventing screen reader speech-queue flooding.
//! - Ready announcements are consumed through [`LiveRegionMonitor::drain`]
//!   by platform adapters, which map them onto the native notification:
//!   - Windows UI Automation: `UIA_LiveRegionChangedEventId`
//!   - macOS `NSAccessibility`: `NSAccessibilityAnnouncementRequestedNotification`
//!   - Linux AT-SPI2: `object:state-changed:showing` on live region objects
//!
//! All timing is driven by a caller-supplied millisecond timestamp
//! (`now_ms`), keeping the coalescing behaviour deterministic and testable
//! without wall-clock dependencies.

use std::collections::{HashMap, VecDeque};

use accesskit::{Live, NodeId};

/// The coalescing window, in milliseconds, applied to
/// [`Live::Polite`] announcements.
///
/// Successive polite changes to a live region within this window are
/// merged into a single announcement that is emitted once the window
/// expires. This matches the mitigation strategy for rapid signal-update
/// spam in the v0.11.0 accessibility specification.
pub const POLITE_COALESCE_MS: u64 = 80;

/// A live region announcement ready to be dispatched to a platform
/// accessibility bridge.
///
/// # Examples
///
/// ```
/// use martensite_access::live::LiveAnnouncement;
/// use accesskit::{Live, NodeId};
///
/// let ann = LiveAnnouncement::new(NodeId(7), Live::Assertive, "Alert!", 0);
/// assert_eq!(ann.node, NodeId(7));
/// assert_eq!(ann.politeness, Live::Assertive);
/// assert!(ann.is_assertive());
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveAnnouncement {
    /// The accessibility node whose live region changed.
    pub node: NodeId,
    /// The politeness level of the region.
    pub politeness: Live,
    /// The announcement payload (typically the region's new accessible
    /// value or label).
    pub message: String,
    /// Monotonically increasing sequence number, assigned in detection
    /// order. Adapters may use this to discard out-of-order deliveries.
    pub sequence: u64,
    /// Number of intermediate updates to this node that were coalesced
    /// away into this announcement. Always `0` for assertive events.
    pub coalesced: u64,
}

impl LiveAnnouncement {
    /// Creates a new announcement.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_access::live::LiveAnnouncement;
    /// use accesskit::{Live, NodeId};
    ///
    /// let ann = LiveAnnouncement::new(NodeId(1), Live::Polite, "Saved", 42);
    /// assert_eq!(ann.sequence, 42);
    /// assert_eq!(ann.coalesced, 0);
    /// ```
    #[inline]
    pub fn new(node: NodeId, politeness: Live, message: impl Into<String>, sequence: u64) -> Self {
        Self {
            node,
            politeness,
            message: message.into(),
            sequence,
            coalesced: 0,
        }
    }

    /// Returns `true` if this is an assertive (interruptive) announcement.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_access::live::LiveAnnouncement;
    /// use accesskit::{Live, NodeId};
    ///
    /// assert!(LiveAnnouncement::new(NodeId(1), Live::Assertive, "x", 0).is_assertive());
    /// assert!(!LiveAnnouncement::new(NodeId(1), Live::Polite, "x", 0).is_assertive());
    /// ```
    #[inline]
    pub fn is_assertive(&self) -> bool {
        self.politeness == Live::Assertive
    }
}

/// Internal state for a live region that currently has a pending polite
/// announcement inside the coalescing window.
#[derive(Debug, Clone)]
struct PendingPolite {
    /// The latest message observed for this node.
    message: String,
    /// Timestamp (ms) at which the current coalescing window opened.
    /// The window is **not** reset by subsequent updates — expiry is
    /// `window_start + POLITE_COALESCE_MS`, making flush timing
    /// deterministic.
    window_start_ms: u64,
    /// Sequence number assigned to the first change in this window.
    sequence: u64,
    /// How many updates were merged into this pending announcement
    /// beyond the first.
    coalesced: u64,
}

/// Monitors live region nodes and produces a platform-agnostic queue of
/// announcements.
///
/// The monitor tracks the last announced value for each live region node.
/// Call [`update`](Self::update) whenever a live region's content is
/// re-evaluated; changes are turned into [`LiveAnnouncement`] events.
/// Assertive events are pushed onto the ready queue immediately, while
/// polite events sit in the coalescing window until
/// [`POLITE_COALESCE_MS`] has elapsed since the first change.
///
/// Adapters (e.g. the `winit` bridge) call [`drain`](Self::drain) each
/// frame — typically right after building the `TreeUpdate` — and forward
/// the returned events to the platform notification for their OS.
///
/// # Examples
///
/// ```
/// use martensite_access::live::{LiveRegionMonitor, POLITE_COALESCE_MS};
/// use accesskit::{Live, NodeId};
///
/// let mut mon = LiveRegionMonitor::new();
/// let node = NodeId(10);
///
/// // Assertive: dispatched immediately.
/// mon.update(node, Live::Assertive, "Critical failure", 0);
/// assert_eq!(mon.drain(0).len(), 1);
///
/// // Polite: coalesced for POLITE_COALESCE_MS.
/// mon.update(node, Live::Polite, "Loading…", 0);
/// mon.update(node, Live::Polite, "Loading 50%", 30);
/// assert!(mon.drain(30).is_empty());
/// let ready = mon.drain(POLITE_COALESCE_MS);
/// assert_eq!(ready.len(), 1);
/// assert_eq!(ready[0].message, "Loading 50%");
/// assert_eq!(ready[0].coalesced, 1);
/// ```
#[derive(Debug, Default)]
pub struct LiveRegionMonitor {
    /// Last announced content per node, used for change detection.
    announced: HashMap<NodeId, String>,
    /// Polite announcements inside the coalescing window.
    pending: HashMap<NodeId, PendingPolite>,
    /// FIFO queue of announcements ready for platform dispatch.
    /// Assertive events land here immediately; polite events are moved
    /// here when their window expires during [`drain`](Self::drain).
    ready: VecDeque<LiveAnnouncement>,
    /// Global monotonic sequence counter.
    next_sequence: u64,
}

impl LiveRegionMonitor {
    /// Creates an empty monitor.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_access::live::LiveRegionMonitor;
    ///
    /// let mon = LiveRegionMonitor::new();
    /// assert!(mon.is_idle());
    /// ```
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records the current content of a live region and queues an
    /// announcement if it changed.
    ///
    /// - If `value` equals the last announced content for `node`, nothing
    ///   happens (returns `false`).
    /// - For [`Live::Assertive`] regions the announcement is queued for
    ///   immediate dispatch and any pending polite announcement for the
    ///   same node is superseded.
    /// - For [`Live::Polite`] regions the change opens (or joins) an
    ///   80 ms coalescing window keyed on the *first* change timestamp;
    ///   subsequent changes within the window update the pending message
    ///   without extending the window.
    ///
    /// `now_ms` is a caller-supplied millisecond timestamp (e.g. from the
    /// frame clock). Returns `true` if the change was registered.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_access::live::LiveRegionMonitor;
    /// use accesskit::{Live, NodeId};
    ///
    /// let mut mon = LiveRegionMonitor::new();
    /// assert!(mon.update(NodeId(1), Live::Polite, "a", 0));
    /// // Identical content is not re-announced.
    /// assert!(!mon.update(NodeId(1), Live::Polite, "a", 10));
    /// ```
    pub fn update(
        &mut self,
        node: NodeId,
        politeness: Live,
        value: impl Into<String>,
        now_ms: u64,
    ) -> bool {
        let value = value.into();

        // Change detection: skip if identical to last announced content,
        // unless there is a pending (not yet announced) polite update —
        // in that case the pending message is the latest observed value.
        if let Some(pending) = self.pending.get_mut(&node) {
            if pending.message == value {
                return false;
            }
        } else if self.announced.get(&node) == Some(&value) {
            return false;
        }

        match politeness {
            Live::Assertive => {
                let sequence = self.next_sequence();
                // An assertive update supersedes any pending polite
                // announcement for the same region.
                self.pending.remove(&node);
                self.announced.insert(node, value.clone());
                self.ready.push_back(LiveAnnouncement {
                    node,
                    politeness,
                    message: value,
                    sequence,
                    coalesced: 0,
                });
                true
            }
            Live::Polite | Live::Off => {
                if politeness == Live::Off {
                    // Regions toggled off announce nothing, but the value
                    // is still recorded so re-enabling does not replay.
                    self.pending.remove(&node);
                    self.announced.insert(node, value);
                    return false;
                }
                match self.pending.get_mut(&node) {
                    Some(pending) => {
                        pending.message = value;
                        pending.coalesced = pending.coalesced.saturating_add(1);
                    }
                    None => {
                        let sequence = self.next_sequence();
                        self.pending.insert(
                            node,
                            PendingPolite {
                                message: value,
                                window_start_ms: now_ms,
                                sequence,
                                coalesced: 0,
                            },
                        );
                    }
                }
                true
            }
        }
    }

    /// Feeds a built [`accesskit::Node`] into the monitor.
    ///
    /// Reads the node's `live` and `value` properties; nodes without a
    /// live region or without a value are ignored. Intended to be called
    /// by adapters for every emitted live-region node right after a
    /// `TreeUpdate` is built, so content changes surface as announcements.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_access::live::LiveRegionMonitor;
    /// use accesskit::{Live, Node, NodeId, Role};
    ///
    /// let mut node = Node::new(Role::Status);
    /// node.set_live(Live::Assertive);
    /// node.set_value("3 new messages");
    ///
    /// let mut mon = LiveRegionMonitor::new();
    /// assert!(mon.update_from_node(NodeId(5), &node, 0));
    /// assert_eq!(mon.drain(0)[0].message, "3 new messages");
    /// ```
    pub fn update_from_node(
        &mut self,
        node_id: NodeId,
        node: &accesskit::Node,
        now_ms: u64,
    ) -> bool {
        let Some(live) = node.live() else {
            return false;
        };
        match node.value().or_else(|| node.label()) {
            Some(v) => self.update(node_id, live, v, now_ms),
            None => false,
        }
    }

    /// Queues an explicit announcement regardless of change detection.
    ///
    /// Use this for regions where the widget itself decides something is
    /// worth announcing (e.g. a status bar emitting a message unrelated
    /// to its text value). Assertive events dispatch immediately; polite
    /// events join the coalescing window.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_access::live::LiveRegionMonitor;
    /// use accesskit::{Live, NodeId};
    ///
    /// let mut mon = LiveRegionMonitor::new();
    /// mon.announce(NodeId(2), Live::Assertive, "Form submitted", 0);
    /// assert_eq!(mon.drain(0)[0].message, "Form submitted");
    /// ```
    pub fn announce(
        &mut self,
        node: NodeId,
        politeness: Live,
        message: impl Into<String>,
        now_ms: u64,
    ) {
        // Force registration by clearing the dedup key.
        self.announced.remove(&node);
        self.pending.remove(&node);
        self.update(node, politeness, message, now_ms);
    }

    /// Drains all announcements that are ready for platform dispatch at
    /// `now_ms`.
    ///
    /// Expired polite windows are flushed (oldest window first, so the
    /// output order is deterministic), followed by any assertive or
    /// already-ready events in FIFO order.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_access::live::{LiveRegionMonitor, POLITE_COALESCE_MS};
    /// use accesskit::{Live, NodeId};
    ///
    /// let mut mon = LiveRegionMonitor::new();
    /// mon.update(NodeId(1), Live::Polite, "tick", 0);
    /// assert!(mon.drain(POLITE_COALESCE_MS - 1).is_empty());
    /// assert_eq!(mon.drain(POLITE_COALESCE_MS).len(), 1);
    /// ```
    pub fn drain(&mut self, now_ms: u64) -> Vec<LiveAnnouncement> {
        // Flush expired polite windows oldest first, sorting by window start
        // then by the initial sequence so truly-oldest windows are emitted
        // before newer ones.
        let mut expired: Vec<(u64, u64, NodeId)> = self
            .pending
            .iter()
            .filter(|(_, p)| now_ms.saturating_sub(p.window_start_ms) >= POLITE_COALESCE_MS)
            .map(|(id, p)| (p.window_start_ms, p.sequence, *id))
            .collect();
        expired.sort_unstable();

        let mut flushed = Vec::with_capacity(expired.len());
        for (_, _, id) in expired {
            if let Some(p) = self.pending.remove(&id) {
                self.announced.insert(id, p.message.clone());
                flushed.push(LiveAnnouncement {
                    node: id,
                    politeness: Live::Polite,
                    message: p.message,
                    sequence: p.sequence,
                    coalesced: p.coalesced,
                });
            }
        }

        // Merge flushed polite events with the ready queue, ordered by
        // sequence so detection order is preserved.
        let mut out: Vec<LiveAnnouncement> = flushed;
        while let Some(ev) = self.ready.pop_front() {
            out.push(ev);
        }
        out.sort_by_key(|a| a.sequence);
        out
    }

    /// Returns `true` if there are no pending or queued announcements.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_access::live::LiveRegionMonitor;
    /// use accesskit::{Live, NodeId};
    ///
    /// let mut mon = LiveRegionMonitor::new();
    /// assert!(mon.is_idle());
    /// mon.update(NodeId(1), Live::Polite, "x", 0);
    /// assert!(!mon.is_idle());
    /// ```
    #[inline]
    pub fn is_idle(&self) -> bool {
        self.pending.is_empty() && self.ready.is_empty()
    }

    /// Returns `true` if the node has a polite announcement still inside
    /// its coalescing window.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_access::live::LiveRegionMonitor;
    /// use accesskit::{Live, NodeId};
    ///
    /// let mut mon = LiveRegionMonitor::new();
    /// mon.update(NodeId(1), Live::Polite, "x", 0);
    /// assert!(mon.has_pending(NodeId(1)));
    /// assert!(!mon.has_pending(NodeId(2)));
    /// ```
    #[inline]
    pub fn has_pending(&self, node: NodeId) -> bool {
        self.pending.contains_key(&node)
    }

    /// Removes all tracking state for `node` (e.g. when the widget is
    /// destroyed). Any pending announcement for the node is discarded.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_access::live::LiveRegionMonitor;
    /// use accesskit::{Live, NodeId};
    ///
    /// let mut mon = LiveRegionMonitor::new();
    /// mon.update(NodeId(1), Live::Polite, "x", 0);
    /// mon.remove(NodeId(1));
    /// assert!(!mon.has_pending(NodeId(1)));
    /// ```
    pub fn remove(&mut self, node: NodeId) {
        self.pending.remove(&node);
        self.announced.remove(&node);
    }

    /// Clears all state: pending windows, queued events, and announced
    /// values.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_access::live::LiveRegionMonitor;
    /// use accesskit::{Live, NodeId};
    ///
    /// let mut mon = LiveRegionMonitor::new();
    /// mon.update(NodeId(1), Live::Assertive, "x", 0);
    /// mon.clear();
    /// assert!(mon.is_idle());
    /// ```
    pub fn clear(&mut self) {
        self.pending.clear();
        self.announced.clear();
        self.ready.clear();
    }

    /// The earliest timestamp at which a pending polite announcement will
    /// flush, or `None` if no polite events are pending. Adapters can use
    /// this to schedule a wakeup rather than polling every frame.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_access::live::{LiveRegionMonitor, POLITE_COALESCE_MS};
    /// use accesskit::{Live, NodeId};
    ///
    /// let mut mon = LiveRegionMonitor::new();
    /// assert_eq!(mon.next_flush_ms(), None);
    /// mon.update(NodeId(1), Live::Polite, "x", 100);
    /// assert_eq!(mon.next_flush_ms(), Some(100 + POLITE_COALESCE_MS));
    /// ```
    pub fn next_flush_ms(&self) -> Option<u64> {
        self.pending
            .values()
            .map(|p| p.window_start_ms.saturating_add(POLITE_COALESCE_MS))
            .min()
    }

    fn next_sequence(&mut self) -> u64 {
        let seq = self.next_sequence;
        self.next_sequence += 1;
        seq
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assertive_is_immediate() {
        let mut mon = LiveRegionMonitor::new();
        assert!(mon.update(NodeId(1), Live::Assertive, "alert", 5));
        let out = mon.drain(5);
        assert_eq!(out.len(), 1);
        assert!(out[0].is_assertive());
        assert_eq!(out[0].message, "alert");
    }

    #[test]
    fn polite_waits_for_window() {
        let mut mon = LiveRegionMonitor::new();
        mon.update(NodeId(1), Live::Polite, "a", 0);
        assert!(mon.drain(POLITE_COALESCE_MS - 1).is_empty());
        assert_eq!(mon.drain(POLITE_COALESCE_MS).len(), 1);
    }

    #[test]
    fn polite_window_is_not_extended() {
        let mut mon = LiveRegionMonitor::new();
        mon.update(NodeId(1), Live::Polite, "a", 0);
        mon.update(NodeId(1), Live::Polite, "b", 70);
        // Window opened at t=0, so it flushes at t=80 even though a
        // change landed at t=70.
        let out = mon.drain(POLITE_COALESCE_MS);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].message, "b");
        assert_eq!(out[0].coalesced, 1);
    }

    #[test]
    fn polite_coalesces_many_updates() {
        let mut mon = LiveRegionMonitor::new();
        for i in 0..10u64 {
            mon.update(NodeId(1), Live::Polite, format!("v{i}"), i * 5);
        }
        assert!(mon.drain(40).is_empty());
        let out = mon.drain(POLITE_COALESCE_MS);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].message, "v9");
        assert_eq!(out[0].coalesced, 9);
    }

    #[test]
    fn identical_value_not_reannounced() {
        let mut mon = LiveRegionMonitor::new();
        mon.update(NodeId(1), Live::Assertive, "x", 0);
        mon.drain(0);
        assert!(!mon.update(NodeId(1), Live::Assertive, "x", 10));
        assert!(mon.drain(200).is_empty());
    }

    #[test]
    fn assertive_supersedes_pending_polite() {
        let mut mon = LiveRegionMonitor::new();
        mon.update(NodeId(1), Live::Polite, "quiet", 0);
        mon.update(NodeId(1), Live::Assertive, "loud", 10);
        let out = mon.drain(10);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].politeness, Live::Assertive);
        assert!(mon.drain(500).is_empty());
    }

    #[test]
    fn off_regions_are_silent() {
        let mut mon = LiveRegionMonitor::new();
        assert!(!mon.update(NodeId(1), Live::Off, "x", 0));
        assert!(mon.drain(500).is_empty());
    }

    #[test]
    fn ordering_is_by_detection_sequence() {
        let mut mon = LiveRegionMonitor::new();
        // Polite for node 1 detected first (seq 0), assertive for node 2
        // second (seq 1). Even though the polite flushes during drain,
        // ordering follows detection sequence.
        mon.update(NodeId(1), Live::Polite, "p", 0);
        mon.update(NodeId(2), Live::Assertive, "a", 40);
        let out = mon.drain(POLITE_COALESCE_MS);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].node, NodeId(1));
        assert_eq!(out[1].node, NodeId(2));
    }

    #[test]
    fn new_window_after_flush() {
        let mut mon = LiveRegionMonitor::new();
        mon.update(NodeId(1), Live::Polite, "a", 0);
        assert_eq!(mon.drain(POLITE_COALESCE_MS).len(), 1);
        mon.update(NodeId(1), Live::Polite, "b", 200);
        assert!(mon.drain(200 + POLITE_COALESCE_MS - 1).is_empty());
        assert_eq!(mon.drain(200 + POLITE_COALESCE_MS).len(), 1);
    }

    #[test]
    fn remove_drops_pending() {
        let mut mon = LiveRegionMonitor::new();
        mon.update(NodeId(1), Live::Polite, "a", 0);
        mon.remove(NodeId(1));
        assert!(mon.drain(1000).is_empty());
    }

    #[test]
    fn next_flush_reports_earliest_window() {
        let mut mon = LiveRegionMonitor::new();
        mon.update(NodeId(1), Live::Polite, "a", 100);
        mon.update(NodeId(2), Live::Polite, "b", 50);
        assert_eq!(mon.next_flush_ms(), Some(50 + POLITE_COALESCE_MS));
    }
}
