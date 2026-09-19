//! `About` — the "About this app" panel (GTK `AboutDialog`,
//! `NSAboutPanel`, Qt `QMessageBox::about`).
//!
//! A centered display column: optional logo widget, app name,
//! version, description paragraph, a clickable website line, copyright,
//! and titled credits sections (authors, artists, translators — GTK's
//! categories all render identically: bold title over muted lines).
//!
//! Mount inside a [`Dialog`](crate::widgets::dialog) for the chrome;
//! the panel itself owns content only. Clicking the website line parks
//! the URL in [`About::take_activated_url`] — opening the browser is
//! app-space (platform seam).
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::about::About;
//!
//! let about = About::new("Martensite Studio")
//!     .version("1.4.0")
//!     .copyright("© 2025 Cognition")
//!     .website("martensite.dev", "https://martensite.dev")
//!     .credits("Written by", ["A. Dev", "B. Designer"]);
//! assert_eq!(about.app_name, "Martensite Studio");
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, SemanticAction, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::text_paint::SharedTextPainter;

const NAME_FONT_PT: f32 = 18.0;
const BODY_FONT_PT: f32 = 12.0;
const SMALL_FONT_PT: f32 = 10.5;
const SECTION_GAP_PT: f32 = 14.0;
const LINE_GAP_PT: f32 = 3.0;
const PAD_PT: f32 = 20.0;
const LOGO_PT: f32 = 64.0;
const INK: [u8; 4] = [30, 30, 34, 255];
const MUTED: [u8; 4] = [100, 100, 110, 255];
const LINK: [u8; 4] = [0, 102, 204, 255];

/// One titled credits section (GTK authors/artists/translators all
/// render the same: bold heading, one name per line).
struct CreditsSection {
    title: String,
    names: Vec<String>,
}

/// The "About" panel — see the module docs.
///
/// ```
/// use martensite::widgets::about::About;
///
/// let a = About::new("App");
/// assert_eq!(a.app_name, "App");
/// ```
pub struct About {
    /// Application name, shown bold at the top.
    pub app_name: String,
    /// Version string shown under the name (empty hides it).
    pub version: String,
    /// Description paragraph (empty hides it).
    pub comments: String,
    /// Copyright/legal line in small print (empty hides it).
    pub copyright: String,
    /// Website link `(label, url)` — clicking the label parks the URL.
    pub website: Option<(String, String)>,
    /// The optional logo child (an [`Image`](crate::widgets::image),
    /// an `Avatar`, …) rendered `64pt` square at the top.
    logo: Option<Box<dyn Widget>>,
    credits: Vec<CreditsSection>,
    activated_url: Option<String>,
    link_hover: bool,
    /// Website-line hit rect — written in `paint` (where the text
    /// width is known) and read by `event`.
    link_rect: parking_lot::Mutex<Rect>,
    bounds: Rect,
    text_painter: Option<SharedTextPainter>,
    scale: f32,
}

impl About {
    /// Creates a panel for `app_name`.
    ///
    /// ```
    /// use martensite::widgets::about::About;
    ///
    /// let a = About::new("My App");
    /// assert_eq!(a.app_name, "My App");
    /// ```
    pub fn new(app_name: impl Into<String>) -> Self {
        Self {
            app_name: app_name.into(),
            version: String::new(),
            comments: String::new(),
            copyright: String::new(),
            website: None,
            logo: None,
            credits: Vec::new(),
            activated_url: None,
            link_hover: false,
            link_rect: parking_lot::Mutex::new(Rect::new(0.0, 0.0, 0.0, 0.0)),
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            text_painter: None,
            scale: 1.0,
        }
    }

    /// Sets the version line.
    ///
    /// ```
    /// use martensite::widgets::about::About;
    ///
    /// let a = About::new("App").version("2.0");
    /// assert_eq!(a.version, "2.0");
    /// ```
    pub fn version(mut self, v: impl Into<String>) -> Self {
        self.version = v.into();
        self
    }

    /// Sets the description paragraph.
    ///
    /// ```
    /// use martensite::widgets::about::About;
    ///
    /// let a = About::new("App").comments("A toolkit demo.");
    /// assert_eq!(a.comments, "A toolkit demo.");
    /// ```
    pub fn comments(mut self, c: impl Into<String>) -> Self {
        self.comments = c.into();
        self
    }

    /// Sets the copyright line.
    ///
    /// ```
    /// use martensite::widgets::about::About;
    ///
    /// let a = About::new("App").copyright("© 2025");
    /// assert_eq!(a.copyright, "© 2025");
    /// ```
    pub fn copyright(mut self, c: impl Into<String>) -> Self {
        self.copyright = c.into();
        self
    }

    /// Sets the website link shown under the comments.
    ///
    /// ```
    /// use martensite::widgets::about::About;
    ///
    /// let a = About::new("App").website("Homepage", "https://x.dev");
    /// assert_eq!(a.website.as_ref().unwrap().0, "Homepage");
    /// ```
    pub fn website(mut self, label: impl Into<String>, url: impl Into<String>) -> Self {
        self.website = Some((label.into(), url.into()));
        self
    }

    /// Installs a logo widget rendered 64pt square at the top.
    ///
    /// ```
    /// use martensite::widgets::about::About;
    /// use martensite::widgets::text::Text;
    /// use martensite_core::Widget;
    ///
    /// let a = About::new("App").logo(Text::new("[icon]"));
    /// assert_eq!(a.child_count(), 1);
    /// ```
    pub fn logo(mut self, child: impl Widget + 'static) -> Self {
        self.logo = Some(Box::new(child));
        self
    }

    /// Appends a titled credits section.
    ///
    /// ```
    /// use martensite::widgets::about::About;
    ///
    /// let a = About::new("App").credits("Written by", ["A", "B"]);
    /// // Rendered as a "Written by" heading over two lines.
    /// assert_eq!(a.credits_len(), 1);
    /// ```
    pub fn credits(
        mut self,
        title: impl Into<String>,
        names: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.credits.push(CreditsSection {
            title: title.into(),
            names: names.into_iter().map(Into::into).collect(),
        });
        self
    }

    /// Number of credits sections (test/debug seam).
    ///
    /// ```
    /// use martensite::widgets::about::About;
    ///
    /// let a = About::new("App").credits("Team", ["A"]);
    /// assert_eq!(a.credits_len(), 1);
    /// ```
    pub fn credits_len(&self) -> usize {
        self.credits.len()
    }

    /// Installs a shared shaped-text painter.
    ///
    /// ```
    /// use martensite::widgets::about::About;
    ///
    /// let a = About::new("App");
    /// let _ = a.app_name;
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Drains the URL a website-line click parked.
    ///
    /// ```
    /// use martensite::widgets::about::About;
    ///
    /// let mut a = About::new("App");
    /// assert_eq!(a.take_activated_url(), None);
    /// ```
    pub fn take_activated_url(&mut self) -> Option<String> {
        self.activated_url.take()
    }

    /// Total logical content height (pt) — the centered stack's
    /// natural size.
    fn content_pt(&self) -> f32 {
        let mut h = 0.0;
        if self.logo.is_some() {
            h += LOGO_PT + SECTION_GAP_PT;
        }
        h += NAME_FONT_PT + LINE_GAP_PT;
        if !self.version.is_empty() {
            h += SMALL_FONT_PT + SECTION_GAP_PT;
        }
        if !self.comments.is_empty() {
            h += BODY_FONT_PT + SECTION_GAP_PT;
        }
        if self.website.is_some() {
            h += BODY_FONT_PT + SECTION_GAP_PT;
        }
        if !self.copyright.is_empty() {
            h += SMALL_FONT_PT + SECTION_GAP_PT;
        }
        for c in &self.credits {
            h += BODY_FONT_PT + LINE_GAP_PT;
            h += c.names.len() as f32 * (SMALL_FONT_PT + LINE_GAP_PT);
            h += SECTION_GAP_PT;
        }
        h
    }

    /// Vertically-centered starting y for the content stack.
    fn stack_top(&self) -> f32 {
        let h = self.content_pt() * self.scale;
        self.bounds.origin.y + (self.bounds.size.y - h).max(0.0) / 2.0
    }
}

impl Widget for About {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            cx.pt(260.0).min(constraints.max_size.x.max(0.0)),
            cx.pt(self.content_pt() + PAD_PT * 2.0)
                .min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(160.0, 120.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        let top = self.stack_top();
        if let Some(logo) = self.logo.as_mut() {
            let side = cx.pt(LOGO_PT);
            cx.layout_child(
                logo.as_mut(),
                Rect::new(
                    bounds.origin.x + (bounds.size.x - side) / 2.0,
                    top,
                    side,
                    side,
                ),
            );
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Group);
        node.set_label(format!("About {}", self.app_name));
        if let Some((_, url)) = &self.website {
            node.set_value(url.as_str());
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerMoved { position } => {
                let h = self.website.is_some() && self.link_rect.lock().contains(*position);
                if h != self.link_hover {
                    self.link_hover = h;
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerLeave => {
                if self.link_hover {
                    self.link_hover = false;
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                ..
            } if self.website.is_some() && self.link_rect.lock().contains(*position) => {
                if let Some((_, url)) = &self.website {
                    self.activated_url = Some(url.clone());
                }
                EventResponse::RequestRepaint
            }
            WidgetEvent::SemanticAction(SemanticAction::Click) => {
                if let Some((_, url)) = &self.website {
                    self.activated_url = Some(url.clone());
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            // Pointer events off the link fall to the logo child via
            // the default bounds-gated protocol — forward explicitly
            // since we override `event`.
            WidgetEvent::PointerPressed { position, .. }
            | WidgetEvent::PointerReleased { position, .. }
            | WidgetEvent::Scroll { position, .. } => {
                let pos = *position;
                if let Some(b) = self.child_bounds(0) {
                    if b.contains(pos) {
                        if let Some(logo) = self.logo.as_mut() {
                            let mut child_cx = EventContext {
                                event: cx.event,
                                bounds: b,
                                scale: cx.scale,
                            };
                            return logo.event(&mut child_cx);
                        }
                    }
                }
                EventResponse::Ignored
            }
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let cx_pt = cx.scale;
        let mut y = self.stack_top();
        let mid_x = self.bounds.origin.x + self.bounds.size.x / 2.0;
        let ink = cx.color(TokenKey::TextColor, INK);
        let muted = cx.color(TokenKey::TextMutedColor, MUTED);
        let clip = kurbo::Rect::new(
            f64::from(self.bounds.min_x()),
            f64::from(self.bounds.min_y()),
            f64::from(self.bounds.max_x()),
            f64::from(self.bounds.max_y()),
        );
        let centered = |list: &mut martensite_core::PaintList,
                        y_top: f32,
                        text: &str,
                        font: f32,
                        c: [u8; 4]| {
            let w = painter
                .and_then(|p| p.measure_text(text, font))
                .unwrap_or(0.0);
            crate::text_paint::paint_label_clipped(
                painter,
                list,
                clip,
                kurbo::Point::new(f64::from(mid_x - w / 2.0), f64::from(y_top)),
                text,
                font,
                c,
            );
        };
        if self.logo.is_some() {
            y += LOGO_PT * cx_pt + SECTION_GAP_PT * cx_pt;
        }
        centered(cx.list, y, &self.app_name, NAME_FONT_PT * cx_pt, ink);
        y += (NAME_FONT_PT + LINE_GAP_PT) * cx_pt;
        if !self.version.is_empty() {
            centered(cx.list, y, &self.version, SMALL_FONT_PT * cx_pt, muted);
            y += (SMALL_FONT_PT + SECTION_GAP_PT) * cx_pt;
        }
        if !self.comments.is_empty() {
            centered(cx.list, y, &self.comments, BODY_FONT_PT * cx_pt, ink);
            y += (BODY_FONT_PT + SECTION_GAP_PT) * cx_pt;
        }
        if let Some((label, _)) = &self.website {
            let font = BODY_FONT_PT * cx_pt;
            let w = painter
                .and_then(|p| p.measure_text(label, font))
                .unwrap_or(0.0);
            *self.link_rect.lock() = Rect::new(
                mid_x - w / 2.0 - 4.0 * self.scale,
                y - 2.0 * self.scale,
                w + 8.0 * self.scale,
                font + 4.0 * self.scale,
            );
            centered(
                cx.list,
                y,
                label,
                font,
                cx.color(TokenKey::AccentColor, LINK),
            );
            if self.link_hover {
                let t = cx.pt(1.0);
                let uy = y + font + cx.pt(1.0);
                cx.list.push_fill_rect(
                    kurbo::Rect::new(
                        f64::from(mid_x - w / 2.0),
                        f64::from(uy),
                        f64::from(mid_x + w / 2.0),
                        f64::from(uy + t),
                    ),
                    cx.color(TokenKey::AccentColor, LINK),
                );
            }
            y += (BODY_FONT_PT + SECTION_GAP_PT) * cx_pt;
        }
        if !self.copyright.is_empty() {
            centered(cx.list, y, &self.copyright, SMALL_FONT_PT * cx_pt, muted);
            y += (SMALL_FONT_PT + SECTION_GAP_PT) * cx_pt;
        }
        for c in &self.credits {
            centered(cx.list, y, &c.title, BODY_FONT_PT * cx_pt, ink);
            y += (BODY_FONT_PT + LINE_GAP_PT) * cx_pt;
            for name in &c.names {
                centered(cx.list, y, name, SMALL_FONT_PT * cx_pt, muted);
                y += (SMALL_FONT_PT + LINE_GAP_PT) * cx_pt;
            }
            y += SECTION_GAP_PT * cx_pt - LINE_GAP_PT * cx_pt;
        }
    }

    fn child_count(&self) -> usize {
        usize::from(self.logo.is_some())
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        if index == 0 {
            self.logo.as_deref()
        } else {
            None
        }
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        if index == 0 {
            self.logo.as_deref_mut()
        } else {
            None
        }
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        if index != 0 || self.logo.is_none() {
            return None;
        }
        let side = LOGO_PT * self.scale;
        Some(Rect::new(
            self.bounds.origin.x + (self.bounds.size.x - side) / 2.0,
            self.stack_top(),
            side,
            side,
        ))
    }
}

impl std::fmt::Debug for About {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("About")
            .field("app_name", &self.app_name)
            .field("version", &self.version)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(a: &mut About, w: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        a.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(w, h),
            },
        );
        a.layout(&mut cx, Rect::new(0.0, 0.0, w, h));
    }

    #[test]
    fn content_grows_with_sections() {
        let bare = About::new("App");
        let full = About::new("App")
            .version("1.0")
            .comments("A demo")
            .copyright("© x")
            .website("Site", "https://x.dev")
            .credits("By", ["A", "B"]);
        assert!(full.content_pt() > bare.content_pt());
    }

    #[test]
    fn logo_becomes_child() {
        let mut a = About::new("App").logo(crate::widgets::text::Text::new("[i]"));
        laid_out(&mut a, 300.0, 300.0);
        assert_eq!(a.child_count(), 1);
        let b = a.child_bounds(0).unwrap();
        assert!((b.size.x - 64.0).abs() < 1.0);
    }

    #[test]
    fn stack_centers_vertically() {
        let mut a = About::new("App");
        laid_out(&mut a, 300.0, 400.0);
        let top = a.stack_top();
        assert!(top > 0.0);
    }
}
