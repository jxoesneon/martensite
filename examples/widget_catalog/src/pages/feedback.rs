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
use crate::pages::{downcast_mut, meta, page};

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
    ],
    build: |p| {
        let mut s = Spinner::new().size(p.f64("size") as f32);
        if p.bool("active") {
            s.start();
        }
        Box::new(s)
    },
    snippet: |p| format!("Spinner::new().size({:?})", p.f64("size") as f32),
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
        Box::new(b)
    },
    snippet: |p| format!(
        "Badge::new(\"\").severity(BadgeSeverity::{}).with_count({})",
        ["Accent", "Info", "Ok", "Warning", "Error"][p.choice("severity")],
        p.i64("count"),
    ),
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
    props: &[PropSpec::Choice {
        key: "shape",
        label: "Shape",
        options: &["Block", "Circle", "Lines", "Rows", "Grid"],
        default: 2,
    }],
    build: |p| {
        let s = match p.choice("shape") {
            0 => Skeleton::block(),
            1 => Skeleton::circle(),
            3 => Skeleton::rows(3),
            4 => Skeleton::grid(3, 2),
            _ => Skeleton::lines(3),
        };
        Box::new(s)
    },
    snippet: |p| format!(
        "Skeleton::{}()",
        ["block", "circle", "lines(3)", "rows(3)", "grid(3, 2)"][p.choice("shape")],
    ),
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
    ],
    build: |p| Box::new(
        EmptyState::new(p.str("title"))
            .description(p.str("desc"))
            .action("New message"),
    ),
    snippet: |p| format!(
        "EmptyState::new({:?})\n    .description({:?})\n    .action(\"New message\")",
        p.str("title"),
        p.str("desc"),
    ),
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
    props: &[PropSpec::Choice {
        key: "status",
        label: "Status",
        options: &["Success", "Warning", "Error", "Info"],
        default: 0,
    }],
    build: |p| {
        let st = match p.choice("status") {
            1 => ResultStatus::Warning,
            2 => ResultStatus::Error,
            3 => ResultStatus::Info,
            _ => ResultStatus::Success,
        };
        Box::new(
            ResultPage::new(st)
                .title("Operation complete")
                .subtitle("The file was processed without issues."),
        )
    },
    snippet: |p| format!(
        "ResultPage::new(ResultStatus::{})",
        ["Success", "Warning", "Error", "Info"][p.choice("status")],
    ),
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
    ],
    build: |p| {
        let st = match p.choice("status") {
            0 => Status::Off,
            1 => Status::Info,
            3 => Status::Warning,
            4 => Status::Error,
            _ => Status::Ok,
        };
        Box::new(StatusDot::new(p.str("text")).status(st))
    },
    snippet: |p| format!(
        "StatusDot::new({:?}).status(Status::{})",
        p.str("text"),
        ["Off", "Info", "Ok", "Warning", "Error"][p.choice("status")],
    ),
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
    props: &[],
    build: |_p| {
        let mut sb = StatusBar::new().label("Status");
        sb.set_message("Ready");
        Box::new(sb)
    },
    snippet: |_p| "StatusBar::new().label(\"Status\")".to_string(),
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
    props: &[PropSpec::Bool {
        key: "active",
        label: "Active",
        default: true
    }],
    build: |p| Box::new(TypingIndicator::new().active(p.bool("active"))),
    snippet: |p| format!("TypingIndicator::new().active({})", p.bool("active")),
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
    props: &[PropSpec::Text {
        key: "message",
        label: "Message",
        default: "Saved to disk",
    }],
    build: |p| {
        let mut host = ToastHost::new();
        host.push(Toast::new(Severity::Info, p.str("message")));
        host.push(Toast::new(Severity::Warning, "Retry queued"));
        Box::new(host)
    },
    snippet: |p| format!(
        "let mut host = ToastHost::new();\nhost.push(Toast::new(Severity::Info, {:?}));",
        p.str("message"),
    ),
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
    ],
    build: |p| Box::new(
        About::new(p.str("name"))
            .version(p.str("version"))
            .comments("A retained-mode widget toolkit.")
            .website("martensite.dev", "https://example.com"),
    ),
    snippet: |p| format!(
        "About::new({:?}).version({:?})",
        p.str("name"),
        p.str("version"),
    ),
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
    props: &[PropSpec::Text {
        key: "message",
        label: "Message",
        default: "We use cookies to improve your experience.",
    }],
    build: |p| Box::new(CookieBanner::new(p.str("message"))),
    snippet: |p| format!("CookieBanner::new({:?})", p.str("message")),
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
    props: &[PropSpec::Int {
        key: "secs",
        label: "Seconds",
        min: 5,
        max: 600,
        default: 90,
    }],
    build: |p| {
        let mut c = Countdown::new(Duration::from_secs(p.i64("secs") as u64));
        c.set_running(true);
        Box::new(c)
    },
    snippet: |p| format!("Countdown::new(Duration::from_secs({}))", p.i64("secs")),
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
    props: &[PropSpec::Int {
        key: "secs",
        label: "Seconds",
        min: 5,
        max: 600,
        default: 60,
    }],
    build: |p| {
        let mut c = CountdownRing::new(Duration::from_secs(p.i64("secs") as u64));
        c.set_running(true);
        Box::new(c)
    },
    snippet: |p| format!("CountdownRing::new(Duration::from_secs({}))", p.i64("secs")),
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
    ],
    build: |p| Box::new(Marquee::new(p.str("text")).speed(p.f64("speed") as f32),),
    snippet: |p| format!(
        "Marquee::new({:?}).speed({:?})",
        p.str("text"),
        p.f64("speed") as f32,
    ),
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
    props: &[],
    build: |_p| {
        let mut nc = NotificationCenter::new();
        nc.push(Notification::new("Backup done", "2.4 GB written to vault"));
        nc.push(Notification::new("Update ready", "Restart to apply v0.16"));
        Box::new(nc)
    },
    snippet: |_p| {
        "let mut nc = NotificationCenter::new();\nnc.push(Notification::new(\"Backup done\", \"…\"));"
            .to_string()
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
    props: &[],
    build: |_p| {
        let mut ap = AlarmPanel::new();
        ap.push(Alarm::new(Severity::Warning, "Temp sensor above range"));
        ap.push(Alarm::new(Severity::Error, "Comms lost on bus 2"));
        Box::new(ap)
    },
    snippet: |_p| "AlarmPanel::new().push(Alarm::new(Severity::Warning, \"…\"))".to_string(),
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
    ],
    build: |p| {
        let mut s = Splash::new(p.str("name"));
        s.set_progress(p.f64("progress") as f32);
        s.set_status("Loading assets…");
        Box::new(s)
    },
    snippet: |p| format!("Splash::new({:?})", p.str("name")),
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
    props: &[],
    build: |_p| {
        Box::new(
            ReleaseNotes::new().release(
                Release::new("0.16.0")
                    .date("2025-06")
                    .change(ChangeKind::Added, "Added widget catalog")
                    .change(ChangeKind::Changed, "Native icon pack"),
            ),
        )
    },
    snippet: |_p| "ReleaseNotes::new().release(Release::new(\"0.16.0\"))".to_string(),
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
    props: &[PropSpec::Text {
        key: "version",
        label: "Version",
        default: "0.16.0"
    }],
    build: |p| Box::new(
        UpdatePrompt::new(p.str("version")).notes("Bug fixes and performance improvements."),
    ),
    snippet: |p| format!("UpdatePrompt::new({:?})", p.str("version")),
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
    props: &[],
    build: |_p| Box::new(
        ActivityRing::new()
            .ring("Move", 0.75, [240, 90, 80, 255])
            .ring("Exercise", 0.5, [160, 230, 90, 255])
            .ring("Stand", 0.9, [80, 200, 250, 255]),
    ),
    snippet: |_p| {
        "ActivityRing::new()\n    .ring(\"Move\", 0.75, [240, 90, 80, 255])\n    .ring(\"Exercise\", 0.5, [160, 230, 90, 255])"
            .to_string()
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
    props: &[PropSpec::Float {
        key: "speed",
        label: "Speed",
        min: 10.0,
        max: 200.0,
        step: 10.0,
        default: 50.0,
    }],
    build: |p| {
        let mut tt = TickerTape::new().speed(p.f64("speed") as f32);
        tt.set_items(vec![
            TickerItem::new("MSFT", "415.20", 0.8),
            TickerItem::new("AAPL", "214.10", -0.4),
            TickerItem::new("NVDA", "131.40", 2.1),
        ]);
        Box::new(tt)
    },
    snippet: |p| format!("TickerTape::new().speed({:?})", p.f64("speed") as f32),
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
    ],
    build: |p| {
        let mut s = martensite::widgets::statistic::Statistic::new(p.str("title"), p.str("value"));
        s.set_value(p.str("value"));
        Box::new(s)
    },
    snippet: |p| format!("Statistic::new({:?}, {:?})", p.str("title"), p.str("value"),),
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
    ],
    build: |p| Box::new(Watermark::new(p.str("text")).opacity(p.f64("opacity") as f32),),
    snippet: |p| format!(
        "Watermark::new({:?}).opacity({:?})",
        p.str("text"),
        p.f64("opacity") as f32,
    ),
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
    props: &[PropSpec::Bool {
        key: "flashing",
        label: "Warn flashing",
        default: true
    }],
    build: |p| Box::new(
        StackLight::new()
            .label("Cell 4")
            .lamp(Lamp::new("Run", [90, 200, 120, 255]).lit(true))
            .lamp(
                Lamp::new("Warn", [240, 180, 60, 255])
                    .lit(true)
                    .flashing(p.bool("flashing"))
            )
            .lamp(Lamp::new("Fault", [240, 90, 80, 255])),
    ),
    snippet: |p| {
        format!(
        "StackLight::new()\n    .lamp(Lamp::new(\"Run\", [90, 200, 120, 255]).lit(true))\n    .lamp(Lamp::new(\"Warn\", [240, 180, 60, 255]).flashing({}))",
        p.bool("flashing"),
    )
    },
    poll: |w, out| {
        if let Some(s) = downcast_mut::<StackLight>(w) {
            if let Some(i) = s.take_changed() {
                out.push(format!("lamp {i} toggled"));
            }
        }
    },
});
