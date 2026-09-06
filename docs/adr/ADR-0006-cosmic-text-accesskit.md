# [ADR-0006] Native Typography & Universal Accessibility as Day-Zero Primitives

* **Status:** Accepted
* **Date:** 2026-09-06
* **Deciders:** Master (Sovereign Architect), Ciel (Systems, Experience & Quality Guilds)
* **Technical Domain:** `martensite-text`, `martensite-access`, `martensite-core`

## Context and Problem Statement

Historically, GUI toolkits treat accessibility (a11y) and non-Latin typography as secondary afterthoughts or auxiliary bolt-on libraries. 
* **Typography Failure**: Engines rely on basic ASCII/Latin rasterizers (e.g., `ab_glyph`, `stb_truetype`). When confronted with Arabic cursive shaping, Hebrew bidirectional (BiDi) text, Indic consonant conjuncts (Devanagari/Tamil), or Japanese Kanji fallbacks, these engines render disconnected glyphs, incorrect reading orders, or replacement boxes ("tofu" `□`). Retrofitting HarfBuzz text shaping and Unicode bidirectional analysis (UAX #9) after layout engine stabilization requires destructive architectural rewrites.
* **Accessibility Failure**: Immediate-mode toolkits (`egui`) and procedural rendering loops lack a persistent semantic tree. Because widgets exist only for the microsecond of their procedural execution, screen readers (Windows UI Automation, macOS NSAccessibility, Linux AT-SPI2) cannot query persistent node hierarchies, inspect focus states, or dispatch asynchronous actions without brittle, high-latency frame-diffing shims. Furthermore, web-canvas hybrid approaches (Flutter Web) synthesize invisible, desynchronized HTML DOM overlays that break password managers and assistive hardware.

We must decide whether to defer typography and accessibility to post-v1.0 iterations or embed them as non-negotiable architectural primitives from Day Zero.

## Decision Drivers

* Guarantee zero "tofu" glyphs across all world languages from commit zero.
* Complete compliance with international accessibility mandates (ADA Title III, Section 508, EN 301 549) without separate accessibility trees.
* Elimination of parallel accessibility hierarchy overhead: the UI node graph and the semantic tree must synchronize via incremental delta updates.
* Pure-Rust supply chain purity: zero C/C++ build dependencies (`libharfbuzz-sys`, `libfreetype-sys`, `fontconfig`, `libatspi-2.0`).

## Considered Options

* **Option 1**: Basic Latin/ASCII rasterizer (`ab_glyph`) + deferred accessibility shims.
* **Option 2**: C FFI bindings to system libraries (`harfbuzz-sys`, `freetype-sys`, `fontconfig-sys`, `libatspi-sys`).
* **Option 3**: Universal pure-Rust integration of **`cosmic-text`** (`rustybuzz` + `swash` + `fontdb` + `unicode-bidi`) and **`accesskit`** (`accesskit_winit` + `accesskit_unix` via `zbus`).

## Decision Outcome

Chosen option: **Option 3**, because it delivers complete global typographic fidelity and native operating system accessibility without compromising Martensite's 100% pure-Rust cross-compilation mandate.

### Positive Consequences

* **Global Text Parity**: Complex scripts (Arabic, Hebrew, Devanagari, Thai, CJK) render with native OpenType feature evaluation (`GSUB`/`GPOS` ligature tables) and contextual glyph substitution.
* **Persistent Semantic Synchrony**: The generational slotmap arena maps 1:1 to persistent `accesskit::NodeId` handles. State mutations trigger incremental `accesskit::TreeUpdate` diffs with zero full-tree reconstruction overhead.
* **Zero Host C Dependencies**: Cross-compiling across Windows, macOS, and Linux requires zero host system libraries; Linux accessibility speaks directly to D-Bus via pure-Rust `zbus`.

### Negative Consequences

* **Initial Memory Footprint**: System font indexing via `fontdb` consumes ~4MB - 8MB of initial heap space to cache OS font metadata.
* **Layout Sizing Overhead**: Text bounding boxes require full glyph shaping runs during Taffy's intrinsic measurement pass (`MeasureFunc`), necessitating an aggressive multi-tier shaping cache.

## Technical Implementation Details

```rust
use cosmic_text::{Attrs, Buffer, Family, FontSystem, Metrics, Shaping, SwashCache};
use martensite_core::id::WidgetId;

pub struct TextRenderer {
    pub font_system: FontSystem,
    pub swash_cache: SwashCache,
}

impl TextRenderer {
    pub fn shape_text(
        &mut self,
        buffer: &mut Buffer,
        text: &str,
        metrics: Metrics,
        attrs: Attrs,
    ) {
        buffer.set_metrics(&mut self.font_system, metrics);
        buffer.set_text(&mut self.font_system, text, attrs, Shaping::Advanced);
        buffer.shape_until_scroll(&mut self.font_system, false);
    }
}
```
