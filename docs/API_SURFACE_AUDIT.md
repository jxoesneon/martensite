# API Surface Audit — v0.18.0

Machine-enumerated public API surface of every publishable Martensite
crate, with a STABLE / EXPERIMENTAL / VENDORED classification per item.
Policy context lives in [`API_FREEZE_AUDIT.md`](API_FREEZE_AUDIT.md);
deprecation and MSRV rules in [`DEPRECATION_POLICY.md`](DEPRECATION_POLICY.md).

## 1. Methodology

`python3 scripts/api_surface_audit.py` regenerates the section between the
`GENERATED` markers below:

1. `cargo metadata` selects the publishable crates — workspace members
   under `crates/` without `publish = false` (47 crates). Excluded:
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
   `martensite`. Each re-export is resolved through `paths`/`index` to the
   canonical item it targets and **inherits that item's classification**,
   so module-scoped rules (e.g. `::docking`) also cover root aliases like
   `martensite_blessed::DockArea` whose paths drop the module segment.
   Whole-crate aliases (`pub use martensite_wgpu as wgpu`) whose target
   crate is only partially experimental carry a caveat in the crate
   section — the alias tag cannot reflect the experimental sub-surface.
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
- Re-export aliases inherit the strictest tier along their spelled path,
  so the same item can legitimately carry different tiers by path:
  `martensite_core::PointerButton` is STABLE while
  `martensite::prelude::PointerButton` is EXPERIMENTAL because that alias
  routes through `martensite_engine_bridge`. The EXPERIMENTAL tag on the
  alias means the alias path itself is not frozen — not that the item
  changed.

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

_47 publishable crates. Generated by `python3 scripts/api_surface_audit.py` from rustdoc JSON (stable + `RUSTC_BOOTSTRAP`); do not edit between the markers._

**Workspace total: 2948 public items, 2521 stable / 291 experimental / 136 vendored.**

| Crate | Items | Stable | Experimental | Vendored | Notes |
| :--- | ---: | ---: | ---: | ---: | :--- |
| `martensite` | 1237 | 1209 | 28 | 0 | crate-root aliases expose experimental sub-surface |
| `martensite-access` | 117 | 117 | 0 | 0 |  |
| `martensite-access-platform` | 0 | 0 | 0 | 0 | crate-level experimental |
| `martensite-accesskit-winit` | 1 | 0 | 0 | 1 | vendored fork |
| `martensite-assets` | 33 | 31 | 2 | 0 |  |
| `martensite-blessed` | 58 | 0 | 58 | 0 |  |
| `martensite-clipboard` | 29 | 29 | 0 | 0 |  |
| `martensite-clipboard-platform` | 4 | 4 | 0 | 0 |  |
| `martensite-core` | 116 | 109 | 7 | 0 |  |
| `martensite-cosmic-text` | 86 | 0 | 0 | 86 | vendored fork |
| `martensite-design-lint` | 55 | 55 | 0 | 0 |  |
| `martensite-devtools` | 25 | 18 | 7 | 0 |  |
| `martensite-dialog` | 20 | 20 | 0 | 0 |  |
| `martensite-dialog-platform` | 5 | 5 | 0 | 0 |  |
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
| `martensite-notify` | 18 | 18 | 0 | 0 |  |
| `martensite-notify-platform` | 4 | 4 | 0 | 0 |  |
| `martensite-pdf` | 22 | 22 | 0 | 0 |  |
| `martensite-pdf-platform` | 21 | 21 | 0 | 0 |  |
| `martensite-persist` | 14 | 14 | 0 | 0 |  |
| `martensite-plugin` | 31 | 0 | 31 | 0 |  |
| `martensite-print` | 28 | 28 | 0 | 0 |  |
| `martensite-print-platform` | 7 | 7 | 0 | 0 |  |
| `martensite-reactive` | 71 | 58 | 13 | 0 |  |
| `martensite-render` | 44 | 44 | 0 | 0 |  |
| `martensite-share` | 16 | 16 | 0 | 0 |  |
| `martensite-share-platform` | 7 | 7 | 0 | 0 |  |
| `martensite-shell` | 31 | 26 | 5 | 0 |  |
| `martensite-test` | 36 | 36 | 0 | 0 |  |
| `martensite-text` | 142 | 142 | 0 | 0 |  |
| `martensite-theme` | 45 | 45 | 0 | 0 |  |
| `martensite-vello` | 49 | 0 | 0 | 49 | vendored fork |
| `martensite-webview` | 12 | 12 | 0 | 0 |  |
| `martensite-webview-platform` | 9 | 9 | 0 | 0 |  |
| `martensite-wgpu` | 87 | 47 | 40 | 0 |  |
| `martensite-window` | 110 | 99 | 11 | 0 |  |

### `martensite` (v0.19.0)

**1237 public items** — 1209 stable, 28 experimental. Categories: 3 constants, 71 enums, 3 functions, 278 modules, 525 re-exports, 354 structs, 1 traits, 2 type aliases.

Experimental areas: decoder pipeline added in v0.16.0; external-engine widget embedding (v0.17.0); umbrella re-exports of experimental subsystems.

Caveat: `martensite::blessed` re-exports the whole of `martensite_blessed`, which contains experimental sub-surface (complex widget, API still settling; docking workspace framework added in v0.15.0) not reflected in the alias's stable tag.

Caveat: `martensite::core` re-exports the whole of `martensite_core`, which contains experimental sub-surface (devtools-timemachine arena snapshot/restore surface) not reflected in the alias's stable tag.

Caveat: `martensite::devtools` re-exports the whole of `martensite_devtools`, which contains experimental sub-surface (time-travel debugging behind devtools-timemachine (v0.17.0)) not reflected in the alias's stable tag.

Caveat: `martensite::media` re-exports the whole of `martensite_media`, which contains experimental sub-surface (decoder pipeline added in v0.16.0, hardware-verified backends pending) not reflected in the alias's stable tag.

Caveat: `martensite::reactive` re-exports the whole of `martensite_reactive`, which contains experimental sub-surface (devtools-timemachine write journal + signal snapshots) not reflected in the alias's stable tag.

Caveat: `martensite::shell` re-exports the whole of `martensite_shell`, which contains experimental sub-surface (StatusNotifierItem tray protocol, Linux-only; per-OS shell backends behind platform features) not reflected in the alias's stable tag.

Caveat: `martensite::wgpu` re-exports the whole of `martensite_wgpu`, which contains experimental sub-surface (device-loss recovery API added in v0.11.0, still hardening; external-engine embedding bridge surface (v0.17.0); wasm/web backend glue, compile-verified only; zero-allocation theme transitions, perf-gated and young) not reflected in the alias's stable tag.

Caveat: `martensite::window` re-exports the whole of `martensite_window`, which contains experimental sub-surface (Kalman stylus filtering, latency-gated and young; client-side decoration hit-testing, young shell surface; wasm/web backend glue, compile-verified only) not reflected in the alias's stable tag.

Large surface — digest by module (> 160 items):

| Module | Items | Stable | Experimental | Kinds |
| :--- | ---: | ---: | ---: | :--- |
| `(crate root)` | 25 | 24 | 1 | 4 modules, 21 re-exports |
| `app` | 4 | 4 | 0 | 1 constants, 3 structs |
| `prelude` | 79 | 63 | 16 | 79 re-exports |
| `text_paint` | 3 | 3 | 0 | 1 functions, 2 structs |
| `widgets` | 1126 | 1115 | 11 | 2 constants, 71 enums, 2 functions, 274 modules, 425 re-exports, 349 structs, 1 traits, 2 type aliases |

<details><summary>Full item list</summary>

- `martensite::access` — re-exports, stable
- `martensite::app` — modules, stable
- `martensite::app::App` — structs, stable
- `martensite::app::AppBuilder` — structs, stable
- `martensite::app::AppConfig` — structs, stable
- `martensite::app::DEFAULT_FALLBACK_TIMEOUT` — constants, stable
- `martensite::blessed` — re-exports, stable
- `martensite::clipboard` — re-exports, stable
- `martensite::core` — re-exports, stable
- `martensite::design_lint` — re-exports, stable
- `martensite::devtools` — re-exports, experimental
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
- `martensite::prelude::BindError` — re-exports, experimental
- `martensite::prelude::BridgeHandle` — re-exports, experimental
- `martensite::prelude::BridgeRegistry` — re-exports, experimental
- `martensite::prelude::ColdNode` — re-exports, stable
- `martensite::prelude::ColorRange` — re-exports, stable
- `martensite::prelude::ColorSpace` — re-exports, stable
- `martensite::prelude::Container` — re-exports, stable
- `martensite::prelude::CornerRadii` — re-exports, stable
- `martensite::prelude::CornerStyle` — re-exports, stable
- `martensite::prelude::CornerStyles` — re-exports, stable
- `martensite::prelude::DisplayProfile` — re-exports, stable
- `martensite::prelude::Dropdown` — re-exports, stable
- `martensite::prelude::Effect` — re-exports, stable
- `martensite::prelude::Engine` — re-exports, experimental
- `martensite::prelude::EngineContext` — re-exports, experimental
- `martensite::prelude::EngineEvent` — re-exports, experimental
- `martensite::prelude::EventContext` — re-exports, stable
- `martensite::prelude::EventResponse` — re-exports, stable
- `martensite::prelude::ExternalEngine` — re-exports, experimental
- `martensite::prelude::ExternalEngines` — re-exports, experimental
- `martensite::prelude::Flex` — re-exports, stable
- `martensite::prelude::FlexDirection` — re-exports, stable
- `martensite::prelude::Frame` — re-exports, experimental
- `martensite::prelude::FramePoll` — re-exports, experimental
- `martensite::prelude::FrameSync` — re-exports, experimental
- `martensite::prelude::FrameToken` — re-exports, experimental
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
- `martensite::prelude::PointerButton` — re-exports, experimental
- `martensite::prelude::RadioGroup` — re-exports, stable
- `martensite::prelude::ReactiveError` — re-exports, stable
- `martensite::prelude::ReactiveRuntime` — re-exports, stable
- `martensite::prelude::Rect` — re-exports, stable
- `martensite::prelude::ScRgb` — re-exports, stable
- `martensite::prelude::ScrollView` — re-exports, stable
- `martensite::prelude::SemanticAction` — re-exports, stable
- `martensite::prelude::Shape` — re-exports, stable
- `martensite::prelude::Signal` — re-exports, stable
- `martensite::prelude::Slider` — re-exports, stable
- `martensite::prelude::SliderOrientation` — re-exports, stable
- `martensite::prelude::SourceAlpha` — re-exports, experimental
- `martensite::prelude::SpringConfig` — re-exports, stable
- `martensite::prelude::SpringSolver` — re-exports, stable
- `martensite::prelude::Stack` — re-exports, stable
- `martensite::prelude::SurfaceId` — re-exports, experimental
- `martensite::prelude::TabActivation` — re-exports, stable
- `martensite::prelude::Tabs` — re-exports, stable
- `martensite::prelude::Text` — re-exports, stable
- `martensite::prelude::Theme` — re-exports, stable
- `martensite::prelude::ThemeToken` — re-exports, stable
- `martensite::prelude::TokenKey` — re-exports, stable
- `martensite::prelude::ToneMapOperator` — re-exports, stable
- `martensite::prelude::Tooltip` — re-exports, stable
- `martensite::prelude::TransferFunction` — re-exports, stable
- `martensite::prelude::VideoFit` — re-exports, stable
- `martensite::prelude::VideoFrameMetadata` — re-exports, stable
- `martensite::prelude::VideoPixelFormat` — re-exports, stable
- `martensite::prelude::VideoSurface` — re-exports, stable
- `martensite::prelude::Viewport` — re-exports, experimental
- `martensite::prelude::Widget` — re-exports, stable
- `martensite::prelude::WidgetArena` — re-exports, stable
- `martensite::prelude::WidgetEvent` — re-exports, stable
- `martensite::prelude::WidgetId` — re-exports, stable
- `martensite::prelude::batch` — re-exports, stable
- `martensite::prelude::create_effect` — re-exports, stable
- `martensite::prelude::create_memo` — re-exports, stable
- `martensite::prelude::create_signal` — re-exports, stable
- `martensite::prelude::flush` — re-exports, stable
- `martensite::reactive` — re-exports, stable
- `martensite::render` — re-exports, stable
- `martensite::shell` — re-exports, stable
- `martensite::text` — re-exports, stable
- `martensite::text_paint` — modules, stable
- `martensite::text_paint::SharedTextPainter` — structs, stable
- `martensite::text_paint::TextPainter` — structs, stable
- `martensite::text_paint::shared_painter` — functions, stable
- `martensite::theme` — re-exports, stable
- `martensite::wgpu` — re-exports, stable
- `martensite::widgets` — modules, stable
- `martensite::widgets::About` — re-exports, stable
- `martensite::widgets::Accordion` — re-exports, stable
- `martensite::widgets::ActionSheet` — re-exports, stable
- `martensite::widgets::ActionSheetResult` — re-exports, stable
- `martensite::widgets::ActivityRing` — re-exports, stable
- `martensite::widgets::AddressAction` — re-exports, stable
- `martensite::widgets::AddressBar` — re-exports, stable
- `martensite::widgets::Alarm` — re-exports, stable
- `martensite::widgets::AlarmPanel` — re-exports, stable
- `martensite::widgets::AlarmState` — re-exports, stable
- `martensite::widgets::AlertDialog` — re-exports, stable
- `martensite::widgets::AlertResult` — re-exports, stable
- `martensite::widgets::AlertRole` — re-exports, stable
- `martensite::widgets::AlertSeverity` — re-exports, stable
- `martensite::widgets::AlphaSlider` — re-exports, stable
- `martensite::widgets::AnalogClock` — re-exports, stable
- `martensite::widgets::Anchor` — re-exports, stable
- `martensite::widgets::AnchorItem` — re-exports, stable
- `martensite::widgets::AppEntry` — re-exports, stable
- `martensite::widgets::AppGrid` — re-exports, stable
- `martensite::widgets::AspectFrame` — re-exports, stable
- `martensite::widgets::Attachment` — re-exports, stable
- `martensite::widgets::Attendee` — re-exports, stable
- `martensite::widgets::AttendeeList` — re-exports, stable
- `martensite::widgets::AutoComplete` — re-exports, stable
- `martensite::widgets::Avatar` — re-exports, stable
- `martensite::widgets::AvatarGroup` — re-exports, stable
- `martensite::widgets::Badge` — re-exports, stable
- `martensite::widgets::Banner` — re-exports, stable
- `martensite::widgets::BarChart` — re-exports, stable
- `martensite::widgets::Barcode` — re-exports, stable
- `martensite::widgets::Battery` — re-exports, stable
- `martensite::widgets::BindError` — re-exports, experimental
- `martensite::widgets::Bone` — re-exports, stable
- `martensite::widgets::BottomSheet` — re-exports, stable
- `martensite::widgets::BoxPlot` — re-exports, stable
- `martensite::widgets::BoxSeries` — re-exports, stable
- `martensite::widgets::Breadcrumb` — re-exports, stable
- `martensite::widgets::BreakoutRooms` — re-exports, stable
- `martensite::widgets::BulletChart` — re-exports, stable
- `martensite::widgets::Burndown` — re-exports, stable
- `martensite::widgets::Button` — re-exports, stable
- `martensite::widgets::Calendar` — re-exports, stable
- `martensite::widgets::CalendarSelection` — re-exports, stable
- `martensite::widgets::CallControl` — re-exports, stable
- `martensite::widgets::CallControls` — re-exports, stable
- `martensite::widgets::Candle` — re-exports, stable
- `martensite::widgets::Candlestick` — re-exports, stable
- `martensite::widgets::CaptionCue` — re-exports, stable
- `martensite::widgets::CaptionStyle` — re-exports, stable
- `martensite::widgets::Captions` — re-exports, stable
- `martensite::widgets::Card` — re-exports, stable
- `martensite::widgets::CardAction` — re-exports, stable
- `martensite::widgets::CardDeck` — re-exports, stable
- `martensite::widgets::CardVariant` — re-exports, stable
- `martensite::widgets::Carousel` — re-exports, stable
- `martensite::widgets::Cascader` — re-exports, stable
- `martensite::widgets::CascaderOption` — re-exports, stable
- `martensite::widgets::ChangeKind` — re-exports, stable
- `martensite::widgets::ChatInput` — re-exports, stable
- `martensite::widgets::CheckBox` — re-exports, stable
- `martensite::widgets::CheckItem` — re-exports, stable
- `martensite::widgets::CheckList` — re-exports, stable
- `martensite::widgets::CheckState` — re-exports, stable
- `martensite::widgets::ChessBoard` — re-exports, stable
- `martensite::widgets::ChessClock` — re-exports, stable
- `martensite::widgets::Chip` — re-exports, stable
- `martensite::widgets::ChipGroup` — re-exports, stable
- `martensite::widgets::ChipKind` — re-exports, stable
- `martensite::widgets::ChipSelection` — re-exports, stable
- `martensite::widgets::ChooserMode` — re-exports, stable
- `martensite::widgets::Clamp` — re-exports, stable
- `martensite::widgets::ClipEntry` — re-exports, stable
- `martensite::widgets::ClipboardHistory` — re-exports, stable
- `martensite::widgets::ClockSide` — re-exports, stable
- `martensite::widgets::CodeView` — re-exports, stable
- `martensite::widgets::Color` — re-exports, stable
- `martensite::widgets::ColorButton` — re-exports, stable
- `martensite::widgets::ColorPalette` — re-exports, stable
- `martensite::widgets::ColorPicker` — re-exports, stable
- `martensite::widgets::ColorWheel` — re-exports, stable
- `martensite::widgets::CommandAction` — re-exports, stable
- `martensite::widgets::CommandLink` — re-exports, stable
- `martensite::widgets::CommandPalette` — re-exports, stable
- `martensite::widgets::Comment` — re-exports, stable
- `martensite::widgets::CommentThread` — re-exports, stable
- `martensite::widgets::Compass` — re-exports, stable
- `martensite::widgets::Confetti` — re-exports, stable
- `martensite::widgets::ConfirmResult` — re-exports, stable
- `martensite::widgets::Container` — re-exports, stable
- `martensite::widgets::ContextMenu` — re-exports, stable
- `martensite::widgets::ControlCenter` — re-exports, stable
- `martensite::widgets::CookieBanner` — re-exports, stable
- `martensite::widgets::CookieConsent` — re-exports, stable
- `martensite::widgets::Copyable` — re-exports, stable
- `martensite::widgets::Countdown` — re-exports, stable
- `martensite::widgets::CountdownRing` — re-exports, stable
- `martensite::widgets::Coverflow` — re-exports, stable
- `martensite::widgets::CropBox` — re-exports, stable
- `martensite::widgets::Crosshair` — re-exports, stable
- `martensite::widgets::CurveEditor` — re-exports, stable
- `martensite::widgets::DEFAULT_TOOLTIP_DELAY_MS` — re-exports, stable
- `martensite::widgets::Date` — re-exports, stable
- `martensite::widgets::DatePicker` — re-exports, stable
- `martensite::widgets::DescriptionItem` — re-exports, stable
- `martensite::widgets::Descriptions` — re-exports, stable
- `martensite::widgets::DeviceKind` — re-exports, stable
- `martensite::widgets::DevicePicker` — re-exports, stable
- `martensite::widgets::Dial` — re-exports, stable
- `martensite::widgets::Dialog` — re-exports, stable
- `martensite::widgets::DiffKind` — re-exports, stable
- `martensite::widgets::DiffView` — re-exports, stable
- `martensite::widgets::DigitalClock` — re-exports, stable
- `martensite::widgets::Disclosure` — re-exports, stable
- `martensite::widgets::Dock` — re-exports, stable
- `martensite::widgets::DockItem` — re-exports, stable
- `martensite::widgets::DownloadAction` — re-exports, stable
- `martensite::widgets::DownloadItem` — re-exports, stable
- `martensite::widgets::DownloadState` — re-exports, stable
- `martensite::widgets::Drawer` — re-exports, stable
- `martensite::widgets::Dropdown` — re-exports, stable
- `martensite::widgets::Emoji` — re-exports, stable
- `martensite::widgets::EmojiPicker` — re-exports, stable
- `martensite::widgets::EmptyState` — re-exports, stable
- `martensite::widgets::Equalizer` — re-exports, stable
- `martensite::widgets::ExpanderRow` — re-exports, stable
- `martensite::widgets::ExternalEngine` — re-exports, experimental
- `martensite::widgets::ExternalEngines` — re-exports, experimental
- `martensite::widgets::FileChooserButton` — re-exports, stable
- `martensite::widgets::Filmstrip` — re-exports, stable
- `martensite::widgets::FilterMode` — re-exports, stable
- `martensite::widgets::Fishbone` — re-exports, stable
- `martensite::widgets::Flashcard` — re-exports, stable
- `martensite::widgets::Flex` — re-exports, stable
- `martensite::widgets::FlexDirection` — re-exports, stable
- `martensite::widgets::FloatButton` — re-exports, stable
- `martensite::widgets::FlowBox` — re-exports, stable
- `martensite::widgets::FlowSelection` — re-exports, stable
- `martensite::widgets::FontButton` — re-exports, stable
- `martensite::widgets::FormField` — re-exports, stable
- `martensite::widgets::FramePoll` — re-exports, experimental
- `martensite::widgets::Fretboard` — re-exports, stable
- `martensite::widgets::FunnelChart` — re-exports, stable
- `martensite::widgets::Gantt` — re-exports, stable
- `martensite::widgets::GanttTask` — re-exports, stable
- `martensite::widgets::Gauge` — re-exports, stable
- `martensite::widgets::GradientEditor` — re-exports, stable
- `martensite::widgets::GradientStop` — re-exports, stable
- `martensite::widgets::GraphView` — re-exports, stable
- `martensite::widgets::Grid` — re-exports, stable
- `martensite::widgets::GridCell` — re-exports, stable
- `martensite::widgets::GroupBox` — re-exports, stable
- `martensite::widgets::HeaderBar` — re-exports, stable
- `martensite::widgets::HeatMap` — re-exports, stable
- `martensite::widgets::HeroAction` — re-exports, stable
- `martensite::widgets::HeroHeader` — re-exports, stable
- `martensite::widgets::HexView` — re-exports, stable
- `martensite::widgets::Histogram` — re-exports, stable
- `martensite::widgets::HoverCard` — re-exports, stable
- `martensite::widgets::HueSlider` — re-exports, stable
- `martensite::widgets::Image` — re-exports, stable
- `martensite::widgets::ImageFit` — re-exports, stable
- `martensite::widgets::ImageViewer` — re-exports, stable
- `martensite::widgets::InkCanvas` — re-exports, stable
- `martensite::widgets::InlineEdit` — re-exports, stable
- `martensite::widgets::Inspector` — re-exports, stable
- `martensite::widgets::InspectorRow` — re-exports, stable
- `martensite::widgets::InspectorSection` — re-exports, stable
- `martensite::widgets::IpInput` — re-exports, stable
- `martensite::widgets::Joystick` — re-exports, stable
- `martensite::widgets::JsonNode` — re-exports, stable
- `martensite::widgets::JsonValue` — re-exports, stable
- `martensite::widgets::JsonView` — re-exports, stable
- `martensite::widgets::Kanban` — re-exports, stable
- `martensite::widgets::Kbd` — re-exports, stable
- `martensite::widgets::KeyCapture` — re-exports, stable
- `martensite::widgets::KeyboardShortcuts` — re-exports, stable
- `martensite::widgets::Keypad` — re-exports, stable
- `martensite::widgets::LabelPosition` — re-exports, stable
- `martensite::widgets::Lamp` — re-exports, stable
- `martensite::widgets::LcdNumber` — re-exports, stable
- `martensite::widgets::LedMatrix` — re-exports, stable
- `martensite::widgets::Legend` — re-exports, stable
- `martensite::widgets::LegendEntry` — re-exports, stable
- `martensite::widgets::LevelBar` — re-exports, stable
- `martensite::widgets::LevelZone` — re-exports, stable
- `martensite::widgets::Lightbox` — re-exports, stable
- `martensite::widgets::LineChart` — re-exports, stable
- `martensite::widgets::LineSeries` — re-exports, stable
- `martensite::widgets::Link` — re-exports, stable
- `martensite::widgets::ListView` — re-exports, stable
- `martensite::widgets::LogLine` — re-exports, stable
- `martensite::widgets::LogSeverity` — re-exports, stable
- `martensite::widgets::LogView` — re-exports, stable
- `martensite::widgets::Magnifier` — re-exports, stable
- `martensite::widgets::Markdown` — re-exports, stable
- `martensite::widgets::Marquee` — re-exports, stable
- `martensite::widgets::Masonry` — re-exports, stable
- `martensite::widgets::MediaControls` — re-exports, stable
- `martensite::widgets::MediaView` — re-exports, stable
- `martensite::widgets::Mention` — re-exports, stable
- `martensite::widgets::Menu` — re-exports, stable
- `martensite::widgets::MenuBar` — re-exports, stable
- `martensite::widgets::MenuButton` — re-exports, stable
- `martensite::widgets::MenuItem` — re-exports, stable
- `martensite::widgets::MenuPath` — re-exports, stable
- `martensite::widgets::MenuState` — re-exports, stable
- `martensite::widgets::MergeRow` — re-exports, stable
- `martensite::widgets::MergeSide` — re-exports, stable
- `martensite::widgets::MergeView` — re-exports, stable
- `martensite::widgets::Message` — re-exports, stable
- `martensite::widgets::MessageList` — re-exports, stable
- `martensite::widgets::Metronome` — re-exports, stable
- `martensite::widgets::MindMap` — re-exports, stable
- `martensite::widgets::Minimap` — re-exports, stable
- `martensite::widgets::MoveDir` — re-exports, stable
- `martensite::widgets::NavDestination` — re-exports, stable
- `martensite::widgets::NavRail` — re-exports, stable
- `martensite::widgets::NavStack` — re-exports, stable
- `martensite::widgets::Notification` — re-exports, stable
- `martensite::widgets::NotificationCenter` — re-exports, stable
- `martensite::widgets::NowPlaying` — re-exports, stable
- `martensite::widgets::Odometer` — re-exports, stable
- `martensite::widgets::OrgChart` — re-exports, stable
- `martensite::widgets::OrgNode` — re-exports, stable
- `martensite::widgets::OtpInput` — re-exports, stable
- `martensite::widgets::PadGrid` — re-exports, stable
- `martensite::widgets::PageFlip` — re-exports, stable
- `martensite::widgets::PageHeader` — re-exports, stable
- `martensite::widgets::Pagination` — re-exports, stable
- `martensite::widgets::Participant` — re-exports, stable
- `martensite::widgets::PasswordStrength` — re-exports, stable
- `martensite::widgets::PatternLock` — re-exports, stable
- `martensite::widgets::PdfView` — re-exports, stable
- `martensite::widgets::PerfOverlay` — re-exports, stable
- `martensite::widgets::PianoKeys` — re-exports, stable
- `martensite::widgets::PieChart` — re-exports, stable
- `martensite::widgets::PieSlice` — re-exports, stable
- `martensite::widgets::Piece` — re-exports, stable
- `martensite::widgets::Pip` — re-exports, stable
- `martensite::widgets::PipsPager` — re-exports, stable
- `martensite::widgets::Plan` — re-exports, stable
- `martensite::widgets::Playlist` — re-exports, stable
- `martensite::widgets::PolarArea` — re-exports, stable
- `martensite::widgets::Poll` — re-exports, stable
- `martensite::widgets::PollOption` — re-exports, stable
- `martensite::widgets::Popconfirm` — re-exports, stable
- `martensite::widgets::Popover` — re-exports, stable
- `martensite::widgets::Presence` — re-exports, stable
- `martensite::widgets::PresenceStatus` — re-exports, stable
- `martensite::widgets::PricingTable` — re-exports, stable
- `martensite::widgets::ProgressBar` — re-exports, stable
- `martensite::widgets::PropertyEditor` — re-exports, stable
- `martensite::widgets::PropertyGrid` — re-exports, stable
- `martensite::widgets::PropertyRow` — re-exports, stable
- `martensite::widgets::PropertyRowKey` — re-exports, stable
- `martensite::widgets::PropertySection` — re-exports, stable
- `martensite::widgets::PullToRefresh` — re-exports, stable
- `martensite::widgets::QrCode` — re-exports, stable
- `martensite::widgets::Quadrant` — re-exports, stable
- `martensite::widgets::QuadrantItem` — re-exports, stable
- `martensite::widgets::RadarChart` — re-exports, stable
- `martensite::widgets::RadarSeries` — re-exports, stable
- `martensite::widgets::RadialMenu` — re-exports, stable
- `martensite::widgets::RadioGroup` — re-exports, stable
- `martensite::widgets::RadioOption` — re-exports, stable
- `martensite::widgets::RangeSlider` — re-exports, stable
- `martensite::widgets::RangeThumb` — re-exports, stable
- `martensite::widgets::Rating` — re-exports, stable
- `martensite::widgets::RatingSummary` — re-exports, stable
- `martensite::widgets::Reaction` — re-exports, stable
- `martensite::widgets::ReactionBar` — re-exports, stable
- `martensite::widgets::Release` — re-exports, stable
- `martensite::widgets::ReleaseNotes` — re-exports, stable
- `martensite::widgets::ResizeHandle` — re-exports, stable
- `martensite::widgets::ResultAction` — re-exports, stable
- `martensite::widgets::ResultPage` — re-exports, stable
- `martensite::widgets::ResultStatus` — re-exports, stable
- `martensite::widgets::Ribbon` — re-exports, stable
- `martensite::widgets::RibbonCorner` — re-exports, stable
- `martensite::widgets::Ring` — re-exports, stable
- `martensite::widgets::Room` — re-exports, stable
- `martensite::widgets::RubberBand` — re-exports, stable
- `martensite::widgets::Ruler` — re-exports, stable
- `martensite::widgets::RulerOrientation` — re-exports, stable
- `martensite::widgets::Sankey` — re-exports, stable
- `martensite::widgets::ScatterChart` — re-exports, stable
- `martensite::widgets::ScatterSeries` — re-exports, stable
- `martensite::widgets::ScratchCard` — re-exports, stable
- `martensite::widgets::ScrollBarWidget` — re-exports, stable
- `martensite::widgets::ScrollIndicator` — re-exports, stable
- `martensite::widgets::ScrollView` — re-exports, stable
- `martensite::widgets::SearchBar` — re-exports, stable
- `martensite::widgets::SearchField` — re-exports, stable
- `martensite::widgets::SecurityState` — re-exports, stable
- `martensite::widgets::Segment` — re-exports, stable
- `martensite::widgets::Segmented` — re-exports, stable
- `martensite::widgets::SelectionMode` — re-exports, stable
- `martensite::widgets::SelectionModel` — re-exports, experimental
- `martensite::widgets::Separator` — re-exports, stable
- `martensite::widgets::SettingsGroup` — re-exports, stable
- `martensite::widgets::SettingsRow` — re-exports, stable
- `martensite::widgets::Severity` — re-exports, stable
- `martensite::widgets::ShortcutGroup` — re-exports, stable
- `martensite::widgets::ShortcutRow` — re-exports, stable
- `martensite::widgets::Side` — re-exports, stable
- `martensite::widgets::SignalStrength` — re-exports, stable
- `martensite::widgets::Skeleton` — re-exports, stable
- `martensite::widgets::SkeletonShape` — re-exports, stable
- `martensite::widgets::Slider` — re-exports, stable
- `martensite::widgets::SliderOrientation` — re-exports, stable
- `martensite::widgets::SocialCard` — re-exports, stable
- `martensite::widgets::SortDir` — re-exports, stable
- `martensite::widgets::SparkStyle` — re-exports, stable
- `martensite::widgets::Sparkline` — re-exports, stable
- `martensite::widgets::Spectrum` — re-exports, stable
- `martensite::widgets::SpeedDial` — re-exports, stable
- `martensite::widgets::SpinBox` — re-exports, stable
- `martensite::widgets::Spinner` — re-exports, stable
- `martensite::widgets::Splash` — re-exports, stable
- `martensite::widgets::SplitButton` — re-exports, stable
- `martensite::widgets::SplitFlap` — re-exports, stable
- `martensite::widgets::SplitOrientation` — re-exports, stable
- `martensite::widgets::SplitView` — re-exports, stable
- `martensite::widgets::Stack` — re-exports, stable
- `martensite::widgets::StackLight` — re-exports, stable
- `martensite::widgets::Statistic` — re-exports, stable
- `martensite::widgets::Status` — re-exports, stable
- `martensite::widgets::StatusBar` — re-exports, stable
- `martensite::widgets::StatusDot` — re-exports, stable
- `martensite::widgets::StatusItem` — re-exports, stable
- `martensite::widgets::Step` — re-exports, stable
- `martensite::widgets::StepSequencer` — re-exports, stable
- `martensite::widgets::Steps` — re-exports, stable
- `martensite::widgets::Stopwatch` — re-exports, stable
- `martensite::widgets::StreamGraph` — re-exports, stable
- `martensite::widgets::StripChart` — re-exports, stable
- `martensite::widgets::Stroke` — re-exports, stable
- `martensite::widgets::Sunburst` — re-exports, stable
- `martensite::widgets::SunburstNode` — re-exports, stable
- `martensite::widgets::SwipeAction` — re-exports, stable
- `martensite::widgets::SwipeActions` — re-exports, stable
- `martensite::widgets::SwipeEdge` — re-exports, stable
- `martensite::widgets::Switch` — re-exports, stable
- `martensite::widgets::TOOLTIP_HOVER_GRACE_MS` — re-exports, stable
- `martensite::widgets::TabActivation` — re-exports, stable
- `martensite::widgets::TabItem` — re-exports, stable
- `martensite::widgets::Table` — re-exports, stable
- `martensite::widgets::TableAlign` — re-exports, stable
- `martensite::widgets::TableColumn` — re-exports, stable
- `martensite::widgets::Tabs` — re-exports, stable
- `martensite::widgets::TaskSwitcher` — re-exports, stable
- `martensite::widgets::Terminal` — re-exports, stable
- `martensite::widgets::Text` — re-exports, stable
- `martensite::widgets::TextArea` — re-exports, stable
- `martensite::widgets::TextInput` — re-exports, stable
- `martensite::widgets::ThemeOption` — re-exports, stable
- `martensite::widgets::ThemePicker` — re-exports, stable
- `martensite::widgets::Thermometer` — re-exports, stable
- `martensite::widgets::Thumbnail` — re-exports, stable
- `martensite::widgets::TickerItem` — re-exports, stable
- `martensite::widgets::TickerTape` — re-exports, stable
- `martensite::widgets::Ticket` — re-exports, stable
- `martensite::widgets::Time` — re-exports, stable
- `martensite::widgets::TimePicker` — re-exports, stable
- `martensite::widgets::Timeline` — re-exports, stable
- `martensite::widgets::TimelineDot` — re-exports, stable
- `martensite::widgets::TimelineItem` — re-exports, stable
- `martensite::widgets::Toast` — re-exports, stable
- `martensite::widgets::ToastHost` — re-exports, stable
- `martensite::widgets::ToggleButton` — re-exports, stable
- `martensite::widgets::TokenField` — re-exports, stable
- `martensite::widgets::ToolItem` — re-exports, stable
- `martensite::widgets::ToolPalette` — re-exports, stable
- `martensite::widgets::Toolbar` — re-exports, stable
- `martensite::widgets::ToolbarOverflow` — re-exports, stable
- `martensite::widgets::Tooltip` — re-exports, stable
- `martensite::widgets::TooltipBubble` — re-exports, stable
- `martensite::widgets::Tour` — re-exports, stable
- `martensite::widgets::TourStep` — re-exports, stable
- `martensite::widgets::Track` — re-exports, stable
- `martensite::widgets::Transfer` — re-exports, stable
- `martensite::widgets::TreeNode` — re-exports, stable
- `martensite::widgets::TreeSelect` — re-exports, stable
- `martensite::widgets::TreeView` — re-exports, stable
- `martensite::widgets::Treemap` — re-exports, stable
- `martensite::widgets::TreemapItem` — re-exports, stable
- `martensite::widgets::Trend` — re-exports, stable
- `martensite::widgets::Tuner` — re-exports, stable
- `martensite::widgets::TypingIndicator` — re-exports, stable
- `martensite::widgets::UnitCategory` — re-exports, stable
- `martensite::widgets::UnitConverter` — re-exports, stable
- `martensite::widgets::UpdatePrompt` — re-exports, stable
- `martensite::widgets::Venn` — re-exports, stable
- `martensite::widgets::VideoFit` — re-exports, stable
- `martensite::widgets::VideoGrid` — re-exports, stable
- `martensite::widgets::Viewport` — re-exports, stable
- `martensite::widgets::Violin` — re-exports, stable
- `martensite::widgets::VirtualKeyboard` — re-exports, stable
- `martensite::widgets::Volume` — re-exports, stable
- `martensite::widgets::VuMeter` — re-exports, stable
- `martensite::widgets::WaitingRoom` — re-exports, stable
- `martensite::widgets::Waterfall` — re-exports, stable
- `martensite::widgets::WaterfallEntry` — re-exports, stable
- `martensite::widgets::Watermark` — re-exports, stable
- `martensite::widgets::Waveform` — re-exports, stable
- `martensite::widgets::Weather` — re-exports, stable
- `martensite::widgets::WeatherCondition` — re-exports, stable
- `martensite::widgets::WebView` — re-exports, stable
- `martensite::widgets::WeekEvent` — re-exports, stable
- `martensite::widgets::WeekView` — re-exports, stable
- `martensite::widgets::WheelPicker` — re-exports, stable
- `martensite::widgets::WindowAction` — re-exports, stable
- `martensite::widgets::WindowControls` — re-exports, stable
- `martensite::widgets::Wizard` — re-exports, stable
- `martensite::widgets::WordCloud` — re-exports, stable
- `martensite::widgets::WorldClock` — re-exports, stable
- `martensite::widgets::XYPad` — re-exports, stable
- `martensite::widgets::ZoneEntry` — re-exports, stable
- `martensite::widgets::ZoomAction` — re-exports, stable
- `martensite::widgets::ZoomControls` — re-exports, stable
- `martensite::widgets::ZoomMode` — re-exports, stable
- `martensite::widgets::about` — modules, stable
- `martensite::widgets::about::About` — structs, stable
- `martensite::widgets::accordion` — modules, stable
- `martensite::widgets::accordion::Accordion` — structs, stable
- `martensite::widgets::action_sheet` — modules, stable
- `martensite::widgets::action_sheet::ActionSheet` — structs, stable
- `martensite::widgets::action_sheet::ActionSheetResult` — enums, stable
- `martensite::widgets::activity_ring` — modules, stable
- `martensite::widgets::activity_ring::ActivityRing` — structs, stable
- `martensite::widgets::activity_ring::Ring` — structs, stable
- `martensite::widgets::address_bar` — modules, stable
- `martensite::widgets::address_bar::AddressAction` — enums, stable
- `martensite::widgets::address_bar::AddressBar` — structs, stable
- `martensite::widgets::address_bar::SecurityState` — enums, stable
- `martensite::widgets::alarm_panel` — modules, stable
- `martensite::widgets::alarm_panel::Alarm` — structs, stable
- `martensite::widgets::alarm_panel::AlarmPanel` — structs, stable
- `martensite::widgets::alarm_panel::AlarmState` — enums, stable
- `martensite::widgets::alert_dialog` — modules, stable
- `martensite::widgets::alert_dialog::AlertDialog` — structs, stable
- `martensite::widgets::alert_dialog::AlertResult` — enums, stable
- `martensite::widgets::alert_dialog::AlertRole` — enums, stable
- `martensite::widgets::alert_dialog::AlertSeverity` — enums, stable
- `martensite::widgets::alpha_slider` — modules, stable
- `martensite::widgets::alpha_slider::AlphaSlider` — structs, stable
- `martensite::widgets::analog_clock` — modules, stable
- `martensite::widgets::analog_clock::AnalogClock` — structs, stable
- `martensite::widgets::anchor` — modules, stable
- `martensite::widgets::anchor::Anchor` — structs, stable
- `martensite::widgets::anchor::AnchorItem` — structs, stable
- `martensite::widgets::app_grid` — modules, stable
- `martensite::widgets::app_grid::AppEntry` — structs, stable
- `martensite::widgets::app_grid::AppGrid` — structs, stable
- `martensite::widgets::aspect_frame` — modules, stable
- `martensite::widgets::aspect_frame::AspectFrame` — structs, stable
- `martensite::widgets::attachment` — modules, stable
- `martensite::widgets::attachment::Attachment` — structs, stable
- `martensite::widgets::attendee_list` — modules, stable
- `martensite::widgets::attendee_list::Attendee` — structs, stable
- `martensite::widgets::attendee_list::AttendeeList` — structs, stable
- `martensite::widgets::auto_complete` — modules, stable
- `martensite::widgets::auto_complete::AutoComplete` — structs, stable
- `martensite::widgets::auto_complete::FilterMode` — enums, stable
- `martensite::widgets::avatar` — modules, stable
- `martensite::widgets::avatar::Avatar` — structs, stable
- `martensite::widgets::avatar_group` — modules, stable
- `martensite::widgets::avatar_group::AvatarGroup` — structs, stable
- `martensite::widgets::badge` — modules, stable
- `martensite::widgets::badge::Badge` — structs, stable
- `martensite::widgets::banner` — modules, stable
- `martensite::widgets::banner::Banner` — structs, stable
- `martensite::widgets::banner::Severity` — enums, stable
- `martensite::widgets::bar_chart` — modules, stable
- `martensite::widgets::bar_chart::BarChart` — structs, stable
- `martensite::widgets::barcode` — modules, stable
- `martensite::widgets::barcode::Barcode` — structs, stable
- `martensite::widgets::battery` — modules, stable
- `martensite::widgets::battery::Battery` — structs, stable
- `martensite::widgets::bottom_sheet` — modules, stable
- `martensite::widgets::bottom_sheet::BottomSheet` — structs, stable
- `martensite::widgets::box_plot` — modules, stable
- `martensite::widgets::box_plot::BoxPlot` — structs, stable
- `martensite::widgets::box_plot::BoxSeries` — structs, stable
- `martensite::widgets::breadcrumb` — modules, stable
- `martensite::widgets::breadcrumb::Breadcrumb` — structs, stable
- `martensite::widgets::breakout_rooms` — modules, stable
- `martensite::widgets::breakout_rooms::BreakoutRooms` — structs, stable
- `martensite::widgets::breakout_rooms::Room` — structs, stable
- `martensite::widgets::bullet_chart` — modules, stable
- `martensite::widgets::bullet_chart::BulletChart` — structs, stable
- `martensite::widgets::burndown` — modules, stable
- `martensite::widgets::burndown::Burndown` — structs, stable
- `martensite::widgets::button` — modules, stable
- `martensite::widgets::button::Button` — structs, stable
- `martensite::widgets::calendar` — modules, stable
- `martensite::widgets::calendar::Calendar` — structs, stable
- `martensite::widgets::calendar::CalendarSelection` — enums, stable
- `martensite::widgets::call_controls` — modules, stable
- `martensite::widgets::call_controls::CallControl` — enums, stable
- `martensite::widgets::call_controls::CallControls` — structs, stable
- `martensite::widgets::candlestick` — modules, stable
- `martensite::widgets::candlestick::Candle` — structs, stable
- `martensite::widgets::candlestick::Candlestick` — structs, stable
- `martensite::widgets::captions` — modules, stable
- `martensite::widgets::captions::CaptionCue` — structs, stable
- `martensite::widgets::captions::Captions` — structs, stable
- `martensite::widgets::card` — modules, stable
- `martensite::widgets::card::Card` — structs, stable
- `martensite::widgets::card::CardVariant` — enums, stable
- `martensite::widgets::card_deck` — modules, stable
- `martensite::widgets::card_deck::CardDeck` — structs, stable
- `martensite::widgets::carousel` — modules, stable
- `martensite::widgets::carousel::Carousel` — structs, stable
- `martensite::widgets::cascader` — modules, stable
- `martensite::widgets::cascader::Cascader` — structs, stable
- `martensite::widgets::cascader::CascaderOption` — structs, stable
- `martensite::widgets::chat_input` — modules, stable
- `martensite::widgets::chat_input::ChatInput` — structs, stable
- `martensite::widgets::check_list` — modules, stable
- `martensite::widgets::check_list::CheckItem` — structs, stable
- `martensite::widgets::check_list::CheckList` — structs, stable
- `martensite::widgets::checkbox` — modules, stable
- `martensite::widgets::checkbox::CheckBox` — structs, stable
- `martensite::widgets::checkbox::CheckState` — enums, stable
- `martensite::widgets::chess_board` — modules, stable
- `martensite::widgets::chess_board::ChessBoard` — structs, stable
- `martensite::widgets::chess_board::Piece` — enums, stable
- `martensite::widgets::chess_board::Side` — enums, stable
- `martensite::widgets::chess_clock` — modules, stable
- `martensite::widgets::chess_clock::ChessClock` — structs, stable
- `martensite::widgets::chess_clock::ClockSide` — enums, stable
- `martensite::widgets::chip` — modules, stable
- `martensite::widgets::chip::Chip` — structs, stable
- `martensite::widgets::chip::ChipKind` — enums, stable
- `martensite::widgets::chip_group` — modules, stable
- `martensite::widgets::chip_group::ChipGroup` — structs, stable
- `martensite::widgets::chip_group::ChipSelection` — enums, stable
- `martensite::widgets::clamp` — modules, stable
- `martensite::widgets::clamp::Clamp` — structs, stable
- `martensite::widgets::clipboard_history` — modules, stable
- `martensite::widgets::clipboard_history::ClipEntry` — structs, stable
- `martensite::widgets::clipboard_history::ClipboardHistory` — structs, stable
- `martensite::widgets::code_view` — modules, stable
- `martensite::widgets::code_view::CodeView` — structs, stable
- `martensite::widgets::color_button` — modules, stable
- `martensite::widgets::color_button::ColorButton` — structs, stable
- `martensite::widgets::color_palette` — modules, stable
- `martensite::widgets::color_palette::ColorPalette` — structs, stable
- `martensite::widgets::color_picker` — modules, stable
- `martensite::widgets::color_picker::Color` — structs, stable
- `martensite::widgets::color_picker::ColorPicker` — structs, stable
- `martensite::widgets::color_picker::hsv_to_rgb` — functions, stable
- `martensite::widgets::color_picker::rgb_to_hsv` — functions, stable
- `martensite::widgets::color_wheel` — modules, stable
- `martensite::widgets::color_wheel::ColorWheel` — structs, stable
- `martensite::widgets::command_link` — modules, stable
- `martensite::widgets::command_link::CommandLink` — structs, stable
- `martensite::widgets::command_palette` — modules, stable
- `martensite::widgets::command_palette::CommandAction` — structs, stable
- `martensite::widgets::command_palette::CommandPalette` — structs, stable
- `martensite::widgets::comment_thread` — modules, stable
- `martensite::widgets::comment_thread::Comment` — structs, stable
- `martensite::widgets::comment_thread::CommentThread` — structs, stable
- `martensite::widgets::compass` — modules, stable
- `martensite::widgets::compass::Compass` — structs, stable
- `martensite::widgets::confetti` — modules, stable
- `martensite::widgets::confetti::Confetti` — structs, stable
- `martensite::widgets::container` — modules, stable
- `martensite::widgets::container::Container` — structs, stable
- `martensite::widgets::context_menu` — modules, stable
- `martensite::widgets::context_menu::ContextMenu` — structs, stable
- `martensite::widgets::control_center` — modules, stable
- `martensite::widgets::control_center::ControlCenter` — structs, stable
- `martensite::widgets::cookie_banner` — modules, stable
- `martensite::widgets::cookie_banner::CookieBanner` — structs, stable
- `martensite::widgets::cookie_banner::CookieConsent` — enums, stable
- `martensite::widgets::copyable` — modules, stable
- `martensite::widgets::copyable::Copyable` — structs, stable
- `martensite::widgets::countdown` — modules, stable
- `martensite::widgets::countdown::Countdown` — structs, stable
- `martensite::widgets::countdown_ring` — modules, stable
- `martensite::widgets::countdown_ring::CountdownRing` — structs, stable
- `martensite::widgets::coverflow` — modules, stable
- `martensite::widgets::coverflow::Coverflow` — structs, stable
- `martensite::widgets::crop_box` — modules, stable
- `martensite::widgets::crop_box::CropBox` — structs, stable
- `martensite::widgets::crosshair` — modules, stable
- `martensite::widgets::crosshair::Crosshair` — structs, stable
- `martensite::widgets::curve_editor` — modules, stable
- `martensite::widgets::curve_editor::CurveEditor` — structs, stable
- `martensite::widgets::date_picker` — modules, stable
- `martensite::widgets::date_picker::Date` — structs, stable
- `martensite::widgets::date_picker::DatePicker` — structs, stable
- `martensite::widgets::descriptions` — modules, stable
- `martensite::widgets::descriptions::DescriptionItem` — structs, stable
- `martensite::widgets::descriptions::Descriptions` — structs, stable
- `martensite::widgets::device_picker` — modules, stable
- `martensite::widgets::device_picker::DeviceKind` — enums, stable
- `martensite::widgets::device_picker::DevicePicker` — structs, stable
- `martensite::widgets::dial` — modules, stable
- `martensite::widgets::dial::Dial` — structs, stable
- `martensite::widgets::dialog` — modules, stable
- `martensite::widgets::dialog::Dialog` — structs, stable
- `martensite::widgets::diff_view` — modules, stable
- `martensite::widgets::diff_view::DiffKind` — enums, stable
- `martensite::widgets::diff_view::DiffView` — structs, stable
- `martensite::widgets::digital_clock` — modules, stable
- `martensite::widgets::digital_clock::DigitalClock` — structs, stable
- `martensite::widgets::disclosure` — modules, stable
- `martensite::widgets::disclosure::Disclosure` — structs, stable
- `martensite::widgets::dock` — modules, stable
- `martensite::widgets::dock::Dock` — structs, stable
- `martensite::widgets::dock::DockItem` — structs, stable
- `martensite::widgets::download_item` — modules, stable
- `martensite::widgets::download_item::DownloadAction` — enums, stable
- `martensite::widgets::download_item::DownloadItem` — structs, stable
- `martensite::widgets::download_item::DownloadState` — enums, stable
- `martensite::widgets::drawer` — modules, stable
- `martensite::widgets::drawer::Drawer` — structs, stable
- `martensite::widgets::dropdown` — modules, stable
- `martensite::widgets::dropdown::Dropdown` — structs, stable
- `martensite::widgets::emoji_picker` — modules, stable
- `martensite::widgets::emoji_picker::Emoji` — structs, stable
- `martensite::widgets::emoji_picker::EmojiPicker` — structs, stable
- `martensite::widgets::empty_state` — modules, stable
- `martensite::widgets::empty_state::EmptyState` — structs, stable
- `martensite::widgets::equalizer` — modules, stable
- `martensite::widgets::equalizer::Equalizer` — structs, stable
- `martensite::widgets::expander_row` — modules, stable
- `martensite::widgets::expander_row::ExpanderRow` — structs, stable
- `martensite::widgets::external` — modules, experimental
- `martensite::widgets::external::BindError` — enums, experimental
- `martensite::widgets::external::ExternalEngine` — structs, experimental
- `martensite::widgets::external::ExternalEngines` — structs, experimental
- `martensite::widgets::external::FramePoll` — enums, experimental
- `martensite::widgets::file_chooser_button` — modules, stable
- `martensite::widgets::file_chooser_button::ChooserMode` — enums, stable
- `martensite::widgets::file_chooser_button::FileChooserButton` — structs, stable
- `martensite::widgets::filmstrip` — modules, stable
- `martensite::widgets::filmstrip::Filmstrip` — structs, stable
- `martensite::widgets::filmstrip::Thumbnail` — structs, stable
- `martensite::widgets::fishbone` — modules, stable
- `martensite::widgets::fishbone::Bone` — structs, stable
- `martensite::widgets::fishbone::Fishbone` — structs, stable
- `martensite::widgets::flashcard` — modules, stable
- `martensite::widgets::flashcard::Flashcard` — structs, stable
- `martensite::widgets::flex` — modules, stable
- `martensite::widgets::flex::CrossAxisAlignment` — enums, stable
- `martensite::widgets::flex::Flex` — structs, stable
- `martensite::widgets::flex::FlexDirection` — enums, stable
- `martensite::widgets::flex::MainAxisAlignment` — enums, stable
- `martensite::widgets::float_button` — modules, stable
- `martensite::widgets::float_button::FloatButton` — structs, stable
- `martensite::widgets::flow_box` — modules, stable
- `martensite::widgets::flow_box::FlowBox` — structs, stable
- `martensite::widgets::flow_box::FlowSelection` — enums, stable
- `martensite::widgets::font_button` — modules, stable
- `martensite::widgets::font_button::FontButton` — structs, stable
- `martensite::widgets::form_field` — modules, stable
- `martensite::widgets::form_field::FormField` — structs, stable
- `martensite::widgets::form_field::LabelPosition` — enums, stable
- `martensite::widgets::fretboard` — modules, stable
- `martensite::widgets::fretboard::Fretboard` — structs, stable
- `martensite::widgets::funnel_chart` — modules, stable
- `martensite::widgets::funnel_chart::FunnelChart` — structs, stable
- `martensite::widgets::gantt` — modules, stable
- `martensite::widgets::gantt::Gantt` — structs, stable
- `martensite::widgets::gantt::GanttTask` — structs, stable
- `martensite::widgets::gauge` — modules, stable
- `martensite::widgets::gauge::Gauge` — structs, stable
- `martensite::widgets::gradient_editor` — modules, stable
- `martensite::widgets::gradient_editor::GradientEditor` — structs, stable
- `martensite::widgets::gradient_editor::GradientStop` — structs, stable
- `martensite::widgets::graph_view` — modules, stable
- `martensite::widgets::graph_view::GraphView` — structs, stable
- `martensite::widgets::grid` — modules, stable
- `martensite::widgets::grid::Grid` — structs, stable
- `martensite::widgets::grid::GridCell` — structs, stable
- `martensite::widgets::group_box` — modules, stable
- `martensite::widgets::group_box::GroupBox` — structs, stable
- `martensite::widgets::header_bar` — modules, stable
- `martensite::widgets::header_bar::HeaderBar` — structs, stable
- `martensite::widgets::heat_map` — modules, stable
- `martensite::widgets::heat_map::HeatMap` — structs, stable
- `martensite::widgets::hero_header` — modules, stable
- `martensite::widgets::hero_header::HeroAction` — enums, stable
- `martensite::widgets::hero_header::HeroHeader` — structs, stable
- `martensite::widgets::hex_view` — modules, stable
- `martensite::widgets::hex_view::HexView` — structs, stable
- `martensite::widgets::histogram` — modules, stable
- `martensite::widgets::histogram::Histogram` — structs, stable
- `martensite::widgets::hover_card` — modules, stable
- `martensite::widgets::hover_card::HoverCard` — structs, stable
- `martensite::widgets::hsv_to_rgb` — re-exports, stable
- `martensite::widgets::hue_slider` — modules, stable
- `martensite::widgets::hue_slider::HueSlider` — structs, stable
- `martensite::widgets::image` — modules, stable
- `martensite::widgets::image::Image` — structs, stable
- `martensite::widgets::image::ImageFit` — enums, stable
- `martensite::widgets::image_viewer` — modules, stable
- `martensite::widgets::image_viewer::ImageViewer` — structs, stable
- `martensite::widgets::ink_canvas` — modules, stable
- `martensite::widgets::ink_canvas::InkCanvas` — structs, stable
- `martensite::widgets::ink_canvas::Stroke` — type aliases, stable
- `martensite::widgets::inline_edit` — modules, stable
- `martensite::widgets::inline_edit::InlineEdit` — structs, stable
- `martensite::widgets::inspector` — modules, stable
- `martensite::widgets::inspector::Inspector` — structs, stable
- `martensite::widgets::inspector::InspectorRow` — structs, stable
- `martensite::widgets::inspector::InspectorSection` — structs, stable
- `martensite::widgets::ip_input` — modules, stable
- `martensite::widgets::ip_input::IpInput` — structs, stable
- `martensite::widgets::joystick` — modules, stable
- `martensite::widgets::joystick::Joystick` — structs, stable
- `martensite::widgets::json_view` — modules, stable
- `martensite::widgets::json_view::JsonNode` — structs, stable
- `martensite::widgets::json_view::JsonValue` — enums, stable
- `martensite::widgets::json_view::JsonView` — structs, stable
- `martensite::widgets::kanban` — modules, stable
- `martensite::widgets::kanban::Kanban` — structs, stable
- `martensite::widgets::kbd` — modules, stable
- `martensite::widgets::kbd::Kbd` — structs, stable
- `martensite::widgets::key_capture` — modules, stable
- `martensite::widgets::key_capture::KeyCapture` — structs, stable
- `martensite::widgets::keyboard_shortcuts` — modules, stable
- `martensite::widgets::keyboard_shortcuts::KeyboardShortcuts` — structs, stable
- `martensite::widgets::keyboard_shortcuts::ShortcutGroup` — structs, stable
- `martensite::widgets::keyboard_shortcuts::ShortcutRow` — structs, stable
- `martensite::widgets::keypad` — modules, stable
- `martensite::widgets::keypad::Keypad` — structs, stable
- `martensite::widgets::lcd_number` — modules, stable
- `martensite::widgets::lcd_number::LcdNumber` — structs, stable
- `martensite::widgets::led_matrix` — modules, stable
- `martensite::widgets::led_matrix::LedMatrix` — structs, stable
- `martensite::widgets::legend` — modules, stable
- `martensite::widgets::legend::Legend` — structs, stable
- `martensite::widgets::legend::LegendEntry` — structs, stable
- `martensite::widgets::level_bar` — modules, stable
- `martensite::widgets::level_bar::LevelBar` — structs, stable
- `martensite::widgets::level_bar::LevelZone` — enums, stable
- `martensite::widgets::lightbox` — modules, stable
- `martensite::widgets::lightbox::Lightbox` — structs, stable
- `martensite::widgets::line_chart` — modules, stable
- `martensite::widgets::line_chart::LineChart` — structs, stable
- `martensite::widgets::line_chart::LineSeries` — structs, stable
- `martensite::widgets::link` — modules, stable
- `martensite::widgets::link::Link` — structs, stable
- `martensite::widgets::list_view` — modules, stable
- `martensite::widgets::list_view::ListView` — structs, stable
- `martensite::widgets::list_view::SelectionMode` — enums, stable
- `martensite::widgets::list_view::SelectionModel` — re-exports, experimental
- `martensite::widgets::log_view` — modules, stable
- `martensite::widgets::log_view::LogLine` — structs, stable
- `martensite::widgets::log_view::LogSeverity` — enums, stable
- `martensite::widgets::log_view::LogView` — structs, stable
- `martensite::widgets::magnifier` — modules, stable
- `martensite::widgets::magnifier::Magnifier` — structs, stable
- `martensite::widgets::markdown` — modules, stable
- `martensite::widgets::markdown::Markdown` — structs, stable
- `martensite::widgets::marquee` — modules, stable
- `martensite::widgets::marquee::Marquee` — structs, stable
- `martensite::widgets::masonry` — modules, stable
- `martensite::widgets::masonry::Masonry` — structs, stable
- `martensite::widgets::media` — modules, stable
- `martensite::widgets::media::MediaView` — structs, stable
- `martensite::widgets::media::VideoFit` — enums, stable
- `martensite::widgets::media_controls` — modules, stable
- `martensite::widgets::media_controls::MediaControls` — structs, stable
- `martensite::widgets::mention` — modules, stable
- `martensite::widgets::mention::Mention` — structs, stable
- `martensite::widgets::menu` — modules, stable
- `martensite::widgets::menu::Menu` — structs, stable
- `martensite::widgets::menu::MenuItem` — enums, stable
- `martensite::widgets::menu::MenuPath` — type aliases, stable
- `martensite::widgets::menu::MenuState` — structs, stable
- `martensite::widgets::menu_bar` — modules, stable
- `martensite::widgets::menu_bar::MenuBar` — structs, stable
- `martensite::widgets::menu_button` — modules, stable
- `martensite::widgets::menu_button::MenuButton` — structs, stable
- `martensite::widgets::merge_view` — modules, stable
- `martensite::widgets::merge_view::MergeRow` — structs, stable
- `martensite::widgets::merge_view::MergeSide` — enums, stable
- `martensite::widgets::merge_view::MergeView` — structs, stable
- `martensite::widgets::message_list` — modules, stable
- `martensite::widgets::message_list::Message` — structs, stable
- `martensite::widgets::message_list::MessageList` — structs, stable
- `martensite::widgets::metronome` — modules, stable
- `martensite::widgets::metronome::Metronome` — structs, stable
- `martensite::widgets::mind_map` — modules, stable
- `martensite::widgets::mind_map::MindMap` — structs, stable
- `martensite::widgets::minimap` — modules, stable
- `martensite::widgets::minimap::Minimap` — structs, stable
- `martensite::widgets::nav_rail` — modules, stable
- `martensite::widgets::nav_rail::NavDestination` — structs, stable
- `martensite::widgets::nav_rail::NavRail` — structs, stable
- `martensite::widgets::nav_stack` — modules, stable
- `martensite::widgets::nav_stack::NavStack` — structs, stable
- `martensite::widgets::notification_center` — modules, stable
- `martensite::widgets::notification_center::Notification` — structs, stable
- `martensite::widgets::notification_center::NotificationCenter` — structs, stable
- `martensite::widgets::now_playing` — modules, stable
- `martensite::widgets::now_playing::NowPlaying` — structs, stable
- `martensite::widgets::odometer` — modules, stable
- `martensite::widgets::odometer::Odometer` — structs, stable
- `martensite::widgets::org_chart` — modules, stable
- `martensite::widgets::org_chart::OrgChart` — structs, stable
- `martensite::widgets::org_chart::OrgNode` — structs, stable
- `martensite::widgets::otp_input` — modules, stable
- `martensite::widgets::otp_input::OtpInput` — structs, stable
- `martensite::widgets::pad_grid` — modules, stable
- `martensite::widgets::pad_grid::PadGrid` — structs, stable
- `martensite::widgets::page_flip` — modules, stable
- `martensite::widgets::page_flip::PageFlip` — structs, stable
- `martensite::widgets::page_header` — modules, stable
- `martensite::widgets::page_header::PageHeader` — structs, stable
- `martensite::widgets::pagination` — modules, stable
- `martensite::widgets::pagination::Pagination` — structs, stable
- `martensite::widgets::password_strength` — modules, stable
- `martensite::widgets::password_strength::PasswordStrength` — structs, stable
- `martensite::widgets::pattern_lock` — modules, stable
- `martensite::widgets::pattern_lock::PatternLock` — structs, stable
- `martensite::widgets::pdf_view` — modules, stable
- `martensite::widgets::pdf_view::PdfView` — structs, stable
- `martensite::widgets::pdf_view::ZoomMode` — enums, stable
- `martensite::widgets::perf_overlay` — modules, stable
- `martensite::widgets::perf_overlay::PerfOverlay` — structs, stable
- `martensite::widgets::piano_keys` — modules, stable
- `martensite::widgets::piano_keys::PianoKeys` — structs, stable
- `martensite::widgets::pie_chart` — modules, stable
- `martensite::widgets::pie_chart::PieChart` — structs, stable
- `martensite::widgets::pie_chart::PieSlice` — structs, stable
- `martensite::widgets::pip` — modules, stable
- `martensite::widgets::pip::Pip` — structs, stable
- `martensite::widgets::pips_pager` — modules, stable
- `martensite::widgets::pips_pager::PipsPager` — structs, stable
- `martensite::widgets::playlist` — modules, stable
- `martensite::widgets::playlist::Playlist` — structs, stable
- `martensite::widgets::playlist::Track` — structs, stable
- `martensite::widgets::polar_area` — modules, stable
- `martensite::widgets::polar_area::PolarArea` — structs, stable
- `martensite::widgets::poll` — modules, stable
- `martensite::widgets::poll::Poll` — structs, stable
- `martensite::widgets::poll::PollOption` — structs, stable
- `martensite::widgets::popconfirm` — modules, stable
- `martensite::widgets::popconfirm::ConfirmResult` — enums, stable
- `martensite::widgets::popconfirm::Popconfirm` — structs, stable
- `martensite::widgets::popover` — modules, stable
- `martensite::widgets::popover::Popover` — structs, stable
- `martensite::widgets::presence` — modules, stable
- `martensite::widgets::presence::Presence` — structs, stable
- `martensite::widgets::presence::PresenceStatus` — enums, stable
- `martensite::widgets::pricing_table` — modules, stable
- `martensite::widgets::pricing_table::Plan` — structs, stable
- `martensite::widgets::pricing_table::PricingTable` — structs, stable
- `martensite::widgets::progress` — modules, stable
- `martensite::widgets::progress::ProgressBar` — structs, stable
- `martensite::widgets::progress::Spinner` — structs, stable
- `martensite::widgets::property_grid` — modules, stable
- `martensite::widgets::property_grid::PropertyEditor` — enums, stable
- `martensite::widgets::property_grid::PropertyGrid` — structs, stable
- `martensite::widgets::property_grid::PropertyRow` — structs, stable
- `martensite::widgets::property_grid::PropertyRowKey` — traits, stable
- `martensite::widgets::property_grid::PropertySection` — structs, stable
- `martensite::widgets::pull_to_refresh` — modules, stable
- `martensite::widgets::pull_to_refresh::PullToRefresh` — structs, stable
- `martensite::widgets::qr_code` — modules, stable
- `martensite::widgets::qr_code::QrCode` — structs, stable
- `martensite::widgets::quadrant` — modules, stable
- `martensite::widgets::quadrant::Quadrant` — structs, stable
- `martensite::widgets::quadrant::QuadrantItem` — structs, stable
- `martensite::widgets::radar_chart` — modules, stable
- `martensite::widgets::radar_chart::RadarChart` — structs, stable
- `martensite::widgets::radar_chart::RadarSeries` — structs, stable
- `martensite::widgets::radial_menu` — modules, stable
- `martensite::widgets::radial_menu::RadialMenu` — structs, stable
- `martensite::widgets::radio` — modules, stable
- `martensite::widgets::radio::RadioGroup` — structs, stable
- `martensite::widgets::radio::RadioOption` — structs, stable
- `martensite::widgets::range_slider` — modules, stable
- `martensite::widgets::range_slider::RangeSlider` — structs, stable
- `martensite::widgets::range_slider::RangeThumb` — enums, stable
- `martensite::widgets::rating` — modules, stable
- `martensite::widgets::rating::Rating` — structs, stable
- `martensite::widgets::rating_summary` — modules, stable
- `martensite::widgets::rating_summary::RatingSummary` — structs, stable
- `martensite::widgets::reaction_bar` — modules, stable
- `martensite::widgets::reaction_bar::Reaction` — structs, stable
- `martensite::widgets::reaction_bar::ReactionBar` — structs, stable
- `martensite::widgets::release_notes` — modules, stable
- `martensite::widgets::release_notes::ChangeKind` — enums, stable
- `martensite::widgets::release_notes::Release` — structs, stable
- `martensite::widgets::release_notes::ReleaseNotes` — structs, stable
- `martensite::widgets::resize_handle` — modules, stable
- `martensite::widgets::resize_handle::ResizeHandle` — structs, stable
- `martensite::widgets::result_page` — modules, stable
- `martensite::widgets::result_page::ResultAction` — enums, stable
- `martensite::widgets::result_page::ResultPage` — structs, stable
- `martensite::widgets::result_page::ResultStatus` — enums, stable
- `martensite::widgets::rgb_to_hsv` — re-exports, stable
- `martensite::widgets::ribbon` — modules, stable
- `martensite::widgets::ribbon::Ribbon` — structs, stable
- `martensite::widgets::ribbon::RibbonCorner` — enums, stable
- `martensite::widgets::rubber_band` — modules, stable
- `martensite::widgets::rubber_band::RubberBand` — structs, stable
- `martensite::widgets::ruler` — modules, stable
- `martensite::widgets::ruler::Ruler` — structs, stable
- `martensite::widgets::ruler::RulerOrientation` — enums, stable
- `martensite::widgets::sankey` — modules, stable
- `martensite::widgets::sankey::Sankey` — structs, stable
- `martensite::widgets::scatter_chart` — modules, stable
- `martensite::widgets::scatter_chart::ScatterChart` — structs, stable
- `martensite::widgets::scatter_chart::ScatterSeries` — structs, stable
- `martensite::widgets::scratch_card` — modules, stable
- `martensite::widgets::scratch_card::ScratchCard` — structs, stable
- `martensite::widgets::scroll_indicator` — modules, stable
- `martensite::widgets::scroll_indicator::ScrollIndicator` — structs, stable
- `martensite::widgets::scrollview` — modules, stable
- `martensite::widgets::scrollview::ScrollBarWidget` — structs, stable
- `martensite::widgets::scrollview::ScrollView` — structs, stable
- `martensite::widgets::search_bar` — modules, stable
- `martensite::widgets::search_bar::SearchBar` — structs, stable
- `martensite::widgets::search_field` — modules, stable
- `martensite::widgets::search_field::SearchField` — structs, stable
- `martensite::widgets::segmented` — modules, stable
- `martensite::widgets::segmented::Segment` — structs, stable
- `martensite::widgets::segmented::Segmented` — structs, stable
- `martensite::widgets::separator` — modules, stable
- `martensite::widgets::separator::Separator` — structs, stable
- `martensite::widgets::settings_row` — modules, stable
- `martensite::widgets::settings_row::SettingsGroup` — structs, stable
- `martensite::widgets::settings_row::SettingsRow` — structs, stable
- `martensite::widgets::signal_strength` — modules, stable
- `martensite::widgets::signal_strength::SignalStrength` — structs, stable
- `martensite::widgets::skeleton` — modules, stable
- `martensite::widgets::skeleton::Skeleton` — structs, stable
- `martensite::widgets::skeleton::SkeletonShape` — enums, stable
- `martensite::widgets::slider` — modules, stable
- `martensite::widgets::slider::Slider` — structs, stable
- `martensite::widgets::slider::SliderOrientation` — enums, stable
- `martensite::widgets::social_card` — modules, stable
- `martensite::widgets::social_card::CardAction` — structs, stable
- `martensite::widgets::social_card::SocialCard` — structs, stable
- `martensite::widgets::sparkline` — modules, stable
- `martensite::widgets::sparkline::SparkStyle` — enums, stable
- `martensite::widgets::sparkline::Sparkline` — structs, stable
- `martensite::widgets::spectrum` — modules, stable
- `martensite::widgets::spectrum::Spectrum` — structs, stable
- `martensite::widgets::speed_dial` — modules, stable
- `martensite::widgets::speed_dial::SpeedDial` — structs, stable
- `martensite::widgets::spinbox` — modules, stable
- `martensite::widgets::spinbox::SpinBox` — structs, stable
- `martensite::widgets::splash` — modules, stable
- `martensite::widgets::splash::Splash` — structs, stable
- `martensite::widgets::split_button` — modules, stable
- `martensite::widgets::split_button::SplitButton` — structs, stable
- `martensite::widgets::split_flap` — modules, stable
- `martensite::widgets::split_flap::SplitFlap` — structs, stable
- `martensite::widgets::split_view` — modules, stable
- `martensite::widgets::split_view::SplitOrientation` — enums, stable
- `martensite::widgets::split_view::SplitView` — structs, stable
- `martensite::widgets::stack` — modules, stable
- `martensite::widgets::stack::Stack` — structs, stable
- `martensite::widgets::stack::StackAlignment` — enums, stable
- `martensite::widgets::stack_light` — modules, stable
- `martensite::widgets::stack_light::Lamp` — structs, stable
- `martensite::widgets::stack_light::StackLight` — structs, stable
- `martensite::widgets::statistic` — modules, stable
- `martensite::widgets::statistic::Statistic` — structs, stable
- `martensite::widgets::statistic::Trend` — enums, stable
- `martensite::widgets::status_bar` — modules, stable
- `martensite::widgets::status_bar::StatusBar` — structs, stable
- `martensite::widgets::status_bar::StatusItem` — enums, stable
- `martensite::widgets::status_dot` — modules, stable
- `martensite::widgets::status_dot::Status` — enums, stable
- `martensite::widgets::status_dot::StatusDot` — structs, stable
- `martensite::widgets::step_sequencer` — modules, stable
- `martensite::widgets::step_sequencer::StepSequencer` — structs, stable
- `martensite::widgets::steps` — modules, stable
- `martensite::widgets::steps::Step` — structs, stable
- `martensite::widgets::steps::Steps` — structs, stable
- `martensite::widgets::stopwatch` — modules, stable
- `martensite::widgets::stopwatch::Stopwatch` — structs, stable
- `martensite::widgets::stream_graph` — modules, stable
- `martensite::widgets::stream_graph::StreamGraph` — structs, stable
- `martensite::widgets::strip_chart` — modules, stable
- `martensite::widgets::strip_chart::StripChart` — structs, stable
- `martensite::widgets::sunburst` — modules, stable
- `martensite::widgets::sunburst::Sunburst` — structs, stable
- `martensite::widgets::sunburst::SunburstNode` — structs, stable
- `martensite::widgets::swipe_actions` — modules, stable
- `martensite::widgets::swipe_actions::SwipeAction` — structs, stable
- `martensite::widgets::swipe_actions::SwipeActions` — structs, stable
- `martensite::widgets::swipe_actions::SwipeEdge` — enums, stable
- `martensite::widgets::switch` — modules, stable
- `martensite::widgets::switch::Switch` — structs, stable
- `martensite::widgets::table` — modules, stable
- `martensite::widgets::table::SortDir` — enums, stable
- `martensite::widgets::table::Table` — structs, stable
- `martensite::widgets::table::TableAlign` — enums, stable
- `martensite::widgets::table::TableColumn` — structs, stable
- `martensite::widgets::tabs` — modules, stable
- `martensite::widgets::tabs::TabActivation` — enums, stable
- `martensite::widgets::tabs::TabItem` — structs, stable
- `martensite::widgets::tabs::Tabs` — structs, stable
- `martensite::widgets::task_switcher` — modules, stable
- `martensite::widgets::task_switcher::TaskSwitcher` — structs, stable
- `martensite::widgets::terminal` — modules, stable
- `martensite::widgets::terminal::Terminal` — structs, stable
- `martensite::widgets::text` — modules, stable
- `martensite::widgets::text::Text` — structs, stable
- `martensite::widgets::text_area` — modules, stable
- `martensite::widgets::text_area::TextArea` — structs, stable
- `martensite::widgets::text_input` — modules, stable
- `martensite::widgets::text_input::TextInput` — structs, stable
- `martensite::widgets::text_input::ValidationState` — enums, stable
- `martensite::widgets::theme_picker` — modules, stable
- `martensite::widgets::theme_picker::ThemeOption` — structs, stable
- `martensite::widgets::theme_picker::ThemePicker` — structs, stable
- `martensite::widgets::thermometer` — modules, stable
- `martensite::widgets::thermometer::Thermometer` — structs, stable
- `martensite::widgets::ticker_tape` — modules, stable
- `martensite::widgets::ticker_tape::TickerItem` — structs, stable
- `martensite::widgets::ticker_tape::TickerTape` — structs, stable
- `martensite::widgets::ticket` — modules, stable
- `martensite::widgets::ticket::Ticket` — structs, stable
- `martensite::widgets::time_picker` — modules, stable
- `martensite::widgets::time_picker::Time` — structs, stable
- `martensite::widgets::time_picker::TimePicker` — structs, stable
- `martensite::widgets::timeline` — modules, stable
- `martensite::widgets::timeline::Timeline` — structs, stable
- `martensite::widgets::timeline::TimelineDot` — enums, stable
- `martensite::widgets::timeline::TimelineItem` — structs, stable
- `martensite::widgets::toast` — modules, stable
- `martensite::widgets::toast::Toast` — structs, stable
- `martensite::widgets::toast::ToastHost` — structs, stable
- `martensite::widgets::toggle_button` — modules, stable
- `martensite::widgets::toggle_button::ToggleButton` — structs, stable
- `martensite::widgets::token_field` — modules, stable
- `martensite::widgets::token_field::TokenField` — structs, stable
- `martensite::widgets::tool_palette` — modules, stable
- `martensite::widgets::tool_palette::ToolItem` — structs, stable
- `martensite::widgets::tool_palette::ToolPalette` — structs, stable
- `martensite::widgets::toolbar` — modules, stable
- `martensite::widgets::toolbar::Toolbar` — structs, stable
- `martensite::widgets::toolbar_overflow` — modules, stable
- `martensite::widgets::toolbar_overflow::ToolbarOverflow` — structs, stable
- `martensite::widgets::tooltip` — modules, stable
- `martensite::widgets::tooltip::DEFAULT_TOOLTIP_DELAY_MS` — constants, stable
- `martensite::widgets::tooltip::TOOLTIP_HOVER_GRACE_MS` — constants, stable
- `martensite::widgets::tooltip::Tooltip` — structs, stable
- `martensite::widgets::tooltip::TooltipBubble` — structs, stable
- `martensite::widgets::tour` — modules, stable
- `martensite::widgets::tour::Tour` — structs, stable
- `martensite::widgets::tour::TourStep` — structs, stable
- `martensite::widgets::transfer` — modules, stable
- `martensite::widgets::transfer::MoveDir` — enums, stable
- `martensite::widgets::transfer::Transfer` — structs, stable
- `martensite::widgets::tree_select` — modules, stable
- `martensite::widgets::tree_select::TreeSelect` — structs, stable
- `martensite::widgets::tree_view` — modules, stable
- `martensite::widgets::tree_view::TreeNode` — structs, stable
- `martensite::widgets::tree_view::TreeView` — structs, stable
- `martensite::widgets::treemap` — modules, stable
- `martensite::widgets::treemap::Treemap` — structs, stable
- `martensite::widgets::treemap::TreemapItem` — structs, stable
- `martensite::widgets::tuner` — modules, stable
- `martensite::widgets::tuner::Tuner` — structs, stable
- `martensite::widgets::typing_indicator` — modules, stable
- `martensite::widgets::typing_indicator::TypingIndicator` — structs, stable
- `martensite::widgets::unit_converter` — modules, stable
- `martensite::widgets::unit_converter::UnitCategory` — enums, stable
- `martensite::widgets::unit_converter::UnitConverter` — structs, stable
- `martensite::widgets::update_prompt` — modules, stable
- `martensite::widgets::update_prompt::UpdatePrompt` — structs, stable
- `martensite::widgets::venn` — modules, stable
- `martensite::widgets::venn::Venn` — structs, stable
- `martensite::widgets::video_grid` — modules, stable
- `martensite::widgets::video_grid::Participant` — structs, stable
- `martensite::widgets::video_grid::VideoGrid` — structs, stable
- `martensite::widgets::viewport` — modules, stable
- `martensite::widgets::viewport::Viewport` — structs, stable
- `martensite::widgets::violin` — modules, stable
- `martensite::widgets::violin::Violin` — structs, stable
- `martensite::widgets::virtual_keyboard` — modules, stable
- `martensite::widgets::virtual_keyboard::VirtualKeyboard` — structs, stable
- `martensite::widgets::volume` — modules, stable
- `martensite::widgets::volume::Volume` — structs, stable
- `martensite::widgets::vu_meter` — modules, stable
- `martensite::widgets::vu_meter::VuMeter` — structs, stable
- `martensite::widgets::waiting_room` — modules, stable
- `martensite::widgets::waiting_room::WaitingRoom` — structs, stable
- `martensite::widgets::waterfall` — modules, stable
- `martensite::widgets::waterfall::Waterfall` — structs, stable
- `martensite::widgets::waterfall::WaterfallEntry` — enums, stable
- `martensite::widgets::watermark` — modules, stable
- `martensite::widgets::watermark::Watermark` — structs, stable
- `martensite::widgets::waveform` — modules, stable
- `martensite::widgets::waveform::Waveform` — structs, stable
- `martensite::widgets::weather` — modules, stable
- `martensite::widgets::weather::Weather` — structs, stable
- `martensite::widgets::weather::WeatherCondition` — enums, stable
- `martensite::widgets::webview` — modules, stable
- `martensite::widgets::webview::WebView` — structs, stable
- `martensite::widgets::week_view` — modules, stable
- `martensite::widgets::week_view::WeekEvent` — structs, stable
- `martensite::widgets::week_view::WeekView` — structs, stable
- `martensite::widgets::wheel_picker` — modules, stable
- `martensite::widgets::wheel_picker::WheelPicker` — structs, stable
- `martensite::widgets::window_controls` — modules, stable
- `martensite::widgets::window_controls::CaptionStyle` — enums, stable
- `martensite::widgets::window_controls::WindowAction` — enums, stable
- `martensite::widgets::window_controls::WindowControls` — structs, stable
- `martensite::widgets::wizard` — modules, stable
- `martensite::widgets::wizard::Wizard` — structs, stable
- `martensite::widgets::word_cloud` — modules, stable
- `martensite::widgets::word_cloud::WordCloud` — structs, stable
- `martensite::widgets::world_clock` — modules, stable
- `martensite::widgets::world_clock::WorldClock` — structs, stable
- `martensite::widgets::world_clock::ZoneEntry` — structs, stable
- `martensite::widgets::xy_pad` — modules, stable
- `martensite::widgets::xy_pad::XYPad` — structs, stable
- `martensite::widgets::zoom_controls` — modules, stable
- `martensite::widgets::zoom_controls::ZoomAction` — enums, stable
- `martensite::widgets::zoom_controls::ZoomControls` — structs, stable
- `martensite::window` — re-exports, stable

</details>

### `martensite-access` (v0.19.0)

**117 public items** — 117 stable, 0 experimental. Categories: 3 constants, 11 enums, 38 functions, 9 modules, 30 re-exports, 24 structs, 1 traits, 1 type aliases.

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
- `martensite_access::paint_audit` — modules, stable
- `martensite_access::paint_audit::DEFAULT_MIN_TEXT_SIZE_PT` — constants, stable
- `martensite_access::paint_audit::LARGE_TEXT_PT` — constants, stable
- `martensite_access::paint_audit::LintReporter` — structs, stable
- `martensite_access::paint_audit::LintSeverity` — enums, stable
- `martensite_access::paint_audit::LocaleProbe` — structs, stable
- `martensite_access::paint_audit::LocaleProbeFn` — type aliases, stable
- `martensite_access::paint_audit::PaintAuditConfig` — structs, stable
- `martensite_access::paint_audit::PaintLint` — structs, stable
- `martensite_access::paint_audit::PaintLintKind` — enums, stable
- `martensite_access::paint_audit::audit_paint_list` — functions, stable
- `martensite_access::paint_audit::audit_target_sizes` — functions, stable
- `martensite_access::paint_audit::audit_underflow` — functions, stable
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

### `martensite-access-platform` (v0.19.0)

**0 public items** — 0 stable, 0 experimental. Categories: —.

Whole crate classified EXPERIMENTAL (young integration surface, expected to settle during the RC line).


### `martensite-accesskit-winit` (v0.19.0)

**1 public items** — 0 stable, 0 experimental, 1 vendored-upstream. Categories: 1 structs.

Vendored upstream fork: the entire surface tracks its upstream project and is EXPERIMENTAL until the maintenance policy in `API_FREEZE_AUDIT.md` §5 pins a re-sync contract.

- `accesskit_winit::Adapter` — structs, vendored

### `martensite-assets` (v0.19.0)

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

### `martensite-blessed` (v0.19.0)

**58 public items** — 0 stable, 58 experimental. Categories: 8 enums, 5 modules, 25 re-exports, 19 structs, 1 traits.

Experimental areas: complex widget, API still settling; docking workspace framework added in v0.15.0.

- `martensite_blessed::AreaSeries` — re-exports, experimental
- `martensite_blessed::AudioWaveform` — re-exports, experimental
- `martensite_blessed::Chart` — re-exports, experimental
- `martensite_blessed::ChartBounds` — re-exports, experimental
- `martensite_blessed::CodeEditor` — re-exports, experimental
- `martensite_blessed::Cursor` — re-exports, experimental
- `martensite_blessed::DataTable` — re-exports, experimental
- `martensite_blessed::DockDragSession` — re-exports, experimental
- `martensite_blessed::DockDropZone` — re-exports, experimental
- `martensite_blessed::DockError` — re-exports, experimental
- `martensite_blessed::DockNode` — re-exports, experimental
- `martensite_blessed::DockNodeLayout` — re-exports, experimental
- `martensite_blessed::DockNodeLayoutKind` — re-exports, experimental
- `martensite_blessed::DockPanel` — re-exports, experimental
- `martensite_blessed::DockTree` — re-exports, experimental
- `martensite_blessed::HighlightedSpan` — re-exports, experimental
- `martensite_blessed::LineSeries` — re-exports, experimental
- `martensite_blessed::NodeId` — re-exports, experimental
- `martensite_blessed::Point` — re-exports, experimental
- `martensite_blessed::Rect` — re-exports, experimental
- `martensite_blessed::ScatterSeries` — re-exports, experimental
- `martensite_blessed::SplitDirection` — re-exports, experimental
- `martensite_blessed::TableStorage` — re-exports, experimental
- `martensite_blessed::TokenKind` — re-exports, experimental
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

### `martensite-clipboard` (v0.19.0)

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

### `martensite-clipboard-platform` (v0.19.0)

**4 public items** — 4 stable, 0 experimental. Categories: 1 functions, 1 modules, 1 structs, 1 traits.

- `martensite_clipboard_platform::ClipboardBackend` — traits, stable
- `martensite_clipboard_platform::macos` — modules, stable
- `martensite_clipboard_platform::macos::MacosBackend` — structs, stable
- `martensite_clipboard_platform::native_backend` — functions, stable

### `martensite-core` (v0.19.0)

**116 public items** — 109 stable, 7 experimental. Categories: 1 constants, 14 enums, 2 functions, 9 modules, 50 re-exports, 37 structs, 3 traits.

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
- `martensite_core::ImageData` — re-exports, stable
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
- `martensite_core::RenderMinimum` — re-exports, stable
- `martensite_core::SemanticAction` — re-exports, stable
- `martensite_core::SubtreeIter` — re-exports, stable
- `martensite_core::SurfaceId` — re-exports, stable
- `martensite_core::Theme` — re-exports, stable
- `martensite_core::ThemeToken` — re-exports, stable
- `martensite_core::TimemachineState` — re-exports, experimental, feature `devtools-timemachine`
- `martensite_core::TokenKey` — re-exports, stable
- `martensite_core::UnderflowPolicy` — re-exports, stable
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
- `martensite_core::overlay::AnchorEdge` — enums, stable
- `martensite_core::overlay::OverlayAnchor` — enums, stable
- `martensite_core::overlay::OverlayEntry` — structs, stable
- `martensite_core::overlay::OverlayLayer` — structs, stable
- `martensite_core::overlay::OverlayOptions` — structs, stable
- `martensite_core::overlay::ViewportAlign` — enums, stable
- `martensite_core::paint` — modules, stable
- `martensite_core::paint::FontResource` — structs, stable
- `martensite_core::paint::GlyphInstance` — structs, stable
- `martensite_core::paint::GlyphRun` — structs, stable
- `martensite_core::paint::GradientStop` — structs, stable
- `martensite_core::paint::GradientStops` — structs, stable
- `martensite_core::paint::ImageData` — structs, stable
- `martensite_core::paint::PaintCommand` — enums, stable
- `martensite_core::paint::PaintList` — structs, stable
- `martensite_core::paint::PaintSegment` — enums, stable
- `martensite_core::paint::PathBuilder` — structs, stable
- `martensite_core::paint::TextShaper` — traits, stable
- `martensite_core::shape` — modules, stable
- `martensite_core::shape::CornerRadii` — structs, stable
- `martensite_core::shape::CornerStyle` — enums, stable
- `martensite_core::shape::CornerStyles` — structs, stable
- `martensite_core::shape::Shape` — enums, stable
- `martensite_core::shape::point_in_polygon` — functions, stable
- `martensite_core::shape::winding_contains` — functions, stable
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
- `martensite_core::widget::RenderMinimum` — structs, stable
- `martensite_core::widget::SemanticAction` — enums, stable
- `martensite_core::widget::UnderflowPolicy` — enums, stable
- `martensite_core::widget::Widget` — traits, stable
- `martensite_core::widget::WidgetEvent` — enums, stable

### `martensite-cosmic-text` (v0.19.0-martensite.2)

**86 public items** — 0 stable, 0 experimental, 86 vendored-upstream. Categories: 16 enums, 1 functions, 15 glob re-exports, 1 modules, 1 re-exports, 49 structs, 3 traits.

Vendored upstream fork: the entire surface tracks its upstream project and is EXPERIMENTAL until the maintenance policy in `API_FREEZE_AUDIT.md` §5 pins a re-sync contract.

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

### `martensite-design-lint` (v0.19.0)

**55 public items** — 55 stable, 0 experimental. Categories: 8 enums, 6 functions, 25 re-exports, 15 structs, 1 traits.

- `martensite_design_lint::AlignEdge` — re-exports, stable
- `martensite_design_lint::AppliedFix` — re-exports, stable
- `martensite_design_lint::Confidence` — re-exports, stable
- `martensite_design_lint::FillStat` — re-exports, stable
- `martensite_design_lint::Finding` — re-exports, stable
- `martensite_design_lint::FixIteration` — re-exports, stable
- `martensite_design_lint::FixOp` — re-exports, stable
- `martensite_design_lint::FixOptions` — re-exports, stable
- `martensite_design_lint::FixReport` — re-exports, stable
- `martensite_design_lint::FixSafety` — re-exports, stable
- `martensite_design_lint::LintConfig` — re-exports, stable
- `martensite_design_lint::LintConfigError` — re-exports, stable
- `martensite_design_lint::LintFix` — re-exports, stable
- `martensite_design_lint::LintNode` — re-exports, stable
- `martensite_design_lint::LintReport` — re-exports, stable
- `martensite_design_lint::LintRule` — re-exports, stable
- `martensite_design_lint::LintScene` — re-exports, stable
- `martensite_design_lint::NodeKind` — re-exports, stable
- `martensite_design_lint::PathAllow` — re-exports, stable
- `martensite_design_lint::RuleSetting` — re-exports, stable
- `martensite_design_lint::Severity` — re-exports, stable
- `martensite_design_lint::Standard` — re-exports, stable
- `martensite_design_lint::TextStat` — re-exports, stable
- `martensite_design_lint::all_rules` — re-exports, stable
- `martensite_design_lint::autofix` — re-exports, stable
- `martensite_design_lint::config::LintConfig` — structs, stable
- `martensite_design_lint::config::LintConfigError` — enums, stable
- `martensite_design_lint::config::PathAllow` — structs, stable
- `martensite_design_lint::config::RuleSetting` — structs, stable
- `martensite_design_lint::fix::AlignEdge` — enums, stable
- `martensite_design_lint::fix::AppliedFix` — structs, stable
- `martensite_design_lint::fix::FixIteration` — structs, stable
- `martensite_design_lint::fix::FixOp` — enums, stable
- `martensite_design_lint::fix::FixOptions` — structs, stable
- `martensite_design_lint::fix::FixReport` — structs, stable
- `martensite_design_lint::fix::FixSafety` — enums, stable
- `martensite_design_lint::fix::LintFix` — structs, stable
- `martensite_design_lint::fix::autofix` — functions, stable
- `martensite_design_lint::lint` — functions, stable
- `martensite_design_lint::lint_paint_list` — functions, stable
- `martensite_design_lint::lint_with` — functions, stable
- `martensite_design_lint::report::Finding` — structs, stable
- `martensite_design_lint::report::LintReport` — structs, stable
- `martensite_design_lint::rule::Confidence` — enums, stable
- `martensite_design_lint::rule::LintRule` — traits, stable
- `martensite_design_lint::rule_catalog` — functions, stable
- `martensite_design_lint::rules::all_rules` — functions, stable
- `martensite_design_lint::scene::FillStat` — structs, stable
- `martensite_design_lint::scene::LintNode` — structs, stable
- `martensite_design_lint::scene::LintScene` — structs, stable
- `martensite_design_lint::scene::NodeKind` — enums, stable
- `martensite_design_lint::scene::TextStat` — structs, stable
- `martensite_design_lint::scene::Walk` — structs, stable
- `martensite_design_lint::severity::Severity` — enums, stable
- `martensite_design_lint::standard::Standard` — enums, stable

### `martensite-devtools` (v0.19.0)

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

### `martensite-dialog` (v0.19.0)

**20 public items** — 20 stable, 0 experimental. Categories: 2 enums, 1 functions, 2 modules, 9 re-exports, 4 structs, 2 traits.

- `martensite_dialog::DialogKind` — re-exports, stable
- `martensite_dialog::DialogOutcome` — re-exports, stable
- `martensite_dialog::DialogService` — re-exports, stable
- `martensite_dialog::FileDialogRequest` — re-exports, stable
- `martensite_dialog::FileFilter` — re-exports, stable
- `martensite_dialog::PlatformDialog` — re-exports, stable
- `martensite_dialog::ScriptedDialog` — re-exports, stable
- `martensite_dialog::StubDialog` — re-exports, stable
- `martensite_dialog::default_platform_dialog` — re-exports, stable
- `martensite_dialog::dialog` — modules, stable
- `martensite_dialog::dialog::DialogKind` — enums, stable
- `martensite_dialog::dialog::DialogOutcome` — enums, stable
- `martensite_dialog::dialog::DialogService` — traits, stable
- `martensite_dialog::dialog::FileDialogRequest` — structs, stable
- `martensite_dialog::dialog::FileFilter` — structs, stable
- `martensite_dialog::dialog::ScriptedDialog` — structs, stable
- `martensite_dialog::platform` — modules, stable
- `martensite_dialog::platform::PlatformDialog` — traits, stable
- `martensite_dialog::platform::StubDialog` — structs, stable
- `martensite_dialog::platform::default_platform_dialog` — functions, stable

### `martensite-dialog-platform` (v0.19.0)

**5 public items** — 5 stable, 0 experimental. Categories: 2 enums, 1 functions, 1 structs, 1 traits.

- `martensite_dialog_platform::DialogBackend` — traits, stable
- `martensite_dialog_platform::DialogReply` — enums, stable
- `martensite_dialog_platform::DialogSpec` — structs, stable
- `martensite_dialog_platform::SpecKind` — enums, stable
- `martensite_dialog_platform::native_backend` — functions, stable

### `martensite-dnd` (v0.19.0)

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

### `martensite-engine-bridge` (v0.19.0)

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

### `martensite-focus` (v0.19.0)

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

### `martensite-font-fallback` (v0.19.0)

**3 public items** — 3 stable, 0 experimental. Categories: 1 functions, 1 modules, 1 structs.

- `martensite_font_fallback::coretext` — modules, stable
- `martensite_font_fallback::coretext::CoreTextFontFallback` — structs, stable
- `martensite_font_fallback::native_provider` — functions, stable

### `martensite-history` (v0.19.0)

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

### `martensite-host` (v0.19.0)

**4 public items** — 0 stable, 4 experimental. Categories: 1 constants, 1 enums, 2 structs.

Whole crate classified EXPERIMENTAL (young integration surface, expected to settle during the RC line).

- `martensite_host::GuestLibrary` — structs, experimental
- `martensite_host::HostApp` — structs, experimental
- `martensite_host::HostError` — enums, experimental
- `martensite_host::RENDER_SYMBOL_NAME` — constants, experimental

### `martensite-l10n` (v0.19.0)

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

### `martensite-layout` (v0.19.0)

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

### `martensite-macros` (v0.19.0)

**1 public items** — 1 stable, 0 experimental. Categories: 1 macros.

- `martensite_macros::widget` — macros, stable

### `martensite-media` (v0.19.0)

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

### `martensite-media-platform` (v0.19.0)

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

### `martensite-motion` (v0.19.0)

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

### `martensite-notify` (v0.19.0)

**18 public items** — 18 stable, 0 experimental. Categories: 2 enums, 1 functions, 2 modules, 8 re-exports, 3 structs, 2 traits.

- `martensite_notify::Notification` — re-exports, stable
- `martensite_notify::NotifyError` — re-exports, stable
- `martensite_notify::NotifyService` — re-exports, stable
- `martensite_notify::PlatformNotifier` — re-exports, stable
- `martensite_notify::ScriptedNotifier` — re-exports, stable
- `martensite_notify::StubNotifier` — re-exports, stable
- `martensite_notify::Urgency` — re-exports, stable
- `martensite_notify::default_platform_notifier` — re-exports, stable
- `martensite_notify::notify` — modules, stable
- `martensite_notify::notify::Notification` — structs, stable
- `martensite_notify::notify::NotifyError` — enums, stable
- `martensite_notify::notify::NotifyService` — traits, stable
- `martensite_notify::notify::ScriptedNotifier` — structs, stable
- `martensite_notify::notify::Urgency` — enums, stable
- `martensite_notify::platform` — modules, stable
- `martensite_notify::platform::PlatformNotifier` — traits, stable
- `martensite_notify::platform::StubNotifier` — structs, stable
- `martensite_notify::platform::default_platform_notifier` — functions, stable

### `martensite-notify-platform` (v0.19.0)

**4 public items** — 4 stable, 0 experimental. Categories: 1 enums, 1 functions, 1 structs, 1 traits.

- `martensite_notify_platform::NotifyBackend` — traits, stable
- `martensite_notify_platform::NotifySpec` — structs, stable
- `martensite_notify_platform::SpecUrgency` — enums, stable
- `martensite_notify_platform::native_backend` — functions, stable

### `martensite-pdf` (v0.19.0)

**22 public items** — 22 stable, 0 experimental. Categories: 2 enums, 1 functions, 11 re-exports, 6 structs, 2 traits.

- `martensite_pdf::BlankPdfDocument` — re-exports, stable
- `martensite_pdf::CliPdfProvider` — re-exports, stable, feature `platform`
- `martensite_pdf::NullPdfProvider` — re-exports, stable
- `martensite_pdf::PageSize` — re-exports, stable
- `martensite_pdf::PdfDocInfo` — re-exports, stable
- `martensite_pdf::PdfDocument` — re-exports, stable
- `martensite_pdf::PdfError` — re-exports, stable
- `martensite_pdf::PdfPageBitmap` — re-exports, stable
- `martensite_pdf::PdfProvider` — re-exports, stable
- `martensite_pdf::PdfSource` — re-exports, stable
- `martensite_pdf::blank::BlankPdfDocument` — structs, stable
- `martensite_pdf::default_pdf_provider` — re-exports, stable
- `martensite_pdf::doc::NullPdfProvider` — structs, stable
- `martensite_pdf::doc::PageSize` — structs, stable
- `martensite_pdf::doc::PdfDocInfo` — structs, stable
- `martensite_pdf::doc::PdfDocument` — traits, stable
- `martensite_pdf::doc::PdfError` — enums, stable
- `martensite_pdf::doc::PdfPageBitmap` — structs, stable
- `martensite_pdf::doc::PdfProvider` — traits, stable
- `martensite_pdf::doc::PdfSource` — enums, stable
- `martensite_pdf::doc::default_pdf_provider` — functions, stable
- `martensite_pdf::platform::CliPdfProvider` — structs, stable, feature `platform`

### `martensite-pdf-platform` (v0.19.0)

**21 public items** — 21 stable, 0 experimental. Categories: 3 enums, 4 functions, 1 modules, 9 re-exports, 4 structs.

- `martensite_pdf_platform::CliBitmap` — re-exports, stable
- `martensite_pdf_platform::CliDocInfo` — re-exports, stable
- `martensite_pdf_platform::CliError` — re-exports, stable
- `martensite_pdf_platform::CliPageSize` — re-exports, stable
- `martensite_pdf_platform::CliSource` — re-exports, stable
- `martensite_pdf_platform::PdfBackend` — re-exports, stable
- `martensite_pdf_platform::SubprocessDocument` — re-exports, stable
- `martensite_pdf_platform::open_document` — re-exports, stable
- `martensite_pdf_platform::ppm` — modules, stable
- `martensite_pdf_platform::ppm::decode_ppm` — functions, stable
- `martensite_pdf_platform::ppm::decode_ppm_header` — functions, stable
- `martensite_pdf_platform::probe_backend` — re-exports, stable
- `martensite_pdf_platform::provider::CliBitmap` — structs, stable
- `martensite_pdf_platform::provider::CliDocInfo` — structs, stable
- `martensite_pdf_platform::provider::CliError` — enums, stable
- `martensite_pdf_platform::provider::CliPageSize` — structs, stable
- `martensite_pdf_platform::provider::CliSource` — enums, stable
- `martensite_pdf_platform::provider::PdfBackend` — enums, stable
- `martensite_pdf_platform::provider::SubprocessDocument` — structs, stable
- `martensite_pdf_platform::provider::open_document` — functions, stable
- `martensite_pdf_platform::provider::probe_backend` — functions, stable

### `martensite-persist` (v0.19.0)

**14 public items** — 14 stable, 0 experimental. Categories: 1 enums, 2 functions, 2 modules, 6 re-exports, 2 structs, 1 traits.

- `martensite_persist::JsonFileStore` — re-exports, stable
- `martensite_persist::MemoryStore` — re-exports, stable
- `martensite_persist::PersistError` — re-exports, stable
- `martensite_persist::StateStore` — re-exports, stable
- `martensite_persist::app_config_dir` — re-exports, stable
- `martensite_persist::default_store_path` — re-exports, stable
- `martensite_persist::paths` — modules, stable
- `martensite_persist::paths::app_config_dir` — functions, stable
- `martensite_persist::paths::default_store_path` — functions, stable
- `martensite_persist::store` — modules, stable
- `martensite_persist::store::JsonFileStore` — structs, stable
- `martensite_persist::store::MemoryStore` — structs, stable
- `martensite_persist::store::PersistError` — enums, stable
- `martensite_persist::store::StateStore` — traits, stable

### `martensite-plugin` (v0.19.0)

**31 public items** — 0 stable, 31 experimental. Categories: 4 constants, 3 enums, 3 modules, 14 re-exports, 7 structs.

Experimental areas: plugin IPC ring buffer, part of the runtime ABI; plugin runtime ABI added in v0.11.0, ecosystem immature; plugin sandbox/policy surface, ecosystem immature.

- `martensite_plugin::Capability` — re-exports, experimental
- `martensite_plugin::CapabilitySet` — re-exports, experimental
- `martensite_plugin::DEFAULT_CAPACITY` — re-exports, experimental
- `martensite_plugin::DEFAULT_FUEL_BUDGET` — re-exports, experimental
- `martensite_plugin::PluginBuilder` — re-exports, experimental
- `martensite_plugin::PluginError` — re-exports, experimental
- `martensite_plugin::PluginInstance` — re-exports, experimental
- `martensite_plugin::PluginPaintCmd` — re-exports, experimental
- `martensite_plugin::PluginRingBuffer` — re-exports, experimental
- `martensite_plugin::PluginRuntime` — re-exports, experimental
- `martensite_plugin::PluginState` — re-exports, experimental
- `martensite_plugin::RING_BUFFER_REGION_SIZE` — re-exports, experimental
- `martensite_plugin::RingBufferError` — re-exports, experimental
- `martensite_plugin::SHARED_HEADER_SIZE` — re-exports, experimental
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

### `martensite-print` (v0.19.0)

**28 public items** — 28 stable, 0 experimental. Categories: 5 enums, 1 functions, 2 modules, 13 re-exports, 5 structs, 2 traits.

- `martensite_print::Duplex` — re-exports, stable
- `martensite_print::Orientation` — re-exports, stable
- `martensite_print::PageRange` — re-exports, stable
- `martensite_print::PageSize` — re-exports, stable
- `martensite_print::PlatformPrinter` — re-exports, stable
- `martensite_print::PrintJob` — re-exports, stable
- `martensite_print::PrintOutcome` — re-exports, stable
- `martensite_print::PrintSource` — re-exports, stable
- `martensite_print::PrinterInfo` — re-exports, stable
- `martensite_print::PrinterService` — re-exports, stable
- `martensite_print::ScriptedPrinter` — re-exports, stable
- `martensite_print::StubPrinter` — re-exports, stable
- `martensite_print::default_platform_printer` — re-exports, stable
- `martensite_print::job` — modules, stable
- `martensite_print::job::Duplex` — enums, stable
- `martensite_print::job::Orientation` — enums, stable
- `martensite_print::job::PageRange` — structs, stable
- `martensite_print::job::PageSize` — enums, stable
- `martensite_print::job::PrintJob` — structs, stable
- `martensite_print::job::PrintOutcome` — enums, stable
- `martensite_print::job::PrintSource` — enums, stable
- `martensite_print::job::PrinterInfo` — structs, stable
- `martensite_print::job::PrinterService` — traits, stable
- `martensite_print::job::ScriptedPrinter` — structs, stable
- `martensite_print::platform` — modules, stable
- `martensite_print::platform::PlatformPrinter` — traits, stable
- `martensite_print::platform::StubPrinter` — structs, stable
- `martensite_print::platform::default_platform_printer` — functions, stable

### `martensite-print-platform` (v0.19.0)

**7 public items** — 7 stable, 0 experimental. Categories: 3 enums, 1 functions, 2 structs, 1 traits.

- `martensite_print_platform::BackendPrinter` — structs, stable
- `martensite_print_platform::PrintBackend` — traits, stable
- `martensite_print_platform::PrintReply` — enums, stable
- `martensite_print_platform::PrintSpec` — structs, stable
- `martensite_print_platform::SpecSides` — enums, stable
- `martensite_print_platform::SpecSource` — enums, stable
- `martensite_print_platform::native_backend` — functions, stable

### `martensite-reactive` (v0.19.0)

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

### `martensite-render` (v0.19.0)

**44 public items** — 44 stable, 0 experimental. Categories: 2 constants, 2 enums, 4 functions, 1 glob re-exports, 5 modules, 25 re-exports, 4 structs, 1 traits.

- `martensite_render::BezPath` — re-exports, stable
- `martensite_render::ClearMode` — enums, stable
- `martensite_render::CornerRadii` — re-exports, stable
- `martensite_render::CornerStyle` — re-exports, stable
- `martensite_render::CornerStyles` — re-exports, stable
- `martensite_render::FontResource` — re-exports, stable
- `martensite_render::GlyphInstance` — re-exports, stable
- `martensite_render::GlyphRun` — re-exports, stable
- `martensite_render::GradientStop` — re-exports, stable
- `martensite_render::GradientStops` — re-exports, stable
- `martensite_render::ImageData` — re-exports, stable
- `martensite_render::PaintCommand` — re-exports, stable
- `martensite_render::PaintList` — re-exports, stable
- `martensite_render::PaintSegment` — re-exports, stable
- `martensite_render::PathBuilder` — re-exports, stable
- `martensite_render::Point` — re-exports, stable
- `martensite_render::PresentationError` — re-exports, stable
- `martensite_render::Rect` — re-exports, stable
- `martensite_render::RenderBackend` — traits, stable
- `martensite_render::Shape` — re-exports, stable
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
- `martensite_render::shape` — re-exports, stable
- `martensite_render::tinyskia_backend` — modules, stable
- `martensite_render::tinyskia_backend::TinySkiaBackend` — structs, stable
- `martensite_render::vello_backend` — modules, stable
- `martensite_render::vello_backend::VelloRenderer` — structs, stable

### `martensite-share` (v0.19.0)

**16 public items** — 16 stable, 0 experimental. Categories: 1 enums, 1 functions, 2 modules, 7 re-exports, 3 structs, 2 traits.

- `martensite_share::PlatformShare` — re-exports, stable
- `martensite_share::ScriptedShare` — re-exports, stable
- `martensite_share::ShareOutcome` — re-exports, stable
- `martensite_share::ShareRequest` — re-exports, stable
- `martensite_share::ShareService` — re-exports, stable
- `martensite_share::StubShare` — re-exports, stable
- `martensite_share::default_platform_share` — re-exports, stable
- `martensite_share::platform` — modules, stable
- `martensite_share::platform::PlatformShare` — traits, stable
- `martensite_share::platform::StubShare` — structs, stable
- `martensite_share::platform::default_platform_share` — functions, stable
- `martensite_share::share` — modules, stable
- `martensite_share::share::ScriptedShare` — structs, stable
- `martensite_share::share::ShareOutcome` — enums, stable
- `martensite_share::share::ShareRequest` — structs, stable
- `martensite_share::share::ShareService` — traits, stable

### `martensite-share-platform` (v0.19.0)

**7 public items** — 7 stable, 0 experimental. Categories: 1 enums, 4 functions, 1 structs, 1 traits.

- `martensite_share_platform::ShareBackend` — traits, stable
- `martensite_share_platform::ShareReply` — enums, stable
- `martensite_share_platform::ShareSpec` — structs, stable
- `martensite_share_platform::mailto_uri` — functions, stable
- `martensite_share_platform::native_backend` — functions, stable
- `martensite_share_platform::percent_encode` — functions, stable
- `martensite_share_platform::share_uri` — functions, stable

### `martensite-shell` (v0.19.0)

**31 public items** — 26 stable, 5 experimental. Categories: 5 enums, 2 functions, 5 modules, 11 re-exports, 6 structs, 2 traits.

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
- `martensite_shell::resolve_backdrop_material` — re-exports, stable
- `martensite_shell::resolve_vibrancy_material` — re-exports, stable
- `martensite_shell::snap` — modules, stable
- `martensite_shell::snap::SnapLayout` — structs, stable

### `martensite-test` (v0.19.0)

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

### `martensite-text` (v0.19.0)

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

### `martensite-theme` (v0.19.0)

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

Vendored upstream fork: the entire surface tracks its upstream project and is EXPERIMENTAL until the maintenance policy in `API_FREEZE_AUDIT.md` §5 pins a re-sync contract.

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

### `martensite-webview` (v0.19.0)

**12 public items** — 12 stable, 0 experimental. Categories: 2 enums, 2 modules, 5 re-exports, 2 structs, 1 traits.

- `martensite_webview::SimulatedWebView` — re-exports, stable
- `martensite_webview::WebViewCommand` — re-exports, stable
- `martensite_webview::WebViewEvent` — re-exports, stable
- `martensite_webview::WebViewHost` — re-exports, stable
- `martensite_webview::WebViewState` — re-exports, stable
- `martensite_webview::simulated` — modules, stable
- `martensite_webview::simulated::SimulatedWebView` — structs, stable
- `martensite_webview::state` — modules, stable
- `martensite_webview::state::WebViewCommand` — enums, stable
- `martensite_webview::state::WebViewEvent` — enums, stable
- `martensite_webview::state::WebViewHost` — traits, stable
- `martensite_webview::state::WebViewState` — structs, stable

### `martensite-webview-platform` (v0.19.0)

**9 public items** — 9 stable, 0 experimental. Categories: 2 functions, 2 modules, 3 re-exports, 2 structs.

- `martensite_webview_platform::FetchWebView` — re-exports, stable
- `martensite_webview_platform::SystemBrowserWebView` — re-exports, stable
- `martensite_webview_platform::browser` — modules, stable
- `martensite_webview_platform::browser::SystemBrowserWebView` — structs, stable
- `martensite_webview_platform::default_platform_host` — functions, stable
- `martensite_webview_platform::extract_title` — re-exports, stable
- `martensite_webview_platform::fetch` — modules, stable
- `martensite_webview_platform::fetch::FetchWebView` — structs, stable
- `martensite_webview_platform::fetch::extract_title` — functions, stable

### `martensite-wgpu` (v0.19.0)

**87 public items** — 47 stable, 40 experimental. Categories: 5 constants, 13 enums, 8 functions, 8 modules, 39 re-exports, 13 structs, 1 type aliases.

Experimental areas: device-loss recovery API added in v0.11.0, still hardening; external-engine embedding bridge surface (v0.17.0); wasm/web backend glue, compile-verified only; zero-allocation theme transitions, perf-gated and young.

- `martensite_wgpu::BackdropMode` — re-exports, stable
- `martensite_wgpu::CompositeTarget` — re-exports, experimental
- `martensite_wgpu::DEFAULT_FALLBACK_THRESHOLD` — re-exports, experimental
- `martensite_wgpu::DEFAULT_MAX_RETRIES` — re-exports, experimental
- `martensite_wgpu::DeviceStatus` — re-exports, experimental
- `martensite_wgpu::ExternalError` — re-exports, experimental
- `martensite_wgpu::FormatNegotiator` — re-exports, stable
- `martensite_wgpu::GpuContext` — re-exports, stable
- `martensite_wgpu::GpuContextError` — re-exports, stable
- `martensite_wgpu::MEDIA_YUV_EOTF_WGSL` — re-exports, stable
- `martensite_wgpu::OrchestratorConfig` — re-exports, stable
- `martensite_wgpu::OrchestratorError` — re-exports, stable
- `martensite_wgpu::PresentModePreference` — re-exports, stable
- `martensite_wgpu::RECOVERY_BUDGET` — re-exports, experimental
- `martensite_wgpu::RecoveryError` — re-exports, experimental
- `martensite_wgpu::RecoveryHarness` — re-exports, experimental
- `martensite_wgpu::RecoveryMachine` — re-exports, experimental
- `martensite_wgpu::RecoveryOutcome` — re-exports, experimental
- `martensite_wgpu::RenderMode` — re-exports, stable
- `martensite_wgpu::RenderOrchestrator` — re-exports, stable
- `martensite_wgpu::SurfaceError` — re-exports, experimental
- `martensite_wgpu::SurfaceWrapper` — re-exports, stable
- `martensite_wgpu::SurfaceWrapperError` — re-exports, stable
- `martensite_wgpu::THEME_UNIFORM_SIZE` — re-exports, experimental
- `martensite_wgpu::TakenFrame` — re-exports, experimental
- `martensite_wgpu::ThemeTransitionError` — re-exports, experimental
- `martensite_wgpu::ThemeTransitionPipeline` — re-exports, experimental
- `martensite_wgpu::VideoPipelineUniforms` — re-exports, stable
- `martensite_wgpu::VideoProcessor` — re-exports, stable
- `martensite_wgpu::VideoProcessorError` — re-exports, stable
- `martensite_wgpu::WgpuHost` — re-exports, experimental
- `martensite_wgpu::backoff_duration` — re-exports, experimental
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
- `martensite_wgpu::render_theme_transition` — re-exports, experimental
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

### `martensite-window` (v0.19.0)

**110 public items** — 99 stable, 11 experimental. Categories: 1 constants, 12 enums, 16 functions, 11 modules, 46 re-exports, 24 structs.

Experimental areas: Kalman stylus filtering, latency-gated and young; client-side decoration hit-testing, young shell surface; wasm/web backend glue, compile-verified only.

- `martensite_window::ActiveEventLoop` — re-exports, stable
- `martensite_window::AffineTransform` — re-exports, stable
- `martensite_window::ClipShape` — re-exports, stable
- `martensite_window::CsdController` — re-exports, experimental
- `martensite_window::CsdHitRegion` — re-exports, experimental
- `martensite_window::DpiScale` — re-exports, stable
- `martensite_window::DropAction` — re-exports, stable
- `martensite_window::DropEvent` — re-exports, stable
- `martensite_window::EventDispatchOutcome` — re-exports, stable
- `martensite_window::EventRouter` — re-exports, stable
- `martensite_window::HitTestResult` — re-exports, stable
- `martensite_window::HitTester` — re-exports, stable
- `martensite_window::ImeEvent` — re-exports, stable
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
- `martensite_window::event::ImeEvent` — enums, stable
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
- `martensite_window::event::ime_event_for_winit` — functions, stable
- `martensite_window::event::widget_event_for_pointer` — functions, stable
- `martensite_window::gesture` — modules, stable
- `martensite_window::gesture::Gesture` — enums, stable
- `martensite_window::gesture::GestureConfig` — structs, stable
- `martensite_window::gesture::GestureRecognizer` — structs, stable
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
- `martensite_window::ime_event_for_winit` — re-exports, stable
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
