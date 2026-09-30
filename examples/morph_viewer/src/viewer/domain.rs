use super::*;

impl MorphViewer {
    /// Rebuilds `filtered` from `last_filter` + `last_category` over
    /// the current pack, ordered by `last_sort`. An empty filter maps
    /// the identity so wall math sees the whole pack.
    pub(super) fn recompute_filtered(&mut self) {
        let Some(pack) = self.pack() else {
            self.filtered.clear();
            return;
        };
        let needle = self.last_filter.to_lowercase();
        let cat = self.last_category.to_lowercase();
        let mut v: Vec<u32> = pack
            .icons
            .iter()
            .enumerate()
            .filter(|(_, i)| {
                let name = i.name.to_lowercase();
                (needle.is_empty() || name.contains(&needle))
                    && (cat.is_empty()
                        || name.starts_with(&cat) && name.as_bytes().get(cat.len()) == Some(&b'-'))
            })
            .map(|(i, _)| i as u32)
            .collect();
        match self.last_sort.as_str() {
            "az" => v.sort_by_key(|&i| pack.icons[i as usize].name),
            "za" => v.sort_by_key(|&i| std::cmp::Reverse(pack.icons[i as usize].name)),
            _ => {}
        }
        self.filtered = v;
    }

    /// Natural matches for the current filters — the pill's count.
    pub(super) fn filtered_len(&self) -> usize {
        self.filtered.len()
    }

    /// First-token categories in the current pack with ≥ 6 icons,
    /// most common first — the popover's category choices.
    pub(super) fn categories(&self) -> Vec<String> {
        let Some(pack) = self.pack() else {
            return Vec::new();
        };
        let mut counts: std::collections::BTreeMap<String, usize> =
            std::collections::BTreeMap::new();
        for icon in pack.icons {
            if let Some(head) = icon.name.split('-').next() {
                *counts.entry(head.to_string()).or_default() += 1;
            }
        }
        let mut v: Vec<(String, usize)> = counts.into_iter().filter(|(_, n)| *n >= 6).collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        v.truncate(12);
        v.into_iter().map(|(s, _)| s).collect()
    }

    /// Rebuilds the display list: selections pin to the front ONLY
    /// when absent from the natural results — an icon already shown
    /// holds its grid position.
    pub(super) fn rebuild_display(&mut self) {
        self.display.clear();
        let in_results = |s: Sel| s.0 == self.last_pack && self.filtered.contains(&s.1);
        if let Some(b) = self.base_sel.filter(|s| !in_results(*s)) {
            self.display.push(b);
        }
        if let Some(t) = self.target_sel.filter(|s| !in_results(*s)) {
            self.display.push(t);
        }
        let cur = self.last_pack;
        self.display
            .extend(self.filtered.iter().copied().map(|i| (cur, i)));
    }

    pub(super) fn display_len(&self) -> usize {
        self.display.len()
    }

    /// `(cell pitch, icon extent)` in logical pt for the active zoom
    /// level. Pre-first-layout (`last_zoom == MAX`) falls back to
    /// the default level.
    pub(super) fn pitch_icon_pt(&self) -> (f32, f32) {
        let z: usize = if self.last_zoom == u32::MAX {
            DEFAULT_ZOOM as usize
        } else {
            self.last_zoom.min(ZOOM_LEVELS.len() as u32 - 1) as usize
        };
        ZOOM_LEVELS[z]
    }

    /// Recomputes cell + icon rects from the current scroll offset —
    /// pixel-precise, partial rows included. Slots are
    /// `item - visible.start` so the pooled cells, their rects, and
    /// `cascade`'s bindings all agree.
    pub(super) fn position_cells(&mut self, cx: &mut LayoutContext) {
        let s = self.scale.max(0.01);
        let (pitch_pt, icon_pt) = self.pitch_icon_pt();
        let pitch = pitch_pt * s;
        let icon_side = icon_pt * s;
        let v = self.vrows();
        let items = v.visible_items();
        let first = items.start;
        let wall = self.wall_rect;
        let cols = self.cols;
        let offset_px = v.offset() * s;
        // Horizontally center the wall's columns inside the viewport.
        let ox = wall.origin.x + (wall.size.x - cols as f32 * pitch).max(0.0) * 0.5;
        self.cell_rects = vec![Rect::default(); self.cells.len()];
        self.icon_rects = vec![Rect::default(); self.cells.len()];
        for it in items {
            let slot = it - first;
            if slot >= self.cells.len() {
                break;
            }
            let (col, row) = (it % cols, it / cols);
            let cell = Rect::new(
                ox + col as f32 * pitch,
                wall.origin.y + row as f32 * pitch - offset_px,
                pitch,
                pitch,
            );
            let inset = (pitch - icon_side) * 0.5;
            let icon_rect = Rect::new(
                cell.origin.x + inset,
                cell.origin.y + inset,
                icon_side,
                icon_side,
            );
            self.cell_rects[slot] = cell;
            self.icon_rects[slot] = icon_rect;
            cx.layout_child(&mut self.cells[slot].icon, icon_rect);
        }
    }

    /// Wall viewport extent in logical pt.
    pub(super) fn wall_h_pt(&self) -> f32 {
        self.wall_rect.size.y / self.scale.max(0.01)
    }

    /// The wall's `VirtualRows` at the current geometry/offset.
    pub(super) fn vrows(&self) -> VirtualRows {
        let (pitch, _) = self.pitch_icon_pt();
        let mut v = VirtualRows::new(
            self.display_len(),
            self.cols.max(1),
            pitch.max(1.0),
            self.wall_h_pt().max(0.0),
        );
        v.set_offset(self.scroll_pt);
        v
    }

    /// Maximum scroll in logical pt — the wall clamps here.
    pub(super) fn max_scroll_pt(&self) -> f32 {
        self.vrows().max_offset()
    }

    /// Clamps `scroll_pt` into `[0, max]` and syncs the `scroll`
    /// signal (integer top row).
    pub(super) fn clamp_scroll(&mut self) {
        let (pitch, _) = self.pitch_icon_pt();
        self.scroll_pt = self.scroll_pt.clamp(0.0, self.max_scroll_pt());
        let row = (self.scroll_pt / pitch.max(1.0)).floor().max(0.0) as u32;
        if row != self.last_scroll {
            self.last_scroll = row;
            self.signals.scroll.set(row);
        }
    }

    /// Scrolls by `delta_pt` logical pt and rebinds; returns whether
    /// the offset actually moved (0 at either bound = chaining).
    pub(super) fn scroll_keys(&mut self, delta_pt: f32) -> bool {
        let mut v = self.vrows();
        let consumed = v.scroll_by(delta_pt);
        self.scroll_pt = v.offset();
        self.clamp_scroll();
        if consumed.abs() > f32::EPSILON {
            self.reposition_and_cascade();
            true
        } else {
            false
        }
    }

    /// Sets the pixel offset so `row` is the top row (scroll-signal
    /// semantics), clamped.
    pub(super) fn scroll_to_row(&mut self, row: u32) {
        let (pitch, _) = self.pitch_icon_pt();
        self.scroll_pt = row as f32 * pitch.max(1.0);
        self.clamp_scroll();
        self.reposition_and_cascade();
    }

    /// Repositions pooled cells from the pixel offset, then rebinds
    /// the newly exposed items.
    pub(super) fn reposition_and_cascade(&mut self) {
        let mut hot = martensite::core::HotNode::default();
        let mut lcx = LayoutContext {
            hot: &mut hot,
            scale: self.scale,
        };
        self.position_cells(&mut lcx);
        self.cascade();
    }

    /// The `(pack, icon)` a grid slot shows at the current scroll —
    /// `None` past the last entry.
    pub(super) fn slot_icon(&self, slot: usize) -> Option<Sel> {
        self.display.get(slot).copied()
    }

    /// Rebinds every pooled cell to the item `VirtualRows` places
    /// in its slot (`item - visible.start`). Statically, the grid
    /// never morphs: it shows icons in their original state.
    pub(super) fn cascade(&mut self) {
        let mut tmp = martensite::core::HotNode::default();
        if self.display_len() == 0 || self.cells.is_empty() {
            // No entries — re-lay each icon into a zero rect: `paint`
            // honors `MorphIcon`'s stored bounds, so this is the only
            // way to keep stale glyphs out from under the message.
            for cell in &mut self.cells {
                cell.filled = false;
                let mut cx = LayoutContext {
                    hot: &mut tmp,
                    scale: self.scale,
                };
                cell.icon.layout(&mut cx, Rect::default());
            }
            self.semantic_dirty = true;
            return;
        }
        let items = self.vrows().visible_items();
        let first = items.start;
        // `targets[slot]` is the display entry for slot `slot`.
        let targets: Vec<Option<Sel>> = items.clone().map(|it| self.slot_icon(it)).collect();
        for (slot, cell) in self.cells.iter_mut().enumerate() {
            let target = targets.get(slot).copied().flatten();
            match target {
                Some((p, i)) => {
                    cell.filled = true;
                    cell.pack = p;
                    cell.icon_idx = i;
                    cell.item = first + slot;
                    if let Some(icon) = icon_at(p, i) {
                        set_icon_checked(&mut cell.icon, icon.d);
                    }
                }
                None => {
                    // Slot outside the visible range or past the end —
                    // zero-lay the glyph so stale icons never linger.
                    cell.filled = false;
                    let mut cx = LayoutContext {
                        hot: &mut tmp,
                        scale: self.scale,
                    };
                    cell.icon.layout(&mut cx, Rect::default());
                }
            }
        }
        self.semantic_dirty = true;
    }
}
