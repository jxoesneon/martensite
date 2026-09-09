//! In-app diagnostic HUD overlay.
//!
//! Activating F12 toggles a zero-allocation diagnostic overlay that
//! displays:
//!
//! - A rolling 120-frame timing histogram (layout, paint, GPU wait).
//! - Real-time dirty rect visualization.
//! - Live `WidgetArena` slot utilization and compaction telemetry.
//!
//! When the `render` feature is enabled, [`DiagnosticHud::render_hud`]
//! encodes the overlay into a [`martensite_render::PaintList`] using simple
//! fill, stroke, and text commands.
//!
//! # Example
//!
//! ```
//! use martensite_devtools::hud::{DiagnosticHud, FrameTiming, Rect};
//!
//! let mut hud = DiagnosticHud::new();
//! hud.toggle();
//! assert!(hud.is_enabled());
//!
//! hud.record_frame(FrameTiming {
//!     layout_time_ns: 500_000,
//!     paint_time_ns: 300_000,
//!     gpu_wait_time_ns: 100_000,
//!     total_time_ns: 900_000,
//! });
//!
//! hud.add_dirty_rect(Rect::new(0, 0, 100, 100));
//! ```

#[cfg(feature = "render")]
use kurbo::{Point, Rect as KurboRect};

#[cfg(feature = "render")]
use martensite_render::PaintList;

/// Number of frames retained in the rolling histogram.
const HISTOGRAM_SIZE: usize = 120;

/// Frame timing data for a single frame.
///
/// All times are in nanoseconds for consistent units.
///
/// # Example
///
/// ```
/// use martensite_devtools::hud::FrameTiming;
///
/// let timing = FrameTiming {
///     layout_time_ns: 500_000,
///     paint_time_ns: 300_000,
///     gpu_wait_time_ns: 100_000,
///     total_time_ns: 900_000,
/// };
/// assert_eq!(timing.total_time_ns, 900_000);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FrameTiming {
    /// Time spent in layout passes (nanoseconds).
    pub layout_time_ns: u64,
    /// Time spent encoding the paint list (nanoseconds).
    pub paint_time_ns: u64,
    /// Time spent waiting for the GPU (nanoseconds).
    pub gpu_wait_time_ns: u64,
    /// Total frame time (nanoseconds).
    pub total_time_ns: u64,
}

impl FrameTiming {
    /// Creates a `FrameTiming` from millisecond values.
    ///
    /// # Example
    ///
    /// ```
    /// use martensite_devtools::hud::FrameTiming;
    ///
    /// let t = FrameTiming::from_ms(0.5, 0.3, 0.1, 0.9);
    /// assert_eq!(t.layout_time_ns, 500_000);
    /// ```
    pub fn from_ms(layout: f64, paint: f64, gpu_wait: f64, total: f64) -> Self {
        Self {
            layout_time_ns: (layout * 1_000_000.0) as u64,
            paint_time_ns: (paint * 1_000_000.0) as u64,
            gpu_wait_time_ns: (gpu_wait * 1_000_000.0) as u64,
            total_time_ns: (total * 1_000_000.0) as u64,
        }
    }

    /// Adds two `FrameTiming` values component-wise.
    #[inline]
    pub fn add(&self, other: &Self) -> Self {
        Self {
            layout_time_ns: self.layout_time_ns.saturating_add(other.layout_time_ns),
            paint_time_ns: self.paint_time_ns.saturating_add(other.paint_time_ns),
            gpu_wait_time_ns: self.gpu_wait_time_ns.saturating_add(other.gpu_wait_time_ns),
            total_time_ns: self.total_time_ns.saturating_add(other.total_time_ns),
        }
    }

    /// Divides all components by a scalar.
    #[inline]
    pub fn div(&self, n: u64) -> Self {
        if n == 0 {
            return Self::default();
        }
        Self {
            layout_time_ns: self.layout_time_ns / n,
            paint_time_ns: self.paint_time_ns / n,
            gpu_wait_time_ns: self.gpu_wait_time_ns / n,
            total_time_ns: self.total_time_ns / n,
        }
    }
}

/// Rolling 120-frame timing histogram.
///
/// Stores the last `120` frame timings in a fixed-size ring buffer
/// with zero heap allocation.
///
/// # Example
///
/// ```
/// use martensite_devtools::hud::{FrameHistogram, FrameTiming};
///
/// let mut hist = FrameHistogram::new();
/// hist.record(FrameTiming {
///     layout_time_ns: 500_000,
///     paint_time_ns: 300_000,
///     gpu_wait_time_ns: 100_000,
///     total_time_ns: 900_000,
/// });
/// assert_eq!(hist.len(), 1);
/// let avg = hist.average();
/// assert_eq!(avg.total_time_ns, 900_000);
/// ```
#[derive(Debug, Clone)]
pub struct FrameHistogram {
    frames: [FrameTiming; HISTOGRAM_SIZE],
    index: usize,
    count: usize,
}

impl Default for FrameHistogram {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameHistogram {
    /// Creates a new empty histogram.
    ///
    /// # Example
    ///
    /// ```
    /// use martensite_devtools::hud::FrameHistogram;
    ///
    /// let hist = FrameHistogram::new();
    /// assert!(hist.is_empty());
    /// ```
    pub fn new() -> Self {
        Self {
            frames: [FrameTiming::default(); HISTOGRAM_SIZE],
            index: 0,
            count: 0,
        }
    }

    /// Records a frame timing into the ring buffer.
    ///
    /// # Example
    ///
    /// ```
    /// use martensite_devtools::hud::{FrameHistogram, FrameTiming};
    ///
    /// let mut hist = FrameHistogram::new();
    /// hist.record(FrameTiming { total_time_ns: 16_000_000, ..Default::default() });
    /// assert_eq!(hist.len(), 1);
    /// ```
    pub fn record(&mut self, timing: FrameTiming) {
        self.frames[self.index] = timing;
        self.index = (self.index + 1) % HISTOGRAM_SIZE;
        if self.count < HISTOGRAM_SIZE {
            self.count += 1;
        }
    }

    /// Returns the average frame timing across all recorded frames.
    ///
    /// # Example
    ///
    /// ```
    /// use martensite_devtools::hud::{FrameHistogram, FrameTiming};
    ///
    /// let mut hist = FrameHistogram::new();
    /// hist.record(FrameTiming { total_time_ns: 10_000_000, ..Default::default() });
    /// hist.record(FrameTiming { total_time_ns: 20_000_000, ..Default::default() });
    /// assert_eq!(hist.average().total_time_ns, 15_000_000);
    /// ```
    pub fn average(&self) -> FrameTiming {
        if self.count == 0 {
            return FrameTiming::default();
        }
        let mut sum = FrameTiming::default();
        for i in 0..self.count {
            sum = sum.add(&self.frames[i]);
        }
        sum.div(self.count as u64)
    }

    /// Returns the timing at the given percentile (0.0–100.0).
    ///
    /// `p=50` gives the median, `p=99` gives the 99th percentile.
    ///
    /// # Example
    ///
    /// ```
    /// use martensite_devtools::hud::{FrameHistogram, FrameTiming};
    ///
    /// let mut hist = FrameHistogram::new();
    /// for i in 1..=100 {
    ///     hist.record(FrameTiming { total_time_ns: i * 1_000_000, ..Default::default() });
    /// }
    /// let p50 = hist.percentile(50.0);
    /// assert!(p50.total_time_ns >= 49_000_000 && p50.total_time_ns <= 51_000_000);
    /// ```
    pub fn percentile(&self, p: f32) -> FrameTiming {
        if self.count == 0 {
            return FrameTiming::default();
        }
        let mut totals: Vec<u64> = self.frames[..self.count]
            .iter()
            .map(|f| f.total_time_ns)
            .collect();
        totals.sort_unstable();
        let idx = ((p.clamp(0.0, 100.0) / 100.0) * (self.count as f32 - 1.0)) as usize;
        let target = totals[idx];
        self.frames[..self.count]
            .iter()
            .find(|f| f.total_time_ns == target)
            .copied()
            .unwrap_or_default()
    }

    /// Returns the maximum recorded frame timing.
    ///
    /// # Example
    ///
    /// ```
    /// use martensite_devtools::hud::{FrameHistogram, FrameTiming};
    ///
    /// let mut hist = FrameHistogram::new();
    /// hist.record(FrameTiming { total_time_ns: 10_000_000, ..Default::default() });
    /// hist.record(FrameTiming { total_time_ns: 30_000_000, ..Default::default() });
    /// hist.record(FrameTiming { total_time_ns: 20_000_000, ..Default::default() });
    /// assert_eq!(hist.max().total_time_ns, 30_000_000);
    /// ```
    pub fn max(&self) -> FrameTiming {
        self.frames[..self.count]
            .iter()
            .copied()
            .max_by_key(|f| f.total_time_ns)
            .unwrap_or_default()
    }

    /// Returns a slice of all recorded frame timings.
    ///
    /// **Note**: After the ring buffer wraps (more than 120 frames recorded),
    /// the slice is in physical storage order, not chronological order. The
    /// newest frame may appear at any position. This is suitable for
    /// aggregation (average, max, percentile) but not for chronological
    /// display.
    ///
    /// # Example
    ///
    /// ```
    /// use martensite_devtools::hud::{FrameHistogram, FrameTiming};
    ///
    /// let mut hist = FrameHistogram::new();
    /// hist.record(FrameTiming::default());
    /// assert_eq!(hist.frames().len(), 1);
    /// ```
    pub fn frames(&self) -> &[FrameTiming] {
        &self.frames[..self.count]
    }

    /// Returns the number of recorded frames.
    #[inline]
    pub fn len(&self) -> usize {
        self.count
    }

    /// Returns `true` if no frames have been recorded.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Clears all recorded frames.
    pub fn clear(&mut self) {
        self.index = 0;
        self.count = 0;
    }
}

/// A rectangle for dirty rect tracking.
///
/// # Example
///
/// ```
/// use martensite_devtools::hud::Rect;
///
/// let r = Rect::new(10, 20, 100, 200);
/// assert_eq!(r.area(), 20_000);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rect {
    /// X coordinate (pixels from left).
    pub x: i32,
    /// Y coordinate (pixels from top).
    pub y: i32,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

impl Rect {
    /// Creates a new rectangle.
    ///
    /// # Example
    ///
    /// ```
    /// use martensite_devtools::hud::Rect;
    ///
    /// let r = Rect::new(0, 0, 100, 100);
    /// assert_eq!(r.area(), 10_000);
    /// ```
    pub fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Returns the area of the rectangle in pixels.
    ///
    /// # Example
    ///
    /// ```
    /// use martensite_devtools::hud::Rect;
    ///
    /// assert_eq!(Rect::new(0, 0, 100, 50).area(), 5_000);
    /// ```
    pub fn area(&self) -> u64 {
        self.width as u64 * self.height as u64
    }

    /// Returns `true` if this rectangle intersects another.
    ///
    /// # Example
    ///
    /// ```
    /// use martensite_devtools::hud::Rect;
    ///
    /// let a = Rect::new(0, 0, 100, 100);
    /// let b = Rect::new(50, 50, 100, 100);
    /// let c = Rect::new(200, 200, 50, 50);
    /// assert!(a.intersects(&b));
    /// assert!(!a.intersects(&c));
    /// ```
    pub fn intersects(&self, other: &Rect) -> bool {
        let self_right = self.x + self.width as i32;
        let self_bottom = self.y + self.height as i32;
        let other_right = other.x + other.width as i32;
        let other_bottom = other.y + other.height as i32;
        self.x < other_right
            && self_right > other.x
            && self.y < other_bottom
            && self_bottom > other.y
    }

    /// Returns the bounding union of two rectangles.
    ///
    /// # Example
    ///
    /// ```
    /// use martensite_devtools::hud::Rect;
    ///
    /// let a = Rect::new(0, 0, 100, 100);
    /// let b = Rect::new(50, 50, 100, 100);
    /// let u = a.union(&b);
    /// assert_eq!(u.x, 0);
    /// assert_eq!(u.y, 0);
    /// assert_eq!(u.width, 150);
    /// assert_eq!(u.height, 150);
    /// ```
    pub fn union(&self, other: &Rect) -> Rect {
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        let right = (self.x + self.width as i32).max(other.x + other.width as i32);
        let bottom = (self.y + self.height as i32).max(other.y + other.height as i32);
        Rect::new(x, y, (right - x) as u32, (bottom - y) as u32)
    }
}

/// Dirty rectangle tracking for visualization.
///
/// Tracks repainted screen regions for the diagnostic HUD overlay.
///
/// # Example
///
/// ```
/// use martensite_devtools::hud::{DirtyRectTracker, Rect};
///
/// let mut tracker = DirtyRectTracker::new(64);
/// tracker.add(Rect::new(0, 0, 100, 100));
/// tracker.add(Rect::new(50, 50, 100, 100));
/// assert_eq!(tracker.rects().len(), 2);
/// assert!(tracker.total_area() > 0);
/// ```
#[derive(Debug, Clone)]
pub struct DirtyRectTracker {
    rects: Vec<Rect>,
    max_rects: usize,
}

impl DirtyRectTracker {
    /// Creates a new tracker with the given maximum number of rectangles.
    ///
    /// # Example
    ///
    /// ```
    /// use martensite_devtools::hud::DirtyRectTracker;
    ///
    /// let tracker = DirtyRectTracker::new(32);
    /// assert!(tracker.is_empty());
    /// ```
    pub fn new(max_rects: usize) -> Self {
        Self {
            rects: Vec::with_capacity(max_rects),
            max_rects,
        }
    }

    /// Adds a dirty rectangle. If the tracker is full, the oldest
    /// rectangle is dropped.
    ///
    /// # Example
    ///
    /// ```
    /// use martensite_devtools::hud::{DirtyRectTracker, Rect};
    ///
    /// let mut tracker = DirtyRectTracker::new(2);
    /// tracker.add(Rect::new(0, 0, 10, 10));
    /// tracker.add(Rect::new(10, 10, 10, 10));
    /// tracker.add(Rect::new(20, 20, 10, 10)); // drops oldest
    /// assert_eq!(tracker.rects().len(), 2);
    /// ```
    pub fn add(&mut self, rect: Rect) {
        if self.rects.len() >= self.max_rects {
            self.rects.remove(0);
        }
        self.rects.push(rect);
    }

    /// Returns the tracked rectangles.
    #[inline]
    pub fn rects(&self) -> &[Rect] {
        &self.rects
    }

    /// Clears all tracked rectangles.
    pub fn clear(&mut self) {
        self.rects.clear();
    }

    /// Returns the total area of all tracked rectangles.
    ///
    /// # Example
    ///
    /// ```
    /// use martensite_devtools::hud::{DirtyRectTracker, Rect};
    ///
    /// let mut tracker = DirtyRectTracker::new(10);
    /// tracker.add(Rect::new(0, 0, 100, 100));
    /// assert_eq!(tracker.total_area(), 10_000);
    /// ```
    pub fn total_area(&self) -> u64 {
        self.rects.iter().map(|r| r.area()).sum()
    }

    /// Returns `true` if no rectangles are tracked.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.rects.is_empty()
    }

    /// Returns the number of tracked rectangles.
    #[inline]
    pub fn len(&self) -> usize {
        self.rects.len()
    }
}

/// Arena memory telemetry data.
///
/// Tracks `WidgetArena` slot utilization and compaction statistics.
///
/// # Example
///
/// ```
/// use martensite_devtools::hud::ArenaTelemetry;
///
/// let telemetry = ArenaTelemetry {
///     total_slots: 1000,
///     used_slots: 750,
///     free_slots: 250,
///     utilization_pct: 75.0,
///     compaction_count: 3,
/// };
/// assert_eq!(telemetry.free_slots, 250);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ArenaTelemetry {
    /// Total number of slots in the arena.
    pub total_slots: usize,
    /// Number of slots currently in use.
    pub used_slots: usize,
    /// Number of free (available) slots.
    pub free_slots: usize,
    /// Slot utilization as a percentage (0.0–100.0).
    pub utilization_pct: f32,
    /// Number of compaction passes performed.
    pub compaction_count: u64,
}

impl ArenaTelemetry {
    /// Creates telemetry from slot counts, computing derived fields.
    ///
    /// # Example
    ///
    /// ```
    /// use martensite_devtools::hud::ArenaTelemetry;
    ///
    /// let t = ArenaTelemetry::from_slots(1000, 750, 2);
    /// assert_eq!(t.free_slots, 250);
    /// assert_eq!(t.utilization_pct, 75.0);
    /// ```
    pub fn from_slots(total: usize, used: usize, compactions: u64) -> Self {
        let free = total.saturating_sub(used);
        let utilization_pct = if total > 0 {
            (used as f32 / total as f32) * 100.0
        } else {
            0.0
        };
        Self {
            total_slots: total,
            used_slots: used,
            free_slots: free,
            utilization_pct,
            compaction_count: compactions,
        }
    }
}

/// The diagnostic HUD state.
///
/// Aggregates frame timing, dirty rect, and arena telemetry data
/// for the F12 diagnostic overlay.
///
/// # Example
///
/// ```
/// use martensite_devtools::hud::{DiagnosticHud, FrameTiming, Rect, ArenaTelemetry};
///
/// let mut hud = DiagnosticHud::new();
/// hud.toggle();
/// assert!(hud.is_enabled());
///
/// hud.record_frame(FrameTiming {
///     layout_time_ns: 500_000,
///     paint_time_ns: 300_000,
///     gpu_wait_time_ns: 100_000,
///     total_time_ns: 900_000,
/// });
/// hud.add_dirty_rect(Rect::new(0, 0, 100, 100));
/// hud.update_arena_telemetry(ArenaTelemetry::from_slots(1000, 500, 0));
///
/// assert_eq!(hud.histogram().len(), 1);
/// assert_eq!(hud.dirty_rects().len(), 1);
/// ```
#[derive(Debug, Clone)]
pub struct DiagnosticHud {
    enabled: bool,
    histogram: FrameHistogram,
    dirty_rects: DirtyRectTracker,
    arena_telemetry: ArenaTelemetry,
}

impl Default for DiagnosticHud {
    fn default() -> Self {
        Self::new()
    }
}

impl DiagnosticHud {
    /// Creates a new disabled HUD.
    ///
    /// # Example
    ///
    /// ```
    /// use martensite_devtools::hud::DiagnosticHud;
    ///
    /// let hud = DiagnosticHud::new();
    /// assert!(!hud.is_enabled());
    /// ```
    pub fn new() -> Self {
        Self {
            enabled: false,
            histogram: FrameHistogram::new(),
            dirty_rects: DirtyRectTracker::new(256),
            arena_telemetry: ArenaTelemetry::default(),
        }
    }

    /// Toggles the HUD on/off (bound to F12 in the application).
    ///
    /// # Example
    ///
    /// ```
    /// use martensite_devtools::hud::DiagnosticHud;
    ///
    /// let mut hud = DiagnosticHud::new();
    /// hud.toggle();
    /// assert!(hud.is_enabled());
    /// hud.toggle();
    /// assert!(!hud.is_enabled());
    /// ```
    pub fn toggle(&mut self) {
        self.enabled = !self.enabled;
    }

    /// Returns whether the HUD is currently visible.
    #[inline]
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Records a frame's timing data.
    ///
    /// # Example
    ///
    /// ```
    /// use martensite_devtools::hud::{DiagnosticHud, FrameTiming};
    ///
    /// let mut hud = DiagnosticHud::new();
    /// hud.record_frame(FrameTiming { total_time_ns: 16_000_000, ..Default::default() });
    /// assert_eq!(hud.histogram().len(), 1);
    /// ```
    pub fn record_frame(&mut self, timing: FrameTiming) {
        self.histogram.record(timing);
    }

    /// Adds a dirty rectangle for visualization.
    pub fn add_dirty_rect(&mut self, rect: Rect) {
        self.dirty_rects.add(rect);
    }

    /// Updates the arena telemetry data.
    pub fn update_arena_telemetry(&mut self, telemetry: ArenaTelemetry) {
        self.arena_telemetry = telemetry;
    }

    /// Returns the frame timing histogram.
    #[inline]
    pub fn histogram(&self) -> &FrameHistogram {
        &self.histogram
    }

    /// Returns the dirty rect tracker.
    #[inline]
    pub fn dirty_rects(&self) -> &DirtyRectTracker {
        &self.dirty_rects
    }

    /// Returns the arena telemetry.
    #[inline]
    pub fn arena_telemetry(&self) -> &ArenaTelemetry {
        &self.arena_telemetry
    }

    /// Clears all dirty rects (call after each frame).
    pub fn clear_dirty_rects(&mut self) {
        self.dirty_rects.clear();
    }

    /// Renders the HUD overlay into the given [`PaintList`].
    ///
    /// This is only available when the `render` feature is enabled. The HUD
    /// is positioned in the top-left corner and uses a semi-transparent
    /// background for readability. It draws:
    ///
    /// - A frame timing histogram (one bar per recorded frame).
    /// - Dirty rect visualization (outlined rectangles).
    /// - Arena telemetry (text).
    /// - Memory usage (text).
    ///
    /// # Example
    ///
    /// ```no_run
    /// # #[cfg(feature = "render")]
    /// # {
    /// use martensite_devtools::hud::{DiagnosticHud, FrameTiming, Rect};
    /// use martensite_render::PaintList;
    ///
    /// let mut hud = DiagnosticHud::new();
    /// hud.toggle();
    /// hud.record_frame(FrameTiming { total_time_ns: 16_000_000, ..Default::default() });
    /// hud.add_dirty_rect(Rect::new(10, 10, 50, 50));
    ///
    /// let mut paint = PaintList::new();
    /// hud.render_hud(&mut paint);
    /// assert!(!paint.is_empty());
    /// # }
    /// ```
    #[cfg(feature = "render")]
    pub fn render_hud(&self, paint: &mut PaintList) {
        // --- Semi-transparent background panel (top-left corner). ---
        let bg = KurboRect::new(HUD_X, HUD_Y, HUD_X + HUD_WIDTH, HUD_Y + HUD_HEIGHT);
        paint.push_fill_rect(bg, HUD_BG_COLOR);

        // --- Frame timing histogram (bars). ---
        let hist_origin_y = HUD_Y + HUD_PADDING + HUD_TITLE_SIZE as f64 + 4.0;
        let hist_bottom = hist_origin_y + HUD_HISTOGRAM_HEIGHT;
        let bar_area_width = HUD_WIDTH - 2.0 * HUD_PADDING;
        let frames = self.histogram.frames();
        let max_total = frames
            .iter()
            .map(|f| f.total_time_ns)
            .max()
            .unwrap_or(1)
            .max(1);
        let bar_width = if frames.is_empty() {
            bar_area_width
        } else {
            (bar_area_width / frames.len() as f64).max(1.0)
        };
        for (i, frame) in frames.iter().enumerate() {
            let bar_x = HUD_X + HUD_PADDING + i as f64 * bar_width;
            let bar_h = (frame.total_time_ns as f64 / max_total as f64) * HUD_HISTOGRAM_HEIGHT;
            let bar = KurboRect::new(bar_x, hist_bottom - bar_h, bar_x + bar_width, hist_bottom);
            let color = if frame.total_time_ns > 16_666_666 {
                HUD_BAR_COLOR_SLOW
            } else {
                HUD_BAR_COLOR_OK
            };
            paint.push_fill_rect(bar, color);
        }

        // Outline the histogram region.
        let hist_outline = KurboRect::new(
            HUD_X + HUD_PADDING,
            hist_origin_y,
            HUD_X + HUD_PADDING + bar_area_width,
            hist_bottom,
        );
        paint.push_stroke_rect(hist_outline, 1.0, HUD_OUTLINE_COLOR);

        // --- Text: title and averages. ---
        let text_x = HUD_X + HUD_PADDING;
        let mut text_y = hist_bottom + 16.0;
        paint.push_text(
            Point::new(text_x, HUD_Y + HUD_PADDING + HUD_TITLE_SIZE as f64),
            "Martensite HUD".to_string(),
            HUD_TITLE_SIZE,
            HUD_TEXT_COLOR,
        );

        let avg = self.histogram.average();
        let avg_ms = avg.total_time_ns as f64 / 1_000_000.0;
        paint.push_text(
            Point::new(text_x, text_y),
            format!("avg frame: {avg_ms:.2} ms"),
            HUD_TEXT_SIZE,
            HUD_TEXT_COLOR,
        );
        text_y += HUD_LINE_HEIGHT;

        let max = self.histogram.max();
        let max_ms = max.total_time_ns as f64 / 1_000_000.0;
        paint.push_text(
            Point::new(text_x, text_y),
            format!("max frame: {max_ms:.2} ms"),
            HUD_TEXT_SIZE,
            HUD_TEXT_COLOR,
        );
        text_y += HUD_LINE_HEIGHT;

        // --- Arena telemetry (text). ---
        let t = &self.arena_telemetry;
        paint.push_text(
            Point::new(text_x, text_y),
            format!(
                "arena: {}/{} slots ({:.1}%)",
                t.used_slots, t.total_slots, t.utilization_pct
            ),
            HUD_TEXT_SIZE,
            HUD_TEXT_COLOR,
        );
        text_y += HUD_LINE_HEIGHT;
        paint.push_text(
            Point::new(text_x, text_y),
            format!("compactions: {}", t.compaction_count),
            HUD_TEXT_SIZE,
            HUD_TEXT_COLOR,
        );
        text_y += HUD_LINE_HEIGHT;

        // --- Memory usage (text). ---
        let mem_bytes = t.used_slots * 64; // approximate bytes per slot
        paint.push_text(
            Point::new(text_x, text_y),
            format!("mem (est): {} KB", mem_bytes / 1024),
            HUD_TEXT_SIZE,
            HUD_TEXT_COLOR,
        );

        // --- Dirty rect visualization (outlined rectangles). ---
        for rect in self.dirty_rects.rects() {
            let dirty = KurboRect::new(
                rect.x as f64,
                rect.y as f64,
                (rect.x + rect.width as i32) as f64,
                (rect.y + rect.height as i32) as f64,
            );
            paint.push_stroke_rect(dirty, 2.0, HUD_DIRTY_COLOR);
        }
    }

    /// Paints the HUD overlay into the given [`PaintList`].
    ///
    /// This is a convenience alias for [`DiagnosticHud::render_hud`].
    ///
    /// # Example
    ///
    /// ```no_run
    /// # #[cfg(feature = "render")]
    /// # {
    /// use martensite_devtools::hud::{DiagnosticHud, FrameTiming};
    /// use martensite_render::PaintList;
    ///
    /// let mut hud = DiagnosticHud::new();
    /// hud.toggle();
    /// hud.record_frame(FrameTiming { total_time_ns: 16_000_000, ..Default::default() });
    ///
    /// let mut paint = PaintList::new();
    /// hud.paint(&mut paint);
    /// assert!(!paint.is_empty());
    /// # }
    /// ```
    #[cfg(feature = "render")]
    pub fn paint(&self, paint: &mut PaintList) {
        self.render_hud(paint);
    }
}

/// HUD layout and color constants (only compiled with the `render` feature).
#[cfg(feature = "render")]
mod hud_layout {
    /// X offset of the HUD panel (pixels from left).
    pub(super) const HUD_X: f64 = 8.0;
    /// Y offset of the HUD panel (pixels from top).
    pub(super) const HUD_Y: f64 = 8.0;
    /// Width of the HUD panel.
    pub(super) const HUD_WIDTH: f64 = 320.0;
    /// Height of the HUD panel.
    pub(super) const HUD_HEIGHT: f64 = 220.0;
    /// Inner padding of the HUD panel.
    pub(super) const HUD_PADDING: f64 = 8.0;
    /// Font size of the HUD title.
    pub(super) const HUD_TITLE_SIZE: f32 = 14.0;
    /// Font size of HUD body text.
    pub(super) const HUD_TEXT_SIZE: f32 = 12.0;
    /// Line height for stacked text lines.
    pub(super) const HUD_LINE_HEIGHT: f64 = 16.0;
    /// Height of the histogram bar region.
    pub(super) const HUD_HISTOGRAM_HEIGHT: f64 = 60.0;
    /// Semi-transparent dark background color.
    pub(super) const HUD_BG_COLOR: [u8; 4] = [20, 20, 30, 200];
    /// Outline color for the histogram region.
    pub(super) const HUD_OUTLINE_COLOR: [u8; 4] = [120, 120, 140, 255];
    /// Bar color for frames within the 60 fps budget (green).
    pub(super) const HUD_BAR_COLOR_OK: [u8; 4] = [80, 200, 120, 255];
    /// Bar color for frames exceeding the 60 fps budget (red).
    pub(super) const HUD_BAR_COLOR_SLOW: [u8; 4] = [220, 80, 80, 255];
    /// Text color (light gray).
    pub(super) const HUD_TEXT_COLOR: [u8; 4] = [230, 230, 230, 255];
    /// Dirty rect outline color (cyan).
    pub(super) const HUD_DIRTY_COLOR: [u8; 4] = [80, 200, 220, 255];
}

#[cfg(feature = "render")]
use hud_layout::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_timing_from_ms() {
        let t = FrameTiming::from_ms(0.5, 0.3, 0.1, 0.9);
        assert_eq!(t.layout_time_ns, 500_000);
        assert_eq!(t.paint_time_ns, 300_000);
        assert_eq!(t.gpu_wait_time_ns, 100_000);
        assert_eq!(t.total_time_ns, 900_000);
    }

    #[test]
    fn frame_timing_add_and_div() {
        let a = FrameTiming {
            total_time_ns: 10,
            ..Default::default()
        };
        let b = FrameTiming {
            total_time_ns: 20,
            ..Default::default()
        };
        let sum = a.add(&b);
        assert_eq!(sum.total_time_ns, 30);
        let avg = sum.div(2);
        assert_eq!(avg.total_time_ns, 15);
        let div_zero = sum.div(0);
        assert_eq!(div_zero.total_time_ns, 0);
    }

    #[test]
    fn histogram_new_is_empty() {
        let hist = FrameHistogram::new();
        assert!(hist.is_empty());
        assert_eq!(hist.len(), 0);
    }

    #[test]
    fn histogram_record_and_average() {
        let mut hist = FrameHistogram::new();
        hist.record(FrameTiming {
            total_time_ns: 10_000_000,
            ..Default::default()
        });
        hist.record(FrameTiming {
            total_time_ns: 20_000_000,
            ..Default::default()
        });
        assert_eq!(hist.len(), 2);
        let avg = hist.average();
        assert_eq!(avg.total_time_ns, 15_000_000);
    }

    #[test]
    fn histogram_max() {
        let mut hist = FrameHistogram::new();
        hist.record(FrameTiming {
            total_time_ns: 10,
            ..Default::default()
        });
        hist.record(FrameTiming {
            total_time_ns: 30,
            ..Default::default()
        });
        hist.record(FrameTiming {
            total_time_ns: 20,
            ..Default::default()
        });
        assert_eq!(hist.max().total_time_ns, 30);
    }

    #[test]
    fn histogram_percentile() {
        let mut hist = FrameHistogram::new();
        for i in 1..=100 {
            hist.record(FrameTiming {
                total_time_ns: i * 1_000_000,
                ..Default::default()
            });
        }
        let p50 = hist.percentile(50.0);
        assert!(p50.total_time_ns >= 49_000_000 && p50.total_time_ns <= 51_000_000);
        let p99 = hist.percentile(99.0);
        assert!(p99.total_time_ns >= 98_000_000);
    }

    #[test]
    fn histogram_ring_buffer_wrap() {
        let mut hist = FrameHistogram::new();
        for i in 0..150 {
            hist.record(FrameTiming {
                total_time_ns: i,
                ..Default::default()
            });
        }
        assert_eq!(hist.len(), 120);
    }

    #[test]
    fn histogram_clear() {
        let mut hist = FrameHistogram::new();
        hist.record(FrameTiming::default());
        hist.clear();
        assert!(hist.is_empty());
    }

    #[test]
    fn rect_area() {
        assert_eq!(Rect::new(0, 0, 100, 50).area(), 5_000);
        assert_eq!(Rect::new(10, 20, 0, 100).area(), 0);
    }

    #[test]
    fn rect_intersects() {
        let a = Rect::new(0, 0, 100, 100);
        let b = Rect::new(50, 50, 100, 100);
        let c = Rect::new(200, 200, 50, 50);
        assert!(a.intersects(&b));
        assert!(!a.intersects(&c));
    }

    #[test]
    fn rect_union() {
        let a = Rect::new(0, 0, 100, 100);
        let b = Rect::new(50, 50, 100, 100);
        let u = a.union(&b);
        assert_eq!(u.x, 0);
        assert_eq!(u.y, 0);
        assert_eq!(u.width, 150);
        assert_eq!(u.height, 150);
    }

    #[test]
    fn dirty_rect_tracker_basic() {
        let mut tracker = DirtyRectTracker::new(64);
        assert!(tracker.is_empty());
        tracker.add(Rect::new(0, 0, 100, 100));
        tracker.add(Rect::new(50, 50, 100, 100));
        assert_eq!(tracker.len(), 2);
        assert!(!tracker.is_empty());
        assert!(tracker.total_area() > 0);
        tracker.clear();
        assert!(tracker.is_empty());
    }

    #[test]
    fn dirty_rect_tracker_eviction() {
        let mut tracker = DirtyRectTracker::new(2);
        tracker.add(Rect::new(0, 0, 10, 10));
        tracker.add(Rect::new(10, 10, 10, 10));
        tracker.add(Rect::new(20, 20, 10, 10));
        assert_eq!(tracker.len(), 2);
        assert_eq!(tracker.rects()[0], Rect::new(10, 10, 10, 10));
    }

    #[test]
    fn arena_telemetry_from_slots() {
        let t = ArenaTelemetry::from_slots(1000, 750, 3);
        assert_eq!(t.total_slots, 1000);
        assert_eq!(t.used_slots, 750);
        assert_eq!(t.free_slots, 250);
        assert_eq!(t.utilization_pct, 75.0);
        assert_eq!(t.compaction_count, 3);
    }

    #[test]
    fn arena_telemetry_zero_total() {
        let t = ArenaTelemetry::from_slots(0, 0, 0);
        assert_eq!(t.utilization_pct, 0.0);
    }

    #[test]
    fn hud_toggle() {
        let mut hud = DiagnosticHud::new();
        assert!(!hud.is_enabled());
        hud.toggle();
        assert!(hud.is_enabled());
        hud.toggle();
        assert!(!hud.is_enabled());
    }

    #[test]
    fn hud_record_frame_and_dirty_rect() {
        let mut hud = DiagnosticHud::new();
        hud.record_frame(FrameTiming {
            total_time_ns: 16_000_000,
            ..Default::default()
        });
        hud.add_dirty_rect(Rect::new(0, 0, 100, 100));
        assert_eq!(hud.histogram().len(), 1);
        assert_eq!(hud.dirty_rects().len(), 1);
        hud.clear_dirty_rects();
        assert_eq!(hud.dirty_rects().len(), 0);
    }

    #[test]
    fn hud_arena_telemetry_update() {
        let mut hud = DiagnosticHud::new();
        hud.update_arena_telemetry(ArenaTelemetry::from_slots(500, 250, 1));
        assert_eq!(hud.arena_telemetry().used_slots, 250);
    }

    #[cfg(feature = "render")]
    #[test]
    fn render_hud_emits_background_fill() {
        let mut hud = DiagnosticHud::new();
        hud.toggle();
        let mut paint = PaintList::new();
        hud.render_hud(&mut paint);
        // The first command must be the semi-transparent background fill.
        assert!(!paint.is_empty());
        assert!(matches!(
            paint.commands[0],
            martensite_render::PaintCommand::FillRect(..)
        ));
    }

    #[cfg(feature = "render")]
    #[test]
    fn render_hud_emits_histogram_bars_for_recorded_frames() {
        let mut hud = DiagnosticHud::new();
        hud.toggle();
        hud.record_frame(FrameTiming {
            total_time_ns: 16_000_000,
            ..Default::default()
        });
        hud.record_frame(FrameTiming {
            total_time_ns: 8_000_000,
            ..Default::default()
        });
        let mut paint = PaintList::new();
        hud.render_hud(&mut paint);
        // Background + 2 bars + 1 outline = at least 4 fill/stroke rects.
        let fills = paint
            .commands
            .iter()
            .filter(|c| matches!(c, martensite_render::PaintCommand::FillRect(..)))
            .count();
        assert!(
            fills >= 3,
            "expected at least 3 FillRect commands, got {fills}"
        );
    }

    #[cfg(feature = "render")]
    #[test]
    fn render_hud_emits_dirty_rect_strokes() {
        let mut hud = DiagnosticHud::new();
        hud.toggle();
        hud.add_dirty_rect(Rect::new(10, 20, 30, 40));
        let mut paint = PaintList::new();
        hud.render_hud(&mut paint);
        // At least one StrokeRect for the dirty rect visualization.
        let strokes = paint
            .commands
            .iter()
            .filter(|c| matches!(c, martensite_render::PaintCommand::StrokeRect(..)))
            .count();
        assert!(
            strokes >= 2,
            "expected at least 2 StrokeRect commands (outline + dirty rect), got {strokes}"
        );
    }

    #[cfg(feature = "render")]
    #[test]
    fn render_hud_emits_text_commands() {
        let mut hud = DiagnosticHud::new();
        hud.toggle();
        hud.update_arena_telemetry(ArenaTelemetry::from_slots(1000, 750, 2));
        let mut paint = PaintList::new();
        hud.render_hud(&mut paint);
        let texts = paint
            .commands
            .iter()
            .filter(|c| matches!(c, martensite_render::PaintCommand::DrawText(..)))
            .count();
        // Title + avg + max + arena + compactions + mem = 6 text lines.
        assert!(
            texts >= 6,
            "expected at least 6 DrawText commands, got {texts}"
        );
        // Verify the arena telemetry text contains the slot count.
        let has_arena_text = paint.commands.iter().any(|c| {
            if let martensite_render::PaintCommand::DrawText(_, text, _, _) = c {
                text.contains("arena")
            } else {
                false
            }
        });
        assert!(has_arena_text, "expected an arena telemetry text line");
    }

    #[cfg(feature = "render")]
    #[test]
    fn render_hud_disabled_produces_commands_anyway() {
        // render_hud always emits commands regardless of the enabled flag;
        // the caller is responsible for gating on is_enabled().
        let hud = DiagnosticHud::new();
        assert!(!hud.is_enabled());
        let mut paint = PaintList::new();
        hud.render_hud(&mut paint);
        assert!(!paint.is_empty());
    }

    #[cfg(feature = "render")]
    #[test]
    fn render_hud_empty_histogram_does_not_panic() {
        let mut hud = DiagnosticHud::new();
        hud.toggle();
        let mut paint = PaintList::new();
        // No frames recorded — must not divide by zero or panic.
        hud.render_hud(&mut paint);
        assert!(!paint.is_empty());
    }
}
