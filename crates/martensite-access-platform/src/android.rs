//! Android accessibility glue: GameActivity surface-view extraction and
//! `accesskit_android` [`InjectingAdapter`] construction.
//!
//! This module contains the JNI boundary for Android accessibility. It
//! resolves the `JavaVM` and the `GameActivity.mSurfaceView`
//! (`InputEnabledSurfaceView`) that hosts the Martensite surface, then
//! injects an AccessKit accessibility delegate into that view.
//!
//! **GameActivity is required.** `NativeActivity` is unsupported: it has
//! no `mSurfaceView` field, its IME event delivery is unreliable, and the
//! AccessKit delegate cannot be injected into its window (other Rust UI
//! frameworks disable accessibility on `NativeActivity` for the same
//! reasons). Select the GameActivity backend by enabling winit's
//! `android-game-activity` feature — `martensite-window` does this for
//! `cfg(target_os = "android")`.
//!
//! The `embedded-dex` feature of `accesskit_android` (enabled in this
//! crate's manifest) bundles the compiled
//! `dev.accesskit.android.Delegate` class, so no Java sources need to be
//! added to the application package.

use accesskit::{ActionHandler, ActivationHandler, TreeUpdate};
use accesskit_android::{
    jni::{errors::Error as JniError, objects::JObject, JavaVM},
    InjectingAdapter,
};
use android_activity::AndroidApp;

/// JNI signature of the `GameActivity.mSurfaceView` field. AccessKit's
/// injecting adapter installs its accessibility delegate on this view.
const GAME_ACTIVITY_SURFACE_VIEW_SIGNATURE: &str =
    "Lcom/google/androidgamesdk/GameActivity$InputEnabledSurfaceView;";

/// An error produced while constructing an [`AndroidAdapter`].
///
/// Carries the operation that was in progress when JNI reported a
/// failure plus the original [`JniError`]. Construction failures are
/// almost always a `NativeActivity`/missing-`GameActivity` mismatch —
/// see the [module documentation](self).
///
/// # Examples
///
/// ```ignore
/// # #[cfg(target_os = "android")]
/// # {
/// use martensite_access_platform::android::AndroidAdapterError;
/// use std::error::Error;
///
/// # fn demo(err: AndroidAdapterError) {
/// assert!(err.source().is_some());
/// # }
/// # }
/// ```
#[derive(Debug)]
pub struct AndroidAdapterError {
    /// The JNI operation that failed, for diagnostics.
    context: &'static str,
    /// The underlying JNI error, when the failure came from JNI itself
    /// (`None` when the surface-view field resolved but held `null`).
    source: Option<JniError>,
}

impl AndroidAdapterError {
    fn new(context: &'static str, source: JniError) -> Self {
        Self {
            context,
            source: Some(source),
        }
    }

    /// The `mSurfaceView` field resolved but was `null` — the activity is
    /// GameActivity-shaped but has not created its surface view yet, or
    /// the view hierarchy was torn down before adapter construction.
    fn null_surface_view() -> Self {
        Self {
            context: "GameActivity.mSurfaceView (null)",
            source: None,
        }
    }
}

impl std::fmt::Display for AndroidAdapterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.source {
            Some(source) => write!(
                f,
                "android accessibility adapter creation failed at {context}: {source}",
                context = self.context,
            ),
            None => write!(
                f,
                "android accessibility adapter creation failed at {context}",
                context = self.context
            ),
        }
    }
}

impl std::error::Error for AndroidAdapterError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source
            .as_ref()
            .map(|e| e as &(dyn std::error::Error + 'static))
    }
}

/// The platform AccessKit adapter for Android.
///
/// Wraps [`InjectingAdapter`], which installs an accessibility delegate
/// on the GameActivity surface view and services `AccessibilityNodeInfo`
/// requests on Android's UI thread. Tree updates are applied through
/// [`AndroidAdapter::update_if_active`].
///
/// # Examples
///
/// ```ignore
/// use martensite_access_platform::android::AndroidAdapter;
///
/// # fn example(
/// #     app: &android_activity::AndroidApp,
/// #     activation: impl 'static + accesskit::ActivationHandler + Send,
/// #     actions: impl 'static + accesskit::ActionHandler + Send,
/// # ) {
/// let adapter = AndroidAdapter::new(app, activation, actions)
///     .expect("GameActivity surface view must exist");
/// # }
/// ```
pub struct AndroidAdapter {
    adapter: InjectingAdapter,
}

impl AndroidAdapter {
    /// Creates the adapter for the running [`AndroidApp`].
    ///
    /// `activation_handler` and `action_handler` are the AccessKit
    /// handlers the injected delegate drives; they are typically the
    /// `MartensiteAccessBridge` handles from `martensite-access`.
    ///
    /// # Errors
    ///
    /// Returns [`AndroidAdapterError`] if the `JavaVM` or the
    /// `GameActivity.mSurfaceView` field cannot be resolved — the latter
    /// indicates the process is not running under `GameActivity`.
    ///
    /// # Panics
    ///
    /// [`InjectingAdapter::new`] itself performs infallible-signature JNI
    /// calls upstream that panic if the Java delegate class cannot be
    /// injected (for example when the view already has an accessibility
    /// delegate installed).
    pub fn new(
        app: &AndroidApp,
        activation_handler: impl 'static + ActivationHandler + Send,
        action_handler: impl 'static + ActionHandler + Send,
    ) -> Result<Self, AndroidAdapterError> {
        // SAFETY: `AndroidApp::vm_as_ptr` returns the process-wide
        // `JavaVM*` owned by the Android runtime. It outlives the
        // activity and is valid for the entire process lifetime.
        let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) }
            .map_err(|e| AndroidAdapterError::new("JavaVM::from_raw", e))?;
        // The adapter is constructed on the `android_main` thread, which
        // the runtime has already attached to the JVM, so `get_env`
        // cannot hit the detached-thread error path in practice.
        let mut env = vm
            .get_env()
            .map_err(|e| AndroidAdapterError::new("JavaVM::get_env", e))?;
        // SAFETY: `AndroidApp::activity_as_ptr` returns the activity's
        // global `jobject` reference, which is valid for the lifetime of
        // the activity (and therefore of this adapter construction call).
        let activity = unsafe { JObject::from_raw(app.activity_as_ptr().cast()) };
        let view = env
            .get_field(
                &activity,
                "mSurfaceView",
                GAME_ACTIVITY_SURFACE_VIEW_SIGNATURE,
            )
            .and_then(|value| value.l())
            .map_err(|e| {
                // A missing `mSurfaceView` field raises a pending
                // `NoSuchFieldError` (NativeActivity instead of
                // GameActivity). Clear it so the exception does not
                // escape into unrelated JNI calls on this thread.
                let _ = env.exception_clear();
                AndroidAdapterError::new("GameActivity.mSurfaceView", e)
            })?;
        // A resolved-but-null field is legal Java state (surface view not
        // yet created or already torn down); handing a null `JObject` to
        // `InjectingAdapter::new` would panic inside upstream JNI calls
        // with no context, so surface it as a structured error instead.
        if view.is_null() {
            return Err(AndroidAdapterError::null_surface_view());
        }
        let adapter = InjectingAdapter::new(&mut env, &view, activation_handler, action_handler);
        Ok(Self { adapter })
    }

    /// If and only if the accessibility tree has been initialized, calls
    /// `updater` and forwards the resulting [`TreeUpdate`] to the
    /// Android accessibility framework.
    ///
    /// See [`InjectingAdapter::update_if_active`].
    ///
    /// # Examples
    ///
    /// ```ignore
    /// # fn example(adapter: &mut martensite_access_platform::android::AndroidAdapter) {
    /// adapter.update_if_active(|| accesskit::TreeUpdate {
    ///     nodes: Vec::new(),
    ///     tree: None,
    ///     tree_id: accesskit::TreeId::ROOT,
    ///     focus: accesskit::NodeId(0),
    /// });
    /// # }
    /// ```
    pub fn update_if_active(&mut self, updater: impl FnOnce() -> TreeUpdate) {
        self.adapter.update_if_active(updater);
    }
}

impl std::fmt::Debug for AndroidAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AndroidAdapter").finish_non_exhaustive()
    }
}
