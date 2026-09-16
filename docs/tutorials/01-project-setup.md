# Tutorial 1 — Project setup and `cargo-martensite`

This tutorial walks from an empty directory to a compiling Martensite
application and shows what the `cargo-martensite` developer CLI does (and
does not) provide.

> **Honesty note.** `cargo-martensite` does **not** ship a `new`/`init`
> scaffolding subcommand — there is no template generator. The CLI today
> implements exactly four commands: `dev`, `build`, `help`, and
> `--version`. Project initialization is plain `cargo new` plus a
> dependency line. This tutorial documents the real behavior.

## 1. Create the project

```sh
cargo new counter-app
cd counter-app
cargo add martensite
```

`cargo add martensite` resolves the latest published `0.x` release
(`0.17.0` at the time of writing). The umbrella crate re-exports the
subsystem crates under `martensite::*` — `core`, `reactive`, `layout`,
`wgpu`, `render`, `text`, `access`, `window`, `focus`, `clipboard`,
`dnd`, `theme`, `motion`, `history`, `l10n`, `media`, `macros` — plus a
`prelude` module that also pulls in the engine-bridge types, so a single
dependency is enough. (`martensite-assets`, `martensite-plugin`,
`martensite-test`, and `martensite-devtools` are dependencies but are
not re-exported as `martensite::*` modules.)

## 2. A minimal program

The smallest useful program exercises the two primitives everything else
builds on: the `WidgetArena` (generational widget storage) and `Signal`
(reactive state). Replace `src/main.rs` with:

```rust
use martensite::prelude::*;

fn main() {
    let mut arena = WidgetArena::new();

    // A widget enters the arena as a (HotNode, ColdNode) pair; the arena
    // returns a generational WidgetId handle.
    let root = arena.insert_with_widget(
        HotNode::default(),
        Box::new(Container::new().padding_uniform(16.0)),
    );
    assert!(arena.is_alive(root));

    let count = Signal::new(0);
    count.update(|c| *c += 1);
    println!("count = {}", count.get()); // count = 1
}
```

```sh
cargo run
```

`insert_with_widget` wraps the widget in a `ColdNode` with default
metadata; use `arena.insert(hot, cold)` directly when you need
`ColdNode::with_role`/`with_a11y_name`/other metadata.

## 3. A real windowed application

There is no `App::run` convenience runner in v0.17.0 — `App::build()`
produces an `AppConfig` consumed by the render pipeline, and the event
loop is wired explicitly through `winit`'s `ApplicationHandler`. The
canonical skeleton (adapted from the `martensite-window` crate docs):

```rust
use martensite_window::{WindowManager, manager::WindowEventOutcome};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{WindowAttributes, WindowId};

struct App {
    mgr: WindowManager,
}

impl ApplicationHandler for App {
    fn can_create_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) {
        let attrs = WindowAttributes::default().with_title("Counter");
        self.mgr
            .create_window(event_loop, attrs)
            .expect("window creation failed");
    }

    fn window_event(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        id: WindowId,
        event: WindowEvent,
    ) {
        match self.mgr.handle_window_event(id, &event) {
            WindowEventOutcome::CloseRequested => event_loop.exit(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &dyn ActiveEventLoop) {}
}

fn main() {
    let event_loop = EventLoop::new().expect("failed to create event loop");
    let app = App {
        mgr: WindowManager::new(),
    };
    event_loop.run_app(app).expect("event loop exited with error");
}
```

This needs `winit` as a direct dependency matching the workspace pin:

```sh
cargo add winit@0.31.0-beta.3
```

The full GPU pipeline — `GpuContext`, `SurfaceWrapper`,
`RenderOrchestrator`, damage-driven repaint — is wired the same way but is
a larger surface; `examples/engine_embed` in the repository is the
complete, compilable reference for that loop.

## 4. Installing the developer CLI

`cargo-martensite` is `publish = false` — it is **not** on crates.io.
Install it from a repository checkout:

```sh
git clone https://github.com/jxoesneon/martensite.git
cargo install --path martensite/tools/cargo-martensite
```

## 5. What the CLI actually does

The CLI implements the **guest-crate hot-reload loop** from the v0.9.0
architecture: your component code lives in a crate compiled as a
`cdylib`; `cargo-martensite` rebuilds it into a *versioned* shared
library under `target/martensite/` when sources change, and the host
binary re-links it.

```sh
cargo martensite help        # prints real usage
cargo martensite --version   # prints the toolchain version
cargo martensite build       # one-shot: build the guest crate as a cdylib
cargo martensite dev         # watch src/ and rebuild on change (default: --watch)
cargo martensite dev --no-watch   # single build cycle, no watcher
cargo martensite dev --port 9000  # bind the dev server to a custom port (default 8765)
cargo martensite dev --package my_guest   # build a crate other than the current package
```

Details worth knowing:

- The guest crate name defaults to the `name = "..."` in the current
  directory's `Cargo.toml` (falling back to `"guest"`); `--package`
  overrides it.
- If the guest crate does not declare `crate-type = ["cdylib"]`, the
  build forces it via `cargo rustc --lib -- --crate-type cdylib`.
- Reload cycles are measured against a **350 ms budget**; overruns print
  a warning.
- The watcher is a polling `FileWatcher` (100 ms default interval)
  watching `src/` — it detects mtime changes and triggers
  `build_guest_crate`.
- `dev` only *builds* the versioned cdylib; the actual `dlopen`-style
  re-link is performed by the host binary (via `martensite-host`), which
  is where the `unsafe` lives — the CLI itself is
  `#![forbid(unsafe_code)]`.

## Next steps

- [Tutorial 2 — Reactive state management](02-reactive-state.md)
- [Tutorial 3 — Writing a custom widget](03-custom-widget.md)
