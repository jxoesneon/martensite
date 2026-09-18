//! Element outline shapes shared by painting, clipping, and hit-testing.
//!
//! [`Shape`] is the framework's shape vocabulary — the same outline drives
//! [`PaintList::push_fill_shape`](crate::PaintList::push_fill_shape),
//! [`push_stroke_shape`](crate::PaintList::push_stroke_shape),
//! [`push_clip_shape`](crate::PaintList::push_clip_shape), and
//! [`Widget::hit_shape`](crate::Widget::hit_shape), so the region a widget
//! paints, clips to, and accepts input inside can never drift apart.
//!
//! The corner vocabulary matches what competing toolkits expose:
//!
//! | [`CornerStyle`] | CSS `corner-shape` | Flutter | Compose |
//! |---|---|---|---|
//! | [`Round`](CornerStyle::Round) | `round` | `RoundedRectangleBorder` | `RoundedCornerShape` |
//! | [`Cut`](CornerStyle::Cut) | `bevel`/`angle` | `BeveledRectangleBorder` | `CutCornerShape` |
//! | [`Notch`](CornerStyle::Notch) | `notch` | — | — |
//! | [`Scoop`](CornerStyle::Scoop) | `scoop` | — | — |
//! | [`Squircle`](CornerStyle::Squircle) | `squircle` | `RoundedSuperellipseBorder` | — |
//!
//! Silhouette conveniences cover the rest of the competitor set:
//! [`Shape::Pill`] (stadium), [`Shape::Ellipse`], [`Shape::Circle`], and
//! [`Shape::Path`] for arbitrary Bézier outlines.
//!
//! # Geometry conventions
//!
//! - Bounds are [`kurbo::Rect`]s in the same coordinate space as paint
//!   commands (physical pixels at paint time).
//! - Corner radii are elliptical (`rx`, `ry`) per corner, matching CSS
//!   `border-radius` longhands. When adjacent radii would overflow an edge,
//!   all radii are scaled down proportionally — the CSS "overlapping
//!   curves" rule — so degenerate input produces well-formed geometry.
//! - [`Squircle`](CornerStyle::Squircle) corners are n=4 superellipse arcs
//!   (`|x|^4 + |y|^4 = r^4`), the iOS continuous-corner approximation,
//!   emitted as a 12-segment polyline per corner.
//! - [`Shape::contains`] uses the non-zero winding rule on a flattened
//!   outline (with analytic fast paths for the common silhouettes).

use glam::Vec2;
use kurbo::{BezPath, PathEl, Point, Rect};

/// Cubic approximation constant for a quarter-ellipse arc.
const KAPPA: f64 = 0.552_284_749_830_793_6;

/// Polyline segments sampled per squircle corner.
const SQUIRCLE_SEGMENTS: usize = 12;

/// Default flatten tolerance (in pixels) for outline→polygon conversion.
const FLATTEN_TOLERANCE: f64 = 0.25;

/// How a single corner of a [`Shape::Corners`] outline is rendered.
///
/// `Round` and `Squircle` are convex (the outline bows toward the corner
/// vertex); `Cut` is a straight diagonal; `Notch` and `Scoop` are concave
/// (the outline recedes into the element).
///
/// # Examples
///
/// ```
/// use martensite_core::shape::{CornerStyle, CornerStyles};
///
/// // Uniform round corners — the CSS `border-radius` default.
/// let styles = CornerStyles::uniform(CornerStyle::Round);
/// assert_eq!(styles.top_left, CornerStyle::Round);
/// ```
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CornerStyle {
    /// Quarter-ellipse arc — CSS `round`, `RoundedRectangleBorder`,
    /// `RoundedCornerShape`.
    #[default]
    Round,
    /// Straight diagonal across the corner — CSS `bevel`/`angle`,
    /// `BeveledRectangleBorder`, `CutCornerShape`.
    Cut,
    /// Concave right-angle notch carved into the corner — CSS `notch`.
    Notch,
    /// Concave quarter-ellipse scooped out of the corner — CSS `scoop`.
    Scoop,
    /// Continuous superellipse (n=4) corner — CSS `squircle`, iOS
    /// continuous corners, `RoundedSuperellipseBorder`. Emitted as a
    /// [`SQUIRCLE_SEGMENTS`]-segment polyline.
    Squircle,
}

/// Per-corner [`CornerStyle`]s — the corner-shape analogue of
/// [`CornerRadii`]. A single style applies to all four corners via
/// [`CornerStyles::uniform`] or `From<CornerStyle>`; styles may also be
/// mixed per corner.
///
/// # Examples
///
/// ```
/// use martensite_core::shape::{CornerStyle, CornerStyles};
///
/// let mut styles = CornerStyles::uniform(CornerStyle::Round);
/// styles.bottom_left = CornerStyle::Cut;
/// assert_eq!(styles.bottom_left, CornerStyle::Cut);
/// assert_eq!(styles.top_left, CornerStyle::Round);
/// ```
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct CornerStyles {
    /// Top-left corner style.
    pub top_left: CornerStyle,
    /// Top-right corner style.
    pub top_right: CornerStyle,
    /// Bottom-right corner style.
    pub bottom_right: CornerStyle,
    /// Bottom-left corner style.
    pub bottom_left: CornerStyle,
}

impl CornerStyles {
    /// All four corners share `style`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::shape::{CornerStyle, CornerStyles};
    ///
    /// let s = CornerStyles::uniform(CornerStyle::Squircle);
    /// assert_eq!(s.bottom_right, CornerStyle::Squircle);
    /// ```
    #[inline]
    #[must_use]
    pub const fn uniform(style: CornerStyle) -> Self {
        Self {
            top_left: style,
            top_right: style,
            bottom_right: style,
            bottom_left: style,
        }
    }

    /// `true` when every corner uses `style`.
    #[inline]
    #[must_use]
    pub fn all(&self, style: CornerStyle) -> bool {
        self.top_left == style
            && self.top_right == style
            && self.bottom_right == style
            && self.bottom_left == style
    }
}

impl Default for CornerStyles {
    #[inline]
    fn default() -> Self {
        Self::uniform(CornerStyle::Round)
    }
}

impl From<CornerStyle> for CornerStyles {
    #[inline]
    fn from(style: CornerStyle) -> Self {
        Self::uniform(style)
    }
}

/// Elliptical corner radii — one `Vec2 { x: rx, y: ry }` per corner.
///
/// Matches the CSS `border-radius` longhands: `rx` is the horizontal
/// extent of the corner, `ry` the vertical. Use [`CornerRadii::uniform`]
/// for the common square-radius case; helpers like
/// [`CornerRadii::top`]/`left` cover edge-aligned subsets.
///
/// # Examples
///
/// ```
/// use glam::Vec2;
/// use martensite_core::shape::CornerRadii;
///
/// let r = CornerRadii::uniform(8.0);
/// assert_eq!(r.top_left, Vec2::splat(8.0));
///
/// // Tabs: rounded top edge, square bottom edge.
/// let tabs = CornerRadii::top(6.0);
/// assert_eq!(tabs.top_right.y, 6.0);
/// assert_eq!(tabs.bottom_left, Vec2::ZERO);
/// ```
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct CornerRadii {
    /// Top-left elliptical radius.
    pub top_left: Vec2,
    /// Top-right elliptical radius.
    pub top_right: Vec2,
    /// Bottom-right elliptical radius.
    pub bottom_right: Vec2,
    /// Bottom-left elliptical radius.
    pub bottom_left: Vec2,
}

impl CornerRadii {
    /// No corner radius — a plain rectangle.
    pub const ZERO: Self = Self {
        top_left: Vec2::ZERO,
        top_right: Vec2::ZERO,
        bottom_right: Vec2::ZERO,
        bottom_left: Vec2::ZERO,
    };

    /// All four corners share a square radius `r`.
    #[inline]
    #[must_use]
    pub const fn uniform(r: f32) -> Self {
        let v = Vec2::splat(r);
        Self::all(v, v, v, v)
    }

    /// Explicit per-corner radii in CSS order (top-left, top-right,
    /// bottom-right, bottom-left).
    #[inline]
    #[must_use]
    pub const fn all(
        top_left: Vec2,
        top_right: Vec2,
        bottom_right: Vec2,
        bottom_left: Vec2,
    ) -> Self {
        Self {
            top_left,
            top_right,
            bottom_right,
            bottom_left,
        }
    }

    /// The same radius on both top corners.
    #[inline]
    #[must_use]
    pub const fn top(r: f32) -> Self {
        let v = Vec2::splat(r);
        Self::all(v, v, Vec2::ZERO, Vec2::ZERO)
    }

    /// The same radius on both bottom corners.
    #[inline]
    #[must_use]
    pub const fn bottom(r: f32) -> Self {
        let v = Vec2::splat(r);
        Self::all(Vec2::ZERO, Vec2::ZERO, v, v)
    }

    /// The same radius on both left corners.
    #[inline]
    #[must_use]
    pub const fn left(r: f32) -> Self {
        let v = Vec2::splat(r);
        Self::all(v, Vec2::ZERO, Vec2::ZERO, v)
    }

    /// The same radius on both right corners.
    #[inline]
    #[must_use]
    pub const fn right(r: f32) -> Self {
        let v = Vec2::splat(r);
        Self::all(Vec2::ZERO, v, v, Vec2::ZERO)
    }

    /// `true` when every corner radius is zero (or negative).
    #[inline]
    #[must_use]
    pub fn is_zero(&self) -> bool {
        [
            self.top_left,
            self.top_right,
            self.bottom_right,
            self.bottom_left,
        ]
        .iter()
        .all(|r| r.x <= 0.0 || r.y <= 0.0)
    }

    /// `true` when all four corners share one square radius.
    #[must_use]
    pub fn is_uniform(&self) -> Option<f32> {
        let r = self.top_left;
        if self.top_right == r && self.bottom_right == r && self.bottom_left == r {
            Some(r.x.min(r.y))
        } else {
            None
        }
    }

    /// Sanitizes and scales radii for a `width × height` box.
    ///
    /// Negative components become zero, then — when adjacent radii would
    /// sum past an edge length — every radius shrinks by one common
    /// factor. This is the CSS `border-radius` "overlapping curves" rule:
    /// the corner proportions are preserved and the geometry stays
    /// well-formed for any input.
    ///
    /// # Examples
    ///
    /// ```
    /// use glam::Vec2;
    /// use martensite_core::shape::CornerRadii;
    ///
    /// // 60px + 60px on a 100px top edge → scaled to 50/50 (the
    /// // 200px height imposes no constraint).
    /// let r = CornerRadii::uniform(60.0).resolve(100.0, 200.0);
    /// assert_eq!(r.top_left.x, 50.0);
    /// // A 40px height is the tighter bound → one common ×⅓ factor.
    /// let r = CornerRadii::uniform(60.0).resolve(100.0, 40.0);
    /// assert_eq!(r.top_left, Vec2::splat(20.0));
    /// ```
    #[must_use]
    pub fn resolve(&self, width: f64, height: f64) -> Self {
        let clamp = |v: Vec2| Vec2::new(v.x.max(0.0), v.y.max(0.0));
        let mut r = Self::all(
            clamp(self.top_left),
            clamp(self.top_right),
            clamp(self.bottom_right),
            clamp(self.bottom_left),
        );
        let mut f = 1.0f64;
        let shrink = |sum: f32, edge: f64, f: &mut f64| {
            if sum as f64 > edge && sum > 0.0 {
                *f = f.min(edge / f64::from(sum));
            }
        };
        if width > 0.0 {
            shrink(r.top_left.x + r.top_right.x, width, &mut f);
            shrink(r.bottom_left.x + r.bottom_right.x, width, &mut f);
        }
        if height > 0.0 {
            shrink(r.top_left.y + r.bottom_left.y, height, &mut f);
            shrink(r.top_right.y + r.bottom_right.y, height, &mut f);
        }
        if f < 1.0 {
            let s = f as f32;
            for v in [
                &mut r.top_left,
                &mut r.top_right,
                &mut r.bottom_right,
                &mut r.bottom_left,
            ] {
                *v *= s;
            }
        }
        r
    }
}

/// A resolved element outline — the shared currency of fill, stroke,
/// clip, and hit-test.
///
/// `Shape` is bounds-independent: the silhouette is resolved against a
/// concrete [`Rect`] at use time via [`Shape::to_path`],
/// [`Shape::contains`], or [`Shape::flatten`]. Widgets typically return a
/// constant shape from [`Widget::hit_shape`](crate::Widget::hit_shape)
/// or pick one inside `paint` from `cx.bounds`.
///
/// # Examples
///
/// ```
/// use kurbo::Rect;
/// use martensite_core::shape::Shape;
///
/// let bounds = Rect::new(0.0, 0.0, 120.0, 36.0);
/// let pill = Shape::PILL.to_path(bounds);
/// let squircle = Shape::squircle(10.0).to_path(bounds);
/// assert_ne!(pill, squircle);
/// ```
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum Shape {
    /// A plain axis-aligned rectangle — the default silhouette.
    Rect,
    /// A rectangle with styled corners — the general rounded-rect form.
    /// Covers uniform radii, per-corner radii, and per-corner styles.
    Corners {
        /// Elliptical radii per corner, resolved with the CSS
        /// overlapping-curves rule at use time.
        radii: CornerRadii,
        /// Corner rendering style per corner.
        styles: CornerStyles,
    },
    /// A stadium (capsule): a rectangle whose ends are semicircles —
    /// `Corners` with a uniform radius of half the short side.
    Pill,
    /// An ellipse inscribed in the bounds — a circle when square.
    Ellipse,
    /// A circle at an explicit center and radius — ignores `bounds`.
    /// Useful for dots, knobs, and indicators.
    Circle {
        /// Circle center in element coordinates.
        center: Vec2,
        /// Circle radius.
        radius: f32,
    },
    /// An arbitrary closed Bézier outline, already in element
    /// coordinates (`bounds` is ignored).
    Path(BezPath),
}

impl Shape {
    /// A plain rectangle.
    pub const RECT: Self = Self::Rect;
    /// A stadium/capsule outline.
    pub const PILL: Self = Self::Pill;
    /// An ellipse inscribed in the bounds.
    pub const ELLIPSE: Self = Self::Ellipse;

    /// Rectangle with a uniform circular corner radius.
    ///
    /// # Examples
    ///
    /// ```
    /// use kurbo::Rect;
    /// use martensite_core::shape::Shape;
    ///
    /// let path = Shape::rounded(8.0).to_path(Rect::new(0.0, 0.0, 40.0, 40.0));
    /// assert!(!path.is_empty());
    /// ```
    #[must_use]
    pub fn rounded(radius: f32) -> Self {
        Self::corners(CornerRadii::uniform(radius), CornerStyle::Round)
    }

    /// Rectangle with a uniform elliptical corner radius (`rx`, `ry`).
    #[must_use]
    pub fn rounded_each(rx: f32, ry: f32) -> Self {
        Self::Corners {
            radii: CornerRadii::all(
                Vec2::new(rx, ry),
                Vec2::new(rx, ry),
                Vec2::new(rx, ry),
                Vec2::new(rx, ry),
            ),
            styles: CornerStyles::uniform(CornerStyle::Round),
        }
    }

    /// Rectangle with explicit per-corner radii sharing one corner style.
    #[must_use]
    pub fn corners(radii: CornerRadii, style: CornerStyle) -> Self {
        Self::Corners {
            radii,
            styles: CornerStyles::uniform(style),
        }
    }

    /// Rectangle with explicit per-corner radii *and* per-corner styles.
    #[must_use]
    pub const fn corners_styled(radii: CornerRadii, styles: CornerStyles) -> Self {
        Self::Corners { radii, styles }
    }

    /// Rectangle with uniform superellipse (n=4) corners — the iOS
    /// continuous-corner look.
    ///
    /// # Examples
    ///
    /// ```
    /// use kurbo::{Point, Rect};
    /// use martensite_core::shape::Shape;
    ///
    /// let b = Rect::new(0.0, 0.0, 48.0, 48.0);
    /// // Squircle corners are fuller than round: close to the vertex a
    /// // point falls outside the round arc but inside the squircle.
    /// let p = Point::new(3.0, 3.0);
    /// assert!(!Shape::rounded(12.0).contains(b, p));
    /// assert!(Shape::squircle(12.0).contains(b, p));
    /// ```
    #[must_use]
    pub fn squircle(radius: f32) -> Self {
        Self::corners(CornerRadii::uniform(radius), CornerStyle::Squircle)
    }

    /// Rectangle with uniform chamfered (diagonally cut) corners.
    #[must_use]
    pub fn cut(radius: f32) -> Self {
        Self::corners(CornerRadii::uniform(radius), CornerStyle::Cut)
    }

    /// Rectangle with uniform concave square notches.
    #[must_use]
    pub fn notch(radius: f32) -> Self {
        Self::corners(CornerRadii::uniform(radius), CornerStyle::Notch)
    }

    /// Rectangle with uniform concave scooped corners.
    #[must_use]
    pub fn scoop(radius: f32) -> Self {
        Self::corners(CornerRadii::uniform(radius), CornerStyle::Scoop)
    }

    /// A circle at `center` with `radius` — bounds-independent.
    #[must_use]
    pub const fn circle(center: Vec2, radius: f32) -> Self {
        Self::Circle { center, radius }
    }

    /// Wraps an arbitrary Bézier outline verbatim.
    #[must_use]
    pub const fn path(path: BezPath) -> Self {
        Self::Path(path)
    }

    /// `true` when the shape resolves to a plain rectangle for `bounds`.
    ///
    /// `Corners` with all-zero resolved radii counts as a rectangle.
    ///
    /// # Examples
    ///
    /// ```
    /// use kurbo::Rect;
    /// use martensite_core::shape::{CornerRadii, CornerStyle, Shape};
    ///
    /// let b = Rect::new(0.0, 0.0, 10.0, 10.0);
    /// assert!(Shape::RECT.is_rect(b));
    /// assert!(Shape::corners(CornerRadii::ZERO, CornerStyle::Round).is_rect(b));
    /// assert!(!Shape::PILL.is_rect(b));
    /// ```
    #[must_use]
    pub fn is_rect(&self, bounds: Rect) -> bool {
        match self {
            Self::Rect => true,
            Self::Corners { radii, .. } => radii.resolve(bounds.width(), bounds.height()).is_zero(),
            _ => false,
        }
    }

    /// Resolves the outline against `bounds` into a closed [`BezPath`].
    ///
    /// All silhouette and corner variants converge here — this is the
    /// single geometry source for fill, stroke, clip, and (via
    /// [`Shape::flatten`]) hit-testing.
    ///
    /// # Examples
    ///
    /// ```
    /// use kurbo::Rect;
    /// use martensite_core::shape::Shape;
    ///
    /// let b = Rect::new(0.0, 0.0, 100.0, 50.0);
    /// let path = Shape::PILL.to_path(b);
    /// // A pill has straight edges plus two semicircle arcs.
    /// assert!(path.elements().len() >= 6);
    /// ```
    #[must_use]
    pub fn to_path(&self, bounds: Rect) -> BezPath {
        match self {
            Self::Rect => {
                let mut p = BezPath::new();
                p.move_to((bounds.x0, bounds.y0));
                p.line_to((bounds.x1, bounds.y0));
                p.line_to((bounds.x1, bounds.y1));
                p.line_to((bounds.x0, bounds.y1));
                p.close_path();
                p
            }
            Self::Pill => {
                let r = (bounds.width().min(bounds.height()) * 0.5) as f32;
                corners_path(bounds, &CornerRadii::uniform(r), &CornerStyles::default())
            }
            Self::Ellipse => ellipse_path(
                bounds.center().x,
                bounds.center().y,
                bounds.width() * 0.5,
                bounds.height() * 0.5,
            ),
            Self::Circle { center, radius } => ellipse_path(
                f64::from(center.x),
                f64::from(center.y),
                f64::from(radius.max(0.0)),
                f64::from(radius.max(0.0)),
            ),
            Self::Corners { radii, styles } => {
                let r = radii.resolve(bounds.width(), bounds.height());
                corners_path(bounds, &r, styles)
            }
            Self::Path(p) => p.clone(),
        }
    }

    /// Flattens the resolved outline into a closed polygon.
    ///
    /// Curves are subdivided to `tolerance` pixels (default 0.25 when
    /// `tolerance <= 0`). The result feeds `ClipShape::Path`-style
    /// winding-number hit-tests.
    ///
    /// # Examples
    ///
    /// ```
    /// use kurbo::Rect;
    /// use martensite_core::shape::Shape;
    ///
    /// let b = Rect::new(0.0, 0.0, 100.0, 100.0);
    /// let poly = Shape::squircle(16.0).flatten(b, 0.5);
    /// assert!(poly.len() >= 4 * 12); // 12 segments per corner
    /// ```
    #[must_use]
    pub fn flatten(&self, bounds: Rect, tolerance: f64) -> Vec<Vec2> {
        let tol = if tolerance > 0.0 {
            tolerance
        } else {
            FLATTEN_TOLERANCE
        };
        let mut pts = Vec::new();
        kurbo::flatten(self.to_path(bounds), tol, |el| match el {
            PathEl::MoveTo(p) | PathEl::LineTo(p) => {
                pts.push(Vec2::new(p.x as f32, p.y as f32));
            }
            PathEl::ClosePath => {}
            _ => {}
        });
        pts
    }

    /// `true` when `point` lies inside the resolved outline.
    ///
    /// Fast analytic paths handle `Rect`, `Pill`, `Ellipse`, `Circle`,
    /// and uniformly-styled `Corners`; everything else falls back to a
    /// flattened non-zero winding test, which correctly handles concave
    /// (`Notch`/`Scoop`) and self-intersecting outlines.
    ///
    /// # Boundary convention
    ///
    /// Straight edges are half-open `[min, max)` (matching the framework
    /// AABB convention); curved boundaries are inclusive — a point
    /// resting exactly on an arc is inside.
    ///
    /// # Examples
    ///
    /// ```
    /// use kurbo::{Point, Rect};
    /// use martensite_core::shape::Shape;
    ///
    /// let b = Rect::new(0.0, 0.0, 20.0, 20.0);
    /// let rr = Shape::rounded(5.0);
    /// assert!(!rr.contains(b, Point::new(1.0, 1.0)));
    /// assert!(rr.contains(b, Point::new(10.0, 10.0)));
    /// ```
    #[must_use]
    pub fn contains(&self, bounds: Rect, point: Point) -> bool {
        if !point.is_finite() {
            return false;
        }
        match self {
            Self::Rect => aabb_contains(bounds, point),
            Self::Pill => {
                let r = bounds.width().min(bounds.height()) * 0.5;
                corners_contain(
                    bounds,
                    point,
                    &CornerRadii::uniform(r as f32).resolve(bounds.width(), bounds.height()),
                    |px, py, _vx, _vy, cx, cy, rx, ry| {
                        let dx = (px - cx) / rx;
                        let dy = (py - cy) / ry;
                        dx * dx + dy * dy <= 1.0
                    },
                )
            }
            Self::Ellipse => {
                let rx = bounds.width() * 0.5;
                let ry = bounds.height() * 0.5;
                if rx <= 0.0 || ry <= 0.0 {
                    return false;
                }
                let dx = (point.x - bounds.center().x) / rx;
                let dy = (point.y - bounds.center().y) / ry;
                dx * dx + dy * dy <= 1.0
            }
            Self::Circle { center, radius } => {
                let dx = point.x - f64::from(center.x);
                let dy = point.y - f64::from(center.y);
                let r = f64::from(radius.max(0.0));
                dx * dx + dy * dy <= r * r
            }
            Self::Corners { radii, styles } => {
                let r = radii.resolve(bounds.width(), bounds.height());
                if r.is_zero() {
                    return aabb_contains(bounds, point);
                }
                if styles.all(CornerStyle::Round) {
                    return corners_contain(
                        bounds,
                        point,
                        &r,
                        |px, py, _vx, _vy, cx, cy, rx, ry| {
                            let dx = (px - cx) / rx;
                            let dy = (py - cy) / ry;
                            dx * dx + dy * dy <= 1.0
                        },
                    );
                }
                if styles.all(CornerStyle::Cut) {
                    // Inside the corner box the diagonal chops off the
                    // vertex-side triangle: with u/v measured from the
                    // vertex along each axis, outside iff u + v < 1.
                    return corners_contain(
                        bounds,
                        point,
                        &r,
                        |px, py, vx, vy, _cx, _cy, rx, ry| {
                            (px - vx).abs() / rx + (py - vy).abs() / ry >= 1.0
                        },
                    );
                }
                let poly = self.flatten(bounds, FLATTEN_TOLERANCE);
                winding_contains(&poly, Vec2::new(point.x as f32, point.y as f32))
            }
            Self::Path(_) => {
                let poly = self.flatten(bounds, FLATTEN_TOLERANCE);
                winding_contains(&poly, Vec2::new(point.x as f32, point.y as f32))
            }
        }
    }
}

/// Half-open `[min, max)` axis-aligned containment.
#[inline]
fn aabb_contains(r: Rect, p: Point) -> bool {
    p.x >= r.x0 && p.x < r.x1 && p.y >= r.y0 && p.y < r.y1
}

/// Per-corner containment dispatch for `Corners` silhouettes.
///
/// `corner_test(px, py, vx, vy, cx, cy, rx, ry)` receives the corner-box
/// vertex `vx/vy` (on the rect's outer corner) and the inner vertex
/// `cx/cy` (the center of the corner ellipse for convex styles), and
/// returns `true` when a point inside that corner box is inside the
/// shape.
#[allow(clippy::too_many_arguments)]
fn corners_contain(
    bounds: Rect,
    point: Point,
    r: &CornerRadii,
    corner_test: impl Fn(f64, f64, f64, f64, f64, f64, f64, f64) -> bool,
) -> bool {
    if !aabb_contains(bounds, point) {
        return false;
    }
    // (corner box x-range, y-range, ellipse center)
    let corners = [
        (
            bounds.x0,
            bounds.y0,
            f64::from(r.top_left.x),
            f64::from(r.top_left.y),
            1.0,
            1.0,
        ),
        (
            bounds.x1,
            bounds.y0,
            f64::from(r.top_right.x),
            f64::from(r.top_right.y),
            -1.0,
            1.0,
        ),
        (
            bounds.x1,
            bounds.y1,
            f64::from(r.bottom_right.x),
            f64::from(r.bottom_right.y),
            -1.0,
            -1.0,
        ),
        (
            bounds.x0,
            bounds.y1,
            f64::from(r.bottom_left.x),
            f64::from(r.bottom_left.y),
            1.0,
            -1.0,
        ),
    ];
    for (vx, vy, rx, ry, sx, sy) in corners {
        if rx <= 0.0 || ry <= 0.0 {
            continue;
        }
        // Corner box occupies [vx, vx + sx*rx] × [vy, vy + sy*ry].
        let in_x = if sx > 0.0 {
            point.x < vx + rx
        } else {
            point.x > vx - rx
        };
        let in_y = if sy > 0.0 {
            point.y < vy + ry
        } else {
            point.y > vy - ry
        };
        if in_x && in_y {
            let cx = vx + sx * rx;
            let cy = vy + sy * ry;
            return corner_test(point.x, point.y, vx, vy, cx, cy, rx, ry);
        }
    }
    true
}

/// Builds the closed outline of a `Corners` shape.
fn corners_path(bounds: Rect, r: &CornerRadii, styles: &CornerStyles) -> BezPath {
    let mut path = BezPath::new();
    let (x0, y0, x1, y1) = (bounds.x0, bounds.y0, bounds.x1, bounds.y1);
    let tl = Vec2::new(r.top_left.x, r.top_left.y);
    let tr = Vec2::new(r.top_right.x, r.top_right.y);
    let br = Vec2::new(r.bottom_right.x, r.bottom_right.y);
    let bl = Vec2::new(r.bottom_left.x, r.bottom_left.y);

    // Clockwise from just after the top-left corner along the top edge.
    path.move_to((x0 + f64::from(tl.x), y0));
    path.line_to((x1 - f64::from(tr.x), y0));
    emit_corner(
        &mut path,
        styles.top_right,
        Point::new(x1 - f64::from(tr.x), y0),
        Point::new(x1, y0 + f64::from(tr.y)),
        Point::new(x1 - f64::from(tr.x), y0 + f64::from(tr.y)),
        Point::new(x1, y0),
        tr,
        (1.0, -1.0),
    );
    path.line_to((x1, y1 - f64::from(br.y)));
    emit_corner(
        &mut path,
        styles.bottom_right,
        Point::new(x1, y1 - f64::from(br.y)),
        Point::new(x1 - f64::from(br.x), y1),
        Point::new(x1 - f64::from(br.x), y1 - f64::from(br.y)),
        Point::new(x1, y1),
        br,
        (1.0, 1.0),
    );
    path.line_to((x0 + f64::from(bl.x), y1));
    emit_corner(
        &mut path,
        styles.bottom_left,
        Point::new(x0 + f64::from(bl.x), y1),
        Point::new(x0, y1 - f64::from(bl.y)),
        Point::new(x0 + f64::from(bl.x), y1 - f64::from(bl.y)),
        Point::new(x0, y1),
        bl,
        (-1.0, 1.0),
    );
    path.line_to((x0, y0 + f64::from(tl.y)));
    emit_corner(
        &mut path,
        styles.top_left,
        Point::new(x0, y0 + f64::from(tl.y)),
        Point::new(x0 + f64::from(tl.x), y0),
        Point::new(x0 + f64::from(tl.x), y0 + f64::from(tl.y)),
        Point::new(x0, y0),
        tl,
        (-1.0, -1.0),
    );
    path.close_path();
    path
}

/// Emits one corner transition into `path`.
///
/// - `p0`: entry point on the incoming edge (the path's current point).
/// - `p1`: exit point on the outgoing edge.
/// - `inner`: the corner box's far vertex — center of the corner ellipse
///   for convex styles.
/// - `vertex`: the box corner itself.
/// - `sign`: superellipse axis signs for this corner `(sx, sy)`; the arc
///   is parameterized so `theta` sweeps `t0 → t1` where `t0`/`t1` place
///   `p0` and `p1` on the curve.
#[allow(clippy::too_many_arguments)]
fn emit_corner(
    path: &mut BezPath,
    style: CornerStyle,
    p0: Point,
    p1: Point,
    inner: Point,
    vertex: Point,
    radii: Vec2,
    sign: (f64, f64),
) {
    let rx = f64::from(radii.x);
    let ry = f64::from(radii.y);
    if rx <= 0.0 || ry <= 0.0 {
        path.line_to(p1);
        return;
    }
    match style {
        CornerStyle::Round => {
            // Convex quarter-ellipse centered on the corner box's inner
            // vertex — the arc bows toward the outer vertex, so control
            // handles pull toward `vertex`.
            path.curve_to(
                Point::new(
                    p0.x + KAPPA * (vertex.x - p0.x),
                    p0.y + KAPPA * (vertex.y - p0.y),
                ),
                Point::new(
                    p1.x + KAPPA * (vertex.x - p1.x),
                    p1.y + KAPPA * (vertex.y - p1.y),
                ),
                p1,
            );
        }
        CornerStyle::Cut => {
            path.line_to(p1);
        }
        CornerStyle::Notch => {
            // Concave square: dive to the inner vertex and back out.
            path.line_to(inner);
            path.line_to(p1);
        }
        CornerStyle::Scoop => {
            // Concave quarter-arc centered on the outer corner vertex —
            // the arc bows into the element, so control handles pull
            // toward `inner`.
            path.curve_to(
                Point::new(
                    p0.x + KAPPA * (inner.x - p0.x),
                    p0.y + KAPPA * (inner.y - p0.y),
                ),
                Point::new(
                    p1.x + KAPPA * (inner.x - p1.x),
                    p1.y + KAPPA * (inner.y - p1.y),
                ),
                p1,
            );
        }
        CornerStyle::Squircle => {
            // n=4 superellipse arc sampled as a polyline. Parametric:
            // point(θ) = inner + (sx·rx·|cosθ|^0.5, sy·ry·|sinθ|^0.5).
            // The sweep direction is chosen so θ runs from the entry
            // side (perpendicular offset zero) to the exit side.
            let (sx, sy) = sign;
            // θ=0 lands on the +sx axis end; θ=π/2 on the +sy axis end.
            // Determine which end is p0 by comparing offsets.
            let at0 = Point::new(inner.x + sx * rx, inner.y);
            let from_zero = (at0 - p0).hypot() < (at0 - p1).hypot();
            for i in 1..=SQUIRCLE_SEGMENTS {
                let t = i as f64 / SQUIRCLE_SEGMENTS as f64;
                let theta = if from_zero {
                    core::f64::consts::FRAC_PI_2 * t
                } else {
                    core::f64::consts::FRAC_PI_2 * (1.0 - t)
                };
                let px = inner.x + sx * rx * theta.cos().abs().sqrt();
                let py = inner.y + sy * ry * theta.sin().abs().sqrt();
                path.line_to((px, py));
            }
        }
    }
}

/// Builds an ellipse outline from four cubic arcs.
fn ellipse_path(cx: f64, cy: f64, rx: f64, ry: f64) -> BezPath {
    let mut p = BezPath::new();
    if rx <= 0.0 || ry <= 0.0 {
        return p;
    }
    let kx = KAPPA * rx;
    let ky = KAPPA * ry;
    p.move_to((cx + rx, cy));
    p.curve_to((cx + rx, cy + ky), (cx + kx, cy + ry), (cx, cy + ry));
    p.curve_to((cx - kx, cy + ry), (cx - rx, cy + ky), (cx - rx, cy));
    p.curve_to((cx - rx, cy - ky), (cx - kx, cy - ry), (cx, cy - ry));
    p.curve_to((cx + kx, cy - ry), (cx + rx, cy - ky), (cx + rx, cy));
    p.close_path();
    p
}

/// Non-zero winding-number point-in-polygon test.
///
/// Casts a horizontal ray in +x from `point` and sums signed edge
/// crossings; a non-zero sum means *inside*. Shared with
/// `martensite_window::hit_test` so visual outlines and hit regions can
/// never diverge. Degenerate polygons (fewer than 3 vertices) and
/// non-finite points return `false`.
///
/// # Examples
///
/// ```
/// use glam::Vec2;
/// use martensite_core::shape::point_in_polygon;
///
/// let tri = [
///     Vec2::new(0.0, 0.0),
///     Vec2::new(10.0, 0.0),
///     Vec2::new(5.0, 10.0),
/// ];
/// assert!(point_in_polygon(&tri, Vec2::new(5.0, 4.0)));
/// assert!(!point_in_polygon(&tri, Vec2::new(9.0, 9.0)));
/// ```
#[must_use]
pub fn point_in_polygon(pts: &[Vec2], point: Vec2) -> bool {
    if pts.len() < 3 || !point.is_finite() {
        return false;
    }
    let n = pts.len();
    let mut winding = 0i32;
    let mut j = n - 1;
    for i in 0..n {
        let pi = pts[i];
        let pj = pts[j];
        if (pi.y <= point.y) != (pj.y <= point.y) {
            // Edge straddles the horizontal ray: compute the x of the
            // crossing and accumulate a signed winding contribution.
            let cross_x = pj.x + (point.y - pj.y) * (pi.x - pj.x) / (pi.y - pj.y);
            if point.x < cross_x {
                if pj.y > pi.y {
                    winding += 1;
                } else {
                    winding -= 1;
                }
            }
        }
        j = i;
    }
    winding != 0
}

/// Convenience alias for [`point_in_polygon`].
#[inline]
#[must_use]
pub fn winding_contains(pts: &[Vec2], point: Vec2) -> bool {
    point_in_polygon(pts, point)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn el_kinds(path: &BezPath) -> Vec<&'static str> {
        path.elements()
            .iter()
            .map(|e| match e {
                PathEl::MoveTo(_) => "M",
                PathEl::LineTo(_) => "L",
                PathEl::QuadTo(..) => "Q",
                PathEl::CurveTo(..) => "C",
                PathEl::ClosePath => "Z",
            })
            .collect()
    }

    const B: Rect = Rect::new(0.0, 0.0, 100.0, 60.0);

    #[test]
    fn rect_is_four_edges() {
        let p = Shape::RECT.to_path(B);
        assert_eq!(el_kinds(&p), vec!["M", "L", "L", "L", "Z"]);
    }

    #[test]
    fn zero_radii_corners_collapse_to_rect() {
        let p = Shape::rounded(0.0).to_path(B);
        assert!(!el_kinds(&p).contains(&"C"));
        assert!(Shape::rounded(0.0).is_rect(B));
    }

    #[test]
    fn round_emits_four_cubics() {
        let p = Shape::rounded(8.0).to_path(B);
        let kinds = el_kinds(&p);
        assert_eq!(kinds.iter().filter(|k| **k == "C").count(), 4);
        assert_eq!(kinds.iter().filter(|k| **k == "L").count(), 4);
        assert_eq!(kinds.last(), Some(&"Z"));
    }

    #[test]
    fn cut_emits_all_lines() {
        let p = Shape::cut(10.0).to_path(B);
        let kinds = el_kinds(&p);
        assert_eq!(kinds.iter().filter(|k| **k == "L").count(), 8);
        assert!(!kinds.contains(&"C"));
    }

    #[test]
    fn squircle_emits_polylines() {
        let p = Shape::squircle(12.0).to_path(B);
        let kinds = el_kinds(&p);
        // 4 straight edges + 4 corners × 12 sampled segments.
        assert_eq!(kinds.iter().filter(|k| **k == "L").count(), 4 + 48);
        assert!(!kinds.contains(&"C"));
    }

    #[test]
    fn notch_dives_inward() {
        // A notch corner recedes: the notch's inner vertex (rx, ry) must
        // appear in the outline for the top-left corner.
        let p = Shape::notch(10.0).to_path(B);
        let has_inner = p.elements().iter().any(|e| {
            matches!(e, PathEl::LineTo(pt) if (pt.x - 10.0).abs() < 1e-6 && (pt.y - 10.0).abs() < 1e-6)
        });
        assert!(has_inner, "notch outline should pass through (10, 10)");
    }

    #[test]
    fn scoop_emits_four_concave_cubics() {
        let p = Shape::scoop(10.0).to_path(B);
        assert_eq!(el_kinds(&p).iter().filter(|k| **k == "C").count(), 4);
    }

    #[test]
    fn per_corner_radii_resolve() {
        let r = CornerRadii::top(10.0);
        let p = Shape::corners(r, CornerStyle::Round).to_path(B);
        // Only two corners carry arcs.
        assert_eq!(el_kinds(&p).iter().filter(|k| **k == "C").count(), 2);
    }

    #[test]
    fn oversized_radii_shrink_proportionally() {
        // CSS shrinking applies ONE common factor to all radii: the
        // tightest edge wins. Here 160 > height 60 dominates → ×0.375.
        let r = CornerRadii::uniform(80.0).resolve(100.0, 60.0);
        assert_eq!(r.top_left, Vec2::splat(30.0));
        // Width alone (160 > 100 → ×0.625) would give 50; height is
        // tighter so both axes land at 30.
        let w_only = CornerRadii::uniform(80.0).resolve(100.0, 1000.0);
        assert_eq!(w_only.top_left, Vec2::splat(50.0));
    }

    #[test]
    fn negative_radii_clamp_to_zero() {
        let r = CornerRadii::all(
            Vec2::new(-4.0, 8.0),
            Vec2::new(8.0, -4.0),
            Vec2::splat(8.0),
            Vec2::splat(0.0),
        )
        .resolve(100.0, 60.0);
        assert_eq!(r.top_left.x, 0.0);
        assert_eq!(r.top_right.y, 0.0);
        assert_eq!(r.bottom_right, Vec2::splat(8.0));
    }

    #[test]
    fn degenerate_bounds_produce_finite_path() {
        for b in [
            Rect::new(0.0, 0.0, 0.0, 0.0),
            Rect::new(5.0, 5.0, 5.0, 5.0),
            Rect::new(0.0, 0.0, 1.0, 100.0),
        ] {
            let p = Shape::squircle(40.0).to_path(b);
            assert!(p.is_finite());
            assert!(Shape::ELLIPSE.to_path(b).is_finite() || b.is_zero_area());
        }
    }

    #[test]
    fn contains_round_corner() {
        let s = Shape::rounded(10.0);
        // (2, 2) sits in the TL corner box, outside the r=10 circle.
        assert!(!s.contains(B, Point::new(2.0, 2.0)));
        assert!(s.contains(B, Point::new(10.0, 10.0)));
        assert!(s.contains(B, Point::new(50.0, 30.0)));
        // Just inside the arc boundary (2·6.8² = 92.5 < 100).
        assert!(s.contains(B, Point::new(3.2, 3.2)));
    }

    #[test]
    fn contains_cut_corner() {
        let s = Shape::cut(10.0);
        // Vertex-side of the TL diagonal (u+v < 1): (1,1) → 0.2 < 1.
        assert!(!s.contains(B, Point::new(1.0, 1.0)));
        // Deeper in the box but still vertex-side: 0.8 < 1 → outside.
        assert!(!s.contains(B, Point::new(4.0, 4.0)));
        // Past the diagonal (u+v = 1.2 > 1): inside.
        assert!(s.contains(B, Point::new(6.0, 6.0)));
    }

    #[test]
    fn contains_notch_is_concave() {
        let s = Shape::notch(10.0);
        // The notch recess (5,5) is carved OUT — inside the rect's AABB
        // but outside the silhouette.
        assert!(!s.contains(B, Point::new(5.0, 5.0)));
        assert!(s.contains(B, Point::new(15.0, 5.0)));
        assert!(s.contains(B, Point::new(5.0, 15.0)));
    }

    #[test]
    fn contains_scoop_is_concave() {
        let s = Shape::scoop(10.0);
        // The scoop recess: (5,5) is cut away by the concave arc.
        assert!(!s.contains(B, Point::new(5.0, 5.0)));
        assert!(s.contains(B, Point::new(50.0, 30.0)));
    }

    #[test]
    fn contains_squircle_fuller_than_round() {
        // Point at (2,2) is outside the r=10 circle (128 > 100) but
        // inside the fuller n=4 superellipse arc (diagonal cut ≈1.59).
        assert!(!Shape::rounded(10.0).contains(B, Point::new(2.0, 2.0)));
        assert!(Shape::squircle(10.0).contains(B, Point::new(2.0, 2.0)));
    }

    #[test]
    fn contains_pill_and_ellipse() {
        let s = Shape::PILL;
        let pill = Rect::new(0.0, 0.0, 100.0, 40.0);
        // End caps are semicircles of r=20.
        assert!(!s.contains(pill, Point::new(2.0, 2.0)));
        assert!(s.contains(pill, Point::new(20.0, 20.0)));

        assert!(Shape::ELLIPSE.contains(pill, Point::new(50.0, 20.0)));
        assert!(!Shape::ELLIPSE.contains(pill, Point::new(1.0, 1.0)));
    }

    #[test]
    fn contains_circle_ignores_bounds() {
        let s = Shape::circle(Vec2::new(50.0, 50.0), 10.0);
        assert!(s.contains(B, Point::new(55.0, 50.0)));
        assert!(!s.contains(B, Point::new(70.0, 50.0)));
        // Bounds don't constrain the circle.
        assert!(s.contains(Rect::ZERO, Point::new(50.0, 50.0)));
    }

    #[test]
    fn contains_mixed_styles_via_winding() {
        // Top corners squircle, bottom corners cut — exercises the
        // flatten+winding fallback for non-uniform styles.
        let s = Shape::Corners {
            radii: CornerRadii::all(
                Vec2::splat(10.0),
                Vec2::splat(10.0),
                Vec2::splat(10.0),
                Vec2::splat(10.0),
            ),
            styles: CornerStyles {
                top_left: CornerStyle::Squircle,
                top_right: CornerStyle::Squircle,
                bottom_right: CornerStyle::Cut,
                bottom_left: CornerStyle::Cut,
            },
        };
        // Squircle TL: (3,3) inside (fuller than round).
        assert!(s.contains(B, Point::new(3.0, 3.0)));
        // Cut BR: (99,59) sits in the vertex triangle → outside.
        assert!(!s.contains(B, Point::new(99.0, 59.0)));
        assert!(s.contains(B, Point::new(50.0, 30.0)));
    }

    #[test]
    fn contains_arbitrary_path() {
        let mut tri = BezPath::new();
        tri.move_to((10.0, 10.0));
        tri.line_to((90.0, 10.0));
        tri.line_to((50.0, 50.0));
        tri.close_path();
        let s = Shape::path(tri);
        assert!(s.contains(B, Point::new(50.0, 20.0)));
        assert!(!s.contains(B, Point::new(15.0, 45.0)));
    }

    #[test]
    fn flatten_returns_polygon() {
        let poly = Shape::ELLIPSE.flatten(B, 0.25);
        assert!(poly.len() > 16);
        // Polygon must approximate the ellipse: a mid-edge point lies
        // near the ellipse outline.
        let top = poly
            .iter()
            .min_by(|a, b| a.y.partial_cmp(&b.y).unwrap())
            .unwrap();
        assert!((top.x - 50.0).abs() < 2.0);
    }

    #[test]
    fn point_in_polygon_winding() {
        // A bowtie has two lobes (left and right) meeting only at the
        // crossing point — the top/bottom gaps are outside.
        let bowtie = [
            Vec2::new(0.0, 0.0),
            Vec2::new(10.0, 10.0),
            Vec2::new(10.0, 0.0),
            Vec2::new(0.0, 10.0),
        ];
        // Inside the right lobe (triangle (5,5)-(10,10)-(10,0)).
        assert!(point_in_polygon(&bowtie, Vec2::new(8.0, 5.0)));
        // Inside the left lobe.
        assert!(point_in_polygon(&bowtie, Vec2::new(2.0, 5.0)));
        // The top gap above the crossing is not part of the bowtie.
        assert!(!point_in_polygon(&bowtie, Vec2::new(5.0, 4.9)));
        assert!(!point_in_polygon(&bowtie, Vec2::new(20.0, 20.0)));
        // Degenerate inputs.
        assert!(!point_in_polygon(&[], Vec2::ZERO));
        assert!(!point_in_polygon(&bowtie[..2], Vec2::new(5.0, 5.0)));
        assert!(!point_in_polygon(&bowtie, Vec2::NAN));
    }
}
