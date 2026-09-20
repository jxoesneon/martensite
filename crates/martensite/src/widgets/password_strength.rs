//! `PasswordStrength` — a segmented strength meter for password-entry
//! flows (the zxcvbn-meter / signup-form idiom).
//!
//! The host scores a password however it likes (`0`–`4`) and pushes it
//! through [`PasswordStrength::score`] or [`PasswordStrength::set_score`].
//! A [`PasswordStrength::for_password`] heuristic is provided for
//! demos — length plus character-class variety, not a real entropy
//! estimator.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::password_strength::PasswordStrength;
//!
//! let p = PasswordStrength::new().score(3);
//! assert_eq!(p.score_value(), 3);
//! assert_eq!(p.label_for_score(), "Good");
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    LayoutConstraints, LayoutContext, PaintContext, Rect, RenderMinimum, UnderflowPolicy, Widget,
};
use martensite_theme::TokenKey;

const BAR_PT: f32 = 6.0;
const GAP_PT: f32 = 4.0;
const LABEL_PT: f32 = 14.0;

const TRACK: [u8; 4] = [210, 213, 220, 255];
const TEXT: [u8; 4] = [80, 84, 92, 255];
const LEVELS: [[u8; 4]; 5] = [
    [200, 60, 60, 255],  // 0 — very weak
    [230, 130, 50, 255], // 1 — weak
    [230, 190, 40, 255], // 2 — fair
    [140, 190, 70, 255], // 3 — good
    [60, 160, 90, 255],  // 4 — strong
];

/// A five-segment strength meter — see the module docs.
///
/// ```
/// use martensite::widgets::password_strength::PasswordStrength;
///
/// assert_eq!(PasswordStrength::new().score_value(), 0);
/// ```
pub struct PasswordStrength {
    /// Accessibility label.
    pub label: String,
    score: u8,
    /// Draws the score word to the right of the segments.
    show_label: bool,
    bounds: Rect,
    scale: f32,
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl std::fmt::Debug for PasswordStrength {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PasswordStrength")
            .field("score", &self.score)
            .finish()
    }
}

impl Default for PasswordStrength {
    fn default() -> Self {
        Self::new()
    }
}

impl PasswordStrength {
    /// Zero-score meter.
    ///
    /// ```
    /// use martensite::widgets::password_strength::PasswordStrength;
    ///
    /// assert_eq!(PasswordStrength::new().score_value(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Password strength".to_string(),
            score: 0,
            show_label: true,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
            text_painter: None,
        }
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::password_strength::PasswordStrength;
    ///
    /// assert_eq!(PasswordStrength::new().label("New password").label, "New password");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Custom text painter.
    ///
    /// ```
    /// use martensite::widgets::password_strength::PasswordStrength;
    ///
    /// let _ = PasswordStrength::new(); // painter optional
    /// ```
    pub fn with_text_painter(mut self, p: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(p);
        self
    }

    /// Strength score `0`–`4` (clamped).
    ///
    /// ```
    /// use martensite::widgets::password_strength::PasswordStrength;
    ///
    /// assert_eq!(PasswordStrength::new().score(9).score_value(), 4);
    /// ```
    pub fn score(mut self, score: u8) -> Self {
        self.score = score.min(4);
        self
    }

    /// Hides the trailing score word when `off`.
    ///
    /// ```
    /// use martensite::widgets::password_strength::PasswordStrength;
    ///
    /// assert!(!PasswordStrength::new().label_visible(false).is_label_visible());
    /// ```
    pub fn label_visible(mut self, on: bool) -> Self {
        self.show_label = on;
        self
    }

    /// Whether the score word is drawn.
    ///
    /// ```
    /// use martensite::widgets::password_strength::PasswordStrength;
    ///
    /// assert!(PasswordStrength::new().is_label_visible());
    /// ```
    pub fn is_label_visible(&self) -> bool {
        self.show_label
    }

    /// Demo heuristic — scores `password` on length and character-class
    /// variety (`0`–`4`). **Not** an entropy estimator; production apps
    /// should push a real zxcvbn score through [`PasswordStrength::set_score`].
    ///
    /// ```
    /// use martensite::widgets::password_strength::PasswordStrength;
    ///
    /// assert!(PasswordStrength::for_password("a").score_value() < 2);
    /// assert!(PasswordStrength::for_password("Corr3ct-Horse!").score_value() >= 3);
    /// ```
    pub fn for_password(password: &str) -> Self {
        let len = password.chars().count();
        let classes = [
            password.chars().any(|c| c.is_lowercase()),
            password.chars().any(|c| c.is_uppercase()),
            password.chars().any(|c| c.is_ascii_digit()),
            password.chars().any(|c| !c.is_alphanumeric()),
        ]
        .iter()
        .filter(|&&c| c)
        .count();
        let score = if len < 6 {
            0
        } else if len < 8 {
            1
        } else {
            (classes + if len >= 12 { 1 } else { 0 }).min(4) as u8
        };
        Self::new().score(score)
    }

    /// Current score.
    ///
    /// ```
    /// use martensite::widgets::password_strength::PasswordStrength;
    ///
    /// assert_eq!(PasswordStrength::new().score(2).score_value(), 2);
    /// ```
    pub fn score_value(&self) -> u8 {
        self.score
    }

    /// Updates the score (`0`–`4`, clamped).
    ///
    /// ```
    /// use martensite::widgets::password_strength::PasswordStrength;
    ///
    /// let mut p = PasswordStrength::new();
    /// p.set_score(4);
    /// assert_eq!(p.score_value(), 4);
    /// ```
    pub fn set_score(&mut self, score: u8) {
        self.score = score.min(4);
    }

    /// Word for the current score.
    ///
    /// ```
    /// use martensite::widgets::password_strength::PasswordStrength;
    ///
    /// assert_eq!(PasswordStrength::new().score(4).label_for_score(), "Strong");
    /// ```
    pub fn label_for_score(&self) -> &'static str {
        match self.score {
            0 => "Very weak",
            1 => "Weak",
            2 => "Fair",
            3 => "Good",
            _ => "Strong",
        }
    }
}

impl Widget for PasswordStrength {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            cx.pt(200.0).min(constraints.max_size.x.max(0.0)),
            cx.pt(BAR_PT + LABEL_PT)
                .min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(60.0, BAR_PT + 4.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Meter);
        node.set_label(format!("{} — {}", self.label, self.label_for_score()));
        node.set_numeric_value(f64::from(self.score));
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
        let s = self.scale;
        let label_w = if self.show_label { 56.0 * s } else { 0.0 };
        let bar_w = (self.bounds.width() - label_w).max(0.0);
        let seg_w = (bar_w - 4.0 * GAP_PT * s) / 5.0;
        let bar_h = BAR_PT * s;
        let y = self.bounds.min_y() + (self.bounds.height() - bar_h) / 2.0;
        for i in 0..5 {
            let r = Rect::new(
                self.bounds.min_x() + i as f32 * (seg_w + GAP_PT * s),
                y,
                seg_w,
                bar_h,
            );
            // `score` segments plus the leading one, so 0 still shows a
            // red sliver and 4 fills all five.
            let filled = i <= self.score as usize;
            let color = if filled {
                LEVELS[self.score as usize]
            } else {
                cx.color(TokenKey::DividerColor, TRACK)
            };
            cx.list.push_fill_shape(
                krect(r),
                &martensite_core::shape::Shape::rounded(bar_h / 2.0),
                color,
            );
        }
        if self.show_label {
            let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
            let size = 10.0 * s;
            crate::text_paint::paint_label(
                painter,
                cx.list,
                kurbo::Point::new(
                    f64::from(self.bounds.min_x() + bar_w + GAP_PT * s),
                    f64::from(y - (size - bar_h) / 2.0),
                ),
                self.label_for_score(),
                size,
                cx.color(TokenKey::TextMutedColor, TEXT),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::{HotNode, PaintList};

    #[test]
    fn score_clamps() {
        assert_eq!(PasswordStrength::new().score(9).score_value(), 4);
        let mut p = PasswordStrength::new();
        p.set_score(2);
        assert_eq!(p.score_value(), 2);
        p.set_score(7);
        assert_eq!(p.score_value(), 4);
    }

    #[test]
    fn labels_track_score() {
        assert_eq!(
            PasswordStrength::new().score(0).label_for_score(),
            "Very weak"
        );
        assert_eq!(PasswordStrength::new().score(3).label_for_score(), "Good");
        assert_eq!(PasswordStrength::new().score(4).label_for_score(), "Strong");
    }

    #[test]
    fn heuristic_scores() {
        assert_eq!(PasswordStrength::for_password("").score_value(), 0);
        assert_eq!(PasswordStrength::for_password("short1").score_value(), 1);
        assert!(PasswordStrength::for_password("L0ng!Password99").score_value() >= 3);
    }

    #[test]
    fn paints() {
        let mut p = PasswordStrength::new().score(3);
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        p.layout(&mut cx, Rect::new(0.0, 0.0, 200.0, 20.0));
        let theme = martensite_theme::Theme::new("test");
        let mut list = PaintList::new();
        let mut pcx = PaintContext {
            list: &mut list,
            bounds: p.bounds,
            scale: 1.0,
            theme: &theme,
            text_painter: None,
        };
        p.paint(&mut pcx);
    }
}
