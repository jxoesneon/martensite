# martensite-host

Lightweight host binary support for dynamically loading and hot-reloading
Martensite guest cdylibs.

This crate provides the dynamic-loading half of the v0.9.0 hot-reload
pipeline. It uses the `libloading` crate to wrap `dlopen`/`dlsym` (Unix) and
`LoadLibrary`/`GetProcAddress` (Windows) behind a safe, ergonomic API.

## Safety policy

This crate uses `#![allow(unsafe_code)]` at the crate level because dynamic
library loading is inherently unsafe. All `unsafe` blocks are confined to
the `GuestLibrary` methods that delegate to `libloading`. The workspace-level
`unsafe_code = "deny"` policy is preserved for all other crates; this is the
narrowly scoped audited exception described in the v0.9.0 host-binary
boundary decision, mirroring the policy used by `martensite-font-fallback`.
