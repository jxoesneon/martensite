//! Telemetry zone — the live-process panel's functional pages.
//!
//! Seven domain-named tabs per the Design Council's bind-or-cut
//! verdict (docket `20260921`): every mounted widget reads
//! [`PlantModel`] state (`push`) or publishes operator interaction
//! back into it (`pull`). Series charts share the `cpu_hist`/
//! `mem_hist` rings — no per-widget buffers.
//!
//! ## CUT — no honest binding in the model
//!
//! - `Weather` — no atmospheric dataset in `PlantModel`.
//! - `Calendar` — the schedule runs on relative day offsets, not
//!   calendar dates; `WeekView`/`Gantt` carry the real plan.
//! - `Burndown` — work orders have no daily planned-vs-actual series.
//! - `Sunburst` — the asset hierarchy is already rendered honestly by
//!   `OrgChart` (crew) and `Treemap` (cells by OEE).
//! - `Quadrant`, `Fishbone`, `MindMap`, `Venn`, `WordCloud` — no
//!   matching model dataset.
//! - `TickerTape` — the KPI strip (`Statistic` row) covers the same
//!   numbers with real values.
//!
//! ## CUT — not in the toolkit (nearest bound widget mounted)
//!
//! `AreaChart` → `Sparkline(Area)`; `StepChart`/`RealtimeChart` →
//! `StripChart`; `SparkBar` → `Sparkline(Bars)`; `ProgressRing`/
//! `ProgressCircle` → `ActivityRing`/`CountdownRing`; `LedNumber`/
//! `SegmentDisplay` → `LcdNumber`/`Odometer`; `Speedometer` → `Gauge`;
//! `KpiCard`/`StatDisplay` → `Statistic`; `ChordDiagram` → `Sankey` +
//! `GraphView`; `BubbleChart` → `ScatterChart`; `Milestone` →
//! `Timeline` dots; `LevelIndicator`/`Meter` → `LevelBar`/
//! `Thermometer`; `Timer` → `Countdown`/`CountdownRing`.
//! Council-cut widgets (`ChessBoard`, `ChessClock`, `Confetti`,
//! `PricingTable`, `ScratchCard`) stay cut.

use std::collections::{hash_map::DefaultHasher, HashSet};
use std::hash::Hasher;
use std::time::{Duration, Instant};

use martensite::widgets::activity_ring::ActivityRing;
use martensite::widgets::alarm_panel::{Alarm as PanelAlarm, AlarmPanel};
use martensite::widgets::analog_clock::AnalogClock;
use martensite::widgets::attendee_list::{Attendee, AttendeeList};
use martensite::widgets::avatar::Avatar;
use martensite::widgets::avatar_group::AvatarGroup;
use martensite::widgets::badge::Badge;
use martensite::widgets::banner::{Banner, Severity};
use martensite::widgets::bar_chart::BarChart;
use martensite::widgets::box_plot::{BoxPlot, BoxSeries};
use martensite::widgets::bullet_chart::BulletChart;
use martensite::widgets::candlestick::{Candle, Candlestick};
use martensite::widgets::chip::{Chip, ChipKind};
use martensite::widgets::chip_group::{ChipGroup, ChipSelection};
use martensite::widgets::countdown::Countdown;
use martensite::widgets::countdown_ring::CountdownRing;
use martensite::widgets::dial::Dial;
use martensite::widgets::digital_clock::DigitalClock;
use martensite::widgets::empty_state::EmptyState;
use martensite::widgets::flex::Flex;
use martensite::widgets::funnel_chart::FunnelChart;
use martensite::widgets::gantt::Gantt;
use martensite::widgets::gauge::Gauge;
use martensite::widgets::graph_view::GraphView;
use martensite::widgets::heat_map::HeatMap;
use martensite::widgets::histogram::Histogram;
use martensite::widgets::lcd_number::LcdNumber;
use martensite::widgets::led_matrix::LedMatrix;
use martensite::widgets::level_bar::LevelBar;
use martensite::widgets::line_chart::{LineChart, LineSeries};
use martensite::widgets::marquee::Marquee;
use martensite::widgets::odometer::Odometer;
use martensite::widgets::org_chart::{OrgChart, OrgNode};
use martensite::widgets::perf_overlay::PerfOverlay;
use martensite::widgets::pie_chart::{PieChart, PieSlice};
use martensite::widgets::polar_area::PolarArea;
use martensite::widgets::presence::{Presence, PresenceStatus};
use martensite::widgets::progress::{ProgressBar, Spinner};
use martensite::widgets::radar_chart::{RadarChart, RadarSeries};
use martensite::widgets::sankey::Sankey;
use martensite::widgets::scatter_chart::{ScatterChart, ScatterSeries};
use martensite::widgets::sparkline::{SparkStyle, Sparkline};
use martensite::widgets::split_flap::SplitFlap;
use martensite::widgets::stack_light::{Lamp, StackLight};
use martensite::widgets::statistic::{Statistic, Trend};
use martensite::widgets::status_dot::{Status, StatusDot};
use martensite::widgets::stopwatch::Stopwatch;
use martensite::widgets::stream_graph::StreamGraph;
use martensite::widgets::strip_chart::StripChart;
use martensite::widgets::switch::Switch;
use martensite::widgets::text::Text;
use martensite::widgets::thermometer::Thermometer;
use martensite::widgets::timeline::{Timeline, TimelineDot, TimelineItem};
use martensite::widgets::toast::{Toast, ToastHost};
use martensite::widgets::treemap::{Treemap, TreemapItem};
use martensite::widgets::violin::Violin;
use martensite::widgets::waterfall::Waterfall;
use martensite::widgets::week_view::{WeekEvent, WeekView};
use martensite::widgets::world_clock::WorldClock;
use martensite::widgets::Time;

use crate::domain::{
    Alarm as DomAlarm, AlarmSeverity, Asset, AssetKind, AssetStatus, CrewMember, MaintTask,
    PlantModel, WoStatus, HISTORY_LEN,
};
use crate::zone::{band, framed, row, strip, Bound, BAND_L, BAND_M, BAND_S, ZONE_GAP, ZONE_STACK};
use martensite::core::widget::DummyWidget;

/// Simulated shift starts at 06:00 — every clock widget's offset.
const SHIFT_START_HOUR: u32 = 6;
/// Shift length in simulated minutes (8 h).
const SHIFT_LEN_MIN: u32 = 480;
/// Response budget for the oldest unacked alarm (minutes).
const ALARM_SLA_MIN: u32 = 60;
/// Crew swimlane colors — index-aligned with `PlantModel::crew`.
const CREW_COLORS: [[u8; 4]; 6] = [
    [96, 165, 250, 255],
    [92, 200, 120, 255],
    [250, 190, 60, 255],
    [230, 120, 180, 255],
    [140, 140, 220, 255],
    [110, 190, 190, 255],
];
/// Alarm severity tints (Info / Warning / Critical).
const SEV_COLORS: [[u8; 4]; 3] = [[96, 165, 250, 255], [250, 190, 60, 255], [230, 70, 60, 255]];

/// Domain-named zone pages for the Telemetry panel — tab labels are
/// domain names ("ALARM BOARD"), never widget names.
pub fn pages(model: &PlantModel) -> Vec<(&'static str, Flex)> {
    vec![
        ("TRENDS", trends(model)),
        ("INSTRUMENTS", instruments(model)),
        ("ALARMS", alarm_board(model)),
        ("DISTRIBUTIONS", distributions(model)),
        ("SCHEDULE", schedule(model)),
        ("CREW", crew(model)),
        ("SYSTEM", system(model)),
    ]
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Signal-family group label — the council's chart-grouping grammar.
fn group_label(text: &'static str) -> Text {
    Text::new(text).font_size(11.0)
}

/// `Signal<Vec<f64>>` ring → chart points.
fn hist(s: &martensite::reactive::Signal<Vec<f64>>) -> Vec<f32> {
    s.get().iter().map(|v| *v as f32).collect()
}

/// `alarm_filter` bitmask bit for a severity (1/2/4).
fn severity_bit(s: AlarmSeverity) -> u8 {
    match s {
        AlarmSeverity::Info => 1,
        AlarmSeverity::Warning => 2,
        AlarmSeverity::Critical => 4,
    }
}

/// Domain severity → widget `Severity` (banner/alarm-panel/toast).
fn widget_sev(s: AlarmSeverity) -> Severity {
    match s {
        AlarmSeverity::Info => Severity::Info,
        AlarmSeverity::Warning => Severity::Warning,
        AlarmSeverity::Critical => Severity::Error,
    }
}

/// Active, unacked, filter-masked alarms — severity desc, then age.
/// This is the single ordering every alarm-widget binding shares, so
/// panel indices map back to model ids deterministically.
fn visible_alarms(m: &PlantModel) -> Vec<DomAlarm> {
    let mask = m.alarm_filter.get();
    let mut v: Vec<DomAlarm> = m
        .active_alarms()
        .into_iter()
        .filter(|a| mask & severity_bit(a.severity) != 0)
        .collect();
    v.sort_by(|a, b| {
        b.severity
            .cmp(&a.severity)
            .then(a.raised_min.cmp(&b.raised_min))
    });
    v
}

/// Active, unacked alarm count — the badge/empty-state probe; no
/// list materialization.
fn active_count(m: &PlantModel) -> usize {
    m.alarms
        .get()
        .iter()
        .filter(|a| a.active && !a.acked)
        .count()
}

/// Cell-kind assets (the OEE-bearing nodes).
fn cells(m: &PlantModel) -> Vec<Asset> {
    m.assets
        .get()
        .into_iter()
        .filter(|a| a.kind == AssetKind::Cell)
        .collect()
}

/// Line-kind assets.
fn lines(m: &PlantModel) -> Vec<Asset> {
    m.assets
        .get()
        .into_iter()
        .filter(|a| a.kind == AssetKind::Line)
        .collect()
}

/// First `kind` ancestor of asset `id` on an already-fetched slice —
/// the same 8-hop-capped parent walk `line_of`/`site_of` used, minus
/// a signal clone per hop. Callers fetch `m.assets` once.
fn ancestor_in(assets: &[Asset], id: u32, kind: AssetKind) -> Option<u32> {
    let mut cur = assets.iter().find(|a| a.id == id)?;
    for _ in 0..8 {
        if cur.kind == kind {
            return Some(cur.id);
        }
        cur = assets.iter().find(|a| Some(a.id) == cur.parent)?;
    }
    None
}

/// Unique asset ids referenced by `material_flow`, in flow order —
/// shared by the Sankey/GraphView builders and their index pulls.
fn flow_node_ids(m: &PlantModel) -> Vec<u32> {
    let mut ids: Vec<u32> = Vec::new();
    for (f, t, _) in m.material_flow.get() {
        for id in [f, t] {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
    }
    ids
}

/// Simulated-clock time-of-day for `shift_minute`.
fn shift_time(m: &PlantModel) -> Time {
    let min = m.shift_minute.get();
    Time {
        hour: (SHIFT_START_HOUR + min / 60) % 24,
        minute: min % 60,
    }
}

/// Domain presence → widget `PresenceStatus`.
fn crew_status(p: crate::domain::Presence) -> PresenceStatus {
    use crate::domain::Presence as DP;
    match p {
        DP::OnShift => PresenceStatus::Online,
        DP::Remote => PresenceStatus::Busy,
        DP::Break => PresenceStatus::Away,
        DP::OffShift => PresenceStatus::Offline,
    }
}

/// Crew members present for duty (on shift or remote).
fn on_shift(m: &PlantModel) -> usize {
    m.crew
        .get()
        .iter()
        .filter(|c| {
            matches!(
                c.presence,
                crate::domain::Presence::OnShift | crate::domain::Presence::Remote
            )
        })
        .count()
}

// --- push-gate signatures ---------------------------------------------------
// Construct-once widgets (`*w = …` re-seats) rebuild only when the
// model data they render actually changes. Each signature is a cheap
// per-tick probe — field folds, never format!/join.

/// FNV-1a fold — one mix step of the field-hash signatures below.
fn sig_fold(h: u64, v: u64) -> u64 {
    (h ^ v).wrapping_mul(0x100_0000_01b3)
}

/// `&str` content fold — bytes through `Hasher::write`, so identity
/// tracks contents, not pointer provenance (an owned `String` field
/// couldn't alias across reallocations).
fn str_fold(h: u64, s: &str) -> u64 {
    let mut w = DefaultHasher::new();
    w.write(s.as_bytes());
    sig_fold(h, w.finish())
}

/// Asset-store signature — OEE/status/name edits flip every
/// asset-driven gauge and distribution view.
fn assets_sig(m: &PlantModel) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for a in m.assets.get() {
        h = sig_fold(h, u64::from(a.id));
        h = sig_fold(h, a.parent.map(u64::from).unwrap_or(u64::MAX));
        h = sig_fold(h, a.kind as u64);
        h = sig_fold(h, a.status as u64);
        h = sig_fold(h, a.oee.to_bits());
        h = sig_fold(h, u64::from(a.installed));
        h = str_fold(h, a.name);
        h = str_fold(h, a.serial);
        h = str_fold(h, a.note);
    }
    h
}

/// Crew name+count signature — roster membership/identity changes
/// (a rename at the same headcount still flips this).
fn crew_names_sig(m: &PlantModel) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for c in m.crew.get() {
        h = str_fold(h, c.name);
    }
    h
}

/// Crew-roster signature — OrgChart/AvatarGroup/Gantt swimlane names.
fn crew_sig(m: &PlantModel) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for c in m.crew.get() {
        h = str_fold(h, c.name);
        h = str_fold(h, c.role);
        h = sig_fold(h, c.presence as u64);
        h = sig_fold(h, c.room as u64);
        h = sig_fold(h, c.reports_to.map(|i| i as u64).unwrap_or(u64::MAX));
    }
    h
}

/// Alarm-store signature — active/acked flips move the pie and the
/// alarms-by-line polar view.
fn alarms_sig(m: &PlantModel) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for a in m.alarms.get() {
        h = sig_fold(h, u64::from(a.id));
        h = sig_fold(h, u64::from(a.asset));
        h = sig_fold(h, a.severity as u64);
        h = sig_fold(h, a.active as u64);
        h = sig_fold(h, a.acked as u64);
        h = sig_fold(h, u64::from(a.raised_min));
        h = str_fold(h, a.message);
    }
    h
}

/// Work-order signature — funnel stages and the radar's WO axis.
fn wos_sig(m: &PlantModel) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for w in m.work_orders.get() {
        h = sig_fold(h, u64::from(w.id));
        h = sig_fold(h, u64::from(w.asset));
        h = sig_fold(h, w.status as u64);
        h = sig_fold(h, w.progress.to_bits());
        h = sig_fold(h, w.assignee as u64);
        h = sig_fold(h, u64::from(w.due_day));
        h = str_fold(h, w.title);
    }
    h
}

/// Material-flow signature — Sankey/GraphView re-seat gate.
fn flow_sig(m: &PlantModel) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for (f, t, v) in m.material_flow.get() {
        h = sig_fold(h, u64::from(f));
        h = sig_fold(h, u64::from(t));
        h = sig_fold(h, v.to_bits());
    }
    h
}

/// History revision — `push_history` bumps `hist_rev` once after
/// updating both rings, so a bit-identical tail sample on a full
/// ring still re-seats the charts (and no `Vec` clone per probe).
fn hist_sig(m: &PlantModel) -> u64 {
    m.hist_rev.get()
}

/// Shift-log signature — the monotonic `id` covers appends and cap
/// drains; the field folds cover in-place edits (`pinned` toggles
/// via `shift_log.update`, which a tail-only key would miss).
fn log_sig(m: &PlantModel) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for e in m.shift_log.get() {
        h = sig_fold(h, e.id);
        h = sig_fold(h, u64::from(e.minute));
        h = sig_fold(h, e.author as u64);
        h = sig_fold(h, u64::from(e.pinned));
        h = str_fold(h, &e.text);
    }
    h
}

/// Radar inputs — selected asset plus the three stores it reads.
fn radar_sig(m: &PlantModel) -> (Option<u32>, u64, u64, u64) {
    (
        m.selected_asset.get(),
        assets_sig(m),
        alarms_sig(m),
        wos_sig(m),
    )
}

// ---------------------------------------------------------------------------
// PROCESS TRENDS — every series bound to the shared history rings,
// grouped by signal family.
// ---------------------------------------------------------------------------

fn trends(m: &PlantModel) -> Flex {
    // OHLC windows of the cpu ring — an honest derived dataset.
    let candles = |m: &PlantModel| -> Vec<Candle> {
        let h = m.cpu_hist.get();
        let w = (h.len() / 12).max(1);
        h.chunks(w)
            .map(|ch| {
                let o = ch[0] as f32;
                let c = ch[ch.len() - 1] as f32;
                let hi = ch.iter().copied().fold(f64::MIN, f64::max) as f32;
                let lo = ch.iter().copied().fold(f64::MAX, f64::min) as f32;
                Candle::new(o, hi.max(lo), lo.min(hi), c)
            })
            .collect()
    };
    // Memory ring downsampled to 12 bucket means for the bar chart.
    let mem_bars = |m: &PlantModel| -> BarChart {
        let h = m.mem_hist.get();
        let w = (h.len() / 12).max(1);
        let bars: Vec<(String, f32)> = h
            .chunks(w)
            .enumerate()
            .map(|(i, ch)| {
                (
                    format!("S{}", i + 1),
                    (ch.iter().sum::<f64>() / ch.len() as f64 * 100.0) as f32,
                )
            })
            .collect();
        BarChart::new().bars(bars).label("mem bucket mean %")
    };
    Flex::column()
        .gap(ZONE_STACK)
        .child(group_label("CPU LOAD — 240-SAMPLE RING"))
        .child(
            row()
                .child_flex(
                    band(
                        BAND_M,
                        Bound::new(
                            LineChart::new()
                                .series(LineSeries::new("cpu", hist(&m.cpu_hist)))
                                .axis(true),
                            m,
                        )
                        .push({
                            let mut last = hist_sig(m);
                            move |c: &mut LineChart, m| {
                                let sig = hist_sig(m);
                                if sig != last {
                                    if let Some(s) = c.series.first_mut() {
                                        s.points = hist(&m.cpu_hist);
                                    }
                                    last = sig;
                                }
                            }
                        }),
                    ),
                    1.0,
                )
                .child_flex(
                    band(
                        BAND_M,
                        Bound::new(
                            Candlestick::new()
                                .candles(candles(m))
                                .y_range(0.0, 1.0)
                                .grid(true),
                            m,
                        )
                        .push({
                            let mut last = hist_sig(m);
                            move |c: &mut Candlestick, m| {
                                let sig = hist_sig(m);
                                if sig != last {
                                    *c = Candlestick::new()
                                        .candles(candles(m))
                                        .y_range(0.0, 1.0)
                                        .grid(true);
                                    last = sig;
                                }
                            }
                        }),
                    ),
                    1.0,
                ),
        )
        .child(
            row()
                .child_flex(
                    band(
                        BAND_M,
                        Bound::new(
                            StripChart::new()
                                .capacity(HISTORY_LEN)
                                .range(0.0, 1.0)
                                .label("cpu stream"),
                            m,
                        )
                        .push({
                            // Mounted empty — first tick fills the strip.
                            let mut last = None;
                            move |s: &mut StripChart, m| {
                                let sig = hist_sig(m);
                                if Some(sig) != last {
                                    s.clear();
                                    s.extend(hist(&m.cpu_hist));
                                    last = Some(sig);
                                }
                            }
                        }),
                    ),
                    1.0,
                )
                .child_flex(
                    band(
                        BAND_M,
                        Bound::new(
                            Sparkline::new(hist(&m.cpu_hist))
                                .style(SparkStyle::Area)
                                .label("cpu area"),
                            m,
                        )
                        .push({
                            let mut last = hist_sig(m);
                            move |s: &mut Sparkline, m| {
                                let sig = hist_sig(m);
                                if sig != last {
                                    s.set_data(hist(&m.cpu_hist));
                                    last = sig;
                                }
                            }
                        }),
                    ),
                    1.0,
                ),
        )
        .child(group_label("MEMORY — SAME RING, OTHER VIEWS"))
        .child(
            row()
                .child_flex(
                    band(
                        BAND_M,
                        Bound::new(
                            LineChart::new()
                                .series(LineSeries::new("mem", hist(&m.mem_hist)))
                                .axis(true),
                            m,
                        )
                        .push({
                            let mut last = hist_sig(m);
                            move |c: &mut LineChart, m| {
                                let sig = hist_sig(m);
                                if sig != last {
                                    if let Some(s) = c.series.first_mut() {
                                        s.points = hist(&m.mem_hist);
                                    }
                                    last = sig;
                                }
                            }
                        }),
                    ),
                    1.0,
                )
                .child_flex(
                    band(
                        BAND_M,
                        Bound::new(
                            Sparkline::new(hist(&m.mem_hist))
                                .style(SparkStyle::Bars)
                                .label("mem bars"),
                            m,
                        )
                        .push({
                            let mut last = hist_sig(m);
                            move |s: &mut Sparkline, m| {
                                let sig = hist_sig(m);
                                if sig != last {
                                    s.set_data(hist(&m.mem_hist));
                                    last = sig;
                                }
                            }
                        }),
                    ),
                    1.0,
                )
                .child_flex(
                    band(
                        BAND_M,
                        Bound::new(mem_bars(m), m).push({
                            let mut last = hist_sig(m);
                            move |b: &mut BarChart, m| {
                                let sig = hist_sig(m);
                                if sig != last {
                                    *b = mem_bars(m);
                                    last = sig;
                                }
                            }
                        }),
                    ),
                    1.0,
                ),
        )
        .child(group_label("DERIVED — SAME SIGNALS, OTHER SHAPES"))
        .child(
            row()
                .child_flex(
                    band(
                        BAND_M,
                        Bound::new(
                            StreamGraph::new()
                                .layer("cpu", hist(&m.cpu_hist))
                                .layer("mem", hist(&m.mem_hist))
                                .label("load layers"),
                            m,
                        )
                        .push({
                            // One rev covers both rings — `push_history`
                            // bumps it after updating cpu and mem.
                            let mut last = hist_sig(m);
                            move |g: &mut StreamGraph, m| {
                                let sig = hist_sig(m);
                                if sig != last {
                                    *g = StreamGraph::new()
                                        .layer("cpu", hist(&m.cpu_hist))
                                        .layer("mem", hist(&m.mem_hist))
                                        .label("load layers");
                                    last = sig;
                                }
                            }
                        }),
                    ),
                    1.0,
                )
                .child_flex(
                    band(
                        BAND_M,
                        Bound::new(HeatMap::new(6, 40), m).push({
                            // Mounted empty — the first tick fills the grid.
                            let mut last = None;
                            move |hm: &mut HeatMap, m| {
                                let sig = hist_sig(m);
                                if Some(sig) != last {
                                    let h = m.cpu_hist.get();
                                    for r in 0..6 {
                                        for c in 0..40 {
                                            let v = h.get(r * 40 + c).copied().unwrap_or(0.0);
                                            hm.set_cell(r, c, (v * 10.0) as f32);
                                        }
                                    }
                                    last = Some(sig);
                                }
                            }
                        }),
                    ),
                    1.0,
                )
                .child_flex(
                    band(
                        BAND_M,
                        Bound::new(
                            Histogram::new()
                                .bins(10)
                                .samples(hist(&m.cpu_hist))
                                .label("cpu distribution"),
                            m,
                        )
                        .push({
                            let mut last = hist_sig(m);
                            move |h: &mut Histogram, m| {
                                let sig = hist_sig(m);
                                if sig != last {
                                    *h = Histogram::new()
                                        .bins(10)
                                        .samples(hist(&m.cpu_hist))
                                        .label("cpu distribution");
                                    last = sig;
                                }
                            }
                        }),
                    ),
                    1.0,
                ),
        )
}

// ---------------------------------------------------------------------------
// INSTRUMENTS — live readouts of cpu / mem / oee / line state.
// ---------------------------------------------------------------------------

/// The CPU gauge — also the test's binding round-trip target.
fn cpu_gauge(model: &PlantModel) -> Bound<Gauge> {
    Bound::new(
        Gauge::new()
            .range(0.0, 100.0)
            .value(0.0)
            .label("CPU %")
            .zones(70.0, 90.0)
            .ticks(true),
        model,
    )
    .push(|g: &mut Gauge, m| g.set_value(m.cpu.get() * 100.0))
}

fn instruments(m: &PlantModel) -> Flex {
    Flex::column()
        .gap(ZONE_STACK)
        .child(group_label("ANALOG — LIVE LOAD"))
        .child(
            row()
                .child_flex(band(BAND_M, framed(1.0, cpu_gauge(m))), 1.0)
                .child_flex(
                    band(
                        BAND_M,
                        framed(
                            1.0,
                            Bound::new(
                                Dial::new()
                                    .range(80.0, 160.0)
                                    .value(120.0)
                                    .step(0.5)
                                    .enabled(false),
                                m,
                            )
                            .push(|d: &mut Dial, m| {
                                d.set_value(m.acoustic.get().dominant_hz);
                            }),
                        ),
                    ),
                    1.0,
                )
                .child_flex(
                    band(
                        BAND_M,
                        Bound::new(
                            Thermometer::new()
                                .range(0.0, 100.0)
                                .value(0.0)
                                .units("%")
                                .warning(0.7)
                                .critical(0.9)
                                .ticks(5)
                                .label("mem %"),
                            m,
                        )
                        .push({
                            // Reads `mem` only — re-seat when the sample moves.
                            let mut last = None;
                            move |t: &mut Thermometer, m| {
                                let sig = m.mem.get().to_bits();
                                if Some(sig) != last {
                                    *t = Thermometer::new()
                                        .range(0.0, 100.0)
                                        .value((m.mem.get() * 100.0) as f32)
                                        .units("%")
                                        .warning(0.7)
                                        .critical(0.9)
                                        .ticks(5)
                                        .label("mem %");
                                    last = Some(sig);
                                }
                            }
                        }),
                    ),
                    1.0,
                )
                .child_flex(
                    band(
                        BAND_M,
                        Bound::new(
                            LevelBar::new()
                                .value(0.0)
                                .zones(0.5, 0.75, 0.9)
                                .segments(10),
                            m,
                        )
                        .push(|l: &mut LevelBar, m| l.set_value(m.cpu.get() as f32)),
                    ),
                    1.0,
                ),
        )
        .child(group_label("EFFECTIVENESS — OEE"))
        .child(
            row()
                .child_flex(
                    band(
                        BAND_M,
                        Bound::new(ProgressBar::new().value(0.0), m).push({
                            // `plant_oee` folds the cell assets — gate on them.
                            let mut last = None;
                            move |p: &mut ProgressBar, m| {
                                let sig = assets_sig(m);
                                if Some(sig) != last {
                                    *p = ProgressBar::new().value(m.plant_oee() as f32);
                                    last = Some(sig);
                                }
                            }
                        }),
                    ),
                    1.0,
                )
                .child_flex(
                    band(
                        BAND_M,
                        framed(
                            1.0,
                            Bound::new(ActivityRing::new().label("load goals"), m).push({
                                // cpu + mem live signals plus the OEE rollup.
                                let mut last = None;
                                move |a: &mut ActivityRing, m| {
                                    let sig = (
                                        m.cpu.get().to_bits(),
                                        m.mem.get().to_bits(),
                                        assets_sig(m),
                                    );
                                    if Some(sig) != last {
                                        *a = ActivityRing::new()
                                            .label("load goals")
                                            .ring("CPU", m.cpu.get() as f32, [96, 165, 250, 255])
                                            .ring("MEM", m.mem.get() as f32, [110, 180, 130, 255])
                                            .ring("OEE", m.plant_oee() as f32, [250, 190, 60, 255]);
                                        last = Some(sig);
                                    }
                                }
                            }),
                        ),
                    ),
                    1.0,
                )
                .child_flex(
                    band(
                        BAND_M,
                        Bound::new(
                            BulletChart::new()
                                .label("OEE %")
                                .value(0.0)
                                .target(85.0)
                                .ranges([60.0, 80.0, 100.0]),
                            m,
                        )
                        .push({
                            let mut last = None;
                            move |b: &mut BulletChart, m| {
                                let sig = assets_sig(m);
                                if Some(sig) != last {
                                    *b = BulletChart::new()
                                        .label("OEE %")
                                        .value((m.plant_oee() * 100.0) as f32)
                                        .target(85.0)
                                        .ranges([60.0, 80.0, 100.0]);
                                    last = Some(sig);
                                }
                            }
                        }),
                    ),
                    1.0,
                )
                .child(
                    Bound::new(Spinner::new().label("LINE"), m).push(|s: &mut Spinner, m| {
                        let run = m.line_running.get() && !m.paused.get();
                        if s.is_active() != run {
                            if run {
                                s.start();
                            } else {
                                s.stop();
                            }
                        }
                    }),
                ),
        )
        .child(group_label("NUMERIC — SIM CLOCK & STATE"))
        .child(
            strip()
                .child(
                    Bound::new(LcdNumber::new().value(0.0).digits(3).decimals(1), m)
                        .push(|l: &mut LcdNumber, m| l.value = m.cpu.get() * 100.0),
                )
                .child(
                    Bound::new(LcdNumber::new().value(0.0).digits(3).decimals(1), m)
                        .push(|l: &mut LcdNumber, m| l.value = m.mem.get() * 100.0),
                )
                .child(
                    Bound::new(Odometer::new().digits(4).value(0).label("SHIFT MIN"), m)
                        .push(|o: &mut Odometer, m| o.set_value(u64::from(m.shift_minute.get()))),
                )
                .child(
                    Bound::new(SplitFlap::new().cells(6).text("——").label("LINE"), m).push(
                        |f: &mut SplitFlap, m| {
                            let state = if !m.line_running.get() {
                                "DOWN"
                            } else if m.paused.get() {
                                "HOLD"
                            } else {
                                "RUN"
                            };
                            if f.target_text() != state {
                                *f = SplitFlap::new().cells(6).text(state).label("LINE");
                            }
                        },
                    ),
                )
                .child_flex(DummyWidget, 1.0),
        )
        .child(group_label("KPI — HEADLINES"))
        .child(
            strip()
                .child(
                    Bound::new(
                        Statistic::new("PLANT OEE", "—")
                            .suffix("%")
                            .trend(Trend::Up, "cells mean"),
                        m,
                    )
                    .push({
                        // `plant_oee` folds the cell assets — gate on them.
                        let mut last = None;
                        move |s: &mut Statistic, m| {
                            let sig = assets_sig(m);
                            if Some(sig) != last {
                                s.set_value(format!("{:.1}", m.plant_oee() * 100.0));
                                last = Some(sig);
                            }
                        }
                    }),
                )
                .child(Bound::new(Statistic::new("ACTIVE ALARMS", "—"), m).push({
                    let mut last = None;
                    move |s: &mut Statistic, m| {
                        let sig = alarms_sig(m);
                        if Some(sig) != last {
                            s.set_value(format!("{}", active_count(m)));
                            last = Some(sig);
                        }
                    }
                }))
                .child(Bound::new(Statistic::new("CREW ON SHIFT", "—"), m).push({
                    let mut last = None;
                    move |s: &mut Statistic, m| {
                        let sig = crew_sig(m);
                        if Some(sig) != last {
                            s.set_value(format!("{}", on_shift(m)));
                            last = Some(sig);
                        }
                    }
                }))
                .child(
                    Bound::new(Statistic::new("WO DONE", "—").suffix("%"), m).push({
                        let mut last = None;
                        move |s: &mut Statistic, m| {
                            let sig = wos_sig(m);
                            if Some(sig) != last {
                                let wos = m.work_orders.get();
                                let done = wos.iter().filter(|w| w.progress >= 1.0).count() as f64;
                                let pct = if wos.is_empty() {
                                    0.0
                                } else {
                                    done / wos.len() as f64 * 100.0
                                };
                                s.set_value(format!("{:.0}", pct));
                                last = Some(sig);
                            }
                        }
                    }),
                )
                .child_flex(DummyWidget, 1.0),
        )
}

// ---------------------------------------------------------------------------
// ALARM BOARD — active alarms, severity lamps, filter, SLA clock.
// ---------------------------------------------------------------------------

fn filter_chips(mask: u8) -> ChipGroup {
    ChipGroup::new()
        .chip(
            Chip::new("INFO")
                .kind(ChipKind::Filter)
                .selected(mask & 1 != 0),
        )
        .chip(
            Chip::new("WARN")
                .kind(ChipKind::Filter)
                .selected(mask & 2 != 0),
        )
        .chip(
            Chip::new("CRIT")
                .kind(ChipKind::Filter)
                .selected(mask & 4 != 0),
        )
        .selection(ChipSelection::Multiple)
        .label("severity filter")
}

fn chip_mask(g: &ChipGroup) -> u8 {
    g.selected_indices()
        .iter()
        .fold(0u8, |acc, i| acc | (1u8 << i))
}

fn banner_of(m: &PlantModel) -> Banner {
    match visible_alarms(m).first() {
        Some(a) => Banner::new(
            widget_sev(a.severity),
            format!("{} — {}", m.asset_name(a.asset), a.message),
        )
        .dismissible(true),
        None => Banner::new(Severity::Info, "ALL CLEAR — no active alarms"),
    }
}

fn empty_state_of(m: &PlantModel) -> EmptyState {
    let active = m.active_alarms();
    if active.is_empty() {
        EmptyState::new("ALL CLEAR")
            .icon("✓")
            .description("all channels within limits")
    } else {
        EmptyState::new(format!("{} ACTIVE ALARMS", active.len()))
            .icon("⚠")
            .description("acknowledge to clear the board")
            .action("ACK ALL")
    }
}

fn alarm_board(m: &PlantModel) -> Flex {
    Flex::column()
        .gap(ZONE_STACK)
        .child(
            row()
                .child_flex(
                    band(
                        BAND_L,
                        Bound::new(AlarmPanel::new().label("ACTIVE ALARMS"), m)
                            .pull(|p: &mut AlarmPanel, m| {
                                // Row ACK chip → acknowledge the model alarm.
                                if let Some(i) = p.take_acked() {
                                    if let Some(a) = visible_alarms(m).get(i) {
                                        m.ack_alarm(a.id);
                                    }
                                }
                            })
                            .push({
                                // Filter mask + alarm store + asset names —
                                // a message/name edit re-seats too, which the
                                // old id-list key missed.
                                let mut last: Option<(u8, u64, u64)> = None;
                                move |p: &mut AlarmPanel, m| {
                                    let sig = (m.alarm_filter.get(), alarms_sig(m), assets_sig(m));
                                    if Some(sig) != last {
                                        while p.count() > 0 {
                                            p.clear(0);
                                        }
                                        // push() prepends — iterate reversed so
                                        // index 0 is the most severe alarm.
                                        for a in visible_alarms(m).iter().rev() {
                                            p.push(
                                                PanelAlarm::new(widget_sev(a.severity), a.message)
                                                    .source(m.asset_name(a.asset)),
                                            );
                                        }
                                        last = Some(sig);
                                    }
                                }
                            }),
                    ),
                    2.0,
                )
                .child_flex(
                    Flex::column()
                        .gap(ZONE_GAP)
                        .child(
                            Bound::new(
                                StackLight::new()
                                    .lamp(Lamp::new("FAULT", [230, 70, 60, 255]).flashing(true))
                                    .lamp(Lamp::new("WARN", [250, 190, 60, 255]))
                                    .lamp(Lamp::new("RUN", [92, 200, 120, 255])),
                                m,
                            )
                            .push(|s: &mut StackLight, m| {
                                let (_, warn, crit) = m.alarm_distribution();
                                s.set(0, crit > 0);
                                s.set(1, crit == 0 && warn > 0);
                                s.set(2, m.line_running.get());
                            }),
                        )
                        .child(
                            Bound::new(StatusDot::new("LINE").status(Status::Ok), m).push(
                                |d: &mut StatusDot, m| {
                                    let (_, warn, crit) = m.alarm_distribution();
                                    let st = if crit > 0 {
                                        Status::Error
                                    } else if warn > 0 {
                                        Status::Warning
                                    } else if !m.line_running.get() {
                                        Status::Off
                                    } else {
                                        Status::Ok
                                    };
                                    d.set_status(st);
                                },
                            ),
                        )
                        .child(
                            Bound::new(Badge::wrap(Text::new("ACTIVE ALARMS")).with_count(0), m)
                                .push(|b: &mut Badge, m| b.count = active_count(m) as u32),
                        ),
                    1.0,
                ),
        )
        .child(
            strip()
                .child(
                    Bound::new(filter_chips(m.alarm_filter.get()), m)
                        .pull(|g: &mut ChipGroup, m| {
                            // Chip toggles → severity bitmask.
                            if g.take_changed().is_some() {
                                m.alarm_filter.set_if_changed(chip_mask(g));
                            }
                        })
                        .push({
                            // Probe the widget only when the model mask
                            // moved — chip toggles re-sync through `pull`
                            // in the same tick, so a changed mask with
                            // matching chips needs no re-seat, and an
                            // unchanged mask skips the `chip_mask` Vec.
                            let mut last = m.alarm_filter.get();
                            move |g: &mut ChipGroup, m| {
                                let mask = m.alarm_filter.get();
                                if mask != last {
                                    last = mask;
                                    if chip_mask(g) != mask {
                                        *g = filter_chips(mask);
                                    }
                                }
                            }
                        }),
                )
                .child_flex(
                    Bound::new(banner_of(m), m)
                        .pull(|b: &mut Banner, m| {
                            // Dismissing the top banner acknowledges it.
                            if b.take_dismissed() {
                                if let Some(a) = visible_alarms(m).first() {
                                    m.ack_alarm(a.id);
                                }
                            }
                        })
                        .push({
                            // Top visible alarm + its asset label —
                            // folds stand in for the sorted-list probe.
                            let mut last = (m.alarm_filter.get(), alarms_sig(m), assets_sig(m));
                            move |b: &mut Banner, m| {
                                let sig = (m.alarm_filter.get(), alarms_sig(m), assets_sig(m));
                                if sig != last {
                                    *b = banner_of(m);
                                    last = sig;
                                }
                            }
                        }),
                    1.0,
                ),
        )
        .child(
            row()
                .child_flex(
                    band(
                        BAND_M,
                        Bound::new(empty_state_of(m), m)
                            .pull(|e: &mut EmptyState, m| {
                                // "ACK ALL" button press.
                                if e.take_activated() {
                                    for a in m.active_alarms() {
                                        m.ack_alarm(a.id);
                                    }
                                }
                            })
                            .push({
                                let mut last = usize::MAX;
                                move |e: &mut EmptyState, m| {
                                    let n = active_count(m);
                                    if n != last {
                                        *e = empty_state_of(m);
                                        last = n;
                                    }
                                }
                            }),
                    ),
                    1.0,
                )
                .child_flex(
                    band(
                        BAND_M,
                        framed(
                            1.0,
                            Bound::new(
                                CountdownRing::new(Duration::from_secs(
                                    u64::from(ALARM_SLA_MIN) * 60,
                                ))
                                .warn_under(Duration::from_secs(10 * 60))
                                .label("SLA"),
                                m,
                            )
                            .push(|c: &mut CountdownRing, m| {
                                // Oldest unacked alarm vs. its response budget —
                                // folded off the store, no list materialized.
                                let oldest = m
                                    .alarms
                                    .get()
                                    .iter()
                                    .filter(|a| a.active && !a.acked)
                                    .map(|a| a.raised_min)
                                    .min();
                                let rem = match oldest {
                                    Some(raised) => (i64::from(ALARM_SLA_MIN)
                                        - (i64::from(m.shift_minute.get()) - i64::from(raised)))
                                    .max(0)
                                        as u64,
                                    None => u64::from(ALARM_SLA_MIN),
                                };
                                c.set_remaining(Duration::from_secs(rem * 60));
                                c.set_running(!m.paused.get());
                            }),
                        ),
                    ),
                    1.0,
                )
                .child_flex(
                    Bound::new(Marquee::new("—"), m).push({
                        // Text derives from the top visible alarm + its
                        // asset label — folds stand in for the
                        // sorted-list probe. `set_paused` stays ungated
                        // (cheap bool).
                        let mut last = None;
                        move |mq: &mut Marquee, m| {
                            mq.set_paused(m.reduced_motion.get());
                            let sig = (m.alarm_filter.get(), alarms_sig(m), assets_sig(m));
                            if Some(sig) != last {
                                let text = match visible_alarms(m).first() {
                                    Some(a) => format!(
                                        "▲ {} — {} @ {}",
                                        a.severity.label(),
                                        a.message,
                                        m.asset_name(a.asset)
                                    ),
                                    None => "NO ACTIVE ALARMS — ALL CHANNELS NOMINAL".to_string(),
                                };
                                // `set_text` rewinds the scroll — skip it
                                // when a sig change left the text identical.
                                if mq.text() != text {
                                    mq.set_text(text);
                                }
                                last = Some(sig);
                            }
                        }
                    }),
                    1.0,
                )
                .child(Bound::new(ToastHost::new(), m).push({
                    // Seeded alarms are already seen — only transitions
                    // (new ids appearing active) raise a toast. The store
                    // fold gates the per-tick active-set clone.
                    let mut seen: HashSet<u32> = m.active_alarms().iter().map(|a| a.id).collect();
                    let mut last = alarms_sig(m);
                    move |h: &mut ToastHost, m| {
                        let sig = alarms_sig(m);
                        if sig == last {
                            return;
                        }
                        last = sig;
                        let active = m.active_alarms();
                        for a in &active {
                            if seen.insert(a.id) {
                                h.push(
                                    Toast::new(
                                        widget_sev(a.severity),
                                        format!("{} — {}", m.asset_name(a.asset), a.message),
                                    )
                                    .ttl_secs(6.0),
                                );
                            }
                        }
                        // Retire departed ids — a re-raised alarm toasts
                        // again.
                        seen.retain(|id| active.iter().any(|a| a.id == *id));
                    }
                })),
        )
}

// ---------------------------------------------------------------------------
// DISTRIBUTIONS — alarms, OEE, pipeline, material flow.
// ---------------------------------------------------------------------------

fn alarm_slices(m: &PlantModel) -> Vec<PieSlice> {
    let (info, warn, crit) = m.alarm_distribution();
    vec![
        PieSlice::new(info as f32, "INFO").color(SEV_COLORS[0]),
        PieSlice::new(warn as f32, "WARN").color(SEV_COLORS[1]),
        PieSlice::new(crit as f32, "CRIT").color(SEV_COLORS[2]),
    ]
}

fn treemap_of(m: &PlantModel) -> Treemap {
    let mut t = Treemap::new();
    for (i, c) in cells(m).iter().enumerate() {
        t = t.item(
            TreemapItem::new(c.name, (c.oee * 100.0).max(1.0) as f32)
                .color(CREW_COLORS[i % CREW_COLORS.len()]),
        );
    }
    t
}

fn funnel_of(m: &PlantModel) -> FunnelChart {
    let wos = m.work_orders.get();
    let mut f = FunnelChart::new().label("WO pipeline");
    for st in WoStatus::columns() {
        let n = wos.iter().filter(|w| w.status == st).count();
        f = f.stage(st.label(), n as f32);
    }
    f
}

fn sankey_of(m: &PlantModel) -> Sankey {
    let ids = flow_node_ids(m);
    let mut s = Sankey::new().label("material flow t/h");
    for id in &ids {
        s = s.node(m.asset_name(*id));
    }
    for (f, t, v) in m.material_flow.get() {
        s = s.link(m.asset_name(f), m.asset_name(t), v as f32);
    }
    s
}

fn graph_of(m: &PlantModel) -> GraphView {
    let ids = flow_node_ids(m);
    let mut g = GraphView::new().label("flow network");
    for id in &ids {
        g = g.node(m.asset_name(*id));
    }
    for (f, t, _) in m.material_flow.get() {
        let a = ids.iter().position(|x| *x == f).unwrap_or(0);
        let b = ids.iter().position(|x| *x == t).unwrap_or(0);
        g = g.edge(a, b);
    }
    g
}

fn radar_of(m: &PlantModel) -> RadarChart {
    let mut r = RadarChart::new()
        .axes(["OEE", "UPTIME", "SERVICE", "CALM", "WO DONE"])
        .max(5.0);
    if let Some(id) = m.selected_asset.get() {
        if let Some(a) = m.asset(id) {
            let uptime = match a.status {
                AssetStatus::Running => 5.0,
                AssetStatus::Degraded => 3.0,
                AssetStatus::Maintenance => 2.0,
                AssetStatus::Down => 0.5,
            };
            let service =
                (5.0 * (1.0 - (2026 - i32::from(a.installed)) as f32 / 15.0)).clamp(0.0, 5.0);
            let alarms_on = m.active_alarms().iter().filter(|al| al.asset == id).count();
            let calm = (5.0 - alarms_on as f32 * 1.7).clamp(0.0, 5.0);
            let wos: Vec<f64> = m
                .work_orders
                .get()
                .iter()
                .filter(|w| w.asset == id)
                .map(|w| w.progress)
                .collect();
            let wo_done = if wos.is_empty() {
                2.5
            } else {
                (wos.iter().sum::<f64>() / wos.len() as f64 * 5.0) as f32
            };
            r = r.series(RadarSeries::new(
                a.name,
                [a.oee as f32 * 5.0, uptime, service, calm, wo_done],
            ));
        }
    }
    r
}

fn waterfall_of(m: &PlantModel) -> Waterfall {
    let ls = lines(m);
    let n = ls.len().max(1) as f32;
    let mut w = Waterfall::new()
        .total("TARGET", 100.0)
        .label("OEE gap by line %");
    for l in &ls {
        w = w.delta(l.name, -((1.0 - l.oee) as f32) * 100.0 / n);
    }
    w.total("ACTUAL", (m.plant_oee() * 100.0) as f32)
}

fn scatter_of(m: &PlantModel) -> ScatterChart {
    let pts: Vec<(f32, f32)> = cells(m)
        .iter()
        .map(|c| (f32::from(c.installed), (c.oee * 100.0) as f32))
        .collect();
    ScatterChart::new()
        .series(ScatterSeries::new("cells", pts).size(6.0))
        .x_range(2014.0, 2026.0)
        .y_range(0.0, 100.0)
        .grid(true)
}

fn quartiles(v: &[f64]) -> (f32, f32, f32, f32, f32) {
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.total_cmp(b));
    let pick = |p: f64| -> f32 {
        let i = ((s.len() - 1) as f64 * p).round() as usize;
        s[i.min(s.len() - 1)] as f32
    };
    (pick(0.0), pick(0.25), pick(0.5), pick(0.75), pick(1.0))
}

fn boxplot_of(m: &PlantModel) -> BoxPlot {
    // One fetch — the per-line filter reads the same vec.
    let assets = m.assets.get();
    let mut b = BoxPlot::new().label("cell OEE spread %");
    for l in assets.iter().filter(|a| a.kind == AssetKind::Line) {
        let o: Vec<f64> = assets
            .iter()
            .filter(|a| a.kind == AssetKind::Cell && a.parent == Some(l.id))
            .map(|a| a.oee * 100.0)
            .collect();
        if !o.is_empty() {
            let (mn, q1, md, q3, mx) = quartiles(&o);
            b = b.series(BoxSeries::new(l.name, mn, q1, md, q3, mx));
        }
    }
    b
}

fn violin_of(m: &PlantModel) -> Violin {
    // One fetch — sites, cells, and the ancestor walk all read it.
    let assets = m.assets.get();
    let mut v = Violin::new().label("cell OEE density");
    for s in assets.iter().filter(|a| a.kind == AssetKind::Site) {
        let mut bins = [0f32; 7];
        for c in assets.iter().filter(|a| a.kind == AssetKind::Cell) {
            if ancestor_in(&assets, c.id, AssetKind::Site) == Some(s.id) {
                bins[((c.oee * 7.0) as usize).min(6)] += 1.0;
            }
        }
        v = v.series(s.name, bins.to_vec());
    }
    v
}

fn polar_of(m: &PlantModel) -> PolarArea {
    // One fetch — per-alarm line walk + slice labels share it.
    let assets = m.assets.get();
    let mut counts: Vec<(u32, usize)> = Vec::new();
    for a in m.active_alarms() {
        if let Some(l) = ancestor_in(&assets, a.asset, AssetKind::Line) {
            match counts.iter_mut().find(|(id, _)| *id == l) {
                Some((_, n)) => *n += 1,
                None => counts.push((l, 1)),
            }
        }
    }
    let mut p = PolarArea::new().label("alarms by line");
    for (id, n) in counts {
        let name = assets
            .iter()
            .find(|a| a.id == id)
            .map(|a| a.name)
            .unwrap_or("—");
        p = p.slice(name, n as f32);
    }
    p
}

fn distributions(m: &PlantModel) -> Flex {
    Flex::column()
        .gap(ZONE_STACK)
        .child(group_label("ALARMS & WORK — WHERE ATTENTION SITS"))
        .child(
            row()
                .child_flex(
                    band(
                        BAND_M,
                        framed(
                            1.0,
                            Bound::new(PieChart::new(alarm_slices(m)).donut(), m)
                                .pull(|p: &mut PieChart, m| {
                                    // Slice click drills the board's severity
                                    // filter — `1 << i` is a bitmask, so bound
                                    // the widget-supplied index to the 3
                                    // severities (`i >= 8` would overflow).
                                    if let Some(i) = p.take_selected() {
                                        if i < 3 {
                                            m.alarm_filter.set_if_changed(1u8 << i);
                                        }
                                    }
                                })
                                .push({
                                    let mut last = m.alarm_distribution();
                                    move |p: &mut PieChart, m| {
                                        let sig = m.alarm_distribution();
                                        if sig != last {
                                            // `slices` is a plain field — no
                                            // setter on PieChart; writing it in
                                            // place keeps hover/press state.
                                            p.slices = alarm_slices(m);
                                            last = sig;
                                        }
                                    }
                                }),
                        ),
                    ),
                    1.0,
                )
                .child_flex(
                    band(
                        BAND_M,
                        Bound::new(treemap_of(m), m)
                            .pull(|t: &mut Treemap, m| {
                                // Hovering a cell inspects it (shared selection).
                                if let Some(i) = t.take_hovered() {
                                    if let Some(c) = cells(m).get(i) {
                                        m.selected_asset.set_if_changed(Some(c.id));
                                    }
                                }
                            })
                            .push({
                                // Gate the re-seat — an ungated rebuild
                                // would drop hover/press state every tick.
                                let mut last = assets_sig(m);
                                move |t: &mut Treemap, m| {
                                    let sig = assets_sig(m);
                                    if sig != last {
                                        *t = treemap_of(m);
                                        last = sig;
                                    }
                                }
                            }),
                    ),
                    1.0,
                )
                .child_flex(
                    band(
                        BAND_M,
                        Bound::new(funnel_of(m), m).push({
                            let mut last = wos_sig(m);
                            move |f: &mut FunnelChart, m| {
                                let sig = wos_sig(m);
                                if sig != last {
                                    *f = funnel_of(m);
                                    last = sig;
                                }
                            }
                        }),
                    ),
                    1.0,
                ),
        )
        .child(group_label("MATERIAL FLOW — REAL LINE TOPOLOGY"))
        .child(
            row()
                .child_flex(
                    band(
                        BAND_M,
                        Bound::new(sankey_of(m), m).push({
                            // Flow edges + the asset names the labels read.
                            let mut last = (flow_sig(m), assets_sig(m));
                            move |s: &mut Sankey, m| {
                                let sig = (flow_sig(m), assets_sig(m));
                                if sig != last {
                                    *s = sankey_of(m);
                                    last = sig;
                                }
                            }
                        }),
                    ),
                    1.0,
                )
                .child_flex(
                    band(
                        BAND_M,
                        Bound::new(graph_of(m), m)
                            .pull(|g: &mut GraphView, m| {
                                if let Some(i) = g.take_hovered() {
                                    if let Some(id) = flow_node_ids(m).get(i) {
                                        m.selected_asset.set_if_changed(Some(*id));
                                    }
                                }
                            })
                            .push({
                                // Gate the rebuild so force-layout settles —
                                // flow edges + the asset names they label with.
                                let mut last = (flow_sig(m), assets_sig(m));
                                move |g: &mut GraphView, m| {
                                    let sig = (flow_sig(m), assets_sig(m));
                                    if sig != last {
                                        *g = graph_of(m);
                                        last = sig;
                                    }
                                }
                            }),
                    ),
                    1.0,
                ),
        )
        .child(group_label("OEE — SHAPE OF THE FLEET"))
        .child(
            row()
                .child_flex(
                    band(
                        BAND_M,
                        Bound::new(waterfall_of(m), m).push({
                            let mut last = assets_sig(m);
                            move |w: &mut Waterfall, m| {
                                let sig = assets_sig(m);
                                if sig != last {
                                    *w = waterfall_of(m);
                                    last = sig;
                                }
                            }
                        }),
                    ),
                    1.0,
                )
                .child_flex(
                    band(
                        BAND_M,
                        Bound::new(scatter_of(m), m).push({
                            let mut last = assets_sig(m);
                            move |s: &mut ScatterChart, m| {
                                let sig = assets_sig(m);
                                if sig != last {
                                    *s = scatter_of(m);
                                    last = sig;
                                }
                            }
                        }),
                    ),
                    1.0,
                )
                .child_flex(
                    band(
                        BAND_M,
                        Bound::new(boxplot_of(m), m).push({
                            let mut last = assets_sig(m);
                            move |b: &mut BoxPlot, m| {
                                let sig = assets_sig(m);
                                if sig != last {
                                    *b = boxplot_of(m);
                                    last = sig;
                                }
                            }
                        }),
                    ),
                    1.0,
                ),
        )
        .child(
            row()
                .child_flex(
                    band(
                        BAND_M,
                        Bound::new(violin_of(m), m).push({
                            let mut last = assets_sig(m);
                            move |v: &mut Violin, m| {
                                let sig = assets_sig(m);
                                if sig != last {
                                    *v = violin_of(m);
                                    last = sig;
                                }
                            }
                        }),
                    ),
                    1.0,
                )
                .child_flex(
                    band(
                        BAND_M,
                        framed(
                            1.0,
                            Bound::new(polar_of(m), m).push({
                                // Alarms supply the counts, assets the labels.
                                let mut last = (alarms_sig(m), assets_sig(m));
                                move |p: &mut PolarArea, m| {
                                    let sig = (alarms_sig(m), assets_sig(m));
                                    if sig != last {
                                        *p = polar_of(m);
                                        last = sig;
                                    }
                                }
                            }),
                        ),
                    ),
                    1.0,
                )
                .child_flex(
                    band(
                        BAND_M,
                        framed(
                            1.0,
                            Bound::new(radar_of(m), m).push({
                                let mut last = radar_sig(m);
                                move |r: &mut RadarChart, m| {
                                    let sig = radar_sig(m);
                                    if sig != last {
                                        *r = radar_of(m);
                                        last = sig;
                                    }
                                }
                            }),
                        ),
                    ),
                    1.0,
                ),
        )
}

// ---------------------------------------------------------------------------
// SCHEDULE — two-week maintenance plan + shift log.
// ---------------------------------------------------------------------------

/// `(event, task index)` pairs — deterministic order so `take_clicked`
/// maps back to the schedule entry.
fn week_events(m: &PlantModel) -> Vec<(WeekEvent, usize)> {
    let mut v = Vec::new();
    for (ti, t) in m.schedule.get().iter().enumerate() {
        let end = (t.start_day + t.days).min(7);
        for d in t.start_day..end {
            v.push((
                WeekEvent::all_day(t.title, usize::from(d))
                    .color(CREW_COLORS[t.crew % CREW_COLORS.len()]),
                ti,
            ));
        }
    }
    v
}

/// Schedule signature — WeekView/Gantt gate on it. `crew` + `title`
/// ride along: the Gantt bakes both into its task labels.
fn schedule_sig(m: &PlantModel) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for t in m.schedule.get() {
        h = sig_fold(h, u64::from(t.id));
        h = sig_fold(h, u64::from(t.asset));
        h = sig_fold(h, u64::from(t.start_day));
        h = sig_fold(h, u64::from(t.days));
        h = sig_fold(h, u64::from(t.done));
        h = sig_fold(h, t.crew as u64);
        h = str_fold(h, t.title);
    }
    h
}

fn gantt_of(m: &PlantModel) -> Gantt {
    // One roster fetch — every task's swimlane label reads it.
    let crew = m.crew.get();
    let mut g = Gantt::new().total_days(14.0).label("TWO-WEEK PLAN");
    for t in m.schedule.get() {
        let name = crew.get(t.crew).map(|c| c.name).unwrap_or("—");
        g = g
            .task(
                format!("{} · {}", name, t.title),
                f32::from(t.start_day),
                f32::from(t.days),
            )
            .progress(if t.done { 1.0 } else { 0.0 });
    }
    g
}

fn timeline_of(m: &PlantModel) -> Timeline {
    let items: Vec<TimelineItem> = m
        .shift_log
        .get()
        .iter()
        .map(|e| {
            let dot = if e.pinned {
                TimelineDot::Warning
            } else if e.author == usize::MAX {
                TimelineDot::Accent
            } else {
                TimelineDot::Success
            };
            TimelineItem::new(e.text.clone())
                .subtitle(format!("T+{:03}", e.minute))
                .dot(dot)
        })
        .collect();
    Timeline::new().items(items).pending("shift continues…")
}

fn schedule(m: &PlantModel) -> Flex {
    Flex::column()
        .gap(ZONE_STACK)
        .child(band(
            BAND_L,
            Bound::new(gantt_of(m), m).push({
                // Task labels bake in crew names — gate on both stores.
                let mut last = (schedule_sig(m), crew_sig(m));
                move |g: &mut Gantt, m| {
                    let sig = (schedule_sig(m), crew_sig(m));
                    if sig != last {
                        *g = gantt_of(m);
                        last = sig;
                    }
                }
            }),
        ))
        .child(
            row()
                .child_flex(
                    band(
                        BAND_M,
                        Bound::new(
                            {
                                let mut w = WeekView::new().label("THIS WEEK");
                                for (e, _) in week_events(m) {
                                    w = w.event(e);
                                }
                                w
                            },
                            m,
                        )
                        .pull(|w: &mut WeekView, m| {
                            // Empty-slot click → operator reserves a maint block.
                            if let Some((day, _hour)) = w.take_slot() {
                                let mut s = m.schedule.get();
                                let id = s.iter().map(|t| t.id).max().unwrap_or(0) + 1;
                                s.push(MaintTask {
                                    id,
                                    title: "Operator block",
                                    asset: 3,
                                    start_day: day.min(13) as u8,
                                    days: 1,
                                    done: false,
                                    crew: 0,
                                });
                                m.schedule.set_if_changed(s);
                            }
                            // Event click → toggle the task's done flag.
                            if let Some(i) = w.take_clicked() {
                                if let Some((_, ti)) = week_events(m).get(i) {
                                    let mut s = m.schedule.get();
                                    if let Some(t) = s.get_mut(*ti) {
                                        t.done = !t.done;
                                    }
                                    m.schedule.set_if_changed(s);
                                }
                            }
                        })
                        .push({
                            let mut last = schedule_sig(m);
                            move |w: &mut WeekView, m| {
                                let sig = schedule_sig(m);
                                if sig != last {
                                    let mut nw = WeekView::new().label("THIS WEEK");
                                    for (e, _) in week_events(m) {
                                        nw = nw.event(e);
                                    }
                                    *w = nw;
                                    last = sig;
                                }
                            }
                        }),
                    ),
                    1.0,
                )
                .child_flex(
                    band(
                        BAND_M,
                        Bound::new(timeline_of(m), m).push({
                            let mut last = log_sig(m);
                            move |t: &mut Timeline, m| {
                                let sig = log_sig(m);
                                if sig != last {
                                    *t = timeline_of(m);
                                    last = sig;
                                }
                            }
                        }),
                    ),
                    1.0,
                ),
        )
        .child(
            row()
                .child_flex(
                    band(
                        BAND_S,
                        Bound::new(ProgressBar::new().value(0.0), m).push({
                            // The rendered fraction is itself the signature.
                            let mut last = None;
                            move |p: &mut ProgressBar, m| {
                                let s = m.schedule.get();
                                let done = s.iter().filter(|t| t.done).count();
                                let sig = (done, s.len());
                                if Some(sig) != last {
                                    *p = ProgressBar::new().value(if s.is_empty() {
                                        0.0
                                    } else {
                                        done as f32 / s.len() as f32
                                    });
                                    last = Some(sig);
                                }
                            }
                        }),
                    ),
                    1.0,
                )
                .child_flex(
                    band(
                        BAND_S,
                        framed(
                            1.0,
                            Bound::new(
                                CountdownRing::new(Duration::from_secs(
                                    u64::from(SHIFT_LEN_MIN) * 60,
                                ))
                                .warn_under(Duration::from_secs(30 * 60))
                                .label("SHIFT END"),
                                m,
                            )
                            .push(|c: &mut CountdownRing, m| {
                                let rem = SHIFT_LEN_MIN.saturating_sub(m.shift_minute.get());
                                c.set_remaining(Duration::from_secs(u64::from(rem) * 60));
                                c.set_running(!m.paused.get());
                            }),
                        ),
                    ),
                    1.0,
                )
                .child(
                    Bound::new(Countdown::new(Duration::from_secs(0)).label("NEXT TASK"), m).push(
                        |c: &mut Countdown, m| {
                            // Minutes until the next not-done task's start day
                            // (sim days = 1440 min from shift start).
                            let next = m
                                .schedule
                                .get()
                                .iter()
                                .filter(|t| !t.done)
                                .map(|t| t.start_day)
                                .min();
                            let rem = match next {
                                Some(d) => {
                                    let target = u64::from(d) * 1440;
                                    target
                                        .saturating_sub(u64::from(m.shift_minute.get()))
                                        .min(14 * 1440)
                                }
                                None => 0,
                            };
                            c.reset(Duration::from_secs(rem * 60));
                            c.set_running(!m.paused.get());
                        },
                    ),
                )
                .child(Bound::new(Statistic::new("TASKS DONE", "—"), m).push({
                    let mut last = None;
                    move |s: &mut Statistic, m| {
                        let sig = schedule_sig(m);
                        if Some(sig) != last {
                            let t = m.schedule.get();
                            s.set_value(format!(
                                "{}/{}",
                                t.iter().filter(|t| t.done).count(),
                                t.len()
                            ));
                            last = Some(sig);
                        }
                    }
                })),
        )
}

// ---------------------------------------------------------------------------
// SHIFT CREW — org, presence, and the simulated wall clocks.
// ---------------------------------------------------------------------------

/// One roster subtree. `reports_to` is acyclic by construction —
/// seeds root at the shift lead and no write path adds edges — but
/// `seen` makes the guard explicit: a malformed roster prunes instead
/// of overflowing the stack.
fn org_tree(crew: &[CrewMember], idx: usize, seen: &mut HashSet<usize>) -> OrgNode {
    let me = &crew[idx];
    let mut node = OrgNode::new(me.name, me.role);
    for (i, c) in crew.iter().enumerate() {
        if c.reports_to == Some(idx) && seen.insert(i) {
            node = node.child(org_tree(crew, i, seen));
        }
    }
    node
}

fn org_of(m: &PlantModel) -> OrgChart {
    let crew = m.crew.get();
    let root = match crew.iter().position(|c| c.reports_to.is_none()) {
        Some(i) => i,
        None => return OrgChart::new(OrgNode::new("—", "no crew")).label("shift roster"),
    };
    let mut seen = HashSet::from([root]);
    OrgChart::new(org_tree(&crew, root, &mut seen)).label("shift roster")
}

fn crew(m: &PlantModel) -> Flex {
    Flex::column()
        .gap(ZONE_STACK)
        .child(
            row()
                .child_flex(
                    band(
                        BAND_L,
                        Bound::new(org_of(m), m).push({
                            let mut last = crew_sig(m);
                            move |o: &mut OrgChart, m| {
                                let sig = crew_sig(m);
                                if sig != last {
                                    *o = org_of(m);
                                    last = sig;
                                }
                            }
                        }),
                    ),
                    1.0,
                )
                .child_flex(
                    band(BAND_L, {
                        // Builder shared by the mount and the membership-
                        // change re-seat (a joined member can't be added
                        // by index — the widget needs a rebuild).
                        let build_list = |m: &PlantModel| {
                            let mut l = AttendeeList::new().label("CREW");
                            for c in m.crew.get() {
                                l = l.attendee(
                                    Attendee::new(c.name).status(crew_status(c.presence)),
                                );
                            }
                            l
                        };
                        Bound::new(build_list(m), m)
                            .pull(|l: &mut AttendeeList, m| {
                                // Selecting a member pages them — looped
                                // into the log.
                                if let Some(i) = l.take_selected() {
                                    if let Some(c) = m.crew.get().get(i) {
                                        m.log(usize::MAX, format!("{} paged to control", c.name));
                                    }
                                }
                            })
                            .push({
                                // Roster fold — presence edits re-run the
                                // status pass; identical rosters skip it.
                                // Names can't be rewritten by index, so a
                                // membership or rename change re-seats.
                                let mut last = crew_sig(m);
                                let mut last_names = crew_names_sig(m);
                                move |l: &mut AttendeeList, m| {
                                    let sig = crew_sig(m);
                                    if sig != last {
                                        last = sig;
                                        let crew = m.crew.get();
                                        let names = crew_names_sig(m);
                                        if names != last_names || l.attendee_count() != crew.len() {
                                            last_names = names;
                                            *l = build_list(m);
                                        } else {
                                            for (i, c) in crew.iter().enumerate() {
                                                l.set_status(i, crew_status(c.presence));
                                            }
                                        }
                                    }
                                }
                            })
                    }),
                    1.0,
                ),
        )
        .child(
            strip()
                .child({
                    // Seeded at build — mounting empty measures 0-wide
                    // and the seated members would paint into the
                    // sibling Presence cards until the next relayout.
                    let build = |m: &PlantModel| {
                        let mut g = AvatarGroup::new().max_count(4).size(32.0).label("ON SHIFT");
                        for c in m.crew.get().iter().filter(|c| {
                            matches!(
                                c.presence,
                                crate::domain::Presence::OnShift | crate::domain::Presence::Remote
                            )
                        }) {
                            g = g.member(Avatar::new(c.name).size(32.0));
                        }
                        g
                    };
                    Bound::new(build(m), m).push({
                        let mut last = crew_sig(m);
                        move |g: &mut AvatarGroup, m| {
                            let sig = crew_sig(m);
                            if sig != last {
                                *g = build(m);
                                last = sig;
                            }
                        }
                    })
                })
                .child({
                    let lead = m.crew.get().first().cloned();
                    Bound::new(
                        Presence::new(
                            lead.as_ref().map(|c| c.name).unwrap_or("—"),
                            lead.as_ref()
                                .map(|c| crew_status(c.presence))
                                .unwrap_or_default(),
                        )
                        .status_text(lead.as_ref().map(|c| c.role).unwrap_or("shift lead")),
                        m,
                    )
                    .pull(|p: &mut Presence, m| {
                        if p.take_clicked() {
                            m.log(usize::MAX, "Shift lead summoned to floor");
                        }
                    })
                    .push({
                        let mut last = crew_sig(m);
                        move |p: &mut Presence, m| {
                            let sig = crew_sig(m);
                            if sig != last {
                                if let Some(c) = m.crew.get().first() {
                                    p.set_status(crew_status(c.presence));
                                }
                                last = sig;
                            }
                        }
                    })
                })
                .child({
                    // First remote/offsite member — the remote-contact chip.
                    let remote = m
                        .crew
                        .get()
                        .into_iter()
                        .find(|c| c.presence == crate::domain::Presence::Remote);
                    Bound::new(
                        Presence::new(
                            remote.as_ref().map(|c| c.name).unwrap_or("—"),
                            remote
                                .as_ref()
                                .map(|c| crew_status(c.presence))
                                .unwrap_or_default(),
                        )
                        .status_text(remote.as_ref().map(|c| c.role).unwrap_or("remote")),
                        m,
                    )
                    .pull(|p: &mut Presence, m| {
                        if p.take_clicked() {
                            m.log(usize::MAX, "Remote engineer pinged");
                        }
                    })
                    .push({
                        let mut last = crew_sig(m);
                        move |p: &mut Presence, m| {
                            let sig = crew_sig(m);
                            if sig != last {
                                // No remote member → Offline, not a
                                // stale "Busy" from whoever left last.
                                let st = m
                                    .crew
                                    .get()
                                    .iter()
                                    .find(|c| c.presence == crate::domain::Presence::Remote)
                                    .map(|c| crew_status(c.presence))
                                    .unwrap_or(PresenceStatus::Offline);
                                p.set_status(st);
                                last = sig;
                            }
                        }
                    })
                })
                .child_flex(DummyWidget, 1.0),
        )
        .child(group_label("SHIFT CLOCK — SIM TIME"))
        .child(
            row()
                .child_flex(
                    band(
                        BAND_M,
                        framed(
                            1.0,
                            Bound::new(AnalogClock::new().show_seconds(true), m).push(
                                |c: &mut AnalogClock, m| {
                                    let t = shift_time(m);
                                    c.set_time(t.hour as u8, t.minute as u8, 0);
                                },
                            ),
                        ),
                    ),
                    1.0,
                )
                .child(
                    Bound::new(
                        DigitalClock::new()
                            .time(shift_time(m))
                            .running(false)
                            .label("SHIFT"),
                        m,
                    )
                    .push({
                        // Sim minute edges at 1 Hz — no per-tick re-seat.
                        let mut last = m.shift_minute.get();
                        move |c: &mut DigitalClock, m| {
                            let min = m.shift_minute.get();
                            if min != last {
                                *c = DigitalClock::new()
                                    .time(shift_time(m))
                                    .running(false)
                                    .label("SHIFT");
                                last = min;
                            }
                        }
                    }),
                )
                .child_flex(
                    band(
                        BAND_M,
                        Bound::new(
                            WorldClock::new()
                                .zone("PLANT EAST", -300)
                                .zone("PLANT WEST", -480)
                                .zone("HQ", 60)
                                .label("SITES"),
                            m,
                        )
                        .push(|w: &mut WorldClock, m| w.set_utc(shift_time(m))),
                    ),
                    1.0,
                ),
        )
}

// ---------------------------------------------------------------------------
// SYSTEM — honest diagnostics + the global-state switches/lamps.
// ---------------------------------------------------------------------------

fn system(m: &PlantModel) -> Flex {
    Flex::column()
        .gap(ZONE_STACK)
        .child(
            row()
                .child_flex(
                    band(
                        BAND_S,
                        Bound::new(PerfOverlay::new().label("FRAME MS"), m).push({
                            // Real frame cadence — inter-tick wall interval.
                            let mut last = Instant::now();
                            move |p: &mut PerfOverlay, _m| {
                                let now = Instant::now();
                                p.push_frame(now.duration_since(last).as_secs_f32() * 1000.0);
                                last = now;
                            }
                        }),
                    ),
                    1.0,
                )
                .child_flex(
                    band(
                        BAND_S,
                        Bound::new(
                            LedMatrix::new(16, 5)
                                .on_color([92, 200, 120, 255])
                                .label("CPU METER"),
                            m,
                        )
                        .push({
                            // Last 16 ring samples as column heights —
                            // repaint only when a new sample lands.
                            let mut last = None;
                            move |mx: &mut LedMatrix, m| {
                                let sig = hist_sig(m);
                                if Some(sig) != last {
                                    let h = m.cpu_hist.get();
                                    let tail = &h[h.len().saturating_sub(16)..];
                                    for c in 0..16 {
                                        let v = tail.get(c).copied().unwrap_or(0.0);
                                        let height = (v * 5.0).round() as usize;
                                        for r in 0..5 {
                                            mx.set(c, r, r < height);
                                        }
                                    }
                                    last = Some(sig);
                                }
                            }
                        }),
                    ),
                    1.0,
                )
                .child(
                    Bound::new(Stopwatch::new().running(true).label("LINE RUN"), m)
                        .pull(|s: &mut Stopwatch, m| {
                            // Lap button → lap stamp into the shift log.
                            if let Some(d) = s.take_lapped() {
                                m.log(usize::MAX, format!("lap — {}", Stopwatch::fmt_face(d)));
                            }
                        })
                        .push(|s: &mut Stopwatch, m| {
                            let run = m.line_running.get();
                            if s.is_running() != run {
                                if run {
                                    s.start();
                                } else {
                                    s.stop();
                                }
                            }
                        }),
                ),
        )
        .child(group_label("CONTROLS — WRITE BACK TO THE MODEL"))
        .child(
            strip()
                .child(
                    Bound::new(Switch::new("LINE RUNNING").on(m.line_running.get()), m)
                        .pull(|s: &mut Switch, m| {
                            m.line_running.set_if_changed(s.on);
                        })
                        .push(|s: &mut Switch, m| {
                            if s.on != m.line_running.get() {
                                s.on = m.line_running.get();
                            }
                        }),
                )
                .child(
                    Bound::new(Switch::new("ALERTS").on(m.alerts_on.get()), m)
                        .pull(|s: &mut Switch, m| {
                            m.alerts_on.set_if_changed(s.on);
                        })
                        .push(|s: &mut Switch, m| {
                            if s.on != m.alerts_on.get() {
                                s.on = m.alerts_on.get();
                            }
                        }),
                )
                .child(
                    Bound::new(Switch::new("PAUSED").on(m.paused.get()), m)
                        .pull(|s: &mut Switch, m| {
                            m.paused.set_if_changed(s.on);
                        })
                        .push(|s: &mut Switch, m| {
                            if s.on != m.paused.get() {
                                s.on = m.paused.get();
                            }
                        }),
                )
                .child(
                    Bound::new(Switch::new("REDUCED MOTION").on(m.reduced_motion.get()), m)
                        .pull(|s: &mut Switch, m| {
                            m.reduced_motion.set_if_changed(s.on);
                        })
                        .push(|s: &mut Switch, m| {
                            if s.on != m.reduced_motion.get() {
                                s.on = m.reduced_motion.get();
                            }
                        }),
                )
                .child_flex(DummyWidget, 1.0),
        )
        .child(group_label("LAMPS — GLOBAL STATE"))
        .child(
            strip()
                .child(
                    Bound::new(StatusDot::new("LINE RUN").status(Status::Ok), m).push(
                        |d: &mut StatusDot, m| {
                            d.set_status(if m.line_running.get() {
                                Status::Ok
                            } else {
                                Status::Off
                            });
                        },
                    ),
                )
                .child(
                    Bound::new(StatusDot::new("SAMPLER").status(Status::Ok), m).push(
                        |d: &mut StatusDot, m| {
                            d.set_status(if m.paused.get() {
                                Status::Warning
                            } else {
                                Status::Ok
                            });
                        },
                    ),
                )
                .child(
                    Bound::new(StatusDot::new("ALERTS").status(Status::Info), m).push(
                        |d: &mut StatusDot, m| {
                            d.set_status(if m.alerts_on.get() {
                                Status::Info
                            } else {
                                Status::Off
                            });
                        },
                    ),
                )
                .child_flex(DummyWidget, 1.0),
        )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use martensite::core::Widget;
    use martensite::reactive::Signal;

    fn seeded() -> PlantModel {
        PlantModel::seeded(
            Signal::new(0.42),
            Signal::new(0.63),
            Signal::new(false),
            Signal::new(true),
            Signal::new(String::new()),
        )
    }

    #[test]
    fn pages_are_domain_named() {
        let m = seeded();
        let pages = pages(&m);
        assert!((4..=8).contains(&pages.len()));
        let labels: Vec<&str> = pages.iter().map(|(l, _)| *l).collect();
        assert_eq!(
            labels,
            [
                "TRENDS",
                "INSTRUMENTS",
                "ALARMS",
                "DISTRIBUTIONS",
                "SCHEDULE",
                "CREW",
                "SYSTEM",
            ]
        );
    }

    #[test]
    fn org_chart_survives_cyclic_reports_to() {
        // A `reports_to` cycle must prune, not overflow the stack —
        // the visited set is the guard for the acyclic invariant.
        let m = seeded();
        let mut crew = m.crew.get();
        crew[1].reports_to = Some(2);
        crew[2].reports_to = Some(1);
        m.crew.set(crew);
        // 1↔2 become unreachable and prune: root + members 3,4,5.
        let chart = org_of(&m);
        assert_eq!(chart.node_count(), 4);
    }

    #[test]
    fn gauge_push_reflects_cpu_signal() {
        let cpu = Signal::new(0.35);
        let m = PlantModel::seeded(
            cpu.clone(),
            Signal::new(0.5),
            Signal::new(false),
            Signal::new(true),
            Signal::new(String::new()),
        );
        let mut g = cpu_gauge(&m);
        g.tick(Duration::from_millis(16));
        assert!((g.inner().get_value() - 35.0).abs() < 1e-6);
        cpu.set(0.9);
        g.tick(Duration::from_millis(16));
        assert!((g.inner().get_value() - 90.0).abs() < 1e-6);
    }
}
