//! GPU-friendly chart geometry generation.

pub use martensite_render::Point;

/// Data-space axis bounds.
///
/// # Examples
///
/// ```
/// use martensite_blessed::ChartBounds;
/// let bounds = ChartBounds::new(0.0, 10.0, -1.0, 1.0);
/// assert_eq!(bounds.width(), 10.0);
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChartBounds {
    /// Minimum x value.
    pub x_min: f64,
    /// Maximum x value.
    pub x_max: f64,
    /// Minimum y value.
    pub y_min: f64,
    /// Maximum y value.
    pub y_max: f64,
}

impl ChartBounds {
    /// Creates normalized bounds, expanding zero-sized axes.
    pub fn new(x_min: f64, x_max: f64, y_min: f64, y_max: f64) -> Self {
        let (x_min, x_max) = normalized_axis(x_min, x_max);
        let (y_min, y_max) = normalized_axis(y_min, y_max);
        Self {
            x_min,
            x_max,
            y_min,
            y_max,
        }
    }

    /// Returns the x-axis span.
    pub fn width(self) -> f64 {
        self.x_max - self.x_min
    }

    /// Returns the y-axis span.
    pub fn height(self) -> f64 {
        self.y_max - self.y_min
    }
}

/// A connected line series.
///
/// # Examples
///
/// ```
/// use martensite_blessed::{LineSeries, Point};
/// let line = LineSeries::new(vec![Point::new(0.0, 1.0)]);
/// assert_eq!(line.points.len(), 1);
/// ```
#[derive(Clone, Debug, Default)]
pub struct LineSeries {
    /// Data-space points uploaded as one contiguous path.
    pub points: Vec<Point>,
}
impl LineSeries {
    /// Creates a line series.
    pub fn new(points: Vec<Point>) -> Self {
        Self { points }
    }
}

/// A filled series between its points and a baseline.
///
/// # Examples
///
/// ```
/// use martensite_blessed::{AreaSeries, Point};
/// let area = AreaSeries::new(vec![Point::new(1.0, 2.0)], 0.0);
/// assert_eq!(area.baseline, 0.0);
/// ```
#[derive(Clone, Debug, Default)]
pub struct AreaSeries {
    /// Data-space points along the area's upper edge.
    pub points: Vec<Point>,
    /// Data-space y coordinate closing the area.
    pub baseline: f64,
}
impl AreaSeries {
    /// Creates an area series.
    pub fn new(points: Vec<Point>, baseline: f64) -> Self {
        Self { points, baseline }
    }
}

/// A collection of independent scatter points.
///
/// # Examples
///
/// ```
/// use martensite_blessed::{Point, ScatterSeries};
/// let scatter = ScatterSeries::new(vec![Point::new(1.0, 2.0)], 3.0);
/// assert_eq!(scatter.radius, 3.0);
/// ```
#[derive(Clone, Debug)]
pub struct ScatterSeries {
    /// Data-space point centers.
    pub points: Vec<Point>,
    /// Marker radius in logical pixels.
    pub radius: f64,
}
impl ScatterSeries {
    /// Creates a scatter series.
    pub fn new(points: Vec<Point>, radius: f64) -> Self {
        Self {
            points,
            radius: radius.max(0.0),
        }
    }
}

/// A chart containing line, area, and scatter series.
///
/// Geometry remains in contiguous point arrays so a renderer can upload ten
/// thousand points in a small number of GPU transfers.
///
/// # Examples
///
/// ```
/// use martensite_blessed::{Chart, LineSeries, Point};
/// let mut chart = Chart::new();
/// chart.add_line(LineSeries::new(vec![Point::new(0.0, 2.0), Point::new(1.0, 4.0)]));
/// assert!(chart.bounds().is_some());
/// ```
#[derive(Clone, Debug, Default)]
pub struct Chart {
    lines: Vec<LineSeries>,
    areas: Vec<AreaSeries>,
    scatters: Vec<ScatterSeries>,
}

impl Chart {
    /// Creates an empty chart.
    pub fn new() -> Self {
        Self::default()
    }
    /// Adds a line series.
    pub fn add_line(&mut self, series: LineSeries) {
        self.lines.push(series);
    }
    /// Adds an area series.
    pub fn add_area(&mut self, series: AreaSeries) {
        self.areas.push(series);
    }
    /// Adds a scatter series.
    pub fn add_scatter(&mut self, series: ScatterSeries) {
        self.scatters.push(series);
    }
    /// Returns line series.
    pub fn lines(&self) -> &[LineSeries] {
        &self.lines
    }
    /// Returns area series.
    pub fn areas(&self) -> &[AreaSeries] {
        &self.areas
    }
    /// Returns scatter series.
    pub fn scatters(&self) -> &[ScatterSeries] {
        &self.scatters
    }

    /// Computes finite auto-scaled axes across all series.
    pub fn bounds(&self) -> Option<ChartBounds> {
        let line_points = self.lines.iter().flat_map(|series| series.points.iter());
        let scatter_points = self.scatters.iter().flat_map(|series| series.points.iter());
        let area_points = self.areas.iter().flat_map(|series| series.points.iter());
        let mut points = line_points
            .chain(scatter_points)
            .chain(area_points)
            .filter(|point| point.x.is_finite() && point.y.is_finite());
        let first = points.next()?;
        let (mut x_min, mut x_max, mut y_min, mut y_max) = (first.x, first.x, first.y, first.y);
        for point in points {
            x_min = x_min.min(point.x);
            x_max = x_max.max(point.x);
            y_min = y_min.min(point.y);
            y_max = y_max.max(point.y);
        }
        for area in &self.areas {
            if area.baseline.is_finite() {
                y_min = y_min.min(area.baseline);
                y_max = y_max.max(area.baseline);
            }
        }
        Some(ChartBounds::new(x_min, x_max, y_min, y_max))
    }

    /// Projects a data point into a logical-pixel viewport.
    ///
    /// If `bounds` has zero width or height, the original `point` is returned
    /// unchanged to avoid a divide-by-zero.
    pub fn project(point: Point, bounds: ChartBounds, width: f64, height: f64) -> Point {
        if bounds.width() == 0.0 || bounds.height() == 0.0 {
            return point;
        }
        Point::new(
            (point.x - bounds.x_min) / bounds.width() * width,
            height - (point.y - bounds.y_min) / bounds.height() * height,
        )
    }
}

fn normalized_axis(a: f64, b: f64) -> (f64, f64) {
    let (mut low, mut high) = if a <= b { (a, b) } else { (b, a) };
    if !low.is_finite() || !high.is_finite() {
        return (0.0, 1.0);
    }
    if low == high {
        let padding = low.abs().max(1.0) * 0.5;
        low -= padding;
        high += padding;
    }
    (low, high)
}

#[cfg(test)]
mod tests {
    use super::{AreaSeries, Chart, LineSeries, Point};
    #[test]
    fn bounds_include_points_and_area_baseline() {
        let mut chart = Chart::new();
        chart.add_line(LineSeries::new(vec![
            Point::new(-2.0, 3.0),
            Point::new(8.0, 5.0),
        ]));
        chart.add_area(AreaSeries::new(vec![Point::new(2.0, 4.0)], -10.0));
        let bounds = chart.bounds().expect("non-empty chart");
        assert_eq!(
            (bounds.x_min, bounds.x_max, bounds.y_min, bounds.y_max),
            (-2.0, 8.0, -10.0, 5.0)
        );
    }
    #[test]
    fn ten_thousand_points_remain_contiguous() {
        let points = (0..10_000).map(|x| Point::new(f64::from(x), 0.0)).collect();
        let line = LineSeries::new(points);
        assert_eq!(line.points.len(), 10_000);
    }
}
