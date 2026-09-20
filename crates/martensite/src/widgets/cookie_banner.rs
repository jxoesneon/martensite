//! `CookieBanner` — the GDPR consent strip (Cookiebot / GDPR-banner
//! idiom): a policy message plus Accept / Decline / Customize
//! buttons and an optional policy link.
//!
//! Clicks park a [`CookieConsent`] in [`CookieBanner::take_consent`]
//! for the host to persist; the policy link parks
//! [`CookieBanner::take_policy`]. The host hides the banner once a
//! choice is recorded.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::cookie_banner::CookieBanner;
//!
//! let b = CookieBanner::new("We use cookies");
//! assert_eq!(b.pending(), None);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::text_paint::SharedTextPainter;

const PAD_PT: f32 = 12.0;
const GAP_PT: f32 = 8.0;
const BTN_PT_W: f32 = 92.0;
const BTN_PT_H: f32 = 26.0;
const TEXT_PT: f32 = 11.5;
const LINK_PT: f32 = 10.5;

const FACE: [u8; 4] = [30, 32, 40, 252];
const EDGE: [u8; 4] = [78, 82, 92, 255];
const TEXT_FG: [u8; 4] = [235, 237, 240, 255];
const BTN_FACE: [u8; 4] = [52, 55, 66, 255];

/// The recorded consent choice.
///
/// ```
/// use martensite::widgets::cookie_banner::CookieConsent;
///
/// assert_eq!(CookieConsent::Accepted, CookieConsent::Accepted);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CookieConsent {
    /// "Accept all" was clicked.
    Accepted,
    /// "Decline" was clicked.
    Declined,
    /// "Customize" was clicked — the host shows granular settings.
    Customize,
}

/// The consent strip — see the module docs.
///
/// ```
/// use martensite::widgets::cookie_banner::CookieBanner;
///
/// assert_eq!(CookieBanner::new("msg").message, "msg");
/// ```
pub struct CookieBanner {
    /// Accessibility label.
    pub label: String,
    /// Policy message text.
    pub message: String,
    /// Policy link caption (empty hides it).
    pub policy_caption: String,
    /// Primary button label.
    pub accept_label: String,
    /// Secondary button labels.
    pub decline_label: String,
    /// Customize button label.
    pub customize_label: String,
    consent: Option<CookieConsent>,
    policy: bool,
    buttons: [(Rect, CookieConsent); 3],
    link_rect: Rect,
    text_painter: Option<SharedTextPainter>,
    bounds: Rect,
    scale: f32,
}

impl std::fmt::Debug for CookieBanner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CookieBanner")
            .field("message", &self.message)
            .finish()
    }
}

impl CookieBanner {
    /// A banner with `message` and default button labels.
    ///
    /// ```
    /// use martensite::widgets::cookie_banner::CookieBanner;
    ///
    /// assert_eq!(CookieBanner::new("msg").accept_label, "Accept all");
    /// ```
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            label: "Cookie consent".to_string(),
            message: message.into(),
            policy_caption: String::new(),
            accept_label: "Accept all".to_string(),
            decline_label: "Decline".to_string(),
            customize_label: "Customize".to_string(),
            consent: None,
            policy: false,
            buttons: [
                (Rect::new(0.0, 0.0, 0.0, 0.0), CookieConsent::Accepted),
                (Rect::new(0.0, 0.0, 0.0, 0.0), CookieConsent::Declined),
                (Rect::new(0.0, 0.0, 0.0, 0.0), CookieConsent::Customize),
            ],
            link_rect: Rect::new(0.0, 0.0, 0.0, 0.0),
            text_painter: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Policy link caption.
    ///
    /// ```
    /// use martensite::widgets::cookie_banner::CookieBanner;
    ///
    /// assert_eq!(CookieBanner::new("m").policy_link("Privacy policy").policy_caption, "Privacy policy");
    /// ```
    pub fn policy_link(mut self, caption: impl Into<String>) -> Self {
        self.policy_caption = caption.into();
        self
    }

    /// Button labels `(accept, decline, customize)`.
    ///
    /// ```
    /// use martensite::widgets::cookie_banner::CookieBanner;
    ///
    /// assert_eq!(CookieBanner::new("m").labels("OK", "No", "Settings").decline_label, "No");
    /// ```
    pub fn labels(
        mut self,
        accept: impl Into<String>,
        decline: impl Into<String>,
        customize: impl Into<String>,
    ) -> Self {
        self.accept_label = accept.into();
        self.decline_label = decline.into();
        self.customize_label = customize.into();
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::cookie_banner::CookieBanner;
    ///
    /// assert_eq!(CookieBanner::new("m").label("Consent").label, "Consent");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Shared text painter.
    ///
    /// ```no_run
    /// use martensite::widgets::cookie_banner::CookieBanner;
    /// # fn example(p: martensite::text_paint::SharedTextPainter) {
    /// let _b = CookieBanner::new("m").with_text_painter(p);
    /// # }
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Drains the recorded consent choice.
    ///
    /// ```
    /// use martensite::widgets::cookie_banner::CookieBanner;
    ///
    /// assert_eq!(CookieBanner::new("m").take_consent(), None);
    /// ```
    pub fn take_consent(&mut self) -> Option<CookieConsent> {
        self.consent.take()
    }

    /// Non-destructive read of the pending choice.
    ///
    /// ```
    /// use martensite::widgets::cookie_banner::CookieBanner;
    ///
    /// assert_eq!(CookieBanner::new("m").pending(), None);
    /// ```
    pub fn pending(&self) -> Option<CookieConsent> {
        self.consent
    }

    /// `true` once when the policy link is clicked.
    ///
    /// ```
    /// use martensite::widgets::cookie_banner::CookieBanner;
    ///
    /// assert!(!CookieBanner::new("m").take_policy());
    /// ```
    pub fn take_policy(&mut self) -> bool {
        std::mem::take(&mut self.policy)
    }
}

impl Widget for CookieBanner {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let s = cx.scale;
        Vec2::new(
            constraints.max_size.x.max(0.0),
            ((PAD_PT * 2.0 + TEXT_PT + GAP_PT + BTN_PT_H) * s).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(240.0, 56.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        let s = cx.scale;
        let pad = PAD_PT * s;
        let bw = BTN_PT_W * s;
        let bh = BTN_PT_H * s;
        let gap = GAP_PT * s;
        let by = bounds.max_y() - pad - bh;
        // Accept lands rightmost (primary), then Decline, then
        // Customize reading right-to-left.
        let mut x = bounds.max_x() - pad - bw;
        for (i, consent) in [
            CookieConsent::Accepted,
            CookieConsent::Declined,
            CookieConsent::Customize,
        ]
        .into_iter()
        .enumerate()
        {
            self.buttons[i] = (Rect::new(x, by, bw, bh), consent);
            x -= bw + gap;
        }
        self.link_rect = Rect::new(bounds.min_x() + pad, by, bw, bh);
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Group);
        node.set_label(self.label.clone());
        node.set_value(self.message.clone());
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if let WidgetEvent::PointerReleased {
            button: PointerButton::Primary,
            position,
        } = cx.event
        {
            for (rect, consent) in &self.buttons {
                if rect.contains(*position) {
                    self.consent = Some(*consent);
                    return EventResponse::RequestRepaint;
                }
            }
            if !self.policy_caption.is_empty() && self.link_rect.contains(*position) {
                self.policy = true;
                return EventResponse::RequestRepaint;
            }
        }
        EventResponse::Ignored
    }

    fn paint(&self, cx: &mut PaintContext) {
        let s = cx.scale;
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let b = self.bounds;
        let accent = cx.color(TokenKey::AccentColor, [90, 140, 220, 255]);
        cx.list.push_fill_rect(
            kurbo::Rect::new(
                f64::from(b.min_x()),
                f64::from(b.min_y()),
                f64::from(b.max_x()),
                f64::from(b.max_y()),
            ),
            cx.color(TokenKey::SurfaceColor, FACE),
        );
        cx.list.push_stroke_rect(
            kurbo::Rect::new(
                f64::from(b.min_x()),
                f64::from(b.min_y()),
                f64::from(b.max_x()),
                f64::from(b.max_y()),
            ),
            1.0,
            EDGE,
        );
        let pad = PAD_PT * s;
        crate::text_paint::paint_label(
            painter,
            cx.list,
            kurbo::Point::new(
                f64::from(b.min_x() + pad),
                f64::from(b.min_y() + pad + TEXT_PT * s),
            ),
            &self.message,
            TEXT_PT * s,
            cx.color(TokenKey::TextColor, TEXT_FG),
        );
        if !self.policy_caption.is_empty() {
            crate::text_paint::paint_label(
                painter,
                cx.list,
                kurbo::Point::new(
                    f64::from(self.link_rect.min_x()),
                    f64::from(
                        self.link_rect.min_y() + self.link_rect.height() / 2.0 + LINK_PT * s * 0.35,
                    ),
                ),
                &self.policy_caption,
                LINK_PT * s,
                accent,
            );
        }
        let labels = [
            &self.accept_label,
            &self.decline_label,
            &self.customize_label,
        ];
        for (i, (rect, consent)) in self.buttons.iter().enumerate() {
            let primary = *consent == CookieConsent::Accepted;
            let (face, fg) = if primary {
                (accent, [255, 255, 255, 255])
            } else {
                (BTN_FACE, TEXT_FG)
            };
            cx.list.push_fill_shape(
                kurbo::Rect::new(
                    f64::from(rect.min_x()),
                    f64::from(rect.min_y()),
                    f64::from(rect.max_x()),
                    f64::from(rect.max_y()),
                ),
                &martensite_core::shape::Shape::rounded(4.0 * s),
                face,
            );
            let fs = LINK_PT * s;
            let w = labels[i].len() as f32 * fs * 0.55;
            crate::text_paint::paint_label(
                painter,
                cx.list,
                kurbo::Point::new(
                    f64::from(rect.min_x() + (rect.width() - w) / 2.0),
                    f64::from(rect.min_y() + rect.height() / 2.0 + fs * 0.35),
                ),
                labels[i],
                fs,
                fg,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(b: &mut CookieBanner) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        b.layout(&mut cx, Rect::new(0.0, 0.0, 480.0, 90.0));
    }

    fn click(b: &mut CookieBanner, r: Rect) {
        b.event(&mut EventContext {
            event: &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: Vec2::new(r.min_x() + r.width() / 2.0, r.min_y() + r.height() / 2.0),
            },
            bounds: b.bounds,
            scale: 1.0,
        });
    }

    #[test]
    fn accept_records_consent() {
        let mut b = CookieBanner::new("We use cookies");
        laid_out(&mut b);
        let r = b.buttons[0].0;
        click(&mut b, r);
        assert_eq!(b.take_consent(), Some(CookieConsent::Accepted));
        assert_eq!(b.take_consent(), None);
    }

    #[test]
    fn decline_and_customize() {
        let mut b = CookieBanner::new("m");
        laid_out(&mut b);
        let r = b.buttons[1].0;
        click(&mut b, r);
        assert_eq!(b.take_consent(), Some(CookieConsent::Declined));
        let r = b.buttons[2].0;
        click(&mut b, r);
        assert_eq!(b.take_consent(), Some(CookieConsent::Customize));
    }

    #[test]
    fn policy_link_flags() {
        let mut b = CookieBanner::new("m").policy_link("Privacy");
        laid_out(&mut b);
        let r = b.link_rect;
        click(&mut b, r);
        assert!(b.take_policy());
        assert!(!b.take_policy());
    }

    #[test]
    fn paint_without_painter() {
        let mut b = CookieBanner::new("m").policy_link("Privacy");
        laid_out(&mut b);
        let mut list = martensite_core::PaintList::default();
        let theme = martensite_theme::Theme::new("test");
        b.paint(&mut PaintContext {
            list: &mut list,
            bounds: b.bounds,
            theme: &theme,
            scale: 1.0,
            text_painter: None,
        });
        assert!(!list.is_empty());
    }
}
