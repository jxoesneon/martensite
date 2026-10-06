//! `CatalogView` — the catalog's root widget. It composes the toolbar
//! (search + stage tools), the family rail, the [`StageHost`], the
//! props panel, the reference pane (metadata + live snippet), and the
//! event log into one internal-child tree, reconciling control state
//! with model state once per frame in [`CatalogView::reconcile`].

use std::collections::{HashMap, VecDeque};

use glam::Vec2;
use martensite::core::{
    EventContext, EventResponse, FontWeight, LayoutConstraints, LayoutContext, PaintContext, Rect,
    Widget, WidgetEvent,
};
use martensite::reactive::Signal;
use martensite::theme::{tokens, TokenKey};
use martensite::widgets::button::Button;
use martensite::widgets::container::Container;
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
use martensite_core::shape::Shape;

use crate::dynamic_column::DynamicColumn;
use crate::page::{Page, PropSpec, PropValue, PropValues};
use crate::stage::{FramePreset, StageHost};

const TOOLBAR_H: f32 = 48.0;
const TOOLBAR_ITEM_H: f32 = 34.0;
const RAIL_W: f32 = 252.0;
const PROPS_W: f32 = 344.0;
const BOTTOM_H: f32 = 200.0;
const PAD: f32 = 10.0;
/// Panel card corner radius (squircles).
const CARD_R: f32 = 10.0;
/// Inset between a card's hairline and its content.
const CARD_PAD: f32 = 10.0;
/// Titled-panel header strip height (title + hairline).
const HEAD_H: f32 = 28.0;
const LOG_CAP: usize = 240;
/// Width (logical pt) below which the catalog goes compact: narrower
/// side rails and the reference/event dock spanning the full window
/// under all three columns instead of slivers under the canvas.
const COMPACT_W: f32 = 1060.0;
/// Compact-mode rail width.
const RAIL_W_COMPACT: f32 = 196.0;
/// Compact-mode props width.
const PROPS_W_COMPACT: f32 = 264.0;
/// Props panel label column width.
const PROP_LABEL_W: f32 = 110.0;
/// Prop row height.
const PROP_ROW_H: f32 = 34.0;
/// Number of internal children.
const N_CHILDREN: usize = 6;
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
    /// Compact breakpoint state — the brand subtitle and dock layout
    /// follow it; updated in `layout`.
    compact: bool,
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
    /// Toolbar row — brand block left, stage tools right-aligned.
    toolbar_row: Flex, // 0
    /// Nav card — search field over the widget rail.
    rail_card: PanelCard, // 1
    /// Canvas card — widget breadcrumb over the [`StageHost`].
    canvas_card: PanelCard, // 2
    /// Inspector card — "Properties" header over the prop rows.
    props_card: PanelCard, // 3
    /// Reference card — metadata + live snippet.
    info_card: PanelCard, // 4
    /// Event-log card.
    log_card: PanelCard, // 5
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

        let rail_col = Flex::column()
            .gap(6.0)
            .child(search)
            .child_flex(ListView::new().label("Widget rail"), 1.0);
        let brand = Flex::row()
            .gap(8.0)
            .cross_axis_alignment(CrossAxisAlignment::Center)
            .child(
                Text::new("MARTENSITE")
                    .font_size(10.0)
                    .letter_spacing(0.16)
                    .color(muted_text()),
            )
            .child(
                Text::new("Widget Catalog")
                    .font_size(13.0)
                    .font_weight(FontWeight::SEMIBOLD),
            );
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
            compact: false,
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
            toolbar_row: Flex::row()
                .gap(12.0)
                .cross_axis_alignment(CrossAxisAlignment::Center)
                .child(brand)
                .child_flex(tools_cluster, 1.0),
            rail_card: PanelCard::untitled(rail_col),
            canvas_card: PanelCard::titled(
                &format!("{} / {}", pages[0].meta().family, pages[0].meta().name),
                stage,
            ),
            props_card: PanelCard::titled(
                "Properties",
                ScrollView::new(DynamicColumn::new().gap(6.0)),
            ),
            // `@prose` declares the reference pane's text as document
            // payload — NUREG-0700's packing cap targets at-a-glance
            // readouts, not manuals/snippets. The ScrollView keeps
            // overflow inside the pane (page bodies outgrow 196pt).
            info_card: PanelCard::titled(
                "Reference",
                ScrollView::new(DynamicColumn::new().gap(3.0).named("InfoPane@prose")),
            ),
            log_card: PanelCard::titled("Event Log", ListView::new().label("Event log")),
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
        let items: Vec<String> = self.log.iter().cloned().collect();
        self.log_mut().set_items(items);
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
        self.rail_list_mut().set_items(items);
        self.rail_list_mut().set_headers(headers);
        // Keep the rail highlight on the staged page.
        if let Some(row) = self.rail_map.iter().position(|m| *m == Some(self.sel)) {
            self.rail_list_mut().set_selected(row);
        }
    }

    /// The nav column (search over rail) inside the rail card.
    fn rail_col_mut(&mut self) -> &mut Flex {
        self.rail_card
            .content_mut()
            .as_any_mut()
            .and_then(|a| a.downcast_mut::<Flex>())
            .expect("rail card content is the nav column")
    }

    /// The rail's `ListView` (nav column child 1).
    fn rail_list_mut(&mut self) -> &mut ListView {
        self.rail_col_mut()
            .child_mut(1)
            .and_then(Widget::as_any_mut)
            .and_then(|a| a.downcast_mut::<ListView>())
            .expect("rail column child 1 is the ListView")
    }

    /// The search field (nav column child 0).
    fn search_mut(&mut self) -> &mut TextInput {
        self.rail_col_mut()
            .child_mut(0)
            .and_then(Widget::as_any_mut)
            .and_then(|a| a.downcast_mut::<TextInput>())
            .expect("rail column child 0 is the search TextInput")
    }

    /// The staged widget host inside the canvas card.
    fn stage_mut(&mut self) -> &mut StageHost {
        self.canvas_card
            .content_mut()
            .as_any_mut()
            .and_then(|a| a.downcast_mut::<StageHost>())
            .expect("canvas card content is the StageHost")
    }

    /// The props scroll view inside the inspector card.
    fn props_scroll_mut(&mut self) -> &mut ScrollView {
        self.props_card
            .content_mut()
            .as_any_mut()
            .and_then(|a| a.downcast_mut::<ScrollView>())
            .expect("inspector card content is the props ScrollView")
    }

    /// The reference scroll view inside the info card.
    fn info_scroll_mut(&mut self) -> &mut ScrollView {
        self.info_card
            .content_mut()
            .as_any_mut()
            .and_then(|a| a.downcast_mut::<ScrollView>())
            .expect("info card content is the reference ScrollView")
    }

    /// The event-log list inside its card.
    fn log_mut(&mut self) -> &mut ListView {
        self.log_card
            .content_mut()
            .as_any_mut()
            .and_then(|a| a.downcast_mut::<ListView>())
            .expect("log card content is the event ListView")
    }

    /// The tools cluster (toolbar row child 1).
    fn tools_mut(&mut self) -> &mut Cluster {
        self.toolbar_row
            .child_mut(1)
            .and_then(Widget::as_any_mut)
            .and_then(|a| a.downcast_mut::<Cluster>())
            .expect("toolbar row child 1 is the tools cluster")
    }

    /// A stage tool by [`TOOL_*`] index.
    fn tool_mut<T: 'static>(&mut self, i: usize) -> &mut T {
        self.tools_mut()
            .child_mut(i)
            .and_then(Widget::as_any_mut)
            .and_then(|a| a.downcast_mut::<T>())
            .expect("tools cluster child type")
    }

    /// The props-panel host (the column inside the scroll view).
    fn props_host_mut(&mut self) -> Option<&mut DynamicColumn> {
        self.props_scroll_mut()
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
                    // Section divider — the shared labeled-rule
                    // chrome (small-caps muted + trailing hairline).
                    rows.push(Box::new(labeled_rule(label)));
                    continue;
                }
                PropSpec::Probe { .. } => continue,
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

    /// Rebuilds the reference pane — title, kind line, description,
    /// alias table, and the live snippet in a tinted code block, each
    /// under the shared labeled-rule section chrome.
    fn rebuild_info(&mut self) {
        let meta = self.pages[self.sel].meta();
        let props = &self.prop_values[&self.sel];
        let snippet = self.pages[self.sel].snippet(props);
        let muted = muted_text();
        let secondary = secondary_text();
        let mut rows: Vec<Box<dyn Widget>> = Vec::new();

        // Identity — name at display size, family · role as the
        // subdued kind line underneath.
        rows.push(Box::new(
            Text::new(meta.name.to_string())
                .font_size(18.0)
                .font_weight(FontWeight::SEMIBOLD),
        ));
        rows.push(Box::new(
            Text::new(format!("{} · {}", meta.family, meta.role))
                .font_size(11.0)
                .color(muted),
        ));

        rows.push(Box::new(labeled_rule("About")));
        rows.push(Box::new(
            Text::new(meta.description.to_string())
                .font_size(12.0)
                .color(secondary),
        ));

        if !meta.aliases.is_empty() {
            rows.push(Box::new(labeled_rule("Also known as")));
            // One row per framework pair — a muted key with the
            // equivalent name; a wrapped run of pipe-joined text
            // buried the mapping in prose.
            let mut list = Flex::column().gap(2.0);
            for (fw, a) in meta.aliases {
                list = list.child(
                    Text::new(format!("{fw} — {a}"))
                        .font_size(11.0)
                        .color(secondary),
                );
            }
            rows.push(Box::new(list));
        }

        rows.push(Box::new(labeled_rule("Snippet")));
        // Snippet sits on the raised surface so it reads as a code
        // block, not stray prose — the tinted card carries the mono
        // lines at tight leading.
        let mut code = Flex::column().gap(1.0);
        for line in snippet.lines() {
            code = code.child(
                Text::new(line.to_string())
                    .font_size(11.5)
                    .family("monospace")
                    .color(secondary),
            );
        }
        let surface = tokens::default_dark()
            .color(TokenKey::RaisedColor)
            .unwrap_or_else(|| martensite_theme::Oklab::from_srgb(0.16, 0.17, 0.2));
        rows.push(Box::new(
            Container::new()
                .padding_uniform(8.0)
                .background(surface)
                .child(code),
        ));

        if let Some(col) = self
            .info_scroll_mut()
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
            PropSpec::Header { .. } | PropSpec::Probe { .. } => None,
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
                    self.stage_mut().set_frame(f);
                }
            }
        }
        let sig_zoom = self.sig_zoom.get();
        if sig_zoom != self.last_sig_zoom {
            self.last_sig_zoom = sig_zoom;
            self.stage_mut().set_zoom(sig_zoom as f32);
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
        if let Some(i) = self.rail_list_mut().take_activated() {
            if let Some(Some(page)) = self.rail_map.get(i).copied() {
                self.select(page);
            }
        }
        if let Some(row) = self.rail_list_mut().selected() {
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
                self.stage_mut().set_frame(f);
            }
        }
        if self.tool_mut::<Button>(TOOL_ZOOM_OUT).take_activated() {
            let z = self.stage_mut().zoom() * 0.8;
            self.stage_mut().set_zoom(z);
        }
        if self.tool_mut::<Button>(TOOL_ZOOM_IN).take_activated() {
            let z = self.stage_mut().zoom() * 1.25;
            self.stage_mut().set_zoom(z);
        }
        if self.tool_mut::<Button>(TOOL_RESET).take_activated() {
            self.stage_mut().reset_view();
        }

        // ---- prop controls → prop values ----
        let specs = self.pages[self.sel].props().to_vec();
        let mut edits = Vec::new();
        if let Some(host) = self.props_host_mut() {
            // Row indices ≠ spec indices: Header/Probe specs produce no
            // row, so track the row counter separately.
            let mut row_i = 0;
            for spec in specs.iter() {
                if matches!(spec, PropSpec::Header { .. } | PropSpec::Probe { .. }) {
                    continue;
                }
                if let Some(row) = host.child_mut(row_i) {
                    if let Some(v) = Self::read_control(row, spec) {
                        edits.push((spec.key(), v));
                    }
                }
                row_i += 1;
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
            let w = self.pages[self.sel].build(&props);
            self.stage_mut().set_child(w);
            self.rebuild_info();
        }

        // ---- staged subtree event tap → log ----
        // The stage records every event that reaches the widget's
        // subtree; this keeps the panel about the displayed widget,
        // not app traffic.
        for line in self.stage_mut().take_event_log() {
            self.log_line(line);
        }

        // ---- staged widget events + state diffs → log ----
        let page = &self.pages[self.sel];
        let mut lines = Vec::new();
        let mut state = Vec::new();
        // Field-level borrow keeps `page` alive — `stage_mut` would
        // take `&mut self` over the whole view.
        if let Some(staged) = self
            .canvas_card
            .content_mut()
            .as_any_mut()
            .and_then(|a| a.downcast_mut::<StageHost>())
            .and_then(|s| s.staged_mut())
        {
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
        // The log belongs to the displayed widget — a different page
        // means a different widget, so stale lines must not bleed over.
        self.log.clear();
        self.log_mut().set_items(Vec::<String>::new());
        self.sig_sel.set(page);
        self.last_sig[0] = page;
        let props = self.prop_values[&page].clone();
        let w = self.pages[page].build(&props);
        self.stage_mut().set_child(w);
        let meta = self.pages[page].meta();
        self.canvas_card
            .set_title(&format!("{} / {}", meta.family, meta.name));
        self.rebuild_props_panel();
        self.rebuild_info();
        if let Some(row) = self.rail_map.iter().position(|m| *m == Some(page)) {
            self.rail_list_mut().set_selected(row);
            self.rail_list_mut().scroll_row_into_view(row);
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
        let parsed = spec.parse_value(value);
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
        self.stage_mut().set_theme(match theme {
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
        self.stage_mut()
            .set_direction(rtl.then_some(martensite::core::LayoutDirection::Rtl));
        self.sig_rtl.set(rtl);
        self.last_sig[3] = usize::from(rtl);
        self.tool_mut::<Segmented>(TOOL_DIR)
            .set_selected(usize::from(rtl));
    }

    /// Sets the stage locale override.
    fn set_locale(&mut self, idx: Option<usize>) {
        self.locale_idx = idx;
        self.stage_mut()
            .set_locale(idx.map(|i| martensite::core::Locale::new(LOCALE_TAGS[i])));
        self.sig_locale.set(idx.unwrap_or(usize::MAX));
        self.last_sig[2] = self.sig_locale.get();
        let sel = idx.map_or(0, |i| i + 1);
        self.tool_mut::<Dropdown>(TOOL_LOCALE).commit(sel);
    }

    /// Card rects for the five panels — shared by `layout` so content
    /// and [`PanelCard`] chrome can never disagree. Two geometries:
    /// the wide layout docks reference+events under the canvas; under
    /// [`COMPACT_W`] the dock spans the window's full width below all
    /// three columns so no panel degenerates into a sliver with its
    /// hairline edges overlapping its neighbours'.
    fn panel_rects(b: Rect, scale: f32) -> (Rect, Rect, Rect, Rect, Rect) {
        let pad = PAD * scale;
        let th = TOOLBAR_H * scale;
        let body_y = b.min_y() + th + pad;
        let body_h = (b.height() - th - pad - pad).max(0.0);
        let compact = b.width() < COMPACT_W * scale;
        let rail_w = if compact { RAIL_W_COMPACT } else { RAIL_W } * scale;
        let props_w = if compact { PROPS_W_COMPACT } else { PROPS_W } * scale;
        if compact {
            let dock_h = (BOTTOM_H * scale).min(body_h * 0.35);
            let top_h = (body_h - dock_h - pad).max(0.0);
            let rail = Rect::new(b.min_x() + pad, body_y, rail_w, top_h);
            let props = Rect::new(b.max_x() - pad - props_w, body_y, props_w, top_h);
            let cx0 = rail.max_x() + pad;
            let cw = (props.min_x() - pad - cx0).max(0.0);
            let canvas = Rect::new(cx0, body_y, cw, top_h);
            let dock_y = body_y + top_h + pad;
            let half = (b.width() - pad * 3.0) * 0.5;
            let info = Rect::new(b.min_x() + pad, dock_y, half, dock_h);
            let log = Rect::new(info.max_x() + pad, dock_y, half, dock_h);
            return (rail, canvas, props, info, log);
        }
        let rail = Rect::new(b.min_x() + pad, body_y, rail_w, body_h);
        let props = Rect::new(b.max_x() - pad - props_w, body_y, props_w, body_h);
        let cx0 = rail.max_x() + pad;
        let cw = (props.min_x() - pad - cx0).max(0.0);
        let canvas = Rect::new(cx0, body_y, cw, (body_h - BOTTOM_H * scale - pad).max(0.0));
        let dock_y = body_y + canvas.height() + pad;
        let info = Rect::new(cx0, dock_y, (cw - pad) * 0.5, BOTTOM_H * scale);
        let log = Rect::new(
            info.max_x() + pad,
            dock_y,
            (cw - pad) * 0.5,
            BOTTOM_H * scale,
        );
        (rail, canvas, props, info, log)
    }
}

/// Muted label color for catalog chrome (small-caps headers, brand).
/// The catalog shell is dark-first; panel content still follows the
/// ambient theme.
fn muted_text() -> martensite_theme::Oklab {
    tokens::default_dark()
        .color(TokenKey::TextMutedColor)
        .unwrap_or_else(|| martensite_theme::Oklab::from_srgb(0.55, 0.57, 0.62))
}

/// Secondary body-text color — one step off full ink for paragraphs
/// that should read softer than labels.
fn secondary_text() -> martensite_theme::Oklab {
    tokens::default_dark()
        .color(TokenKey::TextColor)
        .unwrap_or_else(|| martensite_theme::Oklab::from_srgb(0.82, 0.83, 0.86))
}

/// A labeled divider — small-caps muted label with a trailing
/// hairline. One construction shared by props-panel section headers,
/// card title strips, and the reference pane's section breaks so the
/// language stays identical everywhere it appears.
fn labeled_rule(label: &str) -> Flex {
    Flex::row()
        .gap(8.0)
        .cross_axis_alignment(CrossAxisAlignment::Center)
        .child(
            Text::new(label.to_uppercase())
                .font_size(10.0)
                .letter_spacing(0.08)
                .color(muted_text()),
        )
        .child_flex(Separator::horizontal(), 1.0)
}

/// A catalog panel — squircle `SurfaceColor` card with a 1pt
/// `BorderColor` hairline, an optional header strip (small-caps muted
/// title with a trailing `Separator` rule — the same labeled-rule
/// language as the props panel's section dividers), and padded content.
/// Children: `[header]` when titled, then `content`.
struct PanelCard {
    header: Option<Flex>,
    content: Box<dyn Widget>,
    header_b: Rect,
    content_b: Rect,
}

impl PanelCard {
    fn untitled(content: impl Widget + 'static) -> Self {
        Self {
            header: None,
            content: Box::new(content),
            header_b: Rect::default(),
            content_b: Rect::default(),
        }
    }

    fn titled(title: &str, content: impl Widget + 'static) -> Self {
        let header = labeled_rule(title);
        Self {
            header: Some(header),
            content: Box::new(content),
            header_b: Rect::default(),
            content_b: Rect::default(),
        }
    }

    /// Retitles the header strip (canvas breadcrumb on page switch).
    fn set_title(&mut self, title: &str) {
        if let Some(text) = self
            .header
            .as_mut()
            .and_then(|h| h.child_mut(0))
            .and_then(|c| c.as_any_mut())
            .and_then(|a| a.downcast_mut::<Text>())
        {
            text.set_content(title.to_uppercase());
        }
    }

    /// The panel's content widget.
    fn content_mut(&mut self) -> &mut dyn Widget {
        self.content.as_mut()
    }
}

impl Widget for PanelCard {
    fn measure(&mut self, cx: &mut LayoutContext, c: LayoutConstraints) -> Vec2 {
        let chrome = cx.pt(CARD_PAD * 2.0 + if self.header.is_some() { HEAD_H } else { 0.0 });
        let inner = LayoutConstraints {
            min_size: Vec2::ZERO,
            max_size: Vec2::new(
                (c.max_size.x - cx.pt(CARD_PAD * 2.0)).max(0.0),
                (c.max_size.y - chrome).max(0.0),
            ),
        };
        if let Some(h) = self.header.as_mut() {
            h.measure(cx, inner);
        }
        let cs = self.content.measure(cx, inner);
        Vec2::new(
            (cs.x + cx.pt(CARD_PAD * 2.0)).min(c.max_size.x),
            (cs.y + chrome).min(c.max_size.y),
        )
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        let pad = cx.pt(CARD_PAD);
        let head = cx.pt(HEAD_H);
        let titled = self.header.is_some();
        let content_y = bounds.min_y() + if titled { head } else { pad };
        self.content_b = Rect::new(
            bounds.min_x() + pad,
            content_y,
            (bounds.width() - pad * 2.0).max(0.0),
            (bounds.max_y() - pad - content_y).max(0.0),
        );
        if let Some(h) = self.header.as_mut() {
            self.header_b = Rect::new(
                bounds.min_x() + pad,
                bounds.min_y() + pad * 0.5,
                (bounds.width() - pad * 2.0).max(0.0),
                head - pad * 0.5,
            );
            cx.layout_child(h, self.header_b);
        }
        cx.layout_child(self.content.as_mut(), self.content_b);
    }

    fn paint(&self, cx: &mut PaintContext) {
        let surface = cx.color(TokenKey::SurfaceColor, [34, 37, 45, 255]);
        let line = cx.color(TokenKey::BorderColor, [64, 68, 78, 255]);
        let b = kurbo::Rect::new(
            f64::from(cx.bounds.min_x()),
            f64::from(cx.bounds.min_y()),
            f64::from(cx.bounds.max_x()),
            f64::from(cx.bounds.max_y()),
        );
        let shape = Shape::squircle(cx.pt(CARD_R));
        cx.list.push_fill_shape(b, &shape, surface);
        cx.list.push_stroke_shape(b, &shape, cx.pt(1.0), line);
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        self.forward_event_to_children(cx)
    }

    fn child_count(&self) -> usize {
        if self.header.is_some() {
            2
        } else {
            1
        }
    }

    fn child(&self, i: usize) -> Option<&dyn Widget> {
        match (i, self.header.as_ref()) {
            (0, Some(h)) => Some(h),
            (1, Some(_)) => Some(self.content.as_ref()),
            (0, None) => Some(self.content.as_ref()),
            _ => None,
        }
    }

    fn child_mut(&mut self, i: usize) -> Option<&mut dyn Widget> {
        match (i, self.header.as_mut()) {
            (0, Some(h)) => Some(h),
            (1, Some(_)) => Some(self.content.as_mut()),
            (0, None) => Some(self.content.as_mut()),
            _ => None,
        }
    }

    fn child_bounds(&self, i: usize) -> Option<Rect> {
        match (i, self.header.is_some()) {
            (0, true) => Some(self.header_b),
            (1, true) | (0, false) => Some(self.content_b),
            _ => None,
        }
    }

    fn debug_name(&self) -> &'static str {
        "PanelCard"
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

    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
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
        let th = cx.pt(TOOLBAR_H);
        let pad = cx.pt(PAD);
        let item_h = cx.pt(TOOLBAR_ITEM_H);
        // Toolbar strip: brand block left, tools cluster right-aligned
        // (the cluster packs its own controls at measured widths).
        // All chrome dimensions are logical pt — `cx.pt` carries them
        // to device px so HiDPI scale doesn't shrink the bar.
        let tb = Rect::new(
            bounds.min_x() + pad,
            bounds.min_y() + (th - item_h) * 0.5,
            (bounds.width() - pad * 2.0).max(0.0),
            item_h,
        );
        self.last_child_bounds[0] = tb;
        self.toolbar_row.measure(
            cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(tb.width(), tb.height()),
            },
        );
        cx.layout_child(&mut self.toolbar_row, tb);

        // Compact breakpoint: the brand subtitle yields to the tools
        // cluster rather than clipping mid-word.
        let compact = bounds.width() < COMPACT_W * cx.scale;
        if compact != self.compact {
            self.compact = compact;
            if let Some(sub) = self
                .toolbar_row
                .child_mut(0)
                .and_then(|c| c.as_any_mut())
                .and_then(|a| a.downcast_mut::<Flex>())
                .and_then(|brand| brand.child_mut(1))
                .and_then(|c| c.as_any_mut())
                .and_then(|a| a.downcast_mut::<Text>())
            {
                sub.set_content(if compact { "" } else { "Widget Catalog" });
            }
        }

        // Panel cards below the toolbar.
        let (rail, canvas, props, info, log) = Self::panel_rects(bounds, cx.scale);
        for (i, r) in [rail, canvas, props, info, log].into_iter().enumerate() {
            let i = i + 1;
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
        // Window canvas — the deepest step of the surface ladder; the
        // cards lift off it by one token.
        let b = cx.bounds;
        let full = kurbo::Rect::new(
            f64::from(b.min_x()),
            f64::from(b.min_y()),
            f64::from(b.max_x()),
            f64::from(b.max_y()),
        );
        let bg = cx.color(TokenKey::BackgroundColor, [21, 23, 28, 255]);
        cx.list.push_fill_rect(full, bg);

        // Toolbar — raised strip with a divider hairline at its edge.
        let raised = cx.color(TokenKey::RaisedColor, [30, 33, 40, 255]);
        let line = cx.color(TokenKey::DividerColor, [52, 55, 64, 255]);
        let th = cx.pt(TOOLBAR_H);
        let strip = kurbo::Rect::new(
            f64::from(b.min_x()),
            f64::from(b.min_y()),
            f64::from(b.max_x()),
            f64::from(b.min_y() + th),
        );
        cx.list.push_fill_rect(strip, raised);
        let hair = kurbo::Rect::new(
            f64::from(b.min_x()),
            f64::from(b.min_y() + th - cx.pt(1.0)),
            f64::from(b.max_x()),
            f64::from(b.min_y() + th),
        );
        cx.list.push_fill_rect(hair, line);
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        // Nav keys default to the widget rail. The unfocused key path
        // is a topmost-first scan, so without this an ArrowDown feeds
        // whichever list paints last — the event log, not the rail.
        // Editing controls and the staged widget keep their keys:
        // when one of those subtrees holds focus the normal focused
        // path delivers, untouched.
        if let WidgetEvent::KeyPressed { key, .. } = cx.event {
            const NAV_KEYS: &[&str] = &[
                "ArrowUp",
                "ArrowDown",
                "ArrowLeft",
                "ArrowRight",
                "PageUp",
                "PageDown",
                "Home",
                "End",
                "Enter",
            ];
            // Any held focus owns its keys — search caret moves, prop
            // controls adjust, a clicked-into event log scrolls, the
            // staged widget navigates. The intercept exists only for
            // the no-focus path, where the topmost-first scan would
            // feed nav keys to whichever list paints last.
            if NAV_KEYS.contains(&key.as_str()) && !self.has_focused_descendant() {
                let bounds = self
                    .rail_card
                    .child(0)
                    .and_then(|col| col.child_bounds(1))
                    .unwrap_or_default();
                let mut key_cx = EventContext {
                    event: cx.event,
                    bounds,
                    scale: cx.scale,
                };
                return self.rail_list_mut().event(&mut key_cx);
            }
        }
        self.forward_event_to_children(cx)
    }

    fn child_count(&self) -> usize {
        N_CHILDREN
    }

    fn child(&self, i: usize) -> Option<&dyn Widget> {
        Some(match i {
            0 => &self.toolbar_row,
            1 => &self.rail_card,
            2 => &self.canvas_card,
            3 => &self.props_card,
            4 => &self.info_card,
            _ => &self.log_card,
        })
    }

    fn child_mut(&mut self, i: usize) -> Option<&mut dyn Widget> {
        Some(match i {
            0 => &mut self.toolbar_row,
            1 => &mut self.rail_card,
            2 => &mut self.canvas_card,
            3 => &mut self.props_card,
            4 => &mut self.info_card,
            _ => &mut self.log_card,
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
