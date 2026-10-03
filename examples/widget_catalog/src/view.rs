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
use martensite::theme::{tokens, TokenKey};
use martensite::widgets::button::Button;
use martensite::widgets::dropdown::Dropdown;
use martensite::widgets::flex::{CrossAxisAlignment, Flex};
use martensite::widgets::list_view::ListView;
use martensite::widgets::scrollview::ScrollView;
use martensite::widgets::segmented::Segmented;
use martensite::widgets::separator::Separator;
use martensite::widgets::slider::Slider;
use martensite::widgets::spinbox::SpinBox;
use martensite::widgets::switch::Switch;
use martensite::widgets::text::Text;
use martensite::widgets::text_input::TextInput;

use crate::dynamic_column::DynamicColumn;
use crate::page::{Page, PropSpec, PropValue, PropValues};
use crate::stage::{FramePreset, StageHost};

const TOOLBAR_H: f32 = 46.0;
const TOOLBAR_ITEM_H: f32 = 34.0;
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
const N_CHILDREN: usize = 7;
/// Tool-cluster child indices — the right-aligned toolbar group.
const TOOL_THEME: usize = 0;
const TOOL_DIR: usize = 1;
const TOOL_LOCALE: usize = 2;
const TOOL_FRAME: usize = 3;
const TOOL_ZOOM_OUT: usize = 4;
const TOOL_ZOOM_IN: usize = 5;
const TOOL_RESET: usize = 6;

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
    /// Left cluster — the rail filter field.
    nav_cluster: Cluster, // 0
    /// Right-aligned cluster — the stage tools (theme, direction,
    /// locale, frame, zoom controls).
    tools_cluster: Cluster, // 1
    rail: ListView,           // 2
    stage: StageHost,         // 3
    props_scroll: ScrollView, // 4
    info: ScrollView,         // 5
    log_list: ListView,       // 6
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

        let nav_cluster = Cluster::new("NavGroup", vec![Box::new(search)], false);
        let tools_cluster = Cluster::new(
            "ToolsGroup",
            vec![
                Box::new(theme_seg),
                Box::new(dir_seg),
                Box::new(locale_dd),
                Box::new(frame_dd),
                Box::new(zoom_out),
                Box::new(zoom_in),
                Box::new(reset_view),
            ],
            true,
        );

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
            nav_cluster,
            tools_cluster,
            rail: ListView::new().label("Widget rail"),
            stage,
            props_scroll: ScrollView::new(DynamicColumn::new().gap(6.0)),
            // `@prose` declares the reference pane's text as document
            // payload — NUREG-0700's packing cap targets at-a-glance
            // readouts, not manuals/snippets. The ScrollView keeps
            // overflow inside the pane (page bodies outgrow 196pt).
            info: ScrollView::new(DynamicColumn::new().gap(3.0).named("InfoPane@prose")),
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
        let query = self.search_mut().value.to_lowercase();
        self.rail_map.clear();
        let mut items = Vec::new();
        let mut headers = Vec::new();
        let mut last_family = "";
        for (i, page) in self.pages.iter().enumerate() {
            let meta = page.meta();
            if !matches_query(&meta, &query) {
                continue;
            }
            if query.is_empty() && meta.family != last_family {
                last_family = meta.family;
                headers.push(items.len());
                items.push(meta.family.to_string());
                self.rail_map.push(None);
            }
            items.push(meta.name.to_string());
            self.rail_map.push(Some(i));
        }
        self.rail.set_items(items);
        self.rail.set_headers(headers);
        // Keep the rail highlight on the staged page.
        if let Some(row) = self.rail_map.iter().position(|m| *m == Some(self.sel)) {
            self.rail.set_selected(row);
        }
    }

    /// The search field (nav cluster's only child).
    fn search_mut(&mut self) -> &mut TextInput {
        self.nav_cluster
            .child_mut(0)
            .and_then(Widget::as_any_mut)
            .and_then(|a| a.downcast_mut::<TextInput>())
            .expect("nav cluster child 0 is the search TextInput")
    }

    /// A stage tool by [`TOOL_*`] index.
    fn tool_mut<T: 'static>(&mut self, i: usize) -> &mut T {
        self.tools_cluster
            .child_mut(i)
            .and_then(Widget::as_any_mut)
            .and_then(|a| a.downcast_mut::<T>())
            .expect("tools cluster child type")
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
                PropSpec::Header { label } => {
                    // Section divider — small-caps muted label with a
                    // trailing hairline (inspector-panel convention).
                    let muted = tokens::default_dark()
                        .color(TokenKey::TextMutedColor)
                        .unwrap_or_else(|| martensite_theme::Oklab::from_srgb(0.55, 0.57, 0.62));
                    rows.push(Box::new(
                        Flex::row()
                            .gap(8.0)
                            .cross_axis_alignment(CrossAxisAlignment::Center)
                            .child(
                                Text::new(label.to_uppercase())
                                    .font_size(10.0)
                                    .letter_spacing(0.08)
                                    .color(muted),
                            )
                            .child_flex(Separator::horizontal(), 1.0),
                    ));
                    continue;
                }
                PropSpec::Bool { default, .. } => {
                    let on = props
                        .get(spec.key())
                        .map_or(*default, |v| matches!(v, PropValue::Bool(b) if *b));
                    // Empty visual label — the row's label column paints
                    // it — but the control still carries the prop name
                    // as its accessible name (`@labeled` for lint).
                    Box::new(Switch::new("").a11y_label(spec.label()).on(on))
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
                    Box::new(
                        Slider::new(*min, *max)
                            .label(spec.label())
                            .with_value(v)
                            .step(*step),
                    )
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
                    Box::new(
                        SpinBox::new()
                            .label(spec.label())
                            .range(*min as f64, *max as f64)
                            .with_value(v),
                    )
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
                                .label(spec.label())
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
        if let Some(col) = self
            .info
            .child_mut(0)
            .and_then(|c| c.as_any_mut())
            .and_then(|a| a.downcast_mut::<DynamicColumn>())
        {
            col.set_children(rows);
        }
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
            PropSpec::Header { .. } => None,
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
            if sig_search != self.search_mut().value {
                self.search_mut().set_value(sig_search);
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
        if self.search_mut().take_edited() {
            let v = self.search_mut().value.clone();
            self.sig_search.set(v.clone());
            self.last_sig_search = v;
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
        if let Some(i) = self.tool_mut::<Segmented>(TOOL_THEME).take_selected() {
            self.set_stage_theme(match i {
                1 => StageTheme::Dark,
                2 => StageTheme::Light,
                _ => StageTheme::Follow,
            });
        }
        if let Some(i) = self.tool_mut::<Segmented>(TOOL_DIR).take_selected() {
            self.set_rtl(i == 1);
        }
        let locale_sel = self.tool_mut::<Dropdown>(TOOL_LOCALE).selected();
        let new_idx = if locale_sel == 0 {
            None
        } else {
            Some(locale_sel - 1)
        };
        if new_idx != self.locale_idx {
            self.set_locale(new_idx);
        }
        let frame_sel = self.tool_mut::<Dropdown>(TOOL_FRAME).selected();
        if let Some(&f) = FramePreset::ALL.get(frame_sel) {
            if f != self.frame {
                self.frame = f;
                self.stage.set_frame(f);
            }
        }
        if self.tool_mut::<Button>(TOOL_ZOOM_OUT).take_activated() {
            self.stage.set_zoom(self.stage.zoom() * 0.8);
        }
        if self.tool_mut::<Button>(TOOL_ZOOM_IN).take_activated() {
            self.stage.set_zoom(self.stage.zoom() * 1.25);
        }
        if self.tool_mut::<Button>(TOOL_RESET).take_activated() {
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

    /// Selects a page by absolute index (harness/dev-channel entry).
    pub fn select_page(&mut self, page: usize) {
        self.select(page);
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
    pub fn apply_prop_text(&mut self, key: &str, value: &str) {
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
            PropSpec::Header { .. } => None,
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
        self.tool_mut::<Segmented>(TOOL_THEME).set_selected(idx);
    }

    /// Sets the stage direction override.
    fn set_rtl(&mut self, rtl: bool) {
        self.rtl = rtl;
        self.stage
            .set_direction(rtl.then_some(martensite::core::LayoutDirection::Rtl));
        self.sig_rtl.set(rtl);
        self.last_sig[3] = usize::from(rtl);
        self.tool_mut::<Segmented>(TOOL_DIR)
            .set_selected(usize::from(rtl));
    }

    /// Sets the stage locale override.
    fn set_locale(&mut self, idx: Option<usize>) {
        self.locale_idx = idx;
        self.stage
            .set_locale(idx.map(|i| martensite::core::Locale::new(LOCALE_TAGS[i])));
        self.sig_locale.set(idx.unwrap_or(usize::MAX));
        self.last_sig[2] = self.sig_locale.get();
        let sel = idx.map_or(0, |i| i + 1);
        self.tool_mut::<Dropdown>(TOOL_LOCALE).commit(sel);
    }

    /// Toolbar child rects — the nav cluster pinned left, the tools
    /// cluster filling the rest and right-aligning its own controls.
    fn toolbar_child_rects(&self, strip: Rect, scale: f32) -> [Rect; 2] {
        let item_h = TOOLBAR_ITEM_H * scale;
        let pad = PAD * scale;
        let item_y = strip.min_y() + (strip.height() - item_h) * 0.5;
        let nav = Rect::new(strip.min_x() + pad, item_y, SEARCH_W * scale, item_h);
        let tools_x = nav.max_x() + pad;
        let tools = Rect::new(
            tools_x,
            item_y,
            (strip.max_x() - pad - tools_x).max(0.0),
            item_h,
        );
        [nav, tools]
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
        let lw = cx.pt(PROP_LABEL_W).min(c.max_size.x * 0.45);
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
                max_size: Vec2::new((c.max_size.x - lw - cx.pt(PAD)).max(0.0), c.max_size.y),
            },
        );
        Vec2::new(c.max_size.x, cs.y.max(cx.pt(PROP_ROW_H * 0.8)))
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        let lw = cx.pt(PROP_LABEL_W).min(bounds.width() * 0.45);
        self.label_b = Rect::new(bounds.min_x(), bounds.min_y(), lw, bounds.height());
        self.ctrl_b = Rect::new(
            bounds.min_x() + lw + cx.pt(PAD),
            bounds.min_y(),
            (bounds.width() - lw - cx.pt(PAD)).max(0.0),
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

/// A named horizontal cluster of controls — a semantic grouping scope
/// (nav vs stage tools) so the toolbar reads as categories instead of
/// one flat strip of actions. Child bounds come from the last
/// [`Cluster::layout`]; `child_bounds` feeds event hit-testing.
struct Cluster {
    name: &'static str,
    children: Vec<Box<dyn Widget>>,
    rects: Vec<Rect>,
    /// Right-pack children against the cluster's trailing edge.
    align_end: bool,
}

impl Cluster {
    fn new(name: &'static str, children: Vec<Box<dyn Widget>>, align_end: bool) -> Self {
        Self {
            name,
            children,
            rects: Vec::new(),
            align_end,
        }
    }
}

impl Widget for Cluster {
    fn measure(&mut self, cx: &mut LayoutContext, c: LayoutConstraints) -> Vec2 {
        let item = LayoutConstraints {
            min_size: Vec2::ZERO,
            max_size: Vec2::new(c.max_size.x, c.max_size.y),
        };
        let mut w = 0.0f32;
        for (i, child) in self.children.iter_mut().enumerate() {
            w += child.measure(cx, item).x.clamp(cx.pt(32.0), cx.pt(260.0));
            if i > 0 {
                w += cx.pt(PAD);
            }
        }
        Vec2::new(w.min(c.max_size.x), c.max_size.y.min(cx.pt(TOOLBAR_ITEM_H)))
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.rects.clear();
        self.rects.resize(self.children.len(), Rect::default());
        let item = LayoutConstraints {
            min_size: Vec2::ZERO,
            max_size: Vec2::new(bounds.width(), bounds.height()),
        };
        // A lone child fills the cluster (the search field takes its
        // fixed-width strip); multi-child clusters pack at measured
        // widths, right-aligned when `align_end`.
        let single = self.children.len() == 1;
        let widths: Vec<f32> = self
            .children
            .iter_mut()
            .map(|c| {
                if single {
                    bounds.width()
                } else {
                    c.measure(cx, item).x.clamp(cx.pt(32.0), cx.pt(260.0))
                }
            })
            .collect();
        let mut x = if self.align_end {
            let total =
                widths.iter().sum::<f32>() + cx.pt(PAD) * widths.len().saturating_sub(1) as f32;
            bounds.max_x() - total
        } else {
            bounds.min_x()
        };
        for (i, child) in self.children.iter_mut().enumerate() {
            let w = widths[i];
            let r = Rect::new(x, bounds.min_y(), w, bounds.height());
            self.rects[i] = r;
            cx.layout_child(child.as_mut(), r);
            x += w + cx.pt(PAD);
        }
    }

    fn paint(&self, _cx: &mut PaintContext) {}

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        self.forward_event_to_children(cx)
    }

    fn child_count(&self) -> usize {
        self.children.len()
    }

    fn child(&self, i: usize) -> Option<&dyn Widget> {
        self.children.get(i).map(|c| &**c)
    }

    fn child_mut(&mut self, i: usize) -> Option<&mut dyn Widget> {
        self.children.get_mut(i).map(|c| &mut **c)
    }

    fn child_bounds(&self, i: usize) -> Option<Rect> {
        self.rects.get(i).copied()
    }

    fn debug_name(&self) -> &'static str {
        self.name
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

        // Toolbar strip: nav cluster left, tools cluster right-aligned.
        // All chrome dimensions are logical pt — `cx.pt` carries them
        // to device px so HiDPI scale doesn't shrink the bar.
        let th = cx.pt(TOOLBAR_H);
        let pad = cx.pt(PAD);
        let strip = Rect::new(x0, y0, w, th);
        let tool_rects = self.toolbar_child_rects(strip, cx.scale);
        for (i, r) in tool_rects.iter().enumerate() {
            self.last_child_bounds[i] = *r;
            cx.layout_child(self.child_mut(i).unwrap(), *r);
        }

        // Columns below the toolbar.
        let body_y = y0 + th + pad;
        let body_h = (h - th - pad - pad).max(0.0);
        let rail = Rect::new(x0 + pad, body_y, cx.pt(RAIL_W), body_h);
        let props = Rect::new(
            x0 + w - pad - cx.pt(PROPS_W),
            body_y,
            cx.pt(PROPS_W),
            body_h,
        );
        let center_x = rail.min_x() + rail.width() + pad;
        let center_w = (props.min_x() - pad - center_x).max(0.0);
        let stage_rect = Rect::new(
            center_x,
            body_y,
            center_w,
            (body_h - cx.pt(BOTTOM_H) - pad).max(0.0),
        );
        let bottom = Rect::new(
            center_x,
            body_y + stage_rect.height() + pad,
            center_w,
            cx.pt(BOTTOM_H),
        );
        let info_rect = Rect::new(
            bottom.min_x(),
            bottom.min_y(),
            (center_w - pad) * 0.5,
            bottom.height(),
        );
        let log_rect = Rect::new(
            info_rect.min_x() + info_rect.width() + pad,
            bottom.min_y(),
            (center_w - pad) * 0.5,
            bottom.height(),
        );

        let big: [(usize, Rect); 5] = [
            (2, rail),
            (3, stage_rect),
            (4, props),
            (5, info_rect),
            (6, log_rect),
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
            f64::from(b.min_y() + cx.pt(TOOLBAR_H)),
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
            0 => &self.nav_cluster,
            1 => &self.tools_cluster,
            2 => &self.rail,
            3 => &self.stage,
            4 => &self.props_scroll,
            5 => &self.info,
            _ => &self.log_list,
        })
    }

    fn child_mut(&mut self, i: usize) -> Option<&mut dyn Widget> {
        Some(match i {
            0 => &mut self.nav_cluster,
            1 => &mut self.tools_cluster,
            2 => &mut self.rail,
            3 => &mut self.stage,
            4 => &mut self.props_scroll,
            5 => &mut self.info,
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
