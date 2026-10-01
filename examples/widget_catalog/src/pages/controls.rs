//! Controls family — buttons, toggles, sliders, pickers, and
//! selection widgets.

use martensite::widgets::alpha_slider::AlphaSlider;
use martensite::widgets::button::Button;
use martensite::widgets::checkbox::CheckBox;
use martensite::widgets::chip::{Chip, ChipKind};
use martensite::widgets::chip_group::ChipGroup;
use martensite::widgets::color_button::ColorButton;
use martensite::widgets::color_palette::ColorPalette;
use martensite::widgets::color_picker::ColorPicker;
use martensite::widgets::color_wheel::ColorWheel;
use martensite::widgets::command_link::CommandLink;
use martensite::widgets::crosshair::Crosshair;
use martensite::widgets::dial::Dial;
use martensite::widgets::dropdown::Dropdown;
use martensite::widgets::file_chooser_button::FileChooserButton;
use martensite::widgets::float_button::FloatButton;
use martensite::widgets::font_button::FontButton;
use martensite::widgets::hue_slider::HueSlider;
use martensite::widgets::joystick::Joystick;
use martensite::widgets::keypad::Keypad;
use martensite::widgets::link::Link;
use martensite::widgets::menu::MenuItem;
use martensite::widgets::menu_button::MenuButton;
use martensite::widgets::pad_grid::PadGrid;
use martensite::widgets::radio::RadioGroup;
use martensite::widgets::range_slider::RangeSlider;
use martensite::widgets::rating::Rating;
use martensite::widgets::segmented::Segmented;
use martensite::widgets::slider::Slider;
use martensite::widgets::spinbox::SpinBox;
use martensite::widgets::split_button::SplitButton;
use martensite::widgets::switch::Switch;
use martensite::widgets::toggle_button::ToggleButton;
use martensite::widgets::tuner::Tuner;
use martensite::widgets::volume::Volume;
use martensite::widgets::wheel_picker::WheelPicker;
use martensite::widgets::xy_pad::XYPad;
use martensite::widgets::zoom_controls::ZoomControls;

use crate::page::{Page, PropSpec};
use crate::pages::{csv, downcast_mut, meta, page};

page!(ButtonPage {
    meta: meta(
        "Button",
        "Controls",
        "Clickable command button with optional icon, primary styling, and tooltip.",
        "Button",
        &[
            ("Qt", "QPushButton"),
            ("GTK", "GtkButton"),
            ("SwiftUI", "Button"),
            ("React", "<button>")
        ],
        false,
    ),
    props: &[
        PropSpec::Text {
            key: "label",
            label: "Label",
            default: "Apply"
        },
        PropSpec::Bool {
            key: "primary",
            label: "Primary",
            default: false
        },
        PropSpec::Bool {
            key: "enabled",
            label: "Enabled",
            default: true
        },
        PropSpec::Text {
            key: "tooltip",
            label: "Tooltip",
            default: ""
        },
    ],
    build: |p| {
        let mut b = Button::new(p.str("label"))
            .primary(p.bool("primary"))
            .enabled(p.bool("enabled"));
        let tip = p.str("tooltip");
        if !tip.is_empty() {
            b = b.tooltip(tip);
        }
        Box::new(b)
    },
    snippet: |p| {
        let mut s = format!("Button::new({:?})", p.str("label"));
        if p.bool("primary") {
            s.push_str("\n    .primary(true)");
        }
        if !p.bool("enabled") {
            s.push_str("\n    .enabled(false)");
        }
        let tip = p.str("tooltip");
        if !tip.is_empty() {
            s.push_str(&format!("\n    .tooltip({tip:?})"));
        }
        s
    },
    poll: |w, out| {
        if downcast_mut::<Button>(w).is_some_and(|b| b.take_activated()) {
            out.push("activated".to_string());
        }
    },
    state: |w| {
        downcast_mut::<Button>(w)
            .map(|b| vec![("pressed".to_string(), b.is_pressed().to_string())])
            .unwrap_or_default()
    },
});

page!(CheckBoxPage {
    meta: meta(
        "CheckBox",
        "Controls",
        "Binary or tristate check control.",
        "CheckBox",
        &[
            ("Qt", "QCheckBox"),
            ("GTK", "GtkCheckButton"),
            ("SwiftUI", "Toggle"),
            ("React", "<input type=checkbox>")
        ],
        false,
    ),
    props: &[
        PropSpec::Text {
            key: "label",
            label: "Label",
            default: "Enable notifications"
        },
        PropSpec::Bool {
            key: "checked",
            label: "Checked",
            default: false
        },
        PropSpec::Bool {
            key: "tristate",
            label: "Tristate",
            default: false
        },
        PropSpec::Bool {
            key: "enabled",
            label: "Enabled",
            default: true
        },
    ],
    build: |p| Box::new(
        CheckBox::new(p.str("label"))
            .checked(p.bool("checked"))
            .tristate(p.bool("tristate"))
            .enabled(p.bool("enabled")),
    ),
    snippet: |p| {
        let mut s = format!("CheckBox::new({:?})", p.str("label"));
        if p.bool("checked") {
            s.push_str("\n    .checked(true)");
        }
        if p.bool("tristate") {
            s.push_str("\n    .tristate(true)");
        }
        if !p.bool("enabled") {
            s.push_str("\n    .enabled(false)");
        }
        s
    },
    state: |w| {
        downcast_mut::<CheckBox>(w)
            .map(|c| vec![("state".to_string(), format!("{:?}", c.state()))])
            .unwrap_or_default()
    },
});

page!(SwitchPage {
    meta: meta(
        "Switch",
        "Controls",
        "Sliding on/off switch.",
        "Switch",
        &[
            ("Qt", "QCheckBox (switch)"),
            ("GTK", "GtkSwitch"),
            ("SwiftUI", "Toggle(switch)"),
            ("iOS", "UISwitch")
        ],
        false,
    ),
    props: &[
        PropSpec::Text {
            key: "label",
            label: "Label",
            default: "Wi-Fi"
        },
        PropSpec::Bool {
            key: "on",
            label: "On",
            default: true
        },
        PropSpec::Bool {
            key: "enabled",
            label: "Enabled",
            default: true
        },
    ],
    build: |p| Box::new(
        Switch::new(p.str("label"))
            .on(p.bool("on"))
            .enabled(p.bool("enabled")),
    ),
    snippet: |p| {
        let mut s = format!("Switch::new({:?}).on({})", p.str("label"), p.bool("on"));
        if !p.bool("enabled") {
            s.push_str("\n    .enabled(false)");
        }
        s
    },
    state: |w| {
        downcast_mut::<Switch>(w)
            .map(|s| vec![("on".to_string(), s.on.to_string())])
            .unwrap_or_default()
    },
});

page!(ToggleButtonPage {
    meta: meta(
        "ToggleButton",
        "Controls",
        "Button that stays pressed — toolbar toggle semantics.",
        "ToggleButton",
        &[
            ("Qt", "QToolButton(checkable)"),
            ("GTK", "GtkToggleButton"),
            ("SwiftUI", "Toggle(button)"),
            ("React", "aria-pressed button")
        ],
        false,
    ),
    props: &[
        PropSpec::Text {
            key: "label",
            label: "Label",
            default: "Bold"
        },
        PropSpec::Bool {
            key: "pressed",
            label: "Pressed",
            default: false
        },
        PropSpec::Bool {
            key: "enabled",
            label: "Enabled",
            default: true
        },
    ],
    build: |p| Box::new(
        ToggleButton::new(p.str("label"))
            .pressed(p.bool("pressed"))
            .enabled(p.bool("enabled")),
    ),
    snippet: |p| format!(
        "ToggleButton::new({:?})\n    .pressed({})\n    .enabled({})",
        p.str("label"),
        p.bool("pressed"),
        p.bool("enabled"),
    ),
    poll: |w, out| {
        if let Some(b) = downcast_mut::<ToggleButton>(w) {
            if let Some(on) = b.take_toggled() {
                out.push(format!("toggled → {on}"));
            }
        }
    },
});

page!(SliderPage {
    meta: meta(
        "Slider",
        "Controls",
        "Continuous value selector on a track.",
        "Slider",
        &[
            ("Qt", "QSlider"),
            ("GTK", "GtkScale"),
            ("SwiftUI", "Slider"),
            ("React", "<input type=range>")
        ],
        false,
    ),
    props: &[
        PropSpec::Float {
            key: "min",
            label: "Min",
            min: 0.0,
            max: 100.0,
            step: 1.0,
            default: 0.0
        },
        PropSpec::Float {
            key: "max",
            label: "Max",
            min: 1.0,
            max: 100.0,
            step: 1.0,
            default: 100.0
        },
        PropSpec::Float {
            key: "value",
            label: "Value",
            min: 0.0,
            max: 100.0,
            step: 1.0,
            default: 40.0
        },
    ],
    build: |p| {
        let (min, max) = (
            p.f64("min").min(p.f64("max")),
            p.f64("max").max(p.f64("min")),
        );
        let v = p.f64("value").clamp(min, max);
        Box::new(Slider::new(min, max).with_value(v))
    },
    snippet: |p| format!(
        "Slider::new({}, {}).with_value({})",
        p.f64("min"),
        p.f64("max"),
        p.f64("value"),
    ),
    state: |w| {
        downcast_mut::<Slider>(w)
            .map(|s| {
                vec![
                    ("value".to_string(), format!("{:.2}", s.value())),
                    ("dragging".to_string(), s.is_dragging().to_string()),
                ]
            })
            .unwrap_or_default()
    },
});

page!(RangeSliderPage {
    meta: meta(
        "RangeSlider",
        "Controls",
        "Dual-thumb range selector.",
        "Slider",
        &[
            ("Qt", "QxtSpanSlider"),
            ("GTK", "two GtkScales"),
            ("iOS", "range slider"),
            ("React", "dual range")
        ],
        false,
    ),
    props: &[
        PropSpec::Float {
            key: "low",
            label: "Low",
            min: 0.0,
            max: 100.0,
            step: 1.0,
            default: 20.0
        },
        PropSpec::Float {
            key: "high",
            label: "High",
            min: 0.0,
            max: 100.0,
            step: 1.0,
            default: 80.0
        },
    ],
    build: |p| {
        let mut s = RangeSlider::new(0.0, 100.0);
        s.set_low(p.f64("low"));
        s.set_high(p.f64("high").max(p.f64("low")));
        Box::new(s)
    },
    snippet: |p| format!(
        "let mut s = RangeSlider::new(0.0, 100.0);\ns.set_low({});\ns.set_high({});",
        p.f64("low"),
        p.f64("high"),
    ),
    state: |w| {
        downcast_mut::<RangeSlider>(w)
            .map(|s| {
                vec![
                    ("low".to_string(), s.low().to_string()),
                    ("high".to_string(), s.high().to_string()),
                ]
            })
            .unwrap_or_default()
    },
});

page!(SpinBoxPage {
    meta: meta(
        "SpinBox",
        "Controls",
        "Numeric field with stepper buttons.",
        "SpinBox",
        &[
            ("Qt", "QSpinBox"),
            ("GTK", "GtkSpinButton"),
            ("SwiftUI", "Stepper"),
            ("React", "<input type=number>")
        ],
        false,
    ),
    props: &[
        PropSpec::Int {
            key: "min",
            label: "Min",
            min: -100,
            max: 100,
            default: 0
        },
        PropSpec::Int {
            key: "max",
            label: "Max",
            min: -100,
            max: 1000,
            default: 100
        },
        PropSpec::Int {
            key: "value",
            label: "Value",
            min: -100,
            max: 1000,
            default: 12
        },
        PropSpec::Text {
            key: "suffix",
            label: "Suffix",
            default: ""
        },
    ],
    build: |p| {
        let mut s = SpinBox::new()
            .range(p.i64("min") as f64, p.i64("max") as f64)
            .with_value(p.i64("value") as f64);
        let sfx = p.str("suffix");
        if !sfx.is_empty() {
            s = s.suffix(sfx);
        }
        Box::new(s)
    },
    snippet: |p| format!(
        "SpinBox::new()\n    .range({}, {})\n    .with_value({})",
        p.i64("min"),
        p.i64("max"),
        p.i64("value"),
    ),
    state: |w| {
        downcast_mut::<SpinBox>(w)
            .map(|s| vec![("value".to_string(), s.value().to_string())])
            .unwrap_or_default()
    },
});

page!(SegmentedPage {
    meta: meta(
        "Segmented",
        "Controls",
        "Single-select segmented control.",
        "RadioGroup",
        &[
            ("Qt", "QButtonGroup"),
            ("GTK", "linked toggle buttons"),
            ("SwiftUI", "Picker(segmented)"),
            ("iOS", "UISegmentedControl")
        ],
        false,
    ),
    props: &[PropSpec::Text {
        key: "options",
        label: "Options (csv)",
        default: "Day,Week,Month",
    }],
    build: |p| Box::new(Segmented::new().options(csv(p, "options")).selected(0)),
    snippet: |p| {
        let opts = csv(p, "options")
            .iter()
            .map(|o| format!("{o:?}"))
            .collect::<Vec<_>>()
            .join(", ");
        format!("Segmented::new().options([{opts}]).selected(0)")
    },
    poll: |w, out| {
        if let Some(seg) = downcast_mut::<Segmented>(w) {
            if let Some(i) = seg.take_selected() {
                out.push(format!("selected → {i}"));
            }
        }
    },
});

page!(DropdownPage {
    meta: meta(
        "Dropdown",
        "Controls",
        "Collapsed select with a popup option list.",
        "ComboBox",
        &[
            ("Qt", "QComboBox"),
            ("GTK", "GtkDropDown"),
            ("SwiftUI", "Picker(menu)"),
            ("React", "<select>")
        ],
        true,
    ),
    props: &[PropSpec::Text {
        key: "options",
        label: "Options (csv)",
        default: "Light,Dark,System",
    }],
    build: |p| Box::new(Dropdown::new(csv(p, "options"))),
    snippet: |p| {
        let opts = csv(p, "options")
            .iter()
            .map(|o| format!("{o:?}"))
            .collect::<Vec<_>>()
            .join(", ");
        format!("Dropdown::new([{opts}])")
    },
    state: |w| {
        downcast_mut::<Dropdown>(w)
            .map(|d| {
                vec![
                    ("selected".to_string(), d.selected().to_string()),
                    ("open".to_string(), d.popup_id().is_some().to_string()),
                ]
            })
            .unwrap_or_default()
    },
});

page!(RadioGroupPage {
    meta: meta(
        "RadioGroup",
        "Controls",
        "Mutually-exclusive radio option list.",
        "RadioGroup",
        &[
            ("Qt", "QRadioButton group"),
            ("GTK", "GtkCheckButton group"),
            ("SwiftUI", "Picker(radio)"),
            ("React", "<input type=radio>")
        ],
        false,
    ),
    props: &[PropSpec::Text {
        key: "options",
        label: "Options (csv)",
        default: "Small,Medium,Large",
    }],
    build: |p| Box::new(RadioGroup::new(csv(p, "options"))),
    snippet: |p| {
        let opts = csv(p, "options")
            .iter()
            .map(|o| format!("{o:?}"))
            .collect::<Vec<_>>()
            .join(", ");
        format!("RadioGroup::new([{opts}])")
    },
    state: |w| {
        downcast_mut::<RadioGroup>(w)
            .map(|r| vec![("selected".to_string(), format!("{:?}", r.selected()))])
            .unwrap_or_default()
    },
});

page!(MenuButtonPage {
    meta: meta(
        "MenuButton",
        "Controls",
        "Button that opens a menu popup.",
        "Button",
        &[
            ("Qt", "QToolButton+menu"),
            ("GTK", "GtkMenuButton"),
            ("SwiftUI", "Menu"),
            ("React", "button+menu")
        ],
        true,
    ),
    props: &[PropSpec::Text {
        key: "label",
        label: "Label",
        default: "Actions",
    }],
    build: |p| {
        let items = vec![
            MenuItem::action("Refresh"),
            MenuItem::action("Duplicate"),
            MenuItem::separator(),
            MenuItem::action("Delete").enabled(true),
        ];
        Box::new(MenuButton::new(p.str("label"), items))
    },
    snippet: |p| {
        format!(
        "MenuButton::new({:?}, vec![\n    MenuItem::action(\"Refresh\"),\n    MenuItem::action(\"Duplicate\"),\n    MenuItem::separator(),\n    MenuItem::action(\"Delete\"),\n])",
        p.str("label"),
    )
    },
    poll: |w, out| {
        if let Some(b) = downcast_mut::<MenuButton>(w) {
            if let Some(i) = b.take_activated() {
                out.push(format!("menu item {i:?}"));
            }
        }
    },
});

page!(SplitButtonPage {
    meta: meta(
        "SplitButton",
        "Controls",
        "Action button with a drop-down chevron half.",
        "Button",
        &[
            ("Qt", "QToolButton(split)"),
            ("GTK", "linked button+menu"),
            ("macOS", "NSSegmentedControl"),
            ("React", "split button")
        ],
        true,
    ),
    props: &[PropSpec::Text {
        key: "label",
        label: "Label",
        default: "Save",
    }],
    build: |p| Box::new(SplitButton::new(p.str("label"))),
    snippet: |p| format!("SplitButton::new({:?})", p.str("label")),
    poll: |w, out| {
        if let Some(b) = downcast_mut::<SplitButton>(w) {
            if b.take_activated() {
                out.push("main action".to_string());
            }
            if b.take_dropped() {
                out.push("chevron → menu".to_string());
            }
        }
    },
});

page!(ChipPage {
    meta: meta(
        "Chip",
        "Controls",
        "Compact filter/tag element — selectable, deletable.",
        "Button",
        &[
            ("Android", "Chip"),
            ("Material", "Chip"),
            ("React", "<Chip>"),
            ("SwiftUI", "capsule tag")
        ],
        false,
    ),
    props: &[
        PropSpec::Text {
            key: "label",
            label: "Label",
            default: "Rust"
        },
        PropSpec::Choice {
            key: "kind",
            label: "Kind",
            options: &["Assist", "Filter", "Input", "Suggestion"],
            default: 1,
        },
        PropSpec::Bool {
            key: "selected",
            label: "Selected",
            default: false
        },
        PropSpec::Bool {
            key: "deletable",
            label: "Deletable",
            default: false
        },
    ],
    build: |p| {
        let kind = match p.choice("kind") {
            0 => ChipKind::Assist,
            2 => ChipKind::Input,
            3 => ChipKind::Suggestion,
            _ => ChipKind::Filter,
        };
        Box::new(
            Chip::new(p.str("label"))
                .kind(kind)
                .selected(p.bool("selected"))
                .deletable(p.bool("deletable")),
        )
    },
    snippet: |p| format!(
        "Chip::new({:?})\n    .kind(ChipKind::{:?})\n    .selected({})\n    .deletable({})",
        p.str("label"),
        ["Assist", "Filter", "Input", "Suggestion"][p.choice("kind")],
        p.bool("selected"),
        p.bool("deletable"),
    ),
    poll: |w, out| {
        if let Some(c) = downcast_mut::<Chip>(w) {
            if let Some(sel) = c.take_selected() {
                out.push(format!("selected → {sel}"));
            }
            if c.take_deleted() {
                out.push("deleted".to_string());
            }
        }
    },
});

page!(ChipGroupPage {
    meta: meta(
        "ChipGroup",
        "Controls",
        "Wrapped group of chips with shared selection.",
        "Group",
        &[
            ("Android", "ChipGroup"),
            ("Material", "Chip set"),
            ("React", "chip list"),
            ("Qt", "flow of chips")
        ],
        false,
    ),
    props: &[PropSpec::Text {
        key: "chips",
        label: "Chips (csv)",
        default: "Rust,Cargo,GUI,Desktop",
    }],
    build: |p| {
        let mut g = ChipGroup::new().label("Tags");
        for label in csv(p, "chips") {
            g = g.chip(Chip::new(label).kind(ChipKind::Filter));
        }
        Box::new(g)
    },
    snippet: |p| {
        let chips = csv(p, "chips")
            .iter()
            .map(|c| format!("Chip::new({c:?})"))
            .collect::<Vec<_>>()
            .join(", ");
        format!("ChipGroup::new()\n    .chips([{chips}])")
    },
    poll: |w, out| {
        if let Some(g) = downcast_mut::<ChipGroup>(w) {
            if let Some((i, on)) = g.take_changed() {
                out.push(format!("chip {i} → {on}"));
            }
        }
    },
});

page!(RatingPage {
    meta: meta(
        "Rating",
        "Controls",
        "Star rating — click to set, half steps optional.",
        "Slider",
        &[
            ("React", "<Rating>"),
            ("Material", "Rating"),
            ("iOS", "star rating"),
            ("Qt", "custom")
        ],
        false,
    ),
    props: &[
        PropSpec::Int {
            key: "max",
            label: "Stars",
            min: 3,
            max: 10,
            default: 5
        },
        PropSpec::Float {
            key: "value",
            label: "Value",
            min: 0.0,
            max: 10.0,
            step: 0.5,
            default: 3.5
        },
        PropSpec::Bool {
            key: "half",
            label: "Half steps",
            default: true
        },
        PropSpec::Bool {
            key: "read_only",
            label: "Read-only",
            default: false
        },
    ],
    build: |p| {
        let mut r = Rating::new()
            .max(p.i64("max") as usize)
            .half_steps(p.bool("half"))
            .read_only(p.bool("read_only"))
            .label("Rating");
        r.set_value(p.f64("value") as f32);
        Box::new(r)
    },
    snippet: |p| format!(
        "Rating::new()\n    .max({})\n    .half_steps({})\n    .read_only({})",
        p.i64("max"),
        p.bool("half"),
        p.bool("read_only"),
    ),
    poll: |w, out| {
        if let Some(r) = downcast_mut::<Rating>(w) {
            if let Some(v) = r.take_changed() {
                out.push(format!("rating → {v}"));
            }
        }
    },
});

page!(CommandLinkPage {
    meta: meta(
        "CommandLink",
        "Controls",
        "Large command link with a note — wizard-style navigation.",
        "Link",
        &[
            ("Win32", "Command Link"),
            ("Qt", "QCommandLinkButton"),
            ("SwiftUI", "big row button"),
            ("React", "link card")
        ],
        false,
    ),
    props: &[
        PropSpec::Text {
            key: "label",
            label: "Label",
            default: "Create account"
        },
        PropSpec::Text {
            key: "note",
            label: "Note",
            default: "Set up a new profile"
        },
        PropSpec::Bool {
            key: "enabled",
            label: "Enabled",
            default: true
        },
    ],
    build: |p| Box::new(
        CommandLink::new(p.str("label"))
            .note(p.str("note"))
            .enabled(p.bool("enabled")),
    ),
    snippet: |p| format!(
        "CommandLink::new({:?}).note({:?})",
        p.str("label"),
        p.str("note"),
    ),
    poll: |w, out| {
        if downcast_mut::<CommandLink>(w).is_some_and(|c| c.take_activated()) {
            out.push("activated".to_string());
        }
    },
});

page!(LinkPage {
    meta: meta(
        "Link",
        "Controls",
        "Inline hyperlink text.",
        "Link",
        &[
            ("HTML", "<a>"),
            ("Qt", "link-styled label"),
            ("SwiftUI", "Link"),
            ("React", "<a href>")
        ],
        false,
    ),
    props: &[
        PropSpec::Text {
            key: "text",
            label: "Text",
            default: "Documentation"
        },
        PropSpec::Text {
            key: "href",
            label: "Href",
            default: "https://example.com"
        },
    ],
    build: |p| Box::new(Link::new(p.str("text")).target(p.str("href"))),
    snippet: |p| format!("Link::new({:?}).target({:?})", p.str("text"), p.str("href")),
    poll: |w, out| {
        if let Some(l) = downcast_mut::<Link>(w) {
            if let Some(target) = l.take_activated() {
                out.push(format!("open → {target}"));
            }
        }
    },
});

page!(FloatButtonPage {
    meta: meta(
        "FloatButton",
        "Controls",
        "Floating action button (FAB) — corner-pinned primary action.",
        "Button",
        &[
            ("Material", "FAB"),
            ("iOS", "floating action"),
            ("React", "<Fab>"),
            ("Qt", "overlay button")
        ],
        false,
    ),
    props: &[
        PropSpec::Text {
            key: "label",
            label: "Label",
            default: "+"
        },
        PropSpec::Bool {
            key: "enabled",
            label: "Enabled",
            default: true
        },
    ],
    build: |p| Box::new(FloatButton::new(p.str("label")).enabled(p.bool("enabled")),),
    snippet: |p| format!("FloatButton::new({:?})", p.str("label")),
    poll: |w, out| {
        if downcast_mut::<FloatButton>(w).is_some_and(|b| b.take_activated()) {
            out.push("activated".to_string());
        }
    },
});

page!(VolumePage {
    meta: meta(
        "Volume",
        "Controls",
        "Volume slider with mute toggle.",
        "Slider",
        &[
            ("Qt", "QVolumeSlider"),
            ("GTK", "GtkVolumeButton"),
            ("macOS", "volume slider"),
            ("React", "volume control")
        ],
        false,
    ),
    props: &[
        PropSpec::Float {
            key: "gain",
            label: "Gain",
            min: 0.0,
            max: 1.0,
            step: 0.05,
            default: 0.7
        },
        PropSpec::Bool {
            key: "muted",
            label: "Muted",
            default: false
        },
    ],
    build: |p| {
        let mut v = Volume::new();
        v.set_gain(p.f64("gain") as f32);
        v.set_muted(p.bool("muted"));
        Box::new(v)
    },
    snippet: |p| format!(
        "let mut v = Volume::new();\nv.set_gain({:?});\nv.set_muted({});",
        p.f64("gain") as f32,
        p.bool("muted"),
    ),
    poll: |w, out| {
        if let Some(v) = downcast_mut::<Volume>(w) {
            if let Some(g) = v.take_changed() {
                out.push(format!("gain → {g:.2}"));
            }
            if let Some(m) = v.take_muted() {
                out.push(format!("muted → {m}"));
            }
        }
    },
});

page!(DialPage {
    meta: meta(
        "Dial",
        "Controls",
        "Rotary knob — drag up/down to change value.",
        "Slider",
        &[
            ("Audio", "knob"),
            ("Qt", "QDial"),
            ("JUCE", "Slider(rotary)"),
            ("React", "knob")
        ],
        false,
    ),
    props: &[PropSpec::Float {
        key: "value",
        label: "Value",
        min: 0.0,
        max: 1.0,
        step: 0.05,
        default: 0.35
    },],
    build: |p| {
        let mut d = Dial::new().range(0.0, 1.0);
        d.set_value(p.f64("value"));
        Box::new(d)
    },
    snippet: |p| format!(
        "let mut d = Dial::new().range(0.0, 1.0);\nd.set_value({});",
        p.f64("value"),
    ),
    poll: |w, out| {
        if let Some(d) = downcast_mut::<Dial>(w) {
            if let Some(v) = d.take_changed() {
                out.push(format!("value → {v:.2}"));
            }
        }
    },
});

page!(WheelPickerPage {
    meta: meta(
        "WheelPicker",
        "Controls",
        "iOS-style spinning-wheel item picker.",
        "ListBox",
        &[
            ("iOS", "UIPickerView"),
            ("Android", "NumberPicker"),
            ("Qt", "QListView wheel"),
            ("SwiftUI", "Picker(wheel)")
        ],
        false,
    ),
    props: &[PropSpec::Text {
        key: "items",
        label: "Items (csv)",
        default: "Mercury,Venus,Earth,Mars",
    }],
    build: |p| Box::new(WheelPicker::new().items(csv(p, "items"))),
    snippet: |p| {
        let items = csv(p, "items")
            .iter()
            .map(|i| format!("{i:?}"))
            .collect::<Vec<_>>()
            .join(", ");
        format!("WheelPicker::new().items([{items}])")
    },
    poll: |w, out| {
        if let Some(wp) = downcast_mut::<WheelPicker>(w) {
            if let Some(i) = wp.take_selected() {
                out.push(format!("selected → {i}"));
            }
        }
    },
});

page!(JoystickPage {
    meta: meta(
        "Joystick",
        "Controls",
        "Two-axis analog stick — drag the knob.",
        "GenericContainer",
        &[
            ("Game", "thumbstick"),
            ("Qt", "custom"),
            ("iOS", "virtual stick"),
            ("React", "joystick")
        ],
        false,
    ),
    props: &[
        PropSpec::Bool {
            key: "spring",
            label: "Spring return",
            default: true
        },
        PropSpec::Float {
            key: "dead_zone",
            label: "Dead zone",
            min: 0.0,
            max: 0.5,
            step: 0.05,
            default: 0.1
        },
    ],
    build: |p| {
        let mut j = Joystick::new().spring(p.bool("spring"));
        j = j.dead_zone(p.f64("dead_zone") as f32);
        Box::new(j)
    },
    snippet: |p| format!(
        "Joystick::new()\n    .spring({})\n    .dead_zone({:?})",
        p.bool("spring"),
        p.f64("dead_zone") as f32,
    ),
    poll: |w, out| {
        if let Some(j) = downcast_mut::<Joystick>(w) {
            if j.take_changed() {
                let (x, y) = j.value_xy();
                out.push(format!("xy → ({x:.2}, {y:.2})"));
            }
        }
    },
});

page!(KeypadPage {
    meta: meta(
        "Keypad",
        "Controls",
        "Numeric 0–9 pad with confirm/clear.",
        "Grid",
        &[
            ("iOS", "numeric keypad"),
            ("POS", "PIN pad"),
            ("Qt", "QKeypad"),
            ("React", "keypad")
        ],
        false,
    ),
    props: &[PropSpec::Bool {
        key: "enabled",
        label: "Enabled",
        default: true
    }],
    build: |p| Box::new(Keypad::new().enabled(p.bool("enabled"))),
    snippet: |p| format!("Keypad::new().enabled({})", p.bool("enabled")),
    poll: |w, out| {
        if let Some(k) = downcast_mut::<Keypad>(w) {
            if let Some(key) = k.take_pressed() {
                out.push(format!("key → {key}"));
            }
        }
    },
});

page!(XyPadPage {
    meta: meta(
        "XYPad",
        "Controls",
        "Two-dimensional value pad — drag maps both axes.",
        "Slider",
        &[
            ("Audio", "XY controller"),
            ("JUCE", "XYPad"),
            ("Korg", "Kaoss pad"),
            ("React", "xy-pad")
        ],
        false,
    ),
    props: &[
        PropSpec::Text {
            key: "x",
            label: "X label",
            default: "Cutoff"
        },
        PropSpec::Text {
            key: "y",
            label: "Y label",
            default: "Resonance"
        },
    ],
    build: |p| {
        let mut pad = XYPad::new();
        pad = pad.labels(p.str("x"), p.str("y"));
        Box::new(pad)
    },
    snippet: |p| format!("XYPad::new().labels({:?}, {:?})", p.str("x"), p.str("y"),),
    poll: |w, out| {
        if let Some(pad) = downcast_mut::<XYPad>(w) {
            if pad.take_changed() {
                let (x, y) = pad.value_xy();
                out.push(format!("xy → ({x:.2}, {y:.2})"));
            }
        }
    },
});

page!(CrosshairPage {
    meta: meta(
        "Crosshair",
        "Controls",
        "Crosshair position picker with readout.",
        "GenericContainer",
        &[
            ("CAD", "crosshair"),
            ("Qt", "custom"),
            ("React", "xy picker"),
            ("JUCE", "crosshair")
        ],
        false,
    ),
    props: &[PropSpec::Bool {
        key: "readout",
        label: "Readout",
        default: true
    }],
    build: |p| {
        let mut c = Crosshair::new();
        c.show_readout = p.bool("readout");
        Box::new(c)
    },
    snippet: |p| format!("Crosshair::new() /* show_readout={} */", p.bool("readout")),
    poll: |w, out| {
        if let Some(c) = downcast_mut::<Crosshair>(w) {
            if let Some(pos) = c.take_moved() {
                out.push(format!("pos → ({:.0}, {:.0})", pos.x, pos.y));
            }
        }
    },
});

page!(TunerPage {
    meta: meta(
        "Tuner",
        "Controls",
        "Chromatic tuner — needle + cents readout.",
        "Slider",
        &[
            ("Music", "tuner"),
            ("Qt", "custom"),
            ("Korg", "pitchblack"),
            ("React", "tuner")
        ],
        false,
    ),
    props: &[PropSpec::Float {
        key: "cents",
        label: "Cents",
        min: -50.0,
        max: 50.0,
        step: 1.0,
        default: -12.0,
    }],
    build: |p| {
        let mut t = Tuner::new();
        t.set_pitch("A", p.f64("cents") as f32);
        Box::new(t)
    },
    snippet: |p| format!(
        "let mut t = Tuner::new();\nt.set_pitch(\"A\", {:?});",
        p.f64("cents") as f32,
    ),
    poll: |w, out| {
        if let Some(t) = downcast_mut::<Tuner>(w) {
            if t.take_steady() {
                out.push("steady".to_string());
            }
        }
    },
});

page!(PadGridPage {
    meta: meta(
        "PadGrid",
        "Controls",
        "MPC-style trigger pad matrix.",
        "Grid",
        &[
            ("Akai", "MPC pads"),
            ("Novation", "Launchpad"),
            ("Qt", "grid of pads"),
            ("React", "pad grid")
        ],
        false,
    ),
    props: &[
        PropSpec::Int {
            key: "cols",
            label: "Cols",
            min: 2,
            max: 8,
            default: 4
        },
        PropSpec::Int {
            key: "rows",
            label: "Rows",
            min: 2,
            max: 8,
            default: 4
        },
    ],
    build: |p| Box::new(PadGrid::new(p.i64("cols") as usize, p.i64("rows") as usize)),
    snippet: |p| format!("PadGrid::new({}, {})", p.i64("cols"), p.i64("rows")),
    poll: |w, out| {
        if let Some(g) = downcast_mut::<PadGrid>(w) {
            if let Some(i) = g.take_triggered() {
                out.push(format!("pad {i}"));
            }
        }
    },
});

page!(ColorButtonPage {
    meta: meta(
        "ColorButton",
        "Controls",
        "Color swatch button — opens a color picker.",
        "Button",
        &[
            ("GTK", "GtkColorButton"),
            ("Qt", "color button"),
            ("macOS", "NSColorWell"),
            ("React", "color swatch")
        ],
        true,
    ),
    props: &[PropSpec::Text {
        key: "title",
        label: "Title",
        default: "Accent"
    }],
    build: |p| Box::new(ColorButton::new([80, 140, 255, 255]).title(p.str("title"))),
    snippet: |p| format!(
        "ColorButton::new([80, 140, 255, 255]).title({:?})",
        p.str("title"),
    ),
    poll: |w, out| {
        if downcast_mut::<ColorButton>(w).is_some_and(|b| b.take_activated()) {
            out.push("activated → picker".to_string());
        }
    },
});

page!(ColorPalettePage {
    meta: meta(
        "ColorPalette",
        "Controls",
        "Predefined swatch grid — click to select.",
        "Grid",
        &[
            ("GTK", "palette grid"),
            ("Qt", "QColorDialog swatches"),
            ("React", "swatch picker"),
            ("macOS", "color palette")
        ],
        false,
    ),
    props: &[],
    build: |_p| Box::new(ColorPalette::new()),
    snippet: |_p| "ColorPalette::new()".to_string(),
    poll: |w, out| {
        if let Some(cp) = downcast_mut::<ColorPalette>(w) {
            if let Some(sel) = cp.take_selected() {
                out.push(format!("swatch {sel:?}"));
            }
        }
    },
});

page!(ColorPickerPage {
    meta: meta(
        "ColorPicker",
        "Controls",
        "HSV picker with optional alpha channel.",
        "Dialog",
        &[
            ("Qt", "QColorDialog"),
            ("GTK", "GtkColorChooser"),
            ("macOS", "NSColorPanel"),
            ("React", "<input type=color>")
        ],
        true,
    ),
    props: &[PropSpec::Bool {
        key: "alpha",
        label: "Alpha channel",
        default: true
    }],
    build: |p| {
        let mut c = ColorPicker::new().with_alpha(p.bool("alpha"));
        c.open();
        Box::new(c)
    },
    snippet: |p| format!("ColorPicker::new().with_alpha({})", p.bool("alpha")),
    poll: |w, out| {
        if let Some(c) = downcast_mut::<ColorPicker>(w) {
            if let Some(col) = c.take_selected() {
                out.push(format!("color → {col:?}"));
            }
        }
    },
});

page!(ColorWheelPage {
    meta: meta(
        "ColorWheel",
        "Controls",
        "Hue/saturation wheel + brightness.",
        "Slider",
        &[
            ("Qt", "hue wheel"),
            ("Adobe", "color wheel"),
            ("macOS", "color wheel"),
            ("React", "color wheel")
        ],
        false,
    ),
    props: &[PropSpec::Float {
        key: "hue",
        label: "Hue (deg)",
        min: 0.0,
        max: 360.0,
        step: 1.0,
        default: 210.0,
    }],
    build: |p| Box::new(ColorWheel::new().hue(p.f64("hue") as f32)),
    snippet: |p| format!(
        "let mut w = ColorWheel::new();\nw.set_hue({:?});",
        p.f64("hue") as f32,
    ),
    poll: |w, out| {
        if let Some(wheel) = downcast_mut::<ColorWheel>(w) {
            if wheel.take_changed() {
                out.push(format!("hue → {:.0}°", wheel.hue_value()));
            }
        }
    },
});

page!(AlphaSliderPage {
    meta: meta(
        "AlphaSlider",
        "Controls",
        "Opacity slider over a checkerboard.",
        "Slider",
        &[
            ("Qt", "alpha slider"),
            ("Adobe", "opacity slider"),
            ("React", "alpha slider"),
            ("GTK", "alpha channel")
        ],
        false,
    ),
    props: &[PropSpec::Float {
        key: "alpha",
        label: "Alpha",
        min: 0.0,
        max: 1.0,
        step: 0.05,
        default: 0.8,
    }],
    build: |p| {
        let mut s = AlphaSlider::new();
        s.set_alpha(p.f64("alpha") as f32);
        Box::new(s)
    },
    snippet: |p| format!(
        "let mut s = AlphaSlider::new();\ns.set_alpha({:?});",
        p.f64("alpha") as f32,
    ),
    poll: |w, out| {
        if let Some(s) = downcast_mut::<AlphaSlider>(w) {
            if let Some(a) = s.take_changed() {
                out.push(format!("alpha → {a:.2}"));
            }
        }
    },
});

page!(HueSliderPage {
    meta: meta(
        "HueSlider",
        "Controls",
        "Rainbow hue slider.",
        "Slider",
        &[
            ("Qt", "hue slider"),
            ("Adobe", "hue track"),
            ("React", "hue slider"),
            ("macOS", "hue bar")
        ],
        false,
    ),
    props: &[PropSpec::Float {
        key: "hue",
        label: "Hue",
        min: 0.0,
        max: 360.0,
        step: 1.0,
        default: 40.0,
    }],
    build: |p| {
        let mut s = HueSlider::new();
        s.set_hue(p.f64("hue") as f32);
        Box::new(s)
    },
    snippet: |p| format!(
        "let mut s = HueSlider::new();\ns.set_hue({:?});",
        p.f64("hue") as f32,
    ),
    poll: |w, out| {
        if let Some(s) = downcast_mut::<HueSlider>(w) {
            if let Some(h) = s.take_changed() {
                out.push(format!("hue → {h:.0}°"));
            }
        }
    },
});

page!(FileChooserButtonPage {
    meta: meta(
        "FileChooserButton",
        "Controls",
        "Button that launches a file dialog.",
        "Button",
        &[
            ("GTK", "GtkFileChooserButton"),
            ("Qt", "QFileDialog button"),
            ("HTML", "<input type=file>"),
            ("React", "file button")
        ],
        false,
    ),
    props: &[PropSpec::Text {
        key: "placeholder",
        label: "Placeholder",
        default: "Choose file…",
    }],
    build: |p| Box::new(FileChooserButton::new().placeholder(p.str("placeholder"))),
    snippet: |p| format!(
        "FileChooserButton::new().placeholder({:?})",
        p.str("placeholder"),
    ),
    poll: |w, out| {
        if downcast_mut::<FileChooserButton>(w).is_some_and(|b| b.take_activated()) {
            out.push("activated → file dialog".to_string());
        }
    },
});

page!(FontButtonPage {
    meta: meta(
        "FontButton",
        "Controls",
        "Button showing current font, opens a font picker.",
        "Button",
        &[
            ("GTK", "GtkFontButton"),
            ("Qt", "QFontDialog button"),
            ("macOS", "NSFontPanel"),
            ("React", "font picker")
        ],
        false,
    ),
    props: &[
        PropSpec::Text {
            key: "family",
            label: "Family",
            default: "Inter"
        },
        PropSpec::Float {
            key: "size",
            label: "Size",
            min: 8.0,
            max: 48.0,
            step: 1.0,
            default: 14.0
        },
    ],
    build: |p| Box::new(FontButton::new(p.str("family"), p.f64("size") as f32)),
    snippet: |p| format!(
        "FontButton::new({:?}, {:?})",
        p.str("family"),
        p.f64("size") as f32,
    ),
    poll: |w, out| {
        if downcast_mut::<FontButton>(w).is_some_and(|b| b.take_activated()) {
            out.push("activated → font picker".to_string());
        }
    },
});

/// All Controls pages, in rail order.
pub(super) fn pages() -> Vec<Box<dyn Page>> {
    vec![
        Box::new(ButtonPage),
        Box::new(CheckBoxPage),
        Box::new(SwitchPage),
        Box::new(ToggleButtonPage),
        Box::new(SliderPage),
        Box::new(RangeSliderPage),
        Box::new(SpinBoxPage),
        Box::new(SegmentedPage),
        Box::new(DropdownPage),
        Box::new(RadioGroupPage),
        Box::new(MenuButtonPage),
        Box::new(SplitButtonPage),
        Box::new(ChipPage),
        Box::new(ChipGroupPage),
        Box::new(RatingPage),
        Box::new(CommandLinkPage),
        Box::new(LinkPage),
        Box::new(FloatButtonPage),
        Box::new(VolumePage),
        Box::new(DialPage),
        Box::new(WheelPickerPage),
        Box::new(JoystickPage),
        Box::new(KeypadPage),
        Box::new(XyPadPage),
        Box::new(CrosshairPage),
        Box::new(TunerPage),
        Box::new(PadGridPage),
        Box::new(ColorButtonPage),
        Box::new(ColorPalettePage),
        Box::new(ColorPickerPage),
        Box::new(ColorWheelPage),
        Box::new(AlphaSliderPage),
        Box::new(HueSliderPage),
        Box::new(FileChooserButtonPage),
        Box::new(FontButtonPage),
        Box::new(ZoomControlsPage),
    ]
}

page!(ZoomControlsPage {
    meta: meta(
        "ZoomControls",
        "Controls",
        "Zoom button cluster — in/out, fit, reset, readout.",
        "ToolBar",
        &[
            ("Maps", "zoom +/-"),
            ("CAD", "zoom tools"),
            ("React", "zoom controls"),
            ("Qt", "custom")
        ],
        false,
    ),
    props: &[
        PropSpec::Float {
            key: "zoom",
            label: "Zoom",
            min: 0.1,
            max: 8.0,
            step: 0.1,
            default: 1.0
        },
        PropSpec::Bool {
            key: "horizontal",
            label: "Horizontal",
            default: true
        },
    ],
    build: |p| Box::new(
        ZoomControls::new()
            .label("Zoom")
            .zoom(p.f64("zoom") as f32)
            .readout(true)
            .fit(true)
            .reset(true)
            .horizontal(p.bool("horizontal")),
    ),
    snippet: |p| format!(
        "ZoomControls::new().zoom({:?}).readout(true).horizontal({})",
        p.f64("zoom") as f32,
        p.bool("horizontal"),
    ),
    poll: |w, out| {
        if let Some(z) = downcast_mut::<ZoomControls>(w) {
            if let Some(a) = z.take_action() {
                out.push(format!("{a:?}"));
            }
        }
    },
});
