//! Visualization family — gauges, meters, diagrams, and displays.

use martensite::widgets::analog_clock::AnalogClock;
use martensite::widgets::barcode::Barcode;
use martensite::widgets::battery::Battery;
use martensite::widgets::compass::Compass;
use martensite::widgets::curve_editor::CurveEditor;
use martensite::widgets::digital_clock::DigitalClock;
use martensite::widgets::fishbone::{Bone, Fishbone};
use martensite::widgets::gauge::Gauge;
use martensite::widgets::gradient_editor::{GradientEditor, GradientStop};
use martensite::widgets::graph_view::GraphView;
use martensite::widgets::lcd_number::LcdNumber;
use martensite::widgets::led_matrix::LedMatrix;
use martensite::widgets::legend::Legend;
use martensite::widgets::mind_map::MindMap;
use martensite::widgets::odometer::Odometer;
use martensite::widgets::org_chart::{OrgChart, OrgNode};
use martensite::widgets::perf_overlay::PerfOverlay;
use martensite::widgets::qr_code::QrCode;
use martensite::widgets::quadrant::{Quadrant, QuadrantItem};
use martensite::widgets::rubber_band::RubberBand;
use martensite::widgets::ruler::Ruler;
use martensite::widgets::signal_strength::SignalStrength;
use martensite::widgets::split_flap::SplitFlap;
use martensite::widgets::step_sequencer::StepSequencer;
use martensite::widgets::thermometer::Thermometer;
use martensite::widgets::timeline::{Timeline, TimelineDot, TimelineItem};
use martensite::widgets::unit_converter::{UnitCategory, UnitConverter};
use martensite::widgets::venn::Venn;
use martensite::widgets::vu_meter::VuMeter;
use martensite::widgets::weather::{Weather, WeatherCondition};

use crate::page::{Page, PropSpec};
use crate::pages::{downcast_mut, meta, page};

page!(GaugePage {
    meta: meta(
        "Gauge",
        "Visualization",
        "Needle gauge with warn/crit zones.",
        "ProgressBar",
        &[
            ("SCADA", "gauge"),
            ("Qt", "QDial"),
            ("Cars", "dash gauge"),
            ("React", "gauge")
        ],
        false,
    ),
    props: &[PropSpec::Float {
        key: "value",
        label: "Value",
        min: 0.0,
        max: 100.0,
        step: 1.0,
        default: 62.0
    }],
    build: |p| Box::new(
        Gauge::new()
            .label("Load")
            .range(0.0, 100.0)
            .value(p.f64("value"))
            .zones(0.7, 0.9)
            .ticks(true),
    ),
    snippet: |p| format!("Gauge::new().range(0.0, 100.0).value({:?})", p.f64("value")),
});

page!(ThermometerPage {
    meta: meta(
        "Thermometer",
        "Visualization",
        "Linear thermometer with warning/critical marks.",
        "ProgressBar",
        &[
            ("Lab", "thermometer"),
            ("Qt", "custom"),
            ("React", "thermometer"),
            ("SCADA", "temp")
        ],
        false,
    ),
    props: &[PropSpec::Float {
        key: "value",
        label: "Value",
        min: 0.0,
        max: 100.0,
        step: 1.0,
        default: 37.0
    }],
    build: |p| Box::new(
        Thermometer::new()
            .range(0.0, 100.0)
            .value(p.f64("value") as f32)
            .warning(0.6)
            .critical(0.85),
    ),
    snippet: |p| format!("Thermometer::new().value({:?})", p.f64("value") as f32),
});

page!(LcdNumberPage {
    meta: meta(
        "LcdNumber",
        "Visualization",
        "Seven-segment LCD digits.",
        "Text",
        &[
            ("Qt", "QLCDNumber"),
            ("Calc", "LCD"),
            ("React", "lcd digits"),
            ("Embedded", "7-seg")
        ],
        false,
    ),
    props: &[
        PropSpec::Float {
            key: "value",
            label: "Value",
            min: 0.0,
            max: 9999.0,
            step: 1.0,
            default: 1234.5
        },
        PropSpec::Int {
            key: "digits",
            label: "Digits",
            min: 2,
            max: 10,
            default: 6
        },
    ],
    build: |p| Box::new(
        LcdNumber::new()
            .value(p.f64("value"))
            .digits(p.i64("digits") as usize)
            .decimals(1),
    ),
    snippet: |p| format!(
        "LcdNumber::new().value({:?}).digits({})",
        p.f64("value"),
        p.i64("digits")
    ),
});

page!(LedMatrixPage {
    meta: meta(
        "LedMatrix",
        "Visualization",
        "Dot-matrix LED grid.",
        "Grid",
        &[
            ("Arduino", "LED matrix"),
            ("Times Sq", "matrix"),
            ("Qt", "custom"),
            ("React", "dot grid")
        ],
        false,
    ),
    props: &[PropSpec::Int {
        key: "size",
        label: "Size",
        min: 4,
        max: 16,
        default: 8
    }],
    build: |p| {
        let n = p.i64("size") as usize;
        let mut m = LedMatrix::new(n, n).on_color([90, 220, 120, 255]);
        for i in 0..n {
            m.set(i, i, true);
            m.set(i, n - 1 - i, true);
        }
        Box::new(m)
    },
    snippet: |p| format!("LedMatrix::new({}, {})", p.i64("size"), p.i64("size")),
});

page!(SplitFlapPage {
    meta: meta(
        "SplitFlap",
        "Visualization",
        "Airport split-flap display — animated flips.",
        "Text",
        &[
            ("Airport", "split-flap"),
            ("Solari", "departure board"),
            ("React", "split flap"),
            ("Qt", "custom")
        ],
        false,
    ),
    props: &[PropSpec::Text {
        key: "text",
        label: "Text",
        default: "PARIS"
    }],
    build: |p| Box::new(
        SplitFlap::new()
            .cells(p.str("text").len().max(1))
            .text(p.str("text"))
    ),
    snippet: |p| format!("SplitFlap::new().cells(5).text({:?})", p.str("text")),
});

page!(OdometerPage {
    meta: meta(
        "Odometer",
        "Visualization",
        "Rolling-digit counter — car odometer.",
        "Text",
        &[
            ("Car", "odometer"),
            ("Web", "odometer.js"),
            ("Qt", "custom"),
            ("React", "odometer")
        ],
        false,
    ),
    props: &[PropSpec::Int {
        key: "value",
        label: "Value",
        min: 0,
        max: 999999,
        default: 42817
    }],
    build: |p| Box::new(
        Odometer::new()
            .digits(6)
            .value(p.i64("value") as u64)
            .speed(4.0),
    ),
    snippet: |p| format!("Odometer::new().value({})", p.i64("value")),
});

page!(DigitalClockPage {
    meta: meta(
        "DigitalClock",
        "Visualization",
        "Digital HH:MM:SS readout — blinking colon.",
        "Text",
        &[
            ("iOS", "clock"),
            ("Qt", "QLCD clock"),
            ("React", "digital clock"),
            ("Desktop", "clock")
        ],
        false,
    ),
    props: &[
        PropSpec::Bool {
            key: "seconds",
            label: "Seconds",
            default: true
        },
        PropSpec::Bool {
            key: "blink",
            label: "Blink colon",
            default: true
        },
        PropSpec::Bool {
            key: "hour12",
            label: "12h",
            default: false
        },
    ],
    build: |p| Box::new(
        DigitalClock::new()
            .show_seconds(p.bool("seconds"))
            .blink(p.bool("blink"))
            .hour12(p.bool("hour12"))
            .running(true),
    ),
    snippet: |p| format!(
        "DigitalClock::new().show_seconds({}).running(true)",
        p.bool("seconds"),
    ),
});

page!(AnalogClockPage {
    meta: meta(
        "AnalogClock",
        "Visualization",
        "Analog clock face — hour/minute/second hands.",
        "Image",
        &[
            ("Qt", "clock"),
            ("GTK", "custom"),
            ("React", "analog clock"),
            ("macOS", "clock")
        ],
        false,
    ),
    props: &[PropSpec::Bool {
        key: "seconds",
        label: "Second hand",
        default: true
    }],
    build: |p| Box::new(
        AnalogClock::new()
            .time(10, 9, 30)
            .show_seconds(p.bool("seconds")),
    ),
    snippet: |p| format!(
        "AnalogClock::new().time(10, 9, 30).show_seconds({})",
        p.bool("seconds")
    ),
});

page!(CompassPage {
    meta: meta(
        "Compass",
        "Visualization",
        "Compass dial with cardinal readout.",
        "Image",
        &[
            ("Nav", "compass"),
            ("Qt", "custom"),
            ("React", "compass"),
            ("iOS", "compass")
        ],
        false,
    ),
    props: &[PropSpec::Float {
        key: "heading",
        label: "Heading",
        min: 0.0,
        max: 360.0,
        step: 5.0,
        default: 45.0
    }],
    build: |p| Box::new(Compass::new().heading(p.f64("heading") as f32).label("HDG")),
    snippet: |p| format!("Compass::new().heading({:?})", p.f64("heading") as f32),
});

page!(WeatherPage {
    meta: meta(
        "Weather",
        "Visualization",
        "Weather card — condition, temperature, hi/lo.",
        "Group",
        &[
            ("iOS", "weather widget"),
            ("GTK", "weather"),
            ("React", "weather card"),
            ("Qt", "custom")
        ],
        false,
    ),
    props: &[
        PropSpec::Text {
            key: "location",
            label: "Location",
            default: "Lisbon"
        },
        PropSpec::Choice {
            key: "condition",
            label: "Condition",
            options: &["Sunny", "Cloudy", "Rain", "Snow"],
            default: 0,
        },
    ],
    build: |p| {
        let cond = match p.choice("condition") {
            1 => WeatherCondition::Cloudy,
            2 => WeatherCondition::Rain,
            3 => WeatherCondition::Snow,
            _ => WeatherCondition::Clear,
        };
        Box::new(Weather::new().location(p.str("location")).condition(cond))
    },
    snippet: |p| format!("Weather::new().location({:?})", p.str("location")),
});

page!(QrCodePage {
    meta: meta(
        "QrCode",
        "Visualization",
        "QR code matrix from a bit grid.",
        "Image",
        &[
            ("Mobile", "QR"),
            ("Qt", "custom"),
            ("React", "qr code"),
            ("ZXing", "QR")
        ],
        false,
    ),
    props: &[],
    build: |_p| {
        // 21×21 demo matrix — finder squares in the corners.
        let mut m = vec![vec![false; 21]; 21];
        for i in 0..7 {
            for j in 0..7 {
                let edge = i == 0 || i == 6 || j == 0 || j == 6;
                let core = (2..5).contains(&i) && (2..5).contains(&j);
                m[i][j] = edge || core;
                m[i][14 + j] = edge || core;
                m[14 + i][j] = edge || core;
            }
        }
        for (i, row) in m.iter_mut().enumerate() {
            row[10] = i % 3 == 0;
        }
        for (i, cell) in m[10].iter_mut().enumerate() {
            *cell = i % 2 == 0;
        }
        Box::new(QrCode::from_matrix(m).label("QR"))
    },
    snippet: |_p| "QrCode::from_matrix(modules)".to_string(),
});

page!(BarcodePage {
    meta: meta(
        "Barcode",
        "Visualization",
        "1-D barcode from digits/text.",
        "Image",
        &[
            ("POS", "barcode"),
            ("Qt", "custom"),
            ("React", "barcode"),
            ("EAN", "barcode")
        ],
        false,
    ),
    props: &[PropSpec::Text {
        key: "text",
        label: "Content",
        default: "5901234123457"
    }],
    build: |p| Box::new(Barcode::new().text(p.str("text")).label("EAN")),
    snippet: |p| format!("Barcode::new().text({:?})", p.str("text")),
});

page!(GraphViewPage {
    meta: meta(
        "GraphView",
        "Visualization",
        "Node-link force-directed graph.",
        "Canvas",
        &[
            ("D3", "force graph"),
            ("Gephi", "graph"),
            ("Qt", "custom"),
            ("React", "graph")
        ],
        false,
    ),
    props: &[],
    build: |_p| Box::new(
        GraphView::new()
            .node("core")
            .node("a")
            .node("b")
            .node("c")
            .edge(0, 1)
            .edge(0, 2)
            .edge(1, 3),
    ),
    snippet: |_p| "GraphView::new().node(\"core\").edge(0, 1)".to_string(),
    poll: |w, out| {
        if let Some(g) = downcast_mut::<GraphView>(w) {
            if let Some(i) = g.take_hovered() {
                out.push(format!("node {i}"));
            }
            if let Some(m) = g.take_moved() {
                out.push(format!("moved {m:?}"));
            }
        }
    },
});

page!(MindMapPage {
    meta: meta(
        "MindMap",
        "Visualization",
        "Radial mind map — root + branching nodes.",
        "Canvas",
        &[
            ("XMind", "mind map"),
            ("Docs", "outline"),
            ("React", "mind map"),
            ("Qt", "custom")
        ],
        false,
    ),
    props: &[],
    build: |_p| Box::new(
        MindMap::new()
            .root("Martensite")
            .child("Martensite", "Widgets")
            .child("Martensite", "Layout")
            .child("Widgets", "Catalog"),
    ),
    snippet: |_p| "MindMap::new().root(\"Martensite\").child(\"Martensite\", \"Widgets\")"
        .to_string(),
    poll: |w, out| {
        if let Some(mm) = downcast_mut::<MindMap>(w) {
            if let Some(i) = mm.take_hovered() {
                out.push(format!("node {i}"));
            }
        }
    },
});

page!(OrgChartPage {
    meta: meta(
        "OrgChart",
        "Visualization",
        "Org chart — hierarchical boxes.",
        "Tree",
        &[
            ("HR", "org chart"),
            ("Visio", "org"),
            ("React", "org chart"),
            ("Qt", "custom")
        ],
        false,
    ),
    props: &[],
    build: |_p| Box::new(OrgChart::new(
        OrgNode::new("Ada", "CEO")
            .child(OrgNode::new("Grace", "CTO").child(OrgNode::new("Linus", "Eng")))
            .child(OrgNode::new("Alan", "Chief Scientist")),
    )),
    snippet: |_p| "OrgChart::new(OrgNode::new(\"Ada\", \"CEO\"))".to_string(),
    poll: |w, out| {
        if let Some(oc) = downcast_mut::<OrgChart>(w) {
            if let Some(i) = oc.take_hovered() {
                out.push(format!("node {i}"));
            }
        }
    },
});

page!(FishbonePage {
    meta: meta(
        "Fishbone",
        "Visualization",
        "Ishikawa cause-and-effect diagram.",
        "Canvas",
        &[
            ("QA", "fishbone"),
            ("Lean", "ishikawa"),
            ("React", "fishbone"),
            ("Qt", "custom")
        ],
        false,
    ),
    props: &[],
    build: |_p| Box::new(
        Fishbone::new("Defect")
            .bone(Bone::new("Process").cause("No review"))
            .bone(Bone::new("Tools").cause("Slow CI")),
    ),
    snippet: |_p| "Fishbone::new(\"Defect\").bone(Bone::new(\"Process\"))".to_string(),
    poll: |w, out| {
        if let Some(f) = downcast_mut::<Fishbone>(w) {
            if let Some(i) = f.take_hovered() {
                out.push(format!("bone {i}"));
            }
        }
    },
});

page!(VennPage {
    meta: meta(
        "Venn",
        "Visualization",
        "Venn diagram — overlapping set circles.",
        "Image",
        &[
            ("Math", "venn"),
            ("D3", "venn"),
            ("React", "venn"),
            ("Qt", "custom")
        ],
        false,
    ),
    props: &[],
    build: |_p| Box::new(Venn::new().label("Sets").set("Rust").set("C++").set("Go")),
    snippet: |_p| "Venn::new().set(\"Rust\").set(\"C++\")".to_string(),
    poll: |w, out| {
        if let Some(v) = downcast_mut::<Venn>(w) {
            if let Some(i) = v.take_hovered() {
                out.push(format!("set {i}"));
            }
        }
    },
});

page!(QuadrantPage {
    meta: meta(
        "Quadrant",
        "Visualization",
        "2×2 quadrant matrix — effort/impact style.",
        "Canvas",
        &[
            ("PM", "2×2 matrix"),
            ("BCG", "matrix"),
            ("React", "quadrant"),
            ("Qt", "custom")
        ],
        false,
    ),
    props: &[],
    build: |_p| Box::new(
        Quadrant::new("Effort", "Impact")
            .regions(["Do", "Plan", "Drop", "Delegate"])
            .item(QuadrantItem::new("Catalog", 0.7, 0.9)),
    ),
    snippet: |_p| {
        "Quadrant::new(\"Effort\", \"Impact\").item(QuadrantItem::new(\"Catalog\", 0.7, 0.9))"
            .to_string()
    },
    poll: |w, out| {
        if let Some(q) = downcast_mut::<Quadrant>(w) {
            if let Some(i) = q.take_selected() {
                out.push(format!("item {i}"));
            }
        }
    },
});

page!(TimelinePage {
    meta: meta(
        "Timeline",
        "Visualization",
        "Vertical timeline — labeled event dots.",
        "List",
        &[
            ("FB", "timeline"),
            ("Ant", "Timeline"),
            ("React", "timeline"),
            ("Qt", "custom")
        ],
        false,
    ),
    props: &[],
    build: |_p| Box::new(
        Timeline::new()
            .item(
                TimelineItem::new("Created")
                    .subtitle("2025-01")
                    .dot(TimelineDot::Accent)
            )
            .item(
                TimelineItem::new("Released")
                    .subtitle("2025-06")
                    .dot(TimelineDot::Success)
            ),
    ),
    snippet: |_p| "Timeline::new().item(TimelineItem::new(\"Created\"))".to_string(),
});

page!(StepSequencerPage {
    meta: meta(
        "StepSequencer",
        "Visualization",
        "Drum-machine step grid — cells on/off per step.",
        "Grid",
        &[
            ("Roland", "TR-808"),
            ("DAW", "step sequencer"),
            ("React", "step grid"),
            ("Qt", "custom")
        ],
        false,
    ),
    props: &[PropSpec::Int {
        key: "cols",
        label: "Steps",
        min: 4,
        max: 32,
        default: 16
    }],
    build: |p| {
        let mut s = StepSequencer::new(4, p.i64("cols") as usize);
        s.set_cell(0, 0, true);
        s.set_cell(1, 4, true);
        s.set_cell(2, 8, true);
        Box::new(s)
    },
    snippet: |p| format!("StepSequencer::new(4, {})", p.i64("cols")),
    poll: |w, out| {
        if let Some(s) = downcast_mut::<StepSequencer>(w) {
            if let Some((r, c, on)) = s.take_changed() {
                out.push(format!(
                    "cell ({r},{c}) → {}",
                    if on { "on" } else { "off" }
                ));
            }
        }
    },
});

page!(VuMeterPage {
    meta: meta(
        "VuMeter",
        "Visualization",
        "Audio level meters with peak hold.",
        "LevelBar",
        &[
            ("Audio", "VU meter"),
            ("Mixer", "level"),
            ("Qt", "custom"),
            ("React", "vu")
        ],
        false,
    ),
    props: &[PropSpec::Int {
        key: "channels",
        label: "Channels",
        min: 1,
        max: 8,
        default: 2
    }],
    build: |p| {
        let mut v = VuMeter::new()
            .channels(p.i64("channels") as usize)
            .peak_hold(1.0);
        v.push([0.7, 0.5, 0.3, 0.8, 0.6, 0.4, 0.9, 0.2]);
        Box::new(v)
    },
    snippet: |p| format!("VuMeter::new().channels({})", p.i64("channels")),
});

page!(SignalStrengthPage {
    meta: meta(
        "SignalStrength",
        "Visualization",
        "Signal bars — Wi-Fi/cell strength indicator.",
        "LevelBar",
        &[
            ("iOS", "signal bars"),
            ("Android", "signal"),
            ("Qt", "custom"),
            ("React", "signal")
        ],
        false,
    ),
    props: &[
        PropSpec::Int {
            key: "level",
            label: "Level (0-4)",
            min: 0,
            max: 4,
            default: 3
        },
        PropSpec::Bool {
            key: "offline",
            label: "Offline",
            default: false
        },
    ],
    build: |p| Box::new(
        SignalStrength::new()
            .level(p.i64("level") as u8)
            .offline(p.bool("offline")),
    ),
    snippet: |p| format!(
        "SignalStrength::new().level({}).offline({})",
        p.i64("level"),
        p.bool("offline"),
    ),
});

page!(BatteryPage {
    meta: meta(
        "Battery",
        "Visualization",
        "Battery level indicator — charge + charging bolt.",
        "LevelBar",
        &[
            ("iOS", "battery"),
            ("GNOME", "battery icon"),
            ("Qt", "custom"),
            ("React", "battery")
        ],
        false,
    ),
    props: &[
        PropSpec::Float {
            key: "level",
            label: "Level",
            min: 0.0,
            max: 1.0,
            step: 0.05,
            default: 0.65
        },
        PropSpec::Bool {
            key: "charging",
            label: "Charging",
            default: false
        },
    ],
    build: |p| Box::new(
        Battery::new()
            .level(p.f64("level") as f32)
            .charging(p.bool("charging")),
    ),
    snippet: |p| format!(
        "Battery::new().level({:?}).charging({})",
        p.f64("level") as f32,
        p.bool("charging"),
    ),
});

page!(RulerPage {
    meta: meta(
        "Ruler",
        "Visualization",
        "Measurement ruler with cursor marker.",
        "Image",
        &[
            ("Design", "ruler"),
            ("Qt", "custom"),
            ("Figma", "ruler"),
            ("React", "ruler")
        ],
        false,
    ),
    props: &[PropSpec::Float {
        key: "pos",
        label: "Cursor",
        min: 0.0,
        max: 1.0,
        step: 0.05,
        default: 0.4
    }],
    build: |p| {
        let mut r = Ruler::new().range(0.0, 100.0);
        r.set_position(p.f64("pos") as f32 * 100.0);
        Box::new(r)
    },
    snippet: |p| format!(
        "Ruler::new().range(0.0, 100.0).set_position({:?})",
        p.f64("pos") as f32 * 100.0
    ),
    poll: |w, out| {
        if let Some(r) = downcast_mut::<Ruler>(w) {
            if let Some(pos) = r.take_picked() {
                out.push(format!("pos → {pos:.1}"));
            }
        }
    },
});

page!(PerfOverlayPage {
    meta: meta(
        "PerfOverlay",
        "Visualization",
        "Frame-time overlay — FPS, worst, average.",
        "Text",
        &[
            ("DevTools", "perf HUD"),
            ("Games", "fps overlay"),
            ("Qt", "custom"),
            ("React", "stats.js")
        ],
        false,
    ),
    props: &[],
    build: |_p| {
        let mut po = PerfOverlay::new();
        for ms in [16.6, 16.9, 17.2, 16.4, 18.0] {
            po.push_frame(ms);
        }
        Box::new(po)
    },
    snippet: |_p| "PerfOverlay::new() /* push_frame(ms) */".to_string(),
});

page!(GradientEditorPage {
    meta: meta(
        "GradientEditor",
        "Visualization",
        "Gradient stop editor — color stops on a ramp.",
        "Slider",
        &[
            ("Design", "gradient editor"),
            ("Qt", "gradient"),
            ("Figma", "gradient"),
            ("React", "gradient")
        ],
        false,
    ),
    props: &[],
    build: |_p| {
        let mut g = GradientEditor::new();
        g.add_stop(GradientStop::new(0.0, [80, 140, 255, 255]));
        g.add_stop(GradientStop::new(1.0, [240, 90, 160, 255]));
        Box::new(g)
    },
    snippet: |_p| "GradientEditor::new().add_stop(GradientStop::new(0.0, [80, 140, 255, 255]))"
        .to_string(),
    poll: |w, out| {
        if let Some(g) = downcast_mut::<GradientEditor>(w) {
            if let Some(i) = g.take_selected() {
                out.push(format!("stop {i}"));
            }
            if g.take_changed() {
                out.push("changed".to_string());
            }
        }
    },
});

page!(CurveEditorPage {
    meta: meta(
        "CurveEditor",
        "Visualization",
        "Cubic-bezier easing curve editor.",
        "Canvas",
        &[
            ("CSS", "cubic-bezier"),
            ("Design", "easing"),
            ("Qt", "custom"),
            ("React", "curve")
        ],
        false,
    ),
    props: &[],
    build: |_p| Box::new(CurveEditor::new().handles((0.25, 0.1), (0.25, 1.0))),
    snippet: |_p| "CurveEditor::new().handles((0.25, 0.1), (0.25, 1.0))".to_string(),
    poll: |w, out| {
        if let Some(c) = downcast_mut::<CurveEditor>(w) {
            if c.take_changed() {
                out.push(format!("curve → {:?}", c.css()));
            }
        }
    },
});

page!(UnitConverterPage {
    meta: meta(
        "UnitConverter",
        "Visualization",
        "Unit conversion — category + from/to units.",
        "Group",
        &[
            ("Calc", "converter"),
            ("GNOME", "units"),
            ("React", "converter"),
            ("Qt", "custom")
        ],
        false,
    ),
    props: &[],
    build: |_p| {
        let mut u = UnitConverter::new()
            .in_category(UnitCategory::Length)
            .with_value(1.0);
        u.set_units("meters", "feet");
        Box::new(u)
    },
    snippet: |_p| "UnitConverter::new().category(\"Length\").with_value(1.0)".to_string(),
    poll: |w, out| {
        if let Some(u) = downcast_mut::<UnitConverter>(w) {
            if u.take_changed() {
                out.push(format!("value → {}", u.value()));
            }
        }
    },
});

page!(LegendPage {
    meta: meta(
        "Legend",
        "Visualization",
        "Chart legend — labeled color chips, dimmable entries.",
        "List",
        &[
            ("Excel", "legend"),
            ("D3", "legend"),
            ("React", "legend"),
            ("Qt", "custom")
        ],
        false,
    ),
    props: &[],
    build: |_p| Box::new(
        Legend::new()
            .entry("CPU", [80, 140, 255, 255])
            .entry("Memory", [240, 160, 60, 255]),
    ),
    snippet: |_p| "Legend::new().entry(LegendEntry::new(\"CPU\", [80, 140, 255, 255]))".to_string(),
    poll: |w, out| {
        if let Some(l) = downcast_mut::<Legend>(w) {
            if let Some(i) = l.take_toggled() {
                out.push(format!("toggled {i}"));
            }
        }
    },
});

page!(RubberBandPage {
    meta: meta(
        "RubberBand",
        "Visualization",
        "Drag-select rubber-band rectangle.",
        "Canvas",
        &[
            ("Desktop", "rubber band"),
            ("Qt", "QRubberBand"),
            ("React", "marquee select"),
            ("macOS", "drag select")
        ],
        false,
    ),
    props: &[],
    build: |_p| Box::new(RubberBand::new()),
    snippet: |_p| "RubberBand::new()".to_string(),
    poll: |w, out| {
        if let Some(rb) = downcast_mut::<RubberBand>(w) {
            if let Some(rect) = rb.take_selection() {
                out.push(format!("select → {rect:?}"));
            }
        }
    },
});

/// All Visualization pages, in rail order.
pub(super) fn pages() -> Vec<Box<dyn Page>> {
    vec![
        Box::new(GaugePage),
        Box::new(ThermometerPage),
        Box::new(LcdNumberPage),
        Box::new(LedMatrixPage),
        Box::new(SplitFlapPage),
        Box::new(OdometerPage),
        Box::new(DigitalClockPage),
        Box::new(AnalogClockPage),
        Box::new(CompassPage),
        Box::new(WeatherPage),
        Box::new(QrCodePage),
        Box::new(BarcodePage),
        Box::new(GraphViewPage),
        Box::new(MindMapPage),
        Box::new(OrgChartPage),
        Box::new(FishbonePage),
        Box::new(VennPage),
        Box::new(QuadrantPage),
        Box::new(TimelinePage),
        Box::new(StepSequencerPage),
        Box::new(VuMeterPage),
        Box::new(SignalStrengthPage),
        Box::new(BatteryPage),
        Box::new(RulerPage),
        Box::new(PerfOverlayPage),
        Box::new(GradientEditorPage),
        Box::new(CurveEditorPage),
        Box::new(UnitConverterPage),
        Box::new(LegendPage),
        Box::new(RubberBandPage),
    ]
}
