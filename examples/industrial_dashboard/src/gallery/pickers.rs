//! Pickers & canvas category — color/alpha picking surfaces, code
//! renderers, and direct-manipulation canvas idioms. Every entry is
//! adapted from the widget's own doctest construction.

use martensite::core::Widget;
use martensite::widgets::{
    AlphaSlider, Barcode, Color, ColorButton, ColorPalette, ColorPicker, ColorWheel, CropBox,
    Crosshair, CurveEditor, GradientEditor, GradientStop, HueSlider, InkCanvas, Joystick,
    Magnifier, Marquee, Minimap, PageFlip, QrCode, RubberBand, Ruler, SwipeAction, SwipeActions,
    Text, VirtualKeyboard, XYPad, ZoomControls,
};

/// A plausible 21×21 demo matrix — finder squares in the corners over
/// a deterministic pseudo-random fill. Real matrices come from an
/// encoder; the widget only needs the module grid.
fn qr_demo_matrix() -> Vec<Vec<bool>> {
    const N: usize = 21;
    let mut m = vec![vec![false; N]; N];
    for (r, row) in m.iter_mut().enumerate() {
        for (c, cell) in row.iter_mut().enumerate() {
            *cell = (r * 31 + c * 17 + r * c) % 5 < 2;
        }
    }
    let finder = |m: &mut [Vec<bool>], y: usize, x: usize| {
        for r in 0..7 {
            for c in 0..7 {
                let edge = r == 0 || r == 6 || c == 0 || c == 6;
                let core = (2..=4).contains(&r) && (2..=4).contains(&c);
                m[y + r][x + c] = edge || core;
            }
        }
    };
    finder(&mut m, 0, 0);
    finder(&mut m, 0, N - 7);
    finder(&mut m, N - 7, 0);
    m
}

/// Pickers & canvas showcase entries — `(display name, live widget)`.
pub fn entries() -> Vec<(&'static str, Box<dyn Widget>)> {
    vec![
        ("Alpha Slider", Box::new(AlphaSlider::new().alpha(0.85))),
        ("Barcode", Box::new(Barcode::new().text("SN-4402-A"))),
        (
            "Color Button",
            Box::new(ColorButton::new([255, 140, 0, 255]).title("Alarm")),
        ),
        (
            "Color Palette",
            Box::new(ColorPalette::new().swatches([
                [34, 197, 94, 255],   // running green
                [234, 179, 8, 255],   // warning amber
                [239, 68, 68, 255],   // alarm red
                [59, 130, 246, 255],  // info blue
                [148, 163, 184, 255], // muted steel
            ])),
        ),
        (
            "Color Picker",
            Box::new(
                ColorPicker::new()
                    .color(Color::rgb(60, 110, 220))
                    .with_alpha(true),
            ),
        ),
        ("Color Wheel", Box::new(ColorWheel::new().hue(120.0))),
        (
            "Crop Box",
            Box::new(CropBox::new().crop(0.1, 0.1, 0.8, 0.8)),
        ),
        ("Crosshair", {
            let mut c = Crosshair::new();
            c.set_position(glam::Vec2::new(0.5, 0.5));
            Box::new(c)
        }),
        (
            "Curve Editor",
            Box::new(CurveEditor::new().handles((0.25, 0.1), (0.25, 1.0))),
        ),
        (
            "Gradient Editor",
            Box::new(GradientEditor::new().stops(vec![
                GradientStop::new(0.0, [30, 60, 114, 255]),
                GradientStop::new(0.5, [234, 179, 8, 255]),
                GradientStop::new(1.0, [239, 68, 68, 255]),
            ])),
        ),
        ("Hue Slider", Box::new(HueSlider::new().hue(200.0))),
        ("Ink Canvas", Box::new(InkCanvas::new())),
        (
            "Joystick",
            Box::new(Joystick::new().dead_zone(0.1).spring(true)),
        ),
        ("Magnifier", Box::new(Magnifier::new().zoom(8.0))),
        (
            "Marquee",
            Box::new(Marquee::new("LINE 4 — SCHEDULED MAINTENANCE AT 14:00").speed(60.0)),
        ),
        (
            "Minimap",
            Box::new(
                Minimap::new()
                    .lines((0..64u32).map(|i| 16 + (i * 37) % 60))
                    .scroll(0.15)
                    .viewport(0.25),
            ),
        ),
        (
            "Page Flip",
            Box::new(PageFlip::new().pages([
                "Shift A report",
                "Shift B report",
                "Shift C report",
                "Maintenance log",
            ])),
        ),
        ("QR Code", Box::new(QrCode::from_matrix(qr_demo_matrix()))),
        ("Rubber Band", Box::new(RubberBand::new())),
        (
            "Ruler",
            Box::new(Ruler::new().range(0.0, 300.0).position(120.0)),
        ),
        ("Swipe Actions", {
            let s = SwipeActions::new(Text::new("Pump P-114 alarm"))
                .leading([SwipeAction::new("Ack")])
                .trailing([SwipeAction::new("Delete").destructive()]);
            Box::new(s)
        }),
        ("Virtual Keyboard", Box::new(VirtualKeyboard::new())),
        (
            "XY Pad",
            Box::new(XYPad::new().value(0.5, 0.25).labels("Pan", "Tilt")),
        ),
        ("Zoom Controls", Box::new(ZoomControls::new().zoom(1.0))),
    ]
}
