// Copyright 2025 The AccessKit Authors. All rights reserved.
// Licensed under the Apache License, Version 2.0 (found in
// the LICENSE-APACHE file).

use crate::raw_window_handle::HasWindowHandle;

use accesskit::{ActionHandler, ActivationHandler, DeactivationHandler, TreeUpdate};
use accesskit_android::{
    jni::{objects::JObject, JavaVM},
    InjectingAdapter,
};
use winit::{
    event::WindowEvent, event_loop::ActiveEventLoop, platform::android::ActiveEventLoopExtAndroid,
    window::Window,
};

/// Platform-specific AccessKit adapter for winit.
pub struct Adapter {
    adapter: InjectingAdapter,
}

impl Adapter {
    pub fn new(
        event_loop: &impl ActiveEventLoop,
        _window: &(impl Window + HasWindowHandle),
        activation_handler: impl 'static + ActivationHandler + Send,
        action_handler: impl 'static + ActionHandler + Send,
        _deactivation_handler: impl 'static + DeactivationHandler,
    ) -> Self {
        let app = event_loop.android_app();
        // SAFETY: The JVM and activity pointers are provided by winit's Android
        // app extension and are valid for the lifetime of the active event loop.
        let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr() as *mut _) }.unwrap();
        let mut env = vm.get_env().unwrap();
        let activity = unsafe { JObject::from_raw(app.activity_as_ptr() as *mut _) };
        let view = env
            .get_field(
                &activity,
                "mSurfaceView",
                "Lcom/google/androidgamesdk/GameActivity$InputEnabledSurfaceView;",
            )
            .unwrap()
            .l()
            .unwrap();
        let adapter = InjectingAdapter::new(&mut env, &view, activation_handler, action_handler);
        Self { adapter }
    }

    pub fn update_if_active(&mut self, updater: impl FnOnce() -> TreeUpdate) {
        self.adapter.update_if_active(updater);
    }

    pub fn process_event(&mut self, _window: &impl Window, _event: &WindowEvent) {}
}
