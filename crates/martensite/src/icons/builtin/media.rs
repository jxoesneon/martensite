//! `media` namespace — transport and audio/video state.

use super::super::{IconEntry, IconPair};

/// Qualified icon names (`media.*`).
///
/// New constants are prefixed `MEDIA_` (`MEDIA_FOO` → `"media.foo"`)
/// so the flattened `builtin::names` re-export cannot collide with
/// leaf names from other namespaces. The original unprefixed six
/// (`PLAY`, `PAUSE`, `VIDEO`, `VOLUME_ON`, `VOLUME_OFF`, `MIC`) are
/// kept for compatibility.
pub mod names {
    /// `"media.play"` — transport start.
    pub const PLAY: &str = "media.play";
    /// `"media.pause"` — transport hold.
    pub const PAUSE: &str = "media.pause";
    /// `"media.video"` — camera / video track.
    pub const VIDEO: &str = "media.video";
    /// `"media.volume-on"` — audible output.
    pub const VOLUME_ON: &str = "media.volume-on";
    /// `"media.volume-off"` — muted output.
    pub const VOLUME_OFF: &str = "media.volume-off";
    /// `"media.mic"` — microphone / audio input.
    pub const MIC: &str = "media.mic";
    /// `"media.stop"` — square stop glyph.
    pub const MEDIA_STOP: &str = "media.stop";
    /// `"media.skip-back"` — previous track / chapter.
    pub const MEDIA_SKIP_BACK: &str = "media.skip-back";
    /// `"media.skip-forward"` — next track / chapter.
    pub const MEDIA_SKIP_FORWARD: &str = "media.skip-forward";
    /// `"media.rewind"` — seek backward fast.
    pub const MEDIA_REWIND: &str = "media.rewind";
    /// `"media.fast-forward"` — seek forward fast.
    pub const MEDIA_FAST_FORWARD: &str = "media.fast-forward";
    /// `"media.repeat"` — loop the queue.
    pub const MEDIA_REPEAT: &str = "media.repeat";
    /// `"media.repeat-1"` — loop the current item.
    pub const MEDIA_REPEAT_1: &str = "media.repeat-1";
    /// `"media.shuffle"` — randomize playback order.
    pub const MEDIA_SHUFFLE: &str = "media.shuffle";
    /// `"media.play-circle"` — circled transport start.
    pub const MEDIA_PLAY_CIRCLE: &str = "media.play-circle";
    /// `"media.pause-circle"` — circled transport hold.
    pub const MEDIA_PAUSE_CIRCLE: &str = "media.pause-circle";
    /// `"media.record"` — record indicator dot.
    pub const MEDIA_RECORD: &str = "media.record";
    /// `"media.volume"` — speaker glyph, no level waves.
    pub const MEDIA_VOLUME: &str = "media.volume";
    /// `"media.volume-1"` — low audible output (one wave).
    pub const MEDIA_VOLUME_1: &str = "media.volume-1";
    /// `"media.volume-2"` — high audible output (two waves).
    pub const MEDIA_VOLUME_2: &str = "media.volume-2";
    /// `"media.mic-off"` — microphone muted.
    pub const MEDIA_MIC_OFF: &str = "media.mic-off";
    /// `"media.headphones"` — headphone output / monitoring.
    pub const MEDIA_HEADPHONES: &str = "media.headphones";
    /// `"media.speaker"` — loudspeaker cabinet.
    pub const MEDIA_SPEAKER: &str = "media.speaker";
    /// `"media.radio"` — broadcast / tuner.
    pub const MEDIA_RADIO: &str = "media.radio";
    /// `"media.music"` — musical note / audio track.
    pub const MEDIA_MUSIC: &str = "media.music";
    /// `"media.audio-lines"` — equalizer bars / audio metering.
    pub const MEDIA_AUDIO_LINES: &str = "media.audio-lines";
    /// `"media.video-off"` — camera / video track disabled.
    pub const MEDIA_VIDEO_OFF: &str = "media.video-off";
    /// `"media.camera"` — still capture.
    pub const MEDIA_CAMERA: &str = "media.camera";
    /// `"media.camera-off"` — still capture disabled.
    pub const MEDIA_CAMERA_OFF: &str = "media.camera-off";
    /// `"media.image"` — picture / artwork.
    pub const MEDIA_IMAGE: &str = "media.image";
    /// `"media.image-off"` — picture hidden / unavailable.
    pub const MEDIA_IMAGE_OFF: &str = "media.image-off";
    /// `"media.film"` — film strip / movie.
    pub const MEDIA_FILM: &str = "media.film";
    /// `"media.clapperboard"` — production slate.
    pub const MEDIA_CLAPPERBOARD: &str = "media.clapperboard";
    /// `"media.cast"` — cast to a remote display.
    pub const MEDIA_CAST: &str = "media.cast";
    /// `"media.airplay"` — stream to an AirPlay receiver.
    pub const MEDIA_AIRPLAY: &str = "media.airplay";
    /// `"media.picture-in-picture"` — floating mini player.
    pub const MEDIA_PICTURE_IN_PICTURE: &str = "media.picture-in-picture";
    /// `"media.projector"` — video projector.
    pub const MEDIA_PROJECTOR: &str = "media.projector";
}

/// `"media.play"` — filled-outline triangle (stroke idiom).
pub const MEDIA_PLAY: &str = "M8 5v14l11-7z";
/// `"media.pause"` — twin bars.
pub const MEDIA_PAUSE: &str = "M7 5h4v14H7zM13 5h4v14h-4z";
/// `"media.video"` — body rect plus lens trapezoid.
pub const MEDIA_VIDEO: &str =
    "M4 6h10a2 2 0 012 2v8a2 2 0 01-2 2H4a2 2 0 01-2-2V8a2 2 0 012-2zM22 8l-6 4 6 4V8z";
/// `"media.volume-on"` — speaker plus two sound waves.
pub const MEDIA_VOLUME_ON: &str =
    "M4 10v4h4l5 4V6L8 10H4zM15.5 9.5c1.3 1.1 1.3 3.9 0 5M18.5 7c2.3 2.2 2.3 7.8 0 10";
/// `"media.volume-off"` — speaker plus strike cross.
pub const MEDIA_VOLUME_OFF: &str = "M4 10v4h4l5 4V6L8 10H4zM16 9l5 6M21 9l-5 6";
/// `"media.mic"` — capsule, cradle arc, stem, foot.
pub const MEDIA_MIC: &str =
    "M12 2a3 3 0 013 3v6a3 3 0 01-6 0V5a3 3 0 013-3zM19 10v1a7 7 0 01-14 0v-1M12 18v4M8 22h8";
/// `"media.stop"` — rounded square.
pub const MEDIA_STOP: &str = "M6 4h12a2 2 0 012 2v12a2 2 0 01-2 2H6a2 2 0 01-2-2V6a2 2 0 012-2z";
/// `"media.skip-back"` — left triangle plus bar.
pub const MEDIA_SKIP_BACK: &str = "M19 5L9 12l10 7zM5 5v14";
/// `"media.skip-forward"` — right triangle plus bar.
pub const MEDIA_SKIP_FORWARD: &str = "M5 5l10 7-10 7zM19 5v14";
/// `"media.rewind"` — twin left triangles.
pub const MEDIA_REWIND: &str = "M11 19L2 12l9-7v14zM22 19l-9-7 9-7v14z";
/// `"media.fast-forward"` — twin right triangles.
pub const MEDIA_FAST_FORWARD: &str = "M2 5l9 7-9 7V5zM13 5l9 7-9 7V5z";
/// `"media.repeat"` — chasing horizontal arrows.
pub const MEDIA_REPEAT: &str =
    "M17 2l4 4-4 4M3 11v-1a4 4 0 014-4h14M7 22l-4-4 4-4M21 13v1a4 4 0 01-4 4H3";
/// `"media.repeat-1"` — repeat arrows plus a `1` mark.
pub const MEDIA_REPEAT_1: &str =
    "M17 2l4 4-4 4M3 11v-1a4 4 0 014-4h14M7 22l-4-4 4-4M21 13v1a4 4 0 01-4 4H3M11 10h1v4";
/// `"media.shuffle"` — crossing paths between two arrows.
pub const MEDIA_SHUFFLE: &str =
    "M18 14l4 4-4 4M18 2l4 4-4 4M2 18h2a4 4 0 003.3-1.7l5.4-8.6a4 4 0 013.3-1.7H22M2 6h2a4 4 0 013.6 2.2M22 18h-6a4 4 0 01-3.3-1.8l-.4-.5";
/// `"media.play-circle"` — triangle inside a ring.
pub const MEDIA_PLAY_CIRCLE: &str = "M21 12a9 9 0 11-18 0 9 9 0 0118 0zM10 8l6 4-6 4V8z";
/// `"media.pause-circle"` — twin bars inside a ring.
pub const MEDIA_PAUSE_CIRCLE: &str = "M21 12a9 9 0 11-18 0 9 9 0 0118 0zM10 15V9M14 15V9";
/// `"media.record"` — centered dot ring.
pub const MEDIA_RECORD: &str = "M20 12a8 8 0 11-16 0 8 8 0 0116 0z";
/// `"media.volume"` — speaker glyph alone.
pub const MEDIA_VOLUME: &str = "M4 10v4h4l5 4V6L8 10H4z";
/// `"media.volume-1"` — speaker plus one sound wave.
pub const MEDIA_VOLUME_1: &str = "M4 10v4h4l5 4V6L8 10H4zM15.5 9.5c1.3 1.1 1.3 3.9 0 5";
/// `"media.volume-2"` — speaker plus two sound waves (same geometry
/// as [`MEDIA_VOLUME_ON`]).
pub const MEDIA_VOLUME_2: &str = MEDIA_VOLUME_ON;
/// `"media.mic-off"` — mic geometry plus strike slash.
pub const MEDIA_MIC_OFF: &str =
    "M12 2a3 3 0 013 3v6a3 3 0 01-6 0V5a3 3 0 013-3zM19 10v1a7 7 0 01-14 0v-1M12 18v4M8 22h8M2 2l20 20";
/// `"media.headphones"` — headband arc plus two earcups.
pub const MEDIA_HEADPHONES: &str =
    "M3 14h3a2 2 0 012 2v3a2 2 0 01-2 2H5a2 2 0 01-2-2v-7a9 9 0 0118 0v7a2 2 0 01-2 2h-1a2 2 0 01-2-2v-3a2 2 0 012-2h3";
/// `"media.speaker"` — cabinet, woofer ring, tweeter dot.
pub const MEDIA_SPEAKER: &str =
    "M6 2h12a2 2 0 012 2v16a2 2 0 01-2 2H6a2 2 0 01-2-2V4a2 2 0 012-2zM16 14a4 4 0 11-8 0 4 4 0 018 0zM12 6h.01";
/// `"media.radio"` — emitter dot plus two wave pairs.
pub const MEDIA_RADIO: &str =
    "M5.9 19.1C2 15.2 2 8.8 5.9 4.9M7.8 16.2c-2.3-2.3-2.3-6.1 0-8.5M14 12a2 2 0 11-4 0 2 2 0 014 0zM16.2 7.8c2.3 2.3 2.3 6.1 0 8.5M18.1 4.9C22 8.8 22 15.2 18.1 19.1";
/// `"media.music"` — eighth-note figure.
pub const MEDIA_MUSIC: &str =
    "M9 18V5l12-2v13M9 18a3 3 0 11-6 0 3 3 0 016 0zM21 16a3 3 0 11-6 0 3 3 0 016 0z";
/// `"media.audio-lines"` — three equalizer bars.
pub const MEDIA_AUDIO_LINES: &str = "M7 9v6M12 4v16M17 9v6";
/// `"media.video-off"` — video geometry plus strike slash.
pub const MEDIA_VIDEO_OFF: &str =
    "M4 6h10a2 2 0 012 2v8a2 2 0 01-2 2H4a2 2 0 01-2-2V8a2 2 0 012-2zM22 8l-6 4 6 4V8zM2 2l20 20";
/// `"media.camera"` — body, top hump, lens ring.
pub const MEDIA_CAMERA: &str =
    "M14.5 4h-5L7 7H4a2 2 0 00-2 2v9a2 2 0 002 2h16a2 2 0 002-2V9a2 2 0 00-2-2h-3l-2.5-3zM15 13a3 3 0 11-6 0 3 3 0 016 0z";
/// `"media.camera-off"` — camera geometry plus strike slash.
pub const MEDIA_CAMERA_OFF: &str =
    "M14.5 4h-5L7 7H4a2 2 0 00-2 2v9a2 2 0 002 2h16a2 2 0 002-2V9a2 2 0 00-2-2h-3l-2.5-3zM15 13a3 3 0 11-6 0 3 3 0 016 0zM2 2l20 20";
/// `"media.image"` — frame, sun, mountain.
pub const MEDIA_IMAGE: &str =
    "M5 3h14a2 2 0 012 2v14a2 2 0 01-2 2H5a2 2 0 01-2-2V5a2 2 0 012-2zM11 9a2 2 0 11-4 0 2 2 0 014 0zM21 15l-3.09-3.09a2 2 0 00-2.82 0L6 21";
/// `"media.image-off"` — image geometry plus strike slash.
pub const MEDIA_IMAGE_OFF: &str =
    "M5 3h14a2 2 0 012 2v14a2 2 0 01-2 2H5a2 2 0 01-2-2V5a2 2 0 012-2zM11 9a2 2 0 11-4 0 2 2 0 014 0zM21 15l-3.09-3.09a2 2 0 00-2.82 0L6 21M2 2l20 20";
/// `"media.film"` — strip frame with sprocket lanes.
pub const MEDIA_FILM: &str =
    "M5 3h14a2 2 0 012 2v14a2 2 0 01-2 2H5a2 2 0 01-2-2V5a2 2 0 012-2zM7 3v18M3 7.5h4M3 12h18M3 16.5h4M17 3v18M17 7.5h4M17 16.5h4";
/// `"media.clapperboard"` — open slate over a body.
pub const MEDIA_CLAPPERBOARD: &str =
    "M20.2 6L3 11l-.6-2.2c-.3-1 .3-2 1.3-2.4l13.5-4c1.1-.3 2.2.3 2.5 1.3zM6.6 5.5l2.5 3.5M12.4 3.9l2.5 3.5M3 11h18v8a2 2 0 01-2 2H5a2 2 0 01-2-2z";
/// `"media.cast"` — screen with a cut corner plus two cast waves.
pub const MEDIA_CAST: &str =
    "M2 8V6a2 2 0 012-2h16a2 2 0 012 2v12a2 2 0 01-2 2h-6M2 12a9 9 0 018 8M2 16a5 5 0 014 4";
/// `"media.airplay"` — screen plus push triangle.
pub const MEDIA_AIRPLAY: &str =
    "M5 17H4a2 2 0 01-2-2V5a2 2 0 012-2h16a2 2 0 012 2v10a2 2 0 01-2 2h-1M12 15l5 6H7z";
/// `"media.picture-in-picture"` — screen plus anchored mini window.
pub const MEDIA_PICTURE_IN_PICTURE: &str =
    "M21 9V6a2 2 0 00-2-2H4a2 2 0 00-2 2v10a2 2 0 002 2h4M14 13h6a2 2 0 012 2v3a2 2 0 01-2 2h-6a2 2 0 01-2-2v-3a2 2 0 012-2z";
/// `"media.projector"` — lens, beams, body, button.
pub const MEDIA_PROJECTOR: &str =
    "M5 7L3 5M9 6V3M13 7l2-2M12 13a3 3 0 11-6 0 3 3 0 016 0zM11.83 12H20a2 2 0 012 2v4a2 2 0 01-2 2H4a2 2 0 01-2-2v-4a2 2 0 012-2h2.17M16 16h2";

/// `media` entries.
pub const ENTRIES: &[IconEntry] = &[
    IconEntry::new(names::PLAY, MEDIA_PLAY),
    IconEntry::new(names::PAUSE, MEDIA_PAUSE),
    IconEntry::new(names::VIDEO, MEDIA_VIDEO),
    IconEntry::new(names::VOLUME_ON, MEDIA_VOLUME_ON),
    IconEntry::new(names::VOLUME_OFF, MEDIA_VOLUME_OFF),
    IconEntry::new(names::MIC, MEDIA_MIC),
    IconEntry::new(names::MEDIA_STOP, MEDIA_STOP),
    IconEntry::new(names::MEDIA_SKIP_BACK, MEDIA_SKIP_BACK),
    IconEntry::new(names::MEDIA_SKIP_FORWARD, MEDIA_SKIP_FORWARD),
    IconEntry::new(names::MEDIA_REWIND, MEDIA_REWIND),
    IconEntry::new(names::MEDIA_FAST_FORWARD, MEDIA_FAST_FORWARD),
    IconEntry::new(names::MEDIA_REPEAT, MEDIA_REPEAT),
    IconEntry::new(names::MEDIA_REPEAT_1, MEDIA_REPEAT_1),
    IconEntry::new(names::MEDIA_SHUFFLE, MEDIA_SHUFFLE),
    IconEntry::new(names::MEDIA_PLAY_CIRCLE, MEDIA_PLAY_CIRCLE),
    IconEntry::new(names::MEDIA_PAUSE_CIRCLE, MEDIA_PAUSE_CIRCLE),
    IconEntry::new(names::MEDIA_RECORD, MEDIA_RECORD),
    IconEntry::new(names::MEDIA_VOLUME, MEDIA_VOLUME),
    IconEntry::new(names::MEDIA_VOLUME_1, MEDIA_VOLUME_1),
    IconEntry::new(names::MEDIA_VOLUME_2, MEDIA_VOLUME_2),
    IconEntry::new(names::MEDIA_MIC_OFF, MEDIA_MIC_OFF),
    IconEntry::new(names::MEDIA_HEADPHONES, MEDIA_HEADPHONES),
    IconEntry::new(names::MEDIA_SPEAKER, MEDIA_SPEAKER),
    IconEntry::new(names::MEDIA_RADIO, MEDIA_RADIO),
    IconEntry::new(names::MEDIA_MUSIC, MEDIA_MUSIC),
    IconEntry::new(names::MEDIA_AUDIO_LINES, MEDIA_AUDIO_LINES),
    IconEntry::new(names::MEDIA_VIDEO_OFF, MEDIA_VIDEO_OFF),
    IconEntry::new(names::MEDIA_CAMERA, MEDIA_CAMERA),
    IconEntry::new(names::MEDIA_CAMERA_OFF, MEDIA_CAMERA_OFF),
    IconEntry::new(names::MEDIA_IMAGE, MEDIA_IMAGE),
    IconEntry::new(names::MEDIA_IMAGE_OFF, MEDIA_IMAGE_OFF),
    IconEntry::new(names::MEDIA_FILM, MEDIA_FILM),
    IconEntry::new(names::MEDIA_CLAPPERBOARD, MEDIA_CLAPPERBOARD),
    IconEntry::new(names::MEDIA_CAST, MEDIA_CAST),
    IconEntry::new(names::MEDIA_AIRPLAY, MEDIA_AIRPLAY),
    IconEntry::new(names::MEDIA_PICTURE_IN_PICTURE, MEDIA_PICTURE_IN_PICTURE),
    IconEntry::new(names::MEDIA_PROJECTOR, MEDIA_PROJECTOR),
];

/// `media` morph pairs.
pub const PAIRS: &[IconPair] = &[
    IconPair::new(names::PLAY, names::PAUSE),
    IconPair::new(names::VOLUME_ON, names::VOLUME_OFF),
    IconPair::new(names::MIC, names::MEDIA_MIC_OFF),
    IconPair::new(names::VIDEO, names::MEDIA_VIDEO_OFF),
    IconPair::new(names::MEDIA_CAMERA, names::MEDIA_CAMERA_OFF),
    IconPair::new(names::MEDIA_IMAGE, names::MEDIA_IMAGE_OFF),
    IconPair::new(names::MEDIA_PLAY_CIRCLE, names::MEDIA_PAUSE_CIRCLE),
    IconPair::new(names::MEDIA_REPEAT, names::MEDIA_REPEAT_1),
];
