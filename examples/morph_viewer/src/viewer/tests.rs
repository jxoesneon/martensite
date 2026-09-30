use super::*;
use martensite::core::{HotNode, LayoutContext};

fn signals() -> ViewerSignals {
    ViewerSignals {
        pack: Signal::new(0),
        scroll: Signal::new(0),
        paused: Signal::new(true), // deterministic: no ambient drift
        select: Signal::new(0),
        base: Signal::new(-1),
        target: Signal::new(-1),
        filter: Signal::new(String::new()),
        sort: Signal::new(String::new()),
        category: Signal::new(String::new()),
        zoom: Signal::new(1),
        speed: Signal::new(1),
    }
}

#[test]
fn every_bundled_icon_loads() {
    let mut icon = MorphIcon::new();
    let mut total = 0usize;
    for pack in icons::PACKS {
        for def in pack.icons {
            icon.set_icon(def.d)
                .unwrap_or_else(|e| panic!("{}/{} failed to load: {e}", pack.name, def.name));
            total += 1;
        }
    }
    let bundled: usize = icons::PACKS.iter().map(|p| p.icons.len()).sum();
    assert_eq!(total, bundled);
}

/// Centers the click on grid cell `i` and returns the press event.
fn click_cell(v: &mut MorphViewer, i: usize) {
    let cell = v.cell_rects[i];
    let p = Vec2::new(
        cell.origin.x + cell.size.x * 0.5,
        cell.origin.y + cell.size.y * 0.5,
    );
    let event = WidgetEvent::PointerPressed {
        position: p,
        button: PointerButton::Primary,
        count: 1,
    };
    let mut cx = EventContext {
        event: &event,
        bounds: v.bounds,
        scale: 1.0,
    };
    assert_eq!(v.event(&mut cx), EventResponse::Handled);
}

fn laid_out() -> MorphViewer {
    let mut v = MorphViewer::new(signals());
    let mut hot = HotNode::default();
    let mut cx = LayoutContext {
        hot: &mut hot,
        scale: 1.0,
    };
    v.layout(&mut cx, Rect::new(0.0, 0.0, 1320.0, 860.0));
    v
}

#[test]
fn layout_fills_grid_and_hero() {
    let v = laid_out();
    assert!(v.shown > 0);
    assert_eq!(v.cells.len(), v.shown);
    assert_eq!(v.cell_rects.len(), v.shown);
    assert_eq!(v.icon_rects.len(), v.shown);
    assert!(v.tab_rects.len() >= icons::PACKS.len());
}

#[test]
fn pack_signal_switches_pack_and_rebinds_cells() {
    let mut v = laid_out();
    v.tick(Duration::from_millis(16)); // initial cascade
    v.signals.pack.set(1);
    v.tick(Duration::from_millis(16));
    assert_eq!(v.last_pack, 1);
    // the wall's first screen shows the new pack's head slice
    for (slot, cell) in v.cells.iter().enumerate().take(v.shown) {
        assert!(cell.filled);
        assert_eq!((cell.pack, cell.icon_idx), (1, slot as u32));
    }
}

#[test]
fn scroll_signal_moves_the_wall_window() {
    let mut v = laid_out();
    v.tick(Duration::from_millis(16));
    let first_at_top = v.cells[0].icon_idx;
    v.signals.scroll.set(3);
    v.tick(Duration::from_millis(16));
    // scrolling three rows shifts every slot by 3*cols icons
    assert_eq!(v.last_scroll, 3);
    assert_eq!(v.cells[0].icon_idx, first_at_top + 3 * v.cols as u32);
    // clamped at the wall's end (fractional bottom row rounds)
    v.signals.scroll.set(u32::MAX);
    v.tick(Duration::from_millis(16));
    let pitch = v.pitch_icon_pt().0;
    let last_row = (v.max_scroll_pt() / pitch).floor() as usize;
    assert_eq!(v.last_scroll as usize, last_row);
    assert!((v.last_scroll as usize) < v.display_len().div_ceil(v.cols));
}

#[test]
fn scroll_event_and_keys_advance_rows() {
    let mut v = laid_out();
    v.tick(Duration::from_millis(16));
    let center = Vec2::new(
        v.wall_rect.origin.x + v.wall_rect.size.x * 0.5,
        v.wall_rect.origin.y + v.wall_rect.size.y * 0.5,
    );
    // Deltas arrive in device px (app layer converts lines); three
    // pitch-heights of wheel = 3 rows.
    let pitch = v.pitch_icon_pt().0;
    let scroll = WidgetEvent::Scroll {
        position: center,
        delta: Vec2::new(0.0, -3.0 * pitch),
    };
    let mut cx = EventContext {
        event: &scroll,
        bounds: v.bounds,
        scale: 1.0,
    };
    assert_eq!(v.event(&mut cx), EventResponse::Handled);
    assert_eq!(v.signals.scroll.get(), 3);
    // wheel up returns to the top
    let up = WidgetEvent::Scroll {
        position: center,
        delta: Vec2::new(0.0, 400.0),
    };
    v.event(&mut EventContext {
        event: &up,
        bounds: v.bounds,
        scale: 1.0,
    });
    assert_eq!(v.signals.scroll.get(), 0);
}

#[test]
fn select_signal_sets_base_and_snaps() {
    let mut v = laid_out();
    v.tick(Duration::from_millis(16));
    v.signals.select.set(7);
    v.tick(Duration::from_millis(16));
    // First pick = base: hero shows it instantly, no target yet.
    assert_eq!(v.base_sel, Some((0, 7)));
    assert_eq!(v.target_sel, None);
    assert_eq!((v.hero_pack_i, v.hero_icon_idx), (0, 7));
    assert_eq!(v.signals.base.get(), 7);
    assert_eq!(v.signals.target.get(), -1);
}

#[test]
fn pick_chain_and_deselect() {
    let mut v = laid_out();
    v.tick(Duration::from_millis(16));
    // In-results selections hold their cells — the wall never
    // reorders icons just because they got picked.
    click_cell(&mut v, 0);
    let s0 = (v.cells[0].pack, v.cells[0].icon_idx);
    click_cell(&mut v, 1);
    let s1 = (v.cells[1].pack, v.cells[1].icon_idx);
    assert_eq!(v.base_sel, Some(s0));
    assert_eq!(v.target_sel, Some(s1));
    assert_eq!((v.cells[0].pack, v.cells[0].icon_idx), s0);
    let s2 = (v.cells[2].pack, v.cells[2].icon_idx);
    click_cell(&mut v, 2);
    assert_eq!(v.base_sel, Some(s1));
    assert_eq!(v.target_sel, Some(s2));
    // deselect base (still cell 1) → target promotes
    click_cell(&mut v, 1);
    assert_eq!(v.base_sel, Some(s2));
    assert_eq!(v.target_sel, None);
    assert_eq!((v.hero_pack_i, v.hero_icon_idx), s2);
    // deselect the lone base (cell 2) → idle resumes
    click_cell(&mut v, 2);
    assert_eq!(v.base_sel, None);
}

#[test]
fn pair_loops_and_pause_freezes() {
    let mut v = laid_out();
    v.tick(Duration::from_millis(16));
    v.signals.base.set(1);
    v.signals.target.set(9);
    v.tick(Duration::from_millis(16));
    assert_eq!(v.base_sel, Some((0, 1)));
    assert_eq!(v.target_sel, Some((0, 9)));
    // paused: no loop leg transitions, hero stays where it is
    let mut animating_seen = false;
    v.signals.paused.set(false);
    for _ in 0..400 {
        v.tick(Duration::from_millis(16));
        animating_seen |= v.hero.is_animating();
    }
    assert!(animating_seen, "loop must morph base → target");
    v.signals.paused.set(true);
    let loop_state = (v.loop_at_base, v.hero_icon_idx);
    for _ in 0..120 {
        v.tick(Duration::from_millis(16));
    }
    assert_eq!(loop_state, (v.loop_at_base, v.hero_icon_idx));
}

#[test]
fn selection_pins_only_when_filtered_out() {
    let mut v = laid_out();
    v.tick(Duration::from_millis(16));
    // An icon already in the results holds its position — no pin.
    v.signals.base.set(3);
    v.tick(Duration::from_millis(16));
    assert_eq!(v.display[0], (0, 0));
    assert_eq!(v.display[3], (0, 3));
    // A filter that excludes the icon pins it to the front.
    let name3 = icons::PACKS[0].icons[3].name;
    let miss = if name3.contains("arrow") {
        "zzz"
    } else {
        "arrow"
    };
    v.signals.filter.set(miss.to_string());
    v.tick(Duration::from_millis(16));
    assert_eq!(v.display[0], (0, 3));
    // deselect → pin drops; icon only reappears if it matches
    v.signals.base.set(-1);
    v.tick(Duration::from_millis(16));
    assert_eq!(v.base_sel, None);
    assert!(!v.display.contains(&(0, 3)));
    assert_eq!(v.cells[0].icon_idx, v.display[0].1);
}

#[test]
fn category_and_sort_signals_narrow_results() {
    let mut v = laid_out();
    v.tick(Duration::from_millis(16));
    let pack_len = icons::PACKS[0].icons.len();
    // category: only icons named `<cat>-*` remain
    v.signals.category.set("arrow".to_string());
    v.tick(Duration::from_millis(16));
    assert!(v.filtered_len() > 0 && v.filtered_len() < pack_len);
    for &i in &v.filtered {
        assert!(icons::PACKS[0].icons[i as usize].name.starts_with("arrow-"));
    }
    // sort: a→z orders the matches lexicographically
    v.signals.sort.set("az".to_string());
    v.tick(Duration::from_millis(16));
    let mut sorted = v.filtered.clone();
    sorted.sort_by_key(|&i| icons::PACKS[0].icons[i as usize].name);
    assert_eq!(v.filtered, sorted);
}

#[test]
fn funnel_opens_overlay_popover_and_rows_apply() {
    let mut v = laid_out();
    v.tick(Duration::from_millis(16));
    let p = Vec2::new(
        v.funnel_rect.origin.x + v.funnel_rect.size.x * 0.5,
        v.funnel_rect.origin.y + v.funnel_rect.size.y * 0.5,
    );
    let press = WidgetEvent::PointerPressed {
        position: p,
        button: PointerButton::Primary,
        count: 1,
    };
    let mut cx = EventContext {
        event: &press,
        bounds: v.bounds,
        scale: 1.0,
    };
    v.event(&mut cx);
    assert!(v.panel_open);
    // `sync_overlay` opens a real overlay entry anchored above
    // the funnel pill — it paints above every window child.
    let mut overlay = OverlayLayer::new();
    overlay.set_viewport(v.bounds);
    overlay.set_scale_factor(1.0);
    v.sync_overlay(&mut overlay);
    overlay.layout_pass();
    assert_eq!(overlay.len(), 1);
    let eb = overlay.entries().next().unwrap().bounds();
    // Click the "name a → z" row (sort row index 1, inside the
    // popup's bounds — the layer routes it to the popup).
    let row_y = eb.origin.y + 6.0 + 16.0 + 1.5 * 20.0;
    overlay.dispatch_event(&WidgetEvent::PointerPressed {
        position: Vec2::new(eb.origin.x + 10.0, row_y),
        button: PointerButton::Primary,
        count: 1,
    });
    assert_eq!(v.signals.sort.get(), "az");
    // The pick lands as a domain change → the popover closes.
    v.tick(Duration::from_millis(16));
    assert!(!v.panel_open);
    v.sync_overlay(&mut overlay);
    assert!(overlay.is_empty());
    // Reopen, then an outside press light-dismisses it.
    v.panel_open = true;
    v.sync_overlay(&mut overlay);
    overlay.layout_pass();
    assert_eq!(overlay.len(), 1);
    overlay.dispatch_event(&WidgetEvent::PointerPressed {
        position: Vec2::new(10.0, 10.0),
        button: PointerButton::Primary,
        count: 1,
    });
    v.sync_overlay(&mut overlay);
    assert!(!v.panel_open);
    assert!(overlay.is_empty());
}

#[test]
fn empty_cells_ignore_hover_and_clicks() {
    let mut v = laid_out();
    v.signals.filter.set("arrow".to_string());
    v.tick(Duration::from_millis(16));
    // a sparse result leaves trailing cells unfilled
    let unfilled = v
        .cells
        .iter()
        .position(|c| !c.filled)
        .expect("filtered result should leave empty slots");
    let r = v.cell_rects[unfilled];
    let press = WidgetEvent::PointerPressed {
        position: Vec2::new(r.origin.x + r.size.x * 0.5, r.origin.y + r.size.y * 0.5),
        button: PointerButton::Primary,
        count: 1,
    };
    let mut cx = EventContext {
        event: &press,
        bounds: v.bounds,
        scale: 1.0,
    };
    assert_eq!(v.event(&mut cx), EventResponse::Ignored);
    assert_eq!(v.base_sel, None);
}

#[test]
fn escape_clears_selection_and_idles() {
    let mut v = laid_out();
    v.tick(Duration::from_millis(16));
    v.signals.base.set(5);
    v.tick(Duration::from_millis(16));
    assert_eq!(v.base_sel, Some((0, 5)));
    let esc = WidgetEvent::KeyPressed {
        key: "Escape".to_string(),
        repeat: false,
    };
    let mut cx = EventContext {
        event: &esc,
        bounds: v.bounds,
        scale: 1.0,
    };
    v.event(&mut cx);
    assert_eq!(v.base_sel, None);
    assert_eq!(v.signals.base.get(), -1);
    // unpaused idle resumes morphing
    v.signals.paused.set(false);
    let start = v.hero_icon_idx;
    for _ in 0..400 {
        v.tick(Duration::from_millis(16));
        if v.hero_icon_idx != start {
            return;
        }
    }
    panic!("hero should resume idle morphing after Esc");
}

#[test]
fn tab_click_sets_pack_signal() {
    let mut v = laid_out();
    let tab = v.tab_rects[1];
    let p = Vec2::new(
        tab.origin.x + tab.size.x * 0.5,
        tab.origin.y + tab.size.y * 0.5,
    );
    let event = WidgetEvent::PointerPressed {
        position: p,
        button: PointerButton::Primary,
        count: 1,
    };
    let mut cx = EventContext {
        event: &event,
        bounds: v.bounds,
        scale: 1.0,
    };
    let r = v.event(&mut cx);
    assert_eq!(r, EventResponse::Handled);
    assert_eq!(v.signals.pack.get(), 1);
}

#[test]
fn cell_click_snaps_hero_to_base() {
    let mut v = laid_out();
    v.tick(Duration::from_millis(16));
    let target = 3usize.min(v.shown - 1);
    let tapped = (v.cells[target].pack, v.cells[target].icon_idx);
    click_cell(&mut v, target);
    assert_eq!(v.base_sel, Some(tapped));
    assert_eq!((v.hero_pack_i, v.hero_icon_idx), tapped);
}

#[test]
fn filter_narrows_grid_and_clamps_page() {
    let mut v = laid_out();
    v.tick(Duration::from_millis(16));
    assert_eq!(v.filtered_len(), v.pack().map(|p| p.icons.len()).unwrap());
    v.signals.filter.set("arrow".to_string());
    v.tick(Duration::from_millis(16));
    assert!(v.filtered_len() > 0);
    assert!(v.filtered_len() < v.pack().map(|p| p.icons.len()).unwrap());
    // every filled cell resolves to an icon whose name matches
    let pack = v.pack().unwrap();
    for cell in v.cells.iter().take(v.shown).filter(|c| c.filled) {
        assert!(pack.icons[cell.icon_idx as usize].name.contains("arrow"));
    }
    // the scroll clamps inside the filtered wall
    v.signals.scroll.set(u32::MAX);
    v.tick(Duration::from_millis(16));
    let pitch = v.pitch_icon_pt().0;
    let last_row = (v.max_scroll_pt() / pitch).floor() as usize;
    assert_eq!(v.last_scroll as usize, last_row);
    // clearing restores the identity domain
    v.signals.filter.set(String::new());
    v.tick(Duration::from_millis(16));
    assert_eq!(v.filtered_len(), pack.icons.len());
}

#[test]
fn filter_no_match_shows_empty_state() {
    let mut v = laid_out();
    v.signals.filter.set("zzzz-no-such-icon".to_string());
    v.tick(Duration::from_millis(16));
    assert_eq!(v.filtered_len(), 0);
    assert_eq!(v.display_len(), 0);
    assert!(v.cells.iter().take(v.shown).all(|c| !c.filled));
}

#[test]
fn ime_commit_appends_and_escape_clears() {
    let mut v = laid_out();
    let commit = WidgetEvent::ImeCommitted {
        text: "che".to_string(),
    };
    let mut cx = EventContext {
        event: &commit,
        bounds: v.bounds,
        scale: 1.0,
    };
    assert_eq!(v.event(&mut cx), EventResponse::Handled);
    assert_eq!(v.signals.filter.get(), "che");
    let esc = WidgetEvent::KeyPressed {
        key: "Escape".to_string(),
        repeat: false,
    };
    let mut cx = EventContext {
        event: &esc,
        bounds: v.bounds,
        scale: 1.0,
    };
    v.event(&mut cx);
    assert_eq!(v.signals.filter.get(), "");
    assert!(!v.filter_focused);
}

#[test]
fn idle_morphs_when_no_selection() {
    let mut v = laid_out();
    v.signals.paused.set(false);
    v.tick(Duration::from_millis(16));
    let start = v.hero_icon_idx;
    // ~4s of ticks > HERO_IDLE_S — hero must have moved on.
    for _ in 0..260 {
        v.tick(Duration::from_millis(16));
        if v.hero_icon_idx != start {
            return;
        }
    }
    panic!("hero should idle-morph with no selection");
}

fn relayout(v: &mut MorphViewer, w: f32, h: f32) {
    let mut hot = HotNode::default();
    let mut cx = LayoutContext {
        hot: &mut hot,
        scale: 1.0,
    };
    v.layout(&mut cx, Rect::new(0.0, 0.0, w, h));
}

/// B2: a resize (grow or shrink) must rebind every pooled cell to the
/// item `VirtualRows` says is visible in that slot — the pre-fix bug
/// left newly grown slots unbound until the next scroll.
#[test]
fn resize_rebinds_every_visible_cell() {
    let check = |v: &MorphViewer| {
        let items = v.vrows().visible_items();
        for (slot, cell) in v.cells.iter().enumerate().take(v.shown) {
            let item = items.start + slot;
            match v.slot_icon(item) {
                Some(sel) if item < items.end => {
                    assert!(
                        cell.filled,
                        "slot {slot} (item {item}) unbound after resize"
                    );
                    assert_eq!((cell.pack, cell.icon_idx), sel);
                }
                _ => assert!(!cell.filled, "slot {slot} stale-filled"),
            }
        }
    };
    // Grow: the compositor's launch resize exposes brand-new slots.
    let mut v = MorphViewer::new(signals());
    relayout(&mut v, 700.0, 480.0);
    v.tick(Duration::from_millis(16));
    check(&v);
    let small_cols = v.cols;
    relayout(&mut v, 1600.0, 1100.0);
    v.tick(Duration::from_millis(16));
    assert!(v.cols > small_cols, "grow must widen the grid");
    check(&v);
    // Shrink back: fewer visible rows, same invariant.
    relayout(&mut v, 700.0, 480.0);
    v.tick(Duration::from_millis(16));
    check(&v);
    // Scrolled state survives a mid-scroll resize too.
    v.signals.scroll.set(20);
    v.tick(Duration::from_millis(16));
    relayout(&mut v, 1600.0, 1100.0);
    v.tick(Duration::from_millis(16));
    check(&v);
    let max_row = v.display_len().div_ceil(v.cols.max(1));
    assert!((v.last_scroll as usize) <= max_row);
}

/// B1: the wall scrolls in pixels — a sub-row offset lands mid-row
/// and the signal still reports the integer top row.
#[test]
fn pixel_scroll_partial_row() {
    let mut v = laid_out();
    v.tick(Duration::from_millis(16));
    let pitch = v.pitch_icon_pt().0;
    v.scroll_keys(pitch * 1.5);
    assert!((v.scroll_pt - pitch * 1.5).abs() < 0.5);
    assert_eq!(v.last_scroll, 1);
    // Slot 0 is now item `cols` (row 1 is the partial top row).
    let items = v.vrows().visible_items();
    assert_eq!(items.start, v.cols);
}

/// C5: base from one pack + target from another loops across packs
/// and the caption would show pack-qualified names.
#[test]
fn cross_pack_selection_loops() {
    let mut v = laid_out();
    v.tick(Duration::from_millis(16));
    // Global id = pack * PACK_STRIDE + icon idx (see `global_id`).
    let base_gid = global_id(0, 4);
    let target_gid = global_id(1, 7);
    v.signals.base.set(i64::from(base_gid));
    v.tick(Duration::from_millis(16));
    assert_eq!(v.base_sel, Some((0, 4)));
    v.signals.pack.set(1);
    v.tick(Duration::from_millis(16));
    v.signals.target.set(i64::from(target_gid));
    v.tick(Duration::from_millis(16));
    assert_eq!(v.base_sel, Some((0, 4)));
    assert_eq!(v.target_sel, Some((1, 7)));
    // Wells show their own icons: base lucide, target tabler.
    assert_eq!(v.well_base.tooltip(), {
        let i = &icons::PACKS[0].icons[4];
        Some(format!("{} · {}", i.name, icons::PACKS[0].name))
    });
    assert_eq!(v.well_target.tooltip(), {
        let i = &icons::PACKS[1].icons[7];
        Some(format!("{} · {}", i.name, icons::PACKS[1].name))
    });
    // The pair loops — hero alternates packs. (The arena ticks
    // internal children; in tests we tick the hero ourselves so its
    // spring actually settles.)
    v.signals.paused.set(false);
    let mut packs = std::collections::BTreeSet::new();
    for _ in 0..500 {
        v.tick(Duration::from_millis(16));
        Widget::tick(&mut v.hero, Duration::from_millis(16));
        packs.insert(v.hero_pack_i);
        if packs.len() == 2 {
            break;
        }
    }
    assert_eq!(packs.len(), 2, "hero must loop across packs");
}

/// B3: a zoom write rebinds at the new pitch and keeps the top item.
#[test]
fn zoom_rebinds_and_anchors_top() {
    let mut v = laid_out();
    v.tick(Duration::from_millis(16));
    v.signals.scroll.set(10);
    v.tick(Duration::from_millis(16));
    let top_item = v.last_scroll as usize * v.cols;
    let top_sel = v.slot_icon(top_item);
    v.signals.zoom.set(0);
    v.tick(Duration::from_millis(16));
    // New pitch; the previously-top item is in the new top row.
    assert_eq!(v.pitch_icon_pt(), ZOOM_LEVELS[0]);
    let new_top = v.last_scroll as usize * v.cols;
    let top_row: Vec<Option<Sel>> = (new_top..new_top + v.cols)
        .map(|i| v.slot_icon(i))
        .collect();
    assert!(top_row.contains(&top_sel));
}

#[test]
fn decorative_cells_hidden_hero_labelled() {
    let mut v = laid_out();
    v.tick(Duration::from_millis(16));
    // Cells are decorative — absent from the a11y tree; the hero
    // carries the current icon name as its label.
    let mut hero_node = AccessKitNode::new(accesskit::Role::Image);
    v.hero.accessibility(&mut hero_node);
    assert!(hero_node.label().is_some());
    let mut cell_node = AccessKitNode::new(accesskit::Role::Image);
    v.cells[0].icon.accessibility(&mut cell_node);
    assert!(cell_node.is_hidden() || cell_node.label().is_none());
}

/// Records the viewer's own paint commands (internal children paint
/// through the arena's child protocol — not needed for the scans
/// below, which only read `MorphViewer`'s chrome).
fn paint_list(v: &MorphViewer) -> martensite::core::PaintList {
    let mut list = martensite::core::PaintList::new();
    let theme = martensite::theme::Theme::new("test");
    let mut cx = martensite::core::PaintContext {
        list: &mut list,
        bounds: v.bounds,
        theme: &theme,
        scale: 1.0,
        text_painter: None,
    };
    v.paint(&mut cx);
    list
}

/// The active-filter badge circle sits on the funnel's top-right
/// corner — detected geometrically in the paint list.
fn badge_pills(v: &MorphViewer) -> usize {
    use kurbo::Shape as _;
    use martensite::core::PaintCommand;
    let f = v.funnel_rect;
    paint_list(v)
        .commands
        .iter()
        .filter(|c| {
            let PaintCommand::FillPath(path, _) = c else {
                return false;
            };
            let bb = path.bounding_box();
            // Badge circle ~14pt, centered just past the corner.
            bb.width() > 8.0
                && bb.width() < 24.0
                && (bb.center().x - f64::from(f.max_x())).abs() < 16.0
                && (bb.center().y - f64::from(f.min_y())).abs() < 16.0
        })
        .count()
}

#[test]
fn badge_hidden_without_filters_counts_to_two() {
    use martensite::core::PaintCommand;
    let mut v = laid_out();
    v.tick(Duration::from_millis(16));
    assert_eq!(badge_pills(&v), 0, "no filters → no badge");
    v.signals.category.set("arrow".to_string());
    v.tick(Duration::from_millis(16));
    assert_eq!(badge_pills(&v), 1);
    // The badge carries the digit — a DrawText inside its bounds.
    let list = paint_list(&v);
    let f = v.funnel_rect;
    let digit = list.commands.iter().any(|c| {
        matches!(c, PaintCommand::DrawText(p, t, _, _)
            if t == "1" && (p.x as f32 - f.max_x()).abs() < 16.0
                && (p.y as f32 - f.min_y()).abs() < 16.0)
    });
    assert!(digit, "badge must paint its count");
    v.signals.sort.set("az".to_string());
    v.tick(Duration::from_millis(16));
    assert_eq!(badge_pills(&v), 1);
    let list = paint_list(&v);
    let two = list.commands.iter().any(|c| {
        matches!(c, PaintCommand::DrawText(p, t, _, _)
            if t == "2" && (p.x as f32 - f.max_x()).abs() < 16.0
                && (p.y as f32 - f.min_y()).abs() < 16.0)
    });
    assert!(two, "two active facets → badge reads 2");
}

#[test]
fn header_count_sits_in_the_header_band() {
    use martensite::core::PaintCommand;
    let v = laid_out();
    let list = paint_list(&v);
    let want = format!("({})", v.display_len());
    let top = v.grid_rect.origin.y;
    let bottom = top + GRID_HEAD_PT; // scale is 1.0 in tests
    let found = list.commands.iter().any(|c| {
        matches!(c, PaintCommand::DrawText(p, t, _, _)
            if *t == want && p.y as f32 > top && p.y as f32 <= bottom)
    });
    assert!(found, "({}) must paint inside the header band", want);
}

/// Hovering a filled cell paints a neutral wash — never an
/// accent-colored ring (accent is the base-selection color).
#[test]
fn hover_wash_is_neutral_not_accent() {
    use kurbo::Shape as _;
    use martensite::core::PaintCommand;
    let mut v = laid_out();
    v.tick(Duration::from_millis(16));
    let r = v.cell_rects[0];
    let mv = WidgetEvent::PointerMoved {
        position: Vec2::new(r.origin.x + r.size.x * 0.5, r.origin.y + r.size.y * 0.5),
    };
    v.event(&mut EventContext {
        event: &mv,
        bounds: v.bounds,
        scale: 1.0,
    });
    assert_eq!(v.hover_cell, Some(0));
    let list = paint_list(&v);
    let cell = kurbo::Rect::new(
        f64::from(r.min_x()),
        f64::from(r.min_y()),
        f64::from(r.max_x()),
        f64::from(r.max_y()),
    );
    // No stroke command may touch the hovered cell.
    let ring = list.commands.iter().any(|c| {
        let bb = match c {
            PaintCommand::StrokePath(p, _, _) => p.bounding_box(),
            PaintCommand::StrokeRect(r, _, _) => *r,
            _ => return false,
        };
        bb.intersect(cell).area() > 0.0
    });
    assert!(!ring, "hover must not paint a ring on the cell");
    // The neutral wash is present instead.
    let wash = list.commands.iter().any(|c| {
        let (bb, col) = match c {
            PaintCommand::FillPath(p, col) => (p.bounding_box(), *col),
            PaintCommand::FillRect(r, col) => (*r, *col),
            _ => return false,
        };
        col == [255, 255, 255, 20] && bb.intersect(cell).area() > 0.0
    });
    assert!(wash, "hovered cell must get the neutral wash");
}

/// A well under ghost preview hides its real thumbnail — the ghost
/// replaces it rather than layering on top.
#[test]
fn ghost_preview_hides_the_wells_real_icon() {
    let mut v = laid_out();
    v.tick(Duration::from_millis(16));
    let b = i64::from(global_id(0, 1));
    let t = i64::from(global_id(0, 9));
    v.signals.base.set(b);
    v.signals.target.set(t);
    v.tick(Duration::from_millis(16));
    assert!(v.well_base.child(0).is_some(), "filled base shows its icon");
    // Hover a filled cell → chain preview: the current target ghosts
    // into base, the hovered icon ghosts into target.
    let r = v.cell_rects[5];
    let mv = WidgetEvent::PointerMoved {
        position: Vec2::new(r.origin.x + r.size.x * 0.5, r.origin.y + r.size.y * 0.5),
    };
    v.event(&mut EventContext {
        event: &mv,
        bounds: v.bounds,
        scale: 1.0,
    });
    assert_eq!(v.hover_cell, Some(5));
    assert!(
        v.well_base.child(1).is_some(),
        "displaced target ghosts into base"
    );
    assert!(
        v.well_base.child(0).is_none(),
        "the real thumbnail hides while a ghost previews"
    );
    assert!(v.well_target.child(1).is_some());
    // Hover exit restores the real icon.
    v.event(&mut EventContext {
        event: &WidgetEvent::PointerLeave,
        bounds: v.bounds,
        scale: 1.0,
    });
    assert!(v.well_base.child(0).is_some());
    assert!(v.well_base.child(1).is_none());
}

/// The hero glyph is adaptive: a taller card gets a bigger glyph and
/// the stack stays inside the card.
#[test]
fn hero_glyph_grows_with_card_height() {
    let mut v = MorphViewer::new(signals());
    relayout(&mut v, 1320.0, 520.0);
    let small = v.hero_icon_rect.height();
    relayout(&mut v, 1320.0, 1200.0);
    let tall = v.hero_icon_rect.height();
    assert!(
        tall > small,
        "glyph must grow with card height ({small} → {tall})"
    );
    // The centered stack never overruns the card.
    assert!(v.extra_rects[3].max_y() <= v.hero_card.max_y());
}

/// The last mounted row crosses the wall's bottom edge whenever the
/// wall height isn't a pitch multiple — partial rows are mounted and
/// clipped, not dropped.
#[test]
fn bottom_partial_row_is_mounted_and_clipped() {
    let mut v = laid_out();
    v.tick(Duration::from_millis(16));
    let pitch = v.pitch_icon_pt().0;
    let rem = v.wall_h_pt() % pitch;
    assert!(rem > 1.0, "test geometry should leave a partial row");
    let items = v.vrows().visible_items();
    let last_slot = items.end - items.start - 1;
    let r = v.cell_rects[last_slot];
    assert!(r.min_y() < v.wall_rect.max_y());
    assert!(
        r.max_y() > v.wall_rect.max_y(),
        "last row must cross the wall's bottom edge"
    );
    assert!(v.cells[last_slot].filled);
    assert_eq!(v.child_clip(last_slot), Some(v.wall_rect));
    // And a mid-row scroll clips a top row too.
    v.scroll_keys(pitch * 0.5);
    assert!(v.cell_rects[0].min_y() < v.wall_rect.min_y());
}

/// A bound ghost must actually emit strokes — the well hands it
/// bounds in `layout`, so rebinding in place (not replacing the
/// widget) is what keeps a mid-session ghost visible.
#[test]
fn ghost_preview_emits_paint_commands() {
    let mut v = laid_out();
    v.tick(Duration::from_millis(16));
    v.signals.base.set(i64::from(global_id(0, 1)));
    v.tick(Duration::from_millis(16));
    let r = v.cell_rects[5];
    v.event(&mut EventContext {
        event: &WidgetEvent::PointerMoved {
            position: Vec2::new(r.origin.x + r.size.x * 0.5, r.origin.y + r.size.y * 0.5),
        },
        bounds: v.bounds,
        scale: 1.0,
    });
    assert_eq!(v.hover_cell, Some(5));
    let g = v.well_target.child(1).expect("ghost child");
    let mut list = martensite::core::PaintList::new();
    let theme = martensite::theme::Theme::new("test");
    let mut cx = martensite::core::PaintContext {
        list: &mut list,
        bounds: v.well_target.child_bounds(1).unwrap_or_default(),
        theme: &theme,
        scale: 1.0,
        text_painter: None,
    };
    g.paint(&mut cx);
    let strokes = list
        .commands
        .iter()
        .filter(|c| matches!(c, martensite::core::PaintCommand::StrokePath(..)))
        .count();
    assert!(strokes > 0, "ghost icon paints no glyph strokes");
}

/// "Glyph top, controls bottom": the display group (glyph, caption,
/// meta) sits above the bottom-anchored controls group (wells,
/// progress, transport), with the source label pinned last — and the
/// order holds even when the card is short.
#[test]
fn hero_sections_ordered_and_controls_bottom_anchored() {
    let check = |v: &MorphViewer| {
        let glyph_b = v.hero_icon_rect.max_y();
        let caption_y = v.hero_caption_y;
        let meta_b = v.hero_meta_y + 14.0; // 11pt meta line box
        let wells_t = v.extra_rects[0].min_y();
        let wells_b = v.extra_rects[0].max_y();
        let prog_t = v.progress_rect.min_y();
        let prog_b = v.progress_rect.max_y();
        let trans_t = v.extra_rects[2].min_y();
        let trans_b = v.extra_rects[2].max_y();
        let src_t = v.hero_src_y;
        assert!(
            glyph_b < caption_y
                && caption_y < wells_t
                && wells_t < prog_t
                && prog_t < trans_t
                && trans_t < src_t,
            "hero sections out of order: glyph {glyph_b} caption {caption_y} \
             wells {wells_t} progress {prog_t} transport {trans_t} source {src_t}"
        );
        // Bottom anchor: transport bottom → source label top = 20pt.
        assert!(
            (src_t - trans_b - 20.0).abs() <= 2.0,
            "controls bottom {trans_b} not 20pt above source {src_t}"
        );
        // Group gaps: wells→progress 16, progress→transport 12.
        assert!((prog_t - wells_b - 16.0).abs() <= 2.0);
        assert!((trans_t - prog_b - 12.0).abs() <= 2.0);
        // Display group clears the controls by at least 24pt.
        assert!(
            wells_t - meta_b >= 23.0,
            "meta→wells gap {}",
            wells_t - meta_b
        );
        // Everything stays inside the card.
        assert!(v.hero_icon_rect.min_y() >= v.hero_card.min_y() - 0.5);
        assert!(trans_b <= v.hero_card.max_y() + 0.5);
    };
    let mut v = MorphViewer::new(signals());
    relayout(&mut v, 1320.0, 860.0);
    check(&v);
    // Short card: the glyph shrinks but the order never inverts.
    relayout(&mut v, 1320.0, 480.0);
    check(&v);
}

/// The search pill leads the grid header band, left-aligned and
/// vertically centered on the (N) · funnel · − + line, and never
/// runs into the right cluster — even at narrow widths.
#[test]
fn search_sits_left_in_grid_header() {
    let check = |v: &MorphViewer| {
        let r = v.filter_rect;
        assert!(r.width() > 0.0, "search pill collapsed");
        // Inside the header band, flush with the grid's left edge.
        assert!(r.min_y() >= v.grid_rect.min_y() - 0.5);
        assert!(r.max_y() <= v.wall_rect.min_y() + 0.5);
        assert!((r.min_x() - v.grid_rect.min_x()).abs() < 0.5);
        // Same center line as the funnel (±1px).
        let rc = r.min_y() + r.size.y * 0.5;
        let fc = v.funnel_rect.min_y() + v.funnel_rect.size.y * 0.5;
        assert!((rc - fc).abs() <= 1.0, "search center {rc} vs funnel {fc}");
        // No overlap with the count/funnel/zoom cluster.
        assert!(r.max_x() <= v.funnel_rect.min_x() - 20.0);
        assert!(r.max_x() <= v.extra_rects[4].min_x() - 20.0);
    };
    let mut v = MorphViewer::new(signals());
    relayout(&mut v, 1320.0, 860.0);
    check(&v);
    relayout(&mut v, 900.0, 700.0);
    check(&v);
}

/// The search pill carries a real magnifier icon (0.9× field height,
/// centered, inside the field), the placeholder is exactly "Search",
/// and a query shows no in-field result count.
#[test]
fn search_field_icon_and_hint() {
    use martensite::core::PaintCommand;
    let v = laid_out();
    let r = v.filter_rect;
    let ib = v.search_icon_rect;
    assert!(r.width() > 0.0 && ib.width() > 0.0);
    assert!(
        (ib.height() - r.height() * 0.9).abs() <= 0.5,
        "icon {} vs field {}",
        ib.height(),
        r.height()
    );
    let rc = r.min_y() + r.height() * 0.5;
    let ic = ib.min_y() + ib.height() * 0.5;
    assert!((ic - rc).abs() <= 0.5, "icon center {ic} vs field {rc}");
    assert!(ib.min_x() >= r.min_x() && ib.max_x() <= r.max_x());

    let texts_in_field = |v: &MorphViewer| -> Vec<(f64, String)> {
        paint_list(v)
            .commands
            .iter()
            .filter_map(|c| match c {
                PaintCommand::DrawText(p, t, ..)
                    if p.x >= f64::from(r.min_x())
                        && p.x <= f64::from(r.max_x())
                        && p.y >= f64::from(r.min_y())
                        && p.y <= f64::from(r.max_y()) =>
                {
                    Some((p.x, t.clone()))
                }
                _ => None,
            })
            .collect()
    };
    let texts = texts_in_field(&v);
    let (px, ph) = texts
        .iter()
        .find(|(_, t)| t == "Search")
        .expect("placeholder 'Search' not painted");
    assert!(
        *px >= ib.max_x() as f64,
        "placeholder overlaps the icon at {px}"
    );
    let _ = ph;

    // With a query set the field shows only the query + caret — the
    // result count lives in the header's (N), not inside the pill.
    v.signals.filter.set("arrow".to_string());
    for (x, t) in texts_in_field(&v) {
        assert!(
            !t.chars().all(|c| c.is_ascii_digit()) && !t.is_empty(),
            "count text '{t}' inside the field at x={x}"
        );
    }
}
