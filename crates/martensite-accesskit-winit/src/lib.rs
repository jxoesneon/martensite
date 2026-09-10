// Copyright 2022 The AccessKit Authors. All rights reserved.
// Licensed under the Apache License, Version 2.0 (found in
// the LICENSE-APACHE file).

//! Vendored and patched copy of `accesskit_winit` 0.34.0, updated for
//! winit 0.31.0-beta.3. This crate is an internal Martensite workspace
//! dependency; do not depend on it outside the workspace.

// The vendored platform adapters require `unsafe` blocks to call platform
// constructors that are `unsafe fn`. This is analogous to the upstream crate.
#![allow(unsafe_code)]

#[cfg(all(
    feature = "accesskit_unix",
    any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd"
    ),
    not(feature = "async-io"),
    not(feature = "tokio")
))]
compile_error!("Either \"async-io\" (default) or \"tokio\" feature must be enabled.");

#[cfg(all(
    feature = "accesskit_unix",
    any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd"
    ),
    feature = "async-io",
    feature = "tokio"
))]
compile_error!(
    "Both \"async-io\" (default) and \"tokio\" features cannot be enabled at the same time."
);

use accesskit::{ActionHandler, ActivationHandler, DeactivationHandler, TreeUpdate};
use raw_window_handle::HasWindowHandle;
use rwh_06 as raw_window_handle;
use winit::{event::WindowEvent as WinitWindowEvent, event_loop::ActiveEventLoop, window::Window};

mod platform_impl;

/// AccessKit adapter for a winit window.
pub struct Adapter {
    inner: platform_impl::Adapter,
}

impl Adapter {
    /// Creates a new AccessKit adapter for a winit window. This must be done
    /// before the window is shown for the first time. This means that you must
    /// use [`winit::window::WindowAttributes::with_visible`] to make the window
    /// initially invisible, then create the adapter, then show the window.
    ///
    /// # Panics
    ///
    /// Panics if the window is already visible.
    pub fn with_direct_handlers(
        _event_loop: &impl ActiveEventLoop,
        window: &(impl Window + HasWindowHandle),
        activation_handler: impl 'static + ActivationHandler + Send,
        action_handler: impl 'static + ActionHandler + Send,
        deactivation_handler: impl 'static + DeactivationHandler + Send,
    ) -> Self {
        if window.is_visible() == Some(true) {
            panic!(
                "The AccessKit winit adapter must be created before the window is shown (made visible) for the first time."
            );
        }

        let inner = platform_impl::Adapter::new(
            _event_loop,
            window,
            activation_handler,
            action_handler,
            deactivation_handler,
        );
        Self { inner }
    }

    /// Allows reacting to window events.
    ///
    /// This must be called whenever a new window event is received
    /// and before it is handled by the application.
    pub fn process_event(&mut self, _window: &impl Window, _event: &WinitWindowEvent) {
        self.inner.process_event(_window, _event);
    }

    /// If and only if the tree has been initialized, call the provided function
    /// and apply the resulting update.
    pub fn update_if_active(&mut self, updater: impl FnOnce() -> TreeUpdate) {
        self.inner.update_if_active(updater);
    }
}
