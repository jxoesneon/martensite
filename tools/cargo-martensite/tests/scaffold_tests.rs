//! Integration and acceptance tests for project scaffolding and template generation.

use cargo_martensite::scaffold::{
    get_template_files, init_project, render_template, scaffold_project, validate_project_name,
    FileInitStatus, InitOptions, ScaffoldError, ScaffoldOptions, TemplateKind,
};
use std::collections::HashMap;
use std::fs;
use tempfile::TempDir;

#[test]
fn test_project_name_validation() {
    // Valid names
    assert!(validate_project_name("my_app").is_ok());
    assert!(validate_project_name("my-app").is_ok());
    assert!(validate_project_name("app123").is_ok());
    assert!(validate_project_name("x").is_ok());
    assert!(validate_project_name("_private_app").is_ok());
    assert!(validate_project_name("industrial-dashboard").is_ok());

    // Invalid names: empty
    assert!(matches!(
        validate_project_name(""),
        Err(ScaffoldError::InvalidProjectName { .. })
    ));

    // Invalid names: starting with digit or hyphen
    assert!(matches!(
        validate_project_name("123app"),
        Err(ScaffoldError::InvalidProjectName { .. })
    ));
    assert!(matches!(
        validate_project_name("-my-app"),
        Err(ScaffoldError::InvalidProjectName { .. })
    ));

    // Invalid characters
    assert!(matches!(
        validate_project_name("my app"),
        Err(ScaffoldError::InvalidProjectName { .. })
    ));
    assert!(matches!(
        validate_project_name("my.app"),
        Err(ScaffoldError::InvalidProjectName { .. })
    ));
    assert!(matches!(
        validate_project_name("my/app"),
        Err(ScaffoldError::InvalidProjectName { .. })
    ));
    assert!(matches!(
        validate_project_name("app@2"),
        Err(ScaffoldError::InvalidProjectName { .. })
    ));

    // Rust keywords
    assert!(matches!(
        validate_project_name("fn"),
        Err(ScaffoldError::InvalidProjectName { .. })
    ));
    assert!(matches!(
        validate_project_name("struct"),
        Err(ScaffoldError::InvalidProjectName { .. })
    ));
    assert!(matches!(
        validate_project_name("match"),
        Err(ScaffoldError::InvalidProjectName { .. })
    ));
    assert!(matches!(
        validate_project_name("crate"),
        Err(ScaffoldError::InvalidProjectName { .. })
    ));
    assert!(matches!(
        validate_project_name("self"),
        Err(ScaffoldError::InvalidProjectName { .. })
    ));
    assert!(matches!(
        validate_project_name("type"),
        Err(ScaffoldError::InvalidProjectName { .. })
    ));

    // Reserved identifiers
    assert!(matches!(
        validate_project_name("test"),
        Err(ScaffoldError::InvalidProjectName { .. })
    ));
    assert!(matches!(
        validate_project_name("con"),
        Err(ScaffoldError::InvalidProjectName { .. })
    ));
    assert!(matches!(
        validate_project_name("nul"),
        Err(ScaffoldError::InvalidProjectName { .. })
    ));
}

#[test]
fn test_strict_placeholder_replacement() {
    let mut ctx = HashMap::new();
    ctx.insert("project_name", "test_app");
    ctx.insert("martensite_version", "0.19.0");

    let template = "package = \"{{project_name}}\"\nversion = \"{{martensite_version}}\"";
    let rendered = render_template(template, &ctx, "test.toml").unwrap();
    assert_eq!(rendered, "package = \"test_app\"\nversion = \"0.19.0\"");

    // Missing placeholder produces hard error
    let bad_template = "package = \"{{project_name}}\"\nmissing = \"{{undefined_key}}\"";
    let err = render_template(bad_template, &ctx, "bad.toml").unwrap_err();
    assert!(matches!(
        err,
        ScaffoldError::UnresolvedPlaceholder { ref variable, ref template_file }
        if variable == "undefined_key" && template_file == "bad.toml"
    ));

    // Unclosed placeholder produces error
    let unclosed = "package = \"{{project_name\"";
    assert!(render_template(unclosed, &ctx, "unclosed.toml").is_err());
}

#[test]
fn test_embedded_templates_strictness() {
    // Verify every embedded template in the binary resolves without errors under standard context
    let mut ctx = HashMap::new();
    ctx.insert("project_name", "sample_app");
    ctx.insert("project_name_ident", "sample_app");
    ctx.insert("martensite_version", "0.19.0");

    for &kind in &[
        TemplateKind::App,
        TemplateKind::Bare,
        TemplateKind::Dashboard,
    ] {
        let files = get_template_files(kind);
        for file in files {
            let rendered = render_template(file.content, &ctx, file.path);
            assert!(
                rendered.is_ok(),
                "Template {:?} file {} failed strict placeholder resolution: {:?}",
                kind,
                file.path,
                rendered.err()
            );

            let content = rendered.unwrap();
            if file.path == "design-lint.toml" {
                let parsed = martensite_design_lint::LintConfig::from_toml(&content);
                assert!(
                    parsed.is_ok(),
                    "Template {:?} design-lint.toml failed to parse: {:?}",
                    kind,
                    parsed.err()
                );
            }
        }
    }
}

#[test]
fn test_scaffold_app_template() {
    let temp = TempDir::new().unwrap();
    let target = temp.path().join("my_app");

    let opts = ScaffoldOptions::new("my_app")
        .with_template(TemplateKind::App)
        .with_target_dir(&target);

    let result = scaffold_project(&opts);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), target);

    // Verify all files were created
    assert!(target.join("Cargo.toml").exists());
    assert!(target.join("src/main.rs").exists());
    assert!(target.join("AGENTS.md").exists());
    assert!(target.join("design-lint.toml").exists());
    assert!(target.join("martensite.toml").exists());
    assert!(target.join("llms.txt").exists());

    // Check Cargo.toml contents
    let cargo_toml = fs::read_to_string(target.join("Cargo.toml")).unwrap();
    assert!(cargo_toml.contains("name = \"my_app\""));
    assert!(cargo_toml.contains("martensite = "));

    // Check src/main.rs contents
    let main_rs = fs::read_to_string(target.join("src/main.rs")).unwrap();
    assert!(main_rs.contains("struct MartensiteApp"));
    assert!(main_rs.contains("pub count: Signal<i32>"));
    assert!(main_rs.contains("fn smoke_test_counter"));

    // Check AGENTS.md contents
    let agents_md = fs::read_to_string(target.join("AGENTS.md")).unwrap();
    assert!(agents_md.contains("The 10-Line Mental Model"));
    assert!(agents_md.contains("Widget Map"));
    assert!(agents_md.contains("Reactive Patterns"));
    assert!(agents_md.contains("cargo martensite dev"));

    // Check design-lint.toml contents
    let lint_toml = fs::read_to_string(target.join("design-lint.toml")).unwrap();
    assert!(lint_toml.contains("standards = ["));
    assert!(lint_toml.contains("wcag"));

    // Check martensite.toml contents
    let martensite_toml = fs::read_to_string(target.join("martensite.toml")).unwrap();
    assert!(martensite_toml.contains("[dev]"));
    assert!(martensite_toml.contains("[lint]"));
    assert!(martensite_toml.contains("[inspector]"));

    // Check llms.txt contents
    let llms_txt = fs::read_to_string(target.join("llms.txt")).unwrap();
    assert!(llms_txt.contains("my_app"));
    assert!(llms_txt.contains("https://martensite.dev"));
}

#[test]
fn test_scaffold_bare_template() {
    let temp = TempDir::new().unwrap();
    let target = temp.path().join("bare_app");

    let opts = ScaffoldOptions::new("bare_app")
        .with_template(TemplateKind::Bare)
        .with_target_dir(&target);

    scaffold_project(&opts).unwrap();

    assert!(target.join("Cargo.toml").exists());
    assert!(target.join("src/main.rs").exists());

    // Bare template must NOT create DX files
    assert!(!target.join("AGENTS.md").exists());
    assert!(!target.join("design-lint.toml").exists());
    assert!(!target.join("martensite.toml").exists());
    assert!(!target.join("llms.txt").exists());

    let main_rs = fs::read_to_string(target.join("src/main.rs")).unwrap();
    assert!(main_rs.contains("Hello from bare_app!"));
}

#[test]
fn test_scaffold_dashboard_template() {
    let temp = TempDir::new().unwrap();
    let target = temp.path().join("dash_app");

    let opts = ScaffoldOptions::new("dash_app")
        .with_template(TemplateKind::Dashboard)
        .with_target_dir(&target);

    scaffold_project(&opts).unwrap();

    assert!(target.join("Cargo.toml").exists());
    assert!(target.join("src/main.rs").exists());
    assert!(target.join("AGENTS.md").exists());
    assert!(target.join("design-lint.toml").exists());
    assert!(target.join("martensite.toml").exists());
    assert!(target.join("llms.txt").exists());

    let main_rs = fs::read_to_string(target.join("src/main.rs")).unwrap();
    assert!(main_rs.contains("struct DashboardApp"));
    assert!(main_rs.contains("pub dock: DockTree"));
    assert!(main_rs.contains("Telemetry Zone"));

    let lint_toml = fs::read_to_string(target.join("design-lint.toml")).unwrap();
    assert!(lint_toml.contains("isa-101"));
    assert!(lint_toml.contains("[rules.choice-count]"));
}

#[test]
fn test_atomic_scaffold_refuses_non_empty_target() {
    let temp = TempDir::new().unwrap();
    let target = temp.path().join("existing_app");
    fs::create_dir_all(&target).unwrap();

    let existing_file = target.join("precious_work.txt");
    fs::write(&existing_file, "do not delete me!").unwrap();

    let opts = ScaffoldOptions::new("existing_app")
        .with_template(TemplateKind::App)
        .with_target_dir(&target);

    let result = scaffold_project(&opts);
    assert!(matches!(
        result,
        Err(ScaffoldError::TargetExistsAndNotEmpty(ref p)) if p == &target
    ));

    // Assert existing directory contents are unchanged byte-for-byte
    assert_eq!(
        fs::read_to_string(&existing_file).unwrap(),
        "do not delete me!"
    );
    assert!(!target.join("Cargo.toml").exists());

    // Verify no staging directory was left behind
    for entry in fs::read_dir(temp.path()).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().to_string_lossy().to_string();
        assert!(
            !name.starts_with(".martensite-new-"),
            "Staging directory `{name}` was not cleaned up"
        );
    }
}

#[test]
fn test_atomic_scaffold_into_empty_directory_succeeds() {
    let temp = TempDir::new().unwrap();
    let target = temp.path().join("empty_dir");
    fs::create_dir_all(&target).unwrap();

    let opts = ScaffoldOptions::new("empty_dir")
        .with_template(TemplateKind::Bare)
        .with_target_dir(&target);

    let result = scaffold_project(&opts);
    assert!(result.is_ok());
    assert!(target.join("Cargo.toml").exists());
}

#[test]
fn test_init_command_generates_missing_agents() {
    let temp = TempDir::new().unwrap();
    let project_dir = temp.path().join("my_proj");
    fs::create_dir_all(&project_dir).unwrap();

    // Create minimal Cargo.toml
    fs::write(
        project_dir.join("Cargo.toml"),
        "[package]\nname = \"my_proj\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();

    let opts = InitOptions::new().with_agents_only(true);

    let statuses = init_project(&project_dir, &opts).unwrap();
    assert_eq!(statuses.len(), 1);
    assert_eq!(
        statuses[0],
        FileInitStatus::Created(project_dir.join("AGENTS.md"))
    );
    assert!(project_dir.join("AGENTS.md").exists());
    assert!(!project_dir.join("design-lint.toml").exists());
}

#[test]
fn test_init_command_generates_missing_lint() {
    let temp = TempDir::new().unwrap();
    let project_dir = temp.path().join("my_proj");
    fs::create_dir_all(&project_dir).unwrap();

    fs::write(
        project_dir.join("Cargo.toml"),
        "[package]\nname = \"my_proj\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();

    let opts = InitOptions::new().with_lint_only(true);

    let statuses = init_project(&project_dir, &opts).unwrap();
    assert_eq!(statuses.len(), 1);
    assert_eq!(
        statuses[0],
        FileInitStatus::Created(project_dir.join("design-lint.toml"))
    );
    assert!(project_dir.join("design-lint.toml").exists());
    assert!(!project_dir.join("AGENTS.md").exists());
}

#[test]
fn test_init_command_never_clobbers_edited_files() {
    let temp = TempDir::new().unwrap();
    let project_dir = temp.path().join("existing_project");
    fs::create_dir_all(&project_dir).unwrap();

    fs::write(
        project_dir.join("Cargo.toml"),
        "[package]\nname = \"existing_project\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();

    let custom_agents = "# My Custom AGENTS.md\nEdited by user with special rules.\n";
    fs::write(project_dir.join("AGENTS.md"), custom_agents).unwrap();

    let opts = InitOptions::new();
    let statuses = init_project(&project_dir, &opts).unwrap();

    // AGENTS.md must report DiffersNotOverwritten and preserve user edits
    let agents_status = statuses.iter().find(|s| match s {
        FileInitStatus::DiffersNotOverwritten(p) => p.ends_with("AGENTS.md"),
        _ => false,
    });
    assert!(agents_status.is_some());
    assert_eq!(
        fs::read_to_string(project_dir.join("AGENTS.md")).unwrap(),
        custom_agents
    );

    // Other missing files must be created
    assert!(project_dir.join("design-lint.toml").exists());
    assert!(project_dir.join("martensite.toml").exists());
    assert!(project_dir.join("llms.txt").exists());
}

#[test]
fn test_init_command_reports_unchanged_on_identical_file() {
    let temp = TempDir::new().unwrap();
    let project_dir = temp.path().join("idempotent_project");
    fs::create_dir_all(&project_dir).unwrap();

    fs::write(
        project_dir.join("Cargo.toml"),
        "[package]\nname = \"idempotent_project\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();

    let opts = InitOptions::new();

    // First init creates files
    let first_run = init_project(&project_dir, &opts).unwrap();
    assert!(first_run
        .iter()
        .all(|s| matches!(s, FileInitStatus::Created(_))));

    // Second init reports unchanged
    let second_run = init_project(&project_dir, &opts).unwrap();
    assert!(second_run
        .iter()
        .all(|s| matches!(s, FileInitStatus::Unchanged(_))));
}
