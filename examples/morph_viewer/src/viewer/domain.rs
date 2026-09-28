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

    /// Maximum scroll in logical pt — the wall clamps here.
    pub(super) fn max_scroll_pt(&self) -> f32 {
        let total_rows = self.display_len().div_ceil(self.cols.max(1));
        (total_rows as f32 * CELL - self.grid_rect.size.y / self.scale.max(0.01)).max(0.0)
    }

    /// Clamps `scroll_pt` into `[0, max]` and syncs the `scroll`
    /// signal (integer top row).
    pub(super) fn clamp_scroll(&mut self) {
        self.scroll_pt = self.scroll_pt.clamp(0.0, self.max_scroll_pt());
        let row = (self.scroll_pt / CELL).round().max(0.0) as u32;
        if row != self.last_scroll {
            self.last_scroll = row;
            self.signals.scroll.set(row);
        }
    }

    /// The `(pack, icon)` a grid slot shows at the current scroll —
    /// `None` past the last entry.
    pub(super) fn slot_icon(&self, slot: usize) -> Option<Sel> {
        self.display.get(slot).copied()
    }

    /// Rebinds every visible cell to its slot — statically, the grid
    /// never morphs: it shows icons in their original state.
    pub(super) fn cascade(&mut self) {
        if self.display_len() == 0 {
            // No entries — re-lay each icon into a zero rect: `paint`
            // honors `MorphIcon`'s stored bounds, so this is the only
            // way to keep stale glyphs out from under the message.
            let mut tmp = martensite::core::HotNode::default();
            for cell in self.cells.iter_mut().take(self.shown) {
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
        // A just-cleared filter may find the icons still zero-laid —
        // restore the inset rect each cascade (cheap, idempotent).
        let icon_side = ICON_PT * self.scale;
        let mut tmp = martensite::core::HotNode::default();
        for (i, cell) in self.cells.iter_mut().enumerate().take(self.shown) {
            let cr = self.cell_rects[i];
            if self.icon_rects[i].size.x != icon_side {
                let inset = (cr.size.x - icon_side) * 0.5;
                self.icon_rects[i] = Rect::new(
                    cr.origin.x + inset,
                    cr.origin.y + inset,
                    icon_side,
                    icon_side,
                );
            }
            let r = self.icon_rects[i];
            let mut cx = LayoutContext {
                hot: &mut tmp,
                scale: self.scale,
            };
            cell.icon.layout(&mut cx, r);
        }
        // The wall scrolls in whole rows: slot i shows
        // `display[top_row * cols + i]`; slots past the end are empty.
        let top_row = self.last_scroll as usize;
        let base = top_row * self.cols.max(1);
        let targets: Vec<Option<Sel>> = (0..self.shown).map(|s| self.slot_icon(base + s)).collect();
        for (cell, target) in self.cells.iter_mut().take(self.shown).zip(targets) {
            match target {
                Some((p, i)) => {
                    cell.filled = true;
                    cell.pack = p;
                    cell.icon_idx = i;
                    if let Some(icon) = icon_at(p, i) {
                        set_icon_checked(&mut cell.icon, icon.d);
                    }
                }
                None => {
                    // Empty slot — zero-lay the glyph and mark unfilled
                    // so hover/picks ignore it.
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
