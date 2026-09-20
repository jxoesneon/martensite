//! `Captions` — a timed subtitle band rendered over media
//! (closed-caption / `.srt` idiom): the host loads [`CaptionCue`]s
//! and calls [`Captions::set_position`] with the media clock; the
//! widget paints the active cue bottom-centered on a translucent
//! band.
//!
//! AccessKit exposes the widget as `Role::Status` (a live
//! region), so screen readers announce cue changes. Companion to
//! [`MediaControls`](crate::widgets::MediaControls).
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::captions::Captions;
//! use std::time::Duration;
//!
//! let mut c = Captions::new().cue("Hello", 0.0, 2.0).cue("World", 2.5, 4.0);
//! c.set_position(Duration::from_secs_f32(1.0));
//! assert_eq!(c.active_text(), Some("Hello"));
//! ```

use std::time::Duration;

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget,
};
use martensite_theme::TokenKey;

use crate::text_paint::SharedTextPainter;

const FONT_PT: f32 = 16.0;
const PAD_X_PT: f32 = 14.0;
const PAD_Y_PT: f32 = 6.0;
const BAND_BOTTOM_PT: f32 = 24.0;

const BAND: [u8; 4] = [0, 0, 0, 170];
const TEXT: [u8; 4] = [255, 255, 255, 255];

/// One timed cue.
///
/// ```
/// use martensite::widgets::captions::CaptionCue;
///
/// let c = CaptionCue::new("Hello", 0.0, 2.0);
/// assert!(c.contains(1.0));
/// assert!(!c.contains(3.0));
/// ```
#[derive(Clone, Debug)]
pub struct CaptionCue {
    /// Cue text (may be multi-line).
    pub text: String,
    /// Start time in seconds.
    pub start: f32,
    /// End time in seconds.
    pub end: f32,
}

impl CaptionCue {
    /// A cue active from `start` to `end` seconds.
    ///
    /// ```
    /// use martensite::widgets::captions::CaptionCue;
    ///
    /// assert_eq!(CaptionCue::new("Hi", 0.0, 1.0).text, "Hi");
    /// ```
    pub fn new(text: impl Into<String>, start: f32, end: f32) -> Self {
        Self {
            text: text.into(),
            start,
            end,
        }
    }

    /// `true` when `t` falls inside the cue window.
    ///
    /// ```
    /// use martensite::widgets::captions::CaptionCue;
    ///
    /// assert!(CaptionCue::new("x", 1.0, 2.0).contains(1.5));
    /// ```
    pub fn contains(&self, t: f32) -> bool {
        t >= self.start && t < self.end
    }
}

/// The caption band — see the module docs.
///
/// ```
/// use martensite::widgets::captions::Captions;
///
/// assert_eq!(Captions::new().cue_count(), 0);
/// ```
pub struct Captions {
    /// Accessibility label.
    pub label: String,
    /// Caption font size in points.
    pub font_size: f32,
    cues: Vec<CaptionCue>,
    active: Option<usize>,
    text_painter: Option<SharedTextPainter>,
    bounds: Rect,
    scale: f32,
}

impl std::fmt::Debug for Captions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Captions")
            .field("cues", &self.cues.len())
            .field("active", &self.active)
            .finish()
    }
}

impl Default for Captions {
    fn default() -> Self {
        Self::new()
    }
}

impl Captions {
    /// Empty caption track.
    ///
    /// ```
    /// use martensite::widgets::captions::Captions;
    ///
    /// assert_eq!(Captions::new().cue_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Captions".to_string(),
            font_size: FONT_PT,
            cues: Vec::new(),
            active: None,
            text_painter: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Appends a cue (cues should be pushed in time order).
    ///
    /// ```
    /// use martensite::widgets::captions::Captions;
    ///
    /// assert_eq!(Captions::new().cue("Hi", 0.0, 1.0).cue_count(), 1);
    /// ```
    pub fn cue(mut self, text: impl Into<String>, start: f32, end: f32) -> Self {
        self.cues.push(CaptionCue::new(text, start, end));
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::captions::Captions;
    ///
    /// assert_eq!(Captions::new().label("Subtitles").label, "Subtitles");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Shared text painter.
    ///
    /// ```no_run
    /// use martensite::widgets::captions::Captions;
    /// # fn example(p: martensite::text_paint::SharedTextPainter) {
    /// let _c = Captions::new().with_text_painter(p);
    /// # }
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Cue count.
    ///
    /// ```
    /// use martensite::widgets::captions::Captions;
    ///
    /// assert_eq!(Captions::new().cue_count(), 0);
    /// ```
    pub fn cue_count(&self) -> usize {
        self.cues.len()
    }

    /// Advances the media clock; returns `true` when the active
    /// cue changed (repaint needed).
    ///
    /// ```
    /// use martensite::widgets::captions::Captions;
    /// use std::time::Duration;
    ///
    /// let mut c = Captions::new().cue("A", 0.0, 1.0);
    /// assert!(c.set_position(Duration::from_secs_f32(0.5)));
    /// assert!(!c.set_position(Duration::from_secs_f32(0.7)));
    /// assert!(c.set_position(Duration::from_secs_f32(2.0))); // cue ended
    /// ```
    pub fn set_position(&mut self, t: Duration) -> bool {
        let secs = t.as_secs_f32();
        let next = self.cues.iter().position(|c| c.contains(secs));
        let changed = next != self.active;
        self.active = next;
        changed
    }

    /// Index of the currently active cue.
    ///
    /// ```
    /// use martensite::widgets::captions::Captions;
    /// use std::time::Duration;
    ///
    /// let mut c = Captions::new().cue("A", 0.0, 1.0);
    /// c.set_position(Duration::from_secs_f32(0.5));
    /// assert_eq!(c.active(), Some(0));
    /// ```
    pub fn active(&self) -> Option<usize> {
        self.active
    }

    /// Text of the active cue.
    ///
    /// ```
    /// use martensite::widgets::captions::Captions;
    /// use std::time::Duration;
    ///
    /// let mut c = Captions::new().cue("Hello", 0.0, 1.0);
    /// c.set_position(Duration::from_secs_f32(0.5));
    /// assert_eq!(c.active_text(), Some("Hello"));
    /// ```
    pub fn active_text(&self) -> Option<&str> {
        self.active
            .and_then(|i| self.cues.get(i))
            .map(|c| c.text.as_str())
    }

    /// Drops all cues and clears the active cue.
    ///
    /// ```
    /// use martensite::widgets::captions::Captions;
    /// use std::time::Duration;
    ///
    /// let mut c = Captions::new().cue("A", 0.0, 1.0);
    /// c.set_position(Duration::from_secs_f32(0.5));
    /// c.clear();
    /// assert_eq!(c.active_text(), None);
    /// ```
    pub fn clear(&mut self) {
        self.cues.clear();
        self.active = None;
    }
}

impl Widget for Captions {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let s = cx.scale;
        Vec2::new(
            (320.0 * s).min(constraints.max_size.x.max(0.0)),
            ((self.font_size + PAD_Y_PT * 2.0 + BAND_BOTTOM_PT) * s)
                .min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(120.0, FONT_PT + PAD_Y_PT * 2.0))
            .with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Status);
        node.set_label(self.label.clone());
        if let Some(t) = self.active_text() {
            node.set_value(t.to_string());
        }
    }

    fn event(&mut self, _cx: &mut EventContext) -> EventResponse {
        EventResponse::Ignored
    }

    fn paint(&self, cx: &mut PaintContext) {
        let s = cx.scale;
        let Some(text) = self.active_text() else {
            return;
        };
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let fs = self.font_size * s;
        let lines: Vec<&str> = text.lines().collect();
        let line_h = fs * 1.25;
        let widest = lines
            .iter()
            .filter_map(|l| painter.and_then(|p| p.measure_text(l, fs)))
            .fold(0.0_f32, f32::max)
            .max(
                lines
                    .iter()
                    .fold(0.0_f32, |w, l| w.max(l.len() as f32 * fs * 0.5)),
            );
        let band_w = (widest + PAD_X_PT * 2.0 * s).min(self.bounds.width() * 0.9);
        let band_h = line_h * lines.len() as f32 + PAD_Y_PT * 2.0 * s;
        let bx = self.bounds.min_x() + (self.bounds.width() - band_w) / 2.0;
        let by = self.bounds.max_y() - BAND_BOTTOM_PT * s - band_h;
        let br = kurbo::Rect::new(
            f64::from(bx),
            f64::from(by),
            f64::from(bx + band_w),
            f64::from(by + band_h),
        );
        cx.list
            .push_fill_shape(br, &martensite_core::shape::Shape::rounded(4.0 * s), BAND);
        for (i, line) in lines.iter().enumerate() {
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                br,
                kurbo::Point::new(
                    br.x0 + f64::from(PAD_X_PT * s),
                    br.y0 + f64::from(PAD_Y_PT * s + line_h * (i as f32 + 0.8)),
                ),
                line,
                fs,
                cx.color(TokenKey::TextColor, TEXT),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn fixture() -> Captions {
        Captions::new()
            .cue("Hello", 0.0, 2.0)
            .cue("World", 2.5, 4.0)
    }

    fn laid_out(c: &mut Captions) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        c.layout(&mut cx, Rect::new(0.0, 0.0, 480.0, 270.0));
    }

    #[test]
    fn position_selects_cue() {
        let mut c = fixture();
        assert!(c.set_position(Duration::from_secs_f32(1.0)));
        assert_eq!(c.active_text(), Some("Hello"));
        assert!(c.set_position(Duration::from_secs_f32(3.0)));
        assert_eq!(c.active_text(), Some("World"));
    }

    #[test]
    fn gaps_show_nothing() {
        let mut c = fixture();
        c.set_position(Duration::from_secs_f32(2.2));
        assert_eq!(c.active_text(), None);
    }

    #[test]
    fn set_position_reports_changes() {
        let mut c = fixture();
        assert!(c.set_position(Duration::from_secs_f32(1.0)));
        assert!(!c.set_position(Duration::from_secs_f32(1.5)));
        assert!(c.set_position(Duration::from_secs_f32(9.0)));
    }

    #[test]
    fn paint_only_when_active() {
        let mut c = fixture();
        laid_out(&mut c);
        let theme = martensite_theme::Theme::new("test");
        let mut list = martensite_core::PaintList::default();
        c.paint(&mut PaintContext {
            list: &mut list,
            bounds: c.bounds,
            theme: &theme,
            scale: 1.0,
            text_painter: None,
        });
        assert!(list.is_empty()); // no active cue → nothing painted
        c.set_position(Duration::from_secs_f32(1.0));
        c.paint(&mut PaintContext {
            list: &mut list,
            bounds: c.bounds,
            theme: &theme,
            scale: 1.0,
            text_painter: None,
        });
        assert!(!list.is_empty());
    }
}
