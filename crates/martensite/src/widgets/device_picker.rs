//! `DevicePicker` — grouped audio/video device selector (Zoom /
//! Meet device-menu idiom).
//!
//! Sections by [`DeviceKind`] (`Microphone`, `Speaker`, `Camera`)
//! list device names as checkable rows with a small kind glyph.
//! Clicking a row parks `(kind, index)` in
//! [`DevicePicker::take_selected`] and marks it active; the host
//! owns the actual device switch.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::device_picker::{DeviceKind, DevicePicker};
//!
//! let p = DevicePicker::new()
//!     .section(DeviceKind::Microphone, ["Built-in mic", "USB mic"]);
//! assert_eq!(p.section_count(), 1);
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
const ROW_PT: f32 = 24.0;
const HEADER_PT: f32 = 18.0;
const FONT_PT: f32 = 11.5;
const CHECK_PT: f32 = 10.0;

const FACE: [u8; 4] = [34, 36, 44, 255];
const EDGE: [u8; 4] = [78, 82, 92, 255];
const TEXT_FG: [u8; 4] = [235, 237, 240, 255];
const MUTED_FG: [u8; 4] = [150, 154, 164, 255];
const HOVER_BG: [u8; 4] = [255, 255, 255, 14];

/// Device category grouping a section.
///
/// ```
/// use martensite::widgets::device_picker::DeviceKind;
///
/// assert_eq!(DeviceKind::Microphone.glyph(), "🎙");
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceKind {
    /// Input device.
    Microphone,
    /// Output device.
    Speaker,
    /// Video device.
    Camera,
}

impl DeviceKind {
    /// Display glyph for the section header.
    ///
    /// ```
    /// use martensite::widgets::device_picker::DeviceKind;
    ///
    /// assert!(!DeviceKind::Camera.glyph().is_empty());
    /// ```
    pub fn glyph(self) -> &'static str {
        match self {
            Self::Microphone => "🎙",
            Self::Speaker => "🔊",
            Self::Camera => "📷",
        }
    }

    /// Section caption.
    ///
    /// ```
    /// use martensite::widgets::device_picker::DeviceKind;
    ///
    /// assert_eq!(DeviceKind::Speaker.title(), "Speaker");
    /// ```
    pub fn title(self) -> &'static str {
        match self {
            Self::Microphone => "Microphone",
            Self::Speaker => "Speaker",
            Self::Camera => "Camera",
        }
    }
}

struct Section {
    kind: DeviceKind,
    items: Vec<String>,
    active: Option<usize>,
}

/// The picker — see the module docs.
///
/// ```
/// use martensite::widgets::device_picker::DevicePicker;
///
/// assert_eq!(DevicePicker::new().section_count(), 0);
/// ```
pub struct DevicePicker {
    /// Accessibility label.
    pub label: String,
    sections: Vec<Section>,
    selected: Option<(usize, usize)>,
    hovered: Option<(usize, usize)>,
    rows: Vec<(Rect, usize, usize)>,
    text_painter: Option<SharedTextPainter>,
    bounds: Rect,
    scale: f32,
}

impl std::fmt::Debug for DevicePicker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DevicePicker")
            .field("sections", &self.sections.len())
            .finish()
    }
}

impl DevicePicker {
    /// An empty picker.
    ///
    /// ```
    /// use martensite::widgets::device_picker::DevicePicker;
    ///
    /// assert_eq!(DevicePicker::new().section_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Devices".to_string(),
            sections: Vec::new(),
            selected: None,
            hovered: None,
            rows: Vec::new(),
            text_painter: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Appends a section.
    ///
    /// ```
    /// use martensite::widgets::device_picker::{DeviceKind, DevicePicker};
    ///
    /// let p = DevicePicker::new().section(DeviceKind::Camera, ["FaceTime"]);
    /// assert_eq!(p.section_count(), 1);
    /// ```
    pub fn section(
        mut self,
        kind: DeviceKind,
        items: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.sections.push(Section {
            kind,
            items: items.into_iter().map(Into::into).collect(),
            active: None,
        });
        self
    }

    /// Marks a device active.
    ///
    /// ```
    /// use martensite::widgets::device_picker::{DeviceKind, DevicePicker};
    ///
    /// let p = DevicePicker::new()
    ///     .section(DeviceKind::Speaker, ["Monitor"])
    ///     .active(DeviceKind::Speaker, 0);
    /// assert_eq!(p.active_at(DeviceKind::Speaker), Some(0));
    /// ```
    pub fn active(mut self, kind: DeviceKind, index: usize) -> Self {
        if let Some(s) = self.sections.iter_mut().find(|s| s.kind == kind) {
            if index < s.items.len() {
                s.active = Some(index);
            }
        }
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::device_picker::DevicePicker;
    ///
    /// assert_eq!(DevicePicker::new().label("Audio").label, "Audio");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Shared text painter.
    ///
    /// ```no_run
    /// use martensite::widgets::device_picker::DevicePicker;
    /// # fn example(p: martensite::text_paint::SharedTextPainter) {
    /// let _d = DevicePicker::new().with_text_painter(p);
    /// # }
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Section count.
    ///
    /// ```
    /// use martensite::widgets::device_picker::DevicePicker;
    ///
    /// assert_eq!(DevicePicker::new().section_count(), 0);
    /// ```
    pub fn section_count(&self) -> usize {
        self.sections.len()
    }

    /// Active device index in a section.
    ///
    /// ```
    /// use martensite::widgets::device_picker::{DeviceKind, DevicePicker};
    ///
    /// assert_eq!(DevicePicker::new().active_at(DeviceKind::Camera), None);
    /// ```
    pub fn active_at(&self, kind: DeviceKind) -> Option<usize> {
        self.sections
            .iter()
            .find(|s| s.kind == kind)
            .and_then(|s| s.active)
    }

    /// Device name at `(section, index)`.
    ///
    /// ```
    /// use martensite::widgets::device_picker::{DeviceKind, DevicePicker};
    ///
    /// let p = DevicePicker::new().section(DeviceKind::Microphone, ["Mic"]);
    /// assert_eq!(p.item_at(DeviceKind::Microphone, 0), Some("Mic"));
    /// ```
    pub fn item_at(&self, kind: DeviceKind, index: usize) -> Option<&str> {
        self.sections
            .iter()
            .find(|s| s.kind == kind)
            .and_then(|s| s.items.get(index))
            .map(String::as_str)
    }

    /// Drains the last picked `(section_index, item_index)`.
    ///
    /// ```
    /// use martensite::widgets::device_picker::DevicePicker;
    ///
    /// assert_eq!(DevicePicker::new().take_selected(), None);
    /// ```
    pub fn take_selected(&mut self) -> Option<(usize, usize)> {
        self.selected.take()
    }

    fn hit(&self, p: Vec2) -> Option<(usize, usize)> {
        self.rows
            .iter()
            .find(|(r, _, _)| r.contains(p))
            .map(|(_, s, i)| (*s, *i))
    }
}

impl Default for DevicePicker {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for DevicePicker {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let s = cx.scale;
        let h: f32 = self
            .sections
            .iter()
            .map(|sec| HEADER_PT + sec.items.len() as f32 * ROW_PT)
            .sum::<f32>()
            + PAD_PT * 2.0;
        Vec2::new(
            (240.0 * s).min(constraints.max_size.x.max(0.0)),
            (h * s).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(160.0, 40.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        let s = cx.scale;
        self.rows.clear();
        let mut y = bounds.min_y() + PAD_PT * s;
        for (si, sec) in self.sections.iter().enumerate() {
            y += HEADER_PT * s;
            for i in 0..sec.items.len() {
                self.rows.push((
                    Rect::new(
                        bounds.min_x() + PAD_PT * s,
                        y,
                        bounds.width() - PAD_PT * 2.0 * s,
                        ROW_PT * s,
                    ),
                    si,
                    i,
                ));
                y += ROW_PT * s;
            }
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Group);
        node.set_label(self.label.clone());
        node.set_value(format!("{} sections", self.sections.len()));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerMoved { position } => {
                let h = self.hit(*position);
                if h != self.hovered {
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
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position,
            } => {
                if let Some((si, i)) = self.hit(*position) {
                    self.sections[si].active = Some(i);
                    self.selected = Some((si, i));
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let s = cx.scale;
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let b = self.bounds;
        let accent = cx.color(TokenKey::AccentColor, [90, 140, 220, 255]);
        cx.list.push_fill_shape(
            kurbo::Rect::new(
                f64::from(b.min_x()),
                f64::from(b.min_y()),
                f64::from(b.max_x()),
                f64::from(b.max_y()),
            ),
            &martensite_core::shape::Shape::rounded(6.0 * s),
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
        let fs = FONT_PT * s;
        let mut y = b.min_y() + pad;
        for (si, sec) in self.sections.iter().enumerate() {
            crate::text_paint::paint_label(
                painter,
                cx.list,
                kurbo::Point::new(f64::from(b.min_x() + pad), f64::from(y + fs * 0.9)),
                &format!("{} {}", sec.kind.glyph(), sec.kind.title()),
                fs * 0.92,
                MUTED_FG,
            );
            y += HEADER_PT * s;
            for (i, item) in sec.items.iter().enumerate() {
                let r = Rect::new(b.min_x() + pad, y, b.width() - pad * 2.0, ROW_PT * s);
                if self.hovered == Some((si, i)) {
                    cx.list.push_fill_rect(
                        kurbo::Rect::new(
                            f64::from(r.min_x()),
                            f64::from(r.min_y()),
                            f64::from(r.max_x()),
                            f64::from(r.max_y()),
                        ),
                        HOVER_BG,
                    );
                }
                if sec.active == Some(i) {
                    // Check mark.
                    let cy = r.min_y() + r.height() / 2.0;
                    let ck = CHECK_PT * s;
                    let mut p = kurbo::BezPath::new();
                    p.move_to((f64::from(r.min_x()), f64::from(cy)));
                    p.line_to((f64::from(r.min_x() + ck * 0.35), f64::from(cy + ck * 0.35)));
                    p.line_to((f64::from(r.min_x() + ck), f64::from(cy - ck * 0.45)));
                    cx.list.push_stroke_path(p, 1.6 * s, accent);
                }
                crate::text_paint::paint_label(
                    painter,
                    cx.list,
                    kurbo::Point::new(
                        f64::from(r.min_x() + CHECK_PT * s + GAP_TEXT * s),
                        f64::from(r.min_y() + r.height() / 2.0 + fs * 0.35),
                    ),
                    item,
                    fs,
                    cx.color(TokenKey::TextColor, TEXT_FG),
                );
                y += ROW_PT * s;
            }
        }
    }
}

const GAP_TEXT: f32 = 6.0;

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn fixture() -> DevicePicker {
        DevicePicker::new()
            .section(DeviceKind::Microphone, ["Built-in", "USB mic"])
            .section(DeviceKind::Speaker, ["Monitor", "Headphones"])
    }

    fn laid_out(p: &mut DevicePicker) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        p.layout(&mut cx, Rect::new(0.0, 0.0, 240.0, 160.0));
    }

    #[test]
    fn click_selects_and_marks_active() {
        let mut p = fixture();
        laid_out(&mut p);
        let r = p.rows[1].0; // second item, section 0
        p.event(&mut EventContext {
            event: &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: Vec2::new(r.min_x() + 4.0, r.min_y() + 4.0),
            },
            bounds: p.bounds,
            scale: 1.0,
        });
        assert_eq!(p.take_selected(), Some((0, 1)));
        assert_eq!(p.active_at(DeviceKind::Microphone), Some(1));
        assert_eq!(p.take_selected(), None);
    }

    #[test]
    fn builder_active() {
        let p = fixture().active(DeviceKind::Speaker, 1);
        assert_eq!(p.active_at(DeviceKind::Speaker), Some(1));
    }

    #[test]
    fn paint_without_painter() {
        let mut p = fixture().active(DeviceKind::Microphone, 0);
        laid_out(&mut p);
        let mut list = martensite_core::PaintList::default();
        let theme = martensite_theme::Theme::new("test");
        p.paint(&mut PaintContext {
            list: &mut list,
            bounds: p.bounds,
            theme: &theme,
            scale: 1.0,
            text_painter: None,
        });
        assert!(!list.is_empty());
    }
}
