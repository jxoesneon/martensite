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

pub use accordion::Accordion;
pub use action_sheet::{ActionSheet, ActionSheetResult};
pub use alert_dialog::{AlertDialog, AlertResult, AlertRole, AlertSeverity};
pub use avatar::Avatar;
pub use badge::Badge;
pub use banner::{Banner, Severity};
pub use bottom_sheet::BottomSheet;
pub use breadcrumb::Breadcrumb;
pub use button::Button;
pub use card::{Card, CardVariant};
pub use checkbox::{CheckBox, CheckState};
pub use chip::{Chip, ChipKind};
pub use container::Container;
pub use context_menu::ContextMenu;
pub use dialog::Dialog;
pub use disclosure::Disclosure;
pub use drawer::Drawer;
pub use dropdown::Dropdown;
pub use empty_state::EmptyState;
pub use external::{BindError, ExternalEngine, ExternalEngines, FramePoll};
pub use flex::{Flex, FlexDirection};
pub use group_box::GroupBox;
pub use image::{Image, ImageFit};
pub use list_view::{ListView, SelectionMode, SelectionModel};
pub use media::{MediaView, VideoFit};
pub use menu::{Menu, MenuItem, MenuPath, MenuState};
pub use menu_bar::MenuBar;
pub use popconfirm::{ConfirmResult, Popconfirm};
pub use popover::Popover;
pub use progress::{ProgressBar, Spinner};
pub use radio::{RadioGroup, RadioOption};
pub use range_slider::{RangeSlider, RangeThumb};
pub use result_page::{ResultAction, ResultPage, ResultStatus};
pub use scrollview::{ScrollBarWidget, ScrollView};
pub use segmented::{Segment, Segmented};
pub use separator::Separator;
pub use skeleton::{Skeleton, SkeletonShape};
pub use slider::{Slider, SliderOrientation};
pub use spinbox::SpinBox;
pub use stack::Stack;
pub use switch::Switch;
pub use tabs::{TabActivation, TabItem, Tabs};
pub use text::Text;
pub use text_area::TextArea;
pub use text_input::TextInput;
pub use toast::{Toast, ToastHost};
pub use toggle_button::ToggleButton;
pub use tooltip::{Tooltip, TooltipBubble, DEFAULT_TOOLTIP_DELAY_MS, TOOLTIP_HOVER_GRACE_MS};
pub use tree_view::{TreeNode, TreeView};
