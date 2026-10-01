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
use crate::pages::{downcast_mut, meta, page};

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
    props: &[PropSpec::Bool {
        key: "multi",
        label: "Allow multiple",
        default: false
    }],
    build: |p| {
        let mut a = Accordion::new().allow_multiple(p.bool("multi"));
        for title in ["Profile", "Security", "Advanced"] {
            a = a.section(title, Text::new(format!("{title} content")));
        }
        Box::new(a)
    },
    snippet: |p| format!(
        "Accordion::new()\n    .allow_multiple({})\n    .section(\"Profile\", content)",
        p.bool("multi"),
    ),
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
    props: &[PropSpec::Text {
        key: "url",
        label: "URL",
        default: "https://example.com/docs",
    }],
    build: |p| Box::new(AddressBar::new(p.str("url"))),
    snippet: |p| format!("AddressBar::new({:?})", p.str("url")),
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
    props: &[PropSpec::Bool {
        key: "monday",
        label: "Week starts Mon",
        default: true
    }],
    build: |p| Box::new(Calendar::new().week_starts_monday(p.bool("monday"))),
    snippet: |p| format!("Calendar::new().week_starts_monday({})", p.bool("monday")),
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
    props: &[PropSpec::Text {
        key: "label",
        label: "Label",
        default: "Due date"
    }],
    build: |p| {
        let mut d = DatePicker::new().label(p.str("label"));
        d.open();
        Box::new(d)
    },
    snippet: |p| format!("DatePicker::new().label({:?})", p.str("label")),
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
    props: &[PropSpec::Bool {
        key: "use24",
        label: "24-hour",
        default: true
    }],
    build: |p| Box::new(TimePicker::new().use_24h(p.bool("use24")).label("Time")),
    snippet: |p| format!("TimePicker::new().use_24h({})", p.bool("use24")),
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
    props: &[],
    build: |_p| {
        let mut wv = WeekView::new();
        wv = wv.event(WeekEvent::new("Standup", 1, 9.0, 9.5));
        wv = wv.event(WeekEvent::new("Review", 3, 14.0, 15.5));
        Box::new(wv)
    },
    snippet: |_p| "WeekView::new().event(WeekEvent::new(\"Standup\", 1, 9.0, 9.5))".to_string(),
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
    props: &[PropSpec::Int {
        key: "max",
        label: "Max visible",
        min: 1,
        max: 8,
        default: 3
    }],
    build: |p| {
        let mut g = AvatarGroup::new().max_count(p.i64("max") as usize);
        for name in ["Ada", "Grace", "Linus", "Alan", "Edsger"] {
            g = g.member(Avatar::new(name));
        }
        Box::new(g)
    },
    snippet: |p| format!("AvatarGroup::new().max_count({})", p.i64("max")),
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
    props: &[],
    build: |_p| {
        let mut ml = MessageList::new();
        ml.push(Message::received("Ada", "Did the catalog ship?"));
        ml.push(Message::sent("Just merged."));
        ml.push(Message::received("Ada", "Nice — screenshots?"));
        Box::new(ml)
    },
    snippet: |_p| "MessageList::new() /* push(Message::received(…)) */".to_string(),
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
    props: &[],
    build: |_p| {
        Box::new(
            CommentThread::new()
                .comment(Comment::new(1, "ada", "2h ago", "This layout works well."))
                .comment(Comment::new(
                    2,
                    "grace",
                    "1h ago",
                    "Agreed — the RTL seam too.",
                )),
        )
    },
    snippet: |_p| "CommentThread::new().comment(Comment::new(1, \"ada\", \"2h\", \"…\"))"
        .to_string(),
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
    props: &[PropSpec::Text {
        key: "body",
        label: "Body",
        default: "Widgets are fun."
    }],
    build: |p| Box::new(SocialCard::new("ada", "@ada", "2h", p.str("body")),),
    snippet: |p| format!(
        "SocialCard::new(\"ada\", \"@ada\", \"2h\", {:?})",
        p.str("body")
    ),
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
    props: &[],
    build: |_p| Box::new(
        ReactionBar::new()
            .reaction(Reaction::new("👍", 4))
            .reaction(Reaction::new("🎉", 2))
            .reaction(Reaction::new("🚀", 7)),
    ),
    snippet: |_p| "ReactionBar::new().reaction(Reaction::new(\"👍\", 4))".to_string(),
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
    props: &[PropSpec::Text {
        key: "question",
        label: "Question",
        default: "Ship it?"
    }],
    build: |p| {
        let mut pl = Poll::new(p.str("question"));
        pl = pl.option(PollOption::new("Yes", 12));
        pl = pl.option(PollOption::new("No", 3));
        Box::new(pl)
    },
    snippet: |p| format!(
        "Poll::new({:?}).option(PollOption::new(\"Yes\", 12))",
        p.str("question")
    ),
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
    props: &[PropSpec::Choice {
        key: "status",
        label: "Status",
        options: &["Online", "Away", "Busy", "Offline"],
        default: 0,
    }],
    build: |p| {
        let st = match p.choice("status") {
            1 => PresenceStatus::Away,
            2 => PresenceStatus::Busy,
            3 => PresenceStatus::Offline,
            _ => PresenceStatus::Online,
        };
        Box::new(Presence::new("Ada", st).show_text(true))
    },
    snippet: |p| format!(
        "Presence::new(\"Ada\", PresenceStatus::{})",
        ["Online", "Away", "Busy", "Offline"][p.choice("status")],
    ),
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
    props: &[],
    build: |_p| {
        let mut al = AttendeeList::new();
        al = al.attendee(Attendee::new("Ada"));
        al = al.attendee(Attendee::new("Grace"));
        Box::new(al)
    },
    snippet: |_p| "AttendeeList::new().attendee(Attendee::new(\"Ada\"))".to_string(),
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
    props: &[PropSpec::Int {
        key: "count",
        label: "Participants",
        min: 1,
        max: 9,
        default: 4
    }],
    build: |p| {
        let mut vg = VideoGrid::new();
        let colors = [
            [80, 140, 255, 255],
            [240, 90, 160, 255],
            [90, 200, 120, 255],
        ];
        for i in 0..p.i64("count") as usize {
            vg = vg.participant(Participant::new(format!("P{}", i + 1), colors[i % 3]));
        }
        Box::new(vg)
    },
    snippet: |p| format!("VideoGrid::new() /* {} participants */", p.i64("count")),
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
    props: &[PropSpec::Text {
        key: "title",
        label: "Title",
        default: "Waiting room"
    }],
    build: |p| {
        let mut wr = WaitingRoom::new().title(p.str("title"));
        wr.queue("Ada");
        wr.queue("Grace");
        Box::new(wr)
    },
    snippet: |p| format!(
        "WaitingRoom::new().title({:?}) /* queue(\"Ada\") */",
        p.str("title")
    ),
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
    props: &[],
    build: |_p| {
        let mut br = BreakoutRooms::new();
        br = br.room(Room::new("Design", 3));
        br = br.room(Room::new("Backend", 5));
        Box::new(br)
    },
    snippet: |_p| "BreakoutRooms::new().room(Room::new(\"Design\", 3))".to_string(),
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
    props: &[],
    build: |_p| Box::new(
        EmojiPicker::new()
            .section(
                "Smileys",
                [("😀", "grinning"), ("😉", "wink"), ("😂", "joy")]
            )
            .section("Objects", [("🚀", "rocket"), ("📦", "package")]),
    ),
    snippet: |_p| "EmojiPicker::new().section(\"Smileys\", [(\"😀\", \"grinning\")])".to_string(),
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
    props: &[PropSpec::Int {
        key: "pages",
        label: "Pages",
        min: 2,
        max: 8,
        default: 3
    }],
    build: |p| {
        let mut c = Carousel::new();
        for i in 0..p.i64("pages") {
            c = c.page(Text::new(format!("Slide {}", i + 1)));
        }
        Box::new(c)
    },
    snippet: |p| format!("Carousel::new() /* {} pages */", p.i64("pages")),
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
    props: &[],
    build: |_p| {
        let mut d = CardDeck::new();
        for label in ["Top card", "Middle card", "Bottom card"] {
            d = d.card(Text::new(label));
        }
        Box::new(d)
    },
    snippet: |_p| "CardDeck::new().card(Text::new(\"Top card\"))".to_string(),
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
    props: &[PropSpec::Text {
        key: "label",
        label: "Label",
        default: "Region"
    }],
    build: |p| {
        Box::new(Cascader::new().label(p.str("label")).options(vec![
                    CascaderOption::new("Europe", "eu")
                        .child(CascaderOption::new("Portugal", "pt"))
                        .child(CascaderOption::new("Spain", "es")),
                    CascaderOption::new("Asia", "as")
                        .child(CascaderOption::new("Japan", "jp")),
                ]))
    },
    snippet: |p| format!("Cascader::new().label({:?})", p.str("label")),
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
    props: &[PropSpec::Bool {
        key: "flipped",
        label: "Flipped",
        default: false
    }],
    build: |p| {
        let mut b = ChessBoard::new().coordinates(true);
        b.reset();
        if p.bool("flipped") {
            // flip via flag when supported
        }
        Box::new(b)
    },
    snippet: |p| format!(
        "ChessBoard::new().coordinates(true) /* flipped={} */",
        p.bool("flipped")
    ),
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
    props: &[PropSpec::Int {
        key: "mins",
        label: "Minutes",
        min: 1,
        max: 30,
        default: 5
    }],
    build: |p| Box::new(ChessClock::new(Duration::from_secs(
        p.i64("mins") as u64 * 60
    ))),
    snippet: |p| format!(
        "ChessClock::new(Duration::from_secs({}))",
        p.i64("mins") * 60
    ),
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
    props: &[PropSpec::Int {
        key: "count",
        label: "Particles",
        min: 10,
        max: 400,
        default: 120
    }],
    build: |p| {
        let mut c = Confetti::new().count(p.i64("count") as usize);
        c.burst(0.5, 0.3);
        Box::new(c)
    },
    snippet: |p| format!("Confetti::new().count({})", p.i64("count")),
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
    props: &[PropSpec::Text {
        key: "text",
        label: "Text",
        default: "cargo add martensite"
    }],
    build: |p| Box::new(Copyable::new(p.str("text"))),
    snippet: |p| format!("Copyable::new({:?})", p.str("text")),
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
    ],
    build: |p| Box::new(Flashcard::new(p.str("front"), p.str("back"))),
    snippet: |p| format!("Flashcard::new({:?}, {:?})", p.str("front"), p.str("back")),
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
    props: &[],
    build: |_p| Box::new(Fretboard::new().set(0, 0).set(2, 2)),
    snippet: |_p| "Fretboard::new().set(0, 0).set(2, 2)".to_string(),
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
    props: &[],
    build: |_p| Box::new(KeyboardShortcuts::new(vec![ShortcutGroup::new("General")
        .row("Save", "Ctrl+S")
        .row("Quit", "Ctrl+Q"),])),
    snippet: |_p| "KeyboardShortcuts::new(vec![ShortcutGroup::new(\"General\")])".to_string(),
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
    props: &[PropSpec::Int {
        key: "bpm",
        label: "BPM",
        min: 30,
        max: 240,
        default: 100
    }],
    build: |p| Box::new(Metronome::new().bpm(p.i64("bpm") as u32)),
    snippet: |p| format!("Metronome::new().bpm({})", p.i64("bpm")),
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
    props: &[PropSpec::Int {
        key: "pages",
        label: "Pages",
        min: 2,
        max: 12,
        default: 4
    }],
    build: |p| {
        let mut pf = PageFlip::new().pages((1..=p.i64("pages")).map(|i| format!("Page {i}")));
        pf.show_counter = true;
        Box::new(pf)
    },
    snippet: |p| format!("PageFlip::new().pages(/* {} pages */)", p.i64("pages")),
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
    build: |_p| Box::new(PatternLock::new().label("Pattern")),
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
    props: &[PropSpec::Int {
        key: "octaves",
        label: "Octaves",
        min: 1,
        max: 4,
        default: 2
    }],
    build: |p| Box::new(PianoKeys::new().octaves(p.i64("octaves") as usize)),
    snippet: |p| format!("PianoKeys::new().octaves({})", p.i64("octaves")),
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
    props: &[PropSpec::Int {
        key: "count",
        label: "Pages",
        min: 2,
        max: 12,
        default: 5
    }],
    build: |p| {
        let mut pg = PipsPager::new(p.i64("count") as usize);
        pg.set_current(1);
        Box::new(pg)
    },
    snippet: |p| format!("PipsPager::new({})", p.i64("count")),
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
    props: &[],
    build: |_p| Box::new(
        PricingTable::new()
            .plan(
                Plan::new("Free", "$0")
                    .feature("1 project")
                    .feature("Community support")
            )
            .plan(
                Plan::new("Pro", "$12")
                    .feature("Unlimited projects")
                    .feature("Priority support")
                    .recommended()
            ),
    ),
    snippet: |_p| "PricingTable::new().plan(Plan::new(\"Pro\", \"$12\"))".to_string(),
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
    props: &[],
    build: |_p| Box::new(PullToRefresh::new(Text::new("Pull down to refresh"))),
    snippet: |_p| "PullToRefresh::new(list)".to_string(),
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
    props: &[],
    build: |_p| {
        let mut rs = RatingSummary::new();
        rs = rs.counts([40, 25, 15, 8, 4]);
        Box::new(rs)
    },
    snippet: |_p| "RatingSummary::new().counts([40, 25, 15, 8, 4])".to_string(),
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
    props: &[],
    build: |_p| Box::new(ScratchCard::new(Text::new("You won a widget!"))),
    snippet: |_p| "ScratchCard::new(Text::new(\"…\"))".to_string(),
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
    props: &[],
    build: |_p| {
        let mut s = Stopwatch::new();
        s.start();
        Box::new(s)
    },
    snippet: |_p| "let mut s = Stopwatch::new();\ns.start();".to_string(),
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
    props: &[PropSpec::Text {
        key: "title",
        label: "Title",
        default: "Admit One"
    }],
    build: |p| Box::new(
        Ticket::new(p.str("title"))
            .caption("Row A · Seat 12")
            .code("MR-4822"),
    ),
    snippet: |p| format!("Ticket::new({:?}).code(\"MR-4822\")", p.str("title")),
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
    props: &[PropSpec::Bool {
        key: "shift",
        label: "Shift",
        default: false
    }],
    build: |p| {
        let mut kb = VirtualKeyboard::new();
        if p.bool("shift") {
            kb.set_shift(true);
        }
        Box::new(kb)
    },
    snippet: |p| format!("VirtualKeyboard::new().set_shift({})", p.bool("shift")),
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
    props: &[PropSpec::Text {
        key: "finish",
        label: "Finish label",
        default: "Done"
    }],
    build: |p| {
        Box::new(
            Wizard::new()
                .finish_text(p.str("finish"))
                .cancelable(true)
                .step("Welcome", Text::new("Step 1 — welcome"))
                .step("Options", Text::new("Step 2 — options")),
        )
    },
    snippet: |p| format!("Wizard::new().finish_text({:?})", p.str("finish")),
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
    props: &[],
    build: |_p| Box::new(
        WordCloud::new()
            .word("widgets", 1.0)
            .word("layout", 0.7)
            .word("paint", 0.5)
            .word("events", 0.4),
    ),
    snippet: |_p| "WordCloud::new().word(\"widgets\", 1.0)".to_string(),
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
    props: &[],
    build: |_p| Box::new(
        WorldClock::new()
            .zone("Lisbon", 60)
            .zone("Tokyo", 540)
            .zone("New York", -300),
    ),
    snippet: |_p| "WorldClock::new().zone(\"Lisbon\", 60).zone(\"Tokyo\", 540)".to_string(),
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
