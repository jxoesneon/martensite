//! `IpInput` — an IPv4 dotted-quad entry field (WinForms
//! `IPAddressControl` idiom).
//!
//! Four [`SpinBox`] octets (`0..=255`, wrapping, no decimals) sit
//! side by side with painted dot separators. The composite reads
//! out as `[u8; 4]`; any octet change parks a flag in
//! [`IpInput::take_changed`]. Each octet stays a fully editable
//! spin field — cursor keys, steppers, and typed entry all work —
//! with the IP-control conveniences layered on top:
//!
//! - Typing `.` jumps to the next octet, committing (and clamping)
//!   the departing field — so does reaching three digits.
//! - Committing a dotted quad (`"192.168.1.1"`) or pasting one via
//!   Ctrl+V distributes it across the octets.
//! - `Backspace` in an emptied octet retreats to the previous one.
//! - Entering an octet (Tab, click, or a `.` jump) selects its text
//!   so the next keystroke replaces the field wholesale.
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
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_sanitize::{Sanitize, SanitizerConfig};
use martensite_theme::TokenKey;
use std::sync::Arc;

use crate::text_paint::SharedTextPainter;
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
    /// The sanitization pipeline — propagated to every octet field.
    sanitizer: SanitizerConfig,
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
            sanitizer: SanitizerConfig::default(),
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

    /// Toggles input sanitization — propagated to every octet field;
    /// `true` (the default) runs the aggressive
    /// [`martensite_sanitize`] profile, `false` keeps the structural
    /// floor, [`raw`](Self::raw) disables the engine entirely.
    ///
    /// ```
    /// use martensite::widgets::ip_input::IpInput;
    ///
    /// assert!(IpInput::new().raw().sanitizer_config().is_raw());
    /// ```
    #[inline]
    #[must_use]
    pub fn sanitize(mut self, on: bool) -> Self {
        self.set_sanitizer(if on {
            SanitizerConfig::Aggressive
        } else {
            SanitizerConfig::Baseline
        });
        self
    }

    /// Fully verbatim input in every octet.
    ///
    /// ```
    /// use martensite::widgets::ip_input::IpInput;
    ///
    /// assert!(IpInput::new().raw().sanitizer_config().is_raw());
    /// ```
    #[inline]
    #[must_use]
    pub fn raw(mut self) -> Self {
        self.set_sanitizer(SanitizerConfig::Raw);
        self
    }

    /// Replaces the sanitization pipeline with a caller-supplied
    /// [`Sanitize`] rule, propagated to every octet.
    ///
    /// ```
    /// use martensite::widgets::ip_input::IpInput;
    /// use martensite_sanitize::Profile;
    /// use std::sync::Arc;
    ///
    /// let ip = IpInput::new().with_sanitizer(Arc::new(Profile::baseline()));
    /// assert!(ip.sanitizer_config().is_custom());
    /// ```
    #[inline]
    #[must_use]
    pub fn with_sanitizer(mut self, rule: Arc<dyn Sanitize>) -> Self {
        self.set_sanitizer(SanitizerConfig::Custom(rule));
        self
    }

    /// Replaces the sanitization configuration, propagating it to all
    /// four octet fields.
    #[inline]
    pub fn set_sanitizer(&mut self, config: SanitizerConfig) {
        for octet in &mut self.octets {
            octet.set_sanitizer(config.clone());
        }
        self.sanitizer = config;
    }

    /// The configured sanitization pipeline.
    ///
    /// ```
    /// use martensite::widgets::ip_input::IpInput;
    ///
    /// assert!(!IpInput::new().sanitizer_config().is_raw());
    /// ```
    #[inline]
    pub fn sanitizer_config(&self) -> &SanitizerConfig {
        &self.sanitizer
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

    /// The octet currently holding keyboard focus, if any.
    fn focused_octet(&self) -> Option<usize> {
        self.octets.iter().position(|o| o.focused())
    }

    /// Delivers an event to octet `index` in its own bounds.
    fn deliver(&mut self, index: usize, event: &WidgetEvent) -> EventResponse {
        let mut ecx = EventContext {
            event,
            bounds: self.octet_bounds(index),
            scale: self.scale,
        };
        self.octets[index].event(&mut ecx)
    }

    /// Moves keyboard focus to octet `index`. Departing octets get
    /// `FocusLost`, which commits their pending text (an over-range
    /// field clamps to 255 on the way out); the arriving octet selects
    /// its text so the next digit replaces the field wholesale — the
    /// IP control's enter-to-overwrite idiom. Re-entering an already
    /// focused octet still round-trips `FocusLost`→`FocusGained` so a
    /// pasted segment that lands focus back on the same octet commits.
    fn transfer_focus(&mut self, index: usize) {
        for i in 0..4 {
            if i != index && self.octets[i].focused() {
                self.deliver(i, &WidgetEvent::FocusLost);
            }
        }
        if self.octets[index].focused() {
            self.deliver(index, &WidgetEvent::FocusLost);
        }
        self.deliver(index, &WidgetEvent::FocusGained);
        let select_all = WidgetEvent::KeyPressed {
            key: "SelectAll".to_string(),
            repeat: false,
        };
        self.deliver(index, &select_all);
    }

    /// Distributes a dotted commit (`"192.168.1.1"`, or a lone `"."`)
    /// across consecutive octets starting at `start`. Each segment
    /// lands in its octet's field and commits — clamping above 255 —
    /// as focus moves on. Surplus segments past the fourth are
    /// dropped. Focus lands on the last octet a segment addressed.
    fn distribute(&mut self, start: usize, text: &str) {
        for (k, seg) in text.split('.').enumerate() {
            let j = start + k;
            if j >= 4 {
                break;
            }
            self.transfer_focus(j);
            if !seg.is_empty() {
                let commit = WidgetEvent::ImeCommitted {
                    text: seg.to_string(),
                };
                self.deliver(j, &commit);
            }
        }
        let last = (start + text.matches('.').count()).min(3);
        self.transfer_focus(last);
    }

    /// The standard child-forwarding pass: positional events go to the
    /// octet under the pointer; keyboard/focus/IME events reach every
    /// octet and the unfocused ones gate themselves out. A claimed
    /// primary press defocuses the sibling octets and selects the
    /// newly entered field's text; a press nobody claimed (a dot
    /// separator) defocuses all of them.
    fn forward(&mut self, cx: &mut EventContext) -> EventResponse {
        let (press, first_click) = match cx.event {
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                count,
                ..
            } => (true, *count == 1),
            _ => (false, false),
        };
        let mut response = EventResponse::Ignored;
        for i in (0..4).rev() {
            let b = self.octet_bounds(i);
            let hit = match cx.event {
                WidgetEvent::PointerPressed { position, .. }
                | WidgetEvent::PointerMoved { position }
                | WidgetEvent::PointerReleased { position, .. }
                | WidgetEvent::Scroll { position, .. } => b.contains(*position),
                _ => true, // keyboard/focus events flow to every octet
            };
            if !hit {
                continue;
            }
            let r = self.deliver(i, cx.event);
            if r != EventResponse::Ignored {
                if press {
                    self.broadcast_focus_lost_except(Some(i), cx.scale);
                    if first_click {
                        let select_all = WidgetEvent::KeyPressed {
                            key: "SelectAll".to_string(),
                            repeat: false,
                        };
                        self.deliver(i, &select_all);
                    }
                }
                response = r;
                break;
            }
        }
        if press && response == EventResponse::Ignored {
            self.broadcast_focus_lost_except(None, cx.scale);
        }
        response
    }

    /// Applies the octet-change diff seam after event handling.
    fn finish(&mut self, response: EventResponse) -> EventResponse {
        if self.address() != self.last {
            self.last = self.address();
            self.changed = true;
        }
        response
    }
}

impl Widget for IpInput {
    #[cfg(feature = "devtools-timemachine")]
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }

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
        let event = cx.event;
        let response = match event {
            // Focus-in lands on the octet that last held it — or the
            // first when nothing was focused. Without this the
            // reverse-order forwarding loop would tab into octet 4.
            WidgetEvent::FocusGained => {
                let target = self.focused_octet().unwrap_or(0);
                self.transfer_focus(target);
                EventResponse::RequestRepaint
            }
            // Focus-out broadcasts: every octet commits pending text
            // and drops focus. Forwarding to the first claimer would
            // strand stale focus and uncommitted digits elsewhere.
            WidgetEvent::FocusLost => {
                self.broadcast_focus_lost_except(None, cx.scale);
                EventResponse::RequestRepaint
            }
            WidgetEvent::KeyPressed { key, .. } => {
                let (word, _, base) = crate::widgets::text_input::parse_key_chord(key);
                match base {
                    // The '.' key press itself is dead — the committed
                    // '.' *text* performs the jump below. Swallowing
                    // the key event keeps the pair from double-jumping.
                    "." | "Period" | "Decimal" | "NumpadDecimal"
                        if self.focused_octet().is_some() =>
                    {
                        EventResponse::Handled
                    }
                    // Backspace in an emptied octet retreats to the
                    // previous field — the IP-control convention.
                    "Backspace" => match self.focused_octet() {
                        Some(i) if i > 0 && self.octets[i].text().is_empty() => {
                            self.transfer_focus(i - 1);
                            EventResponse::RequestRepaint
                        }
                        _ => self.forward(cx),
                    },
                    // A dotted clipboard payload distributes across
                    // octets; anything else pastes into the one field.
                    "Paste" | "v" | "V" if word || base == "Paste" => match self.focused_octet() {
                        Some(start) => {
                            let cb = martensite_clipboard::default_platform_clipboard();
                            match cb.get_contents(martensite_clipboard::clipboard::MIME_TEXT_PLAIN)
                            {
                                Some(bytes) => {
                                    let text = String::from_utf8_lossy(&bytes);
                                    if text.contains('.') {
                                        self.distribute(start, &text);
                                        EventResponse::RequestRepaint
                                    } else {
                                        self.forward(cx)
                                    }
                                }
                                None => self.forward(cx),
                            }
                        }
                        None => EventResponse::Ignored,
                    },
                    _ => self.forward(cx),
                }
            }
            // Committed text containing a '.' distributes across
            // octets — typing "." jumps, pasting a quad fills.
            WidgetEvent::ImeCommitted { text } if text.contains('.') => {
                match self.focused_octet() {
                    Some(start) => {
                        self.distribute(start, text);
                        EventResponse::RequestRepaint
                    }
                    None => EventResponse::Ignored,
                }
            }
            WidgetEvent::ImeCommitted { .. } => match self.focused_octet() {
                Some(i) => {
                    let r = self.deliver(i, event);
                    // Three typed digits auto-advance to the next
                    // octet, committing the field on the way out.
                    let digits = self.octets[i]
                        .text()
                        .chars()
                        .filter(|c| c.is_ascii_digit())
                        .count();
                    if r != EventResponse::Ignored && i < 3 && digits >= 3 {
                        self.transfer_focus(i + 1);
                    }
                    r
                }
                None => EventResponse::Ignored,
            },
            _ => self.forward(cx),
        };
        self.finish(response)
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
            crate::text_paint::paint_label_vcenter(
                painter,
                cx.list,
                kurbo::Rect::new(
                    f64::from(gap.min_x()),
                    f64::from(gap.min_y()),
                    f64::from(gap.max_x()),
                    f64::from(gap.min_y() + (gap.height())),
                ),
                f64::from(gap.min_x() + gap.width() / 2.0 - size * 0.15),
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

    fn send(ip: &mut IpInput, event: &WidgetEvent) -> EventResponse {
        let mut cx = EventContext {
            event,
            bounds: ip.bounds,
            scale: 1.0,
        };
        ip.event(&mut cx)
    }

    fn key(name: &str) -> WidgetEvent {
        WidgetEvent::KeyPressed {
            key: name.into(),
            repeat: false,
        }
    }

    fn commit(text: &str) -> WidgetEvent {
        WidgetEvent::ImeCommitted { text: text.into() }
    }

    fn press_octet(ip: &mut IpInput, i: usize) -> EventResponse {
        let b = ip.octet_bounds(i);
        send(
            ip,
            &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                // +5px stays on the text field, clear of the stepper.
                position: Vec2::new(b.min_x() + 5.0, b.min_y() + b.height() / 2.0),
                count: 1,
            },
        )
    }

    fn focused_index(ip: &IpInput) -> Option<usize> {
        ip.octets.iter().position(|o| o.focused())
    }

    #[test]
    fn focus_gained_lands_on_first_octet() {
        let mut ip = IpInput::new();
        laid_out(&mut ip, 240.0, 26.0);
        send(&mut ip, &WidgetEvent::FocusGained);
        assert_eq!(focused_index(&ip), Some(0));
        send(&mut ip, &commit("5"));
        assert_eq!(focused_index(&ip), Some(0));
        send(&mut ip, &WidgetEvent::FocusLost);
        assert_eq!(ip.address(), [5, 0, 0, 0]);
        assert_eq!(focused_index(&ip), None);
        assert!(ip.take_changed());
    }

    #[test]
    fn focus_lost_broadcasts_to_every_octet() {
        let mut ip = IpInput::new();
        laid_out(&mut ip, 240.0, 26.0);
        // Focus octet 0 by click, type without committing, then blur —
        // the pending digits must commit on the way out, not strand.
        press_octet(&mut ip, 0);
        assert_eq!(focused_index(&ip), Some(0));
        send(&mut ip, &commit("77"));
        send(&mut ip, &WidgetEvent::FocusLost);
        assert_eq!(focused_index(&ip), None);
        assert_eq!(ip.address()[0], 77);
    }

    #[test]
    fn click_defocuses_sibling_octets() {
        let mut ip = IpInput::new();
        laid_out(&mut ip, 240.0, 26.0);
        press_octet(&mut ip, 0);
        send(&mut ip, &commit("9")); // uncommitted in octet 0
        press_octet(&mut ip, 2);
        assert_eq!(focused_index(&ip), Some(2));
        // Moving focus committed octet 0's pending text.
        assert_eq!(ip.address()[0], 9);
        assert!(ip.take_changed());
    }

    #[test]
    fn dot_jumps_to_next_octet_and_commits() {
        let mut ip = IpInput::new();
        laid_out(&mut ip, 240.0, 26.0);
        send(&mut ip, &WidgetEvent::FocusGained);
        send(&mut ip, &commit("12")); // replaces the selected "0"
        send(&mut ip, &commit("."));
        assert_eq!(focused_index(&ip), Some(1));
        assert_eq!(ip.address()[0], 12);
        assert!(ip.take_changed());
    }

    #[test]
    fn period_key_press_is_consumed_without_double_jump() {
        let mut ip = IpInput::new();
        laid_out(&mut ip, 240.0, 26.0);
        send(&mut ip, &WidgetEvent::FocusGained);
        // The raw key event precedes the committed text on real
        // backends — only the commit may jump.
        assert_eq!(send(&mut ip, &key(".")), EventResponse::Handled);
        assert_eq!(focused_index(&ip), Some(0));
        send(&mut ip, &commit("."));
        assert_eq!(focused_index(&ip), Some(1));
    }

    #[test]
    fn three_digits_auto_advance() {
        let mut ip = IpInput::new();
        laid_out(&mut ip, 240.0, 26.0);
        send(&mut ip, &WidgetEvent::FocusGained);
        send(&mut ip, &commit("1"));
        send(&mut ip, &commit("9"));
        assert_eq!(focused_index(&ip), Some(0));
        send(&mut ip, &commit("2"));
        assert_eq!(focused_index(&ip), Some(1));
        // The auto-advance committed octet 0.
        assert_eq!(ip.address()[0], 192);
    }

    #[test]
    fn over_range_octet_clamps_on_exit() {
        let mut ip = IpInput::new();
        laid_out(&mut ip, 240.0, 26.0);
        send(&mut ip, &WidgetEvent::FocusGained);
        send(&mut ip, &commit("999")); // 3 digits → advance + clamp
        assert_eq!(focused_index(&ip), Some(1));
        assert_eq!(ip.address()[0], 255);
    }

    #[test]
    fn dotted_commit_distributes_across_octets() {
        let mut ip = IpInput::new();
        laid_out(&mut ip, 240.0, 26.0);
        send(&mut ip, &WidgetEvent::FocusGained);
        send(&mut ip, &commit("192.168.1.1"));
        assert_eq!(ip.address(), [192, 168, 1, 1]);
        assert_eq!(focused_index(&ip), Some(3));
        assert!(ip.take_changed());
    }

    #[test]
    fn dotted_commit_starts_at_focused_octet() {
        let mut ip = IpInput::new().value([0, 0, 0, 0]);
        laid_out(&mut ip, 240.0, 26.0);
        press_octet(&mut ip, 1);
        send(&mut ip, &commit("8.8"));
        assert_eq!(ip.address(), [0, 8, 8, 0]);
        assert_eq!(focused_index(&ip), Some(2));
    }

    #[test]
    fn dotted_commit_overflow_drops_extra_segments() {
        let mut ip = IpInput::new();
        laid_out(&mut ip, 240.0, 26.0);
        send(&mut ip, &WidgetEvent::FocusGained);
        send(&mut ip, &commit("1.2.3.4.5.6"));
        assert_eq!(ip.address(), [1, 2, 3, 4]);
        assert_eq!(focused_index(&ip), Some(3));
    }

    #[test]
    fn leading_zeros_normalize_on_commit() {
        let mut ip = IpInput::new();
        laid_out(&mut ip, 240.0, 26.0);
        send(&mut ip, &WidgetEvent::FocusGained);
        send(&mut ip, &commit("007")); // 3 digits → advance + commit
        assert_eq!(ip.address()[0], 7);
        assert_eq!(ip.octets[0].text(), "7");
    }

    #[test]
    fn backspace_on_empty_octet_retreats() {
        let mut ip = IpInput::new();
        laid_out(&mut ip, 240.0, 26.0);
        press_octet(&mut ip, 1);
        // The entry select-all is replaced by "" — the octet's field
        // is now empty, so Backspace steps back instead of editing.
        send(&mut ip, &commit(""));
        assert_eq!(ip.octets[1].text(), "");
        send(&mut ip, &key("Backspace"));
        assert_eq!(focused_index(&ip), Some(0));
    }

    #[test]
    fn backspace_on_filled_octet_edits_normally() {
        let mut ip = IpInput::new();
        laid_out(&mut ip, 240.0, 26.0);
        press_octet(&mut ip, 1);
        send(&mut ip, &commit("25"));
        send(&mut ip, &key("Backspace"));
        assert_eq!(focused_index(&ip), Some(1));
        assert_eq!(ip.octets[1].text(), "2");
    }

    #[test]
    fn unparseable_octet_restores_on_exit() {
        let mut ip = IpInput::new();
        laid_out(&mut ip, 240.0, 26.0);
        press_octet(&mut ip, 0);
        send(&mut ip, &commit("abc"));
        assert_eq!(ip.octets[0].text(), "abc");
        send(&mut ip, &WidgetEvent::FocusLost);
        assert_eq!(ip.address()[0], 0);
        assert_eq!(ip.octets[0].text(), "0");
    }

    #[test]
    fn scroll_over_octet_steps_value() {
        let mut ip = IpInput::new();
        laid_out(&mut ip, 240.0, 26.0);
        let b = ip.octet_bounds(0);
        send(
            &mut ip,
            &WidgetEvent::Scroll {
                position: Vec2::new(b.min_x() + 5.0, b.min_y() + 5.0),
                delta: Vec2::new(0.0, 30.0),
            },
        );
        assert_eq!(ip.address()[0], 1);
        assert!(ip.take_changed());
    }

    #[test]
    fn editing_ignored_without_octet_focus() {
        let mut ip = IpInput::new();
        laid_out(&mut ip, 240.0, 26.0);
        assert_eq!(send(&mut ip, &commit("5")), EventResponse::Ignored);
        assert_eq!(send(&mut ip, &key(".")), EventResponse::Ignored);
        assert_eq!(ip.address(), [0, 0, 0, 0]);
    }

    #[test]
    fn entering_octet_selects_for_replace() {
        let mut ip = IpInput::new().value([10, 0, 0, 0]);
        laid_out(&mut ip, 240.0, 26.0);
        press_octet(&mut ip, 0);
        // First keystroke replaces the field wholesale — no "100".
        send(&mut ip, &commit("1"));
        send(&mut ip, &commit("9"));
        send(&mut ip, &commit("2")); // 3 digits → advance
        assert_eq!(ip.address()[0], 192);
    }
}
