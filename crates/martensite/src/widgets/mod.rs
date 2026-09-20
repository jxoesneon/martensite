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

/// Industrial alarm list with an acknowledge lifecycle (the HMI/SCADA
/// alarm-banner idiom) — severity-edged rows, per-row `ACK` chips, and
/// unacknowledged `Error` rows that flash on `tick` until acked.
///
/// # Examples
///
/// ```
/// use martensite::widgets::alarm_panel::{Alarm, AlarmPanel};
/// use martensite::widgets::banner::Severity;
///
/// let mut p = AlarmPanel::new();
/// p.push(Alarm::new(Severity::Error, "Tank 4 overpressure"));
/// assert_eq!(p.unacked_count(), 1);
/// ```
pub mod alarm_panel;

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

/// Opacity rail over a checkerboard backing, with a draggable
/// handle picking alpha in `0.0..=1.0` — the alpha strip in every
/// color picker.
///
/// # Examples
///
/// ```
/// use martensite::widgets::alpha_slider::AlphaSlider;
///
/// let mut s = AlphaSlider::new().alpha(0.5);
/// s.set_alpha(2.0);
/// assert_eq!(s.alpha_value(), 1.0);
/// ```
pub mod alpha_slider;

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

/// Paginated launcher icon grid (GNOME apps view / iOS home
/// screen) — `AppEntry` icon swatches with captions flowing across
/// fixed-size pages, `ArrowLeft`/`ArrowRight` paging with a dot
/// indicator, clicks parking `take_activated`.
///
/// # Examples
///
/// ```
/// use martensite::widgets::app_grid::{AppEntry, AppGrid};
///
/// let g = AppGrid::new().app(AppEntry::new("Files", [1; 4]));
/// assert_eq!(g.app_count(), 1);
/// ```
pub mod app_grid;

/// Conference roster (Zoom/Meet participants panel) — rows of
/// `Attendee` with a `PresenceStatus` dot, name, speaking
/// highlight, and muted / raised-hand badges. Clicks park
/// `take_selected`; `raised_hands` lists hands-up indices.
///
/// # Examples
///
/// ```
/// use martensite::widgets::attendee_list::{Attendee, AttendeeList};
///
/// let a = AttendeeList::new().attendee(Attendee::new("Ana"));
/// assert_eq!(a.attendee_count(), 1);
/// ```
pub mod attendee_list;

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

/// Video-call control cluster (Zoom/Meet idiom) — toggle pills for
/// mic/camera/speaker/share plus a red hang-up. Clicks park
/// `take_toggled`/`take_hangup`; `m`/`v`/`s`/`d`/`h`/`Escape` are
/// the keyboard equivalents.
///
/// # Examples
///
/// ```
/// use martensite::widgets::call_controls::{CallControl, CallControls};
///
/// let c = CallControls::new();
/// assert!(c.is_on(CallControl::Mic));
/// ```
pub mod call_controls;

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

/// Timed subtitle band rendered over media (closed-caption idiom)
/// — the host loads `CaptionCue`s and calls `set_position` with
/// the media clock; the active cue paints bottom-centered on a
/// translucent band. `Role::Status` live region.
///
/// # Examples
///
/// ```
/// use martensite::widgets::captions::Captions;
/// use std::time::Duration;
///
/// let mut c = Captions::new().cue("Hi", 0.0, 1.0);
/// c.set_position(Duration::from_secs_f32(0.5));
/// assert_eq!(c.active_text(), Some("Hi"));
/// ```
pub mod captions;

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

/// Fanned card stack — only the front card is interactive, with up
/// to two cards behind peeking at the bottom edge. A horizontal drag
/// past the swipe threshold dismisses the front card; arrow keys
/// cycle without dismissing (the Tinder/card-stack idiom).
///
/// # Examples
///
/// ```
/// use martensite::widgets::card_deck::CardDeck;
/// use martensite::widgets::text::Text;
///
/// let mut d = CardDeck::new()
///     .card(Text::new("a"))
///     .card(Text::new("b"));
/// d.cycle_next();
/// assert_eq!(d.depth(), 2);
/// ```
pub mod card_deck;

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

/// Message composer — draft field + Send button with optional
/// attach/emoji affordances (Slack/iMessage idiom). Enter commits
/// the draft to `take_sent`, Escape clears; attach/emoji park
/// `take_attach`/`take_emoji`. Completes the chat family with
/// `MessageList` and `TypingIndicator`.
///
/// # Examples
///
/// ```
/// use martensite::widgets::chat_input::ChatInput;
///
/// let mut c = ChatInput::new();
/// c.insert("hi");
/// assert_eq!(c.draft(), "hi");
/// ```
pub mod chat_input;

/// Scrollable list of checkable rows (installer / software-picker
/// idiom) — click or Space toggles, `checked_indices` for the host,
/// `take_changed` seam.
///
/// # Examples
///
/// ```
/// use martensite::widgets::check_list::CheckList;
///
/// let c = CheckList::new().items(["a", "b"]);
/// assert_eq!(c.item_count(), 2);
/// ```
pub mod check_list;

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

/// Interactive 8×8 chess board — click-click moves, FEN placement,
/// undo, keyboard navigation, optional coordinate margin.
///
/// # Examples
///
/// ```
/// use martensite::widgets::chess_board::{ChessBoard, Piece, Side};
///
/// let b = ChessBoard::new();
/// assert_eq!(b.piece_at(0), Some((Piece::Rook, Side::White)));
/// ```
pub mod chess_board;

/// Dual-sided game clock (FIDE idiom) — tapping a face presses its
/// plunger: that side stops, the opponent runs; `tick` drains the
/// running side, expiry parks `take_flagged`, optional Fischer
/// increment. Companion to `ChessBoard`.
///
/// # Examples
///
/// ```
/// use martensite::widgets::chess_clock::ChessClock;
/// use std::time::Duration;
///
/// assert_eq!(ChessClock::new(Duration::from_secs(300)).moves(), 0);
/// ```
pub mod chess_clock;

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

/// Clipboard manager history panel (Windows `Win+V`, CopyQ) —
/// newest-first snippet rows, `push` evicting the oldest unpinned
/// entry past `max`, clicks parking `take_pasted`, and `set_pinned`
/// pinning entries against eviction.
///
/// # Examples
///
/// ```
/// use martensite::widgets::clipboard_history::ClipboardHistory;
///
/// let mut h = ClipboardHistory::new();
/// h.push("hello");
/// assert_eq!(h.entry_count(), 1);
/// ```
pub mod clipboard_history;

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

/// Hue ring with a draggable selector — the classic color-wheel
/// picker; design-tool sibling of `ColorPicker`. Dragging the
/// selector sweeps the hue; `rgb` converts the pick (with
/// saturation/brightness) to RGBA bytes.
///
/// # Examples
///
/// ```
/// use martensite::widgets::color_wheel::ColorWheel;
///
/// assert_eq!(ColorWheel::new().hue(120.0).rgb(), [0, 255, 0, 255]);
/// ```
pub mod color_wheel;

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

/// Nested comment list (forum/blog idiom) — avatar dot, author +
/// timestamp header, body, per-depth indent with a reply connector
/// rail. Reply clicks park `take_reply`; `Up`/`Down` + `Enter`
/// reply from the keyboard. Distinct from `MessageList`'s flat
/// chat bubbles.
///
/// # Examples
///
/// ```
/// use martensite::widgets::comment_thread::{Comment, CommentThread};
///
/// let t = CommentThread::new().comment(Comment::new(1, "ana", "2h", "Hi"));
/// assert_eq!(t.comment_count(), 1);
/// ```
pub mod comment_thread;

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

/// Celebration particle burst (checkout-success idiom) —
/// `burst` spawns colored particles at a normalized origin,
/// `tick` integrates fall/drift/fade, `take_done` fires when the
/// last particle dies. Pure decoration, no hit-testing.
///
/// # Examples
///
/// ```
/// use martensite::widgets::confetti::Confetti;
///
/// let mut c = Confetti::new().count(10);
/// c.burst(0.5, 0.0);
/// assert_eq!(c.particle_count(), 10);
/// ```
pub mod confetti;

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

/// Circular countdown timer (iOS Clock / workout-ring idiom) —
/// fractional sub-second remaining drives a smoothly draining arc
/// clockwise from 12 o'clock around an `MM:SS` readout. Space
/// toggles, `r` resets, `take_finished` reports expiry.
///
/// # Examples
///
/// ```
/// use martensite::widgets::countdown_ring::CountdownRing;
/// use std::time::Duration;
///
/// assert_eq!(CountdownRing::new(Duration::from_secs(60)).fraction(), 1.0);
/// ```
pub mod countdown_ring;

/// iTunes-style cover browser — the selected `Thumbnail` fronts
/// center full-size while neighbors recede to the sides scaled
/// down; arrows/scroll step the selection, side-cover clicks
/// select, index parks in `take_selected`.
///
/// # Examples
///
/// ```
/// use martensite::widgets::coverflow::Coverflow;
/// use martensite::widgets::Thumbnail;
///
/// let c = Coverflow::new().item(Thumbnail::new("A", [255, 0, 0, 255]));
/// assert_eq!(c.item_count(), 1);
/// ```
pub mod coverflow;

/// Draggable, resizable crop region overlay (photo-editor crop
/// tool) — normalized coordinates, corner handles, optional aspect
/// lock, scrim + rule-of-thirds paint, `take_changed` seam.
///
/// # Examples
///
/// ```
/// use martensite::widgets::crop_box::CropBox;
///
/// assert_eq!(CropBox::new().crop(0.1, 0.1, 0.8, 0.8).crop_rect(), (0.1, 0.1, 0.8, 0.8));
/// ```
pub mod crop_box;

/// Design-tool hairlines — vertical + horizontal guides tracking
/// the pointer with an `x, y` readout chip; movement parks
/// normalized coords in `take_moved`, `PointerLeave` clears.
/// Companion to `Magnifier`/`Ruler`.
///
/// # Examples
///
/// ```
/// use martensite::widgets::crosshair::Crosshair;
///
/// let mut c = Crosshair::new();
/// c.set_position(glam::Vec2::new(0.5, 0.5));
/// assert_eq!(c.position(), Some(glam::Vec2::new(0.5, 0.5)));
/// ```
pub mod crosshair;

/// Cubic-bezier easing editor (DevTools / design-tool idiom) —
/// two draggable control handles on a gridded pad, endpoints
/// pinned at (0,0) and (1,1), `bezier`/`css`/`sample` outputs
/// and a `take_changed` seam.
///
/// # Examples
///
/// ```
/// use martensite::widgets::curve_editor::CurveEditor;
///
/// let c = CurveEditor::new().handles((0.4, 0.0), (0.6, 1.0));
/// assert_eq!(c.bezier(), (0.4, 0.0, 0.6, 1.0));
/// ```
pub mod curve_editor;

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

/// Horizontal thumbnail strip with selection — the photo-editor
/// filmstrip / gallery picker idiom. Click selects and parks the
/// index in `take_selected`; `←`/`→` move the selection; the wheel
/// scrolls the strip when it overflows.
///
/// # Examples
///
/// ```
/// use martensite::widgets::filmstrip::{Filmstrip, Thumbnail};
///
/// let f = Filmstrip::new()
///     .thumb(Thumbnail::new("DSC_001", [80, 120, 200, 255]))
///     .thumb(Thumbnail::new("DSC_002", [200, 120, 80, 255]));
/// assert_eq!(f.thumb_count(), 2);
/// ```
pub mod filmstrip;

/// Ishikawa cause-and-effect diagram — a horizontal spine ending
/// in an effect box with alternating `Bone` ribs angled off it,
/// each labeled and carrying cause ticks. Hovering a rib parks
/// `take_hovered`.
///
/// # Examples
///
/// ```
/// use martensite::widgets::fishbone::{Bone, Fishbone};
///
/// let f = Fishbone::new("Defects").bone(Bone::new("People"));
/// assert_eq!(f.bone_count(), 1);
/// ```
pub mod fishbone;

/// Two-faced flip card (Anki / quiz-deck idiom) — click or Space
/// flips question to answer and parks the side in `take_flipped`;
/// `r` returns to the front and `set_card` advances the deck.
///
/// # Examples
///
/// ```
/// use martensite::widgets::flashcard::Flashcard;
///
/// assert!(!Flashcard::new("Q", "A").is_flipped());
/// ```
pub mod flashcard;

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

/// Guitar chord diagram — six strings over four frets with
/// finger dots and X/O nut markers; clicking a fret cell toggles
/// a dot and parks `(string, fret)` in `take_edited`. The
/// tablature sibling of `PianoKeys`.
///
/// # Examples
///
/// ```
/// use martensite::widgets::fretboard::Fretboard;
///
/// let f = Fretboard::new().set(0, 3).mute(5);
/// assert_eq!(f.fret_of(0), Some(3));
/// ```
pub mod fretboard;

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

/// Node-link diagram — labeled nodes on straight edges, seeded
/// on a ring and settled by `relax` spring passes; dragging a
/// node repins it (parked in `take_moved`), hovering parks the
/// index in `take_hovered`. The free-form sibling of
/// `OrgChart`, `MindMap`, and `Sankey`.
///
/// # Examples
///
/// ```
/// use martensite::widgets::graph_view::GraphView;
///
/// let g = GraphView::new().node("a").node("b").edge(0, 1);
/// assert_eq!(g.edge_count(), 1);
/// ```
pub mod graph_view;

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

/// Delayed hover preview card (Reddit/GitHub profile popover) —
/// `tick` accumulates hover time inside the trigger bounds and
/// opens the title/body card past `delay`; `take_opened` /
/// `take_closed` report transitions.
///
/// # Examples
///
/// ```
/// use martensite::widgets::hover_card::HoverCard;
///
/// assert!(!HoverCard::new("T", "B").is_open());
/// ```
pub mod hover_card;

/// Rainbow hue rail with a draggable handle — the hue strip in
/// every color picker. Click or drag to set a hue in degrees;
/// `←`/`→` step by one.
///
/// # Examples
///
/// ```
/// use martensite::widgets::hue_slider::HueSlider;
///
/// let mut s = HueSlider::new().hue(200.0);
/// s.set_hue(480.0);
/// assert_eq!(s.hue_value(), 120.0);
/// ```
pub mod hue_slider;

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

/// Sectioned property panel (Xcode / Figma right rail) —
/// collapsible `InspectorSection`s of label/value rows. Section
/// headers toggle; row clicks park `take_selected` with
/// `(section, row)`, and `set_value` updates values host-side.
///
/// # Examples
///
/// ```
/// use martensite::widgets::inspector::Inspector;
///
/// let i = Inspector::new().section("Transform").row("X", "12.5");
/// assert_eq!(i.value_of(0, 0), Some("12.5"));
/// ```
pub mod inspector;

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

/// Code-39 linear barcode display — the 1D companion to
/// `QrCode`. Accepts the 43-symbol Code-39 alphabet, renders
/// nine-element bar/space patterns inside `*` guards with
/// quiet zones and an optional text strip — real, scannable
/// output.
///
/// # Examples
///
/// ```
/// use martensite::widgets::barcode::Barcode;
///
/// assert_eq!(Barcode::new().text("SKU-42").symbols(), 6);
/// ```
pub mod barcode;

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

/// Agile sprint burndown chart — the ideal diagonal from total
/// work to zero with the actual remaining-work polyline the host
/// extends per `push_day`, red when running above ideal. Hovered
/// day columns park `take_hovered`.
///
/// # Examples
///
/// ```
/// use martensite::widgets::burndown::Burndown;
///
/// let mut b = Burndown::new(40.0, 10);
/// b.push_day(36.0);
/// assert_eq!(b.remaining(), Some(36.0));
/// ```
pub mod burndown;

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

/// macOS-style icon dock with proximity magnification, running dots,
/// and a click-to-launch seam.
///
/// # Examples
///
/// ```
/// use martensite::widgets::dock::{Dock, DockItem};
///
/// let d = Dock::new().item(DockItem::new("App", [80, 120, 200, 255]));
/// assert_eq!(d.item_count(), 1);
/// ```
pub mod dock;

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

/// Industrial andon signal tower — a vertical stack of
/// independently lit colored lamps (the Banner/Patlite tower-light
/// idiom), with click-to-toggle and flashing support on `tick`.
///
/// # Examples
///
/// ```
/// use martensite::widgets::stack_light::{Lamp, StackLight};
///
/// let t = StackLight::new()
///     .lamp(Lamp::new("Fault", [235, 87, 87, 255]))
///     .lamp(Lamp::new("Run", [92, 200, 120, 255]).lit(true));
/// assert!(t.lit(1));
/// ```
pub mod stack_light;

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

/// Instrument×step toggle grid with a tick-driven playhead column
/// (TR-808 / DAW step-sequencer idiom) — clicks park
/// `(row, col, on)` in `take_changed`, playhead advances park the
/// column in `take_step`, Space toggles play. Completes the music
/// family with `PianoKeys`/`Metronome`/`Equalizer`.
///
/// # Examples
///
/// ```
/// use martensite::widgets::step_sequencer::StepSequencer;
///
/// assert_eq!(StepSequencer::new(4, 16).row_count(), 4);
/// ```
pub mod step_sequencer;

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

/// Tick-driven lap timer — start/stop/reset, split recording via
/// `l`, lap pips under the face. Pairs with `Countdown` and
/// `DigitalClock`.
///
/// # Examples
///
/// ```
/// use martensite::widgets::stopwatch::Stopwatch;
///
/// assert_eq!(Stopwatch::new().face(), "00:00.00");
/// ```
pub mod stopwatch;

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

/// Collapsible JSON tree (DevTools / Postman idiom) — one row
/// per visible node with indent guides, disclosure triangles,
/// and type-colored values; clicking toggles expansion and
/// parks the child path in `take_toggled`; the wheel scrolls.
///
/// # Examples
///
/// ```
/// use martensite::widgets::json_view::{JsonNode, JsonView};
///
/// let v = JsonView::new(JsonNode::array("", [JsonNode::null("")]));
/// assert_eq!(v.visible_rows(), 2);
/// ```
pub mod json_view;

/// Card board (Trello / Ant-board idiom) — equal-width lanes
/// of stacked cards; dragging lifts the card and shows a drop
/// slot under the pointer, the board performs the move and
/// parks `(from_col, card_index, to_col)` in `take_moved`.
///
/// # Examples
///
/// ```
/// use martensite::widgets::kanban::Kanban;
///
/// let k = Kanban::new().column("Todo").card("Todo", "Task");
/// assert_eq!(k.card_count(0), 1);
/// ```
pub mod kanban;

/// Spring-return analog stick (gamepad idiom) — draggable
/// knob inside a circular gate, normalized `-1..=1` offset
/// with a dead zone, snaps back to center on release. Arrow
/// keys nudge, `Escape` recenters.
///
/// # Examples
///
/// ```
/// use martensite::widgets::joystick::Joystick;
///
/// assert_eq!(Joystick::new().value_xy(), (0.0, 0.0));
/// ```
pub mod joystick;

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

/// Chart series key (matplotlib / ECharts legend idiom) — colored
/// swatches + labels in a wrapping flow; click or Space toggles an
/// entry's dimmed "hidden series" state and parks the index in
/// `take_toggled`.
///
/// # Examples
///
/// ```
/// use martensite::widgets::legend::Legend;
///
/// let l = Legend::new().entry("Alpha", [96, 165, 250, 255]);
/// assert_eq!(l.entry_count(), 1);
/// ```
pub mod legend;

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

/// Fullscreen media overlay — dim backdrop, centered `Thumbnail`,
/// ‹ › navigation, × close, counter, caption (photo-viewer
/// idiom). Backdrop clicks and Escape park `take_closed`; arrows
/// park `take_navigated`.
///
/// # Examples
///
/// ```
/// use martensite::widgets::lightbox::Lightbox;
/// use martensite::widgets::Thumbnail;
///
/// let l = Lightbox::new().item(Thumbnail::new("a", [1; 4]));
/// assert_eq!(l.item_count(), 1);
/// ```
pub mod lightbox;

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

/// Design-tool loupe — renders a zoomed region of a source
/// `ImageData` snapshot in a clipped lens with crosshair; clicks
/// park source coordinates in `take_picked` for eyedropper flows.
///
/// # Examples
///
/// ```
/// use martensite::widgets::magnifier::Magnifier;
///
/// assert_eq!(Magnifier::new().zoom(8.0).zoom_value(), 8.0);
/// ```
pub mod magnifier;

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

/// Scrolling chat transcript — alternating sent/received
/// bubbles with sender + timestamp meta lines; the wheel
/// scrolls the backlog and `follow` snaps to the newest bubble
/// while pinned to the bottom. Pairs with `TypingIndicator`
/// and `Mention`.
///
/// # Examples
///
/// ```
/// use martensite::widgets::message_list::{Message, MessageList};
///
/// let mut l = MessageList::new();
/// l.push(Message::sent("hi"));
/// assert_eq!(l.len(), 1);
/// ```
pub mod message_list;

/// Three-pane merge display (Meld / GitLens conflict-resolver
/// idiom) — aligned `Ours | Result | Theirs` rows, conflict rows
/// tinted, per-row `‹`/`›` accept buttons writing the chosen side
/// into the result and parking `take_choice`. Sibling of
/// `DiffView`'s unified-diff column.
///
/// # Examples
///
/// ```
/// use martensite::widgets::merge_view::{MergeRow, MergeView};
///
/// let mut m = MergeView::new().row(MergeRow::conflict("a", "", "b"));
/// assert_eq!(m.conflict_count(), 1);
/// ```
pub mod merge_view;

/// Tick-driven tempo indicator — beat lamps (accent on the
/// downbeat) over a BPM label; click or Space toggles the run
/// state, `take_beat` parks each crossing for host audio, and
/// `tap` derives tempo. The rhythm sibling of `PianoKeys`
/// and `Equalizer`.
///
/// # Examples
///
/// ```
/// use martensite::widgets::metronome::Metronome;
///
/// assert_eq!(Metronome::new().bpm(96).bpm_value(), 96);
/// ```
pub mod metronome;

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

/// Balanced mind-map diagram — a center root with subtrees
/// fanned alternately left/right by leaf-count balance,
/// elbow links, and a hover seam parking the node index.
///
/// # Examples
///
/// ```
/// use martensite::widgets::mind_map::MindMap;
///
/// let m = MindMap::new().root("Plan").child("Plan", "Build");
/// assert_eq!(m.node_count(), 2);
/// ```
pub mod mind_map;

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

/// Persistent notification stack (macOS Notification Center / Win11
/// Action Center idiom) — newest-first cards with per-card `✕`
/// dismiss, a `Clear all` row, and wheel scrolling when the stack
/// overflows.
///
/// # Examples
///
/// ```
/// use martensite::widgets::notification_center::{
///     Notification, NotificationCenter,
/// };
///
/// let mut c = NotificationCenter::new();
/// c.push(Notification::new("Build done", "all targets green"));
/// assert_eq!(c.count(), 1);
/// ```
pub mod notification_center;

/// Media-session card — art swatch, title, artist/album line, and
/// an elapsed/total mini progress rail (Spotify/Apple Music "now
/// playing" bar). Metadata companion to `MediaControls` and
/// `Playlist`; a card click parks `take_clicked`.
///
/// # Examples
///
/// ```
/// use martensite::widgets::now_playing::NowPlaying;
///
/// let n = NowPlaying::new("Blue in Green", "Miles Davis").duration(327.0);
/// assert_eq!(n.duration_value(), 327.0);
/// ```
pub mod now_playing;

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

/// Top-down hierarchy diagram (organization chart idiom) —
/// `(title, subtitle)` cards with elbow connectors, the root
/// centered over its subtree and siblings on even spacing;
/// hovering a card parks its DFS index in `take_hovered`.
/// The vertical sibling of `MindMap`.
///
/// # Examples
///
/// ```
/// use martensite::widgets::org_chart::{OrgChart, OrgNode};
///
/// let o = OrgChart::new(OrgNode::new("CEO", "").child(OrgNode::new("CTO", "")));
/// assert_eq!(o.node_count(), 2);
/// ```
pub mod org_chart;

/// Ebook-reader two-page spread — left/right page faces with a
/// gutter; edge clicks or arrows turn spreads, parking the new
/// left-page index in `take_turned`. `n–m / N` counter.
///
/// # Examples
///
/// ```
/// use martensite::widgets::page_flip::PageFlip;
///
/// let mut p = PageFlip::new().pages(["a", "b", "c"]);
/// p.turn(1);
/// assert_eq!(p.left_page(), Some(2));
/// ```
pub mod page_flip;

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

/// Segmented password-strength meter (`0`–`4`) with a score-word
/// label — the zxcvbn-meter / signup-form idiom.
///
/// # Examples
///
/// ```
/// use martensite::widgets::password_strength::PasswordStrength;
///
/// assert_eq!(PasswordStrength::new().score(3).score_value(), 3);
/// ```
pub mod password_strength;

/// Android-style 3×3 unlock pattern — drag connects dots (each
/// usable once), release parks the index sequence in
/// `take_pattern` for the host to verify.
///
/// # Examples
///
/// ```
/// use martensite::widgets::pattern_lock::PatternLock;
///
/// assert_eq!(PatternLock::new().dot_count(), 9);
/// ```
pub mod pattern_lock;

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

/// Floating picture-in-picture mini window hosting one child —
/// hover reveals close/expand buttons, drags park cumulative
/// deltas in `take_dragged` for the host to reposition the
/// overlay. Video-call / media overlay idiom.
///
/// # Examples
///
/// ```
/// use martensite::core::Widget;
/// use martensite::widgets::Pip;
///
/// assert_eq!(Pip::new(martensite_core::DummyWidget).child_count(), 1);
/// ```
pub mod pip;

/// Ordered media queue — title/subtitle rows with a duration
/// column, a `▶` now-playing marker, click-to-select, and
/// drag-to-reorder parking `(from, to)` in `take_moved`.
/// `Next`/`Previous` wrap the marker. Pairs with
/// `MediaControls`.
///
/// # Examples
///
/// ```
/// use martensite::widgets::playlist::{Playlist, Track};
///
/// let mut p = Playlist::new().track(Track::new("a", "b"));
/// p.set_current(0);
/// assert_eq!(p.current(), Some(0));
/// ```
pub mod playlist;

/// Voting widget — question + option rows that reveal percentage
/// bars and counts once voted (Slack/Telegram poll idiom). Clicks
/// cast or move the user's vote and park the index in
/// `take_voted`; `close` freezes voting.
///
/// # Examples
///
/// ```
/// use martensite::widgets::poll::{Poll, PollOption};
///
/// let p = Poll::new("Lunch?").option(PollOption::new("Pizza", 3));
/// assert_eq!(p.option_count(), 1);
/// ```
pub mod poll;

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

/// User-presence chip — initials disc with a colored status dot
/// plus an optional name/status line (Teams/Slack presence idiom).
/// Distinct from `Avatar`: presence is about state, not image
/// chrome.
///
/// # Examples
///
/// ```
/// use martensite::widgets::presence::{Presence, PresenceStatus};
///
/// let p = Presence::new("Ada Lovelace", PresenceStatus::Online);
/// assert_eq!(p.status(), PresenceStatus::Online);
/// ```
pub mod presence;

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

/// Aggregate review block (App Store idiom) — large weighted
/// average, total-review caption, and a five-row distribution of
/// filled bars from 5★ to 1★ fed by `counts`. Display-only;
/// companion to `Rating`.
///
/// # Examples
///
/// ```
/// use martensite::widgets::rating_summary::RatingSummary;
///
/// let r = RatingSummary::new().counts([10, 4, 2, 1, 0]);
/// assert_eq!(r.total(), 17);
/// ```
pub mod rating_summary;

/// Row of emoji reaction chips (Slack/Teams idiom) — click toggles
/// the user's reaction (count and accent ring update, index parks
/// in `take_toggled`), optional `+` chip parks `take_add`. Pairs
/// with `MessageList`.
///
/// # Examples
///
/// ```
/// use martensite::widgets::reaction_bar::{Reaction, ReactionBar};
///
/// assert_eq!(ReactionBar::new().reaction(Reaction::new("👍", 3)).reaction_count(), 1);
/// ```
pub mod reaction_bar;

/// Standalone draggable split sash (VS Code / `QSplitterHandle`
/// idiom) — captures the drag and parks the accumulated px delta in
/// `take_moved`, arrows nudge, double-click parks `take_reset`. For
/// hosts that manage their own split geometry.
///
/// # Examples
///
/// ```
/// use martensite::widgets::resize_handle::ResizeHandle;
///
/// assert_eq!(ResizeHandle::horizontal().take_moved(), None);
/// ```
pub mod resize_handle;

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

/// Flow diagram (d3-sankey / energy-flow idiom) — named nodes
/// layered into columns by longest-path (or `node_at` hints),
/// sized by throughput, joined by value-width ribbons. Hovering
/// a ribbon parks its index in `take_hovered`.
///
/// # Examples
///
/// ```
/// use martensite::widgets::sankey::Sankey;
///
/// let s = Sankey::new().node("a").node("b").link("a", "b", 4.0);
/// assert_eq!(s.throughput(0), 4.0);
/// ```
pub mod sankey;

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

/// iOS-style overlay scroll thumb — a thin rounded pill that
/// flashes on `flash()`/scroll updates and fades on `tick`.
/// Display-only (no hit-testing); the host feeds `set_scroll`
/// position + visible fraction for custom scrollables.
///
/// # Examples
///
/// ```
/// use martensite::widgets::scroll_indicator::ScrollIndicator;
///
/// let i = ScrollIndicator::vertical().scroll(0.5, 0.25);
/// assert_eq!(i.scroll_fraction(), 0.5);
/// ```
pub mod scroll_indicator;

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

/// Feed post card (Twitter/Mastodon idiom) — author row with
/// avatar swatch and handle, body text, and a bottom action bar
/// of `CardAction`s; clicks park `take_action` and the first
/// action toggles `set_liked` state.
///
/// # Examples
///
/// ```
/// use martensite::widgets::social_card::SocialCard;
///
/// let c = SocialCard::new("Ana", "@ana", "2h", "hi");
/// assert_eq!(c.action_count(), 3);
/// ```
pub mod social_card;

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

/// Floating action button that fans out labeled mini-actions on
/// click (Material Design `SpeedDial`) — FAB toggles, mini-action
/// clicks park in `take_action`, `Esc` closes.
///
/// # Examples
///
/// ```
/// use martensite::widgets::speed_dial::SpeedDial;
///
/// let mut d = SpeedDial::new().action("Compose").action("Scan");
/// d.toggle();
/// assert!(d.is_open());
/// ```
pub mod speed_dial;

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

/// Application splash screen — centered logo letter-mark, app
/// name, version caption, a determinate progress bar, and a
/// status line. Display-only; the host drives `set_progress` /
/// `set_status` during init.
///
/// # Examples
///
/// ```
/// use martensite::widgets::splash::Splash;
///
/// let s = Splash::new("Martensite").version("0.18.0");
/// assert_eq!(s.app_name(), "Martensite");
/// ```
pub mod splash;

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

/// Categorized emoji grid (chat-composer picker idiom) — sections of
/// named glyphs; clicking a cell parks the glyph in `take_picked`.
/// Ships a compact built-in set via `EmojiPicker::standard`.
///
/// # Examples
///
/// ```
/// use martensite::widgets::emoji_picker::EmojiPicker;
///
/// assert!(EmojiPicker::standard().emoji_count() >= 40);
/// ```
pub mod emoji_picker;

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

/// Bank of vertical gain faders (graphic-EQ / mixer channel
/// strip idiom) — drag sliders over a unity marker, arrow
/// keys nudge the focused band, `0` resets to unity. Control
/// companion to the `Spectrum` display.
///
/// # Examples
///
/// ```
/// use martensite::widgets::equalizer::Equalizer;
///
/// let e = Equalizer::new().bands([0.5, 0.7, 0.3]);
/// assert_eq!(e.gain(1), 0.7);
/// ```
pub mod equalizer;

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

/// Alt-Tab app switcher strip — horizontal `Thumbnail` tiles with a
/// focus ring. Arrows/`Tab`/scroll/`cycle` move the ring, `Enter`
/// or a tile click parks `take_selected`, `Escape` parks
/// `take_cancelled`.
///
/// # Examples
///
/// ```
/// use martensite::widgets::task_switcher::TaskSwitcher;
/// use martensite::widgets::Thumbnail;
///
/// let mut t = TaskSwitcher::new().item(Thumbnail::new("a", [1; 4]));
/// t.cycle(1);
/// assert_eq!(t.current(), Some(0));
/// ```
pub mod task_switcher;

/// Scrollback display with a live prompt line — `write` appends
/// output, the prompt row shows `input` with a blinking caret,
/// `Enter` echoes `prompt + input` to the scrollback and parks
/// it in `take_submitted`. Read-only emulator surface — VT
/// parsing and PTY wiring are the host's job.
///
/// # Examples
///
/// ```
/// use martensite::widgets::terminal::Terminal;
///
/// let mut t = Terminal::new().prompt("$");
/// t.submit("ls");
/// assert_eq!(t.line(0), Some("$ ls"));
/// ```
pub mod terminal;

/// Theme gallery (GNOME Tweaks / macOS Appearance idiom) — each
/// `ThemeOption` paints as a miniature window mock in its own
/// colors with the selected card ringed. Clicks park
/// `take_selected`; `ArrowLeft`/`ArrowRight` move the selection.
///
/// # Examples
///
/// ```
/// use martensite::widgets::theme_picker::{ThemeOption, ThemePicker};
///
/// let t = ThemePicker::new().option(ThemeOption::new("Dark", [30; 4], [235; 4], [1; 4]));
/// assert_eq!(t.option_count(), 1);
/// ```
pub mod theme_picker;

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

/// Horizontally scrolling strip of structured market/news items
/// (Bloomberg ticker idiom) — symbol + price + gain/loss-colored
/// delta per item, tick-scrolled with wrap, hover pause, and a
/// `take_selected` click seam. Structured sibling of `Marquee`.
///
/// # Examples
///
/// ```
/// use martensite::widgets::ticker_tape::{TickerItem, TickerTape};
///
/// let t = TickerTape::new().item(TickerItem::new("AAPL", "189.30", 0.012));
/// assert_eq!(t.item_count(), 1);
/// ```
pub mod ticker_tape;

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

/// Collapsing toolbar strip (Qt extension / VS Code `···` idiom)
/// — labeled pills laid out left-to-right; items that don't fit
/// fold behind a trailing chevron. Item clicks park
/// `take_activated`; the chevron parks `take_overflow` with the
/// hidden indices.
///
/// # Examples
///
/// ```
/// use martensite::widgets::toolbar_overflow::ToolbarOverflow;
///
/// let t = ToolbarOverflow::new().item("Save").item("Share");
/// assert_eq!(t.item_count(), 2);
/// ```
pub mod toolbar_overflow;

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

/// Compact grid of single-select tool buttons (Photoshop tools
/// palette idiom) — one tool is always active, clicks park the
/// index in `take_selected`, grid arrows navigate. Modal tool
/// choice, unlike `Toolbar`'s action strip.
///
/// # Examples
///
/// ```
/// use martensite::widgets::tool_palette::{ToolItem, ToolPalette};
///
/// assert_eq!(ToolPalette::new().tool(ToolItem::new("✏", "Pencil")).tool_count(), 1);
/// ```
pub mod tool_palette;

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

/// Chromatic tuner display — a ±50¢ deviation needle on a
/// semicircular gauge with a green in-tune band and a large note
/// readout; `set_pitch` feeds readings, `take_steady` reports a
/// held in-tune pitch. Music-family companion to `Fretboard`.
///
/// # Examples
///
/// ```
/// use martensite::widgets::tuner::Tuner;
///
/// assert!(Tuner::new().note("A").cents(0.0).in_tune());
/// ```
pub mod tuner;

/// Bouncing three-dot "composing" affordance from chat UIs —
/// `tick` advances a staggered wave across the dots; `active`
/// gates the animation so hosts can park it statically.
///
/// # Examples
///
/// ```
/// use martensite::widgets::typing_indicator::TypingIndicator;
///
/// assert!(TypingIndicator::new().is_active());
/// ```
pub mod typing_indicator;

/// Unit converter (GNOME Calculator idiom) — category pill,
/// `from`/`to` unit cells that cycle on click, a `⇅` swap, and a
/// computed result line; `x`/`Up`/`Down` work from the keyboard.
/// Length, mass, volume, and temperature tables built in.
///
/// # Examples
///
/// ```
/// use martensite::widgets::unit_converter::{UnitCategory, UnitConverter};
///
/// let c = UnitConverter::new().in_category(UnitCategory::Mass).with_value(1.0);
/// assert!(c.convert().is_some());
/// ```
pub mod unit_converter;

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

/// Conference participant grid (Zoom/Meet idiom) — equal tiles of
/// `Participant` swatches with name captions, an accent speaking
/// ring, and a muted badge; clicks park `take_selected`. Companion
/// to `CallControls`.
///
/// # Examples
///
/// ```
/// use martensite::widgets::video_grid::{Participant, VideoGrid};
///
/// let v = VideoGrid::new().participant(Participant::new("A", [1; 4]));
/// assert_eq!(v.participant_count(), 1);
/// ```
pub mod video_grid;

/// Pannable, zoomable canvas hosting one child (Figma / map / CAD
/// idiom) — middle-drag pans, wheel zooms around the cursor, `0`
/// fits, `Home` resets; `to_content`/`to_screen` convert between
/// view and content space.
///
/// # Examples
///
/// ```
/// use martensite::widgets::viewport::Viewport;
///
/// assert_eq!(Viewport::new().zoom(2.0).zoom_value(), 2.0);
/// ```
pub mod viewport;

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

/// On-screen QWERTY keyboard (OSK / kiosk idiom) — three letter
/// rows plus momentary Shift, Backspace, and a Space bar; each
/// released key parks its produced text in `take_pressed`.
///
/// # Examples
///
/// ```
/// use martensite::widgets::virtual_keyboard::VirtualKeyboard;
///
/// assert_eq!(VirtualKeyboard::new().key_count(), 30);
/// ```
pub mod virtual_keyboard;

/// Speaker icon + gain rail + mute toggle (system-tray / media
/// volume idiom) — drag, scroll, or arrow through `0..=max` with
/// optional boost range; icon click or `m` toggles mute.
///
/// # Examples
///
/// ```
/// use martensite::widgets::volume::Volume;
///
/// assert_eq!(Volume::new().gain(0.5).gain_value(), 0.5);
/// ```
pub mod volume;

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

/// Conference lobby (Zoom/Meet admit panel) — queued attendees
/// with per-row ✓/✕ buttons parking `take_admitted`/`take_denied`,
/// and a header "Admit all" parking `usize::MAX`. Companion to
/// `AttendeeList`.
///
/// # Examples
///
/// ```
/// use martensite::widgets::waiting_room::WaitingRoom;
///
/// let mut w = WaitingRoom::new();
/// w.queue("Ana");
/// assert_eq!(w.waiting_count(), 1);
/// ```
pub mod waiting_room;

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

/// Compact weather display — painted condition glyph, temperature,
/// location, and hi/lo pair (dashboard weather-card idiom).
///
/// # Examples
///
/// ```
/// use martensite::widgets::weather::{Weather, WeatherCondition};
///
/// let w = Weather::new().condition(WeatherCondition::Rain);
/// assert_eq!(w.summary(), "Rain, 20°C");
/// ```
pub mod weather;

/// Seven-day timed agenda grid — all-day strip, hour lines, colored
/// event blocks, and click seams for events and empty slots.
///
/// # Examples
///
/// ```
/// use martensite::widgets::week_view::{WeekView, WeekEvent};
///
/// let w = WeekView::new().event(WeekEvent::new("Standup", 0, 9.0, 9.5));
/// assert_eq!(w.event_count(), 1);
/// ```
pub mod week_view;

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

/// Caption button cluster (Windows min/max/close flat buttons or
/// macOS traffic lights) — presses park `WindowAction` in
/// `take_action`; the shell owns the window ops. Slots into
/// `HeaderBar`'s trailing zone for a custom title bar.
///
/// # Examples
///
/// ```
/// use martensite::widgets::window_controls::WindowControls;
///
/// assert_eq!(WindowControls::new().button_count(), 3);
/// ```
pub mod window_controls;

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

/// Multi-timezone clock list (GNOME Clocks idiom) — `city · UTC±h
/// · HH:MM` rows ticking off a host-set UTC base with `±1d`
/// day-shift markers. `set_utc` seeds, `tick` advances.
///
/// # Examples
///
/// ```
/// use martensite::widgets::world_clock::WorldClock;
/// use martensite::widgets::Time;
///
/// let w = WorldClock::new()
///     .with_utc(Time { hour: 12, minute: 0 })
///     .zone("Tokyo", 540);
/// assert_eq!(w.local(0), Some(Time { hour: 21, minute: 0 }));
/// ```
pub mod world_clock;

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

/// Map/canvas zoom button cluster (Leaflet corner-control idiom) —
/// `+`, `−`, optional `fit`/`1:1` buttons and a percent readout;
/// presses park `ZoomAction` in `take_action` for the host's view.
///
/// # Examples
///
/// ```
/// use martensite::widgets::zoom_controls::ZoomControls;
///
/// assert_eq!(ZoomControls::new().button_count(), 4);
/// ```
pub mod zoom_controls;

pub use about::About;
pub use accordion::Accordion;
pub use action_sheet::{ActionSheet, ActionSheetResult};
pub use activity_ring::{ActivityRing, Ring};
pub use alarm_panel::{Alarm, AlarmPanel, AlarmState};
pub use alert_dialog::{AlertDialog, AlertResult, AlertRole, AlertSeverity};
pub use alpha_slider::AlphaSlider;
pub use analog_clock::AnalogClock;
pub use anchor::{Anchor, AnchorItem};
pub use app_grid::{AppEntry, AppGrid};
pub use aspect_frame::AspectFrame;
pub use attendee_list::{Attendee, AttendeeList};
pub use auto_complete::{AutoComplete, FilterMode};
pub use avatar::Avatar;
pub use avatar_group::AvatarGroup;
pub use badge::Badge;
pub use banner::{Banner, Severity};
pub use bar_chart::BarChart;
pub use barcode::Barcode;
pub use battery::Battery;
pub use bottom_sheet::BottomSheet;
pub use box_plot::{BoxPlot, BoxSeries};
pub use breadcrumb::Breadcrumb;
pub use bullet_chart::BulletChart;
pub use burndown::Burndown;
pub use button::Button;
pub use calendar::{Calendar, CalendarSelection};
pub use call_controls::{CallControl, CallControls};
pub use candlestick::{Candle, Candlestick};
pub use captions::{CaptionCue, Captions};
pub use card::{Card, CardVariant};
pub use card_deck::CardDeck;
pub use carousel::Carousel;
pub use cascader::{Cascader, CascaderOption};
pub use chat_input::ChatInput;
pub use check_list::{CheckItem, CheckList};
pub use checkbox::{CheckBox, CheckState};
pub use chess_board::{ChessBoard, Piece, Side};
pub use chess_clock::{ChessClock, ClockSide};
pub use chip::{Chip, ChipKind};
pub use chip_group::{ChipGroup, ChipSelection};
pub use clamp::Clamp;
pub use clipboard_history::{ClipEntry, ClipboardHistory};
pub use code_view::CodeView;
pub use color_button::ColorButton;
pub use color_palette::ColorPalette;
pub use color_picker::{hsv_to_rgb, rgb_to_hsv, Color, ColorPicker};
pub use color_wheel::ColorWheel;
pub use command_link::CommandLink;
pub use command_palette::{CommandAction, CommandPalette};
pub use comment_thread::{Comment, CommentThread};
pub use compass::Compass;
pub use confetti::Confetti;
pub use container::Container;
pub use context_menu::ContextMenu;
pub use countdown::Countdown;
pub use countdown_ring::CountdownRing;
pub use coverflow::Coverflow;
pub use crop_box::CropBox;
pub use crosshair::Crosshair;
pub use curve_editor::CurveEditor;
pub use date_picker::{Date, DatePicker};
pub use descriptions::{DescriptionItem, Descriptions};
pub use dial::Dial;
pub use dialog::Dialog;
pub use diff_view::{DiffKind, DiffView};
pub use digital_clock::DigitalClock;
pub use disclosure::Disclosure;
pub use dock::{Dock, DockItem};
pub use drawer::Drawer;
pub use dropdown::Dropdown;
pub use emoji_picker::{Emoji, EmojiPicker};
pub use empty_state::EmptyState;
pub use equalizer::Equalizer;
pub use expander_row::ExpanderRow;
pub use external::{BindError, ExternalEngine, ExternalEngines, FramePoll};
pub use file_chooser_button::{ChooserMode, FileChooserButton};
pub use filmstrip::{Filmstrip, Thumbnail};
pub use fishbone::{Bone, Fishbone};
pub use flashcard::Flashcard;
pub use flex::{Flex, FlexDirection};
pub use float_button::FloatButton;
pub use flow_box::{FlowBox, FlowSelection};
pub use font_button::FontButton;
pub use form_field::{FormField, LabelPosition};
pub use fretboard::Fretboard;
pub use funnel_chart::FunnelChart;
pub use gantt::{Gantt, GanttTask};
pub use gauge::Gauge;
pub use gradient_editor::{GradientEditor, GradientStop};
pub use graph_view::GraphView;
pub use grid::{Grid, GridCell};
pub use group_box::GroupBox;
pub use header_bar::HeaderBar;
pub use heat_map::HeatMap;
pub use hex_view::HexView;
pub use histogram::Histogram;
pub use hover_card::HoverCard;
pub use hue_slider::HueSlider;
pub use image::{Image, ImageFit};
pub use image_viewer::ImageViewer;
pub use ink_canvas::{InkCanvas, Stroke};
pub use inline_edit::InlineEdit;
pub use inspector::{Inspector, InspectorRow, InspectorSection};
pub use ip_input::IpInput;
pub use joystick::Joystick;
pub use json_view::{JsonNode, JsonValue, JsonView};
pub use kanban::Kanban;
pub use kbd::Kbd;
pub use key_capture::KeyCapture;
pub use keyboard_shortcuts::{KeyboardShortcuts, ShortcutGroup, ShortcutRow};
pub use keypad::Keypad;
pub use lcd_number::LcdNumber;
pub use led_matrix::LedMatrix;
pub use legend::{Legend, LegendEntry};
pub use level_bar::{LevelBar, LevelZone};
pub use lightbox::Lightbox;
pub use line_chart::{LineChart, LineSeries};
pub use link::Link;
pub use list_view::{ListView, SelectionMode, SelectionModel};
pub use log_view::{LogLine, LogSeverity, LogView};
pub use magnifier::Magnifier;
pub use markdown::Markdown;
pub use marquee::Marquee;
pub use masonry::Masonry;
pub use media::{MediaView, VideoFit};
pub use media_controls::MediaControls;
pub use mention::Mention;
pub use menu::{Menu, MenuItem, MenuPath, MenuState};
pub use menu_bar::MenuBar;
pub use menu_button::MenuButton;
pub use merge_view::{MergeRow, MergeSide, MergeView};
pub use message_list::{Message, MessageList};
pub use metronome::Metronome;
pub use mind_map::MindMap;
pub use minimap::Minimap;
pub use nav_rail::{NavDestination, NavRail};
pub use nav_stack::NavStack;
pub use notification_center::{Notification, NotificationCenter};
pub use now_playing::NowPlaying;
pub use odometer::Odometer;
pub use org_chart::{OrgChart, OrgNode};
pub use otp_input::OtpInput;
pub use page_flip::PageFlip;
pub use page_header::PageHeader;
pub use pagination::Pagination;
pub use password_strength::PasswordStrength;
pub use pattern_lock::PatternLock;
pub use piano_keys::PianoKeys;
pub use pie_chart::{PieChart, PieSlice};
pub use pip::Pip;
pub use pips_pager::PipsPager;
pub use playlist::{Playlist, Track};
pub use polar_area::PolarArea;
pub use poll::{Poll, PollOption};
pub use popconfirm::{ConfirmResult, Popconfirm};
pub use popover::Popover;
pub use presence::{Presence, PresenceStatus};
pub use progress::{ProgressBar, Spinner};
pub use property_grid::{
    PropertyEditor, PropertyGrid, PropertyRow, PropertyRowKey, PropertySection,
};
pub use pull_to_refresh::PullToRefresh;
pub use qr_code::QrCode;
pub use radar_chart::{RadarChart, RadarSeries};
pub use radial_menu::RadialMenu;
pub use radio::{RadioGroup, RadioOption};
pub use range_slider::{RangeSlider, RangeThumb};
pub use rating::Rating;
pub use rating_summary::RatingSummary;
pub use reaction_bar::{Reaction, ReactionBar};
pub use resize_handle::ResizeHandle;
pub use result_page::{ResultAction, ResultPage, ResultStatus};
pub use ribbon::{Ribbon, RibbonCorner};
pub use ruler::{Ruler, RulerOrientation};
pub use sankey::Sankey;
pub use scatter_chart::{ScatterChart, ScatterSeries};
pub use scroll_indicator::ScrollIndicator;
pub use scrollview::{ScrollBarWidget, ScrollView};
pub use search_bar::SearchBar;
pub use search_field::SearchField;
pub use segmented::{Segment, Segmented};
pub use separator::Separator;
pub use settings_row::{SettingsGroup, SettingsRow};
pub use signal_strength::SignalStrength;
pub use skeleton::{Skeleton, SkeletonShape};
pub use slider::{Slider, SliderOrientation};
pub use social_card::{CardAction, SocialCard};
pub use sparkline::{SparkStyle, Sparkline};
pub use spectrum::Spectrum;
pub use speed_dial::SpeedDial;
pub use spinbox::SpinBox;
pub use splash::Splash;
pub use split_button::SplitButton;
pub use split_flap::SplitFlap;
pub use split_view::{SplitOrientation, SplitView};
pub use stack::Stack;
pub use stack_light::{Lamp, StackLight};
pub use statistic::{Statistic, Trend};
pub use status_bar::{StatusBar, StatusItem};
pub use status_dot::{Status, StatusDot};
pub use step_sequencer::StepSequencer;
pub use steps::{Step, Steps};
pub use stopwatch::Stopwatch;
pub use stream_graph::StreamGraph;
pub use strip_chart::StripChart;
pub use sunburst::{Sunburst, SunburstNode};
pub use swipe_actions::{SwipeAction, SwipeActions, SwipeEdge};
pub use switch::Switch;
pub use table::{SortDir, Table, TableAlign, TableColumn};
pub use tabs::{TabActivation, TabItem, Tabs};
pub use task_switcher::TaskSwitcher;
pub use terminal::Terminal;
pub use text::Text;
pub use text_area::TextArea;
pub use text_input::TextInput;
pub use theme_picker::{ThemeOption, ThemePicker};
pub use thermometer::Thermometer;
pub use ticker_tape::{TickerItem, TickerTape};
pub use time_picker::{Time, TimePicker};
pub use timeline::{Timeline, TimelineDot, TimelineItem};
pub use toast::{Toast, ToastHost};
pub use toggle_button::ToggleButton;
pub use token_field::TokenField;
pub use tool_palette::{ToolItem, ToolPalette};
pub use toolbar::Toolbar;
pub use toolbar_overflow::ToolbarOverflow;
pub use tooltip::{Tooltip, TooltipBubble, DEFAULT_TOOLTIP_DELAY_MS, TOOLTIP_HOVER_GRACE_MS};
pub use tour::{Tour, TourStep};
pub use transfer::{MoveDir, Transfer};
pub use tree_select::TreeSelect;
pub use tree_view::{TreeNode, TreeView};
pub use treemap::{Treemap, TreemapItem};
pub use tuner::Tuner;
pub use typing_indicator::TypingIndicator;
pub use unit_converter::{UnitCategory, UnitConverter};
pub use venn::Venn;
pub use video_grid::{Participant, VideoGrid};
pub use viewport::Viewport;
pub use violin::Violin;
pub use virtual_keyboard::VirtualKeyboard;
pub use volume::Volume;
pub use vu_meter::VuMeter;
pub use waiting_room::WaitingRoom;
pub use waterfall::{Waterfall, WaterfallEntry};
pub use watermark::Watermark;
pub use waveform::Waveform;
pub use weather::{Weather, WeatherCondition};
pub use week_view::{WeekEvent, WeekView};
pub use wheel_picker::WheelPicker;
pub use window_controls::{CaptionStyle, WindowAction, WindowControls};
pub use wizard::Wizard;
pub use word_cloud::WordCloud;
pub use world_clock::{WorldClock, ZoneEntry};
pub use xy_pad::XYPad;
pub use zoom_controls::{ZoomAction, ZoomControls};
