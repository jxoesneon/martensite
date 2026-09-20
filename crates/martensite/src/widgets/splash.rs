//! `Splash` — an application splash screen: a centered logo
//! letter-mark, app name, version caption, a determinate progress
//! bar, and a status line (the classic startup-card idiom).
//!
//! Display-only — the host drives [`Splash::set_progress`] and
//! [`Splash::set_status`] from its init pipeline and drops the
//! widget when the main window is ready.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::splash::Splash;
//!
//! let s = Splash::new("Martensite").version("0.18.0");
//! assert_eq!(s.app_name(), "Martensite");
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget,
};
use martensite_theme::TokenKey;

use crate::text_paint::SharedTextPainter;

const LOGO_PT: f32 = 72.0;
const NAME_PT: f32 = 22.0;
const SMALL_PT: f32 = 11.0;
const BAR_W_PT: f32 = 200.0;
const BAR_H_PT: f32 = 4.0;

const FACE: [u8; 4] = [24, 26, 33, 255];
const LOGO: [u8; 4] = [90, 140, 220, 255];
const TEXT: [u8; 4] = [235, 237, 240, 255];
const MUTED_FG: [u8; 4] = [150, 154, 164, 255];
const TRACK: [u8; 4] = [255, 255, 255, 30];

/// The splash card — see the module docs.
///
/// ```
/// use martensite::widgets::splash::Splash;
///
/// assert_eq!(Splash::new("App").progress(), 0.0);
/// ```
pub struct Splash {
    /// Accessibility label.
    pub label: String,
    /// Logo swatch color.
    pub logo_color: [u8; 4],
    name: String,
    version: String,
    status: String,
    progress: f32,
    text_painter: Option<SharedTextPainter>,
    bounds: Rect,
    scale: f32,
}

impl std::fmt::Debug for Splash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Splash")
            .field("name", &self.name)
            .field("progress", &self.progress)
            .finish()
    }
}

impl Splash {
    /// A splash for `name`.
    ///
    /// ```
    /// use martensite::widgets::splash::Splash;
    ///
    /// assert_eq!(Splash::new("App").app_name(), "App");
    /// ```
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            label: "Starting".to_string(),
            logo_color: LOGO,
            name: name.into(),
            version: String::new(),
            status: String::new(),
            progress: 0.0,
            text_painter: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Version caption under the name.
    ///
    /// ```
    /// use martensite::widgets::splash::Splash;
    ///
    /// assert_eq!(Splash::new("A").version("1.0").version_string(), "1.0");
    /// ```
    pub fn version(mut self, version: impl Into<String>) -> Self {
        self.version = version.into();
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::splash::Splash;
    ///
    /// assert_eq!(Splash::new("A").label("Loading").label, "Loading");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Shared text painter.
    ///
    /// ```no_run
    /// use martensite::widgets::splash::Splash;
    /// # fn example(p: martensite::text_paint::SharedTextPainter) {
    /// let _s = Splash::new("A").with_text_painter(p);
    /// # }
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// App name.
    ///
    /// ```
    /// use martensite::widgets::splash::Splash;
    ///
    /// assert_eq!(Splash::new("App").app_name(), "App");
    /// ```
    pub fn app_name(&self) -> &str {
        &self.name
    }

    /// Version caption.
    ///
    /// ```
    /// use martensite::widgets::splash::Splash;
    ///
    /// assert_eq!(Splash::new("A").version_string(), "");
    /// ```
    pub fn version_string(&self) -> &str {
        &self.version
    }

    /// Current progress (0–1).
    ///
    /// ```
    /// use martensite::widgets::splash::Splash;
    ///
    /// assert_eq!(Splash::new("A").progress(), 0.0);
    /// ```
    pub fn progress(&self) -> f32 {
        self.progress
    }

    /// Sets progress, clamped to 0–1.
    ///
    /// ```
    /// use martensite::widgets::splash::Splash;
    ///
    /// let mut s = Splash::new("A");
    /// s.set_progress(0.5);
    /// assert_eq!(s.progress(), 0.5);
    /// s.set_progress(2.0);
    /// assert_eq!(s.progress(), 1.0);
    /// ```
    pub fn set_progress(&mut self, progress: f32) {
        self.progress = progress.clamp(0.0, 1.0);
    }

    /// Status line under the bar.
    ///
    /// ```
    /// use martensite::widgets::splash::Splash;
    ///
    /// let mut s = Splash::new("A");
    /// s.set_status("Loading fonts…");
    /// assert_eq!(s.status(), "Loading fonts…");
    /// ```
    pub fn set_status(&mut self, status: impl Into<String>) {
        self.status = status.into();
    }

    /// Current status line.
    ///
    /// ```
    /// use martensite::widgets::splash::Splash;
    ///
    /// assert_eq!(Splash::new("A").status(), "");
    /// ```
    pub fn status(&self) -> &str {
        &self.status
    }
}

impl Widget for Splash {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let s = cx.scale;
        Vec2::new(
            (360.0 * s).min(constraints.max_size.x.max(0.0)),
            (240.0 * s).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(200.0, 160.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Image);
        node.set_label(format!("{} — {}", self.label, self.name));
        node.set_value(format!("{:.0}%", self.progress * 100.0));
    }

    fn event(&mut self, _cx: &mut EventContext) -> EventResponse {
        EventResponse::Ignored
    }

    fn paint(&self, cx: &mut PaintContext) {
        let s = cx.scale;
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let b = self.bounds;
        let cxm = b.min_x() + b.width() / 2.0;
        cx.list.push_fill_rect(
            kurbo::Rect::new(
                f64::from(b.min_x()),
                f64::from(b.min_y()),
                f64::from(b.max_x()),
                f64::from(b.max_y()),
            ),
            cx.color(TokenKey::BackgroundColor, FACE),
        );
        // Logo letter-mark.
        let logo = LOGO_PT * s;
        let ly = b.min_y() + b.height() * 0.22;
        let lr = kurbo::Rect::new(
            f64::from(cxm - logo / 2.0),
            f64::from(ly),
            f64::from(cxm + logo / 2.0),
            f64::from(ly + logo),
        );
        cx.list.push_fill_shape(
            lr,
            &martensite_core::shape::Shape::rounded(logo * 0.22),
            cx.color(TokenKey::AccentColor, self.logo_color),
        );
        let initial: String = self.name.chars().take(1).collect();
        let ifs = logo * 0.5;
        let iw = painter
            .and_then(|p| p.measure_text(&initial, ifs))
            .unwrap_or(ifs * 0.5);
        crate::text_paint::paint_label(
            painter,
            cx.list,
            kurbo::Point::new(f64::from(cxm - iw / 2.0), f64::from(ly + logo * 0.72)),
            &initial,
            ifs,
            TEXT,
        );
        // Name.
        let nfs = NAME_PT * s;
        let nw = painter
            .and_then(|p| p.measure_text(&self.name, nfs))
            .unwrap_or(self.name.len() as f32 * nfs * 0.5);
        let ny = ly + logo + 20.0 * s;
        crate::text_paint::paint_label(
            painter,
            cx.list,
            kurbo::Point::new(f64::from(cxm - nw / 2.0), f64::from(ny)),
            &self.name,
            nfs,
            cx.color(TokenKey::TextColor, TEXT),
        );
        // Version.
        if !self.version.is_empty() {
            let vfs = SMALL_PT * s;
            let vw = painter
                .and_then(|p| p.measure_text(&self.version, vfs))
                .unwrap_or(self.version.len() as f32 * vfs * 0.5);
            crate::text_paint::paint_label(
                painter,
                cx.list,
                kurbo::Point::new(f64::from(cxm - vw / 2.0), f64::from(ny + 16.0 * s)),
                &self.version,
                vfs,
                MUTED_FG,
            );
        }
        // Progress bar.
        let bw = (BAR_W_PT * s).min(b.width() * 0.7);
        let bh = BAR_H_PT * s;
        let bar = kurbo::Rect::new(
            f64::from(cxm - bw / 2.0),
            f64::from(b.max_y() - b.height() * 0.2),
            f64::from(cxm + bw / 2.0),
            f64::from(b.max_y() - b.height() * 0.2 + bh),
        );
        let shape = martensite_core::shape::Shape::rounded(bh / 2.0);
        cx.list.push_fill_shape(bar, &shape, TRACK);
        if self.progress > 0.0 {
            let fill = kurbo::Rect::new(
                bar.x0,
                bar.y0,
                bar.x0 + f64::from(bw) * f64::from(self.progress),
                bar.y1,
            );
            cx.list
                .push_fill_shape(fill, &shape, cx.color(TokenKey::AccentColor, LOGO));
        }
        // Status line.
        if !self.status.is_empty() {
            let sfs = SMALL_PT * s;
            let sw = painter
                .and_then(|p| p.measure_text(&self.status, sfs))
                .unwrap_or(self.status.len() as f32 * sfs * 0.5);
            crate::text_paint::paint_label(
                painter,
                cx.list,
                kurbo::Point::new(
                    f64::from(cxm - sw / 2.0),
                    f64::from(b.max_y() - b.height() * 0.2 + bh + 14.0 * s),
                ),
                &self.status,
                sfs,
                MUTED_FG,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn fixture() -> Splash {
        Splash::new("Martensite").version("0.18.0")
    }

    fn laid_out(s: &mut Splash) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        s.layout(&mut cx, Rect::new(0.0, 0.0, 360.0, 240.0));
    }

    #[test]
    fn progress_clamps() {
        let mut s = fixture();
        s.set_progress(0.4);
        assert_eq!(s.progress(), 0.4);
        s.set_progress(-1.0);
        assert_eq!(s.progress(), 0.0);
        s.set_progress(7.0);
        assert_eq!(s.progress(), 1.0);
    }

    #[test]
    fn status_updates() {
        let mut s = fixture();
        s.set_status("Loading…");
        assert_eq!(s.status(), "Loading…");
    }

    #[test]
    fn paint_without_painter() {
        let mut s = fixture();
        laid_out(&mut s);
        s.set_progress(0.5);
        s.set_status("Loading");
        let mut list = martensite_core::PaintList::default();
        let theme = martensite_theme::Theme::new("test");
        s.paint(&mut PaintContext {
            list: &mut list,
            bounds: s.bounds,
            theme: &theme,
            scale: 1.0,
            text_painter: None,
        });
        assert!(!list.is_empty());
    }
}
