# Detailed Design Record: DDR-0014
## Title: `martensite-render` PaintList to Vello Scene Translation

### 1. Architectural Role & Invariants
`martensite-render` traverses the spatial geometry and styling attributes of the widget arena and produces a hardware-agnostic intermediate command list (`PaintList`), which is then compiled into a `vello::Scene` for compute-shader rasterization.
* **Invariant 1.1**: Z-index stacking contexts must precisely adhere to W3C DOM stacking rules.
* **Invariant 1.2**: Translation from `PaintList` to `vello::Scene` involves zero deep copies of pixel buffers.
* **Invariant 1.3**: Clip regions must perfectly nest, pushed and popped strictly in pairs.

### 2. PaintList Command Structure
```rust
use glam::{Vec2, Vec4};
use vello::peniko::{Color, Brush};

#[derive(Clone, Debug)]
pub enum PaintCmd {
    /// Draws a solid or gradient rectangle.
    Rect {
        bounds: Rect,
        radii: Vec4, // Top-left, top-right, bottom-right, bottom-left
        brush: Brush,
    },
    /// Pushes a clipping bounding box.
    PushClip(Rect),
    /// Pops the last pushed clip.
    PopClip,
    /// Renders a shaped text run.
    TextRun {
        origin: Vec2,
        glyphs: Vec<Glyph>, // References to font cache
        color: Color,
    },
}

pub struct PaintList {
    pub commands: Vec<PaintCmd>,
}
```

### 3. Vello Scene Translation Algorithm
```rust
pub fn compile_to_vello(paint_list: &PaintList, scene: &mut vello::Scene) {
    for cmd in &paint_list.commands {
        match cmd {
            PaintCmd::Rect { bounds, radii, brush } => {
                let rounded_rect = kurbo::RoundedRect::new(
                    bounds.origin.x as f64, bounds.origin.y as f64,
                    (bounds.origin.x + bounds.size.x) as f64, 
                    (bounds.origin.y + bounds.size.y) as f64,
                    radii.x as f64 // Simplified homogeneous
                );
                scene.fill(vello::peniko::Fill::NonZero, 
                           kurbo::Affine::IDENTITY, 
                           brush, None, &rounded_rect);
            },
            PaintCmd::PushClip(rect) => {
                let r = kurbo::Rect::new(rect.origin.x as f64, rect.origin.y as f64, (rect.origin.x + rect.size.x) as f64, (rect.origin.y + rect.size.y) as f64);
                scene.push_layer(vello::peniko::Mix::Clip, 1.0, kurbo::Affine::IDENTITY, &r);
            },
            PaintCmd::PopClip => scene.pop_layer(),
            // ... Text rendering ...
            _ => {}
        }
    }
}
```

### 4. Performance Invariants
- Command building is $O(V)$ where $V$ is visible nodes (frustum culled).
- Target latency: `PaintList` generation < 1.0ms. `vello::Scene` compile is deterministic and GPU offloaded.
