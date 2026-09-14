//! Android device/emulator acceptance gate — AccessKit adapter injection.
//!
//! This is the v0.17.0 §5 Android gate artifact ("Example APK runs;
//! GameActivity IME works; AccessKit injects"), mirroring the
//! `full_rate_4k120_gate` pattern: `#[ignore]`-gated and additionally
//! env-gated on `MARTENSITE_ANDROID_DEVICE=1`, so `cargo test -- --ignored`
//! on a CI host still skips cleanly.
//!
//! Run it on-device under a GameActivity process, e.g. via `cargo apk test`
//! (cargo-apk2) or `x test` (xbuild). The gate body mirrors
//! `android::AndroidAdapter::new` step-for-step — JavaVM resolution,
//! `GameActivity.mSurfaceView` (`InputEnabledSurfaceView`) lookup, and
//! `InjectingAdapter` construction — so a pass proves the exact path the
//! production adapter takes. `android-activity` publishes the JavaVM and
//! the activity's global `jobject` through `ndk-context` at process start,
//! so an on-device test binary recovers them without winit.
//!
//! Host/desktop builds compile this file but the gate body is a documented
//! no-op; see `docs/android-packaging.md` §7 for the tracking status.

// The gate performs the same raw JNI pointer recovery as the crate's
// `android` module; the package's audited-unsafe scope covers it.
#![allow(unsafe_code)]

#[test]
#[ignore = "requires a GameActivity device/emulator — set MARTENSITE_ANDROID_DEVICE=1"]
fn android_accesskit_injection_gate() {
    if std::env::var_os("MARTENSITE_ANDROID_DEVICE").is_none() {
        eprintln!("MARTENSITE_ANDROID_DEVICE not set; gate skipped");
        return;
    }
    #[cfg(target_os = "android")]
    gate::run();
    #[cfg(not(target_os = "android"))]
    eprintln!("android AccessKit gate: only meaningful on-device; skipped");
}

#[cfg(target_os = "android")]
mod gate {
    use accesskit::{ActionHandler, ActionRequest, ActivationHandler, NodeId, TreeId, TreeUpdate};
    use accesskit_android::{
        jni::{objects::JObject, JavaVM},
        InjectingAdapter,
    };

    /// JNI signature of `GameActivity.mSurfaceView` — the same field
    /// `android::AndroidAdapter::new` resolves before injecting the
    /// AccessKit delegate.
    const GAME_ACTIVITY_SURFACE_VIEW_SIGNATURE: &str =
        "Lcom/google/androidgamesdk/GameActivity$InputEnabledSurfaceView;";

    /// Activation handler that reports "no accessibility tree", the
    /// minimal input `InjectingAdapter` accepts before any tree exists.
    struct NoopActivation;

    impl ActivationHandler for NoopActivation {
        fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
            None
        }
    }

    /// Action handler that drops requests — the gate asserts injection,
    /// not action routing (routing is covered by `martensite-access`
    /// bridge tests).
    struct NoopAction;

    impl ActionHandler for NoopAction {
        fn do_action(&mut self, _request: ActionRequest) {}
    }

    /// The real gate body, compiled only for on-device test runs.
    ///
    /// # Panics
    ///
    /// Panics — i.e. the gate fails — when any step of the production
    /// injection path fails: missing JavaVM/context, a `mSurfaceView`
    /// field that does not resolve (the NativeActivity signature), a
    /// null surface view, or an injection failure inside
    /// `InjectingAdapter::new`.
    pub fn run() {
        let ctx = ndk_context::android_context();
        assert!(!ctx.vm().is_null(), "ndk-context JavaVM must be set");
        assert!(
            !ctx.context().is_null(),
            "ndk-context activity jobject must be set"
        );

        // SAFETY: `ndk_context::android_context().vm()` is the
        // process-wide `JavaVM*` published by `android-activity` at
        // startup; it outlives the activity and is valid for the entire
        // process lifetime.
        let vm = unsafe { JavaVM::from_raw(ctx.vm().cast()) }.expect("JavaVM::from_raw");
        let mut env = vm.get_env().expect("JNI env for the test thread");

        // SAFETY: `android_context().context()` is the activity's global
        // `jobject` reference published alongside the VM; valid for the
        // activity lifetime, which contains the test run.
        let activity = unsafe { JObject::from_raw(ctx.context().cast()) };

        let view = env
            .get_field(
                &activity,
                "mSurfaceView",
                GAME_ACTIVITY_SURFACE_VIEW_SIGNATURE,
            )
            .and_then(|value| value.l())
            .unwrap_or_else(|e| {
                let _ = env.exception_clear();
                panic!(
                    "GameActivity.mSurfaceView did not resolve ({e}) — \
                     is this process running under NativeActivity?"
                );
            });
        assert!(!view.is_null(), "GameActivity surface view must exist");

        // Full injection: this is the call `AndroidAdapter::new` ends in.
        // `InjectingAdapter` services `AccessibilityNodeInfo` requests on
        // the UI thread, so constructing it here proves delegate
        // injection works on this device.
        let mut adapter = InjectingAdapter::new(&mut env, &view, NoopActivation, NoopAction);
        // An empty, well-formed update: no nodes, no tree structure, root
        // tree id, focus on the (unused) zero node.
        adapter.update_if_active(|| TreeUpdate {
            nodes: Vec::new(),
            tree: None,
            tree_id: TreeId::ROOT,
            focus: NodeId(0),
        });
    }
}
