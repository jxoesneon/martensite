//! `device` namespace — monitors, phones, peripherals, batteries, network radios.

use super::super::{IconEntry, IconPair};

/// Qualified icon names (`device.*`).
///
/// Every name constant in this module is prefixed `DEVICE_`
/// (`DEVICE_FOO` → `"device.foo"`) so the flattened
/// `builtin::names` re-export cannot collide.
pub mod names {
    /// `"device.monitor"` — desktop display.
    pub const DEVICE_MONITOR: &str = "device.monitor";
    /// `"device.smartphone"` — handset.
    pub const DEVICE_SMARTPHONE: &str = "device.smartphone";
    /// `"device.tablet"` — tablet slate.
    pub const DEVICE_TABLET: &str = "device.tablet";
    /// `"device.laptop"` — clamshell notebook.
    pub const DEVICE_LAPTOP: &str = "device.laptop";
    /// `"device.watch"` — wristwatch / wearable.
    pub const DEVICE_WATCH: &str = "device.watch";
    /// `"device.printer"` — printer with paper tray.
    pub const DEVICE_PRINTER: &str = "device.printer";
    /// `"device.scanner"` — scan brackets with scan line.
    pub const DEVICE_SCANNER: &str = "device.scanner";
    /// `"device.keyboard"` — typing keyboard.
    pub const DEVICE_KEYBOARD: &str = "device.keyboard";
    /// `"device.mouse"` — pointing mouse.
    pub const DEVICE_MOUSE: &str = "device.mouse";
    /// `"device.webcam"` — camera on a stand.
    pub const DEVICE_WEBCAM: &str = "device.webcam";
    /// `"device.hard-drive"` — disk enclosure.
    pub const DEVICE_HARD_DRIVE: &str = "device.hard-drive";
    /// `"device.memory-stick"` — RAM / DIMM module.
    pub const DEVICE_MEMORY_STICK: &str = "device.memory-stick";
    /// `"device.card"` — SD-style card.
    pub const DEVICE_CARD: &str = "device.card";
    /// `"device.usb"` — USB trident mark.
    pub const DEVICE_USB: &str = "device.usb";
    /// `"device.plug"` — power plug.
    pub const DEVICE_PLUG: &str = "device.plug";
    /// `"device.battery"` — charge cell, empty.
    pub const DEVICE_BATTERY: &str = "device.battery";
    /// `"device.battery-low"` — charge cell, one bar.
    pub const DEVICE_BATTERY_LOW: &str = "device.battery-low";
    /// `"device.battery-medium"` — charge cell, two bars.
    pub const DEVICE_BATTERY_MEDIUM: &str = "device.battery-medium";
    /// `"device.battery-full"` — charge cell, three bars.
    pub const DEVICE_BATTERY_FULL: &str = "device.battery-full";
    /// `"device.battery-charging"` — cell with charge bolt.
    pub const DEVICE_BATTERY_CHARGING: &str = "device.battery-charging";
    /// `"device.battery-warning"` — cell with alert mark.
    pub const DEVICE_BATTERY_WARNING: &str = "device.battery-warning";
    /// `"device.server"` — stacked rack units.
    pub const DEVICE_SERVER: &str = "device.server";
    /// `"device.router"` — network box with antenna.
    pub const DEVICE_ROUTER: &str = "device.router";
    /// `"device.wifi"` — Wi-Fi signal, full.
    pub const DEVICE_WIFI: &str = "device.wifi";
    /// `"device.wifi-off"` — Wi-Fi disabled.
    pub const DEVICE_WIFI_OFF: &str = "device.wifi-off";
    /// `"device.wifi-low"` — Wi-Fi signal, weak.
    pub const DEVICE_WIFI_LOW: &str = "device.wifi-low";
    /// `"device.bluetooth"` — Bluetooth radio.
    pub const DEVICE_BLUETOOTH: &str = "device.bluetooth";
    /// `"device.antenna"` — broadcast tower.
    pub const DEVICE_ANTENNA: &str = "device.antenna";
    /// `"device.gamepad"` — controller, box body.
    pub const DEVICE_GAMEPAD: &str = "device.gamepad";
    /// `"device.gamepad-2"` — controller, winged body.
    pub const DEVICE_GAMEPAD_2: &str = "device.gamepad-2";
    /// `"device.tv"` — television with rabbit ears.
    pub const DEVICE_TV: &str = "device.tv";
    /// `"device.touchpad"` — trackpad with button split.
    pub const DEVICE_TOUCHPAD: &str = "device.touchpad";
}

/// `"device.monitor"` — panel, stand stem, foot.
pub const DEVICE_MONITOR: &str =
    "M4 3h16a2 2 0 012 2v10a2 2 0 01-2 2H4a2 2 0 01-2-2V5a2 2 0 012-2zM8 21h8M12 17v4";
/// `"device.smartphone"` — tall rounded slab plus home dot.
pub const DEVICE_SMARTPHONE: &str =
    "M7 2h10a2 2 0 012 2v16a2 2 0 01-2 2H7a2 2 0 01-2-2V4a2 2 0 012-2zM12 18h.01";
/// `"device.tablet"` — wide rounded slab plus home dot.
pub const DEVICE_TABLET: &str =
    "M6 2h12a2 2 0 012 2v16a2 2 0 01-2 2H6a2 2 0 01-2-2V4a2 2 0 012-2zM12 18h.01";
/// `"device.laptop"` — open lid plus flared base.
pub const DEVICE_LAPTOP: &str =
    "M18 5a2 2 0 012 2v8.53a2 2 0 00.21.9l1.07 2.12a1 1 0 01-.9 1.45H3.62a1 1 0 01-.9-1.45l1.07-2.12A2 2 0 004 15.53V7a2 2 0 012-2zM20.05 15.99H3.95";
/// `"device.watch"` — round face, straps, hands.
pub const DEVICE_WATCH: &str =
    "M18 12a6 6 0 11-12 0 6 6 0 0112 0zM12 10v2.2l1.6 1M16.13 7.66l-.81-4.05a2 2 0 00-2-1.61h-2.68a2 2 0 00-2 1.61l-.78 4.05M7.88 16.36l.8 4a2 2 0 002 1.61h2.72a2 2 0 002-1.61l.81-4.05";
/// `"device.printer"` — body, input tray paper, output page.
pub const DEVICE_PRINTER: &str =
    "M6 18H4a2 2 0 01-2-2v-5a2 2 0 012-2h16a2 2 0 012 2v5a2 2 0 01-2 2h-2M6 9V3a1 1 0 011-1h10a1 1 0 011 1v6M7 14h10a1 1 0 011 1v6a1 1 0 01-1 1H7a1 1 0 01-1-1v-6a1 1 0 011-1z";
/// `"device.scanner"` — four corner brackets plus scan line.
pub const DEVICE_SCANNER: &str =
    "M3 7V5a2 2 0 012-2h2M17 3h2a2 2 0 012 2v2M21 17v2a2 2 0 01-2 2h-2M7 21H5a2 2 0 01-2-2v-2M7 12h10";
/// `"device.keyboard"` — deck with two key rows plus spacebar.
pub const DEVICE_KEYBOARD: &str =
    "M4 4h16a2 2 0 012 2v12a2 2 0 01-2 2H4a2 2 0 01-2-2V6a2 2 0 012-2zM6 8h.01M10 8h.01M14 8h.01M18 8h.01M8 12h.01M12 12h.01M16 12h.01M7 16h10";
/// `"device.mouse"` — capsule body plus scroll wheel.
pub const DEVICE_MOUSE: &str = "M12 2a7 7 0 017 7v6a7 7 0 01-14 0V9a7 7 0 017-7zM12 6v4";
/// `"device.webcam"` — lens rings plus stand.
pub const DEVICE_WEBCAM: &str =
    "M20 10a8 8 0 11-16 0 8 8 0 0116 0zM15 10a3 3 0 11-6 0 3 3 0 016 0zM7 22h10M12 22v-4";
/// `"device.hard-drive"` — trapezoid lid, seam, activity dots.
pub const DEVICE_HARD_DRIVE: &str =
    "M10 16h.01M2.212 11.577a2 2 0 00-.212.896V18a2 2 0 002 2h16a2 2 0 002-2v-5.527a2 2 0 00-.212-.896L18.55 5.11A2 2 0 0016.76 4H7.24a2 2 0 00-1.79 1.11zM21.946 12.013H2.054M6 16h.01";
/// `"device.memory-stick"` — module body, edge pins, chip mark.
pub const DEVICE_MEMORY_STICK: &str =
    "M4 6h16a2 2 0 012 2v6a2 2 0 01-2 2H4a2 2 0 01-2-2V8a2 2 0 012-2zM6 16v3M10 16v3M14 16v3M18 16v3M9 10h6";
/// `"device.card"` — chamfered card plus contact stubs.
pub const DEVICE_CARD: &str =
    "M7 2l-3 4v14a2 2 0 002 2h12a2 2 0 002-2V4a2 2 0 00-2-2H7zM10 4v3M13.5 4v3M17 4v3";
/// `"device.usb"` — USB trident: arrow tip, circle, square ends.
pub const DEVICE_USB: &str =
    "M12 4L10 7h4zM12 7v10.5M13.5 19a1.5 1.5 0 11-3 0 1.5 1.5 0 013 0zM9.5 7.5a1.5 1.5 0 11-3 0 1.5 1.5 0 013 0zM8 9a4 4 0 004 4M12 16a5 5 0 015-5M17 8h4v4h-4z";
/// `"device.plug"` — two prongs, body, cord.
pub const DEVICE_PLUG: &str =
    "M9 8V2M15 8V2M17 8a1 1 0 011 1v4a4 4 0 01-4 4h-4a4 4 0 01-4-4V9a1 1 0 011-1zM12 22v-5";
/// `"device.battery"` — cell body plus nub.
pub const DEVICE_BATTERY: &str =
    "M4 6h14a2 2 0 012 2v8a2 2 0 01-2 2H4a2 2 0 01-2-2V8a2 2 0 012-2zM22 10v4";
/// `"device.battery-low"` — cell plus one fill bar.
pub const DEVICE_BATTERY_LOW: &str =
    "M4 6h14a2 2 0 012 2v8a2 2 0 01-2 2H4a2 2 0 01-2-2V8a2 2 0 012-2zM22 10v4M6 10v4";
/// `"device.battery-medium"` — cell plus two fill bars.
pub const DEVICE_BATTERY_MEDIUM: &str =
    "M4 6h14a2 2 0 012 2v8a2 2 0 01-2 2H4a2 2 0 01-2-2V8a2 2 0 012-2zM22 10v4M6 10v4M10 10v4";
/// `"device.battery-full"` — cell plus three fill bars.
pub const DEVICE_BATTERY_FULL: &str =
    "M4 6h14a2 2 0 012 2v8a2 2 0 01-2 2H4a2 2 0 01-2-2V8a2 2 0 012-2zM22 10v4M6 10v4M10 10v4M14 10v4";
/// `"device.battery-charging"` — broken cell plus charge bolt.
pub const DEVICE_BATTERY_CHARGING: &str =
    "M14.856 6H16a2 2 0 012 2v8a2 2 0 01-2 2h-2.935M5.14 18H4a2 2 0 01-2-2V8a2 2 0 012-2h2.936M11 7l-3 5h4l-3 5M22 14v-4";
/// `"device.battery-warning"` — broken cell plus alert mark.
pub const DEVICE_BATTERY_WARNING: &str =
    "M14 6h2a2 2 0 012 2v8a2 2 0 01-2 2h-2M6 18H4a2 2 0 01-2-2V8a2 2 0 012-2h2M10 7v6M10 17h.01M22 14v-4";
/// `"device.server"` — two rack units plus status dots.
pub const DEVICE_SERVER: &str =
    "M4 2h16a2 2 0 012 2v4a2 2 0 01-2 2H4a2 2 0 01-2-2V4a2 2 0 012-2zM4 14h16a2 2 0 012 2v4a2 2 0 01-2 2H4a2 2 0 01-2-2v-4a2 2 0 012-2zM6 6h.01M6 18h.01";
/// `"device.router"` — base unit, antenna, two wave arcs.
pub const DEVICE_ROUTER: &str =
    "M4 14h16a2 2 0 012 2v4a2 2 0 01-2 2H4a2 2 0 01-2-2v-4a2 2 0 012-2zM6 18h.01M10 18h.01M15 10v4M17.84 7.17a4 4 0 00-5.66 0M19.95 5.05a7 7 0 00-9.9 0";
/// `"device.wifi"` — three signal arcs plus emitter dot.
pub const DEVICE_WIFI: &str =
    "M12 20h.01M2 8.82a15 15 0 0120 0M5 12.86a10 10 0 0114 0M8.5 16.43a5 5 0 017 0";
/// `"device.wifi-off"` — wifi geometry plus strike slash.
pub const DEVICE_WIFI_OFF: &str =
    "M12 20h.01M2 8.82a15 15 0 0120 0M5 12.86a10 10 0 0114 0M8.5 16.43a5 5 0 017 0M2 2l20 20";
/// `"device.wifi-low"` — emitter dot plus weakest arc.
pub const DEVICE_WIFI_LOW: &str = "M12 20h.01M8.5 16.43a5 5 0 017 0";
/// `"device.bluetooth"` — rune ligature.
pub const DEVICE_BLUETOOTH: &str = "M7 7l10 10-5 5V2l5 5L7 17";
/// `"device.antenna"` — A-frame mast plus two wave pairs.
pub const DEVICE_ANTENNA: &str =
    "M8 22l4-11 4 11M9.5 18h5M14 8a2 2 0 11-4 0 2 2 0 014 0zM8 10a4.5 4.5 0 010-4M16 6a4.5 4.5 0 010 4M5.5 12.5a7 7 0 010-9M18.5 3.5a7 7 0 010 9";
/// `"device.gamepad"` — box body, d-pad, two buttons.
pub const DEVICE_GAMEPAD: &str =
    "M4 6h16a2 2 0 012 2v8a2 2 0 01-2 2H4a2 2 0 01-2-2V8a2 2 0 012-2zM6 12h4M8 10v4M15 13h.01M18 11h.01";
/// `"device.gamepad-2"` — winged body, d-pad, two buttons.
pub const DEVICE_GAMEPAD_2: &str =
    "M6 11h4M8 9v4M15 12h.01M18 10h.01M17.32 5H6.68a4 4 0 00-3.978 3.59c-.006.052-.01.101-.017.152C2.604 9.416 2 14.456 2 16a3 3 0 003 3c1 0 1.5-.5 2-1l1.414-1.414A2 2 0 019.828 16h4.344a2 2 0 011.414.586L17 18c.5.5 1 1 2 1a3 3 0 003-3c0-1.545-.604-6.584-.685-7.258-.007-.05-.011-.1-.017-.151A4 4 0 0017.32 5z";
/// `"device.tv"` — set plus rabbit-ear antenna.
pub const DEVICE_TV: &str =
    "M4 7h16a2 2 0 012 2v11a2 2 0 01-2 2H4a2 2 0 01-2-2V9a2 2 0 012-2zM17 2l-5 5-5-5";
/// `"device.touchpad"` — pad with split button row.
pub const DEVICE_TOUCHPAD: &str =
    "M4 4h16a2 2 0 012 2v12a2 2 0 01-2 2H4a2 2 0 01-2-2V6a2 2 0 012-2zM2 14h20M12 20v-6";

/// `device` entries.
pub const ENTRIES: &[IconEntry] = &[
    IconEntry::new(names::DEVICE_MONITOR, DEVICE_MONITOR),
    IconEntry::new(names::DEVICE_SMARTPHONE, DEVICE_SMARTPHONE),
    IconEntry::new(names::DEVICE_TABLET, DEVICE_TABLET),
    IconEntry::new(names::DEVICE_LAPTOP, DEVICE_LAPTOP),
    IconEntry::new(names::DEVICE_WATCH, DEVICE_WATCH),
    IconEntry::new(names::DEVICE_PRINTER, DEVICE_PRINTER),
    IconEntry::new(names::DEVICE_SCANNER, DEVICE_SCANNER),
    IconEntry::new(names::DEVICE_KEYBOARD, DEVICE_KEYBOARD),
    IconEntry::new(names::DEVICE_MOUSE, DEVICE_MOUSE),
    IconEntry::new(names::DEVICE_WEBCAM, DEVICE_WEBCAM),
    IconEntry::new(names::DEVICE_HARD_DRIVE, DEVICE_HARD_DRIVE),
    IconEntry::new(names::DEVICE_MEMORY_STICK, DEVICE_MEMORY_STICK),
    IconEntry::new(names::DEVICE_CARD, DEVICE_CARD),
    IconEntry::new(names::DEVICE_USB, DEVICE_USB),
    IconEntry::new(names::DEVICE_PLUG, DEVICE_PLUG),
    IconEntry::new(names::DEVICE_BATTERY, DEVICE_BATTERY),
    IconEntry::new(names::DEVICE_BATTERY_LOW, DEVICE_BATTERY_LOW),
    IconEntry::new(names::DEVICE_BATTERY_MEDIUM, DEVICE_BATTERY_MEDIUM),
    IconEntry::new(names::DEVICE_BATTERY_FULL, DEVICE_BATTERY_FULL),
    IconEntry::new(names::DEVICE_BATTERY_CHARGING, DEVICE_BATTERY_CHARGING),
    IconEntry::new(names::DEVICE_BATTERY_WARNING, DEVICE_BATTERY_WARNING),
    IconEntry::new(names::DEVICE_SERVER, DEVICE_SERVER),
    IconEntry::new(names::DEVICE_ROUTER, DEVICE_ROUTER),
    IconEntry::new(names::DEVICE_WIFI, DEVICE_WIFI),
    IconEntry::new(names::DEVICE_WIFI_OFF, DEVICE_WIFI_OFF),
    IconEntry::new(names::DEVICE_WIFI_LOW, DEVICE_WIFI_LOW),
    IconEntry::new(names::DEVICE_BLUETOOTH, DEVICE_BLUETOOTH),
    IconEntry::new(names::DEVICE_ANTENNA, DEVICE_ANTENNA),
    IconEntry::new(names::DEVICE_GAMEPAD, DEVICE_GAMEPAD),
    IconEntry::new(names::DEVICE_GAMEPAD_2, DEVICE_GAMEPAD_2),
    IconEntry::new(names::DEVICE_TV, DEVICE_TV),
    IconEntry::new(names::DEVICE_TOUCHPAD, DEVICE_TOUCHPAD),
];

/// `device` morph pairs.
pub const PAIRS: &[IconPair] = &[
    IconPair::new(names::DEVICE_WIFI, names::DEVICE_WIFI_OFF),
    IconPair::new(names::DEVICE_BATTERY_LOW, names::DEVICE_BATTERY_FULL),
];
