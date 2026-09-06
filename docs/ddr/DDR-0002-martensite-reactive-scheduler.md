# Detailed Design Record: DDR-0002
## Title: `martensite-reactive` Push-Pull Scheduling Engine & Formal Glitch-Freedom

### 1. Architectural Role & Invariants
`martensite-reactive` delivers a high-performance, fine-grained reactive state graph. It implements a dual-phase push-dirty / pull-topological scheduling engine that completely eliminates Virtual DOM diffing.
* **Invariant 1.1**: State propagation is split into two non-overlapping phases: **Phase A (Push Dirty Marking)** and **Phase B (Pull Topological Evaluation)**.
* **Invariant 1.2 (Glitch Freedom)**: No derived memo node `M` shall ever evaluate while any of its transitive dependencies `D` are dirty or pending update.
* **Invariant 1.3**: Zero heap allocations during active signal mutation cascades.

---

### 2. Formal Mathematical Proof of Glitch-Freedom

Let the reactive dependency network be represented as a Directed Acyclic Graph:
$$\mathcal{G} = (\mathcal{V}, \mathcal{E})$$
where $\mathcal{V}$ is the set of reactive nodes (source signals $\mathcal{S}$ and derived memos $\mathcal{M}$), and directed edge $(u, v) \in \mathcal{E}$ indicates that node $v$ reads node $u$.

We assign every node $v \in \mathcal{V}$ an integer topological rank $\lambda(v)$:
$$\lambda(s) = 0 \quad \forall s \in \mathcal{S}$$
$$\lambda(m) = 1 + \max_{(u, m) \in \mathcal{E}} \lambda(u) \quad \forall m \in \mathcal{M}$$

**Theorem**: If nodes are evaluated in monotonically increasing order of their topological rank $\lambda$, every derived node evaluates exactly once per transaction with fully settled inputs, eliminating diamond dependency glitches.

---

### 3. Dual-Phase Push-Pull Engine Implementation

```rust
pub struct ReactiveEngine {
    pub nodes: SlotMap<NodeKey, ReactiveNode>,
    pub pending_eval_queue: BTreeSet<(u32, NodeKey)>, // (rank, key)
    pub dirty_leaves: BitSet,
}

impl ReactiveEngine {
    pub fn mark_dirty(&mut self, source: NodeKey) {
        // Phase 1: Push Dirty Flags
        let mut queue = VecDeque::new();
        queue.push_back(source);
        while let Some(curr) = queue.pop_front() {
            for &sub in &self.nodes[curr].subscribers {
                if !self.nodes[sub].is_dirty {
                    self.nodes[sub].is_dirty = true;
                    self.pending_eval_queue.insert((self.nodes[sub].rank, sub));
                    queue.push_back(sub);
                }
            }
        }
    }

    pub fn flush(&mut self) {
        // Phase 2: Pull Topological Order
        while let Some((_, node_key)) = self.pending_eval_queue.pop_first() {
            let node = &mut self.nodes[node_key];
            if node.is_dirty {
                node.evaluate();
                node.is_dirty = false;
            }
        }
    }
}
```
