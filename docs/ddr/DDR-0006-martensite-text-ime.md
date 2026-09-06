# [DDR-0006] Text Subsystem & Global IME Bounds Projection Specification

* **Subsystem:** `martensite-text`
* **Status:** Approved
* **Authors:** Martensite Architecture Working Group
* **Related ADRs:** ADR-0006

## 1. System Topology & Mathematical Layout Invariants

`martensite-text` is the typography, bidirectional shaping, and input-method projection engine for Martensite.
It binds `cosmic-text` (built on `rustybuzz`, `swash`, and `fontdb`) to the generational layout arena and native OS input channels.

### 1.1 Structural Layout & Invariants
```
┌──────────────────────────────────────────────────────────────┐
│                     MARTENSITE TEXT STACK                    │
├──────────────────────────────────────────────────────────────┤
│  Winit Window Events (Ime::Preedit, Ime::Commit)             │
│                            │                                 │
│                            ▼                                 │
│  `TextEditor` / `TextBuffer` (Cosmic-Text Core)              │
│    • In-memory UTF-8 text representation                     │
│    • Cursor & Selection offsets (byte indices & graphemes)   │
│                            │                                 │
│                            ▼                                 │
│  BiDi & Shaping Pipeline (`rustybuzz` + `swash`)             │
│    • Unicode Annex #9 BiDi run resolution                    │
│    • Glyph ID, advance, and cluster mapping                  │
│                            │                                 │
│                            ▼                                 │
│  Taffy Layout Bridge (`MeasureFunc`)                         │
│    • Intrinsic bottom-up text run measurement                │
│    • Line-breaking & hyphenation constraints                 │
│                            │                                 │
│                            ▼                                 │
│  IME Screen-Space Projection                                 │
│    • Local glyph run coordinates -> Global OS Window space   │
│    • macOS Cocoa / Windows TSF / Wayland text-input-v3       │
└──────────────────────────────────────────────────────────────┘
```

* **Invariant 1.1 (Unicode Parity)**: All text operations must execute on extended grapheme cluster boundaries via `unicode-segmentation`.
* **Invariant 1.2 (Sub-Pixel Precision)**: Glyph layout coordinates are tracked as 26.6 fixed-point or IEEE 754 f32 to eliminate rounding errors during fractional DPI scaling.
* **Invariant 1.3 (Candidate Alignment)**: The IME composition candidate window must anchor with zero visual latency to the physical screen bounding box of the active insertion caret.
