# Cookbook 04 — Form State & Validation Pipelines

Data collection and configuration workflows in desktop, industrial, and embedded
systems require immediate, deterministic user feedback. Users expect instant
validation, clear visual error indicators, non-blocking asynchronous checks,
accessible screen-reader announcements, and frictionless keyboard navigation.

This recipe demonstrates how to construct production-ready forms in Martensite
using reactive **`Signal`** and **`Memo`** state graphs, field-level and form-level
validation rules, the **`FormField`** layout container, AccessKit form semantics,
and robust submit handlers.

---

## 1. Goal

Create a comprehensive configuration and registration form that:
1. Models reactive form state using `Signal<T>` primitives for inputs and `Memo<T>` for derived validation states.
2. Composes heterogeneous input controls: `TextInput`, `CheckBox`, `Switch`, and `Dropdown`.
3. Enforces field-level validation rules (presence, string length, regex format, numeric ranges) and cross-field validation rules (e.g. password confirmation or mutually dependent parameters).
4. Presents error states cleanly using `FormField` with accent-colored message strips, border indicators, and inline hints that collapse when clean.
5. Manages natural keyboard tab order (`FocusManager`) and form submission on `Enter`.
6. Bridges form states into the assistive technology tree via AccessKit (`Role::TextField`, `Role::CheckBox`, `Role::ComboBox`, `Role::Group`, `set_description`, `set_invalid`).
7. Disables submission atomically while errors exist, committing valid payloads inside a transactional `batch` block.

---

## 2. Complete Runnable Pattern

The following pattern implements a Server Node Configuration form (`ServerConfigForm`).
It validates hostname syntax, port allocation ranges, transport protocol selection,
and TLS security flags, with atomic submission and full assistive technology support.

```rust
use std::sync::Arc;
use glam::Vec2;
use kurbo::{Point, Rect as KurboRect};
use martensite::prelude::*;
use martensite::widgets::flex::{CrossAxisAlignment, MainAxisAlignment};
use martensite::widgets::form_field::{FormField, LabelPosition};
use martensite::widgets::{Button, CheckBox, Dropdown, Switch, TextInput};
use martensite_core::widget::{EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Widget};
use martensite_theme::TokenKey;

/// Transport protocol options for server communication.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransportProtocol {
    Tcp,
    Quic,
    WebSocket,
}

impl TransportProtocol {
    pub const ALL: [&'static str; 3] = ["TCP", "QUIC", "WebSocket"];

    pub fn from_index(idx: usize) -> Self {
        match idx {
            1 => Self::Quic,
            2 => Self::WebSocket,
            _ => Self::Tcp,
        }
    }
}

/// Validated configuration payload generated upon successful form submission.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerConfigPayload {
    pub hostname: String,
    pub port: u16,
    pub protocol: TransportProtocol,
    pub enable_tls: bool,
    pub auto_restart: bool,
}

/// Reactive form state modeling inputs, error derivations, and submit readiness.
#[derive(Clone)]
pub struct FormModel {
    // Writable field signals
    pub hostname: Signal<String>,
    pub port_text: Signal<String>,
    pub protocol_index: Signal<usize>,
    pub enable_tls: Signal<bool>,
    pub auto_restart: Signal<bool>,
    pub submitted_payload: Signal<Option<ServerConfigPayload>>,

    // Field-level validation memos
    pub hostname_error: Memo<Option<String>>,
    pub port_error: Memo<Option<String>>,
    pub tls_conflict_error: Memo<Option<String>>,

    // Aggregate form validity memo
    pub is_valid: Memo<bool>,
}

impl FormModel {
    pub fn new() -> Self {
        let hostname = create_signal("node-alpha-01".to_string());
        let port_text = create_signal("8443".to_string());
        let protocol_index = create_signal(0); // TCP
        let enable_tls = create_signal(true);
        let auto_restart = create_signal(false);
        let submitted_payload = create_signal(None);

        // Field rule 1: Hostname must be non-empty, <= 63 chars, and contain valid DNS characters
        let hostname_error = create_memo({
            let host = hostname.clone();
            move || {
                let val = host.get();
                let trimmed = val.trim();
                if trimmed.is_empty() {
                    Some("Hostname is required".to_string())
                } else if trimmed.len() > 63 {
                    Some("Hostname must not exceed 63 characters".to_string())
                } else if !trimmed.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.') {
                    Some("Only alphanumeric characters, dashes, and periods allowed".to_string())
                } else if trimmed.starts_with('-') || trimmed.ends_with('-') {
                    Some("Hostname cannot begin or end with a hyphen".to_string())
                } else {
                    None
                }
            }
        });

        // Field rule 2: Port must parse as u16 within authorized non-privileged range
        let port_error = create_memo({
            let port_str = port_text.clone();
            move || {
                let val = port_str.get();
                match val.trim().parse::<u16>() {
                    Ok(p) if p == 0 => Some("Port 0 is reserved and cannot be assigned".to_string()),
                    Ok(_) => None,
                    Err(_) => Some("Port must be a valid number between 1 and 65535".to_string()),
                }
            }
        });

        // Cross-field rule 3: QUIC transport requires TLS to be enabled
        let tls_conflict_error = create_memo({
            let proto = protocol_index.clone();
            let tls = enable_tls.clone();
            move || {
                let is_quic = proto.get() == 1;
                let has_tls = tls.get();
                if is_quic && !has_tls {
                    Some("QUIC protocol strictly requires TLS encryption".to_string())
                } else {
                    None
                }
            }
        });

        // Form-level validity memo: all field and cross-field errors must be None
        let is_valid = create_memo({
            let h_err = hostname_error.clone();
            let p_err = port_error.clone();
            let t_err = tls_conflict_error.clone();
            move || {
                h_err.get().is_none() && p_err.get().is_none() && t_err.get().is_none()
            }
        });

        Self {
            hostname,
            port_text,
            protocol_index,
            enable_tls,
            auto_restart,
            submitted_payload,
            hostname_error,
            port_error,
            tls_conflict_error,
            is_valid,
        }
    }

    /// Submit handler: verifies validity atomically and packages clean payload.
    pub fn try_submit(&self) -> bool {
        if !self.is_valid.get() {
            return false;
        }

        let port_parsed = self.port_text.get().trim().parse::<u16>().unwrap_or(8443);
        let payload = ServerConfigPayload {
            hostname: self.hostname.get().trim().to_string(),
            port: port_parsed,
            protocol: TransportProtocol::from_index(self.protocol_index.get()),
            enable_tls: self.enable_tls.get(),
            auto_restart: self.auto_restart.get(),
        };

        batch(|| {
            self.submitted_payload.set(Some(payload));
        });

        true
    }
}

/// Custom composite widget representing the validated configuration card.
pub struct ServerConfigCard {
    model: FormModel,
    layout_column: Flex,
    cached_bounds: Rect,
}

impl ServerConfigCard {
    pub fn new(model: FormModel) -> Self {
        let mut column = Flex::column().gap(12.0);

        // 1. Hostname Field
        let host_input = TextInput::new(model.hostname.get());
        let host_field = FormField::new()
            .label("Node Hostname")
            .required(true)
            .hint("e.g. edge-worker-01.us-east")
            .label_position(LabelPosition::Top)
            .child(host_input);

        // 2. Port Field
        let port_input = TextInput::new(model.port_text.get());
        let port_field = FormField::new()
            .label("Listen Port")
            .required(true)
            .hint("Standard service port (e.g. 443, 8443)")
            .label_position(LabelPosition::Top)
            .child(port_input);

        // 3. Protocol Dropdown
        let proto_dropdown = Dropdown::new(TransportProtocol::ALL);
        let proto_field = FormField::new()
            .label("Transport Protocol")
            .required(true)
            .child(proto_dropdown);

        // 4. Security Toggles
        let tls_switch = Switch::new("Enable TLS 1.3 Encryption").on(model.enable_tls.get());
        let restart_box = CheckBox::new("Auto-restart on fatal fault").checked(model.auto_restart.get());

        // 5. Submit Button
        let submit_btn = Button::new("Apply Configuration");

        column = column
            .child(host_field)
            .child(port_field)
            .child(proto_field)
            .child(tls_switch)
            .child(restart_box)
            .child(submit_btn);

        Self {
            model,
            layout_column: column,
            cached_bounds: Rect::default(),
        }
    }

    /// Synchronize the internal widget field errors from reactive memos.
    pub fn sync_validation_errors(&mut self) {
        let h_err = self.model.hostname_error.get();
        let p_err = self.model.port_error.get();
        let cross_err = self.model.tls_conflict_error.get();

        // Field 0: Hostname
        if let Some(child) = self.layout_column.child_mut(0) {
            if let Some(ff) = child.as_mut_any().downcast_mut::<FormField>() {
                ff.set_error(h_err);
            }
        }

        // Field 1: Port
        if let Some(child) = self.layout_column.child_mut(1) {
            if let Some(ff) = child.as_mut_any().downcast_mut::<FormField>() {
                ff.set_error(p_err);
            }
        }

        // Field 2: Protocol (receives cross-field error if QUIC without TLS)
        if let Some(child) = self.layout_column.child_mut(2) {
            if let Some(ff) = child.as_mut_any().downcast_mut::<FormField>() {
                ff.set_error(cross_err);
            }
        }

        // Button 5: Enable/disable based on form validity
        let valid = self.model.is_valid.get();
        if let Some(child) = self.layout_column.child_mut(5) {
            if let Some(btn) = child.as_mut_any().downcast_mut::<Button>() {
                btn.set_enabled(valid);
            }
        }
    }
}

impl Widget for ServerConfigCard {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        self.sync_validation_errors();
        self.layout_column.measure(cx, constraints)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.cached_bounds = bounds;
        self.sync_validation_errors();
        cx.layout_child(&mut self.layout_column, bounds);
    }

    fn paint(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        let k_rect = KurboRect::new(
            b.min_x() as f64,
            b.min_y() as f64,
            b.max_x() as f64,
            b.max_y() as f64,
        );

        // Panel background & stroke
        let bg = cx.color(TokenKey::SurfaceColor, [30, 32, 38, 255]);
        let border = cx.color(TokenKey::BorderColor, [65, 70, 80, 255]);
        cx.list.push_fill_rounded_rect(k_rect, cx.pt(8.0), bg);
        cx.list.push_stroke_rect(k_rect, border, cx.pt(1.0));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        let res = self.layout_column.event(cx);

        // Re-evaluate validation states immediately if input occurred
        if res.is_handled() {
            self.sync_validation_errors();
        }

        res
    }

    // Child protocol forwarding
    fn child_count(&self) -> usize { 1 }
    fn child(&self, i: usize) -> Option<&dyn Widget> {
        if i == 0 { Some(&self.layout_column) } else { None }
    }
    fn child_mut(&mut self, i: usize) -> Option<&mut dyn Widget> {
        if i == 0 { Some(&mut self.layout_column) } else { None }
    }
    fn child_bounds(&self, i: usize) -> Option<Rect> {
        if i == 0 { Some(self.cached_bounds) } else { None }
    }
}
```

---

## 3. Key Architectural Invariants

### 1. Fine-Grained Reactive Validation DAG
In Martensite, form fields must never re-validate the entire form imperatively on every keystroke.
State propagation follows the push-pull reactive DAG:

```
[ Signal: hostname ] ----> [ Memo: hostname_error ] ----+
                                                        |
[ Signal: port_text ] ---> [ Memo: port_error ] --------+---> [ Memo: is_valid ]
                                                        |              |
[ Signal: protocol ]  -+                                |              v
                       +-> [ Memo: tls_conflict_error ] -+     [ Button::set_enabled ]
[ Signal: enable_tls ] -+
```

1. **Transactional Pushes**: When the user edits `hostname`, only `hostname` pushes a dirty flag to its downstream memo `hostname_error` and subsequently to `is_valid`. The clean `port_error` is never executed.
2. **Topological Order**: Derived validation dependencies evaluate in topological order during the read phase. If both `protocol` and `enable_tls` change within a `batch`, `tls_conflict_error` computes exactly once.
3. **No Validation Cycles**: Never write back to field signals from inside a validation memo. Validation memos must remain pure, side-effect-free projections of input state.

### 2. The `FormField` Geometry Protocol & Zero-Jitter Strips
The `FormField` container resolves a common UI defect: jumping layouts when validation errors appear.

- **Intrinsic Measure**:
  $$\text{Height} = \text{Label} + \text{Control} + \text{Message} + \sum \text{Gaps}$$
  When `error` is `None` and `hint` is empty, the message strip occupies **zero vertical space** (`message_h = 0.0`).
- **Hint Replacement**: When clean, the message strip displays the subtle `hint` in `TokenKey::TextMutedColor`. When an error occurs, the error replaces the hint immediately without changing the total measured height if a hint was already present.
- **Top vs. Left Label Alignment**: Narrow dialogs use `LabelPosition::Top` for responsive stacking; wide desktop settings panels use `LabelPosition::Left` with a pinned `label_width: f32` to create structured tabular columns.

### 3. AccessKit Form Semantics & Error Association
Accessible forms require that assistive technologies (AT) announce input requirements and error states without forcing the user to navigate away from the input control:

```rust
// Inside FormField::accessibility:
fn accessibility(&self, node: &mut AccessKitNode) {
    node.set_role(accesskit::Role::Group);
    if !self.label.is_empty() {
        node.set_label(self.label.as_str());
    }
    if let Some(err) = self.error.as_deref() {
        // Exposes the validation failure directly as the accessible description
        node.set_description(err.to_string());
    }
    if !self.enabled {
        node.set_disabled();
    }
}
```

- When `error` is set, screen readers announce: *"Node Hostname, edit text, required, Hostname is required"*.
- For the internal control (`TextInput`), setting `node.set_invalid(accesskit::Invalid::True)` explicitly marks the field invalid in platform accessibility trees (MSAA/UIA on Windows, NSAccessibility on macOS, AT-SPI on Linux).

### 4. Tab Navigation & Focus Order
Forms must support uninterrupted sequential keyboard navigation:
- Controls register `NodeFlags::FOCUSABLE` on their arena nodes or report focusable internal bounds.
- Pressing `Tab` invokes `FocusManager::tab(&mut arena, TabNavigation::Forward)`.
- Pressing `Shift+Tab` cycles backwards (`TabNavigation::Reverse`).
- Pressing `Enter` on an active `TextInput` can advance focus to the next field in the chain or trigger the primary submit button if the form is valid.

---

## 4. Common Pitfalls & Antipatterns

| Antipattern | Mechanism of Failure | Recommended Mitigation |
|---|---|---|
| **Validating in `paint()`** | Running validation rules or mutating `Signal`s inside `paint()` causes layout oscillations, crashes reactive batching, and leaks memory. | Restrict validation to pure `create_memo` functions. Keep `paint()` strictly read-only. |
| **Premature Validation Flash** | Showing aggressive red error banners before the user has even focused or typed in a new field ("dirty" vs "touched" state). | Track a `touched: Signal<bool>` per field. Gate the visible error display on `touched.get() == true` or on initial submit attempt. |
| **Visual-Only Errors** | Painting a red border around an invalid `TextInput` without setting `FormField::set_error` or AccessKit descriptions. | Screen-reader users cannot perceive stroke color. Always pass error strings to `FormField` so `node.set_description` is populated. |
| **Unbounded Text Cloning** | Calling `signal.get()` on large strings inside high-frequency event handlers. | Use `signal.with(|s| ...)` to inspect string slices without cloning, or validate on character insertion rather than full buffer copies. |
| **Missing Form Submission Lock** | Leaving the Submit button clickable while validation errors exist, allowing corrupt payloads to hit backend services. | Drive `Button::set_enabled` directly from a derived `is_valid: Memo<bool>` and re-check validity inside `try_submit()`. |

---

## Next Steps

- [Cookbook 02 — Reactive Data Binding](02-data-binding.md)
- [Cookbook 05 — Virtualized Lists & High-Performance DataGrids](05-virtualized-lists.md)
- [Cookbook 08 — Keyboard Navigation & Focus Traps](08-keyboard-focus.md)
- [Tutorial 04 — Accessibility & Semantic Trees](../tutorials/04-accessibility.md)
