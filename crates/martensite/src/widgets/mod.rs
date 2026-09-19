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

pub use accordion::Accordion;
pub use action_sheet::{ActionSheet, ActionSheetResult};
pub use alert_dialog::{AlertDialog, AlertResult, AlertRole, AlertSeverity};
pub use auto_complete::{AutoComplete, FilterMode};
pub use avatar::Avatar;
pub use badge::Badge;
pub use banner::{Banner, Severity};
pub use bottom_sheet::BottomSheet;
pub use breadcrumb::Breadcrumb;
pub use button::Button;
pub use card::{Card, CardVariant};
pub use cascader::{Cascader, CascaderOption};
pub use checkbox::{CheckBox, CheckState};
pub use chip::{Chip, ChipKind};
pub use color_picker::{hsv_to_rgb, rgb_to_hsv, Color, ColorPicker};
pub use command_palette::{CommandAction, CommandPalette};
pub use container::Container;
pub use context_menu::ContextMenu;
pub use date_picker::{Date, DatePicker};
pub use descriptions::{DescriptionItem, Descriptions};
pub use dial::Dial;
pub use dialog::Dialog;
pub use disclosure::Disclosure;
pub use drawer::Drawer;
pub use dropdown::Dropdown;
pub use empty_state::EmptyState;
pub use external::{BindError, ExternalEngine, ExternalEngines, FramePoll};
pub use flex::{Flex, FlexDirection};
pub use gauge::Gauge;
pub use group_box::GroupBox;
pub use image::{Image, ImageFit};
pub use key_capture::KeyCapture;
pub use level_bar::{LevelBar, LevelZone};
pub use list_view::{ListView, SelectionMode, SelectionModel};
pub use masonry::Masonry;
pub use media::{MediaView, VideoFit};
pub use menu::{Menu, MenuItem, MenuPath, MenuState};
pub use menu_bar::MenuBar;
pub use nav_rail::{NavDestination, NavRail};
pub use otp_input::OtpInput;
pub use pagination::Pagination;
pub use popconfirm::{ConfirmResult, Popconfirm};
pub use popover::Popover;
pub use progress::{ProgressBar, Spinner};
pub use pull_to_refresh::PullToRefresh;
pub use radio::{RadioGroup, RadioOption};
pub use range_slider::{RangeSlider, RangeThumb};
pub use rating::Rating;
pub use result_page::{ResultAction, ResultPage, ResultStatus};
pub use scrollview::{ScrollBarWidget, ScrollView};
pub use search_field::SearchField;
pub use segmented::{Segment, Segmented};
pub use separator::Separator;
pub use settings_row::{SettingsGroup, SettingsRow};
pub use skeleton::{Skeleton, SkeletonShape};
pub use slider::{Slider, SliderOrientation};
pub use spinbox::SpinBox;
pub use split_view::{SplitOrientation, SplitView};
pub use stack::Stack;
pub use statistic::{Statistic, Trend};
pub use status_bar::{StatusBar, StatusItem};
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
pub use tree_view::{TreeNode, TreeView};
pub use watermark::Watermark;
pub use wizard::Wizard;
