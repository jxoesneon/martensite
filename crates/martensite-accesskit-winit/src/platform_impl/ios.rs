// Copyright 2026 The AccessKit Authors. All rights reserved.
// Licensed under the Apache License, Version 2.0 (found in
// the LICENSE-APACHE file).

use crate::raw_window_handle::{HasWindowHandle, RawWindowHandle};

use accesskit::{ActionHandler, ActivationHandler, DeactivationHandler, TreeUpdate};
use accesskit_ios::SubclassingAdapter;
use winit::{event::WindowEvent, event_loop::ActiveEventLoop, window::Window};

/// Platform-specific AccessKit adapter for winit.
pub struct Adapter {
    adapter: SubclassingAdapter,
}

impl Adapter {
    pub fn new(
        _event_loop: &impl ActiveEventLoop,
        window: &(impl Window + HasWindowHandle),
        activation_handler: impl 'static + ActivationHandler,
        action_handler: impl 'static + ActionHandler,
        deactivation_handler: impl 'static + DeactivationHandler,
    ) -> Self {
        let view = match window.window_handle().unwrap().as_raw() {
            RawWindowHandle::UiKit(handle) => handle.ui_view.as_ptr(),
            _ => unreachable!(),
        };

        // SAFETY: The view pointer comes from a valid winit window handle
        // and is passed directly to the AccessKit iOS subclassing adapter.
        let adapter = unsafe {
            SubclassingAdapter::new(
                view,
                activation_handler,
                action_handler,
                deactivation_handler,
            )
        };
        Self { adapter }
    }

    pub fn update_if_active(&mut self, updater: impl FnOnce() -> TreeUpdate) {
        if let Some(events) = self.adapter.update_if_active(updater) {
            events.raise();
        }
    }

    pub fn process_event(&mut self, _window: &impl Window, _event: &WindowEvent) {}
}
