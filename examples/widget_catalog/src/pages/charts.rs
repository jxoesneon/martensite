//! Charts family — data-visualization widgets.

use martensite::widgets::bar_chart::BarChart;
use martensite::widgets::box_plot::{BoxPlot, BoxSeries};
use martensite::widgets::bullet_chart::BulletChart;
use martensite::widgets::burndown::Burndown;
use martensite::widgets::candlestick::{Candle, Candlestick};
use martensite::widgets::equalizer::Equalizer;
use martensite::widgets::funnel_chart::FunnelChart;
use martensite::widgets::gantt::Gantt;
use martensite::widgets::heat_map::HeatMap;
use martensite::widgets::histogram::Histogram;
use martensite::widgets::line_chart::{LineChart, LineSeries};
use martensite::widgets::pie_chart::{PieChart, PieSlice};
use martensite::widgets::polar_area::PolarArea;
use martensite::widgets::radar_chart::{RadarChart, RadarSeries};
use martensite::widgets::sankey::Sankey;
use martensite::widgets::scatter_chart::{ScatterChart, ScatterSeries};
use martensite::widgets::sparkline::Sparkline;
use martensite::widgets::spectrum::Spectrum;
use martensite::widgets::stream_graph::StreamGraph;
use martensite::widgets::strip_chart::StripChart;
use martensite::widgets::sunburst::{Sunburst, SunburstNode};
use martensite::widgets::treemap::{Treemap, TreemapItem};
use martensite::widgets::violin::Violin;
use martensite::widgets::waterfall::Waterfall;
use martensite::widgets::waveform::Waveform;

use crate::page::{Page, PropSpec};
use crate::pages::{downcast_mut, meta, page};

page!(BarChartPage {
    meta: meta(
        "BarChart",
        "Charts",
        "Vertical bars with axis labels.",
        "Chart",
        &[
            ("Qt", "QtCharts bar"),
            ("Excel", "column chart"),
            ("React", "bar chart"),
            ("D3", "bar")
        ],
        false,
    ),
    props: &[PropSpec::Int {
        key: "bars",
        label: "Bars",
        min: 2,
        max: 12,
        default: 5
    }],
    build: |p| {
        let mut c = BarChart::new().label("Quarterly");
        for i in 0..p.i64("bars") {
            c = c.bar(format!("B{}", i + 1), 20.0 + i as f32 * 12.0);
        }
        Box::new(c)
    },
    snippet: |p| format!(
        "BarChart::new().label(\"Quarterly\") /* {} bars */",
        p.i64("bars")
    ),
});

page!(LineChartPage {
    meta: meta(
        "LineChart",
        "Charts",
        "Multi-series line chart with hover probe.",
        "Chart",
        &[
            ("Qt", "QLineSeries"),
            ("D3", "line"),
            ("React", "line chart"),
            ("Excel", "line")
        ],
        false,
    ),
    props: &[PropSpec::Bool {
        key: "axis",
        label: "Axis",
        default: true
    }],
    build: |p| {
        let mut c = LineChart::new().axis(p.bool("axis"));
        c = c.series(
            LineSeries::new("cpu", [12.0, 18.0, 9.0, 24.0, 30.0, 22.0]).color([80, 140, 255, 255]),
        );
        c = c.series(
            LineSeries::new("mem", [40.0, 38.0, 44.0, 30.0, 26.0, 34.0]).color([240, 160, 60, 255]),
        );
        Box::new(c)
    },
    snippet: |p| format!(
        "LineChart::new().axis({}).series(LineSeries::new(\"cpu\", […]))",
        p.bool("axis"),
    ),
    poll: |w, out| {
        if let Some(c) = downcast_mut::<LineChart>(w) {
            if let Some(i) = c.take_hovered() {
                out.push(format!("hover point {i}"));
            }
        }
    },
});

page!(PieChartPage {
    meta: meta(
        "PieChart",
        "Charts",
        "Pie/donut slices — click to select.",
        "Chart",
        &[
            ("Qt", "QPieSeries"),
            ("Excel", "pie"),
            ("React", "pie chart"),
            ("D3", "pie")
        ],
        false,
    ),
    props: &[PropSpec::Bool {
        key: "donut",
        label: "Donut",
        default: true
    }],
    build: |p| {
        let slices = vec![
            PieSlice::new(40.0, "Rust").color([230, 120, 60, 255]),
            PieSlice::new(30.0, "Go").color([80, 170, 230, 255]),
            PieSlice::new(30.0, "C++").color([90, 200, 120, 255]),
        ];
        let mut c = PieChart::new(slices);
        if p.bool("donut") {
            c = c.donut();
        }
        Box::new(c)
    },
    snippet: |p| format!("PieChart::new(slices) /* donut={} */", p.bool("donut")),
    poll: |w, out| {
        if let Some(c) = downcast_mut::<PieChart>(w) {
            if let Some(i) = c.take_selected() {
                out.push(format!("slice {i}"));
            }
        }
    },
});

page!(CandlestickPage {
    meta: meta(
        "Candlestick",
        "Charts",
        "OHLC candlestick chart.",
        "Chart",
        &[
            ("Trading", "candles"),
            ("Qt", "QCandlestick"),
            ("D3", "ohlc"),
            ("React", "candlestick")
        ],
        false,
    ),
    props: &[PropSpec::Bool {
        key: "grid",
        label: "Grid",
        default: true
    }],
    build: |p| {
        let mut c = Candlestick::new().grid(p.bool("grid")).y_range(90.0, 130.0);
        for (o, h, l, cl) in [
            (100.0, 110.0, 95.0, 108.0),
            (108.0, 118.0, 104.0, 115.0),
            (115.0, 116.0, 100.0, 102.0),
            (102.0, 112.0, 98.0, 110.0),
        ] {
            c = c.candle(Candle::new(o, h, l, cl));
        }
        Box::new(c)
    },
    snippet: |p| format!(
        "Candlestick::new().grid({}).candle(Candle::new(…))",
        p.bool("grid")
    ),
    poll: |w, out| {
        if let Some(c) = downcast_mut::<Candlestick>(w) {
            if let Some(i) = c.take_hovered() {
                out.push(format!("candle {i}"));
            }
        }
    },
});

page!(HistogramPage {
    meta: meta(
        "Histogram",
        "Charts",
        "Binned frequency distribution.",
        "Chart",
        &[
            ("D3", "histogram"),
            ("Excel", "histogram"),
            ("Qt", "custom"),
            ("React", "histogram")
        ],
        false,
    ),
    props: &[PropSpec::Int {
        key: "bins",
        label: "Bins",
        min: 4,
        max: 40,
        default: 12
    }],
    build: |p| Box::new(
        Histogram::new()
            .label("Response times")
            .bins(p.i64("bins") as usize)
            .counts(
                (0..p.i64("bins"))
                    .map(|i| ((i * 37) % 24 + 2) as usize)
                    .collect()
            ),
    ),
    snippet: |p| format!("Histogram::new().bins({}).counts(vec![…])", p.i64("bins")),
    poll: |w, out| {
        if let Some(h) = downcast_mut::<Histogram>(w) {
            if let Some(i) = h.take_hovered() {
                out.push(format!("bin {i}"));
            }
        }
    },
});

page!(HeatMapPage {
    meta: meta(
        "HeatMap",
        "Charts",
        "Grid heat map — cells colored by value.",
        "Chart",
        &[
            ("GitHub", "contribution graph"),
            ("D3", "heatmap"),
            ("React", "heat map"),
            ("Qt", "custom")
        ],
        false,
    ),
    props: &[
        PropSpec::Int {
            key: "rows",
            label: "Rows",
            min: 2,
            max: 12,
            default: 7
        },
        PropSpec::Int {
            key: "cols",
            label: "Cols",
            min: 2,
            max: 24,
            default: 14
        },
    ],
    build: |p| {
        let mut h = HeatMap::new(p.i64("rows") as usize, p.i64("cols") as usize);
        for r in 0..p.i64("rows") {
            for c in 0..p.i64("cols") {
                h.set_cell(r as usize, c as usize, ((r * c) % 10) as f32 / 10.0);
            }
        }
        Box::new(h)
    },
    snippet: |p| format!(
        "HeatMap::new({}, {}) /* + set_cell */",
        p.i64("rows"),
        p.i64("cols"),
    ),
    poll: |w, out| {
        if let Some(h) = downcast_mut::<HeatMap>(w) {
            if let Some((r, c, v)) = h.take_hovered() {
                out.push(format!("cell ({r},{c}) = {v:.2}"));
            }
        }
    },
});

page!(ScatterChartPage {
    meta: meta(
        "ScatterChart",
        "Charts",
        "X/Y point cloud with grid.",
        "Chart",
        &[
            ("D3", "scatter"),
            ("Qt", "QScatterSeries"),
            ("React", "scatter"),
            ("Excel", "XY")
        ],
        false,
    ),
    props: &[PropSpec::Bool {
        key: "grid",
        label: "Grid",
        default: true
    }],
    build: |p| Box::new(
        ScatterChart::new()
            .grid(p.bool("grid"))
            .series(ScatterSeries::new(
                "samples",
                [(1.0, 2.0), (2.0, 5.0), (3.0, 3.0), (4.0, 8.0), (5.0, 6.0)]
            )),
    ),
    snippet: |p| format!("ScatterChart::new().grid({}).series(…)", p.bool("grid")),
    poll: |w, out| {
        if let Some(c) = downcast_mut::<ScatterChart>(w) {
            if let Some((series, idx, pt)) = c.take_hovered() {
                out.push(format!("point s{series}[{idx}] ({:.1}, {:.1})", pt.0, pt.1));
            }
        }
    },
});

page!(SankeyPage {
    meta: meta(
        "Sankey",
        "Charts",
        "Flow diagram — node columns joined by links.",
        "Chart",
        &[
            ("D3", "sankey"),
            ("Plotly", "sankey"),
            ("React", "sankey"),
            ("Qt", "custom")
        ],
        false,
    ),
    props: &[],
    build: |_p| Box::new(
        Sankey::new()
            .node_at("Source", Some(0))
            .node_at("Queue", Some(1))
            .node_at("Sink", Some(2))
            .link("Source", "Queue", 40.0)
            .link("Queue", "Sink", 40.0),
    ),
    snippet: |_p| "Sankey::new().node_at(\"Source\", Some(0)).link(\"Source\", \"Queue\", 40.0)"
        .to_string(),
    poll: |w, out| {
        if let Some(s) = downcast_mut::<Sankey>(w) {
            if let Some(i) = s.take_hovered() {
                out.push(format!("node {i}"));
            }
        }
    },
});

page!(TreemapPage {
    meta: meta(
        "Treemap",
        "Charts",
        "Area-proportional nested rectangles.",
        "Chart",
        &[
            ("D3", "treemap"),
            ("Excel", "treemap"),
            ("React", "treemap"),
            ("Qt", "custom")
        ],
        false,
    ),
    props: &[],
    build: |_p| Box::new(
        Treemap::new()
            .item(TreemapItem::new("src", 60.0).color([80, 140, 255, 255]))
            .item(TreemapItem::new("docs", 25.0).color([90, 200, 120, 255]))
            .item(TreemapItem::new("tests", 15.0).color([240, 160, 60, 255])),
    ),
    snippet: |_p| "Treemap::new().item(TreemapItem::new(\"src\", 60.0))".to_string(),
    poll: |w, out| {
        if let Some(t) = downcast_mut::<Treemap>(w) {
            if let Some(i) = t.take_hovered() {
                out.push(format!("cell {i}"));
            }
        }
    },
});

page!(SunburstPage {
    meta: meta(
        "Sunburst",
        "Charts",
        "Radial hierarchy — rings of nested sectors.",
        "Chart",
        &[
            ("D3", "sunburst"),
            ("Plotly", "sunburst"),
            ("React", "sunburst"),
            ("Qt", "custom")
        ],
        false,
    ),
    props: &[],
    build: |_p| Box::new(
        Sunburst::new().node(
            SunburstNode::new("root", 100.0)
                .child(SunburstNode::new("a", 40.0))
                .child(SunburstNode::new("b", 60.0)),
        ),
    ),
    snippet: |_p| "Sunburst::new().node(SunburstNode::new(\"root\", 100.0))".to_string(),
    poll: |w, out| {
        if let Some(s) = downcast_mut::<Sunburst>(w) {
            if let Some(i) = s.take_hovered() {
                out.push(format!("sector {i}"));
            }
        }
    },
});

page!(RadarChartPage {
    meta: meta(
        "RadarChart",
        "Charts",
        "Spider/radar chart — multi-axis series.",
        "Chart",
        &[
            ("Excel", "radar"),
            ("D3", "radar"),
            ("React", "spider chart"),
            ("Qt", "custom")
        ],
        false,
    ),
    props: &[PropSpec::Int {
        key: "rings",
        label: "Rings",
        min: 2,
        max: 8,
        default: 4
    }],
    build: |p| Box::new(
        RadarChart::new()
            .axes(["Speed", "Power", "Range", "Agility", "Armor"])
            .rings(p.i64("rings") as usize)
            .series(RadarSeries::new("Unit A", [0.8, 0.6, 0.9, 0.5, 0.7])),
    ),
    snippet: |p| format!("RadarChart::new().rings({}).series(…)", p.i64("rings")),
});

page!(PolarAreaPage {
    meta: meta(
        "PolarArea",
        "Charts",
        "Polar area — equal-angle sectors with radial values.",
        "Chart",
        &[
            ("Chart.js", "polarArea"),
            ("D3", "polar"),
            ("Nightingale", "coxcomb"),
            ("React", "polar")
        ],
        false,
    ),
    props: &[],
    build: |_p| Box::new(
        PolarArea::new()
            .label("Share")
            .slice("North", 30.0)
            .slice("East", 50.0)
            .slice("South", 20.0),
    ),
    snippet: |_p| "PolarArea::new().slice(\"North\", 30.0)".to_string(),
    poll: |w, out| {
        if let Some(pa) = downcast_mut::<PolarArea>(w) {
            if let Some(i) = pa.take_hovered() {
                out.push(format!("slice {i}"));
            }
        }
    },
});

page!(StreamGraphPage {
    meta: meta(
        "StreamGraph",
        "Charts",
        "Stacked flowing area layers.",
        "Chart",
        &[
            ("D3", "streamgraph"),
            ("NYT", "stream"),
            ("React", "stream graph"),
            ("Qt", "custom")
        ],
        false,
    ),
    props: &[],
    build: |_p| Box::new(
        StreamGraph::new()
            .layer("a", vec![1.0, 3.0, 2.0, 4.0, 3.0])
            .layer("b", vec![2.0, 1.0, 3.0, 2.0, 4.0]),
    ),
    snippet: |_p| "StreamGraph::new().layer(\"a\", vec![…])".to_string(),
    poll: |w, out| {
        if let Some(sg) = downcast_mut::<StreamGraph>(w) {
            if let Some(i) = sg.take_hovered() {
                out.push(format!("layer {i}"));
            }
        }
    },
});

page!(WaterfallPage {
    meta: meta(
        "Waterfall",
        "Charts",
        "Running-total bridge chart — deltas and totals.",
        "Chart",
        &[
            ("Excel", "waterfall"),
            ("Finance", "bridge"),
            ("D3", "waterfall"),
            ("React", "waterfall")
        ],
        false,
    ),
    props: &[],
    build: |_p| Box::new(
        Waterfall::new()
            .label("P&L")
            .total("Revenue", 100.0)
            .delta("COGS", -30.0)
            .delta("OpEx", -20.0)
            .total("Net", 50.0),
    ),
    snippet: |_p| "Waterfall::new().total(\"Revenue\", 100.0).delta(\"COGS\", -30.0)".to_string(),
    poll: |w, out| {
        if let Some(wf) = downcast_mut::<Waterfall>(w) {
            if let Some(i) = wf.take_hovered() {
                out.push(format!("bar {i}"));
            }
        }
    },
});

page!(FunnelChartPage {
    meta: meta(
        "FunnelChart",
        "Charts",
        "Conversion funnel — shrinking stage bars.",
        "Chart",
        &[
            ("Sales", "funnel"),
            ("Chart.js", "funnel"),
            ("React", "funnel"),
            ("Qt", "custom")
        ],
        false,
    ),
    props: &[],
    build: |_p| Box::new(
        FunnelChart::new()
            .label("Pipeline")
            .stage("Visited", 1000.0)
            .stage("Signed up", 400.0)
            .stage("Paid", 120.0),
    ),
    snippet: |_p| "FunnelChart::new().stage(\"Visited\", 1000.0)".to_string(),
    poll: |w, out| {
        if let Some(f) = downcast_mut::<FunnelChart>(w) {
            if let Some(i) = f.take_hovered() {
                out.push(format!("stage {i}"));
            }
        }
    },
});

page!(BurndownPage {
    meta: meta(
        "Burndown",
        "Charts",
        "Sprint burndown — ideal line vs actual remaining.",
        "Chart",
        &[
            ("Jira", "burndown"),
            ("Scrum", "burndown"),
            ("React", "burndown"),
            ("Qt", "custom")
        ],
        false,
    ),
    props: &[PropSpec::Int {
        key: "days",
        label: "Days",
        min: 5,
        max: 30,
        default: 10
    }],
    build: |p| {
        let mut b = Burndown::new(100.0, p.i64("days") as usize).label("Sprint");
        let days = (p.i64("days") as usize / 2).max(1);
        for d in 0..days {
            b.push_day(100.0 - d as f32 * 9.0);
        }
        Box::new(b)
    },
    snippet: |p| format!("Burndown::new(100.0, {})", p.i64("days")),
    poll: |w, out| {
        if let Some(b) = downcast_mut::<Burndown>(w) {
            if let Some(d) = b.take_hovered() {
                out.push(format!("day {d}"));
            }
        }
    },
});

page!(BulletChartPage {
    meta: meta(
        "BulletChart",
        "Charts",
        "Bullet graph — value bar vs target vs ranges.",
        "Chart",
        &[
            ("Few", "bullet graph"),
            ("D3", "bullet"),
            ("React", "bullet"),
            ("Qt", "custom")
        ],
        false,
    ),
    props: &[
        PropSpec::Float {
            key: "value",
            label: "Value",
            min: 0.0,
            max: 100.0,
            step: 5.0,
            default: 65.0
        },
        PropSpec::Float {
            key: "target",
            label: "Target",
            min: 0.0,
            max: 100.0,
            step: 5.0,
            default: 80.0
        },
    ],
    build: |p| Box::new(
        BulletChart::new()
            .label("Revenue")
            .ranges([40.0, 70.0, 100.0])
            .value(p.f64("value") as f32)
            .target(p.f64("target") as f32),
    ),
    snippet: |p| format!(
        "BulletChart::new().value({:?}).target({:?})",
        p.f64("value") as f32,
        p.f64("target") as f32,
    ),
});

page!(BoxPlotPage {
    meta: meta(
        "BoxPlot",
        "Charts",
        "Box-and-whisker — quartiles per series.",
        "Chart",
        &[
            ("Stats", "box plot"),
            ("D3", "box"),
            ("React", "box plot"),
            ("Qt", "custom")
        ],
        false,
    ),
    props: &[],
    build: |_p| Box::new(
        BoxPlot::new()
            .label("Latency")
            .series(BoxSeries::new("API", 10.0, 20.0, 35.0, 50.0, 90.0))
            .series(BoxSeries::new("DB", 5.0, 15.0, 25.0, 40.0, 70.0)),
    ),
    snippet: |_p| "BoxPlot::new().series(BoxSeries::new(\"API\", 10., 20., 35., 50., 90.))"
        .to_string(),
    poll: |w, out| {
        if let Some(b) = downcast_mut::<BoxPlot>(w) {
            if let Some(i) = b.take_hovered() {
                out.push(format!("series {i}"));
            }
        }
    },
});

page!(ViolinPage {
    meta: meta(
        "Violin",
        "Charts",
        "Violin plot — density shapes per series.",
        "Chart",
        &[
            ("Stats", "violin"),
            ("D3", "violin"),
            ("React", "violin"),
            ("Qt", "custom")
        ],
        false,
    ),
    props: &[],
    build: |_p| Box::new(
        Violin::new()
            .label("Distribution")
            .series("A", vec![0.1, 0.4, 0.9, 0.6, 0.3, 0.1])
            .series("B", vec![0.3, 0.5, 0.5, 0.8, 0.4, 0.2]),
    ),
    snippet: |_p| "Violin::new().series(\"A\", vec![…])".to_string(),
    poll: |w, out| {
        if let Some(v) = downcast_mut::<Violin>(w) {
            if let Some(i) = v.take_hovered() {
                out.push(format!("violin {i}"));
            }
        }
    },
});

page!(SparklinePage {
    meta: meta(
        "Sparkline",
        "Charts",
        "Inline micro-chart — no axes.",
        "Chart",
        &[
            ("Excel", "sparkline"),
            ("Tufte", "sparkline"),
            ("React", "sparkline"),
            ("Qt", "custom")
        ],
        false,
    ),
    props: &[],
    build: |_p| Box::new(Sparkline::new([3.0, 5.0, 2.0, 8.0, 6.0, 9.0, 4.0]).dot(true)),
    snippet: |_p| "Sparkline::new([3.0, 5.0, 2.0, 8.0, 6.0]).dot(true)".to_string(),
});

page!(StripChartPage {
    meta: meta(
        "StripChart",
        "Charts",
        "Scrolling time-series strip.",
        "Chart",
        &[
            ("SCADA", "strip chart"),
            ("Qt", "rolling chart"),
            ("LabVIEW", "strip"),
            ("React", "stream")
        ],
        false,
    ),
    props: &[PropSpec::Int {
        key: "capacity",
        label: "Capacity",
        min: 20,
        max: 500,
        default: 120
    }],
    build: |p| {
        let mut s = StripChart::new().capacity(p.i64("capacity") as usize);
        for i in 0..60 {
            s.push((i as f32 * 0.3).sin() * 0.5 + 0.5);
        }
        Box::new(s)
    },
    snippet: |p| format!("StripChart::new().capacity({})", p.i64("capacity")),
});

page!(GanttPage {
    meta: meta(
        "Gantt",
        "Charts",
        "Project timeline — task bars with progress.",
        "Chart",
        &[
            ("MS Project", "gantt"),
            ("D3", "gantt"),
            ("React", "gantt"),
            ("Qt", "custom")
        ],
        false,
    ),
    props: &[],
    build: |_p| {
        Box::new(
            Gantt::new()
                .task("Design", 0.0, 4.0)
                .task("Build", 3.0, 6.0)
                .task("Ship", 8.0, 2.0),
        )
    },
    snippet: |_p| "Gantt::new().task(\"Design\", 0.0, 4.0).task(\"Build\", 3.0, 6.0)".to_string(),
    poll: |w, out| {
        if let Some(g) = downcast_mut::<Gantt>(w) {
            if let Some(i) = g.take_hovered() {
                out.push(format!("task {i}"));
            }
        }
    },
});

page!(WaveformPage {
    meta: meta(
        "Waveform",
        "Charts",
        "Audio waveform peaks with seek position.",
        "Chart",
        &[
            ("Audacity", "waveform"),
            ("DAW", "waveform"),
            ("Qt", "custom"),
            ("React", "waveform")
        ],
        false,
    ),
    props: &[PropSpec::Float {
        key: "position",
        label: "Position",
        min: 0.0,
        max: 1.0,
        step: 0.05,
        default: 0.3,
    }],
    build: |p| {
        let mut w = Waveform::new().peaks((0..80).map(|i| ((i * 7) % 20) as f32 / 20.0 + 0.1));
        w.set_position(p.f64("position") as f32);
        Box::new(w)
    },
    snippet: |p| format!(
        "Waveform::new().peaks(vec![…]).set_position({:?})",
        p.f64("position") as f32
    ),
    poll: |w, out| {
        if let Some(wf) = downcast_mut::<Waveform>(w) {
            if let Some(pos) = wf.take_seek() {
                out.push(format!("seek → {pos:.2}"));
            }
        }
    },
});

page!(EqualizerPage {
    meta: meta(
        "Equalizer",
        "Charts",
        "Multi-band EQ faders — drag to boost/cut.",
        "Chart",
        &[
            ("Audio", "EQ"),
            ("Winamp", "equalizer"),
            ("Qt", "custom"),
            ("React", "eq")
        ],
        false,
    ),
    props: &[PropSpec::Int {
        key: "bands",
        label: "Bands",
        min: 3,
        max: 16,
        default: 8
    }],
    build: |p| {
        let mut eq = Equalizer::new();
        for i in 0..p.i64("bands") {
            let gain = ((i * 5) % 7) as f32 - 3.0;
            eq.set_gain(i as usize, gain);
        }
        Box::new(eq)
    },
    snippet: |p| format!("Equalizer::new() /* {} bands */", p.i64("bands")),
    poll: |w, out| {
        if let Some(eq) = downcast_mut::<Equalizer>(w) {
            if eq.take_changed() {
                out.push("levels changed".to_string());
            }
        }
    },
});

page!(SpectrumPage {
    meta: meta(
        "Spectrum",
        "Charts",
        "FFT-style spectrum bars with peak hold.",
        "Chart",
        &[
            ("Audio", "spectrum analyzer"),
            ("Qt", "custom"),
            ("Winamp", "spectrum"),
            ("React", "spectrum")
        ],
        false,
    ),
    props: &[PropSpec::Bool {
        key: "peak",
        label: "Peak hold",
        default: true
    }],
    build: |p| Box::new(
        Spectrum::new()
            .peak_hold(p.bool("peak"))
            .bands(vec![0.2, 0.5, 0.8, 0.6, 0.9, 0.4, 0.3, 0.7]),
    ),
    snippet: |p| format!("Spectrum::new().peak_hold({})", p.bool("peak")),
    poll: |w, out| {
        if let Some(s) = downcast_mut::<Spectrum>(w) {
            if let Some(i) = s.take_pressed() {
                out.push(format!("band {i}"));
            }
        }
    },
});

/// All Charts pages, in rail order.
pub(super) fn pages() -> Vec<Box<dyn Page>> {
    vec![
        Box::new(BarChartPage),
        Box::new(LineChartPage),
        Box::new(PieChartPage),
        Box::new(CandlestickPage),
        Box::new(HistogramPage),
        Box::new(HeatMapPage),
        Box::new(ScatterChartPage),
        Box::new(SankeyPage),
        Box::new(TreemapPage),
        Box::new(SunburstPage),
        Box::new(RadarChartPage),
        Box::new(PolarAreaPage),
        Box::new(StreamGraphPage),
        Box::new(WaterfallPage),
        Box::new(FunnelChartPage),
        Box::new(BurndownPage),
        Box::new(BulletChartPage),
        Box::new(BoxPlotPage),
        Box::new(ViolinPage),
        Box::new(SparklinePage),
        Box::new(StripChartPage),
        Box::new(GanttPage),
        Box::new(WaveformPage),
        Box::new(EqualizerPage),
        Box::new(SpectrumPage),
    ]
}
