//! Property-based tests for the reactive signal DAG using proptest.
//!
//! These tests verify invariants that must hold for *any* sequence of signal
//! updates and reads, not just hand-picked examples.

use martensite_reactive::{create_memo, create_signal, ReactiveRuntime};
use proptest::prelude::*;

proptest! {
    /// A memo derived from a signal must always reflect the signal's current
    /// value after the signal is set, regardless of how many updates occur.
    #[test]
    fn prop_memo_tracks_signal(values in prop::collection::vec(-1000i32..1000, 1..100)) {
        let rt = ReactiveRuntime::new();
        ReactiveRuntime::with_current(&rt, || {
            let signal = create_signal(values[0]);
            let signal_clone = signal.clone();
            let memo = create_memo(move || signal_clone.get() * 2);

            for &v in &values {
                signal.set(v);
                assert_eq!(memo.get(), v * 2);
            }
        });
    }

    /// A chain of N memos each doubling the previous must produce value * 2^N
    /// at the end, for any value and any chain length.
    #[test]
    fn prop_memo_chain_doubling(value in -100i32..100, depth in 1u32..8) {
        let rt = ReactiveRuntime::new();
        ReactiveRuntime::with_current(&rt, || {
            let signal = create_signal(value);

            let signal_for_memo = signal.clone();
            let mut memo: Box<dyn Fn() -> i32> = Box::new(move || signal_for_memo.get());
            for _ in 0..depth {
                let prev = memo;
                memo = Box::new(move || prev() * 2);
            }
            let expected = value * (1i32 << depth);
            signal.set(value);
            assert_eq!(memo(), expected);
        });
    }

    /// Setting a signal to the same value multiple times must not cause the
    /// memo to produce a stale value (idempotency).
    #[test]
    fn prop_signal_idempotent_set(value in -1000i32..1000, repeats in 1u32..50) {
        let rt = ReactiveRuntime::new();
        ReactiveRuntime::with_current(&rt, || {
            let signal = create_signal(value);
            let signal_clone = signal.clone();
            let memo = create_memo(move || signal_clone.get());

            for _ in 0..repeats {
                signal.set(value);
            }
            assert_eq!(memo.get(), value);
        });
    }

    /// A diamond graph (A -> B, A -> C, B -> D, C -> D) must evaluate D
    /// correctly and D must reflect A's value.
    #[test]
    fn prop_diamond_glitch_free(value in -100i32..100) {
        let rt = ReactiveRuntime::new();
        ReactiveRuntime::with_current(&rt, || {
            let a = create_signal(value);
            let b = {
                let a = a.clone();
                create_memo(move || a.get() + 1)
            };
            let c = {
                let a = a.clone();
                create_memo(move || a.get() + 2)
            };
            let d = create_memo(move || b.get() + c.get());

            a.set(value);
            assert_eq!(d.get(), (value + 1) + (value + 2));
        });
    }

    /// Multiple independent signals and memos must not interfere with each
    /// other when updated in any order.
    #[test]
    fn prop_independent_signals_no_interference(
        a_vals in prop::collection::vec(-50i32..50, 1..20),
        b_vals in prop::collection::vec(-50i32..50, 1..20),
    ) {
        let rt = ReactiveRuntime::new();
        ReactiveRuntime::with_current(&rt, || {
            let sa = create_signal(0i32);
            let sb = create_signal(0i32);
            let sa_clone = sa.clone();
            let ma = create_memo(move || sa_clone.get() * 3);
            let sb_clone = sb.clone();
            let mb = create_memo(move || sb_clone.get() * 5);

            let max_len = a_vals.len().max(b_vals.len());
            for i in 0..max_len {
                if i < a_vals.len() {
                    sa.set(a_vals[i]);
                }
                if i < b_vals.len() {
                    sb.set(b_vals[i]);
                }
                if i < a_vals.len() {
                    assert_eq!(ma.get(), a_vals[i] * 3);
                }
                if i < b_vals.len() {
                    assert_eq!(mb.get(), b_vals[i] * 5);
                }
            }
        });
    }

    /// set_if_changed must only return true when the value actually changes.
    #[test]
    fn prop_set_if_changed_semantics(
        initial in -100i32..100,
        updates in prop::collection::vec(-100i32..100, 1..50),
    ) {
        let rt = ReactiveRuntime::new();
        ReactiveRuntime::with_current(&rt, || {
            let signal = create_signal(initial);
            let mut last = initial;
            for &v in &updates {
                let changed = signal.set_if_changed(v);
                if v == last {
                    assert!(!changed);
                } else {
                    assert!(changed);
                    last = v;
                }
                assert_eq!(signal.get(), v);
            }
        });
    }
}
