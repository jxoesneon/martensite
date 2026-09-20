//! `AddressBar` — a browser-style location bar: security chip
//! (🔒/⚠), URL with the registrable domain emphasized, a reload/go
//! button, and a thin page-load progress line.
//!
//! The bar is display-driven: the host sets the URL via
//! [`AddressBar::set_url`], the load fraction via
//! [`AddressBar::set_progress`], and the [`SecurityState`]. Clicks
//! park [`AddressAction`] values in [`AddressBar::take_action`].
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::address_bar::{AddressBar, SecurityState};
//!
//! let a = AddressBar::new("https://example.com/page");
//! assert_eq!(a.state(), SecurityState::Secure);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::text_paint::SharedTextPainter;

const PAD_PT: f32 = 8.0;
const CHIP_PT: f32 = 20.0;
const BTN_PT: f32 = 20.0;
const GAP_PT: f32 = 6.0;
const FONT_PT: f32 = 12.0;
const BAR_PT: f32 = 2.5;

const FACE: [u8; 4] = [40, 43, 52, 255];
const EDGE: [u8; 4] = [78, 82, 92, 255];
const TEXT_FG: [u8; 4] = [235, 237, 240, 255];
const MUTED_FG: [u8; 4] = [150, 154, 164, 255];
const OK: [u8; 4] = [70, 180, 100, 255];
const WARN: [u8; 4] = [220, 160, 40, 255];

/// TLS / page safety state.
///
/// ```
/// use martensite::widgets::address_bar::SecurityState;
///
/// assert_eq!(SecurityState::Secure.glyph(), "🔒");
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SecurityState {
    /// HTTPS with a valid certificate.
    Secure,
    /// HTTP or certificate problem.
    Insecure,
    /// Certificate check pending / page loading.
    Loading,
}

impl SecurityState {
    /// Chip glyph.
    ///
    /// ```
    /// use martensite::widgets::address_bar::SecurityState;
    ///
    /// assert_eq!(SecurityState::Insecure.glyph(), "⚠");
    /// ```
    pub fn glyph(self) -> &'static str {
        match self {
            Self::Secure => "🔒",
            Self::Insecure => "⚠",
            Self::Loading => "◌",
        }
    }
}

/// What the host should do.
///
/// ```
/// use martensite::widgets::address_bar::AddressAction;
///
/// assert_eq!(AddressAction::Reload, AddressAction::Reload);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddressAction {
    /// Reload the page (or stop while loading).
    Reload,
    /// The user activated the field to edit the URL.
    Edit,
    /// The security chip was clicked (host shows details).
    SecurityInfo,
}

/// The location bar — see the module docs.
///
/// ```
/// use martensite::widgets::address_bar::AddressBar;
///
/// assert_eq!(AddressBar::new("https://a.b").url(), "https://a.b");
/// ```
pub struct AddressBar {
    /// Accessibility label.
    pub label: String,
    url: String,
    state: SecurityState,
    progress: Option<f32>,
    action: Option<AddressAction>,
    chip_rect: Rect,
    reload_rect: Rect,
    field_rect: Rect,
    text_painter: Option<SharedTextPainter>,
    bounds: Rect,
    scale: f32,
}

impl std::fmt::Debug for AddressBar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AddressBar")
            .field("url", &self.url)
            .finish()
    }
}

impl AddressBar {
    /// A bar showing `url` (security inferred from the scheme).
    ///
    /// ```
    /// use martensite::widgets::address_bar::{AddressBar, SecurityState};
    ///
    /// assert_eq!(AddressBar::new("http://x").state(), SecurityState::Insecure);
    /// ```
    pub fn new(url: impl Into<String>) -> Self {
        let url = url.into();
        let state = if url.starts_with("https://") {
            SecurityState::Secure
        } else if url.starts_with("http://") {
            SecurityState::Insecure
        } else {
            SecurityState::Loading
        };
        Self {
            label: "Address bar".to_string(),
            url,
            state,
            progress: None,
            action: None,
            chip_rect: Rect::new(0.0, 0.0, 0.0, 0.0),
            reload_rect: Rect::new(0.0, 0.0, 0.0, 0.0),
            field_rect: Rect::new(0.0, 0.0, 0.0, 0.0),
            text_painter: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Current URL.
    ///
    /// ```
    /// use martensite::widgets::address_bar::AddressBar;
    ///
    /// assert_eq!(AddressBar::new("https://a").url(), "https://a");
    /// ```
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Current security state.
    ///
    /// ```
    /// use martensite::widgets::address_bar::{AddressBar, SecurityState};
    ///
    /// assert_eq!(AddressBar::new("https://a").state(), SecurityState::Secure);
    /// ```
    pub fn state(&self) -> SecurityState {
        self.state
    }

    /// Replaces the URL (re-infers security).
    ///
    /// ```
    /// use martensite::widgets::address_bar::{AddressBar, SecurityState};
    ///
    /// let mut a = AddressBar::new("https://a");
    /// a.set_url("http://b");
    /// assert_eq!(a.state(), SecurityState::Insecure);
    /// ```
    pub fn set_url(&mut self, url: impl Into<String>) {
        let url = url.into();
        self.state = if url.starts_with("https://") {
            SecurityState::Secure
        } else if url.starts_with("http://") {
            SecurityState::Insecure
        } else {
            SecurityState::Loading
        };
        self.url = url;
    }

    /// Overrides the security state.
    ///
    /// ```
    /// use martensite::widgets::address_bar::{AddressBar, SecurityState};
    ///
    /// let mut a = AddressBar::new("https://a");
    /// a.set_state(SecurityState::Loading);
    /// assert_eq!(a.state(), SecurityState::Loading);
    /// ```
    pub fn set_state(&mut self, state: SecurityState) {
        self.state = state;
    }

    /// Sets the page-load fraction; `None` hides the line.
    ///
    /// ```
    /// use martensite::widgets::address_bar::AddressBar;
    ///
    /// let mut a = AddressBar::new("https://a");
    /// a.set_progress(Some(0.5));
    /// ```
    pub fn set_progress(&mut self, fraction: Option<f32>) {
        self.progress = fraction.map(|f| f.clamp(0.0, 1.0));
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::address_bar::AddressBar;
    ///
    /// assert_eq!(AddressBar::new("u").label("Location").label, "Location");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Shared text painter.
    ///
    /// ```no_run
    /// use martensite::widgets::address_bar::AddressBar;
    /// # fn example(p: martensite::text_paint::SharedTextPainter) {
    /// let _a = AddressBar::new("u").with_text_painter(p);
    /// # }
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Drains the last clicked action.
    ///
    /// ```
    /// use martensite::widgets::address_bar::AddressBar;
    ///
    /// assert_eq!(AddressBar::new("u").take_action(), None);
    /// ```
    pub fn take_action(&mut self) -> Option<AddressAction> {
        self.action.take()
    }

    /// The registrable-ish domain (host part) for emphasis.
    ///
    /// ```
    /// use martensite::widgets::address_bar::AddressBar;
    ///
    /// assert_eq!(AddressBar::new("https://a.b/c?d").domain(), "a.b");
    /// ```
    pub fn domain(&self) -> &str {
        let after_scheme = self.url.split("://").nth(1).unwrap_or(&self.url);
        after_scheme
            .split(['/', '?', '#'])
            .next()
            .unwrap_or(after_scheme)
    }
}

impl Widget for AddressBar {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let s = cx.scale;
        Vec2::new(
            constraints.max_size.x.max(0.0),
            ((CHIP_PT + PAD_PT * 2.0 + BAR_PT) * s).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(140.0, 26.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        let s = cx.scale;
        let pad = PAD_PT * s;
        let chip = CHIP_PT * s;
        let btn = BTN_PT * s;
        let cy = bounds.min_y() + (bounds.height() - BAR_PT * s - chip) / 2.0;
        self.chip_rect = Rect::new(bounds.min_x() + pad, cy, chip, chip);
        self.reload_rect = Rect::new(bounds.max_x() - pad - btn, cy, btn, btn);
        self.field_rect = Rect::new(
            self.chip_rect.max_x() + GAP_PT * s,
            cy,
            (self.reload_rect.min_x() - GAP_PT * s - self.chip_rect.max_x() - GAP_PT * s).max(0.0),
            chip,
        );
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::TextInput);
        node.set_label(self.label.clone());
        node.set_value(self.url.clone());
        node.add_action(accesskit::Action::Click);
        node.add_action(accesskit::Action::Focus);
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if let WidgetEvent::PointerReleased {
            button: PointerButton::Primary,
            position,
        } = cx.event
        {
            if self.reload_rect.contains(*position) {
                self.action = Some(AddressAction::Reload);
                return EventResponse::RequestRepaint;
            }
            if self.chip_rect.contains(*position) {
                self.action = Some(AddressAction::SecurityInfo);
                return EventResponse::RequestRepaint;
            }
            if self.field_rect.contains(*position) {
                self.action = Some(AddressAction::Edit);
                return EventResponse::CaptureFocus;
            }
        }
        EventResponse::Ignored
    }

    fn paint(&self, cx: &mut PaintContext) {
        let s = cx.scale;
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let b = self.bounds;
        let bar_h = BAR_PT * s;
        let main = kurbo::Rect::new(
            f64::from(b.min_x()),
            f64::from(b.min_y()),
            f64::from(b.max_x()),
            f64::from(b.max_y() - bar_h),
        );
        cx.list.push_fill_shape(
            main,
            &martensite_core::shape::Shape::rounded(6.0 * s),
            cx.color(TokenKey::SurfaceColor, FACE),
        );
        cx.list.push_stroke_rect(main, 1.0, EDGE);
        // Security chip.
        let cr = self.chip_rect;
        let chip_fg = match self.state {
            SecurityState::Secure => OK,
            SecurityState::Insecure => WARN,
            SecurityState::Loading => MUTED_FG,
        };
        let cfs = FONT_PT * s;
        crate::text_paint::paint_label(
            painter,
            cx.list,
            kurbo::Point::new(
                f64::from(cr.min_x() + cr.width() * 0.15),
                f64::from(cr.min_y() + cr.height() * 0.78),
            ),
            self.state.glyph(),
            cfs,
            chip_fg,
        );
        // URL.
        let fs = FONT_PT * s;
        crate::text_paint::paint_label(
            painter,
            cx.list,
            kurbo::Point::new(
                f64::from(self.field_rect.min_x()),
                f64::from(self.field_rect.min_y() + self.field_rect.height() / 2.0 + fs * 0.35),
            ),
            &self.url,
            fs,
            cx.color(TokenKey::TextColor, TEXT_FG),
        );
        // Reload / stop button.
        let rr = self.reload_rect;
        if self.state == SecurityState::Loading || self.progress.is_some_and(|p| p < 1.0) {
            // ✕ stop glyph while loading.
            let q = rr.width() * 0.25;
            let mut p = kurbo::BezPath::new();
            p.move_to((f64::from(rr.min_x() + q), f64::from(rr.min_y() + q)));
            p.line_to((f64::from(rr.max_x() - q), f64::from(rr.max_y() - q)));
            p.move_to((f64::from(rr.max_x() - q), f64::from(rr.min_y() + q)));
            p.line_to((f64::from(rr.min_x() + q), f64::from(rr.max_y() - q)));
            cx.list.push_stroke_path(p, 1.4 * s, MUTED_FG);
        } else {
            // ↻ reload: arc + arrowhead.
            let mut p = kurbo::BezPath::new();
            let c = Vec2::new(
                rr.min_x() + rr.width() / 2.0,
                rr.min_y() + rr.height() / 2.0,
            );
            let r = rr.width() * 0.3;
            for i in 0..=12 {
                let a = -0.6 + (i as f32 / 12.0) * std::f32::consts::TAU * 0.8;
                let pt = Vec2::new(c.x + a.cos() * r, c.y + a.sin() * r);
                if i == 0 {
                    p.move_to((f64::from(pt.x), f64::from(pt.y)));
                } else {
                    p.line_to((f64::from(pt.x), f64::from(pt.y)));
                }
            }
            cx.list.push_stroke_path(p, 1.4 * s, MUTED_FG);
        }
        // Load progress line.
        if let Some(f) = self.progress {
            cx.list.push_fill_rect(
                kurbo::Rect::new(
                    f64::from(b.min_x()),
                    f64::from(b.max_y() - bar_h),
                    f64::from(b.min_x() + b.width() * f),
                    f64::from(b.max_y()),
                ),
                cx.color(TokenKey::AccentColor, [90, 140, 220, 255]),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(a: &mut AddressBar) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        a.layout(&mut cx, Rect::new(0.0, 0.0, 400.0, 32.0));
    }

    fn click(a: &mut AddressBar, r: Rect) {
        a.event(&mut EventContext {
            event: &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: Vec2::new(r.min_x() + r.width() / 2.0, r.min_y() + r.height() / 2.0),
            },
            bounds: a.bounds,
            scale: 1.0,
        });
    }

    #[test]
    fn scheme_infers_state() {
        assert_eq!(AddressBar::new("https://a").state(), SecurityState::Secure);
        assert_eq!(AddressBar::new("http://a").state(), SecurityState::Insecure);
    }

    #[test]
    fn reload_click_parks() {
        let mut a = AddressBar::new("https://a.b");
        laid_out(&mut a);
        let r = a.reload_rect;
        click(&mut a, r);
        assert_eq!(a.take_action(), Some(AddressAction::Reload));
        assert_eq!(a.take_action(), None);
    }

    #[test]
    fn chip_click_parks_security_info() {
        let mut a = AddressBar::new("https://a.b");
        laid_out(&mut a);
        let r = a.chip_rect;
        click(&mut a, r);
        assert_eq!(a.take_action(), Some(AddressAction::SecurityInfo));
    }

    #[test]
    fn domain_parses() {
        assert_eq!(AddressBar::new("https://a.b/c?d").domain(), "a.b");
        assert_eq!(AddressBar::new("a.b/x").domain(), "a.b");
    }

    #[test]
    fn paint_without_painter() {
        let mut a = AddressBar::new("https://a.b/c");
        a.set_progress(Some(0.4));
        laid_out(&mut a);
        let mut list = martensite_core::PaintList::default();
        let theme = martensite_theme::Theme::new("test");
        a.paint(&mut PaintContext {
            list: &mut list,
            bounds: a.bounds,
            theme: &theme,
            scale: 1.0,
            text_painter: None,
        });
        assert!(!list.is_empty());
    }
}
