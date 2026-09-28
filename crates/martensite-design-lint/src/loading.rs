//! Loading-state observation — the temporal probe behind the
//! `loading-stuck` rule.
//!
//! `lint` evaluates one [`LintScene`] at a time and rules are
//! stateless `&'static` objects, so the "skeleton visible across N
//! sampled frames" persistence signal (ADR-0040 §5 — *stuck loading
//! is lint policy, not runtime timers*) cannot live inside a rule.
//! [`LoadingTracker`] holds it instead: drive it once per sampled
//! frame, then [`annotate`](LoadingTracker::annotate) stamps the
//! counts into the scene as `@loading:N` markers before `lint` runs.
//!
//! Two sampling seams cover the two places loading truth lives:
//!
//! - [`sample_arena`](LoadingTracker::sample_arena) reads the arena's
//!   resolved loading state — [`WidgetArena::node_loading`], the
//!   `NodeFlags::LOADING` override OR'd with `Widget::is_loading` —
//!   keyed by `WidgetId`. This is the live-tree path and mirrors
//!   `audit_underflow`'s arena-level sampling.
//! - [`sample_scene`](LoadingTracker::sample_scene) counts scope
//!   names already carrying a `@loading` marker, keyed by scene
//!   path — the offline path for dumped paint lists where no arena
//!   survives (devtools `LintDump` replays, golden fixtures).
//!
//! The marker is the same `Name@marker` lineage convention
//! [`LintScene::from_paint_list`](crate::LintScene::from_paint_list)
//! parses: a bare `@loading` on a scope name is one sighting,
//! `@loading:N` is N consecutive sightings. Producers that emit the
//! marker natively (a paint walker stamping `@loading` on loading
//! scopes) need no tracker for detection — only for the count.
//!
//! Nodes that resolve (or leave the tree) reset to zero on the next
//! sample — `loading-stuck` measures *consecutive* persistence.
//! Widget-internal loading children paint with `PushScope` `id:
//! None`, so arena sampling cannot see them; their counts only
//! accumulate through `sample_scene` when the emitting stream marks
//! them.

use std::collections::{HashMap, HashSet};

use martensite_core::WidgetArena;

use crate::scene::{LintNode, LintScene};

/// Consecutive-sample counts of nodes in the loading state — feed
/// for the `loading-stuck` rule, which reads the stamped
/// `loading:N` markers.
///
/// Rules are stateless by contract; this tracker is the sweep-side
/// memory the persistence check needs. One instance per audited
/// tree: arenas are rebuilt per page in headless sweeps, so a
/// tracker shared across rebuilt arenas sees fresh `WidgetId`s and
/// never accumulates — scope a tracker to a stable tree.
///
/// # Examples
///
/// ```
/// use martensite_core::{ColdNode, DummyWidget, HotNode, NodeFlags, Rect, WidgetArena};
/// use martensite_design_lint::LoadingTracker;
///
/// let mut arena = WidgetArena::new();
/// let mut hot = HotNode::default();
/// hot.flags |= NodeFlags::VISIBLE;
/// hot.bounds = Rect::new(0.0, 0.0, 100.0, 40.0);
/// let id = arena.insert(hot, ColdNode::new(Box::new(DummyWidget)));
///
/// let mut tracker = LoadingTracker::new();
/// arena.set_loading(id, true);
/// tracker.sample_arena(&arena);
/// tracker.sample_arena(&arena);
/// assert!(!tracker.is_empty());
/// ```
#[derive(Debug, Default)]
pub struct LoadingTracker {
    /// Consecutive loading sightings per sampled identity — `Id` for
    /// arena-sampled nodes, `Path` for scene-sampled scopes. Entries
    /// not observed loading in a domain's latest sample are dropped,
    /// so a resolved node restarts from zero.
    counts: HashMap<Key, u32>,
}

/// Identity a sample is counted under — arena `WidgetId` when the
/// tree is live, scene path when only the distilled scope tree
/// survives.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Key {
    /// `WidgetId::to_u64` — arena-sampled.
    Id(u64),
    /// `LintNode::path` — scene-sampled (`@loading` marker sightings).
    Path(String),
}

impl LoadingTracker {
    /// An empty tracker — no samples yet.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_design_lint::LoadingTracker;
    ///
    /// let tracker = LoadingTracker::new();
    /// assert!(tracker.is_empty());
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record one sampled frame against the live arena — each
    /// [`WidgetArena::node_loading`] node's consecutive count ticks
    /// up; nodes that resolved since the last sample reset.
    ///
    /// This is the audit tick the ADR's "N sampled frames" measures:
    /// call it once per swept/painted frame, in lock-step with the
    /// `PaintList` fed to `lint`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::{ColdNode, DummyWidget, HotNode, NodeFlags, Rect, WidgetArena};
    /// use martensite_design_lint::LoadingTracker;
    ///
    /// let mut arena = WidgetArena::new();
    /// let mut hot = HotNode::default();
    /// hot.flags |= NodeFlags::VISIBLE;
    /// hot.bounds = Rect::new(0.0, 0.0, 100.0, 40.0);
    /// let id = arena.insert(hot, ColdNode::new(Box::new(DummyWidget)));
    ///
    /// let mut tracker = LoadingTracker::new();
    /// arena.set_loading(id, true);
    /// tracker.sample_arena(&arena);
    /// assert_eq!(tracker.consecutive(id), 1);
    /// arena.set_loading(id, false);
    /// tracker.sample_arena(&arena);
    /// assert_eq!(tracker.consecutive(id), 0); // resolved → reset
    /// ```
    pub fn sample_arena(&mut self, arena: &WidgetArena) {
        let mut seen = HashSet::new();
        for id in arena.iter_depth_first() {
            if arena.node_loading(id) {
                seen.insert(id.to_u64());
            }
        }
        // Arena-sampled identity domain only — scene-sampled `Path`
        // entries belong to `sample_scene` and are left alone.
        self.counts
            .retain(|k, _| !matches!(k, Key::Id(id) if !seen.contains(id)));
        for raw in seen {
            *self.counts.entry(Key::Id(raw)).or_insert(0) += 1;
        }
    }

    /// Record one sampled frame from a scene — scopes carrying a
    /// `loading` marker (`Foo@loading`, `Foo@loading:2`) count as
    /// loading this sample, keyed by scene path.
    ///
    /// The offline counterpart of
    /// [`sample_arena`](Self::sample_arena): dumped paint lists and
    /// golden fixtures have no arena, so persistence is measured over
    /// markers the producer emitted. Re-sampling a scene this tracker
    /// already [`annotate`](Self::annotate)d counts the stamped
    /// `loading:N` marker again — sample first, annotate last, once
    /// per frame.
    ///
    /// Same-name siblings share a path, so their counts merge — the
    /// same first-wins identity convention `LintScene::by_path`
    /// accepts.
    ///
    /// # Examples
    ///
    /// ```
    /// use kurbo::Rect;
    /// use martensite_core::PaintList;
    /// use martensite_design_lint::{LintScene, LoadingTracker};
    ///
    /// let mut list = PaintList::new();
    /// list.push_scope(None, "Feed@loading", Rect::new(0.0, 0.0, 100.0, 40.0));
    /// list.pop_scope();
    /// let scene = LintScene::from_paint_list(&list);
    ///
    /// let mut tracker = LoadingTracker::new();
    /// tracker.sample_scene(&scene);
    /// tracker.sample_scene(&scene);
    /// assert!(!tracker.is_empty());
    /// ```
    pub fn sample_scene(&mut self, scene: &LintScene) {
        let mut seen = HashSet::new();
        for n in scene.walk() {
            if n.marker_value("loading").is_some() {
                seen.insert(n.path.clone());
            }
        }
        self.counts
            .retain(|k, _| !matches!(k, Key::Path(p) if !seen.contains(p)));
        for path in seen {
            *self.counts.entry(Key::Path(path)).or_insert(0) += 1;
        }
    }

    /// Stamp `loading:N` markers onto `scene`'s nodes — the join that
    /// carries consecutive-sample counts into the frame `lint`
    /// evaluates. Nodes are matched by `widget_id` for arena samples
    /// and by path for scene samples; a tracker holding no count for
    /// a node leaves its markers untouched, so producer-emitted
    /// `@loading`/`@loading:N` names pass through.
    ///
    /// Existing `loading`/`loading:N` markers on a counted node are
    /// replaced — the tracker's accumulated count is the truth, not
    /// a stale name stamp.
    ///
    /// Call after the last sampler for the frame, before `lint`:
    ///
    /// ```
    /// use martensite_core::{
    ///     ColdNode, DummyWidget, HotNode, NodeFlags, PaintList, Rect, WidgetArena,
    /// };
    /// use martensite_design_lint::{lint, LintConfig, LintScene, LoadingTracker};
    ///
    /// let mut arena = WidgetArena::new();
    /// let mut hot = HotNode::default();
    /// hot.flags |= NodeFlags::VISIBLE;
    /// hot.bounds = Rect::new(0.0, 0.0, 100.0, 40.0);
    /// let root = arena.insert(hot, ColdNode::new(Box::new(DummyWidget)));
    /// arena.set_loading(root, true);
    ///
    /// let mut tracker = LoadingTracker::new();
    /// let mut scene = LintScene::default();
    /// for _ in 0..3 {
    ///     let mut list = PaintList::new();
    ///     arena.build_paint_list(root, &mut list);
    ///     tracker.sample_arena(&arena);
    ///     scene = LintScene::from_paint_list(&list);
    ///     tracker.annotate(&mut scene);
    /// }
    /// let report = lint(&scene, &LintConfig::new());
    /// assert!(report.findings.iter().any(|f| f.rule == "loading-stuck"));
    /// ```
    pub fn annotate(&self, scene: &mut LintScene) {
        for root in &mut scene.roots {
            self.annotate_node(root);
        }
    }

    /// Consecutive samples `id` has been observed loading — `0` when
    /// resolved or never seen. Arena-sampled counts only.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::{ColdNode, DummyWidget, HotNode, WidgetArena, WidgetId};
    /// use martensite_design_lint::LoadingTracker;
    ///
    /// let arena = WidgetArena::new();
    /// let tracker = LoadingTracker::new();
    /// assert_eq!(tracker.consecutive(WidgetId::from_u64(1).unwrap()), 0);
    /// ```
    #[must_use]
    pub fn consecutive(&self, id: martensite_core::WidgetId) -> u32 {
        self.counts.get(&Key::Id(id.to_u64())).copied().unwrap_or(0)
    }

    /// Clear every count — e.g. after a theme/route change rebuilt
    /// the tree under the same arena.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_design_lint::LoadingTracker;
    ///
    /// let mut tracker = LoadingTracker::new();
    /// tracker.reset();
    /// assert!(tracker.is_empty());
    /// ```
    pub fn reset(&mut self) {
        self.counts.clear();
    }

    /// Whether any node is being counted.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_design_lint::LoadingTracker;
    ///
    /// assert!(LoadingTracker::new().is_empty());
    /// ```
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.counts.is_empty()
    }

    fn annotate_node(&self, node: &mut LintNode) {
        let count = node
            .widget_id
            .and_then(|raw| self.counts.get(&Key::Id(raw)))
            .or_else(|| self.counts.get(&Key::Path(node.path.clone())))
            .copied();
        if let Some(n) = count {
            node.markers
                .retain(|m| m != "loading" && !m.starts_with("loading:"));
            node.markers.push(format!("loading:{n}"));
        }
        for c in &mut node.children {
            self.annotate_node(c);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kurbo::Rect as KRect;
    use martensite_core::{
        ColdNode, DummyWidget, HotNode, NodeFlags, PaintList, Rect, WidgetArena,
    };

    /// Arena with one visible, bounded `DummyWidget` root.
    fn widget_arena() -> (WidgetArena, martensite_core::WidgetId) {
        let mut arena = WidgetArena::new();
        let mut hot = HotNode::default();
        hot.flags |= NodeFlags::VISIBLE;
        hot.bounds = Rect::new(0.0, 0.0, 120.0, 40.0);
        let root = arena.insert(hot, ColdNode::new(Box::new(DummyWidget)));
        (arena, root)
    }

    #[test]
    fn arena_samples_accumulate_and_reset() {
        let (mut arena, root) = widget_arena();
        let mut tracker = LoadingTracker::new();
        tracker.sample_arena(&arena);
        assert_eq!(tracker.consecutive(root), 0, "not loading yet");

        arena.set_loading(root, true);
        tracker.sample_arena(&arena);
        tracker.sample_arena(&arena);
        assert_eq!(tracker.consecutive(root), 2);

        arena.set_loading(root, false);
        tracker.sample_arena(&arena);
        assert_eq!(tracker.consecutive(root), 0, "resolve resets the count");
    }

    #[test]
    fn removed_node_drops_its_count() {
        let (mut arena, root) = widget_arena();
        let mut tracker = LoadingTracker::new();
        arena.set_loading(root, true);
        tracker.sample_arena(&arena);
        assert_eq!(tracker.consecutive(root), 1);
        arena.remove(root);
        tracker.sample_arena(&arena);
        assert!(tracker.is_empty(), "dropped node keeps no count");
    }

    #[test]
    fn annotate_stamps_count_on_widget_id_join() {
        let (mut arena, root) = widget_arena();
        arena.set_loading(root, true);
        let mut tracker = LoadingTracker::new();
        for _ in 0..4 {
            tracker.sample_arena(&arena);
        }
        let mut list = PaintList::new();
        arena.build_paint_list(root, &mut list);
        let mut scene = LintScene::from_paint_list(&list);
        tracker.annotate(&mut scene);
        let node = scene
            .walk()
            .find(|n| n.widget_id == Some(root.to_u64()))
            .expect("arena scope emitted with widget id");
        assert_eq!(node.marker_value("loading"), Some("4"));
    }

    #[test]
    fn scene_samples_keyed_by_path() {
        let mut list = PaintList::new();
        list.push_scope(None, "Feed@loading", KRect::new(0.0, 0.0, 100.0, 40.0));
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let mut tracker = LoadingTracker::new();
        tracker.sample_scene(&scene);
        tracker.sample_scene(&scene);
        // Re-annotate stamps the accumulated count onto the path.
        let mut scene = LintScene::from_paint_list(&list);
        tracker.annotate(&mut scene);
        let node = scene.node("Feed").expect("scope");
        assert_eq!(node.marker_value("loading"), Some("2"));
    }

    #[test]
    fn annotate_replaces_stale_name_stamp() {
        // A scope name carrying `@loading:1` gets the tracker's real
        // accumulated count, not the stale name value.
        let mut list = PaintList::new();
        list.push_scope(None, "Feed@loading:1", KRect::new(0.0, 0.0, 100.0, 40.0));
        list.pop_scope();
        let scene = LintScene::from_paint_list(&list);
        let mut tracker = LoadingTracker::new();
        tracker.sample_scene(&scene);
        tracker.sample_scene(&scene);
        tracker.sample_scene(&scene);
        let mut scene = LintScene::from_paint_list(&list);
        tracker.annotate(&mut scene);
        assert_eq!(
            scene.node("Feed").unwrap().marker_value("loading"),
            Some("3")
        );
    }

    #[test]
    fn domains_do_not_interfere() {
        // Arena-sampled Id counts and scene-sampled Path counts share
        // the tracker without resetting each other's domain.
        let (mut arena, root) = widget_arena();
        arena.set_loading(root, true);
        let mut marker_list = PaintList::new();
        marker_list.push_scope(None, "Feed@loading", KRect::new(0.0, 0.0, 100.0, 40.0));
        marker_list.pop_scope();
        let scene = LintScene::from_paint_list(&marker_list);

        let mut tracker = LoadingTracker::new();
        tracker.sample_arena(&arena);
        tracker.sample_scene(&scene);
        // Arena-side reset when the arena node resolves must not drop
        // the path-sampled entry.
        arena.set_loading(root, false);
        tracker.sample_arena(&arena);
        assert_eq!(tracker.consecutive(root), 0);
        let mut scene = LintScene::from_paint_list(&marker_list);
        tracker.annotate(&mut scene);
        assert_eq!(
            scene.node("Feed").unwrap().marker_value("loading"),
            Some("1")
        );
    }
}
