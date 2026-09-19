//! Transfer — dual-pane shuttle list: source items on the left,
//! chosen items on the right, with → / ← move buttons between them
//! (Ant `Transfer`, Django admin filter_horizontal).
//!
//! Rows click-select (single-selection per pane for v1); the center
//! buttons move the selected rows across. Moves surface through
//! [`Transfer::take_moved`] as `(Vec<String> moved_labels,
//! MoveDir)` so the consumer can persist the shuttle.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::Transfer;
//!
//! let t = Transfer::new().source(["a", "b"]).target(["c"]);
//! assert_eq!(t.source_count(), 2);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::text_paint::paint_label_clipped;
use crate::widgets::Button;

/// Row height in points.
const ROW_PT: f32 = 26.0;
/// Pane title strip in points.
const TITLE_PT: f32 = 22.0;
/// Shuttle button metrics (points).
const BTN_W_PT: f32 = 36.0;
const BTN_H_PT: f32 = 24.0;
const BTN_GAP_PT: f32 = 8.0;
const PAD_PT: f32 = 6.0;

/// Shuttle direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveDir {
    /// Source → target (the `→` button).
    Right,
    /// Target → source (the `←` button).
    Left,
}

/// Dual-pane shuttle widget.
///
/// # Examples
///
/// ```
/// use martensite::widgets::Transfer;
///
/// assert_eq!(Transfer::new().source_count(), 0);
/// ```
pub struct Transfer {
    /// Accessibility label override.
    pub label: Option<String>,
    /// Whether input reaches the panes.
    pub enabled: bool,
    /// Pane titles.
    pub source_title: String,
    /// Right pane title.
    pub target_title: String,
    source: Vec<String>,
    target: Vec<String>,
    /// Selected row index per pane.
    source_sel: Option<usize>,
    /// Right-pane selection.
    target_sel: Option<usize>,
    to_right: Button,
    to_left: Button,
    /// Parked `(labels, dir)` after a move.
    moved: Option<(Vec<String>, MoveDir)>,
    bounds: Rect,
    source_bounds: Option<Rect>,
    target_bounds: Option<Rect>,
    right_btn_bounds: Option<Rect>,
    left_btn_bounds: Option<Rect>,
    hover: Option<(bool, usize)>, // (is_target, row)
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl Transfer {
    /// Creates an empty shuttle.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Transfer;
    ///
    /// assert_eq!(Transfer::new().target_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: None,
            enabled: true,
            source_title: "Source".to_string(),
            target_title: "Target".to_string(),
            source: Vec::new(),
            target: Vec::new(),
            source_sel: None,
            target_sel: None,
            to_right: Button::new("→"),
            to_left: Button::new("←"),
            moved: None,
            bounds: Rect::default(),
            source_bounds: None,
            target_bounds: None,
            right_btn_bounds: None,
            left_btn_bounds: None,
            hover: None,
            text_painter: None,
        }
    }

    /// Sets the left-pane items.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Transfer;
    ///
    /// assert_eq!(Transfer::new().source(["a"]).source_count(), 1);
    /// ```
    #[must_use]
    pub fn source(mut self, items: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.source = items.into_iter().map(Into::into).collect();
        self
    }

    /// Sets the right-pane items.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Transfer;
    ///
    /// assert_eq!(Transfer::new().target(["a"]).target_count(), 1);
    /// ```
    #[must_use]
    pub fn target(mut self, items: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.target = items.into_iter().map(Into::into).collect();
        self
    }

    /// Sets both pane titles.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Transfer;
    ///
    /// let t = Transfer::new().titles("Avail", "Chosen");
    /// assert_eq!(t.source_title, "Avail");
    /// ```
    #[must_use]
    pub fn titles(mut self, source: impl Into<String>, target: impl Into<String>) -> Self {
        self.source_title = source.into();
        self.target_title = target.into();
        self
    }

    /// Sets the accessibility label.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Transfer;
    ///
    /// let t = Transfer::new().label("Permissions");
    /// assert_eq!(t.label.as_deref(), Some("Permissions"));
    /// ```
    #[must_use]
    pub fn label(mut self, text: impl Into<String>) -> Self {
        self.label = Some(text.into());
        self
    }

    /// Sets whether input reaches the panes.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Transfer;
    ///
    /// assert!(!Transfer::new().enabled(false).enabled);
    /// ```
    #[must_use]
    pub fn enabled(mut self, flag: bool) -> Self {
        self.enabled = flag;
        self.sync_buttons();
        self
    }

    /// Shares a [`crate::text_paint::TextPainter`] for row text.
    #[must_use]
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Left-pane item count.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Transfer;
    ///
    /// assert_eq!(Transfer::new().source_count(), 0);
    /// ```
    #[inline]
    pub fn source_count(&self) -> usize {
        self.source.len()
    }

    /// Right-pane item count.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Transfer;
    ///
    /// assert_eq!(Transfer::new().target_count(), 0);
    /// ```
    #[inline]
    pub fn target_count(&self) -> usize {
        self.target.len()
    }

    /// Source labels (in pane order).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Transfer;
    ///
    /// assert!(Transfer::new().source_items().is_empty());
    /// ```
    #[inline]
    pub fn source_items(&self) -> &[String] {
        &self.source
    }

    /// Target labels (in pane order).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Transfer;
    ///
    /// assert!(Transfer::new().target_items().is_empty());
    /// ```
    #[inline]
    pub fn target_items(&self) -> &[String] {
        &self.target
    }

    /// Moves the selected source row right programmatically.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Transfer;
    ///
    /// let mut t = Transfer::new().source(["a"]);
    /// t.move_selected_right();
    /// assert_eq!(t.source_count(), 1); // nothing selected → no-op
    /// ```
    pub fn move_selected_right(&mut self) {
        if let Some(i) = self.source_sel.take() {
            if i < self.source.len() {
                let item = self.source.remove(i);
                self.moved = Some((vec![item.clone()], MoveDir::Right));
                self.target.push(item);
            }
        }
        self.sync_buttons();
    }

    /// Moves the selected target row left programmatically.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Transfer;
    ///
    /// let mut t = Transfer::new().target(["a"]);
    /// t.move_selected_left();
    /// assert_eq!(t.target_count(), 1); // nothing selected → no-op
    /// ```
    pub fn move_selected_left(&mut self) {
        if let Some(i) = self.target_sel.take() {
            if i < self.target.len() {
                let item = self.target.remove(i);
                self.moved = Some((vec![item.clone()], MoveDir::Left));
                self.source.push(item);
            }
        }
        self.sync_buttons();
    }

    /// Takes the parked `(labels, dir)` move.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Transfer;
    ///
    /// assert!(Transfer::new().take_moved().is_none());
    /// ```
    #[inline]
    pub fn take_moved(&mut self) -> Option<(Vec<String>, MoveDir)> {
        self.moved.take()
    }

    fn sync_buttons(&mut self) {
        self.to_right.enabled = self.enabled && self.source_sel.is_some();
        self.to_left.enabled = self.enabled && self.target_sel.is_some();
    }

    /// Row index under `position` within `pane` rect.
    fn row_at(&self, pane: Rect, position: Vec2, scale: f32) -> Option<usize> {
        if !pane.contains(position) {
            return None;
        }
        let title = TITLE_PT * scale;
        let y = position.y - pane.min_y() - title;
        if y < 0.0 {
            return None;
        }
        Some((y / (ROW_PT * scale)) as usize)
    }

    /// Pane + row for `position`.
    fn hit(&self, position: Vec2, scale: f32) -> Option<(bool, usize)> {
        if let Some(sb) = self.source_bounds {
            if let Some(r) = self.row_at(sb, position, scale) {
                if r < self.source.len() {
                    return Some((false, r));
                }
            }
        }
        if let Some(tb) = self.target_bounds {
            if let Some(r) = self.row_at(tb, position, scale) {
                if r < self.target.len() {
                    return Some((true, r));
                }
            }
        }
        None
    }

    /// Drains shuttle-button activations.
    fn poll_buttons(&mut self) {
        if self.to_right.take_activated() {
            self.move_selected_right();
        }
        if self.to_left.take_activated() {
            self.move_selected_left();
        }
    }
}

impl Default for Transfer {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for Transfer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Transfer")
            .field("source", &self.source.len())
            .field("target", &self.target.len())
            .finish()
    }
}

impl Widget for Transfer {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            constraints
                .max_size
                .x
                .max(cx.pt(300.0).min(constraints.max_size.x)),
            (cx.pt(ROW_PT) * 7.0 + cx.pt(TITLE_PT)).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(240.0, 120.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        // Center shuttle column.
        let btn_w = cx.pt(BTN_W_PT);
        let btn_h = cx.pt(BTN_H_PT);
        let gap = cx.pt(BTN_GAP_PT);
        let pane_w = (bounds.width() - btn_w - 2.0 * gap).max(0.0) / 2.0;
        let left = Rect::new(bounds.min_x(), bounds.min_y(), pane_w, bounds.height());
        let right = Rect::new(
            bounds.min_x() + pane_w + btn_w + 2.0 * gap,
            bounds.min_y(),
            pane_w,
            bounds.height(),
        );
        self.source_bounds = Some(left);
        self.target_bounds = Some(right);
        // Buttons centered vertically in the middle column.
        let cx_mid = bounds.min_x() + pane_w + gap;
        let by = bounds.min_y() + bounds.height() / 2.0 - btn_h - gap / 2.0;
        let right_r = Rect::new(cx_mid, by, btn_w, btn_h);
        let left_r = Rect::new(cx_mid, by + btn_h + gap, btn_w, btn_h);
        self.right_btn_bounds = Some(right_r);
        self.left_btn_bounds = Some(left_r);
        cx.layout_child(&mut self.to_right, right_r);
        cx.layout_child(&mut self.to_left, left_r);
        self.sync_buttons();
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
        let surface = cx.color(TokenKey::SurfaceColor, [45, 45, 48, 255]);
        let border = cx.color(TokenKey::BorderColor, [90, 90, 90, 255]);
        let fg = cx.color(TokenKey::TextColor, [220, 220, 220, 255]);
        let muted = cx.color(TokenKey::TextMutedColor, [140, 140, 140, 255]);
        let accent = cx.color(TokenKey::AccentColor, [0, 122, 204, 255]);
        let sel_wash = [accent[0], accent[1], accent[2], 56];
        let hover_wash = [accent[0], accent[1], accent[2], 28];
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let row_h = cx.pt(ROW_PT);
        let title_h = cx.pt(TITLE_PT);
        let pad = cx.pt(PAD_PT);

        for (is_target, pane, items, title, sel) in [
            (
                false,
                self.source_bounds,
                &self.source,
                &self.source_title,
                self.source_sel,
            ),
            (
                true,
                self.target_bounds,
                &self.target,
                &self.target_title,
                self.target_sel,
            ),
        ] {
            let Some(pane) = pane else { continue };
            cx.list.push_fill_rect(f(pane), surface);
            cx.list
                .push_stroke_rect(f(pane), 1.0_f32.max(cx.pt(0.5)), border);
            let clip = f(pane);
            // Title strip.
            paint_label_clipped(
                painter,
                cx.list,
                clip,
                kurbo::Point::new(
                    f64::from(pane.min_x() + pad),
                    f64::from(pane.min_y() + (title_h - 10.0 * cx.scale) / 2.0),
                ),
                &format!("{title} ({})", items.len()),
                10.0 * cx.scale,
                muted,
            );
            cx.list.push_fill_rect(
                f(Rect::new(
                    pane.min_x(),
                    pane.min_y() + title_h,
                    pane.width(),
                    1.0,
                )),
                border,
            );
            for (i, item) in items.iter().enumerate() {
                let y = pane.min_y() + title_h + i as f32 * row_h;
                if y + row_h > pane.max_y() {
                    break;
                }
                let row_r = Rect::new(pane.min_x(), y, pane.width(), row_h);
                if sel == Some(i) {
                    cx.list.push_fill_rect(f(row_r), sel_wash);
                } else if self.hover == Some((is_target, i)) {
                    cx.list.push_fill_rect(f(row_r), hover_wash);
                }
                paint_label_clipped(
                    painter,
                    cx.list,
                    clip,
                    kurbo::Point::new(
                        f64::from(pane.min_x() + pad),
                        f64::from(y + (row_h - 12.0 * cx.scale) / 2.0),
                    ),
                    item,
                    12.0 * cx.scale,
                    if self.enabled { fg } else { muted },
                );
            }
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.enabled {
            return EventResponse::Ignored;
        }
        self.poll_buttons();
        match cx.event {
            WidgetEvent::PointerMoved { position } => {
                let hit = self.hit(*position, cx.scale);
                if hit != self.hover {
                    self.hover = hit;
                    return EventResponse::RequestRepaint;
                }
            }
            WidgetEvent::PointerReleased { position, button } => {
                if *button != martensite_core::PointerButton::Primary {
                    return EventResponse::Ignored;
                }
                if let Some((is_target, row)) = self.hit(*position, cx.scale) {
                    if is_target {
                        self.target_sel = (self.target_sel != Some(row)).then_some(row);
                    } else {
                        self.source_sel = (self.source_sel != Some(row)).then_some(row);
                    }
                    self.sync_buttons();
                    return EventResponse::Handled;
                }
            }
            _ => {}
        }
        // Buttons are children — forward bounds-gated.
        for i in (0..self.child_count()).rev() {
            let Some(b) = self.child_bounds(i) else {
                continue;
            };
            let Some(pos) = cx.event.position() else {
                break;
            };
            if !b.contains(pos) {
                continue;
            }
            let mut child_cx = EventContext {
                event: cx.event,
                bounds: b,
                scale: cx.scale,
            };
            if let Some(child) = self.child_mut(i) {
                let r = child.event(&mut child_cx);
                if r != EventResponse::Ignored {
                    self.poll_buttons();
                    return r;
                }
            }
        }
        EventResponse::Ignored
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Group);
        if let Some(ref label) = self.label {
            node.set_label(label.as_str());
        }
        if !self.enabled {
            node.set_disabled();
        }
    }

    fn child_count(&self) -> usize {
        2
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        match index {
            0 => Some(&self.to_right),
            1 => Some(&self.to_left),
            _ => None,
        }
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        match index {
            0 => Some(&mut self.to_right),
            1 => Some(&mut self.to_left),
            _ => None,
        }
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        match index {
            0 => self.right_btn_bounds,
            1 => self.left_btn_bounds,
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::{HotNode, PointerButton, WidgetEvent};

    fn shuttle() -> Transfer {
        Transfer::new().source(["a", "b", "c"]).target(["x"])
    }

    fn laid_out(t: &mut Transfer) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        t.layout(&mut cx, Rect::new(0.0, 0.0, 400.0, 220.0));
    }

    fn ev(t: &mut Transfer, event: &WidgetEvent) -> EventResponse {
        let mut cx = EventContext {
            event,
            bounds: Rect::new(0.0, 0.0, 400.0, 220.0),
            scale: 1.0,
        };
        t.event(&mut cx)
    }

    fn click(t: &mut Transfer, x: f32, y: f32) {
        let up = WidgetEvent::PointerReleased {
            position: Vec2::new(x, y),
            button: PointerButton::Primary,
        };
        ev(t, &up);
    }

    #[test]
    fn builder() {
        let mut t = shuttle();
        assert_eq!(t.source_count(), 3);
        assert_eq!(t.target_count(), 1);
        assert!(t.take_moved().is_none());
    }

    #[test]
    fn click_selects_source_row() {
        let mut t = shuttle();
        laid_out(&mut t);
        // Source pane row 1: title 22 + row 1 → y = 22+13+26 = ~48
        click(&mut t, 20.0, 22.0 + 26.0 + 13.0);
        assert_eq!(t.source_sel, Some(1));
    }

    #[test]
    fn shuttle_button_moves_selected() {
        let mut t = shuttle();
        laid_out(&mut t);
        click(&mut t, 20.0, 22.0 + 13.0); // select row 0 ("a")
        assert_eq!(t.source_sel, Some(0));
        // → button lives mid-column; find via child_bounds(0).
        let b = t.child_bounds(0).unwrap();
        let mid = Vec2::new(b.min_x() + b.width() / 2.0, b.min_y() + b.height() / 2.0);
        let down = WidgetEvent::PointerPressed {
            position: mid,
            button: PointerButton::Primary,
            count: 1,
        };
        let up = WidgetEvent::PointerReleased {
            position: mid,
            button: PointerButton::Primary,
        };
        ev(&mut t, &down);
        ev(&mut t, &up);
        assert_eq!(t.source_items(), &["b", "c"]);
        assert_eq!(t.target_items(), &["x", "a"]);
        assert_eq!(t.take_moved().unwrap().1, MoveDir::Right);
    }

    #[test]
    fn move_left_via_api() {
        let mut t = shuttle();
        t.target_sel = Some(0);
        t.move_selected_left();
        assert_eq!(t.target_count(), 0);
        assert_eq!(t.source_items(), &["a", "b", "c", "x"]);
        assert_eq!(t.take_moved().unwrap().1, MoveDir::Left);
    }

    #[test]
    fn toggle_reselects() {
        let mut t = shuttle();
        laid_out(&mut t);
        click(&mut t, 20.0, 35.0);
        assert!(t.source_sel.is_some());
        click(&mut t, 20.0, 35.0);
        assert!(t.source_sel.is_none());
    }

    #[test]
    fn disabled_inert() {
        let mut t = shuttle().enabled(false);
        laid_out(&mut t);
        click(&mut t, 20.0, 35.0);
        assert!(t.source_sel.is_none());
        assert!(!t.to_right.enabled);
    }
}
