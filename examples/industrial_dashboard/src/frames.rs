//! Frame rasterization + golden-diff machinery behind
//! [`SweepOptions::dump_frames`](crate::lint_sweep::SweepOptions::dump_frames)
//! (dashboard-beauty spec Phase A).
//!
//! Each swept [`PaintList`] is rendered by the CPU
//! [`TinySkiaBackend`] and written to `<dir>/<zone>/<page>@<width>.png`
//! (scrolled frames get a `+y<offset>` suffix so the top-of-scroll
//! frame keeps the canonical name; the full-dock pass lands as
//! `app@1600x1000.png`). Alongside every frame, a CVD post-pass emits
//! `<name>.deutan.png` and `<name>.protan.png` (spec A3 — sighted
//! review cannot verify color-vision-deficiency safety, so the
//! simulator makes the V4 gate real).
//!
//! With [`FrameDump::baseline`] set (`--check`), each fresh frame is
//! perceptually diffed against `<baseline>/<name>.png` via
//! `martensite_render::diff::perceptual_diff` — the render crate's
//! DSSIM composes cleanly here, so the golden-diff hook (deliverable
//! 6) is wired rather than deferred.
//!
//! ## Deterministic font fixture (spec A2)
//!
//! [`FixtureTextShaper`] shapes through a `FontSystem` whose `fontdb`
//! contains ONLY the bundled Fira Mono — no system scan — so every
//! label emitted through the ambient painter (`paint_label` /
//! `TextShaper::paint_shaped_text`) is bit-identical across machines.
//! Precedent: `martensite-render-test/tests/parity.rs` bundles the
//! same face for its glyph-run tests.
//!
//! `Text` widgets lazily construct `FontManager::new()` per widget —
//! the sweep installs the same bundled face through
//! [`install_test_fonts`] (the thread-local fixture override in
//! `martensite-text`) so widget-shaped text resolves identically on
//! every host as well. Both text paths are now host-independent.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use kurbo::{Point, Rect};
use martensite::core::paint::{
    FontResource, GlyphInstance, GlyphRun, PaintList, TextShaper, TextStyle,
};
use martensite::text::{shape_text_with_attrs, Attrs, FontManager, FontSource, Style, Weight};
use martensite_render::diff::perceptual_diff;
use martensite_render::{RenderBackend, TinySkiaBackend};
use parking_lot::Mutex;

/// The widths `dump_frames` renders per spec A1/V2 — logical sweep
/// widths, distinct from the lint sweep's wider default set.
pub const FRAME_WIDTHS: [f32; 3] = [700.0, 1200.0, 1600.0];

/// Bundled Fira Mono — the same face `martensite-render-test`'s
/// `parity.rs` embeds for deterministic glyph-run tests.
const FIRA_MONO: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../crates/martensite-render-test/tests/assets/FiraMono-Medium.ttf"
));

/// Where `dump_frames` output lands and what it is compared against.
pub struct FrameDump {
    /// Directory PNG frames are written to (created recursively,
    /// zone subdirectories included).
    pub dir: PathBuf,
    /// Optional baseline directory. When `Some`, each fresh frame is
    /// perceptually diffed against `<baseline>/<name>.png` and drift
    /// is reported through the sweep log + `SweepReport` counters.
    pub baseline: Option<PathBuf>,
}

/// Default dump root: `target/dashboard-frames/` in the workspace.
///
/// Honors `CARGO_TARGET_DIR` so redirected builds keep artifacts next
/// to the rest of the target output.
pub fn default_dir() -> PathBuf {
    if let Ok(target) = std::env::var("CARGO_TARGET_DIR") {
        return PathBuf::from(target).join("dashboard-frames");
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/dashboard-frames")
}

/// Per-frame accumulation returned by [`dump_frame`].
#[derive(Default)]
pub struct DumpOutcome {
    /// PNGs written (each frame plus its two CVD simulations).
    pub written: usize,
    /// Frames compared against a baseline image (`--check` only).
    pub checked: usize,
    /// Baseline comparisons that failed, plus frames with a missing
    /// or undecodable baseline — drift the reviewer must look at.
    pub drifted: usize,
}

/// `tag` (`zone/PAGE@width`) → file stem under the dump dir. The
/// zone label stays a path segment so frames group per zone; scrolled
/// frames carry `+y<offset>` so `y == 0` keeps the canonical name.
fn frame_name(tag: &str, scroll_y: f32) -> String {
    if scroll_y > 0.0 {
        format!("{tag}+y{scroll_y:.0}")
    } else {
        tag.to_string()
    }
}

/// Rasterizes `list` and writes `<dir>/<name>.png` plus `.deutan.png`
/// / `.protan.png` CVD simulations; with a baseline configured, diffs
/// the fresh pixels against `<baseline>/<name>.png`. Errors are
/// logged, never fatal — a failed dump must not lose the lint sweep.
pub fn dump_frame(
    opts: &FrameDump,
    tag: &str,
    scroll_y: f32,
    list: &PaintList,
    width: u32,
    height: u32,
    log: &mut String,
) -> DumpOutcome {
    let mut out = DumpOutcome::default();
    let Some(mut backend) = TinySkiaBackend::new(width, height) else {
        let _ = writeln!(
            log,
            "{tag}: frame dump skipped — {width}x{height} pixmap rejected"
        );
        return out;
    };
    backend.render(list);
    let rgba = unpremultiply(backend.pixels());
    let name = frame_name(tag, scroll_y);
    let path = opts.dir.join(format!("{name}.png"));
    match write_png(&path, width, height, &rgba) {
        Ok(()) => out.written += 1,
        Err(e) => {
            let _ = writeln!(log, "{name}: PNG write failed: {e}");
        }
    }
    // CVD post-processor (spec A3): deuteranopia + protanopia sims
    // beside every frame.
    for (suffix, matrix) in [("deutan", DEUTERANOPIA), ("protan", PROTANOPIA)] {
        let mut sim = rgba.clone();
        simulate_cvd(&mut sim, &matrix);
        let cvd_path = opts.dir.join(format!("{name}.{suffix}.png"));
        match write_png(&cvd_path, width, height, &sim) {
            Ok(()) => out.written += 1,
            Err(e) => {
                let _ = writeln!(log, "{name}.{suffix}: PNG write failed: {e}");
            }
        }
    }
    if let Some(base) = &opts.baseline {
        out.checked += 1;
        let baseline_path = base.join(format!("{name}.png"));
        match read_png_rgba(&baseline_path) {
            Ok((bw, bh, expected)) if bw == width && bh == height => {
                let diff = perceptual_diff(&expected, &rgba, width, height);
                if !diff.passed {
                    out.drifted += 1;
                    let _ = writeln!(
                        log,
                        "{name}: DRIFT interior_ssim={:.5} edge_ssim={:.5}",
                        diff.interior_ssim, diff.edge_ssim
                    );
                }
            }
            Ok((bw, bh, _)) => {
                out.drifted += 1;
                let _ = writeln!(
                    log,
                    "{name}: DRIFT baseline is {bw}x{bh}, fresh frame is {width}x{height}"
                );
            }
            Err(e) => {
                out.drifted += 1;
                let _ = writeln!(log, "{name}: DRIFT no readable baseline ({e})");
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// CVD post-processor (spec A3)
//
// Machado, Oliveira & Fernandes (2009), "A Physiologically-based Model
// for Simulation of Color Vision Deficiency" — severity 1.0 (full
// dichromacy) matrices. They fold the RGB→LMS→simulated-LMS→RGB
// pipeline into one 3×3 acting directly on display-referred sRGB
// triples (the model was fitted on monitor primaries, so no explicit
// linearization is applied); alpha is preserved.
// ---------------------------------------------------------------------------

/// Deuteranopia simulation (Machado 2009, severity 1.0).
const DEUTERANOPIA: [[f32; 3]; 3] = [
    [0.367_322, 0.860_646, -0.227_968],
    [0.280_085, 0.672_501, 0.047_413],
    [-0.011_820, 0.042_940, 0.968_881],
];

/// Protanopia simulation (Machado 2009, severity 1.0).
const PROTANOPIA: [[f32; 3]; 3] = [
    [0.152_286, 1.052_583, -0.204_868],
    [0.114_503, 0.786_281, 0.099_216],
    [-0.003_882, -0.048_116, 1.051_998],
];

/// Applies a CVD simulation matrix to a straight-alpha RGBA8 buffer
/// in place; the alpha channel is passed through untouched.
fn simulate_cvd(rgba: &mut [u8], matrix: &[[f32; 3]; 3]) {
    for px in rgba.as_chunks_mut::<4>().0 {
        let [r, g, b, _a] = *px;
        let (r, g, b) = (f32::from(r), f32::from(g), f32::from(b));
        for (i, row) in matrix.iter().enumerate() {
            px[i] = (row[0] * r + row[1] * g + row[2] * b)
                .clamp(0.0, 255.0)
                .round() as u8;
        }
    }
}

// ---------------------------------------------------------------------------
// PNG encode/decode (png 0.18.1 — pinned in Cargo.toml per spec A1;
// tiny-skia's `png-format` feature stays off).
// ---------------------------------------------------------------------------

/// `TinySkiaBackend::pixels` is premultiplied RGBA; PNG stores
/// straight alpha — divide back out so translucent edges round-trip.
fn unpremultiply(px: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(px.len());
    for p in px.as_chunks::<4>().0 {
        let [r, g, b, a] = *p;
        if a == 0 || a == 255 {
            out.extend_from_slice(&[r, g, b, a]);
        } else {
            let a16 = u16::from(a);
            let un = |c: u8| ((u16::from(c) * 255 + a16 / 2) / a16).min(255) as u8;
            out.extend_from_slice(&[un(r), un(g), un(b), a]);
        }
    }
    out
}

fn io_err<E: Into<Box<dyn std::error::Error + Send + Sync>>>(e: E) -> std::io::Error {
    std::io::Error::other(e)
}

/// Encodes straight-alpha RGBA8 pixels as PNG, creating parent dirs.
fn write_png(path: &Path, width: u32, height: u32, rgba: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = std::fs::File::create(path)?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().map_err(io_err)?;
    writer.write_image_data(rgba).map_err(io_err)
}

/// Decodes a PNG into `(width, height, straight RGBA8)`, expanding
/// indexed/16-bit/low-depth sources and synthesizing alpha for
/// alpha-less color types so baseline comparisons always see RGBA.
fn read_png_rgba(path: &Path) -> std::io::Result<(u32, u32, Vec<u8>)> {
    let file = std::fs::File::open(path)?;
    let mut dec = png::Decoder::new(std::io::BufReader::new(file));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = dec.read_info().map_err(io_err)?;
    let Some(size) = reader.output_buffer_size() else {
        return Err(io_err("baseline declares no decodable frame"));
    };
    let mut buf = vec![0u8; size];
    let info = reader.next_frame(&mut buf).map_err(io_err)?;
    buf.truncate(info.buffer_size());
    let px_count = info.width as usize * info.height as usize;
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => {
            let mut out = Vec::with_capacity(px_count * 4);
            for t in buf.as_chunks::<3>().0.iter().take(px_count) {
                out.extend_from_slice(&[t[0], t[1], t[2], 255]);
            }
            out
        }
        png::ColorType::Grayscale => {
            let mut out = Vec::with_capacity(px_count * 4);
            for &g in buf.iter().take(px_count) {
                out.extend_from_slice(&[g, g, g, 255]);
            }
            out
        }
        png::ColorType::GrayscaleAlpha => {
            let mut out = Vec::with_capacity(px_count * 4);
            for ga in buf.as_chunks::<2>().0.iter().take(px_count) {
                out.extend_from_slice(&[ga[0], ga[0], ga[0], ga[1]]);
            }
            out
        }
        _ => return Err(io_err("unsupported baseline color type")),
    };
    Ok((info.width, info.height, rgba))
}

// ---------------------------------------------------------------------------
// Deterministic font fixture (spec A2)
// ---------------------------------------------------------------------------

/// Builds a [`FontManager`] whose `fontdb` contains ONLY the bundled
/// Fira Mono — [`FontManager::only_fonts`] skips `load_system_fonts`
/// and aliases every generic family to the loaded face, which is what
/// makes the fixture deterministic across hosts.
fn fixture_font_manager() -> FontManager {
    FontManager::only_fonts([FontSource::binary(FIRA_MONO.to_vec())])
}

/// Installs the bundled Fira Mono as the thread-local
/// [`FontManager::new`] override (`martensite_text::font::set_test_fonts`)
/// so `Text` widgets — which lazily construct their own managers —
/// shape through the same fixture as the ambient painter. Hold the
/// guard for the duration of the sweep.
pub fn install_test_fonts() -> martensite::text::font::TestFontsGuard {
    martensite::text::font::set_test_fonts(vec![FontSource::binary(FIRA_MONO.to_vec())])
}

/// A [`TextShaper`] backed by the bundled-font [`FontManager`] —
/// installed as the sweep arenas' ambient painter whenever
/// `dump_frames` is active so ambient text is machine-independent.
///
/// Emission logic mirrors `martensite::text_paint::TextPainter`
/// (same `shape_text` entry point, same per-(line,font) `GlyphRun`
/// splitting, same ink-box model) so the fixture changes WHICH font
/// resolves, never how runs are emitted. `Clone` is cheap — every
/// arena shares one `FontSystem` behind the mutex.
#[derive(Clone)]
pub struct FixtureTextShaper(Arc<Mutex<FontManager>>);

impl FixtureTextShaper {
    /// Builds the bundled-font shaper. Cheap: no system font scan.
    pub fn new() -> Self {
        Self(Arc::new(Mutex::new(fixture_font_manager())))
    }
}

impl Default for FixtureTextShaper {
    fn default() -> Self {
        Self::new()
    }
}

/// Builds cosmic-text [`Attrs`] for a [`TextStyle`] — the same mapping
/// `martensite::text_paint` and the dashboard `TextPainter` apply, so
/// styled chrome text shapes through the same attribute channel in
/// dumped frames as in production.
fn attrs_for(style: TextStyle) -> Attrs<'static> {
    let mut attrs = Attrs::new()
        .weight(Weight(style.weight.0))
        .style(if style.italic {
            Style::Italic
        } else {
            Style::Normal
        });
    if let Some(em) = style.letter_spacing {
        attrs = attrs.letter_spacing(em);
    }
    attrs
}

impl FixtureTextShaper {
    /// Shared emission for both shaper entry points — one `GlyphRun`
    /// per (line, font) segment, tagged with `style` so paint-audit
    /// weight checks see the same metadata production runs carry.
    fn emit(
        &self,
        list: &mut PaintList,
        origin: Point,
        text: &str,
        size_px: f32,
        color: [u8; 4],
        style: TextStyle,
    ) {
        if text.is_empty() || size_px <= 0.0 {
            return;
        }
        let fonts = &mut *self.0.lock();
        let attrs = attrs_for(style);
        for line in shape_text_with_attrs(fonts, text, &attrs, size_px, size_px * 1.25, None) {
            let baseline_y = origin.y as f32 + line.line_y;
            let mut run = GlyphRun::new(size_px, color).with_style(style);
            let mut run_font = None;
            for g in &line.glyphs {
                if let Some(prev) = run_font {
                    if prev != g.font_id {
                        if let Some((data, index)) = fonts.font_data(prev) {
                            run.set_font(FontResource::new(data, index));
                        }
                        list.push_glyph_run(run);
                        run = GlyphRun::new(g.font_size, color).with_style(style);
                    }
                }
                run.font_size = g.font_size;
                run_font = Some(g.font_id);
                run.push(GlyphInstance::new(
                    origin.x as f32 + g.x,
                    baseline_y + g.y,
                    u32::from(g.glyph_id),
                    g.w,
                    line.line_height,
                ));
            }
            if let Some(font_id) = run_font.filter(|_| !run.is_empty()) {
                if let Some((data, index)) = fonts.font_data(font_id) {
                    run.set_font(FontResource::new(data, index));
                }
                list.push_glyph_run(run);
            }
        }
    }
}

impl TextShaper for FixtureTextShaper {
    /// Mirror of `TextPainter::push` — one `GlyphRun` per
    /// (line, font) segment with real font bytes attached.
    fn paint_shaped_text(
        &self,
        list: &mut PaintList,
        origin: Point,
        text: &str,
        size_px: f32,
        color: [u8; 4],
    ) {
        self.emit(list, origin, text, size_px, color, TextStyle::REGULAR);
    }

    /// Styled override — the bundled fixture ships one face, so weight
    /// resolves to the same glyphs, but the style axis still flows
    /// through shaping (tracking DOES change advances) and onto the
    /// emitted `GlyphRun`s for the paint audit's weight checks.
    fn paint_shaped_text_styled(
        &self,
        list: &mut PaintList,
        origin: Point,
        text: &str,
        size_px: f32,
        color: [u8; 4],
        style: TextStyle,
    ) {
        self.emit(list, origin, text, size_px, color, style);
    }

    /// Mirror of `TextPainter::measure` — max glyph advance end.
    fn measure_text(&self, text: &str, size_px: f32) -> Option<f32> {
        self.measure_text_styled(text, size_px, TextStyle::REGULAR)
    }

    fn measure_text_styled(&self, text: &str, size_px: f32, style: TextStyle) -> Option<f32> {
        let fonts = &mut *self.0.lock();
        let attrs = attrs_for(style);
        let mut end = 0.0f32;
        for line in shape_text_with_attrs(fonts, text, &attrs, size_px, size_px * 1.25, None) {
            for g in &line.glyphs {
                end = end.max(g.x + g.w);
            }
        }
        Some(end)
    }

    /// Mirror of `TextPainter::ink_bounds` — union of per-glyph ink
    /// boxes (`baseline − 0.8·size … baseline + 0.25·size`), matching
    /// what the paint audit probes.
    fn ink_bounds(&self, origin: Point, text: &str, size_px: f32) -> Option<Rect> {
        self.ink_bounds_styled(origin, text, size_px, TextStyle::REGULAR)
    }

    fn ink_bounds_styled(
        &self,
        origin: Point,
        text: &str,
        size_px: f32,
        style: TextStyle,
    ) -> Option<Rect> {
        if text.is_empty() || size_px <= 0.0 {
            return Some(Rect::ZERO);
        }
        let fonts = &mut *self.0.lock();
        let attrs = attrs_for(style);
        let mut box_: Option<Rect> = None;
        for line in shape_text_with_attrs(fonts, text, &attrs, size_px, size_px * 1.25, None) {
            let baseline_y = origin.y as f32 + line.line_y;
            for g in &line.glyphs {
                let glyph = Rect::new(
                    f64::from(origin.x as f32 + g.x),
                    f64::from(baseline_y + g.y - g.font_size * 0.8),
                    f64::from(origin.x as f32 + g.x + g.w),
                    f64::from(baseline_y + g.y + g.font_size * 0.25),
                );
                box_ = Some(box_.map_or(glyph, |b: Rect| b.union(glyph)));
            }
        }
        box_
    }
}
