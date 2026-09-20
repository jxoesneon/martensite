//! `NowPlaying` — a media-session card: art swatch, title, artist,
//! and an elapsed/total mini progress line (Spotify / Apple Music
//! "now playing" bar idiom).
//!
//! The metadata companion to
//! [`MediaControls`](crate::widgets::MediaControls): transport
//! lives there; this shows *what* is playing. The host drives
//! `set_position`/`set_duration` from decoder state; a card click
//! parks [`NowPlaying::take_clicked`] (expand-to-full intent).
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::now_playing::NowPlaying;
//!
//! let n = NowPlaying::new("Blue in Green", "Miles Davis")
//!     .album("Kind of Blue")
//!     .duration(327.0)
//!     .position(60.0);
//! assert_eq!(n.progress(), 60.0 / 327.0);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::text_paint::SharedTextPainter;

const ART_PT: f32 = 44.0;
const PAD_PT: f32 = 8.0;
const GAP_PT: f32 = 10.0;
const TITLE_PT: f32 = 13.0;
const SUB_PT: f32 = 10.0;
const RAIL_H_PT: f32 = 3.0;
const RADIUS_PT: f32 = 6.0;

const FACE: [u8; 4] = [36, 38, 44, 255];
const ART: [u8; 4] = [58, 60, 68, 255];
const ACCENT: [u8; 4] = [88, 130, 247, 255];
const RAIL: [u8; 4] = [70, 72, 80, 255];
const TEXT: [u8; 4] = [220, 222, 228, 255];
const MUTED: [u8; 4] = [139, 148, 158, 255];

/// A now-playing card — see the module docs.
///
/// ```
/// use martensite::widgets::now_playing::NowPlaying;
///
/// assert_eq!(NowPlaying::new("t", "a").title(), "t");
/// ```
pub struct NowPlaying {
    /// Accessibility label.
    pub label: Option<String>,
    title: String,
    artist: String,
    album: Option<String>,
    /// Swatch color for the art block.
    pub art_color: [u8; 4],
    /// Art initials (derived from title when unset).
    pub art_text: Option<String>,
    position: f32,
    duration: f32,
    playing: bool,
    clicked: bool,
    text_painter: Option<SharedTextPainter>,
    bounds: Rect,
    scale: f32,
}

impl std::fmt::Debug for NowPlaying {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NowPlaying")
            .field("title", &self.title)
            .field("artist", &self.artist)
            .field("position", &self.position)
            .finish()
    }
}

impl NowPlaying {
    /// Card for `title` by `artist`.
    ///
    /// ```
    /// use martensite::widgets::now_playing::NowPlaying;
    ///
    /// assert_eq!(NowPlaying::new("Song", "Band").artist(), "Band");
    /// ```
    pub fn new(title: impl Into<String>, artist: impl Into<String>) -> Self {
        Self {
            label: None,
            title: title.into(),
            artist: artist.into(),
            album: None,
            art_color: ART,
            art_text: None,
            position: 0.0,
            duration: 0.0,
            playing: false,
            clicked: false,
            text_painter: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Album builder (shown as `artist — album`).
    ///
    /// ```
    /// use martensite::widgets::now_playing::NowPlaying;
    ///
    /// assert_eq!(NowPlaying::new("t", "a").album("X").album_name(), Some("X"));
    /// ```
    pub fn album(mut self, album: impl Into<String>) -> Self {
        self.album = Some(album.into());
        self
    }

    /// Art swatch color.
    ///
    /// ```
    /// use martensite::widgets::now_playing::NowPlaying;
    ///
    /// assert_eq!(NowPlaying::new("t", "a").art_color([255, 0, 0, 255]).art_color[0], 255);
    /// ```
    pub fn art_color(mut self, color: [u8; 4]) -> Self {
        self.art_color = color;
        self
    }

    /// Art text override (glyph or short initials).
    ///
    /// ```
    /// use martensite::widgets::now_playing::NowPlaying;
    ///
    /// assert_eq!(NowPlaying::new("t", "a").art_text("♪").art_text.as_deref(), Some("♪"));
    /// ```
    pub fn art_text(mut self, text: impl Into<String>) -> Self {
        self.art_text = Some(text.into());
        self
    }

    /// Position builder (seconds).
    ///
    /// ```
    /// use martensite::widgets::now_playing::NowPlaying;
    ///
    /// assert_eq!(NowPlaying::new("t", "a").position(30.0).position_value(), 30.0);
    /// ```
    pub fn position(mut self, position: f32) -> Self {
        self.position = position.max(0.0);
        self
    }

    /// Duration builder (seconds).
    ///
    /// ```
    /// use martensite::widgets::now_playing::NowPlaying;
    ///
    /// assert_eq!(NowPlaying::new("t", "a").duration(60.0).duration_value(), 60.0);
    /// ```
    pub fn duration(mut self, duration: f32) -> Self {
        self.duration = duration.max(0.0);
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::now_playing::NowPlaying;
    ///
    /// assert_eq!(NowPlaying::new("t", "a").label("Player").label.as_deref(), Some("Player"));
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Shared text painter for real glyph metrics.
    ///
    /// ```no_run
    /// use martensite::widgets::now_playing::NowPlaying;
    /// # fn example(p: martensite::text_paint::SharedTextPainter) {
    /// let _n = NowPlaying::new("t", "a").with_text_painter(p);
    /// # }
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Track title.
    ///
    /// ```
    /// use martensite::widgets::now_playing::NowPlaying;
    ///
    /// assert_eq!(NowPlaying::new("Song", "a").title(), "Song");
    /// ```
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Artist name.
    ///
    /// ```
    /// use martensite::widgets::now_playing::NowPlaying;
    ///
    /// assert_eq!(NowPlaying::new("t", "Band").artist(), "Band");
    /// ```
    pub fn artist(&self) -> &str {
        &self.artist
    }

    /// Album name when set.
    ///
    /// ```
    /// use martensite::widgets::now_playing::NowPlaying;
    ///
    /// assert_eq!(NowPlaying::new("t", "a").album_name(), None);
    /// ```
    pub fn album_name(&self) -> Option<&str> {
        self.album.as_deref()
    }

    /// Elapsed seconds.
    ///
    /// ```
    /// use martensite::widgets::now_playing::NowPlaying;
    ///
    /// assert_eq!(NowPlaying::new("t", "a").position_value(), 0.0);
    /// ```
    pub fn position_value(&self) -> f32 {
        self.position
    }

    /// Track length in seconds.
    ///
    /// ```
    /// use martensite::widgets::now_playing::NowPlaying;
    ///
    /// assert_eq!(NowPlaying::new("t", "a").duration_value(), 0.0);
    /// ```
    pub fn duration_value(&self) -> f32 {
        self.duration
    }

    /// Played fraction `0.0–1.0`.
    ///
    /// ```
    /// use martensite::widgets::now_playing::NowPlaying;
    ///
    /// assert_eq!(NowPlaying::new("t", "a").duration(100.0).position(25.0).progress(), 0.25);
    /// ```
    pub fn progress(&self) -> f32 {
        if self.duration <= 0.0 {
            0.0
        } else {
            (self.position / self.duration).clamp(0.0, 1.0)
        }
    }

    /// Whether the session reports playing.
    ///
    /// ```
    /// use martensite::widgets::now_playing::NowPlaying;
    ///
    /// assert!(!NowPlaying::new("t", "a").is_playing());
    /// ```
    pub fn is_playing(&self) -> bool {
        self.playing
    }

    /// Host seam: reflect decoder position.
    ///
    /// ```
    /// use martensite::widgets::now_playing::NowPlaying;
    ///
    /// let mut n = NowPlaying::new("t", "a");
    /// n.set_position(42.0);
    /// assert_eq!(n.position_value(), 42.0);
    /// ```
    pub fn set_position(&mut self, position: f32) {
        self.position = position.max(0.0);
    }

    /// Host seam: reflect track length.
    ///
    /// ```
    /// use martensite::widgets::now_playing::NowPlaying;
    ///
    /// let mut n = NowPlaying::new("t", "a");
    /// n.set_duration(200.0);
    /// assert_eq!(n.duration_value(), 200.0);
    /// ```
    pub fn set_duration(&mut self, duration: f32) {
        self.duration = duration.max(0.0);
    }

    /// Host seam: reflect transport state (drives the ▶ marker).
    ///
    /// ```
    /// use martensite::widgets::now_playing::NowPlaying;
    ///
    /// let mut n = NowPlaying::new("t", "a");
    /// n.set_playing(true);
    /// assert!(n.is_playing());
    /// ```
    pub fn set_playing(&mut self, playing: bool) {
        self.playing = playing;
    }

    /// Drains a card click (expand intent).
    ///
    /// ```
    /// use martensite::widgets::now_playing::NowPlaying;
    ///
    /// let mut n = NowPlaying::new("t", "a");
    /// assert!(!n.take_clicked());
    /// ```
    pub fn take_clicked(&mut self) -> bool {
        std::mem::take(&mut self.clicked)
    }

    /// `mm:ss` format.
    fn fmt_time(secs: f32) -> String {
        let s = secs.max(0.0) as u64;
        format!("{}:{:02}", s / 60, s % 60)
    }
}

impl Widget for NowPlaying {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let h = (ART_PT + PAD_PT * 2.0 + RAIL_H_PT + 4.0) * cx.scale;
        Vec2::new(constraints.max_size.x.max(200.0 * cx.scale), h)
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(160.0, 44.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Group);
        let label = self
            .label
            .clone()
            .unwrap_or_else(|| format!("Now playing {} by {}", self.title, self.artist));
        node.set_label(label);
        node.set_value(format!(
            "{} of {}",
            Self::fmt_time(self.position),
            Self::fmt_time(self.duration)
        ));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if let WidgetEvent::PointerReleased {
            button: PointerButton::Primary,
            position,
        } = cx.event
        {
            if self.bounds.contains(*position) {
                self.clicked = true;
                return EventResponse::Handled;
            }
        }
        EventResponse::Ignored
    }

    fn paint(&self, cx: &mut PaintContext) {
        let s = cx.scale;
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let krect = |r: Rect| {
            kurbo::Rect::new(
                f64::from(r.min_x()),
                f64::from(r.min_y()),
                f64::from(r.max_x()),
                f64::from(r.max_y()),
            )
        };
        let card = Rect::new(
            self.bounds.min_x(),
            self.bounds.min_y(),
            self.bounds.width(),
            self.bounds.height() - (RAIL_H_PT + 4.0) * s,
        );
        let shape = martensite_core::shape::Shape::rounded(RADIUS_PT * s);
        cx.list
            .push_fill_shape(krect(card), &shape, cx.color(TokenKey::SurfaceColor, FACE));
        // Art swatch.
        let d = ART_PT * s;
        let art = Rect::new(
            card.min_x() + PAD_PT * s,
            card.min_y() + (card.height() - d) / 2.0,
            d,
            d,
        );
        cx.list.push_fill_shape(krect(art), &shape, self.art_color);
        let glyph = self.art_text.clone().unwrap_or_else(|| {
            self.title
                .chars()
                .next()
                .map(|c| c.to_ascii_uppercase().to_string())
                .unwrap_or_else(|| "♪".to_string())
        });
        let gsize = TITLE_PT * 1.4 * s;
        let gw = painter
            .and_then(|p| p.measure_text(&glyph, gsize))
            .unwrap_or(glyph.chars().count() as f32 * gsize * 0.6);
        let go = kurbo::Point::new(
            f64::from(art.min_x() + (d - gw) / 2.0),
            f64::from(art.min_y() + d / 2.0),
        );
        crate::text_paint::paint_label(
            painter,
            cx.list,
            go,
            &glyph,
            gsize,
            cx.color(TokenKey::TextInverseColor, TEXT),
        );
        // Title + artist — album.
        let tx = art.max_x() + GAP_PT * s;
        let cy = card.min_y() + card.height() / 2.0;
        let title = if self.playing {
            format!("▶ {}", self.title)
        } else {
            self.title.clone()
        };
        crate::text_paint::paint_label(
            painter,
            cx.list,
            kurbo::Point::new(f64::from(tx), f64::from(cy - 4.0 * s)),
            &title,
            TITLE_PT * s,
            cx.color(TokenKey::TextColor, TEXT),
        );
        let sub = match &self.album {
            Some(a) => format!("{} — {}", self.artist, a),
            None => self.artist.clone(),
        };
        crate::text_paint::paint_label(
            painter,
            cx.list,
            kurbo::Point::new(f64::from(tx), f64::from(cy + SUB_PT * s)),
            &sub,
            SUB_PT * s,
            cx.color(TokenKey::TextMutedColor, MUTED),
        );
        // Times right-aligned.
        let times = format!(
            "{} / {}",
            Self::fmt_time(self.position),
            Self::fmt_time(self.duration)
        );
        let tw = painter
            .and_then(|p| p.measure_text(&times, SUB_PT * s))
            .unwrap_or(times.len() as f32 * SUB_PT * 0.6 * s);
        crate::text_paint::paint_label(
            painter,
            cx.list,
            kurbo::Point::new(
                f64::from(card.max_x() - PAD_PT * s - tw),
                f64::from(cy + SUB_PT * s),
            ),
            &times,
            SUB_PT * s,
            cx.color(TokenKey::TextMutedColor, MUTED),
        );
        // Progress rail along the card's bottom.
        let rail = kurbo::Rect::new(
            f64::from(self.bounds.min_x()),
            f64::from(self.bounds.max_y() - RAIL_H_PT * s),
            f64::from(self.bounds.max_x()),
            f64::from(self.bounds.max_y()),
        );
        cx.list
            .push_fill_rect(rail, cx.color(TokenKey::DividerColor, RAIL));
        let fill = kurbo::Rect::new(
            rail.x0,
            rail.y0,
            rail.x0 + (rail.x1 - rail.x0) * f64::from(self.progress()),
            rail.y1,
        );
        cx.list
            .push_fill_rect(fill, cx.color(TokenKey::AccentColor, ACCENT));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(n: &mut NowPlaying) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        n.layout(&mut cx, Rect::new(0.0, 0.0, 320.0, 64.0));
    }

    #[test]
    fn progress_clamps() {
        let mut n = NowPlaying::new("t", "a").duration(100.0).position(150.0);
        assert_eq!(n.progress(), 1.0);
        n.set_duration(0.0);
        assert_eq!(n.progress(), 0.0);
    }

    #[test]
    fn click_parks() {
        let mut n = NowPlaying::new("t", "a");
        laid_out(&mut n);
        n.event(&mut EventContext {
            event: &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: Vec2::new(50.0, 30.0),
            },
            bounds: n.bounds,
            scale: 1.0,
        });
        assert!(n.take_clicked());
        assert!(!n.take_clicked());
    }

    #[test]
    fn fmt_time_minutes() {
        assert_eq!(NowPlaying::fmt_time(327.0), "5:27");
        assert_eq!(NowPlaying::fmt_time(65.0), "1:05");
    }

    #[test]
    fn paint_without_painter() {
        let mut n = NowPlaying::new("Blue in Green", "Miles Davis")
            .duration(327.0)
            .position(60.0);
        laid_out(&mut n);
        let mut list = martensite_core::PaintList::default();
        let theme = martensite_theme::Theme::new("test");
        n.paint(&mut PaintContext {
            list: &mut list,
            bounds: n.bounds,
            theme: &theme,
            scale: 1.0,
            text_painter: None,
        });
        assert!(!list.is_empty());
    }
}
