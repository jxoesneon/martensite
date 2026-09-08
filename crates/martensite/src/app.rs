//! Application builder and software fallback configuration.
//!
//! The [`AppBuilder`] configures top-level Martensite application behavior,
//! including the CPU software fallback path described in the v0.2.0 milestone
//! specification. When `allow_software_fallback(true)` is set, the rendering
//! pipeline will transparently fall back to [`TinySkiaBackend`] when the GPU
//! device is lost or unavailable, presenting via the `softbuffer` crate to the window.
//!
//! The [`AppConfig`] produced by the builder should be converted to
//! [`martensite_wgpu::OrchestratorConfig`] and passed to
//! [`martensite_wgpu::RenderOrchestrator::new`] to control:
//! - Whether CPU fallback is permitted (`allow_software_fallback`)
//! - Whether to bypass the GPU entirely (`prefer_cpu`)
//! - How long to wait before activating fallback (`fallback_timeout`)
//!
//! # Examples
//!
//! ```
//! use martensite::app::App;
//!
//! let app = App::build()
//!     .allow_software_fallback(true)
//!     .build();
//! assert!(app.allow_software_fallback());
//! ```
//!
//! [`TinySkiaBackend`]: martensite_render::TinySkiaBackend
//! [`AppBuilder`]: crate::app::AppBuilder
//! [`AppConfig`]: crate::app::AppConfig

use std::time::Duration;

/// Default maximum time to wait for GPU recovery before switching to CPU fallback.
///
/// # Examples
///
/// ```
/// use martensite::app::DEFAULT_FALLBACK_TIMEOUT;
/// use std::time::Duration;
///
/// assert_eq!(DEFAULT_FALLBACK_TIMEOUT, Duration::from_millis(32));
/// ```
pub const DEFAULT_FALLBACK_TIMEOUT: Duration = Duration::from_millis(32);

/// Configuration for the Martensite application runtime.
///
/// Built via [`AppBuilder`] and consumed by the render pipeline to decide
/// whether software fallback is permitted and how long to wait before
/// activating it.
///
/// # Examples
///
/// ```
/// use martensite::app::{App, AppConfig};
///
/// let config = App::build().build();
/// assert!(!config.allow_software_fallback());
/// ```
#[derive(Debug, Clone)]
pub struct AppConfig {
    /// Whether CPU software fallback via TinySkia is allowed.
    allow_software_fallback: bool,
    /// Maximum time to wait for GPU recovery before switching to CPU fallback.
    fallback_timeout: Duration,
    /// Whether to prefer the CPU backend even when a GPU is available.
    prefer_cpu: bool,
}

impl AppConfig {
    /// Returns whether CPU software fallback is allowed.
    ///
    /// When `true`, the render pipeline will transition to
    /// [`TinySkiaBackend`] when the GPU device is lost or unavailable.
    ///
    /// [`TinySkiaBackend`]: martensite_render::TinySkiaBackend
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::app::App;
    ///
    /// let config = App::build().allow_software_fallback(true).build();
    /// assert!(config.allow_software_fallback());
    /// ```
    #[must_use]
    pub fn allow_software_fallback(&self) -> bool {
        self.allow_software_fallback
    }

    /// Returns the maximum time to wait for GPU recovery before switching
    /// to CPU fallback.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::app::{App, DEFAULT_FALLBACK_TIMEOUT};
    ///
    /// let config = App::build().build();
    /// assert_eq!(config.fallback_timeout(), DEFAULT_FALLBACK_TIMEOUT);
    /// ```
    #[must_use]
    pub fn fallback_timeout(&self) -> Duration {
        self.fallback_timeout
    }

    /// Returns whether the CPU backend is preferred even when a GPU is
    /// available. This is useful for headless CI and testing.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::app::App;
    ///
    /// let config = App::build().prefer_cpu(true).build();
    /// assert!(config.prefer_cpu());
    /// ```
    #[must_use]
    pub fn prefer_cpu(&self) -> bool {
        self.prefer_cpu
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            allow_software_fallback: false,
            fallback_timeout: DEFAULT_FALLBACK_TIMEOUT,
            prefer_cpu: false,
        }
    }
}

impl From<AppConfig> for martensite_wgpu::OrchestratorConfig {
    fn from(config: AppConfig) -> Self {
        Self {
            allow_software_fallback: config.allow_software_fallback,
            prefer_cpu: config.prefer_cpu,
        }
    }
}

/// Builder for [`AppConfig`].
///
/// Created via [`App::build()`].
///
/// # Examples
///
/// ```
/// use martensite::app::App;
///
/// let config = App::build()
///     .allow_software_fallback(true)
///     .fallback_timeout(std::time::Duration::from_millis(50))
///     .build();
///
/// assert!(config.allow_software_fallback());
/// assert_eq!(config.fallback_timeout(), std::time::Duration::from_millis(50));
/// ```
#[derive(Debug, Clone)]
pub struct AppBuilder {
    config: AppConfig,
}

impl AppBuilder {
    /// Enables or disables CPU software fallback via TinySkia.
    ///
    /// When enabled, the rendering pipeline will fall back to
    /// [`TinySkiaBackend`] when the GPU device is lost or unavailable,
    /// presenting via `softbuffer` to the window surface.
    ///
    /// [`TinySkiaBackend`]: martensite_render::TinySkiaBackend
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::app::App;
    ///
    /// let builder = App::build().allow_software_fallback(true);
    /// assert!(builder.build().allow_software_fallback());
    /// ```
    #[must_use]
    pub fn allow_software_fallback(mut self, allow: bool) -> Self {
        self.config.allow_software_fallback = allow;
        self
    }

    /// Sets the maximum time to wait for GPU recovery before switching
    /// to CPU fallback.
    ///
    /// Defaults to 32ms per the v0.2.0 milestone specification.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::app::App;
    /// use std::time::Duration;
    ///
    /// let builder = App::build().fallback_timeout(Duration::from_millis(100));
    /// assert_eq!(builder.build().fallback_timeout(), Duration::from_millis(100));
    /// ```
    #[must_use]
    pub fn fallback_timeout(mut self, timeout: Duration) -> Self {
        self.config.fallback_timeout = timeout;
        self
    }

    /// Forces the CPU backend to be used even when a GPU is available.
    ///
    /// This is useful for headless CI environments and testing.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::app::App;
    ///
    /// let builder = App::build().prefer_cpu(true);
    /// assert!(builder.build().prefer_cpu());
    /// ```
    #[must_use]
    pub fn prefer_cpu(mut self, prefer: bool) -> Self {
        self.config.prefer_cpu = prefer;
        self
    }

    /// Builds the final [`AppConfig`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::app::App;
    ///
    /// let config = App::build().build();
    /// assert!(!config.allow_software_fallback());
    /// ```
    #[must_use]
    pub fn build(self) -> AppConfig {
        self.config
    }
}

/// Top-level Martensite application entry point.
///
/// Use [`App::build()`] to create an [`AppBuilder`] for configuring the
/// application runtime.
///
/// # Examples
///
/// ```
/// use martensite::app::App;
///
/// let builder = App::build();
/// let config = builder.build();
/// assert!(!config.prefer_cpu());
/// ```
pub struct App;

impl App {
    /// Creates a new [`AppBuilder`] for configuring the application.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::app::App;
    ///
    /// let builder = App::build();
    /// ```
    #[must_use]
    pub fn build() -> AppBuilder {
        AppBuilder {
            config: AppConfig::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{App, AppConfig, DEFAULT_FALLBACK_TIMEOUT};
    use std::time::Duration;

    #[test]
    fn default_config_disallows_software_fallback() {
        let config = AppConfig::default();
        assert!(!config.allow_software_fallback());
    }

    #[test]
    fn default_config_has_32ms_timeout() {
        let config = AppConfig::default();
        assert_eq!(config.fallback_timeout(), DEFAULT_FALLBACK_TIMEOUT);
        assert_eq!(config.fallback_timeout(), Duration::from_millis(32));
    }

    #[test]
    fn default_config_does_not_prefer_cpu() {
        let config = AppConfig::default();
        assert!(!config.prefer_cpu());
    }

    #[test]
    fn build_returns_app_builder() {
        let _builder = App::build();
    }

    #[test]
    fn allow_software_fallback_enables_it() {
        let config = App::build().allow_software_fallback(true).build();
        assert!(config.allow_software_fallback());
    }

    #[test]
    fn allow_software_fallback_false_disables_it() {
        let config = App::build().allow_software_fallback(false).build();
        assert!(!config.allow_software_fallback());
    }

    #[test]
    fn fallback_timeout_sets_custom_duration() {
        let config = App::build()
            .fallback_timeout(Duration::from_millis(100))
            .build();
        assert_eq!(config.fallback_timeout(), Duration::from_millis(100));
    }

    #[test]
    fn prefer_cpu_enables_cpu_preference() {
        let config = App::build().prefer_cpu(true).build();
        assert!(config.prefer_cpu());
    }

    #[test]
    fn builder_chains_multiple_options() {
        let config = App::build()
            .allow_software_fallback(true)
            .fallback_timeout(Duration::from_millis(50))
            .prefer_cpu(true)
            .build();
        assert!(config.allow_software_fallback());
        assert_eq!(config.fallback_timeout(), Duration::from_millis(50));
        assert!(config.prefer_cpu());
    }

    #[test]
    fn builder_is_cloneable() {
        let builder = App::build().allow_software_fallback(true);
        let cloned = builder.clone();
        assert!(cloned.build().allow_software_fallback());
    }

    #[test]
    fn config_is_cloneable() {
        let config = App::build().allow_software_fallback(true).build();
        let cloned = config.clone();
        assert!(cloned.allow_software_fallback());
    }

    #[test]
    fn app_config_converts_to_orchestrator_config() {
        let config = App::build()
            .allow_software_fallback(true)
            .prefer_cpu(true)
            .build();
        let orchestrator_config: martensite_wgpu::OrchestratorConfig = config.into();
        assert!(orchestrator_config.allow_software_fallback);
        assert!(orchestrator_config.prefer_cpu);
    }

    #[test]
    fn default_app_config_converts_to_default_orchestrator_config() {
        let config = AppConfig::default();
        let orchestrator_config: martensite_wgpu::OrchestratorConfig = config.into();
        assert!(!orchestrator_config.allow_software_fallback);
        assert!(!orchestrator_config.prefer_cpu);
    }
}
