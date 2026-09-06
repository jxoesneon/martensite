//! Property-based tests for the generational arena using proptest.
//!
//! These tests verify invariants that must hold for *any* sequence of arena
//! operations, not just hand-picked examples.

use martensite_core::{ColdNode, HotNode, WidgetArena, WidgetId};
use proptest::prelude::*;

/// A single arena operation used to drive the property tests.
#[derive(Debug, Clone, Copy)]
enum Op {
    Insert,
    Remove(u32),
    Get(u32),
}

/// Generates a bounded sequence of arena operations.
fn arb_ops(max_slots: u32) -> impl Strategy<Value = Vec<Op>> {
    prop::collection::vec(
        prop_oneof![
            Just(Op::Insert),
            (0u32..max_slots).prop_map(Op::Remove),
            (0u32..max_slots).prop_map(Op::Get),
        ],
        0..256,
    )
}

fn make_default_node() -> (HotNode, ColdNode) {
    (HotNode::default(), ColdNode::default())
}

proptest! {
    /// Inserting N nodes must always produce N live dense entries.
    /// Every allocated id must dereference.
    #[test]
    fn prop_insert_count_matches_live(n in 1u32..500) {
        let mut arena = WidgetArena::new();
        let mut ids = Vec::new();
        for _ in 0..n {
            let (hot, cold) = make_default_node();
            let id = arena.insert(hot, cold);
            ids.push(id);
        }
        prop_assert_eq!(arena.len(), n as usize);
        for id in &ids {
            prop_assert!(arena.get_hot(*id).is_some());
        }
    }

    /// After any sequence of insert/remove/get operations, the number of live
    /// nodes must equal the number of successful inserts minus successful
    /// removes, and every live id must dereference.
    #[test]
    fn prop_arena_invariants_hold(ops in arb_ops(64)) {
        let mut arena = WidgetArena::new();
        let mut live_ids: Vec<WidgetId> = Vec::new();

        for op in ops {
            match op {
                Op::Insert => {
                    let (hot, cold) = make_default_node();
                    let id = arena.insert(hot, cold);
                    live_ids.push(id);
                }
                Op::Remove(idx) => {
                    if !live_ids.is_empty() {
                        let i = (idx as usize) % live_ids.len();
                        let id = live_ids.swap_remove(i);
                        arena.remove(id);
                    }
                }
                Op::Get(idx) => {
                    if !live_ids.is_empty() {
                        let i = (idx as usize) % live_ids.len();
                        let id = live_ids[i];
                        prop_assert!(arena.get_hot(id).is_some());
                    }
                }
            }
        }
        prop_assert_eq!(arena.len(), live_ids.len());
    }

    /// Removing the same id twice must be a no-op (second remove returns None
    /// or is otherwise harmless).
    #[test]
    fn prop_double_remove_is_safe(n in 1u32..100) {
        let mut arena = WidgetArena::new();
        let mut ids = Vec::new();
        for _ in 0..n {
            let (hot, cold) = make_default_node();
            ids.push(arena.insert(hot, cold));
        }
        // Remove all
        for id in &ids {
            arena.remove(*id);
        }
        // Remove all again — must not panic
        for id in &ids {
            arena.remove(*id);
        }
        prop_assert_eq!(arena.len(), 0);
    }

    /// WidgetId round-trips through to_le_bytes / from_le_bytes.
    #[test]
    fn prop_widget_id_roundtrip(slot in 1u32..(1 << 20), gen in 1u32..u32::MAX) {
        let id = WidgetId::new(slot, gen).expect("generation != 0");
        let bytes = id.to_le_bytes();
        let restored = WidgetId::from_le_bytes(bytes).expect("roundtrip preserves validity");
        prop_assert_eq!(id, restored);
        prop_assert_eq!(restored.slot_idx(), slot);
        prop_assert_eq!(restored.generation(), gen);
    }

    /// Generation zero is always rejected by WidgetId::new.
    #[test]
    fn prop_generation_zero_rejected(slot in 1u32..(1 << 20)) {
        prop_assert!(WidgetId::new(slot, 0).is_none());
    }

    /// WidgetId round-trips through to_u64 / from_u64.
    #[test]
    fn prop_widget_id_u64_roundtrip(slot in 1u32..(1 << 20), gen in 1u32..u32::MAX) {
        let id = WidgetId::new(slot, gen).expect("generation != 0");
        let val = id.to_u64();
        let restored = WidgetId::from_u64(val).expect("roundtrip preserves validity");
        prop_assert_eq!(id, restored);
    }

    /// is_alive returns true for live ids and false for removed ids.
    #[test]
    fn prop_is_alive_consistency(n in 1u32..100) {
        let mut arena = WidgetArena::new();
        let mut ids = Vec::new();
        for _ in 0..n {
            let (hot, cold) = make_default_node();
            ids.push(arena.insert(hot, cold));
        }
        for id in &ids {
            prop_assert!(arena.is_alive(*id));
        }
        // Remove half
        let half = n / 2;
        for i in 0..half {
            arena.remove(ids[i as usize]);
            prop_assert!(!arena.is_alive(ids[i as usize]));
        }
        // Remaining must still be alive
        for i in half..n {
            prop_assert!(arena.is_alive(ids[i as usize]));
        }
        prop_assert_eq!(arena.len(), (n - half) as usize);
    }
}
