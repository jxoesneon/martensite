//! Base widgets: `Container`, `Flex`, `Stack`, `Text`.
//!
//! These are the foundational building blocks for Martensite UIs.
//! Each widget implements the [`Widget`](martensite_core::widget::Widget)
//! trait and integrates with the arena, layout, text, and rendering
//! pipelines.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::{Button, Container, Text};
//!
//! let btn = Button::new("Click me");
//! assert_eq!(btn.label, "Click me");
//! ```
//!
//! ## Layout Architecture
//!
//! Martensite uses a **two-level layout architecture**:
//!
//! 1. **Arena-level layout** (`LayoutEngine`): The `LayoutEngine` uses
//!    Taffy to compute bounds for top-level widgets registered in the
//!    `WidgetArena`. Each widget is a Taffy leaf node whose intrinsic
//!    size is determined by `Widget::measure`.
//!
//! 2. **Widget-internal layout** (`Widget::layout`): Each widget
//!    manages its own children internally. `Container` applies padding
//!    and positions its single child. `Flex` arranges its children in
//!    a row or column with alignment and gap. `Stack` layers children.
//!    These children are not registered in the arena — they are owned
//!    by the parent widget as `Box<dyn Widget>`.
//!
//! This means `Flex`/`Container`/`Stack` are treated as **leaf nodes**
//! by the `LayoutEngine` (they have no arena children). Their internal
//! children are laid out during `Widget::layout`, not by Taffy. This
//! architecture gives widgets full control over their internal layout
//! while Taffy handles the top-level arena tree.
//!
//! Internal children are not invisible, though: the framework reaches
//! them through the `Widget::child_count`/`child`/`child_mut`/
//! `child_bounds` protocol — `Widget::event` forwards events into them
//! (bounds-gated, topmost-first), `WidgetArena::build_paint_list`
//! recurses into them after the parent's chrome, and the AccessKit
//! adapter emits them as virtual nodes in the accessibility tree.

/// "About this app" panel — GTK `AboutDialog` / `NSAboutPanel`
/// content: logo, name, version, description, website link, copyright,
/// and titled credits sections. Mount inside a `Dialog` for chrome.
///
/// # Examples
///
/// ```
/// use martensite::widgets::about::About;
///
/// let a = About::new("App").version("1.0").credits("By", ["A"]);
/// assert_eq!(a.app_name, "App");
/// ```
pub mod about;

/// Vertically stacked collapsible sections (Ant Collapse / MUI
/// Accordion) with managed expansion.
///
/// # Examples
///
/// ```
/// use martensite::widgets::accordion::Accordion;
///
/// let a = Accordion::new();
/// assert!(a.sections.is_empty());
/// ```
pub mod accordion;

/// Bottom action sheet — tappable action rows with destructive/cancel
/// semantics (iOS action sheet, Ant `ActionSheet`).
///
/// # Examples
///
/// ```
/// use martensite::widgets::ActionSheet;
///
/// let s = ActionSheet::new().action("Save").destructive("Delete");
/// ```
pub mod action_sheet;

/// Concentric progress rings (Apple Watch Activity idiom) — up to
/// five named arcs sweeping from twelve o'clock around a shared
/// center, with dimmed track circles underneath and the average
/// completion at the center.
///
/// # Examples
///
/// ```
/// use martensite::widgets::activity_ring::ActivityRing;
///
/// let a = ActivityRing::new().ring("Move", 0.8, [255, 60, 80, 255]);
/// assert_eq!(a.ring_count(), 1);
/// ```
pub mod activity_ring;

/// Modal severity-tinted alert card for the overlay layer (NSAlert /
/// `AlertDialog`) — title, message, footer buttons, `AlertResult` seam.
///
/// # Examples
///
/// ```
/// use martensite::widgets::alert_dialog::{AlertDialog, AlertRole};
///
/// let d = AlertDialog::new()
///     .title("Delete?")
///     .button("OK", AlertRole::Confirm);
/// assert_eq!(d.buttons.len(), 1);
/// ```
pub mod alert_dialog;

/// Scroll-spy navigation rail — a vertical link list where the active
/// section follows scroll position (Ant `Anchor`). Clicks park the
/// target in `take_clicked`; the app reports position via
/// `set_active`.
///
/// # Examples
///
/// ```
/// use martensite::widgets::anchor::{Anchor, AnchorItem};
///
/// let mut a = Anchor::new().items([AnchorItem::new("Top", "top")]);
/// a.set_active(0);
/// assert_eq!(a.active(), Some(0));
/// ```
pub mod anchor;

/// Clock-face display — hour/minute/second hands over a ticked dial
/// (Qt `QAnalogClock`). Display-only and driven: the app sets the
/// time from a timer tick; the widget never reads the wall clock.
///
/// # Examples
///
/// ```
/// use martensite::widgets::analog_clock::AnalogClock;
///
/// let clock = AnalogClock::new().time(10, 9, 30);
/// assert_eq!(clock.time_value(), (10, 9, 30));
/// ```
pub mod analog_clock;

/// Editable text input with a filtered suggestion popup (Qt
/// `QCompleter`, WinUI `AutoSuggestBox`, Ant `AutoComplete`) —
/// `Role::ComboBox` + overlay `ListBox` wiring.
///
/// # Examples
///
/// ```
/// use martensite::widgets::auto_complete::AutoComplete;
///
/// let ac = AutoComplete::new().suggestions(["Apple", "Banana"]);
/// assert_eq!(ac.suggestion_count(), 2);
/// ```
pub mod auto_complete;

/// Aspect-ratio-locked container (GTK `AspectFrame`) — the child
/// gets the largest rect preserving `ratio`, centered by xalign/
/// yalign. For video surfaces and previews that must not stretch.
///
/// # Examples
///
/// ```
/// use martensite::widgets::aspect_frame::AspectFrame;
///
/// assert_eq!(AspectFrame::new(16.0 / 9.0).ratio, 16.0 / 9.0);
/// ```
pub mod aspect_frame;

/// Circular user avatar — image content clipped to the silhouette, or
/// initials on an accent disc.
///
/// # Examples
///
/// ```
/// use martensite::widgets::avatar::Avatar;
///
/// let a = Avatar::new("Ada Lovelace");
/// assert_eq!(a.name, "Ada Lovelace");
/// ```
pub mod avatar;

/// Overlapping avatar stack — members paint left-to-right on top of
/// each other; beyond `max_count` the extras collapse into a trailing
/// `+N` chip (Ant `Avatar.Group`).
///
/// # Examples
///
/// ```
/// use martensite::widgets::{Avatar, AvatarGroup};
///
/// let g = AvatarGroup::new()
///     .member(Avatar::new("Ada Lovelace"))
///     .member(Avatar::new("Grace Hopper"))
///     .max_count(1);
/// assert_eq!(g.overflow_count(), 1);
/// ```
pub mod avatar_group;

/// Notification badge — count pill, capped `99+`, or bare dot,
/// standalone or anchored to a wrapped child's top-right corner.
///
/// # Examples
///
/// ```
/// use martensite::widgets::badge::Badge;
///
/// let b = Badge::new(5);
/// assert_eq!(b.text(), "5");
/// ```
pub mod badge;

/// Interactive button widget.
///
/// # Examples
///
/// ```
/// use martensite::widgets::button::Button;
///
/// let btn = Button::new("Click me");
/// assert_eq!(btn.label, "Click me");
/// ```
pub mod button;

/// Always-visible month-grid date selector (Ant `Calendar` panel /
/// `QCalendarWidget` / GTK `Calendar`). `DatePicker` hides its grid
/// behind a popup; `Calendar` renders the month inline with chevron
/// navigation, a weekday header, today/selection/range states, and
/// arrow-key focus.
///
/// # Examples
///
/// ```
/// use martensite::widgets::calendar::Calendar;
/// use martensite::widgets::date_picker::Date;
///
/// let cal = Calendar::new().date(Date { year: 2024, month: 6, day: 15 });
/// assert_eq!(cal.displayed_month(), (2024, 6));
/// ```
pub mod calendar;

/// OHLC financial chart — high–low wicks plus open–close bodies in
/// success/error tones (Qt `QCandlestickSeries`, trading-view
/// candles). Y range auto-fits or pins; hover parks the candle index.
///
/// # Examples
///
/// ```
/// use martensite::widgets::candlestick::{Candle, Candlestick};
///
/// let c = Candlestick::new().candle(Candle::new(10.0, 12.0, 9.0, 11.0));
/// assert_eq!(c.candle_count(), 1);
/// ```
pub mod candlestick;

/// Elevated content surface with title and action row (M3 / Ant
/// Card).
///
/// # Examples
///
/// ```
/// use martensite::widgets::card::Card;
///
/// let c = Card::new();
/// assert!(c.title.is_none());
/// ```
pub mod card;

/// Paged content rotator — one child page visible at a time with dot
/// indicators, side arrow zones, arrow/PageUp/PageDown/Home/End keys,
/// and horizontal `Scroll` paging (Ant `Carousel`). Hidden pages
/// report `None` bounds and drop out of traversal.
///
/// # Examples
///
/// ```
/// use martensite::prelude::*;
/// use martensite::widgets::carousel::Carousel;
///
/// let mut c = Carousel::new().page(Text::new("a")).page(Text::new("b"));
/// c.go_to(1);
/// assert_eq!(c.current(), 1);
/// ```
pub mod carousel;

/// Cascading column picker — drill-down option lists whose leaf
/// click commits the full value path (macOS `NSBrowser`, Ant
/// `Cascader`).
///
/// # Examples
///
/// ```
/// use martensite::widgets::{Cascader, CascaderOption};
///
/// let c = Cascader::new().options([CascaderOption::new("a", "a")]);
/// assert_eq!(c.option_count(), 1);
/// ```
pub mod cascader;

/// Toggleable checkbox widget.
///
/// # Examples
///
/// ```
/// use martensite::widgets::checkbox::CheckBox;
///
/// let cb = CheckBox::new("Check");
/// assert!(!cb.checked);
/// ```
pub mod checkbox;

/// Compact pill chip for selection, input, or actions (M3 chips /
/// Ant Tag).
///
/// # Examples
///
/// ```
/// use martensite::widgets::chip::Chip;
///
/// let c = Chip::new("Tag");
/// assert!(!c.selected);
/// ```
pub mod chip;

/// Wrapping set of selectable chips (Material 3 filter-chip set /
/// Ant `Tag.CheckableTag` group) — `FlowBox`-style wrap layout plus
/// a `ChipSelection` mode (None/Single/Multiple) enforced on the
/// children's own toggle seam.
///
/// # Examples
///
/// ```
/// use martensite::widgets::chip_group::{ChipGroup, ChipSelection};
/// use martensite::widgets::Chip;
///
/// let g = ChipGroup::new()
///     .chip(Chip::new("a"))
///     .selection(ChipSelection::Single);
/// assert_eq!(g.chip_count(), 1);
/// ```
pub mod chip_group;

/// Maximum-width centering container (libadwaita `AdwClamp`) — the
/// child fills height but its width is capped at `maximum`,
/// centered horizontally. The readability container for long text
/// and forms on wide windows.
///
/// # Examples
///
/// ```
/// use martensite::widgets::clamp::Clamp;
/// use martensite::widgets::Text;
/// use martensite::core::Widget;
///
/// let c = Clamp::new().maximum(600.0).child(Text::new("x"));
/// assert_eq!(c.child_count(), 1);
/// ```
pub mod clamp;

/// Read-only monospace code display with a line-number gutter
/// (editor / review-tool idiom) — the `current` line gets an
/// accent wash, clicking a line parks its index in
/// `take_selected`, and mouse-wheel scrolls with a `follow` flag
/// keeping `current` visible.
///
/// # Examples
///
/// ```
/// use martensite::widgets::code_view::CodeView;
///
/// let c = CodeView::new().lines(["fn main() {", "}"]);
/// assert_eq!(c.line_count(), 2);
/// ```
pub mod code_view;

/// Color-swatch button face that parks a picker request (GTK
/// `ColorButton`, `NSColorWell`) — checkerboard-backed swatch plus
/// optional title; activation drains via `take_activated` for the app
/// to mount a `ColorPicker`/`ColorPalette` popup.
///
/// # Examples
///
/// ```
/// use martensite::widgets::color_button::ColorButton;
///
/// let cb = ColorButton::new([220, 60, 60, 255]).title("Accent");
/// assert_eq!(cb.color()[0], 220);
/// ```
pub mod color_button;

/// Preset color-swatch grid — the curated palette row (Ant
/// `ColorPicker` presets / `NSColorList`) where click selects and
/// parks `(index, rgba)` in `take_selected`. Unlike `ColorPicker`'s
/// free-form spectrum, a palette is the fixed swatch list.
///
/// # Examples
///
/// ```
/// use martensite::widgets::color_palette::ColorPalette;
///
/// let p = ColorPalette::new().swatches([[255, 0, 0, 255]]);
/// assert_eq!(p.swatch_count(), 1);
/// ```
pub mod color_palette;

/// Color well with an HSV picker popup (WinUI `ColorPicker` /
/// `NSColorWell`) — SV square, hue/alpha strips, live `take_edited`
/// seam plus an OK-confirm `take_selected` seam.
///
/// # Examples
///
/// ```
/// use martensite::widgets::{Color, ColorPicker};
///
/// let c = ColorPicker::new().color(Color::rgb(60, 110, 220));
/// assert!(!c.is_open());
/// ```
pub mod color_picker;

/// Win32 command-link button — a full-width action row with a bold
/// label, smaller explanatory note, and trailing `›`. The
/// "descriptive action" between a button and a link.
///
/// # Examples
///
/// ```
/// use martensite::widgets::command_link::CommandLink;
///
/// let c = CommandLink::new("Create project").note("From a template");
/// assert_eq!(c.label, "Create project");
/// ```
pub mod command_link;

/// Fuzzy action launcher (KDE `KCommandBar`, cmdk `Command`, VS Code
/// `Ctrl+Shift+P`) — a search-field trigger whose scored result list
/// hangs in an overlay `ListBox`, mirroring `AutoComplete`'s
/// `Role::ComboBox` wiring.
///
/// # Examples
///
/// ```
/// use martensite::widgets::{CommandAction, CommandPalette};
///
/// let mut pal = CommandPalette::new().actions([CommandAction::new("a", "Alpha")]);
/// pal.open();
/// assert!(pal.is_open());
/// ```
pub mod command_palette;

/// Cardinal heading indicator (navigation / embedded-instrument
/// idiom) — `N E S W` letters and tick marks around a circular
/// dial with a two-tone needle at `heading` degrees.
/// Display-only companion to `AnalogClock`.
///
/// # Examples
///
/// ```
/// use martensite::widgets::compass::Compass;
///
/// let c = Compass::new().heading(135.0);
/// assert_eq!(c.cardinal(), "SE");
/// ```
pub mod compass;

/// Box container layout primitive.
///
/// # Examples
///
/// ```
/// use martensite::widgets::container::Container;
///
/// let c = Container::new();
/// ```
pub mod container;

/// Right-click context-menu trigger wrapping a content child.
///
/// # Examples
///
/// ```
/// use martensite::widgets::{ContextMenu, MenuItem, Text};
///
/// let m = ContextMenu::new(Text::new("area"), vec![MenuItem::action("Copy")]);
/// assert!(!m.is_open());
/// ```
pub mod context_menu;

/// Tick-driven countdown timer display (pomodoro / cycle-time
/// idiom) — `MM:SS` (or `H:MM:SS`) face decrementing each frame,
/// warning color under a threshold, alert color at zero, a
/// one-shot `take_elapsed` flag, and `Space` pause/resume.
///
/// # Examples
///
/// ```
/// use martensite::widgets::countdown::Countdown;
/// use std::time::Duration;
///
/// let c = Countdown::new(Duration::from_secs(90));
/// assert_eq!(c.remaining().as_secs(), 90);
/// ```
pub mod countdown;

/// Read-only date field with a calendar-grid popup (`QDateEdit` /
/// `GtkCalendar` / WinUI `CalendarDatePicker`) — min/max clamping,
/// host-injected "today", `take_selected` pick seam.
///
/// # Examples
///
/// ```
/// use martensite::widgets::{Date, DatePicker};
///
/// let d = DatePicker::new().date(Date { year: 2024, month: 6, day: 15 });
/// assert!(!d.is_open());
/// ```
pub mod date_picker;

/// Label:value detail grid for detail pages (Ant `Descriptions`).
///
/// # Examples
///
/// ```
/// use martensite::widgets::Descriptions;
///
/// let d = Descriptions::new().title("Device").item("Model", "MX");
/// assert_eq!(d.item_count(), 1);
/// ```
pub mod descriptions;

/// Rotary knob — circular value control with a 270° sweep
/// (Qt `QDial`, audio-plugin knob idiom).
///
/// # Examples
///
/// ```
/// use martensite::widgets::dial::Dial;
///
/// let d = Dial::new().range(0.0, 100.0).value(25.0);
/// assert_eq!(d.get_value(), 25.0);
/// ```
pub mod dial;

/// Flexbox row and column layout.
///
/// # Examples
///
/// ```
/// use martensite::widgets::flex::Flex;
///
/// let row = Flex::row();
/// ```
pub mod flex;

/// Floating action button — a circular overlay button for `Stack`
/// layering, with a `back_top` scroll-to-top idiom (M3 FAB, Ant
/// `FloatButton`).
///
/// # Examples
///
/// ```
/// use martensite::widgets::FloatButton;
///
/// assert!(!FloatButton::back_top().visible);
/// ```
pub mod float_button;

/// Wrap-flow cell container (GTK `FlowBox`) — children flow
/// left-to-right at natural size and wrap when out of width, each
/// row's height set by its tallest cell. Optional single-select
/// cell semantics park the index in `take_selected`.
///
/// # Examples
///
/// ```
/// use martensite::widgets::flow_box::FlowBox;
/// use martensite::widgets::Text;
/// use martensite::core::Widget;
///
/// let fb = FlowBox::new().child(Text::new("a")).child(Text::new("b"));
/// assert_eq!(fb.child_count(), 2);
/// ```
pub mod flow_box;

/// Font-swatch button face that parks a chooser request (GTK
/// `FontButton` / `NSFontPanel` well) — `"Family NN"` face plus a
/// disclosure chevron; activation drains via `take_activated`.
///
/// # Examples
///
/// ```
/// use martensite::widgets::font_button::FontButton;
///
/// let fb = FontButton::new("Inter", 13.0);
/// assert_eq!(fb.family(), "Inter");
/// ```
pub mod font_button;

/// Labeled control row with a validation strip — the Ant `Form.Item`
/// pattern: label (top or left column), one control child, and an
/// error/hint message line that takes space only when it has content.
///
/// # Examples
///
/// ```
/// use martensite::widgets::form_field::FormField;
/// use martensite::widgets::TextInput;
///
/// let f = FormField::new().label("Host").required(true).child(TextInput::new("x"));
/// assert!(f.error().is_none());
/// ```
pub mod form_field;

/// Conversion-funnel chart — a vertical stack of centered
/// trapezoids tapering with each stage's value (Ant `Funnel` /
/// pipeline-chart idiom). Stage names and values paint beside each
/// band; hovering a stage parks its index in `take_hovered`.
///
/// # Examples
///
/// ```
/// use martensite::widgets::funnel_chart::FunnelChart;
///
/// let f = FunnelChart::new().stage("Visits", 1000.0).stage("Paid", 90.0);
/// assert_eq!(f.stage_count(), 2);
/// ```
pub mod funnel_chart;

/// Horizontal task-bar timeline (MS Project / enterprise Gantt
/// idiom) — rounded bars positioned by start day and duration
/// across a day-scale axis, task names down a left column, day
/// ticks along the bottom, and a progress-shaded segment per bar.
///
/// # Examples
///
/// ```
/// use martensite::widgets::gantt::Gantt;
///
/// let g = Gantt::new().total_days(14.0).task("Build", 3.0, 5.0);
/// assert_eq!(g.task_count(), 1);
/// ```
pub mod gantt;

/// Radial gauge display — value arc with zone colors, ticks, and a
/// centered readout (WCT `RadialGauge`, SwiftUI `Gauge`).
///
/// # Examples
///
/// ```
/// use martensite::widgets::gauge::Gauge;
///
/// let g = Gauge::new().range(0.0, 100.0).value(62.0);
/// assert_eq!(g.get_value(), 62.0);
/// ```
pub mod gauge;

/// Gradient rail with draggable color stops (design-tool /
/// CSS gradient-editor idiom) — a horizontal color bar with a
/// diamond handle per `(position, color)` stop. Dragging a handle
/// moves its stop, clicking empty rail space inserts a sampled
/// stop, and a double-click removes one (two-stop minimum). Any
/// edit parks [`GradientEditor::take_changed`] and the selected
/// index in `take_selected`.
///
/// # Examples
///
/// ```
/// use martensite::widgets::gradient_editor::{GradientEditor, GradientStop};
///
/// let mut g = GradientEditor::new();
/// g.add_stop(GradientStop::new(0.5, [255, 0, 0, 255]));
/// assert_eq!(g.stop_count(), 3);
/// ```
pub mod gradient_editor;

/// Column-grid layout container — Ant `Row`/`Col` style: a fixed
/// column count with per-cell `col_span`/`row_span` and automatic
/// left-to-right placement that wraps to fresh rows.
///
/// # Examples
///
/// ```
/// use martensite::prelude::*;
/// use martensite::widgets::grid::{Grid, GridCell};
///
/// let g = Grid::new().columns(12)
///     .cell(GridCell::new(Text::new("half")).col_span(6))
///     .cell(GridCell::new(Text::new("half")).col_span(6));
/// assert_eq!(g.cell_count(), 2);
/// ```
pub mod grid;

/// Titled frame container with optional checkable title (QGroupBox /
/// GTK Frame).
///
/// # Examples
///
/// ```
/// use martensite::widgets::group_box::GroupBox;
///
/// let gb = GroupBox::new("Options");
/// assert_eq!(gb.title, "Options");
/// ```
pub mod group_box;

/// Window-top bar with a centered title, an optional subtitle, and
/// leading/trailing slots (GTK `HeaderBar`, WinUI `AppTitleBar`).
///
/// # Examples
///
/// ```
/// use martensite::widgets::header_bar::HeaderBar;
/// use martensite::widgets::Button;
///
/// let bar = HeaderBar::new("Document")
///     .leading(Button::new("Back"))
///     .trailing(Button::new("Save"));
/// assert_eq!(bar.title(), "Document");
/// ```
pub mod header_bar;

/// Intensity grid — GitHub contribution-graph / d3-heatmap cells
/// mapped through a sequential ramp, with cell hover hit-testing for
/// tooltips.
///
/// # Examples
///
/// ```
/// use martensite::widgets::heat_map::HeatMap;
///
/// let hm = HeatMap::new(7, 52).set(0, 0, 5.0);
/// assert_eq!(hm.get(0, 0), 5.0);
/// ```
pub mod heat_map;

/// Hex-dump display (binary-inspection / dev-tool idiom) — rows
/// of `offset hh hh … |ascii|`: muted offset column, 16 hex
/// pairs with an 8-byte gap, and a printable-ASCII gutter.
/// Clicking a byte cell parks its index in `take_selected`.
///
/// # Examples
///
/// ```
/// use martensite::widgets::hex_view::HexView;
///
/// let h = HexView::new().bytes(vec![0xde, 0xad, 0xbe, 0xef]);
/// assert_eq!(h.row_count(), 1);
/// ```
pub mod hex_view;

/// Binned frequency chart — contiguous equal-width bars over a
/// numeric range (the distribution/density idiom, complementing
/// `BarChart`'s categorical columns). Takes raw samples (auto-
/// binned over their span) or pre-binned counts; hovering a bin
/// parks its index in `take_hovered`.
///
/// # Examples
///
/// ```
/// use martensite::widgets::histogram::Histogram;
///
/// let h = Histogram::new().bins(4).samples([0.1, 0.4, 0.6, 0.9]);
/// assert_eq!(h.bin_count(), 4);
/// ```
pub mod histogram;

/// Raster image display with aspect-fit modes (`Contain`, `Cover`,
/// `Fill`, `None`).
///
/// # Examples
///
/// ```
/// use martensite::widgets::image::Image;
/// use martensite_core::ImageData;
///
/// let data = ImageData::from_rgba(1, 1, vec![0, 0, 0, 255]).unwrap();
/// let i = Image::new(data);
/// ```
pub mod image;

/// Pan/zoom image viewport — the photo-viewer idiom. `Scroll` zooms
/// around the pointer, primary-drag pans (clamped so a quarter of
/// the image stays reachable), double-click or `0` resets to fit,
/// and `+`/`-` zoom from the keyboard. A checkerboard underlay
/// reads alpha as transparency.
///
/// # Examples
///
/// ```
/// use martensite::widgets::image_viewer::ImageViewer;
/// use martensite_core::ImageData;
///
/// let data = ImageData::from_rgba(4, 4, vec![0; 4 * 4 * 4]).unwrap();
/// let viewer = ImageViewer::new(data);
/// assert_eq!(viewer.zoom(), 1.0);
/// ```
pub mod image_viewer;

/// Freehand stroke-capture surface — the signature-pad / sketch
/// idiom. Primary-drag collects canvas-clamped points into strokes;
/// each released stroke is parked in `take_stroke`, `Backspace`
/// undoes the last committed stroke, and `Escape` cancels the one
/// in flight.
///
/// # Examples
///
/// ```
/// use martensite::widgets::ink_canvas::InkCanvas;
///
/// let canvas = InkCanvas::new().pen(2.5).label("Sign here");
/// assert!(canvas.is_empty());
/// ```
pub mod ink_canvas;

/// Click-to-edit text — a label that swaps in a [`TextInput`] on
/// press (`Enter` commits, `Escape` reverts, blur commits).
///
/// # Examples
///
/// ```
/// use martensite::widgets::inline_edit::InlineEdit;
///
/// let mut edit = InlineEdit::new("Title").placeholder("Untitled");
/// edit.begin_edit();
/// assert!(edit.is_editing());
/// ```
pub mod inline_edit;

/// IPv4 dotted-quad entry (WinForms `IPAddressControl` idiom) —
/// four [`SpinBox`] octets side by side with painted dot
/// separators, reading out as `[u8; 4]` with an any-octet
/// `take_changed` seam.
///
/// # Examples
///
/// ```
/// use martensite::widgets::ip_input::IpInput;
///
/// let ip = IpInput::new().value([192, 168, 1, 1]);
/// assert_eq!(ip.text(), "192.168.1.1");
/// ```
pub mod ip_input;

/// Z-ordered layering stack.
///
/// # Examples
///
/// ```
/// use martensite::widgets::stack::Stack;
///
/// let stack = Stack::new();
/// ```
pub mod stack;

/// Severity-tinted inline banner strip.
///
/// # Examples
///
/// ```
/// use martensite::widgets::banner::{Banner, Severity};
///
/// let b = Banner::new(Severity::Info, "Heads up");
/// assert_eq!(b.message, "Heads up");
/// ```
pub mod banner;

/// Categorical column chart — labeled bars scaled to max with a
/// baseline axis, the display-only KPI companion to `Sparkline`'s
/// continuous trend line.
///
/// # Examples
///
/// ```
/// use martensite::widgets::bar_chart::BarChart;
///
/// let c = BarChart::new().bar("Q1", 12.0).bar("Q2", 30.0);
/// assert_eq!(c.bar_count(), 2);
/// ```
pub mod bar_chart;

/// Stephen Few bullet graph — a compact KPI strip reading
/// value-vs-target against qualitative range bands (poor / ok /
/// good). The measure paints as a solid bar, the comparative
/// target as a tick, and label + formatted value sit at the edges.
///
/// # Examples
///
/// ```
/// use martensite::widgets::bullet_chart::BulletChart;
///
/// let c = BulletChart::new().label("Revenue").value(75.0).target(90.0);
/// assert_eq!(c.measure_value(), 75.0);
/// ```
pub mod bullet_chart;

/// Edge-bottom sheet with snap-point detents and a drag handle
/// (Material bottom sheet).
///
/// # Examples
///
/// ```
/// use martensite::widgets::BottomSheet;
///
/// let s = BottomSheet::new().detents(&[0.35, 0.9]);
/// ```
pub mod bottom_sheet;

/// Five-number statistical summary chart (Tukey box-and-whisker /
/// Ant `Box` idiom) — a q1→q3 box with a median line and whisker
/// caps to min/max per series along a shared value axis, series
/// names underneath, and a hover seam parking the series index.
///
/// # Examples
///
/// ```
/// use martensite::widgets::box_plot::{BoxPlot, BoxSeries};
///
/// let p = BoxPlot::new().series(BoxSeries::new("A", 0.0, 1.0, 2.0, 3.0, 4.0));
/// assert_eq!(p.series_count(), 1);
/// ```
pub mod box_plot;

/// Breadcrumb path strip — navigable segments with ellipsis collapse
/// when the path overflows (WinUI `BreadcrumbBar`, `NSPathControl`).
///
/// # Examples
///
/// ```
/// use martensite::widgets::Breadcrumb;
///
/// let b = Breadcrumb::new().segments(["Home", "Docs", "API"]);
/// ```
pub mod breadcrumb;

/// Modal dialog card for the overlay layer.
///
/// # Examples
///
/// ```
/// use martensite::widgets::dialog::Dialog;
///
/// let d = Dialog::new("Title").buttons(&["OK"]);
/// assert_eq!(d.buttons.len(), 1);
/// ```
pub mod dialog;

/// Unified-diff display (code-review idiom) — `Added` rows get a
/// green wash, `Removed` a red wash, `Hunk` headers a muted tone,
/// each with the leading marker character. Clicking a row parks
/// its index in `take_selected`; mouse-wheel scrolls.
///
/// # Examples
///
/// ```
/// use martensite::widgets::diff_view::{DiffKind, DiffView};
///
/// let d = DiffView::new()
///     .line(DiffKind::Removed, "old")
///     .line(DiffKind::Added, "new");
/// assert_eq!(d.tally(), (1, 1, 0, 0));
/// ```
pub mod diff_view;

/// Tick-driven digital time display — monospace block digits
/// over a dark face, `hh:mm` (+ `:ss`), 12/24-hour modes, and an
/// optional blinking colon. Textual sibling of `AnalogClock`.
///
/// # Examples
///
/// ```
/// use martensite::widgets::digital_clock::DigitalClock;
/// use martensite::widgets::Time;
///
/// let c = DigitalClock::new().time(Time { hour: 9, minute: 30 });
/// assert_eq!(c.text(), "09:30");
/// ```
pub mod digital_clock;

/// Collapsible section with a disclosure chevron.
///
/// # Examples
///
/// ```
/// use martensite::widgets::disclosure::Disclosure;
///
/// let d = Disclosure::new("Section");
/// assert!(!d.open);
/// ```
pub mod disclosure;

/// Edge-docked drawer / side panel for the overlay layer.
///
/// # Examples
///
/// ```
/// use martensite::widgets::drawer::Drawer;
///
/// let d = Drawer::new("Panel");
/// assert_eq!(d.depth, 300.0);
/// ```
pub mod drawer;

/// Determinate and indeterminate progress indicators.
///
/// # Examples
///
/// ```
/// use martensite::widgets::progress::ProgressBar;
///
/// let p = ProgressBar::new();
/// assert_eq!(p.fraction(), None);
/// ```
pub mod progress;

/// Pull-to-refresh wrapper — downward drag reveals an indicator and
/// parks a refresh request past the threshold (`UIRefreshControl`,
/// `SwipeRefreshLayout`).
///
/// # Examples
///
/// ```
/// use martensite::widgets::{PullToRefresh, Text};
///
/// let p = PullToRefresh::new(Text::new("feed"));
/// assert!(!p.refreshing());
/// ```
pub mod pull_to_refresh;

/// Hairline separator between content regions.
///
/// # Examples
///
/// ```
/// use martensite::widgets::separator::Separator;
///
/// let _ = Separator::horizontal();
/// ```
pub mod separator;

/// Preferences-page row + titled group — icon/title/subtitle rows
/// with a trailing control (Libadwaita `ActionRow`/`PreferencesGroup`,
/// WCT `SettingsCard`).
///
/// # Examples
///
/// ```
/// use martensite::widgets::settings_row::{SettingsGroup, SettingsRow};
///
/// let g = SettingsGroup::new("General").row(SettingsRow::new("About"));
/// assert_eq!(g.row_count(), 1);
/// ```
pub mod settings_row;

/// Ascending-bars connectivity indicator (cellular / Wi-Fi status
/// icon idiom) — a `0..=4` level lights that many bars and an
/// `offline` flag swaps the lit color to the error token.
///
/// # Examples
///
/// ```
/// use martensite::widgets::signal_strength::SignalStrength;
///
/// let s = SignalStrength::new().level(3);
/// assert_eq!(s.level_value(), 3);
/// ```
pub mod signal_strength;

/// Charge-level battery indicator — the status-bar glyph idiom:
/// a rounded body with a terminal nub and a level-proportional
/// fill that shifts green to amber to red, plus a `charging` bolt
/// overlay. Display-only companion to `SignalStrength`.
///
/// # Examples
///
/// ```
/// use martensite::widgets::battery::Battery;
///
/// let b = Battery::new().level(0.65).charging(true);
/// assert!(b.is_charging());
/// ```
pub mod battery;

/// Bottom-of-window status strip (QStatusBar / WPF `StatusBar`) —
/// left zone widgets, a permanent/temporary message zone, and
/// right-docked permanent widgets.
///
/// # Examples
///
/// ```
/// use martensite::widgets::StatusBar;
///
/// let mut bar = StatusBar::new().message("Ready");
/// bar.temporary("Saving…");
/// assert_eq!(bar.current_message(), "Saving…");
/// ```
pub mod status_bar;

/// Severity status lamp — a filled status-colored dot with an
/// optional label and `pulse` halo (industrial HMI lamp, Ant
/// `Badge.Status`). Display-only; the app flips states through
/// `set_status`.
///
/// # Examples
///
/// ```
/// use martensite::widgets::status_dot::{Status, StatusDot};
///
/// let d = StatusDot::new("Pump A").status(Status::Ok);
/// assert_eq!(d.status_value(), Status::Ok);
/// ```
pub mod status_dot;

/// Split button — primary-action zone fused with a chevron that
/// parks a dropdown request (WinUI `SplitButton`).
///
/// # Examples
///
/// ```
/// use martensite::widgets::SplitButton;
///
/// let mut b = SplitButton::new("Save");
/// assert!(!b.take_dropped());
/// ```
pub mod split_button;

/// Split-flap departure-board display (Solari / flip-board
/// idiom) — `text` sets the target and each cell flips through
/// the character set on `tick` until it lands, rippling
/// left-to-right. Display-only.
///
/// # Examples
///
/// ```
/// use martensite::widgets::split_flap::SplitFlap;
///
/// let mut s = SplitFlap::new().cells(7).text("ON TIME");
/// assert_eq!(s.cell_count(), 7);
/// ```
pub mod split_flap;

/// Two-pane container with a draggable divider (Qt `QSplitter`,
/// `NSSplitViewController`, WinUI `SplitView`).
///
/// # Examples
///
/// ```
/// use martensite::widgets::split_view::SplitView;
///
/// let s = SplitView::horizontal().ratio(0.3);
/// assert_eq!(s.get_ratio(), 0.3);
/// ```
pub mod split_view;

/// Wizard progress indicator — numbered step nodes connected by
/// lines (Ant `Steps`, Carbon `ProgressIndicator`, `QWizard` header).
///
/// # Examples
///
/// ```
/// use martensite::widgets::steps::Steps;
///
/// let s = Steps::new().steps(["Account", "Profile", "Done"]).current(1);
/// assert_eq!(s.current_step(), 1);
/// ```
pub mod steps;

/// Scrolling time-series trace (oscilloscope / strip-recorder
/// idiom) — `push` appends to a bounded ring, the newest sample
/// anchors the right edge and older data scrolls left, with a
/// center grid line and pinned or auto-fit vertical range.
///
/// # Examples
///
/// ```
/// use martensite::widgets::strip_chart::StripChart;
///
/// let mut s = StripChart::new().capacity(60).range(-1.0, 1.0);
/// s.push(0.5);
/// assert_eq!(s.latest(), Some(0.5));
/// ```
pub mod strip_chart;

/// Flowing stacked-area chart (ThemeRiver / streamgraph idiom) —
/// equal-length layer series stacked symmetrically around a
/// drifting center baseline in a categorical palette. Hovering a
/// band parks its layer index in `take_hovered`.
///
/// # Examples
///
/// ```
/// use martensite::widgets::stream_graph::StreamGraph;
///
/// let s = StreamGraph::new()
///     .layer("cpu", [1.0, 3.0, 2.0])
///     .layer("mem", [0.5, 1.0, 0.8]);
/// assert_eq!(s.layer_count(), 2);
/// ```
pub mod stream_graph;

/// Nested radial hierarchy chart (d3 sunburst / multi-level pie
/// idiom) — root siblings partition the inner ring by weight and
/// each node's children subdivide its angular span on the next
/// ring outward. Hovering a sector parks its index in
/// `take_hovered` and draws its name at the center.
///
/// # Examples
///
/// ```
/// use martensite::widgets::sunburst::{Sunburst, SunburstNode};
///
/// let s = Sunburst::new().node(SunburstNode::new("docs", 3.0));
/// assert_eq!(s.node_count(), 1);
/// ```
pub mod sunburst;

/// Swipe-to-reveal row actions — horizontal drags expose
/// leading/trailing action strips (iOS `UISwipeActionsConfiguration`).
///
/// # Examples
///
/// ```
/// use martensite::widgets::{SwipeAction, SwipeActions, Text};
///
/// let s = SwipeActions::new(Text::new("row"))
///     .trailing([SwipeAction::new("Delete").destructive()]);
/// assert_eq!(s.trailing_count(), 1);
/// ```
pub mod swipe_actions;

/// Pill toggle switch.
///
/// # Examples
///
/// ```
/// use martensite::widgets::switch::Switch;
///
/// let s = Switch::new("Toggle");
/// assert!(!s.on);
/// ```
pub mod switch;

/// Toast notification stack for the overlay layer.
///
/// # Examples
///
/// ```
/// use martensite::widgets::toast::ToastHost;
///
/// let h = ToastHost::new();
/// assert!(h.is_empty());
/// ```
pub mod toast;

/// Shaped text display widget.
///
/// # Examples
///
/// ```
/// use martensite::widgets::text::Text;
///
/// let t = Text::new("Hello");
/// ```
pub mod text;

/// Multiline plain-text editor widget.
///
/// # Examples
///
/// ```
/// use martensite::widgets::text_area::TextArea;
///
/// let area = TextArea::new();
/// ```
pub mod text_area;

/// Editable text input widget.
///
/// # Examples
///
/// ```
/// use martensite::widgets::text_input::TextInput;
///
/// let input = TextInput::new("Label");
/// ```
pub mod text_input;

/// External GPU surface widget (`PaintCallback` primitive).
///
/// ```
/// use martensite::widgets::external::ExternalEngine;
/// use martensite_engine_bridge::BridgeHandle;
///
/// let handle = BridgeHandle::new();
/// let surface = handle.lock().register();
/// let widget = ExternalEngine::new(handle, surface);
/// ```
pub mod external;

/// Keyboard-shortcut recorder field (Qt `QKeySequenceEdit`, KDE
/// `KKeySequenceWidget`).
///
/// # Examples
///
/// ```
/// use martensite::widgets::key_capture::KeyCapture;
///
/// let k = KeyCapture::new().shortcut("Ctrl+S");
/// assert_eq!(k.get_shortcut(), "Ctrl+S");
/// ```
pub mod key_capture;

/// Keyboard-key cap chip — the `<kbd>` element: a small beveled box
/// around a key name for docs, shortcut hints, and menus. Pair with
/// `KeyCapture` (recording) and `KeyMap` (dispatch).
///
/// # Examples
///
/// ```
/// use martensite::widgets::kbd::Kbd;
///
/// assert_eq!(Kbd::new("⌘").text(), "⌘");
/// ```
pub mod kbd;

/// Grouped shortcut reference — GTK `ShortcutsWindow` content:
/// titled [`ShortcutGroup`]s of `label : keys` rows flowed across
/// columns. Display-only; mount inside a dialog for the chrome.
///
/// # Examples
///
/// ```
/// use martensite::widgets::keyboard_shortcuts::{
///     KeyboardShortcuts, ShortcutGroup,
/// };
///
/// let s = KeyboardShortcuts::new(vec![ShortcutGroup::new("General")]);
/// assert_eq!(s.columns, 2);
/// ```
pub mod keyboard_shortcuts;

/// Telephony 3×4 digit pad (POS / dialer idiom) — rounded cells
/// for `1`–`9`, `*`, `0`, `#` that park the pressed character in
/// `take_pressed`, with keyboard digits and `Backspace` (`'\x08'`)
/// routed through the same seam.
///
/// # Examples
///
/// ```
/// use martensite::widgets::keypad::Keypad;
///
/// assert_eq!(Keypad::new().key_count(), 12);
/// ```
pub mod keypad;

/// Seven-segment digit display (Qt `QLCDNumber`) — a fixed-cell
/// right-aligned readout with ghost segments on the glass.
/// Display-only.
///
/// # Examples
///
/// ```
/// use martensite::widgets::lcd_number::LcdNumber;
///
/// let lcd = LcdNumber::new().value(42.0).digits(5);
/// assert_eq!(lcd.value, 42.0);
/// ```
pub mod lcd_number;

/// Dot-matrix LED display (departure-board / marquee-sign idiom) —
/// a `cols × rows` grid of circular dots with `set`/`toggle`/
/// `fill_all`/`clear_all` control. Lit dots paint in the accent
/// color (or `on_color`), unlit as faint ghosts. Display-only.
///
/// # Examples
///
/// ```
/// use martensite::widgets::led_matrix::LedMatrix;
///
/// let mut m = LedMatrix::new(8, 8);
/// m.set(2, 3, true);
/// assert!(m.get(2, 3));
/// ```
pub mod led_matrix;

/// Level/capacity meter — battery, disk-usage, or signal-strength
/// indicator with zone colors (GTK `GtkLevelBar`, `NSLevelIndicator`).
///
/// # Examples
///
/// ```
/// use martensite::widgets::level_bar::LevelBar;
///
/// let b = LevelBar::new().value(0.75);
/// assert_eq!(b.get_value(), 0.75);
/// ```
pub mod level_bar;

/// Inline hyperlink label — underlined accent text that parks its
/// target in `take_activated` on click, `Enter`, or AT `Click` (Ant
/// `Typography.Link`, GTK `LinkButton`). The shell owns navigation.
///
/// # Examples
///
/// ```
/// use martensite::widgets::link::Link;
///
/// let l = Link::new("Docs").target("https://docs.rs");
/// assert_eq!(l.href(), Some("https://docs.rs"));
/// ```
pub mod link;

/// Multi-series XY line chart (Ant `Line`, Swift `LineMark`) — the
/// full-size companion to `Sparkline`: shared domain, baseline ticks,
/// pointer-proximity series highlight.
///
/// # Examples
///
/// ```
/// use martensite::widgets::line_chart::{LineChart, LineSeries};
///
/// let c = LineChart::new().series(LineSeries::new("S", [1.0, 2.0]));
/// assert_eq!(c.series.len(), 1);
/// ```
pub mod line_chart;

/// Virtualized selectable list of string rows (APG listbox).
///
/// # Examples
///
/// ```
/// use martensite::widgets::ListView;
///
/// let l = ListView::new().items(["A", "B", "C"]);
/// assert_eq!(l.item_count(), 3);
/// ```
pub mod list_view;

/// Scrolling monospace log display — append-only `LogLine` ring with
/// severity colors, bottom-follow, and wheel-scroll history (IDE
/// console / `journalctl -f` pattern).
///
/// # Examples
///
/// ```
/// use martensite::widgets::log_view::{LogSeverity, LogView};
///
/// let mut log = LogView::new();
/// log.push(LogSeverity::Info, "started");
/// assert_eq!(log.len(), 1);
/// ```
pub mod log_view;

/// Read-only Markdown rich-text renderer — a pragmatic subset
/// (headings, paragraphs, code blocks, quotes, lists, rules; inline
/// emphasis, code, links) flowed as styled runs (Ant `Typography`,
/// `QTextEdit` markdown mode).
///
/// # Examples
///
/// ```
/// use martensite::widgets::Markdown;
///
/// let m = Markdown::new("# Hi\n\nsome *text*");
/// assert_eq!(m.source(), "# Hi\n\nsome *text*");
/// ```
pub mod markdown;

/// Horizontally scrolling text ticker (LED marquee / news-crawl
/// idiom) — text slides left at a configurable device-px speed and
/// wraps back in from the right after a gap, all inside the widget
/// clip. `Space` toggles pause while focused.
///
/// # Examples
///
/// ```
/// use martensite::widgets::marquee::Marquee;
///
/// let m = Marquee::new("Breaking news").speed(60.0);
/// assert_eq!(m.text(), "Breaking news");
/// ```
pub mod marquee;

/// Masonry layout — children flow into the currently-shortest
/// column (Pinterest layout, CSS `masonry`).
///
/// # Examples
///
/// ```
/// use martensite::core::Widget;
/// use martensite::widgets::{Masonry, Text};
///
/// let m = Masonry::new().columns(3).child(Text::new("a"));
/// assert_eq!(m.child_count(), 1);
/// ```
pub mod masonry;

/// Hardware video presentation widget.
///
/// # Examples
///
/// ```
/// use martensite::widgets::media::MediaView;
///
/// let view = MediaView::new();
/// ```
pub mod media;

/// Transport strip for `MediaView` (WinUI `MediaTransportControls`)
/// — play/pause, elapsed/total labels, draggable seek bar, volume
/// slider + mute, fullscreen button. Driven: user intent drains via
/// `take_*` seams and the app reflects decoder state back.
///
/// # Examples
///
/// ```
/// use martensite::widgets::media_controls::MediaControls;
///
/// let mc = MediaControls::new().duration(120.0);
/// assert_eq!(mc.duration_value(), 120.0);
/// ```
pub mod media_controls;

/// Menu item model and the `Role::Menu` popup surface (items,
/// submenus, checkables, radios, separators, headings).
///
/// # Examples
///
/// ```
/// use martensite::widgets::{Menu, MenuItem};
///
/// let m = Menu::new([MenuItem::action("Open")]);
/// assert_eq!(m.item_count(), 1);
/// ```
pub mod menu;

/// Horizontal menu strip (QMenuBar/NSMenuBar) driving `Menu` popups.
///
/// # Examples
///
/// ```
/// use martensite::widgets::{MenuBar, MenuItem};
///
/// let bar = MenuBar::new().menu("File", vec![MenuItem::action("Open")]);
/// assert_eq!(bar.menu_count(), 1);
/// ```
pub mod menu_bar;

/// Button face that opens a `Menu` popup below it (GTK `MenuButton`,
/// WinUI `DropDownButton`) — shares `MenuBar`/`ContextMenu`'s
/// `MenuStack` overlay machinery and `MenuPath` activation seam.
///
/// # Examples
///
/// ```
/// use martensite::widgets::{MenuButton, MenuItem};
///
/// let mut b = MenuButton::new("Actions", vec![MenuItem::action("A")]);
/// b.open();
/// assert!(b.is_open());
/// ```
pub mod menu_button;

/// Text field with a trigger-character suggestion popup — the Ant
/// `Mentions` / Slack `@`-completion pattern. Shares `AutoComplete`'s
/// APG editable-combobox architecture: the popup is an overlay
/// `ListBox` of candidates filtered by the token under the caret.
///
/// # Examples
///
/// ```
/// use martensite::widgets::mention::Mention;
///
/// let m = Mention::new().suggestions(["alice", "bob"]);
/// ```
pub mod mention;

/// Document-overview strip (editor minimap idiom) — squashed
/// content bars down a narrow column plus a translucent viewport
/// rect at `scroll`; clicking or dragging parks the picked scroll
/// fraction in `take_scrolled`.
///
/// # Examples
///
/// ```
/// use martensite::widgets::minimap::Minimap;
///
/// let m = Minimap::new().lines([20u32, 8, 35]).viewport(0.4);
/// assert_eq!(m.line_count(), 3);
/// ```
pub mod minimap;

/// Vertical icon+label destination rail for app-level navigation
/// (Material 3 `NavigationRail`, WinUI `NavigationView` rail mode).
///
/// # Examples
///
/// ```
/// use martensite::widgets::nav_rail::NavRail;
///
/// let r = NavRail::new().destination("🏠", "Home").selected(0);
/// assert_eq!(r.destination_count(), 1);
/// ```
pub mod nav_rail;

/// Push/pop navigation container — titled header with a `‹ Back`
/// affordance over a page stack where only the topmost is visible
/// (SwiftUI `NavigationStack`). Push, pop, back-zone click, `Escape`,
/// and `Backspace` all park `(depth, title)` in `take_navigated`.
///
/// # Examples
///
/// ```
/// use martensite::prelude::*;
/// use martensite::widgets::nav_stack::NavStack;
///
/// let mut nav = NavStack::new(Text::new("root")).title("Home");
/// nav.push(Text::new("detail"), "Detail");
/// assert_eq!(nav.depth(), 2);
/// ```
pub mod nav_stack;

/// Mechanical-reel digit counter (trip-odometer / web hit-counter
/// idiom) — a row of digit windows, each reel rolling upward
/// through neighboring digits toward its target on `tick`, clipped
/// to the window like a real odometer drum.
///
/// # Examples
///
/// ```
/// use martensite::widgets::odometer::Odometer;
///
/// let o = Odometer::new().digits(5).value(42);
/// assert_eq!(o.reading(), 42);
/// ```
pub mod odometer;

/// Segmented one-time-code / PIN field (Ant `Input.OTP`).
///
/// # Examples
///
/// ```
/// use martensite::widgets::otp_input::OtpInput;
///
/// let o = OtpInput::new().length(6).masked(true);
/// ```
pub mod otp_input;

/// Page-top bar with a back chevron, title/subtitle, and a trailing
/// action slot (Ant `PageHeader`). Left-anchored content chrome —
/// unlike `HeaderBar`'s centered window-title idiom.
///
/// # Examples
///
/// ```
/// use martensite::widgets::page_header::PageHeader;
/// use martensite::widgets::Button;
///
/// let h = PageHeader::new("Orders").back(true).action(Button::new("New"));
/// assert_eq!(h.action_count(), 1);
/// ```
pub mod page_header;

/// Page switcher — prev/next arrows + a windowed page run with
/// ellipsis gaps (Ant `Pagination`, Carbon `Pagination`).
///
/// # Examples
///
/// ```
/// use martensite::widgets::Pagination;
///
/// let p = Pagination::new().total_pages(20).current(7);
/// ```
pub mod pagination;

/// Musical keyboard strip (DAW piano-roll / MIDI-input idiom) —
/// `octaves` white keys with overlay black keys; clicks and
/// drags (glissando) park the struck semitone index in
/// `take_struck`, and held keys highlight.
///
/// # Examples
///
/// ```
/// use martensite::widgets::piano_keys::PianoKeys;
///
/// let p = PianoKeys::new().octaves(2);
/// assert_eq!(p.note_count(), 24);
/// ```
pub mod piano_keys;

/// Proportional wedge chart — pie or donut ring (Ant `Pie`, Swift
/// `SectorMark`). Angle+radius hit-testing, categorical palette,
/// per-slice press selection.
///
/// # Examples
///
/// ```
/// use martensite::widgets::pie_chart::{PieChart, PieSlice};
///
/// let c = PieChart::new(vec![PieSlice::new(1.0, "A")]).donut();
/// assert!(c.is_donut());
/// ```
pub mod pie_chart;

/// Page-dot strip — WinUI `PipsPager` / iOS `UIPageControl`: `count`
/// dots with `current` emphasized, press-to-select, sliding window for
/// long lists. Pair with `Carousel` or any custom page switcher.
///
/// # Examples
///
/// ```
/// use martensite::widgets::pips_pager::PipsPager;
///
/// let p = PipsPager::new(5);
/// assert_eq!(p.current, 0);
/// ```
pub mod pips_pager;

/// Polar area chart (Nightingale / Coxcomb rose idiom) — every
/// wedge spans an equal angle and its radius encodes the value
/// (sqrt-scaled so area stays linear). Hovering a wedge parks its
/// index in `take_hovered`.
///
/// # Examples
///
/// ```
/// use martensite::widgets::polar_area::PolarArea;
///
/// let p = PolarArea::new().slice("Jan", 12.0).slice("Feb", 7.0);
/// assert_eq!(p.slice_count(), 2);
/// ```
pub mod polar_area;

/// Mini anchored confirmation bubble (Ant `Popconfirm`) — question,
/// Confirm/Cancel pair, arrow tail, `ConfirmResult` seam.
///
/// # Examples
///
/// ```
/// use martensite::widgets::Popconfirm;
///
/// let c = Popconfirm::new().question("Sure?");
/// assert!(!c.is_open());
/// ```
pub mod popconfirm;

/// Anchored bubble with an arrow tail (GtkPopover / NSPopover) —
/// optional title, single content child, `BoundsEdge` placement with
/// flip, `autohide` light-dismiss semantics.
///
/// # Examples
///
/// ```
/// use martensite::widgets::{Popover, Text};
///
/// let p = Popover::new().title("Info").child(Text::new("body"));
/// assert!(!p.is_open());
/// ```
pub mod popover;

/// Two-column property inspector (Qt PropertyBrowser / Xcode
/// inspector) — `name | value` rows with inline Text/Bool/Choice
/// editors under collapsible section headers, `take_changed` seam.
///
/// # Examples
///
/// ```
/// use martensite::widgets::{PropertyGrid, PropertyRow};
///
/// let g = PropertyGrid::new()
///     .section("Transform", [PropertyRow::bool("Visible", true)]);
/// assert_eq!(g.row_count(), 1);
/// ```
pub mod property_grid;

/// QR barcode renderer — paints a precomputed module matrix with the
/// standard quiet zone (Ant `QRCode` display side). Encoding is
/// app-space: feed `from_matrix`/`from_bits` the output of `qrcode`,
/// `fast_qr`, or your own encoder.
///
/// # Examples
///
/// ```
/// use martensite::widgets::qr_code::QrCode;
///
/// let qr = QrCode::from_matrix(vec![vec![false; 21]; 21]);
/// assert_eq!(qr.module_count(), 21);
/// ```
pub mod qr_code;

/// Spider/polar chart (Ant `Radar`, Qt `QPolarChart`) — concentric
/// ring polygons + axis spokes, translucent series fills with stroked
/// edges and vertex dots, palette-cycled colors.
///
/// # Examples
///
/// ```
/// use martensite::widgets::radar_chart::{RadarChart, RadarSeries};
///
/// let c = RadarChart::new()
///     .axes(["A", "B", "C"])
///     .series(RadarSeries::new("S", [1.0, 2.0, 3.0]));
/// assert_eq!(c.axes.len(), 3);
/// ```
pub mod radar_chart;

/// Pie-menu selector — equal annular sectors around a
/// dead-zone center (marking-menu / game-ui idiom). Click a
/// sector to park its index in `take_selected`; arrows cycle
/// the highlight, Enter confirms.
///
/// # Examples
///
/// ```
/// use martensite::widgets::radial_menu::RadialMenu;
///
/// let r = RadialMenu::new().items(["Cut", "Copy", "Paste"]);
/// assert_eq!(r.item_count(), 3);
/// ```
pub mod radial_menu;

/// ARIA APG slider widget.
///
/// # Examples
///
/// ```
/// use martensite::widgets::Slider;
///
/// let s = Slider::new(0.0, 100.0).with_value(50.0);
/// ```
pub mod slider;

/// ARIA APG radio group widget.
///
/// # Examples
///
/// ```
/// use martensite::widgets::RadioGroup;
///
/// let g = RadioGroup::new(["A", "B"]);
/// ```
pub mod radio;

/// Dual-thumb range slider widget.
///
/// # Examples
///
/// ```
/// use martensite::widgets::RangeSlider;
///
/// let rs = RangeSlider::new(0.0, 100.0).with_range(20.0, 80.0);
/// ```
pub mod range_slider;

/// Star-style rating input/display (WinUI `RatingControl`,
/// KDE `KRatingWidget`, Ant `Rate`).
///
/// # Examples
///
/// ```
/// use martensite::widgets::rating::Rating;
///
/// let r = Rating::new().half_steps(true).value(3.5);
/// assert_eq!(r.get_value(), 3.5);
/// ```
pub mod rating;

/// Status result page — coloured status glyph + title + subtitle +
/// action buttons, centred in the view (Ant `Result`).
///
/// # Examples
///
/// ```
/// use martensite::widgets::{ResultPage, ResultStatus};
///
/// let r = ResultPage::new(ResultStatus::Success).title("Saved");
/// ```
pub mod result_page;

/// Corner ribbon overlay — an Ant `Badge.Ribbon` band pinned to a
/// corner of the wrapped child carrying short status text ("Beta",
/// "New"). Chrome only; the child keeps full bounds and event flow.
///
/// # Examples
///
/// ```
/// use martensite::prelude::*;
/// use martensite::widgets::ribbon::Ribbon;
///
/// let r = Ribbon::new("Beta").child(Container::new());
/// assert_eq!(r.text(), "Beta");
/// ```
pub mod ribbon;

/// Measurement-scale strip (design-tool ruler idiom) — major and
/// minor ticks across a value range on a horizontal or vertical
/// strip, a marker line at `position`, and click/drag picking that
/// parks the value in `take_picked`.
///
/// # Examples
///
/// ```
/// use martensite::widgets::ruler::Ruler;
///
/// let r = Ruler::new().range(0.0, 300.0).ticks(50.0, 10.0);
/// assert_eq!(r.major_step_value(), 50.0);
/// ```
pub mod ruler;

/// XY point-cloud chart (Ant `Scatter`, Qt `QScatterSeries`) —
/// marker dots inside a gridded axis frame, auto-fit or pinned
/// ranges, nearest-point hover seam for tooltips.
///
/// # Examples
///
/// ```
/// use martensite::widgets::scatter_chart::{ScatterChart, ScatterSeries};
///
/// let c = ScatterChart::new()
///     .series(ScatterSeries::new("A", [(0.0, 1.0), (1.0, 2.0)]));
/// assert_eq!(c.series.len(), 1);
/// ```
pub mod scatter_chart;

/// ARIA APG scroll view with smart scrollbars and rubber-band
/// overscroll.
///
/// # Examples
///
/// ```
/// use martensite::widgets::{ScrollView, Text};
///
/// let v = ScrollView::new(Text::new("content"));
/// ```
pub mod scrollview;

/// Toggleable search strip — collapses to zero height while
/// `search_mode` is off (libadwaita `AdwSearchBar`). `Escape` parks a
/// close request the app answers by flipping `search_mode`.
///
/// # Examples
///
/// ```
/// use martensite::widgets::search_bar::SearchBar;
///
/// let mut bar = SearchBar::new().placeholder("Search…");
/// bar.search_mode = true;
/// assert!(bar.field().placeholder.starts_with("Search"));
/// ```
pub mod search_bar;

/// Dedicated search input (NSSearchField, Carbon `Search`) — a
/// magnifier-prefixed, clearable `TextInput` child with an `Enter`
/// submit seam and `Role::SearchInput` accessibility.
///
/// # Examples
///
/// ```
/// use martensite::widgets::search_field::SearchField;
///
/// let s = SearchField::new().placeholder("Search…");
/// assert_eq!(s.value(), "");
/// ```
pub mod search_field;

/// Inline word-sized trend chart — line, translucent area, or bars,
/// normalized to the series' min/max with an endpoint dot (Tufte
/// sparkline / Swift Charts mini-series). Display-only; pair with
/// `Statistic` for the readout.
///
/// # Examples
///
/// ```
/// use martensite::widgets::sparkline::{Sparkline, SparkStyle};
///
/// let s = Sparkline::new([1.0, 3.0, 2.0]).style(SparkStyle::Area);
/// assert_eq!(s.point_count(), 3);
/// ```
pub mod sparkline;

/// Single-select segmented pill strip (radio-group semantics).
///
/// # Examples
///
/// ```
/// use martensite::widgets::Segmented;
///
/// let seg = Segmented::new().options(vec!["A", "B"]);
/// assert_eq!(seg.selected_index(), 0);
/// ```
pub mod segmented;

/// Shimmer loading placeholder — block, circle, or text lines — that
/// can wrap and hide a child while `loading` holds (Ant `Skeleton`).
///
/// # Examples
///
/// ```
/// use martensite::widgets::Skeleton;
///
/// let s = Skeleton::lines(3);
/// assert!(s.is_loading());
/// ```
pub mod skeleton;

/// Frequency-band equalizer display — app-driven `0..=1` band
/// amplitudes painted as segmented LED-meter columns (green /
/// amber / red thirds) with an optional decaying peak-hold tick.
/// Clicking a band parks its index in `take_pressed`.
///
/// # Examples
///
/// ```
/// use martensite::widgets::spectrum::Spectrum;
///
/// let s = Spectrum::new().bands([0.3, 0.7, 0.5]);
/// assert_eq!(s.band_count(), 3);
/// ```
pub mod spectrum;

/// Numeric spin box with ▲/▼ step buttons and an editable field.
///
/// # Examples
///
/// ```
/// use martensite::widgets::SpinBox;
///
/// let sb = SpinBox::new().range(0.0, 10.0).suffix(" px");
/// ```
pub mod spinbox;

/// KPI block — title, large formatted value, prefix/suffix, and a
/// coloured trend indicator (Ant `Statistic`).
///
/// # Examples
///
/// ```
/// use martensite::widgets::{Statistic, Trend};
///
/// let s = Statistic::new("Uptime", "99.9%").trend(Trend::Up, "+0.1%");
/// assert_eq!(s.value_text(), "99.9%");
/// ```
pub mod statistic;

/// ARIA APG select-only combobox with an overlay listbox popup.
///
/// # Examples
///
/// ```
/// use martensite::widgets::Dropdown;
///
/// let dd = Dropdown::new(["Small", "Medium", "Large"]);
/// ```
pub mod dropdown;

/// Centred icon + title + description + action placeholder for empty
/// views (ADW `StatusPage`, Ant `Empty`).
///
/// # Examples
///
/// ```
/// use martensite::widgets::EmptyState;
///
/// let e = EmptyState::new("No results").action("Clear filter");
/// ```
pub mod empty_state;

/// A settings row that expands to reveal indented nested child rows
/// (libadwaita `AdwExpanderRow`) — activatable `SettingsRow` header,
/// caret affordance, `ArrowRight`/`ArrowLeft` + semantic
/// expand/collapse.
///
/// # Examples
///
/// ```
/// use martensite::widgets::expander_row::ExpanderRow;
/// use martensite::widgets::SettingsRow;
///
/// let row = ExpanderRow::new("Network").child(SettingsRow::new("Wi-Fi"));
/// assert_eq!(row.child_len(), 1);
/// ```
pub mod expander_row;

/// File-name button face that parks a chooser request (GTK
/// `FileChooserButton`, `NSOpenPanel` well) — folder/document glyph
/// plus file name or placeholder; activation drains via
/// `take_activated` for the app to mount a platform dialog.
///
/// # Examples
///
/// ```
/// use martensite::widgets::file_chooser_button::FileChooserButton;
///
/// let b = FileChooserButton::new().placeholder("Choose…");
/// assert!(b.selected_name().is_none());
/// ```
pub mod file_chooser_button;

/// Virtualized data grid — pinned sortable/resizable column header
/// over a scrollable striped body (QTableView, GTK `ColumnView`,
/// WinUI `DataGrid`, Ant `Table`).
///
/// # Examples
///
/// ```
/// use martensite::widgets::{Table, TableColumn};
///
/// let t = Table::new()
///     .columns([TableColumn::new("name", "Name")])
///     .row(["Ada"]);
/// assert_eq!(t.row_count(), 1);
/// ```
pub mod table;

/// ARIA APG tabs widget (tab list + tab panels).
///
/// # Examples
///
/// ```
/// use martensite::widgets::Tabs;
///
/// let t = Tabs::with_labels(["General", "Advanced"]);
/// ```
pub mod tabs;

/// Classic temperature-scale indicator — bulb + column fill
/// against a ticked `min..=max` range, with `warning`/`critical`
/// thresholds tinting the fluid. Status-display companion to
/// `Battery` and `VuMeter`.
///
/// # Examples
///
/// ```
/// use martensite::widgets::thermometer::Thermometer;
///
/// let t = Thermometer::new().range(0.0, 100.0).value(42.0);
/// assert_eq!(t.reading(), 42.0);
/// ```
pub mod thermometer;

/// Segmented time-of-day field with inline hour/minute/AM-PM
/// editing (`QTimeEdit` / WinUI `TimePicker`) — arrow stepping,
/// two-digit rollover typing, `take_edited` seam.
///
/// # Examples
///
/// ```
/// use martensite::widgets::{Time, TimePicker};
///
/// let t = TimePicker::new().time(Time { hour: 9, minute: 30 });
/// assert_eq!(t.text(), "09:30");
/// ```
pub mod time_picker;

/// Vertical event feed with a dot-and-connector rail (Ant `Timeline`).
///
/// # Examples
///
/// ```
/// use martensite::widgets::{Timeline, TimelineItem};
///
/// let tl = Timeline::new().item(TimelineItem::new("a")).pending("live…");
/// assert_eq!(tl.item_count(), 1);
/// assert!(tl.has_pending());
/// ```
pub mod timeline;

/// Pressed-state (latched) toggle button.
///
/// # Examples
///
/// ```
/// use martensite::widgets::ToggleButton;
///
/// let b = ToggleButton::new("Bold").pressed(true);
/// assert!(b.pressed);
/// ```
pub mod toggle_button;

/// Horizontal strip of action items (QToolBar / NSToolbar / WinUI
/// `CommandBar`) — buttons, arbitrary widgets, separators, flexible
/// spacers, and a `»` overflow collapse.
///
/// # Examples
///
/// ```
/// use martensite::widgets::{Button, Toolbar};
///
/// let bar = Toolbar::new().item(Button::new("New")).spacer().item(Button::new("About"));
/// assert_eq!(bar.item_count(), 3);
/// ```
pub mod toolbar;

/// Chip-ized token entry field — typed text commits into removable
/// tokens (AppKit `NSTokenField`, WCT `TokenizingTextBox`).
///
/// # Examples
///
/// ```
/// use martensite::widgets::token_field::TokenField;
///
/// let t = TokenField::new().tokens(["rust", "gui"]);
/// assert_eq!(t.token_list().len(), 2);
/// ```
pub mod token_field;

/// Guided-tour overlay — coach-mark cards over a dimmed backdrop
/// with an optional spotlight cutout (Ant `Tour`, driver.js).
///
/// # Examples
///
/// ```
/// use martensite::widgets::Tour;
///
/// let t = Tour::new().step("Welcome", "This is the app.", None);
/// assert_eq!(t.step_count(), 1);
/// ```
pub mod tour;

/// ARIA APG tooltip with an overlay bubble and `aria-describedby`.
///
/// # Examples
///
/// ```
/// use martensite::widgets::{Text, Tooltip};
///
/// let tip = Tooltip::new(Text::new("trigger"), "tip text");
/// ```
pub mod tooltip;

/// Dual-pane shuttle list — source items left, chosen items right,
/// with → / ← move buttons (Ant `Transfer`).
///
/// # Examples
///
/// ```
/// use martensite::widgets::Transfer;
///
/// let t = Transfer::new().source(["a", "b"]).target(["c"]);
/// assert_eq!(t.source_count(), 2);
/// ```
pub mod transfer;

/// Dropdown face whose popup hosts a `TreeView` — the Ant
/// `TreeSelect` / WinUI tree-combo pattern. Leaf selection commits
/// the value; branch selection toggles expansion.
///
/// # Examples
///
/// ```
/// use martensite::widgets::tree_select::TreeSelect;
/// use martensite::widgets::TreeNode;
///
/// let ts = TreeSelect::new().tree(vec![TreeNode::new("root")]);
/// assert_eq!(ts.node_count(), 1);
/// ```
pub mod tree_select;

/// Virtualized hierarchical tree of labelled nodes (APG tree view).
///
/// # Examples
///
/// ```
/// use martensite::widgets::{TreeNode, TreeView};
///
/// let t = TreeView::new().roots(vec![TreeNode::new("root")]);
/// assert_eq!(t.visible_row_count(), 1);
/// ```
pub mod tree_view;

/// Squarified treemap — area-proportional item cells laid out by the
/// worst-aspect-ratio folding algorithm (Ant `Treemap`, d3 treemap).
/// Palette-cycled cells, fit-checked labels, hover index seam.
///
/// # Examples
///
/// ```
/// use martensite::widgets::treemap::{Treemap, TreemapItem};
///
/// let t = Treemap::new().item(TreemapItem::new("src", 60.0));
/// assert_eq!(t.items.len(), 1);
/// ```
pub mod treemap;

/// Two- or three-set overlap diagram — translucent categorical
/// circles in the classic side-by-side or triangular layout with
/// a label at each circle's outer point. Hovering a unique region
/// parks its set index in `take_hovered`; the shared center parks
/// `usize::MAX`.
///
/// # Examples
///
/// ```
/// use martensite::widgets::venn::Venn;
///
/// let v = Venn::new().set("Rust").set("Go").set("Zig");
/// assert_eq!(v.set_count(), 3);
/// ```
pub mod venn;

/// Mirrored-density distribution chart (violin-plot idiom) —
/// symmetric silhouettes per category from `0..=1` half-width
/// profiles, with a center line and quartile ticks. Hovering a
/// violin's slot parks its index in `take_hovered`. Companion to
/// `BoxPlot`.
///
/// # Examples
///
/// ```
/// use martensite::widgets::violin::Violin;
///
/// let v = Violin::new().series("A", [0.1, 0.9, 1.0, 0.9, 0.1]);
/// assert_eq!(v.series_count(), 1);
/// ```
pub mod violin;

/// Multi-channel VU / PPM level meter — instantaneous
/// channel strips over a green→amber→red zone gradient with
/// slowly decaying peak-hold markers, driven by `push` or
/// `levels`. Companion to `Spectrum` and `Waveform` in the
/// audio-display family.
///
/// # Examples
///
/// ```
/// use martensite::widgets::vu_meter::VuMeter;
///
/// let mut v = VuMeter::new().channels(2);
/// v.push([0.8, 0.4]);
/// assert_eq!(v.peak_list()[0], 0.8);
/// ```
pub mod vu_meter;

/// Running-total bridge chart (McKinsey / finance waterfall
/// idiom) — floating delta columns spanning previous-to-new
/// cumulative totals, full columns for totals, dashed connectors
/// between consecutive tops, and a hover seam parking the column
/// index.
///
/// # Examples
///
/// ```
/// use martensite::widgets::waterfall::Waterfall;
///
/// let w = Waterfall::new().total("Start", 100.0).delta("Gain", 25.0);
/// assert_eq!(w.entry_count(), 2);
/// ```
pub mod waterfall;

/// Tiled text overlay for stamping watermarks over content (Ant
/// `Watermark`) — a leaf meant for [`Stack`] layering.
///
/// # Examples
///
/// ```
/// use martensite::widgets::Watermark;
///
/// let wm = Watermark::new("Draft").opacity(0.2);
/// assert_eq!(wm.text(), "Draft");
/// ```
pub mod watermark;

/// Amplitude-column audio display (SoundCloud / Audacity waveform
/// idiom) — symmetric peak columns around the midline split into
/// played (accent) and unplayed (muted) regions by the playhead
/// fraction, with a click-to-seek seam and a playhead line.
///
/// # Examples
///
/// ```
/// use martensite::widgets::waveform::Waveform;
///
/// let w = Waveform::new().peaks([0.2, 0.8, 0.5]).position(0.3);
/// assert_eq!(w.peak_count(), 3);
/// ```
pub mod waveform;

/// Vertically scrollable option drum that snaps to the centered row
/// (iOS `UIPickerView`, SwiftUI wheel style).
///
/// # Examples
///
/// ```
/// use martensite::widgets::WheelPicker;
///
/// let w = WheelPicker::new().items(["a", "b"]);
/// assert_eq!(w.item_count(), 2);
/// ```
pub mod wheel_picker;

/// Multi-step flow — `Steps` header, one visible page, and a
/// back/next/cancel footer (`QWizard`, Ant Steps+form idiom).
///
/// # Examples
///
/// ```
/// use martensite::widgets::{Text, Wizard};
///
/// let w = Wizard::new().step("a", Text::new("p")).step("b", Text::new("q"));
/// assert_eq!(w.step_count(), 2);
/// ```
pub mod wizard;

/// Weight-scaled packed word display (Ant `WordCloud` / tag-cloud
/// idiom) — the heaviest words render largest in a categorical
/// palette, packed row-wise across the surface. Clicking a word
/// parks its original index in `take_clicked`; hovering parks
/// `take_hovered`.
///
/// # Examples
///
/// ```
/// use martensite::widgets::word_cloud::WordCloud;
///
/// let w = WordCloud::new().word("rust", 10.0).word("gui", 5.0);
/// assert_eq!(w.max_weight(), 10.0);
/// ```
pub mod word_cloud;

/// Two-dimensional drag controller (Kaoss-pad / Ableton XY idiom) —
/// a square pad whose thumb tracks a normalized `(x, y)` value with
/// crosshair guides, arrow-key nudges, `Home`/`End` snaps, and a
/// two-axis `"x,y"` `SetValue` semantic.
///
/// # Examples
///
/// ```
/// use martensite::widgets::xy_pad::XYPad;
///
/// let pad = XYPad::new().value(0.5, 0.5).labels("Cutoff", "Res");
/// assert_eq!(pad.value_xy(), (0.5, 0.5));
/// ```
pub mod xy_pad;

pub use about::About;
pub use accordion::Accordion;
pub use action_sheet::{ActionSheet, ActionSheetResult};
pub use activity_ring::{ActivityRing, Ring};
pub use alert_dialog::{AlertDialog, AlertResult, AlertRole, AlertSeverity};
pub use analog_clock::AnalogClock;
pub use anchor::{Anchor, AnchorItem};
pub use aspect_frame::AspectFrame;
pub use auto_complete::{AutoComplete, FilterMode};
pub use avatar::Avatar;
pub use avatar_group::AvatarGroup;
pub use badge::Badge;
pub use banner::{Banner, Severity};
pub use bar_chart::BarChart;
pub use bottom_sheet::BottomSheet;
pub use box_plot::{BoxPlot, BoxSeries};
pub use breadcrumb::Breadcrumb;
pub use bullet_chart::BulletChart;
pub use button::Button;
pub use calendar::{Calendar, CalendarSelection};
pub use candlestick::{Candle, Candlestick};
pub use card::{Card, CardVariant};
pub use carousel::Carousel;
pub use cascader::{Cascader, CascaderOption};
pub use checkbox::{CheckBox, CheckState};
pub use chip::{Chip, ChipKind};
pub use chip_group::{ChipGroup, ChipSelection};
pub use clamp::Clamp;
pub use color_button::ColorButton;
pub use color_palette::ColorPalette;
pub use color_picker::{hsv_to_rgb, rgb_to_hsv, Color, ColorPicker};
pub use command_link::CommandLink;
pub use command_palette::{CommandAction, CommandPalette};
pub use container::Container;
pub use context_menu::ContextMenu;
pub use countdown::Countdown;
pub use date_picker::{Date, DatePicker};
pub use descriptions::{DescriptionItem, Descriptions};
pub use dial::Dial;
pub use dialog::Dialog;
pub use disclosure::Disclosure;
pub use drawer::Drawer;
pub use dropdown::Dropdown;
pub use empty_state::EmptyState;
pub use expander_row::ExpanderRow;
pub use external::{BindError, ExternalEngine, ExternalEngines, FramePoll};
pub use file_chooser_button::{ChooserMode, FileChooserButton};
pub use flex::{Flex, FlexDirection};
pub use float_button::FloatButton;
pub use flow_box::{FlowBox, FlowSelection};
pub use font_button::FontButton;
pub use form_field::{FormField, LabelPosition};
pub use funnel_chart::FunnelChart;
pub use gantt::{Gantt, GanttTask};
pub use gauge::Gauge;
pub use grid::{Grid, GridCell};
pub use group_box::GroupBox;
pub use header_bar::HeaderBar;
pub use heat_map::HeatMap;
pub use image::{Image, ImageFit};
pub use ink_canvas::{InkCanvas, Stroke};
pub use inline_edit::InlineEdit;
pub use ip_input::IpInput;
pub use kbd::Kbd;
pub use key_capture::KeyCapture;
pub use keyboard_shortcuts::{KeyboardShortcuts, ShortcutGroup, ShortcutRow};
pub use keypad::Keypad;
pub use lcd_number::LcdNumber;
pub use level_bar::{LevelBar, LevelZone};
pub use line_chart::{LineChart, LineSeries};
pub use link::Link;
pub use list_view::{ListView, SelectionMode, SelectionModel};
pub use log_view::{LogLine, LogSeverity, LogView};
pub use markdown::Markdown;
pub use marquee::Marquee;
pub use masonry::Masonry;
pub use media::{MediaView, VideoFit};
pub use media_controls::MediaControls;
pub use mention::Mention;
pub use menu::{Menu, MenuItem, MenuPath, MenuState};
pub use menu_bar::MenuBar;
pub use menu_button::MenuButton;
pub use nav_rail::{NavDestination, NavRail};
pub use nav_stack::NavStack;
pub use odometer::Odometer;
pub use otp_input::OtpInput;
pub use page_header::PageHeader;
pub use pagination::Pagination;
pub use pie_chart::{PieChart, PieSlice};
pub use pips_pager::PipsPager;
pub use popconfirm::{ConfirmResult, Popconfirm};
pub use popover::Popover;
pub use progress::{ProgressBar, Spinner};
pub use property_grid::{
    PropertyEditor, PropertyGrid, PropertyRow, PropertyRowKey, PropertySection,
};
pub use pull_to_refresh::PullToRefresh;
pub use qr_code::QrCode;
pub use radar_chart::{RadarChart, RadarSeries};
pub use radio::{RadioGroup, RadioOption};
pub use range_slider::{RangeSlider, RangeThumb};
pub use rating::Rating;
pub use result_page::{ResultAction, ResultPage, ResultStatus};
pub use ribbon::{Ribbon, RibbonCorner};
pub use scatter_chart::{ScatterChart, ScatterSeries};
pub use scrollview::{ScrollBarWidget, ScrollView};
pub use search_bar::SearchBar;
pub use search_field::SearchField;
pub use segmented::{Segment, Segmented};
pub use separator::Separator;
pub use settings_row::{SettingsGroup, SettingsRow};
pub use signal_strength::SignalStrength;
pub use skeleton::{Skeleton, SkeletonShape};
pub use slider::{Slider, SliderOrientation};
pub use sparkline::{SparkStyle, Sparkline};
pub use spectrum::Spectrum;
pub use spinbox::SpinBox;
pub use split_button::SplitButton;
pub use split_view::{SplitOrientation, SplitView};
pub use stack::Stack;
pub use statistic::{Statistic, Trend};
pub use status_bar::{StatusBar, StatusItem};
pub use status_dot::{Status, StatusDot};
pub use steps::{Step, Steps};
pub use swipe_actions::{SwipeAction, SwipeActions, SwipeEdge};
pub use switch::Switch;
pub use table::{SortDir, Table, TableAlign, TableColumn};
pub use tabs::{TabActivation, TabItem, Tabs};
pub use text::Text;
pub use text_area::TextArea;
pub use text_input::TextInput;
pub use time_picker::{Time, TimePicker};
pub use timeline::{Timeline, TimelineDot, TimelineItem};
pub use toast::{Toast, ToastHost};
pub use toggle_button::ToggleButton;
pub use token_field::TokenField;
pub use toolbar::Toolbar;
pub use tooltip::{Tooltip, TooltipBubble, DEFAULT_TOOLTIP_DELAY_MS, TOOLTIP_HOVER_GRACE_MS};
pub use tour::{Tour, TourStep};
pub use transfer::{MoveDir, Transfer};
pub use tree_select::TreeSelect;
pub use tree_view::{TreeNode, TreeView};
pub use treemap::{Treemap, TreemapItem};
pub use waterfall::{Waterfall, WaterfallEntry};
pub use watermark::Watermark;
pub use waveform::Waveform;
pub use wheel_picker::WheelPicker;
pub use wizard::Wizard;
pub use xy_pad::XYPad;
