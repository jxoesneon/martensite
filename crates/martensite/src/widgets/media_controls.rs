//! `MediaControls` — a transport strip for [`crate::widgets::MediaView`]
//! (WinUI `MediaTransportControls`, GTK `Video` controls, Ant
//! `Video` player bar).
//!
//! Play/pause toggle, elapsed/total time labels, a draggable seek
//! bar, a volume slider with mute toggle, and a fullscreen button —
//! all *driven*: the widget parks user intent in `take_*` seams
//! ([`MediaControls::take_play_toggled`],
//! [`MediaControls::take_seek`], [`MediaControls::take_volume`],
//! [`MediaControls::take_mute_toggled`],
//! [`MediaControls::take_fullscreen`]) and the app reflects decoder
//! state back through `set_position`/`set_duration`/`set_playing`.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::media_controls::MediaControls;
//!
//! let mut mc = MediaControls::new().duration(120.0).position(30.0);
//! assert_eq!(mc.position_value(), 30.0);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};

const HEIGHT_PT: f32 = 40.0;
const PAD_PT: f32 = 8.0;
const BTN_PT: f32 = 24.0;
const VOL_PT: f32 = 56.0;
const FONT_PT: f32 = 12.0;
const SEEK_H_PT: f32 = 4.0;

const SURFACE: [u8; 4] = [32, 32, 36, 240];
const FG: [u8; 4] = [230, 230, 235, 255];
const MUTED: [u8; 4] = [140, 140, 148, 255];
const TRACK: [u8; 4] = [80, 80, 88, 255];
const ACCENT: [u8; 4] = [80, 140, 220, 255];

#[derive(Clone, Copy, PartialEq)]
enum Drag {
    None,
    Seek,
    Volume,
}

/// A transport-control strip — see the module docs.
///
/// ```
/// use martensite::widgets::media_controls::MediaControls;
///
/// let mc = MediaControls::new();
/// assert!(mc.playing());
/// ```
pub struct MediaControls {
    /// When `false` the strip is inert.
    pub enabled: bool,
    /// Whether the volume zone renders.
    pub show_volume: bool,
    /// Whether the fullscreen button renders.
    pub show_fullscreen: bool,
    playing: bool,
    position: f64,
    duration: f64,
    volume: f32,
    muted: bool,
    play_toggled: bool,
    seek_out: Option<f64>,
    volume_out: Option<f32>,
    mute_toggled: bool,
    fullscreen_req: bool,
    drag: Drag,
    bounds: Rect,
    scale: f32,
    play_rect: Rect,
    seek_rect: Rect,
    vol_rect: Rect,
    mute_rect: Rect,
    fs_rect: Rect,
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl Default for MediaControls {
    fn default() -> Self {
        Self::new()
    }
}

impl MediaControls {
    /// Creates a strip — playing, 0:00/0:00, full volume.
    ///
    /// ```
    /// use martensite::widgets::media_controls::MediaControls;
    ///
    /// let mc = MediaControls::new();
    /// assert_eq!(mc.volume_value(), 1.0);
    /// ```
    pub fn new() -> Self {
        Self {
            enabled: true,
            show_volume: true,
            show_fullscreen: true,
            playing: true,
            position: 0.0,
            duration: 0.0,
            volume: 1.0,
            muted: false,
            play_toggled: false,
            seek_out: None,
            volume_out: None,
            mute_toggled: false,
            fullscreen_req: false,
            drag: Drag::None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            play_rect: Rect::new(0.0, 0.0, 0.0, 0.0),
            seek_rect: Rect::new(0.0, 0.0, 0.0, 0.0),
            vol_rect: Rect::new(0.0, 0.0, 0.0, 0.0),
            mute_rect: Rect::new(0.0, 0.0, 0.0, 0.0),
            fs_rect: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
            text_painter: None,
        }
    }

    /// Initial position in seconds.
    ///
    /// ```
    /// use martensite::widgets::media_controls::MediaControls;
    ///
    /// let mc = MediaControls::new().position(10.0);
    /// assert_eq!(mc.position_value(), 10.0);
    /// ```
    pub fn position(mut self, seconds: f64) -> Self {
        self.position = seconds.max(0.0);
        self
    }

    /// Stream length in seconds.
    ///
    /// ```
    /// use martensite::widgets::media_controls::MediaControls;
    ///
    /// let mc = MediaControls::new().duration(90.0);
    /// assert_eq!(mc.duration_value(), 90.0);
    /// ```
    pub fn duration(mut self, seconds: f64) -> Self {
        self.duration = seconds.max(0.0);
        self
    }

    /// Initial volume `0..=1`.
    ///
    /// ```
    /// use martensite::widgets::media_controls::MediaControls;
    ///
    /// let mc = MediaControls::new().volume(0.5);
    /// assert_eq!(mc.volume_value(), 0.5);
    /// ```
    pub fn volume(mut self, v: f32) -> Self {
        self.volume = v.clamp(0.0, 1.0);
        self
    }

    /// Whether the volume zone renders.
    ///
    /// ```
    /// use martensite::widgets::media_controls::MediaControls;
    ///
    /// let mc = MediaControls::new().show_volume(false);
    /// assert!(!mc.show_volume);
    /// ```
    pub fn show_volume(mut self, show: bool) -> Self {
        self.show_volume = show;
        self
    }

    /// Whether the fullscreen button renders.
    ///
    /// ```
    /// use martensite::widgets::media_controls::MediaControls;
    ///
    /// let mc = MediaControls::new().show_fullscreen(false);
    /// assert!(!mc.show_fullscreen);
    /// ```
    pub fn show_fullscreen(mut self, show: bool) -> Self {
        self.show_fullscreen = show;
        self
    }

    /// Enables or disables the strip.
    ///
    /// ```
    /// use martensite::widgets::media_controls::MediaControls;
    ///
    /// let mc = MediaControls::new().enabled(false);
    /// assert!(!mc.enabled);
    /// ```
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// `true` while the app reports playing.
    ///
    /// ```
    /// use martensite::widgets::media_controls::MediaControls;
    ///
    /// assert!(MediaControls::new().playing());
    /// ```
    pub fn playing(&self) -> bool {
        self.playing
    }

    /// Reflects decoder play state back onto the face.
    ///
    /// ```
    /// use martensite::widgets::media_controls::MediaControls;
    ///
    /// let mut mc = MediaControls::new();
    /// mc.set_playing(false);
    /// assert!(!mc.playing());
    /// ```
    pub fn set_playing(&mut self, playing: bool) {
        self.playing = playing;
    }

    /// Current position in seconds.
    ///
    /// ```
    /// use martensite::widgets::media_controls::MediaControls;
    ///
    /// assert_eq!(MediaControls::new().position(5.0).position_value(), 5.0);
    /// ```
    pub fn position_value(&self) -> f64 {
        self.position
    }

    /// Reflects playback progress back onto the bar.
    ///
    /// ```
    /// use martensite::widgets::media_controls::MediaControls;
    ///
    /// let mut mc = MediaControls::new();
    /// mc.set_position(7.0);
    /// assert_eq!(mc.position_value(), 7.0);
    /// ```
    pub fn set_position(&mut self, seconds: f64) {
        self.position = seconds.max(0.0);
    }

    /// Stream length in seconds.
    ///
    /// ```
    /// use martensite::widgets::media_controls::MediaControls;
    ///
    /// assert_eq!(MediaControls::new().duration(3.0).duration_value(), 3.0);
    /// ```
    pub fn duration_value(&self) -> f64 {
        self.duration
    }

    /// Reflects a newly-learned duration.
    ///
    /// ```
    /// use martensite::widgets::media_controls::MediaControls;
    ///
    /// let mut mc = MediaControls::new();
    /// mc.set_duration(60.0);
    /// assert_eq!(mc.duration_value(), 60.0);
    /// ```
    pub fn set_duration(&mut self, seconds: f64) {
        self.duration = seconds.max(0.0);
    }

    /// Volume `0..=1` (post-mute visual is `0`).
    ///
    /// ```
    /// use martensite::widgets::media_controls::MediaControls;
    ///
    /// assert_eq!(MediaControls::new().volume(0.3).volume_value(), 0.3);
    /// ```
    pub fn volume_value(&self) -> f32 {
        self.volume
    }

    /// `true` while muted.
    ///
    /// ```
    /// use martensite::widgets::media_controls::MediaControls;
    ///
    /// assert!(!MediaControls::new().muted());
    /// ```
    pub fn muted(&self) -> bool {
        self.muted
    }

    /// Reflects mute state back onto the face.
    ///
    /// ```
    /// use martensite::widgets::media_controls::MediaControls;
    ///
    /// let mut mc = MediaControls::new();
    /// mc.set_muted(true);
    /// assert!(mc.muted());
    /// ```
    pub fn set_muted(&mut self, muted: bool) {
        self.muted = muted;
    }

    /// Explicit painter override — see [`crate::text_paint`].
    ///
    /// ```
    /// use martensite::widgets::media_controls::MediaControls;
    /// use martensite::text_paint::shared_painter;
    ///
    /// let mc = MediaControls::new().with_text_painter(shared_painter());
    /// assert!(mc.playing());
    /// ```
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Drains a pending play/pause toggle.
    ///
    /// ```
    /// use martensite::widgets::media_controls::MediaControls;
    ///
    /// let mut mc = MediaControls::new();
    /// assert!(!mc.take_play_toggled());
    /// ```
    pub fn take_play_toggled(&mut self) -> bool {
        std::mem::take(&mut self.play_toggled)
    }

    /// Drains a pending seek target in seconds.
    ///
    /// ```
    /// use martensite::widgets::media_controls::MediaControls;
    ///
    /// let mut mc = MediaControls::new();
    /// assert_eq!(mc.take_seek(), None);
    /// ```
    pub fn take_seek(&mut self) -> Option<f64> {
        self.seek_out.take()
    }

    /// Drains a pending volume `0..=1`.
    ///
    /// ```
    /// use martensite::widgets::media_controls::MediaControls;
    ///
    /// let mut mc = MediaControls::new();
    /// assert_eq!(mc.take_volume(), None);
    /// ```
    pub fn take_volume(&mut self) -> Option<f32> {
        self.volume_out.take()
    }

    /// Drains a pending mute toggle.
    ///
    /// ```
    /// use martensite::widgets::media_controls::MediaControls;
    ///
    /// let mut mc = MediaControls::new();
    /// assert!(!mc.take_mute_toggled());
    /// ```
    pub fn take_mute_toggled(&mut self) -> bool {
        std::mem::take(&mut self.mute_toggled)
    }

    /// Drains a pending fullscreen request.
    ///
    /// ```
    /// use martensite::widgets::media_controls::MediaControls;
    ///
    /// let mut mc = MediaControls::new();
    /// assert!(!mc.take_fullscreen());
    /// ```
    pub fn take_fullscreen(&mut self) -> bool {
        std::mem::take(&mut self.fullscreen_req)
    }

    /// `M:SS` display for a seconds value.
    fn fmt_time(seconds: f64) -> String {
        let s = seconds.max(0.0) as u64;
        format!("{}:{:02}", s / 60, s % 60)
    }

    /// Seek fraction `0..=1` at an x position.
    fn seek_frac(&self, x: f32) -> f32 {
        ((x - self.seek_rect.min_x()) / self.seek_rect.width().max(1.0)).clamp(0.0, 1.0)
    }

    /// Volume `0..=1` at an x position.
    fn vol_at(&self, x: f32) -> f32 {
        ((x - self.vol_rect.min_x()) / self.vol_rect.width().max(1.0)).clamp(0.0, 1.0)
    }
}

impl Widget for MediaControls {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            cx.pt(320.0).min(constraints.max_size.x.max(0.0)),
            cx.pt(HEIGHT_PT).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(200.0, 32.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        let pad = cx.pt(PAD_PT);
        let btn = cx.pt(BTN_PT);
        let cy = bounds.min_y() + (bounds.height() - btn) / 2.0;
        let mut x = bounds.min_x() + pad;
        self.play_rect = Rect::new(x, cy, btn, btn);
        x += btn + pad;
        // Time labels reserve fixed ~4-char widths.
        let time_w = cx.pt(34.0);
        x += time_w + pad * 0.5;
        // Trailing chrome yields when the strip can't fit it — the
        // volume cluster first, then fullscreen — instead of sliding
        // left over the elapsed-time label. The seek bar keeps a
        // workable minimum.
        let min_seek = cx.pt(24.0);
        let vol_w = cx.pt(VOL_PT);
        // Space left for optional chrome after play + both time
        // labels + a usable seek.
        let mut spare = (bounds.max_x() - pad) - x - time_w - pad * 0.5 - min_seek;
        let show_vol = self.show_volume && spare >= vol_w + btn + pad * 1.5;
        if show_vol {
            spare -= vol_w + btn + pad * 1.5;
        }
        let show_fs = self.show_fullscreen && spare >= btn + pad;
        let mut right = bounds.max_x() - pad;
        if show_fs {
            self.fs_rect = Rect::new(right - btn, cy, btn, btn);
            right -= btn + pad;
        } else {
            self.fs_rect = Rect::new(0.0, 0.0, 0.0, 0.0);
        }
        if show_vol {
            self.vol_rect = Rect::new(right - vol_w, cy + btn * 0.25, vol_w, btn * 0.5);
            right -= vol_w + pad * 0.5;
            self.mute_rect = Rect::new(right - btn, cy, btn, btn);
            right -= btn + pad;
        } else {
            self.vol_rect = Rect::new(0.0, 0.0, 0.0, 0.0);
            self.mute_rect = Rect::new(0.0, 0.0, 0.0, 0.0);
        }
        right -= time_w + pad * 0.5;
        let seek_h = cx.pt(SEEK_H_PT);
        self.seek_rect = Rect::new(
            x,
            bounds.min_y() + (bounds.height() - seek_h) / 2.0,
            (right - x).max(0.0),
            seek_h,
        );
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Group);
        node.set_label("Media controls");
        node.set_value(format!(
            "{} of {}, {}",
            Self::fmt_time(self.position),
            Self::fmt_time(self.duration),
            if self.playing { "playing" } else { "paused" }
        ));
        if !self.enabled {
            node.set_disabled();
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.enabled {
            return EventResponse::Ignored;
        }
        match cx.event {
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                ..
            } => {
                let p = *position;
                if self.play_rect.contains(p) {
                    self.play_toggled = true;
                    return EventResponse::Handled;
                }
                if self.show_volume && self.mute_rect.contains(p) {
                    self.mute_toggled = true;
                    return EventResponse::Handled;
                }
                if self.show_fullscreen && self.fs_rect.contains(p) {
                    self.fullscreen_req = true;
                    return EventResponse::Handled;
                }
                if self.seek_rect.contains(p) {
                    self.drag = Drag::Seek;
                    self.seek_out = Some(self.seek_frac(p.x) as f64 * self.duration);
                    return EventResponse::CapturePointer;
                }
                if self.show_volume && self.vol_rect.contains(p) {
                    self.drag = Drag::Volume;
                    let v = self.vol_at(p.x);
                    self.volume = v;
                    self.volume_out = Some(v);
                    return EventResponse::CapturePointer;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerMoved { position } => match self.drag {
                Drag::Seek => {
                    self.seek_out = Some(self.seek_frac(position.x) as f64 * self.duration);
                    EventResponse::RequestRepaint
                }
                Drag::Volume => {
                    let v = self.vol_at(position.x);
                    self.volume = v;
                    self.volume_out = Some(v);
                    EventResponse::RequestRepaint
                }
                Drag::None => EventResponse::Ignored,
            },
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                ..
            } => {
                if self.drag != Drag::None {
                    self.drag = Drag::None;
                    return EventResponse::Handled;
                }
                EventResponse::Ignored
            }
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let f = |r: Rect| {
            kurbo::Rect::new(
                f64::from(r.min_x()),
                f64::from(r.min_y()),
                f64::from(r.max_x()),
                f64::from(r.max_y()),
            )
        };
        let surface = cx.color(martensite_theme::TokenKey::SurfaceColor, SURFACE);
        let fg = if self.enabled {
            cx.color(martensite_theme::TokenKey::TextColor, FG)
        } else {
            cx.color(martensite_theme::TokenKey::TextMutedColor, MUTED)
        };
        let muted = cx.color(martensite_theme::TokenKey::TextMutedColor, MUTED);
        let accent = cx.color(martensite_theme::TokenKey::AccentColor, ACCENT);
        let track = cx.color(martensite_theme::TokenKey::DividerColor, TRACK);
        cx.list.push_fill_shape(
            f(self.bounds),
            &martensite_core::shape::Shape::rounded(cx.pt(6.0)),
            surface,
        );

        // Play/pause glyph.
        let (l, t, r, b) = (
            f64::from(self.play_rect.min_x()),
            f64::from(self.play_rect.min_y()),
            f64::from(self.play_rect.max_x()),
            f64::from(self.play_rect.max_y()),
        );
        let mut g = kurbo::BezPath::new();
        if self.playing {
            // Pause: two bars.
            let bw = (r - l) * 0.22;
            let x1 = l + (r - l) * 0.26;
            let x2 = r - (r - l) * 0.26;
            g.move_to((x1, t + 3.0));
            g.line_to((x1 + bw, t + 3.0));
            g.line_to((x1 + bw, b - 3.0));
            g.line_to((x1, b - 3.0));
            g.close_path();
            g.move_to((x2 - bw, t + 3.0));
            g.line_to((x2, t + 3.0));
            g.line_to((x2, b - 3.0));
            g.line_to((x2 - bw, b - 3.0));
            g.close_path();
        } else {
            // Play: right triangle.
            g.move_to((l + (r - l) * 0.3, t + 3.0));
            g.line_to((r - (r - l) * 0.2, (t + b) / 2.0));
            g.line_to((l + (r - l) * 0.3, b - 3.0));
            g.close_path();
        }
        cx.list.push_path(g, fg);

        // Seek bar: track + played fill + thumb.
        cx.list.push_fill_shape(
            f(self.seek_rect),
            &martensite_core::shape::Shape::rounded(self.seek_rect.height() / 2.0),
            track,
        );
        let frac = if self.duration > 0.0 {
            (self.position / self.duration).clamp(0.0, 1.0) as f32
        } else {
            0.0
        };
        let played = Rect::new(
            self.seek_rect.min_x(),
            self.seek_rect.min_y(),
            self.seek_rect.width() * frac,
            self.seek_rect.height(),
        );
        cx.list.push_fill_shape(
            f(played),
            &martensite_core::shape::Shape::rounded(self.seek_rect.height() / 2.0),
            accent,
        );
        let thumb_r = cx.pt(5.0);
        let thumb_x = self.seek_rect.min_x() + self.seek_rect.width() * frac;
        let thumb_y = self.seek_rect.min_y() + self.seek_rect.height() / 2.0;
        cx.list.push_fill_shape(
            kurbo::Rect::new(
                f64::from(thumb_x - thumb_r),
                f64::from(thumb_y - thumb_r),
                f64::from(thumb_x + thumb_r),
                f64::from(thumb_y + thumb_r),
            ),
            &martensite_core::shape::Shape::circle(Vec2::new(thumb_x, thumb_y), thumb_r),
            fg,
        );

        // Time labels (placeholder boxes are fine — real paint uses the
        // ambient shaper through paint_label).
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let size = FONT_PT * cx.scale;
        let elapsed = Self::fmt_time(self.position);
        let total = Self::fmt_time(self.duration);
        let time_w = cx.pt(34.0);
        let y = self.bounds.min_y() + (self.bounds.height() - size) / 2.0;
        crate::text_paint::paint_label_clipped(
            painter,
            cx.list,
            f(Rect::new(
                self.play_rect.max_x() + cx.pt(4.0),
                self.bounds.min_y(),
                time_w,
                self.bounds.height(),
            )),
            kurbo::Point::new(f64::from(self.play_rect.max_x() + cx.pt(4.0)), f64::from(y)),
            &elapsed,
            size,
            muted,
        );
        let end_x = self.seek_rect.max_x() + cx.pt(4.0);
        crate::text_paint::paint_label_clipped(
            painter,
            cx.list,
            f(Rect::new(
                end_x,
                self.bounds.min_y(),
                time_w,
                self.bounds.height(),
            )),
            kurbo::Point::new(f64::from(end_x), f64::from(y)),
            &total,
            size,
            muted,
        );

        // Volume: speaker glyph + slider.
        if self.show_volume {
            let (l, t, r, b) = (
                f64::from(self.mute_rect.min_x()),
                f64::from(self.mute_rect.min_y()),
                f64::from(self.mute_rect.max_x()),
                f64::from(self.mute_rect.max_y()),
            );
            let mut sp = kurbo::BezPath::new();
            sp.move_to((l + 3.0, (t + b) / 2.0 - 3.0));
            sp.line_to((l + 8.0, (t + b) / 2.0 - 3.0));
            sp.line_to((r - 4.0, t + 4.0));
            sp.line_to((r - 4.0, b - 4.0));
            sp.line_to((l + 8.0, (t + b) / 2.0 + 3.0));
            sp.line_to((l + 3.0, (t + b) / 2.0 + 3.0));
            sp.close_path();
            cx.list.push_path(sp, if self.muted { muted } else { fg });
            // Slider.
            cx.list.push_fill_shape(
                f(self.vol_rect),
                &martensite_core::shape::Shape::rounded(self.vol_rect.height() / 2.0),
                track,
            );
            let vf = if self.muted { 0.0 } else { self.volume };
            let vfill = Rect::new(
                self.vol_rect.min_x(),
                self.vol_rect.min_y(),
                self.vol_rect.width() * vf,
                self.vol_rect.height(),
            );
            cx.list.push_fill_shape(
                f(vfill),
                &martensite_core::shape::Shape::rounded(self.vol_rect.height() / 2.0),
                if self.muted { muted } else { accent },
            );
        }

        // Fullscreen: four corner ticks.
        if self.show_fullscreen {
            let (l, t, r, b) = (
                f64::from(self.fs_rect.min_x() + 4.0),
                f64::from(self.fs_rect.min_y() + 4.0),
                f64::from(self.fs_rect.max_x() - 4.0),
                f64::from(self.fs_rect.max_y() - 4.0),
            );
            let k = (r - l) * 0.3;
            let mut fs = kurbo::BezPath::new();
            for (cx0, cy0, dx, dy) in [
                (l, t, 1.0, 1.0),
                (r, t, -1.0, 1.0),
                (r, b, -1.0, -1.0),
                (l, b, 1.0, -1.0),
            ] {
                fs.move_to((cx0 + dx * k, cy0));
                fs.line_to((cx0, cy0));
                fs.line_to((cx0, cy0 + dy * k));
            }
            cx.list.push_stroke_path(fs, cx.pt(1.5), fg);
        }
    }
}

impl std::fmt::Debug for MediaControls {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MediaControls")
            .field("playing", &self.playing)
            .field("position", &self.position)
            .field("duration", &self.duration)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(mc: &mut MediaControls, w: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        mc.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(w, 40.0),
            },
        );
        mc.layout(&mut cx, Rect::new(0.0, 0.0, w, 40.0));
    }

    fn press(mc: &mut MediaControls, p: Vec2) -> EventResponse {
        mc.event(&mut EventContext {
            event: &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: p,
                count: 1,
            },
            bounds: Rect::new(0.0, 0.0, 400.0, 40.0),
            scale: 1.0,
        })
    }

    #[test]
    fn play_button_parks_toggle() {
        let mut mc = MediaControls::new();
        laid_out(&mut mc, 400.0);
        let c = mc.play_rect;
        press(&mut mc, Vec2::new(c.min_x() + 4.0, c.min_y() + 4.0));
        assert!(mc.take_play_toggled());
        assert!(!mc.take_play_toggled());
    }

    #[test]
    fn seek_press_parks_seconds() {
        let mut mc = MediaControls::new().duration(100.0);
        laid_out(&mut mc, 400.0);
        let s = mc.seek_rect;
        press(&mut mc, Vec2::new(s.min_x() + s.width() / 2.0, s.min_y()));
        let seek = mc.take_seek().unwrap();
        assert!((seek - 50.0).abs() < 5.0);
    }

    #[test]
    fn seek_drag_updates() {
        let mut mc = MediaControls::new().duration(100.0);
        laid_out(&mut mc, 400.0);
        let s = mc.seek_rect;
        press(&mut mc, Vec2::new(s.min_x(), s.min_y()));
        mc.event(&mut EventContext {
            event: &WidgetEvent::PointerMoved {
                position: Vec2::new(s.max_x(), s.min_y()),
            },
            bounds: Rect::new(0.0, 0.0, 400.0, 40.0),
            scale: 1.0,
        });
        assert!((mc.take_seek().unwrap() - 100.0).abs() < 1.0);
    }

    #[test]
    fn volume_drag_parks() {
        let mut mc = MediaControls::new();
        laid_out(&mut mc, 400.0);
        let v = mc.vol_rect;
        press(&mut mc, Vec2::new(v.min_x() + v.width() / 2.0, v.min_y()));
        let vol = mc.take_volume().unwrap();
        assert!((vol - 0.5).abs() < 0.1);
        assert!((mc.volume_value() - 0.5).abs() < 0.1);
    }

    #[test]
    fn mute_and_fullscreen_park() {
        let mut mc = MediaControls::new();
        laid_out(&mut mc, 400.0);
        let m = mc.mute_rect;
        press(&mut mc, Vec2::new(m.min_x() + 4.0, m.min_y() + 4.0));
        assert!(mc.take_mute_toggled());
        let fsr = mc.fs_rect;
        press(&mut mc, Vec2::new(fsr.min_x() + 4.0, fsr.min_y() + 4.0));
        assert!(mc.take_fullscreen());
    }

    #[test]
    fn hidden_zones_have_empty_rects() {
        let mut mc = MediaControls::new()
            .show_volume(false)
            .show_fullscreen(false);
        laid_out(&mut mc, 400.0);
        assert_eq!(mc.vol_rect.width(), 0.0);
        assert_eq!(mc.fs_rect.width(), 0.0);
    }

    #[test]
    fn fmt_time_minutes() {
        assert_eq!(MediaControls::fmt_time(0.0), "0:00");
        assert_eq!(MediaControls::fmt_time(65.0), "1:05");
        assert_eq!(MediaControls::fmt_time(600.0), "10:00");
    }

    #[test]
    fn disabled_inert() {
        let mut mc = MediaControls::new().enabled(false);
        laid_out(&mut mc, 400.0);
        let c = mc.play_rect;
        press(&mut mc, Vec2::new(c.min_x() + 4.0, c.min_y() + 4.0));
        assert!(!mc.take_play_toggled());
    }
}
