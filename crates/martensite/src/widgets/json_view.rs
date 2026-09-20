//! `JsonView` — a collapsible JSON tree (the DevTools /
//! Postman response-inspector idiom).
//!
//! Hosts build a [`JsonNode`] tree — `Null`, `Bool`, `Number`,
//! `String`, `Array`, `Object` — and the widget renders one row
//! per visible node with indent guides, `▸`/`▾` disclosure
//! triangles, and type-colored values. Clicking a disclosure
//! (or the row) toggles expansion and parks the node's path in
//! [`JsonView::take_toggled`]. The wheel scrolls.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::json_view::{JsonNode, JsonView};
//!
//! let root = JsonNode::object("root", [
//!     ("name".into(), JsonNode::string("", "martensite")),
//!     ("tags".into(), JsonNode::array("", [JsonNode::string("", "gui")])),
//! ]);
//! let v = JsonView::new(root);
//! assert_eq!(v.visible_rows(), 4); // root + name + tags + gui
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

const W_PT: f32 = 320.0;
const H_PT: f32 = 240.0;
const ROW_PT: f32 = 18.0;
const INDENT_PT: f32 = 14.0;
const PAD_PT: f32 = 8.0;

const FACE: [u8; 4] = [30, 30, 34, 255];
const KEY: [u8; 4] = [140, 190, 255, 255];
const STR: [u8; 4] = [150, 220, 140, 255];
const NUM: [u8; 4] = [255, 190, 120, 255];
const BOOL: [u8; 4] = [220, 140, 255, 255];
const NULL: [u8; 4] = [130, 130, 138, 255];
const META: [u8; 4] = [140, 140, 150, 255];
const HOVER: [u8; 4] = [44, 44, 50, 255];

/// One JSON value — see [`JsonView`].
///
/// ```
/// use martensite::widgets::json_view::JsonNode;
///
/// assert_eq!(JsonNode::number("n", 42.0).kind_name(), "number");
/// ```
#[derive(Debug, Clone)]
pub struct JsonNode {
    /// Object key, or `""` for array items / the root.
    pub key: String,
    /// The value payload.
    pub value: JsonValue,
    /// Whether container nodes start expanded.
    pub expanded: bool,
}

/// The payload of a [`JsonNode`].
///
/// ```
/// use martensite::widgets::json_view::JsonValue;
///
/// assert_eq!(JsonValue::Null.children().len(), 0);
/// ```
#[derive(Debug, Clone)]
pub enum JsonValue {
    /// JSON `null`.
    Null,
    /// `true` / `false`.
    Bool(bool),
    /// A numeric literal (kept as text for fidelity).
    Number(String),
    /// A string literal (unquoted in storage).
    Str(String),
    /// Ordered array items.
    Array(Vec<JsonNode>),
    /// Ordered object members.
    Object(Vec<JsonNode>),
}

impl JsonValue {
    /// Child nodes for containers.
    ///
    /// ```
    /// use martensite::widgets::json_view::{JsonNode, JsonValue};
    ///
    /// let v = JsonValue::Array(vec![JsonNode::null("")]);
    /// assert_eq!(v.children().len(), 1);
    /// ```
    pub fn children(&self) -> &[JsonNode] {
        match self {
            JsonValue::Array(c) | JsonValue::Object(c) => c,
            _ => &[],
        }
    }

    /// Mutable children for expansion toggles.
    fn children_mut(&mut self) -> &mut [JsonNode] {
        match self {
            JsonValue::Array(c) | JsonValue::Object(c) => c,
            _ => &mut [],
        }
    }

    /// Whether this node can expand.
    fn is_container(&self) -> bool {
        matches!(self, JsonValue::Array(_) | JsonValue::Object(_))
    }
}

impl JsonNode {
    /// `null` leaf.
    ///
    /// ```
    /// use martensite::widgets::json_view::JsonNode;
    ///
    /// assert_eq!(JsonNode::null("x").kind_name(), "null");
    /// ```
    pub fn null(key: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            value: JsonValue::Null,
            expanded: false,
        }
    }

    /// Boolean leaf.
    ///
    /// ```
    /// use martensite::widgets::json_view::JsonNode;
    ///
    /// assert_eq!(JsonNode::boolean("ok", true).kind_name(), "boolean");
    /// ```
    pub fn boolean(key: impl Into<String>, v: bool) -> Self {
        Self {
            key: key.into(),
            value: JsonValue::Bool(v),
            expanded: false,
        }
    }

    /// Numeric leaf.
    ///
    /// ```
    /// use martensite::widgets::json_view::JsonNode;
    ///
    /// assert_eq!(JsonNode::number("n", 3.5).kind_name(), "number");
    /// ```
    pub fn number(key: impl Into<String>, v: f64) -> Self {
        Self {
            key: key.into(),
            value: JsonValue::Number(v.to_string()),
            expanded: false,
        }
    }

    /// String leaf.
    ///
    /// ```
    /// use martensite::widgets::json_view::JsonNode;
    ///
    /// assert_eq!(JsonNode::string("s", "v").kind_name(), "string");
    /// ```
    pub fn string(key: impl Into<String>, v: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            value: JsonValue::Str(v.into()),
            expanded: false,
        }
    }

    /// Array container (starts expanded).
    ///
    /// ```
    /// use martensite::widgets::json_view::JsonNode;
    ///
    /// let a = JsonNode::array("t", [JsonNode::number("", 1.0)]);
    /// assert_eq!(a.kind_name(), "array");
    /// ```
    pub fn array(key: impl Into<String>, items: impl IntoIterator<Item = JsonNode>) -> Self {
        Self {
            key: key.into(),
            value: JsonValue::Array(items.into_iter().collect()),
            expanded: true,
        }
    }

    /// Object container (starts expanded).
    ///
    /// ```
    /// use martensite::widgets::json_view::JsonNode;
    ///
    /// let o = JsonNode::object("o", [("k".into(), JsonNode::null(""))]);
    /// assert_eq!(o.kind_name(), "object");
    /// ```
    pub fn object(
        key: impl Into<String>,
        members: impl IntoIterator<Item = (String, JsonNode)>,
    ) -> Self {
        let children = members
            .into_iter()
            .map(|(k, mut n)| {
                n.key = k;
                n
            })
            .collect();
        Self {
            key: key.into(),
            value: JsonValue::Object(children),
            expanded: true,
        }
    }

    /// Type name for accessibility/tests.
    ///
    /// ```
    /// use martensite::widgets::json_view::JsonNode;
    ///
    /// assert_eq!(JsonNode::null("").kind_name(), "null");
    /// ```
    pub fn kind_name(&self) -> &'static str {
        match self.value {
            JsonValue::Null => "null",
            JsonValue::Bool(_) => "boolean",
            JsonValue::Number(_) => "number",
            JsonValue::Str(_) => "string",
            JsonValue::Array(_) => "array",
            JsonValue::Object(_) => "object",
        }
    }
}

/// A collapsible JSON tree — see the module docs.
///
/// ```
/// use martensite::widgets::json_view::{JsonNode, JsonView};
///
/// assert_eq!(JsonView::new(JsonNode::null("root")).visible_rows(), 1);
/// ```
pub struct JsonView {
    /// Accessibility label.
    pub label: String,
    /// The document root.
    pub root: JsonNode,
    scroll: f32,
    hover: Option<usize>,
    toggled: Option<Vec<usize>>,
    bounds: Rect,
    scale: f32,
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl std::fmt::Debug for JsonView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JsonView")
            .field("label", &self.label)
            .field("rows", &self.visible_rows())
            .finish()
    }
}

impl JsonView {
    /// Creates a viewer over `root` (expanded as built).
    ///
    /// ```
    /// use martensite::widgets::json_view::{JsonNode, JsonView};
    ///
    /// let v = JsonView::new(JsonNode::object("", []));
    /// assert_eq!(v.visible_rows(), 1);
    /// ```
    pub fn new(root: JsonNode) -> Self {
        Self {
            label: "JSON".to_string(),
            root,
            scroll: 0.0,
            hover: None,
            toggled: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
            text_painter: None,
        }
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::json_view::{JsonNode, JsonView};
    ///
    /// assert_eq!(JsonView::new(JsonNode::null("")).label("resp").label, "resp");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Custom text painter.
    ///
    /// ```
    /// use martensite::widgets::json_view::{JsonNode, JsonView};
    ///
    /// let _ = JsonView::new(JsonNode::null("")); // painter optional
    /// ```
    pub fn with_text_painter(mut self, p: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(p);
        self
    }

    /// Rows currently visible (expanded containers included).
    ///
    /// ```
    /// use martensite::widgets::json_view::{JsonNode, JsonView};
    ///
    /// let v = JsonView::new(JsonNode::array("", [JsonNode::null(""), JsonNode::null("")]));
    /// assert_eq!(v.visible_rows(), 3);
    /// ```
    pub fn visible_rows(&self) -> usize {
        let mut n = 0;
        Self::count(&self.root, &mut n);
        n
    }

    fn count(node: &JsonNode, n: &mut usize) {
        *n += 1;
        if node.expanded {
            for c in node.value.children() {
                Self::count(c, n);
            }
        }
    }

    /// Drains the child path (`[1, 0]` = root→1→0) of the node
    /// whose expansion last toggled.
    ///
    /// ```
    /// use martensite::widgets::json_view::{JsonNode, JsonView};
    ///
    /// assert_eq!(JsonView::new(JsonNode::null("")).take_toggled(), None);
    /// ```
    pub fn take_toggled(&mut self) -> Option<Vec<usize>> {
        self.toggled.take()
    }

    /// Expands or collapses the node at `path`.
    ///
    /// ```
    /// use martensite::widgets::json_view::{JsonNode, JsonView};
    ///
    /// let mut v = JsonView::new(JsonNode::array("", [JsonNode::null("")]));
    /// v.toggle(&[]);
    /// assert_eq!(v.visible_rows(), 1); // collapsed root
    /// ```
    pub fn toggle(&mut self, path: &[usize]) {
        let mut node = &mut self.root;
        for &i in path {
            let kids = node.value.children_mut();
            if i >= kids.len() {
                return;
            }
            node = &mut kids[i];
        }
        if node.value.is_container() {
            node.expanded = !node.expanded;
            self.toggled = Some(path.to_vec());
        }
    }

    /// Flattened `(path, depth)` of visible rows for hit tests
    /// and paint.
    fn rows(&self) -> Vec<(Vec<usize>, usize)> {
        let mut out = Vec::new();
        Self::walk(&self.root, &mut Vec::new(), 0, &mut out);
        out
    }

    fn walk(
        node: &JsonNode,
        path: &mut Vec<usize>,
        depth: usize,
        out: &mut Vec<(Vec<usize>, usize)>,
    ) {
        out.push((path.clone(), depth));
        if node.expanded {
            for (i, c) in node.value.children().iter().enumerate() {
                path.push(i);
                Self::walk(c, path, depth + 1, out);
                path.pop();
            }
        }
    }

    /// Node at `path`.
    fn node_at(&self, path: &[usize]) -> &JsonNode {
        let mut n = &self.root;
        for &i in path {
            n = &n.value.children()[i];
        }
        n
    }

    /// Max scroll offset.
    fn max_scroll(&self) -> f32 {
        let row = ROW_PT * self.scale;
        let pad = PAD_PT * self.scale;
        (self.visible_rows() as f32 * row + pad * 2.0 - self.bounds.height()).max(0.0)
    }

    /// Render text for a node: `key: value`.
    fn row_text(&self, n: &JsonNode) -> (String, String) {
        let key = if n.key.is_empty() {
            String::new()
        } else {
            format!("{}: ", n.key)
        };
        let val = match &n.value {
            JsonValue::Null => "null".to_string(),
            JsonValue::Bool(b) => b.to_string(),
            JsonValue::Number(v) => v.clone(),
            JsonValue::Str(s) => format!("\"{s}\""),
            JsonValue::Array(c) => {
                if n.expanded {
                    "[".to_string()
                } else {
                    format!("[{}]", c.len())
                }
            }
            JsonValue::Object(c) => {
                if n.expanded {
                    "{".to_string()
                } else {
                    format!("{{…{}}}", c.len())
                }
            }
        };
        (key, val)
    }
}

impl Widget for JsonView {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            cx.pt(W_PT).min(constraints.max_size.x.max(0.0)),
            cx.pt(H_PT).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(120.0, 48.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        self.scroll = self.scroll.clamp(0.0, self.max_scroll());
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Tree);
        node.set_label(format!("{} — {} rows", self.label, self.visible_rows()));
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
            WidgetEvent::PointerMoved { position } => {
                if !self.bounds.contains(*position) {
                    return EventResponse::Ignored;
                }
                let row = ROW_PT * self.scale;
                let pad = PAD_PT * self.scale;
                let i = ((position.y - self.bounds.min_y() - pad + self.scroll) / row) as usize;
                let new = if i < self.visible_rows() {
                    Some(i)
                } else {
                    None
                };
                if new != self.hover {
                    self.hover = new;
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerLeave => {
                if self.hover.take().is_some() {
                    EventResponse::RequestRepaint
                } else {
                    EventResponse::Ignored
                }
            }
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                ..
            } => {
                if !self.bounds.contains(*position) {
                    return EventResponse::Ignored;
                }
                let row = ROW_PT * self.scale;
                let pad = PAD_PT * self.scale;
                let i = ((position.y - self.bounds.min_y() - pad + self.scroll) / row) as usize;
                let rows = self.rows();
                if let Some((path, _)) = rows.get(i) {
                    if self.node_at(path).value.is_container() {
                        let p = path.clone();
                        self.toggle(&p);
                        return EventResponse::RequestRepaint;
                    }
                }
                EventResponse::Handled
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
        cx.list.push_fill_shape(
            krect(self.bounds),
            &martensite_core::shape::Shape::RECT,
            cx.color(TokenKey::SurfaceColor, FACE),
        );
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let s = self.scale;
        let row = ROW_PT * s;
        let indent = INDENT_PT * s;
        let pad = PAD_PT * s;
        let size = 11.5 * s;
        let rows = self.rows();
        cx.list.push_clip(krect(self.bounds));
        for (i, (path, depth)) in rows.iter().enumerate() {
            let y = self.bounds.min_y() + pad + i as f32 * row - self.scroll;
            if y + row < self.bounds.min_y() || y > self.bounds.max_y() {
                continue;
            }
            let n = self.node_at(path);
            if self.hover == Some(i) {
                cx.list.push_fill_shape(
                    kurbo::Rect::new(
                        f64::from(self.bounds.min_x()),
                        f64::from(y),
                        f64::from(self.bounds.max_x()),
                        f64::from(y + row),
                    ),
                    &martensite_core::shape::Shape::RECT,
                    HOVER,
                );
            }
            let mut x = self.bounds.min_x() + pad + *depth as f32 * indent;
            // Disclosure triangle.
            if n.value.is_container() {
                let tri = if n.expanded { "▾" } else { "▸" };
                crate::text_paint::paint_label(
                    painter,
                    cx.list,
                    kurbo::Point::new(f64::from(x), f64::from(y + row * 0.18)),
                    tri,
                    size,
                    cx.color(TokenKey::TextMutedColor, META),
                );
            }
            x += indent;
            // Key then value.
            let (key, val) = self.row_text(n);
            if !key.is_empty() {
                crate::text_paint::paint_label(
                    painter,
                    cx.list,
                    kurbo::Point::new(f64::from(x), f64::from(y + row * 0.18)),
                    &key,
                    size,
                    cx.color(TokenKey::AccentColor, KEY),
                );
                x += key.chars().count() as f32 * size * 0.55;
            }
            let vcolor = match &n.value {
                JsonValue::Str(_) => STR,
                JsonValue::Number(_) => NUM,
                JsonValue::Bool(_) => BOOL,
                JsonValue::Null => NULL,
                _ => META,
            };
            crate::text_paint::paint_label(
                painter,
                cx.list,
                kurbo::Point::new(f64::from(x), f64::from(y + row * 0.18)),
                &val,
                size,
                vcolor,
            );
        }
        cx.list.pop_clip();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn tree() -> JsonNode {
        JsonNode::object(
            "root",
            [
                ("name".into(), JsonNode::string("", "martensite")),
                (
                    "deps".into(),
                    JsonNode::array("", [JsonNode::number("", 1.0), JsonNode::number("", 2.0)]),
                ),
                ("ok".into(), JsonNode::boolean("", true)),
            ],
        )
    }

    fn laid_out(w: &mut JsonView, wd: f32, h: f32) {
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

    #[test]
    fn visible_counts() {
        let v = JsonView::new(tree());
        assert_eq!(v.visible_rows(), 1 + 3 + 2); // root + 3 members + 2 items
    }

    #[test]
    fn toggle_collapses() {
        let mut v = JsonView::new(tree());
        v.toggle(&[1]); // deps
        assert_eq!(v.visible_rows(), 1 + 3);
        assert_eq!(v.take_toggled(), Some(vec![1]));
        v.toggle(&[1]);
        assert_eq!(v.visible_rows(), 6);
    }

    #[test]
    fn click_toggles_row() {
        let mut v = JsonView::new(tree());
        laid_out(&mut v, 320.0, 240.0);
        // Row 2 is "deps" (root=0, name=1, deps=2).
        let pad = PAD_PT;
        let y = pad + 2.0 * ROW_PT + 4.0;
        v.event(&mut EventContext {
            event: &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(30.0, y),
                count: 1,
            },
            bounds: v.bounds,
            scale: 1.0,
        });
        assert_eq!(v.visible_rows(), 4);
        assert_eq!(v.take_toggled(), Some(vec![1]));
    }

    #[test]
    fn leaf_click_does_nothing() {
        let mut v = JsonView::new(tree());
        laid_out(&mut v, 320.0, 240.0);
        let y = PAD_PT + ROW_PT + 4.0; // row 1 = "name" leaf
        v.event(&mut EventContext {
            event: &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(30.0, y),
                count: 1,
            },
            bounds: v.bounds,
            scale: 1.0,
        });
        assert_eq!(v.take_toggled(), None);
        assert_eq!(v.visible_rows(), 6);
    }

    #[test]
    fn scroll_clamps() {
        let mut v = JsonView::new(tree());
        laid_out(&mut v, 320.0, 60.0);
        v.event(&mut EventContext {
            event: &WidgetEvent::Scroll {
                position: Vec2::new(160.0, 30.0),
                delta: Vec2::new(0.0, -9999.0),
            },
            bounds: v.bounds,
            scale: 1.0,
        });
        assert!(v.scroll <= v.max_scroll());
    }
}
