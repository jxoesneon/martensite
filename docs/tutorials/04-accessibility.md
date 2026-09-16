# Tutorial 4 — Accessibility validation

Martensite's accessibility layer is `martensite-access`, built on
[AccessKit](https://accesskit.dev): the widget tree is serialized into
AccessKit `TreeUpdate`s and handed to the platform-native adapter —
UIA on Windows, NSAccessibility on macOS, AT-SPI2 on Linux,
`accesskit_ios`/`accesskit_android` on mobile, and the
`WebA11yBridge` hidden-DOM mirror on wasm. This tutorial shows how to
build, inspect, and test that tree **without** a screen reader.

## 1. From arena to `TreeUpdate`

`AccessKitAdapter` walks the `WidgetArena`, calls each widget's
`Widget::accessibility` hook, and packages the result as a
`accesskit::TreeUpdate`:

```rust
use martensite_access::AccessKitAdapter;
use martensite_core::{ColdNode, HotNode, WidgetArena};

let mut arena = WidgetArena::new();
let root = arena.insert(
    HotNode::default(),
    ColdNode::new(Box::new(MyRootWidget::new()))
        .with_role(accesskit::Role::Window)
        .with_a11y_name("main window"),
);

let mut adapter = AccessKitAdapter::new(root);
let update: accesskit::TreeUpdate = adapter.build_update(&mut arena);

assert!(!update.nodes.is_empty());
```

`TreeUpdate` carries `nodes: Vec<(NodeId, Node)>`, the `TreeInfo`
(root id), `focus`, and `tree_id`. Widget `WidgetId`s map to `NodeId`s
by direct 64-bit reinterpretation (`widget_id_to_node_id`); internal
children and overlay popups get virtual ids minted from the
generation-0 space, resolvable via `adapter.resolve_internal` /
`adapter.resolve_overlay`.

Two emission modes:

- `build_update(&mut arena)` — full tree, every node.
- `build_incremental_update(&mut arena) -> Option<TreeUpdate>` — only
  `DIRTY_A11Y` nodes; `None` when nothing changed.

## 2. Asserting on the tree in tests

This is the primary no-screen-reader validation path — the same
technique `martensite-access`'s own tests use:

```rust
use martensite_access::AccessKitAdapter;
use martensite_core::{ColdNode, HotNode, WidgetArena};

let mut arena = WidgetArena::new();
let button = arena.insert(
    HotNode::default(),
    ColdNode::new(Box::new(martensite::widgets::Button::new("Save")))
        .with_role(accesskit::Role::Button),
);
let mut adapter = AccessKitAdapter::new(button);
let update = adapter.build_update(&mut arena);

let node = &update.nodes[0].1;
assert_eq!(node.role(), accesskit::Role::Button);
assert!(node.supports_action(accesskit::Action::Click));
assert_eq!(node.label(), Some("Save"));
```

What to assert for a widget audit:

- **Role** — `node.role()` (`Button`, `CheckBox`, `Slider`, `ListBox`,
  `Tab`, …). The stock widgets set these in `Widget::accessibility`.
- **Name** — `node.label()` reflects `set_label` /
  `ColdNode::with_a11y_name`.
- **Actions** — `node.supports_action(Action::Click/Focus/…)`; these
  are what a screen reader can invoke.
- **Relations** — `aria-activedescendant`/`aria-controls` are wired by
  `Widget::a11y_fixup` after descendant ids are minted (see the
  `Dropdown` widget for a complete example).

## 3. Simulating assistive-technology actions

AT activations enter the widget pipeline as `A11yAction`s decoded from
AccessKit `ActionRequest`s, and are dispatched as
`WidgetEvent::SemanticAction` through the normal event path:

```rust
use martensite_access::actions::{dispatch_a11y_action, A11yAction};
use martensite_core::{EventResponse, WidgetArena};

let mut arena = WidgetArena::new();
// ... build arena, adapter, initial update ...
let response = dispatch_a11y_action(
    &mut arena,
    &A11yAction::Click(button_id.into()),
);
assert_ne!(response, EventResponse::Ignored);
```

> **Verify before you assert.** The dispatch only produces a
> non-`Ignored` response if the target widget actually handles the
> action — `WidgetEvent::SemanticAction` flows through the same `match`
> in `Widget::event` as any other event. The `CounterBadge` from
> [Tutorial 3](03-custom-widget.md) returns `RequestRepaint` on
> `SemanticAction::Click`; a widget that does not match the variant
> returns `Ignored` and the AT activation is a no-op. When auditing a
> widget, assert on the *state change* (e.g. `badge.count() == 1`), not
> just the response.

Variants cover the AT vocabulary: `Click`, `Focus`, `Blur`,
`SetValue`, `Increment`/`Decrement` (sliders), `Expand`/`Collapse`
(disclosure), `ShowTooltip`/`HideTooltip`, `ShowContextMenu`, and
`Other` for anything else. Targets can be arena nodes
(`ActionTarget::Arena`), widget-internal virtual nodes
(`ActionTarget::Internal`), or overlay popup nodes
(`ActionTarget::Overlay`) — `dispatch_a11y_action` resolves all three.

So a screen-reader "activate" test for a custom widget is: dispatch
`A11yAction::Click` and assert the widget's state changed — no AT
required.

## 4. WCAG compliance helpers

`martensite_access::compliance` provides programmatic WCAG 2.2 checks:

```rust
use martensite_access::compliance::{
    check_target_size, check_text_contrast, check_ui_component_contrast,
    ColorRgba, TextSize, WcagLevel,
};

// 7:1 for AAA normal text, 4.5:1 for AAA large text.
assert!(check_text_contrast(
    ColorRgba::rgb(0.0, 0.0, 0.0),
    ColorRgba::rgb(1.0, 1.0, 1.0),
    TextSize::Normal,
    WcagLevel::Aaa,
));

// 3:1 for UI component boundaries.
assert!(check_ui_component_contrast(
    ColorRgba::rgb(0.4, 0.4, 0.4),
    ColorRgba::rgb(1.0, 1.0, 1.0),
));

// 24x24 CSS-pixel minimum target (WCAG 2.5.8).
assert!(check_target_size(44.0, 44.0));
```

Focus-appearance and Section 508 reporting live alongside:
`FocusAppearanceCheck`, `FocusAreaCheck`, `VpatReport`, and
`Section508VpatReport` (see `crates/martensite-access/tests/
v0_11_conformance.rs` for the full conformance harness — it generates a
*non-certifying* VPAT document; a real Section 508 claim still needs a
specialist pass).

## 5. Wiring to a real platform

For a live application, `MartensiteAccessBridge`
(`martensite_access::winit`) is the `parking_lot::Mutex`-guarded owner
of the arena + adapter pair; it implements AccessKit's
`ActivationHandler`/`ActionHandler`/`DeactivationHandler`, so the
platform adapter (`martensite-accesskit-winit`, a vendored
`accesskit_winit` patched for winit 0.31) can request initial trees and
deliver actions:

```rust
use martensite_access::winit::MartensiteAccessBridge;
use martensite_access::AccessKitAdapter;
use martensite_core::WidgetArena;

let arena = WidgetArena::new();
let adapter = AccessKitAdapter::new(root);
let bridge = std::sync::Arc::new(MartensiteAccessBridge::new(arena, adapter));

// Per frame:
bridge.tick(std::time::Duration::from_millis(16));
let actions = bridge.process_pending_actions();
let update = bridge.build_incremental_update(); // Option<TreeUpdate>
```

## 6. Platform coverage and its limits

| Platform | Adapter | Verification status |
| --- | --- | --- |
| Windows | UIA via `accesskit_windows` | CI build-verified |
| macOS | NSAccessibility via `accesskit_macos` | CI build-verified |
| Linux | AT-SPI2 via `accesskit_unix` | CI build-verified |
| iOS | `accesskit_ios` `SubclassingAdapter` | runtime-verified on the iOS simulator (`MARTENSITE_IOS_SIM_TESTS` adapter smoke test) |
| Android | `accesskit_android` `InjectingAdapter` (GameActivity only) | compile-verified; device gate attempted and descoped (needs a GameActivity APK harness — see `PLATFORM_SUPPORT.md`) |
| Web | `WebA11yBridge` hidden-DOM/ARIA mirror | minimal bridge — **not** a full AccessKit adapter; headless-Chromium smoke gate passed (`MARTENSITE_WEB_BROWSER`) |

What these tests *cannot* tell you: real screen-reader UX (focus
order, announcement timing, verbosity). The AT-harness legs
(NVDA/VoiceOver/Orca) are env-gated and tracked in `WORKING_ON.md`;
`PLATFORM_SUPPORT.md` records exactly which legs have run. Test with a
real screen reader before shipping claims of AT compatibility.

## See also

- `docs/PLATFORM_SUPPORT.md` — verified vs compile-verified status
- `crates/martensite-access/tests/v0_11_conformance.rs` — the
  conformance harness
