# [ADR-0042] Authenticated Loopback WebSocket Bridge for the Dev Channel

* **Status:** Accepted
* **Date:** 2026-10-08
* **Deciders:** Martensite Architecture Working Group
* **Technical Domain:** `tools/cargo-martensite` (`dev-web` relay),
  `martensite-devtools` (`web_channel` wasm transport),
  `martensite-host` (`dev_channel` auth + method classification)
* **Amends:** None; companion to ADR-0038 (dev channel) and ADR-0039
  (MCP server). Implements the forwarder path ADR-0039 §"Future
  Extensibility & Platform Bridges" (item 6, line 78) pre-charters:
  *"Mobile and web runtime bridging … via authenticated local port
  forwarders … loopback WebSocket proxy … translating to the internal
  dev-channel socket without altering the application security
  profile."*

## Context and Problem Statement

ADR-0038 deliberately gives the dev channel no TCP listener — the socket
boundary is a Unix domain socket (named pipe on Windows) because the
remote-debugging exposure class is declined *by construction*. But a
browser-hosted Martensite app cannot open, or be reached over, that
socket: `wasm32-unknown-unknown` has no listening sockets and no Unix
socket API at all. The only transport a web target can originate is a
WebSocket the app *dials out* to.

That inverts the topology: the wasm app must act as the dev-channel
**server** over a client-initiated socket, and something on the host
must bridge local tools to it. The hazard is obvious — a plain
`ws://` listener is reachable from *any* web page in the user's browser
(`fetch`/`WebSocket` to loopback is unrestricted same-site), so an
unauthenticated bridge would let any website drive a running dev
session, including mutation-capable methods (`theme.set`,
`live.event.dispatch`, `signal.set`, `a11y.perform_action`). This is
precisely the Chrome `--remote-debugging-port` failure class ADR-0038
rejected; the bridge must reproduce its guarantees on the new hop.

## Decision Drivers

* The internal Unix socket remains the real dev-channel boundary —
  the relay is a translator, not a new security surface.
* Browser pages cannot be trusted by address alone; loopback needs
  explicit Origin filtering and a bearer credential.
* Read-mostly stays the default posture: mutation methods are a
  deliberate, flag-gated door (ADR-0038, ADR-0039 W5 semantics).
* Everything compiles out: no relay code in default builds, no
  WebSocket transport in wasm builds without the feature.
* `ws://` on loopback can never be reached from an HTTPS-hosted page
  anyway — browsers block it as mixed content / Private Network
  Access — so this is a **local-development-only** mechanism, never a
  hosted-tooling surface.

## Considered Options

* **Option 1**: Unauthenticated `ws://localhost` bridge — simplest, but
  reintroduces the exact exposure class ADR-0038 rejects; any local
  website could drive a running app.
* **Option 2**: TLS + certs on loopback (`wss://`) — heavy CA machinery
  for a dev-only tool; still needs auth underneath.
* **Option 3**: **Authenticated loopback WebSocket relay** — bind
  `127.0.0.1` only, per-run random bearer token, Origin allowlist,
  read-only method filter by default, `hello`-level bearer support on
  the internal channel, wasm-side `web-sys` transport leg, everything
  feature-gated.

## Decision Outcome

Chosen option: **Option 3**.

* **Command:** `cargo martensite dev-web [--port N] [--socket P]
  [--allow-mutations]` — explicit, opt-in, never started by default.
* **Bind:** the relay listens on `127.0.0.1` *only* (explicit IPv4
  loopback; never bare `localhost`, which could dual-bind `::1` and
  widen the origin surface). Default port `8788`.
* **Topology:** the wasm app dials
  `ws://127.0.0.1:<port>/dev-channel?token=<bearer>` and speaks the
  same newline-delimited JSON-RPC protocol as ADR-0038, acting as the
  dev-channel server. The relay binds the ordinary per-session Unix
  socket (`$XDG_RUNTIME_DIR/martensite/web.sock` or `--socket`) and is
  a **dumb pipe after authentication**: request lines in, response
  lines out, request ids renumbered/renumbered-back to correlate
  across concurrently bridged clients.
* **Authentication:**
  - The relay generates a fresh 256-bit bearer token per run and
    prints it for the operator; the app receives it via a page query
    parameter (`?dev_token=…`) or build-time flag.
  - The WS upgrade is refused unless the `token` query param matches
    and `Origin` is absent (non-browser tooling), `null` (file:// or
    sandboxed local pages), or `http(s)://{127.0.0.1,localhost,[::1]}:*`.
  - `HelloParams` gains an optional `auth_token`; a server configured
    with `DevChannelConfig::with_auth_token` rejects missing/mismatched
    tokens with a structured `auth_failed` error (-32002). **No
    configured token ⇒ behavior is unchanged** — existing Unix-socket
    clients keep working unauthenticated.
  - The relay injects the bearer into forwarded `hello` requests, so a
    wasm app that requires the token on its dev-channel server keeps
    end-to-end authentication without local tools knowing it.
* **Method classification:** every registered RPC method is statically
  classified `Read` or `Mutate` via `method_class()` in
  `martensite-host::dev_channel`. Unknown methods classify `Mutate`
  (conservative). The relay forwards `Read` methods only by default
  and answers blocked requests with `method_blocked` (-32010);
  `--allow-mutations` unlocks the full table.
* **Wasm transport:** `martensite_devtools::web_channel` provides a
  `web_sys::WebSocket` line transport (`WebDevChannel`,
  `WebChannelOptions`, `relay_url`) that speaks the same protocol and
  can require the bearer on incoming `hello`s. `martensite-host` is
  host-only, so the wasm leg exposes a protocol-dispatcher callback —
  the app wires it to its `DevSession`/handler equivalent.
* **Gating:** the relay needs `--features web-dev-channel` on
  `cargo-martensite` (dep: `tokio-tungstenite`); the wasm leg needs
  `--features web-dev-channel` on `martensite-devtools` and only
  compiles for `wasm32-unknown-unknown`. Neither is on by default, so
  release and gh-pages builds carry zero bridge code.

### Positive Consequences

* Browser-hosted dev apps become inspectable by the *same* local
  toolchain (`inspect`, `lint`, `mcp`, `tweak`) with no new protocol.
* The ADR-0038 surface stays narrow: the socket is still the boundary,
  still 0600/user-only, still version-handshook — the bridge is a
  translator pre-chartered by ADR-0039 §78.
* Per-run token + Origin allowlist + read-only default means a random
  web page cannot even *connect*, let alone mutate.
* Feature gating keeps release binaries and gh-pages artifacts free of
  the bridge.

### Negative Consequences

* The token must reach the app out-of-band (page query param or
  console copy) — an extra setup step versus native dev sessions.
* One attached web session at a time: a new WS connection replaces the
  previous one (page reloads churn sockets; pending requests fail).
* `ws://` cannot be used from HTTPS-hosted pages (mixed content / PNA),
  so the bridge is inherently **local-development-only** — a remote or
  hosted inspector would need its own ADR and transport story.
* The tool-facing side is Unix-socket-only for now; Windows named-pipe
  bridging reports `UnsupportedPlatform` until implemented.

## Links

* ADR-0038 (dev channel), ADR-0039 §"Future Extensibility &
  Platform Bridges" (forwarder path, line 78), ADR-0036, ADR-0037
* `tools/cargo-martensite/src/web_relay.rs`,
  `crates/martensite-devtools/src/web_channel.rs`,
  `crates/martensite-host/src/dev_channel.rs`
