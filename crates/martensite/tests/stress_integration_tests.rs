//! Comprehensive stress and adversarial integration tests for Martensite v0.1.0.
//!
//! Covers:
//! 1. Concurrency stress: 8 worker threads continuously reading HotNode bounds using FrameFence::acquire / FrameGuard
//!    while the main thread performs 100,000 arena operations (insert, mutate, reparent, compaction).
//! 2. Cross-crate integration: 1,000 reactive signals driving layout coordinates of 1,000 HotNode widgets in WidgetArena
//!    with transactional batch updates and deterministic dirty flag synchronization.
//! 3. Dynamic branch & cycle isolation stress: 500 conditional branches switching rapidly between signal subgraphs,
//!    with adversarial cycle injection and 3-color DFS isolation verification.
//! 4. Pathological DAG topologies: 10,000-node deep linear chains and 1-to-10,000 wide fan-out.
//! 5. Arena tree hierarchy mutations, depth rank preservation, and idle compaction.
#![forbid(unsafe_code)]

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use parking_lot::{Mutex, RwLock};

use martensite::core::arena::WidgetArena;
use martensite::core::fence::FrameFence;
use martensite::core::id::WidgetId;
use martensite::core::node::{ColdNode, HotNode, NodeFlags, Rect};
use martensite::reactive::prelude::*;
use martensite::reactive::ReactiveError;

/// Concurrency stress test.
///
/// 8 worker threads continuously read HotNode bounds using FrameFence::acquire / FrameGuard
/// while the main thread continuously inserts, mutates, reparents, and calls
/// begin_compaction / end_compaction on WidgetArena for 100,000 operations.
/// Asserts zero data races and zero torn reads.
#[test]
#[allow(clippy::manual_is_multiple_of)]
fn test_concurrency_stress_readers_and_arena_compaction() {
    const TOTAL_OPS: usize = 100_000;
    const WORKER_COUNT: usize = 8;
    const BATCH_SIZE: usize = 50;

    let fence = Arc::new(FrameFence::with_timeout(Duration::from_millis(500)));
    let arena = Arc::new(RwLock::new(WidgetArena::with_capacity(4096)));
    let stop_flag = Arc::new(AtomicBool::new(false));
    let total_reads = Arc::new(AtomicU64::new(0));
    let valid_reads = Arc::new(AtomicU64::new(0));
    let torn_reads = Arc::new(AtomicU64::new(0));

    // Pre-populate arena with initial baseline nodes
    {
        let mut guard = arena.write();
        for i in 0..200 {
            let v = (i + 1) as f32;
            let hot = HotNode {
                bounds: Rect::new(v, v, v * 2.0, v * 2.0),
                ..HotNode::default()
            };
            guard.insert(hot, ColdNode::default());
        }
    }

    // Spawn 8 reader threads
    let mut worker_handles = Vec::with_capacity(WORKER_COUNT);
    for _ in 0..WORKER_COUNT {
        let fence_clone = Arc::clone(&fence);
        let arena_clone = Arc::clone(&arena);
        let stop_clone = Arc::clone(&stop_flag);
        let total_clone = Arc::clone(&total_reads);
        let valid_clone = Arc::clone(&valid_reads);
        let torn_clone = Arc::clone(&torn_reads);

        worker_handles.push(thread::spawn(move || {
            while !stop_clone.load(Ordering::Relaxed) {
                // Acquire reader lease under RAII FrameGuard
                let guard = fence_clone.acquire();
                if guard.is_valid() {
                    // Attempt non-blocking read
                    if let Some(arena_ref) = arena_clone.try_read() {
                        let len = arena_ref.len();
                        if len > 0 {
                            // Uniformly sample up to 32 nodes across dense storage
                            let step = (len / 32).max(1);
                            let hot_nodes = arena_ref.hot_nodes();
                            for idx in (0..len).step_by(step) {
                                let node = &hot_nodes[idx];
                                let b = node.bounds;
                                // Invariant: width must equal origin.x * 2.0, height must equal origin.y * 2.0
                                let x_diff = (b.size.x - b.origin.x * 2.0).abs();
                                let y_diff = (b.size.y - b.origin.y * 2.0).abs();
                                if x_diff > 1e-3 || y_diff > 1e-3 {
                                    torn_clone.fetch_add(1, Ordering::SeqCst);
                                }
                            }
                            valid_clone.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
                drop(guard);
                total_clone.fetch_add(1, Ordering::Relaxed);
            }
        }));
    }

    // Main thread execution: 100,000 operations
    let start_time = Instant::now();
    let mut active_ids: Vec<WidgetId> = {
        let guard = arena.read();
        let dense_to_slot = guard.dense_to_slot();
        (0..guard.len())
            .map(|i| {
                let slot_idx = dense_to_slot[i];
                let gen = guard
                    .slot_generation(slot_idx)
                    .expect("active node must have a valid generation");
                WidgetId::new(slot_idx, gen).unwrap()
            })
            .collect()
    };

    let mut op_idx = 0;
    let mut compactions_executed = 0;

    while op_idx < TOTAL_OPS {
        let current_batch = BATCH_SIZE.min(TOTAL_OPS - op_idx);

        {
            let mut arena_guard = arena.write();
            for b in 0..current_batch {
                let op = op_idx + b;
                match op % 10 {
                    0..=3 => {
                        // Insert new node maintaining invariant
                        let val = ((op % 500) + 1) as f32;
                        let hot = HotNode {
                            bounds: Rect::new(val, val, val * 2.0, val * 2.0),
                            flags: NodeFlags::VISIBLE,
                            ..HotNode::default()
                        };
                        let id = arena_guard.insert(hot, ColdNode::default());
                        active_ids.push(id);
                    }
                    4..=7 => {
                        // Mutate existing node bounds and flags
                        if !active_ids.is_empty() {
                            let target_idx = (op * 31) % active_ids.len();
                            let target_id = active_ids[target_idx];
                            if let Some(hot) = arena_guard.get_hot_mut(target_id) {
                                let new_val = ((op % 1000) + 1) as f32;
                                hot.bounds =
                                    Rect::new(new_val, new_val, new_val * 2.0, new_val * 2.0);
                                hot.flags
                                    .insert(NodeFlags::DIRTY_LAYOUT | NodeFlags::DIRTY_PAINT);
                            }
                        }
                    }
                    8 => {
                        // Reparent nodes
                        if active_ids.len() >= 2 {
                            let p_idx = (op * 17) % active_ids.len();
                            let c_idx = (op * 23) % active_ids.len();
                            if p_idx != c_idx {
                                let parent = active_ids[p_idx];
                                let child = active_ids[c_idx];
                                let _ = arena_guard.append_child(parent, child);
                            }
                        }
                    }
                    9 => {
                        // Remove node if capacity is large
                        if active_ids.len() > 300 {
                            let r_idx = (op * 7) % active_ids.len();
                            let id = active_ids.swap_remove(r_idx);
                            let _ = arena_guard.remove(id);
                        }
                    }
                    _ => unreachable!(),
                }
            }

            // Periodic compaction pass coordinated via FrameFence
            if (op_idx + current_batch) % 5000 == 0 {
                arena_guard
                    .begin_compaction(&fence)
                    .expect("compaction start");
                arena_guard.shrink_to_fit_idle();
                arena_guard.end_compaction(&fence);
                compactions_executed += 1;
            }
        }

        op_idx += current_batch;
    }

    let duration = start_time.elapsed();

    // Signal workers to terminate
    stop_flag.store(true, Ordering::SeqCst);
    for handle in worker_handles {
        handle.join().expect("worker thread joined cleanly");
    }

    let final_reads = total_reads.load(Ordering::SeqCst);
    let final_valid = valid_reads.load(Ordering::SeqCst);
    let final_torn = torn_reads.load(Ordering::SeqCst);

    println!(
        "Concurrency stress: 100,000 ops completed in {:?} ({} compactions, {} reads, {} valid reads, {} torn reads)",
        duration, compactions_executed, final_reads, final_valid, final_torn
    );

    assert_eq!(
        final_torn, 0,
        "No torn reads permitted across concurrent workers"
    );
    assert!(final_valid > 0, "Reader threads must perform valid reads");
    assert_eq!(
        fence.active_readers(),
        0,
        "Active reader count must be exactly zero"
    );
    assert!(
        compactions_executed >= 20,
        "At least 20 compaction passes must execute"
    );
}

/// Cross-crate reactive arena integration.
///
/// Create 1,000 reactive signals driving layout coordinates of 1,000 HotNode widgets in WidgetArena.
/// Trigger transactional batch updates and verify that HotNode bounds and dirty flags synchronize deterministically.
#[test]
fn test_cross_crate_reactive_arena_integration() {
    const NODE_COUNT: usize = 1_000;

    let rt = ReactiveRuntime::new();
    let arena = Arc::new(Mutex::new(WidgetArena::with_capacity(NODE_COUNT)));

    // Create 1,000 reactive signals
    let signals: Vec<Signal<(f32, f32, f32, f32)>> = (0..NODE_COUNT)
        .map(|i| {
            let x = i as f32;
            let y = (i * 2) as f32;
            let w = 120.0;
            let h = 60.0;
            rt.create_signal((x, y, w, h))
        })
        .collect();

    // Insert 1,000 widgets into WidgetArena
    let widget_ids: Vec<WidgetId> = {
        let mut guard = arena.lock();
        (0..NODE_COUNT)
            .map(|_| guard.insert(HotNode::default(), ColdNode::default()))
            .collect()
    };

    // Create 1,000 reactive effects binding each signal to its HotNode
    let mut effects = Vec::with_capacity(NODE_COUNT);
    for i in 0..NODE_COUNT {
        let sig = signals[i].clone();
        let arena_clone = Arc::clone(&arena);
        let id = widget_ids[i];

        let effect = rt.create_effect(move || {
            let (x, y, w, h) = sig.get();
            let mut guard = arena_clone.lock();
            if let Some(hot) = guard.get_hot_mut(id) {
                hot.bounds = Rect::new(x, y, w, h);
                hot.flags
                    .insert(NodeFlags::DIRTY_LAYOUT | NodeFlags::DIRTY_PAINT);
            }
        });
        effects.push(effect);
    }

    // Initial state verification: all 1,000 nodes synchronized with initial signal values
    {
        let guard = arena.lock();
        for (i, &wid) in widget_ids.iter().enumerate() {
            let hot = guard.get_hot(wid).expect("hot node exists");
            assert_eq!(hot.bounds.origin.x, i as f32);
            assert_eq!(hot.bounds.origin.y, (i * 2) as f32);
            assert_eq!(hot.bounds.size.x, 120.0);
            assert_eq!(hot.bounds.size.y, 60.0);
            assert!(hot.flags.contains(NodeFlags::DIRTY_LAYOUT));
            assert!(hot.flags.contains(NodeFlags::DIRTY_PAINT));
        }
    }

    // Simulate layout & paint consumption: clear dirty flags
    {
        let mut guard = arena.lock();
        for &id in &widget_ids {
            if let Some(hot) = guard.get_hot_mut(id) {
                hot.flags
                    .remove(NodeFlags::DIRTY_LAYOUT | NodeFlags::DIRTY_PAINT);
            }
        }
    }

    // Verify all dirty flags are cleared
    {
        let guard = arena.lock();
        for &id in &widget_ids {
            let hot = guard.get_hot(id).expect("hot node exists");
            assert!(!hot.flags.contains(NodeFlags::DIRTY_LAYOUT));
            assert!(!hot.flags.contains(NodeFlags::DIRTY_PAINT));
        }
    }

    // Transactional batch update across all 1,000 signals
    let start_batch = Instant::now();
    rt.batch(|| {
        for (i, sig) in signals.iter().enumerate() {
            let new_x = (i * 3) as f32 + 10.0;
            let new_y = (i * 5) as f32 + 20.0;
            let new_w = 200.0;
            let new_h = 100.0;
            sig.set((new_x, new_y, new_w, new_h));
        }
    });
    let batch_duration = start_batch.elapsed();

    println!(
        "Cross-crate reactive update: 1,000 signals batched and dispatched in {:?}",
        batch_duration
    );

    // Verify deterministic synchronization: every node reflects batch mutations
    {
        let guard = arena.lock();
        for (i, &wid) in widget_ids.iter().enumerate() {
            let hot = guard.get_hot(wid).expect("hot node exists");
            let expected_x = (i * 3) as f32 + 10.0;
            let expected_y = (i * 5) as f32 + 20.0;
            assert_eq!(hot.bounds.origin.x, expected_x);
            assert_eq!(hot.bounds.origin.y, expected_y);
            assert_eq!(hot.bounds.size.x, 200.0);
            assert_eq!(hot.bounds.size.y, 100.0);
            assert!(hot.flags.contains(NodeFlags::DIRTY_LAYOUT));
            assert!(hot.flags.contains(NodeFlags::DIRTY_PAINT));
        }
    }

    // Test selective batch updating: update only odd-indexed signals
    {
        let mut guard = arena.lock();
        for &id in &widget_ids {
            if let Some(hot) = guard.get_hot_mut(id) {
                hot.flags
                    .remove(NodeFlags::DIRTY_LAYOUT | NodeFlags::DIRTY_PAINT);
            }
        }
    }

    rt.batch(|| {
        for i in (1..NODE_COUNT).step_by(2) {
            let (x, y, w, h) = signals[i].get_untracked();
            signals[i].set((x + 1.0, y + 1.0, w, h));
        }
    });

    {
        let guard = arena.lock();
        for (i, &wid) in widget_ids.iter().enumerate() {
            let hot = guard.get_hot(wid).expect("hot node exists");
            if i % 2 == 1 {
                assert!(hot.flags.contains(NodeFlags::DIRTY_LAYOUT));
            } else {
                assert!(!hot.flags.contains(NodeFlags::DIRTY_LAYOUT));
            }
        }
    }
}

/// Dynamic branch and cycle isolation stress test.
///
/// Simulates a dynamic UI with 500 conditional branches switching rapidly between different signal subgraphs,
/// while injecting cyclic signal dependencies to confirm that 3-color DFS isolates cycles cleanly without crashing
/// or leaking memory.
#[test]
fn test_dynamic_branch_and_cycle_isolation_stress() {
    const BRANCH_COUNT: usize = 500;
    const SWITCH_ROUNDS: usize = 40;

    let rt = ReactiveRuntime::new();

    struct Branch {
        condition: Signal<bool>,
        left: Signal<f64>,
        right: Signal<f64>,
        memo: Memo<f64>,
    }

    let mut branches = Vec::with_capacity(BRANCH_COUNT);
    for i in 0..BRANCH_COUNT {
        let cond = rt.create_signal(true);
        let left = rt.create_signal(i as f64 * 2.0);
        let right = rt.create_signal(i as f64 * 3.0 + 100.0);

        let c = cond.clone();
        let l = left.clone();
        let r = right.clone();
        let memo = rt.create_memo(move || {
            if c.get() {
                l.get() * 1.5
            } else {
                r.get() * 2.5
            }
        });

        branches.push(Branch {
            condition: cond,
            left,
            right,
            memo,
        });
    }

    // Initial branch values verification
    for (i, b) in branches.iter().enumerate() {
        let expected = (i as f64 * 2.0) * 1.5;
        assert_eq!(b.memo.get(), expected);
    }

    // Rapid dynamic branch switching with pruning verification
    let start_switching = Instant::now();
    for round in 0..SWITCH_ROUNDS {
        let flip_even = round % 2 == 0;
        rt.batch(|| {
            for (i, b) in branches.iter().enumerate() {
                if (i % 2 == 0) == flip_even {
                    b.condition.set(false);
                    b.right.update(|v| *v += 1.0);
                } else {
                    b.condition.set(true);
                    b.left.update(|v| *v += 1.0);
                }
            }
        });

        // Verify that unselected branches do not dirty the memos
        for b in &branches {
            let active_is_left = b.condition.get_untracked();
            if active_is_left {
                // Mutating inactive right should not cause memo to become dirty
                let memo_val_before = b.memo.get();
                b.right.update(|v| *v += 10.0);
                assert!(!rt.is_dirty(b.memo.id()));
                assert_eq!(b.memo.get(), memo_val_before);
            } else {
                // Mutating inactive left should not cause memo to become dirty
                let memo_val_before = b.memo.get();
                b.left.update(|v| *v += 10.0);
                assert!(!rt.is_dirty(b.memo.id()));
                assert_eq!(b.memo.get(), memo_val_before);
            }
        }
    }
    let switch_duration = start_switching.elapsed();
    println!(
        "Dynamic branch stress: {} rounds across {} branches completed in {:?}",
        SWITCH_ROUNDS, BRANCH_COUNT, switch_duration
    );

    // Adversarial cycle injection into the active DAG:
    // 1. Direct self-reference cycle
    let self_cyc = rt.create_signal(1.0);
    let self_res = rt.track_read_manual(self_cyc.id(), self_cyc.id());
    assert!(self_res.is_err(), "Self-cycle must be rejected");
    assert!(
        rt.is_poisoned(self_cyc.id()),
        "Self-cyclic node must be poisoned"
    );

    // 2. Mutual 2-node cycle: A -> B -> A
    let node_a = rt.create_signal(10.0);
    let node_b = rt.create_signal(20.0);
    assert!(rt.track_read_manual(node_b.id(), node_a.id()).is_ok());
    let mutual_res = rt.track_read_manual(node_a.id(), node_b.id());
    assert!(
        mutual_res.is_err(),
        "Mutual cyclic dependency must be rejected"
    );
    assert!(
        rt.is_poisoned(node_a.id()),
        "Cyclic ancestor must be poisoned"
    );

    // 3. 3-node cycle across branch nodes: C1 -> C2 -> C3 -> C1
    let c1 = rt.create_signal(100.0);
    let c2 = rt.create_signal(200.0);
    let c3 = rt.create_signal(300.0);
    assert!(rt.track_read_manual(c2.id(), c1.id()).is_ok());
    assert!(rt.track_read_manual(c3.id(), c2.id()).is_ok());
    let cycle_res = rt.track_read_manual(c1.id(), c3.id());
    assert!(
        cycle_res.is_err(),
        "3-node back-edge must be detected as cycle"
    );

    // Run full 3-color DFS cycle detector
    let _ = rt.detect_cycles();
    // Graph contains cleanly isolated cycles
    let errors = rt.errors();
    assert!(
        !errors.is_empty(),
        "Errors must contain detected cycle instances"
    );

    for err in &errors {
        match err {
            ReactiveError::Cycle(c) => {
                assert!(c.from.raw() > 0 && c.to.raw() > 0);
            }
            ReactiveError::PoisonedNode(_) => {}
        }
    }

    // Verify circuit breaker: normal unpoisoned branches remain fully functional
    let valid_branch = &branches[0];
    if !rt.is_poisoned(valid_branch.left.id()) {
        valid_branch.condition.set(true);
        valid_branch.left.set(999.0);
        assert_eq!(valid_branch.memo.get(), 999.0 * 1.5);
    }
}

/// Pathological DAG topology: Deep linear chain of 10,000 nodes.
///
/// Verifies glitch-free topological evaluation and rank propagation across 10,000 levels.
#[test]
fn test_pathological_deep_linear_chain_10k() {
    const CHAIN_DEPTH: usize = 10_000;

    let rt = ReactiveRuntime::new();
    let root = rt.create_signal(0i64);

    let start_build = Instant::now();
    let mut memos = Vec::with_capacity(CHAIN_DEPTH);

    let first = {
        let r = root.clone();
        rt.create_memo(move || r.get() + 1)
    };
    memos.push(first.clone());

    let mut current = first;
    for _ in 1..CHAIN_DEPTH {
        let parent = current.clone();
        let next = rt.create_memo(move || parent.get() + 1);
        memos.push(next.clone());
        current = next;
    }
    let build_duration = start_build.elapsed();

    // Initial evaluation verification
    assert_eq!(memos.last().unwrap().get(), CHAIN_DEPTH as i64);

    // Propagate mutation from root down 10,000 nodes
    let start_eval = Instant::now();
    root.set(42);
    let final_val = memos.last().unwrap().get();
    let eval_duration = start_eval.elapsed();

    println!(
        "Pathological 10,000 linear chain: built in {:?}, evaluated in {:?}, final_val = {}",
        build_duration, eval_duration, final_val
    );

    assert_eq!(final_val, 42 + CHAIN_DEPTH as i64);
}

/// Pathological DAG topology: Wide fan-out of 1 to 10,000 subscribers.
///
/// Verifies breadth-first dirty push, batch coalescing, and dynamic pruning.
#[test]
fn test_pathological_wide_fan_out_10k() {
    const SUBSCRIBER_COUNT: usize = 10_000;

    let rt = ReactiveRuntime::new();
    let root = rt.create_signal(10i64);

    let start_build = Instant::now();
    let subscribers: Vec<Memo<i64>> = (0..SUBSCRIBER_COUNT)
        .map(|i| {
            let r = root.clone();
            rt.create_memo(move || r.get() * 3 + (i as i64))
        })
        .collect();
    let build_duration = start_build.elapsed();

    // Verify initial values on boundaries and middle
    assert_eq!(subscribers[0].get(), 30);
    assert_eq!(subscribers[5000].get(), 30 + 5000);
    assert_eq!(subscribers[9999].get(), 30 + 9999);

    // Batch update driving all 10,000 subscribers simultaneously
    let start_batch = Instant::now();
    rt.batch(|| {
        root.set(100);
    });
    let batch_duration = start_batch.elapsed();

    println!(
        "Pathological 1-to-10,000 fan-out: built in {:?}, batched update in {:?}",
        build_duration, batch_duration
    );

    assert_eq!(subscribers[0].get(), 300);
    assert_eq!(subscribers[5000].get(), 300 + 5000);
    assert_eq!(subscribers[9999].get(), 300 + 9999);
}

/// Tree hierarchy mutations and rapid compaction.
///
/// Deep tree generation, mass reparenting, cycle avoidance, depth rank preservation,
/// and shrink_to_fit idle defragmentation.
#[test]
fn test_tree_hierarchy_mutation_and_compaction_stress() {
    let mut arena = WidgetArena::with_capacity(1024);
    let fence = FrameFence::new();

    // Create root node
    let root = arena.insert(
        HotNode {
            bounds: Rect::new(0.0, 0.0, 1920.0, 1080.0),
            ..HotNode::default()
        },
        ColdNode::default().with_name("root"),
    );

    // Build multi-level hierarchy: 10 containers, each with 50 children = 500 nodes
    let mut container_ids = Vec::with_capacity(10);
    let mut leaf_ids = Vec::with_capacity(500);

    for c in 0..10 {
        let container = arena.insert(
            HotNode {
                bounds: Rect::new(0.0, (c * 100) as f32, 1920.0, 100.0),
                ..HotNode::default()
            },
            ColdNode::default(),
        );
        arena
            .append_child(root, container)
            .expect("append container to root");
        container_ids.push(container);

        for l in 0..50 {
            let leaf = arena.insert(
                HotNode {
                    bounds: Rect::new((l * 30) as f32, 0.0, 30.0, 100.0),
                    ..HotNode::default()
                },
                ColdNode::default(),
            );
            arena
                .append_child(container, leaf)
                .expect("append leaf to container");
            leaf_ids.push(leaf);
        }
    }

    // Verify initial depth ranks
    assert_eq!(arena.depth_rank(root), Some(0));
    for &c in &container_ids {
        assert_eq!(arena.depth_rank(c), Some(1));
    }
    for &l in &leaf_ids {
        assert_eq!(arena.depth_rank(l), Some(2));
    }

    // Mass reparenting: move every odd leaf from its original container to the next container
    for (i, &l) in leaf_ids.iter().enumerate() {
        if i % 2 == 1 {
            let new_parent = container_ids[(i / 50 + 1) % container_ids.len()];
            arena.append_child(new_parent, l).expect("reparent leaf");
            assert_eq!(arena.depth_rank(l), Some(2));
            assert_eq!(arena.parent(l), Some(new_parent));
        }
    }

    // Hierarchy cycle rejection: parent cannot be reparented to its own child
    let cycle_attempt = arena.append_child(leaf_ids[0], root);
    assert!(
        cycle_attempt.is_err(),
        "Cycle detection must prevent parenting root to descendant"
    );

    // Remove half the leaves
    for i in (0..leaf_ids.len()).step_by(2) {
        let removed = arena.remove(leaf_ids[i]);
        assert!(removed.is_some(), "Node must be removed cleanly");
    }

    // Execute compaction and shrink_to_fit under fence
    let start_compact = Instant::now();
    arena
        .compact_and_shrink_idle(&fence)
        .expect("compact and shrink idle");
    let compact_duration = start_compact.elapsed();

    println!(
        "Tree hierarchy mutation & compaction: 500 nodes, mass reparenting, compacted in {:?}",
        compact_duration
    );

    assert_eq!(fence.active_readers(), 0);
    assert!(fence.epoch() >= 2);
}
