//! Media family — viewers, players, and document widgets.

use glam::Vec2;
use martensite::core::paint::ImageData;
use martensite::core::SurfaceId;
use martensite::widgets::attachment::Attachment;
use martensite::widgets::captions::Captions;
use martensite::widgets::code_view::CodeView;
use martensite::widgets::coverflow::Coverflow;
use martensite::widgets::crop_box::CropBox;
use martensite::widgets::diff_view::{DiffKind, DiffView};
use martensite::widgets::download_item::DownloadItem;
use martensite::widgets::external::ExternalEngine;
use martensite::widgets::filmstrip::{Filmstrip, Thumbnail};
use martensite::widgets::hex_view::HexView;
use martensite::widgets::image::Image;
use martensite::widgets::image_viewer::ImageViewer;
use martensite::widgets::ink_canvas::InkCanvas;
use martensite::widgets::lightbox::Lightbox;
use martensite::widgets::magnifier::Magnifier;
use martensite::widgets::markdown::Markdown;
use martensite::widgets::media::MediaView;
use martensite::widgets::media_controls::MediaControls;
use martensite::widgets::merge_view::MergeView;
use martensite::widgets::minimap::Minimap;
use martensite::widgets::now_playing::NowPlaying;
use martensite::widgets::pdf_view::PdfView;
use martensite::widgets::playlist::{Playlist, Track};
use martensite::widgets::terminal::Terminal;
use martensite::widgets::text::Text;
use martensite::widgets::webview::WebView;
use martensite_engine_bridge::BridgeHandle;

use crate::page::{Page, PropSpec};
use crate::pages::{downcast_mut, meta, page, SnipProp};

/// 64×64 checkerboard demo image.
fn demo_image() -> ImageData {
    let mut px = vec![0u8; 64 * 64 * 4];
    for y in 0..64u32 {
        for x in 0..64u32 {
            let i = ((y * 64 + x) * 4) as usize;
            let on = (x / 8 + y / 8) % 2 == 0;
            px[i] = if on { 90 } else { 140 };
            px[i + 1] = if on { 140 } else { 160 };
            px[i + 2] = 255;
            px[i + 3] = 255;
        }
    }
    ImageData::from_rgba(64, 64, px).expect("valid RGBA buffer")
}

page!(TextPage {
    meta: meta(
        "Text",
        "Media",
        "Shaped text — the fundamental label widget.",
        "Text",
        &[
            ("Qt", "QLabel"),
            ("GTK", "GtkLabel"),
            ("SwiftUI", "Text"),
            ("HTML", "<p>")
        ],
        false,
    ),
    props: &[
        PropSpec::Text {
            key: "content",
            label: "Content",
            default: "The quick brown fox"
        },
        PropSpec::Float {
            key: "size",
            label: "Size",
            min: 8.0,
            max: 48.0,
            step: 1.0,
            default: 14.0
        },
        PropSpec::Bool {
            key: "rtl",
            label: "RTL",
            default: false
        },
        PropSpec::Float {
            key: "line_height",
            label: "Line Height",
            min: 0.0,
            max: 64.0,
            step: 0.5,
            default: 0.0
        },
        PropSpec::Text {
            key: "family",
            label: "Family",
            default: ""
        },
        PropSpec::Float {
            key: "letter_spacing",
            label: "Letter Spacing",
            min: 0.0,
            max: 64.0,
            step: 0.5,
            default: 0.0
        },
        PropSpec::Bool {
            key: "focused",
            label: "Focused",
            default: false
        },
        PropSpec::Int {
            key: "font_weight",
            label: "Font Weight",
            min: 0,
            max: 100,
            default: 0
        },
        PropSpec::Text {
            key: "color",
            label: "Color",
            default: ""
        },
    ],
    build: |p| {
        let mut t = Text::new(p.str("content")).font_size(p.f64("size") as f32);
        if p.bool("rtl") {
            t = t.rtl();
        }
        {
            let mut __w = t;
            if p.f64("line_height") != 0.0 {
                __w = __w.line_height(p.f64("line_height") as f32);
            }
            if !p.str("family").is_empty() {
                __w = __w.family(p.str("family"));
            }
            if p.f64("letter_spacing") != 0.0 {
                __w = __w.letter_spacing(p.f64("letter_spacing") as f32);
            }
            if p.bool("focused") {
                __w = __w.focused(p.bool("focused"));
            }
            if p.i64("font_weight") != 0 {
                __w = __w.font_weight(martensite_core::FontWeight(p.i64("font_weight") as u16));
            }
            if let Some([r, g, b, _]) = crate::pages::parse_rgba(p.str("color")) {
                __w = __w.color(martensite_theme::Oklab::from_srgb(
                    r as f32 / 255.0,
                    g as f32 / 255.0,
                    b as f32 / 255.0,
                ));
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = format!(
            "Text::new({:?}).font_size({:?}).rtl({})",
            p.str("content"),
            p.f64("size") as f32,
            p.bool("rtl"),
        );
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("line_height", ".line_height", SnipProp::Float(0.0)),
                ("family", ".family", SnipProp::Text("")),
                ("letter_spacing", ".letter_spacing", SnipProp::Float(0.0)),
                ("focused", ".focused", SnipProp::Bool(false)),
            ],
        ));
        if p.i64("font_weight") != 0 {
            __s.push_str(&format!(
                "\n    .font_weight(FontWeight({}))",
                p.i64("font_weight")
            ));
        }
        __s.push_str(&crate::pages::snip_textmap(
            p,
            "color",
            ".color",
            "",
            crate::pages::expr_oklab,
        ));
        __s
    },
});

page!(MarkdownPage {
    meta: meta(
        "Markdown",
        "Media",
        "Rendered markdown — headings, lists, links, code.",
        "Text",
        &[
            ("HTML", "markdown"),
            ("Qt", "QTextBrowser"),
            ("React", "react-markdown"),
            ("GTK", "custom")
        ],
        false,
    ),
    props: &[
        PropSpec::Float {
            key: "size",
            label: "Base size",
            min: 10.0,
            max: 24.0,
            step: 1.0,
            default: 14.0,
        },
        PropSpec::Header {
            label: "State & Accessibility"
        },
        PropSpec::Bool {
            key: "enabled",
            label: "Enabled",
            default: true
        },
        PropSpec::Text {
            key: "a11y_label",
            label: "A11y label",
            default: ""
        },
    ],
    build: |p| {
        let mut __w = Markdown::new(
            "# Heading\n\nBody text with **bold** and *italic*.\n\n- Item A\n- Item B",
        )
        .base_size(p.f64("size") as f32);
        __w = __w.enabled(p.bool("enabled"));
        if !p.str("a11y_label").is_empty() {
            __w = __w.label(p.str("a11y_label"));
        }
        Box::new(__w)
    },
    snippet: |p| {
        let mut __s = format!(
            "Markdown::new(source).base_size({:?})",
            p.f64("size") as f32
        );
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("enabled", ".enabled", SnipProp::Bool(true)),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(m) = downcast_mut::<Markdown>(w) {
            if let Some(link) = m.take_link_clicked() {
                out.push(format!("link → {link}"));
            }
        }
    },
});

page!(CodeViewPage {
    meta: meta(
        "CodeView",
        "Media",
        "Monospace code listing with line numbers.",
        "Text",
        &[
            ("Qt", "code editor"),
            ("VS Code", "editor"),
            ("React", "code block"),
            ("GTK", "GtkSourceView")
        ],
        false,
    ),
    props: &[
        PropSpec::Bool {
            key: "follow",
            label: "Follow",
            default: true
        },
        PropSpec::Int {
            key: "current",
            label: "Current",
            min: 0,
            max: 100,
            default: 0
        },
        PropSpec::Header {
            label: "State & Accessibility"
        },
        PropSpec::Text {
            key: "a11y_label",
            label: "A11y label",
            default: ""
        },
    ],
    build: |_p| {
        let mut __w = CodeView::new().lines([
            "fn main() {",
            "    let app = App::new();",
            "    app.run();",
            "}",
        ]);
        if !_p.str("a11y_label").is_empty() {
            __w = __w.label(_p.str("a11y_label"));
        }
        if !_p.bool("follow") {
            __w = __w.follow(_p.bool("follow"));
        }
        if _p.i64("current") != 0 {
            __w = __w.current(Some(_p.i64("current") as usize));
        }
        Box::new(__w)
    },
    snippet: |_p| {
        let mut __s = "CodeView::new().lines([\"fn main() {\", \"}\"])".to_string();
        __s.push_str(&crate::pages::prop_snippet(
            _p,
            &[
                ("follow", ".follow", SnipProp::Bool(true)),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        if _p.i64("current") != 0 {
            __s.push_str(&format!("\n    .current(Some({}))", _p.i64("current")));
        }
        __s
    },
    poll: |w, out| {
        if let Some(cv) = downcast_mut::<CodeView>(w) {
            if let Some(i) = cv.take_selected() {
                out.push(format!("line {i}"));
            }
        }
    },
});

page!(DiffViewPage {
    meta: meta(
        "DiffView",
        "Media",
        "Unified diff view — added/removed/context lines.",
        "Text",
        &[
            ("Git", "diff"),
            ("VS Code", "diff editor"),
            ("Qt", "custom"),
            ("React", "diff viewer")
        ],
        false,
    ),
    props: &[
        PropSpec::Choice {
            key: "line_kind",
            label: "Line Kind",
            options: &["Context", "Added", "Removed", "Hunk"],
            default: 0
        },
        PropSpec::Text {
            key: "line_text",
            label: "Line Text",
            default: ""
        },
        PropSpec::Header {
            label: "State & Accessibility"
        },
        PropSpec::Text {
            key: "a11y_label",
            label: "A11y label",
            default: ""
        },
    ],
    build: |_p| {
        let mut __w = DiffView::new().lines(vec![
            (DiffKind::Hunk, "@@ -1,4 +1,4 @@".into()),
            (DiffKind::Context, " fn main() {".into()),
            (DiffKind::Removed, "-    old_call()".into()),
            (DiffKind::Added, "+    new_call()".into()),
            (DiffKind::Context, " }".into()),
        ]);
        if !_p.str("a11y_label").is_empty() {
            __w = __w.label(_p.str("a11y_label"));
        }
        if _p.choice("line_kind") != 0 || !_p.str("line_text").is_empty() {
            __w = __w.line(
                match _p.choice("line_kind") {
                    0 => martensite::widgets::diff_view::DiffKind::Context,
                    1 => martensite::widgets::diff_view::DiffKind::Added,
                    2 => martensite::widgets::diff_view::DiffKind::Removed,
                    3 => martensite::widgets::diff_view::DiffKind::Hunk,
                    _ => martensite::widgets::diff_view::DiffKind::Context,
                },
                _p.str("line_text"),
            );
        }
        Box::new(__w)
    },
    snippet: |_p| {
        let mut __s =
            { "DiffView::new().lines(vec![(DiffKind::Added, \"+ line\".into())])".to_string() };
        __s.push_str(&crate::pages::prop_snippet(
            _p,
            &[("a11y_label", ".label", SnipProp::Text(""))],
        ));
        if _p.choice("line_kind") != 0 || !_p.str("line_text").is_empty() {
            __s.push_str(&format!(
                "\n    .line({}, {:?})",
                [
                    "martensite::widgets::diff_view::DiffKind::Context",
                    "martensite::widgets::diff_view::DiffKind::Added",
                    "martensite::widgets::diff_view::DiffKind::Removed",
                    "martensite::widgets::diff_view::DiffKind::Hunk"
                ][_p.choice("line_kind")],
                _p.str("line_text")
            ));
        }
        __s
    },
    poll: |w, out| {
        if let Some(dv) = downcast_mut::<DiffView>(w) {
            if let Some(i) = dv.take_selected() {
                out.push(format!("line {i}"));
            }
        }
    },
});

page!(MergeViewPage {
    meta: meta(
        "MergeView",
        "Media",
        "Three-way merge — ours/theirs with conflict choices.",
        "Table",
        &[
            ("Git", "merge tool"),
            ("VS Code", "merge editor"),
            ("Qt", "custom"),
            ("React", "merge view")
        ],
        false,
    ),
    props: &[
        PropSpec::Text {
            key: "row_ours",
            label: "Row Ours",
            default: ""
        },
        PropSpec::Text {
            key: "row_result",
            label: "Row Result",
            default: ""
        },
        PropSpec::Text {
            key: "row_theirs",
            label: "Row Theirs",
            default: ""
        },
        PropSpec::Header {
            label: "State & Accessibility"
        },
        PropSpec::Text {
            key: "a11y_label",
            label: "A11y label",
            default: ""
        },
    ],
    build: |_p| {
        let mut __w = MergeView::new();
        if !_p.str("a11y_label").is_empty() {
            __w = __w.label(_p.str("a11y_label"));
        }
        if !_p.str("row_ours").is_empty() {
            __w = __w.row(martensite::widgets::merge_view::MergeRow::aligned(
                _p.str("row_ours"),
                _p.str("row_result"),
                _p.str("row_theirs"),
            ));
        }
        Box::new(__w)
    },
    snippet: |_p| {
        let mut __s = "MergeView::new()".to_string();
        __s.push_str(&crate::pages::prop_snippet(
            _p,
            &[("a11y_label", ".label", SnipProp::Text(""))],
        ));
        if !_p.str("row_ours").is_empty() {
            __s.push_str(&format!(
                "\n    .row(MergeRow::aligned({:?}, {:?}, {:?}))",
                _p.str("row_ours"),
                _p.str("row_result"),
                _p.str("row_theirs")
            ));
        }
        __s
    },
    poll: |w, out| {
        if let Some(mv) = downcast_mut::<MergeView>(w) {
            if let Some(c) = mv.take_choice() {
                out.push(format!("choice {c:?}"));
            }
        }
    },
});

page!(HexViewPage {
    meta: meta(
        "HexView",
        "Media",
        "Hex editor — offset, bytes, ASCII column.",
        "Table",
        &[
            ("HxD", "hex editor"),
            ("Qt", "QHexView"),
            ("CLI", "xxd"),
            ("React", "hex view")
        ],
        false,
    ),
    props: &[
        PropSpec::Int {
            key: "len",
            label: "Bytes",
            min: 16,
            max: 512,
            default: 96
        },
        PropSpec::Header {
            label: "State & Accessibility"
        },
        PropSpec::Text {
            key: "a11y_label",
            label: "A11y label",
            default: ""
        },
    ],
    build: |p| {
        let mut __w = HexView::new().bytes((0..p.i64("len") as u8).collect::<Vec<u8>>());
        if !p.str("a11y_label").is_empty() {
            __w = __w.label(p.str("a11y_label"));
        }
        Box::new(__w)
    },
    snippet: |p| {
        let mut __s = format!("HexView::new().bytes(vec![0u8; {}])", p.i64("len"));
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[("a11y_label", ".label", SnipProp::Text(""))],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(hv) = downcast_mut::<HexView>(w) {
            if let Some(off) = hv.take_selected() {
                out.push(format!("offset 0x{off:x}"));
            }
        }
    },
});

page!(TerminalPage {
    meta: meta(
        "Terminal",
        "Media",
        "Terminal emulator surface — prompt + scrollback.",
        "Text",
        &[
            ("xterm", "terminal"),
            ("Qt", "QTermWidget"),
            ("VS Code", "terminal"),
            ("GTK", "VTE")
        ],
        false,
    ),
    props: &[
        PropSpec::Choice {
            key: "sanitize",
            label: "Sanitize",
            options: &["Aggressive", "Baseline", "Raw"],
            default: 0,
        },
        PropSpec::Text {
            key: "prompt",
            label: "Prompt",
            default: ">"
        },
        PropSpec::Header {
            label: "State & Accessibility"
        },
        PropSpec::Text {
            key: "a11y_label",
            label: "A11y label",
            default: ""
        },
    ],
    build: |p| {
        let mut t = Terminal::new().lines(["$ cargo build", "   Compiling martensite…"]);
        t.set_sanitizer(crate::pages::sanitize_cfg(p));
        t.submit_line();
        {
            let mut __w = t;
            if !p.str("a11y_label").is_empty() {
                __w = __w.label(p.str("a11y_label"));
            }
            if p.str("prompt") != ">" {
                __w = __w.prompt(p.str("prompt"));
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = {
            let base: String = { "Terminal::new().lines([\"$ cargo build\"])".to_string() };
            base + crate::pages::sanitize_snippet(p)
        };
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("prompt", ".prompt", SnipProp::Text(">")),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(t) = downcast_mut::<Terminal>(w) {
            if let Some(cmd) = t.take_submitted() {
                out.push(format!("submitted → {cmd}"));
            }
        }
    },
});

page!(PdfViewPage {
    meta: meta(
        "PdfView",
        "Media",
        "PDF page viewer — navigation + zoom.",
        "Document",
        &[
            ("Qt", "QPdfView"),
            ("GTK", "Evince"),
            ("macOS", "PDFKit"),
            ("React", "pdf viewer")
        ],
        false,
    ),
    props: &[
        PropSpec::Header {
            label: "State & Accessibility"
        },
        PropSpec::Text {
            key: "a11y_label",
            label: "A11y label",
            default: ""
        },
    ],
    build: |_p| {
        let mut __w = PdfView::new();
        if !_p.str("a11y_label").is_empty() {
            __w = __w.label(_p.str("a11y_label"));
        }
        Box::new(__w)
    },
    snippet: |_p| {
        let mut __s = "PdfView::new().with_document(doc)".to_string();
        __s.push_str(&crate::pages::prop_snippet(
            _p,
            &[("a11y_label", ".label", SnipProp::Text(""))],
        ));
        __s
    },
});

page!(ImagePage {
    meta: meta(
        "Image",
        "Media",
        "Raster image with fit modes and alt text.",
        "Image",
        &[
            ("Qt", "QLabel pixmap"),
            ("GTK", "GtkImage"),
            ("HTML", "<img>"),
            ("SwiftUI", "Image")
        ],
        false,
    ),
    props: &[PropSpec::Choice {
        key: "fit",
        label: "Fit",
        options: &["Contain", "Cover", "Fill", "None"],
        default: 0
    },],
    build: |_p| {
        let mut __w = Image::new(demo_image()).alt("Checkerboard");
        if _p.choice("fit") != 0 {
            __w = __w.fit(match _p.choice("fit") {
                0 => martensite::widgets::image::ImageFit::Contain,
                1 => martensite::widgets::image::ImageFit::Cover,
                2 => martensite::widgets::image::ImageFit::Fill,
                3 => martensite::widgets::image::ImageFit::None,
                _ => martensite::widgets::image::ImageFit::Contain,
            });
        }
        Box::new(__w)
    },
    snippet: |_p| {
        let mut __s = "Image::new(image_data).alt(\"Checkerboard\")".to_string();
        __s.push_str(&crate::pages::prop_snippet(
            _p,
            &[(
                "fit",
                ".fit",
                SnipProp::Choice(&[
                    "martensite::widgets::image::ImageFit::Contain",
                    "martensite::widgets::image::ImageFit::Cover",
                    "martensite::widgets::image::ImageFit::Fill",
                    "martensite::widgets::image::ImageFit::None",
                ]),
            )],
        ));
        __s
    },
});

page!(ImageViewerPage {
    meta: meta(
        "ImageViewer",
        "Media",
        "Pan/zoom image viewer with checkerboard underlay.",
        "Image",
        &[
            ("macOS", "Preview"),
            ("Qt", "QGraphicsView"),
            ("React", "image viewer"),
            ("GTK", "custom")
        ],
        false,
    ),
    props: &[
        PropSpec::Bool {
            key: "checker",
            label: "Checkerboard",
            default: true
        },
        PropSpec::Header {
            label: "State & Accessibility"
        },
        PropSpec::Bool {
            key: "enabled",
            label: "Enabled",
            default: true
        },
        PropSpec::Text {
            key: "a11y_label",
            label: "A11y label",
            default: ""
        },
    ],
    build: |p| {
        let mut __w = ImageViewer::new(demo_image()).checker(p.bool("checker"));
        __w = __w.enabled(p.bool("enabled"));
        if !p.str("a11y_label").is_empty() {
            __w = __w.label(p.str("a11y_label"));
        }
        Box::new(__w)
    },
    snippet: |p| {
        let mut __s = format!("ImageViewer::new(img).checker({})", p.bool("checker"));
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("enabled", ".enabled", SnipProp::Bool(true)),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        __s
    },
    state: |w| {
        downcast_mut::<ImageViewer>(w)
            .map(|v| vec![("zoom".to_string(), format!("{:.2}", v.zoom()))])
            .unwrap_or_default()
    },
});

page!(MagnifierPage {
    meta: meta(
        "Magnifier",
        "Media",
        "Lens magnifier — zoomed pixel region.",
        "Image",
        &[
            ("macOS", "Digital Color Meter"),
            ("iOS", "loupe"),
            ("Qt", "custom"),
            ("React", "magnifier")
        ],
        false,
    ),
    props: &[
        PropSpec::Float {
            key: "zoom",
            label: "Zoom",
            min: 1.0,
            max: 16.0,
            step: 1.0,
            default: 4.0,
        },
        PropSpec::Bool {
            key: "zoom_caption",
            label: "Zoom Caption",
            default: false
        },
        PropSpec::Header {
            label: "State & Accessibility"
        },
        PropSpec::Text {
            key: "a11y_label",
            label: "A11y label",
            default: ""
        },
    ],
    build: |p| {
        let mut m = Magnifier::new()
            .source(demo_image())
            .zoom(p.f64("zoom") as f32);
        m.set_focus(Vec2::new(32.0, 32.0));
        {
            let mut __w = m;
            if !p.str("a11y_label").is_empty() {
                __w = __w.label(p.str("a11y_label"));
            }
            if p.bool("zoom_caption") {
                __w = __w.zoom_caption(p.bool("zoom_caption"));
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = format!(
            "Magnifier::new().source(img).zoom({:?})",
            p.f64("zoom") as f32
        );
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("zoom_caption", ".zoom_caption", SnipProp::Bool(false)),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        __s
    },
});

page!(MinimapPage {
    meta: meta(
        "Minimap",
        "Media",
        "Code-style minimap — document overview strip.",
        "ScrollBar",
        &[
            ("Sublime", "minimap"),
            ("VS Code", "minimap"),
            ("Qt", "custom"),
            ("React", "minimap")
        ],
        false,
    ),
    props: &[
        PropSpec::Float {
            key: "scroll",
            label: "Scroll",
            min: -10.0,
            max: 100.0,
            step: 1.0,
            default: 0.0
        },
        PropSpec::Float {
            key: "viewport",
            label: "Viewport",
            min: -9.625,
            max: 100.0,
            step: 1.0,
            default: 0.25
        },
        PropSpec::Header {
            label: "State & Accessibility"
        },
        PropSpec::Text {
            key: "a11y_label",
            label: "A11y label",
            default: ""
        },
    ],
    build: |_p| {
        let mut __w = Minimap::new().lines((3..20).step_by(2));
        if !_p.str("a11y_label").is_empty() {
            __w = __w.label(_p.str("a11y_label"));
        }
        if _p.f64("scroll") != 0.0 {
            __w = __w.scroll(_p.f64("scroll") as f32);
        }
        if _p.f64("viewport") != 0.25 {
            __w = __w.viewport(_p.f64("viewport") as f32);
        }
        Box::new(__w)
    },
    snippet: |_p| {
        let mut __s = "Minimap::new().lines(line_lengths)".to_string();
        __s.push_str(&crate::pages::prop_snippet(
            _p,
            &[
                ("scroll", ".scroll", SnipProp::Float(0.0)),
                ("viewport", ".viewport", SnipProp::Float(0.25)),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(mm) = downcast_mut::<Minimap>(w) {
            if let Some(f) = mm.take_scrolled() {
                out.push(format!("scroll → {f:.2}"));
            }
        }
    },
});

page!(CropBoxPage {
    meta: meta(
        "CropBox",
        "Media",
        "Image crop rectangle with draggable handles.",
        "Image",
        &[
            ("Photos", "crop tool"),
            ("Qt", "crop rect"),
            ("React", "react-cropper"),
            ("macOS", "crop")
        ],
        false,
    ),
    props: &[
        PropSpec::Float {
            key: "aspect",
            label: "Aspect",
            min: 0.0,
            max: 3.0,
            step: 0.25,
            default: 0.0,
        },
        PropSpec::Float {
            key: "min_size",
            label: "Min Size",
            min: 0.0,
            max: 64.0,
            step: 0.5,
            default: 0.0
        },
        PropSpec::Text {
            key: "crop",
            label: "Crop (csv)",
            default: ""
        },
        PropSpec::Header {
            label: "State & Accessibility"
        },
        PropSpec::Text {
            key: "a11y_label",
            label: "A11y label",
            default: ""
        },
    ],
    build: |p| {
        let mut c = CropBox::new().aspect_ratio(p.f64("aspect") as f32);
        c.set_crop(0.2, 0.2, 0.6, 0.6);
        {
            let mut __w = c;
            if !p.str("a11y_label").is_empty() {
                __w = __w.label(p.str("a11y_label"));
            }
            if p.f64("min_size") != 0.0 {
                __w = __w.min_size(p.f64("min_size") as f32);
            }
            if let Some(v) = crate::pages::parse_quad(p.str("crop")) {
                __w = __w.crop(v.0 as f32, v.1 as f32, v.2 as f32, v.3 as f32);
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = format!("CropBox::new().aspect_ratio({:?})", p.f64("aspect") as f32);
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("min_size", ".min_size", SnipProp::Float(0.0)),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        __s.push_str(&crate::pages::snip_textmap(
            p,
            "crop",
            ".crop",
            "",
            crate::pages::expr_quad,
        ));
        __s
    },
    poll: |w, out| {
        if let Some(c) = downcast_mut::<CropBox>(w) {
            if let Some(rect) = c.take_changed() {
                out.push(format!("crop → {rect:?}"));
            }
        }
    },
});

page!(InkCanvasPage {
    meta: meta(
        "InkCanvas",
        "Media",
        "Freehand pen strokes — draw with pointer.",
        "Canvas",
        &[
            ("iPadOS", "PencilKit"),
            ("Qt", "paint canvas"),
            ("React", "signature pad"),
            ("Windows", "InkCanvas")
        ],
        false,
    ),
    props: &[
        PropSpec::Float {
            key: "pen",
            label: "Pen",
            min: -10.0,
            max: 100.0,
            step: 1.0,
            default: 0.0
        },
        PropSpec::Header {
            label: "State & Accessibility"
        },
        PropSpec::Bool {
            key: "enabled",
            label: "Enabled",
            default: true
        },
        PropSpec::Text {
            key: "a11y_label",
            label: "A11y label",
            default: ""
        },
    ],
    build: |_p| {
        let mut __w = InkCanvas::new();
        __w = __w.enabled(_p.bool("enabled"));
        if !_p.str("a11y_label").is_empty() {
            __w = __w.label(_p.str("a11y_label"));
        }
        if _p.f64("pen") != 0.0 {
            __w = __w.pen(_p.f64("pen") as f32);
        }
        Box::new(__w)
    },
    snippet: |_p| {
        let mut __s = "InkCanvas::new()".to_string();
        __s.push_str(&crate::pages::prop_snippet(
            _p,
            &[
                ("pen", ".pen", SnipProp::Float(0.0)),
                ("enabled", ".enabled", SnipProp::Bool(true)),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(c) = downcast_mut::<InkCanvas>(w) {
            if let Some(stroke) = c.take_stroke() {
                out.push(format!("stroke {stroke:?}"));
            }
        }
    },
});

page!(MediaViewPage {
    meta: meta(
        "MediaView",
        "Media",
        "Video surface — hardware-decoded frame sink.",
        "Image",
        &[
            ("Qt", "QVideoWidget"),
            ("GTK", "GtkPicture"),
            ("AVKit", "AVPlayerLayer"),
            ("React", "<video>")
        ],
        false,
    ),
    props: &[
        PropSpec::Choice {
            key: "with_fit",
            label: "With Fit",
            options: &["Contain", "Cover", "Fill", "Fixed"],
            default: 0
        },
        PropSpec::Float {
            key: "with_aspect_ratio",
            label: "With Aspect Ratio",
            min: 0.0,
            max: 1.0,
            step: 0.05,
            default: 0.0
        },
    ],
    build: |_p| {
        let mut __w = MediaView::new();
        if _p.choice("with_fit") != 0 {
            __w = __w.with_fit(match _p.choice("with_fit") {
                0 => martensite::widgets::media::VideoFit::Contain,
                1 => martensite::widgets::media::VideoFit::Cover,
                2 => martensite::widgets::media::VideoFit::Fill,
                3 => martensite::widgets::media::VideoFit::Fixed,
                _ => martensite::widgets::media::VideoFit::Contain,
            });
        }
        if _p.f64("with_aspect_ratio") != 0.0 {
            __w = __w.with_aspect_ratio(_p.f64("with_aspect_ratio") as f32);
        }
        Box::new(__w)
    },
    snippet: |_p| {
        let mut __s = "MediaView::new().with_decoder(decoder)".to_string();
        __s.push_str(&crate::pages::prop_snippet(
            _p,
            &[
                (
                    "with_fit",
                    ".with_fit",
                    SnipProp::Choice(&[
                        "martensite::widgets::media::VideoFit::Contain",
                        "martensite::widgets::media::VideoFit::Cover",
                        "martensite::widgets::media::VideoFit::Fill",
                        "martensite::widgets::media::VideoFit::Fixed",
                    ]),
                ),
                (
                    "with_aspect_ratio",
                    ".with_aspect_ratio",
                    SnipProp::Float(0.0),
                ),
            ],
        ));
        __s
    },
});

page!(MediaControlsPage {
    meta: meta(
        "MediaControls",
        "Media",
        "Transport bar — play/seek/volume/fullscreen.",
        "Toolbar",
        &[
            ("HTML", "video controls"),
            ("Qt", "media controls"),
            ("iOS", "playback bar"),
            ("React", "player controls")
        ],
        false,
    ),
    props: &[
        PropSpec::Float {
            key: "position",
            label: "Position",
            min: 0.0,
            max: 1.0,
            step: 0.05,
            default: 0.35,
        },
        PropSpec::Float {
            key: "duration",
            label: "Duration",
            min: 0.0,
            max: 1.0,
            step: 0.05,
            default: 0.0
        },
        PropSpec::Float {
            key: "volume",
            label: "Volume",
            min: 0.0,
            max: 1.0,
            step: 0.05,
            default: 1.0
        },
        PropSpec::Bool {
            key: "show_volume",
            label: "Show Volume",
            default: true
        },
        PropSpec::Bool {
            key: "show_fullscreen",
            label: "Show Fullscreen",
            default: true
        },
        PropSpec::Header {
            label: "State & Accessibility"
        },
        PropSpec::Bool {
            key: "enabled",
            label: "Enabled",
            default: true
        },
        PropSpec::Text {
            key: "a11y_label",
            label: "A11y label",
            default: ""
        },
    ],
    build: |p| {
        let mut mc = MediaControls::new();
        mc.set_duration(120.0);
        mc.set_position(p.f64("position") * 120.0);
        {
            let mut __w = mc;
            __w = __w.enabled(p.bool("enabled"));
            if !p.str("a11y_label").is_empty() {
                __w = __w.a11y_label(p.str("a11y_label"));
            }
            if p.f64("duration") != 0.0 {
                __w = __w.duration(p.f64("duration"));
            }
            if p.f64("volume") != 1.0 {
                __w = __w.volume(p.f64("volume") as f32);
            }
            if !p.bool("show_volume") {
                __w = __w.show_volume(p.bool("show_volume"));
            }
            if !p.bool("show_fullscreen") {
                __w = __w.show_fullscreen(p.bool("show_fullscreen"));
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = format!(
            "let mut mc = MediaControls::new();\nmc.set_position({:?});",
            p.f64("position") as f32 * 120.0,
        );
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("duration", ".duration", SnipProp::Float(0.0)),
                ("volume", ".volume", SnipProp::Float(1.0)),
                ("show_volume", ".show_volume", SnipProp::Bool(true)),
                ("show_fullscreen", ".show_fullscreen", SnipProp::Bool(true)),
                ("enabled", ".enabled", SnipProp::Bool(true)),
                ("a11y_label", ".a11y_label", SnipProp::Text("")),
            ],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(mc) = downcast_mut::<MediaControls>(w) {
            if mc.take_play_toggled() {
                out.push(format!("playing → {}", mc.playing()));
            }
            if let Some(pos) = mc.take_seek() {
                out.push(format!("seek → {pos:.1}"));
            }
        }
    },
});

page!(NowPlayingPage {
    meta: meta(
        "NowPlaying",
        "Media",
        "Now-playing card — art, title, artist, scrubber.",
        "Group",
        &[
            ("iOS", "Now Playing"),
            ("Spotify", "player card"),
            ("Qt", "custom"),
            ("React", "music card")
        ],
        false,
    ),
    props: &[
        PropSpec::Text {
            key: "title",
            label: "Title",
            default: "Night Shift"
        },
        PropSpec::Text {
            key: "artist",
            label: "Artist",
            default: "Martensite"
        },
        PropSpec::Text {
            key: "album",
            label: "Album",
            default: ""
        },
        PropSpec::Text {
            key: "art_text",
            label: "Art Text",
            default: ""
        },
        PropSpec::Float {
            key: "position",
            label: "Position",
            min: -10.0,
            max: 100.0,
            step: 1.0,
            default: 0.0
        },
        PropSpec::Float {
            key: "duration",
            label: "Duration",
            min: 0.0,
            max: 1.0,
            step: 0.05,
            default: 0.0
        },
        PropSpec::Text {
            key: "art_color",
            label: "Art Color",
            default: ""
        },
        PropSpec::Header {
            label: "State & Accessibility"
        },
        PropSpec::Text {
            key: "a11y_label",
            label: "A11y label",
            default: ""
        },
    ],
    build: |p| {
        let mut __w = NowPlaying::new(p.str("title"), p.str("artist"));
        if !p.str("a11y_label").is_empty() {
            __w = __w.label(p.str("a11y_label"));
        }
        if !p.str("album").is_empty() {
            __w = __w.album(p.str("album"));
        }
        if !p.str("art_text").is_empty() {
            __w = __w.art_text(p.str("art_text"));
        }
        if p.f64("position") != 0.0 {
            __w = __w.position(p.f64("position") as f32);
        }
        if p.f64("duration") != 0.0 {
            __w = __w.duration(p.f64("duration") as f32);
        }
        if let Some(v) = crate::pages::parse_rgba(p.str("art_color")) {
            __w = __w.art_color(v);
        }
        Box::new(__w)
    },
    snippet: |p| {
        let mut __s = format!(
            "NowPlaying::new({:?}, {:?})",
            p.str("title"),
            p.str("artist"),
        );
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("album", ".album", SnipProp::Text("")),
                ("art_text", ".art_text", SnipProp::Text("")),
                ("position", ".position", SnipProp::Float(0.0)),
                ("duration", ".duration", SnipProp::Float(0.0)),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        __s.push_str(&crate::pages::snip_textmap(
            p,
            "art_color",
            ".art_color",
            "",
            crate::pages::expr_rgba,
        ));
        __s
    },
    poll: |w, out| {
        if let Some(np) = downcast_mut::<NowPlaying>(w) {
            if np.take_clicked() {
                out.push("clicked".to_string());
            }
        }
    },
});

page!(PlaylistPage {
    meta: meta(
        "Playlist",
        "Media",
        "Track list with current-track highlight.",
        "List",
        &[
            ("Music", "playlist"),
            ("Qt", "media playlist"),
            ("iTunes", "playlist"),
            ("React", "track list")
        ],
        false,
    ),
    props: &[
        PropSpec::Header {
            label: "State & Accessibility"
        },
        PropSpec::Text {
            key: "a11y_label",
            label: "A11y label",
            default: ""
        },
    ],
    build: |_p| {
        let mut pl = Playlist::new();
        pl = pl.track(Track::new("Night Shift", "Martensite"));
        pl = pl.track(Track::new("Daybreak", "Martensite"));
        {
            let mut __w = pl;
            if !_p.str("a11y_label").is_empty() {
                __w = __w.label(_p.str("a11y_label"));
            }
            Box::new(__w)
        }
    },
    snippet: |_p| {
        let mut __s = "Playlist::new().track(Track::new(\"Night Shift\", \"…\"))".to_string();
        __s.push_str(&crate::pages::prop_snippet(
            _p,
            &[("a11y_label", ".label", SnipProp::Text(""))],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(pl) = downcast_mut::<Playlist>(w) {
            if let Some(m) = pl.take_moved() {
                out.push(format!("moved {m:?}"));
            }
        }
    },
});

page!(FilmstripPage {
    meta: meta(
        "Filmstrip",
        "Media",
        "Thumbnail filmstrip — horizontal photo strip.",
        "List",
        &[
            ("Photos", "filmstrip"),
            ("Qt", "thumbnail strip"),
            ("iMovie", "filmstrip"),
            ("React", "strip")
        ],
        false,
    ),
    props: &[
        PropSpec::Int {
            key: "count",
            label: "Thumbs",
            min: 2,
            max: 12,
            default: 5
        },
        PropSpec::Header {
            label: "State & Accessibility"
        },
        PropSpec::Text {
            key: "a11y_label",
            label: "A11y label",
            default: ""
        },
    ],
    build: |p| {
        let mut fs = Filmstrip::new();
        for i in 0..p.i64("count") as u8 {
            fs = fs.thumb(Thumbnail::new(
                format!("Shot {}", i + 1),
                [90, 100 + i * 20, 200, 255],
            ));
        }
        {
            let mut __w = fs;
            if !p.str("a11y_label").is_empty() {
                __w = __w.label(p.str("a11y_label"));
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = format!("Filmstrip::new() /* {} thumbs */", p.i64("count"));
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[("a11y_label", ".label", SnipProp::Text(""))],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(fs) = downcast_mut::<Filmstrip>(w) {
            if let Some(i) = fs.take_selected() {
                out.push(format!("thumb {i}"));
            }
        }
    },
});

page!(CoverflowPage {
    meta: meta(
        "Coverflow",
        "Media",
        "Cover-flow carousel — centered item + flanks.",
        "List",
        &[
            ("iTunes", "Cover Flow"),
            ("Finder", "cover flow"),
            ("Qt", "custom"),
            ("React", "carousel")
        ],
        false,
    ),
    props: &[
        PropSpec::Int {
            key: "selected",
            label: "Selected",
            min: 0,
            max: 100,
            default: 0
        },
        PropSpec::Header {
            label: "State & Accessibility"
        },
        PropSpec::Text {
            key: "a11y_label",
            label: "A11y label",
            default: ""
        },
    ],
    build: |_p| {
        let mut cf = Coverflow::new();
        cf.show_title = true;
        for (i, name) in ["A", "B", "C", "D", "E"].iter().enumerate() {
            cf = cf.item(Thumbnail::new(
                format!("Album {name}"),
                [70 + i as u8 * 20, 120, 220, 255],
            ));
        }
        {
            let mut __w = cf;
            if !_p.str("a11y_label").is_empty() {
                __w = __w.label(_p.str("a11y_label"));
            }
            if _p.i64("selected") != 0 {
                __w = __w.selected(_p.i64("selected") as usize);
            }
            Box::new(__w)
        }
    },
    snippet: |_p| {
        let mut __s = "Coverflow::new().item(Thumbnail::new(\"Album A\", …))".to_string();
        __s.push_str(&crate::pages::prop_snippet(
            _p,
            &[
                ("selected", ".selected", SnipProp::Int(0)),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(cf) = downcast_mut::<Coverflow>(w) {
            if let Some(i) = cf.take_selected() {
                out.push(format!("cover {i}"));
            }
        }
    },
});

page!(LightboxPage {
    meta: meta(
        "Lightbox",
        "Media",
        "Full-screen image viewer — nav arrows + caption.",
        "Dialog",
        &[
            ("Web", "lightbox"),
            ("Photos", "viewer"),
            ("React", "lightbox"),
            ("Qt", "custom")
        ],
        true,
    ),
    props: &[
        PropSpec::Int {
            key: "index",
            label: "Index",
            min: 0,
            max: 100,
            default: 0
        },
        PropSpec::Header {
            label: "State & Accessibility"
        },
        PropSpec::Text {
            key: "a11y_label",
            label: "A11y label",
            default: ""
        },
    ],
    build: |_p| {
        let mut lb = Lightbox::new();
        lb.show_caption = true;
        lb.show_counter = true;
        for (i, name) in ["One", "Two", "Three"].iter().enumerate() {
            lb = lb.item(Thumbnail::new(
                name.to_string(),
                [100, 110 + i as u8 * 30, 200, 255],
            ));
        }
        lb.set_index(0);
        {
            let mut __w = lb;
            if !_p.str("a11y_label").is_empty() {
                __w = __w.label(_p.str("a11y_label"));
            }
            if _p.i64("index") != 0 {
                __w = __w.index(_p.i64("index") as usize);
            }
            Box::new(__w)
        }
    },
    snippet: |_p| {
        let mut __s = "Lightbox::new().item(Thumbnail::new(\"One\", …))".to_string();
        __s.push_str(&crate::pages::prop_snippet(
            _p,
            &[
                ("index", ".index", SnipProp::Int(0)),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(lb) = downcast_mut::<Lightbox>(w) {
            if let Some(i) = lb.take_navigated() {
                out.push(format!("index {i}"));
            }
            if lb.take_closed() {
                out.push("closed".to_string());
            }
        }
    },
});

page!(CaptionsPage {
    meta: meta(
        "Captions",
        "Media",
        "Timed caption/subtitle cue list.",
        "Text",
        &[
            ("HTML", "<track>"),
            ("AV", "subtitles"),
            ("Qt", "custom"),
            ("React", "captions")
        ],
        false,
    ),
    props: &[
        PropSpec::Header {
            label: "State & Accessibility"
        },
        PropSpec::Text {
            key: "a11y_label",
            label: "A11y label",
            default: ""
        },
    ],
    build: |_p| {
        let mut c = Captions::new()
            .cue("First caption", 0.0, 2.0)
            .cue("Second caption", 2.0, 4.0);
        c.set_position(std::time::Duration::from_secs_f32(1.0));
        {
            let mut __w = c;
            if !_p.str("a11y_label").is_empty() {
                __w = __w.label(_p.str("a11y_label"));
            }
            Box::new(__w)
        }
    },
    snippet: |_p| {
        let mut __s = "Captions::new().cue(\"First caption\", 0.0, 2.0)".to_string();
        __s.push_str(&crate::pages::prop_snippet(
            _p,
            &[("a11y_label", ".label", SnipProp::Text(""))],
        ));
        __s
    },
});

page!(AttachmentPage {
    meta: meta(
        "Attachment",
        "Media",
        "File attachment chip — name, size, remove.",
        "ListItem",
        &[
            ("Mail", "attachment"),
            ("Slack", "file chip"),
            ("Qt", "custom"),
            ("React", "file row")
        ],
        false,
    ),
    props: &[
        PropSpec::Text {
            key: "name",
            label: "Name",
            default: "spec.pdf"
        },
        PropSpec::Text {
            key: "glyph",
            label: "Glyph",
            default: "📄"
        },
        PropSpec::Float {
            key: "uploading",
            label: "Uploading",
            min: -10.0,
            max: 100.0,
            step: 1.0,
            default: 0.0
        },
        PropSpec::Header {
            label: "State & Accessibility"
        },
        PropSpec::Text {
            key: "a11y_label",
            label: "A11y label",
            default: ""
        },
    ],
    build: |p| {
        let mut __w = Attachment::new(p.str("name"), 1_240_000);
        if !p.str("a11y_label").is_empty() {
            __w = __w.label(p.str("a11y_label"));
        }
        if p.str("glyph") != "📄" {
            __w = __w.glyph(p.str("glyph"));
        }
        if p.f64("uploading") != 0.0 {
            __w = __w.uploading(p.f64("uploading") as f32);
        }
        Box::new(__w)
    },
    snippet: |p| {
        let mut __s = format!("Attachment::new({:?}, 1_240_000)", p.str("name"));
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("glyph", ".glyph", SnipProp::Text("📄")),
                ("uploading", ".uploading", SnipProp::Float(0.0)),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        __s
    },
    poll: |w, out| {
        if downcast_mut::<Attachment>(w).is_some_and(|a| a.take_removed()) {
            out.push("removed".to_string());
        }
    },
});

page!(DownloadItemPage {
    meta: meta(
        "DownloadItem",
        "Media",
        "Download progress row — state, rate, action.",
        "ListItem",
        &[
            ("Browser", "download row"),
            ("Qt", "custom"),
            ("React", "download item"),
            ("GTK", "custom")
        ],
        false,
    ),
    props: &[
        PropSpec::Text {
            key: "name",
            label: "Name",
            default: "release.tar.gz"
        },
        PropSpec::Float {
            key: "progress",
            label: "Progress",
            min: 0.0,
            max: 1.0,
            step: 0.05,
            default: 0.0
        },
        PropSpec::Text {
            key: "glyph",
            label: "Glyph",
            default: "⬇"
        },
        PropSpec::Header {
            label: "State & Accessibility"
        },
        PropSpec::Text {
            key: "a11y_label",
            label: "A11y label",
            default: ""
        },
    ],
    build: |p| {
        let mut d = DownloadItem::new(p.str("name"), 48_000_000);
        d.set_progress(0.42);
        d.set_rate(1_200_000.0);
        {
            let mut __w = d;
            if !p.str("a11y_label").is_empty() {
                __w = __w.label(p.str("a11y_label"));
            }
            if p.f64("progress") != 0.0 {
                __w = __w.progress(p.f64("progress") as f32);
            }
            if p.str("glyph") != "⬇" {
                __w = __w.glyph(p.str("glyph"));
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = format!("DownloadItem::new({:?}, 48_000_000)", p.str("name"));
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("progress", ".progress", SnipProp::Float(0.0)),
                ("glyph", ".glyph", SnipProp::Text("⬇")),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(d) = downcast_mut::<DownloadItem>(w) {
            if let Some(a) = d.take_action() {
                out.push(format!("action {a:?}"));
            }
        }
    },
});

page!(WebViewPage {
    meta: meta(
        "WebView",
        "Media",
        "Embedded web surface — navigate, eval, history.",
        "Document",
        &[
            ("Qt", "QWebEngineView"),
            ("macOS", "WKWebView"),
            ("GTK", "WebKitWebView"),
            ("Android", "WebView")
        ],
        false,
    ),
    props: &[
        PropSpec::Text {
            key: "url",
            label: "URL",
            default: "https://example.com",
        },
        PropSpec::Header {
            label: "State & Accessibility"
        },
        PropSpec::Text {
            key: "a11y_label",
            label: "A11y label",
            default: ""
        },
    ],
    build: |p| {
        let mut wv = WebView::new();
        wv.navigate(p.str("url"));
        {
            let mut __w = wv;
            if !p.str("a11y_label").is_empty() {
                __w = __w.label(p.str("a11y_label"));
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = format!("WebView::new() /* navigate {:?} */", p.str("url"));
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[("a11y_label", ".label", SnipProp::Text(""))],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(wv) = downcast_mut::<WebView>(w) {
            for ev in wv.take_events() {
                out.push(format!("web → {ev:?}"));
            }
        }
    },
});

/// All Media pages, in rail order.
pub(super) fn pages() -> Vec<Box<dyn Page>> {
    vec![
        Box::new(TextPage),
        Box::new(MarkdownPage),
        Box::new(CodeViewPage),
        Box::new(DiffViewPage),
        Box::new(MergeViewPage),
        Box::new(HexViewPage),
        Box::new(TerminalPage),
        Box::new(PdfViewPage),
        Box::new(ImagePage),
        Box::new(ImageViewerPage),
        Box::new(MagnifierPage),
        Box::new(MinimapPage),
        Box::new(CropBoxPage),
        Box::new(InkCanvasPage),
        Box::new(MediaViewPage),
        Box::new(MediaControlsPage),
        Box::new(NowPlayingPage),
        Box::new(PlaylistPage),
        Box::new(FilmstripPage),
        Box::new(CoverflowPage),
        Box::new(LightboxPage),
        Box::new(CaptionsPage),
        Box::new(AttachmentPage),
        Box::new(DownloadItemPage),
        Box::new(WebViewPage),
        Box::new(ExternalEnginePage),
    ]
}

page!(ExternalEnginePage {
    meta: meta(
        "ExternalEngine",
        "Media",
        "External GPU engine surface — frames arrive via the engine bridge.",
        "Canvas",
        &[
            ("wgpu", "shared texture"),
            ("Qt", "QQuickRHI"),
            ("GTK", "GLArea"),
            ("React", "canvas bridge")
        ],
        false,
    ),
    props: &[
        PropSpec::Float {
            key: "with_scale_factor",
            label: "With Scale Factor",
            min: 0.0,
            max: 1.0,
            step: 0.05,
            default: 1.0
        },
        PropSpec::Choice {
            key: "with_fit",
            label: "With Fit",
            options: &["Contain", "Cover", "Fill", "Fixed"],
            default: 0
        },
        PropSpec::Text {
            key: "with_label",
            label: "With Label",
            default: ""
        },
        PropSpec::Float {
            key: "with_aspect_ratio",
            label: "With Aspect Ratio",
            min: 0.0,
            max: 1.0,
            step: 0.05,
            default: 0.0
        },
    ],
    build: |_p| {
        let handle = BridgeHandle::new();
        {
            let mut __w = ExternalEngine::new(handle, SurfaceId(1));
            if _p.f64("with_scale_factor") != 1.0 {
                __w = __w.with_scale_factor(_p.f64("with_scale_factor"));
            }
            if _p.choice("with_fit") != 0 {
                __w = __w.with_fit(match _p.choice("with_fit") {
                    0 => martensite::widgets::media::VideoFit::Contain,
                    1 => martensite::widgets::media::VideoFit::Cover,
                    2 => martensite::widgets::media::VideoFit::Fill,
                    3 => martensite::widgets::media::VideoFit::Fixed,
                    _ => martensite::widgets::media::VideoFit::Contain,
                });
            }
            if !_p.str("with_label").is_empty() {
                __w = __w.with_label(_p.str("with_label"));
            }
            if _p.f64("with_aspect_ratio") != 0.0 {
                __w = __w.with_aspect_ratio(_p.f64("with_aspect_ratio") as f32);
            }
            Box::new(__w)
        }
    },
    snippet: |_p| {
        let mut __s = {
            "let handle = BridgeHandle::new();\nExternalEngine::new(handle, SurfaceId(1))"
                .to_string()
        };
        __s.push_str(&crate::pages::prop_snippet(
            _p,
            &[
                (
                    "with_scale_factor",
                    ".with_scale_factor",
                    SnipProp::Float(1.0),
                ),
                (
                    "with_fit",
                    ".with_fit",
                    SnipProp::Choice(&[
                        "martensite::widgets::media::VideoFit::Contain",
                        "martensite::widgets::media::VideoFit::Cover",
                        "martensite::widgets::media::VideoFit::Fill",
                        "martensite::widgets::media::VideoFit::Fixed",
                    ]),
                ),
                ("with_label", ".with_label", SnipProp::Text("")),
                (
                    "with_aspect_ratio",
                    ".with_aspect_ratio",
                    SnipProp::Float(0.0),
                ),
            ],
        ));
        __s
    },
});
