# Cookbook 08 — Keyboard Navigation & Focus Traps

Accessible and efficient productivity software—ranging from code editors and CAD
suites to mission-critical industrial dashboards and TV/gamepad consoles—must be
fully operable without pointing devices. Ensuring complete keyboard accessibility
requires more than simple sequential tab stops: it demands **2D spatial directional
navigation**, **modal focus trapping**, **keyboard shortcut routing**, and
**flawless synchronization with OS assistive technology**.

This recipe demonstrates how to orchestrate keyboard interaction in Martensite
using **`FocusManager`**, the **2D projected-beam spatial navigation algorithm**,
modal **`FocusScope`** stacks, and **AccessKit** focus event synchronization.

---

## 1. Goal

Build a comprehensive keyboard and gamepad navigation architecture that:
1. Coordinates focus stops across the `WidgetArena` using `FocusManager`.
2. Supports standard 1D linear tab cycling (`TabNavigation::Forward` and `TabNavigation::Reverse`).
3. Executes 2D projected-beam directional navigation (`FocusDirection::Up`, `Down`, `Left`, `Right`) across non-linear widget grids using angular cone filtering.
4. Traps keyboard focus inside modal dialogs and popovers using `FocusScope`, preventing focus from escaping to background controls.
5. Automatically restores focus to the previously focused widget upon modal dismissal.
6. Intercepts and executes global and local keyboard shortcuts via `WidgetEvent::KeyPressed`.
7. Synchronizes focus updates bidirectionally with the platform accessibility tree via AccessKit.

---

## 2. Complete Runnable Pattern

The following pattern implements an Industrial Machine Operator Console (`MachineOperatorConsole`).
It features a top toolbar, a 2D grid of machine controls navigable with Arrow keys,
and a modal Emergency Shutdown confirmation dialog that traps focus securely.

```rust
use glam::Vec2;
use kurbo::{Point, Rect as KurboRect};
use martensite::prelude::*;
use martensite_core::widget::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Widget,
    WidgetEvent,
};
use martensite_core::{ColdNode, HotNode, NodeFlags, Rect, TokenKey, WidgetArena, WidgetId};
use martensite_focus::scope::FocusScope;
use martensite_focus::spatial::FocusDirection;
use martensite_focus::{FocusManager, TabNavigation};

/// A custom button widget that visually renders its active focus state.
pub struct FocusableButton {
    pub label: String,
    pub is_danger: bool,
    cached_bounds: Rect,
}

impl FocusableButton {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            is_danger: false,
            cached_bounds: Rect::default(),
        }
    }

    pub fn danger(mut self, danger: bool) -> Self {
        self.is_danger = danger;
        self
    }
}

impl Widget for FocusableButton {
    fn measure(&mut self, cx: &mut LayoutContext, _constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(cx.pt(140.0), cx.pt(40.0))
    }

    fn layout(&mut self, _cx: &mut LayoutContext, bounds: Rect) {
        self.cached_bounds = bounds;
    }

    fn paint(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        let k_rect = KurboRect::new(
            b.min_x() as f64,
            b.min_y() as f64,
            b.max_x() as f64,
            b.max_y() as f64,
        );

        // 1. Determine background color
        let bg_color = if self.is_danger {
            cx.color(TokenKey::ErrorColor, [220, 50, 50, 255])
        } else {
            cx.color(TokenKey::SurfaceColor, [45, 48, 56, 255])
        };
        cx.list.push_fill_rounded_rect(k_rect, cx.pt(6.0), bg_color);

        // 2. High-contrast focus indicator (WCAG 2.4.7 focus appearance)
        // If this widget holds focus in the active arena, draw a distinct double ring
        if cx.is_focused {
            let focus_ring = cx.color(TokenKey::PrimaryColor, [80, 160, 255, 255]);
            let outer_ring = k_rect.inset(cx.pt(2.0) as f64);
            cx.list.push_stroke_rounded_rect(outer_ring, cx.pt(8.0), focus_ring, cx.pt(2.5));
        }

        // 3. Render label
        let text_color = cx.color(TokenKey::TextColor, [240, 245, 250, 255]);
        let text_pos = Point::new(
            k_rect.x0 + cx.pt(12.0) as f64,
            k_rect.y0 + cx.pt(24.0) as f64,
        );
        cx.list.push_text(text_pos, self.label.clone(), cx.pt(13.0), text_color);
    }

    fn accessibility(&self, node: &mut accesskit::Node) {
        node.set_role(accesskit::Role::Button);
        node.set_label(self.label.as_str());
    }
}

/// Comprehensive operator console demonstrating 1D Tab, 2D Spatial, and Modal Scopes.
pub struct OperatorConsole {
    pub arena: WidgetArena,
    pub focus_manager: FocusManager,

    // Arena node handles
    pub toolbar_buttons: Vec<WidgetId>,
    pub grid_buttons: Vec<WidgetId>,

    // Modal dialog state
    pub modal_active: bool,
    pub modal_dialog_id: Option<WidgetId>,
    pub modal_confirm_btn: Option<WidgetId>,
    pub modal_cancel_btn: Option<WidgetId>,
}

impl OperatorConsole {
    pub fn new() -> Self {
        let mut arena = WidgetArena::new();
        let mut focus_manager = FocusManager::new();

        // 1. Create Toolbar Buttons (Horizontal Row)
        let mut toolbar = Vec::new();
        for name in ["Start Cycle", "Pause", "Calibrate", "Emergency Stop"] {
            let mut hot = HotNode::default();
            hot.flags |= NodeFlags::FOCUSABLE | NodeFlags::VISIBLE;

            let is_danger = name == "Emergency Stop";
            let cold = ColdNode::new(Box::new(FocusableButton::new(name).danger(is_danger)));
            let id = arena.insert(hot, cold);
            toolbar.push(id);
        }

        // 2. Create 2D Matrix of Actuators (3x3 Grid)
        let mut grid = Vec::new();
        for row in 0..3 {
            for col in 0..3 {
                let mut hot = HotNode::default();
                hot.flags |= NodeFlags::FOCUSABLE | NodeFlags::VISIBLE;
                // Assign layout bounds representing a 2D spatial grid
                hot.bounds = Rect::new(
                    col as f32 * 150.0 + 20.0,
                    row as f32 * 60.0 + 100.0,
                    140.0,
                    45.0,
                );

                let label = format!("Actuator {}-{}", row + 1, col + 1);
                let cold = ColdNode::new(Box::new(FocusableButton::new(label)));
                let id = arena.insert(hot, cold);
                grid.push(id);
            }
        }

        // Set initial focus to the first toolbar button
        if let Some(&first) = toolbar.first() {
            focus_manager.set_focus(&mut arena, first);
        }

        Self {
            arena,
            focus_manager,
            toolbar_buttons: toolbar,
            grid_buttons: grid,
            modal_active: false,
            modal_dialog_id: None,
            modal_confirm_btn: None,
            modal_cancel_btn: None,
        }
    }

    /// Present an Emergency Confirmation Modal and trap focus inside it.
    pub fn open_emergency_modal(&mut self) {
        if self.modal_active {
            return;
        }

        let prior_focus = self.focus_manager.current_focus();

        // 1. Create Modal Container
        let mut modal_hot = HotNode::default();
        modal_hot.flags |= NodeFlags::VISIBLE;
        modal_hot.bounds = Rect::new(100.0, 100.0, 320.0, 180.0);
        let modal_root = self.arena.insert(modal_hot, ColdNode::default());

        // 2. Create Modal Action Buttons
        let mut confirm_hot = HotNode::default();
        confirm_hot.flags |= NodeFlags::FOCUSABLE | NodeFlags::VISIBLE;
        confirm_hot.bounds = Rect::new(120.0, 200.0, 120.0, 40.0);
        let confirm_id = self.arena.insert(
            confirm_hot,
            ColdNode::new(Box::new(FocusableButton::new("CONFIRM STOP").danger(true))),
        );

        let mut cancel_hot = HotNode::default();
        cancel_hot.flags |= NodeFlags::FOCUSABLE | NodeFlags::VISIBLE;
        cancel_hot.bounds = Rect::new(260.0, 200.0, 120.0, 40.0);
        let cancel_id = self.arena.insert(
            cancel_hot,
            ColdNode::new(Box::new(FocusableButton::new("Cancel"))),
        );

        self.arena.set_parent(confirm_id, modal_root);
        self.arena.set_parent(cancel_id, modal_root);

        // 3. Push modal focus scope onto the stack (capturing prior focus)
        let scope = FocusScope::new(modal_root, prior_focus);
        self.focus_manager.push_scope(&mut self.arena, scope);

        // 4. Force initial focus onto the safe default action (Cancel)
        self.focus_manager.set_focus(&mut self.arena, cancel_id);

        self.modal_active = true;
        self.modal_dialog_id = Some(modal_root);
        self.modal_confirm_btn = Some(confirm_id);
        self.modal_cancel_btn = Some(cancel_id);
    }

    /// Dismiss the modal dialog and restore focus back to the triggering control.
    pub fn close_emergency_modal(&mut self) {
        if !self.modal_active {
            return;
        }

        // Pop focus scope: automatically restores focus to prior_focus
        let _popped_scope = self.focus_manager.pop_scope(&mut self.arena);

        // Cleanup modal nodes from the arena
        if let Some(confirm_id) = self.modal_confirm_btn.take() {
            self.arena.remove(confirm_id);
        }
        if let Some(cancel_id) = self.modal_cancel_btn.take() {
            self.arena.remove(cancel_id);
        }
        if let Some(root_id) = self.modal_dialog_id.take() {
            self.arena.remove(root_id);
        }

        self.modal_active = false;
    }

    /// Handle keyboard input across Tab, Arrow Keys, Escape, and Shortcuts.
    pub fn handle_key_event(&mut self, key: &str, shift_held: bool, ctrl_held: bool) {
        // Global Shortcut: Ctrl+E opens emergency modal
        if ctrl_held && (key == "e" || key == "E") {
            self.open_emergency_modal();
            return;
        }

        // Modal dismissal via Escape key
        if key == "Escape" && self.modal_active {
            self.close_emergency_modal();
            return;
        }

        match key {
            // 1D Linear Tab Navigation
            "Tab" => {
                let dir = if shift_held {
                    TabNavigation::Reverse
                } else {
                    TabNavigation::Forward
                };
                self.focus_manager.tab(&mut self.arena, dir);
            }

            // 2D Spatial Directional Navigation (Projected-beam cone search)
            "ArrowUp" => {
                self.focus_manager.navigate_directional(&mut self.arena, FocusDirection::Up);
            }
            "ArrowDown" => {
                self.focus_manager.navigate_directional(&mut self.arena, FocusDirection::Down);
            }
            "ArrowLeft" => {
                self.focus_manager.navigate_directional(&mut self.arena, FocusDirection::Left);
            }
            "ArrowRight" => {
                self.focus_manager.navigate_directional(&mut self.arena, FocusDirection::Right);
            }

            // Enter / Space activates the currently focused button
            "Enter" | " " => {
                if let Some(focused) = self.focus_manager.current_focus() {
                    if Some(focused) == self.modal_confirm_btn {
                        println!("ACTION: Emergency Shutdown Confirmed!");
                        self.close_emergency_modal();
                    } else if Some(focused) == self.modal_cancel_btn {
                        self.close_emergency_modal();
                    } else if Some(focused) == self.toolbar_buttons.last().copied() {
                        self.open_emergency_modal();
                    }
                }
            }

            _ => {}
        }
    }
}
```

---

## 3. Key Architectural Invariants

### 1. Arena-Level Focus Flags
In Martensite, focusability is an explicit structural property registered on the `HotNode` in the generational arena:
- `NodeFlags::FOCUSABLE`: Identifies nodes eligible for keyboard focus. The `FocusManager` unconditionally ignores nodes without this flag.
- `NodeFlags::VISIBLE`: Hidden (`UnderflowPolicy::Hide` or `UnderflowPolicy::Collapse`) nodes are automatically disqualified from focus candidate pools.
- When `FocusManager::set_focus` executes, it clears focus from the previous node and marks the new node with `NodeFlags::DIRTY_PAINT` so the focus indicator is re-rendered immediately.

### 2. The 2D Projected-Beam Navigation Algorithm
Linear `Tab` cycling fails on 2D layouts (matrices, video walls, complex parameter grids). Arrow-key navigation must calculate geometric adjacency.

Martensite uses the **Projected-Beam Vector Scoring Algorithm**:
$$\text{Score}(A, B, d) = \alpha \cdot \text{Distance}(A, B) + \beta \cdot \text{AngularDeviation}(AB, d)$$

```
          [ Node C ] (Rejected: outside 80° cone)
              ^
             /
            /  80° cone
 [ Node A ] ----------> [ Node B ] (Selected: Min Score in Direction d)
 (Source)   \
             \
              v
          [ Node D ] (Higher score: greater angular offset)
```

1. **Direction Vector ($d$)**: Cardinal unit vector $d \in \{(0, -1), (0, 1), (-1, 0), (1, 0)\}$.
2. **Forward Cone Rejection**: Candidates behind the source node or outside an $80^\circ$ half-angle cone (`FORWARD_CONE_DEGREES = 80.0`) are immediately discarded.
3. **Hyperparameters**:
   - Distance Weight: $\alpha = 1.0$ (`DEFAULT_ALPHA`)
   - Angular Weight: $\beta = 100.0$ (`DEFAULT_BETA`). The high $\beta$ penalizes diagonal drift, keeping navigation locked to collinear rows/columns.
4. **Deterministic Tie-Breaking**: If two candidates have identical scores, layout tree order breaks the tie deterministically.

### 3. Modal Focus Scopes & Auto-Restoration
A critical security and accessibility requirement in GUI architecture is preventing focus leakage through modal overlays (e.g. keyboard tabbing into background fields behind a dialog):

- **Scope Stack (`FocusScopeStack`)**: Modal containers push a `FocusScope` onto a LIFO stack.
- **Subtree Clamping**: While a scope is active, `tab()` and `navigate_directional()` restrict their candidate search strictly to descendants of `scope.root()`.
- **Prior Focus Restoration**:
  When `pop_scope()` is called upon dialog dismissal:
  1. It retrieves `scope.prior_focus()`.
  2. If the prior widget still exists and is visible, focus returns to it immediately.
  3. If the prior widget was destroyed while the modal was open, the manager safely cascades to the nearest valid in-scope sibling or the registered root container fallback (`FocusManager::set_root`).

### 4. AccessKit Focus Synchronization
Martensite maintains zero divergence between internal framework focus and OS accessibility services:
- Every change in `FocusManager::current_focus` triggers an `accesskit::ActionRequest` or updates the root `TreeUpdate::focus`.
- Screen readers (NVDA on Windows, VoiceOver on macOS, Orca on Linux) receive the OS focus event and instantly announce the newly focused element.

---

## 4. Common Pitfalls & Antipatterns

| Antipattern | Mechanism of Failure | Recommended Mitigation |
|---|---|---|
| **Modal Focus Escapes** | Opening a dialog without a `FocusScope` allows users to tab into hidden background inputs and press buttons invisibly. | Always push a `FocusScope::new(modal_root, prior_focus)` when presenting modals, drawers, or context menus. |
| **Orphaned Focus on Modal Close** | Dismissing a modal without restoring prior focus drops keyboard input into an unselectable void. | Always use `focus_manager.pop_scope()`, which restores `prior_focus` automatically. |
| **Low-Contrast Focus Rings** | Using subtle 1px gray focus outlines violates WCAG 2.4.7 (Focus Appearance) and makes navigation unusable in bright rooms. | Draw high-contrast focus rings ($3:1$ vs background, $\ge 2\text{px}$ thickness) using `TokenKey::PrimaryColor`. |
| **Focusing Non-Interactive Nodes** | Setting `NodeFlags::FOCUSABLE` on static text headers or background cards creates frustrating "dead" tab stops. | Reserve `FOCUSABLE` exclusively for interactive controls (buttons, inputs, sliders, list rows). |
| **Arrow Key Grid Trapping** | Relying solely on `Tab` for 2D grids forces users to press Tab 50 times to reach a specific cell. | Provide 2D spatial navigation on Arrow keys alongside standard Tab navigation. |

---

## Next Steps

- [Cookbook 01 — Responsive Layout & Underflow Policies](01-responsive-layout.md)
- [Cookbook 04 — Form State & Validation Pipelines](04-form-validation.md)
- [Cookbook 07 — Design Tokens & Dynamic Theming](07-theming-tokens.md)
- [Design Standards — WCAG 2.4 Focus Order & Appearance](../design-standards/rules/wcag-focus-order.md)
