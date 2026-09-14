// Copyright 2026 The AccessKit Authors. All rights reserved.
// Licensed under the Apache License, Version 2.0 (found in
// the LICENSE-APACHE file).

use crate::raw_window_handle::{HasWindowHandle, RawWindowHandle};

use accesskit::{ActionHandler, ActivationHandler, DeactivationHandler, TreeUpdate};
use martensite_access_platform::ios::IosAdapter;
use winit::{event::WindowEvent, event_loop::ActiveEventLoop, window::Window};

/// Platform-specific AccessKit adapter for winit.
///
/// The `accesskit_ios` `SubclassingAdapter` Objective-C FFI boundary is
/// delegated to [`IosAdapter`] in `martensite-access-platform`, the
/// workspace's whitelisted-unsafe mobile glue crate.
pub struct Adapter {
    adapter: IosAdapter,
}

impl Adapter {
    pub fn new(
        _event_loop: &dyn ActiveEventLoop,
        window: &dyn Window,
        activation_handler: impl 'static + ActivationHandler,
        action_handler: impl 'static + ActionHandler,
        deactivation_handler: impl 'static + DeactivationHandler,
    ) -> Self {
        let view = match window.window_handle().unwrap().as_raw() {
            RawWindowHandle::UiKit(handle) => handle.ui_view,
            _ => unreachable!(),
        };

        // Caller requirements upheld here (see `IosAdapter::new`): the
        // view pointer comes from a valid winit window handle, this runs
        // on the main thread inside `can_create_surfaces`, and the adapter
        // is created before the app first renders. Note that the
        // `is_visible()` guard in `Adapter::with_direct_handlers` cannot
        // fire on iOS — winit-uikit reports visibility as `None` — so the
        // before-show ordering is upheld by this call convention alone.
        let adapter = IosAdapter::new(
            view,
            activation_handler,
            action_handler,
            deactivation_handler,
        );
        Self { adapter }
    }

    pub fn update_if_active(&mut self, updater: impl FnOnce() -> TreeUpdate) {
        self.adapter.update_if_active(updater);
    }

    pub fn process_event(&mut self, _window: &dyn Window, _event: &WindowEvent) {}
}
