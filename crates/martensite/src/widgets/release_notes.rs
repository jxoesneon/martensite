//! `ReleaseNotes` — a versioned changelog list ("What's New" sheet /
//! GitHub-release idiom): per-release headers (version + date) with
//! categorized change bullets — Added / Changed / Fixed / Removed —
//! each with a colored kind tag.
//!
//! Display-only: the host feeds [`Release`] entries; the widget
//! renders grouped bullets in order.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::release_notes::{ChangeKind, Release, ReleaseNotes};
//!
//! let n = ReleaseNotes::new()
//!     .release(Release::new("1.4.0").change(ChangeKind::Added, "Dark mode"));
//! assert_eq!(n.release_count(), 1);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget,
};
use martensite_theme::TokenKey;

use crate::text_paint::SharedTextPainter;

const PAD_PT: f32 = 14.0;
const VERSION_PT: f32 = 15.0;
const DATE_PT: f32 = 10.5;
const ROW_PT: f32 = 20.0;
const TAG_PT_W: f32 = 62.0;
const TAG_PT_H: f32 = 14.0;
const TAG_PT: f32 = 9.0;

const FACE: [u8; 4] = [34, 36, 44, 255];
const TEXT_FG: [u8; 4] = [235, 237, 240, 255];
const MUTED_FG: [u8; 4] = [150, 154, 164, 255];
const ADDED: [u8; 4] = [70, 160, 100, 255];
const CHANGED: [u8; 4] = [90, 140, 220, 255];
const FIXED: [u8; 4] = [220, 160, 40, 255];
const REMOVED: [u8; 4] = [200, 90, 90, 255];

/// Change category with its tag color.
///
/// ```
/// use martensite::widgets::release_notes::ChangeKind;
///
/// assert_eq!(ChangeKind::Fixed.tag(), "Fixed");
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeKind {
    /// New functionality.
    Added,
    /// Modified behavior.
    Changed,
    /// Bug fix.
    Fixed,
    /// Removed functionality.
    Removed,
}

impl ChangeKind {
    /// Tag caption.
    ///
    /// ```
    /// use martensite::widgets::release_notes::ChangeKind;
    ///
    /// assert_eq!(ChangeKind::Added.tag(), "Added");
    /// ```
    pub fn tag(self) -> &'static str {
        match self {
            Self::Added => "Added",
            Self::Changed => "Changed",
            Self::Fixed => "Fixed",
            Self::Removed => "Removed",
        }
    }

    fn color(self) -> [u8; 4] {
        match self {
            Self::Added => ADDED,
            Self::Changed => CHANGED,
            Self::Fixed => FIXED,
            Self::Removed => REMOVED,
        }
    }
}

/// One release's entries.
///
/// ```
/// use martensite::widgets::release_notes::{ChangeKind, Release};
///
/// let r = Release::new("1.0").change(ChangeKind::Fixed, "Bug");
/// assert_eq!(r.changes.len(), 1);
/// ```
#[derive(Clone, Debug)]
pub struct Release {
    /// Version string.
    pub version: String,
    /// Release date caption.
    pub date: String,
    /// `(kind, description)` bullets.
    pub changes: Vec<(ChangeKind, String)>,
}

impl Release {
    /// A release for `version`.
    ///
    /// ```
    /// use martensite::widgets::release_notes::Release;
    ///
    /// assert_eq!(Release::new("2.0").version, "2.0");
    /// ```
    pub fn new(version: impl Into<String>) -> Self {
        Self {
            version: version.into(),
            date: String::new(),
            changes: Vec::new(),
        }
    }

    /// Date caption.
    ///
    /// ```
    /// use martensite::widgets::release_notes::Release;
    ///
    /// assert_eq!(Release::new("1.0").date("2026-09").date, "2026-09");
    /// ```
    pub fn date(mut self, date: impl Into<String>) -> Self {
        self.date = date.into();
        self
    }

    /// Appends a change bullet.
    ///
    /// ```
    /// use martensite::widgets::release_notes::{ChangeKind, Release};
    ///
    /// assert_eq!(Release::new("1.0").change(ChangeKind::Added, "x").changes.len(), 1);
    /// ```
    pub fn change(mut self, kind: ChangeKind, text: impl Into<String>) -> Self {
        self.changes.push((kind, text.into()));
        self
    }
}

/// The changelog list — see the module docs.
///
/// ```
/// use martensite::widgets::release_notes::ReleaseNotes;
///
/// assert_eq!(ReleaseNotes::new().release_count(), 0);
/// ```
pub struct ReleaseNotes {
    /// Accessibility label.
    pub label: String,
    releases: Vec<Release>,
    text_painter: Option<SharedTextPainter>,
    bounds: Rect,
    scale: f32,
}

impl std::fmt::Debug for ReleaseNotes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReleaseNotes")
            .field("releases", &self.releases.len())
            .finish()
    }
}

impl ReleaseNotes {
    /// An empty list.
    ///
    /// ```
    /// use martensite::widgets::release_notes::ReleaseNotes;
    ///
    /// assert_eq!(ReleaseNotes::new().release_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Release notes".to_string(),
            releases: Vec::new(),
            text_painter: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Appends a release.
    ///
    /// ```
    /// use martensite::widgets::release_notes::{Release, ReleaseNotes};
    ///
    /// assert_eq!(ReleaseNotes::new().release(Release::new("1.0")).release_count(), 1);
    /// ```
    pub fn release(mut self, release: Release) -> Self {
        self.releases.push(release);
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::release_notes::ReleaseNotes;
    ///
    /// assert_eq!(ReleaseNotes::new().label("Changes").label, "Changes");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Shared text painter.
    ///
    /// ```no_run
    /// use martensite::widgets::release_notes::ReleaseNotes;
    /// # fn example(p: martensite::text_paint::SharedTextPainter) {
    /// let _n = ReleaseNotes::new().with_text_painter(p);
    /// # }
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Release count.
    ///
    /// ```
    /// use martensite::widgets::release_notes::ReleaseNotes;
    ///
    /// assert_eq!(ReleaseNotes::new().release_count(), 0);
    /// ```
    pub fn release_count(&self) -> usize {
        self.releases.len()
    }

    /// A release entry.
    ///
    /// ```
    /// use martensite::widgets::release_notes::{Release, ReleaseNotes};
    ///
    /// let n = ReleaseNotes::new().release(Release::new("3.1"));
    /// assert_eq!(n.release_at(0).unwrap().version, "3.1");
    /// ```
    pub fn release_at(&self, index: usize) -> Option<&Release> {
        self.releases.get(index)
    }

    /// Total bullet count across releases.
    ///
    /// ```
    /// use martensite::widgets::release_notes::{ChangeKind, Release, ReleaseNotes};
    ///
    /// let n = ReleaseNotes::new()
    ///     .release(Release::new("1.0").change(ChangeKind::Added, "a").change(ChangeKind::Fixed, "b"));
    /// assert_eq!(n.change_count(), 2);
    /// ```
    pub fn change_count(&self) -> usize {
        self.releases.iter().map(|r| r.changes.len()).sum()
    }
}

impl Default for ReleaseNotes {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for ReleaseNotes {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let s = cx.scale;
        let h: f32 = self
            .releases
            .iter()
            .map(|r| VERSION_PT + 8.0 + r.changes.len() as f32 * ROW_PT + PAD_PT)
            .sum::<f32>()
            + PAD_PT;
        Vec2::new(
            (340.0 * s).min(constraints.max_size.x.max(0.0)),
            (h * s).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(160.0, 60.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::List);
        node.set_label(self.label.clone());
        node.set_value(format!("{} releases", self.releases.len()));
    }

    fn event(&mut self, _cx: &mut EventContext) -> EventResponse {
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
        let pad = PAD_PT * s;
        let mut y = b.min_y() + pad;
        for rel in &self.releases {
            // Version + date header.
            let vfs = VERSION_PT * s;
            crate::text_paint::paint_label(
                painter,
                cx.list,
                kurbo::Point::new(f64::from(b.min_x() + pad), f64::from(y + vfs)),
                &format!("v{}", rel.version),
                vfs,
                cx.color(TokenKey::TextColor, TEXT_FG),
            );
            if !rel.date.is_empty() {
                let vw = rel.version.len() as f32 * vfs * 0.6 + vfs;
                crate::text_paint::paint_label(
                    painter,
                    cx.list,
                    kurbo::Point::new(
                        f64::from(b.min_x() + pad + vw + 8.0 * s),
                        f64::from(y + vfs),
                    ),
                    &rel.date,
                    DATE_PT * s,
                    MUTED_FG,
                );
            }
            y += (VERSION_PT + 8.0) * s;
            // Change bullets.
            for (kind, text) in &rel.changes {
                let tr = Rect::new(b.min_x() + pad, y, TAG_PT_W * s, TAG_PT_H * s);
                cx.list.push_fill_shape(
                    kurbo::Rect::new(
                        f64::from(tr.min_x()),
                        f64::from(tr.min_y()),
                        f64::from(tr.max_x()),
                        f64::from(tr.max_y()),
                    ),
                    &martensite_core::shape::Shape::rounded(3.0 * s),
                    kind.color(),
                );
                let tfs = TAG_PT * s;
                let tw = kind.tag().len() as f32 * tfs * 0.62;
                crate::text_paint::paint_label(
                    painter,
                    cx.list,
                    kurbo::Point::new(
                        f64::from(tr.min_x() + (tr.width() - tw) / 2.0),
                        f64::from(tr.min_y() + tr.height() / 2.0 + tfs * 0.35),
                    ),
                    kind.tag(),
                    tfs,
                    [255, 255, 255, 255],
                );
                crate::text_paint::paint_label(
                    painter,
                    cx.list,
                    kurbo::Point::new(
                        f64::from(tr.max_x() + 8.0 * s),
                        f64::from(y + tr.height() * 0.8),
                    ),
                    text,
                    DATE_PT * s,
                    cx.color(TokenKey::TextColor, TEXT_FG),
                );
                y += ROW_PT * s;
            }
            y += pad * 0.6;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    #[test]
    fn counts_accumulate() {
        let n = ReleaseNotes::new()
            .release(
                Release::new("1.4.0")
                    .date("2026-09")
                    .change(ChangeKind::Added, "Dark mode")
                    .change(ChangeKind::Fixed, "Crash"),
            )
            .release(Release::new("1.3.0").change(ChangeKind::Removed, "Legacy API"));
        assert_eq!(n.release_count(), 2);
        assert_eq!(n.change_count(), 3);
    }

    #[test]
    fn paint_without_painter() {
        let mut n = ReleaseNotes::new().release(
            Release::new("1.0")
                .date("today")
                .change(ChangeKind::Added, "x"),
        );
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        n.layout(&mut cx, Rect::new(0.0, 0.0, 340.0, 160.0));
        let mut list = martensite_core::PaintList::default();
        let theme = martensite_theme::Theme::new("test");
        n.paint(&mut PaintContext {
            list: &mut list,
            bounds: n.bounds,
            theme: &theme,
            scale: 1.0,
            text_painter: None,
        });
        assert!(!list.is_empty());
    }
}
