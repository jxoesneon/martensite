//! Forms & input category — one live card per input-widget module.
//!
//! Each entry adapts the widget's own doctest construction (the
//! verified canonical shape); demo data is industrial-flavoured where
//! it does not distort that shape.

use martensite::core::Widget;
use martensite::widgets::auto_complete::AutoComplete;
use martensite::widgets::calendar::Calendar;
use martensite::widgets::cascader::{Cascader, CascaderOption};
use martensite::widgets::check_list::{CheckItem, CheckList};
use martensite::widgets::checkbox::CheckBox;
use martensite::widgets::date_picker::{Date, DatePicker};
use martensite::widgets::dropdown::Dropdown;
use martensite::widgets::file_chooser_button::{ChooserMode, FileChooserButton};
use martensite::widgets::font_button::FontButton;
use martensite::widgets::form_field::FormField;
use martensite::widgets::inline_edit::InlineEdit;
use martensite::widgets::ip_input::IpInput;
use martensite::widgets::key_capture::KeyCapture;
use martensite::widgets::keypad::Keypad;
use martensite::widgets::mention::Mention;
use martensite::widgets::otp_input::OtpInput;
use martensite::widgets::password_strength::PasswordStrength;
use martensite::widgets::pattern_lock::PatternLock;
use martensite::widgets::poll::{Poll, PollOption};
use martensite::widgets::radio::RadioGroup;
use martensite::widgets::range_slider::RangeSlider;
use martensite::widgets::rating::Rating;
use martensite::widgets::search_bar::SearchBar;
use martensite::widgets::search_field::SearchField;
use martensite::widgets::segmented::Segmented;
use martensite::widgets::slider::Slider;
use martensite::widgets::spinbox::SpinBox;
use martensite::widgets::switch::Switch;
use martensite::widgets::text_area::TextArea;
use martensite::widgets::text_input::TextInput;
use martensite::widgets::time_picker::{Time, TimePicker};
use martensite::widgets::toggle_button::ToggleButton;
use martensite::widgets::token_field::TokenField;
use martensite::widgets::transfer::Transfer;
use martensite::widgets::tree_select::TreeSelect;
use martensite::widgets::tree_view::TreeNode;
use martensite::widgets::unit_converter::{UnitCategory, UnitConverter};
use martensite::widgets::wheel_picker::WheelPicker;

/// Forms & input showcase entries — `(display name, live widget)`.
pub fn entries() -> Vec<(&'static str, Box<dyn Widget>)> {
    vec![
        (
            "Auto Complete",
            Box::new(AutoComplete::new().suggestions([
                "CNC-Mill-02",
                "PLC-East-07",
                "Robot-Arm-K7",
                "AGV-Dock-04",
                "HVAC-Skid-11",
            ])),
        ),
        (
            "Calendar",
            Box::new(
                Calendar::new()
                    .date(Date {
                        year: 2024,
                        month: 6,
                        day: 15,
                    })
                    .today(Date {
                        year: 2024,
                        month: 6,
                        day: 1,
                    }),
            ),
        ),
        (
            "Cascader",
            Box::new(
                Cascader::new().options([
                    CascaderOption::new("Plant East", "east").child(
                        CascaderOption::new("Line 1", "east-l1")
                            .child(CascaderOption::new("CNC-Mill-02", "cnc-02")),
                    ),
                    CascaderOption::new("Plant West", "west").child(
                        CascaderOption::new("Line 3", "west-l3")
                            .child(CascaderOption::new("Lathe-07", "lathe-07")),
                    ),
                ]),
            ),
        ),
        (
            "Checklist",
            Box::new(
                CheckList::new()
                    .item(CheckItem::new("Guard door closed").checked(true))
                    .item(CheckItem::new("Torque verified").checked(true))
                    .item(CheckItem::new("Label applied"))
                    .item(CheckItem::new("Photo captured")),
            ),
        ),
        (
            "Checkbox",
            Box::new(CheckBox::new("Auto-reorder below min stock").checked(true)),
        ),
        (
            "Date Picker",
            Box::new(
                DatePicker::new()
                    .date(Date {
                        year: 2024,
                        month: 6,
                        day: 15,
                    })
                    .today(Date {
                        year: 2024,
                        month: 6,
                        day: 1,
                    }),
            ),
        ),
        (
            "Dropdown",
            Box::new(Dropdown::new([
                "Shift A (06–14)",
                "Shift B (14–22)",
                "Shift C (22–06)",
            ])),
        ),
        (
            "File Chooser",
            Box::new(
                FileChooserButton::new()
                    .mode(ChooserMode::Open)
                    .placeholder("Import BOM file…"),
            ),
        ),
        ("Font Button", Box::new(FontButton::new("Inter", 13.0))),
        (
            "Form Field",
            Box::new(
                FormField::new()
                    .label("Sensor ID")
                    .required(true)
                    .hint("e.g. SEN-EAST-4471")
                    .child(TextInput::new("Sensor ID")),
            ),
        ),
        (
            "Inline Edit",
            Box::new(InlineEdit::new("Line 3 — packaging").placeholder("Line name")),
        ),
        (
            "IP Input",
            Box::new(IpInput::new().value([192, 168, 10, 15])),
        ),
        (
            "Key Capture",
            Box::new(KeyCapture::new().placeholder("Press shortcut…")),
        ),
        ("Keypad", Box::new(Keypad::new())),
        (
            "Mention",
            Box::new(
                Mention::new()
                    .suggestions(["aylin.k", "boris.m", "cengiz.t", "derya.s"])
                    .placeholder("Notify @operator…"),
            ),
        ),
        ("OTP Input", Box::new(OtpInput::new().length(6))),
        (
            "Password Strength",
            Box::new(PasswordStrength::new().score(3)),
        ),
        ("Pattern Lock", Box::new(PatternLock::new())),
        (
            "Poll",
            Box::new(
                Poll::new("Run Line 4 this Saturday?")
                    .option(PollOption::new("Yes — full shift", 7))
                    .option(PollOption::new("Yes — half shift", 4))
                    .option(PollOption::new("No", 2)),
            ),
        ),
        (
            "Radio Group",
            Box::new(RadioGroup::new(["Low", "Medium", "High"])),
        ),
        (
            "Range Slider",
            Box::new(RangeSlider::new(0.0, 100.0).with_range(20.0, 80.0)),
        ),
        (
            "Rating",
            Box::new(Rating::new().half_steps(true).value(3.5)),
        ),
        (
            "Search Bar",
            Box::new({
                let mut bar = SearchBar::new().placeholder("Search work orders…");
                bar.search_mode = true;
                bar
            }),
        ),
        (
            "Search Field",
            Box::new(SearchField::new().placeholder("Search parts…")),
        ),
        (
            "Segmented",
            Box::new(
                Segmented::new()
                    .options(["Shift", "Day", "Week"])
                    .selected(1),
            ),
        ),
        (
            "Slider",
            Box::new(
                Slider::new(0.0, 100.0)
                    .label("Target OEE %")
                    .with_value(62.0)
                    .step(1.0),
            ),
        ),
        (
            "Spinbox",
            Box::new(
                SpinBox::new()
                    .range(0.0, 50.0)
                    .step(0.5)
                    .suffix(" mm")
                    .with_value(12.5),
            ),
        ),
        ("Switch", Box::new(Switch::new("Conveyor running").on(true))),
        (
            "Text Area",
            Box::new(
                TextArea::new()
                    .label("Shift notes")
                    .placeholder("Handover notes…")
                    .min_lines(3),
            ),
        ),
        (
            "Text Input",
            Box::new(TextInput::new("Operator ID").placeholder("e.g. OP-4471")),
        ),
        (
            "Time Picker",
            Box::new(
                TimePicker::new()
                    .time(Time { hour: 6, minute: 0 })
                    .use_24h(true)
                    .minute_step(15),
            ),
        ),
        (
            "Toggle Button",
            Box::new(ToggleButton::new("Mute alarms").pressed(true)),
        ),
        (
            "Token Field",
            Box::new(
                TokenField::new()
                    .tokens(["line-3", "urgent", "qa-hold"])
                    .placeholder("Add tag…"),
            ),
        ),
        (
            "Transfer",
            Box::new(
                Transfer::new()
                    .titles("In stock", "Assigned")
                    .source(["Bearing 6204", "Seal kit", "V-belt A68", "Fuse 10A"])
                    .target(["Motor coupler"]),
            ),
        ),
        (
            "Tree Select",
            Box::new(TreeSelect::new().tree(vec![
                TreeNode::new("Plant East").with_children(vec![
                    TreeNode::new("Line 1").with_children(vec![TreeNode::new("CNC-Mill-02")]),
                    TreeNode::new("Line 2"),
                ]),
                TreeNode::new("Plant West"),
            ])),
        ),
        (
            "Unit Converter",
            Box::new(
                UnitConverter::new()
                    .in_category(UnitCategory::Mass)
                    .with_value(2.5),
            ),
        ),
        (
            "Wheel Picker",
            Box::new(
                WheelPicker::new()
                    .items(["Line 1", "Line 2", "Line 3", "Line 4"])
                    .selected_index(2),
            ),
        ),
    ]
}
