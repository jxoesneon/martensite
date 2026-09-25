//! Runtime Design-Lint Bridge.
//!
//! Feeds per-frame [`PaintList`]s into [`martensite_design_lint`] to evaluate
//! UI design standards at runtime. Caches lint reports by scene fingerprint so
//! evaluation is $O(\text{frame-change})$ rather than $O(\text{frame})$,
//! supplies HUD badge summaries, filters findings by widget tree path, and
//! serializes offline dumps for `cargo martensite lint --scene` or dev-channel
//! CLI attach requests.
//!
//! # Architecture
//!
//! ```text
//! PaintList (per frame)
//!    │
//!    ▼
//! LintBridge::on_frame
//!    │
//!    ├── Fast Fingerprint Check ──► (unchanged? return cached report)
//!    │
//!    ▼ (changed)
//! Replay to LintScene + lint() ──► update cached LintReport
//!    │
//!    ├── Inspector Lint panel (`findings_for`)
//!    ├── HUD badge (`badge_summary`)
//!    └── Offline dump (`dump_to_file` / MARTENSITE_LINT_DUMP)
//! ```

use std::path::{Path, PathBuf};

use kurbo::{Point, Rect};
use martensite_core::PaintList;
pub use martensite_design_lint::{
    Confidence, FillStat, Finding, FixSafety, LintConfig, LintConfigError, LintNode, LintReport,
    LintScene, NodeKind, Severity, Standard, TextStat,
};
use serde::{Deserialize, Serialize};

/// The runtime bridge connecting rendering paint streams to the design-lint engine.
///
/// # Examples
///
/// ```
/// use martensite_core::PaintList;
/// use martensite_design_lint::LintConfig;
/// use martensite_devtools::lint_bridge::LintBridge;
///
/// let mut bridge = LintBridge::new(LintConfig::new());
/// let list = PaintList::new();
/// let report = bridge.on_frame(&list);
/// assert!(report.is_some());
/// assert!(bridge.badge_summary().is_clean());
/// ```
#[derive(Debug)]
pub struct LintBridge {
    config: LintConfig,
    last_scene: Option<LintScene>,
    last_report: Option<LintReport>,
    last_fingerprint: Option<u64>,
    enabled: bool,
    relint_count: usize,
    skipped_count: usize,
    frames_evaluated: usize,
    dump_path: Option<PathBuf>,
}

impl Default for LintBridge {
    fn default() -> Self {
        Self::new(LintConfig::new())
    }
}

impl LintBridge {
    /// Create a new `LintBridge` with the specified configuration.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_design_lint::LintConfig;
    /// use martensite_devtools::lint_bridge::LintBridge;
    ///
    /// let bridge = LintBridge::new(LintConfig::new());
    /// assert!(bridge.is_enabled());
    /// assert_eq!(bridge.relint_count(), 0);
    /// ```
    pub fn new(config: LintConfig) -> Self {
        Self {
            config,
            last_scene: None,
            last_report: None,
            last_fingerprint: None,
            enabled: true,
            relint_count: 0,
            skipped_count: 0,
            frames_evaluated: 0,
            dump_path: None,
        }
    }

    /// Load configuration from a `design-lint.toml` file.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_devtools::lint_bridge::LintBridge;
    ///
    /// let bridge = LintBridge::from_config_file("design-lint.toml").unwrap();
    /// assert!(bridge.is_enabled());
    /// ```
    pub fn from_config_file(path: impl AsRef<Path>) -> Result<Self, LintConfigError> {
        let config = LintConfig::from_file(path.as_ref())?.unwrap_or_default();
        Ok(Self::new(config))
    }

    /// Set whether runtime linting is enabled.
    ///
    /// When disabled, [`on_frame`](Self::on_frame) immediately returns `None`
    /// without building a [`LintScene`] (zero-cost-off).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_design_lint::LintConfig;
    /// use martensite_devtools::lint_bridge::LintBridge;
    ///
    /// let mut bridge = LintBridge::new(LintConfig::new()).with_enabled(false);
    /// assert!(!bridge.is_enabled());
    /// ```
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Configure a target path for offline dumps on each frame change.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_design_lint::LintConfig;
    /// use martensite_devtools::lint_bridge::LintBridge;
    ///
    /// let bridge = LintBridge::new(LintConfig::new()).with_dump_path("lint_dump.bin");
    /// assert!(bridge.is_enabled());
    /// ```
    pub fn with_dump_path(mut self, path: impl AsRef<Path>) -> Self {
        self.dump_path = Some(path.as_ref().to_path_buf());
        self
    }

    /// Reload configuration from a `design-lint.toml` file, invalidating
    /// cached report fingerprints so the next frame re-evaluates.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_design_lint::LintConfig;
    /// use martensite_devtools::lint_bridge::LintBridge;
    ///
    /// let mut bridge = LintBridge::new(LintConfig::new());
    /// bridge.reload_config_from_file("design-lint.toml").unwrap();
    /// ```
    pub fn reload_config_from_file(
        &mut self,
        path: impl AsRef<Path>,
    ) -> Result<(), LintConfigError> {
        let config = LintConfig::from_file(path.as_ref())?.unwrap_or_default();
        self.set_config(config);
        Ok(())
    }

    /// Current lint configuration.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_design_lint::LintConfig;
    /// use martensite_devtools::lint_bridge::LintBridge;
    ///
    /// let bridge = LintBridge::new(LintConfig::new());
    /// assert_eq!(bridge.config().scale_factor, 1.0);
    /// ```
    pub fn config(&self) -> &LintConfig {
        &self.config
    }

    /// Mutable reference to current lint configuration.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_design_lint::LintConfig;
    /// use martensite_devtools::lint_bridge::LintBridge;
    ///
    /// let mut bridge = LintBridge::new(LintConfig::new());
    /// bridge.config_mut().scale_factor = 2.0;
    /// assert_eq!(bridge.config().scale_factor, 2.0);
    /// ```
    pub fn config_mut(&mut self) -> &mut LintConfig {
        self.last_fingerprint = None;
        &mut self.config
    }

    /// Replace configuration, invalidating the cached fingerprint so
    /// the next frame re-evaluates.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_design_lint::LintConfig;
    /// use martensite_devtools::lint_bridge::LintBridge;
    ///
    /// let mut bridge = LintBridge::new(LintConfig::new());
    /// bridge.set_config(LintConfig::new());
    /// ```
    pub fn set_config(&mut self, config: LintConfig) {
        self.config = config;
        self.last_fingerprint = None;
    }

    /// Whether runtime design linting is enabled.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_design_lint::LintConfig;
    /// use martensite_devtools::lint_bridge::LintBridge;
    ///
    /// let bridge = LintBridge::new(LintConfig::new());
    /// assert!(bridge.is_enabled());
    /// ```
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Enable or disable runtime design linting.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_design_lint::LintConfig;
    /// use martensite_devtools::lint_bridge::LintBridge;
    ///
    /// let mut bridge = LintBridge::new(LintConfig::new());
    /// bridge.set_enabled(false);
    /// assert!(!bridge.is_enabled());
    /// ```
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Number of frames that actually ran the lint evaluation engine.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_design_lint::LintConfig;
    /// use martensite_devtools::lint_bridge::LintBridge;
    ///
    /// let bridge = LintBridge::new(LintConfig::new());
    /// assert_eq!(bridge.relint_count(), 0);
    /// ```
    pub fn relint_count(&self) -> usize {
        self.relint_count
    }

    /// Number of frames where linting was skipped because the scene
    /// fingerprint was unchanged.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_design_lint::LintConfig;
    /// use martensite_devtools::lint_bridge::LintBridge;
    ///
    /// let bridge = LintBridge::new(LintConfig::new());
    /// assert_eq!(bridge.skipped_count(), 0);
    /// ```
    pub fn skipped_count(&self) -> usize {
        self.skipped_count
    }

    /// Total frames fed to [`on_frame`](Self::on_frame) while enabled.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_design_lint::LintConfig;
    /// use martensite_devtools::lint_bridge::LintBridge;
    ///
    /// let bridge = LintBridge::new(LintConfig::new());
    /// assert_eq!(bridge.frames_evaluated(), 0);
    /// ```
    pub fn frames_evaluated(&self) -> usize {
        self.frames_evaluated
    }

    /// Cached report from the most recent evaluated frame, if any.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_design_lint::LintConfig;
    /// use martensite_devtools::lint_bridge::LintBridge;
    ///
    /// let bridge = LintBridge::new(LintConfig::new());
    /// assert!(bridge.report().is_none());
    /// ```
    pub fn report(&self) -> Option<&LintReport> {
        self.last_report.as_ref()
    }

    /// Cached scene from the most recent evaluated frame, if any.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_design_lint::LintConfig;
    /// use martensite_devtools::lint_bridge::LintBridge;
    ///
    /// let bridge = LintBridge::new(LintConfig::new());
    /// assert!(bridge.scene().is_none());
    /// ```
    pub fn scene(&self) -> Option<&LintScene> {
        self.last_scene.as_ref()
    }

    /// Fingerprint of the most recent evaluated scene, if any.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_design_lint::LintConfig;
    /// use martensite_devtools::lint_bridge::LintBridge;
    ///
    /// let bridge = LintBridge::new(LintConfig::new());
    /// assert!(bridge.last_fingerprint().is_none());
    /// ```
    pub fn last_fingerprint(&self) -> Option<u64> {
        self.last_fingerprint
    }

    /// Feed the frame's paint list.
    ///
    /// When disabled, this is a zero-cost early return that does not construct
    /// a [`LintScene`].
    ///
    /// When enabled, replays the paint stream into a [`LintScene`]. If the scene's
    /// fingerprint matches the previous frame's, the cached report is reused
    /// ($O(\text{frame-change})$, not $O(\text{frame})$).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::PaintList;
    /// use martensite_design_lint::LintConfig;
    /// use martensite_devtools::lint_bridge::LintBridge;
    ///
    /// let mut bridge = LintBridge::new(LintConfig::new());
    /// let list = PaintList::new();
    /// let report1 = bridge.on_frame(&list);
    /// assert_eq!(bridge.relint_count(), 1);
    ///
    /// // Unchanged frame uses cached report.
    /// let report2 = bridge.on_frame(&list);
    /// assert_eq!(bridge.relint_count(), 1);
    /// assert_eq!(bridge.skipped_count(), 1);
    /// ```
    pub fn on_frame(&mut self, list: &PaintList) -> Option<&LintReport> {
        if !self.enabled {
            return None;
        }

        self.frames_evaluated += 1;
        let mut scene = LintScene::from_paint_list(list);
        scene.scale_factor = self.config.scale_factor;
        let fp = scene.fingerprint();

        if self.last_fingerprint == Some(fp) && self.last_report.is_some() {
            self.skipped_count += 1;
            return self.last_report.as_ref();
        }

        let report = martensite_design_lint::lint(&scene, &self.config);
        self.relint_count += 1;
        self.last_scene = Some(scene);
        self.last_report = Some(report);
        self.last_fingerprint = Some(fp);

        self.maybe_write_env_dump();

        self.last_report.as_ref()
    }

    fn maybe_write_env_dump(&self) {
        let dump_target = self
            .dump_path
            .as_ref()
            .cloned()
            .or_else(|| std::env::var_os("MARTENSITE_LINT_DUMP").map(PathBuf::from));

        if let Some(target) = dump_target {
            if let (Some(scene), Some(report)) = (&self.last_scene, &self.last_report) {
                let dump = LintDump::new(scene, report);
                let _ = dump.dump_to_file(&target);
            }
        }
    }

    /// Return findings anchored to `node_path` or any descendant within its subtree.
    ///
    /// For example, `"App/Panel"` will match findings anchored to `"App/Panel"`
    /// and `"App/Panel/Button"`, but not `"App/PanelGroup"`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::PaintList;
    /// use martensite_design_lint::LintConfig;
    /// use martensite_devtools::lint_bridge::LintBridge;
    ///
    /// let mut bridge = LintBridge::new(LintConfig::new());
    /// bridge.on_frame(&PaintList::new());
    /// assert!(bridge.findings_for("App").is_empty());
    /// ```
    pub fn findings_for(&self, node_path: &str) -> Vec<&Finding> {
        let Some(report) = &self.last_report else {
            return Vec::new();
        };

        let trimmed = node_path.trim_end_matches('/');
        let prefix = format!("{trimmed}/");

        report
            .findings
            .iter()
            .filter(|f| f.path == trimmed || f.path.starts_with(&prefix))
            .collect()
    }

    /// Return findings anchored strictly to `node_path` (exact path match only).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::PaintList;
    /// use martensite_design_lint::LintConfig;
    /// use martensite_devtools::lint_bridge::LintBridge;
    ///
    /// let mut bridge = LintBridge::new(LintConfig::new());
    /// bridge.on_frame(&PaintList::new());
    /// assert!(bridge.exact_findings_for("App").is_empty());
    /// ```
    pub fn exact_findings_for(&self, node_path: &str) -> Vec<&Finding> {
        let Some(report) = &self.last_report else {
            return Vec::new();
        };

        report
            .findings
            .iter()
            .filter(|f| f.path == node_path)
            .collect()
    }

    /// HUD badge summary helper returning categorized counts of active findings.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_design_lint::LintConfig;
    /// use martensite_devtools::lint_bridge::LintBridge;
    ///
    /// let bridge = LintBridge::new(LintConfig::new());
    /// let summary = bridge.badge_summary();
    /// assert_eq!(summary.total(), 0);
    /// assert_eq!(summary.badge_text(), "lint: clean");
    /// ```
    pub fn badge_summary(&self) -> LintBadgeSummary {
        let Some(report) = &self.last_report else {
            return LintBadgeSummary::default();
        };

        let mut errors = 0;
        let mut warnings = 0;
        let mut infos = 0;

        for f in &report.findings {
            match f.severity {
                Severity::Error | Severity::Forbid => errors += 1,
                Severity::Warn => warnings += 1,
                Severity::Info => infos += 1,
                Severity::Off => {}
            }
        }

        LintBadgeSummary {
            errors,
            warnings,
            infos,
        }
    }

    /// Serialize the current frame's scene and report to a JSON string.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::PaintList;
    /// use martensite_design_lint::LintConfig;
    /// use martensite_devtools::lint_bridge::LintBridge;
    ///
    /// let mut bridge = LintBridge::new(LintConfig::new());
    /// bridge.on_frame(&PaintList::new());
    /// let json = bridge.dump_json().unwrap();
    /// assert!(json.contains("\"version\""));
    /// ```
    pub fn dump_json(&self) -> Result<String, LintDumpError> {
        let (scene, report) = self.current_snapshot()?;
        LintDump::new(scene, report)
            .to_json()
            .map_err(LintDumpError::Json)
    }

    /// Serialize the current frame's scene and report to binary format.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::PaintList;
    /// use martensite_design_lint::LintConfig;
    /// use martensite_devtools::lint_bridge::LintBridge;
    ///
    /// let mut bridge = LintBridge::new(LintConfig::new());
    /// bridge.on_frame(&PaintList::new());
    /// let bin = bridge.dump_binary().unwrap();
    /// assert!(bin.starts_with(b"MLNT"));
    /// ```
    pub fn dump_binary(&self) -> Result<Vec<u8>, LintDumpError> {
        let (scene, report) = self.current_snapshot()?;
        LintDump::new(scene, report)
            .to_binary()
            .map_err(LintDumpError::Json)
    }

    /// Dump the current frame's scene and report to a file.
    ///
    /// Writes JSON if `path` ends with `.json`, or versioned binary format otherwise.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_core::PaintList;
    /// use martensite_design_lint::LintConfig;
    /// use martensite_devtools::lint_bridge::LintBridge;
    ///
    /// let mut bridge = LintBridge::new(LintConfig::new());
    /// bridge.on_frame(&PaintList::new());
    /// bridge.dump_to_file("out.bin").unwrap();
    /// ```
    pub fn dump_to_file(&self, path: impl AsRef<Path>) -> Result<(), LintDumpError> {
        let (scene, report) = self.current_snapshot()?;
        LintDump::new(scene, report).dump_to_file(path)
    }

    /// Load a dumped snapshot from disk and return reconstructed [`LintScene`] and [`LintReport`].
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_devtools::lint_bridge::LintBridge;
    ///
    /// let (scene, report) = LintBridge::load_dump_file("out.bin").unwrap();
    /// assert!(report.is_clean());
    /// ```
    pub fn load_dump_file(
        path: impl AsRef<Path>,
    ) -> Result<(LintScene, LintReport), LintDumpError> {
        let dump = LintDump::load_from_file(path)?;
        Ok((dump.to_scene(), dump.to_report()))
    }

    fn current_snapshot(&self) -> Result<(&LintScene, &LintReport), LintDumpError> {
        match (&self.last_scene, &self.last_report) {
            (Some(s), Some(r)) => Ok((s, r)),
            _ => Err(LintDumpError::NoFrameAvailable),
        }
    }
}

/// Finding counts categorized for the HUD badge display.
///
/// # Examples
///
/// ```
/// use martensite_devtools::lint_bridge::LintBadgeSummary;
///
/// let summary = LintBadgeSummary {
///     errors: 2,
///     warnings: 5,
///     infos: 1,
/// };
/// assert_eq!(summary.total(), 8);
/// assert_eq!(summary.badge_text(), "lint: 5w 2e");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct LintBadgeSummary {
    /// Number of error-level findings (including `Forbid`).
    pub errors: usize,
    /// Number of warning-level findings.
    pub warnings: usize,
    /// Number of info-level findings.
    pub infos: usize,
}

impl LintBadgeSummary {
    /// Total count of active findings across all severities.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::lint_bridge::LintBadgeSummary;
    ///
    /// let summary = LintBadgeSummary { errors: 1, warnings: 2, infos: 3 };
    /// assert_eq!(summary.total(), 6);
    /// ```
    pub fn total(&self) -> usize {
        self.errors + self.warnings + self.infos
    }

    /// Whether there are zero warnings or errors.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::lint_bridge::LintBadgeSummary;
    ///
    /// let clean = LintBadgeSummary { errors: 0, warnings: 0, infos: 2 };
    /// assert!(clean.is_clean());
    /// ```
    pub fn is_clean(&self) -> bool {
        self.errors == 0 && self.warnings == 0
    }

    /// Format as standard HUD badge text (e.g., `"lint: 12w 3e"`, `"lint: clean"`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::lint_bridge::LintBadgeSummary;
    ///
    /// assert_eq!(LintBadgeSummary::default().badge_text(), "lint: clean");
    /// assert_eq!(
    ///     LintBadgeSummary { errors: 1, warnings: 2, infos: 0 }.badge_text(),
    ///     "lint: 2w 1e"
    /// );
    /// ```
    pub fn badge_text(&self) -> String {
        if self.errors > 0 && self.warnings > 0 {
            format!("lint: {}w {}e", self.warnings, self.errors)
        } else if self.warnings > 0 {
            format!("lint: {}w", self.warnings)
        } else if self.errors > 0 {
            format!("lint: {}e", self.errors)
        } else if self.infos > 0 {
            format!("lint: {}i", self.infos)
        } else {
            "lint: clean".to_string()
        }
    }
}

/// Magic identifier prefixing binary dump snapshots.
pub const DUMP_MAGIC: &[u8; 4] = b"MLNT";

/// Current offline dump format version.
pub const DUMP_VERSION: u32 = 1;

/// A serializable offline snapshot containing a [`LintScene`] and [`LintReport`].
///
/// Can be serialized to binary or JSON for `MARTENSITE_LINT_DUMP` and CLI attach mode.
///
/// # Examples
///
/// ```
/// use martensite_design_lint::{LintConfig, LintScene};
/// use martensite_devtools::lint_bridge::LintDump;
///
/// let scene = LintScene::default();
/// let report = martensite_design_lint::lint(&scene, &LintConfig::new());
/// let dump = LintDump::new(&scene, &report);
///
/// let json = dump.to_json().unwrap();
/// let loaded = LintDump::from_json(&json).unwrap();
/// assert_eq!(dump.scene.roots.len(), loaded.scene.roots.len());
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LintDump {
    /// Schema format version.
    pub version: u32,
    /// Serialized lint scene.
    pub scene: SerializedLintScene,
    /// Serialized lint report.
    pub report: SerializedLintReport,
}

impl LintDump {
    /// Create a new dump snapshot from a live scene and report.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_design_lint::{LintConfig, LintScene};
    /// use martensite_devtools::lint_bridge::LintDump;
    ///
    /// let scene = LintScene::default();
    /// let report = martensite_design_lint::lint(&scene, &LintConfig::new());
    /// let dump = LintDump::new(&scene, &report);
    /// assert_eq!(dump.version, 1);
    /// ```
    pub fn new(scene: &LintScene, report: &LintReport) -> Self {
        Self {
            version: DUMP_VERSION,
            scene: SerializedLintScene::from(scene),
            report: SerializedLintReport::from(report),
        }
    }

    /// Reconstruct the [`LintScene`] from this dump.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_design_lint::{LintConfig, LintScene};
    /// use martensite_devtools::lint_bridge::LintDump;
    ///
    /// let scene = LintScene::default();
    /// let report = martensite_design_lint::lint(&scene, &LintConfig::new());
    /// let dump = LintDump::new(&scene, &report);
    /// let restored = dump.to_scene();
    /// assert_eq!(restored.scale_factor, scene.scale_factor);
    /// ```
    pub fn to_scene(&self) -> LintScene {
        self.scene.to_scene()
    }

    /// Reconstruct the [`LintReport`] from this dump.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_design_lint::{LintConfig, LintScene};
    /// use martensite_devtools::lint_bridge::LintDump;
    ///
    /// let scene = LintScene::default();
    /// let report = martensite_design_lint::lint(&scene, &LintConfig::new());
    /// let dump = LintDump::new(&scene, &report);
    /// let restored = dump.to_report();
    /// assert!(restored.is_clean());
    /// ```
    pub fn to_report(&self) -> LintReport {
        self.report.to_report()
    }

    /// Serialize this dump to a pretty-printed JSON string.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_design_lint::{LintConfig, LintScene};
    /// use martensite_devtools::lint_bridge::LintDump;
    ///
    /// let scene = LintScene::default();
    /// let report = martensite_design_lint::lint(&scene, &LintConfig::new());
    /// let dump = LintDump::new(&scene, &report);
    /// assert!(dump.to_json().is_ok());
    /// ```
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Deserialize a dump from a JSON string.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_design_lint::{LintConfig, LintScene};
    /// use martensite_devtools::lint_bridge::LintDump;
    ///
    /// let scene = LintScene::default();
    /// let report = martensite_design_lint::lint(&scene, &LintConfig::new());
    /// let json = LintDump::new(&scene, &report).to_json().unwrap();
    /// let loaded = LintDump::from_json(&json).unwrap();
    /// assert_eq!(loaded.version, 1);
    /// ```
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// Serialize this dump to binary format with magic header and version.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_design_lint::{LintConfig, LintScene};
    /// use martensite_devtools::lint_bridge::LintDump;
    ///
    /// let scene = LintScene::default();
    /// let report = martensite_design_lint::lint(&scene, &LintConfig::new());
    /// let bin = LintDump::new(&scene, &report).to_binary().unwrap();
    /// assert!(bin.starts_with(b"MLNT"));
    /// ```
    pub fn to_binary(&self) -> Result<Vec<u8>, serde_json::Error> {
        let payload = serde_json::to_vec(self)?;
        let mut bytes = Vec::with_capacity(12 + payload.len());
        bytes.extend_from_slice(DUMP_MAGIC);
        bytes.extend_from_slice(&self.version.to_le_bytes());
        bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&payload);
        Ok(bytes)
    }

    /// Deserialize a dump from binary format.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_design_lint::{LintConfig, LintScene};
    /// use martensite_devtools::lint_bridge::LintDump;
    ///
    /// let scene = LintScene::default();
    /// let report = martensite_design_lint::lint(&scene, &LintConfig::new());
    /// let dump = LintDump::new(&scene, &report);
    /// let bin = dump.to_binary().unwrap();
    /// let loaded = LintDump::from_binary(&bin).unwrap();
    /// assert_eq!(loaded.version, dump.version);
    /// ```
    pub fn from_binary(bytes: &[u8]) -> Result<Self, LintDumpError> {
        if bytes.len() < 12 {
            return Err(LintDumpError::CorruptedPayload);
        }
        if &bytes[0..4] != DUMP_MAGIC {
            return Err(LintDumpError::InvalidMagic);
        }
        let version = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
        if version != DUMP_VERSION {
            return Err(LintDumpError::UnsupportedVersion(version));
        }
        let length = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
        if bytes.len() < 12 + length {
            return Err(LintDumpError::CorruptedPayload);
        }
        let payload = &bytes[12..12 + length];
        let dump: LintDump = serde_json::from_slice(payload).map_err(LintDumpError::Json)?;
        Ok(dump)
    }

    /// Write dump to a file (JSON if path ends in `.json`, binary otherwise).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_design_lint::{LintConfig, LintScene};
    /// use martensite_devtools::lint_bridge::LintDump;
    ///
    /// let scene = LintScene::default();
    /// let report = martensite_design_lint::lint(&scene, &LintConfig::new());
    /// LintDump::new(&scene, &report).dump_to_file("out.bin").unwrap();
    /// ```
    pub fn dump_to_file(&self, path: impl AsRef<Path>) -> Result<(), LintDumpError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if path.extension().and_then(|s| s.to_str()) == Some("json") {
            let json = self.to_json().map_err(LintDumpError::Json)?;
            std::fs::write(path, json).map_err(LintDumpError::Io)?;
        } else {
            let binary = self.to_binary().map_err(LintDumpError::Json)?;
            std::fs::write(path, binary).map_err(LintDumpError::Io)?;
        }
        Ok(())
    }

    /// Load dump from a file (auto-detects binary magic header or JSON).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_devtools::lint_bridge::LintDump;
    ///
    /// let dump = LintDump::load_from_file("out.bin").unwrap();
    /// assert_eq!(dump.version, 1);
    /// ```
    pub fn load_from_file(path: impl AsRef<Path>) -> Result<Self, LintDumpError> {
        let bytes = std::fs::read(path).map_err(LintDumpError::Io)?;
        if bytes.starts_with(DUMP_MAGIC) {
            Self::from_binary(&bytes)
        } else {
            let s = std::str::from_utf8(&bytes).map_err(|_| LintDumpError::CorruptedPayload)?;
            Self::from_json(s).map_err(LintDumpError::Json)
        }
    }
}

/// Serializable representation of a [`LintScene`].
///
/// # Examples
///
/// ```
/// use martensite_design_lint::LintScene;
/// use martensite_devtools::lint_bridge::SerializedLintScene;
///
/// let scene = LintScene::default();
/// let serialized = SerializedLintScene::from(&scene);
/// let restored = serialized.to_scene();
/// assert_eq!(restored.scale_factor, scene.scale_factor);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SerializedLintScene {
    /// Root scopes of the scene.
    pub roots: Vec<SerializedLintNode>,
    /// Frame bounds `[x0, y0, x1, y1]` in device px if known.
    pub frame: Option<[f64; 4]>,
    /// Display scale factor.
    pub scale_factor: f32,
}

impl SerializedLintScene {
    /// Convert back to a runtime [`LintScene`].
    pub fn to_scene(&self) -> LintScene {
        LintScene {
            roots: self.roots.iter().map(|n| n.to_node()).collect(),
            frame: self.frame.map(|b| Rect::new(b[0], b[1], b[2], b[3])),
            scale_factor: self.scale_factor,
        }
    }
}

impl From<&LintScene> for SerializedLintScene {
    fn from(scene: &LintScene) -> Self {
        Self {
            roots: scene.roots.iter().map(SerializedLintNode::from).collect(),
            frame: scene.frame.map(|r| [r.x0, r.y0, r.x1, r.y1]),
            scale_factor: scene.scale_factor,
        }
    }
}

/// Serializable representation of a [`LintNode`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SerializedLintNode {
    /// Node short name.
    pub name: String,
    /// Full scope name / type path.
    pub full_name: String,
    /// Node hierarchy path (`"App/Header/Button"`).
    pub path: String,
    /// Bounds `[x0, y0, x1, y1]` in device px.
    pub bounds: [f64; 4],
    /// Optional arena widget id.
    pub widget_id: Option<u64>,
    /// Classified kind string ("Navigation", "Interactive", etc.).
    pub kind: String,
    /// Allow tags applicable to this node.
    pub allows: Vec<String>,
    /// Own inline allows.
    pub own_allows: Vec<String>,
    /// Display level if set (1..4).
    pub display_level: Option<u8>,
    /// Semantic markers.
    pub markers: Vec<String>,
    /// Distinct font sizes in device px.
    pub font_sizes: Vec<f32>,
    /// Distinct colors in RGBA.
    pub colors: Vec<[u8; 4]>,
    /// Text stats painted in this scope.
    pub texts: Vec<SerializedTextStat>,
    /// Fill stats painted in this scope.
    pub fills: Vec<SerializedFillStat>,
    /// Painted area in square px.
    pub painted_area: f64,
    /// Child nodes.
    pub children: Vec<SerializedLintNode>,
}

impl SerializedLintNode {
    /// Convert back to a [`LintNode`].
    pub fn to_node(&self) -> LintNode {
        LintNode {
            name: self.name.clone(),
            full_name: self.full_name.clone(),
            path: self.path.clone(),
            bounds: Rect::new(
                self.bounds[0],
                self.bounds[1],
                self.bounds[2],
                self.bounds[3],
            ),
            widget_id: self.widget_id,
            kind: match self.kind.as_str() {
                "Navigation" => NodeKind::Navigation,
                "Interactive" => NodeKind::Interactive,
                "Content" => NodeKind::Content,
                "Container" => NodeKind::Container,
                "Chrome" => NodeKind::Chrome,
                _ => NodeKind::Unknown,
            },
            allows: self.allows.clone(),
            own_allows: self.own_allows.clone(),
            display_level: self.display_level,
            markers: self.markers.clone(),
            font_sizes: self.font_sizes.clone(),
            colors: self.colors.clone(),
            texts: self
                .texts
                .iter()
                .map(|t| TextStat {
                    origin: Point::new(t.origin[0], t.origin[1]),
                    size: t.size,
                    color: t.color,
                    text: t.text.clone(),
                    width: t.width,
                })
                .collect(),
            fills: self
                .fills
                .iter()
                .map(|f| FillStat {
                    rect: Rect::new(f.rect[0], f.rect[1], f.rect[2], f.rect[3]),
                    color: f.color,
                    clip: f.clip.map(|c| Rect::new(c[0], c[1], c[2], c[3])),
                })
                .collect(),
            painted_area: self.painted_area,
            children: self.children.iter().map(|c| c.to_node()).collect(),
        }
    }
}

impl From<&LintNode> for SerializedLintNode {
    fn from(n: &LintNode) -> Self {
        Self {
            name: n.name.clone(),
            full_name: n.full_name.clone(),
            path: n.path.clone(),
            bounds: [n.bounds.x0, n.bounds.y0, n.bounds.x1, n.bounds.y1],
            widget_id: n.widget_id,
            kind: format!("{:?}", n.kind),
            allows: n.allows.clone(),
            own_allows: n.own_allows.clone(),
            display_level: n.display_level,
            markers: n.markers.clone(),
            font_sizes: n.font_sizes.clone(),
            colors: n.colors.clone(),
            texts: n
                .texts
                .iter()
                .map(|t| SerializedTextStat {
                    origin: [t.origin.x, t.origin.y],
                    size: t.size,
                    color: t.color,
                    text: t.text.clone(),
                    width: t.width,
                })
                .collect(),
            fills: n
                .fills
                .iter()
                .map(|f| SerializedFillStat {
                    rect: [f.rect.x0, f.rect.y0, f.rect.x1, f.rect.y1],
                    color: f.color,
                    clip: f.clip.map(|c| [c.x0, c.y0, c.x1, c.y1]),
                })
                .collect(),
            painted_area: n.painted_area,
            children: n.children.iter().map(SerializedLintNode::from).collect(),
        }
    }
}

/// Serializable representation of a [`TextStat`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SerializedTextStat {
    /// Baseline origin `[x, y]` in device px.
    pub origin: [f64; 2],
    /// Font size in device px.
    pub size: f32,
    /// Color `[r, g, b, a]`.
    pub color: [u8; 4],
    /// Text content.
    pub text: String,
    /// Advance width in px.
    pub width: Option<f64>,
}

/// Serializable representation of a [`FillStat`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SerializedFillStat {
    /// Fill rect `[x0, y0, x1, y1]`.
    pub rect: [f64; 4],
    /// Color `[r, g, b, a]`.
    pub color: [u8; 4],
    /// Clip rect `[x0, y0, x1, y1]` if active.
    pub clip: Option<[f64; 4]>,
}

/// Serializable representation of a [`LintReport`].
///
/// # Examples
///
/// ```
/// use martensite_design_lint::{LintConfig, LintScene};
/// use martensite_devtools::lint_bridge::SerializedLintReport;
///
/// let scene = LintScene::default();
/// let report = martensite_design_lint::lint(&scene, &LintConfig::new());
/// let serialized = SerializedLintReport::from(&report);
/// let restored = serialized.to_report();
/// assert!(restored.is_clean());
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SerializedLintReport {
    /// Active findings.
    pub findings: Vec<SerializedFinding>,
    /// Suppressed findings.
    pub suppressed: Vec<SerializedFinding>,
    /// Unused allows.
    pub unused_allows: Vec<String>,
    /// Used allows.
    pub used_allows: Vec<String>,
}

impl SerializedLintReport {
    /// Convert back to a [`LintReport`].
    pub fn to_report(&self) -> LintReport {
        LintReport {
            findings: self.findings.iter().map(|f| f.to_finding()).collect(),
            suppressed: self.suppressed.iter().map(|f| f.to_finding()).collect(),
            unused_allows: self.unused_allows.clone(),
            used_allows: self.used_allows.clone(),
        }
    }
}

impl From<&LintReport> for SerializedLintReport {
    fn from(report: &LintReport) -> Self {
        Self {
            findings: report
                .findings
                .iter()
                .map(SerializedFinding::from)
                .collect(),
            suppressed: report
                .suppressed
                .iter()
                .map(SerializedFinding::from)
                .collect(),
            unused_allows: report.unused_allows.clone(),
            used_allows: report.used_allows.clone(),
        }
    }
}

/// Serializable representation of a [`Finding`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SerializedFinding {
    /// Rule identifier.
    pub rule: String,
    /// Intrinsic severity.
    pub intrinsic: String,
    /// Effective severity.
    pub severity: String,
    /// Confidence tier.
    pub confidence: String,
    /// Standards cited.
    pub standards: Vec<String>,
    /// Citation text.
    pub citation: String,
    /// Scope path.
    pub path: String,
    /// Bounds `[x0, y0, x1, y1]` if applicable.
    pub bounds: Option<[f64; 4]>,
    /// Finding explanation message.
    pub message: String,
    /// Documentation reference path/URL.
    pub doc: String,
    /// Suppressing allow if suppressed.
    pub suppressed_by: Option<String>,
    /// Optional autofix summary.
    pub fix_summary: Option<String>,
}

impl SerializedFinding {
    /// Convert back to a runtime [`Finding`].
    pub fn to_finding(&self) -> Finding {
        let rule: &'static str = match self.rule.as_str() {
            "target-size" => "target-size",
            "target-spacing" => "target-spacing",
            "text-contrast" => "text-contrast",
            "text-min-size" => "text-min-size",
            "text-truncation" => "text-truncation",
            "reading-order" => "reading-order",
            "whitespace" => "whitespace",
            "color-only-info" => "color-only-info",
            "progressive-disclosure" => "progressive-disclosure",
            "choice-count" => "choice-count",
            "nav-depth" => "nav-depth",
            "token-drift" => "token-drift",
            "edge-density" => "edge-density",
            "overflow-clip" => "overflow-clip",
            "nontext-contrast" => "nontext-contrast",
            "icon-only" => "icon-only",
            "saturated-area" => "saturated-area",
            "reserved-hue" => "reserved-hue",
            "priority-mix" => "priority-mix",
            "flood-cap" => "flood-cap",
            "kpi-context" => "kpi-context",
            "level-purity" => "level-purity",
            "level-skip" => "level-skip",
            "packing-density" => "packing-density",
            "text-density" => "text-density",
            "simultaneous-channels" => "simultaneous-channels",
            "scroll-competition" => "scroll-competition",
            other => Box::leak(other.to_string().into_boxed_str()),
        };

        let citation: &'static str = Box::leak(self.citation.clone().into_boxed_str());

        Finding {
            rule,
            intrinsic: Severity::from_key(&self.intrinsic).unwrap_or(Severity::Warn),
            severity: Severity::from_key(&self.severity).unwrap_or(Severity::Warn),
            confidence: match self.confidence.as_str() {
                "Heuristic" => Confidence::Heuristic,
                _ => Confidence::Deterministic,
            },
            standards: &[],
            citation,
            path: self.path.clone(),
            bounds: self.bounds.map(|b| Rect::new(b[0], b[1], b[2], b[3])),
            message: self.message.clone(),
            doc: self.doc.clone(),
            suppressed_by: self.suppressed_by.clone(),
            fix: None,
        }
    }
}

impl From<&Finding> for SerializedFinding {
    fn from(f: &Finding) -> Self {
        Self {
            rule: f.rule.to_string(),
            intrinsic: f.intrinsic.config_key().to_string(),
            severity: f.severity.config_key().to_string(),
            confidence: format!("{:?}", f.confidence),
            standards: f.standards.iter().map(|s| format!("{s:?}")).collect(),
            citation: f.citation.to_string(),
            path: f.path.clone(),
            bounds: f.bounds.map(|b| [b.x0, b.y0, b.x1, b.y1]),
            message: f.message.clone(),
            doc: f.doc.clone(),
            suppressed_by: f.suppressed_by.clone(),
            fix_summary: f.fix.as_ref().map(|x| x.summary.clone()),
        }
    }
}

/// Error type for offline dump serialization and deserialization.
#[derive(Debug)]
pub enum LintDumpError {
    /// No frame has been evaluated yet to dump.
    NoFrameAvailable,
    /// I/O error reading or writing dump.
    Io(std::io::Error),
    /// JSON serialization or deserialization error.
    Json(serde_json::Error),
    /// Invalid magic bytes in binary header.
    InvalidMagic,
    /// Unsupported format version.
    UnsupportedVersion(u32),
    /// Payload was truncated or corrupted.
    CorruptedPayload,
}

impl std::fmt::Display for LintDumpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LintDumpError::NoFrameAvailable => write!(f, "no frame has been evaluated yet to dump"),
            LintDumpError::Io(e) => write!(f, "dump I/O error: {e}"),
            LintDumpError::Json(e) => write!(f, "dump serialization error: {e}"),
            LintDumpError::InvalidMagic => write!(f, "invalid dump magic header"),
            LintDumpError::UnsupportedVersion(v) => write!(f, "unsupported dump version: {v}"),
            LintDumpError::CorruptedPayload => write!(f, "corrupted dump payload"),
        }
    }
}

impl std::error::Error for LintDumpError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            LintDumpError::Io(e) => Some(e),
            LintDumpError::Json(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for LintDumpError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<serde_json::Error> for LintDumpError {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}
