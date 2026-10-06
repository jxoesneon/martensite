//! `EmptyState` widget: a centred icon + title + description + action
//! placeholder for empty content areas (ADW `StatusPage`, SwiftUI
//! `ContentUnavailableView`, Ant Design `Empty`).
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::empty_state::EmptyState;
//!
//! let e = EmptyState::new("No results")
//!     .icon("◇")
//!     .description("Try adjusting the filter.")
//!     .action("Clear filter");
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::widget::{LayoutConstraints, LayoutContext, PaintContext, Widget};
use martensite_core::{Rect, TokenKey};

use crate::widgets::button::Button;
use crate::widgets::morph_icon::MorphIcon;

/// Glyph size for the icon, logical points.
const ICON_PT: f32 = 42.0;
/// Title font size, logical points.
const TITLE_PT: f32 = 17.0;
/// Description font size, logical points.
const DESC_PT: f32 = 13.0;
/// Vertical gap between stacked elements, logical points.
const GAP_PT: f32 = 10.0;
/// Maximum width of the description line, logical points.
const DESC_MAX_W_PT: f32 = 320.0;
/// Fallback icon ink — muted so the glyph reads as a placeholder.
const ICON_INK: [u8; 4] = [150, 154, 163, 255];
/// Title ink.
const TITLE_INK: [u8; 4] = [30, 31, 36, 255];
/// Description ink.
const DESC_INK: [u8; 4] = [105, 109, 118, 255];

/// The icon lane content — a text glyph / icon name or a hosted
/// stroke icon.
enum EmptyIcon {
    /// A short string painted large — a namespaced icon name paints
    /// through the ambient icon family; anything else stays text.
    Glyph(String),
    /// A hosted [`MorphIcon`] stroke icon — a real internal child, so
    /// it ticks with the arena (`morph_to_named` animates) and reports
    /// decorative-hidden in the a11y tree.
    Stroke(MorphIcon),
}

/// A centred placeholder shown when a view has no content.
///
/// Stacks an optional icon glyph or stroke icon, a title, an optional
/// description line, and an optional action [`Button`] vertically in
/// the centre of the allocated bounds. The action button — and a
/// stroke icon installed via [`icon_d`](Self::icon_d) /
/// [`icon_named`](Self::icon_named) — is a real widget child: events,
/// focus, and its accessibility node flow through the framework's
/// internal-child protocol, and the action's activation is drained via
/// [`EmptyState::take_activated`].
///
/// # Examples
///
/// ```
/// use martensite::widgets::empty_state::EmptyState;
///
/// let e = EmptyState::new("Inbox zero").icon("status.check");
/// assert_eq!(e.title(), "Inbox zero");
/// ```
pub struct EmptyState {
    /// Optional icon painted large above the title — a text glyph or
    /// a hosted [`MorphIcon`].
    icon: Option<EmptyIcon>,
    /// The headline text.
    title: String,
    /// Optional secondary line under the title.
    description: Option<String>,
    /// Optional action button under the description.
    action: Option<Button>,
    /// Cached bounds from the last layout pass.
    cached_bounds: Rect,
    /// Bounds assigned to the action button, in widget space.
    action_rect: Rect,
    /// Bounds assigned to a hosted stroke icon, in widget space.
    icon_rect: Rect,
    /// Shared shaped-text painter for real `GlyphRun`s. See
    /// [`crate::text_paint`].
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl EmptyState {
    /// Creates an empty state with the given title.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::empty_state::EmptyState;
    ///
    /// let e = EmptyState::new("Nothing here");
    /// assert_eq!(e.title(), "Nothing here");
    /// ```
    #[must_use]
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            icon: None,
            title: title.into(),
            description: None,
            action: None,
            cached_bounds: Rect::default(),
            action_rect: Rect::default(),
            icon_rect: Rect::default(),
            text_painter: None,
        }
    }

    /// Sets the icon mark — a short string painted large above the
    /// title. An icon name (`"comms.mail"`, `"file.inbox"`, …)
    /// resolves through the ambient icon family
    /// ([`crate::icons::resolve_icon`]); any other string stays a
    /// text glyph (a symbol, an icon-font codepoint, …).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::empty_state::EmptyState;
    ///
    /// let e = EmptyState::new("No mail").icon("comms.mail");
    /// ```
    #[must_use]
    pub fn icon(mut self, glyph: impl Into<String>) -> Self {
        self.icon = Some(EmptyIcon::Glyph(glyph.into()));
        self
    }

    /// Sets the icon to a hosted [`MorphIcon`] stroke icon — `d` is
    /// SVG path data on the 24-unit icon grid (the lucide/feather
    /// idiom; see [`crate::icons::builtin`]). The icon is a real
    /// internal child: it ticks with the arena, paints the stroke
    /// itself, and reports decorative-hidden in the a11y tree — the
    /// state owns the accessible name.
    ///
    /// A `d` the icon engine rejects parses to no icon at all — the
    /// state keeps its title and behaves iconless rather than failing
    /// the build.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::icons::builtin;
    /// use martensite::widgets::empty_state::EmptyState;
    ///
    /// let d = builtin().lookup("status.check-circle").unwrap();
    /// let e = EmptyState::new("Inbox zero").icon_d(d);
    /// assert!(e.icon_widget().is_some());
    /// ```
    #[must_use]
    pub fn icon_d(mut self, d: &str) -> Self {
        self.icon = MorphIcon::icon(d)
            .ok()
            .map(|i| EmptyIcon::Stroke(i.decorative(true)));
        self
    }

    /// Sets the icon to a hosted [`MorphIcon`] resolving `name`
    /// through the native icon pack
    /// ([`icons::BUILTIN`](crate::icons::BUILTIN)) — `"status.check-circle"`,
    /// `"file.inbox"`, … An unknown name degrades to an iconless state
    /// rather than failing the build — same contract as
    /// [`icon_d`](Self::icon_d).
    ///
    /// Apps carrying overlay packs resolve names through their own
    /// [`IconSet`](crate::icons::IconSet) and pass the `d` to
    /// [`icon_d`](Self::icon_d).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::empty_state::EmptyState;
    ///
    /// let e = EmptyState::new("All clear").icon_named("status.check-circle");
    /// assert!(e.icon_widget().is_some());
    /// assert!(EmptyState::new("x")
    ///     .icon_named("bogus.name")
    ///     .icon_widget()
    ///     .is_none());
    /// ```
    #[must_use]
    pub fn icon_named(mut self, name: &str) -> Self {
        self.icon = MorphIcon::named(name)
            .ok()
            .map(|i| EmptyIcon::Stroke(i.decorative(true)));
        self
    }

    /// The hosted stroke icon, when one is installed —
    /// `None` for a glyph icon or none. Mutate through
    /// [`icon_widget_mut`](Self::icon_widget_mut) to morph it (e.g.
    /// `morph_to_named("status.warning")` when a later state change
    /// turns "all clear" into "attention").
    #[must_use]
    pub fn icon_widget(&self) -> Option<&MorphIcon> {
        match &self.icon {
            Some(EmptyIcon::Stroke(i)) => Some(i),
            _ => None,
        }
    }

    /// Mutable twin of [`icon_widget`](Self::icon_widget).
    pub fn icon_widget_mut(&mut self) -> Option<&mut MorphIcon> {
        match &mut self.icon {
            Some(EmptyIcon::Stroke(i)) => Some(i),
            _ => None,
        }
    }

    /// Sets the description line under the title.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::empty_state::EmptyState;
    ///
    /// let e = EmptyState::new("No results").description("Try a broader query.");
    /// ```
    #[must_use]
    pub fn description(mut self, text: impl Into<String>) -> Self {
        self.description = Some(text.into());
        self
    }

    /// Adds an action [`Button`] under the description. Its
    /// activation is drained with [`EmptyState::take_activated`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::empty_state::EmptyState;
    ///
    /// let e = EmptyState::new("No items").action("Add item");
    /// assert!(e.action_button().is_some());
    /// ```
    #[must_use]
    pub fn action(mut self, label: impl Into<String>) -> Self {
        self.action = Some(Button::new(label));
        self
    }

    /// The title text.
    #[inline]
    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    /// The action button, if one was configured.
    #[inline]
    #[must_use]
    pub fn action_button(&self) -> Option<&Button> {
        self.action.as_ref()
    }

    /// Drains the action button's activation flag — `true` once per
    /// completed press. Returns `false` when no action is configured.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::empty_state::EmptyState;
    ///
    /// let mut e = EmptyState::new("Empty").action("Retry");
    /// assert!(!e.take_activated());
    /// ```
    pub fn take_activated(&mut self) -> bool {
        self.action.as_mut().is_some_and(Button::take_activated)
    }

    /// Installs a shared shaped-text painter for the icon, title, and
    /// description.
    #[must_use]
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Total stacked height of the content in logical points.
    fn content_height_pt(&self) -> f32 {
        let mut h = 0.0;
        if self.icon.is_some() {
            h += ICON_PT + GAP_PT;
        }
        h += TITLE_PT;
        if self.description.is_some() {
            h += GAP_PT + DESC_PT;
        }
        if self.action.is_some() {
            h += GAP_PT + 32.0;
        }
        h
    }

    /// Emits one centred text line into the paint list.
    fn paint_centered(
        &self,
        cx: &mut PaintContext,
        text: &str,
        size_pt: f32,
        max_w_pt: f32,
        y_center: f32,
        ink: [u8; 4],
    ) {
        let b = cx.bounds;
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let size_px = cx.pt(size_pt);
        // Centre via the shaped advance when a painter is available so
        // the line is truly centred, not padded — then clip to the
        // column so an overlong run fades at the edge instead of
        // spilling.
        let w = painter
            .and_then(|p| p.measure_text(text, size_px))
            .unwrap_or(size_px * text.chars().count() as f32 * 0.5);
        let max_w = cx.pt(max_w_pt).min(b.size.x);
        let x = b.origin.x + (b.size.x - w.min(max_w)) / 2.0;
        // `y_center` is the slot's optical centre — centre the shaped
        // ink on it (`centered_label_top`) rather than using the font
        // size as the ink height. The clip band is `2·size` tall so
        // descenders and line-box leading aren't sheared off.
        let top = crate::text_paint::centered_label_top(painter, y_center, text, size_px);
        crate::text_paint::paint_label_clipped(
            painter,
            cx.list,
            kurbo::Rect::new(
                f64::from(x),
                f64::from(y_center - size_px),
                f64::from(x + max_w),
                f64::from(y_center + size_px),
            ),
            kurbo::Point::new(f64::from(x), top),
            text,
            size_px,
            ink,
        );
    }
}

impl Widget for EmptyState {
    /// Centered title/body fade-clip at the column edge — truncation
    /// is the designed degradation for overlong strings.
    fn debug_name(&self) -> &'static str {
        "EmptyState@lint:text-truncation"
    }
    #[cfg(feature = "devtools-timemachine")]
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }

    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        // Fill whatever the parent offers; the stack centres itself.
        // A modest floor keeps the state readable under tight layout.
        Vec2::new(
            constraints.max_size.x.max(cx.pt(200.0)),
            constraints.max_size.y.max(cx.pt(120.0)),
        )
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.cached_bounds = bounds;
        self.action_rect = Rect::default();
        self.icon_rect = Rect::default();
        let content_h = cx.pt(self.content_height_pt());
        let button_h = cx.pt(32.0);
        let button_w = cx.pt(140.0).min(bounds.size.x);
        let stack_top = bounds.origin.y + (bounds.size.y - content_h).max(0.0) / 2.0;
        if let Some(EmptyIcon::Stroke(icon)) = &mut self.icon {
            let side = cx.pt(ICON_PT);
            self.icon_rect = Rect::new(
                bounds.origin.x + (bounds.size.x - side) / 2.0,
                stack_top,
                side,
                side,
            );
            cx.layout_child(icon, self.icon_rect);
        }
        if let Some(button) = &mut self.action {
            let y = stack_top + content_h - button_h;
            self.action_rect = Rect::new(
                bounds.origin.x + (bounds.size.x - button_w) / 2.0,
                y,
                button_w,
                button_h,
            );
            cx.layout_child(button, self.action_rect);
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Group);
        node.set_label(self.title.as_str());
    }

    fn paint(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        if b.size.x <= 0.0 || b.size.y <= 0.0 {
            return;
        }
        let content_h = cx.pt(self.content_height_pt());
        let mut y_center = b.origin.y + (b.size.y - content_h).max(0.0) / 2.0;

        if let Some(icon) = &self.icon {
            y_center += cx.pt(ICON_PT) / 2.0;
            // A hosted stroke icon paints itself at `icon_rect` as an
            // internal child — the arena emits it after this pass.
            if let EmptyIcon::Glyph(glyph) = icon {
                // A namespaced icon name resolves through the ambient
                // icon family; any other string stays a text glyph.
                let side = cx.pt(ICON_PT);
                let painted = crate::widgets::morph_icon::paint_icon_named(
                    cx.list,
                    martensite_core::Rect::new(
                        b.origin.x + (b.size.x - side) / 2.0,
                        y_center - side / 2.0,
                        side,
                        side,
                    ),
                    glyph,
                    cx.scale,
                    cx.color(TokenKey::TextMutedColor, ICON_INK),
                );
                if !painted {
                    self.paint_centered(
                        cx,
                        glyph,
                        ICON_PT,
                        ICON_PT * 2.0,
                        y_center,
                        cx.color(TokenKey::TextMutedColor, ICON_INK),
                    );
                }
            }
            y_center += cx.pt(ICON_PT) / 2.0 + cx.pt(GAP_PT);
        }

        y_center += cx.pt(TITLE_PT) / 2.0;
        self.paint_centered(
            cx,
            &self.title,
            TITLE_PT,
            DESC_MAX_W_PT,
            y_center,
            cx.color(TokenKey::TextColor, TITLE_INK),
        );
        y_center += cx.pt(TITLE_PT) / 2.0;

        if let Some(desc) = &self.description {
            y_center += cx.pt(GAP_PT) + cx.pt(DESC_PT) / 2.0;
            self.paint_centered(
                cx,
                desc,
                DESC_PT,
                DESC_MAX_W_PT,
                y_center,
                cx.color(TokenKey::TextMutedColor, DESC_INK),
            );
        }
    }

    fn child_count(&self) -> usize {
        usize::from(matches!(self.icon, Some(EmptyIcon::Stroke(_))))
            + usize::from(self.action.is_some())
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        let icon = match &self.icon {
            Some(EmptyIcon::Stroke(i)) => Some(i as &dyn Widget),
            _ => None,
        };
        if index == 0 && icon.is_some() {
            return icon;
        }
        if index == usize::from(icon.is_some()) {
            self.action.as_ref().map(|b| b as &dyn Widget)
        } else {
            None
        }
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        let has_icon = matches!(self.icon, Some(EmptyIcon::Stroke(_)));
        if index == 0 && has_icon {
            return match &mut self.icon {
                Some(EmptyIcon::Stroke(i)) => Some(i as &mut dyn Widget),
                _ => None,
            };
        }
        let action_idx = usize::from(has_icon);
        if index == action_idx {
            self.action.as_mut().map(|b| b as &mut dyn Widget)
        } else {
            None
        }
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        let has_icon = matches!(self.icon, Some(EmptyIcon::Stroke(_)));
        if index == 0 && has_icon {
            return Some(self.icon_rect);
        }
        let action_idx = usize::from(has_icon);
        if index == action_idx && self.action.is_some() {
            Some(self.action_rect)
        } else {
            None
        }
    }
}

impl std::fmt::Debug for EmptyState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let icon = match &self.icon {
            Some(EmptyIcon::Glyph(g)) => format!("glyph:{g:?}"),
            Some(EmptyIcon::Stroke(_)) => "stroke".to_string(),
            None => "none".to_string(),
        };
        f.debug_struct("EmptyState")
            .field("title", &self.title)
            .field("icon", &icon)
            .field("description", &self.description)
            .field("has_action", &self.action.is_some())
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
    fn builder_sets_fields() {
        let e = EmptyState::new("No data")
            .icon("◇")
            .description("desc")
            .action("Go");
        assert_eq!(e.title(), "No data");
        assert_eq!(e.child_count(), 1);
    }

    #[test]
    fn no_action_means_no_children() {
        let e = EmptyState::new("Empty");
        assert_eq!(e.child_count(), 0);
        assert!(e.child(0).is_none());
    }

    #[test]
    fn action_child_is_exposed() {
        let mut e = EmptyState::new("Empty").action("Retry");
        let mut hot = HotNode::default();
        e.layout(&mut make_cx(&mut hot), Rect::new(0.0, 0.0, 400.0, 300.0));
        assert!(e.child(0).is_some());
        let r = e.child_bounds(0).unwrap();
        assert!(r.size.x > 0.0 && r.size.y > 0.0);
        // The action sits in the lower half of the centred stack.
        assert!(r.origin.y > 120.0);
    }

    #[test]
    fn take_activated_forwards_to_button() {
        let mut e = EmptyState::new("Empty");
        assert!(!e.take_activated());
        let mut e2 = EmptyState::new("Empty").action("Retry");
        assert!(!e2.take_activated());
    }

    #[test]
    fn measure_fills_and_floors() {
        let mut e = EmptyState::new("Empty");
        let mut hot = HotNode::default();
        let size = e.measure(
            &mut make_cx(&mut hot),
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(600.0, 400.0),
            },
        );
        assert_eq!(size, Vec2::new(600.0, 400.0));
    }
}
