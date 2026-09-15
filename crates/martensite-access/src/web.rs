//! Web (`wasm32-unknown-unknown`) accessibility bridge — the minimal
//! viable hidden-DOM/ARIA mirror committed to by milestone §4.3 (option
//! a).
//!
//! There is no AccessKit platform adapter for the web, so this module
//! translates [`TreeUpdate`]s into a live DOM subtree:
//!
//! * **DOM mirror** — a visually hidden container (the standard
//!   `sr-only` offscreen clip, *not* `display: none`, which removes
//!   elements from the accessibility tree entirely) holding one element
//!   per focusable-or-semantic AccessKit node, with `role`, `aria-label`,
//!   `aria-disabled`, `aria-expanded`, `aria-checked`, `aria-selected`,
//!   `aria-hidden`, `aria-level`, and `tabindex` mirrored from node
//!   properties.
//! * **Live-region announcer** — a separate `aria-live="polite"`
//!   element; nodes carrying [`accesskit::Live`] emit their text there
//!   when it changes, and [`announce`](WebA11yBridge::announce) lets
//!   callers announce directly. The shared region is the *single*
//!   announcement channel: `aria-live` is deliberately not mirrored onto
//!   individual elements, which would double-announce the same change.
//! * **Action return path** — DOM `click`/`focus`/`blur` events on
//!   mirrored elements are translated back into
//!   [`accesskit::ActionRequest`]s delivered to a caller-installed
//!   handler, closing the loop for AT-driven activation.
//!
//! # Deliberately out of scope (v1.0+ hardening)
//!
//! Full editable-text projection (`aria-valuetext`/selection
//! mirroring), `aria-owns`/`aria-activedescendant` composite widgets,
//! and roving-tabindex focus management beyond the single
//! `update.focus` node. The mirror is a *supplement*: it gives screen
//! readers a navigable semantic tree, not a pixel-accurate one.
//!
//! # Examples
//!
//! ```no_run
//! use martensite_access::web::WebA11yBridge;
//!
//! # fn example() -> Result<(), martensite_access::web::WebA11yError> {
//! let mut bridge = WebA11yBridge::new()?;
//! bridge.set_action_handler(|request| {
//!     let _ = request.action;
//! });
//! bridge.announce("Ready");
//! # Ok(())
//! # }
//! ```

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use accesskit::{Action, ActionRequest, Live, Node, NodeId, Role, Toggled, TreeId, TreeUpdate};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{Document, HtmlElement};

/// Error type for [`WebA11yBridge`] operations.
///
/// # Examples
///
/// ```
/// use martensite_access::web::WebA11yError;
///
/// let err = WebA11yError::new("no document");
/// assert!(err.to_string().contains("no document"));
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WebA11yError(String);

impl WebA11yError {
    /// Creates a [`WebA11yError`] from a message.
    #[must_use]
    pub fn new(msg: impl Into<String>) -> Self {
        Self(msg.into())
    }

    /// Creates a [`WebA11yError`] from a rejected DOM call.
    #[must_use]
    pub fn from_js(value: &JsValue) -> Self {
        Self(
            value
                .as_string()
                .or_else(|| value.dyn_ref::<js_sys::Error>().map(|e| e.message().into()))
                .unwrap_or_else(|| format!("{value:?}")),
        )
    }
}

impl std::fmt::Display for WebA11yError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "web accessibility bridge error: {}", self.0)
    }
}

impl std::error::Error for WebA11yError {}

fn document() -> Result<Document, WebA11yError> {
    let window = web_sys::window().ok_or_else(|| WebA11yError::new("no `window` object"))?;
    window
        .document()
        .ok_or_else(|| WebA11yError::new("no `document` object"))
}

/// The `sr-only` clip style: rendered (so AT sees it) but invisible and
/// out of flow.
const SR_ONLY: &str =
    "position:absolute;left:-10000px;top:auto;width:1px;height:1px;overflow:hidden;";

/// Maps an AccessKit [`Role`] to the ARIA `role` token written to the
/// mirror element, or `None` when the role has no meaningful ARIA
/// analogue (the mirror element then carries no `role` attribute).
///
/// The mapping covers the full semantic range of `Role`; exotic
/// document-structure roles map to their nearest ARIA token, and
/// genuinely unmapped roles fall back to `generic`-style anonymity
/// rather than a wrong token.
///
/// # Examples
///
/// ```
/// use martensite_access::web::aria_role;
/// use accesskit::Role;
///
/// assert_eq!(aria_role(Role::Button), Some("button"));
/// assert_eq!(aria_role(Role::TextInput), Some("textbox"));
/// ```
#[must_use]
pub fn aria_role(role: Role) -> Option<&'static str> {
    Some(match role {
        Role::Unknown => return None,
        Role::Button | Role::DefaultButton => "button",
        Role::CheckBox => "checkbox",
        Role::RadioButton => "radio",
        Role::RadioGroup => "radiogroup",
        Role::Switch => "switch",
        Role::TextInput
        | Role::MultilineTextInput
        | Role::SearchInput
        | Role::DateInput
        | Role::DateTimeInput
        | Role::WeekInput
        | Role::MonthInput
        | Role::TimeInput
        | Role::EmailInput
        | Role::NumberInput
        | Role::PasswordInput
        | Role::PhoneNumberInput
        | Role::UrlInput => "textbox",
        Role::ComboBox | Role::EditableComboBox => "combobox",
        Role::SpinButton => "spinbutton",
        Role::Slider => "slider",
        Role::Link => "link",
        Role::Image | Role::Canvas => "img",
        // `role="text"` is a non-standard WebKit-only token; Label/
        // TextRun/Paragraph/LineBreak carry their text as element content
        // instead and get no role (handled in the no-token arm below).
        Role::List | Role::DescriptionList => "list",
        Role::ListItem => "listitem",
        Role::ListBox => "listbox",
        Role::ListBoxOption | Role::MenuListOption => "option",
        Role::Tree | Role::TreeGrid => "tree",
        Role::TreeItem => "treeitem",
        Role::Tab => "tab",
        Role::TabList => "tablist",
        Role::TabPanel => "tabpanel",
        Role::Menu | Role::MenuListPopup => "menu",
        Role::MenuBar => "menubar",
        Role::MenuItem | Role::MenuItemCheckBox | Role::MenuItemRadio => "menuitem",
        Role::Toolbar => "toolbar",
        Role::Tooltip => "tooltip",
        Role::Dialog | Role::AlertDialog => "dialog",
        Role::Alert => "alert",
        Role::Status => "status",
        Role::Log => "log",
        Role::Marquee => "marquee",
        Role::ProgressIndicator => "progressbar",
        Role::Timer => "timer",
        // ScrollBar has a dedicated ARIA role; only Splitter is the
        // generic "separator" (a focusable splitter is still
        // separator+aria-valuenow, which the value mirroring covers).
        Role::ScrollBar => "scrollbar",
        Role::Splitter => "separator",
        Role::ScrollView | Role::Section | Role::Region => "region",
        Role::Meter => "meter",
        Role::Group | Role::GenericContainer | Role::Pane | Role::RowGroup => "group",
        Role::Table | Role::LayoutTable => "table",
        Role::Row | Role::LayoutTableRow => "row",
        Role::Cell | Role::LayoutTableCell => "cell",
        Role::ColumnHeader => "columnheader",
        Role::RowHeader => "rowheader",
        Role::Grid => "grid",
        Role::GridCell => "gridcell",
        Role::Banner => "banner",
        Role::Complementary => "complementary",
        Role::ContentInfo => "contentinfo",
        // "banner"/"contentinfo" are page-scoped landmarks — wrong for
        // AccessKit's section-scoped Header/Footer/SectionFooter. Section
        // headers map to "heading" (+ aria-level via the level property);
        // Footer/SectionFooter and TitleBar have no fitting token and get
        // no role.
        Role::Header | Role::SectionHeader => "heading",
        Role::Main => "main",
        Role::Navigation => "navigation",
        Role::Search => "search",
        Role::Form => "form",
        Role::Article => "article",
        Role::Comment => "article",
        Role::Definition | Role::Term => "term",
        Role::Document | Role::WebView | Role::RootWebArea => "document",
        Role::Application => "application",
        Role::Window => "application",
        Role::Figure => "figure",
        Role::FigureCaption | Role::Caption | Role::Legend => "caption",
        Role::Code => "code",
        Role::Emphasis => "emphasis",
        Role::Strong => "strong",
        Role::Mark | Role::Abbr | Role::RubyAnnotation => "mark",
        Role::Blockquote => "blockquote",
        Role::Note => "note",
        Role::Math => "math",
        Role::Time => "time",
        Role::Feed => "feed",
        Role::Details => "group",
        Role::DisclosureTriangle => "button",
        Role::Suggestion => "insertion",
        Role::ContentInsertion => "insertion",
        Role::ContentDeletion => "deletion",
        Role::GraphicsDocument => "graphics-document",
        Role::GraphicsObject => "graphics-object",
        Role::GraphicsSymbol => "graphics-symbol",
        Role::Label
        | Role::TextRun
        | Role::Paragraph
        | Role::LineBreak
        | Role::Footer
        | Role::SectionFooter
        | Role::TitleBar
        | Role::Audio
        | Role::Video
        | Role::PluginObject
        | Role::EmbeddedObject
        | Role::Iframe
        | Role::IframePresentational
        | Role::Keyboard
        | Role::ImeCandidate
        | Role::Caret
        | Role::ListMarker
        | Role::Ruby
        | Role::PdfActionableHighlight
        | Role::PdfRoot
        | Role::DocAbstract
        | Role::DocAcknowledgements
        | Role::DocAfterword
        | Role::DocAppendix
        | Role::DocBackLink
        | Role::DocBiblioEntry
        | Role::DocBiblioRef
        | Role::DocChapter
        | Role::DocColophon
        | Role::DocConclusion
        | Role::DocCover
        | Role::DocCredit
        | Role::DocCredits
        | Role::DocDedication
        | Role::DocEndnote
        | Role::DocEndnotes
        | Role::DocEpigraph
        | Role::DocEpilogue
        | Role::DocErrata
        | Role::DocExample => return None,
        // Remaining roles (inline/media/Doc-* structures and any future
        // additions) have no ARIA token worth asserting.
        _ => return None,
    })
}

/// Returns the visible text for a mirror element: `label`, else `value`,
/// else `description`.
fn node_text(node: &Node) -> Option<String> {
    node.label()
        .or_else(|| node.value())
        .or_else(|| node.description())
        .map(str::to_string)
}

/// Per-node mirror state.
struct MirrorEntry {
    element: HtmlElement,
    /// Child node ids as of the last update that mentioned this parent.
    children: Vec<u64>,
    /// Last text announced through the shared live-region announcer for
    /// this node (nodes carrying `Live::Polite`/`Live::Assertive`).
    live_text: Option<String>,
    /// Whether the node supports `Action::Focus` — governs `tabindex`
    /// handling in [`WebA11yBridge::set_focus`].
    focusable: bool,
    /// Whether the element currently carries text content (set for
    /// text-leaf roles) — tracked so a role change off a leaf clears it
    /// instead of leaving stale text behind.
    has_text: bool,
    /// DOM listeners (click/focus/blur) kept alive for the element's
    /// lifetime.
    _closures: Vec<Closure<dyn FnMut(web_sys::Event)>>,
}

/// The action-request handler slot shared with the DOM event closures.
///
/// `Option` so dispatch can *take* the handler out before invoking it —
/// the DOM closures then run user code with no borrow held, so a handler
/// may call back into the bridge (`set_action_handler`, `update`, …)
/// without a `RefCell` double-borrow panic. This matters even without
/// explicit reentrancy: `element.focus()` dispatches the DOM `focus`
/// event *synchronously*, so `update` → `set_focus` → `focus()` would
/// otherwise re-enter a closure while its `borrow_mut` is still live.
type ActionHandler = Rc<RefCell<Option<Box<dyn FnMut(ActionRequest)>>>>;

/// The minimal viable web accessibility bridge.
///
/// See the [module docs](self) for the architecture and scope. The
/// bridge is `!Send` (DOM handles are single-threaded); it must be used
/// on the wasm main thread.
///
/// # Examples
///
/// ```no_run
/// use martensite_access::web::WebA11yBridge;
///
/// # fn example() -> Result<(), martensite_access::web::WebA11yError> {
/// let mut bridge = WebA11yBridge::new()?;
/// bridge.announce("Application ready");
/// # Ok(())
/// # }
/// ```
pub struct WebA11yBridge {
    /// The hidden container holding the semantic mirror subtree.
    container: HtmlElement,
    /// The shared `aria-live` announcer element.
    live: HtmlElement,
    /// Mirror elements keyed by `NodeId.0`.
    entries: HashMap<u64, MirrorEntry>,
    /// DOM focus bookkeeping: the node currently mirrored as focused.
    focused: Option<u64>,
    /// The DOM element last given `tabindex="0"`/`focus()` — tracked
    /// separately from `focused` so a *re-created* entry under the same
    /// id still gets focus reapplied instead of hitting the early
    /// return.
    focused_element: Option<HtmlElement>,
    /// The currently attached tree root (its element hangs off
    /// `container`), so a root change can detach the old root.
    root: Option<u64>,
    /// When `false`, [`set_focus`](Self::set_focus) updates `tabindex`
    /// but does not call `element.focus()` — used to suspend DOM focus
    /// mirroring while an IME composition holds focus on the hidden
    /// input overlay.
    dom_focus_enabled: bool,
    /// Handler slot for action requests from DOM events (see
    /// [`ActionHandler`] for why it is an `Option` cell).
    action_handler: ActionHandler,
    /// The document used to create elements (kept for `update`).
    document: Document,
}

impl std::fmt::Debug for WebA11yBridge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebA11yBridge")
            .field("mirrored_nodes", &self.entries.len())
            .finish()
    }
}

impl WebA11yBridge {
    /// Creates the bridge: a hidden mirror container and an
    /// `aria-live="polite"` announcer, both appended to `document.body`.
    ///
    /// # Errors
    ///
    /// Returns [`WebA11yError`] when there is no DOM document/body or
    /// element creation fails.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_access::web::WebA11yBridge;
    ///
    /// # fn example() -> Result<(), martensite_access::web::WebA11yError> {
    /// let _bridge = WebA11yBridge::new()?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn new() -> Result<Self, WebA11yError> {
        let document = document()?;
        let body = document
            .body()
            .ok_or_else(|| WebA11yError::new("document has no <body>"))?;

        let container: HtmlElement = document
            .create_element("div")
            .map_err(|e| WebA11yError::from_js(&e))?
            .unchecked_into();
        container
            .set_attribute("style", SR_ONLY)
            .map_err(|e| WebA11yError::from_js(&e))?;
        container
            .set_attribute("data-martensite-a11y-mirror", "")
            .map_err(|e| WebA11yError::from_js(&e))?;
        // The container carries no role and no aria-label (a label on a
        // role-less element is ignored by most AT); the tree's own root
        // node provides the semantic entry point.

        let live: HtmlElement = document
            .create_element("div")
            .map_err(|e| WebA11yError::from_js(&e))?
            .unchecked_into();
        live.set_attribute("style", SR_ONLY)
            .map_err(|e| WebA11yError::from_js(&e))?;
        live.set_attribute("aria-live", "polite")
            .map_err(|e| WebA11yError::from_js(&e))?;
        live.set_attribute("aria-atomic", "true")
            .map_err(|e| WebA11yError::from_js(&e))?;

        body.append_child(&container)
            .map_err(|e| WebA11yError::from_js(&e))?;
        body.append_child(&live)
            .map_err(|e| WebA11yError::from_js(&e))?;

        Ok(Self {
            container,
            live,
            entries: HashMap::new(),
            focused: None,
            focused_element: None,
            root: None,
            dom_focus_enabled: true,
            action_handler: Rc::new(RefCell::new(None)),
            document,
        })
    }

    /// Installs the handler receiving [`ActionRequest`]s generated when
    /// AT interacts with the mirror (DOM `click` → `Action::Click`,
    /// `focus` → `Action::Focus`, `blur` → `Action::Blur`).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn example(bridge: &martensite_access::web::WebA11yBridge) {
    /// bridge.set_action_handler(|request| {
    ///     let _ = request.action;
    /// });
    /// # }
    /// ```
    pub fn set_action_handler(&self, handler: impl FnMut(ActionRequest) + 'static) {
        *self.action_handler.borrow_mut() = Some(Box::new(handler));
    }

    /// Returns the hidden container element (for tests/inspection).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn example(bridge: &martensite_access::web::WebA11yBridge) {
    /// let container = bridge.container();
    /// let _ = container.child_element_count();
    /// # }
    /// ```
    #[must_use]
    pub fn container(&self) -> &HtmlElement {
        &self.container
    }

    /// Returns the number of nodes currently mirrored.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn example(bridge: &martensite_access::web::WebA11yBridge) {
    /// let _ = bridge.mirrored_count();
    /// # }
    /// ```
    #[must_use]
    pub fn mirrored_count(&self) -> usize {
        self.entries.len()
    }

    /// Returns the [`NodeId`] currently mirrored as focused.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn example(bridge: &martensite_access::web::WebA11yBridge) {
    /// let _ = bridge.focused_node();
    /// # }
    /// ```
    #[must_use]
    pub fn focused_node(&self) -> Option<NodeId> {
        self.focused.map(NodeId)
    }

    /// Announces `text` through the shared `aria-live` region.
    ///
    /// The region is rewritten in place; repeated identical announcements
    /// alternate a trailing zero-width space on/off so *every* call
    /// produces a DOM text mutation — a fixed suffix would only defeat
    /// the screen reader's text-no-change dedup once.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn example(bridge: &martensite_access::web::WebA11yBridge) {
    /// bridge.announce("3 results");
    /// # }
    /// ```
    pub fn announce(&self, text: &str) {
        let current = self.live.text_content().unwrap_or_default();
        let next = if current.trim_end_matches('\u{200B}') == text {
            // Same logical text again: toggle the ZWSP so the DOM text
            // changes every time.
            if current.ends_with('\u{200B}') {
                text.to_string()
            } else {
                format!("{text}\u{200B}")
            }
        } else {
            text.to_string()
        };
        self.live.set_text_content(Some(&next));
    }

    /// Applies a [`TreeUpdate`]: creates/updates mirror elements,
    /// re-parents DOM children to match `Node::children`, removes mirror
    /// elements for nodes dropped from the tree, mirrors `live` nodes
    /// into the announcer, and applies `update.focus`.
    ///
    /// # Errors
    ///
    /// Returns [`WebA11yError`] if DOM manipulation fails. Partial
    /// application is possible — the mirror is always left consistent
    /// for the nodes processed before the failure.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn example(
    /// #    bridge: &mut martensite_access::web::WebA11yBridge,
    /// #    update: &accesskit::TreeUpdate,
    /// # ) -> Result<(), martensite_access::web::WebA11yError> {
    /// bridge.update(update)?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn update(&mut self, update: &TreeUpdate) -> Result<(), WebA11yError> {
        // Phase 1: create/update elements, collect the new
        // parent→child edges, and queue live-region announcements for
        // after the borrow on `entries` ends.
        let mut new_children: HashMap<u64, Vec<u64>> = HashMap::new();
        let mut announcements: Vec<(String, Live)> = Vec::new();
        for (node_id, node) in &update.nodes {
            let id = node_id.0;
            let focused = self.focused == Some(id);
            let entry = self.entry_for(id)?;
            entry.focusable = Self::apply_node(&entry.element, node, focused);
            // Text-bearing leaf roles carry their text as element
            // content; a node that re-roles away from a text leaf gets
            // its stale text cleared.
            let is_text_leaf = matches!(node.role(), Role::Label | Role::TextRun | Role::Paragraph);
            if is_text_leaf {
                entry.element.set_text_content(node_text(node).as_deref());
                entry.has_text = true;
            } else if entry.has_text {
                entry.element.set_text_content(None);
                entry.has_text = false;
            }
            new_children.insert(id, node.children().iter().map(|c| c.0).collect());
            // Live-region mirroring: announce changed text on nodes
            // carrying a `live` property.
            match node.live() {
                Some(live_kind) if live_kind != Live::Off => {
                    if let Some(text) = node_text(node) {
                        if entry.live_text.as_deref() != Some(text.as_str()) {
                            entry.live_text = Some(text.clone());
                            announcements.push((text, live_kind));
                        }
                    }
                }
                // Live dropped/turned off: forget the announced baseline
                // so a later re-enable re-announces instead of diffing
                // against stale text.
                _ => entry.live_text = None,
            }
        }
        for (text, live_kind) in announcements {
            // Assertive gets its own transient level on the shared
            // announcer element.
            let level = match live_kind {
                Live::Assertive => "assertive",
                _ => "polite",
            };
            let _ = self.live.set_attribute("aria-live", level);
            self.announce(&text);
        }
        // Restore the resting level so a finished assertive announcement
        // doesn't leave the region stuck on assertive for later
        // `announce` calls.
        let _ = self.live.set_attribute("aria-live", "polite");

        // Phase 2: rewire parent→child edges. Elements whose parent no
        // longer lists them are detached; their subtrees are purged.
        for (parent_id, children) in &new_children {
            let Some(parent_entry) = self.entries.get(parent_id) else {
                continue;
            };
            for &child_id in children {
                if let Some(child_entry) = self.entries.get(&child_id) {
                    parent_entry
                        .element
                        .append_child(&child_entry.element)
                        .map_err(|e| WebA11yError::from_js(&e))?;
                }
            }
        }

        // The root node hangs directly off the container.
        if let Some(tree) = &update.tree {
            if let Some(root_entry) = self.entries.get(&tree.root.0) {
                self.container
                    .append_child(&root_entry.element)
                    .map_err(|e| WebA11yError::from_js(&e))?;
            }
        }

        // Phase 3: detach children that disappeared from their parent's
        // list, then purge fully-orphaned subtrees.
        //
        // A child missing from its old parent's new list may have been
        // re-parented in this same update — Phase 2 already moved its
        // element. Purging it anyway would destroy the moved subtree's
        // element, `entries` record, and listeners permanently. `claimed`
        // therefore covers every id this update still references: all
        // new parent→child lists plus the tree root, which hangs off the
        // container rather than any parent's list. Stored `children` of
        // parents the update did not touch are deliberately *not*
        // consulted — a move always includes the new parent in the
        // update, so a reference surviving only in stale bookkeeping is
        // not a real claim, and honouring it would leak mirror elements
        // whose node was deleted after moving.
        let mut claimed: std::collections::HashSet<u64> = std::collections::HashSet::new();
        for children in new_children.values() {
            claimed.extend(children.iter().copied());
        }
        if let Some(tree) = &update.tree {
            claimed.insert(tree.root.0);
        }
        for (parent_id, children) in &new_children {
            let stale: Vec<u64> = self
                .entries
                .get(parent_id)
                .map(|entry| {
                    entry
                        .children
                        .iter()
                        .copied()
                        .filter(|c| !children.contains(c) && !claimed.contains(c))
                        .collect()
                })
                .unwrap_or_default();
            for child_id in stale {
                if let Some(child_entry) = self.entries.get(&child_id) {
                    child_entry.element.remove();
                }
                self.purge_subtree(child_id, &claimed);
            }
            if let Some(parent_entry) = self.entries.get_mut(parent_id) {
                parent_entry.children = children.clone();
            }
        }

        // Root handoff: a replaced root is not in any parent's child
        // list — its parent is the container — so Phase 3 never sees it.
        // Detach and purge it here when the update no longer references
        // it (a demoted-but-still-present old root is claimed and keeps
        // its already-moved element).
        if let Some(tree) = &update.tree {
            let new_root = tree.root.0;
            if let Some(old_root) = self.root {
                if old_root != new_root && !claimed.contains(&old_root) {
                    if let Some(entry) = self.entries.get(&old_root) {
                        entry.element.remove();
                    }
                    self.purge_subtree(old_root, &claimed);
                }
            }
            self.root = Some(new_root);
        }

        // Phase 4: focus.
        self.set_focus(update.focus);
        Ok(())
    }

    /// Suspends or resumes DOM focus mirroring.
    ///
    /// While `enabled` is `false`, [`set_focus`](Self::set_focus) still
    /// tracks the focused node and maintains roving `tabindex`, but does
    /// not call `element.focus()`. Use this when another element must
    /// keep DOM focus — e.g. a `martensite_window::web::HiddenImeInput`
    /// overlay holding focus during an IME composition, where a
    /// mirror-side `focus()` would steal focus back and cancel the
    /// in-flight composition. The default is `true`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn example(bridge: &mut martensite_access::web::WebA11yBridge) {
    /// bridge.set_dom_focus_enabled(false); // IME overlay owns DOM focus
    /// bridge.set_dom_focus_enabled(true);
    /// # }
    /// ```
    pub fn set_dom_focus_enabled(&mut self, enabled: bool) {
        self.dom_focus_enabled = enabled;
    }

    /// Mirrors `focus` as the DOM-focused element: `tabindex="0"` and
    /// `element.focus()`; every other *focusable* element gets
    /// `tabindex="-1"` (roving tabindex) and non-focusable elements keep
    /// no `tabindex` at all — the same policy [`apply_node`](Self::update)
    /// applies, so a focus change never pushes a non-focusable mirror
    /// element into the tab order.
    ///
    /// DOM `focus()` is only invoked while
    /// [`dom_focus_enabled`](Self::set_dom_focus_enabled) is `true`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn example(bridge: &mut martensite_access::web::WebA11yBridge) {
    /// bridge.set_focus(accesskit::NodeId(1));
    /// # }
    /// ```
    pub fn set_focus(&mut self, node_id: NodeId) {
        // Early-out only when the same id *and* the same element are
        // still focused — an entry purged and re-created under the same
        // id owns a fresh element that needs tabindex/focus reapplied.
        let element = self.entries.get(&node_id.0).map(|e| e.element.clone());
        if self.focused == Some(node_id.0) && self.focused_element == element {
            return;
        }
        self.focused = Some(node_id.0);
        self.focused_element = element;
        for (id, entry) in &self.entries {
            if *id == node_id.0 {
                let _ = entry.element.set_attribute("tabindex", "0");
            } else if entry.focusable {
                let _ = entry.element.set_attribute("tabindex", "-1");
            } else {
                let _ = entry.element.remove_attribute("tabindex");
            }
        }
        if self.dom_focus_enabled {
            if let Some(entry) = self.entries.get(&node_id.0) {
                entry.element.focus().ok();
            }
        }
    }

    /// Removes all mirrored elements (e.g. on tree teardown).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn example(bridge: &mut martensite_access::web::WebA11yBridge) {
    /// bridge.clear();
    /// assert_eq!(bridge.mirrored_count(), 0);
    /// # }
    /// ```
    pub fn clear(&mut self) {
        for (_, entry) in self.entries.drain() {
            entry.element.remove();
        }
        self.focused = None;
        self.focused_element = None;
        self.root = None;
    }

    /// Fetches (creating if needed) the mirror entry for `id`.
    fn entry_for(&mut self, id: u64) -> Result<&mut MirrorEntry, WebA11yError> {
        if !self.entries.contains_key(&id) {
            let element: HtmlElement = self
                .document
                .create_element("div")
                .map_err(|e| WebA11yError::from_js(&e))?
                .unchecked_into();
            let mut closures: Vec<Closure<dyn FnMut(web_sys::Event)>> = Vec::new();
            for (kind, action) in [
                ("click", Action::Click),
                ("focus", Action::Focus),
                ("blur", Action::Blur),
            ] {
                let handler = Rc::clone(&self.action_handler);
                let closure = Closure::wrap(Box::new(move |_: web_sys::Event| {
                    // Take-and-reinstall: the handler runs with no borrow
                    // held, so it may call `set_action_handler` or trigger
                    // synchronous DOM events (e.g. `focus()` inside
                    // `set_focus`) that re-enter a sibling closure without
                    // a RefCell double-borrow panic. An event arriving
                    // while the slot is empty (mid-dispatch reentrancy) is
                    // dropped rather than recursed into.
                    let mut cb = handler.borrow_mut().take();
                    if let Some(f) = cb.as_mut() {
                        f(ActionRequest {
                            action,
                            // The mirror models a single tree; multi-tree
                            // sources are out of scope for the minimal
                            // bridge, so `target_tree` is always ROOT.
                            target_tree: TreeId::ROOT,
                            target_node: NodeId(id),
                            data: None,
                        });
                    }
                    let mut slot = handler.borrow_mut();
                    if slot.is_none() {
                        *slot = cb;
                    }
                }) as Box<dyn FnMut(web_sys::Event)>);
                element
                    .add_event_listener_with_callback(kind, closure.as_ref().unchecked_ref())
                    .map_err(|e| WebA11yError::from_js(&e))?;
                closures.push(closure);
            }
            self.entries.insert(
                id,
                MirrorEntry {
                    element,
                    children: Vec::new(),
                    live_text: None,
                    focusable: false,
                    has_text: false,
                    _closures: closures,
                },
            );
        }
        Ok(self.entries.get_mut(&id).expect("just inserted"))
    }

    /// Recursively removes a subtree's mirror entries, skipping ids in
    /// `claimed` — children re-parented elsewhere in the same update have
    /// already been moved by Phase 2 and their subtrees must survive.
    fn purge_subtree(&mut self, id: u64, claimed: &std::collections::HashSet<u64>) {
        if claimed.contains(&id) {
            return;
        }
        if let Some(entry) = self.entries.remove(&id) {
            entry.element.remove();
            for child_id in entry.children {
                self.purge_subtree(child_id, claimed);
            }
        }
    }

    /// Writes a node's ARIA properties to `element`, returning whether
    /// the node supports `Action::Focus` (stored on the entry for
    /// [`set_focus`](Self::set_focus)'s roving-tabindex policy).
    fn apply_node(element: &HtmlElement, node: &Node, focused: bool) -> bool {
        if let Some(role) = aria_role(node.role()) {
            let _ = element.set_attribute("role", role);
        } else {
            let _ = element.remove_attribute("role");
        }
        if let Some(label) = node.label() {
            let _ = element.set_attribute("aria-label", label);
        } else {
            let _ = element.remove_attribute("aria-label");
        }
        if let Some(description) = node.description() {
            let _ = element.set_attribute("aria-description", description);
        } else {
            let _ = element.remove_attribute("aria-description");
        }
        if let Some(value) = node.value() {
            let _ = element.set_attribute("aria-valuetext", value);
        } else {
            let _ = element.remove_attribute("aria-valuetext");
        }
        let _ = element.set_attribute("aria-disabled", bool_str(node.is_disabled()));
        let _ = element.set_attribute("aria-hidden", bool_str(node.is_hidden()));
        match node.is_selected() {
            Some(selected) => {
                let _ = element.set_attribute("aria-selected", bool_str(selected));
            }
            None => {
                let _ = element.remove_attribute("aria-selected");
            }
        }
        match node.is_expanded() {
            Some(expanded) => {
                let _ = element.set_attribute("aria-expanded", bool_str(expanded));
            }
            None => {
                let _ = element.remove_attribute("aria-expanded");
            }
        }
        match node.toggled() {
            Some(Toggled::True) => {
                let _ = element.set_attribute("aria-checked", "true");
            }
            Some(Toggled::Mixed) => {
                let _ = element.set_attribute("aria-checked", "mixed");
            }
            Some(Toggled::False) => {
                let _ = element.set_attribute("aria-checked", "false");
            }
            None => {
                let _ = element.remove_attribute("aria-checked");
            }
        }
        if let Some(level) = node.level() {
            let _ = element.set_attribute("aria-level", &level.to_string());
        } else {
            let _ = element.remove_attribute("aria-level");
        }
        // `node.live()`/`is_live_atomic()` are deliberately NOT mirrored
        // onto the element: live-region text changes are announced
        // through the shared announcer element (`update` phase 1), which
        // is the single announcement channel. Mirroring `aria-live` here
        // too would double-announce every live-node change.
        if node.is_busy() {
            let _ = element.set_attribute("aria-busy", "true");
        } else {
            let _ = element.remove_attribute("aria-busy");
        }
        if node.is_modal() {
            let _ = element.set_attribute("aria-modal", "true");
        } else {
            let _ = element.remove_attribute("aria-modal");
        }
        if node.is_required() {
            let _ = element.set_attribute("aria-required", "true");
        } else {
            let _ = element.remove_attribute("aria-required");
        }
        if node.is_read_only() {
            let _ = element.set_attribute("aria-readonly", "true");
        } else {
            let _ = element.remove_attribute("aria-readonly");
        }
        // Text content for leaf roles is handled by the caller (it needs
        // `MirrorEntry::has_text` to clear stale text on a role change).
        let focusable = node.supports_action(Action::Focus);
        if focused {
            let _ = element.set_attribute("tabindex", "0");
        } else if focusable {
            let _ = element.set_attribute("tabindex", "-1");
        } else {
            // Non-focusable nodes leave the tab order entirely.
            let _ = element.remove_attribute("tabindex");
        }
        focusable
    }
}

fn bool_str(b: bool) -> &'static str {
    if b {
        "true"
    } else {
        "false"
    }
}

impl Drop for WebA11yBridge {
    fn drop(&mut self) {
        self.container.remove();
        self.live.remove();
    }
}
