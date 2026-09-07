//! Design tokens and Oklab uniform blending.
//!
//! This crate provides the Oklab perceptual color pipeline, semantic design
//! tokens, and GPU theme-transition uniforms for Martensite.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod gpu_transition;
pub mod oklab;
pub mod tokens;

pub use gpu_transition::{
    ThemeTransition, ThemeUniformBuffer, ThemeUniforms, THEME_TRANSITION_WGSL,
};
pub use oklab::{
    apca_contrast, gamut_map, linear_to_srgb, srgb_to_linear, wcag_contrast, Gamut, Oklab, Oklch,
};
pub use tokens::{Theme, ThemeDictionary, ThemeDiff, ThemeMode, ThemeToken, TokenKey};
