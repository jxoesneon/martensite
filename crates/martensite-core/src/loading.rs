//! Shared skeleton/shimmer painter for loading placeholders.
//!
//! This module is the single paint path behind the native loading
//! state ([`Widget::is_loading`](crate::widget::Widget::is_loading) /
//! [`Widget::paint_loading`](crate::widget::Widget::paint_loading)) and
//! the facade `Skeleton` widget. All loading placeholders in the
//! framework render through [`paint_skeleton`](crate::loading::paint_skeleton)
//! so shimmer timing,
//! band geometry, and colours can never drift between widgets.
//!
//! The painter is phase-driven, not self-animated: callers pass
//! `phase = Some(t)` for an animated sweep (normally the shared
//! arena clock) or `phase = None` for a static placeholder
//! (reduced motion, tests, golden frames).

use crate::widget::PaintContext;
use crate::{GradientStop, GradientStops, Rect, TokenKey};

/// Base placeholder colour.
const BASE: [u8; 4] = [222, 225, 231, 255];
/// Shimmer highlight colour — translucent white band sweeping across.
const SHIMMER: [u8; 4] = [255, 255, 255, 150];
/// Width of the shimmer band as a fraction of the placeholder width.
const BAND_FRAC: f32 = 0.45;
/// Line height of a text-line placeholder, logical points.
const LINE_PT: f32 = 12.0;
/// Gap between text lines, logical points.
const LINE_GAP_PT: f32 = 8.0;
/// Corner radius for block/line placeholders, logical points.
const RADIUS: f64 = 4.0;
/// Seconds per shimmer sweep.
pub const SWEEP_SECS: f32 = 1.4;

/// Which placeholder geometry a loading surface paints.
///
/// `Block`, `Circle`, and `Lines` are the standalone shapes; `Rows`
/// and `Grid` exist for shape-accurate placeholders inside
/// collections (a pending list looks like rows, a pending gallery
/// looks like a grid of cells).
///
/// # Examples
///
/// ```
/// use martensite_core::loading::SkeletonShape;
///
/// assert_ne!(SkeletonShape::Block, SkeletonShape::Circle);
/// assert_eq!(SkeletonShape::Rows { count: 4 }, SkeletonShape::Rows { count: 4 });
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkeletonShape {
    /// A single rounded block filling the bounds.
    Block,
    /// A circle centred in the bounds (avatar placeholders).
    Circle,
    /// A paragraph of `n` text lines; the last line is shorter,
    /// matching real paragraph raggedness.
    Lines(usize),
    /// `count` uniform row bars — the shape of a pending list,
    /// table body, or feed.
    Rows {
        /// Number of row placeholders to paint.
        count: usize,
    },
    /// `cols`×`rows` uniform cells — the shape of a pending grid
    /// or gallery.
    Grid {
        /// Cells per row.
        cols: usize,
        /// Rows of cells.
        rows: usize,
    },
}

/// Preferred natural size (logical points) of a standalone shape.
#[must_use]
pub fn preferred_size(shape: SkeletonShape) -> (f32, f32) {
    match shape {
        SkeletonShape::Block => (120.0, 24.0),
        SkeletonShape::Circle => (40.0, 40.0),
        SkeletonShape::Lines(n) => {
            let n = n.max(1) as f32;
            (200.0, n * (LINE_PT + LINE_GAP_PT) - LINE_GAP_PT)
        }
        SkeletonShape::Rows { count } => {
            let n = count.max(1) as f32;
            (200.0, n * (LINE_PT + LINE_GAP_PT))
        }
        SkeletonShape::Grid { cols, rows } => {
            (48.0 * cols.max(1) as f32, 48.0 * rows.max(1) as f32)
        }
    }
}

/// Paints one placeholder rect (or circle) at `rect`, including the
/// shimmer band when `phase` is `Some`.
///
/// `phase` runs `0.0..=1.0` across one sweep; pass
/// [`sweep_phase`] output or any fraction. `None` paints the static
/// base only — the reduced-motion and deterministic-test path.
pub fn paint_placeholder(cx: &mut PaintContext, rect: Rect, circle: bool, phase: Option<f32>) {
    use kurbo::Shape as _;
    let kr = kurbo::Rect::new(
        f64::from(rect.min_x()),
        f64::from(rect.min_y()),
        f64::from(rect.max_x()),
        f64::from(rect.max_y()),
    );
    let base = cx.color(TokenKey::SurfaceColor, BASE);
    if circle {
        let ellipse = kurbo::Ellipse::from_rect(kr).into_path(0.1);
        cx.list.push_path(ellipse.clone(), base);
        cx.list.push_clip_path(ellipse);
    } else {
        let rounded = kurbo::RoundedRect::from_rect(kr, cx.ptf(RADIUS)).into_path(0.1);
        cx.list.push_path(rounded.clone(), base);
        cx.list.push_clip_path(rounded);
    }

    if let Some(phase) = phase {
        // A translucent light band sweeping left → right; it travels
        // one band-width past each edge so the sweep fully clears
        // the placeholder at both ends.
        let band = rect.size.x * BAND_FRAC;
        let x = rect.origin.x - band + phase * (rect.size.x + band);
        let stops = GradientStops::from_slice(&[
            GradientStop::new(0.0, [SHIMMER[0], SHIMMER[1], SHIMMER[2], 0]),
            GradientStop::new(0.5, SHIMMER),
            GradientStop::new(1.0, [SHIMMER[0], SHIMMER[1], SHIMMER[2], 0]),
        ]);
        cx.list.push_linear_gradient(
            kurbo::Rect::new(
                f64::from(x),
                f64::from(rect.min_y()),
                f64::from(x + band),
                f64::from(rect.max_y()),
            ),
            stops,
            [f64::from(x), f64::from(rect.origin.y)],
            [f64::from(x + band), f64::from(rect.origin.y)],
        );
    }
    cx.list.pop_clip();
}

/// Paints a complete placeholder of `shape` filling `bounds`.
///
/// This is the default body of
/// [`Widget::paint_loading`](crate::widget::Widget::paint_loading) and
/// the shared path behind the facade `Skeleton` widget.
///
/// # Examples
///
/// ```
/// use martensite_core::loading::{paint_skeleton, SkeletonShape};
/// use martensite_core::{HotNode, PaintContext, PaintList, Rect, Theme};
///
/// let mut list = PaintList::new();
/// let theme = Theme::new("test");
/// let mut cx = PaintContext {
///     list: &mut list,
///     bounds: Rect::new(0.0, 0.0, 200.0, 80.0),
///     theme: &theme,
///     scale: 1.0,
///     text_painter: None,
/// };
/// let bounds = cx.bounds;
/// paint_skeleton(&mut cx, bounds, SkeletonShape::Rows { count: 3 }, None);
/// assert!(list.len() >= 3);
/// ```
pub fn paint_skeleton(
    cx: &mut PaintContext,
    bounds: Rect,
    shape: SkeletonShape,
    phase: Option<f32>,
) {
    if bounds.size.x <= 0.0 || bounds.size.y <= 0.0 {
        return;
    }
    match shape {
        SkeletonShape::Block => paint_placeholder(cx, bounds, false, phase),
        SkeletonShape::Circle => {
            let side = bounds.size.x.min(bounds.size.y);
            let r = Rect::new(
                bounds.origin.x + (bounds.size.x - side) / 2.0,
                bounds.origin.y + (bounds.size.y - side) / 2.0,
                side,
                side,
            );
            paint_placeholder(cx, r, true, phase);
        }
        SkeletonShape::Lines(n) => {
            let n = n.max(1);
            let line_h = cx.pt(LINE_PT);
            let gap = cx.pt(LINE_GAP_PT);
            let mut y = bounds.origin.y;
            for i in 0..n {
                if y + line_h > bounds.max_y() {
                    break;
                }
                // Ragged paragraph edge — the last line runs short.
                let w = if i == n - 1 {
                    bounds.size.x * 0.62
                } else {
                    bounds.size.x
                };
                paint_placeholder(cx, Rect::new(bounds.origin.x, y, w, line_h), false, phase);
                y += line_h + gap;
            }
        }
        SkeletonShape::Rows { count } => {
            let count = count.max(1);
            let row_h = cx.pt(LINE_PT);
            let gap = cx.pt(LINE_GAP_PT);
            let mut y = bounds.origin.y;
            for _ in 0..count {
                if y + row_h > bounds.max_y() {
                    break;
                }
                paint_placeholder(
                    cx,
                    Rect::new(bounds.origin.x, y, bounds.size.x, row_h),
                    false,
                    phase,
                );
                y += row_h + gap;
            }
        }
        SkeletonShape::Grid { cols, rows } => {
            let cols = cols.max(1);
            let rows = rows.max(1);
            let gap = cx.pt(LINE_GAP_PT);
            let cell_w = (bounds.size.x - gap * (cols as f32 - 1.0).max(0.0)) / cols as f32;
            let cell_h = (bounds.size.y - gap * (rows as f32 - 1.0).max(0.0)) / rows as f32;
            if cell_w <= 0.0 || cell_h <= 0.0 {
                return;
            }
            for r in 0..rows {
                for c in 0..cols {
                    paint_placeholder(
                        cx,
                        Rect::new(
                            bounds.origin.x + c as f32 * (cell_w + gap),
                            bounds.origin.y + r as f32 * (cell_h + gap),
                            cell_w,
                            cell_h,
                        ),
                        false,
                        phase,
                    );
                }
            }
        }
    }
}

/// Shimmer phase `0.0..=1.0` for a monotonically increasing elapsed
/// time in seconds — the shared arena clock feeds this so all
/// skeletons sweep in lock-step.
///
/// # Examples
///
/// ```
/// use martensite_core::loading::{sweep_phase, SWEEP_SECS};
///
/// assert_eq!(sweep_phase(0.0), 0.0);
/// assert_eq!(sweep_phase(SWEEP_SECS), 0.0); // wraps after one sweep
/// ```
#[must_use]
pub fn sweep_phase(elapsed_secs: f32) -> f32 {
    (elapsed_secs / SWEEP_SECS) % 1.0
}
