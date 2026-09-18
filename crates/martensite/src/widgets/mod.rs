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

pub use banner::{Banner, Severity};
pub use button::Button;
pub use checkbox::CheckBox;
pub use container::Container;
pub use dialog::Dialog;
pub use disclosure::Disclosure;
pub use drawer::Drawer;
pub use dropdown::Dropdown;
pub use external::{BindError, ExternalEngine, ExternalEngines, FramePoll};
pub use flex::{Flex, FlexDirection};
pub use media::{MediaView, VideoFit};
pub use progress::{ProgressBar, Spinner};
pub use radio::{RadioGroup, RadioOption};
pub use scrollview::{ScrollBarWidget, ScrollView};
pub use separator::Separator;
pub use slider::{Slider, SliderOrientation};
pub use stack::Stack;
pub use switch::Switch;
pub use tabs::{TabActivation, TabItem, Tabs};
pub use text::Text;
pub use text_input::TextInput;
pub use toast::{Toast, ToastHost};
pub use tooltip::{Tooltip, TooltipBubble, DEFAULT_TOOLTIP_DELAY_MS, TOOLTIP_HOVER_GRACE_MS};
