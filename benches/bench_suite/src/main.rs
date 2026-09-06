//! Milestone v0.1.0 performance verification benchmarks for Martensite.
//!
//! Evaluates core generational slotmap arena operations, linear DAG signal propagation,
//! and transactional diamond reactive networks against formal milestone exit gates.
#![forbid(unsafe_code)]

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

    // Milestone v0.1.0 verification: propagation latency must strictly be < 1.0ms.
    let start = Instant::now();
    root.set(2);
    let verified_val = leaf.get();
    let elapsed = start.elapsed();
    assert_eq!(verified_val, 2 + 9_999);
    assert!(
        elapsed < Duration::from_millis(1),
        "Milestone v0.1.0 exit gate failure: linear DAG propagation took {:?} (>= 1.0ms threshold)",
        elapsed
    );

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

criterion_group!(
    benches,
    bench_signal_propagation_10k,
    bench_arena_operations_10k,
    bench_diamond_reactive_network
);
criterion_main!(benches);
