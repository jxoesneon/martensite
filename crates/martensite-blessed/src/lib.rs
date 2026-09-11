//! Curated, production-oriented widgets for the Martensite GUI framework.
//!
//! The crate provides virtualized tabular data, GPU-friendly chart and audio
//! geometry, and a syntax-highlighted multi-cursor code editor.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

/// Interactive audio waveform model and geometry.
pub mod audio_waveform;
/// Two-dimensional chart series and auto-scaling.
pub mod chart;
/// Line-based syntax-highlighted editor.
pub mod code_editor;
/// Virtualized data table.
pub mod data_table;
/// Binary space partitioning docking tree.
pub mod docking;

pub use audio_waveform::AudioWaveform;
pub use chart::{AreaSeries, Chart, ChartBounds, LineSeries, Point, ScatterSeries};
pub use code_editor::{CodeEditor, Cursor, HighlightedSpan, TokenKind};
pub use data_table::{DataTable, TableStorage};
pub use docking::{
    DockDragSession, DockDropZone, DockError, DockNode, DockNodeLayout, DockNodeLayoutKind,
    DockPanel, DockTree, NodeId, Rect, SplitDirection,
};
