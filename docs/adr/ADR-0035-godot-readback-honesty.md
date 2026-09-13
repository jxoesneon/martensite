# [ADR-0035] Godot Readback Honesty — Two Tiers, No Zero-Copy Claim

* **Status:** Accepted
* **Date:** 2026-09-12
* **Deciders:** Martensite Architecture Working Group
* **Technical Domain:** `martensite-godot`, `martensite-engine-bridge`,
  `martensite-wgpu`
* **Amends:** None; implements ADR-0033 (host-mode embedding) for the
  Godot adapter and records the copy-count contract the milestone
  promised to document.

## Context and Problem Statement

The v0.15.0 spec requires Martensite to display a live Godot 4
viewport. Deep research established that **true zero-copy from Godot is
impossible without upstream engine patches**:

* `RenderingDevice::texture_get_native_handle` /
  `get_driver_resource` return API object pointers (`VkImage`,
  `ID3D12Resource*`, `id<MTLTexture>`) — not cross-process or
  shareable handles.
* Godot's internal render targets are **not** allocated with export
  flags (`D3D12_HEAP_FLAG_SHARED`, `VkExportMemoryAllocateInfo`,
  IOSurface backing), and a GDExtension cannot retroactively add them.
* GDExtension exposes **no fence/semaphore API**; the only public
  completion primitive is the CPU-side `texture_get_data_async`
  callback.
* `libgodot` (merged to Godot master) puts Godot in-process but does
  not solve logical-device sharing.

Shipping a "zero-copy Godot viewport" claim under these constraints
would be dishonest. The decision is how to ship something useful while
stating the real copy counts.

## Decision Drivers

* Ship a path that works on every Godot driver (Vulkan/D3D12/Metal/GL)
  and every platform.
* Never claim zero-copy for Godot; publish measured
  throughput/latency instead.
* Keep the experimental faster path opt-in and clearly labeled
  one-GPU-copy.
* Record the exact upstream changes that would unlock true zero-copy
  as contribution opportunities.

## Considered Options

* **Option 1**: Readback only — `texture_get_data_async` → transport →
  `queue.write_texture`.
* **Option 2**: Shared-texture blit only — import a Martensite-allocated
  exportable texture into Godot and `CompositorEffect`-copy into it.
* **Option 3**: **Two tiers** — Tier 1 readback as the shipped default;
  Tier 2 shared-texture blit behind the `godot-gpu-copy` feature flag,
  documented as experimental.

## Decision Outcome

Chosen option: **Option 3**.

* **Tier 1 — readback (shipped):** `SubViewport` → `ViewportTexture` →
  `texture_get_rd_texture` → `RenderingDevice::texture_get_data_async`
  → transport channel → `queue.write_texture` into the viewport
  texture. Exactly **one GPU→CPU copy plus one CPU→GPU upload** —
  stated plainly in docs; throughput and latency are measured and
  published, not hidden.
* **Tier 2 — `godot-gpu-copy` (experimental, feature-gated):**
  Martensite allocates a shareable wgpu texture, imports it into Godot
  via `texture_create_from_extension`, and a `CompositorEffect` at
  `EFFECT_CALLBACK_TYPE_POST_TRANSPARENT` blits the
  `RenderSceneBuffersRD` color buffer into it — **one GPU→GPU copy**.
  Synchronization is best-effort (one-frame delay tolerance) because
  Godot exposes no fence API.

True zero-copy requires four upstream Godot changes, documented as
upstream-contribution opportunities: (1) export-flagged internal render
targets, (2) external-texture render targets, (3) a public
fence/semaphore API in GDExtension, (4) merged `libgodot` native-window
support.

### Positive Consequences

* A working Godot viewport on all drivers today, with honestly stated
  cost.
* Tier 2 provides the fast path for users who accept one GPU copy and
  best-effort sync.
* The four upstream requirements convert "impossible" into an
  actionable contribution list.

### Negative Consequences

* Every shipped path copies at least once; latency-sensitive uses
  (e.g., VR) are not served until upstream lands the patches.
* `martensite-godot` is a `cdylib` FFI boundary: it joins the
  `AGENTS.md` allowed-`unsafe` list as the **eighth** crate (unsafe is
  confined to the extension; every `unsafe` block on the Tier-2
  shared-handle path is documented). It is `publish = false` and
  excluded from the default workspace build.
