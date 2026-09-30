//! Hero-card internal children: the slot wells and the transport's
//! play/pause button. Painted/behavioral widgets, kept private to the
//! viewer (`pub(super)`) — the parent drives their contents and polls
//! their click out-seams.

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use kurbo::Shape as _;
use martensite::core::{
    shape::Shape, EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext,
    PointerButton, Rect, Widget, WidgetEvent,
};
use martensite::theme::TokenKey;
use martensite::widgets::MorphIcon;

use super::set_icon_checked;
use crate::icons::IconDef;

/// Which hero slot a well displays — the ring color matches the
/// grid's selection rings (accent = base, success = target).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum WellRole {
    /// The morph's origin slot.
    Base,
    /// The morph's destination slot.
    Target,
}

impl WellRole {
    fn label_prefix(self) -> &'static str {
        match self {
            WellRole::Base => "base",
            WellRole::Target => "target",
        }
    }
}

/// A dashed rounded-rect outline, emitted as one stroked path.
/// `dash`/`gap` are device px along the flattened perimeter.
fn dashed_round_rect(rect: Rect, radius: f32, dash: f32, gap: f32) -> kurbo::BezPath {
    let rr = kurbo::RoundedRect::new(
        f64::from(rect.min_x()),
        f64::from(rect.min_y()),
        f64::from(rect.max_x()),
        f64::from(rect.max_y()),
        f64::from(radius),
    );
    let path = rr.into_path(0.05);
    let mut pts: Vec<kurbo::Point> = Vec::new();
    kurbo::flatten(path, 0.2, |el| match el {
        kurbo::PathEl::MoveTo(p) => pts.push(p),
        kurbo::PathEl::LineTo(p) => pts.push(p),
        kurbo::PathEl::ClosePath => {
            if let Some(&first) = pts.first() {
                pts.push(first);
            }
        }
        _ => {}
    });
    let mut out = kurbo::BezPath::new();
    if pts.len() < 2 {
        return out;
    }
    let mut pen = 0.0f64;
    let mut drawing = true;
    let mut prev = pts[0];
    for &p in &pts[1..] {
        let seg = p - prev;
        let mut len = seg.hypot();
        let dir = if len > 0.0 { seg / len } else { seg };
        while len > 0.0 {
            let step = (if drawing { dash } else { gap }) as f64;
            let take = step.min(len);
            let q = prev + dir * take;
            if drawing {
                out.move_to(prev);
                out.line_to(q);
            }
            prev = q;
            len -= take;
            pen += take;
            if pen >= step {
                pen = 0.0;
                drawing = !drawing;
            }
        }
        prev = p;
    }
    out
}

/// One hero slot well: a square that is either an empty dashed
/// outline (the "pick a slot" constraint) or a small `MorphIcon`
/// thumbnail ringed in its role's selection color. Clicking a filled
/// well parks a click in [`take_clicked`](Self::take_clicked) for the
/// parent to apply deselect semantics to.
pub(super) struct SlotWell {
    role: WellRole,
    /// Bound icon thumbnail.
    icon: MorphIcon,
    /// Feedforward preview — same well, translucent ink.
    ghost: MorphIcon,
    /// Translucent ink used when a ghost is bound.
    ghost_ink: [u8; 4],
    has_icon: bool,
    has_ghost: bool,
    /// Icon display name + pack for labels/tooltips.
    name: String,
    pack_name: &'static str,
    hovered: bool,
    armed: bool,
    clicked: bool,
    bounds: Rect,
    scale: f32,
}

impl SlotWell {
    pub(super) fn new(role: WellRole, icon_pt: f32, stroke_pt: f32) -> Self {
        Self {
            role,
            icon: MorphIcon::new()
                .size(icon_pt)
                .stroke_width(stroke_pt)
                .decorative(true),
            ghost: MorphIcon::new()
                .size(icon_pt)
                .stroke_width(stroke_pt)
                .ink([122, 130, 150, 255])
                .decorative(true),
            ghost_ink: [122, 130, 150, 255],
            has_icon: false,
            has_ghost: false,
            name: String::new(),
            pack_name: "",
            hovered: false,
            armed: false,
            clicked: false,
            bounds: Rect::default(),
            scale: 1.0,
        }
    }

    /// Binds the slot's icon + display name (`None` empties the well).
    /// `pack` is the icon's pack name for tooltips.
    pub(super) fn set(&mut self, icon: Option<(&IconDef, &'static str)>) {
        match icon {
            Some((def, pack)) => {
                set_icon_checked(&mut self.icon, def.d);
                self.name = def.name.to_string();
                self.pack_name = pack;
                self.has_icon = true;
            }
            None => {
                self.has_icon = false;
                self.name.clear();
            }
        }
    }

    /// Sets the feedforward ghost (`None` clears it). Ghosts snap —
    /// they never animate. The ghost is a persistent child: rebinding
    /// the `d` in place keeps the bounds `layout` already assigned —
    /// a fresh `MorphIcon` would paint nothing until the next layout.
    pub(super) fn set_ghost(&mut self, d: Option<&str>) {
        match d {
            Some(d) => {
                set_icon_checked(&mut self.ghost, d);
                self.has_ghost = true;
            }
            None => {
                self.has_ghost = false;
            }
        }
    }

    /// Restyles the ghost's translucent ink (theme-aware callers).
    pub(super) fn ghost_ink(&mut self, rgba: [u8; 4]) {
        self.ghost_ink = rgba;
        self.ghost.set_ink(rgba);
    }

    pub(super) fn set_hovered(&mut self, hovered: bool) {
        self.hovered = hovered;
    }

    /// The `name · pack` tooltip text when filled.
    pub(super) fn tooltip(&self) -> Option<String> {
        self.has_icon.then(|| {
            if self.pack_name.is_empty() {
                self.name.clone()
            } else {
                format!("{} · {}", self.name, self.pack_name)
            }
        })
    }

    /// Drains a completed primary click inside the well.
    pub(super) fn take_clicked(&mut self) -> bool {
        std::mem::take(&mut self.clicked)
    }
}

impl Widget for SlotWell {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let s = cx.pt(44.0);
        Vec2::new(
            s.min(constraints.max_size.x.max(0.0)),
            s.min(constraints.max_size.y.max(0.0)),
        )
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        let inset = bounds.width() * 0.22;
        let inner = Rect::new(
            bounds.min_x() + inset,
            bounds.min_y() + inset,
            (bounds.width() - inset * 2.0).max(0.0),
            (bounds.height() - inset * 2.0).max(0.0),
        );
        cx.layout_child(&mut self.icon, inner);
        cx.layout_child(&mut self.ghost, inner);
    }

    fn paint(&self, cx: &mut PaintContext) {
        let b = self.bounds;
        if b.width() <= 0.0 {
            return;
        }
        let s = self.scale;
        let k = kurbo::Rect::new(
            f64::from(b.min_x()),
            f64::from(b.min_y()),
            f64::from(b.max_x()),
            f64::from(b.max_y()),
        );
        let accent = cx.color(TokenKey::AccentColor, [96, 165, 250, 255]);
        let success = cx.color(TokenKey::SuccessColor, [74, 222, 128, 255]);
        let dim = cx.color(TokenKey::TextMutedColor, [122, 130, 150, 255]);
        let role_color = match self.role {
            WellRole::Base => accent,
            WellRole::Target => success,
        };
        let shape = Shape::rounded(10.0 * s);
        cx.list.push_fill_shape(
            k,
            &shape,
            [255, 255, 255, if self.has_icon { 10 } else { 5 }],
        );
        if self.has_icon {
            cx.list.push_stroke_shape(k, &shape, 1.5 * s, role_color);
        } else {
            let dashes = dashed_round_rect(b, 10.0 * s, 4.0 * s, 3.5 * s);
            cx.list.push_stroke_path(dashes, 1.2 * s, dim);
        }
        // Hover signifier: a small × centered over a filled well —
        // "click clears the slot".
        if self.has_icon && self.hovered {
            let cxf = f64::from(b.min_x() + b.width() * 0.5);
            let cyf = f64::from(b.min_y() + b.height() * 0.5);
            let d = f64::from(6.0 * s);
            let mut x = kurbo::BezPath::new();
            x.move_to(kurbo::Point::new(cxf - d, cyf - d));
            x.line_to(kurbo::Point::new(cxf + d, cyf + d));
            x.move_to(kurbo::Point::new(cxf + d, cyf - d));
            x.line_to(kurbo::Point::new(cxf - d, cyf + d));
            cx.list.push_fill_shape(
                kurbo::Rect::new(cxf - d * 1.7, cyf - d * 1.7, cxf + d * 1.7, cyf + d * 1.7),
                &Shape::rounded(6.0 * s),
                [13, 15, 21, 200],
            );
            cx.list.push_stroke_path(x, 1.6 * s, [226, 230, 240, 235]);
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                ..
            } if self.has_icon && self.bounds.contains(*position) => {
                self.armed = true;
                EventResponse::CapturePointer
            }
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position,
            } => {
                if self.armed {
                    self.armed = false;
                    if self.bounds.contains(*position) {
                        self.clicked = true;
                    }
                    return EventResponse::ReleasePointer;
                }
                EventResponse::Ignored
            }
            WidgetEvent::SemanticAction(martensite::core::SemanticAction::Click)
                if self.has_icon =>
            {
                self.clicked = true;
                EventResponse::Handled
            }
            _ => EventResponse::Ignored,
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Button);
        if self.has_icon {
            node.set_label(format!("{}: {}", self.role.label_prefix(), self.name));
        } else {
            node.set_label(format!("{}: empty", self.role.label_prefix()));
        }
        if !self.has_icon {
            node.set_disabled();
        }
    }

    fn child_count(&self) -> usize {
        2
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        match index {
            // A ghost preview replaces the real thumbnail — painting
            // both muddies the slot.
            0 if self.has_icon && !self.has_ghost => Some(&self.icon),
            1 if self.has_ghost => Some(&self.ghost),
            _ => None,
        }
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        match index {
            0 if self.has_icon && !self.has_ghost => Some(&mut self.icon),
            1 if self.has_ghost => Some(&mut self.ghost),
            _ => None,
        }
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        let _ = index;
        Some(match index {
            0 | 1 => {
                let inset = self.bounds.width() * 0.22;
                Rect::new(
                    self.bounds.min_x() + inset,
                    self.bounds.min_y() + inset,
                    (self.bounds.width() - inset * 2.0).max(0.0),
                    (self.bounds.height() - inset * 2.0).max(0.0),
                )
            }
            _ => return None,
        })
    }
}

/// A pill button painting a single glyph — the transport's
/// play/pause control. Parks clicks in
/// [`take_clicked`](Self::take_clicked).
pub(super) struct IconButton {
    glyph: String,
    label: String,
    hovered: bool,
    armed: bool,
    /// Accent-tinted fill — the paused transport reads as engaged.
    active: bool,
    clicked: bool,
    bounds: Rect,
    scale: f32,
}

impl IconButton {
    pub(super) fn new(glyph: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            glyph: glyph.into(),
            label: label.into(),
            hovered: false,
            armed: false,
            active: false,
            clicked: false,
            bounds: Rect::default(),
            scale: 1.0,
        }
    }

    /// Swaps the painted glyph (play ↔ pause).
    pub(super) fn set_glyph(&mut self, glyph: impl Into<String>, label: impl Into<String>) {
        self.glyph = glyph.into();
        self.label = label.into();
    }

    /// Accent-tint the pill (paused transport).
    pub(super) fn set_active(&mut self, active: bool) {
        self.active = active;
    }

    pub(super) fn set_hovered(&mut self, hovered: bool) {
        self.hovered = hovered;
    }

    /// Current a11y label (the glyph's meaning).
    pub(super) fn label(&self) -> &str {
        &self.label
    }

    /// Drains a completed primary click.
    pub(super) fn take_clicked(&mut self) -> bool {
        std::mem::take(&mut self.clicked)
    }
}

impl Widget for IconButton {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let s = cx.pt(30.0);
        Vec2::new(
            s.min(constraints.max_size.x.max(0.0)),
            s.min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> martensite::core::RenderMinimum {
        martensite::core::RenderMinimum::new(glam::Vec2::new(24.0, 24.0))
            .with_policy(martensite::core::UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
    }

    fn paint(&self, cx: &mut PaintContext) {
        let b = self.bounds;
        if b.width() <= 0.0 {
            return;
        }
        let s = self.scale;
        let k = kurbo::Rect::new(
            f64::from(b.min_x()),
            f64::from(b.min_y()),
            f64::from(b.max_x()),
            f64::from(b.max_y()),
        );
        let ink = cx.color(TokenKey::TextColor, [226, 230, 240, 255]);
        let border = cx.color(TokenKey::BorderColor, [43, 48, 63, 255]);
        let accent = cx.color(TokenKey::AccentColor, [96, 165, 250, 255]);
        let shape = Shape::rounded(10.0 * s);
        cx.list.push_fill_shape(
            k,
            &shape,
            if self.armed {
                [255, 255, 255, 34]
            } else if self.active {
                [accent[0], accent[1], accent[2], 44]
            } else if self.hovered {
                [255, 255, 255, 22]
            } else {
                [255, 255, 255, 10]
            },
        );
        cx.list
            .push_stroke_shape(k, &shape, s, if self.active { accent } else { border });
        // Geometric-shapes glyphs (▮▮/▸) resolve through the normal
        // text cascade — monochrome ink, never emoji full-color — and
        // count as a real label for the icon-only-control rule.
        let size = 13.0 * s;
        let w = cx
            .text_painter
            .and_then(|p| p.measure_text(&self.glyph, size))
            .unwrap_or(self.glyph.chars().count() as f32 * size * 0.55);
        let origin = kurbo::Point::new(
            f64::from(b.min_x() + (b.width() - w).max(0.0) * 0.5),
            f64::from(b.min_y() + (b.height() - size * 1.25).max(0.0) * 0.5),
        );
        if let Some(p) = cx.text_painter {
            p.paint_shaped_text(cx.list, origin, &self.glyph, size, ink);
        } else {
            cx.list.push_text(origin, self.glyph.clone(), size, ink);
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                ..
            } if self.bounds.contains(*position) => {
                self.armed = true;
                EventResponse::CapturePointer
            }
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position,
            } => {
                if self.armed {
                    self.armed = false;
                    if self.bounds.contains(*position) {
                        self.clicked = true;
                    }
                    return EventResponse::ReleasePointer;
                }
                EventResponse::Ignored
            }
            WidgetEvent::SemanticAction(martensite::core::SemanticAction::Click) => {
                self.clicked = true;
                EventResponse::Handled
            }
            _ => EventResponse::Ignored,
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Button);
        node.set_label(self.label.as_str());
    }
}
