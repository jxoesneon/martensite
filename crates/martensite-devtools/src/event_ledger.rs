//! Event ledger and dispatch observability infrastructure.
//!
//! Provides a preallocated, fixed-capacity ring buffer for recording and diagnosing
//! UI event dispatch decisions with sub-0.1ms per-frame overhead and zero heap allocation
//! in the hot dispatch loop.
//!
//! # Instrumentation Overview
//!
//! During UI event processing, dispatch paths in `martensite-window` record
//! [`EventRecord`]s detailing:
//!
//! - Which widget was hit or why hit-testing was rejected ([`HitRejection`]).
//! - The hit-test path from root to leaf ([`HitPath`]), bounded to 16 inline entries.
//! - How the event was handled ([`Disposition`]).
//! - Focus transitions between widgets ([`WidgetId`]).
//!
//! # Example
//!
//! ```
//! use martensite_core::WidgetId;
//! use martensite_devtools::event_ledger::{
//!     Disposition, EventKind, EventLedger, EventRecord, Point,
//! };
//!
//! let mut ledger = EventLedger::new();
//! let button_id = WidgetId::from_parts(42, 1);
//!
//! let mut record = EventRecord::new(1, 100, EventKind::Pointer, Disposition::Handled(button_id))
//!     .with_position(Point::new(412.0, 301.0));
//! record.hit_path.push(button_id);
//!
//! ledger.push(record);
//! assert_eq!(ledger.len(), 1);
//! assert_eq!(ledger.filter_by_kind(EventKind::Pointer).count(), 1);
//! ```

use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};

use martensite_core::WidgetId;

/// Maximum number of widget IDs stored inline in a [`HitPath`].
pub const HIT_PATH_CAPACITY: usize = 16;

/// Default capacity for the event ledger ring buffer (1024 entries ≈ 224 KB).
pub const DEFAULT_LEDGER_CAPACITY: usize = 1024;

static DEBUG_EVENTS_CACHED: AtomicBool = AtomicBool::new(false);
static DEBUG_EVENTS_INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Returns whether event debug logging to stderr is enabled via `MARTENSITE_DEBUG_EVENTS=1`.
///
/// Uses an atomic cached boolean so checking this in hot paths takes a single branch.
///
/// # Examples
///
/// ```
/// use martensite_devtools::event_ledger::is_debug_events_enabled;
///
/// let _ = is_debug_events_enabled();
/// ```
pub fn is_debug_events_enabled() -> bool {
    if !DEBUG_EVENTS_INITIALIZED.load(Ordering::Relaxed) {
        let enabled = std::env::var("MARTENSITE_DEBUG_EVENTS")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        DEBUG_EVENTS_CACHED.store(enabled, Ordering::Relaxed);
        DEBUG_EVENTS_INITIALIZED.store(true, Ordering::Relaxed);
        enabled
    } else {
        DEBUG_EVENTS_CACHED.load(Ordering::Relaxed)
    }
}

/// Allows programmatic override or test refresh of the `MARTENSITE_DEBUG_EVENTS` flag.
///
/// # Examples
///
/// ```
/// use martensite_devtools::event_ledger::{is_debug_events_enabled, set_debug_events_enabled};
///
/// set_debug_events_enabled(true);
/// assert!(is_debug_events_enabled());
/// set_debug_events_enabled(false);
/// assert!(!is_debug_events_enabled());
/// ```
pub fn set_debug_events_enabled(enabled: bool) {
    DEBUG_EVENTS_CACHED.store(enabled, Ordering::Relaxed);
    DEBUG_EVENTS_INITIALIZED.store(true, Ordering::Relaxed);
}

/// Category of an input or interaction event.
///
/// # Examples
///
/// ```
/// use martensite_devtools::event_ledger::EventKind;
///
/// let kind = EventKind::Pointer;
/// assert_eq!(format!("{kind}"), "Pointer");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum EventKind {
    /// Mouse, touch, or pen pointer event.
    #[default]
    Pointer,
    /// Keyboard key press, release, or repeat.
    Key,
    /// Mouse wheel, trackpad, or gesture scroll event.
    Scroll,
    /// Input Method Editor text composition event.
    Ime,
    /// Focus change or focus navigation event.
    Focus,
    /// Drag and drop interaction event.
    Dnd,
}

impl fmt::Display for EventKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pointer => write!(f, "Pointer"),
            Self::Key => write!(f, "Key"),
            Self::Scroll => write!(f, "Scroll"),
            Self::Ime => write!(f, "Ime"),
            Self::Focus => write!(f, "Focus"),
            Self::Dnd => write!(f, "Dnd"),
        }
    }
}

/// Reason why an event was rejected or did not reach a target widget during dispatch.
///
/// # Examples
///
/// ```
/// use martensite_core::WidgetId;
/// use martensite_devtools::event_ledger::HitRejection;
///
/// let id = WidgetId::from_parts(1, 1);
/// let rejection = HitRejection::OccludedBy(id);
/// assert_eq!(rejection.occluding_widget(), Some(id));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HitRejection {
    /// The event coordinates fell outside the widget or viewport bounds.
    OutsideBounds,
    /// The widget was occluded by another widget situated higher in the z-order.
    OccludedBy(WidgetId),
    /// The widget or ancestor has hit-testing explicitly disabled.
    HitTestDisabled,
    /// The widget is beneath an active modal overlay and not reachable.
    UnderModal,
    /// The event was captured by another widget with active pointer or key capture.
    CapturedByOther(WidgetId),
}

impl HitRejection {
    /// Returns the widget that caused the rejection, if applicable.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::WidgetId;
    /// use martensite_devtools::event_ledger::HitRejection;
    ///
    /// let id = WidgetId::from_parts(5, 1);
    /// assert_eq!(HitRejection::OccludedBy(id).occluding_widget(), Some(id));
    /// assert_eq!(HitRejection::OutsideBounds.occluding_widget(), None);
    /// ```
    pub const fn occluding_widget(&self) -> Option<WidgetId> {
        match *self {
            Self::OccludedBy(id) | Self::CapturedByOther(id) => Some(id),
            Self::OutsideBounds | Self::HitTestDisabled | Self::UnderModal => None,
        }
    }
}

impl fmt::Display for HitRejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutsideBounds => write!(f, "OutsideBounds"),
            Self::OccludedBy(id) => write!(f, "OccludedBy({}:{})", id.slot_idx(), id.generation()),
            Self::HitTestDisabled => write!(f, "HitTestDisabled"),
            Self::UnderModal => write!(f, "UnderModal"),
            Self::CapturedByOther(id) => {
                write!(f, "CapturedByOther({}:{})", id.slot_idx(), id.generation())
            }
        }
    }
}

/// The final routing outcome of an event.
///
/// # Examples
///
/// ```
/// use martensite_core::WidgetId;
/// use martensite_devtools::event_ledger::Disposition;
///
/// let id = WidgetId::from_parts(10, 1);
/// let disp = Disposition::Handled(id);
/// assert_eq!(disp.target_widget(), Some(id));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Disposition {
    /// The event was successfully handled by the specified widget.
    Handled(WidgetId),
    /// The event was ignored by all candidate widgets in the hierarchy.
    #[default]
    Ignored,
    /// The event was not consumed by the initial target and bubbled to an ancestor.
    BubbledTo(WidgetId),
    /// The event was intercepted by an active pointer, key, or scroll capture.
    Captured(WidgetId),
}

impl Disposition {
    /// Returns the target widget ID if this disposition references a specific widget.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::WidgetId;
    /// use martensite_devtools::event_ledger::Disposition;
    ///
    /// let id = WidgetId::from_parts(12, 1);
    /// assert_eq!(Disposition::Handled(id).target_widget(), Some(id));
    /// assert_eq!(Disposition::Ignored.target_widget(), None);
    /// ```
    pub const fn target_widget(&self) -> Option<WidgetId> {
        match *self {
            Self::Handled(id) | Self::BubbledTo(id) | Self::Captured(id) => Some(id),
            Self::Ignored => None,
        }
    }
}

impl fmt::Display for Disposition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Handled(id) => write!(f, "Handled({}:{})", id.slot_idx(), id.generation()),
            Self::Ignored => write!(f, "Ignored"),
            Self::BubbledTo(id) => write!(f, "BubbledTo({}:{})", id.slot_idx(), id.generation()),
            Self::Captured(id) => write!(f, "Captured({}:{})", id.slot_idx(), id.generation()),
        }
    }
}

/// A 2D point in logical pixel coordinates.
///
/// # Examples
///
/// ```
/// use martensite_devtools::event_ledger::Point;
///
/// let pt = Point::new(10.0, 20.0);
/// assert_eq!(pt.x, 10.0);
/// assert_eq!(pt.y, 20.0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Point {
    /// Horizontal coordinate in logical pixels.
    pub x: f32,
    /// Vertical coordinate in logical pixels.
    pub y: f32,
}

impl Point {
    /// Creates a new point.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::event_ledger::Point;
    ///
    /// let pt = Point::new(5.0, 10.0);
    /// assert_eq!(pt.x, 5.0);
    /// assert_eq!(pt.y, 10.0);
    /// ```
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

impl From<glam::Vec2> for Point {
    fn from(v: glam::Vec2) -> Self {
        Self { x: v.x, y: v.y }
    }
}

impl From<Point> for glam::Vec2 {
    fn from(p: Point) -> Self {
        glam::Vec2::new(p.x, p.y)
    }
}

impl From<(f32, f32)> for Point {
    fn from((x, y): (f32, f32)) -> Self {
        Self { x, y }
    }
}

impl From<[f32; 2]> for Point {
    fn from([x, y]: [f32; 2]) -> Self {
        Self { x, y }
    }
}

impl fmt::Display for Point {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{},{}", format_coord(self.x), format_coord(self.y))
    }
}

/// Bounded inline storage for widget hit-test paths.
///
/// Stores up to [`HIT_PATH_CAPACITY`] (16) [`WidgetId`]s without heap allocation.
/// If more than 16 widgets are traversed in a hit-test path, excess widgets are
/// discarded and the `truncated` flag is set to `true`.
///
/// # Examples
///
/// ```
/// use martensite_core::WidgetId;
/// use martensite_devtools::event_ledger::HitPath;
///
/// let mut path = HitPath::new();
/// let id1 = WidgetId::from_parts(1, 1);
/// let id2 = WidgetId::from_parts(2, 1);
/// path.push(id1);
/// path.push(id2);
///
/// assert_eq!(path.len(), 2);
/// assert_eq!(path.first(), Some(id1));
/// assert_eq!(path.last(), Some(id2));
/// assert!(!path.is_truncated());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct HitPath {
    ids: [Option<WidgetId>; HIT_PATH_CAPACITY],
    len: u8,
    truncated: bool,
}

impl HitPath {
    /// Creates an empty hit path.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::event_ledger::HitPath;
    ///
    /// let path = HitPath::new();
    /// assert!(path.is_empty());
    /// ```
    pub const fn new() -> Self {
        Self {
            ids: [None; HIT_PATH_CAPACITY],
            len: 0,
            truncated: false,
        }
    }

    /// Appends a widget ID to the hit path.
    ///
    /// If the path has reached [`HIT_PATH_CAPACITY`] (16), subsequent pushes
    /// are ignored and [`is_truncated`](Self::is_truncated) becomes `true`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::WidgetId;
    /// use martensite_devtools::event_ledger::HitPath;
    ///
    /// let mut path = HitPath::new();
    /// path.push(WidgetId::from_parts(10, 1));
    /// assert_eq!(path.len(), 1);
    /// ```
    pub fn push(&mut self, id: WidgetId) {
        let idx = self.len as usize;
        if idx < HIT_PATH_CAPACITY {
            self.ids[idx] = Some(id);
            self.len += 1;
        } else {
            self.truncated = true;
        }
    }

    /// Returns the number of widgets recorded in this path (up to 16).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::event_ledger::HitPath;
    ///
    /// let path = HitPath::new();
    /// assert_eq!(path.len(), 0);
    /// ```
    pub const fn len(&self) -> usize {
        self.len as usize
    }

    /// Returns `true` if no widgets have been recorded.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::event_ledger::HitPath;
    ///
    /// let path = HitPath::new();
    /// assert!(path.is_empty());
    /// ```
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Returns `true` if more than 16 widgets were traversed and the path was capped.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::WidgetId;
    /// use martensite_devtools::event_ledger::HitPath;
    ///
    /// let mut path = HitPath::new();
    /// for i in 1..=20 {
    ///     path.push(WidgetId::from_parts(i, 1));
    /// }
    /// assert!(path.is_truncated());
    /// assert_eq!(path.len(), 16);
    /// ```
    pub const fn is_truncated(&self) -> bool {
        self.truncated
    }

    /// Returns `true` if this hit path contains the specified widget ID.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::WidgetId;
    /// use martensite_devtools::event_ledger::HitPath;
    ///
    /// let id = WidgetId::from_parts(5, 1);
    /// let mut path = HitPath::new();
    /// path.push(id);
    /// assert!(path.contains(id));
    /// ```
    pub fn contains(&self, id: WidgetId) -> bool {
        self.ids[..self.len as usize].contains(&Some(id))
    }

    /// Returns the first widget in the path (the root or topmost candidate).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::WidgetId;
    /// use martensite_devtools::event_ledger::HitPath;
    ///
    /// let mut path = HitPath::new();
    /// let id = WidgetId::from_parts(1, 1);
    /// path.push(id);
    /// assert_eq!(path.first(), Some(id));
    /// ```
    pub fn first(&self) -> Option<WidgetId> {
        if self.len > 0 {
            self.ids[0]
        } else {
            None
        }
    }

    /// Returns the last widget in the path (the leaf target).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::WidgetId;
    /// use martensite_devtools::event_ledger::HitPath;
    ///
    /// let mut path = HitPath::new();
    /// let id1 = WidgetId::from_parts(1, 1);
    /// let id2 = WidgetId::from_parts(2, 1);
    /// path.push(id1);
    /// path.push(id2);
    /// assert_eq!(path.last(), Some(id2));
    /// ```
    pub fn last(&self) -> Option<WidgetId> {
        if self.len > 0 {
            self.ids[(self.len - 1) as usize]
        } else {
            None
        }
    }

    /// Returns the widget at the given index in the hit path.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::WidgetId;
    /// use martensite_devtools::event_ledger::HitPath;
    ///
    /// let id = WidgetId::from_parts(7, 1);
    /// let mut path = HitPath::new();
    /// path.push(id);
    /// assert_eq!(path.get(0), Some(id));
    /// assert_eq!(path.get(1), None);
    /// ```
    pub fn get(&self, index: usize) -> Option<WidgetId> {
        if index < self.len as usize {
            self.ids[index]
        } else {
            None
        }
    }

    /// Returns an iterator over the widget IDs in traversal order.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::WidgetId;
    /// use martensite_devtools::event_ledger::HitPath;
    ///
    /// let mut path = HitPath::new();
    /// path.push(WidgetId::from_parts(1, 1));
    /// path.push(WidgetId::from_parts(2, 1));
    /// let count = path.iter().count();
    /// assert_eq!(count, 2);
    /// ```
    pub fn iter(&self) -> HitPathIter<'_> {
        HitPathIter {
            ids: &self.ids,
            front: 0,
            back: self.len as usize,
        }
    }

    /// Converts the inline hit path into an allocated [`Vec<WidgetId>`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::WidgetId;
    /// use martensite_devtools::event_ledger::HitPath;
    ///
    /// let mut path = HitPath::new();
    /// path.push(WidgetId::from_parts(1, 1));
    /// assert_eq!(path.to_vec(), vec![WidgetId::from_parts(1, 1)]);
    /// ```
    pub fn to_vec(&self) -> Vec<WidgetId> {
        self.iter().collect()
    }
}

impl From<&[WidgetId]> for HitPath {
    fn from(slice: &[WidgetId]) -> Self {
        let mut path = Self::new();
        for &id in slice {
            path.push(id);
        }
        path
    }
}

impl<const N: usize> From<[WidgetId; N]> for HitPath {
    fn from(arr: [WidgetId; N]) -> Self {
        let mut path = Self::new();
        for id in arr {
            path.push(id);
        }
        path
    }
}

impl FromIterator<WidgetId> for HitPath {
    fn from_iter<T: IntoIterator<Item = WidgetId>>(iter: T) -> Self {
        let mut path = Self::new();
        for id in iter {
            path.push(id);
        }
        path
    }
}

impl std::ops::Index<usize> for HitPath {
    type Output = WidgetId;

    fn index(&self, index: usize) -> &Self::Output {
        if index < self.len as usize {
            self.ids[index].as_ref().expect("valid index")
        } else {
            panic!(
                "index out of bounds: the len is {} but the index is {}",
                self.len, index
            );
        }
    }
}

impl fmt::Display for HitPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, id) in self.iter().enumerate() {
            if i > 0 {
                write!(f, "/")?;
            }
            write!(f, "{}:{}", id.slot_idx(), id.generation())?;
        }
        if self.is_truncated() {
            write!(f, "/...truncated")?;
        }
        Ok(())
    }
}

/// Iterator over widget IDs in a [`HitPath`].
///
/// Supports exact sizing and double-ended traversal.
///
/// # Examples
///
/// ```
/// use martensite_core::WidgetId;
/// use martensite_devtools::event_ledger::HitPath;
///
/// let mut path = HitPath::new();
/// path.push(WidgetId::from_parts(1, 1));
/// let mut iter = path.iter();
/// assert_eq!(iter.len(), 1);
/// assert!(iter.next().is_some());
/// ```
#[derive(Debug, Clone)]
pub struct HitPathIter<'a> {
    ids: &'a [Option<WidgetId>; HIT_PATH_CAPACITY],
    front: usize,
    back: usize,
}

impl<'a> Iterator for HitPathIter<'a> {
    type Item = WidgetId;

    fn next(&mut self) -> Option<Self::Item> {
        if self.front < self.back {
            let item = self.ids[self.front];
            self.front += 1;
            item
        } else {
            None
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.back.saturating_sub(self.front);
        (remaining, Some(remaining))
    }
}

impl<'a> DoubleEndedIterator for HitPathIter<'a> {
    fn next_back(&mut self) -> Option<Self::Item> {
        if self.front < self.back {
            self.back -= 1;
            self.ids[self.back]
        } else {
            None
        }
    }
}

impl<'a> ExactSizeIterator for HitPathIter<'a> {}

/// A single entry in the per-frame event ledger. Fixed-size, [`Copy`], zero-allocation.
///
/// # Examples
///
/// ```
/// use martensite_core::WidgetId;
/// use martensite_devtools::event_ledger::{Disposition, EventKind, EventRecord, Point};
///
/// let id = WidgetId::from_parts(42, 1);
/// let record = EventRecord::new(1, 10, EventKind::Pointer, Disposition::Handled(id))
///     .with_position(Point::new(412.0, 301.0));
///
/// assert_eq!(record.seq, 1);
/// assert_eq!(record.frame, 10);
/// assert_eq!(record.kind, EventKind::Pointer);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct EventRecord {
    /// Monotonically increasing event sequence number.
    pub seq: u64,
    /// Frame number in which this event was dispatched.
    pub frame: u64,
    /// Category of the event.
    pub kind: EventKind,
    /// Pointer or interaction position in logical pixels, if applicable.
    pub position: Option<Point>,
    /// Traversed widget path from root to target, bounded to 16 inline entries.
    pub hit_path: HitPath,
    /// Rejection reason if hit-testing failed or was rejected.
    pub hit_rejection: Option<HitRejection>,
    /// Final routing disposition.
    pub disposition: Disposition,
    /// Prior focused widget if focus changed.
    pub focus_from: Option<WidgetId>,
    /// Newly focused widget if focus changed.
    pub focus_to: Option<WidgetId>,
    /// Nanosecond timestamp of event dispatch.
    pub timestamp: u64,
}

impl EventRecord {
    /// Creates a new minimal event record with default empty fields.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::event_ledger::{Disposition, EventKind, EventRecord};
    ///
    /// let record = EventRecord::new(1, 100, EventKind::Key, Disposition::Ignored);
    /// assert_eq!(record.seq, 1);
    /// assert_eq!(record.frame, 100);
    /// ```
    pub fn new(seq: u64, frame: u64, kind: EventKind, disposition: Disposition) -> Self {
        Self {
            seq,
            frame,
            kind,
            position: None,
            hit_path: HitPath::new(),
            hit_rejection: None,
            disposition,
            focus_from: None,
            focus_to: None,
            timestamp: 0,
        }
    }

    /// Sets the interaction position.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::event_ledger::{Disposition, EventKind, EventRecord, Point};
    ///
    /// let record = EventRecord::new(1, 1, EventKind::Pointer, Disposition::Ignored)
    ///     .with_position(Point::new(10.0, 20.0));
    /// assert!(record.position.is_some());
    /// ```
    pub fn with_position(mut self, pos: impl Into<Point>) -> Self {
        self.position = Some(pos.into());
        self
    }

    /// Sets the hit path.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::WidgetId;
    /// use martensite_devtools::event_ledger::{Disposition, EventKind, EventRecord, HitPath};
    ///
    /// let mut path = HitPath::new();
    /// path.push(WidgetId::from_parts(1, 1));
    /// let record = EventRecord::new(1, 1, EventKind::Pointer, Disposition::Ignored)
    ///     .with_hit_path(path);
    /// assert_eq!(record.hit_path.len(), 1);
    /// ```
    pub fn with_hit_path(mut self, path: impl Into<HitPath>) -> Self {
        self.hit_path = path.into();
        self
    }

    /// Sets the hit rejection reason.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::event_ledger::{Disposition, EventKind, EventRecord, HitRejection};
    ///
    /// let record = EventRecord::new(1, 1, EventKind::Pointer, Disposition::Ignored)
    ///     .with_hit_rejection(HitRejection::OutsideBounds);
    /// assert_eq!(record.hit_rejection, Some(HitRejection::OutsideBounds));
    /// ```
    pub fn with_hit_rejection(mut self, rejection: HitRejection) -> Self {
        self.hit_rejection = Some(rejection);
        self
    }

    /// Sets focus transition from one widget to another.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::WidgetId;
    /// use martensite_devtools::event_ledger::{Disposition, EventKind, EventRecord};
    ///
    /// let from = WidgetId::from_parts(1, 1);
    /// let to = WidgetId::from_parts(2, 1);
    /// let record = EventRecord::new(1, 1, EventKind::Focus, Disposition::Ignored)
    ///     .with_focus(Some(from), Some(to));
    /// assert_eq!(record.focus_from, Some(from));
    /// assert_eq!(record.focus_to, Some(to));
    /// ```
    pub fn with_focus(mut self, from: Option<WidgetId>, to: Option<WidgetId>) -> Self {
        self.focus_from = from;
        self.focus_to = to;
        self
    }

    /// Sets the nanosecond timestamp.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::event_ledger::{Disposition, EventKind, EventRecord};
    ///
    /// let record = EventRecord::new(1, 1, EventKind::Key, Disposition::Ignored)
    ///     .with_timestamp(1_000_000);
    /// assert_eq!(record.timestamp, 1_000_000);
    /// ```
    pub fn with_timestamp(mut self, timestamp_ns: u64) -> Self {
        self.timestamp = timestamp_ns;
        self
    }

    /// Returns `true` if this event record mentions the given widget ID anywhere
    /// (in its hit path, disposition, hit rejection, or focus transitions).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::WidgetId;
    /// use martensite_devtools::event_ledger::{Disposition, EventKind, EventRecord};
    ///
    /// let target = WidgetId::from_parts(42, 1);
    /// let record = EventRecord::new(1, 1, EventKind::Pointer, Disposition::Handled(target));
    /// assert!(record.mentions_widget(target));
    /// ```
    pub fn mentions_widget(&self, widget_id: WidgetId) -> bool {
        self.hit_path.contains(widget_id)
            || self.disposition.target_widget() == Some(widget_id)
            || self.hit_rejection.and_then(|r| r.occluding_widget()) == Some(widget_id)
            || self.focus_from == Some(widget_id)
            || self.focus_to == Some(widget_id)
    }

    /// Formats this record as a compact one-line diagnostic string.
    ///
    /// Suitable for `MARTENSITE_DEBUG_EVENTS=1` output, logging, and grep filtering.
    /// Formats widget IDs as `{slot}:{generation}`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::WidgetId;
    /// use martensite_devtools::event_ledger::{Disposition, EventKind, EventRecord, Point};
    ///
    /// let id = WidgetId::from_parts(42, 1);
    /// let mut record = EventRecord::new(1, 1, EventKind::Pointer, Disposition::Handled(id))
    ///     .with_position(Point::new(412.0, 301.0));
    /// record.hit_path.push(id);
    ///
    /// assert_eq!(record.format_diagnostic(), "ptr@ 412,301 → hit[42:1] handled");
    /// ```
    pub fn format_diagnostic(&self) -> String {
        self.format_diagnostic_with(|id| Some(format!("{}:{}", id.slot_idx(), id.generation())))
    }

    /// Formats this record as a one-line diagnostic string using a custom widget name resolver.
    ///
    /// If the resolver returns `None` for a widget ID, it falls back to `{slot}:{generation}`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::WidgetId;
    /// use martensite_devtools::event_ledger::{Disposition, EventKind, EventRecord, Point};
    ///
    /// let id = WidgetId::from_parts(42, 1);
    /// let mut record = EventRecord::new(1, 1, EventKind::Pointer, Disposition::Handled(id))
    ///     .with_position(Point::new(412.0, 301.0));
    /// record.hit_path.push(id);
    ///
    /// let formatted = record.format_diagnostic_with(|_| Some("App/ZonePanel/Grid/Cell(r42,c7)".to_string()));
    /// assert_eq!(formatted, "ptr@ 412,301 → hit[App/ZonePanel/Grid/Cell(r42,c7)] handled");
    /// ```
    pub fn format_diagnostic_with<F>(&self, mut name_of: F) -> String
    where
        F: FnMut(WidgetId) -> Option<String>,
    {
        let mut s = String::with_capacity(64);

        // 1. Kind and position
        match self.kind {
            EventKind::Pointer => {
                s.push_str("ptr");
                if let Some(pos) = self.position {
                    s.push_str("@ ");
                    s.push_str(&format_coord(pos.x));
                    s.push(',');
                    s.push_str(&format_coord(pos.y));
                }
            }
            EventKind::Key => s.push_str("key"),
            EventKind::Scroll => {
                s.push_str("scroll");
                if let Some(pos) = self.position {
                    s.push_str("@ ");
                    s.push_str(&format_coord(pos.x));
                    s.push(',');
                    s.push_str(&format_coord(pos.y));
                }
            }
            EventKind::Ime => s.push_str("ime"),
            EventKind::Focus => s.push_str("focus"),
            EventKind::Dnd => {
                s.push_str("dnd");
                if let Some(pos) = self.position {
                    s.push_str("@ ");
                    s.push_str(&format_coord(pos.x));
                    s.push(',');
                    s.push_str(&format_coord(pos.y));
                }
            }
        }

        s.push_str(" → ");

        // 2. Focus transition if present
        if self.focus_from.is_some() || self.focus_to.is_some() {
            let from_str = self.focus_from.and_then(&mut name_of).unwrap_or_else(|| {
                self.focus_from.map_or_else(
                    || "-".to_string(),
                    |id| format!("{}:{}", id.slot_idx(), id.generation()),
                )
            });
            let to_str = self.focus_to.and_then(&mut name_of).unwrap_or_else(|| {
                self.focus_to.map_or_else(
                    || "-".to_string(),
                    |id| format!("{}:{}", id.slot_idx(), id.generation()),
                )
            });
            s.push_str(&format!("[{} → {}] ", from_str, to_str));
        }

        // 3. Hit path if not empty
        if !self.hit_path.is_empty() {
            s.push_str("hit[");
            for (i, id) in self.hit_path.iter().enumerate() {
                if i > 0 {
                    s.push('/');
                }
                let name =
                    name_of(id).unwrap_or_else(|| format!("{}:{}", id.slot_idx(), id.generation()));
                s.push_str(&name);
            }
            if self.hit_path.is_truncated() {
                s.push_str("/...truncated");
            }
            s.push_str("] ");
        }

        // 4. Hit rejection if present
        if let Some(rejection) = self.hit_rejection {
            match rejection {
                HitRejection::OutsideBounds => s.push_str("rejected:OutsideBounds "),
                HitRejection::OccludedBy(id) => {
                    let name = name_of(id)
                        .unwrap_or_else(|| format!("{}:{}", id.slot_idx(), id.generation()));
                    s.push_str(&format!("rejected:OccludedBy({}) ", name));
                }
                HitRejection::HitTestDisabled => s.push_str("rejected:HitTestDisabled "),
                HitRejection::UnderModal => s.push_str("rejected:UnderModal "),
                HitRejection::CapturedByOther(id) => {
                    let name = name_of(id)
                        .unwrap_or_else(|| format!("{}:{}", id.slot_idx(), id.generation()));
                    s.push_str(&format!("rejected:CapturedByOther({}) ", name));
                }
            }
        }

        // 5. Disposition
        match self.disposition {
            Disposition::Handled(id) => {
                if self.hit_path.last() == Some(id) {
                    s.push_str("handled");
                } else {
                    let name = name_of(id)
                        .unwrap_or_else(|| format!("{}:{}", id.slot_idx(), id.generation()));
                    s.push_str(&format!("handled({})", name));
                }
            }
            Disposition::Ignored => s.push_str("ignored"),
            Disposition::BubbledTo(id) => {
                let name =
                    name_of(id).unwrap_or_else(|| format!("{}:{}", id.slot_idx(), id.generation()));
                s.push_str(&format!("bubbled_to({})", name));
            }
            Disposition::Captured(id) => {
                let name =
                    name_of(id).unwrap_or_else(|| format!("{}:{}", id.slot_idx(), id.generation()));
                s.push_str(&format!("captured({})", name));
            }
        }

        s
    }

    /// Emits this record to stderr if `MARTENSITE_DEBUG_EVENTS=1` is set in the environment.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::event_ledger::{Disposition, EventKind, EventRecord};
    ///
    /// let record = EventRecord::new(1, 1, EventKind::Key, Disposition::Ignored);
    /// record.log_if_debug_enabled();
    /// ```
    pub fn log_if_debug_enabled(&self) {
        if is_debug_events_enabled() {
            eprintln!("{}", self.format_diagnostic());
        }
    }
}

impl fmt::Display for EventRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.format_diagnostic())
    }
}

fn format_coord(v: f32) -> String {
    if v.fract() == 0.0 {
        format!("{:.0}", v)
    } else {
        format!("{:.1}", v)
    }
}

/// Fixed-capacity, zero-allocation circular ring buffer for event dispatch observability.
///
/// Preallocates a contiguous buffer of [`EventRecord`]s upon creation. Pushing new
/// records into the ledger performs zero heap allocation, overwriting the oldest entries
/// once capacity is reached.
///
/// # Examples
///
/// ```
/// use martensite_core::WidgetId;
/// use martensite_devtools::event_ledger::{
///     Disposition, EventKind, EventLedger, EventRecord,
/// };
///
/// let mut ledger = EventLedger::with_capacity(4);
/// for i in 1..=6 {
///     let id = WidgetId::from_parts(i, 1);
///     ledger.push(EventRecord::new(i as u64, 1, EventKind::Pointer, Disposition::Handled(id)));
/// }
///
/// // Capacity is 4, so only the last 4 records (3, 4, 5, 6) remain.
/// assert_eq!(ledger.len(), 4);
/// assert_eq!(ledger.oldest().unwrap().seq, 3);
/// assert_eq!(ledger.newest().unwrap().seq, 6);
/// ```
#[derive(Debug, Clone)]
pub struct EventLedger {
    buffer: Box<[EventRecord]>,
    capacity: usize,
    head: usize,
    len: usize,
    seq_counter: u64,
}

impl Default for EventLedger {
    fn default() -> Self {
        Self::new()
    }
}

impl EventLedger {
    /// Creates a new event ledger with default capacity ([`DEFAULT_LEDGER_CAPACITY`], 1024 entries).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::event_ledger::{DEFAULT_LEDGER_CAPACITY, EventLedger};
    ///
    /// let ledger = EventLedger::new();
    /// assert_eq!(ledger.capacity(), DEFAULT_LEDGER_CAPACITY);
    /// assert!(ledger.is_empty());
    /// ```
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_LEDGER_CAPACITY)
    }

    /// Creates an event ledger with a custom fixed capacity.
    ///
    /// Clamps capacity to at least 1. Preallocates the full storage buffer immediately.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::event_ledger::EventLedger;
    ///
    /// let ledger = EventLedger::with_capacity(256);
    /// assert_eq!(ledger.capacity(), 256);
    /// ```
    pub fn with_capacity(capacity: usize) -> Self {
        let capacity = capacity.max(1);
        Self {
            buffer: vec![EventRecord::default(); capacity].into_boxed_slice(),
            capacity,
            head: 0,
            len: 0,
            seq_counter: 0,
        }
    }

    /// Returns the maximum capacity of the ring buffer.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::event_ledger::EventLedger;
    ///
    /// let ledger = EventLedger::with_capacity(128);
    /// assert_eq!(ledger.capacity(), 128);
    /// ```
    pub const fn capacity(&self) -> usize {
        self.capacity
    }

    /// Returns the current number of events stored in the ledger.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::event_ledger::{Disposition, EventKind, EventLedger, EventRecord};
    ///
    /// let mut ledger = EventLedger::new();
    /// assert_eq!(ledger.len(), 0);
    /// ledger.push(EventRecord::new(1, 1, EventKind::Key, Disposition::Ignored));
    /// assert_eq!(ledger.len(), 1);
    /// ```
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Returns `true` if the ledger contains no events.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::event_ledger::EventLedger;
    ///
    /// let ledger = EventLedger::new();
    /// assert!(ledger.is_empty());
    /// ```
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Clears all recorded events from the ledger while preserving preallocated storage.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::event_ledger::{Disposition, EventKind, EventLedger, EventRecord};
    ///
    /// let mut ledger = EventLedger::new();
    /// ledger.push(EventRecord::new(1, 1, EventKind::Key, Disposition::Ignored));
    /// ledger.clear();
    /// assert!(ledger.is_empty());
    /// ```
    pub fn clear(&mut self) {
        self.head = 0;
        self.len = 0;
    }

    /// Pushes an event record into the ring buffer.
    ///
    /// Zero heap allocation. If `record.seq == 0`, a monotonic sequence number
    /// is automatically assigned. Emits to stderr if `MARTENSITE_DEBUG_EVENTS=1`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::event_ledger::{Disposition, EventKind, EventLedger, EventRecord};
    ///
    /// let mut ledger = EventLedger::new();
    /// ledger.push(EventRecord::new(0, 1, EventKind::Key, Disposition::Ignored));
    /// assert_eq!(ledger.len(), 1);
    /// assert_eq!(ledger.newest().unwrap().seq, 1);
    /// ```
    pub fn push(&mut self, mut record: EventRecord) {
        if record.seq == 0 {
            self.seq_counter += 1;
            record.seq = self.seq_counter;
        } else if record.seq > self.seq_counter {
            self.seq_counter = record.seq;
        }

        record.log_if_debug_enabled();

        self.buffer[self.head] = record;
        self.head = (self.head + 1) % self.capacity;
        if self.len < self.capacity {
            self.len += 1;
        }
    }

    /// Returns the record at the given logical chronological index (0 = oldest, len - 1 = newest).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::event_ledger::{Disposition, EventKind, EventLedger, EventRecord};
    ///
    /// let mut ledger = EventLedger::with_capacity(3);
    /// ledger.push(EventRecord::new(10, 1, EventKind::Key, Disposition::Ignored));
    /// ledger.push(EventRecord::new(20, 1, EventKind::Key, Disposition::Ignored));
    /// assert_eq!(ledger.get(0).unwrap().seq, 10);
    /// assert_eq!(ledger.get(1).unwrap().seq, 20);
    /// assert_eq!(ledger.get(2), None);
    /// ```
    pub fn get(&self, index: usize) -> Option<&EventRecord> {
        if index >= self.len {
            None
        } else {
            let start = if self.len < self.capacity {
                0
            } else {
                self.head
            };
            let physical = (start + index) % self.capacity;
            Some(&self.buffer[physical])
        }
    }

    /// Returns the oldest recorded event in the ledger.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::event_ledger::{Disposition, EventKind, EventLedger, EventRecord};
    ///
    /// let mut ledger = EventLedger::with_capacity(2);
    /// ledger.push(EventRecord::new(1, 1, EventKind::Key, Disposition::Ignored));
    /// ledger.push(EventRecord::new(2, 1, EventKind::Key, Disposition::Ignored));
    /// ledger.push(EventRecord::new(3, 1, EventKind::Key, Disposition::Ignored));
    /// assert_eq!(ledger.oldest().unwrap().seq, 2);
    /// ```
    pub fn oldest(&self) -> Option<&EventRecord> {
        self.get(0)
    }

    /// Returns the newest recorded event in the ledger.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::event_ledger::{Disposition, EventKind, EventLedger, EventRecord};
    ///
    /// let mut ledger = EventLedger::new();
    /// ledger.push(EventRecord::new(1, 1, EventKind::Key, Disposition::Ignored));
    /// ledger.push(EventRecord::new(2, 1, EventKind::Key, Disposition::Ignored));
    /// assert_eq!(ledger.newest().unwrap().seq, 2);
    /// ```
    pub fn newest(&self) -> Option<&EventRecord> {
        if self.len > 0 {
            self.get(self.len - 1)
        } else {
            None
        }
    }

    /// Returns an iterator over all recorded events in chronological order (oldest to newest).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::event_ledger::{Disposition, EventKind, EventLedger, EventRecord};
    ///
    /// let mut ledger = EventLedger::with_capacity(3);
    /// for i in 1..=4 {
    ///     ledger.push(EventRecord::new(i, 1, EventKind::Key, Disposition::Ignored));
    /// }
    /// let seqs: Vec<u64> = ledger.iter().map(|r| r.seq).collect();
    /// assert_eq!(seqs, vec![2, 3, 4]);
    /// ```
    pub fn iter(&self) -> EventLedgerIter<'_> {
        EventLedgerIter {
            ledger: self,
            front: 0,
            back: self.len,
        }
    }

    /// Drains all events from the ledger in chronological order.
    ///
    /// Preserves the underlying preallocated storage for subsequent zero-allocation pushes.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::event_ledger::{Disposition, EventKind, EventLedger, EventRecord};
    ///
    /// let mut ledger = EventLedger::with_capacity(3);
    /// ledger.push(EventRecord::new(1, 1, EventKind::Key, Disposition::Ignored));
    /// ledger.push(EventRecord::new(2, 1, EventKind::Key, Disposition::Ignored));
    ///
    /// let drained: Vec<u64> = ledger.drain().map(|r| r.seq).collect();
    /// assert_eq!(drained, vec![1, 2]);
    /// assert!(ledger.is_empty());
    /// ```
    pub fn drain(&mut self) -> Drain<'_> {
        let total = self.len;
        Drain {
            ledger: self,
            index: 0,
            total,
        }
    }

    /// Returns an iterator over records matching an arbitrary predicate in chronological order.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::event_ledger::{Disposition, EventKind, EventLedger, EventRecord};
    ///
    /// let mut ledger = EventLedger::new();
    /// ledger.push(EventRecord::new(1, 10, EventKind::Pointer, Disposition::Ignored));
    /// ledger.push(EventRecord::new(2, 20, EventKind::Key, Disposition::Ignored));
    ///
    /// let matches: Vec<u64> = ledger.query(|r| r.frame > 15).map(|r| r.seq).collect();
    /// assert_eq!(matches, vec![2]);
    /// ```
    pub fn query<P>(&self, mut predicate: P) -> impl Iterator<Item = &EventRecord>
    where
        P: FnMut(&EventRecord) -> bool,
    {
        self.iter().filter(move |record| predicate(record))
    }

    /// Returns an iterator over all records that mention the specified widget.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::WidgetId;
    /// use martensite_devtools::event_ledger::{
    ///     Disposition, EventKind, EventLedger, EventRecord,
    /// };
    ///
    /// let target = WidgetId::from_parts(42, 1);
    /// let other = WidgetId::from_parts(99, 1);
    /// let mut ledger = EventLedger::new();
    /// ledger.push(EventRecord::new(1, 1, EventKind::Pointer, Disposition::Handled(target)));
    /// ledger.push(EventRecord::new(2, 1, EventKind::Pointer, Disposition::Handled(other)));
    ///
    /// assert_eq!(ledger.filter_by_widget(target).count(), 1);
    /// ```
    pub fn filter_by_widget(&self, widget_id: WidgetId) -> impl Iterator<Item = &EventRecord> {
        self.iter()
            .filter(move |record| record.mentions_widget(widget_id))
    }

    /// Returns an iterator over all records matching the specified event kind.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::event_ledger::{Disposition, EventKind, EventLedger, EventRecord};
    ///
    /// let mut ledger = EventLedger::new();
    /// ledger.push(EventRecord::new(1, 1, EventKind::Pointer, Disposition::Ignored));
    /// ledger.push(EventRecord::new(2, 1, EventKind::Key, Disposition::Ignored));
    ///
    /// assert_eq!(ledger.filter_by_kind(EventKind::Pointer).count(), 1);
    /// ```
    pub fn filter_by_kind(&self, kind: EventKind) -> impl Iterator<Item = &EventRecord> {
        self.iter().filter(move |record| record.kind == kind)
    }

    /// Returns an iterator over all records recorded during the specified frame.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::event_ledger::{Disposition, EventKind, EventLedger, EventRecord};
    ///
    /// let mut ledger = EventLedger::new();
    /// ledger.push(EventRecord::new(1, 100, EventKind::Key, Disposition::Ignored));
    /// ledger.push(EventRecord::new(2, 101, EventKind::Key, Disposition::Ignored));
    ///
    /// assert_eq!(ledger.filter_by_frame(100).count(), 1);
    /// ```
    pub fn filter_by_frame(&self, frame: u64) -> impl Iterator<Item = &EventRecord> {
        self.iter().filter(move |record| record.frame == frame)
    }

    /// Returns an iterator over all records matching a structured [`EventFilter`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::event_ledger::{
    ///     Disposition, EventFilter, EventKind, EventLedger, EventRecord,
    /// };
    ///
    /// let mut ledger = EventLedger::new();
    /// ledger.push(EventRecord::new(1, 100, EventKind::Pointer, Disposition::Ignored));
    /// ledger.push(EventRecord::new(2, 101, EventKind::Pointer, Disposition::Ignored));
    ///
    /// let filter = EventFilter::new().with_kind(EventKind::Pointer).with_frame(100);
    /// assert_eq!(ledger.filter(filter).count(), 1);
    /// ```
    pub fn filter(&self, filter: EventFilter) -> impl Iterator<Item = &EventRecord> + '_ {
        self.iter().filter(move |record| filter.matches(record))
    }
}

/// Iterator over event records in chronological order (oldest to newest).
///
/// Supports exact sizing and double-ended traversal.
///
/// # Examples
///
/// ```
/// use martensite_devtools::event_ledger::{Disposition, EventKind, EventLedger, EventRecord};
///
/// let mut ledger = EventLedger::new();
/// ledger.push(EventRecord::new(1, 1, EventKind::Key, Disposition::Ignored));
/// let mut iter = ledger.iter();
/// assert_eq!(iter.len(), 1);
/// assert!(iter.next().is_some());
/// assert!(iter.next().is_none());
/// ```
#[derive(Debug, Clone)]
pub struct EventLedgerIter<'a> {
    ledger: &'a EventLedger,
    front: usize,
    back: usize,
}

impl<'a> Iterator for EventLedgerIter<'a> {
    type Item = &'a EventRecord;

    fn next(&mut self) -> Option<Self::Item> {
        if self.front < self.back {
            let item = self.ledger.get(self.front);
            self.front += 1;
            item
        } else {
            None
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.back.saturating_sub(self.front);
        (remaining, Some(remaining))
    }
}

impl<'a> DoubleEndedIterator for EventLedgerIter<'a> {
    fn next_back(&mut self) -> Option<Self::Item> {
        if self.front < self.back {
            self.back -= 1;
            self.ledger.get(self.back)
        } else {
            None
        }
    }
}

impl<'a> ExactSizeIterator for EventLedgerIter<'a> {}

impl<'a> IntoIterator for &'a EventLedger {
    type Item = &'a EventRecord;
    type IntoIter = EventLedgerIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// Draining iterator that removes and yields all records from the ledger in chronological order.
///
/// Preserves the preallocated storage capacity so subsequent pushes do not allocate.
///
/// # Examples
///
/// ```
/// use martensite_devtools::event_ledger::{Disposition, EventKind, EventLedger, EventRecord};
///
/// let mut ledger = EventLedger::with_capacity(3);
/// ledger.push(EventRecord::new(1, 1, EventKind::Key, Disposition::Ignored));
/// let mut drain = ledger.drain();
/// assert_eq!(drain.len(), 1);
/// assert!(drain.next().is_some());
/// ```
#[derive(Debug)]
pub struct Drain<'a> {
    ledger: &'a mut EventLedger,
    index: usize,
    total: usize,
}

impl<'a> Iterator for Drain<'a> {
    type Item = EventRecord;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index < self.total {
            let item = self.ledger.get(self.index).copied();
            self.index += 1;
            item
        } else {
            None
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.total.saturating_sub(self.index);
        (remaining, Some(remaining))
    }
}

impl<'a> ExactSizeIterator for Drain<'a> {}

impl<'a> Drop for Drain<'a> {
    fn drop(&mut self) {
        self.ledger.len = 0;
        self.ledger.head = 0;
    }
}

/// Structured query filter for matching events across multiple dimensions.
///
/// # Examples
///
/// ```
/// use martensite_core::WidgetId;
/// use martensite_devtools::event_ledger::{
///     Disposition, EventFilter, EventKind, EventRecord,
/// };
///
/// let id = WidgetId::from_parts(10, 1);
/// let record = EventRecord::new(1, 42, EventKind::Pointer, Disposition::Handled(id));
///
/// let filter = EventFilter::new().with_widget(id).with_kind(EventKind::Pointer).with_frame(42);
/// assert!(filter.matches(&record));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EventFilter {
    /// Optional widget filter.
    pub widget: Option<WidgetId>,
    /// Optional event kind filter.
    pub kind: Option<EventKind>,
    /// Optional frame number filter.
    pub frame: Option<u64>,
}

impl EventFilter {
    /// Creates an empty filter that matches all records.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::event_ledger::{Disposition, EventFilter, EventKind, EventRecord};
    ///
    /// let filter = EventFilter::new();
    /// let record = EventRecord::new(1, 1, EventKind::Key, Disposition::Ignored);
    /// assert!(filter.matches(&record));
    /// ```
    pub const fn new() -> Self {
        Self {
            widget: None,
            kind: None,
            frame: None,
        }
    }

    /// Filters by widget ID (matches hit path, disposition, rejection, or focus).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::WidgetId;
    /// use martensite_devtools::event_ledger::EventFilter;
    ///
    /// let id = WidgetId::from_parts(1, 1);
    /// let filter = EventFilter::new().with_widget(id);
    /// assert_eq!(filter.widget, Some(id));
    /// ```
    pub const fn with_widget(mut self, id: WidgetId) -> Self {
        self.widget = Some(id);
        self
    }

    /// Filters by event kind.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::event_ledger::{EventFilter, EventKind};
    ///
    /// let filter = EventFilter::new().with_kind(EventKind::Pointer);
    /// assert_eq!(filter.kind, Some(EventKind::Pointer));
    /// ```
    pub const fn with_kind(mut self, kind: EventKind) -> Self {
        self.kind = Some(kind);
        self
    }

    /// Filters by frame number.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::event_ledger::EventFilter;
    ///
    /// let filter = EventFilter::new().with_frame(100);
    /// assert_eq!(filter.frame, Some(100));
    /// ```
    pub const fn with_frame(mut self, frame: u64) -> Self {
        self.frame = Some(frame);
        self
    }

    /// Returns `true` if the record matches all configured filter criteria.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::event_ledger::{
    ///     Disposition, EventFilter, EventKind, EventRecord,
    /// };
    ///
    /// let filter = EventFilter::new().with_kind(EventKind::Pointer);
    /// let ptr_record = EventRecord::new(1, 1, EventKind::Pointer, Disposition::Ignored);
    /// let key_record = EventRecord::new(2, 1, EventKind::Key, Disposition::Ignored);
    ///
    /// assert!(filter.matches(&ptr_record));
    /// assert!(!filter.matches(&key_record));
    /// ```
    pub fn matches(&self, record: &EventRecord) -> bool {
        if let Some(widget) = self.widget {
            if !record.mentions_widget(widget) {
                return false;
            }
        }
        if let Some(kind) = self.kind {
            if record.kind != kind {
                return false;
            }
        }
        if let Some(frame) = self.frame {
            if record.frame != frame {
                return false;
            }
        }
        true
    }
}
