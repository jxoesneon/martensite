//! The lint scene — a normalized widget tree distilled from a
//! [`PaintList`]'s `PushScope`/`PopScope` provenance markers.
//!
//! The paint walker wraps every widget's commands in a scope carrying
//! its `debug_name` and layout bounds, so the command stream already
//! encodes the structural information design rules need: hierarchy,
//! geometry, text sizes, and colors. `LintScene::from_paint_list`
//! replays a paint list into that tree — no app instrumentation is
//! required and the same scene works for live frames, headless tests,
//! and golden fixtures.

use std::collections::BTreeSet;

use kurbo::{Rect, Shape};
use martensite_core::{PaintCommand, PaintList};

/// What a node is *for*, inferred from its `debug_name`'s final
/// `::`-separated segment (the concrete widget type name). Apps can
/// reclassify individual names via
/// [`LintConfig::classify`](crate::LintConfig::classify) or opt a
/// subtree out with an inline `@lint:` marker — see
/// [`LintScene::from_paint_list`].
///
/// # Examples
///
/// ```
/// use martensite_design_lint::NodeKind;
///
/// // Built-in classification recognizes common widget names.
/// assert_eq!(NodeKind::from_widget_name("Tabs"), NodeKind::Navigation);
/// assert_eq!(NodeKind::from_widget_name("Button"), NodeKind::Interactive);
/// assert_eq!(
///     NodeKind::from_widget_name("Sparkline"),
///     NodeKind::Content
/// );
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NodeKind {
    /// Orientation device — tabs, toolbars, menu bars, breadcrumbs,
    /// nav rails. Stacked navigation is the primary crowding driver
    /// the `nav-depth` rule measures.
    Navigation,
    /// A control the user can act on — buttons, fields, pickers,
    /// sliders. Feeds `choice-count`, `target-size`, and
    /// `interactive-density`.
    Interactive,
    /// Passive information — labels, charts, text, images.
    Content,
    /// Structural grouping — flex columns, panels, scroll regions.
    Container,
    /// Window/surface furniture — title bars, status bars, headers.
    Chrome,
    /// Unclassified — treated as `Content` by the rules.
    Unknown,
}

impl NodeKind {
    /// Classify a widget by the last segment of its `debug_name`.
    ///
    /// Matching is case-insensitive substring-based on a curated table
    /// of common widget names — `IconButton` matches `Button`,
    /// `ZoneTabs` matches `Tabs`. Order matters: nav names win over
    /// interactive names for devices that are both (a `Tab` bar is
    /// navigation; an individual `Tab` is a control inside it).
    pub fn from_widget_name(name: &str) -> Self {
        let short = name.rsplit("::").next().unwrap_or(name);
        let short = short.split('@').next().unwrap_or(short).trim();
        let lower = short.to_ascii_lowercase();
        let has = |s: &str| lower.contains(s);
        // Chrome first — status/title bars are furniture, not content.
        if has("titlebar") || has("statusbar") || has("title_bar") || has("status_bar") {
            return NodeKind::Chrome;
        }
        // Navigation devices.
        if has("tabbar")
            || has("tabs")
            || has("menubar")
            || has("toolbar")
            || has("ribbon")
            || has("breadcrumb")
            || has("navrail")
            || has("nav_rail")
            || has("sidebar")
            || has("pagination")
            || has("pager")
        {
            return NodeKind::Navigation;
        }
        // Interactive controls — substring matches cover the common
        // suffixes (IconButton→button, SearchField→field, ...).
        const INTERACTIVE: &[&str] = &[
            "button",
            "dropdown",
            "checkbox",
            "radio",
            "slider",
            "switch",
            "toggle",
            "field",
            "combobox",
            "picker",
            "stepper",
            "spinbox",
            "rating",
            "link",
            "menuitem",
            "segmented",
            "cascader",
            "treeselect",
            "listbox",
            "select",
            "input",
            "menu",
            "tab",
        ];
        if INTERACTIVE.iter().any(|s| has(s)) {
            return NodeKind::Interactive;
        }
        if has("text")
            || has("label")
            || has("chart")
            || has("graph")
            || has("sparkline")
            || has("image")
            || has("canvas")
            || has("gauge")
            || has("indicator")
            || has("badge")
            || has("progress")
            || has("meter")
            || has("media")
            || has("view")
        {
            return NodeKind::Content;
        }
        if has("flex")
            || has("container")
            || has("panel")
            || has("scroll")
            || has("stack")
            || has("grid")
            || has("column")
            || has("row")
            || has("page")
            || has("group")
            || has("card")
            || has("section")
            || has("zone")
            || has("split")
            || has("dock")
        {
            return NodeKind::Container;
        }
        NodeKind::Unknown
    }
}

/// One node of a [`LintScene`] — a widget scope with its own-scope
/// paint statistics.
#[derive(Debug, Clone)]
pub struct LintNode {
    /// Short display name (last `::` segment, `@lint:` marker stripped).
    pub name: String,
    /// Full `debug_name` as emitted — usually the Rust type path.
    pub full_name: String,
    /// `/`-joined ancestor path (`"App/ZonePanel/Tabs"`) — the anchor
    /// for path-based `allow` overrides in [`LintConfig`](crate::LintConfig).
    pub path: String,
    /// Layout bounds in device pixels (as painted).
    pub bounds: Rect,
    /// Arena handle of the emitting widget, when one exists.
    pub widget_id: Option<u64>,
    /// Classified kind — see [`NodeKind::from_widget_name`].
    pub kind: NodeKind,
    /// Allow specifiers applying to this node — its own inline
    /// `@lint:` markers **merged with all ancestors'**, so an allow on
    /// a container suppresses its whole subtree (the `tools:ignore`
    /// inheritance model). Entries are rule ids, `standard:<key>`, or
    /// `all`.
    pub allows: Vec<String>,
    /// Own inline allows only (not inherited) — used for stale-marker
    /// reporting.
    pub own_allows: Vec<String>,
    /// ISA-101 display level declared via an `@level:1..4` name
    /// marker. `None` = undeclared — hierarchy rules treat undeclared
    /// surfaces as reviewable, not violations.
    pub display_level: Option<u8>,
    /// Font sizes painted *in this scope* (device px), deduplicated.
    pub font_sizes: Vec<f32>,
    /// Distinct fill/stroke/text colors painted *in this scope*.
    pub colors: Vec<[u8; 4]>,
    /// Approximate painted coverage of this scope — sum of fill-rect
    /// areas clipped to `bounds`, capped at the bounds area. Text and
    /// strokes contribute nothing; fills dominate coverage in
    /// practice.
    pub painted_area: f64,
    /// Child scopes, in paint order.
    pub children: Vec<LintNode>,
}

impl LintNode {
    /// Depth-first iterator over this subtree (self included).
    pub fn walk(&self) -> Walk<'_> {
        Walk { stack: vec![self] }
    }

    /// True when `allow` (a rule id, `standard:<key>`, or `all`)
    /// appears in this node's inline allow list.
    pub fn allows(&self, spec: &str) -> bool {
        self.allows.iter().any(|a| a == spec || a == "all")
    }

    /// Area of `bounds` in square device pixels.
    pub fn area(&self) -> f64 {
        (self.bounds.width() * self.bounds.height()).max(0.0)
    }

    /// Interactive descendants not nested inside another interactive
    /// descendant — the "decision surface" population `choice-count`
    /// and `interactive-density` measure.
    pub fn interactive_leaves(&self) -> Vec<&LintNode> {
        fn collect<'a>(n: &'a LintNode, inside: bool, out: &mut Vec<&'a LintNode>) {
            let interactive = n.kind == NodeKind::Interactive;
            if interactive && !inside {
                out.push(n);
                return; // don't double-count controls nested in controls
            }
            for c in &n.children {
                collect(c, inside || interactive, out);
            }
        }
        let mut out = Vec::new();
        for c in &self.children {
            collect(c, false, &mut out);
        }
        out
    }

    /// Distinct font sizes across the whole subtree.
    pub fn subtree_font_sizes(&self) -> BTreeSet<u32> {
        let mut set = BTreeSet::new();
        for n in self.walk() {
            for s in &n.font_sizes {
                set.insert((s * 2.0).round() as u32); // quantize to 0.5px
            }
        }
        set
    }

    /// Distinct colors across the whole subtree.
    pub fn subtree_colors(&self) -> Vec<[u8; 4]> {
        let mut seen = Vec::new();
        for n in self.walk() {
            for c in &n.colors {
                if !seen.contains(c) {
                    seen.push(*c);
                }
            }
        }
        seen
    }
}

/// Depth-first iterator returned by [`LintNode::walk`].
pub struct Walk<'a> {
    stack: Vec<&'a LintNode>,
}

impl<'a> Iterator for Walk<'a> {
    type Item = &'a LintNode;
    fn next(&mut self) -> Option<&'a LintNode> {
        let n = self.stack.pop()?;
        // Push children in reverse so iteration is paint order.
        for c in n.children.iter().rev() {
            self.stack.push(c);
        }
        Some(n)
    }
}

/// A normalized scene for the rules — one or more scope roots
/// (usually one: the app root) plus frame geometry.
///
/// # Examples
///
/// ```
/// use martensite_core::PaintList;
/// use martensite_design_lint::LintScene;
///
/// let scene = LintScene::from_paint_list(&PaintList::new());
/// assert!(scene.roots.is_empty());
/// ```
#[derive(Debug, Clone)]
pub struct LintScene {
    /// Top-level scopes — usually one app root, but manually-built
    /// scenes and multi-window dumps can carry several.
    pub roots: Vec<LintNode>,
    /// The drawable frame in device pixels, when the caller knows it
    /// (surface size). `None` leaves surface-relative rules using the
    /// union of root bounds instead.
    pub frame: Option<Rect>,
    /// Display scale factor — converts device px to logical pt for
    /// pt-denominated thresholds (WCAG 2.5.8's 24pt, font sizes).
    pub scale_factor: f32,
}

impl Default for LintScene {
    fn default() -> Self {
        LintScene {
            roots: Vec::new(),
            frame: None,
            scale_factor: 1.0,
        }
    }
}

impl LintScene {
    /// Distill a paint list into a scene tree.
    ///
    /// Replays `PushScope`/`PopScope` into nested [`LintNode`]s and
    /// attributes fills, strokes, and text to the innermost enclosing
    /// scope. `debug_name` suffixes of the form
    /// `"Name@lint:rule-id,standard:isa-101"` or `"Name@lint:all"` are
    /// parsed as inline allows and stripped from the display name —
    /// the per-element ignore seam, usable from any `debug_name`
    /// override without touching widget internals.
    pub fn from_paint_list(list: &PaintList) -> Self {
        struct Acc {
            node: LintNode,
        }
        let mut stack: Vec<Acc> = Vec::new();
        let mut roots: Vec<LintNode> = Vec::new();
        let mut clip: Vec<Rect> = Vec::new();
        let clip_rect = |clip: &[Rect]| clip.iter().copied().reduce(|a, b| a.intersect(b));

        // Attribute a paint stat to the innermost live scope.
        macro_rules! current {
            () => {
                stack.last_mut()
            };
        }

        for cmd in &list.commands {
            match cmd {
                PaintCommand::PushScope { id, name, bounds } => {
                    let (display, own_allows, display_level) = parse_markers(name);
                    let short = display.rsplit("::").next().unwrap_or(display).to_string();
                    let (path, allows) = match stack.last() {
                        Some(parent) => {
                            // Inherit ancestors' allows — an allow on a
                            // container covers its subtree.
                            let mut merged = parent.node.allows.clone();
                            for a in &own_allows {
                                if !merged.contains(a) {
                                    merged.push(a.clone());
                                }
                            }
                            (format!("{}/{}", parent.node.path, short), merged)
                        }
                        None => (short.clone(), own_allows.clone()),
                    };
                    stack.push(Acc {
                        node: LintNode {
                            name: short,
                            full_name: display.to_string(),
                            path,
                            bounds: *bounds,
                            widget_id: id.map(|w| w.to_u64()),
                            kind: NodeKind::from_widget_name(display),
                            allows,
                            own_allows,
                            display_level,
                            font_sizes: Vec::new(),
                            colors: Vec::new(),
                            painted_area: 0.0,
                            children: Vec::new(),
                        },
                    });
                }
                PaintCommand::PopScope => {
                    if let Some(acc) = stack.pop() {
                        match stack.last_mut() {
                            Some(parent) => parent.node.children.push(acc.node),
                            None => roots.push(acc.node),
                        }
                    }
                }
                PaintCommand::ClipRect(r) | PaintCommand::ClipRoundedRect(r, _) => clip.push(*r),
                PaintCommand::ClipPath(p) => clip.push(p.bounding_box()),
                PaintCommand::PopClip => {
                    clip.pop();
                }
                PaintCommand::FillRect(r, c) => {
                    if let Some(acc) = current!() {
                        acc.node.painted_area +=
                            visible_fill_area(*r, acc.node.bounds, &clip_rect(&clip));
                        push_unique(&mut acc.node.colors, *c);
                    }
                }
                PaintCommand::FillPath(p, c) => {
                    if let Some(acc) = current!() {
                        acc.node.painted_area +=
                            visible_fill_area(p.bounding_box(), acc.node.bounds, &clip_rect(&clip));
                        push_unique(&mut acc.node.colors, *c);
                    }
                }
                PaintCommand::StrokeRect(_, _, c) | PaintCommand::StrokePath(_, _, c) => {
                    if let Some(acc) = current!() {
                        push_unique(&mut acc.node.colors, *c);
                    }
                }
                PaintCommand::DrawText(_, _t, size, c) => {
                    if let Some(acc) = current!() {
                        push_unique_f32(&mut acc.node.font_sizes, *size);
                        push_unique(&mut acc.node.colors, *c);
                    }
                }
                PaintCommand::DrawGlyphRun(run) => {
                    if let Some(acc) = current!() {
                        push_unique_f32(&mut acc.node.font_sizes, run.font_size);
                        push_unique(&mut acc.node.colors, run.color);
                    }
                }
                _ => {}
            }
        }
        // Unbalanced scopes tolerated — surface them as roots rather
        // than discarding the stats they collected.
        while let Some(acc) = stack.pop() {
            match stack.last_mut() {
                Some(parent) => parent.node.children.push(acc.node),
                None => roots.push(acc.node),
            }
        }
        LintScene {
            roots,
            frame: None,
            scale_factor: 1.0,
        }
    }

    /// The scene's reference surface — `frame` when set, else the
    /// union of root bounds.
    pub fn surface(&self) -> Rect {
        if let Some(f) = self.frame {
            return f;
        }
        self.roots
            .iter()
            .map(|r| r.bounds)
            .reduce(|a, b| a.union(b))
            .unwrap_or_default()
    }

    /// Depth-first iterator over every node in the scene.
    pub fn walk(&self) -> impl Iterator<Item = &LintNode> {
        self.roots.iter().flat_map(|r| r.walk())
    }
}

/// Parse `Name@lint:...` and `Name@level:N` marker suffixes from a
/// scope name. A name may carry several markers
/// (`"Panel@level:2@lint:color-budget"`); each `@`-section is parsed
/// independently. Returns the cleaned display name, the allow
/// specifiers, and the declared display level.
fn parse_markers(name: &str) -> (&str, Vec<String>, Option<u8>) {
    let mut allows = Vec::new();
    let mut level = None;
    // Find the first marker (whichever comes first); everything
    // before it is the display name.
    let marker_at = [name.find("@lint:"), name.find("@level:")]
        .into_iter()
        .flatten()
        .min();
    let Some(mi) = marker_at else {
        return (name, allows, level);
    };
    let display = &name[..mi];
    for section in name[mi..].split('@').filter(|s| !s.is_empty()) {
        if let Some(spec) = section.strip_prefix("lint:") {
            allows.extend(
                spec.split(',')
                    .map(|s| s.trim().to_ascii_lowercase())
                    .filter(|s| !s.is_empty()),
            );
        } else if let Some(l) = section.strip_prefix("level:") {
            level = l.trim().parse::<u8>().ok().filter(|l| (1..=4).contains(l));
        }
    }
    (display, allows, level)
}

fn push_unique(v: &mut Vec<[u8; 4]>, c: [u8; 4]) {
    if !v.contains(&c) {
        v.push(c);
    }
}

fn push_unique_f32(v: &mut Vec<f32>, x: f32) {
    if !v.iter().any(|s| (s - x).abs() < 0.5) {
        v.push(x);
    }
}

/// Fill-rect area after clipping to the scope bounds and the active
/// clip stack — coverage outside either doesn't paint.
fn visible_fill_area(fill: Rect, scope: Rect, clip: &Option<Rect>) -> f64 {
    let mut vis = fill.intersect(scope);
    if let Some(c) = clip {
        vis = vis.intersect(*c);
    }
    (vis.width() * vis.height()).max(0.0)
}
