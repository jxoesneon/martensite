//! First-party Model Context Protocol (MCP) server library for Martensite.
//!
//! Provides native semantic observability, Taffy layout chain introspection,
//! design-standard lint diagnostics, AccessKit semantic tree inspection,
//! and live tweak synchronization for AI coding agents.
//!
//! Architecture (ADR-0039, `docs/dx/MCP.md`):
//!
//! - [`tools`] — the 28 `martensite_*` tools, implemented as `rmcp`
//!   `ToolBase`/`AsyncTool` units and merged into one `ToolRouter`.
//! - [`client`] — ADR-0038 dev-channel client (local Unix socket / named
//!   pipe, newline-delimited JSON-RPC-lite, D1 version-locked `hello`).
//! - [`offline`] — offline-mode context: workspace discovery, scene dumps,
//!   disk-mutation confinement, and the mutation audit log.
//! - [`server`] — the [`MartensiteMcp`](server::MartensiteMcp) type, the
//!   `rmcp::ServerHandler` wiring, and [`serve_stdio`].
//! - [`resources`] / [`prompts`] — MCP resource and prompt providers.
//! - [`types`] — shared serializable descriptor payloads.
//! - [`error`] — [`McpError`](error::McpError) and its JSON-RPC code mapping.
//!
//! # Examples
//!
//! ```
//! use martensite_mcp::error::McpError;
//!
//! let err = McpError::NodeNotFound("root".to_string());
//! assert_eq!(err.code(), -32004);
//! ```

#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! The MCP server drives a live Martensite session over stdio — a
//! host-side development tool boundary. On `wasm32-unknown-unknown`
//! the crate compiles to an empty library and its deps (rmcp, tokio)
//! are target-gated to match.
#![cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]

pub mod client;
pub mod error;
pub mod offline;
pub mod prompts;
pub mod resources;
pub mod server;
pub mod tools;
pub mod types;

pub use error::McpError;
pub use server::{serve_stdio, MartensiteMcp, McpServerOptions, ServerMode};
