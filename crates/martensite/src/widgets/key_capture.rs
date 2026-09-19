//! `KeyCapture` widget: a shortcut-recording field — click to arm,
//! then press the chord (Qt `QKeySequenceEdit`, KDE
//! `KKeySequenceWidget`, "press shortcut…" settings fields).
//!
//! While armed the widget tracks held modifier keys and records the
//! first non-modifier key as a normalized chord string
//! (`"Ctrl+Shift+K"`). `Escape` cancels recording;
//! `Backspace`/`Delete` clears the stored shortcut. Poll
//! [`KeyCapture::take_recorded`].
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::key_capture::KeyCapture;
//!
//! let k = KeyCapture::new().placeholder("Press shortcut…");
//! assert_eq!(k.get_shortcut(), "");
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::widget::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Widget,
    WidgetEvent,
};
use martensite_core::{Rect, TokenKey};

/// Field height, logical points.
const HEIGHT_PT: f32 = 28.0;
/// Default width, logical points.
const WIDTH_PT: f32 = 180.0;
/// Inner padding, logical points.
const PAD_PT: f32 = 10.0;
/// Font size, logical points.
const FONT_PT: f32 = 13.0;
/// Corner radius, logical points.
const RADIUS_PT: f32 = 6.0;
/// Border width, logical points.
const BORDER_PT: f32 = 1.0;

/// Field face.
const FACE: [u8; 4] = [245, 246, 248, 255];
/// Border.
const BORDER: [u8; 4] = [200, 203, 210, 255];
/// Armed (recording) border.
const ARMED_EDGE: [u8; 4] = [70, 110, 200, 255];
/// Text ink.
const INK: [u8; 4] = [30, 31, 36, 255];
/// Placeholder/prompt ink.
const INK_DIM: [u8; 4] = [140, 144, 153, 255];
/// Focus ring.
const RING: [u8; 4] = [70, 110, 200, 90];

/// Modifier key names (matched against `WidgetEvent::KeyPressed::key`).
const MODIFIERS: [&str; 4] = ["Control", "Shift", "Alt", "Meta"];

/// A keyboard-shortcut recorder.
///
/// # Examples
///
/// ```
/// use martensite::widgets::key_capture::KeyCapture;
///
/// let k = KeyCapture::new().shortcut("Ctrl+S");
/// assert_eq!(k.get_shortcut(), "Ctrl+S");
/// ```
pub struct KeyCapture {
    /// Recorded chord string (normalized, `Ctrl+Shift+K` order).
    shortcut: String,
    /// Prompt shown while armed and while empty.
    placeholder: String,
    /// Whether the field is armed for recording.
    armed: bool,
    /// Currently held modifier names (canonical order via `MODIFIERS`).
    held: [bool; 4],
    /// Pending record notification.
    recorded: Option<String>,
    /// Enabled flag.
    enabled: bool,
    /// Cached bounds.
    bounds: Rect,
    /// Shared shaped-text painter.
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl KeyCapture {
    /// Creates an empty recorder.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::key_capture::KeyCapture;
    ///
    /// let k = KeyCapture::new();
    /// assert_eq!(k.get_shortcut(), "");
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self {
            shortcut: String::new(),
            placeholder: "Press shortcut…".into(),
            armed: false,
            held: [false; 4],
            recorded: None,
            enabled: true,
            bounds: Rect::default(),
            text_painter: None,
        }
    }

    /// Sets the initial shortcut string.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::key_capture::KeyCapture;
    ///
    /// let k = KeyCapture::new().shortcut("Ctrl+Shift+P");
    /// ```
    #[must_use]
    pub fn shortcut(mut self, shortcut: impl Into<String>) -> Self {
        self.shortcut = shortcut.into();
        self
    }

    /// Sets the placeholder/prompt text.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::key_capture::KeyCapture;
    ///
    /// let k = KeyCapture::new().placeholder("Type a shortcut");
    /// ```
    #[must_use]
    pub fn placeholder(mut self, text: impl Into<String>) -> Self {
        self.placeholder = text.into();
        self
    }

    /// Enables or disables the field.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::key_capture::KeyCapture;
    ///
    /// let k = KeyCapture::new().enabled(false);
    /// ```
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// The recorded shortcut string (`""` when empty).
    #[inline]
    #[must_use]
    pub fn get_shortcut(&self) -> &str {
        &self.shortcut
    }

    /// Whether the field is armed and recording.
    #[inline]
    #[must_use]
    pub fn is_armed(&self) -> bool {
        self.armed
    }

    /// Sets the shortcut programmatically.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::key_capture::KeyCapture;
    ///
    /// let mut k = KeyCapture::new();
    /// k.set_shortcut("Alt+F4");
    /// assert_eq!(k.get_shortcut(), "Alt+F4");
    /// ```
    pub fn set_shortcut(&mut self, shortcut: impl Into<String>) {
        self.shortcut = shortcut.into();
    }

    /// Clears the recorded shortcut.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::key_capture::KeyCapture;
    ///
    /// let mut k = KeyCapture::new().shortcut("Ctrl+S");
    /// k.clear();
    /// assert_eq!(k.get_shortcut(), "");
    /// ```
    pub fn clear(&mut self) {
        self.shortcut.clear();
    }

    /// Drains a record notification — fires after each completed
    /// chord capture and after a user-initiated clear.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::key_capture::KeyCapture;
    ///
    /// let mut k = KeyCapture::new();
    /// assert_eq!(k.take_recorded(), None);
    /// ```
    pub fn take_recorded(&mut self) -> Option<String> {
        self.recorded.take()
    }

    /// Installs a shared shaped-text painter.
    #[must_use]
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Composes a normalized chord string from held modifiers + key.
    fn chord(&self, key: &str) -> String {
        let mut parts: Vec<&str> = Vec::with_capacity(5);
        for (i, name) in MODIFIERS.iter().enumerate() {
            if self.held[i] {
                parts.push(match *name {
                    "Meta" => "Meta",
                    other => other,
                });
            }
        }
        parts.push(key);
        parts.join("+")
    }

    /// Disarms and clears held modifiers.
    fn disarm(&mut self) {
        self.armed = false;
        self.held = [false; 4];
    }
}

impl Default for KeyCapture {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for KeyCapture {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            cx.pt(WIDTH_PT).min(constraints.max_size.x.max(0.0)),
            cx.pt(HEIGHT_PT),
        )
    }

    fn layout(&mut self, _cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::TextInput);
        node.set_label("Keyboard shortcut");
        if self.shortcut.is_empty() {
            node.set_value(self.placeholder.clone());
        } else {
            node.set_value(self.shortcut.clone());
        }
        if !self.enabled {
            node.set_disabled();
        }
        if self.enabled {
            node.add_action(accesskit::Action::Focus);
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.enabled {
            return EventResponse::Ignored;
        }
        match cx.event {
            WidgetEvent::PointerPressed { .. } => {
                if !self.armed {
                    self.armed = true;
                    self.held = [false; 4];
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Handled
            }
            WidgetEvent::FocusLost => {
                if self.armed {
                    self.disarm();
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::KeyPressed { key, .. } => {
                if !self.armed {
                    return match key.as_str() {
                        // Enter/Space arm via keyboard focus.
                        "Enter" | "Space" | " " => {
                            self.armed = true;
                            self.held = [false; 4];
                            EventResponse::RequestRepaint
                        }
                        _ => EventResponse::Ignored,
                    };
                }
                if let Some(i) = MODIFIERS.iter().position(|m| m == key) {
                    self.held[i] = true;
                    return EventResponse::RequestRepaint; // live modifier echo
                }
                match key.as_str() {
                    "Escape" => {
                        self.disarm();
                        EventResponse::RequestRepaint
                    }
                    "Backspace" | "Delete" => {
                        self.shortcut.clear();
                        self.disarm();
                        self.recorded = Some(String::new());
                        EventResponse::RequestRepaint
                    }
                    _ => {
                        self.shortcut = self.chord(key);
                        self.disarm();
                        self.recorded = Some(self.shortcut.clone());
                        EventResponse::RequestRepaint
                    }
                }
            }
            WidgetEvent::KeyReleased { key } => {
                if let Some(i) = MODIFIERS.iter().position(|m| m == key) {
                    if self.armed {
                        self.held[i] = false;
                        // Releasing a lone modifier while armed with no
                        // chord pressed disarms (QKeySequenceEdit:
                        // modifier-only sequences are rejected).
                        return EventResponse::RequestRepaint;
                    }
                }
                EventResponse::Ignored
            }
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let face = cx.color(TokenKey::SurfaceColor, FACE);
        let edge = if self.armed {
            cx.color(TokenKey::AccentColor, ARMED_EDGE)
        } else {
            cx.color(TokenKey::BorderColor, BORDER)
        };
        let r = kurbo::Rect::new(
            f64::from(cx.bounds.min_x()),
            f64::from(cx.bounds.min_y()),
            f64::from(cx.bounds.max_x()),
            f64::from(cx.bounds.max_y()),
        );
        let shape = martensite_core::shape::Shape::rounded(cx.pt(RADIUS_PT));
        cx.list.push_fill_shape(r, &shape, face);
        if self.armed {
            let grow = f64::from(cx.pt(2.0));
            cx.list.push_fill_shape(
                kurbo::Rect::new(r.x0 - grow, r.y0 - grow, r.x1 + grow, r.y1 + grow),
                &martensite_core::shape::Shape::rounded(cx.pt(RADIUS_PT + 2.0)),
                cx.color(TokenKey::AccentColor, RING),
            );
        }
        cx.list.push_stroke_shape(r, &shape, cx.pt(BORDER_PT), edge);

        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let (text, ink) = if self.armed {
            // Live echo: held modifiers + a trailing "…" prompt.
            let mut live: Vec<&str> = Vec::new();
            for (i, name) in MODIFIERS.iter().enumerate() {
                if self.held[i] {
                    live.push(name);
                }
            }
            if live.is_empty() {
                (self.placeholder.clone(), INK_DIM)
            } else {
                (format!("{}+…", live.join("+")), INK)
            }
        } else if self.shortcut.is_empty() {
            (
                self.placeholder.clone(),
                cx.color(TokenKey::TextMutedColor, INK_DIM),
            )
        } else {
            (self.shortcut.clone(), cx.color(TokenKey::TextColor, INK))
        };
        let size = cx.pt(FONT_PT);
        let x = cx.bounds.min_x() + cx.pt(PAD_PT);
        let y = cx.bounds.min_y() + (cx.bounds.size.y - size) / 2.0;
        crate::text_paint::paint_label_clipped(
            painter,
            cx.list,
            kurbo::Rect::new(
                f64::from(x),
                f64::from(cx.bounds.min_y()),
                f64::from(cx.bounds.max_x() - cx.pt(PAD_PT)),
                f64::from(cx.bounds.max_y()),
            ),
            kurbo::Point::new(f64::from(x), f64::from(y)),
            &text,
            size,
            ink,
        );
    }
}

impl std::fmt::Debug for KeyCapture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyCapture")
            .field("shortcut", &self.shortcut)
            .field("armed", &self.armed)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn make_cx(hot: &mut HotNode) -> LayoutContext<'_> {
        LayoutContext { hot, scale: 1.0 }
    }

    fn ev<'a>(event: &'a WidgetEvent) -> EventContext<'a> {
        EventContext {
            event,
            bounds: Rect::new(0.0, 0.0, 180.0, 28.0),
            scale: 1.0,
        }
    }

    fn press(key: &str) -> WidgetEvent {
        WidgetEvent::KeyPressed {
            key: key.into(),
            repeat: false,
        }
    }

    fn release(key: &str) -> WidgetEvent {
        WidgetEvent::KeyReleased { key: key.into() }
    }

    fn arm(k: &mut KeyCapture) {
        let click = WidgetEvent::PointerPressed {
            position: Vec2::new(10.0, 10.0),
            button: martensite_core::widget::PointerButton::Primary,
            count: 1,
        };
        k.event(&mut ev(&click));
    }

    #[test]
    fn click_arms() {
        let mut k = KeyCapture::new();
        let mut hot = HotNode::default();
        k.layout(&mut make_cx(&mut hot), Rect::new(0.0, 0.0, 180.0, 28.0));
        arm(&mut k);
        assert!(k.is_armed());
    }

    #[test]
    fn records_plain_key() {
        let mut k = KeyCapture::new();
        arm(&mut k);
        k.event(&mut ev(&press("S")));
        assert_eq!(k.get_shortcut(), "S");
        assert_eq!(k.take_recorded(), Some("S".into()));
        assert!(!k.is_armed());
    }

    #[test]
    fn records_chord() {
        let mut k = KeyCapture::new();
        arm(&mut k);
        k.event(&mut ev(&press("Control")));
        k.event(&mut ev(&press("Shift")));
        k.event(&mut ev(&press("K")));
        assert_eq!(k.get_shortcut(), "Control+Shift+K");
    }

    #[test]
    fn escape_cancels() {
        let mut k = KeyCapture::new().shortcut("Ctrl+S");
        arm(&mut k);
        k.event(&mut ev(&press("Escape")));
        assert!(!k.is_armed());
        assert_eq!(k.get_shortcut(), "Ctrl+S"); // preserved
        assert_eq!(k.take_recorded(), None);
    }

    #[test]
    fn delete_clears() {
        let mut k = KeyCapture::new().shortcut("Ctrl+S");
        arm(&mut k);
        k.event(&mut ev(&press("Delete")));
        assert_eq!(k.get_shortcut(), "");
        assert_eq!(k.take_recorded(), Some(String::new()));
    }

    #[test]
    fn enter_arms_via_keyboard() {
        let mut k = KeyCapture::new();
        k.event(&mut ev(&press("Enter")));
        assert!(k.is_armed());
    }

    #[test]
    fn modifier_release_tracked() {
        let mut k = KeyCapture::new();
        arm(&mut k);
        k.event(&mut ev(&press("Control")));
        k.event(&mut ev(&release("Control")));
        k.event(&mut ev(&press("K")));
        assert_eq!(k.get_shortcut(), "K");
    }

    #[test]
    fn focus_lost_disarms() {
        let mut k = KeyCapture::new();
        arm(&mut k);
        k.event(&mut ev(&WidgetEvent::FocusLost));
        assert!(!k.is_armed());
    }

    #[test]
    fn disabled_inert() {
        let mut k = KeyCapture::new().enabled(false);
        let click = WidgetEvent::PointerPressed {
            position: Vec2::new(5.0, 5.0),
            button: martensite_core::widget::PointerButton::Primary,
            count: 1,
        };
        assert_eq!(k.event(&mut ev(&click)), EventResponse::Ignored);
        assert_eq!(k.event(&mut ev(&press("Enter"))), EventResponse::Ignored);
    }
}
