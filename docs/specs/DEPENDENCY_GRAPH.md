# Martensite Dependency Graph (v0.x -> v1.0.0)

**Document Identifier:** DOC-0001-DEP-GRAPH
**Status:** Canonical

## Crate Dependency DAG

```mermaid
graph TD
    %% Meta-crate
    martensite --> martensite-window
    martensite --> martensite-reactive
    martensite --> martensite-core
    martensite --> martensite-macros
    
    %% Windowing & Input
    martensite-window --> martensite-core
    martensite-window --> martensite-render
    martensite-window --> martensite-access
    martensite-window --> martensite-focus
    martensite-window --> martensite-clipboard
    martensite-window --> martensite-dnd

    %% Rendering
    martensite-render --> martensite-wgpu
    martensite-render --> martensite-text
    martensite-render --> martensite-theme
    martensite-render --> martensite-assets
    martensite-render --> martensite-media
    martensite-render --> martensite-core

    %% State & Architecture
    martensite-core --> martensite-layout
    martensite-core --> martensite-reactive
    martensite-core --> martensite-history
    
    %% Utilities
    martensite-motion --> martensite-core
    martensite-l10n --> martensite-core
    
    %% Testing & Dev
    martensite-devtools --> martensite-core
    martensite-test --> martensite-core
    martensite-test --> martensite-render
```

## Architectural Layers

1. **Foundational (Tier 0):** `martensite-wgpu`, `martensite-reactive`, `martensite-macros`
2. **Structural (Tier 1):** `martensite-core`, `martensite-layout`, `martensite-theme`
3. **Features (Tier 2):** `martensite-text`, `martensite-motion`, `martensite-history`, `martensite-access`
4. **Integration (Tier 3):** `martensite-render`, `martensite-window`
5. **Facade (Tier 4):** `martensite`
