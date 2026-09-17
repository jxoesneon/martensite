# industrial_dashboard

Part of the Martensite GUI framework. This is an internal/non-publishable workspace member.

Industrial Workstation — the v0.18.0+ dogfooding example and the
project's windowed flagship. One shared model drives two modes:

- **Windowed** (default): the full production assembly — winit +
  `RenderOrchestrator` (Vello GPU path), four arena-owned `Widget`
  panels over the blessed models, BSP `DockTree` geometry, `Signal`
  reactivity, `FocusManager` traversal, a live AccessKit tree, and the
  advisory paint-compliance audit running against its own output.
- **`--headless`**: the original CI composition — every subsystem
  exercised through model APIs with no display server, printing a
  verification report.

Showcased: a virtualized 1,000,000-row `DataTable` (sort/filter/
selection/keyboard), a `Signal`-driven telemetry `Chart`, a syntax-
highlighted `CodeEditor` model, a `MediaView` over a mock NV12 surface
(honest empty state — no decoder attached), keyboard focus navigation,
shell chrome material resolution, and a feature-gated devtools HUD.

```sh
cargo run -p industrial_dashboard                      # windowed showcase
cargo run -p industrial_dashboard -- --headless        # headless CI report
cargo run -p industrial_dashboard --features devtools  # with the diagnostic HUD pass
```

`F1`–`F16` comments in `src/headless.rs` log the API friction found
while wiring the headless composition; `F17`–`F23` in `src/panels.rs`
log what the windowed path surfaced (pre-freeze fix candidates, see
`docs/milestones/v0.18.0-production-hardening.md` §4.6).
