//! Charts & dataviz category — one live card per chart-widget module.
//!
//! Each entry adapts the widget's own doctest construction (the
//! verified canonical shape); demo data is industrial-flavoured where
//! it does not distort that shape.

use martensite::core::Widget;
use martensite::widgets::bar_chart::BarChart;
use martensite::widgets::box_plot::{BoxPlot, BoxSeries};
use martensite::widgets::bullet_chart::BulletChart;
use martensite::widgets::burndown::Burndown;
use martensite::widgets::candlestick::{Candle, Candlestick};
use martensite::widgets::fishbone::{Bone, Fishbone};
use martensite::widgets::funnel_chart::FunnelChart;
use martensite::widgets::gantt::Gantt;
use martensite::widgets::graph_view::GraphView;
use martensite::widgets::heat_map::HeatMap;
use martensite::widgets::histogram::Histogram;
use martensite::widgets::legend::Legend;
use martensite::widgets::line_chart::{LineChart, LineSeries};
use martensite::widgets::mind_map::MindMap;
use martensite::widgets::org_chart::{OrgChart, OrgNode};
use martensite::widgets::pie_chart::{PieChart, PieSlice};
use martensite::widgets::polar_area::PolarArea;
use martensite::widgets::quadrant::{Quadrant, QuadrantItem};
use martensite::widgets::radar_chart::{RadarChart, RadarSeries};
use martensite::widgets::sankey::Sankey;
use martensite::widgets::scatter_chart::{ScatterChart, ScatterSeries};
use martensite::widgets::sparkline::{SparkStyle, Sparkline};
use martensite::widgets::stream_graph::StreamGraph;
use martensite::widgets::strip_chart::StripChart;
use martensite::widgets::sunburst::{Sunburst, SunburstNode};
use martensite::widgets::ticker_tape::{TickerItem, TickerTape};
use martensite::widgets::timeline::{Timeline, TimelineDot, TimelineItem};
use martensite::widgets::treemap::{Treemap, TreemapItem};
use martensite::widgets::venn::Venn;
use martensite::widgets::violin::Violin;
use martensite::widgets::waterfall::Waterfall;
use martensite::widgets::word_cloud::WordCloud;

/// Charts & dataviz showcase entries — `(display name, live widget)`.
pub fn entries() -> Vec<(&'static str, Box<dyn Widget>)> {
    vec![
        (
            "Bar Chart",
            Box::new(
                BarChart::new()
                    .bar("Shift A", 84.0)
                    .bar("Shift B", 92.0)
                    .bar("Shift C", 71.0)
                    .label("Units per shift"),
            ),
        ),
        (
            "Box Plot",
            Box::new(
                BoxPlot::new()
                    .series(BoxSeries::new("Line 1", 42.0, 58.0, 67.0, 74.0, 91.0))
                    .series(BoxSeries::new("Line 2", 38.0, 55.0, 63.0, 70.0, 88.0))
                    .label("Cycle-time spread (s)"),
            ),
        ),
        (
            "Bullet Chart",
            Box::new(
                BulletChart::new()
                    .label("OEE %")
                    .value(78.0)
                    .target(85.0)
                    .ranges([60.0, 80.0, 100.0]),
            ),
        ),
        (
            "Burndown",
            Box::new({
                let mut b = Burndown::new(120.0, 10).label("Sprint 14 — backlog pts");
                b.push_day(108.0);
                b.push_day(96.0);
                b.push_day(88.0);
                b.push_day(74.0);
                b
            }),
        ),
        (
            "Candlestick",
            Box::new(
                Candlestick::new()
                    .candles([
                        Candle::new(10.0, 12.0, 9.0, 11.0),
                        Candle::new(11.0, 13.0, 10.5, 12.5),
                        Candle::new(12.5, 12.8, 11.0, 11.5),
                        Candle::new(11.5, 14.0, 11.2, 13.5),
                    ])
                    .grid(true),
            ),
        ),
        (
            "Funnel Chart",
            Box::new(
                FunnelChart::new()
                    .stage("Work orders", 420.0)
                    .stage("Scheduled", 310.0)
                    .stage("Completed", 262.0)
                    .stage("QA passed", 244.0),
            ),
        ),
        (
            "Gantt",
            Box::new(
                Gantt::new()
                    .total_days(14.0)
                    .task("Install", 0.0, 4.0)
                    .progress(1.0)
                    .task("Wiring", 4.0, 5.0)
                    .progress(0.6)
                    .task("Commissioning", 9.0, 3.0)
                    .task("Handover", 12.0, 2.0),
            ),
        ),
        (
            "Heat Map",
            Box::new({
                let mut hm = HeatMap::new(4, 7);
                for row in 0..4 {
                    for col in 0..7 {
                        hm.set_cell(row, col, ((row * 3 + col * 2) % 10) as f32);
                    }
                }
                hm
            }),
        ),
        (
            "Histogram",
            Box::new(
                Histogram::new()
                    .bins(6)
                    .samples([0.1, 0.2, 0.25, 0.4, 0.45, 0.5, 0.55, 0.7, 0.8, 0.95])
                    .label("Fill-level distribution"),
            ),
        ),
        (
            "Legend",
            Box::new(
                Legend::new()
                    .entry("Line 1", [96, 165, 250, 255])
                    .entry("Line 2", [110, 180, 130, 255])
                    .entry("Line 3", [230, 170, 80, 255]),
            ),
        ),
        (
            "Line Chart",
            Box::new(
                LineChart::new()
                    .series(LineSeries::new(
                        "Throughput",
                        [62.0, 71.0, 68.0, 80.0, 84.0, 79.0],
                    ))
                    .series(LineSeries::new(
                        "Target",
                        [70.0, 70.0, 75.0, 75.0, 80.0, 80.0],
                    ))
                    .axis(true),
            ),
        ),
        (
            "Pie Chart",
            Box::new(
                PieChart::new(vec![
                    PieSlice::new(38.0, "Stamping"),
                    PieSlice::new(27.0, "Assembly"),
                    PieSlice::new(21.0, "Packaging"),
                    PieSlice::new(14.0, "Rework"),
                ])
                .donut(),
            ),
        ),
        (
            "Polar Area",
            Box::new(
                PolarArea::new()
                    .slice("Stamping", 7.0)
                    .slice("Welding", 5.0)
                    .slice("Painting", 8.0)
                    .slice("Assembly", 6.0),
            ),
        ),
        (
            "Quadrant",
            Box::new(
                Quadrant::new("Impact", "Effort")
                    .regions(["Quick wins", "Big bets", "Fill-ins", "Money pits"])
                    .item(QuadrantItem::new("Swap worn dies", 0.85, 0.3))
                    .item(QuadrantItem::new("Retool Line 3", 0.9, 0.85))
                    .item(QuadrantItem::new("Label fix", 0.2, 0.15)),
            ),
        ),
        (
            "Radar Chart",
            Box::new(
                RadarChart::new()
                    .axes(["OEE", "Yield", "Uptime", "Safety", "Energy"])
                    .series(RadarSeries::new("Line 1", [4.0, 3.5, 4.5, 5.0, 3.0]))
                    .series(RadarSeries::new("Line 2", [3.0, 4.0, 3.5, 4.5, 4.0])),
            ),
        ),
        (
            "Sankey",
            Box::new(
                Sankey::new()
                    .node("Raw stock")
                    .node("Line 1")
                    .node("Line 2")
                    .node("Packaging")
                    .link("Raw stock", "Line 1", 60.0)
                    .link("Raw stock", "Line 2", 40.0)
                    .link("Line 1", "Packaging", 56.0)
                    .link("Line 2", "Packaging", 37.0),
            ),
        ),
        (
            "Scatter Chart",
            Box::new(
                ScatterChart::new()
                    .series(ScatterSeries::new(
                        "Torque vs temp",
                        [
                            (20.0, 41.0),
                            (35.0, 44.0),
                            (50.0, 43.0),
                            (65.0, 47.0),
                            (80.0, 52.0),
                        ],
                    ))
                    .grid(true),
            ),
        ),
        (
            "Sparkline",
            Box::new(
                Sparkline::new([4.0, 6.0, 5.0, 8.0, 7.0, 9.0, 8.5, 10.0])
                    .style(SparkStyle::Area)
                    .label("Hourly output"),
            ),
        ),
        (
            "Stream Graph",
            Box::new(
                StreamGraph::new()
                    .layer("Shift A", [3.0, 5.0, 4.0, 6.0, 5.0])
                    .layer("Shift B", [2.0, 3.0, 4.0, 3.0, 4.5])
                    .layer("Shift C", [1.0, 1.5, 2.0, 2.5, 2.0]),
            ),
        ),
        (
            "Strip Chart",
            Box::new({
                let mut s = StripChart::new()
                    .capacity(60)
                    .range(0.0, 100.0)
                    .label("Motor temp °C");
                s.extend([62.0, 63.0, 64.5, 63.5, 65.0, 66.0, 65.5, 67.0]);
                s
            }),
        ),
        (
            "Sunburst",
            Box::new(
                Sunburst::new()
                    .node(
                        SunburstNode::new("Plant East", 10.0)
                            .child(SunburstNode::new("Line 1", 4.0))
                            .child(SunburstNode::new("Line 2", 6.0)),
                    )
                    .node(
                        SunburstNode::new("Plant West", 8.0)
                            .child(SunburstNode::new("Line 3", 8.0)),
                    ),
            ),
        ),
        (
            "Ticker Tape",
            Box::new(
                TickerTape::new()
                    .item(TickerItem::new("OEE", "78.4%", 0.012))
                    .item(TickerItem::new("YIELD", "96.1%", 0.004))
                    .item(TickerItem::new("SCRAP", "2.3%", -0.007))
                    .item(TickerItem::new("ENERGY", "412kWh", -0.011)),
            ),
        ),
        (
            "Treemap",
            Box::new(
                Treemap::new()
                    .item(TreemapItem::new("Stamping", 42.0))
                    .item(TreemapItem::new("Assembly", 30.0))
                    .item(TreemapItem::new("Packaging", 18.0))
                    .item(TreemapItem::new("Tool room", 10.0)),
            ),
        ),
        (
            "Venn",
            Box::new(
                Venn::new()
                    .set("Day shift")
                    .set("Certified")
                    .set("Forklift"),
            ),
        ),
        (
            "Waterfall",
            Box::new(
                Waterfall::new()
                    .total("Planned", 1000.0)
                    .delta("Scrap", -40.0)
                    .delta("Downtime", -65.0)
                    .delta("Overtime", 35.0)
                    .total("Actual", 930.0),
            ),
        ),
        (
            "Word Cloud",
            Box::new(
                WordCloud::new()
                    .word("downtime", 10.0)
                    .word("changeover", 7.0)
                    .word("scrap", 6.0)
                    .word("maintenance", 5.0)
                    .word("calibration", 3.0),
            ),
        ),
        (
            "Violin",
            Box::new(
                Violin::new()
                    .series("Line 1", [0.1, 0.4, 0.9, 1.0, 0.9, 0.4, 0.1])
                    .series("Line 2", [0.2, 0.7, 0.8, 0.7, 0.5, 0.3, 0.2]),
            ),
        ),
        (
            "Fishbone",
            Box::new(
                Fishbone::new("Unplanned downtime")
                    .bone(
                        Bone::new("Machine")
                            .cause("worn bearing")
                            .cause("lube schedule"),
                    )
                    .bone(Bone::new("Method").cause("no SMED checklist"))
                    .bone(Bone::new("People").cause("training gap")),
            ),
        ),
        (
            "Graph View",
            Box::new(
                GraphView::new()
                    .node("PLC-1")
                    .node("HMI-3")
                    .node("SCADA")
                    .node("Historian")
                    .edge(0, 1)
                    .edge(0, 2)
                    .edge(2, 3),
            ),
        ),
        (
            "Mind Map",
            Box::new(
                MindMap::new()
                    .root("Line 3")
                    .child("Line 3", "OEE")
                    .child("Line 3", "Maintenance")
                    .child("Line 3", "Staffing")
                    .child("OEE", "Downtime")
                    .child("OEE", "Scrap"),
            ),
        ),
        (
            "Org Chart",
            Box::new(OrgChart::new(
                OrgNode::new("Plant Manager", "Site")
                    .child(
                        OrgNode::new("Shift Lead A", "06–14")
                            .child(OrgNode::new("Operators", "12 head")),
                    )
                    .child(OrgNode::new("QA Lead", "Quality"))
                    .child(OrgNode::new("Maintenance", "Facilities")),
            )),
        ),
        (
            "Timeline",
            Box::new(
                Timeline::new()
                    .item(TimelineItem::new("Shift started").subtitle("06:00"))
                    .item(
                        TimelineItem::new("Changeover complete")
                            .subtitle("10:15")
                            .dot(TimelineDot::Success),
                    )
                    .item(
                        TimelineItem::new("Temp alarm cleared")
                            .subtitle("13:40")
                            .dot(TimelineDot::Warning),
                    )
                    .pending("Running…"),
            ),
        ),
    ]
}
