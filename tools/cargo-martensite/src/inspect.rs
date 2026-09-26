//! Headless widget inspector attached to a running Martensite development session.
//!
//! Provides the `cargo martensite inspect` subcommand specified in `docs/dx/CLI.md`.
//!
//! Connects over the ADR-0038 dev-channel Unix domain socket to query the live widget
//! hierarchy, inspection select-mode (`--pick`), layout constraint chains, and active
//! signal states.
//!
//! # Examples
//!
//! ```no_run
//! use cargo_martensite::inspect::{run_inspect, InspectOptions};
//!
//! let options = InspectOptions::default();
//! let _ = run_inspect(&options);
//! ```

use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use crate::dev_channel::{
    discover_socket, DevChannelError, DevClient, InspectSelectData, TreeSnapshotData,
};
use crate::lint::OutputFormat;

/// Options configuring the `inspect` subcommand run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InspectOptions {
    /// Stream continuous updates as the UI tree changes.
    pub follow: bool,
    /// Wait for the user to click a widget in the app and display that node.
    pub pick: bool,
    /// Presentation format (`text` or `json`).
    pub format: OutputFormat,
    /// Socket path override for dev-channel IPC.
    pub socket: Option<PathBuf>,
    /// Whether to ignore version handshake mismatches.
    pub allow_version_mismatch: bool,
}

impl Default for InspectOptions {
    fn default() -> Self {
        Self {
            follow: false,
            pick: false,
            format: OutputFormat::Text,
            socket: None,
            allow_version_mismatch: false,
        }
    }
}

/// Errors produced during headless inspector execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InspectError {
    /// Dev channel communication failure (exit code 3).
    DevChannel(DevChannelError),
    /// I/O error during output generation.
    Io(String),
}

impl fmt::Display for InspectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InspectError::DevChannel(err) => write!(f, "{err}"),
            InspectError::Io(msg) => write!(f, "I/O error: {msg}"),
        }
    }
}

impl std::error::Error for InspectError {}

impl From<DevChannelError> for InspectError {
    fn from(err: DevChannelError) -> Self {
        InspectError::DevChannel(err)
    }
}

/// Executes the `cargo martensite inspect` subcommand.
pub fn run_inspect(options: &InspectOptions) -> Result<(), InspectError> {
    let socket_path = discover_socket(options.socket.as_deref())?;
    let mut client = DevClient::connect(&socket_path, options.allow_version_mismatch)?;

    if options.pick {
        // Arm select mode and wait for user click in app.
        println!("Inspector select mode armed. Click a widget in the application window...");
        let select_data = client.inspector_select()?;
        render_inspect_select(&select_data, options.format)?;
        return Ok(());
    }

    if options.follow {
        // Stream tree updates.
        let mut last_fingerprint = None;
        println!("Following widget tree updates (Ctrl+C to stop)...");

        // Bounded loop or continuous poll.
        loop {
            let snapshot = client.tree_snapshot()?;
            let fp = format!("{:?}", snapshot.root);
            if last_fingerprint.as_deref() != Some(fp.as_str()) {
                last_fingerprint = Some(fp);
                render_tree_snapshot(&snapshot, options.format)?;
                println!("---");
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    } else {
        // One-shot tree snapshot.
        let snapshot = client.tree_snapshot()?;
        render_tree_snapshot(&snapshot, options.format)?;
    }

    Ok(())
}

/// Renders a tree snapshot in the chosen presentation format.
fn render_tree_snapshot(
    snapshot: &TreeSnapshotData,
    format: OutputFormat,
) -> Result<(), InspectError> {
    match format {
        OutputFormat::Json => {
            let json = serde_json::to_string_pretty(snapshot)
                .map_err(|e| InspectError::Io(e.to_string()))?;
            println!("{json}");
        }
        OutputFormat::Text => {
            print!("{}", snapshot.root.to_tree_string());

            if let Some(id) = snapshot.selected_id {
                println!("\nSelected widget ID: {id}");
            }

            if !snapshot.layout_chain.is_empty() {
                println!("\nLayout constraint chain:");
                for (i, step) in snapshot.layout_chain.iter().enumerate() {
                    let violations = if step.violations.is_empty() {
                        String::new()
                    } else {
                        format!(" [VIOLATION: {}]", step.violations.join(", "))
                    };
                    println!(
                        "  {}. {} -> {} => [{:.0}, {:.0}]{violations}",
                        i + 1,
                        step.name,
                        step.constraints,
                        step.result_size[0],
                        step.result_size[1]
                    );
                }
            }

            if !snapshot.signals.is_empty() {
                println!("\nActive signals:");
                for sig in &snapshot.signals {
                    println!("  • {sig}");
                }
            }
        }
    }
    Ok(())
}

/// Renders inspection data for a picked widget.
fn render_inspect_select(
    data: &InspectSelectData,
    format: OutputFormat,
) -> Result<(), InspectError> {
    match format {
        OutputFormat::Json => {
            let json =
                serde_json::to_string_pretty(data).map_err(|e| InspectError::Io(e.to_string()))?;
            println!("{json}");
        }
        OutputFormat::Text => {
            let node = &data.selected_node;
            println!("Selected Node:");
            println!("  Label:         {}", node.display_label());
            println!("  ID:            {}", node.id);
            println!("  Kind:          {}", node.kind);
            println!(
                "  Screen Bounds: [{:.1}, {:.1} {:.1}×{:.1}]",
                node.screen_bounds[0],
                node.screen_bounds[1],
                node.screen_bounds[2],
                node.screen_bounds[3]
            );
            println!(
                "  Local Bounds:  [{:.1}, {:.1} {:.1}×{:.1}]",
                node.local_bounds[0],
                node.local_bounds[1],
                node.local_bounds[2],
                node.local_bounds[3]
            );

            if !node.badges.is_empty() {
                println!("  Badges:        {}", node.badges.join(", "));
            }

            if !data.properties.is_empty() {
                println!("\nProperties:");
                for (k, v) in &data.properties {
                    println!("  {k}: {v}");
                }
            }

            if !data.layout_chain.is_empty() {
                println!("\nLayout Constraint Chain:");
                for (i, step) in data.layout_chain.iter().enumerate() {
                    let violations = if step.violations.is_empty() {
                        String::new()
                    } else {
                        format!(" [VIOLATION: {}]", step.violations.join(", "))
                    };
                    println!(
                        "  {}. {} -> {} => [{:.1}, {:.1}]{violations}",
                        i + 1,
                        step.name,
                        step.constraints,
                        step.result_size[0],
                        step.result_size[1]
                    );
                }
            }
        }
    }
    Ok(())
}
