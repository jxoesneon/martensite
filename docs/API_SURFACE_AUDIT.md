# API Surface Audit — v0.18.0

Machine-enumerated public API surface of every publishable Martensite
crate, with a STABLE / EXPERIMENTAL / VENDORED classification per item.
Policy context lives in [`API_FREEZE_AUDIT.md`](API_FREEZE_AUDIT.md);
deprecation and MSRV rules in [`DEPRECATION_POLICY.md`](DEPRECATION_POLICY.md).

## 1. Methodology

`python3 scripts/api_surface_audit.py` regenerates the section between the
`GENERATED` markers below:

1. `cargo metadata` selects the publishable crates — workspace members
   under `crates/` without `publish = false` (33 crates). Excluded:
   `martensite-bevy`, `martensite-godot`, `martensite-media-test`,
   `martensite-render-test`, `martensite-text-reference` (`publish =
   false`), and everything under `tools/`, `examples/`, `benches/`,
   `stubs/`.
2. Each crate is documented twice with `cargo doc --no-deps` emitting
   rustdoc JSON (`RUSTC_BOOTSTRAP=1` unlocks the unstable format on the
   stable toolchain — used for enumeration only):
   - a **default-features** pass, and
   - a **feature** pass enabling the crate's optional public features
     (`EXTRA_FEATURES` in the script) so feature-gated items are counted
     and tagged `feature …`.
3. Items are collected from the JSON `paths` table (canonical paths of
   defined items) plus public `use` re-exports found by walking the module
   tree — re-exports are the nameable surface of umbrella crates like
   `martensite`, and their source paths feed classification.
4. Classification:
   - **VENDORED** — entire crate tracks an upstream project
     (`martensite-vello`, `martensite-cosmic-text`,
     `martensite-accesskit-winit`).
   - **EXPERIMENTAL** — whole-crate (`martensite-access-platform`,
     `martensite-engine-bridge`, `martensite-host`) or per-path regex
     rules (`EXPERIMENTAL_RULES` in the script), each carrying a rationale
     rendered in the per-crate sections.
   - **STABLE** — everything else.

### Caveats

- The enumeration host is macOS; `cfg(target_os)`-gated items for other
  platforms are absent from the JSON. The affected crates are noted where
  their counts are conspicuously small (`martensite-access-platform`:
  entirely cfg-gated; `martensite-font-fallback`,
  `martensite-clipboard-platform`: per-OS provider types).
- `decoder-ffmpeg` and `decoder-vaapi` feature items are not enumerable on
  this host (system ffmpeg/libva); they live inside modules already
  classified EXPERIMENTAL, so no STABLE item is missed.
- `pub` items inside private modules are unreachable and therefore absent,
  matching the semver-relevant surface.
- Methods and fields are not listed; the item-level surface
  (types/traits/functions/constants/modules/macros) is what
  `cargo-semver-checks` enforces names against.
- `#[doc(hidden)]` items are stripped by rustdoc before JSON emission, so
  they are absent from this enumeration entirely — counted under no tier.
  They remain nameable public API: downstream code can still call them and
  `cargo-semver-checks` still lints them, so a `#[doc(hidden)]` item
  carries **no stability guarantee** regardless of classification. At
  audit time the only uses are test-introspection methods on
  `martensite-reactive`'s scheduler (`test_node_count`, etc. — below the
  item granularity enumerated here anyway) and upstream attributes inside
  the vendored `martensite-vello` fork (already VENDORED wholesale). No
  STABLE top-level item is hidden.

### Stability gating (reconciling milestone spec §4.1)

Milestone v0.18.0 §4.1 asks to "gate every experimental item behind an
explicit flag". The recorded convention — rationale in
[`API_FREEZE_AUDIT.md`](API_FREEZE_AUDIT.md) §3 — is:

- **On the 0.x line, EXPERIMENTAL is a classification, not a flag.** It
  marks surface that may change across minor bumps (a freedom SemVer §4
  already grants to all of 0.x). Mechanical gating exists only where an
  opt-in Cargo feature already exposes the items — those carry a
  `feature …` tag in the lists below. The remaining EXPERIMENTAL items
  (see the generated totals below) rely on the documented classification
  plus the changelog-notice rules in `DEPRECATION_POLICY.md` §1/§3; per-item
  `unstable-*` gates pre-1.0 would duplicate a guarantee SemVer already
  withholds, without additional enforcement.
- **Post-1.0, incubating API ships behind an explicit opt-in gate.**
  `governance/GOVERNANCE.md` §3.2 (Tier 2) defines the mechanism as the
  `#[cfg(martensite_unstable)]` compile-time cfg — consumers enable it
  via `RUSTFLAGS="--cfg martensite_unstable"` — with RFC Phase 5
  additionally permitting an opt-in Cargo feature. The freeze audit
  records the concrete post-1.0 form as `unstable-*` Cargo feature flags
  (`API_FREEZE_AUDIT.md` §3), which satisfies the charter's "opt-in
  Cargo feature" alternative.
- **Neither mechanism exists in the tree today**: there are zero
  `unstable-*` Cargo features and zero `#[cfg(martensite_unstable)]`
  uses. Both are post-1.0 conventions recorded in advance so the
  stabilization gate is mechanical rather than retroactive.

## 2. Enumerated Surface

<!-- BEGIN GENERATED: api-surface -->

_33 publishable crates. Generated by `python3 scripts/api_surface_audit.py` from rustdoc JSON (stable + `RUSTC_BOOTSTRAP`); do not edit between the markers._

**Workspace total: 1589 public items, 1240 stable / 210 experimental / 139 vendored.**

| Crate | Items | Stable | Experimental | Vendored | Notes |
| :--- | ---: | ---: | ---: | ---: | :--- |
| `martensite` | 162 | 157 | 5 | 0 |  |
| `martensite-access` | 104 | 104 | 0 | 0 |  |
| `martensite-access-platform` | 0 | 0 | 0 | 0 | crate-level experimental |
| `martensite-accesskit-winit` | 1 | 0 | 0 | 1 | vendored fork |
| `martensite-assets` | 33 | 31 | 2 | 0 |  |
| `martensite-blessed` | 58 | 24 | 34 | 0 |  |
| `martensite-clipboard` | 29 | 29 | 0 | 0 |  |
| `martensite-clipboard-platform` | 4 | 4 | 0 | 0 |  |
| `martensite-core` | 96 | 89 | 7 | 0 |  |
| `martensite-cosmic-text` | 89 | 0 | 0 | 89 | vendored fork |
| `martensite-devtools` | 25 | 18 | 7 | 0 |  |
| `martensite-dnd` | 49 | 49 | 0 | 0 |  |
| `martensite-engine-bridge` | 47 | 0 | 47 | 0 | crate-level experimental |
| `martensite-focus` | 22 | 22 | 0 | 0 |  |
| `martensite-font-fallback` | 3 | 3 | 0 | 0 |  |
| `martensite-history` | 17 | 17 | 0 | 0 |  |
| `martensite-host` | 4 | 0 | 4 | 0 | crate-level experimental |
| `martensite-l10n` | 11 | 11 | 0 | 0 |  |
| `martensite-layout` | 56 | 56 | 0 | 0 |  |
| `martensite-macros` | 1 | 1 | 0 | 0 |  |
| `martensite-media` | 88 | 65 | 23 | 0 |  |
| `martensite-media-platform` | 33 | 18 | 15 | 0 |  |
| `martensite-motion` | 27 | 27 | 0 | 0 |  |
| `martensite-plugin` | 31 | 14 | 17 | 0 |  |
| `martensite-reactive` | 71 | 58 | 13 | 0 |  |
| `martensite-render` | 38 | 38 | 0 | 0 |  |
| `martensite-shell` | 29 | 24 | 5 | 0 |  |
| `martensite-test` | 36 | 36 | 0 | 0 |  |
| `martensite-text` | 142 | 142 | 0 | 0 |  |
| `martensite-theme` | 45 | 45 | 0 | 0 |  |
| `martensite-vello` | 49 | 0 | 0 | 49 | vendored fork |
| `martensite-wgpu` | 87 | 65 | 22 | 0 |  |
| `martensite-window` | 102 | 93 | 9 | 0 |  |

### `martensite` (v0.17.0)

**162 public items** — 157 stable, 5 experimental. Categories: 3 constants, 9 enums, 18 modules, 109 re-exports, 23 structs.

Experimental areas: decoder pipeline added in v0.16.0; external-engine widget embedding (v0.17.0); umbrella re-exports of experimental subsystems.

Large surface — digest by module (> 160 items):

| Module | Items | Stable | Experimental | Kinds |
| :--- | ---: | ---: | ---: | :--- |
| `(crate root)` | 20 | 20 | 0 | 3 modules, 17 re-exports |
| `app` | 4 | 4 | 0 | 1 constants, 3 structs |
| `prelude` | 64 | 64 | 0 | 64 re-exports |
| `widgets` | 74 | 69 | 5 | 2 constants, 9 enums, 15 modules, 28 re-exports, 20 structs |

<details><summary>Full item list</summary>

- `martensite::access` — re-exports, stable
- `martensite::app` — modules, stable
- `martensite::app::App` — structs, stable
- `martensite::app::AppBuilder` — structs, stable
- `martensite::app::AppConfig` — structs, stable
- `martensite::app::DEFAULT_FALLBACK_TIMEOUT` — constants, stable
- `martensite::clipboard` — re-exports, stable
- `martensite::core` — re-exports, stable
- `martensite::dnd` — re-exports, stable
- `martensite::focus` — re-exports, stable
- `martensite::history` — re-exports, stable
- `martensite::l10n` — re-exports, stable
- `martensite::layout` — re-exports, stable
- `martensite::macros` — re-exports, stable
- `martensite::media` — re-exports, stable
- `martensite::motion` — re-exports, stable
- `martensite::prelude` — modules, stable
- `martensite::prelude::App` — re-exports, stable
- `martensite::prelude::AppBuilder` — re-exports, stable
- `martensite::prelude::AppConfig` — re-exports, stable
- `martensite::prelude::BindError` — re-exports, stable
- `martensite::prelude::BridgeHandle` — re-exports, stable
- `martensite::prelude::BridgeRegistry` — re-exports, stable
- `martensite::prelude::ColdNode` — re-exports, stable
- `martensite::prelude::ColorRange` — re-exports, stable
- `martensite::prelude::ColorSpace` — re-exports, stable
- `martensite::prelude::Container` — re-exports, stable
- `martensite::prelude::DisplayProfile` — re-exports, stable
- `martensite::prelude::Dropdown` — re-exports, stable
- `martensite::prelude::Engine` — re-exports, stable
- `martensite::prelude::EngineContext` — re-exports, stable
- `martensite::prelude::EngineEvent` — re-exports, stable
- `martensite::prelude::EventContext` — re-exports, stable
- `martensite::prelude::EventResponse` — re-exports, stable
- `martensite::prelude::ExternalEngine` — re-exports, stable
- `martensite::prelude::ExternalEngines` — re-exports, stable
- `martensite::prelude::Flex` — re-exports, stable
- `martensite::prelude::FlexDirection` — re-exports, stable
- `martensite::prelude::Frame` — re-exports, stable
- `martensite::prelude::FramePoll` — re-exports, stable
- `martensite::prelude::FrameSync` — re-exports, stable
- `martensite::prelude::FrameToken` — re-exports, stable
- `martensite::prelude::HardwareHandle` — re-exports, stable
- `martensite::prelude::HotNode` — re-exports, stable
- `martensite::prelude::MediaView` — re-exports, stable
- `martensite::prelude::Memo` — re-exports, stable
- `martensite::prelude::NodeFlags` — re-exports, stable
- `martensite::prelude::Oklab` — re-exports, stable
- `martensite::prelude::OverlayAnchor` — re-exports, stable
- `martensite::prelude::OverlayLayer` — re-exports, stable
- `martensite::prelude::PaintContext` — re-exports, stable
- `martensite::prelude::PaintList` — re-exports, stable
- `martensite::prelude::PointerButton` — re-exports, stable
- `martensite::prelude::RadioGroup` — re-exports, stable
- `martensite::prelude::Rect` — re-exports, stable
- `martensite::prelude::ScRgb` — re-exports, stable
- `martensite::prelude::ScrollView` — re-exports, stable
- `martensite::prelude::SemanticAction` — re-exports, stable
- `martensite::prelude::Signal` — re-exports, stable
- `martensite::prelude::Slider` — re-exports, stable
- `martensite::prelude::SliderOrientation` — re-exports, stable
- `martensite::prelude::SourceAlpha` — re-exports, stable
- `martensite::prelude::SpringConfig` — re-exports, stable
- `martensite::prelude::SpringSolver` — re-exports, stable
- `martensite::prelude::Stack` — re-exports, stable
- `martensite::prelude::SurfaceId` — re-exports, stable
- `martensite::prelude::TabActivation` — re-exports, stable
- `martensite::prelude::Tabs` — re-exports, stable
- `martensite::prelude::Text` — re-exports, stable
- `martensite::prelude::ToneMapOperator` — re-exports, stable
- `martensite::prelude::Tooltip` — re-exports, stable
- `martensite::prelude::TransferFunction` — re-exports, stable
- `martensite::prelude::VideoFit` — re-exports, stable
- `martensite::prelude::VideoFrameMetadata` — re-exports, stable
- `martensite::prelude::VideoPixelFormat` — re-exports, stable
- `martensite::prelude::VideoSurface` — re-exports, stable
- `martensite::prelude::Viewport` — re-exports, stable
- `martensite::prelude::Widget` — re-exports, stable
- `martensite::prelude::WidgetArena` — re-exports, stable
- `martensite::prelude::WidgetEvent` — re-exports, stable
- `martensite::prelude::WidgetId` — re-exports, stable
- `martensite::reactive` — re-exports, stable
- `martensite::render` — re-exports, stable
- `martensite::text` — re-exports, stable
- `martensite::theme` — re-exports, stable
- `martensite::wgpu` — re-exports, stable
- `martensite::widgets` — modules, stable
- `martensite::widgets::BindError` — re-exports, stable
- `martensite::widgets::Button` — re-exports, stable
- `martensite::widgets::CheckBox` — re-exports, stable
- `martensite::widgets::Container` — re-exports, stable
- `martensite::widgets::DEFAULT_TOOLTIP_DELAY_MS` — re-exports, stable
- `martensite::widgets::Dropdown` — re-exports, stable
- `martensite::widgets::ExternalEngine` — re-exports, stable
- `martensite::widgets::ExternalEngines` — re-exports, stable
- `martensite::widgets::Flex` — re-exports, stable
- `martensite::widgets::FlexDirection` — re-exports, stable
- `martensite::widgets::FramePoll` — re-exports, stable
- `martensite::widgets::MediaView` — re-exports, stable
- `martensite::widgets::RadioGroup` — re-exports, stable
- `martensite::widgets::RadioOption` — re-exports, stable
- `martensite::widgets::ScrollBarWidget` — re-exports, stable
- `martensite::widgets::ScrollView` — re-exports, stable
- `martensite::widgets::Slider` — re-exports, stable
- `martensite::widgets::SliderOrientation` — re-exports, stable
- `martensite::widgets::Stack` — re-exports, stable
- `martensite::widgets::TOOLTIP_HOVER_GRACE_MS` — re-exports, stable
- `martensite::widgets::TabActivation` — re-exports, stable
- `martensite::widgets::TabItem` — re-exports, stable
- `martensite::widgets::Tabs` — re-exports, stable
- `martensite::widgets::Text` — re-exports, stable
- `martensite::widgets::TextInput` — re-exports, stable
- `martensite::widgets::Tooltip` — re-exports, stable
- `martensite::widgets::TooltipBubble` — re-exports, stable
- `martensite::widgets::VideoFit` — re-exports, stable
- `martensite::widgets::button` — modules, stable
- `martensite::widgets::button::Button` — structs, stable
- `martensite::widgets::checkbox` — modules, stable
- `martensite::widgets::checkbox::CheckBox` — structs, stable
- `martensite::widgets::container` — modules, stable
- `martensite::widgets::container::Container` — structs, stable
- `martensite::widgets::dropdown` — modules, stable
- `martensite::widgets::dropdown::Dropdown` — structs, stable
- `martensite::widgets::external` — modules, experimental
- `martensite::widgets::external::BindError` — enums, experimental
- `martensite::widgets::external::ExternalEngine` — structs, experimental
- `martensite::widgets::external::ExternalEngines` — structs, experimental
- `martensite::widgets::external::FramePoll` — enums, experimental
- `martensite::widgets::flex` — modules, stable
- `martensite::widgets::flex::CrossAxisAlignment` — enums, stable
- `martensite::widgets::flex::Flex` — structs, stable
- `martensite::widgets::flex::FlexDirection` — enums, stable
- `martensite::widgets::flex::MainAxisAlignment` — enums, stable
- `martensite::widgets::media` — modules, stable
- `martensite::widgets::media::MediaView` — structs, stable
- `martensite::widgets::media::VideoFit` — enums, stable
- `martensite::widgets::radio` — modules, stable
- `martensite::widgets::radio::RadioGroup` — structs, stable
- `martensite::widgets::radio::RadioOption` — structs, stable
- `martensite::widgets::scrollview` — modules, stable
- `martensite::widgets::scrollview::ScrollBarWidget` — structs, stable
- `martensite::widgets::scrollview::ScrollView` — structs, stable
- `martensite::widgets::slider` — modules, stable
- `martensite::widgets::slider::Slider` — structs, stable
- `martensite::widgets::slider::SliderOrientation` — enums, stable
- `martensite::widgets::stack` — modules, stable
- `martensite::widgets::stack::Stack` — structs, stable
- `martensite::widgets::stack::StackAlignment` — enums, stable
- `martensite::widgets::tabs` — modules, stable
- `martensite::widgets::tabs::TabActivation` — enums, stable
- `martensite::widgets::tabs::TabItem` — structs, stable
- `martensite::widgets::tabs::Tabs` — structs, stable
- `martensite::widgets::text` — modules, stable
- `martensite::widgets::text::Text` — structs, stable
- `martensite::widgets::text_input` — modules, stable
- `martensite::widgets::text_input::TextInput` — structs, stable
- `martensite::widgets::tooltip` — modules, stable
- `martensite::widgets::tooltip::DEFAULT_TOOLTIP_DELAY_MS` — constants, stable
- `martensite::widgets::tooltip::TOOLTIP_HOVER_GRACE_MS` — constants, stable
- `martensite::widgets::tooltip::Tooltip` — structs, stable
- `martensite::widgets::tooltip::TooltipBubble` — structs, stable
- `martensite::window` — re-exports, stable

</details>

### `martensite-access` (v0.17.0)

**104 public items** — 104 stable, 0 experimental. Categories: 1 constants, 9 enums, 35 functions, 8 modules, 30 re-exports, 20 structs, 1 traits.

- `martensite_access::AccessKitAdapter` — re-exports, stable
- `martensite_access::AccessibilityBuilder` — re-exports, stable
- `martensite_access::AsyncEventPump` — re-exports, stable
- `martensite_access::CaretTracker` — re-exports, stable
- `martensite_access::ColorRgba` — re-exports, stable
- `martensite_access::FocusAppearanceCheck` — re-exports, stable
- `martensite_access::FocusAreaCheck` — re-exports, stable
- `martensite_access::Node` — re-exports, stable
- `martensite_access::NodeFingerprint` — re-exports, stable
- `martensite_access::NodeId` — re-exports, stable
- `martensite_access::Rect` — re-exports, stable
- `martensite_access::Role` — re-exports, stable
- `martensite_access::Section508VpatReport` — re-exports, stable
- `martensite_access::SemanticTreeSync` — re-exports, stable
- `martensite_access::TextAffinity` — re-exports, stable
- `martensite_access::TextBoundary` — re-exports, stable
- `martensite_access::TextSelection` — re-exports, stable
- `martensite_access::TextSize` — re-exports, stable
- `martensite_access::TreeDiff` — re-exports, stable
- `martensite_access::TreeUpdate` — re-exports, stable
- `martensite_access::VpatConformanceLevel` — re-exports, stable
- `martensite_access::VpatCriterion` — re-exports, stable
- `martensite_access::VpatReport` — re-exports, stable
- `martensite_access::WcagLevel` — re-exports, stable
- `martensite_access::actions` — modules, stable
- `martensite_access::actions::A11yAction` — enums, stable
- `martensite_access::actions::ActionHandler` — traits, stable
- `martensite_access::actions::ActionTarget` — enums, stable
- `martensite_access::actions::ClosureActionHandler` — structs, stable
- `martensite_access::actions::QueuedActionDispatcher` — structs, stable
- `martensite_access::actions::dispatch_a11y_action` — functions, stable
- `martensite_access::actions::semantic_action_for` — functions, stable
- `martensite_access::adapter` — modules, stable
- `martensite_access::adapter::AccessKitAdapter` — structs, stable
- `martensite_access::caret` — modules, stable
- `martensite_access::caret::CaretGeometry` — structs, stable
- `martensite_access::caret::CaretTracker` — structs, stable
- `martensite_access::caret::ReadingDirection` — enums, stable
- `martensite_access::caret::TextAffinity` — enums, stable
- `martensite_access::caret::TextBoundary` — enums, stable
- `martensite_access::caret::TextSelection` — structs, stable
- `martensite_access::caret::WritingMode` — enums, stable
- `martensite_access::caret::byte_offset_to_char_index` — functions, stable
- `martensite_access::caret::char_index_to_byte_offset` — functions, stable
- `martensite_access::check_target_size` — re-exports, stable
- `martensite_access::check_text_contrast` — re-exports, stable
- `martensite_access::check_ui_component_contrast` — re-exports, stable
- `martensite_access::compliance` — modules, stable
- `martensite_access::compliance::ColorRgba` — structs, stable
- `martensite_access::compliance::FocusAppearanceCheck` — structs, stable
- `martensite_access::compliance::FocusAreaCheck` — structs, stable
- `martensite_access::compliance::Section508VpatReport` — structs, stable
- `martensite_access::compliance::TargetSizeContext` — structs, stable
- `martensite_access::compliance::TextSize` — enums, stable
- `martensite_access::compliance::VpatConformanceLevel` — enums, stable
- `martensite_access::compliance::VpatCriterion` — structs, stable
- `martensite_access::compliance::VpatReport` — structs, stable
- `martensite_access::compliance::WcagLevel` — enums, stable
- `martensite_access::compliance::check_target_size` — functions, stable
- `martensite_access::compliance::check_target_size_with_exceptions` — functions, stable
- `martensite_access::compliance::check_text_contrast` — functions, stable
- `martensite_access::compliance::check_ui_component_contrast` — functions, stable
- `martensite_access::compliance::contrast_ratio` — functions, stable
- `martensite_access::compliance::minimum_focus_indicator_area` — functions, stable
- `martensite_access::compliance::relative_luminance` — functions, stable
- `martensite_access::contrast_ratio` — re-exports, stable
- `martensite_access::minimum_focus_indicator_area` — re-exports, stable
- `martensite_access::node_id_to_widget_id` — functions, stable
- `martensite_access::properties` — modules, stable
- `martensite_access::properties::AccessibilityBuilder` — structs, stable
- `martensite_access::properties::role_for_button` — functions, stable
- `martensite_access::properties::role_for_checkbox` — functions, stable
- `martensite_access::properties::role_for_container` — functions, stable
- `martensite_access::properties::role_for_dialog` — functions, stable
- `martensite_access::properties::role_for_image` — functions, stable
- `martensite_access::properties::role_for_link` — functions, stable
- `martensite_access::properties::role_for_list` — functions, stable
- `martensite_access::properties::role_for_list_item` — functions, stable
- `martensite_access::properties::role_for_radio_button` — functions, stable
- `martensite_access::properties::role_for_slider` — functions, stable
- `martensite_access::properties::role_for_text_display` — functions, stable
- `martensite_access::properties::role_for_text_input` — functions, stable
- `martensite_access::properties::set_clickable` — functions, stable
- `martensite_access::properties::set_description` — functions, stable
- `martensite_access::properties::set_disabled` — functions, stable
- `martensite_access::properties::set_expanded` — functions, stable
- `martensite_access::properties::set_focusable` — functions, stable
- `martensite_access::properties::set_label` — functions, stable
- `martensite_access::properties::set_live` — functions, stable
- `martensite_access::properties::set_toggled` — functions, stable
- `martensite_access::properties::set_value` — functions, stable
- `martensite_access::pump` — modules, stable
- `martensite_access::pump::AsyncEventPump` — structs, stable
- `martensite_access::pump::MAX_QUEUE_SIZE` — constants, stable
- `martensite_access::rect_to_accesskit` — functions, stable
- `martensite_access::relative_luminance` — re-exports, stable
- `martensite_access::tree` — modules, stable
- `martensite_access::tree::NodeFingerprint` — structs, stable
- `martensite_access::tree::SemanticTreeSync` — structs, stable
- `martensite_access::tree::TreeDiff` — structs, stable
- `martensite_access::widget_id_to_node_id` — functions, stable
- `martensite_access::winit` — modules, stable
- `martensite_access::winit::BridgeHandlers` — structs, stable
- `martensite_access::winit::MartensiteAccessBridge` — structs, stable

### `martensite-access-platform` (v0.17.0)

**0 public items** — 0 stable, 0 experimental. Categories: —.

Whole crate classified EXPERIMENTAL (young integration surface, expected to settle during the RC line).


### `martensite-accesskit-winit` (v0.17.0)

**1 public items** — 0 stable, 0 experimental, 1 vendored-upstream. Categories: 1 structs.

Vendored upstream fork: the entire surface tracks its upstream project and is EXPERIMENTAL until the maintenance policy in §5 pins a re-sync contract.

- `accesskit_winit::Adapter` — structs, vendored

### `martensite-assets` (v0.17.0)

**33 public items** — 31 stable, 2 experimental. Categories: 5 enums, 2 modules, 16 re-exports, 9 structs, 1 traits.

Experimental areas: reactive VFS watcher behind the `reactive` feature.

- `martensite_assets::AssetHandle` — re-exports, stable
- `martensite_assets::AssetPath` — re-exports, stable
- `martensite_assets::AssetPathError` — re-exports, stable
- `martensite_assets::BindingInfo` — re-exports, stable
- `martensite_assets::BindingType` — re-exports, stable
- `martensite_assets::DiskVfs` — re-exports, stable
- `martensite_assets::EmbeddedVfs` — re-exports, stable
- `martensite_assets::EntryPoint` — re-exports, stable
- `martensite_assets::ShaderError` — re-exports, stable
- `martensite_assets::ShaderReflection` — re-exports, stable
- `martensite_assets::ShaderStage` — re-exports, stable
- `martensite_assets::ShaderValidator` — re-exports, stable
- `martensite_assets::Vfs` — re-exports, stable
- `martensite_assets::VfsBackend` — re-exports, stable
- `martensite_assets::shader` — modules, stable
- `martensite_assets::shader::BindingInfo` — structs, stable
- `martensite_assets::shader::BindingType` — enums, stable
- `martensite_assets::shader::EntryPoint` — structs, stable
- `martensite_assets::shader::ShaderError` — enums, stable
- `martensite_assets::shader::ShaderReflection` — structs, stable
- `martensite_assets::shader::ShaderStage` — enums, stable
- `martensite_assets::shader::ShaderValidator` — structs, stable
- `martensite_assets::vfs` — modules, stable
- `martensite_assets::vfs::AssetHandle` — structs, stable
- `martensite_assets::vfs::AssetPath` — structs, stable
- `martensite_assets::vfs::AssetPathError` — enums, stable
- `martensite_assets::vfs::DiskVfs` — re-exports, stable
- `martensite_assets::vfs::EmbeddedVfs` — structs, stable
- `martensite_assets::vfs::ReactiveVfsWatcher` — re-exports, experimental, feature `reactive`
- `martensite_assets::vfs::Vfs` — traits, stable
- `martensite_assets::vfs::VfsBackend` — enums, stable
- `martensite_assets::vfs::disk::DiskVfs` — structs, stable
- `martensite_assets::vfs::reactive::ReactiveVfsWatcher` — structs, experimental, feature `reactive`

### `martensite-blessed` (v0.17.0)

**58 public items** — 24 stable, 34 experimental. Categories: 8 enums, 5 modules, 25 re-exports, 19 structs, 1 traits.

Experimental areas: complex widget, API still settling; docking workspace framework added in v0.15.0.

- `martensite_blessed::AreaSeries` — re-exports, stable
- `martensite_blessed::AudioWaveform` — re-exports, stable
- `martensite_blessed::Chart` — re-exports, stable
- `martensite_blessed::ChartBounds` — re-exports, stable
- `martensite_blessed::CodeEditor` — re-exports, stable
- `martensite_blessed::Cursor` — re-exports, stable
- `martensite_blessed::DataTable` — re-exports, stable
- `martensite_blessed::DockDragSession` — re-exports, stable
- `martensite_blessed::DockDropZone` — re-exports, stable
- `martensite_blessed::DockError` — re-exports, stable
- `martensite_blessed::DockNode` — re-exports, stable
- `martensite_blessed::DockNodeLayout` — re-exports, stable
- `martensite_blessed::DockNodeLayoutKind` — re-exports, stable
- `martensite_blessed::DockPanel` — re-exports, stable
- `martensite_blessed::DockTree` — re-exports, stable
- `martensite_blessed::HighlightedSpan` — re-exports, stable
- `martensite_blessed::LineSeries` — re-exports, stable
- `martensite_blessed::NodeId` — re-exports, stable
- `martensite_blessed::Point` — re-exports, stable
- `martensite_blessed::Rect` — re-exports, stable
- `martensite_blessed::ScatterSeries` — re-exports, stable
- `martensite_blessed::SplitDirection` — re-exports, stable
- `martensite_blessed::TableStorage` — re-exports, stable
- `martensite_blessed::TokenKind` — re-exports, stable
- `martensite_blessed::audio_waveform` — modules, experimental
- `martensite_blessed::audio_waveform::AudioWaveform` — structs, experimental
- `martensite_blessed::chart` — modules, experimental
- `martensite_blessed::chart::AreaSeries` — structs, experimental
- `martensite_blessed::chart::Chart` — structs, experimental
- `martensite_blessed::chart::ChartBounds` — structs, experimental
- `martensite_blessed::chart::LineSeries` — structs, experimental
- `martensite_blessed::chart::Point` — re-exports, experimental
- `martensite_blessed::chart::ScatterSeries` — structs, experimental
- `martensite_blessed::code_editor` — modules, experimental
- `martensite_blessed::code_editor::CodeEditor` — structs, experimental
- `martensite_blessed::code_editor::Cursor` — structs, experimental
- `martensite_blessed::code_editor::HighlightedSpan` — structs, experimental
- `martensite_blessed::code_editor::TokenKind` — enums, experimental
- `martensite_blessed::data_table` — modules, experimental
- `martensite_blessed::data_table::ColumnConfig` — structs, experimental
- `martensite_blessed::data_table::ColumnSort` — enums, experimental
- `martensite_blessed::data_table::DataTable` — structs, experimental
- `martensite_blessed::data_table::KeyAction` — enums, experimental
- `martensite_blessed::data_table::RowFilter` — structs, experimental
- `martensite_blessed::data_table::SelectionModel` — structs, experimental
- `martensite_blessed::data_table::TableStorage` — traits, experimental
- `martensite_blessed::docking` — modules, experimental
- `martensite_blessed::docking::DockDragSession` — structs, experimental
- `martensite_blessed::docking::DockDropZone` — enums, experimental
- `martensite_blessed::docking::DockError` — enums, experimental
- `martensite_blessed::docking::DockNode` — enums, experimental
- `martensite_blessed::docking::DockNodeLayout` — structs, experimental
- `martensite_blessed::docking::DockNodeLayoutKind` — enums, experimental
- `martensite_blessed::docking::DockPanel` — structs, experimental
- `martensite_blessed::docking::DockTree` — structs, experimental
- `martensite_blessed::docking::NodeId` — structs, experimental
- `martensite_blessed::docking::Rect` — structs, experimental
- `martensite_blessed::docking::SplitDirection` — enums, experimental

### `martensite-clipboard` (v0.17.0)

**29 public items** — 29 stable, 0 experimental. Categories: 5 constants, 1 enums, 2 functions, 2 modules, 11 re-exports, 6 structs, 2 traits.

- `martensite_clipboard::ClipboardItem` — re-exports, stable
- `martensite_clipboard::ClipboardPayload` — re-exports, stable
- `martensite_clipboard::ClipboardService` — re-exports, stable
- `martensite_clipboard::DEFAULT_LAZY_DEADLINE` — re-exports, stable
- `martensite_clipboard::InMemoryClipboard` — re-exports, stable
- `martensite_clipboard::LazyPayload` — re-exports, stable
- `martensite_clipboard::Mime` — re-exports, stable
- `martensite_clipboard::PlatformClipboard` — re-exports, stable
- `martensite_clipboard::StubClipboard` — re-exports, stable
- `martensite_clipboard::canonicalize_mime` — re-exports, stable
- `martensite_clipboard::clipboard` — modules, stable
- `martensite_clipboard::clipboard::ClipboardItem` — structs, stable
- `martensite_clipboard::clipboard::ClipboardPayload` — enums, stable
- `martensite_clipboard::clipboard::ClipboardService` — traits, stable
- `martensite_clipboard::clipboard::DEFAULT_LAZY_DEADLINE` — constants, stable
- `martensite_clipboard::clipboard::InMemoryClipboard` — structs, stable
- `martensite_clipboard::clipboard::LazyPayload` — structs, stable
- `martensite_clipboard::clipboard::MIME_IMAGE_PNG` — constants, stable
- `martensite_clipboard::clipboard::MIME_TEXT_HTML` — constants, stable
- `martensite_clipboard::clipboard::MIME_TEXT_PLAIN` — constants, stable
- `martensite_clipboard::clipboard::MIME_TEXT_RTF` — constants, stable
- `martensite_clipboard::clipboard::Mime` — structs, stable
- `martensite_clipboard::clipboard::canonicalize_mime` — functions, stable
- `martensite_clipboard::default_platform_clipboard` — re-exports, stable
- `martensite_clipboard::platform` — modules, stable
- `martensite_clipboard::platform::NsPasteboardClipboard` — structs, stable
- `martensite_clipboard::platform::PlatformClipboard` — traits, stable
- `martensite_clipboard::platform::StubClipboard` — structs, stable
- `martensite_clipboard::platform::default_platform_clipboard` — functions, stable

### `martensite-clipboard-platform` (v0.17.0)

**4 public items** — 4 stable, 0 experimental. Categories: 1 functions, 1 modules, 1 structs, 1 traits.

- `martensite_clipboard_platform::ClipboardBackend` — traits, stable
- `martensite_clipboard_platform::macos` — modules, stable
- `martensite_clipboard_platform::macos::MacosBackend` — structs, stable
- `martensite_clipboard_platform::native_backend` — functions, stable

### `martensite-core` (v0.17.0)

**96 public items** — 89 stable, 7 experimental. Categories: 1 constants, 9 enums, 8 modules, 44 re-exports, 32 structs, 2 traits.

Experimental areas: devtools-timemachine arena snapshot/restore surface.

- `martensite_core::A11yEmittedNode` — re-exports, stable
- `martensite_core::AccessibilityContext` — re-exports, stable
- `martensite_core::ArenaError` — re-exports, stable
- `martensite_core::ArenaRestoreError` — re-exports, experimental, feature `devtools-timemachine`
- `martensite_core::ArenaState` — re-exports, experimental, feature `devtools-timemachine`
- `martensite_core::BreadthFirstIter` — re-exports, stable
- `martensite_core::Children` — re-exports, stable
- `martensite_core::ColdNode` — re-exports, stable
- `martensite_core::DEFAULT_LEASE_TIMEOUT` — re-exports, stable
- `martensite_core::DepthFirstIter` — re-exports, stable
- `martensite_core::DummyWidget` — re-exports, stable
- `martensite_core::EventContext` — re-exports, stable
- `martensite_core::EventResponse` — re-exports, stable
- `martensite_core::FontResource` — re-exports, stable
- `martensite_core::FrameFence` — re-exports, stable
- `martensite_core::FrameGuard` — re-exports, stable
- `martensite_core::GlyphInstance` — re-exports, stable
- `martensite_core::GlyphRun` — re-exports, stable
- `martensite_core::GradientStop` — re-exports, stable
- `martensite_core::GradientStops` — re-exports, stable
- `martensite_core::HotNode` — re-exports, stable
- `martensite_core::InlineTextCache` — re-exports, stable
- `martensite_core::LayoutConstraints` — re-exports, stable
- `martensite_core::LayoutContext` — re-exports, stable
- `martensite_core::NodeFlags` — re-exports, stable
- `martensite_core::OverlayA11yRef` — re-exports, stable
- `martensite_core::OverlayAnchor` — re-exports, stable
- `martensite_core::OverlayEntry` — re-exports, stable
- `martensite_core::OverlayLayer` — re-exports, stable
- `martensite_core::PaintCommand` — re-exports, stable
- `martensite_core::PaintContext` — re-exports, stable
- `martensite_core::PaintList` — re-exports, stable
- `martensite_core::PaintSegment` — re-exports, stable
- `martensite_core::PathBuilder` — re-exports, stable
- `martensite_core::PointerButton` — re-exports, stable
- `martensite_core::Rect` — re-exports, stable
- `martensite_core::SemanticAction` — re-exports, stable
- `martensite_core::SubtreeIter` — re-exports, stable
- `martensite_core::SurfaceId` — re-exports, stable
- `martensite_core::TimemachineState` — re-exports, experimental, feature `devtools-timemachine`
- `martensite_core::Widget` — re-exports, stable
- `martensite_core::WidgetArena` — re-exports, stable
- `martensite_core::WidgetEvent` — re-exports, stable
- `martensite_core::WidgetId` — re-exports, stable
- `martensite_core::arena` — modules, stable
- `martensite_core::arena::ArenaError` — enums, stable
- `martensite_core::arena::BreadthFirstIter` — structs, stable
- `martensite_core::arena::Children` — structs, stable
- `martensite_core::arena::DepthFirstIter` — structs, stable
- `martensite_core::arena::SubtreeIter` — structs, stable
- `martensite_core::arena::WidgetArena` — structs, stable
- `martensite_core::fence` — modules, stable
- `martensite_core::fence::DEFAULT_LEASE_TIMEOUT` — constants, stable
- `martensite_core::fence::FrameFence` — structs, stable
- `martensite_core::fence::FrameGuard` — structs, stable
- `martensite_core::id` — modules, stable
- `martensite_core::id::SurfaceId` — structs, stable
- `martensite_core::id::WidgetId` — structs, stable
- `martensite_core::node` — modules, stable
- `martensite_core::node::ColdNode` — structs, stable
- `martensite_core::node::HotNode` — structs, stable
- `martensite_core::node::InlineTextCache` — structs, stable
- `martensite_core::node::NodeFlags` — structs, stable
- `martensite_core::node::Rect` — structs, stable
- `martensite_core::overlay` — modules, stable
- `martensite_core::overlay::OverlayAnchor` — enums, stable
- `martensite_core::overlay::OverlayEntry` — structs, stable
- `martensite_core::overlay::OverlayLayer` — structs, stable
- `martensite_core::paint` — modules, stable
- `martensite_core::paint::FontResource` — structs, stable
- `martensite_core::paint::GlyphInstance` — structs, stable
- `martensite_core::paint::GlyphRun` — structs, stable
- `martensite_core::paint::GradientStop` — structs, stable
- `martensite_core::paint::GradientStops` — structs, stable
- `martensite_core::paint::PaintCommand` — enums, stable
- `martensite_core::paint::PaintList` — structs, stable
- `martensite_core::paint::PaintSegment` — enums, stable
- `martensite_core::paint::PathBuilder` — structs, stable
- `martensite_core::snapshot` — modules, experimental, feature `devtools-timemachine`
- `martensite_core::snapshot::ArenaRestoreError` — enums, experimental, feature `devtools-timemachine`
- `martensite_core::snapshot::ArenaState` — structs, experimental, feature `devtools-timemachine`
- `martensite_core::snapshot::TimemachineState` — traits, experimental, feature `devtools-timemachine`
- `martensite_core::widget` — modules, stable
- `martensite_core::widget::A11yEmittedNode` — structs, stable
- `martensite_core::widget::AccessibilityContext` — structs, stable
- `martensite_core::widget::DummyWidget` — structs, stable
- `martensite_core::widget::EventContext` — structs, stable
- `martensite_core::widget::EventResponse` — enums, stable
- `martensite_core::widget::LayoutConstraints` — structs, stable
- `martensite_core::widget::LayoutContext` — structs, stable
- `martensite_core::widget::OverlayA11yRef` — structs, stable
- `martensite_core::widget::PaintContext` — structs, stable
- `martensite_core::widget::PointerButton` — enums, stable
- `martensite_core::widget::SemanticAction` — enums, stable
- `martensite_core::widget::Widget` — traits, stable
- `martensite_core::widget::WidgetEvent` — enums, stable

### `martensite-cosmic-text` (v0.19.0-martensite.1)

**89 public items** — 0 stable, 0 experimental, 89 vendored-upstream. Categories: 16 enums, 1 functions, 15 glob re-exports, 1 modules, 1 re-exports, 52 structs, 3 traits.

Vendored upstream fork: the entire surface tracks its upstream project and is EXPERIMENTAL until the maintenance policy in §5 pins a re-sync contract.

- `cosmic_text::* (from self::attrs)` — glob re-exports, vendored
- `cosmic_text::* (from self::bidi_para)` — glob re-exports, vendored
- `cosmic_text::* (from self::buffer)` — glob re-exports, vendored
- `cosmic_text::* (from self::buffer_line)` — glob re-exports, vendored
- `cosmic_text::* (from self::cached)` — glob re-exports, vendored
- `cosmic_text::* (from self::cursor)` — glob re-exports, vendored
- `cosmic_text::* (from self::edit)` — glob re-exports, vendored
- `cosmic_text::* (from self::font)` — glob re-exports, vendored
- `cosmic_text::* (from self::glyph_cache)` — glob re-exports, vendored
- `cosmic_text::* (from self::layout)` — glob re-exports, vendored
- `cosmic_text::* (from self::line_ending)` — glob re-exports, vendored
- `cosmic_text::* (from self::render)` — glob re-exports, vendored
- `cosmic_text::* (from self::shape)` — glob re-exports, vendored
- `cosmic_text::* (from self::shape_run_cache)` — glob re-exports, vendored
- `cosmic_text::* (from self::swash)` — glob re-exports, vendored
- `cosmic_text::attrs::Attrs` — structs, vendored
- `cosmic_text::attrs::AttrsList` — structs, vendored
- `cosmic_text::attrs::AttrsOwned` — structs, vendored
- `cosmic_text::attrs::CacheMetrics` — structs, vendored
- `cosmic_text::attrs::Color` — structs, vendored
- `cosmic_text::attrs::DecorationMetrics` — structs, vendored
- `cosmic_text::attrs::FamilyOwned` — enums, vendored
- `cosmic_text::attrs::Feature` — structs, vendored
- `cosmic_text::attrs::FeatureTag` — structs, vendored
- `cosmic_text::attrs::FontFeatures` — structs, vendored
- `cosmic_text::attrs::FontMatchAttrs` — structs, vendored
- `cosmic_text::attrs::GlyphDecorationData` — structs, vendored
- `cosmic_text::attrs::LetterSpacing` — structs, vendored
- `cosmic_text::attrs::TextDecoration` — structs, vendored
- `cosmic_text::attrs::UnderlineStyle` — enums, vendored
- `cosmic_text::bidi_para::BidiParagraphs` — structs, vendored
- `cosmic_text::buffer::Buffer` — structs, vendored
- `cosmic_text::buffer::LayoutRun` — structs, vendored
- `cosmic_text::buffer::LayoutRunIter` — structs, vendored
- `cosmic_text::buffer::Metrics` — structs, vendored
- `cosmic_text::buffer_line::BufferLine` — structs, vendored
- `cosmic_text::cached::Cached` — enums, vendored
- `cosmic_text::cursor::Affinity` — enums, vendored
- `cosmic_text::cursor::Cursor` — structs, vendored
- `cosmic_text::cursor::LayoutCursor` — structs, vendored
- `cosmic_text::cursor::Motion` — enums, vendored
- `cosmic_text::cursor::Scroll` — structs, vendored
- `cosmic_text::edit::Action` — enums, vendored
- `cosmic_text::edit::BufferRef` — enums, vendored
- `cosmic_text::edit::Change` — structs, vendored
- `cosmic_text::edit::ChangeItem` — structs, vendored
- `cosmic_text::edit::Edit` — traits, vendored
- `cosmic_text::edit::Selection` — enums, vendored
- `cosmic_text::edit::editor::Editor` — structs, vendored
- `cosmic_text::edit::syntect::SyntaxEditor` — structs, vendored, feature `--all-features`
- `cosmic_text::edit::syntect::SyntaxSystem` — structs, vendored, feature `--all-features`
- `cosmic_text::edit::vi::ViEditor` — structs, vendored, feature `--all-features`
- `cosmic_text::font::Font` — structs, vendored
- `cosmic_text::font::fallback` — modules, vendored
- `cosmic_text::font::fallback::Fallback` — traits, vendored
- `cosmic_text::font::fallback::Fallbacks` — structs, vendored
- `cosmic_text::font::fallback::FontFallbackIter` — structs, vendored
- `cosmic_text::font::fallback::MonospaceFallbackInfo` — structs, vendored
- `cosmic_text::font::fallback::PlatformFallback` — re-exports, vendored
- `cosmic_text::font::fallback::platform::PlatformFallback` — structs, vendored
- `cosmic_text::font::system::BorrowedWithFontSystem` — structs, vendored
- `cosmic_text::font::system::FontMatchKey` — structs, vendored
- `cosmic_text::font::system::FontSystem` — structs, vendored
- `cosmic_text::glyph_cache::CacheKey` — structs, vendored
- `cosmic_text::glyph_cache::CacheKeyFlags` — structs, vendored
- `cosmic_text::glyph_cache::SubpixelBin` — enums, vendored
- `cosmic_text::layout::Align` — enums, vendored
- `cosmic_text::layout::DecorationSpan` — structs, vendored
- `cosmic_text::layout::Ellipsize` — enums, vendored
- `cosmic_text::layout::EllipsizeHeightLimit` — enums, vendored
- `cosmic_text::layout::Hinting` — enums, vendored
- `cosmic_text::layout::LayoutGlyph` — structs, vendored
- `cosmic_text::layout::LayoutLine` — structs, vendored
- `cosmic_text::layout::PhysicalGlyph` — structs, vendored
- `cosmic_text::layout::Wrap` — enums, vendored
- `cosmic_text::line_ending::LineEnding` — enums, vendored
- `cosmic_text::line_ending::LineIter` — structs, vendored
- `cosmic_text::render::LegacyRenderer` — structs, vendored
- `cosmic_text::render::Renderer` — traits, vendored
- `cosmic_text::render::render_decoration` — functions, vendored
- `cosmic_text::shape::ShapeBuffer` — structs, vendored
- `cosmic_text::shape::ShapeGlyph` — structs, vendored
- `cosmic_text::shape::ShapeLine` — structs, vendored
- `cosmic_text::shape::ShapeSpan` — structs, vendored
- `cosmic_text::shape::ShapeWord` — structs, vendored
- `cosmic_text::shape::Shaping` — enums, vendored
- `cosmic_text::shape_run_cache::ShapeRunCache` — structs, vendored
- `cosmic_text::shape_run_cache::ShapeRunKey` — structs, vendored
- `cosmic_text::swash::SwashCache` — structs, vendored

### `martensite-devtools` (v0.17.0)

**25 public items** — 18 stable, 7 experimental. Categories: 1 enums, 7 functions, 3 modules, 13 structs, 1 type aliases.

Experimental areas: time-travel debugging behind devtools-timemachine (v0.17.0).

- `martensite_devtools::hud` — modules, stable
- `martensite_devtools::hud::ArenaTelemetry` — structs, stable
- `martensite_devtools::hud::DiagnosticHud` — structs, stable
- `martensite_devtools::hud::DirtyRectTracker` — structs, stable
- `martensite_devtools::hud::FrameHistogram` — structs, stable
- `martensite_devtools::hud::FrameTiming` — structs, stable
- `martensite_devtools::hud::Rect` — structs, stable
- `martensite_devtools::timemachine` — modules, experimental, feature `render,devtools-timemachine`
- `martensite_devtools::timemachine::Checkpoint` — structs, experimental, feature `render,devtools-timemachine`
- `martensite_devtools::timemachine::ReplayError` — enums, experimental, feature `render,devtools-timemachine`
- `martensite_devtools::timemachine::SignalWrite` — structs, experimental, feature `render,devtools-timemachine`
- `martensite_devtools::timemachine::TimeMachine` — structs, experimental, feature `render,devtools-timemachine`
- `martensite_devtools::timemachine::WidgetFactory` — type aliases, experimental, feature `render,devtools-timemachine`
- `martensite_devtools::timemachine::World` — structs, experimental, feature `render,devtools-timemachine`
- `martensite_devtools::tracy` — modules, stable
- `martensite_devtools::tracy::SpanRecord` — structs, stable
- `martensite_devtools::tracy::TracySpan` — structs, stable
- `martensite_devtools::tracy::TracySpanGuard` — structs, stable
- `martensite_devtools::tracy::frame_count` — functions, stable
- `martensite_devtools::tracy::frame_mark` — functions, stable
- `martensite_devtools::tracy::last_span_duration` — functions, stable
- `martensite_devtools::tracy::plot` — functions, stable
- `martensite_devtools::tracy::plot_value` — functions, stable
- `martensite_devtools::tracy::span` — functions, stable
- `martensite_devtools::tracy::span_record_count` — functions, stable

### `martensite-dnd` (v0.17.0)

**49 public items** — 49 stable, 0 experimental. Categories: 8 enums, 1 functions, 4 modules, 22 re-exports, 13 structs, 1 traits.

- `martensite_dnd::DndCapabilities` — re-exports, stable
- `martensite_dnd::DndPlatform` — re-exports, stable
- `martensite_dnd::DndPlatformError` — re-exports, stable
- `martensite_dnd::DndSession` — re-exports, stable
- `martensite_dnd::DndSessionManager` — re-exports, stable
- `martensite_dnd::DndStatus` — re-exports, stable
- `martensite_dnd::DragPayload` — re-exports, stable
- `martensite_dnd::DropBridge` — re-exports, stable
- `martensite_dnd::DropEffect` — enums, stable
- `martensite_dnd::DropEffectMask` — re-exports, stable
- `martensite_dnd::DropInput` — re-exports, stable
- `martensite_dnd::DropOutcome` — re-exports, stable
- `martensite_dnd::DropTarget` — re-exports, stable
- `martensite_dnd::DropTargetRegistry` — re-exports, stable
- `martensite_dnd::DropTargetState` — re-exports, stable
- `martensite_dnd::DropTypeHint` — re-exports, stable
- `martensite_dnd::PlatformTransferId` — re-exports, stable
- `martensite_dnd::ProposedAction` — re-exports, stable
- `martensite_dnd::SessionId` — re-exports, stable
- `martensite_dnd::TargetId` — re-exports, stable
- `martensite_dnd::UnsupportedDndPlatform` — re-exports, stable
- `martensite_dnd::WinitDndPlatform` — re-exports, stable
- `martensite_dnd::bridge` — modules, stable
- `martensite_dnd::bridge::DropBridge` — structs, stable
- `martensite_dnd::bridge::DropInput` — enums, stable
- `martensite_dnd::bridge::DropOutcome` — structs, stable
- `martensite_dnd::bridge::convert_winit_drop_event` — functions, stable
- `martensite_dnd::convert_winit_drop_event` — re-exports, stable
- `martensite_dnd::platform` — modules, stable
- `martensite_dnd::platform::DndCapabilities` — structs, stable
- `martensite_dnd::platform::DndPlatform` — traits, stable
- `martensite_dnd::platform::DndPlatformError` — enums, stable
- `martensite_dnd::platform::DragPayload` — enums, stable
- `martensite_dnd::platform::DropTypeHint` — enums, stable
- `martensite_dnd::platform::PlatformTransferId` — structs, stable
- `martensite_dnd::platform::ProposedAction` — enums, stable
- `martensite_dnd::platform::UnsupportedDndPlatform` — structs, stable
- `martensite_dnd::platform::WinitDndPlatform` — structs, stable
- `martensite_dnd::session` — modules, stable
- `martensite_dnd::session::DndSession` — structs, stable
- `martensite_dnd::session::DndSessionManager` — structs, stable
- `martensite_dnd::session::DndStatus` — enums, stable
- `martensite_dnd::session::SessionId` — structs, stable
- `martensite_dnd::target` — modules, stable
- `martensite_dnd::target::DropEffectMask` — structs, stable
- `martensite_dnd::target::DropTarget` — structs, stable
- `martensite_dnd::target::DropTargetRegistry` — structs, stable
- `martensite_dnd::target::DropTargetState` — enums, stable
- `martensite_dnd::target::TargetId` — structs, stable

### `martensite-engine-bridge` (v0.17.0)

**47 public items** — 0 stable, 47 experimental. Categories: 1 constants, 5 enums, 5 modules, 22 re-exports, 11 structs, 2 traits, 1 type aliases.

Whole crate classified EXPERIMENTAL (young integration surface, expected to settle during the RC line).

- `martensite_engine_bridge::BridgeError` — re-exports, experimental
- `martensite_engine_bridge::BridgeHandle` — re-exports, experimental
- `martensite_engine_bridge::BridgeRegistry` — re-exports, experimental
- `martensite_engine_bridge::CpuFrame` — re-exports, experimental
- `martensite_engine_bridge::Engine` — re-exports, experimental
- `martensite_engine_bridge::EngineContext` — re-exports, experimental
- `martensite_engine_bridge::EngineEvent` — re-exports, experimental
- `martensite_engine_bridge::Frame` — re-exports, experimental
- `martensite_engine_bridge::FrameSync` — re-exports, experimental
- `martensite_engine_bridge::FrameToken` — re-exports, experimental
- `martensite_engine_bridge::FrontFrame` — re-exports, experimental
- `martensite_engine_bridge::MAX_FRAME_DIM` — constants, experimental
- `martensite_engine_bridge::NativeFrame` — re-exports, experimental
- `martensite_engine_bridge::PointerButton` — re-exports, experimental
- `martensite_engine_bridge::SharedTexture` — re-exports, experimental
- `martensite_engine_bridge::SourceAlpha` — re-exports, experimental
- `martensite_engine_bridge::SurfaceId` — re-exports, experimental
- `martensite_engine_bridge::SurfaceRing` — re-exports, experimental
- `martensite_engine_bridge::TakenFrontFrame` — re-exports, experimental
- `martensite_engine_bridge::TextureFrame` — re-exports, experimental
- `martensite_engine_bridge::Viewport` — re-exports, experimental
- `martensite_engine_bridge::bridge` — modules, experimental
- `martensite_engine_bridge::bridge::BridgeHandle` — structs, experimental
- `martensite_engine_bridge::bridge::BridgeRegistry` — structs, experimental
- `martensite_engine_bridge::bridge::FrontFrame` — structs, experimental
- `martensite_engine_bridge::bridge::SurfaceId` — re-exports, experimental
- `martensite_engine_bridge::bridge::SurfaceRing` — structs, experimental
- `martensite_engine_bridge::bridge::TakenFrontFrame` — structs, experimental
- `martensite_engine_bridge::engine` — modules, experimental
- `martensite_engine_bridge::engine::Engine` — traits, experimental
- `martensite_engine_bridge::engine::EngineContext` — structs, experimental
- `martensite_engine_bridge::engine::EngineEvent` — enums, experimental
- `martensite_engine_bridge::engine::PointerButton` — re-exports, experimental
- `martensite_engine_bridge::engine::Viewport` — structs, experimental
- `martensite_engine_bridge::error` — modules, experimental
- `martensite_engine_bridge::error::BridgeError` — enums, experimental
- `martensite_engine_bridge::frame` — modules, experimental
- `martensite_engine_bridge::frame::CpuFrame` — structs, experimental
- `martensite_engine_bridge::frame::Frame` — traits, experimental
- `martensite_engine_bridge::frame::FrameSync` — enums, experimental
- `martensite_engine_bridge::frame::FrameToken` — structs, experimental
- `martensite_engine_bridge::frame::NativeFrame` — enums, experimental
- `martensite_engine_bridge::frame::SharedTexture` — type aliases, experimental
- `martensite_engine_bridge::frame::SourceAlpha` — enums, experimental
- `martensite_engine_bridge::frame::TextureFrame` — structs, experimental
- `martensite_engine_bridge::testing` — modules, experimental
- `martensite_engine_bridge::testing::MockEngine` — structs, experimental

### `martensite-focus` (v0.17.0)

**22 public items** — 22 stable, 0 experimental. Categories: 3 constants, 2 enums, 3 modules, 10 re-exports, 4 structs.

- `martensite_focus::DEFAULT_ALPHA` — re-exports, stable
- `martensite_focus::DEFAULT_BETA` — re-exports, stable
- `martensite_focus::Direction` — re-exports, stable
- `martensite_focus::FORWARD_CONE_DEGREES` — re-exports, stable
- `martensite_focus::FocusDirection` — re-exports, stable
- `martensite_focus::FocusManager` — re-exports, stable
- `martensite_focus::FocusScope` — re-exports, stable
- `martensite_focus::FocusScopeStack` — re-exports, stable
- `martensite_focus::SpatialNavigator` — re-exports, stable
- `martensite_focus::TabNavigation` — re-exports, stable
- `martensite_focus::manager` — modules, stable
- `martensite_focus::manager::FocusManager` — structs, stable
- `martensite_focus::manager::TabNavigation` — enums, stable
- `martensite_focus::scope` — modules, stable
- `martensite_focus::scope::FocusScope` — structs, stable
- `martensite_focus::scope::FocusScopeStack` — structs, stable
- `martensite_focus::spatial` — modules, stable
- `martensite_focus::spatial::DEFAULT_ALPHA` — constants, stable
- `martensite_focus::spatial::DEFAULT_BETA` — constants, stable
- `martensite_focus::spatial::FORWARD_CONE_DEGREES` — constants, stable
- `martensite_focus::spatial::FocusDirection` — enums, stable
- `martensite_focus::spatial::SpatialNavigator` — structs, stable

### `martensite-font-fallback` (v0.17.0)

**3 public items** — 3 stable, 0 experimental. Categories: 1 functions, 1 modules, 1 structs.

- `martensite_font_fallback::coretext` — modules, stable
- `martensite_font_fallback::coretext::CoreTextFontFallback` — structs, stable
- `martensite_font_fallback::native_provider` — functions, stable

### `martensite-history` (v0.17.0)

**17 public items** — 17 stable, 0 experimental. Categories: 1 enums, 2 modules, 7 re-exports, 6 structs, 1 traits.

- `martensite_history::ChangeOp` — re-exports, stable
- `martensite_history::HistoryLedger` — re-exports, stable
- `martensite_history::HistoryNode` — re-exports, stable
- `martensite_history::HistoryTree` — re-exports, stable
- `martensite_history::LedgerError` — re-exports, stable
- `martensite_history::NodeId` — re-exports, stable
- `martensite_history::NodeIdError` — re-exports, stable
- `martensite_history::lca` — modules, stable
- `martensite_history::lca::HistoryNode` — structs, stable
- `martensite_history::lca::HistoryTree` — structs, stable
- `martensite_history::lca::NavPath` — structs, stable
- `martensite_history::lca::NodeId` — structs, stable
- `martensite_history::lca::NodeIdError` — structs, stable
- `martensite_history::ledger` — modules, stable
- `martensite_history::ledger::ChangeOp` — traits, stable
- `martensite_history::ledger::HistoryLedger` — structs, stable
- `martensite_history::ledger::LedgerError` — enums, stable

### `martensite-host` (v0.17.0)

**4 public items** — 0 stable, 4 experimental. Categories: 1 constants, 1 enums, 2 structs.

Whole crate classified EXPERIMENTAL (young integration surface, expected to settle during the RC line).

- `martensite_host::GuestLibrary` — structs, experimental
- `martensite_host::HostApp` — structs, experimental
- `martensite_host::HostError` — enums, experimental
- `martensite_host::RENDER_SYMBOL_NAME` — constants, experimental

### `martensite-l10n` (v0.17.0)

**11 public items** — 11 stable, 0 experimental. Categories: 2 enums, 2 functions, 3 modules, 1 re-exports, 2 structs, 1 type aliases.

- `martensite_l10n::LanguageIdentifier` — re-exports, stable
- `martensite_l10n::direction` — modules, stable
- `martensite_l10n::direction::ScriptDirection` — enums, stable
- `martensite_l10n::direction::direction_for_locale` — functions, stable
- `martensite_l10n::direction::direction_for_script` — functions, stable
- `martensite_l10n::fluent` — modules, stable
- `martensite_l10n::fluent::FluentBundle` — type aliases, stable
- `martensite_l10n::fluent::FluentCatalog` — structs, stable
- `martensite_l10n::fluent::L10nError` — enums, stable
- `martensite_l10n::reactive` — modules, stable
- `martensite_l10n::reactive::L10n` — structs, stable

### `martensite-layout` (v0.17.0)

**56 public items** — 56 stable, 0 experimental. Categories: 1 constants, 2 enums, 10 functions, 1 glob re-exports, 5 modules, 22 re-exports, 15 structs.

- `martensite_layout::* (from taffy::prelude)` — glob re-exports, stable
- `martensite_layout::ArenaBridge` — re-exports, stable
- `martensite_layout::ArenaChildIter` — re-exports, stable
- `martensite_layout::BidiRect` — re-exports, stable
- `martensite_layout::Constraints` — re-exports, stable
- `martensite_layout::EdgeInsets` — re-exports, stable
- `martensite_layout::FlowTransposition` — re-exports, stable
- `martensite_layout::IdMapIter` — re-exports, stable
- `martensite_layout::LayoutEngine` — re-exports, stable
- `martensite_layout::LayoutError` — re-exports, stable
- `martensite_layout::LogicalPoint` — re-exports, stable
- `martensite_layout::LogicalSize` — re-exports, stable
- `martensite_layout::Point` — re-exports, stable
- `martensite_layout::SelectionGeometry` — re-exports, stable
- `martensite_layout::Size` — re-exports, stable
- `martensite_layout::TaffyTree` — re-exports, stable
- `martensite_layout::WritingMode` — re-exports, stable
- `martensite_layout::bidi_rect` — modules, stable
- `martensite_layout::bidi_rect::BidiSelectionRect` — structs, stable
- `martensite_layout::bidi_rect::PhysicalRect` — structs, stable
- `martensite_layout::bidi_rect::flip_inline_offset` — functions, stable
- `martensite_layout::bidi_rect::is_rtl_level` — functions, stable
- `martensite_layout::bidi_rect::logical_to_physical` — functions, stable
- `martensite_layout::bidi_rect::physical_to_logical_inline` — functions, stable
- `martensite_layout::constraints_to_available` — re-exports, stable
- `martensite_layout::edge_insets_to_style` — re-exports, stable
- `martensite_layout::engine` — modules, stable
- `martensite_layout::engine::IdMapIter` — structs, stable
- `martensite_layout::engine::LayoutEngine` — structs, stable
- `martensite_layout::engine::LayoutError` — enums, stable
- `martensite_layout::engine::MAX_LAYOUT_DEPTH` — constants, stable
- `martensite_layout::engine::constraints_to_available` — functions, stable
- `martensite_layout::engine::edge_insets_to_style` — functions, stable
- `martensite_layout::engine::taffy_layout_to_bidi_rect` — functions, stable
- `martensite_layout::engine::taffy_layout_to_rect` — functions, stable
- `martensite_layout::geometry` — modules, stable
- `martensite_layout::geometry::BidiRect` — structs, stable
- `martensite_layout::geometry::Constraints` — structs, stable
- `martensite_layout::geometry::EdgeInsets` — structs, stable
- `martensite_layout::geometry::Point` — structs, stable
- `martensite_layout::geometry::SelectionGeometry` — structs, stable
- `martensite_layout::geometry::Size` — structs, stable
- `martensite_layout::node_id_to_widget_id` — re-exports, stable
- `martensite_layout::taffy_bridge` — modules, stable
- `martensite_layout::taffy_bridge::ArenaBridge` — structs, stable
- `martensite_layout::taffy_bridge::ArenaChildIter` — structs, stable
- `martensite_layout::taffy_bridge::node_id_to_widget_id` — functions, stable
- `martensite_layout::taffy_bridge::widget_id_to_node_id` — functions, stable
- `martensite_layout::taffy_layout_to_bidi_rect` — re-exports, stable
- `martensite_layout::taffy_layout_to_rect` — re-exports, stable
- `martensite_layout::vertical_flow` — modules, stable
- `martensite_layout::vertical_flow::FlowTransposition` — structs, stable
- `martensite_layout::vertical_flow::LogicalPoint` — structs, stable
- `martensite_layout::vertical_flow::LogicalSize` — structs, stable
- `martensite_layout::vertical_flow::WritingMode` — enums, stable
- `martensite_layout::widget_id_to_node_id` — re-exports, stable

### `martensite-macros` (v0.17.0)

**1 public items** — 1 stable, 0 experimental. Categories: 1 macros.

- `martensite_macros::widget` — macros, stable

### `martensite-media` (v0.17.0)

**88 public items** — 65 stable, 23 experimental. Categories: 5 enums, 12 functions, 6 modules, 55 re-exports, 9 structs, 1 traits.

Experimental areas: decoder pipeline added in v0.16.0, hardware-verified backends pending.

- `martensite_media::ColorRange` — re-exports, stable
- `martensite_media::ColorSpace` — re-exports, stable
- `martensite_media::ContentLightLevel` — re-exports, stable
- `martensite_media::DecodeStats` — re-exports, experimental
- `martensite_media::DecodedFrame` — re-exports, experimental
- `martensite_media::DecoderBackend` — re-exports, experimental
- `martensite_media::DecoderConfig` — re-exports, experimental
- `martensite_media::DisplayCapabilities` — re-exports, stable
- `martensite_media::DisplayProfile` — re-exports, stable
- `martensite_media::EncodedPacket` — re-exports, experimental
- `martensite_media::Eotf` — re-exports, stable
- `martensite_media::FrameQueue` — re-exports, stable
- `martensite_media::HardwareHandle` — re-exports, stable
- `martensite_media::HdrMetadata` — re-exports, stable
- `martensite_media::MasteringDisplayVolume` — re-exports, stable
- `martensite_media::MediaError` — re-exports, stable
- `martensite_media::MockDecoder` — re-exports, experimental
- `martensite_media::QueueAction` — re-exports, stable
- `martensite_media::ScRgb` — re-exports, stable
- `martensite_media::ToneMapOperator` — re-exports, stable
- `martensite_media::TransferFunction` — re-exports, stable
- `martensite_media::VideoCodec` — re-exports, experimental
- `martensite_media::VideoDecoder` — re-exports, experimental
- `martensite_media::VideoFrameMetadata` — re-exports, stable
- `martensite_media::VideoPixelFormat` — re-exports, stable
- `martensite_media::VideoSurface` — re-exports, stable
- `martensite_media::bt2020_to_bt709_linear` — re-exports, stable
- `martensite_media::bt2020_yuv_to_rgb` — re-exports, stable
- `martensite_media::bt709_yuv_to_rgb` — re-exports, stable
- `martensite_media::color` — modules, stable
- `martensite_media::color::ColorSpace` — enums, stable
- `martensite_media::color::ScRgb` — structs, stable
- `martensite_media::color::TransferFunction` — enums, stable
- `martensite_media::color::bt2020_to_bt709_linear` — functions, stable
- `martensite_media::color::bt2020_yuv_to_rgb` — functions, stable
- `martensite_media::color::bt709_yuv_to_rgb` — functions, stable
- `martensite_media::color::delta_e_76` — functions, stable
- `martensite_media::color::pq_eotf` — functions, stable
- `martensite_media::color::pq_oetf` — functions, stable
- `martensite_media::color::rgb_to_bt2020_yuv` — functions, stable
- `martensite_media::color::rgb_to_bt709_yuv` — functions, stable
- `martensite_media::color::rgb_to_xyz` — functions, stable
- `martensite_media::color::xyz_to_lab` — functions, stable
- `martensite_media::decoder` — modules, experimental
- `martensite_media::decoder::ColorRange` — re-exports, experimental
- `martensite_media::decoder::DecodeStats` — re-exports, experimental
- `martensite_media::decoder::DecodedFrame` — re-exports, experimental
- `martensite_media::decoder::DecoderBackend` — re-exports, experimental
- `martensite_media::decoder::DecoderConfig` — re-exports, experimental
- `martensite_media::decoder::EncodedPacket` — re-exports, experimental
- `martensite_media::decoder::HardwareHandle` — re-exports, experimental
- `martensite_media::decoder::HdrSideData` — re-exports, experimental
- `martensite_media::decoder::MockDecoder` — structs, experimental
- `martensite_media::decoder::VideoCodec` — re-exports, experimental
- `martensite_media::decoder::VideoDecoder` — traits, experimental
- `martensite_media::decoder::VideoFrameMetadata` — re-exports, experimental
- `martensite_media::decoder::VideoPixelFormat` — re-exports, experimental
- `martensite_media::decoder::videotoolbox` — re-exports, experimental, feature `decoder-videotoolbox,decoder-mf,test-noop`
- `martensite_media::delta_e_76` — re-exports, stable
- `martensite_media::hable_tonemap_scalar` — re-exports, stable
- `martensite_media::hdr` — modules, stable
- `martensite_media::hdr::ContentLightLevel` — structs, stable
- `martensite_media::hdr::Eotf` — enums, stable
- `martensite_media::hdr::HdrMetadata` — structs, stable
- `martensite_media::hdr::MasteringDisplayVolume` — structs, stable
- `martensite_media::pq_eotf` — re-exports, stable
- `martensite_media::pq_oetf` — re-exports, stable
- `martensite_media::queue` — modules, stable
- `martensite_media::queue::FrameQueue` — structs, stable
- `martensite_media::queue::QueueAction` — enums, stable
- `martensite_media::rgb_to_bt2020_yuv` — re-exports, stable
- `martensite_media::rgb_to_bt709_yuv` — re-exports, stable
- `martensite_media::rgb_to_xyz` — re-exports, stable
- `martensite_media::surface` — modules, stable
- `martensite_media::surface::ColorRange` — re-exports, stable
- `martensite_media::surface::HardwareHandle` — re-exports, stable
- `martensite_media::surface::MediaError` — re-exports, stable
- `martensite_media::surface::VideoFrameMetadata` — re-exports, stable
- `martensite_media::surface::VideoPixelFormat` — re-exports, stable
- `martensite_media::surface::VideoSurface` — structs, stable
- `martensite_media::tonemap` — modules, stable
- `martensite_media::tonemap::DisplayCapabilities` — structs, stable
- `martensite_media::tonemap::DisplayProfile` — structs, stable
- `martensite_media::tonemap::ToneMapOperator` — enums, stable
- `martensite_media::tonemap::hable_tonemap_scalar` — functions, stable
- `martensite_media::tonemap::uchimura_tonemap_scalar` — functions, stable
- `martensite_media::uchimura_tonemap_scalar` — re-exports, stable
- `martensite_media::xyz_to_lab` — re-exports, stable

### `martensite-media-platform` (v0.17.0)

**33 public items** — 18 stable, 15 experimental. Categories: 7 enums, 7 functions, 3 modules, 6 re-exports, 10 structs.

Experimental areas: decoder wire types/backends added in v0.16.0; hardware surface import FFI; wgpu-hal API still evolving.

- `martensite_media_platform::ColorRange` — re-exports, stable
- `martensite_media_platform::HardwareHandle` — re-exports, stable
- `martensite_media_platform::ImportTextureDescriptor` — structs, stable
- `martensite_media_platform::MediaError` — re-exports, stable
- `martensite_media_platform::VideoFrameMetadata` — re-exports, stable
- `martensite_media_platform::VideoPixelFormat` — re-exports, stable
- `martensite_media_platform::VideoTexture` — structs, stable
- `martensite_media_platform::chroma_texture_format` — functions, stable
- `martensite_media_platform::create_video_texture_view` — functions, stable
- `martensite_media_platform::decoder` — modules, experimental
- `martensite_media_platform::decoder::DecodeError` — enums, experimental
- `martensite_media_platform::decoder::DecodeStats` — structs, experimental
- `martensite_media_platform::decoder::DecodedFrame` — structs, experimental
- `martensite_media_platform::decoder::DecoderBackend` — enums, experimental
- `martensite_media_platform::decoder::DecoderConfig` — structs, experimental
- `martensite_media_platform::decoder::EncodedPacket` — structs, experimental
- `martensite_media_platform::decoder::HdrSideData` — structs, experimental
- `martensite_media_platform::decoder::VideoCodec` — enums, experimental
- `martensite_media_platform::decoder::videotoolbox` — modules, experimental, feature `decoder-videotoolbox,decoder-mf`
- `martensite_media_platform::decoder::videotoolbox::VideoToolboxDecoder` — structs, experimental, feature `decoder-videotoolbox,decoder-mf`
- `martensite_media_platform::hal` — re-exports, stable
- `martensite_media_platform::import_cpu_memory` — functions, experimental
- `martensite_media_platform::import_external_planes` — functions, experimental
- `martensite_media_platform::import_external_texture` — functions, experimental
- `martensite_media_platform::import_iosurface` — functions, experimental
- `martensite_media_platform::luma_texture_format` — functions, stable
- `martensite_media_platform::surface` — modules, stable
- `martensite_media_platform::surface::ColorRange` — enums, stable
- `martensite_media_platform::surface::DmaBufPlane` — structs, stable
- `martensite_media_platform::surface::HardwareHandle` — enums, stable
- `martensite_media_platform::surface::MediaError` — enums, stable
- `martensite_media_platform::surface::VideoFrameMetadata` — structs, stable
- `martensite_media_platform::surface::VideoPixelFormat` — enums, stable

### `martensite-motion` (v0.17.0)

**27 public items** — 27 stable, 0 experimental. Categories: 1 constants, 4 enums, 3 modules, 12 re-exports, 7 structs.

- `martensite_motion::AnimationDriver` — re-exports, stable
- `martensite_motion::AnimationDriver2D` — re-exports, stable
- `martensite_motion::AnimationError` — re-exports, stable
- `martensite_motion::AnimationId` — re-exports, stable
- `martensite_motion::AxisLock` — re-exports, stable
- `martensite_motion::DampingRegime` — re-exports, stable
- `martensite_motion::RUBBER_BAND_COEFFICIENT` — re-exports, stable
- `martensite_motion::RubberBandScroller` — re-exports, stable
- `martensite_motion::RubberBandScroller2D` — re-exports, stable
- `martensite_motion::SpringConfig` — re-exports, stable
- `martensite_motion::SpringConfigError` — re-exports, stable
- `martensite_motion::SpringSolver` — re-exports, stable
- `martensite_motion::animation` — modules, stable
- `martensite_motion::animation::AnimationDriver` — structs, stable
- `martensite_motion::animation::AnimationDriver2D` — structs, stable
- `martensite_motion::animation::AnimationError` — enums, stable
- `martensite_motion::animation::AnimationId` — structs, stable
- `martensite_motion::rubber_band` — modules, stable
- `martensite_motion::rubber_band::AxisLock` — enums, stable
- `martensite_motion::rubber_band::RUBBER_BAND_COEFFICIENT` — constants, stable
- `martensite_motion::rubber_band::RubberBandScroller` — structs, stable
- `martensite_motion::rubber_band::RubberBandScroller2D` — structs, stable
- `martensite_motion::spring` — modules, stable
- `martensite_motion::spring::DampingRegime` — enums, stable
- `martensite_motion::spring::SpringConfig` — structs, stable
- `martensite_motion::spring::SpringConfigError` — enums, stable
- `martensite_motion::spring::SpringSolver` — structs, stable

### `martensite-plugin` (v0.17.0)

**31 public items** — 14 stable, 17 experimental. Categories: 4 constants, 3 enums, 3 modules, 14 re-exports, 7 structs.

Experimental areas: plugin IPC ring buffer, part of the runtime ABI; plugin runtime ABI added in v0.11.0, ecosystem immature; plugin sandbox/policy surface, ecosystem immature.

- `martensite_plugin::Capability` — re-exports, stable
- `martensite_plugin::CapabilitySet` — re-exports, stable
- `martensite_plugin::DEFAULT_CAPACITY` — re-exports, stable
- `martensite_plugin::DEFAULT_FUEL_BUDGET` — re-exports, stable
- `martensite_plugin::PluginBuilder` — re-exports, stable
- `martensite_plugin::PluginError` — re-exports, stable
- `martensite_plugin::PluginInstance` — re-exports, stable
- `martensite_plugin::PluginPaintCmd` — re-exports, stable
- `martensite_plugin::PluginRingBuffer` — re-exports, stable
- `martensite_plugin::PluginRuntime` — re-exports, stable
- `martensite_plugin::PluginState` — re-exports, stable
- `martensite_plugin::RING_BUFFER_REGION_SIZE` — re-exports, stable
- `martensite_plugin::RingBufferError` — re-exports, stable
- `martensite_plugin::SHARED_HEADER_SIZE` — re-exports, stable
- `martensite_plugin::ring_buffer` — modules, experimental
- `martensite_plugin::ring_buffer::DEFAULT_CAPACITY` — constants, experimental
- `martensite_plugin::ring_buffer::PluginPaintCmd` — structs, experimental
- `martensite_plugin::ring_buffer::PluginRingBuffer` — structs, experimental
- `martensite_plugin::ring_buffer::RingBufferError` — enums, experimental
- `martensite_plugin::ring_buffer::SHARED_HEADER_SIZE` — constants, experimental
- `martensite_plugin::runtime` — modules, experimental
- `martensite_plugin::runtime::DEFAULT_FUEL_BUDGET` — constants, experimental
- `martensite_plugin::runtime::PluginError` — enums, experimental
- `martensite_plugin::runtime::PluginInstance` — structs, experimental
- `martensite_plugin::runtime::PluginRuntime` — structs, experimental
- `martensite_plugin::runtime::PluginState` — structs, experimental
- `martensite_plugin::runtime::RING_BUFFER_REGION_SIZE` — constants, experimental
- `martensite_plugin::security` — modules, experimental
- `martensite_plugin::security::Capability` — enums, experimental
- `martensite_plugin::security::CapabilitySet` — structs, experimental
- `martensite_plugin::security::PluginBuilder` — structs, experimental

### `martensite-reactive` (v0.17.0)

**71 public items** — 58 stable, 13 experimental. Categories: 2 enums, 5 functions, 8 modules, 39 re-exports, 13 structs, 1 traits, 3 type aliases.

Experimental areas: devtools-timemachine write journal + signal snapshots.

- `martensite_reactive::CycleError` — re-exports, stable
- `martensite_reactive::Effect` — re-exports, stable
- `martensite_reactive::FastBuildHasher` — re-exports, stable
- `martensite_reactive::FastHasher` — re-exports, stable
- `martensite_reactive::JournalGuard` — re-exports, experimental, feature `devtools-timemachine`
- `martensite_reactive::Memo` — re-exports, stable
- `martensite_reactive::NodeColor` — re-exports, stable
- `martensite_reactive::NodeEvaluator` — re-exports, stable
- `martensite_reactive::NodeRecord` — re-exports, stable
- `martensite_reactive::ReactiveError` — re-exports, stable
- `martensite_reactive::ReactiveRuntime` — re-exports, stable
- `martensite_reactive::SchedulerState` — re-exports, stable
- `martensite_reactive::Signal` — re-exports, stable
- `martensite_reactive::SignalId` — re-exports, stable
- `martensite_reactive::SignalSnapshot` — re-exports, experimental, feature `devtools-timemachine`
- `martensite_reactive::SourceJournal` — re-exports, experimental, feature `devtools-timemachine`
- `martensite_reactive::WriteRecord` — re-exports, experimental, feature `devtools-timemachine`
- `martensite_reactive::batch` — re-exports, stable
- `martensite_reactive::create_effect` — re-exports, stable
- `martensite_reactive::create_memo` — re-exports, stable
- `martensite_reactive::create_signal` — re-exports, stable
- `martensite_reactive::cycle` — modules, stable
- `martensite_reactive::cycle::CycleError` — structs, stable
- `martensite_reactive::cycle::NodeColor` — enums, stable
- `martensite_reactive::effect` — modules, stable
- `martensite_reactive::effect::Effect` — structs, stable
- `martensite_reactive::flush` — re-exports, stable
- `martensite_reactive::journal` — modules, experimental, feature `devtools-timemachine`
- `martensite_reactive::journal::JournalGuard` — structs, experimental, feature `devtools-timemachine`
- `martensite_reactive::journal::SignalSnapshot` — structs, experimental, feature `devtools-timemachine`
- `martensite_reactive::journal::SourceJournal` — structs, experimental, feature `devtools-timemachine`
- `martensite_reactive::journal::WriteRecord` — structs, experimental, feature `devtools-timemachine`
- `martensite_reactive::memo` — modules, stable
- `martensite_reactive::memo::Memo` — structs, stable
- `martensite_reactive::prelude` — modules, stable
- `martensite_reactive::prelude::CycleError` — re-exports, stable
- `martensite_reactive::prelude::Effect` — re-exports, stable
- `martensite_reactive::prelude::JournalGuard` — re-exports, experimental, feature `devtools-timemachine`
- `martensite_reactive::prelude::Memo` — re-exports, stable
- `martensite_reactive::prelude::NodeColor` — re-exports, stable
- `martensite_reactive::prelude::ReactiveError` — re-exports, stable
- `martensite_reactive::prelude::ReactiveRuntime` — re-exports, stable
- `martensite_reactive::prelude::Signal` — re-exports, stable
- `martensite_reactive::prelude::SignalId` — re-exports, stable
- `martensite_reactive::prelude::SignalSnapshot` — re-exports, experimental, feature `devtools-timemachine`
- `martensite_reactive::prelude::SourceJournal` — re-exports, experimental, feature `devtools-timemachine`
- `martensite_reactive::prelude::WriteRecord` — re-exports, experimental, feature `devtools-timemachine`
- `martensite_reactive::prelude::batch` — re-exports, stable
- `martensite_reactive::prelude::create_effect` — re-exports, stable
- `martensite_reactive::prelude::create_memo` — re-exports, stable
- `martensite_reactive::prelude::create_signal` — re-exports, stable
- `martensite_reactive::prelude::flush` — re-exports, stable
- `martensite_reactive::runtime` — modules, stable
- `martensite_reactive::runtime::NodeEvaluator` — traits, stable
- `martensite_reactive::runtime::ReactiveError` — enums, stable
- `martensite_reactive::runtime::ReactiveRuntime` — structs, stable
- `martensite_reactive::runtime::batch` — functions, stable
- `martensite_reactive::runtime::create_effect` — functions, stable
- `martensite_reactive::runtime::create_memo` — functions, stable
- `martensite_reactive::runtime::create_signal` — functions, stable
- `martensite_reactive::runtime::flush` — functions, stable
- `martensite_reactive::scheduler` — modules, stable
- `martensite_reactive::scheduler::EvaluatorHandle` — type aliases, stable
- `martensite_reactive::scheduler::FastBuildHasher` — type aliases, stable
- `martensite_reactive::scheduler::FastHasher` — structs, stable
- `martensite_reactive::scheduler::NodeRecord` — structs, stable
- `martensite_reactive::scheduler::PendingEvaluation` — type aliases, stable
- `martensite_reactive::scheduler::SchedulerState` — structs, stable
- `martensite_reactive::signal` — modules, stable
- `martensite_reactive::signal::Signal` — structs, stable
- `martensite_reactive::signal::SignalId` — structs, stable

### `martensite-render` (v0.17.0)

**38 public items** — 38 stable, 0 experimental. Categories: 2 constants, 2 enums, 4 functions, 1 glob re-exports, 5 modules, 19 re-exports, 4 structs, 1 traits.

- `martensite_render::BezPath` — re-exports, stable
- `martensite_render::ClearMode` — enums, stable
- `martensite_render::FontResource` — re-exports, stable
- `martensite_render::GlyphInstance` — re-exports, stable
- `martensite_render::GlyphRun` — re-exports, stable
- `martensite_render::GradientStop` — re-exports, stable
- `martensite_render::GradientStops` — re-exports, stable
- `martensite_render::PaintCommand` — re-exports, stable
- `martensite_render::PaintList` — re-exports, stable
- `martensite_render::PaintSegment` — re-exports, stable
- `martensite_render::PathBuilder` — re-exports, stable
- `martensite_render::Point` — re-exports, stable
- `martensite_render::PresentationError` — re-exports, stable
- `martensite_render::Rect` — re-exports, stable
- `martensite_render::RenderBackend` — traits, stable
- `martensite_render::SoftbufferPresenter` — re-exports, stable
- `martensite_render::TinySkiaBackend` — re-exports, stable
- `martensite_render::VelloRenderer` — re-exports, stable
- `martensite_render::diff` — modules, stable
- `martensite_render::diff::DiffResult` — structs, stable
- `martensite_render::diff::EDGE_SSIM_THRESHOLD` — constants, stable
- `martensite_render::diff::FILL_SSIM_THRESHOLD` — constants, stable
- `martensite_render::diff::perceptual_diff` — functions, stable
- `martensite_render::paint` — modules, stable
- `martensite_render::paint::* (from martensite_core::paint)` — glob re-exports, stable
- `martensite_render::present_rgba_to_softbuffer` — re-exports, stable
- `martensite_render::presentation` — modules, stable
- `martensite_render::presentation::PresentationError` — enums, stable
- `martensite_render::presentation::SoftbufferPresenter` — structs, stable
- `martensite_render::presentation::nonzero` — functions, stable
- `martensite_render::presentation::present_rgba_to_softbuffer` — functions, stable
- `martensite_render::presentation::rgba_to_softbuffer` — functions, stable
- `martensite_render::presentation_nonzero` — re-exports, stable
- `martensite_render::rgba_to_softbuffer` — re-exports, stable
- `martensite_render::tinyskia_backend` — modules, stable
- `martensite_render::tinyskia_backend::TinySkiaBackend` — structs, stable
- `martensite_render::vello_backend` — modules, stable
- `martensite_render::vello_backend::VelloRenderer` — structs, stable

### `martensite-shell` (v0.17.0)

**29 public items** — 24 stable, 5 experimental. Categories: 5 enums, 2 functions, 5 modules, 9 re-exports, 6 structs, 2 traits.

Experimental areas: StatusNotifierItem tray protocol, Linux-only; per-OS shell backends behind platform features.

- `martensite_shell::BackdropController` — re-exports, stable
- `martensite_shell::BackdropMaterial` — re-exports, stable
- `martensite_shell::BackdropMode` — re-exports, stable
- `martensite_shell::ShellEvent` — re-exports, stable
- `martensite_shell::ShellEventQueue` — re-exports, stable
- `martensite_shell::SnapLayout` — re-exports, stable
- `martensite_shell::StubBackdropController` — re-exports, stable
- `martensite_shell::VibrancyMaterial` — re-exports, stable
- `martensite_shell::Window` — re-exports, stable
- `martensite_shell::backdrop` — modules, stable
- `martensite_shell::backdrop::BackdropController` — traits, stable
- `martensite_shell::backdrop::BackdropMaterial` — enums, stable
- `martensite_shell::backdrop::BackdropMode` — enums, stable
- `martensite_shell::backdrop::ShellThemeTokens` — structs, stable
- `martensite_shell::backdrop::StubBackdropController` — structs, stable
- `martensite_shell::backdrop::VibrancyMaterial` — enums, stable
- `martensite_shell::backdrop::Window` — traits, stable
- `martensite_shell::backdrop::resolve_backdrop_material` — functions, stable
- `martensite_shell::backdrop::resolve_vibrancy_material` — functions, stable
- `martensite_shell::event` — modules, stable
- `martensite_shell::event::ShellEvent` — enums, stable
- `martensite_shell::event::ShellEventQueue` — structs, stable
- `martensite_shell::platform_impl` — modules, experimental
- `martensite_shell::platform_impl::macos` — modules, experimental, feature `macos-backend`
- `martensite_shell::platform_impl::macos::AppearanceObserver` — structs, experimental, feature `macos-backend`
- `martensite_shell::platform_impl::macos::EffectViewKind` — enums, experimental, feature `macos-backend`
- `martensite_shell::platform_impl::macos::MacosBackdropController` — structs, experimental, feature `macos-backend`
- `martensite_shell::snap` — modules, stable
- `martensite_shell::snap::SnapLayout` — structs, stable

### `martensite-test` (v0.17.0)

**36 public items** — 36 stable, 0 experimental. Categories: 4 constants, 2 enums, 3 functions, 4 modules, 15 re-exports, 8 structs.

- `martensite_test::FRAME_120FPS` — re-exports, stable
- `martensite_test::FRAME_30FPS` — re-exports, stable
- `martensite_test::FRAME_60FPS` — re-exports, stable
- `martensite_test::FuzzConfig` — re-exports, stable
- `martensite_test::FuzzEngine` — re-exports, stable
- `martensite_test::FuzzError` — re-exports, stable
- `martensite_test::FuzzReport` — re-exports, stable
- `martensite_test::FuzzTarget` — re-exports, stable
- `martensite_test::GoldenError` — re-exports, stable
- `martensite_test::GoldenImages` — re-exports, stable
- `martensite_test::HeadlessHarness` — re-exports, stable
- `martensite_test::ImageBuffer` — re-exports, stable
- `martensite_test::VirtualClock` — re-exports, stable
- `martensite_test::dssim` — modules, stable
- `martensite_test::dssim::BLOCK_SIZE` — constants, stable
- `martensite_test::dssim::ImageBuffer` — structs, stable
- `martensite_test::dssim::dssim` — functions, stable
- `martensite_test::dssim::images_match` — functions, stable
- `martensite_test::fuzz` — modules, stable
- `martensite_test::fuzz::FuzzConfig` — structs, stable
- `martensite_test::fuzz::FuzzEngine` — structs, stable
- `martensite_test::fuzz::FuzzError` — structs, stable
- `martensite_test::fuzz::FuzzReport` — structs, stable
- `martensite_test::fuzz::FuzzTarget` — enums, stable
- `martensite_test::fuzz::run_fuzz_campaign` — functions, stable
- `martensite_test::harness` — modules, stable
- `martensite_test::harness::GoldenError` — enums, stable
- `martensite_test::harness::GoldenImages` — structs, stable
- `martensite_test::harness::HeadlessHarness` — structs, stable
- `martensite_test::images_match` — re-exports, stable
- `martensite_test::run_fuzz_campaign` — re-exports, stable
- `martensite_test::virtual_clock` — modules, stable
- `martensite_test::virtual_clock::FRAME_120FPS` — constants, stable
- `martensite_test::virtual_clock::FRAME_30FPS` — constants, stable
- `martensite_test::virtual_clock::FRAME_60FPS` — constants, stable
- `martensite_test::virtual_clock::VirtualClock` — structs, stable

### `martensite-text` (v0.17.0)

**142 public items** — 142 stable, 0 experimental. Categories: 6 constants, 9 enums, 16 functions, 9 modules, 66 re-exports, 35 structs, 1 traits.

- `martensite_text::Attrs` — re-exports, stable
- `martensite_text::BidiDirection` — re-exports, stable
- `martensite_text::BidiMirrorMap` — re-exports, stable
- `martensite_text::BidiParagraph` — re-exports, stable
- `martensite_text::BidiResolved` — re-exports, stable
- `martensite_text::BidiRun` — re-exports, stable
- `martensite_text::BreakOpportunity` — re-exports, stable
- `martensite_text::Buffer` — re-exports, stable
- `martensite_text::CachedShape` — re-exports, stable
- `martensite_text::DEFAULT_MEMORY_BUDGET` — re-exports, stable
- `martensite_text::DirectionBits` — re-exports, stable
- `martensite_text::FallbackDecisionCache` — re-exports, stable
- `martensite_text::FallbackHash` — re-exports, stable
- `martensite_text::FallbackKey` — re-exports, stable
- `martensite_text::Family` — re-exports, stable
- `martensite_text::FontFaceInfo` — re-exports, stable
- `martensite_text::FontFallbackCache` — re-exports, stable
- `martensite_text::FontFallbackChain` — re-exports, stable
- `martensite_text::FontFallbackProvider` — re-exports, stable
- `martensite_text::FontId` — re-exports, stable
- `martensite_text::FontManager` — re-exports, stable
- `martensite_text::FontSizeBits` — re-exports, stable
- `martensite_text::FontSource` — re-exports, stable
- `martensite_text::FontStyle` — re-exports, stable
- `martensite_text::FontSystem` — re-exports, stable
- `martensite_text::GraphemeBreaker` — re-exports, stable
- `martensite_text::ImePositioner` — re-exports, stable
- `martensite_text::InstalledFontFallbackResolver` — re-exports, stable
- `martensite_text::LineBreaker` — re-exports, stable
- `martensite_text::LineHeightBits` — re-exports, stable
- `martensite_text::MaxWidthBits` — re-exports, stable
- `martensite_text::Metrics` — re-exports, stable
- `martensite_text::PlatformCascadeResolver` — re-exports, stable
- `martensite_text::ScriptTag` — re-exports, stable
- `martensite_text::ScrollKinematics` — re-exports, stable
- `martensite_text::ShapeCacheKey` — re-exports, stable
- `martensite_text::ShapedGlyph` — re-exports, stable
- `martensite_text::ShapedLine` — re-exports, stable
- `martensite_text::Shaper` — re-exports, stable
- `martensite_text::Shaping` — re-exports, stable
- `martensite_text::ShapingOptions` — re-exports, stable
- `martensite_text::TextHash` — re-exports, stable
- `martensite_text::TextMetrics` — re-exports, stable
- `martensite_text::TextShapeCache` — re-exports, stable
- `martensite_text::VerticalFeatureTags` — re-exports, stable
- `martensite_text::VerticalGlyphTransform` — re-exports, stable
- `martensite_text::VerticalMetrics` — re-exports, stable
- `martensite_text::VerticalOrientation` — re-exports, stable
- `martensite_text::VerticalRun` — re-exports, stable
- `martensite_text::Viewport` — re-exports, stable
- `martensite_text::WritingMode` — re-exports, stable
- `martensite_text::WritingModeBits` — re-exports, stable
- `martensite_text::apply_vertical_features` — re-exports, stable
- `martensite_text::bidi` — modules, stable
- `martensite_text::bidi::BidiDirection` — enums, stable
- `martensite_text::bidi::BidiMirrorMap` — structs, stable
- `martensite_text::bidi::BidiParagraph` — structs, stable
- `martensite_text::bidi::BidiResolved` — structs, stable
- `martensite_text::bidi::BidiRun` — structs, stable
- `martensite_text::cache` — modules, stable
- `martensite_text::cache::CachedShape` — structs, stable
- `martensite_text::cache::DEFAULT_MEMORY_BUDGET` — constants, stable
- `martensite_text::cache::DirectionBits` — enums, stable
- `martensite_text::cache::FallbackHash` — structs, stable
- `martensite_text::cache::FontSizeBits` — structs, stable
- `martensite_text::cache::LineHeightBits` — structs, stable
- `martensite_text::cache::MaxWidthBits` — structs, stable
- `martensite_text::cache::ShapeCacheKey` — structs, stable
- `martensite_text::cache::TextHash` — structs, stable
- `martensite_text::cache::TextShapeCache` — structs, stable
- `martensite_text::cache::WritingModeBits` — enums, stable
- `martensite_text::cascade` — modules, stable
- `martensite_text::cascade::FALLBACK_DECISION_CACHE_CAPACITY` — constants, stable
- `martensite_text::cascade::FallbackDecisionCache` — structs, stable
- `martensite_text::cascade::FallbackKey` — structs, stable
- `martensite_text::cascade::FontFallbackCache` — structs, stable
- `martensite_text::cascade::FontFallbackChain` — structs, stable
- `martensite_text::cascade::FontFallbackProvider` — traits, stable
- `martensite_text::cascade::InstalledFontFallbackResolver` — structs, stable
- `martensite_text::cascade::PlatformCascadeResolver` — structs, stable
- `martensite_text::cascade::ScriptTag` — enums, stable
- `martensite_text::cascade::classify_script` — functions, stable
- `martensite_text::cascade::dominant_script` — functions, stable
- `martensite_text::classify_script` — re-exports, stable
- `martensite_text::classify_vertical_orientation` — re-exports, stable
- `martensite_text::collect_vertical_runs` — re-exports, stable
- `martensite_text::font` — modules, stable
- `martensite_text::font::FontFaceInfo` — structs, stable
- `martensite_text::font::FontId` — structs, stable
- `martensite_text::font::FontManager` — structs, stable
- `martensite_text::font::FontSource` — enums, stable
- `martensite_text::font::FontStyle` — enums, stable
- `martensite_text::grapheme` — modules, stable
- `martensite_text::grapheme::GraphemeBreaker` — structs, stable
- `martensite_text::grapheme::grapheme_at` — functions, stable
- `martensite_text::grapheme::grapheme_boundary_before` — functions, stable
- `martensite_text::grapheme::grapheme_byte_offset` — functions, stable
- `martensite_text::grapheme::grapheme_clusters` — functions, stable
- `martensite_text::grapheme::grapheme_count` — functions, stable
- `martensite_text::grapheme::next_grapheme_boundary` — functions, stable
- `martensite_text::grapheme::prev_grapheme_boundary` — functions, stable
- `martensite_text::grapheme_at` — re-exports, stable
- `martensite_text::grapheme_boundary_before` — re-exports, stable
- `martensite_text::grapheme_byte_offset` — re-exports, stable
- `martensite_text::grapheme_clusters` — re-exports, stable
- `martensite_text::grapheme_count` — re-exports, stable
- `martensite_text::ime` — modules, stable
- `martensite_text::ime::DEFAULT_DAMPING_FACTOR` — constants, stable
- `martensite_text::ime::DEFAULT_DECELERATION` — constants, stable
- `martensite_text::ime::DEFAULT_EMA_ALPHA` — constants, stable
- `martensite_text::ime::ImePositioner` — structs, stable
- `martensite_text::ime::SCROLL_VELOCITY_THRESHOLD` — constants, stable
- `martensite_text::ime::ScrollKinematics` — structs, stable
- `martensite_text::ime::Viewport` — structs, stable
- `martensite_text::line_break` — modules, stable
- `martensite_text::line_break::BreakOpportunity` — enums, stable
- `martensite_text::line_break::LineBreaker` — structs, stable
- `martensite_text::measure_text` — re-exports, stable
- `martensite_text::measure_text_with_attrs` — re-exports, stable
- `martensite_text::next_grapheme_boundary` — re-exports, stable
- `martensite_text::prev_grapheme_boundary` — re-exports, stable
- `martensite_text::shape_text` — re-exports, stable
- `martensite_text::shaping` — modules, stable
- `martensite_text::shaping::ShapedGlyph` — structs, stable
- `martensite_text::shaping::ShapedLine` — structs, stable
- `martensite_text::shaping::Shaper` — structs, stable
- `martensite_text::shaping::ShapingOptions` — structs, stable
- `martensite_text::shaping::TextMetrics` — structs, stable
- `martensite_text::shaping::measure_text` — functions, stable
- `martensite_text::shaping::measure_text_with_attrs` — functions, stable
- `martensite_text::shaping::shape_text` — functions, stable
- `martensite_text::vertical` — modules, stable
- `martensite_text::vertical::VerticalFeatureTags` — structs, stable
- `martensite_text::vertical::VerticalGlyphTransform` — structs, stable
- `martensite_text::vertical::VerticalMetrics` — structs, stable
- `martensite_text::vertical::VerticalOrientation` — enums, stable
- `martensite_text::vertical::VerticalRun` — structs, stable
- `martensite_text::vertical::WritingMode` — enums, stable
- `martensite_text::vertical::apply_vertical_features` — functions, stable
- `martensite_text::vertical::classify_vertical_orientation` — functions, stable
- `martensite_text::vertical::collect_vertical_runs` — functions, stable
- `martensite_text::vertical::vertical_features` — functions, stable

### `martensite-theme` (v0.17.0)

**45 public items** — 45 stable, 0 experimental. Categories: 4 constants, 4 enums, 7 functions, 3 modules, 18 re-exports, 9 structs.

- `martensite_theme::Gamut` — re-exports, stable
- `martensite_theme::Oklab` — re-exports, stable
- `martensite_theme::Oklch` — re-exports, stable
- `martensite_theme::THEME_TRANSITION_WGSL` — re-exports, stable
- `martensite_theme::Theme` — re-exports, stable
- `martensite_theme::ThemeDictionary` — re-exports, stable
- `martensite_theme::ThemeDiff` — re-exports, stable
- `martensite_theme::ThemeMode` — re-exports, stable
- `martensite_theme::ThemeToken` — re-exports, stable
- `martensite_theme::ThemeTransition` — re-exports, stable
- `martensite_theme::ThemeUniformBuffer` — re-exports, stable
- `martensite_theme::ThemeUniforms` — re-exports, stable
- `martensite_theme::TokenKey` — re-exports, stable
- `martensite_theme::apca_contrast` — re-exports, stable
- `martensite_theme::gamut_map` — re-exports, stable
- `martensite_theme::gpu_transition` — modules, stable
- `martensite_theme::gpu_transition::COLOR_TOKEN_KEYS` — constants, stable
- `martensite_theme::gpu_transition::DEFAULT_TRANSITION_DURATION` — constants, stable
- `martensite_theme::gpu_transition::MAX_THEME_COLORS` — constants, stable
- `martensite_theme::gpu_transition::THEME_TRANSITION_WGSL` — constants, stable
- `martensite_theme::gpu_transition::ThemeTransition` — structs, stable
- `martensite_theme::gpu_transition::ThemeUniformBuffer` — structs, stable
- `martensite_theme::gpu_transition::ThemeUniforms` — structs, stable
- `martensite_theme::linear_to_srgb` — re-exports, stable
- `martensite_theme::oklab` — modules, stable
- `martensite_theme::oklab::Gamut` — enums, stable
- `martensite_theme::oklab::Oklab` — structs, stable
- `martensite_theme::oklab::Oklch` — structs, stable
- `martensite_theme::oklab::apca_contrast` — functions, stable
- `martensite_theme::oklab::gamut_map` — functions, stable
- `martensite_theme::oklab::linear_to_srgb` — functions, stable
- `martensite_theme::oklab::srgb_to_linear` — functions, stable
- `martensite_theme::oklab::wcag_contrast` — functions, stable
- `martensite_theme::srgb_to_linear` — re-exports, stable
- `martensite_theme::tokens` — modules, stable
- `martensite_theme::tokens::Theme` — structs, stable
- `martensite_theme::tokens::ThemeDictionary` — structs, stable
- `martensite_theme::tokens::ThemeDiff` — structs, stable
- `martensite_theme::tokens::ThemeMode` — enums, stable
- `martensite_theme::tokens::ThemeToken` — enums, stable
- `martensite_theme::tokens::TokenColorDelta` — structs, stable
- `martensite_theme::tokens::TokenKey` — enums, stable
- `martensite_theme::tokens::default_dark` — functions, stable
- `martensite_theme::tokens::default_light` — functions, stable
- `martensite_theme::wcag_contrast` — re-exports, stable

### `martensite-vello` (v0.10.0-martensite.1)

**49 public items** — 0 stable, 0 experimental, 49 vendored-upstream. Categories: 6 enums, 1 functions, 2 modules, 21 re-exports, 19 structs.

Vendored upstream fork: the entire surface tracks its upstream project and is EXPERIMENTAL until the maintenance policy in §5 pins a re-sync contract.

- `vello::AaConfig` — enums, vendored
- `vello::AaSupport` — structs, vendored
- `vello::DrawGlyphs` — re-exports, vendored
- `vello::Error` — enums, vendored
- `vello::FontEmbolden` — re-exports, vendored
- `vello::Glyph` — re-exports, vendored
- `vello::NormalizedCoord` — re-exports, vendored
- `vello::RenderParams` — structs, vendored
- `vello::Renderer` — structs, vendored
- `vello::RendererOptions` — structs, vendored
- `vello::Scene` — re-exports, vendored
- `vello::debug::DebugLayers` — structs, vendored
- `vello::kurbo` — re-exports, vendored
- `vello::low_level` — modules, vendored
- `vello::low_level::BindType` — re-exports, vendored
- `vello::low_level::BufferProxy` — re-exports, vendored
- `vello::low_level::BumpAllocators` — re-exports, vendored
- `vello::low_level::Command` — re-exports, vendored
- `vello::low_level::DebugLayers` — re-exports, vendored
- `vello::low_level::FullShaders` — re-exports, vendored
- `vello::low_level::ImageFormat` — re-exports, vendored
- `vello::low_level::ImageProxy` — re-exports, vendored
- `vello::low_level::Recording` — re-exports, vendored
- `vello::low_level::Render` — re-exports, vendored
- `vello::low_level::ResourceId` — re-exports, vendored
- `vello::low_level::ResourceProxy` — re-exports, vendored
- `vello::low_level::ShaderId` — re-exports, vendored
- `vello::peniko` — re-exports, vendored
- `vello::recording::BindType` — enums, vendored
- `vello::recording::BufferProxy` — structs, vendored
- `vello::recording::Command` — enums, vendored
- `vello::recording::DrawParams` — structs, vendored, feature `wgpu,wgpu_default,bump_estimate,debug_layers,wgpu-profiler`
- `vello::recording::ImageFormat` — enums, vendored
- `vello::recording::ImageProxy` — structs, vendored
- `vello::recording::Recording` — structs, vendored
- `vello::recording::ResourceId` — structs, vendored
- `vello::recording::ResourceProxy` — enums, vendored
- `vello::recording::ShaderId` — structs, vendored
- `vello::render::CapturedBuffers` — structs, vendored, feature `wgpu,wgpu_default,bump_estimate,debug_layers,wgpu-profiler`
- `vello::render::Render` — structs, vendored
- `vello::scene::DrawGlyphs` — structs, vendored
- `vello::scene::Scene` — structs, vendored
- `vello::shaders::FullShaders` — structs, vendored
- `vello::util` — modules, vendored
- `vello::util::DeviceHandle` — structs, vendored
- `vello::util::RenderContext` — structs, vendored
- `vello::util::RenderSurface` — structs, vendored
- `vello::util::block_on_wgpu` — functions, vendored
- `vello::wgpu` — re-exports, vendored

### `martensite-wgpu` (v0.17.0)

**87 public items** — 65 stable, 22 experimental. Categories: 5 constants, 13 enums, 8 functions, 8 modules, 39 re-exports, 13 structs, 1 type aliases.

Experimental areas: device-loss recovery API added in v0.11.0, still hardening; external-engine embedding bridge surface (v0.17.0); wasm/web backend glue, compile-verified only; zero-allocation theme transitions, perf-gated and young.

- `martensite_wgpu::BackdropMode` — re-exports, stable
- `martensite_wgpu::CompositeTarget` — re-exports, stable
- `martensite_wgpu::DEFAULT_FALLBACK_THRESHOLD` — re-exports, stable
- `martensite_wgpu::DEFAULT_MAX_RETRIES` — re-exports, stable
- `martensite_wgpu::DeviceStatus` — re-exports, stable
- `martensite_wgpu::ExternalError` — re-exports, stable
- `martensite_wgpu::FormatNegotiator` — re-exports, stable
- `martensite_wgpu::GpuContext` — re-exports, stable
- `martensite_wgpu::GpuContextError` — re-exports, stable
- `martensite_wgpu::MEDIA_YUV_EOTF_WGSL` — re-exports, stable
- `martensite_wgpu::OrchestratorConfig` — re-exports, stable
- `martensite_wgpu::OrchestratorError` — re-exports, stable
- `martensite_wgpu::PresentModePreference` — re-exports, stable
- `martensite_wgpu::RECOVERY_BUDGET` — re-exports, stable
- `martensite_wgpu::RecoveryError` — re-exports, stable
- `martensite_wgpu::RecoveryHarness` — re-exports, stable
- `martensite_wgpu::RecoveryMachine` — re-exports, stable
- `martensite_wgpu::RecoveryOutcome` — re-exports, stable
- `martensite_wgpu::RenderMode` — re-exports, stable
- `martensite_wgpu::RenderOrchestrator` — re-exports, stable
- `martensite_wgpu::SurfaceError` — re-exports, stable
- `martensite_wgpu::SurfaceWrapper` — re-exports, stable
- `martensite_wgpu::SurfaceWrapperError` — re-exports, stable
- `martensite_wgpu::THEME_UNIFORM_SIZE` — re-exports, stable
- `martensite_wgpu::TakenFrame` — re-exports, stable
- `martensite_wgpu::ThemeTransitionError` — re-exports, stable
- `martensite_wgpu::ThemeTransitionPipeline` — re-exports, stable
- `martensite_wgpu::VideoPipelineUniforms` — re-exports, stable
- `martensite_wgpu::VideoProcessor` — re-exports, stable
- `martensite_wgpu::VideoProcessorError` — re-exports, stable
- `martensite_wgpu::WgpuHost` — re-exports, stable
- `martensite_wgpu::backoff_duration` — re-exports, stable
- `martensite_wgpu::create_video_texture_view` — re-exports, stable
- `martensite_wgpu::device` — modules, stable
- `martensite_wgpu::device::GpuContext` — structs, stable
- `martensite_wgpu::device::GpuContextError` — enums, stable
- `martensite_wgpu::device::new_instance` — functions, stable
- `martensite_wgpu::device::platform_backends` — functions, stable
- `martensite_wgpu::external` — modules, experimental
- `martensite_wgpu::external::CompositeTarget` — structs, experimental
- `martensite_wgpu::external::ExternalError` — enums, experimental
- `martensite_wgpu::external::SourceAlpha` — re-exports, experimental
- `martensite_wgpu::external::TakenFrame` — structs, experimental
- `martensite_wgpu::external::WgpuHost` — structs, experimental
- `martensite_wgpu::interop` — modules, stable
- `martensite_wgpu::interop::FormatNegotiator` — structs, stable
- `martensite_wgpu::interop::MEDIA_YUV_EOTF_WGSL` — constants, stable
- `martensite_wgpu::interop::VideoPipelineUniforms` — structs, stable
- `martensite_wgpu::interop::VideoProcessor` — structs, stable
- `martensite_wgpu::interop::VideoProcessorError` — enums, stable
- `martensite_wgpu::interop::create_video_texture_view` — functions, stable
- `martensite_wgpu::interop::platform_import` — modules, stable
- `martensite_wgpu::interop::platform_import::import_cpu_memory` — functions, stable
- `martensite_wgpu::interop::platform_import::import_external_texture` — functions, stable
- `martensite_wgpu::interop::video_texture_views` — functions, stable
- `martensite_wgpu::new_instance` — re-exports, stable
- `martensite_wgpu::orchestrator` — modules, stable
- `martensite_wgpu::orchestrator::CpuFrameResolver` — type aliases, stable
- `martensite_wgpu::orchestrator::OrchestratorConfig` — structs, stable
- `martensite_wgpu::orchestrator::OrchestratorError` — enums, stable
- `martensite_wgpu::orchestrator::RenderMode` — enums, stable
- `martensite_wgpu::orchestrator::RenderOrchestrator` — structs, stable
- `martensite_wgpu::platform_backends` — re-exports, stable
- `martensite_wgpu::render_theme_transition` — re-exports, stable
- `martensite_wgpu::resilience` — modules, experimental
- `martensite_wgpu::resilience::DEFAULT_FALLBACK_THRESHOLD` — constants, experimental
- `martensite_wgpu::resilience::DEFAULT_MAX_RETRIES` — constants, experimental
- `martensite_wgpu::resilience::DeviceStatus` — enums, experimental
- `martensite_wgpu::resilience::RECOVERY_BUDGET` — constants, experimental
- `martensite_wgpu::resilience::RecoveryError` — enums, experimental
- `martensite_wgpu::resilience::RecoveryHarness` — structs, experimental
- `martensite_wgpu::resilience::RecoveryMachine` — structs, experimental
- `martensite_wgpu::resilience::RecoveryOutcome` — enums, experimental
- `martensite_wgpu::resilience::SurfaceError` — enums, experimental
- `martensite_wgpu::resilience::backoff_duration` — functions, experimental
- `martensite_wgpu::surface` — modules, stable
- `martensite_wgpu::surface::BackdropMode` — enums, stable
- `martensite_wgpu::surface::PresentModePreference` — enums, stable
- `martensite_wgpu::surface::SurfaceWrapper` — structs, stable
- `martensite_wgpu::surface::SurfaceWrapperError` — enums, stable
- `martensite_wgpu::theme_transition` — modules, experimental
- `martensite_wgpu::theme_transition::THEME_UNIFORM_SIZE` — constants, experimental
- `martensite_wgpu::theme_transition::ThemeTransitionError` — enums, experimental
- `martensite_wgpu::theme_transition::ThemeTransitionPipeline` — structs, experimental
- `martensite_wgpu::theme_transition::render_theme_transition` — functions, experimental
- `martensite_wgpu::video_texture_views` — re-exports, stable
- `martensite_wgpu::wgpu` — re-exports, stable

### `martensite-window` (v0.17.0)

**102 public items** — 93 stable, 9 experimental. Categories: 1 constants, 10 enums, 15 functions, 10 modules, 44 re-exports, 22 structs.

Experimental areas: Kalman stylus filtering, latency-gated and young; client-side decoration hit-testing, young shell surface; wasm/web backend glue, compile-verified only.

- `martensite_window::ActiveEventLoop` — re-exports, stable
- `martensite_window::AffineTransform` — re-exports, stable
- `martensite_window::ClipShape` — re-exports, stable
- `martensite_window::CsdController` — re-exports, stable
- `martensite_window::CsdHitRegion` — re-exports, stable
- `martensite_window::DpiScale` — re-exports, stable
- `martensite_window::DropAction` — re-exports, stable
- `martensite_window::DropEvent` — re-exports, stable
- `martensite_window::EventDispatchOutcome` — re-exports, stable
- `martensite_window::EventRouter` — re-exports, stable
- `martensite_window::HitTestResult` — re-exports, stable
- `martensite_window::HitTester` — re-exports, stable
- `martensite_window::MacOSWindowAttributes` — re-exports, stable
- `martensite_window::ModifierKeys` — re-exports, stable
- `martensite_window::MouseTracker` — re-exports, stable
- `martensite_window::PointerCapture` — re-exports, stable
- `martensite_window::PointerEvent` — re-exports, stable
- `martensite_window::PointerId` — re-exports, stable
- `martensite_window::PointerKind` — re-exports, stable
- `martensite_window::PointerState` — re-exports, stable
- `martensite_window::Quiescence` — re-exports, stable
- `martensite_window::QuiescentApp` — re-exports, stable
- `martensite_window::RequestError` — re-exports, stable
- `martensite_window::RoundedRect` — re-exports, stable
- `martensite_window::SurfaceLifecycle` — re-exports, stable
- `martensite_window::Window` — re-exports, stable
- `martensite_window::WindowAttributes` — re-exports, stable
- `martensite_window::WindowEntry` — re-exports, stable
- `martensite_window::WindowEvent` — re-exports, stable
- `martensite_window::WindowEventOutcome` — re-exports, stable
- `martensite_window::WindowId` — re-exports, stable
- `martensite_window::WindowKey` — re-exports, stable
- `martensite_window::WindowManager` — re-exports, stable
- `martensite_window::WindowsWindowAttributes` — re-exports, stable
- `martensite_window::convert_drop_event` — re-exports, stable
- `martensite_window::convert_modifiers` — re-exports, stable
- `martensite_window::convert_modifiers_state` — re-exports, stable
- `martensite_window::convert_mouse_button` — re-exports, stable
- `martensite_window::convert_window_event` — re-exports, stable
- `martensite_window::csd` — modules, experimental
- `martensite_window::csd::CsdController` — structs, experimental
- `martensite_window::csd::CsdHitRegion` — enums, experimental
- `martensite_window::csd::csd_region_for_point` — functions, experimental
- `martensite_window::csd_region_for_point` — re-exports, experimental
- `martensite_window::dpi` — modules, stable
- `martensite_window::dpi::DpiScale` — structs, stable
- `martensite_window::event` — modules, stable
- `martensite_window::event::DropAction` — enums, stable
- `martensite_window::event::DropEvent` — enums, stable
- `martensite_window::event::EventDispatchOutcome` — enums, stable
- `martensite_window::event::EventRouter` — structs, stable
- `martensite_window::event::ModifierKeys` — structs, stable
- `martensite_window::event::MouseButton` — enums, stable
- `martensite_window::event::MouseTracker` — structs, stable
- `martensite_window::event::PointerCapture` — structs, stable
- `martensite_window::event::PointerEvent` — structs, stable
- `martensite_window::event::PointerId` — structs, stable
- `martensite_window::event::PointerKind` — enums, stable
- `martensite_window::event::PointerState` — enums, stable
- `martensite_window::event::convert_drop_event` — functions, stable
- `martensite_window::event::convert_modifiers` — functions, stable
- `martensite_window::event::convert_modifiers_state` — functions, stable
- `martensite_window::event::convert_mouse_button` — functions, stable
- `martensite_window::event::convert_pointer_button` — functions, stable
- `martensite_window::event::convert_window_event` — functions, stable
- `martensite_window::event::widget_event_for_pointer` — functions, stable
- `martensite_window::hit_test` — modules, stable
- `martensite_window::hit_test::AffineTransform` — structs, stable
- `martensite_window::hit_test::ClipShape` — enums, stable
- `martensite_window::hit_test::HitTestResult` — structs, stable
- `martensite_window::hit_test::HitTester` — structs, stable
- `martensite_window::hit_test::RoundedRect` — structs, stable
- `martensite_window::hit_test::SINGULAR_EPSILON` — constants, stable
- `martensite_window::hit_test::point_in_rect` — functions, stable
- `martensite_window::hit_test::point_in_shape` — functions, stable
- `martensite_window::ime` — modules, stable
- `martensite_window::ime::ImeHint` — re-exports, stable
- `martensite_window::ime::ImePurpose` — re-exports, stable
- `martensite_window::ime::ImeSurroundingText` — re-exports, stable
- `martensite_window::ime::disable_ime` — functions, stable
- `martensite_window::ime::enable_ime` — functions, stable
- `martensite_window::ime::ime_capabilities` — functions, stable
- `martensite_window::ime::update_ime` — functions, stable
- `martensite_window::lifecycle` — modules, stable
- `martensite_window::lifecycle::SurfaceLifecycle` — enums, stable
- `martensite_window::lifecycle::surface_lifecycle` — functions, stable
- `martensite_window::manager` — modules, stable
- `martensite_window::manager::WindowEntry` — structs, stable
- `martensite_window::manager::WindowEventOutcome` — enums, stable
- `martensite_window::manager::WindowKey` — structs, stable
- `martensite_window::manager::WindowManager` — structs, stable
- `martensite_window::quiescent` — modules, stable
- `martensite_window::quiescent::Quiescence` — structs, stable
- `martensite_window::quiescent::QuiescentApp` — structs, stable
- `martensite_window::stylus` — modules, experimental
- `martensite_window::stylus::FilteredStylusState` — structs, experimental
- `martensite_window::stylus::KalmanStylus` — structs, experimental
- `martensite_window::stylus::StylusState` — structs, experimental
- `martensite_window::surface_lifecycle` — re-exports, stable
- `martensite_window::window_attributes` — modules, stable
- `martensite_window::window_attributes::MacOSWindowAttributes` — structs, stable
- `martensite_window::window_attributes::WindowsWindowAttributes` — structs, stable

<!-- END GENERATED: api-surface -->

## 3. Experimental Surface & Feature Gates

The `unstable-*` inventory is finalized in
[`API_FREEZE_AUDIT.md` §3](API_FREEZE_AUDIT.md#3-feature-flag-inventory-unstable--final-inventory):
no `unstable-*` flags exist pre-freeze (0.x SemVer already permits minor
bumps to break), opt-in subsystems sit behind the named feature gates
listed there, and everything else classified EXPERIMENTAL above carries a
documented rationale in its crate section. Post-1.0, new incubating API
ships behind `unstable-*` flags per `DEPRECATION_POLICY.md` §3.

## 4. Semver Enforcement

The `semver-checks` job in `.github/workflows/ci.yml` runs
`cargo-semver-checks` per crate against the crates.io baseline; see
`API_FREEZE_AUDIT.md` §4 for the baseline and strictness strategy.
