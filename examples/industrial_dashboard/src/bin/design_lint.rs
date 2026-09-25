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
//!   --dump-frames          rasterize every swept frame via
//!                          TinySkiaBackend → PNG under
//!                          target/dashboard-frames/ (plus deutan/
//!                          protan CVD sims); widths default to
//!                          700,1200,1600 per spec A1
//!   --frames-dir DIR       override the dump output dir
//!   --widths W1,W2,...     override the sweep widths
//!   --check DIR            imply --dump-frames and diff each frame
//!                          against baseline DIR (drift → exit 1)
//!   --list-rules           print the rule catalog and exit
//! ```

use industrial_dashboard::frames::FrameDump;
use industrial_dashboard::lint_sweep::{self, SweepOptions};
use martensite_design_lint::{rule_catalog, FixOptions, LintConfig};

fn usage() -> ! {
    eprintln!(
        "usage: design-lint [--fix] [--force] [--no-recursive] \
         [--max-recursiveness N] [--config PATH] [--filter STR] \
         [--scopes] [--quiet] [--dump-frames] [--frames-dir DIR] \
         [--widths W1,W2,...] [--check DIR] [--list-rules]"
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
    let mut dump_frames = false;
    let mut frames_dir = None;
    let mut baseline = None;
    let mut widths = None;
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
            "--dump-frames" => dump_frames = true,
            "--frames-dir" => {
                let Some(v) = args.next() else {
                    usage();
                };
                frames_dir = Some(v.into());
            }
            "--widths" => {
                let Some(v) = args.next() else {
                    usage();
                };
                widths = Some(
                    v.split(',')
                        .map(|w| w.trim().parse().unwrap_or_else(|_| usage()))
                        .collect::<Vec<f32>>(),
                );
            }
            "--check" => {
                let Some(v) = args.next() else {
                    usage();
                };
                baseline = Some(v.into());
            }
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

    // Frame dump (spec A1): --check implies --dump-frames; dump runs
    // use the spec widths 700/1200/1600 unless --widths overrides.
    if baseline.is_some() && !dump_frames {
        eprintln!("design-lint: --check implies --dump-frames; enabling it");
        dump_frames = true;
    }
    if dump_frames {
        opts.dump_frames = Some(FrameDump {
            dir: frames_dir.unwrap_or_else(industrial_dashboard::frames::default_dir),
            baseline,
        });
        opts.widths = widths.unwrap_or_else(|| industrial_dashboard::frames::FRAME_WIDTHS.to_vec());
    } else if let Some(w) = widths {
        opts.widths = w;
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
    // review prompt, not a failure (CI-usable). Golden drift under
    // --check gates identically.
    if report.gating_findings > 0 || report.frames_drifted > 0 {
        std::process::exit(1);
    }
}
