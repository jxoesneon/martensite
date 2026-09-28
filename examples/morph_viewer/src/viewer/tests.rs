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
    assert!(v.shown > 0 && v.shown <= MAX_CELLS);
    assert_eq!(v.cell_rects.len(), v.shown);
    assert_eq!(v.icon_rects.len(), v.shown + 1); // hero + cells
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
    let last_row = (v.max_scroll_pt() / CELL).round() as usize;
    assert_eq!(v.last_scroll as usize, last_row);
    assert!((v.last_scroll as usize) < v.display_len().div_ceil(v.cols));
}

#[test]
fn scroll_event_and_keys_advance_rows() {
    let mut v = laid_out();
    v.tick(Duration::from_millis(16));
    let center = Vec2::new(
        v.grid_rect.origin.x + v.grid_rect.size.x * 0.5,
        v.grid_rect.origin.y + v.grid_rect.size.y * 0.5,
    );
    // Deltas arrive in device px (app layer converts lines);
    // ~174px = 3 rows.
    let scroll = WidgetEvent::Scroll {
        position: center,
        delta: Vec2::new(0.0, -174.0),
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
    let last_row = (v.max_scroll_pt() / CELL).round() as usize;
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
