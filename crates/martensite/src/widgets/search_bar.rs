//! `SearchBar` — a toggleable search strip (libadwaita `AdwSearchBar`,
//! WinUI `AutoSuggestBox` container).
//!
//! Wraps a [`SearchField`] in a bar that measures **zero height while
//! `search_mode` is off** — the GTK pattern where the app toggles the
//! property (e.g. from `Ctrl+F` or a toolbar button) and the bar
//! appears/disappears without manual show/hide bookkeeping. `Escape`
//! inside the field parks a [`take_close_requested`](Self::take_close_requested)
//! signal so the app can flip `search_mode` back off (or not — the
//! mode is always app-owned).
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::search_bar::SearchBar;
//!
//! let mut bar = SearchBar::new().placeholder("Search documents…");
//! assert!(!bar.search_mode);
//! bar.search_mode = true;
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::text_paint::SharedTextPainter;
use crate::widgets::search_field::SearchField;

const PAD_PT: f32 = 6.0;
const FACE: [u8; 4] = [244, 244, 246, 255];
const EDGE: [u8; 4] = [0, 0, 0, 24];

/// A toggleable search strip — see the module docs.
///
/// ```
/// use martensite::widgets::search_bar::SearchBar;
///
/// let bar = SearchBar::new();
/// assert!(!bar.search_mode);
/// ```
pub struct SearchBar {
    /// Whether the bar is revealed. App-owned — toggle it in response
    /// to [`take_close_requested`](Self::take_close_requested) or your
    /// own triggers (`Ctrl+F`, toolbar button, …).
    pub search_mode: bool,
    /// When `false` the whole bar ignores input.
    pub enabled: bool,
    field: SearchField,
    close_requested: bool,
    bounds: Rect,
    scale: f32,
}

impl SearchBar {
    /// Creates a collapsed search bar (`search_mode` off).
    ///
    /// ```
    /// use martensite::widgets::search_bar::SearchBar;
    ///
    /// let bar = SearchBar::new();
    /// assert!(!bar.search_mode);
    /// ```
    pub fn new() -> Self {
        Self {
            search_mode: false,
            enabled: true,
            field: SearchField::new(),
            close_requested: false,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Sets the field's placeholder.
    ///
    /// ```
    /// use martensite::widgets::search_bar::SearchBar;
    ///
    /// let bar = SearchBar::new().placeholder("Search…");
    /// assert_eq!(bar.field().placeholder, "Search…");
    /// ```
    pub fn placeholder(mut self, text: impl Into<String>) -> Self {
        self.field = self.field.placeholder(text);
        self
    }

    /// Enables or disables the bar.
    ///
    /// ```
    /// use martensite::widgets::search_bar::SearchBar;
    ///
    /// let bar = SearchBar::new().enabled(false);
    /// assert!(!bar.enabled);
    /// ```
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self.field = self.field.enabled(enabled);
        self
    }

    /// Installs a shared shaped-text painter on the field.
    ///
    /// ```
    /// use martensite::widgets::search_bar::SearchBar;
    ///
    /// let bar = SearchBar::new();
    /// let _ = bar.search_mode;
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.field = self.field.with_text_painter(painter);
        self
    }

    /// The embedded search field.
    ///
    /// ```
    /// use martensite::widgets::search_bar::SearchBar;
    ///
    /// let bar = SearchBar::new().placeholder("Find");
    /// assert_eq!(bar.field().placeholder, "Find");
    /// ```
    pub fn field(&self) -> &SearchField {
        &self.field
    }

    /// Mutable access to the embedded field (set value, drain its
    /// edit/submit seams directly).
    ///
    /// ```
    /// use martensite::widgets::search_bar::SearchBar;
    ///
    /// let mut bar = SearchBar::new();
    /// bar.field_mut().set_value("query");
    /// assert_eq!(bar.field().value(), "query");
    /// ```
    pub fn field_mut(&mut self) -> &mut SearchField {
        &mut self.field
    }

    /// Drains the field's submitted query (`Enter` inside it).
    ///
    /// ```
    /// use martensite::widgets::search_bar::SearchBar;
    ///
    /// let mut bar = SearchBar::new();
    /// assert_eq!(bar.take_submitted(), None);
    /// ```
    pub fn take_submitted(&mut self) -> Option<String> {
        self.field.take_submitted()
    }

    /// Drains the field's edited flag.
    ///
    /// ```
    /// use martensite::widgets::search_bar::SearchBar;
    ///
    /// let mut bar = SearchBar::new();
    /// assert!(!bar.take_edited());
    /// ```
    pub fn take_edited(&mut self) -> bool {
        self.field.take_edited()
    }

    /// Drains the `Escape` close request.
    ///
    /// ```
    /// use martensite::widgets::search_bar::SearchBar;
    ///
    /// let mut bar = SearchBar::new();
    /// assert!(!bar.take_close_requested());
    /// ```
    pub fn take_close_requested(&mut self) -> bool {
        std::mem::take(&mut self.close_requested)
    }
}

impl Default for SearchBar {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for SearchBar {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        if !self.search_mode {
            return Vec2::ZERO;
        }
        let inner = self.field.measure(cx, constraints);
        Vec2::new(
            inner.x.min(constraints.max_size.x.max(0.0)),
            (inner.y + cx.pt(PAD_PT * 2.0)).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(0.0, 0.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        if self.search_mode {
            let pad = cx.pt(PAD_PT);
            cx.layout_child(
                &mut self.field,
                Rect::new(
                    bounds.origin.x + pad,
                    bounds.origin.y + pad,
                    (bounds.size.x - pad * 2.0).max(0.0),
                    (bounds.size.y - pad * 2.0).max(0.0),
                ),
            );
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Group);
        node.set_label("Search bar");
        if !self.enabled {
            node.set_disabled();
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.enabled || !self.search_mode {
            return EventResponse::Ignored;
        }
        // Escape anywhere in the bar requests mode-off.
        if let WidgetEvent::KeyPressed { key, .. } = cx.event {
            if key == "Escape" {
                self.close_requested = true;
                return EventResponse::RequestRepaint;
            }
        }
        // Forward through the child protocol — the field owns its
        // own hit-testing and focus.
        let pos = cx.event.position();
        if let Some(b) = self.child_bounds(0) {
            if pos.is_none_or(|p| b.contains(p)) {
                let mut child_cx = EventContext {
                    event: cx.event,
                    bounds: b,
                    scale: cx.scale,
                };
                return self.field.event(&mut child_cx);
            }
        }
        EventResponse::Ignored
    }

    fn paint(&self, cx: &mut PaintContext) {
        if !self.search_mode {
            return;
        }
        let r = kurbo::Rect::new(
            f64::from(self.bounds.min_x()),
            f64::from(self.bounds.min_y()),
            f64::from(self.bounds.max_x()),
            f64::from(self.bounds.max_y()),
        );
        cx.list
            .push_fill_rect(r, cx.color(TokenKey::SurfaceColor, FACE));
        // Bottom hairline — the bar reads as a revealed strip.
        let t = cx.pt(1.0);
        cx.list.push_fill_rect(
            kurbo::Rect::new(r.x0, r.y1 - f64::from(t), r.x1, r.y1),
            cx.color(TokenKey::DividerColor, EDGE),
        );
    }

    fn child_count(&self) -> usize {
        1
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        (index == 0).then_some(&self.field as &dyn Widget)
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        (index == 0).then_some(&mut self.field as &mut dyn Widget)
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        if index == 0 && self.search_mode {
            let pad = PAD_PT * self.scale;
            Some(Rect::new(
                self.bounds.origin.x + pad,
                self.bounds.origin.y + pad,
                (self.bounds.size.x - pad * 2.0).max(0.0),
                (self.bounds.size.y - pad * 2.0).max(0.0),
            ))
        } else {
            None
        }
    }
}

impl std::fmt::Debug for SearchBar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SearchBar")
            .field("search_mode", &self.search_mode)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(bar: &mut SearchBar, w: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        bar.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(w, h),
            },
        );
        bar.layout(&mut cx, Rect::new(0.0, 0.0, w, h));
    }

    fn ev<'a>(event: &'a WidgetEvent) -> EventContext<'a> {
        EventContext {
            event,
            bounds: Rect::new(0.0, 0.0, 400.0, 36.0),
            scale: 1.0,
        }
    }

    #[test]
    fn collapsed_measures_zero() {
        let mut bar = SearchBar::new();
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        let size = bar.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(400.0, 400.0),
            },
        );
        assert_eq!(size, Vec2::ZERO);
    }

    #[test]
    fn revealed_measures_field_plus_padding() {
        let mut bar = SearchBar::new();
        bar.search_mode = true;
        laid_out(&mut bar, 400.0, 36.0);
        let b = bar.child_bounds(0).unwrap();
        assert!(b.size.y > 0.0);
        assert!(b.origin.x > 0.0); // padding insets the field
    }

    #[test]
    fn hidden_field_gets_no_bounds() {
        let mut bar = SearchBar::new();
        laid_out(&mut bar, 400.0, 36.0);
        assert_eq!(bar.child_bounds(0), None);
    }

    #[test]
    fn escape_parks_close_request() {
        let mut bar = SearchBar::new();
        bar.search_mode = true;
        laid_out(&mut bar, 400.0, 36.0);
        bar.event(&mut ev(&WidgetEvent::KeyPressed {
            key: "Escape".into(),
            repeat: false,
        }));
        assert!(bar.take_close_requested());
        assert!(!bar.take_close_requested());
    }

    #[test]
    fn events_forward_to_field() {
        let mut bar = SearchBar::new();
        bar.search_mode = true;
        laid_out(&mut bar, 400.0, 36.0);
        let field_b = bar.child_bounds(0).unwrap();
        let resp = bar.event(&mut ev(&WidgetEvent::PointerPressed {
            position: Vec2::new(field_b.origin.x + 10.0, field_b.origin.y + 10.0),
            button: martensite_core::PointerButton::Primary,
            count: 1,
        }));
        assert_ne!(resp, EventResponse::Ignored);
    }

    #[test]
    fn collapsed_ignores_input() {
        let mut bar = SearchBar::new();
        laid_out(&mut bar, 400.0, 36.0);
        assert_eq!(
            bar.event(&mut ev(&WidgetEvent::KeyPressed {
                key: "Escape".into(),
                repeat: false,
            })),
            EventResponse::Ignored
        );
    }
}
