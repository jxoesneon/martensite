# Martensite Tutorials

Four end-to-end tutorials covering the workflows exercised by the v1.0.0
documentation requirements (milestone v0.18.0 §4.7 / v1.0.0 §4.2):

1. [Project setup and `cargo-martensite`](01-project-setup.md) — scaffold a
   Martensite application, install the developer CLI, and use the hot-reload
   build loop.
2. [Reactive state management](02-reactive-state.md) — `Signal`, `Memo`,
   `Effect`, batching, and the push-pull DAG in `martensite-reactive`.
3. [Writing a custom widget](03-custom-widget.md) — the `Widget` trait
   two-pass layout contract (`measure`/`layout`/`paint`), event handling,
   and the internal-children protocol.
4. [Accessibility validation](04-accessibility.md) — the AccessKit tree
   adapter, semantic-action dispatch, WCAG compliance helpers, and how to
   test accessibility without a screen reader.

## Accuracy policy

Every code snippet in these tutorials was written against the real crate
sources at v0.17.0 and compile-checked. Where a capability does not exist
yet (for example, `cargo-martensite` has no `new` scaffolding subcommand),
the tutorial says so explicitly rather than documenting aspirational APIs.
If a snippet drifts out of date, that is a bug — please file an issue.
