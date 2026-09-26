//! Media panel zones — shift comms, camera/PTZ ops, acoustic
//! condition monitoring, alarm-tone programming, media transport,
//! and inspection stills. Every mounted widget binds to
//! [`PlantModel`] through [`Bound`] (Design Council docket
//! `20260921`, bind-or-cut): comms widgets loop back through
//! `shift_log`, PTZ widgets steer `jog`/`camera_sel`, the audio
//! suite reads/trims `acoustic`, and transport widgets drive
//! `media_playing`/`media_pos`/`media_vol`.
//!
//! ## CUT (widgets with no honest binding)
//!
//! - `MessageComposer` — does not exist in the widget set;
//!   `ChatInput` is the real composer and owns the send path.
//! - `Announcements` / `InAppNotification` — no such widgets;
//!   `NotificationCenter` covers the pinned-log announcement role.
//! - `ConferenceGrid` / `PresenceList` — no such widgets; `VideoGrid`
//!   (roster tiles) + `AttendeeList` cover the room-presence role.
//! - `SeekBar` / `VolumeSlider` — no such widgets; `Slider` +
//!   `Volume` bind the same model signals.
//! - `SkipControls` / `RewindControls` / `RateMenu` — no such
//!   widgets; rate is a `Dropdown` on a local `Signal` that genuinely
//!   scales playhead advance.
//! - `AudioSpectrum`/`AudioLevelMeter`/`FrequencyDisplay`/
//!   `LoudnessMeter` — no such widgets; `Spectrum`, `VuMeter`,
//!   `LevelBar`, and a `Text` readout cover them.
//! - `Tilt` / `PositionDisplay` — no such widgets; `Slider` +
//!   `Text` readout bind `jog.tilt`/`jog.axis`.
//! - `TrackList` — `Playlist` covers it.
//! - `SocialCard` — stays cut: a static "post" card adds nothing the
//!   `MessageList`/`CommentThread` views of the same log don't
//!   already show honestly.
//!
//! ## Local signals (documented non-model bindings)
//!
//! - `rate` (`Signal<f64>`) — playback-rate select; genuinely scales
//!   the `media_pos` advance inside the transport pull. No model
//!   field exists for it and none should (it's console-local).
//! - `eq_trim` (`Signal<Vec<f64>>`) — monitor-path EQ trims written
//!   by `Equalizer` and applied to the displayed `Spectrum`/
//!   `Waveform`/`VuMeter`. Deliberately NOT `acoustic.bands`:
//!   `tick_acoustic` re-derives source bands every frame, so trims
//!   written there would be stomped — a real monitor EQ trims the
//!   console's monitor path, not the line.

use std::sync::Arc;
use std::time::Instant;

use martensite::core::{ImageData, LayoutConstraints, LayoutContext, Rect, Widget};
use martensite::media::surface::{
    ColorRange, HardwareHandle, VideoFrameMetadata, VideoPixelFormat, VideoSurface,
};
use martensite::reactive::Signal;
use martensite::widgets::attendee_list::{Attendee, AttendeeList};
use martensite::widgets::avatar::Avatar;
use martensite::widgets::avatar_group::AvatarGroup;
use martensite::widgets::breakout_rooms::{BreakoutRooms, Room as BreakoutRoom};
use martensite::widgets::button::Button;
use martensite::widgets::carousel::Carousel;
use martensite::widgets::chat_input::ChatInput;
use martensite::widgets::comment_thread::{Comment, CommentThread};
use martensite::widgets::coverflow::Coverflow;
use martensite::widgets::descriptions::Descriptions;
use martensite::widgets::dropdown::Dropdown;
use martensite::widgets::emoji_picker::EmojiPicker;
use martensite::widgets::equalizer::Equalizer;
use martensite::widgets::filmstrip::Filmstrip;
use martensite::widgets::flex::Flex;
use martensite::widgets::fretboard::Fretboard;
use martensite::widgets::group_box::GroupBox;
use martensite::widgets::image::Image;
use martensite::widgets::image_viewer::ImageViewer;
use martensite::widgets::joystick::Joystick;
use martensite::widgets::level_bar::LevelBar;
use martensite::widgets::lightbox::Lightbox;
use martensite::widgets::list_view::{ListView, SelectionMode};
use martensite::widgets::media::{MediaView, VideoFit};
use martensite::widgets::media_controls::MediaControls;
use martensite::widgets::mention::Mention;
use martensite::widgets::message_list::{Message, MessageList};
use martensite::widgets::metronome::Metronome;
use martensite::widgets::notification_center::{Notification, NotificationCenter};
use martensite::widgets::now_playing::NowPlaying;
use martensite::widgets::piano_keys::PianoKeys;
use martensite::widgets::pip::Pip;
use martensite::widgets::playlist::{Playlist, Track};
use martensite::widgets::poll::{Poll, PollOption};
use martensite::widgets::presence::PresenceStatus;
use martensite::widgets::reaction_bar::{Reaction, ReactionBar};
use martensite::widgets::scrollview::ScrollView;
use martensite::widgets::segmented::Segmented;
use martensite::widgets::slider::Slider;
use martensite::widgets::spectrum::Spectrum;
use martensite::widgets::status_dot::{Status, StatusDot};
use martensite::widgets::step_sequencer::StepSequencer;
use martensite::widgets::text::Text;
use martensite::widgets::tuner::Tuner;
use martensite::widgets::typing_indicator::TypingIndicator;
use martensite::widgets::video_grid::{Participant, VideoGrid};
use martensite::widgets::volume::Volume;
use martensite::widgets::vu_meter::VuMeter;
use martensite::widgets::waiting_room::WaitingRoom;
use martensite::widgets::waveform::Waveform;
use martensite::widgets::xy_pad::XYPad;
use martensite::widgets::zoom_controls::{ZoomAction, ZoomControls};
use martensite::widgets::Thumbnail;
use parking_lot::Mutex;

use crate::domain::{AssetKind, CrewMember, LogChannel, PlantModel, Presence, Room};
use crate::zone::{
    band, fill, framed, row, scroll, strip, Bound, Page, Swap, Variant, BAND_L, BAND_M, BAND_S,
    ZONE_GAP, ZONE_STACK,
};
use martensite::core::widget::DummyWidget;

// ---------------------------------------------------------------------------
// Domain constants — the fictional facility's camera/loop inventory.
// ---------------------------------------------------------------------------

/// Fixed camera inventory — the PTZ stack and camera wall address
/// these by index (`camera_sel`).
const CAMERAS: [&str; 4] = [
    "CAM-01 NORTH LINE",
    "CAM-02 EAST YARD",
    "CAM-03 WELD CELLS",
    "CAM-04 DOCK",
];
/// Per-camera tile colors (also the recording thumbnails).
const CAM_COLORS: [[u8; 4]; 4] = [
    [90, 140, 220, 255],
    [200, 140, 90, 255],
    [120, 180, 120, 255],
    [180, 90, 140, 255],
];
/// Short camera ids for captions and tiles — the descriptive
/// `CAMERAS` names (17 chars) cannot fit a `VideoGrid` tile
/// (~9–15 chars depending on column count) or a `Playlist`
/// subtitle lane in the transport rail.
const CAM_SHORT: [&str; 4] = ["CAM-01", "CAM-02", "CAM-03", "CAM-04"];
/// Recorded loops the transport scrubs — one per camera. Titles
/// stay ≤14 chars so the `Playlist` rows fit the narrow master
/// rail (`MASTER_FRAC` at 1200 px ≈ 110 pt of title lane).
const LOOPS: [&str; 4] = [
    "Loop A — Night",
    "Loop B — Weld",
    "Loop C — Yard",
    "Loop D — Dock",
];
/// Loop length in seconds — kept under 600 so the `MediaControls`
/// time lanes (fixed ~4-char fields) always read "M:SS", never a
/// clipped "10:00".
const LOOP_SECS: f64 = 540.0;
/// Content width of the six-thumb stills strip — mirrors
/// `Filmstrip`'s internal geometry (`6 × (72 + 6) + 2 × 8 pt`), so
/// the horizontal scroller lays it out at full content width and
/// owns the viewport-edge clip itself.
const STILLS_W_PT: f32 = 478.0;
/// Ack emoji the ReactionBar counts in the shift log.
const REACTIONS: [&str; 3] = ["👍", "⚠️", "✅"];
/// MIDI note names for the tone readouts.
const NOTE_NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];
/// Standard-tuning open-string MIDI notes (low E → high E) — the
/// Fretboard's string/fret reference for the programmed tone.
const OPEN_TUNING: [u8; 6] = [40, 45, 50, 55, 59, 64];

// ---------------------------------------------------------------------------
// Binding helpers — shared by the page builders and the tests.
// ---------------------------------------------------------------------------

/// Display name for a log author (`usize::MAX` = system annunciator).
fn crew_name(m: &PlantModel, author: usize) -> String {
    if author == usize::MAX {
        return "SYSTEM".to_string();
    }
    m.crew
        .get()
        .get(author)
        .map(|c| c.name.to_string())
        .unwrap_or_else(|| "—".to_string())
}

/// Shift-clock timestamp (`+047m` — minutes into the shift).
fn stamp(minute: u32) -> String {
    format!("+{minute:03}m")
}

/// `Presence` → `PresenceStatus` for the roster widgets.
fn presence_status(p: Presence) -> PresenceStatus {
    match p {
        Presence::OnShift | Presence::Remote => PresenceStatus::Online,
        Presence::Break => PresenceStatus::Away,
        Presence::OffShift => PresenceStatus::Offline,
    }
}

/// Tile color per presence state (huddle grid).
fn presence_color(p: Presence) -> [u8; 4] {
    match p {
        Presence::OnShift => [90, 180, 120, 255],
        Presence::Remote => [90, 140, 220, 255],
        Presence::Break => [220, 170, 80, 255],
        Presence::OffShift => [90, 90, 95, 255],
    }
}

/// Crew indices waiting on remote check-in: offsite + remote.
/// `WaitingRoom`'s queue order mirrors this list.
fn remote_queue(m: &PlantModel) -> Vec<usize> {
    m.crew
        .get()
        .iter()
        .enumerate()
        .filter(|(_, c)| c.presence == Presence::Remote && c.room == Room::Offsite)
        .map(|(i, _)| i)
        .collect()
}

/// FNV-1a fold — one mix step of the field-hash signatures below
/// (allocation-free; the old `format!`/`join` sigs allocated per call).
fn sig_fold(h: u64, v: u64) -> u64 {
    (h ^ v).wrapping_mul(0x100_0000_01b3)
}

/// `&'static str` identity = (ptr, len) — contents can't change under
/// the same slice.
fn str_fold(h: u64, s: &'static str) -> u64 {
    sig_fold(sig_fold(h, s.as_ptr() as usize as u64), s.len() as u64)
}

/// Change-detection signature for the crew roster — `push` closures
/// compare it before re-seating roster-built widgets (huddle grid,
/// waiting queue, avatar groups, mention targets), since a re-seat
/// drops in-flight press state.
fn crew_sig(m: &PlantModel) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for c in m.crew.get() {
        h = str_fold(h, c.name);
        h = sig_fold(h, c.presence as u64);
        h = sig_fold(h, c.room as u64);
    }
    h
}

/// Asset-identity signature for the inspection-photo surfaces — the
/// fields the stills, alt text, and thumb colors read (id, name,
/// status). A property-grid rename changes it; an OEE tick doesn't.
fn assets_sig(m: &PlantModel) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for a in m.assets.get() {
        h = sig_fold(h, u64::from(a.id));
        h = str_fold(h, a.name);
        h = sig_fold(h, a.status as u64);
    }
    h
}

/// Append-mostly signature for `shift_log`: length folded with the
/// newest entry. Entries only leave via append-time cap drains, so a
/// moved tail means the readable content moved — enough for readers
/// that re-derive counts from entry text (reaction tallies).
fn log_sig(m: &PlantModel) -> u64 {
    let log = m.shift_log.get();
    let mut h = sig_fold(0xcbf2_9ce4_8422_2325, log.len() as u64);
    // Fold every entry's id + pin state — appends and `LOG_CAP`
    // front-drains move `last`, pin/unpin does not.
    for e in &log {
        h = sig_fold(h, e.id);
        h = sig_fold(h, u64::from(e.pinned));
    }
    if let Some(e) = log.last() {
        h = sig_fold(h, u64::from(e.minute));
        h = sig_fold(h, e.author as u64);
    }
    h
}

/// Appends any `shift_log` entries the list hasn't rendered yet —
/// the comms loopback: `m.log` → next `tick` → visible bubble.
///
/// The cursor keys on `LogEntry::id` — a len/positional cursor
/// freezes the moment `log()` front-drains at `LOG_CAP`. `rendered`
/// is the id window (base..=last) the widget shows; when the log's
/// head moves past `base` a drain evicted rows the widget still
/// displays (or skipped unrendered entries — a gap). The widget is
/// append-only, so a drain re-seats it.
#[cfg(test)]
fn sync_message_list() -> impl FnMut(&mut MessageList, &PlantModel) + Send + Sync {
    sync_message_list_ch(None)
}

/// Channel-filtered variant — `Some(sel)` shows only the channel the
/// COMMS strip selects; `None` shows everything (the log's raw tail).
fn sync_message_list_ch(
    channel: Option<Signal<u8>>,
) -> impl FnMut(&mut MessageList, &PlantModel) + Send + Sync {
    let mut rendered: Option<(u64, u64)> = None;
    let mut last_ch = channel.as_ref().map(|c| c.get());
    move |l, m| {
        let log = m.shift_log.get();
        let ch = channel.as_ref().map(|c| c.get());
        if ch != last_ch {
            last_ch = ch;
            *l = MessageList::new().label("SHIFT LOG");
            rendered = None;
        }
        // Head moved (front-drain evicted rendered rows) or the log
        // emptied entirely — either way the widget's rows are stale.
        let chan_of = |e: &crate::domain::LogEntry| e.channel as u8;
        if rendered.is_some_and(|(b, _)| {
            log.iter()
                .find(|e| ch.is_none_or(|c| chan_of(e) == c))
                .map(|f| f.id)
                != Some(b)
        }) {
            *l = MessageList::new().label("SHIFT LOG");
            rendered = None;
        }
        for e in log.iter().filter(|e| ch.is_none_or(|c| chan_of(e) == c)) {
            if rendered.is_some_and(|(_, last)| e.id <= last) {
                continue;
            }
            let msg = if e.author == 0 {
                // Author 0 is the console operator — her entries
                // render as outgoing bubbles.
                Message::sent(bubble_line(&e.text)).time(stamp(e.minute))
            } else {
                Message::received(crew_name(m, e.author), bubble_line(&e.text))
                    .time(stamp(e.minute))
            };
            l.push(msg);
            rendered = Some((rendered.map(|(b, _)| b).unwrap_or(e.id), e.id));
        }
    }
}

/// Channel-filtered threaded view — the COMMS "Threaded" lens shows
/// the selected channel's entries nested (system roots, crew replies).
fn sync_comment_thread_ch(
    channel: Signal<u8>,
) -> impl FnMut(&mut CommentThread, &PlantModel) + Send + Sync {
    let mut rendered: Option<(u64, u64)> = None;
    let mut last_ch = channel.get();
    move |t, m| {
        let log = m.shift_log.get();
        let ch = channel.get();
        if ch != last_ch {
            last_ch = ch;
            *t = CommentThread::new().label("THREADED");
            rendered = None;
        }
        let chan_of = |e: &crate::domain::LogEntry| e.channel as u8;
        if rendered
            .is_some_and(|(b, _)| log.iter().find(|e| chan_of(e) == ch).map(|f| f.id) != Some(b))
        {
            *t = CommentThread::new().label("THREADED");
            rendered = None;
        }
        for e in log.iter().filter(|e| chan_of(e) == ch) {
            if rendered.is_some_and(|(_, last)| e.id <= last) {
                continue;
            }
            let c = Comment::new(
                e.id,
                crew_name(m, e.author),
                stamp(e.minute),
                thread_line(&e.text),
            )
            .depth(if e.author == usize::MAX { 0 } else { 1 });
            let label = std::mem::take(&mut t.label);
            *t = std::mem::take(t).comment(c).label(label);
            rendered = Some((rendered.map(|(b, _)| b).unwrap_or(e.id), e.id));
        }
    }
}

/// ChatInput → `shift_log` (author 0 = console operator Ana Ruiz).
fn drain_chat_input(c: &mut ChatInput, m: &PlantModel) {
    if let Some(text) = c.take_sent() {
        if !text.trim().is_empty() {
            m.log(0, text);
        }
    }
    if c.take_attach() {
        m.log(0, "📎 attachment");
    }
    c.take_emoji(); // opens the widget's own picker — nothing to publish
}

/// `camera_sel` + rewind — the shared "load this recording" action
/// for Playlist/Coverflow/Carousel/Filmstrip.
fn load_recording(m: &PlantModel, i: usize) {
    if i < CAMERAS.len() {
        m.camera_sel.set(i);
        m.media_pos.set(0.0);
    }
}

/// Builds the breakout-room selector from current crew placement.
fn breakouts(crew: &[CrewMember]) -> BreakoutRooms {
    let mut b = BreakoutRooms::new().label("BREAKOUTS");
    for (name, room) in [
        ("Breakout A — safety review", Room::BreakoutA),
        ("Breakout B — QA triage", Room::BreakoutB),
    ] {
        let occupants = crew.iter().filter(|c| c.room == room).count();
        let mut r = BreakoutRoom::new(name, occupants).capacity(4);
        if crew.first().is_some_and(|c| c.room == room) {
            r = r.current();
        }
        b = b.room(r);
    }
    b
}

/// MIDI note number → name (`57` → `"A3"`).
fn note_name(note: u8) -> String {
    format!("{}{}", NOTE_NAMES[note as usize % 12], note as i32 / 12 - 1)
}

/// Reference frequency of a MIDI note (A4 = 440).
fn note_hz(note: u8) -> f64 {
    440.0 * 2f64.powf((f64::from(note) - 69.0) / 12.0)
}

/// Best (string, fret) showing `note` in standard tuning — smallest
/// reachable fret wins.
fn tone_pos(note: u8) -> Option<(u8, u8)> {
    (0u8..6)
        .filter_map(|s| {
            let f = i16::from(note) - i16::from(OPEN_TUNING[s as usize]);
            (0..=15).contains(&f).then_some((s, f as u8))
        })
        .min_by_key(|&(_, f)| f)
}

/// Fretboard view of the programmed alarm tone — label names the
/// note, the marker shows its shortest-fret fingering.
fn fretboard_of(note: u8) -> Fretboard {
    let mut fb = Fretboard::new().label(format!("TONE REF — {}", note_name(note)));
    if let Some((s, fr)) = tone_pos(note) {
        fb = fb.set(s, fr);
    }
    fb
}

/// Procedural inspection still — deterministic per asset id (grid +
/// tinted gradient, the honest "camera frame" stand-in the showcase
/// uses for its image widgets).
fn inspection_photo(seed: u32, w: u32, h: u32) -> ImageData {
    let px: Vec<u8> = (0..w * h)
        .flat_map(|i| {
            let x = i % w;
            let y = i / w;
            let grid = x.is_multiple_of(12) || y.is_multiple_of(12);
            let tint = ((seed * 37) % 80) as u8;
            if grid {
                [28, 30, 36, 255]
            } else {
                [
                    (48 + x * 2 + u32::from(tint)).min(235) as u8,
                    (70 + y * 2 + u32::from(tint) / 2).min(235) as u8,
                    150,
                    255,
                ]
            }
        })
        .collect();
    ImageData::from_rgba(w, h, px).expect("pixel count matches dimensions")
}

/// Cell assets (the inspection-photo subjects), in asset order.
fn cells(m: &PlantModel) -> Vec<crate::domain::Asset> {
    m.assets
        .get()
        .into_iter()
        .filter(|a| a.kind == AssetKind::Cell)
        .collect()
}

/// Tile color per asset status (inspection thumbnails).
fn status_color(s: crate::domain::AssetStatus) -> [u8; 4] {
    use crate::domain::AssetStatus::*;
    match s {
        Running => [90, 180, 120, 255],
        Degraded => [220, 170, 80, 255],
        Down => [210, 80, 80, 255],
        Maintenance => [110, 110, 160, 255],
    }
}

/// `AssetStatus` → status-lamp chip for the inspection thumbnails
/// (spec D1): the tile keeps its status tint and the chip adds the
/// redundant mark channel — status is never carried by hue alone.
/// `Maintenance` maps to `Off` (neutral): a maintained cell is
/// deliberately out of reporting, not a fault.
fn thumb_status(s: crate::domain::AssetStatus) -> Status {
    use crate::domain::AssetStatus::*;
    match s {
        Running => Status::Ok,
        Degraded => Status::Warning,
        Down => Status::Error,
        Maintenance => Status::Off,
    }
}

// ---------------------------------------------------------------------------
// Text-fit + layout helpers — the feed lenses paint bodies as single
// unwrapped lines inside hard clips (no wrap, no scroll), so the
// bound text has to be pre-fit through the real painter.
// ---------------------------------------------------------------------------

/// Shaped advance of `text` at `size_pt`, through the shared painter.
fn measure_pt(text: &str, size_pt: f32) -> f32 {
    thread_local! {
        static PAINTER: martensite::text_paint::SharedTextPainter =
            martensite::text_paint::shared_painter();
    }
    PAINTER.with(|p| p.measure(text, size_pt))
}

/// Elide `text` with a trailing "…" until it measures within
/// `max_pt` at `size_pt` — the honest fit for a hard-clipped,
/// non-wrapping text lane.
fn elide_to(text: &str, max_pt: f32, size_pt: f32) -> String {
    if measure_pt(text, size_pt) <= max_pt {
        return text.to_string();
    }
    let mut t = String::new();
    for c in text.chars() {
        let mut cand = t.clone();
        cand.push(c);
        cand.push('…');
        if measure_pt(&cand, size_pt) > max_pt {
            break;
        }
        t.push(c);
    }
    t.push('…');
    t
}

/// Feed-line fit for `MessageList` bubbles. Bubbles clamp at 72% of
/// the list width and can neither wrap nor scroll a line, so a body
/// longer than the narrowest clamped text zone (~700 px feed column:
/// ~316 pt list → `0.72 × 316 − 2 × 8 pt` pads) is elided. Bubble
/// width itself is honest — the widget measures real glyph advances.
fn bubble_line(text: &str) -> String {
    elide_to(text, 210.0, 12.0)
}

/// Row-body fit for `CommentThread` — same single-line hard clip
/// as the bubbles; the lane is the widget width minus the depth
/// indent and avatar (≈253 pt at the narrowest probed width).
fn thread_line(text: &str) -> String {
    elide_to(text, 240.0, 13.0)
}

/// Minimum-width mount — reports `min_pt` as the lower bound of its
/// measured width and lays its child out to the real bounds. Used
/// inside `ScrollView::horizontal` for a child whose `measure`
/// under-reports its true content extent (`Filmstrip` caps its
/// report at 320 pt regardless of thumb count): the child then gets
/// full content width and the scrollport owns the viewport-edge
/// clip — the sanctioned overflow affordance — instead of the
/// widget's own hard clip cutting captions mid-glyph.
struct MinW {
    child: Box<dyn Widget>,
    min_pt: f32,
    bounds: Rect,
}

impl MinW {
    fn new(min_pt: f32, w: impl Widget + 'static) -> Self {
        Self {
            child: Box::new(w),
            min_pt,
            bounds: Rect::default(),
        }
    }
}

/// Reported-width cap for scroll-mounted columns. `ScrollView` layout
/// re-measures its content with `max_size.x = bounds.width()` and
/// trusts the report — a child that echoes the offer (`Band`, `Fill`,
/// `Swap` → `Coverflow`, `Playlist`'s `min(W_PT, max)`) hands back
/// `desired.x = bounds.width()`, which is exactly one scrollbar wider
/// than the real viewport. That trips a phantom horizontal scrollbar
/// and lets right-edge text (NowPlaying's time lane, Playlist's
/// duration column) slide under the viewport clip mid-glyph. `CapW`
/// clamps the *offer* during `measure` — `layout` still hands the
/// child its real bounds — so `desired.x` lands below the viewport and
/// the column packs to the visible width.
///
/// 140pt sits under every real viewport this zone sees (~166pt for a
/// just-disclosed rail, ~326pt for the primary at a 700pt zone) while
/// staying honest: content with a genuine fixed minimum still reports
/// it, so a truly narrow zone keeps its sanctioned h-scroller.
const SCROLL_CAP_PT: f32 = 140.0;

/// Width-offer cap — see [`SCROLL_CAP_PT`]. Mirrors [`MinW`].
struct CapW {
    child: Box<dyn Widget>,
    max_pt: f32,
    bounds: Rect,
}

impl CapW {
    fn new(max_pt: f32, w: impl Widget + 'static) -> Self {
        Self {
            child: Box::new(w),
            max_pt,
            bounds: Rect::default(),
        }
    }
}

impl Widget for CapW {
    fn debug_name(&self) -> &'static str {
        "CapW"
    }

    fn measure(&mut self, cx: &mut LayoutContext, c: LayoutConstraints) -> glam::Vec2 {
        self.child.measure(
            cx,
            LayoutConstraints {
                min_size: c.min_size,
                max_size: glam::Vec2::new(c.max_size.x.min(cx.pt(self.max_pt)), c.max_size.y),
            },
        )
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.child.layout(cx, bounds);
    }

    fn accessibility(&self, node: &mut accesskit::Node) {
        node.set_role(accesskit::Role::Group);
    }

    fn child_count(&self) -> usize {
        1
    }
    fn child(&self, index: usize) -> Option<&dyn Widget> {
        (index == 0).then_some(&*self.child)
    }
    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        (index == 0).then_some(&mut *self.child)
    }
    fn child_bounds(&self, index: usize) -> Option<Rect> {
        (index == 0).then_some(self.bounds)
    }
}

impl Widget for MinW {
    fn debug_name(&self) -> &'static str {
        "MinW"
    }

    fn measure(&mut self, cx: &mut LayoutContext, c: LayoutConstraints) -> glam::Vec2 {
        let s = self.child.measure(cx, c);
        glam::Vec2::new(s.x.max(cx.pt(self.min_pt)), s.y)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.child.layout(cx, bounds);
    }

    fn accessibility(&self, node: &mut accesskit::Node) {
        node.set_role(accesskit::Role::Group);
    }

    fn child_count(&self) -> usize {
        1
    }
    fn child(&self, index: usize) -> Option<&dyn Widget> {
        (index == 0).then_some(&*self.child)
    }
    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        (index == 0).then_some(&mut *self.child)
    }
    fn child_bounds(&self, index: usize) -> Option<Rect> {
        (index == 0).then_some(self.bounds)
    }
}

// ---------------------------------------------------------------------------
// The pages.
// ---------------------------------------------------------------------------

/// Domain-named zone pages for the Media panel — tab labels are
/// domain names ("SHIFT COMMS"), never widget names.
pub fn pages(model: &PlantModel) -> Vec<(&'static str, Page)> {
    // Console-local signals (see module doc): playback rate and the
    // monitor-path EQ trims.
    let rate = Signal::new(1.0f64);
    let eq_trim = Signal::new(vec![1.0f64; 16]);

    vec![
        ("COMMS", comms_page(model)),
        ("ROOMS", rooms_page(model)),
        ("CAMERAS", cameras_page(model)),
        ("TRANSPORT", transport_page(model, rate)),
        ("ACOUSTIC", acoustic_page(model, eq_trim)),
    ]
}

// ---------------------------------------------------------------------------
// SHIFT COMMS — the loopback demo: every widget is a view of, or an
// editor into, `shift_log` (+ `poll_votes`, `crew` presence).
// ---------------------------------------------------------------------------
/// COMMS — "what's the crew saying on this channel?" MasterDetail.
/// Strip: channel selector (Ops | Comms | System → `channel_sel`)
/// and the feed lens (Feed | Threaded). Primary: the channel's
/// message surface over composer + ack rows (reactions, emoji),
/// with announcements (pinned entries) as a bounded secondary.
/// Rail: the engage tools — crew ping, remote-presence dots, and
/// the standing poll; it stacks under the feed at stack widths and
/// discloses below `RAIL_DISCLOSE_W`, so the feed always owns the
/// width it needs for unwrapped bubble text.
fn comms_page(model: &PlantModel) -> Page {
    // Remote-crew mentions + a "ping" publish path.
    let mention = Bound::new(
        Mention::new()
            .suggestions(model.crew.get().iter().map(|c| c.name))
            .placeholder("@crew to ping…")
            .label("MENTIONS"),
        model,
    )
    .pull(|mn, m| {
        if let Some(name) = mn.take_committed() {
            m.log(0, format!("@{} — ping from console", name.trim_end()));
            mn.set_value("");
        }
        mn.take_edited(); // live-text echo — no model field, drain
    })
    .push({
        // Suggestion list follows the roster — refresh on change only.
        let mut last = crew_sig(model);
        move |mn, m| {
            let s = crew_sig(m);
            if s != last {
                last = s;
                mn.set_suggestions(m.crew.get().iter().map(|c| c.name));
            }
        }
    });

    // Remote presence → "monitoring remotely" indicator.
    let typing = Bound::new(TypingIndicator::new(), model).push({
        let mut last = None;
        move |t, m| {
            let s = crew_sig(m);
            if Some(s) != last {
                last = Some(s);
                let remote: Vec<String> = m
                    .crew
                    .get()
                    .iter()
                    .filter(|c| c.presence == Presence::Remote)
                    .map(|c| format!("{} is monitoring remotely", c.name))
                    .collect();
                *t = TypingIndicator::new()
                    .label(remote.join(" · "))
                    .active(!remote.is_empty());
            }
        }
    });

    // Acks are log entries — the bar counts them, clicking posts one.
    let reactions = Bound::new(
        ReactionBar::new()
            .reaction(Reaction::new("👍", 0))
            .reaction(Reaction::new("⚠️", 0))
            .reaction(Reaction::new("✅", 0))
            .label("ACKS"),
        model,
    )
    .pull(|r, m| {
        if let Some(i) = r.take_toggled() {
            if let Some(rx) = r.reaction_at(i) {
                let emoji = rx.emoji.clone();
                m.log(0, emoji);
            }
        }
        r.take_add(); // custom-ack affordance — no model field, drain
    })
    .push({
        // Tallies derive from the log — re-count only when it moves.
        let mut last = None;
        move |r, m| {
            let s = log_sig(m);
            if Some(s) != last {
                last = Some(s);
                let log = m.shift_log.get();
                r.set_reactions(
                    REACTIONS
                        .iter()
                        .map(|e| {
                            let count = log.iter().filter(|le| le.text.trim() == *e).count() as u32;
                            let mine = log.iter().any(|le| le.text.trim() == *e && le.author == 0);
                            Reaction::new(*e, count).mine(mine)
                        })
                        .collect(),
                );
            }
        }
    });

    // Emoji picker → posts the glyph as a log entry (operator ack).
    let emoji = Bound::new(EmojiPicker::standard().label("QUICK ACK"), model).pull(|e, m| {
        if let Some(glyph) = e.take_picked() {
            m.log(0, glyph);
        }
    });

    // The standing crew poll → `poll_votes`. Pull-only by design:
    // rebuilding the widget per push would erase its `my_vote`
    // (results) state; the tally originates here anyway.
    let votes = model.poll_votes.get();
    let poll = Bound::new(
        // The question paints unclipped at card width — keep it
        // inside the rail's narrowest card (~165 pt of inner lane
        // at a 1200 px zone).
        Poll::new("Approve PM window?")
            .option(PollOption::new("Yes", votes[0]))
            .option(PollOption::new("No", votes[1]))
            .option(PollOption::new("Abstain", votes[2])),
        model,
    )
    .pull(|p, m| {
        if let Some(i) = p.take_voted() {
            if i < 3 {
                m.poll_votes.update(|v| v[i] += 1);
            }
        }
    });

    // Announcements = pinned log entries; dismissing a card unpins
    // it. `pushed` maps card index → `LogEntry::id` — positional log
    // indices would misalign the moment `log()` front-drains. `push`
    // prepends, so each new card leads both lists.
    let pushed = Arc::new(Mutex::new(Vec::<u64>::new()));
    let announcements = {
        let pushed_pull = Arc::clone(&pushed);
        let pushed_push = Arc::clone(&pushed);
        Bound::new(NotificationCenter::new().label("ANNOUNCEMENTS"), model)
            .pull(move |nc, m| {
                if let Some(i) = nc.take_dismissed() {
                    let mut pushed = pushed_pull.lock();
                    if let Some(&id) = pushed.get(i) {
                        m.shift_log.update(|l| {
                            if let Some(e) = l.iter_mut().find(|e| e.id == id) {
                                e.pinned = false;
                            }
                        });
                    }
                    if i < pushed.len() {
                        pushed.remove(i);
                    }
                }
                if nc.take_cleared() {
                    let mut pushed = pushed_pull.lock();
                    for &id in pushed.iter() {
                        m.shift_log.update(|l| {
                            if let Some(e) = l.iter_mut().find(|e| e.id == id) {
                                e.pinned = false;
                            }
                        });
                    }
                    pushed.clear();
                }
            })
            .push(move |nc, m| {
                let log = m.shift_log.get();
                let mut pushed = pushed_push.lock();
                // A `LOG_CAP` drain can evict a pinned entry — its
                // card would stay orphaned and its id misalign
                // `pushed`. Re-seat the whole center (the widget has
                // no card-removal API) and repush the live pins.
                if pushed.iter().any(|id| !log.iter().any(|e| e.id == *id)) {
                    *nc = NotificationCenter::new().label("ANNOUNCEMENTS");
                    pushed.clear();
                }
                // Iterate oldest→newest: `push` prepends, so the
                // newest pin lands on top and card order matches
                // `pushed` (index i ↔ pushed[i]).
                for e in log.iter().filter(|e| e.pinned) {
                    if !pushed.contains(&e.id) {
                        nc.push(
                            Notification::new("ANNOUNCEMENT", e.text.clone()).meta(format!(
                                "{} · {}",
                                crew_name(m, e.author),
                                stamp(e.minute)
                            )),
                        );
                        pushed.insert(0, e.id);
                    }
                }
            })
    };

    // Channel selector — the page's named scope; the composer tags
    // posts with it and the feed filters by it.
    let channel = {
        let cs = model.channel_sel.clone();
        Bound::new(
            Segmented::new()
                .options(LogChannel::all().map(|c| c.label()))
                .selected(model.channel_sel.get() as usize)
                .label("channel"),
            model,
        )
        .pull(move |w: &mut Segmented, m| {
            if let Some(i) = w.take_selected() {
                cs.set_if_changed(i as u8);
                m.channel_sel.set_if_changed(i as u8);
            }
        })
    };
    let feed_sel = Signal::new(0usize);
    let feed = {
        let fs = feed_sel.clone();
        Bound::new(
            Segmented::new()
                .options(["Feed", "Threaded"])
                .selected(0)
                .label("feed view"),
            model,
        )
        .pull(move |w: &mut Segmented, _m| {
            if let Some(i) = w.take_selected() {
                fs.set_if_changed(i);
            }
        })
    };

    // Channel-filtered surfaces — the MessageList follows
    // `channel_sel`; the Threaded lens shows the same channel nested.
    let list = Bound::new(MessageList::new().label("channel feed"), model)
        .push(sync_message_list_ch(Some(model.channel_sel.clone())));
    let thread = Bound::new(CommentThread::new().label("threaded"), model)
        .push(sync_comment_thread_ch(model.channel_sel.clone()));
    let surface = Swap::new(&feed_sel).view(list).view(thread);

    // Composer — posts to the selected channel.
    let composer = Bound::new(
        ChatInput::new()
            .placeholder("Message the channel…")
            .attachable(true)
            .emoji_button(true),
        model,
    )
    .pull({
        let cs = model.channel_sel.clone();
        move |c: &mut ChatInput, m| {
            if let Some(text) = c.take_sent() {
                if !text.trim().is_empty() {
                    let ch = match cs.get() {
                        1 => crate::domain::LogChannel::Comms,
                        2 => crate::domain::LogChannel::System,
                        _ => crate::domain::LogChannel::Ops,
                    };
                    m.log_ch(0, text, ch, None);
                }
            }
            drain_chat_input(c, m); // attach/emoji seams
        }
    });

    let primary = Flex::column()
        .gap(ZONE_GAP)
        .child_flex(surface, 1.0)
        // The composer owns its own row — the ping/typing affordances
        // moved to the rail so the field never shares width with
        // controls it would crush at narrow zones.
        .child(strip().child_flex(composer, 1.0))
        .child(strip().child(reactions).child_flex(DummyWidget, 1.0))
        // The picker is an intrinsic panel (never banded): it takes
        // the column's full width and the page scroll reaches it.
        .child(emoji)
        .child(band(BAND_S, announcements));

    // Engage rail — the standing poll plus the remote-ping field and
    // presence dots. Detail of the comms surface: sits beside the
    // feed at rail widths, stacks under it at stack widths, and
    // collapses into a `Disclosure` below `RAIL_DISCLOSE_W` — the
    // feed owns the width either way.
    let rail_col = Flex::column()
        .gap(ZONE_GAP)
        .child(mention)
        .child(typing)
        .child(poll);

    // Feed + composer + reactions + announcements — an intrinsic stack
    // that can exceed a short zone; scroll-mounted so the trailing
    // bands stay reachable instead of crushing the feed to zero.
    Page::new(
        Variant::MasterDetail,
        fill(scroll(CapW::new(SCROLL_CAP_PT, primary))),
        &model.zone_width[3],
    )
    .strip(
        strip()
            .child(channel)
            .child(feed)
            .child_flex(DummyWidget, 1.0),
    )
    .rail("Engage", CapW::new(SCROLL_CAP_PT, rail_col))
}

// ---------------------------------------------------------------------------
// ROOMS — crew location/presence views. VideoGrid = huddle roster,
// AttendeeList = roster w/ live status, WaitingRoom = remote
// check-in queue (admit → joins the control-room call, deny → off
// roster), BreakoutRooms = occupancy + join, AvatarGroups = per-room
// presence.
// ---------------------------------------------------------------------------
/// ROOMS — "who's where, and who needs admitting?" Master-detail |
/// primary: huddle wall + roster + room presence | selection:
/// `selected_member` (roster row picks) | rail: the member's card +
/// roster fields.
fn rooms_page(model: &PlantModel) -> Page {
    let huddle = Bound::new(VideoGrid::new().label("SHIFT HUDDLE"), model)
        .pull(|g, m| {
            // Tile click opens a 1:1 huddle — the console logs it like
            // any other comms action.
            if let Some(i) = g.take_selected() {
                m.log(0, format!("huddle opened with {}", crew_name(m, i)));
            }
        })
        .push({
            let mut last = None;
            move |g, m| {
                // Re-seat only when the roster or the selected feed
                // moves — a per-tick rebuild drops in-flight presses.
                let s = (crew_sig(m), m.camera_sel.get());
                if Some(s) != last {
                    last = Some(s);
                    let mut ng = VideoGrid::new().label("SHIFT HUDDLE");
                    for c in m.crew.get().iter() {
                        ng = ng.participant(
                            Participant::new(c.name, presence_color(c.presence))
                                .muted(matches!(c.presence, Presence::Break | Presence::OffShift))
                                .speaking(c.presence == Presence::Remote),
                        );
                    }
                    *g = ng;
                }
            }
        });

    let mut roster = AttendeeList::new().label("SHIFT ROSTER");
    for c in model.crew.get().iter() {
        roster = roster.attendee(Attendee::new(c.name).status(presence_status(c.presence)));
    }
    let roster = Bound::new(roster, model)
        .pull(|a, m| {
            // Row click opens a direct channel — logged by the console.
            if let Some(i) = a.take_selected() {
                m.log(0, format!("direct channel to {}", crew_name(m, i)));
            }
        })
        .push(|a, m| {
            for (i, c) in m.crew.get().iter().enumerate() {
                a.set_status(i, presence_status(c.presence));
            }
        });

    let waiting = Bound::new(
        WaitingRoom::new().title("Remote check-in").label("WAITING"),
        model,
    )
    .pull(|w, m| {
        if let Some(i) = w.take_admitted() {
            let queue = remote_queue(m);
            if i == usize::MAX {
                for &j in &queue {
                    // Admit-all → everyone waiting joins the call.
                    m.crew.update(|c| {
                        if let Some(c) = c.get_mut(j) {
                            c.room = Room::Control;
                        }
                    });
                }
            } else if let Some(&j) = queue.get(i) {
                // Admitted → joins the control-room call.
                m.crew.update(|c| {
                    if let Some(c) = c.get_mut(j) {
                        c.room = Room::Control;
                    }
                });
            }
        }
        if let Some(i) = w.take_denied() {
            if let Some(&j) = remote_queue(m).get(i) {
                // Denied → drops off the shift roster.
                m.crew.update(|c| {
                    if let Some(c) = c.get_mut(j) {
                        c.presence = Presence::OffShift;
                    }
                });
            }
        }
    })
    .push({
        let mut last = None;
        move |w, m| {
            // Re-seat the queue only on roster change — a per-tick
            // clear+requeue eats in-flight admit/deny presses.
            let s = crew_sig(m);
            if Some(s) != last {
                last = Some(s);
                w.clear();
                let crew = m.crew.get();
                for &j in &remote_queue(m) {
                    if let Some(c) = crew.get(j) {
                        w.queue(c.name);
                    }
                }
            }
        }
    });

    let breakout = Bound::new(breakouts(&model.crew.get()), model)
        .pull(|b, m| {
            if let Some(i) = b.take_joined() {
                // Crew 0 (the console operator) joins that breakout.
                let room = if i == 0 {
                    Room::BreakoutA
                } else {
                    Room::BreakoutB
                };
                m.crew.update(|c| {
                    if let Some(c) = c.first_mut() {
                        c.room = room;
                    }
                });
            }
        })
        .push({
            // Re-seat on roster change — rebuilding per tick drops
            // in-flight join presses.
            let mut last = crew_sig(model);
            move |b, m| {
                let s = crew_sig(m);
                if s != last {
                    last = s;
                    *b = breakouts(&m.crew.get());
                }
            }
        });

    let floor = Bound::new(AvatarGroup::new().max_count(6).label("FLOOR"), model).push({
        let mut last = None;
        move |g, m| {
            let s = crew_sig(m);
            if Some(s) != last {
                last = Some(s);
                let mut ng = AvatarGroup::new().max_count(6).label("FLOOR");
                for c in m.crew.get().iter().filter(|c| c.room == Room::Floor) {
                    ng = ng.member(Avatar::new(c.name));
                }
                *g = ng;
            }
        }
    });

    let control = Bound::new(AvatarGroup::new().max_count(6).label("CONTROL"), model).push({
        let mut last = None;
        move |g, m| {
            let s = crew_sig(m);
            if Some(s) != last {
                last = Some(s);
                let mut ng = AvatarGroup::new().max_count(6).label("CONTROL");
                for c in m.crew.get().iter().filter(|c| c.room == Room::Control) {
                    ng = ng.member(Avatar::new(c.name));
                }
                *g = ng;
            }
        }
    });

    let primary = Flex::column()
        .gap(ZONE_STACK)
        // The huddle wall owns a full-width row — paired with the
        // roster it halves to a single ~180 pt tile column and the
        // `VideoGrid` shrinks tiles until names clip mid-glyph.
        // Rows share one band height, so the roster gets its own
        // full-width band row rather than a squeezed half share.
        .child(row().child_flex(band(BAND_L, huddle), 1.0))
        .child(row().child_flex(band(BAND_L, roster), 1.0))
        .child(
            row()
                .child_flex(band(BAND_M, waiting), 1.0)
                .child_flex(band(BAND_M, breakout), 1.0),
        )
        .child(
            row()
                .child_flex(band(BAND_S, floor), 1.0)
                .child_flex(band(BAND_S, control), 1.0),
        );

    // Rail — `selected_member`: the roster/huddle pick lands here.
    // One column only: at the narrow rail width the default two
    // columns give each label lane ~37 pt and the keys clip.
    let member_detail =
        Bound::new(Descriptions::new().column_count(1), model).push(|d: &mut Descriptions, m| {
            *d = match m
                .selected_member
                .get()
                .and_then(|i| m.crew.get().get(i).cloned())
            {
                Some(c) => Descriptions::new()
                    .column_count(1)
                    .title(c.name)
                    .bordered(true)
                    .item("role", c.role)
                    .item(
                        // Short key — the bordered label lane in the
                        // rail narrows to ~50 pt at 1200 px.
                        "status",
                        match c.presence {
                            Presence::OnShift => "on shift",
                            Presence::Remote => "remote",
                            Presence::Break => "on break",
                            Presence::OffShift => "off shift",
                        },
                    )
                    .item("room", format!("{:?}", c.room)),
                None => Descriptions::new()
                    .column_count(1)
                    .title("MEMBER")
                    .item("state", "none selected"),
            };
        });
    let rail_col = Flex::column().gap(ZONE_GAP).child(member_detail);
    Page::new(
        Variant::MasterDetail,
        fill(scroll(CapW::new(SCROLL_CAP_PT, primary))),
        &model.zone_width[3],
    )
    .rail("Member", CapW::new(SCROLL_CAP_PT, rail_col))
}

// ---------------------------------------------------------------------------
// CAMERAS / PTZ — the camera wall selects `camera_sel`; the PTZ
// cluster steers `jog` (momentary Joystick, absolute XYPad, tilt
// Slider, ZoomControls); MediaView/Pip show the feeds; Filmstrip
// stills scrub `media_pos`.
// ---------------------------------------------------------------------------
/// CAMERAS — "what is CAM-n seeing, and where should it point?"
/// Master-detail | strip: camera selector + feed readout | primary:
/// the camera wall + stills strip | selection: `camera_sel` | rail:
/// PTZ cluster (jog/pad/tilt/zoom) steering `jog` + the recordings
/// archive (`selected_recording`).
fn cameras_page(model: &PlantModel) -> Page {
    // The main view tracks the wall selection: a real build would bind
    // each camera's own hardware surface, so the mock stands in with a
    // per-camera handle id. `jog.zoom` has no factor on the widget —
    // >1x reads as crop-to-fill, mapped onto the fit mode.
    let view = Bound::new(
        MediaView::new()
            .with_surface(VideoSurface::new_mock(1280, 720, VideoPixelFormat::Nv12))
            .with_fit(VideoFit::Contain),
        model,
    )
    .push({
        let mut last: Option<(usize, VideoFit)> = None;
        move |v, m| {
            let sel = m.camera_sel.get().min(CAMERAS.len() - 1);
            let fit = if m.jog.get().zoom > 1.0 {
                VideoFit::Cover
            } else {
                VideoFit::Contain
            };
            if last != Some((sel, fit)) {
                last = Some((sel, fit));
                *v = MediaView::new()
                    .with_surface(VideoSurface::new(
                        HardwareHandle::Mock { id: sel as u64 },
                        VideoFrameMetadata::new(
                            1280,
                            720,
                            VideoPixelFormat::Nv12,
                            ColorRange::Limited,
                        ),
                    ))
                    .with_fit(fit);
            }
        }
    });

    // PiP = next camera's feed; expanding swaps it onto the main view.
    // Captions use the short ids — the full `CAMERAS` names (17
    // chars) cannot fit a PiP/`VideoGrid` tile lane at narrow zones.
    let pip = Bound::new(Pip::new(Text::new(CAM_SHORT[1])).label("SECONDARY"), model)
        .pull(|p, m| {
            if p.take_expanded() {
                let next = (m.camera_sel.get() + 1) % CAMERAS.len();
                m.camera_sel.set(next);
            }
            p.take_closed(); // panel dismiss — no model field, drain
            p.take_dragged(); // panel drag offset — no model field, drain
        })
        .push({
            let mut last = (model.camera_sel.get() + 1) % CAMERAS.len();
            move |p, m| {
                // The label only re-seats when the next-up feed changes.
                let next = (m.camera_sel.get() + 1) % CAMERAS.len();
                if next != last {
                    last = next;
                    p.set_content(Text::new(CAM_SHORT[next]));
                }
            }
        });

    let mut wall = VideoGrid::new().label("CAMERA WALL");
    for (i, name) in CAM_SHORT.iter().enumerate() {
        wall = wall.participant(Participant::new(*name, CAM_COLORS[i]));
    }
    let wall = Bound::new(wall, model)
        .pull(|g, m| {
            if let Some(i) = g.take_selected() {
                m.camera_sel.set(i.min(CAMERAS.len() - 1));
            }
        })
        .push(|g, m| {
            // The selected feed gets the speaking ring — the wall's
            // honest "active" marker.
            for i in 0..CAMERAS.len() {
                g.set_speaking(i, m.camera_sel.get() == i);
            }
        });

    // Recording stills — six frames spaced through the selected
    // camera's loop; picking a still seeks the playhead.
    let stills = {
        let mut f = Filmstrip::new().label("STILLS");
        for i in 0..6 {
            // Captions live in a 72 pt tile lane — keep them ≤ ~10
            // chars so they read whole, never clipped mid-glyph.
            f = f.thumb(Thumbnail::new(
                format!("C01 {:>3}s", i * 100),
                CAM_COLORS[0],
            ));
        }
        let mut last_cam = model.camera_sel.get();
        Bound::new(f, model)
            .pull(move |f, m| {
                if let Some(i) = f.take_selected() {
                    m.media_pos.set(i as f64 / 5.0);
                }
            })
            .push(move |f, m| {
                let sel = m.camera_sel.get().min(CAMERAS.len() - 1);
                if sel != last_cam {
                    last_cam = sel;
                    let mut nf = Filmstrip::new().label("STILLS");
                    for i in 0..6 {
                        nf = nf.thumb(Thumbnail::new(
                            format!("C{:02} {:>3}s", sel + 1, i * 100),
                            CAM_COLORS[sel],
                        ));
                    }
                    *f = nf;
                }
                // Nearest still to the playhead stays highlighted.
                f.select(((m.media_pos.get() * 5.0).round() as usize).min(5));
            })
    };

    // Momentary jog stick → axis target (spring returns it to 0).
    let stick = Bound::new(Joystick::new().spring(true).label("JOG"), model).pull(|j, m| {
        if j.take_changed() {
            let (x, y) = j.value_xy();
            m.jog.update(|s| s.axis = (f64::from(x), f64::from(y)));
        }
    });

    // Absolute pan/tilt pad → same axis target (push reflects it back).
    let pad = Bound::new(XYPad::new().labels("PAN", "TILT"), model)
        .pull(|p, m| {
            if p.take_changed() {
                let (x, y) = p.value_xy();
                m.jog.update(|s| s.axis = (f64::from(x), f64::from(y)));
            }
        })
        .push(|p, m| {
            let a = m.jog.get().axis;
            p.set_value(a.0 as f32, a.1 as f32);
        });

    let tilt = Bound::new(
        Slider::new(-1.0, 1.0)
            .label("TILT")
            .with_value(model.jog.get().tilt)
            .step(0.05),
        model,
    )
    .pull(|s, m| {
        if s.is_dragging() {
            let v = s.value();
            m.jog.update(|j| j.tilt = v);
        }
    })
    .push(|s, m| {
        if !s.is_dragging() {
            s.set_value(m.jog.get().tilt);
        }
    });

    let zoom = Bound::new(
        ZoomControls::new()
            .zoom(model.jog.get().zoom as f32)
            .fit(true)
            .reset(true)
            .label("ZOOM"),
        model,
    )
    .pull(|z, m| {
        if let Some(a) = z.take_action() {
            m.jog.update(|j| {
                j.zoom = match a {
                    ZoomAction::ZoomIn => j.zoom * 1.25,
                    ZoomAction::ZoomOut => j.zoom / 1.25,
                    ZoomAction::Fit | ZoomAction::Reset => 1.0,
                }
                .clamp(0.5, 4.0);
            });
        }
    })
    .push(|z, m| z.set_zoom(m.jog.get().zoom as f32));

    let selector = Bound::new(
        Segmented::new()
            .options(["CAM-01", "CAM-02", "CAM-03", "CAM-04"])
            .selected(model.camera_sel.get())
            .label("ACTIVE CAM"),
        model,
    )
    .pull(|s, m| {
        if let Some(i) = s.take_selected() {
            m.camera_sel.set(i.min(CAMERAS.len() - 1));
        }
    })
    .push(|s, m| {
        let sel = m.camera_sel.get().min(CAMERAS.len() - 1);
        if s.selected_index() != sel {
            s.set_selected(sel);
        }
    });

    // PTZ feedback readout — kept under ~32 glyphs so it fits the
    // tightest primary viewport (~326pt at a 700pt zone) inside its
    // own row; the strip's horizontal scroller would shear it.
    let readout = Bound::new(Text::new(""), model).push(|t, m| {
        let j = m.jog.get();
        let sel = m.camera_sel.get().min(CAMERAS.len() - 1);
        t.set_content(format!(
            "PAN {:+.1} TILT {:+.1} ×{:.1} → {}",
            j.axis.0, j.tilt, j.zoom, CAM_SHORT[sel]
        ));
    });

    // Primary — the selected feed (view + PiP) dominant over the
    // camera wall and the stills strip. Every surface sits in a
    // banded row (equal heights per row); the wall gets the full
    // width so its tiles never shrink into clipped captions, and the
    // stills ride a horizontal scroller whose `MinW` mount hands the
    // filmstrip its real content width (`measure` caps at 320 pt).
    let primary = Flex::column()
        .gap(ZONE_STACK)
        .child(strip().child_flex(readout, 1.0))
        .child(
            row()
                .child_flex(band(BAND_L, view), 2.0)
                .child_flex(band(BAND_L, framed(1.5, pip)), 1.0),
        )
        .child(row().child_flex(band(BAND_M, wall), 1.0))
        .child(row().child_flex(
            band(
                BAND_S,
                ScrollView::horizontal(MinW::new(STILLS_W_PT, stills)),
            ),
            1.0,
        ));

    // Rail — the PTZ cluster steers `camera_sel`'s feed via `jog`;
    // the recordings archive below selects `selected_recording` for
    // the TRANSPORT page.
    let ptz = Flex::column()
        .gap(ZONE_GAP)
        .child(stick)
        .child(pad)
        .child(tilt)
        .child(zoom);
    let recordings = {
        let build = |m: &PlantModel| {
            ListView::new()
                .items(
                    m.recordings
                        .get()
                        .iter()
                        .map(|r| format!("{} · {}:{:02}", r.title, r.secs / 60, r.secs % 60)),
                )
                .selection_mode(SelectionMode::Single)
                .label("recordings")
        };
        let mut last = model.recordings.get().len();
        Bound::new(build(model), model)
            .pull(|w: &mut ListView, m| {
                if let Some(i) = w.take_activated().or_else(|| w.selected()) {
                    if let Some(r) = m.recordings.get().get(i) {
                        m.selected_recording.set_if_changed(Some(r.id));
                    }
                }
            })
            .push(move |w: &mut ListView, m| {
                let n = m.recordings.get().len();
                if n != last {
                    last = n;
                    *w = build(m);
                }
                if let Some(i) = m
                    .selected_recording
                    .get()
                    .and_then(|id| m.recordings.get().iter().position(|r| r.id == id))
                {
                    w.set_selected(i);
                }
            })
    };
    // "Record clip" — files a new archive entry for the selected
    // camera (the sim's 30s manual clip).
    let record = Bound::new(Button::new("● Record clip"), model).pull(|w: &mut Button, m| {
        if w.take_activated() {
            let cam = m.camera_sel.get();
            let id = m.add_recording("manual clip", cam, 30);
            m.selected_recording.set_if_changed(Some(id));
            m.log_ch(
                0,
                format!("recording {id} started on camera {cam}"),
                LogChannel::System,
                None,
            );
        }
    });
    let rail_col = Flex::column()
        .gap(ZONE_STACK)
        .child(GroupBox::new("PTZ").child(ptz))
        .child_flex(
            GroupBox::new("RECORDINGS").child(
                Flex::column()
                    .gap(ZONE_GAP)
                    .child(record)
                    .child_flex(recordings, 1.0),
            ),
            1.0,
        );

    // Banded surfaces stack past a short zone's height — scroll-mount
    // so the wall/stills stay reachable instead of crushing to zero.
    Page::new(
        Variant::MasterDetail,
        fill(scroll(CapW::new(SCROLL_CAP_PT, primary))),
        &model.zone_width[3],
    )
    .strip(strip().child(selector).child_flex(DummyWidget, 1.0))
    .rail("Camera", CapW::new(SCROLL_CAP_PT, rail_col))
}

// ---------------------------------------------------------------------------
// TRANSPORT — the loop monitor. MediaControls/Slider/Volume own the
// three media_* signals; Playlist/Coverflow/Carousel are three views
// of the same recording inventory (all load via `load_recording`);
// the rate Dropdown feeds the playhead accumulator.
// ---------------------------------------------------------------------------
/// TRANSPORT — "play this loop, watch it here." MasterLeft: the
/// playlist is the chooser (writes `camera_sel` via `load_recording`);
/// the artifact is the transport console — controls + now-playing +
/// seek/volume/rate over a bounded browser lens (Covers | Reel).
fn transport_page(model: &PlantModel, rate: Signal<f64>) -> Page {
    let mut last_tick = Instant::now();
    // One shared cell for both mute paths — MediaControls and Volume
    // each stash the pre-mute level here, so un-muting from either
    // widget restores what the other muted (twin cells diverge).
    let saved_gain = Arc::new(Mutex::new(model.media_vol.get()));
    let saved_gain_mc = Arc::clone(&saved_gain);
    let rate_pull = rate.clone();
    let controls = Bound::new(
        MediaControls::new()
            .duration(LOOP_SECS)
            .position(0.0)
            .volume(model.media_vol.get() as f32),
        model,
    )
    .pull(move |mc, m| {
        let dt = last_tick.elapsed().as_secs_f64();
        last_tick = Instant::now();
        if mc.take_play_toggled() {
            m.media_playing.set(mc.playing());
        }
        if let Some(secs) = mc.take_seek() {
            m.media_pos.set((secs / LOOP_SECS).clamp(0.0, 1.0));
        }
        if let Some(v) = mc.take_volume() {
            m.media_vol.set(f64::from(v));
        }
        if mc.take_mute_toggled() {
            let mut saved = saved_gain_mc.lock();
            if mc.muted() {
                // Positive level only — saving a zeroed `media_vol`
                // would clobber the level un-mute restores.
                if m.media_vol.get() > 0.0 {
                    *saved = m.media_vol.get();
                    m.media_vol.set(0.0);
                }
            } else if m.media_vol.get() <= 0.0 {
                m.media_vol.set(saved.max(0.1));
            }
        }
        mc.take_fullscreen(); // fullscreen is OS chrome — no model field, drain
                              // The playhead advances here — owning the phase accumulator
                              // keeps `media_pos` honest (rate-scaled); pause freezes the
                              // whole sim, playhead included.
        if m.media_playing.get() && !m.paused.get() {
            let p = (m.media_pos.get() + dt * rate_pull.get() / LOOP_SECS) % 1.0;
            m.media_pos.set(p);
        }
    })
    .push(|mc, m| {
        mc.set_playing(m.media_playing.get());
        mc.set_position(m.media_pos.get() * LOOP_SECS);
        mc.set_muted(m.media_vol.get() <= 0.0);
    });

    let now_playing = Bound::new(
        // Subtitle uses the short id — the lane beside the 44 pt art
        // block fits ~12 chars at the narrowest probed primary.
        NowPlaying::new(LOOPS[0], CAM_SHORT[0])
            .album("Recorded loop")
            .duration(LOOP_SECS as f32)
            .art_color(CAM_COLORS[0]),
        model,
    )
    .pull(|n, m| {
        if n.take_clicked() {
            m.media_playing.set(!m.media_playing.get());
        }
    })
    .push({
        let mut last_sel = model.camera_sel.get().min(CAMERAS.len() - 1);
        move |n, m| {
            // Only the track re-seat is gated — it drops click/press
            // state; the playing/position setters are cheap writes.
            let sel = m.camera_sel.get().min(CAMERAS.len() - 1);
            if sel != last_sel {
                last_sel = sel;
                *n = NowPlaying::new(LOOPS[sel], CAM_SHORT[sel])
                    .album("Recorded loop")
                    .duration(LOOP_SECS as f32)
                    .art_color(CAM_COLORS[sel]);
            }
            n.set_playing(m.media_playing.get());
            n.set_position((m.media_pos.get() * LOOP_SECS) as f32);
        }
    });

    let seek = Bound::new(
        Slider::new(0.0, 1.0).label("LOOP POSITION").step(0.001),
        model,
    )
    .pull(|s, m| {
        if s.is_dragging() {
            m.media_pos.set(s.value().clamp(0.0, 1.0));
        }
    })
    .push(|s, m| {
        if !s.is_dragging() {
            s.set_value(m.media_pos.get());
        }
    });

    let saved_gain_vol = Arc::clone(&saved_gain);
    let volume = Bound::new(
        Volume::new()
            .gain(model.media_vol.get() as f32)
            .max(1.0)
            .label("MONITOR GAIN"),
        model,
    )
    .pull(move |v, m| {
        if let Some(g) = v.take_changed() {
            m.media_vol.set(f64::from(g));
        }
        if let Some(muted) = v.take_muted() {
            let mut saved = saved_gain_vol.lock();
            if muted {
                // Stash a positive level only — a model-side mute
                // already zeroed `media_vol`, and saving that would
                // clobber the level un-mute restores.
                if m.media_vol.get() > 0.0 {
                    *saved = m.media_vol.get();
                    m.media_vol.set(0.0);
                }
            } else if m.media_vol.get() <= 0.0 {
                m.media_vol.set(saved.max(0.1));
            }
        }
    })
    .push(|v, m| {
        // `media_vol == 0` is the model's mute (MediaControls reads it
        // the same way). Never set_gain while muted — that un-mutes
        // and re-flags `changed`, undoing the widget's own press.
        let target = m.media_vol.get() as f32;
        v.set_muted(target <= 0.0);
        if !v.is_muted() && (v.gain_value() - target).abs() > 1e-3 {
            v.set_gain(target);
        }
    });

    let mut last_sel: Option<usize> = None;
    let mut list = Playlist::new().label("CAMERA LOOPS");
    for (i, title) in LOOPS.iter().enumerate() {
        // Subtitle = short camera id — the master rail narrows to
        // ~110 pt of subtitle lane at the 1200 px sweep width.
        list = list.track(Track::new(*title, CAM_SHORT[i]).duration(LOOP_SECS as u32));
    }
    // Row → canonical-loop permutation: `Playlist::event` physically
    // reorders `tracks`, so post-reorder row indices (`selected`,
    // `current`, `take_moved`) are widget-order while `LOOPS`/
    // `camera_sel` are canonical — every canonical lookup goes
    // through this map.
    let order = Arc::new(Mutex::new((0..LOOPS.len()).collect::<Vec<usize>>()));
    let order_pull = Arc::clone(&order);
    let order_push = Arc::clone(&order);
    let playlist = Bound::new(list, model)
        .pull(move |p, m| {
            let mut order = order_pull.lock();
            // Drain moves BEFORE reading `selected()` — the widget
            // remaps `selected` to follow the moved track, so a
            // pre-move `order` would resolve the wrong canonical loop
            // and fire a spurious `load_recording`.
            while let Some((from, to)) = p.take_moved() {
                let moved = if from < order.len() {
                    let c = order.remove(from);
                    let to = to.min(order.len());
                    order.insert(to, c);
                    Some(c)
                } else {
                    None
                };
                m.log(
                    0,
                    format!(
                        "loops reordered — {} → position {}",
                        moved.and_then(|c| LOOPS.get(c)).copied().unwrap_or("?"),
                        to + 1
                    ),
                );
            }
            let sel = p
                .selected()
                .map(|row| order.get(row).copied().unwrap_or(row));
            if sel != last_sel {
                last_sel = sel;
                if let Some(i) = sel {
                    load_recording(m, i);
                }
            }
        })
        .push(move |p, m| {
            let sel = m.camera_sel.get().min(LOOPS.len() - 1);
            // Canonical → row through the permutation — the marker
            // lands on the track's current row, not its slot. (The
            // widget's own n/p keys move `current` without a take
            // channel, so the push re-snaps it — model wins.)
            let row = order_push
                .lock()
                .iter()
                .position(|&c| c == sel)
                .unwrap_or(sel);
            if p.current() != Some(row) {
                p.set_current(row);
            }
        });

    let mut flow = Coverflow::new().label("LOOPS");
    for (i, title) in LOOPS.iter().enumerate() {
        flow = flow.item(Thumbnail::new(*title, CAM_COLORS[i]));
    }
    let coverflow = Bound::new(flow, model)
        .pull(|c, m| {
            if let Some(i) = c.take_selected() {
                load_recording(m, i);
            }
        })
        .push(|c, m| c.set_selected(m.camera_sel.get().min(LOOPS.len() - 1)));

    let mut carousel = Carousel::new().wrap(true).label("RECENT CLIPS");
    for title in LOOPS {
        carousel = carousel.page(Text::new(title));
    }
    let carousel = Bound::new(carousel, model)
        .pull(|c, m| {
            if let Some(i) = c.take_navigated() {
                load_recording(m, i);
            }
        })
        .push(|c, m| {
            let sel = m.camera_sel.get().min(LOOPS.len() - 1);
            if c.current() != sel {
                c.go_to(sel);
            }
        });

    let mut last_idx = 1usize; // "1×"
    let rate_push = rate.clone();
    let mut rate_dd = Dropdown::new(["0.5×", "1×", "2×", "4×"]).label("RATE");
    rate_dd.commit(1);
    let rate_menu = Bound::new(rate_dd, model)
        .pull(move |d, _| {
            let i = d.selected();
            if i != last_idx {
                last_idx = i;
                rate.set([0.5, 1.0, 2.0, 4.0][i.min(3)]);
            }
        })
        .push(move |d, _| {
            let want = match rate_push.get() {
                r if r < 0.75 => 0,
                r if r < 1.5 => 1,
                r if r < 3.0 => 2,
                _ => 3,
            };
            if d.selected() != want && !d.is_open() {
                d.commit(want);
            }
        });

    // Browser lens — two presentations of the same loop inventory;
    // both write `camera_sel` through `load_recording`.
    let lens_sel = Signal::new(0usize);
    let lens = {
        let ls = lens_sel.clone();
        Bound::new(
            Segmented::new()
                .options(["Covers", "Reel"])
                .selected(0)
                .label("browser"),
            model,
        )
        .pull(move |w: &mut Segmented, _m| {
            if let Some(i) = w.take_selected() {
                ls.set_if_changed(i);
            }
        })
    };
    let browser = Swap::new(&lens_sel).view(coverflow).view(carousel);

    // Every transport control owns its own strip row — pairing them
    // side-by-side splits ~322 pt primaries into lanes narrower than
    // either widget's fixed text fields (time lanes, track title)
    // and the edges clip mid-glyph. Stacked, each keeps full width.
    let primary = Flex::column()
        .gap(ZONE_STACK)
        .child(strip().child_flex(controls, 1.0))
        .child(strip().child_flex(now_playing, 1.0))
        .child(strip().child_flex(seek, 1.0))
        .child(strip().child_flex(volume, 1.0).child(rate_menu))
        .child_flex(browser, 1.0);

    Page::new(
        Variant::MasterLeft,
        fill(scroll(CapW::new(SCROLL_CAP_PT, primary))),
        &model.zone_width[3],
    )
    .strip(strip().child(lens).child_flex(DummyWidget, 1.0))
    .rail("Loops", CapW::new(SCROLL_CAP_PT, playlist))
}

// ---------------------------------------------------------------------------
// ACOUSTIC MONITOR — line-noise condition monitoring. Spectrum/
// Waveform/VuMeter render `acoustic` through the monitor-path trims
// + monitor gain; Equalizer writes those trims; LevelBar/StatusDot/
// Text carry loudness + line state + dominant frequency.
// ---------------------------------------------------------------------------
/// ACOUSTIC — "what is the line *saying*?" Wall — spectrum, waveform,
/// VU, and EQ over the shared `acoustic` model: one coherent
/// condition-monitoring surface.
fn acoustic_page(model: &PlantModel, eq_trim: Signal<Vec<f64>>) -> Page {
    let eq = eq_trim.clone();
    let spectrum = Bound::new(
        Spectrum::new().peak_hold(true).label("LINE SPECTRUM"),
        model,
    )
    .push(move |s, m| {
        let a = m.acoustic.get();
        let t = eq.get();
        s.set_bands(a.bands.iter().zip(t.iter()).map(|(b, g)| (b * g) as f32));
    });

    let eq2 = eq_trim.clone();
    let wave = Bound::new(Waveform::new().label("LINE NOISE"), model).push({
        let mut last: Vec<f32> = Vec::new();
        move |w, m| {
            let a = m.acoustic.get();
            let t = eq2.get();
            // Mirrored spectrum = the waveform envelope (32 peaks).
            let peaks: Vec<f32> = a
                .bands
                .iter()
                .zip(t.iter())
                .map(|(b, g)| (b * g) as f32)
                .chain(
                    a.bands
                        .iter()
                        .zip(t.iter())
                        .rev()
                        .map(|(b, g)| (b * g) as f32),
                )
                .collect();
            // Re-seat only when the envelope actually moved — the
            // widget has no peaks setter.
            if peaks != last {
                *w = Waveform::new()
                    .peaks(peaks.iter().copied())
                    .label("LINE NOISE");
                last = peaks;
            }
        }
    });

    let eq3 = eq_trim.clone();
    let meter = Bound::new(
        VuMeter::new()
            .channels(2)
            .peak_hold(1.5)
            .label("LINE LEVEL"),
        model,
    )
    .push(move |v, m| {
        let a = m.acoustic.get();
        let t = eq3.get();
        let vol = m.media_vol.get();
        let trim = |band: usize| t.get(band).copied().unwrap_or(1.0);
        // Stereo level through monitor gain + the band trims feeding
        // each channel (bands 2/5 drive the model's level pair).
        v.push([
            (a.level.0 * vol * trim(2)) as f32,
            (a.level.1 * vol * trim(5)) as f32,
        ]);
    });

    let eq4 = eq_trim.clone();
    let equalizer = Bound::new(
        Equalizer::new()
            .faders(16)
            .bands(vec![0.5; 16])
            .label("MONITOR EQ"),
        model,
    )
    .pull(move |e, _| {
        // EQ gains are 0..1 with 0.5 = unity → trim 0..2. Pull-only:
        // set_gain would re-flag `take_changed`, and trims live here.
        if e.take_changed() {
            eq4.set(e.gains().iter().map(|g| f64::from(*g) * 2.0).collect());
        }
    });

    let loudness = Bound::new(LevelBar::new().segments(12).value(0.0), model).push(|l, m| {
        let a = m.acoustic.get();
        l.set_value(((a.level.0 + a.level.1) * 0.5 * m.media_vol.get()) as f32);
    });

    let line = Bound::new(StatusDot::new("LINE DRIVE").status(Status::Ok), model).push(|d, m| {
        d.set_status(if m.line_running.get() {
            Status::Ok
        } else {
            Status::Warning
        });
    });

    let freq = Bound::new(Text::new(""), model).push(|t, m| {
        let a = m.acoustic.get();
        t.set_content(format!(
            "DOMINANT {:>6.1} Hz — {}",
            a.dominant_hz,
            if m.line_running.get() {
                "line running"
            } else {
                "line stopped — ambient floor"
            }
        ));
    });

    let primary = Flex::column()
        .gap(ZONE_STACK)
        .child(
            row()
                .child_flex(band(BAND_M, spectrum), 1.0)
                .child_flex(band(BAND_M, wave), 1.0),
        )
        .child(
            row()
                .child_flex(band(BAND_M, meter), 1.0)
                .child_flex(band(BAND_M, equalizer), 2.0)
                .child(loudness),
        )
        .child(strip().child(line).child_flex(freq, 1.0));
    // Two meter bands + the status strip — scroll-mounted so a short
    // zone scrolls instead of crushing the strip.
    Page::new(
        Variant::Wall,
        fill(scroll(CapW::new(SCROLL_CAP_PT, primary))),
        &model.zone_width[3],
    )
}

// ---------------------------------------------------------------------------
// ALARM TONES — the annunciator programmer. `acoustic.tone_*` is
// widget-owned state (tick_acoustic never touches it): PianoKeys set
// the note, StepSequencer the pattern, Metronome the tempo,
// Fretboard/Tuner the readouts.
// ---------------------------------------------------------------------------
/// The annunciator tone bench — mounted as SYSTEM's bounded
/// secondary rail (Telemetry zone), not a standalone page.
pub(crate) fn tones_bench(model: &PlantModel) -> Flex {
    // PianoKeys index 48 = C4 (MIDI 60) → note = index + 12. Four
    // octaves covers the annunciator's usable range (incl. seed A3).
    let keys = Bound::new(PianoKeys::new().octaves(4).label("TONE NOTE"), model).pull(|k, m| {
        if let Some(i) = k.take_struck() {
            m.acoustic.update(|a| a.tone_note = (i + 12).min(127) as u8);
        }
    });

    let mut seq = StepSequencer::new(1, 8)
        .lanes(["TONE"])
        .label("TONE PATTERN");
    {
        let pat = model.acoustic.get().tone_pattern;
        for (i, on) in pat.iter().enumerate() {
            if *on {
                seq = seq.cells_on([(0, i)]);
            }
        }
    }
    let seq = Bound::new(seq, model)
        .pull(|s, m| {
            if let Some((_, col, on)) = s.take_changed() {
                if col < 8 {
                    m.acoustic.update(|a| a.tone_pattern[col] = on);
                }
            }
            s.take_step(); // playhead ticks — drain
        })
        .push(|s, m| {
            let a = m.acoustic.get();
            for (i, &on) in a.tone_pattern.iter().enumerate() {
                s.set_cell(0, i, on);
            }
            // The preview runs while the line runs — annunciator
            // only sounds a running plant. Transition only:
            // set_playing(true) re-arms the step clock.
            let run = m.line_running.get();
            if s.is_playing() != run {
                s.set_playing(run);
            }
        });

    let fret = Bound::new(fretboard_of(model.acoustic.get().tone_note), model)
        .pull(|f, m| {
            if let Some((s, fr)) = f.take_edited() {
                if (s as usize) < OPEN_TUNING.len() {
                    m.acoustic
                        .update(|a| a.tone_note = OPEN_TUNING[s as usize].saturating_add(fr));
                }
            }
        })
        .push({
            // Re-seat only when the programmed note moves — rebuilding
            // per tick drops in-flight edit presses.
            let mut last = model.acoustic.get().tone_note;
            move |f, m| {
                let note = m.acoustic.get().tone_note;
                if note != last {
                    last = note;
                    *f = fretboard_of(note);
                }
            }
        });

    // Tap-tempo changes `bpm_value`; mirror it into tone_bpm. No
    // setter exists model→widget, so the last-seen value guards the
    // direction (Metronome is the only tone_bpm writer anyway).
    let mut last_bpm = u32::from(model.acoustic.get().tone_bpm);
    let metro = Bound::new(
        Metronome::new()
            .bpm(u32::from(model.acoustic.get().tone_bpm))
            .beats(8)
            .running()
            .label("TEMPO"),
        model,
    )
    .pull(move |mt, m| {
        let cur = mt.bpm_value();
        if cur != last_bpm {
            last_bpm = cur;
            m.acoustic.update(|a| a.tone_bpm = cur as u16);
        }
        mt.take_beat(); // beat flash — no model field, drain
    });

    // Tuner shows how far the line's dominant tone sits from the
    // programmed alarm note — the annunciator-vs-plant diagnostic.
    let tuner = Bound::new(Tuner::new().label("LINE vs TONE"), model).push(|t, m| {
        let a = m.acoustic.get();
        let cents = (1200.0 * (a.dominant_hz / note_hz(a.tone_note)).log2()).clamp(-50.0, 50.0);
        t.set_pitch(note_name(a.tone_note), cents as f32);
    });

    let program = Bound::new(Text::new(""), model).push(|t, m| {
        let a = m.acoustic.get();
        let pat: String = a
            .tone_pattern
            .iter()
            .map(|&b| if b { '●' } else { '○' })
            .collect();
        t.set_content(format!(
            "PROGRAMMED  {}  ·  {} BPM  ·  {}",
            note_name(a.tone_note),
            a.tone_bpm,
            pat
        ));
    });

    Flex::column()
        .gap(ZONE_STACK)
        .child(
            row()
                .child_flex(band(BAND_M, keys), 1.0)
                .child_flex(band(BAND_M, seq), 2.0),
        )
        .child(
            row()
                .child_flex(band(BAND_M, fret), 1.0)
                .child(metro)
                .child_flex(band(BAND_M, tuner), 1.0),
        )
        .child(strip().child_flex(program, 1.0))
}

// ---------------------------------------------------------------------------
// INSPECTION PHOTOS — procedural per-asset stills. Image/ImageViewer
// follow `selected_asset`; Lightbox browses cells and publishes its
// navigation back to `selected_asset`.
// ---------------------------------------------------------------------------
/// Per-asset inspection photos — mounted as DETAIL's "PHOTOS"
/// dossier lens (Process Grid zone), not a standalone page.
pub(crate) fn inspection_photos(model: &PlantModel) -> Flex {
    let sel = model.selected_asset.get();
    let sel_asset = sel.and_then(|id| model.asset(id));

    // `(selection, assets_sig)` gates — a property-grid rename must
    // refresh the alt text even when the selected id is unchanged.
    let mut last_thumb = (sel, assets_sig(model));
    let thumb = Bound::new(
        Image::new(inspection_photo(sel.unwrap_or(0), 48, 32)).alt(
            sel_asset
                .as_ref()
                .map(|a| a.name)
                .unwrap_or("No asset selected"),
        ),
        model,
    )
    .push(move |im, m| {
        let cur = (m.selected_asset.get(), assets_sig(m));
        if cur != last_thumb {
            last_thumb = cur;
            let a = cur.0.and_then(|id| m.asset(id));
            *im = Image::new(inspection_photo(cur.0.unwrap_or(0), 48, 32))
                .alt(a.as_ref().map(|x| x.name).unwrap_or("No asset selected"));
        }
    });

    let mut last_view = (sel, assets_sig(model));
    let viewer = Bound::new(
        ImageViewer::new(inspection_photo(sel.unwrap_or(0), 96, 64)).label(
            sel_asset
                .as_ref()
                .map(|a| a.name)
                .unwrap_or("No asset selected"),
        ),
        model,
    )
    .push(move |v, m| {
        let cur = (m.selected_asset.get(), assets_sig(m));
        if cur != last_view {
            last_view = cur;
            let a = cur.0.and_then(|id| m.asset(id));
            *v = ImageViewer::new(inspection_photo(cur.0.unwrap_or(0), 96, 64))
                .label(a.as_ref().map(|x| x.name).unwrap_or("No asset selected"));
        }
    });

    // Shared builder — the mount and the `assets_sig` re-seat both
    // need it (thumbs carry names + status colors that asset edits
    // change; Lightbox has no item mutator).
    let build_lightbox = |m: &PlantModel| {
        let mut lb = Lightbox::new().label("CELL STILLS");
        for a in cells(m) {
            lb = lb.item(
                Thumbnail::new(a.name, status_color(a.status)).status(thumb_status(a.status)),
            );
        }
        lb
    };
    let mut last_items = assets_sig(model);
    let lightbox = Bound::new(build_lightbox(model), model)
        .pull(|l, m| {
            if let Some(i) = l.take_navigated() {
                let cs = cells(m);
                if let Some(a) = cs.get(i) {
                    m.selected_asset.set(Some(a.id));
                }
            }
            l.take_closed(); // viewer dismiss — no model field, drain
        })
        .push(move |l, m| {
            let sig = assets_sig(m);
            if sig != last_items {
                last_items = sig;
                let cur = l.current();
                *l = build_lightbox(m);
                if let Some(i) = cur {
                    l.set_index(i.min(cells(m).len().saturating_sub(1)));
                }
            }
            if let Some(sel) = m.selected_asset.get() {
                let cs = cells(m);
                if let Some(i) = cs.iter().position(|a| a.id == sel) {
                    if l.current() != Some(i) {
                        l.set_index(i);
                    }
                }
            }
        });

    Flex::column()
        .gap(ZONE_STACK)
        .child(
            row()
                .child_flex(band(BAND_L, framed(1.5, thumb)), 1.0)
                .child_flex(band(BAND_L, viewer), 2.0),
        )
        .child(row().child_flex(band(BAND_M, lightbox), 1.0))
}

// ---------------------------------------------------------------------------
// Tests.
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use martensite::core::Widget;
    use std::time::Duration;

    fn model() -> PlantModel {
        PlantModel::seeded(
            Signal::new(0.5),
            Signal::new(0.5),
            Signal::new(false),
            Signal::new(true),
            Signal::new(String::new()),
        )
    }

    #[test]
    fn pages_are_domain_named() {
        let m = model();
        let pgs = pages(&m);
        let names: Vec<&str> = pgs.iter().map(|(n, _)| *n).collect();
        assert_eq!(
            names,
            ["COMMS", "ROOMS", "CAMERAS", "TRANSPORT", "ACOUSTIC",]
        );
    }

    /// Paint-list survey: walk every media page's frames across the
    /// dump widths and report *mid-glyph horizontal clips* — a glyph
    /// or text run whose ink starts inside its active clip rect but
    /// lands past the clip's left/right edge. That is the hard-clip
    /// defect class: content cut at a boundary with no way to reach
    /// it. The clip owner is printed so sanctioned scrollport cuts
    /// (`ScrollView` — the designed overflow affordance) read apart
    /// from true hard clips (bubble/segment/card edges).
    /// `cargo test -p industrial_dashboard probe_mid_glyph -- --nocapture`.
    #[test]
    fn probe_mid_glyph_clips() {
        use martensite::core::paint::PaintCommand;
        use martensite::core::{LayoutContext, PaintList};
        use martensite::prelude::*;
        use martensite::widgets::container::Container;
        use martensite::widgets::scrollview::ScrollView;

        fn tick_all(w: &mut dyn martensite::core::Widget, dt: Duration) {
            let _ = w.tick(dt);
            for i in 0..w.child_count() {
                if w.child_bounds(i).is_none() {
                    continue;
                }
                if let Some(c) = w.child_mut(i) {
                    tick_all(c, dt);
                }
            }
        }

        // Flag one ink span `[x0,x1]` against the active clip stack;
        // a span beginning inside the effective clip but landing past
        // its left/right edge is a mid-glyph cut.
        fn note_hit(
            clips: &[(kurbo::Rect, String)],
            scopes: &[String],
            zw: f32,
            x0: f64,
            x1: f64,
            what: &str,
            hits: &mut std::collections::BTreeMap<String, usize>,
        ) {
            let mut eff = kurbo::Rect::new(0.0, 0.0, zw as f64, 350.0);
            for (r, _) in clips {
                eff = eff.intersect(*r);
            }
            let (side, edge) = if x0 < eff.max_x() - 1.0 && x1 > eff.max_x() + 1.0 {
                ("R", eff.max_x())
            } else if x1 > eff.min_x() + 1.0 && x0 < eff.min_x() - 1.0 {
                ("L", eff.min_x())
            } else {
                return;
            };
            let owner = clips
                .iter()
                .rev()
                .find(|(r, _)| {
                    (side == "R" && (r.max_x() - eff.max_x()).abs() < 0.5)
                        || (side == "L" && (r.min_x() - eff.min_x()).abs() < 0.5)
                })
                .map(|(_, n)| n.clone())
                .unwrap_or_else(|| "<frame>".into());
            let key = format!(
                "{side} cut @x{edge:.0} clip_owner={owner} scope={} | {what} x0={x0:.0}",
                scopes.last().map(String::as_str).unwrap_or("?")
            );
            *hits.entry(key).or_insert(0) += 1;
        }

        let _font_guard = crate::frames::install_test_fonts();
        let shaper = crate::frames::FixtureTextShaper::new();
        let m = model();
        m.warm_demo_state();

        for zw in [700.0f32, 1200.0, 1600.0] {
            m.zone_width[3].set(zw / 2.0);
            for (label, page) in pages(&m) {
                let view = ScrollView::new(
                    Container::new()
                        .padding_uniform(crate::zone::ZONE_PAD)
                        .child(page),
                );
                let mut arena = WidgetArena::new();
                arena.set_theme(martensite::theme::tokens::default_dark());
                arena.set_scale_factor(2.0);
                arena.set_text_painter(shaper.clone());
                let mut hot = HotNode::default();
                hot.flags |= NodeFlags::VISIBLE;
                let root = arena.insert_with_widget(hot, Box::new(view));
                let bounds = Rect::new(0.0, 0.0, zw, 350.0);
                if let Some((hot, cold)) = arena.get_both_mut(root) {
                    hot.bounds = bounds;
                    cold.widget
                        .layout(&mut LayoutContext { hot, scale: 2.0 }, bounds);
                }
                if let Some(cold) = arena.get_cold_mut(root) {
                    tick_all(&mut *cold.widget, Duration::from_millis(16));
                }
                let content_h = arena
                    .get_cold(root)
                    .and_then(|c| c.widget.child_bounds(0))
                    .map(|b| b.height())
                    .unwrap_or(0.0);
                let mut y = 0.0f32;
                let mut hits: std::collections::BTreeMap<String, usize> =
                    std::collections::BTreeMap::new();
                loop {
                    arena.dispatch_event(
                        root,
                        &WidgetEvent::SemanticAction(SemanticAction::SetScrollOffset(
                            glam::Vec2::new(0.0, y),
                        )),
                    );
                    let mut list = PaintList::new();
                    arena.build_paint_list(root, &mut list);
                    // Walk the paint list tracking the scope path and
                    // the clip stack; flag runs cut mid-glyph on the
                    // clip's left/right edge.
                    let mut scopes: Vec<String> = Vec::new();
                    let mut clips: Vec<(kurbo::Rect, String)> = Vec::new();
                    for cmd in &list.commands {
                        match cmd {
                            PaintCommand::PushScope { name, .. } => scopes.push(name.to_string()),
                            PaintCommand::PopScope => {
                                scopes.pop();
                            }
                            PaintCommand::ClipRect(r) => {
                                clips.push((*r, scopes.last().cloned().unwrap_or_default()));
                            }
                            PaintCommand::ClipRoundedRect(r, _) => {
                                clips.push((*r, scopes.last().cloned().unwrap_or_default()));
                            }
                            PaintCommand::ClipPath(_) => clips.push((
                                kurbo::Rect::new(0.0, 0.0, zw as f64, 350.0),
                                scopes.last().cloned().unwrap_or_default(),
                            )),
                            PaintCommand::PopClip => {
                                clips.pop();
                            }
                            PaintCommand::DrawText(p, s, size, _) => {
                                if s.trim().is_empty() {
                                    continue;
                                }
                                let w = martensite::core::paint::TextShaper::measure_text(
                                    &shaper, s, *size,
                                )
                                .unwrap_or(0.0);
                                note_hit(
                                    &clips,
                                    &scopes,
                                    zw,
                                    p.x,
                                    p.x + w as f64,
                                    &format!("\"{s}\""),
                                    &mut hits,
                                );
                            }
                            PaintCommand::DrawGlyphRun(run) => {
                                let r0 = run.glyphs.first().map(|g| g.x).unwrap_or(0.0);
                                let r1 = run
                                    .glyphs
                                    .iter()
                                    .map(|g| g.x + g.width)
                                    .fold(0.0f32, f32::max);
                                for (gi, g) in run.glyphs.iter().enumerate() {
                                    note_hit(
                                        &clips,
                                        &scopes,
                                        zw,
                                        g.x as f64,
                                        (g.x + g.width) as f64,
                                        &format!(
                                            "run[{gi}/{}] fs={:.0} span={r0:.0}..{r1:.0}",
                                            run.glyphs.len(),
                                            run.font_size
                                        ),
                                        &mut hits,
                                    );
                                }
                            }
                            _ => {}
                        }
                    }
                    if y >= content_h {
                        break;
                    }
                    y += 175.0;
                }
                eprintln!("=== media/{label}@{zw:.0} ===");
                for (k, n) in &hits {
                    eprintln!("  x{n} {k}");
                }
            }
        }
    }

    #[test]
    fn comms_round_trip() {
        let m = model();
        let before = m.shift_log.get().len();
        // The publish path: ChatInput's pull calls m.log(0, text).
        m.log(0, "test round trip");
        assert_eq!(m.shift_log.get().len(), before + 1);
        // …and the bound MessageList renders it on the next tick.
        let mut list = Bound::new(MessageList::new(), &m).push(sync_message_list());
        list.tick(Duration::from_millis(16));
        assert_eq!(list.inner().len(), before + 1);
        // `bubble_line` may append zero-width-space pads so the
        // widget's per-char bubble estimate covers the real ink —
        // strip them for the payload comparison.
        assert_eq!(
            list.inner()
                .message(before)
                .expect("new bubble")
                .body
                .trim_end_matches('\u{200B}'),
            "test round trip"
        );
    }
}
