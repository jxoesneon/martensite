//! `CheckList` — a scrollable list of checkable rows (the installer /
//! software-picker / multi-choice idiom).
//!
//! Each row shows a checkbox and a label. Click toggles the row and
//! parks `(index, checked)` in [`CheckList::take_changed`]; arrows
//! move the focus and `Space` toggles it. The wheel scrolls when the
//! list overflows. [`CheckList::checked_indices`] returns every
//! checked row for the host.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::check_list::CheckList;
//!
//! let mut c = CheckList::new().items(["Alpha", "Beta"]);
//! c.set_checked(1, true);
//! assert_eq!(c.checked_indices(), &[1]);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;
use parking_lot::Mutex;

const ROW_PT: f32 = 26.0;
const PAD_PT: f32 = 6.0;
const BOX_PT: f32 = 16.0;

const FACE: [u8; 4] = [30, 30, 34, 255];
const ROW_HOT: [u8; 4] = [45, 46, 52, 255];
const EDGE: [u8; 4] = [80, 82, 90, 255];
const CHECK: [u8; 4] = [96, 165, 250, 255];
const TEXT: [u8; 4] = [215, 215, 222, 255];
const MARK: [u8; 4] = [20, 20, 24, 255];

/// One row — see [`CheckList`].
///
/// ```
/// use martensite::widgets::check_list::CheckItem;
///
/// let i = CheckItem::new("Option").checked(true);
/// assert!(i.checked);
/// ```
#[derive(Debug, Clone)]
pub struct CheckItem {
    /// Row label.
    pub label: String,
    /// Whether the row is checked.
    pub checked: bool,
}

impl CheckItem {
    /// An unchecked row.
    ///
    /// ```
    /// use martensite::widgets::check_list::CheckItem;
    ///
    /// assert!(!CheckItem::new("x").checked);
    /// ```
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            checked: false,
        }
    }

    /// Initial checked state.
    ///
    /// ```
    /// use martensite::widgets::check_list::CheckItem;
    ///
    /// assert!(CheckItem::new("x").checked(true).checked);
    /// ```
    pub fn checked(mut self, on: bool) -> Self {
        self.checked = on;
        self
    }
}

/// A checkable row list — see the module docs.
///
/// ```
/// use martensite::widgets::check_list::CheckList;
///
/// assert_eq!(CheckList::new().item_count(), 0);
/// ```
pub struct CheckList {
    /// Accessibility label.
    pub label: String,
    items: Vec<CheckItem>,
    focus: usize,
    changed: Option<(usize, bool)>,
    scroll: f32,
    bounds: Rect,
    scale: f32,
    /// Row rects painted last frame.
    hits: Mutex<Vec<(usize, Rect)>>,
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl std::fmt::Debug for CheckList {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CheckList")
            .field("items", &self.items.len())
            .field("checked", &self.checked_indices())
            .finish()
    }
}

impl Default for CheckList {
    fn default() -> Self {
        Self::new()
    }
}

impl CheckList {
    /// Empty list.
    ///
    /// ```
    /// use martensite::widgets::check_list::CheckList;
    ///
    /// assert_eq!(CheckList::new().item_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Checklist".to_string(),
            items: Vec::new(),
            focus: 0,
            changed: None,
            scroll: 0.0,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
            hits: Mutex::new(Vec::new()),
            text_painter: None,
        }
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::check_list::CheckList;
    ///
    /// assert_eq!(CheckList::new().label("Plugins").label, "Plugins");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Custom text painter.
    ///
    /// ```
    /// use martensite::widgets::check_list::CheckList;
    ///
    /// let _ = CheckList::new(); // painter optional
    /// ```
    pub fn with_text_painter(mut self, p: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(p);
        self
    }

    /// Bulk rows from labels.
    ///
    /// ```
    /// use martensite::widgets::check_list::CheckList;
    ///
    /// assert_eq!(CheckList::new().items(["a", "b", "c"]).item_count(), 3);
    /// ```
    pub fn items(mut self, labels: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.items = labels.into_iter().map(|l| CheckItem::new(l)).collect();
        self
    }

    /// Appends a row.
    ///
    /// ```
    /// use martensite::widgets::check_list::{CheckList, CheckItem};
    ///
    /// assert_eq!(CheckList::new().item(CheckItem::new("x")).item_count(), 1);
    /// ```
    pub fn item(mut self, item: CheckItem) -> Self {
        self.items.push(item);
        self
    }

    /// Row count.
    ///
    /// ```
    /// use martensite::widgets::check_list::CheckList;
    ///
    /// assert_eq!(CheckList::new().item_count(), 0);
    /// ```
    pub fn item_count(&self) -> usize {
        self.items.len()
    }

    /// Row `i`, if in range.
    ///
    /// ```
    /// use martensite::widgets::check_list::CheckList;
    ///
    /// let c = CheckList::new().items(["a"]);
    /// assert_eq!(c.item_at(0).unwrap().label, "a");
    /// ```
    pub fn item_at(&self, i: usize) -> Option<&CheckItem> {
        self.items.get(i)
    }

    /// Whether row `i` is checked.
    ///
    /// ```
    /// use martensite::widgets::check_list::CheckList;
    ///
    /// assert_eq!(CheckList::new().items(["a"]).is_checked(0), false);
    /// ```
    pub fn is_checked(&self, i: usize) -> bool {
        self.items.get(i).is_some_and(|it| it.checked)
    }

    /// Sets row `i`'s checked state.
    ///
    /// ```
    /// use martensite::widgets::check_list::CheckList;
    ///
    /// let mut c = CheckList::new().items(["a", "b"]);
    /// c.set_checked(0, true);
    /// assert!(c.is_checked(0));
    /// ```
    pub fn set_checked(&mut self, i: usize, on: bool) {
        if let Some(it) = self.items.get_mut(i) {
            it.checked = on;
        }
    }

    /// Toggles row `i` and parks the change.
    ///
    /// ```
    /// use martensite::widgets::check_list::CheckList;
    ///
    /// let mut c = CheckList::new().items(["a"]);
    /// c.toggle(0);
    /// assert!(c.is_checked(0));
    /// assert_eq!(c.take_changed(), Some((0, true)));
    /// ```
    pub fn toggle(&mut self, i: usize) {
        if let Some(it) = self.items.get_mut(i) {
            it.checked = !it.checked;
            self.changed = Some((i, it.checked));
        }
    }

    /// Checks every row.
    ///
    /// ```
    /// use martensite::widgets::check_list::CheckList;
    ///
    /// let mut c = CheckList::new().items(["a", "b"]);
    /// c.check_all();
    /// assert_eq!(c.checked_indices().len(), 2);
    /// ```
    pub fn check_all(&mut self) {
        for it in &mut self.items {
            it.checked = true;
        }
    }

    /// Unchecks every row.
    ///
    /// ```
    /// use martensite::widgets::check_list::CheckList;
    ///
    /// let mut c = CheckList::new().items(["a"]);
    /// c.check_all();
    /// c.clear_all();
    /// assert!(c.checked_indices().is_empty());
    /// ```
    pub fn clear_all(&mut self) {
        for it in &mut self.items {
            it.checked = false;
        }
    }

    /// Indices of all checked rows, ascending.
    ///
    /// ```
    /// use martensite::widgets::check_list::CheckList;
    ///
    /// assert!(CheckList::new().checked_indices().is_empty());
    /// ```
    pub fn checked_indices(&self) -> Vec<usize> {
        self.items
            .iter()
            .enumerate()
            .filter_map(|(i, it)| it.checked.then_some(i))
            .collect()
    }

    /// Focused row index.
    ///
    /// ```
    /// use martensite::widgets::check_list::CheckList;
    ///
    /// assert_eq!(CheckList::new().focused(), 0);
    /// ```
    pub fn focused(&self) -> usize {
        self.focus
    }

    /// Drains the last `(index, checked)` toggle.
    ///
    /// ```
    /// use martensite::widgets::check_list::CheckList;
    ///
    /// assert_eq!(CheckList::new().take_changed(), None);
    /// ```
    pub fn take_changed(&mut self) -> Option<(usize, bool)> {
        self.changed.take()
    }

    /// Content height.
    fn content_h(&self) -> f32 {
        self.items.len() as f32 * ROW_PT * self.scale + PAD_PT * self.scale
    }

    /// Max scroll offset.
    fn max_scroll(&self) -> f32 {
        (self.content_h() - self.bounds.height()).max(0.0)
    }
}

impl Widget for CheckList {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let h = self.items.len().clamp(3, 8) as f32 * ROW_PT + PAD_PT;
        Vec2::new(
            cx.pt(240.0).min(constraints.max_size.x.max(0.0)),
            cx.pt(h).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(100.0, ROW_PT * 2.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        self.scroll = self.scroll.clamp(0.0, self.max_scroll());
        if !self.items.is_empty() {
            self.focus = self.focus.min(self.items.len() - 1);
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::List);
        node.set_label(format!(
            "{} — {} of {} checked",
            self.label,
            self.checked_indices().len(),
            self.items.len()
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
                let hits = self.hits.lock();
                if let Some((i, _)) = hits.iter().find(|(_, r)| r.contains(*position)) {
                    let i = *i;
                    drop(hits);
                    self.focus = i;
                    self.toggle(i);
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Handled
            }
            WidgetEvent::KeyPressed { key, .. } => match key.as_str() {
                "ArrowUp" | "ArrowDown" if !self.items.is_empty() => {
                    self.focus = if key == "ArrowDown" {
                        (self.focus + 1).min(self.items.len() - 1)
                    } else {
                        self.focus.saturating_sub(1)
                    };
                    EventResponse::RequestRepaint
                }
                " " | "Enter" if !self.items.is_empty() => {
                    self.toggle(self.focus);
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
        let s = self.scale;
        cx.list.push_fill_shape(
            krect(self.bounds),
            &martensite_core::shape::Shape::rounded(4.0 * s),
            cx.color(TokenKey::SurfaceColor, FACE),
        );
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let row_h = ROW_PT * s;
        let pad = PAD_PT * s;
        let box_sz = BOX_PT * s;
        let size = 12.0 * s;
        let mut hits = self.hits.lock();
        hits.clear();
        cx.list.push_clip(krect(self.bounds));
        for (i, it) in self.items.iter().enumerate() {
            let y = self.bounds.min_y() + pad / 2.0 + i as f32 * row_h - self.scroll;
            if y + row_h < self.bounds.min_y() || y > self.bounds.max_y() {
                continue;
            }
            let r = Rect::new(self.bounds.min_x(), y, self.bounds.width(), row_h);
            hits.push((i, r));
            let kr = krect(r);
            if i == self.focus {
                cx.list.push_fill_shape(
                    kr,
                    &martensite_core::shape::Shape::rounded(4.0 * s),
                    cx.color(TokenKey::SurfaceColor, ROW_HOT),
                );
            }
            // Checkbox.
            let bx = Rect::new(
                self.bounds.min_x() + pad,
                y + (row_h - box_sz) / 2.0,
                box_sz,
                box_sz,
            );
            let kbx = krect(bx);
            cx.list.push_stroke_shape(
                kbx,
                &martensite_core::shape::Shape::rounded(3.0 * s),
                s,
                cx.color(TokenKey::BorderColor, EDGE),
            );
            if it.checked {
                cx.list.push_fill_shape(
                    kbx,
                    &martensite_core::shape::Shape::rounded(3.0 * s),
                    cx.color(TokenKey::AccentColor, CHECK),
                );
                // Check mark.
                let mut mark = kurbo::BezPath::new();
                mark.move_to((
                    f64::from(bx.min_x() + box_sz * 0.22),
                    f64::from(bx.min_y() + box_sz * 0.52),
                ));
                mark.line_to((
                    f64::from(bx.min_x() + box_sz * 0.44),
                    f64::from(bx.min_y() + box_sz * 0.72),
                ));
                mark.line_to((
                    f64::from(bx.min_x() + box_sz * 0.78),
                    f64::from(bx.min_y() + box_sz * 0.3),
                ));
                cx.list
                    .push_stroke_path(mark, 1.6 * s, cx.color(TokenKey::TextInverseColor, MARK));
            }
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                kr,
                kurbo::Point::new(
                    f64::from(bx.max_x() + 8.0 * s),
                    f64::from(y + (row_h - size) / 2.0),
                ),
                &it.label,
                size,
                cx.color(TokenKey::TextColor, TEXT),
            );
        }
        cx.list.pop_clip();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::{HotNode, PaintList};

    fn laid_out(c: &mut CheckList, w: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        c.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(w, h),
            },
        );
        c.layout(&mut cx, Rect::new(0.0, 0.0, w, h));
    }

    fn painted(c: &CheckList) {
        let theme = martensite_theme::Theme::new("test");
        let mut list = PaintList::new();
        let mut cx = PaintContext {
            list: &mut list,
            bounds: c.bounds,
            scale: 1.0,
            theme: &theme,
            text_painter: None,
        };
        c.paint(&mut cx);
    }

    fn ev(c: &mut CheckList, e: &WidgetEvent) {
        c.event(&mut EventContext {
            event: e,
            bounds: c.bounds,
            scale: 1.0,
        });
    }

    #[test]
    fn click_toggles() {
        let mut c = CheckList::new().items(["a", "b", "c"]);
        laid_out(&mut c, 240.0, 120.0);
        painted(&c);
        let r = c.hits.lock()[1].1;
        ev(
            &mut c,
            &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new((r.min_x() + r.max_x()) / 2.0, (r.min_y() + r.max_y()) / 2.0),
                count: 1,
            },
        );
        assert!(c.is_checked(1));
        assert_eq!(c.take_changed(), Some((1, true)));
        assert_eq!(c.checked_indices(), &[1]);
    }

    #[test]
    fn space_toggles_focus() {
        let mut c = CheckList::new().items(["a", "b"]);
        laid_out(&mut c, 240.0, 120.0);
        ev(
            &mut c,
            &WidgetEvent::KeyPressed {
                key: "ArrowDown".to_string(),
                repeat: false,
            },
        );
        assert_eq!(c.focused(), 1);
        ev(
            &mut c,
            &WidgetEvent::KeyPressed {
                key: " ".to_string(),
                repeat: false,
            },
        );
        assert!(c.is_checked(1));
        assert_eq!(c.take_changed(), Some((1, true)));
    }

    #[test]
    fn check_and_clear_all() {
        let mut c = CheckList::new().items(["a", "b", "c"]);
        c.check_all();
        assert_eq!(c.checked_indices(), &[0, 1, 2]);
        c.clear_all();
        assert!(c.checked_indices().is_empty());
    }

    #[test]
    fn scroll_clamps() {
        let mut c = CheckList::new().items((0..30).map(|i| format!("row {i}")));
        laid_out(&mut c, 240.0, 100.0);
        ev(
            &mut c,
            &WidgetEvent::Scroll {
                position: Vec2::new(120.0, 50.0),
                delta: Vec2::new(0.0, -9999.0),
            },
        );
        assert_eq!(c.scroll, c.max_scroll());
    }
}
