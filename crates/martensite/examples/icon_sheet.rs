//! Rasterizes the builtin icon pack (or a namespace slice of it) into
//! a labelled PNG contact sheet for visual review.
//!
//! Usage:
//!     cargo run -p martensite --example icon_sheet -- [-o out.png]
//!         [--ns PREFIX] [--cols N] [--cell PX] [--labels/--no-labels]
//!
//! Each cell renders the icon's `d` stroked at 2/24 of the cell size
//! (the DESIGN.md stroke convention) with round caps/joins, plus the
//! icon name under it. Run at `--cell 24` to check pixel-grid
//! sharpness and `--cell 64`+ to inspect curve quality.

use std::collections::BTreeMap;
use std::path::Path;

use kurbo::{BezPath, PathEl};
use martensite::icons;
use tiny_skia::{LineCap, LineJoin, Paint, PathBuilder, Pixmap, Stroke, Transform};

// ---- 4x6 bitmap font for labels (A-Z 0-9 . - _ /) ---------------

const GLYPHS: &[(&str, [&str; 6])] = &[
    ("a", ["0110", "1001", "1001", "1111", "1001", "1001"]),
    ("b", ["1110", "1001", "1110", "1001", "1001", "1110"]),
    ("c", ["0110", "1001", "1000", "1000", "1001", "0110"]),
    ("d", ["1110", "1001", "1001", "1001", "1001", "1110"]),
    ("e", ["1111", "1000", "1110", "1000", "1000", "1111"]),
    ("f", ["1111", "1000", "1110", "1000", "1000", "1000"]),
    ("g", ["0110", "1001", "1000", "1011", "1001", "0110"]),
    ("h", ["1001", "1001", "1111", "1001", "1001", "1001"]),
    ("i", ["0111", "0010", "0010", "0010", "0010", "0111"]),
    ("j", ["0011", "0001", "0001", "0001", "1001", "0110"]),
    ("k", ["1001", "1001", "1010", "1100", "1010", "1001"]),
    ("l", ["1000", "1000", "1000", "1000", "1000", "1111"]),
    ("m", ["1001", "1111", "1111", "1001", "1001", "1001"]),
    ("n", ["1001", "1101", "1011", "1011", "1001", "1001"]),
    ("o", ["0110", "1001", "1001", "1001", "1001", "0110"]),
    ("p", ["1110", "1001", "1001", "1110", "1000", "1000"]),
    ("q", ["0110", "1001", "1001", "1010", "1001", "0111"]),
    ("r", ["1110", "1001", "1001", "1110", "1010", "1001"]),
    ("s", ["0111", "1000", "0110", "0001", "0001", "1110"]),
    ("t", ["1111", "0010", "0010", "0010", "0010", "0010"]),
    ("u", ["1001", "1001", "1001", "1001", "1001", "0110"]),
    ("v", ["1001", "1001", "1001", "1001", "0110", "0110"]),
    ("w", ["1001", "1001", "1001", "1111", "1111", "1001"]),
    ("x", ["1001", "1001", "0110", "0110", "1001", "1001"]),
    ("y", ["1001", "1001", "0110", "0010", "0010", "0010"]),
    ("z", ["1111", "0001", "0010", "0100", "1000", "1111"]),
    ("0", ["0110", "1001", "1011", "1101", "1001", "0110"]),
    ("1", ["0010", "0110", "0010", "0010", "0010", "0111"]),
    ("2", ["0110", "1001", "0010", "0100", "1000", "1111"]),
    ("3", ["1110", "0001", "0110", "0001", "0001", "1110"]),
    ("4", ["0010", "0110", "1010", "1111", "0010", "0010"]),
    ("5", ["1111", "1000", "1110", "0001", "1001", "0110"]),
    ("6", ["0110", "1000", "1110", "1001", "1001", "0110"]),
    ("7", ["1111", "0001", "0010", "0100", "0100", "0100"]),
    ("8", ["0110", "1001", "0110", "1001", "1001", "0110"]),
    ("9", ["0110", "1001", "1001", "0111", "0001", "0110"]),
    (".", ["0000", "0000", "0000", "0000", "0110", "0110"]),
    ("-", ["0000", "0000", "0110", "0000", "0000", "0000"]),
    ("_", ["0000", "0000", "0000", "0000", "0000", "1111"]),
    ("/", ["0001", "0001", "0010", "0100", "1000", "1000"]),
    (" ", ["0000", "0000", "0000", "0000", "0000", "0000"]),
];

fn glyph(c: char) -> [&'static str; 6] {
    GLYPHS
        .iter()
        .find(|(g, _)| g.starts_with(c.to_ascii_lowercase()))
        .map(|(_, rows)| *rows)
        .unwrap_or(["1111", "1111", "1111", "1111", "1111", "1111"])
}

fn blit_text(pix: &mut Pixmap, text: &str, x: i32, y: i32, rgba: [u8; 4]) {
    let mut cx = x;
    for c in text.chars() {
        for (ry, row) in glyph(c).iter().enumerate() {
            for (rx, bit) in row.chars().enumerate() {
                if bit == '1' {
                    let px = cx + rx as i32;
                    let py = y + ry as i32;
                    if px >= 0 && py >= 0 && px < pix.width() as i32 && py < pix.height() as i32 {
                        let i = py as usize * pix.width() as usize + px as usize;
                        pix.pixels_mut()[i] =
                            tiny_skia::Color::from_rgba8(rgba[0], rgba[1], rgba[2], rgba[3])
                                .premultiply()
                                .to_color_u8();
                    }
                }
            }
        }
        cx += 5;
    }
}

fn bez_to_skia(path: &BezPath) -> Option<tiny_skia::Path> {
    let mut b = PathBuilder::new();
    for el in path.iter() {
        match el {
            PathEl::MoveTo(p) => b.move_to(p.x as f32, p.y as f32),
            PathEl::LineTo(p) => b.line_to(p.x as f32, p.y as f32),
            PathEl::QuadTo(p1, p2) => b.quad_to(p1.x as f32, p1.y as f32, p2.x as f32, p2.y as f32),
            PathEl::CurveTo(p1, p2, p3) => b.cubic_to(
                p1.x as f32,
                p1.y as f32,
                p2.x as f32,
                p2.y as f32,
                p3.x as f32,
                p3.y as f32,
            ),
            PathEl::ClosePath => b.close(),
        }
    }
    b.finish()
}

/// DESIGN.md conformance audit: parse, coordinate bounds, subpath
/// budget, and pixel-grid alignment. Exits non-zero on any FAIL.
fn check_entries(entries: &[(&str, &str)]) {
    const MAX_SUBPATHS: usize = 24; // morph engine cap (engine.rs)
    let mut fails = 0usize;
    let mut warns = 0usize;
    for (name, d) in entries {
        let path = match BezPath::from_svg(d) {
            Ok(p) => p,
            Err(e) => {
                println!("FAIL {name}: parse: {e}");
                fails += 1;
                continue;
            }
        };
        let mut subs = 0usize;
        let (mut xmin, mut ymin, mut xmax, mut ymax) = (
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        );
        let mut offgrid = 0usize;
        for el in path.iter() {
            if let PathEl::MoveTo(_) = el {
                subs += 1;
            }
            for p in match el {
                PathEl::MoveTo(p) | PathEl::LineTo(p) => vec![p],
                PathEl::QuadTo(a, b) => vec![a, b],
                PathEl::CurveTo(a, b, c) => vec![a, b, c],
                PathEl::ClosePath => vec![],
            } {
                xmin = xmin.min(p.x);
                ymin = ymin.min(p.y);
                xmax = xmax.max(p.x);
                ymax = ymax.max(p.y);
                if (p.x * 4.0).fract() != 0.0 || (p.y * 4.0).fract() != 0.0 {
                    offgrid += 1;
                }
            }
        }
        if subs > MAX_SUBPATHS {
            println!("FAIL {name}: {subs} subpaths > {MAX_SUBPATHS}");
            fails += 1;
        }
        if xmin < 0.5 || ymin < 0.5 || xmax > 23.5 || ymax > 23.5 {
            println!("FAIL {name}: bounds [{xmin},{ymin}]-[{xmax},{ymax}] outside [0.5,23.5]");
            fails += 1;
        } else if xmin < 1.8 || ymin < 1.8 || xmax > 22.2 || ymax > 22.2 {
            println!(
                "WARN {name}: bounds [{xmin},{ymin}]-[{xmax},{ymax}] outside preferred [2,22]"
            );
            warns += 1;
        }
        if xmin < 3.0 && ymin < 3.0 && xmax > 21.0 && ymax > 21.0 {
            // likely oversized glyph — icon touches most of the canvas
            println!("WARN {name}: spans [{xmin},{ymin}]-[{xmax},{ymax}] — check visual weight");
            warns += 1;
        }
        if offgrid > 0 {
            println!("INFO {name}: {offgrid} coords not on the quarter grid");
        }
    }
    println!("check: {} icons, {fails} FAIL, {warns} WARN", entries.len());
    if fails > 0 {
        std::process::exit(1);
    }
}

fn main() {
    let mut out = "icon_sheet.png".to_string();
    let mut ns = String::new();
    let mut cols = 10u32;
    let mut cell = 48f32;
    let mut labels = true;
    let mut check = false;
    let args: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-o" | "--out" => {
                i += 1;
                out = args[i].clone();
            }
            "--ns" => {
                i += 1;
                ns = args[i].clone();
            }
            "--cols" => {
                i += 1;
                cols = args[i].parse().unwrap();
            }
            "--cell" => {
                i += 1;
                cell = args[i].parse().unwrap();
            }
            "--labels" => labels = true,
            "--no-labels" => labels = false,
            "--check" => check = true,
            other => {
                eprintln!("unknown arg {other}; usage: icon_sheet [-o out] [--ns p] [--cols n] [--cell px] [--no-labels] [--check]");
                std::process::exit(2);
            }
        }
        i += 1;
    }

    let mut entries: Vec<(&str, &str)> = icons::builtin()
        .entries()
        .iter()
        .map(|e| (e.name.as_ref(), e.d.as_ref()))
        .filter(|(name, _)| ns.is_empty() || name.starts_with(ns.as_str()))
        .collect();
    entries.sort();

    if check {
        check_entries(&entries);
        return;
    }

    let rows = entries.len().div_ceil(cols as usize) as u32;
    let label_h = if labels { 14.0f32 } else { 0.0 };
    let cell_h = cell + label_h + 8.0;
    let pad = 8.0f32;
    let w = (pad * 2.0 + cols as f32 * cell) as u32;
    let h = (pad * 2.0 + rows as f32 * cell_h) as u32;
    let mut pix = Pixmap::new(w, h).expect("pixmap");
    // dark canvas to match app surfaces
    pix.fill(tiny_skia::Color::from_rgba8(18, 18, 20, 255));

    let stroke = Stroke {
        width: cell / 12.0, // 2px at 24 grid
        line_cap: LineCap::Round,
        line_join: LineJoin::Round,
        ..Stroke::default()
    };
    let ink = {
        let mut p = Paint::default();
        p.set_color_rgba8(230, 230, 235, 255);
        p
    };

    let mut manifest = BTreeMap::new();
    for (idx, (name, d)) in entries.iter().enumerate() {
        let col = (idx as u32) % cols;
        let row = (idx as u32) / cols;
        let ox = pad + col as f32 * cell;
        let oy = pad + row as f32 * cell_h;
        let path = BezPath::from_svg(d).unwrap_or_else(|e| panic!("{name}: parse failed: {e}"));
        let sk = bez_to_skia(&path).expect("path build");
        let ts = Transform::from_scale(cell / 24.0, cell / 24.0).post_translate(ox, oy);
        pix.stroke_path(&sk, &ink, &stroke, ts, None);
        if labels {
            let label = match name.split_once('.') {
                Some((ns_part, leaf)) if !ns.is_empty() && ns_part == ns => leaf,
                Some((_, leaf)) => leaf,
                None => name,
            };
            blit_text(
                &mut pix,
                label,
                ox as i32 + 1,
                (oy + cell + 4.0) as i32,
                [150, 150, 158, 255],
            );
        }
        manifest.insert(format!("{row},{col}"), name.to_string());
    }

    pix.save_png(Path::new(&out)).expect("save png");
    println!("{out}: {} icons, {w}x{h}", entries.len());
    for (cell_pos, name) in &manifest {
        println!("  {cell_pos}  {name}");
    }
}
