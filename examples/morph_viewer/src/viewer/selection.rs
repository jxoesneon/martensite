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
        self.sync_wells();
        self.sync_ghosts();
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
        self.sync_wells();
        self.rebuild_display();
        self.cascade();
        self.semantic_dirty = true;
    }

    pub(super) fn hit_tab(&self, p: Vec2) -> Option<usize> {
        self.tab_rects.iter().position(|r| r.contains(p))
    }

    /// Slot of the pooled cell under `pos`, honoring the pixel
    /// scroll offset and the wall clip. Partially visible top rows
    /// hit-test correctly (a point in the sliver still lands on its
    /// row).
    pub(super) fn hit_cell(&self, pos: Vec2) -> Option<usize> {
        if !self.wall_rect.contains(pos) {
            return None;
        }
        let s = self.scale.max(0.01);
        let pitch = self.pitch_icon_pt().0.max(1.0);
        let local_x = (pos.x - self.wall_rect.origin.x) / s;
        let local_y = (pos.y - self.wall_rect.origin.y) / s;
        let row = ((self.scroll_pt + local_y) / pitch).floor();
        let col = (local_x / pitch).floor();
        if row < 0.0 || col < 0.0 || col as usize >= self.cols {
            return None;
        }
        let v = self.vrows();
        let items = v.visible_items();
        let item = row as usize * self.cols + col as usize;
        if item < items.start || item >= items.end {
            return None;
        }
        let slot = item - items.start;
        (slot < self.cells.len() && self.cells[slot].filled).then_some(slot)
    }

    /// Forwards pointer/keyboard events to the interactive internal
    /// children (wells, transport, segmented speed, zoom). The
    /// default trait path doesn't reach them because we manage our
    /// own child dispatch order.
    pub(super) fn forward_controls(&mut self, cx: &mut EventContext) -> EventResponse {
        // Releases must reach an armed child even when the pointer
        // left its bounds mid-press.
        let release = matches!(cx.event, WidgetEvent::PointerReleased { .. });
        let hit = |r: Option<&Rect>| -> bool {
            release
                || match cx.event {
                    WidgetEvent::PointerMoved { position }
                    | WidgetEvent::PointerPressed { position, .. }
                    | WidgetEvent::PointerReleased { position, .. } => {
                        r.is_some_and(|r| r.contains(*position))
                    }
                    _ => r.is_some(),
                }
        };
        let er = &self.extra_rects;
        // Fixed child-rect slots: base well, target well, play,
        // speed segments, zoom controls.
        let rects = [
            er.first().copied(),
            er.get(1).copied(),
            er.get(2).copied(),
            er.get(3).copied(),
            er.get(4).copied(),
        ];
        macro_rules! fwd {
            ($w:expr, $i:expr) => {
                if hit(rects[$i].as_ref()) {
                    match $w.event(cx) {
                        EventResponse::Ignored => {}
                        other => return other,
                    }
                }
            };
        }
        fwd!(self.well_base, 0);
        fwd!(self.well_target, 1);
        fwd!(self.play_btn, 2);
        fwd!(self.speed_seg, 3);
        fwd!(self.zoom_ctl, 4);
        EventResponse::Ignored
    }

    /// Updates the hover-driven tooltip target + well hover states
    /// from a pointer position. Ghosts re-sync only when the hovered
    /// CELL changes (the caller compares before `sync_ghosts`).
    pub(super) fn update_hover_target(&mut self, pos: Vec2) {
        let wb = self.extra_rects.first().is_some_and(|r| r.contains(pos));
        let wt = self.extra_rects.get(1).is_some_and(|r| r.contains(pos));
        self.well_base.set_hovered(wb);
        self.well_target.set_hovered(wt);
        self.play_btn
            .set_hovered(self.extra_rects.get(2).is_some_and(|r| r.contains(pos)));
        let target = if self.hover_cell.is_some() {
            // A grid hover's tooltip stays on the cell itself; the
            // feedforward ghost lives in the wells.
            self.hover_cell.map(TipFor::Cell)
        } else if wb && self.well_base.tooltip().is_some() {
            Some(TipFor::WellBase)
        } else if wt && self.well_target.tooltip().is_some() {
            Some(TipFor::WellTarget)
        } else if self.hover_funnel {
            Some(TipFor::Funnel)
        } else if self.hover_filter {
            Some(TipFor::FilterPill)
        } else {
            None
        };
        if target != self.tip_for {
            self.tip_for = target;
            self.hover_since = Some(Instant::now());
        }
    }

    /// Binds translucent ghosts into the wells per the feedforward
    /// rules. Called only when the hovered cell (or selection)
    /// changed — ghosts snap, they never animate.
    pub(super) fn sync_ghosts(&mut self) {
        let hover_d: Option<&'static str> = self.hover_cell.and_then(|slot| {
            self.cells
                .get(slot)
                .filter(|c| c.filled)
                .and_then(|c| icon_at(c.pack, c.icon_idx))
                .map(|i| i.d)
        });
        let target_d = self
            .target_sel
            .and_then(|(p, i)| icon_at(p, i))
            .map(|i| i.d);
        let (gb, gt) = match (self.base_sel, self.target_sel, hover_d) {
            // Nothing selected — hover ghosts into base.
            (None, _, Some(h)) => (Some(h), None),
            // Base only — hover previews the target slot.
            (Some(_), None, Some(h)) => (None, Some(h)),
            // Both — target slot shows the hover, base previews the
            // displaced target (chain preview).
            (Some(_), Some(_), Some(h)) => (target_d, Some(h)),
            _ => (None, None),
        };
        self.well_base.set_ghost(gb);
        self.well_target.set_ghost(gt);
    }

    /// Syncs the well widgets' bound icons + names from the
    /// selection state (tooltips/labels derive inside the well).
    pub(super) fn sync_wells(&mut self) {
        let bind = |sel: Option<Sel>| -> Option<(&IconDef, &'static str)> {
            let (p, i) = sel?;
            let icon = icon_at(p, i)?;
            let pack = icons::all_packs().get(p as usize)?.name;
            Some((icon, pack))
        };
        self.well_base.set(bind(self.base_sel));
        self.well_target.set(bind(self.target_sel));
    }

    /// Tooltip text for a hover target.
    pub(super) fn tip_text(&self, t: TipFor) -> Option<String> {
        match t {
            TipFor::Cell(slot) => {
                let c = self.cells.get(slot)?;
                if !c.filled {
                    return None;
                }
                let icon = icon_at(c.pack, c.icon_idx)?;
                let pack = icons::all_packs()
                    .get(c.pack as usize)
                    .map_or("?", |p| p.name);
                // Cross-pack pinned icons say where they came from.
                if c.pack != self.last_pack {
                    Some(format!("{} · {}", icon.name, pack))
                } else {
                    Some(icon.name.to_string())
                }
            }
            TipFor::WellBase | TipFor::WellTarget => {
                let w = if t == TipFor::WellBase {
                    &self.well_base
                } else {
                    &self.well_target
                };
                w.tooltip()
            }
            TipFor::Funnel => Some("filter icons".to_string()),
            TipFor::FilterPill => Some("type to filter this pack".to_string()),
        }
    }

    /// Anchor rect for a tooltip target.
    pub(super) fn tip_anchor(&self, t: TipFor) -> Option<Rect> {
        match t {
            TipFor::Cell(slot) => self.cell_rects.get(slot).copied(),
            TipFor::WellBase => self.extra_rects.first().copied(),
            TipFor::WellTarget => self.extra_rects.get(1).copied(),
            TipFor::Funnel => Some(self.funnel_rect),
            TipFor::FilterPill => Some(self.filter_rect),
        }
    }
}
