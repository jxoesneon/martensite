//! `Playlist` — an ordered media queue: title/subtitle rows
//! with a duration column, a playing-track marker, and
//! drag-to-reorder (the queue pane of a media app — pairs with
//! [`crate::widgets::media_controls::MediaControls`]).
//!
//! Rows highlight on hover, click selects, and dragging a row
//! reorders the queue — parking `(from, to)` in
//! [`Playlist::take_moved`]. [`Playlist::set_current`] marks the
//! now-playing track; `Next`/`Previous` wrap the marker.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::playlist::{Playlist, Track};
//!
//! let mut p = Playlist::new()
//!     .track(Track::new("Signal", "Cell A").duration(214))
//!     .track(Track::new("Drift", "Cell B").duration(187));
//! assert_eq!(p.track_count(), 2);
//! p.set_current(0);
//! assert_eq!(p.current(), Some(0));
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

const W_PT: f32 = 300.0;
const H_PT: f32 = 240.0;
const ROW_PT: f32 = 34.0;
const PAD_PT: f32 = 6.0;

const FACE: [u8; 4] = [30, 30, 34, 255];
const HOVER: [u8; 4] = [44, 44, 50, 255];
const SELECTED: [u8; 4] = [44, 60, 84, 255];
const TITLE: [u8; 4] = [215, 215, 220, 255];
const SUB: [u8; 4] = [145, 145, 155, 255];
const DUR: [u8; 4] = [130, 130, 140, 255];
const NOW: [u8; 4] = [120, 200, 255, 255];

/// One queue entry — see [`Playlist`].
///
/// ```
/// use martensite::widgets::playlist::Track;
///
/// assert_eq!(Track::new("a", "b").title, "a");
/// ```
#[derive(Debug, Clone)]
pub struct Track {
    /// Primary label.
    pub title: String,
    /// Secondary label (artist, source, cell…).
    pub subtitle: String,
    /// Duration in seconds (`0` hides the column value).
    pub secs: u32,
}

impl Track {
    /// A track with title + subtitle.
    ///
    /// ```
    /// use martensite::widgets::playlist::Track;
    ///
    /// assert_eq!(Track::new("t", "s").secs, 0);
    /// ```
    pub fn new(title: impl Into<String>, subtitle: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            subtitle: subtitle.into(),
            secs: 0,
        }
    }

    /// Duration in seconds.
    ///
    /// ```
    /// use martensite::widgets::playlist::Track;
    ///
    /// assert_eq!(Track::new("t", "s").duration(95).secs, 95);
    /// ```
    pub fn duration(mut self, secs: u32) -> Self {
        self.secs = secs;
        self
    }

    /// `M:SS` display string.
    ///
    /// ```
    /// use martensite::widgets::playlist::Track;
    ///
    /// assert_eq!(Track::new("t", "s").duration(187).clock(), "3:07");
    /// ```
    pub fn clock(&self) -> String {
        format!("{}:{:02}", self.secs / 60, self.secs % 60)
    }
}

/// An ordered media queue — see the module docs.
///
/// ```
/// use martensite::widgets::playlist::Playlist;
///
/// assert_eq!(Playlist::new().track_count(), 0);
/// ```
pub struct Playlist {
    /// Accessibility label.
    pub label: String,
    /// Queue order (index = play position).
    pub tracks: Vec<Track>,
    current: Option<usize>,
    hover: Option<usize>,
    drag: Option<usize>,
    drop_at: Option<usize>,
    /// FIFO of `(from, to)` reorders — several drags can land between
    /// drains, so a single `Option` would silently drop all but the
    /// last.
    moved: std::collections::VecDeque<(usize, usize)>,
    selected: Option<usize>,
    scroll: f32,
    bounds: Rect,
    scale: f32,
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl std::fmt::Debug for Playlist {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Playlist")
            .field("tracks", &self.tracks.len())
            .field("current", &self.current)
            .finish()
    }
}

impl Default for Playlist {
    fn default() -> Self {
        Self::new()
    }
}

impl Playlist {
    /// Creates an empty queue.
    ///
    /// ```
    /// use martensite::widgets::playlist::Playlist;
    ///
    /// assert!(Playlist::new().is_empty());
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Playlist".to_string(),
            tracks: Vec::new(),
            current: None,
            hover: None,
            drag: None,
            drop_at: None,
            moved: std::collections::VecDeque::new(),
            selected: None,
            scroll: 0.0,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
            text_painter: None,
        }
    }

    /// Appends a track.
    ///
    /// ```
    /// use martensite::widgets::playlist::{Playlist, Track};
    ///
    /// assert_eq!(Playlist::new().track(Track::new("a", "b")).track_count(), 1);
    /// ```
    pub fn track(mut self, track: Track) -> Self {
        self.tracks.push(track);
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::playlist::Playlist;
    ///
    /// assert_eq!(Playlist::new().label("Shift mix").label, "Shift mix");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Custom text painter.
    ///
    /// ```
    /// use martensite::widgets::playlist::Playlist;
    ///
    /// let _ = Playlist::new(); // painter optional
    /// ```
    pub fn with_text_painter(mut self, p: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(p);
        self
    }

    /// Track count.
    ///
    /// ```
    /// use martensite::widgets::playlist::{Playlist, Track};
    ///
    /// assert_eq!(Playlist::new().track(Track::new("a", "b")).track_count(), 1);
    /// ```
    pub fn track_count(&self) -> usize {
        self.tracks.len()
    }

    /// Whether the queue is empty.
    ///
    /// ```
    /// use martensite::widgets::playlist::Playlist;
    ///
    /// assert!(Playlist::new().is_empty());
    /// ```
    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty()
    }

    /// Now-playing index.
    ///
    /// ```
    /// use martensite::widgets::playlist::Playlist;
    ///
    /// assert_eq!(Playlist::new().current(), None);
    /// ```
    pub fn current(&self) -> Option<usize> {
        self.current
    }

    /// Marks `i` as now-playing (`None` clears).
    ///
    /// ```
    /// use martensite::widgets::playlist::{Playlist, Track};
    ///
    /// let mut p = Playlist::new().track(Track::new("a", "b"));
    /// p.set_current(0);
    /// assert_eq!(p.current(), Some(0));
    /// ```
    pub fn set_current(&mut self, i: usize) {
        self.current = (i < self.tracks.len()).then_some(i);
    }

    /// Advances the marker to the next track (wraps).
    ///
    /// ```
    /// use martensite::widgets::playlist::{Playlist, Track};
    ///
    /// let mut p = Playlist::new().track(Track::new("a", "b")).track(Track::new("c", "d"));
    /// p.set_current(0);
    /// p.next();
    /// assert_eq!(p.current(), Some(1));
    /// ```
    pub fn next(&mut self) {
        if self.tracks.is_empty() {
            return;
        }
        let i = self
            .current
            .map(|c| (c + 1) % self.tracks.len())
            .unwrap_or(0);
        self.current = Some(i);
    }

    /// Backs the marker up (wraps).
    ///
    /// ```
    /// use martensite::widgets::playlist::{Playlist, Track};
    ///
    /// let mut p = Playlist::new().track(Track::new("a", "b"));
    /// p.previous();
    /// assert_eq!(p.current(), Some(0)); // wraps to itself
    /// ```
    pub fn previous(&mut self) {
        if self.tracks.is_empty() {
            return;
        }
        let n = self.tracks.len();
        let i = self.current.map(|c| (c + n - 1) % n).unwrap_or(0);
        self.current = Some(i);
    }

    /// Selected row.
    ///
    /// ```
    /// use martensite::widgets::playlist::Playlist;
    ///
    /// assert_eq!(Playlist::new().selected(), None);
    /// ```
    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    /// Drains the oldest pending `(from, to)` reorder. Call in a
    /// `while let` loop to consume every reorder since the last drain.
    ///
    /// ```
    /// use martensite::widgets::playlist::Playlist;
    ///
    /// assert_eq!(Playlist::new().take_moved(), None);
    /// ```
    pub fn take_moved(&mut self) -> Option<(usize, usize)> {
        self.moved.pop_front()
    }

    /// Row index under `y` (widget coords).
    fn row_at(&self, y: f32) -> Option<usize> {
        let row = ROW_PT * self.scale;
        let pad = PAD_PT * self.scale;
        let i = ((y - self.bounds.min_y() - pad + self.scroll) / row) as usize;
        (i < self.tracks.len()).then_some(i)
    }

    /// Drop slot under `y` (`0..=len`).
    fn slot_at(&self, y: f32) -> usize {
        let row = ROW_PT * self.scale;
        let pad = PAD_PT * self.scale;
        let i = ((y - self.bounds.min_y() - pad + self.scroll) / row + 0.5) as usize;
        i.min(self.tracks.len())
    }

    /// Max scroll offset.
    fn max_scroll(&self) -> f32 {
        let row = ROW_PT * self.scale;
        let pad = PAD_PT * self.scale;
        (self.tracks.len() as f32 * row + pad * 2.0 - self.bounds.height()).max(0.0)
    }
}

impl Widget for Playlist {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            cx.pt(W_PT).min(constraints.max_size.x.max(0.0)),
            cx.pt(H_PT).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(140.0, 60.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        self.scroll = self.scroll.clamp(0.0, self.max_scroll());
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::List);
        node.set_label(format!(
            "{} — {} tracks{}",
            self.label,
            self.tracks.len(),
            self.current
                .map(|c| format!(", playing {}", c + 1))
                .unwrap_or_default()
        ));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::Scroll { position, delta } => {
                if self.bounds.contains(*position) {
                    self.scroll = (self.scroll - delta.y).clamp(0.0, self.max_scroll());
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                ..
            } => {
                if !self.bounds.contains(*position) {
                    return EventResponse::Ignored;
                }
                if let Some(i) = self.row_at(position.y) {
                    self.selected = Some(i);
                    self.drag = Some(i);
                    self.drop_at = Some(i);
                    return EventResponse::CapturePointer;
                }
                EventResponse::Handled
            }
            WidgetEvent::PointerMoved { position } => {
                if let Some(i) = self.drag {
                    let slot = self.slot_at(position.y);
                    if Some(slot) != self.drop_at {
                        self.drop_at = Some(slot);
                        return EventResponse::RequestRepaint;
                    }
                    let _ = i;
                    return EventResponse::Ignored;
                }
                let h = if self.bounds.contains(*position) {
                    self.row_at(position.y)
                } else {
                    None
                };
                if h != self.hover {
                    self.hover = h;
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position,
            } => {
                if let Some(from) = self.drag.take() {
                    let mut to = self.slot_at(position.y);
                    if to > from {
                        to -= 1;
                    }
                    if to != from && to < self.tracks.len() {
                        let t = self.tracks.remove(from);
                        self.tracks.insert(to, t);
                        // Keep markers aligned with the moved track.
                        let remap = |x: Option<usize>| {
                            x.map(|i| {
                                if i == from {
                                    to
                                } else if from < to && i > from && i <= to {
                                    i - 1
                                } else if to < from && i >= to && i < from {
                                    i + 1
                                } else {
                                    i
                                }
                            })
                        };
                        self.current = remap(self.current);
                        self.selected = remap(self.selected);
                        self.moved.push_back((from, to));
                    }
                    self.drop_at = None;
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerLeave => {
                if self.hover.take().is_some() {
                    EventResponse::RequestRepaint
                } else {
                    EventResponse::Ignored
                }
            }
            WidgetEvent::KeyPressed { key, .. } => match key.as_str() {
                "Next Track" | "n" => {
                    self.next();
                    EventResponse::RequestRepaint
                }
                "Previous Track" | "p" => {
                    self.previous();
                    EventResponse::RequestRepaint
                }
                _ => EventResponse::Ignored,
            },
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let krect = |r: Rect| {
            kurbo::Rect::new(
                f64::from(r.min_x()),
                f64::from(r.min_y()),
                f64::from(r.max_x()),
                f64::from(r.max_y()),
            )
        };
        cx.list.push_fill_shape(
            krect(self.bounds),
            &martensite_core::shape::Shape::RECT,
            cx.color(TokenKey::SurfaceColor, FACE),
        );
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let s = self.scale;
        let row = ROW_PT * s;
        let pad = PAD_PT * s;
        let title_sz = 12.0 * s;
        let sub_sz = 9.5 * s;
        cx.list.push_clip(krect(self.bounds));
        for (i, t) in self.tracks.iter().enumerate() {
            let y = self.bounds.min_y() + pad + i as f32 * row - self.scroll;
            if y + row < self.bounds.min_y() || y > self.bounds.max_y() {
                continue;
            }
            if self.drag.is_none() {
                if self.selected == Some(i) {
                    cx.list.push_fill_shape(
                        kurbo::Rect::new(
                            f64::from(self.bounds.min_x()),
                            f64::from(y),
                            f64::from(self.bounds.max_x()),
                            f64::from(y + row),
                        ),
                        &martensite_core::shape::Shape::RECT,
                        SELECTED,
                    );
                } else if self.hover == Some(i) {
                    cx.list.push_fill_shape(
                        kurbo::Rect::new(
                            f64::from(self.bounds.min_x()),
                            f64::from(y),
                            f64::from(self.bounds.max_x()),
                            f64::from(y + row),
                        ),
                        &martensite_core::shape::Shape::RECT,
                        HOVER,
                    );
                }
            }
            // Now-playing marker.
            let x = self.bounds.min_x() + pad + 4.0 * s;
            if self.current == Some(i) {
                crate::text_paint::paint_label(
                    painter,
                    cx.list,
                    kurbo::Point::new(f64::from(x), f64::from(y + row * 0.3)),
                    "▶",
                    sub_sz,
                    cx.color(TokenKey::AccentColor, NOW),
                );
            }
            crate::text_paint::paint_label(
                painter,
                cx.list,
                kurbo::Point::new(f64::from(x + 14.0 * s), f64::from(y + 5.0 * s)),
                &t.title,
                title_sz,
                cx.color(TokenKey::TextColor, TITLE),
            );
            crate::text_paint::paint_label(
                painter,
                cx.list,
                kurbo::Point::new(
                    f64::from(x + 14.0 * s),
                    f64::from(y + 5.0 * s + title_sz * 1.3),
                ),
                &t.subtitle,
                sub_sz,
                cx.color(TokenKey::TextMutedColor, SUB),
            );
            if t.secs > 0 {
                let clock = t.clock();
                let w = clock.chars().count() as f32 * sub_sz * 0.55;
                crate::text_paint::paint_label(
                    painter,
                    cx.list,
                    kurbo::Point::new(
                        f64::from(self.bounds.max_x() - pad - w),
                        f64::from(y + row * 0.3),
                    ),
                    &clock,
                    sub_sz,
                    cx.color(TokenKey::TextMutedColor, DUR),
                );
            }
        }
        // Drop indicator while dragging.
        if let Some(slot) = self.drop_at {
            if self.drag.is_some() {
                let y = self.bounds.min_y() + pad + slot as f32 * row - self.scroll - s;
                cx.list.push_fill_shape(
                    kurbo::Rect::new(
                        f64::from(self.bounds.min_x() + pad),
                        f64::from(y),
                        f64::from(self.bounds.max_x() - pad),
                        f64::from(y + 2.0 * s),
                    ),
                    &martensite_core::shape::Shape::RECT,
                    cx.color(TokenKey::AccentColor, NOW),
                );
            }
        }
        cx.list.pop_clip();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn queue() -> Playlist {
        Playlist::new()
            .track(Track::new("Signal", "Cell A").duration(214))
            .track(Track::new("Drift", "Cell B").duration(187))
            .track(Track::new("Pulse", "Cell C").duration(243))
    }

    fn laid_out(w: &mut Playlist, wd: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        w.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(wd, h),
            },
        );
        w.layout(&mut cx, Rect::new(0.0, 0.0, wd, h));
    }

    fn ev(w: &mut Playlist, e: &WidgetEvent) {
        w.event(&mut EventContext {
            event: e,
            bounds: w.bounds,
            scale: 1.0,
        });
    }

    #[test]
    fn track_clock() {
        assert_eq!(Track::new("t", "s").duration(187).clock(), "3:07");
        assert_eq!(Track::new("t", "s").clock(), "0:00");
    }

    #[test]
    fn next_wraps() {
        let mut p = queue();
        p.set_current(2);
        p.next();
        assert_eq!(p.current(), Some(0));
        p.previous();
        assert_eq!(p.current(), Some(2));
    }

    #[test]
    fn click_selects() {
        let mut p = queue();
        laid_out(&mut p, 300.0, 240.0);
        ev(
            &mut p,
            &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(150.0, 6.0 + ROW_PT + 10.0), // row 1
                count: 1,
            },
        );
        ev(
            &mut p,
            &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: Vec2::new(150.0, 6.0 + ROW_PT + 10.0),
            },
        );
        assert_eq!(p.selected(), Some(1));
    }

    #[test]
    fn drag_reorders() {
        let mut p = queue();
        p.set_current(0);
        laid_out(&mut p, 300.0, 240.0);
        // Grab row 0, drop below row 2.
        ev(
            &mut p,
            &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(150.0, 16.0),
                count: 1,
            },
        );
        ev(
            &mut p,
            &WidgetEvent::PointerMoved {
                position: Vec2::new(150.0, 6.0 + ROW_PT * 2.8),
            },
        );
        ev(
            &mut p,
            &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: Vec2::new(150.0, 6.0 + ROW_PT * 2.8),
            },
        );
        assert_eq!(p.take_moved(), Some((0, 2)));
        assert_eq!(p.tracks[2].title, "Signal");
        assert_eq!(p.current(), Some(2)); // marker followed the move
    }
}
