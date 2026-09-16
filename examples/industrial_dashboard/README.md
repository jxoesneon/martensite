# industrial_dashboard

Part of the Martensite GUI framework. This is an internal/non-publishable workspace member.

Industrial Workstation — the v0.18.0 dogfooding example. A headless
composition of the application stack: reactive signals, theme tokens, a
widget arena resolved by the Taffy layout engine, a BSP docking tree, a
virtualized 1,000,000-row `DataTable`, chart series, a code editor model,
a `MediaView` display surface (stub surface — no video file required),
keyboard focus navigation, shell chrome material resolution, and a
feature-gated devtools HUD.

```sh
cargo run -p industrial_dashboard                      # headless, no display needed
cargo run -p industrial_dashboard --features devtools  # with the diagnostic HUD pass
```

`F1`–`F15` comments in `src/main.rs` log every API friction found while
wiring the demo (pre-freeze fix candidates, see
`docs/milestones/v0.18.0-production-hardening.md` §4.6).
