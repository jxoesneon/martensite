//! `SpeedDial` — a floating action button that fans out labeled
//! mini-actions on click (Material Design `SpeedDial`).
//!
//! Closed, it's a single round button with a `+` glyph; open, a
//! vertical fan of label-chips + mini-buttons rises above it.
//! Clicking a mini-action parks its index in
//! [`SpeedDial::take_action`] and closes the fan; clicking the FAB
//! toggles; `Esc` closes.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::speed_dial::SpeedDial;
//!
//! let mut d = SpeedDial::new()
//!     .action("Compose")
//!     .action("Scan");
//! assert_eq!(d.action_count(), 2);
//! assert!(!d.is_open());
//! d.toggle();
//! assert!(d.is_open());
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;
use parking_lot::Mutex;

const FAB_PT: f32 = 56.0;
const MINI_PT: f32 = 40.0;
const CHIP_PT: f32 = 28.0;
const GAP_PT: f32 = 12.0;
const W_PT: f32 = 200.0;

const FAB_BG: [u8; 4] = [96, 165, 250, 255];
const FAB_FG: [u8; 4] = [18, 18, 22, 255];
const MINI_BG: [u8; 4] = [58, 58, 66, 255];
const MINI_FG: [u8; 4] = [220, 220, 226, 255];
const CHIP_BG: [u8; 4] = [30, 30, 34, 240];
const CHIP_FG: [u8; 4] = [210, 210, 216, 255];

/// A floating-action-button fan — see the module docs.
///
/// ```
/// use martensite::widgets::speed_dial::SpeedDial;
///
/// assert_eq!(SpeedDial::new().action_count(), 0);
/// ```
pub struct SpeedDial {
    /// Accessibility label.
    pub label: String,
    /// Mini-action labels, bottom-up.
    actions: Vec<String>,
    open: bool,
    clicked: Option<usize>,
    bounds: Rect,
    scale: f32,
    /// FAB rect + per-action `(mini, chip)` rects painted last frame.
    hits: Mutex<Hits>,
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

#[derive(Debug, Default)]
struct Hits {
    fab: Option<Rect>,
    minis: Vec<(usize, Rect)>,
}

impl std::fmt::Debug for SpeedDial {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SpeedDial")
            .field("actions", &self.actions.len())
            .field("open", &self.open)
            .finish()
    }
}

impl Default for SpeedDial {
    fn default() -> Self {
        Self::new()
    }
}

impl SpeedDial {
    /// Creates a closed speed dial.
    ///
    /// ```
    /// use martensite::widgets::speed_dial::SpeedDial;
    ///
    /// assert!(!SpeedDial::new().is_open());
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Actions".to_string(),
            actions: Vec::new(),
            open: false,
            clicked: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
            hits: Mutex::new(Hits::default()),
            text_painter: None,
        }
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::speed_dial::SpeedDial;
    ///
    /// assert_eq!(SpeedDial::new().label("Create").label, "Create");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Custom text painter.
    ///
    /// ```
    /// use martensite::widgets::speed_dial::SpeedDial;
    ///
    /// let _ = SpeedDial::new(); // painter optional
    /// ```
    pub fn with_text_painter(mut self, p: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(p);
        self
    }

    /// Appends a mini-action (fans out bottom-up).
    ///
    /// ```
    /// use martensite::widgets::speed_dial::SpeedDial;
    ///
    /// assert_eq!(SpeedDial::new().action("Share").action_count(), 1);
    /// ```
    pub fn action(mut self, label: impl Into<String>) -> Self {
        self.actions.push(label.into());
        self
    }

    /// Mini-action count.
    ///
    /// ```
    /// use martensite::widgets::speed_dial::SpeedDial;
    ///
    /// assert_eq!(SpeedDial::new().action_count(), 0);
    /// ```
    pub fn action_count(&self) -> usize {
        self.actions.len()
    }

    /// Action label at `i` (0 = nearest the FAB).
    ///
    /// ```
    /// use martensite::widgets::speed_dial::SpeedDial;
    ///
    /// let d = SpeedDial::new().action("Compose");
    /// assert_eq!(d.action_label(0), Some("Compose"));
    /// ```
    pub fn action_label(&self, i: usize) -> Option<&str> {
        self.actions.get(i).map(String::as_str)
    }

    /// Whether the fan is open.
    ///
    /// ```
    /// use martensite::widgets::speed_dial::SpeedDial;
    ///
    /// assert!(!SpeedDial::new().is_open());
    /// ```
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Opens the fan.
    ///
    /// ```
    /// use martensite::widgets::speed_dial::SpeedDial;
    ///
    /// let mut d = SpeedDial::new();
    /// d.open();
    /// assert!(d.is_open());
    /// ```
    pub fn open(&mut self) {
        self.open = true;
    }

    /// Closes the fan.
    ///
    /// ```
    /// use martensite::widgets::speed_dial::SpeedDial;
    ///
    /// let mut d = SpeedDial::new();
    /// d.open();
    /// d.close();
    /// assert!(!d.is_open());
    /// ```
    pub fn close(&mut self) {
        self.open = false;
    }

    /// Toggles the fan.
    ///
    /// ```
    /// use martensite::widgets::speed_dial::SpeedDial;
    ///
    /// let mut d = SpeedDial::new();
    /// d.toggle();
    /// assert!(d.is_open());
    /// ```
    pub fn toggle(&mut self) {
        self.open = !self.open;
    }

    /// Drains the index of the last clicked mini-action.
    ///
    /// ```
    /// use martensite::widgets::speed_dial::SpeedDial;
    ///
    /// assert_eq!(SpeedDial::new().take_action(), None);
    /// ```
    pub fn take_action(&mut self) -> Option<usize> {
        self.clicked.take()
    }
}

impl Widget for SpeedDial {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let h = FAB_PT + GAP_PT + self.actions.len() as f32 * (MINI_PT + GAP_PT) + GAP_PT;
        Vec2::new(
            cx.pt(W_PT).min(constraints.max_size.x.max(0.0)),
            cx.pt(h.max(FAB_PT + 8.0))
                .min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(64.0, 64.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Button);
        node.set_label(if self.open {
            format!("{} — expanded", self.label)
        } else {
            self.label.clone()
        });
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                ..
            } => {
                if !self.bounds.contains(*position) {
                    return EventResponse::Ignored;
                }
                let hits = self.hits.lock();
                if let Some((i, _)) = hits.minis.iter().find(|(_, r)| r.contains(*position)) {
                    let i = *i;
                    drop(hits);
                    self.clicked = Some(i);
                    self.open = false;
                    return EventResponse::RequestRepaint;
                }
                if hits.fab.is_some_and(|r| r.contains(*position)) {
                    drop(hits);
                    self.toggle();
                    return EventResponse::RequestRepaint;
                }
                // Click inside bounds but on empty space closes an
                // open fan (scrim behavior).
                if self.open {
                    self.open = false;
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Handled
            }
            WidgetEvent::KeyPressed { key, .. } if key == "Escape" && self.open => {
                self.open = false;
                EventResponse::RequestRepaint
            }
            WidgetEvent::KeyPressed { key, .. }
                if (key == "Enter" || key == "Space") && !self.open =>
            {
                self.open = true;
                EventResponse::RequestRepaint
            }
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
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let s = self.scale;
        let fab_d = FAB_PT * s;
        let mini_d = MINI_PT * s;
        let gap = GAP_PT * s;
        let mut hits = self.hits.lock();
        hits.minis.clear();
        // FAB pinned at the bottom-right of bounds.
        let fab = Rect::new(
            self.bounds.max_x() - fab_d,
            self.bounds.max_y() - fab_d,
            fab_d,
            fab_d,
        );
        hits.fab = Some(fab);
        // Fan rises above the FAB when open.
        if self.open {
            let cx_center = fab.min_x() + fab_d / 2.0;
            for (i, a) in self.actions.iter().enumerate() {
                let my = fab.min_y() - gap - (i as f32 + 1.0) * (mini_d + gap) + gap;
                let mini = Rect::new(cx_center - mini_d / 2.0, my, mini_d, mini_d);
                hits.minis.push((i, mini));
                // Label chip left of the mini button.
                let chip_h = CHIP_PT * s;
                let sz = 10.5 * s;
                let tw = a.chars().count() as f32 * sz * 0.58;
                let chip = Rect::new(
                    mini.min_x() - tw - 16.0 * s,
                    my + (mini_d - chip_h) / 2.0,
                    tw + 12.0 * s,
                    chip_h,
                );
                cx.list.push_fill_shape(
                    krect(chip),
                    &martensite_core::shape::Shape::rounded(chip_h / 2.0),
                    cx.color(TokenKey::BackgroundColor, CHIP_BG),
                );
                crate::text_paint::paint_label(
                    painter,
                    cx.list,
                    kurbo::Point::new(
                        f64::from(chip.min_x() + 6.0 * s),
                        f64::from(chip.min_y() + (chip_h - sz * 1.3) / 2.0 + 1.0 * s),
                    ),
                    a,
                    sz,
                    cx.color(TokenKey::TextColor, CHIP_FG),
                );
                cx.list.push_fill_shape(
                    krect(mini),
                    &martensite_core::shape::Shape::ELLIPSE,
                    cx.color(TokenKey::SurfaceColor, MINI_BG),
                );
                // Mini button shows the label's first glyph.
                let glyph: String = a.chars().take(1).collect();
                crate::text_paint::paint_label(
                    painter,
                    cx.list,
                    kurbo::Point::new(
                        f64::from(cx_center - sz * 0.35),
                        f64::from(my + (mini_d - sz * 1.4) / 2.0 + 2.0 * s),
                    ),
                    &glyph,
                    sz * 1.3,
                    cx.color(TokenKey::TextColor, MINI_FG),
                );
            }
        }
        cx.list.push_fill_shape(
            krect(fab),
            &martensite_core::shape::Shape::ELLIPSE,
            cx.color(TokenKey::AccentColor, FAB_BG),
        );
        let glyph = if self.open { "×" } else { "+" };
        let gsz = 24.0 * s;
        crate::text_paint::paint_label(
            painter,
            cx.list,
            kurbo::Point::new(
                f64::from(fab.min_x() + fab_d / 2.0 - gsz * 0.3),
                f64::from(fab.min_y() + fab_d / 2.0 - gsz * 0.68),
            ),
            glyph,
            gsz,
            cx.color(TokenKey::TextInverseColor, FAB_FG),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;
    use martensite_core::PaintList;

    fn laid_out(w: &mut SpeedDial, wd: f32, h: f32) {
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

    fn painted(w: &SpeedDial) {
        let theme = martensite_theme::Theme::new("test");
        let mut list = PaintList::new();
        let mut cx = PaintContext {
            list: &mut list,
            bounds: w.bounds,
            scale: 1.0,
            theme: &theme,
            text_painter: None,
        };
        w.paint(&mut cx);
    }

    #[test]
    fn toggle_opens_and_closes() {
        let mut d = SpeedDial::new().action("a");
        d.toggle();
        assert!(d.is_open());
        d.toggle();
        assert!(!d.is_open());
    }

    #[test]
    fn fab_click_toggles() {
        let mut d = SpeedDial::new().action("a");
        laid_out(&mut d, 200.0, 200.0);
        painted(&d);
        let fab = d.hits.lock().fab.unwrap();
        d.event(&mut EventContext {
            event: &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(
                    (fab.min_x() + fab.max_x()) / 2.0,
                    (fab.min_y() + fab.max_y()) / 2.0,
                ),
                count: 1,
            },
            bounds: d.bounds,
            scale: 1.0,
        });
        assert!(d.is_open());
    }

    #[test]
    fn mini_click_parks_and_closes() {
        let mut d = SpeedDial::new().action("Compose").action("Scan");
        d.open();
        laid_out(&mut d, 200.0, 220.0);
        painted(&d);
        let mini = d.hits.lock().minis[1].1; // "Scan" — second up the fan
        d.event(&mut EventContext {
            event: &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(
                    (mini.min_x() + mini.max_x()) / 2.0,
                    (mini.min_y() + mini.max_y()) / 2.0,
                ),
                count: 1,
            },
            bounds: d.bounds,
            scale: 1.0,
        });
        assert_eq!(d.take_action(), Some(1));
        assert!(!d.is_open());
    }

    #[test]
    fn escape_closes() {
        let mut d = SpeedDial::new().action("a");
        d.open();
        d.event(&mut EventContext {
            event: &WidgetEvent::KeyPressed {
                key: "Escape".to_string(),
                repeat: false,
            },
            bounds: d.bounds,
            scale: 1.0,
        });
        assert!(!d.is_open());
    }
}
