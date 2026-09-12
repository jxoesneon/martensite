# martensite-shell

Cross-platform shell integration for
[Martensite](https://github.com/jxoesneon/martensite): system backdrop
materials (Mica, Acrylic, Vibrancy), snap layouts, client-side
decorations, and system tray.

Provides a `BackdropController` trait with per-platform implementations —
DWM on Windows, `NSVisualEffectView` on macOS — plus safe-Rust Wayland
client-side decorations.

## License

MIT OR Apache-2.0
