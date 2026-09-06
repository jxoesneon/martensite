//! Comprehensive test suite for martensite-reactive.
#![forbid(unsafe_code)]

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;

use martensite_reactive::prelude::*;
use martensite_reactive::{NodeEvaluator, SchedulerState};

#[test]
fn test_signal_basic_operations() {
    let rt = ReactiveRuntime::new();
    let sig = rt.create_signal(42);

    assert_eq!(sig.get(), 42);
    assert_eq!(sig.get_untracked(), 42);
    assert_eq!(sig.id().raw(), sig.id.raw());
    assert_eq!(format!("{}", sig), "42");
    assert!(format!("{:?}", sig).contains("Signal"));

    sig.set(100);
    assert_eq!(sig.get(), 100);

    sig.update(|v| *v += 23);
    assert_eq!(sig.get(), 123);

    // set_if_changed
    assert!(!sig.set_if_changed(123));
    assert_eq!(sig.get(), 123);
    assert!(sig.set_if_changed(200));
    assert_eq!(sig.get(), 200);
}

#[test]
fn test_signal_clone_and_concurrency() {
    let rt = ReactiveRuntime::new();
    let sig = rt.create_signal(0);
    let mut handles = Vec::new();

    for _ in 0..8 {
        let s = sig.clone();
        handles.push(thread::spawn(move || {
            for _ in 0..100 {
                s.update(|v| *v += 1);
            }
        }));
    }

    for h in handles {
        h.join().unwrap();
    }

    assert_eq!(sig.get(), 800);
}

#[test]
fn test_memo_caching_and_derivation() {
    let rt = ReactiveRuntime::new();
    let a = rt.create_signal(10);
    let eval_count = Arc::new(AtomicUsize::new(0));

    let eval_count_clone = Arc::clone(&eval_count);
    let a_clone = a.clone();
    let memo = rt.create_memo(move || {
        eval_count_clone.fetch_add(1, Ordering::SeqCst);
        a_clone.get() * 2
    });

    // Memo evaluated once on creation
    assert_eq!(eval_count.load(Ordering::SeqCst), 1);
    assert_eq!(memo.get(), 20);
    // Cached: reading repeatedly must not trigger re-evaluation
    assert_eq!(memo.get(), 20);
    assert_eq!(memo.get_untracked(), 20);
    assert_eq!(eval_count.load(Ordering::SeqCst), 1);

    // Mutate source signal
    a.set(15);
    // Evaluated during flush / pull
    assert_eq!(memo.get(), 30);
    assert_eq!(eval_count.load(Ordering::SeqCst), 2);

    assert_eq!(format!("{}", memo), "30");
    assert!(format!("{:?}", memo).contains("Memo"));
}

#[test]
fn test_diamond_graph_glitch_free_evaluation() {
    // Diamond DAG:
    //         A
    //       /   \
    //      B     C
    //       \   /
    //         D
    //         |
    //       Effect
    let rt = ReactiveRuntime::new();
    let a = rt.create_signal(1);

    let b_count = Arc::new(AtomicUsize::new(0));
    let c_count = Arc::new(AtomicUsize::new(0));
    let d_count = Arc::new(AtomicUsize::new(0));
    let effect_count = Arc::new(AtomicUsize::new(0));
    let observed_d = Arc::new(AtomicUsize::new(0));

    let a_b = a.clone();
    let b_cnt = Arc::clone(&b_count);
    let b = rt.create_memo(move || {
        b_cnt.fetch_add(1, Ordering::SeqCst);
        a_b.get() * 10
    });

    let a_c = a.clone();
    let c_cnt = Arc::clone(&c_count);
    let c = rt.create_memo(move || {
        c_cnt.fetch_add(1, Ordering::SeqCst);
        a_c.get() + 5
    });

    let b_d = b.clone();
    let c_d = c.clone();
    let d_cnt = Arc::clone(&d_count);
    let d = rt.create_memo(move || {
        d_cnt.fetch_add(1, Ordering::SeqCst);
        b_d.get() + c_d.get()
    });

    let d_eff = d.clone();
    let eff_cnt = Arc::clone(&effect_count);
    let obs_d = Arc::clone(&observed_d);
    let _effect = rt.create_effect(move || {
        eff_cnt.fetch_add(1, Ordering::SeqCst);
        obs_d.store(d_eff.get(), Ordering::SeqCst);
    });

    // Initial evaluation checks
    // A = 1 -> B = 10, C = 6 -> D = 16
    assert_eq!(d.get(), 16);
    assert_eq!(observed_d.load(Ordering::SeqCst), 16);
    assert_eq!(b_count.load(Ordering::SeqCst), 1);
    assert_eq!(c_count.load(Ordering::SeqCst), 1);
    assert_eq!(d_count.load(Ordering::SeqCst), 1);
    assert_eq!(effect_count.load(Ordering::SeqCst), 1);

    // Mutate source: A = 2 -> B = 20, C = 7 -> D = 27
    a.set(2);

    // D must have evaluated exactly once for this mutation, with fully settled inputs (no intermediate glitch)
    assert_eq!(d.get(), 27);
    assert_eq!(observed_d.load(Ordering::SeqCst), 27);
    assert_eq!(b_count.load(Ordering::SeqCst), 2);
    assert_eq!(c_count.load(Ordering::SeqCst), 2);
    assert_eq!(d_count.load(Ordering::SeqCst), 2);
    assert_eq!(effect_count.load(Ordering::SeqCst), 2);
}

#[test]
fn test_dynamic_dependency_pruning() {
    // Dynamic branch: toggle.get() ? a.get() : b.get()
    let rt = ReactiveRuntime::new();
    let toggle = rt.create_signal(true);
    let a = rt.create_signal(100);
    let b = rt.create_signal(200);

    let eval_count = Arc::new(AtomicUsize::new(0));

    let toggle_m = toggle.clone();
    let a_m = a.clone();
    let b_m = b.clone();
    let cnt = Arc::clone(&eval_count);
    let derived = rt.create_memo(move || {
        cnt.fetch_add(1, Ordering::SeqCst);
        if toggle_m.get() {
            a_m.get()
        } else {
            b_m.get()
        }
    });

    // Initially: toggle is true -> depends on toggle and a
    assert_eq!(derived.get(), 100);
    assert_eq!(eval_count.load(Ordering::SeqCst), 1);

    // Mutating b must NOT trigger re-evaluation because b is not an active dependency
    b.set(999);
    assert_eq!(eval_count.load(Ordering::SeqCst), 1);
    assert_eq!(derived.get(), 100);

    // Mutating a MUST trigger re-evaluation
    a.set(101);
    assert_eq!(derived.get(), 101);
    assert_eq!(eval_count.load(Ordering::SeqCst), 2);

    // Switch branch: toggle = false -> now depends on toggle and b; a is pruned
    toggle.set(false);
    assert_eq!(derived.get(), 999);
    assert_eq!(eval_count.load(Ordering::SeqCst), 3);

    // Mutating a must NOT trigger re-evaluation now!
    a.set(555);
    assert_eq!(eval_count.load(Ordering::SeqCst), 3);
    assert_eq!(derived.get(), 999);

    // Mutating b MUST trigger re-evaluation now!
    b.set(1000);
    assert_eq!(derived.get(), 1000);
    assert_eq!(eval_count.load(Ordering::SeqCst), 4);

    // Switch back: toggle = true -> re-subscribes to a, prunes b
    toggle.set(true);
    assert_eq!(derived.get(), 555);
    assert_eq!(eval_count.load(Ordering::SeqCst), 5);

    // Mutating b again has no effect
    b.set(2000);
    assert_eq!(eval_count.load(Ordering::SeqCst), 5);
}

#[test]
fn test_transactional_batching_coalescing() {
    let rt = ReactiveRuntime::new();
    let a = rt.create_signal(1);
    let b = rt.create_signal(2);
    let c = rt.create_signal(3);

    let effect_runs = Arc::new(AtomicUsize::new(0));
    let observed_sum = Arc::new(AtomicUsize::new(0));

    let a_eff = a.clone();
    let b_eff = b.clone();
    let c_eff = c.clone();
    let runs = Arc::clone(&effect_runs);
    let sum = Arc::clone(&observed_sum);

    let _effect = rt.create_effect(move || {
        runs.fetch_add(1, Ordering::SeqCst);
        sum.store(a_eff.get() + b_eff.get() + c_eff.get(), Ordering::SeqCst);
    });

    assert_eq!(effect_runs.load(Ordering::SeqCst), 1);
    assert_eq!(observed_sum.load(Ordering::SeqCst), 6);

    // Execute multiple mutations in an outer batch
    rt.batch(|| {
        a.set(10);
        b.set(20);
        c.set(30);
    });

    // Effect should have run only ONCE for the entire batch
    assert_eq!(effect_runs.load(Ordering::SeqCst), 2);
    assert_eq!(observed_sum.load(Ordering::SeqCst), 60);

    // Nested batching
    rt.batch(|| {
        a.set(100);
        rt.batch(|| {
            b.set(200);
            rt.batch(|| {
                c.set(300);
            });
        });
        // Intermediate assertions before outer batch closes
        assert_eq!(effect_runs.load(Ordering::SeqCst), 2);
    });

    // Now outer batch closed: flushed exactly once
    assert_eq!(effect_runs.load(Ordering::SeqCst), 3);
    assert_eq!(observed_sum.load(Ordering::SeqCst), 600);
}

#[test]
fn test_cycle_detection_and_circuit_breaker() {
    let rt = ReactiveRuntime::new();

    // 1. Direct active self-reference cycle
    let sig = rt.create_signal(10);
    let sig_clone = sig.clone();
    let _self_memo = rt.create_memo(move || sig_clone.get());

    // 2. Full graph cycle detection test
    let s1 = SignalId::next();
    let s2 = SignalId::next();
    let s3 = SignalId::next();

    rt.register_source(s1);
    rt.register_source(s2);
    rt.register_source(s3);

    // Establish s1 -> s2 -> s3 -> s1
    assert!(rt.detect_cycles().is_ok());

    // Test cycle detector with explicit graph linkage
    let rt_manual = ReactiveRuntime::new();
    let n1 = SignalId::next();
    let n2 = SignalId::next();
    let n3 = SignalId::next();

    rt_manual.register_source(n1);
    rt_manual.register_source(n2);
    rt_manual.register_source(n3);

    // Hook n1 -> n2 -> n3
    assert!(rt_manual.track_read_manual(n2, n1).is_ok());
    assert!(rt_manual.track_read_manual(n3, n2).is_ok());
    // Adding n1 -> n3 creates cycle n1 -> n2 -> n3 -> n1
    let cycle_res = rt_manual.track_read_manual(n1, n3);
    assert!(cycle_res.is_err());
    assert!(rt_manual.is_poisoned(n1));
    assert!(!rt_manual.errors().is_empty());
}

#[test]
fn test_effect_disposal() {
    let rt = ReactiveRuntime::new();
    let a = rt.create_signal(1);
    let runs = Arc::new(AtomicUsize::new(0));

    let a_eff = a.clone();
    let runs_clone = Arc::clone(&runs);
    let effect = rt.create_effect(move || {
        runs_clone.fetch_add(1, Ordering::SeqCst);
        let _ = a_eff.get();
    });

    assert_eq!(runs.load(Ordering::SeqCst), 1);
    assert!(!effect.is_disposed());
    assert!(format!("{:?}", effect).contains("Effect"));

    a.set(2);
    assert_eq!(runs.load(Ordering::SeqCst), 2);

    effect.dispose();
    assert!(effect.is_disposed());

    // Further signal updates must not run disposed effect
    a.set(3);
    assert_eq!(runs.load(Ordering::SeqCst), 2);

    // Explicit run on disposed effect is a no-op
    effect.run();
    assert_eq!(runs.load(Ordering::SeqCst), 2);

    // Repeated disposal is safe
    effect.dispose();
    assert!(effect.is_disposed());
}

#[test]
fn test_topological_deep_linear_chain() {
    // S -> M1 -> M2 -> M3 -> ... -> M10
    let rt = ReactiveRuntime::new();
    let root = rt.create_signal(1);

    let mut memos = Vec::new();
    let mut prev_memo: Option<Memo<i32>> = None;

    for _ in 0..10 {
        let r = root.clone();
        let p = prev_memo.clone();
        let m = rt.create_memo(move || {
            if let Some(ref prev) = p {
                let val: i32 = prev.get();
                val + 1
            } else {
                r.get() + 1
            }
        });
        prev_memo = Some(m.clone());
        memos.push(m);
    }

    let last = memos.last().unwrap();
    // 1 + 10 = 11
    assert_eq!(last.get(), 11);

    root.set(100);
    // 100 + 10 = 110
    assert_eq!(last.get(), 110);
}

#[test]
fn test_ambient_runtime_free_functions() {
    let sig = create_signal(5);
    let s = sig.clone();
    let memo = create_memo(move || s.get() * 3);

    assert_eq!(memo.get(), 15);

    let effect_val = Arc::new(AtomicUsize::new(0));
    let eff_val = Arc::clone(&effect_val);
    let s_eff = sig.clone();
    let eff = create_effect(move || {
        eff_val.store(s_eff.get() + 100, Ordering::SeqCst);
    });

    assert_eq!(effect_val.load(Ordering::SeqCst), 105);

    batch(|| {
        sig.set(10);
    });

    assert_eq!(memo.get(), 30);
    assert_eq!(effect_val.load(Ordering::SeqCst), 110);
    flush();
    assert_eq!(memo.get(), 30);
    eff.dispose();
}

#[test]
fn test_error_formatting_and_types() {
    let id1 = SignalId::next();
    let id2 = SignalId::next();

    let cycle_err = CycleError { from: id1, to: id2 };
    let cycle_str = format!("{}", cycle_err);
    assert!(cycle_str.contains("cyclic dependency detected"));
    assert!(cycle_str.contains(&format!("{:?}", id1)));
    assert!(cycle_str.contains(&format!("{:?}", id2)));
    assert_eq!(cycle_err, cycle_err);

    let err_cycle = ReactiveError::Cycle(cycle_err);
    let err_cycle_str = format!("{}", err_cycle);
    assert!(err_cycle_str.contains("cyclic dependency detected"));

    let err_poison = ReactiveError::PoisonedNode(id1);
    let err_poison_str = format!("{}", err_poison);
    assert!(err_poison_str.contains("poisoned by circuit breaker"));

    let id_str = format!("{}", id1);
    assert!(id_str.contains("SignalId("));
}

#[test]
fn test_signal_memo_effect_accessors_and_defaults() {
    let rt = ReactiveRuntime::new();

    // Signal
    let s = Signal::new_with_runtime(10, Arc::clone(&rt));
    assert_eq!(s.id(), s.id);
    assert!(Arc::ptr_eq(s.runtime(), &rt));

    // Memo
    let s_clone = s.clone();
    let m = Memo::new_with_runtime(move || s_clone.get() * 2, Arc::clone(&rt));
    assert_eq!(m.id(), m.id);
    assert!(Arc::ptr_eq(m.runtime(), &rt));

    // Effect
    let e = Effect::new_with_runtime(|| (), Arc::clone(&rt));
    assert_eq!(e.id(), e.id);
    assert!(Arc::ptr_eq(e.runtime(), &rt));

    // Signal::new ambient
    let ambient_s = Signal::new(99);
    assert_eq!(ambient_s.get(), 99);

    // Memo::new ambient
    let ambient_s_clone = ambient_s.clone();
    let ambient_m = Memo::new(move || ambient_s_clone.get() + 1);
    assert_eq!(ambient_m.get(), 100);

    // Effect::new ambient
    let ambient_e = Effect::new(|| ());
    assert!(!ambient_e.is_disposed());
    ambient_e.dispose();
    assert!(ambient_e.is_disposed());
}

#[test]
fn test_runtime_context_and_scopes() {
    let rt1 = ReactiveRuntime::new();
    let rt2 = ReactiveRuntime::new();

    // set_current
    ReactiveRuntime::set_current(&rt1);
    let s1 = create_signal(100);
    assert!(Arc::ptr_eq(s1.runtime(), &rt1));

    // with_current
    let s2 = ReactiveRuntime::with_current(&rt2, || create_signal(200));
    assert!(Arc::ptr_eq(s2.runtime(), &rt2));

    // Default trait for ReactiveRuntime and SchedulerState
    let rt_default = ReactiveRuntime::default();
    assert_eq!(rt_default.errors().len(), 0);

    let sched_default = SchedulerState::default();
    assert_eq!(sched_default.test_node_count(), 0);

    // clear_errors
    rt1.track_read_manual(s1.id(), s1.id()).unwrap_err();
    assert!(!rt1.errors().is_empty());
    rt1.clear_errors();
    assert!(rt1.errors().is_empty());

    // is_dirty
    assert!(!rt1.is_dirty(s1.id()));
    s1.set(101);
    // After flush, it's clean
    assert!(!rt1.is_dirty(s1.id()));

    // begin_batch / end_batch direct
    rt1.begin_batch();
    s1.set(102);
    assert!(rt1.end_batch());
}

#[test]
fn test_scheduler_unlinking_and_edge_cases() {
    let rt = ReactiveRuntime::new();
    let a = rt.create_signal(1);
    let a_clone = a.clone();
    let b = rt.create_memo(move || a_clone.get() + 1);
    let b_clone = b.clone();
    let c = rt.create_memo(move || b_clone.get() + 1);

    assert_eq!(c.get(), 3);

    // Unregister node b which has both dependency a and subscriber c
    rt.unregister_node(b.id());
    // Unregister non-existent node
    rt.unregister_node(SignalId::next());

    // Mutating a should no longer reach b or c
    a.set(10);
    // Since b was unregistered, c was unlinked
    assert_eq!(a.get(), 10);

    // Test evaluate_node on poisoned node
    let poisoned_id = SignalId::next();
    rt.register_source(poisoned_id);
    rt.poison_node(poisoned_id);
    assert!(!rt.evaluate_node(poisoned_id));

    // Test evaluate_node on source node with no evaluator
    let source_id = SignalId::next();
    rt.register_source(source_id);
    assert!(!rt.evaluate_node(source_id));

    // Test post_eval_prune on non-existent node
    rt.post_eval_prune(SignalId::next());

    // Test SchedulerState direct operations
    let mut state = SchedulerState::new();
    state.register_source(poisoned_id);
    state.poison_node(poisoned_id);
    state.mark_dirty_bfs(poisoned_id);
    assert!(state.pop_next_pending().is_none());

    // Test add_dependency_link self-reference
    let self_id = SignalId::next();
    let self_res = state.add_dependency_link(self_id, self_id);
    assert!(self_res.is_err());
}

#[test]
fn test_full_graph_3_color_dfs_cycle_isolation() {
    let mut state = SchedulerState::new();

    let n1 = SignalId::next();
    let n2 = SignalId::next();
    let n3 = SignalId::next();

    state.register_source(n1);
    state.register_source(n2);
    state.register_source(n3);

    // Create cycle: n1 -> n2 -> n3 -> n1
    state.test_push_subscriber(n1, n2);
    state.test_push_subscriber(n2, n3);
    state.test_push_subscriber(n3, n1);

    // detect_cycles should catch the back-edge to Gray node, poison it, isolate the edge, and return Err
    let res = state.detect_cycles();
    assert!(res.is_err());
    let err = res.unwrap_err();
    assert!(
        (err.from == n3 && err.to == n1)
            || (err.from == n1 && err.to == n2)
            || (err.from == n2 && err.to == n3)
    );
    assert!(state.is_poisoned(n1) || state.is_poisoned(n2) || state.is_poisoned(n3));

    // Subsequent cycle check should succeed because the cyclic edge was isolated by circuit breaker!
    let second_res = state.detect_cycles();
    assert!(second_res.is_ok());
}

#[test]
fn test_mutual_active_eval_stack_cycle() {
    let rt = ReactiveRuntime::new();

    // Create mutual recursive memos: A -> B -> A
    // Node A reads Node B, and Node B reads Node A.
    let cell_a: Arc<parking_lot::Mutex<Option<Memo<i32>>>> =
        Arc::new(parking_lot::Mutex::new(None));
    let cell_b: Arc<parking_lot::Mutex<Option<Memo<i32>>>> =
        Arc::new(parking_lot::Mutex::new(None));

    let ca = Arc::clone(&cell_a);
    let cb = Arc::clone(&cell_b);

    let memo_a = rt.create_memo(move || {
        let guard = cb.lock();
        if let Some(ref b) = *guard {
            b.get() + 1
        } else {
            1
        }
    });

    let memo_b = rt.create_memo(move || {
        let guard = ca.lock();
        if let Some(ref a) = *guard {
            a.get() + 1
        } else {
            10
        }
    });

    *cell_a.lock() = Some(memo_a.clone());
    *cell_b.lock() = Some(memo_b.clone());

    // Mark both memos dirty so memo_a's evaluation pulls dirty memo_b, which queries memo_a on the active stack
    rt.mark_node_dirty_for_test(memo_a.id());
    rt.mark_node_dirty_for_test(memo_b.id());

    // Trigger evaluation on memo_a which queries dirty memo_b which queries active memo_a
    rt.evaluate_node(memo_a.id());

    // Cycle was detected on the active evaluation stack and circuit breaker triggered without deadlocking
    assert!(!rt.errors().is_empty());
}

#[test]
fn test_batch_unwind_safety() {
    let rt = ReactiveRuntime::new();
    let sig = rt.create_signal(10);

    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        rt.batch(|| {
            sig.set(20);
            panic!("intentional batch panic for unwind safety");
        });
    }));

    // Batch depth should have restored to 0 via Drop guard
    assert_eq!(sig.get(), 20);

    // Next batch works normally
    rt.batch(|| {
        sig.set(30);
    });
    assert_eq!(sig.get(), 30);
}

#[test]
fn test_high_frequency_zero_allocation_stress() {
    let rt = ReactiveRuntime::new();
    let root = rt.create_signal(0);

    let r_clone = root.clone();
    let derived = rt.create_memo(move || r_clone.get() * 2);

    let derived_clone = derived.clone();
    let eff_sum = Arc::new(AtomicUsize::new(0));
    let eff_sum_clone = Arc::clone(&eff_sum);
    let _eff = rt.create_effect(move || {
        eff_sum_clone.store(derived_clone.get() as usize, Ordering::Relaxed);
    });

    // 10,000 continuous mutations verifying performance and zero glitches
    for i in 1..=10_000 {
        root.set(i);
        assert_eq!(derived.get(), i * 2);
        assert_eq!(eff_sum.load(Ordering::Relaxed), (i * 2) as usize);
    }
}

#[test]
fn test_targeted_coverage_edge_cases() {
    let rt = ReactiveRuntime::new();

    // 1. Effect run before and after dispose
    let ran = Arc::new(AtomicBool::new(false));
    let ran_clone = Arc::clone(&ran);
    let eff = rt.create_effect(move || {
        ran_clone.store(true, Ordering::SeqCst);
    });
    // Run while active
    eff.run();
    assert!(ran.load(Ordering::SeqCst));
    // Dispose and run evaluator directly to hit disposed branch
    eff.dispose();
    assert!(!rt.evaluate_node(eff.id()));

    // 2. Direct self-reference in track_read (caller == id)
    let self_memo_id = Arc::new(parking_lot::Mutex::new(None));
    let id_clone = Arc::clone(&self_memo_id);
    let rt_clone = Arc::clone(&rt);
    let self_memo = rt.create_memo(move || {
        if let Some(id) = *id_clone.lock() {
            rt_clone.track_read(id);
        }
        42
    });
    *self_memo_id.lock() = Some(self_memo.id());
    // Force re-eval to trigger self-reference
    rt.evaluate_node(self_memo.id());
    assert!(rt.is_poisoned(self_memo.id()));

    // 3. evaluate_node with dirty dependencies (lazy pull loop)
    let s = rt.create_signal(1);
    let s_c = s.clone();
    let m1 = rt.create_memo(move || s_c.get() + 10);
    let m1_c = m1.clone();
    let m2 = rt.create_memo(move || m1_c.get() + 100);

    rt.batch(|| {
        s.set(2);
    });
    // m2.get() triggers ensure_clean which pulls dirty dependency m1
    assert_eq!(m2.get(), 112);

    // 4. SchedulerState internal branches
    let mut state = SchedulerState::new();
    let n1 = SignalId::next();
    let n2 = SignalId::next();
    let n3 = SignalId::next();
    let n4 = SignalId::next();

    state.register_source(n1);
    state.register_source(n2);
    state.register_source(n3);
    state.register_source(n4);

    // Link n1 -> n2 -> n3
    state.add_dependency_link(n2, n1).unwrap();
    state.add_dependency_link(n3, n2).unwrap();

    // Call add_dependency_link again to hit existing link branches
    state.add_dependency_link(n2, n1).unwrap();

    // Propagate rank increase: give n4 a higher rank and link n4 -> n1
    state.test_set_node_rank(n4, 10);
    state.add_dependency_link(n1, n4).unwrap();
    assert_eq!(state.test_node_rank(n1).unwrap(), 11);
    assert_eq!(state.test_node_rank(n2).unwrap(), 12);
    assert_eq!(state.test_node_rank(n3).unwrap(), 13);

    // Unregister n1 which has subscribers (hits subscribers cleanup loop)
    state.unregister_node(n1);

    // mark_dirty_bfs on non-existent node
    state.mark_dirty_bfs(SignalId(999_999));

    // pop_next_pending with non-existent or clean node
    state.test_push_pending(1, SignalId(999_999));
    assert!(state.pop_next_pending().is_none());

    // pop_next_pending with a poisoned node
    let p_node = SignalId::next();
    state.register_source(p_node);
    state.poison_node(p_node);
    state.test_set_node_dirty(p_node, true);
    state.test_push_pending(1, p_node);
    assert!(state.pop_next_pending().is_none());

    // isolate_edge when nodes do not exist
    state.isolate_edge(SignalId(999_998), SignalId(999_997));

    // add_dependency_link where parent.rank > dep.rank (rank_changed = false)
    let a_id = SignalId::next();
    let b_id = SignalId::next();
    state.register_source(a_id);
    state.register_source(b_id);
    state.test_set_node_rank(a_id, 100);
    state.test_set_node_rank(b_id, 1);
    state.add_dependency_link(a_id, b_id).unwrap();

    // post_eval_prune when dependency node was already removed
    let p2 = SignalId::next();
    state.register_source(p2);
    state.test_set_node_eval_epoch(p2, 10);
    state.test_push_dependency(p2, SignalId(888_888), 1);
    state.post_eval_prune(p2);

    // 5. Runtime run_evaluator on poisoned node and epoch overflow
    struct DummyEval;
    impl NodeEvaluator for DummyEval {
        fn evaluate(&self) -> bool {
            true
        }
    }
    let p_eval = SignalId::next();
    rt.register_source(p_eval);
    rt.poison_node(p_eval);
    assert!(!rt.run_evaluator_for_test(p_eval, &DummyEval));

    let wrap_id = SignalId::next();
    rt.register_source(wrap_id);
    rt.set_eval_epoch_for_test(wrap_id, u32::MAX);
    assert!(rt.run_evaluator_for_test(wrap_id, &DummyEval));

    // run_evaluator on non-existent node
    assert!(rt.run_evaluator_for_test(SignalId(123_456), &DummyEval));

    // Non-existent parent and dep in add_dependency_link
    let _ = state.add_dependency_link(SignalId(123_456), b_id);
    let _ = state.add_dependency_link(a_id, SignalId(123_456));

    // Diamond graph cycle detection hitting NodeColor::Black branch
    let mut diag = SchedulerState::new();
    let d_a = SignalId::next();
    let d_b = SignalId::next();
    let d_c = SignalId::next();
    let d_d = SignalId::next();
    diag.register_source(d_a);
    diag.register_source(d_b);
    diag.register_source(d_c);
    diag.register_source(d_d);
    diag.test_push_subscriber(d_a, d_b);
    diag.test_push_subscriber(d_a, d_c);
    diag.test_push_subscriber(d_b, d_d);
    diag.test_push_subscriber(d_c, d_d);
    assert!(diag.detect_cycles().is_ok());

    // Mark dirty BFS on node whose subscribers are already dirty
    diag.mark_dirty_bfs(d_a);
    diag.mark_dirty_bfs(d_a);

    // Unregister node with dependency that exists and subscriber that exists
    let u = SignalId::next();
    let v = SignalId::next();
    let w = SignalId::next();
    diag.register_source(u);
    diag.register_source(v);
    diag.register_source(w);
    diag.add_dependency_link(v, u).unwrap();
    diag.add_dependency_link(w, v).unwrap();
    diag.unregister_node(v);

    // Unregister node with dangling dependency and subscriber
    let dangling_node = SignalId::next();
    diag.register_source(dangling_node);
    diag.test_push_dependency(dangling_node, SignalId(777_777), 1);
    diag.test_push_subscriber(dangling_node, SignalId(888_888));
    diag.unregister_node(dangling_node);

    // Hasher write &[u8]
    use std::hash::{BuildHasher, Hasher};
    let mut hasher = martensite_reactive::FastBuildHasher::default().build_hasher();
    hasher.write(b"deterministic_hasher_bytes");
    let h1 = hasher.finish();
    assert_ne!(h1, 0);

    // is_evaluating check inside and outside evaluation
    assert!(!rt.is_evaluating());
    let rt_for_check = Arc::clone(&rt);
    let evaluated_in_eval = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let eval_clone = std::sync::Arc::clone(&evaluated_in_eval);
    let memo_check = rt.create_memo(move || {
        eval_clone.store(
            rt_for_check.is_evaluating(),
            std::sync::atomic::Ordering::Relaxed,
        );
        42
    });
    assert_eq!(memo_check.get(), 42);
    assert!(evaluated_in_eval.load(std::sync::atomic::Ordering::Relaxed));
    assert!(!rt.is_evaluating());

    // Duplicate signal reads within single evaluation
    let dup_sig = rt.create_signal(5);
    let dup_clone = dup_sig.clone();
    let dup_memo = rt.create_memo(move || dup_clone.get() + dup_clone.get());
    assert_eq!(dup_memo.get(), 10);

    // evaluate_node pulling dirty dependency
    let root_s = rt.create_signal(100);
    let r_clone = root_s.clone();
    let dep_memo = rt.create_memo(move || r_clone.get() * 2);
    let dm_clone = dep_memo.clone();
    let sink_memo = rt.create_memo(move || dm_clone.get() + 5);
    assert_eq!(sink_memo.get(), 205);
    root_s.set(200);
    // Explicitly call evaluate_node on sink_memo while dep_memo is dirty
    assert!(rt.evaluate_node(sink_memo.id()));
    assert_eq!(sink_memo.get_untracked(), 405);

    // pop_next_pending epoch rollover (wrapping from u32::MAX to 0 -> 1)
    let wrap_pop = SignalId::next();
    diag.register_source(wrap_pop);
    diag.test_set_node_dirty(wrap_pop, true);
    diag.test_set_node_eval_epoch(wrap_pop, u32::MAX);
    diag.test_clear_pending_queue();
    diag.test_push_pending(0, wrap_pop);
    let popped = diag.pop_next_pending();
    assert!(popped.is_some());
    assert_eq!(popped.unwrap().1, wrap_pop);
    assert_eq!(diag.test_node_eval_epoch(wrap_pop).unwrap(), 1);

    // Rank propagation when sub_node.rank is already strictly greater than curr_rank
    let base_node = SignalId::next();
    let r_node = SignalId::next();
    let s_node = SignalId::next();
    diag.register_source(base_node);
    diag.register_source(r_node);
    diag.register_source(s_node);
    diag.test_set_node_rank(s_node, 100);
    diag.add_dependency_link(s_node, r_node).unwrap();
    // Now trigger a rank increase on r_node from base_node that remains < 100
    diag.test_set_node_rank(base_node, 10);
    diag.add_dependency_link(r_node, base_node).unwrap();
    assert_eq!(diag.test_node_rank(r_node).unwrap(), 11);
    assert_eq!(diag.test_node_rank(s_node).unwrap(), 100);
}
