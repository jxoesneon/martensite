//! `Waveform` — an amplitude-column audio display (SoundCloud /
//! Audacity waveform idiom).
//!
//! Peaks are `0..=1` amplitudes painted as symmetric columns around
//! the midline; the fraction before [`Waveform::position`] paints
//! in the accent color and the rest muted — the played/unplayed
//! split. Clicking parks the fraction under the pointer in
//! [`Waveform::take_seek`] so the app can seek.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::waveform::Waveform;
//!
//! let w = Waveform::new().peaks([0.2, 0.8, 0.5, 1.0]).position(0.5);
//! assert_eq!(w.peak_count(), 4);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

const WIDTH_PT: f32 = 320.0;
const HEIGHT_PT: f32 = 64.0;

const TRACK: [u8; 4] = [48, 48, 52, 255];
const PLAYED: [u8; 4] = [90, 150, 230, 255];
const UNPLAYED: [u8; 4] = [140, 140, 148, 255];
const FG: [u8; 4] = [230, 230, 235, 255];

/// An amplitude-column audio display — see the module docs.
///
/// ```
/// use martensite::widgets::waveform::Waveform;
///
/// assert_eq!(Waveform::new().peak_count(), 0);
/// ```
pub struct Waveform {
    /// When `false` clicks are ignored.
    pub enabled: bool,
    /// Accessibility label.
    pub label: String,
    peaks: Vec<f32>,
    position: f32,
    pending_seek: Option<f32>,
    hovered: bool,
    bounds: Rect,
}

impl Default for Waveform {
    fn default() -> Self {
        Self::new()
    }
}

impl Waveform {
    /// Creates an empty waveform.
    ///
    /// ```
    /// use martensite::widgets::waveform::Waveform;
    ///
    /// assert_eq!(Waveform::new().peak_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            enabled: true,
            label: "Waveform".to_string(),
            peaks: Vec::new(),
            position: 0.0,
            pending_seek: None,
            hovered: false,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
        }
    }

    /// Peak amplitudes (`0..=1`, clamped).
    ///
    /// ```
    /// use martensite::widgets::waveform::Waveform;
    ///
    /// let w = Waveform::new().peaks([0.5, 2.0]);
    /// assert_eq!(w.peak_list()[1], 1.0);
    /// ```
    pub fn peaks(mut self, peaks: impl IntoIterator<Item = f32>) -> Self {
        self.peaks = peaks.into_iter().map(|p| p.clamp(0.0, 1.0)).collect();
        self
    }

    /// Playhead fraction `0..=1` — the played/unplayed split point.
    ///
    /// ```
    /// use martensite::widgets::waveform::Waveform;
    ///
    /// let w = Waveform::new().position(0.25);
    /// assert_eq!(w.position_value(), 0.25);
    /// ```
    pub fn position(mut self, p: f32) -> Self {
        self.position = p.clamp(0.0, 1.0);
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::waveform::Waveform;
    ///
    /// let w = Waveform::new().label("Take 3");
    /// assert_eq!(w.label, "Take 3");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Enables or disables clicks.
    ///
    /// ```
    /// use martensite::widgets::waveform::Waveform;
    ///
    /// assert!(!Waveform::new().enabled(false).enabled);
    /// ```
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Peak list.
    ///
    /// ```
    /// use martensite::widgets::waveform::Waveform;
    ///
    /// assert!(Waveform::new().peak_list().is_empty());
    /// ```
    pub fn peak_list(&self) -> &[f32] {
        &self.peaks
    }

    /// Peak count.
    ///
    /// ```
    /// use martensite::widgets::waveform::Waveform;
    ///
    /// assert_eq!(Waveform::new().peaks([0.1, 0.2]).peak_count(), 2);
    /// ```
    pub fn peak_count(&self) -> usize {
        self.peaks.len()
    }

    /// Playhead fraction.
    ///
    /// ```
    /// use martensite::widgets::waveform::Waveform;
    ///
    /// assert_eq!(Waveform::new().position_value(), 0.0);
    /// ```
    pub fn position_value(&self) -> f32 {
        self.position
    }

    /// Sets the playhead programmatically.
    ///
    /// ```
    /// use martensite::widgets::waveform::Waveform;
    ///
    /// let mut w = Waveform::new();
    /// w.set_position(0.75);
    /// assert_eq!(w.position_value(), 0.75);
    /// ```
    pub fn set_position(&mut self, p: f32) {
        self.position = p.clamp(0.0, 1.0);
    }

    /// Drains the seek fraction clicked since the last drain.
    ///
    /// ```
    /// use martensite::widgets::waveform::Waveform;
    ///
    /// let mut w = Waveform::new();
    /// assert!(w.take_seek().is_none());
    /// ```
    pub fn take_seek(&mut self) -> Option<f32> {
        self.pending_seek.take()
    }

    /// Fraction `0..1` under a device-space x.
    fn fraction_at(&self, x: f32) -> f32 {
        ((x - self.bounds.min_x()) / self.bounds.width().max(1.0)).clamp(0.0, 1.0)
    }
}

impl Widget for Waveform {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            cx.pt(WIDTH_PT).min(constraints.max_size.x.max(0.0)),
            cx.pt(HEIGHT_PT).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(80.0, 24.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, _cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Image);
        node.set_label(self.label.clone());
        node.set_description(format!("{:.0}% played", self.position * 100.0));
        if self.enabled {
            node.add_action(accesskit::Action::SetValue);
        } else {
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
                if self.bounds.contains(*position) {
                    let f = self.fraction_at(position.x);
                    self.position = f;
                    self.pending_seek = Some(f);
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerMoved { position } => {
                let inside = self.bounds.contains(*position);
                if inside != self.hovered {
                    self.hovered = inside;
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::SemanticAction(martensite_core::widget::SemanticAction::SetValue(
                text,
            )) => {
                if let Ok(v) = text.trim().parse::<f32>() {
                    self.set_position(v);
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
        cx.list
            .push_fill_rect(f(self.bounds), cx.color(TokenKey::SurfaceColor, TRACK));
        if self.peaks.is_empty() {
            return;
        }
        let played = cx.color(TokenKey::AccentColor, PLAYED);
        let unplayed = cx.color(TokenKey::TextMutedColor, UNPLAYED);
        let n = self.peaks.len();
        let col_w = self.bounds.width() / n as f32;
        let mid_y = self.bounds.min_y() + self.bounds.height() / 2.0;
        let half_h = (self.bounds.height() / 2.0 - cx.pt(2.0)).max(1.0);
        let bar_w = (col_w * 0.6).max(1.0).min(cx.pt(4.0));
        let shape = martensite_core::shape::Shape::rounded(bar_w / 2.0);
        for (i, &amp) in self.peaks.iter().enumerate() {
            let x = self.bounds.min_x() + i as f32 * col_w + (col_w - bar_w) / 2.0;
            let h = (amp * half_h).max(cx.pt(1.0));
            let frac = i as f32 / n as f32;
            cx.list.push_fill_shape(
                f(Rect::new(x, mid_y - h, bar_w, h * 2.0)),
                &shape,
                if frac < self.position {
                    played
                } else {
                    unplayed
                },
            );
        }
        // Playhead line.
        let px = self.bounds.min_x() + self.position * self.bounds.width();
        let mut line = kurbo::BezPath::new();
        line.move_to((f64::from(px), f64::from(self.bounds.min_y())));
        line.line_to((f64::from(px), f64::from(self.bounds.max_y())));
        cx.list
            .push_stroke_path(line, cx.pt(1.0), cx.color(TokenKey::TextColor, FG));
    }
}

impl std::fmt::Debug for Waveform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Waveform")
            .field("peaks", &self.peaks.len())
            .field("position", &self.position)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(w: &mut Waveform, width: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        w.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(width, h),
            },
        );
        w.layout(&mut cx, Rect::new(0.0, 0.0, width, h));
    }

    #[test]
    fn peaks_clamp() {
        let w = Waveform::new().peaks([-1.0, 0.5, 3.0]);
        assert_eq!(w.peak_list(), &[0.0, 0.5, 1.0]);
    }

    #[test]
    fn click_seeks() {
        let mut w = Waveform::new().peaks([0.5; 10]);
        laid_out(&mut w, 200.0, 60.0);
        w.event(&mut EventContext {
            event: &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(150.0, 30.0),
                count: 1,
            },
            bounds: Rect::new(0.0, 0.0, 200.0, 60.0),
            scale: 1.0,
        });
        let seek = w.take_seek().unwrap();
        assert!((seek - 0.75).abs() < 0.01);
        assert!((w.position_value() - 0.75).abs() < 0.01);
        assert!(w.take_seek().is_none());
    }

    #[test]
    fn click_outside_ignored() {
        let mut w = Waveform::new().peaks([0.5; 4]);
        laid_out(&mut w, 200.0, 60.0);
        w.event(&mut EventContext {
            event: &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(500.0, 30.0),
                count: 1,
            },
            bounds: Rect::new(0.0, 0.0, 200.0, 60.0),
            scale: 1.0,
        });
        assert!(w.take_seek().is_none());
    }

    #[test]
    fn semantic_set_value() {
        use martensite_core::widget::SemanticAction;
        let mut w = Waveform::new();
        laid_out(&mut w, 200.0, 60.0);
        w.event(&mut EventContext {
            event: &WidgetEvent::SemanticAction(SemanticAction::SetValue("0.5".to_string())),
            bounds: Rect::new(0.0, 0.0, 200.0, 60.0),
            scale: 1.0,
        });
        assert_eq!(w.position_value(), 0.5);
    }

    #[test]
    fn disabled_inert() {
        let mut w = Waveform::new().peaks([0.5; 4]).enabled(false);
        laid_out(&mut w, 200.0, 60.0);
        w.event(&mut EventContext {
            event: &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(100.0, 30.0),
                count: 1,
            },
            bounds: Rect::new(0.0, 0.0, 200.0, 60.0),
            scale: 1.0,
        });
        assert!(w.take_seek().is_none());
    }
}
