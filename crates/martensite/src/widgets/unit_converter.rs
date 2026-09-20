//! `UnitConverter` — the GNOME-Calculator unit converter: a
//! category picker, `from`/`to` unit cells, a numeric value
//! field, and a `⇅` swap button over a computed result line.
//!
//! Ships [`UnitCategory`] tables for length, mass, volume, and
//! temperature. Clicking a unit cell cycles to the next unit in
//! the category; digit keys edit the value; `⇅` or `x` swaps the
//! sides. Every mutation parks [`UnitConverter::take_changed`].
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::unit_converter::{UnitCategory, UnitConverter};
//!
//! let mut c = UnitConverter::new().with_value(1.0);
//! assert!((c.convert().unwrap() - 3.28084).abs() < 1e-4); // 1 m → ft
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::text_paint::SharedTextPainter;

const ROW_PT: f32 = 30.0;
const PAD_PT: f32 = 10.0;
const CELL_PT: f32 = 96.0;
const SWAP_PT: f32 = 28.0;
const FONT_PT: f32 = 13.0;
const RESULT_PT: f32 = 16.0;

const FACE: [u8; 4] = [38, 40, 48, 255];
const CELL: [u8; 4] = [50, 52, 62, 255];
const TEXT: [u8; 4] = [235, 237, 240, 255];
const ACCENT: [u8; 4] = [110, 140, 230, 255];

/// Conversion kind for a unit.
#[derive(Clone, Copy, Debug)]
enum Scale {
    /// Multiply by factor to reach the base unit.
    Factor(f64),
    /// Temperature (nonlinear) — see [`UnitConverter::temp_to_c`].
    TempC,
    TempF,
    TempK,
}

/// A measurement category with its unit table.
///
/// ```
/// use martensite::widgets::unit_converter::UnitCategory;
///
/// assert_eq!(UnitCategory::Length.name(), "Length");
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UnitCategory {
    /// mm / cm / m / km / in / ft / yd / mi
    #[default]
    Length,
    /// mg / g / kg / t / oz / lb
    Mass,
    /// ml / l / m³ / tsp / cup / gal
    Volume,
    /// °C / °F / K
    Temperature,
}

impl UnitCategory {
    /// Display name.
    ///
    /// ```
    /// use martensite::widgets::unit_converter::UnitCategory;
    ///
    /// assert_eq!(UnitCategory::Mass.name(), "Mass");
    /// ```
    pub fn name(&self) -> &'static str {
        match self {
            Self::Length => "Length",
            Self::Mass => "Mass",
            Self::Volume => "Volume",
            Self::Temperature => "Temperature",
        }
    }

    /// All categories (cycle order).
    const ALL: [Self; 4] = [Self::Length, Self::Mass, Self::Volume, Self::Temperature];

    fn units(&self) -> &'static [(&'static str, Scale)] {
        match self {
            Self::Length => &[
                ("mm", Scale::Factor(0.001)),
                ("cm", Scale::Factor(0.01)),
                ("m", Scale::Factor(1.0)),
                ("km", Scale::Factor(1000.0)),
                ("in", Scale::Factor(0.0254)),
                ("ft", Scale::Factor(0.3048)),
                ("yd", Scale::Factor(0.9144)),
                ("mi", Scale::Factor(1609.344)),
            ],
            Self::Mass => &[
                ("mg", Scale::Factor(1e-6)),
                ("g", Scale::Factor(0.001)),
                ("kg", Scale::Factor(1.0)),
                ("t", Scale::Factor(1000.0)),
                ("oz", Scale::Factor(0.0283495)),
                ("lb", Scale::Factor(0.45359237)),
            ],
            Self::Volume => &[
                ("ml", Scale::Factor(0.001)),
                ("l", Scale::Factor(1.0)),
                ("m³", Scale::Factor(1000.0)),
                ("tsp", Scale::Factor(0.00492892)),
                ("cup", Scale::Factor(0.236588)),
                ("gal", Scale::Factor(3.78541)),
            ],
            Self::Temperature => &[
                ("°C", Scale::TempC),
                ("°F", Scale::TempF),
                ("K", Scale::TempK),
            ],
        }
    }
}

/// The converter — see the module docs.
///
/// ```
/// use martensite::widgets::unit_converter::UnitConverter;
///
/// assert_eq!(UnitConverter::new().value(), 1.0);
/// ```
pub struct UnitConverter {
    /// Accessibility label.
    pub label: String,
    category: UnitCategory,
    from: usize,
    to: usize,
    value: f64,
    changed: bool,
    cat_rect: Rect,
    from_rect: Rect,
    to_rect: Rect,
    swap_rect: Rect,
    text_painter: Option<SharedTextPainter>,
    bounds: Rect,
    scale: f32,
}

impl std::fmt::Debug for UnitConverter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UnitConverter")
            .field("category", &self.category)
            .field("value", &self.value)
            .finish()
    }
}

impl Default for UnitConverter {
    fn default() -> Self {
        Self::new()
    }
}

impl UnitConverter {
    /// `1 m → ft` converter.
    ///
    /// ```
    /// use martensite::widgets::unit_converter::UnitConverter;
    ///
    /// assert_eq!(UnitConverter::new().value(), 1.0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Unit converter".to_string(),
            category: UnitCategory::Length,
            from: 2, // m
            to: 5,   // ft
            value: 1.0,
            changed: false,
            cat_rect: Rect::new(0.0, 0.0, 0.0, 0.0),
            from_rect: Rect::new(0.0, 0.0, 0.0, 0.0),
            to_rect: Rect::new(0.0, 0.0, 0.0, 0.0),
            swap_rect: Rect::new(0.0, 0.0, 0.0, 0.0),
            text_painter: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Initial category (resets unit indices to sensible defaults).
    ///
    /// ```
    /// use martensite::widgets::unit_converter::{UnitCategory, UnitConverter};
    ///
    /// assert_eq!(UnitConverter::new().in_category(UnitCategory::Mass).category(), UnitCategory::Mass);
    /// ```
    pub fn in_category(mut self, category: UnitCategory) -> Self {
        self.category = category;
        self.from = 0;
        self.to = 1.min(category.units().len() - 1);
        self
    }

    /// Initial value.
    ///
    /// ```
    /// use martensite::widgets::unit_converter::UnitConverter;
    ///
    /// assert_eq!(UnitConverter::new().with_value(2.5).value(), 2.5);
    /// ```
    pub fn with_value(mut self, value: f64) -> Self {
        self.value = value;
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::unit_converter::UnitConverter;
    ///
    /// assert_eq!(UnitConverter::new().label("Convert").label, "Convert");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Shared text painter.
    ///
    /// ```no_run
    /// use martensite::widgets::unit_converter::UnitConverter;
    /// # fn example(p: martensite::text_paint::SharedTextPainter) {
    /// let _c = UnitConverter::new().with_text_painter(p);
    /// # }
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Current category.
    ///
    /// ```
    /// use martensite::widgets::unit_converter::{UnitCategory, UnitConverter};
    ///
    /// assert_eq!(UnitConverter::new().category(), UnitCategory::Length);
    /// ```
    pub fn category(&self) -> UnitCategory {
        self.category
    }

    /// Current value.
    ///
    /// ```
    /// use martensite::widgets::unit_converter::UnitConverter;
    ///
    /// assert_eq!(UnitConverter::new().value(), 1.0);
    /// ```
    pub fn value(&self) -> f64 {
        self.value
    }

    /// Sets the value (host-driven).
    ///
    /// ```
    /// use martensite::widgets::unit_converter::UnitConverter;
    ///
    /// let mut c = UnitConverter::new();
    /// c.set_value(10.0);
    /// assert_eq!(c.value(), 10.0);
    /// ```
    pub fn set_value(&mut self, value: f64) {
        self.value = value;
        self.changed = true;
    }

    /// `from` unit index.
    ///
    /// ```
    /// use martensite::widgets::unit_converter::UnitConverter;
    ///
    /// assert_eq!(UnitConverter::new().from_unit(), "m");
    /// ```
    pub fn from_unit(&self) -> &'static str {
        self.category.units()[self.from].0
    }

    /// `to` unit index.
    ///
    /// ```
    /// use martensite::widgets::unit_converter::UnitConverter;
    ///
    /// assert_eq!(UnitConverter::new().to_unit(), "ft");
    /// ```
    pub fn to_unit(&self) -> &'static str {
        self.category.units()[self.to].0
    }

    /// Sets the from/to units by name (host-driven).
    ///
    /// ```
    /// use martensite::widgets::unit_converter::UnitConverter;
    ///
    /// let mut c = UnitConverter::new();
    /// c.set_units("km", "mi");
    /// assert!((c.convert().unwrap() - 0.6213712).abs() < 1e-6);
    /// ```
    pub fn set_units(&mut self, from: &str, to: &str) {
        let u = self.category.units();
        if let Some(f) = u.iter().position(|(n, _)| *n == from) {
            self.from = f;
        }
        if let Some(t) = u.iter().position(|(n, _)| *n == to) {
            self.to = t;
        }
        self.changed = true;
    }

    /// Swaps the sides.
    ///
    /// ```
    /// use martensite::widgets::unit_converter::UnitConverter;
    ///
    /// let mut c = UnitConverter::new();
    /// c.swap();
    /// assert_eq!(c.from_unit(), "ft");
    /// ```
    pub fn swap(&mut self) {
        std::mem::swap(&mut self.from, &mut self.to);
        self.changed = true;
    }

    /// Converts `value` into the `to` unit.
    ///
    /// ```
    /// use martensite::widgets::unit_converter::{UnitCategory, UnitConverter};
    ///
    /// let c = UnitConverter::new()
    ///     .in_category(UnitCategory::Temperature)
    ///     .with_value(100.0);
    /// // default from=°C, to=°F
    /// assert_eq!(c.convert(), Some(212.0));
    /// ```
    pub fn convert(&self) -> Option<f64> {
        let units = self.category.units();
        let from = units.get(self.from)?.1;
        let to = units.get(self.to)?.1;
        Some(match (from, to) {
            (Scale::Factor(f), Scale::Factor(t)) => self.value * f / t,
            (fs, ts) => {
                let c = Self::to_celsius(self.value, fs);
                Self::from_celsius(c, ts)
            }
        })
    }

    fn to_celsius(v: f64, s: Scale) -> f64 {
        match s {
            Scale::TempC => v,
            Scale::TempF => (v - 32.0) * 5.0 / 9.0,
            Scale::TempK => v - 273.15,
            Scale::Factor(_) => v,
        }
    }

    fn from_celsius(c: f64, s: Scale) -> f64 {
        match s {
            Scale::TempC => c,
            Scale::TempF => c * 9.0 / 5.0 + 32.0,
            Scale::TempK => c + 273.15,
            Scale::Factor(_) => c,
        }
    }

    /// Drains a change flag (value/unit/category mutation).
    ///
    /// ```
    /// use martensite::widgets::unit_converter::UnitConverter;
    ///
    /// let mut c = UnitConverter::new();
    /// assert!(!c.take_changed());
    /// c.swap();
    /// assert!(c.take_changed());
    /// ```
    pub fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }
}

impl Widget for UnitConverter {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let s = cx.scale;
        Vec2::new(
            (320.0 * s).min(constraints.max_size.x.max(0.0)),
            ((ROW_PT * 3.0 + PAD_PT * 2.0) * s).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(240.0, 90.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        let s = cx.scale;
        let cell = CELL_PT * s;
        let swap = SWAP_PT * s;
        let row = ROW_PT * s;
        let mut x =
            bounds.min_x() + (bounds.width() - (cell * 2.0 + swap + PAD_PT * s)).max(0.0) / 2.0;
        let y1 = bounds.min_y() + PAD_PT * s;
        let y2 = y1 + row;
        self.cat_rect = Rect::new(
            bounds.min_x() + (bounds.width() - cell * 1.4).max(0.0) / 2.0,
            y1,
            cell * 1.4,
            row - 4.0 * s,
        );
        self.from_rect = Rect::new(x, y2, cell, row - 4.0 * s);
        x += cell + PAD_PT * 0.5 * s;
        self.swap_rect = Rect::new(x, y2, swap, row - 4.0 * s);
        x += swap + PAD_PT * 0.5 * s;
        self.to_rect = Rect::new(x, y2, cell, row - 4.0 * s);
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Group);
        node.set_label(self.label.clone());
        if let Some(v) = self.convert() {
            node.set_value(format!(
                "{} {} = {:.4} {}",
                self.value,
                self.from_unit(),
                v,
                self.to_unit()
            ));
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::KeyPressed { key, .. } => match key.as_str() {
                "x" => {
                    self.swap();
                    EventResponse::RequestRepaint
                }
                "ArrowUp" => {
                    self.set_value(self.value + 1.0);
                    EventResponse::RequestRepaint
                }
                "ArrowDown" => {
                    self.set_value(self.value - 1.0);
                    EventResponse::RequestRepaint
                }
                _ => EventResponse::Ignored,
            },
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position,
            } => {
                if self.swap_rect.contains(*position) {
                    self.swap();
                    return EventResponse::RequestRepaint;
                }
                if self.cat_rect.contains(*position) {
                    let i = UnitCategory::ALL
                        .iter()
                        .position(|c| *c == self.category)
                        .unwrap_or(0);
                    self.category = UnitCategory::ALL[(i + 1) % UnitCategory::ALL.len()];
                    self.from = 0;
                    self.to = 1.min(self.category.units().len() - 1);
                    self.changed = true;
                    return EventResponse::RequestRepaint;
                }
                let units = self.category.units().len();
                if self.from_rect.contains(*position) {
                    self.from = (self.from + 1) % units;
                    self.changed = true;
                    return EventResponse::RequestRepaint;
                }
                if self.to_rect.contains(*position) {
                    self.to = (self.to + 1) % units;
                    self.changed = true;
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            _ => EventResponse::Ignored,
        }
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
        cx.list
            .push_fill_rect(krect(self.bounds), cx.color(TokenKey::SurfaceColor, FACE));
        // Category pill.
        cx.list.push_fill_rect(
            krect(self.cat_rect),
            cx.color(TokenKey::SecondaryColor, CELL),
        );
        crate::text_paint::paint_label(
            painter,
            cx.list,
            kurbo::Point::new(
                f64::from(self.cat_rect.min_x() + PAD_PT * 0.6 * s),
                f64::from(self.cat_rect.min_y() + self.cat_rect.height() * 0.68),
            ),
            &format!("{} ▾", self.category.name()),
            FONT_PT * s,
            cx.color(TokenKey::TextColor, TEXT),
        );
        // From/to cells.
        for (rect, text) in [
            (
                self.from_rect,
                format!("{} {}", trim_num(self.value), self.from_unit()),
            ),
            (self.to_rect, self.to_unit().to_string()),
        ] {
            cx.list
                .push_fill_rect(krect(rect), cx.color(TokenKey::SecondaryColor, CELL));
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                krect(rect),
                kurbo::Point::new(
                    f64::from(rect.min_x() + PAD_PT * 0.6 * s),
                    f64::from(rect.min_y() + rect.height() * 0.68),
                ),
                &text,
                FONT_PT * s,
                cx.color(TokenKey::TextColor, TEXT),
            );
        }
        // Swap button.
        let r = self.swap_rect;
        cx.list.push_stroke_rect(
            krect(r),
            s.max(1.0),
            cx.color(TokenKey::BorderColor, ACCENT),
        );
        crate::text_paint::paint_label(
            painter,
            cx.list,
            kurbo::Point::new(
                f64::from(r.min_x() + r.width() * 0.28),
                f64::from(r.min_y() + r.height() * 0.72),
            ),
            "⇅",
            FONT_PT * s,
            cx.color(TokenKey::AccentColor, ACCENT),
        );
        // Result line.
        if let Some(v) = self.convert() {
            let result = format!("= {} {}", trim_num(v), self.to_unit());
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                krect(Rect::new(
                    self.bounds.min_x(),
                    self.from_rect.max_y(),
                    self.bounds.width(),
                    (self.bounds.max_y() - self.from_rect.max_y()).max(0.0),
                )),
                kurbo::Point::new(
                    f64::from(self.bounds.min_x() + PAD_PT * s),
                    f64::from(self.from_rect.max_y() + RESULT_PT * 1.3 * s),
                ),
                &result,
                RESULT_PT * s,
                cx.color(TokenKey::AccentColor, ACCENT),
            );
        }
    }
}

/// Formats a number without a trailing `.0`.
fn trim_num(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{v:.0}")
    } else {
        let s = format!("{v:.4}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(c: &mut UnitConverter) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        c.layout(&mut cx, Rect::new(0.0, 0.0, 320.0, 110.0));
    }

    #[test]
    fn converts_linear() {
        let mut c = UnitConverter::new();
        c.set_units("km", "mi");
        assert!((c.convert().unwrap() - 0.6213712).abs() < 1e-6);
    }

    #[test]
    fn converts_temperature() {
        let c = UnitConverter::new()
            .in_category(UnitCategory::Temperature)
            .with_value(100.0);
        assert_eq!(c.convert(), Some(212.0));
    }

    #[test]
    fn swap_exchanges_units() {
        let mut c = UnitConverter::new();
        c.swap();
        assert_eq!(c.from_unit(), "ft");
        assert_eq!(c.to_unit(), "m");
    }

    #[test]
    fn clicks_cycle_units_and_category() {
        let mut c = UnitConverter::new();
        laid_out(&mut c);
        let ev = |c: &mut UnitConverter, r: Rect| {
            c.event(&mut EventContext {
                event: &WidgetEvent::PointerReleased {
                    button: PointerButton::Primary,
                    position: Vec2::new(
                        (r.min_x() + r.max_x()) / 2.0,
                        (r.min_y() + r.max_y()) / 2.0,
                    ),
                },
                bounds: c.bounds,
                scale: 1.0,
            })
        };
        let r = c.from_rect;
        ev(&mut c, r);
        assert_eq!(c.from_unit(), "km"); // m → next
        let r = c.cat_rect;
        ev(&mut c, r);
        assert_eq!(c.category(), UnitCategory::Mass);
        assert!(c.take_changed());
    }

    #[test]
    fn keys_swap_and_nudge() {
        let mut c = UnitConverter::new();
        laid_out(&mut c);
        for key in ["x", "ArrowUp"] {
            c.event(&mut EventContext {
                event: &WidgetEvent::KeyPressed {
                    key: key.to_string(),
                    repeat: false,
                },
                bounds: c.bounds,
                scale: 1.0,
            });
        }
        assert_eq!(c.from_unit(), "ft");
        assert_eq!(c.value(), 2.0);
    }

    #[test]
    fn paint_without_painter() {
        let mut c = UnitConverter::new();
        laid_out(&mut c);
        let mut list = martensite_core::PaintList::default();
        let theme = martensite_theme::Theme::new("test");
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
