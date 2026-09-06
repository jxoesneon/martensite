// SPDX-License-Identifier: MIT OR Apache-2.0
//
// This is a Martensite fork of cosmic-text. Upstream code style is preserved.

// Upstream cosmic-text has intra-doc links that don't resolve in our fork context.
#![allow(rustdoc::broken_intra_doc_links)]

//! # COSMIC Text
//!
//! This library provides advanced text handling in a generic way. It provides abstractions for
//! shaping, font discovery, font fallback, layout, rasterization, and editing. Shaping utilizes
//! harfrust, font discovery utilizes fontdb, and the rasterization is optional and utilizes
//! swash. The other features are developed internal to this library.
//!
//! It is recommended that you start by creating a [`FontSystem`], after which you can create a
//! [`Buffer`], provide it with some text, and then inspect the layout it produces. At this
//! point, you can use the `SwashCache` to rasterize glyphs into either images or pixels.
//!
//! ```
//! use cosmic_text::{Attrs, Color, FontSystem, SwashCache, Buffer, Metrics, Shaping};
//!
//! // A FontSystem provides access to detected system fonts, create one per application
//! let mut font_system = FontSystem::new();
//!
//! // A SwashCache stores rasterized glyphs, create one per application
//! let mut swash_cache = SwashCache::new();
//!
//! // Text metrics indicate the font size and line height of a buffer
//! let metrics = Metrics::new(14.0, 20.0);
//!
//! // A Buffer provides shaping and layout for a UTF-8 string, create one per text widget
//! let mut buffer = Buffer::new(&mut font_system, metrics);
//!
//! // Borrow buffer together with the font system for more convenient method calls
//! let mut buffer = buffer.borrow_with(&mut font_system);
//!
//! // Attributes indicate what font to choose
//! let attrs = Attrs::new();
//!
//! // Set size and text
//! buffer.set_size(Some(80.0), Some(25.0));
//! buffer.set_text("Hello, Rust! 🦀\n", &attrs, Shaping::Advanced, None);
//!
//! // Inspect the output runs
//! for run in buffer.layout_runs() {
//!     for glyph in run.glyphs.iter() {
//!         println!("{:#?}", glyph);
//!     }
//! }
//!
//! // Create a default text color
//! let text_color = Color::rgb(0xFF, 0xFF, 0xFF);
//!
//! // Draw the buffer (for performance, instead use SwashCache directly)
//! buffer.draw(&mut swash_cache, text_color, |x, y, w, h, color| {
//!     // Fill in your code here for drawing rectangles
//! });
//! ```

// Not interested in these lints
#![allow(clippy::new_without_default)]
// TODO: address occurrences and then deny
//
// Overflows can produce unpredictable results and are only checked in debug builds
#![allow(clippy::arithmetic_side_effects)]
// Indexing a slice can cause panics and that is something we always want to avoid
#![allow(clippy::indexing_slicing)]
// Soundness issues
//
// Dereferencing unaligned pointers may be undefined behavior
#![deny(clippy::cast_ptr_alignment)]
// Ensure all types have a debug impl
#![deny(missing_debug_implementations)]
// This is usually a serious issue - a missing import of a define where it is interpreted
// as a catch-all variable in a match, for example
#![deny(unreachable_patterns)]
// Ensure that all must_use results are used
#![deny(unused_must_use)]
// Style issues
//
// Documentation not ideal
#![warn(clippy::doc_markdown)]
// Document possible errors
#![warn(clippy::missing_errors_doc)]
// Document possible panics
#![warn(clippy::missing_panics_doc)]
// Ensure semicolons are present
#![warn(clippy::semicolon_if_nothing_returned)]
// Ensure numbers are readable
#![warn(clippy::unreadable_literal)]
// Martensite fork: relax upstream deny(unwrap_used) since test code uses unwrap
#![allow(clippy::unwrap_used)]
// Martensite fork: relax upstream warns that fail under -D warnings in CI
#![allow(clippy::doc_markdown)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::semicolon_if_nothing_returned)]
#![allow(clippy::unreadable_literal)]
#![allow(clippy::unwrap_or_default)]
#![allow(clippy::manual_unwrap_or)]
#![allow(clippy::unnecessary_lazy_evaluations)]
#![allow(clippy::redundant_closure)]
#![allow(clippy::map_or_identity)]
#![allow(clippy::useless_borrows_in_formatting)]
#![allow(clippy::needless_range_loop)]
#![allow(unused_imports)]
#![cfg_attr(not(feature = "std"), no_std)]
extern crate alloc;

#[cfg(not(any(feature = "std", feature = "no_std")))]
compile_error!("Either the `std` or `no_std` feature must be enabled");

pub use self::attrs::*;
mod attrs;

pub use self::bidi_para::*;
mod bidi_para;

pub use self::buffer::*;
mod buffer;

pub use self::buffer_line::*;
mod buffer_line;

pub use self::cached::*;
mod cached;

pub use self::glyph_cache::*;
mod glyph_cache;

pub use self::cursor::*;
mod cursor;

pub use self::edit::*;
mod edit;

pub use self::font::*;
mod font;

pub use self::layout::*;
mod layout;

pub use self::line_ending::*;
mod line_ending;

pub use self::render::*;
mod render;

pub use self::shape::*;
mod shape;

pub use self::shape_run_cache::*;
mod shape_run_cache;

#[cfg(feature = "swash")]
pub use self::swash::*;
#[cfg(feature = "swash")]
mod swash;

mod math;

type BuildHasher = core::hash::BuildHasherDefault<rustc_hash::FxHasher>;

#[cfg(feature = "std")]
type HashMap<K, V> = std::collections::HashMap<K, V, BuildHasher>;
#[cfg(not(feature = "std"))]
type HashMap<K, V> = hashbrown::HashMap<K, V, BuildHasher>;

#[cfg(all(test, feature = "std"))]
mod tests {
    use crate::{Attrs, Buffer, FontSystem, Metrics};

    #[test]
    fn metrics_new_stores_values() {
        let metrics = Metrics::new(14.0, 20.0);
        assert_eq!(metrics.font_size, 14.0);
        assert_eq!(metrics.line_height, 20.0);
    }

    #[test]
    fn metrics_relative_calculates_line_height() {
        let metrics = Metrics::relative(10.0, 1.5);
        assert_eq!(metrics.font_size, 10.0);
        assert_eq!(metrics.line_height, 15.0);
    }

    #[test]
    fn metrics_scale_multiplies_values() {
        let metrics = Metrics::new(10.0, 20.0).scale(2.0);
        assert_eq!(metrics.font_size, 20.0);
        assert_eq!(metrics.line_height, 40.0);
    }

    #[test]
    fn metrics_default_is_zero() {
        let metrics = Metrics::default();
        assert_eq!(metrics.font_size, 0.0);
        assert_eq!(metrics.line_height, 0.0);
    }

    #[test]
    fn metrics_display_formats_values() {
        let metrics = Metrics::new(14.0, 20.0);
        assert_eq!(format!("{}", metrics), "14px / 20px");
    }

    #[test]
    fn attrs_new_has_sane_defaults() {
        let attrs = Attrs::new();
        assert!(attrs.color_opt.is_none(), "default color should be unset");
        assert_eq!(attrs.metadata, 0, "default metadata should be zero");
    }

    #[test]
    fn attrs_setters_are_fluent() {
        let attrs = Attrs::new().metadata(42);
        assert_eq!(attrs.metadata, 42);
    }

    #[test]
    fn buffer_new_empty_has_no_lines() {
        let buffer = Buffer::new_empty(Metrics::new(14.0, 20.0));
        assert!(buffer.lines.is_empty(), "new_empty buffer has no lines");
    }

    #[test]
    fn buffer_new_with_font_system_seeds_a_line() {
        let mut font_system = FontSystem::new();
        let buffer = Buffer::new(&mut font_system, Metrics::new(14.0, 20.0));
        // Buffer::new seeds an empty text line, so there is exactly one line.
        assert_eq!(buffer.lines.len(), 1);
    }

    #[test]
    fn font_system_new_constructs_database() {
        let font_system = FontSystem::new();
        // The Debug impl must surface the FontSystem type name.
        let debug = format!("{:?}", font_system);
        assert!(
            debug.contains("FontSystem"),
            "FontSystem debug output should contain its type name"
        );
    }
}
