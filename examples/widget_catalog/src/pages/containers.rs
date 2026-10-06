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
use crate::pages::{downcast_mut, meta, page, SnipProp};

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
    props: &[
        PropSpec::Float {
            key: "padding",
            label: "Padding",
            min: 0.0,
            max: 64.0,
            step: 2.0,
            default: 16.0
        },
        PropSpec::Text {
            key: "background",
            label: "Background",
            // A visible tint by default — padding only reads against
            // the container's edge; an unpainted box grows invisibly
            // around the centered child, which is why the prop
            // audited dead before this default existed.
            default: "40,45,55,255"
        },
        PropSpec::Probe {
            key: "background",
            value: "40,80,60,255"
        },
    ],
    build: |p| {
        let mut __w = Container::new()
            .padding_uniform(p.f64("padding") as f32)
            .child(Text::new("Contained content"));
        if let Some([r, g, b, _]) = crate::pages::parse_rgba(p.str("background")) {
            __w = __w.background(martensite_theme::Oklab::from_srgb(
                r as f32 / 255.0,
                g as f32 / 255.0,
                b as f32 / 255.0,
            ));
        }
        Box::new(__w)
    },
    snippet: |p| {
        let mut __s = {
            format!(
        "Container::new()\n    .padding_uniform({:?})\n    .child(Text::new(\"Contained content\"))",
        p.f64("padding") as f32,
    )
        };
        __s.push_str(&crate::pages::prop_snippet(p, &[]));
        __s.push_str(&crate::pages::snip_textmap(
            p,
            "background",
            ".background",
            "",
            crate::pages::expr_oklab,
        ));
        __s
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
        PropSpec::Choice {
            key: "main_axis_alignment",
            label: "Main Axis Alignment",
            options: &["Start", "End", "Center", "Space Between", "Space Evenly"],
            default: 0
        },
        PropSpec::Choice {
            key: "cross_axis_alignment",
            label: "Cross Axis Alignment",
            options: &["Stretch", "Start", "End", "Center"],
            default: 0
        },
    ],
    build: |p| {
        let dir = if p.choice("direction") == 1 {
            FlexDirection::Column
        } else {
            FlexDirection::Row
        };
        {
            // `main_axis_size(Max)` claims the stage's main extent so
            // alignment has slack to distribute; the tall middle child
            // gives cross-axis alignment something to move.
            let mut __w = Flex::new(dir)
                .main_axis_size(martensite::widgets::flex::MainAxisSize::Max)
                .gap(p.f64("gap") as f32)
                .child(Button::new("One"))
                .child(
                    Container::new()
                        .padding_uniform(20.0)
                        .child(Button::new("Two")),
                )
                .child(Button::new("Three"));
            if p.choice("main_axis_alignment") != 0 {
                __w = __w.main_axis_alignment(match p.choice("main_axis_alignment") {
                    0 => martensite::widgets::flex::MainAxisAlignment::Start,
                    1 => martensite::widgets::flex::MainAxisAlignment::End,
                    2 => martensite::widgets::flex::MainAxisAlignment::Center,
                    3 => martensite::widgets::flex::MainAxisAlignment::SpaceBetween,
                    4 => martensite::widgets::flex::MainAxisAlignment::SpaceEvenly,
                    _ => martensite::widgets::flex::MainAxisAlignment::Start,
                });
            }
            if p.choice("cross_axis_alignment") != 0 {
                __w = __w.cross_axis_alignment(match p.choice("cross_axis_alignment") {
                    0 => martensite::widgets::flex::CrossAxisAlignment::Stretch,
                    1 => martensite::widgets::flex::CrossAxisAlignment::Start,
                    2 => martensite::widgets::flex::CrossAxisAlignment::End,
                    3 => martensite::widgets::flex::CrossAxisAlignment::Center,
                    _ => martensite::widgets::flex::CrossAxisAlignment::Stretch,
                });
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = {
            format!(
        "Flex::new(FlexDirection::{})\n    .gap({:?})\n    .child(Button::new(\"One\"))\n    .child(Button::new(\"Two\"))",
        if p.choice("direction") == 1 { "Column" } else { "Row" },
        p.f64("gap") as f32,
    )
        };
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                (
                    "main_axis_alignment",
                    ".main_axis_alignment",
                    SnipProp::Choice(&[
                        "martensite::widgets::flex::MainAxisAlignment::Start",
                        "martensite::widgets::flex::MainAxisAlignment::End",
                        "martensite::widgets::flex::MainAxisAlignment::Center",
                        "martensite::widgets::flex::MainAxisAlignment::SpaceBetween",
                        "martensite::widgets::flex::MainAxisAlignment::SpaceEvenly",
                    ]),
                ),
                (
                    "cross_axis_alignment",
                    ".cross_axis_alignment",
                    SnipProp::Choice(&[
                        "martensite::widgets::flex::CrossAxisAlignment::Stretch",
                        "martensite::widgets::flex::CrossAxisAlignment::Start",
                        "martensite::widgets::flex::CrossAxisAlignment::End",
                        "martensite::widgets::flex::CrossAxisAlignment::Center",
                    ]),
                ),
            ],
        ));
        __s
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
    props: &[PropSpec::Choice {
        key: "alignment",
        label: "Alignment",
        options: &[
            "Top Start",
            "Top End",
            "Bottom Start",
            "Bottom End",
            "Center",
            "Stretch"
        ],
        default: 0
    },],
    build: |_p| {
        let mut s = Stack::new().child(
            Container::new()
                .padding_uniform(8.0)
                .child(Text::new("Base layer")),
        );
        s = s.child(Text::new("Overlay layer"));
        {
            let mut __w = s;
            if _p.choice("alignment") != 0 {
                __w = __w.alignment(match _p.choice("alignment") {
                    0 => martensite::widgets::stack::StackAlignment::TopStart,
                    1 => martensite::widgets::stack::StackAlignment::TopEnd,
                    2 => martensite::widgets::stack::StackAlignment::BottomStart,
                    3 => martensite::widgets::stack::StackAlignment::BottomEnd,
                    4 => martensite::widgets::stack::StackAlignment::Center,
                    5 => martensite::widgets::stack::StackAlignment::Stretch,
                    _ => martensite::widgets::stack::StackAlignment::TopStart,
                });
            }
            Box::new(__w)
        }
    },
    snippet: |_p| {
        let mut __s =
            { "Stack::new()\n    .child(base_widget)\n    .child(badge_overlay)".to_string() };
        __s.push_str(&crate::pages::prop_snippet(
            _p,
            &[(
                "alignment",
                ".alignment",
                SnipProp::Choice(&[
                    "martensite::widgets::stack::StackAlignment::TopStart",
                    "martensite::widgets::stack::StackAlignment::TopEnd",
                    "martensite::widgets::stack::StackAlignment::BottomStart",
                    "martensite::widgets::stack::StackAlignment::BottomEnd",
                    "martensite::widgets::stack::StackAlignment::Center",
                    "martensite::widgets::stack::StackAlignment::Stretch",
                ]),
            )],
        ));
        __s
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
    props: &[
        PropSpec::Int {
            key: "cols",
            label: "Columns",
            min: 1,
            max: 6,
            default: 3
        },
        PropSpec::Float {
            key: "gap",
            label: "Gap",
            min: 0.0,
            max: 64.0,
            step: 0.5,
            default: 8.0
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
        let mut g = Grid::new().columns(p.i64("cols") as u32).row_height(32.0);
        for i in 1..=6 {
            g = g.cell(GridCell::new(Button::new(format!("Cell {i}"))));
        }
        {
            let mut __w = g;
            __w = __w.enabled(p.bool("enabled"));
            if !p.str("a11y_label").is_empty() {
                __w = __w.label(p.str("a11y_label"));
            }
            if p.f64("gap") != 8.0 {
                __w = __w.gap(p.f64("gap") as f32);
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = {
            format!(
        "Grid::new()\n    .columns({})\n    .row_height(32.0)\n    .cell(GridCell::new(Button::new(\"Cell\")))",
        p.i64("cols"),
    )
        };
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("gap", ".gap", SnipProp::Float(8.0)),
                ("enabled", ".enabled", SnipProp::Bool(true)),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        __s
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
        PropSpec::Bool {
            key: "checked",
            label: "Checked",
            default: false
        },
        PropSpec::Float {
            key: "padding",
            label: "Padding",
            min: 0.0,
            max: 100.0,
            step: 0.5,
            default: 0.0
        },
    ],
    build: |p| {
        // A checked box needs the checkbox — `checked` implies checkable.
        let mut __w = GroupBox::new(p.str("title"))
            .checkable(p.bool("checkable") || p.bool("checked"))
            .checked(p.bool("checked"));
        if p.f64("padding") != 0.0 {
            __w = __w.padding(martensite_layout::geometry::EdgeInsets::uniform(
                p.f64("padding") as f32,
            ));
        }
        Box::new(__w.child(Text::new("Grouped content")))
    },
    snippet: |p| {
        let mut __s = format!(
            "GroupBox::new({:?})\n    .checkable({})\n    .child(Text::new(\"Grouped content\"))",
            p.str("title"),
            p.bool("checkable") || p.bool("checked"),
        );
        if p.bool("checked") {
            __s.push_str("\n    .checked(true)");
        }
        __s.push_str(&crate::pages::prop_snippet(p, &[]));
        if p.f64("padding") != 0.0 {
            __s.push_str(&format!(
                "\n    .padding(EdgeInsets::uniform({}))",
                p.f64("padding")
            ));
        }
        __s
    },
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
        PropSpec::Float {
            key: "padding_uniform",
            label: "Padding Uniform",
            min: 0.0,
            max: 64.0,
            step: 0.5,
            default: 0.0
        },
        PropSpec::Float {
            key: "padding",
            label: "Padding",
            min: 0.0,
            max: 100.0,
            step: 0.5,
            default: 0.0
        },
    ],
    build: |p| {
        let variant = match p.choice("variant") {
            1 => CardVariant::Filled,
            2 => CardVariant::Outlined,
            _ => CardVariant::Elevated,
        };
        {
            let mut __w = Card::new()
                .title(p.str("title"))
                .variant(variant)
                .child(Text::new("Card body content goes here."));
            if p.f64("padding_uniform") != 0.0 {
                __w = __w.padding_uniform(p.f64("padding_uniform") as f32);
            }
            if p.f64("padding") != 0.0 {
                __w = __w.padding(martensite_layout::geometry::EdgeInsets::uniform(
                    p.f64("padding") as f32,
                ));
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = {
            format!(
        "Card::new()\n    .title({:?})\n    .variant(CardVariant::{})\n    .child(Text::new(\"Card body\"))",
        p.str("title"),
        ["Elevated", "Filled", "Outlined"][p.choice("variant")],
    )
        };
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[("padding_uniform", ".padding_uniform", SnipProp::Float(0.0))],
        ));
        if p.f64("padding") != 0.0 {
            __s.push_str(&format!(
                "\n    .padding(EdgeInsets::uniform({}))",
                p.f64("padding")
            ));
        }
        __s
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
        PropSpec::Text {
            key: "icon",
            label: "Icon",
            default: ""
        },
        PropSpec::Text {
            key: "icon_d",
            label: "Icon D",
            default: ""
        },
        PropSpec::Probe {
            key: "icon_d",
            value: "M4 4h16v16H4z"
        },
        PropSpec::Text {
            key: "icon_named",
            label: "Icon Named",
            default: ""
        },
        PropSpec::Probe {
            key: "icon_named",
            value: "media.play"
        },
        PropSpec::Bool {
            key: "expanded",
            label: "Expanded",
            default: false
        },
        PropSpec::Header {
            label: "State & Accessibility"
        },
        PropSpec::Bool {
            key: "enabled",
            label: "Enabled",
            default: true
        },
    ],
    build: |p| {
        let mut __w = ExpanderRow::new(p.str("title"))
            .subtitle(p.str("subtitle"))
            .child(Switch::new("Deep option").on(true));
        __w = __w.enabled(p.bool("enabled"));
        if !p.str("icon").is_empty() {
            __w = __w.icon(p.str("icon"));
        }
        if !p.str("icon_d").is_empty() {
            __w = __w.icon_d(p.str("icon_d"));
        }
        if !p.str("icon_named").is_empty() {
            __w = __w.icon_named(p.str("icon_named"));
        }
        if p.bool("expanded") {
            __w = __w.expanded(p.bool("expanded"));
        }
        Box::new(__w)
    },
    snippet: |p| {
        let mut __s = format!(
            "ExpanderRow::new({:?})\n    .subtitle({:?})\n    .child(Switch::new(\"Deep option\"))",
            p.str("title"),
            p.str("subtitle"),
        );
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("icon", ".icon", SnipProp::Text("")),
                ("icon_d", ".icon_d", SnipProp::Text("")),
                ("icon_named", ".icon_named", SnipProp::Text("")),
                ("expanded", ".expanded", SnipProp::Bool(false)),
                ("enabled", ".enabled", SnipProp::Bool(true)),
            ],
        ));
        __s
    },
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
        PropSpec::Bool {
            key: "carded",
            label: "Carded",
            default: true
        },
        PropSpec::Text {
            key: "icon",
            label: "Icon",
            default: ""
        },
        PropSpec::Text {
            key: "icon_d",
            label: "Icon D",
            default: ""
        },
        PropSpec::Probe {
            key: "icon_d",
            value: "M4 4h16v16H4z"
        },
        PropSpec::Text {
            key: "icon_named",
            label: "Icon Named",
            default: ""
        },
        PropSpec::Probe {
            key: "icon_named",
            value: "media.play"
        },
        PropSpec::Bool {
            key: "activatable",
            label: "Activatable",
            default: false
        },
        PropSpec::Header {
            label: "State & Accessibility"
        },
        PropSpec::Bool {
            key: "enabled",
            label: "Enabled",
            default: true
        },
    ],
    build: |p| {
        let mut row = SettingsRow::new(p.str("title"))
            .subtitle(p.str("subtitle"))
            .trailing(Switch::new("").on(true));
        if !p.str("icon").is_empty() {
            row = row.icon(p.str("icon"));
        }
        if !p.str("icon_d").is_empty() {
            row = row.icon_d(p.str("icon_d"));
        }
        if !p.str("icon_named").is_empty() {
            row = row.icon_named(p.str("icon_named"));
        }
        if p.bool("activatable") {
            row = row.activatable(true);
        }
        if !p.bool("enabled") {
            row = row.enabled(false);
        }
        let mut __w = SettingsGroup::new("General").row(row);
        if !p.bool("carded") {
            __w = __w.carded(p.bool("carded"));
        }
        Box::new(__w)
    },
    snippet: |p| {
        let mut row_s = format!(
            "SettingsRow::new({:?})\n        .subtitle({:?})\n        .trailing(Switch::new(\"\")",
            p.str("title"),
            p.str("subtitle")
        );
        if !p.str("icon").is_empty() {
            row_s.push_str(&format!("\n        .icon({:?})", p.str("icon")));
        }
        if !p.str("icon_d").is_empty() {
            row_s.push_str(&format!("\n        .icon_d({:?})", p.str("icon_d")));
        }
        if !p.str("icon_named").is_empty() {
            row_s.push_str(&format!("\n        .icon_named({:?})", p.str("icon_named")));
        }
        if p.bool("activatable") {
            row_s.push_str("\n        .activatable(true)");
        }
        if !p.bool("enabled") {
            row_s.push_str("\n        .enabled(false)");
        }
        let mut __s = format!("SettingsGroup::new(\"General\")\n    .row({row_s})");
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[("carded", ".carded", SnipProp::Bool(true))],
        ));
        __s
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
    props: &[
        PropSpec::Float {
            key: "ratio",
            label: "Ratio",
            min: 0.25,
            max: 4.0,
            step: 0.25,
            default: 1.78
        },
        PropSpec::Float {
            key: "xalign",
            label: "Xalign",
            min: -9.25,
            max: 100.0,
            step: 1.0,
            default: 0.5
        },
        PropSpec::Float {
            key: "yalign",
            label: "Yalign",
            min: -9.25,
            max: 100.0,
            step: 1.0,
            default: 0.5
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
        // One frame can only letterbox one axis at a time: the top
        // row's cells are narrower than `ratio` so they letterbox
        // vertically (yalign live); the bottom wide cell is wider
        // than `ratio` so it letterboxes horizontally (xalign live).
        let frame = |xalign: f32, yalign: f32| {
            let mut w = AspectFrame::new(p.f64("ratio") as f32).child(
                Container::new()
                    .background(martensite_theme::Oklab::from_srgb(0.10, 0.19, 0.34))
                    .child(Text::new("16:9")),
            );
            if !p.str("a11y_label").is_empty() {
                w = w.label(p.str("a11y_label"));
            }
            if xalign != 0.5 {
                w = w.xalign(xalign);
            }
            if yalign != 0.5 {
                w = w.yalign(yalign);
            }
            w
        };
        let top = Flex::row()
            .gap(8.0)
            .main_axis_size(martensite::widgets::flex::MainAxisSize::Max)
            .child_flex(frame(0.5, p.f64("yalign") as f32), 1.0)
            .child_flex(frame(0.5, p.f64("yalign") as f32), 1.0);
        let bottom = frame(p.f64("xalign") as f32, 0.5);
        Box::new(
            Flex::column()
                .gap(8.0)
                .main_axis_size(martensite::widgets::flex::MainAxisSize::Max)
                .child_flex(top, 1.0)
                .child_flex(bottom, 1.0),
        )
    },
    snippet: |p| {
        let mut __s = format!(
            "AspectFrame::new({:?}).child(Text::new(\"16:9\"))",
            p.f64("ratio") as f32,
        );
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("xalign", ".xalign", SnipProp::Float(0.5)),
                ("yalign", ".yalign", SnipProp::Float(0.5)),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        __s
    },
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
    props: &[
        PropSpec::Float {
            key: "maximum",
            label: "Max width",
            min: 60.0,
            max: 480.0,
            step: 10.0,
            default: 240.0,
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
        let mut __w = Clamp::new()
            .maximum(p.f64("maximum") as f32)
            .child(Text::new(
                "This paragraph is clamped to a maximum readable width.",
            ));
        if !p.str("a11y_label").is_empty() {
            __w = __w.label(p.str("a11y_label"));
        }
        Box::new(__w)
    },
    snippet: |p| {
        let mut __s = format!(
            "Clamp::new()\n    .maximum({:?})\n    .child(Text::new(\"…\"))",
            p.f64("maximum") as f32,
        );
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[("a11y_label", ".label", SnipProp::Text(""))],
        ));
        __s
    },
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
        PropSpec::Float {
            key: "default_ratio",
            label: "Default Ratio",
            min: 0.0,
            max: 1.0,
            step: 0.05,
            default: 0.0
        },
        PropSpec::Float {
            key: "minimums",
            label: "Minimums",
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
    ],
    build: |p| {
        let (a, b) = (Text::new("Left pane"), Text::new("Right pane"));
        let s = if p.choice("orientation") == 1 {
            SplitView::vertical()
        } else {
            SplitView::horizontal()
        }
        .first(Container::new().padding_uniform(8.0).child(a))
        .second(Container::new().padding_uniform(8.0).child(b));
        {
            let mut __w = s;
            __w = __w.enabled(p.bool("enabled"));
            // `minimums` must land before the staged split so the clamp
            // bites; `default_ratio` is staged live so the reset
            // target is what the divider shows.
            if p.f64("minimums") != 0.0 {
                __w = __w.minimums(p.f64("minimums") as f32 / 100.0);
            }
            if p.f64("default_ratio") != 0.0 {
                __w = __w.default_ratio(p.f64("default_ratio") as f32);
                __w.set_ratio(p.f64("default_ratio") as f32);
            } else {
                __w.set_ratio(p.f64("ratio") as f32);
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = format!(
            "SplitView::{}()\n    .first(pane_a)\n    .second(pane_b)",
            if p.choice("orientation") == 1 {
                "vertical"
            } else {
                "horizontal"
            },
        );
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("default_ratio", ".default_ratio", SnipProp::Float(0.0)),
                ("minimums", ".minimums", SnipProp::Float(0.0)),
                ("enabled", ".enabled", SnipProp::Bool(true)),
            ],
        ));
        __s
    },
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
    props: &[
        PropSpec::Int {
            key: "lines",
            label: "Content lines",
            min: 4,
            max: 60,
            default: 24,
        },
        PropSpec::Header {
            label: "State & Accessibility"
        },
        PropSpec::Bool {
            key: "enabled",
            label: "Enabled",
            default: true
        },
    ],
    build: |p| {
        let mut f = Flex::new(FlexDirection::Column).gap(4.0);
        for i in 1..=p.i64("lines") {
            f = f.child(Text::new(format!("Scrollable row {i}")));
        }
        // The stage viewport paints a decorative dot grid across the
        // whole surface; the content needs its own opaque face so the
        // rows read (and lint-probe) against a solid background.
        let sheet = Container::new()
            .background(martensite_theme::Oklab::from_srgb(0.15, 0.16, 0.19))
            .padding_uniform(6.0)
            .child(f);
        {
            let mut __w = ScrollView::new(sheet);
            __w = __w.enabled(p.bool("enabled"));
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = format!(
            "ScrollView::new(Flex::new(Column) /* {} rows */)",
            p.i64("lines"),
        );
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[("enabled", ".enabled", SnipProp::Bool(true))],
        ));
        __s
    },
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
    props: &[
        PropSpec::Float {
            key: "zoom",
            label: "Zoom",
            min: 0.0,
            max: 1.0,
            step: 0.05,
            default: 1.0
        },
        PropSpec::Text {
            key: "pan",
            label: "Pan",
            default: ""
        },
        PropSpec::Probe {
            key: "pan",
            value: "40,40"
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
        let mut v = Viewport::new();
        v.set_child(Box::new(Text::new("Drag to pan, scroll to zoom.")));
        {
            let mut __w = v;
            __w = __w.enabled(_p.bool("enabled"));
            if !_p.str("a11y_label").is_empty() {
                __w = __w.label(_p.str("a11y_label"));
            }
            if _p.f64("zoom") != 1.0 {
                __w = __w.zoom(_p.f64("zoom") as f32);
            }
            if let Some((x, y)) = crate::pages::parse_pair(_p.str("pan")) {
                __w = __w.pan(glam::Vec2::new(x as f32, y as f32));
            }
            Box::new(__w)
        }
    },
    snippet: |_p| {
        let mut __s = "let mut v = Viewport::new();\nv.set_child(Box::new(content));".to_string();
        __s.push_str(&crate::pages::prop_snippet(
            _p,
            &[
                ("zoom", ".zoom", SnipProp::Float(1.0)),
                ("enabled", ".enabled", SnipProp::Bool(true)),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        __s.push_str(&crate::pages::snip_textmap(
            _p,
            "pan",
            ".pan",
            "",
            crate::pages::expr_pair,
        ));
        __s
    },
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
    props: &[
        PropSpec::Int {
            key: "cols",
            label: "Columns",
            min: 1,
            max: 5,
            default: 3
        },
        PropSpec::Float {
            key: "gap",
            label: "Gap",
            min: 0.0,
            max: 64.0,
            step: 0.5,
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
    build: |p| {
        let mut m = Masonry::new().columns(p.i64("cols") as usize);
        let raised = martensite_theme::Oklab::from_srgb(0.16, 0.17, 0.21);
        for (i, h) in [48.0f32, 72.0, 40.0, 88.0, 56.0, 64.0].iter().enumerate() {
            m = m.child(
                Container::new()
                    .background(raised)
                    .padding(martensite_layout::geometry::EdgeInsets::new(
                        8.0, 8.0, 8.0, *h,
                    ))
                    .child(Text::new(format!("Tile {} ({}px)", i + 1, h))),
            );
        }
        {
            let mut __w = m;
            __w = __w.enabled(p.bool("enabled"));
            if !p.str("a11y_label").is_empty() {
                __w = __w.label(p.str("a11y_label"));
            }
            if p.f64("gap") != 0.0 {
                __w = __w.gap(p.f64("gap") as f32);
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = format!("Masonry::new().columns({})", p.i64("cols"));
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("gap", ".gap", SnipProp::Float(0.0)),
                ("enabled", ".enabled", SnipProp::Bool(true)),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        __s
    },
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
    props: &[
        PropSpec::Float {
            key: "gap",
            label: "Gap",
            min: 0.0,
            max: 24.0,
            step: 2.0,
            default: 8.0
        },
        PropSpec::Choice {
            key: "selection_mode",
            label: "Selection Mode",
            options: &["None", "Single"],
            default: 0
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
        let mut fb = FlowBox::new().gap(p.f64("gap") as f32);
        for label in ["alpha", "beta", "gamma", "delta", "epsilon"] {
            fb = fb.child(
                Container::new()
                    .padding_uniform(6.0)
                    .child(Text::new(label)),
            );
        }
        {
            let mut __w = fb;
            __w = __w.enabled(p.bool("enabled"));
            if !p.str("a11y_label").is_empty() {
                __w = __w.label(p.str("a11y_label"));
            }
            if p.choice("selection_mode") != 0 {
                __w = __w.selection_mode(match p.choice("selection_mode") {
                    0 => martensite::widgets::flow_box::FlowSelection::None,
                    1 => martensite::widgets::flow_box::FlowSelection::Single,
                    _ => martensite::widgets::flow_box::FlowSelection::None,
                });
                // Stage a committed selection so the mode's accent
                // ring is what the frame shows — selection is
                // otherwise invisible until a click lands.
                __w.select(1);
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = format!("FlowBox::new().gap({:?})", p.f64("gap") as f32);
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                (
                    "selection_mode",
                    ".selection_mode",
                    SnipProp::Choice(&[
                        "martensite::widgets::flow_box::FlowSelection::None",
                        "martensite::widgets::flow_box::FlowSelection::Single",
                    ]),
                ),
                ("enabled", ".enabled", SnipProp::Bool(true)),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        __s
    },
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
        PropSpec::Text {
            key: "item_label",
            label: "Item Label",
            default: ""
        },
        PropSpec::Text {
            key: "item_content",
            label: "Item Content",
            default: ""
        },
        PropSpec::Header {
            label: "State & Accessibility"
        },
        PropSpec::Bool {
            key: "enabled",
            label: "Enabled",
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
        {
            let mut __w = d;
            __w = __w.enabled(p.bool("enabled"));
            if !p.str("item_label").is_empty() || !p.str("item_content").is_empty() {
                __w = __w.item(p.str("item_label"), p.str("item_content"));
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = format!(
            "Descriptions::new()\n    .title(\"Asset\")\n    .column_count({})\n    .bordered({})",
            p.i64("cols"),
            p.bool("bordered"),
        );
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[("enabled", ".enabled", SnipProp::Bool(true))],
        ));
        if !p.str("item_label").is_empty() || !p.str("item_content").is_empty() {
            __s.push_str(&format!(
                "\n    .item({:?}, {:?})",
                p.str("item_label"),
                p.str("item_content")
            ));
        }
        __s
    },
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
