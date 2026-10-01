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
use crate::pages::{csv, downcast_mut, meta, page};

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
    ],
    build: |p| {
        let mut t = Tabs::new().closable(p.bool("closable"));
        for label in csv(p, "tabs") {
            t = t.tab(label.clone(), Text::new(format!("{label} panel content")));
        }
        Box::new(t)
    },
    snippet: |p| {
        let tabs = csv(p, "tabs")
            .iter()
            .map(|t| format!("{t:?}"))
            .collect::<Vec<_>>()
            .join(", ");
        format!("Tabs::new().tabs([{tabs}]) /* + panels */")
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
    props: &[PropSpec::Text {
        key: "dests",
        label: "Destinations (csv)",
        default: "Home,Search,Library,Settings",
    }],
    build: |p| {
        let icons = ["home", "magnify", "bookshelf", "cog"];
        let mut rail = NavRail::new();
        for (i, d) in csv(p, "dests").iter().enumerate() {
            rail = rail.destination(icons.get(i).copied().unwrap_or("dot"), d.clone());
        }
        rail.set_selected(Some(0));
        Box::new(rail)
    },
    snippet: |p| format!(
        "NavRail::new() /* {} destinations */",
        csv(p, "dests").len(),
    ),
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
    props: &[],
    build: |_p| {
        let mut ns = NavStack::new(Text::new("Root page"));
        ns.push(Text::new("Detail page"), "Detail");
        Box::new(ns)
    },
    snippet: |_p| "NavStack::new(root_widget).push(detail, \"Detail\")".to_string(),
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
    props: &[PropSpec::Text {
        key: "path",
        label: "Path (csv)",
        default: "Home,Documents,Project,RFC",
    }],
    build: |p| {
        let mut b = Breadcrumb::new();
        for seg in csv(p, "path") {
            b = b.push(seg);
        }
        Box::new(b)
    },
    snippet: |p| {
        let segs = csv(p, "path")
            .iter()
            .map(|s| format!("{s:?}"))
            .collect::<Vec<_>>()
            .join(", ");
        format!("Breadcrumb::new().segments([{segs}])")
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
    ],
    build: |p| {
        Box::new(
            Pagination::new()
                .total_pages(p.i64("total") as usize)
                .current((p.i64("current") as usize).min(p.i64("total") as usize - 1)),
        )
    },
    snippet: |p| format!(
        "Pagination::new().total({}).set_current({})",
        p.i64("total"),
        p.i64("current"),
    ),
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
    ],
    build: |p| {
        let mut s = Steps::new().steps(csv(p, "steps"));
        s.set_current((p.i64("current") as usize).min(csv(p, "steps").len().saturating_sub(1)));
        Box::new(s)
    },
    snippet: |p| {
        let steps = csv(p, "steps")
            .iter()
            .map(|s| format!("{s:?}"))
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "Steps::new().steps([{steps}]).set_current({})",
            p.i64("current")
        )
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
    props: &[PropSpec::Bool {
        key: "magnify",
        label: "Magnify",
        default: true
    }],
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
        Box::new(d)
    },
    snippet: |_p| "Dock::new().item(DockItem::new(\"Finder\", color))".to_string(),
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
    props: &[PropSpec::Int {
        key: "items",
        label: "Items",
        min: 2,
        max: 12,
        default: 5
    }],
    build: |p| {
        let mut tb = Toolbar::new().label("Toolbar");
        for i in 1..=p.i64("items") {
            tb = tb.item(Button::new(format!("Action {i}")));
        }
        Box::new(tb)
    },
    snippet: |p| format!("Toolbar::new() /* {} button items */", p.i64("items"),),
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
    props: &[PropSpec::Text {
        key: "title",
        label: "Title",
        default: "Settings"
    }],
    build: |p| Box::new(
        PageHeader::new(p.str("title"))
            .back(true)
            .action(Button::new("Apply")),
    ),
    snippet: |p| format!(
        "PageHeader::new({:?}).back().action(Button::new(\"Apply\"))",
        p.str("title"),
    ),
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
    props: &[],
    build: |_p| {
        let mut cp = CommandPalette::new()
            .action(CommandAction::new("open", "Open File"))
            .action(CommandAction::new("save", "Save All"))
            .action(CommandAction::new("prefs", "Preferences"));
        cp.open();
        Box::new(cp)
    },
    snippet: |_p| {
        "CommandPalette::new()\n    .action(CommandAction::new(\"open\", \"Open File\"))"
            .to_string()
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
    props: &[PropSpec::Int {
        key: "cols",
        label: "Columns",
        min: 2,
        max: 8,
        default: 4
    }],
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
        Box::new(tp)
    },
    snippet: |p| format!("ToolPalette::new().columns({})", p.i64("cols")),
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
    props: &[PropSpec::Text {
        key: "items",
        label: "Items (csv)",
        default: "Cut,Copy,Paste,Undo,Redo",
    }],
    build: |p| Box::new(RadialMenu::new().items(csv(p, "items"))),
    snippet: |p| {
        let items = csv(p, "items")
            .iter()
            .map(|i| format!("{i:?}"))
            .collect::<Vec<_>>()
            .join(", ");
        format!("RadialMenu::new().items([{items}])")
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
    props: &[],
    build: |_p| Box::new(
        SpeedDial::new()
            .action("Compose")
            .action("Share")
            .action("Archive"),
    ),
    snippet: |_p| "SpeedDial::new().action(\"Compose\").action(\"Share\")".to_string(),
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
    props: &[PropSpec::Int {
        key: "cols",
        label: "Columns",
        min: 2,
        max: 8,
        default: 4
    }],
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
        Box::new(g)
    },
    snippet: |p| format!("AppGrid::new().columns({})", p.i64("cols")),
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
    props: &[],
    build: |_p| Box::new(
        DevicePicker::new()
            .section(DeviceKind::Microphone, ["Internal Mic", "USB Mic"])
            .section(DeviceKind::Speaker, ["Speakers", "Headphones"]),
    ),
    snippet: |_p| {
        "DevicePicker::new()\n    .section(DeviceKind::Microphone, [\"Internal Mic\"])".to_string()
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
    props: &[],
    build: |_p| Box::new(
        ThemePicker::new()
            .option(ThemeOption::new(
                "Light",
                [250, 250, 250, 255],
                [20, 20, 20, 255],
                [80, 140, 255, 255]
            ))
            .option(ThemeOption::new(
                "Dark",
                [24, 26, 32, 255],
                [230, 230, 235, 255],
                [120, 160, 255, 255]
            )),
    ),
    snippet: |_p| "ThemePicker::new().option(ThemeOption::new(\"Dark\", …))".to_string(),
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
    props: &[PropSpec::Bool {
        key: "mac",
        label: "mac style",
        default: false
    }],
    build: |p| {
        if p.bool("mac") {
            Box::new(WindowControls::mac())
        } else {
            Box::new(WindowControls::new())
        }
    },
    snippet: |p| format!("WindowControls::new() /* mac={} */", p.bool("mac")),
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
    props: &[],
    build: |_p| Box::new(
        ControlCenter::new()
            .tile("wifi", "Wi-Fi", true)
            .tile("bluetooth", "Bluetooth", false)
            .slider("sun", "Brightness", 0.7),
    ),
    snippet: |_p| {
        "ControlCenter::new()\n    .tile(\"wifi\", \"Wi-Fi\", true)\n    .slider(\"sun\", \"Brightness\", 0.7)".to_string()
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
    props: &[PropSpec::Text {
        key: "items",
        label: "Items (csv)",
        default: "Intro,Usage,API,Changelog",
    }],
    build: |p| {
        let items: Vec<AnchorItem> = csv(p, "items")
            .iter()
            .map(|i| AnchorItem::new(i.clone(), format!("#{i}")))
            .collect();
        Box::new(Anchor::new().items(items))
    },
    snippet: |p| format!("Anchor::new() /* {} items */", csv(p, "items").len()),
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
    props: &[PropSpec::Text {
        key: "title",
        label: "Title",
        default: "Ship faster"
    }],
    build: |p| Box::new(
        HeroHeader::new(p.str("title"))
            .eyebrow("Martensite")
            .subtitle("Retained-mode widgets with a real pipeline.")
            .primary("Get started")
            .secondary("Docs"),
    ),
    snippet: |p| format!(
        "HeroHeader::new({:?}).primary(\"Get started\")",
        p.str("title")
    ),
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
    props: &[PropSpec::Choice {
        key: "orientation",
        label: "Orientation",
        options: &["Horizontal", "Vertical"],
        default: 0,
    }],
    build: |p| {
        let o = if p.choice("orientation") == 1 {
            SplitOrientation::Vertical
        } else {
            SplitOrientation::Horizontal
        };
        Box::new(ResizeHandle::new(o))
    },
    snippet: |p| format!(
        "ResizeHandle::new(SplitOrientation::{})",
        if p.choice("orientation") == 1 {
            "Vertical"
        } else {
            "Horizontal"
        },
    ),
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
    props: &[PropSpec::Float {
        key: "fraction",
        label: "Scroll",
        min: 0.0,
        max: 1.0,
        step: 0.05,
        default: 0.3,
    }],
    build: |p| {
        let mut si = ScrollIndicator::new(SplitOrientation::Horizontal);
        si.set_scroll(p.f64("fraction") as f32, 0.25);
        Box::new(si)
    },
    snippet: |p| format!(
        "ScrollIndicator::new(Horizontal).set_scroll({:?}, 0.25)",
        p.f64("fraction") as f32,
    ),
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
    props: &[],
    build: |_p| {
        let mut ts = TaskSwitcher::new();
        for (i, name) in ["Editor", "Terminal", "Browser"].iter().enumerate() {
            ts = ts.item(Thumbnail::new(
                name.to_string(),
                [90, 120 + i as u8 * 40, 200, 255],
            ));
        }
        Box::new(ts)
    },
    snippet: |_p| "TaskSwitcher::new().item(Thumbnail::new(\"Editor\", …))".to_string(),
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
    props: &[PropSpec::Text {
        key: "text",
        label: "Text",
        default: "Beta"
    }],
    build: |p| Box::new(
        Ribbon::new(p.str("text")).child(
            martensite::widgets::container::Container::new()
                .padding_uniform(24.0)
                .child(Text::new("Card with ribbon"))
        ),
    ),
    snippet: |p| format!("Ribbon::new({:?}).child(content)", p.str("text")),
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
