//! `StepSequencer` — an instrument×step toggle grid with a
//! tick-driven playhead column (TR-808 / DAW step-sequencer idiom).
//!
//! Rows are instrument lanes, columns are beats. Click toggles a
//! cell and parks `(row, col, on)` in
//! [`StepSequencer::take_changed`]; `tick` advances the lit
//! playhead column while [`StepSequencer::set_playing`] is on, and
//! every advance parks the column in [`StepSequencer::take_step`]
//! so the host can trigger sounds. The leftmost lane column shows
//! instrument labels.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::step_sequencer::StepSequencer;
//!
//! let s = StepSequencer::new(4, 16);
//! assert_eq!(s.row_count(), 4);
//! ```

use std::time::Duration;

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::text_paint::SharedTextPainter;

const CELL_PT: f32 = 20.0;
const CELL_GAP_PT: f32 = 4.0;
const LABEL_W_PT: f32 = 64.0;
const FONT_PT: f32 = 12.0;

const OFF: [u8; 4] = [36, 38, 44, 255];
const ON: [u8; 4] = [88, 130, 247, 255];
const ON_BEAT: [u8; 4] = [120, 155, 255, 255];
const EDGE: [u8; 4] = [58, 60, 68, 255];
const PLAY_COL: [u8; 4] = [46, 160, 67, 70];
const MUTED: [u8; 4] = [139, 148, 158, 255];

/// A step grid — see the module docs.
///
/// ```
/// use martensite::widgets::step_sequencer::StepSequencer;
///
/// assert_eq!(StepSequencer::new(2, 8).col_count(), 8);
/// ```
pub struct StepSequencer {
    /// Accessibility label.
    pub label: String,
    /// Seconds per step while playing.
    pub step_secs: f32,
    /// Instrument lane labels.
    pub lanes: Vec<String>,
    grid: Vec<Vec<bool>>,
    rows: usize,
    cols: usize,
    playing: bool,
    play_col: usize,
    step_clock: f32,
    changed: Option<(usize, usize, bool)>,
    stepped: Option<usize>,
    held: Option<(usize, usize)>,
    cells: Vec<Vec<Rect>>,
    text_painter: Option<SharedTextPainter>,
    bounds: Rect,
    scale: f32,
}

impl std::fmt::Debug for StepSequencer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StepSequencer")
            .field("rows", &self.rows)
            .field("cols", &self.cols)
            .field("playing", &self.playing)
            .finish()
    }
}

impl StepSequencer {
    /// Empty `rows`×`cols` grid.
    ///
    /// ```
    /// use martensite::widgets::step_sequencer::StepSequencer;
    ///
    /// assert_eq!(StepSequencer::new(3, 8).row_count(), 3);
    /// ```
    pub fn new(rows: usize, cols: usize) -> Self {
        Self {
            label: "Step sequencer".to_string(),
            step_secs: 0.125,
            lanes: Vec::new(),
            grid: vec![vec![false; cols]; rows],
            rows,
            cols,
            playing: false,
            play_col: 0,
            step_clock: 0.0,
            changed: None,
            stepped: None,
            held: None,
            cells: Vec::new(),
            text_painter: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Lane labels builder (up to `rows` used).
    ///
    /// ```
    /// use martensite::widgets::step_sequencer::StepSequencer;
    ///
    /// assert_eq!(StepSequencer::new(2, 4).lanes(["kick", "snare"]).lanes.len(), 2);
    /// ```
    pub fn lanes<I, S>(mut self, lanes: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.lanes = lanes.into_iter().map(Into::into).collect();
        self
    }

    /// Step-duration builder.
    ///
    /// ```
    /// use martensite::widgets::step_sequencer::StepSequencer;
    ///
    /// assert_eq!(StepSequencer::new(1, 4).step_secs(0.25).step_secs, 0.25);
    /// ```
    pub fn step_secs(mut self, secs: f32) -> Self {
        self.step_secs = secs.max(0.01);
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::step_sequencer::StepSequencer;
    ///
    /// assert_eq!(StepSequencer::new(1, 4).label("Drums").label, "Drums");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Initial cell states from `(row, col)` pairs.
    ///
    /// ```
    /// use martensite::widgets::step_sequencer::StepSequencer;
    ///
    /// assert!(StepSequencer::new(2, 4).cells_on([(0, 1)]).cell(0, 1));
    /// ```
    pub fn cells_on<I>(mut self, cells: I) -> Self
    where
        I: IntoIterator<Item = (usize, usize)>,
    {
        for (r, c) in cells {
            if r < self.rows && c < self.cols {
                self.grid[r][c] = true;
            }
        }
        self
    }

    /// Shared text painter for lane labels.
    ///
    /// ```no_run
    /// use martensite::widgets::step_sequencer::StepSequencer;
    /// # fn example(p: martensite::text_paint::SharedTextPainter) {
    /// let _s = StepSequencer::new(1, 4).with_text_painter(p);
    /// # }
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Row count.
    ///
    /// ```
    /// use martensite::widgets::step_sequencer::StepSequencer;
    ///
    /// assert_eq!(StepSequencer::new(4, 8).row_count(), 4);
    /// ```
    pub fn row_count(&self) -> usize {
        self.rows
    }

    /// Column count.
    ///
    /// ```
    /// use martensite::widgets::step_sequencer::StepSequencer;
    ///
    /// assert_eq!(StepSequencer::new(4, 8).col_count(), 8);
    /// ```
    pub fn col_count(&self) -> usize {
        self.cols
    }

    /// Cell state.
    ///
    /// ```
    /// use martensite::widgets::step_sequencer::StepSequencer;
    ///
    /// assert!(!StepSequencer::new(1, 4).cell(0, 0));
    /// ```
    pub fn cell(&self, row: usize, col: usize) -> bool {
        self.grid
            .get(row)
            .and_then(|r| r.get(col))
            .copied()
            .unwrap_or(false)
    }

    /// Sets a cell programmatically (no `take_changed` — host-driven).
    ///
    /// ```
    /// use martensite::widgets::step_sequencer::StepSequencer;
    ///
    /// let mut s = StepSequencer::new(1, 4);
    /// s.set_cell(0, 2, true);
    /// assert!(s.cell(0, 2));
    /// ```
    pub fn set_cell(&mut self, row: usize, col: usize, on: bool) {
        if let Some(c) = self.grid.get_mut(row).and_then(|r| r.get_mut(col)) {
            *c = on;
        }
    }

    /// Clears all cells.
    ///
    /// ```
    /// use martensite::widgets::step_sequencer::StepSequencer;
    ///
    /// let mut s = StepSequencer::new(1, 2).cells_on([(0, 0)]);
    /// s.clear();
    /// assert!(!s.cell(0, 0));
    /// ```
    pub fn clear(&mut self) {
        for row in &mut self.grid {
            row.fill(false);
        }
    }

    /// Whether the playhead runs.
    ///
    /// ```
    /// use martensite::widgets::step_sequencer::StepSequencer;
    ///
    /// assert!(!StepSequencer::new(1, 4).is_playing());
    /// ```
    pub fn is_playing(&self) -> bool {
        self.playing
    }

    /// Starts/stops the playhead.
    ///
    /// ```
    /// use martensite::widgets::step_sequencer::StepSequencer;
    ///
    /// let mut s = StepSequencer::new(1, 4);
    /// s.set_playing(true);
    /// assert!(s.is_playing());
    /// ```
    pub fn set_playing(&mut self, playing: bool) {
        self.playing = playing;
        if playing {
            self.step_clock = 0.0;
        }
    }

    /// Current playhead column.
    ///
    /// ```
    /// use martensite::widgets::step_sequencer::StepSequencer;
    ///
    /// assert_eq!(StepSequencer::new(1, 4).play_col(), 0);
    /// ```
    pub fn play_col(&self) -> usize {
        self.play_col
    }

    /// Drains the last toggled `(row, col, on)`.
    ///
    /// ```
    /// use martensite::widgets::step_sequencer::StepSequencer;
    ///
    /// let mut s = StepSequencer::new(1, 4);
    /// assert_eq!(s.take_changed(), None);
    /// ```
    pub fn take_changed(&mut self) -> Option<(usize, usize, bool)> {
        self.changed.take()
    }

    /// Drains the last playhead column advance.
    ///
    /// ```
    /// use martensite::widgets::step_sequencer::StepSequencer;
    ///
    /// let mut s = StepSequencer::new(1, 4);
    /// assert_eq!(s.take_step(), None);
    /// ```
    pub fn take_step(&mut self) -> Option<usize> {
        self.stepped.take()
    }

    /// Cell hit-test.
    fn cell_at(&self, p: Vec2) -> Option<(usize, usize)> {
        for (r, row) in self.cells.iter().enumerate() {
            for (c, rect) in row.iter().enumerate() {
                if rect.contains(p) {
                    return Some((r, c));
                }
            }
        }
        None
    }
}

impl Widget for StepSequencer {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let s = cx.scale;
        let labels = if self.lanes.is_empty() {
            0.0
        } else {
            LABEL_W_PT * s
        };
        let w = labels + self.cols as f32 * (CELL_PT + CELL_GAP_PT) * s;
        let h = self.rows as f32 * (CELL_PT + CELL_GAP_PT) * s;
        Vec2::new(
            w.min(constraints.max_size.x.max(0.0)),
            h.min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(60.0, 24.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        let s = cx.scale;
        let labels = if self.lanes.is_empty() {
            0.0
        } else {
            LABEL_W_PT * s
        };
        let gx = bounds.min_x() + labels;
        let gw = bounds.width() - labels;
        let cell_w = if self.cols > 0 {
            (gw - self.cols as f32 * CELL_GAP_PT * s) / self.cols as f32
        } else {
            0.0
        };
        let cell_h = if self.rows > 0 {
            (bounds.height() - self.rows as f32 * CELL_GAP_PT * s) / self.rows as f32
        } else {
            0.0
        };
        self.cells = (0..self.rows)
            .map(|r| {
                (0..self.cols)
                    .map(|c| {
                        Rect::new(
                            gx + c as f32 * (cell_w + CELL_GAP_PT * s),
                            bounds.min_y() + r as f32 * (cell_h + CELL_GAP_PT * s),
                            cell_w.max(0.0),
                            cell_h.max(0.0),
                        )
                    })
                    .collect()
            })
            .collect();
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Grid);
        node.set_label(self.label.clone());
        let on = self.grid.iter().flatten().filter(|c| **c).count();
        node.set_value(format!("{on} steps on"));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                ..
            } => {
                if let Some(rc) = self.cell_at(*position) {
                    self.held = Some(rc);
                    return EventResponse::CapturePointer;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position,
            } => {
                if let Some((r, c)) = self.held.take() {
                    if self.cell_at(*position) == Some((r, c)) {
                        let on = !self.grid[r][c];
                        self.grid[r][c] = on;
                        self.changed = Some((r, c, on));
                    }
                    return EventResponse::ReleasePointer;
                }
                EventResponse::Ignored
            }
            WidgetEvent::KeyPressed { key, .. } => match key.as_str() {
                // Space toggles play/stop — the DAW convention.
                " " | "Space" => {
                    self.set_playing(!self.playing);
                    EventResponse::RequestRepaint
                }
                _ => EventResponse::Ignored,
            },
            _ => EventResponse::Ignored,
        }
    }

    fn tick(&mut self, dt: Duration) -> bool {
        if !self.playing || self.cols == 0 {
            return false;
        }
        self.step_clock += dt.as_secs_f32();
        if self.step_clock >= self.step_secs {
            self.step_clock -= self.step_secs;
            self.play_col = (self.play_col + 1) % self.cols;
            self.stepped = Some(self.play_col);
        }
        true
    }

    fn paint(&self, cx: &mut PaintContext) {
        let s = cx.scale;
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let krect = |r: Rect| {
            kurbo::Rect::new(
                f64::from(r.min_x()),
                f64::from(r.min_y()),
                f64::from(r.max_x()),
                f64::from(r.max_y()),
            )
        };
        let edge = cx.color(TokenKey::DividerColor, EDGE);
        // Lane labels.
        for (r, lane) in self.lanes.iter().take(self.rows).enumerate() {
            if let Some(row) = self.cells.get(r).and_then(|r| r.first()) {
                let o = kurbo::Point::new(
                    f64::from(self.bounds.min_x()),
                    f64::from(row.min_y() + row.height() / 2.0),
                );
                crate::text_paint::paint_label(
                    painter,
                    cx.list,
                    o,
                    lane,
                    FONT_PT * s,
                    cx.color(TokenKey::TextMutedColor, MUTED),
                );
            }
        }
        // Playhead column highlight under the cells.
        if self.playing {
            if let Some(col_rects) = self.cells.first() {
                if let Some(first) = col_rects.get(self.play_col) {
                    let col = kurbo::Rect::new(
                        f64::from(first.min_x()),
                        f64::from(self.bounds.min_y()),
                        f64::from(first.max_x()),
                        f64::from(self.bounds.max_y()),
                    );
                    cx.list.push_fill_rect(col, PLAY_COL);
                }
            }
        }
        // Cells.
        for r in 0..self.rows {
            for c in 0..self.cols {
                let rect = self.cells[r][c];
                let on = self.grid[r][c];
                let face = if on {
                    // Downbeat columns (every 4th) get a brighter on-cell.
                    if c % 4 == 0 {
                        cx.color(TokenKey::AccentColor, ON_BEAT)
                    } else {
                        cx.color(TokenKey::SecondaryColor, ON)
                    }
                } else {
                    cx.color(TokenKey::SurfaceColor, OFF)
                };
                cx.list.push_fill_rect(krect(rect), face);
                // Cell edge on a state color — pick whichever reads.
                let cell_edge = crate::text_paint::better_ink(face, edge, [20, 20, 24, 255]);
                cx.list.push_stroke_rect(krect(rect), 0.5 * s, cell_edge);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(s: &mut StepSequencer) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        s.layout(&mut cx, Rect::new(0.0, 0.0, 400.0, 100.0));
    }

    fn ev(s: &mut StepSequencer, e: &WidgetEvent) -> EventResponse {
        s.event(&mut EventContext {
            event: e,
            bounds: s.bounds,
            scale: 1.0,
        })
    }

    fn tap_cell(s: &mut StepSequencer, r: usize, c: usize) {
        let rect = s.cells[r][c];
        let mid = Vec2::new(
            (rect.min_x() + rect.max_x()) / 2.0,
            (rect.min_y() + rect.max_y()) / 2.0,
        );
        ev(
            s,
            &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: mid,
                count: 1,
            },
        );
        ev(
            s,
            &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: mid,
            },
        );
    }

    #[test]
    fn click_toggles_and_parks() {
        let mut s = StepSequencer::new(2, 8);
        laid_out(&mut s);
        tap_cell(&mut s, 1, 3);
        assert!(s.cell(1, 3));
        assert_eq!(s.take_changed(), Some((1, 3, true)));
        tap_cell(&mut s, 1, 3);
        assert!(!s.cell(1, 3));
        assert_eq!(s.take_changed(), Some((1, 3, false)));
    }

    #[test]
    fn playhead_advances_and_parks() {
        let mut s = StepSequencer::new(1, 4).step_secs(0.1);
        laid_out(&mut s);
        s.set_playing(true);
        assert!(s.tick(Duration::from_millis(150)));
        assert_eq!(s.play_col(), 1);
        assert_eq!(s.take_step(), Some(1));
        // Wraps.
        for _ in 0..3 {
            s.tick(Duration::from_millis(150));
        }
        assert_eq!(s.play_col(), 0);
        assert_eq!(s.take_step(), Some(0));
    }

    #[test]
    fn space_toggles_play() {
        let mut s = StepSequencer::new(1, 4);
        laid_out(&mut s);
        ev(
            &mut s,
            &WidgetEvent::KeyPressed {
                key: " ".to_string(),
                repeat: false,
            },
        );
        assert!(s.is_playing());
        ev(
            &mut s,
            &WidgetEvent::KeyPressed {
                key: " ".to_string(),
                repeat: false,
            },
        );
        assert!(!s.is_playing());
    }

    #[test]
    fn stopped_tick_idles() {
        let mut s = StepSequencer::new(1, 4);
        laid_out(&mut s);
        assert!(!s.tick(Duration::from_secs(1)));
        assert_eq!(s.play_col(), 0);
    }

    #[test]
    fn paint_without_painter() {
        let mut s = StepSequencer::new(2, 8)
            .lanes(["kick", "snare"])
            .cells_on([(0, 0), (1, 4)]);
        laid_out(&mut s);
        s.set_playing(true);
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
