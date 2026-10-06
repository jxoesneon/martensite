//! Controls family — buttons, toggles, sliders, pickers, and
//! selection widgets.

use martensite::widgets::alpha_slider::AlphaSlider;
use martensite::widgets::button::Button;
use martensite::widgets::checkbox::{CheckBox, CheckState};
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
use crate::pages::{csv, downcast_mut, meta, page, SnipProp};

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
            key: "icon_only",
            label: "Icon Only",
            default: false
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
        {
            let mut __w = b;
            if !p.str("icon_d").is_empty() {
                __w = __w.icon_d(p.str("icon_d"));
            }
            if !p.str("icon_named").is_empty() {
                __w = __w.icon_named(p.str("icon_named"));
            }
            if p.bool("icon_only") {
                __w = __w.icon_only(p.bool("icon_only"));
                // Icon-only hides the label only when an icon paints —
                // stage one so the toggle demonstrates itself.
                if p.str("icon_d").is_empty() && p.str("icon_named").is_empty() {
                    __w = __w.icon_named("media.play");
                }
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = {
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
        };
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("icon_d", ".icon_d", SnipProp::Text("")),
                ("icon_named", ".icon_named", SnipProp::Text("")),
                ("icon_only", ".icon_only", SnipProp::Bool(false)),
            ],
        ));
        __s
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
    build: |p| {
        let mut cb = CheckBox::new(p.str("label"))
            .checked(p.bool("checked"))
            .tristate(p.bool("tristate"))
            .enabled(p.bool("enabled"));
        // The flag alone changes only click cycling — stage the third
        // state so tristate demonstrates itself on the raster.
        if p.bool("tristate") && !p.bool("checked") {
            cb.set_state(CheckState::Indeterminate);
        }
        Box::new(cb)
    },
    snippet: |p| {
        let mut s = format!("CheckBox::new({:?})", p.str("label"));
        if p.bool("checked") {
            s.push_str("\n    .checked(true)");
        }
        if p.bool("tristate") {
            s.push_str("\n    .tristate(true)");
            if !p.bool("checked") {
                s.push_str(" /* staged CheckState::Indeterminate */");
            }
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
        let mut __w = Switch::new(p.str("label"))
            .on(p.bool("on"))
            .enabled(p.bool("enabled"));
        if !p.str("a11y_label").is_empty() {
            __w = __w.a11y_label(p.str("a11y_label"));
        }
        Box::new(__w)
    },
    snippet: |p| {
        let mut __s = {
            let mut s = format!("Switch::new({:?}).on({})", p.str("label"), p.bool("on"));
            if !p.bool("enabled") {
                s.push_str("\n    .enabled(false)");
            }
            s
        };
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[("a11y_label", ".a11y_label", SnipProp::Text(""))],
        ));
        __s
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
        PropSpec::Text {
            key: "tooltip",
            label: "Tooltip",
            default: ""
        },
    ],
    build: |p| {
        let mut __w = ToggleButton::new(p.str("label"))
            .pressed(p.bool("pressed"))
            .enabled(p.bool("enabled"));
        if !p.str("tooltip").is_empty() {
            __w = __w.tooltip(p.str("tooltip"));
        }
        Box::new(__w)
    },
    snippet: |p| {
        let mut __s = format!(
            "ToggleButton::new({:?})\n    .pressed({})\n    .enabled({})",
            p.str("label"),
            p.bool("pressed"),
            p.bool("enabled"),
        );
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[("tooltip", ".tooltip", SnipProp::Text(""))],
        ));
        __s
    },
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
        PropSpec::Float {
            key: "step",
            label: "Step",
            min: -8.5,
            max: 100.0,
            step: 1.0,
            default: 1.0
        },
        PropSpec::Float {
            key: "with_page_step",
            label: "With Page Step",
            min: -10.0,
            max: 100.0,
            step: 1.0,
            default: 0.0
        },
        PropSpec::Choice {
            key: "orientation",
            label: "Orientation",
            options: &["Horizontal", "Vertical"],
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
        let (min, max) = (
            p.f64("min").min(p.f64("max")),
            p.f64("max").max(p.f64("min")),
        );
        let v = p.f64("value").clamp(min, max);
        {
            let mut __w = Slider::new(min, max).with_value(v);
            __w = __w.enabled(p.bool("enabled"));
            if !p.str("a11y_label").is_empty() {
                __w = __w.label(p.str("a11y_label"));
            }
            if p.f64("step") != 1.0 {
                __w = __w.step(p.f64("step"));
            }
            if p.f64("with_page_step") != 0.0 {
                __w = __w.with_page_step(p.f64("with_page_step"));
            }
            if p.choice("orientation") != 0 {
                __w = __w.orientation(match p.choice("orientation") {
                    0 => martensite::widgets::slider::SliderOrientation::Horizontal,
                    1 => martensite::widgets::slider::SliderOrientation::Vertical,
                    _ => martensite::widgets::slider::SliderOrientation::Horizontal,
                });
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = format!(
            "Slider::new({}, {}).with_value({})",
            p.f64("min"),
            p.f64("max"),
            p.f64("value"),
        );
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("step", ".step", SnipProp::Float(1.0)),
                ("with_page_step", ".with_page_step", SnipProp::Float(0.0)),
                (
                    "orientation",
                    ".orientation",
                    SnipProp::Choice(&[
                        "martensite::widgets::slider::SliderOrientation::Horizontal",
                        "martensite::widgets::slider::SliderOrientation::Vertical",
                    ]),
                ),
                ("enabled", ".enabled", SnipProp::Bool(true)),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        __s
    },
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
        PropSpec::Float {
            key: "step",
            label: "Step",
            min: -8.5,
            max: 100.0,
            step: 1.0,
            default: 1.0
        },
        PropSpec::Float {
            key: "with_page_step",
            label: "With Page Step",
            min: -10.0,
            max: 100.0,
            step: 1.0,
            default: 0.0
        },
        PropSpec::Choice {
            key: "orientation",
            label: "Orientation",
            options: &["Horizontal", "Vertical"],
            default: 0
        },
        PropSpec::Text {
            key: "with_range",
            label: "With Range (csv)",
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
        PropSpec::Text {
            key: "a11y_label",
            label: "A11y label",
            default: ""
        },
    ],
    build: |p| {
        let mut s = RangeSlider::new(0.0, 100.0);
        s.set_low(p.f64("low"));
        s.set_high(p.f64("high").max(p.f64("low")));
        {
            let mut __w = s;
            __w = __w.enabled(p.bool("enabled"));
            if !p.str("a11y_label").is_empty() {
                __w = __w.label(p.str("a11y_label"));
            }
            if p.f64("step") != 1.0 {
                __w = __w.step(p.f64("step"));
            }
            if p.f64("with_page_step") != 0.0 {
                __w = __w.with_page_step(p.f64("with_page_step"));
            }
            if p.choice("orientation") != 0 {
                __w = __w.orientation(match p.choice("orientation") {
                    0 => martensite::widgets::slider::SliderOrientation::Horizontal,
                    1 => martensite::widgets::slider::SliderOrientation::Vertical,
                    _ => martensite::widgets::slider::SliderOrientation::Horizontal,
                });
            }
            if let Some(v) = crate::pages::parse_pair(p.str("with_range")) {
                __w = __w.with_range(v.0, v.1);
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = format!(
            "let mut s = RangeSlider::new(0.0, 100.0);\ns.set_low({});\ns.set_high({});",
            p.f64("low"),
            p.f64("high"),
        );
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("step", ".step", SnipProp::Float(1.0)),
                ("with_page_step", ".with_page_step", SnipProp::Float(0.0)),
                (
                    "orientation",
                    ".orientation",
                    SnipProp::Choice(&[
                        "martensite::widgets::slider::SliderOrientation::Horizontal",
                        "martensite::widgets::slider::SliderOrientation::Vertical",
                    ]),
                ),
                ("enabled", ".enabled", SnipProp::Bool(true)),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        __s.push_str(&crate::pages::snip_textmap(
            p,
            "with_range",
            ".with_range",
            "",
            crate::pages::expr_pair,
        ));
        __s
    },
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
        PropSpec::Float {
            key: "step",
            label: "Step",
            min: -8.5,
            max: 100.0,
            step: 1.0,
            default: 1.0
        },
        PropSpec::Float {
            key: "with_page_step",
            label: "With Page Step",
            min: -10.0,
            max: 100.0,
            step: 1.0,
            default: 0.0
        },
        PropSpec::Int {
            key: "decimals",
            label: "Decimals",
            min: 0,
            max: 100,
            default: 0
        },
        PropSpec::Text {
            key: "prefix",
            label: "Prefix",
            default: ""
        },
        PropSpec::Bool {
            key: "editable",
            label: "Editable",
            default: true
        },
        PropSpec::Bool {
            key: "wrap",
            label: "Wrap",
            default: false
        },
        PropSpec::Choice {
            key: "sanitize",
            label: "Sanitize",
            options: &["Aggressive", "Baseline", "Raw"],
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
        let mut s = SpinBox::new()
            .range(p.i64("min") as f64, p.i64("max") as f64)
            .with_value(p.i64("value") as f64);
        let sfx = p.str("suffix");
        if !sfx.is_empty() {
            s = s.suffix(sfx);
        }
        {
            let mut __w = s;
            __w = __w.enabled(p.bool("enabled"));
            if !p.str("a11y_label").is_empty() {
                __w = __w.label(p.str("a11y_label"));
            }
            if p.f64("step") != 1.0 {
                __w = __w.step(p.f64("step"));
            }
            if p.f64("with_page_step") != 0.0 {
                __w = __w.with_page_step(p.f64("with_page_step"));
            }
            if p.i64("decimals") != 0 {
                __w = __w.decimals(p.i64("decimals") as usize);
            }
            if !p.str("prefix").is_empty() {
                __w = __w.prefix(p.str("prefix"));
            }
            if !p.bool("editable") {
                __w = __w.editable(p.bool("editable"));
            }
            if p.bool("wrap") {
                __w = __w.wrap(p.bool("wrap"));
            }
            __w.set_sanitizer(crate::pages::sanitize_cfg(p));
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = format!(
            "SpinBox::new()\n    .range({}, {})\n    .with_value({})",
            p.i64("min"),
            p.i64("max"),
            p.i64("value"),
        );
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("step", ".step", SnipProp::Float(1.0)),
                ("with_page_step", ".with_page_step", SnipProp::Float(0.0)),
                ("decimals", ".decimals", SnipProp::Int(0)),
                ("prefix", ".prefix", SnipProp::Text("")),
                ("editable", ".editable", SnipProp::Bool(true)),
                ("wrap", ".wrap", SnipProp::Bool(false)),
                ("enabled", ".enabled", SnipProp::Bool(true)),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        __s.push_str(crate::pages::sanitize_snippet(p));
        __s
    },
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
    props: &[
        PropSpec::Text {
            key: "options",
            label: "Options (csv)",
            default: "Day,Week,Month",
        },
        PropSpec::Text {
            key: "option",
            label: "Option",
            default: ""
        },
        PropSpec::Choice {
            key: "direction",
            label: "Direction",
            options: &["Row", "Column"],
            default: 0
        },
        PropSpec::Text {
            key: "option_with_badge_label",
            label: "Option With Badge Label",
            default: ""
        },
        PropSpec::Text {
            key: "option_with_badge_badge",
            label: "Option With Badge Badge",
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
        PropSpec::Text {
            key: "a11y_label",
            label: "A11y label",
            default: ""
        },
    ],
    build: |p| {
        let mut __w = Segmented::new().options(csv(p, "options")).selected(0);
        __w = __w.enabled(p.bool("enabled"));
        if !p.str("a11y_label").is_empty() {
            __w = __w.label(p.str("a11y_label"));
        }
        if !p.str("option").is_empty() {
            __w = __w.option(p.str("option"));
        }
        if p.choice("direction") != 0 {
            __w = __w.direction(match p.choice("direction") {
                0 => martensite::widgets::flex::FlexDirection::Row,
                1 => martensite::widgets::flex::FlexDirection::Column,
                _ => martensite::widgets::flex::FlexDirection::Row,
            });
        }
        if !p.str("option_with_badge_label").is_empty()
            || !p.str("option_with_badge_badge").is_empty()
        {
            __w = __w.option_with_badge(
                p.str("option_with_badge_label"),
                martensite::widgets::badge::BadgeSpec::new(p.str("option_with_badge_badge")),
            );
        }
        Box::new(__w)
    },
    snippet: |p| {
        let mut __s = {
            let opts = csv(p, "options")
                .iter()
                .map(|o| format!("{o:?}"))
                .collect::<Vec<_>>()
                .join(", ");
            format!("Segmented::new().options([{opts}]).selected(0)")
        };
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("option", ".option", SnipProp::Text("")),
                (
                    "direction",
                    ".direction",
                    SnipProp::Choice(&[
                        "martensite::widgets::flex::FlexDirection::Row",
                        "martensite::widgets::flex::FlexDirection::Column",
                    ]),
                ),
                ("enabled", ".enabled", SnipProp::Bool(true)),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        if !p.str("option_with_badge_label").is_empty()
            || !p.str("option_with_badge_badge").is_empty()
        {
            __s.push_str(&format!(
                "\n    .option_with_badge({:?}, BadgeSpec::new({:?}))",
                p.str("option_with_badge_label"),
                p.str("option_with_badge_badge")
            ));
        }
        __s
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
    props: &[
        PropSpec::Text {
            key: "options",
            label: "Options (csv)",
            default: "Light,Dark,System",
        },
        PropSpec::Text {
            key: "placeholder",
            label: "Placeholder",
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
        PropSpec::Bool {
            key: "loading",
            label: "Loading",
            default: false
        },
        PropSpec::Text {
            key: "a11y_label",
            label: "A11y label",
            default: ""
        },
    ],
    build: |p| {
        let mut __w = Dropdown::new(csv(p, "options"));
        __w = __w.enabled(p.bool("enabled"));
        __w = __w.loading(p.bool("loading"));
        if !p.str("a11y_label").is_empty() {
            __w = __w.label(p.str("a11y_label"));
        }
        if !p.str("placeholder").is_empty() {
            __w = __w.placeholder(p.str("placeholder"));
            // The face only paints the placeholder while nothing is
            // selected — clear the selection so the text shows.
            __w.deselect();
        }
        Box::new(__w)
    },
    snippet: |p| {
        let mut __s = {
            let opts = csv(p, "options")
                .iter()
                .map(|o| format!("{o:?}"))
                .collect::<Vec<_>>()
                .join(", ");
            format!("Dropdown::new([{opts}])")
        };
        if !p.str("placeholder").is_empty() {
            __s.push_str(" /* deselected — placeholder paints */");
        }
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("placeholder", ".placeholder", SnipProp::Text("")),
                ("enabled", ".enabled", SnipProp::Bool(true)),
                ("loading", ".loading", SnipProp::Bool(false)),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        __s
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
    props: &[
        PropSpec::Text {
            key: "options",
            label: "Options (csv)",
            default: "Small,Medium,Large",
        },
        PropSpec::Choice {
            key: "direction",
            label: "Direction",
            options: &["Column", "Row"],
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
        let mut __w = RadioGroup::new(csv(p, "options"));
        __w = __w.enabled(p.bool("enabled"));
        if !p.str("a11y_label").is_empty() {
            __w = __w.label(p.str("a11y_label"));
        }
        if p.choice("direction") != 0 {
            __w = __w.direction(match p.choice("direction") {
                1 => martensite::widgets::flex::FlexDirection::Row,
                _ => martensite::widgets::flex::FlexDirection::Column,
            });
        }
        Box::new(__w)
    },
    snippet: |p| {
        let mut __s = {
            let opts = csv(p, "options")
                .iter()
                .map(|o| format!("{o:?}"))
                .collect::<Vec<_>>()
                .join(", ");
            format!("RadioGroup::new([{opts}])")
        };
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                (
                    "direction",
                    ".direction",
                    SnipProp::Choice(&[
                        "martensite::widgets::flex::FlexDirection::Column",
                        "martensite::widgets::flex::FlexDirection::Row",
                    ]),
                ),
                ("enabled", ".enabled", SnipProp::Bool(true)),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        __s
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
    props: &[
        PropSpec::Text {
            key: "label",
            label: "Label",
            default: "Save",
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
        let mut __w = SplitButton::new(p.str("label"));
        __w = __w.enabled(p.bool("enabled"));
        if !p.str("a11y_label").is_empty() {
            __w = __w.label(p.str("a11y_label"));
        }
        Box::new(__w)
    },
    snippet: |p| {
        let mut __s = format!("SplitButton::new({:?})", p.str("label"));
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
            default: true
        },
        PropSpec::Bool {
            key: "deletable",
            label: "Deletable",
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
        let kind = match p.choice("kind") {
            0 => ChipKind::Assist,
            2 => ChipKind::Input,
            3 => ChipKind::Suggestion,
            _ => ChipKind::Filter,
        };
        {
            let mut __w = Chip::new(p.str("label"))
                .kind(kind)
                .selected(p.bool("selected"))
                .deletable(p.bool("deletable"));
            __w = __w.enabled(p.bool("enabled"));
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = format!(
            "Chip::new({:?})\n    .kind(ChipKind::{:?})\n    .selected({})\n    .deletable({})",
            p.str("label"),
            ["Assist", "Filter", "Input", "Suggestion"][p.choice("kind")],
            p.bool("selected"),
            p.bool("deletable"),
        );
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[("enabled", ".enabled", SnipProp::Bool(true))],
        ));
        __s
    },
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
    props: &[
        PropSpec::Text {
            key: "chips",
            label: "Chips (csv)",
            default: "Rust,Cargo,GUI,Desktop",
        },
        PropSpec::Float {
            key: "gap",
            label: "Gap",
            min: 0.0,
            max: 64.0,
            step: 0.5,
            default: 0.0
        },
        PropSpec::Choice {
            key: "selection",
            label: "Selection",
            options: &["Multiple", "Single", "None"],
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
    ],
    build: |p| {
        let mut g = ChipGroup::new().label("Tags");
        for label in csv(p, "chips") {
            g = g.chip(Chip::new(label).kind(ChipKind::Filter));
        }
        {
            let mut __w = g;
            __w = __w.enabled(p.bool("enabled"));
            if p.f64("gap") != 0.0 {
                __w = __w.gap(p.f64("gap") as f32);
            }
            if p.choice("selection") != 0 {
                __w = __w.selection(match p.choice("selection") {
                    1 => martensite::widgets::chip_group::ChipSelection::Single,
                    2 => martensite::widgets::chip_group::ChipSelection::None,
                    _ => martensite::widgets::chip_group::ChipSelection::Multiple,
                });
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = {
            let chips = csv(p, "chips")
                .iter()
                .map(|c| format!("Chip::new({c:?})"))
                .collect::<Vec<_>>()
                .join(", ");
            format!("ChipGroup::new()\n    .chips([{chips}])")
        };
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("gap", ".gap", SnipProp::Float(0.0)),
                (
                    "selection",
                    ".selection",
                    SnipProp::Choice(&[
                        "martensite::widgets::chip_group::ChipSelection::Multiple",
                        "martensite::widgets::chip_group::ChipSelection::Single",
                        "martensite::widgets::chip_group::ChipSelection::None",
                    ]),
                ),
                ("enabled", ".enabled", SnipProp::Bool(true)),
            ],
        ));
        __s
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
        PropSpec::Bool {
            key: "allow_clear",
            label: "Allow Clear",
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
    ],
    build: |p| {
        let mut r = Rating::new()
            .max(p.i64("max") as usize)
            .half_steps(p.bool("half"))
            .read_only(p.bool("read_only"))
            .label("Rating");
        r.set_value(p.f64("value") as f32);
        {
            let mut __w = r;
            __w = __w.enabled(p.bool("enabled"));
            if !p.bool("allow_clear") {
                __w = __w.allow_clear(p.bool("allow_clear"));
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = format!(
            "Rating::new()\n    .max({})\n    .half_steps({})\n    .read_only({})",
            p.i64("max"),
            p.bool("half"),
            p.bool("read_only"),
        );
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("allow_clear", ".allow_clear", SnipProp::Bool(true)),
                ("enabled", ".enabled", SnipProp::Bool(true)),
            ],
        ));
        __s
    },
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
        PropSpec::Bool {
            key: "visited",
            label: "Visited",
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
        let mut __w = Link::new(p.str("text")).target(p.str("href"));
        __w = __w.enabled(p.bool("enabled"));
        if p.bool("visited") {
            __w = __w.visited(p.bool("visited"));
        }
        Box::new(__w)
    },
    snippet: |p| {
        let mut __s = format!("Link::new({:?}).target({:?})", p.str("text"), p.str("href"));
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("visited", ".visited", SnipProp::Bool(false)),
                ("enabled", ".enabled", SnipProp::Bool(true)),
            ],
        ));
        __s
    },
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
        PropSpec::Bool {
            key: "visible",
            label: "Visible",
            default: true
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
        let mut __w = FloatButton::new(p.str("label")).enabled(p.bool("enabled"));
        if !p.str("a11y_label").is_empty() {
            __w = __w.label(p.str("a11y_label"));
        }
        if !p.bool("visible") {
            __w = __w.visible(p.bool("visible"));
        }
        if !p.str("icon_d").is_empty() {
            __w = __w.icon_d(p.str("icon_d"));
        }
        if !p.str("icon_named").is_empty() {
            __w = __w.icon_named(p.str("icon_named"));
        }
        Box::new(__w)
    },
    snippet: |p| {
        let mut __s = format!("FloatButton::new({:?})", p.str("label"));
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("visible", ".visible", SnipProp::Bool(true)),
                ("icon_d", ".icon_d", SnipProp::Text("")),
                ("icon_named", ".icon_named", SnipProp::Text("")),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        __s
    },
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
        PropSpec::Float {
            key: "max",
            label: "Max",
            min: -8.5,
            max: 100.0,
            step: 1.0,
            default: 1.0
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
        let mut v = Volume::new();
        v.set_gain(p.f64("gain") as f32);
        v.set_muted(p.bool("muted"));
        {
            let mut __w = v;
            if !p.str("a11y_label").is_empty() {
                __w = __w.label(p.str("a11y_label"));
            }
            if p.f64("max") != 1.0 {
                __w = __w.max(p.f64("max") as f32);
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = format!(
            "let mut v = Volume::new();\nv.set_gain({:?});\nv.set_muted({});",
            p.f64("gain") as f32,
            p.bool("muted"),
        );
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("max", ".max", SnipProp::Float(1.0)),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        __s
    },
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
    props: &[
        PropSpec::Float {
            key: "value",
            label: "Value",
            min: 0.0,
            max: 1.0,
            step: 0.05,
            default: 0.35
        },
        PropSpec::Float {
            key: "step",
            label: "Step",
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
    build: |p| {
        let mut d = Dial::new().range(0.0, 1.0);
        d.set_value(p.f64("value"));
        {
            let mut __w = d;
            __w = __w.enabled(p.bool("enabled"));
            if !p.str("a11y_label").is_empty() {
                __w = __w.a11y_label(p.str("a11y_label"));
            }
            if p.f64("step") != 0.0 {
                __w = __w.step(p.f64("step"));
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = format!(
            "let mut d = Dial::new().range(0.0, 1.0);\nd.set_value({});",
            p.f64("value"),
        );
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("step", ".step", SnipProp::Float(0.0)),
                ("enabled", ".enabled", SnipProp::Bool(true)),
                ("a11y_label", ".a11y_label", SnipProp::Text("")),
            ],
        ));
        __s
    },
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
    props: &[
        PropSpec::Text {
            key: "items",
            label: "Items (csv)",
            default: "Mercury,Venus,Earth,Mars",
        },
        PropSpec::Int {
            key: "selected_index",
            label: "Selected Index",
            min: 0,
            max: 100,
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
        let mut __w = WheelPicker::new().items(csv(p, "items"));
        __w = __w.enabled(p.bool("enabled"));
        if !p.str("a11y_label").is_empty() {
            __w = __w.label(p.str("a11y_label"));
        }
        if p.i64("selected_index") != 0 {
            __w = __w.selected_index(p.i64("selected_index") as usize);
        }
        Box::new(__w)
    },
    snippet: |p| {
        let mut __s = {
            let items = csv(p, "items")
                .iter()
                .map(|i| format!("{i:?}"))
                .collect::<Vec<_>>()
                .join(", ");
            format!("WheelPicker::new().items([{items}])")
        };
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("selected_index", ".selected_index", SnipProp::Int(0)),
                ("enabled", ".enabled", SnipProp::Bool(true)),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        __s
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
        let mut j = Joystick::new().spring(p.bool("spring"));
        j = j.dead_zone(p.f64("dead_zone") as f32);
        {
            let mut __w = j;
            if !p.str("a11y_label").is_empty() {
                __w = __w.label(p.str("a11y_label"));
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = format!(
            "Joystick::new()\n    .spring({})\n    .dead_zone({:?})",
            p.bool("spring"),
            p.f64("dead_zone") as f32,
        );
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[("a11y_label", ".label", SnipProp::Text(""))],
        ));
        __s
    },
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
    props: &[
        PropSpec::Bool {
            key: "enabled",
            label: "Enabled",
            default: true
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
        let mut __w = Keypad::new().enabled(p.bool("enabled"));
        if !p.str("a11y_label").is_empty() {
            __w = __w.label(p.str("a11y_label"));
        }
        Box::new(__w)
    },
    snippet: |p| {
        let mut __s = format!("Keypad::new().enabled({})", p.bool("enabled"));
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[("a11y_label", ".label", SnipProp::Text(""))],
        ));
        __s
    },
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
        PropSpec::Choice {
            key: "ymode",
            label: "Y mode",
            options: &["Rotated", "Upright"],
            default: 0
        },
        PropSpec::Text {
            key: "value",
            label: "Value (csv)",
            default: ""
        },
        PropSpec::Probe {
            key: "value",
            value: "0.3,0.7"
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
        let mut pad = XYPad::new();
        pad = pad.labels(p.str("x"), p.str("y"));
        pad = pad.y_label_mode(if p.choice("ymode") == 0 {
            martensite::text_paint::VerticalTextMode::Rotated
        } else {
            martensite::text_paint::VerticalTextMode::Upright
        });
        {
            let mut __w = pad;
            __w = __w.enabled(p.bool("enabled"));
            if let Some(v) = crate::pages::parse_pair(p.str("value")) {
                __w = __w.value(v.0 as f32, v.1 as f32);
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = format!(
            "XYPad::new().labels({:?}, {:?}).y_label_mode(VerticalTextMode::{})",
            p.str("x"),
            p.str("y"),
            if p.choice("ymode") == 0 {
                "Rotated"
            } else {
                "Upright"
            }
        );
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[("enabled", ".enabled", SnipProp::Bool(true))],
        ));
        __s.push_str(&crate::pages::snip_textmap(
            p,
            "value",
            ".value",
            "",
            crate::pages::expr_pair,
        ));
        __s
    },
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
    props: &[
        PropSpec::Bool {
            key: "readout",
            label: "Readout",
            default: true
        },
        PropSpec::Text {
            key: "color",
            label: "Color",
            default: ""
        },
        PropSpec::Probe {
            key: "color",
            value: "255,64,64,255"
        },
        PropSpec::Float {
            key: "pos_x",
            label: "Position X",
            min: 0.0,
            max: 1.0,
            step: 0.05,
            default: 0.62
        },
        PropSpec::Float {
            key: "pos_y",
            label: "Position Y",
            min: 0.0,
            max: 1.0,
            step: 0.05,
            default: 0.42
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
        let mut c = Crosshair::new();
        c.show_readout = p.bool("readout");
        c.set_position(glam::Vec2::new(
            p.f64("pos_x") as f32,
            p.f64("pos_y") as f32,
        ));
        {
            let mut __w = c;
            if !p.str("a11y_label").is_empty() {
                __w = __w.label(p.str("a11y_label"));
            }
            if let Some(v) = crate::pages::parse_rgba(p.str("color")) {
                __w = __w.color(v);
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = format!("Crosshair::new() /* show_readout={} */", p.bool("readout"));
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[("a11y_label", ".label", SnipProp::Text(""))],
        ));
        __s.push_str(&crate::pages::snip_textmap(
            p,
            "color",
            ".color",
            "",
            crate::pages::expr_rgba,
        ));
        __s
    },
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
    props: &[
        PropSpec::Float {
            key: "cents",
            label: "Cents",
            min: -50.0,
            max: 50.0,
            step: 1.0,
            default: -12.0,
        },
        PropSpec::Text {
            key: "note",
            label: "Note",
            default: "—"
        },
        PropSpec::Float {
            key: "band",
            label: "Band",
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
        let mut t = Tuner::new();
        t.set_pitch("A", p.f64("cents") as f32);
        {
            let mut __w = t;
            if !p.str("a11y_label").is_empty() {
                __w = __w.label(p.str("a11y_label"));
            }
            if p.str("note") != "—" {
                __w = __w.note(p.str("note"));
            }
            if p.f64("band") != 0.0 {
                __w = __w.band(p.f64("band") as f32);
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = format!(
            "let mut t = Tuner::new();\nt.set_pitch(\"A\", {:?});",
            p.f64("cents") as f32,
        );
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("note", ".note", SnipProp::Text("—")),
                ("band", ".band", SnipProp::Float(0.0)),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        __s
    },
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
        PropSpec::Int {
            key: "pad_color_index",
            label: "Pad Color Index",
            min: 0,
            max: 32,
            default: 0
        },
        PropSpec::Text {
            key: "pad_color_color",
            label: "Pad Color Color",
            default: ""
        },
        PropSpec::Int {
            key: "pad_label_index",
            label: "Pad Label Index",
            min: 0,
            max: 32,
            default: 0
        },
        PropSpec::Text {
            key: "pad_label_label",
            label: "Pad Label Label",
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
        let mut __w = PadGrid::new(p.i64("cols") as usize, p.i64("rows") as usize);
        if !p.str("a11y_label").is_empty() {
            __w = __w.label(p.str("a11y_label"));
        }
        // Index props target one pad — clamp into the staged grid so
        // every index lands somewhere visible.
        let last = __w.pad_count() - 1;
        if p.i64("pad_color_index") != 0 || !p.str("pad_color_color").is_empty() {
            __w = __w.pad_color(
                (p.i64("pad_color_index") as usize).min(last),
                crate::pages::parse_rgba(p.str("pad_color_color")).unwrap_or([0, 0, 0, 255]),
            );
        }
        if p.i64("pad_label_index") != 0 || !p.str("pad_label_label").is_empty() {
            // An index without text is invisible — stage a placeholder
            // so the index slider demonstrates itself.
            let text = p.str("pad_label_label");
            __w = __w.pad_label(
                (p.i64("pad_label_index") as usize).min(last),
                if text.is_empty() { "Pad" } else { text },
            );
        }
        Box::new(__w)
    },
    snippet: |p| {
        let mut __s = format!("PadGrid::new({}, {})", p.i64("cols"), p.i64("rows"));
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[("a11y_label", ".label", SnipProp::Text(""))],
        ));
        if p.i64("pad_color_index") != 0 || !p.str("pad_color_color").is_empty() {
            __s.push_str(&format!(
                "\n    .pad_color({}, {})",
                p.i64("pad_color_index"),
                crate::pages::expr_rgba(p.str("pad_color_color")).unwrap_or_default()
            ));
        }
        if p.i64("pad_label_index") != 0 || !p.str("pad_label_label").is_empty() {
            __s.push_str(&format!(
                "\n    .pad_label({}, {:?})",
                p.i64("pad_label_index"),
                p.str("pad_label_label")
            ));
        }
        __s
    },
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
    props: &[
        PropSpec::Text {
            key: "title",
            label: "Title",
            default: "Accent"
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
        let mut __w = ColorButton::new([80, 140, 255, 255]).title(p.str("title"));
        __w = __w.enabled(p.bool("enabled"));
        Box::new(__w)
    },
    snippet: |p| {
        let mut __s = format!(
            "ColorButton::new([80, 140, 255, 255]).title({:?})",
            p.str("title"),
        );
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[("enabled", ".enabled", SnipProp::Bool(true))],
        ));
        __s
    },
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
    props: &[
        PropSpec::Text {
            key: "swatches",
            label: "Swatches",
            default: "154,163,255;124,135,240;86,81,217;74,79,208;96,196,140;240,178,66;224,92,92;120,124,140;168,172,190;40,42,52"
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
        let mut __w = ColorPalette::new();
        __w = __w.enabled(_p.bool("enabled"));
        if !_p.str("a11y_label").is_empty() {
            __w = __w.label(_p.str("a11y_label"));
        }
        let __v: Vec<[u8; 4]> = _p
            .str("swatches")
            .split(';')
            .filter_map(crate::pages::parse_rgba)
            .collect();
        if !__v.is_empty() {
            __w = __w.swatches(__v);
        }
        Box::new(__w)
    },
    snippet: |_p| {
        let mut __s = "ColorPalette::new()".to_string();
        __s.push_str(&crate::pages::prop_snippet(
            _p,
            &[
                ("enabled", ".enabled", SnipProp::Bool(true)),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        __s.push_str(&crate::pages::snip_textmap(
            _p,
            "swatches",
            ".swatches",
            "",
            crate::pages::expr_strs,
        ));
        __s
    },
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
    props: &[
        PropSpec::Bool {
            key: "alpha",
            label: "Alpha channel",
            default: true
        },
        PropSpec::Text {
            key: "color",
            label: "Color",
            default: ""
        },
        PropSpec::Probe {
            key: "color",
            value: "255,64,64,255"
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
        let mut c = ColorPicker::new().with_alpha(p.bool("alpha"));
        c.open();
        {
            let mut __w = c;
            __w = __w.enabled(p.bool("enabled"));
            if !p.str("a11y_label").is_empty() {
                __w = __w.label(p.str("a11y_label"));
            }
            if let Some([r, g, b, a]) = crate::pages::parse_rgba(p.str("color")) {
                __w = __w.color(martensite::widgets::color_picker::Color::rgba(r, g, b, a));
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = format!("ColorPicker::new().with_alpha({})", p.bool("alpha"));
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("enabled", ".enabled", SnipProp::Bool(true)),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        __s.push_str(&crate::pages::snip_textmap(
            p,
            "color",
            ".color",
            "",
            crate::pages::expr_rgba,
        ));
        __s
    },
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
    props: &[
        PropSpec::Float {
            key: "hue",
            label: "Hue (deg)",
            min: 0.0,
            max: 360.0,
            step: 1.0,
            default: 210.0,
        },
        PropSpec::Float {
            key: "saturation",
            label: "Saturation",
            min: 0.0,
            max: 1.0,
            step: 0.05,
            default: 1.0
        },
        PropSpec::Float {
            key: "brightness",
            label: "Brightness",
            min: -8.5,
            max: 100.0,
            step: 1.0,
            default: 1.0
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
        let mut __w = ColorWheel::new().hue(p.f64("hue") as f32);
        if !p.str("a11y_label").is_empty() {
            __w = __w.label(p.str("a11y_label"));
        }
        if p.f64("saturation") != 1.0 {
            __w = __w.saturation(p.f64("saturation") as f32);
        }
        if p.f64("brightness") != 1.0 {
            __w = __w.brightness(p.f64("brightness") as f32);
        }
        Box::new(__w)
    },
    snippet: |p| {
        let mut __s = format!(
            "let mut w = ColorWheel::new();\nw.set_hue({:?});",
            p.f64("hue") as f32,
        );
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("saturation", ".saturation", SnipProp::Float(1.0)),
                ("brightness", ".brightness", SnipProp::Float(1.0)),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        __s
    },
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
    props: &[
        PropSpec::Float {
            key: "alpha",
            label: "Alpha",
            min: 0.0,
            max: 1.0,
            step: 0.05,
            default: 0.8,
        },
        PropSpec::Text {
            key: "color",
            label: "Color",
            default: ""
        },
        PropSpec::Probe {
            key: "color",
            value: "255,64,64,255"
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
        let mut s = AlphaSlider::new();
        s.set_alpha(p.f64("alpha") as f32);
        {
            let mut __w = s;
            if !p.str("a11y_label").is_empty() {
                __w = __w.label(p.str("a11y_label"));
            }
            if let Some(v) = crate::pages::parse_rgba(p.str("color")) {
                __w = __w.color(v);
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = format!(
            "let mut s = AlphaSlider::new();\ns.set_alpha({:?});",
            p.f64("alpha") as f32,
        );
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[("a11y_label", ".label", SnipProp::Text(""))],
        ));
        __s.push_str(&crate::pages::snip_textmap(
            p,
            "color",
            ".color",
            "",
            crate::pages::expr_rgba,
        ));
        __s
    },
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
    props: &[
        PropSpec::Float {
            key: "hue",
            label: "Hue",
            min: 0.0,
            max: 360.0,
            step: 1.0,
            default: 40.0,
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
        let mut s = HueSlider::new();
        s.set_hue(p.f64("hue") as f32);
        {
            let mut __w = s;
            if !p.str("a11y_label").is_empty() {
                __w = __w.label(p.str("a11y_label"));
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = format!(
            "let mut s = HueSlider::new();\ns.set_hue({:?});",
            p.f64("hue") as f32,
        );
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[("a11y_label", ".label", SnipProp::Text(""))],
        ));
        __s
    },
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
    props: &[
        PropSpec::Text {
            key: "placeholder",
            label: "Placeholder",
            default: "Choose file…",
        },
        PropSpec::Choice {
            key: "mode",
            label: "Mode",
            options: &["Open", "Save"],
            default: 0
        },
        PropSpec::Text {
            key: "file_name",
            label: "File Name",
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
        let mut __w = FileChooserButton::new().placeholder(p.str("placeholder"));
        __w = __w.enabled(p.bool("enabled"));
        if p.choice("mode") != 0 {
            __w = __w.mode(match p.choice("mode") {
                0 => martensite::widgets::file_chooser_button::ChooserMode::Open,
                1 => martensite::widgets::file_chooser_button::ChooserMode::Save,
                _ => martensite::widgets::file_chooser_button::ChooserMode::Open,
            });
        }
        if !p.str("file_name").is_empty() {
            __w = __w.file_name(p.str("file_name"));
        }
        Box::new(__w)
    },
    snippet: |p| {
        let mut __s = format!(
            "FileChooserButton::new().placeholder({:?})",
            p.str("placeholder"),
        );
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                (
                    "mode",
                    ".mode",
                    SnipProp::Choice(&[
                        "martensite::widgets::file_chooser_button::ChooserMode::Open",
                        "martensite::widgets::file_chooser_button::ChooserMode::Save",
                    ]),
                ),
                ("file_name", ".file_name", SnipProp::Text("")),
                ("enabled", ".enabled", SnipProp::Bool(true)),
            ],
        ));
        __s
    },
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
        let mut __w = FontButton::new(p.str("family"), p.f64("size") as f32);
        __w = __w.enabled(p.bool("enabled"));
        if !p.str("a11y_label").is_empty() {
            __w = __w.a11y_label(p.str("a11y_label"));
        }
        Box::new(__w)
    },
    snippet: |p| {
        let mut __s = format!(
            "FontButton::new({:?}, {:?})",
            p.str("family"),
            p.f64("size") as f32,
        );
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("enabled", ".enabled", SnipProp::Bool(true)),
                ("a11y_label", ".a11y_label", SnipProp::Text("")),
            ],
        ));
        __s
    },
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
        PropSpec::Text {
            key: "zoom_range",
            label: "Zoom Range (csv)",
            default: ""
        },
    ],
    build: |p| {
        let mut __w = ZoomControls::new()
            .label("Zoom")
            .zoom(p.f64("zoom") as f32)
            .readout(true)
            .fit(true)
            .reset(true)
            .horizontal(p.bool("horizontal"));
        if let Some(v) = crate::pages::parse_pair(p.str("zoom_range")) {
            __w = __w.zoom_range(v.0 as f32, v.1 as f32);
        }
        Box::new(__w)
    },
    snippet: |p| {
        let mut __s = format!(
            "ZoomControls::new().zoom({:?}).readout(true).horizontal({})",
            p.f64("zoom") as f32,
            p.bool("horizontal"),
        );
        __s.push_str(&crate::pages::prop_snippet(p, &[]));
        __s.push_str(&crate::pages::snip_textmap(
            p,
            "zoom_range",
            ".zoom_range",
            "",
            crate::pages::expr_pair,
        ));
        __s
    },
    poll: |w, out| {
        if let Some(z) = downcast_mut::<ZoomControls>(w) {
            if let Some(a) = z.take_action() {
                out.push(format!("{a:?}"));
            }
        }
    },
});
