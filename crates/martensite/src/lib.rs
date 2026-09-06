//! The Martensite GUI Framework.
#![forbid(unsafe_code)]

pub use martensite_core as core;
pub use martensite_reactive as reactive;
pub use martensite_layout as layout;
pub use martensite_wgpu as wgpu;
pub use martensite_render as render;
pub use martensite_text as text;
pub use martensite_access as access;
pub use martensite_window as window;
pub use martensite_focus as focus;
pub use martensite_clipboard as clipboard;
pub use martensite_dnd as dnd;
pub use martensite_theme as theme;
pub use martensite_motion as motion;
pub use martensite_media as media;
pub use martensite_history as history;
pub use martensite_assets as assets;
pub use martensite_l10n as l10n;
pub use martensite_devtools as devtools;
pub use martensite_macros as macros;
pub use martensite_test as test;

pub mod prelude {
    pub use martensite_core::{Widget, WidgetId, HotNode, ColdNode, NodeFlags, Rect, WidgetArena};
    pub use martensite_reactive::{Signal, Memo};
    pub use martensite_theme::Oklab;
    pub use martensite_motion::{SpringConfig, SpringSolver};
}
