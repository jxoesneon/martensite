//! Downloads the free stroke-icon packs morphicons targets — lucide
//! (ISC), tabler (MIT), feather (MIT), heroicons (MIT), iconoir (MIT)
//! — straight from their upstream repositories, extracts every glyph
//! as a single multi-subpath SVG `d` string, and writes
//! `src/icons_gen.rs` for the viewer.
//!
//! ```sh
//! cargo run -p morph_viewer --bin fetch_icons          # all packs
//! cargo run -p morph_viewer --bin fetch_icons -- lucide feather
//! ```
//!
//! Requires `curl` and `tar` on PATH. Extraction caches under
//! `target/morph_viewer_packs/` so re-runs skip unchanged downloads
//! (delete that dir to force a refresh).

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Upstream pack specs: `(name, tarball, svg dir below the archive
/// root, license)`. Refs track each project's default branch — pin
/// per-pack by exporting `MORPH_REF_<NAME>` (e.g.
/// `MORPH_REF_LUCIDE=0.475.0`).
const PACKS: &[PackSpec] = &[
    PackSpec {
        name: "lucide",
        repo: "lucide-icons/lucide",
        branch: "main",
        svg_dir: "icons",
        license: "ISC",
    },
    PackSpec {
        name: "tabler",
        repo: "tabler/tabler-icons",
        branch: "main",
        svg_dir: "icons/outline",
        license: "MIT",
    },
    PackSpec {
        name: "feather",
        repo: "feathericons/feather",
        branch: "main",
        svg_dir: "icons",
        license: "MIT",
    },
    PackSpec {
        name: "heroicons",
        repo: "tailwindlabs/heroicons",
        branch: "master",
        svg_dir: "optimized/24/outline",
        license: "MIT",
    },
    PackSpec {
        name: "iconoir",
        repo: "iconoir-icons/iconoir",
        branch: "main",
        svg_dir: "icons/regular",
        license: "MIT",
    },
];

struct PackSpec {
    name: &'static str,
    repo: &'static str,
    branch: &'static str,
    svg_dir: &'static str,
    license: &'static str,
}

fn main() {
    let only: Vec<String> = env::args()
        .skip(1)
        .filter(|a| !a.starts_with('-'))
        .collect();
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let out_dir = manifest.join("../../target/morph_viewer_packs");
    fs::create_dir_all(&out_dir).expect("create pack cache");

    let mut generated = String::from(
        "// Icon data fetched by `cargo run -p morph_viewer --bin fetch_icons`.\n\
         //\n\
         // Each `d` is the pack's own stroke geometry (paths and shape\n\
         // elements merged into one multi-subpath path string) on the\n\
         // sets' conventional 24×24 grid. The icon data remains under\n\
         // each upstream license named per pack below.\n\n",
    );
    generated.push_str("pub static PACKS: &[PackDef] = &[\n");

    for spec in PACKS {
        if !only.is_empty() && !only.iter().any(|o| o == spec.name) {
            continue;
        }
        match fetch_pack(spec, &out_dir) {
            Ok(icons) => {
                println!("{:>9}: {} icons", spec.name, icons.len());
                generated.push_str(&format!(
                    "    PackDef {{\n        name: \"{}\",\n        source: \"https://github.com/{}\",\n        license: \"{}\",\n        icons: &[\n",
                    spec.name, spec.repo, spec.license
                ));
                for (name, d) in &icons {
                    generated.push_str(&format!(
                        "            IconDef {{ name: \"{}\", d: \"{}\" }},\n",
                        escape(name),
                        escape(d)
                    ));
                }
                generated.push_str("        ],\n    },\n");
            }
            Err(e) => eprintln!("{:>9}: FAILED — {e}", spec.name),
        }
    }
    generated.push_str("];\n");

    let dest = manifest.join("src/icons_gen.rs");
    fs::write(&dest, generated).expect("write icons_gen.rs");
    println!("wrote {}", dest.display());
}

/// Downloads (or reuses) a pack tarball, extracts it, and converts
/// every `.svg` under the pack's icon directory into `(name, d)`.
fn fetch_pack(spec: &PackSpec, cache: &Path) -> Result<Vec<(String, String)>, String> {
    let branch_ref = env::var(format!("MORPH_REF_{}", spec.name.to_uppercase()))
        .unwrap_or_else(|_| spec.branch.to_string());
    let url = format!(
        "https://codeload.github.com/{}/tar.gz/refs/heads/{}",
        spec.repo, branch_ref
    );
    let tarball = cache.join(format!("{}.tar.gz", spec.name));
    let extract_root = cache.join(spec.name);
    if !tarball.exists() {
        let status = Command::new("curl")
            .args(["-fsSL", "--retry", "3", &url, "-o"])
            .arg(&tarball)
            .status()
            .map_err(|e| format!("curl spawn: {e}"))?;
        if !status.success() {
            return Err(format!("curl {url} → {status}"));
        }
    }
    if extract_root.exists() {
        fs::remove_dir_all(&extract_root).map_err(|e| format!("clean extract: {e}"))?;
    }
    fs::create_dir_all(&extract_root).map_err(|e| format!("mkdir: {e}"))?;
    let status = Command::new("tar")
        .arg("-xzf")
        .arg(&tarball)
        .arg("-C")
        .arg(&extract_root)
        .status()
        .map_err(|e| format!("tar spawn: {e}"))?;
    if !status.success() {
        return Err(format!("tar extract → {status}"));
    }

    // The archive nests everything under `<repo>-<branch>/`; locate the
    // declared icon dir, with a recursive fallback if upstream moved it.
    let root = fs::read_dir(&extract_root)
        .map_err(|e| format!("list extract: {e}"))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .find(|p| p.is_dir())
        .ok_or("empty archive")?;
    let mut svg_dir = root.join(spec.svg_dir);
    if !svg_dir.is_dir() {
        svg_dir = find_svg_dir(&root).ok_or("no svg dir found in archive")?;
    }

    let mut svgs: Vec<PathBuf> = Vec::new();
    collect_svgs(&svg_dir, &mut svgs);
    svgs.sort();
    let mut icons = Vec::with_capacity(svgs.len());
    let mut skipped = 0usize;
    for path in svgs {
        let text = fs::read_to_string(&path).map_err(|e| format!("read {path:?}: {e}"))?;
        match svg_to_d(&text) {
            // Gate on the same engine load `MorphIcon` performs — anything
            // that fails `set_icon` (unparsable or over the subpath limit)
            // would silently keep the previous glyph at runtime.
            Some(d)
                if !d.is_empty()
                    && kurbo::BezPath::from_svg(&d).is_ok()
                    && martensite::widgets::MorphIcon::new().set_icon(&d).is_ok() =>
            {
                let name = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("icon")
                    .to_string();
                icons.push((name, d));
            }
            _ => skipped += 1,
        }
    }
    if skipped > 0 {
        eprintln!(
            "{:>9}:   ({} files had no convertible stroke geometry)",
            spec.name, skipped
        );
    }
    if icons.is_empty() {
        return Err("no icons converted".into());
    }
    Ok(icons)
}

/// Finds the directory with the most `.svg` files under `root` —
/// the fallback when a pack's documented layout moved.
fn find_svg_dir(root: &Path) -> Option<PathBuf> {
    fn walk(dir: &Path, best: &mut (usize, Option<PathBuf>)) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        let mut svgs_here = 0;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, best);
            } else if path.extension().is_some_and(|e| e == "svg") {
                svgs_here += 1;
            }
        }
        if svgs_here > best.0 {
            *best = (svgs_here, Some(dir.to_path_buf()));
        }
    }
    let mut best = (0, None);
    walk(root, &mut best);
    best.1
}

fn collect_svgs(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_svgs(&path, out);
        } else if path.extension().is_some_and(|e| e == "svg") {
            out.push(path);
        }
    }
}

/// Converts one icon's SVG into a single multi-subpath `d` string.
///
/// Handles every shape element the stroke sets use — `path`, `line`,
/// `polyline`, `polygon`, `rect` (incl. `rx`), `circle`, `ellipse` —
/// and skips anything under `defs`/`mask`/`clipPath` or carrying a
/// `transform` (rare in these sets; better skipped than wrong).
fn svg_to_d(svg: &str) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    let mut rest = svg;
    let mut skip_depth = 0u32;
    while let Some(lt) = rest.find('<') {
        rest = &rest[lt + 1..];
        if rest.starts_with("!--") {
            let Some(end) = rest.find("-->") else { break };
            rest = &rest[end + 3..];
            continue;
        }
        if rest.starts_with('!') || rest.starts_with('?') {
            let Some(end) = rest.find('>') else { break };
            rest = &rest[end + 1..];
            continue;
        }
        let closing = rest.starts_with('/');
        if closing {
            rest = &rest[1..];
        }
        let end = rest.find('>')?;
        let tag = &rest[..end];
        let self_close = tag.ends_with('/');
        let name_end = tag
            .find(|c: char| c.is_whitespace() || c == '/')
            .unwrap_or(tag.len());
        let name = tag[..name_end].to_ascii_lowercase();
        rest = &rest[end + 1..];

        const SKIP: &[&str] = &[
            "defs", "mask", "clippath", "pattern", "title", "desc", "metadata", "script", "style",
        ];
        if closing {
            // Close tags unwind skip depth for anything that opened
            // it (defs/mask/… plus transformed `<g>` below).
            if SKIP.contains(&name.as_str()) || name == "g" {
                skip_depth = skip_depth.saturating_sub(1);
            }
            continue;
        }
        if SKIP.contains(&name.as_str()) && !self_close {
            skip_depth += 1;
            continue;
        }
        if SKIP.contains(&name.as_str()) || skip_depth > 0 {
            continue;
        }
        // A `transform`ed element's children would render in the wrong
        // space — the transformed subtree is skipped wholesale. These
        // packs are essentially transform-free; icons that lose a
        // transformed part are still valid morph targets.
        if tag.contains("transform") {
            if name == "g" && !self_close {
                skip_depth += 1;
            }
            continue;
        }
        let d = match name.as_str() {
            "path" => attr(tag, "d").map(|raw| {
                // A leading `m` resolves against (0,0) in the source
                // path element — but once elements are joined into one
                // `d`, a relative `m` would reinterpret against the
                // *previous subpath's endpoint* and drift off-grid.
                // Anchoring `M0 0` keeps the relative moveto resolving
                // against the origin, exactly as in the standalone path
                // — and leaves its implicit relative linetos untouched.
                let d = raw.trim_start();
                if d.starts_with('m') {
                    format!("M0 0 {d}")
                } else {
                    d.to_string()
                }
            }),
            "line" => {
                let a = |k| {
                    attr(tag, k)
                        .and_then(|v| v.parse::<f64>().ok())
                        .unwrap_or(0.0)
                };
                Some(format!("M{} {} L{} {}", a("x1"), a("y1"), a("x2"), a("y2")))
            }
            "polyline" | "polygon" => attr(tag, "points").map(|p| {
                let nums = numbers(p);
                let mut d = String::new();
                for pair in nums.as_chunks::<2>().0 {
                    d.push_str(if d.is_empty() { "M" } else { " L" });
                    d.push_str(&format!("{} {}", pair[0], pair[1]));
                }
                if name == "polygon" {
                    d.push('Z');
                }
                d
            }),
            "rect" => {
                let a = |k| {
                    attr(tag, k)
                        .and_then(|v| v.parse::<f64>().ok())
                        .unwrap_or(0.0)
                };
                let (x, y, w, h) = (a("x"), a("y"), a("width"), a("height"));
                let rx = a("rx").min(w / 2.0);
                if rx > 0.0 {
                    // Rounded corners via arcs (kurbo lowers them).
                    Some(format!(
                        "M{} {} H{} A{} {} 0 0 1 {} {} V{} A{} {} 0 0 1 {} {} H{} A{} {} 0 0 1 {} {} V{} A{} {} 0 0 1 {} {}Z",
                        x + rx, y, x + w - rx, rx, rx, x + w, y + rx,
                        y + h - rx, rx, rx, x + w - rx, y + h,
                        x + rx, rx, rx, x, y + h - rx,
                        y + rx, rx, rx, x + rx, y,
                    ))
                } else {
                    Some(format!("M{} {} H{} V{} H{}Z", x, y, x + w, y + h, x))
                }
            }
            "circle" => {
                let a = |k| {
                    attr(tag, k)
                        .and_then(|v| v.parse::<f64>().ok())
                        .unwrap_or(0.0)
                };
                let (cx, cy, r) = (a("cx"), a("cy"), a("r"));
                Some(format!(
                    "M{} {} A{} {} 0 1 0 {} {} A{} {} 0 1 0 {} {}Z",
                    cx - r,
                    cy,
                    r,
                    r,
                    cx + r,
                    cy,
                    r,
                    r,
                    cx - r,
                    cy
                ))
            }
            "ellipse" => {
                let a = |k| {
                    attr(tag, k)
                        .and_then(|v| v.parse::<f64>().ok())
                        .unwrap_or(0.0)
                };
                let (cx, cy, rx, ry) = (a("cx"), a("cy"), a("rx"), a("ry"));
                Some(format!(
                    "M{} {} A{} {} 0 1 0 {} {} A{} {} 0 1 0 {} {}Z",
                    cx - rx,
                    cy,
                    rx,
                    ry,
                    cx + rx,
                    cy,
                    rx,
                    ry,
                    cx - rx,
                    cy
                ))
            }
            _ => None,
        };
        if let Some(d) = d {
            if !d.trim().is_empty() {
                parts.push(d);
            }
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" "))
    }
}

/// Reads `attr="…"`/`attr='…'` out of a tag body. The leading space
/// anchors the key at a word boundary so `width` can't match inside
/// `stroke-width`.
fn attr<'a>(tag: &'a str, key: &str) -> Option<&'a str> {
    for q in ['"', '\''] {
        let needle = format!(" {key}={q}");
        if let Some(at) = tag.find(&needle) {
            let start = at + needle.len();
            let end = tag[start..].find(q)? + start;
            return Some(&tag[start..end]);
        }
    }
    None
}

fn numbers(s: &str) -> Vec<f64> {
    s.split([',', ' ', '\t'])
        .filter_map(|t| t.parse::<f64>().ok())
        .collect()
}

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}
