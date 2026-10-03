//! Navigation family — tabs, rails, stacks, menus, and chrome.

use martensite::widgets::anchor::{Anchor, AnchorItem};
use martensite::widgets::app_grid::{AppEntry, AppGrid};
use martensite::widgets::breadcrumb::Breadcrumb;
use martensite::widgets::button::Button;
use martensite::widgets::command_palette::{CommandAction, CommandPalette};
use martensite::widgets::control_center::ControlCenter;
use martensite::widgets::device_picker::{DeviceKind, DevicePicker};
use martensite::widgets::dock::{Dock, DockItem};
use martensite::widgets::filmstrip::Thumbnail;
use martensite::widgets::header_bar::HeaderBar;
use martensite::widgets::hero_header::HeroHeader;
use martensite::widgets::menu::MenuItem;
use martensite::widgets::menu_bar::MenuBar;
use martensite::widgets::nav_rail::NavRail;
use martensite::widgets::nav_stack::NavStack;
use martensite::widgets::page_header::PageHeader;
use martensite::widgets::pagination::Pagination;
use martensite::widgets::radial_menu::RadialMenu;
use martensite::widgets::resize_handle::ResizeHandle;
use martensite::widgets::ribbon::Ribbon;
use martensite::widgets::scroll_indicator::ScrollIndicator;
use martensite::widgets::speed_dial::SpeedDial;
use martensite::widgets::split_view::SplitOrientation;
use martensite::widgets::steps::Steps;
use martensite::widgets::tabs::Tabs;
use martensite::widgets::task_switcher::TaskSwitcher;
use martensite::widgets::text::Text;
use martensite::widgets::theme_picker::{ThemeOption, ThemePicker};
use martensite::widgets::tool_palette::{ToolItem, ToolPalette};
use martensite::widgets::toolbar::Toolbar;
use martensite::widgets::toolbar_overflow::ToolbarOverflow;
use martensite::widgets::window_controls::WindowControls;

use crate::page::{Page, PropSpec};
use crate::pages::{csv, downcast_mut, meta, page, SnipProp};

page!(TabsPage {
    meta: meta(
        "Tabs",
        "Navigation",
        "Tab strip with selectable panels — closable, movable.",
        "TabList",
        &[
            ("Qt", "QTabWidget"),
            ("GTK", "GtkNotebook"),
            ("SwiftUI", "TabView"),
            ("HTML", "tabs")
        ],
        false,
    ),
    props: &[
        PropSpec::Text {
            key: "tabs",
            label: "Tabs (csv)",
            default: "General,Advanced,About"
        },
        PropSpec::Bool {
            key: "closable",
            label: "Closable",
            default: false
        },
        PropSpec::Choice {
            key: "activation",
            label: "Activation",
            options: &["Automatic", "Manual"],
            default: 0
        },
        PropSpec::Bool {
            key: "movable",
            label: "Movable",
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
        let mut t = Tabs::new().closable(p.bool("closable"));
        for label in csv(p, "tabs") {
            t = t.tab(label.clone(), Text::new(format!("{label} panel content")));
        }
        {
            let mut __w = t;
            __w = __w.enabled(p.bool("enabled"));
            if !p.str("a11y_label").is_empty() {
                __w = __w.label(p.str("a11y_label"));
            }
            if p.choice("activation") != 0 {
                __w = __w.activation(match p.choice("activation") {
                    0 => martensite::widgets::tabs::TabActivation::Automatic,
                    1 => martensite::widgets::tabs::TabActivation::Manual,
                    _ => martensite::widgets::tabs::TabActivation::Automatic,
                });
            }
            if p.bool("movable") {
                __w = __w.movable(p.bool("movable"));
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = {
            let tabs = csv(p, "tabs")
                .iter()
                .map(|t| format!("{t:?}"))
                .collect::<Vec<_>>()
                .join(", ");
            format!("Tabs::new().tabs([{tabs}]) /* + panels */")
        };
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                (
                    "activation",
                    ".activation",
                    SnipProp::Choice(&[
                        "martensite::widgets::tabs::TabActivation::Automatic",
                        "martensite::widgets::tabs::TabActivation::Manual",
                    ]),
                ),
                ("movable", ".movable", SnipProp::Bool(false)),
                ("enabled", ".enabled", SnipProp::Bool(true)),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(t) = downcast_mut::<Tabs>(w) {
            if let Some(i) = t.take_close_requested() {
                out.push(format!("close tab {i}"));
            }
        }
    },
    state: |w| {
        downcast_mut::<Tabs>(w)
            .map(|t| vec![("selected".to_string(), format!("{}", t.selected()))])
            .unwrap_or_default()
    },
});

page!(NavRailPage {
    meta: meta(
        "NavRail",
        "Navigation",
        "Compact icon rail — destination navigation.",
        "NavigationBar",
        &[
            ("Material", "NavigationRail"),
            ("Qt", "icon rail"),
            ("iPadOS", "sidebar"),
            ("React", "nav rail")
        ],
        false,
    ),
    props: &[
        PropSpec::Text {
            key: "dests",
            label: "Destinations (csv)",
            default: "Home,Search,Library,Settings",
        },
        PropSpec::Int {
            key: "selected",
            label: "Selected",
            min: 0,
            max: 100,
            default: 0
        },
        PropSpec::Text {
            key: "destination_icon_icon_d",
            label: "Destination Icon Icon D",
            default: ""
        },
        PropSpec::Text {
            key: "destination_icon_label",
            label: "Destination Icon Label",
            default: ""
        },
        PropSpec::Text {
            key: "destination_named_name",
            label: "Destination Named Name",
            default: ""
        },
        PropSpec::Text {
            key: "destination_named_label",
            label: "Destination Named Label",
            default: ""
        },
        PropSpec::Text {
            key: "destinations",
            label: "Destinations",
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
        let icons = ["home", "magnify", "bookshelf", "cog"];
        let mut rail = NavRail::new();
        for (i, d) in csv(p, "dests").iter().enumerate() {
            rail = rail.destination(icons.get(i).copied().unwrap_or("dot"), d.clone());
        }
        rail.set_selected(Some(0));
        {
            let mut __w = rail;
            __w = __w.enabled(p.bool("enabled"));
            if !p.str("a11y_label").is_empty() {
                __w = __w.a11y_label(p.str("a11y_label"));
            }
            if p.i64("selected") != 0 {
                __w = __w.selected(p.i64("selected") as usize);
            }
            if !p.str("destination_icon_icon_d").is_empty()
                || !p.str("destination_icon_label").is_empty()
            {
                __w = __w.destination_icon(
                    p.str("destination_icon_icon_d"),
                    p.str("destination_icon_label"),
                );
            }
            if !p.str("destination_named_name").is_empty()
                || !p.str("destination_named_label").is_empty()
            {
                __w = __w.destination_named(
                    p.str("destination_named_name"),
                    p.str("destination_named_label"),
                );
            }
            let __v = crate::pages::csv(p, "destinations");
            if !__v.is_empty() {
                __w = __w.destinations(
                    __v.into_iter()
                        .map(|t| martensite::widgets::nav_rail::NavDestination::new("", t))
                        .collect(),
                );
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = format!(
            "NavRail::new() /* {} destinations */",
            csv(p, "dests").len(),
        );
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("selected", ".selected", SnipProp::Int(0)),
                ("enabled", ".enabled", SnipProp::Bool(true)),
                ("a11y_label", ".a11y_label", SnipProp::Text("")),
            ],
        ));
        if !p.str("destination_icon_icon_d").is_empty()
            || !p.str("destination_icon_label").is_empty()
        {
            __s.push_str(&format!(
                "\n    .destination_icon({:?}, {:?})",
                p.str("destination_icon_icon_d"),
                p.str("destination_icon_label")
            ));
        }
        if !p.str("destination_named_name").is_empty()
            || !p.str("destination_named_label").is_empty()
        {
            __s.push_str(&format!(
                "\n    .destination_named({:?}, {:?})",
                p.str("destination_named_name"),
                p.str("destination_named_label")
            ));
        }
        __s.push_str(&crate::pages::snip_textmap(
            p,
            "destinations",
            ".destinations",
            "",
            crate::pages::expr_strs,
        ));
        __s
    },
    poll: |w, out| {
        if let Some(r) = downcast_mut::<NavRail>(w) {
            if let Some(i) = r.take_activated() {
                out.push(format!("destination {i}"));
            }
        }
    },
});

page!(NavStackPage {
    meta: meta(
        "NavStack",
        "Navigation",
        "Push/pop page stack with back navigation.",
        "NavigationBar",
        &[
            ("iOS", "UINavigationController"),
            ("SwiftUI", "NavigationStack"),
            ("Android", "NavHost"),
            ("Qt", "stacked")
        ],
        false,
    ),
    props: &[
        PropSpec::Text {
            key: "title",
            label: "Title",
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
    build: |_p| {
        let mut ns = NavStack::new(Text::new("Root page"));
        ns.push(Text::new("Detail page"), "Detail");
        {
            let mut __w = ns;
            __w = __w.enabled(_p.bool("enabled"));
            if !_p.str("a11y_label").is_empty() {
                __w = __w.label(_p.str("a11y_label"));
            }
            if !_p.str("title").is_empty() {
                __w = __w.title(_p.str("title"));
            }
            Box::new(__w)
        }
    },
    snippet: |_p| {
        let mut __s = "NavStack::new(root_widget).push(detail, \"Detail\")".to_string();
        __s.push_str(&crate::pages::prop_snippet(
            _p,
            &[
                ("title", ".title", SnipProp::Text("")),
                ("enabled", ".enabled", SnipProp::Bool(true)),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(ns) = downcast_mut::<NavStack>(w) {
            if let Some((depth, title)) = ns.take_navigated() {
                out.push(format!("depth {depth} → {title}"));
            }
        }
    },
});

page!(BreadcrumbPage {
    meta: meta(
        "Breadcrumb",
        "Navigation",
        "Path trail — ancestors + current location.",
        "Navigation",
        &[
            ("HTML", "breadcrumb"),
            ("Qt", "path bar"),
            ("GNOME", "pathbar"),
            ("React", "breadcrumb")
        ],
        false,
    ),
    props: &[
        PropSpec::Text {
            key: "path",
            label: "Path (csv)",
            default: "Home,Documents,Project,RFC",
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
        let mut b = Breadcrumb::new();
        for seg in csv(p, "path") {
            b = b.push(seg);
        }
        {
            let mut __w = b;
            if !p.str("a11y_label").is_empty() {
                __w = __w.a11y_label(p.str("a11y_label"));
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = {
            let segs = csv(p, "path")
                .iter()
                .map(|s| format!("{s:?}"))
                .collect::<Vec<_>>()
                .join(", ");
            format!("Breadcrumb::new().segments([{segs}])")
        };
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[("a11y_label", ".a11y_label", SnipProp::Text(""))],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(b) = downcast_mut::<Breadcrumb>(w) {
            if let Some(i) = b.take_navigated() {
                out.push(format!("navigate → {i}"));
            }
        }
    },
});

page!(PaginationPage {
    meta: meta(
        "Pagination",
        "Navigation",
        "Page-cell strip — ‹ 1 2 … N ›.",
        "Navigation",
        &[
            ("HTML", "pagination"),
            ("iOS", "page control"),
            ("Qt", "custom"),
            ("React", "<Pagination>")
        ],
        false,
    ),
    props: &[
        PropSpec::Int {
            key: "total",
            label: "Pages",
            min: 2,
            max: 40,
            default: 12
        },
        PropSpec::Int {
            key: "current",
            label: "Current",
            min: 0,
            max: 39,
            default: 3
        },
        PropSpec::Int {
            key: "sibling_count",
            label: "Sibling Count",
            min: 0,
            max: 32,
            default: 1
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
        {
            let mut __w = Pagination::new()
                .total_pages(p.i64("total") as usize)
                .current((p.i64("current") as usize).min(p.i64("total") as usize - 1));
            if !p.str("a11y_label").is_empty() {
                __w = __w.a11y_label(p.str("a11y_label"));
            }
            if p.i64("sibling_count") != 1 {
                __w = __w.sibling_count(p.i64("sibling_count") as usize);
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = format!(
            "Pagination::new().total({}).set_current({})",
            p.i64("total"),
            p.i64("current"),
        );
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("sibling_count", ".sibling_count", SnipProp::Int(1)),
                ("a11y_label", ".a11y_label", SnipProp::Text("")),
            ],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(pg) = downcast_mut::<Pagination>(w) {
            if let Some(i) = pg.take_selected() {
                out.push(format!("page → {i}"));
            }
        }
    },
});

page!(StepsPage {
    meta: meta(
        "Steps",
        "Navigation",
        "Horizontal stepper — multi-step flow progress.",
        "Navigation",
        &[
            ("Ant", "Steps"),
            ("Material", "stepper"),
            ("Qt", "wizard steps"),
            ("React", "stepper")
        ],
        false,
    ),
    props: &[
        PropSpec::Text {
            key: "steps",
            label: "Steps (csv)",
            default: "Account,Profile,Confirm"
        },
        PropSpec::Int {
            key: "current",
            label: "Current",
            min: 0,
            max: 9,
            default: 1
        },
        PropSpec::Bool {
            key: "clickable_completed",
            label: "Clickable Completed",
            default: true
        },
        PropSpec::Text {
            key: "steps_full",
            label: "Steps Full",
            default: ""
        },
    ],
    build: |p| {
        let mut s = Steps::new().steps(csv(p, "steps"));
        s.set_current((p.i64("current") as usize).min(csv(p, "steps").len().saturating_sub(1)));
        {
            let mut __w = s;
            if !p.bool("clickable_completed") {
                __w = __w.clickable_completed(p.bool("clickable_completed"));
            }
            let __v = crate::pages::csv(p, "steps_full");
            if !__v.is_empty() {
                __w = __w.steps_full(
                    __v.into_iter()
                        .map(martensite::widgets::steps::Step::new)
                        .collect::<Vec<_>>(),
                );
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = {
            let steps = csv(p, "steps")
                .iter()
                .map(|s| format!("{s:?}"))
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "Steps::new().steps([{steps}]).set_current({})",
                p.i64("current")
            )
        };
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[(
                "clickable_completed",
                ".clickable_completed",
                SnipProp::Bool(true),
            )],
        ));
        __s.push_str(&crate::pages::snip_textmap(
            p,
            "steps_full",
            ".steps_full",
            "",
            crate::pages::expr_strs,
        ));
        __s
    },
    poll: |w, out| {
        if let Some(s) = downcast_mut::<Steps>(w) {
            if let Some(i) = s.take_navigated() {
                out.push(format!("step → {i}"));
            }
        }
    },
});

page!(MenuBarPage {
    meta: meta(
        "MenuBar",
        "Navigation",
        "Top-of-window menu row — File/Edit/View.",
        "MenuBar",
        &[
            ("Qt", "QMenuBar"),
            ("macOS", "menu bar"),
            ("GTK", "popover bar"),
            ("Win32", "menu")
        ],
        true,
    ),
    props: &[],
    build: |_p| Box::new(
        MenuBar::new()
            .menu(
                "File",
                vec![MenuItem::action("New"), MenuItem::action("Open")]
            )
            .menu(
                "Edit",
                vec![MenuItem::action("Undo"), MenuItem::action("Redo")]
            ),
    ),
    snippet: |_p| {
        "MenuBar::new()\n    .menu(\"File\", vec![MenuItem::action(\"New\")])".to_string()
    },
    poll: |w, out| {
        if let Some(mb) = downcast_mut::<MenuBar>(w) {
            if let Some(a) = mb.take_activated() {
                out.push(format!("menu → {a:?}"));
            }
        }
    },
});

page!(DockPage {
    meta: meta(
        "Dock",
        "Navigation",
        "macOS-style app dock with magnification.",
        "Toolbar",
        &[
            ("macOS", "Dock"),
            ("GNOME", "dash"),
            ("Qt", "custom"),
            ("React", "dock")
        ],
        false,
    ),
    props: &[
        PropSpec::Bool {
            key: "magnify",
            label: "Magnify",
            default: true
        },
        PropSpec::Float {
            key: "magnification",
            label: "Magnification",
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
        let colors = [
            [80, 140, 255, 255],
            [240, 160, 60, 255],
            [90, 200, 120, 255],
        ];
        let mut d = Dock::new();
        for (i, name) in ["Finder", "Editor", "Terminal"].iter().enumerate() {
            d = d.item(DockItem::new(name.to_string(), colors[i % 3]));
        }
        if p.bool("magnify") {
            // magnification is on by default; prop kept for parity
        }
        {
            let mut __w = d;
            if !p.str("a11y_label").is_empty() {
                __w = __w.label(p.str("a11y_label"));
            }
            if p.f64("magnification") != 0.0 {
                __w = __w.magnification(p.f64("magnification") as f32);
            }
            Box::new(__w)
        }
    },
    snippet: |_p| {
        let mut __s = "Dock::new().item(DockItem::new(\"Finder\", color))".to_string();
        __s.push_str(&crate::pages::prop_snippet(
            _p,
            &[
                ("magnification", ".magnification", SnipProp::Float(0.0)),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(d) = downcast_mut::<Dock>(w) {
            if let Some(i) = d.take_launched() {
                out.push(format!("launch {i}"));
            }
        }
    },
});

page!(ToolbarPage {
    meta: meta(
        "Toolbar",
        "Navigation",
        "App toolbar — buttons, separators, spacers, overflow.",
        "Toolbar",
        &[
            ("Qt", "QToolBar"),
            ("GTK", "GtkHeaderBar"),
            ("macOS", "NSToolbar"),
            ("React", "toolbar")
        ],
        false,
    ),
    props: &[
        PropSpec::Int {
            key: "items",
            label: "Items",
            min: 2,
            max: 12,
            default: 5
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
        let mut tb = Toolbar::new().label("Toolbar");
        for i in 1..=p.i64("items") {
            tb = tb.item(Button::new(format!("Action {i}")));
        }
        {
            let mut __w = tb;
            __w = __w.enabled(p.bool("enabled"));
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = format!("Toolbar::new() /* {} button items */", p.i64("items"),);
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[("enabled", ".enabled", SnipProp::Bool(true))],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(tb) = downcast_mut::<Toolbar>(w) {
            if let Some(i) = tb.take_activated() {
                out.push(format!("item {i}"));
            }
        }
    },
});

page!(ToolbarOverflowPage {
    meta: meta(
        "ToolbarOverflow",
        "Navigation",
        "Toolbar that collapses items into an overflow menu.",
        "Toolbar",
        &[
            ("GTK", "AdwToolbarView"),
            ("Qt", "extension"),
            ("React", "responsive toolbar"),
            ("macOS", "NSToolbar")
        ],
        true,
    ),
    props: &[PropSpec::Int {
        key: "items",
        label: "Items",
        min: 2,
        max: 16,
        default: 8
    }],
    build: |p| {
        let mut tb = ToolbarOverflow::new().label("Toolbar");
        for i in 1..=p.i64("items") {
            tb = tb.item(format!("{i}"));
        }
        Box::new(tb)
    },
    snippet: |p| format!("ToolbarOverflow::new() /* {} items */", p.i64("items")),
    poll: |w, out| {
        if let Some(tb) = downcast_mut::<ToolbarOverflow>(w) {
            if let Some(i) = tb.take_activated() {
                out.push(format!("item {i}"));
            }
        }
    },
});

page!(HeaderBarPage {
    meta: meta(
        "HeaderBar",
        "Navigation",
        "Window title bar — leading/trailing slots + title/subtitle.",
        "Toolbar",
        &[
            ("GTK", "GtkHeaderBar"),
            ("Qt", "title bar"),
            ("macOS", "titlebar"),
            ("GNOME", "AdwHeaderBar")
        ],
        false,
    ),
    props: &[
        PropSpec::Text {
            key: "title",
            label: "Title",
            default: "Document"
        },
        PropSpec::Text {
            key: "subtitle",
            label: "Subtitle",
            default: "editing"
        },
    ],
    build: |p| Box::new(
        HeaderBar::new(p.str("title"))
            .subtitle(p.str("subtitle"))
            .trailing(Button::new("Save")),
    ),
    snippet: |p| format!(
        "HeaderBar::new({:?})\n    .subtitle({:?})\n    .trailing(Button::new(\"Save\"))",
        p.str("title"),
        p.str("subtitle"),
    ),
});

page!(PageHeaderPage {
    meta: meta(
        "PageHeader",
        "Navigation",
        "In-content page header — back button + title + actions.",
        "Toolbar",
        &[
            ("Ant", "PageHeader"),
            ("Qt", "header"),
            ("React", "page header"),
            ("GNOME", "view title")
        ],
        false,
    ),
    props: &[
        PropSpec::Text {
            key: "title",
            label: "Title",
            default: "Settings"
        },
        PropSpec::Text {
            key: "subtitle",
            label: "Subtitle",
            default: ""
        },
    ],
    build: |p| {
        let mut __w = PageHeader::new(p.str("title"))
            .back(true)
            .action(Button::new("Apply"));
        if !p.str("subtitle").is_empty() {
            __w = __w.subtitle(p.str("subtitle"));
        }
        Box::new(__w)
    },
    snippet: |p| {
        let mut __s = format!(
            "PageHeader::new({:?}).back().action(Button::new(\"Apply\"))",
            p.str("title"),
        );
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[("subtitle", ".subtitle", SnipProp::Text(""))],
        ));
        __s
    },
    poll: |w, out| {
        if downcast_mut::<PageHeader>(w).is_some_and(|h| h.take_back()) {
            out.push("back".to_string());
        }
    },
});

page!(CommandPalettePage {
    meta: meta(
        "CommandPalette",
        "Navigation",
        "Fuzzy action launcher — type to filter commands.",
        "Dialog",
        &[
            ("VS Code", "command palette"),
            ("JetBrains", "Search Everywhere"),
            ("React", "cmdk"),
            ("macOS", "Spotlight")
        ],
        true,
    ),
    props: &[
        PropSpec::Text {
            key: "placeholder",
            label: "Placeholder",
            default: ""
        },
        PropSpec::Int {
            key: "max_results",
            label: "Max Results",
            min: 0,
            max: 100,
            default: 0
        },
        PropSpec::Choice {
            key: "sanitize",
            label: "Sanitize",
            options: &["Aggressive", "Baseline", "Raw"],
            default: 0
        },
        PropSpec::Text {
            key: "with_query",
            label: "With Query",
            default: ""
        },
        PropSpec::Text {
            key: "actions",
            label: "Actions",
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
    build: |_p| {
        let mut cp = CommandPalette::new()
            .action(CommandAction::new("open", "Open File"))
            .action(CommandAction::new("save", "Save All"))
            .action(CommandAction::new("prefs", "Preferences"));
        cp.open();
        {
            let mut __w = cp;
            __w = __w.enabled(_p.bool("enabled"));
            __w = __w.loading(_p.bool("loading"));
            if !_p.str("a11y_label").is_empty() {
                __w = __w.label(_p.str("a11y_label"));
            }
            if !_p.str("placeholder").is_empty() {
                __w = __w.placeholder(_p.str("placeholder"));
            }
            if _p.i64("max_results") != 0 {
                __w = __w.max_results(_p.i64("max_results") as usize);
            }
            __w.set_sanitizer(crate::pages::sanitize_cfg(_p));
            if !_p.str("with_query").is_empty() {
                __w = __w.with_query(_p.str("with_query"));
            }
            let __v = crate::pages::csv(_p, "actions");
            if !__v.is_empty() {
                __w = __w.actions(__v.into_iter().map(|t| {
                    martensite::widgets::command_palette::CommandAction::new(t.clone(), t)
                }));
            }
            Box::new(__w)
        }
    },
    snippet: |_p| {
        let mut __s = {
            "CommandPalette::new()\n    .action(CommandAction::new(\"open\", \"Open File\"))"
                .to_string()
        };
        __s.push_str(&crate::pages::prop_snippet(
            _p,
            &[
                ("placeholder", ".placeholder", SnipProp::Text("")),
                ("max_results", ".max_results", SnipProp::Int(0)),
                ("with_query", ".with_query", SnipProp::Text("")),
                ("enabled", ".enabled", SnipProp::Bool(true)),
                ("loading", ".loading", SnipProp::Bool(false)),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        __s.push_str(&crate::pages::snip_textmap(
            _p,
            "actions",
            ".actions",
            "",
            crate::pages::expr_strs,
        ));
        __s.push_str(crate::pages::sanitize_snippet(_p));
        __s
    },
    poll: |w, out| {
        if let Some(cp) = downcast_mut::<CommandPalette>(w) {
            if let Some(id) = cp.take_activated() {
                out.push(format!("command → {id}"));
            }
        }
    },
});

page!(ToolPalettePage {
    meta: meta(
        "ToolPalette",
        "Navigation",
        "Grid of tool icons — paint-app tool strip.",
        "Grid",
        &[
            ("GIMP", "toolbox"),
            ("Qt", "tool palette"),
            ("iOS", "tool strip"),
            ("React", "tool grid")
        ],
        false,
    ),
    props: &[
        PropSpec::Int {
            key: "cols",
            label: "Columns",
            min: 2,
            max: 8,
            default: 4
        },
        PropSpec::Int {
            key: "selected",
            label: "Selected",
            min: 0,
            max: 100,
            default: 0
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
        let mut tp = ToolPalette::new()
            .columns(p.i64("cols") as usize)
            .show_labels(true);
        for (glyph, label) in [
            ("✏", "Brush"),
            ("⬚", "Select"),
            ("🗑", "Erase"),
            ("⤢", "Move"),
        ] {
            tp = tp.tool(ToolItem::new(glyph, label));
        }
        {
            let mut __w = tp;
            if !p.str("a11y_label").is_empty() {
                __w = __w.label(p.str("a11y_label"));
            }
            if p.i64("selected") != 0 {
                __w = __w.selected(p.i64("selected") as usize);
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = format!("ToolPalette::new().columns({})", p.i64("cols"));
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("selected", ".selected", SnipProp::Int(0)),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(tp) = downcast_mut::<ToolPalette>(w) {
            if let Some(i) = tp.take_selected() {
                out.push(format!("tool {i}"));
            }
        }
    },
});

page!(RadialMenuPage {
    meta: meta(
        "RadialMenu",
        "Navigation",
        "Pie menu — radial slices around the cursor.",
        "Menu",
        &[
            ("Maya", "marking menu"),
            ("Qt", "pie menu"),
            ("Game", "radial"),
            ("React", "radial menu")
        ],
        true,
    ),
    props: &[
        PropSpec::Text {
            key: "items",
            label: "Items (csv)",
            default: "Cut,Copy,Paste,Undo,Redo",
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
        let mut __w = RadialMenu::new().items(csv(p, "items"));
        if !p.str("a11y_label").is_empty() {
            __w = __w.label(p.str("a11y_label"));
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
            format!("RadialMenu::new().items([{items}])")
        };
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[("a11y_label", ".label", SnipProp::Text(""))],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(r) = downcast_mut::<RadialMenu>(w) {
            if let Some(i) = r.take_selected() {
                out.push(format!("slice {i}"));
            }
        }
    },
});

page!(SpeedDialPage {
    meta: meta(
        "SpeedDial",
        "Navigation",
        "Expandable FAB — fans out to quick actions.",
        "Button",
        &[
            ("Material", "SpeedDial"),
            ("Android", "FAB menu"),
            ("React", "speed dial"),
            ("Qt", "custom")
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
        let mut __w = SpeedDial::new()
            .action("Compose")
            .action("Share")
            .action("Archive");
        if !_p.str("a11y_label").is_empty() {
            __w = __w.label(_p.str("a11y_label"));
        }
        Box::new(__w)
    },
    snippet: |_p| {
        let mut __s = "SpeedDial::new().action(\"Compose\").action(\"Share\")".to_string();
        __s.push_str(&crate::pages::prop_snippet(
            _p,
            &[("a11y_label", ".label", SnipProp::Text(""))],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(sd) = downcast_mut::<SpeedDial>(w) {
            if let Some(i) = sd.take_action() {
                out.push(format!("action {i}"));
            }
        }
    },
});

page!(AppGridPage {
    meta: meta(
        "AppGrid",
        "Navigation",
        "Launcher grid of app icons — paged.",
        "Grid",
        &[
            ("GNOME", "app grid"),
            ("iOS", "home screen"),
            ("Android", "launcher"),
            ("Qt", "icon grid")
        ],
        false,
    ),
    props: &[
        PropSpec::Int {
            key: "cols",
            label: "Columns",
            min: 2,
            max: 8,
            default: 4
        },
        PropSpec::Text {
            key: "page_size",
            label: "Page Size (csv)",
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
        let mut g = AppGrid::new();
        g.columns = p.i64("cols") as usize;
        let colors = [[80, 140, 255, 255], [240, 90, 90, 255], [90, 200, 120, 255]];
        for (i, name) in ["Mail", "Maps", "Music", "Photos", "Notes", "Files"]
            .iter()
            .enumerate()
        {
            g = g.app(AppEntry::new(name.to_string(), colors[i % 3]));
        }
        {
            let mut __w = g;
            if !p.str("a11y_label").is_empty() {
                __w = __w.label(p.str("a11y_label"));
            }
            if let Some(v) = crate::pages::parse_pair(p.str("page_size")) {
                __w = __w.page_size(v.0 as usize, v.1 as usize);
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = format!("AppGrid::new().columns({})", p.i64("cols"));
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[("a11y_label", ".label", SnipProp::Text(""))],
        ));
        __s.push_str(&crate::pages::snip_textmap(
            p,
            "page_size",
            ".page_size",
            "",
            crate::pages::expr_pair,
        ));
        __s
    },
    poll: |w, out| {
        if let Some(g) = downcast_mut::<AppGrid>(w) {
            if let Some(i) = g.take_activated() {
                out.push(format!("launch {i}"));
            }
        }
    },
});

page!(DevicePickerPage {
    meta: meta(
        "DevicePicker",
        "Navigation",
        "Sectioned device chooser — mics, speakers, cameras.",
        "List",
        &[
            ("Zoom", "device picker"),
            ("GNOME", "sound prefs"),
            ("Qt", "custom"),
            ("React", "device list")
        ],
        false,
    ),
    props: &[
        PropSpec::Choice {
            key: "active_kind",
            label: "Active Kind",
            options: &["Microphone", "Speaker", "Camera"],
            default: 0
        },
        PropSpec::Int {
            key: "active_index",
            label: "Active Index",
            min: 0,
            max: 32,
            default: 0
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
    build: |_p| {
        let mut __w = DevicePicker::new()
            .section(DeviceKind::Microphone, ["Internal Mic", "USB Mic"])
            .section(DeviceKind::Speaker, ["Speakers", "Headphones"]);
        if !_p.str("a11y_label").is_empty() {
            __w = __w.label(_p.str("a11y_label"));
        }
        if _p.choice("active_kind") != 0 || _p.i64("active_index") != 0 {
            __w = __w.active(
                match _p.choice("active_kind") {
                    0 => martensite::widgets::device_picker::DeviceKind::Microphone,
                    1 => martensite::widgets::device_picker::DeviceKind::Speaker,
                    2 => martensite::widgets::device_picker::DeviceKind::Camera,
                    _ => martensite::widgets::device_picker::DeviceKind::Microphone,
                },
                _p.i64("active_index") as usize,
            );
        }
        Box::new(__w)
    },
    snippet: |_p| {
        let mut __s = {
            "DevicePicker::new()\n    .section(DeviceKind::Microphone, [\"Internal Mic\"])"
                .to_string()
        };
        __s.push_str(&crate::pages::prop_snippet(
            _p,
            &[("a11y_label", ".label", SnipProp::Text(""))],
        ));
        if _p.choice("active_kind") != 0 || _p.i64("active_index") != 0 {
            __s.push_str(&format!(
                "\n    .active({}, {})",
                [
                    "martensite::widgets::device_picker::DeviceKind::Microphone",
                    "martensite::widgets::device_picker::DeviceKind::Speaker",
                    "martensite::widgets::device_picker::DeviceKind::Camera"
                ][_p.choice("active_kind")],
                _p.i64("active_index")
            ));
        }
        __s
    },
    poll: |w, out| {
        if let Some(dp) = downcast_mut::<DevicePicker>(w) {
            if let Some(s) = dp.take_selected() {
                out.push(format!("selected {s:?}"));
            }
        }
    },
});

page!(ThemePickerPage {
    meta: meta(
        "ThemePicker",
        "Navigation",
        "Visual theme swatch picker.",
        "List",
        &[
            ("GNOME", "style picker"),
            ("macOS", "appearance"),
            ("Qt", "custom"),
            ("React", "theme toggle")
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
        let mut __w = ThemePicker::new()
            .option(ThemeOption::new(
                "Light",
                [250, 250, 250, 255],
                [20, 20, 20, 255],
                [80, 140, 255, 255],
            ))
            .option(ThemeOption::new(
                "Dark",
                [24, 26, 32, 255],
                [230, 230, 235, 255],
                [120, 160, 255, 255],
            ));
        if !_p.str("a11y_label").is_empty() {
            __w = __w.label(_p.str("a11y_label"));
        }
        Box::new(__w)
    },
    snippet: |_p| {
        let mut __s = "ThemePicker::new().option(ThemeOption::new(\"Dark\", …))".to_string();
        __s.push_str(&crate::pages::prop_snippet(
            _p,
            &[("a11y_label", ".label", SnipProp::Text(""))],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(tp) = downcast_mut::<ThemePicker>(w) {
            if let Some(i) = tp.take_selected() {
                out.push(format!("theme {i}"));
            }
        }
    },
});

page!(WindowControlsPage {
    meta: meta(
        "WindowControls",
        "Navigation",
        "Min/max/close chrome buttons — platform-styled.",
        "Button",
        &[
            ("Win32", "caption buttons"),
            ("macOS", "traffic lights"),
            ("Qt", "window controls"),
            ("GTK", "CSD")
        ],
        false,
    ),
    props: &[
        PropSpec::Bool {
            key: "mac",
            label: "mac style",
            default: false
        },
        PropSpec::Choice {
            key: "style",
            label: "Style",
            options: &["Windows", "Mac"],
            default: 0
        },
        PropSpec::Bool {
            key: "maximized",
            label: "Maximized",
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
        if p.bool("mac") {
            Box::new(WindowControls::mac())
        } else {
            {
                let mut __w = WindowControls::new();
                if !p.str("a11y_label").is_empty() {
                    __w = __w.label(p.str("a11y_label"));
                }
                if p.choice("style") != 0 {
                    __w = __w.style(match p.choice("style") {
                        0 => martensite::widgets::window_controls::CaptionStyle::Windows,
                        1 => martensite::widgets::window_controls::CaptionStyle::Mac,
                        _ => martensite::widgets::window_controls::CaptionStyle::Windows,
                    });
                }
                if p.bool("maximized") {
                    __w = __w.maximized(p.bool("maximized"));
                }
                Box::new(__w)
            }
        }
    },
    snippet: |p| {
        let mut __s = format!("WindowControls::new() /* mac={} */", p.bool("mac"));
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                (
                    "style",
                    ".style",
                    SnipProp::Choice(&[
                        "martensite::widgets::window_controls::CaptionStyle::Windows",
                        "martensite::widgets::window_controls::CaptionStyle::Mac",
                    ]),
                ),
                ("maximized", ".maximized", SnipProp::Bool(false)),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(wc) = downcast_mut::<WindowControls>(w) {
            if let Some(a) = wc.take_action() {
                out.push(format!("window → {a:?}"));
            }
        }
    },
});

page!(ControlCenterPage {
    meta: meta(
        "ControlCenter",
        "Navigation",
        "macOS-style toggle tile + slider grid.",
        "Panel",
        &[
            ("macOS", "Control Center"),
            ("iOS", "Control Center"),
            ("GNOME", "quick settings"),
            ("Android", "shade")
        ],
        true,
    ),
    props: &[
        PropSpec::Text {
            key: "tile_named_name",
            label: "Tile Named Name",
            default: ""
        },
        PropSpec::Text {
            key: "tile_named_title",
            label: "Tile Named Title",
            default: ""
        },
        PropSpec::Bool {
            key: "tile_named_on",
            label: "Tile Named On",
            default: false
        },
        PropSpec::Text {
            key: "slider_named_name",
            label: "Slider Named Name",
            default: ""
        },
        PropSpec::Text {
            key: "slider_named_title",
            label: "Slider Named Title",
            default: ""
        },
        PropSpec::Float {
            key: "slider_named_value",
            label: "Slider Named Value",
            min: 0.0,
            max: 100.0,
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
    build: |_p| {
        let mut __w = ControlCenter::new()
            .tile("wifi", "Wi-Fi", true)
            .tile("bluetooth", "Bluetooth", false)
            .slider("sun", "Brightness", 0.7);
        if !_p.str("a11y_label").is_empty() {
            __w = __w.label(_p.str("a11y_label"));
        }
        if !_p.str("tile_named_name").is_empty()
            || !_p.str("tile_named_title").is_empty()
            || !_p.bool("tile_named_on")
        {
            __w = __w.tile_named(
                _p.str("tile_named_name"),
                _p.str("tile_named_title"),
                _p.bool("tile_named_on"),
            );
        }
        if !_p.str("slider_named_name").is_empty()
            || !_p.str("slider_named_title").is_empty()
            || _p.f64("slider_named_value") != 0.0
        {
            __w = __w.slider_named(
                _p.str("slider_named_name"),
                _p.str("slider_named_title"),
                _p.f64("slider_named_value") as f32,
            );
        }
        Box::new(__w)
    },
    snippet: |_p| {
        let mut __s = {
            "ControlCenter::new()\n    .tile(\"wifi\", \"Wi-Fi\", true)\n    .slider(\"sun\", \"Brightness\", 0.7)".to_string()
        };
        __s.push_str(&crate::pages::prop_snippet(
            _p,
            &[("a11y_label", ".label", SnipProp::Text(""))],
        ));
        if !_p.str("tile_named_name").is_empty()
            || !_p.str("tile_named_title").is_empty()
            || !_p.bool("tile_named_on")
        {
            __s.push_str(&format!(
                "\n    .tile_named({:?}, {:?}, {:?})",
                _p.str("tile_named_name"),
                _p.str("tile_named_title"),
                _p.bool("tile_named_on")
            ));
        }
        if !_p.str("slider_named_name").is_empty()
            || !_p.str("slider_named_title").is_empty()
            || _p.f64("slider_named_value") != 0.0
        {
            __s.push_str(&format!(
                "\n    .slider_named({:?}, {:?}, {})",
                _p.str("slider_named_name"),
                _p.str("slider_named_title"),
                _p.f64("slider_named_value")
            ));
        }
        __s
    },
    poll: |w, out| {
        if let Some(cc) = downcast_mut::<ControlCenter>(w) {
            if let Some(t) = cc.take_toggled() {
                out.push(format!("tile {t:?}"));
            }
            if let Some((name, v)) = cc.take_adjusted() {
                out.push(format!("{name} → {v:.2}"));
            }
        }
    },
});

page!(AnchorPage {
    meta: meta(
        "Anchor",
        "Navigation",
        "Scroll-spy anchor list — highlights the section in view.",
        "List",
        &[
            ("Docs", "table of contents"),
            ("HTML", "anchor nav"),
            ("React", "scroll spy"),
            ("Qt", "custom")
        ],
        false,
    ),
    props: &[
        PropSpec::Text {
            key: "items",
            label: "Items (csv)",
            default: "Intro,Usage,API,Changelog",
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
        let items: Vec<AnchorItem> = csv(p, "items")
            .iter()
            .map(|i| AnchorItem::new(i.clone(), format!("#{i}")))
            .collect();
        {
            let mut __w = Anchor::new().items(items);
            __w = __w.enabled(p.bool("enabled"));
            if !p.str("a11y_label").is_empty() {
                __w = __w.label(p.str("a11y_label"));
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = format!("Anchor::new() /* {} items */", csv(p, "items").len());
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
        if let Some(a) = downcast_mut::<Anchor>(w) {
            if let Some((idx, t)) = a.take_clicked() {
                out.push(format!("anchor {idx} → {t}"));
            }
        }
    },
});

page!(HeroHeaderPage {
    meta: meta(
        "HeroHeader",
        "Navigation",
        "Landing hero — eyebrow, title, subtitle, dual CTAs.",
        "Group",
        &[
            ("Web", "hero section"),
            ("Marketing", "masthead"),
            ("React", "hero"),
            ("Qt", "custom")
        ],
        false,
    ),
    props: &[
        PropSpec::Text {
            key: "title",
            label: "Title",
            default: "Ship faster"
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
        let mut __w = HeroHeader::new(p.str("title"))
            .eyebrow("Martensite")
            .subtitle("Retained-mode widgets with a real pipeline.")
            .primary("Get started")
            .secondary("Docs");
        if !p.str("a11y_label").is_empty() {
            __w = __w.label(p.str("a11y_label"));
        }
        Box::new(__w)
    },
    snippet: |p| {
        let mut __s = format!(
            "HeroHeader::new({:?}).primary(\"Get started\")",
            p.str("title")
        );
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[("a11y_label", ".label", SnipProp::Text(""))],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(h) = downcast_mut::<HeroHeader>(w) {
            if let Some(a) = h.take_action() {
                out.push(format!("cta → {a:?}"));
            }
        }
    },
});

page!(ResizeHandlePage {
    meta: meta(
        "ResizeHandle",
        "Navigation",
        "Draggable split divider — used between panes.",
        "Separator",
        &[
            ("Qt", "splitter handle"),
            ("GTK", "paned handle"),
            ("Win32", "splitter"),
            ("React", "divider")
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
        let o = if p.choice("orientation") == 1 {
            SplitOrientation::Vertical
        } else {
            SplitOrientation::Horizontal
        };
        {
            let mut __w = ResizeHandle::new(o);
            __w = __w.enabled(p.bool("enabled"));
            if !p.str("a11y_label").is_empty() {
                __w = __w.label(p.str("a11y_label"));
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = format!(
            "ResizeHandle::new(SplitOrientation::{})",
            if p.choice("orientation") == 1 {
                "Vertical"
            } else {
                "Horizontal"
            },
        );
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
        if let Some(rh) = downcast_mut::<ResizeHandle>(w) {
            if let Some(delta) = rh.take_moved() {
                out.push(format!("moved {delta:+.1}"));
            }
        }
    },
});

page!(ScrollIndicatorPage {
    meta: meta(
        "ScrollIndicator",
        "Navigation",
        "Overlay scrollbar indicator — thumb + track.",
        "ScrollBar",
        &[
            ("iOS", "scroll indicator"),
            ("Qt", "QScrollBar"),
            ("GNOME", "overlay scrollbar"),
            ("React", "scrollbar")
        ],
        false,
    ),
    props: &[
        PropSpec::Float {
            key: "fraction",
            label: "Scroll",
            min: 0.0,
            max: 1.0,
            step: 0.05,
            default: 0.3,
        },
        PropSpec::Bool {
            key: "always_visible",
            label: "Always Visible",
            default: false
        },
        PropSpec::Text {
            key: "scroll",
            label: "Scroll (csv)",
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
        let mut si = ScrollIndicator::new(SplitOrientation::Horizontal);
        si.set_scroll(p.f64("fraction") as f32, 0.25);
        {
            let mut __w = si;
            if !p.str("a11y_label").is_empty() {
                __w = __w.label(p.str("a11y_label"));
            }
            if p.bool("always_visible") {
                __w = __w.always_visible(p.bool("always_visible"));
            }
            if let Some(v) = crate::pages::parse_pair(p.str("scroll")) {
                __w = __w.scroll(v.0 as f32, v.1 as f32);
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = format!(
            "ScrollIndicator::new(Horizontal).set_scroll({:?}, 0.25)",
            p.f64("fraction") as f32,
        );
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("always_visible", ".always_visible", SnipProp::Bool(false)),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        __s.push_str(&crate::pages::snip_textmap(
            p,
            "scroll",
            ".scroll",
            "",
            crate::pages::expr_pair,
        ));
        __s
    },
});

page!(TaskSwitcherPage {
    meta: meta(
        "TaskSwitcher",
        "Navigation",
        "Alt-Tab window switcher — thumbnail strip.",
        "List",
        &[
            ("Win32", "Alt+Tab"),
            ("macOS", "cmd-tab"),
            ("GNOME", "switcher"),
            ("Qt", "custom")
        ],
        true,
    ),
    props: &[
        PropSpec::Int {
            key: "index",
            label: "Index",
            min: 0,
            max: 100,
            default: 0
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
    build: |_p| {
        let mut ts = TaskSwitcher::new();
        for (i, name) in ["Editor", "Terminal", "Browser"].iter().enumerate() {
            ts = ts.item(Thumbnail::new(
                name.to_string(),
                [90, 120 + i as u8 * 40, 200, 255],
            ));
        }
        {
            let mut __w = ts;
            if !_p.str("a11y_label").is_empty() {
                __w = __w.label(_p.str("a11y_label"));
            }
            if _p.i64("index") != 0 {
                __w = __w.index(_p.i64("index") as usize);
            }
            Box::new(__w)
        }
    },
    snippet: |_p| {
        let mut __s = "TaskSwitcher::new().item(Thumbnail::new(\"Editor\", …))".to_string();
        __s.push_str(&crate::pages::prop_snippet(
            _p,
            &[
                ("index", ".index", SnipProp::Int(0)),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(ts) = downcast_mut::<TaskSwitcher>(w) {
            if let Some(i) = ts.take_selected() {
                out.push(format!("window {i}"));
            }
        }
    },
});

page!(RibbonPage {
    meta: meta(
        "Ribbon",
        "Navigation",
        "Corner ribbon tag on a child — \"Beta\", \"New\".",
        "Text",
        &[
            ("Web", "corner ribbon"),
            ("Qt", "badge"),
            ("React", "ribbon tag"),
            ("CSS", "corner tag")
        ],
        false,
    ),
    props: &[
        PropSpec::Text {
            key: "text",
            label: "Text",
            default: "Beta"
        },
        PropSpec::Choice {
            key: "corner",
            label: "Corner",
            options: &["Top End", "Top Start"],
            default: 0
        },
        PropSpec::Choice {
            key: "color",
            label: "Color",
            options: &[
                "Background Color",
                "Surface Color",
                "Primary Color",
                "Secondary Color",
                "Accent Color",
                "Text Color",
                "Text Muted Color",
                "Text Inverse Color",
                "Border Color",
                "Divider Color",
                "Raised Color",
                "Error Color",
                "Warning Color",
                "Success Color",
                "Info Color",
                "Spacing",
                "Spacing Small",
                "Spacing Large",
                "Border Radius",
                "Border Radius Small",
                "Border Radius Large",
                "Font Size Small",
                "Font Size Medium",
                "Font Size Large",
                "Animation Duration",
                "Animation Easing",
                "Backdrop Material",
                "Backdrop Tint Opacity",
                "Backdrop Fallback Color",
                "Csd Title Bar Height",
                "Csd Button Radius",
                "Csd Shadow Blur",
                "Csd Shadow Color",
                "Vibrancy Material",
                "Scrim Color",
                "Inset Color",
                "Overlay Color",
                "Series Color1",
                "Series Color2",
                "Series Color3",
                "Series Color4",
                "Series Color5",
                "Series Color6",
                "Font Size Micro",
                "Font Size Caption",
                "Font Size Body",
                "Font Size Title",
                "Font Size Display"
            ],
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
        let mut __w = Ribbon::new(p.str("text")).child(
            martensite::widgets::container::Container::new()
                .padding_uniform(24.0)
                .child(Text::new("Card with ribbon")),
        );
        __w = __w.enabled(p.bool("enabled"));
        if p.choice("corner") != 0 {
            __w = __w.corner(match p.choice("corner") {
                0 => martensite::widgets::ribbon::RibbonCorner::TopEnd,
                1 => martensite::widgets::ribbon::RibbonCorner::TopStart,
                _ => martensite::widgets::ribbon::RibbonCorner::TopEnd,
            });
        }
        if p.choice("color") != 0 {
            __w = __w.color(match p.choice("color") {
                0 => martensite_theme::TokenKey::BackgroundColor,
                1 => martensite_theme::TokenKey::SurfaceColor,
                2 => martensite_theme::TokenKey::PrimaryColor,
                3 => martensite_theme::TokenKey::SecondaryColor,
                4 => martensite_theme::TokenKey::AccentColor,
                5 => martensite_theme::TokenKey::TextColor,
                6 => martensite_theme::TokenKey::TextMutedColor,
                7 => martensite_theme::TokenKey::TextInverseColor,
                8 => martensite_theme::TokenKey::BorderColor,
                9 => martensite_theme::TokenKey::DividerColor,
                10 => martensite_theme::TokenKey::RaisedColor,
                11 => martensite_theme::TokenKey::ErrorColor,
                12 => martensite_theme::TokenKey::WarningColor,
                13 => martensite_theme::TokenKey::SuccessColor,
                14 => martensite_theme::TokenKey::InfoColor,
                15 => martensite_theme::TokenKey::Spacing,
                16 => martensite_theme::TokenKey::SpacingSmall,
                17 => martensite_theme::TokenKey::SpacingLarge,
                18 => martensite_theme::TokenKey::BorderRadius,
                19 => martensite_theme::TokenKey::BorderRadiusSmall,
                20 => martensite_theme::TokenKey::BorderRadiusLarge,
                21 => martensite_theme::TokenKey::FontSizeSmall,
                22 => martensite_theme::TokenKey::FontSizeMedium,
                23 => martensite_theme::TokenKey::FontSizeLarge,
                24 => martensite_theme::TokenKey::AnimationDuration,
                25 => martensite_theme::TokenKey::AnimationEasing,
                26 => martensite_theme::TokenKey::BackdropMaterial,
                27 => martensite_theme::TokenKey::BackdropTintOpacity,
                28 => martensite_theme::TokenKey::BackdropFallbackColor,
                29 => martensite_theme::TokenKey::CsdTitleBarHeight,
                30 => martensite_theme::TokenKey::CsdButtonRadius,
                31 => martensite_theme::TokenKey::CsdShadowBlur,
                32 => martensite_theme::TokenKey::CsdShadowColor,
                33 => martensite_theme::TokenKey::VibrancyMaterial,
                34 => martensite_theme::TokenKey::ScrimColor,
                35 => martensite_theme::TokenKey::InsetColor,
                36 => martensite_theme::TokenKey::OverlayColor,
                37 => martensite_theme::TokenKey::SeriesColor1,
                38 => martensite_theme::TokenKey::SeriesColor2,
                39 => martensite_theme::TokenKey::SeriesColor3,
                40 => martensite_theme::TokenKey::SeriesColor4,
                41 => martensite_theme::TokenKey::SeriesColor5,
                42 => martensite_theme::TokenKey::SeriesColor6,
                43 => martensite_theme::TokenKey::FontSizeMicro,
                44 => martensite_theme::TokenKey::FontSizeCaption,
                45 => martensite_theme::TokenKey::FontSizeBody,
                46 => martensite_theme::TokenKey::FontSizeTitle,
                47 => martensite_theme::TokenKey::FontSizeDisplay,
                _ => martensite_theme::TokenKey::BackgroundColor,
            });
        }
        Box::new(__w)
    },
    snippet: |p| {
        let mut __s = format!("Ribbon::new({:?}).child(content)", p.str("text"));
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                (
                    "color",
                    ".color",
                    SnipProp::Choice(&[
                        "martensite_theme::TokenKey::BackgroundColor",
                        "martensite_theme::TokenKey::SurfaceColor",
                        "martensite_theme::TokenKey::PrimaryColor",
                        "martensite_theme::TokenKey::SecondaryColor",
                        "martensite_theme::TokenKey::AccentColor",
                        "martensite_theme::TokenKey::TextColor",
                        "martensite_theme::TokenKey::TextMutedColor",
                        "martensite_theme::TokenKey::TextInverseColor",
                        "martensite_theme::TokenKey::BorderColor",
                        "martensite_theme::TokenKey::DividerColor",
                        "martensite_theme::TokenKey::RaisedColor",
                        "martensite_theme::TokenKey::ErrorColor",
                        "martensite_theme::TokenKey::WarningColor",
                        "martensite_theme::TokenKey::SuccessColor",
                        "martensite_theme::TokenKey::InfoColor",
                        "martensite_theme::TokenKey::Spacing",
                        "martensite_theme::TokenKey::SpacingSmall",
                        "martensite_theme::TokenKey::SpacingLarge",
                        "martensite_theme::TokenKey::BorderRadius",
                        "martensite_theme::TokenKey::BorderRadiusSmall",
                        "martensite_theme::TokenKey::BorderRadiusLarge",
                        "martensite_theme::TokenKey::FontSizeSmall",
                        "martensite_theme::TokenKey::FontSizeMedium",
                        "martensite_theme::TokenKey::FontSizeLarge",
                        "martensite_theme::TokenKey::AnimationDuration",
                        "martensite_theme::TokenKey::AnimationEasing",
                        "martensite_theme::TokenKey::BackdropMaterial",
                        "martensite_theme::TokenKey::BackdropTintOpacity",
                        "martensite_theme::TokenKey::BackdropFallbackColor",
                        "martensite_theme::TokenKey::CsdTitleBarHeight",
                        "martensite_theme::TokenKey::CsdButtonRadius",
                        "martensite_theme::TokenKey::CsdShadowBlur",
                        "martensite_theme::TokenKey::CsdShadowColor",
                        "martensite_theme::TokenKey::VibrancyMaterial",
                        "martensite_theme::TokenKey::ScrimColor",
                        "martensite_theme::TokenKey::InsetColor",
                        "martensite_theme::TokenKey::OverlayColor",
                        "martensite_theme::TokenKey::SeriesColor1",
                        "martensite_theme::TokenKey::SeriesColor2",
                        "martensite_theme::TokenKey::SeriesColor3",
                        "martensite_theme::TokenKey::SeriesColor4",
                        "martensite_theme::TokenKey::SeriesColor5",
                        "martensite_theme::TokenKey::SeriesColor6",
                        "martensite_theme::TokenKey::FontSizeMicro",
                        "martensite_theme::TokenKey::FontSizeCaption",
                        "martensite_theme::TokenKey::FontSizeBody",
                        "martensite_theme::TokenKey::FontSizeTitle",
                        "martensite_theme::TokenKey::FontSizeDisplay",
                    ]),
                ),
                (
                    "corner",
                    ".corner",
                    SnipProp::Choice(&[
                        "martensite::widgets::ribbon::RibbonCorner::TopEnd",
                        "martensite::widgets::ribbon::RibbonCorner::TopStart",
                    ]),
                ),
                ("enabled", ".enabled", SnipProp::Bool(true)),
            ],
        ));
        __s
    },
});

/// All Navigation pages, in rail order.
pub(super) fn pages() -> Vec<Box<dyn Page>> {
    vec![
        Box::new(TabsPage),
        Box::new(NavRailPage),
        Box::new(NavStackPage),
        Box::new(BreadcrumbPage),
        Box::new(PaginationPage),
        Box::new(StepsPage),
        Box::new(MenuBarPage),
        Box::new(DockPage),
        Box::new(ToolbarPage),
        Box::new(ToolbarOverflowPage),
        Box::new(HeaderBarPage),
        Box::new(PageHeaderPage),
        Box::new(CommandPalettePage),
        Box::new(ToolPalettePage),
        Box::new(RadialMenuPage),
        Box::new(SpeedDialPage),
        Box::new(AppGridPage),
        Box::new(DevicePickerPage),
        Box::new(ThemePickerPage),
        Box::new(WindowControlsPage),
        Box::new(ControlCenterPage),
        Box::new(AnchorPage),
        Box::new(HeroHeaderPage),
        Box::new(ResizeHandlePage),
        Box::new(ScrollIndicatorPage),
        Box::new(TaskSwitcherPage),
        Box::new(RibbonPage),
    ]
}
