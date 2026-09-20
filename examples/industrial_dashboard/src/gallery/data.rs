//! Data & documents category — one live card per data/document
//! widget module.
//!
//! Each entry adapts the widget's own doctest construction (the
//! verified canonical shape); demo data is industrial-flavoured where
//! it does not distort that shape.

use martensite::core::{ImageData, Widget};
use martensite::media::surface::{VideoPixelFormat, VideoSurface};
use martensite::prelude::BridgeHandle;
use martensite::widgets::attachment::Attachment;
use martensite::widgets::button::Button;
use martensite::widgets::card::Card;
use martensite::widgets::carousel::Carousel;
use martensite::widgets::clipboard_history::ClipboardHistory;
use martensite::widgets::code_view::CodeView;
use martensite::widgets::coverflow::Coverflow;
use martensite::widgets::descriptions::Descriptions;
use martensite::widgets::diff_view::{DiffKind, DiffView};
use martensite::widgets::download_item::DownloadItem;
use martensite::widgets::external::ExternalEngine;
use martensite::widgets::filmstrip::Filmstrip;
use martensite::widgets::hex_view::HexView;
use martensite::widgets::image::Image;
use martensite::widgets::image_viewer::ImageViewer;
use martensite::widgets::inspector::Inspector;
use martensite::widgets::json_view::{JsonNode, JsonView};
use martensite::widgets::kanban::Kanban;
use martensite::widgets::lightbox::Lightbox;
use martensite::widgets::list_view::ListView;
use martensite::widgets::log_view::{LogSeverity, LogView};
use martensite::widgets::markdown::Markdown;
use martensite::widgets::media::{MediaView, VideoFit};
use martensite::widgets::merge_view::{MergeRow, MergeView};
use martensite::widgets::message_list::{Message, MessageList};
use martensite::widgets::pdf_view::PdfView;
use martensite::widgets::perf_overlay::PerfOverlay;
use martensite::widgets::pip::Pip;
use martensite::widgets::playlist::{Playlist, Track};
use martensite::widgets::property_grid::{PropertyGrid, PropertyRow};
use martensite::widgets::table::{Table, TableColumn};
use martensite::widgets::terminal::Terminal;
use martensite::widgets::text::Text;
use martensite::widgets::tree_view::{TreeNode, TreeView};
use martensite::widgets::video_grid::{Participant, VideoGrid};
use martensite::widgets::webview::WebView;
use martensite::widgets::week_view::{WeekEvent, WeekView};
use martensite::widgets::Thumbnail;

/// A small stand-in raster for the image widgets — a blue-grey
/// gradient, industrial-camera-placeholder style, where the doctests
/// used flat fills.
fn demo_image(w: u32, h: u32) -> ImageData {
    let px: Vec<u8> = (0..w * h)
        .flat_map(|i| {
            let x = i % w;
            let y = i / w;
            [
                (40 + x * 6).min(200) as u8,
                (70 + y * 4).min(220) as u8,
                140,
                255,
            ]
        })
        .collect();
    ImageData::from_rgba(w, h, px).expect("pixel count matches dimensions")
}

/// Data & documents showcase entries — `(display name, live widget)`.
pub fn entries() -> Vec<(&'static str, Box<dyn Widget>)> {
    vec![
        (
            "Card",
            Box::new(
                Card::outlined()
                    .title("CNC-Mill-02")
                    .child(Text::new("Spindle 12 000 rpm — nominal"))
                    .action(Button::new("Acknowledge")),
            ),
        ),
        (
            "Carousel",
            Box::new(
                Carousel::new()
                    .page(Text::new("Shift A — OEE 87%"))
                    .page(Text::new("Shift B — OEE 82%"))
                    .page(Text::new("Shift C — OEE 91%"))
                    .wrap(true),
            ),
        ),
        (
            "Cover Flow",
            Box::new(
                Coverflow::new()
                    .item(Thumbnail::new("Cam 01", [90, 140, 200, 255]))
                    .item(Thumbnail::new("Cam 02", [200, 140, 90, 255]))
                    .item(Thumbnail::new("Cam 03", [120, 180, 120, 255]))
                    .item(Thumbnail::new("Cam 04", [180, 90, 140, 255])),
            ),
        ),
        (
            "Descriptions",
            Box::new(
                Descriptions::new()
                    .title("CNC-Mill-02")
                    .item("Model", "MX-2000")
                    .item("Firmware", "1.4.2")
                    .item("Cell", "Line 3"),
            ),
        ),
        (
            "Diff View",
            Box::new(
                DiffView::new()
                    .line(DiffKind::Hunk, "@@ -12,4 +12,5 @@ recipe.yaml")
                    .line(DiffKind::Context, "  feed_rate: 1200")
                    .line(DiffKind::Removed, "- spindle_rpm: 9500")
                    .line(DiffKind::Added, "+ spindle_rpm: 12000")
                    .line(DiffKind::Added, "+ coolant: mist"),
            ),
        ),
        (
            "Download Item",
            Box::new(DownloadItem::new("firmware-1.4.2.pkg", 48_300_000).progress(0.4)),
        ),
        (
            "Filmstrip",
            Box::new(
                Filmstrip::new()
                    .thumb(Thumbnail::new("IMG_0441", [80, 120, 200, 255]))
                    .thumb(Thumbnail::new("IMG_0442", [200, 120, 80, 255]))
                    .thumb(Thumbnail::new("IMG_0443", [90, 170, 110, 255])),
            ),
        ),
        (
            "Hex View",
            Box::new(HexView::new().bytes(
                b"MES\x01\x02\x03\x04\xde\xad\xbe\xef\x00\x10\x20\x30\x40\x50\x60\x70".to_vec(),
            )),
        ),
        (
            "Image",
            Box::new(Image::new(demo_image(24, 16)).alt("Weld-scan preview")),
        ),
        (
            "Image Viewer",
            Box::new(ImageViewer::new(demo_image(32, 24)).label("Weld scan")),
        ),
        (
            "JSON View",
            Box::new(JsonView::new(JsonNode::object(
                "cell",
                [
                    ("asset".into(), JsonNode::string("", "CNC-Mill-02")),
                    ("oee".into(), JsonNode::number("", 0.87)),
                    ("running".into(), JsonNode::boolean("", true)),
                    (
                        "tags".into(),
                        JsonNode::array(
                            "",
                            [
                                JsonNode::string("", "milling"),
                                JsonNode::string("", "line-3"),
                            ],
                        ),
                    ),
                ],
            ))),
        ),
        (
            "Lightbox",
            Box::new(
                Lightbox::new()
                    .item(Thumbnail::new("Weld A", [200, 90, 60, 255]))
                    .item(Thumbnail::new("Weld B", [90, 140, 200, 255])),
            ),
        ),
        (
            "List View",
            Box::new(ListView::new().items([
                "WO-4471 — housing, 250 pcs",
                "WO-4472 — bracket, 120 pcs",
                "WO-4473 — shaft, 80 pcs",
                "WO-4474 — flange, 300 pcs",
            ])),
        ),
        (
            "Log View",
            Box::new({
                let mut log = LogView::new();
                log.push(LogSeverity::Info, "cycle start — WO-4471");
                log.push(LogSeverity::Debug, "spindle ramp 12 000 rpm");
                log.push(LogSeverity::Warning, "coolant pressure low");
                log.push(LogSeverity::Error, "estop channel B fault");
                log
            }),
        ),
        (
            "Markdown",
            Box::new(Markdown::new(
                "# Line 3 — shift handover\n\n- OEE **87%** vs target 85%\n- 2 minor alarms cleared\n- `recipe_v12.yaml` loaded\n\n> Conveyor guard replaced at 14:20.",
            )),
        ),
        (
            "Media View",
            Box::new(
                MediaView::new()
                    .with_surface(VideoSurface::new_mock(1920, 1080, VideoPixelFormat::Nv12))
                    .with_fit(VideoFit::Contain),
            ),
        ),
        (
            "Merge View",
            Box::new(
                MergeView::new()
                    .row(MergeRow::aligned(
                        "feed_rate: 1200",
                        "feed_rate: 1200",
                        "feed_rate: 1200",
                    ))
                    .row(MergeRow::conflict(
                        "spindle_rpm: 9500",
                        "",
                        "spindle_rpm: 12000",
                    ))
                    .row(MergeRow::conflict("coolant: flood", "", "coolant: mist")),
            ),
        ),
        ("PDF View", Box::new(PdfView::new())),
        (
            "Picture in Picture",
            Box::new(Pip::new(Text::new("CAM-04 — live"))),
        ),
        (
            "Property Grid",
            Box::new(
                PropertyGrid::new()
                    .section(
                        "Servo Drive",
                        [
                            PropertyRow::text("Position", "142.505 mm"),
                            PropertyRow::bool("Enabled", true),
                        ],
                    )
                    .row(PropertyRow::choice("Mode", ["Auto", "Manual", "Jog"])),
            ),
        ),
        (
            "Table",
            Box::new(
                Table::new()
                    .columns([
                        TableColumn::new("wo", "Work order").width(90.0),
                        TableColumn::new("cell", "Cell").width(110.0),
                        TableColumn::new("qty", "Qty").width(50.0),
                    ])
                    .row(["WO-4471", "CNC-Mill-02", "250"])
                    .row(["WO-4472", "Lathe-07", "120"])
                    .row(["WO-4473", "Robot-Arm-K7", "80"]),
            ),
        ),
        (
            "Terminal",
            Box::new({
                let mut t = Terminal::new().prompt("$");
                t.write("martensite shell — plant-east");
                t.submit("oee --line 3");
                t
            }),
        ),
        (
            "Tree View",
            Box::new(TreeView::new().roots(vec![
                TreeNode::new("Plant East").with_children(vec![
                    TreeNode::new("Line 1").with_children(vec![TreeNode::new("CNC-Mill-02")]),
                    TreeNode::new("Line 2"),
                ]),
                TreeNode::new("Plant West"),
            ])),
        ),
        (
            "Code View",
            Box::new(
                CodeView::new()
                    .lines([
                        "fn cycle_start(cell: &Cell) {",
                        "    cell.load_recipe(\"v12\");",
                        "    cell.spindle(12_000);",
                        "}",
                    ])
                    .current(Some(1)),
            ),
        ),
        (
            "Clipboard History",
            Box::new({
                let mut h = ClipboardHistory::new();
                h.push("WO-4471");
                h.push("recipe_v12.yaml");
                h.push("192.168.10.15");
                h
            }),
        ),
        (
            "External Engine",
            Box::new({
                let handle = BridgeHandle::new();
                let surface = handle.lock().register();
                ExternalEngine::new(handle, surface).with_label("SCADA viewport")
            }),
        ),
        (
            "Inspector",
            Box::new(
                Inspector::new()
                    .section("Servo")
                    .row("Position", "142.5 mm")
                    .row("Velocity", "1.2 m/s")
                    .section("Limits")
                    .row("Torque", "85 %"),
            ),
        ),
        (
            "Kanban",
            Box::new(
                Kanban::new()
                    .column("Queued")
                    .card("Queued", "WO-4471 housing")
                    .column("In progress")
                    .card("In progress", "WO-4470 bracket")
                    .column("Done")
                    .card("Done", "WO-4469 shaft"),
            ),
        ),
        (
            "Message List",
            Box::new({
                let mut l = MessageList::new();
                l.push(Message::received("SCADA", "Alarm ACK on cell K7").time("14:02"));
                l.push(Message::sent("Maintenance dispatched").time("14:04"));
                l
            }),
        ),
        (
            "Web View",
            Box::new({
                let mut wv = WebView::new();
                wv.navigate("https://mes.plant-east.local/oee");
                wv
            }),
        ),
        (
            "Perf Overlay",
            Box::new({
                let mut p = PerfOverlay::new();
                for ms in [16.6, 16.7, 16.8, 17.1, 16.5, 16.9, 16.7] {
                    p.push_frame(ms);
                }
                p
            }),
        ),
        (
            "Playlist",
            Box::new({
                let mut p = Playlist::new()
                    .track(Track::new("Cycle start", "Cell A").duration(214))
                    .track(Track::new("Tool change", "Cell B").duration(187))
                    .track(Track::new("Shift bell", "PA system").duration(12));
                p.set_current(0);
                p
            }),
        ),
        (
            "Video Grid",
            Box::new(
                VideoGrid::new()
                    .participant(Participant::new("Control", [90, 140, 200, 255]).speaking(true))
                    .participant(Participant::new("Cell K7", [200, 140, 90, 255]))
                    .participant(Participant::new("QA Lab", [120, 180, 120, 255]).muted(true)),
            ),
        ),
        (
            "Week View",
            Box::new(
                WeekView::new()
                    .event(WeekEvent::new("PM — Line 3", 1, 8.0, 10.0))
                    .event(WeekEvent::new("Tool change", 2, 13.0, 14.5))
                    .event(WeekEvent::all_day("Shutdown prep", 5)),
            ),
        ),
        (
            "Attachment",
            Box::new(Attachment::new("shift-report.pdf", 204_800).uploading(0.65)),
        ),
    ]
}
