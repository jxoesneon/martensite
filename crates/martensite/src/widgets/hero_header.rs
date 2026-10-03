//! `HeroHeader` — the landing-page hero block (product-site idiom):
//! an eyebrow caption, a big title, a subtitle line, and primary /
//! secondary call-to-action buttons.
//!
//! Clicks park a [`HeroAction`] in [`HeroHeader::take_action`] for
//! the host to route.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::hero_header::HeroHeader;
//!
//! let h = HeroHeader::new("Ship faster")
//!     .subtitle("The widget toolkit for Rust");
//! assert_eq!(h.title, "Ship faster");
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::text_paint::SharedTextPainter;

const EYEBROW_PT: f32 = 11.0;
const TITLE_PT: f32 = 34.0;
const SUB_PT: f32 = 14.0;
const BTN_PT_W: f32 = 120.0;
const BTN_PT_H: f32 = 34.0;
const GAP_PT: f32 = 12.0;
/// Line-box rhythm — glyph extent plus breathing room per stack row.
const LEAD_EYEBROW: f32 = 26.0;
const LEAD_TITLE: f32 = 60.0;
const LEAD_SUB: f32 = 32.0;
const CTA_GAP: f32 = 20.0;

const TEXT_FG: [u8; 4] = [235, 237, 240, 255];
const MUTED_FG: [u8; 4] = [160, 164, 174, 255];
const BTN_FACE: [u8; 4] = [52, 55, 66, 255];

/// Which CTA fired.
///
/// ```
/// use martensite::widgets::hero_header::HeroAction;
///
/// assert_eq!(HeroAction::Primary, HeroAction::Primary);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeroAction {
    /// Primary CTA.
    Primary,
    /// Secondary CTA.
    Secondary,
}

/// The hero block — see the module docs.
///
/// ```
/// use martensite::widgets::hero_header::HeroHeader;
///
/// assert_eq!(HeroHeader::new("Hi").title, "Hi");
/// ```
pub struct HeroHeader {
    /// Accessibility label.
    pub label: String,
    /// Small caption above the title.
    pub eyebrow: String,
    /// Big headline.
    pub title: String,
    /// Supporting line.
    pub subtitle: String,
    /// Primary CTA caption (empty hides the button).
    pub primary: String,
    /// Secondary CTA caption (empty hides it).
    pub secondary: String,
    action: Option<HeroAction>,
    primary_rect: Rect,
    secondary_rect: Rect,
    text_painter: Option<SharedTextPainter>,
    bounds: Rect,
    scale: f32,
}

impl std::fmt::Debug for HeroHeader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeroHeader")
            .field("title", &self.title)
            .finish()
    }
}

impl HeroHeader {
    /// A hero with `title`.
    ///
    /// ```
    /// use martensite::widgets::hero_header::HeroHeader;
    ///
    /// assert_eq!(HeroHeader::new("T").title, "T");
    /// ```
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            label: "Hero".to_string(),
            eyebrow: String::new(),
            title: title.into(),
            subtitle: String::new(),
            primary: "Get started".to_string(),
            secondary: String::new(),
            action: None,
            primary_rect: Rect::new(0.0, 0.0, 0.0, 0.0),
            secondary_rect: Rect::new(0.0, 0.0, 0.0, 0.0),
            text_painter: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Eyebrow caption.
    ///
    /// ```
    /// use martensite::widgets::hero_header::HeroHeader;
    ///
    /// assert_eq!(HeroHeader::new("T").eyebrow("NEW").eyebrow, "NEW");
    /// ```
    pub fn eyebrow(mut self, text: impl Into<String>) -> Self {
        self.eyebrow = text.into();
        self
    }

    /// Subtitle line.
    ///
    /// ```
    /// use martensite::widgets::hero_header::HeroHeader;
    ///
    /// assert_eq!(HeroHeader::new("T").subtitle("s").subtitle, "s");
    /// ```
    pub fn subtitle(mut self, text: impl Into<String>) -> Self {
        self.subtitle = text.into();
        self
    }

    /// Primary CTA caption.
    ///
    /// ```
    /// use martensite::widgets::hero_header::HeroHeader;
    ///
    /// assert_eq!(HeroHeader::new("T").primary("Buy").primary, "Buy");
    /// ```
    pub fn primary(mut self, text: impl Into<String>) -> Self {
        self.primary = text.into();
        self
    }

    /// Secondary CTA caption.
    ///
    /// ```
    /// use martensite::widgets::hero_header::HeroHeader;
    ///
    /// assert_eq!(HeroHeader::new("T").secondary("Docs").secondary, "Docs");
    /// ```
    pub fn secondary(mut self, text: impl Into<String>) -> Self {
        self.secondary = text.into();
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::hero_header::HeroHeader;
    ///
    /// assert_eq!(HeroHeader::new("T").label("Hero").label, "Hero");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Shared text painter.
    ///
    /// ```no_run
    /// use martensite::widgets::hero_header::HeroHeader;
    /// # fn example(p: martensite::text_paint::SharedTextPainter) {
    /// let _h = HeroHeader::new("T").with_text_painter(p);
    /// # }
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Drains the last clicked CTA.
    ///
    /// ```
    /// use martensite::widgets::hero_header::HeroHeader;
    ///
    /// assert_eq!(HeroHeader::new("T").take_action(), None);
    /// ```
    pub fn take_action(&mut self) -> Option<HeroAction> {
        self.action.take()
    }

    /// Height of the content stack: pad, eyebrow, title, subtitle,
    /// CTA row, pad — shared by `measure` and `layout` so the two can
    /// never drift apart.
    fn content_h(&self, s: f32) -> f32 {
        let mut h = 8.0;
        if !self.eyebrow.is_empty() {
            h += LEAD_EYEBROW;
        }
        h += LEAD_TITLE;
        if !self.subtitle.is_empty() {
            h += LEAD_SUB;
        }
        if !self.primary.is_empty() || !self.secondary.is_empty() {
            h += CTA_GAP + BTN_PT_H;
        }
        (h + 8.0) * s
    }
}

impl Widget for HeroHeader {
    #[cfg(feature = "devtools-timemachine")]
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }

    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            constraints.max_size.x.max(0.0),
            self.content_h(cx.scale)
                .min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(200.0, 80.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        let s = cx.scale;
        let bw = BTN_PT_W * s;
        let bh = BTN_PT_H * s;
        let gap = GAP_PT * s;
        // CTAs follow the text stack (eyebrow → title → subtitle) —
        // the same flow `measure`/`content_h` accounts for.
        let mut ty = bounds.min_y() + 8.0 * s;
        if !self.eyebrow.is_empty() {
            ty += LEAD_EYEBROW * s;
        }
        ty += LEAD_TITLE * s;
        if !self.subtitle.is_empty() {
            ty += LEAD_SUB * s;
        }
        let by = ty + CTA_GAP * s;
        let n = [!self.primary.is_empty(), !self.secondary.is_empty()]
            .iter()
            .filter(|v| **v)
            .count();
        let total = n as f32 * bw + n.saturating_sub(1) as f32 * gap;
        let mut x = bounds.min_x() + (bounds.width() - total) / 2.0;
        self.primary_rect = if self.primary.is_empty() {
            Rect::new(0.0, 0.0, 0.0, 0.0)
        } else {
            let r = Rect::new(x, by, bw, bh);
            x += bw + gap;
            r
        };
        self.secondary_rect = if self.secondary.is_empty() {
            Rect::new(0.0, 0.0, 0.0, 0.0)
        } else {
            Rect::new(x, by, bw, bh)
        };
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Group);
        node.set_label(self.label.clone());
        node.set_value(self.title.clone());
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if let WidgetEvent::PointerReleased {
            button: PointerButton::Primary,
            position,
        } = cx.event
        {
            if !self.primary.is_empty() && self.primary_rect.contains(*position) {
                self.action = Some(HeroAction::Primary);
                return EventResponse::RequestRepaint;
            }
            if !self.secondary.is_empty() && self.secondary_rect.contains(*position) {
                self.action = Some(HeroAction::Secondary);
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
        let cxm = b.min_x() + b.width() / 2.0;
        let mut y = b.min_y() + 8.0 * s;
        // Eyebrow.
        if !self.eyebrow.is_empty() {
            let fs = EYEBROW_PT * s;
            let w = self.eyebrow.len() as f32 * fs * 0.62;
            crate::text_paint::paint_label(
                painter,
                cx.list,
                kurbo::Point::new(f64::from(cxm - w / 2.0), f64::from(y + fs)),
                &self.eyebrow.to_uppercase(),
                fs,
                accent,
            );
            y += LEAD_EYEBROW * s;
        }
        // Title.
        let tfs = TITLE_PT * s;
        let tw = self.title.len() as f32 * tfs * 0.55;
        crate::text_paint::paint_label(
            painter,
            cx.list,
            kurbo::Point::new(f64::from(cxm - tw / 2.0), f64::from(y + tfs)),
            &self.title,
            tfs,
            cx.color(TokenKey::TextColor, TEXT_FG),
        );
        y += LEAD_TITLE * s;
        // Subtitle.
        if !self.subtitle.is_empty() {
            let sfs = SUB_PT * s;
            let sw = self.subtitle.len() as f32 * sfs * 0.55;
            crate::text_paint::paint_label(
                painter,
                cx.list,
                kurbo::Point::new(f64::from(cxm - sw / 2.0), f64::from(y + sfs)),
                &self.subtitle,
                sfs,
                MUTED_FG,
            );
        }
        // CTAs.
        for (rect, caption, primary) in [
            (self.primary_rect, &self.primary, true),
            (self.secondary_rect, &self.secondary, false),
        ] {
            if caption.is_empty() {
                continue;
            }
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
                &martensite_core::shape::Shape::squircle(6.0 * s),
                face,
            );
            let cfs = SUB_PT * s;
            let w = caption.len() as f32 * cfs * 0.55;
            crate::text_paint::paint_label(
                painter,
                cx.list,
                kurbo::Point::new(
                    f64::from(rect.min_x() + (rect.width() - w) / 2.0),
                    crate::text_paint::centered_label_top(
                        painter,
                        rect.min_y() + (rect.height()) / 2.0,
                        caption,
                        cfs,
                    ),
                ),
                caption,
                cfs,
                fg,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(h: &mut HeroHeader) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        h.layout(&mut cx, Rect::new(0.0, 0.0, 640.0, 200.0));
    }

    fn click(h: &mut HeroHeader, r: Rect) {
        h.event(&mut EventContext {
            event: &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: Vec2::new(r.min_x() + r.width() / 2.0, r.min_y() + r.height() / 2.0),
            },
            bounds: h.bounds,
            scale: 1.0,
        });
    }

    #[test]
    fn primary_cta_parks() {
        let mut h = HeroHeader::new("T").secondary("Docs");
        laid_out(&mut h);
        let r = h.primary_rect;
        click(&mut h, r);
        assert_eq!(h.take_action(), Some(HeroAction::Primary));
        assert_eq!(h.take_action(), None);
    }

    #[test]
    fn secondary_cta_parks() {
        let mut h = HeroHeader::new("T").secondary("Docs");
        laid_out(&mut h);
        let r = h.secondary_rect;
        click(&mut h, r);
        assert_eq!(h.take_action(), Some(HeroAction::Secondary));
    }

    #[test]
    fn hidden_secondary_cant_click() {
        let mut h = HeroHeader::new("T");
        laid_out(&mut h);
        assert_eq!(h.secondary_rect.width(), 0.0);
    }

    #[test]
    fn paint_without_painter() {
        let mut h = HeroHeader::new("Ship it")
            .eyebrow("NEW")
            .subtitle("Sub")
            .secondary("Docs");
        laid_out(&mut h);
        let mut list = martensite_core::PaintList::default();
        let theme = martensite_theme::Theme::new("test");
        h.paint(&mut PaintContext {
            list: &mut list,
            bounds: h.bounds,
            theme: &theme,
            scale: 1.0,
            text_painter: None,
        });
        assert!(!list.is_empty());
    }
}
