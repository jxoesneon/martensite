// Copyright 2025 The AccessKit Authors. All rights reserved.
// Licensed under the Apache License, Version 2.0 (found in
// the LICENSE-APACHE file).

use accesskit::{ActionHandler, ActivationHandler, DeactivationHandler, TreeUpdate};
use martensite_access_platform::android::AndroidAdapter;
use winit::{
    event::WindowEvent, event_loop::ActiveEventLoop, platform::android::ActiveEventLoopExtAndroid,
    window::Window,
};

/// Platform-specific AccessKit adapter for winit.
///
/// This is a thin, safe wrapper over
/// [`martensite_access_platform::android::AndroidAdapter`], which owns
/// the JNI boundary (the workspace keeps `unsafe` FFI in that crate).
/// It requires the **GameActivity** backend — `NativeActivity` is
/// unsupported because its IME events are unreliable and the AccessKit
/// delegate cannot be injected into its window.
pub struct Adapter {
    adapter: AndroidAdapter,
}

impl Adapter {
    pub fn new(
        event_loop: &dyn ActiveEventLoop,
        _window: &dyn Window,
        activation_handler: impl 'static + ActivationHandler + Send,
        action_handler: impl 'static + ActionHandler + Send,
        _deactivation_handler: impl 'static + DeactivationHandler,
    ) -> Self {
        let app = event_loop.android_app();
        let adapter = AndroidAdapter::new(app, activation_handler, action_handler)
            .expect("failed to create the AccessKit Android adapter");
        Self { adapter }
    }

    pub fn update_if_active(&mut self, updater: impl FnOnce() -> TreeUpdate) {
        self.adapter.update_if_active(updater);
    }

    pub fn process_event(&mut self, _window: &dyn Window, _event: &WindowEvent) {}
}
