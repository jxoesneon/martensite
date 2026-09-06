# Security & Supply Chain Policy

**Document Identifier:** DOC-0004-SECURITY
**Status:** Maintained
**Target:** v1.0.0

## 1. Threat Model

Martensite is a local graphical user interface engine. Its primary security mandates are:
* **Memory Integrity:** Preventing arbitrary code execution via buffer overflows, use-after-free, or invalid pointer math within the core framework.
* **Resource Exhaustion:** Defending the OS from memory leaks, GPU lockups, and CPU starvation caused by malicious or poorly formed UI declarations.
* **Data Isolation:** Ensuring the clipboard, drag-and-drop buffers, and file access APIs respect host OS sandboxing boundaries.

## 2. Supply Chain Purity

Martensite strictly governs its dependency graph via `cargo-deny`, enforced on every CI run.
* **Licenses:** Only `MIT`, `Apache-2.0`, `Zlib`, and `BSD-3-Clause` are permitted. Copyleft (GPL, AGPL) and proprietary licenses are permanently banned.
* **Duplicate Dependencies:** Multiple versions of the same crate (e.g., `winit` v0.29 and v0.30) are rejected to prevent binary bloat.
* **RUSTSEC Monitoring:** `cargo-audit` is run nightly against the RustSec Advisory Database. Vulnerable dependencies trigger an immediate high-priority issue.

## 3. The `unsafe` Policy

Martensite is built on the `forbid(unsafe_code)` guarantee wherever possible, particularly in `martensite-core`, `martensite-reactive`, and `martensite-layout`.

In subsystems requiring FFI or hardware interaction (e.g., `martensite-wgpu`, `martensite-window`):
* `unsafe` is strictly quarantined.
* Every `unsafe` block must be immediately preceded by a `// SAFETY:` block documenting the explicit preconditions required to avoid Undefined Behavior (UB).
* Adding new `unsafe` blocks requires explicit sign-off from the Lead Architect or the respective Working Group lead.

## 4. Sandbox Isolation

Martensite respects host OS sandboxes (macOS App Sandbox, Flatpak portals). 
* The engine will gracefully degrade if arbitrary filesystem access is restricted.
* Drag-and-drop and Clipboard operations utilize standard OS APIs, ensuring security entitlements and user-consent prompts (e.g., Wayland clipboard protocols) are properly surfaced by the compositor.

## 5. Vulnerability Disclosure Policy

Security issues should not be reported via public GitHub issues.
* **Reporting:** Email `security@martensite.dev`. 
* **Response Time:** The core team will acknowledge receipt within 48 hours.
* **Embargo:** We request a 90-day responsible disclosure embargo to patch and propagate fixes to crates.io before public disclosure. 
* Once resolved, a RustSec advisory will be published and an explicit `Security` section added to the CHANGELOG.
