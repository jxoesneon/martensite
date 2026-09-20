//! Wizard — a multi-step flow: `Steps` header, one visible page, and
//! a back/next/cancel footer.
//!
//! Mirrors `QWizard`, WinUI's setup flows, and Ant `Steps` + form
//! composition. Pages are real widget children — only the current
//! page is laid out and receives input. `next` on the final step
//! parks a finish request; the app reads it via
//! [`take_finished`](crate::widgets::wizard::Wizard::take_finished).
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::{Text, Wizard};
//!
//! let w = Wizard::new()
//!     .step("Account", Text::new("page 1"))
//!     .step("Confirm", Text::new("page 2"));
//! assert_eq!(w.step_count(), 2);
//! assert_eq!(w.current(), 0);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::widgets::{Button, Steps};

/// Steps header height in points.
const HEADER_PT: f32 = 56.0;
/// Footer strip height in points.
const FOOTER_PT: f32 = 44.0;
/// Button dimensions in points.
const BUTTON_W_PT: f32 = 84.0;
/// Button height in points.
const BUTTON_H_PT: f32 = 28.0;
/// Footer padding/gap in points.
const PAD_PT: f32 = 10.0;

/// One wizard page — wraps user content so it is emitted as a
/// `Role::Group` child.
struct WizardPage {
    /// The page content.
    content: Box<dyn Widget>,
    /// Whether this page is current (visible + interactive).
    shown: bool,
    /// Page bounds from the last layout pass.
    bounds: Option<Rect>,
}

impl Widget for WizardPage {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        self.content.measure(cx, constraints)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = Some(bounds);
        cx.layout_child(self.content.as_mut(), bounds);
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Group);
        if !self.shown {
            node.set_hidden();
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.shown {
            return EventResponse::Ignored;
        }
        self.content.event(cx)
    }

    fn child_count(&self) -> usize {
        1
    }

    fn child(&self, _index: usize) -> Option<&dyn Widget> {
        Some(&*self.content)
    }

    fn child_mut(&mut self, _index: usize) -> Option<&mut dyn Widget> {
        Some(&mut *self.content)
    }

    fn child_bounds(&self, _index: usize) -> Option<Rect> {
        // Hidden pages drop out of paint and hit-testing entirely —
        // the `PanelSet` convention.
        if self.shown {
            self.bounds
        } else {
            None
        }
    }
}

/// A multi-step flow container.
///
/// # Examples
///
/// ```
/// use martensite::widgets::{Text, Wizard};
///
/// let w = Wizard::new()
///     .step("a", Text::new("p"))
///     .step("b", Text::new("q"));
/// assert!(w.has_next());
/// ```
pub struct Wizard {
    /// Optional accessible label.
    pub label: Option<String>,
    /// Whether the flow accepts input.
    pub enabled: bool,
    /// Whether a cancel affordance appears in the footer.
    pub cancelable: bool,
    /// Text of the final-step button (default `"Finish"`).
    pub finish_text: String,
    /// The steps header (internal child 0).
    steps: Steps,
    /// Step titles — the `Steps` widget is rebuilt from these as
    /// steps are appended (it has no single-step append API).
    step_titles: Vec<String>,
    /// Pages (internal children 1..=n).
    pages: Vec<WizardPage>,
    /// Footer buttons: back (child n+1), next/finish (n+2), cancel
    /// (n+3, present only when `cancelable`).
    back_button: Button,
    /// Next/finish button.
    next_button: Button,
    /// Optional cancel button.
    cancel_button: Button,
    /// Current page index.
    current: usize,
    /// Parked finish request.
    finished: bool,
    /// Parked cancel request.
    cancelled: bool,
    /// Cached bounds from the last layout pass.
    bounds: Rect,
    /// Header bounds from the last layout pass.
    header_bounds: Option<Rect>,
    /// Content region bounds from the last layout pass.
    content_bounds: Option<Rect>,
    /// Footer bounds from the last layout pass.
    footer_bounds: Option<Rect>,
    /// Back button bounds from the last layout pass.
    back_bounds: Option<Rect>,
    /// Next/finish button bounds from the last layout pass.
    next_bounds: Option<Rect>,
    /// Cancel button bounds from the last layout pass.
    cancel_bounds: Option<Rect>,
}

impl Wizard {
    /// Creates an empty wizard.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Wizard;
    ///
    /// assert_eq!(Wizard::new().step_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: None,
            enabled: true,
            cancelable: true,
            finish_text: "Finish".to_string(),
            steps: Steps::new(),
            step_titles: Vec::new(),
            pages: Vec::new(),
            back_button: Button::new("Back"),
            next_button: Button::new("Next"),
            cancel_button: Button::new("Cancel"),
            current: 0,
            finished: false,
            cancelled: false,
            bounds: Rect::default(),
            header_bounds: None,
            content_bounds: None,
            footer_bounds: None,
            back_bounds: None,
            next_bounds: None,
            cancel_bounds: None,
        }
    }

    /// Appends a step `title` with page `content`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Text, Wizard};
    ///
    /// let w = Wizard::new().step("a", Text::new("p")).step("b", Text::new("q"));
    /// assert_eq!(w.step_count(), 2);
    /// ```
    #[must_use]
    pub fn step(mut self, title: impl Into<String>, content: impl Widget + 'static) -> Self {
        self.step_titles.push(title.into());
        self.steps = Steps::new().steps(self.step_titles.iter().cloned());
        self.pages.push(WizardPage {
            content: Box::new(content),
            shown: self.pages.is_empty(),
            bounds: None,
        });
        self.sync_buttons();
        self
    }

    /// Sets whether a cancel affordance appears.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Wizard;
    ///
    /// let w = Wizard::new().cancelable(false);
    /// assert!(!w.cancelable);
    /// ```
    #[must_use]
    pub fn cancelable(mut self, cancelable: bool) -> Self {
        self.cancelable = cancelable;
        self
    }

    /// Sets the final-step button text.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Wizard;
    ///
    /// let w = Wizard::new().finish_text("Deploy");
    /// assert_eq!(w.finish_text, "Deploy");
    /// ```
    #[must_use]
    pub fn finish_text(mut self, text: impl Into<String>) -> Self {
        self.finish_text = text.into();
        self.sync_buttons();
        self
    }

    /// Sets the accessible label.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Wizard;
    ///
    /// let w = Wizard::new().label("Setup");
    /// assert_eq!(w.label.as_deref(), Some("Setup"));
    /// ```
    #[must_use]
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Sets whether the flow is enabled.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Wizard;
    ///
    /// let w = Wizard::new().enabled(false);
    /// assert!(!w.enabled);
    /// ```
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self.sync_buttons();
        self
    }

    /// Number of steps.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Text, Wizard};
    ///
    /// assert_eq!(Wizard::new().step("a", Text::new("p")).step_count(), 1);
    /// ```
    #[inline]
    pub fn step_count(&self) -> usize {
        self.pages.len()
    }

    /// Current page index.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Text, Wizard};
    ///
    /// assert_eq!(Wizard::new().step("a", Text::new("p")).current(), 0);
    /// ```
    #[inline]
    pub fn current(&self) -> usize {
        self.current
    }

    /// Whether a next page exists.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Text, Wizard};
    ///
    /// let w = Wizard::new().step("a", Text::new("p")).step("b", Text::new("q"));
    /// assert!(w.has_next());
    /// ```
    #[inline]
    pub fn has_next(&self) -> bool {
        self.current + 1 < self.pages.len()
    }

    /// Whether a previous page exists.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Text, Wizard};
    ///
    /// let w = Wizard::new().step("a", Text::new("p"));
    /// assert!(!w.has_back());
    /// ```
    #[inline]
    pub fn has_back(&self) -> bool {
        self.current > 0
    }

    /// Advances to the next page, or parks a finish request on the
    /// last page.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Text, Wizard};
    ///
    /// let mut w = Wizard::new().step("a", Text::new("p")).step("b", Text::new("q"));
    /// w.go_next();
    /// assert_eq!(w.current(), 1);
    /// w.go_next();
    /// assert!(w.take_finished());
    /// ```
    pub fn go_next(&mut self) {
        if self.has_next() {
            self.current += 1;
            self.sync_pages();
        } else if !self.pages.is_empty() {
            self.finished = true;
        }
        self.sync_buttons();
    }

    /// Returns to the previous page.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Text, Wizard};
    ///
    /// let mut w = Wizard::new().step("a", Text::new("p")).step("b", Text::new("q"));
    /// w.go_next();
    /// w.go_back();
    /// assert_eq!(w.current(), 0);
    /// ```
    pub fn go_back(&mut self) {
        if self.has_back() {
            self.current -= 1;
            self.sync_pages();
        }
        self.sync_buttons();
    }

    /// Jumps to `index` (clamped).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Text, Wizard};
    ///
    /// let mut w = Wizard::new().step("a", Text::new("p")).step("b", Text::new("q"));
    /// w.go_to(1);
    /// assert_eq!(w.current(), 1);
    /// ```
    pub fn go_to(&mut self, index: usize) {
        if self.pages.is_empty() {
            return;
        }
        self.current = index.min(self.pages.len() - 1);
        self.sync_pages();
        self.sync_buttons();
    }

    /// Takes the parked finish request.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Wizard;
    ///
    /// assert!(!Wizard::new().take_finished());
    /// ```
    #[inline]
    pub fn take_finished(&mut self) -> bool {
        std::mem::take(&mut self.finished)
    }

    /// Takes the parked cancel request.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Wizard;
    ///
    /// assert!(!Wizard::new().take_cancelled());
    /// ```
    #[inline]
    pub fn take_cancelled(&mut self) -> bool {
        std::mem::take(&mut self.cancelled)
    }

    /// Mirrors current/enabled onto steps, pages, and buttons.
    fn sync_pages(&mut self) {
        self.steps.set_current(self.current);
        for (i, page) in self.pages.iter_mut().enumerate() {
            page.shown = i == self.current;
        }
    }

    /// Mirrors enabled/position onto the footer buttons.
    fn sync_buttons(&mut self) {
        self.back_button.enabled = self.enabled && self.has_back();
        self.next_button.enabled = self.enabled && !self.pages.is_empty();
        self.next_button.label = if self.has_next() || self.pages.is_empty() {
            "Next".to_string()
        } else {
            self.finish_text.clone()
        };
        self.cancel_button.enabled = self.enabled;
    }

    /// Total internal child count: steps + pages + 2 or 3 buttons.
    fn total_children(&self) -> usize {
        1 + self.pages.len() + if self.cancelable { 3 } else { 2 }
    }

    /// Whether `index` addresses the steps header.
    fn is_steps_index(index: usize) -> bool {
        index == 0
    }

    /// Maps a child index to the page index, if it addresses a page.
    fn page_index(&self, index: usize) -> Option<usize> {
        (index >= 1 && index <= self.pages.len()).then_some(index - 1)
    }

    /// Child index of the back button.
    fn back_index(&self) -> usize {
        1 + self.pages.len()
    }

    /// Child index of the next/finish button.
    fn next_index(&self) -> usize {
        2 + self.pages.len()
    }

    /// Child index of the cancel button (only when `cancelable`).
    fn cancel_index(&self) -> usize {
        3 + self.pages.len()
    }

    /// Drains footer button activations into navigation.
    fn poll_buttons(&mut self) {
        if self.back_button.take_activated() {
            self.go_back();
        }
        if self.next_button.take_activated() {
            self.go_next();
        }
        if self.cancelable && self.cancel_button.take_activated() {
            self.cancelled = true;
        }
    }
}

impl Default for Wizard {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for Wizard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Wizard")
            .field("steps", &self.pages.len())
            .field("current", &self.current)
            .field("enabled", &self.enabled)
            .finish()
    }
}

impl Widget for Wizard {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let chrome = cx.pt(HEADER_PT + FOOTER_PT);
        let max_w = constraints.max_size.x.max(0.0);
        let max_h = constraints.max_size.y.max(0.0);
        Vec2::new(
            cx.pt(320.0).min(max_w).max(cx.pt(160.0).min(max_w)),
            (chrome + cx.pt(120.0)).min(max_h).max(chrome.min(max_h)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(200.0, 160.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        let header = Rect::new(
            bounds.min_x(),
            bounds.min_y(),
            bounds.width(),
            cx.pt(HEADER_PT).min(bounds.height()),
        );
        let footer = Rect::new(
            bounds.min_x(),
            (bounds.max_y() - cx.pt(FOOTER_PT)).max(header.max_y()),
            bounds.width(),
            (bounds.height() - header.height()).min(cx.pt(FOOTER_PT)),
        );
        let content = Rect::new(
            bounds.min_x(),
            header.max_y(),
            bounds.width(),
            (footer.min_y() - header.max_y()).max(0.0),
        );
        self.header_bounds = Some(header);
        self.content_bounds = Some(content);
        self.footer_bounds = Some(footer);
        cx.layout_child(&mut self.steps, header);
        for page in self.pages.iter_mut() {
            cx.layout_child(page, content);
        }
        // Footer buttons right-aligned: [Cancel] [Back] [Next].
        let bh = cx.pt(BUTTON_H_PT).min(footer.height());
        let bw = cx.pt(BUTTON_W_PT);
        let pad = cx.pt(PAD_PT);
        let by = footer.min_y() + (footer.height() - bh) / 2.0;
        let mut bx = footer.max_x() - pad - bw;
        let next_r = Rect::new(bx, by, bw, bh);
        bx -= bw + pad;
        let back_r = Rect::new(bx, by, bw, bh);
        bx -= bw + pad;
        self.next_bounds = Some(next_r);
        self.back_bounds = Some(back_r);
        cx.layout_child(&mut self.next_button, next_r);
        cx.layout_child(&mut self.back_button, back_r);
        if self.cancelable {
            let cancel_r = Rect::new(bx, by, bw, bh);
            self.cancel_bounds = Some(cancel_r);
            cx.layout_child(&mut self.cancel_button, cancel_r);
        } else {
            self.cancel_bounds = None;
        }
        self.sync_buttons();
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Group);
        if let Some(ref label) = self.label {
            node.set_label(label.as_str());
        }
        if !self.enabled {
            node.set_disabled();
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.enabled {
            return EventResponse::Ignored;
        }
        self.poll_buttons();
        // Escape cancels — the universal wizard affordance.
        if let WidgetEvent::KeyPressed { key, .. } = cx.event {
            if key == "Escape" && self.cancelable {
                self.cancelled = true;
                return EventResponse::Handled;
            }
        }
        // Forward through the child protocol, bounds-gated,
        // topmost-first — the same dispatch the default `event`
        // would perform.
        let mut response = EventResponse::Ignored;
        if let WidgetEvent::PointerPressed { position, .. }
        | WidgetEvent::PointerReleased { position, .. }
        | WidgetEvent::PointerMoved { position, .. }
        | WidgetEvent::Scroll { position, .. } = cx.event
        {
            let pos = *position;
            for i in (0..self.child_count()).rev() {
                let Some(b) = self.child_bounds(i) else {
                    continue;
                };
                if !b.contains(pos) {
                    continue;
                }
                let mut child_cx = EventContext {
                    event: cx.event,
                    bounds: b,
                    scale: cx.scale,
                };
                if let Some(child) = self.child_mut(i) {
                    response = child.event(&mut child_cx);
                }
                if response != EventResponse::Ignored {
                    break;
                }
            }
        } else {
            // Keyboard/IME/semantic events belong to the focus owner —
            // the shown page. Buttons take pointer activation only
            // (they'd swallow Enter/Space meant for page content).
            let page_child = 1 + self.current;
            if let Some(b) = self.child_bounds(page_child) {
                if let Some(child) = self.child_mut(page_child) {
                    let mut child_cx = EventContext {
                        event: cx.event,
                        bounds: b,
                        scale: cx.scale,
                    };
                    response = child.event(&mut child_cx);
                }
            }
        }
        self.poll_buttons();
        response
    }

    fn paint(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        let bg = cx.color(TokenKey::BackgroundColor, [255, 255, 255, 255]);
        let border = cx.color(TokenKey::DividerColor, [210, 212, 218, 255]);
        cx.list.push_fill_rect(
            kurbo::Rect::new(
                f64::from(b.min_x()),
                f64::from(b.min_y()),
                f64::from(b.max_x()),
                f64::from(b.max_y()),
            ),
            bg,
        );
        // Hairlines separating the chrome from the content region.
        for edge in [self.header_bounds, self.footer_bounds]
            .into_iter()
            .flatten()
        {
            let y = if edge.min_y() == b.min_y() {
                edge.max_y()
            } else {
                edge.min_y()
            };
            cx.list.push_fill_rect(
                kurbo::Rect::new(
                    f64::from(b.min_x()),
                    f64::from(y - cx.pt(0.5)),
                    f64::from(b.max_x()),
                    f64::from(y + cx.pt(0.5)),
                ),
                border,
            );
        }
    }

    fn child_count(&self) -> usize {
        self.total_children()
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        if Self::is_steps_index(index) {
            return Some(&self.steps);
        }
        if let Some(p) = self.page_index(index) {
            return self.pages.get(p).map(|w| w as &dyn Widget);
        }
        if index == self.back_index() {
            Some(&self.back_button)
        } else if index == self.next_index() {
            Some(&self.next_button)
        } else if self.cancelable && index == self.cancel_index() {
            Some(&self.cancel_button)
        } else {
            None
        }
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        if Self::is_steps_index(index) {
            return Some(&mut self.steps);
        }
        if let Some(p) = self.page_index(index) {
            return self.pages.get_mut(p).map(|w| w as &mut dyn Widget);
        }
        if index == self.back_index() {
            Some(&mut self.back_button)
        } else if index == self.next_index() {
            Some(&mut self.next_button)
        } else if self.cancelable && index == self.cancel_index() {
            Some(&mut self.cancel_button)
        } else {
            None
        }
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        if Self::is_steps_index(index) {
            return self.header_bounds;
        }
        if let Some(p) = self.page_index(index) {
            // Hidden pages report no bounds — skipped by paint,
            // hit-testing, and AT bounds derivation (the `PanelSet`
            // convention).
            return self.pages.get(p).filter(|w| w.shown).and_then(|w| w.bounds);
        }
        if index == self.back_index() {
            self.back_bounds
        } else if index == self.next_index() {
            self.next_bounds
        } else if self.cancelable && index == self.cancel_index() {
            self.cancel_bounds
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::Text;
    use martensite_core::{HotNode, PointerButton, WidgetEvent};

    fn laid_out(w: &mut Wizard, width: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        w.layout(&mut cx, Rect::new(0.0, 0.0, width, h));
    }

    fn ev(w: &mut Wizard, event: &WidgetEvent) -> EventResponse {
        let mut cx = EventContext {
            event,
            bounds: w.bounds,
            scale: 1.0,
        };
        w.event(&mut cx)
    }

    #[test]
    fn builder() {
        let w = Wizard::new()
            .step("a", Text::new("p1"))
            .step("b", Text::new("p2"))
            .cancelable(false)
            .finish_text("Done")
            .label("Setup");
        assert_eq!(w.step_count(), 2);
        assert!(!w.cancelable);
        assert_eq!(w.finish_text, "Done");
    }

    #[test]
    fn navigation() {
        let mut w = Wizard::new()
            .step("a", Text::new("1"))
            .step("b", Text::new("2"))
            .step("c", Text::new("3"));
        assert!(w.has_next());
        assert!(!w.has_back());
        w.go_next();
        assert_eq!(w.current(), 1);
        w.go_back();
        assert_eq!(w.current(), 0);
        w.go_to(2);
        assert_eq!(w.current(), 2);
        assert!(!w.has_next());
    }

    #[test]
    fn finish_on_last_step() {
        let mut w = Wizard::new()
            .step("a", Text::new("1"))
            .step("b", Text::new("2"));
        w.go_next();
        w.go_next();
        assert!(w.take_finished());
        assert!(!w.take_finished());
    }

    #[test]
    fn escape_cancels() {
        let mut w = Wizard::new().step("a", Text::new("1"));
        let esc = WidgetEvent::KeyPressed {
            key: "Escape".to_string(),
            repeat: false,
        };
        assert_eq!(ev(&mut w, &esc), EventResponse::Handled);
        assert!(w.take_cancelled());
    }

    #[test]
    fn only_current_page_shown() {
        let mut w = Wizard::new()
            .step("a", Text::new("1"))
            .step("b", Text::new("2"));
        laid_out(&mut w, 400.0, 300.0);
        w.go_next();
        let page0 = w.child(1).unwrap();
        let page1 = w.child(2).unwrap();
        let mut n0 = AccessKitNode::new(accesskit::Role::Unknown);
        let mut n1 = AccessKitNode::new(accesskit::Role::Unknown);
        page0.accessibility(&mut n0);
        page1.accessibility(&mut n1);
        assert!(n0.is_hidden());
        assert!(!n1.is_hidden());
    }

    #[test]
    fn next_button_drives_flow() {
        let mut w = Wizard::new()
            .step("a", Text::new("1"))
            .step("b", Text::new("2"));
        laid_out(&mut w, 400.0, 300.0);
        let next_bounds = w.child_bounds(w.next_index()).unwrap();
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(
                (next_bounds.min_x() + next_bounds.max_x()) / 2.0,
                (next_bounds.min_y() + next_bounds.max_y()) / 2.0,
            ),
            button: PointerButton::Primary,
            count: 1,
        };
        let release = WidgetEvent::PointerReleased {
            position: Vec2::new(
                (next_bounds.min_x() + next_bounds.max_x()) / 2.0,
                (next_bounds.min_y() + next_bounds.max_y()) / 2.0,
            ),
            button: PointerButton::Primary,
        };
        let _ = ev(&mut w, &press);
        let _ = ev(&mut w, &release);
        assert_eq!(w.current(), 1);
    }

    #[test]
    fn final_button_shows_finish_text() {
        let mut w = Wizard::new()
            .step("a", Text::new("1"))
            .finish_text("Deploy");
        laid_out(&mut w, 400.0, 300.0);
        assert_eq!(w.next_button.label, "Deploy");
    }

    #[test]
    fn back_disabled_on_first_page() {
        let mut w = Wizard::new()
            .step("a", Text::new("1"))
            .step("b", Text::new("2"));
        laid_out(&mut w, 400.0, 300.0);
        assert!(!w.back_button.enabled);
        w.go_next();
        laid_out(&mut w, 400.0, 300.0);
        assert!(w.back_button.enabled);
    }

    #[test]
    fn disabled_inert() {
        let mut w = Wizard::new().step("a", Text::new("1")).enabled(false);
        let esc = WidgetEvent::KeyPressed {
            key: "Escape".to_string(),
            repeat: false,
        };
        assert_eq!(ev(&mut w, &esc), EventResponse::Ignored);
        assert!(!w.take_cancelled());
    }

    #[test]
    fn child_topology() {
        let w = Wizard::new()
            .step("a", Text::new("1"))
            .step("b", Text::new("2"));
        // steps + 2 pages + 3 buttons
        assert_eq!(w.child_count(), 6);
        assert!(w.child(0).is_some());
        assert!(w.child(6).is_none());
        let w2 = Wizard::new().step("a", Text::new("1")).cancelable(false);
        assert_eq!(w2.child_count(), 4);
    }
}
