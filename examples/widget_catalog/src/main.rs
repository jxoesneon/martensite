//! Martensite Widget Catalog executable.

use widget_catalog::{all_pages, CatalogView};

/// Rasterizes the catalog view to a PNG — the same
/// `WidgetArena` → `PaintList` → `TinySkiaBackend` path the dev
/// channel's `capture_node` uses, without needing a window or GPU.
fn render_png(path: &str, w: f32, h: f32, scale: f32, page: Option<usize>) {
    use glam::Vec2;
    use martensite::core::{LayoutConstraints, LayoutContext, PaintList};
    use martensite::prelude::*;
    use martensite_render::RenderBackend;

    let mut arena = WidgetArena::new();
    arena.set_theme(martensite::theme::tokens::default_dark());
    arena.set_scale_factor(scale);
    arena.set_text_painter(martensite::text_paint::shared_painter());
    let _measurer = arena
        .text_painter_shared()
        .map(martensite::core::paint::install_ambient_measurer);
    let mut hot = HotNode::default();
    hot.flags |= NodeFlags::VISIBLE | NodeFlags::HIT_TEST_ENABLED;
    let mut view = CatalogView::new(all_pages());
    if let Some(page) = page {
        view.select_page(page);
    }
    let root = arena.insert_with_widget(hot, Box::new(view));

    // Same contract as the live path: layout works in surface
    // (device) pixels; widgets convert pt→px via `cx.pt(scale)`.
    let bounds = Rect::new(0.0, 0.0, w, h);
    if let Some((hot, cold)) = arena.get_both_mut(root) {
        cold.widget.measure(
            &mut LayoutContext { hot, scale },
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(w, h),
            },
        );
        hot.bounds = bounds;
        cold.widget
            .layout(&mut LayoutContext { hot, scale }, bounds);
    }
    let mut list = PaintList::new();
    arena.build_paint_list(root, &mut list);
    let mut backend =
        martensite_render::TinySkiaBackend::new(w as u32, h as u32).expect("pixmap alloc");
    RenderBackend::render(&mut backend, &list);
    let png = backend.pixmap().encode_png().expect("png encode");
    std::fs::write(path, png).expect("png write");
    eprintln!("catalog → {path} ({w}x{h} @ {scale}x)");
}

/// `WxH` like the live-headless env knob, in device pixels.
fn headless_size() -> (f32, f32) {
    std::env::var("MARTENSITE_HEADLESS_SIZE")
        .ok()
        .and_then(|s| {
            let (w, h) = s.split_once('x')?;
            Some((w.parse().ok()?, h.parse().ok()?))
        })
        .unwrap_or((1600.0, 1000.0))
}

fn headless_scale() -> f32 {
    std::env::var("MARTENSITE_HEADLESS_SCALE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1.0)
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();

    // Windowed by default — the catalog is a GUI showcase.
    if args.is_empty() {
        widget_catalog::run().expect("windowed catalog");
        return;
    }
    if args.iter().any(|a| a == "--live-headless") {
        widget_catalog::run_live_headless().expect("live-headless catalog");
        return;
    }

    let mut page_sel: Option<usize> = None;
    let mut args = args.drain(..);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--filter" => {
                if let Some(query) = args.next() {
                    let q = query.to_lowercase();
                    let pages = all_pages();
                    println!("Widget Catalog: {} pages registered.", pages.len());
                    println!("Filter \"{query}\":");
                    for p in &pages {
                        let m = p.meta();
                        if m.name.to_lowercase().contains(&q)
                            || m.family.to_lowercase().contains(&q)
                        {
                            println!("  - {} [{}] (Role: {})", m.name, m.family, m.role);
                        }
                    }
                }
            }
            "--png" => {
                let path = args
                    .next()
                    .unwrap_or_else(|| "/tmp/widget_catalog.png".into());
                let (w, h) = headless_size();
                render_png(&path, w, h, headless_scale(), page_sel);
            }
            "--page" => {
                if let Some(n) = args.next() {
                    page_sel = n.parse().ok();
                }
            }
            _ => {}
        }
    }
}
