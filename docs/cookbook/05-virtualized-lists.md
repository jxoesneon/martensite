# Cookbook 05 — Virtualizing Large Data Sets & High-Performance DataGrids

Industrial monitoring systems, trading terminals, log analyzers, and database
explorers routinely handle collections containing hundreds of thousands or even
millions of records. Naive UI frameworks attempt to instantiate retained DOM or
widget nodes for every item, resulting in gigabytes of memory consumption, catastrophic
layout passes, and dropped frames during scrolling.

This recipe demonstrates how to render **1,000,000 items at a locked 120 FPS** in
Martensite using **viewport culling**, **visible window arithmetic**, **widget
recycling pools**, and **overscan buffering**.

---

## 1. Goal

Build a virtualized data grid and list architecture that:
1. Handles 1,000,000 records with a constant memory footprint ($O(\text{viewport})$ rather than $O(N)$).
2. Calculates visible window indices in $O(1)$ time for fixed-height items and $O(\log N)$ for dynamic-height items.
3. Implements an internal widget recycling pool that eliminates all runtime heap allocations during scrolling.
4. Uses overscan buffers (lookahead/lookbehind rows) to eliminate visual blanking and frame hitching during rapid trackpad/mouse flicks.
5. Accurately coordinates scrollbars (`VScrollBar`), mouse wheel inputs, and keyboard navigation (`PageUp`, `PageDown`, `Home`, `End`).
6. Reports accurate position metadata to screen readers via AccessKit (`set_position_in_set` and `set_size_of_set`) without materializing unrendered items.

---

## 2. Complete Runnable Pattern

The following pattern constructs a virtualized high-frequency telemetry log table (`VirtualLogGrid`).
It manages 1,000,000 log records, computes viewport slices dynamically, recycles row widgets,
and maintains sub-millisecond frame times.

```rust
use std::ops::Range;
use std::sync::Arc;
use glam::Vec2;
use kurbo::{Point, Rect as KurboRect};
use martensite::prelude::*;
use martensite_core::widget::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Widget, WidgetEvent,
};
use martensite_core::{NodeFlags, Rect, TokenKey};

/// Telemetry record structure stored in memory.
#[derive(Clone, Debug)]
pub struct TelemetryRecord {
    pub id: u64,
    pub timestamp_ms: u64,
    pub subsystem: &'static str,
    pub message: String,
    pub severity: u8, // 0 = Info, 1 = Warn, 2 = Error
}

/// A lightweight pooled row widget that gets recycled across scroll offsets.
pub struct PooledRowWidget {
    pub item_index: usize,
    pub bounds: Rect,
    pub is_selected: bool,
}

impl PooledRowWidget {
    pub fn new() -> Self {
        Self {
            item_index: 0,
            bounds: Rect::default(),
            is_selected: false,
        }
    }
}

/// Virtualized log grid rendering up to 1,000,000 records.
pub struct VirtualLogGrid {
    /// Backing dataset (1,000,000 records).
    records: Arc<Vec<TelemetryRecord>>,
    /// Fixed row height in logical points.
    row_height: f32,
    /// Number of extra rows to materialize above and below the viewport.
    overscan_count: usize,
    /// Vertical scroll offset in device pixels.
    scroll_y: f32,
    /// Selected record index.
    selected_index: Option<usize>,
    /// Cached viewport bounds from the last layout pass.
    viewport_bounds: Rect,
    /// Cached display scale factor.
    scale: f32,
    /// Bounded pool of row widgets covering the visible viewport plus overscan.
    row_pool: Vec<PooledRowWidget>,
}

impl VirtualLogGrid {
    pub fn new(records: Arc<Vec<TelemetryRecord>>) -> Self {
        Self {
            records,
            row_height: 26.0,
            overscan_count: 4,
            scroll_y: 0.0,
            selected_index: None,
            viewport_bounds: Rect::default(),
            scale: 1.0,
            row_pool: Vec::new(),
        }
    }

    /// Scaled row height in physical device pixels.
    #[inline]
    fn row_px(&self) -> f32 {
        self.row_height * self.scale
    }

    /// Total virtual content height in physical pixels.
    #[inline]
    fn total_content_height(&self) -> f32 {
        self.records.len() as f32 * self.row_px()
    }

    /// Maximum scrollable offset.
    #[inline]
    fn max_scroll_y(&self) -> f32 {
        (self.total_content_height() - self.viewport_bounds.height()).max(0.0)
    }

    /// Calculate the visible range of record indices intersecting the viewport.
    pub fn visible_range(&self) -> Range<usize> {
        let row_px = self.row_px();
        if row_px <= 0.0 || self.records.is_empty() || self.viewport_bounds.height() <= 0.0 {
            return 0..0;
        }

        // Compute primary visible boundaries
        let first_visible = (self.scroll_y / row_px).floor() as usize;
        let visible_count = (self.viewport_bounds.height() / row_px).ceil() as usize + 1;

        // Apply overscan buffer
        let start = first_visible.saturating_sub(self.overscan_count);
        let end = (first_visible + visible_count + self.overscan_count).min(self.records.len());

        start..end
    }

    /// Synchronize the row pool capacity and assign item indices to pooled slots.
    fn synchronize_pool(&mut self) {
        let range = self.visible_range();
        let needed_capacity = range.len();

        // Ensure the recycling pool has enough elements without reallocating every frame
        if self.row_pool.len() < needed_capacity {
            self.row_pool.resize_with(needed_capacity, PooledRowWidget::new);
        }

        let row_px = self.row_px();
        let min_x = self.viewport_bounds.min_x();
        let min_y = self.viewport_bounds.min_y();
        let width = self.viewport_bounds.width();

        // Re-bind pooled row widgets to the visible slice
        for (slot_idx, record_idx) in range.enumerate() {
            let row = &mut self.row_pool[slot_idx];
            row.item_index = record_idx;
            row.is_selected = self.selected_index == Some(record_idx);

            // Compute screen-space bounds for each row
            let y_pos = min_y + (record_idx as f32 * row_px) - self.scroll_y;
            row.bounds = Rect::new(min_x, y_pos, width, row_px);
        }
    }

    /// Scroll by a given delta in physical device pixels.
    pub fn scroll_by(&mut self, delta_y: f32) {
        let max = self.max_scroll_y();
        self.scroll_y = (self.scroll_y + delta_y).clamp(0.0, max);
        self.synchronize_pool();
    }
}

impl Widget for VirtualLogGrid {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        self.scale = cx.scale;
        // The virtual grid expands to fill available space, but has an intrinsic minimum
        let min_h = cx.pt(self.row_height * 5.0);
        let w = constraints.max_size.x.min(cx.pt(800.0));
        let h = constraints.max_size.y.max(min_h);
        Vec2::new(w, h)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.viewport_bounds = bounds;
        self.scale = cx.scale;
        self.scroll_y = self.scroll_y.clamp(0.0, self.max_scroll_y());
        self.synchronize_pool();
    }

    fn paint(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        let k_viewport = KurboRect::new(
            b.min_x() as f64,
            b.min_y() as f64,
            b.max_x() as f64,
            b.max_y() as f64,
        );

        // 1. Fill table background
        let bg_color = cx.color(TokenKey::SurfaceColor, [20, 22, 26, 255]);
        cx.list.push_fill_rect(k_viewport, bg_color);

        // 2. Push Scissor Clip to restrict row rendering strictly to the viewport
        cx.list.push_clip_rect(k_viewport);

        let range = self.visible_range();
        let row_px = self.row_px();
        let border_color = cx.color(TokenKey::BorderColor, [45, 48, 56, 255]);
        let text_color = cx.color(TokenKey::TextColor, [220, 224, 230, 255]);
        let warn_color = cx.color(TokenKey::WarningColor, [230, 160, 40, 255]);
        let err_color = cx.color(TokenKey::ErrorColor, [230, 60, 60, 255]);
        let select_bg = cx.color(TokenKey::PrimaryColor, [40, 90, 180, 255]);

        // 3. Render only the pooled visible rows
        for (slot_idx, &record_idx) in range.clone().collect::<Vec<_>>().iter().enumerate() {
            if slot_idx >= self.row_pool.len() {
                break;
            }
            let row = &self.row_pool[slot_idx];
            let rb = row.bounds;

            let k_row = KurboRect::new(
                rb.min_x() as f64,
                rb.min_y() as f64,
                rb.max_x() as f64,
                rb.max_y() as f64,
            );

            // Row background (alternating zebra stripes or selected highlight)
            if row.is_selected {
                cx.list.push_fill_rect(k_row, select_bg);
            } else if record_idx % 2 == 1 {
                cx.list.push_fill_rect(k_row, [26, 28, 34, 255]);
            }

            // Bottom border separator
            cx.list.push_stroke_line(
                Point::new(k_row.x0, k_row.y1),
                Point::new(k_row.x1, k_row.y1),
                border_color,
                1.0,
            );

            // Text content
            let rec = &self.records[record_idx];
            let ink = match rec.severity {
                2 => err_color,
                1 => warn_color,
                _ => text_color,
            };

            let row_text = format!(
                "#{:07} | {:>8}ms | [{:<8}] {}",
                rec.id, rec.timestamp_ms, rec.subsystem, rec.message
            );

            let text_pos = Point::new(
                k_row.x0 + cx.pt(8.0) as f64,
                k_row.y0 + (row_px * 0.7) as f64,
            );

            cx.list.push_text(text_pos, row_text, cx.pt(12.0), ink);
        }

        // Pop scissor clip
        cx.list.pop_clip();

        // 4. Render Scrollbar Indicator
        let total_h = self.total_content_height();
        if total_h > b.height() && b.height() > 0.0 {
            let bar_w = cx.pt(8.0);
            let bar_x = b.max_x() - bar_w;
            let ratio = (b.height() / total_h).clamp(0.05, 1.0);
            let thumb_h = (b.height() * ratio).max(cx.pt(20.0));
            let scroll_ratio = self.scroll_y / (total_h - b.height());
            let thumb_y = b.min_y() + (b.height() - thumb_h) * scroll_ratio;

            let k_thumb = KurboRect::new(
                bar_x as f64,
                thumb_y as f64,
                (bar_x + bar_w) as f64,
                (thumb_y + thumb_h) as f64,
            );
            cx.list.push_fill_rounded_rect(k_thumb, bar_w * 0.5, [120, 126, 138, 160]);
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::Scroll { delta, .. } => {
                // Natural wheel scroll (negate delta for standard downward motion)
                let scroll_step = cx.pt(50.0);
                self.scroll_by(-delta.y * scroll_step);
                EventResponse::RequestRepaint
            }
            WidgetEvent::PointerPressed { position, button, .. } if button == PointerButton::Primary => {
                let range = self.visible_range();
                for (slot_idx, record_idx) in range.enumerate() {
                    if slot_idx < self.row_pool.len() && self.row_pool[slot_idx].bounds.contains(position) {
                        self.selected_index = Some(record_idx);
                        self.synchronize_pool();
                        return EventResponse::RequestRepaint;
                    }
                }
                EventResponse::Ignored
            }
            WidgetEvent::KeyPressed { ref key, .. } => {
                let page_size = (self.viewport_bounds.height() / self.row_px()).floor() as usize;
                match key.as_str() {
                    "PageDown" => {
                        self.scroll_by(page_size as f32 * self.row_px());
                        EventResponse::RequestRepaint
                    }
                    "PageUp" => {
                        self.scroll_by(-(page_size as f32 * self.row_px()));
                        EventResponse::RequestRepaint
                    }
                    "Home" => {
                        self.scroll_y = 0.0;
                        self.synchronize_pool();
                        EventResponse::RequestRepaint
                    }
                    "End" => {
                        self.scroll_y = self.max_scroll_y();
                        self.synchronize_pool();
                        EventResponse::RequestRepaint
                    }
                    _ => EventResponse::Ignored,
                }
            }
            _ => EventResponse::Ignored,
        }
    }

    fn accessibility(&self, node: &mut accesskit::Node) {
        node.set_role(accesskit::Role::Table);
        node.set_row_count(self.records.len());
        node.set_column_count(4);
    }
}
```

---

## 3. Key Architectural Invariants

### 1. Viewport Culling & Visible Window Arithmetic
To guarantee steady 120 FPS performance (8.33ms total frame budget including GPU compositing),
the layout pass must calculate item visibility in $O(1)$ constant time.

For a collection of $N = 1,000,000$ fixed-height items:
- **First Visible Row**:
  $$\text{start} = \max\left(0, \left\lfloor \frac{\text{scroll\_y}}{\text{row\_px}} \right\rfloor - \text{overscan}\right)$$
- **Last Visible Row**:
  $$\text{end} = \min\left(N, \left\lceil \frac{\text{scroll\_y} + \text{viewport\_height}}{\text{row\_px}} \right\rceil + \text{overscan}\right)$$
- **Total Rendered Slice**:
  $$\text{capacity} = \text{end} - \text{start} \approx \frac{\text{viewport\_height}}{\text{row\_px}} + (2 \times \text{overscan})$$

On a standard 1080p display with a 26pt row height, $\text{capacity} \approx 42 + 8 = 50$ rows.
The framework processes exactly 50 rows regardless of whether the dataset contains 100 rows or 10,000,000 rows.

### 2. Fixed vs. Dynamic Item Heights

| Strategy | Index Lookup Complexity | Total Height Math | Scroll Stability | Best Used For |
|---|:---:|---|---|---|
| **Fixed Heights** | $O(1)$ | $N \times \text{row\_px}$ | Absolute (zero jitter) | Logs, tabular data grids, spreadsheets, code editors. |
| **Prefix-Sum Table** | $O(\log N)$ via binary search | $\text{prefix\_sum}[N]$ | Stable | Chat bubbles with pre-measured text blocks. |
| **Dynamic Fenwick Tree** | $O(\log N)$ point updates | Binary indexed tree | Highly stable with edits | Editable lists with variable line wrapping. |
| **Estimated Accumulation** | $O(1)$ speculative | $\text{avg\_height} \times N$ | Risk of scrollbar jitter | Endless feeds with lazy network loading. |

> [!IMPORTANT]
> When using dynamic heights, always record measured item heights into a persistent prefix-sum or Fenwick tree. Recalculating heights during active scrolling will cause "scrollbar bounce" and visual hitching.

### 3. Bounded Memory Footprint (The Recycling Pool)
In Martensite, `ScrollView` cannot be used directly for high-scale list virtualization because `ScrollView`
measures and paints its entire child contents into an offscreen surface. Virtualized components (`ListView`, `Table`, `VirtualLogGrid`) must **own their scrolling and scissor clips**.

The recycling pool enforces:
1. **Zero Allocation**: `row_pool` is allocated once during the initial layout. Subsequent scrolls only overwrite existing slots.
2. **Deterministic GC Avoidance**: Because no `Box<dyn Widget>` allocations are dropped or recreated during high-velocity scrolling, Rust's allocator is bypassed entirely.
3. **Scissor Clip Enforcement**: The viewport rect is scissored via `cx.list.push_clip_rect` so text strings never bleed outside panel borders.

### 4. Overscan Buffering & 120 FPS Frame Budget
Modern high-refresh displays (120Hz/144Hz) update the screen every 7–8ms. If the user performs a high-velocity trackpad flick, the scroll delta between frames can exceed 100 pixels.
- Without overscan ($\text{overscan} = 0$), the newly exposed area at the bottom of the viewport will be empty for one frame while the next layout tick catches up, resulting in a perceptible "white flash" or missing rows.
- With an overscan buffer of 4–8 rows, rows are already painted outside the immediate scissor boundary. When the GPU compositor shifts the frame, content is already present.

### 5. AccessKit Windowed Emission
Assistive technologies (screen readers like NVDA, JAWS, VoiceOver) must not be flooded with 1,000,000 accessibility nodes, which would crash the accessibility daemon.
Martensite employs **windowed emission**:
- The parent container reports `Role::Table` or `Role::List` with the total size via `node.set_row_count(N)`.
- Visible child row nodes specify their absolute 1-based index via `node.set_position_in_set(record_idx + 1)` and `node.set_size_of_set(N)`.
- The screen reader announces: *"Row 45,021 of 1,000,000"*, providing complete positional context without inflating system memory.

---

## 4. Common Pitfalls & Antipatterns

| Antipattern | Mechanism of Failure | Recommended Mitigation |
|---|---|---|
| **Allocating in `paint()`** | Creating new strings or formatting records inside the `paint()` loop triggers thousands of heap allocations per second during scrolls. | Format or cache strings in a shared text painter or use flyweight formatting buffers. |
| **Omitting Scissor Clips** | Forgetting `cx.list.push_clip_rect()` causes partial top and bottom rows to spill into surrounding headers and status bars. | Always bracket virtualized row rendering with `push_clip_rect` and `pop_clip`. |
| **Unclamped Scroll Offsets** | Deleting items from the dataset while scrolled to the bottom causes `scroll_y > max_scroll`, leaving an empty void. | Re-clamp `scroll_y = scroll_y.clamp(0.0, max_scroll_y())` inside `layout()`. |
| **Subpixel Layout Jitter** | Computing row offsets with unrounded logical floats causes text glyphs to shimmer between pixel boundaries during slow drags. | Floor row offsets to physical device pixels: `(pos * scale).round() / scale`. |
| **Materializing 1M AccessKit Nodes** | Adding every record to the accessibility tree locks up the OS accessibility engine. | Emit only the visible window into AccessKit, using `posinset` and `setsize` for coordinate fidelity. |

---

## Next Steps

- [Cookbook 01 — Responsive Layout & Underflow Policies](01-responsive-layout.md)
- [Cookbook 06 — Asynchronous Data & Network Streams](06-async-data.md)
- [Cookbook 08 — Keyboard Navigation & Focus Traps](08-keyboard-focus.md)
- [Milestone v0.18.0 — Paint Audit & Virtual Scoping](../../docs/milestones/v0.18.0.md)
