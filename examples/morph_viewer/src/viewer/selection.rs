use super::*;

impl MorphViewer {
    /// Idle ambient pick: hero morphs to a random filtered icon.
    pub(super) fn morph_hero_to(&mut self, icon_idx: usize) {
        if let Some(pack) = self.pack() {
            let len = pack.icons.len().max(1);
            self.hero_pack_i = self.last_pack;
            self.hero_icon_idx = (icon_idx % len) as u32;
            let icon = &pack.icons[self.hero_icon_idx as usize];
            morph_hero_checked(&mut self.hero, icon.d);
            self.hero.set_label(icon.name.to_string());
            self.semantic_dirty = true;
        }
        self.hero_idle_s = 0.0;
    }

    /// Hero snapshot for a settled selection state.
    pub(super) fn snap_hero(&mut self, sel: Sel) {
        if let Some(icon) = icon_at(sel.0, sel.1) {
            set_icon_checked(&mut self.hero, icon.d);
            self.hero.set_label(icon.name.to_string());
            self.hero_pack_i = sel.0;
            self.hero_icon_idx = sel.1;
            self.semantic_dirty = true;
        }
    }

    /// Writes the selection signals so MCP clients observe clicks,
    /// and updates the `last_*` shadows so `tick` doesn't re-apply
    /// the same write back.
    pub(super) fn sync_sel_signals(&mut self) {
        let b = self.base_sel.map_or(-1, |s| i64::from(global_id(s.0, s.1)));
        let t = self
            .target_sel
            .map_or(-1, |s| i64::from(global_id(s.0, s.1)));
        self.last_base = b;
        self.last_target = t;
        self.signals.base.set(b);
        self.signals.target.set(t);
    }

    /// Applies the current selection to the hero: a pair loops
    /// `snap base → morph target`; a lone base snaps statically.
    pub(super) fn apply_selection(&mut self) {
        match (self.base_sel, self.target_sel) {
            (Some(b), Some(t)) => {
                self.snap_hero(b);
                let bname = self.sel_name(b);
                if let Some(icon) = icon_at(t.0, t.1) {
                    morph_hero_checked(&mut self.hero, icon.d);
                    self.hero.set_label(format!("{bname} → {}", icon.name));
                    self.hero_pack_i = t.0;
                    self.hero_icon_idx = t.1;
                }
                self.loop_at_base = false;
                self.loop_rest_s = LOOP_HOLD_TARGET_S;
            }
            (Some(b), None) => {
                self.snap_hero(b);
                self.loop_at_base = true;
            }
            _ => {}
        }
        self.hero_idle_s = 0.0;
        self.semantic_dirty = true;
    }

    pub(super) fn sel_name(&self, sel: Sel) -> &'static str {
        icon_at(sel.0, sel.1).map_or("?", |i| i.name)
    }

    /// Click semantics: deselect an already-selected icon (deselecting
    /// base promotes the target), otherwise chain base → target.
    pub(super) fn pick(&mut self, sel: Sel) {
        if self.base_sel == Some(sel) {
            self.base_sel = self.target_sel.take();
        } else if self.target_sel == Some(sel) {
            self.target_sel = None;
        } else {
            match (self.base_sel, self.target_sel) {
                (None, _) => self.base_sel = Some(sel),
                (Some(_), None) => self.target_sel = Some(sel),
                (Some(_), Some(t)) => {
                    self.base_sel = Some(t);
                    self.target_sel = Some(sel);
                }
            }
        }
        // The `select` signal reports the last clicked icon.
        let gid = global_id(sel.0, sel.1);
        self.last_select = gid;
        self.signals.select.set(gid);
        self.sync_sel_signals();
        self.apply_selection();
        self.rebuild_display();
        self.cascade();
    }

    /// Drops both selections — the hero returns to idle morphing.
    pub(super) fn clear_selection(&mut self) {
        if self.base_sel.is_none() && self.target_sel.is_none() {
            return;
        }
        self.base_sel = None;
        self.target_sel = None;
        self.sync_sel_signals();
        self.rebuild_display();
        self.cascade();
        self.semantic_dirty = true;
    }

    pub(super) fn hit_cell(&self, p: Vec2) -> Option<usize> {
        if self.display.is_empty() || !self.grid_rect.contains(p) {
            return None;
        }
        self.cell_rects
            .iter()
            .take(self.shown)
            .position(|r| r.contains(p))
            .filter(|&i| self.cells[i].filled)
    }

    pub(super) fn hit_tab(&self, p: Vec2) -> Option<usize> {
        self.tab_rects.iter().position(|r| r.contains(p))
    }
}
