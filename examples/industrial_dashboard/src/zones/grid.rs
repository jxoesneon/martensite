//! Process Grid zone pages — the records-and-operations panel's
//! functional surfaces per the Design Council verdict (docket
//! `20260921`): no exhibit walls; every mounted widget reads
//! [`PlantModel`] state (`push`) or publishes operator interaction
//! back (`pull`) through [`Bound`].
//!
//! Pages (domain-named, never widget-named):
//!
//! - **ASSET REGISTRY** — the plant hierarchy (Site→Line→Cell) through
//!   five selector surfaces sharing `selected_asset`, plus the
//!   filtered/paged register table.
//! - **ASSET DETAIL** — the selected asset's record: editable property
//!   grid, live inspector, identity codes, operator note, alarms.
//! - **WORK ORDERS** — the WO pipeline: kanban moves, register table,
//!   status stepper, checklist/crew ops, intake wizard.
//! - **MAINTENANCE** — the two-week plan: gantt/week grid, task-window
//!   pickers writing `MaintTask.start_day`/`days`, milestone rail.
//! - **DOCUMENTS** — the plant record's documents: registry JSON,
//!   firmware blob, alarm journal, recipe revisions, shift log, files.
//! - **DIAGNOSTICS** — the operator console: a real command
//!   interpreter, command palette over plant actions, console
//!   settings, frame instrumentation.
//! - **HIERARCHY** — relational views of real structure: asset graph,
//!   org roster, OEE-weighted sunburst, material flow, alarm
//!   root-cause.
//!
//! Construct-once widgets (`Kanban`, `Ticket`, `Gantt`, `CheckList`, …)
//! have no relabel mutators — their `push` re-seats a fresh widget
//! when the model signature changes (the `*w = …` pattern), so a
//! write from any other surface lands here on the next tick.
//!
//! CUT: `PdfView` (its document seam needs `martensite-pdf` types the
//!      example doesn't depend on; `PdfView::new()` is a blank
//!      placeholder), `ExternalEngine` (a host for external-engine
//!      frames — no engine produces them), `Milestone`/`TreeTable`/
//!      `FilterBar`/`DataTable`/`DateRangePicker` (no such widgets —
//!      `Timeline` dots, `Table`, `SearchField` and
//!      `DatePicker::range_mode` cover the roles). Media widgets
//!      (`MediaView`, `Pip`, `Playlist`, `Filmstrip`, `ImageViewer`,
//!      `Image`, `Lightbox`, `Carousel`, `Coverflow`) belong to the
//!      media panel. Council-cut widgets stay cut.

use std::collections::hash_map::DefaultHasher;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::time::Instant;

use martensite::reactive::Signal;
use martensite::widgets::about::About;
use martensite::widgets::accordion::Accordion;
use martensite::widgets::anchor::{Anchor, AnchorItem};
use martensite::widgets::app_grid::{AppEntry, AppGrid};
use martensite::widgets::attachment::Attachment;
use martensite::widgets::badge::Badge;
use martensite::widgets::barcode::Barcode;
use martensite::widgets::breadcrumb::Breadcrumb;
use martensite::widgets::button::Button;
use martensite::widgets::calendar::{Calendar, CalendarSelection};
use martensite::widgets::card::Card;
use martensite::widgets::card_deck::CardDeck;
use martensite::widgets::cascader::{Cascader, CascaderOption};
use martensite::widgets::check_list::{CheckItem, CheckList};
use martensite::widgets::clamp::Clamp;
use martensite::widgets::clipboard_history::ClipboardHistory;
use martensite::widgets::code_view::CodeView;
use martensite::widgets::command_palette::{CommandAction, CommandPalette};
use martensite::widgets::container::Container;
use martensite::widgets::date_picker::{Date, DatePicker};
use martensite::widgets::descriptions::Descriptions;
use martensite::widgets::diff_view::{DiffKind, DiffView};
use martensite::widgets::disclosure::Disclosure;
use martensite::widgets::dock::{Dock, DockItem};
use martensite::widgets::download_item::{DownloadAction, DownloadItem, DownloadState};
use martensite::widgets::drawer::Drawer;
use martensite::widgets::dropdown::Dropdown;
use martensite::widgets::expander_row::ExpanderRow;
use martensite::widgets::filmstrip::Thumbnail;
use martensite::widgets::fishbone::{Bone, Fishbone};
use martensite::widgets::flex::Flex;
use martensite::widgets::flow_box::{FlowBox, FlowSelection};
use martensite::widgets::gantt::Gantt;
use martensite::widgets::graph_view::GraphView;
use martensite::widgets::grid::{Grid, GridCell};
use martensite::widgets::group_box::GroupBox;
use martensite::widgets::hex_view::HexView;
use martensite::widgets::inline_edit::InlineEdit;
use martensite::widgets::inspector::Inspector;
use martensite::widgets::json_view::{JsonNode, JsonView};
use martensite::widgets::kanban::Kanban;
use martensite::widgets::list_view::{ListView, SelectionMode};
use martensite::widgets::log_view::{LogSeverity, LogView};
use martensite::widgets::markdown::Markdown;
use martensite::widgets::masonry::Masonry;
use martensite::widgets::merge_view::{MergeRow, MergeSide, MergeView};
use martensite::widgets::message_list::{Message, MessageList};
use martensite::widgets::mind_map::MindMap;
use martensite::widgets::nav_rail::NavRail;
use martensite::widgets::nav_stack::NavStack;
use martensite::widgets::org_chart::{OrgChart, OrgNode};
use martensite::widgets::page_header::PageHeader;
use martensite::widgets::pagination::Pagination;
use martensite::widgets::perf_overlay::PerfOverlay;
use martensite::widgets::pips_pager::PipsPager;
use martensite::widgets::progress::ProgressBar;
use martensite::widgets::property_grid::{PropertyGrid, PropertyRow};
use martensite::widgets::pull_to_refresh::PullToRefresh;
use martensite::widgets::qr_code::QrCode;
use martensite::widgets::rating::Rating;
use martensite::widgets::resize_handle::ResizeHandle;
use martensite::widgets::sankey::Sankey;
use martensite::widgets::scroll_indicator::ScrollIndicator;
use martensite::widgets::scrollview::ScrollView;
use martensite::widgets::search_field::SearchField;
use martensite::widgets::segmented::Segmented;
use martensite::widgets::separator::Separator;
use martensite::widgets::settings_row::{SettingsGroup, SettingsRow};
use martensite::widgets::slider::Slider;
use martensite::widgets::split_view::SplitView;
use martensite::widgets::stack::{Stack, StackAlignment};
use martensite::widgets::status_dot::{Status, StatusDot};
use martensite::widgets::steps::Steps;
use martensite::widgets::sunburst::{Sunburst, SunburstNode};
use martensite::widgets::switch::Switch;
use martensite::widgets::table::{Table, TableColumn};
use martensite::widgets::tabs::Tabs;
use martensite::widgets::task_switcher::TaskSwitcher;
use martensite::widgets::terminal::Terminal;
use martensite::widgets::text::Text;
use martensite::widgets::text_area::TextArea;
use martensite::widgets::ticket::Ticket;
use martensite::widgets::timeline::{Timeline, TimelineDot, TimelineItem};
use martensite::widgets::transfer::{MoveDir, Transfer};
use martensite::widgets::tree_select::TreeSelect;
use martensite::widgets::tree_view::{TreeNode, TreeView};
use martensite::widgets::video_grid::{Participant, VideoGrid};
use martensite::widgets::viewport::Viewport;
use martensite::widgets::webview::WebView;
use martensite::widgets::week_view::{WeekEvent, WeekView};
use martensite::widgets::wizard::Wizard;

use crate::domain::{
    AlarmSeverity, Asset, AssetKind, AssetStatus, MaintTask, PlantModel, WoStatus, WorkOrder,
};
use crate::zone::{band, framed, row, strip, Bound, BAND_L, BAND_M, BAND_S, ZONE_GAP, ZONE_STACK};
use martensite::core::widget::DummyWidget;

/// Sim "today" — the schedule's day-0 (docket date, deterministic).
const BASE_DAY: Date = Date {
    year: 2026,
    month: 9,
    day: 21,
};
/// Assets per register page — drives the Pagination strip.
const REGISTRY_PAGE: usize = 5;
/// Day-of-week labels for `WorkOrder.due_day` (0 = Mon).
const DUE_DAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
/// Intake fault kinds — the wizard Segmented's options; each maps
/// to the new WO's priority (a real field write, not decoration).
const INTAKE_FAULTS: [(&str, crate::domain::WoPriority); 4] = [
    ("inspection", crate::domain::WoPriority::Low),
    ("repair", crate::domain::WoPriority::Medium),
    ("overhaul", crate::domain::WoPriority::High),
    ("safety", crate::domain::WoPriority::Critical),
];
/// Status→hue for launcher/selector glyphs.
const STATUS_COLORS: [[u8; 4]; 4] = [
    [92, 200, 120, 255],  // Running — green
    [250, 190, 60, 255],  // Degraded — amber
    [230, 70, 60, 255],   // Down — red
    [140, 140, 220, 255], // Maintenance — blue
];
/// WO priority → card tint.
const PRIORITY_COLORS: [[u8; 4]; 4] = [
    [110, 150, 170, 255], // Low
    [96, 165, 250, 255],  // Medium
    [250, 190, 60, 255],  // High
    [230, 70, 60, 255],   // Critical
];

/// Domain-named zone pages for the Process Grid panel — tab labels
/// are domain names ("WORK ORDERS"), never widget names.
pub fn pages(model: &PlantModel) -> Vec<(&'static str, Flex)> {
    vec![
        ("REGISTRY", registry(model)),
        ("DETAIL", detail(model)),
        ("WORK ORDERS", work_orders(model)),
        ("MAINTENANCE", maintenance(model)),
        ("DOCUMENTS", documents(model)),
        ("DIAGNOSTICS", diagnostics(model)),
        ("HIERARCHY", hierarchy(model)),
    ]
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Recursive Site→Line→Cell tree as `TreeNode`s (label = asset name).
fn asset_tree(m: &PlantModel, parent: Option<u32>) -> Vec<TreeNode> {
    m.asset_children(parent)
        .into_iter()
        .map(|a| {
            let kids = asset_tree(m, Some(a.id));
            if kids.is_empty() {
                TreeNode::new(a.name)
            } else {
                TreeNode::new(a.name).with_children(kids)
            }
        })
        .collect()
}

/// Same hierarchy as `CascaderOption`s — the option *value* is the
/// asset id, so `take_selected` hands back a parseable path.
fn cascader_opts(m: &PlantModel, parent: Option<u32>) -> Vec<CascaderOption> {
    m.asset_children(parent)
        .into_iter()
        .map(|a| {
            let mut o = CascaderOption::new(a.name, a.id.to_string());
            for c in cascader_opts(m, Some(a.id)) {
                o = o.child(c);
            }
            o
        })
        .collect()
}

/// DFS path of child indices to `id` — mirrors `asset_tree`'s order.
fn asset_path(m: &PlantModel, id: u32) -> Option<Vec<usize>> {
    fn walk(m: &PlantModel, parent: Option<u32>, id: u32, trail: &mut Vec<usize>) -> bool {
        for (i, a) in m.asset_children(parent).iter().enumerate() {
            trail.push(i);
            if a.id == id || walk(m, Some(a.id), id, trail) {
                return true;
            }
            trail.pop();
        }
        false
    }
    let mut trail = Vec::new();
    walk(m, None, id, &mut trail).then_some(trail)
}

/// Inverse of [`asset_path`]: child-index path → asset id.
fn path_asset(m: &PlantModel, path: &[usize]) -> Option<u32> {
    let mut parent = None;
    let mut hit = None;
    for &i in path {
        let kids = m.asset_children(parent);
        let a = kids.get(i)?;
        hit = Some(a.id);
        parent = Some(a.id);
    }
    hit
}

/// Ancestry chain root→self for `id` (empty when unknown).
fn ancestry(m: &PlantModel, id: u32) -> Vec<Asset> {
    let mut chain = Vec::new();
    let mut cur = m.asset(id);
    for _ in 0..8 {
        let Some(a) = cur else { break };
        chain.push(a.clone());
        cur = a.parent.and_then(|p| m.asset(p));
    }
    chain.reverse();
    chain
}

/// Selected asset record.
fn sel_asset(m: &PlantModel) -> Option<Asset> {
    m.selected_asset.get().and_then(|id| m.asset(id))
}

/// Selected work order record.
fn sel_wo(m: &PlantModel) -> Option<WorkOrder> {
    let sel = m.selected_wo.get()?;
    m.work_orders.get().into_iter().find(|w| w.id == sel)
}

/// Cell-kind assets in store order — the launchers' entry set.
fn cell_assets(m: &PlantModel) -> Vec<Asset> {
    m.assets
        .get()
        .into_iter()
        .filter(|a| a.kind == AssetKind::Cell)
        .collect()
}

/// Alarms (any state) raised on the selected asset.
fn asset_alarms(m: &PlantModel) -> Vec<crate::domain::Alarm> {
    m.alarms
        .get()
        .into_iter()
        .filter(|a| Some(a.asset) == m.selected_asset.get())
        .collect()
}

/// Mutate one asset in place (the registry's write path). A closure
/// that changes nothing skips the `set` — no dirty churn.
fn set_asset(m: &PlantModel, id: u32, f: impl FnOnce(&mut Asset)) {
    let mut assets = m.assets.get();
    if let Some(a) = assets.iter_mut().find(|a| a.id == id) {
        let before = a.clone();
        f(a);
        if *a != before {
            m.assets.set(assets);
        }
    }
}

/// Mutate one work order by id (Checklist/crew writes target a fixed
/// WO, not necessarily the selected one). No-op closures skip `set`.
fn update_wo_id(m: &PlantModel, id: u32, f: impl FnOnce(&mut WorkOrder)) {
    let mut wos = m.work_orders.get();
    if let Some(w) = wos.iter_mut().find(|w| w.id == id) {
        let before = w.clone();
        f(w);
        if *w != before {
            m.work_orders.set(wos);
        }
    }
}

/// Mutate one maintenance task by id. No-op closures skip `set`.
fn update_task(m: &PlantModel, id: u32, f: impl FnOnce(&mut MaintTask)) {
    let mut s = m.schedule.get();
    if let Some(t) = s.iter_mut().find(|t| t.id == id) {
        let before = t.clone();
        f(t);
        if *t != before {
            m.schedule.set(s);
        }
    }
}

/// `AssetStatus` → status-lamp severity.
fn status_lamp(s: AssetStatus) -> Status {
    match s {
        AssetStatus::Running => Status::Ok,
        AssetStatus::Degraded => Status::Warning,
        AssetStatus::Down => Status::Error,
        AssetStatus::Maintenance => Status::Info,
    }
}

/// `AssetStatus` → launcher glyph color.
fn status_color(s: AssetStatus) -> [u8; 4] {
    match s {
        AssetStatus::Running => STATUS_COLORS[0],
        AssetStatus::Degraded => STATUS_COLORS[1],
        AssetStatus::Down => STATUS_COLORS[2],
        AssetStatus::Maintenance => STATUS_COLORS[3],
    }
}

/// Change-detection signature for the asset store — `push` closures
/// compare it before re-seating construct-once widgets. A folded
/// `u64` over every field's primitive bits — no `format!`, no
/// allocation beyond the one `Signal::get` clone.
fn assets_sig(m: &PlantModel) -> u64 {
    let mut h = DefaultHasher::new();
    for a in m.assets.get().iter() {
        a.id.hash(&mut h);
        a.parent.hash(&mut h);
        h.write_u8(a.kind as u8);
        a.name.hash(&mut h);
        h.write_u8(a.status as u8);
        h.write_u64(a.oee.to_bits());
        a.serial.hash(&mut h);
        h.write_u16(a.installed);
        a.note.hash(&mut h);
    }
    h.finish()
}

/// Same for the WO store — includes checklist + note state.
fn wos_sig(m: &PlantModel) -> u64 {
    let mut h = DefaultHasher::new();
    for w in m.work_orders.get().iter() {
        w.id.hash(&mut h);
        w.title.hash(&mut h);
        w.asset.hash(&mut h);
        h.write_u8(w.status as u8);
        h.write_u8(w.priority as u8);
        h.write_usize(w.assignee);
        w.checklist.hash(&mut h);
        h.write_u8(w.due_day);
        h.write_u64(w.progress.to_bits());
        w.notes.hash(&mut h);
    }
    h.finish()
}

/// Same for the maintenance schedule.
fn sched_sig(m: &PlantModel) -> u64 {
    let mut h = DefaultHasher::new();
    for t in m.schedule.get().iter() {
        t.id.hash(&mut h);
        t.title.hash(&mut h);
        t.asset.hash(&mut h);
        h.write_u8(t.start_day);
        h.write_u8(t.days);
        t.done.hash(&mut h);
        h.write_usize(t.crew);
    }
    h.finish()
}

/// Same for the alarm store.
fn alarm_sig(m: &PlantModel) -> u64 {
    let mut h = DefaultHasher::new();
    for a in m.alarms.get().iter() {
        a.id.hash(&mut h);
        a.asset.hash(&mut h);
        h.write_u8(a.severity as u8);
        a.message.hash(&mut h);
        a.active.hash(&mut h);
        a.acked.hash(&mut h);
        h.write_u32(a.raised_min);
    }
    h.finish()
}

/// Same for the material-flow dataset (Sankey links).
fn flow_sig(m: &PlantModel) -> u64 {
    let mut h = DefaultHasher::new();
    for (f, t, v) in m.material_flow.get() {
        f.hash(&mut h);
        t.hash(&mut h);
        h.write_u64(v.to_bits());
    }
    h.finish()
}

/// Same for crew presence — the board only repaints on presence
/// moves, so names/rooms stay out of the fold.
fn presence_sig(m: &PlantModel) -> u64 {
    let mut h = DefaultHasher::new();
    for c in m.crew.get().iter() {
        h.write_u8(c.presence as u8);
    }
    h.finish()
}

/// Crew-structure signature — OrgChart roster shape (name, role,
/// `reports_to` edges, room).
fn crew_sig(m: &PlantModel) -> u64 {
    let mut h = DefaultHasher::new();
    for c in m.crew.get().iter() {
        c.name.hash(&mut h);
        c.role.hash(&mut h);
        h.write_u8(c.presence as u8);
        h.write_u8(c.room as u8);
        c.reports_to.map(|i| i as u64).hash(&mut h);
    }
    h.finish()
}

/// "WO-4471 housing" → 4471 (kanban/deck card titles carry the id).
fn wo_id(title: &str) -> Option<u32> {
    title
        .strip_prefix("WO-")?
        .split(|c: char| !c.is_ascii_digit())
        .next()?
        .parse()
        .ok()
}

/// Next WO id for intake writes.
fn next_wo_id(m: &PlantModel) -> u32 {
    m.work_orders
        .get()
        .iter()
        .map(|w| w.id)
        .max()
        .unwrap_or(4470)
        + 1
}

/// Pending WO intake — the wizard's step controls collect these
/// values; `create_wo` consumes them. `Default` is the quick-create
/// path (hero CTA, console palette) and preserves the seeded shape.
#[derive(Clone, Copy)]
struct WoIntake {
    /// Asset the order covers (`None` → the selected asset).
    asset: Option<u32>,
    /// `INTAKE_FAULTS` index → title word + priority.
    fault: usize,
    /// Crew index → `assignee`.
    crew: usize,
    /// Due day-of-week (0 = Mon).
    due: u8,
}

impl Default for WoIntake {
    fn default() -> Self {
        Self {
            asset: None,
            fault: 1, // repair → Medium, the seeded default
            crew: 0,
            due: 2,
        }
    }
}

/// Append a freshly-created work order (HeroHeader/Wizard/console).
fn create_wo(m: &PlantModel, intake: WoIntake) {
    let id = next_wo_id(m);
    let (fault, priority) = INTAKE_FAULTS[intake.fault.min(INTAKE_FAULTS.len() - 1)];
    let mut wos = m.work_orders.get();
    wos.push(WorkOrder {
        id,
        // `title` is `&'static` seed data — intake titles leak a
        // short string per created WO (bounded; dogfood sim).
        title: Box::leak(format!("WO-{id} {fault}").into_boxed_str()),
        asset: intake.asset.or_else(|| m.selected_asset.get()).unwrap_or(1),
        status: WoStatus::Queued,
        priority,
        assignee: intake.crew.min(m.crew.get().len().saturating_sub(1)),
        checklist: vec![("Scope fault", false), ("Schedule crew", false)],
        due_day: intake.due.min(6),
        progress: 0.0,
        notes: String::new(),
    });
    m.work_orders.set(wos);
    m.selected_wo.set(Some(id));
    m.log(usize::MAX, format!("WO-{id} created — queued"));
}

/// Civil day number (Hinnant) — Date↔schedule-offset conversion.
fn daynum(d: Date) -> i64 {
    let y = if d.month <= 2 { d.year - 1 } else { d.year } as i64;
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (d.month as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d.day as i64 - 1;
    era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468
}

/// Inverse of [`daynum`].
fn date_from_daynum(z: i64) -> Date {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    Date {
        year: (if mp < 10 { y } else { y + 1 }) as i32,
        month: (if mp < 10 { mp + 3 } else { mp - 9 }) as u32,
        day: (doy - (153 * mp + 2) / 5 + 1) as u32,
    }
}

/// Schedule offset → calendar date.
fn day_date(offset: i64) -> Date {
    date_from_daynum(daynum(BASE_DAY) + offset)
}

/// A task's `[start, start+days)` window as a calendar range.
fn task_range(t: &MaintTask) -> (Date, Date) {
    (
        day_date(i64::from(t.start_day)),
        day_date(i64::from(t.start_day) + i64::from(t.days) - 1),
    )
}

/// Deterministic module matrix for an asset tag — finder squares
/// plus payload bits derived from the serial's bytes (the facade
/// renders the tag's pattern; not a standards QR encoder).
fn qr_modules(serial: &str) -> Vec<Vec<bool>> {
    const N: usize = 21;
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut m = vec![vec![false; N]; N];
    for (r, row) in m.iter_mut().enumerate() {
        for (c, cell) in row.iter_mut().enumerate() {
            for &b in serial.as_bytes() {
                hash = (hash ^ u64::from(b)).wrapping_mul(0x100_0000_01b3);
            }
            hash ^= (r * N + c) as u64;
            *cell = hash % 7 < 3;
            hash = hash.rotate_left(13);
        }
    }
    let finder = |m: &mut [Vec<bool>], y: usize, x: usize| {
        for r in 0..7 {
            for c in 0..7 {
                let edge = r == 0 || r == 6 || c == 0 || c == 6;
                let core = (2..=4).contains(&r) && (2..=4).contains(&c);
                m[y + r][x + c] = edge || core;
            }
        }
    };
    finder(&mut m, 0, 0);
    finder(&mut m, 0, N - 7);
    finder(&mut m, N - 7, 0);
    m
}

/// Firmware blob for an asset — 64 bytes synthesized from its serial
/// (header magic + serial bytes + running checksum), shown by HexView.
fn fw_blob(serial: &str) -> Vec<u8> {
    // A blanked serial (the PropertyGrid row writes "" through) has
    // no bytes to mix — emit a zeroed image, not an empty-slice index.
    if serial.is_empty() {
        return vec![0u8; 64];
    }
    let mut b = b"MSFW".to_vec();
    let mut sum: u8 = 0xA5;
    for i in 0..60 {
        let s = serial.as_bytes()[i % serial.len()];
        sum = sum.wrapping_add(s).rotate_left(1);
        b.push(s ^ sum ^ (i as u8));
    }
    b
}

/// Sim-clock label "HH:MM" for a shift minute (shift starts 06:00).
fn shift_hhmm(minute: u32) -> String {
    format!("{:02}:{:02}", (6 + minute / 60) % 24, minute % 60)
}

/// Filtered registry ids — the unpaged id list the kind Segmented +
/// `filter_text` produce (order = `assets` store order).
fn registry_filtered(m: &PlantModel, kind: usize) -> Vec<u32> {
    let f = m.filter_text.get().to_lowercase();
    m.assets
        .get()
        .into_iter()
        .filter(|a| match kind {
            1 => a.kind == AssetKind::Site,
            2 => a.kind == AssetKind::Line,
            3 => a.kind == AssetKind::Cell,
            _ => true,
        })
        .filter(|a| {
            f.is_empty()
                || a.name.to_lowercase().contains(&f)
                || a.serial.to_lowercase().contains(&f)
                || a.status.label().to_lowercase().contains(&f)
        })
        .map(|a| a.id)
        .collect()
}

/// One page of [`registry_filtered`] — the Table/Pagination view.
fn registry_ids(m: &PlantModel, kind: usize, page: usize) -> Vec<u32> {
    let v = registry_filtered(m, kind);
    let start = (page * REGISTRY_PAGE).min(v.len());
    v[start..(start + REGISTRY_PAGE).min(v.len())].to_vec()
}

// ---------------------------------------------------------------------------
// ASSET REGISTRY — every selector writes `selected_asset`; the table
// is filtered by `filter_text`, kind Segmented, and the page strip.
// ---------------------------------------------------------------------------

fn registry(m: &PlantModel) -> Flex {
    // View-state signals shared between selector chrome and the
    // register views (the toolbar's outcome-signal pattern, local to
    // this page).
    let kind_sel = Signal::new(0usize);
    let page_sel = Signal::new(0usize);
    let split = Signal::new(0.42f32);

    // --- selector chrome -------------------------------------------------
    let search = {
        let mut last_push = m.filter_text.get();
        Bound::new(
            SearchField::new()
                .placeholder("filter assets — name · serial · status")
                .label("registry filter"),
            m,
        )
        .pull(|w: &mut SearchField, m| {
            // `take_edited`/`take_submitted` are themselves the change
            // signals — no dedup needed on this side.
            if w.take_edited() {
                m.filter_text.set_if_changed(w.value().to_string());
            }
            if let Some(q) = w.take_submitted() {
                m.filter_text.set_if_changed(q);
            }
        })
        .push(move |w: &mut SearchField, m| {
            let v = m.filter_text.get();
            if v != last_push {
                last_push = v.clone();
                if w.value() != v {
                    w.set_value(v);
                }
            }
        })
    };

    const SITES: [&str; 3] = ["All sites", "Plant East", "Plant West"];
    let site = {
        let idx = |s: &str| match s {
            "Plant East" => 1,
            "Plant West" => 2,
            _ => 0,
        };
        let mut dd = Dropdown::new(SITES).label("site");
        dd.commit(idx(&m.site_filter.get()));
        let mut last = dd.selected();
        Bound::new(dd, m)
            .pull(move |w: &mut Dropdown, m| {
                let i = w.selected();
                if i != last {
                    last = i;
                    m.site_filter.set_if_changed(SITES[i].to_string());
                }
            })
            .push(move |w: &mut Dropdown, m| {
                let want = idx(&m.site_filter.get());
                if want != last && !w.is_open() {
                    last = want;
                    w.commit(want);
                }
            })
    };

    let cascade = {
        let mut last_sig = assets_sig(m);
        Bound::new(
            Cascader::new()
                .options(cascader_opts(m, None))
                .placeholder("site ▸ line ▸ cell")
                .label("asset path"),
            m,
        )
        .pull(|w: &mut Cascader, m| {
            if let Some(path) = w.take_selected() {
                if let Some(id) = path.last().and_then(|v| v.parse::<u32>().ok()) {
                    m.selected_asset.set_if_changed(Some(id));
                }
            }
        })
        .push(move |w: &mut Cascader, m| {
            // Options are construct-time — re-seat on asset edits.
            let s = assets_sig(m);
            if s != last_sig {
                last_sig = s;
                *w = Cascader::new()
                    .options(cascader_opts(m, None))
                    .placeholder("site ▸ line ▸ cell")
                    .label("asset path");
            }
        })
    };

    let tree_sel = {
        let mut last = m.selected_asset.get();
        let mut last_sig = assets_sig(m);
        Bound::new(
            TreeSelect::new()
                .tree(asset_tree(m, None))
                .placeholder("jump to asset…")
                .label("asset"),
            m,
        )
        .pull(|w: &mut TreeSelect, m| {
            if let Some(name) = w.take_selected() {
                if let Some(a) = m.assets.get().iter().find(|a| a.name == name) {
                    m.selected_asset.set_if_changed(Some(a.id));
                }
            }
        })
        .push(move |w: &mut TreeSelect, m| {
            let s = assets_sig(m);
            if s != last_sig && !w.is_open() {
                last_sig = s;
                *w = TreeSelect::new()
                    .tree(asset_tree(m, None))
                    .placeholder("jump to asset…")
                    .label("asset");
                last = m.selected_asset.get();
                w.set_selected(last.map(|id| m.asset_name(id).to_string()));
                return;
            }
            let sel = m.selected_asset.get();
            if sel != last && !w.is_open() {
                last = sel;
                w.set_selected(sel.map(|id| m.asset_name(id).to_string()));
            }
        })
    };

    // --- view strip ------------------------------------------------------
    let kind = {
        let sig = kind_sel.clone();
        Bound::new(
            Segmented::new()
                .options(["All", "Sites", "Lines", "Cells"])
                .selected(0)
                .label("kind filter"),
            m,
        )
        .pull(move |w: &mut Segmented, _m| {
            if let Some(i) = w.take_selected() {
                sig.set_if_changed(i);
            }
        })
    };

    // Pagination — `total_pages` is construct-time, so the strip is
    // re-seated when the filtered count moves the page count; the
    // current page clamps into range via `page_sel`.
    let pager = {
        let sig_pull = page_sel.clone();
        let sig_push = page_sel.clone();
        let ks = kind_sel.clone();
        let ks_key = kind_sel.clone();
        let total_of = move |m: &PlantModel| {
            registry_filtered(m, ks.get())
                .len()
                .div_ceil(REGISTRY_PAGE)
                .clamp(1, crate::zone::MAX_SELECTOR_ENTRIES)
        };
        // (kind, filter, assets) — the filtered count's full input
        // set; `total_of` only reruns when it moves.
        let key_of = move |m: &PlantModel| (ks_key.get(), m.filter_text.get(), assets_sig(m));
        let mut last_p = 0usize;
        let mut last_t = total_of(m);
        let mut last_key = key_of(m);
        Bound::new(Pagination::new().total_pages(last_t).current(0), m)
            .pull(move |w: &mut Pagination, _m| {
                if let Some(p) = w.take_selected() {
                    sig_pull.set_if_changed(p);
                }
            })
            .push(move |w: &mut Pagination, m| {
                let key = key_of(m);
                let t = if key != last_key {
                    last_key = key;
                    total_of(m)
                } else {
                    last_t
                };
                let p = sig_push.get().min(t - 1);
                sig_push.set_if_changed(p);
                if t != last_t {
                    last_t = t;
                    last_p = p;
                    *w = Pagination::new().total_pages(t).current(p);
                } else if p != last_p {
                    last_p = p;
                    w.set_current(p);
                }
            })
    };

    // Breadcrumb — ancestry of the selected asset; clicking a segment
    // selects that ancestor. Re-seated on selection change (segments
    // have no mutator).
    let crumb = {
        let mut last = (m.selected_asset.get(), assets_sig(m));
        let build = |m: &PlantModel| {
            Breadcrumb::new().segments(
                ancestry(m, m.selected_asset.get().unwrap_or(1))
                    .iter()
                    .map(|a| a.name),
            )
        };
        Bound::new(build(m), m)
            .pull(|w: &mut Breadcrumb, m| {
                if let Some(i) = w.take_navigated() {
                    let anc = ancestry(m, m.selected_asset.get().unwrap_or(1));
                    if let Some(a) = anc.get(i) {
                        m.selected_asset.set_if_changed(Some(a.id));
                    }
                }
            })
            .push(move |w: &mut Breadcrumb, m| {
                // Selection moves or a name edit both re-seat segments.
                let sig = (m.selected_asset.get(), assets_sig(m));
                if sig != last {
                    last = sig;
                    *w = build(m);
                }
            })
    };

    // NavRail — the two sites as destinations.
    let site_ids: Vec<u32> = m
        .assets
        .get()
        .iter()
        .filter(|a| a.kind == AssetKind::Site)
        .map(|a| a.id)
        .collect();
    let rail = {
        let ids = site_ids.clone();
        let ids2 = site_ids.clone();
        let mut last_sig = assets_sig(m);
        let build = |m: &PlantModel| {
            let mut r = NavRail::new();
            for a in m.assets.get().iter().filter(|a| a.kind == AssetKind::Site) {
                r = r.destination("▦", a.name);
            }
            r
        };
        Bound::new(build(m), m)
            .pull(move |w: &mut NavRail, m| {
                if let Some(i) = w.take_activated() {
                    if let Some(id) = ids.get(i) {
                        m.selected_asset.set_if_changed(Some(*id));
                    }
                }
            })
            .push(move |w: &mut NavRail, m| {
                let s = assets_sig(m);
                if s != last_sig {
                    last_sig = s;
                    *w = build(m);
                }
                let sel = m
                    .selected_asset
                    .get()
                    .and_then(|id| ids2.iter().position(|s| *s == id));
                if w.selected_index() != sel {
                    w.set_selected(sel);
                }
            })
    };

    let badge = {
        let mut last = m.assets.get().len();
        Bound::new(
            Badge::wrap(Text::new("assets on record")).with_count(last as u32),
            m,
        )
        .push(move |w: &mut Badge, m| {
            let n = m.assets.get().len();
            if n != last {
                last = n;
                *w = Badge::wrap(Text::new("assets on record")).with_count(n as u32);
            }
        })
    };

    // --- main surface: hierarchy | register ------------------------------
    let tree = Bound::new(
        TreeView::new()
            .roots(asset_tree(m, None))
            .label("plant hierarchy"),
        m,
    )
    .pull({
        // Widget-side tracking — selection changes write
        // `selected_asset`; a reselect of the same path is a no-op.
        let mut last_path: Option<Vec<usize>> = None;
        move |w: &mut TreeView, m| {
            w.take_activated(); // activation == selection here; drain
            let cur = w.selected_path().map(|p| p.to_vec());
            if cur != last_path {
                last_path = cur.clone();
                m.selected_asset
                    .set_if_changed(cur.as_deref().and_then(|p| path_asset(m, p)));
            }
        }
    })
    .push({
        // Model-side tracking — external selection mirrors back;
        // asset edits (name/status) re-seat the roots.
        let mut last_sel = m.selected_asset.get();
        let mut last_sig = assets_sig(m);
        move |w: &mut TreeView, m| {
            let sel = m.selected_asset.get();
            let s = assets_sig(m);
            if s != last_sig {
                last_sig = s;
                w.set_roots(asset_tree(m, None));
                if let Some(p) = sel.and_then(|id| asset_path(m, id)) {
                    w.select_path(&p);
                }
            } else if sel != last_sel {
                match sel.and_then(|id| asset_path(m, id)) {
                    Some(p) => w.select_path(&p),
                    None => w.clear_selection(),
                }
            }
            last_sel = sel;
        }
    });

    let table = {
        let ks = kind_sel.clone();
        let ps = page_sel.clone();
        let ks2 = kind_sel.clone();
        let ps2 = page_sel.clone();
        // The paged view is pure over (kind, page, filter, assets) —
        // both halves key on that tuple instead of recomputing
        // `registry_ids` every tick.
        let mut last_view: Vec<u32> = registry_ids(m, 0, 0);
        let mut last_key = (
            kind_sel.get(),
            page_sel.get(),
            m.filter_text.get(),
            assets_sig(m),
        );
        let mut last_sel = m.selected_asset.get();
        Bound::new(asset_table(m, &last_view), m)
            .pull({
                // Row→id resolution needs the view only when the row
                // or the key moved — `selected()` is sticky state,
                // not an event, so it can't gate alone.
                let mut last_map: Option<(Option<usize>, usize, usize, String, u64)> = None;
                move |w: &mut Table, m| {
                    // `take_activated` for double-click commit;
                    // `selected()` polling covers plain clicks.
                    let cur = w.take_activated().or_else(|| w.selected());
                    if cur.is_none() && last_map.is_none() {
                        return; // never selected — nothing to drain
                    }
                    let key = (cur, ks.get(), ps.get(), m.filter_text.get(), assets_sig(m));
                    if last_map.as_ref() == Some(&key) {
                        return;
                    }
                    last_map = Some(key);
                    if let Some(id) =
                        cur.and_then(|i| registry_ids(m, ks.get(), ps.get()).get(i).copied())
                    {
                        m.selected_asset.set_if_changed(Some(id));
                    }
                }
            })
            .push(move |w: &mut Table, m| {
                let key = (ks2.get(), ps2.get(), m.filter_text.get(), assets_sig(m));
                if key != last_key {
                    last_key = key;
                    last_view = registry_ids(m, ks2.get(), ps2.get());
                    *w = asset_table(m, &last_view);
                }
                let sel = m.selected_asset.get();
                if sel != last_sel {
                    last_sel = sel;
                    match sel.and_then(|id| last_view.iter().position(|v| *v == id)) {
                        Some(i) => w.select(i),
                        None => w.clear_selection(),
                    }
                }
            })
    };

    let split_view = {
        let sig = split.clone();
        let sig2 = split.clone();
        Bound::new(
            SplitView::horizontal()
                .first(tree)
                .second(table)
                .ratio(split.get()),
            m,
        )
        .pull(move |w: &mut SplitView, _m| {
            if let Some(r) = w.take_moved() {
                sig.set_if_changed(r.clamp(0.15, 0.85));
            }
        })
        .push(move |w: &mut SplitView, _m| {
            let r = sig2.get();
            if (w.get_ratio() - r).abs() > 1e-3 {
                w.set_ratio(r);
            }
        })
    };

    let handle = {
        let sig = split.clone();
        Bound::new(ResizeHandle::horizontal().label("resize split"), m).pull(
            move |w: &mut ResizeHandle, _m| {
                if let Some(dx) = w.take_moved() {
                    // px delta → ratio nudge against a nominal lane.
                    sig.set((sig.get() + dx / 640.0).clamp(0.15, 0.85));
                }
                if w.take_reset() {
                    sig.set(0.42);
                }
            },
        )
    };

    // --- secondary launchers ---------------------------------------------
    let list = {
        let ks = kind_sel.clone();
        let ks2 = kind_sel.clone();
        // Same pure-key gating as the register table — the filtered
        // id set moves only with (kind, filter, assets).
        let mut last_ids = registry_filtered(m, kind_sel.get());
        let mut last_key = (kind_sel.get(), m.filter_text.get(), assets_sig(m));
        Bound::new(
            ListView::new()
                .items(registry_list_items(m, &last_ids))
                .selection_mode(SelectionMode::Single)
                .label("registry list"),
            m,
        )
        .pull({
            let mut last_map: Option<(Option<usize>, usize, String, u64)> = None;
            move |w: &mut ListView, m| {
                let cur = w.take_activated().or_else(|| w.selected());
                if cur.is_none() && last_map.is_none() {
                    return;
                }
                let key = (cur, ks.get(), m.filter_text.get(), assets_sig(m));
                if last_map.as_ref() == Some(&key) {
                    return;
                }
                last_map = Some(key);
                if let Some(id) = cur.and_then(|i| registry_filtered(m, ks.get()).get(i).copied()) {
                    m.selected_asset.set_if_changed(Some(id));
                }
            }
        })
        .push(move |w: &mut ListView, m| {
            let key = (ks2.get(), m.filter_text.get(), assets_sig(m));
            if key != last_key {
                last_key = key;
                last_ids = registry_filtered(m, ks2.get());
                w.set_items(registry_list_items(m, &last_ids));
            }
            if let Some(i) = m
                .selected_asset
                .get()
                .and_then(|id| last_ids.iter().position(|v| *v == id))
            {
                w.set_selected(i);
            }
        })
    };

    let refresh = Bound::new(
        PullToRefresh::new(list).label("pull to re-read the registry"),
        m,
    )
    .pull(|w: &mut PullToRefresh, m| {
        if w.take_refresh() {
            m.log(usize::MAX, "operator refreshed the asset registry");
        }
    });

    let apps = {
        let ids: Vec<u32> = cell_assets(m).iter().map(|a| a.id).collect();
        let mut g = AppGrid::new().page_size(4, 2).label("cell stations");
        for a in cell_assets(m) {
            g = g.app(AppEntry::new(a.name, status_color(a.status)));
        }
        let mut sig = assets_sig(m);
        Bound::new(g, m)
            .pull(move |w: &mut AppGrid, m| {
                if let Some(i) = w.take_activated() {
                    if let Some(id) = ids.get(i) {
                        m.selected_asset.set_if_changed(Some(*id));
                    }
                }
            })
            .push(move |w: &mut AppGrid, m| {
                let s = assets_sig(m);
                if s != sig {
                    sig = s;
                    let mut g = AppGrid::new().page_size(4, 2).label("cell stations");
                    for a in m
                        .assets
                        .get()
                        .into_iter()
                        .filter(|a| a.kind == AssetKind::Cell)
                    {
                        g = g.app(AppEntry::new(a.name, status_color(a.status)));
                    }
                    *w = g;
                }
            })
    };

    let dock = {
        let lines: Vec<Asset> = m
            .assets
            .get()
            .into_iter()
            .filter(|a| a.kind == AssetKind::Line)
            .collect();
        let ids: Vec<u32> = lines.iter().map(|a| a.id).collect();
        let build = |m: &PlantModel| {
            let mut d = Dock::new().label("lines");
            for a in m
                .assets
                .get()
                .into_iter()
                .filter(|a| a.kind == AssetKind::Line)
            {
                d = d.item(
                    DockItem::new(a.name, status_color(a.status))
                        .running(a.status == AssetStatus::Running),
                );
            }
            d
        };
        let mut sig = assets_sig(m);
        Bound::new(build(m), m)
            .pull(move |w: &mut Dock, m| {
                if let Some(i) = w.take_launched() {
                    if let Some(id) = ids.get(i) {
                        m.selected_asset.set_if_changed(Some(*id));
                    }
                }
            })
            .push(move |w: &mut Dock, m| {
                let s = assets_sig(m);
                if s != sig {
                    sig = s;
                    *w = {
                        let mut d = Dock::new().label("lines");
                        for a in m
                            .assets
                            .get()
                            .into_iter()
                            .filter(|a| a.kind == AssetKind::Line)
                        {
                            d = d.item(
                                DockItem::new(a.name, status_color(a.status))
                                    .running(a.status == AssetStatus::Running),
                            );
                        }
                        d
                    };
                }
            })
    };

    let flow = {
        let ids: Vec<u32> = cell_assets(m).iter().map(|a| a.id).collect();
        let ids2 = ids.clone();
        let build = |m: &PlantModel| {
            let mut fb = FlowBox::new()
                .gap(6.0)
                .selection_mode(FlowSelection::Single)
                .label("cells");
            for a in cell_assets(m) {
                fb = fb.child(Text::new(a.name).font_size(11.0));
            }
            fb
        };
        let mut last_sig = assets_sig(m);
        Bound::new(build(m), m)
            .pull(move |w: &mut FlowBox, m| {
                if let Some(i) = w.take_selected() {
                    if let Some(id) = ids.get(i) {
                        m.selected_asset.set_if_changed(Some(*id));
                    }
                }
            })
            .push(move |w: &mut FlowBox, m| {
                let s = assets_sig(m);
                if s != last_sig {
                    last_sig = s;
                    *w = build(m);
                }
                if let Some(i) = m
                    .selected_asset
                    .get()
                    .and_then(|id| ids2.iter().position(|v| *v == id))
                {
                    if w.selected_index() != Some(i) {
                        w.select(i);
                    }
                }
            })
    };

    // NavStack — root is the selected asset's site; pushes mirror the
    // ancestry chain. Navigating back re-selects the ancestor.
    let nav = {
        let mut last = (m.selected_asset.get(), assets_sig(m));
        let build = |m: &PlantModel| {
            let anc = ancestry(m, m.selected_asset.get().unwrap_or(1));
            let mut nav = NavStack::new(Text::new(anc.first().map(|a| a.name).unwrap_or("plant")))
                .title(anc.first().map(|a| a.name).unwrap_or("plant"));
            for a in anc.iter().skip(1) {
                nav.push(Text::new(a.name).font_size(11.0), a.name);
            }
            nav
        };
        Bound::new(build(m), m)
            .pull(|w: &mut NavStack, m| {
                if let Some((depth, _)) = w.take_navigated() {
                    let anc = ancestry(m, m.selected_asset.get().unwrap_or(1));
                    if let Some(a) = anc.get(depth) {
                        m.selected_asset.set_if_changed(Some(a.id));
                    }
                }
            })
            .push(move |w: &mut NavStack, m| {
                // Selection moves or a name edit both re-seat the stack.
                let sig = (m.selected_asset.get(), assets_sig(m));
                if sig != last {
                    last = sig;
                    *w = {
                        let anc = ancestry(m, m.selected_asset.get().unwrap_or(1));
                        let mut nav = NavStack::new(Text::new(
                            anc.first().map(|a| a.name).unwrap_or("plant"),
                        ))
                        .title(anc.first().map(|a| a.name).unwrap_or("plant"));
                        for a in anc.iter().skip(1) {
                            nav.push(Text::new(a.name).font_size(11.0), a.name);
                        }
                        nav
                    };
                }
            })
    };

    // Registry summary — kind counts + plant OEE, re-seated on edits.
    let summary = {
        let build = |m: &PlantModel| {
            let a = m.assets.get();
            let n = |k: AssetKind| a.iter().filter(|x| x.kind == k).count();
            Descriptions::new()
                .title("Plant registry")
                .item("Sites", n(AssetKind::Site).to_string())
                .item("Lines", n(AssetKind::Line).to_string())
                .item("Cells", n(AssetKind::Cell).to_string())
                .item("Plant OEE", format!("{:.0}%", m.plant_oee() * 100.0))
                .bordered(true)
                .column_count(2)
        };
        let mut last = assets_sig(m);
        Bound::new(build(m), m).push(move |w: &mut Descriptions, m| {
            let s = assets_sig(m);
            if s != last {
                last = s;
                *w = build(m);
            }
        })
    };

    Flex::column()
        .gap(ZONE_STACK)
        .child(
            strip()
                .child(search)
                .child(site)
                .child(cascade)
                .child_flex(DummyWidget, 1.0),
        )
        .child(
            strip()
                .child(kind)
                .child(tree_sel)
                .child(crumb)
                .child_flex(DummyWidget, 1.0),
        )
        .child(
            strip()
                .child(pager)
                .child(rail)
                .child(badge)
                .child_flex(DummyWidget, 1.0),
        )
        .child(Separator::horizontal())
        .child(
            row()
                .child_flex(band(BAND_L, split_view), 1.0)
                .child(handle),
        )
        .child(
            row()
                .child_flex(band(BAND_L, refresh), 2.0)
                .child_flex(band(BAND_L, apps), 1.0)
                .child(dock),
        )
        .child(
            row()
                .child_flex(band(BAND_M, flow), 1.0)
                .child_flex(band(BAND_M, nav), 1.0)
                .child_flex(band(BAND_M, summary), 1.0),
        )
}

/// The paged register table for `registry`.
fn asset_table(m: &PlantModel, view: &[u32]) -> Table {
    let mut t = Table::new()
        .columns([
            TableColumn::new("id", "ID").width(36.0),
            TableColumn::new("asset", "Asset").width(150.0),
            TableColumn::new("kind", "Kind").width(48.0),
            TableColumn::new("status", "Status").width(88.0),
            TableColumn::new("oee", "OEE").width(52.0),
        ])
        .striped(true)
        .label("asset register");
    for id in view {
        if let Some(a) = m.asset(*id) {
            t = t.row([
                a.id.to_string(),
                a.name.to_string(),
                match a.kind {
                    AssetKind::Site => "Site",
                    AssetKind::Line => "Line",
                    AssetKind::Cell => "Cell",
                }
                .to_string(),
                a.status.label().to_string(),
                format!("{:.0}%", a.oee * 100.0),
            ]);
        }
    }
    t
}

/// Item labels for the registry ListView — the filtered id set
/// rendered "name — status".
fn registry_list_items(m: &PlantModel, ids: &[u32]) -> Vec<String> {
    m.assets
        .get()
        .into_iter()
        .filter(|a| ids.contains(&a.id))
        .map(|a| format!("{} — {}", a.name, a.status.label()))
        .collect()
}

// ---------------------------------------------------------------------------
// ASSET DETAIL — the selected asset's record: editable properties,
// live inspector, identity codes, notes, alarms.
// ---------------------------------------------------------------------------

fn detail(m: &PlantModel) -> Flex {
    // Header — back clears the selection (a real deselect write);
    // the action is a bound button that acks every alarm on the
    // selected asset.
    let header = Bound::new(
        PageHeader::new("Asset detail")
            .back(true)
            .subtitle("registry record")
            .action(
                Bound::new(Button::new("Ack asset alarms"), m).pull(|w: &mut Button, m| {
                    if w.take_activated() {
                        if let Some(id) = m.selected_asset.get() {
                            for a in m.active_alarms() {
                                if a.asset == id {
                                    m.ack_alarm(a.id);
                                }
                            }
                        }
                    }
                }),
            ),
        m,
    )
    .pull(|w: &mut PageHeader, m| {
        if w.take_back() {
            m.selected_asset.set_if_changed(None);
        }
    });

    let headline = Bound::new(Text::new("—").font_size(16.0), m).push(|w: &mut Text, m| {
        let s = match sel_asset(m) {
            Some(a) => format!("{}  ·  {}  ·  {}", a.name, kind_name(a.kind), a.serial),
            None => "no asset selected".to_string(),
        };
        w.set_content(s);
    });

    // Property grid — the editable record. Flat row order:
    // 0 name, 1 kind(choice), 2 serial, 3 installed, 4 status(choice),
    // 5 oee, 6 note.
    let grid = {
        let build = |m: &PlantModel| {
            let a = sel_asset(m);
            PropertyGrid::new()
                .label("asset properties")
                .section(
                    "Identity",
                    [
                        PropertyRow::text("Name", a.as_ref().map(|a| a.name).unwrap_or("—")),
                        PropertyRow::choice("Kind", ["Site", "Line", "Cell"]).selected(
                            a.as_ref()
                                .map(|a| match a.kind {
                                    AssetKind::Site => 0,
                                    AssetKind::Line => 1,
                                    AssetKind::Cell => 2,
                                })
                                .unwrap_or(0),
                        ),
                        PropertyRow::text("Serial", a.as_ref().map(|a| a.serial).unwrap_or("—")),
                        PropertyRow::text(
                            "Installed",
                            a.as_ref()
                                .map(|a| a.installed.to_string())
                                .unwrap_or_default(),
                        ),
                    ],
                )
                .section(
                    "Condition",
                    [
                        PropertyRow::choice(
                            "Status",
                            ["Running", "Degraded", "Down", "Maintenance"],
                        )
                        .selected(
                            a.as_ref()
                                .map(|a| match a.status {
                                    AssetStatus::Running => 0,
                                    AssetStatus::Degraded => 1,
                                    AssetStatus::Down => 2,
                                    AssetStatus::Maintenance => 3,
                                })
                                .unwrap_or(0),
                        ),
                        PropertyRow::text(
                            "OEE",
                            a.as_ref()
                                .map(|a| format!("{:.2}", a.oee))
                                .unwrap_or_default(),
                        ),
                        PropertyRow::text("Note", a.as_ref().map(|a| a.note).unwrap_or("")),
                    ],
                )
        };
        let mut last_sel = m.selected_asset.get();
        let mut last_sig = assets_sig(m);
        Bound::new(build(m), m)
            .pull(|w: &mut PropertyGrid, m| {
                while let Some((i, v)) = w.take_changed() {
                    let Some(id) = m.selected_asset.get() else {
                        break;
                    };
                    match i {
                        // Flat row index across sections.
                        0 => set_asset(m, id, |a| a.name = Box::leak(v.clone().into_boxed_str())),
                        1 => set_asset(m, id, |a| {
                            a.kind = match v.as_str() {
                                "Site" => AssetKind::Site,
                                "Cell" => AssetKind::Cell,
                                _ => AssetKind::Line,
                            }
                        }),
                        2 => set_asset(m, id, |a| a.serial = Box::leak(v.clone().into_boxed_str())),
                        3 => set_asset(m, id, |a| a.installed = v.parse().unwrap_or(a.installed)),
                        4 => set_asset(m, id, |a| {
                            a.status = match v.as_str() {
                                "Running" => AssetStatus::Running,
                                "Down" => AssetStatus::Down,
                                "Maintenance" => AssetStatus::Maintenance,
                                _ => AssetStatus::Degraded,
                            }
                        }),
                        5 => set_asset(m, id, |a| {
                            a.oee = v.parse::<f64>().unwrap_or(a.oee).clamp(0.0, 1.0)
                        }),
                        6 => set_asset(m, id, |a| a.note = Box::leak(v.clone().into_boxed_str())),
                        _ => {}
                    }
                }
            })
            .push(move |w: &mut PropertyGrid, m| {
                let sel = m.selected_asset.get();
                let sig = assets_sig(m);
                if sel != last_sel || sig != last_sig {
                    last_sel = sel;
                    last_sig = sig;
                    *w = build(m);
                }
            })
    };

    // Inspector — live readouts that follow selection + alarms.
    let inspector = {
        let build = |m: &PlantModel| {
            let a = sel_asset(m);
            let alarms = m
                .alarms
                .get()
                .iter()
                .filter(|al| Some(al.asset) == a.as_ref().map(|x| x.id) && al.active)
                .count();
            let wos = m
                .work_orders
                .get()
                .iter()
                .filter(|w| Some(w.asset) == a.as_ref().map(|x| x.id) && w.status != WoStatus::Done)
                .count();
            Inspector::new()
                .label("asset inspector")
                .section("Condition")
                .row(
                    "Status",
                    a.as_ref().map(|a| a.status.label()).unwrap_or("—"),
                )
                .row(
                    "OEE",
                    a.as_ref()
                        .map(|a| format!("{:.0}%", a.oee * 100.0))
                        .unwrap_or_default(),
                )
                .row("Active alarms", alarms.to_string())
                .section("Workload")
                .row("Open work orders", wos.to_string())
                .row(
                    "Next maint.",
                    m.schedule
                        .get()
                        .iter()
                        .filter(|t| Some(t.asset) == a.as_ref().map(|x| x.id))
                        .map(|t| format!("D+{} {}d", t.start_day, t.days))
                        .next()
                        .unwrap_or_else(|| "—".into()),
                )
        };
        let mut last = (
            m.selected_asset.get(),
            assets_sig(m),
            alarm_sig(m),
            wos_sig(m),
            sched_sig(m),
        );
        Bound::new(build(m), m).push(move |w: &mut Inspector, m| {
            let sig = (
                m.selected_asset.get(),
                assets_sig(m),
                alarm_sig(m),
                wos_sig(m),
                sched_sig(m),
            );
            if sig != last {
                last = sig;
                *w = build(m);
            }
        })
    };

    // Identity — tag matrix + linear code of the serial; status lamp
    // and OEE-as-stars follow the selection AND serial edits.
    let qr = {
        let mut last = sel_asset(m).map(|a| a.serial);
        Bound::new(
            QrCode::from_matrix(qr_modules(sel_asset(m).map(|a| a.serial).unwrap_or("NONE")))
                .label("asset tag"),
            m,
        )
        .push(move |w: &mut QrCode, m| {
            let serial = sel_asset(m).map(|a| a.serial);
            if serial != last {
                last = serial;
                *w = QrCode::from_matrix(qr_modules(serial.unwrap_or("NONE"))).label("asset tag");
            }
        })
    };

    let bar = Bound::new(Barcode::new().text("—").label("serial"), m).push(|w: &mut Barcode, m| {
        let s = sel_asset(m).map(|a| a.serial).unwrap_or("—");
        if w.content() != s {
            w.set_text(s);
        }
    });

    let lamp = {
        let mut last = None;
        Bound::new(StatusDot::new("—").status(Status::Off), m).push(move |w: &mut StatusDot, m| {
            let s = sel_asset(m).map(|a| (a.status, a.status.label()));
            if s != last {
                last = s;
                if let Some((st, label)) = s {
                    *w = StatusDot::new(label).status(status_lamp(st));
                }
            }
        })
    };

    let rating = Bound::new(Rating::new().max(5).half_steps(true).read_only(true), m).push(
        |w: &mut Rating, m| {
            w.set_value(sel_asset(m).map(|a| a.oee as f32 * 5.0).unwrap_or(0.0));
        },
    );

    // Operator note — committed edits write `Asset.note` (a `&'static`
    // field: the sim leaks the short string per commit — bounded).
    let note_edit = {
        let mut last = sel_asset(m).map(|a| (Some(a.id), a.note));
        Bound::new(
            InlineEdit::new(sel_asset(m).map(|a| a.note).unwrap_or(""))
                .placeholder("operator note"),
            m,
        )
        .pull(|w: &mut InlineEdit, m| {
            if let Some((_, new)) = w.take_committed() {
                if let Some(id) = m.selected_asset.get() {
                    set_asset(m, id, |a| a.note = Box::leak(new.clone().into_boxed_str()));
                }
            }
        })
        .push(move |w: &mut InlineEdit, m| {
            let cur = sel_asset(m).map(|a| (Some(a.id), a.note));
            if cur != last && !w.is_editing() {
                last = cur;
                *w = InlineEdit::new(sel_asset(m).map(|a| a.note).unwrap_or(""))
                    .placeholder("operator note");
            }
        })
    };

    // Multi-line view of the same note field — the two editors stay
    // in sync through the model.
    let note_pad = {
        let mut last = m.selected_asset.get();
        Bound::new(
            TextArea::new()
                .label("Operator note")
                .min_lines(3)
                .placeholder("shift notes for this asset…"),
            m,
        )
        .pull(|w: &mut TextArea, m| {
            if w.take_edited().is_some() {
                if let Some(id) = m.selected_asset.get() {
                    let v = w.value();
                    set_asset(m, id, |a| a.note = Box::leak(v.clone().into_boxed_str()));
                }
            }
        })
        .push(move |w: &mut TextArea, m| {
            let sel = m.selected_asset.get();
            let note = sel_asset(m).map(|a| a.note).unwrap_or("");
            if sel != last {
                last = sel;
            }
            // External writes (the inline editor, another zone)
            // mirror in; the pull path already wrote through, so this
            // never stomps an in-flight edit.
            if w.value() != note {
                w.set_value(note);
                // `set_value` raises `edited` too — drain it so the
                // pull doesn't echo our own write back into the model.
                let _ = w.take_edited();
            }
        })
    };

    // Anchor — sibling cells of the selected asset (jump list).
    let siblings = {
        let build = |m: &PlantModel| {
            let parent = sel_asset(m).and_then(|a| a.parent);
            let mut items = Vec::new();
            if let Some(p) = parent.or_else(|| sel_asset(m).map(|a| a.id)) {
                for sib in m.asset_children(Some(p)) {
                    items.push(AnchorItem::new(sib.name, sib.id.to_string()));
                }
            }
            Anchor::new().items(items).label("sibling cells")
        };
        let mut last = (m.selected_asset.get(), assets_sig(m));
        Bound::new(build(m), m)
            .pull(|w: &mut Anchor, m| {
                if let Some((_, target)) = w.take_clicked() {
                    if let Ok(id) = target.parse::<u32>() {
                        m.selected_asset.set_if_changed(Some(id));
                    }
                }
            })
            .push(move |w: &mut Anchor, m| {
                let sig = (m.selected_asset.get(), assets_sig(m));
                if sig != last {
                    last = sig;
                    *w = build(m);
                }
            })
    };

    // Alarms on this asset — a live list; double-click acks.
    let alarm_list = {
        let mut last = (m.selected_asset.get(), alarm_sig(m));
        let build = |m: &PlantModel| {
            ListView::new()
                .items(asset_alarms(m).iter().map(|a| {
                    format!(
                        "#{} [{}] {}{}",
                        a.id,
                        a.severity.label(),
                        a.message,
                        if a.acked { " (acked)" } else { "" }
                    )
                }))
                .label("asset alarms")
        };
        Bound::new(build(m), m)
            .pull(|w: &mut ListView, m| {
                if let Some(i) = w.take_activated() {
                    if let Some(a) = asset_alarms(m).get(i) {
                        m.ack_alarm(a.id);
                    }
                }
            })
            .push(move |w: &mut ListView, m| {
                let sig = (m.selected_asset.get(), alarm_sig(m));
                if sig != last {
                    last = sig;
                    *w = build(m);
                }
            })
    };

    // Record summary — live text mounted under static chrome.
    let summary_text = Bound::new(Text::new("—").font_size(11.0), m).push(|w: &mut Text, m| {
        w.set_content(match sel_asset(m) {
            Some(a) => format!(
                "{} · installed {} · OEE {:.0}% · note: {}",
                a.status.label(),
                a.installed,
                a.oee * 100.0,
                if a.note.is_empty() { "—" } else { a.note }
            ),
            None => "select an asset in the registry".to_string(),
        });
    });

    Flex::column()
        .gap(ZONE_STACK)
        .child(header)
        .child(headline)
        .child(
            Grid::new()
                .columns(12)
                .gap(ZONE_GAP)
                .cell(GridCell::new(Clamp::new().maximum(560.0).child(grid)).col_span(7))
                .cell(GridCell::new(inspector).col_span(5)),
        )
        .child(
            strip()
                .child(lamp)
                .child(rating)
                .child(note_edit)
                .child_flex(DummyWidget, 1.0),
        )
        .child(
            GroupBox::new("IDENTITY").child(
                row()
                    .child_flex(band(BAND_S, framed(1.0, qr)), 1.0)
                    .child_flex(band(BAND_S, bar), 1.0)
                    .child_flex(summary_text, 1.0),
            ),
        )
        .child(
            Accordion::new()
                .allow_multiple(true)
                .section("Operator note", note_pad)
                .section("Sibling cells", siblings),
        )
        .child(
            ExpanderRow::new("Alarms on this asset")
                .subtitle("double-click a row to ack")
                .icon("⚠")
                .child(alarm_list),
        )
}

/// `AssetKind` display name.
fn kind_name(k: AssetKind) -> &'static str {
    match k {
        AssetKind::Site => "Site",
        AssetKind::Line => "Line",
        AssetKind::Cell => "Cell",
    }
}

// ---------------------------------------------------------------------------
// WORK ORDERS — the pipeline: kanban moves, register, status stepper,
// checklist + crew ops on WO-4471, intake wizard.
// ---------------------------------------------------------------------------

fn work_orders(m: &PlantModel) -> Flex {
    // Action strip — the two CTAs write the model exactly as the old
    // banner did: New WO appends, ack-all clears the alarm board.
    let status = Bound::new(Text::new("—").font_size(11.0), m).push(|w: &mut Text, m| {
        let wos = m.work_orders.get();
        let n = |s: WoStatus| wos.iter().filter(|w| w.status == s).count();
        w.set_content(format!(
            "{} queued · {} in progress · {} review",
            n(WoStatus::Queued),
            n(WoStatus::InProgress),
            n(WoStatus::Review)
        ));
    });
    let new_wo = Bound::new(Button::new("New WO"), m).pull(|w: &mut Button, m| {
        if w.take_activated() {
            create_wo(m, WoIntake::default());
        }
    });
    let ack_all = Bound::new(Button::new("Ack all alarms"), m).pull(|w: &mut Button, m| {
        if w.take_activated() {
            for a in m.active_alarms() {
                m.ack_alarm(a.id);
            }
            m.log(usize::MAX, "all active alarms acknowledged");
        }
    });

    // Kanban — columns are `WoStatus::columns()`; a drop writes
    // `move_wo`, a card click selects the WO.
    let kanban = {
        let build = |m: &PlantModel| {
            let mut k = Kanban::new().label("work orders");
            for s in WoStatus::columns() {
                k = k.column(s.label());
            }
            for w in m.work_orders.get() {
                k = k.card(w.status.label(), w.title);
            }
            k
        };
        let mut last = wos_sig(m);
        Bound::new(build(m), m)
            .pull(|w: &mut Kanban, m| {
                if let Some((_from, slot, to)) = w.take_moved() {
                    let title = w.card_title(to, slot).to_string();
                    // The emitted column index is widget state —
                    // checked, not trusted.
                    if let (Some(id), Some(status)) =
                        (wo_id(&title), WoStatus::columns().get(to).copied())
                    {
                        m.move_wo(id, status);
                    }
                }
                if let Some(flat) = w.take_selected() {
                    // Flat index walks the columns in order.
                    let mut n = flat;
                    for c in 0..w.column_count() {
                        if n < w.card_count(c) {
                            if let Some(id) = wo_id(w.card_title(c, n)) {
                                m.selected_wo.set_if_changed(Some(id));
                            }
                            break;
                        }
                        n -= w.card_count(c);
                    }
                }
            })
            .push(move |w: &mut Kanban, m| {
                let s = wos_sig(m);
                if s != last {
                    last = s;
                    *w = build(m);
                }
            })
    };

    // Register — row select writes `selected_wo`.
    let table = {
        let build = |m: &PlantModel| {
            let mut t = Table::new()
                .columns([
                    TableColumn::new("wo", "WO").width(64.0),
                    TableColumn::new("asset", "Asset").width(110.0),
                    TableColumn::new("status", "Status").width(80.0),
                    TableColumn::new("pri", "Priority").width(64.0),
                    TableColumn::new("due", "Due").width(44.0),
                ])
                .striped(true)
                .label("work order register");
            for w in m.work_orders.get() {
                t = t.row([
                    format!("WO-{}", w.id),
                    m.asset_name(w.asset).to_string(),
                    w.status.label().to_string(),
                    w.priority.label().to_string(),
                    DUE_DAYS[w.due_day.min(6) as usize].to_string(),
                ]);
            }
            t
        };
        let mut last = (wos_sig(m), assets_sig(m));
        let mut last_sel = m.selected_wo.get();
        Bound::new(build(m), m)
            .pull(|w: &mut Table, m| {
                if let Some(i) = w.take_activated().or_else(|| w.selected()) {
                    if let Some(wo) = m.work_orders.get().get(i) {
                        m.selected_wo.set_if_changed(Some(wo.id));
                    }
                }
            })
            .push(move |w: &mut Table, m| {
                let s = (wos_sig(m), assets_sig(m));
                if s != last {
                    last = s;
                    *w = build(m);
                }
                let sel = m.selected_wo.get();
                if sel != last_sel {
                    last_sel = sel;
                    match sel.and_then(|id| m.work_orders.get().iter().position(|w| w.id == id)) {
                        Some(i) => w.select(i),
                        None => w.clear_selection(),
                    }
                }
            })
    };

    // Status stepper — completed steps are navigable; clicking one
    // moves the WO *back* to that stage (a real revert write).
    let stepper = {
        let mut last = m.selected_wo.get();
        Bound::new(
            Steps::new()
                .steps(
                    WoStatus::columns()
                        .iter()
                        .map(|s| s.label())
                        .collect::<Vec<_>>(),
                )
                .current(
                    sel_wo(m)
                        .map(|w| {
                            WoStatus::columns()
                                .iter()
                                .position(|s| *s == w.status)
                                .unwrap_or(0)
                        })
                        .unwrap_or(0),
                )
                .clickable_completed(true),
            m,
        )
        .pull(|w: &mut Steps, m| {
            if let Some(i) = w.take_navigated() {
                if let Some(id) = m.selected_wo.get() {
                    m.move_wo(id, WoStatus::columns()[i.min(3)]);
                }
            }
        })
        .push(move |w: &mut Steps, m| {
            let sel = m.selected_wo.get();
            let idx = sel_wo(m)
                .map(|w| {
                    WoStatus::columns()
                        .iter()
                        .position(|s| *s == w.status)
                        .unwrap_or(0)
                })
                .unwrap_or(0);
            if sel != last || w.current_step() != idx {
                last = sel;
                w.set_current(idx);
            }
        })
    };

    // Pips — one dot per WO.
    let pips = {
        let mut last_sel = m.selected_wo.get();
        let mut last_n = m.work_orders.get().len();
        Bound::new(
            PipsPager::new(m.work_orders.get().len())
                .max_visible(crate::zone::MAX_SELECTOR_ENTRIES),
            m,
        )
        .pull(|w: &mut PipsPager, m| {
            if let Some(i) = w.take_selected() {
                if let Some(wo) = m.work_orders.get().get(i) {
                    m.selected_wo.set_if_changed(Some(wo.id));
                }
            }
        })
        .push(move |w: &mut PipsPager, m| {
            if m.work_orders.get().len() != last_n {
                last_n = m.work_orders.get().len();
                *w = PipsPager::new(last_n).max_visible(crate::zone::MAX_SELECTOR_ENTRIES);
            }
            let sel = m.selected_wo.get();
            if sel != last_sel {
                last_sel = sel;
                if let Some(i) =
                    sel.and_then(|id| m.work_orders.get().iter().position(|w| w.id == id))
                {
                    w.set_current(i);
                }
            }
        })
    };

    // WO switcher — thumbnails tinted by priority.
    let switcher = {
        let build = |m: &PlantModel| {
            let mut t = TaskSwitcher::new().label("wo switcher");
            for w in m.work_orders.get() {
                t = t.item(Thumbnail::new(
                    w.title,
                    PRIORITY_COLORS[match w.priority {
                        crate::domain::WoPriority::Low => 0,
                        crate::domain::WoPriority::Medium => 1,
                        crate::domain::WoPriority::High => 2,
                        crate::domain::WoPriority::Critical => 3,
                    }],
                ));
            }
            t
        };
        let mut last = wos_sig(m);
        let mut last_sel = m.selected_wo.get();
        Bound::new(build(m), m)
            .pull(|w: &mut TaskSwitcher, m| {
                if let Some(i) = w.take_selected() {
                    if let Some(wo) = m.work_orders.get().get(i) {
                        m.selected_wo.set_if_changed(Some(wo.id));
                    }
                }
            })
            .push(move |w: &mut TaskSwitcher, m| {
                let s = wos_sig(m);
                if s != last {
                    last = s;
                    *w = build(m);
                }
                let sel = m.selected_wo.get();
                if sel != last_sel {
                    last_sel = sel;
                    if let Some(i) =
                        sel.and_then(|id| m.work_orders.get().iter().position(|w| w.id == id))
                    {
                        w.set_index(i);
                    }
                }
            })
    };

    // WO-4471 ops cluster — CheckList/Transfer/Ticket hold fixed
    // label sets (no relabel mutators), so they bind by id to the
    // seeded active WO; every write is real.
    const OPS_WO: u32 = 4471;
    let checklist = {
        let build = |m: &PlantModel| {
            let mut c = CheckList::new().label("WO-4471 checklist");
            if let Some(w) = m.work_orders.get().into_iter().find(|w| w.id == OPS_WO) {
                for (label, done) in &w.checklist {
                    c = c.item(CheckItem::new(*label).checked(*done));
                }
            }
            c
        };
        let mut last = wos_sig(m);
        Bound::new(build(m), m)
            .pull(|w: &mut CheckList, m| {
                while let Some((i, on)) = w.take_changed() {
                    update_wo_id(m, OPS_WO, |wo| {
                        if let Some(item) = wo.checklist.get_mut(i) {
                            item.1 = on;
                        }
                    });
                }
            })
            .push(move |w: &mut CheckList, m| {
                let s = wos_sig(m);
                if s != last {
                    last = s;
                    *w = build(m);
                }
            })
    };

    let transfer = {
        let crew_of = |m: &PlantModel, assigned: usize| {
            let crew = m.crew.get();
            let src: Vec<String> = crew
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != assigned)
                .map(|(_, c)| c.name.to_string())
                .collect();
            let tgt: Vec<String> = crew
                .get(assigned)
                .map(|c| vec![c.name.to_string()])
                .unwrap_or_default();
            (src, tgt)
        };
        let assignee0 = m
            .work_orders
            .get()
            .into_iter()
            .find(|w| w.id == OPS_WO)
            .map(|w| w.assignee)
            .unwrap_or(0);
        let (src0, tgt0) = crew_of(m, assignee0);
        // `crew_of` reads crew names — fold `crew_sig` so a roster
        // rename re-seats the shuttle even when no WO field moved.
        let mut last = (wos_sig(m), crew_sig(m));
        Bound::new(
            Transfer::new()
                .titles("crew", "WO-4471 assigned")
                .source(src0)
                .target(tgt0)
                .label("crew shuttle"),
            m,
        )
        .pull(|w: &mut Transfer, m| {
            if let Some((names, dir)) = w.take_moved() {
                // The target side holds one name (the assignee) —
                // process every moved name; the last Right move wins
                // the assignment, a Left move unassigns.
                for name in &names {
                    match dir {
                        MoveDir::Right => {
                            if let Some(i) =
                                m.crew.get().iter().position(|c| c.name == name.as_str())
                            {
                                update_wo_id(m, OPS_WO, |wo| wo.assignee = i);
                            }
                        }
                        MoveDir::Left => {
                            update_wo_id(m, OPS_WO, |wo| wo.assignee = usize::MAX);
                        }
                    }
                }
            }
        })
        .push(move |w: &mut Transfer, m| {
            let s = (wos_sig(m), crew_sig(m));
            if s != last {
                last = s;
                let assigned = m
                    .work_orders
                    .get()
                    .into_iter()
                    .find(|w| w.id == OPS_WO)
                    .map(|w| w.assignee)
                    .unwrap_or(0);
                let (src, tgt) = crew_of(m, assigned);
                *w = Transfer::new()
                    .titles("crew", "WO-4471 assigned")
                    .source(src)
                    .target(tgt)
                    .label("crew shuttle");
            }
        })
    };

    let ticket = {
        let build = |m: &PlantModel| {
            let w = m.work_orders.get().into_iter().find(|w| w.id == OPS_WO);
            match w {
                Some(w) => Ticket::new(w.title)
                    .caption(m.asset_name(w.asset))
                    .field("Status", w.status.label())
                    .field("Priority", w.priority.label())
                    .field(
                        "Crew",
                        m.crew.get().get(w.assignee).map(|c| c.name).unwrap_or("—"),
                    )
                    .field("Progress", format!("{:.0}%", w.progress * 100.0))
                    .code(m.asset(w.asset).map(|a| a.serial).unwrap_or("—"))
                    .label("wo card"),
                None => Ticket::new("WO-4471").label("wo card"),
            }
        };
        // Crew field renders the assignee's name — `crew_sig` covers
        // roster renames even when no WO field moved.
        let mut last = (wos_sig(m), assets_sig(m), crew_sig(m));
        Bound::new(build(m), m)
            .pull(|w: &mut Ticket, m| {
                if w.take_torn() {
                    // Tearing the ticket = closing the order out.
                    m.move_wo(OPS_WO, WoStatus::Done);
                }
            })
            .push(move |w: &mut Ticket, m| {
                let s = (wos_sig(m), assets_sig(m), crew_sig(m));
                if s != last {
                    last = s;
                    *w = build(m);
                }
            })
    };

    // Deck of open WOs — dismissing the top card moves it to Done.
    let deck = {
        let build = |m: &PlantModel| {
            let mut d = CardDeck::new().label("open work orders");
            for w in m
                .work_orders
                .get()
                .iter()
                .filter(|w| w.status != WoStatus::Done)
            {
                d = d
                    .card(Text::new(format!("{} — {}", w.title, w.status.label())).font_size(11.0));
            }
            d
        };
        let mut last = wos_sig(m);
        Bound::new(build(m), m)
            .pull(|w: &mut CardDeck, m| {
                if w.take_dismissed() {
                    // The deck's top card is the last non-Done WO —
                    // the same order `build` pushed it in.
                    if let Some(id) = m
                        .work_orders
                        .get()
                        .iter()
                        .rev()
                        .find(|w| w.status != WoStatus::Done)
                        .map(|w| w.id)
                    {
                        m.move_wo(id, WoStatus::Done);
                    }
                }
            })
            .push(move |w: &mut CardDeck, m| {
                let s = wos_sig(m);
                if s != last {
                    last = s;
                    *w = build(m);
                }
            })
    };

    // Crew presence board — clicking a member assigns the selected WO.
    let crew = {
        // `None` seed forces the first-tick apply (speaking/muted
        // lamps) the build's `muted(...)` alone doesn't cover.
        let mut last = None;
        // Names are baked into Participants at mount — a `crew_sig`
        // change (membership or rename) must re-seat the grid, while
        // presence-only changes update lamps in place.
        let mut last_crew = crew_sig(m);
        let build = |m: &PlantModel| {
            let mut v = VideoGrid::new().label("crew board — click to assign");
            for c in m.crew.get() {
                v = v.participant(
                    Participant::new(c.name, STATUS_COLORS[0])
                        .muted(c.presence != crate::domain::Presence::OnShift),
                );
            }
            v
        };
        Bound::new(build(m), m)
            .pull(|w: &mut VideoGrid, m| {
                if let Some(i) = w.take_selected() {
                    // Widget-supplied index — bound it to the roster.
                    if i < m.crew.get().len() {
                        m.update_wo(|wo| wo.assignee = i);
                    }
                }
            })
            .push(move |w: &mut VideoGrid, m| {
                let cs = crew_sig(m);
                if cs != last_crew {
                    last_crew = cs;
                    last = Some(presence_sig(m));
                    *w = build(m);
                    return;
                }
                let sig = presence_sig(m);
                if last != Some(sig) {
                    last = Some(sig);
                    for (i, c) in m.crew.get().iter().enumerate() {
                        w.set_muted(i, c.presence != crate::domain::Presence::OnShift);
                        w.set_speaking(i, c.presence == crate::domain::Presence::OnShift);
                    }
                }
            })
    };

    // Wizard — the intake flow. Each step hosts real bound controls
    // writing the pending intake signals (Bound children tick only
    // while their page is shown — hidden pages report no bounds);
    // Finish consumes the pending state into `create_wo`.
    let intake_asset = Signal::new(m.selected_asset.get());
    let intake_fault = Signal::new(WoIntake::default().fault);
    let intake_crew = Signal::new(0usize);
    let intake_due = Signal::new(WoIntake::default().due as usize);

    // Step 1 "Scope" — the asset picker (cell assets; options are
    // construct-time so it re-seats on registry edits) + fault kind.
    let scope_asset = {
        let ia = intake_asset.clone();
        let ia2 = intake_asset.clone();
        let build = |m: &PlantModel, sel: Option<u32>| {
            let cells = cell_assets(m);
            let mut d = Dropdown::new(cells.iter().map(|a| a.name))
                .placeholder("asset to cover")
                .label("asset");
            if let Some(i) = sel.and_then(|id| cells.iter().position(|a| a.id == id)) {
                d.commit(i);
            }
            d
        };
        let dd = build(m, ia.get());
        let mut last_i = dd.selected();
        let mut last_sig = assets_sig(m);
        Bound::new(dd, m)
            .pull(move |w: &mut Dropdown, m| {
                // No `take_selected` — poll the index like the site
                // filter; the row maps to the cell's asset id.
                let i = w.selected();
                if i != last_i {
                    last_i = i;
                    ia.set_if_changed(cell_assets(m).get(i).map(|a| a.id));
                }
            })
            .push(move |w: &mut Dropdown, m| {
                let s = assets_sig(m);
                if s != last_sig && !w.is_open() {
                    last_sig = s;
                    *w = build(m, ia2.get());
                }
            })
    };
    let scope_fault = {
        let fs = intake_fault.clone();
        Bound::new(
            Segmented::new()
                .options(INTAKE_FAULTS.iter().map(|f| f.0))
                .selected(fs.get())
                .label("fault kind"),
            m,
        )
        .pull(move |w: &mut Segmented, _m| {
            if let Some(i) = w.take_selected() {
                fs.set_if_changed(i);
            }
        })
    };

    // Step 2 "Schedule" — crew picker + due day-of-week slider. The
    // roster's names are stable (only presence/room move), so mount-
    // time options suffice.
    let sched_crew = {
        let ic = intake_crew.clone();
        let mut dd = Dropdown::new(m.crew.get().iter().map(|c| c.name))
            .placeholder("crew member")
            .label("crew");
        dd.commit(ic.get());
        let mut last_i = dd.selected();
        Bound::new(dd, m).pull(move |w: &mut Dropdown, _m| {
            let i = w.selected();
            if i != last_i {
                last_i = i;
                ic.set_if_changed(i);
            }
        })
    };
    let sched_due = {
        let idu = intake_due.clone();
        let mut last = intake_due.get();
        Bound::new(
            Slider::new(0.0, 6.0)
                .step(1.0)
                .with_value(last as f64)
                .with_value_text(|v| DUE_DAYS[v.round().clamp(0.0, 6.0) as usize].to_string())
                .label("due day"),
            m,
        )
        .pull(move |w: &mut Slider, _m| {
            let v = (w.value().round() as usize).min(6);
            if v != last {
                last = v;
                idu.set_if_changed(v);
            }
        })
    };

    // Step 3 "Review" — live summary of the pending intake.
    let review = {
        let (ia, fs, ic, idu) = (
            intake_asset.clone(),
            intake_fault.clone(),
            intake_crew.clone(),
            intake_due.clone(),
        );
        Bound::new(Text::new("—").font_size(11.0), m).push(move |w: &mut Text, m| {
            let (fault, pri) = INTAKE_FAULTS[fs.get().min(INTAKE_FAULTS.len() - 1)];
            w.set_content(format!(
                "new WO — {} · {fault} ({}) · {} · due {}",
                ia.get().map(|id| m.asset_name(id)).unwrap_or("—"),
                pri.label(),
                m.crew.get().get(ic.get()).map(|c| c.name).unwrap_or("—"),
                DUE_DAYS[idu.get().min(6)],
            ));
        })
    };

    let wizard = {
        let (ia, fs, ic, idu) = (
            intake_asset.clone(),
            intake_fault.clone(),
            intake_crew.clone(),
            intake_due.clone(),
        );
        Bound::new(
            Wizard::new()
                .step(
                    "Scope",
                    Flex::column()
                        .gap(ZONE_GAP)
                        .child(scope_asset)
                        .child(scope_fault),
                )
                .step(
                    "Schedule",
                    Flex::column()
                        .gap(ZONE_GAP)
                        .child(sched_crew)
                        .child(sched_due),
                )
                .step("Review", review)
                .finish_text("Create WO")
                .cancelable(true)
                .label("new work order"),
            m,
        )
        .pull(move |w: &mut Wizard, m| {
            if w.take_finished() {
                create_wo(
                    m,
                    WoIntake {
                        asset: ia.get(),
                        fault: fs.get(),
                        crew: ic.get(),
                        due: idu.get() as u8,
                    },
                );
                w.go_to(0); // restart the flow for the next intake
            }
            if w.take_cancelled() {
                m.log(usize::MAX, "WO intake cancelled");
            }
        })
    };

    // Inspector drawer — the selected WO's fields, live.
    let wo_inspector = {
        let build = |m: &PlantModel| {
            let w = sel_wo(m);
            Inspector::new()
                .label("wo inspector")
                .section("Work order")
                .row("Title", w.as_ref().map(|w| w.title).unwrap_or("—"))
                .row(
                    "Asset",
                    w.as_ref().map(|w| m.asset_name(w.asset)).unwrap_or("—"),
                )
                .row(
                    "Status",
                    w.as_ref().map(|w| w.status.label()).unwrap_or("—"),
                )
                .row(
                    "Priority",
                    w.as_ref().map(|w| w.priority.label()).unwrap_or("—"),
                )
                .row(
                    "Crew",
                    w.as_ref()
                        .and_then(|w| m.crew.get().get(w.assignee).map(|c| c.name))
                        .unwrap_or("—"),
                )
                .row(
                    "Progress",
                    w.as_ref()
                        .map(|w| format!("{:.0}%", w.progress * 100.0))
                        .unwrap_or_default(),
                )
        };
        // Crew names render in the Crew row — `crew_sig` covers them.
        let mut last = (m.selected_wo.get(), wos_sig(m), assets_sig(m), crew_sig(m));
        Bound::new(build(m), m).push(move |w: &mut Inspector, m| {
            let sig = (m.selected_wo.get(), wos_sig(m), assets_sig(m), crew_sig(m));
            if sig != last {
                last = sig;
                *w = build(m);
            }
        })
    };

    let drawer = Bound::new(
        Drawer::new("WO inspector")
            .width(280.0)
            .content(wo_inspector),
        m,
    )
    .pull(|w: &mut Drawer, m| {
        if w.take_close_requested() {
            m.log(usize::MAX, "WO inspector drawer closed");
        }
    });

    // Selected-WO card — live summary text under static chrome; the
    // Advance button beside it moves the WO one stage on. (`Card`'s
    // action slot is `Button`-typed, so the bound button mounts as a
    // sibling in the same row.)
    let sel_text = Bound::new(Text::new("—").font_size(11.0), m).push(|w: &mut Text, m| {
        w.set_content(match sel_wo(m) {
            Some(w) => format!(
                "{} · {} · {} · {:.0}% done",
                w.title,
                w.status.label(),
                w.priority.label(),
                w.progress * 100.0
            ),
            None => "no work order selected".to_string(),
        });
    });
    let sel_card = Card::outlined()
        .title("Selected work order")
        .child(sel_text);
    let advance = Bound::new(Button::new("Advance ▸"), m).pull(|w: &mut Button, m| {
        if w.take_activated() {
            if let Some(wo) = sel_wo(m) {
                let cols = WoStatus::columns();
                let i = cols.iter().position(|s| *s == wo.status).unwrap_or(0);
                m.move_wo(wo.id, cols[(i + 1).min(3)]);
            }
        }
    });

    // WO notes — the String field edits cleanly (no leak needed).
    let notes = {
        let mut last = m.selected_wo.get();
        Bound::new(
            TextArea::new()
                .label("WO notes")
                .min_lines(3)
                .placeholder("work order notes…"),
            m,
        )
        .pull(|w: &mut TextArea, m| {
            if w.take_edited().is_some() {
                let v = w.value();
                m.update_wo(|wo| wo.notes = v.clone());
            }
        })
        .push(move |w: &mut TextArea, m| {
            let sel = m.selected_wo.get();
            if sel != last {
                last = sel;
                w.set_value(sel_wo(m).map(|w| w.notes.clone()).unwrap_or_default());
                // `set_value` flags `edited`; drain so the pull doesn't
                // echo this programmatic write back into `work_orders`.
                let _ = w.take_edited();
            }
        })
    };

    Flex::column()
        .gap(ZONE_STACK)
        .child(strip().child_flex(status, 1.0).child(new_wo).child(ack_all))
        .child(row().child_flex(band(BAND_L, kanban), 1.0))
        .child(row().child_flex(band(BAND_L, table), 1.0))
        .child(
            strip()
                .child(stepper)
                .child(pips)
                .child_flex(DummyWidget, 1.0),
        )
        .child(
            row()
                .child_flex(band(BAND_M, checklist), 1.0)
                .child_flex(band(BAND_M, transfer), 1.0),
        )
        .child(
            row()
                .child_flex(band(BAND_M, switcher), 1.0)
                .child_flex(band(BAND_M, crew), 1.0),
        )
        .child(
            row()
                .child_flex(band(BAND_M, ticket), 1.0)
                .child_flex(band(BAND_M, deck), 1.0),
        )
        .child(
            row()
                .child_flex(band(BAND_M, wizard), 2.0)
                .child_flex(band(BAND_M, notes), 1.0),
        )
        .child(strip().child_flex(sel_card, 1.0).child(advance))
        .child(drawer)
}

// ---------------------------------------------------------------------------
// MAINTENANCE — the two-week plan. Pickers write `MaintTask` windows;
// `task_sel` is the page's shared selection signal.
// ---------------------------------------------------------------------------

fn maintenance(m: &PlantModel) -> Flex {
    let task_sel = Signal::new(m.schedule.get().first().map(|t| t.id).unwrap_or(0));

    // Gantt — task bars from the schedule; hovering a bar selects the
    // task (the pickers below then edit it).
    let gantt = {
        let build = |m: &PlantModel| {
            let mut g = Gantt::new().total_days(14.0).label("two-week plan");
            for t in m.schedule.get() {
                g = g
                    .task(t.title, f32::from(t.start_day), f32::from(t.days))
                    .progress(if t.done { 1.0 } else { 0.0 });
            }
            g
        };
        let mut last = sched_sig(m);
        let ts = task_sel.clone();
        Bound::new(build(m), m)
            .pull(move |w: &mut Gantt, m| {
                if let Some(i) = w.take_hovered() {
                    if let Some(t) = m.schedule.get().get(i) {
                        ts.set_if_changed(t.id);
                    }
                }
            })
            .push(move |w: &mut Gantt, m| {
                let s = sched_sig(m);
                if s != last {
                    last = s;
                    *w = build(m);
                }
            })
    };

    // Week grid — click a task to select it (and its asset); click an
    // empty slot to book a one-day block.
    let week = {
        let build = |m: &PlantModel| {
            let mut v = WeekView::new().label("week grid");
            for t in m.schedule.get() {
                let day = (t.start_day % 7) as usize;
                let (s, e) = (8.0, (8.0 + f32::from(t.days) * 2.0).min(18.0));
                v = v.event(WeekEvent::new(t.title, day, s, e));
            }
            v
        };
        let mut last = sched_sig(m);
        let ts = task_sel.clone();
        Bound::new(build(m), m)
            .pull(move |w: &mut WeekView, m| {
                if let Some((day, _hour)) = w.take_slot() {
                    let mut s = m.schedule.get();
                    let id = s.iter().map(|t| t.id).max().unwrap_or(0) + 1;
                    s.push(MaintTask {
                        id,
                        title: "Operator block",
                        asset: m.selected_asset.get().unwrap_or(3),
                        start_day: day.min(13) as u8,
                        days: 1,
                        done: false,
                        crew: 0,
                    });
                    m.schedule.set(s);
                }
                if let Some(i) = w.take_clicked() {
                    if let Some(t) = m.schedule.get().get(i) {
                        ts.set_if_changed(t.id);
                        m.selected_asset.set_if_changed(Some(t.asset));
                    }
                }
            })
            .push(move |w: &mut WeekView, m| {
                let s = sched_sig(m);
                if s != last {
                    last = s;
                    *w = build(m);
                }
            })
    };

    // Task window pickers — the month calendar and the range picker
    // edit the same `start_day`/`days` of `task_sel`, so they stay in
    // sync through the model.
    let calendar = {
        let ts = task_sel.clone();
        let ts2 = task_sel.clone();
        let mut last = sched_sig(m);
        let mut last_task = task_sel.get();
        let mut cal = Calendar::new()
            .selection(CalendarSelection::Range)
            .today(BASE_DAY)
            .week_starts_monday(true)
            .label("task window");
        if let Some(t) = m.schedule.get().iter().find(|t| t.id == task_sel.get()) {
            let (a, b) = task_range(t);
            cal = cal.range(a, b);
        }
        Bound::new(cal, m)
            .pull(move |w: &mut Calendar, m| {
                if let Some((a, b)) = w.take_range() {
                    let start = (daynum(a) - daynum(BASE_DAY)).clamp(0, 13) as u8;
                    let days = ((daynum(b) - daynum(a)).abs() + 1).clamp(1, 14) as u8;
                    update_task(m, ts.get(), |t| {
                        t.start_day = start;
                        t.days = days;
                    });
                }
            })
            .push(move |w: &mut Calendar, m| {
                let s = sched_sig(m);
                let task = m.schedule.get().into_iter().find(|t| t.id == ts2.get());
                let want = task.as_ref().map(task_range);
                if let Some((a, b)) = want {
                    if s != last || ts2.get() != last_task {
                        last = s;
                        last_task = ts2.get();
                        if w.range_value() != Some((a, b)) {
                            w.set_range(a, b);
                        }
                    }
                }
            })
    };

    let picker = {
        let ts = task_sel.clone();
        let ts2 = task_sel.clone();
        let mut last = sched_sig(m);
        let mut last_task = task_sel.get();
        let mut dp = DatePicker::new()
            .range_mode(true)
            .today(BASE_DAY)
            .label("task window");
        if let Some(t) = m.schedule.get().iter().find(|t| t.id == task_sel.get()) {
            let (a, b) = task_range(t);
            dp.set_range((a, b));
        }
        Bound::new(dp, m)
            .pull(move |w: &mut DatePicker, m| {
                if let Some((a, b)) = w.take_range() {
                    let start = (daynum(a) - daynum(BASE_DAY)).clamp(0, 13) as u8;
                    let days = ((daynum(b) - daynum(a)).abs() + 1).clamp(1, 14) as u8;
                    update_task(m, ts.get(), |t| {
                        t.start_day = start;
                        t.days = days;
                    });
                }
            })
            .push(move |w: &mut DatePicker, m| {
                let s = sched_sig(m);
                let task = m.schedule.get().into_iter().find(|t| t.id == ts2.get());
                let want = task.as_ref().map(task_range);
                if (s != last || ts2.get() != last_task) && !w.is_open() {
                    last = s;
                    last_task = ts2.get();
                    if let Some((a, b)) = want {
                        if w.range_value() != Some((a, b)) {
                            w.set_range((a, b));
                        }
                    }
                }
            })
    };

    // Milestone rail — schedule entries as a timeline.
    let timeline = {
        let build = |m: &PlantModel| {
            let mut t = Timeline::new().label("milestones");
            let mut tasks = m.schedule.get();
            tasks.sort_by_key(|t| t.start_day);
            for task in tasks {
                t = t.item(
                    TimelineItem::new(task.title)
                        .subtitle(format!(
                            "D+{} · {}d · {}",
                            task.start_day,
                            task.days,
                            m.crew.get().get(task.crew).map(|c| c.name).unwrap_or("—")
                        ))
                        .dot(if task.done {
                            TimelineDot::Success
                        } else {
                            TimelineDot::Accent
                        }),
                );
            }
            t.pending("planning window open")
        };
        // Subtitles carry crew names — `crew_sig` covers renames.
        let mut last = (sched_sig(m), crew_sig(m));
        Bound::new(build(m), m).push(move |w: &mut Timeline, m| {
            let s = (sched_sig(m), crew_sig(m));
            if s != last {
                last = s;
                *w = build(m);
            }
        })
    };

    // Plan progress — `value` is construct-time, so the bar re-seats;
    // gated on the value's bits (done count / task count) instead of
    // every tick.
    let progress = Bound::new(ProgressBar::new().value(0.0), m).push({
        let mut last = 0.0f32.to_bits();
        move |w: &mut ProgressBar, m| {
            let s = m.schedule.get();
            let done = s.iter().filter(|t| t.done).count() as f32;
            let v = if s.is_empty() {
                0.0
            } else {
                done / s.len() as f32
            };
            if v.to_bits() != last {
                last = v.to_bits();
                *w = ProgressBar::new().value(v);
            }
        }
    });

    let summary = Bound::new(Text::new("—").font_size(11.0), m).push({
        let ts = task_sel.clone();
        move |w: &mut Text, m| {
            let s = m.schedule.get();
            let done = s.iter().filter(|t| t.done).count();
            let sel = s.iter().find(|t| t.id == ts.get());
            w.set_content(format!(
                "{} of {} tasks done — editing: {}",
                done,
                s.len(),
                sel.map(|t| t.title).unwrap_or("—")
            ));
        }
    });

    Flex::column()
        .gap(ZONE_STACK)
        .child(
            row()
                .child_flex(band(BAND_L, gantt), 3.0)
                .child_flex(band(BAND_L, week), 2.0),
        )
        .child(
            row()
                .child_flex(band(BAND_M, calendar), 1.0)
                .child_flex(band(BAND_M, timeline), 1.0),
        )
        .child(
            row()
                .child_flex(band(BAND_S, picker), 1.0)
                .child_flex(band(BAND_S, progress), 1.0),
        )
        .child(strip().child(summary).child_flex(DummyWidget, 1.0))
}

// ---------------------------------------------------------------------------
// DOCUMENTS — the plant record's documents. Big viewers sit behind a
// Tabs selector (one surface at a time); files + the shift log below.
// ---------------------------------------------------------------------------

fn documents(m: &PlantModel) -> Flex {
    let scroll_pos = Signal::new((0.0f32, 0.2f32));

    // Registry as JSON — a live document of the asset store.
    let json = {
        let build = |m: &PlantModel| {
            let site_nodes: Vec<(String, JsonNode)> = m
                .asset_children(None)
                .iter()
                .map(|s| {
                    (
                        s.name.to_string(),
                        JsonNode::object(
                            s.name,
                            m.asset_children(Some(s.id))
                                .iter()
                                .map(|l| {
                                    (
                                        l.name.to_string(),
                                        JsonNode::object(
                                            l.name,
                                            [
                                                (
                                                    "status".into(),
                                                    JsonNode::string("", l.status.label()),
                                                ),
                                                ("oee".into(), JsonNode::number("", l.oee)),
                                                (
                                                    "cells".into(),
                                                    JsonNode::number(
                                                        "",
                                                        m.asset_children(Some(l.id)).len() as f64,
                                                    ),
                                                ),
                                            ],
                                        ),
                                    )
                                })
                                .collect::<Vec<_>>(),
                        ),
                    )
                })
                .collect();
            JsonView::new(JsonNode::object("plant", site_nodes)).label("registry json")
        };
        let mut last = assets_sig(m);
        Bound::new(build(m), m).push(move |w: &mut JsonView, m| {
            let s = assets_sig(m);
            if s != last {
                last = s;
                *w = build(m);
            }
        })
    };

    // Firmware blob — the selected asset's synthesized image; a
    // serial edit regenerates it.
    let hex = {
        let mut last = sel_asset(m).map(|a| a.serial);
        let build = |m: &PlantModel| {
            HexView::new()
                .bytes(fw_blob(sel_asset(m).map(|a| a.serial).unwrap_or("MSFW")))
                .label("firmware image")
        };
        Bound::new(build(m), m).push(move |w: &mut HexView, m| {
            let serial = sel_asset(m).map(|a| a.serial);
            if serial != last {
                last = serial;
                *w = build(m);
            }
        })
    };

    // Alarm journal — severity-mapped; re-seated on alarm changes.
    let journal = {
        let fill = |w: &mut LogView, m: &PlantModel| {
            w.clear();
            for a in m.alarms.get() {
                let sev = if a.acked {
                    LogSeverity::Debug
                } else {
                    match a.severity {
                        AlarmSeverity::Info => LogSeverity::Info,
                        AlarmSeverity::Warning => LogSeverity::Warning,
                        AlarmSeverity::Critical => LogSeverity::Error,
                    }
                };
                w.push(
                    sev,
                    format!(
                        "[{}] #{} {} — {}{}",
                        shift_hhmm(a.raised_min),
                        a.id,
                        m.asset_name(a.asset),
                        a.message,
                        if a.acked { " (acked)" } else { "" }
                    ),
                );
            }
        };
        let mut last = (alarm_sig(m), assets_sig(m));
        let mut lv = LogView::new().max_lines(200);
        fill(&mut lv, m);
        Bound::new(lv, m).push(move |w: &mut LogView, m| {
            let s = (alarm_sig(m), assets_sig(m));
            if s != last {
                last = s;
                fill(w, m);
            }
        })
    };

    // Viewer tabs — one document surface at a time.
    let diff = {
        let build = |m: &PlantModel| {
            let a = sel_asset(m);
            let serial = a.as_ref().map(|a| a.serial).unwrap_or("—");
            let mut d = DiffView::new()
                .line(DiffKind::Hunk, "@@ recipe.cfg — installed → current @@")
                .line(DiffKind::Context, format!("  serial: {serial}"))
                .line(
                    DiffKind::Context,
                    format!(
                        "  installed: {}",
                        a.as_ref().map(|a| a.installed).unwrap_or(0)
                    ),
                );
            if let Some(a) = &a {
                if a.status != AssetStatus::Running {
                    d = d.line(DiffKind::Removed, "  state: running").line(
                        DiffKind::Added,
                        format!("  state: {}", a.status.label().to_lowercase()),
                    );
                } else {
                    d = d.line(DiffKind::Context, "  state: running");
                }
                if a.oee < 0.8 {
                    d = d
                        .line(DiffKind::Removed, "  oee_target: 0.85")
                        .line(DiffKind::Added, format!("  oee_actual: {:.2}", a.oee));
                }
                if !a.note.is_empty() {
                    d = d.line(DiffKind::Added, format!("  note: {}", a.note));
                }
            }
            d.label("recipe diff")
        };
        let mut last = assets_sig(m);
        let mut last_sel = m.selected_asset.get();
        Bound::new(build(m), m).push(move |w: &mut DiffView, m| {
            let s = assets_sig(m);
            if s != last || m.selected_asset.get() != last_sel {
                last = s;
                last_sel = m.selected_asset.get();
                *w = build(m);
            }
        })
    };

    let merge = {
        let build = |m: &PlantModel| {
            let a = sel_asset(m);
            let name = a.as_ref().map(|a| a.name).unwrap_or("asset");
            MergeView::new()
                .headers(("ours", "base", "theirs"))
                .row(MergeRow::aligned(
                    format!("asset: {name}"),
                    format!("asset: {name}"),
                    format!("asset: {name}"),
                ))
                .row(MergeRow::conflict(
                    "setpoint: 1200",
                    "setpoint: 1100",
                    "setpoint: 1350",
                ))
                .row(MergeRow::conflict(
                    format!("oee_floor: {:.2}", a.as_ref().map(|a| a.oee).unwrap_or(0.8)),
                    "oee_floor: 0.80",
                    "oee_floor: 0.85",
                ))
                .label("config merge")
        };
        let mut last = (m.selected_asset.get(), assets_sig(m));
        Bound::new(build(m), m)
            .pull(|w: &mut MergeView, m| {
                if let Some((row, side)) = w.take_choice() {
                    m.log(
                        usize::MAX,
                        format!(
                            "merge row {row} → {}",
                            match side {
                                MergeSide::Ours => "ours",
                                MergeSide::Theirs => "theirs",
                            }
                        ),
                    );
                }
            })
            .push(move |w: &mut MergeView, m| {
                let sig = (m.selected_asset.get(), assets_sig(m));
                if sig != last {
                    last = sig;
                    *w = build(m);
                }
            })
    };

    let code = {
        let mut last = (m.selected_asset.get(), assets_sig(m));
        let build = |m: &PlantModel| {
            let a = sel_asset(m);
            CodeView::new()
                .lines([
                    "// interlock — auto-generated".to_string(),
                    format!("ASSET {}", a.as_ref().map(|a| a.serial).unwrap_or("—")),
                    "WHEN guard_closed AND estop_clear THEN".to_string(),
                    format!(
                        "    set_state({})",
                        a.as_ref().map(|a| a.status.label()).unwrap_or("?")
                    ),
                    "ELSE".to_string(),
                    "    raise_alarm(torque_drift)".to_string(),
                    "END".to_string(),
                ])
                .current(Some(2))
                .label("plc interlock")
        };
        Bound::new(build(m), m).push(move |w: &mut CodeView, m| {
            let sig = (m.selected_asset.get(), assets_sig(m));
            if sig != last {
                last = sig;
                *w = build(m);
            }
        })
    };

    // Procedure doc — rendered from the selected WO's real fields.
    let doc = {
        let src_of = |m: &PlantModel| {
            match sel_wo(m) {
            Some(w) => format!(
                "# {}\n\n**Asset:** {}  \n**Priority:** {}  \n**Status:** {}\n\n## Checklist\n{}\n\n## Notes\n{}",
                w.title,
                m.asset_name(w.asset),
                w.priority.label(),
                w.status.label(),
                w.checklist
                    .iter()
                    .map(|(l, d)| format!("- [{}] {}", if *d { "x" } else { " " }, l))
                    .collect::<Vec<_>>()
                    .join("\n"),
                if w.notes.is_empty() { "—" } else { &w.notes },
            ),
            None => "# No work order selected".to_string(),
        }
        };
        let mut last = (wos_sig(m), m.selected_wo.get(), assets_sig(m));
        Bound::new(Markdown::new(src_of(m)).label("wo procedure"), m).push(
            move |w: &mut Markdown, m| {
                let sig = (wos_sig(m), m.selected_wo.get(), assets_sig(m));
                if sig != last {
                    last = sig;
                    w.set_source(src_of(m));
                }
            },
        )
    };

    // MES portal — navigates to the selected asset's record; a
    // serial edit re-targets it too.
    let portal = {
        let mut last = sel_asset(m).map(|a| a.serial);
        let url_of = |m: &PlantModel| {
            format!(
                "https://mes.plant-east.local/assets/{}",
                sel_asset(m).map(|a| a.serial).unwrap_or("none")
            )
        };
        let mut wv = WebView::new().label("mes portal");
        wv.navigate(&url_of(m));
        Bound::new(wv, m).push(move |w: &mut WebView, m| {
            let serial = sel_asset(m).map(|a| a.serial);
            if serial != last {
                last = serial;
                let url = url_of(m);
                if w.url() != url {
                    w.navigate(&url);
                }
            }
        })
    };

    let viewers = Tabs::new()
        .tab("REVISIONS", diff)
        .tab("MERGE REVIEW", merge)
        .tab("PLC SCRIPT", code)
        .tab("WO PROCEDURE", doc)
        .tab("MES PORTAL", portal)
        .label("documents");

    // Shift log — the record, as a scrollable message list paired
    // with its scroll indicator.
    let log_list = {
        // The cursor keys on `LogEntry::id` (monotonic, drain-proof) —
        // a positional `len` cursor freezes once `log()` front-drains
        // at LOG_CAP. `first` is the widget's head id; a moved head
        // means a drain, and MessageList is append-only, so re-seat.
        let mut first = None;
        let mut last = None;
        let mut ml = MessageList::new().label("shift log");
        for e in m.shift_log.get() {
            first = first.or(Some(e.id));
            last = Some(e.id);
            ml.push(log_message(m, &e));
        }
        Bound::new(ml, m).push(move |w: &mut MessageList, m| {
            let log = m.shift_log.get();
            if log.first().map(|e| e.id) != first {
                *w = MessageList::new().label("shift log");
                first = None;
                last = None;
            }
            for e in &log {
                if last.is_none_or(|l| e.id > l) {
                    w.push(log_message(m, e));
                    first = first.or(Some(e.id));
                    last = Some(e.id);
                }
            }
        })
    };

    let scroller = {
        let sig = scroll_pos.clone();
        Bound::new(ScrollView::new(log_list), m).pull(move |w: &mut ScrollView, _m| {
            let off = w.scroll_offset();
            let max = w.max_offset();
            let frac = {
                let c = w.content_size();
                let v = w.viewport();
                if c.y > 0.0 {
                    (v.height() / c.y).clamp(0.0, 1.0)
                } else {
                    1.0
                }
            };
            sig.set_if_changed((if max.y > 0.0 { off.y / max.y } else { 0.0 }, frac));
        })
    };

    let indicator = Bound::new(
        ScrollIndicator::vertical()
            .always_visible(true)
            .scroll(0.0, 0.2)
            .label("log position"),
        m,
    )
    .push(move |w: &mut ScrollIndicator, _m| {
        let (p, f) = scroll_pos.get();
        w.set_scroll(p, f);
    });

    // Files — the report, the firmware download (progress = WO-4473),
    // and the identifier clipboard.
    let attach = Bound::new(
        Attachment::new(
            "shift-report-A.txt",
            m.shift_log
                .get()
                .iter()
                .map(|e| e.text.len() as u64)
                .sum::<u64>()
                .max(1),
        )
        .label("shift report"),
        m,
    )
    .pull(|w: &mut Attachment, m| {
        if w.take_removed() {
            m.log(usize::MAX, "shift report detached from the record");
        }
    });

    let download = Bound::new(
        DownloadItem::new("firmware-hv11-1.4.2.pkg", 48_300_000)
            .progress(0.1)
            .label("firmware download"),
        m,
    )
    .pull(|w: &mut DownloadItem, m| {
        if let Some(a) = w.take_action() {
            w.set_state(match a {
                DownloadAction::Pause => DownloadState::Paused,
                DownloadAction::Resume | DownloadAction::Retry => DownloadState::Downloading,
                DownloadAction::Cancel => DownloadState::Canceled,
                DownloadAction::ShowInFolder => DownloadState::Done,
            });
            m.log(usize::MAX, format!("firmware download: {a:?}"));
        }
    })
    .push(|w: &mut DownloadItem, m| {
        let p = m
            .work_orders
            .get()
            .iter()
            .find(|w| w.id == 4473)
            .map(|w| w.progress as f32)
            .unwrap_or(0.0);
        w.set_progress(p);
    });

    let clips = {
        let build = |m: &PlantModel| {
            let mut h = ClipboardHistory::new().max(16).label("identifiers");
            for a in m.assets.get() {
                h.push(a.serial);
            }
            for w in m.work_orders.get() {
                h.push(format!("WO-{}", w.id));
            }
            h
        };
        let mut last = (assets_sig(m), wos_sig(m));
        Bound::new(build(m), m)
            .pull(|w: &mut ClipboardHistory, m| {
                if let Some(i) = w.take_pasted() {
                    if let Some(e) = w.entry(i) {
                        // Paste = filter the registry to the identifier.
                        m.filter_text.set_if_changed(e.text.clone());
                    }
                }
            })
            .push(move |w: &mut ClipboardHistory, m| {
                let s = (assets_sig(m), wos_sig(m));
                if s != last {
                    last = s;
                    *w = build(m);
                }
            })
    };

    let files = Container::new().padding_uniform(4.0).child(
        Masonry::new()
            .columns(2)
            .gap(ZONE_GAP)
            .child(attach)
            .child(download)
            .child(clips)
            .label("files"),
    );

    Flex::column()
        .gap(ZONE_STACK)
        .child(
            row()
                .child_flex(band(BAND_L, json), 1.0)
                .child_flex(band(BAND_L, hex), 1.0)
                .child_flex(band(BAND_L, journal), 1.0),
        )
        .child(band(BAND_L, viewers))
        .child(
            row()
                .child_flex(band(BAND_M, scroller), 1.0)
                .child(indicator),
        )
        .child(files)
}

/// A shift-log entry as a `MessageList` row.
fn log_message(m: &PlantModel, e: &crate::domain::LogEntry) -> Message {
    let sender = if e.author == usize::MAX {
        "SCADA".to_string()
    } else {
        m.crew
            .get()
            .get(e.author)
            .map(|c| c.name)
            .unwrap_or("crew")
            .to_string()
    };
    Message::received(sender, e.text.clone()).time(shift_hhmm(e.minute))
}

// ---------------------------------------------------------------------------
// DIAGNOSTICS — the operator console: a real interpreter over the
// model, a palette of plant actions, console settings, frame timing.
// ---------------------------------------------------------------------------

/// The console's command set — shared by the Terminal interpreter and
/// the help disclosure.
const CONSOLE_HELP: &[&str] = &[
    "status            — plant OEE, line state, alarm counts",
    "alarms            — list active alarms",
    "ack <id>|all      — acknowledge an alarm",
    "sel <asset-id>    — inspect an asset",
    "wo <id> <status>  — move a work order (queued|progress|review|done)",
    "log <text>        — append to the shift log",
    "run / stop        — toggle the line",
    "lock / unlock     — console lock",
];

/// Execute one console command against the model — a real
/// interpreter: reads write output, verbs write the model.
fn exec_console(t: &mut Terminal, cmd: &str, m: &PlantModel) {
    let mut it = cmd.split_whitespace();
    match it.next().unwrap_or("") {
        "" => {}
        "help" | "?" => {
            for l in CONSOLE_HELP {
                t.write(*l);
            }
        }
        "status" => {
            let (i, w, c) = m.alarm_distribution();
            t.write(format!(
                "line {} — plant OEE {:.0}% — {} assets — alarms {i}i/{w}w/{c}c",
                if m.line_running.get() {
                    "RUNNING"
                } else {
                    "STOPPED"
                },
                m.plant_oee() * 100.0,
                m.assets.get().len(),
            ));
        }
        "alarms" => {
            let act = m.active_alarms();
            if act.is_empty() {
                t.write("no active alarms");
            }
            for a in act {
                t.write(format!(
                    "  #{} [{}] {} — {}",
                    a.id,
                    a.severity.label(),
                    m.asset_name(a.asset),
                    a.message
                ));
            }
        }
        "ack" => match it.next() {
            Some("all") => {
                let n = m.active_alarms().len();
                for a in m.active_alarms() {
                    m.ack_alarm(a.id);
                }
                t.write(format!("acked {n} alarm(s)"));
            }
            Some(id) => match id.parse::<u32>() {
                Ok(id) => {
                    m.ack_alarm(id);
                    t.write(format!("acked #{id}"));
                }
                Err(_) => t.write("usage: ack <id|all>"),
            },
            None => t.write("usage: ack <id|all>"),
        },
        "sel" | "inspect" => match it.next().and_then(|s| s.parse::<u32>().ok()) {
            Some(id) => match m.asset(id) {
                Some(a) => {
                    m.selected_asset.set_if_changed(Some(id));
                    t.write(format!("inspecting {} ({})", a.name, a.serial));
                }
                None => t.write(format!("no asset {id}")),
            },
            None => t.write("usage: sel <asset-id>"),
        },
        "wo" => {
            let id = it.next().and_then(|s| s.parse::<u32>().ok());
            let st = it.next().unwrap_or("");
            let status = match st {
                "queued" => Some(WoStatus::Queued),
                "progress" | "inprogress" => Some(WoStatus::InProgress),
                "review" => Some(WoStatus::Review),
                "done" => Some(WoStatus::Done),
                _ => None,
            };
            match (id, status) {
                (Some(id), Some(st)) => {
                    m.move_wo(id, st);
                    t.write(format!("WO-{id} → {}", st.label()));
                }
                _ => t.write("usage: wo <id> <queued|progress|review|done>"),
            }
        }
        "log" => {
            let msg = cmd.strip_prefix("log").unwrap_or("").trim().to_string();
            if msg.is_empty() {
                t.write("usage: log <text>");
            } else {
                m.log(0, msg);
                t.write("logged to shift log");
            }
        }
        "run" => {
            m.line_running.set(true);
            t.write("line started");
        }
        "stop" => {
            m.line_running.set(false);
            t.write("line stopped");
        }
        "lock" => {
            m.console_locked.set(true);
            t.write("console locked");
        }
        "unlock" => {
            m.console_locked.set(false);
            t.write("console unlocked");
        }
        other => t.write(format!("unknown: {other} — try 'help'")),
    }
}

/// A `Switch` two-way bound to a bool signal (toolbar reconcile:
/// widget movement wins, external writes mirror back). The mirror
/// lives in `push` so the mutation reports dirty via `has_push` —
/// a pull-side field write would leave the node clean.
fn bound_switch(model: &PlantModel, label: &'static str, sig: Signal<bool>) -> Bound<Switch> {
    let mut last = sig.get();
    let sig_push = sig.clone();
    Bound::new(Switch::new(label).on(last), model)
        .pull(move |w: &mut Switch, _m| {
            if w.on != last {
                last = w.on;
                sig.set_if_changed(w.on);
            }
        })
        .push(move |w: &mut Switch, _m| {
            let v = sig_push.get();
            if v != last {
                last = v;
                w.on = v;
            }
        })
}

fn diagnostics(m: &PlantModel) -> Flex {
    // The console — echo + interpreter over the model.
    let term = {
        let mut t = Terminal::new()
            .prompt("plant>")
            .label("diagnostics console");
        t.write("plant-east diagnostics — 'help' for commands");
        t.write("type 'status' for the plant summary");
        Bound::new(t, m).pull(|w: &mut Terminal, m| {
            while let Some(cmd) = w.take_submitted() {
                exec_console(w, &cmd, m);
            }
        })
    };

    // Palette — plant actions + per-asset jump targets (re-seated
    // when asset names/serials change).
    let palette = {
        let build = |m: &PlantModel| {
            let mut p = CommandPalette::new()
                .placeholder("type a plant action…")
                .max_results(crate::zone::MAX_SELECTOR_ENTRIES)
                .label("plant actions")
                .actions([
                    CommandAction::new("alarm.ack-all", "Acknowledge all alarms")
                        .keywords(["ack", "clear"]),
                    CommandAction::new("line.toggle", "Toggle line running"),
                    CommandAction::new("console.lock", "Lock the console"),
                    CommandAction::new("wo.new", "Create work order"),
                ]);
            for a in m.assets.get() {
                p = p.action(
                    CommandAction::new(format!("asset.{}", a.id), format!("Inspect {}", a.name))
                        .keywords([a.serial]),
                );
            }
            p
        };
        let mut last = assets_sig(m);
        Bound::new(build(m), m)
            .pull(|w: &mut CommandPalette, m| {
                while let Some(id) = w.take_activated() {
                    if let Some(rest) = id.strip_prefix("asset.") {
                        if let Ok(id) = rest.parse::<u32>() {
                            m.selected_asset.set_if_changed(Some(id));
                        }
                        continue;
                    }
                    match id.as_str() {
                        "alarm.ack-all" => {
                            for a in m.active_alarms() {
                                m.ack_alarm(a.id);
                            }
                        }
                        "line.toggle" => m.line_running.set(!m.line_running.get()),
                        "console.lock" => m.console_locked.set(true),
                        "wo.new" => create_wo(m, WoIntake::default()),
                        _ => {}
                    }
                }
            })
            .push(move |w: &mut CommandPalette, m| {
                let s = assets_sig(m);
                if s != last {
                    last = s;
                    *w = build(m);
                }
            })
    };

    let ack_all = Bound::new(Button::new("Ack all alarms"), m).pull(|w: &mut Button, m| {
        if w.take_activated() {
            for a in m.active_alarms() {
                m.ack_alarm(a.id);
            }
        }
    });

    let help = Disclosure::new("COMMAND REFERENCE")
        .child(Text::new(CONSOLE_HELP.join("\n")).font_size(11.0));

    // Console settings — real HMI flags the rest of the app reads;
    // each row's trailing Switch is two-way bound to its signal.
    let settings = SettingsGroup::new("CONSOLE")
        .row(
            SettingsRow::new("Line running")
                .subtitle("drives the acoustic model + stack light")
                .trailing(bound_switch(m, "line running", m.line_running.clone())),
        )
        .row(
            SettingsRow::new("Console lock")
                .subtitle("security inputs gate the HMI")
                .trailing(bound_switch(m, "console lock", m.console_locked.clone())),
        )
        .row(
            SettingsRow::new("Alert strip")
                .subtitle("toolbar banner")
                .trailing(bound_switch(m, "alerts", m.alerts_on.clone())),
        )
        .row(
            SettingsRow::new("Reduced motion")
                .subtitle("accessibility")
                .trailing(bound_switch(m, "reduced motion", m.reduced_motion.clone())),
        );

    // About — the HMI's identity; credits list the real crew roster.
    let about = Bound::new(
        About::new("Plant East HMI")
            .version("martensite 0.18 — dogfood")
            .comments("Industrial dashboard example")
            .website("project page", "https://github.com/jxoesneon/martensite")
            .credits(
                "Shift crew",
                m.crew.get().iter().map(|c| c.name).collect::<Vec<_>>(),
            ),
        m,
    )
    .pull(|w: &mut About, m| {
        if let Some(url) = w.take_activated_url() {
            m.log(usize::MAX, format!("opened {url}"));
        }
    });

    // Frame cadence of this zone's own tick — real instrumentation.
    let perf = Bound::new(PerfOverlay::new().label("ZONE TICK MS"), m).push({
        let mut last = Instant::now();
        move |p: &mut PerfOverlay, _m| {
            let now = Instant::now();
            p.push_frame(now.duration_since(last).as_secs_f32() * 1000.0);
            last = now;
        }
    });

    Flex::column()
        .gap(ZONE_STACK)
        .child(row().child_flex(band(BAND_L, term), 1.0))
        .child(strip().child_flex(palette, 1.0).child(ack_all).child(help))
        .child(
            row()
                .child_flex(band(BAND_M, settings), 1.0)
                .child_flex(band(BAND_M, about), 1.0),
        )
        .child(band(BAND_S, perf))
}

// ---------------------------------------------------------------------------
// HIERARCHY — relational views of real structure only.
// ---------------------------------------------------------------------------

fn hierarchy(m: &PlantModel) -> Flex {
    // Graph — asset nodes, parent→child edges; hover selects. The
    // Viewport's pan/zoom persists to a view-state signal.
    let vp_state = Signal::new((0.0f32, 0.0f32, 1.0f32));
    let graph = {
        let build = |m: &PlantModel| {
            let assets = m.assets.get();
            let order: Vec<u32> = assets.iter().map(|a| a.id).collect();
            let mut g = GraphView::new().label("asset graph");
            for a in &assets {
                g = g.node(a.name);
            }
            for a in &assets {
                if let (Some(p), Some(to)) = (a.parent, order.iter().position(|i| *i == a.id)) {
                    if let Some(from) = order.iter().position(|i| *i == p) {
                        g = g.edge(from, to);
                    }
                }
            }
            g
        };
        let mut last = assets_sig(m);
        Bound::new(build(m), m)
            .pull(move |w: &mut GraphView, m| {
                if let Some(i) = w.take_hovered().or_else(|| w.take_moved()) {
                    // Derive the row→id map fresh — a `assets_sig`
                    // re-seat rebuilds node order, so a build-time
                    // snapshot would misresolve after edits.
                    if let Some(id) = m.assets.get().get(i).map(|a| a.id) {
                        m.selected_asset.set_if_changed(Some(id));
                    }
                }
            })
            .push(move |w: &mut GraphView, m| {
                let s = assets_sig(m);
                if s != last {
                    last = s;
                    *w = build(m);
                }
            })
    };

    let viewport = {
        let sig = vp_state.clone();
        Bound::new(
            Viewport::new()
                .child(graph)
                .zoom(vp_state.get().2)
                .label("graph canvas"),
            m,
        )
        .pull(move |w: &mut Viewport, _m| {
            if let Some((pan, zoom)) = w.take_changed() {
                sig.set((pan.x, pan.y, zoom));
            }
        })
    };

    // Org chart — the crew roster by `reports_to` (title=name,
    // subtitle=role). Display-only interaction (the widget publishes
    // hover but no selection channel, so nothing commits a WO
    // assignee) but it still reads `m.crew` — the push re-seats it on
    // `crew_sig` so roster edits propagate.
    let org = {
        // `reports_to` is a forest by seed invariant; `seen` still
        // guards the recursion — a corrupted cycle truncates that
        // subtree (and a member reachable twice reports once) rather
        // than overflowing the stack.
        fn crew_node(
            crew: &[crate::domain::CrewMember],
            i: usize,
            seen: &mut HashSet<usize>,
        ) -> OrgNode {
            seen.insert(i);
            let c = &crew[i];
            let mut n = OrgNode::new(c.name, c.role);
            for (j, k) in crew.iter().enumerate() {
                if k.reports_to == Some(i) && !seen.contains(&j) {
                    n = n.child(crew_node(crew, j, seen));
                }
            }
            n
        }
        fn org_tree_of(m: &PlantModel) -> OrgChart {
            let crew = m.crew.get();
            let mut seen = HashSet::new();
            let roots: Vec<usize> = (0..crew.len())
                .filter(|&i| crew[i].reports_to.is_none())
                .collect();
            let root = if roots.len() == 1 {
                crew_node(&crew, roots[0], &mut seen)
            } else {
                // Multiple reporting roots — a synthetic head.
                let mut n = OrgNode::new("Plant crew", "all shifts");
                for i in roots {
                    n = n.child(crew_node(&crew, i, &mut seen));
                }
                n
            };
            OrgChart::new(root).label("crew roster")
        }
        let mut last = crew_sig(m);
        Bound::new(org_tree_of(m), m).push(move |o, m| {
            let s = crew_sig(m);
            if s != last {
                last = s;
                *o = org_tree_of(m);
            }
        })
    };

    // Sunburst — hierarchy weighted by OEE, captioned.
    let sunburst = {
        let build = |m: &PlantModel| {
            let mut s = Sunburst::new().label("oee-weighted hierarchy");
            for site in m.asset_children(None) {
                let mut sn = SunburstNode::new(site.name, (site.oee.max(0.05) * 10.0) as f32);
                for line in m.asset_children(Some(site.id)) {
                    let mut ln = SunburstNode::new(line.name, (line.oee.max(0.05) * 10.0) as f32);
                    for cell in m.asset_children(Some(line.id)) {
                        ln = ln.child(SunburstNode::new(
                            cell.name,
                            (cell.oee.max(0.05) * 10.0) as f32,
                        ));
                    }
                    sn = sn.child(ln);
                }
                s = s.node(sn);
            }
            s
        };
        let mut last = assets_sig(m);
        Bound::new(build(m), m)
            .pull(move |w: &mut Sunburst, m| {
                if let Some(i) = w.take_hovered() {
                    // Resolve the hovered node's name against the live
                    // registry — a re-seat after `assets_sig` changes
                    // rebuilds the ring order, so no build-time id map
                    // can be trusted.
                    let name = w.names().get(i).cloned();
                    if let Some(id) = name.and_then(|n| {
                        m.assets
                            .get()
                            .iter()
                            .find(|a| a.name == n.as_str())
                            .map(|a| a.id)
                    }) {
                        m.selected_asset.set_if_changed(Some(id));
                    }
                }
            })
            .push(move |w: &mut Sunburst, m| {
                let s = assets_sig(m);
                if s != last {
                    last = s;
                    *w = build(m);
                }
            })
    };

    // Sankey — the `material_flow` dataset (tons/hr between lines);
    // re-seated when names or flow weights change.
    let sankey = {
        let build = |m: &PlantModel| {
            let mut s = Sankey::new().label("material flow t/h");
            for id in flow_ids(m) {
                s = s.node(m.asset_name(id));
            }
            for (f, t, v) in m.material_flow.get() {
                s = s.link(m.asset_name(f), m.asset_name(t), v as f32);
            }
            s
        };
        let mut last = (assets_sig(m), flow_sig(m));
        Bound::new(build(m), m).push(move |w: &mut Sankey, m| {
            let s = (assets_sig(m), flow_sig(m));
            if s != last {
                last = s;
                *w = build(m);
            }
        })
    };

    // Mind map — the selected asset decomposed (children = sub-cells,
    // or its fields when it's a leaf).
    let mind = {
        let mut last = (m.selected_asset.get(), assets_sig(m));
        let build = |m: &PlantModel| {
            let a = sel_asset(m);
            let name = a.as_ref().map(|a| a.name).unwrap_or("plant");
            let mut mm = MindMap::new().root(name).label("asset decomposition");
            let kids = m.asset_children(a.as_ref().map(|a| a.id));
            if kids.is_empty() {
                for f in ["Condition", "Work orders", "Schedule", "Note"] {
                    mm = mm.child(name, f);
                }
            } else {
                for c in kids {
                    mm = mm.child(name, c.name);
                }
            }
            mm
        };
        Bound::new(build(m), m).push(move |w: &mut MindMap, m| {
            let sig = (m.selected_asset.get(), assets_sig(m));
            if sig != last {
                last = sig;
                *w = build(m);
            }
        })
    };

    // Fishbone — root-cause of the first active alarm: the effect is
    // the alarm, the bones carry factors from its asset.
    let fish = {
        let mut last = (alarm_sig(m), assets_sig(m));
        let build = |m: &PlantModel| match m.active_alarms().first() {
            Some(al) => {
                let a = m.asset(al.asset);
                Fishbone::new(format!("#{} {}", al.id, al.message))
                    .bone(
                        Bone::new("Machine")
                            .cause(a.as_ref().map(|a| a.serial).unwrap_or("—"))
                            .cause(a.as_ref().map(|a| a.note).unwrap_or("no note")),
                    )
                    .bone(
                        Bone::new("Condition")
                            .cause(a.as_ref().map(|a| a.status.label()).unwrap_or("—"))
                            .cause(format!(
                                "oee {:.0}%",
                                a.as_ref().map(|a| a.oee).unwrap_or(0.0) * 100.0
                            )),
                    )
                    .bone(Bone::new("Severity").cause(al.severity.label()))
                    .label("root cause")
            }
            None => Fishbone::new("no active alarms")
                .bone(Bone::new("—").cause("all clear"))
                .label("root cause"),
        };
        Bound::new(build(m), m).push(move |w: &mut Fishbone, m| {
            let s = (alarm_sig(m), assets_sig(m));
            if s != last {
                last = s;
                *w = build(m);
            }
        })
    };

    Flex::column()
        .gap(ZONE_STACK)
        .child(
            row()
                .child_flex(band(BAND_L, viewport), 2.0)
                .child_flex(band(BAND_L, org), 1.0),
        )
        .child(
            row()
                .child_flex(
                    band(
                        BAND_M,
                        framed(
                            1.0,
                            Stack::new()
                                .alignment(StackAlignment::Center)
                                .child(sunburst)
                                .child(Text::new("OEE-weighted").font_size(11.0)),
                        ),
                    ),
                    1.0,
                )
                .child_flex(band(BAND_M, sankey), 1.0),
        )
        .child(row().child_flex(band(BAND_M, mind), 1.0).child_flex(
            band(BAND_M, Container::new().padding_uniform(4.0).child(fish)),
            1.0,
        ))
}

/// Unique asset ids named by `material_flow` — the Sankey's node set.
fn flow_ids(m: &PlantModel) -> Vec<u32> {
    let mut ids = Vec::new();
    for (f, t, _) in m.material_flow.get() {
        for id in [f, t] {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
    }
    ids
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use martensite::core::Widget;
    use std::time::Duration;

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
    fn pages_are_domain_named_and_bounded() {
        let m = seeded();
        let pages = pages(&m);
        assert!((4..=8).contains(&pages.len()));
        const WIDGET_WORDS: &[&str] = &[
            "TREE", "TABLE", "LIST", "KANBAN", "GANTT", "TERMINAL", "GRID", "TABS", "CHART",
            "WIDGET", "PICKER",
        ];
        for (label, page) in &pages {
            assert!(!label.trim().is_empty());
            for w in WIDGET_WORDS {
                assert!(
                    !label.contains(w),
                    "tab label {label:?} reads like a widget name"
                );
            }
            assert!(page.child_count() > 1, "{label} has no content rows");
        }
    }

    /// Push path: model → TreeView selection; pull path: a widget
    /// selection writes `selected_asset` back.
    #[test]
    fn tree_selection_round_trips() {
        let m = seeded();
        let mut tree = Bound::new(
            TreeView::new()
                .roots(asset_tree(&m, None))
                .label("plant hierarchy"),
            &m,
        )
        .push({
            let mut last = m.selected_asset.get();
            move |w: &mut TreeView, m| {
                let sel = m.selected_asset.get();
                if sel != last {
                    last = sel;
                    if let Some(p) = sel.and_then(|id| asset_path(m, id)) {
                        w.select_path(&p);
                    }
                }
            }
        });

        // Seed selection is asset 7 (Robot-Arm-K7) → path [0,1,0].
        m.selected_asset.set(Some(9));
        tree.tick(Duration::from_millis(16));
        assert_eq!(asset_path(&m, 9), Some(vec![0, 0, 0]));
        assert_eq!(
            m.selected_asset.get(),
            path_asset(&m, tree.inner().selected_path().unwrap_or(&[]))
        );
    }

    /// The console interpreter acks and logs for real.
    #[test]
    fn console_drives_the_model() {
        let m = seeded();
        let mut t = Terminal::new().prompt("plant>");
        exec_console(&mut t, "ack 101", &m);
        assert!(m.alarms.get().iter().find(|a| a.id == 101).unwrap().acked);
        let n = m.shift_log.get().len();
        exec_console(&mut t, "log hello plant", &m);
        assert_eq!(m.shift_log.get().len(), n + 1);
        exec_console(&mut t, "sel 12", &m);
        assert_eq!(m.selected_asset.get(), Some(12));
        exec_console(&mut t, "wo 4470 done", &m);
        assert_eq!(
            m.work_orders
                .get()
                .iter()
                .find(|w| w.id == 4470)
                .unwrap()
                .status,
            WoStatus::Done
        );
    }
}
