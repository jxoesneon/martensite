//! Data family — lists, tables, trees, and structured viewers.

use martensite::widgets::check_list::CheckList;
use martensite::widgets::clipboard_history::ClipboardHistory;
use martensite::widgets::inspector::Inspector;
use martensite::widgets::json_view::{JsonNode, JsonView};
use martensite::widgets::kanban::Kanban;
use martensite::widgets::list_view::ListView;
use martensite::widgets::log_view::{LogSeverity, LogView};
use martensite::widgets::property_grid::{PropertyGrid, PropertyRow};
use martensite::widgets::table::{Table, TableColumn};
use martensite::widgets::transfer::Transfer;
use martensite::widgets::tree_select::TreeSelect;
use martensite::widgets::tree_view::{TreeNode, TreeView};

use crate::page::{Page, PropSpec};
use crate::pages::{csv, downcast_mut, meta, page, SnipProp};

page!(ListViewPage {
    meta: meta(
        "ListView",
        "Data",
        "Virtualized single-column list with selection modes.",
        "List",
        &[
            ("Qt", "QListView"),
            ("GTK", "GtkListView"),
            ("SwiftUI", "List"),
            ("React", "<ul>")
        ],
        false,
    ),
    props: &[
        PropSpec::Int {
            key: "count",
            label: "Items",
            min: 1,
            max: 500,
            default: 40
        },
        PropSpec::Bool {
            key: "alternating",
            label: "Alternating rows",
            default: true
        },
        PropSpec::Float {
            key: "row_height",
            label: "Row Height",
            min: 0.0,
            max: 64.0,
            step: 0.5,
            default: 0.0
        },
        PropSpec::Int {
            key: "pending_tail",
            label: "Pending Tail",
            min: 0,
            max: 100,
            default: 0
        },
        PropSpec::Choice {
            key: "selection_mode",
            label: "Selection Mode",
            options: &["Single", "Multiple"],
            default: 0
        },
        PropSpec::Text {
            key: "items",
            label: "Items",
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
    ],
    build: |p| {
        let mut lv = ListView::new()
            .label("Items")
            .alternating_rows(p.bool("alternating"));
        lv.set_items((1..=p.i64("count")).map(|i| format!("Item {i}")));
        {
            let mut __w = lv;
            __w = __w.enabled(p.bool("enabled"));
            __w = __w.loading(p.bool("loading"));
            if p.f64("row_height") != 0.0 {
                __w = __w.row_height(p.f64("row_height") as f32);
            }
            if p.i64("pending_tail") != 0 {
                __w = __w.pending_tail(p.i64("pending_tail") as usize);
            }
            if p.choice("selection_mode") != 0 {
                __w = __w.selection_mode(match p.choice("selection_mode") {
                    0 => martensite::widgets::list_view::SelectionMode::Single,
                    1 => martensite::widgets::list_view::SelectionMode::Multiple,
                    _ => martensite::widgets::list_view::SelectionMode::Single,
                });
            }
            let __v = crate::pages::csv(p, "items");
            if !__v.is_empty() {
                __w = __w.items(__v);
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = format!(
        "let mut lv = ListView::new();\nlv.set_items((1..={}).map(|i| format!(\"Item {{i}}\")));",
        p.i64("count"),
    );
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("row_height", ".row_height", SnipProp::Float(0.0)),
                ("pending_tail", ".pending_tail", SnipProp::Int(0)),
                (
                    "selection_mode",
                    ".selection_mode",
                    SnipProp::Choice(&[
                        "martensite::widgets::list_view::SelectionMode::Single",
                        "martensite::widgets::list_view::SelectionMode::Multiple",
                    ]),
                ),
                ("enabled", ".enabled", SnipProp::Bool(true)),
                ("loading", ".loading", SnipProp::Bool(false)),
            ],
        ));
        __s.push_str(&crate::pages::snip_textmap(
            p,
            "items",
            ".items",
            "",
            crate::pages::expr_strs,
        ));
        __s
    },
    poll: |w, out| {
        if let Some(lv) = downcast_mut::<ListView>(w) {
            if let Some(i) = lv.take_activated() {
                out.push(format!("activated row {i}"));
            }
        }
    },
    state: |w| {
        downcast_mut::<ListView>(w)
            .map(|lv| {
                vec![
                    ("selected".to_string(), format!("{:?}", lv.selected())),
                    ("items".to_string(), lv.item_count().to_string()),
                ]
            })
            .unwrap_or_default()
    },
});

page!(TablePage {
    meta: meta(
        "Table",
        "Data",
        "Sortable multi-column data table.",
        "Table",
        &[
            ("Qt", "QTableView"),
            ("GTK", "GtkColumnView"),
            ("SwiftUI", "Table"),
            ("HTML", "<table>")
        ],
        false,
    ),
    props: &[
        PropSpec::Int {
            key: "rows",
            label: "Rows",
            min: 2,
            max: 200,
            default: 24
        },
        PropSpec::Bool {
            key: "striped",
            label: "Striped",
            default: true
        },
        PropSpec::Bool {
            key: "grid",
            label: "Grid lines",
            default: true
        },
        PropSpec::Bool {
            key: "sortable",
            label: "Sortable",
            default: true
        },
        PropSpec::Float {
            key: "row_height",
            label: "Row Height",
            min: 0.0,
            max: 64.0,
            step: 0.5,
            default: 0.0
        },
        PropSpec::Float {
            key: "header_height",
            label: "Header Height",
            min: 0.0,
            max: 64.0,
            step: 0.5,
            default: 0.0
        },
        PropSpec::Int {
            key: "pending_tail",
            label: "Pending Tail",
            min: 0,
            max: 100,
            default: 0
        },
        PropSpec::Text {
            key: "row",
            label: "Row",
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
        let mut t = Table::new()
            .columns([
                TableColumn::new("id", "ID"),
                TableColumn::new("name", "Name"),
                TableColumn::new("status", "Status"),
            ])
            .striped(p.bool("striped"))
            .grid_lines(p.bool("grid"));
        t.set_rows((1..=p.i64("rows")).map(|i| {
            vec![
                format!("{i}"),
                format!("Entry {i}"),
                if i % 3 == 0 {
                    "OK".into()
                } else {
                    "Pending".into()
                },
            ]
        }));
        {
            let mut __w = t;
            __w = __w.enabled(p.bool("enabled"));
            __w = __w.loading(p.bool("loading"));
            if !p.str("a11y_label").is_empty() {
                __w = __w.label(p.str("a11y_label"));
            }
            if !p.bool("sortable") {
                __w = __w.sortable(p.bool("sortable"));
            }
            if p.f64("row_height") != 0.0 {
                __w = __w.row_height(p.f64("row_height") as f32);
            }
            if p.f64("header_height") != 0.0 {
                __w = __w.header_height(p.f64("header_height") as f32);
            }
            if p.i64("pending_tail") != 0 {
                __w = __w.pending_tail(p.i64("pending_tail") as usize);
            }
            let __v = crate::pages::csv(p, "row");
            if !__v.is_empty() {
                __w = __w.row(__v);
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = format!(
        "Table::new()\n    .columns([…])\n    .striped({})\n    .grid_lines({})\n    /* {} rows */",
        p.bool("striped"),
        p.bool("grid"),
        p.i64("rows"),
    );
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("sortable", ".sortable", SnipProp::Bool(true)),
                ("row_height", ".row_height", SnipProp::Float(0.0)),
                ("header_height", ".header_height", SnipProp::Float(0.0)),
                ("pending_tail", ".pending_tail", SnipProp::Int(0)),
                ("enabled", ".enabled", SnipProp::Bool(true)),
                ("loading", ".loading", SnipProp::Bool(false)),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        __s.push_str(&crate::pages::snip_textmap(
            p,
            "row",
            ".row",
            "",
            crate::pages::expr_strs,
        ));
        __s
    },
    poll: |w, out| {
        if let Some(t) = downcast_mut::<Table>(w) {
            if let Some(r) = t.take_activated() {
                out.push(format!("activated row {r}"));
            }
            if let Some((col, dir)) = t.take_sort() {
                out.push(format!("sort col {col} {dir:?}"));
            }
        }
    },
});

page!(CheckListPage {
    meta: meta(
        "CheckList",
        "Data",
        "List of checkable items — multi-select tasks.",
        "List",
        &[
            ("Qt", "checkable list"),
            ("GTK", "check rows"),
            ("React", "checkbox list"),
            ("Android", "checked list")
        ],
        false,
    ),
    props: &[
        PropSpec::Text {
            key: "items",
            label: "Items (csv)",
            default: "Write spec,Review code,Run tests,Release",
        },
        PropSpec::Text {
            key: "item",
            label: "Item",
            default: ""
        },
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
    build: |p| {
        let mut __w = CheckList::new().items(csv(p, "items"));
        __w = __w.loading(p.bool("loading"));
        if !p.str("a11y_label").is_empty() {
            __w = __w.label(p.str("a11y_label"));
        }
        if !p.str("item").is_empty() {
            __w = __w.item(martensite::widgets::check_list::CheckItem::new(
                p.str("item"),
            ));
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
            format!("CheckList::new().items([{items}])")
        };
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("loading", ".loading", SnipProp::Bool(false)),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        if !p.str("item").is_empty() {
            __s.push_str(&format!("\n    .item(CheckItem::new({:?}))", p.str("item")));
        }
        __s
    },
    poll: |w, out| {
        if let Some(c) = downcast_mut::<CheckList>(w) {
            if let Some((idx, on)) = c.take_changed() {
                out.push(format!("item {idx} → {}", if on { "on" } else { "off" }));
            }
        }
    },
});

page!(TreeViewPage {
    meta: meta(
        "TreeView",
        "Data",
        "Expandable hierarchical tree with selection.",
        "Tree",
        &[
            ("Qt", "QTreeView"),
            ("GTK", "GtkTreeView"),
            ("SwiftUI", "OutlineGroup"),
            ("React", "tree view")
        ],
        false,
    ),
    props: &[
        PropSpec::Bool {
            key: "indent",
            label: "Wide indent",
            default: false
        },
        PropSpec::Float {
            key: "row_height",
            label: "Row Height",
            min: 0.0,
            max: 64.0,
            step: 0.5,
            default: 0.0
        },
        PropSpec::Text {
            key: "roots",
            label: "Roots",
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
    ],
    build: |p| {
        let mut tv = TreeView::new().label("Files");
        if p.bool("indent") {
            tv = tv.indent(24.0);
        }
        tv.set_roots(vec![
            TreeNode::new("src").with_children(vec![
                TreeNode::new("main.rs"),
                TreeNode::new("lib.rs"),
                TreeNode::new("widgets")
                    .with_children(vec![TreeNode::new("button.rs"), TreeNode::new("slider.rs")]),
            ]),
            TreeNode::new("Cargo.toml"),
        ]);
        {
            let mut __w = tv;
            __w = __w.enabled(p.bool("enabled"));
            __w = __w.loading(p.bool("loading"));
            if p.f64("row_height") != 0.0 {
                __w = __w.row_height(p.f64("row_height") as f32);
            }
            let __v = crate::pages::csv(p, "roots");
            if !__v.is_empty() {
                __w = __w.roots(
                    __v.into_iter()
                        .map(martensite::widgets::tree_view::TreeNode::new)
                        .collect::<Vec<_>>(),
                );
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = format!(
            "TreeView::new()\n    .indent({:?})\n    /* nested TreeNode roots */",
            if p.bool("indent") { 24.0 } else { 16.0 },
        );
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("row_height", ".row_height", SnipProp::Float(0.0)),
                ("enabled", ".enabled", SnipProp::Bool(true)),
                ("loading", ".loading", SnipProp::Bool(false)),
            ],
        ));
        __s.push_str(&crate::pages::snip_textmap(
            p,
            "roots",
            ".roots",
            "",
            crate::pages::expr_strs,
        ));
        __s
    },
    poll: |w, out| {
        if let Some(tv) = downcast_mut::<TreeView>(w) {
            if let Some(path) = tv.take_activated() {
                out.push(format!("activated → {path:?}"));
            }
        }
    },
});

page!(TreeSelectPage {
    meta: meta(
        "TreeSelect",
        "Data",
        "Dropdown-style hierarchical picker.",
        "ComboBox",
        &[
            ("Ant", "TreeSelect"),
            ("Qt", "tree combo"),
            ("React", "tree select"),
            ("GTK", "custom")
        ],
        true,
    ),
    props: &[
        PropSpec::Text {
            key: "label",
            label: "Label",
            default: "Location"
        },
        PropSpec::Text {
            key: "placeholder",
            label: "Placeholder",
            default: ""
        },
        PropSpec::Text {
            key: "tree",
            label: "Tree",
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
    ],
    build: |p| {
        let mut ts = TreeSelect::new().label(p.str("label"));
        ts.open();
        {
            let mut __w = ts;
            __w = __w.enabled(p.bool("enabled"));
            __w = __w.loading(p.bool("loading"));
            if !p.str("placeholder").is_empty() {
                __w = __w.placeholder(p.str("placeholder"));
            }
            let __v = crate::pages::csv(p, "tree");
            if !__v.is_empty() {
                __w = __w.tree(
                    __v.into_iter()
                        .map(martensite::widgets::tree_view::TreeNode::new)
                        .collect::<Vec<_>>(),
                );
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = format!("TreeSelect::new().label({:?})", p.str("label"));
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("placeholder", ".placeholder", SnipProp::Text("")),
                ("enabled", ".enabled", SnipProp::Bool(true)),
                ("loading", ".loading", SnipProp::Bool(false)),
            ],
        ));
        __s.push_str(&crate::pages::snip_textmap(
            p,
            "tree",
            ".tree",
            "",
            crate::pages::expr_strs,
        ));
        __s
    },
    poll: |w, out| {
        if let Some(ts) = downcast_mut::<TreeSelect>(w) {
            if let Some(path) = ts.take_selected() {
                out.push(format!("selected → {path:?}"));
            }
        }
    },
});

page!(JsonViewPage {
    meta: meta(
        "JsonView",
        "Data",
        "Expandable JSON tree viewer.",
        "Tree",
        &[
            ("DevTools", "JSON viewer"),
            ("Qt", "QJsonView"),
            ("React", "react-json-view"),
            ("GTK", "custom")
        ],
        false,
    ),
    props: &[],
    build: |_p| Box::new(JsonView::new(JsonNode::object(
        "root",
        [
            ("name".to_string(), JsonNode::string("", "martensite")),
            ("version".to_string(), JsonNode::number("", 16.0)),
            ("stable".to_string(), JsonNode::boolean("", true)),
            (
                "deps".to_string(),
                JsonNode::array(
                    "",
                    [
                        JsonNode::string("", "taffy"),
                        JsonNode::string("", "accesskit")
                    ],
                ),
            ),
        ],
    ))),
    snippet: |_p| "JsonView::new(JsonNode::object(\"root\", vec![…]))".to_string(),
    poll: |w, out| {
        if let Some(jv) = downcast_mut::<JsonView>(w) {
            if let Some(path) = jv.take_toggled() {
                out.push(format!("toggled {path:?}"));
            }
        }
    },
});

page!(InspectorPage {
    meta: meta(
        "Inspector",
        "Data",
        "Property inspector — sections of label/value rows.",
        "Table",
        &[
            ("Xcode", "inspector"),
            ("Blender", "properties"),
            ("Qt", "QtPropertyBrowser"),
            ("GTK", "property list")
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
        let mut __w = Inspector::new()
            .section("Transform")
            .row("X", "12.0")
            .row("Y", "48.0")
            .row("Width", "240")
            .section("Style")
            .row("Opacity", "0.9")
            .row("Radius", "8");
        if !_p.str("a11y_label").is_empty() {
            __w = __w.label(_p.str("a11y_label"));
        }
        Box::new(__w)
    },
    snippet: |_p| {
        let mut __s = {
            "Inspector::new()\n    .section(\"Transform\")\n    .row(\"X\", \"12.0\")".to_string()
        };
        __s.push_str(&crate::pages::prop_snippet(
            _p,
            &[("a11y_label", ".label", SnipProp::Text(""))],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(ins) = downcast_mut::<Inspector>(w) {
            if let Some(sel) = ins.take_selected() {
                out.push(format!("selected {sel:?}"));
            }
        }
    },
});

page!(KanbanPage {
    meta: meta(
        "Kanban",
        "Data",
        "Column board with draggable cards.",
        "List",
        &[
            ("Trello", "board"),
            ("Qt", "custom"),
            ("React", "kanban"),
            ("Jira", "board")
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
        {
            let mut __w = Kanban::new()
                .column("Backlog")
                .column("Doing")
                .column("Done")
                .card("Backlog", "Write spec")
                .card("Doing", "Implement RFC")
                .card("Done", "Ship it");
            __w = __w.loading(_p.bool("loading"));
            if !_p.str("a11y_label").is_empty() {
                __w = __w.label(_p.str("a11y_label"));
            }
            Box::new(__w)
        }
    },
    snippet: |_p| {
        let mut __s = {
            "Kanban::new()\n    .column(\"Backlog\")\n    .column(\"Doing\")\n    .column(\"Done\")"
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
        if let Some(k) = downcast_mut::<Kanban>(w) {
            if let Some(m) = k.take_moved() {
                out.push(format!("moved {m:?}"));
            }
            if let Some(s) = k.take_selected() {
                out.push(format!("selected {s:?}"));
            }
        }
    },
});

page!(TransferPage {
    meta: meta(
        "Transfer",
        "Data",
        "Dual-list shuttle — move items between panes.",
        "List",
        &[
            ("Ant", "Transfer"),
            ("Qt", "dual list"),
            ("GTK", "custom"),
            ("React", "transfer")
        ],
        false,
    ),
    props: &[
        PropSpec::Text {
            key: "source",
            label: "Source title",
            default: "Available"
        },
        PropSpec::Text {
            key: "target",
            label: "Target title",
            default: "Selected"
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
        let mut __w = Transfer::new()
            .titles(p.str("source"), p.str("target"))
            .source(["Alpha", "Beta", "Gamma", "Delta"])
            .target(["Omega"]);
        __w = __w.enabled(p.bool("enabled"));
        if !p.str("a11y_label").is_empty() {
            __w = __w.label(p.str("a11y_label"));
        }
        Box::new(__w)
    },
    snippet: |p| {
        let mut __s = format!(
            "Transfer::new()\n    .titles({:?}, {:?})\n    .source([…]).target([…])",
            p.str("source"),
            p.str("target"),
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
        if let Some(t) = downcast_mut::<Transfer>(w) {
            if let Some((items, dir)) = t.take_moved() {
                out.push(format!("moved {dir:?} — {} item(s)", items.len()));
            }
        }
    },
});

page!(LogViewPage {
    meta: meta(
        "LogView",
        "Data",
        "Severity-colored scrolling log with follow mode.",
        "Text",
        &[
            ("Qt", "log view"),
            ("IDE", "console"),
            ("React", "log stream"),
            ("GTK", "custom")
        ],
        false,
    ),
    props: &[
        PropSpec::Int {
            key: "lines",
            label: "Lines",
            min: 4,
            max: 200,
            default: 40
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
        let mut lv = LogView::new();
        for i in 1..=p.i64("lines") {
            let sev = match i % 4 {
                0 => LogSeverity::Warning,
                1 => LogSeverity::Info,
                2 => LogSeverity::Debug,
                _ => LogSeverity::Error,
            };
            lv.push(sev, format!("log line {i}"));
        }
        {
            let mut __w = lv;
            __w = __w.enabled(p.bool("enabled"));
            if !p.str("a11y_label").is_empty() {
                __w = __w.a11y_label(p.str("a11y_label"));
            }
            if p.i64("max_lines") != 0 {
                __w = __w.max_lines(p.i64("max_lines") as usize);
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = format!(
            "let mut lv = LogView::new();\nlv.push(LogSeverity::Info, \"…\"); /* {} lines */",
            p.i64("lines"),
        );
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("max_lines", ".max_lines", SnipProp::Int(0)),
                ("enabled", ".enabled", SnipProp::Bool(true)),
                ("a11y_label", ".a11y_label", SnipProp::Text("")),
            ],
        ));
        __s
    },
});

page!(PropertyGridPage {
    meta: meta(
        "PropertyGrid",
        "Data",
        "IDE-style name/value editor grid with sections.",
        "Table",
        &[
            ("Win32", "PropertyGrid"),
            ("Qt", "QtPropertyBrowser"),
            ("Blender", "properties"),
            ("IDE", "inspector")
        ],
        false,
    ),
    props: &[
        PropSpec::Float {
            key: "fraction",
            label: "Name fraction",
            min: 0.2,
            max: 0.8,
            step: 0.05,
            default: 0.4,
        },
        PropSpec::Text {
            key: "row",
            label: "Row",
            default: ""
        },
        PropSpec::Text {
            key: "section_title",
            label: "Section Title",
            default: ""
        },
        PropSpec::Text {
            key: "section_rows",
            label: "Section Rows",
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
        let mut pg = PropertyGrid::new().columns(p.f64("fraction") as f32);
        pg.add_row(PropertyRow::text("Name", "alpha-7"));
        pg.add_row(PropertyRow::text("Size", "240 × 160"));
        pg.add_row(PropertyRow::bool("Visible", true));
        {
            let mut __w = pg;
            __w = __w.enabled(p.bool("enabled"));
            __w = __w.loading(p.bool("loading"));
            if !p.str("a11y_label").is_empty() {
                __w = __w.label(p.str("a11y_label"));
            }
            if !p.str("row").is_empty() {
                __w = __w.row(martensite::widgets::property_grid::PropertyRow::new(
                    p.str("row"),
                ));
            }
            if !p.str("section_title").is_empty() || !p.str("section_rows").is_empty() {
                __w = __w.section(
                    p.str("section_title"),
                    crate::pages::csv(p, "section_rows")
                        .into_iter()
                        .map(martensite::widgets::property_grid::PropertyRow::new),
                );
            }
            Box::new(__w)
        }
    },
    snippet: |p| {
        let mut __s = {
            format!(
        "PropertyGrid::new()\n    .columns({:?})\n    .add_row(PropertyRow::text(\"Name\", \"alpha-7\"))",
        p.f64("fraction") as f32,
    )
        };
        __s.push_str(&crate::pages::prop_snippet(
            p,
            &[
                ("enabled", ".enabled", SnipProp::Bool(true)),
                ("loading", ".loading", SnipProp::Bool(false)),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        if !p.str("row").is_empty() {
            __s.push_str(&format!("\n    .row(PropertyRow::new({:?}))", p.str("row")));
        }
        if !p.str("section_title").is_empty() || !p.str("section_rows").is_empty() {
            __s.push_str(&format!(
                "\n    .section({:?}, {})",
                p.str("section_title"),
                crate::pages::expr_strs(p.str("section_rows")).unwrap_or_default()
            ));
        }
        __s
    },
    poll: |w, out| {
        if let Some(pg) = downcast_mut::<PropertyGrid>(w) {
            if let Some((row, val)) = pg.take_changed() {
                out.push(format!("row {row} → {val}"));
            }
        }
    },
});

page!(ClipboardHistoryPage {
    meta: meta(
        "ClipboardHistory",
        "Data",
        "Clipboard ring buffer — pinned entries survive clears.",
        "List",
        &[
            ("KDE", "Klipper"),
            ("macOS", "paste mgr"),
            ("Windows", "Win+V"),
            ("React", "clipboard list")
        ],
        false,
    ),
    props: &[
        PropSpec::Int {
            key: "max",
            label: "Max",
            min: 0,
            max: 100,
            default: 50
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
        let mut ch = ClipboardHistory::new();
        ch.push("cargo test --workspace");
        ch.push("https://example.com/spec");
        ch.push("fn main() { … }");
        {
            let mut __w = ch;
            if !_p.str("a11y_label").is_empty() {
                __w = __w.label(_p.str("a11y_label"));
            }
            if _p.i64("max") != 50 {
                __w = __w.max(_p.i64("max") as usize);
            }
            Box::new(__w)
        }
    },
    snippet: |_p| {
        let mut __s = {
            "let mut ch = ClipboardHistory::new();\nch.push(\"cargo test --workspace\");"
                .to_string()
        };
        __s.push_str(&crate::pages::prop_snippet(
            _p,
            &[
                ("max", ".max", SnipProp::Int(50)),
                ("a11y_label", ".label", SnipProp::Text("")),
            ],
        ));
        __s
    },
    poll: |w, out| {
        if let Some(ch) = downcast_mut::<ClipboardHistory>(w) {
            if let Some(text) = ch.take_pasted() {
                out.push(format!("pasted → \"{text}\""));
            }
        }
    },
});

/// All Data pages, in rail order.
pub(super) fn pages() -> Vec<Box<dyn Page>> {
    vec![
        Box::new(ListViewPage),
        Box::new(TablePage),
        Box::new(CheckListPage),
        Box::new(TreeViewPage),
        Box::new(TreeSelectPage),
        Box::new(JsonViewPage),
        Box::new(InspectorPage),
        Box::new(KanbanPage),
        Box::new(TransferPage),
        Box::new(LogViewPage),
        Box::new(PropertyGridPage),
        Box::new(ClipboardHistoryPage),
    ]
}
