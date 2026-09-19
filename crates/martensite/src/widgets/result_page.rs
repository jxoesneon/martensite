//! `ResultPage` widget: a full-area status result with a coloured
//! status icon, title, subtitle, and action buttons (Ant `Result`,
//! `GtkStatusPage` for outcomes).
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::result_page::{ResultPage, ResultStatus};
//!
//! let r = ResultPage::new(ResultStatus::Success)
//!     .title("Purchase complete")
//!     .action("Continue");
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::widget::{LayoutConstraints, LayoutContext, PaintContext, Widget};
use martensite_core::{Rect, TokenKey};

use crate::widgets::button::Button;

/// Status glyph size, logical points.
const ICON_PT: f32 = 56.0;
/// Title font size, logical points.
const TITLE_PT: f32 = 20.0;
/// Subtitle font size, logical points.
const SUB_PT: f32 = 13.0;
/// Vertical gap between elements, logical points.
const GAP_PT: f32 = 10.0;
/// Maximum text width, logical points.
const TEXT_MAX_W_PT: f32 = 380.0;
/// Button metrics, logical points.
const BUTTON_H_PT: f32 = 32.0;
const BUTTON_W_PT: f32 = 120.0;
const BUTTON_GAP_PT: f32 = 12.0;

const TITLE_INK: [u8; 4] = [30, 31, 36, 255];
const SUB_INK: [u8; 4] = [105, 109, 118, 255];

/// The status a [`ResultPage`] communicates — each variant paints a
/// distinct glyph and colour.
///
/// # Examples
///
/// ```
/// use martensite::widgets::result_page::ResultStatus;
///
/// assert_eq!(ResultStatus::Success.glyph(), "✓");
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResultStatus {
    /// Operation succeeded — green check.
    Success,
    /// Non-fatal problem — amber warning.
    Warning,
    /// Operation failed — red cross.
    Error,
    /// Neutral information — blue info dot.
    Info,
    /// HTTP-style 403: access denied.
    Forbidden,
    /// HTTP-style 404: not found.
    NotFound,
    /// HTTP-style 500: server error.
    ServerError,
}

impl ResultStatus {
    /// The glyph painted for this status.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::result_page::ResultStatus;
    ///
    /// assert_eq!(ResultStatus::NotFound.glyph(), "404");
    /// ```
    #[must_use]
    pub fn glyph(self) -> &'static str {
        match self {
            Self::Success => "✓",
            Self::Warning => "!",
            Self::Error => "✗",
            Self::Info => "i",
            Self::Forbidden => "403",
            Self::NotFound => "404",
            Self::ServerError => "500",
        }
    }

    /// The fallback accent colour for this status.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::result_page::ResultStatus;
    ///
    /// assert_eq!(ResultStatus::Success.color()[1], 160);
    /// ```
    #[must_use]
    pub fn color(self) -> [u8; 4] {
        match self {
            Self::Success => [50, 160, 90, 255],
            Self::Warning => [220, 150, 40, 255],
            Self::Error | Self::ServerError => [210, 60, 60, 255],
            Self::Info => [70, 110, 200, 255],
            Self::Forbidden => [220, 150, 40, 255],
            Self::NotFound => [70, 110, 200, 255],
        }
    }
}

/// Which result-page button was activated.
///
/// # Examples
///
/// ```
/// use martensite::widgets::result_page::ResultAction;
///
/// assert_ne!(ResultAction::Primary, ResultAction::Secondary);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResultAction {
    /// The primary action button (first added).
    Primary,
    /// The secondary action button (second added).
    Secondary,
}

/// A status result page: status icon + title + subtitle + up to two
/// action buttons, centred in the allocated area.
///
/// # Examples
///
/// ```
/// use martensite::widgets::result_page::{ResultPage, ResultStatus};
///
/// let r = ResultPage::new(ResultStatus::NotFound)
///     .title("Page not found")
///     .subtitle("The link may be broken.")
///     .action("Back home");
/// assert_eq!(r.status(), ResultStatus::NotFound);
/// ```
pub struct ResultPage {
    /// The communicated status.
    status: ResultStatus,
    /// Headline under the icon.
    title: String,
    /// Optional supporting line.
    subtitle: Option<String>,
    /// Up to two action buttons (primary, secondary).
    actions: Vec<Button>,
    /// Cached bounds from the last layout pass.
    cached_bounds: Rect,
    /// Bounds assigned to each action button.
    action_rects: Vec<Rect>,
    /// Shared shaped-text painter. See [`crate::text_paint`].
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl ResultPage {
    /// Creates a result page for `status`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::result_page::{ResultPage, ResultStatus};
    ///
    /// let r = ResultPage::new(ResultStatus::Error);
    /// assert_eq!(r.status(), ResultStatus::Error);
    /// ```
    #[must_use]
    pub fn new(status: ResultStatus) -> Self {
        Self {
            status,
            title: String::new(),
            subtitle: None,
            actions: Vec::new(),
            cached_bounds: Rect::default(),
            action_rects: Vec::new(),
            text_painter: None,
        }
    }

    /// Sets the headline text.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::result_page::{ResultPage, ResultStatus};
    ///
    /// let r = ResultPage::new(ResultStatus::Success).title("Done");
    /// ```
    #[must_use]
    pub fn title(mut self, text: impl Into<String>) -> Self {
        self.title = text.into();
        self
    }

    /// Sets the supporting line under the title.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::result_page::{ResultPage, ResultStatus};
    ///
    /// let r = ResultPage::new(ResultStatus::Info).subtitle("Details here.");
    /// ```
    #[must_use]
    pub fn subtitle(mut self, text: impl Into<String>) -> Self {
        self.subtitle = Some(text.into());
        self
    }

    /// Adds an action button — the first is the primary, the second
    /// the secondary. More than two are ignored (a result page with
    /// three choices should use a [`crate::widgets::Dialog`]).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::result_page::{ResultPage, ResultStatus};
    ///
    /// let r = ResultPage::new(ResultStatus::Success).action("Continue");
    /// assert_eq!(r.action_count(), 1);
    /// ```
    #[must_use]
    pub fn action(mut self, label: impl Into<String>) -> Self {
        if self.actions.len() < 2 {
            self.actions.push(Button::new(label));
        }
        self
    }

    /// The page's status.
    #[inline]
    #[must_use]
    pub fn status(&self) -> ResultStatus {
        self.status
    }

    /// Number of configured action buttons (0–2).
    #[inline]
    #[must_use]
    pub fn action_count(&self) -> usize {
        self.actions.len()
    }

    /// Drains one activation from the action buttons — `Some` with
    /// which button fired, `None` when neither has since the last
    /// call.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::result_page::{ResultPage, ResultStatus};
    ///
    /// let mut r = ResultPage::new(ResultStatus::Success).action("OK");
    /// assert_eq!(r.take_activated(), None);
    /// ```
    pub fn take_activated(&mut self) -> Option<ResultAction> {
        for (i, button) in self.actions.iter_mut().enumerate() {
            if button.take_activated() {
                return Some(if i == 0 {
                    ResultAction::Primary
                } else {
                    ResultAction::Secondary
                });
            }
        }
        None
    }

    /// Installs a shared shaped-text painter for icon and text.
    #[must_use]
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Total stacked content height in logical points.
    fn content_height_pt(&self) -> f32 {
        let mut h = ICON_PT + GAP_PT + TITLE_PT;
        if self.subtitle.is_some() {
            h += GAP_PT + SUB_PT;
        }
        if !self.actions.is_empty() {
            h += GAP_PT + BUTTON_H_PT;
        }
        h
    }

    /// Paints one horizontally centred text line.
    fn paint_centered(
        &self,
        cx: &mut PaintContext,
        text: &str,
        size_pt: f32,
        y_center: f32,
        ink: [u8; 4],
    ) {
        let b = cx.bounds;
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let size_px = cx.pt(size_pt);
        let w = painter
            .and_then(|p| p.measure_text(text, size_px))
            .unwrap_or(size_px * text.chars().count() as f32 * 0.5);
        let max_w = cx.pt(TEXT_MAX_W_PT).min(b.size.x);
        let x = b.origin.x + (b.size.x - w.min(max_w)) / 2.0;
        let y = y_center - size_px / 2.0;
        crate::text_paint::paint_label_clipped(
            painter,
            cx.list,
            kurbo::Rect::new(
                f64::from(x),
                f64::from(y),
                f64::from(x + max_w),
                f64::from(y + size_px),
            ),
            kurbo::Point::new(f64::from(x), f64::from(y)),
            text,
            size_px,
            ink,
        );
    }
}

impl Widget for ResultPage {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            constraints.max_size.x.max(cx.pt(240.0)),
            constraints.max_size.y.max(cx.pt(140.0)),
        )
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.cached_bounds = bounds;
        self.action_rects.clear();
        let content_h = cx.pt(self.content_height_pt());
        let stack_top = bounds.origin.y + (bounds.size.y - content_h).max(0.0) / 2.0;
        if !self.actions.is_empty() {
            let button_h = cx.pt(BUTTON_H_PT);
            let button_w = cx.pt(BUTTON_W_PT).min(bounds.size.x);
            let gap = cx.pt(BUTTON_GAP_PT);
            let total_w =
                self.actions.len() as f32 * button_w + (self.actions.len() - 1) as f32 * gap;
            let mut x = bounds.origin.x + (bounds.size.x - total_w) / 2.0;
            let y = stack_top + content_h - button_h;
            for button in &mut self.actions {
                let r = Rect::new(x, y, button_w, button_h);
                self.action_rects.push(r);
                cx.layout_child(button, r);
                x += button_w + gap;
            }
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        // Status role: the page announces itself politely — matching
        // `role="status"` semantics for a completed outcome.
        node.set_role(accesskit::Role::Status);
        if self.title.is_empty() {
            node.set_label(match self.status {
                ResultStatus::Success => "Success",
                ResultStatus::Warning => "Warning",
                ResultStatus::Error => "Error",
                ResultStatus::Info => "Information",
                ResultStatus::Forbidden => "Access denied",
                ResultStatus::NotFound => "Not found",
                ResultStatus::ServerError => "Server error",
            });
        } else {
            node.set_label(self.title.as_str());
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        if b.size.x <= 0.0 || b.size.y <= 0.0 {
            return;
        }
        let content_h = cx.pt(self.content_height_pt());
        let mut y_center = b.origin.y + (b.size.y - content_h).max(0.0) / 2.0;

        // Status glyph in its accent colour — the whole page's signal.
        y_center += cx.pt(ICON_PT) / 2.0;
        self.paint_centered(
            cx,
            self.status.glyph(),
            ICON_PT,
            y_center,
            self.status.color(),
        );
        y_center += cx.pt(ICON_PT) / 2.0 + cx.pt(GAP_PT);

        if !self.title.is_empty() {
            y_center += cx.pt(TITLE_PT) / 2.0;
            self.paint_centered(
                cx,
                &self.title,
                TITLE_PT,
                y_center,
                cx.color(TokenKey::TextColor, TITLE_INK),
            );
            y_center += cx.pt(TITLE_PT) / 2.0;
        }

        if let Some(sub) = &self.subtitle {
            y_center += cx.pt(GAP_PT) + cx.pt(SUB_PT) / 2.0;
            self.paint_centered(
                cx,
                sub,
                SUB_PT,
                y_center,
                cx.color(TokenKey::TextMutedColor, SUB_INK),
            );
        }
    }

    fn child_count(&self) -> usize {
        self.actions.len()
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        self.actions.get(index).map(|b| b as &dyn Widget)
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        self.actions.get_mut(index).map(|b| b as &mut dyn Widget)
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        self.action_rects.get(index).copied()
    }
}

impl std::fmt::Debug for ResultPage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResultPage")
            .field("status", &self.status)
            .field("title", &self.title)
            .field("actions", &self.actions.len())
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

    #[test]
    fn status_glyphs_and_colors() {
        assert_eq!(ResultStatus::Success.glyph(), "✓");
        assert_eq!(ResultStatus::NotFound.glyph(), "404");
        assert_ne!(ResultStatus::Success.color(), ResultStatus::Error.color());
    }

    #[test]
    fn builder_caps_actions_at_two() {
        let r = ResultPage::new(ResultStatus::Info)
            .action("A")
            .action("B")
            .action("C");
        assert_eq!(r.action_count(), 2);
    }

    #[test]
    fn actions_get_bounds() {
        let mut r = ResultPage::new(ResultStatus::Success)
            .title("Done")
            .action("OK")
            .action("Cancel");
        let mut hot = HotNode::default();
        r.layout(&mut make_cx(&mut hot), Rect::new(0.0, 0.0, 500.0, 400.0));
        assert_eq!(r.child_count(), 2);
        let a = r.child_bounds(0).unwrap();
        let b = r.child_bounds(1).unwrap();
        assert!(a.size.x > 0.0 && b.origin.x > a.max_x());
    }

    #[test]
    fn take_activated_drains_buttons() {
        let mut r = ResultPage::new(ResultStatus::Error).action("Retry");
        assert_eq!(r.take_activated(), None);
    }

    #[test]
    fn measure_fills_with_floor() {
        let mut r = ResultPage::new(ResultStatus::Success);
        let mut hot = HotNode::default();
        let size = r.measure(
            &mut make_cx(&mut hot),
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(800.0, 600.0),
            },
        );
        assert_eq!(size, Vec2::new(800.0, 600.0));
    }

    #[test]
    fn accessibility_labels_from_title_or_status() {
        let r = ResultPage::new(ResultStatus::Forbidden);
        let mut node = AccessKitNode::new(accesskit::Role::Status);
        r.accessibility(&mut node);
        let r2 = ResultPage::new(ResultStatus::Success).title("Saved");
        r2.accessibility(&mut node);
    }
}
