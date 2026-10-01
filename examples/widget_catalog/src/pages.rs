//! Page registry — one [`Page`] per public widget, grouped by family
//! in rail order.
//!
//! Each page builds the staged widget from [`PropValues`], emits the
//! matching Rust expression for the live snippet, drains the widget's
//! event channels into the log, and reports observable state for the
//! diff logger.

use martensite::core::Widget;
use martensite::widgets::button::Button;
use martensite::widgets::checkbox::CheckBox;
use martensite::widgets::dropdown::Dropdown;
use martensite::widgets::list_view::ListView;
use martensite::widgets::progress::{ProgressBar, Spinner};
use martensite::widgets::segmented::Segmented;
use martensite::widgets::slider::Slider;
use martensite::widgets::spinbox::SpinBox;
use martensite::widgets::switch::Switch;
use martensite::widgets::text::Text;
use martensite::widgets::text_input::TextInput;

use crate::page::{Page, PageMeta, PropSpec, PropValues};

/// Downcasts the staged widget for event/state drains.
fn downcast_mut<T: 'static>(w: &mut dyn Widget) -> Option<&mut T> {
    w.as_any_mut()?.downcast_mut::<T>()
}

/// `Text` prop parsed as comma-separated options.
fn csv(props: &PropValues, key: &str) -> Vec<String> {
    props
        .str(key)
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

// ------------------------------------------------------------------
// Controls
// ------------------------------------------------------------------

struct ButtonPage;

impl Page for ButtonPage {
    fn meta(&self) -> PageMeta {
        PageMeta {
            name: "Button",
            family: "Controls",
            description: "Clickable command button with optional icon, \
                          primary styling, and tooltip.",
            role: "Button",
            aliases: &[
                ("Qt", "QPushButton"),
                ("GTK", "GtkButton"),
                ("SwiftUI", "Button"),
                ("React", "<button>"),
            ],
            needs_overlay: false,
        }
    }

    fn props(&self) -> &'static [PropSpec] {
        &[
            PropSpec::Text {
                key: "label",
                label: "Label",
                default: "Apply",
            },
            PropSpec::Bool {
                key: "primary",
                label: "Primary",
                default: false,
            },
            PropSpec::Bool {
                key: "enabled",
                label: "Enabled",
                default: true,
            },
            PropSpec::Text {
                key: "tooltip",
                label: "Tooltip",
                default: "",
            },
        ]
    }

    fn build(&self, p: &PropValues) -> Box<dyn Widget> {
        let mut b = Button::new(p.str("label"))
            .primary(p.bool("primary"))
            .enabled(p.bool("enabled"));
        let tip = p.str("tooltip");
        if !tip.is_empty() {
            b = b.tooltip(tip);
        }
        Box::new(b)
    }

    fn snippet(&self, p: &PropValues) -> String {
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
    }

    fn poll_events(&self, w: &mut dyn Widget, out: &mut Vec<String>) {
        if let Some(b) = downcast_mut::<Button>(w) {
            if b.take_activated() {
                out.push("activated".to_string());
            }
        }
    }

    fn describe_state(&self, w: &mut dyn Widget) -> Vec<(String, String)> {
        downcast_mut::<Button>(w)
            .map(|b| vec![("pressed".to_string(), b.is_pressed().to_string())])
            .unwrap_or_default()
    }
}

struct CheckBoxPage;

impl Page for CheckBoxPage {
    fn meta(&self) -> PageMeta {
        PageMeta {
            name: "Checkbox",
            family: "Controls",
            description: "Binary or tristate check control.",
            role: "CheckBox",
            aliases: &[
                ("Qt", "QCheckBox"),
                ("GTK", "GtkCheckButton"),
                ("SwiftUI", "Toggle"),
                ("React", "<input type=checkbox>"),
            ],
            needs_overlay: false,
        }
    }

    fn props(&self) -> &'static [PropSpec] {
        &[
            PropSpec::Text {
                key: "label",
                label: "Label",
                default: "Enable notifications",
            },
            PropSpec::Bool {
                key: "checked",
                label: "Checked",
                default: false,
            },
            PropSpec::Bool {
                key: "tristate",
                label: "Tristate",
                default: false,
            },
            PropSpec::Bool {
                key: "enabled",
                label: "Enabled",
                default: true,
            },
        ]
    }

    fn build(&self, p: &PropValues) -> Box<dyn Widget> {
        Box::new(
            CheckBox::new(p.str("label"))
                .checked(p.bool("checked"))
                .tristate(p.bool("tristate"))
                .enabled(p.bool("enabled")),
        )
    }

    fn snippet(&self, p: &PropValues) -> String {
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
    }

    fn describe_state(&self, w: &mut dyn Widget) -> Vec<(String, String)> {
        downcast_mut::<CheckBox>(w)
            .map(|c| vec![("state".to_string(), format!("{:?}", c.state()))])
            .unwrap_or_default()
    }
}

struct SwitchPage;

impl Page for SwitchPage {
    fn meta(&self) -> PageMeta {
        PageMeta {
            name: "Switch",
            family: "Controls",
            description: "Sliding on/off switch.",
            role: "Switch",
            aliases: &[
                ("Qt", "QCheckBox (switch style)"),
                ("GTK", "GtkSwitch"),
                ("SwiftUI", "Toggle(switch)"),
                ("React", "role=switch"),
            ],
            needs_overlay: false,
        }
    }

    fn props(&self) -> &'static [PropSpec] {
        &[
            PropSpec::Text {
                key: "label",
                label: "Label",
                default: "Wi-Fi",
            },
            PropSpec::Bool {
                key: "on",
                label: "On",
                default: true,
            },
        ]
    }

    fn build(&self, p: &PropValues) -> Box<dyn Widget> {
        Box::new(Switch::new(p.str("label")).on(p.bool("on")))
    }

    fn snippet(&self, p: &PropValues) -> String {
        format!("Switch::new({:?}).on({})", p.str("label"), p.bool("on"))
    }

    fn describe_state(&self, w: &mut dyn Widget) -> Vec<(String, String)> {
        downcast_mut::<Switch>(w)
            .map(|s| vec![("on".to_string(), s.on.to_string())])
            .unwrap_or_default()
    }
}

struct SliderPage;

impl Page for SliderPage {
    fn meta(&self) -> PageMeta {
        PageMeta {
            name: "Slider",
            family: "Controls",
            description: "Continuous value selector on a track.",
            role: "Slider",
            aliases: &[
                ("Qt", "QSlider"),
                ("GTK", "GtkScale"),
                ("SwiftUI", "Slider"),
                ("React", "<input type=range>"),
            ],
            needs_overlay: false,
        }
    }

    fn props(&self) -> &'static [PropSpec] {
        &[
            PropSpec::Float {
                key: "min",
                label: "Min",
                min: 0.0,
                max: 100.0,
                step: 1.0,
                default: 0.0,
            },
            PropSpec::Float {
                key: "max",
                label: "Max",
                min: 1.0,
                max: 100.0,
                step: 1.0,
                default: 100.0,
            },
            PropSpec::Float {
                key: "value",
                label: "Value",
                min: 0.0,
                max: 100.0,
                step: 1.0,
                default: 40.0,
            },
        ]
    }

    fn build(&self, p: &PropValues) -> Box<dyn Widget> {
        let (min, max) = (
            p.f64("min").min(p.f64("max")),
            p.f64("max").max(p.f64("min")),
        );
        let v = p.f64("value").clamp(min, max);
        Box::new(Slider::new(min, max).with_value(v))
    }

    fn snippet(&self, p: &PropValues) -> String {
        format!(
            "Slider::new({}, {}).with_value({})",
            p.f64("min"),
            p.f64("max"),
            p.f64("value")
        )
    }

    fn describe_state(&self, w: &mut dyn Widget) -> Vec<(String, String)> {
        downcast_mut::<Slider>(w)
            .map(|s| vec![("value".to_string(), format!("{:.2}", s.value()))])
            .unwrap_or_default()
    }
}

struct SpinBoxPage;

impl Page for SpinBoxPage {
    fn meta(&self) -> PageMeta {
        PageMeta {
            name: "Spinbox",
            family: "Controls",
            description: "Numeric field with stepper buttons.",
            role: "SpinBox",
            aliases: &[
                ("Qt", "QSpinBox"),
                ("GTK", "GtkSpinButton"),
                ("SwiftUI", "Stepper"),
                ("React", "<input type=number>"),
            ],
            needs_overlay: false,
        }
    }

    fn props(&self) -> &'static [PropSpec] {
        &[
            PropSpec::Int {
                key: "min",
                label: "Min",
                min: -100,
                max: 100,
                default: 0,
            },
            PropSpec::Int {
                key: "max",
                label: "Max",
                min: -100,
                max: 1000,
                default: 100,
            },
            PropSpec::Int {
                key: "value",
                label: "Value",
                min: -100,
                max: 1000,
                default: 12,
            },
        ]
    }

    fn build(&self, p: &PropValues) -> Box<dyn Widget> {
        Box::new(
            SpinBox::new()
                .range(p.i64("min") as f64, p.i64("max") as f64)
                .with_value(p.i64("value") as f64),
        )
    }

    fn snippet(&self, p: &PropValues) -> String {
        format!(
            "SpinBox::new().range({}, {}).with_value({})",
            p.i64("min"),
            p.i64("max"),
            p.i64("value")
        )
    }

    fn describe_state(&self, w: &mut dyn Widget) -> Vec<(String, String)> {
        downcast_mut::<SpinBox>(w)
            .map(|s| vec![("value".to_string(), s.value().to_string())])
            .unwrap_or_default()
    }
}

struct SegmentedPage;

impl Page for SegmentedPage {
    fn meta(&self) -> PageMeta {
        PageMeta {
            name: "Segmented",
            family: "Controls",
            description: "Single-select segmented control.",
            role: "RadioGroup",
            aliases: &[
                ("Qt", "QButtonGroup"),
                ("GTK", "linked GtkToggleButtons"),
                ("SwiftUI", "Picker(segmented)"),
                ("iOS", "UISegmentedControl"),
            ],
            needs_overlay: false,
        }
    }

    fn props(&self) -> &'static [PropSpec] {
        &[PropSpec::Text {
            key: "options",
            label: "Options (csv)",
            default: "Day,Week,Month",
        }]
    }

    fn build(&self, p: &PropValues) -> Box<dyn Widget> {
        Box::new(Segmented::new().options(csv(p, "options")).selected(0))
    }

    fn snippet(&self, p: &PropValues) -> String {
        let opts = csv(p, "options")
            .iter()
            .map(|o| format!("{o:?}"))
            .collect::<Vec<_>>()
            .join(", ");
        format!("Segmented::new().options([{opts}]).selected(0)")
    }

    fn poll_events(&self, w: &mut dyn Widget, out: &mut Vec<String>) {
        if let Some(seg) = downcast_mut::<Segmented>(w) {
            if let Some(i) = seg.take_selected() {
                out.push(format!("selected → {i}"));
            }
        }
    }
}

struct DropdownPage;

impl Page for DropdownPage {
    fn meta(&self) -> PageMeta {
        PageMeta {
            name: "Dropdown",
            family: "Controls",
            description: "Collapsed select with a popup option list.",
            role: "ComboBox",
            aliases: &[
                ("Qt", "QComboBox"),
                ("GTK", "GtkDropDown"),
                ("SwiftUI", "Picker(menu)"),
                ("React", "<select>"),
            ],
            needs_overlay: true,
        }
    }

    fn props(&self) -> &'static [PropSpec] {
        &[PropSpec::Text {
            key: "options",
            label: "Options (csv)",
            default: "Light,Dark,System",
        }]
    }

    fn build(&self, p: &PropValues) -> Box<dyn Widget> {
        Box::new(Dropdown::new(csv(p, "options")))
    }

    fn snippet(&self, p: &PropValues) -> String {
        let opts = csv(p, "options")
            .iter()
            .map(|o| format!("{o:?}"))
            .collect::<Vec<_>>()
            .join(", ");
        format!("Dropdown::new([{opts}])")
    }

    fn describe_state(&self, w: &mut dyn Widget) -> Vec<(String, String)> {
        downcast_mut::<Dropdown>(w)
            .map(|d| {
                vec![
                    ("selected".to_string(), d.selected().to_string()),
                    ("open".to_string(), d.popup_id().is_some().to_string()),
                ]
            })
            .unwrap_or_default()
    }
}

// ------------------------------------------------------------------
// Text & input
// ------------------------------------------------------------------

struct TextPage;

impl Page for TextPage {
    fn meta(&self) -> PageMeta {
        PageMeta {
            name: "Text",
            family: "Text",
            description: "Static shaped text with size, color, and \
                          family control. Honors ambient direction for \
                          RTL scripts.",
            role: "Label",
            aliases: &[
                ("Qt", "QLabel"),
                ("GTK", "GtkLabel"),
                ("SwiftUI", "Text"),
                ("React", "<span>"),
            ],
            needs_overlay: false,
        }
    }

    fn props(&self) -> &'static [PropSpec] {
        &[
            PropSpec::Text {
                key: "content",
                label: "Content",
                default: "The quick brown fox",
            },
            PropSpec::Float {
                key: "size",
                label: "Size",
                min: 8.0,
                max: 64.0,
                step: 1.0,
                default: 16.0,
            },
        ]
    }

    fn build(&self, p: &PropValues) -> Box<dyn Widget> {
        Box::new(Text::new(p.str("content").to_string()).font_size(p.f64("size") as f32))
    }

    fn snippet(&self, p: &PropValues) -> String {
        format!(
            "Text::new({:?}).font_size({:?})",
            p.str("content"),
            p.f64("size") as f32
        )
    }
}

struct TextInputPage;

impl Page for TextInputPage {
    fn meta(&self) -> PageMeta {
        PageMeta {
            name: "TextInput",
            family: "Text",
            description: "Single-line editable text field with \
                          placeholder, password echo, and clipboard.",
            role: "TextInput",
            aliases: &[
                ("Qt", "QLineEdit"),
                ("GTK", "GtkEntry"),
                ("SwiftUI", "TextField"),
                ("React", "<input>"),
            ],
            needs_overlay: false,
        }
    }

    fn props(&self) -> &'static [PropSpec] {
        &[
            PropSpec::Text {
                key: "value",
                label: "Value",
                default: "",
            },
            PropSpec::Text {
                key: "placeholder",
                label: "Placeholder",
                default: "Type here…",
            },
            PropSpec::Bool {
                key: "password",
                label: "Password",
                default: false,
            },
            PropSpec::Bool {
                key: "read_only",
                label: "Read-only",
                default: false,
            },
        ]
    }

    fn build(&self, p: &PropValues) -> Box<dyn Widget> {
        Box::new(
            TextInput::new("Sample field")
                .value(p.str("value"))
                .placeholder(p.str("placeholder"))
                .secure(p.bool("password"))
                .read_only(p.bool("read_only")),
        )
    }

    fn snippet(&self, p: &PropValues) -> String {
        let mut s = format!(
            "TextInput::new(\"Sample field\").value({:?})",
            p.str("value")
        );
        s.push_str(&format!("\n    .placeholder({:?})", p.str("placeholder")));
        if p.bool("password") {
            s.push_str("\n    .secure(true)");
        }
        if p.bool("read_only") {
            s.push_str("\n    .read_only(true)");
        }
        s
    }

    fn poll_events(&self, w: &mut dyn Widget, out: &mut Vec<String>) {
        if let Some(t) = downcast_mut::<TextInput>(w) {
            if t.take_edited() {
                out.push(format!("edited → {:?}", t.value));
            }
        }
    }

    fn describe_state(&self, w: &mut dyn Widget) -> Vec<(String, String)> {
        downcast_mut::<TextInput>(w)
            .map(|t| vec![("value".to_string(), t.value.clone())])
            .unwrap_or_default()
    }
}

// ------------------------------------------------------------------
// Feedback
// ------------------------------------------------------------------

struct ProgressBarPage;

impl Page for ProgressBarPage {
    fn meta(&self) -> PageMeta {
        PageMeta {
            name: "ProgressBar",
            family: "Feedback",
            description: "Determinate progress indicator.",
            role: "ProgressIndicator",
            aliases: &[
                ("Qt", "QProgressBar"),
                ("GTK", "GtkProgressBar"),
                ("SwiftUI", "ProgressView"),
                ("React", "<progress>"),
            ],
            needs_overlay: false,
        }
    }

    fn props(&self) -> &'static [PropSpec] {
        &[PropSpec::Float {
            key: "value",
            label: "Value 0–1",
            min: 0.0,
            max: 1.0,
            step: 0.05,
            default: 0.4,
        }]
    }

    fn build(&self, p: &PropValues) -> Box<dyn Widget> {
        Box::new(ProgressBar::new().value(p.f64("value") as f32))
    }

    fn snippet(&self, p: &PropValues) -> String {
        format!("ProgressBar::new().value({:?})", p.f64("value") as f32)
    }
}

struct SpinnerPage;

impl Page for SpinnerPage {
    fn meta(&self) -> PageMeta {
        PageMeta {
            name: "Spinner",
            family: "Feedback",
            description: "Indeterminate activity indicator.",
            role: "ProgressIndicator",
            aliases: &[
                ("Qt", "QProgressIndicator"),
                ("GTK", "GtkSpinner"),
                ("SwiftUI", "ProgressView()"),
                ("React", "spinner icon"),
            ],
            needs_overlay: false,
        }
    }

    fn build(&self, _p: &PropValues) -> Box<dyn Widget> {
        Box::new(Spinner::new())
    }

    fn snippet(&self, _p: &PropValues) -> String {
        "Spinner::new()".to_string()
    }
}

// ------------------------------------------------------------------
// Data
// ------------------------------------------------------------------

struct ListViewPage;

impl Page for ListViewPage {
    fn meta(&self) -> PageMeta {
        PageMeta {
            name: "ListView",
            family: "Data",
            description: "Virtualized scrollable item list with \
                          selection and activation.",
            role: "ListBox",
            aliases: &[
                ("Qt", "QListView"),
                ("GTK", "GtkListView"),
                ("SwiftUI", "List"),
                ("React", "<ul role=listbox>"),
            ],
            needs_overlay: false,
        }
    }

    fn props(&self) -> &'static [PropSpec] {
        &[PropSpec::Text {
            key: "items",
            label: "Items (csv)",
            default: "Alpha,Beta,Gamma,Delta,Epsilon",
        }]
    }

    fn build(&self, p: &PropValues) -> Box<dyn Widget> {
        let mut lv = ListView::new().label("Demo list");
        lv.set_items(csv(p, "items"));
        Box::new(lv)
    }

    fn snippet(&self, p: &PropValues) -> String {
        let items = csv(p, "items")
            .iter()
            .map(|i| format!("{i:?}"))
            .collect::<Vec<_>>()
            .join(", ");
        format!("let mut list = ListView::new();\nlist.set_items([{items}]);")
    }

    fn poll_events(&self, w: &mut dyn Widget, out: &mut Vec<String>) {
        if let Some(lv) = downcast_mut::<ListView>(w) {
            if let Some(i) = lv.take_activated() {
                out.push(format!("activated row {i}"));
            }
        }
    }

    fn describe_state(&self, w: &mut dyn Widget) -> Vec<(String, String)> {
        downcast_mut::<ListView>(w)
            .map(|lv| {
                vec![(
                    "selected".to_string(),
                    lv.selected().map_or("-".to_string(), |i| i.to_string()),
                )]
            })
            .unwrap_or_default()
    }
}

/// Every registered page, in rail order — grouped by family.
pub fn all_pages() -> Vec<Box<dyn Page>> {
    vec![
        // Controls
        Box::new(ButtonPage),
        Box::new(CheckBoxPage),
        Box::new(SwitchPage),
        Box::new(SliderPage),
        Box::new(SpinBoxPage),
        Box::new(SegmentedPage),
        Box::new(DropdownPage),
        // Text
        Box::new(TextPage),
        Box::new(TextInputPage),
        // Feedback
        Box::new(ProgressBarPage),
        Box::new(SpinnerPage),
        // Data
        Box::new(ListViewPage),
    ]
}
