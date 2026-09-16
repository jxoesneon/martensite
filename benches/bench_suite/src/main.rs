//! Milestone v0.1.0 performance verification benchmarks for Martensite.
//!
//! Evaluates core generational slotmap arena operations, linear DAG signal propagation,
//! and transactional diamond reactive networks against formal milestone exit gates.
#![forbid(unsafe_code)]
// Benchmark binaries generate criterion functions via macros that cannot be
// individually documented; allow missing docs for the generated functions only.
#![allow(missing_docs)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use criterion::{black_box, criterion_group, criterion_main, BatchSize, Criterion, Throughput};
use martensite_core::{ColdNode, HotNode, WidgetArena, WidgetId};
use martensite_reactive::{Memo, ReactiveRuntime, Signal};

/// Benchmark 1: 10,000-node linear DAG signal propagation latency.
///
/// Exit Gate: Propagation latency < 1.0ms across a 10,000-node linear dependency chain.
fn bench_signal_propagation_10k(c: &mut Criterion) {
    let runtime = ReactiveRuntime::new();
    let root = runtime.create_signal(0u64);

    // Construct 10,000-node linear DAG: 1 root Signal + 9,999 derived Memos.
    let mut leaf = runtime.create_memo({
        let r = root.clone();
        move || r.get().wrapping_add(1)
    });

    for _ in 2..10_000 {
        let prev = leaf.clone();
        leaf = runtime.create_memo(move || prev.get().wrapping_add(1));
    }

    // Warm-up iteration to populate internal scheduler queue capacities.
    root.set(1);
    let warm_val = leaf.get();
    assert_eq!(warm_val, 1 + 9_999);

    // Milestone v0.1.0 verification: propagation latency must be < 1.0ms on
    // dedicated hardware. The strict gate is enforced when
    // MARTENSITE_STRICT_BENCH=1 (release-time). We take the median of 100
    // runs to reduce noise; the CI threshold is 5.0ms to account for shared
    // runner variance, while the milestone target remains < 1.0ms.
    let strict = std::env::var("MARTENSITE_STRICT_BENCH")
        .map(|v| v == "1")
        .unwrap_or(false);
    if strict {
        let mut samples: Vec<Duration> = Vec::with_capacity(100);
        for i in 0..100u64 {
            root.set(100 + i);
            let start = Instant::now();
            root.set(200 + i);
            let _ = leaf.get();
            samples.push(start.elapsed());
        }
        samples.sort();
        let median = samples[samples.len() / 2];
        assert!(
            median < Duration::from_millis(5),
            "Milestone v0.1.0 exit gate failure: linear DAG propagation median {:?} (>= 5.0ms CI threshold; target is < 1.0ms on dedicated hardware)",
            median
        );
        eprintln!(
            "Signal propagation 10k: median {:?} (PASSED strict gate; target < 1.0ms on dedicated hardware)",
            median
        );
    } else {
        eprintln!(
            "Signal propagation 10k: strict gate disabled; set MARTENSITE_STRICT_BENCH=1 to enforce"
        );
    }

    let mut counter = 2u64;
    let mut group = c.benchmark_group("signal_propagation_10k");
    group.throughput(Throughput::Elements(10_000));
    group.bench_function("latency", |b| {
        b.iter(|| {
            counter = counter.wrapping_add(1);
            root.set(counter);
            let val = leaf.get();
            black_box(val);
        });
    });
    group.finish();
}

/// Helper function to construct a 10,000-node 4-ary hierarchical tree in WidgetArena.
fn setup_arena_tree() -> (WidgetArena, Vec<WidgetId>) {
    let mut arena = WidgetArena::with_capacity(10_000);
    let mut ids = Vec::with_capacity(10_000);

    for _ in 0..10_000 {
        let id = arena.insert(HotNode::default(), ColdNode::default());
        ids.push(id);
    }

    for i in 1..10_000 {
        let parent_idx = (i - 1) / 4;
        arena
            .append_child(ids[parent_idx], ids[i])
            .expect("append_child hierarchy construction must succeed");
    }

    (arena, ids)
}

/// Benchmark 2: 10,000-slot arena operations.
///
/// Evaluates:
/// 1. 10,000 HotNode/ColdNode allocations and insertions.
/// 2. Hierarchical 4-ary tree construction (depth-first pointers).
/// 3. Zero-allocation depth-first tree traversal.
/// 4. 1,000 removals with dense storage swap_remove compaction and FIFO free-list recycling.
///
/// Exit Gate: Full lifecycle completes in < 25ms on CI runners (reference
/// target < 1.14ms on dedicated hardware). Enforced when
/// `MARTENSITE_STRICT_BENCH=1`.
fn bench_arena_operations_10k(c: &mut Criterion) {
    // Pre-verify complete lifecycle correctness before running benchmark loop.
    {
        let (mut arena, ids) = setup_arena_tree();
        assert_eq!(arena.len(), 10_000);

        let mut visited = 0usize;
        for node_id in arena.iter_depth_first() {
            black_box(node_id);
            visited += 1;
        }
        assert_eq!(visited, 10_000);

        for i in (0..10_000).step_by(10) {
            let removed = arena.remove(ids[i]);
            assert!(removed.is_some());
        }
        assert_eq!(arena.len(), 9_000);
    }

    // Strict exit gate: median of 100 full-lifecycle runs must be < 25ms on
    // CI runners. The reference target is < 1.14ms on dedicated hardware;
    // the CI threshold is intentionally loose to absorb shared-runner
    // variance while still catching gross regressions.
    let strict = std::env::var("MARTENSITE_STRICT_BENCH")
        .map(|v| v == "1")
        .unwrap_or(false);
    if strict {
        let mut samples: Vec<Duration> = Vec::with_capacity(100);
        for _ in 0..100 {
            let start = Instant::now();
            let (mut arena, ids) = setup_arena_tree();
            for node_id in arena.iter_depth_first() {
                black_box(node_id);
            }
            for i in (0..10_000).step_by(10) {
                let removed = arena.remove(ids[i]);
                black_box(removed);
            }
            black_box(arena.len());
            samples.push(start.elapsed());
        }
        samples.sort();
        let median = samples[samples.len() / 2];
        assert!(
            median < Duration::from_millis(25),
            "Milestone arena exit gate failure: 10k lifecycle median {:?} (>= 25.0ms CI threshold; \
             target is < 1.14ms on dedicated hardware)",
            median
        );
        eprintln!(
            "Arena operations 10k: median {:?} (PASSED strict gate; target < 1.14ms on dedicated hardware)",
            median
        );
    } else {
        eprintln!(
            "Arena operations 10k: strict gate disabled; set MARTENSITE_STRICT_BENCH=1 to enforce"
        );
    }

    let mut group = c.benchmark_group("arena_operations_10k");
    group.throughput(Throughput::Elements(10_000));

    // Full lifecycle benchmark: insertion, hierarchy linkage, DFS traversal, and compaction.
    group.bench_function("lifecycle_10k", |b| {
        b.iter(|| {
            let (mut arena, ids) = setup_arena_tree();

            let mut visited = 0usize;
            for node_id in arena.iter_depth_first() {
                black_box(node_id);
                visited += 1;
            }
            black_box(visited);

            for i in (0..10_000).step_by(10) {
                let removed = arena.remove(ids[i]);
                black_box(removed);
            }
            black_box(arena.len());
        });
    });

    // Isolated zero-allocation depth-first traversal of 10,000-node tree.
    group.bench_function("dfs_traversal_10k", |b| {
        let (arena, _ids) = setup_arena_tree();
        b.iter(|| {
            let mut visited = 0usize;
            for node_id in arena.iter_depth_first() {
                black_box(node_id);
                visited += 1;
            }
            black_box(visited);
        });
    });

    // Isolated swap_remove compaction: 1,000 removals from a 10,000-node tree.
    group.bench_function("compaction_1k_removals", |b| {
        b.iter_batched(
            setup_arena_tree,
            |(mut arena, ids)| {
                for i in (0..10_000).step_by(10) {
                    let removed = arena.remove(ids[i]);
                    black_box(removed);
                }
                black_box(arena.len());
            },
            BatchSize::SmallInput,
        );
    });

    group.finish();
}

/// Benchmark 3: 1,000 diamond reactive network evaluation latency.
///
/// Constructs 1,000 diamond subgraphs (Signal -> MemoA, MemoB -> MemoD(A, B)),
/// mutates root signals inside a transactional batch, and measures topological evaluation latency.
///
/// Exit Gate: Evaluation executes with zero glitches and zero redundant evaluations.
fn bench_diamond_reactive_network(c: &mut Criterion) {
    let runtime = ReactiveRuntime::new();

    struct Diamond {
        root: Signal<u64>,
        _memo_a: Memo<u64>,
        _memo_b: Memo<u64>,
        memo_d: Memo<u64>,
    }

    // Instrumented diamond to verify glitch-free execution (zero redundant evaluations).
    let d_eval_count = Arc::new(AtomicUsize::new(0));
    let d_eval_count_clone = Arc::clone(&d_eval_count);

    let test_root = runtime.create_signal(10u64);
    let r_a = test_root.clone();
    let test_a = runtime.create_memo(move || r_a.get().wrapping_mul(2));
    let r_b = test_root.clone();
    let test_b = runtime.create_memo(move || r_b.get().wrapping_add(5));
    let a_d = test_a.clone();
    let b_d = test_b.clone();
    let test_d = runtime.create_memo(move || {
        d_eval_count_clone.fetch_add(1, Ordering::SeqCst);
        a_d.get().wrapping_add(b_d.get())
    });

    // Initial evaluation on creation: exactly 1 evaluation.
    assert_eq!(test_d.get(), (10 * 2) + (10 + 5));
    assert_eq!(d_eval_count.load(Ordering::SeqCst), 1);

    // Mutate root inside a batch. Topological scheduling must evaluate D exactly once.
    runtime.batch(|| {
        test_root.set(20);
    });
    assert_eq!(test_d.get(), (20 * 2) + (20 + 5));
    assert_eq!(
        d_eval_count.load(Ordering::SeqCst),
        2,
        "Exit gate failed: diamond evaluation had redundant/glitch evaluations"
    );

    // Construct 1,000 diamond subgraphs (4,000 reactive nodes total).
    let diamonds: Vec<Diamond> = (0..1_000)
        .map(|i| {
            let root = runtime.create_signal(i as u64);
            let root_a = root.clone();
            let memo_a = runtime.create_memo(move || root_a.get().wrapping_mul(3));
            let root_b = root.clone();
            let memo_b = runtime.create_memo(move || root_b.get().wrapping_add(7));
            let a_d = memo_a.clone();
            let b_d = memo_b.clone();
            let memo_d = runtime.create_memo(move || a_d.get().wrapping_add(b_d.get()));
            Diamond {
                root,
                _memo_a: memo_a,
                _memo_b: memo_b,
                memo_d,
            }
        })
        .collect();

    // Verify initial values across all 1,000 diamond terminals.
    for (i, d) in diamonds.iter().enumerate() {
        let expected = ((i as u64) * 3) + ((i as u64) + 7);
        assert_eq!(d.memo_d.get(), expected);
    }

    // Warm-up batch evaluation
    runtime.batch(|| {
        for d in &diamonds {
            d.root.set(100);
        }
    });
    for d in &diamonds {
        assert_eq!(d.memo_d.get(), (100 * 3) + (100 + 7));
    }

    // Strict exit gate: median of 100 batch evaluations must be < 25ms on
    // CI runners. There is no dedicated-hardware reference target for this
    // workload; the CI threshold is intentionally loose to absorb
    // shared-runner variance while catching gross regressions and
    // verifying glitch-free topological evaluation.
    let strict = std::env::var("MARTENSITE_STRICT_BENCH")
        .map(|v| v == "1")
        .unwrap_or(false);
    if strict {
        let mut samples: Vec<Duration> = Vec::with_capacity(100);
        let mut counter = 100u64;
        for _ in 0..100 {
            counter = counter.wrapping_add(1);
            let start = Instant::now();
            runtime.batch(|| {
                for d in &diamonds {
                    d.root.set(counter);
                }
            });
            for d in &diamonds {
                black_box(d.memo_d.get());
            }
            samples.push(start.elapsed());
        }
        samples.sort();
        let median = samples[samples.len() / 2];
        assert!(
            median < Duration::from_millis(25),
            "Milestone diamond exit gate failure: 1k batch evaluation median {:?} (>= 25.0ms CI threshold)",
            median
        );
        eprintln!(
            "Diamond reactive network 1k: median {:?} (PASSED strict gate)",
            median
        );
    } else {
        eprintln!(
            "Diamond reactive network 1k: strict gate disabled; set MARTENSITE_STRICT_BENCH=1 to enforce"
        );
    }

    let mut counter = 100u64;
    let mut group = c.benchmark_group("diamond_reactive_network");
    group.throughput(Throughput::Elements(1_000));
    group.bench_function("batch_evaluation_1k", |b| {
        b.iter(|| {
            counter = counter.wrapping_add(1);
            runtime.batch(|| {
                for d in &diamonds {
                    d.root.set(counter);
                }
            });
            for d in &diamonds {
                black_box(d.memo_d.get());
            }
        });
    });
    group.finish();
}

// =========================================================================
// v0.18.0 competitive baselines — egui/iced-comparable shared primitives
// =========================================================================
//
// These benchmarks measure the Martensite primitives that correspond to
// the workloads competitor frameworks publish numbers for. They are NOT
// cross-framework benchmarks: egui/iced run their own harnesses, so the
// results are directional comparisons only (see docs/BENCHMARKS.md §5).
// No competitor crates are linked — the lockfile stays clean.

/// Builds a 1,051-node widget tree inside `LayoutEngine`:
/// 1 root column + 50 row containers + 1,000 leaf cells (50 × 20).
///
/// WidgetIds are minted directly with `WidgetId::from_parts` — the engine
/// only needs the id as a map key, no `WidgetArena` is required for a
/// pure layout-throughput measurement.
fn setup_layout_tree() -> (martensite_layout::LayoutEngine, taffy::NodeId) {
    use martensite_layout::{Display, LayoutEngine};
    use taffy::prelude::*;
    let mut engine = LayoutEngine::new();
    let root_wid = WidgetId::from_parts(0, 1);
    let root = engine
        .register_node(
            root_wid,
            Style {
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                gap: Size {
                    width: length(0.0),
                    height: length(4.0),
                },
                ..Default::default()
            },
        )
        .expect("register root");

    let row_wids: Vec<WidgetId> = (1..=50u32).map(|i| WidgetId::from_parts(i, 1)).collect();
    engine
        .set_children(root_wid, &row_wids)
        .expect("set root children");

    let mut next = 51u32;
    for &row in &row_wids {
        engine
            .register_node(
                row,
                Style {
                    display: Display::Flex,
                    flex_direction: FlexDirection::Row,
                    gap: Size {
                        width: length(6.0),
                        height: length(0.0),
                    },
                    ..Default::default()
                },
            )
            .expect("register row");
        let leaf_wids: Vec<WidgetId> = (0..20)
            .map(|_| {
                let w = WidgetId::from_parts(next, 1);
                next += 1;
                w
            })
            .collect();
        engine
            .set_children(row, &leaf_wids)
            .expect("set row children");
        // Style the leaves AFTER set_children (register_node re-applies
        // the given style to an already-registered id — see F11 note in
        // examples/industrial_dashboard).
        for (j, &w) in leaf_wids.iter().enumerate() {
            engine
                .register_node(
                    w,
                    Style {
                        size: Size {
                            width: length(24.0 + (j % 5) as f32 * 8.0),
                            height: length(16.0),
                        },
                        ..Default::default()
                    },
                )
                .expect("style leaf");
        }
    }
    (engine, root)
}

/// Competitive baseline 1: two-pass layout throughput on a ~1k-node tree.
///
/// Comparable workload: a full retained/immediate layout pass over a
/// realistic panel (50 rows × 20 cells). egui re-layouts every widget
/// every frame; iced recomputes its widget-tree layout on each view pass.
fn bench_layout_1k_widget_tree(c: &mut Criterion) {
    use martensite_layout::{constraints_to_available, Constraints};
    let mut group = c.benchmark_group("competitive_layout_1k");
    group.throughput(Throughput::Elements(1_051));

    group.bench_function("two_pass_compute", |b| {
        let (mut engine, root) = setup_layout_tree();
        let avail = constraints_to_available(Constraints::tight(1600.0, 900.0));
        b.iter(|| {
            engine.compute(root, avail).expect("compute");
        });
    });

    // "Relayout" in immediate-mode terms: identical second pass after
    // mutating one leaf's style — Taffy recomputes from root.
    group.bench_function("recompute_after_leaf_change", |b| {
        let (mut engine, root) = setup_layout_tree();
        let avail = constraints_to_available(Constraints::tight(1600.0, 900.0));
        engine.compute(root, avail).expect("warm compute");
        let leaf = WidgetId::from_parts(51, 1);
        let mut flip = false;
        b.iter(|| {
            flip = !flip;
            engine
                .register_node(
                    leaf,
                    taffy::prelude::Style {
                        size: taffy::prelude::Size {
                            width: taffy::prelude::length(if flip { 24.0 } else { 40.0 }),
                            height: taffy::prelude::length(16.0),
                        },
                        ..Default::default()
                    },
                )
                .expect("restyle leaf");
            engine.compute(root, avail).expect("recompute");
        });
    });
    group.finish();
}

/// Competitive baseline 2: UI-scale signal fan-out.
///
/// One root signal feeding 200 derived memos — the scale of a real
/// dashboard's per-frame state propagation (vs. the 10k-node milestone
/// gate). Comparable workload: iced's update→view re-evaluation per
/// message, egui's per-frame immediate re-run.
fn bench_signal_fan_out_200(c: &mut Criterion) {
    let runtime = ReactiveRuntime::new();
    let root = runtime.create_signal(0u64);
    let memos: Vec<Memo<u64>> = (0..200)
        .map(|i| {
            let r = root.clone();
            runtime.create_memo(move || r.get().wrapping_add(i))
        })
        .collect();
    root.set(1);
    for (i, m) in memos.iter().enumerate() {
        assert_eq!(m.get(), 1 + i as u64);
    }

    let mut counter = 1u64;
    let mut group = c.benchmark_group("competitive_signal_fan_out_200");
    group.throughput(Throughput::Elements(200));
    group.bench_function("set_and_resolve", |b| {
        b.iter(|| {
            counter = counter.wrapping_add(1);
            root.set(counter);
            let mut acc = 0u64;
            for m in &memos {
                acc = acc.wrapping_add(m.get());
            }
            black_box(acc);
        });
    });
    group.finish();
}

/// Competitive baseline 3: text shaping throughput.
///
/// Comparable workload: per-frame label shaping plus one wrapped
/// paragraph. Note iced 0.13 also sits on cosmic-text, so this is the
/// closest-to-apples comparison in the suite — remaining differences are
/// harness overhead and cache layers, not the shaper.
fn bench_text_shaping(c: &mut Criterion) {
    use martensite_text::{shape_text, FontManager};
    let mut manager = FontManager::new();

    // 500 realistic label strings (~30 chars), the scale of a dense
    // dashboard's visible text per frame.
    let labels: Vec<String> = (0..500)
        .map(|i| {
            format!(
                "core {:02}  load {:.1}%  pid {}",
                i % 64,
                (i % 100) as f32,
                1000 + i
            )
        })
        .collect();

    let paragraph: String = (0..40)
        .map(|i| format!("Sensor {i} reports nominal throughput across the monitored segment; "))
        .collect();

    let mut group = c.benchmark_group("competitive_text_shaping");
    group.throughput(Throughput::Elements(500));
    group.bench_function("labels_500_cold", |b| {
        b.iter(|| {
            for s in &labels {
                black_box(shape_text(&mut manager, black_box(s), 16.0, 20.0, None));
            }
        });
    });
    group.throughput(Throughput::Elements(paragraph.len() as u64));
    group.bench_function("paragraph_wrap_480px", |b| {
        b.iter(|| {
            black_box(shape_text(
                &mut manager,
                black_box(&paragraph),
                16.0,
                20.0,
                Some(480.0),
            ));
        });
    });
    group.finish();
}

/// Competitive baseline 4: virtualized scroll on a 1M-row table.
///
/// Comparable workload: egui `ScrollArea`/`Grid` visible-row culling and
/// iced `scrollable` viewport math. The Martensite claim is O(1) per
/// scroll step with zero allocation in `visible_rows`.
fn bench_virtualized_scroll_1m(c: &mut Criterion) {
    use martensite_blessed::DataTable;
    let mut table = DataTable::new(vec![0_u64; 1_000_000], 22.0);
    table.set_viewport_height(880.0);
    assert_eq!(table.visible_range().len(), 40);

    let mut group = c.benchmark_group("competitive_virtualized_scroll_1m");
    group.throughput(Throughput::Elements(1_000_000));
    group.bench_function("scroll_step_plus_visible_window", |b| {
        let mut dir = 1.0f32;
        b.iter(|| {
            dir = -dir;
            table.scroll_by(220.0 * dir);
            let mut visited = 0usize;
            for (idx, row) in table.visible_rows() {
                black_box((idx, row));
                visited += 1;
            }
            black_box(visited);
        });
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_signal_propagation_10k,
    bench_arena_operations_10k,
    bench_diamond_reactive_network,
    bench_layout_1k_widget_tree,
    bench_signal_fan_out_200,
    bench_text_shaping,
    bench_virtualized_scroll_1m
);
criterion_main!(benches);
