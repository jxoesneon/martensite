//! `WebView` — an embedded-webview surface. The widget owns a
//! [`WebViewHost`] (`martensite-webview`'s engine contract) and paints
//! the host's state snapshot: a loading progress bar, document title,
//! URL, and error banner. When the host reports `has_surface() == false`
//! (the simulated engine, or a backend without raster output yet) the
//! body paints a placeholder — the chrome is identical either way, so
//! apps build against this widget before a native engine lands.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::webview::WebView;
//!
//! let mut wv = WebView::new();
//! wv.navigate("https://example.com");
//! assert_eq!(wv.url(), "https://example.com");
//! assert!(wv.take_events().next().is_some());
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, TokenKey, UnderflowPolicy, Widget,
};
use martensite_webview::{
    SimulatedWebView, WebViewCommand, WebViewEvent, WebViewHost, WebViewState,
};

use crate::text_paint::SharedTextPainter;
use std::collections::VecDeque;

const PAD_PT: f32 = 10.0;
const BAR_PT: f32 = 3.0;
const TITLE_PT: f32 = 14.0;
const SUB_PT: f32 = 11.0;

const FACE: [u8; 4] = [255, 255, 255, 255];
const EDGE: [u8; 4] = [200, 205, 215, 255];
const INK: [u8; 4] = [40, 44, 55, 255];
const MUTED: [u8; 4] = [130, 136, 150, 255];
const ACCENT: [u8; 4] = [80, 130, 230, 255];
const ERR: [u8; 4] = [200, 70, 60, 255];

/// The embedded-webview surface — see the module docs.
///
/// ```
/// use martensite::widgets::webview::WebView;
///
/// let wv = WebView::new();
/// assert_eq!(wv.backend_name(), "simulated");
/// ```
pub struct WebView {
    /// Accessibility label (overrides the document title).
    pub label: String,
    host: Box<dyn WebViewHost>,
    /// Engine events drained on `tick`, retained for the app.
    pending: VecDeque<WebViewEvent>,
    /// Set on any pending event — returned by `tick` for repaint.
    dirty: bool,
    text_painter: Option<SharedTextPainter>,
    bounds: Rect,
    scale: f32,
}

impl std::fmt::Debug for WebView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebView")
            .field("backend", &self.host.backend_name())
            .field("url", &self.host.state().url)
            .finish()
    }
}

impl WebView {
    /// A webview backed by the [`SimulatedWebView`] engine.
    ///
    /// ```
    /// use martensite::widgets::webview::WebView;
    ///
    /// assert_eq!(WebView::new().backend_name(), "simulated");
    /// ```
    pub fn new() -> Self {
        Self::with_host(Box::new(SimulatedWebView::new()))
    }

    /// A webview backed by the best OS-adjacent host —
    /// `martensite-webview-platform`'s `FetchWebView` (real `curl`
    /// HTTP fetches) when `curl` is on `$PATH`, else
    /// `SystemBrowserWebView` (hands navigations to the OS browser).
    /// Still no raster surface; a real engine backend plugs into
    /// [`with_host`](Self::with_host).
    ///
    /// ```
    /// use martensite::widgets::webview::WebView;
    ///
    /// let wv = WebView::native();
    /// assert!(matches!(wv.backend_name(), "fetch" | "system-browser"));
    /// ```
    pub fn native() -> Self {
        Self::with_host(martensite_webview_platform::default_platform_host())
    }

    /// A webview backed by an explicit [`WebViewHost`] — the seam a
    /// native engine backend (WKWebView/WebView2/WebKitGTK) plugs into.
    ///
    /// ```
    /// use martensite::widgets::webview::WebView;
    /// use martensite_webview::SimulatedWebView;
    ///
    /// let wv = WebView::with_host(Box::new(SimulatedWebView::new()));
    /// assert_eq!(wv.url(), "");
    /// ```
    pub fn with_host(host: Box<dyn WebViewHost>) -> Self {
        Self {
            label: String::new(),
            host,
            pending: VecDeque::new(),
            dirty: false,
            text_painter: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Accessibility label override.
    ///
    /// ```
    /// use martensite::widgets::webview::WebView;
    ///
    /// assert_eq!(WebView::new().label("Docs").label, "Docs");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Shared text painter.
    ///
    /// ```no_run
    /// use martensite::widgets::webview::WebView;
    /// # fn example(p: martensite::text_paint::SharedTextPainter) {
    /// let _w = WebView::new().with_text_painter(p);
    /// # }
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    // ---- engine passthroughs -----------------------------------------

    /// Navigate to `url`.
    ///
    /// ```
    /// use martensite::widgets::webview::WebView;
    ///
    /// let mut wv = WebView::new();
    /// wv.navigate("https://a");
    /// assert_eq!(wv.url(), "https://a");
    /// ```
    pub fn navigate(&mut self, url: &str) {
        self.host.navigate(url);
        self.pump();
    }

    /// Issue a raw [`WebViewCommand`].
    ///
    /// ```
    /// use martensite::widgets::webview::WebView;
    /// use martensite_webview::WebViewCommand;
    ///
    /// let mut wv = WebView::new();
    /// wv.command(WebViewCommand::Reload);
    /// ```
    pub fn command(&mut self, cmd: WebViewCommand) {
        self.host.command(cmd);
        self.pump();
    }

    /// History back.
    ///
    /// ```
    /// use martensite::widgets::webview::WebView;
    ///
    /// let mut wv = WebView::new();
    /// wv.navigate("https://a");
    /// wv.navigate("https://b");
    /// wv.go_back();
    /// assert_eq!(wv.url(), "https://a");
    /// ```
    pub fn go_back(&mut self) {
        self.host.go_back();
        self.pump();
    }

    /// History forward.
    ///
    /// ```
    /// use martensite::widgets::webview::WebView;
    ///
    /// let mut wv = WebView::new();
    /// wv.navigate("https://a");
    /// wv.navigate("https://b");
    /// wv.go_back();
    /// wv.go_forward();
    /// assert_eq!(wv.url(), "https://b");
    /// ```
    pub fn go_forward(&mut self) {
        self.host.go_forward();
        self.pump();
    }

    /// Reload the current page.
    ///
    /// ```
    /// use martensite::widgets::webview::WebView;
    ///
    /// let mut wv = WebView::new();
    /// wv.navigate("https://a");
    /// wv.reload();
    /// assert_eq!(wv.url(), "https://a");
    /// ```
    pub fn reload(&mut self) {
        self.host.reload();
        self.pump();
    }

    /// Evaluate JavaScript; the call id matches a
    /// [`WebViewEvent::ScriptResult`] in `take_events`.
    ///
    /// ```
    /// use martensite::widgets::webview::WebView;
    /// use martensite_webview::WebViewEvent;
    ///
    /// let mut wv = WebView::new();
    /// let id = wv.eval_js("2+2");
    /// assert!(wv.take_events().any(|e| matches!(e,
    ///     WebViewEvent::ScriptResult { call_id, .. } if call_id == id)));
    /// ```
    pub fn eval_js(&mut self, source: &str) -> u64 {
        self.host.eval_js(source)
    }

    /// Drain engine events for the app (also called implicitly by
    /// `tick` — whichever drains first wins).
    ///
    /// ```
    /// use martensite::widgets::webview::WebView;
    ///
    /// let mut wv = WebView::new();
    /// wv.navigate("https://x");
    /// assert!(wv.take_events().next().is_some());
    /// ```
    pub fn take_events(&mut self) -> impl Iterator<Item = WebViewEvent> + '_ {
        self.pump();
        std::iter::from_fn(|| self.pending.pop_front())
    }

    fn pump(&mut self) {
        for e in self.host.drain_events() {
            self.pending.push_back(e);
            self.dirty = true;
        }
    }

    // ---- state accessors ---------------------------------------------

    /// The engine's committed state snapshot.
    ///
    /// ```
    /// use martensite::widgets::webview::WebView;
    ///
    /// assert!(!WebView::new().state().loading);
    /// ```
    pub fn state(&self) -> WebViewState {
        self.host.state()
    }

    /// The current or pending URL.
    ///
    /// ```
    /// use martensite::widgets::webview::WebView;
    ///
    /// assert_eq!(WebView::new().url(), "");
    /// ```
    pub fn url(&self) -> String {
        self.state().url
    }

    /// The document title (empty until the engine reports one).
    ///
    /// ```
    /// use martensite::widgets::webview::WebView;
    ///
    /// let mut wv = WebView::new();
    /// wv.navigate("https://a");
    /// assert!(wv.title().contains("https://a"));
    /// ```
    pub fn title(&self) -> String {
        self.state().title
    }

    /// `true` while a load is in flight.
    ///
    /// ```
    /// use martensite::widgets::webview::WebView;
    ///
    /// assert!(!WebView::new().loading());
    /// ```
    pub fn loading(&self) -> bool {
        self.state().loading
    }

    /// Estimated load progress `0.0..=1.0`.
    ///
    /// ```
    /// use martensite::widgets::webview::WebView;
    ///
    /// let mut wv = WebView::new();
    /// wv.navigate("https://a");
    /// assert_eq!(wv.progress(), 1.0); // simulated loads complete
    /// ```
    pub fn progress(&self) -> f32 {
        self.state().progress
    }

    /// `true` when `go_back` would succeed.
    ///
    /// ```
    /// use martensite::widgets::webview::WebView;
    ///
    /// assert!(!WebView::new().can_go_back());
    /// ```
    pub fn can_go_back(&self) -> bool {
        self.state().can_go_back
    }

    /// `true` when `go_forward` would succeed.
    ///
    /// ```
    /// use martensite::widgets::webview::WebView;
    ///
    /// assert!(!WebView::new().can_go_forward());
    /// ```
    pub fn can_go_forward(&self) -> bool {
        self.state().can_go_forward
    }

    /// The backend engine name (`"simulated"` until a native host is
    /// injected).
    ///
    /// ```
    /// use martensite::widgets::webview::WebView;
    ///
    /// assert_eq!(WebView::new().backend_name(), "simulated");
    /// ```
    pub fn backend_name(&self) -> &str {
        self.host.backend_name()
    }
}

impl Default for WebView {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for WebView {
    fn measure(&mut self, _cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        // Fill whatever the parent offers — a webview is chrome-free
        // content area.
        Vec2::new(
            constraints.max_size.x.max(0.0),
            constraints.max_size.y.max(0.0),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(160.0, 90.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::WebView);
        let state = self.state();
        node.set_label(if !self.label.is_empty() {
            self.label.clone()
        } else if !state.title.is_empty() {
            state.title.clone()
        } else {
            "Web view".to_string()
        });
        node.set_value(state.url.clone());
    }

    fn event(&mut self, _cx: &mut EventContext) -> EventResponse {
        EventResponse::Ignored
    }

    fn tick(&mut self, _dt: std::time::Duration) -> bool {
        self.pump();
        self.dirty
    }

    fn paint(&self, cx: &mut PaintContext) {
        let s = cx.scale;
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let b = self.bounds;
        let rect = kurbo::Rect::new(
            f64::from(b.min_x()),
            f64::from(b.min_y()),
            f64::from(b.max_x()),
            f64::from(b.max_y()),
        );
        cx.list.push_fill_shape(
            rect,
            &martensite_core::shape::Shape::rounded(8.0 * s),
            cx.color(TokenKey::SurfaceColor, FACE),
        );
        cx.list.push_stroke_shape(
            rect,
            &martensite_core::shape::Shape::rounded(8.0 * s),
            cx.pt(1.0),
            cx.color(TokenKey::BorderColor, EDGE),
        );

        let state = self.state();
        let pad = PAD_PT * s;

        // Loading progress bar along the top edge.
        if state.loading {
            let bw = b.width() * state.progress.clamp(0.0, 1.0);
            cx.list.push_fill_rect(
                kurbo::Rect::new(
                    f64::from(b.min_x()),
                    f64::from(b.min_y()),
                    f64::from(b.min_x() + bw),
                    f64::from(b.min_y() + BAR_PT * s),
                ),
                cx.color(TokenKey::AccentColor, ACCENT),
            );
        }

        // Body: title + URL centered, or error, or placeholder.
        let cy = b.min_y() + b.height() * 0.5;
        let fs_title = TITLE_PT * s;
        let fs_sub = SUB_PT * s;
        if let Some(err) = &state.error {
            crate::text_paint::paint_label(
                painter,
                cx.list,
                kurbo::Point::new(f64::from(b.min_x() + pad), f64::from(cy - fs_sub)),
                &format!("Failed to load — {err}"),
                fs_sub,
                cx.color(TokenKey::ErrorColor, ERR),
            );
        } else if !state.url.is_empty() {
            crate::text_paint::paint_label(
                painter,
                cx.list,
                kurbo::Point::new(f64::from(b.min_x() + pad), f64::from(cy)),
                if state.title.is_empty() {
                    &state.url
                } else {
                    &state.title
                },
                fs_title,
                cx.color(TokenKey::TextColor, INK),
            );
            crate::text_paint::paint_label(
                painter,
                cx.list,
                kurbo::Point::new(
                    f64::from(b.min_x() + pad),
                    f64::from(cy + fs_title + 2.0 * s),
                ),
                &state.url,
                fs_sub,
                cx.color(TokenKey::TextMutedColor, MUTED),
            );
            if !self.host.has_surface() {
                crate::text_paint::paint_label(
                    painter,
                    cx.list,
                    kurbo::Point::new(
                        f64::from(b.min_x() + pad),
                        f64::from(cy + fs_title + fs_sub + 6.0 * s),
                    ),
                    &format!(
                        "preview — {} engine paints no surface",
                        self.host.backend_name()
                    ),
                    fs_sub,
                    cx.color(TokenKey::TextMutedColor, MUTED),
                );
            }
        } else {
            crate::text_paint::paint_label(
                painter,
                cx.list,
                kurbo::Point::new(f64::from(b.min_x() + pad), f64::from(cy)),
                "No page loaded",
                fs_sub,
                cx.color(TokenKey::TextMutedColor, MUTED),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    #[test]
    fn navigate_back_forward_round_trip() {
        let mut wv = WebView::new();
        wv.navigate("https://a");
        wv.navigate("https://b");
        assert!(wv.can_go_back());
        assert!(!wv.can_go_forward());
        wv.go_back();
        assert_eq!(wv.url(), "https://a");
        wv.go_forward();
        assert_eq!(wv.url(), "https://b");
    }

    #[test]
    fn events_drain_through_widget() {
        let mut wv = WebView::new();
        wv.navigate("https://x");
        let evs: Vec<_> = wv.take_events().collect();
        assert!(evs
            .iter()
            .any(|e| matches!(e, WebViewEvent::LoadFinished { .. })));
        assert!(wv.take_events().next().is_none());
    }

    #[test]
    fn error_state_surfaces() {
        let mut host = SimulatedWebView::new();
        host.fail_next("dns");
        let mut wv = WebView::with_host(Box::new(host));
        wv.navigate("https://gone");
        assert_eq!(wv.state().error.as_deref(), Some("dns"));
    }

    #[test]
    fn paint_without_painter() {
        let mut wv = WebView::new();
        wv.navigate("https://example.com");
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        wv.layout(&mut cx, Rect::new(0.0, 0.0, 300.0, 200.0));
        let mut list = martensite_core::PaintList::default();
        let theme = martensite_theme::Theme::new("test");
        wv.paint(&mut PaintContext {
            list: &mut list,
            bounds: wv.bounds,
            theme: &theme,
            scale: 1.0,
            text_painter: None,
        });
        assert!(!list.is_empty());
    }
}
