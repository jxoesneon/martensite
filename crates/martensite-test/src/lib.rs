//! Headless CI mock testing harness.
//!
//! `martensite-test` provides deterministic testing infrastructure for
//! applications built with the Martensite GUI framework. It replaces OS
//! monotonic clocks with a manually-advancing [`VirtualClock`] and compares
//! rendered frames against golden reference images using a perceptual DSSIM
//! metric, enabling pixel-perfect snapshot tests that are 100% reproducible
//! across CI runners.
//!
//! The crate is organized into three modules:
//!
//! - [`virtual_clock`]: a deterministic, fixed-step clock.
//! - [`mod@dssim`]: a self-contained perceptual image difference metric and
//!   grayscale [`ImageBuffer`].
//! - [`harness`]: the [`HeadlessHarness`] that ties the clock and the
//!   snapshot diffing together, plus [`GoldenImages`] for persisting
//!   reference images.
//!
//! # Determinism
//!
//! Because time only advances through explicit calls to
//! [`VirtualClock::advance`] (or the `step_*fps` helpers), the same test
//! sequence always produces the same elapsed time and the same rendered
//! output. This is the foundation of the v0.9.0 deterministic CI test gate:
//! 100 consecutive runs of the headless test suite yield 100% identical pass
//! results with zero timing jitter.
//!
//! # Examples
//!
//! Driving a render loop deterministically and comparing against a golden
//! image:
//!
//! ```
//! use martensite_test::dssim::ImageBuffer;
//! use martensite_test::HeadlessHarness;
//!
//! let mut harness = HeadlessHarness::new(8, 8);
//! // Render three identical black frames.
//! harness.run_frames(3, |_| vec![0u8; 8 * 8 * 4]);
//! assert_eq!(harness.frame_count(), 3);
//!
//! // The last captured frame should match a black golden image.
//! let golden = ImageBuffer::new(8, 8);
//! assert!(harness.compare_to_golden(&golden, 0.0));
//! ```
#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod dssim;
pub mod fuzz;
pub mod harness;
pub mod virtual_clock;

pub use dssim::{dssim, images_match, ImageBuffer};
pub use fuzz::{run_fuzz_campaign, FuzzConfig, FuzzEngine, FuzzError, FuzzReport, FuzzTarget};
pub use harness::{GoldenError, GoldenImages, HeadlessHarness};
pub use virtual_clock::{VirtualClock, FRAME_120FPS, FRAME_30FPS, FRAME_60FPS};
