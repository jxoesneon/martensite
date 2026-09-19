//! Cascading column picker — N columns of option lists; selecting a
//! parent populates the next column and a leaf click commits the
//! full path (macOS `NSBrowser`, Ant `Cascader` panel).
//!
//! The columns are always visible inside the widget's bounds (the
//! `NSBrowser`/Miller-columns convention rather than a popup), so
//! the widget is self-contained: click column *i* to drill down —
//! columns to the right rebuild from the chosen node's children.
//!
//! Selection surfaces as a *path* through [`Cascader::take_selected`]
//! — `["fruit", "citrus", "lemon"]` — matching Ant's value model.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::{Cascader, CascaderOption};
//!
//! let c = Cascader::new().options([CascaderOption::new("a", "a")]);
//! assert_eq!(c.option_count(), 1);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::text_paint::paint_label_clipped;

/// Row height (points).
const ROW_PT: f32 = 26.0;
/// Column hairline width (points).
const HAIR_PT: f32 = 1.0;
/// Chevron gutter on expandable rows (points).
const CHEV_PT: f32 = 14.0;
/// Cell padding (points).
const PAD_PT: f32 = 8.0;
/// Max columns shown at once (deeper paths scroll horizontally —
/// out of scope; the deepest columns replace the shallow ones).
const MAX_COLS: usize = 4;

/// One node in the option tree.
///
/// # Examples
///
/// ```
/// use martensite::widgets::CascaderOption;
///
/// let o = CascaderOption::new("Fruit", "fruit")
///     .child(CascaderOption::new("Citrus", "citrus"));
/// assert_eq!(o.children.len(), 1);
/// ```
#[derive(Debug, Clone)]
pub struct CascaderOption {
    /// Display text.
    pub label: String,
    /// Value carried in the emitted path.
    pub value: String,
    /// Child options — non-empty makes this row expandable.
    pub children: Vec<CascaderOption>,
    /// Whether the row may be selected.
    pub enabled: bool,
}

impl CascaderOption {
    /// Creates a leaf option.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::CascaderOption;
    ///
    /// let o = CascaderOption::new("Yes", "yes");
    /// assert!(o.children.is_empty());
    /// ```
    pub fn new(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
            children: Vec::new(),
            enabled: true,
        }
    }

    /// Appends a child option.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::CascaderOption;
    ///
    /// assert_eq!(CascaderOption::new("p", "p").child(CascaderOption::new("c", "c")).children.len(), 1);
    /// ```
    #[must_use]
    pub fn child(mut self, option: CascaderOption) -> Self {
        self.children.push(option);
        self
    }

    /// Marks the row disabled.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::CascaderOption;
    ///
    /// assert!(!CascaderOption::new("x", "x").enabled(false).enabled);
    /// ```
    #[must_use]
    pub fn enabled(mut self, flag: bool) -> Self {
        self.enabled = flag;
        self
    }

    /// Whether this row expands into the next column.
    fn expandable(&self) -> bool {
        !self.children.is_empty()
    }
}

/// Cascading column picker.
///
/// # Examples
///
/// ```
/// use martensite::widgets::{Cascader, CascaderOption};
///
/// let c = Cascader::new();
/// assert!(c.selected_path().is_empty());
/// ```
pub struct Cascader {
    /// Accessibility label override.
    pub label: Option<String>,
    /// Whether input reaches the columns.
    pub enabled: bool,
    /// Placeholder in the value strip when nothing is committed.
    pub placeholder: String,
    root: Vec<CascaderOption>,
    /// Chosen option index per column — `path[0]` is the root pick.
    path: Vec<usize>,
    /// Committed value path (labels) after a leaf click.
    committed: Vec<String>,
    /// Parked selection for the consumer.
    selected: Option<Vec<String>>,
    bounds: Rect,
    /// Highlighted row per column (hover feedback).
    hover: Option<(usize, usize)>,
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl Cascader {
    /// Creates an empty picker.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Cascader;
    ///
    /// assert_eq!(Cascader::new().option_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: None,
            enabled: true,
            placeholder: "Select…".to_string(),
            root: Vec::new(),
            path: Vec::new(),
            committed: Vec::new(),
            selected: None,
            bounds: Rect::default(),
            hover: None,
            text_painter: None,
        }
    }

    /// Sets the root options.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Cascader, CascaderOption};
    ///
    /// assert_eq!(Cascader::new().options([CascaderOption::new("a", "a")]).option_count(), 1);
    /// ```
    #[must_use]
    pub fn options(mut self, options: impl Into<Vec<CascaderOption>>) -> Self {
        self.root = options.into();
        self.path.clear();
        self.committed.clear();
        self
    }

    /// Sets the accessibility label.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Cascader;
    ///
    /// let c = Cascader::new().label("Region");
    /// assert_eq!(c.label.as_deref(), Some("Region"));
    /// ```
    #[must_use]
    pub fn label(mut self, text: impl Into<String>) -> Self {
        self.label = Some(text.into());
        self
    }

    /// Sets the placeholder text.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Cascader;
    ///
    /// assert_eq!(Cascader::new().placeholder("Pick").placeholder, "Pick");
    /// ```
    #[must_use]
    pub fn placeholder(mut self, text: impl Into<String>) -> Self {
        self.placeholder = text.into();
        self
    }

    /// Sets whether input reaches the columns.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Cascader;
    ///
    /// assert!(!Cascader::new().enabled(false).enabled);
    /// ```
    #[must_use]
    pub fn enabled(mut self, flag: bool) -> Self {
        self.enabled = flag;
        self
    }

    /// Shares a [`crate::text_paint::TextPainter`] for row text.
    #[must_use]
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Root option count.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Cascader, CascaderOption};
    ///
    /// assert_eq!(Cascader::new().option_count(), 0);
    /// ```
    #[inline]
    pub fn option_count(&self) -> usize {
        self.root.len()
    }

    /// Currently chosen index per column (the drill path).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Cascader;
    ///
    /// assert!(Cascader::new().path().is_empty());
    /// ```
    #[inline]
    pub fn path(&self) -> &[usize] {
        &self.path
    }

    /// The committed value path — empty until a leaf is chosen.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Cascader;
    ///
    /// assert!(Cascader::new().selected_path().is_empty());
    /// ```
    #[inline]
    pub fn selected_path(&self) -> &[String] {
        &self.committed
    }

    /// Takes the parked committed path (`Some` once per leaf click).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Cascader;
    ///
    /// assert!(Cascader::new().take_selected().is_none());
    /// ```
    #[inline]
    pub fn take_selected(&mut self) -> Option<Vec<String>> {
        self.selected.take()
    }

    /// Clears path and commit.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Cascader;
    ///
    /// let mut c = Cascader::new();
    /// c.clear();
    /// assert!(c.path().is_empty());
    /// ```
    pub fn clear(&mut self) {
        self.path.clear();
        self.committed.clear();
        self.selected = None;
    }

    /// Options feeding column `depth` by walking `path`.
    fn column_options(&self, depth: usize) -> &[CascaderOption] {
        if depth == 0 {
            return &self.root;
        }
        let mut level = &self.root;
        for d in 0..depth {
            match self.path.get(d).and_then(|&i| level.get(i)) {
                Some(node) => level = &node.children,
                None => return &[],
            }
        }
        level
    }

    /// Visible column count — path depth + one more while deeper
    /// levels exist.
    fn column_count(&self) -> usize {
        let mut n = 0usize;
        while n < MAX_COLS && !self.column_options(n).is_empty() {
            n += 1;
        }
        n.max(1)
    }

    /// Column index + row for `position`.
    fn hit(&self, position: Vec2, scale: f32) -> Option<(usize, usize)> {
        let cols = self.column_count();
        if !self.bounds.contains(position) || cols == 0 {
            return None;
        }
        // Recomputed live — the column count grows as the user
        // drills, so a layout-cached width would be stale.
        let col_w = self.bounds.width() / cols as f32;
        let col = ((position.x - self.bounds.min_x()) / col_w) as usize;
        let row = ((position.y - self.bounds.min_y()) / (ROW_PT * scale)) as usize;
        (col < cols && row < self.column_options(col).len()).then_some((col, row))
    }

    /// Applies a `(col, row)` click.
    fn click(&mut self, col: usize, row: usize) {
        self.path.truncate(col);
        self.path.push(row);
        let node = &self.column_options(col)[row];
        if node.expandable() {
            self.committed.clear();
        } else {
            // Leaf — commit the value path.
            let mut values = Vec::with_capacity(col + 1);
            let mut level = &self.root;
            for (d, &i) in self.path.iter().enumerate() {
                if let Some(n) = level.get(i) {
                    values.push(n.value.clone());
                    if d + 1 < self.path.len() {
                        level = &n.children;
                    }
                }
            }
            self.committed = values.clone();
            self.selected = Some(values);
        }
    }
}

impl Default for Cascader {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for Cascader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Cascader")
            .field("options", &self.root.len())
            .field("path", &self.path)
            .finish()
    }
}

impl Widget for Cascader {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let h = (cx.pt(ROW_PT) * 8.0).min(constraints.max_size.y.max(0.0));
        Vec2::new(
            constraints
                .max_size
                .x
                .max(cx.pt(240.0).min(constraints.max_size.x)),
            h,
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(200.0, 100.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, _cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
    }

    fn paint(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
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
        let hover_wash = [accent[0], accent[1], accent[2], 32];
        let sel_wash = [accent[0], accent[1], accent[2], 56];

        cx.list.push_fill_rect(f(b), surface);
        cx.list
            .push_stroke_rect(f(b), 1.0_f32.max(cx.pt(0.5)), border);

        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let row_h = cx.pt(ROW_PT);
        let pad = cx.pt(PAD_PT);
        let cols = self.column_count();
        let col_w = if cols > 0 {
            b.width() / cols as f32
        } else {
            b.width()
        };

        for col in 0..cols {
            let options = self.column_options(col);
            let col_x = b.min_x() + col as f32 * col_w;
            // Column separator.
            if col > 0 {
                cx.list.push_fill_rect(
                    f(Rect::new(
                        col_x,
                        b.min_y(),
                        cx.pt(HAIR_PT).max(1.0),
                        b.height(),
                    )),
                    border,
                );
            }
            let clip = kurbo::Rect::new(
                f64::from(col_x),
                f64::from(b.min_y()),
                f64::from(col_x + col_w),
                f64::from(b.max_y()),
            );
            for (row, option) in options.iter().enumerate() {
                let y = b.min_y() + row as f32 * row_h;
                if y + row_h > b.max_y() {
                    break;
                }
                let row_r = Rect::new(col_x, y, col_w, row_h);
                let chosen = self.path.get(col) == Some(&row);
                if chosen {
                    cx.list.push_fill_rect(f(row_r), sel_wash);
                } else if self.hover == Some((col, row)) && option.enabled {
                    cx.list.push_fill_rect(f(row_r), hover_wash);
                }
                let ink = if !self.enabled || !option.enabled {
                    muted
                } else {
                    fg
                };
                let chev = cx.pt(CHEV_PT);
                let text_w = (col_w - 2.0 * pad - chev).max(0.0);
                paint_label_clipped(
                    painter,
                    cx.list,
                    clip,
                    kurbo::Point::new(
                        f64::from(col_x + pad),
                        f64::from(y + (row_h - 12.0 * cx.scale) / 2.0),
                    ),
                    &option.label,
                    12.0 * cx.scale,
                    ink,
                );
                let _ = text_w;
                // Disclosure chevron for expandable rows.
                if option.expandable() {
                    paint_label_clipped(
                        painter,
                        cx.list,
                        clip,
                        kurbo::Point::new(
                            f64::from(col_x + col_w - chev + pad / 2.0),
                            f64::from(y + (row_h - 10.0 * cx.scale) / 2.0),
                        ),
                        "›",
                        10.0 * cx.scale,
                        muted,
                    );
                }
            }
        }

        // Empty-state hint when the root is empty.
        if self.root.is_empty() {
            paint_label_clipped(
                painter,
                cx.list,
                f(b),
                kurbo::Point::new(f64::from(b.min_x() + pad), f64::from(b.min_y() + pad)),
                &self.placeholder,
                12.0 * cx.scale,
                muted,
            );
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.enabled {
            return EventResponse::Ignored;
        }
        match cx.event {
            WidgetEvent::PointerMoved { position } => {
                let hit = self
                    .hit(*position, cx.scale)
                    .filter(|(c, r)| self.column_options(*c).get(*r).is_some_and(|o| o.enabled));
                if hit != self.hover {
                    self.hover = hit;
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerReleased { position, button } => {
                if *button != martensite_core::PointerButton::Primary {
                    return EventResponse::Ignored;
                }
                if let Some((col, row)) = self.hit(*position, cx.scale) {
                    if self.column_options(col).get(row).is_some_and(|o| o.enabled) {
                        self.click(col, row);
                        return EventResponse::Handled;
                    }
                }
                EventResponse::Ignored
            }
            WidgetEvent::KeyPressed { key, .. } => {
                // Escape clears the drill path.
                if key == "Escape" && !self.path.is_empty() {
                    self.path.clear();
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            _ => EventResponse::Ignored,
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Group);
        if let Some(ref label) = self.label {
            node.set_label(label.as_str());
        }
        if !self.enabled {
            node.set_disabled();
        }
        if !self.committed.is_empty() {
            node.set_value(self.committed.join(" / "));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::{HotNode, PointerButton, WidgetEvent};

    fn tree() -> Vec<CascaderOption> {
        vec![
            CascaderOption::new("Fruit", "fruit").child(
                CascaderOption::new("Citrus", "citrus")
                    .child(CascaderOption::new("Lemon", "lemon"))
                    .child(CascaderOption::new("Lime", "lime")),
            ),
            CascaderOption::new("Vegetable", "veg"),
        ]
    }

    fn laid_out(c: &mut Cascader, width: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        c.layout(&mut cx, Rect::new(0.0, 0.0, width, h));
    }

    fn ev(c: &mut Cascader, event: &WidgetEvent) -> EventResponse {
        let mut cx = EventContext {
            event,
            bounds: Rect::new(0.0, 0.0, 400.0, 208.0),
            scale: 1.0,
        };
        c.event(&mut cx)
    }

    fn click_cell(c: &mut Cascader, col: usize, row: usize) {
        let cols = c.column_count();
        let col_w = 400.0 / cols as f32;
        let pos = Vec2::new(col as f32 * col_w + 10.0, row as f32 * 26.0 + 13.0);
        let rel = WidgetEvent::PointerReleased {
            position: pos,
            button: PointerButton::Primary,
        };
        ev(c, &rel);
    }

    #[test]
    fn builder() {
        let c = Cascader::new().options(tree());
        assert_eq!(c.option_count(), 2);
        assert!(c.path().is_empty());
        assert!(c.selected_path().is_empty());
    }

    #[test]
    fn column_count_grows_with_path() {
        let mut c = Cascader::new().options(tree());
        assert_eq!(c.column_count(), 1);
        c.click(0, 0); // Fruit — expandable
        assert_eq!(c.column_count(), 2);
        c.click(1, 0); // Citrus — expandable
        assert_eq!(c.column_count(), 3);
    }

    #[test]
    fn leaf_click_commits_path() {
        let mut c = Cascader::new().options(tree());
        laid_out(&mut c, 400.0, 208.0);
        c.click(0, 0);
        c.click(1, 0);
        c.click(2, 0);
        assert_eq!(c.selected_path(), &["fruit", "citrus", "lemon"]);
        assert_eq!(c.take_selected().unwrap(), vec!["fruit", "citrus", "lemon"]);
        assert!(c.take_selected().is_none());
    }

    #[test]
    fn parent_click_clears_commit() {
        let mut c = Cascader::new().options(tree());
        c.click(0, 0);
        c.click(1, 0);
        c.click(2, 0);
        assert_eq!(c.selected_path().len(), 3);
        c.click(0, 1); // Vegetable — leaf at depth 0
        assert_eq!(c.selected_path(), &["veg"]);
    }

    #[test]
    fn shallower_click_truncates_deeper() {
        let mut c = Cascader::new().options(tree());
        c.click(0, 0);
        c.click(1, 0);
        c.click(0, 0);
        assert_eq!(c.path(), &[0]);
    }

    #[test]
    fn disabled_row_ignored() {
        let mut c = Cascader::new().options([CascaderOption::new("off", "off").enabled(false)]);
        laid_out(&mut c, 400.0, 208.0);
        click_cell(&mut c, 0, 0);
        assert!(c.path().is_empty());
        assert!(c.take_selected().is_none());
    }

    #[test]
    fn pointer_click_hits() {
        let mut c = Cascader::new().options(tree());
        laid_out(&mut c, 400.0, 208.0);
        click_cell(&mut c, 0, 0); // Fruit
        assert_eq!(c.path(), &[0]);
        click_cell(&mut c, 1, 0); // Citrus
        assert_eq!(c.path(), &[0, 0]);
        click_cell(&mut c, 2, 1); // Lime — leaf
        assert_eq!(c.selected_path(), &["fruit", "citrus", "lime"]);
    }

    #[test]
    fn escape_clears_path() {
        let mut c = Cascader::new().options(tree());
        laid_out(&mut c, 400.0, 208.0);
        c.click(0, 0);
        let esc = WidgetEvent::KeyPressed {
            key: "Escape".to_string(),
            repeat: false,
        };
        ev(&mut c, &esc);
        assert!(c.path().is_empty());
    }

    #[test]
    fn disabled_inert() {
        let mut c = Cascader::new().options(tree()).enabled(false);
        laid_out(&mut c, 400.0, 208.0);
        click_cell(&mut c, 0, 0);
        assert!(c.path().is_empty());
    }

    #[test]
    fn clear_resets() {
        let mut c = Cascader::new().options(tree());
        c.click(0, 0);
        c.clear();
        assert!(c.path().is_empty());
        assert!(c.selected_path().is_empty());
    }
}
