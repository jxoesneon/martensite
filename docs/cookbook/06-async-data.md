# Cookbook 06 — Asynchronous Data & Network Streams

Modern graphical user interfaces cannot pause or stutter while waiting for network
responses, disk I/O, database queries, or CPU-intensive computations. A single
synchronous blocking call on the UI thread drops frames, introduces visual hitching,
and risks triggering OS "Application Not Responding" (ANR) dialogs.

This recipe demonstrates how to implement the **`Resource<T>`** asynchronous data
pattern in Martensite: bridging background worker threads and asynchronous futures
into reactive signals via thread-safe channels, cooperative cancellation tokens,
and non-blocking event-loop synchronization.

---

## 1. Goal

Architect a robust asynchronous data integration pipeline that:
1. Keeps the main UI thread running at continuous 60/120 FPS without blocking on I/O or futures.
2. Encapsulates asynchronous query states using an explicit `Resource<T>` enum (`Uninitialized`, `Loading`, `Ready(T)`, `Error(String)`).
3. Executes long-running tasks on background worker pools (Tokio or OS worker threads).
4. Bridges background channel messages into the reactive `Signal` DAG atomically inside `batch(|| ...)`.
5. Cancels obsolete in-flight requests cooperatively using `CancellationToken`s and generational request IDs.
6. Eliminates stale frame presentation, race conditions (out-of-order response arrival), and UI flickering during rapid query modifications.

---

## 2. Complete Runnable Pattern

The following pattern implements an Asynchronous Cluster Metrics Monitor (`ClusterMonitorWidget`).
It queries remote telemetry endpoints in the background, renders loading states with progress,
presents live data cards, handles network failures with retry actions, and cooperatively cancels
pending queries when switching server nodes.

```rust
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use glam::Vec2;
use kurbo::{Point, Rect as KurboRect};
use martensite::prelude::*;
use martensite::widgets::flex::{CrossAxisAlignment, MainAxisAlignment};
use martensite::widgets::{Button, Flex, Text};
use martensite_core::widget::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Widget,
};
use martensite_core::{NodeFlags, Rect, TokenKey};

/// Current state of an asynchronous operation.
#[derive(Clone, Debug, PartialEq)]
pub enum AsyncState<T> {
    /// No fetch has been initiated.
    Uninitialized,
    /// In-flight fetch with optional progress percentage (0.0 to 1.0).
    Loading { progress: Option<f32>, cached_previous: Option<T> },
    /// Successful completion with fresh payload.
    Ready(T),
    /// Operation failed with an explanatory error message.
    Error(String),
}

/// Domain payload returned by the remote telemetry service.
#[derive(Clone, Debug, PartialEq)]
pub struct ClusterMetrics {
    pub node_id: String,
    pub cpu_usage: f32,
    pub memory_gb: f32,
    pub active_connections: usize,
    pub uptime_hours: u32,
}

/// A cooperative cancellation token shared between UI and background tasks.
#[derive(Clone)]
pub struct CancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl CancellationToken {
    pub fn new() -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

/// Channel envelope sent from worker threads to the UI event loop.
pub struct AsyncEnvelope<T> {
    pub request_id: u64,
    pub result: Result<T, String>,
}

/// Reactive asynchronous resource coordinator.
#[derive(Clone)]
pub struct ClusterResource {
    pub state: Signal<AsyncState<ClusterMetrics>>,
    current_request_id: Arc<AtomicU64>,
    active_cancel_token: Arc<parking_lot::Mutex<Option<CancellationToken>>>,
    sender: Sender<AsyncEnvelope<ClusterMetrics>>,
}

impl ClusterResource {
    pub fn new(sender: Sender<AsyncEnvelope<ClusterMetrics>>) -> Self {
        Self {
            state: create_signal(AsyncState::Uninitialized),
            current_request_id: Arc::new(AtomicU64::new(0)),
            active_cancel_token: Arc::new(parking_lot::Mutex::new(None)),
            sender,
        }
    }

    /// Trigger a new asynchronous fetch for a cluster node.
    pub fn fetch(&self, node_id: String) {
        // 1. Cancel previous in-flight task
        let mut token_guard = self.active_cancel_token.lock();
        if let Some(old_token) = token_guard.take() {
            old_token.cancel();
        }

        let new_token = CancellationToken::new();
        *token_guard = Some(new_token.clone());

        // 2. Increment generational request counter
        let req_id = self.current_request_id.fetch_add(1, Ordering::SeqCst) + 1;

        // 3. Update reactive state to Loading (preserving previous data for optimistic UI)
        let prev = match self.state.get() {
            AsyncState::Ready(data) => Some(data),
            AsyncState::Loading { cached_previous, .. } => cached_previous,
            _ => None,
        };

        self.state.set(AsyncState::Loading {
            progress: Some(0.1),
            cached_previous: prev,
        });

        // 4. Spawn background worker (Tokio task or OS thread)
        let tx = self.sender.clone();
        thread::spawn(move || {
            // Simulate staged network latency with cancellation checkpoints
            for step in 1..=4 {
                thread::sleep(Duration::from_millis(80));
                if new_token.is_cancelled() {
                    return; // Abort early without sending stale result
                }
            }

            // Simulate simulated response or error
            let result = if node_id.starts_with("offline") {
                Err(format!("Node {} is unreachable (connection timed out)", node_id))
            } else {
                Ok(ClusterMetrics {
                    node_id: node_id.clone(),
                    cpu_usage: 47.8,
                    memory_gb: 18.4,
                    active_connections: 1240,
                    uptime_hours: 312,
                })
            };

            // Send result back across the thread boundary
            let _ = tx.send(AsyncEnvelope {
                request_id: req_id,
                result,
            });
        });
    }

    /// Drain pending messages from the channel and commit to reactive signals.
    /// Called once per frame during the UI tick.
    pub fn pump_channel(&self, rx: &Receiver<AsyncEnvelope<ClusterMetrics>>) {
        while let Ok(envelope) = rx.try_recv() {
            // Verify that this message belongs to the current request generation
            if envelope.request_id == self.current_request_id.load(Ordering::Acquire) {
                batch(|| match envelope.result {
                    Ok(payload) => self.state.set(AsyncState::Ready(payload)),
                    Err(err_msg) => self.state.set(AsyncState::Error(err_msg)),
                });
            }
        }
    }
}

/// UI Widget rendering the asynchronous cluster monitor.
pub struct ClusterMonitorWidget {
    resource: ClusterResource,
    receiver: Receiver<AsyncEnvelope<ClusterMetrics>>,
    root_flex: Flex,
    cached_bounds: Rect,
}

impl ClusterMonitorWidget {
    pub fn new() -> Self {
        let (tx, rx) = channel();
        let resource = ClusterResource::new(tx);

        // Initial trigger
        resource.fetch("node-alpha-42".to_string());

        let mut root = Flex::column().gap(12.0);
        root = root
            .child(Text::new("Cluster Node Telemetry").font_size(16.0))
            .child(Text::new("Status: Connecting...").font_size(13.0))
            .child(Button::new("Refresh Telemetry"));

        Self {
            resource,
            receiver: rx,
            root_flex: root,
            cached_bounds: Rect::default(),
        }
    }

    /// Update internal child labels based on reactive async state.
    pub fn sync_view(&mut self) {
        // Pump channel to apply any completed background fetches
        self.resource.pump_channel(&self.receiver);

        let state = self.resource.state.get();

        if let Some(status_widget) = self.root_flex.child_mut(1) {
            if let Some(text) = status_widget.as_mut_any().downcast_mut::<Text>() {
                match state {
                    AsyncState::Uninitialized => {
                        text.set_content("Status: Idle");
                    }
                    AsyncState::Loading { progress, ref cached_previous } => {
                        let pct = progress.map(|p| (p * 100.0) as u32).unwrap_or(0);
                        if let Some(prev) = cached_previous {
                            text.set_content(format!(
                                "Status: Refreshing ({}%). Last CPU: {:.1}%",
                                pct, prev.cpu_usage
                            ));
                        } else {
                            text.set_content(format!("Status: Fetching telemetry ({}%)...", pct));
                        }
                    }
                    AsyncState::Ready(ref metrics) => {
                        text.set_content(format!(
                            "Node: {} | CPU: {:.1}% | RAM: {:.1}GB | Connections: {} | Up: {}h",
                            metrics.node_id,
                            metrics.cpu_usage,
                            metrics.memory_gb,
                            metrics.active_connections,
                            metrics.uptime_hours
                        ));
                    }
                    AsyncState::Error(ref err) => {
                        text.set_content(format!("Error: {}", err));
                    }
                }
            }
        }
    }
}

impl Widget for ClusterMonitorWidget {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        self.sync_view();
        self.root_flex.measure(cx, constraints)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.cached_bounds = bounds;
        self.sync_view();
        cx.layout_child(&mut self.root_flex, bounds);
    }

    fn paint(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        let k_rect = KurboRect::new(
            b.min_x() as f64,
            b.min_y() as f64,
            b.max_x() as f64,
            b.max_y() as f64,
        );

        let bg_color = cx.color(TokenKey::SurfaceColor, [28, 30, 36, 255]);
        let border_color = cx.color(TokenKey::BorderColor, [55, 60, 72, 255]);

        cx.list.push_fill_rounded_rect(k_rect, cx.pt(8.0), bg_color);
        cx.list.push_stroke_rect(k_rect, border_color, cx.pt(1.0));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        self.sync_view();
        let res = self.root_flex.event(cx);

        // Check if the Refresh button was clicked
        if res.is_handled() {
            self.resource.fetch("node-alpha-42".to_string());
            return EventResponse::RequestRepaint;
        }

        res
    }

    // Child protocol forwarding
    fn child_count(&self) -> usize { 1 }
    fn child(&self, i: usize) -> Option<&dyn Widget> {
        if i == 0 { Some(&self.root_flex) } else { None }
    }
    fn child_mut(&mut self, i: usize) -> Option<&mut dyn Widget> {
        if i == 0 { Some(&mut self.root_flex) } else { None }
    }
    fn child_bounds(&self, i: usize) -> Option<Rect> {
        if i == 0 { Some(self.cached_bounds) } else { None }
    }
}
```

---

## 3. Key Architectural Invariants

### 1. The UI Thread Synchronous Invariant
The primary rule of the Martensite engine is:
$$\text{Duration}(\text{measure}) + \text{Duration}(\text{layout}) + \text{Duration}(\text{paint}) < 8.33\,\text{ms}$$
Any invocation of `.await`, blocking I/O, synchronous mutex waiting, or heavy serialization on the main thread will immediately stall the compositor.

- Background workers operate on isolated thread pools or the Tokio runtime.
- Background tasks communicate exclusively via **lock-free or bounded MPSC queues** (`std::sync::mpsc`, `crossbeam_channel`).
- The main UI thread only performs `try_recv()` during its event or tick cycle, ensuring zero wait time.

### 2. Push-Pull Reactive Bridging
When data crosses from a worker thread to the UI thread, it must integrate cleanly into the reactive DAG:

```
[ Worker Thread ] ---> [ Thread-Safe MPSC Queue ]
                               |
                               | (Non-blocking try_recv on UI Frame)
                               v
                       [ UI Event Loop ]
                               |
                               v
                      batch(|| { signal.set(...) })
                               |
                               +---> [ Memo: derived_metrics ]
                               +---> [ Effect: repaint_scheduler ]
                               +---> [ NodeFlags::DIRTY_PAINT ]
```

1. Background workers do **not** invoke `signal.set()` directly across threads without thread-safe handles; instead, they post an envelope into the channel.
2. The UI thread pumps the channel inside `batch(|| ...)` so that multiple concurrent network arrivals update downstream memos in a single, coherent topological evaluation pass.

### 3. Request Generation & Cancellation Hygiene
A common bug in asynchronous UIs is the **Out-of-Order Arrival** race condition:
1. User searches for `"A"`. Request 1 launches (slow, 500ms).
2. User quickly types `"B"`. Request 2 launches (fast, 100ms).
3. Request 2 completes first; UI shows results for `"B"`.
4. Request 1 completes later; UI overwrites `"B"` with outdated results for `"A"`!

Martensite resolves this through **two-tier cancellation**:
- **Generational Request ID (`AtomicU64`)**: Each fetch increments an atomic counter. When a message is received from the channel, if `envelope.request_id != current_request_id.load()`, the message is silently discarded.
- **Cooperative Cancellation Token (`AtomicBool`)**: In-flight workers periodically check `token.is_cancelled()`. If cancelled, the worker halts processing before performing expensive parsing or allocations.

### 4. Stale Frame Prevention & Optimistic Rendering
When re-fetching active data (e.g. user clicks "Refresh" on a table), completely removing existing data to show a full-screen spinner causes jarring layout jumps and eye fatigue.

- **`stale_while_revalidate` Pattern**:
  Store the existing data inside `AsyncState::Loading { cached_previous: Some(data), .. }`.
- The UI continues rendering the existing table or metrics card with an overlay spinner or muted opacity indicator until the fresh payload arrives, delivering a fluid user experience.

### 5. Event Loop Wakeup & Quiescent Loop Coordination
When an application is idle, Martensite enters a low-power quiescent state (`QuiescentEventLoop`), parking the thread until OS events occur.
- When background tasks complete, they must call `event_loop_proxy.wake_up()` (or platform event signal) to unpark the UI thread.
- Upon wakeup, the event loop pumps the async channels, triggers reactive evaluations, and requests a repaint.

---

## 4. Common Pitfalls & Antipatterns

| Antipattern | Mechanism of Failure | Recommended Mitigation |
|---|---|---|
| **Blocking `.await` in Widgets** | Calling `futures::executor::block_on` inside `Widget::paint` freezes the entire window and crashes GPU swapchains. | Never block in widgets. Offload futures to `tokio::spawn` or background threads and bridge via channels. |
| **Out-of-Order Response Overwrite** | Superseded slow requests overwriting fresh fast requests due to lack of request IDs. | Tag every request with a monotonically increasing `request_id: u64` and discard stale arrivals. |
| **Thread Zombie Leaks** | Spawning unmonitored threads on every keystroke exhausts OS thread handles. | Use a shared worker thread pool, debounce rapid input signals, and check `CancellationToken`s. |
| **High-Frequency Channel Flooding** | Worker threads blasting 10,000 telemetry messages per second saturate the MPSC queue and starve UI rendering. | Batch or coalesce messages at the producer side (e.g. throttle to 60Hz) before sending to the UI channel. |
| **Uncaught Worker Panics** | A background worker thread panicking on JSON parsing drops the sender channel, silently stranding the UI in `Loading` forever. | Wrap background tasks in `std::panic::catch_unwind` and send an explicit `AsyncState::Error` envelope on panic. |

---

## Next Steps

- [Cookbook 02 — Reactive Data Binding](02-data-binding.md)
- [Cookbook 04 — Form State & Validation Pipelines](04-form-validation.md)
- [Cookbook 05 — Virtualized Lists & High-Performance DataGrids](05-virtualized-lists.md)
- [Tutorial 02 — Reactive State Management](../tutorials/02-reactive-state.md)
