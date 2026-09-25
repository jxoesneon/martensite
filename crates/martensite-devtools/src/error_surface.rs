//! Dev-mode error surface and diagnostic overlay.
//!
//! Implements the three severity tiers specified in `docs/dx/ERROR_SURFACE.md`:
//!
//! - **Tier 1 (Inline annotations):** ambient per-widget decorations drawn directly
//!   into the frame over offending widgets (Flutter-style yellow/black diagonal
//!   hatch tape with overflow px deltas, clipped-text indicators, and error-severity
//!   corner ticks).
//! - **Tier 2 (Diagnostics overlay):** per-frame diagnostic entries aggregated for
//!   the HUD section and inspector tab, including linkable node paths, one-line causes,
//!   frame-age tracking (`for 240f`), and documentation links.
//! - **Tier 3 (Structured dev panic):** panic handler capturing crash bundles (panic
//!   message, in-flight widget path, recovery policies: continue with pruned node for
//!   paint vs restart-only for layout, and recent event-ledger history).
//!
//! # Anti-Noise Invariants
//!
//! 1. **Severity Floor:** Ambient inline decorations (Tier 1) display [`DiagnosticSeverity::Error`]
//!    and [`DiagnosticSeverity::Fatal`] only. Warnings and info stay in the inspector panel.
//! 2. **Deduplication:** Repeated diagnostics on the same node across frames increment an
//!    occurrence count and age counter rather than creating duplicate entries.
//! 3. **Collapse Cap:** When total simultaneous diagnostics exceed a configurable threshold
//!    (default 50), Tier 1 collapses to a single summary bar (`"N diagnostics — open inspector"`)
//!    to avoid carpeting the screen.
//! 4. **Zero Cost When Clean:** When no diagnostics are recorded, zero paint commands are emitted
//!    and frame overhead is negligible.
//!
//! # Examples
//!
//! ```
//! use glam::Vec2;
//! use kurbo::Rect;
//! use martensite_core::{PaintList, WidgetId};
//! use martensite_devtools::error_surface::{ErrorSurface, LayoutDiagnostic};
//!
//! let mut surface = ErrorSurface::new();
//! let widget = WidgetId::from_parts(1, 1);
//!
//! // Record an overflow diagnostic.
//! surface.record_layout_diagnostic(LayoutDiagnostic::new(
//!     widget,
//!     "Root/Container/Row",
//!     Rect::new(0.0, 0.0, 100.0, 50.0),
//!     Vec2::new(100.0, 50.0),
//!     Vec2::new(142.5, 50.0),
//! ));
//!
//! assert!(surface.has_diagnostics());
//! assert_eq!(surface.diagnostic_count(), 1);
//!
//! let mut paint = PaintList::new();
//! surface.render_tier1_annotations(&mut paint);
//! assert!(!paint.is_empty());
//! ```

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use glam::Vec2;
use kurbo::{BezPath, Point, Rect};
use martensite_core::{PaintList, WidgetId};

use crate::event_ledger::EventLedger;
use crate::inspector::Axis;

/// Default yellow stripe color matching Flutter's overflow tape convention (`#FFD600`).
pub const DEFAULT_STRIPE_YELLOW: [u8; 4] = [255, 214, 0, 255];

/// Default black stripe color matching Flutter's overflow tape convention (`#212121`).
pub const DEFAULT_STRIPE_BLACK: [u8; 4] = [33, 33, 33, 255];

/// Default error red color (`#EF4444`).
pub const DEFAULT_ERROR_RED: [u8; 4] = [239, 68, 68, 255];

/// Default warning amber color (`#F59E0B`).
pub const DEFAULT_WARN_AMBER: [u8; 4] = [245, 158, 11, 255];

/// Default maximum number of inline annotations before collapsing to summary.
pub const DEFAULT_MAX_INLINE_ANNOTATIONS: usize = 50;

static DEV_PANIC_ENABLED: AtomicBool = AtomicBool::new(false);
static DEV_PANIC_CHECKED: AtomicBool = AtomicBool::new(false);
static GLOBAL_LAST_PANIC: Mutex<Option<CrashBundle>> = Mutex::new(None);

thread_local! {
    #[allow(clippy::missing_const_for_thread_local)]
    static IN_FLIGHT_CONTEXT: RefCell<Option<InFlightContext>> = const { RefCell::new(None) };
}

/// Severity classification of a diagnostic issue.
///
/// # Examples
///
/// ```
/// use martensite_devtools::error_surface::DiagnosticSeverity;
///
/// assert!(DiagnosticSeverity::Error.shows_inline());
/// assert!(DiagnosticSeverity::Fatal.shows_inline());
/// assert!(!DiagnosticSeverity::Warn.shows_inline());
/// assert!(!DiagnosticSeverity::Info.shows_inline());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DiagnosticSeverity {
    /// Informational telemetry or suggestion.
    Info,
    /// Warning that may degrade usability or performance, but does not break layout.
    Warn,
    /// Actionable layout overflow, clipping, or design standard violation.
    Error,
    /// Critical or fatal error requiring panic card or restart.
    Fatal,
}

impl DiagnosticSeverity {
    /// Returns `true` if this severity requires developer attention.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::error_surface::DiagnosticSeverity;
    ///
    /// assert!(DiagnosticSeverity::Warn.is_actionable());
    /// assert!(DiagnosticSeverity::Error.is_actionable());
    /// assert!(!DiagnosticSeverity::Info.is_actionable());
    /// ```
    #[inline]
    pub const fn is_actionable(&self) -> bool {
        matches!(self, Self::Warn | Self::Error | Self::Fatal)
    }

    /// Returns `true` if this severity qualifies for Tier 1 ambient inline display.
    ///
    /// Inline display is reserved for Error and Fatal to avoid becoming wallpaper.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::error_surface::DiagnosticSeverity;
    ///
    /// assert!(DiagnosticSeverity::Error.shows_inline());
    /// assert!(!DiagnosticSeverity::Warn.shows_inline());
    /// ```
    #[inline]
    pub const fn shows_inline(&self) -> bool {
        matches!(self, Self::Error | Self::Fatal)
    }

    /// Returns a string identifier for this severity.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::error_surface::DiagnosticSeverity;
    ///
    /// assert_eq!(DiagnosticSeverity::Error.as_str(), "error");
    /// ```
    #[inline]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
            Self::Fatal => "fatal",
        }
    }

    /// Returns the badge color associated with this severity.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::error_surface::DiagnosticSeverity;
    ///
    /// let red = DiagnosticSeverity::Error.badge_color();
    /// assert_eq!(red[0], 239);
    /// ```
    #[inline]
    pub const fn badge_color(&self) -> [u8; 4] {
        match self {
            Self::Info => [59, 130, 246, 255],
            Self::Warn => DEFAULT_WARN_AMBER,
            Self::Error => DEFAULT_ERROR_RED,
            Self::Fatal => [185, 28, 28, 255],
        }
    }
}

/// Structured diagnostic describing a layout constraint violation or dimension overflow.
///
/// # Examples
///
/// ```
/// use glam::Vec2;
/// use kurbo::Rect;
/// use martensite_core::WidgetId;
/// use martensite_devtools::error_surface::LayoutDiagnostic;
/// use martensite_devtools::Axis;
///
/// let diag = LayoutDiagnostic::new(
///     WidgetId::from_parts(1, 1),
///     "App/Sidebar",
///     Rect::new(0.0, 0.0, 200.0, 100.0),
///     Vec2::new(200.0, 100.0),
///     Vec2::new(245.0, 100.0),
/// );
///
/// assert!(diag.has_overflow());
/// assert_eq!(diag.axis, Axis::Horizontal);
/// assert_eq!(diag.overflow_delta.x, 45.0);
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutDiagnostic {
    /// Widget arena ID associated with this diagnostic, if known.
    pub node: Option<WidgetId>,
    /// Linkable node path (e.g. `"Root/Container/Row"`).
    pub node_path: String,
    /// Bounding rectangle of the widget in logical pixels.
    pub bounds: Rect,
    /// Constraint bounds offered to the node during layout.
    pub offered_size: Vec2,
    /// Final resolved dimensions of the node.
    pub resolved_size: Vec2,
    /// Overflow amount along each axis (`(resolved - offered).max(0)`).
    pub overflow_delta: Vec2,
    /// Primary axis or axes along which overflow occurred.
    pub axis: Axis,
    /// Documentation link explaining this violation.
    pub doc_link: String,
}

impl LayoutDiagnostic {
    /// Creates a new `LayoutDiagnostic` computing the overflow delta and axis.
    ///
    /// # Examples
    ///
    /// ```
    /// use glam::Vec2;
    /// use kurbo::Rect;
    /// use martensite_core::WidgetId;
    /// use martensite_devtools::error_surface::LayoutDiagnostic;
    ///
    /// let diag = LayoutDiagnostic::new(
    ///     WidgetId::from_parts(10, 1),
    ///     "App/Content/Card",
    ///     Rect::new(0.0, 0.0, 300.0, 150.0),
    ///     Vec2::new(300.0, 150.0),
    ///     Vec2::new(320.0, 150.0),
    /// );
    /// assert_eq!(diag.overflow_delta.x, 20.0);
    /// ```
    pub fn new(
        node: impl Into<Option<WidgetId>>,
        node_path: impl Into<String>,
        mut bounds: Rect,
        offered_size: Vec2,
        resolved_size: Vec2,
    ) -> Self {
        let delta_x = (resolved_size.x - offered_size.x).max(0.0);
        let delta_y = (resolved_size.y - offered_size.y).max(0.0);
        let overflow_delta = Vec2::new(delta_x, delta_y);

        let has_x = delta_x > 0.01;
        let has_y = delta_y > 0.01;
        let axis = if has_x && has_y {
            Axis::Both
        } else if has_x {
            Axis::Horizontal
        } else {
            Axis::Vertical
        };

        if bounds.width() <= 0.0 || bounds.height() <= 0.0 {
            bounds = Rect::new(0.0, 0.0, resolved_size.x as f64, resolved_size.y as f64);
        }

        Self {
            node: node.into(),
            node_path: node_path.into(),
            bounds,
            offered_size,
            resolved_size,
            overflow_delta,
            axis,
            doc_link: "https://martensite.dev/docs/errors/layout-overflow".to_string(),
        }
    }

    /// Sets a custom documentation link.
    ///
    /// # Examples
    ///
    /// ```
    /// use glam::Vec2;
    /// use kurbo::Rect;
    /// use martensite_devtools::error_surface::LayoutDiagnostic;
    ///
    /// let diag = LayoutDiagnostic::new(
    ///     None,
    ///     "Root/Child",
    ///     Rect::ZERO,
    ///     Vec2::new(10.0, 10.0),
    ///     Vec2::new(20.0, 10.0),
    /// ).with_doc_link("https://custom.link/overflow");
    /// assert_eq!(diag.doc_link, "https://custom.link/overflow");
    /// ```
    #[must_use]
    pub fn with_doc_link(mut self, doc_link: impl Into<String>) -> Self {
        self.doc_link = doc_link.into();
        self
    }

    /// Sets an explicit axis.
    ///
    /// # Examples
    ///
    /// ```
    /// use glam::Vec2;
    /// use kurbo::Rect;
    /// use martensite_devtools::error_surface::LayoutDiagnostic;
    /// use martensite_devtools::Axis;
    ///
    /// let diag = LayoutDiagnostic::new(
    ///     None,
    ///     "Root/Child",
    ///     Rect::ZERO,
    ///     Vec2::new(10.0, 10.0),
    ///     Vec2::new(20.0, 10.0),
    /// ).with_axis(Axis::Both);
    /// assert_eq!(diag.axis, Axis::Both);
    /// ```
    #[must_use]
    pub fn with_axis(mut self, axis: Axis) -> Self {
        self.axis = axis;
        self
    }

    /// Returns `true` if there is a measurable overflow (> 0.01 px).
    ///
    /// # Examples
    ///
    /// ```
    /// use glam::Vec2;
    /// use kurbo::Rect;
    /// use martensite_devtools::error_surface::LayoutDiagnostic;
    ///
    /// let diag = LayoutDiagnostic::new(
    ///     None,
    ///     "A",
    ///     Rect::ZERO,
    ///     Vec2::new(100.0, 100.0),
    ///     Vec2::new(110.0, 100.0),
    /// );
    /// assert!(diag.has_overflow());
    /// ```
    #[inline]
    pub fn has_overflow(&self) -> bool {
        self.overflow_delta.x > 0.01 || self.overflow_delta.y > 0.01
    }

    /// Returns the maximum overflow magnitude across both axes in pixels.
    ///
    /// # Examples
    ///
    /// ```
    /// use glam::Vec2;
    /// use kurbo::Rect;
    /// use martensite_devtools::error_surface::LayoutDiagnostic;
    ///
    /// let diag = LayoutDiagnostic::new(
    ///     None,
    ///     "A",
    ///     Rect::ZERO,
    ///     Vec2::new(100.0, 100.0),
    ///     Vec2::new(125.5, 100.0),
    /// );
    /// assert_eq!(diag.max_overflow(), 25.5);
    /// ```
    #[inline]
    pub fn max_overflow(&self) -> f32 {
        self.overflow_delta.x.max(self.overflow_delta.y)
    }

    /// Returns a human-readable one-line description of the violation.
    ///
    /// # Examples
    ///
    /// ```
    /// use glam::Vec2;
    /// use kurbo::Rect;
    /// use martensite_devtools::error_surface::LayoutDiagnostic;
    ///
    /// let diag = LayoutDiagnostic::new(
    ///     None,
    ///     "Row",
    ///     Rect::ZERO,
    ///     Vec2::new(100.0, 50.0),
    ///     Vec2::new(140.0, 50.0),
    /// );
    /// assert!(diag.one_line_cause().contains("40.0px"));
    /// ```
    pub fn one_line_cause(&self) -> String {
        format!(
            "Layout overflow: {:.1}px along {:?} axis (offered {:.1}x{:.1}, resolved {:.1}x{:.1})",
            self.max_overflow(),
            self.axis,
            self.offered_size.x,
            self.offered_size.y,
            self.resolved_size.x,
            self.resolved_size.y,
        )
    }

    /// Builds the Tier 1 diagonal hatch tape metadata for this overflow.
    ///
    /// # Examples
    ///
    /// ```
    /// use glam::Vec2;
    /// use kurbo::Rect;
    /// use martensite_devtools::error_surface::LayoutDiagnostic;
    ///
    /// let diag = LayoutDiagnostic::new(
    ///     None,
    ///     "Row",
    ///     Rect::new(0.0, 0.0, 100.0, 50.0),
    ///     Vec2::new(100.0, 50.0),
    ///     Vec2::new(135.0, 50.0),
    /// );
    /// let tape = diag.to_tape_metadata(10.0);
    /// assert_eq!(tape.overflow_px, 35.0);
    /// ```
    pub fn to_tape_metadata(&self, tape_thickness: f32) -> OverflowTape {
        let thickness = (tape_thickness as f64).max(4.0);
        let edge_rect = match self.axis {
            Axis::Horizontal => {
                let x0 = (self.bounds.x1 - thickness).max(self.bounds.x0);
                Rect::new(x0, self.bounds.y0, self.bounds.x1, self.bounds.y1)
            }
            Axis::Vertical => {
                let y0 = (self.bounds.y1 - thickness).max(self.bounds.y0);
                Rect::new(self.bounds.x0, y0, self.bounds.x1, self.bounds.y1)
            }
            Axis::Both => {
                let x0 = (self.bounds.x1 - thickness).max(self.bounds.x0);
                let y0 = (self.bounds.y1 - thickness).max(self.bounds.y0);
                Rect::new(x0, y0, self.bounds.x1, self.bounds.y1)
            }
        };

        OverflowTape::new(edge_rect, self.max_overflow(), self.axis)
    }

    /// Converts this diagnostic to a Tier 2 diagnostic entry for the HUD/inspector.
    ///
    /// # Examples
    ///
    /// ```
    /// use glam::Vec2;
    /// use kurbo::Rect;
    /// use martensite_devtools::error_surface::LayoutDiagnostic;
    ///
    /// let diag = LayoutDiagnostic::new(
    ///     None,
    ///     "Row",
    ///     Rect::ZERO,
    ///     Vec2::new(10.0, 10.0),
    ///     Vec2::new(20.0, 10.0),
    /// );
    /// let entry = diag.to_entry(1);
    /// assert_eq!(entry.node_path, "Row");
    /// ```
    pub fn to_entry(&self, frame: u64) -> DiagnosticEntry {
        DiagnosticEntry {
            id: format!("layout_overflow:{}:{:?}", self.node_path, self.axis),
            node_id: self.node,
            node_path: self.node_path.clone(),
            one_line_cause: self.one_line_cause(),
            doc_link: self.doc_link.clone(),
            severity: DiagnosticSeverity::Error,
            first_seen_frame: frame,
            last_seen_frame: frame,
            occurrences: 1,
        }
    }
}

/// Metadata and rendering parameters for Tier 1 Flutter-style diagonal hatch tape.
///
/// # Examples
///
/// ```
/// use kurbo::Rect;
/// use martensite_devtools::error_surface::OverflowTape;
/// use martensite_devtools::Axis;
///
/// let tape = OverflowTape::new(Rect::new(90.0, 0.0, 100.0, 50.0), 42.5, Axis::Horizontal);
/// assert_eq!(tape.label_text, "+42.5px");
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct OverflowTape {
    /// Bounding rectangle of the tape along the overflowing edge.
    pub edge_rect: Rect,
    /// Pixel delta of the overflow.
    pub overflow_px: f32,
    /// Primary axis of the overflow.
    pub axis: Axis,
    /// Color of primary diagonal stripes (default yellow).
    pub stripe_color_1: [u8; 4],
    /// Color of secondary diagonal stripes / background (default black).
    pub stripe_color_2: [u8; 4],
    /// Text label shown on the badge (e.g. `"+42.5px"`).
    pub label_text: String,
}

impl OverflowTape {
    /// Creates a new `OverflowTape` with default Flutter colors and formatted label.
    ///
    /// # Examples
    ///
    /// ```
    /// use kurbo::Rect;
    /// use martensite_devtools::error_surface::OverflowTape;
    /// use martensite_devtools::Axis;
    ///
    /// let tape = OverflowTape::new(Rect::new(0.0, 40.0, 100.0, 50.0), 12.0, Axis::Vertical);
    /// assert_eq!(tape.label_text, "+12.0px");
    /// ```
    pub fn new(edge_rect: Rect, overflow_px: f32, axis: Axis) -> Self {
        Self {
            edge_rect,
            overflow_px,
            axis,
            stripe_color_1: DEFAULT_STRIPE_YELLOW,
            stripe_color_2: DEFAULT_STRIPE_BLACK,
            label_text: format!("+{:.1}px", overflow_px),
        }
    }

    /// Sets custom stripe colors.
    ///
    /// # Examples
    ///
    /// ```
    /// use kurbo::Rect;
    /// use martensite_devtools::error_surface::OverflowTape;
    /// use martensite_devtools::Axis;
    ///
    /// let tape = OverflowTape::new(Rect::ZERO, 5.0, Axis::Horizontal)
    ///     .with_colors([255, 0, 0, 255], [0, 0, 0, 255]);
    /// assert_eq!(tape.stripe_color_1[0], 255);
    /// ```
    #[must_use]
    pub fn with_colors(mut self, c1: [u8; 4], c2: [u8; 4]) -> Self {
        self.stripe_color_1 = c1;
        self.stripe_color_2 = c2;
        self
    }

    /// Sets a custom label string.
    ///
    /// # Examples
    ///
    /// ```
    /// use kurbo::Rect;
    /// use martensite_devtools::error_surface::OverflowTape;
    /// use martensite_devtools::Axis;
    ///
    /// let tape = OverflowTape::new(Rect::ZERO, 5.0, Axis::Horizontal)
    ///     .with_label("OVERFLOW 5px");
    /// assert_eq!(tape.label_text, "OVERFLOW 5px");
    /// ```
    #[must_use]
    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label_text = label.into();
        self
    }

    /// Renders the diagonal hatch pattern and badge into a [`PaintList`].
    ///
    /// # Examples
    ///
    /// ```
    /// use kurbo::Rect;
    /// use martensite_core::PaintList;
    /// use martensite_devtools::error_surface::OverflowTape;
    /// use martensite_devtools::Axis;
    ///
    /// let tape = OverflowTape::new(Rect::new(0.0, 0.0, 100.0, 20.0), 10.0, Axis::Horizontal);
    /// let mut paint = PaintList::new();
    /// tape.render(&mut paint);
    /// assert!(!paint.is_empty());
    /// ```
    pub fn render(&self, paint: &mut PaintList) {
        if self.edge_rect.width() <= 0.0 || self.edge_rect.height() <= 0.0 {
            return;
        }

        // 1. Draw solid dark base for the tape.
        paint.push_fill_rect(self.edge_rect, self.stripe_color_2);

        // 2. Clip stripes to the tape bounds.
        paint.push_clip(self.edge_rect);

        let w = self.edge_rect.width();
        let h = self.edge_rect.height();
        let stripe_width = 8.0;
        let stripe_step = 16.0;
        let mut offset = -h;
        while offset < w + h {
            let mut path = BezPath::new();
            path.move_to(Point::new(self.edge_rect.x0 + offset, self.edge_rect.y0));
            path.line_to(Point::new(
                self.edge_rect.x0 + offset + stripe_width,
                self.edge_rect.y0,
            ));
            path.line_to(Point::new(
                self.edge_rect.x0 + offset + stripe_width + h,
                self.edge_rect.y1,
            ));
            path.line_to(Point::new(
                self.edge_rect.x0 + offset + h,
                self.edge_rect.y1,
            ));
            path.close_path();
            paint.push_path(path, self.stripe_color_1);
            offset += stripe_step;
        }

        paint.pop_clip();

        // 3. Overflow badge with amount pill.
        let badge_width = (self.label_text.len() as f64 * 8.0 + 12.0).max(48.0);
        let badge_height = 18.0;
        let badge_x = (self.edge_rect.x1 - badge_width).max(self.edge_rect.x0);
        let badge_y = (self.edge_rect.y1 - badge_height).max(self.edge_rect.y0);
        let badge_rect = Rect::new(
            badge_x,
            badge_y,
            badge_x + badge_width,
            badge_y + badge_height,
        );

        paint.push_fill_rect(badge_rect, [15, 23, 42, 240]);
        paint.push_stroke_rect(badge_rect, 1.0, self.stripe_color_1);
        paint.push_text(
            Point::new(badge_x + 4.0, badge_y + 13.0),
            self.label_text.clone(),
            11.0,
            self.stripe_color_1,
        );
    }
}

/// Diagnostic representing a clipped text truncation error.
///
/// # Examples
///
/// ```
/// use kurbo::Rect;
/// use martensite_core::WidgetId;
/// use martensite_devtools::error_surface::ClippedTextDiagnostic;
///
/// let diag = ClippedTextDiagnostic::new(
///     WidgetId::from_parts(2, 1),
///     "App/Header/Title",
///     "Welcome to Martensite Studio Pro",
///     Rect::new(0.0, 0.0, 100.0, 20.0),
///     35.0,
/// );
/// assert!(diag.one_line_cause().contains("35.0px"));
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct ClippedTextDiagnostic {
    /// Widget arena ID associated with this text node.
    pub node: Option<WidgetId>,
    /// Linkable node path.
    pub node_path: String,
    /// The string content that was clipped.
    pub text: String,
    /// Bounding rectangle of the text widget in logical pixels.
    pub bounds: Rect,
    /// Pixel deficit by which the text was truncated.
    pub deficit_px: f32,
    /// Link to documentation on text layout and wrapping.
    pub doc_link: String,
}

impl ClippedTextDiagnostic {
    /// Creates a new `ClippedTextDiagnostic`.
    ///
    /// # Examples
    ///
    /// ```
    /// use kurbo::Rect;
    /// use martensite_devtools::error_surface::ClippedTextDiagnostic;
    ///
    /// let diag = ClippedTextDiagnostic::new(
    ///     None,
    ///     "Title",
    ///     "Hello World",
    ///     Rect::new(0.0, 0.0, 40.0, 15.0),
    ///     20.0,
    /// );
    /// assert_eq!(diag.deficit_px, 20.0);
    /// ```
    pub fn new(
        node: impl Into<Option<WidgetId>>,
        node_path: impl Into<String>,
        text: impl Into<String>,
        bounds: Rect,
        deficit_px: f32,
    ) -> Self {
        Self {
            node: node.into(),
            node_path: node_path.into(),
            text: text.into(),
            bounds,
            deficit_px,
            doc_link: "https://martensite.dev/docs/errors/text-clipping".to_string(),
        }
    }

    /// Sets a custom doc link.
    ///
    /// # Examples
    ///
    /// ```
    /// use kurbo::Rect;
    /// use martensite_devtools::error_surface::ClippedTextDiagnostic;
    ///
    /// let diag = ClippedTextDiagnostic::new(None, "T", "text", Rect::ZERO, 5.0)
    ///     .with_doc_link("https://docs/clip");
    /// assert_eq!(diag.doc_link, "https://docs/clip");
    /// ```
    #[must_use]
    pub fn with_doc_link(mut self, link: impl Into<String>) -> Self {
        self.doc_link = link.into();
        self
    }

    /// Returns a human-readable one-line description of the violation.
    ///
    /// # Examples
    ///
    /// ```
    /// use kurbo::Rect;
    /// use martensite_devtools::error_surface::ClippedTextDiagnostic;
    ///
    /// let diag = ClippedTextDiagnostic::new(None, "T", "Long text", Rect::ZERO, 14.5);
    /// assert!(diag.one_line_cause().contains("14.5px"));
    /// ```
    pub fn one_line_cause(&self) -> String {
        format!(
            "Clipped text: '{}' truncated by {:.1}px",
            self.text, self.deficit_px
        )
    }

    /// Converts this diagnostic to a Tier 2 diagnostic entry.
    ///
    /// # Examples
    ///
    /// ```
    /// use kurbo::Rect;
    /// use martensite_devtools::error_surface::ClippedTextDiagnostic;
    ///
    /// let diag = ClippedTextDiagnostic::new(None, "T", "text", Rect::ZERO, 5.0);
    /// let entry = diag.to_entry(1);
    /// assert_eq!(entry.node_path, "T");
    /// ```
    pub fn to_entry(&self, frame: u64) -> DiagnosticEntry {
        DiagnosticEntry {
            id: format!("clipped_text:{}", self.node_path),
            node_id: self.node,
            node_path: self.node_path.clone(),
            one_line_cause: self.one_line_cause(),
            doc_link: self.doc_link.clone(),
            severity: DiagnosticSeverity::Error,
            first_seen_frame: frame,
            last_seen_frame: frame,
            occurrences: 1,
        }
    }

    /// Renders an underline truncation marker along the bottom of the text bounds.
    ///
    /// # Examples
    ///
    /// ```
    /// use kurbo::Rect;
    /// use martensite_core::PaintList;
    /// use martensite_devtools::error_surface::ClippedTextDiagnostic;
    ///
    /// let diag = ClippedTextDiagnostic::new(None, "T", "text", Rect::new(0.0, 0.0, 50.0, 20.0), 10.0);
    /// let mut paint = PaintList::new();
    /// diag.render_marker(&mut paint);
    /// assert!(!paint.is_empty());
    /// ```
    pub fn render_marker(&self, paint: &mut PaintList) {
        if self.bounds.width() <= 0.0 || self.bounds.height() <= 0.0 {
            return;
        }

        let underline_rect = Rect::new(
            self.bounds.x0,
            (self.bounds.y1 - 2.5).max(self.bounds.y0),
            self.bounds.x1,
            self.bounds.y1,
        );
        paint.push_fill_rect(underline_rect, DEFAULT_ERROR_RED);

        if self.bounds.width() > 32.0 {
            let note = format!("clip -{:.0}px", self.deficit_px);
            paint.push_text(
                Point::new(self.bounds.x0, self.bounds.y1 + 10.0),
                note,
                9.0,
                DEFAULT_ERROR_RED,
            );
        }
    }
}

/// Diagnostic representing a design-lint finding (WCAG contrast, touch target, etc.).
///
/// # Examples
///
/// ```
/// use kurbo::Rect;
/// use martensite_core::WidgetId;
/// use martensite_devtools::error_surface::{DiagnosticSeverity, LintDiagnostic};
///
/// let diag = LintDiagnostic::new(
///     WidgetId::from_parts(5, 1),
///     "App/Button",
///     "wcag-contrast",
///     "Contrast ratio 2.1:1 fails 4.5:1 minimum",
///     DiagnosticSeverity::Error,
///     Rect::new(0.0, 0.0, 80.0, 32.0),
/// );
/// assert!(diag.severity.shows_inline());
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct LintDiagnostic {
    /// Widget arena ID associated with this finding.
    pub node: Option<WidgetId>,
    /// Linkable node path.
    pub node_path: String,
    /// Identifier of the lint rule that fired.
    pub rule_id: String,
    /// Explanation of the lint finding.
    pub message: String,
    /// Severity tier of the finding.
    pub severity: DiagnosticSeverity,
    /// Bounding rectangle of the widget in logical pixels.
    pub bounds: Rect,
    /// Link to documentation for this rule.
    pub doc_link: String,
}

impl LintDiagnostic {
    /// Creates a new `LintDiagnostic`.
    ///
    /// # Examples
    ///
    /// ```
    /// use kurbo::Rect;
    /// use martensite_devtools::error_surface::{DiagnosticSeverity, LintDiagnostic};
    ///
    /// let diag = LintDiagnostic::new(
    ///     None,
    ///     "Card",
    ///     "min-size",
    ///     "Target height 24px is below 44px minimum",
    ///     DiagnosticSeverity::Error,
    ///     Rect::ZERO,
    /// );
    /// assert_eq!(diag.rule_id, "min-size");
    /// ```
    pub fn new(
        node: impl Into<Option<WidgetId>>,
        node_path: impl Into<String>,
        rule_id: impl Into<String>,
        message: impl Into<String>,
        severity: DiagnosticSeverity,
        bounds: Rect,
    ) -> Self {
        let rule_str = rule_id.into();
        let doc_link = format!(
            "https://martensite.dev/docs/design-standards/rules/{}",
            rule_str
        );
        Self {
            node: node.into(),
            node_path: node_path.into(),
            rule_id: rule_str,
            message: message.into(),
            severity,
            bounds,
            doc_link,
        }
    }

    /// Sets a custom doc link.
    ///
    /// # Examples
    ///
    /// ```
    /// use kurbo::Rect;
    /// use martensite_devtools::error_surface::{DiagnosticSeverity, LintDiagnostic};
    ///
    /// let diag = LintDiagnostic::new(None, "Card", "rule", "msg", DiagnosticSeverity::Warn, Rect::ZERO)
    ///     .with_doc_link("https://docs/rule");
    /// assert_eq!(diag.doc_link, "https://docs/rule");
    /// ```
    #[must_use]
    pub fn with_doc_link(mut self, link: impl Into<String>) -> Self {
        self.doc_link = link.into();
        self
    }

    /// Returns a human-readable one-line description of the violation.
    ///
    /// # Examples
    ///
    /// ```
    /// use kurbo::Rect;
    /// use martensite_devtools::error_surface::{DiagnosticSeverity, LintDiagnostic};
    ///
    /// let diag = LintDiagnostic::new(None, "Card", "contrast", "Fails 4.5:1", DiagnosticSeverity::Error, Rect::ZERO);
    /// assert!(diag.one_line_cause().contains("[contrast]"));
    /// ```
    pub fn one_line_cause(&self) -> String {
        format!("Design lint [{}]: {}", self.rule_id, self.message)
    }

    /// Converts this diagnostic to a Tier 2 diagnostic entry.
    ///
    /// # Examples
    ///
    /// ```
    /// use kurbo::Rect;
    /// use martensite_devtools::error_surface::{DiagnosticSeverity, LintDiagnostic};
    ///
    /// let diag = LintDiagnostic::new(None, "Card", "rule", "msg", DiagnosticSeverity::Error, Rect::ZERO);
    /// let entry = diag.to_entry(1);
    /// assert_eq!(entry.severity, DiagnosticSeverity::Error);
    /// ```
    pub fn to_entry(&self, frame: u64) -> DiagnosticEntry {
        DiagnosticEntry {
            id: format!("lint:{}:{}", self.rule_id, self.node_path),
            node_id: self.node,
            node_path: self.node_path.clone(),
            one_line_cause: self.one_line_cause(),
            doc_link: self.doc_link.clone(),
            severity: self.severity,
            first_seen_frame: frame,
            last_seen_frame: frame,
            occurrences: 1,
        }
    }

    /// Renders a corner tick on the widget bounds (Tier 1 inline).
    ///
    /// # Examples
    ///
    /// ```
    /// use kurbo::Rect;
    /// use martensite_core::PaintList;
    /// use martensite_devtools::error_surface::{DiagnosticSeverity, LintDiagnostic};
    ///
    /// let diag = LintDiagnostic::new(None, "Card", "rule", "msg", DiagnosticSeverity::Error, Rect::new(0.0, 0.0, 50.0, 50.0));
    /// let mut paint = PaintList::new();
    /// diag.render_corner_tick(&mut paint);
    /// assert!(!paint.is_empty());
    /// ```
    pub fn render_corner_tick(&self, paint: &mut PaintList) {
        if !self.severity.shows_inline()
            || self.bounds.width() <= 0.0
            || self.bounds.height() <= 0.0
        {
            return;
        }

        let tick_size = 9.0;
        let tick_rect = Rect::new(
            (self.bounds.x1 - tick_size).max(self.bounds.x0),
            self.bounds.y0,
            self.bounds.x1,
            (self.bounds.y0 + tick_size).min(self.bounds.y1),
        );
        paint.push_fill_rect(tick_rect, DEFAULT_ERROR_RED);
    }
}

/// Diagnostic representing a paint error or backend rendering exception.
///
/// # Examples
///
/// ```
/// use kurbo::Rect;
/// use martensite_devtools::error_surface::PaintErrorDiagnostic;
///
/// let diag = PaintErrorDiagnostic::new(
///     "App/Canvas",
///     "Vello bump allocation exceeded buffer limit",
///     Some(Rect::new(0.0, 0.0, 800.0, 600.0)),
/// );
/// assert!(diag.one_line_cause().contains("Vello bump"));
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct PaintErrorDiagnostic {
    /// Linkable node path where the paint failure occurred.
    pub node_path: String,
    /// Error message or reason.
    pub message: String,
    /// Bounding rectangle where the error occurred, if applicable.
    pub bounds: Option<Rect>,
    /// Link to documentation on paint and rendering backend configuration.
    pub doc_link: String,
}

impl PaintErrorDiagnostic {
    /// Creates a new `PaintErrorDiagnostic`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::error_surface::PaintErrorDiagnostic;
    ///
    /// let diag = PaintErrorDiagnostic::new("Node", "Texture allocation failed", None);
    /// assert_eq!(diag.node_path, "Node");
    /// ```
    pub fn new(
        node_path: impl Into<String>,
        message: impl Into<String>,
        bounds: Option<Rect>,
    ) -> Self {
        Self {
            node_path: node_path.into(),
            message: message.into(),
            bounds,
            doc_link: "https://martensite.dev/docs/errors/paint".to_string(),
        }
    }

    /// Sets a custom doc link.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::error_surface::PaintErrorDiagnostic;
    ///
    /// let diag = PaintErrorDiagnostic::new("Node", "err", None)
    ///     .with_doc_link("https://docs/paint");
    /// assert_eq!(diag.doc_link, "https://docs/paint");
    /// ```
    #[must_use]
    pub fn with_doc_link(mut self, link: impl Into<String>) -> Self {
        self.doc_link = link.into();
        self
    }

    /// Returns a human-readable one-line description of the violation.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::error_surface::PaintErrorDiagnostic;
    ///
    /// let diag = PaintErrorDiagnostic::new("Node", "shader error", None);
    /// assert_eq!(diag.one_line_cause(), "Paint error: shader error");
    /// ```
    pub fn one_line_cause(&self) -> String {
        format!("Paint error: {}", self.message)
    }

    /// Converts this diagnostic to a Tier 2 diagnostic entry.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::error_surface::PaintErrorDiagnostic;
    ///
    /// let diag = PaintErrorDiagnostic::new("Node", "err", None);
    /// let entry = diag.to_entry(1);
    /// assert_eq!(entry.node_path, "Node");
    /// ```
    pub fn to_entry(&self, frame: u64) -> DiagnosticEntry {
        DiagnosticEntry {
            id: format!("paint_error:{}", self.node_path),
            node_id: None,
            node_path: self.node_path.clone(),
            one_line_cause: self.one_line_cause(),
            doc_link: self.doc_link.clone(),
            severity: DiagnosticSeverity::Error,
            first_seen_frame: frame,
            last_seen_frame: frame,
            occurrences: 1,
        }
    }
}

/// A single active Tier 1 ambient inline visual decoration drawn into the frame.
///
/// # Examples
///
/// ```
/// use kurbo::Rect;
/// use martensite_devtools::error_surface::{InlineAnnotation, OverflowTape};
/// use martensite_devtools::Axis;
///
/// let tape = OverflowTape::new(Rect::new(0.0, 0.0, 100.0, 10.0), 20.0, Axis::Horizontal);
/// let annotation = InlineAnnotation::LayoutOverflowTape(tape);
/// assert_eq!(annotation.kind_name(), "layout_overflow_tape");
/// ```
#[derive(Debug, Clone, PartialEq)]
pub enum InlineAnnotation {
    /// Flutter-style diagonal hatch tape on an overflowing widget edge.
    LayoutOverflowTape(OverflowTape),
    /// Red underline marker underneath truncated or clipped text.
    ClippedTextUnderline {
        /// Text bounds.
        bounds: Rect,
        /// Truncation deficit in pixels.
        deficit_px: f32,
        /// Decoration color.
        color: [u8; 4],
    },
    /// Red corner tick indicating an Error-severity design-lint finding.
    CornerTick {
        /// Widget bounds.
        bounds: Rect,
        /// Tick color.
        color: [u8; 4],
        /// Name of the firing rule.
        rule_id: String,
    },
    /// Collapsed banner indicator shown when diagnostics exceed the collapse cap.
    CollapsedSummary {
        /// Total number of active diagnostics.
        count: usize,
        /// Summary text string.
        summary: String,
    },
}

impl InlineAnnotation {
    /// Returns the name of this annotation type.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::error_surface::InlineAnnotation;
    ///
    /// let a = InlineAnnotation::CollapsedSummary { count: 5, summary: "5 errors".into() };
    /// assert_eq!(a.kind_name(), "collapsed_summary");
    /// ```
    pub fn kind_name(&self) -> &'static str {
        match self {
            Self::LayoutOverflowTape(..) => "layout_overflow_tape",
            Self::ClippedTextUnderline { .. } => "clipped_text_underline",
            Self::CornerTick { .. } => "corner_tick",
            Self::CollapsedSummary { .. } => "collapsed_summary",
        }
    }

    /// Renders this annotation into the given [`PaintList`].
    ///
    /// # Examples
    ///
    /// ```
    /// use kurbo::Rect;
    /// use martensite_core::PaintList;
    /// use martensite_devtools::error_surface::InlineAnnotation;
    ///
    /// let a = InlineAnnotation::CornerTick {
    ///     bounds: Rect::new(0.0, 0.0, 40.0, 40.0),
    ///     color: [239, 68, 68, 255],
    ///     rule_id: "contrast".into(),
    /// };
    /// let mut paint = PaintList::new();
    /// a.render(&mut paint);
    /// assert!(!paint.is_empty());
    /// ```
    pub fn render(&self, paint: &mut PaintList) {
        match self {
            Self::LayoutOverflowTape(tape) => tape.render(paint),
            Self::ClippedTextUnderline {
                bounds,
                deficit_px,
                color,
            } => {
                if bounds.width() <= 0.0 || bounds.height() <= 0.0 {
                    return;
                }
                let underline_rect = Rect::new(
                    bounds.x0,
                    (bounds.y1 - 2.5).max(bounds.y0),
                    bounds.x1,
                    bounds.y1,
                );
                paint.push_fill_rect(underline_rect, *color);
                if bounds.width() > 32.0 {
                    let note = format!("clip -{:.0}px", deficit_px);
                    paint.push_text(Point::new(bounds.x0, bounds.y1 + 10.0), note, 9.0, *color);
                }
            }
            Self::CornerTick { bounds, color, .. } => {
                if bounds.width() <= 0.0 || bounds.height() <= 0.0 {
                    return;
                }
                let tick_size = 9.0;
                let tick_rect = Rect::new(
                    (bounds.x1 - tick_size).max(bounds.x0),
                    bounds.y0,
                    bounds.x1,
                    (bounds.y0 + tick_size).min(bounds.y1),
                );
                paint.push_fill_rect(tick_rect, *color);
            }
            Self::CollapsedSummary { summary, .. } => {
                let banner_rect = Rect::new(0.0, 0.0, 360.0, 26.0);
                paint.push_fill_rect(banner_rect, [220, 38, 38, 245]);
                paint.push_stroke_rect(banner_rect, 1.0, [255, 255, 255, 200]);
                paint.push_text(
                    Point::new(10.0, 18.0),
                    summary.clone(),
                    12.0,
                    [255, 255, 255, 255],
                );
            }
        }
    }
}

/// A single Tier 2 diagnostic entry listed in the HUD section and inspector tab.
///
/// # Examples
///
/// ```
/// use martensite_core::WidgetId;
/// use martensite_devtools::error_surface::{DiagnosticEntry, DiagnosticSeverity};
///
/// let entry = DiagnosticEntry {
///     id: "diag:1".into(),
///     node_id: Some(WidgetId::from_parts(1, 1)),
///     node_path: "App/Header".into(),
///     one_line_cause: "Overflow 12px".into(),
///     doc_link: "https://docs".into(),
///     severity: DiagnosticSeverity::Error,
///     first_seen_frame: 100,
///     last_seen_frame: 340,
///     occurrences: 241,
/// };
///
/// assert_eq!(entry.frame_age(340), 241);
/// assert_eq!(entry.age_badge(340), "for 241f");
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct DiagnosticEntry {
    /// Unique identity key for deduplication.
    pub id: String,
    /// Optional arena widget handle.
    pub node_id: Option<WidgetId>,
    /// Linkable node path resolvable by inspector reveal.
    pub node_path: String,
    /// Human-readable one-line description of the cause.
    pub one_line_cause: String,
    /// Link to documentation.
    pub doc_link: String,
    /// Severity classification.
    pub severity: DiagnosticSeverity,
    /// Frame number where this diagnostic was first observed.
    pub first_seen_frame: u64,
    /// Frame number where this diagnostic was most recently observed.
    pub last_seen_frame: u64,
    /// Number of consecutive frames this diagnostic has fired.
    pub occurrences: u32,
}

impl DiagnosticEntry {
    /// Computes the age of this diagnostic in frames relative to `current_frame`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::error_surface::{DiagnosticEntry, DiagnosticSeverity};
    ///
    /// let entry = DiagnosticEntry {
    ///     id: "test".into(),
    ///     node_id: None,
    ///     node_path: "Root".into(),
    ///     one_line_cause: "msg".into(),
    ///     doc_link: "".into(),
    ///     severity: DiagnosticSeverity::Error,
    ///     first_seen_frame: 10,
    ///     last_seen_frame: 20,
    ///     occurrences: 11,
    /// };
    /// assert_eq!(entry.frame_age(20), 11);
    /// ```
    #[inline]
    pub fn frame_age(&self, current_frame: u64) -> u64 {
        current_frame.saturating_sub(self.first_seen_frame) + 1
    }

    /// Formats the frame-age badge string (e.g. `"for 240f"`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::error_surface::{DiagnosticEntry, DiagnosticSeverity};
    ///
    /// let entry = DiagnosticEntry {
    ///     id: "test".into(),
    ///     node_id: None,
    ///     node_path: "Root".into(),
    ///     one_line_cause: "msg".into(),
    ///     doc_link: "".into(),
    ///     severity: DiagnosticSeverity::Error,
    ///     first_seen_frame: 1,
    ///     last_seen_frame: 1,
    ///     occurrences: 1,
    /// };
    /// assert_eq!(entry.age_badge(240), "for 240f");
    /// ```
    #[inline]
    pub fn age_badge(&self, current_frame: u64) -> String {
        format!("for {}f", self.frame_age(current_frame))
    }
}

/// Lifecycle phase during which a panic or fatal exception occurred.
///
/// # Examples
///
/// ```
/// use martensite_devtools::error_surface::PanicPhase;
///
/// assert_eq!(PanicPhase::Paint.as_str(), "Paint");
/// assert_eq!(PanicPhase::Layout.as_str(), "Layout");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PanicPhase {
    /// Layout constraint resolution or measure phase.
    Layout,
    /// Paint dispatch or command recording phase.
    Paint,
    /// Event dispatch or hit-testing phase.
    Event,
    /// Unknown or outside tracked phases.
    Unknown,
}

impl PanicPhase {
    /// Returns the string name of this phase.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::error_surface::PanicPhase;
    ///
    /// assert_eq!(PanicPhase::Paint.as_str(), "Paint");
    /// ```
    #[inline]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Layout => "Layout",
            Self::Paint => "Paint",
            Self::Event => "Event",
            Self::Unknown => "Unknown",
        }
    }
}

/// Recovery action available after an in-app error or panic.
///
/// # Examples
///
/// ```
/// use martensite_devtools::error_surface::RecoveryPolicy;
///
/// assert!(RecoveryPolicy::ContinueWithPrunedNode.can_continue());
/// assert!(!RecoveryPolicy::RestartOnly.can_continue());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RecoveryPolicy {
    /// Safe to prune the offending node from the paint list and continue execution.
    ContinueWithPrunedNode,
    /// Invariant or tree geometry corrupted; application must restart.
    RestartOnly,
}

impl RecoveryPolicy {
    /// Returns `true` if the application can continue running without restarting.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::error_surface::RecoveryPolicy;
    ///
    /// assert!(RecoveryPolicy::ContinueWithPrunedNode.can_continue());
    /// assert!(!RecoveryPolicy::RestartOnly.can_continue());
    /// ```
    #[inline]
    pub const fn can_continue(&self) -> bool {
        matches!(self, Self::ContinueWithPrunedNode)
    }

    /// Short policy name.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::error_surface::RecoveryPolicy;
    ///
    /// assert_eq!(RecoveryPolicy::ContinueWithPrunedNode.as_str(), "Continue (prune node)");
    /// ```
    #[inline]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::ContinueWithPrunedNode => "Continue (prune node)",
            Self::RestartOnly => "Restart Required",
        }
    }

    /// Detailed description of the policy rationale.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::error_surface::RecoveryPolicy;
    ///
    /// assert!(RecoveryPolicy::ContinueWithPrunedNode.description().contains("pruned"));
    /// ```
    #[inline]
    pub const fn description(&self) -> &'static str {
        match self {
            Self::ContinueWithPrunedNode => {
                "The offending widget can be pruned from the paint pass safely. The application can continue running."
            }
            Self::RestartOnly => {
                "Layout state or core tree geometry invariant was violated. Restart is required to avoid invalid state."
            }
        }
    }
}

/// Complete diagnostic bundle captured when a structured dev panic occurs.
///
/// # Examples
///
/// ```
/// use martensite_core::WidgetId;
/// use martensite_devtools::error_surface::{CrashBundle, PanicPhase, RecoveryPolicy};
///
/// let bundle = CrashBundle::new(
///     "index out of bounds",
///     Some(WidgetId::from_parts(42, 1)),
///     "App/Grid/Cell[5]",
///     PanicPhase::Paint,
///     RecoveryPolicy::ContinueWithPrunedNode,
/// );
///
/// let report = bundle.format_copy_report();
/// assert!(report.contains("App/Grid/Cell[5]"));
/// assert!(report.contains("index out of bounds"));
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct CrashBundle {
    /// Panic message extracted from payload.
    pub panic_message: String,
    /// Arena ID of the widget being processed when panic occurred.
    pub in_flight_node: Option<WidgetId>,
    /// Tree path or `debug_name` of the in-flight widget.
    pub in_flight_path: String,
    /// Phase during which the panic occurred.
    pub phase: PanicPhase,
    /// Recovery policy offered by the crash surface.
    pub policy: RecoveryPolicy,
    /// Captured backtrace if `RUST_BACKTRACE` was enabled.
    pub backtrace: Option<String>,
    /// Diagnostic records from the last N event-ledger entries.
    pub recent_events: Vec<String>,
    /// Nanosecond timestamp when captured.
    pub timestamp_ns: u64,
}

impl CrashBundle {
    /// Creates a new `CrashBundle`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::error_surface::{CrashBundle, PanicPhase, RecoveryPolicy};
    ///
    /// let b = CrashBundle::new("failed", None, "Root", PanicPhase::Layout, RecoveryPolicy::RestartOnly);
    /// assert_eq!(b.phase, PanicPhase::Layout);
    /// ```
    pub fn new(
        panic_message: impl Into<String>,
        in_flight_node: Option<WidgetId>,
        in_flight_path: impl Into<String>,
        phase: PanicPhase,
        policy: RecoveryPolicy,
    ) -> Self {
        let timestamp_ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);

        Self {
            panic_message: panic_message.into(),
            in_flight_node,
            in_flight_path: in_flight_path.into(),
            phase,
            policy,
            backtrace: None,
            recent_events: Vec::new(),
            timestamp_ns,
        }
    }

    /// Sets the captured backtrace string.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::error_surface::{CrashBundle, PanicPhase, RecoveryPolicy};
    ///
    /// let b = CrashBundle::new("err", None, "R", PanicPhase::Unknown, RecoveryPolicy::RestartOnly)
    ///     .with_backtrace("stack frame 1\nstack frame 2");
    /// assert!(b.backtrace.is_some());
    /// ```
    #[must_use]
    pub fn with_backtrace(mut self, bt: impl Into<String>) -> Self {
        self.backtrace = Some(bt.into());
        self
    }

    /// Sets an optional backtrace string.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::error_surface::{CrashBundle, PanicPhase, RecoveryPolicy};
    ///
    /// let b = CrashBundle::new("err", None, "R", PanicPhase::Unknown, RecoveryPolicy::RestartOnly)
    ///     .with_backtrace_opt(None);
    /// assert!(b.backtrace.is_none());
    /// ```
    #[must_use]
    pub fn with_backtrace_opt(mut self, bt: Option<String>) -> Self {
        self.backtrace = bt;
        self
    }

    /// Sets the list of recent event-ledger diagnostics.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::error_surface::{CrashBundle, PanicPhase, RecoveryPolicy};
    ///
    /// let b = CrashBundle::new("err", None, "R", PanicPhase::Unknown, RecoveryPolicy::RestartOnly)
    ///     .with_recent_events(vec!["[01] Pointer Press at (10, 20)".into()]);
    /// assert_eq!(b.recent_events.len(), 1);
    /// ```
    #[must_use]
    pub fn with_recent_events(mut self, events: Vec<String>) -> Self {
        self.recent_events = events;
        self
    }

    /// Sets an explicit timestamp.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::error_surface::{CrashBundle, PanicPhase, RecoveryPolicy};
    ///
    /// let b = CrashBundle::new("err", None, "R", PanicPhase::Unknown, RecoveryPolicy::RestartOnly)
    ///     .with_timestamp(1_000_000);
    /// assert_eq!(b.timestamp_ns, 1_000_000);
    /// ```
    #[must_use]
    pub fn with_timestamp(mut self, ns: u64) -> Self {
        self.timestamp_ns = ns;
        self
    }

    /// Formats the crash bundle as a structured, copy-pasteable Markdown report.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::error_surface::{CrashBundle, PanicPhase, RecoveryPolicy};
    ///
    /// let b = CrashBundle::new("assertion failed", None, "Root/Sidebar", PanicPhase::Paint, RecoveryPolicy::ContinueWithPrunedNode);
    /// let text = b.format_copy_report();
    /// assert!(text.contains("MARTENSITE DEV CRASH REPORT"));
    /// assert!(text.contains("assertion failed"));
    /// ```
    pub fn format_copy_report(&self) -> String {
        let mut out = String::new();
        out.push_str("=== MARTENSITE DEV CRASH REPORT ===\n");
        out.push_str(&format!("Panic Message: {}\n", self.panic_message));
        out.push_str(&format!("Phase: {}\n", self.phase.as_str()));
        out.push_str(&format!("Policy: {}\n", self.policy.as_str()));
        out.push_str(&format!("Policy Note: {}\n", self.policy.description()));
        if let Some(id) = self.in_flight_node {
            out.push_str(&format!("In-Flight Node: WidgetId({:?})\n", id));
        } else {
            out.push_str("In-Flight Node: <none>\n");
        }
        out.push_str(&format!("In-Flight Path: {}\n", self.in_flight_path));
        out.push_str(&format!("Timestamp: {} ns\n", self.timestamp_ns));

        if let Some(ref bt) = self.backtrace {
            out.push_str("\n--- Backtrace ---\n");
            out.push_str(bt);
            out.push('\n');
        }

        out.push_str("\n--- Recent Event Ledger ---\n");
        if self.recent_events.is_empty() {
            out.push_str("(No recent event ledger records)\n");
        } else {
            for (i, ev) in self.recent_events.iter().enumerate() {
                out.push_str(&format!("[{:02}] {}\n", i + 1, ev));
            }
        }
        out.push_str("===================================\n");
        out
    }

    /// Renders an in-app error card surface for this panic into a [`PaintList`].
    ///
    /// # Examples
    ///
    /// ```
    /// use glam::Vec2;
    /// use martensite_core::PaintList;
    /// use martensite_devtools::error_surface::{CrashBundle, PanicPhase, RecoveryPolicy};
    ///
    /// let b = CrashBundle::new("fail", None, "Node", PanicPhase::Paint, RecoveryPolicy::ContinueWithPrunedNode);
    /// let mut paint = PaintList::new();
    /// b.render_card(&mut paint, Vec2::new(1024.0, 768.0));
    /// assert!(!paint.is_empty());
    /// ```
    pub fn render_card(&self, paint: &mut PaintList, screen_size: Vec2) {
        let screen_w = (screen_size.x as f64).max(800.0);
        let screen_h = (screen_size.y as f64).max(600.0);

        // 1. Semi-transparent backdrop overlay.
        paint.push_fill_rect(Rect::new(0.0, 0.0, screen_w, screen_h), [0, 0, 0, 180]);

        // 2. Card container.
        let card_w = 640.0;
        let card_h = 360.0;
        let card_x = (screen_w - card_w) * 0.5;
        let card_y = (screen_h - card_h) * 0.5;
        let card_rect = Rect::new(card_x, card_y, card_x + card_w, card_y + card_h);

        paint.push_fill_rect(card_rect, [24, 24, 27, 255]);
        paint.push_stroke_rect(card_rect, 2.0, [220, 38, 38, 255]);

        // 3. Header bar.
        let header_rect = Rect::new(card_x, card_y, card_x + card_w, card_y + 40.0);
        paint.push_fill_rect(header_rect, [220, 38, 38, 255]);
        paint.push_text(
            Point::new(card_x + 16.0, card_y + 26.0),
            format!(
                "MARTENSITE DEV PANIC — {} PHASE",
                self.phase.as_str().to_uppercase()
            ),
            15.0,
            [255, 255, 255, 255],
        );

        // 4. Panic message.
        let msg_rect = Rect::new(
            card_x + 16.0,
            card_y + 52.0,
            card_x + card_w - 16.0,
            card_y + 110.0,
        );
        paint.push_fill_rect(msg_rect, [39, 39, 42, 255]);
        paint.push_text(
            Point::new(card_x + 24.0, card_y + 78.0),
            format!("Error: {}", self.panic_message),
            13.0,
            [254, 202, 202, 255],
        );

        // 5. In-flight node & path.
        paint.push_text(
            Point::new(card_x + 16.0, card_y + 135.0),
            format!("In-Flight Node Path: {}", self.in_flight_path),
            12.0,
            [228, 228, 231, 255],
        );

        // 6. Recovery policy badge.
        let policy_color = if self.policy.can_continue() {
            [34, 197, 94, 255]
        } else {
            [239, 68, 68, 255]
        };
        paint.push_text(
            Point::new(card_x + 16.0, card_y + 160.0),
            format!(
                "Policy: {} — {}",
                self.policy.as_str(),
                self.policy.description()
            ),
            12.0,
            policy_color,
        );

        // 7. Recent events count note.
        paint.push_text(
            Point::new(card_x + 16.0, card_y + 185.0),
            format!(
                "Crash bundle captured {} recent event ledger records.",
                self.recent_events.len()
            ),
            11.0,
            [161, 161, 170, 255],
        );

        // 8. Action buttons.
        let copy_btn = Rect::new(
            card_x + 16.0,
            card_y + card_h - 48.0,
            card_x + 150.0,
            card_y + card_h - 16.0,
        );
        paint.push_fill_rect(copy_btn, [59, 130, 246, 255]);
        paint.push_text(
            Point::new(card_x + 32.0, card_y + card_h - 28.0),
            "Copy Report".to_string(),
            12.0,
            [255, 255, 255, 255],
        );

        let action_btn = Rect::new(
            card_x + 165.0,
            card_y + card_h - 48.0,
            card_x + 295.0,
            card_y + card_h - 16.0,
        );
        if self.policy.can_continue() {
            paint.push_fill_rect(action_btn, [34, 197, 94, 255]);
            paint.push_text(
                Point::new(card_x + 195.0, card_y + card_h - 28.0),
                "Continue".to_string(),
                12.0,
                [255, 255, 255, 255],
            );
        } else {
            paint.push_fill_rect(action_btn, [239, 68, 68, 255]);
            paint.push_text(
                Point::new(card_x + 195.0, card_y + card_h - 28.0),
                "Restart".to_string(),
                12.0,
                [255, 255, 255, 255],
            );
        }
    }
}

/// In-flight dispatch context used for pinpointing offending nodes during panics.
///
/// # Examples
///
/// ```
/// use martensite_devtools::error_surface::{InFlightContext, PanicPhase};
///
/// let ctx = InFlightContext {
///     node: None,
///     path: "Root/Body".into(),
///     phase: PanicPhase::Paint,
/// };
/// assert_eq!(ctx.phase, PanicPhase::Paint);
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct InFlightContext {
    /// Arena ID of the currently dispatched widget.
    pub node: Option<WidgetId>,
    /// Path or debug name of the currently dispatched widget.
    pub path: String,
    /// Active engine processing phase.
    pub phase: PanicPhase,
}

/// Sets the current thread-local in-flight widget context.
///
/// # Examples
///
/// ```
/// use martensite_devtools::error_surface::{current_in_flight, set_in_flight, PanicPhase};
///
/// set_in_flight(None, "App/Header", PanicPhase::Paint);
/// let cur = current_in_flight().unwrap();
/// assert_eq!(cur.path, "App/Header");
/// ```
pub fn set_in_flight(node: Option<WidgetId>, path: impl Into<String>, phase: PanicPhase) {
    IN_FLIGHT_CONTEXT.with(|c| {
        *c.borrow_mut() = Some(InFlightContext {
            node,
            path: path.into(),
            phase,
        });
    });
}

/// Clears the thread-local in-flight widget context.
///
/// # Examples
///
/// ```
/// use martensite_devtools::error_surface::{clear_in_flight, current_in_flight, set_in_flight, PanicPhase};
///
/// set_in_flight(None, "App", PanicPhase::Layout);
/// clear_in_flight();
/// assert!(current_in_flight().is_none());
/// ```
pub fn clear_in_flight() {
    IN_FLIGHT_CONTEXT.with(|c| {
        *c.borrow_mut() = None;
    });
}

/// Retrieves the active thread-local in-flight widget context.
///
/// # Examples
///
/// ```
/// use martensite_devtools::error_surface::{clear_in_flight, current_in_flight};
///
/// clear_in_flight();
/// assert!(current_in_flight().is_none());
/// ```
pub fn current_in_flight() -> Option<InFlightContext> {
    IN_FLIGHT_CONTEXT.with(|c| c.borrow().clone())
}

/// Executes a closure within an in-flight tracking scope.
///
/// Automatically restores the previous in-flight context on return or unwind.
///
/// # Examples
///
/// ```
/// use martensite_devtools::error_surface::{current_in_flight, scope_in_flight, PanicPhase};
///
/// let result = scope_in_flight(None, "TestWidget", PanicPhase::Paint, || {
///     assert_eq!(current_in_flight().unwrap().path, "TestWidget");
///     42
/// });
/// assert_eq!(result, 42);
/// ```
pub fn scope_in_flight<F, R>(
    node: Option<WidgetId>,
    path: impl Into<String>,
    phase: PanicPhase,
    f: F,
) -> R
where
    F: FnOnce() -> R,
{
    let prev = current_in_flight();
    set_in_flight(node, path, phase);
    let res = f();
    if let Some(p) = prev {
        set_in_flight(p.node, p.path, p.phase);
    } else {
        clear_in_flight();
    }
    res
}

/// Returns whether the dev panic overlay is enabled via `MARTENSITE_DEV_PANIC=overlay`
/// or programmatic configuration.
///
/// # Examples
///
/// ```
/// use martensite_devtools::error_surface::{is_dev_panic_enabled, set_dev_panic_enabled};
///
/// set_dev_panic_enabled(true);
/// assert!(is_dev_panic_enabled());
/// set_dev_panic_enabled(false);
/// assert!(!is_dev_panic_enabled());
/// ```
pub fn is_dev_panic_enabled() -> bool {
    if !DEV_PANIC_CHECKED.load(Ordering::Relaxed) {
        let env_enabled = std::env::var("MARTENSITE_DEV_PANIC")
            .map(|v| {
                v == "1" || v.eq_ignore_ascii_case("overlay") || v.eq_ignore_ascii_case("true")
            })
            .unwrap_or(false);
        if env_enabled {
            DEV_PANIC_ENABLED.store(true, Ordering::Relaxed);
        }
        DEV_PANIC_CHECKED.store(true, Ordering::Relaxed);
    }
    DEV_PANIC_ENABLED.load(Ordering::Relaxed)
}

/// Sets whether the dev panic overlay is programmatically enabled.
///
/// # Examples
///
/// ```
/// use martensite_devtools::error_surface::{is_dev_panic_enabled, set_dev_panic_enabled};
///
/// set_dev_panic_enabled(true);
/// assert!(is_dev_panic_enabled());
/// ```
pub fn set_dev_panic_enabled(enabled: bool) {
    DEV_PANIC_ENABLED.store(enabled, Ordering::Relaxed);
    DEV_PANIC_CHECKED.store(true, Ordering::Relaxed);
}

/// Installs the structured dev-mode panic hook.
///
/// When enabled, catches panics, captures in-flight node context and backtrace,
/// formats the crash report to stderr, and records the crash bundle for overlay rendering.
/// If dev panic mode is disabled, delegates to the existing default panic handler.
///
/// # Examples
///
/// ```
/// use martensite_devtools::error_surface::install_dev_panic_hook;
///
/// install_dev_panic_hook();
/// ```
pub fn install_dev_panic_hook() {
    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if is_dev_panic_enabled() {
            let msg = if let Some(s) = info.payload().downcast_ref::<&str>() {
                (*s).to_string()
            } else if let Some(s) = info.payload().downcast_ref::<String>() {
                s.clone()
            } else {
                "Box<Any> panic payload".to_string()
            };

            let in_flight = current_in_flight();
            let (node, path, phase) = match in_flight {
                Some(ctx) => (ctx.node, ctx.path, ctx.phase),
                None => (None, "<unknown>".to_string(), PanicPhase::Unknown),
            };

            let policy = match phase {
                PanicPhase::Paint => RecoveryPolicy::ContinueWithPrunedNode,
                _ => RecoveryPolicy::RestartOnly,
            };

            let backtrace = if std::env::var("RUST_BACKTRACE")
                .map(|v| v != "0")
                .unwrap_or(false)
            {
                Some(format!("{:?}", std::backtrace::Backtrace::capture()))
            } else {
                None
            };

            let bundle =
                CrashBundle::new(msg, node, path, phase, policy).with_backtrace_opt(backtrace);

            if let Ok(mut lock) = GLOBAL_LAST_PANIC.lock() {
                *lock = Some(bundle.clone());
            }

            eprintln!("{}", bundle.format_copy_report());
        } else {
            prev_hook(info);
        }
    }));
}

/// Takes and clears the last recorded global crash bundle, if any.
///
/// # Examples
///
/// ```
/// use martensite_devtools::error_surface::take_last_panic;
///
/// let _ = take_last_panic();
/// ```
pub fn take_last_panic() -> Option<CrashBundle> {
    GLOBAL_LAST_PANIC.lock().ok()?.take()
}

/// Peeks at the last recorded global crash bundle without clearing it.
///
/// # Examples
///
/// ```
/// use martensite_devtools::error_surface::peek_last_panic;
///
/// let _ = peek_last_panic();
/// ```
pub fn peek_last_panic() -> Option<CrashBundle> {
    GLOBAL_LAST_PANIC.lock().ok()?.clone()
}

/// Primary orchestrator for the dev-mode error surface across Tiers 1, 2, and 3.
///
/// # Examples
///
/// ```
/// use glam::Vec2;
/// use kurbo::Rect;
/// use martensite_devtools::error_surface::{ErrorSurface, LayoutDiagnostic};
///
/// let mut surface = ErrorSurface::new();
/// assert!(!surface.has_diagnostics());
///
/// surface.record_layout_diagnostic(LayoutDiagnostic::new(
///     None,
///     "App/Content",
///     Rect::new(0.0, 0.0, 100.0, 50.0),
///     Vec2::new(100.0, 50.0),
///     Vec2::new(120.0, 50.0),
/// ));
///
/// assert!(surface.has_diagnostics());
/// assert_eq!(surface.tier2_entries().len(), 1);
/// ```
#[derive(Debug, Clone)]
pub struct ErrorSurface {
    active: bool,
    current_frame: u64,
    layout_diagnostics: Vec<LayoutDiagnostic>,
    clipped_text_diagnostics: Vec<ClippedTextDiagnostic>,
    lint_diagnostics: Vec<LintDiagnostic>,
    paint_diagnostics: Vec<PaintErrorDiagnostic>,
    tracked_entries: HashMap<String, DiagnosticEntry>,
    max_inline_annotations: usize,
    last_crash: Option<CrashBundle>,
}

impl Default for ErrorSurface {
    fn default() -> Self {
        Self::new()
    }
}

impl ErrorSurface {
    /// Creates a new active `ErrorSurface` with default configuration.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::error_surface::ErrorSurface;
    ///
    /// let surface = ErrorSurface::new();
    /// assert!(surface.is_active());
    /// assert_eq!(surface.current_frame(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            active: true,
            current_frame: 0,
            layout_diagnostics: Vec::new(),
            clipped_text_diagnostics: Vec::new(),
            lint_diagnostics: Vec::new(),
            paint_diagnostics: Vec::new(),
            tracked_entries: HashMap::new(),
            max_inline_annotations: DEFAULT_MAX_INLINE_ANNOTATIONS,
            last_crash: None,
        }
    }

    /// Sets the maximum number of inline annotations before collapsing to summary.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::error_surface::ErrorSurface;
    ///
    /// let surface = ErrorSurface::new().with_max_inline(100);
    /// assert!(!surface.is_collapsed());
    /// ```
    #[must_use]
    pub fn with_max_inline(mut self, max: usize) -> Self {
        self.max_inline_annotations = max;
        self
    }

    /// Returns `true` if the error surface is enabled.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::error_surface::ErrorSurface;
    ///
    /// let surface = ErrorSurface::new();
    /// assert!(surface.is_active());
    /// ```
    #[inline]
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Enables or disables the error surface.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::error_surface::ErrorSurface;
    ///
    /// let mut surface = ErrorSurface::new();
    /// surface.set_active(false);
    /// assert!(!surface.is_active());
    /// ```
    #[inline]
    pub fn set_active(&mut self, active: bool) {
        self.active = active;
    }

    /// Returns the current frame index.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::error_surface::ErrorSurface;
    ///
    /// let surface = ErrorSurface::new();
    /// assert_eq!(surface.current_frame(), 0);
    /// ```
    #[inline]
    pub fn current_frame(&self) -> u64 {
        self.current_frame
    }

    /// Sets the current frame index.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::error_surface::ErrorSurface;
    ///
    /// let mut surface = ErrorSurface::new();
    /// surface.set_current_frame(120);
    /// assert_eq!(surface.current_frame(), 120);
    /// ```
    #[inline]
    pub fn set_current_frame(&mut self, frame: u64) {
        self.current_frame = frame;
    }

    /// Advances the current frame counter by 1.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::error_surface::ErrorSurface;
    ///
    /// let mut surface = ErrorSurface::new();
    /// surface.advance_frame();
    /// assert_eq!(surface.current_frame(), 1);
    /// ```
    #[inline]
    pub fn advance_frame(&mut self) {
        self.current_frame = self.current_frame.saturating_add(1);
    }

    /// Clears per-frame diagnostics while preserving deduplication history in `tracked_entries`.
    ///
    /// # Examples
    ///
    /// ```
    /// use glam::Vec2;
    /// use kurbo::Rect;
    /// use martensite_devtools::error_surface::{ErrorSurface, LayoutDiagnostic};
    ///
    /// let mut surface = ErrorSurface::new();
    /// surface.record_layout_diagnostic(LayoutDiagnostic::new(None, "Node", Rect::ZERO, Vec2::new(10.0, 10.0), Vec2::new(20.0, 10.0)));
    /// assert!(surface.has_diagnostics());
    /// surface.clear_frame();
    /// assert!(!surface.has_diagnostics());
    /// ```
    pub fn clear_frame(&mut self) {
        self.layout_diagnostics.clear();
        self.clipped_text_diagnostics.clear();
        self.lint_diagnostics.clear();
        self.paint_diagnostics.clear();
    }

    /// Records a layout constraint violation or overflow diagnostic.
    ///
    /// # Examples
    ///
    /// ```
    /// use glam::Vec2;
    /// use kurbo::Rect;
    /// use martensite_devtools::error_surface::{ErrorSurface, LayoutDiagnostic};
    ///
    /// let mut surface = ErrorSurface::new();
    /// surface.record_layout_diagnostic(LayoutDiagnostic::new(
    ///     None,
    ///     "App/Content",
    ///     Rect::new(0.0, 0.0, 100.0, 50.0),
    ///     Vec2::new(100.0, 50.0),
    ///     Vec2::new(130.0, 50.0),
    /// ));
    /// assert_eq!(surface.diagnostic_count(), 1);
    /// ```
    pub fn record_layout_diagnostic(&mut self, diag: LayoutDiagnostic) {
        let entry = diag.to_entry(self.current_frame);
        self.upsert_tracked_entry(entry);
        self.layout_diagnostics.push(diag);
    }

    /// Records a clipped text truncation diagnostic.
    ///
    /// # Examples
    ///
    /// ```
    /// use kurbo::Rect;
    /// use martensite_devtools::error_surface::{ClippedTextDiagnostic, ErrorSurface};
    ///
    /// let mut surface = ErrorSurface::new();
    /// surface.record_clipped_text(ClippedTextDiagnostic::new(
    ///     None,
    ///     "App/Title",
    ///     "Very long title text",
    ///     Rect::new(0.0, 0.0, 50.0, 20.0),
    ///     25.0,
    /// ));
    /// assert_eq!(surface.diagnostic_count(), 1);
    /// ```
    pub fn record_clipped_text(&mut self, diag: ClippedTextDiagnostic) {
        let entry = diag.to_entry(self.current_frame);
        self.upsert_tracked_entry(entry);
        self.clipped_text_diagnostics.push(diag);
    }

    /// Records a design-lint finding diagnostic.
    ///
    /// # Examples
    ///
    /// ```
    /// use kurbo::Rect;
    /// use martensite_devtools::error_surface::{DiagnosticSeverity, ErrorSurface, LintDiagnostic};
    ///
    /// let mut surface = ErrorSurface::new();
    /// surface.record_lint_diagnostic(LintDiagnostic::new(
    ///     None,
    ///     "App/Button",
    ///     "contrast",
    ///     "Low contrast",
    ///     DiagnosticSeverity::Error,
    ///     Rect::ZERO,
    /// ));
    /// assert_eq!(surface.diagnostic_count(), 1);
    /// ```
    pub fn record_lint_diagnostic(&mut self, diag: LintDiagnostic) {
        let entry = diag.to_entry(self.current_frame);
        self.upsert_tracked_entry(entry);
        self.lint_diagnostics.push(diag);
    }

    /// Records a paint error or backend rendering exception.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::error_surface::{ErrorSurface, PaintErrorDiagnostic};
    ///
    /// let mut surface = ErrorSurface::new();
    /// surface.record_paint_error(PaintErrorDiagnostic::new("Canvas", "Out of memory", None));
    /// assert_eq!(surface.diagnostic_count(), 1);
    /// ```
    pub fn record_paint_error(&mut self, diag: PaintErrorDiagnostic) {
        let entry = diag.to_entry(self.current_frame);
        self.upsert_tracked_entry(entry);
        self.paint_diagnostics.push(diag);
    }

    fn upsert_tracked_entry(&mut self, entry: DiagnosticEntry) {
        match self.tracked_entries.get_mut(&entry.id) {
            Some(existing) => {
                existing.last_seen_frame = self.current_frame;
                existing.occurrences = existing.occurrences.saturating_add(1);
                existing.one_line_cause = entry.one_line_cause;
            }
            None => {
                self.tracked_entries.insert(entry.id.clone(), entry);
            }
        }
    }

    /// Returns `true` if there are any active diagnostics recorded in the current frame.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::error_surface::ErrorSurface;
    ///
    /// let surface = ErrorSurface::new();
    /// assert!(!surface.has_diagnostics());
    /// ```
    #[inline]
    pub fn has_diagnostics(&self) -> bool {
        !self.layout_diagnostics.is_empty()
            || !self.clipped_text_diagnostics.is_empty()
            || !self.lint_diagnostics.is_empty()
            || !self.paint_diagnostics.is_empty()
    }

    /// Returns the total number of diagnostics recorded in the current frame.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::error_surface::ErrorSurface;
    ///
    /// let surface = ErrorSurface::new();
    /// assert_eq!(surface.diagnostic_count(), 0);
    /// ```
    #[inline]
    pub fn diagnostic_count(&self) -> usize {
        self.layout_diagnostics.len()
            + self.clipped_text_diagnostics.len()
            + self.lint_diagnostics.len()
            + self.paint_diagnostics.len()
    }

    /// Returns `true` if total diagnostics exceed the collapse cap threshold.
    ///
    /// # Examples
    ///
    /// ```
    /// use glam::Vec2;
    /// use kurbo::Rect;
    /// use martensite_devtools::error_surface::{ErrorSurface, LayoutDiagnostic};
    ///
    /// let mut surface = ErrorSurface::new().with_max_inline(1);
    /// surface.record_layout_diagnostic(LayoutDiagnostic::new(None, "A", Rect::ZERO, Vec2::new(10.0, 10.0), Vec2::new(20.0, 10.0)));
    /// surface.record_layout_diagnostic(LayoutDiagnostic::new(None, "B", Rect::ZERO, Vec2::new(10.0, 10.0), Vec2::new(20.0, 10.0)));
    /// assert!(surface.is_collapsed());
    /// ```
    #[inline]
    pub fn is_collapsed(&self) -> bool {
        self.diagnostic_count() > self.max_inline_annotations
    }

    /// Returns the collapsed summary text string if currently in collapsed state.
    ///
    /// # Examples
    ///
    /// ```
    /// use glam::Vec2;
    /// use kurbo::Rect;
    /// use martensite_devtools::error_surface::{ErrorSurface, LayoutDiagnostic};
    ///
    /// let mut surface = ErrorSurface::new().with_max_inline(1);
    /// surface.record_layout_diagnostic(LayoutDiagnostic::new(None, "A", Rect::ZERO, Vec2::new(10.0, 10.0), Vec2::new(20.0, 10.0)));
    /// surface.record_layout_diagnostic(LayoutDiagnostic::new(None, "B", Rect::ZERO, Vec2::new(10.0, 10.0), Vec2::new(20.0, 10.0)));
    /// assert_eq!(surface.collapsed_summary(), Some("2 diagnostics — open inspector".to_string()));
    /// ```
    pub fn collapsed_summary(&self) -> Option<String> {
        if self.is_collapsed() {
            Some(format!(
                "{} diagnostics — open inspector",
                self.diagnostic_count()
            ))
        } else {
            None
        }
    }

    /// Collects active Tier 1 ambient inline annotations respecting severity and collapse cap.
    ///
    /// # Examples
    ///
    /// ```
    /// use glam::Vec2;
    /// use kurbo::Rect;
    /// use martensite_devtools::error_surface::{ErrorSurface, LayoutDiagnostic};
    ///
    /// let mut surface = ErrorSurface::new();
    /// surface.record_layout_diagnostic(LayoutDiagnostic::new(
    ///     None,
    ///     "Row",
    ///     Rect::new(0.0, 0.0, 100.0, 50.0),
    ///     Vec2::new(100.0, 50.0),
    ///     Vec2::new(125.0, 50.0),
    /// ));
    /// let annotations = surface.collect_tier1_annotations();
    /// assert_eq!(annotations.len(), 1);
    /// ```
    pub fn collect_tier1_annotations(&self) -> Vec<InlineAnnotation> {
        if !self.active || !self.has_diagnostics() {
            return Vec::new();
        }

        if let Some(summary) = self.collapsed_summary() {
            return vec![InlineAnnotation::CollapsedSummary {
                count: self.diagnostic_count(),
                summary,
            }];
        }

        let mut annotations = Vec::with_capacity(self.diagnostic_count());

        for layout in &self.layout_diagnostics {
            annotations.push(InlineAnnotation::LayoutOverflowTape(
                layout.to_tape_metadata(10.0),
            ));
        }

        for text in &self.clipped_text_diagnostics {
            annotations.push(InlineAnnotation::ClippedTextUnderline {
                bounds: text.bounds,
                deficit_px: text.deficit_px,
                color: DEFAULT_ERROR_RED,
            });
        }

        for lint in &self.lint_diagnostics {
            if lint.severity.shows_inline() {
                annotations.push(InlineAnnotation::CornerTick {
                    bounds: lint.bounds,
                    color: lint.severity.badge_color(),
                    rule_id: lint.rule_id.clone(),
                });
            }
        }

        annotations
    }

    /// Renders Tier 1 ambient inline decorations into the frame's [`PaintList`].
    ///
    /// If there are 0 diagnostics or `!is_active()`, this function performs zero allocations
    /// and pushes zero commands into `paint`.
    ///
    /// # Examples
    ///
    /// ```
    /// use glam::Vec2;
    /// use kurbo::Rect;
    /// use martensite_core::PaintList;
    /// use martensite_devtools::error_surface::{ErrorSurface, LayoutDiagnostic};
    ///
    /// let mut surface = ErrorSurface::new();
    /// let mut paint = PaintList::new();
    /// surface.render_tier1_annotations(&mut paint);
    /// assert!(paint.is_empty());
    ///
    /// surface.record_layout_diagnostic(LayoutDiagnostic::new(
    ///     None,
    ///     "Row",
    ///     Rect::new(0.0, 0.0, 100.0, 50.0),
    ///     Vec2::new(100.0, 50.0),
    ///     Vec2::new(125.0, 50.0),
    /// ));
    /// surface.render_tier1_annotations(&mut paint);
    /// assert!(!paint.is_empty());
    /// ```
    pub fn render_tier1_annotations(&self, paint: &mut PaintList) {
        if !self.active || !self.has_diagnostics() {
            return;
        }

        let annotations = self.collect_tier1_annotations();
        for ann in annotations {
            ann.render(paint);
        }
    }

    /// Returns a list of all active Tier 2 diagnostic entries for the HUD/inspector.
    ///
    /// # Examples
    ///
    /// ```
    /// use glam::Vec2;
    /// use kurbo::Rect;
    /// use martensite_devtools::error_surface::{ErrorSurface, LayoutDiagnostic};
    ///
    /// let mut surface = ErrorSurface::new();
    /// surface.record_layout_diagnostic(LayoutDiagnostic::new(None, "R", Rect::ZERO, Vec2::new(10.0, 10.0), Vec2::new(20.0, 10.0)));
    /// assert_eq!(surface.tier2_entries().len(), 1);
    /// ```
    pub fn tier2_entries(&self) -> Vec<&DiagnosticEntry> {
        self.tracked_entries.values().collect()
    }

    /// Returns a filtered list of Tier 2 diagnostic entries meeting `min_severity`.
    ///
    /// # Examples
    ///
    /// ```
    /// use kurbo::Rect;
    /// use martensite_devtools::error_surface::{DiagnosticSeverity, ErrorSurface, LintDiagnostic};
    ///
    /// let mut surface = ErrorSurface::new();
    /// surface.record_lint_diagnostic(LintDiagnostic::new(None, "W1", "r1", "warn", DiagnosticSeverity::Warn, Rect::ZERO));
    /// surface.record_lint_diagnostic(LintDiagnostic::new(None, "W2", "r2", "err", DiagnosticSeverity::Error, Rect::ZERO));
    ///
    /// assert_eq!(surface.tier2_entries_by_severity(DiagnosticSeverity::Error).len(), 1);
    /// assert_eq!(surface.tier2_entries_by_severity(DiagnosticSeverity::Warn).len(), 2);
    /// ```
    pub fn tier2_entries_by_severity(
        &self,
        min_severity: DiagnosticSeverity,
    ) -> Vec<&DiagnosticEntry> {
        self.tracked_entries
            .values()
            .filter(|e| e.severity >= min_severity)
            .collect()
    }

    /// Resolves a diagnostic entry by its linkable node path.
    ///
    /// Used for round-trip node reveal in the inspector tree panel.
    ///
    /// # Examples
    ///
    /// ```
    /// use glam::Vec2;
    /// use kurbo::Rect;
    /// use martensite_core::WidgetId;
    /// use martensite_devtools::error_surface::{ErrorSurface, LayoutDiagnostic};
    ///
    /// let mut surface = ErrorSurface::new();
    /// let widget = WidgetId::from_parts(7, 1);
    /// surface.record_layout_diagnostic(LayoutDiagnostic::new(
    ///     widget,
    ///     "App/Sidebar/Item[2]",
    ///     Rect::ZERO,
    ///     Vec2::new(100.0, 50.0),
    ///     Vec2::new(130.0, 50.0),
    /// ));
    ///
    /// let entry = surface.find_by_node_path("App/Sidebar/Item[2]");
    /// assert!(entry.is_some());
    /// assert_eq!(entry.unwrap().node_id, Some(widget));
    /// ```
    pub fn find_by_node_path(&self, path: &str) -> Option<&DiagnosticEntry> {
        self.tracked_entries.values().find(|e| e.node_path == path)
    }

    /// Captures a structured dev panic report and crash bundle.
    ///
    /// Pulls the last `max_events` records from `ledger` if provided.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::error_surface::{ErrorSurface, PanicPhase};
    ///
    /// let mut surface = ErrorSurface::new();
    /// let bundle = surface.capture_panic(
    ///     "render target unbound",
    ///     None,
    ///     "App/Viewport",
    ///     PanicPhase::Paint,
    ///     None,
    ///     10,
    /// );
    /// assert!(bundle.policy.can_continue());
    /// assert_eq!(bundle.phase, PanicPhase::Paint);
    /// ```
    pub fn capture_panic(
        &mut self,
        message: impl Into<String>,
        in_flight_node: Option<WidgetId>,
        in_flight_path: impl Into<String>,
        phase: PanicPhase,
        ledger: Option<&EventLedger>,
        max_events: usize,
    ) -> &CrashBundle {
        let policy = match phase {
            PanicPhase::Paint => RecoveryPolicy::ContinueWithPrunedNode,
            _ => RecoveryPolicy::RestartOnly,
        };

        let mut recent_events = Vec::new();
        if let Some(l) = ledger {
            for record in l.iter().take(max_events) {
                recent_events.push(record.format_diagnostic());
            }
            recent_events.reverse();
        }

        let backtrace = if std::env::var("RUST_BACKTRACE")
            .map(|v| v != "0")
            .unwrap_or(false)
        {
            Some(format!("{:?}", std::backtrace::Backtrace::capture()))
        } else {
            None
        };

        let bundle = CrashBundle::new(message, in_flight_node, in_flight_path, phase, policy)
            .with_recent_events(recent_events)
            .with_backtrace_opt(backtrace);

        self.last_crash = Some(bundle);
        self.last_crash.as_ref().unwrap()
    }

    /// Returns the last captured crash bundle, if any.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::error_surface::ErrorSurface;
    ///
    /// let surface = ErrorSurface::new();
    /// assert!(surface.last_crash().is_none());
    /// ```
    #[inline]
    pub fn last_crash(&self) -> Option<&CrashBundle> {
        self.last_crash.as_ref()
    }

    /// Clears the last captured crash bundle.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::error_surface::{ErrorSurface, PanicPhase};
    ///
    /// let mut surface = ErrorSurface::new();
    /// surface.capture_panic("err", None, "R", PanicPhase::Unknown, None, 0);
    /// assert!(surface.last_crash().is_some());
    /// surface.clear_crash();
    /// assert!(surface.last_crash().is_none());
    /// ```
    #[inline]
    pub fn clear_crash(&mut self) {
        self.last_crash = None;
    }
}
