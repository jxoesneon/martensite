//! `data` namespace — density, charts, and data shaping.

use super::super::{IconEntry, IconPair};

/// Qualified icon names (`data.*`).
pub mod names {
    /// `"data.activity"` — waveform / live signal.
    pub const ACTIVITY: &str = "data.activity";
    /// `"data.flatline"` — flat trace / stopped signal.
    pub const FLATLINE: &str = "data.flatline";
    /// `"data.chart"` — bar chart.
    pub const CHART: &str = "data.chart";
    /// `"data.grid"` — grid layout / tiles.
    pub const GRID: &str = "data.grid";
    /// `"data.rows"` — row layout / list.
    pub const ROWS: &str = "data.rows";
    /// `"data.filter"` — funnel / filter.
    pub const FILTER: &str = "data.filter";
    /// `"data.chart-line"` — line chart.
    pub const DATA_CHART_LINE: &str = "data.chart-line";
    /// `"data.chart-pie"` — pie chart.
    pub const DATA_CHART_PIE: &str = "data.chart-pie";
    /// `"data.chart-area"` — filled area chart.
    pub const DATA_CHART_AREA: &str = "data.chart-area";
    /// `"data.chart-scatter"` — scatter plot.
    pub const DATA_CHART_SCATTER: &str = "data.chart-scatter";
    /// `"data.chart-horizontal"` — horizontal bar chart.
    pub const DATA_CHART_HORIZONTAL: &str = "data.chart-horizontal";
    /// `"data.trending-up"` — upward trend.
    pub const DATA_TRENDING_UP: &str = "data.trending-up";
    /// `"data.trending-down"` — downward trend.
    pub const DATA_TRENDING_DOWN: &str = "data.trending-down";
    /// `"data.table"` — data table.
    pub const DATA_TABLE: &str = "data.table";
    /// `"data.sort-asc"` — ascending sort.
    pub const DATA_SORT_ASC: &str = "data.sort-asc";
    /// `"data.sort-desc"` — descending sort.
    pub const DATA_SORT_DESC: &str = "data.sort-desc";
    /// `"data.calculator"` — calculator.
    pub const DATA_CALCULATOR: &str = "data.calculator";
    /// `"data.percent"` — percentage.
    pub const DATA_PERCENT: &str = "data.percent";
    /// `"data.hash"` — number / count.
    pub const DATA_HASH: &str = "data.hash";
    /// `"data.sigma"` — summation.
    pub const DATA_SIGMA: &str = "data.sigma";
}

/// `"data.activity"` — ECG/polyline pulse.
pub const DATA_ACTIVITY: &str = "M22 12h-4l-3 9L9 3l-3 9H2";
/// `"data.flatline"` — flat trace across the midline.
pub const DATA_FLATLINE: &str = "M3 12h18";
/// `"data.chart"` — axes plus three bars.
pub const DATA_CHART: &str = "M4 4v16h16M9 16v-5M13.5 16V8M18 16v-8";
/// `"data.grid"` — four quadrants.
pub const DATA_GRID: &str = "M4 4h6v6H4zM14 4h6v6H4zM4 14h6v6H4zM14 14h6v6H4z";
/// `"data.rows"` — two stacked row blocks.
pub const DATA_ROWS: &str = "M4 5h16v5H4zM4 14h16v5H4z";
/// `"data.filter"` — funnel silhouette.
pub const DATA_FILTER: &str = "M4 5h16l-6.5 7.5V20l-3-2v-5.5L4 5z";
/// `"data.chart-line"` — axes plus a rising polyline.
pub const DATA_CHART_LINE: &str = "M4 4v16h16M7 15l4-4 3 3 5-6";
/// `"data.chart-pie"` — pie disc with a cut quarter.
pub const DATA_CHART_PIE: &str = "M21.21 15.89A10 10 0 118 2.83M22 12A10 10 0 0012 2v10z";
/// `"data.chart-area"` — axes plus a filled area outline.
pub const DATA_CHART_AREA: &str = "M4 4v16h16M7 20v-7l4-4 3 3 6-6v14z";
/// `"data.chart-scatter"` — axes plus five data points.
pub const DATA_CHART_SCATTER: &str =
    "M4 4v16h16M8 8h0.01M17.5 6.5h0.01M12 12h0.01M8 16h0.01M17 14.5h0.01";
/// `"data.chart-horizontal"` — axes plus three horizontal bars.
pub const DATA_CHART_HORIZONTAL: &str = "M4 4v16h16M8 8h8M8 12h11M8 16h5";
/// `"data.trending-up"` — rising arrow trace.
pub const DATA_TRENDING_UP: &str = "M22 7l-8.5 8.5-5-5L2 17M16 7h6v6";
/// `"data.trending-down"` — falling arrow trace.
pub const DATA_TRENDING_DOWN: &str = "M22 17l-8.5-8.5-5 5L2 7M16 17h6v-6";
/// `"data.table"` — gridded table.
pub const DATA_TABLE: &str =
    "M5 3h14a2 2 0 012 2v14a2 2 0 01-2 2H5a2 2 0 01-2-2V5a2 2 0 012-2zM3 9h18M3 15h18M12 3v18";
/// `"data.sort-asc"` — shrinking rows plus a downward arrow.
pub const DATA_SORT_ASC: &str = "M11 5h10M11 9h7M11 13h4M7 9v12M3 17l4 4 4-4";
/// `"data.sort-desc"` — growing rows plus an upward arrow.
pub const DATA_SORT_DESC: &str = "M11 7h10M11 11h7M11 15h4M7 3v12M3 7l4-4 4 4";
/// `"data.calculator"` — body, display, and a key grid.
pub const DATA_CALCULATOR: &str =
    "M6 2h12a2 2 0 012 2v16a2 2 0 01-2 2H6a2 2 0 01-2-2V4a2 2 0 012-2zM8 6h8M16 10h0.01M12 10h0.01M8 10h0.01M16 14h0.01M12 14h0.01M8 14h0.01M16 18h0.01M12 18h0.01M8 18h0.01";
/// `"data.percent"` — two dots and a diagonal slash.
pub const DATA_PERCENT: &str =
    "M19 5L5 19M9 6.5a2.5 2.5 0 11-5 0 2.5 2.5 0 015 0zM19 17.5a2.5 2.5 0 11-5 0 2.5 2.5 0 015 0z";
/// `"data.hash"` — hash / pound sign.
pub const DATA_HASH: &str = "M4 9h16M4 15h16M10 3L8 21M16 3l-2 18";
/// `"data.sigma"` — summation sign.
pub const DATA_SIGMA: &str = "M18 7V4H6l5.5 8L6 20h12v-3";

/// `data` entries.
pub const ENTRIES: &[IconEntry] = &[
    IconEntry::new(names::ACTIVITY, DATA_ACTIVITY),
    IconEntry::new(names::FLATLINE, DATA_FLATLINE),
    IconEntry::new(names::CHART, DATA_CHART),
    IconEntry::new(names::GRID, DATA_GRID),
    IconEntry::new(names::ROWS, DATA_ROWS),
    IconEntry::new(names::FILTER, DATA_FILTER),
    IconEntry::new(names::DATA_CHART_LINE, DATA_CHART_LINE),
    IconEntry::new(names::DATA_CHART_PIE, DATA_CHART_PIE),
    IconEntry::new(names::DATA_CHART_AREA, DATA_CHART_AREA),
    IconEntry::new(names::DATA_CHART_SCATTER, DATA_CHART_SCATTER),
    IconEntry::new(names::DATA_CHART_HORIZONTAL, DATA_CHART_HORIZONTAL),
    IconEntry::new(names::DATA_TRENDING_UP, DATA_TRENDING_UP),
    IconEntry::new(names::DATA_TRENDING_DOWN, DATA_TRENDING_DOWN),
    IconEntry::new(names::DATA_TABLE, DATA_TABLE),
    IconEntry::new(names::DATA_SORT_ASC, DATA_SORT_ASC),
    IconEntry::new(names::DATA_SORT_DESC, DATA_SORT_DESC),
    IconEntry::new(names::DATA_CALCULATOR, DATA_CALCULATOR),
    IconEntry::new(names::DATA_PERCENT, DATA_PERCENT),
    IconEntry::new(names::DATA_HASH, DATA_HASH),
    IconEntry::new(names::DATA_SIGMA, DATA_SIGMA),
];

/// `data` morph pairs.
pub const PAIRS: &[IconPair] = &[
    IconPair::new(names::ACTIVITY, names::FLATLINE),
    IconPair::new(names::DATA_TRENDING_UP, names::DATA_TRENDING_DOWN),
    IconPair::new(names::DATA_SORT_ASC, names::DATA_SORT_DESC),
];
