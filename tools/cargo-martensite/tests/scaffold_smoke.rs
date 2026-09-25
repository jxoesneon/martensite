//! End-to-end first-run funnel integration tests (`scaffold_smoke`).
//!
//! Validates the full developer onboarding lifecycle:
//! 1. Scaffolds new projects from embedded templates (`Bare`, `App`, `Dashboard`).
//! 2. Asserts existence, non-emptiness, and validity of all generated files.
//! 3. Validates `design-lint.toml` parsing via `martensite_design_lint::config::LintConfig::from_toml`.
//! 4. Validates `martensite.toml` syntax and required section hierarchy.
//! 5. Validates `AGENTS.md` and `llms.txt` markdown formatting and placeholder resolution (`{{...}}`).
//! 6. Runs `cargo martensite doctor` diagnostics on scaffolded projects and asserts core checks succeed.
//! 7. Verifies `init_project` idempotency (repeated invocations do not clobber pristine or edited files).
//! 8. Enforces a strict 60-second SLA execution budget across the entire funnel.

use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use cargo_martensite::doctor::{run_doctor, CheckStatus, DoctorOptions};
use cargo_martensite::scaffold::{
    get_template_files, init_project, scaffold_project, FileInitStatus, InitOptions,
    ScaffoldOptions, TemplateKind,
};
use martensite_design_lint::config::LintConfig;
use tempfile::TempDir;

/// Maximum allowable execution time for the complete first-run funnel smoke test.
const SLA_BUDGET: Duration = Duration::from_secs(60);

/// Expected agent DX files generated for App and Dashboard templates.
const EXPECTED_DX_FILES: &[&str] = &[
    "Cargo.toml",
    "src/main.rs",
    "AGENTS.md",
    "design-lint.toml",
    "martensite.toml",
    "llms.txt",
];

/// Helper asserting that all embedded template files exist on disk and are non-empty.
fn assert_generated_files_exist_and_non_empty(project_dir: &Path, template: TemplateKind) {
    let embedded_files = get_template_files(template);
    for tf in embedded_files {
        let path = project_dir.join(tf.path);
        assert!(
            path.is_file(),
            "Expected embedded template file `{}` to exist on disk at `{}`",
            tf.path,
            path.display()
        );
        let metadata = fs::metadata(&path).unwrap_or_else(|err| {
            panic!("Failed to read metadata for `{}`: {err}", path.display())
        });
        assert!(
            metadata.len() > 0,
            "Template file `{}` in {:?} scaffold must not be empty",
            tf.path,
            template
        );
    }

    match template {
        TemplateKind::App | TemplateKind::Dashboard => {
            for &filename in EXPECTED_DX_FILES {
                let path = project_dir.join(filename);
                assert!(
                    path.is_file(),
                    "Expected DX file `{filename}` to exist in scaffolded {template} project",
                );
                let metadata = fs::metadata(&path).unwrap_or_else(|err| {
                    panic!("Failed to read metadata for `{filename}`: {err}")
                });
                assert!(
                    metadata.len() > 0,
                    "DX file `{filename}` in {template} scaffold must not be empty"
                );
            }
        }
        TemplateKind::Bare => {
            for &filename in &["Cargo.toml", "src/main.rs"] {
                let path = project_dir.join(filename);
                assert!(
                    path.is_file(),
                    "Expected `{filename}` to exist in Bare scaffold",
                );
                assert!(
                    fs::metadata(&path).unwrap().len() > 0,
                    "`{filename}` in Bare scaffold must not be empty"
                );
            }
            // Bare template intentionally excludes extra DX files until init is executed
            for &filename in &[
                "AGENTS.md",
                "design-lint.toml",
                "martensite.toml",
                "llms.txt",
            ] {
                assert!(
                    !project_dir.join(filename).exists(),
                    "Bare template must not generate DX file `{filename}`"
                );
            }
        }
    }
}

/// Helper validating `design-lint.toml` syntax and standards configuration.
fn validate_design_lint_toml(lint_path: &Path, template: TemplateKind) {
    let content = fs::read_to_string(lint_path)
        .unwrap_or_else(|err| panic!("Failed to read `{}`: {err}", lint_path.display()));

    let config = LintConfig::from_toml(&content).unwrap_or_else(|err| {
        panic!("`design-lint.toml` for {template} failed to parse via LintConfig: {err}")
    });

    assert!(
        !config.standards.is_empty(),
        "`design-lint.toml` for {template} must enable at least one design standard"
    );
}

/// Helper validating `martensite.toml` syntax and section structure.
fn validate_martensite_toml(config_path: &Path, template: TemplateKind) {
    let content = fs::read_to_string(config_path)
        .unwrap_or_else(|err| panic!("Failed to read `{}`: {err}", config_path.display()));

    let table: toml::Table = toml::from_str(&content).unwrap_or_else(|err| {
        panic!("`martensite.toml` for {template} failed to parse as valid TOML: {err}")
    });

    assert!(
        table.contains_key("dev"),
        "`martensite.toml` for {template} missing [dev] section"
    );
    assert!(
        table.contains_key("lint"),
        "`martensite.toml` for {template} missing [lint] section"
    );
    assert!(
        table.contains_key("inspector"),
        "`martensite.toml` for {template} missing [inspector] section"
    );

    let dev_table = table.get("dev").and_then(toml::Value::as_table);
    assert!(
        dev_table.is_some() && dev_table.unwrap().contains_key("port"),
        "`martensite.toml` [dev] section must define `port`"
    );
}

/// Helper validating markdown files for non-emptiness, valid markdown headers, and zero unrendered placeholders.
fn validate_markdown_file(path: &Path, file_name: &str) {
    let content = fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("Failed to read `{}`: {err}", path.display()));

    assert!(
        !content.trim().is_empty(),
        "Markdown file `{file_name}` must not be empty"
    );

    assert!(
        !content.contains("{{") && !content.contains("}}"),
        "Markdown file `{file_name}` contains unrendered placeholder markers: {content}"
    );

    assert!(
        content.lines().any(|l| l.trim_start().starts_with('#')),
        "Markdown file `{file_name}` must contain markdown headers"
    );
}

/// Helper verifying that core `run_doctor` diagnostics succeed for a scaffolded project.
fn assert_doctor_core_checks_succeed(project_dir: &Path, template: TemplateKind) {
    let options = DoctorOptions {
        fix: false,
        path: Some(project_dir.to_path_buf()),
    };

    let report = run_doctor(&options);

    // 1. Toolchain rustc check
    let rustc_result = report
        .results
        .iter()
        .find(|r| r.name == "toolchain/rustc")
        .expect("doctor report must include `toolchain/rustc` check");
    assert!(
        rustc_result.status.is_ok(),
        "toolchain/rustc check failed: {}",
        rustc_result.details
    );

    // 2. GPU adapter capabilities or graceful CPU fallback
    let gpu_result = report
        .results
        .iter()
        .find(|r| r.name == "gpu/adapter")
        .expect("doctor report must include `gpu/adapter` check");
    assert!(
        gpu_result.status.is_ok(),
        "gpu/adapter check failed: {}",
        gpu_result.details
    );

    // 3. Text & font subsystem reachability
    let fonts_result = report
        .results
        .iter()
        .find(|r| r.name == "text/fonts")
        .expect("doctor report must include `text/fonts` check");
    assert!(
        fonts_result.status.is_ok(),
        "text/fonts check failed: {}",
        fonts_result.details
    );

    // 4. Platform accessibility reachability (Pass or non-fatal Warning)
    let access_result = report
        .results
        .iter()
        .find(|r| r.name == "accessibility")
        .expect("doctor report must include `accessibility` check");
    assert!(
        access_result.status.is_ok() || access_result.status == CheckStatus::Warning,
        "accessibility check failed fatally: {}",
        access_result.details
    );

    // 5. Version parity between CLI and scaffolded project dependency
    let parity_result = report
        .results
        .iter()
        .find(|r| r.name == "version/parity")
        .expect("doctor report must include `version/parity` check");
    assert!(
        parity_result.status.is_ok(),
        "version/parity check failed: {}",
        parity_result.details
    );

    // 6. For App and Dashboard templates, design-lint.toml is present and must pass cleanly
    if template != TemplateKind::Bare {
        let lint_result = report
            .results
            .iter()
            .find(|r| r.name == "design-lint/config")
            .expect("doctor report must include `design-lint/config` check");
        assert!(
            lint_result.status.is_ok(),
            "design-lint/config check failed: {}",
            lint_result.details
        );
        assert!(
            report.is_success(),
            "Doctor report reported failures for {template}: {:?}",
            report.failures()
        );
    }
}

/// Helper testing `init_project` idempotency and non-clobbering preservation.
fn assert_init_idempotency_and_non_clobber(project_dir: &Path, template: TemplateKind) {
    match template {
        TemplateKind::App | TemplateKind::Dashboard => {
            let opts = InitOptions::new().with_template(template);

            // First run on pristine scaffolded project: all files are unchanged
            let statuses_1 = init_project(project_dir, &opts)
                .unwrap_or_else(|err| panic!("init_project failed on {template}: {err}"));
            assert!(
                !statuses_1.is_empty(),
                "init_project should return statuses for {template}"
            );
            for s in &statuses_1 {
                assert!(
                    matches!(s, FileInitStatus::Unchanged(_)),
                    "Expected Unchanged status for pristine scaffolded file, got {s:?}"
                );
            }

            // Second run: completely idempotent
            let statuses_2 = init_project(project_dir, &opts)
                .unwrap_or_else(|err| panic!("second init_project failed on {template}: {err}"));
            assert_eq!(
                statuses_1, statuses_2,
                "Repeated init_project must return identical statuses"
            );

            // Verify non-clobbering: edit AGENTS.md with custom user rules
            let agents_path = project_dir.join("AGENTS.md");
            let custom_content = "# Custom Team Rules\nDo not overwrite my manual guidelines.\n";
            fs::write(&agents_path, custom_content).unwrap();

            let statuses_3 = init_project(project_dir, &opts)
                .unwrap_or_else(|err| panic!("third init_project failed on {template}: {err}"));
            let agents_status = statuses_3.iter().find(|s| match s {
                FileInitStatus::DiffersNotOverwritten(p) => p == &agents_path,
                _ => false,
            });
            assert!(
                agents_status.is_some(),
                "Customized AGENTS.md must report DiffersNotOverwritten and never be clobbered"
            );
            assert_eq!(
                fs::read_to_string(&agents_path).unwrap(),
                custom_content,
                "Customized AGENTS.md contents must be preserved byte-for-byte"
            );
        }
        TemplateKind::Bare => {
            // 1. Bare template init with Bare kind produces 0 changes
            let bare_opts = InitOptions::new().with_template(TemplateKind::Bare);
            let statuses_bare = init_project(project_dir, &bare_opts)
                .unwrap_or_else(|err| panic!("init_project with Bare options failed: {err}"));
            assert!(statuses_bare.is_empty());

            // 2. Initializing DX files into Bare project creates missing files
            let app_opts = InitOptions::new();
            let statuses_create = init_project(project_dir, &app_opts)
                .unwrap_or_else(|err| panic!("init_project into Bare project failed: {err}"));
            assert!(
                statuses_create
                    .iter()
                    .all(|s| matches!(s, FileInitStatus::Created(_))),
                "Initial init into Bare project should report Created for all DX files"
            );

            // 3. Second run must be idempotent (all Unchanged)
            let statuses_idempotent = init_project(project_dir, &app_opts)
                .unwrap_or_else(|err| panic!("second init_project failed: {err}"));
            assert!(
                statuses_idempotent
                    .iter()
                    .all(|s| matches!(s, FileInitStatus::Unchanged(_))),
                "Second init into Bare project must report Unchanged for all DX files"
            );

            // 4. Non-clobbering preservation test
            let agents_path = project_dir.join("AGENTS.md");
            let custom_content = "# Bare Project Custom Rules\nPreserved.\n";
            fs::write(&agents_path, custom_content).unwrap();

            let statuses_custom = init_project(project_dir, &app_opts).unwrap();
            assert!(statuses_custom.iter().any(|s| match s {
                FileInitStatus::DiffersNotOverwritten(p) => p == &agents_path,
                _ => false,
            }));
            assert_eq!(fs::read_to_string(&agents_path).unwrap(), custom_content);
        }
    }
}

/// Executes the full first-run funnel verification suite for a given template.
fn verify_template_funnel(template: TemplateKind) {
    let funnel_start = Instant::now();
    let temp = TempDir::new().expect("Failed to create temporary directory for smoke test");
    let project_name = format!("{}_smoke_app", template.as_str());
    let target = temp.path().join(&project_name);

    // 1. Scaffold project into temporary directory
    let opts = ScaffoldOptions::new(&project_name)
        .with_template(template)
        .with_target_dir(&target);

    let scaffolded_path = scaffold_project(&opts)
        .unwrap_or_else(|err| panic!("Failed to scaffold template {template}: {err}"));
    assert_eq!(scaffolded_path, target);

    // 2. Verify all generated files exist and are non-empty
    assert_generated_files_exist_and_non_empty(&target, template);

    // 3. Validate design-lint.toml parses correctly via LintConfig::from_toml
    let lint_toml_path = target.join("design-lint.toml");
    if lint_toml_path.exists() {
        validate_design_lint_toml(&lint_toml_path, template);
    }

    // 4. Validate martensite.toml parses correctly
    let martensite_toml_path = target.join("martensite.toml");
    if martensite_toml_path.exists() {
        validate_martensite_toml(&martensite_toml_path, template);
    }

    // 5. Validate AGENTS.md and llms.txt contain valid markdown and no unrendered placeholders
    let agents_path = target.join("AGENTS.md");
    if agents_path.exists() {
        validate_markdown_file(&agents_path, "AGENTS.md");
    }

    let llms_path = target.join("llms.txt");
    if llms_path.exists() {
        validate_markdown_file(&llms_path, "llms.txt");
    }

    // 6. Run run_doctor on generated project and assert core checks succeed
    assert_doctor_core_checks_succeed(&target, template);

    // 7. Test init_project in scaffolded project to verify idempotency (no files clobbered)
    assert_init_idempotency_and_non_clobber(&target, template);

    // 8. Verify execution latency is within SLA budget
    let elapsed = funnel_start.elapsed();
    assert!(
        elapsed < SLA_BUDGET,
        "Funnel verification for {template} exceeded SLA budget: {elapsed:?} >= {SLA_BUDGET:?}"
    );
}

#[test]
fn test_scaffold_smoke_bare() {
    verify_template_funnel(TemplateKind::Bare);
}

#[test]
fn test_scaffold_smoke_app() {
    verify_template_funnel(TemplateKind::App);
}

#[test]
fn test_scaffold_smoke_dashboard() {
    verify_template_funnel(TemplateKind::Dashboard);
}

#[test]
fn test_scaffold_smoke_all_templates_within_60s_budget() {
    let suite_start = Instant::now();

    for &template in &[
        TemplateKind::Bare,
        TemplateKind::App,
        TemplateKind::Dashboard,
    ] {
        verify_template_funnel(template);
    }

    let total_elapsed = suite_start.elapsed();
    assert!(
        total_elapsed < SLA_BUDGET,
        "All 3 templates funnel verification exceeded SLA budget: {total_elapsed:?} >= {SLA_BUDGET:?}"
    );
}
