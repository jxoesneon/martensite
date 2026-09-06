//! Two-phase push-pull reactive DAG scheduler.
#![forbid(unsafe_code)]

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, VecDeque};
use std::sync::Arc;

use smallvec::SmallVec;

use crate::cycle::{CycleError, NodeColor};
use crate::runtime::{NodeEvaluator, ReactiveError};
use crate::signal::SignalId;

/// Thread-safe handle to a dynamic node evaluator.
pub type EvaluatorHandle = Arc<dyn NodeEvaluator + Send + Sync>;

/// Next pending evaluation task tuple `(rank, id, evaluator)`.
pub type PendingEvaluation = (u32, SignalId, Option<EvaluatorHandle>);

/// Internal representation of a reactive node within the scheduler DAG.
pub struct NodeRecord {
    /// Unique signal identifier.
    pub id: SignalId,
    /// Topological depth rank ($\lambda(v)$).
    pub rank: u32,
    /// Indicates whether the node requires re-evaluation.
    pub is_dirty: bool,
    /// Indicates whether the node has been poisoned by the circuit breaker.
    pub is_poisoned: bool,
    /// Current evaluation epoch for dynamic dependency pruning.
    pub eval_epoch: u32,
    /// Color state used for 3-color DFS cycle detection.
    pub color: NodeColor,
    /// Downstream subscriber nodes that read from this node.
    pub subscribers: SmallVec<[SignalId; 4]>,
    /// Upstream dependency nodes read by this node paired with the last observed `eval_epoch`.
    pub dependencies: SmallVec<[(SignalId, u32); 4]>,
    /// Optional dynamic evaluator trait object.
    pub evaluator: Option<EvaluatorHandle>,
    /// Optional thread-safe atomic dirty flag mirror for lock-free reader checks.
    pub dirty_flag: Option<Arc<std::sync::atomic::AtomicBool>>,
}

#[derive(Default, Clone, Copy)]
pub struct FastHasher(u64);

impl std::hash::Hasher for FastHasher {
    #[inline(always)]
    fn finish(&self) -> u64 {
        self.0
    }
    #[inline(always)]
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0.rotate_left(5) ^ (b as u64)).wrapping_mul(0x517cc1b727220a95);
        }
    }
    #[inline(always)]
    fn write_u64(&mut self, i: u64) {
        self.0 = (self.0.rotate_left(5) ^ i).wrapping_mul(0x517cc1b727220a95);
    }
}

pub type FastBuildHasher = std::hash::BuildHasherDefault<FastHasher>;

/// Internal scheduler state managing DAG topologies, dirty bitsets, and evaluation priority queues.
pub struct SchedulerState {
    /// Node map indexing all reactive records.
    pub(crate) nodes: HashMap<SignalId, NodeRecord, FastBuildHasher>,
    /// Phase 2 priority queue sorted strictly by ascending topological depth rank.
    pub(crate) pending_eval_queue: BinaryHeap<Reverse<(u32, SignalId)>>,
    /// Pre-allocated reusable queue for Phase 1 breadth-first dirty marking.
    pub(crate) push_queue: VecDeque<SignalId>,
    /// Pre-allocated reusable queue for topological rank propagation.
    pub(crate) rank_queue: VecDeque<SignalId>,
    /// Cumulative log of runtime and cycle errors.
    pub(crate) errors: Vec<ReactiveError>,
}

impl Default for SchedulerState {
    fn default() -> Self {
        Self::new()
    }
}

impl SchedulerState {
    /// Creates a new, empty `SchedulerState` with pre-allocated queue capacities.
    pub fn new() -> Self {
        Self {
            nodes: HashMap::with_capacity_and_hasher(128, FastBuildHasher::default()),
            pending_eval_queue: BinaryHeap::with_capacity(128),
            push_queue: VecDeque::with_capacity(64),
            rank_queue: VecDeque::with_capacity(64),
            errors: Vec::new(),
        }
    }

    /// Registers a source state signal node with rank 0.
    pub fn register_source(&mut self, id: SignalId) {
        self.nodes.insert(
            id,
            NodeRecord {
                id,
                rank: 0,
                is_dirty: false,
                is_poisoned: false,
                eval_epoch: 0,
                color: NodeColor::White,
                subscribers: SmallVec::new(),
                dependencies: SmallVec::new(),
                evaluator: None,
                dirty_flag: None,
            },
        );
    }

    /// Registers a derived memo or effect node with an attached evaluator.
    pub fn register_derived(
        &mut self,
        id: SignalId,
        evaluator: Arc<dyn NodeEvaluator + Send + Sync>,
    ) {
        self.register_derived_with_flag(id, evaluator, None);
    }

    /// Registers a derived memo or effect node with an attached evaluator and atomic dirty flag mirror.
    pub fn register_derived_with_flag(
        &mut self,
        id: SignalId,
        evaluator: Arc<dyn NodeEvaluator + Send + Sync>,
        dirty_flag: Option<Arc<std::sync::atomic::AtomicBool>>,
    ) {
        self.nodes.insert(
            id,
            NodeRecord {
                id,
                rank: 1,
                is_dirty: true,
                is_poisoned: false,
                eval_epoch: 0,
                color: NodeColor::White,
                subscribers: SmallVec::new(),
                dependencies: SmallVec::new(),
                evaluator: Some(evaluator),
                dirty_flag,
            },
        );
    }

    /// Unregisters a node and unlinks all inbound and outbound edges.
    pub fn unregister_node(&mut self, id: SignalId) {
        if let Some(node) = self.nodes.remove(&id) {
            for &(dep_id, _) in &node.dependencies {
                if let Some(dep_node) = self.nodes.get_mut(&dep_id) {
                    dep_node.subscribers.retain(|s| *s != id);
                }
            }
            for sub_id in node.subscribers {
                if let Some(sub_node) = self.nodes.get_mut(&sub_id) {
                    sub_node.dependencies.retain(|(d, _)| *d != id);
                }
            }
        }
    }

    /// Returns `true` if the node is flagged as poisoned.
    pub fn is_poisoned(&self, id: SignalId) -> bool {
        self.nodes.get(&id).is_some_and(|n| n.is_poisoned)
    }

    /// Marks a node as poisoned by the circuit breaker.
    pub fn poison_node(&mut self, id: SignalId) {
        if let Some(node) = self.nodes.get_mut(&id) {
            node.is_poisoned = true;
        }
    }

    /// Isolates a cyclic edge between two nodes to prevent infinite traversal.
    pub fn isolate_edge(&mut self, from: SignalId, to: SignalId) {
        if let Some(from_node) = self.nodes.get_mut(&from) {
            from_node.subscribers.retain(|s| *s != to);
        }
        if let Some(to_node) = self.nodes.get_mut(&to) {
            to_node.dependencies.retain(|(d, _)| *d != from);
        }
    }

    /// Phase 1: Push dirty bitset flags across downstream subscribers using BFS.
    pub fn mark_dirty_bfs(&mut self, source: SignalId) {
        if self.is_poisoned(source) {
            return;
        }

        self.push_queue.clear();
        self.push_queue.push_back(source);

        while let Some(curr) = self.push_queue.pop_front() {
            let subscribers = self
                .nodes
                .get(&curr)
                .map(|n| n.subscribers.clone())
                .unwrap_or_default();

            for sub_id in subscribers {
                if let Some(sub_node) = self.nodes.get_mut(&sub_id) {
                    if !sub_node.is_poisoned && !sub_node.is_dirty {
                        sub_node.is_dirty = true;
                        if let Some(ref flag) = sub_node.dirty_flag {
                            flag.store(true, std::sync::atomic::Ordering::Release);
                        }
                        let rank = sub_node.rank;
                        self.pending_eval_queue.push(Reverse((rank, sub_id)));
                        self.push_queue.push_back(sub_id);
                    }
                }
            }
        }
    }

    /// Phase 2: Pops the pending dirty node with the lowest topological depth rank.
    pub fn pop_next_pending(&mut self) -> Option<PendingEvaluation> {
        while let Some(Reverse((rank, id))) = self.pending_eval_queue.pop() {
            if let Some(node) = self.nodes.get_mut(&id) {
                if node.is_dirty && !node.is_poisoned {
                    node.eval_epoch = node.eval_epoch.wrapping_add(1);
                    if node.eval_epoch == 0 {
                        node.eval_epoch = 1;
                    }
                    return Some((rank, id, node.evaluator.clone()));
                }
            }
        }
        None
    }

    /// Registers a dependency edge from `dep` to `parent` ($dep \to parent$).
    ///
    /// Verifies acyclicity using 3-color reachability, updates `last_epoch`, and adjusts topological depth ranks.
    pub fn add_dependency_link(
        &mut self,
        parent: SignalId,
        dep: SignalId,
    ) -> Result<(), CycleError> {
        if parent == dep {
            self.poison_node(parent);
            let err = CycleError {
                from: dep,
                to: parent,
            };
            self.errors.push(ReactiveError::Cycle(err));
            return Err(err);
        }

        let parent_epoch = self.nodes.get(&parent).map(|n| n.eval_epoch).unwrap_or(0);

        // Fast path: if edge already exists, update epoch for dynamic pruning and return
        if let Some(parent_node) = self.nodes.get_mut(&parent) {
            if let Some(link) = parent_node.dependencies.iter_mut().find(|(d, _)| *d == dep) {
                link.1 = parent_epoch;
                return Ok(());
            }
        }

        // Verify that adding dep -> parent does not form a cycle (i.e. parent cannot already reach dep)
        if self.has_path(parent, dep) {
            self.poison_node(parent);
            self.isolate_edge(dep, parent);
            let err = CycleError {
                from: dep,
                to: parent,
            };
            self.errors.push(ReactiveError::Cycle(err));
            return Err(err);
        }

        let dep_rank = self.nodes.get(&dep).map(|n| n.rank).unwrap_or(0);

        // Insert new dependency link on parent
        if let Some(parent_node) = self.nodes.get_mut(&parent) {
            parent_node.dependencies.push((dep, parent_epoch));
        }

        // Add parent to dep subscribers if not present
        if let Some(dep_node) = self.nodes.get_mut(&dep) {
            if !dep_node.subscribers.contains(&parent) {
                dep_node.subscribers.push(parent);
            }
        }

        // Maintain topological rank invariant: rank(parent) >= rank(dep) + 1
        let mut rank_changed = false;
        if let Some(parent_node) = self.nodes.get_mut(&parent) {
            if parent_node.rank <= dep_rank {
                parent_node.rank = dep_rank + 1;
                rank_changed = true;
            }
        }

        if rank_changed {
            self.propagate_rank_increase(parent);
        }

        Ok(())
    }

    /// Breadth-first propagation of topological depth rank increases downstream.
    fn propagate_rank_increase(&mut self, start: SignalId) {
        self.rank_queue.clear();
        self.rank_queue.push_back(start);

        while let Some(curr) = self.rank_queue.pop_front() {
            let curr_rank = self.nodes.get(&curr).map(|n| n.rank).unwrap_or(0);
            let subscribers = self
                .nodes
                .get(&curr)
                .map(|n| n.subscribers.clone())
                .unwrap_or_default();

            for sub_id in subscribers {
                if let Some(sub_node) = self.nodes.get_mut(&sub_id) {
                    if sub_node.rank <= curr_rank {
                        sub_node.rank = curr_rank + 1;
                        self.rank_queue.push_back(sub_id);
                    }
                }
            }
        }
    }

    /// Dynamic Dependency Pruning: Unlinks all dependencies whose `last_epoch < node.eval_epoch`.
    pub fn post_eval_prune(&mut self, node_id: SignalId) {
        let Some(node) = self.nodes.get_mut(&node_id) else {
            return;
        };
        let current_epoch = node.eval_epoch;
        if !node
            .dependencies
            .iter()
            .any(|(_, epoch)| *epoch < current_epoch)
        {
            return;
        }
        let mut stale = SmallVec::<[SignalId; 4]>::new();

        node.dependencies.retain(|(dep_id, last_epoch)| {
            if *last_epoch < current_epoch {
                stale.push(*dep_id);
                false
            } else {
                true
            }
        });

        for dep_id in stale {
            if let Some(dep_node) = self.nodes.get_mut(&dep_id) {
                dep_node.subscribers.retain(|sub| *sub != node_id);
            }
        }
    }

    /// Checks if a directed path exists from `from` to `target` using 3-color DFS traversal.
    fn has_path(&mut self, from: SignalId, target: SignalId) -> bool {
        for node in self.nodes.values_mut() {
            node.color = NodeColor::White;
        }
        self.dfs_reaches(from, target)
    }

    fn dfs_reaches(&mut self, current: SignalId, target: SignalId) -> bool {
        if current == target {
            return true;
        }
        if let Some(node) = self.nodes.get_mut(&current) {
            node.color = NodeColor::Gray;
        }
        let subscribers = self
            .nodes
            .get(&current)
            .map(|n| n.subscribers.clone())
            .unwrap_or_default();
        for sub in subscribers {
            let color = self
                .nodes
                .get(&sub)
                .map(|n| n.color)
                .unwrap_or(NodeColor::White);
            if color == NodeColor::White && self.dfs_reaches(sub, target) {
                return true;
            }
        }
        if let Some(node) = self.nodes.get_mut(&current) {
            node.color = NodeColor::Black;
        }
        false
    }

    /// Executes full 3-color DFS cycle verification across all graph nodes.
    pub fn detect_cycles(&mut self) -> Result<(), CycleError> {
        for node in self.nodes.values_mut() {
            node.color = NodeColor::White;
        }
        let node_ids: Vec<SignalId> = self.nodes.keys().copied().collect();
        for id in node_ids {
            if self.nodes.get(&id).map(|n| n.color) == Some(NodeColor::White) {
                self.dfs_cycle(id)?;
            }
        }
        Ok(())
    }

    fn dfs_cycle(&mut self, current: SignalId) -> Result<(), CycleError> {
        if let Some(node) = self.nodes.get_mut(&current) {
            node.color = NodeColor::Gray;
        }
        let subscribers = self
            .nodes
            .get(&current)
            .map(|n| n.subscribers.clone())
            .unwrap_or_default();
        for sub in subscribers {
            let sub_color = self
                .nodes
                .get(&sub)
                .map(|n| n.color)
                .unwrap_or(NodeColor::White);
            match sub_color {
                NodeColor::Gray => {
                    self.poison_node(sub);
                    self.isolate_edge(current, sub);
                    let err = CycleError {
                        from: current,
                        to: sub,
                    };
                    self.errors.push(ReactiveError::Cycle(err));
                    return Err(err);
                }
                NodeColor::White => {
                    self.dfs_cycle(sub)?;
                }
                NodeColor::Black => {}
            }
        }
        if let Some(node) = self.nodes.get_mut(&current) {
            node.color = NodeColor::Black;
        }
        Ok(())
    }

    // --- Test support helpers (do not use in production code) ---

    /// Returns the number of registered node records.
    #[doc(hidden)]
    pub fn test_node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Returns `true` if a node record exists for `id`.
    #[doc(hidden)]
    pub fn test_node_exists(&self, id: SignalId) -> bool {
        self.nodes.contains_key(&id)
    }

    /// Returns the topological rank of `id`, if it exists.
    #[doc(hidden)]
    pub fn test_node_rank(&self, id: SignalId) -> Option<u32> {
        self.nodes.get(&id).map(|n| n.rank)
    }

    /// Sets the topological rank of `id`.
    #[doc(hidden)]
    pub fn test_set_node_rank(&mut self, id: SignalId, rank: u32) {
        if let Some(node) = self.nodes.get_mut(&id) {
            node.rank = rank;
        }
    }

    /// Returns the eval epoch of `id`, if it exists.
    #[doc(hidden)]
    pub fn test_node_eval_epoch(&self, id: SignalId) -> Option<u32> {
        self.nodes.get(&id).map(|n| n.eval_epoch)
    }

    /// Sets the dirty flag of `id`.
    #[doc(hidden)]
    pub fn test_set_node_dirty(&mut self, id: SignalId, dirty: bool) {
        if let Some(node) = self.nodes.get_mut(&id) {
            node.is_dirty = dirty;
        }
    }

    /// Sets the eval epoch of `id`.
    #[doc(hidden)]
    pub fn test_set_node_eval_epoch(&mut self, id: SignalId, epoch: u32) {
        if let Some(node) = self.nodes.get_mut(&id) {
            node.eval_epoch = epoch;
        }
    }

    /// Adds `subscriber` to `id`'s subscriber list without validation.
    #[doc(hidden)]
    pub fn test_push_subscriber(&mut self, id: SignalId, subscriber: SignalId) {
        if let Some(node) = self.nodes.get_mut(&id) {
            if !node.subscribers.contains(&subscriber) {
                node.subscribers.push(subscriber);
            }
        }
    }

    /// Adds `dependency` to `id`'s dependency list without validation.
    #[doc(hidden)]
    pub fn test_push_dependency(&mut self, id: SignalId, dependency: SignalId, epoch: u32) {
        if let Some(node) = self.nodes.get_mut(&id) {
            if !node.dependencies.iter().any(|(d, _)| *d == dependency) {
                node.dependencies.push((dependency, epoch));
            }
        }
    }

    /// Clears the pending evaluation queue.
    #[doc(hidden)]
    pub fn test_clear_pending_queue(&mut self) {
        self.pending_eval_queue.clear();
    }

    /// Pushes a pending evaluation entry onto the queue.
    #[doc(hidden)]
    pub fn test_push_pending(&mut self, rank: u32, id: SignalId) {
        self.pending_eval_queue.push(Reverse((rank, id)));
    }
}
