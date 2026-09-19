//! Statistic — a KPI block: title, large formatted value, optional
//! prefix/suffix, and a coloured trend indicator.
//!
//! Mirrors Ant Design `Statistic` and the KPI cards ubiquitous in
//! admin dashboards. Display-only; interactive drill-down belongs to
//! the surrounding `Card`/row.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::{Statistic, Trend};
//!
//! let s = Statistic::new("Uptime", "99.98%")
//!     .trend(Trend::Up, "+0.02%");
//! assert_eq!(s.title(), "Uptime");
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget,
};
use martensite_theme::TokenKey;

/// Title font size in points.
const TITLE_PT: f32 = 12.0;
/// Value font size in points.
const VALUE_PT: f32 = 26.0;
/// Trend font size in points.
const TREND_PT: f32 = 12.0;
/// Vertical rhythm in points.
const GAP_PT: f32 = 4.0;
/// Prefix/suffix inset around the value in points.
const AFFIX_GAP_PT: f32 = 4.0;

/// Trend direction — colours the indicator success-green or
/// error-red.
///
/// # Examples
///
/// ```
/// use martensite::widgets::Trend;
///
/// assert_eq!(Trend::default(), Trend::Neutral);
/// ```
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum Trend {
    /// No directional signal — muted.
    #[default]
    Neutral,
    /// Upward — success green with a `▲` marker.
    Up,
    /// Downward — error red with a `▼` marker.
    Down,
}

/// A KPI block.
///
/// # Examples
///
/// ```
/// use martensite::widgets::Statistic;
///
/// let s = Statistic::new("Revenue", "$12,400");
/// assert_eq!(s.value_text(), "$12,400");
/// ```
pub struct Statistic {
    /// The metric's title.
    title: String,
    /// Pre-formatted value text (grouping/units are the app's call).
    value: String,
    /// Optional prefix drawn ahead of the value (e.g. `$`).
    prefix: Option<String>,
    /// Optional suffix drawn after the value (e.g. `ms`).
    suffix: Option<String>,
    /// Optional trend indicator `(direction, text)`.
    trend: Option<(Trend, String)>,
    /// Whether the widget is enabled.
    pub enabled: bool,
    /// Cached bounds from the last layout pass.
    bounds: Rect,
    /// Shared shaped-text painter — see [`crate::text_paint`].
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl Statistic {
    /// Creates a statistic with `title` and a pre-formatted `value`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Statistic;
    ///
    /// let s = Statistic::new("Latency", "42ms");
    /// assert_eq!(s.title(), "Latency");
    /// ```
    pub fn new(title: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            value: value.into(),
            prefix: None,
            suffix: None,
            trend: None,
            enabled: true,
            bounds: Rect::default(),
            text_painter: None,
        }
    }

    /// Sets a prefix drawn before the value.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Statistic;
    ///
    /// let s = Statistic::new("a", "1").prefix("$");
    /// assert_eq!(s.get_prefix(), Some("$"));
    /// ```
    #[must_use]
    pub fn prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = Some(prefix.into());
        self
    }

    /// Sets a suffix drawn after the value.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Statistic;
    ///
    /// let s = Statistic::new("a", "1").suffix("%");
    /// assert_eq!(s.get_suffix(), Some("%"));
    /// ```
    #[must_use]
    pub fn suffix(mut self, suffix: impl Into<String>) -> Self {
        self.suffix = Some(suffix.into());
        self
    }

    /// Sets the trend indicator — a coloured `▲`/`▼` plus text.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Statistic, Trend};
    ///
    /// let s = Statistic::new("a", "1").trend(Trend::Down, "-2%");
    /// assert_eq!(s.get_trend(), Some((Trend::Down, "-2%")));
    /// ```
    #[must_use]
    pub fn trend(mut self, dir: Trend, text: impl Into<String>) -> Self {
        self.trend = Some((dir, text.into()));
        self
    }

    /// Sets whether the widget is enabled.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Statistic;
    ///
    /// let s = Statistic::new("a", "1").enabled(false);
    /// assert!(!s.enabled);
    /// ```
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Shares a [`crate::text_paint::TextPainter`] so runs emit real
    /// glyphs.
    #[must_use]
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// The metric's title.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Statistic;
    ///
    /// assert_eq!(Statistic::new("a", "1").title(), "a");
    /// ```
    #[inline]
    pub fn title(&self) -> &str {
        &self.title
    }

    /// The pre-formatted value text.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Statistic;
    ///
    /// assert_eq!(Statistic::new("a", "1").value_text(), "1");
    /// ```
    #[inline]
    pub fn value_text(&self) -> &str {
        &self.value
    }

    /// The prefix, if any.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Statistic;
    ///
    /// assert_eq!(Statistic::new("a", "1").get_prefix(), None);
    /// ```
    #[inline]
    pub fn get_prefix(&self) -> Option<&str> {
        self.prefix.as_deref()
    }

    /// The suffix, if any.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Statistic;
    ///
    /// assert_eq!(Statistic::new("a", "1").get_suffix(), None);
    /// ```
    #[inline]
    pub fn get_suffix(&self) -> Option<&str> {
        self.suffix.as_deref()
    }

    /// The trend `(direction, text)`, if any.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Statistic;
    ///
    /// assert_eq!(Statistic::new("a", "1").get_trend(), None);
    /// ```
    #[inline]
    pub fn get_trend(&self) -> Option<(Trend, &str)> {
        self.trend.as_ref().map(|(d, t)| (*d, t.as_str()))
    }

    /// Updates the value in place (live metrics).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Statistic;
    ///
    /// let mut s = Statistic::new("a", "1");
    /// s.set_value("2");
    /// assert_eq!(s.value_text(), "2");
    /// ```
    pub fn set_value(&mut self, value: impl Into<String>) {
        self.value = value.into();
    }
}

impl Default for Statistic {
    fn default() -> Self {
        Self::new("", "0")
    }
}

impl std::fmt::Debug for Statistic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Statistic")
            .field("title", &self.title)
            .field("value", &self.value)
            .field("trend", &self.trend.is_some())
            .finish()
    }
}

impl Widget for Statistic {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let mut h = cx.pt(TITLE_PT + GAP_PT + VALUE_PT + GAP_PT);
        if self.trend.is_some() {
            h += cx.pt(TREND_PT + GAP_PT);
        }
        let max_w = constraints.max_size.x.max(0.0);
        Vec2::new(
            cx.pt(120.0).min(max_w).max(cx.pt(60.0).min(max_w)),
            h.min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(60.0, 40.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, _cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Group);
        // One spoken summary: "Title: value trend".
        let mut text = format!("{}: {}", self.title, self.value);
        if let Some((dir, ref t)) = self.trend {
            let word = match dir {
                Trend::Up => "up",
                Trend::Down => "down",
                Trend::Neutral => "flat",
            };
            text = format!("{text} ({word} {t})");
        }
        node.set_label(text);
        if !self.enabled {
            node.set_disabled();
        }
    }

    fn event(&mut self, _cx: &mut EventContext) -> EventResponse {
        EventResponse::Ignored
    }

    fn paint(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let ink = if self.enabled {
            cx.color(TokenKey::TextColor, [30, 30, 36, 255])
        } else {
            cx.color(TokenKey::TextMutedColor, [110, 110, 118, 255])
        };
        let muted = cx.color(TokenKey::TextMutedColor, [110, 110, 118, 255]);
        let clip = kurbo::Rect::new(
            f64::from(b.min_x()),
            f64::from(b.min_y()),
            f64::from(b.max_x()),
            f64::from(b.max_y()),
        );
        let mut y = b.min_y();
        // Title.
        crate::text_paint::paint_label_clipped(
            painter,
            cx.list,
            clip,
            kurbo::Point::new(f64::from(b.min_x()), f64::from(y)),
            &self.title,
            cx.pt(TITLE_PT),
            muted,
        );
        y += cx.pt(TITLE_PT + GAP_PT);
        // Value line: prefix + value + suffix on one baseline.
        let value_px = cx.pt(VALUE_PT);
        let affix_px = cx.pt(VALUE_PT * 0.55);
        let mut x = b.min_x();
        if let Some(ref p) = self.prefix {
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                clip,
                kurbo::Point::new(f64::from(x), f64::from(y + (value_px - affix_px) / 2.0)),
                p,
                affix_px,
                muted,
            );
            let w = painter
                .and_then(|p_| p_.measure_text(p, affix_px))
                .unwrap_or(affix_px * p.chars().count() as f32 * 0.6);
            x += w + cx.pt(AFFIX_GAP_PT);
        }
        crate::text_paint::paint_label_clipped(
            painter,
            cx.list,
            clip,
            kurbo::Point::new(f64::from(x), f64::from(y)),
            &self.value,
            value_px,
            ink,
        );
        let vw = painter
            .and_then(|p_| p_.measure_text(&self.value, value_px))
            .unwrap_or(value_px * self.value.chars().count() as f32 * 0.55);
        x += vw + cx.pt(AFFIX_GAP_PT);
        if let Some(ref s) = self.suffix {
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                clip,
                kurbo::Point::new(f64::from(x), f64::from(y + (value_px - affix_px) / 2.0)),
                s,
                affix_px,
                muted,
            );
        }
        y += value_px + cx.pt(GAP_PT);
        // Trend line.
        if let Some((dir, ref t)) = self.trend {
            let (mark, col) = match dir {
                Trend::Up => ("▲", cx.color(TokenKey::SuccessColor, [60, 160, 90, 255])),
                Trend::Down => ("▼", cx.color(TokenKey::ErrorColor, [200, 60, 60, 255])),
                Trend::Neutral => ("", muted),
            };
            let trend_text = if mark.is_empty() {
                t.clone()
            } else {
                format!("{mark} {t}")
            };
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                clip,
                kurbo::Point::new(f64::from(b.min_x()), f64::from(y)),
                &trend_text,
                cx.pt(TREND_PT),
                col,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::{HotNode, Theme};

    #[test]
    fn builder() {
        let s = Statistic::new("Uptime", "99.9%")
            .prefix("~")
            .suffix("avg")
            .trend(Trend::Up, "+0.1%")
            .enabled(false);
        assert_eq!(s.title(), "Uptime");
        assert_eq!(s.value_text(), "99.9%");
        assert_eq!(s.get_prefix(), Some("~"));
        assert_eq!(s.get_suffix(), Some("avg"));
        assert_eq!(s.get_trend(), Some((Trend::Up, "+0.1%")));
        assert!(!s.enabled);
    }

    #[test]
    fn set_value_updates() {
        let mut s = Statistic::new("a", "1");
        s.set_value("42");
        assert_eq!(s.value_text(), "42");
    }

    #[test]
    fn measure_grows_with_trend() {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        let c = LayoutConstraints {
            min_size: Vec2::ZERO,
            max_size: Vec2::new(300.0, 300.0),
        };
        let plain = Statistic::new("a", "1").measure(&mut cx, c);
        let trend = Statistic::new("a", "1")
            .trend(Trend::Up, "+1")
            .measure(&mut cx, c);
        assert!(trend.y > plain.y);
    }

    #[test]
    fn accessibility_speaks_summary() {
        let s = Statistic::new("Latency", "42ms").trend(Trend::Down, "-3%");
        let mut node = AccessKitNode::new(accesskit::Role::Unknown);
        s.accessibility(&mut node);
        assert_eq!(node.role(), accesskit::Role::Group);
        let label = node.label().unwrap_or_default();
        assert!(label.contains("Latency"));
        assert!(label.contains("42ms"));
        assert!(label.contains("down"));
    }

    #[test]
    fn paint_emits_runs() {
        let mut s = Statistic::new("a", "1").trend(Trend::Up, "+1");
        let mut hot = HotNode::default();
        let bounds = Rect::new(0.0, 0.0, 160.0, 90.0);
        {
            let mut cx = LayoutContext {
                hot: &mut hot,
                scale: 1.0,
            };
            s.layout(&mut cx, bounds);
        }
        let theme = Theme::new("test");
        let mut list = martensite_core::paint::PaintList::default();
        let mut cx = PaintContext {
            list: &mut list,
            bounds,
            theme: &theme,
            scale: 1.0,
            text_painter: None,
        };
        s.paint(&mut cx);
        assert!(!list.is_empty());
    }

    #[test]
    fn disabled_is_inert() {
        let mut s = Statistic::new("a", "1").enabled(false);
        let ev = martensite_core::WidgetEvent::PointerPressed {
            position: Vec2::new(5.0, 5.0),
            button: martensite_core::PointerButton::Primary,
            count: 1,
        };
        let mut cx = EventContext {
            event: &ev,
            bounds: Rect::new(0.0, 0.0, 100.0, 60.0),
            scale: 1.0,
        };
        assert_eq!(s.event(&mut cx), EventResponse::Ignored);
    }
}
