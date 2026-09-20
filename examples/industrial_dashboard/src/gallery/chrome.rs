//! Chrome & actions category — window chrome, menus, dialogs,
//! launchers, and action affordances. Each entry is built from the
//! widget's own documented construction (its doctest), adapted to the
//! industrial-dashboard theme where natural.

use martensite::core::Widget;
use martensite::widgets::{
    ActionSheet, AddressBar, AlertDialog, AlertRole, AlertSeverity, BottomSheet, Button,
    CommandAction, CommandLink, CommandPalette, Container, ContextMenu, CookieBanner, Copyable,
    Dialog, FloatButton, HeaderBar, KeyboardShortcuts, Link, Menu, MenuBar, MenuButton, MenuItem,
    Notification, NotificationCenter, Popconfirm, Popover, ProgressBar, RadialMenu, Ribbon,
    ShortcutGroup, SpeedDial, Splash, SplitButton, StatusBar, StatusItem, Text, ToolItem,
    ToolPalette, Toolbar, ToolbarOverflow, Tour, UpdatePrompt, WindowControls,
};

/// Chrome & actions showcase entries — `(display name, live widget)`.
pub fn entries() -> Vec<(&'static str, Box<dyn Widget>)> {
    vec![
        (
            "Action Sheet",
            Box::new(
                ActionSheet::new()
                    .title("Pump P-101")
                    .message("Select an action for the selected pump.")
                    .action("Acknowledge")
                    .action("View Trend")
                    .destructive("Trip Pump")
                    .cancel("Cancel"),
            ),
        ),
        (
            "Address Bar",
            Box::new(AddressBar::new("https://scada.plant.local/line-a")),
        ),
        (
            "Alert Dialog",
            Box::new(
                AlertDialog::new()
                    .title("Overtemperature")
                    .message("Reactor T-201 exceeded 240°C.")
                    .severity(AlertSeverity::Warning)
                    .button("Acknowledge", AlertRole::Confirm)
                    .button("Snooze", AlertRole::Cancel),
            ),
        ),
        (
            "Bottom Sheet",
            Box::new(
                BottomSheet::new()
                    .title("Line A Controls")
                    .detents(&[0.4, 0.9])
                    .child(Text::new("Pump controls")),
            ),
        ),
        (
            "Button",
            Box::new(Button::new("Acknowledge Alarm").tooltip("Silence the active alarm")),
        ),
        (
            "Command Link",
            Box::new(
                CommandLink::new("Run Diagnostics").note("Executes the full self-test sequence"),
            ),
        ),
        (
            "Command Palette",
            Box::new(
                CommandPalette::new()
                    .actions([
                        CommandAction::new("line.start", "Start Line A"),
                        CommandAction::new("line.stop", "Stop Line A").keywords(["halt"]),
                        CommandAction::new("alarm.ack", "Acknowledge All Alarms"),
                    ])
                    .placeholder("Type a command…"),
            ),
        ),
        (
            "Context Menu",
            Box::new(ContextMenu::new(
                Text::new("Right-click me"),
                vec![
                    MenuItem::action("Inspect"),
                    MenuItem::action("Calibrate"),
                    MenuItem::separator(),
                    MenuItem::action("Remove"),
                ],
            )),
        ),
        (
            "Cookie Banner",
            Box::new(
                CookieBanner::new("This HMI stores session preferences on this terminal.")
                    .policy_link("Privacy policy")
                    .labels("Accept", "Decline", "Customize"),
            ),
        ),
        (
            "Copyable",
            Box::new(Copyable::new("SN-2024-08-1147").label("Copy serial number")),
        ),
        (
            "Dialog",
            Box::new(
                Dialog::new("Confirm Shutdown")
                    .body("This will halt Line A. Continue?")
                    .buttons(&["Cancel", "Shut Down"]),
            ),
        ),
        ("Float Button", Box::new(FloatButton::new("+"))),
        (
            "Header Bar",
            Box::new(
                HeaderBar::new("Line A — Overview")
                    .subtitle("Cell 3")
                    .leading(Button::new("<"))
                    .trailing(Button::new("+")),
            ),
        ),
        (
            "Keyboard Shortcuts",
            Box::new(KeyboardShortcuts::new(vec![
                ShortcutGroup::new("Alarms")
                    .row("Acknowledge", "⌘K")
                    .row("Silence", "⌘⇧S"),
                ShortcutGroup::new("View").row("Overview", "⌘1"),
            ])),
        ),
        (
            "Link",
            Box::new(Link::new("PLC-4 datasheet").target("https://docs.plant.local/plc-4")),
        ),
        (
            "Menu",
            Box::new(Menu::new([
                MenuItem::action("Open Trend").with_shortcut("⌘T"),
                MenuItem::checkable("Auto-scroll", true),
                MenuItem::separator(),
                MenuItem::submenu(
                    "Export",
                    vec![MenuItem::action("CSV"), MenuItem::action("PDF")],
                ),
            ])),
        ),
        (
            "Menu Bar",
            Box::new(
                MenuBar::new()
                    .menu(
                        "File",
                        vec![
                            MenuItem::action("New Recipe").with_shortcut("⌘N"),
                            MenuItem::action("Quit"),
                        ],
                    )
                    .menu(
                        "Alarms",
                        vec![
                            MenuItem::action("Acknowledge All"),
                            MenuItem::checkable("Audible", true),
                        ],
                    ),
            ),
        ),
        (
            "Menu Button",
            Box::new(MenuButton::new(
                "Actions",
                vec![
                    MenuItem::action("Start"),
                    MenuItem::action("Stop"),
                    MenuItem::separator(),
                    MenuItem::action("Reset"),
                ],
            )),
        ),
        ("Notification Center", {
            let mut center = NotificationCenter::new();
            center.push(Notification::new("Alarm 1024", "Tank T-3 level high").meta("2 min ago"));
            center.push(Notification::new("PM due", "Pump P-101 service").meta("1 h ago"));
            Box::new(center)
        }),
        (
            "Popconfirm",
            Box::new(
                Popconfirm::new()
                    .question("Trip pump P-101?")
                    .confirm_label("Trip")
                    .cancel_label("Cancel"),
            ),
        ),
        (
            "Popover",
            Box::new(
                Popover::new()
                    .title("Sensor S-12")
                    .child(Text::new("4–20 mA · calibrated 12 d ago")),
            ),
        ),
        (
            "Radial Menu",
            Box::new(
                RadialMenu::new()
                    .items(["Start", "Stop", "Jog", "Reset", "Home"])
                    .label("Axis control"),
            ),
        ),
        (
            "Ribbon",
            Box::new(Ribbon::new("NEW").child(Container::new())),
        ),
        (
            "Speed Dial",
            Box::new(
                SpeedDial::new()
                    .label("Quick actions")
                    .action("Start Line A")
                    .action("Stop Line A")
                    .action("Call Supervisor"),
            ),
        ),
        ("Split Button", Box::new(SplitButton::new("Export Report"))),
        ("Splash", {
            let mut splash = Splash::new("Martensite HMI").version("4.2.0");
            splash.set_progress(0.65);
            splash.set_status("Loading recipes…");
            Box::new(splash)
        }),
        (
            "Status Bar",
            Box::new(
                StatusBar::new()
                    .message("Ready")
                    .add_zone(Button::new("Stop"))
                    .add_permanent(StatusItem::widget(ProgressBar::new())),
            ),
        ),
        (
            "Tool Palette",
            Box::new(
                ToolPalette::new()
                    .tool(ToolItem::new("⌀", "Measure"))
                    .tool(ToolItem::new("✚", "Annotate"))
                    .tool(ToolItem::new("⚙", "Calibrate"))
                    .columns(3)
                    .selected(0)
                    .show_labels(true),
            ),
        ),
        (
            "Toolbar",
            Box::new(
                Toolbar::new()
                    .item(Button::new("Start"))
                    .item(Button::new("Stop"))
                    .spacer()
                    .item(Button::new("E-Stop")),
            ),
        ),
        (
            "Toolbar Overflow",
            Box::new(
                ToolbarOverflow::new()
                    .item("Start")
                    .item("Pause")
                    .item("Stop")
                    .item("Reset")
                    .item("Diagnostics"),
            ),
        ),
        (
            "Tour",
            Box::new(
                Tour::new()
                    .step("Welcome", "This is the Line A overview.", None)
                    .step("Alarms", "Active alarms appear here.", None)
                    .skippable(true),
            ),
        ),
        (
            "Update Prompt",
            Box::new(
                UpdatePrompt::new("4.3.1")
                    .title("Firmware update available")
                    .notes("Adds OPC UA browsing; fixes trend export.")
                    .action_label("Install"),
            ),
        ),
        ("Window Controls", Box::new(WindowControls::new())),
    ]
}
