//! Media & social category — one live card per assigned widget
//! module. Every construction is adapted from the widget's own `///`
//! doctest (the verified canonical example in its source file).

use std::time::Duration;

use martensite::core::Widget;
use martensite::widgets::attendee_list::{Attendee, AttendeeList};
use martensite::widgets::breakout_rooms::{BreakoutRooms, Room};
use martensite::widgets::call_controls::CallControls;
use martensite::widgets::captions::Captions;
use martensite::widgets::chat_input::ChatInput;
use martensite::widgets::chess_board::ChessBoard;
use martensite::widgets::chess_clock::ChessClock;
use martensite::widgets::comment_thread::{Comment, CommentThread};
use martensite::widgets::control_center::ControlCenter;
use martensite::widgets::device_picker::{DeviceKind, DevicePicker};
use martensite::widgets::dial::Dial;
use martensite::widgets::emoji_picker::EmojiPicker;
use martensite::widgets::equalizer::Equalizer;
use martensite::widgets::fretboard::Fretboard;
use martensite::widgets::media_controls::MediaControls;
use martensite::widgets::metronome::Metronome;
use martensite::widgets::now_playing::NowPlaying;
use martensite::widgets::pad_grid::PadGrid;
use martensite::widgets::piano_keys::PianoKeys;
use martensite::widgets::presence::PresenceStatus;
use martensite::widgets::pricing_table::{Plan, PricingTable};
use martensite::widgets::reaction_bar::{Reaction, ReactionBar};
use martensite::widgets::release_notes::{ChangeKind, Release, ReleaseNotes};
use martensite::widgets::social_card::{CardAction, SocialCard};
use martensite::widgets::spectrum::Spectrum;
use martensite::widgets::step_sequencer::StepSequencer;
use martensite::widgets::theme_picker::{ThemeOption, ThemePicker};
use martensite::widgets::ticket::Ticket;
use martensite::widgets::tuner::Tuner;
use martensite::widgets::volume::Volume;
use martensite::widgets::waiting_room::WaitingRoom;
use martensite::widgets::waveform::Waveform;

/// Media & social showcase entries — `(display name, live widget)`.
pub fn entries() -> Vec<(&'static str, Box<dyn Widget>)> {
    vec![
        (
            "Attendee List",
            Box::new(
                AttendeeList::new()
                    .attendee(Attendee::new("I. Chen").speaking(true))
                    .attendee(Attendee::new("T. Alvarez").muted(true))
                    .attendee(Attendee::new("S. Wu").hand_raised(true))
                    .attendee(Attendee::new("R. Okafor").status(PresenceStatus::Away))
                    .label("Shift huddle"),
            ),
        ),
        (
            "Breakout Rooms",
            Box::new(
                BreakoutRooms::new()
                    .room(Room::new("Safety review", 4).capacity(8))
                    .room(Room::new("QA triage", 6).capacity(6))
                    .room(Room::new("Main floor", 12).current())
                    .label("Breakout rooms"),
            ),
        ),
        (
            "Call Controls",
            Box::new(CallControls::new().label("Line 3 call")),
        ),
        (
            "Captions",
            Box::new(
                Captions::new()
                    .cue("Line 3 at full rate", 0.0, 2.5)
                    .cue("Shift change in 15 min", 3.0, 6.0)
                    .label("Announcements"),
            ),
        ),
        (
            "Chat Input",
            Box::new(
                ChatInput::new()
                    .placeholder("Message #line-3…")
                    .attachable(true)
                    .emoji_button(true),
            ),
        ),
        (
            "Chess Board",
            Box::new(ChessBoard::new().coordinates(true).label("Break room game")),
        ),
        (
            "Chess Clock",
            Box::new(ChessClock::new(Duration::from_secs(300)).increment(Duration::from_secs(2))),
        ),
        (
            "Comment Thread",
            Box::new(
                CommentThread::new()
                    .comment(Comment::new(
                        1,
                        "ops_lead",
                        "2h",
                        "Torque spec updated for F-9",
                    ))
                    .comment(Comment::new(2, "qa_tech", "1h", "Verified on fixture F-9").depth(1))
                    .comment(Comment::new(3, "maint", "20m", "Spare head on order").depth(1)),
            ),
        ),
        (
            "Control Center",
            Box::new(
                ControlCenter::new()
                    .tile("📶", "Wi-Fi", true)
                    .tile("📡", "Radio link", true)
                    .tile("🌙", "Do not disturb", false)
                    .slider("☀", "Brightness", 0.8)
                    .slider("🔊", "Alerts", 0.6)
                    .label("Quick settings"),
            ),
        ),
        (
            "Device Picker",
            Box::new(
                DevicePicker::new()
                    .section(DeviceKind::Microphone, ["Built-in mic", "USB mic"])
                    .section(DeviceKind::Speaker, ["Control room", "Headset"])
                    .section(DeviceKind::Camera, ["FaceTime HD"])
                    .active(DeviceKind::Microphone, 1)
                    .active(DeviceKind::Speaker, 0),
            ),
        ),
        (
            "Dial",
            Box::new(Dial::new().range(0.0, 100.0).value(62.0).step(1.0)),
        ),
        ("Emoji Picker", Box::new(EmojiPicker::standard())),
        (
            "Equalizer",
            Box::new(
                Equalizer::new()
                    .bands([0.4, 0.55, 0.7, 0.6, 0.5, 0.45, 0.65, 0.35])
                    .label("Mixer bus EQ"),
            ),
        ),
        (
            "Fretboard",
            Box::new(
                Fretboard::new()
                    .label("G major")
                    .set(0, 3)
                    .set(1, 2)
                    .set(5, 3),
            ),
        ),
        (
            "Media Controls",
            Box::new(
                MediaControls::new()
                    .duration(184.0)
                    .position(42.0)
                    .volume(0.7),
            ),
        ),
        (
            "Metronome",
            Box::new(Metronome::new().bpm(112).beats(4).running()),
        ),
        (
            "Now Playing",
            Box::new(
                NowPlaying::new("Line 3 Ambient", "Plant Radio")
                    .album("Floor Loops Vol. 2")
                    .duration(212.0)
                    .position(64.0)
                    .art_color([90, 140, 220, 255]),
            ),
        ),
        (
            "Pad Grid",
            Box::new(
                PadGrid::new(4, 2)
                    .pad_color(0, [220, 80, 80, 255])
                    .pad_label(0, "STOP")
                    .pad_color(1, [250, 190, 60, 255])
                    .pad_label(1, "HOLD")
                    .pad_color(2, [92, 200, 120, 255])
                    .pad_label(2, "RUN")
                    .pad_color(3, [90, 160, 240, 255])
                    .pad_label(3, "JOG")
                    .label("Machine pads"),
            ),
        ),
        (
            "Piano Keys",
            Box::new(PianoKeys::new().octaves(2).label("Alarm synth")),
        ),
        (
            "Pricing Table",
            Box::new(
                PricingTable::new()
                    .plan(
                        Plan::new("Basic", "$0")
                            .feature("1 line")
                            .feature("Email alerts"),
                    )
                    .plan(
                        Plan::new("Pro", "$49")
                            .period("/mo")
                            .feature("10 lines")
                            .feature("SCADA bridge")
                            .recommended()
                            .cta("Start trial"),
                    )
                    .plan(
                        Plan::new("Plant", "$199")
                            .period("/mo")
                            .feature("Unlimited lines")
                            .feature("SSO"),
                    ),
            ),
        ),
        (
            "Reaction Bar",
            Box::new(
                ReactionBar::new()
                    .reaction(Reaction::new("👍", 6).mine(true))
                    .reaction(Reaction::new("🎉", 3))
                    .reaction(Reaction::new("⚠️", 1))
                    .addable(true),
            ),
        ),
        (
            "Release Notes",
            Box::new(
                ReleaseNotes::new()
                    .release(
                        Release::new("2.4.0")
                            .date("2026-02-12")
                            .change(ChangeKind::Added, "Predictive-maintenance alerts")
                            .change(ChangeKind::Fixed, "FT-104 sensor drift"),
                    )
                    .release(
                        Release::new("2.3.1")
                            .date("2026-01-20")
                            .change(ChangeKind::Changed, "Faster trend rendering"),
                    ),
            ),
        ),
        (
            "Social Card",
            Box::new(
                SocialCard::new(
                    "Plant Ops",
                    "@plant_ops",
                    "12m",
                    "Line 2 back at full rate — OEE 91.4%",
                )
                .avatar_color([90, 140, 220, 255])
                .with_actions(vec![
                    CardAction::new("♥", 14),
                    CardAction::new("💬", 3),
                    CardAction::new("↗", 2),
                ]),
            ),
        ),
        (
            "Spectrum",
            Box::new(
                Spectrum::new()
                    .bands([0.3, 0.5, 0.8, 0.65, 0.9, 0.4, 0.55, 0.25])
                    .peak_hold(true)
                    .label("Ambient noise"),
            ),
        ),
        (
            "Step Sequencer",
            Box::new(
                StepSequencer::new(4, 8)
                    .lanes(["Kick", "Snare", "Hat", "Clap"])
                    .cells_on([
                        (0, 0),
                        (0, 4),
                        (1, 2),
                        (1, 6),
                        (2, 0),
                        (2, 2),
                        (2, 4),
                        (2, 6),
                        (3, 7),
                    ]),
            ),
        ),
        (
            "Theme Picker",
            Box::new(
                ThemePicker::new()
                    .option(ThemeOption::new(
                        "Light",
                        [250; 4],
                        [30; 4],
                        [80, 120, 200, 255],
                    ))
                    .option(ThemeOption::new(
                        "Dark",
                        [30; 4],
                        [235; 4],
                        [120, 160, 240, 255],
                    ))
                    .option(ThemeOption::new(
                        "Contrast",
                        [0; 4],
                        [255; 4],
                        [250, 200, 60, 255],
                    )),
            ),
        ),
        (
            "Ticket",
            Box::new(
                Ticket::new("Line 4 → Site B")
                    .caption("Maintenance visit · Tue")
                    .field("Gate", "B7")
                    .field("Seat", "12A")
                    .field("Zone", "2")
                    .code("MRT-8842"),
            ),
        ),
        (
            "Tuner",
            Box::new(Tuner::new().note("A").cents(-3.0).label("Belt drive tone")),
        ),
        (
            "Volume",
            Box::new(Volume::new().gain(0.6).max(1.5).label("Alarm audio")),
        ),
        ("Waiting Room", {
            let mut w = WaitingRoom::new().title("Contractor check-in");
            w.queue("M. Chen");
            w.queue("J. Park");
            w.queue("L. Gomez");
            Box::new(w)
        }),
        (
            "Waveform",
            Box::new(
                Waveform::new()
                    .peaks([0.2, 0.6, 0.9, 0.45, 0.75, 1.0, 0.35, 0.55, 0.8, 0.3])
                    .position(0.4)
                    .label("Line noise"),
            ),
        ),
    ]
}
