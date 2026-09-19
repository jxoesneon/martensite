//! `SettingsRow` + `SettingsGroup` — the preferences-page idiom:
//! icon + title + subtitle rows with a trailing control, grouped
//! under a titled header (Libadwaita `ActionRow`/`PreferencesGroup`,
//! WinUI/WCT `SettingsCard`/`SettingsExpander`, iOS Settings rows).
//!
//! A row's body press activates the row itself (poll
//! [`SettingsRow::take_activated`]); presses inside the trailing
//! widget's zone forward to that child. Groups lay rows out
//! vertically with hairline separators.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::settings_row::{SettingsGroup, SettingsRow};
//! use martensite::widgets::switch::Switch;
//!
//! let row = SettingsRow::new("Wi-Fi").subtitle("Connected");
//! let group = SettingsGroup::new("Network").row(row);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::widget::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Widget, WidgetEvent,
};
use martensite_core::{Rect, TokenKey};

/// Row height, logical points.
const ROW_PT: f32 = 52.0;
/// Compact row height (no subtitle), logical points.
const ROW_COMPACT_PT: f32 = 40.0;
/// Horizontal padding, logical points.
const PAD_PT: f32 = 14.0;
/// Icon slot width, logical points.
const ICON_PT: f32 = 28.0;
/// Icon glyph size, logical points.
const ICON_FONT_PT: f32 = 18.0;
/// Title font size, logical points.
const TITLE_PT: f32 = 13.0;
/// Subtitle font size, logical points.
const SUBTITLE_PT: f32 = 11.0;
/// Gap between title block and trailing widget, logical points.
const TRAIL_GAP_PT: f32 = 12.0;
/// Group header font size, logical points.
const HEADER_PT: f32 = 12.0;
/// Group header bottom gap, logical points.
const HEADER_GAP_PT: f32 = 6.0;
/// Row corner radius inside a group, logical points.
const GROUP_RADIUS_PT: f32 = 10.0;
/// Separator inset, logical points.
const SEP_INSET_PT: f32 = 14.0;

/// Title ink.
const TITLE_INK: [u8; 4] = [30, 31, 36, 255];
/// Subtitle ink.
const SUB_INK: [u8; 4] = [110, 114, 123, 255];
/// Group face.
const GROUP_FACE: [u8; 4] = [245, 246, 248, 255];
/// Row hover tint.
const HOVER: [u8; 4] = [30, 31, 36, 8];
/// Separator ink.
const SEP: [u8; 4] = [222, 225, 231, 255];
/// Header ink.
const HEADER_INK: [u8; 4] = [110, 114, 123, 255];

/// A preferences/settings row.
///
/// # Examples
///
/// ```
/// use martensite::widgets::settings_row::SettingsRow;
///
/// let r = SettingsRow::new("Appearance").icon("🎨").activatable(true);
/// ```
pub struct SettingsRow {
    /// Optional leading icon glyph.
    icon: Option<String>,
    /// Row title.
    title: String,
    /// Optional subtitle line.
    subtitle: Option<String>,
    /// Trailing control (switch, dropdown, button, chevron…).
    trailing: Option<Box<dyn Widget>>,
    /// Whether a body press activates the row.
    activatable: bool,
    /// Body press pending.
    activated: bool,
    /// Hovered state.
    highlighted: bool,
    /// Enabled flag.
    enabled: bool,
    /// Trailing child bounds from the last layout.
    trailing_rect: Rect,
    /// Cached bounds.
    bounds: Rect,
    /// Shared shaped-text painter.
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl SettingsRow {
    /// Creates a row with a title.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::settings_row::SettingsRow;
    ///
    /// let r = SettingsRow::new("Notifications");
    /// ```
    #[must_use]
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            icon: None,
            title: title.into(),
            subtitle: None,
            trailing: None,
            activatable: false,
            activated: false,
            highlighted: false,
            enabled: true,
            trailing_rect: Rect::default(),
            bounds: Rect::default(),
            text_painter: None,
        }
    }

    /// Sets the leading icon glyph.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::settings_row::SettingsRow;
    ///
    /// let r = SettingsRow::new("Airplane Mode").icon("✈");
    /// ```
    #[must_use]
    pub fn icon(mut self, glyph: impl Into<String>) -> Self {
        self.icon = Some(glyph.into());
        self
    }

    /// Sets the subtitle.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::settings_row::SettingsRow;
    ///
    /// let r = SettingsRow::new("Wi-Fi").subtitle("HomeNet-5G");
    /// ```
    #[must_use]
    pub fn subtitle(mut self, text: impl Into<String>) -> Self {
        self.subtitle = Some(text.into());
        self
    }

    /// Sets the trailing control.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::settings_row::SettingsRow;
    /// use martensite::widgets::switch::Switch;
    ///
    /// let r = SettingsRow::new("Dark mode").trailing(Switch::new(""));
    /// ```
    #[must_use]
    pub fn trailing(mut self, child: impl Widget + 'static) -> Self {
        self.trailing = Some(Box::new(child));
        self
    }

    /// Whether a body press activates the row (default false —
    /// passive rows still highlight).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::settings_row::SettingsRow;
    ///
    /// let r = SettingsRow::new("About").activatable(true);
    /// ```
    #[must_use]
    pub fn activatable(mut self, activatable: bool) -> Self {
        self.activatable = activatable;
        self
    }

    /// Enables or disables the row.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::settings_row::SettingsRow;
    ///
    /// let r = SettingsRow::new("Row").enabled(false);
    /// ```
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// The row title.
    #[inline]
    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Drains a body-press activation.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::settings_row::SettingsRow;
    ///
    /// let mut r = SettingsRow::new("Row");
    /// assert!(!r.take_activated());
    /// ```
    pub fn take_activated(&mut self) -> bool {
        std::mem::take(&mut self.activated)
    }

    /// Accesses the trailing control.
    #[inline]
    #[must_use]
    pub fn trailing_widget(&self) -> Option<&dyn Widget> {
        self.trailing.as_deref()
    }

    /// Accesses the trailing control mutably (e.g. to poll its
    /// own `take_*` seams).
    #[inline]
    #[must_use]
    pub fn trailing_widget_mut(&mut self) -> Option<&mut dyn Widget> {
        self.trailing.as_deref_mut()
    }

    /// Installs a shared shaped-text painter.
    #[must_use]
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }
}

impl Widget for SettingsRow {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let h = if self.subtitle.is_some() {
            ROW_PT
        } else {
            ROW_COMPACT_PT
        };
        Vec2::new(constraints.max_size.x.max(cx.pt(200.0)), cx.pt(h))
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.trailing_rect = Rect::default();
        if let Some(child) = self.trailing.as_mut() {
            // Right-dock the trailing control at its measured size.
            let size = child.measure(
                cx,
                LayoutConstraints {
                    min_size: Vec2::ZERO,
                    max_size: bounds.size,
                },
            );
            let pad = cx.pt(PAD_PT);
            let w = size.x.min(bounds.size.x / 3.0);
            let r = Rect::new(
                bounds.max_x() - pad - w,
                bounds.origin.y + (bounds.size.y - size.y.min(bounds.size.y)) / 2.0,
                w,
                size.y.min(bounds.size.y - cx.pt(8.0)),
            );
            self.trailing_rect = r;
            cx.layout_child(child.as_mut(), r);
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::ListItem);
        node.set_label(self.title.clone());
        if let Some(sub) = &self.subtitle {
            node.set_description(sub.clone());
        }
        if !self.enabled {
            node.set_disabled();
        }
        if self.enabled && self.activatable {
            node.add_action(accesskit::Action::Click);
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.enabled {
            return EventResponse::Ignored;
        }
        match cx.event {
            WidgetEvent::PointerMoved { position } => {
                let hot =
                    self.bounds.contains(*position) && !self.trailing_rect.contains(*position);
                if hot != self.highlighted {
                    self.highlighted = hot;
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerLeave => {
                self.highlighted = false;
                EventResponse::Ignored
            }
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                ..
            } => {
                // The trailing control's zone belongs to the child.
                if self.trailing_rect.contains(*position) {
                    return EventResponse::Ignored;
                }
                if self.activatable && self.bounds.contains(*position) {
                    self.activated = true;
                    return EventResponse::Handled;
                }
                EventResponse::Ignored
            }
            WidgetEvent::KeyPressed { key, .. } => match key.as_str() {
                "Enter" | "Space" | " " if self.activatable => {
                    self.activated = true;
                    EventResponse::Handled
                }
                _ => EventResponse::Ignored,
            },
            WidgetEvent::SemanticAction(action) => match action {
                martensite_core::widget::SemanticAction::Click if self.activatable => {
                    self.activated = true;
                    EventResponse::Handled
                }
                _ => EventResponse::Ignored,
            },
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let r = kurbo::Rect::new(
            f64::from(cx.bounds.min_x()),
            f64::from(cx.bounds.min_y()),
            f64::from(cx.bounds.max_x()),
            f64::from(cx.bounds.max_y()),
        );
        if self.highlighted {
            cx.list.push_fill_shape(
                r,
                &martensite_core::shape::Shape::rounded(cx.pt(6.0)),
                HOVER,
            );
        }
        let pad = cx.pt(PAD_PT);
        let mut x = cx.bounds.min_x() + pad;
        // Icon slot.
        if let Some(icon) = &self.icon {
            let size = cx.pt(ICON_FONT_PT);
            let w = painter
                .and_then(|p| p.measure_text(icon, size))
                .unwrap_or(size);
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                kurbo::Rect::new(
                    f64::from(x),
                    f64::from(cx.bounds.min_y()),
                    f64::from(x + cx.pt(ICON_PT)),
                    f64::from(cx.bounds.max_y()),
                ),
                kurbo::Point::new(
                    f64::from(x + (cx.pt(ICON_PT) - w) / 2.0),
                    f64::from(cx.bounds.min_y() + (cx.bounds.size.y - size) / 2.0),
                ),
                icon,
                size,
                cx.color(TokenKey::TextColor, TITLE_INK),
            );
            x += cx.pt(ICON_PT) + cx.pt(4.0);
        }
        // Title + subtitle block.
        let title_size = cx.pt(TITLE_PT);
        let sub_size = cx.pt(SUBTITLE_PT);
        let text_right = if self.trailing.is_some() {
            self.trailing_rect.min_x() - cx.pt(TRAIL_GAP_PT)
        } else {
            cx.bounds.max_x() - pad
        };
        let clip = kurbo::Rect::new(
            f64::from(x),
            f64::from(cx.bounds.min_y()),
            f64::from(text_right),
            f64::from(cx.bounds.max_y()),
        );
        if let Some(sub) = &self.subtitle {
            let total = title_size + sub_size + cx.pt(2.0);
            let ty = cx.bounds.min_y() + (cx.bounds.size.y - total) / 2.0;
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                clip,
                kurbo::Point::new(f64::from(x), f64::from(ty)),
                &self.title,
                title_size,
                cx.color(TokenKey::TextColor, TITLE_INK),
            );
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                clip,
                kurbo::Point::new(f64::from(x), f64::from(ty + title_size + cx.pt(2.0))),
                sub,
                sub_size,
                cx.color(TokenKey::TextMutedColor, SUB_INK),
            );
        } else {
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                clip,
                kurbo::Point::new(
                    f64::from(x),
                    f64::from(cx.bounds.min_y() + (cx.bounds.size.y - title_size) / 2.0),
                ),
                &self.title,
                title_size,
                cx.color(TokenKey::TextColor, TITLE_INK),
            );
        }
    }

    fn child_count(&self) -> usize {
        self.trailing.is_some() as usize
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        if index == 0 {
            self.trailing.as_deref()
        } else {
            None
        }
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        if index == 0 {
            self.trailing.as_deref_mut()
        } else {
            None
        }
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        (index == 0 && self.trailing.is_some()).then_some(self.trailing_rect)
    }
}

impl std::fmt::Debug for SettingsRow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SettingsRow")
            .field("title", &self.title)
            .finish()
    }
}

/// A titled group of settings rows.
///
/// # Examples
///
/// ```
/// use martensite::widgets::settings_row::{SettingsGroup, SettingsRow};
///
/// let g = SettingsGroup::new("General")
///     .row(SettingsRow::new("About"))
///     .row(SettingsRow::new("Updates"));
/// assert_eq!(g.row_count(), 2);
/// ```
pub struct SettingsGroup {
    /// Group header text.
    header: String,
    /// Rows in order.
    rows: Vec<SettingsRow>,
    /// Whether rows sit in a rounded card (vs flat list).
    carded: bool,
    /// Shared shaped-text painter.
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl SettingsGroup {
    /// Creates a group with a header.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::settings_row::SettingsGroup;
    ///
    /// let g = SettingsGroup::new("Privacy");
    /// ```
    #[must_use]
    pub fn new(header: impl Into<String>) -> Self {
        Self {
            header: header.into(),
            rows: Vec::new(),
            carded: true,
            text_painter: None,
        }
    }

    /// Appends a row.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::settings_row::{SettingsGroup, SettingsRow};
    ///
    /// let g = SettingsGroup::new("G").row(SettingsRow::new("Row"));
    /// ```
    #[must_use]
    pub fn row(mut self, row: SettingsRow) -> Self {
        self.rows.push(row);
        self
    }

    /// Whether rows paint inside a rounded card (default true).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::settings_row::SettingsGroup;
    ///
    /// let g = SettingsGroup::new("G").carded(false);
    /// ```
    #[must_use]
    pub fn carded(mut self, carded: bool) -> Self {
        self.carded = carded;
        self
    }

    /// The number of rows.
    #[inline]
    #[must_use]
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// Accesses a row by index.
    #[inline]
    #[must_use]
    pub fn row_at(&self, index: usize) -> Option<&SettingsRow> {
        self.rows.get(index)
    }

    /// Accesses a row mutably (e.g. to drain `take_activated`).
    #[inline]
    #[must_use]
    pub fn row_at_mut(&mut self, index: usize) -> Option<&mut SettingsRow> {
        self.rows.get_mut(index)
    }

    /// Installs a shared shaped-text painter on the group and rows.
    #[must_use]
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        for row in &mut self.rows {
            row.text_painter = Some(painter.clone());
        }
        self.text_painter = Some(painter);
        self
    }
}

impl Widget for SettingsGroup {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let header_h = if self.header.is_empty() {
            0.0
        } else {
            cx.pt(HEADER_PT + HEADER_GAP_PT)
        };
        let rows_h: f32 = self
            .rows
            .iter()
            .map(|r| {
                if r.subtitle.is_some() {
                    cx.pt(ROW_PT)
                } else {
                    cx.pt(ROW_COMPACT_PT)
                }
            })
            .sum();
        Vec2::new(constraints.max_size.x.max(cx.pt(240.0)), header_h + rows_h)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        let heights: Vec<f32> = self
            .rows
            .iter()
            .map(|r| {
                if r.subtitle.is_some() {
                    cx.pt(ROW_PT)
                } else {
                    cx.pt(ROW_COMPACT_PT)
                }
            })
            .collect();
        let header_h = if self.header.is_empty() {
            0.0
        } else {
            cx.pt(HEADER_PT + HEADER_GAP_PT)
        };
        let mut y = bounds.origin.y + header_h;
        for (row, h) in self.rows.iter_mut().zip(heights) {
            let r = Rect::new(bounds.origin.x, y, bounds.size.x, h);
            cx.layout_child(row, r);
            y += h;
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Group);
        if !self.header.is_empty() {
            node.set_label(self.header.clone());
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        // Header.
        if !self.header.is_empty() {
            let size = cx.pt(HEADER_PT);
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                kurbo::Rect::new(
                    f64::from(cx.bounds.min_x() + cx.pt(PAD_PT)),
                    f64::from(cx.bounds.min_y()),
                    f64::from(cx.bounds.max_x()),
                    f64::from(cx.bounds.min_y() + size + cx.pt(HEADER_GAP_PT)),
                ),
                kurbo::Point::new(
                    f64::from(cx.bounds.min_x() + cx.pt(PAD_PT)),
                    f64::from(cx.bounds.min_y()),
                ),
                &self.header,
                size,
                cx.color(TokenKey::TextMutedColor, HEADER_INK),
            );
        }
        // Card face + separators between rows.
        let header_h = if self.header.is_empty() {
            0.0
        } else {
            cx.pt(HEADER_PT + HEADER_GAP_PT)
        };
        let card = kurbo::Rect::new(
            f64::from(cx.bounds.min_x()),
            f64::from(cx.bounds.min_y() + header_h),
            f64::from(cx.bounds.max_x()),
            f64::from(cx.bounds.max_y()),
        );
        if self.carded {
            cx.list.push_fill_shape(
                card,
                &martensite_core::shape::Shape::rounded(cx.pt(GROUP_RADIUS_PT)),
                cx.color(TokenKey::SurfaceColor, GROUP_FACE),
            );
        }
        // Hairline separators between rows.
        let mut y = card.y0;
        for row in self.rows.iter().take(self.rows.len().saturating_sub(1)) {
            y += f64::from(if row.subtitle.is_some() {
                cx.pt(ROW_PT)
            } else {
                cx.pt(ROW_COMPACT_PT)
            });
            cx.list.push_fill_rect(
                kurbo::Rect::new(
                    card.x0 + f64::from(cx.pt(SEP_INSET_PT)),
                    y,
                    card.x1,
                    y + 1.0,
                ),
                cx.color(TokenKey::DividerColor, SEP),
            );
        }
    }

    fn child_count(&self) -> usize {
        self.rows.len()
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        self.rows.get(index).map(|r| r as &dyn Widget)
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        self.rows.get_mut(index).map(|r| r as &mut dyn Widget)
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        // Rows cache their laid-out bounds — the authoritative rect.
        self.rows.get(index).map(|r| r.bounds)
    }
}

impl std::fmt::Debug for SettingsGroup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SettingsGroup")
            .field("header", &self.header)
            .field("rows", &self.rows.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::switch::Switch;
    use martensite_core::HotNode;

    fn make_cx(hot: &mut HotNode) -> LayoutContext<'_> {
        LayoutContext { hot, scale: 1.0 }
    }

    fn ev<'a>(event: &'a WidgetEvent) -> EventContext<'a> {
        EventContext {
            event,
            bounds: Rect::new(0.0, 0.0, 400.0, 52.0),
            scale: 1.0,
        }
    }

    fn press(x: f32, y: f32) -> WidgetEvent {
        WidgetEvent::PointerPressed {
            position: Vec2::new(x, y),
            button: PointerButton::Primary,
            count: 1,
        }
    }

    #[test]
    fn row_builder() {
        let r = SettingsRow::new("Wi-Fi").icon("📶").subtitle("On");
        assert_eq!(r.title(), "Wi-Fi");
        assert_eq!(Widget::child_count(&r), 0);
    }

    #[test]
    fn activatable_row_signals() {
        let mut r = SettingsRow::new("About").activatable(true);
        let mut hot = HotNode::default();
        r.layout(&mut make_cx(&mut hot), Rect::new(0.0, 0.0, 400.0, 52.0));
        assert_eq!(r.event(&mut ev(&press(20.0, 20.0))), EventResponse::Handled);
        assert!(r.take_activated());
        assert!(!r.take_activated());
    }

    #[test]
    fn passive_row_ignores() {
        let mut r = SettingsRow::new("Info");
        assert_eq!(r.event(&mut ev(&press(20.0, 20.0))), EventResponse::Ignored);
    }

    #[test]
    fn trailing_zone_belongs_to_child() {
        let mut r = SettingsRow::new("Dark")
            .activatable(true)
            .trailing(Switch::new(""));
        let mut hot = HotNode::default();
        r.layout(&mut make_cx(&mut hot), Rect::new(0.0, 0.0, 400.0, 52.0));
        // Press inside the trailing rect → Ignored (child handles).
        let tr = r.trailing_rect;
        let p = press(tr.origin.x + 2.0, tr.origin.y + 2.0);
        assert_eq!(r.event(&mut ev(&p)), EventResponse::Ignored);
        assert!(!r.take_activated());
        assert_eq!(Widget::child_count(&r), 1);
    }

    #[test]
    fn group_lays_rows() {
        let mut g = SettingsGroup::new("General")
            .row(SettingsRow::new("A").subtitle("sub"))
            .row(SettingsRow::new("B"));
        let mut hot = HotNode::default();
        g.layout(&mut make_cx(&mut hot), Rect::new(0.0, 0.0, 400.0, 200.0));
        assert_eq!(Widget::child_count(&g), 2);
        let r0 = g.child_bounds(0).unwrap();
        let r1 = g.child_bounds(1).unwrap();
        assert!(r1.origin.y > r0.origin.y);
    }

    #[test]
    fn group_row_access() {
        let mut g = SettingsGroup::new("G")
            .row(SettingsRow::new("A"))
            .row(SettingsRow::new("B").activatable(true));
        g.row_at_mut(1).unwrap().activated = true;
        assert!(g.row_at_mut(1).unwrap().take_activated());
    }

    #[test]
    fn keyboard_activation() {
        let mut r = SettingsRow::new("About").activatable(true);
        let key = WidgetEvent::KeyPressed {
            key: "Enter".into(),
            repeat: false,
        };
        assert_eq!(r.event(&mut ev(&key)), EventResponse::Handled);
        assert!(r.take_activated());
    }
}
