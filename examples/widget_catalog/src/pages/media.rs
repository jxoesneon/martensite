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
use crate::pages::{downcast_mut, meta, page};

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
    ],
    build: |p| {
        let mut t = Text::new(p.str("content")).font_size(p.f64("size") as f32);
        if p.bool("rtl") {
            t = t.rtl();
        }
        Box::new(t)
    },
    snippet: |p| format!(
        "Text::new({:?}).font_size({:?}).rtl({})",
        p.str("content"),
        p.f64("size") as f32,
        p.bool("rtl"),
    ),
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
    props: &[PropSpec::Float {
        key: "size",
        label: "Base size",
        min: 10.0,
        max: 24.0,
        step: 1.0,
        default: 14.0,
    }],
    build: |p| Box::new(
        Markdown::new("# Heading\n\nBody text with **bold** and *italic*.\n\n- Item A\n- Item B")
            .base_size(p.f64("size") as f32),
    ),
    snippet: |p| format!(
        "Markdown::new(source).base_size({:?})",
        p.f64("size") as f32
    ),
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
    props: &[],
    build: |_p| Box::new(CodeView::new().lines([
        "fn main() {",
        "    let app = App::new();",
        "    app.run();",
        "}",
    ]),),
    snippet: |_p| "CodeView::new().lines([\"fn main() {\", \"}\"])".to_string(),
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
    props: &[],
    build: |_p| Box::new(DiffView::new().lines(vec![
        (DiffKind::Hunk, "@@ -1,4 +1,4 @@".into()),
        (DiffKind::Context, " fn main() {".into()),
        (DiffKind::Removed, "-    old_call()".into()),
        (DiffKind::Added, "+    new_call()".into()),
        (DiffKind::Context, " }".into()),
    ])),
    snippet: |_p| {
        "DiffView::new().lines(vec![(DiffKind::Added, \"+ line\".into())])".to_string()
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
    props: &[],
    build: |_p| Box::new(MergeView::new()),
    snippet: |_p| "MergeView::new()".to_string(),
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
    props: &[PropSpec::Int {
        key: "len",
        label: "Bytes",
        min: 16,
        max: 512,
        default: 96
    }],
    build: |p| Box::new(HexView::new().bytes((0..p.i64("len") as u8).collect::<Vec<u8>>()),),
    snippet: |p| format!("HexView::new().bytes(vec![0u8; {}])", p.i64("len")),
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
    props: &[],
    build: |_p| {
        let mut t = Terminal::new().lines(["$ cargo build", "   Compiling martensite…"]);
        t.submit_line();
        Box::new(t)
    },
    snippet: |_p| "Terminal::new().lines([\"$ cargo build\"])".to_string(),
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
    props: &[],
    build: |_p| Box::new(PdfView::new()),
    snippet: |_p| "PdfView::new().with_document(doc)".to_string(),
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
    props: &[],
    build: |_p| Box::new(Image::new(demo_image()).alt("Checkerboard")),
    snippet: |_p| "Image::new(image_data).alt(\"Checkerboard\")".to_string(),
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
    props: &[PropSpec::Bool {
        key: "checker",
        label: "Checkerboard",
        default: true
    }],
    build: |p| Box::new(ImageViewer::new(demo_image()).checker(p.bool("checker"))),
    snippet: |p| format!("ImageViewer::new(img).checker({})", p.bool("checker")),
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
    props: &[PropSpec::Float {
        key: "zoom",
        label: "Zoom",
        min: 1.0,
        max: 16.0,
        step: 1.0,
        default: 4.0,
    }],
    build: |p| {
        let mut m = Magnifier::new()
            .source(demo_image())
            .zoom(p.f64("zoom") as f32);
        m.set_focus(Vec2::new(32.0, 32.0));
        Box::new(m)
    },
    snippet: |p| format!(
        "Magnifier::new().source(img).zoom({:?})",
        p.f64("zoom") as f32
    ),
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
    props: &[],
    build: |_p| Box::new(Minimap::new().lines((3..20).step_by(2))),
    snippet: |_p| "Minimap::new().lines(line_lengths)".to_string(),
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
    props: &[PropSpec::Float {
        key: "aspect",
        label: "Aspect",
        min: 0.0,
        max: 3.0,
        step: 0.25,
        default: 0.0,
    }],
    build: |p| {
        let mut c = CropBox::new().aspect_ratio(p.f64("aspect") as f32);
        c.set_crop(0.2, 0.2, 0.6, 0.6);
        Box::new(c)
    },
    snippet: |p| format!("CropBox::new().aspect_ratio({:?})", p.f64("aspect") as f32),
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
    props: &[],
    build: |_p| Box::new(InkCanvas::new()),
    snippet: |_p| "InkCanvas::new()".to_string(),
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
    props: &[],
    build: |_p| Box::new(MediaView::new()),
    snippet: |_p| "MediaView::new().with_decoder(decoder)".to_string(),
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
    props: &[PropSpec::Float {
        key: "position",
        label: "Position",
        min: 0.0,
        max: 1.0,
        step: 0.05,
        default: 0.35,
    }],
    build: |p| {
        let mut mc = MediaControls::new();
        mc.set_duration(120.0);
        mc.set_position(p.f64("position") * 120.0);
        Box::new(mc)
    },
    snippet: |p| format!(
        "let mut mc = MediaControls::new();\nmc.set_position({:?});",
        p.f64("position") as f32 * 120.0,
    ),
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
    ],
    build: |p| Box::new(NowPlaying::new(p.str("title"), p.str("artist"))),
    snippet: |p| format!(
        "NowPlaying::new({:?}, {:?})",
        p.str("title"),
        p.str("artist"),
    ),
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
    props: &[],
    build: |_p| {
        let mut pl = Playlist::new();
        pl = pl.track(Track::new("Night Shift", "Martensite"));
        pl = pl.track(Track::new("Daybreak", "Martensite"));
        Box::new(pl)
    },
    snippet: |_p| "Playlist::new().track(Track::new(\"Night Shift\", \"…\"))".to_string(),
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
    props: &[PropSpec::Int {
        key: "count",
        label: "Thumbs",
        min: 2,
        max: 12,
        default: 5
    }],
    build: |p| {
        let mut fs = Filmstrip::new();
        for i in 0..p.i64("count") as u8 {
            fs = fs.thumb(Thumbnail::new(
                format!("Shot {}", i + 1),
                [90, 100 + i * 20, 200, 255],
            ));
        }
        Box::new(fs)
    },
    snippet: |p| format!("Filmstrip::new() /* {} thumbs */", p.i64("count")),
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
    props: &[],
    build: |_p| {
        let mut cf = Coverflow::new();
        cf.show_title = true;
        for (i, name) in ["A", "B", "C", "D", "E"].iter().enumerate() {
            cf = cf.item(Thumbnail::new(
                format!("Album {name}"),
                [70 + i as u8 * 20, 120, 220, 255],
            ));
        }
        Box::new(cf)
    },
    snippet: |_p| "Coverflow::new().item(Thumbnail::new(\"Album A\", …))".to_string(),
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
    props: &[],
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
        Box::new(lb)
    },
    snippet: |_p| "Lightbox::new().item(Thumbnail::new(\"One\", …))".to_string(),
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
    props: &[],
    build: |_p| {
        let mut c = Captions::new()
            .cue("First caption", 0.0, 2.0)
            .cue("Second caption", 2.0, 4.0);
        c.set_position(std::time::Duration::from_secs_f32(1.0));
        Box::new(c)
    },
    snippet: |_p| "Captions::new().cue(\"First caption\", 0.0, 2.0)".to_string(),
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
    props: &[PropSpec::Text {
        key: "name",
        label: "Name",
        default: "spec.pdf"
    }],
    build: |p| Box::new(Attachment::new(p.str("name"), 1_240_000)),
    snippet: |p| format!("Attachment::new({:?}, 1_240_000)", p.str("name")),
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
    props: &[PropSpec::Text {
        key: "name",
        label: "Name",
        default: "release.tar.gz"
    }],
    build: |p| {
        let mut d = DownloadItem::new(p.str("name"), 48_000_000);
        d.set_progress(0.42);
        d.set_rate(1_200_000.0);
        Box::new(d)
    },
    snippet: |p| format!("DownloadItem::new({:?}, 48_000_000)", p.str("name")),
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
    props: &[PropSpec::Text {
        key: "url",
        label: "URL",
        default: "https://example.com",
    }],
    build: |p| {
        let mut wv = WebView::new();
        wv.navigate(p.str("url"));
        Box::new(wv)
    },
    snippet: |p| format!("WebView::new() /* navigate {:?} */", p.str("url")),
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
    props: &[],
    build: |_p| {
        let handle = BridgeHandle::new();
        Box::new(ExternalEngine::new(handle, SurfaceId(1)))
    },
    snippet: |_p| {
        "let handle = BridgeHandle::new();\nExternalEngine::new(handle, SurfaceId(1))".to_string()
    },
});
