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
//!   callers announce directly.
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
        Role::Label | Role::TextRun | Role::Paragraph | Role::LineBreak => "text",
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
        Role::Status | Role::Timer => "status",
        Role::Log => "log",
        Role::Marquee => "marquee",
        Role::ProgressIndicator => "progressbar",
        Role::ScrollBar | Role::Splitter => "separator",
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
        Role::Footer => "contentinfo",
        Role::Header | Role::SectionHeader | Role::TitleBar => "banner",
        Role::SectionFooter => "contentinfo",
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
        Role::Audio
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
    /// Last text announced through this node's `aria-live` setting.
    live_text: Option<String>,
    /// DOM listeners (click/focus/blur) kept alive for the element's
    /// lifetime.
    _closures: Vec<Closure<dyn FnMut(web_sys::Event)>>,
}

/// The shared mutable state the DOM event closures write into.
struct Shared {
    /// Callback for AT-driven actions (click/focus/blur on mirrored
    /// elements).
    action_handler: Box<dyn FnMut(ActionRequest)>,
}

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
    /// Shared handler slot for action requests from DOM events.
    shared: Rc<RefCell<Shared>>,
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
        // The mirror must remain visible to AT — it is the tree — but it
        // is not itself a landmark.
        container
            .set_attribute("aria-label", "Application UI")
            .map_err(|e| WebA11yError::from_js(&e))?;

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
            shared: Rc::new(RefCell::new(Shared {
                action_handler: Box::new(|_| {}),
            })),
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
        self.shared.borrow_mut().action_handler = Box::new(handler);
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
    /// get a trailing zero-width space to defeat DOM text-no-change dedup
    /// in screen readers.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn example(bridge: &martensite_access::web::WebA11yBridge) {
    /// bridge.announce("3 results");
    /// # }
    /// ```
    pub fn announce(&self, text: &str) {
        // Alternating suffix so announcing the same string twice still
        // produces a text mutation AT will surface.
        let current = self.live.text_content().unwrap_or_default();
        let suffix = if current.trim_end_matches('\u{200B}') == text.trim_end_matches('\u{200B}') {
            "\u{200B}"
        } else {
            ""
        };
        self.live.set_text_content(Some(&format!("{text}{suffix}")));
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
            Self::apply_node(&entry.element, node, focused);
            new_children.insert(id, node.children().iter().map(|c| c.0).collect());
            // Live-region mirroring: announce changed text on nodes
            // carrying a `live` property.
            if let Some(live_kind) = node.live() {
                if live_kind != Live::Off {
                    if let Some(text) = node_text(node) {
                        if entry.live_text.as_deref() != Some(text.as_str()) {
                            entry.live_text = Some(text.clone());
                            announcements.push((text, live_kind));
                        }
                    }
                }
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
        for (parent_id, children) in &new_children {
            let stale: Vec<u64> = self
                .entries
                .get(parent_id)
                .map(|entry| {
                    entry
                        .children
                        .iter()
                        .copied()
                        .filter(|c| !children.contains(c))
                        .collect()
                })
                .unwrap_or_default();
            for child_id in stale {
                if let Some(child_entry) = self.entries.get(&child_id) {
                    child_entry.element.remove();
                }
                self.purge_subtree(child_id);
            }
            if let Some(parent_entry) = self.entries.get_mut(parent_id) {
                parent_entry.children = children.clone();
            }
        }

        // Phase 4: focus.
        self.set_focus(update.focus);
        Ok(())
    }

    /// Mirrors `focus` as the DOM-focused element: `tabindex="0"` and
    /// `element.focus()`; every other focusable element gets
    /// `tabindex="-1"` (roving tabindex).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn example(bridge: &mut martensite_access::web::WebA11yBridge) {
    /// bridge.set_focus(accesskit::NodeId(1));
    /// # }
    /// ```
    pub fn set_focus(&mut self, node_id: NodeId) {
        if self.focused == Some(node_id.0) {
            return;
        }
        self.focused = Some(node_id.0);
        for (id, entry) in &self.entries {
            let _ = entry
                .element
                .set_attribute("tabindex", if *id == node_id.0 { "0" } else { "-1" });
        }
        if let Some(entry) = self.entries.get(&node_id.0) {
            entry.element.focus().ok();
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
                let shared = Rc::clone(&self.shared);
                let closure = Closure::wrap(Box::new(move |_: web_sys::Event| {
                    (shared.borrow_mut().action_handler)(ActionRequest {
                        action,
                        target_tree: TreeId::ROOT,
                        target_node: NodeId(id),
                        data: None,
                    });
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
                    _closures: closures,
                },
            );
        }
        Ok(self.entries.get_mut(&id).expect("just inserted"))
    }

    /// Recursively removes a subtree's mirror entries.
    fn purge_subtree(&mut self, id: u64) {
        if let Some(entry) = self.entries.remove(&id) {
            entry.element.remove();
            for child_id in entry.children {
                self.purge_subtree(child_id);
            }
        }
    }

    /// Writes a node's ARIA properties to `element`.
    fn apply_node(element: &HtmlElement, node: &Node, focused: bool) {
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
        match node.live() {
            Some(Live::Polite) => {
                let _ = element.set_attribute("aria-live", "polite");
            }
            Some(Live::Assertive) => {
                let _ = element.set_attribute("aria-live", "assertive");
            }
            _ => {
                let _ = element.remove_attribute("aria-live");
            }
        }
        if node.is_live_atomic() {
            let _ = element.set_attribute("aria-atomic", "true");
        } else {
            let _ = element.remove_attribute("aria-atomic");
        }
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
        // Text-bearing leaf roles get real text so screen readers can
        // announce them without relying on aria-label alone.
        if matches!(node.role(), Role::Label | Role::TextRun | Role::Paragraph) {
            element.set_text_content(node_text(node).as_deref());
        }
        let focusable = node.supports_action(Action::Focus);
        if focused {
            let _ = element.set_attribute("tabindex", "0");
        } else if focusable {
            let _ = element.set_attribute("tabindex", "-1");
        } else {
            // Non-focusable nodes leave the tab order entirely.
            let _ = element.remove_attribute("tabindex");
        }
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
