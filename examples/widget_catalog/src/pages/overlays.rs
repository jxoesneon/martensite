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
use crate::pages::{downcast_mut, meta, page};

/// Stage-area anchor rect — center-ish, below the trigger zone.
fn anchor() -> Rect {
    Rect::new(40.0, 40.0, 1.0, 1.0)
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
    props: &[PropSpec::Text {
        key: "title",
        label: "Title",
        default: "Filter"
    }],
    build: |p| {
        let mut pop = Popover::new()
            .title(p.str("title"))
            .preferred_edge(AnchorEdge::Bottom)
            .child(Text::new("Popover body content"))
            .anchor(anchor());
        pop.open();
        Box::new(pop)
    },
    snippet: |p| {
        format!(
        "Popover::new()\n    .title({:?})\n    .preferred_edge(AnchorEdge::Bottom)\n    .anchor(rect)",
        p.str("title"),
    )
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
    props: &[PropSpec::Text {
        key: "question",
        label: "Question",
        default: "Delete this item?",
    }],
    build: |p| {
        let mut pc = Popconfirm::new()
            .question(p.str("question"))
            .confirm_label("Delete")
            .preferred_edge(AnchorEdge::Top)
            .anchor(anchor());
        pc.open();
        Box::new(pc)
    },
    snippet: |p| format!("Popconfirm::new().question({:?})", p.str("question")),
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
    ],
    build: |p| Box::new(
        AlertDialog::new()
            .title(p.str("title"))
            .message(p.str("message"))
            .destructive(p.bool("destructive"))
            .button("Cancel", AlertRole::Cancel)
            .button("Delete", AlertRole::Confirm),
    ),
    snippet: |p| format!(
        "AlertDialog::new()\n    .title({:?})\n    .message({:?})\n    .destructive({})",
        p.str("title"),
        p.str("message"),
        p.bool("destructive"),
    ),
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
    props: &[PropSpec::Text {
        key: "title",
        label: "Title",
        default: "Photo options"
    }],
    build: |p| Box::new(
        ActionSheet::new()
            .title(p.str("title"))
            .action("Share")
            .action("Duplicate")
            .destructive("Delete")
            .cancel("Cancel"),
    ),
    snippet: |p| {
        format!(
        "ActionSheet::new()\n    .title({:?})\n    .action(\"Share\")\n    .destructive(\"Delete\")",
        p.str("title"),
    )
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
    props: &[PropSpec::Text {
        key: "title",
        label: "Title",
        default: "Share"
    }],
    build: |p| {
        let mut s = BottomSheet::new()
            .title(p.str("title"))
            .child(Text::new("Sheet content"));
        s.set_fraction(0.7);
        Box::new(s)
    },
    snippet: |p| format!(
        "BottomSheet::new().title({:?}).child(content)",
        p.str("title")
    ),
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
    props: &[PropSpec::Text {
        key: "title",
        label: "Title",
        default: "Layers"
    }],
    build: |p| Box::new(Drawer::new(p.str("title")).content(Text::new("Drawer content")),),
    snippet: |p| format!(
        "Drawer::new({:?}).content(Text::new(\"Drawer content\"))",
        p.str("title"),
    ),
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
    props: &[],
    build: |_p| {
        let mut t = Tour::new()
            .step("Welcome", "This is the stage.", None)
            .step("Props", "Edit props on the right.", Some(anchor()));
        t.restart();
        Box::new(t)
    },
    snippet: |_p| {
        "Tour::new()\n    .step(\"Welcome\", \"This is the stage.\", None)".to_string()
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
    ],
    build: |p| {
        let mut h = HoverCard::new(p.str("title"), "Retained-mode widget toolkit")
            .with_delay(std::time::Duration::from_millis(p.i64("delay") as u64));
        h.set_hovered(true);
        Box::new(h)
    },
    snippet: |p| format!(
        "HoverCard::new({:?}, \"…\").with_delay({:?})",
        p.str("title"),
        p.i64("delay") as f32 / 1000.0,
    ),
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
    props: &[PropSpec::Bool {
        key: "closable",
        label: "Closable",
        default: true
    }],
    build: |p| {
        let mut pip = Pip::new(Text::new("PiP content"));
        pip.closable = p.bool("closable");
        pip.maximizable = true;
        Box::new(pip)
    },
    snippet: |p| format!(
        "Pip::new(content).closable({}).maximizable(true)",
        p.bool("closable"),
    ),
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
    props: &[],
    build: |_p| Box::new(
        SwipeActions::new(Text::new("Swipe me"))
            .leading(vec![SwipeAction::new("Archive")])
            .trailing(vec![{
                let mut a = SwipeAction::new("Delete");
                a.destructive = true;
                a
            }]),
    ),
    snippet: |_p| {
        "SwipeActions::new(row)\n    .leading(vec![SwipeAction::new(\"Archive\")])\n    .trailing(vec![SwipeAction::new(\"Delete\")])"
            .to_string()
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
