//! `FileChooserButton` — a file-name button face that requests a
//! chooser (GTK `FileChooserButton`, `NSOpenPanel` well).
//!
//! The face shows a folder/document glyph plus the current file name
//! (or a placeholder); pressing it parks a request in
//! [`FileChooserButton::take_activated`] for the app to mount a
//! platform open/save dialog — the same seam [`crate::widgets::ColorButton`]
//! and [`crate::widgets::FontButton`] use. `Save`-mode faces show a
//! different glyph.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::file_chooser_button::FileChooserButton;
//!
//! let b = FileChooserButton::new().placeholder("Choose a file…");
//! assert!(b.selected_name().is_none());
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

const HEIGHT_PT: f32 = 32.0;
const PAD_PT: f32 = 10.0;
const FONT_PT: f32 = 13.0;
const RADIUS_PT: f32 = 6.0;
const GLYPH_PT: f32 = 14.0;

const SURFACE: [u8; 4] = [55, 55, 58, 255];
const BORDER: [u8; 4] = [90, 90, 90, 255];
const FG: [u8; 4] = [220, 220, 220, 255];
const MUTED: [u8; 4] = [140, 140, 140, 255];
const ACCENT: [u8; 4] = [80, 140, 220, 255];

/// Chooser flavor — open shows a folder glyph, save a document.
///
/// ```
/// use martensite::widgets::file_chooser_button::ChooserMode;
///
/// assert_ne!(ChooserMode::Open, ChooserMode::Save);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChooserMode {
    /// Pick an existing file (folder glyph).
    Open,
    /// Pick a save destination (document glyph).
    Save,
}

/// A file-chooser button — see the module docs.
///
/// ```
/// use martensite::widgets::file_chooser_button::FileChooserButton;
///
/// let mut b = FileChooserButton::new();
/// assert!(!b.take_activated());
/// ```
pub struct FileChooserButton {
    /// Open vs. save glyph/mode.
    pub mode: ChooserMode,
    /// When `false` the button is inert.
    pub enabled: bool,
    file_name: Option<String>,
    placeholder: String,
    activated: bool,
    pressed: bool,
    hot: bool,
    focused: bool,
    bounds: Rect,
    scale: f32,
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl Default for FileChooserButton {
    fn default() -> Self {
        Self::new()
    }
}

impl FileChooserButton {
    /// Creates an open-mode chooser button.
    ///
    /// ```
    /// use martensite::widgets::file_chooser_button::FileChooserButton;
    ///
    /// let b = FileChooserButton::new();
    /// assert_eq!(b.mode, martensite::widgets::file_chooser_button::ChooserMode::Open);
    /// ```
    pub fn new() -> Self {
        Self {
            mode: ChooserMode::Open,
            enabled: true,
            file_name: None,
            placeholder: "Select a file…".to_string(),
            activated: false,
            pressed: false,
            hot: false,
            focused: false,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
            text_painter: None,
        }
    }

    /// Sets the chooser mode.
    ///
    /// ```
    /// use martensite::widgets::file_chooser_button::{
    ///     ChooserMode, FileChooserButton,
    /// };
    ///
    /// let b = FileChooserButton::new().mode(ChooserMode::Save);
    /// assert_eq!(b.mode, ChooserMode::Save);
    /// ```
    pub fn mode(mut self, mode: ChooserMode) -> Self {
        self.mode = mode;
        self
    }

    /// Pre-selects a displayed file name.
    ///
    /// ```
    /// use martensite::widgets::file_chooser_button::FileChooserButton;
    ///
    /// let b = FileChooserButton::new().file_name("report.pdf");
    /// assert_eq!(b.selected_name(), Some("report.pdf"));
    /// ```
    pub fn file_name(mut self, name: impl Into<String>) -> Self {
        self.file_name = Some(name.into());
        self
    }

    /// Text shown before a file is chosen.
    ///
    /// ```
    /// use martensite::widgets::file_chooser_button::FileChooserButton;
    ///
    /// let b = FileChooserButton::new().placeholder("Pick…");
    /// assert_eq!(b.placeholder_text(), "Pick…");
    /// ```
    pub fn placeholder(mut self, text: impl Into<String>) -> Self {
        self.placeholder = text.into();
        self
    }

    /// Enables or disables the button.
    ///
    /// ```
    /// use martensite::widgets::file_chooser_button::FileChooserButton;
    ///
    /// let b = FileChooserButton::new().enabled(false);
    /// assert!(!b.enabled);
    /// ```
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Explicit painter override — see [`crate::text_paint`].
    ///
    /// ```
    /// use martensite::widgets::file_chooser_button::FileChooserButton;
    /// use martensite::text_paint::shared_painter;
    ///
    /// let b = FileChooserButton::new().with_text_painter(shared_painter());
    /// assert!(b.selected_name().is_none());
    /// ```
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// The displayed file name (None before a pick).
    ///
    /// ```
    /// use martensite::widgets::file_chooser_button::FileChooserButton;
    ///
    /// assert_eq!(FileChooserButton::new().selected_name(), None);
    /// ```
    pub fn selected_name(&self) -> Option<&str> {
        self.file_name.as_deref()
    }

    /// The placeholder text.
    ///
    /// ```
    /// use martensite::widgets::file_chooser_button::FileChooserButton;
    ///
    /// assert_eq!(FileChooserButton::new().placeholder_text(), "Select a file…");
    /// ```
    pub fn placeholder_text(&self) -> &str {
        &self.placeholder
    }

    /// Sets the file name after a chooser commit.
    ///
    /// ```
    /// use martensite::widgets::file_chooser_button::FileChooserButton;
    ///
    /// let mut b = FileChooserButton::new();
    /// b.set_file_name(Some("a.txt".to_string()));
    /// assert_eq!(b.selected_name(), Some("a.txt"));
    /// ```
    pub fn set_file_name(&mut self, name: Option<String>) {
        self.file_name = name;
    }

    /// Drains a pending chooser request.
    ///
    /// ```
    /// use martensite::widgets::file_chooser_button::FileChooserButton;
    ///
    /// let mut b = FileChooserButton::new();
    /// assert!(!b.take_activated());
    /// ```
    pub fn take_activated(&mut self) -> bool {
        std::mem::take(&mut self.activated)
    }

    fn face_text(&self) -> &str {
        self.file_name.as_deref().unwrap_or(&self.placeholder)
    }
}

impl Widget for FileChooserButton {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let text_w = self.face_text().chars().count() as f32 * FONT_PT * 0.55 * cx.scale;
        let w = text_w + cx.pt(2.0 * PAD_PT + GLYPH_PT + 6.0);
        Vec2::new(
            w.min(constraints.max_size.x.max(0.0)).max(cx.pt(80.0)),
            cx.pt(HEIGHT_PT).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(80.0, 24.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Button);
        node.set_label(match self.mode {
            ChooserMode::Open => "Choose file",
            ChooserMode::Save => "Choose save location",
        });
        node.set_value(self.face_text().to_string());
        if self.enabled {
            node.add_action(accesskit::Action::Click);
        } else {
            node.set_disabled();
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.enabled {
            return EventResponse::Ignored;
        }
        match cx.event {
            WidgetEvent::PointerMoved { position } => {
                let hot = self.bounds.contains(*position);
                if hot != self.hot {
                    self.hot = hot;
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerLeave => {
                self.hot = false;
                EventResponse::Ignored
            }
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                ..
            } => {
                if self.bounds.contains(*position) {
                    self.pressed = true;
                    return EventResponse::CapturePointer;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position,
            } => {
                if self.pressed {
                    self.pressed = false;
                    if self.bounds.contains(*position) {
                        self.activated = true;
                    }
                    return EventResponse::Handled;
                }
                EventResponse::Ignored
            }
            WidgetEvent::KeyPressed { key, .. } => match key.as_str() {
                "Enter" | "Space" | " " => {
                    self.activated = true;
                    EventResponse::Handled
                }
                _ => EventResponse::Ignored,
            },
            WidgetEvent::FocusGained => {
                self.focused = true;
                EventResponse::RequestRepaint
            }
            WidgetEvent::FocusLost => {
                self.focused = false;
                self.pressed = false;
                EventResponse::RequestRepaint
            }
            WidgetEvent::SemanticAction(martensite_core::widget::SemanticAction::Click) => {
                self.activated = true;
                EventResponse::Handled
            }
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let f = |r: Rect| {
            kurbo::Rect::new(
                f64::from(r.min_x()),
                f64::from(r.min_y()),
                f64::from(r.max_x()),
                f64::from(r.max_y()),
            )
        };
        let surface = cx.color(TokenKey::SurfaceColor, SURFACE);
        let border = cx.color(TokenKey::BorderColor, BORDER);
        let muted = cx.color(TokenKey::TextMutedColor, MUTED);
        let fg = if self.enabled {
            cx.color(TokenKey::TextColor, FG)
        } else {
            muted
        };
        let face = if self.pressed && self.enabled {
            [
                surface[0].saturating_sub(16),
                surface[1].saturating_sub(16),
                surface[2].saturating_sub(16),
                255,
            ]
        } else if self.hot && self.enabled {
            [
                surface[0].saturating_add(10),
                surface[1].saturating_add(10),
                surface[2].saturating_add(10),
                255,
            ]
        } else {
            surface
        };
        let shape = martensite_core::shape::Shape::rounded(cx.pt(RADIUS_PT));
        cx.list.push_fill_shape(f(self.bounds), &shape, face);
        cx.list
            .push_stroke_shape(f(self.bounds), &shape, cx.pt(0.5).max(1.0), border);
        if self.focused {
            let inset = Rect::new(
                self.bounds.min_x() + 1.5,
                self.bounds.min_y() + 1.5,
                (self.bounds.width() - 3.0).max(0.0),
                (self.bounds.height() - 3.0).max(0.0),
            );
            let ring = martensite_core::shape::Shape::rounded(cx.pt(RADIUS_PT - 1.0).max(0.0));
            cx.list.push_stroke_shape(
                f(inset),
                &ring,
                cx.pt(1.5),
                cx.color(TokenKey::AccentColor, ACCENT),
            );
        }

        let pad = cx.pt(PAD_PT);
        let gs = cx.pt(GLYPH_PT);
        let gy = self.bounds.min_y() + (self.bounds.height() - gs) / 2.0;
        let gx = self.bounds.min_x() + pad;
        let glyph_rect = Rect::new(gx, gy, gs, gs);
        let mut p = kurbo::BezPath::new();
        match self.mode {
            ChooserMode::Open => {
                // Folder: tab + body outline.
                let (l, t, r, b) = (
                    f64::from(glyph_rect.min_x()),
                    f64::from(glyph_rect.min_y()),
                    f64::from(glyph_rect.max_x()),
                    f64::from(glyph_rect.max_y()),
                );
                p.move_to((l, t + 2.0));
                p.line_to((l + (r - l) * 0.4, t + 2.0));
                p.line_to((l + (r - l) * 0.5, t + 4.0));
                p.line_to((r, t + 4.0));
                p.line_to((r, b));
                p.line_to((l, b));
                p.close_path();
            }
            ChooserMode::Save => {
                // Document: folded-corner page.
                let (l, t, r, b) = (
                    f64::from(glyph_rect.min_x()),
                    f64::from(glyph_rect.min_y()),
                    f64::from(glyph_rect.max_x()),
                    f64::from(glyph_rect.max_y()),
                );
                p.move_to((l, t));
                p.line_to((r - 4.0, t));
                p.line_to((r, t + 4.0));
                p.line_to((r, b));
                p.line_to((l, b));
                p.close_path();
            }
        }
        cx.list.push_stroke_path(p, cx.pt(1.2), muted);

        // Label — file name in ink, placeholder muted.
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let size = FONT_PT * cx.scale;
        let clip = Rect::new(
            glyph_rect.max_x() + cx.pt(6.0),
            self.bounds.min_y(),
            (self.bounds.max_x() - pad - glyph_rect.max_x() - cx.pt(6.0)).max(0.0),
            self.bounds.height(),
        );
        crate::text_paint::paint_label_clipped(
            painter,
            cx.list,
            f(clip),
            kurbo::Point::new(
                f64::from(clip.min_x()),
                f64::from(self.bounds.min_y() + (self.bounds.height() - size) / 2.0),
            ),
            self.face_text(),
            size,
            if self.file_name.is_some() { fg } else { muted },
        );
    }
}

impl std::fmt::Debug for FileChooserButton {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileChooserButton")
            .field("mode", &self.mode)
            .field("file_name", &self.file_name)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(b: &mut FileChooserButton, w: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        b.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(w, h),
            },
        );
        b.layout(&mut cx, Rect::new(0.0, 0.0, w, h));
    }

    fn click(b: &mut FileChooserButton) {
        for event in [
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(10.0, 10.0),
                count: 1,
            },
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: Vec2::new(10.0, 10.0),
            },
        ] {
            b.event(&mut EventContext {
                event: &event,
                bounds: Rect::new(0.0, 0.0, 240.0, 32.0),
                scale: 1.0,
            });
        }
    }

    #[test]
    fn click_activates() {
        let mut b = FileChooserButton::new();
        laid_out(&mut b, 240.0, 32.0);
        click(&mut b);
        assert!(b.take_activated());
        assert!(!b.take_activated());
    }

    #[test]
    fn release_outside_cancels() {
        let mut b = FileChooserButton::new();
        laid_out(&mut b, 240.0, 32.0);
        b.event(&mut EventContext {
            event: &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(10.0, 10.0),
                count: 1,
            },
            bounds: Rect::new(0.0, 0.0, 240.0, 32.0),
            scale: 1.0,
        });
        b.event(&mut EventContext {
            event: &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: Vec2::new(500.0, 500.0),
            },
            bounds: Rect::new(0.0, 0.0, 240.0, 32.0),
            scale: 1.0,
        });
        assert!(!b.take_activated());
    }

    #[test]
    fn face_text_prefers_file_name() {
        let b = FileChooserButton::new()
            .file_name("a.txt")
            .placeholder("Pick…");
        assert_eq!(b.face_text(), "a.txt");
        let empty = FileChooserButton::new().placeholder("Pick…");
        assert_eq!(empty.face_text(), "Pick…");
    }

    #[test]
    fn set_file_name_updates() {
        let mut b = FileChooserButton::new();
        b.set_file_name(Some("doc.md".to_string()));
        assert_eq!(b.selected_name(), Some("doc.md"));
        b.set_file_name(None);
        assert!(b.selected_name().is_none());
    }

    #[test]
    fn enter_activates() {
        let mut b = FileChooserButton::new();
        laid_out(&mut b, 240.0, 32.0);
        b.event(&mut EventContext {
            event: &WidgetEvent::KeyPressed {
                key: "Enter".to_string(),
                repeat: false,
            },
            bounds: Rect::new(0.0, 0.0, 240.0, 32.0),
            scale: 1.0,
        });
        assert!(b.take_activated());
    }

    #[test]
    fn disabled_inert() {
        let mut b = FileChooserButton::new().enabled(false);
        laid_out(&mut b, 240.0, 32.0);
        click(&mut b);
        assert!(!b.take_activated());
    }

    #[test]
    fn mode_switch() {
        let b = FileChooserButton::new().mode(ChooserMode::Save);
        assert_eq!(b.mode, ChooserMode::Save);
    }
}
