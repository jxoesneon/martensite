//! `IpInput` — an IPv4 dotted-quad entry field (WinForms
//! `IPAddressControl` idiom).
//!
//! Four [`SpinBox`] octets (`0..=255`, wrapping, no decimals) sit
//! side by side with painted dot separators. The composite reads
//! out as `[u8; 4]`; any octet change parks a flag in
//! [`IpInput::take_changed`]. Each octet stays a fully editable
//! spin field — cursor keys, steppers, and typed entry all work.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::ip_input::IpInput;
//!
//! let ip = IpInput::new().value([192, 168, 1, 1]);
//! assert_eq!(ip.address(), [192, 168, 1, 1]);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget,
};
use martensite_theme::TokenKey;

use crate::text_paint::{paint_label_clipped, SharedTextPainter};
use crate::widgets::SpinBox;

const OCTET_W_PT: f32 = 52.0;
const DOT_W_PT: f32 = 8.0;
const HEIGHT_PT: f32 = 26.0;

const MUTED: [u8; 4] = [140, 140, 148, 255];

/// An IPv4 dotted-quad entry — see the module docs.
///
/// ```
/// use martensite::widgets::ip_input::IpInput;
///
/// assert_eq!(IpInput::new().address(), [0, 0, 0, 0]);
/// ```
pub struct IpInput {
    /// When `false` all octets are inert.
    pub enabled: bool,
    /// Accessibility label.
    pub label: String,
    octets: Vec<SpinBox>,
    last: [u8; 4],
    changed: bool,
    bounds: Rect,
    scale: f32,
    text_painter: Option<SharedTextPainter>,
}

impl Default for IpInput {
    fn default() -> Self {
        Self::new()
    }
}

impl IpInput {
    /// Creates a `0.0.0.0` field.
    ///
    /// ```
    /// use martensite::widgets::ip_input::IpInput;
    ///
    /// assert_eq!(IpInput::new().address(), [0, 0, 0, 0]);
    /// ```
    pub fn new() -> Self {
        Self {
            enabled: true,
            label: "IP address".to_string(),
            octets: (0..4)
                .map(|i| {
                    SpinBox::new()
                        .range(0.0, 255.0)
                        .decimals(0)
                        .wrap(true)
                        .label(format!("octet {}", i + 1))
                })
                .collect(),
            last: [0; 4],
            changed: false,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
            text_painter: None,
        }
    }

    /// Initial address.
    ///
    /// ```
    /// use martensite::widgets::ip_input::IpInput;
    ///
    /// let ip = IpInput::new().value([10, 0, 0, 1]);
    /// assert_eq!(ip.address(), [10, 0, 0, 1]);
    /// ```
    pub fn value(mut self, addr: [u8; 4]) -> Self {
        self.set_value(addr);
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::ip_input::IpInput;
    ///
    /// let ip = IpInput::new().label("Gateway");
    /// assert_eq!(ip.label, "Gateway");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Enables or disables all octets.
    ///
    /// ```
    /// use martensite::widgets::ip_input::IpInput;
    ///
    /// let ip = IpInput::new().enabled(false);
    /// assert!(!ip.enabled);
    /// ```
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        for o in &mut self.octets {
            o.enabled = enabled;
        }
        self
    }

    /// Explicit painter override — see [`crate::text_paint`].
    ///
    /// ```
    /// use martensite::widgets::ip_input::IpInput;
    /// use martensite::text_paint::shared_painter;
    ///
    /// let ip = IpInput::new().with_text_painter(shared_painter());
    /// assert_eq!(ip.address(), [0, 0, 0, 0]);
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        for o in &mut self.octets {
            *o = std::mem::take(o).with_text_painter(painter.clone());
        }
        self.text_painter = Some(painter);
        self
    }

    /// The address as four octets.
    ///
    /// ```
    /// use martensite::widgets::ip_input::IpInput;
    ///
    /// assert_eq!(IpInput::new().value([8, 8, 8, 8]).address(), [8, 8, 8, 8]);
    /// ```
    pub fn address(&self) -> [u8; 4] {
        let mut a = [0u8; 4];
        for (i, o) in self.octets.iter().enumerate() {
            a[i] = o.value().clamp(0.0, 255.0) as u8;
        }
        a
    }

    /// Dotted-quad text (`"192.168.1.1"`).
    ///
    /// ```
    /// use martensite::widgets::ip_input::IpInput;
    ///
    /// assert_eq!(IpInput::new().value([1, 2, 3, 4]).text(), "1.2.3.4");
    /// ```
    pub fn text(&self) -> String {
        let a = self.address();
        format!("{}.{}.{}.{}", a[0], a[1], a[2], a[3])
    }

    /// Sets all octets.
    ///
    /// ```
    /// use martensite::widgets::ip_input::IpInput;
    ///
    /// let mut ip = IpInput::new();
    /// ip.set_value([127, 0, 0, 1]);
    /// assert_eq!(ip.text(), "127.0.0.1");
    /// ```
    pub fn set_value(&mut self, addr: [u8; 4]) {
        for (o, &v) in self.octets.iter_mut().zip(&addr) {
            o.set_value(f64::from(v));
        }
        self.last = addr;
    }

    /// Drains the any-octet-changed flag.
    ///
    /// ```
    /// use martensite::widgets::ip_input::IpInput;
    ///
    /// let mut ip = IpInput::new();
    /// assert!(!ip.take_changed());
    /// ```
    pub fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }

    /// Octet widget `i`'s bounds (for tests and overlays).
    fn octet_bounds(&self, i: usize) -> Rect {
        let octet_w = (self.bounds.width() - 3.0 * self.dot_w()) / 4.0;
        Rect::new(
            self.bounds.min_x() + i as f32 * (octet_w + self.dot_w()),
            self.bounds.min_y(),
            octet_w,
            self.bounds.height(),
        )
    }

    fn dot_w(&self) -> f32 {
        DOT_W_PT * self.scale
    }
}

impl Widget for IpInput {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            cx.pt(OCTET_W_PT * 4.0 + DOT_W_PT * 3.0)
                .min(constraints.max_size.x.max(0.0)),
            cx.pt(HEIGHT_PT).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(120.0, 20.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        for i in 0..4 {
            let b = self.octet_bounds(i);
            self.octets[i].layout(cx, b);
        }
    }

    fn child_count(&self) -> usize {
        4
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        self.octets.get(index).map(|o| o as &dyn Widget)
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        self.octets.get_mut(index).map(|o| o as &mut dyn Widget)
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        (index < 4).then(|| self.octet_bounds(index))
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Group);
        node.set_label(format!("{} — {}", self.label, self.text()));
        if !self.enabled {
            node.set_disabled();
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.enabled {
            return EventResponse::Ignored;
        }
        let mut response = EventResponse::Ignored;
        for i in (0..4).rev() {
            let b = self.octet_bounds(i);
            let hit = match cx.event {
                martensite_core::WidgetEvent::PointerPressed { position, .. }
                | martensite_core::WidgetEvent::PointerMoved { position }
                | martensite_core::WidgetEvent::PointerReleased { position, .. }
                | martensite_core::WidgetEvent::Scroll { position, .. } => b.contains(*position),
                _ => true, // keyboard/focus events flow to every octet
            };
            if !hit {
                continue;
            }
            let mut ecx = EventContext {
                event: cx.event,
                bounds: b,
                scale: cx.scale,
            };
            let r = self.octets[i].event(&mut ecx);
            if r != EventResponse::Ignored {
                response = r;
                break;
            }
        }
        if self.address() != self.last {
            self.last = self.address();
            self.changed = true;
        }
        response
    }

    fn paint(&self, cx: &mut PaintContext) {
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let size = 12.0 * cx.scale;
        let muted = cx.color(TokenKey::TextMutedColor, MUTED);
        for i in 0..3 {
            let gap = Rect::new(
                self.octet_bounds(i).max_x(),
                self.bounds.min_y(),
                self.dot_w(),
                self.bounds.height(),
            );
            paint_label_clipped(
                painter,
                cx.list,
                kurbo::Rect::new(
                    f64::from(gap.min_x()),
                    f64::from(gap.min_y()),
                    f64::from(gap.max_x()),
                    f64::from(gap.max_y()),
                ),
                kurbo::Point::new(
                    f64::from(gap.min_x() + gap.width() / 2.0 - size * 0.15),
                    f64::from(gap.min_y() + (gap.height() - size * 1.2) / 2.0),
                ),
                ".",
                size,
                muted,
            );
        }
    }
}

impl std::fmt::Debug for IpInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IpInput")
            .field("address", &self.text())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::{HotNode, PointerButton, WidgetEvent};

    fn laid_out(ip: &mut IpInput, w: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        ip.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(w, h),
            },
        );
        ip.layout(&mut cx, Rect::new(0.0, 0.0, w, h));
    }

    #[test]
    fn value_sets_octets() {
        let ip = IpInput::new().value([192, 168, 1, 1]);
        assert_eq!(ip.address(), [192, 168, 1, 1]);
        assert_eq!(ip.text(), "192.168.1.1");
    }

    #[test]
    fn child_protocol() {
        let mut ip = IpInput::new();
        laid_out(&mut ip, 240.0, 26.0);
        assert_eq!(ip.child_count(), 4);
        for i in 0..4 {
            assert!(ip.child(i).is_some());
            assert!(ip.child_bounds(i).is_some());
        }
        assert!(ip.child(4).is_none());
    }

    #[test]
    fn octet_bounds_tile() {
        let mut ip = IpInput::new();
        laid_out(&mut ip, 240.0, 26.0);
        let b0 = ip.octet_bounds(0);
        let b1 = ip.octet_bounds(1);
        assert!(b1.min_x() > b0.max_x());
    }

    #[test]
    fn octet_change_parks_flag() {
        let mut ip = IpInput::new();
        laid_out(&mut ip, 240.0, 26.0);
        ip.octets[0].set_value(10.0);
        // The flag lands on the next event pass (the diff seam).
        ip.event(&mut EventContext {
            event: &WidgetEvent::PointerMoved {
                position: Vec2::new(-100.0, -100.0),
            },
            bounds: Rect::new(0.0, 0.0, 240.0, 26.0),
            scale: 1.0,
        });
        assert!(ip.take_changed());
        assert_eq!(ip.address()[0], 10);
    }

    #[test]
    fn disabled_inert() {
        let mut ip = IpInput::new().enabled(false);
        laid_out(&mut ip, 240.0, 26.0);
        ip.event(&mut EventContext {
            event: &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(20.0, 13.0),
                count: 1,
            },
            bounds: Rect::new(0.0, 0.0, 240.0, 26.0),
            scale: 1.0,
        });
        assert!(!ip.take_changed());
    }
}
