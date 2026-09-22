//! `Fishbone` — an Ishikawa cause-and-effect diagram (quality-
//! management idiom): a horizontal spine ending in the *effect*
//! box, with [`Bone`] ribs angled off it — each rib carries a
//! category label and short *cause* tick marks.
//!
//! Hovering a rib parks its index in [`Fishbone::take_hovered`];
//! the widget is otherwise display-only. Companion to
//! [`OrgChart`](crate::widgets::OrgChart) and
//! [`MindMap`](crate::widgets::MindMap).
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::fishbone::{Bone, Fishbone};
//!
//! let f = Fishbone::new("Defects")
//!     .bone(Bone::new("People").cause("training").cause("staffing"))
//!     .bone(Bone::new("Process").cause("no checklist"));
//! assert_eq!(f.bone_count(), 2);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::text_paint::SharedTextPainter;

const PAD_PT: f32 = 12.0;
const FONT_PT: f32 = 11.0;
const CAUSE_PT: f32 = 9.5;
const RIB_PT: f32 = 90.0;

const FACE: [u8; 4] = [30, 32, 40, 255];
const BONE: [u8; 4] = [140, 146, 158, 255];
const SPINE: [u8; 4] = [170, 176, 188, 255];
const TEXT: [u8; 4] = [235, 237, 240, 255];
const EFFECT_BG: [u8; 4] = [80, 110, 170, 255];

/// One category rib: a label plus its causes.
///
/// ```
/// use martensite::widgets::fishbone::Bone;
///
/// let b = Bone::new("Process").cause("no checklist");
/// assert_eq!(b.causes.len(), 1);
/// ```
#[derive(Clone, Debug)]
pub struct Bone {
    /// Category label at the rib tip.
    pub category: String,
    /// Cause labels spaced along the rib.
    pub causes: Vec<String>,
}

impl Bone {
    /// A rib for `category`.
    ///
    /// ```
    /// use martensite::widgets::fishbone::Bone;
    ///
    /// assert_eq!(Bone::new("Tools").category, "Tools");
    /// ```
    pub fn new(category: impl Into<String>) -> Self {
        Self {
            category: category.into(),
            causes: Vec::new(),
        }
    }

    /// Appends a cause.
    ///
    /// ```
    /// use martensite::widgets::fishbone::Bone;
    ///
    /// assert_eq!(Bone::new("T").cause("a").cause("b").causes.len(), 2);
    /// ```
    pub fn cause(mut self, cause: impl Into<String>) -> Self {
        self.causes.push(cause.into());
        self
    }
}

/// The diagram — see the module docs.
///
/// ```
/// use martensite::widgets::fishbone::Fishbone;
///
/// assert_eq!(Fishbone::new("E").bone_count(), 0);
/// ```
pub struct Fishbone {
    /// Accessibility label.
    pub label: String,
    /// Effect text in the head box.
    pub effect: String,
    /// Rib reach in points.
    pub rib: f32,
    bones: Vec<Bone>,
    hovered: Option<usize>,
    ribs: Vec<(Rect, bool)>, // bounding rect + upper/lower
    text_painter: Option<SharedTextPainter>,
    bounds: Rect,
    scale: f32,
}

impl std::fmt::Debug for Fishbone {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Fishbone")
            .field("bones", &self.bones.len())
            .finish()
    }
}

impl Fishbone {
    /// A diagram ending in `effect`.
    ///
    /// ```
    /// use martensite::widgets::fishbone::Fishbone;
    ///
    /// assert_eq!(Fishbone::new("Defects").effect, "Defects");
    /// ```
    pub fn new(effect: impl Into<String>) -> Self {
        Self {
            label: "Cause and effect".to_string(),
            effect: effect.into(),
            rib: RIB_PT,
            bones: Vec::new(),
            hovered: None,
            ribs: Vec::new(),
            text_painter: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Appends a rib.
    ///
    /// ```
    /// use martensite::widgets::fishbone::{Bone, Fishbone};
    ///
    /// assert_eq!(Fishbone::new("E").bone(Bone::new("A")).bone_count(), 1);
    /// ```
    pub fn bone(mut self, bone: Bone) -> Self {
        self.bones.push(bone);
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::fishbone::Fishbone;
    ///
    /// assert_eq!(Fishbone::new("E").label("Ishikawa").label, "Ishikawa");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Shared text painter.
    ///
    /// ```no_run
    /// use martensite::widgets::fishbone::Fishbone;
    /// # fn example(p: martensite::text_paint::SharedTextPainter) {
    /// let _f = Fishbone::new("E").with_text_painter(p);
    /// # }
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Rib count.
    ///
    /// ```
    /// use martensite::widgets::fishbone::Fishbone;
    ///
    /// assert_eq!(Fishbone::new("E").bone_count(), 0);
    /// ```
    pub fn bone_count(&self) -> usize {
        self.bones.len()
    }

    /// Drains the last hovered rib index.
    ///
    /// ```
    /// use martensite::widgets::fishbone::Fishbone;
    ///
    /// let mut f = Fishbone::new("E");
    /// assert_eq!(f.take_hovered(), None);
    /// ```
    pub fn take_hovered(&mut self) -> Option<usize> {
        self.hovered.take()
    }
}

fn line_path(x0: f32, y0: f32, x1: f32, y1: f32) -> kurbo::BezPath {
    let mut p = kurbo::BezPath::new();
    p.move_to((f64::from(x0), f64::from(y0)));
    p.line_to((f64::from(x1), f64::from(y1)));
    p
}

impl Widget for Fishbone {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let s = cx.scale;
        let pairs = self.bones.len().div_ceil(2).max(1) as f32;
        Vec2::new(
            ((pairs * 120.0 + 160.0) * s).min(constraints.max_size.x.max(0.0)),
            ((self.rib * 2.0 + PAD_PT * 2.0) * s).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(200.0, self.rib)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        // Ribs alternate up/down, evenly spaced along the spine.
        self.ribs.clear();
        let s = cx.scale;
        let n = self.bones.len();
        let head_w = 90.0 * s;
        let usable = (bounds.width() - head_w - PAD_PT * 2.0 * s).max(0.0);
        let mid = bounds.min_y() + bounds.height() / 2.0;
        // Tip labels sit `fs` beyond the tip with ink running ~1.25·fs
        // further — cap the rib reach so a shallow allocation doesn't
        // push captions past the edge.
        let rib =
            (self.rib * s).min((bounds.height() / 2.0 - FONT_PT * s * 2.4 - 4.0 * s).max(4.0 * s));
        for i in 0..n {
            let t = (i + 1) as f32 / (n + 1) as f32;
            let bx = bounds.min_x() + PAD_PT * s + usable * t;
            let upper = i % 2 == 0;
            let tip_y = if upper { mid - rib } else { mid + rib };
            self.ribs.push((
                Rect::new(bx - 40.0 * s, tip_y.min(mid), 80.0 * s, (tip_y - mid).abs()),
                upper,
            ));
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Image);
        node.set_label(format!("{}: {}", self.label, self.effect));
        node.set_value(format!("{} categories", self.bones.len()));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerMoved { position } => {
                let h = self.ribs.iter().position(|(r, _)| r.contains(*position));
                if h.is_some() && h != self.hovered {
                    self.hovered = h;
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerLeave => {
                if self.hovered.take().is_some() {
                    EventResponse::RequestRepaint
                } else {
                    EventResponse::Ignored
                }
            }
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let s = cx.scale;
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let mid = self.bounds.min_y() + self.bounds.height() / 2.0;
        let head_w = 90.0 * s;
        let spine_end = self.bounds.max_x() - head_w;
        cx.list.push_fill_rect(
            kurbo::Rect::new(
                f64::from(self.bounds.min_x()),
                f64::from(self.bounds.min_y()),
                f64::from(self.bounds.max_x()),
                f64::from(self.bounds.max_y()),
            ),
            cx.color(TokenKey::BackgroundColor, FACE),
        );
        // Spine.
        cx.list.push_stroke_path(
            line_path(self.bounds.min_x() + PAD_PT * s, mid, spine_end, mid),
            2.0 * s,
            cx.color(TokenKey::TextColor, SPINE),
        );
        // Head box + effect.
        let hr = kurbo::Rect::new(
            f64::from(spine_end),
            f64::from(mid - 18.0 * s),
            f64::from(self.bounds.max_x() - PAD_PT * s),
            f64::from(mid + 18.0 * s),
        );
        cx.list.push_fill_shape(
            hr,
            &martensite_core::shape::Shape::rounded(6.0 * s),
            cx.color(TokenKey::AccentColor, EFFECT_BG),
        );
        crate::text_paint::paint_label_clipped(
            painter,
            cx.list,
            hr,
            kurbo::Point::new(hr.x0 + f64::from(8.0 * s), f64::from(mid + 4.0 * s)),
            &self.effect,
            FONT_PT * s,
            TEXT,
        );
        // Ribs.
        for (i, bone) in self.bones.iter().enumerate() {
            let (r, upper) = self.ribs[i];
            let bx = r.min_x() + r.width() / 2.0;
            // The rib rect already encodes the (possibly height-capped)
            // tip — read it back instead of recomputing.
            let tip_y = if upper { r.min_y() } else { r.max_y() };
            let tip_x = bx + 30.0 * s; // angled toward the head
            let color = if self.hovered == Some(i) {
                cx.color(TokenKey::AccentColor, [90, 140, 220, 255])
            } else {
                cx.color(TokenKey::TextColor, BONE)
            };
            cx.list
                .push_stroke_path(line_path(bx, mid, tip_x, tip_y), 1.5 * s, color);
            // Category label at the tip.
            let fs = FONT_PT * s;
            let tw = painter
                .and_then(|p| p.measure_text(&bone.category, fs))
                .unwrap_or(bone.category.len() as f32 * fs * 0.5);
            let tip_origin = kurbo::Point::new(
                f64::from(tip_x - tw / 2.0),
                f64::from(tip_y + if upper { -4.0 * s } else { fs }),
            );
            if crate::text_paint::label_ink_bounds(painter, tip_origin, &bone.category, fs)
                .is_none_or(|ink| crate::text_paint::visible_ink(cx.list, ink))
            {
                crate::text_paint::paint_label(
                    painter,
                    cx.list,
                    tip_origin,
                    &bone.category,
                    fs,
                    color,
                );
            }
            // Cause ticks along the rib.
            for (j, cause) in bone.causes.iter().enumerate() {
                let t = (j + 1) as f32 / (bone.causes.len() + 1) as f32;
                let cxp = bx + (tip_x - bx) * t;
                let cyp = mid + (tip_y - mid) * t;
                cx.list.push_stroke_path(
                    line_path(cxp - 8.0 * s, cyp, cxp + 8.0 * s, cyp),
                    1.0 * s,
                    color,
                );
                let cause_origin = kurbo::Point::new(
                    f64::from(cxp + 10.0 * s),
                    f64::from(cyp + CAUSE_PT * s * 0.4),
                );
                if crate::text_paint::label_ink_bounds(painter, cause_origin, cause, CAUSE_PT * s)
                    .is_none_or(|ink| crate::text_paint::visible_ink(cx.list, ink))
                {
                    crate::text_paint::paint_label(
                        painter,
                        cx.list,
                        cause_origin,
                        cause,
                        CAUSE_PT * s,
                        color,
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn fixture() -> Fishbone {
        Fishbone::new("Defects")
            .bone(Bone::new("People").cause("training"))
            .bone(Bone::new("Process").cause("no checklist"))
            .bone(Bone::new("Tools"))
    }

    fn laid_out(f: &mut Fishbone) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        f.layout(&mut cx, Rect::new(0.0, 0.0, 640.0, 240.0));
    }

    #[test]
    fn ribs_alternate_and_space() {
        let mut f = fixture();
        laid_out(&mut f);
        assert_eq!(f.ribs.len(), 3);
        assert!(f.ribs[0].1);
        assert!(!f.ribs[1].1);
        assert!(f.ribs[1].0.min_x() > f.ribs[0].0.min_x());
    }

    #[test]
    fn hover_parks_rib() {
        let mut f = fixture();
        laid_out(&mut f);
        let r = f.ribs[0].0;
        f.event(&mut EventContext {
            event: &WidgetEvent::PointerMoved {
                position: Vec2::new((r.min_x() + r.max_x()) / 2.0, (r.min_y() + r.max_y()) / 2.0),
            },
            bounds: f.bounds,
            scale: 1.0,
        });
        assert_eq!(f.take_hovered(), Some(0));
    }

    #[test]
    fn paint_without_painter() {
        let mut f = fixture();
        laid_out(&mut f);
        let mut list = martensite_core::PaintList::default();
        let theme = martensite_theme::Theme::new("test");
        f.paint(&mut PaintContext {
            list: &mut list,
            bounds: f.bounds,
            theme: &theme,
            scale: 1.0,
            text_painter: None,
        });
        assert!(!list.is_empty());
    }
}
