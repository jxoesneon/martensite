//! `design-lint` — the design-standards CLI for the industrial
//! dashboard.
//!
//! Sweeps every zone page in a `ScrollView` across widths and scroll
//! offsets plus the full dock, running `martensite-design-lint` over
//! each frame — the same surfaces as the `dump_design_lints` test.
//!
//! ```text
//! cargo run -p industrial_dashboard --bin design_lint -- [FLAGS]
//!
//!   --fix                  apply Safe autofix ops, then re-lint
//!   --force                also apply Risky fixes (with --fix)
//!   --no-recursive         single lint→fix pass (default: recurse
//!                          until convergence)
//!   --max-recursiveness N  cap fix passes (default 8)
//!   --config PATH          design-lint.toml (default:
//!                          design-lint.toml next to this package)
//!   --filter STR           only sweep tags containing STR
//!   --scopes               dump the scope tree per frame
//!   --quiet                summary only
//!   --list-rules           print the rule catalog and exit
//! ```

use industrial_dashboard::lint_sweep::{self, SweepOptions};
use martensite_design_lint::{rule_catalog, FixOptions, LintConfig};

fn usage() -> ! {
    eprintln!(
        "usage: design-lint [--fix] [--force] [--no-recursive] \
         [--max-recursiveness N] [--config PATH] [--filter STR] \
         [--scopes] [--quiet] [--list-rules]"
    );
    std::process::exit(2);
}

fn main() {
    let mut fix = false;
    let mut force = false;
    let mut recursive = true;
    let mut max_depth = 8usize;
    let mut config_path =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("design-lint.toml");
    let mut opts = SweepOptions::default();

    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--fix" => fix = true,
            "--force" => force = true,
            "--no-recursive" => recursive = false,
            "--max-recursiveness" => {
                let Some(v) = args.next() else {
                    usage();
                };
                max_depth = v.parse().unwrap_or_else(|_| usage());
            }
            "--config" => {
                let Some(v) = args.next() else {
                    usage();
                };
                config_path = v.into();
            }
            "--filter" => {
                let Some(v) = args.next() else {
                    usage();
                };
                opts.page_filter = v;
            }
            "--scopes" => opts.dump_scopes = true,
            "--quiet" => opts.quiet = true,
            "--list-rules" => {
                for (id, title, sev, standards) in rule_catalog() {
                    let stds = standards
                        .iter()
                        .map(|s| s.config_key())
                        .collect::<Vec<_>>()
                        .join(",");
                    println!("{id:<24} {sev:<6} [{stds}]  {title}");
                }
                return;
            }
            "--help" | "-h" => usage(),
            _ => usage(),
        }
    }

    if force && !fix {
        eprintln!("design-lint: --force implies --fix; enabling it");
        fix = true;
    }
    if fix {
        opts.fix = Some(FixOptions {
            force,
            recursive,
            max_depth,
        });
    }

    let cfg = LintConfig::from_file(&config_path)
        .unwrap_or_else(|e| {
            eprintln!("design-lint: {}: {e}", config_path.display());
            std::process::exit(2)
        })
        .unwrap_or_else(|| {
            eprintln!("design-lint: {}: not found", config_path.display());
            std::process::exit(2)
        });

    let report = lint_sweep::run(&cfg, &opts);
    print!("{}", report.log);

    // Exit code: 1 only when Warn+ findings remain — Info is a
    // review prompt, not a failure (CI-usable).
    if report.gating_findings > 0 {
        std::process::exit(1);
    }
}
