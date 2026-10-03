//! Feedback family — status indicators, progress, and empty/result
//! surfaces.

use std::time::Duration;

use martensite::widgets::about::About;
use martensite::widgets::activity_ring::ActivityRing;
use martensite::widgets::alarm_panel::{Alarm, AlarmPanel};
use martensite::widgets::badge::{Badge, BadgeSeverity};
use martensite::widgets::banner::{Banner, Severity};
use martensite::widgets::cookie_banner::CookieBanner;
use martensite::widgets::countdown::Countdown;
use martensite::widgets::countdown_ring::CountdownRing;
use martensite::widgets::empty_state::EmptyState;
use martensite::widgets::level_bar::LevelBar;
use martensite::widgets::marquee::Marquee;
use martensite::widgets::notification_center::{Notification, NotificationCenter};
use martensite::widgets::progress::{ProgressBar, Spinner};
use martensite::widgets::release_notes::{ChangeKind, Release, ReleaseNotes};
use martensite::widgets::result_page::{ResultPage, ResultStatus};
use martensite::widgets::skeleton::Skeleton;
use martensite::widgets::splash::Splash;
use martensite::widgets::stack_light::{Lamp, StackLight};
use martensite::widgets::status_bar::StatusBar;
use martensite::widgets::status_dot::{Status, StatusDot};
use martensite::widgets::ticker_tape::{TickerItem, TickerTape};
use martensite::widgets::toast::{Toast, ToastHost};
use martensite::widgets::typing_indicator::TypingIndicator;
use martensite::widgets::update_prompt::UpdatePrompt;
use martensite::widgets::watermark::Watermark;

use crate::page::{Page, PropSpec};
use crate::pages::{downcast_mut, meta, page, SnipProp};

page!(ProgressBarPage {
    meta: meta(
        "ProgressBar",
        "Feedback",
        "Determinate or indeterminate linear progress.",
        "ProgressBar",
        &[
            ("Qt", "QProgressBar"),
            ("GTK", "GtkProgressBar"),
            ("SwiftUI", "ProgressView"),
            ("HTML", "<progress>")
        ],
        false,
    ),
    props: &[PropSpec::Float {
        key: "value",
        label: "Value",
        min: 0.0,
        max: 1.0,
        step: 0.05,
        default: 0.62,
    }],
    build: |p| Box::new(ProgressBar::new().value(p.f64("value") as f32)),
    snippet: |p| format!("ProgressBar::new().value({:?})", p.f64("value") as f32),
    state: |w| {
        downcast_mut::<ProgressBar>(w)
            .map(|b| vec![("fraction".to_string(), format!("{:?}", b.fraction()))])
            .unwrap_or_default()
    },
});

page!(SpinnerPage {
    meta: meta(
        "Spinner",
        "Feedback",
        "Indeterminate activity spinner.",
        "ProgressBar",
        &[
            ("Qt", "busy indicator"),
            ("GTK", "GtkSpinner"),
            ("SwiftUI", "ProgressView()"),
            ("React", "spinner")
        ],
        false,
    ),
    props: &[
        PropSpec::Float {
            key: "size",
            label: "Size",
            min: 12.0,
            max: 64.0,
            step: 2.0,
            default: 24.0
        },
        PropSpec::Bool {
            key: "active",
            label: "Active",
            default: true
        },
        PropSpec::Float {
            key: "speed",
            label: "Speed",
            min: 0.0,
            max: 10.0,
            step: 0.1,
            default: 1.25
        },
        PropSpec::Float {
            key: "thickness",
            label: "Thickness",
            min: 0.0,
            max: 64.0,
            step: 0.5,
            default: 2.0
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
        let mut s = Spinner::new().size(p.f64("size") as f32);
        if p.bool("active") {
            s.start();
        }
        {
            let mut __w = s;
            if !p.str("a11y_label").is_empty() {
                __w = __w.label(p.str("a11y_label"));
            }
            if p.f64("speed") != 1.25 {
                __w = __w.speed(p.f64("speed") as f32);
            }
            if p.f64("thickness") != 2.0 {
                __w = __w.thickness(p.f64("thickness") as f32);
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = format!("Spinner::new().size({:?})", p.f64("size") as f32);
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("speed", ".speed", SnipProp::Float(1.25)),
                ("thickness", ".thickness", SnipProp::Float(2.0)),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        __s
    },
});

page!(BadgePage {
    meta: meta(
        "Badge",
        "Feedback",
        "Count or dot marker on a control.",
        "Text",
        &[
            ("iOS", "badge"),
            ("Material", "Badge"),
            ("Qt", "custom"),
            ("React", "<Badge>")
        ],
        false,
    ),
    props: &[
        PropSpec::Int {
            key: "count",
            label: "Count",
            min: 0,
            max: 999,
            default: 7
        },
        PropSpec::Choice {
            key: "severity",
            label: "Severity",
            options: &["Accent", "Info", "Ok", "Warning", "Error"],
            default: 0,
        },
        PropSpec::Bool {
            key: "dot",
            label: "Dot only",
            default: false
        },
        PropSpec::Int {
            key: "max",
            label: "Max",
            min: 0,
            max: 100,
            default: 99
        },
        PropSpec::Text {
            key: "color",
            label: "Color",
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
        let sev = match p.choice("severity") {
            1 => BadgeSeverity::Info,
            2 => BadgeSeverity::Ok,
            3 => BadgeSeverity::Warning,
            _ => BadgeSeverity::Accent,
        };
        let mut b = Badge::new(p.i64("count") as u32).severity(sev);
        if p.bool("dot") {
            b = b.dot(true);
        }
        {
            let mut __w = b;
            if !p.str("a11y_label").is_empty() {
                __w = __w.a11y_label(p.str("a11y_label"));
            }
            if p.i64("max") != 99 {
                __w = __w.max(p.i64("max") as u32);
            }
            if let Some([r, g, b, _]) = crate::pages::parse_rgba(p.str("color")) {
                __w = __w.color(martensite_theme::Oklab::from_srgb(
                    r as f32 / 255.0,
                    g as f32 / 255.0,
                    b as f32 / 255.0,
                ));
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = format!(
            "Badge::new(\"\").severity(BadgeSeverity::{}).with_count({})",
            ["Accent", "Info", "Ok", "Warning", "Error"][p.choice("severity")],
            p.i64("count"),
        );
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("max", ".max", SnipProp::Int(99)),
                ("a11y_label", ".a11y_label", SnipProp::Text("")),
            ],
        ));
        __s.push_str(&crate::pages::snip_textmap(
            p,
            "color",
            ".color",
            "",
            crate::pages::expr_oklab,
        ));
        __s
    },
});

page!(BannerPage {
    meta: meta(
        "Banner",
        "Feedback",
        "Dismissible severity strip pinned at content top.",
        "Group",
        &[
            ("GNOME", "AdwBanner"),
            ("iOS", "banner"),
            ("React", "alert bar"),
            ("Qt", "info bar")
        ],
        false,
    ),
    props: &[
        PropSpec::Choice {
            key: "severity",
            label: "Severity",
            options: &["Info", "Warning", "Error"],
            default: 1,
        },
        PropSpec::Text {
            key: "message",
            label: "Message",
            default: "Update available"
        },
        PropSpec::Bool {
            key: "dismissible",
            label: "Dismissible",
            default: true
        },
    ],
    build: |p| {
        let sev = match p.choice("severity") {
            0 => Severity::Info,
            2 => Severity::Error,
            _ => Severity::Warning,
        };
        Box::new(Banner::new(sev, p.str("message")).dismissible(p.bool("dismissible")))
    },
    snippet: |p| format!(
        "Banner::new(Severity::{}, {:?}).dismissible({})",
        ["Info", "Warning", "Error"][p.choice("severity")],
        p.str("message"),
        p.bool("dismissible"),
    ),
    poll: |w, out| {
        if downcast_mut::<Banner>(w).is_some_and(|b| b.take_dismissed()) {
            out.push("dismissed".to_string());
        }
    },
});

page!(SkeletonPage {
    meta: meta(
        "Skeleton",
        "Feedback",
        "Shimmering placeholder blocks while content loads.",
        "Group",
        &[
            ("React", "skeleton"),
            ("Flutter", "shimmer"),
            ("Qt", "placeholder"),
            ("iOS", "redacted")
        ],
        false,
    ),
    props: &[
        PropSpec::Choice {
            key: "shape",
            label: "Shape",
            options: &["Block", "Circle", "Lines", "Rows", "Grid"],
            default: 2,
        },
        PropSpec::Bool {
            key: "animated",
            label: "Animated",
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
        let s = match p.choice("shape") {
            0 => Skeleton::block(),
            1 => Skeleton::circle(),
            3 => Skeleton::rows(3),
            4 => Skeleton::grid(3, 2),
            _ => Skeleton::lines(3),
        };
        {
            let mut __w = s;
            if !p.str("a11y_label").is_empty() {
                __w = __w.a11y_label(p.str("a11y_label"));
            }
            if p.bool("animated") {
                __w = __w.animated(p.bool("animated"));
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = format!(
            "Skeleton::{}()",
            ["block", "circle", "lines(3)", "rows(3)", "grid(3, 2)"][p.choice("shape")],
        );
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("animated", ".animated", SnipProp::Bool(false)),
                ("a11y_label", ".a11y_label", SnipProp::Text("")),
            ],
        ));
        __s
    },
});

page!(EmptyStatePage {
    meta: meta(
        "EmptyState",
        "Feedback",
        "Zero-state placeholder — icon, title, description, action.",
        "Group",
        &[
            ("React", "empty state"),
            ("GNOME", "AdwStatusPage"),
            ("Qt", "placeholder"),
            ("iOS", "empty view")
        ],
        false,
    ),
    props: &[
        PropSpec::Text {
            key: "title",
            label: "Title",
            default: "No messages"
        },
        PropSpec::Text {
            key: "desc",
            label: "Description",
            default: "Start a conversation"
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
        PropSpec::Text {
            key: "icon_named",
            label: "Icon Named",
            default: ""
        },
    ],
    build: |p| {
        let mut __w = EmptyState::new(p.str("title"))
            .description(p.str("desc"))
            .action("New message");
        if !p.str("icon").is_empty() {
            __w = __w.icon(p.str("icon"));
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
        let mut __s = format!(
            "EmptyState::new({:?})\n    .description({:?})\n    .action(\"New message\")",
            p.str("title"),
            p.str("desc"),
        );
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("icon", ".icon", SnipProp::Text("")),
                ("icon_d", ".icon_d", SnipProp::Text("")),
                ("icon_named", ".icon_named", SnipProp::Text("")),
            ],
        ));
        __s
    },
    poll: |w, out| {
        if downcast_mut::<EmptyState>(w).is_some_and(|e| e.take_activated()) {
            out.push("action".to_string());
        }
    },
});

page!(ResultPagePage {
    meta: meta(
        "ResultPage",
        "Feedback",
        "Full-surface result — success/warning/error/info.",
        "Group",
        &[
            ("Ant", "Result"),
            ("React", "result page"),
            ("Qt", "status page"),
            ("GNOME", "status page")
        ],
        false,
    ),
    props: &[
        PropSpec::Choice {
            key: "status",
            label: "Status",
            options: &["Success", "Warning", "Error", "Info"],
            default: 0,
        },
        PropSpec::Text {
            key: "action",
            label: "Action",
            default: ""
        },
    ],
    build: |p| {
        let st = match p.choice("status") {
            1 => ResultStatus::Warning,
            2 => ResultStatus::Error,
            3 => ResultStatus::Info,
            _ => ResultStatus::Success,
        };
        {
            let mut __w = ResultPage::new(st)
                .title("Operation complete")
                .subtitle("The file was processed without issues.");
            if !p.str("action").is_empty() {
                __w = __w.action(p.str("action"));
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = format!(
            "ResultPage::new(ResultStatus::{})",
            ["Success", "Warning", "Error", "Info"][p.choice("status")],
        );
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[("action", ".action", SnipProp::Text(""))],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(rp) = downcast_mut::<ResultPage>(w) {
            if let Some(action) = rp.take_activated() {
                out.push(format!("action {action:?}"));
            }
        }
    },
});

page!(StatusDotPage {
    meta: meta(
        "StatusDot",
        "Feedback",
        "Pulsing status indicator dot + label.",
        "Text",
        &[
            ("DevOps", "status light"),
            ("Qt", "LED"),
            ("React", "status dot"),
            ("OT", "stack light")
        ],
        false,
    ),
    props: &[
        PropSpec::Text {
            key: "text",
            label: "Text",
            default: "Online"
        },
        PropSpec::Choice {
            key: "status",
            label: "Status",
            options: &["Off", "Info", "Ok", "Warning", "Error"],
            default: 2,
        },
        PropSpec::Bool {
            key: "pulse",
            label: "Pulse",
            default: false
        },
        PropSpec::Bool {
            key: "glyph",
            label: "Glyph",
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
        PropSpec::Bool {
            key: "loading",
            label: "Loading",
            default: false
        },
    ],
    build: |p| {
        let st = match p.choice("status") {
            0 => Status::Off,
            1 => Status::Info,
            3 => Status::Warning,
            4 => Status::Error,
            _ => Status::Ok,
        };
        {
            let mut __w = StatusDot::new(p.str("text")).status(st);
            __w = __w.enabled(p.bool("enabled"));
            __w = __w.loading(p.bool("loading"));
            if p.bool("pulse") {
                __w = __w.pulse(p.bool("pulse"));
            }
            if !p.bool("glyph") {
                __w = __w.glyph(p.bool("glyph"));
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = format!(
            "StatusDot::new({:?}).status(Status::{})",
            p.str("text"),
            ["Off", "Info", "Ok", "Warning", "Error"][p.choice("status")],
        );
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("pulse", ".pulse", SnipProp::Bool(false)),
                ("glyph", ".glyph", SnipProp::Bool(true)),
                ("enabled", ".enabled", SnipProp::Bool(true)),
                ("loading", ".loading", SnipProp::Bool(false)),
            ],
        ));
        __s
    },
});

page!(StatusBarPage {
    meta: meta(
        "StatusBar",
        "Feedback",
        "Bottom-edge status strip — zones, transient messages.",
        "Toolbar",
        &[
            ("Win32", "status bar"),
            ("Qt", "QStatusBar"),
            ("GNOME", "toast bar"),
            ("React", "footer bar")
        ],
        false,
    ),
    props: &[
        PropSpec::Text {
            key: "message",
            label: "Message",
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
    build: |_p| {
        let mut sb = StatusBar::new().label("Status");
        sb.set_message("Ready");
        {
            let mut __w = sb;
            __w = __w.enabled(_p.bool("enabled"));
            if !_p.str("message").is_empty() {
                __w = __w.message(_p.str("message"));
            }
            Box::new(__w)
        }
    },
    snippet: |_p| {
        let mut __s = "StatusBar::new().label(\"Status\")".to_string();
        __s.push_str(&crate::pages::prop_snippet(
            _p,
            &[
                ("message", ".message", SnipProp::Text("")),
                ("enabled", ".enabled", SnipProp::Bool(true)),
            ],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(sb) = downcast_mut::<StatusBar>(w) {
            if let Some(zone) = sb.take_activated() {
                out.push(format!("zone {zone}"));
            }
        }
    },
});

page!(TypingIndicatorPage {
    meta: meta(
        "TypingIndicator",
        "Feedback",
        "Animated typing dots — chat activity cue.",
        "ProgressBar",
        &[
            ("iOS", "typing dots"),
            ("Slack", "is typing"),
            ("React", "typing indicator"),
            ("Material", "three dots")
        ],
        false,
    ),
    props: &[
        PropSpec::Bool {
            key: "active",
            label: "Active",
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
        let mut __w = TypingIndicator::new().active(p.bool("active"));
        if !p.str("a11y_label").is_empty() {
            __w = __w.label(p.str("a11y_label"));
        }
        Box::new(__w)
    },
    snippet: |p| {
        let mut __s = format!("TypingIndicator::new().active({})", p.bool("active"));
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[("a11y_label", ".label", SnipProp::Text(""))],
        ));
        __s
    },
});

page!(ToastHostPage {
    meta: meta(
        "Toast",
        "Feedback",
        "Transient notification stack — auto-expiring toasts.",
        "Alert",
        &[
            ("Android", "Toast"),
            ("GNOME", "AdwToast"),
            ("React", "snackbar"),
            ("Qt", "custom")
        ],
        true,
    ),
    props: &[
        PropSpec::Text {
            key: "message",
            label: "Message",
            default: "Saved to disk",
        },
        PropSpec::Float {
            key: "ttl_secs",
            label: "Ttl Secs",
            min: 0.0,
            max: 60.0,
            step: 0.5,
            default: 0.0
        },
    ],
    build: |p| {
        let mut host = ToastHost::new();
        let mut t = Toast::new(Severity::Info, p.str("message"));
        if p.f64("ttl_secs") != 0.0 {
            t = t.ttl_secs(p.f64("ttl_secs") as f32);
        }
        host.push(t);
        host.push(Toast::new(Severity::Warning, "Retry queued"));
        Box::new(host)
    },
    snippet: |p| {
        let mut __s = format!(
            "let mut host = ToastHost::new();\nhost.push(Toast::new(Severity::Info, {:?})",
            p.str("message"),
        );
        if p.f64("ttl_secs") != 0.0 {
            __s.push_str(&format!(".ttl_secs({:?})", p.f64("ttl_secs") as f32));
        }
        __s.push_str(");");
        __s
    },
});

page!(AboutPage {
    meta: meta(
        "About",
        "Feedback",
        "About dialog — app name, version, credits, links.",
        "Dialog",
        &[
            ("GTK", "AdwAboutDialog"),
            ("Qt", "QMessageBox::about"),
            ("macOS", "About panel"),
            ("React", "about modal")
        ],
        true,
    ),
    props: &[
        PropSpec::Text {
            key: "name",
            label: "App name",
            default: "Martensite"
        },
        PropSpec::Text {
            key: "version",
            label: "Version",
            default: "0.16.0"
        },
        PropSpec::Text {
            key: "copyright",
            label: "Copyright",
            default: ""
        },
        PropSpec::Text {
            key: "credits_title",
            label: "Credits Title",
            default: ""
        },
        PropSpec::Text {
            key: "credits_names",
            label: "Credits Names",
            default: ""
        },
    ],
    build: |p| {
        let mut __w = About::new(p.str("name"))
            .version(p.str("version"))
            .comments("A retained-mode widget toolkit.")
            .website("martensite.dev", "https://example.com");
        if !p.str("copyright").is_empty() {
            __w = __w.copyright(p.str("copyright"));
        }
        if !p.str("credits_title").is_empty() || !p.str("credits_names").is_empty() {
            __w = __w.credits(
                p.str("credits_title"),
                crate::pages::csv(p, "credits_names"),
            );
        }
        Box::new(__w)
    },
    snippet: |p| {
        let mut __s = format!(
            "About::new({:?}).version({:?})",
            p.str("name"),
            p.str("version"),
        );
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[("copyright", ".copyright", SnipProp::Text(""))],
        ));
        if !p.str("credits_title").is_empty() || !p.str("credits_names").is_empty() {
            __s.push_str(&format!(
                "\n    .credits({:?}, {})",
                p.str("credits_title"),
                crate::pages::expr_strs(p.str("credits_names")).unwrap_or_default()
            ));
        }
        __s
    },
    poll: |w, out| {
        if let Some(a) = downcast_mut::<About>(w) {
            if let Some(url) = a.take_activated_url() {
                out.push(format!("open → {url}"));
            }
        }
    },
});

page!(CookieBannerPage {
    meta: meta(
        "CookieBanner",
        "Feedback",
        "Consent banner — accept/decline/customize.",
        "Banner",
        &[
            ("Web", "cookie banner"),
            ("GDPR", "consent"),
            ("React", "consent bar"),
            ("Qt", "custom")
        ],
        false,
    ),
    props: &[
        PropSpec::Text {
            key: "message",
            label: "Message",
            default: "We use cookies to improve your experience.",
        },
        PropSpec::Text {
            key: "policy_link",
            label: "Policy Link",
            default: ""
        },
        PropSpec::Text {
            key: "labels_accept",
            label: "Labels Accept",
            default: ""
        },
        PropSpec::Text {
            key: "labels_decline",
            label: "Labels Decline",
            default: ""
        },
        PropSpec::Text {
            key: "labels_customize",
            label: "Labels Customize",
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
        let mut __w = CookieBanner::new(p.str("message"));
        if !p.str("a11y_label").is_empty() {
            __w = __w.label(p.str("a11y_label"));
        }
        if !p.str("policy_link").is_empty() {
            __w = __w.policy_link(p.str("policy_link"));
        }
        if !p.str("labels_accept").is_empty()
            || !p.str("labels_decline").is_empty()
            || !p.str("labels_customize").is_empty()
        {
            __w = __w.labels(
                p.str("labels_accept"),
                p.str("labels_decline"),
                p.str("labels_customize"),
            );
        }
        Box::new(__w)
    },
    snippet: |p| {
        let mut __s = format!("CookieBanner::new({:?})", p.str("message"));
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("policy_link", ".policy_link", SnipProp::Text("")),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        if !p.str("labels_accept").is_empty()
            || !p.str("labels_decline").is_empty()
            || !p.str("labels_customize").is_empty()
        {
            __s.push_str(&format!(
                "\n    .labels({:?}, {:?}, {:?})",
                p.str("labels_accept"),
                p.str("labels_decline"),
                p.str("labels_customize")
            ));
        }
        __s
    },
    poll: |w, out| {
        if let Some(cb) = downcast_mut::<CookieBanner>(w) {
            if let Some(consent) = cb.take_consent() {
                out.push(format!("consent → {consent:?}"));
            }
        }
    },
});

page!(CountdownPage {
    meta: meta(
        "Countdown",
        "Feedback",
        "Countdown timer — remaining time readout.",
        "Text",
        &[
            ("iOS", "timer"),
            ("Qt", "custom"),
            ("React", "countdown"),
            ("TV", "countdown")
        ],
        false,
    ),
    props: &[
        PropSpec::Int {
            key: "secs",
            label: "Seconds",
            min: 5,
            max: 600,
            default: 90,
        },
        PropSpec::Bool {
            key: "paused",
            label: "Paused",
            default: false
        },
        PropSpec::Text {
            key: "warn_under",
            label: "Warn Under",
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
        let mut c = Countdown::new(Duration::from_secs(p.i64("secs") as u64));
        c.set_running(true);
        {
            let mut __w = c;
            __w = __w.enabled(p.bool("enabled"));
            if !p.str("a11y_label").is_empty() {
                __w = __w.label(p.str("a11y_label"));
            }
            if p.bool("paused") {
                __w = __w.paused(p.bool("paused"));
            }
            if let Some(v) = crate::pages::parse_secs(p.str("warn_under")) {
                __w = __w.warn_under(v);
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = format!("Countdown::new(Duration::from_secs({}))", p.i64("secs"));
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("paused", ".paused", SnipProp::Bool(false)),
                ("enabled", ".enabled", SnipProp::Bool(true)),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        __s.push_str(&crate::pages::snip_textmap(
            p,
            "warn_under",
            ".warn_under",
            "",
            crate::pages::expr_secs,
        ));
        __s
    },
    poll: |w, out| {
        if downcast_mut::<Countdown>(w).is_some_and(|c| c.take_elapsed()) {
            out.push("elapsed".to_string());
        }
    },
    state: |w| {
        downcast_mut::<Countdown>(w)
            .map(|c| vec![("remaining".to_string(), format!("{:?}", c.remaining()))])
            .unwrap_or_default()
    },
});

page!(CountdownRingPage {
    meta: meta(
        "CountdownRing",
        "Feedback",
        "Circular countdown — ring drains as time runs out.",
        "ProgressBar",
        &[
            ("iOS", "timer ring"),
            ("Web", "ring timer"),
            ("React", "countdown ring"),
            ("Qt", "custom")
        ],
        false,
    ),
    props: &[
        PropSpec::Int {
            key: "secs",
            label: "Seconds",
            min: 5,
            max: 600,
            default: 60,
        },
        PropSpec::Bool {
            key: "paused",
            label: "Paused",
            default: false
        },
        PropSpec::Text {
            key: "warn_under",
            label: "Warn Under",
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
        let mut c = CountdownRing::new(Duration::from_secs(p.i64("secs") as u64));
        c.set_running(true);
        {
            let mut __w = c;
            __w = __w.enabled(p.bool("enabled"));
            if !p.str("a11y_label").is_empty() {
                __w = __w.label(p.str("a11y_label"));
            }
            if p.bool("paused") {
                __w = __w.paused(p.bool("paused"));
            }
            if let Some(v) = crate::pages::parse_secs(p.str("warn_under")) {
                __w = __w.warn_under(v);
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = format!("CountdownRing::new(Duration::from_secs({}))", p.i64("secs"));
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("paused", ".paused", SnipProp::Bool(false)),
                ("enabled", ".enabled", SnipProp::Bool(true)),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        __s.push_str(&crate::pages::snip_textmap(
            p,
            "warn_under",
            ".warn_under",
            "",
            crate::pages::expr_secs,
        ));
        __s
    },
    poll: |w, out| {
        if downcast_mut::<CountdownRing>(w).is_some_and(|c| c.take_finished()) {
            out.push("finished".to_string());
        }
    },
});

page!(LevelBarPage {
    meta: meta(
        "LevelBar",
        "Feedback",
        "Discrete level meter — battery/signal segments.",
        "LevelBar",
        &[
            ("GTK", "GtkLevelBar"),
            ("Qt", "custom"),
            ("iOS", "level bar"),
            ("React", "meter")
        ],
        false,
    ),
    props: &[
        PropSpec::Float {
            key: "value",
            label: "Value",
            min: 0.0,
            max: 1.0,
            step: 0.1,
            default: 0.7
        },
        PropSpec::Int {
            key: "segments",
            label: "Segments",
            min: 2,
            max: 10,
            default: 5
        },
    ],
    build: |p| {
        let mut lb = LevelBar::new()
            .segments(p.i64("segments") as usize)
            .zones(0.3, 0.6, 0.9);
        lb.set_value(p.f64("value") as f32);
        Box::new(lb)
    },
    snippet: |p| format!(
        "LevelBar::new().segments({}).zones(0.3, 0.6, 0.9)",
        p.i64("segments"),
    ),
});

page!(MarqueePage {
    meta: meta(
        "Marquee",
        "Feedback",
        "Horizontally scrolling text ticker.",
        "Text",
        &[
            ("HTML", "<marquee>"),
            ("Qt", "QLabel scroll"),
            ("TV", "ticker"),
            ("React", "marquee")
        ],
        false,
    ),
    props: &[
        PropSpec::Text {
            key: "text",
            label: "Text",
            default: "Breaking: Martensite ships the widget catalog"
        },
        PropSpec::Float {
            key: "speed",
            label: "Speed",
            min: 10.0,
            max: 200.0,
            step: 10.0,
            default: 60.0
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
    ],
    build: |p| {
        let mut __w = Marquee::new(p.str("text")).speed(p.f64("speed") as f32);
        __w = __w.enabled(p.bool("enabled"));
        if p.f64("gap") != 0.0 {
            __w = __w.gap(p.f64("gap") as f32);
        }
        __w.set_scroll_offset(220.0); // stage mid-scroll for a static snapshot
        Box::new(__w)
    },
    snippet: |p| {
        let mut __s = format!(
            "Marquee::new({:?}).speed({:?})",
            p.str("text"),
            p.f64("speed") as f32,
        );
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("gap", ".gap", SnipProp::Float(0.0)),
                ("enabled", ".enabled", SnipProp::Bool(true)),
            ],
        ));
        __s
    },
});

page!(NotificationCenterPage {
    meta: meta(
        "NotificationCenter",
        "Feedback",
        "Notification list — grouped cards with dismiss.",
        "List",
        &[
            ("macOS", "Notification Center"),
            ("Android", "shade"),
            ("iOS", "notification list"),
            ("GNOME", "notifications")
        ],
        true,
    ),
    props: &[
        PropSpec::Header {
            label: "State & Accessibility"
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
    build: |_p| {
        let mut nc = NotificationCenter::new();
        nc.push(Notification::new("Backup done", "2.4 GB written to vault"));
        nc.push(Notification::new("Update ready", "Restart to apply v0.16"));
        {
            let mut __w = nc;
            __w = __w.loading(_p.bool("loading"));
            if !_p.str("a11y_label").is_empty() {
                __w = __w.label(_p.str("a11y_label"));
            }
            Box::new(__w)
        }
    },
    snippet: |_p| {
        let mut __s = {
            "let mut nc = NotificationCenter::new();\nnc.push(Notification::new(\"Backup done\", \"…\"));"
            .to_string()
        };
        __s.push_str(&crate::pages::prop_snippet(
            _p,
            &[
                ("loading", ".loading", SnipProp::Bool(false)),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(nc) = downcast_mut::<NotificationCenter>(w) {
            if let Some(i) = nc.take_dismissed() {
                out.push(format!("dismissed {i}"));
            }
            if nc.take_cleared() {
                out.push("cleared all".to_string());
            }
        }
    },
});

page!(AlarmPanelPage {
    meta: meta(
        "AlarmPanel",
        "Feedback",
        "Industrial alarm list with acknowledge flow.",
        "List",
        &[
            ("OT", "alarm panel"),
            ("SCADA", "alarms"),
            ("Qt", "custom"),
            ("React", "alert list")
        ],
        false,
    ),
    props: &[
        PropSpec::Header {
            label: "State & Accessibility"
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
    build: |_p| {
        let mut ap = AlarmPanel::new();
        ap.push(Alarm::new(Severity::Warning, "Temp sensor above range"));
        ap.push(Alarm::new(Severity::Error, "Comms lost on bus 2"));
        {
            let mut __w = ap;
            __w = __w.loading(_p.bool("loading"));
            if !_p.str("a11y_label").is_empty() {
                __w = __w.label(_p.str("a11y_label"));
            }
            Box::new(__w)
        }
    },
    snippet: |_p| {
        let mut __s = "AlarmPanel::new().push(Alarm::new(Severity::Warning, \"…\"))".to_string();
        __s.push_str(&crate::pages::prop_snippet(
            _p,
            &[
                ("loading", ".loading", SnipProp::Bool(false)),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(ap) = downcast_mut::<AlarmPanel>(w) {
            if let Some(i) = ap.take_acked() {
                out.push(format!("acked {i}"));
            }
        }
    },
});

page!(SplashPage {
    meta: meta(
        "Splash",
        "Feedback",
        "App splash screen — name, progress, status.",
        "Window",
        &[
            ("Qt", "QSplashScreen"),
            ("Android", "splash"),
            ("iOS", "launch screen"),
            ("React", "loader")
        ],
        false,
    ),
    props: &[
        PropSpec::Text {
            key: "name",
            label: "App name",
            default: "Martensite"
        },
        PropSpec::Float {
            key: "progress",
            label: "Progress",
            min: 0.0,
            max: 1.0,
            step: 0.1,
            default: 0.4
        },
        PropSpec::Text {
            key: "version",
            label: "Version",
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
        let mut s = Splash::new(p.str("name"));
        s.set_progress(p.f64("progress") as f32);
        s.set_status("Loading assets…");
        {
            let mut __w = s;
            if !p.str("a11y_label").is_empty() {
                __w = __w.label(p.str("a11y_label"));
            }
            if !p.str("version").is_empty() {
                __w = __w.version(p.str("version"));
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = format!("Splash::new({:?})", p.str("name"));
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("version", ".version", SnipProp::Text("")),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        __s
    },
});

page!(ReleaseNotesPage {
    meta: meta(
        "ReleaseNotes",
        "Feedback",
        "Versioned changelog view.",
        "List",
        &[
            ("GitHub", "release notes"),
            ("App Store", "What's New"),
            ("Qt", "custom"),
            ("React", "changelog")
        ],
        false,
    ),
    props: &[
        PropSpec::Header {
            label: "State & Accessibility"
        },
        PropSpec::Text {
            key: "a11y_label",
            label: "A11y label",
            default: ""
        },
    ],
    build: |_p| {
        {
            let mut __w = ReleaseNotes::new().release(
                Release::new("0.16.0")
                    .date("2025-06")
                    .change(ChangeKind::Added, "Added widget catalog")
                    .change(ChangeKind::Changed, "Native icon pack"),
            );
            if !_p.str("a11y_label").is_empty() {
                __w = __w.label(_p.str("a11y_label"));
            }
            Box::new(__w)
        }
    },
    snippet: |_p| {
        let mut __s = "ReleaseNotes::new().release(Release::new(\"0.16.0\"))".to_string();
        __s.push_str(&crate::pages::prop_snippet(
            _p,
            &[("a11y_label", ".label", SnipProp::Text(""))],
        ));
        __s
    },
});

page!(UpdatePromptPage {
    meta: meta(
        "UpdatePrompt",
        "Feedback",
        "Update-available card with notes and action.",
        "Alert",
        &[
            ("macOS", "update dialog"),
            ("Sparkle", "update prompt"),
            ("React", "update banner"),
            ("Qt", "custom")
        ],
        false,
    ),
    props: &[
        PropSpec::Text {
            key: "version",
            label: "Version",
            default: "0.16.0"
        },
        PropSpec::Text {
            key: "title",
            label: "Title",
            default: "Update available"
        },
        PropSpec::Text {
            key: "action_label",
            label: "Action Label",
            default: "Update available"
        },
        PropSpec::Float {
            key: "progress",
            label: "Progress",
            min: 0.0,
            max: 1.0,
            step: 0.05,
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
        let mut __w =
            UpdatePrompt::new(p.str("version")).notes("Bug fixes and performance improvements.");
        if !p.str("a11y_label").is_empty() {
            __w = __w.label(p.str("a11y_label"));
        }
        if p.str("title") != "Update available" {
            __w = __w.title(p.str("title"));
        }
        if p.str("action_label") != "Update available" {
            __w = __w.action_label(p.str("action_label"));
        }
        if p.f64("progress") != 0.0 {
            __w = __w.progress(p.f64("progress") as f32);
        }
        Box::new(__w)
    },
    snippet: |p| {
        let mut __s = format!("UpdatePrompt::new({:?})", p.str("version"));
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("title", ".title", SnipProp::Text("Update available")),
                (
                    "action_label",
                    ".action_label",
                    SnipProp::Text("Update available"),
                ),
                ("progress", ".progress", SnipProp::Float(0.0)),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(up) = downcast_mut::<UpdatePrompt>(w) {
            if up.take_update() {
                out.push("update".to_string());
            }
            if up.take_later() {
                out.push("later".to_string());
            }
        }
    },
});

page!(ActivityRingPage {
    meta: meta(
        "ActivityRing",
        "Feedback",
        "Apple-style concentric progress rings.",
        "ProgressBar",
        &[
            ("watchOS", "activity rings"),
            ("React", "ring chart"),
            ("Qt", "custom"),
            ("Health", "rings")
        ],
        false,
    ),
    props: &[
        PropSpec::Header {
            label: "State & Accessibility"
        },
        PropSpec::Text {
            key: "a11y_label",
            label: "A11y label",
            default: ""
        },
    ],
    build: |_p| {
        let mut __w = ActivityRing::new()
            .ring("Move", 0.75, [240, 90, 80, 255])
            .ring("Exercise", 0.5, [160, 230, 90, 255])
            .ring("Stand", 0.9, [80, 200, 250, 255]);
        if !_p.str("a11y_label").is_empty() {
            __w = __w.label(_p.str("a11y_label"));
        }
        Box::new(__w)
    },
    snippet: |_p| {
        let mut __s = {
            "ActivityRing::new()\n    .ring(\"Move\", 0.75, [240, 90, 80, 255])\n    .ring(\"Exercise\", 0.5, [160, 230, 90, 255])"
            .to_string()
        };
        __s.push_str(&crate::pages::prop_snippet(
            _p,
            &[("a11y_label", ".label", SnipProp::Text(""))],
        ));
        __s
    },
});

page!(TickerTapePage {
    meta: meta(
        "TickerTape",
        "Feedback",
        "Scrolling stock/symbol ticker.",
        "Text",
        &[
            ("TV", "ticker tape"),
            ("NYSE", "ticker"),
            ("React", "ticker"),
            ("Qt", "custom")
        ],
        false,
    ),
    props: &[
        PropSpec::Float {
            key: "speed",
            label: "Speed",
            min: 10.0,
            max: 200.0,
            step: 10.0,
            default: 50.0,
        },
        PropSpec::Text {
            key: "ticker_sym",
            label: "Ticker Symbol",
            default: ""
        },
        PropSpec::Text {
            key: "ticker_price",
            label: "Ticker Price",
            default: ""
        },
        PropSpec::Float {
            key: "ticker_delta",
            label: "Ticker Delta",
            min: -50.0,
            max: 50.0,
            step: 0.5,
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
        let mut tt = TickerTape::new().speed(p.f64("speed") as f32);
        tt.set_items(vec![
            TickerItem::new("MSFT", "415.20", 0.8),
            TickerItem::new("AAPL", "214.10", -0.4),
            TickerItem::new("NVDA", "131.40", 2.1),
        ]);
        {
            let mut __w = tt;
            if !p.str("a11y_label").is_empty() {
                __w = __w.label(p.str("a11y_label"));
            }
            if !p.str("ticker_sym").is_empty() {
                __w = __w.item(martensite::widgets::ticker_tape::TickerItem::new(
                    p.str("ticker_sym"),
                    p.str("ticker_price"),
                    p.f64("ticker_delta") as f32,
                ));
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = format!("TickerTape::new().speed({:?})", p.f64("speed") as f32);
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[("a11y_label", ".label", SnipProp::Text(""))],
        ));
        if !p.str("ticker_sym").is_empty() {
            __s.push_str(&format!(
                "\n    .item(TickerItem::new({:?}, {:?}, {}))",
                p.str("ticker_sym"),
                p.str("ticker_price"),
                p.f64("ticker_delta")
            ));
        }
        __s
    },
    poll: |w, out| {
        if let Some(tt) = downcast_mut::<TickerTape>(w) {
            if let Some(i) = tt.take_selected() {
                out.push(format!("item {i}"));
            }
        }
    },
});

page!(StatisticPage {
    meta: meta(
        "Statistic",
        "Feedback",
        "Prominent metric — title, value, trend, prefix/suffix.",
        "Text",
        &[
            ("Ant", "Statistic"),
            ("Dashboard", "KPI"),
            ("React", "stat card"),
            ("Qt", "custom")
        ],
        false,
    ),
    props: &[
        PropSpec::Text {
            key: "title",
            label: "Title",
            default: "Uptime"
        },
        PropSpec::Text {
            key: "value",
            label: "Value",
            default: "99.98%"
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
            key: "trend_dir",
            label: "Trend",
            options: &["Neutral", "Up", "Down"],
            default: 0
        },
        PropSpec::Text {
            key: "trend_text",
            label: "Trend Text",
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
        use martensite::widgets::statistic::{Statistic, Trend};
        let mut s = Statistic::new(p.str("title"), p.str("value"));
        s.set_value(p.str("value"));
        if !p.str("prefix").is_empty() {
            s = s.prefix(p.str("prefix"));
        }
        if !p.str("suffix").is_empty() {
            s = s.suffix(p.str("suffix"));
        }
        let dir = match p.choice("trend_dir") {
            1 => Trend::Up,
            2 => Trend::Down,
            _ => Trend::Neutral,
        };
        if dir != Trend::Neutral || !p.str("trend_text").is_empty() {
            s = s.trend(dir, p.str("trend_text"));
        }
        if !p.bool("enabled") {
            s = s.enabled(false);
        }
        Box::new(s)
    },
    snippet: |p| {
        let mut __s = format!("Statistic::new({:?}, {:?})", p.str("title"), p.str("value"),);
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("prefix", ".prefix", SnipProp::Text("")),
                ("suffix", ".suffix", SnipProp::Text("")),
                ("enabled", ".enabled", SnipProp::Bool(true)),
            ],
        ));
        let dir = match p.choice("trend_dir") {
            1 => "Trend::Up",
            2 => "Trend::Down",
            _ => "Trend::Neutral",
        };
        if dir != "Trend::Neutral" || !p.str("trend_text").is_empty() {
            __s.push_str(&format!("\n    .trend({}, {:?})", dir, p.str("trend_text")));
        }
        __s
    },
});

page!(WatermarkPage {
    meta: meta(
        "Watermark",
        "Feedback",
        "Tiled watermark text behind content.",
        "Text",
        &[
            ("Word", "watermark"),
            ("PDF", "draft stamp"),
            ("React", "watermark"),
            ("Qt", "custom")
        ],
        false,
    ),
    props: &[
        PropSpec::Text {
            key: "text",
            label: "Text",
            default: "DRAFT"
        },
        PropSpec::Float {
            key: "opacity",
            label: "Opacity",
            min: 0.0,
            max: 1.0,
            step: 0.05,
            default: 0.15
        },
        PropSpec::Float {
            key: "font_size",
            label: "Font Size",
            min: 0.0,
            max: 64.0,
            step: 0.5,
            default: 0.0
        },
        PropSpec::Text {
            key: "gap",
            label: "Gap (csv)",
            default: ""
        },
        PropSpec::Text {
            key: "offset",
            label: "Offset (csv)",
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
        let mut __w = Watermark::new(p.str("text")).opacity(p.f64("opacity") as f32);
        __w = __w.enabled(p.bool("enabled"));
        if p.f64("font_size") != 0.0 {
            __w = __w.font_size(p.f64("font_size") as f32);
        }
        if let Some(v) = crate::pages::parse_pair(p.str("gap")) {
            __w = __w.gap(v.0 as f32, v.1 as f32);
        }
        if let Some(v) = crate::pages::parse_pair(p.str("offset")) {
            __w = __w.offset(v.0 as f32, v.1 as f32);
        }
        Box::new(__w)
    },
    snippet: |p| {
        let mut __s = format!(
            "Watermark::new({:?}).opacity({:?})",
            p.str("text"),
            p.f64("opacity") as f32,
        );
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("font_size", ".font_size", SnipProp::Float(0.0)),
                ("enabled", ".enabled", SnipProp::Bool(true)),
            ],
        ));
        __s.push_str(&crate::pages::snip_textmap(
            p,
            "gap",
            ".gap",
            "",
            crate::pages::expr_pair,
        ));
        __s.push_str(&crate::pages::snip_textmap(
            p,
            "offset",
            ".offset",
            "",
            crate::pages::expr_pair,
        ));
        __s
    },
});

/// All Feedback pages, in rail order.
pub(super) fn pages() -> Vec<Box<dyn Page>> {
    vec![
        Box::new(ProgressBarPage),
        Box::new(SpinnerPage),
        Box::new(BadgePage),
        Box::new(BannerPage),
        Box::new(SkeletonPage),
        Box::new(EmptyStatePage),
        Box::new(ResultPagePage),
        Box::new(StatusDotPage),
        Box::new(StatusBarPage),
        Box::new(TypingIndicatorPage),
        Box::new(ToastHostPage),
        Box::new(AboutPage),
        Box::new(CookieBannerPage),
        Box::new(CountdownPage),
        Box::new(CountdownRingPage),
        Box::new(LevelBarPage),
        Box::new(MarqueePage),
        Box::new(NotificationCenterPage),
        Box::new(AlarmPanelPage),
        Box::new(SplashPage),
        Box::new(ReleaseNotesPage),
        Box::new(UpdatePromptPage),
        Box::new(ActivityRingPage),
        Box::new(TickerTapePage),
        Box::new(StatisticPage),
        Box::new(WatermarkPage),
        Box::new(StackLightPage),
    ]
}

page!(StackLightPage {
    meta: meta(
        "StackLight",
        "Feedback",
        "Andon tower — stacked status lamps with lit/flashing states.",
        "Indicator",
        &[
            ("Factory", "andon"),
            ("PLC", "stack light"),
            ("React", "status tower"),
            ("Qt", "custom")
        ],
        false,
    ),
    props: &[
        PropSpec::Bool {
            key: "flashing",
            label: "Warn flashing",
            default: true
        },
        PropSpec::Header {
            label: "State & Accessibility"
        },
        PropSpec::Bool {
            key: "loading",
            label: "Loading",
            default: false
        },
    ],
    build: |p| {
        let mut __w = StackLight::new()
            .label("Cell 4")
            .lamp(Lamp::new("Run", [90, 200, 120, 255]).lit(true))
            .lamp(
                Lamp::new("Warn", [240, 180, 60, 255])
                    .lit(true)
                    .flashing(p.bool("flashing")),
            )
            .lamp(Lamp::new("Fault", [240, 90, 80, 255]));
        __w = __w.loading(p.bool("loading"));
        Box::new(__w)
    },
    snippet: |p| {
        let mut __s = {
            format!(
        "StackLight::new()\n    .lamp(Lamp::new(\"Run\", [90, 200, 120, 255]).lit(true))\n    .lamp(Lamp::new(\"Warn\", [240, 180, 60, 255]).flashing({}))",
        p.bool("flashing"),
    )
        };
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[("loading", ".loading", SnipProp::Bool(false))],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(s) = downcast_mut::<StackLight>(w) {
            if let Some(i) = s.take_changed() {
                out.push(format!("lamp {i} toggled"));
            }
        }
    },
});
