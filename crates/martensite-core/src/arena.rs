use crate::id::WidgetId;
use crate::node::{HotNode, ColdNode};

#[derive(Copy, Clone)]
pub struct Slot {
    pub generation: u32,
    pub dense_idx: u32,
}

pub struct WidgetArena {
    pub slots: Vec<Slot>,
    pub hot_nodes: Vec<HotNode>,
    pub cold_nodes: Vec<ColdNode>,
    pub dense_to_slot: Vec<u32>,
    pub free_slots: Vec<u32>,
}

impl Default for WidgetArena {
    fn default() -> Self {
        Self::new()
    }
}

impl WidgetArena {
    pub fn new() -> Self {
        Self {
            slots: Vec::with_capacity(256),
            hot_nodes: Vec::with_capacity(256),
            cold_nodes: Vec::with_capacity(256),
            dense_to_slot: Vec::with_capacity(256),
            free_slots: Vec::new(),
        }
    }

    #[inline(always)]
    pub fn is_alive(&self, id: WidgetId) -> bool {
        self.slots.get(id.slot_idx() as usize)
            .is_some_and(|slot| slot.generation == id.generation())
    }


    #[inline(always)]
    pub fn get_hot(&self, id: WidgetId) -> Option<&HotNode> {
        let slot = self.slots.get(id.slot_idx() as usize)?;
        if slot.generation == id.generation() {
            Some(&self.hot_nodes[slot.dense_idx as usize])
        } else {
            None
        }
    }

    pub fn insert(&mut self, hot: HotNode, cold: ColdNode) -> WidgetId {
        let dense_idx = self.hot_nodes.len() as u32;
        self.hot_nodes.push(hot);
        self.cold_nodes.push(cold);

        let slot_idx = if let Some(free_idx) = self.free_slots.pop() {
            let slot = &mut self.slots[free_idx as usize];
            slot.dense_idx = dense_idx;
            free_idx
        } else {
            let idx = self.slots.len() as u32;
            self.slots.push(Slot { generation: 1, dense_idx });
            idx
        };

        self.dense_to_slot.push(slot_idx);
        WidgetId::new(slot_idx, self.slots[slot_idx as usize].generation)
    }

    pub fn remove(&mut self, id: WidgetId) -> Option<(HotNode, ColdNode)> {
        let slot = self.slots.get_mut(id.slot_idx() as usize)?;
        if slot.generation != id.generation() {
            return None;
        }

        // Advance generation skipping 0 to preserve NonZero niche optimization
        slot.generation = if slot.generation == u32::MAX { 1 } else { slot.generation + 1 };
        let removed_dense = slot.dense_idx as usize;
        let last_dense = self.hot_nodes.len() - 1;

        let hot = self.hot_nodes.swap_remove(removed_dense);
        let cold = self.cold_nodes.swap_remove(removed_dense);
        self.dense_to_slot.swap_remove(removed_dense);
        self.free_slots.push(id.slot_idx());

        if removed_dense != last_dense {
            let relocated_slot_idx = self.dense_to_slot[removed_dense] as usize;
            self.slots[relocated_slot_idx].dense_idx = removed_dense as u32;
        }

        Some((hot, cold))
    }

}
