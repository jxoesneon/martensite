//! `Weather` — a compact conditions display (icon + temperature +
//! hi/lo — the dashboard weather-card idiom).
//!
//! The widget is display-only: hosts feed a [`WeatherCondition`],
//! temperature, unit, and hi/lo pair. Condition icons are painted
//! shapes (sun disc, cloud blobs, rain/snow drops, bolt) so they
//! render without a text painter; labels use the painter when
//! available.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::weather::{Weather, WeatherCondition};
//!
//! let w = Weather::new()
//!     .location("Lisbon")
//!     .condition(WeatherCondition::Rain)
//!     .temperature(18.4)
//!     .hi_lo(21.0, 14.0);
//! assert_eq!(w.face(), "18°C");
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    LayoutConstraints, LayoutContext, PaintContext, Rect, RenderMinimum, UnderflowPolicy, Widget,
};
use martensite_theme::TokenKey;

const ICON_PT: f32 = 36.0;
const PAD_PT: f32 = 10.0;

const SUN: [u8; 4] = [250, 190, 50, 255];
const CLOUD: [u8; 4] = [170, 175, 185, 255];
const RAIN: [u8; 4] = [90, 150, 240, 255];
const SNOW: [u8; 4] = [230, 235, 245, 255];
const BOLT: [u8; 4] = [250, 210, 60, 255];
const FOG: [u8; 4] = [160, 165, 175, 255];
const FG: [u8; 4] = [235, 235, 240, 255];
const MUTED: [u8; 4] = [140, 144, 152, 255];

/// Sky condition — see [`Weather`].
///
/// ```
/// use martensite::widgets::weather::WeatherCondition;
///
/// assert_eq!(WeatherCondition::Storm.name(), "Storm");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WeatherCondition {
    /// Clear sky.
    #[default]
    Clear,
    /// Sun behind a cloud.
    PartlyCloudy,
    /// Overcast.
    Cloudy,
    /// Rain.
    Rain,
    /// Thunderstorm.
    Storm,
    /// Snow.
    Snow,
    /// Fog.
    Fog,
}

impl WeatherCondition {
    /// Human-readable name.
    ///
    /// ```
    /// use martensite::widgets::weather::WeatherCondition;
    ///
    /// assert_eq!(WeatherCondition::PartlyCloudy.name(), "Partly cloudy");
    /// ```
    pub fn name(self) -> &'static str {
        match self {
            Self::Clear => "Clear",
            Self::PartlyCloudy => "Partly cloudy",
            Self::Cloudy => "Cloudy",
            Self::Rain => "Rain",
            Self::Storm => "Storm",
            Self::Snow => "Snow",
            Self::Fog => "Fog",
        }
    }
}

/// A compact weather display — see the module docs.
///
/// ```
/// use martensite::widgets::weather::Weather;
///
/// assert_eq!(Weather::new().face(), "20°C");
/// ```
pub struct Weather {
    /// Accessibility label.
    pub label: String,
    location: String,
    condition: WeatherCondition,
    temp: f32,
    fahrenheit: bool,
    hi_lo: Option<(f32, f32)>,
    bounds: Rect,
    scale: f32,
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl std::fmt::Debug for Weather {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Weather")
            .field("condition", &self.condition)
            .field("temp", &self.temp)
            .finish()
    }
}

impl Default for Weather {
    fn default() -> Self {
        Self::new()
    }
}

impl Weather {
    /// Clear sky, 20 °C.
    ///
    /// ```
    /// use martensite::widgets::weather::{Weather, WeatherCondition};
    ///
    /// assert_eq!(Weather::new().condition_value(), WeatherCondition::Clear);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Weather".to_string(),
            location: String::new(),
            condition: WeatherCondition::Clear,
            temp: 20.0,
            fahrenheit: false,
            hi_lo: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
            text_painter: None,
        }
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::weather::Weather;
    ///
    /// assert_eq!(Weather::new().label("Now").label, "Now");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Custom text painter.
    ///
    /// ```
    /// use martensite::widgets::weather::Weather;
    ///
    /// let _ = Weather::new(); // painter optional
    /// ```
    pub fn with_text_painter(mut self, p: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(p);
        self
    }

    /// Place name drawn over the temperature.
    ///
    /// ```
    /// use martensite::widgets::weather::Weather;
    ///
    /// assert_eq!(Weather::new().location("Oslo").location_name(), "Oslo");
    /// ```
    pub fn location(mut self, name: impl Into<String>) -> Self {
        self.location = name.into();
        self
    }

    /// The configured place name.
    ///
    /// ```
    /// use martensite::widgets::weather::Weather;
    ///
    /// assert!(Weather::new().location_name().is_empty());
    /// ```
    pub fn location_name(&self) -> &str {
        &self.location
    }

    /// Sky condition.
    ///
    /// ```
    /// use martensite::widgets::weather::{Weather, WeatherCondition};
    ///
    /// assert_eq!(Weather::new().condition(WeatherCondition::Snow).condition_value(), WeatherCondition::Snow);
    /// ```
    pub fn condition(mut self, c: WeatherCondition) -> Self {
        self.condition = c;
        self
    }

    /// Current condition.
    ///
    /// ```
    /// use martensite::widgets::weather::{Weather, WeatherCondition};
    ///
    /// assert_eq!(Weather::new().condition_value(), WeatherCondition::Clear);
    /// ```
    pub fn condition_value(&self) -> WeatherCondition {
        self.condition
    }

    /// Temperature in the configured unit.
    ///
    /// ```
    /// use martensite::widgets::weather::Weather;
    ///
    /// assert_eq!(Weather::new().temperature(-4.2).temperature_value(), -4.2);
    /// ```
    pub fn temperature(mut self, t: f32) -> Self {
        self.temp = t;
        self
    }

    /// Current temperature.
    ///
    /// ```
    /// use martensite::widgets::weather::Weather;
    ///
    /// assert_eq!(Weather::new().temperature_value(), 20.0);
    /// ```
    pub fn temperature_value(&self) -> f32 {
        self.temp
    }

    /// Displays °F instead of °C when `on`.
    ///
    /// ```
    /// use martensite::widgets::weather::Weather;
    ///
    /// assert_eq!(Weather::new().fahrenheit(true).face(), "20°F");
    /// ```
    pub fn fahrenheit(mut self, on: bool) -> Self {
        self.fahrenheit = on;
        self
    }

    /// Whether the unit is Fahrenheit.
    ///
    /// ```
    /// use martensite::widgets::weather::Weather;
    ///
    /// assert!(!Weather::new().is_fahrenheit());
    /// ```
    pub fn is_fahrenheit(&self) -> bool {
        self.fahrenheit
    }

    /// Daily high/low pair drawn under the temperature.
    ///
    /// ```
    /// use martensite::widgets::weather::Weather;
    ///
    /// assert_eq!(Weather::new().hi_lo(25.0, 12.0).hi_lo_value(), Some((25.0, 12.0)));
    /// ```
    pub fn hi_lo(mut self, hi: f32, lo: f32) -> Self {
        self.hi_lo = Some((hi, lo));
        self
    }

    /// The configured high/low pair.
    ///
    /// ```
    /// use martensite::widgets::weather::Weather;
    ///
    /// assert_eq!(Weather::new().hi_lo_value(), None);
    /// ```
    pub fn hi_lo_value(&self) -> Option<(f32, f32)> {
        self.hi_lo
    }

    /// Rounded temperature with unit suffix (`"18°C"`).
    ///
    /// ```
    /// use martensite::widgets::weather::Weather;
    ///
    /// assert_eq!(Weather::new().temperature(18.4).face(), "18°C");
    /// ```
    pub fn face(&self) -> String {
        format!(
            "{}°{}",
            self.temp.round() as i32,
            if self.fahrenheit { "F" } else { "C" }
        )
    }

    /// Full summary used for accessibility (`"Rain, 18°C"`).
    ///
    /// ```
    /// use martensite::widgets::weather::{Weather, WeatherCondition};
    ///
    /// let w = Weather::new().condition(WeatherCondition::Fog);
    /// assert_eq!(w.summary(), "Fog, 20°C");
    /// ```
    pub fn summary(&self) -> String {
        format!("{}, {}", self.condition.name(), self.face())
    }

    /// Paints the condition glyph inside `r`.
    fn paint_icon(&self, cx: &mut PaintContext, r: Rect) {
        let kr = |rr: Rect| {
            kurbo::Rect::new(
                f64::from(rr.min_x()),
                f64::from(rr.min_y()),
                f64::from(rr.max_x()),
                f64::from(rr.max_y()),
            )
        };
        let shape = &martensite_core::shape::Shape::ELLIPSE;
        let sun = |cx: &mut PaintContext, cxm: f32, cym: f32, rad: f32| {
            cx.list.push_fill_shape(
                kr(Rect::new(cxm - rad, cym - rad, rad * 2.0, rad * 2.0)),
                shape,
                SUN,
            );
        };
        let cloud = |cx: &mut PaintContext, x: f32, y: f32, w: f32, h: f32| {
            // Two overlapping ellipses approximate a cloud blob.
            cx.list
                .push_fill_shape(kr(Rect::new(x, y + h * 0.35, w, h * 0.65)), shape, CLOUD);
            cx.list.push_fill_shape(
                kr(Rect::new(x + w * 0.18, y, w * 0.55, h * 0.7)),
                shape,
                CLOUD,
            );
        };
        match self.condition {
            WeatherCondition::Clear => {
                sun(
                    cx,
                    r.min_x() + r.width() / 2.0,
                    r.min_y() + r.height() / 2.0,
                    r.width() * 0.3,
                );
            }
            WeatherCondition::PartlyCloudy => {
                sun(
                    cx,
                    r.min_x() + r.width() * 0.35,
                    r.min_y() + r.height() * 0.35,
                    r.width() * 0.22,
                );
                cloud(
                    cx,
                    r.min_x() + r.width() * 0.25,
                    r.min_y() + r.height() * 0.45,
                    r.width() * 0.65,
                    r.height() * 0.45,
                );
            }
            WeatherCondition::Cloudy => {
                cloud(
                    cx,
                    r.min_x() + r.width() * 0.15,
                    r.min_y() + r.height() * 0.3,
                    r.width() * 0.7,
                    r.height() * 0.5,
                );
            }
            WeatherCondition::Rain => {
                cloud(
                    cx,
                    r.min_x() + r.width() * 0.15,
                    r.min_y() + r.height() * 0.15,
                    r.width() * 0.7,
                    r.height() * 0.5,
                );
                for i in 0..3 {
                    let x = r.min_x() + r.width() * (0.3 + i as f32 * 0.2);
                    let mut drop = kurbo::BezPath::new();
                    drop.move_to((f64::from(x), f64::from(r.min_y() + r.height() * 0.7)));
                    drop.line_to((
                        f64::from(x - r.width() * 0.06),
                        f64::from(r.min_y() + r.height() * 0.95),
                    ));
                    cx.list.push_stroke_path(drop, 1.5 * self.scale, RAIN);
                }
            }
            WeatherCondition::Storm => {
                cloud(
                    cx,
                    r.min_x() + r.width() * 0.15,
                    r.min_y() + r.height() * 0.1,
                    r.width() * 0.7,
                    r.height() * 0.5,
                );
                let mid = r.min_x() + r.width() * 0.5;
                let mut bolt = kurbo::BezPath::new();
                bolt.move_to((
                    f64::from(mid + r.width() * 0.08),
                    f64::from(r.min_y() + r.height() * 0.55),
                ));
                bolt.line_to((
                    f64::from(mid - r.width() * 0.05),
                    f64::from(r.min_y() + r.height() * 0.78),
                ));
                bolt.line_to((
                    f64::from(mid + r.width() * 0.05),
                    f64::from(r.min_y() + r.height() * 0.78),
                ));
                bolt.line_to((f64::from(mid - r.width() * 0.08), f64::from(r.max_y())));
                cx.list.push_stroke_path(bolt, 1.5 * self.scale, BOLT);
            }
            WeatherCondition::Snow => {
                cloud(
                    cx,
                    r.min_x() + r.width() * 0.15,
                    r.min_y() + r.height() * 0.15,
                    r.width() * 0.7,
                    r.height() * 0.5,
                );
                for i in 0..3 {
                    let x = r.min_x() + r.width() * (0.28 + i as f32 * 0.22);
                    let d = r.width() * 0.08;
                    cx.list.push_fill_shape(
                        kr(Rect::new(
                            x - d / 2.0,
                            r.min_y() + r.height() * 0.72 + (i % 2) as f32 * d,
                            d,
                            d,
                        )),
                        shape,
                        SNOW,
                    );
                }
            }
            WeatherCondition::Fog => {
                for i in 0..3 {
                    let y = r.min_y() + r.height() * (0.35 + i as f32 * 0.22);
                    let w = r.width() * (0.75 - i as f32 * 0.1);
                    let mut bar = kurbo::BezPath::new();
                    bar.move_to((f64::from(r.min_x() + r.width() * 0.12), f64::from(y)));
                    bar.line_to((f64::from(r.min_x() + r.width() * 0.12 + w), f64::from(y)));
                    cx.list.push_stroke_path(bar, 2.5 * self.scale, FOG);
                }
            }
        }
    }
}

impl Widget for Weather {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            cx.pt(200.0).min(constraints.max_size.x.max(0.0)),
            cx.pt(ICON_PT + PAD_PT * 2.0)
                .min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(90.0, 36.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Image);
        let mut s = self.summary();
        if !self.location.is_empty() {
            s = format!("{} — {s}", self.location);
        }
        node.set_label(format!("{} — {s}", self.label));
    }

    fn paint(&self, cx: &mut PaintContext) {
        let s = self.scale;
        let icon_r = Rect::new(
            self.bounds.min_x() + PAD_PT * s,
            self.bounds.min_y() + (self.bounds.height() - ICON_PT * s).max(0.0) / 2.0,
            ICON_PT * s,
            ICON_PT * s,
        );
        self.paint_icon(cx, icon_r);

        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let tx = icon_r.max_x() + PAD_PT * s;
        let cy = self.bounds.min_y() + self.bounds.height() / 2.0;
        let loc_sz = 10.0 * s;
        let temp_sz = 20.0 * s;
        let hl_sz = 9.0 * s;
        let fg = cx.color(TokenKey::TextColor, FG);
        let muted = cx.color(TokenKey::TextMutedColor, MUTED);
        if !self.location.is_empty() {
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                kurbo::Rect::new(
                    f64::from(tx),
                    f64::from(self.bounds.min_y()),
                    f64::from(self.bounds.max_x()),
                    f64::from(self.bounds.max_y()),
                ),
                kurbo::Point::new(f64::from(tx), f64::from(cy - temp_sz - loc_sz * 0.4)),
                &self.location,
                loc_sz,
                muted,
            );
        }
        crate::text_paint::paint_label_clipped(
            painter,
            cx.list,
            kurbo::Rect::new(
                f64::from(tx),
                f64::from(self.bounds.min_y()),
                f64::from(self.bounds.max_x()),
                f64::from(self.bounds.max_y()),
            ),
            kurbo::Point::new(f64::from(tx), f64::from(cy - temp_sz * 0.55)),
            &self.face(),
            temp_sz,
            fg,
        );
        if let Some((hi, lo)) = self.hi_lo {
            let t = format!("H {}°  L {}°", hi.round() as i32, lo.round() as i32);
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                kurbo::Rect::new(
                    f64::from(tx),
                    f64::from(self.bounds.min_y()),
                    f64::from(self.bounds.max_x()),
                    f64::from(self.bounds.max_y()),
                ),
                kurbo::Point::new(f64::from(tx), f64::from(cy + temp_sz * 0.45)),
                &t,
                hl_sz,
                muted,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::{HotNode, PaintList};

    #[test]
    fn face_rounds_and_units() {
        assert_eq!(Weather::new().temperature(18.4).face(), "18°C");
        assert_eq!(
            Weather::new().temperature(72.4).fahrenheit(true).face(),
            "72°F"
        );
        assert_eq!(Weather::new().temperature(-3.6).face(), "-4°C");
    }

    #[test]
    fn summary_includes_condition() {
        let w = Weather::new()
            .condition(WeatherCondition::Storm)
            .temperature(9.0);
        assert_eq!(w.summary(), "Storm, 9°C");
    }

    #[test]
    fn hi_lo_round_trips() {
        let w = Weather::new().hi_lo(21.0, 14.0);
        assert_eq!(w.hi_lo_value(), Some((21.0, 14.0)));
    }

    #[test]
    fn paints_all_conditions() {
        let theme = martensite_theme::Theme::new("test");
        for c in [
            WeatherCondition::Clear,
            WeatherCondition::PartlyCloudy,
            WeatherCondition::Cloudy,
            WeatherCondition::Rain,
            WeatherCondition::Storm,
            WeatherCondition::Snow,
            WeatherCondition::Fog,
        ] {
            let mut w = Weather::new().condition(c).location("X").hi_lo(20.0, 10.0);
            let mut hot = HotNode::default();
            let mut cx = LayoutContext {
                hot: &mut hot,
                scale: 1.0,
            };
            w.layout(&mut cx, Rect::new(0.0, 0.0, 200.0, 56.0));
            let mut list = PaintList::new();
            let mut pcx = PaintContext {
                list: &mut list,
                bounds: w.bounds,
                scale: 1.0,
                theme: &theme,
                text_painter: None,
            };
            w.paint(&mut pcx);
        }
    }
}
