//! Overlays family — popups, dialogs, sheets, and hover surfaces.
//! All pages mark `needs_overlay: true` and open their surface on
//! stage build where the widget requires it.

use glam::Vec2;
use martensite::core::overlay::AnchorEdge;
use martensite::core::Rect;
use martensite::widgets::action_sheet::ActionSheet;
use martensite::widgets::alert_dialog::{AlertDialog, AlertRole};
use martensite::widgets::bottom_sheet::BottomSheet;
use martensite::widgets::button::Button;
use martensite::widgets::context_menu::ContextMenu;
use martensite::widgets::dialog::Dialog;
use martensite::widgets::drawer::Drawer;
use martensite::widgets::hover_card::HoverCard;
use martensite::widgets::menu::{Menu, MenuItem};
use martensite::widgets::pip::Pip;
use martensite::widgets::popconfirm::Popconfirm;
use martensite::widgets::popover::Popover;
use martensite::widgets::swipe_actions::{SwipeAction, SwipeActions};
use martensite::widgets::text::Text;
use martensite::widgets::tooltip::Tooltip;
use martensite::widgets::tour::Tour;

use crate::page::{Page, PropSpec};
use crate::pages::{downcast_mut, meta, page, SnipProp};

/// Stage-area anchor rect — a trigger-sized rect at upper-center so
/// bubbles open inside the stage instead of hugging the left edge.
fn anchor() -> Rect {
    Rect::new(336.0, 140.0, 48.0, 24.0)
}

page!(TooltipPage {
    meta: meta(
        "Tooltip",
        "Overlays",
        "Hover hint bubble anchored to a widget.",
        "Tooltip",
        &[
            ("Qt", "QToolTip"),
            ("GTK", "tooltip"),
            ("HTML", "title attr"),
            ("SwiftUI", "help")
        ],
        true,
    ),
    props: &[
        PropSpec::Text {
            key: "text",
            label: "Text",
            default: "Saves the document"
        },
        PropSpec::Int {
            key: "delay",
            label: "Delay (ms)",
            min: 0,
            max: 2000,
            default: 400
        },
    ],
    build: |p| {
        let mut t =
            Tooltip::new(Button::new("Hover me"), p.str("text")).delay_ms(p.i64("delay") as u64);
        t.show();
        Box::new(t)
    },
    snippet: |p| format!(
        "Tooltip::new(Button::new(\"Hover me\"), {:?})\n    .delay_ms({})",
        p.str("text"),
        p.i64("delay"),
    ),
});

page!(PopoverPage {
    meta: meta(
        "Popover",
        "Overlays",
        "Anchored floating panel with arbitrary content.",
        "Dialog",
        &[
            ("macOS", "NSPopover"),
            ("GTK", "GtkPopover"),
            ("Qt", "popup"),
            ("React", "popover")
        ],
        true,
    ),
    props: &[
        PropSpec::Text {
            key: "title",
            label: "Title",
            default: "Filter"
        },
        PropSpec::Bool {
            key: "autohide",
            label: "Autohide",
            default: true
        },
    ],
    build: |p| {
        let mut pop = Popover::new()
            .title(p.str("title"))
            .preferred_edge(AnchorEdge::Bottom)
            .child(Text::new("Popover body content"))
            .anchor(anchor());
        pop.open();
        {
            let mut __w = pop;
            if !p.bool("autohide") {
                __w = __w.autohide(p.bool("autohide"));
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = {
            format!(
                "Popover::new()\n    .title({:?})\n    .preferred_edge(AnchorEdge::Bottom)\n    .anchor(rect)",
                p.str("title"),
            )
        };
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[("autohide", ".autohide", SnipProp::Bool(true))],
        ));
        __s
    },
});

page!(PopconfirmPage {
    meta: meta(
        "Popconfirm",
        "Overlays",
        "Inline confirm bubble — question + confirm/cancel.",
        "Alert",
        &[
            ("Ant", "Popconfirm"),
            ("Qt", "confirm bubble"),
            ("React", "popconfirm"),
            ("GTK", "custom")
        ],
        true,
    ),
    props: &[
        PropSpec::Text {
            key: "question",
            label: "Question",
            default: "Delete this item?",
        },
        PropSpec::Text {
            key: "cancel_label",
            label: "Cancel Label",
            default: "Cancel"
        },
    ],
    build: |p| {
        let mut pc = Popconfirm::new()
            .question(p.str("question"))
            .confirm_label("Delete")
            .preferred_edge(AnchorEdge::Top)
            .anchor(anchor());
        pc.open();
        {
            let mut __w = pc;
            if p.str("cancel_label") != "Cancel" {
                __w = __w.cancel_label(p.str("cancel_label"));
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = format!("Popconfirm::new().question({:?})", p.str("question"));
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[("cancel_label", ".cancel_label", SnipProp::Text("Cancel"))],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(pc) = downcast_mut::<Popconfirm>(w) {
            if let Some(res) = pc.take_result() {
                out.push(format!("result → {res:?}"));
            }
        }
    },
});

page!(DialogPage {
    meta: meta(
        "Dialog",
        "Overlays",
        "Modal dialog — title, body, response buttons.",
        "Dialog",
        &[
            ("Qt", "QDialog"),
            ("GTK", "GtkDialog"),
            ("HTML", "<dialog>"),
            ("SwiftUI", "sheet")
        ],
        true,
    ),
    props: &[PropSpec::Text {
        key: "title",
        label: "Title",
        default: "Preferences"
    }],
    build: |p| Box::new(
        Dialog::new(p.str("title"))
            .body("Dialog body content goes here.")
            .buttons(&["Cancel", "OK"]),
    ),
    snippet: |p| format!(
        "Dialog::new({:?})\n    .body(\"…\")\n    .buttons(&[\"Cancel\", \"OK\"])",
        p.str("title"),
    ),
    poll: |w, out| {
        if let Some(d) = downcast_mut::<Dialog>(w) {
            if let Some(res) = d.take_response() {
                out.push(format!("response → {res:?}"));
            }
        }
    },
});

page!(AlertDialogPage {
    meta: meta(
        "AlertDialog",
        "Overlays",
        "Severity alert with confirm/cancel roles.",
        "AlertDialog",
        &[
            ("Android", "AlertDialog"),
            ("iOS", "UIAlertController"),
            ("Qt", "QMessageBox"),
            ("React", "alert dialog")
        ],
        true,
    ),
    props: &[
        PropSpec::Text {
            key: "title",
            label: "Title",
            default: "Delete file?"
        },
        PropSpec::Text {
            key: "message",
            label: "Message",
            default: "This cannot be undone."
        },
        PropSpec::Bool {
            key: "destructive",
            label: "Destructive",
            default: true
        },
        PropSpec::Choice {
            key: "severity",
            label: "Severity",
            options: &["Info", "Warning", "Error"],
            default: 0
        },
    ],
    build: |p| {
        let mut __w = AlertDialog::new()
            .title(p.str("title"))
            .message(p.str("message"))
            .destructive(p.bool("destructive"))
            .button("Cancel", AlertRole::Cancel)
            .button("Delete", AlertRole::Confirm);
        if p.choice("severity") != 0 {
            __w = __w.severity(match p.choice("severity") {
                0 => martensite::widgets::alert_dialog::AlertSeverity::Info,
                1 => martensite::widgets::alert_dialog::AlertSeverity::Warning,
                2 => martensite::widgets::alert_dialog::AlertSeverity::Error,
                _ => martensite::widgets::alert_dialog::AlertSeverity::Info,
            });
        }
        Box::new(__w)
    },
    snippet: |p| {
        let mut __s = format!(
            "AlertDialog::new()\n    .title({:?})\n    .message({:?})\n    .destructive({})",
            p.str("title"),
            p.str("message"),
            p.bool("destructive"),
        );
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[(
                "severity",
                ".severity",
                SnipProp::Choice(&[
                    "martensite::widgets::alert_dialog::AlertSeverity::Info",
                    "martensite::widgets::alert_dialog::AlertSeverity::Warning",
                    "martensite::widgets::alert_dialog::AlertSeverity::Error",
                ]),
            )],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(d) = downcast_mut::<AlertDialog>(w) {
            if let Some(res) = d.take_result() {
                out.push(format!("result → {res:?}"));
            }
        }
    },
});

page!(ActionSheetPage {
    meta: meta(
        "ActionSheet",
        "Overlays",
        "iOS-style bottom action list — destructive + cancel.",
        "Dialog",
        &[
            ("iOS", "UIActionSheet"),
            ("Android", "bottom sheet"),
            ("Qt", "custom"),
            ("React", "action sheet")
        ],
        true,
    ),
    props: &[
        PropSpec::Text {
            key: "title",
            label: "Title",
            default: "Photo options"
        },
        PropSpec::Text {
            key: "message",
            label: "Message",
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
        let mut __w = ActionSheet::new()
            .title(p.str("title"))
            .action("Share")
            .action("Duplicate")
            .destructive("Delete")
            .cancel("Cancel");
        if !p.str("a11y_label").is_empty() {
            __w = __w.a11y_label(p.str("a11y_label"));
        }
        if !p.str("message").is_empty() {
            __w = __w.message(p.str("message"));
        }
        Box::new(__w)
    },
    snippet: |p| {
        let mut __s = {
            format!(
                "ActionSheet::new()\n    .title({:?})\n    .action(\"Share\")\n    .destructive(\"Delete\")",
                p.str("title"),
            )
        };
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("message", ".message", SnipProp::Text("")),
                ("a11y_label", ".a11y_label", SnipProp::Text("")),
            ],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(a) = downcast_mut::<ActionSheet>(w) {
            if let Some(res) = a.take_result() {
                out.push(format!("result → {res:?}"));
            }
        }
    },
});

page!(BottomSheetPage {
    meta: meta(
        "BottomSheet",
        "Overlays",
        "Sliding bottom panel with drag detents.",
        "Dialog",
        &[
            ("Android", "BottomSheet"),
            ("iOS", "sheet detents"),
            ("GNOME", "bottom sheet"),
            ("React", "sheet")
        ],
        true,
    ),
    props: &[
        PropSpec::Text {
            key: "title",
            label: "Title",
            default: "Share"
        },
        PropSpec::Text {
            key: "detents",
            label: "Detents",
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
        let mut s = BottomSheet::new()
            .title(p.str("title"))
            .child(Text::new("Sheet content"));
        s.set_fraction(0.7);
        {
            let mut __w = s;
            if !p.str("a11y_label").is_empty() {
                __w = __w.a11y_label(p.str("a11y_label"));
            }
            let __v = crate::pages::parse_f32s(p.str("detents"));
            if !__v.is_empty() {
                __w = __w.detents(&__v);
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = format!(
            "BottomSheet::new().title({:?}).child(content)",
            p.str("title")
        );
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[("a11y_label", ".a11y_label", SnipProp::Text(""))],
        ));
        __s.push_str(&crate::pages::snip_textmap(
            p,
            "detents",
            ".detents",
            "",
            crate::pages::expr_f32s,
        ));
        __s
    },
    poll: |w, out| {
        if downcast_mut::<BottomSheet>(w).is_some_and(|s| s.take_close_requested()) {
            out.push("close requested".to_string());
        }
    },
});

page!(DrawerPage {
    meta: meta(
        "Drawer",
        "Overlays",
        "Edge-sliding side panel.",
        "Dialog",
        &[
            ("Material", "Drawer"),
            ("Qt", "drawer"),
            ("Android", "navigation drawer"),
            ("React", "drawer")
        ],
        true,
    ),
    props: &[
        PropSpec::Text {
            key: "title",
            label: "Title",
            default: "Layers"
        },
        PropSpec::Float {
            key: "width",
            label: "Width",
            min: 0.0,
            max: 64.0,
            step: 0.5,
            default: 300.0
        },
    ],
    build: |p| {
        let mut __w = Drawer::new(p.str("title")).content(Text::new("Drawer content"));
        if p.f64("width") != 300.0 {
            __w = __w.width(p.f64("width") as f32);
        }
        Box::new(__w)
    },
    snippet: |p| {
        let mut __s = format!(
            "Drawer::new({:?}).content(Text::new(\"Drawer content\"))",
            p.str("title"),
        );
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[("width", ".width", SnipProp::Float(300.0))],
        ));
        __s
    },
    poll: |w, out| {
        if downcast_mut::<Drawer>(w).is_some_and(|d| d.take_close_requested()) {
            out.push("close requested".to_string());
        }
    },
});

page!(MenuPage {
    meta: meta(
        "Menu",
        "Overlays",
        "Popup menu — actions, checks, radios, submenus.",
        "Menu",
        &[
            ("Qt", "QMenu"),
            ("GTK", "GMenu"),
            ("HTML", "contextmenu"),
            ("macOS", "NSMenu")
        ],
        true,
    ),
    props: &[],
    build: |_p| {
        let m = Menu::new(vec![
            MenuItem::action("Cut"),
            MenuItem::action("Copy"),
            MenuItem::checkable("Wrap text", true),
            MenuItem::separator(),
            MenuItem::submenu(
                "Export",
                vec![MenuItem::action("PNG"), MenuItem::action("SVG")],
            ),
        ]);
        Box::new(m)
    },
    snippet: |_p| {
        "Menu::new(vec![\n    MenuItem::action(\"Cut\"),\n    MenuItem::checkable(\"Wrap text\", true),\n])"
            .to_string()
    },
    poll: |w, out| {
        if let Some(m) = downcast_mut::<Menu>(w) {
            if let Some(i) = m.take_activated() {
                out.push(format!("item {i:?}"));
            }
        }
    },
});

page!(ContextMenuPage {
    meta: meta(
        "ContextMenu",
        "Overlays",
        "Right-click menu attached to a child widget.",
        "Menu",
        &[
            ("Qt", "contextMenu"),
            ("HTML", "context menu"),
            ("GTK", "popup"),
            ("React", "context menu")
        ],
        true,
    ),
    props: &[],
    build: |_p| {
        let mut cm = ContextMenu::new(
            Text::new("Right-click me"),
            vec![MenuItem::action("Inspect"), MenuItem::action("Copy path")],
        );
        cm.open_at(Vec2::new(60.0, 60.0));
        Box::new(cm)
    },
    snippet: |_p| "ContextMenu::new(child, vec![MenuItem::action(\"Inspect\")])".to_string(),
    poll: |w, out| {
        if let Some(cm) = downcast_mut::<ContextMenu>(w) {
            if let Some(i) = cm.take_activated() {
                out.push(format!("item {i:?}"));
            }
        }
    },
});

page!(TourPage {
    meta: meta(
        "Tour",
        "Overlays",
        "Guided walkthrough — anchored step cards.",
        "Dialog",
        &[
            ("Web", "product tour"),
            ("Intro.js", "tour"),
            ("GNOME", "tour"),
            ("React", "walkthrough")
        ],
        true,
    ),
    props: &[
        PropSpec::Bool {
            key: "skippable",
            label: "Skippable",
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
        PropSpec::Text {
            key: "a11y_label",
            label: "A11y label",
            default: ""
        },
    ],
    build: |_p| {
        let mut t = Tour::new()
            .step("Welcome", "This is the stage.", None)
            .step("Props", "Edit props on the right.", Some(anchor()));
        t.restart();
        {
            let mut __w = t;
            __w = __w.enabled(_p.bool("enabled"));
            if !_p.str("a11y_label").is_empty() {
                __w = __w.label(_p.str("a11y_label"));
            }
            if !_p.bool("skippable") {
                __w = __w.skippable(_p.bool("skippable"));
            }
            Box::new(__w)
        }
    },
    snippet: |_p| {
        let mut __s =
            { "Tour::new()\n    .step(\"Welcome\", \"This is the stage.\", None)".to_string() };
        __s.push_str(&crate::pages::prop_snippet(
            _p,
            &[
                ("skippable", ".skippable", SnipProp::Bool(true)),
                ("enabled", ".enabled", SnipProp::Bool(true)),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(t) = downcast_mut::<Tour>(w) {
            if t.take_finished() {
                out.push("finished".to_string());
            }
            if t.take_dismissed() {
                out.push("dismissed".to_string());
            }
        }
    },
});

page!(HoverCardPage {
    meta: meta(
        "HoverCard",
        "Overlays",
        "Rich hover card — delayed, dismissible.",
        "Dialog",
        &[
            ("Twitter", "profile card"),
            ("Linear", "hover card"),
            ("React", "hover card"),
            ("Qt", "custom")
        ],
        true,
    ),
    props: &[
        PropSpec::Text {
            key: "title",
            label: "Title",
            default: "@martensite"
        },
        PropSpec::Int {
            key: "delay",
            label: "Delay (ms)",
            min: 0,
            max: 2000,
            default: 300
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
        let mut h = HoverCard::new(p.str("title"), "Retained-mode widget toolkit")
            .with_delay(std::time::Duration::from_millis(p.i64("delay") as u64));
        h.set_hovered(true);
        // Advance past the open delay so a static frame shows the card.
        h.tick(std::time::Duration::from_millis(p.i64("delay") as u64 + 1));
        {
            let mut __w = h;
            if !p.str("a11y_label").is_empty() {
                __w = __w.label(p.str("a11y_label"));
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = format!(
            "HoverCard::new({:?}, \"…\").with_delay({:?})",
            p.str("title"),
            p.i64("delay") as f32 / 1000.0,
        );
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[("a11y_label", ".label", SnipProp::Text(""))],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(h) = downcast_mut::<HoverCard>(w) {
            if h.take_opened() {
                out.push("opened".to_string());
            }
            if h.take_closed() {
                out.push("closed".to_string());
            }
        }
    },
});

page!(PipPage {
    meta: meta(
        "Pip",
        "Overlays",
        "Picture-in-picture floating window — drag/expand/close.",
        "Window",
        &[
            ("macOS", "PiP"),
            ("Android", "PiP"),
            ("Qt", "floating window"),
            ("React", "mini player")
        ],
        true,
    ),
    props: &[
        PropSpec::Bool {
            key: "closable",
            label: "Closable",
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
        let mut pip = Pip::new(Text::new("PiP content"));
        pip.closable = p.bool("closable");
        pip.maximizable = true;
        // Close/expand chrome only paints while hovered — stage the
        // hover so `closable` toggles a visible button.
        pip.set_hovered(true);
        {
            let mut __w = pip;
            if !p.str("a11y_label").is_empty() {
                __w = __w.label(p.str("a11y_label"));
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = format!(
            "Pip::new(content).closable({}).maximizable(true)",
            p.bool("closable"),
        );
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[("a11y_label", ".label", SnipProp::Text(""))],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(pip) = downcast_mut::<Pip>(w) {
            if pip.take_closed() {
                out.push("closed".to_string());
            }
            if pip.take_expanded() {
                out.push("expanded".to_string());
            }
        }
    },
});

page!(SwipeActionsPage {
    meta: meta(
        "SwipeActions",
        "Overlays",
        "Swipe-to-reveal row actions — mail-style.",
        "ListItem",
        &[
            ("iOS", "swipe actions"),
            ("Android", "swipe reveal"),
            ("Mail", "swipe"),
            ("React", "swipe row")
        ],
        false,
    ),
    props: &[
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
        let mut __w = SwipeActions::new(Text::new("Swipe me"))
            .leading(vec![SwipeAction::new("Archive")])
            .trailing(vec![{
                let mut a = SwipeAction::new("Delete");
                a.destructive = true;
                a
            }]);
        __w = __w.enabled(_p.bool("enabled"));
        if !_p.str("a11y_label").is_empty() {
            __w = __w.label(_p.str("a11y_label"));
        }
        Box::new(__w)
    },
    snippet: |_p| {
        let mut __s = {
            "SwipeActions::new(row)\n    .leading(vec![SwipeAction::new(\"Archive\")])\n    .trailing(vec![SwipeAction::new(\"Delete\")])"
            .to_string()
        };
        __s.push_str(&crate::pages::prop_snippet(
            _p,
            &[
                ("enabled", ".enabled", SnipProp::Bool(true)),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(sa) = downcast_mut::<SwipeActions>(w) {
            if let Some(a) = sa.take_triggered() {
                out.push(format!("action {a:?}"));
            }
        }
    },
});

/// All Overlays pages, in rail order.
pub(super) fn pages() -> Vec<Box<dyn Page>> {
    vec![
        Box::new(TooltipPage),
        Box::new(PopoverPage),
        Box::new(PopconfirmPage),
        Box::new(DialogPage),
        Box::new(AlertDialogPage),
        Box::new(ActionSheetPage),
        Box::new(BottomSheetPage),
        Box::new(DrawerPage),
        Box::new(MenuPage),
        Box::new(ContextMenuPage),
        Box::new(TourPage),
        Box::new(HoverCardPage),
        Box::new(PipPage),
        Box::new(SwipeActionsPage),
    ]
}
