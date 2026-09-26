//! Concrete widget catalog entries across Controls, Containers, Data, Overlays, and Navigation.

use accesskit::Role;
use martensite::widgets::{
    Button, CheckBox, Container, Flex, ListView, NavRail, Popconfirm, Popover, ScrollView, Slider,
    Stack, Switch, Table, TableColumn, Tabs, Text, TextInput, Tooltip,
};

use crate::model::{WidgetEntry, WidgetFamily, WidgetStateKind};

/// Returns all catalog entries showcasing Martensite's widget ecosystem.
pub fn catalog_entries() -> Vec<WidgetEntry> {
    vec![
        // ===================================================================
        // CONTROLS
        // ===================================================================
        WidgetEntry {
            name: "Button",
            family: WidgetFamily::Controls,
            description: "Interactive button triggering click actions with accessible role, focus management, and primary variant styling.",
            accesskit_role: Role::Button,
            aliases: &[
                ("Qt", "QPushButton"),
                ("GTK", "GtkButton"),
                ("SwiftUI", "Button"),
                ("React", "<button>"),
            ],
            code_snippet: "use martensite::widgets::Button;\n\nlet btn = Button::new(\"Click Me\")\n    .primary(true);",
            instantiate: |state| match state {
                WidgetStateKind::Default => Box::new(Button::new("Normal Button")),
                WidgetStateKind::Hover => Box::new(Button::new("Hover Active").primary(true)),
                WidgetStateKind::Disabled => Box::new(Button::new("Disabled").enabled(false)),
                WidgetStateKind::Focused => Box::new(Button::new("Focused Action")),
            },
        },
        WidgetEntry {
            name: "Slider",
            family: WidgetFamily::Controls,
            description: "APG compliant range slider supporting keyboard stepping, page jumps, drag tracking, and value formatters.",
            accesskit_role: Role::Slider,
            aliases: &[
                ("Qt", "QSlider"),
                ("GTK", "GtkScale"),
                ("SwiftUI", "Slider"),
                ("React", "<input type=\"range\">"),
            ],
            code_snippet: "use martensite::widgets::Slider;\n\nlet slider = Slider::new(0.0, 100.0)\n    .with_value(50.0)\n    .step(5.0);",
            instantiate: |state| match state {
                WidgetStateKind::Default => Box::new(Slider::new(0.0, 100.0).with_value(25.0)),
                WidgetStateKind::Hover => Box::new(Slider::new(0.0, 100.0).with_value(65.0)),
                WidgetStateKind::Disabled => Box::new(Slider::new(0.0, 100.0).with_value(40.0).enabled(false)),
                WidgetStateKind::Focused => Box::new(Slider::new(0.0, 100.0).with_value(50.0)),
            },
        },
        WidgetEntry {
            name: "Toggle",
            family: WidgetFamily::Controls,
            description: "Pill-shaped toggle switch for binary preferences with accessible Toggled state and keyboard activation.",
            accesskit_role: Role::Switch,
            aliases: &[
                ("Qt", "QCheckBox (Switch style)"),
                ("GTK", "GtkSwitch"),
                ("SwiftUI", "Toggle"),
                ("React", "<input type=\"checkbox\" role=\"switch\">"),
            ],
            code_snippet: "use martensite::widgets::Switch;\n\nlet toggle = Switch::new(\"Live updates\")\n    .on(true);",
            instantiate: |state| match state {
                WidgetStateKind::Default => Box::new(Switch::new("Notifications").on(false)),
                WidgetStateKind::Hover => Box::new(Switch::new("Notifications").on(true)),
                WidgetStateKind::Disabled => Box::new(Switch::new("Notifications").on(false).enabled(false)),
                WidgetStateKind::Focused => Box::new(Switch::new("Notifications").on(true)),
            },
        },
        WidgetEntry {
            name: "TextInput",
            family: WidgetFamily::Controls,
            description: "Single-line text entry field with full IME composition pre-edit, undo/redo history, and validation states.",
            accesskit_role: Role::TextInput,
            aliases: &[
                ("Qt", "QLineEdit"),
                ("GTK", "GtkEntry"),
                ("SwiftUI", "TextField"),
                ("React", "<input type=\"text\">"),
            ],
            code_snippet: "use martensite::widgets::TextInput;\n\nlet input = TextInput::new(\"Search\")\n    .placeholder(\"Search entries...\")\n    .clearable(true);",
            instantiate: |state| match state {
                WidgetStateKind::Default => Box::new(TextInput::new("Field").placeholder("Default input...")),
                WidgetStateKind::Hover => Box::new(TextInput::new("Hover").value("Editing query")),
                WidgetStateKind::Disabled => Box::new(TextInput::new("Disabled").value("Locked input").enabled(false)),
                WidgetStateKind::Focused => Box::new(TextInput::new("Focused").value("Active focus")),
            },
        },
        WidgetEntry {
            name: "Checkbox",
            family: WidgetFamily::Controls,
            description: "Accessible toggleable checkbox supporting binary and tri-state (indeterminate) selection.",
            accesskit_role: Role::CheckBox,
            aliases: &[
                ("Qt", "QCheckBox"),
                ("GTK", "GtkCheckButton"),
                ("SwiftUI", "Toggle(style: .checkbox)"),
                ("React", "<input type=\"checkbox\">"),
            ],
            code_snippet: "use martensite::widgets::CheckBox;\n\nlet cb = CheckBox::new(\"Auto-reorder\")\n    .checked(true);",
            instantiate: |state| match state {
                WidgetStateKind::Default => Box::new(CheckBox::new("Unchecked").checked(false)),
                WidgetStateKind::Hover => Box::new(CheckBox::new("Checked").checked(true)),
                WidgetStateKind::Disabled => Box::new(CheckBox::new("Disabled").checked(false).enabled(false)),
                WidgetStateKind::Focused => Box::new(CheckBox::new("Focused").checked(true)),
            },
        },

        // ===================================================================
        // CONTAINERS
        // ===================================================================
        WidgetEntry {
            name: "Flex",
            family: WidgetFamily::Containers,
            description: "Taffy-powered flexbox container arranging children in rows or columns with configurable gaps and cross/main alignments.",
            accesskit_role: Role::GenericContainer,
            aliases: &[
                ("Qt", "QHBoxLayout / QVBoxLayout"),
                ("GTK", "GtkBox"),
                ("SwiftUI", "HStack / VStack"),
                ("React", "<div style={{ display: 'flex' }}>"),
            ],
            code_snippet: "use martensite::widgets::{Button, Flex};\n\nlet row = Flex::row()\n    .gap(12.0)\n    .child(Button::new(\"Save\"))\n    .child(Button::new(\"Cancel\"));",
            instantiate: |state| match state {
                WidgetStateKind::Default => Box::new(
                    Flex::row()
                        .gap(8.0)
                        .child(Button::new("Item Alpha"))
                        .child(Button::new("Item Beta")),
                ),
                WidgetStateKind::Hover => Box::new(
                    Flex::column()
                        .gap(6.0)
                        .child(Text::new("Row One"))
                        .child(Text::new("Row Two")),
                ),
                WidgetStateKind::Disabled => Box::new(
                    Flex::row()
                        .gap(8.0)
                        .child(Button::new("Inert").enabled(false))
                        .child(Button::new("Disabled").enabled(false)),
                ),
                WidgetStateKind::Focused => Box::new(
                    Flex::row()
                        .gap(8.0)
                        .child(Button::new("Primary Action").primary(true))
                        .child(Button::new("Secondary")),
                ),
            },
        },
        WidgetEntry {
            name: "Stack",
            family: WidgetFamily::Containers,
            description: "Layers children atop one another in Z-order, useful for badge overlays, card watermarks, and composite views.",
            accesskit_role: Role::GenericContainer,
            aliases: &[
                ("Qt", "QStackedLayout"),
                ("GTK", "GtkOverlay"),
                ("SwiftUI", "ZStack"),
                ("React", "<div style={{ position: 'relative' }}>"),
            ],
            code_snippet: "use martensite::widgets::{Container, Stack, Text};\n\nlet stack = Stack::new()\n    .child(Container::new().padding_uniform(8.0).child(Text::new(\"Card\")))\n    .child(Text::new(\"Badge\"));",
            instantiate: |state| match state {
                WidgetStateKind::Default => Box::new(
                    Stack::new()
                        .child(Container::new().padding_uniform(8.0).child(Text::new("Base Layer")))
                        .child(Text::new("Overlay Badge")),
                ),
                WidgetStateKind::Hover => Box::new(
                    Stack::new()
                        .child(Container::new().padding_uniform(8.0).child(Text::new("Hover Layer")))
                        .child(Text::new("Active Pin")),
                ),
                WidgetStateKind::Disabled => Box::new(
                    Stack::new()
                        .child(Container::new().padding_uniform(8.0).child(Text::new("Inert Base")))
                        .child(Text::new("Muted")),
                ),
                WidgetStateKind::Focused => Box::new(
                    Stack::new()
                        .child(Container::new().padding_uniform(8.0).child(Text::new("Focus Layer")))
                        .child(Text::new("Highlight")),
                ),
            },
        },
        WidgetEntry {
            name: "Container",
            family: WidgetFamily::Containers,
            description: "Single-child box container applying uniform or edge-specific padding and optional background tint.",
            accesskit_role: Role::GenericContainer,
            aliases: &[
                ("Qt", "QFrame"),
                ("GTK", "GtkFrame"),
                ("SwiftUI", "GroupBox"),
                ("React", "<div className=\"container\">"),
            ],
            code_snippet: "use martensite::widgets::{Container, Text};\n\nlet box_container = Container::new()\n    .padding_uniform(16.0)\n    .child(Text::new(\"Padded Content\"));",
            instantiate: |state| match state {
                WidgetStateKind::Default => Box::new(
                    Container::new()
                        .padding_uniform(12.0)
                        .child(Text::new("Normal Container Content")),
                ),
                WidgetStateKind::Hover => Box::new(
                    Container::new()
                        .padding_uniform(12.0)
                        .child(Text::new("Hover Container Surface")),
                ),
                WidgetStateKind::Disabled => Box::new(
                    Container::new()
                        .padding_uniform(12.0)
                        .child(Text::new("Disabled Container Box")),
                ),
                WidgetStateKind::Focused => Box::new(
                    Container::new()
                        .padding_uniform(12.0)
                        .child(Text::new("Active Container Focus")),
                ),
            },
        },
        WidgetEntry {
            name: "ScrollView",
            family: WidgetFamily::Containers,
            description: "Viewport container with virtualized clipping and spring momentum scroll mechanics.",
            accesskit_role: Role::ScrollView,
            aliases: &[
                ("Qt", "QScrollArea"),
                ("GTK", "GtkScrolledWindow"),
                ("SwiftUI", "ScrollView"),
                ("React", "<div style={{ overflow: 'auto' }}>"),
            ],
            code_snippet: "use martensite::widgets::{ScrollView, Text};\n\nlet scroll = ScrollView::new(Text::new(\"Long scrollable document content\"));",
            instantiate: |state| match state {
                WidgetStateKind::Default => Box::new(ScrollView::new(Text::new("Document ScrollView Port"))),
                WidgetStateKind::Hover => Box::new(ScrollView::new(Text::new("Active Scroll Area"))),
                WidgetStateKind::Disabled => Box::new(ScrollView::new(Text::new("Muted Scroll Region"))),
                WidgetStateKind::Focused => Box::new(ScrollView::new(Text::new("Focused Scroll Target"))),
            },
        },

        // ===================================================================
        // DATA
        // ===================================================================
        WidgetEntry {
            name: "DataGrid",
            family: WidgetFamily::Data,
            description: "High-performance virtualized data grid with pinned column headers, sorting, and row virtualization.",
            accesskit_role: Role::Table,
            aliases: &[
                ("Qt", "QTableView"),
                ("GTK", "GtkColumnView"),
                ("SwiftUI", "Table"),
                ("React", "<table /> / AG Grid / DataGrid"),
            ],
            code_snippet: "use martensite::widgets::{Table, TableColumn};\n\nlet grid = Table::new()\n    .columns([\n        TableColumn::new(\"id\", \"ID\").width(60.0),\n        TableColumn::new(\"name\", \"Name\").width(160.0),\n    ])\n    .row([\"#1\", \"Pressure Sensor\"])\n    .row([\"#2\", \"Thermal Couple\"]);",
            instantiate: |state| match state {
                WidgetStateKind::Default => Box::new(
                    Table::new()
                        .columns([
                            TableColumn::new("id", "Sensor ID").width(80.0),
                            TableColumn::new("status", "Status").width(120.0),
                        ])
                        .row(["SEN-01", "Online"])
                        .row(["SEN-02", "Calibrating"]),
                ),
                WidgetStateKind::Hover => Box::new(
                    Table::new()
                        .columns([
                            TableColumn::new("id", "Sensor ID").width(80.0),
                            TableColumn::new("status", "Status").width(120.0),
                        ])
                        .row(["SEN-01", "Online"])
                        .row(["SEN-02", "Calibrating"]),
                ),
                WidgetStateKind::Disabled => Box::new(
                    Table::new()
                        .columns([
                            TableColumn::new("id", "Sensor ID").width(80.0),
                            TableColumn::new("status", "Status").width(120.0),
                        ])
                        .row(["SEN-01", "Offline"])
                        .enabled(false),
                ),
                WidgetStateKind::Focused => Box::new(
                    Table::new()
                        .columns([
                            TableColumn::new("id", "Sensor ID").width(80.0),
                            TableColumn::new("status", "Status").width(120.0),
                        ])
                        .row(["SEN-01", "Online"])
                        .row(["SEN-02", "Calibrating"]),
                ),
            },
        },
        WidgetEntry {
            name: "Table",
            family: WidgetFamily::Data,
            description: "Tabular data presentation with sortable, resizable headers, row selection, and keyboard navigation.",
            accesskit_role: Role::Table,
            aliases: &[
                ("Qt", "QTableWidget"),
                ("GTK", "GtkGrid"),
                ("SwiftUI", "Grid"),
                ("React", "<table />"),
            ],
            code_snippet: "use martensite::widgets::{Table, TableColumn};\n\nlet table = Table::new()\n    .columns([TableColumn::new(\"device\", \"Device\"), TableColumn::new(\"val\", \"Value\")])\n    .row([\"Pump 1\", \"42.5 kPa\"]);",
            instantiate: |state| match state {
                WidgetStateKind::Default => Box::new(
                    Table::new()
                        .columns([
                            TableColumn::new("param", "Parameter").width(100.0),
                            TableColumn::new("val", "Reading").width(100.0),
                        ])
                        .row(["Voltage", "230 V"])
                        .row(["Current", "4.8 A"]),
                ),
                WidgetStateKind::Hover => Box::new(
                    Table::new()
                        .columns([
                            TableColumn::new("param", "Parameter").width(100.0),
                            TableColumn::new("val", "Reading").width(100.0),
                        ])
                        .row(["Voltage", "230 V"]),
                ),
                WidgetStateKind::Disabled => Box::new(
                    Table::new()
                        .columns([
                            TableColumn::new("param", "Parameter").width(100.0),
                            TableColumn::new("val", "Reading").width(100.0),
                        ])
                        .row(["Voltage", "0 V"])
                        .enabled(false),
                ),
                WidgetStateKind::Focused => Box::new(
                    Table::new()
                        .columns([
                            TableColumn::new("param", "Parameter").width(100.0),
                            TableColumn::new("val", "Reading").width(100.0),
                        ])
                        .row(["Voltage", "230 V"])
                        .row(["Current", "4.8 A"]),
                ),
            },
        },
        WidgetEntry {
            name: "List",
            family: WidgetFamily::Data,
            description: "Virtualized vertical list view with alternating rows, single/multiple selection, and keyboard navigation.",
            accesskit_role: Role::List,
            aliases: &[
                ("Qt", "QListView"),
                ("GTK", "GtkListView"),
                ("SwiftUI", "List"),
                ("React", "<ul /> / VirtualList"),
            ],
            code_snippet: "use martensite::widgets::ListView;\n\nlet list = ListView::new()\n    .items([\"Alpha Feed\", \"Beta Pump\", \"Gamma Tank\"])\n    .row_height(32.0);",
            instantiate: |state| match state {
                WidgetStateKind::Default => Box::new(
                    ListView::new()
                        .items(["Workstation 01", "Workstation 02", "Workstation 03"])
                        .row_height(28.0),
                ),
                WidgetStateKind::Hover => Box::new(
                    ListView::new()
                        .items(["Active Unit A", "Active Unit B"])
                        .row_height(28.0),
                ),
                WidgetStateKind::Disabled => Box::new(
                    ListView::new()
                        .items(["Offline Unit"])
                        .row_height(28.0)
                        .enabled(false),
                ),
                WidgetStateKind::Focused => Box::new(
                    ListView::new()
                        .items(["Selected Item", "Pending Item"])
                        .row_height(28.0),
                ),
            },
        },

        // ===================================================================
        // OVERLAYS
        // ===================================================================
        WidgetEntry {
            name: "Tooltip",
            family: WidgetFamily::Overlays,
            description: "ARIA APG compliant tooltip popup with grace window timing, keyboard focus exposure, and light-dismiss.",
            accesskit_role: Role::Tooltip,
            aliases: &[
                ("Qt", "QToolTip"),
                ("GTK", "gtk_widget_set_tooltip_text"),
                ("SwiftUI", ".help(...)"),
                ("React", "<Tooltip />"),
            ],
            code_snippet: "use martensite::widgets::{Button, Tooltip};\n\nlet tip = Tooltip::new(\n    Button::new(\"Save\"),\n    \"Save changes to persistent disk\"\n);",
            instantiate: |state| match state {
                WidgetStateKind::Default => Box::new(Tooltip::new(Button::new("Inspect"), "Inspect machine diagnostics")),
                WidgetStateKind::Hover => Box::new(Tooltip::new(Button::new("Status"), "Current operational status: Normal")),
                WidgetStateKind::Disabled => Box::new(Tooltip::new(Button::new("Inert").enabled(false), "Action is unavailable")),
                WidgetStateKind::Focused => Box::new(Tooltip::new(Button::new("Active Focus"), "Keyboard accessible tooltip")),
            },
        },
        WidgetEntry {
            name: "Popover",
            family: WidgetFamily::Overlays,
            description: "Anchored floating popup bubble with directional arrow tail, light-dismiss, and viewport boundary clamping.",
            accesskit_role: Role::Dialog,
            aliases: &[
                ("Qt", "QMenu popup"),
                ("GTK", "GtkPopover"),
                ("SwiftUI", ".popover(...)"),
                ("React", "<Popover />"),
            ],
            code_snippet: "use martensite::widgets::{Popover, Text};\n\nlet pop = Popover::new()\n    .title(\"Options\")\n    .child(Text::new(\"Details panel\"));",
            instantiate: |state| match state {
                WidgetStateKind::Default => Box::new(Popover::new().title("Details").child(Text::new("Panel Content"))),
                WidgetStateKind::Hover => Box::new(Popover::new().title("Preview").child(Text::new("Quick View"))),
                WidgetStateKind::Disabled => Box::new(Popover::new().title("Locked").child(Text::new("Unavailable"))),
                WidgetStateKind::Focused => Box::new(Popover::new().title("Settings").child(Text::new("Config Pane"))),
            },
        },
        WidgetEntry {
            name: "Popconfirm",
            family: WidgetFamily::Overlays,
            description: "Compact confirmation bubble asking the user to confirm or cancel an action before proceeding.",
            accesskit_role: Role::Dialog,
            aliases: &[
                ("Qt", "QMessageBox"),
                ("GTK", "GtkMessageDialog"),
                ("SwiftUI", ".confirmationDialog(...)"),
                ("React", "<Popconfirm />"),
            ],
            code_snippet: "use martensite::widgets::Popconfirm;\n\nlet confirm = Popconfirm::new()\n    .question(\"Restart system?\")\n    .confirm_label(\"Restart\")\n    .cancel_label(\"Abort\");",
            instantiate: |state| match state {
                WidgetStateKind::Default => Box::new(
                    Popconfirm::new()
                        .question("Apply changes?")
                        .confirm_label("Yes")
                        .cancel_label("No"),
                ),
                WidgetStateKind::Hover => Box::new(
                    Popconfirm::new()
                        .question("Export report?")
                        .confirm_label("Export")
                        .cancel_label("Cancel"),
                ),
                WidgetStateKind::Disabled => Box::new(
                    Popconfirm::new()
                        .question("Action locked")
                        .confirm_label("OK")
                        .cancel_label("Dismiss"),
                ),
                WidgetStateKind::Focused => Box::new(
                    Popconfirm::new()
                        .question("Confirm reboot?")
                        .confirm_label("Reboot")
                        .cancel_label("Cancel"),
                ),
            },
        },

        // ===================================================================
        // NAVIGATION
        // ===================================================================
        WidgetEntry {
            name: "Tabs",
            family: WidgetFamily::Navigation,
            description: "APG tab set with roving tabindex keyboard navigation, automatic/manual activation, and tab panel switching.",
            accesskit_role: Role::TabList,
            aliases: &[
                ("Qt", "QTabWidget"),
                ("GTK", "GtkNotebook"),
                ("SwiftUI", "TabView"),
                ("React", "<Tabs />"),
            ],
            code_snippet: "use martensite::widgets::{Tabs, Text};\n\nlet tabs = Tabs::new()\n    .tab(\"Overview\", Text::new(\"System status\"))\n    .tab(\"Telemetry\", Text::new(\"Live metrics\"));",
            instantiate: |state| match state {
                WidgetStateKind::Default => Box::new(
                    Tabs::new()
                        .tab("General", Text::new("General parameters"))
                        .tab("Sensors", Text::new("Sensor readings")),
                ),
                WidgetStateKind::Hover => Box::new(
                    Tabs::new()
                        .tab("Active Tab", Text::new("Active panel"))
                        .tab("Queued", Text::new("Queue panel")),
                ),
                WidgetStateKind::Disabled => Box::new(
                    Tabs::new()
                        .tab("Muted Tab", Text::new("Inert content"))
                        .enabled(false),
                ),
                WidgetStateKind::Focused => Box::new(
                    Tabs::new()
                        .tab("Focused Tab", Text::new("Focus content"))
                        .tab("Next", Text::new("Next panel")),
                ),
            },
        },
        WidgetEntry {
            name: "NavRail",
            family: WidgetFamily::Navigation,
            description: "Vertical destination rail for top-level navigation with icon pills, keyboard focus, and activation notifications.",
            accesskit_role: Role::Navigation,
            aliases: &[
                ("Qt", "QToolBar / NavigationRail"),
                ("GTK", "AdwNavigationRail"),
                ("SwiftUI", "NavigationSplitView Sidebar"),
                ("React", "<NavRail />"),
            ],
            code_snippet: "use martensite::widgets::NavRail;\n\nlet rail = NavRail::new()\n    .destination(\"🏠\", \"Home\")\n    .destination(\"📊\", \"Metrics\")\n    .destination(\"⚙\", \"Config\");",
            instantiate: |state| match state {
                WidgetStateKind::Default => Box::new(
                    NavRail::new()
                        .destination("🏠", "Home")
                        .destination("📊", "Metrics"),
                ),
                WidgetStateKind::Hover => Box::new(
                    NavRail::new()
                        .destination("🏠", "Home")
                        .destination("⚙", "Settings")
                        .selected(1),
                ),
                WidgetStateKind::Disabled => Box::new(
                    NavRail::new()
                        .destination("🔒", "Locked")
                        .enabled(false),
                ),
                WidgetStateKind::Focused => Box::new(
                    NavRail::new()
                        .destination("🏠", "Home")
                        .destination("🔍", "Search")
                        .selected(0),
                ),
            },
        },
    ]
}
