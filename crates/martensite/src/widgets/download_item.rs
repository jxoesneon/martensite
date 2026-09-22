//! `DownloadItem` — a browser-style download row (name + progress
//! bar + speed/ETA + pause / resume / cancel / show-in-folder
//! affordances).
//!
//! The row is display-driven: the host feeds
//! [`DownloadItem::set_progress`] and
//! [`DownloadItem::set_state`]. Button clicks park a
//! [`DownloadAction`] in [`DownloadItem::take_action`] for the host
//! to act on.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::download_item::{DownloadItem, DownloadState};
//!
//! let d = DownloadItem::new("setup.dmg", 1_048_576).progress(0.4);
//! assert_eq!(d.state, DownloadState::Downloading);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::text_paint::SharedTextPainter;

const PAD_PT: f32 = 8.0;
const ICON_PT: f32 = 24.0;
const GAP_PT: f32 = 8.0;
const NAME_PT: f32 = 11.5;
const INFO_PT: f32 = 9.5;
const BAR_PT: f32 = 4.0;
const BTN_PT: f32 = 20.0;

const FACE: [u8; 4] = [40, 43, 52, 255];
const EDGE: [u8; 4] = [78, 82, 92, 255];
const TEXT_FG: [u8; 4] = [235, 237, 240, 255];
const MUTED_FG: [u8; 4] = [150, 154, 164, 255];
const ICON_BG: [u8; 4] = [60, 64, 76, 255];
const BAR_BG: [u8; 4] = [60, 63, 72, 255];

/// Download lifecycle.
///
/// ```
/// use martensite::widgets::download_item::DownloadState;
///
/// assert_eq!(DownloadState::Downloading.info(0.5, 524_288.0), "0.5 MB/s");
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DownloadState {
    /// Actively transferring.
    Downloading,
    /// Paused by the user.
    Paused,
    /// Finished — folder affordance shown.
    Done,
    /// Failed — caption shows in warning color.
    Failed,
    /// Canceled by the host.
    Canceled,
}

impl DownloadState {
    /// Secondary-line text: speed while downloading, else a status.
    ///
    /// ```
    /// use martensite::widgets::download_item::DownloadState;
    ///
    /// assert_eq!(DownloadState::Done.info(1.0, 0.0), "Done");
    /// ```
    pub fn info(self, fraction: f32, bytes_per_sec: f64) -> String {
        match self {
            Self::Downloading => {
                if bytes_per_sec > 0.0 {
                    let mb = bytes_per_sec / 1_048_576.0;
                    format!("{:.1} MB/s", mb.max(0.1))
                } else {
                    format!("{:.0}%", fraction * 100.0)
                }
            }
            Self::Paused => "Paused".to_string(),
            Self::Done => "Done".to_string(),
            Self::Failed => "Failed".to_string(),
            Self::Canceled => "Canceled".to_string(),
        }
    }
}

/// What the host should do.
///
/// ```
/// use martensite::widgets::download_item::DownloadAction;
///
/// assert_eq!(DownloadAction::Pause, DownloadAction::Pause);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DownloadAction {
    /// Pause the transfer.
    Pause,
    /// Resume a paused transfer.
    Resume,
    /// Cancel the download.
    Cancel,
    /// Reveal the file in the file manager.
    ShowInFolder,
    /// Retry a failed download.
    Retry,
}

/// The download row — see the module docs.
///
/// ```
/// use martensite::widgets::download_item::DownloadItem;
///
/// assert_eq!(DownloadItem::new("f.zip", 10).name, "f.zip");
/// ```
pub struct DownloadItem {
    /// Accessibility label.
    pub label: String,
    /// File name.
    pub name: String,
    /// Total bytes.
    pub total_bytes: u64,
    /// Type glyph.
    pub glyph: String,
    /// Lifecycle state.
    pub state: DownloadState,
    /// Progress `0.0..=1.0`.
    pub fraction: f32,
    /// Current rate in bytes/sec (drives the info line).
    pub rate: f64,
    action: Option<DownloadAction>,
    buttons: Vec<(Rect, DownloadAction)>,
    bar_rect: Rect,
    text_painter: Option<SharedTextPainter>,
    bounds: Rect,
    scale: f32,
}

impl std::fmt::Debug for DownloadItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DownloadItem")
            .field("name", &self.name)
            .field("state", &self.state)
            .finish()
    }
}

impl DownloadItem {
    /// A row for `name` of `total_bytes`.
    ///
    /// ```
    /// use martensite::widgets::download_item::DownloadItem;
    ///
    /// assert_eq!(DownloadItem::new("a", 1).total_bytes, 1);
    /// ```
    pub fn new(name: impl Into<String>, total_bytes: u64) -> Self {
        Self {
            label: "Download".to_string(),
            name: name.into(),
            total_bytes,
            glyph: "⬇".to_string(),
            state: DownloadState::Downloading,
            fraction: 0.0,
            rate: 0.0,
            action: None,
            buttons: Vec::new(),
            bar_rect: Rect::new(0.0, 0.0, 0.0, 0.0),
            text_painter: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Progress fraction.
    ///
    /// ```
    /// use martensite::widgets::download_item::DownloadItem;
    ///
    /// assert_eq!(DownloadItem::new("a", 1).progress(0.7).fraction, 0.7);
    /// ```
    pub fn progress(mut self, fraction: f32) -> Self {
        self.fraction = fraction.clamp(0.0, 1.0);
        self
    }

    /// Type glyph override.
    ///
    /// ```
    /// use martensite::widgets::download_item::DownloadItem;
    ///
    /// assert_eq!(DownloadItem::new("a", 1).glyph("📦").glyph, "📦");
    /// ```
    pub fn glyph(mut self, glyph: impl Into<String>) -> Self {
        self.glyph = glyph.into();
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::download_item::DownloadItem;
    ///
    /// assert_eq!(DownloadItem::new("a", 1).label("DL").label, "DL");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Shared text painter.
    ///
    /// ```no_run
    /// use martensite::widgets::download_item::DownloadItem;
    /// # fn example(p: martensite::text_paint::SharedTextPainter) {
    /// let _d = DownloadItem::new("a", 1).with_text_painter(p);
    /// # }
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Sets the lifecycle state.
    ///
    /// ```
    /// use martensite::widgets::download_item::{DownloadItem, DownloadState};
    ///
    /// let mut d = DownloadItem::new("a", 1);
    /// d.set_state(DownloadState::Paused);
    /// assert_eq!(d.state, DownloadState::Paused);
    /// ```
    pub fn set_state(&mut self, state: DownloadState) {
        self.state = state;
    }

    /// Sets the progress fraction.
    ///
    /// ```
    /// use martensite::widgets::download_item::DownloadItem;
    ///
    /// let mut d = DownloadItem::new("a", 1);
    /// d.set_progress(0.9);
    /// assert_eq!(d.fraction, 0.9);
    /// ```
    pub fn set_progress(&mut self, fraction: f32) {
        self.fraction = fraction.clamp(0.0, 1.0);
    }

    /// Sets the transfer rate (bytes/sec).
    ///
    /// ```
    /// use martensite::widgets::download_item::DownloadItem;
    ///
    /// let mut d = DownloadItem::new("a", 1);
    /// d.set_rate(2_000_000.0);
    /// assert_eq!(d.rate, 2_000_000.0);
    /// ```
    pub fn set_rate(&mut self, bytes_per_sec: f64) {
        self.rate = bytes_per_sec.max(0.0);
    }

    /// Drains the last clicked action.
    ///
    /// ```
    /// use martensite::widgets::download_item::DownloadItem;
    ///
    /// assert_eq!(DownloadItem::new("a", 1).take_action(), None);
    /// ```
    pub fn take_action(&mut self) -> Option<DownloadAction> {
        self.action.take()
    }

    fn action_buttons(&self) -> Vec<DownloadAction> {
        match self.state {
            DownloadState::Downloading => vec![DownloadAction::Pause, DownloadAction::Cancel],
            DownloadState::Paused => vec![DownloadAction::Resume, DownloadAction::Cancel],
            DownloadState::Done => vec![DownloadAction::ShowInFolder],
            DownloadState::Failed => vec![DownloadAction::Retry],
            DownloadState::Canceled => vec![],
        }
    }
}

fn glyph_for(action: DownloadAction) -> &'static str {
    match action {
        DownloadAction::Pause => "⏸",
        DownloadAction::Resume => "▶",
        DownloadAction::Cancel => "✕",
        DownloadAction::ShowInFolder => "📂",
        DownloadAction::Retry => "↻",
    }
}

impl Widget for DownloadItem {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let s = cx.scale;
        Vec2::new(
            (280.0 * s).min(constraints.max_size.x.max(0.0)),
            ((PAD_PT * 2.0 + NAME_PT + 3.0 + BAR_PT + 3.0 + INFO_PT) * s)
                .min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(160.0, 40.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        let s = cx.scale;
        let pad = PAD_PT * s;
        let bs = BTN_PT * s;
        // First action lands rightmost (primary affordance).
        self.buttons.clear();
        let mut x = bounds.max_x() - pad - bs;
        for action in self.action_buttons() {
            self.buttons.push((
                Rect::new(x, bounds.min_y() + (bounds.height() - bs) / 2.0, bs, bs),
                action,
            ));
            x -= bs + GAP_PT * s;
        }
        let icon = ICON_PT * s;
        self.bar_rect = Rect::new(
            bounds.min_x() + pad + icon + GAP_PT * s,
            bounds.min_y() + bounds.height() - pad - BAR_PT * s - INFO_PT * s - 3.0 * s,
            (x - pad - icon - GAP_PT * 2.0 * s - bounds.min_x()).max(0.0),
            BAR_PT * s,
        );
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::ListItem);
        node.set_label(format!("{}: {}", self.label, self.name));
        node.set_value(format!(
            "{} — {:.0}%",
            self.state.info(self.fraction, self.rate),
            self.fraction * 100.0
        ));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if let WidgetEvent::PointerReleased {
            button: PointerButton::Primary,
            position,
        } = cx.event
        {
            for (rect, action) in &self.buttons {
                if rect.contains(*position) {
                    self.action = Some(*action);
                    return EventResponse::RequestRepaint;
                }
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
            &martensite_core::shape::Shape::rounded(5.0 * s),
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
            cx.color(TokenKey::BorderColor, EDGE),
        );
        let pad = PAD_PT * s;
        // Icon.
        let ic = ICON_PT * s;
        let iy = b.min_y() + pad;
        cx.list.push_fill_shape(
            kurbo::Rect::new(
                f64::from(b.min_x() + pad),
                f64::from(iy),
                f64::from(b.min_x() + pad + ic),
                f64::from(iy + ic),
            ),
            &martensite_core::shape::Shape::rounded(4.0 * s),
            ICON_BG,
        );
        crate::text_paint::paint_label(
            painter,
            cx.list,
            kurbo::Point::new(
                f64::from(b.min_x() + pad + ic * 0.2),
                f64::from(iy + ic * 0.72),
            ),
            &self.glyph,
            INFO_PT * s,
            TEXT_FG,
        );
        // Name + info line.
        let tx = b.min_x() + pad + ic + GAP_PT * s;
        let nfs = NAME_PT * s;
        crate::text_paint::paint_label(
            painter,
            cx.list,
            kurbo::Point::new(f64::from(tx), f64::from(b.min_y() + pad + nfs)),
            &self.name,
            nfs,
            cx.color(TokenKey::TextColor, TEXT_FG),
        );
        let info = self.state.info(self.fraction, self.rate);
        let info_fg = if self.state == DownloadState::Failed {
            cx.color(TokenKey::ErrorColor, [210, 90, 80, 255])
        } else {
            MUTED_FG
        };
        crate::text_paint::paint_label(
            painter,
            cx.list,
            kurbo::Point::new(f64::from(tx), f64::from(b.max_y() - pad - INFO_PT * s)),
            &info,
            INFO_PT * s,
            info_fg,
        );
        // Progress bar.
        let br = self.bar_rect;
        if self.state != DownloadState::Done && br.width() > 0.0 {
            cx.list.push_fill_rect(
                kurbo::Rect::new(
                    f64::from(br.min_x()),
                    f64::from(br.min_y()),
                    f64::from(br.max_x()),
                    f64::from(br.max_y()),
                ),
                BAR_BG,
            );
            cx.list.push_fill_rect(
                kurbo::Rect::new(
                    f64::from(br.min_x()),
                    f64::from(br.min_y()),
                    f64::from(br.min_x() + br.width() * self.fraction),
                    f64::from(br.max_y()),
                ),
                cx.color(TokenKey::AccentColor, [90, 140, 220, 255]),
            );
        }
        // Action buttons.
        let bfs = INFO_PT * s;
        for (rect, action) in &self.buttons {
            cx.list.push_fill_shape(
                kurbo::Rect::new(
                    f64::from(rect.min_x()),
                    f64::from(rect.min_y()),
                    f64::from(rect.max_x()),
                    f64::from(rect.max_y()),
                ),
                &martensite_core::shape::Shape::rounded(4.0 * s),
                ICON_BG,
            );
            let g = glyph_for(*action);
            let w = g.len() as f32 * bfs * 0.7;
            crate::text_paint::paint_label(
                painter,
                cx.list,
                kurbo::Point::new(
                    f64::from(rect.min_x() + (rect.width() - w) / 2.0),
                    f64::from(rect.min_y() + rect.height() / 2.0 + bfs * 0.35),
                ),
                g,
                bfs,
                TEXT_FG,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(d: &mut DownloadItem) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        d.layout(&mut cx, Rect::new(0.0, 0.0, 300.0, 56.0));
    }

    fn click(d: &mut DownloadItem, r: Rect) {
        d.event(&mut EventContext {
            event: &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: Vec2::new(r.min_x() + r.width() / 2.0, r.min_y() + r.height() / 2.0),
            },
            bounds: d.bounds,
            scale: 1.0,
        });
    }

    #[test]
    fn pause_click_parks_action() {
        let mut d = DownloadItem::new("setup.dmg", 1_000_000);
        laid_out(&mut d);
        // Buttons: Pause rightmost, Cancel left of it (reversed fill).
        let r = d.buttons[0].0;
        click(&mut d, r);
        assert_eq!(d.take_action(), Some(DownloadAction::Pause));
    }

    #[test]
    fn done_shows_folder_only() {
        let mut d = DownloadItem::new("a", 1);
        d.set_state(DownloadState::Done);
        laid_out(&mut d);
        assert_eq!(d.buttons.len(), 1);
        assert_eq!(d.buttons[0].1, DownloadAction::ShowInFolder);
    }

    #[test]
    fn info_line_states() {
        assert_eq!(DownloadState::Paused.info(0.0, 0.0), "Paused");
        assert_eq!(DownloadState::Downloading.info(0.5, 0.0), "50%");
    }

    #[test]
    fn paint_without_painter() {
        let mut d = DownloadItem::new("a", 1).progress(0.4);
        laid_out(&mut d);
        let mut list = martensite_core::PaintList::default();
        let theme = martensite_theme::Theme::new("test");
        d.paint(&mut PaintContext {
            list: &mut list,
            bounds: d.bounds,
            theme: &theme,
            scale: 1.0,
            text_painter: None,
        });
        assert!(!list.is_empty());
    }
}
