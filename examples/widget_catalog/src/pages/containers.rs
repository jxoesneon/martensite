//! Containers family — layout and grouping widgets.

use martensite::widgets::aspect_frame::AspectFrame;
use martensite::widgets::button::Button;
use martensite::widgets::card::{Card, CardVariant};
use martensite::widgets::clamp::Clamp;
use martensite::widgets::container::Container;
use martensite::widgets::descriptions::{DescriptionItem, Descriptions};
use martensite::widgets::disclosure::Disclosure;
use martensite::widgets::expander_row::ExpanderRow;
use martensite::widgets::flex::{Flex, FlexDirection};
use martensite::widgets::flow_box::FlowBox;
use martensite::widgets::grid::{Grid, GridCell};
use martensite::widgets::group_box::GroupBox;
use martensite::widgets::masonry::Masonry;
use martensite::widgets::scrollview::ScrollView;
use martensite::widgets::separator::Separator;
use martensite::widgets::settings_row::{SettingsGroup, SettingsRow};
use martensite::widgets::split_view::SplitView;
use martensite::widgets::stack::Stack;
use martensite::widgets::switch::Switch;
use martensite::widgets::text::Text;
use martensite::widgets::viewport::Viewport;

use crate::page::{Page, PropSpec};
use crate::pages::{downcast_mut, meta, page};

page!(ContainerPage {
    meta: meta(
        "Container",
        "Containers",
        "Padded, optionally-tinted box around a single child.",
        "Group",
        &[
            ("Flutter", "Container"),
            ("Qt", "QWidget+layout"),
            ("HTML", "<div>"),
            ("SwiftUI", "padding+background")
        ],
        false,
    ),
    props: &[PropSpec::Float {
        key: "padding",
        label: "Padding",
        min: 0.0,
        max: 64.0,
        step: 2.0,
        default: 16.0
    },],
    build: |p| Box::new(
        Container::new()
            .padding_uniform(p.f64("padding") as f32)
            .child(Text::new("Contained content")),
    ),
    snippet: |p| {
        format!(
        "Container::new()\n    .padding_uniform({:?})\n    .child(Text::new(\"Contained content\"))",
        p.f64("padding") as f32,
    )
    },
});

page!(FlexPage {
    meta: meta(
        "Flex",
        "Containers",
        "Row/column flex layout with gap and alignment.",
        "Group",
        &[
            ("CSS", "flexbox"),
            ("Qt", "QHBoxLayout"),
            ("GTK", "GtkBox"),
            ("SwiftUI", "HStack/VStack")
        ],
        false,
    ),
    props: &[
        PropSpec::Choice {
            key: "direction",
            label: "Direction",
            options: &["Row", "Column"],
            default: 0,
        },
        PropSpec::Float {
            key: "gap",
            label: "Gap",
            min: 0.0,
            max: 32.0,
            step: 2.0,
            default: 8.0
        },
    ],
    build: |p| {
        let dir = if p.choice("direction") == 1 {
            FlexDirection::Column
        } else {
            FlexDirection::Row
        };
        Box::new(
            Flex::new(dir)
                .gap(p.f64("gap") as f32)
                .child(Button::new("One"))
                .child(Button::new("Two"))
                .child(Button::new("Three")),
        )
    },
    snippet: |p| {
        format!(
        "Flex::new(FlexDirection::{})\n    .gap({:?})\n    .child(Button::new(\"One\"))\n    .child(Button::new(\"Two\"))",
        if p.choice("direction") == 1 { "Column" } else { "Row" },
        p.f64("gap") as f32,
    )
    },
});

page!(StackPage {
    meta: meta(
        "Stack",
        "Containers",
        "Z-stacked children — overlays within one cell.",
        "Group",
        &[
            ("SwiftUI", "ZStack"),
            ("Qt", "QStackedLayout"),
            ("GTK", "GtkStack"),
            ("CSS", "absolute layers")
        ],
        false,
    ),
    props: &[],
    build: |_p| {
        let mut s = Stack::new().child(
            Container::new()
                .padding_uniform(8.0)
                .child(Text::new("Base layer")),
        );
        s = s.child(Text::new("Overlay layer"));
        Box::new(s)
    },
    snippet: |_p| {
        "Stack::new()\n    .child(base_widget)\n    .child(badge_overlay)".to_string()
    },
});

page!(GridPage {
    meta: meta(
        "Grid",
        "Containers",
        "Column/row-span grid container.",
        "Grid",
        &[
            ("CSS", "grid"),
            ("Qt", "QGridLayout"),
            ("GTK", "GtkGrid"),
            ("SwiftUI", "Grid")
        ],
        false,
    ),
    props: &[PropSpec::Int {
        key: "cols",
        label: "Columns",
        min: 1,
        max: 6,
        default: 3
    },],
    build: |p| {
        let mut g = Grid::new().columns(p.i64("cols") as u32).row_height(32.0);
        for i in 1..=6 {
            g = g.cell(GridCell::new(Button::new(format!("Cell {i}"))));
        }
        Box::new(g)
    },
    snippet: |p| {
        format!(
        "Grid::new()\n    .columns({})\n    .row_height(32.0)\n    .cell(GridCell::new(Button::new(\"Cell\")))",
        p.i64("cols"),
    )
    },
});

page!(GroupBoxPage {
    meta: meta(
        "GroupBox",
        "Containers",
        "Titled frame around grouped content — optionally checkable.",
        "Group",
        &[
            ("Qt", "QGroupBox"),
            ("GTK", "GtkFrame"),
            ("Win32", "group box"),
            ("SwiftUI", "GroupBox")
        ],
        false,
    ),
    props: &[
        PropSpec::Text {
            key: "title",
            label: "Title",
            default: "Options"
        },
        PropSpec::Bool {
            key: "checkable",
            label: "Checkable",
            default: false
        },
    ],
    build: |p| Box::new(
        GroupBox::new(p.str("title"))
            .checkable(p.bool("checkable"))
            .child(Text::new("Grouped content")),
    ),
    snippet: |p| format!(
        "GroupBox::new({:?})\n    .checkable({})\n    .child(Text::new(\"Grouped content\"))",
        p.str("title"),
        p.bool("checkable"),
    ),
    state: |w| {
        downcast_mut::<GroupBox>(w)
            .map(|g| vec![("checked".to_string(), g.is_checked().to_string())])
            .unwrap_or_default()
    },
});

page!(CardPage {
    meta: meta(
        "Card",
        "Containers",
        "Elevated/filled/outlined surface with title, content, actions.",
        "Group",
        &[
            ("Material", "Card"),
            ("Qt", "card frame"),
            ("React", "<Card>"),
            ("SwiftUI", "groupBox")
        ],
        false,
    ),
    props: &[
        PropSpec::Text {
            key: "title",
            label: "Title",
            default: "Summary"
        },
        PropSpec::Choice {
            key: "variant",
            label: "Variant",
            options: &["Elevated", "Filled", "Outlined"],
            default: 0,
        },
    ],
    build: |p| {
        let variant = match p.choice("variant") {
            1 => CardVariant::Filled,
            2 => CardVariant::Outlined,
            _ => CardVariant::Elevated,
        };
        Box::new(
            Card::new()
                .title(p.str("title"))
                .variant(variant)
                .child(Text::new("Card body content goes here.")),
        )
    },
    snippet: |p| {
        format!(
        "Card::new()\n    .title({:?})\n    .variant(CardVariant::{})\n    .child(Text::new(\"Card body\"))",
        p.str("title"),
        ["Elevated", "Filled", "Outlined"][p.choice("variant")],
    )
    },
});

page!(ExpanderRowPage {
    meta: meta(
        "ExpanderRow",
        "Containers",
        "Collapsible preferences row with subtitle and children.",
        "Group",
        &[
            ("GTK", "ExpanderRow"),
            ("GNOME", "preferences expander"),
            ("Qt", "collapsible group"),
            ("React", "disclosure row")
        ],
        false,
    ),
    props: &[
        PropSpec::Text {
            key: "title",
            label: "Title",
            default: "Advanced"
        },
        PropSpec::Text {
            key: "subtitle",
            label: "Subtitle",
            default: "Fine-tune behavior"
        },
    ],
    build: |p| Box::new(
        ExpanderRow::new(p.str("title"))
            .subtitle(p.str("subtitle"))
            .child(Switch::new("Deep option").on(true)),
    ),
    snippet: |p| format!(
        "ExpanderRow::new({:?})\n    .subtitle({:?})\n    .child(Switch::new(\"Deep option\"))",
        p.str("title"),
        p.str("subtitle"),
    ),
    poll: |w, out| {
        if let Some(e) = downcast_mut::<ExpanderRow>(w) {
            if e.take_toggled() {
                out.push(format!("expanded → {}", e.is_expanded()));
            }
        }
    },
});

page!(SettingsRowPage {
    meta: meta(
        "SettingsRow",
        "Containers",
        "Preferences row — title, subtitle, trailing control.",
        "ListItem",
        &[
            ("GNOME", "AdwActionRow"),
            ("iOS", "settings cell"),
            ("Qt", "settings row"),
            ("React", "settings item")
        ],
        false,
    ),
    props: &[
        PropSpec::Text {
            key: "title",
            label: "Title",
            default: "Sound"
        },
        PropSpec::Text {
            key: "subtitle",
            label: "Subtitle",
            default: "Output device"
        },
    ],
    build: |p| Box::new(
        SettingsGroup::new("General").row(
            SettingsRow::new(p.str("title"))
                .subtitle(p.str("subtitle"))
                .trailing(Switch::new("").on(true)),
        ),
    ),
    snippet: |p| {
        format!(
        "SettingsGroup::new(\"General\")\n    .row(SettingsRow::new({:?})\n        .subtitle({:?})\n        .trailing(Switch::new(\"\")))",
        p.str("title"),
        p.str("subtitle"),
    )
    },
});

page!(DisclosurePage {
    meta: meta(
        "Disclosure",
        "Containers",
        "Disclosure triangle that reveals a child.",
        "Group",
        &[
            ("macOS", "NSDisclosure"),
            ("GTK", "GtkExpander"),
            ("Qt", "collapsible"),
            ("React", "<details>")
        ],
        false,
    ),
    props: &[PropSpec::Text {
        key: "title",
        label: "Title",
        default: "Details"
    }],
    build: |p| Box::new(
        Disclosure::new(p.str("title")).child(Text::new("Hidden content revealed on expand.")),
    ),
    snippet: |p| format!(
        "Disclosure::new({:?}).child(Text::new(\"Hidden content\"))",
        p.str("title"),
    ),
});

page!(AspectFramePage {
    meta: meta(
        "AspectFrame",
        "Containers",
        "Constrains its child to a fixed aspect ratio.",
        "Group",
        &[
            ("GTK", "GtkAspectFrame"),
            ("Qt", "aspect policy"),
            ("SwiftUI", "aspectRatio"),
            ("CSS", "aspect-ratio")
        ],
        false,
    ),
    props: &[PropSpec::Float {
        key: "ratio",
        label: "Ratio",
        min: 0.25,
        max: 4.0,
        step: 0.25,
        default: 1.78
    },],
    build: |p| Box::new(AspectFrame::new(p.f64("ratio") as f32).child(Text::new("16:9")),),
    snippet: |p| format!(
        "AspectFrame::new({:?}).child(Text::new(\"16:9\"))",
        p.f64("ratio") as f32,
    ),
});

page!(ClampPage {
    meta: meta(
        "Clamp",
        "Containers",
        "Caps content width — readability column.",
        "Group",
        &[
            ("GNOME", "AdwClamp"),
            ("CSS", "max-width"),
            ("Qt", "size policy"),
            ("SwiftUI", "frame(max:)")
        ],
        false,
    ),
    props: &[PropSpec::Float {
        key: "maximum",
        label: "Max width",
        min: 60.0,
        max: 480.0,
        step: 10.0,
        default: 240.0,
    }],
    build: |p| Box::new(
        Clamp::new()
            .maximum(p.f64("maximum") as f32)
            .child(Text::new(
                "This paragraph is clamped to a maximum readable width."
            )),
    ),
    snippet: |p| format!(
        "Clamp::new()\n    .maximum({:?})\n    .child(Text::new(\"…\"))",
        p.f64("maximum") as f32,
    ),
});

page!(SplitViewPage {
    meta: meta(
        "SplitView",
        "Containers",
        "Two resizable panes with a draggable divider.",
        "Group",
        &[
            ("Qt", "QSplitter"),
            ("GTK", "GtkPaned"),
            ("SwiftUI", "HSplitView"),
            ("React", "split pane")
        ],
        false,
    ),
    props: &[
        PropSpec::Choice {
            key: "orientation",
            label: "Orientation",
            options: &["Horizontal", "Vertical"],
            default: 0,
        },
        PropSpec::Float {
            key: "ratio",
            label: "Ratio",
            min: 0.1,
            max: 0.9,
            step: 0.05,
            default: 0.4
        },
    ],
    build: |p| {
        let (a, b) = (Text::new("Left pane"), Text::new("Right pane"));
        let mut s = if p.choice("orientation") == 1 {
            SplitView::vertical()
        } else {
            SplitView::horizontal()
        }
        .first(Container::new().padding_uniform(8.0).child(a))
        .second(Container::new().padding_uniform(8.0).child(b));
        s.set_ratio(p.f64("ratio") as f32);
        Box::new(s)
    },
    snippet: |p| format!(
        "SplitView::{}()\n    .first(pane_a)\n    .second(pane_b)",
        if p.choice("orientation") == 1 {
            "vertical"
        } else {
            "horizontal"
        },
    ),
    poll: |w, out| {
        if let Some(s) = downcast_mut::<SplitView>(w) {
            if let Some(r) = s.take_moved() {
                out.push(format!("ratio → {r:.2}"));
            }
        }
    },
});

page!(ScrollViewPage {
    meta: meta(
        "ScrollView",
        "Containers",
        "Scrolling viewport with scrollbars.",
        "ScrollArea",
        &[
            ("Qt", "QScrollArea"),
            ("GTK", "GtkScrolledWindow"),
            ("SwiftUI", "ScrollView"),
            ("CSS", "overflow:auto")
        ],
        false,
    ),
    props: &[PropSpec::Int {
        key: "lines",
        label: "Content lines",
        min: 4,
        max: 60,
        default: 24,
    }],
    build: |p| {
        let mut f = Flex::new(FlexDirection::Column).gap(4.0);
        for i in 1..=p.i64("lines") {
            f = f.child(Text::new(format!("Scrollable row {i}")));
        }
        Box::new(ScrollView::new(f))
    },
    snippet: |p| format!(
        "ScrollView::new(Flex::new(Column) /* {} rows */)",
        p.i64("lines"),
    ),
});

page!(ViewportPage {
    meta: meta(
        "Viewport",
        "Containers",
        "Pan/zoom canvas for large content.",
        "ScrollArea",
        &[
            ("Qt", "QGraphicsView"),
            ("GTK", "custom"),
            ("React", "pan-zoom"),
            ("Figma", "canvas")
        ],
        false,
    ),
    props: &[],
    build: |_p| {
        let mut v = Viewport::new();
        v.set_child(Box::new(Text::new("Drag to pan, scroll to zoom.")));
        Box::new(v)
    },
    snippet: |_p| "let mut v = Viewport::new();\nv.set_child(Box::new(content));".to_string(),
    state: |w| {
        downcast_mut::<Viewport>(w)
            .map(|v| {
                vec![
                    ("zoom".to_string(), format!("{:.2}", v.zoom_value())),
                    ("pan".to_string(), format!("{:?}", v.pan_offset())),
                ]
            })
            .unwrap_or_default()
    },
});

page!(MasonryPage {
    meta: meta(
        "Masonry",
        "Containers",
        "Pinterest-style column-packing layout.",
        "Group",
        &[
            ("Web", "masonry"),
            ("CSS", "columns"),
            ("Qt", "flow layout"),
            ("React", "masonry")
        ],
        false,
    ),
    props: &[PropSpec::Int {
        key: "cols",
        label: "Columns",
        min: 1,
        max: 5,
        default: 3
    }],
    build: |p| {
        let mut m = Masonry::new().columns(p.i64("cols") as usize);
        for (i, h) in [48.0f32, 72.0, 40.0, 88.0, 56.0, 64.0].iter().enumerate() {
            m = m.child(
                Container::new()
                    .padding_uniform(6.0)
                    .child(Text::new(format!("Tile {} ({}px)", i + 1, h))),
            );
        }
        Box::new(m)
    },
    snippet: |p| format!("Masonry::new().columns({})", p.i64("cols")),
});

page!(FlowBoxPage {
    meta: meta(
        "FlowBox",
        "Containers",
        "Wrapping flow of equal cells — GTK FlowBox.",
        "Group",
        &[
            ("GTK", "GtkFlowBox"),
            ("CSS", "flex-wrap"),
            ("Qt", "flow layout"),
            ("Android", "FlowLayout")
        ],
        false,
    ),
    props: &[PropSpec::Float {
        key: "gap",
        label: "Gap",
        min: 0.0,
        max: 24.0,
        step: 2.0,
        default: 8.0
    }],
    build: |p| {
        let mut fb = FlowBox::new().gap(p.f64("gap") as f32);
        for label in ["alpha", "beta", "gamma", "delta", "epsilon"] {
            fb = fb.child(
                Container::new()
                    .padding_uniform(6.0)
                    .child(Text::new(label)),
            );
        }
        Box::new(fb)
    },
    snippet: |p| format!("FlowBox::new().gap({:?})", p.f64("gap") as f32),
    poll: |w, out| {
        if let Some(fb) = downcast_mut::<FlowBox>(w) {
            if let Some(i) = fb.take_selected() {
                out.push(format!("cell {i}"));
            }
        }
    },
});

page!(SeparatorPage {
    meta: meta(
        "Separator",
        "Containers",
        "Hairline divider — horizontal or vertical.",
        "Separator",
        &[
            ("Qt", "QFrame line"),
            ("GTK", "GtkSeparator"),
            ("HTML", "<hr>"),
            ("SwiftUI", "Divider")
        ],
        false,
    ),
    props: &[PropSpec::Choice {
        key: "dir",
        label: "Direction",
        options: &["Horizontal", "Vertical"],
        default: 0,
    }],
    build: |p| {
        if p.choice("dir") == 1 {
            Box::new(Separator::vertical())
        } else {
            Box::new(Separator::horizontal())
        }
    },
    snippet: |p| format!(
        "Separator::{}()",
        if p.choice("dir") == 1 {
            "vertical"
        } else {
            "horizontal"
        },
    ),
});

page!(DescriptionsPage {
    meta: meta(
        "Descriptions",
        "Containers",
        "Label/value spec sheet — multi-column term list.",
        "Table",
        &[
            ("HTML", "<dl>"),
            ("Ant", "Descriptions"),
            ("Qt", "form grid"),
            ("GTK", "property rows")
        ],
        false,
    ),
    props: &[
        PropSpec::Int {
            key: "cols",
            label: "Columns",
            min: 1,
            max: 4,
            default: 2
        },
        PropSpec::Bool {
            key: "bordered",
            label: "Bordered",
            default: true
        },
    ],
    build: |p| {
        let mut d = Descriptions::new()
            .title("Asset")
            .column_count(p.i64("cols") as usize)
            .bordered(p.bool("bordered"));
        for (k, v) in [
            ("Name", "alpha-7"),
            ("Serial", "SN-4822"),
            ("Firmware", "v3.1.4"),
            ("Status", "Online"),
        ] {
            d = d.with_item(DescriptionItem::new(k, v));
        }
        Box::new(d)
    },
    snippet: |p| format!(
        "Descriptions::new()\n    .title(\"Asset\")\n    .column_count({})\n    .bordered({})",
        p.i64("cols"),
        p.bool("bordered"),
    ),
});

/// All Containers pages, in rail order.
pub(super) fn pages() -> Vec<Box<dyn Page>> {
    vec![
        Box::new(ContainerPage),
        Box::new(FlexPage),
        Box::new(StackPage),
        Box::new(GridPage),
        Box::new(GroupBoxPage),
        Box::new(CardPage),
        Box::new(ExpanderRowPage),
        Box::new(SettingsRowPage),
        Box::new(DisclosurePage),
        Box::new(AspectFramePage),
        Box::new(ClampPage),
        Box::new(SplitViewPage),
        Box::new(ScrollViewPage),
        Box::new(ViewportPage),
        Box::new(MasonryPage),
        Box::new(FlowBoxPage),
        Box::new(SeparatorPage),
        Box::new(DescriptionsPage),
    ]
}
