//! Input family — text entry and editing widgets.

use martensite::widgets::auto_complete::AutoComplete;
use martensite::widgets::chat_input::ChatInput;
use martensite::widgets::form_field::FormField;
use martensite::widgets::inline_edit::InlineEdit;
use martensite::widgets::ip_input::IpInput;
use martensite::widgets::kbd::Kbd;
use martensite::widgets::key_capture::KeyCapture;
use martensite::widgets::mention::Mention;
use martensite::widgets::otp_input::OtpInput;
use martensite::widgets::password_strength::PasswordStrength;
use martensite::widgets::search_bar::SearchBar;
use martensite::widgets::search_field::SearchField;
use martensite::widgets::slider::Slider;
use martensite::widgets::text_area::TextArea;
use martensite::widgets::text_input::TextInput;
use martensite::widgets::token_field::TokenField;

use crate::page::{Page, PropSpec};
use crate::pages::{csv, downcast_mut, meta, page, SnipProp};

page!(TextInputPage {
    meta: meta(
        "TextInput",
        "Input",
        "Single-line text field — placeholder, icons, validation, secure entry.",
        "TextField",
        &[
            ("Qt", "QLineEdit"),
            ("GTK", "GtkEntry"),
            ("SwiftUI", "TextField"),
            ("React", "<input>")
        ],
        false,
    ),
    props: &[
        PropSpec::Text {
            key: "label",
            label: "Label",
            default: "Email"
        },
        PropSpec::Text {
            key: "placeholder",
            label: "Placeholder",
            default: "you@example.com"
        },
        PropSpec::Bool {
            key: "secure",
            label: "Secure",
            default: false
        },
        PropSpec::Bool {
            key: "clearable",
            label: "Clearable",
            default: false
        },
        PropSpec::Bool {
            key: "enabled",
            label: "Enabled",
            default: true
        },
        PropSpec::Choice {
            key: "sanitize",
            label: "Sanitize",
            options: &["Aggressive", "Baseline", "Raw"],
            default: 0,
        },
        PropSpec::Text {
            key: "value",
            label: "Value",
            default: ""
        },
        PropSpec::Bool {
            key: "read_only",
            label: "Read Only",
            default: false
        },
        PropSpec::Bool {
            key: "revealable",
            label: "Revealable",
            default: false
        },
        PropSpec::Text {
            key: "prefix",
            label: "Prefix",
            default: ""
        },
        PropSpec::Text {
            key: "suffix",
            label: "Suffix",
            default: ""
        },
        PropSpec::Choice {
            key: "validation",
            label: "Validation",
            options: &["Error", "Warning", "Valid"],
            default: 0
        },
        PropSpec::Text {
            key: "validation_message",
            label: "Validation Message",
            default: ""
        },
    ],
    build: |p| {
        let mut w = TextInput::new(p.str("label"))
            .placeholder(p.str("placeholder"))
            .secure(p.bool("secure"))
            .clearable(p.bool("clearable"))
            .enabled(p.bool("enabled"));
        w.set_sanitizer(crate::pages::sanitize_cfg(p));
        {
            let mut __w = w;
            if !p.str("value").is_empty() {
                __w = __w.value(p.str("value"));
            }
            if p.bool("read_only") {
                __w = __w.read_only(p.bool("read_only"));
            }
            if p.bool("revealable") {
                __w = __w.revealable(p.bool("revealable"));
            }
            if !p.str("prefix").is_empty() {
                __w = __w.prefix(p.str("prefix"));
            }
            if !p.str("suffix").is_empty() {
                __w = __w.suffix(p.str("suffix"));
            }
            if p.choice("validation") != 0 {
                __w = __w.validation(match p.choice("validation") {
                    0 => martensite::widgets::text_input::ValidationState::Error,
                    1 => martensite::widgets::text_input::ValidationState::Warning,
                    2 => martensite::widgets::text_input::ValidationState::Valid,
                    _ => martensite::widgets::text_input::ValidationState::Error,
                });
            }
            if !p.str("validation_message").is_empty() {
                __w = __w.validation_message(p.str("validation_message"));
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = {
            let base: String = {
                {
                    let mut s = format!(
                        "TextInput::new({:?})\n    .placeholder({:?})",
                        p.str("label"),
                        p.str("placeholder")
                    );
                    if p.bool("secure") {
                        s.push_str("\n    .secure(true)");
                    }
                    if p.bool("clearable") {
                        s.push_str("\n    .clearable(true)");
                    }
                    if !p.bool("enabled") {
                        s.push_str("\n    .enabled(false)");
                    }
                    s
                }
            };
            base + crate::pages::sanitize_snippet(p)
        };
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("value", ".value", SnipProp::Text("")),
                ("read_only", ".read_only", SnipProp::Bool(false)),
                ("revealable", ".revealable", SnipProp::Bool(false)),
                ("prefix", ".prefix", SnipProp::Text("")),
                ("suffix", ".suffix", SnipProp::Text("")),
                (
                    "validation",
                    ".validation",
                    SnipProp::Choice(&[
                        "martensite::widgets::text_input::ValidationState::Error",
                        "martensite::widgets::text_input::ValidationState::Warning",
                        "martensite::widgets::text_input::ValidationState::Valid",
                    ]),
                ),
                (
                    "validation_message",
                    ".validation_message",
                    SnipProp::Text(""),
                ),
            ],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(ti) = downcast_mut::<TextInput>(w) {
            if ti.take_edited() {
                out.push(format!("edited → \"{}\"", ti.value));
            }
        }
    },
    state: |w| {
        downcast_mut::<TextInput>(w)
            .map(|ti| {
                let mut v = vec![
                    ("value".to_string(), format!("\"{}\"", ti.value)),
                    ("undo".to_string(), ti.can_undo().to_string()),
                    ("redo".to_string(), ti.can_redo().to_string()),
                ];
                if let Some(val) = &ti.validation {
                    v.push(("validation".to_string(), format!("{val:?}")));
                }
                v
            })
            .unwrap_or_default()
    },
});

page!(TextAreaPage {
    meta: meta(
        "TextArea",
        "Input",
        "Multi-line text editor with wrap and scrolling.",
        "TextField",
        &[
            ("Qt", "QTextEdit"),
            ("GTK", "GtkTextView"),
            ("SwiftUI", "TextEditor"),
            ("React", "<textarea>")
        ],
        false,
    ),
    props: &[
        PropSpec::Text {
            key: "placeholder",
            label: "Placeholder",
            default: "Write something…"
        },
        PropSpec::Bool {
            key: "wrap",
            label: "Wrap",
            default: true
        },
        PropSpec::Int {
            key: "min_lines",
            label: "Min lines",
            min: 1,
            max: 10,
            default: 3
        },
        PropSpec::Bool {
            key: "read_only",
            label: "Read-only",
            default: false
        },
        PropSpec::Choice {
            key: "sanitize",
            label: "Sanitize",
            options: &["Aggressive", "Baseline", "Raw"],
            default: 0,
        },
        PropSpec::Text {
            key: "with_value",
            label: "With Value",
            default: ""
        },
        PropSpec::Int {
            key: "max_lines",
            label: "Max Lines",
            min: 0,
            max: 32,
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
        let mut w = TextArea::new()
            .placeholder(p.str("placeholder"))
            .wrap(p.bool("wrap"))
            .min_lines(p.i64("min_lines") as usize)
            .read_only(p.bool("read_only"));
        w.set_sanitizer(crate::pages::sanitize_cfg(p));
        {
            let mut __w = w;
            __w = __w.enabled(p.bool("enabled"));
            if !p.str("a11y_label").is_empty() {
                __w = __w.label(p.str("a11y_label"));
            }
            if !p.str("with_value").is_empty() {
                __w = __w.with_value(p.str("with_value"));
            }
            if p.i64("max_lines") != 0 {
                __w = __w.max_lines(p.i64("max_lines") as usize);
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = {
            let base: String = {
                format!(
                    "TextArea::new()\n    .placeholder({:?})\n    .wrap({})\n    .min_lines({})",
                    p.str("placeholder"),
                    p.bool("wrap"),
                    p.i64("min_lines"),
                )
            };
            base + crate::pages::sanitize_snippet(p)
        };
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("with_value", ".with_value", SnipProp::Text("")),
                ("max_lines", ".max_lines", SnipProp::Int(0)),
                ("enabled", ".enabled", SnipProp::Bool(true)),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(ta) = downcast_mut::<TextArea>(w) {
            if ta.take_edited().is_some() {
                out.push(format!("edited → {} chars", ta.value().len()));
            }
        }
    },
});

page!(SearchFieldPage {
    meta: meta(
        "SearchField",
        "Input",
        "Search entry with round styling and clear affordance.",
        "TextField",
        &[
            ("macOS", "NSSearchField"),
            ("GTK", "GtkSearchEntry"),
            ("Qt", "search QLineEdit"),
            ("React", "<input type=search>")
        ],
        false,
    ),
    props: &[
        PropSpec::Text {
            key: "placeholder",
            label: "Placeholder",
            default: "Search"
        },
        PropSpec::Text {
            key: "value",
            label: "Value",
            default: ""
        },
        PropSpec::Bool {
            key: "enabled",
            label: "Enabled",
            default: true
        },
        PropSpec::Choice {
            key: "sanitize",
            label: "Sanitize",
            options: &["Aggressive", "Baseline", "Raw"],
            default: 0,
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
        let mut w = SearchField::new()
            .placeholder(p.str("placeholder"))
            .with_value(p.str("value"))
            .enabled(p.bool("enabled"));
        w.set_sanitizer(crate::pages::sanitize_cfg(p));
        {
            let mut __w = w;
            if !p.str("a11y_label").is_empty() {
                __w = __w.label(p.str("a11y_label"));
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = {
            let base: String = {
                format!(
                    "SearchField::new()\n    .placeholder({:?})\n    .with_value({:?})",
                    p.str("placeholder"),
                    p.str("value"),
                )
            };
            base + crate::pages::sanitize_snippet(p)
        };
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[("a11y_label", ".label", SnipProp::Text(""))],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(s) = downcast_mut::<SearchField>(w) {
            if let Some(q) = s.take_submitted() {
                out.push(format!("submitted → \"{q}\""));
            }
            if s.take_edited() {
                out.push(format!("edited → \"{}\"", s.value()));
            }
        }
    },
});

page!(SearchBarPage {
    meta: meta(
        "SearchBar",
        "Input",
        "App-level search bar — pill field with close action.",
        "TextField",
        &[
            ("macOS", "toolbar search"),
            ("iOS", "UISearchBar"),
            ("Android", "SearchView"),
            ("React", "search bar")
        ],
        false,
    ),
    props: &[
        PropSpec::Text {
            key: "placeholder",
            label: "Placeholder",
            default: "Search files",
        },
        PropSpec::Choice {
            key: "sanitize",
            label: "Sanitize",
            options: &["Aggressive", "Baseline", "Raw"],
            default: 0,
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
        let mut w = SearchBar::new().placeholder(p.str("placeholder"));
        w.search_mode = true; // demo the revealed field, not the collapsed marker
        w.set_sanitizer(crate::pages::sanitize_cfg(p));
        {
            let mut __w = w;
            __w = __w.enabled(p.bool("enabled"));
            if !p.str("a11y_label").is_empty() {
                __w = __w.a11y_label(p.str("a11y_label"));
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = {
            let base: String =
                { format!("SearchBar::new().placeholder({:?})", p.str("placeholder")) };
            base + crate::pages::sanitize_snippet(p)
        };
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
        if let Some(s) = downcast_mut::<SearchBar>(w) {
            if let Some(q) = s.take_submitted() {
                out.push(format!("submitted → \"{q}\""));
            }
            if s.take_close_requested() {
                out.push("close requested".to_string());
            }
        }
    },
});

page!(AutoCompletePage {
    meta: meta(
        "AutoComplete",
        "Input",
        "Text field with a filtered suggestion popup.",
        "ComboBox",
        &[
            ("Qt", "QCompleter"),
            ("HTML", "<datalist>"),
            ("React", "autocomplete"),
            ("GTK", "entry completion")
        ],
        true,
    ),
    props: &[
        PropSpec::Text {
            key: "placeholder",
            label: "Placeholder",
            default: "Type a fruit…"
        },
        PropSpec::Text {
            key: "suggestions",
            label: "Suggestions (csv)",
            default: "Apple,Apricot,Banana,Cherry,Grape,Mango",
        },
        PropSpec::Choice {
            key: "sanitize",
            label: "Sanitize",
            options: &["Aggressive", "Baseline", "Raw"],
            default: 0,
        },
        PropSpec::Choice {
            key: "filter_mode",
            label: "Filter Mode",
            options: &["Substring", "Prefix"],
            default: 0
        },
        PropSpec::Int {
            key: "min_chars",
            label: "Min Chars",
            min: 0,
            max: 32,
            default: 0
        },
        PropSpec::Int {
            key: "max_visible",
            label: "Max Visible",
            min: 0,
            max: 32,
            default: 0
        },
        PropSpec::Text {
            key: "with_value",
            label: "With Value",
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
        {
            let mut w = AutoComplete::new()
                .placeholder(p.str("placeholder"))
                .suggestions(csv(p, "suggestions"));
            w.set_sanitizer(crate::pages::sanitize_cfg(p));
            {
                let mut __w = w;
                __w = __w.enabled(p.bool("enabled"));
                __w = __w.loading(p.bool("loading"));
                if !p.str("a11y_label").is_empty() {
                    __w = __w.label(p.str("a11y_label"));
                }
                if p.choice("filter_mode") != 0 {
                    __w = __w.filter_mode(match p.choice("filter_mode") {
                        0 => martensite::widgets::auto_complete::FilterMode::Substring,
                        1 => martensite::widgets::auto_complete::FilterMode::Prefix,
                        _ => martensite::widgets::auto_complete::FilterMode::Substring,
                    });
                }
                if p.i64("min_chars") != 0 {
                    __w = __w.min_chars(p.i64("min_chars") as usize);
                }
                if p.i64("max_visible") != 0 {
                    __w = __w.max_visible(p.i64("max_visible") as usize);
                }
                if !p.str("with_value").is_empty() {
                    __w = __w.with_value(p.str("with_value"));
                }
                Box::new(__w)
            }
        }
    },
    snippet: |p| {
        let mut __s = {
            let base: String = {
                {
                    let sg = csv(p, "suggestions")
                        .iter()
                        .map(|s| format!("{s:?}"))
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!(
                        "AutoComplete::new()\n    .placeholder({:?})\n    .suggestions([{sg}])",
                        p.str("placeholder"),
                    )
                }
            };
            base + crate::pages::sanitize_snippet(p)
        };
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                (
                    "filter_mode",
                    ".filter_mode",
                    SnipProp::Choice(&[
                        "martensite::widgets::auto_complete::FilterMode::Substring",
                        "martensite::widgets::auto_complete::FilterMode::Prefix",
                    ]),
                ),
                ("min_chars", ".min_chars", SnipProp::Int(0)),
                ("max_visible", ".max_visible", SnipProp::Int(0)),
                ("with_value", ".with_value", SnipProp::Text("")),
                ("enabled", ".enabled", SnipProp::Bool(true)),
                ("loading", ".loading", SnipProp::Bool(false)),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(a) = downcast_mut::<AutoComplete>(w) {
            if let Some(v) = a.take_committed() {
                out.push(format!("committed → \"{v}\""));
            }
            if a.take_edited() {
                out.push(format!("edited → \"{}\"", a.value()));
            }
        }
    },
});

page!(ChatInputPage {
    meta: meta(
        "ChatInput",
        "Input",
        "Message composer — attachment, emoji, send.",
        "TextField",
        &[
            ("Slack", "composer"),
            ("Qt", "chat field"),
            ("React", "message input"),
            ("iOS", "iMessage field")
        ],
        false,
    ),
    props: &[
        PropSpec::Text {
            key: "placeholder",
            label: "Placeholder",
            default: "Message #general"
        },
        PropSpec::Bool {
            key: "attachable",
            label: "Attach",
            default: true
        },
        PropSpec::Bool {
            key: "emoji",
            label: "Emoji button",
            default: true
        },
        PropSpec::Choice {
            key: "sanitize",
            label: "Sanitize",
            options: &["Aggressive", "Baseline", "Raw"],
            default: 0,
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
        let mut w = ChatInput::new()
            .placeholder(p.str("placeholder"))
            .attachable(p.bool("attachable"))
            .emoji_button(p.bool("emoji"));
        w.set_sanitizer(crate::pages::sanitize_cfg(p));
        {
            let mut __w = w;
            __w = __w.enabled(p.bool("enabled"));
            if !p.str("a11y_label").is_empty() {
                __w = __w.label(p.str("a11y_label"));
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = {
            let base: String = {
                format!(
            "ChatInput::new()\n    .placeholder({:?})\n    .attachable({})\n    .emoji_button({})",
            p.str("placeholder"),
            p.bool("attachable"),
            p.bool("emoji"),
            )
            };
            base + crate::pages::sanitize_snippet(p)
        };
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
        if let Some(c) = downcast_mut::<ChatInput>(w) {
            if let Some(msg) = c.take_sent() {
                out.push(format!("sent → \"{msg}\""));
            }
            if c.take_attach() {
                out.push("attach".to_string());
            }
            if c.take_emoji() {
                out.push("emoji".to_string());
            }
        }
        // chat attach/emoji flags are booleans, not Options
    },
});

page!(InlineEditPage {
    meta: meta(
        "InlineEdit",
        "Input",
        "Click-to-edit label — label ↔ field swap.",
        "TextField",
        &[
            ("GTK", "editable label"),
            ("macOS", "rename in place"),
            ("React", "inline edit"),
            ("Qt", "editable item")
        ],
        false,
    ),
    props: &[
        PropSpec::Text {
            key: "value",
            label: "Value",
            default: "untitled.txt",
        },
        PropSpec::Choice {
            key: "sanitize",
            label: "Sanitize",
            options: &["Aggressive", "Baseline", "Raw"],
            default: 0,
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
        PropSpec::Text {
            key: "a11y_label",
            label: "A11y label",
            default: ""
        },
    ],
    build: |p| {
        let mut w = InlineEdit::new(p.str("value"));
        w.set_sanitizer(crate::pages::sanitize_cfg(p));
        {
            let mut __w = w;
            __w = __w.enabled(p.bool("enabled"));
            if !p.str("a11y_label").is_empty() {
                __w = __w.a11y_label(p.str("a11y_label"));
            }
            if !p.str("placeholder").is_empty() {
                __w = __w.placeholder(p.str("placeholder"));
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = {
            let base: String = { format!("InlineEdit::new({:?})", p.str("value")) };
            base + crate::pages::sanitize_snippet(p)
        };
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("placeholder", ".placeholder", SnipProp::Text("")),
                ("enabled", ".enabled", SnipProp::Bool(true)),
                ("a11y_label", ".a11y_label", SnipProp::Text("")),
            ],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(e) = downcast_mut::<InlineEdit>(w) {
            if let Some((old, new)) = e.take_committed() {
                out.push(format!("committed \"{old}\" → \"{new}\""));
            }
        }
    },
});

page!(IpInputPage {
    meta: meta(
        "IpInput",
        "Input",
        "Four-octet IPv4 address entry.",
        "TextField",
        &[
            ("Qt", "masked input"),
            ("Win32", "IP control"),
            ("React", "IP field"),
            ("GTK", "custom")
        ],
        false,
    ),
    props: &[
        PropSpec::Text {
            key: "label",
            label: "Label",
            default: "Server"
        },
        PropSpec::Choice {
            key: "sanitize",
            label: "Sanitize",
            options: &["Aggressive", "Baseline", "Raw"],
            default: 0,
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
        let mut w = IpInput::new()
            .label(p.str("label"))
            .value([192, 168, 1, 10]);
        w.set_sanitizer(crate::pages::sanitize_cfg(p));
        {
            let mut __w = w;
            __w = __w.enabled(p.bool("enabled"));
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = {
            let base: String = {
                format!(
                    "IpInput::new().label({:?}).value([192, 168, 1, 10])",
                    p.str("label")
                )
            };
            base + crate::pages::sanitize_snippet(p)
        };
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[("enabled", ".enabled", SnipProp::Bool(true))],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(i) = downcast_mut::<IpInput>(w) {
            if i.take_changed() {
                out.push(format!("addr → {}", i.text()));
            }
        }
    },
});

page!(OtpInputPage {
    meta: meta(
        "OtpInput",
        "Input",
        "One-time-code segmented input.",
        "TextField",
        &[
            ("iOS", "OTP field"),
            ("Android", "SMS code"),
            ("React", "otp input"),
            ("HTML", "one-time-code")
        ],
        false,
    ),
    props: &[
        PropSpec::Int {
            key: "length",
            label: "Length",
            min: 4,
            max: 8,
            default: 6
        },
        PropSpec::Bool {
            key: "masked",
            label: "Masked",
            default: false
        },
        PropSpec::Choice {
            key: "sanitize",
            label: "Sanitize",
            options: &["Aggressive", "Baseline", "Raw"],
            default: 0,
        },
        PropSpec::Text {
            key: "value",
            label: "Value",
            default: ""
        },
        PropSpec::Bool {
            key: "alphabetic",
            label: "Alphabetic",
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
        PropSpec::Text {
            key: "a11y_label",
            label: "A11y label",
            default: ""
        },
    ],
    build: |p| {
        let mut w = OtpInput::new()
            .length(p.i64("length") as usize)
            .masked(p.bool("masked"));
        w.set_sanitizer(crate::pages::sanitize_cfg(p));
        {
            let mut __w = w;
            __w = __w.enabled(p.bool("enabled"));
            if !p.str("a11y_label").is_empty() {
                __w = __w.a11y_label(p.str("a11y_label"));
            }
            if !p.str("value").is_empty() {
                __w = __w.value(p.str("value"));
            }
            if p.bool("alphabetic") {
                __w = __w.alphabetic(p.bool("alphabetic"));
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = {
            let base: String = {
                format!(
                    "OtpInput::new().length({}).masked({})",
                    p.i64("length"),
                    p.bool("masked"),
                )
            };
            base + crate::pages::sanitize_snippet(p)
        };
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("value", ".value", SnipProp::Text("")),
                ("alphabetic", ".alphabetic", SnipProp::Bool(false)),
                ("enabled", ".enabled", SnipProp::Bool(true)),
                ("a11y_label", ".a11y_label", SnipProp::Text("")),
            ],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(o) = downcast_mut::<OtpInput>(w) {
            if let Some(code) = o.take_completed() {
                out.push(format!("completed → {code}"));
            }
        }
    },
});

page!(KeyCapturePage {
    meta: meta(
        "KeyCapture",
        "Input",
        "Shortcut recorder — captures the next key chord.",
        "Button",
        &[
            ("macOS", "shortcut recorder"),
            ("Qt", "QKeySequenceEdit"),
            ("GTK", "accel button"),
            ("React", "key capture")
        ],
        false,
    ),
    props: &[
        PropSpec::Text {
            key: "placeholder",
            label: "Placeholder",
            default: "Press shortcut…",
        },
        PropSpec::Text {
            key: "shortcut",
            label: "Shortcut",
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
        let mut __w = KeyCapture::new().placeholder(p.str("placeholder"));
        __w = __w.enabled(p.bool("enabled"));
        if !p.str("a11y_label").is_empty() {
            __w = __w.a11y_label(p.str("a11y_label"));
        }
        if !p.str("shortcut").is_empty() {
            __w = __w.shortcut(p.str("shortcut"));
        }
        Box::new(__w)
    },
    snippet: |p| {
        let mut __s = format!("KeyCapture::new().placeholder({:?})", p.str("placeholder"));
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("shortcut", ".shortcut", SnipProp::Text("")),
                ("enabled", ".enabled", SnipProp::Bool(true)),
                ("a11y_label", ".a11y_label", SnipProp::Text("")),
            ],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(k) = downcast_mut::<KeyCapture>(w) {
            if let Some(s) = k.take_recorded() {
                out.push(format!("shortcut → {s}"));
            }
        }
    },
});

page!(KbdPage {
    meta: meta(
        "Kbd",
        "Input",
        "Keyboard-key cap badge — ⌘K style.",
        "Text",
        &[
            ("HTML", "<kbd>"),
            ("Docs", "key cap"),
            ("React", "<Kbd>"),
            ("Qt", "styled label")
        ],
        false,
    ),
    props: &[
        PropSpec::Text {
            key: "text",
            label: "Text",
            default: "⌘S"
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
        let mut __w = Kbd::new(p.str("text"));
        __w = __w.enabled(p.bool("enabled"));
        if !p.str("a11y_label").is_empty() {
            __w = __w.label(p.str("a11y_label"));
        }
        Box::new(__w)
    },
    snippet: |p| {
        let mut __s = format!("Kbd::new({:?})", p.str("text"));
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("enabled", ".enabled", SnipProp::Bool(true)),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        __s
    },
});

page!(MentionPage {
    meta: meta(
        "Mention",
        "Input",
        "@-mention field with suggestion popup.",
        "TextField",
        &[
            ("Slack", "@mention"),
            ("Twitter", "mention"),
            ("React", "mentions"),
            ("Qt", "custom")
        ],
        true,
    ),
    props: &[
        PropSpec::Text {
            key: "placeholder",
            label: "Placeholder",
            default: "Mention someone…"
        },
        PropSpec::Text {
            key: "trigger",
            label: "Trigger",
            default: "@"
        },
        PropSpec::Choice {
            key: "sanitize",
            label: "Sanitize",
            options: &["Aggressive", "Baseline", "Raw"],
            default: 0,
        },
        PropSpec::Int {
            key: "min_chars",
            label: "Min Chars",
            min: 0,
            max: 32,
            default: 0
        },
        PropSpec::Int {
            key: "max_visible",
            label: "Max Visible",
            min: 0,
            max: 32,
            default: 0
        },
        PropSpec::Text {
            key: "with_value",
            label: "With Value",
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
        let trig = p.str("trigger").chars().next().unwrap_or('@');
        {
            let mut w = Mention::new()
                .placeholder(p.str("placeholder"))
                .trigger(trig)
                .suggestions(["@ada", "@grace", "@linus", "@turing"]);
            w.set_sanitizer(crate::pages::sanitize_cfg(p));
            {
                let mut __w = w;
                __w = __w.enabled(p.bool("enabled"));
                __w = __w.loading(p.bool("loading"));
                if !p.str("a11y_label").is_empty() {
                    __w = __w.label(p.str("a11y_label"));
                }
                if p.i64("min_chars") != 0 {
                    __w = __w.min_chars(p.i64("min_chars") as usize);
                }
                if p.i64("max_visible") != 0 {
                    __w = __w.max_visible(p.i64("max_visible") as usize);
                }
                if !p.str("with_value").is_empty() {
                    __w = __w.with_value(p.str("with_value"));
                }
                Box::new(__w)
            }
        }
    },
    snippet: |p| {
        let mut __s = {
            let base: String = {
                format!(
                    "Mention::new()\n    .placeholder({:?})\n    .trigger('{}')",
                    p.str("placeholder"),
                    p.str("trigger"),
                )
            };
            base + crate::pages::sanitize_snippet(p)
        };
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("min_chars", ".min_chars", SnipProp::Int(0)),
                ("max_visible", ".max_visible", SnipProp::Int(0)),
                ("with_value", ".with_value", SnipProp::Text("")),
                ("enabled", ".enabled", SnipProp::Bool(true)),
                ("loading", ".loading", SnipProp::Bool(false)),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(m) = downcast_mut::<Mention>(w) {
            if let Some(v) = m.take_committed() {
                out.push(format!("committed → \"{v}\""));
            }
        }
    },
});

page!(TokenFieldPage {
    meta: meta(
        "TokenField",
        "Input",
        "Tokenized tag field — type, delimiter, chip.",
        "TextField",
        &[
            ("macOS", "NSTokenField"),
            ("Qt", "tag input"),
            ("React", "token input"),
            ("iOS", "Mail To field")
        ],
        false,
    ),
    props: &[
        PropSpec::Text {
            key: "placeholder",
            label: "Placeholder",
            default: "Add tag"
        },
        PropSpec::Text {
            key: "tokens",
            label: "Tokens (csv)",
            default: "ui,theme"
        },
        PropSpec::Choice {
            key: "sanitize",
            label: "Sanitize",
            options: &["Aggressive", "Baseline", "Raw"],
            default: 0,
        },
        PropSpec::Text {
            key: "delimiters",
            label: "Delimiters",
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
        let mut w = TokenField::new()
            .placeholder(p.str("placeholder"))
            .tokens(csv(p, "tokens"));
        w.set_sanitizer(crate::pages::sanitize_cfg(p));
        {
            let mut __w = w;
            __w = __w.enabled(p.bool("enabled"));
            if !p.str("delimiters").is_empty() {
                __w = __w.delimiters(p.str("delimiters").chars().collect::<Vec<_>>());
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = {
            let base: String = {
                {
                    let toks = csv(p, "tokens")
                        .iter()
                        .map(|t| format!("{t:?}"))
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!(
                        "TokenField::new()\n    .placeholder({:?})\n    .tokens([{toks}])",
                        p.str("placeholder"),
                    )
                }
            };
            base + crate::pages::sanitize_snippet(p)
        };
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[("enabled", ".enabled", SnipProp::Bool(true))],
        ));
        __s.push_str(&crate::pages::snip_textmap(
            p,
            "delimiters",
            ".delimiters",
            "",
            crate::pages::expr_strs,
        ));
        __s
    },
    poll: |w, out| {
        if let Some(t) = downcast_mut::<TokenField>(w) {
            if let Some(tok) = t.take_added() {
                out.push(format!("added → \"{tok}\""));
            }
            if let Some(i) = t.take_removed() {
                out.push(format!("removed index {i}"));
            }
        }
    },
});

page!(FormFieldPage {
    meta: meta(
        "FormField",
        "Input",
        "Label + control + hint/error wrapper — form row.",
        "Group",
        &[
            ("Qt", "QFormLayout row"),
            ("GTK", "preferences row"),
            ("React", "<FormItem>"),
            ("HTML", "label+field")
        ],
        false,
    ),
    props: &[
        PropSpec::Text {
            key: "label",
            label: "Label",
            default: "Volume"
        },
        PropSpec::Text {
            key: "hint",
            label: "Hint",
            default: "Master output level"
        },
        PropSpec::Bool {
            key: "required",
            label: "Required",
            default: false
        },
        PropSpec::Choice {
            key: "label_position",
            label: "Label Position",
            options: &["Top", "Left"],
            default: 0
        },
        PropSpec::Float {
            key: "label_width",
            label: "Label Width",
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
    ],
    build: |p| {
        let mut __w = FormField::new()
            .label(p.str("label"))
            .hint(p.str("hint"))
            .required(p.bool("required"))
            .child(Slider::new(0.0, 1.0).with_value(0.6));
        __w = __w.enabled(p.bool("enabled"));
        if p.choice("label_position") != 0 {
            __w = __w.label_position(match p.choice("label_position") {
                0 => martensite::widgets::form_field::LabelPosition::Top,
                1 => martensite::widgets::form_field::LabelPosition::Left,
                _ => martensite::widgets::form_field::LabelPosition::Top,
            });
        }
        if p.f64("label_width") != 0.0 {
            __w = __w.label_width(p.f64("label_width") as f32);
        }
        Box::new(__w)
    },
    snippet: |p| {
        let mut __s = format!(
        "FormField::new()\n    .label({:?})\n    .hint({:?})\n    .child(Slider::new(0.0, 1.0))",
        p.str("label"),
        p.str("hint"),
    );
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                (
                    "label_position",
                    ".label_position",
                    SnipProp::Choice(&[
                        "martensite::widgets::form_field::LabelPosition::Top",
                        "martensite::widgets::form_field::LabelPosition::Left",
                    ]),
                ),
                ("label_width", ".label_width", SnipProp::Float(0.0)),
                ("enabled", ".enabled", SnipProp::Bool(true)),
            ],
        ));
        __s
    },
});

page!(PasswordStrengthPage {
    meta: meta(
        "PasswordStrength",
        "Input",
        "Password-strength meter — weak→strong bar.",
        "LevelBar",
        &[
            ("Web", "password meter"),
            ("Qt", "custom"),
            ("React", "strength bar"),
            ("1Password", "meter")
        ],
        false,
    ),
    props: &[
        PropSpec::Int {
            key: "score",
            label: "Score (0-4)",
            min: 0,
            max: 4,
            default: 2,
        },
        PropSpec::Bool {
            key: "label_visible",
            label: "Label Visible",
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
        let mut __w = PasswordStrength::new().score(p.i64("score") as u8);
        if !p.str("a11y_label").is_empty() {
            __w = __w.label(p.str("a11y_label"));
        }
        if p.bool("label_visible") {
            __w = __w.label_visible(p.bool("label_visible"));
        }
        Box::new(__w)
    },
    snippet: |p| {
        let mut __s = format!("PasswordStrength::new().score({})", p.i64("score"));
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("label_visible", ".label_visible", SnipProp::Bool(false)),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        __s
    },
    state: |w| {
        downcast_mut::<PasswordStrength>(w)
            .map(|s| vec![("score".to_string(), s.score_value().to_string())])
            .unwrap_or_default()
    },
});

/// All Input pages, in rail order.
pub(super) fn pages() -> Vec<Box<dyn Page>> {
    vec![
        Box::new(TextInputPage),
        Box::new(TextAreaPage),
        Box::new(SearchFieldPage),
        Box::new(SearchBarPage),
        Box::new(AutoCompletePage),
        Box::new(ChatInputPage),
        Box::new(InlineEditPage),
        Box::new(IpInputPage),
        Box::new(OtpInputPage),
        Box::new(KeyCapturePage),
        Box::new(KbdPage),
        Box::new(MentionPage),
        Box::new(TokenFieldPage),
        Box::new(FormFieldPage),
        Box::new(PasswordStrengthPage),
    ]
}
