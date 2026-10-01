//! `CatalogView` — the catalog's root widget. It composes the toolbar
//! (search + stage tools), the family rail, the [`StageHost`], the
//! props panel, the reference pane (metadata + live snippet), and the
//! event log into one internal-child tree, reconciling control state
//! with model state once per frame in [`CatalogView::reconcile`].

use std::collections::{HashMap, VecDeque};

use glam::Vec2;
use martensite::core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect, Widget,
};
use martensite::reactive::Signal;
use martensite::theme::tokens;
use martensite::widgets::button::Button;
use martensite::widgets::dropdown::Dropdown;
use martensite::widgets::list_view::ListView;
use martensite::widgets::scrollview::ScrollView;
use martensite::widgets::segmented::Segmented;
use martensite::widgets::slider::Slider;
use martensite::widgets::spinbox::SpinBox;
use martensite::widgets::switch::Switch;
use martensite::widgets::text::Text;
use martensite::widgets::text_input::TextInput;

use crate::dynamic_column::DynamicColumn;
use crate::page::{Page, PropSpec, PropValue, PropValues};
use crate::stage::{FramePreset, StageHost};

const TOOLBAR_H: f32 = 40.0;
const TOOLBAR_ITEM_H: f32 = 30.0;
const RAIL_W: f32 = 236.0;
const PROPS_W: f32 = 340.0;
const BOTTOM_H: f32 = 196.0;
const PAD: f32 = 8.0;
const LOG_CAP: usize = 240;
/// Toolbar left side: the search field's fixed width.
const SEARCH_W: f32 = 220.0;
/// Props panel label column width.
const PROP_LABEL_W: f32 = 110.0;
/// Prop row height.
const PROP_ROW_H: f32 = 34.0;
/// Number of internal children.
const N_CHILDREN: usize = 13;

/// Locales offered by the stage's locale dropdown — two RTL (ar-EG,
/// he-IL), four LTR.
pub const LOCALE_TAGS: &[&str] = &["ar-EG", "de-DE", "en-US", "fr-FR", "he-IL", "ja-JP"];

const THEME_OPTIONS: &[&str] = &["Follow", "Dark", "Light"];
const DIR_OPTIONS: &[&str] = &["LTR", "RTL"];

/// Rail-filter predicate: case-insensitive match on the page's name,
/// family, or any cross-framework alias. `query` is already
/// lowercased by the caller.
pub fn matches_query(meta: &crate::page::PageMeta, query: &str) -> bool {
    query.is_empty()
        || meta.name.to_lowercase().contains(query)
        || meta.family.to_lowercase().contains(query)
        || meta
            .aliases
            .iter()
            .any(|(fw, a)| fw.to_lowercase().contains(query) || a.to_lowercase().contains(query))
}

/// Which theme the stage runs — `Follow` inherits the arena theme.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum StageTheme {
    /// Inherit the catalog's own theme.
    #[default]
    Follow,
    /// Force the dark tokens.
    Dark,
    /// Force the light tokens.
    Light,
}

/// The catalog's root widget.
pub struct CatalogView {
    /// Registered pages, rail order.
    pages: Vec<Box<dyn Page>>,
    /// Rail row → page index (`None` = family header row).
    rail_map: Vec<Option<usize>>,
    /// Currently staged page index into `pages`.
    sel: usize,
    /// Per-page prop values, persisted across navigation.
    prop_values: HashMap<usize, PropValues>,
    /// Ring of event-log lines.
    log: VecDeque<String>,
    /// Last `describe_state` snapshot for diffing.
    last_state: Vec<(String, String)>,
    /// Stage tool state.
    stage_theme: StageTheme,
    rtl: bool,
    locale_idx: Option<usize>,
    frame: FramePreset,
    /// True when the staged widget must be rebuilt (page or props
    /// changed).
    stage_dirty: bool,
    /// Child rects from the last `layout` — event hit-testing and
    /// `child_bounds` read these.
    last_child_bounds: [Rect; N_CHILDREN],

    // ---- dev-channel signals ----
    /// `sel` — staged page index.
    pub sig_sel: Signal<usize>,
    /// `search` — the rail filter.
    pub sig_search: Signal<String>,
    /// `stage_theme` — 0 follow, 1 dark, 2 light.
    pub sig_theme: Signal<usize>,
    /// `rtl` — stage direction override.
    pub sig_rtl: Signal<bool>,
    /// `stage_locale` — index into [`LOCALE_TAGS`], or `usize::MAX`
    /// to follow.
    pub sig_locale: Signal<usize>,
    /// `frame` — index into [`FramePreset::ALL`].
    pub sig_frame: Signal<usize>,
    /// `zoom` — absolute stage zoom.
    pub sig_zoom: Signal<f64>,
    /// `prop` — `"key=value"` applied to the staged page's props.
    pub sig_prop: Signal<String>,
    /// Signal values last applied — reconcile watches for external
    /// writes by comparing.
    last_sig: [usize; 6],
    last_sig_search: String,
    last_sig_prop: String,
    last_sig_zoom: f64,

    // ---- children (fixed order for the child_* protocol) ----
    search: TextInput,        // 0
    theme_seg: Segmented,     // 1
    dir_seg: Segmented,       // 2
    locale_dd: Dropdown,      // 3
    frame_dd: Dropdown,       // 4
    zoom_out: Button,         // 5
    zoom_in: Button,          // 6
    reset_view: Button,       // 7
    rail: ListView,           // 8
    stage: StageHost,         // 9
    props_scroll: ScrollView, // 10
    info: DynamicColumn,      // 11
    log_list: ListView,       // 12
}

impl CatalogView {
    /// A catalog over `pages` — the first page staged.
    pub fn new(pages: Vec<Box<dyn Page>>) -> Self {
        assert!(!pages.is_empty(), "catalog needs at least one page");
        let mut prop_values = HashMap::new();
        for (i, p) in pages.iter().enumerate() {
            prop_values.insert(i, PropValues::from_specs(p.props()));
        }
        let props = prop_values[&0].clone();
        let stage = StageHost::new(pages[0].build(&props));
        let search = TextInput::new("Search").placeholder("Filter widgets…");
        let theme_seg = Segmented::new()
            .options(THEME_OPTIONS.iter().copied())
            .selected(0);
        let dir_seg = Segmented::new()
            .options(DIR_OPTIONS.iter().copied())
            .selected(0);
        let locale_dd = Dropdown::new(std::iter::once("Follow").chain(LOCALE_TAGS.iter().copied()))
            .label("Stage locale");
        let frame_dd =
            Dropdown::new(FramePreset::ALL.iter().map(|f| f.label())).label("Stage frame");
        let zoom_out = Button::new("Zoom out")
            .icon_named("nav.chevron-down")
            .icon_only(true);
        let zoom_in = Button::new("Zoom in")
            .icon_named("nav.chevron-up")
            .icon_only(true);
        let reset_view = Button::new("Reset").tooltip("Reset zoom and pan");

        let mut view = Self {
            rail_map: Vec::new(),
            sel: 0,
            prop_values,
            log: VecDeque::new(),
            last_state: Vec::new(),
            stage_theme: StageTheme::Follow,
            rtl: false,
            locale_idx: None,
            frame: FramePreset::Fill,
            stage_dirty: false,
            last_child_bounds: [Rect::default(); N_CHILDREN],
            sig_sel: Signal::new(0usize),
            sig_search: Signal::new(String::new()),
            sig_theme: Signal::new(0usize),
            sig_rtl: Signal::new(false),
            sig_locale: Signal::new(usize::MAX),
            sig_frame: Signal::new(0usize),
            sig_zoom: Signal::new(1.0f64),
            sig_prop: Signal::new(String::new()),
            last_sig: [0, 0, usize::MAX, 0, 0, 0],
            last_sig_search: String::new(),
            last_sig_prop: String::new(),
            last_sig_zoom: 1.0,
            search,
            theme_seg,
            dir_seg,
            locale_dd,
            frame_dd,
            zoom_out,
            zoom_in,
            reset_view,
            rail: ListView::new().label("Widget rail"),
            stage,
            props_scroll: ScrollView::new(DynamicColumn::new().gap(6.0)),
            info: DynamicColumn::new().gap(3.0),
            log_list: ListView::new().label("Event log"),
            pages,
        };
        view.rebuild_rail();
        view.rebuild_props_panel();
        view.rebuild_info();
        view
    }

    /// Human label for the selected page.
    pub fn selected_name(&self) -> &'static str {
        self.pages[self.sel].meta().name
    }

    /// Pushes a line onto the event log (newest last), capping at
    /// [`LOG_CAP`].
    fn log_line(&mut self, line: String) {
        if self.log.len() >= LOG_CAP {
            self.log.pop_front();
        }
        self.log.push_back(line);
        self.log_list
            .set_items(self.log.iter().cloned().collect::<Vec<_>>());
    }

    /// Rebuilds the rail rows for the current query — family header
    /// rows carry `None` in `rail_map`.
    fn rebuild_rail(&mut self) {
        let query = self.search.value.to_lowercase();
        self.rail_map.clear();
        let mut items = Vec::new();
        let mut last_family = "";
        for (i, page) in self.pages.iter().enumerate() {
            let meta = page.meta();
            if !matches_query(&meta, &query) {
                continue;
            }
            if query.is_empty() && meta.family != last_family {
                last_family = meta.family;
                items.push(format!("── {} ──", meta.family));
                self.rail_map.push(None);
            }
            items.push(meta.name.to_string());
            self.rail_map.push(Some(i));
        }
        self.rail.set_items(items);
        // Keep the rail highlight on the staged page.
        if let Some(row) = self.rail_map.iter().position(|m| *m == Some(self.sel)) {
            self.rail.set_selected(row);
        }
    }

    /// The props-panel host (the column inside the scroll view).
    fn props_host_mut(&mut self) -> Option<&mut DynamicColumn> {
        self.props_scroll
            .child_mut(0)?
            .as_any_mut()?
            .downcast_mut::<DynamicColumn>()
    }

    /// Builds the props panel for the selected page's spec list.
    fn rebuild_props_panel(&mut self) {
        let props = self.prop_values[&self.sel].clone();
        let specs = self.pages[self.sel].props();
        let mut rows: Vec<Box<dyn Widget>> = Vec::new();
        for spec in specs {
            let control: Box<dyn Widget> = match spec {
                PropSpec::Bool { default, .. } => {
                    let on = props
                        .get(spec.key())
                        .map_or(*default, |v| matches!(v, PropValue::Bool(b) if *b));
                    // Empty label — the row's label column carries the
                    // name; the switch paints only the track/thumb.
                    Box::new(Switch::new("").on(on))
                }
                PropSpec::Float {
                    min,
                    max,
                    step,
                    default,
                    ..
                } => {
                    let v = props
                        .get(spec.key())
                        .and_then(|v| match v {
                            PropValue::Float(f) => Some(*f),
                            _ => None,
                        })
                        .unwrap_or(*default)
                        .clamp(*min, *max);
                    Box::new(Slider::new(*min, *max).with_value(v).step(*step))
                }
                PropSpec::Int {
                    min, max, default, ..
                } => {
                    let v = props
                        .get(spec.key())
                        .and_then(|v| match v {
                            PropValue::Int(i) => Some(*i as f64),
                            _ => None,
                        })
                        .unwrap_or(*default as f64)
                        .clamp(*min as f64, *max as f64);
                    Box::new(SpinBox::new().range(*min as f64, *max as f64).with_value(v))
                }
                PropSpec::Text { default, .. } => {
                    let v = props
                        .get(spec.key())
                        .and_then(|v| match v {
                            PropValue::Text(t) => Some(t.clone()),
                            _ => None,
                        })
                        .unwrap_or_else(|| default.to_string());
                    Box::new(TextInput::new(spec.label()).value(v))
                }
                PropSpec::Choice {
                    options, default, ..
                } => {
                    let sel = props
                        .get(spec.key())
                        .and_then(|v| match v {
                            PropValue::Choice(i) => Some(*i),
                            _ => None,
                        })
                        .unwrap_or(*default)
                        .min(options.len() - 1);
                    if options.len() <= 4 {
                        Box::new(
                            Segmented::new()
                                .options(options.iter().copied())
                                .selected(sel),
                        )
                    } else {
                        let mut dd = Dropdown::new(options.iter().copied()).label(spec.label());
                        dd.commit(sel);
                        Box::new(dd)
                    }
                }
            };
            rows.push(Box::new(PropRow::new(spec.label(), control)));
        }
        if let Some(host) = self.props_host_mut() {
            host.set_children(rows);
        }
    }

    /// Rebuilds the reference pane: name/role/description/aliases plus
    /// the live snippet lines.
    fn rebuild_info(&mut self) {
        let meta = self.pages[self.sel].meta();
        let props = &self.prop_values[&self.sel];
        let snippet = self.pages[self.sel].snippet(props);
        let mut rows: Vec<Box<dyn Widget>> = Vec::new();
        rows.push(Box::new(Text::new(meta.name.to_string()).font_size(18.0)));
        rows.push(Box::new(Text::new(format!(
            "Role: {}  ·  {}",
            meta.role, meta.family
        ))));
        if !meta.aliases.is_empty() {
            let aliases = meta
                .aliases
                .iter()
                .map(|(fw, a)| format!("{fw}: {a}"))
                .collect::<Vec<_>>()
                .join("   |   ");
            rows.push(Box::new(Text::new(format!("Aliases: {aliases}"))));
        }
        rows.push(Box::new(Text::new(meta.description.to_string())));
        rows.push(Box::new(Text::new("Snippet:".to_string()).font_size(13.0)));
        for line in snippet.lines() {
            rows.push(Box::new(
                Text::new(line.to_string())
                    .font_size(12.0)
                    .family("monospace"),
            ));
        }
        self.info.set_children(rows);
    }

    /// Drains a prop row's control value. Returns `Some(value)` when it
    /// changed (or when the control type reports continuous values —
    /// callers compare before dirtying).
    fn read_control(row: &mut dyn Widget, spec: &PropSpec) -> Option<PropValue> {
        let control = row.child_mut(1)?;
        match spec {
            PropSpec::Bool { .. } => control
                .as_any_mut()?
                .downcast_mut::<Switch>()
                .map(|s| PropValue::Bool(s.on)),
            PropSpec::Float { min, max, .. } => control
                .as_any_mut()?
                .downcast_mut::<Slider>()
                .map(|s| PropValue::Float(s.value().clamp(*min, *max))),
            PropSpec::Int { min, max, .. } => control
                .as_any_mut()?
                .downcast_mut::<SpinBox>()
                .map(|s| PropValue::Int(s.value().round().clamp(*min as f64, *max as f64) as i64)),
            PropSpec::Text { .. } => {
                let input = control.as_any_mut()?.downcast_mut::<TextInput>()?;
                input
                    .take_edited()
                    .then(|| PropValue::Text(input.value.clone()))
            }
            PropSpec::Choice { options, .. } => {
                if options.len() <= 4 {
                    let seg = control.as_any_mut()?.downcast_mut::<Segmented>()?;
                    seg.take_selected()
                        .map(|i| PropValue::Choice(i.min(options.len() - 1)))
                } else {
                    let dd = control.as_any_mut()?.downcast_mut::<Dropdown>()?;
                    Some(PropValue::Choice(dd.selected().min(options.len() - 1)))
                }
            }
        }
    }

    /// One reconcile pass — drains controls and signals, rebuilds the
    /// stage/props/info on change, appends drained widget events to the
    /// log. Call once per frame after `arena.tick`.
    pub fn reconcile(&mut self) {
        // ---- signals → state ----
        let sig_sel = self.sig_sel.get();
        if sig_sel != self.last_sig[0] {
            self.last_sig[0] = sig_sel;
            if sig_sel < self.pages.len() && sig_sel != self.sel {
                self.select(sig_sel);
            }
        }
        let sig_search = self.sig_search.get();
        if sig_search != self.last_sig_search {
            self.last_sig_search = sig_search.clone();
            if sig_search != self.search.value {
                self.search.set_value(sig_search);
                self.rebuild_rail();
            }
        }
        let sig_theme = self.sig_theme.get();
        if sig_theme != self.last_sig[1] {
            self.last_sig[1] = sig_theme;
            self.set_stage_theme(match sig_theme {
                1 => StageTheme::Dark,
                2 => StageTheme::Light,
                _ => StageTheme::Follow,
            });
        }
        let sig_rtl = self.sig_rtl.get();
        if sig_rtl != (self.last_sig[3] != 0) {
            self.last_sig[3] = usize::from(sig_rtl);
            self.set_rtl(sig_rtl);
        }
        let sig_locale = self.sig_locale.get();
        if sig_locale != self.last_sig[2] {
            self.last_sig[2] = sig_locale;
            let idx = (sig_locale < LOCALE_TAGS.len()).then_some(sig_locale);
            self.set_locale(idx);
        }
        let sig_frame = self.sig_frame.get();
        if sig_frame != self.last_sig[4] {
            self.last_sig[4] = sig_frame;
            if let Some(&f) = FramePreset::ALL.get(sig_frame) {
                if f != self.frame {
                    self.frame = f;
                    self.stage.set_frame(f);
                }
            }
        }
        let sig_zoom = self.sig_zoom.get();
        if sig_zoom != self.last_sig_zoom {
            self.last_sig_zoom = sig_zoom;
            self.stage.set_zoom(sig_zoom as f32);
        }
        let sig_prop = self.sig_prop.get();
        if !sig_prop.is_empty() && sig_prop != self.last_sig_prop {
            self.last_sig_prop = sig_prop.clone();
            if let Some((key, value)) = sig_prop.split_once('=') {
                self.apply_prop_text(key, value);
            }
        }

        // ---- controls → state ----
        if self.search.take_edited() {
            self.sig_search.set(self.search.value.clone());
            self.last_sig_search = self.search.value.clone();
            self.rebuild_rail();
        }
        if let Some(i) = self.rail.take_activated() {
            if let Some(Some(page)) = self.rail_map.get(i).copied() {
                self.select(page);
            }
        }
        if let Some(row) = self.rail.selected() {
            if let Some(Some(page)) = self.rail_map.get(row).copied() {
                if page != self.sel {
                    self.select(page);
                }
            }
        }
        if let Some(i) = self.theme_seg.take_selected() {
            self.set_stage_theme(match i {
                1 => StageTheme::Dark,
                2 => StageTheme::Light,
                _ => StageTheme::Follow,
            });
        }
        if let Some(i) = self.dir_seg.take_selected() {
            self.set_rtl(i == 1);
        }
        let locale_sel = self.locale_dd.selected();
        let new_idx = if locale_sel == 0 {
            None
        } else {
            Some(locale_sel - 1)
        };
        if new_idx != self.locale_idx {
            self.set_locale(new_idx);
        }
        let frame_sel = self.frame_dd.selected();
        if let Some(&f) = FramePreset::ALL.get(frame_sel) {
            if f != self.frame {
                self.frame = f;
                self.stage.set_frame(f);
            }
        }
        if self.zoom_out.take_activated() {
            self.stage.set_zoom(self.stage.zoom() * 0.8);
        }
        if self.zoom_in.take_activated() {
            self.stage.set_zoom(self.stage.zoom() * 1.25);
        }
        if self.reset_view.take_activated() {
            self.stage.reset_view();
        }

        // ---- prop controls → prop values ----
        let specs = self.pages[self.sel].props().to_vec();
        let mut edits = Vec::new();
        if let Some(host) = self.props_host_mut() {
            for (i, spec) in specs.iter().enumerate() {
                if let Some(row) = host.child_mut(i) {
                    if let Some(v) = Self::read_control(row, spec) {
                        edits.push((spec.key(), v));
                    }
                }
            }
        }
        if !edits.is_empty() {
            let vals = self.prop_values.get_mut(&self.sel).unwrap();
            for (k, v) in edits {
                if vals.get(k) != Some(&v) {
                    vals.set(k, v);
                    self.stage_dirty = true;
                }
            }
        }

        if self.stage_dirty {
            self.stage_dirty = false;
            let props = self.prop_values[&self.sel].clone();
            self.stage.set_child(self.pages[self.sel].build(&props));
            self.rebuild_info();
        }

        // ---- staged widget events + state diffs → log ----
        let page = &self.pages[self.sel];
        let mut lines = Vec::new();
        let mut state = Vec::new();
        if let Some(staged) = self.stage.staged_mut() {
            page.poll_events(staged, &mut lines);
            state = page.describe_state(staged);
        }
        for line in lines {
            self.log_line(line);
        }
        let mut diffs = Vec::new();
        for (k, v) in state {
            match self.last_state.iter_mut().find(|(lk, _)| *lk == k) {
                Some((_, lv)) if *lv == v => {}
                Some((_, lv)) => {
                    diffs.push(format!("{k} → {v}"));
                    *lv = v;
                }
                None => {
                    self.last_state.push((k, v));
                }
            }
        }
        for line in diffs {
            self.log_line(line);
        }
    }

    /// Selects a page by absolute index.
    fn select(&mut self, page: usize) {
        if page == self.sel || page >= self.pages.len() {
            return;
        }
        self.sel = page;
        self.last_state.clear();
        self.sig_sel.set(page);
        self.last_sig[0] = page;
        let props = self.prop_values[&page].clone();
        self.stage.set_child(self.pages[page].build(&props));
        self.rebuild_props_panel();
        self.rebuild_info();
        if let Some(row) = self.rail_map.iter().position(|m| *m == Some(page)) {
            self.rail.set_selected(row);
            self.rail.scroll_row_into_view(row);
        }
        self.log_line(format!("page → {}", self.pages[page].meta().name));
    }

    /// Applies `"key=value"` text (dev channel `prop` signal) to the
    /// staged page's props.
    fn apply_prop_text(&mut self, key: &str, value: &str) {
        let Some(spec) = self.pages[self.sel].props().iter().find(|s| s.key() == key) else {
            self.log_line(format!("prop {key}: unknown"));
            return;
        };
        let parsed = match spec {
            PropSpec::Bool { .. } => value.parse::<bool>().ok().map(PropValue::Bool),
            PropSpec::Float { .. } => value.parse::<f64>().ok().map(PropValue::Float),
            PropSpec::Int { .. } => value.parse::<i64>().ok().map(PropValue::Int),
            PropSpec::Text { .. } => Some(PropValue::Text(value.to_string())),
            PropSpec::Choice { options, .. } => value
                .parse::<usize>()
                .ok()
                .or_else(|| options.iter().position(|o| *o == value))
                .map(PropValue::Choice),
        };
        match parsed {
            Some(v) => {
                self.prop_values
                    .get_mut(&self.sel)
                    .unwrap()
                    .set(spec.key(), v);
                self.stage_dirty = true;
                self.rebuild_props_panel();
            }
            None => self.log_line(format!("prop {key}: bad value {value:?}")),
        }
    }

    /// Sets the stage theme override.
    fn set_stage_theme(&mut self, theme: StageTheme) {
        self.stage_theme = theme;
        self.stage.set_theme(match theme {
            StageTheme::Follow => None,
            StageTheme::Dark => Some(tokens::default_dark()),
            StageTheme::Light => Some(tokens::default_light()),
        });
        let idx = match theme {
            StageTheme::Follow => 0,
            StageTheme::Dark => 1,
            StageTheme::Light => 2,
        };
        self.sig_theme.set(idx);
        self.last_sig[1] = idx;
        self.theme_seg.set_selected(idx);
    }

    /// Sets the stage direction override.
    fn set_rtl(&mut self, rtl: bool) {
        self.rtl = rtl;
        self.stage
            .set_direction(rtl.then_some(martensite::core::LayoutDirection::Rtl));
        self.sig_rtl.set(rtl);
        self.last_sig[3] = usize::from(rtl);
        self.dir_seg.set_selected(usize::from(rtl));
    }

    /// Sets the stage locale override.
    fn set_locale(&mut self, idx: Option<usize>) {
        self.locale_idx = idx;
        self.stage
            .set_locale(idx.map(|i| martensite::core::Locale::new(LOCALE_TAGS[i])));
        self.sig_locale.set(idx.unwrap_or(usize::MAX));
        self.last_sig[2] = self.sig_locale.get();
        let sel = idx.map_or(0, |i| i + 1);
        self.locale_dd.commit(sel);
    }

    /// Toolbar child rects — left: search; right-aligned group: theme,
    /// direction, locale, frame, zoom controls.
    fn toolbar_child_rects(&mut self, cx: &mut LayoutContext, strip: Rect) -> [Rect; 8] {
        let item_y = strip.min_y() + (strip.height() - TOOLBAR_ITEM_H) * 0.5;
        let mut out = [Rect::default(); 8];
        let c = LayoutConstraints {
            min_size: Vec2::ZERO,
            max_size: Vec2::new(strip.width(), TOOLBAR_ITEM_H),
        };
        out[0] = Rect::new(strip.min_x() + PAD, item_y, SEARCH_W, TOOLBAR_ITEM_H);
        // Right-aligned: measure each control for its intrinsic width.
        let mut x = strip.max_x() - PAD;
        for child_idx in (1..8usize).rev() {
            let size = self.child_mut(child_idx).unwrap().measure(cx, c);
            let w = size.x.clamp(32.0, 220.0);
            x -= w;
            out[child_idx] = Rect::new(x, item_y, w, TOOLBAR_ITEM_H);
            x -= PAD;
        }
        out
    }
}

/// A props-panel row: fixed-width label on the left, control filling
/// the rest. Child 0 is the label, child 1 the control —
/// [`CatalogView::read_control`] drains `child_mut(1)`.
struct PropRow {
    label: Text,
    control: Box<dyn Widget>,
    label_b: Rect,
    ctrl_b: Rect,
}

impl PropRow {
    fn new(label: &'static str, control: Box<dyn Widget>) -> Self {
        Self {
            label: Text::new(label.to_string()).font_size(12.0),
            control,
            label_b: Rect::default(),
            ctrl_b: Rect::default(),
        }
    }
}

impl Widget for PropRow {
    fn measure(&mut self, cx: &mut LayoutContext, c: LayoutConstraints) -> Vec2 {
        let lw = PROP_LABEL_W.min(c.max_size.x * 0.45);
        self.label.measure(
            cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(lw, c.max_size.y),
            },
        );
        let cs = self.control.measure(
            cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new((c.max_size.x - lw - PAD).max(0.0), c.max_size.y),
            },
        );
        Vec2::new(c.max_size.x, cs.y.max(PROP_ROW_H * 0.8))
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        let lw = PROP_LABEL_W.min(bounds.width() * 0.45);
        self.label_b = Rect::new(bounds.min_x(), bounds.min_y(), lw, bounds.height());
        self.ctrl_b = Rect::new(
            bounds.min_x() + lw + PAD,
            bounds.min_y(),
            (bounds.width() - lw - PAD).max(0.0),
            bounds.height(),
        );
        cx.layout_child(&mut self.label, self.label_b);
        cx.layout_child(self.control.as_mut(), self.ctrl_b);
    }

    fn paint(&self, _cx: &mut PaintContext) {}

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        self.forward_event_to_children(cx)
    }

    fn child_count(&self) -> usize {
        2
    }

    fn child(&self, i: usize) -> Option<&dyn Widget> {
        match i {
            0 => Some(&self.label),
            1 => Some(&*self.control),
            _ => None,
        }
    }

    fn child_mut(&mut self, i: usize) -> Option<&mut dyn Widget> {
        match i {
            0 => Some(&mut self.label),
            1 => Some(&mut *self.control),
            _ => None,
        }
    }

    fn child_bounds(&self, i: usize) -> Option<Rect> {
        match i {
            0 => Some(self.label_b),
            1 => Some(self.ctrl_b),
            _ => None,
        }
    }
}

impl Widget for CatalogView {
    fn measure(&mut self, _cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        constraints.max_size
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        let w = bounds.width();
        let h = bounds.height();
        let x0 = bounds.min_x();
        let y0 = bounds.min_y();

        // Toolbar strip: search at left, controls right-aligned.
        let strip = Rect::new(x0, y0, w, TOOLBAR_H);
        let tool_rects = self.toolbar_child_rects(cx, strip);
        for (i, r) in tool_rects.iter().enumerate() {
            self.last_child_bounds[i] = *r;
            cx.layout_child(self.child_mut(i).unwrap(), *r);
        }

        // Columns below the toolbar.
        let body_y = y0 + TOOLBAR_H + PAD;
        let body_h = (h - TOOLBAR_H - PAD - PAD).max(0.0);
        let rail = Rect::new(x0 + PAD, body_y, RAIL_W, body_h);
        let props = Rect::new(x0 + w - PAD - PROPS_W, body_y, PROPS_W, body_h);
        let center_x = rail.min_x() + rail.width() + PAD;
        let center_w = (props.min_x() - PAD - center_x).max(0.0);
        let stage_rect = Rect::new(
            center_x,
            body_y,
            center_w,
            (body_h - BOTTOM_H - PAD).max(0.0),
        );
        let bottom = Rect::new(
            center_x,
            body_y + stage_rect.height() + PAD,
            center_w,
            BOTTOM_H,
        );
        let info_rect = Rect::new(
            bottom.min_x(),
            bottom.min_y(),
            (center_w - PAD) * 0.5,
            bottom.height(),
        );
        let log_rect = Rect::new(
            info_rect.min_x() + info_rect.width() + PAD,
            bottom.min_y(),
            (center_w - PAD) * 0.5,
            bottom.height(),
        );

        let big: [(usize, Rect); 5] = [
            (8, rail),
            (9, stage_rect),
            (10, props),
            (11, info_rect),
            (12, log_rect),
        ];
        for (i, r) in big {
            self.last_child_bounds[i] = r;
            let c = LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(r.width(), r.height()),
            };
            let child = self.child_mut(i).unwrap();
            child.measure(cx, c);
            cx.layout_child(child, r);
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        // Toolbar + bottom-panel backplates.
        let bg = cx.color(martensite::theme::TokenKey::SurfaceColor, [30, 33, 40, 255]);
        let b = cx.bounds;
        let toolbar = kurbo::Rect::new(
            f64::from(b.min_x()),
            f64::from(b.min_y()),
            f64::from(b.max_x()),
            f64::from(b.min_y() + TOOLBAR_H),
        );
        cx.list.push_fill_rect(toolbar, bg);
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        self.forward_event_to_children(cx)
    }

    fn child_count(&self) -> usize {
        N_CHILDREN
    }

    fn child(&self, i: usize) -> Option<&dyn Widget> {
        Some(match i {
            0 => &self.search,
            1 => &self.theme_seg,
            2 => &self.dir_seg,
            3 => &self.locale_dd,
            4 => &self.frame_dd,
            5 => &self.zoom_out,
            6 => &self.zoom_in,
            7 => &self.reset_view,
            8 => &self.rail,
            9 => &self.stage,
            10 => &self.props_scroll,
            11 => &self.info,
            12 => &self.log_list,
            _ => return None,
        })
    }

    fn child_mut(&mut self, i: usize) -> Option<&mut dyn Widget> {
        Some(match i {
            0 => &mut self.search,
            1 => &mut self.theme_seg,
            2 => &mut self.dir_seg,
            3 => &mut self.locale_dd,
            4 => &mut self.frame_dd,
            5 => &mut self.zoom_out,
            6 => &mut self.zoom_in,
            7 => &mut self.reset_view,
            8 => &mut self.rail,
            9 => &mut self.stage,
            10 => &mut self.props_scroll,
            11 => &mut self.info,
            _ => &mut self.log_list,
        })
    }

    fn child_bounds(&self, i: usize) -> Option<Rect> {
        self.last_child_bounds.get(i).copied()
    }

    fn accessibility(&self, node: &mut accesskit::Node) {
        node.set_role(accesskit::Role::GenericContainer);
        node.set_label("Widget Catalog");
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
}
