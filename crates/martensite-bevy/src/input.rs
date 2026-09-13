//! `EngineEvent` → Bevy input mapping.
//!
//! The host calls
//! [`Engine::on_event`](martensite_engine_bridge::Engine::on_event) on the
//! paint thread; [`BevyEngine`](crate::BevyEngine) pushes each event onto a
//! channel whose receiver lives in the app as `EngineEventQueue`. The
//! `PreUpdate` system `drain_engine_events` converts every queued event
//! into the corresponding Bevy messages:
//!
//! | `EngineEvent`     | Bevy messages written |
//! |-------------------|-----------------------|
//! | `PointerMove`     | `PointerInput` (`Move`, picking) |
//! | `PointerButton`   | `PointerInput` (`Press`/`Release`, picking) + `MouseButtonInput` (`ButtonInput<MouseButton>`) |
//! | `Scroll`          | `PointerInput` (`Scroll`, picking) + `MouseWheel` (`AccumulatedMouseScroll`) |
//! | `Key`             | `KeyboardInput` (`ButtonInput<KeyCode>`/`ButtonInput<Key>`) |
//! | `TextInput`       | `KeyboardInput` pressed+released pair carrying `text`/`logical_key` |
//! | `Focus`           | `WindowFocused`; on focus loss also `KeyboardFocusLost` (clears `ButtonInput` caches) |
//!
//! Pointer locations target `NormalizedRenderTarget::TextureView` of the
//! engine camera's
//! [`ManualTextureViewHandle`](bevy::camera::ManualTextureViewHandle).
//! Positions are surface-local
//! physical pixels; a `ManualTextureView` has no scale factor, so they map
//! 1:1 onto `Camera::logical_viewport_rect`.

use std::sync::Mutex;
use std::sync::mpsc::Receiver;

use bevy::app::{App, Plugin, PreUpdate};
use bevy::camera::{ManualTextureViewHandle, NormalizedRenderTarget};
use bevy::ecs::entity::Entity;
use bevy::ecs::message::MessageWriter;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Local, Res};
use bevy::input::keyboard::{
    Key, KeyCode, KeyboardFocusLost, KeyboardInput, NativeKey, NativeKeyCode,
};
use bevy::input::mouse::{MouseButton, MouseButtonInput, MouseScrollUnit, MouseWheel};
use bevy::input::touch::TouchPhase;
use bevy::input::{ButtonState, InputSystems};
use bevy::math::Vec2;
use bevy::picking::PickingSystems;
use bevy::picking::pointer::{Location, PointerAction, PointerInput};
use bevy::window::WindowFocused;
use martensite_engine_bridge::{EngineEvent, PointerButton};

/// Receiver half of the engine's input channel, installed as a resource.
///
/// `Receiver` is `Send` but not `Sync`, so it sits behind a `Mutex` — the
/// drain system is the only consumer.
#[derive(Resource)]
struct EngineEventQueue(Mutex<Receiver<EngineEvent>>);

/// The render target pointer locations are reported against.
#[derive(Resource)]
struct PointerTarget(ManualTextureViewHandle);

/// Routes queued [`EngineEvent`]s into Bevy's input pipeline.
///
/// [`BevyEngine`](crate::BevyEngine) adds this plugin itself; it is public so
/// hosts that assemble their own headless app (rather than going through
/// `BevyEngine`) can reuse the same event→message mapping. The drain runs in
/// `PreUpdate` before both [`InputSystems`] (so `ButtonInput` resources
/// reflect events the same frame) and [`PickingSystems::ProcessInput`] (so
/// `bevy_picking` consumes `PointerInput` the same frame).
///
/// # Examples
///
/// ```
/// use std::sync::mpsc::channel;
/// use martensite_bevy::input::MartensiteInputPlugin;
/// use martensite_bevy::CAMERA_VIEW_HANDLE;
/// use martensite_engine_bridge::EngineEvent;
///
/// let (tx, rx) = channel::<EngineEvent>();
/// let plugin = MartensiteInputPlugin::new(rx, CAMERA_VIEW_HANDLE);
/// tx.send(EngineEvent::Focus { focused: true }).unwrap();
/// ```
pub struct MartensiteInputPlugin {
    rx: Mutex<Option<Receiver<EngineEvent>>>,
    target: ManualTextureViewHandle,
}

impl MartensiteInputPlugin {
    /// Creates the plugin draining `rx` and reporting pointer locations
    /// against `target`.
    ///
    /// `rx` is consumed when the plugin is built into an [`App`]; sending
    /// `EngineEvent`s on the paired `Sender` afterwards feeds them to the
    /// app's input pipeline.
    ///
    /// [`App`]: bevy::app::App
    pub fn new(rx: Receiver<EngineEvent>, target: ManualTextureViewHandle) -> Self {
        Self {
            rx: Mutex::new(Some(rx)),
            target,
        }
    }
}

impl Plugin for MartensiteInputPlugin {
    fn build(&self, app: &mut App) {
        let rx = self
            .rx
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
            .expect("MartensiteInputPlugin builds once");
        app.insert_resource(EngineEventQueue(Mutex::new(rx)))
            .insert_resource(PointerTarget(self.target))
            .add_systems(
                PreUpdate,
                drain_engine_events
                    .before(InputSystems)
                    .before(PickingSystems::ProcessInput),
            );
    }
}

/// The `PreUpdate` drain: pulls every queued [`EngineEvent`] and writes the
/// mapped Bevy messages.
#[expect(
    clippy::too_many_arguments,
    reason = "one writer per produced message type"
)]
fn drain_engine_events(
    queue: Res<EngineEventQueue>,
    target: Res<PointerTarget>,
    mut last_position: Local<Option<Vec2>>,
    mut pointer: MessageWriter<PointerInput>,
    mut mouse_buttons: MessageWriter<MouseButtonInput>,
    mut wheel: MessageWriter<MouseWheel>,
    mut keys: MessageWriter<KeyboardInput>,
    mut focus: MessageWriter<WindowFocused>,
    mut focus_lost: MessageWriter<KeyboardFocusLost>,
) {
    let rx = queue.0.lock().unwrap_or_else(|e| e.into_inner());
    while let Ok(event) = rx.try_recv() {
        let location = |position: [f32; 2]| Location {
            target: NormalizedRenderTarget::TextureView(target.0),
            position: Vec2::new(position[0], position[1]),
        };
        match event {
            EngineEvent::PointerMove { position } => {
                let pos = Vec2::new(position[0], position[1]);
                let delta = last_position.map_or(Vec2::ZERO, |last| pos - last);
                *last_position = Some(pos);
                pointer.write(PointerInput::new(
                    bevy::picking::pointer::PointerId::Mouse,
                    location(position),
                    PointerAction::Move { delta },
                ));
            }
            EngineEvent::PointerButton {
                position,
                button,
                pressed,
            } => {
                *last_position = Some(Vec2::new(position[0], position[1]));
                // Picking only knows the three primary buttons; every button
                // still reaches `ButtonInput<MouseButton>`.
                if let Some(button) = picking_button(button) {
                    pointer.write(PointerInput::new(
                        bevy::picking::pointer::PointerId::Mouse,
                        location(position),
                        if pressed {
                            PointerAction::Press(button)
                        } else {
                            PointerAction::Release(button)
                        },
                    ));
                }
                mouse_buttons.write(MouseButtonInput {
                    button: mouse_button(button),
                    state: button_state(pressed),
                    window: Entity::PLACEHOLDER,
                });
            }
            EngineEvent::Scroll { position, delta } => {
                *last_position = Some(Vec2::new(position[0], position[1]));
                // The bridge contract is physical pixels, so scroll deltas
                // map to `MouseScrollUnit::Pixel` on both paths.
                pointer.write(PointerInput::new(
                    bevy::picking::pointer::PointerId::Mouse,
                    location(position),
                    PointerAction::Scroll {
                        unit: MouseScrollUnit::Pixel,
                        x: delta[0],
                        y: delta[1],
                        phase: TouchPhase::Moved,
                    },
                ));
                wheel.write(MouseWheel {
                    unit: MouseScrollUnit::Pixel,
                    x: delta[0],
                    y: delta[1],
                    window: Entity::PLACEHOLDER,
                    phase: TouchPhase::Moved,
                });
            }
            EngineEvent::Key { scancode, pressed } => {
                keys.write(KeyboardInput {
                    key_code: KeyCode::Unidentified(native_key_code(scancode)),
                    logical_key: Key::Unidentified(native_key(scancode)),
                    state: button_state(pressed),
                    text: None,
                    repeat: false,
                    window: Entity::PLACEHOLDER,
                });
            }
            EngineEvent::TextInput { text } => {
                // Committed text arrives as a synthetic press+release pair so
                // `ButtonInput` never latches a phantom "pressed" key; the
                // composed string rides on `text`/`logical_key` of the press.
                keys.write(KeyboardInput {
                    key_code: KeyCode::Unidentified(NativeKeyCode::Unidentified),
                    logical_key: Key::Character(text.clone().into()),
                    state: ButtonState::Pressed,
                    text: Some(text.into()),
                    repeat: false,
                    window: Entity::PLACEHOLDER,
                });
                keys.write(KeyboardInput {
                    key_code: KeyCode::Unidentified(NativeKeyCode::Unidentified),
                    logical_key: Key::Unidentified(NativeKey::Unidentified),
                    state: ButtonState::Released,
                    text: None,
                    repeat: false,
                    window: Entity::PLACEHOLDER,
                });
            }
            EngineEvent::Focus { focused } => {
                focus.write(WindowFocused {
                    window: Entity::PLACEHOLDER,
                    focused,
                });
                if !focused {
                    // Same recovery path as `bevy_winit`: clears `ButtonInput`
                    // caches so keys/buttons don't stick while unfocused.
                    focus_lost.write(KeyboardFocusLost);
                }
            }
            // `EngineEvent` is `#[non_exhaustive]` — newer kinds are ignored.
            _ => {}
        }
    }
}

/// `PointerButton` → `bevy_picking`'s three-button
/// [`PointerButton`](bevy::picking::pointer::PointerButton), or `None` for
/// buttons picking has no representation of (`Back`/`Forward`/`Other` still
/// reach `ButtonInput<MouseButton>`).
fn picking_button(button: PointerButton) -> Option<bevy::picking::pointer::PointerButton> {
    match button {
        PointerButton::Primary => Some(bevy::picking::pointer::PointerButton::Primary),
        PointerButton::Secondary => Some(bevy::picking::pointer::PointerButton::Secondary),
        PointerButton::Middle => Some(bevy::picking::pointer::PointerButton::Middle),
        PointerButton::Back | PointerButton::Forward | PointerButton::Other(_) => None,
    }
}

/// `PointerButton` → `bevy_input`'s [`MouseButton`] — total mapping, all five
/// buttons carry through.
fn mouse_button(button: PointerButton) -> MouseButton {
    match button {
        PointerButton::Primary => MouseButton::Left,
        PointerButton::Secondary => MouseButton::Right,
        PointerButton::Middle => MouseButton::Middle,
        PointerButton::Back => MouseButton::Back,
        PointerButton::Forward => MouseButton::Forward,
        PointerButton::Other(n) => MouseButton::Other(n),
    }
}

fn button_state(pressed: bool) -> ButtonState {
    if pressed {
        ButtonState::Pressed
    } else {
        ButtonState::Released
    }
}

/// Best-effort platform scancode → [`NativeKeyCode`].
///
/// `EngineEvent::Key` carries the platform scancode (winit convention:
/// Windows scan code, macOS virtual keycode, evdev/XKB keycode elsewhere).
/// We cannot recover a layout-independent [`KeyCode`] from it, so keys arrive
/// as [`KeyCode::Unidentified`] with the platform-specific code preserved —
/// press/release pairing still works because the code is stable per key.
fn native_key_code(scancode: u32) -> NativeKeyCode {
    #[cfg(target_os = "windows")]
    {
        NativeKeyCode::Windows(scancode as u16)
    }
    #[cfg(target_os = "macos")]
    {
        NativeKeyCode::MacOS(scancode as u16)
    }
    #[cfg(target_os = "android")]
    {
        NativeKeyCode::Android(scancode)
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "android")))]
    {
        NativeKeyCode::Xkb(scancode)
    }
}

/// The logical-key counterpart of [`native_key_code`].
fn native_key(scancode: u32) -> NativeKey {
    #[cfg(target_os = "windows")]
    {
        NativeKey::Windows(scancode as u16)
    }
    #[cfg(target_os = "macos")]
    {
        NativeKey::MacOS(scancode as u16)
    }
    #[cfg(target_os = "android")]
    {
        NativeKey::Android(scancode)
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "android")))]
    {
        NativeKey::Xkb(scancode)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picking_button_covers_primary_buttons_only() {
        assert_eq!(
            picking_button(PointerButton::Primary),
            Some(bevy::picking::pointer::PointerButton::Primary)
        );
        assert_eq!(
            picking_button(PointerButton::Secondary),
            Some(bevy::picking::pointer::PointerButton::Secondary)
        );
        assert_eq!(
            picking_button(PointerButton::Middle),
            Some(bevy::picking::pointer::PointerButton::Middle)
        );
        assert_eq!(picking_button(PointerButton::Back), None);
        assert_eq!(picking_button(PointerButton::Forward), None);
        assert_eq!(picking_button(PointerButton::Other(9)), None);
    }

    #[test]
    fn mouse_button_mapping_is_total() {
        assert_eq!(mouse_button(PointerButton::Primary), MouseButton::Left);
        assert_eq!(mouse_button(PointerButton::Secondary), MouseButton::Right);
        assert_eq!(mouse_button(PointerButton::Middle), MouseButton::Middle);
        assert_eq!(mouse_button(PointerButton::Back), MouseButton::Back);
        assert_eq!(mouse_button(PointerButton::Forward), MouseButton::Forward);
        assert_eq!(
            mouse_button(PointerButton::Other(42)),
            MouseButton::Other(42)
        );
    }

    #[test]
    fn native_key_code_preserves_platform_scancode() {
        let code = native_key_code(30);
        // Every platform variant preserves the raw scancode value.
        let preserved = match code {
            NativeKeyCode::Windows(c) => u32::from(c),
            NativeKeyCode::MacOS(c) => u32::from(c),
            NativeKeyCode::Android(c) | NativeKeyCode::Xkb(c) => c,
            NativeKeyCode::Unidentified => u32::MAX,
        };
        assert_eq!(preserved, 30);
    }
}
