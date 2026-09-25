//! Editor zone pages — the panel's functional surfaces per the Design
//! Council verdict (docket `20260921`): no exhibit walls, every mounted
//! widget reads [`PlantModel`] state (`push`) or publishes interaction
//! back (`pull`) through [`Bound`].
//!
//! Pages (domain-named, never widget-named):
//!
//! - **WORK ORDER FORM** — the inspector: every field of the selected
//!   `WorkOrder` editable through `m.update_wo`, write-through with an
//!   explicit VALIDATE that reports to the shift log.
//! - **SCHEDULING** — the selected WO's `due_day` (three views of one
//!   field, kept in sync) plus the linked `MaintTask` window and the
//!   simulated shift clock.
//! - **APPEARANCE** — console preferences: `editor_font_pt`,
//!   `editor_autosave`, `editor_wrap`, `reduced_motion`, `alerts_on`,
//!   `tour_seen`, and the full color suite all editing the single
//!   `editor_accent` (that they stay in sync *is* the demo).
//! - **COMMAND SURFACE** — one primary command system (`MenuBar` +
//!   `Toolbar`) plus a `Tabs` selector mounting ONE alternate surface
//!   at a time (`CommandPalette`/`RadialMenu`/`SpeedDial`/`FloatButton`/
//!   `Ribbon`) — every surface dispatches the same action set.
//! - **CONSOLE LOCK** — the security inputs' honest role: locking and
//!   unlocking the HMI console (`console_locked`).
//! - **ANNOTATION & SIGN-OFF** — per-asset inspection markup, photo
//!   crop memory, and supervisor sign-off that moves the WO to Review.
//! - **SCAN & LOOKUP** — serial-lookup write path into `selected_asset`,
//!   asset-identity codes, config import.
//!
//! CUT: ChessBoard, ChessClock, Confetti, PricingTable, ScratchCard
//!      (council cut list — nothing to bind), SignaturePad (not in the
//!      widget set — `InkCanvas` covers sign-off), SvgView (no such
//!      widget — the schematic idea is unbuildable), QrScanner /
//!      BarcodeScanner (no scanner widgets — `SearchField` serial
//!      lookup is the honest scan path; `QrCode`/`Barcode` display the
//!      identity instead), XYPad (PTZ belongs to the media panel),
//!      GradientEditor (a gradient is not a single accent — the suite
//!      edits `editor_accent` only), MonthPicker / YearPicker /
//!      DateRangePicker / TimeInput / NumberInput / ChecklistWidget /
//!      Form / LabeledControl (don't exist — `DatePicker::range_mode`,
//!      `Calendar`, `TimePicker`, `SpinBox`, `CheckList`, `FormField`
//!      cover), FontPicker / ColorField / SwatchPicker / LockIndicator /
//!      PasswordInput / TagInput / ActionBar / ButtonGroup /
//!      AnnotationCanvas (don't exist — `FontButton`, `ColorButton`,
//!      `ColorPalette`, `StatusDot`, `TextInput::secure`, `TokenField`,
//!      a `Bound<Button>` row, `InkCanvas` cover), AlarmPanel (an alarm
//!      *view* — the editor's alarm role is the ack *commands* that
//!      operate on it).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use martensite::core::Widget;
use martensite::reactive::Signal;
use martensite::widgets::alpha_slider::AlphaSlider;
use martensite::widgets::barcode::Barcode;
use martensite::widgets::button::Button;
use martensite::widgets::calendar::Calendar;
use martensite::widgets::cascader::{Cascader, CascaderOption};
use martensite::widgets::check_list::{CheckItem, CheckList};
use martensite::widgets::checkbox::CheckBox;
use martensite::widgets::color_button::ColorButton;
use martensite::widgets::color_palette::ColorPalette;
use martensite::widgets::color_picker::{rgb_to_hsv, Color, ColorPicker};
use martensite::widgets::color_wheel::ColorWheel;
use martensite::widgets::command_link::CommandLink;
use martensite::widgets::command_palette::{CommandAction, CommandPalette};
use martensite::widgets::context_menu::ContextMenu;
use martensite::widgets::copyable::Copyable;
use martensite::widgets::crop_box::CropBox;
use martensite::widgets::date_picker::{Date, DatePicker};
use martensite::widgets::descriptions::Descriptions;
use martensite::widgets::dropdown::Dropdown;
use martensite::widgets::file_chooser_button::{ChooserMode, FileChooserButton};
use martensite::widgets::flex::Flex;
use martensite::widgets::float_button::FloatButton;
use martensite::widgets::font_button::FontButton;
use martensite::widgets::form_field::{FormField, LabelPosition};
use martensite::widgets::group_box::GroupBox;
use martensite::widgets::hue_slider::HueSlider;
use martensite::widgets::ink_canvas::InkCanvas;
use martensite::widgets::inline_edit::InlineEdit;
use martensite::widgets::keypad::Keypad;
use martensite::widgets::list_view::{ListView, SelectionMode};
use martensite::widgets::menu::MenuItem;
use martensite::widgets::menu_bar::MenuBar;
use martensite::widgets::nav_rail::NavRail;
use martensite::widgets::otp_input::OtpInput;
use martensite::widgets::password_strength::PasswordStrength;
use martensite::widgets::pattern_lock::PatternLock;
use martensite::widgets::popconfirm::{ConfirmResult, Popconfirm};
use martensite::widgets::qr_code::QrCode;
use martensite::widgets::radial_menu::RadialMenu;
use martensite::widgets::radio::RadioGroup;
use martensite::widgets::ribbon::Ribbon;
use martensite::widgets::search_field::SearchField;
use martensite::widgets::segmented::Segmented;
use martensite::widgets::slider::Slider;
use martensite::widgets::speed_dial::SpeedDial;
use martensite::widgets::spinbox::SpinBox;
use martensite::widgets::status_dot::{Status as DotStatus, StatusDot};
use martensite::widgets::switch::Switch;
use martensite::widgets::text::Text;
use martensite::widgets::text_area::TextArea;
use martensite::widgets::text_input::TextInput;
use martensite::widgets::time_picker::{Time, TimePicker};
use martensite::widgets::token_field::TokenField;
use martensite::widgets::toolbar::Toolbar;
use martensite::widgets::wheel_picker::WheelPicker;
use parking_lot::Mutex;

use crate::domain::{Asset, MaintTask, PlantModel, WoPriority, WoStatus, WorkOrder};
use crate::zone::{
    band, framed, row, strip, Bound, Page, Swap, Variant, BAND_M, BAND_S, ZONE_GAP, ZONE_STACK,
};
use martensite::core::widget::DummyWidget;

// ---------------------------------------------------------------------------
// Shared binding helpers.
// ---------------------------------------------------------------------------

/// Shared last-synced slot — the direction-aware reconcile the toolbar
/// uses (`reconcile`/`publish`): `pull` publishes only when the widget
/// moved off the slot, `push` reflects only when the model did. A stale
/// side can never stomp a fresh write.
type Slot<T> = Arc<Mutex<T>>;

fn slot<T>(v: T) -> Slot<T> {
    Arc::new(Mutex::new(v))
}

/// The selected work order, cloned out of the store — reads go through
/// this, writes through `m.update_wo`.
fn sel_wo(m: &PlantModel) -> Option<WorkOrder> {
    let sel = m.selected_wo.get()?;
    m.work_orders.get().into_iter().find(|w| w.id == sel)
}

/// WO titles are `&'static str` (the seed table is static); an edited
/// title is interned for the app's lifetime — the sim's title store.
fn intern(s: &str) -> &'static str {
    Box::leak(s.to_string().into_boxed_str())
}

/// A labeled, left-aligned form row — the form looks like a form.
fn field(label: &'static str, control: impl Widget + 'static) -> FormField {
    FormField::new()
        .label(label)
        .label_position(LabelPosition::Left)
        .label_width(110.0)
        .child(control)
}

/// A hint line — static chrome, not a bound widget.
fn hint(text: &'static str) -> Text {
    Text::new(text).font_size(12.0)
}

/// A live text line: `push` rewrites content only on change.
fn live_text(
    model: &PlantModel,
    f: impl Fn(&PlantModel) -> String + Send + Sync + 'static,
) -> Bound<Text> {
    Bound::new(Text::new(f(model)).font_size(12.0), model).push(move |w, m| {
        let s = f(m);
        if w.content() != s {
            w.set_content(s);
        }
    })
}

const WEEKDAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
const STATUS_COLS: [WoStatus; 4] = [
    WoStatus::Queued,
    WoStatus::InProgress,
    WoStatus::Review,
    WoStatus::Done,
];
const PRIORITIES: [WoPriority; 4] = [
    WoPriority::Low,
    WoPriority::Medium,
    WoPriority::High,
    WoPriority::Critical,
];
/// The seeded accent — ColorButton's "restore default" target.
const ACCENT_DEFAULT: [u8; 4] = [96, 165, 250, 255];

// ---------------------------------------------------------------------------
// Civil date math — the schedule model stores *day offsets*, pickers
// store absolute `Date`s. `days_from_civil`/`civil_from_days` (Howard
// Hinnant's algorithm) convert between them against REF_MONDAY.
// ---------------------------------------------------------------------------

/// The week the WO `due_day` field indexes into (0 = Mon of this week).
/// 2024-06-03 is a Monday (`Date::weekday_of(2024, 6, 3) == 1`).
const REF_MONDAY: Date = Date {
    year: 2024,
    month: 6,
    day: 3,
};
/// Shift A starts 06:00 — `shift_minute` counts from there.
const SHIFT_START_MIN: u32 = 6 * 60;

fn daynum(d: Date) -> i64 {
    let y = i64::from(d.year) - i64::from(d.month <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (i64::from(d.month) + 9) % 12;
    let doy = (153 * mp + 2) / 5 + i64::from(d.day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn date_from_daynum(z: i64) -> Date {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = mp + 3 - 12 * (mp / 10);
    Date {
        year: (y + i64::from(mo <= 2)) as i32,
        month: mo as u32,
        day: d as u32,
    }
}

/// `due_day` (0=Mon) ↔ a `Date` inside the reference week.
fn due_date(due_day: u8) -> Date {
    date_from_daynum(daynum(REF_MONDAY) + i64::from(due_day.min(6)))
}

/// The schedule task the window picker edits — the open `MaintTask`
/// on the selected WO's asset (first open, else first on the asset).
fn wo_task(m: &PlantModel) -> Option<MaintTask> {
    let wo = sel_wo(m)?;
    let sched = m.schedule.get();
    sched
        .iter()
        .find(|t| t.asset == wo.asset && !t.done)
        .or_else(|| sched.iter().find(|t| t.asset == wo.asset))
        .cloned()
}

/// `#tag` tokens live in the WO notes tail — the model has no label
/// field, so TokenField edits the notes' hashtag run (visible in the
/// notes editor — a real write path, not a parallel store).
fn note_tags(notes: &str) -> Vec<String> {
    notes
        .split_whitespace()
        .filter_map(|t| t.strip_prefix('#'))
        .map(str::to_string)
        .collect()
}

// ---------------------------------------------------------------------------
// Command dispatch — the one action set every command surface fires.
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum Cmd {
    AckAll,
    ToggleLine,
    LockConsole,
    NewWo,
    TripLine,
    Diagnose,
    InspectAsset,
    AckAssetAlarms,
}

fn dispatch(m: &PlantModel, cmd: Cmd) {
    match cmd {
        Cmd::AckAll => {
            let active = m.active_alarms();
            let n = active.len();
            for a in active {
                m.ack_alarm(a.id);
            }
            m.log(usize::MAX, format!("all alarms acknowledged ({n})"));
        }
        Cmd::ToggleLine => {
            let on = !m.line_running.get();
            m.line_running.set(on);
            m.log(usize::MAX, if on { "line started" } else { "line stopped" });
        }
        Cmd::LockConsole => {
            m.console_locked.set_if_changed(true);
            m.log(usize::MAX, "HMI console locked");
        }
        Cmd::NewWo => {
            let mut wos = m.work_orders.get();
            let id = wos.iter().map(|w| w.id).max().unwrap_or(4470) + 1;
            wos.push(WorkOrder {
                id,
                title: intern(&format!("WO-{id} operator request")),
                asset: m.selected_asset.get().unwrap_or(7),
                status: WoStatus::Queued,
                priority: WoPriority::Medium,
                assignee: 0,
                checklist: vec![("Triage", false)],
                due_day: 0,
                progress: 0.0,
                notes: String::new(),
                signature: None,
                photo: None,
            });
            m.work_orders.set(wos);
            m.selected_wo.set(Some(id));
            m.log(usize::MAX, format!("WO-{id} created from command surface"));
        }
        Cmd::TripLine => {
            m.line_running.set_if_changed(false);
            m.log(usize::MAX, "LINE TRIP — operator confirmed stop");
        }
        Cmd::Diagnose => {
            let name = m
                .selected_asset
                .get()
                .map(|id| m.asset_name(id))
                .unwrap_or("—");
            m.log(usize::MAX, format!("diagnostics started on {name}"));
        }
        Cmd::InspectAsset => {
            let name = m
                .selected_asset
                .get()
                .map(|id| m.asset_name(id))
                .unwrap_or("—");
            m.log(usize::MAX, format!("inspection opened for {name}"));
        }
        Cmd::AckAssetAlarms => {
            let Some(id) = m.selected_asset.get() else {
                return;
            };
            let mut n = 0;
            for a in m.alarms.get() {
                if a.asset == id && a.active && !a.acked {
                    m.ack_alarm(a.id);
                    n += 1;
                }
            }
            m.log(
                usize::MAX,
                format!("{} asset alarms acknowledged ({n})", m.asset_name(id)),
            );
        }
    }
}

// ---------------------------------------------------------------------------
// WORK ORDER FORM — the inspector. Every field writes `update_wo`.
// ---------------------------------------------------------------------------

fn work_order_form(model: &PlantModel) -> Page {
    // Live inspector card — rebuilt only when the shown summary moves.
    let header = Bound::new(
        Descriptions::new()
            .title("INSPECTED WORK ORDER")
            .bordered(true),
        model,
    )
    .push({
        let last = slot(String::new());
        move |w, m| {
            let Some(wo) = sel_wo(m) else { return };
            // `asset` + its resolved name ride the key — a property-
            // grid rename must invalidate the ASSET row too.
            let summary = format!(
                "{}|{}|{}|{}|{}|{}|{}|{:.3}",
                wo.id,
                wo.status.label(),
                wo.priority.label(),
                wo.assignee,
                wo.due_day,
                wo.asset,
                m.asset_name(wo.asset),
                wo.progress
            );
            let mut g = last.lock();
            if *g == summary {
                return;
            }
            *g = summary;
            *w = Descriptions::new()
                .title("INSPECTED WORK ORDER")
                .bordered(true)
                .item("WO", format!("WO-{}", wo.id))
                .item("ASSET", m.asset_name(wo.asset))
                .item("STATUS", wo.status.label())
                .item("PRIORITY", wo.priority.label())
                .item("DUE", WEEKDAYS[wo.due_day as usize % 7])
                .item("PROGRESS", format!("{:.0}%", wo.progress * 100.0));
        }
    });

    let wo0 = sel_wo(model);

    // Title — write-through; the title store interns edited strings.
    let title = Bound::new(
        TextInput::new("title")
            .placeholder("work order title")
            .value(wo0.as_ref().map(|w| w.title).unwrap_or_default()),
        model,
    )
    .pull(|w: &mut TextInput, m| {
        if w.take_edited() {
            let v = w.value.clone();
            m.update_wo(|wo| wo.title = intern(&v));
        }
    })
    .push(|w, m| {
        if let Some(wo) = sel_wo(m) {
            if wo.title != w.value {
                w.set_value(wo.title);
            }
        }
    });

    // Quick title edit — the InlineEdit's commit seam; stays in sync
    // with the TextInput through the same field.
    let quick = Bound::new(
        InlineEdit::new(wo0.as_ref().map(|w| w.title).unwrap_or_default())
            .placeholder("click to rename"),
        model,
    )
    .pull(|w: &mut InlineEdit, m| {
        if let Some((_old, new)) = w.take_committed() {
            m.update_wo(|wo| wo.title = intern(&new));
        }
    })
    .push(|w, m| {
        if let Some(wo) = sel_wo(m) {
            if !w.is_editing() && w.value != wo.title {
                w.value = wo.title.to_string();
            }
        }
    });

    // Status — Segmented drains `take_selected`; the echo (push →
    // set_selected → parked change) is filtered by comparing against
    // the model before writing.
    let status_sel = wo0
        .as_ref()
        .map(|w| STATUS_COLS.iter().position(|s| *s == w.status).unwrap_or(0))
        .unwrap_or(0);
    let status = Bound::new(
        Segmented::new()
            .options(STATUS_COLS.iter().map(|s| s.label()))
            .selected(status_sel),
        model,
    )
    .pull(|w: &mut Segmented, m| {
        while let Some(i) = w.take_selected() {
            if let Some(st) = STATUS_COLS.get(i) {
                if sel_wo(m).map(|wo| wo.status) != Some(*st) {
                    m.update_wo(|wo| wo.status = *st);
                }
            }
        }
    })
    .push(|w, m| {
        if let Some(wo) = sel_wo(m) {
            let idx = STATUS_COLS
                .iter()
                .position(|s| *s == wo.status)
                .unwrap_or(0);
            if w.selected_index() != idx {
                w.set_selected(idx);
            }
        }
    });

    // Priority — RadioGroup has no drain; the slot tracks direction.
    let prio_sel = wo0
        .as_ref()
        .map(|w| {
            PRIORITIES
                .iter()
                .position(|p| *p == w.priority)
                .unwrap_or(0)
        })
        .unwrap_or(0);
    let priority = Bound::new(
        RadioGroup::new(PRIORITIES.iter().map(|p| p.label())).label("priority"),
        model,
    )
    .pull({
        let last = slot(prio_sel);
        move |w: &mut RadioGroup, m| {
            let v = w.selected();
            let mut g = last.lock();
            if v != *g {
                *g = v;
                if let Some(p) = PRIORITIES.get(v) {
                    m.update_wo(|wo| wo.priority = *p);
                }
            }
        }
    })
    .push({
        let last = slot(prio_sel);
        move |w, m| {
            if let Some(wo) = sel_wo(m) {
                let idx = PRIORITIES
                    .iter()
                    .position(|p| *p == wo.priority)
                    .unwrap_or(0);
                let mut g = last.lock();
                if idx != *g {
                    *g = idx;
                    w.select(idx);
                }
            }
        }
    });

    // Assignee — Dropdown over the live crew roster.
    let crew: Vec<String> = model
        .crew
        .get()
        .iter()
        .map(|c| c.name.to_string())
        .collect();
    let assignee_sel = wo0
        .as_ref()
        .map(|w| w.assignee)
        .unwrap_or(0)
        .min(crew.len().saturating_sub(1));
    let mut assignee_dd = Dropdown::new(crew.clone()).label("assignee");
    assignee_dd.commit(assignee_sel);
    let crew_n = crew.len();
    let assignee = Bound::new(assignee_dd, model)
        .pull({
            let last = slot(assignee_sel);
            move |w: &mut Dropdown, m| {
                let v = w.selected().min(crew_n.saturating_sub(1));
                let mut g = last.lock();
                if v != *g {
                    *g = v;
                    m.update_wo(|wo| wo.assignee = v);
                }
            }
        })
        .push({
            let last = slot(assignee_sel);
            move |w, m| {
                let Some(wo) = sel_wo(m) else { return };
                let idx = wo.assignee.min(crew_n.saturating_sub(1));
                let mut g = last.lock();
                if idx != *g && !w.is_open() {
                    *g = idx;
                    w.commit(idx);
                }
            }
        });

    // Due weekday — SpinBox 0=Mon..6=Sun (the pickers page owns the
    // calendar views of the same field).
    let due_sel = f64::from(wo0.as_ref().map(|w| w.due_day).unwrap_or(0));
    let due = Bound::new(
        SpinBox::new()
            .range(0.0, 6.0)
            .step(1.0)
            .with_value(due_sel)
            .label("due day"),
        model,
    )
    .pull({
        let last = slot(due_sel);
        move |w: &mut SpinBox, m| {
            let v = w.value();
            let mut g = last.lock();
            if (v - *g).abs() > 0.5 {
                *g = v;
                m.update_wo(|wo| wo.due_day = v.round().clamp(0.0, 6.0) as u8);
            }
        }
    })
    .push({
        let last = slot(due_sel);
        move |w, m| {
            if let Some(wo) = sel_wo(m) {
                let v = f64::from(wo.due_day);
                let mut g = last.lock();
                if (v - *g).abs() > 0.5 && !w.is_editing() && !w.is_pressed() {
                    *g = v;
                    w.set_value(v);
                }
            }
        }
    });

    // Progress — Slider % write-through, drag-guarded reflect.
    let prog_sel = wo0.as_ref().map(|w| w.progress * 100.0).unwrap_or(0.0);
    let progress = progress_slider(model, prog_sel);

    // Checklist — take_changed drains toggle marks; a WO switch
    // rebuilds rows wholesale.
    let mut cl = CheckList::new().label("checklist");
    if let Some(wo) = &wo0 {
        for (l, d) in &wo.checklist {
            cl = cl.item(CheckItem::new(*l).checked(*d));
        }
    }
    let checklist = Bound::new(cl, model)
        .pull(|w: &mut CheckList, m| {
            while let Some((i, done)) = w.take_changed() {
                m.update_wo(|wo| {
                    if let Some(item) = wo.checklist.get_mut(i) {
                        item.1 = done;
                    }
                });
            }
        })
        .push(|w, m| {
            let Some(wo) = sel_wo(m) else { return };
            let wrong_rows = w.item_count() != wo.checklist.len()
                || wo
                    .checklist
                    .iter()
                    .enumerate()
                    .any(|(i, (l, _))| w.item_at(i).is_none_or(|it| it.label != *l));
            if wrong_rows {
                let mut nl = CheckList::new().label("checklist");
                for (l, d) in &wo.checklist {
                    nl = nl.item(CheckItem::new(*l).checked(*d));
                }
                *w = nl;
            } else {
                for (i, (_, d)) in wo.checklist.iter().enumerate() {
                    if w.is_checked(i) != *d {
                        w.set_checked(i, *d);
                    }
                }
            }
        });

    // Notes — TextArea writes the String field directly.
    let notes = Bound::new(
        TextArea::new()
            .label("notes")
            .placeholder("operator notes…")
            .with_value(wo0.as_ref().map(|w| w.notes.clone()).unwrap_or_default())
            .min_lines(3),
        model,
    )
    .pull(|w: &mut TextArea, m| {
        if w.take_edited().is_some() {
            let v = w.value();
            m.update_wo(|wo| wo.notes = v);
        }
    })
    .push(|w, m| {
        if let Some(wo) = sel_wo(m) {
            if wo.notes != w.value() {
                w.set_value(wo.notes.clone());
                w.take_edited(); // set_value flags edited — drain the echo
            }
        }
    });

    // Tags — TokenField edits the notes' `#tag` run (the model has no
    // label field; hashtags in notes are the honest store).
    let init_tags = wo0
        .as_ref()
        .map(|w| note_tags(&w.notes))
        .unwrap_or_default();
    let tags = Bound::new(
        TokenField::new().tokens(init_tags).placeholder("add #tag…"),
        model,
    )
    .pull(|w: &mut TokenField, m| {
        while let Some(t) = w.take_added() {
            let tag = t.trim().trim_start_matches('#').to_string();
            if tag.is_empty() {
                continue;
            }
            m.update_wo(|wo| {
                let marker = format!("#{tag}");
                if !wo.notes.split_whitespace().any(|x| x == marker) {
                    if !wo.notes.is_empty() {
                        wo.notes.push(' ');
                    }
                    wo.notes.push_str(&marker);
                }
            });
        }
        while let Some(t) = w.take_removed() {
            let marker = format!("#{}", t.trim_start_matches('#'));
            m.update_wo(|wo| {
                wo.notes = wo
                    .notes
                    .split_whitespace()
                    .filter(|x| *x != marker)
                    .collect::<Vec<_>>()
                    .join(" ");
            });
        }
    })
    .push(|w, m| {
        if let Some(wo) = sel_wo(m) {
            let tags = note_tags(&wo.notes);
            if w.token_list() != tags.as_slice() {
                *w = TokenField::new().tokens(tags).placeholder("add #tag…");
            }
        }
    });

    // Actions — write-through is immediate, so the buttons validate,
    // transition, and file to the shift log.
    let validate = Bound::new(
        Button::new("VALIDATE").tooltip("check the WO + file result to the shift log"),
        model,
    )
    .pull(|w: &mut Button, m| {
        if w.take_activated() {
            if let Some(wo) = sel_wo(m) {
                let mut issues: Vec<String> = Vec::new();
                if wo.title.trim().is_empty() {
                    issues.push("title empty".into());
                }
                let open = wo.checklist.iter().filter(|(_, d)| !d).count();
                if open > 0 {
                    issues.push(format!("{open} checklist items open"));
                }
                if wo.status == WoStatus::Done && wo.progress < 1.0 {
                    issues.push("done but progress < 100%".into());
                }
                let msg = if issues.is_empty() {
                    "OK".to_string()
                } else {
                    issues.join(", ")
                };
                m.log(wo.assignee, format!("WO-{} validate: {msg}", wo.id));
            }
        }
    });
    let done = Bound::new(Button::new("MARK DONE"), model).pull(|w: &mut Button, m| {
        if w.take_activated() {
            if let Some(wo) = sel_wo(m) {
                let id = wo.id;
                m.update_wo(|w| w.status = WoStatus::Done);
                m.log(wo.assignee, format!("WO-{id} marked done"));
            }
        }
    });
    let reopen = Bound::new(Button::new("REOPEN"), model).pull(|w: &mut Button, m| {
        if w.take_activated() {
            if let Some(wo) = sel_wo(m) {
                let id = wo.id;
                m.update_wo(|w| w.status = WoStatus::Queued);
                m.log(wo.assignee, format!("WO-{id} reopened"));
            }
        }
    });

    let primary = Flex::column()
        .gap(ZONE_STACK)
        .child(header)
        .child(
            strip()
                .child(field("TITLE", title))
                .child(field("QUICK EDIT", quick))
                .child_flex(DummyWidget, 1.0),
        )
        .child(
            strip()
                .child(field("STATUS", status))
                .child(field("PRIORITY", priority))
                .child_flex(DummyWidget, 1.0),
        )
        .child(
            strip()
                .child(field("ASSIGNEE", assignee))
                .child(field("DUE DAY", due))
                .child_flex(DummyWidget, 1.0),
        )
        .child(strip().child_flex(field("PROGRESS", progress), 1.0))
        .child(
            strip()
                .child(field("CHECKLIST", checklist))
                .child(field("TAGS", tags))
                .child_flex(DummyWidget, 1.0),
        )
        .child(strip().child_flex(field("NOTES", notes), 1.0))
        .child(
            strip()
                .child(validate)
                .child(done)
                .child(reopen)
                .child_flex(DummyWidget, 1.0),
        );
    Page::new(
        Variant::MasterDetail,
        // A form column taller than a short zone — scroll-mounted so
        // the footer buttons never crush to zero.
        crate::zone::fill(crate::zone::scroll(primary)),
        &model.zone_width[2],
    )
    .rail("Work order", wo_rail(model))
}

/// The progress slider's binding, factored out so tests can drive the
/// same pull the page mounts.
fn progress_slider(model: &PlantModel, initial: f64) -> Bound<Slider> {
    Bound::new(
        Slider::new(0.0, 100.0)
            .label("progress %")
            .with_value(initial)
            .step(1.0),
        model,
    )
    .pull({
        let last = slot(initial);
        move |s: &mut Slider, m| {
            let v = s.value();
            let mut g = last.lock();
            if (v - *g).abs() > 0.001 {
                *g = v;
                m.update_wo(|w| w.progress = v / 100.0);
            }
        }
    })
    .push({
        let last = slot(initial);
        move |s, m| {
            if let Some(wo) = sel_wo(m) {
                let v = wo.progress * 100.0;
                let mut g = last.lock();
                if (v - *g).abs() > 0.001 && !s.is_dragging() {
                    *g = v;
                    s.set_value(v);
                }
            }
        }
    })
}

// ---------------------------------------------------------------------------
// SCHEDULING — due_day views, task window, shift clock.
// ---------------------------------------------------------------------------

fn scheduling(model: &PlantModel) -> Page {
    let wo0 = sel_wo(model);
    let due0 = wo0.as_ref().map(|w| w.due_day).unwrap_or(0);

    // Due date — absolute pick, folded back to a weekday index.
    let date = Bound::new(
        DatePicker::new()
            .date(due_date(due0))
            .today(REF_MONDAY)
            .label("due date"),
        model,
    )
    .pull(|w: &mut DatePicker, m| {
        if let Some(d) = w.take_selected() {
            let dd = (daynum(d) - daynum(REF_MONDAY)).clamp(0, 6) as u8;
            m.update_wo(|wo| wo.due_day = dd);
        }
    })
    .push(|w, m| {
        if let Some(wo) = sel_wo(m) {
            let want = due_date(wo.due_day);
            if w.get_date() != Some(want) && !w.is_open() {
                w.set_date(Some(want));
            }
        }
    });

    // Weekday wheel — the same field through a drum picker.
    let wheel = Bound::new(
        WheelPicker::new()
            .items(WEEKDAYS)
            .selected_index(due0 as usize)
            .label("weekday"),
        model,
    )
    .pull(|w: &mut WheelPicker, m| {
        while let Some(i) = w.take_selected() {
            if sel_wo(m).map(|wo| wo.due_day as usize) != Some(i) {
                m.update_wo(|wo| wo.due_day = i.min(6) as u8);
            }
        }
    })
    .push(|w, m| {
        if let Some(wo) = sel_wo(m) {
            let i = wo.due_day as usize;
            if w.selected() != i {
                w.select(i.min(6));
            }
        }
    });

    // Month calendar — a third view of due_day (week of the pick
    // lands inside the reference week).
    let cal = Bound::new(
        Calendar::new()
            .date(due_date(due0))
            .today(REF_MONDAY)
            .week_starts_monday(true)
            .label("calendar"),
        model,
    )
    .pull(|w: &mut Calendar, m| {
        if let Some(d) = w.take_selected() {
            let dd = (daynum(d) - daynum(REF_MONDAY)).clamp(0, 6) as u8;
            m.update_wo(|wo| wo.due_day = dd);
        }
    })
    .push(|w, m| {
        if let Some(wo) = sel_wo(m) {
            let want = due_date(wo.due_day);
            if w.selected() != Some(want) {
                w.set_date(Some(want));
            }
        }
    });

    // Task window — range pick edits the open MaintTask on the WO's
    // asset (start_day/days, offsets from REF_MONDAY as "today").
    let task0 = wo_task(model);
    let range = Bound::new(
        {
            let mut dp = DatePicker::new()
                .range_mode(true)
                .today(REF_MONDAY)
                .label("task window");
            if let Some(t) = &task0 {
                dp.set_range((
                    date_from_daynum(daynum(REF_MONDAY) + i64::from(t.start_day)),
                    date_from_daynum(
                        daynum(REF_MONDAY) + i64::from(t.start_day) + i64::from(t.days) - 1,
                    ),
                ));
            }
            dp
        },
        model,
    )
    .pull(|w: &mut DatePicker, m| {
        if let Some((a, b)) = w.take_range() {
            if let Some(t) = wo_task(m) {
                let tid = t.id;
                let start = (daynum(a) - daynum(REF_MONDAY)).clamp(0, 13) as u8;
                let days = ((daynum(b) - daynum(a)).abs() + 1).clamp(1, 14) as u8;
                let mut sched = m.schedule.get();
                if let Some(t) = sched.iter_mut().find(|t| t.id == tid) {
                    t.start_day = start;
                    t.days = days;
                }
                m.schedule.set_if_changed(sched);
            }
        }
    })
    .push(move |w, m| {
        if let Some(t) = wo_task(m) {
            let want = (
                date_from_daynum(daynum(REF_MONDAY) + i64::from(t.start_day)),
                date_from_daynum(
                    daynum(REF_MONDAY) + i64::from(t.start_day) + i64::from(t.days) - 1,
                ),
            );
            if w.range_value() != Some(want) && !w.is_open() {
                w.set_range(want);
            }
        }
    });

    // Shift clock — TimePicker adjusts the simulated plant clock.
    let clock_of = |m: &PlantModel| -> Time {
        let t = SHIFT_START_MIN + m.shift_minute.get().min(16 * 60);
        Time {
            hour: (t / 60) % 24,
            minute: t % 60,
        }
    };
    let clock = Bound::new(
        TimePicker::new()
            .time(clock_of(model))
            .use_24h(true)
            .minute_step(15)
            .label("shift clock"),
        model,
    )
    .pull(|w: &mut TimePicker, m| {
        if let Some(t) = w.take_edited() {
            let mins = t.hour * 60 + t.minute;
            m.shift_minute
                .set_if_changed(mins.saturating_sub(SHIFT_START_MIN));
        }
    })
    .push(move |w, m| {
        let want = clock_of(m);
        if w.get_time() != want {
            w.set_time(want);
        }
    });

    let task_note = live_text(model, |m| {
        let Some(wo) = sel_wo(m) else {
            return "no work order selected".into();
        };
        match wo_task(m) {
            Some(t) => format!("window edits task #{} on {}", t.id, m.asset_name(wo.asset)),
            None => format!("no scheduled task on {}", m.asset_name(wo.asset)),
        }
    });

    let primary = Flex::column()
        .gap(ZONE_STACK)
        .child(hint(
            "due date, weekday drum, and the month grid edit one field — selected WO due_day",
        ))
        .child(
            strip()
                .child(field("DUE DATE", date))
                .child(field("WEEKDAY", wheel))
                .child_flex(DummyWidget, 1.0),
        )
        .child(row().child_flex(field("MONTH", band(BAND_M, cal)), 1.0))
        .child(task_note)
        .child(
            strip()
                .child(field("TASK WINDOW", range))
                .child(field("SHIFT CLOCK", clock))
                .child_flex(DummyWidget, 1.0),
        );
    Page::new(
        Variant::MasterDetail,
        // Form column — scroll-mounted so a short zone scrolls rather
        // than crushing the trailing rows.
        crate::zone::fill(crate::zone::scroll(primary)),
        &model.zone_width[2],
    )
    .rail("Work order", wo_rail(model))
}

// ---------------------------------------------------------------------------
// APPEARANCE — console preferences + the accent color suite.
// ---------------------------------------------------------------------------

/// APPEARANCE — "how does this console look and behave?" MasterLeft:
/// a section rail (Typography | Behavior | Accent) selects the
/// detail surface via `editor_section`; the accent suite edits one
/// `editor_accent` signal so every picker stays in sync.
fn appearance(model: &PlantModel) -> Page {
    let font0 = model.editor_font_pt.get();

    // Font size — two controls on one signal (they stay in sync).
    let font_spin = Bound::new(
        SpinBox::new()
            .range(8.0, 24.0)
            .step(0.5)
            .with_value(font0)
            .suffix(" pt"),
        model,
    )
    .pull({
        let last = slot(font0);
        move |w: &mut SpinBox, m| {
            let v = w.value();
            let mut g = last.lock();
            if (v - *g).abs() > 0.01 {
                *g = v;
                m.editor_font_pt.set(v);
            }
        }
    })
    .push({
        let last = slot(font0);
        move |w, m| {
            let v = m.editor_font_pt.get();
            let mut g = last.lock();
            if (v - *g).abs() > 0.01 && !w.is_editing() && !w.is_pressed() {
                *g = v;
                w.set_value(v);
            }
        }
    });
    let font_slide = Bound::new(
        Slider::new(8.0, 24.0)
            .label("editor font pt")
            .with_value(font0)
            .step(0.5),
        model,
    )
    .pull({
        let last = slot(font0);
        move |w: &mut Slider, m| {
            let v = w.value();
            let mut g = last.lock();
            if (v - *g).abs() > 0.01 {
                *g = v;
                m.editor_font_pt.set(v);
            }
        }
    })
    .push({
        let last = slot(font0);
        move |w, m| {
            let v = m.editor_font_pt.get();
            let mut g = last.lock();
            if (v - *g).abs() > 0.01 && !w.is_dragging() {
                *g = v;
                w.set_value(v);
            }
        }
    });

    // FontButton — the editor face readout. The code panel shapes with
    // `Attrs::new()` (generic sans-serif — the platform cascade resolves
    // the concrete font, no named family is configured), so the family
    // rides a local signal seeded with that truth; the pt tracks
    // `editor_font_pt`. Activation files the picker request (no OS
    // dialog in the sim).
    let font_family = Signal::new("Sans Serif".to_string());
    let font_btn = Bound::new(FontButton::new(font_family.get(), font0 as f32), model)
        .pull(|w: &mut FontButton, m| {
            if w.take_activated() {
                m.log(
                    usize::MAX,
                    "font dialog requested — size edits via spin/slider",
                );
            }
        })
        .push({
            let family = font_family.clone();
            move |w, m| {
                let want = family.get();
                let pt = m.editor_font_pt.get() as f32;
                if w.family() != want || (w.size() - pt).abs() > 0.01 {
                    w.set_font(want, pt);
                }
            }
        });

    // Toggles — console prefs and global flags, each a Bound on a
    // boolean signal through the shared slot helpers.
    let autosave = toggle_switch(model, "autosave", &model.editor_autosave);
    let wrap = toggle_check(model, "wrap notes", &model.editor_wrap);
    let motion = toggle_switch(model, "reduced motion", &model.reduced_motion);
    let alerts = toggle_switch(model, "alerts", &model.alerts_on);

    let tour = Bound::new(
        Button::new("RESET TOUR").tooltip("re-arms the first-run tour"),
        model,
    )
    .pull(|w: &mut Button, m| {
        if w.take_activated() {
            m.tour_seen.set_if_changed(false);
            m.log(usize::MAX, "tour reset — shows on next launch");
        }
    });

    // --- The accent suite: every control edits editor_accent [u8;4].
    let accent0 = model.editor_accent.get();

    let picker = Bound::new(
        ColorPicker::new()
            .color(Color::rgba(accent0[0], accent0[1], accent0[2], accent0[3]))
            .with_alpha(true)
            .label("accent"),
        model,
    )
    .pull(|w: &mut ColorPicker, m| {
        if let Some(c) = w.take_edited().or_else(|| w.take_selected()) {
            m.editor_accent.set_if_changed(c.to_rgba8());
        }
    })
    .push(|w, m| {
        let a = m.editor_accent.get();
        if w.get_color().to_rgba8() != a && !w.is_open() {
            w.set_color(Color::rgba(a[0], a[1], a[2], a[3]));
        }
    });

    let hue = Bound::new(
        HueSlider::new()
            .hue(rgb_to_hsv(accent0[0], accent0[1], accent0[2]).0)
            .label("hue"),
        model,
    )
    .pull(|w: &mut HueSlider, m| {
        while let Some(h) = w.take_changed() {
            let a = m.editor_accent.get();
            let (_oh, s, v) = rgb_to_hsv(a[0], a[1], a[2]);
            let (r, g, b) = martensite::widgets::color_picker::hsv_to_rgb(h, s, v);
            m.editor_accent.set_if_changed([r, g, b, a[3]]);
        }
    })
    .push(|w, m| {
        let a = m.editor_accent.get();
        let (h, _, _) = rgb_to_hsv(a[0], a[1], a[2]);
        if (w.hue_value() - h).abs() > 0.5 {
            w.set_hue(h);
        }
    });

    let alpha = Bound::new(
        AlphaSlider::new()
            .color(accent0)
            .alpha(f32::from(accent0[3]) / 255.0)
            .label("alpha"),
        model,
    )
    .pull(|w: &mut AlphaSlider, m| {
        while let Some(a) = w.take_changed() {
            let mut c = m.editor_accent.get();
            c[3] = (a.clamp(0.0, 1.0) * 255.0).round() as u8;
            m.editor_accent.set_if_changed(c);
        }
    })
    .push(|w, m| {
        let a = f32::from(m.editor_accent.get()[3]) / 255.0;
        if (w.alpha_value() - a).abs() > 0.01 {
            w.set_alpha(a);
        }
    });

    let wheel = Bound::new(
        {
            let (h, s, v) = rgb_to_hsv(accent0[0], accent0[1], accent0[2]);
            ColorWheel::new()
                .hue(h)
                .saturation(s)
                .brightness(v)
                .label("accent wheel")
        },
        model,
    )
    .pull(|w: &mut ColorWheel, m| {
        if w.take_changed() {
            let mut c = w.rgb();
            c[3] = m.editor_accent.get()[3];
            m.editor_accent.set_if_changed(c);
        }
    })
    .push(|w, m| {
        let a = m.editor_accent.get();
        let mut cur = w.rgb();
        cur[3] = a[3];
        if cur != a {
            let (h, s, v) = rgb_to_hsv(a[0], a[1], a[2]);
            *w = ColorWheel::new()
                .hue(h)
                .saturation(s)
                .brightness(v)
                .label("accent wheel");
        }
    });

    // Palette — swatches that nominate the accent; selecting one sets
    // it, and an accent matching a swatch highlights it.
    const SWATCHES: [[u8; 4]; 5] = [
        [96, 165, 250, 255],  // console blue (default)
        [34, 197, 94, 255],   // running green
        [234, 179, 8, 255],   // warning amber
        [239, 68, 68, 255],   // alarm red
        [148, 163, 184, 255], // muted steel
    ];
    let palette = Bound::new(ColorPalette::new().swatches(SWATCHES), model)
        .pull(|w: &mut ColorPalette, m| {
            while let Some((_i, c)) = w.take_selected() {
                let mut a = m.editor_accent.get();
                a[..3].copy_from_slice(&c[..3]);
                m.editor_accent.set_if_changed(a);
            }
        })
        .push(|w, m| {
            let a = m.editor_accent.get();
            if w.selected_color() != Some(a) {
                if let Some(i) = SWATCHES.iter().position(|s| s[..3] == a[..3]) {
                    w.select(i);
                }
            }
        });

    // Accent button — live swatch; activation restores the seed default.
    let accent_btn = Bound::new(
        ColorButton::new(accent0).title("accent — tap to reset"),
        model,
    )
    .pull(|w: &mut ColorButton, m| {
        if w.take_activated() {
            m.editor_accent.set_if_changed(ACCENT_DEFAULT);
            m.log(usize::MAX, "accent reset to console blue");
        }
    })
    .push(|w, m| {
        let a = m.editor_accent.get();
        if w.color() != a {
            w.set_color(a);
        }
    });

    // Section rail — `editor_section` names the detail surface.
    let sec_sel = Signal::new(model.editor_section.get() as usize);
    let nav = {
        let ss = sec_sel.clone();
        Bound::new(
            NavRail::new()
                .destination("Aa", "Typography")
                .destination("⚙", "Behavior")
                .destination("◐", "Accent")
                .selected(sec_sel.get()),
            model,
        )
        .pull(move |w: &mut NavRail, m| {
            if let Some(i) = w.take_activated() {
                ss.set_if_changed(i);
                m.editor_section.set_if_changed(i as u8);
            }
        })
        .push(move |w: &mut NavRail, m| {
            let s = m.editor_section.get() as usize;
            if w.selected_index() != Some(s) {
                w.set_selected(Some(s));
            }
        })
    };

    let typography = Flex::column().gap(ZONE_GAP).child(
        strip()
            .child(field("FONT PT", font_spin))
            .child(field("FONT PT", font_slide))
            .child(font_btn)
            .child_flex(DummyWidget, 1.0),
    );
    let behavior = Flex::column()
        .gap(ZONE_GAP)
        .child(
            strip()
                .child(autosave)
                .child(wrap)
                .child(motion)
                .child(alerts)
                .child_flex(DummyWidget, 1.0),
        )
        .child(strip().child(tour).child_flex(DummyWidget, 1.0));
    let accent = Flex::column()
        .gap(ZONE_GAP)
        .child(hint(
            "the whole color suite edits the single console accent — every picker stays in sync",
        ))
        .child(
            strip()
                .child(field("ACCENT", picker))
                .child(field("HUE", hue))
                .child(field("ALPHA", alpha))
                .child_flex(DummyWidget, 1.0),
        )
        .child(
            strip()
                .child(field("WHEEL", wheel))
                .child(field("SWATCHES", palette))
                .child(accent_btn)
                .child_flex(DummyWidget, 1.0),
        );

    let primary = Swap::new(&sec_sel)
        .view(typography)
        .view(behavior)
        .view(accent);

    Page::new(
        Variant::MasterLeft,
        crate::zone::fill(primary),
        &model.zone_width[2],
    )
    .rail("Section", nav)
}

/// A `Bound<Switch>` on a boolean signal — the slot reconcile shared
/// by every console toggle.
fn toggle_switch(
    model: &PlantModel,
    label: &'static str,
    sig: &martensite::reactive::Signal<bool>,
) -> Bound<Switch> {
    let initial = sig.get();
    let s = sig.clone();
    Bound::new(Switch::new(label).on(initial), model)
        .pull({
            let last = slot(initial);
            let s = s.clone();
            move |w: &mut Switch, _m| {
                let v = w.on;
                let mut g = last.lock();
                if v != *g {
                    *g = v;
                    s.set(v);
                }
            }
        })
        .push({
            let last = slot(initial);
            move |w, _m| {
                let v = s.get();
                let mut g = last.lock();
                if v != *g {
                    *g = v;
                    w.on = v;
                }
            }
        })
}

/// A `Bound<CheckBox>` on a boolean signal — same reconcile, checkbox
/// chrome instead of a switch.
fn toggle_check(
    model: &PlantModel,
    label: &'static str,
    sig: &martensite::reactive::Signal<bool>,
) -> Bound<CheckBox> {
    let initial = sig.get();
    let s = sig.clone();
    Bound::new(CheckBox::new(label).checked(initial), model)
        .pull({
            let last = slot(initial);
            let s = s.clone();
            move |w: &mut CheckBox, _m| {
                let v = w.checked;
                let mut g = last.lock();
                if v != *g {
                    *g = v;
                    s.set(v);
                }
            }
        })
        .push({
            let last = slot(initial);
            move |w, _m| {
                let v = s.get();
                let mut g = last.lock();
                if v != *g {
                    *g = v;
                    w.set_checked(v);
                }
            }
        })
}

// ---------------------------------------------------------------------------
// COMMAND SURFACE — one action set, mutually-exclusive alternates.
// ---------------------------------------------------------------------------

/// CHROME — "which command surface fits this job?" Master-detail |
/// strip: chrome selector (Bars | Overlays | Launchers | Actions)
/// writes `active_chrome` | primary: the selected chrome, live |
/// rail: the `commands` registry filtered to the active chrome —
/// activating an entry routes it through `dispatch` like the real
/// surface would.
fn command_surface(model: &PlantModel) -> Page {
    // Primary: MenuBar + Toolbar dispatch the same action set.
    let menubar = Bound::new(
        MenuBar::new().menu(
            "PLANT",
            vec![
                MenuItem::action("New work order"),
                MenuItem::action("Acknowledge all alarms"),
                MenuItem::action("Toggle line"),
                MenuItem::action("Lock console"),
            ],
        ),
        model,
    )
    .pull(|w: &mut MenuBar, m| {
        while let Some(path) = w.take_activated() {
            // path = [menu, item] — only the PLANT menu (index 0) exists.
            if path.first() != Some(&0) {
                continue;
            }
            if let Some(&cmd) = path
                .get(1)
                .and_then(|&i| [Cmd::NewWo, Cmd::AckAll, Cmd::ToggleLine, Cmd::LockConsole].get(i))
            {
                dispatch(m, cmd);
            }
        }
    });

    let toolbar = Bound::new(
        Toolbar::new()
            .item(Button::new("Ack all"))
            .item(Button::new("Toggle line"))
            .item(Button::new("New WO"))
            .item(Button::new("Lock"))
            .label("command toolbar"),
        model,
    )
    .pull(|w: &mut Toolbar, m| {
        while let Some(i) = w.take_activated() {
            // take_activated reports the declaration-order item index.
            if let Some(&cmd) = [Cmd::AckAll, Cmd::ToggleLine, Cmd::NewWo, Cmd::LockConsole].get(i)
            {
                dispatch(m, cmd);
            }
        }
    });

    // Alternates — a Tabs mounts ONE surface at a time; each drains
    // its own activation seam into `dispatch`.
    let palette = Bound::new(
        CommandPalette::new()
            .actions([
                CommandAction::new("wo.new", "New work order"),
                CommandAction::new("alarm.ack", "Acknowledge all alarms"),
                CommandAction::new("line.toggle", "Toggle line"),
                CommandAction::new("line.trip", "Trip line").keywords(["stop", "estop"]),
                CommandAction::new("console.lock", "Lock console"),
                CommandAction::new("asset.inspect", "Inspect selected asset"),
                CommandAction::new("asset.diagnose", "Run diagnostics").keywords(["test"]),
            ])
            .placeholder("type a command…"),
        model,
    )
    .pull(|w: &mut CommandPalette, m| {
        while let Some(id) = w.take_activated() {
            let cmd = match id.as_str() {
                "wo.new" => Cmd::NewWo,
                "alarm.ack" => Cmd::AckAll,
                "line.toggle" => Cmd::ToggleLine,
                "line.trip" => Cmd::TripLine,
                "console.lock" => Cmd::LockConsole,
                "asset.inspect" => Cmd::InspectAsset,
                "asset.diagnose" => Cmd::Diagnose,
                _ => continue, // unknown action id — drop, don't guess
            };
            dispatch(m, cmd);
        }
    });

    let radial = Bound::new(
        RadialMenu::new()
            .items(["Ack", "Line", "Lock", "New WO", "Diagnose"])
            .label("quick actions"),
        model,
    )
    .pull(|w: &mut RadialMenu, m| {
        while let Some(i) = w.take_selected() {
            if let Some(&cmd) = [
                Cmd::AckAll,
                Cmd::ToggleLine,
                Cmd::LockConsole,
                Cmd::NewWo,
                Cmd::Diagnose,
            ]
            .get(i)
            {
                dispatch(m, cmd);
            }
        }
    });

    let dial = Bound::new(
        SpeedDial::new()
            .label("quick actions")
            .action("Ack all alarms")
            .action("Toggle line")
            .action("New work order")
            .action("Lock console"),
        model,
    )
    .pull(|w: &mut SpeedDial, m| {
        while let Some(i) = w.take_action() {
            if let Some(&cmd) = [Cmd::AckAll, Cmd::ToggleLine, Cmd::NewWo, Cmd::LockConsole].get(i)
            {
                dispatch(m, cmd);
            }
        }
    });

    let fab = Bound::new(FloatButton::new("+").label("new work order"), model).pull(
        |w: &mut FloatButton, m| {
            if w.take_activated() {
                dispatch(m, Cmd::NewWo);
            }
        },
    );

    let ribbon = Ribbon::new("LIVE").child(
        Bound::new(
            CommandLink::new("Run diagnostics").note("full self-test on the selected asset"),
            model,
        )
        .pull(|w: &mut CommandLink, m| {
            if w.take_activated() {
                dispatch(m, Cmd::Diagnose);
            }
        }),
    );

    // Context menu on the selected asset + a confirmed destructive
    // action + a command link — same action set, asset-scoped.
    let asset_ctx = Bound::new(
        ContextMenu::new(
            Text::new("right-click — asset ops").font_size(12.0),
            vec![
                MenuItem::action("Inspect asset"),
                MenuItem::action("Ack asset alarms"),
                MenuItem::action("Run diagnostics"),
            ],
        ),
        model,
    )
    .pull(|w: &mut ContextMenu, m| {
        while let Some(path) = w.take_activated() {
            if let Some(&cmd) = path
                .first()
                .and_then(|&i| [Cmd::InspectAsset, Cmd::AckAssetAlarms, Cmd::Diagnose].get(i))
            {
                dispatch(m, cmd);
            }
        }
    })
    .push({
        let last = slot(String::new());
        move |w, m| {
            let name = m
                .selected_asset
                .get()
                .map(|id| m.asset_name(id))
                .unwrap_or("—")
                .to_string();
            let mut g = last.lock();
            if *g != name {
                *g = name.clone();
                *w = ContextMenu::new(
                    Text::new(format!("right-click — {name} ops")).font_size(12.0),
                    vec![
                        MenuItem::action("Inspect asset"),
                        MenuItem::action("Ack asset alarms"),
                        MenuItem::action("Run diagnostics"),
                    ],
                );
            }
        }
    });

    let trip = Bound::new(
        Popconfirm::new()
            .question("Trip the line? Running stops immediately.")
            .confirm_label("Trip")
            .cancel_label("Cancel"),
        model,
    )
    .pull(|w: &mut Popconfirm, m| {
        while let Some(r) = w.take_result() {
            if r == ConfirmResult::Confirm {
                dispatch(m, Cmd::TripLine);
            }
        }
    });

    let ack_link = Bound::new(
        CommandLink::new("Acknowledge all alarms").note("clears every active alarm"),
        model,
    )
    .pull(|w: &mut CommandLink, m| {
        if w.take_activated() {
            dispatch(m, Cmd::AckAll);
        }
    });

    // Button group — the same action set as plain buttons.
    let ack_btn = Bound::new(Button::new("ACK ALL"), model).pull(|w: &mut Button, m| {
        if w.take_activated() {
            dispatch(m, Cmd::AckAll);
        }
    });
    let line_btn = Bound::new(Button::new("TOGGLE LINE"), model).pull(|w: &mut Button, m| {
        if w.take_activated() {
            dispatch(m, Cmd::ToggleLine);
        }
    });
    let lock_btn = Bound::new(Button::new("LOCK"), model).pull(|w: &mut Button, m| {
        if w.take_activated() {
            dispatch(m, Cmd::LockConsole);
        }
    });
    let new_btn = Bound::new(Button::new("NEW WO"), model).pull(|w: &mut Button, m| {
        if w.take_activated() {
            dispatch(m, Cmd::NewWo);
        }
    });

    // Chrome selector — `active_chrome` names the previewed surface.
    let chrome_sel = Signal::new(model.active_chrome.get() as usize);
    let chrome_view = {
        let cs = chrome_sel.clone();
        Bound::new(
            Segmented::new()
                .options(["Bars", "Overlays", "Launchers", "Actions"])
                .selected(chrome_sel.get())
                .label("chrome"),
            model,
        )
        .pull(move |w: &mut Segmented, m| {
            if let Some(i) = w.take_selected() {
                cs.set_if_changed(i);
                m.active_chrome.set_if_changed(i as u8);
            }
        })
        .push(move |w: &mut Segmented, m| {
            let s = m.active_chrome.get() as usize;
            if w.selected_index() != s {
                w.set_selected(s);
            }
        })
    };

    // Primary — one chrome at a time, selected by the strip.
    let bars = Flex::column()
        .gap(ZONE_GAP)
        .child(hint("menu bar + toolbar dispatch the same verbs"))
        .child(strip().child_flex(menubar, 1.0))
        .child(strip().child_flex(toolbar, 1.0));
    let overlays = Flex::column()
        .gap(ZONE_GAP)
        .child(hint("overlay idioms — invoked, never docked"))
        .child_flex(band(BAND_M, palette), 1.0)
        .child(
            row()
                .child_flex(band(BAND_M, radial), 1.0)
                .child_flex(band(BAND_M, dial), 1.0),
        );
    let launchers = Flex::column()
        .gap(ZONE_GAP)
        .child(hint("right-click the context card; trip needs a confirm"))
        .child(
            strip()
                .child(asset_ctx)
                .child(trip)
                .child(ack_link)
                .child_flex(DummyWidget, 1.0),
        )
        .child(strip().child(fab).child_flex(ribbon, 1.0));
    let actions = Flex::column()
        .gap(ZONE_GAP)
        .child(hint("the same verbs as plain buttons"))
        .child(
            strip()
                .child(ack_btn)
                .child(line_btn)
                .child(lock_btn)
                .child(new_btn)
                .child_flex(DummyWidget, 1.0),
        );
    let primary = Swap::new(&chrome_sel)
        .view(bars)
        .view(overlays)
        .view(launchers)
        .view(actions);

    // Rail — the command registry filtered to the active chrome's
    // bits; activation routes through `dispatch` (real write path).
    let registry = {
        fn bit_of(sel: usize) -> u8 {
            match sel {
                0 => 0b001000, // Bars → menubar(+toolbar shares it)
                1 => 0b010001, // Overlays → palette + radial
                2 => 0b000100, // Launchers → context
                _ => 0b111111, // Actions → everything
            }
        }
        fn items_of(m: &PlantModel, sel: usize) -> Vec<String> {
            let bit = bit_of(sel);
            m.commands
                .get()
                .iter()
                .filter(|c| c.chromes & bit != 0)
                .map(|c| c.label.to_string())
                .collect()
        }
        let cs = chrome_sel.clone();
        let cs2 = chrome_sel.clone();
        let mut last = (chrome_sel.get(), usize::MAX);
        let mut lv = ListView::new()
            .items(items_of(model, chrome_sel.get()))
            .selection_mode(SelectionMode::Single)
            .label("command registry");
        lv.set_selected(0);
        Bound::new(lv, model)
            .pull(move |w: &mut ListView, m| {
                if let Some(i) = w.take_activated().or_else(|| w.selected()) {
                    let bit = bit_of(cs.get());
                    let ids: Vec<&'static str> = m
                        .commands
                        .get()
                        .iter()
                        .filter(|c| c.chromes & bit != 0)
                        .map(|c| c.id)
                        .collect();
                    if let Some(id) = ids.get(i) {
                        if let Some(cmd) = cmd_of(id) {
                            dispatch(m, cmd);
                        } else {
                            m.log(usize::MAX, format!("command {id} routed"));
                        }
                    }
                }
            })
            .push(move |w: &mut ListView, m| {
                let k = (cs2.get(), m.commands.get().len());
                if k != last {
                    last = k;
                    w.set_items(items_of(m, cs2.get()));
                }
            })
    };

    Page::new(
        Variant::MasterDetail,
        crate::zone::fill(primary),
        &model.zone_width[2],
    )
    .strip(strip().child(chrome_view).child_flex(DummyWidget, 1.0))
    .rail("Commands", registry)
}

/// Registry id → `Cmd` — the CHROME rail's dispatch mapping. Returns
/// `None` for registry entries with no direct verb (navigation,
/// filters) — those log instead.
fn cmd_of(id: &str) -> Option<Cmd> {
    Some(match id {
        "wo.new" => Cmd::NewWo,
        "alarm.ack" => Cmd::AckAll,
        "line.toggle" => Cmd::ToggleLine,
        "line.trip" => Cmd::TripLine,
        "lock.toggle" => Cmd::LockConsole,
        "asset.inspect" => Cmd::InspectAsset,
        "asset.diagnose" => Cmd::Diagnose,
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// CONSOLE LOCK — security inputs' honest role: the HMI console lock.
// ---------------------------------------------------------------------------

/// Demo credentials for the sim's lock screen — the model has no
/// credential store, so the enrolled secrets live in the zone.
const CONSOLE_PIN: &str = "4471";
const CONSOLE_OTP: &str = "447100";
const CONSOLE_PW: &str = "martensite";

fn console_lock(model: &PlantModel) -> Page {
    // Status — the lock indicator reflects console_locked.
    let status = Bound::new(
        StatusDot::new("CONSOLE — ACTIVE").status(DotStatus::Ok),
        model,
    )
    .push({
        let last = slot(false);
        move |w, m| {
            let locked = m.console_locked.get();
            let mut g = last.lock();
            if locked != *g {
                *g = locked;
                *w = StatusDot::new(if locked {
                    "CONSOLE — LOCKED"
                } else {
                    "CONSOLE — ACTIVE"
                })
                .status(if locked {
                    DotStatus::Error
                } else {
                    DotStatus::Ok
                })
                .pulse(locked);
            }
        }
    });
    let who = live_text(model, |m| {
        if m.console_locked.get() {
            "console locked — pattern, PIN, OTP, or password unlocks".into()
        } else {
            "console active — LOCK CONSOLE engages the lock".into()
        }
    });

    let lock = Bound::new(Button::new("LOCK CONSOLE"), model).pull(|w: &mut Button, m| {
        if w.take_activated() {
            m.console_locked.set_if_changed(true);
            m.log(usize::MAX, "console locked by operator");
        }
    });

    // Pattern — any 4+ dot gesture is the enrolled pattern.
    let pattern =
        Bound::new(PatternLock::new().label("pattern"), model).pull(|w: &mut PatternLock, m| {
            while let Some(p) = w.take_pattern() {
                if p.len() >= 4 {
                    m.console_locked.set_if_changed(false);
                    m.log(usize::MAX, "console unlocked — pattern accepted");
                } else {
                    m.log(usize::MAX, "pattern rejected — 4+ dots required");
                }
                w.clear();
            }
        });

    // PIN — the keypad accumulates digits into the shared attempt
    // buffer; '#' clears, 4 digits submit.
    let pin_attempt = slot(String::new());
    let keypad = Bound::new(Keypad::new().label("PIN pad"), model).pull({
        let pin = pin_attempt.clone();
        move |w: &mut Keypad, m| {
            while let Some(c) = w.take_pressed() {
                let mut buf = pin.lock();
                match c {
                    '0'..='9' => {
                        buf.push(c);
                        if buf.len() >= CONSOLE_PIN.len() {
                            if *buf == CONSOLE_PIN {
                                m.console_locked.set_if_changed(false);
                                m.log(usize::MAX, "console unlocked — PIN accepted");
                            } else {
                                m.log(usize::MAX, "PIN rejected");
                            }
                            buf.clear();
                        }
                    }
                    '*' => {
                        buf.pop();
                    }
                    _ => buf.clear(),
                }
            }
        }
    });

    // OTP — the authenticator code for this sim's console.
    let otp = Bound::new(OtpInput::new().length(6), model).pull(|w: &mut OtpInput, m| {
        while let Some(code) = w.take_completed() {
            if code == CONSOLE_OTP {
                m.console_locked.set_if_changed(false);
                m.log(usize::MAX, "console unlocked — OTP accepted");
            } else {
                m.log(usize::MAX, "OTP rejected");
            }
            w.set_value("");
        }
    });

    // Password — secure TextInput; the strength meter reads the same
    // attempt buffer (shared slot, honest pairing).
    let pw_attempt = slot(String::new());
    let password = Bound::new(
        TextInput::new("password")
            .secure(true)
            .placeholder("console password…"),
        model,
    )
    .pull({
        let pw = pw_attempt.clone();
        move |w: &mut TextInput, m| {
            if w.take_edited() {
                *pw.lock() = w.value.clone();
                if w.value == CONSOLE_PW {
                    m.console_locked.set_if_changed(false);
                    m.log(usize::MAX, "console unlocked — password accepted");
                    w.set_value("");
                    pw.lock().clear();
                }
            }
        }
    });
    let strength = Bound::new(PasswordStrength::new().score(0), model).push({
        let pw = pw_attempt.clone();
        move |w, _m| {
            let score = PasswordStrength::for_password(&pw.lock()).score_value();
            if w.score_value() != score {
                w.set_score(score);
            }
        }
    });

    let primary = Flex::column()
        .gap(ZONE_STACK)
        .child(
            strip()
                .child(status)
                .child(who)
                .child(lock)
                .child_flex(DummyWidget, 1.0),
        )
        .child(hint(
            "any enrolled factor unlocks — pattern, PIN pad, OTP, or password",
        ))
        .child(
            GroupBox::new("UNLOCK FACTORS").child(
                Flex::column().gap(ZONE_GAP).child(
                    strip()
                        .child(field("PATTERN", pattern))
                        .child(field("PIN", keypad))
                        .child_flex(DummyWidget, 1.0),
                ),
            ),
        )
        .child(
            strip()
                .child(field("OTP", otp))
                .child_flex(field("PASSWORD", password), 1.0),
        )
        // The strength meter sits under the password field — the
        // standard pairing — not squeezed into the field row where an
        // over-wide strip crushes it to zero width.
        .child(strip().child_flex(strength, 1.0));
    Page::new(
        Variant::Centered,
        crate::zone::fill(crate::zone::scroll(primary)),
        &model.zone_width[2],
    )
}

// ---------------------------------------------------------------------------
// ANNOTATION & SIGN-OFF — markup, crop memory, supervisor signature.
// ---------------------------------------------------------------------------

fn annotation(model: &PlantModel) -> Page {
    let asset0 = model.selected_asset.get();
    let target = live_text(model, |m| {
        match m.selected_asset.get().map(|id| (id, m.asset_name(id))) {
            Some((_id, name)) => {
                format!("markup files against {name} — strokes log to the shift record")
            }
            None => "select an asset to mark up".into(),
        }
    });

    // Inspection markup — strokes log with the asset id; the canvas
    // clears on asset switch (per-asset surface).
    let undo = Arc::new(AtomicBool::new(false));
    let stroke_n = slot(0u32);
    let markup = Bound::new(InkCanvas::new().pen(2.0).label("inspection markup"), model)
        .pull({
            let undo = undo.clone();
            let stroke_n = stroke_n.clone();
            move |w: &mut InkCanvas, m| {
                if undo.swap(false, Ordering::Relaxed) && w.undo().is_some() {
                    m.log(usize::MAX, "markup stroke undone");
                }
                while let Some(stroke) = w.take_stroke() {
                    let mut n = stroke_n.lock();
                    *n += 1;
                    let asset = m
                        .selected_asset
                        .get()
                        .map(|id| m.asset_name(id))
                        .unwrap_or("—");
                    m.log(
                        usize::MAX,
                        format!("markup #{} on {asset} ({} pts)", *n, stroke.len()),
                    );
                }
            }
        })
        .push({
            let last = slot(asset0);
            move |w, m| {
                let cur = m.selected_asset.get();
                let mut g = last.lock();
                if cur != *g {
                    *g = cur;
                    w.clear();
                }
            }
        });
    let undo_btn = Bound::new(Button::new("UNDO STROKE"), model).pull({
        let undo = undo.clone();
        move |w: &mut Button, _m| {
            if w.take_activated() {
                undo.store(true, Ordering::Relaxed);
            }
        }
    });

    // Supervisor sign-off — a stroke on the pad signs the WO: status
    // → Review, any "Sign-off" checklist row closes, log entry files.
    let signoff = Bound::new(
        InkCanvas::new().pen(2.5).label("supervisor sign-off"),
        model,
    )
    .pull(|w: &mut InkCanvas, m| {
        while let Some(_stroke) = w.take_stroke() {
            // Only a real sign-off files a log entry — no WO selected
            // or already-Reviewed means the stroke just clears.
            let Some(id) = m.selected_wo.get() else {
                w.clear();
                continue;
            };
            // A dangling selection (id not in the store) must not log.
            let Some(wo) = m.work_orders.get().into_iter().find(|wo| wo.id == id) else {
                w.clear();
                continue;
            };
            let was_review = wo.status == WoStatus::Review;
            m.update_wo(|wo| {
                wo.signature = Some(crate::domain::Signature::Ink);
                wo.status = WoStatus::Review;
                for item in wo.checklist.iter_mut() {
                    if item.0.contains("Sign-off") || item.0.contains("sign-off") {
                        item.1 = true;
                    }
                }
            });
            if !was_review {
                m.log(usize::MAX, format!("supervisor sign-off on WO-{id}"));
            }
            w.clear();
        }
    })
    .push({
        let last = slot(model.selected_wo.get());
        move |w, m| {
            let cur = m.selected_wo.get();
            let mut g = last.lock();
            if cur != *g {
                *g = cur;
                w.clear();
            }
        }
    });

    // Photo crop — the sim has no image store, so the crop rect is
    // remembered per asset in the binding's own map (a real per-asset
    // view state, restored on switch, drag-guarded).
    let crops: Slot<HashMap<u32, (f32, f32, f32, f32)>> = slot(HashMap::new());
    let crop = Bound::new(
        CropBox::new().crop(0.1, 0.1, 0.8, 0.8).label("photo crop"),
        model,
    )
    .pull({
        let crops = crops.clone();
        move |w: &mut CropBox, m| {
            while let Some(r) = w.take_changed() {
                if let Some(id) = m.selected_asset.get() {
                    crops.lock().insert(id, r);
                    m.update_wo(|wo| {
                        wo.photo = Some(format!(
                            "crop@{id}:{:.2},{:.2},{:.2},{:.2}",
                            r.0, r.1, r.2, r.3
                        ));
                    });
                }
            }
        }
    })
    .push({
        let crops = crops.clone();
        let last = slot(asset0);
        move |w, m| {
            let cur = m.selected_asset.get();
            let mut g = last.lock();
            if cur != *g && !w.is_dragging() {
                *g = cur;
                let r = cur
                    .and_then(|id| crops.lock().get(&id).copied())
                    .unwrap_or((0.1, 0.1, 0.8, 0.8));
                w.set_crop(r.0, r.1, r.2, r.3);
            }
        }
    });

    let primary = Flex::column()
        .gap(ZONE_STACK)
        .child(target)
        .child(
            row()
                .child_flex(field("MARKUP", band(BAND_M, markup)), 1.0)
                .child(undo_btn),
        )
        .child(
            row()
                .child_flex(field("SIGN-OFF", band(BAND_M, signoff)), 3.0)
                .child_flex(field("PHOTO CROP", band(BAND_M, crop)), 2.0),
        );
    Page::new(
        Variant::Centered,
        crate::zone::fill(primary),
        &model.zone_width[2],
    )
}

// ---------------------------------------------------------------------------
// SCAN & LOOKUP — serial lookup, asset identity, config import.
// ---------------------------------------------------------------------------

/// A deterministic QR matrix derived from the asset serial — finder
/// squares in the corners over an FNV-seeded fill, like the showcase's
/// demo matrix but driven by real data.
fn qr_matrix(serial: &str) -> Vec<Vec<bool>> {
    const N: usize = 21;
    let mut m = vec![vec![false; N]; N];
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in serial.bytes() {
        h = (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    let mut x = h;
    for (r, row) in m.iter_mut().enumerate() {
        for (c, cell) in row.iter_mut().enumerate() {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            *cell = (x ^ (r as u64) ^ (c as u64)) & 1 == 1;
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

/// The scan-lookup: serial or name substring → the asset (what a
/// handheld scanner would resolve).
fn find_asset(m: &PlantModel, q: &str) -> Option<Asset> {
    let q = q.trim().to_lowercase();
    if q.is_empty() {
        return None;
    }
    m.assets
        .get()
        .into_iter()
        .find(|a| a.serial.to_lowercase() == q)
        .or_else(|| {
            m.assets.get().into_iter().find(|a| {
                a.serial.to_lowercase().contains(&q) || a.name.to_lowercase().contains(&q)
            })
        })
}

/// The scan/lookup station — mounted as DETAIL's "Scan" dossier
/// lens (Process Grid zone), not a standalone page.
pub(crate) fn scan_lookup(model: &PlantModel) -> Flex {
    // Search — live grid filter + submit = the "scan" lookup.
    let search = Bound::new(
        SearchField::new()
            .placeholder("serial or asset name…")
            .with_value(model.filter_text.get()),
        model,
    )
    .pull(|w: &mut SearchField, m| {
        if w.take_edited() {
            m.filter_text.set_if_changed(w.value().to_string());
        }
        if let Some(q) = w.take_submitted() {
            match find_asset(m, &q) {
                Some(a) => {
                    m.selected_asset.set_if_changed(Some(a.id));
                    m.log(usize::MAX, format!("lookup '{q}' → {}", a.name));
                }
                None => m.log(usize::MAX, format!("lookup '{q}' — no asset match")),
            }
        }
    })
    .push(|w, m| {
        let ft = m.filter_text.get();
        if w.value() != ft {
            w.set_value(ft);
        }
    });

    // Cascader — the site→line→cell tree as a picker; drains into
    // selected_asset. Pull-only: Cascader exposes no path setter, so
    // external selection changes can't be reflected — documented
    // one-way mount.
    let cascader = Bound::new(
        Cascader::new()
            .options(asset_options(model))
            .placeholder("site ▸ line ▸ cell"),
        model,
    )
    .pull(|w: &mut Cascader, m| {
        while let Some(path) = w.take_selected() {
            if let Some(id) = path.last().and_then(|v| v.parse::<u32>().ok()) {
                m.selected_asset.set_if_changed(Some(id));
            }
        }
    });

    // Identity — serial rendered as QR + barcode + a copyable readout.
    let serial0 = model
        .selected_asset
        .get()
        .and_then(|id| model.asset(id))
        .map(|a| a.serial)
        .unwrap_or("—");
    let qr = Bound::new(
        QrCode::from_matrix(qr_matrix(serial0)).label("asset QR"),
        model,
    )
    .push({
        let last = slot(serial0.to_string());
        move |w, m| {
            let serial = m
                .selected_asset
                .get()
                .and_then(|id| m.asset(id))
                .map(|a| a.serial)
                .unwrap_or("—");
            let mut g = last.lock();
            if *g != serial {
                *g = serial.to_string();
                *w = QrCode::from_matrix(qr_matrix(serial)).label("asset QR");
            }
        }
    });
    let code = Bound::new(Barcode::new().text(serial0).label("serial"), model).push({
        let last = slot(serial0.to_string());
        move |w, m| {
            let serial = m
                .selected_asset
                .get()
                .and_then(|id| m.asset(id))
                .map(|a| a.serial)
                .unwrap_or("—");
            let mut g = last.lock();
            if *g != serial {
                *g = serial.to_string();
                w.set_text(serial);
            }
        }
    });
    let copyable = Bound::new(Copyable::new(serial0).label("serial"), model)
        .pull(|w: &mut Copyable, m| {
            if let Some(s) = w.take_copied() {
                m.log(usize::MAX, format!("serial {s} copied"));
            }
        })
        .push({
            let last = slot(serial0.to_string());
            move |w, m| {
                let serial = m
                    .selected_asset
                    .get()
                    .and_then(|id| m.asset(id))
                    .map(|a| a.serial)
                    .unwrap_or("—");
                let mut g = last.lock();
                if *g != serial {
                    *g = serial.to_string();
                    w.set_text(serial);
                }
            }
        });

    // Config import — activation resolves to the asset's deterministic
    // config file and files a shift-log entry (the sim's "open"
    // resolves, there's no OS dialog).
    let import = Bound::new(
        FileChooserButton::new()
            .mode(ChooserMode::Open)
            .placeholder("import asset config…"),
        model,
    )
    .pull(|w: &mut FileChooserButton, m| {
        if w.take_activated() {
            let name = m
                .selected_asset
                .get()
                .map(|id| m.asset_name(id))
                .unwrap_or("asset");
            let file = format!("{}-config.toml", name.to_lowercase().replace(' ', "-"));
            w.set_file_name(Some(file.clone()));
            m.log(usize::MAX, format!("imported {file}"));
        }
    });

    Flex::column()
        .gap(ZONE_STACK)
        .child(hint(
            "submit a serial or name to select the asset — the search also drives the grid filter",
        ))
        .child(
            strip()
                .child(field("LOOKUP", search))
                .child(field("ASSET PATH", cascader))
                .child_flex(DummyWidget, 1.0),
        )
        .child(
            row()
                .child_flex(band(BAND_S, framed(1.0, qr)), 1.0)
                .child_flex(band(BAND_S, framed(3.0, code)), 2.0)
                .child(copyable)
                .child(import),
        )
}

/// Site → line → cell options for the Cascader — values are asset ids.
fn asset_options(m: &PlantModel) -> Vec<CascaderOption> {
    let assets = m.assets.get();
    let node = |a: &Asset| CascaderOption::new(a.name, a.id.to_string());
    assets
        .iter()
        .filter(|a| a.parent.is_none())
        .map(|site| {
            let mut s = node(site);
            for line in assets.iter().filter(|a| a.parent == Some(site.id)) {
                let mut l = node(line);
                for cell in assets.iter().filter(|a| a.parent == Some(line.id)) {
                    l = l.child(node(cell));
                }
                s = s.child(l);
            }
            s
        })
        .collect()
}

// ---------------------------------------------------------------------------
// The zone's pages.
// ---------------------------------------------------------------------------

/// Domain-named zone pages for the Editor panel — `(tab label, page
/// column)` pairs. Tab labels are DOMAIN names ("WORK ORDER FORM"),
/// never widget names.
///
/// The shared `selected_wo` rail — chooser + readout. WORK ORDER and
/// SCHEDULING both edit the selected order, so both carry it.
fn wo_rail(model: &PlantModel) -> Flex {
    let pick = {
        let build = |m: &PlantModel| {
            Dropdown::new(m.work_orders.get().iter().map(|w| format!("WO-{}", w.id)))
                .label("work order")
        };
        let mut last = wo_sig(model);
        let mut last_i = usize::MAX;
        Bound::new(build(model), model)
            .pull(move |w: &mut Dropdown, m| {
                let i = w.selected();
                if i != last_i {
                    last_i = i;
                    if let Some(wo) = m.work_orders.get().get(i) {
                        m.selected_wo.set_if_changed(Some(wo.id));
                    }
                }
            })
            .push(move |w: &mut Dropdown, m| {
                let s = wo_sig(m);
                if s != last {
                    last = s;
                    *w = build(m);
                    if let Some(i) = m
                        .selected_wo
                        .get()
                        .and_then(|id| m.work_orders.get().iter().position(|w| w.id == id))
                    {
                        w.commit(i);
                    }
                }
            })
    };
    let detail = Bound::new(Descriptions::new(), model).push(|d: &mut Descriptions, m| {
        *d = match sel_wo(m) {
            Some(w) => Descriptions::new()
                .title(format!("WO-{}", w.id))
                .bordered(true)
                .item("asset", m.asset_name(w.asset))
                .item("status", w.status.label())
                .item("priority", w.priority.label())
                .item("due", format!("day {}", w.due_day + 1)),
            None => Descriptions::new()
                .title("WO")
                .item("state", "none selected"),
        };
    });
    Flex::column().gap(ZONE_GAP).child(pick).child(detail)
}

/// `work_orders` store signature for rail re-seats.
fn wo_sig(m: &PlantModel) -> u64 {
    // FNV-1a 64-bit — the offset basis and prime are the standard hash
    // constants, not colors.
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;
    let wos = m.work_orders.get();
    let mut h = FNV_OFFSET;
    for w in &wos {
        h = (h ^ w.id as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ w.status as u64).wrapping_mul(FNV_PRIME);
    }
    h ^ wos.len() as u64
}

pub fn pages(model: &PlantModel) -> Vec<(&'static str, Page)> {
    vec![
        ("WORK ORDER", work_order_form(model)),
        ("SCHEDULING", scheduling(model)),
        ("APPEARANCE", appearance(model)),
        ("CHROME", command_surface(model)),
        ("CONSOLE LOCK", console_lock(model)),
        ("SIGN-OFF", annotation(model)),
    ]
}

// ---------------------------------------------------------------------------
// Tests — the binding contract, exercised headless.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use martensite::reactive::Signal;
    use std::time::Duration;

    fn model() -> PlantModel {
        PlantModel::seeded(
            Signal::new(0.4),
            Signal::new(0.5),
            Signal::new(false),
            Signal::new(true),
            Signal::new(String::new()),
        )
    }

    /// Tab labels are domain names — uppercase, no widget vocabulary.
    #[test]
    fn pages_are_domain_named() {
        let m = model();
        let pages = pages(&m);
        assert_eq!(pages.len(), 6);
        for (label, _page) in &pages {
            assert!(
                !label.is_empty() && label.chars().all(|c| !c.is_lowercase()),
                "{label} is not a domain name"
            );
            for widget_word in [
                "SLIDER", "DROPDOWN", "PICKER", "CANVAS", "MENU", "TABS", "INPUT", "WIDGET",
            ] {
                assert!(!label.contains(widget_word), "{label} names a widget");
            }
        }
    }

    /// Write-path round-trip: the form's progress slider publishes a
    /// simulated drag into `work_orders` through `update_wo`.
    #[test]
    fn slider_pull_writes_selected_wo() {
        let m = model();
        m.selected_wo.set(Some(4471));
        let mut b = progress_slider(&m, 50.0);
        b.inner_mut().set_value(80.0);
        b.tick(Duration::from_millis(16));
        let wo = m
            .work_orders
            .get()
            .into_iter()
            .find(|w| w.id == 4471)
            .expect("WO-4471 seeded");
        assert!((wo.progress - 0.8).abs() < 1e-9);
    }

    /// Second leg of the round-trip: a model write reflects back into
    /// the widget on `push`.
    #[test]
    fn slider_push_reflects_model() {
        let m = model();
        m.selected_wo.set(Some(4471));
        let mut b = progress_slider(&m, 50.0);
        m.update_wo(|w| w.progress = 0.25);
        b.tick(Duration::from_millis(16));
        assert!((b.inner().value() - 25.0).abs() < 1e-9);
    }

    /// The Segmented status control's drain writes the WO status.
    #[test]
    fn segmented_pull_writes_status() {
        let m = model();
        m.selected_wo.set(Some(4470)); // Queued
        let mut seg = Bound::new(
            Segmented::new()
                .options(STATUS_COLS.iter().map(|s| s.label()))
                .selected(0),
            &m,
        )
        .pull(|w: &mut Segmented, m| {
            while let Some(i) = w.take_selected() {
                if let Some(st) = STATUS_COLS.get(i) {
                    if sel_wo(m).map(|wo| wo.status) != Some(*st) {
                        m.update_wo(|wo| wo.status = *st);
                    }
                }
            }
        });
        seg.inner_mut().set_selected(3); // Done
        seg.tick(Duration::from_millis(16));
        let wo = m
            .work_orders
            .get()
            .into_iter()
            .find(|w| w.id == 4470)
            .unwrap();
        assert_eq!(wo.status, WoStatus::Done);
    }

    /// The reference week anchor really is a Monday (the due-day
    /// mapping the pickers share).
    #[test]
    fn ref_monday_is_monday() {
        assert_eq!(
            Date::weekday_of(REF_MONDAY.year, REF_MONDAY.month, REF_MONDAY.day),
            1
        );
        let d = date_from_daynum(daynum(REF_MONDAY) + 4);
        assert_eq!(d.day, 7); // Mon + 4 = Fri the 7th
    }
}
