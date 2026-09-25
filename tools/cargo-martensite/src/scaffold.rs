//! Project scaffolding and embedded templates for `cargo-martensite`.
//!
//! Provides project template generation (`new`) and agent-native documentation
//! initialization (`init`) per the specification in `docs/dx/SCAFFOLDING.md`.
//!
//! All templates are embedded directly in the CLI binary without network or git
//! dependencies, ensuring zero-latency instantiation and complete immunity from
//! remote version skew.
//!
//! # Examples
//!
//! ```
//! use cargo_martensite::scaffold::{validate_project_name, TemplateKind};
//!
//! assert!(validate_project_name("my_app").is_ok());
//! assert_eq!(TemplateKind::App.as_str(), "app");
//! ```

use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static STAGING_COUNTER: AtomicU64 = AtomicU64::new(1);

/// The default framework version pinned by generated templates.
pub const DEFAULT_MARTENSITE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Supported starter template kinds.
///
/// # Examples
///
/// ```
/// use cargo_martensite::scaffold::TemplateKind;
///
/// let kind = TemplateKind::parse("dashboard").unwrap();
/// assert_eq!(kind, TemplateKind::Dashboard);
/// assert_eq!(kind.as_str(), "dashboard");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TemplateKind {
    /// Single window, `MartensiteApp` scaffold, reactive counter, DX files, and smoke test.
    #[default]
    App,
    /// Smallest compiling main without extra DX files.
    Bare,
    /// Contextual dashboard skeleton with dock layout, one zone, and HMI-tuned lint config.
    Dashboard,
}

impl TemplateKind {
    /// Parses a template name string into a [`TemplateKind`].
    ///
    /// # Examples
    ///
    /// ```
    /// use cargo_martensite::scaffold::TemplateKind;
    ///
    /// assert_eq!(TemplateKind::parse("app").unwrap(), TemplateKind::App);
    /// assert_eq!(TemplateKind::parse("bare").unwrap(), TemplateKind::Bare);
    /// assert_eq!(TemplateKind::parse("dashboard").unwrap(), TemplateKind::Dashboard);
    /// assert!(TemplateKind::parse("unknown").is_err());
    /// ```
    pub fn parse(s: &str) -> Result<Self, ScaffoldError> {
        match s.trim().to_ascii_lowercase().as_str() {
            "app" => Ok(TemplateKind::App),
            "bare" => Ok(TemplateKind::Bare),
            "dashboard" => Ok(TemplateKind::Dashboard),
            other => Err(ScaffoldError::UnknownTemplate(other.to_string())),
        }
    }

    /// Returns the canonical string representation of this template kind.
    ///
    /// # Examples
    ///
    /// ```
    /// use cargo_martensite::scaffold::TemplateKind;
    ///
    /// assert_eq!(TemplateKind::App.as_str(), "app");
    /// assert_eq!(TemplateKind::Bare.as_str(), "bare");
    /// assert_eq!(TemplateKind::Dashboard.as_str(), "dashboard");
    /// ```
    pub const fn as_str(&self) -> &'static str {
        match self {
            TemplateKind::App => "app",
            TemplateKind::Bare => "bare",
            TemplateKind::Dashboard => "dashboard",
        }
    }
}

impl fmt::Display for TemplateKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// An embedded template file with its relative path and content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TemplateFile {
    /// Relative file path within the project root.
    pub path: &'static str,
    /// Unrendered template text containing placeholders.
    pub content: &'static str,
}

// Embedded template contents: `app`
const APP_CARGO_TOML: &str = include_str!("../templates/app/Cargo.toml");
const APP_MAIN_RS: &str = include_str!("../templates/app/src/main.rs");
const APP_AGENTS_MD: &str = include_str!("../templates/app/AGENTS.md");
const APP_DESIGN_LINT_TOML: &str = include_str!("../templates/app/design-lint.toml");
const APP_MARTENSITE_TOML: &str = include_str!("../templates/app/martensite.toml");
const APP_LLMS_TXT: &str = include_str!("../templates/app/llms.txt");

// Embedded template contents: `bare`
const BARE_CARGO_TOML: &str = include_str!("../templates/bare/Cargo.toml");
const BARE_MAIN_RS: &str = include_str!("../templates/bare/src/main.rs");

// Embedded template contents: `dashboard`
const DASHBOARD_CARGO_TOML: &str = include_str!("../templates/dashboard/Cargo.toml");
const DASHBOARD_MAIN_RS: &str = include_str!("../templates/dashboard/src/main.rs");
const DASHBOARD_AGENTS_MD: &str = include_str!("../templates/dashboard/AGENTS.md");
const DASHBOARD_DESIGN_LINT_TOML: &str = include_str!("../templates/dashboard/design-lint.toml");
const DASHBOARD_MARTENSITE_TOML: &str = include_str!("../templates/dashboard/martensite.toml");
const DASHBOARD_LLMS_TXT: &str = include_str!("../templates/dashboard/llms.txt");

/// Static file definitions for the `app` template.
pub const APP_FILES: &[TemplateFile] = &[
    TemplateFile {
        path: "Cargo.toml",
        content: APP_CARGO_TOML,
    },
    TemplateFile {
        path: "src/main.rs",
        content: APP_MAIN_RS,
    },
    TemplateFile {
        path: "AGENTS.md",
        content: APP_AGENTS_MD,
    },
    TemplateFile {
        path: "design-lint.toml",
        content: APP_DESIGN_LINT_TOML,
    },
    TemplateFile {
        path: "martensite.toml",
        content: APP_MARTENSITE_TOML,
    },
    TemplateFile {
        path: "llms.txt",
        content: APP_LLMS_TXT,
    },
];

/// Static file definitions for the `bare` template.
pub const BARE_FILES: &[TemplateFile] = &[
    TemplateFile {
        path: "Cargo.toml",
        content: BARE_CARGO_TOML,
    },
    TemplateFile {
        path: "src/main.rs",
        content: BARE_MAIN_RS,
    },
];

/// Static file definitions for the `dashboard` template.
pub const DASHBOARD_FILES: &[TemplateFile] = &[
    TemplateFile {
        path: "Cargo.toml",
        content: DASHBOARD_CARGO_TOML,
    },
    TemplateFile {
        path: "src/main.rs",
        content: DASHBOARD_MAIN_RS,
    },
    TemplateFile {
        path: "AGENTS.md",
        content: DASHBOARD_AGENTS_MD,
    },
    TemplateFile {
        path: "design-lint.toml",
        content: DASHBOARD_DESIGN_LINT_TOML,
    },
    TemplateFile {
        path: "martensite.toml",
        content: DASHBOARD_MARTENSITE_TOML,
    },
    TemplateFile {
        path: "llms.txt",
        content: DASHBOARD_LLMS_TXT,
    },
];

/// Returns the embedded file manifest for a given template kind.
///
/// # Examples
///
/// ```
/// use cargo_martensite::scaffold::{get_template_files, TemplateKind};
///
/// let files = get_template_files(TemplateKind::Bare);
/// assert_eq!(files.len(), 2);
/// ```
pub fn get_template_files(kind: TemplateKind) -> &'static [TemplateFile] {
    match kind {
        TemplateKind::App => APP_FILES,
        TemplateKind::Bare => BARE_FILES,
        TemplateKind::Dashboard => DASHBOARD_FILES,
    }
}

/// Errors that can occur during project scaffolding or initialization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScaffoldError {
    /// The specified project name is not a valid crate name.
    InvalidProjectName {
        /// The invalid name.
        name: String,
        /// Reason for rejection.
        reason: String,
    },
    /// The target directory already exists and is not empty.
    TargetExistsAndNotEmpty(PathBuf),
    /// The target path exists but is not a directory.
    TargetNotDirectory(PathBuf),
    /// An unresolvable placeholder was encountered during template expansion.
    UnresolvedPlaceholder {
        /// The missing placeholder variable name.
        variable: String,
        /// The template file in which the error occurred.
        template_file: String,
    },
    /// An unknown template kind was requested.
    UnknownTemplate(String),
    /// An I/O error occurred during staging, writing, or renaming.
    IoError(String),
}

impl fmt::Display for ScaffoldError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ScaffoldError::InvalidProjectName { name, reason } => {
                write!(f, "invalid project name `{name}`: {reason}")
            }
            ScaffoldError::TargetExistsAndNotEmpty(path) => {
                write!(
                    f,
                    "target directory `{}` already exists and is not empty",
                    path.display()
                )
            }
            ScaffoldError::TargetNotDirectory(path) => {
                write!(f, "target path `{}` is not a directory", path.display())
            }
            ScaffoldError::UnresolvedPlaceholder {
                variable,
                template_file,
            } => {
                write!(
                    f,
                    "unresolved placeholder `{{{{{variable}}}}}` in template file `{template_file}`"
                )
            }
            ScaffoldError::UnknownTemplate(name) => {
                write!(
                    f,
                    "unknown template `{name}` (valid choices: `app`, `bare`, `dashboard`)"
                )
            }
            ScaffoldError::IoError(msg) => write!(f, "I/O error: {msg}"),
        }
    }
}

impl std::error::Error for ScaffoldError {}

impl From<std::io::Error> for ScaffoldError {
    fn from(err: std::io::Error) -> Self {
        ScaffoldError::IoError(err.to_string())
    }
}

/// Validates that a project name conforms to Cargo package naming rules.
///
/// A valid package name:
/// - Must not be empty.
/// - Must contain only ASCII letters, digits, underscores, and hyphens (`[a-z0-9_-]`).
/// - Must start with an ASCII letter or underscore (cannot start with a digit or hyphen).
/// - Must not be a Rust keyword or reserved identifier.
/// - Must not be a reserved OS device name (e.g. `con`, `aux`, `nul`) or Cargo reserved name (`test`).
///
/// # Examples
///
/// ```
/// use cargo_martensite::scaffold::validate_project_name;
///
/// assert!(validate_project_name("my_app").is_ok());
/// assert!(validate_project_name("app-2").is_ok());
/// assert!(validate_project_name("123app").is_err());
/// assert!(validate_project_name("fn").is_err());
/// ```
pub fn validate_project_name(name: &str) -> Result<(), ScaffoldError> {
    if name.is_empty() {
        return Err(ScaffoldError::InvalidProjectName {
            name: name.to_string(),
            reason: "project name cannot be empty".to_string(),
        });
    }

    let first_char = name.chars().next().unwrap();
    if !first_char.is_ascii_alphabetic() && first_char != '_' {
        return Err(ScaffoldError::InvalidProjectName {
            name: name.to_string(),
            reason: format!(
                "project name must start with an ASCII letter or underscore, not `{first_char}`"
            ),
        });
    }

    for c in name.chars() {
        if !c.is_ascii_alphanumeric() && c != '_' && c != '-' {
            return Err(ScaffoldError::InvalidProjectName {
                name: name.to_string(),
                reason: format!("invalid character `{c}` in project name (must be `[a-z0-9_-]`)"),
            });
        }
    }

    // Rust keywords and reserved words per reference manual
    const RUST_KEYWORDS: &[&str] = &[
        "as", "break", "const", "continue", "crate", "else", "enum", "extern", "false", "fn",
        "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref",
        "return", "self", "Self", "static", "struct", "super", "trait", "true", "type", "unsafe",
        "use", "where", "while", "async", "await", "dyn", "abstract", "become", "box", "do",
        "final", "macro", "override", "priv", "typeof", "unsized", "virtual", "yield", "try",
    ];

    let lower = name.to_ascii_lowercase();
    for &kw in RUST_KEYWORDS {
        if kw.eq_ignore_ascii_case(name) {
            return Err(ScaffoldError::InvalidProjectName {
                name: name.to_string(),
                reason: format!("`{name}` is a Rust keyword and cannot be used as a package name"),
            });
        }
    }

    // Cargo and Windows reserved identifiers
    const RESERVED_NAMES: &[&str] = &[
        "test", "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7",
        "com8", "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
    ];

    if RESERVED_NAMES.contains(&lower.as_str()) {
        return Err(ScaffoldError::InvalidProjectName {
            name: name.to_string(),
            reason: format!("`{name}` is a reserved name and cannot be used as a package name"),
        });
    }

    Ok(())
}

/// Strictly renders a template string by replacing all `{{placeholder}}` markers.
///
/// An unresolved placeholder is treated as a hard failure, preventing silent
/// misconfiguration.
///
/// # Examples
///
/// ```
/// use std::collections::HashMap;
/// use cargo_martensite::scaffold::render_template;
///
/// let mut ctx = HashMap::new();
/// ctx.insert("project_name", "alpha");
///
/// let rendered = render_template("name = \"{{project_name}}\"", &ctx, "test.toml").unwrap();
/// assert_eq!(rendered, "name = \"alpha\"");
/// ```
pub fn render_template(
    template_content: &str,
    context: &HashMap<&str, &str>,
    file_name: &str,
) -> Result<String, ScaffoldError> {
    let mut result = String::with_capacity(template_content.len());
    let mut remainder = template_content;

    while let Some(start_idx) = remainder.find("{{") {
        result.push_str(&remainder[..start_idx]);
        let after_start = &remainder[start_idx + 2..];

        let end_idx =
            after_start
                .find("}}")
                .ok_or_else(|| ScaffoldError::UnresolvedPlaceholder {
                    variable: "<unclosed placeholder>".to_string(),
                    template_file: file_name.to_string(),
                })?;

        let raw_var = &after_start[..end_idx];
        let var_name = raw_var.trim();

        if var_name.is_empty() {
            return Err(ScaffoldError::UnresolvedPlaceholder {
                variable: String::new(),
                template_file: file_name.to_string(),
            });
        }

        match context.get(var_name) {
            Some(&val) => result.push_str(val),
            None => {
                return Err(ScaffoldError::UnresolvedPlaceholder {
                    variable: var_name.to_string(),
                    template_file: file_name.to_string(),
                });
            }
        }

        remainder = &after_start[end_idx + 2..];
    }

    result.push_str(remainder);

    // Final safety check: no unparsed placeholder sequence remaining
    if result.contains("{{") {
        return Err(ScaffoldError::UnresolvedPlaceholder {
            variable: "<residual delimiter>".to_string(),
            template_file: file_name.to_string(),
        });
    }

    Ok(result)
}

/// Options configuring a `new` project scaffolding operation.
///
/// # Examples
///
/// ```
/// use cargo_martensite::scaffold::{ScaffoldOptions, TemplateKind};
///
/// let opts = ScaffoldOptions::new("my_app")
///     .with_template(TemplateKind::Dashboard);
/// assert_eq!(opts.project_name, "my_app");
/// assert_eq!(opts.template, TemplateKind::Dashboard);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScaffoldOptions {
    /// The crate name for the newly created project.
    pub project_name: String,
    /// The template to instantiate.
    pub template: TemplateKind,
    /// Explicit Martensite framework version (defaults to [`DEFAULT_MARTENSITE_VERSION`]).
    pub martensite_version: Option<String>,
    /// Optional target directory override (defaults to `./{project_name}`).
    pub target_dir: Option<PathBuf>,
}

impl ScaffoldOptions {
    /// Creates options with default settings for the given project name.
    ///
    /// # Examples
    ///
    /// ```
    /// use cargo_martensite::scaffold::{ScaffoldOptions, TemplateKind};
    ///
    /// let opts = ScaffoldOptions::new("demo");
    /// assert_eq!(opts.project_name, "demo");
    /// assert_eq!(opts.template, TemplateKind::App);
    /// ```
    pub fn new(project_name: impl Into<String>) -> Self {
        Self {
            project_name: project_name.into(),
            template: TemplateKind::App,
            martensite_version: None,
            target_dir: None,
        }
    }

    /// Sets the template kind.
    ///
    /// # Examples
    ///
    /// ```
    /// use cargo_martensite::scaffold::{ScaffoldOptions, TemplateKind};
    ///
    /// let opts = ScaffoldOptions::new("demo").with_template(TemplateKind::Bare);
    /// assert_eq!(opts.template, TemplateKind::Bare);
    /// ```
    pub fn with_template(mut self, template: TemplateKind) -> Self {
        self.template = template;
        self
    }

    /// Overrides the framework dependency version.
    ///
    /// # Examples
    ///
    /// ```
    /// use cargo_martensite::scaffold::ScaffoldOptions;
    ///
    /// let opts = ScaffoldOptions::new("demo").with_martensite_version("0.19.0");
    /// assert_eq!(opts.martensite_version.as_deref(), Some("0.19.0"));
    /// ```
    pub fn with_martensite_version(mut self, version: impl Into<String>) -> Self {
        self.martensite_version = Some(version.into());
        self
    }

    /// Sets an explicit target directory.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::path::PathBuf;
    /// use cargo_martensite::scaffold::ScaffoldOptions;
    ///
    /// let opts = ScaffoldOptions::new("demo").with_target_dir(PathBuf::from("/tmp/demo"));
    /// assert_eq!(opts.target_dir, Some(PathBuf::from("/tmp/demo")));
    /// ```
    pub fn with_target_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.target_dir = Some(dir.into());
        self
    }
}

/// RAII cleanup guard to guarantee atomic staging cleanup on error or panic.
struct StagingGuard<'a> {
    path: &'a Path,
    committed: bool,
}

impl<'a> Drop for StagingGuard<'a> {
    fn drop(&mut self) {
        if !self.committed && self.path.exists() {
            let _ = fs::remove_dir_all(self.path);
        }
    }
}

/// Scaffolds a new project using atomic staging.
///
/// Renders all template files into an isolated staging directory
/// (`.martensite-new-<pid>-<seq>`), then atomically renames the staged directory
/// to the target. If the target directory exists and is non-empty, the operation
/// is refused without performing any writes.
///
/// Returns the canonical path of the scaffolded project.
///
/// # Examples
///
/// ```no_run
/// use cargo_martensite::scaffold::{scaffold_project, ScaffoldOptions, TemplateKind};
///
/// let opts = ScaffoldOptions::new("my_new_app")
///     .with_template(TemplateKind::App);
/// let path = scaffold_project(&opts).unwrap();
/// assert!(path.exists());
/// ```
pub fn scaffold_project(options: &ScaffoldOptions) -> Result<PathBuf, ScaffoldError> {
    validate_project_name(&options.project_name)?;

    let target_dir = match &options.target_dir {
        Some(dir) => dir.clone(),
        None => PathBuf::from(&options.project_name),
    };

    // Check target directory state
    let target_existed_empty = if target_dir.exists() {
        if !target_dir.is_dir() {
            return Err(ScaffoldError::TargetNotDirectory(target_dir));
        }
        let mut entries = fs::read_dir(&target_dir)?;
        if entries.next().is_some() {
            return Err(ScaffoldError::TargetExistsAndNotEmpty(target_dir));
        }
        true
    } else {
        false
    };

    let parent_dir = target_dir
        .parent()
        .map(|p| {
            if p.as_os_str().is_empty() {
                Path::new(".")
            } else {
                p
            }
        })
        .unwrap_or_else(|| Path::new("."));

    if !parent_dir.exists() {
        fs::create_dir_all(parent_dir)?;
    }

    let pid = std::process::id();
    let seq = STAGING_COUNTER.fetch_add(1, Ordering::Relaxed);
    let staging_dir_name = format!(".martensite-new-{pid}-{seq}");
    let staging_dir = parent_dir.join(&staging_dir_name);

    if staging_dir.exists() {
        let _ = fs::remove_dir_all(&staging_dir);
    }
    fs::create_dir_all(&staging_dir)?;

    let mut guard = StagingGuard {
        path: &staging_dir,
        committed: false,
    };

    let version = options
        .martensite_version
        .as_deref()
        .unwrap_or(DEFAULT_MARTENSITE_VERSION);

    let ident = options.project_name.replace('-', "_");

    let mut context = HashMap::new();
    context.insert("project_name", options.project_name.as_str());
    context.insert("project_name_ident", ident.as_str());
    context.insert("martensite_version", version);

    let files = get_template_files(options.template);
    for file in files {
        let rendered = render_template(file.content, &context, file.path)?;
        let file_path = staging_dir.join(file.path);
        if let Some(parent) = file_path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)?;
            }
        }
        fs::write(&file_path, rendered)?;
    }

    // Atomic commit: rename staging directory to target
    if target_existed_empty {
        // Windows refuses rename over an existing empty directory
        fs::remove_dir(&target_dir)?;
    }

    fs::rename(&staging_dir, &target_dir)?;
    guard.committed = true;

    Ok(target_dir)
}

/// The result status of an individual file initialization in [`init_project`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileInitStatus {
    /// File was created freshly as it did not exist.
    Created(PathBuf),
    /// File already existed with identical content and was preserved.
    Unchanged(PathBuf),
    /// File already existed with customized content and was preserved without overwriting.
    DiffersNotOverwritten(PathBuf),
}

/// Options configuring an `init` operation on an existing project.
///
/// # Examples
///
/// ```
/// use cargo_martensite::scaffold::InitOptions;
///
/// let opts = InitOptions::new()
///     .with_agents_only(true);
/// assert!(opts.agents_only);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InitOptions {
    /// If true, only generate `AGENTS.md`.
    pub agents_only: bool,
    /// If true, only generate `design-lint.toml`.
    pub lint_only: bool,
    /// Template defaults to draw DX files from.
    pub template: TemplateKind,
}

impl InitOptions {
    /// Creates options with default settings.
    ///
    /// # Examples
    ///
    /// ```
    /// use cargo_martensite::scaffold::{InitOptions, TemplateKind};
    ///
    /// let opts = InitOptions::new();
    /// assert_eq!(opts.template, TemplateKind::App);
    /// assert!(!opts.agents_only);
    /// ```
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the template to draw DX conventions from.
    ///
    /// # Examples
    ///
    /// ```
    /// use cargo_martensite::scaffold::{InitOptions, TemplateKind};
    ///
    /// let opts = InitOptions::new().with_template(TemplateKind::Dashboard);
    /// assert_eq!(opts.template, TemplateKind::Dashboard);
    /// ```
    pub fn with_template(mut self, template: TemplateKind) -> Self {
        self.template = template;
        self
    }

    /// Limits initialization to `AGENTS.md`.
    ///
    /// # Examples
    ///
    /// ```
    /// use cargo_martensite::scaffold::InitOptions;
    ///
    /// let opts = InitOptions::new().with_agents_only(true);
    /// assert!(opts.agents_only);
    /// ```
    pub fn with_agents_only(mut self, agents_only: bool) -> Self {
        self.agents_only = agents_only;
        self
    }

    /// Limits initialization to `design-lint.toml`.
    ///
    /// # Examples
    ///
    /// ```
    /// use cargo_martensite::scaffold::InitOptions;
    ///
    /// let opts = InitOptions::new().with_lint_only(true);
    /// assert!(opts.lint_only);
    /// ```
    pub fn with_lint_only(mut self, lint_only: bool) -> Self {
        self.lint_only = lint_only;
        self
    }
}

/// Initializes agent-native DX context files into an existing project.
///
/// Generates missing `AGENTS.md`, `design-lint.toml`, `martensite.toml`, and
/// `llms.txt`. If an existing file differs from template output, it is preserved
/// without overwriting, fulfilling the non-clobbering acceptance contract.
///
/// # Examples
///
/// ```no_run
/// use std::path::Path;
/// use cargo_martensite::scaffold::{init_project, InitOptions};
///
/// let opts = InitOptions::new();
/// let results = init_project(Path::new("."), &opts).unwrap();
/// ```
pub fn init_project(
    target_dir: &Path,
    options: &InitOptions,
) -> Result<Vec<FileInitStatus>, ScaffoldError> {
    if !target_dir.exists() {
        return Err(ScaffoldError::IoError(format!(
            "target directory `{}` does not exist",
            target_dir.display()
        )));
    }
    if !target_dir.is_dir() {
        return Err(ScaffoldError::TargetNotDirectory(target_dir.to_path_buf()));
    }

    // Attempt to extract package name from Cargo.toml
    let project_name = resolve_package_name(target_dir);
    let version = DEFAULT_MARTENSITE_VERSION;

    let ident = project_name.replace('-', "_");

    let mut context = HashMap::new();
    context.insert("project_name", project_name.as_str());
    context.insert("project_name_ident", ident.as_str());
    context.insert("martensite_version", version);

    // Determine target files based on flags
    let target_filenames: Vec<&'static str> = if options.agents_only {
        vec!["AGENTS.md"]
    } else if options.lint_only {
        vec!["design-lint.toml"]
    } else {
        vec![
            "AGENTS.md",
            "design-lint.toml",
            "martensite.toml",
            "llms.txt",
        ]
    };

    let template_files = get_template_files(options.template);
    let mut statuses = Vec::new();

    for filename in target_filenames {
        let template_file = template_files.iter().find(|f| f.path == filename);
        let Some(file) = template_file else {
            continue;
        };

        let rendered = render_template(file.content, &context, file.path)?;
        let file_path = target_dir.join(filename);

        if !file_path.exists() {
            fs::write(&file_path, rendered)?;
            statuses.push(FileInitStatus::Created(file_path));
        } else {
            let existing = fs::read_to_string(&file_path)?;
            if existing == rendered {
                statuses.push(FileInitStatus::Unchanged(file_path));
            } else {
                // Non-clobbering: preserve user edits
                statuses.push(FileInitStatus::DiffersNotOverwritten(file_path));
            }
        }
    }

    Ok(statuses)
}

/// Attempts to parse the package name from a project's `Cargo.toml`.
fn resolve_package_name(dir: &Path) -> String {
    let cargo_toml = dir.join("Cargo.toml");
    if let Ok(content) = fs::read_to_string(cargo_toml) {
        let mut in_package = false;
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('[') && trimmed.ends_with(']') {
                in_package = trimmed == "[package]";
                continue;
            }
            if in_package {
                if let Some(rest) = trimmed.strip_prefix("name") {
                    let rest = rest.trim();
                    if let Some(value) = rest.strip_prefix('=') {
                        let name = value.trim().trim_matches('"').trim_matches('\'');
                        if !name.is_empty() {
                            return name.to_string();
                        }
                    }
                }
            }
        }
    }

    // Fall back to directory name
    dir.canonicalize()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
        .unwrap_or_else(|| "martensite_app".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_project_names() {
        assert!(validate_project_name("my_app").is_ok());
        assert!(validate_project_name("my-app").is_ok());
        assert!(validate_project_name("app123").is_ok());
        assert!(validate_project_name("foo_bar-baz").is_ok());
        assert!(validate_project_name("_private_app").is_ok());
    }

    #[test]
    fn test_invalid_project_names() {
        assert!(validate_project_name("").is_err());
        assert!(validate_project_name("123app").is_err());
        assert!(validate_project_name("-my-app").is_err());
        assert!(validate_project_name("my app").is_err());
        assert!(validate_project_name("app@world").is_err());
        assert!(validate_project_name("app/sub").is_err());
        // Keywords
        assert!(validate_project_name("fn").is_err());
        assert!(validate_project_name("struct").is_err());
        assert!(validate_project_name("match").is_err());
        assert!(validate_project_name("self").is_err());
        assert!(validate_project_name("crate").is_err());
        // Reserved
        assert!(validate_project_name("test").is_err());
        assert!(validate_project_name("nul").is_err());
        assert!(validate_project_name("con").is_err());
    }

    #[test]
    fn test_strict_placeholder_resolution() {
        let mut ctx = HashMap::new();
        ctx.insert("project_name", "alpha");
        ctx.insert("martensite_version", "0.19.0");

        let text = "name = \"{{project_name}}\"\nversion = \"{{martensite_version}}\"";
        let res = render_template(text, &ctx, "test.toml").unwrap();
        assert_eq!(res, "name = \"alpha\"\nversion = \"0.19.0\"");

        // Missing placeholder
        let bad_text = "name = \"{{project_name}}\"\nunknown = \"{{missing_var}}\"";
        let err = render_template(bad_text, &ctx, "bad.toml").unwrap_err();
        assert!(matches!(
            err,
            ScaffoldError::UnresolvedPlaceholder { ref variable, .. } if variable == "missing_var"
        ));
    }

    #[test]
    fn test_template_kind_parsing() {
        assert_eq!(TemplateKind::parse("app").unwrap(), TemplateKind::App);
        assert_eq!(TemplateKind::parse("App").unwrap(), TemplateKind::App);
        assert_eq!(TemplateKind::parse("bare").unwrap(), TemplateKind::Bare);
        assert_eq!(
            TemplateKind::parse("dashboard").unwrap(),
            TemplateKind::Dashboard
        );
        assert!(TemplateKind::parse("bogus").is_err());
    }
}
