//! `UpdatePrompt` — the non-modal "update available" card (Sparkle /
//! in-app updater idiom): an icon, title + version line, changelog
//! notes, a download progress bar, and Update / Later buttons.
//!
//! The card is display-driven: the host sets [`UpdatePrompt::new`]'s
//! version, optional [`UpdatePrompt::notes`], and a
//! [`UpdatePrompt::progress`] fraction while downloading. Clicks park
//! in [`UpdatePrompt::take_update`] and [`UpdatePrompt::take_later`]
//! for the host to act on.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::update_prompt::UpdatePrompt;
//!
//! let p = UpdatePrompt::new("1.4.0").notes("Faster startup");
//! assert_eq!(p.version, "1.4.0");
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
const ICON_PT: f32 = 30.0;
const GAP_PT: f32 = 10.0;
const BTN_PT_W: f32 = 76.0;
const BTN_PT_H: f32 = 24.0;
const BAR_PT: f32 = 4.0;
const TITLE_PT: f32 = 12.5;
const NOTE_PT: f32 = 11.0;

const FACE: [u8; 4] = [34, 36, 44, 255];
const EDGE: [u8; 4] = [78, 82, 92, 255];
const TEXT: [u8; 4] = [235, 237, 240, 255];
const MUTED_FG: [u8; 4] = [160, 164, 174, 255];
const BTN_FACE: [u8; 4] = [52, 55, 66, 255];
const BAR_BG: [u8; 4] = [60, 63, 72, 255];

/// The update card — see the module docs.
///
/// ```
/// use martensite::widgets::update_prompt::UpdatePrompt;
///
/// assert_eq!(UpdatePrompt::new("2.0").version, "2.0");
/// ```
pub struct UpdatePrompt {
    /// Accessibility label.
    pub label: String,
    /// Card heading.
    pub title: String,
    /// Target version string.
    pub version: String,
    /// Changelog line (empty hides it).
    pub notes: String,
    /// Label on the primary action button.
    pub action_label: String,
    /// Download progress `0.0..=1.0`; `None` hides the bar.
    pub progress: Option<f32>,
    update: bool,
    later: bool,
    update_rect: Rect,
    later_rect: Rect,
    bar_rect: Rect,
    text_painter: Option<SharedTextPainter>,
    bounds: Rect,
    scale: f32,
}

impl std::fmt::Debug for UpdatePrompt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UpdatePrompt")
            .field("version", &self.version)
            .finish()
    }
}

impl UpdatePrompt {
    /// A card announcing `version`.
    ///
    /// ```
    /// use martensite::widgets::update_prompt::UpdatePrompt;
    ///
    /// assert_eq!(UpdatePrompt::new("1.0").version, "1.0");
    /// ```
    pub fn new(version: impl Into<String>) -> Self {
        Self {
            label: "Update available".to_string(),
            title: "Update available".to_string(),
            version: version.into(),
            notes: String::new(),
            action_label: "Update".to_string(),
            progress: None,
            update: false,
            later: false,
            update_rect: Rect::new(0.0, 0.0, 0.0, 0.0),
            later_rect: Rect::new(0.0, 0.0, 0.0, 0.0),
            bar_rect: Rect::new(0.0, 0.0, 0.0, 0.0),
            text_painter: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Card heading.
    ///
    /// ```
    /// use martensite::widgets::update_prompt::UpdatePrompt;
    ///
    /// assert_eq!(UpdatePrompt::new("1.0").title("Restart to finish").title, "Restart to finish");
    /// ```
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    /// Changelog line.
    ///
    /// ```
    /// use martensite::widgets::update_prompt::UpdatePrompt;
    ///
    /// assert_eq!(UpdatePrompt::new("1.0").notes("Bug fixes").notes, "Bug fixes");
    /// ```
    pub fn notes(mut self, notes: impl Into<String>) -> Self {
        self.notes = notes.into();
        self
    }

    /// Primary button label.
    ///
    /// ```
    /// use martensite::widgets::update_prompt::UpdatePrompt;
    ///
    /// assert_eq!(UpdatePrompt::new("1.0").action_label("Restart").action_label, "Restart");
    /// ```
    pub fn action_label(mut self, label: impl Into<String>) -> Self {
        self.action_label = label.into();
        self
    }

    /// Shows the download bar at `0.0..=1.0`.
    ///
    /// ```
    /// use martensite::widgets::update_prompt::UpdatePrompt;
    ///
    /// assert_eq!(UpdatePrompt::new("1.0").progress(0.5).progress, Some(0.5));
    /// ```
    pub fn progress(mut self, fraction: f32) -> Self {
        self.progress = Some(fraction.clamp(0.0, 1.0));
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::update_prompt::UpdatePrompt;
    ///
    /// assert_eq!(UpdatePrompt::new("1.0").label("Update").label, "Update");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Shared text painter.
    ///
    /// ```no_run
    /// use martensite::widgets::update_prompt::UpdatePrompt;
    /// # fn example(p: martensite::text_paint::SharedTextPainter) {
    /// let _p = UpdatePrompt::new("1.0").with_text_painter(p);
    /// # }
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// `true` once when the primary action is clicked.
    ///
    /// ```
    /// use martensite::widgets::update_prompt::UpdatePrompt;
    ///
    /// assert!(!UpdatePrompt::new("1.0").take_update());
    /// ```
    pub fn take_update(&mut self) -> bool {
        std::mem::take(&mut self.update)
    }

    /// `true` once when "Later" is clicked.
    ///
    /// ```
    /// use martensite::widgets::update_prompt::UpdatePrompt;
    ///
    /// assert!(!UpdatePrompt::new("1.0").take_later());
    /// ```
    pub fn take_later(&mut self) -> bool {
        std::mem::take(&mut self.later)
    }
}

fn btn(
    cx: &mut PaintContext,
    painter: Option<&(dyn martensite_core::paint::TextShaper + Send + Sync)>,
    rect: Rect,
    label: &str,
    fs: f32,
    primary: bool,
    s: f32,
) {
    let accent = cx.color(TokenKey::AccentColor, [90, 140, 220, 255]);
    let (face, fg) = if primary {
        (accent, [255, 255, 255, 255])
    } else {
        (BTN_FACE, TEXT)
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
    let w = label.len() as f32 * fs * 0.55;
    let o = Vec2::new(
        rect.min_x() + (rect.width() - w) / 2.0,
        rect.min_y() + rect.height() / 2.0 + fs * 0.35,
    );
    crate::text_paint::paint_label(
        painter,
        cx.list,
        kurbo::Point::new(f64::from(o.x), f64::from(o.y)),
        label,
        fs,
        fg,
    );
}

impl Widget for UpdatePrompt {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let s = cx.scale;
        let h = PAD_PT * 2.0
            + ICON_PT.max(TITLE_PT + 4.0 + NOTE_PT)
            + if self.progress.is_some() {
                GAP_PT + BAR_PT
            } else {
                0.0
            };
        Vec2::new(
            (320.0 * s).min(constraints.max_size.x.max(0.0)),
            (h * s).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(200.0, 56.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        let s = cx.scale;
        let pad = PAD_PT * s;
        let bw = BTN_PT_W * s;
        let bh = BTN_PT_H * s;
        let by = bounds.min_y() + (bounds.height() - bh) / 2.0;
        self.update_rect = Rect::new(bounds.max_x() - pad - bw, by, bw, bh);
        self.later_rect = Rect::new(bounds.max_x() - pad - bw * 2.0 - GAP_PT * s, by, bw, bh);
        self.bar_rect = Rect::new(
            bounds.min_x() + pad,
            bounds.max_y() - pad - BAR_PT * s,
            (bounds.width() - pad * 2.0).max(0.0),
            BAR_PT * s,
        );
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Group);
        node.set_label(self.label.clone());
        node.set_value(format!("version {}", self.version));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if let WidgetEvent::PointerReleased {
            button: PointerButton::Primary,
            position,
        } = cx.event
        {
            if self.update_rect.contains(*position) {
                self.update = true;
                return EventResponse::RequestRepaint;
            }
            if self.later_rect.contains(*position) {
                self.later = true;
                return EventResponse::RequestRepaint;
            }
        }
        EventResponse::Ignored
    }

    fn paint(&self, cx: &mut PaintContext) {
        let s = cx.scale;
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let b = self.bounds;
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
        // Icon circle.
        let ic = ICON_PT * s;
        let ix = b.min_x() + PAD_PT * s;
        let iy = b.min_y() + PAD_PT * s;
        cx.list.push_fill_shape(
            kurbo::Rect::new(
                f64::from(ix),
                f64::from(iy),
                f64::from(ix + ic),
                f64::from(iy + ic),
            ),
            &martensite_core::shape::Shape::ELLIPSE,
            cx.color(TokenKey::AccentColor, [90, 140, 220, 255]),
        );
        // Title + version + notes.
        let tx = ix + ic + GAP_PT * s;
        let text_fg = cx.color(TokenKey::TextColor, TEXT);
        let line = format!("{} — v{}", self.title, self.version);
        crate::text_paint::paint_label(
            painter,
            cx.list,
            kurbo::Point::new(f64::from(tx), f64::from(iy + TITLE_PT * s)),
            &line,
            TITLE_PT * s,
            text_fg,
        );
        if !self.notes.is_empty() {
            crate::text_paint::paint_label(
                painter,
                cx.list,
                kurbo::Point::new(
                    f64::from(tx),
                    f64::from(iy + (TITLE_PT + NOTE_PT + 4.0) * s),
                ),
                &self.notes,
                NOTE_PT * s,
                MUTED_FG,
            );
        }
        // Progress bar.
        if let Some(p) = self.progress {
            let br = self.bar_rect;
            cx.list.push_fill_shape(
                kurbo::Rect::new(
                    f64::from(br.min_x()),
                    f64::from(br.min_y()),
                    f64::from(br.max_x()),
                    f64::from(br.max_y()),
                ),
                &martensite_core::shape::Shape::rounded(2.0 * s),
                BAR_BG,
            );
            cx.list.push_fill_shape(
                kurbo::Rect::new(
                    f64::from(br.min_x()),
                    f64::from(br.min_y()),
                    f64::from(br.min_x() + br.width() * p),
                    f64::from(br.max_y()),
                ),
                &martensite_core::shape::Shape::rounded(2.0 * s),
                cx.color(TokenKey::AccentColor, [90, 140, 220, 255]),
            );
        }
        // Buttons.
        btn(cx, painter, self.later_rect, "Later", NOTE_PT, false, s);
        btn(
            cx,
            painter,
            self.update_rect,
            &self.action_label,
            NOTE_PT,
            true,
            s,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(p: &mut UpdatePrompt) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        p.layout(&mut cx, Rect::new(0.0, 0.0, 340.0, 80.0));
    }

    fn click(p: &mut UpdatePrompt, at: Vec2) {
        p.event(&mut EventContext {
            event: &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: at,
            },
            bounds: p.bounds,
            scale: 1.0,
        });
    }

    #[test]
    fn update_click_flags() {
        let mut p = UpdatePrompt::new("1.2");
        laid_out(&mut p);
        let r = p.update_rect;
        click(
            &mut p,
            Vec2::new(r.min_x() + r.width() / 2.0, r.min_y() + r.height() / 2.0),
        );
        assert!(p.take_update());
        assert!(!p.take_update());
    }

    #[test]
    fn later_click_flags() {
        let mut p = UpdatePrompt::new("1.2");
        laid_out(&mut p);
        let r = p.later_rect;
        click(
            &mut p,
            Vec2::new(r.min_x() + r.width() / 2.0, r.min_y() + r.height() / 2.0),
        );
        assert!(p.take_later());
    }

    #[test]
    fn progress_clamps() {
        assert_eq!(UpdatePrompt::new("1.0").progress(1.5).progress, Some(1.0));
    }

    #[test]
    fn paint_without_painter() {
        let mut p = UpdatePrompt::new("1.2").notes("Fixes").progress(0.4);
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
