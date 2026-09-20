//! `PricingTable` — a row of plan cards (Stripe-pricing-page /
//! SaaS tiers idiom): name, price + period, a feature list with
//! ✓/✕ marks, and a CTA button per tier.
//!
//! The `"recommended"` [`Plan`] gets an accent ring and filled CTA.
//! Clicks park the tier index in [`PricingTable::take_chosen`] for
//! the host to start checkout.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::pricing_table::{Plan, PricingTable};
//!
//! let t = PricingTable::new().plan(Plan::new("Pro", "$9").feature("SSO"));
//! assert_eq!(t.plan_count(), 1);
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
const GAP_PT: f32 = 12.0;
const TITLE_PT: f32 = 13.0;
const PRICE_PT: f32 = 20.0;
const FEATURE_PT: f32 = 11.0;
const CTA_PT_H: f32 = 28.0;

const FACE: [u8; 4] = [36, 39, 48, 255];
const EDGE: [u8; 4] = [78, 82, 92, 255];
const TEXT_FG: [u8; 4] = [235, 237, 240, 255];
const MUTED_FG: [u8; 4] = [150, 154, 164, 255];
const OK: [u8; 4] = [70, 180, 100, 255];

/// One pricing tier.
///
/// ```
/// use martensite::widgets::pricing_table::Plan;
///
/// let p = Plan::new("Pro", "$9").period("/mo").feature("SSO");
/// assert_eq!(p.features.len(), 1);
/// ```
#[derive(Clone, Debug)]
pub struct Plan {
    /// Tier name.
    pub name: String,
    /// Price text (e.g. `"$9"`, `"Free"`).
    pub price: String,
    /// Period caption (e.g. `"/mo"`).
    pub period: String,
    /// Feature lines.
    pub features: Vec<String>,
    /// Feature availability marks (parallel to `features`);
    /// missing entries render as ✓.
    pub included: Vec<bool>,
    /// CTA caption.
    pub cta: String,
    /// Renders with the accent ring and filled CTA.
    pub recommended: bool,
}

impl Plan {
    /// A tier named `name` priced at `price`.
    ///
    /// ```
    /// use martensite::widgets::pricing_table::Plan;
    ///
    /// assert_eq!(Plan::new("Free", "$0").price, "$0");
    /// ```
    pub fn new(name: impl Into<String>, price: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            price: price.into(),
            period: String::new(),
            features: Vec::new(),
            included: Vec::new(),
            cta: "Choose".to_string(),
            recommended: false,
        }
    }

    /// Period caption.
    ///
    /// ```
    /// use martensite::widgets::pricing_table::Plan;
    ///
    /// assert_eq!(Plan::new("P", "$1").period("/yr").period, "/yr");
    /// ```
    pub fn period(mut self, period: impl Into<String>) -> Self {
        self.period = period.into();
        self
    }

    /// Appends an included feature.
    ///
    /// ```
    /// use martensite::widgets::pricing_table::Plan;
    ///
    /// assert_eq!(Plan::new("P", "$1").feature("x").features.len(), 1);
    /// ```
    pub fn feature(mut self, text: impl Into<String>) -> Self {
        self.features.push(text.into());
        self.included.push(true);
        self
    }

    /// Appends an unavailable (✕) feature.
    ///
    /// ```
    /// use martensite::widgets::pricing_table::Plan;
    ///
    /// assert_eq!(Plan::new("P", "$1").feature_off("x").included[0], false);
    /// ```
    pub fn feature_off(mut self, text: impl Into<String>) -> Self {
        self.features.push(text.into());
        self.included.push(false);
        self
    }

    /// CTA caption.
    ///
    /// ```
    /// use martensite::widgets::pricing_table::Plan;
    ///
    /// assert_eq!(Plan::new("P", "$1").cta("Buy").cta, "Buy");
    /// ```
    pub fn cta(mut self, cta: impl Into<String>) -> Self {
        self.cta = cta.into();
        self
    }

    /// Marks the tier recommended.
    ///
    /// ```
    /// use martensite::widgets::pricing_table::Plan;
    ///
    /// assert!(Plan::new("P", "$1").recommended().recommended);
    /// ```
    pub fn recommended(mut self) -> Self {
        self.recommended = true;
        self
    }
}

/// The pricing table — see the module docs.
///
/// ```
/// use martensite::widgets::pricing_table::PricingTable;
///
/// assert_eq!(PricingTable::new().plan_count(), 0);
/// ```
pub struct PricingTable {
    /// Accessibility label.
    pub label: String,
    plans: Vec<Plan>,
    chosen: Option<usize>,
    cta_rects: Vec<Rect>,
    text_painter: Option<SharedTextPainter>,
    bounds: Rect,
    scale: f32,
}

impl std::fmt::Debug for PricingTable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PricingTable")
            .field("plans", &self.plans.len())
            .finish()
    }
}

impl PricingTable {
    /// An empty table.
    ///
    /// ```
    /// use martensite::widgets::pricing_table::PricingTable;
    ///
    /// assert_eq!(PricingTable::new().plan_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Pricing".to_string(),
            plans: Vec::new(),
            chosen: None,
            cta_rects: Vec::new(),
            text_painter: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Appends a tier.
    ///
    /// ```
    /// use martensite::widgets::pricing_table::{Plan, PricingTable};
    ///
    /// assert_eq!(PricingTable::new().plan(Plan::new("P", "$1")).plan_count(), 1);
    /// ```
    pub fn plan(mut self, plan: Plan) -> Self {
        self.plans.push(plan);
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::pricing_table::PricingTable;
    ///
    /// assert_eq!(PricingTable::new().label("Plans").label, "Plans");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Shared text painter.
    ///
    /// ```no_run
    /// use martensite::widgets::pricing_table::PricingTable;
    /// # fn example(p: martensite::text_paint::SharedTextPainter) {
    /// let _t = PricingTable::new().with_text_painter(p);
    /// # }
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Tier count.
    ///
    /// ```
    /// use martensite::widgets::pricing_table::PricingTable;
    ///
    /// assert_eq!(PricingTable::new().plan_count(), 0);
    /// ```
    pub fn plan_count(&self) -> usize {
        self.plans.len()
    }

    /// A tier.
    ///
    /// ```
    /// use martensite::widgets::pricing_table::{Plan, PricingTable};
    ///
    /// let t = PricingTable::new().plan(Plan::new("Solo", "$5"));
    /// assert_eq!(t.plan_at(0).unwrap().name, "Solo");
    /// ```
    pub fn plan_at(&self, index: usize) -> Option<&Plan> {
        self.plans.get(index)
    }

    /// Drains the last clicked CTA's tier index.
    ///
    /// ```
    /// use martensite::widgets::pricing_table::PricingTable;
    ///
    /// assert_eq!(PricingTable::new().take_chosen(), None);
    /// ```
    pub fn take_chosen(&mut self) -> Option<usize> {
        self.chosen.take()
    }
}

impl Default for PricingTable {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for PricingTable {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let s = cx.scale;
        let n = self.plans.len().max(1) as f32;
        let w = (200.0 * n + GAP_PT * (n - 1.0)) * s;
        let max_feats = self
            .plans
            .iter()
            .map(|p| p.features.len())
            .max()
            .unwrap_or(0) as f32;
        let h =
            PAD_PT * 2.0 + TITLE_PT + PRICE_PT + max_feats * (FEATURE_PT + 6.0) + CTA_PT_H + 20.0;
        Vec2::new(
            w.min(constraints.max_size.x.max(0.0)),
            (h * s).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(160.0, 120.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        let s = cx.scale;
        self.cta_rects.clear();
        let n = self.plans.len();
        if n == 0 {
            return;
        }
        let pad = PAD_PT * s;
        let gap = GAP_PT * s;
        let cw = (bounds.width() - pad * 2.0 - gap * (n - 1) as f32) / n as f32;
        let cta_h = CTA_PT_H * s;
        for i in 0..n {
            let x = bounds.min_x() + pad + i as f32 * (cw + gap);
            self.cta_rects.push(Rect::new(
                x + pad,
                bounds.max_y() - pad - cta_h,
                (cw - pad * 2.0).max(0.0),
                cta_h,
            ));
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Group);
        node.set_label(self.label.clone());
        node.set_value(format!("{} plans", self.plans.len()));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if let WidgetEvent::PointerReleased {
            button: PointerButton::Primary,
            position,
        } = cx.event
        {
            if let Some(i) = self.cta_rects.iter().position(|r| r.contains(*position)) {
                self.chosen = Some(i);
                return EventResponse::RequestRepaint;
            }
        }
        EventResponse::Ignored
    }

    fn paint(&self, cx: &mut PaintContext) {
        let s = cx.scale;
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let b = self.bounds;
        let pad = PAD_PT * s;
        let gap = GAP_PT * s;
        let n = self.plans.len();
        if n == 0 {
            return;
        }
        let accent = cx.color(TokenKey::AccentColor, [90, 140, 220, 255]);
        let cw = (b.width() - pad * 2.0 - gap * (n - 1) as f32) / n as f32;
        for (i, plan) in self.plans.iter().enumerate() {
            let x = b.min_x() + pad + i as f32 * (cw + gap);
            let card = kurbo::Rect::new(
                f64::from(x),
                f64::from(b.min_y() + pad),
                f64::from(x + cw),
                f64::from(b.max_y() - pad),
            );
            cx.list.push_fill_shape(
                card,
                &martensite_core::shape::Shape::rounded(8.0 * s),
                cx.color(TokenKey::SurfaceColor, FACE),
            );
            cx.list.push_stroke_rect(
                card,
                if plan.recommended { 2.0 } else { 1.0 },
                if plan.recommended { accent } else { EDGE },
            );
            let mut y = b.min_y() + pad * 1.6;
            // Name.
            crate::text_paint::paint_label(
                painter,
                cx.list,
                kurbo::Point::new(f64::from(x + pad), f64::from(y + TITLE_PT * s)),
                &plan.name,
                TITLE_PT * s,
                cx.color(TokenKey::TextColor, TEXT_FG),
            );
            y += TITLE_PT * s + 8.0 * s;
            // Price + period.
            crate::text_paint::paint_label(
                painter,
                cx.list,
                kurbo::Point::new(f64::from(x + pad), f64::from(y + PRICE_PT * s)),
                &plan.price,
                PRICE_PT * s,
                cx.color(TokenKey::TextColor, TEXT_FG),
            );
            let pw = plan.price.len() as f32 * PRICE_PT * 0.6 * s;
            crate::text_paint::paint_label(
                painter,
                cx.list,
                kurbo::Point::new(
                    f64::from(x + pad + pw + 4.0 * s),
                    f64::from(y + PRICE_PT * s),
                ),
                &plan.period,
                FEATURE_PT * s,
                MUTED_FG,
            );
            y += PRICE_PT * s + 12.0 * s;
            // Features.
            for (fi, feat) in plan.features.iter().enumerate() {
                let ok = plan.included.get(fi).copied().unwrap_or(true);
                crate::text_paint::paint_label(
                    painter,
                    cx.list,
                    kurbo::Point::new(f64::from(x + pad), f64::from(y + FEATURE_PT * s)),
                    if ok { "✓" } else { "✕" },
                    FEATURE_PT * s,
                    if ok { OK } else { MUTED_FG },
                );
                crate::text_paint::paint_label(
                    painter,
                    cx.list,
                    kurbo::Point::new(
                        f64::from(x + pad + FEATURE_PT * s + 4.0 * s),
                        f64::from(y + FEATURE_PT * s),
                    ),
                    feat,
                    FEATURE_PT * s,
                    if ok {
                        cx.color(TokenKey::TextColor, TEXT_FG)
                    } else {
                        MUTED_FG
                    },
                );
                y += (FEATURE_PT + 6.0) * s;
            }
            // CTA.
            let cta = self.cta_rects[i];
            let (face, fg) = if plan.recommended {
                (accent, [255, 255, 255, 255])
            } else {
                ([60, 64, 76, 255], TEXT_FG)
            };
            cx.list.push_fill_shape(
                kurbo::Rect::new(
                    f64::from(cta.min_x()),
                    f64::from(cta.min_y()),
                    f64::from(cta.max_x()),
                    f64::from(cta.max_y()),
                ),
                &martensite_core::shape::Shape::rounded(5.0 * s),
                face,
            );
            let cfs = FEATURE_PT * s;
            let w = plan.cta.len() as f32 * cfs * 0.55;
            crate::text_paint::paint_label(
                painter,
                cx.list,
                kurbo::Point::new(
                    f64::from(cta.min_x() + (cta.width() - w) / 2.0),
                    f64::from(cta.min_y() + cta.height() / 2.0 + cfs * 0.35),
                ),
                &plan.cta,
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

    fn fixture() -> PricingTable {
        PricingTable::new()
            .plan(
                Plan::new("Free", "$0")
                    .feature("1 project")
                    .feature_off("SSO"),
            )
            .plan(
                Plan::new("Pro", "$9")
                    .period("/mo")
                    .feature("SSO")
                    .recommended(),
            )
    }

    fn laid_out(t: &mut PricingTable) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        t.layout(&mut cx, Rect::new(0.0, 0.0, 480.0, 260.0));
    }

    #[test]
    fn cta_click_parks_index() {
        let mut t = fixture();
        laid_out(&mut t);
        let r = t.cta_rects[1];
        t.event(&mut EventContext {
            event: &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: Vec2::new(r.min_x() + 4.0, r.min_y() + 4.0),
            },
            bounds: t.bounds,
            scale: 1.0,
        });
        assert_eq!(t.take_chosen(), Some(1));
        assert_eq!(t.take_chosen(), None);
    }

    #[test]
    fn plan_features_track_included() {
        let p = Plan::new("P", "$1").feature("a").feature_off("b");
        assert_eq!(p.included, [true, false]);
    }

    #[test]
    fn paint_without_painter() {
        let mut t = fixture();
        laid_out(&mut t);
        let mut list = martensite_core::PaintList::default();
        let theme = martensite_theme::Theme::new("test");
        t.paint(&mut PaintContext {
            list: &mut list,
            bounds: t.bounds,
            theme: &theme,
            scale: 1.0,
            text_painter: None,
        });
        assert!(!list.is_empty());
    }
}
