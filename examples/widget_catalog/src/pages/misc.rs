//! Misc family — date/time, social, communication, and specialty
//! widgets that don't fit a tighter group.

use std::time::Duration;

use martensite::widgets::accordion::Accordion;
use martensite::widgets::address_bar::AddressBar;
use martensite::widgets::attendee_list::{Attendee, AttendeeList};
use martensite::widgets::avatar::Avatar;
use martensite::widgets::avatar_group::AvatarGroup;
use martensite::widgets::breakout_rooms::{BreakoutRooms, Room};
use martensite::widgets::calendar::Calendar;
use martensite::widgets::call_controls::CallControls;
use martensite::widgets::card_deck::CardDeck;
use martensite::widgets::carousel::Carousel;
use martensite::widgets::cascader::{Cascader, CascaderOption};
use martensite::widgets::chess_board::ChessBoard;
use martensite::widgets::chess_clock::ChessClock;
use martensite::widgets::comment_thread::{Comment, CommentThread};
use martensite::widgets::confetti::Confetti;
use martensite::widgets::copyable::Copyable;
use martensite::widgets::date_picker::DatePicker;
use martensite::widgets::emoji_picker::EmojiPicker;
use martensite::widgets::flashcard::Flashcard;
use martensite::widgets::fretboard::Fretboard;
use martensite::widgets::keyboard_shortcuts::{KeyboardShortcuts, ShortcutGroup};
use martensite::widgets::message_list::{Message, MessageList};
use martensite::widgets::metronome::Metronome;
use martensite::widgets::page_flip::PageFlip;
use martensite::widgets::pattern_lock::PatternLock;
use martensite::widgets::piano_keys::PianoKeys;
use martensite::widgets::pips_pager::PipsPager;
use martensite::widgets::poll::{Poll, PollOption};
use martensite::widgets::presence::{Presence, PresenceStatus};
use martensite::widgets::pricing_table::{Plan, PricingTable};
use martensite::widgets::pull_to_refresh::PullToRefresh;
use martensite::widgets::rating_summary::RatingSummary;
use martensite::widgets::reaction_bar::{Reaction, ReactionBar};
use martensite::widgets::scratch_card::ScratchCard;
use martensite::widgets::social_card::SocialCard;
use martensite::widgets::stopwatch::Stopwatch;
use martensite::widgets::text::Text;
use martensite::widgets::ticket::Ticket;
use martensite::widgets::time_picker::TimePicker;
use martensite::widgets::video_grid::{Participant, VideoGrid};
use martensite::widgets::virtual_keyboard::VirtualKeyboard;
use martensite::widgets::waiting_room::WaitingRoom;
use martensite::widgets::week_view::{WeekEvent, WeekView};
use martensite::widgets::wizard::Wizard;
use martensite::widgets::word_cloud::WordCloud;
use martensite::widgets::world_clock::WorldClock;

use crate::page::{Page, PropSpec};
use crate::pages::{downcast_mut, meta, page, SnipProp};

// ---- Sectioned/boxed lists -------------------------------------------------

page!(AccordionPage {
    meta: meta(
        "Accordion",
        "Misc",
        "Vertically-stacked collapsible sections.",
        "Group",
        &[
            ("Qt", "QToolBox"),
            ("HTML", "<details> stack"),
            ("React", "accordion"),
            ("GNOME", "expander rows")
        ],
        false,
    ),
    props: &[
        PropSpec::Bool {
            key: "multi",
            label: "Allow multiple",
            default: false
        },
        PropSpec::Float {
            key: "gap",
            label: "Gap",
            min: 0.0,
            max: 64.0,
            step: 0.5,
            default: 0.0
        },
    ],
    build: |p| {
        let mut a = Accordion::new().allow_multiple(p.bool("multi"));
        for title in ["Profile", "Security", "Advanced"] {
            a = a.section(title, Text::new(format!("{title} content")));
        }
        {
            let mut __w = a;
            if p.f64("gap") != 0.0 {
                __w = __w.gap(p.f64("gap") as f32);
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = format!(
            "Accordion::new()\n    .allow_multiple({})\n    .section(\"Profile\", content)",
            p.bool("multi"),
        );
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[("gap", ".gap", SnipProp::Float(0.0))],
        ));
        __s
    },
});

page!(AddressBarPage {
    meta: meta(
        "AddressBar",
        "Misc",
        "Browser address field — URL, state icon, progress.",
        "TextField",
        &[
            ("Browser", "address bar"),
            ("Qt", "QLineEdit"),
            ("Safari", "URL bar"),
            ("React", "url bar")
        ],
        false,
    ),
    props: &[
        PropSpec::Text {
            key: "url",
            label: "URL",
            default: "https://example.com/docs",
        },
        PropSpec::Header {
            label: "State & Accessibility"
        },
        PropSpec::Text {
            key: "a11y_label",
            label: "A11y label",
            default: ""
        },
    ],
    build: |p| {
        let mut __w = AddressBar::new(p.str("url"));
        if !p.str("a11y_label").is_empty() {
            __w = __w.label(p.str("a11y_label"));
        }
        Box::new(__w)
    },
    snippet: |p| {
        let mut __s = format!("AddressBar::new({:?})", p.str("url"));
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[("a11y_label", ".label", SnipProp::Text(""))],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(a) = downcast_mut::<AddressBar>(w) {
            if let Some(act) = a.take_action() {
                out.push(format!("action {act:?}"));
            }
        }
    },
});

page!(CalendarPage {
    meta: meta(
        "Calendar",
        "Misc",
        "Month grid — date selection + optional range.",
        "Grid",
        &[
            ("GTK", "GtkCalendar"),
            ("Qt", "QCalendarWidget"),
            ("iOS", "calendar"),
            ("React", "<Calendar>")
        ],
        false,
    ),
    props: &[
        PropSpec::Bool {
            key: "monday",
            label: "Week starts Mon",
            default: true
        },
        PropSpec::Choice {
            key: "selection",
            label: "Selection",
            options: &["Day", "Range"],
            default: 0
        },
        PropSpec::Text {
            key: "date",
            label: "Date",
            default: ""
        },
        PropSpec::Text {
            key: "today",
            label: "Today",
            default: ""
        },
        PropSpec::Text {
            key: "min_date",
            label: "Min Date",
            default: ""
        },
        PropSpec::Text {
            key: "max_date",
            label: "Max Date",
            default: ""
        },
        PropSpec::Text {
            key: "month_names",
            label: "Month Names",
            default: ""
        },
        PropSpec::Text {
            key: "weekday_names",
            label: "Weekday Names",
            default: ""
        },
        PropSpec::Text {
            key: "range_start",
            label: "Range Start",
            default: ""
        },
        PropSpec::Text {
            key: "range_end",
            label: "Range End",
            default: ""
        },
        PropSpec::Header {
            label: "State & Accessibility"
        },
        PropSpec::Bool {
            key: "enabled",
            label: "Enabled",
            default: true
        },
        PropSpec::Text {
            key: "a11y_label",
            label: "A11y label",
            default: ""
        },
    ],
    build: |p| {
        let mut __w = Calendar::new().week_starts_monday(p.bool("monday"));
        __w = __w.enabled(p.bool("enabled"));
        if !p.str("a11y_label").is_empty() {
            __w = __w.label(p.str("a11y_label"));
        }
        if p.choice("selection") != 0 {
            __w = __w.selection(match p.choice("selection") {
                0 => martensite::widgets::calendar::CalendarSelection::Day,
                1 => martensite::widgets::calendar::CalendarSelection::Range,
                _ => martensite::widgets::calendar::CalendarSelection::Day,
            });
        }
        if let Some(v) = crate::pages::parse_date(p.str("date")) {
            __w = __w.date(v);
        }
        if let Some(v) = crate::pages::parse_date(p.str("today")) {
            __w = __w.today(v);
        }
        if let Some(v) = crate::pages::parse_date(p.str("min_date")) {
            __w = __w.min_date(v);
        }
        if let Some(v) = crate::pages::parse_date(p.str("max_date")) {
            __w = __w.max_date(v);
        }
        if let Some(v) = crate::pages::str_arr::<12>(p.str("month_names")) {
            __w = __w.month_names(v);
        }
        if let Some(v) = crate::pages::str_arr::<7>(p.str("weekday_names")) {
            __w = __w.weekday_names(v);
        }
        if !p.str("range_start").is_empty() || !p.str("range_end").is_empty() {
            __w = __w.range(
                crate::pages::parse_date(p.str("range_start")).unwrap_or(
                    martensite::widgets::date_picker::Date {
                        year: 2024,
                        month: 1,
                        day: 1,
                    },
                ),
                crate::pages::parse_date(p.str("range_end")).unwrap_or(
                    martensite::widgets::date_picker::Date {
                        year: 2024,
                        month: 1,
                        day: 1,
                    },
                ),
            );
        }
        Box::new(__w)
    },
    snippet: |p| {
        let mut __s = format!("Calendar::new().week_starts_monday({})", p.bool("monday"));
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                (
                    "selection",
                    ".selection",
                    SnipProp::Choice(&[
                        "martensite::widgets::calendar::CalendarSelection::Day",
                        "martensite::widgets::calendar::CalendarSelection::Range",
                    ]),
                ),
                ("enabled", ".enabled", SnipProp::Bool(true)),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        __s.push_str(&crate::pages::snip_textmap(
            p,
            "date",
            ".date",
            "",
            crate::pages::expr_date,
        ));
        __s.push_str(&crate::pages::snip_textmap(
            p,
            "today",
            ".today",
            "",
            crate::pages::expr_date,
        ));
        __s.push_str(&crate::pages::snip_textmap(
            p,
            "min_date",
            ".min_date",
            "",
            crate::pages::expr_date,
        ));
        __s.push_str(&crate::pages::snip_textmap(
            p,
            "max_date",
            ".max_date",
            "",
            crate::pages::expr_date,
        ));
        __s.push_str(&crate::pages::snip_textmap(
            p,
            "month_names",
            ".month_names",
            "",
            crate::pages::expr_arr,
        ));
        __s.push_str(&crate::pages::snip_textmap(
            p,
            "weekday_names",
            ".weekday_names",
            "",
            crate::pages::expr_arr,
        ));
        if !p.str("range_start").is_empty() || !p.str("range_end").is_empty() {
            __s.push_str(&format!(
                "\n    .range({}, {})",
                crate::pages::expr_date(p.str("range_start")).unwrap_or_default(),
                crate::pages::expr_date(p.str("range_end")).unwrap_or_default()
            ));
        }
        __s
    },
    poll: |w, out| {
        if let Some(c) = downcast_mut::<Calendar>(w) {
            if let Some(d) = c.take_selected() {
                out.push(format!("selected → {d:?}"));
            }
        }
    },
});

page!(DatePickerPage {
    meta: meta(
        "DatePicker",
        "Misc",
        "Date field with a calendar popup.",
        "ComboBox",
        &[
            ("HTML", "<input type=date>"),
            ("Qt", "QDateEdit"),
            ("SwiftUI", "DatePicker"),
            ("React", "date picker")
        ],
        true,
    ),
    props: &[
        PropSpec::Text {
            key: "label",
            label: "Label",
            default: "Due date"
        },
        PropSpec::Text {
            key: "placeholder",
            label: "Placeholder",
            default: ""
        },
        PropSpec::Text {
            key: "format",
            label: "Format",
            default: "{year}-{month:02}-{day:02}"
        },
        PropSpec::Bool {
            key: "week_starts_monday",
            label: "Week Starts Monday",
            default: true
        },
        PropSpec::Bool {
            key: "range_mode",
            label: "Range Mode",
            default: false
        },
        PropSpec::Text {
            key: "date",
            label: "Date",
            default: ""
        },
        PropSpec::Text {
            key: "weekday_names",
            label: "Weekday Names",
            default: ""
        },
        PropSpec::Text {
            key: "month_names",
            label: "Month Names",
            default: ""
        },
        PropSpec::Text {
            key: "min_date",
            label: "Min Date",
            default: ""
        },
        PropSpec::Text {
            key: "max_date",
            label: "Max Date",
            default: ""
        },
        PropSpec::Text {
            key: "today",
            label: "Today",
            default: ""
        },
        PropSpec::Header {
            label: "State & Accessibility"
        },
        PropSpec::Bool {
            key: "enabled",
            label: "Enabled",
            default: true
        },
    ],
    build: |p| {
        let mut d = DatePicker::new().label(p.str("label"));
        d.open();
        {
            let mut __w = d;
            __w = __w.enabled(p.bool("enabled"));
            if !p.str("placeholder").is_empty() {
                __w = __w.placeholder(p.str("placeholder"));
            }
            if p.str("format") != "{year}-{month:02}-{day:02}" {
                __w = __w.format(p.str("format"));
            }
            if !p.bool("week_starts_monday") {
                __w = __w.week_starts_monday(p.bool("week_starts_monday"));
            }
            if p.bool("range_mode") {
                __w = __w.range_mode(p.bool("range_mode"));
            }
            if let Some(v) = crate::pages::parse_date(p.str("date")) {
                __w = __w.date(v);
            }
            if let Some(v) = crate::pages::str_arr::<7>(p.str("weekday_names")) {
                __w = __w.weekday_names(v);
            }
            if let Some(v) = crate::pages::str_arr::<12>(p.str("month_names")) {
                __w = __w.month_names(v);
            }
            if let Some(v) = crate::pages::parse_date(p.str("min_date")) {
                __w = __w.min_date(v);
            }
            if let Some(v) = crate::pages::parse_date(p.str("max_date")) {
                __w = __w.max_date(v);
            }
            if let Some(v) = crate::pages::parse_date(p.str("today")) {
                __w = __w.today(v);
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = format!("DatePicker::new().label({:?})", p.str("label"));
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("placeholder", ".placeholder", SnipProp::Text("")),
                (
                    "format",
                    ".format",
                    SnipProp::Text("{year}-{month:02}-{day:02}"),
                ),
                (
                    "week_starts_monday",
                    ".week_starts_monday",
                    SnipProp::Bool(true),
                ),
                ("range_mode", ".range_mode", SnipProp::Bool(false)),
                ("enabled", ".enabled", SnipProp::Bool(true)),
            ],
        ));
        __s.push_str(&crate::pages::snip_textmap(
            p,
            "date",
            ".date",
            "",
            crate::pages::expr_date,
        ));
        __s.push_str(&crate::pages::snip_textmap(
            p,
            "weekday_names",
            ".weekday_names",
            "",
            crate::pages::expr_arr,
        ));
        __s.push_str(&crate::pages::snip_textmap(
            p,
            "month_names",
            ".month_names",
            "",
            crate::pages::expr_arr,
        ));
        __s.push_str(&crate::pages::snip_textmap(
            p,
            "min_date",
            ".min_date",
            "",
            crate::pages::expr_date,
        ));
        __s.push_str(&crate::pages::snip_textmap(
            p,
            "max_date",
            ".max_date",
            "",
            crate::pages::expr_date,
        ));
        __s.push_str(&crate::pages::snip_textmap(
            p,
            "today",
            ".today",
            "",
            crate::pages::expr_date,
        ));
        __s
    },
    poll: |w, out| {
        if let Some(d) = downcast_mut::<DatePicker>(w) {
            if let Some(v) = d.take_selected() {
                out.push(format!("date → {v:?}"));
            }
        }
    },
});

page!(TimePickerPage {
    meta: meta(
        "TimePicker",
        "Misc",
        "HH:MM segmented time field.",
        "TextField",
        &[
            ("HTML", "<input type=time>"),
            ("Qt", "QTimeEdit"),
            ("SwiftUI", "DatePicker(hour)"),
            ("React", "time field")
        ],
        false,
    ),
    props: &[
        PropSpec::Bool {
            key: "use24",
            label: "24-hour",
            default: true
        },
        PropSpec::Int {
            key: "minute_step",
            label: "Minute Step",
            min: 0,
            max: 32,
            default: 1
        },
        PropSpec::Text {
            key: "time",
            label: "Time",
            default: ""
        },
        PropSpec::Header {
            label: "State & Accessibility"
        },
        PropSpec::Bool {
            key: "enabled",
            label: "Enabled",
            default: true
        },
    ],
    build: |p| {
        let mut __w = TimePicker::new().use_24h(p.bool("use24")).label("Time");
        __w = __w.enabled(p.bool("enabled"));
        if p.i64("minute_step") != 1 {
            __w = __w.minute_step(p.i64("minute_step") as u32);
        }
        if let Some(v) = crate::pages::parse_time(p.str("time")) {
            __w = __w.time(v);
        }
        Box::new(__w)
    },
    snippet: |p| {
        let mut __s = format!("TimePicker::new().use_24h({})", p.bool("use24"));
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("minute_step", ".minute_step", SnipProp::Int(1)),
                ("enabled", ".enabled", SnipProp::Bool(true)),
            ],
        ));
        __s.push_str(&crate::pages::snip_textmap(
            p,
            "time",
            ".time",
            "",
            crate::pages::expr_time,
        ));
        __s
    },
    poll: |w, out| {
        if let Some(t) = downcast_mut::<TimePicker>(w) {
            if let Some(time) = t.take_edited() {
                out.push(format!("time → {time:?}"));
            }
        }
    },
});

page!(WeekViewPage {
    meta: meta(
        "WeekView",
        "Misc",
        "Week calendar grid with event blocks.",
        "Grid",
        &[
            ("Calendar", "week view"),
            ("Qt", "custom"),
            ("React", "week grid"),
            ("Outlook", "week")
        ],
        false,
    ),
    props: &[
        PropSpec::Text {
            key: "hour_range",
            label: "Hour Range (csv)",
            default: ""
        },
        PropSpec::Text {
            key: "day_names",
            label: "Day Names (7 csv)",
            default: ""
        },
        PropSpec::Header {
            label: "State & Accessibility"
        },
        PropSpec::Text {
            key: "a11y_label",
            label: "A11y label",
            default: ""
        },
    ],
    build: |_p| {
        let mut wv = WeekView::new();
        wv = wv.event(WeekEvent::new("Standup", 1, 9.0, 9.5));
        wv = wv.event(WeekEvent::new("Review", 3, 14.0, 15.5));
        {
            let mut __w = wv;
            if !_p.str("a11y_label").is_empty() {
                __w = __w.label(_p.str("a11y_label"));
            }
            if let Some(v) = crate::pages::parse_pair(_p.str("hour_range")) {
                __w = __w.hour_range(v.0 as f32, v.1 as f32);
            }
            if let Some(v) = crate::pages::str_arr::<7>(_p.str("day_names")) {
                __w = __w.day_names(v.each_ref().map(|s| s.as_str()));
            }
            Box::new(__w)
        }
    },
    snippet: |_p| {
        let mut __s = "WeekView::new().event(WeekEvent::new(\"Standup\", 1, 9.0, 9.5))".to_string();
        __s.push_str(&crate::pages::prop_snippet(
            _p,
            &[("a11y_label", ".label", SnipProp::Text(""))],
        ));
        __s.push_str(&crate::pages::snip_textmap(
            _p,
            "hour_range",
            ".hour_range",
            "",
            crate::pages::expr_pair,
        ));
        __s.push_str(&crate::pages::snip_textmap(
            _p,
            "day_names",
            ".day_names",
            "",
            crate::pages::expr_arr,
        ));
        __s
    },
    poll: |w, out| {
        if let Some(wv) = downcast_mut::<WeekView>(w) {
            if let Some(i) = wv.take_clicked() {
                out.push(format!("event {i}"));
            }
        }
    },
});

// ---- Social / communication ------------------------------------------------

page!(AvatarPage {
    meta: meta(
        "Avatar",
        "Misc",
        "User avatar — image or initials monogram.",
        "Image",
        &[
            ("Material", "Avatar"),
            ("Qt", "custom"),
            ("React", "<Avatar>"),
            ("iOS", "monogram")
        ],
        false,
    ),
    props: &[
        PropSpec::Text {
            key: "name",
            label: "Name",
            default: "Ada Lovelace"
        },
        PropSpec::Float {
            key: "size",
            label: "Size",
            min: 20.0,
            max: 96.0,
            step: 4.0,
            default: 40.0
        },
        PropSpec::Bool {
            key: "rounded",
            label: "Rounded",
            default: true
        },
    ],
    build: |p| Box::new(
        Avatar::new(p.str("name"))
            .size(p.f64("size") as f32)
            .rounded(if p.bool("rounded") { 999.0 } else { 0.0 }),
    ),
    snippet: |p| format!(
        "Avatar::new({:?}).size({:?})",
        p.str("name"),
        p.f64("size") as f32,
    ),
});

page!(AvatarGroupPage {
    meta: meta(
        "AvatarGroup",
        "Misc",
        "Overlapping avatar stack with +N overflow.",
        "Group",
        &[
            ("Material", "AvatarGroup"),
            ("Slack", "avatar stack"),
            ("React", "avatar group"),
            ("Qt", "custom")
        ],
        false,
    ),
    props: &[
        PropSpec::Int {
            key: "max",
            label: "Max visible",
            min: 1,
            max: 8,
            default: 3
        },
        PropSpec::Float {
            key: "overlap",
            label: "Overlap",
            min: -9.625,
            max: 100.0,
            step: 1.0,
            default: 0.25
        },
        PropSpec::Float {
            key: "size",
            label: "Size",
            min: 0.0,
            max: 64.0,
            step: 0.5,
            default: 0.0
        },
        PropSpec::Header {
            label: "State & Accessibility"
        },
        PropSpec::Bool {
            key: "enabled",
            label: "Enabled",
            default: true
        },
        PropSpec::Text {
            key: "a11y_label",
            label: "A11y label",
            default: ""
        },
    ],
    build: |p| {
        let mut g = AvatarGroup::new().max_count(p.i64("max") as usize);
        for name in ["Ada", "Grace", "Linus", "Alan", "Edsger"] {
            g = g.member(Avatar::new(name));
        }
        {
            let mut __w = g;
            __w = __w.enabled(p.bool("enabled"));
            if !p.str("a11y_label").is_empty() {
                __w = __w.label(p.str("a11y_label"));
            }
            if p.f64("overlap") != 0.25 {
                __w = __w.overlap(p.f64("overlap") as f32);
            }
            if p.f64("size") != 0.0 {
                __w = __w.size(p.f64("size") as f32);
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = format!("AvatarGroup::new().max_count({})", p.i64("max"));
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("overlap", ".overlap", SnipProp::Float(0.25)),
                ("size", ".size", SnipProp::Float(0.0)),
                ("enabled", ".enabled", SnipProp::Bool(true)),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        __s
    },
});

page!(MessageListPage {
    meta: meta(
        "MessageList",
        "Misc",
        "Chat bubble list — incoming/outgoing with times.",
        "List",
        &[
            ("iOS", "Messages"),
            ("Slack", "channel"),
            ("React", "chat list"),
            ("Qt", "custom")
        ],
        false,
    ),
    props: &[
        PropSpec::Text {
            key: "messages",
            label: "Messages (sender|body; > = sent)",
            default: ""
        },
        PropSpec::Header {
            label: "State & Accessibility"
        },
        PropSpec::Bool {
            key: "loading",
            label: "Loading",
            default: false
        },
        PropSpec::Text {
            key: "a11y_label",
            label: "A11y label",
            default: ""
        },
    ],
    build: |_p| {
        let mut ml = MessageList::new();
        ml.push(Message::received("Ada", "Did the catalog ship?"));
        ml.push(Message::sent("Just merged."));
        ml.push(Message::received("Ada", "Nice — screenshots?"));
        {
            let mut __w = ml;
            __w = __w.loading(_p.bool("loading"));
            if !_p.str("a11y_label").is_empty() {
                __w = __w.label(_p.str("a11y_label"));
            }
            let __v = crate::pages::parse_messages(_p.str("messages"));
            if !__v.is_empty() {
                __w = __w.messages(__v);
            }
            Box::new(__w)
        }
    },
    snippet: |_p| {
        let mut __s = "MessageList::new() /* push(Message::received(…)) */".to_string();
        __s.push_str(&crate::pages::prop_snippet(
            _p,
            &[
                ("loading", ".loading", SnipProp::Bool(false)),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        __s.push_str(&crate::pages::snip_textmap(
            _p,
            "messages",
            ".messages",
            "",
            crate::pages::expr_messages,
        ));
        __s
    },
});

page!(CommentThreadPage {
    meta: meta(
        "CommentThread",
        "Misc",
        "Nested comment tree — replies, loading state.",
        "List",
        &[
            ("Reddit", "comments"),
            ("GitHub", "review"),
            ("React", "comment thread"),
            ("Qt", "custom")
        ],
        false,
    ),
    props: &[
        PropSpec::Header {
            label: "State & Accessibility"
        },
        PropSpec::Bool {
            key: "loading",
            label: "Loading",
            default: false
        },
        PropSpec::Text {
            key: "a11y_label",
            label: "A11y label",
            default: ""
        },
    ],
    build: |_p| {
        {
            let mut __w = CommentThread::new()
                .comment(Comment::new(1, "ada", "2h ago", "This layout works well."))
                .comment(Comment::new(
                    2,
                    "grace",
                    "1h ago",
                    "Agreed — the RTL seam too.",
                ));
            __w = __w.loading(_p.bool("loading"));
            if !_p.str("a11y_label").is_empty() {
                __w = __w.label(_p.str("a11y_label"));
            }
            Box::new(__w)
        }
    },
    snippet: |_p| {
        let mut __s =
            "CommentThread::new().comment(Comment::new(1, \"ada\", \"2h\", \"…\"))".to_string();
        __s.push_str(&crate::pages::prop_snippet(
            _p,
            &[
                ("loading", ".loading", SnipProp::Bool(false)),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(ct) = downcast_mut::<CommentThread>(w) {
            if let Some(id) = ct.take_reply() {
                out.push(format!("reply → {id}"));
            }
        }
    },
});

page!(SocialCardPage {
    meta: meta(
        "SocialCard",
        "Misc",
        "Social post card — author, body, like/reply actions.",
        "Group",
        &[
            ("Twitter", "tweet"),
            ("Mastodon", "post"),
            ("React", "post card"),
            ("Qt", "custom")
        ],
        false,
    ),
    props: &[
        PropSpec::Text {
            key: "body",
            label: "Body",
            default: "Widgets are fun."
        },
        PropSpec::Text {
            key: "avatar_color",
            label: "Avatar Color",
            default: ""
        },
        PropSpec::Text {
            key: "with_actions",
            label: "With Actions",
            default: ""
        },
        PropSpec::Header {
            label: "State & Accessibility"
        },
        PropSpec::Text {
            key: "a11y_label",
            label: "A11y label",
            default: ""
        },
    ],
    build: |p| {
        let mut __w = SocialCard::new("ada", "@ada", "2h", p.str("body"));
        if !p.str("a11y_label").is_empty() {
            __w = __w.label(p.str("a11y_label"));
        }
        if let Some(v) = crate::pages::parse_rgba(p.str("avatar_color")) {
            __w = __w.avatar_color(v);
        }
        let __v = crate::pages::csv(p, "with_actions");
        if !__v.is_empty() {
            __w = __w.with_actions(
                __v.into_iter()
                    .map(|t| martensite::widgets::social_card::CardAction::new(t, 0))
                    .collect(),
            );
        }
        Box::new(__w)
    },
    snippet: |p| {
        let mut __s = format!(
            "SocialCard::new(\"ada\", \"@ada\", \"2h\", {:?})",
            p.str("body")
        );
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[("a11y_label", ".label", SnipProp::Text(""))],
        ));
        __s.push_str(&crate::pages::snip_textmap(
            p,
            "avatar_color",
            ".avatar_color",
            "",
            crate::pages::expr_rgba,
        ));
        __s.push_str(&crate::pages::snip_textmap(
            p,
            "with_actions",
            ".with_actions",
            "",
            crate::pages::expr_strs,
        ));
        __s
    },
    poll: |w, out| {
        if let Some(sc) = downcast_mut::<SocialCard>(w) {
            if let Some(a) = sc.take_action() {
                out.push(format!("action {a:?}"));
            }
        }
    },
});

page!(ReactionBarPage {
    meta: meta(
        "ReactionBar",
        "Misc",
        "Emoji reaction chips with counts — tap to toggle.",
        "Group",
        &[
            ("Slack", "reactions"),
            ("GitHub", "reactions"),
            ("React", "reaction bar"),
            ("Discord", "reacts")
        ],
        false,
    ),
    props: &[
        PropSpec::Bool {
            key: "addable",
            label: "Addable",
            default: true
        },
        PropSpec::Header {
            label: "State & Accessibility"
        },
        PropSpec::Text {
            key: "a11y_label",
            label: "A11y label",
            default: ""
        },
    ],
    build: |_p| {
        let mut __w = ReactionBar::new()
            .reaction(Reaction::new("👍", 4))
            .reaction(Reaction::new("🎉", 2))
            .reaction(Reaction::new("🚀", 7));
        if !_p.str("a11y_label").is_empty() {
            __w = __w.label(_p.str("a11y_label"));
        }
        if !_p.bool("addable") {
            __w = __w.addable(_p.bool("addable"));
        }
        Box::new(__w)
    },
    snippet: |_p| {
        let mut __s = "ReactionBar::new().reaction(Reaction::new(\"👍\", 4))".to_string();
        __s.push_str(&crate::pages::prop_snippet(
            _p,
            &[
                ("addable", ".addable", SnipProp::Bool(true)),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(rb) = downcast_mut::<ReactionBar>(w) {
            if let Some(i) = rb.take_toggled() {
                out.push(format!("reaction {i}"));
            }
        }
    },
});

page!(PollPage {
    meta: meta(
        "Poll",
        "Misc",
        "Voting poll — options, counts, results.",
        "Group",
        &[
            ("Telegram", "poll"),
            ("Slack", "poll"),
            ("React", "poll"),
            ("Qt", "custom")
        ],
        false,
    ),
    props: &[
        PropSpec::Text {
            key: "question",
            label: "Question",
            default: "Ship it?"
        },
        PropSpec::Bool {
            key: "closed",
            label: "Closed",
            default: false
        },
        PropSpec::Bool {
            key: "anonymous",
            label: "Anonymous",
            default: false
        },
    ],
    build: |p| {
        let mut pl = Poll::new(p.str("question"));
        pl = pl.option(PollOption::new("Yes", 12));
        pl = pl.option(PollOption::new("No", 3));
        {
            let mut __w = pl;
            if p.bool("closed") {
                __w = __w.closed(p.bool("closed"));
            }
            if p.bool("anonymous") {
                __w = __w.anonymous(p.bool("anonymous"));
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = format!(
            "Poll::new({:?}).option(PollOption::new(\"Yes\", 12))",
            p.str("question")
        );
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("closed", ".closed", SnipProp::Bool(false)),
                ("anonymous", ".anonymous", SnipProp::Bool(false)),
            ],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(pl) = downcast_mut::<Poll>(w) {
            if let Some(i) = pl.take_voted() {
                out.push(format!("voted {i}"));
            }
        }
    },
});

page!(PresencePage {
    meta: meta(
        "Presence",
        "Misc",
        "Avatar + presence dot — online/away/busy/offline.",
        "Image",
        &[
            ("Slack", "presence"),
            ("Teams", "presence"),
            ("React", "presence"),
            ("Qt", "custom")
        ],
        false,
    ),
    props: &[
        PropSpec::Choice {
            key: "status",
            label: "Status",
            options: &["Online", "Away", "Busy", "Offline"],
            default: 0,
        },
        PropSpec::Text {
            key: "status_text",
            label: "Status Text",
            default: ""
        },
        PropSpec::Header {
            label: "State & Accessibility"
        },
        PropSpec::Text {
            key: "a11y_label",
            label: "A11y label",
            default: ""
        },
    ],
    build: |p| {
        let st = match p.choice("status") {
            1 => PresenceStatus::Away,
            2 => PresenceStatus::Busy,
            3 => PresenceStatus::Offline,
            _ => PresenceStatus::Online,
        };
        {
            let mut __w = Presence::new("Ada", st).show_text(true);
            if !p.str("a11y_label").is_empty() {
                __w = __w.label(p.str("a11y_label"));
            }
            if !p.str("status_text").is_empty() {
                __w = __w.status_text(p.str("status_text"));
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = format!(
            "Presence::new(\"Ada\", PresenceStatus::{})",
            ["Online", "Away", "Busy", "Offline"][p.choice("status")],
        );
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("status_text", ".status_text", SnipProp::Text("")),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        __s
    },
});

page!(AttendeeListPage {
    meta: meta(
        "AttendeeList",
        "Misc",
        "Meeting roster — mute/hand/speaking badges.",
        "List",
        &[
            ("Zoom", "participants"),
            ("Meet", "people"),
            ("Teams", "roster"),
            ("React", "attendees")
        ],
        false,
    ),
    props: &[
        PropSpec::Header {
            label: "State & Accessibility"
        },
        PropSpec::Text {
            key: "a11y_label",
            label: "A11y label",
            default: ""
        },
    ],
    build: |_p| {
        let mut al = AttendeeList::new();
        al = al.attendee(Attendee::new("Ada"));
        al = al.attendee(Attendee::new("Grace"));
        {
            let mut __w = al;
            if !_p.str("a11y_label").is_empty() {
                __w = __w.label(_p.str("a11y_label"));
            }
            Box::new(__w)
        }
    },
    snippet: |_p| {
        let mut __s = "AttendeeList::new().attendee(Attendee::new(\"Ada\"))".to_string();
        __s.push_str(&crate::pages::prop_snippet(
            _p,
            &[("a11y_label", ".label", SnipProp::Text(""))],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(al) = downcast_mut::<AttendeeList>(w) {
            if let Some(i) = al.take_selected() {
                out.push(format!("attendee {i}"));
            }
        }
    },
});

page!(VideoGridPage {
    meta: meta(
        "VideoGrid",
        "Misc",
        "Call tile grid — speaking/mute badges.",
        "Grid",
        &[
            ("Zoom", "gallery"),
            ("Meet", "grid"),
            ("Teams", "together"),
            ("React", "video grid")
        ],
        false,
    ),
    props: &[
        PropSpec::Int {
            key: "count",
            label: "Participants",
            min: 1,
            max: 9,
            default: 4
        },
        PropSpec::Header {
            label: "State & Accessibility"
        },
        PropSpec::Text {
            key: "a11y_label",
            label: "A11y label",
            default: ""
        },
    ],
    build: |p| {
        let mut vg = VideoGrid::new();
        let colors = [
            [124, 135, 240, 255],
            [190, 132, 168, 255],
            [92, 168, 142, 255],
        ];
        for i in 0..p.i64("count") as usize {
            vg = vg.participant(Participant::new(format!("P{}", i + 1), colors[i % 3]));
        }
        {
            let mut __w = vg;
            if !p.str("a11y_label").is_empty() {
                __w = __w.label(p.str("a11y_label"));
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = format!("VideoGrid::new() /* {} participants */", p.i64("count"));
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[("a11y_label", ".label", SnipProp::Text(""))],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(vg) = downcast_mut::<VideoGrid>(w) {
            if let Some(i) = vg.take_selected() {
                out.push(format!("tile {i}"));
            }
        }
    },
});

page!(CallControlsPage {
    meta: meta(
        "CallControls",
        "Misc",
        "Call bar — mic, camera, screen, hangup.",
        "Toolbar",
        &[
            ("Zoom", "call bar"),
            ("Meet", "controls"),
            ("iOS", "CallKit"),
            ("React", "call controls")
        ],
        false,
    ),
    props: &[],
    build: |_p| Box::new(CallControls::new().label("Call")),
    snippet: |_p| "CallControls::new()".to_string(),
    poll: |w, out| {
        if let Some(cc) = downcast_mut::<CallControls>(w) {
            if let Some(t) = cc.take_toggled() {
                out.push(format!("toggle {t:?}"));
            }
            if cc.take_hangup() {
                out.push("hangup".to_string());
            }
        }
    },
});

page!(WaitingRoomPage {
    meta: meta(
        "WaitingRoom",
        "Misc",
        "Meeting lobby — admit/deny queue.",
        "List",
        &[
            ("Zoom", "waiting room"),
            ("Meet", "lobby"),
            ("Teams", "lobby"),
            ("React", "queue")
        ],
        false,
    ),
    props: &[
        PropSpec::Text {
            key: "title",
            label: "Title",
            default: "Waiting room"
        },
        PropSpec::Header {
            label: "State & Accessibility"
        },
        PropSpec::Text {
            key: "a11y_label",
            label: "A11y label",
            default: ""
        },
    ],
    build: |p| {
        let mut wr = WaitingRoom::new().title(p.str("title"));
        wr.queue("Ada");
        wr.queue("Grace");
        {
            let mut __w = wr;
            if !p.str("a11y_label").is_empty() {
                __w = __w.label(p.str("a11y_label"));
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = format!(
            "WaitingRoom::new().title({:?}) /* queue(\"Ada\") */",
            p.str("title")
        );
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[("a11y_label", ".label", SnipProp::Text(""))],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(wr) = downcast_mut::<WaitingRoom>(w) {
            if let Some(i) = wr.take_admitted() {
                out.push(format!("admitted {i}"));
            }
            if let Some(i) = wr.take_denied() {
                out.push(format!("denied {i}"));
            }
        }
    },
});

page!(BreakoutRoomsPage {
    meta: meta(
        "BreakoutRooms",
        "Misc",
        "Breakout room list — occupants, join.",
        "List",
        &[
            ("Zoom", "breakout rooms"),
            ("Meet", "breakouts"),
            ("Teams", "rooms"),
            ("React", "rooms")
        ],
        false,
    ),
    props: &[
        PropSpec::Header {
            label: "State & Accessibility"
        },
        PropSpec::Text {
            key: "a11y_label",
            label: "A11y label",
            default: ""
        },
    ],
    build: |_p| {
        let mut br = BreakoutRooms::new();
        br = br.room(Room::new("Design", 3));
        br = br.room(Room::new("Backend", 5));
        {
            let mut __w = br;
            if !_p.str("a11y_label").is_empty() {
                __w = __w.label(_p.str("a11y_label"));
            }
            Box::new(__w)
        }
    },
    snippet: |_p| {
        let mut __s = "BreakoutRooms::new().room(Room::new(\"Design\", 3))".to_string();
        __s.push_str(&crate::pages::prop_snippet(
            _p,
            &[("a11y_label", ".label", SnipProp::Text(""))],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(br) = downcast_mut::<BreakoutRooms>(w) {
            if let Some(i) = br.take_joined() {
                out.push(format!("joined {i}"));
            }
        }
    },
});

page!(EmojiPickerPage {
    meta: meta(
        "EmojiPicker",
        "Misc",
        "Emoji grid picker — sections + search.",
        "Grid",
        &[
            ("iOS", "emoji picker"),
            ("Slack", "emoji"),
            ("Qt", "custom"),
            ("React", "emoji mart")
        ],
        true,
    ),
    props: &[
        PropSpec::Header {
            label: "State & Accessibility"
        },
        PropSpec::Text {
            key: "a11y_label",
            label: "A11y label",
            default: ""
        },
    ],
    build: |_p| {
        let mut __w = EmojiPicker::new()
            .section(
                "Smileys",
                [("😀", "grinning"), ("😉", "wink"), ("😂", "joy")],
            )
            .section("Objects", [("🚀", "rocket"), ("📦", "package")]);
        if !_p.str("a11y_label").is_empty() {
            __w = __w.label(_p.str("a11y_label"));
        }
        Box::new(__w)
    },
    snippet: |_p| {
        let mut __s =
            "EmojiPicker::new().section(\"Smileys\", [(\"😀\", \"grinning\")])".to_string();
        __s.push_str(&crate::pages::prop_snippet(
            _p,
            &[("a11y_label", ".label", SnipProp::Text(""))],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(ep) = downcast_mut::<EmojiPicker>(w) {
            if let Some(g) = ep.take_picked() {
                out.push(format!("picked {g}"));
            }
        }
    },
});

// ---- Specialty -------------------------------------------------------------

page!(CarouselPage {
    meta: meta(
        "Carousel",
        "Misc",
        "Paged carousel — swipe/dots navigation.",
        "Group",
        &[
            ("iOS", "page control"),
            ("Bootstrap", "carousel"),
            ("React", "carousel"),
            ("Qt", "custom")
        ],
        false,
    ),
    props: &[
        PropSpec::Int {
            key: "pages",
            label: "Pages",
            min: 2,
            max: 8,
            default: 3
        },
        PropSpec::Bool {
            key: "wrap",
            label: "Wrap",
            default: false
        },
        PropSpec::Header {
            label: "State & Accessibility"
        },
        PropSpec::Bool {
            key: "enabled",
            label: "Enabled",
            default: true
        },
        PropSpec::Text {
            key: "a11y_label",
            label: "A11y label",
            default: ""
        },
    ],
    build: |p| {
        let mut c = Carousel::new();
        for i in 0..p.i64("pages") {
            c = c.page(Text::new(format!("Slide {}", i + 1)));
        }
        {
            let mut __w = c;
            __w = __w.enabled(p.bool("enabled"));
            if !p.str("a11y_label").is_empty() {
                __w = __w.label(p.str("a11y_label"));
            }
            if p.bool("wrap") {
                __w = __w.wrap(p.bool("wrap"));
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = format!("Carousel::new() /* {} pages */", p.i64("pages"));
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("wrap", ".wrap", SnipProp::Bool(false)),
                ("enabled", ".enabled", SnipProp::Bool(true)),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(c) = downcast_mut::<Carousel>(w) {
            if let Some(i) = c.take_navigated() {
                out.push(format!("page {i}"));
            }
        }
    },
});

page!(CardDeckPage {
    meta: meta(
        "CardDeck",
        "Misc",
        "Tinder-style swipeable card stack.",
        "Group",
        &[
            ("Tinder", "card deck"),
            ("React", "swipe cards"),
            ("Qt", "custom"),
            ("iOS", "card stack")
        ],
        false,
    ),
    props: &[
        PropSpec::Header {
            label: "State & Accessibility"
        },
        PropSpec::Text {
            key: "a11y_label",
            label: "A11y label",
            default: ""
        },
    ],
    build: |_p| {
        let mut d = CardDeck::new();
        for label in ["Top card", "Middle card", "Bottom card"] {
            d = d.card(Text::new(label));
        }
        {
            let mut __w = d;
            if !_p.str("a11y_label").is_empty() {
                __w = __w.label(_p.str("a11y_label"));
            }
            Box::new(__w)
        }
    },
    snippet: |_p| {
        let mut __s = "CardDeck::new().card(Text::new(\"Top card\"))".to_string();
        __s.push_str(&crate::pages::prop_snippet(
            _p,
            &[("a11y_label", ".label", SnipProp::Text(""))],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(d) = downcast_mut::<CardDeck>(w) {
            if d.take_dismissed() {
                out.push("card dismissed".to_string());
            }
        }
    },
});

page!(CascaderPage {
    meta: meta(
        "Cascader",
        "Misc",
        "Multi-level cascading option picker.",
        "ComboBox",
        &[
            ("Ant", "Cascader"),
            ("Qt", "cascading combo"),
            ("React", "cascader"),
            ("macOS", "column view")
        ],
        true,
    ),
    props: &[
        PropSpec::Text {
            key: "label",
            label: "Label",
            default: "Region"
        },
        PropSpec::Text {
            key: "placeholder",
            label: "Placeholder",
            default: "Select…"
        },
        PropSpec::Header {
            label: "State & Accessibility"
        },
        PropSpec::Bool {
            key: "enabled",
            label: "Enabled",
            default: true
        },
        PropSpec::Bool {
            key: "loading",
            label: "Loading",
            default: false
        },
    ],
    build: |p| {
        {
            let mut __w = Cascader::new().label(p.str("label")).options(vec![
                CascaderOption::new("Europe", "eu")
                    .child(CascaderOption::new("Portugal", "pt"))
                    .child(CascaderOption::new("Spain", "es")),
                CascaderOption::new("Asia", "as").child(CascaderOption::new("Japan", "jp")),
            ]);
            __w = __w.enabled(p.bool("enabled"));
            __w = __w.loading(p.bool("loading"));
            if p.str("placeholder") != "Select…" {
                __w = __w.placeholder(p.str("placeholder"));
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = format!("Cascader::new().label({:?})", p.str("label"));
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("placeholder", ".placeholder", SnipProp::Text("Select…")),
                ("enabled", ".enabled", SnipProp::Bool(true)),
                ("loading", ".loading", SnipProp::Bool(false)),
            ],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(c) = downcast_mut::<Cascader>(w) {
            if let Some(path) = c.take_selected() {
                out.push(format!("path → {path:?}"));
            }
        }
    },
});

page!(ChessBoardPage {
    meta: meta(
        "ChessBoard",
        "Misc",
        "Chessboard — pieces, moves, FEN.",
        "Grid",
        &[
            ("Chess", "board"),
            ("Qt", "custom"),
            ("React", "chessboard"),
            ("lichess", "board")
        ],
        false,
    ),
    props: &[
        PropSpec::Bool {
            key: "flipped",
            label: "Flipped",
            default: false
        },
        PropSpec::Bool {
            key: "read_only",
            label: "Read Only",
            default: false
        },
        PropSpec::Text {
            key: "fen",
            label: "Fen",
            default: ""
        },
        PropSpec::Header {
            label: "State & Accessibility"
        },
        PropSpec::Text {
            key: "a11y_label",
            label: "A11y label",
            default: ""
        },
    ],
    build: |p| {
        let mut b = ChessBoard::new().coordinates(true);
        b.reset();
        if p.bool("flipped") {
            // flip via flag when supported
        }
        {
            let mut __w = b;
            if !p.str("a11y_label").is_empty() {
                __w = __w.label(p.str("a11y_label"));
            }
            if p.bool("read_only") {
                __w = __w.read_only(p.bool("read_only"));
            }
            if !p.str("fen").is_empty() {
                __w = __w.fen(p.str("fen"));
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = format!(
            "ChessBoard::new().coordinates(true) /* flipped={} */",
            p.bool("flipped")
        );
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("read_only", ".read_only", SnipProp::Bool(false)),
                ("fen", ".fen", SnipProp::Text("")),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(b) = downcast_mut::<ChessBoard>(w) {
            if let Some((from, to)) = b.take_moved() {
                out.push(format!("move {from} → {to}"));
            }
        }
    },
});

page!(ChessClockPage {
    meta: meta(
        "ChessClock",
        "Misc",
        "Dual-face chess clock — tap to pass the turn.",
        "Group",
        &[
            ("Chess", "clock"),
            ("Qt", "custom"),
            ("React", "chess clock"),
            ("OTB", "analog")
        ],
        false,
    ),
    props: &[
        PropSpec::Int {
            key: "mins",
            label: "Minutes",
            min: 1,
            max: 30,
            default: 5
        },
        PropSpec::Text {
            key: "increment",
            label: "Increment",
            default: ""
        },
        PropSpec::Header {
            label: "State & Accessibility"
        },
        PropSpec::Bool {
            key: "enabled",
            label: "Enabled",
            default: true
        },
        PropSpec::Text {
            key: "a11y_label",
            label: "A11y label",
            default: ""
        },
    ],
    build: |p| {
        let mut __w = ChessClock::new(Duration::from_secs(p.i64("mins") as u64 * 60));
        __w = __w.enabled(p.bool("enabled"));
        if !p.str("a11y_label").is_empty() {
            __w = __w.label(p.str("a11y_label"));
        }
        if let Some(v) = crate::pages::parse_secs(p.str("increment")) {
            __w = __w.increment(v);
        }
        Box::new(__w)
    },
    snippet: |p| {
        let mut __s = format!(
            "ChessClock::new(Duration::from_secs({}))",
            p.i64("mins") * 60
        );
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("enabled", ".enabled", SnipProp::Bool(true)),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        __s.push_str(&crate::pages::snip_textmap(
            p,
            "increment",
            ".increment",
            "",
            crate::pages::expr_secs,
        ));
        __s
    },
    poll: |w, out| {
        if let Some(c) = downcast_mut::<ChessClock>(w) {
            if let Some(side) = c.take_pressed() {
                out.push(format!("pressed {side:?}"));
            }
            if let Some(side) = c.take_flagged() {
                out.push(format!("flagged {side:?}"));
            }
        }
    },
});

page!(ConfettiPage {
    meta: meta(
        "Confetti",
        "Misc",
        "Celebration confetti burst overlay.",
        "Canvas",
        &[
            ("iOS", "confetti"),
            ("Web", "confetti.js"),
            ("React", "confetti"),
            ("Qt", "custom")
        ],
        false,
    ),
    props: &[
        PropSpec::Int {
            key: "count",
            label: "Particles",
            min: 10,
            max: 400,
            default: 120
        },
        PropSpec::Header {
            label: "State & Accessibility"
        },
        PropSpec::Text {
            key: "a11y_label",
            label: "A11y label",
            default: ""
        },
    ],
    build: |p| {
        let mut c = Confetti::new().count(p.i64("count") as usize);
        c.burst(0.5, 0.3);
        {
            let mut __w = c;
            if !p.str("a11y_label").is_empty() {
                __w = __w.label(p.str("a11y_label"));
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = format!("Confetti::new().count({})", p.i64("count"));
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[("a11y_label", ".label", SnipProp::Text(""))],
        ));
        __s
    },
    poll: |w, out| {
        if downcast_mut::<Confetti>(w).is_some_and(|c| c.take_done()) {
            out.push("done".to_string());
        }
    },
});

page!(CopyablePage {
    meta: meta(
        "Copyable",
        "Misc",
        "Text + copy button — flash on copy.",
        "Text",
        &[
            ("Docs", "copy button"),
            ("GitHub", "copy"),
            ("React", "copyable"),
            ("Qt", "custom")
        ],
        false,
    ),
    props: &[
        PropSpec::Text {
            key: "text",
            label: "Text",
            default: "cargo add martensite"
        },
        PropSpec::Header {
            label: "State & Accessibility"
        },
        PropSpec::Text {
            key: "a11y_label",
            label: "A11y label",
            default: ""
        },
    ],
    build: |p| {
        let mut __w = Copyable::new(p.str("text"));
        if !p.str("a11y_label").is_empty() {
            __w = __w.label(p.str("a11y_label"));
        }
        Box::new(__w)
    },
    snippet: |p| {
        let mut __s = format!("Copyable::new({:?})", p.str("text"));
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[("a11y_label", ".label", SnipProp::Text(""))],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(c) = downcast_mut::<Copyable>(w) {
            if let Some(text) = c.take_copied() {
                out.push(format!("copied \"{text}\""));
            }
        }
    },
});

page!(FlashcardPage {
    meta: meta(
        "Flashcard",
        "Misc",
        "Flip card — front/back memorization card.",
        "Group",
        &[
            ("Anki", "flashcard"),
            ("React", "flip card"),
            ("Qt", "custom"),
            ("iOS", "card flip")
        ],
        false,
    ),
    props: &[
        PropSpec::Text {
            key: "front",
            label: "Front",
            default: "Widget"
        },
        PropSpec::Text {
            key: "back",
            label: "Back",
            default: "Retained-mode UI element"
        },
        PropSpec::Bool {
            key: "flipped",
            label: "Flipped",
            default: false
        },
        PropSpec::Header {
            label: "State & Accessibility"
        },
        PropSpec::Text {
            key: "a11y_label",
            label: "A11y label",
            default: ""
        },
    ],
    build: |p| {
        let mut __w = Flashcard::new(p.str("front"), p.str("back"));
        if !p.str("a11y_label").is_empty() {
            __w = __w.label(p.str("a11y_label"));
        }
        if p.bool("flipped") {
            __w = __w.flipped(p.bool("flipped"));
        }
        Box::new(__w)
    },
    snippet: |p| {
        let mut __s = format!("Flashcard::new({:?}, {:?})", p.str("front"), p.str("back"));
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("flipped", ".flipped", SnipProp::Bool(false)),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(f) = downcast_mut::<Flashcard>(w) {
            if let Some(front) = f.take_flipped() {
                out.push(format!(
                    "flipped → {}",
                    if front { "front" } else { "back" }
                ));
            }
        }
    },
});

page!(FretboardPage {
    meta: meta(
        "Fretboard",
        "Misc",
        "Guitar fretboard — note/chord diagram.",
        "Canvas",
        &[
            ("Music", "fretboard"),
            ("Qt", "custom"),
            ("React", "fretboard"),
            ("Tabs", "diagram")
        ],
        false,
    ),
    props: &[
        PropSpec::Int {
            key: "mute",
            label: "Mute",
            min: 0,
            max: 100,
            default: 0
        },
        PropSpec::Header {
            label: "State & Accessibility"
        },
        PropSpec::Text {
            key: "a11y_label",
            label: "A11y label",
            default: ""
        },
    ],
    build: |_p| {
        let mut __w = Fretboard::new().set(0, 0).set(2, 2);
        if !_p.str("a11y_label").is_empty() {
            __w = __w.label(_p.str("a11y_label"));
        }
        if _p.i64("mute") != 0 {
            __w = __w.mute(_p.i64("mute") as u8);
        }
        Box::new(__w)
    },
    snippet: |_p| {
        let mut __s = "Fretboard::new().set(0, 0).set(2, 2)".to_string();
        __s.push_str(&crate::pages::prop_snippet(
            _p,
            &[
                ("mute", ".mute", SnipProp::Int(0)),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(f) = downcast_mut::<Fretboard>(w) {
            if let Some((string, fret)) = f.take_edited() {
                out.push(format!("string {string} fret {fret}"));
            }
        }
    },
});

page!(KeyboardShortcutsPage {
    meta: meta(
        "KeyboardShortcuts",
        "Misc",
        "Shortcut cheat-sheet — grouped key rows.",
        "List",
        &[
            ("VS Code", "keybindings"),
            ("macOS", "shortcut help"),
            ("React", "shortcuts"),
            ("Qt", "custom")
        ],
        false,
    ),
    props: &[
        PropSpec::Int {
            key: "columns",
            label: "Columns",
            min: 0,
            max: 100,
            default: 2
        },
        PropSpec::Header {
            label: "State & Accessibility"
        },
        PropSpec::Bool {
            key: "enabled",
            label: "Enabled",
            default: true
        },
        PropSpec::Text {
            key: "a11y_label",
            label: "A11y label",
            default: ""
        },
    ],
    build: |_p| {
        let mut __w = KeyboardShortcuts::new(vec![ShortcutGroup::new("General")
            .row("Save", "Ctrl+S")
            .row("Quit", "Ctrl+Q")]);
        __w = __w.enabled(_p.bool("enabled"));
        if !_p.str("a11y_label").is_empty() {
            __w = __w.a11y_label(_p.str("a11y_label"));
        }
        if _p.i64("columns") != 2 {
            __w = __w.columns(_p.i64("columns") as usize);
        }
        Box::new(__w)
    },
    snippet: |_p| {
        let mut __s = "KeyboardShortcuts::new(vec![ShortcutGroup::new(\"General\")])".to_string();
        __s.push_str(&crate::pages::prop_snippet(
            _p,
            &[
                ("columns", ".columns", SnipProp::Int(2)),
                ("enabled", ".enabled", SnipProp::Bool(true)),
                ("a11y_label", ".a11y_label", SnipProp::Text("")),
            ],
        ));
        __s
    },
});

page!(MetronomePage {
    meta: meta(
        "Metronome",
        "Misc",
        "Metronome — BPM, beats-per-bar, pendulum.",
        "Group",
        &[
            ("Music", "metronome"),
            ("Qt", "custom"),
            ("React", "metronome"),
            ("iOS", "metronome")
        ],
        false,
    ),
    props: &[
        PropSpec::Int {
            key: "bpm",
            label: "BPM",
            min: 30,
            max: 240,
            default: 100
        },
        PropSpec::Int {
            key: "beats",
            label: "Beats",
            min: 0,
            max: 100,
            default: 4
        },
        PropSpec::Header {
            label: "State & Accessibility"
        },
        PropSpec::Text {
            key: "a11y_label",
            label: "A11y label",
            default: ""
        },
    ],
    build: |p| {
        let mut __w = Metronome::new().bpm(p.i64("bpm") as u32);
        if !p.str("a11y_label").is_empty() {
            __w = __w.label(p.str("a11y_label"));
        }
        if p.i64("beats") != 4 {
            __w = __w.beats(p.i64("beats") as u8);
        }
        Box::new(__w)
    },
    snippet: |p| {
        let mut __s = format!("Metronome::new().bpm({})", p.i64("bpm"));
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("beats", ".beats", SnipProp::Int(4)),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(m) = downcast_mut::<Metronome>(w) {
            if let Some(beat) = m.take_beat() {
                out.push(format!("beat {beat}"));
            }
        }
    },
});

page!(PageFlipPage {
    meta: meta(
        "PageFlip",
        "Misc",
        "Book page-flip — left/right leaves.",
        "Group",
        &[
            ("iBooks", "page curl"),
            ("Qt", "custom"),
            ("React", "page flip"),
            ("Web", "flipbook")
        ],
        false,
    ),
    props: &[
        PropSpec::Int {
            key: "pages",
            label: "Pages",
            min: 2,
            max: 12,
            default: 4
        },
        PropSpec::Text {
            key: "page",
            label: "Page",
            default: ""
        },
        PropSpec::Int {
            key: "start",
            label: "Start",
            min: 0,
            max: 100,
            default: 0
        },
        PropSpec::Header {
            label: "State & Accessibility"
        },
        PropSpec::Text {
            key: "a11y_label",
            label: "A11y label",
            default: ""
        },
    ],
    build: |p| {
        let mut pf = PageFlip::new().pages((1..=p.i64("pages")).map(|i| format!("Page {i}")));
        pf.show_counter = true;
        {
            let mut __w = pf;
            if !p.str("a11y_label").is_empty() {
                __w = __w.label(p.str("a11y_label"));
            }
            if !p.str("page").is_empty() {
                __w = __w.page(p.str("page"));
            }
            if p.i64("start") != 0 {
                __w = __w.start(p.i64("start") as usize);
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = format!("PageFlip::new().pages(/* {} pages */)", p.i64("pages"));
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("page", ".page", SnipProp::Text("")),
                ("start", ".start", SnipProp::Int(0)),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(pf) = downcast_mut::<PageFlip>(w) {
            if let Some(i) = pf.take_turned() {
                out.push(format!("page {i}"));
            }
        }
    },
});

page!(PatternLockPage {
    meta: meta(
        "PatternLock",
        "Misc",
        "3×3 pattern-unlock grid.",
        "Grid",
        &[
            ("Android", "pattern lock"),
            ("Qt", "custom"),
            ("React", "pattern"),
            ("iOS", "custom")
        ],
        false,
    ),
    props: &[],
    build: |_p| {
        let mut l = PatternLock::new().label("Pattern");
        l.set_pattern(&[0, 1, 4, 6, 8]); // stage a drawn pattern
        Box::new(l)
    },
    snippet: |_p| "PatternLock::new()".to_string(),
    poll: |w, out| {
        if let Some(pl) = downcast_mut::<PatternLock>(w) {
            if let Some(pat) = pl.take_pattern() {
                out.push(format!("pattern → {pat:?}"));
            }
        }
    },
});

page!(PianoKeysPage {
    meta: meta(
        "PianoKeys",
        "Misc",
        "Piano keyboard — white/black keys.",
        "Canvas",
        &[
            ("Music", "piano"),
            ("Qt", "custom"),
            ("React", "piano"),
            ("DAW", "keys")
        ],
        false,
    ),
    props: &[
        PropSpec::Int {
            key: "octaves",
            label: "Octaves",
            min: 1,
            max: 4,
            default: 2
        },
        PropSpec::Header {
            label: "State & Accessibility"
        },
        PropSpec::Text {
            key: "a11y_label",
            label: "A11y label",
            default: ""
        },
    ],
    build: |p| {
        let mut __w = PianoKeys::new().octaves(p.i64("octaves") as usize);
        if !p.str("a11y_label").is_empty() {
            __w = __w.label(p.str("a11y_label"));
        }
        Box::new(__w)
    },
    snippet: |p| {
        let mut __s = format!("PianoKeys::new().octaves({})", p.i64("octaves"));
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[("a11y_label", ".label", SnipProp::Text(""))],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(pk) = downcast_mut::<PianoKeys>(w) {
            if let Some(note) = pk.take_struck() {
                out.push(format!("note {note}"));
            }
        }
    },
});

page!(PipsPagerPage {
    meta: meta(
        "PipsPager",
        "Misc",
        "Page-dot indicator — carousel pagination.",
        "Navigation",
        &[
            ("iOS", "page dots"),
            ("Android", "pager dots"),
            ("React", "pips"),
            ("Qt", "custom")
        ],
        false,
    ),
    props: &[
        PropSpec::Int {
            key: "count",
            label: "Pages",
            min: 2,
            max: 12,
            default: 5
        },
        PropSpec::Int {
            key: "max_visible",
            label: "Max Visible",
            min: 0,
            max: 32,
            default: 0
        },
        PropSpec::Header {
            label: "State & Accessibility"
        },
        PropSpec::Bool {
            key: "enabled",
            label: "Enabled",
            default: true
        },
        PropSpec::Text {
            key: "a11y_label",
            label: "A11y label",
            default: ""
        },
    ],
    build: |p| {
        let mut pg = PipsPager::new(p.i64("count") as usize);
        pg.set_current(1);
        {
            let mut __w = pg;
            __w = __w.enabled(p.bool("enabled"));
            if !p.str("a11y_label").is_empty() {
                __w = __w.a11y_label(p.str("a11y_label"));
            }
            if p.i64("max_visible") != 0 {
                __w = __w.max_visible(p.i64("max_visible") as usize);
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = format!("PipsPager::new({})", p.i64("count"));
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("max_visible", ".max_visible", SnipProp::Int(0)),
                ("enabled", ".enabled", SnipProp::Bool(true)),
                ("a11y_label", ".a11y_label", SnipProp::Text("")),
            ],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(pg) = downcast_mut::<PipsPager>(w) {
            if let Some(i) = pg.take_selected() {
                out.push(format!("pip {i}"));
            }
        }
    },
});

page!(PricingTablePage {
    meta: meta(
        "PricingTable",
        "Misc",
        "Plan comparison — features, CTA, recommended.",
        "Table",
        &[
            ("Web", "pricing"),
            ("Stripe", "plans"),
            ("React", "pricing table"),
            ("Qt", "custom")
        ],
        false,
    ),
    props: &[
        PropSpec::Header {
            label: "State & Accessibility"
        },
        PropSpec::Text {
            key: "a11y_label",
            label: "A11y label",
            default: ""
        },
    ],
    build: |_p| {
        let mut __w = PricingTable::new()
            .plan(
                Plan::new("Free", "$0")
                    .feature("1 project")
                    .feature("Community support"),
            )
            .plan(
                Plan::new("Pro", "$12")
                    .feature("Unlimited projects")
                    .feature("Priority support")
                    .recommended(),
            );
        if !_p.str("a11y_label").is_empty() {
            __w = __w.label(_p.str("a11y_label"));
        }
        Box::new(__w)
    },
    snippet: |_p| {
        let mut __s = "PricingTable::new().plan(Plan::new(\"Pro\", \"$12\"))".to_string();
        __s.push_str(&crate::pages::prop_snippet(
            _p,
            &[("a11y_label", ".label", SnipProp::Text(""))],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(pt) = downcast_mut::<PricingTable>(w) {
            if let Some(i) = pt.take_chosen() {
                out.push(format!("plan {i}"));
            }
        }
    },
});

page!(PullToRefreshPage {
    meta: meta(
        "PullToRefresh",
        "Misc",
        "Pull-down-to-refresh wrapper.",
        "ScrollArea",
        &[
            ("iOS", "UIRefreshControl"),
            ("Android", "SwipeRefresh"),
            ("React", "pull refresh"),
            ("Qt", "custom")
        ],
        false,
    ),
    props: &[
        PropSpec::Header {
            label: "State & Accessibility"
        },
        PropSpec::Bool {
            key: "enabled",
            label: "Enabled",
            default: true
        },
        PropSpec::Text {
            key: "a11y_label",
            label: "A11y label",
            default: ""
        },
    ],
    build: |_p| {
        let mut __w = PullToRefresh::new(Text::new("Pull down to refresh"));
        __w = __w.enabled(_p.bool("enabled"));
        if !_p.str("a11y_label").is_empty() {
            __w = __w.label(_p.str("a11y_label"));
        }
        __w.set_pull(56.0); // stage mid-pull for a static snapshot
        Box::new(__w)
    },
    snippet: |_p| {
        let mut __s = "PullToRefresh::new(list)".to_string();
        __s.push_str(&crate::pages::prop_snippet(
            _p,
            &[
                ("enabled", ".enabled", SnipProp::Bool(true)),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        __s
    },
    poll: |w, out| {
        if downcast_mut::<PullToRefresh>(w).is_some_and(|pr| pr.take_refresh()) {
            out.push("refresh".to_string());
        }
    },
});

page!(RatingSummaryPage {
    meta: meta(
        "RatingSummary",
        "Misc",
        "Rating histogram — average + per-star counts.",
        "Chart",
        &[
            ("App Store", "ratings"),
            ("Amazon", "rating bars"),
            ("React", "rating summary"),
            ("Qt", "custom")
        ],
        false,
    ),
    props: &[
        PropSpec::Header {
            label: "State & Accessibility"
        },
        PropSpec::Text {
            key: "a11y_label",
            label: "A11y label",
            default: ""
        },
    ],
    build: |_p| {
        let mut rs = RatingSummary::new();
        rs = rs.counts([40, 25, 15, 8, 4]);
        {
            let mut __w = rs;
            if !_p.str("a11y_label").is_empty() {
                __w = __w.label(_p.str("a11y_label"));
            }
            Box::new(__w)
        }
    },
    snippet: |_p| {
        let mut __s = "RatingSummary::new().counts([40, 25, 15, 8, 4])".to_string();
        __s.push_str(&crate::pages::prop_snippet(
            _p,
            &[("a11y_label", ".label", SnipProp::Text(""))],
        ));
        __s
    },
});

page!(ScratchCardPage {
    meta: meta(
        "ScratchCard",
        "Misc",
        "Scratch-off foil revealing hidden content.",
        "Canvas",
        &[
            ("Lottery", "scratch card"),
            ("iOS", "scratch"),
            ("React", "scratch off"),
            ("Qt", "custom")
        ],
        false,
    ),
    props: &[
        PropSpec::Float {
            key: "reveal_threshold",
            label: "Reveal Threshold",
            min: -9.1,
            max: 100.0,
            step: 1.0,
            default: 0.6
        },
        PropSpec::Header {
            label: "State & Accessibility"
        },
        PropSpec::Text {
            key: "a11y_label",
            label: "A11y label",
            default: ""
        },
    ],
    build: |_p| {
        let mut __w = ScratchCard::new(Text::new("You won a widget!"));
        if !_p.str("a11y_label").is_empty() {
            __w = __w.label(_p.str("a11y_label"));
        }
        if _p.f64("reveal_threshold") != 0.6 {
            __w = __w.reveal_threshold(_p.f64("reveal_threshold") as f32);
        }
        Box::new(__w)
    },
    snippet: |_p| {
        let mut __s = "ScratchCard::new(Text::new(\"…\"))".to_string();
        __s.push_str(&crate::pages::prop_snippet(
            _p,
            &[
                (
                    "reveal_threshold",
                    ".reveal_threshold",
                    SnipProp::Float(0.6),
                ),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        __s
    },
    poll: |w, out| {
        if downcast_mut::<ScratchCard>(w).is_some_and(|s| s.take_revealed()) {
            out.push("revealed".to_string());
        }
    },
});

page!(StopwatchPage {
    meta: meta(
        "Stopwatch",
        "Misc",
        "Stopwatch — lap list, running state.",
        "Group",
        &[
            ("iOS", "stopwatch"),
            ("Qt", "custom"),
            ("React", "stopwatch"),
            ("Web", "timer")
        ],
        false,
    ),
    props: &[
        PropSpec::Bool {
            key: "running",
            label: "Running",
            default: false
        },
        PropSpec::Header {
            label: "State & Accessibility"
        },
        PropSpec::Text {
            key: "a11y_label",
            label: "A11y label",
            default: ""
        },
    ],
    build: |_p| {
        let mut s = Stopwatch::new();
        s.start();
        {
            let mut __w = s;
            if !_p.str("a11y_label").is_empty() {
                __w = __w.label(_p.str("a11y_label"));
            }
            if _p.bool("running") {
                __w = __w.running(_p.bool("running"));
            }
            Box::new(__w)
        }
    },
    snippet: |_p| {
        let mut __s = "let mut s = Stopwatch::new();\ns.start();".to_string();
        __s.push_str(&crate::pages::prop_snippet(
            _p,
            &[
                ("running", ".running", SnipProp::Bool(false)),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(s) = downcast_mut::<Stopwatch>(w) {
            if let Some(lap) = s.take_lapped() {
                out.push(format!("lap {lap:?}"));
            }
        }
    },
});

page!(TicketPage {
    meta: meta(
        "Ticket",
        "Misc",
        "Perforated ticket — title, fields, tear stub.",
        "Group",
        &[
            ("Event", "ticket"),
            ("Airline", "boarding pass"),
            ("React", "ticket"),
            ("Qt", "custom")
        ],
        false,
    ),
    props: &[
        PropSpec::Text {
            key: "title",
            label: "Title",
            default: "Admit One"
        },
        PropSpec::Text {
            key: "field_label",
            label: "Field Label",
            default: ""
        },
        PropSpec::Text {
            key: "field_value",
            label: "Field Value",
            default: ""
        },
        PropSpec::Header {
            label: "State & Accessibility"
        },
        PropSpec::Text {
            key: "a11y_label",
            label: "A11y label",
            default: ""
        },
    ],
    build: |p| {
        let mut __w = Ticket::new(p.str("title"))
            .caption("Row A · Seat 12")
            .code("MR-4822");
        if !p.str("a11y_label").is_empty() {
            __w = __w.label(p.str("a11y_label"));
        }
        if !p.str("field_label").is_empty() || !p.str("field_value").is_empty() {
            __w = __w.field(p.str("field_label"), p.str("field_value"));
        }
        Box::new(__w)
    },
    snippet: |p| {
        let mut __s = format!("Ticket::new({:?}).code(\"MR-4822\")", p.str("title"));
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[("a11y_label", ".label", SnipProp::Text(""))],
        ));
        if !p.str("field_label").is_empty() || !p.str("field_value").is_empty() {
            __s.push_str(&format!(
                "\n    .field({:?}, {:?})",
                p.str("field_label"),
                p.str("field_value")
            ));
        }
        __s
    },
    poll: |w, out| {
        if downcast_mut::<Ticket>(w).is_some_and(|t| t.take_torn()) {
            out.push("torn".to_string());
        }
    },
});

page!(VirtualKeyboardPage {
    meta: meta(
        "VirtualKeyboard",
        "Misc",
        "On-screen keyboard — tap keys.",
        "Keyboard",
        &[
            ("iOS", "keyboard"),
            ("Android", "IME"),
            ("Qt", "virtual keyboard"),
            ("React", "osk")
        ],
        false,
    ),
    props: &[
        PropSpec::Bool {
            key: "shift",
            label: "Shift",
            default: false
        },
        PropSpec::Bool {
            key: "shifted",
            label: "Shifted",
            default: false
        },
        PropSpec::Header {
            label: "State & Accessibility"
        },
        PropSpec::Text {
            key: "a11y_label",
            label: "A11y label",
            default: ""
        },
    ],
    build: |p| {
        let mut kb = VirtualKeyboard::new();
        if p.bool("shift") {
            kb.set_shift(true);
        }
        {
            let mut __w = kb;
            if !p.str("a11y_label").is_empty() {
                __w = __w.label(p.str("a11y_label"));
            }
            if p.bool("shifted") {
                __w = __w.shifted(p.bool("shifted"));
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = format!("VirtualKeyboard::new().set_shift({})", p.bool("shift"));
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("shifted", ".shifted", SnipProp::Bool(false)),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(kb) = downcast_mut::<VirtualKeyboard>(w) {
            if let Some(key) = kb.take_pressed() {
                out.push(format!("key → {key}"));
            }
        }
    },
});

page!(WizardPage {
    meta: meta(
        "Wizard",
        "Misc",
        "Step-by-step wizard — back/next/finish.",
        "Dialog",
        &[
            ("Qt", "QWizard"),
            ("Win32", "wizard"),
            ("React", "stepper form"),
            ("GNOME", "assistant")
        ],
        false,
    ),
    props: &[
        PropSpec::Text {
            key: "finish",
            label: "Finish label",
            default: "Done"
        },
        PropSpec::Header {
            label: "State & Accessibility"
        },
        PropSpec::Bool {
            key: "enabled",
            label: "Enabled",
            default: true
        },
        PropSpec::Text {
            key: "a11y_label",
            label: "A11y label",
            default: ""
        },
    ],
    build: |p| {
        {
            let mut __w = Wizard::new()
                .finish_text(p.str("finish"))
                .cancelable(true)
                .step("Welcome", Text::new("Step 1 — welcome"))
                .step("Options", Text::new("Step 2 — options"));
            __w = __w.enabled(p.bool("enabled"));
            if !p.str("a11y_label").is_empty() {
                __w = __w.label(p.str("a11y_label"));
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = format!("Wizard::new().finish_text({:?})", p.str("finish"));
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("enabled", ".enabled", SnipProp::Bool(true)),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(wz) = downcast_mut::<Wizard>(w) {
            if wz.take_finished() {
                out.push("finished".to_string());
            }
            if wz.take_cancelled() {
                out.push("cancelled".to_string());
            }
        }
    },
});

page!(WordCloudPage {
    meta: meta(
        "WordCloud",
        "Misc",
        "Word-frequency cloud — size encodes weight.",
        "Canvas",
        &[
            ("Web", "word cloud"),
            ("D3", "cloud"),
            ("React", "word cloud"),
            ("Qt", "custom")
        ],
        false,
    ),
    props: &[
        PropSpec::Header {
            label: "State & Accessibility"
        },
        PropSpec::Text {
            key: "a11y_label",
            label: "A11y label",
            default: ""
        },
    ],
    build: |_p| {
        let mut __w = WordCloud::new()
            .word("widgets", 1.0)
            .word("layout", 0.7)
            .word("paint", 0.5)
            .word("events", 0.4);
        if !_p.str("a11y_label").is_empty() {
            __w = __w.label(_p.str("a11y_label"));
        }
        Box::new(__w)
    },
    snippet: |_p| {
        let mut __s = "WordCloud::new().word(\"widgets\", 1.0)".to_string();
        __s.push_str(&crate::pages::prop_snippet(
            _p,
            &[("a11y_label", ".label", SnipProp::Text(""))],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(wc) = downcast_mut::<WordCloud>(w) {
            if let Some(i) = wc.take_clicked() {
                out.push(format!("word {i}"));
            }
        }
    },
});

page!(WorldClockPage {
    meta: meta(
        "WorldClock",
        "Misc",
        "Multiple timezone clocks.",
        "List",
        &[
            ("iOS", "world clock"),
            ("macOS", "world clock"),
            ("React", "timezones"),
            ("Qt", "custom")
        ],
        false,
    ),
    props: &[
        PropSpec::Text {
            key: "with_utc",
            label: "With Utc",
            default: ""
        },
        PropSpec::Header {
            label: "State & Accessibility"
        },
        PropSpec::Text {
            key: "a11y_label",
            label: "A11y label",
            default: ""
        },
    ],
    build: |_p| {
        let mut __w = WorldClock::new()
            .zone("Lisbon", 60)
            .zone("Tokyo", 540)
            .zone("New York", -300);
        if !_p.str("a11y_label").is_empty() {
            __w = __w.label(_p.str("a11y_label"));
        }
        if let Some(v) = crate::pages::parse_time(_p.str("with_utc")) {
            __w = __w.with_utc(v);
        }
        Box::new(__w)
    },
    snippet: |_p| {
        let mut __s = "WorldClock::new().zone(\"Lisbon\", 60).zone(\"Tokyo\", 540)".to_string();
        __s.push_str(&crate::pages::prop_snippet(
            _p,
            &[("a11y_label", ".label", SnipProp::Text(""))],
        ));
        __s.push_str(&crate::pages::snip_textmap(
            _p,
            "with_utc",
            ".with_utc",
            "",
            crate::pages::expr_time,
        ));
        __s
    },
});

/// All Misc pages, in rail order.
pub(super) fn pages() -> Vec<Box<dyn Page>> {
    vec![
        Box::new(AccordionPage),
        Box::new(AddressBarPage),
        Box::new(CalendarPage),
        Box::new(DatePickerPage),
        Box::new(TimePickerPage),
        Box::new(WeekViewPage),
        Box::new(AvatarPage),
        Box::new(AvatarGroupPage),
        Box::new(MessageListPage),
        Box::new(CommentThreadPage),
        Box::new(SocialCardPage),
        Box::new(ReactionBarPage),
        Box::new(PollPage),
        Box::new(PresencePage),
        Box::new(AttendeeListPage),
        Box::new(VideoGridPage),
        Box::new(CallControlsPage),
        Box::new(WaitingRoomPage),
        Box::new(BreakoutRoomsPage),
        Box::new(EmojiPickerPage),
        Box::new(CarouselPage),
        Box::new(CardDeckPage),
        Box::new(CascaderPage),
        Box::new(ChessBoardPage),
        Box::new(ChessClockPage),
        Box::new(ConfettiPage),
        Box::new(CopyablePage),
        Box::new(FlashcardPage),
        Box::new(FretboardPage),
        Box::new(KeyboardShortcutsPage),
        Box::new(MetronomePage),
        Box::new(PageFlipPage),
        Box::new(PatternLockPage),
        Box::new(PianoKeysPage),
        Box::new(PipsPagerPage),
        Box::new(PricingTablePage),
        Box::new(PullToRefreshPage),
        Box::new(RatingSummaryPage),
        Box::new(ScratchCardPage),
        Box::new(StopwatchPage),
        Box::new(TicketPage),
        Box::new(VirtualKeyboardPage),
        Box::new(WizardPage),
        Box::new(WordCloudPage),
        Box::new(WorldClockPage),
    ]
}
