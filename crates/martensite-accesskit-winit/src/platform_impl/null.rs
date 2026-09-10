// Copyright 2022 The AccessKit Authors. All rights reserved.
// Licensed under the Apache License, Version 2.0 (found in
// the LICENSE-APACHE file).

use crate::raw_window_handle::HasWindowHandle;

use accesskit::{ActionHandler, ActivationHandler, DeactivationHandler, TreeUpdate};
use winit::{event::WindowEvent, event_loop::ActiveEventLoop, window::Window};

/// Platform-specific AccessKit adapter for winit.
pub struct Adapter;

impl Adapter {
    pub fn new(
        _event_loop: &impl ActiveEventLoop,
        _window: &(impl Window + HasWindowHandle),
        _activation_handler: impl 'static + ActivationHandler,
        _action_handler: impl 'static + ActionHandler,
        _deactivation_handler: impl 'static + DeactivationHandler,
    ) -> Self {
        Self {}
    }

    pub fn update_if_active(&mut self, _updater: impl FnOnce() -> TreeUpdate) {}

    pub fn process_event(&mut self, _window: &impl Window, _event: &WindowEvent) {}
}
