# Martensite v1.0.0 Public API Specification

**Document Identifier:** DOC-API-1.0.0  
**Status:** Stable V1 Contract  
**Target:** `martensite` crates ecosystem

This document provides the technical specification for the Martensite v1.0.0 application programming interface. Specifications are declarative, measurable, and focused on operational mechanics.

## 1. The App Entry Point

The `App` builder is the primary host-process bootstrapper. It configures the native OS window, initializes the WGPU render context, and enters the event-driven sleep loop (Requirement III, Event-Driven Sleep).

```rust
pub struct App {
    // Internal configurations
}

impl App {
    /// Initializes a new Martensite application builder.
    pub fn build() -> AppBuilder {
        AppBuilder::default()
    }
}

pub struct AppBuilder { /* ... */ }

impl AppBuilder {
    /// Sets the primary window title.
    ///
    /// # Platform Behavior
    /// - **macOS**: Sets the `NSWindow` title.
    /// - **Windows**: Sets the `HWND` text.
    /// - **Linux**: Sets the Wayland/X11 surface title.
    ///
    /// # Default
    /// `"Martensite Application"`
    pub fn title(self, title: impl Into<String>) -> Self;

    /// Sets the initial logical size of the window.
    ///
    /// # Arguments
    /// * `width` - Logical width in points.
    /// * `height` - Logical height in points.
    ///
    /// # Panics
    /// Panics if `width` or `height` is less than or equal to `0.0`, or if `NaN` or `Infinity`.
    pub fn size(self, width: f32, height: f32) -> Self;

    /// Sets the minimum logical size of the window.
    /// Resizing below this threshold is constrained by the native OS window manager.
    ///
    /// # Panics
    /// Panics if bounds are `NaN` or `Infinity`, or if `min_size` > `max_size` (if set).
    pub fn min_size(self, width: f32, height: f32) -> Self;

    /// Mounts the application theme into the ambient root context.
    pub fn theme(self, theme: Theme) -> Self;

    /// Sets the root localization identifier.
    pub fn locale(self, locale: unic_langid::LanguageIdentifier) -> Self;

    /// Finalizes the build, locks the configuration, and yields control to the OS event loop.
    ///
    /// The provided closure receives a `&mut Context` and returns a `Widget`.
    /// This function blocks the main thread indefinitely.
    ///
    /// # Panics
    /// Panics if the GPU hardware fails to initialize a compute-capable WGPU adapter.
    pub fn run<F, W>(self, app_root: F) -> !
    where
        F: FnOnce(&mut Context) -> W + 'static,
        W: Widget + 'static;
}
```

## 2. The Context API (`cx`)

The `Context` (`cx`) is the single-threaded orchestrator for the reactive arena ([ADR-0001](adr/ADR-0001-generational-slotmap-arena.md)). It allows nodes to safely spawn signals, read ambient data, and spawn async tasks.

```rust
pub struct Context<'a> { /* ... */ }

impl<'a> Context<'a> {
    /// Creates a push-pull reactive signal ([ADR-0002](adr/ADR-0002-push-pull-reactive-signals.md)).
    ///
    /// # Returns
    /// A lightweight 64-bit copyable `Signal<T>` handle.
    pub fn signal<T: Clone + 'static>(&mut self, initial: T) -> Signal<T>;

    /// Creates a lazy, glitch-free derived computation.
    /// Evaluates strictly when dependencies update and layout/paint requests the value.
    pub fn memo<T: Clone + 'static, F>(&mut self, compute: F) -> Memo<T>
    where
        F: Fn() -> T + Send + Sync + 'static;

    /// Registers a side effect that executes immediately, and re-executes whenever 
    /// any accessed `Signal` dependencies mutate.
    pub fn effect<F>(&mut self, effect: F)
    where
        F: FnMut() + 'static;

    /// Retrieves the current theme from the nearest provider boundary.
    pub fn theme(&self) -> &Theme;

    /// Retrieves the current locale identifier.
    pub fn locale(&self) -> &unic_langid::LanguageIdentifier;

    /// Spawns a non-blocking asynchronous task onto the Martensite executor.
    pub fn spawn<F>(&self, future: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static;

    /// Injects an ambient typed value into the widget subtree.
    pub fn provide_context<T: Clone + Send + Sync + 'static>(&mut self, value: T);

    /// Retrieves an ambient typed value.
    ///
    /// # Panics
    /// Panics if type `T` was not provided higher in the widget tree.
    pub fn use_context<T: Clone + Send + Sync + 'static>(&self) -> T;
}
```

## 3. The Widget Builder API

Martensite widgets are instantiated via factory functions and chained method mutators. Memory remains within the `WidgetArena` without heap scattering.

### Primitive Factories
```rust
pub fn text(content: impl Into<String>) -> TextWidget;
pub fn button(label: impl Into<String>) -> ButtonWidget;
pub fn column() -> FlexWidget;
pub fn row() -> FlexWidget;
pub fn stack() -> StackWidget;
pub fn scroll() -> ScrollWidget;
pub fn spacer() -> SpacerWidget;
```

### Modifiers (Chained Trait)
```rust
pub trait WidgetExt: Sized + Widget {
    fn padding(self, points: f32) -> Self;
    fn margin(self, points: f32) -> Self;
    fn background(self, color: impl Into<Color>) -> Self;
    fn border(self, width: f32, color: impl Into<Color>) -> Self;
    fn clip(self) -> Self;
    fn opacity(self, alpha: f32) -> Self; // 0.0 to 1.0 bounds
    fn shadow(self, offset: glam::Vec2, blur: f32, color: impl Into<Color>) -> Self;
    fn tooltip(self, text: impl Into<String>) -> Self;
    
    // Layout
    fn width(self, width: f32) -> Self;
    fn height(self, height: f32) -> Self;
    fn min_width(self, width: f32) -> Self;
    fn flex(self, factor: f32) -> Self;
    fn align(self, alignment: Alignment) -> Self;
    fn justify(self, justification: Justification) -> Self;

    // Events
    fn on_click<F: FnMut(&mut EventContext) + 'static>(self, handler: F) -> Self;
    fn on_hover<F: FnMut(&mut EventContext, bool) + 'static>(self, handler: F) -> Self;
    fn on_focus<F: FnMut(&mut EventContext, bool) + 'static>(self, handler: F) -> Self;
    fn on_key<F: FnMut(&mut EventContext, &KeyEvent) -> EventResponse + 'static>(self, handler: F) -> Self;
}
```

## 4. The Signal/Reactivity API

Reactivity relies on push-pull mechanics for zero-allocation state mutations.

```rust
impl<T: Clone + 'static> Signal<T> {
    /// Reads the current value. Marks the executing context (Memo/Effect/Layout) as dependent.
    pub fn get(&self) -> T;

    /// Overwrites the signal. Pushes dirty bitsets to downstream observers. $O(1)$ operation.
    pub fn set(&self, val: T);

    /// Mutates the current value in place.
    pub fn update(&self, f: impl FnOnce(&mut T));

    /// Reads the value without subscribing to updates.
    pub fn with<R>(&self, f: impl FnOnce(&T) -> R) -> R;
}

// Transactional State Management
pub struct Transactional<T: Clone + 'static> { /* ... */ }

impl<'a> Context<'a> {
    pub fn transactional<T: Clone + 'static>(&mut self, initial: T) -> Transactional<T>;
    pub fn undo(&mut self);
    pub fn redo(&mut self);
}

impl<T: Clone + 'static> Transactional<T> {
    pub fn commit(&self, update: impl FnOnce(&mut T));
}
```

## 5. Theme & Styling API

Styles are strongly typed, physically accurate (Oklab), and structurally guaranteed.

```rust
pub struct Theme { /* ... */ }

pub struct ThemeBuilder { /* ... */ }

impl Theme {
    pub fn builder() -> ThemeBuilder { ThemeBuilder::default() }
    pub fn dark() -> Self;
    pub fn light() -> Self;
}

impl ThemeBuilder {
    pub fn surface_primary(self, color: Color) -> Self;
    pub fn text_primary(self, color: Color) -> Self;
    pub fn accent(self, color: Color) -> Self;
    pub fn build(self) -> Theme;
}

pub struct Color { /* ... */ }
impl Color {
    pub const WHITE: Self = Self { /* ... */ };
    pub fn from_oklab(l: f32, a: f32, b: f32) -> Self;
    pub fn from_hex(hex: &str) -> Self;
}
```

## 6. Localization API

Message formatting utilizes standard ICU syntax.

```rust
impl<'a> Context<'a> {
    pub fn locale_bundle(&self) -> &FluentBundle;
}

impl FluentBundle {
    /// Formats a localized message by key.
    ///
    /// # Arguments
    /// * `key` - The message key defined in the localization assets.
    /// * `args` - Key-value pairs for substitution.
    ///
    /// # Panics
    /// In debug mode, panics if the key is missing. In release, falls back to the key string.
    pub fn message(&self, key: &str, args: FluentArgs) -> String;
}
```

## 7. Custom Widget Contract (3rd-party extensibility)

The core contract for all visual elements. Implementing `Widget` provides total control over layout, painting, and accessibility without inheritance chains.

```rust
pub trait Widget: Send + Sync + 'static {
    /// **Pass 1: Intrinsic Measurement**. Computes required dimensions independent of final placement.
    fn measure(&mut self, cx: &mut MeasureContext, constraints: Constraints) -> Size;

    /// **Pass 2: Constraint Placement**. Applies final bounding boxes from the engine.
    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect);

    /// **Event Dispatch**. Responds to OS input. Returning `Handled` stops propagation.
    fn event(&mut self, cx: &mut EventContext, event: &Event) -> EventResponse;

    /// **Accessibility Sync**. Writes structural and semantic updates to the AccessKit tree.
    fn accessibility(&self, cx: &mut AccessibilityContext, node: &mut AccessKitNode);

    /// **Rendering**. Emits drawing commands to the scene graph.
    fn paint(&self, cx: &mut PaintContext, scene: &mut Scene);

    /// **Graph Navigation**. Returns 64-bit slot identifiers of active children.
    fn children(&self) -> &[WidgetId] { &[] }
}
```

## 8. Async/Await Integration

Tasks yield directly into the native event loop, avoiding blocking `App::run`.

```rust
cx.spawn(async move {
    let data = fetch_data().await;
    // Signals are Send + Sync, allowing direct manipulation from background tasks.
    // The signal DAG inherently wakes the main thread UI if necessary.
    result_signal.set(data);
});
```
*Note*: `Signal::set` contains atomics that flag the main event loop `waker`, adhering strictly to the Event-Sleep Law (Mandate III).

## 9. Breaking Change Policy

The V1 API surface is governed by SemVer 2.0.0.

- **Stable (No breaking changes in 1.x)**:
  - `App`, `AppBuilder`, `Context` signatures.
  - Core `Widget` trait definition and `WidgetExt` modifier methods.
  - `Signal<T>`, `Memo<T>`, and fundamental reactivity semantics.
- **Unstable (Subject to breakage in 1.x)**:
  - Internal Vello `Scene` encoding structures inside `PaintContext`.
  - WGPU raw adapter access (`cx.wgpu_device()`).
  - Undocumented layout algorithm internals in Taffy bridging.
