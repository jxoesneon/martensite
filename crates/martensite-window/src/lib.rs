//! Multi-window management and fractional DPI scaling.
#![forbid(unsafe_code)]

pub use winit::window::{Window, WindowId};

#[cfg(test)]
mod tests {
    use super::{Window, WindowId};

    #[test]
    fn reexports_are_accessible() {
        // Window and WindowId require a running event loop to construct,
        // so we only verify the re-exported types are accessible from this
        // crate's public API.
        // `WindowId` is a struct; verify it is usable in type position.
        let _: Option<WindowId> = None;
        // `Window` is a trait; verify it is accessible as a trait bound.
        fn _accepts_window<T: Window + ?Sized>() {}
        // Reaching this point proves the re-exported types are accessible.
    }
}
