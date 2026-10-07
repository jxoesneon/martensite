//! Web (`wasm32-unknown-unknown`) accessibility bridge — a positioned,
//! transparent DOM/ARIA mirror of the AccessKit tree in the style of
//! Flutter's `flt-semantics` layer, committed to by milestone §4.3
//! (option a).
//!
//! There is no AccessKit platform adapter for the web, so this module
//! translates [`TreeUpdate`]s into a live DOM subtree:
//!
//! * **Positioned DOM mirror** — each mirrored node owns an element
//!   absolutely positioned at the node's real [`Node::bounds`] inside
//!   the `data-martensite-a11y-mirror` container. Elements are visually
//!   transparent (`opacity: 0`, `pointer-events: none`) but fully
//!   exposed to assistive technology: screen readers see a semantic
//!   tree that is pixel-accurate for browse-mode focus outlines and
//!   touch exploration, not a single `sr-only` clip. Elements without
//!   bounds collapse to a zero-size element at their parent's origin.
//!   Mirror state is diffed before writing — style, attribute, text,
//!   value, and selection writes are skipped when unchanged, so the
//!   per-frame `DIRTY_A11Y` updates produced by layout do not trigger
//!   DOM layout thrash.
//! * **Editable-text projection** — text-input roles
//!   ([`Role::TextInput`], [`Role::MultilineTextInput`],
//!   [`Role::SearchInput`], [`Role::PasswordInput`], and the other
//!   input-role family) project to real `<input>`/`<textarea>`
//!   elements rather than generic `<div>`s, so assistive technology
//!   receives native value, selection, and IME behaviour.
//!   [`Role::PasswordInput`] projects to `<input type="password">`;
//!   its value is written only into the input's own `value` property —
//!   never into `aria-valuetext`, text content, or any other DOM
//!   attribute. DOM `input`/`select` events on projected elements are
//!   translated back into [`Action::SetValue`] /
//!   [`Action::SetTextSelection`] requests.
//! * **Composite widgets** — [`Node::active_descendant`],
//!   [`Node::owns`], and [`Node::controls`] mirror to
//!   `aria-activedescendant`, `aria-owns`, and `aria-controls` for
//!   combobox/listbox/menu/tablist patterns. Containers using
//!   `aria-activedescendant` keep DOM focus themselves: their
//!   descendants are removed from the tab order entirely.
//! * **Roving tabindex** — composite containers
//!   (tablist/menu/menubar/radiogroup/toolbar/listbox/tree/grid) run
//!   roving tabindex: only the active item — the container's
//!   `active_descendant`, else the `selected` item, else the focused
//!   item, else the first focusable item — carries `tabindex="0"`;
//!   all other focusable members carry `tabindex="-1"`. Outside
//!   composites every focusable element stays in the tab order.
//! * **Opt-in activation** — the mirror is off by default. A visually
//!   hidden "Enable accessibility" button is appended to the document;
//!   a trusted click on it turns the bridge on. The programmatic path
//!   is [`set_enabled`](WebA11yBridge::set_enabled) /
//!   [`builder`](WebA11yBridge::builder). While disabled no per-node
//!   DOM is created; tree updates are accumulated and applied
//!   atomically on activation. The shared announcer exists regardless.
//! * **Live-region announcer** — a separate `aria-live="polite"`
//!   element; nodes carrying [`accesskit::Live`] emit their text there
//!   when it changes, and [`announce`](WebA11yBridge::announce) lets
//!   callers announce directly. The shared region is the *single*
//!   announcement channel: `aria-live` is deliberately not mirrored
//!   onto individual elements, which would double-announce the same
//!   change.
//! * **Trusted-event gate** — every DOM event closure ignores events
//!   with [`is_trusted()`](web_sys::Event::is_trusted) `== false`, so
//!   same-origin scripts cannot inject [`ActionRequest`]s through the
//!   mirror. The activation button likewise requires a trusted click.
//!
//! # IME boundary
//!
//! `martensite_window::web::HiddenImeInput` is a separate, deliberately
//! `aria-hidden="true"` `<input>` that exists solely to host IME
//! composition and clipboard keyboard paths for the canvas. It is not
//! part of this mirror and the two never register for each other's
//! role: the mirror's projected `<input>`/`<textarea>` elements are the
//! *semantic* editing surface exposed to AT, while the hidden IME input
//! is a transient DOM-focus holder. Keep
//! [`set_dom_focus_enabled`](WebA11yBridge::set_dom_focus_enabled)`(false)`
//! while a composition owns DOM focus on the overlay so a mirror-side
//! `focus()` call cannot steal it back and cancel the composition.
//!
//! # Deliberately out of scope
//!
//! Multi-`TreeId` sources (the bridge models a single tree;
//! `target_tree` is always [`TreeId::ROOT`]), `html_id`/`inner_html`
//! passthrough, and braille attributes. Everything else in the
//! AccessKit property surface — relations, position/set metadata,
//! `aria-sort`, `aria-current`, `aria-invalid`, `aria-orientation`,
//! `aria-keyshortcuts`, `lang`, `dir` — is mirrored.
//!
//! # Examples
//!
//! ```no_run
//! use martensite_access::web::WebA11yBridge;
//!
//! # fn example() -> Result<(), martensite_access::web::WebA11yError> {
//! let mut bridge = WebA11yBridge::new()?;
//! bridge.set_enabled(true); // or let the user hit the hidden button
//! bridge.set_action_handler(|request| {
//!     let _ = request.action;
//! });
//! bridge.announce("Ready");
//! # Ok(())
//! # }
//! ```

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use accesskit::{
    Action, ActionData, ActionRequest, AriaCurrent, AutoComplete, HasPopup, Invalid, Live, Node,
    NodeId, Orientation, Rect, Role, SortDirection, TextDirection, TextPosition, TextSelection,
    Toggled, TreeId, TreeUpdate,
};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{Document, HtmlElement, HtmlInputElement, HtmlTextAreaElement};

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
/// out of flow. Used for the shared announcer and the activation
/// button — mirror elements are positioned at real bounds instead.
const SR_ONLY: &str =
    "position:absolute;left:-10000px;top:auto;width:1px;height:1px;overflow:hidden;";

/// Base style for positioned mirror elements: absolutely positioned
/// (against the nearest positioned ancestor, i.e. the parent mirror
/// element), transparent, and pointer-transparent so real pointer input
/// keeps flowing to the canvas — AT `click()` synthesis still produces
/// trusted DOM click events.
const MIRROR_STYLE_BASE: &str =
    "position:absolute;margin:0;padding:0;border:0;opacity:0;pointer-events:none;";

/// The DOM element `id` assigned to every mirror element. Composite
/// attributes (`aria-activedescendant`, `aria-owns`, `aria-controls`,
/// `aria-labelledby`, …) reference these ids.
fn dom_id(id: u64) -> String {
    format!("martensite-a11y-{id}")
}

/// Joins a node-id relation list into a space-separated DOM idref list.
fn idrefs(ids: &[NodeId]) -> String {
    ids.iter()
        .map(|id| dom_id(id.0))
        .collect::<Vec<_>>()
        .join(" ")
}

/// The DOM projection a node occupies. Determined by role (and
/// `html_tag` for generic nodes) — a projection change rebuilds the
/// mirror element because `<input>`/`<textarea>`/`<div>` are
/// incompatible element kinds.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Projection {
    /// A generic element (default `<div>`, or [`Node::html_tag`] when
    /// the node supplies one).
    Generic,
    /// A single-line `<input>` with the given `type` token.
    Input(&'static str),
    /// A `<textarea>`.
    TextArea,
}

impl Projection {
    /// Whether this projection is an editable text surface carrying a
    /// native value/selection.
    fn is_text_editable(&self) -> bool {
        matches!(self, Projection::Input(_) | Projection::TextArea)
    }
}

/// Maps a text-input role to its DOM projection; every other role gets
/// [`Projection::Generic`]. [`Role::PasswordInput`] always projects to
/// `type="password"` so the browser applies native masking.
fn projection(role: Role) -> Projection {
    match role {
        Role::MultilineTextInput => Projection::TextArea,
        Role::PasswordInput => Projection::Input("password"),
        Role::SearchInput => Projection::Input("search"),
        Role::EmailInput => Projection::Input("email"),
        Role::UrlInput => Projection::Input("url"),
        Role::PhoneNumberInput => Projection::Input("tel"),
        Role::NumberInput => Projection::Input("number"),
        Role::DateInput => Projection::Input("date"),
        Role::DateTimeInput => Projection::Input("datetime-local"),
        Role::WeekInput => Projection::Input("week"),
        Role::MonthInput => Projection::Input("month"),
        Role::TimeInput => Projection::Input("time"),
        Role::TextInput | Role::EditableComboBox => Projection::Input("text"),
        _ => Projection::Generic,
    }
}

/// The tag name a mirror element is created with.
fn tag_name(node: &Node, projection: &Projection) -> String {
    match projection {
        Projection::Input(_) => "input".into(),
        Projection::TextArea => "textarea".into(),
        Projection::Generic => node.html_tag().unwrap_or("div").to_string(),
    }
}

/// Whether `role` is a composite container whose focusable members
/// share a single tab stop via roving tabindex (ARIA APG composite
/// widgets). Containers that instead carry
/// [`Node::active_descendant`] manage focus themselves and their
/// members leave the tab order — that case takes precedence over this
/// list.
fn roving_container(role: Role) -> bool {
    matches!(
        role,
        Role::TabList
            | Role::Menu
            | Role::MenuBar
            | Role::MenuListPopup
            | Role::RadioGroup
            | Role::Toolbar
            | Role::ListBox
            | Role::Tree
            | Role::TreeGrid
            | Role::Grid
    )
}

/// How a node's *parent* manages the tab stops of its members.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GroupMembership {
    /// Not inside a composite — normal sequential tab order.
    None,
    /// Parent uses `aria-activedescendant`: focus stays on the parent,
    /// members are never tabbable.
    Managed,
    /// Parent is a roving-tabindex composite: exactly one member is
    /// tabbable.
    Roving,
}

/// The `tabindex` a mirror element should carry.
///
/// * Non-focusable nodes get no `tabindex`.
/// * Members of an `aria-activedescendant` container get `-1`.
/// * Members of a roving container get `0` iff they are the group's
///   active item or hold tree focus, else `-1`.
/// * Every other focusable node gets `0`.
fn desired_tabindex(
    focusable: bool,
    focused: bool,
    group: GroupMembership,
    group_active: bool,
) -> Option<i8> {
    if !focusable {
        return None;
    }
    match group {
        GroupMembership::Managed => Some(-1),
        GroupMembership::Roving => Some(if focused || group_active { 0 } else { -1 }),
        GroupMembership::None => Some(0),
    }
}

/// Maps an AccessKit [`Role`] to the ARIA `role` token written to the
/// mirror element, or `None` when the role has no meaningful ARIA
/// analogue (the mirror element then carries no `role` attribute).
///
/// The mapping covers the full semantic range of `Role`; exotic
/// document-structure roles map to their nearest ARIA token, and
/// genuinely unmapped roles fall back to `generic`-style anonymity
/// rather than a wrong token. Elements projected to real
/// `<input>`/`<textarea>` tags mostly rely on implicit semantics — the
/// `role` attribute is only asserted when it adds information (e.g.
/// `combobox`, `aria-multiline`).
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

/// The pure, DOM-free description of everything [`Inner`] writes to a
/// mirror element for one node — attributes (sorted), leaf text
/// content, and the projected input value/selection. Comparing a
/// freshly computed spec to the last applied one is what suppresses
/// redundant DOM writes on unchanged nodes.
#[derive(Clone, Debug, Default, PartialEq)]
struct NodeSpec {
    /// ARIA/native attributes, sorted by name.
    attrs: Vec<(String, String)>,
    /// Text content for leaf roles (`Label`/`TextRun`/`Paragraph`).
    text: Option<String>,
    /// The `value` property for projected `<input>`/`<textarea>`
    /// elements. Written via `set_value` (the DOM *property*, never an
    /// attribute) — this is also the only place a password value may
    /// flow; attributes and text content never carry it.
    value: Option<String>,
    /// Selection `(start, end)` in UTF-16 code units for projected
    /// elements, mirrored via `setSelectionRange`.
    selection: Option<(u32, u32)>,
}

/// Computes the DOM-side attribute/value spec for `node`.
///
/// `setsize` is the parent's `size_of_set` (re-published on each item
/// as `aria-setsize`, matching ARIA's item-scoped placement); `id` is
/// the node's own id for the generated element `id` and relation
/// idrefs.
fn node_spec(id: u64, node: &Node, proj: &Projection, setsize: Option<usize>) -> NodeSpec {
    // Raw (name, optional value) pairs; `None` values are dropped and
    // the result sorted so spec comparison is order-stable.
    let mut raw: Vec<(&'static str, Option<String>)> = Vec::new();
    macro_rules! attr {
        ($k:expr, $v:expr) => {
            raw.push(($k, Some($v)))
        };
        (opt $k:expr, $v:expr) => {
            raw.push(($k, $v))
        };
    }

    attr!("id", dom_id(id));
    match proj {
        Projection::Input(ty) => {
            attr!("type", (*ty).to_string());
            // Implicit input semantics cover the input family; only the
            // editable combobox needs an explicit role to reach
            // `combobox` from a real <input>.
            if node.role() == Role::EditableComboBox {
                attr!("role", "combobox".to_string());
            }
        }
        Projection::TextArea => {
            attr!("role", "textbox".to_string());
            attr!("aria-multiline", "true".to_string());
        }
        Projection::Generic => attr!(opt "role", aria_role(node.role()).map(str::to_string)),
    }
    attr!(opt "aria-label", node.label().map(str::to_string));
    attr!(opt "aria-description", node.description().map(str::to_string));
    // `aria-valuetext` only on generic elements: projected inputs carry
    // the value natively (and for passwords the value may never leave
    // `input.value`).
    if *proj == Projection::Generic {
        attr!(opt "aria-valuetext", node.value().map(str::to_string));
    }
    attr!("aria-disabled", bool_str(node.is_disabled()).to_string());
    attr!("aria-hidden", bool_str(node.is_hidden()).to_string());
    attr!(opt "aria-selected", node.is_selected().map(|s| bool_str(s).to_string()));
    attr!(opt "aria-expanded", node.is_expanded().map(|e| bool_str(e).to_string()));
    attr!(opt "aria-checked", node.toggled().map(|t| match t {
        Toggled::True => "true",
        Toggled::Mixed => "mixed",
        Toggled::False => "false",
    }
    .to_string()));
    // AccessKit level/posinset/row/col indices are zero-based; ARIA is
    // one-based.
    attr!(opt "aria-level", node.level().map(|l| (l + 1).to_string()));
    attr!(opt "aria-posinset", node.position_in_set().map(|p| (p + 1).to_string()));
    attr!(opt "aria-setsize", setsize.map(|s| s.to_string()));
    attr!(opt "aria-rowindex", node.row_index().map(|r| (r + 1).to_string()));
    attr!(opt "aria-colindex", node.column_index().map(|c| (c + 1).to_string()));
    attr!(opt "aria-rowcount", node.row_count().map(|r| r.to_string()));
    attr!(opt "aria-colcount", node.column_count().map(|c| c.to_string()));
    attr!(opt "aria-rowspan", node.row_span().map(|r| r.to_string()));
    attr!(opt "aria-colspan", node.column_span().map(|c| c.to_string()));
    attr!(opt "aria-rowindextext", node.row_index_text().map(str::to_string));
    attr!(opt "aria-colindextext", node.column_index_text().map(str::to_string));
    attr!(opt "aria-valuenow", node.numeric_value().map(|v| v.to_string()));
    attr!(opt "aria-valuemin", node.min_numeric_value().map(|v| v.to_string()));
    attr!(opt "aria-valuemax", node.max_numeric_value().map(|v| v.to_string()));
    // `node.live()`/`is_live_atomic()` are deliberately NOT mirrored:
    // live-region text changes are announced through the shared
    // announcer element, the single announcement channel. Mirroring
    // `aria-live` here too would double-announce every change.
    if node.is_busy() {
        attr!("aria-busy", "true".to_string());
    }
    if node.is_modal() {
        attr!("aria-modal", "true".to_string());
    }
    if node.is_required() {
        attr!("aria-required", "true".to_string());
    }
    if node.is_read_only() {
        attr!("aria-readonly", "true".to_string());
    }
    if node.is_multiselectable() {
        attr!("aria-multiselectable", "true".to_string());
    }
    attr!(opt "aria-orientation", node.orientation().map(|o| match o {
        Orientation::Horizontal => "horizontal",
        Orientation::Vertical => "vertical",
    }
    .to_string()));
    attr!(opt "aria-current", node.aria_current().map(|c| match c {
        AriaCurrent::False => "false",
        AriaCurrent::True => "true",
        AriaCurrent::Page => "page",
        AriaCurrent::Step => "step",
        AriaCurrent::Location => "location",
        AriaCurrent::Date => "date",
        AriaCurrent::Time => "time",
    }
    .to_string()));
    attr!(opt "aria-invalid", node.invalid().map(|i| match i {
        Invalid::True => "true",
        Invalid::Grammar => "grammar",
        Invalid::Spelling => "spelling",
    }
    .to_string()));
    attr!(opt "aria-haspopup", node.has_popup().map(|p| match p {
        HasPopup::Menu => "menu",
        HasPopup::Listbox => "listbox",
        HasPopup::Tree => "tree",
        HasPopup::Grid => "grid",
        HasPopup::Dialog => "dialog",
    }
    .to_string()));
    attr!(opt "aria-autocomplete", node.auto_complete().map(|a| match a {
        AutoComplete::Inline => "inline",
        AutoComplete::List => "list",
        AutoComplete::Both => "both",
    }
    .to_string()));
    attr!(opt "aria-sort", node.sort_direction().map(|s| match s {
        SortDirection::Ascending => "ascending",
        SortDirection::Descending => "descending",
        SortDirection::Other => "other",
    }
    .to_string()));
    attr!(opt "dir", node.text_direction().and_then(|d| match d {
        TextDirection::LeftToRight => Some("ltr".to_string()),
        TextDirection::RightToLeft => Some("rtl".to_string()),
        _ => None,
    }));
    attr!(opt "aria-roledescription", node.role_description().map(str::to_string));
    attr!(opt "aria-keyshortcuts", node.keyboard_shortcut().map(str::to_string));
    attr!(opt "lang", node.language().map(str::to_string));
    attr!(opt "accesskey", node.access_key().map(str::to_string));
    if proj.is_text_editable() {
        attr!(opt "placeholder", node.placeholder().map(str::to_string));
    } else {
        attr!(opt "aria-placeholder", node.placeholder().map(str::to_string));
    }
    // Composite relations → idrefs on the generated element ids.
    attr!(opt "aria-activedescendant", node.active_descendant().map(|d| dom_id(d.0)));
    if !node.owns().is_empty() {
        attr!("aria-owns", idrefs(node.owns()));
    }
    if !node.controls().is_empty() {
        attr!("aria-controls", idrefs(node.controls()));
    }
    if !node.labelled_by().is_empty() {
        attr!("aria-labelledby", idrefs(node.labelled_by()));
    }
    if !node.described_by().is_empty() {
        attr!("aria-describedby", idrefs(node.described_by()));
    }
    if !node.details().is_empty() {
        attr!("aria-details", idrefs(node.details()));
    }
    if !node.flow_to().is_empty() {
        attr!("aria-flowto", idrefs(node.flow_to()));
    }
    attr!(opt "aria-errormessage", node.error_message().map(|e| dom_id(e.0)));
    attr!(opt "class", node.class_name().map(str::to_string));
    let mut attrs: Vec<(String, String)> = raw
        .into_iter()
        .filter_map(|(k, v)| v.map(|v| (k.to_string(), v)))
        .collect();
    attrs.sort();

    let is_text_leaf = matches!(node.role(), Role::Label | Role::TextRun | Role::Paragraph);
    let text = if is_text_leaf && *proj == Projection::Generic {
        node_text(node)
    } else {
        None
    };
    let value = if proj.is_text_editable() {
        Some(node.value().unwrap_or_default().to_string())
    } else {
        None
    };
    let selection = if proj.is_text_editable() {
        node.text_selection().map(|sel| {
            let (a, f) = (sel.anchor.character_index, sel.focus.character_index);
            (a.min(f) as u32, a.max(f) as u32)
        })
    } else {
        None
    };

    NodeSpec {
        attrs,
        text,
        value,
        selection,
    }
}

/// Attribute writes needed to turn `old` into `new` (both sorted):
/// `(remove_names, set_pairs)`. Pure so the diff policy is testable
/// without a DOM.
fn attr_ops<'a>(
    old: &'a [(String, String)],
    new: &'a [(String, String)],
) -> (Vec<&'a str>, Vec<(&'a str, &'a str)>) {
    let mut remove = Vec::new();
    let mut set = Vec::new();
    for (k, _) in old {
        if !new.iter().any(|(nk, _)| nk == k) {
            remove.push(k.as_str());
        }
    }
    for (k, v) in new {
        match old.iter().find(|(ok, _)| ok == k) {
            Some((_, ov)) if ov == v => {}
            _ => set.push((k.as_str(), v.as_str())),
        }
    }
    (remove, set)
}

/// The `style` attribute for a mirror element.
///
/// `origin` is the element's absolute-position anchor: the parent's
/// resolved absolute origin, since `position:absolute` resolves
/// against the nearest positioned ancestor (the parent mirror
/// element). Nodes without positive-size bounds collapse to a
/// zero-size element at that origin — still present in the a11y tree.
fn mirror_style(bounds: Option<Rect>, origin: (f64, f64), clip: bool) -> String {
    match bounds {
        Some(b) if b.width() > 0.0 && b.height() > 0.0 => {
            let overflow = if clip { "hidden" } else { "visible" };
            format!(
                "{MIRROR_STYLE_BASE}left:{}px;top:{}px;width:{}px;height:{}px;overflow:{overflow};",
                b.x0 - origin.0,
                b.y0 - origin.1,
                b.width(),
                b.height(),
            )
        }
        _ => format!("{MIRROR_STYLE_BASE}left:0;top:0;width:0;height:0;overflow:visible;"),
    }
}

/// Per-node mirror state.
struct MirrorEntry {
    element: HtmlElement,
    /// The element tag it was created with (`div`/`input`/`textarea`/
    /// `html_tag`) — a change rebuilds the element.
    tag: String,
    /// The projection the element was built for.
    projection: Projection,
    /// Child node ids as of the last update that mentioned this parent.
    children: Vec<u64>,
    /// Parent node id (`None` for the root, which hangs off the
    /// container).
    parent: Option<u64>,
    /// Last text announced through the shared live-region announcer.
    live_text: Option<String>,
    /// Whether the node supports `Action::Focus` — governs `tabindex`.
    focusable: bool,
    /// The node's role, kept for roving-container membership checks.
    role: Role,
    /// `is_selected` as of the last apply — used by roving-tabindex to
    /// pick the active item.
    selected: Option<bool>,
    /// `active_descendant` as of the last apply — marks the entry as a
    /// focus-managing composite and names its managed member.
    active_descendant: Option<u64>,
    /// `size_of_set` for republishing `aria-setsize` on items.
    size_of_set: Option<usize>,
    /// Whether the node clips its children (`overflow: hidden`).
    clips_children: bool,
    /// Node bounds in window coordinates.
    bounds: Option<Rect>,
    /// The last spec applied to the DOM (attribute-write diffing).
    applied_attrs: Vec<(String, String)>,
    /// The `style` attribute last written, for bounds-write diffing.
    applied_style: Option<String>,
    /// The `tabindex` currently on the element (`None` = attribute
    /// absent).
    applied_tabindex: Option<i8>,
    /// The text content currently on the element.
    applied_text: Option<String>,
    /// The `value` property currently on a projected input.
    applied_value: Option<String>,
    /// The selection range currently on a projected input.
    applied_selection: Option<(u32, u32)>,
    /// DOM listeners kept alive for the element's lifetime.
    _closures: Vec<EventClosure>,
}

/// A DOM event listener closure kept alive for an element's lifetime.
type EventClosure = Closure<dyn FnMut(web_sys::Event)>;

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

/// Reads `value` off a projected element, whichever input kind it is.
fn input_value(element: &HtmlElement) -> Option<String> {
    element
        .dyn_ref::<HtmlInputElement>()
        .map(|i| i.value())
        .or_else(|| element.dyn_ref::<HtmlTextAreaElement>().map(|t| t.value()))
}

/// Writes `value` onto a projected element's `value` *property* — the
/// property is not serialized into markup, which is why this is the
/// only channel allowed to carry a password.
fn set_input_value(element: &HtmlElement, value: &str) {
    if let Some(input) = element.dyn_ref::<HtmlInputElement>() {
        input.set_value(value);
    } else if let Some(area) = element.dyn_ref::<HtmlTextAreaElement>() {
        area.set_value(value);
    }
}

/// Reads `(anchor, focus)` selection offsets off a projected element.
fn input_selection(element: &HtmlElement) -> Option<(u32, u32)> {
    let read = |start: Option<u32>, end: Option<u32>, backward: bool| -> Option<(u32, u32)> {
        let (s, e) = (start?, end?);
        if backward {
            Some((e, s))
        } else {
            Some((s, e))
        }
    };
    if let Some(input) = element.dyn_ref::<HtmlInputElement>() {
        let start = input.selection_start().ok().flatten();
        let end = input.selection_end().ok().flatten();
        let backward = input.selection_direction().ok().flatten().as_deref() == Some("backward");
        read(start, end, backward)
    } else if let Some(area) = element.dyn_ref::<HtmlTextAreaElement>() {
        let start = area.selection_start().ok().flatten();
        let end = area.selection_end().ok().flatten();
        let backward = area.selection_direction().ok().flatten().as_deref() == Some("backward");
        read(start, end, backward)
    } else {
        None
    }
}

/// Applies a mirrored selection range to a projected element.
fn set_input_selection(element: &HtmlElement, start: u32, end: u32) {
    if let Some(input) = element.dyn_ref::<HtmlInputElement>() {
        let _ = input.set_selection_range(start, end);
    } else if let Some(area) = element.dyn_ref::<HtmlTextAreaElement>() {
        let _ = area.set_selection_range(start, end);
    }
}

/// Dispatches `request` through the handler slot, taking it out first
/// so user code runs with no borrow held (see [`ActionHandler`]).
fn dispatch_action(handler: &ActionHandler, request: ActionRequest) {
    let mut cb = handler.borrow_mut().take();
    if let Some(f) = cb.as_mut() {
        f(request);
    }
    let mut slot = handler.borrow_mut();
    if slot.is_none() {
        *slot = cb;
    }
}

/// Attaches the mirror's event listeners to `element`. Every closure
/// first rejects untrusted events (`event.is_trusted() == false`) so
/// synthetic script-dispatched events cannot inject action requests.
fn attach_listeners(
    element: &HtmlElement,
    id: u64,
    text_editable: bool,
    handler: &ActionHandler,
) -> Result<Vec<EventClosure>, WebA11yError> {
    let mut closures: Vec<EventClosure> = Vec::new();
    for (kind, action) in [
        ("click", Action::Click),
        ("focus", Action::Focus),
        ("blur", Action::Blur),
    ] {
        let handler = Rc::clone(handler);
        let closure = Closure::wrap(Box::new(move |event: web_sys::Event| {
            if !event.is_trusted() {
                return;
            }
            dispatch_action(
                &handler,
                ActionRequest {
                    action,
                    // The mirror models a single tree; multi-tree
                    // sources are out of scope, so `target_tree` is
                    // always ROOT.
                    target_tree: TreeId::ROOT,
                    target_node: NodeId(id),
                    data: None,
                },
            );
        }) as Box<dyn FnMut(web_sys::Event)>);
        element
            .add_event_listener_with_callback(kind, closure.as_ref().unchecked_ref())
            .map_err(|e| WebA11yError::from_js(&e))?;
        closures.push(closure);
    }
    if text_editable {
        // `input` → SetValue with the live DOM value.
        let input_el = element.clone();
        let input_handler = Rc::clone(handler);
        let input_closure = Closure::wrap(Box::new(move |event: web_sys::Event| {
            if !event.is_trusted() {
                return;
            }
            let Some(value) = input_value(&input_el) else {
                return;
            };
            dispatch_action(
                &input_handler,
                ActionRequest {
                    action: Action::SetValue,
                    target_tree: TreeId::ROOT,
                    target_node: NodeId(id),
                    data: Some(ActionData::Value(value.into_boxed_str())),
                },
            );
        }) as Box<dyn FnMut(web_sys::Event)>);
        element
            .add_event_listener_with_callback("input", input_closure.as_ref().unchecked_ref())
            .map_err(|e| WebA11yError::from_js(&e))?;
        closures.push(input_closure);

        // `select` → SetTextSelection with the live DOM selection.
        let select_el = element.clone();
        let select_handler = Rc::clone(handler);
        let select_closure = Closure::wrap(Box::new(move |event: web_sys::Event| {
            if !event.is_trusted() {
                return;
            }
            let Some((anchor, focus)) = input_selection(&select_el) else {
                return;
            };
            let position = |character_index: u32| TextPosition {
                // For projected single-value controls the position's
                // owning node is the control itself.
                node: NodeId(id),
                character_index: character_index as usize,
            };
            dispatch_action(
                &select_handler,
                ActionRequest {
                    action: Action::SetTextSelection,
                    target_tree: TreeId::ROOT,
                    target_node: NodeId(id),
                    data: Some(ActionData::SetTextSelection(TextSelection {
                        anchor: position(anchor),
                        focus: position(focus),
                    })),
                },
            );
        }) as Box<dyn FnMut(web_sys::Event)>);
        element
            .add_event_listener_with_callback("select", select_closure.as_ref().unchecked_ref())
            .map_err(|e| WebA11yError::from_js(&e))?;
        closures.push(select_closure);
    }
    Ok(closures)
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
/// bridge.set_enabled(true);
/// bridge.announce("Application ready");
/// # Ok(())
/// # }
/// ```
pub struct WebA11yBridge {
    /// The positioned container holding the semantic mirror subtree.
    container: HtmlElement,
    /// The shared `aria-live` announcer element.
    live: HtmlElement,
    /// The visually hidden activation button ("Enable accessibility").
    activation: HtmlElement,
    /// Mutable mirror state behind a shared cell so the activation
    /// button's click closure can turn the bridge on without `&mut`
    /// access to the `WebA11yBridge` itself.
    inner: Rc<RefCell<Inner>>,
    /// Handler slot for action requests from DOM events (see
    /// [`ActionHandler`] for why it is an `Option` cell).
    action_handler: ActionHandler,
    /// The activation button's click closure.
    _activation_closure: EventClosure,
}

/// The bridge's mutable mirror state.
struct Inner {
    /// The positioned container holding the semantic mirror subtree
    /// (clone of the bridge's handle).
    container: HtmlElement,
    /// The shared `aria-live` announcer element (clone of the bridge's
    /// handle).
    live: HtmlElement,
    /// Handler slot for action requests — the same `Rc` cell the
    /// bridge's `action_handler` field points at.
    action_handler: ActionHandler,
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
    /// When `false`, `set_focus` updates `tabindex` but does not call
    /// `element.focus()` — used to suspend DOM focus mirroring while an
    /// IME composition holds focus on the hidden input overlay.
    dom_focus_enabled: bool,
    /// Whether the mirror is active. While `false`, updates are
    /// accumulated into `pending` and no per-node DOM exists.
    enabled: bool,
    /// Tree updates merged while disabled, applied atomically on
    /// activation so the first enabled frame materializes the complete
    /// tree rather than only the nodes changed since enablement.
    pending: Option<TreeUpdate>,
    /// The document used to create elements.
    document: Document,
}

impl std::fmt::Debug for WebA11yBridge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebA11yBridge")
            .field("mirrored_nodes", &self.inner.borrow().entries.len())
            .field("enabled", &self.inner.borrow().enabled)
            .finish()
    }
}

impl WebA11yBridge {
    /// Creates the bridge: the positioned mirror container, an
    /// `aria-live="polite"` announcer, and the visually hidden
    /// "Enable accessibility" activation button, all appended to
    /// `document.body`. The mirror starts **disabled** — see
    /// [`set_enabled`](Self::set_enabled).
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
        Self::builder().build()
    }

    /// Returns a builder for a bridge with custom activation settings.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_access::web::WebA11yBridge;
    ///
    /// # fn example() -> Result<(), martensite_access::web::WebA11yError> {
    /// let bridge = WebA11yBridge::builder()
    ///     .enabled(true)
    ///     .activation_label("Enable screen reader support")
    ///     .build()?;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn builder() -> WebA11yBridgeBuilder {
        WebA11yBridgeBuilder::default()
    }

    /// Whether the mirror is currently active. While disabled,
    /// [`update`](Self::update) accumulates tree state without creating
    /// per-node DOM.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn example(bridge: &mut martensite_access::web::WebA11yBridge) {
    /// assert!(!bridge.is_enabled());
    /// bridge.set_enabled(true);
    /// assert!(bridge.is_enabled());
    /// # }
    /// ```
    #[must_use]
    pub fn is_enabled(&self) -> bool {
        self.inner.borrow().enabled
    }

    /// Enables or disables the mirror — the programmatic equivalent of
    /// the hidden activation button.
    ///
    /// Enabling hides the activation button and atomically applies any
    /// [`TreeUpdate`]s accumulated while disabled; disabling removes
    /// all mirror DOM and shows the button again.
    ///
    /// # Errors
    ///
    /// Returns [`WebA11yError`] if applying the accumulated tree fails.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn example(
    /// #     bridge: &mut martensite_access::web::WebA11yBridge,
    /// # ) -> Result<(), martensite_access::web::WebA11yError> {
    /// bridge.set_enabled(true)?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn set_enabled(&mut self, enabled: bool) -> Result<(), WebA11yError> {
        let (focus, result) = {
            let mut inner = self.inner.borrow_mut();
            if enabled {
                match inner.enable() {
                    Ok(focus) => (focus, Ok(())),
                    Err(e) => (None, Err(e)),
                }
            } else {
                inner.disable();
                (None, Ok(()))
            }
        };
        if enabled {
            let _ = self.activation.set_attribute("hidden", "");
        } else {
            let _ = self.activation.remove_attribute("hidden");
        }
        // `focus()` is called only after the inner borrow is released:
        // it dispatches the DOM `focus` event synchronously, and the
        // resulting action handler may call back into the bridge.
        if let Some(el) = focus {
            el.focus().ok();
        }
        result
    }

    /// Installs the handler receiving [`ActionRequest`]s generated when
    /// AT interacts with the mirror (DOM `click` → `Action::Click`,
    /// `focus` → `Action::Focus`, `blur` → `Action::Blur`, and on
    /// projected inputs `input` → `Action::SetValue`, `select` →
    /// `Action::SetTextSelection`). Untrusted events are dropped
    /// before reaching the handler.
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

    /// Returns the mirror container element (for tests/inspection).
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
    pub fn container(&self) -> HtmlElement {
        self.container.clone()
    }

    /// Returns the number of nodes currently mirrored (always `0` while
    /// disabled).
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
        self.inner.borrow().entries.len()
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
        self.inner.borrow().focused.map(NodeId)
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
        announce_through(&self.live, text);
    }

    /// Applies a [`TreeUpdate`]: creates/updates mirror elements,
    /// re-parents DOM children to match `Node::children`, removes mirror
    /// elements for nodes dropped from the tree, mirrors `live` nodes
    /// into the announcer, refreshes positions/tabindex, and applies
    /// `update.focus`.
    ///
    /// While the bridge is disabled the update is merged into a pending
    /// tree that is applied atomically when the bridge is next enabled.
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
        let focus = {
            let mut inner = self.inner.borrow_mut();
            inner.ingest(update)?
        };
        if let Some(el) = focus {
            el.focus().ok();
        }
        Ok(())
    }

    /// Suspends or resumes DOM focus mirroring.
    ///
    /// While `enabled` is `false`, `set_focus` still tracks the focused
    /// node and maintains roving `tabindex`, but does not call
    /// `element.focus()`. Use this when another element must keep DOM
    /// focus — e.g. a `martensite_window::web::HiddenImeInput` overlay
    /// holding focus during an IME composition, where a mirror-side
    /// `focus()` would steal focus back and cancel the in-flight
    /// composition. The default is `true`.
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
        self.inner.borrow_mut().dom_focus_enabled = enabled;
    }

    /// Mirrors `node_id` as the DOM-focused element: `tabindex="0"`
    /// (subject to composite roving/managed rules) and `element.focus()`.
    /// When the focused node is a managed member of an
    /// `aria-activedescendant` composite, DOM focus lands on the
    /// *container* element instead, matching the ARIA pattern.
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
        let focus = { self.inner.borrow_mut().set_focus(node_id) };
        if let Some(el) = focus {
            el.focus().ok();
        }
    }

    /// Removes all mirrored elements (e.g. on tree teardown). Does not
    /// change the enabled state.
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
        self.inner.borrow_mut().clear();
    }
}

/// Builder for [`WebA11yBridge`], returned by
/// [`WebA11yBridge::builder`].
///
/// # Examples
///
/// ```no_run
/// use martensite_access::web::WebA11yBridge;
///
/// # fn example() -> Result<(), martensite_access::web::WebA11yError> {
/// let bridge = WebA11yBridge::builder()
///     .enabled(true)
///     .build()?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct WebA11yBridgeBuilder {
    enabled: bool,
    activation_label: String,
}

impl Default for WebA11yBridgeBuilder {
    fn default() -> Self {
        Self {
            enabled: false,
            activation_label: "Enable accessibility".to_string(),
        }
    }
}

impl WebA11yBridgeBuilder {
    /// Whether the mirror starts enabled (default `false` — the hidden
    /// activation button is shown instead).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_access::web::WebA11yBridge;
    ///
    /// # fn example() -> Result<(), martensite_access::web::WebA11yError> {
    /// let bridge = WebA11yBridge::builder().enabled(true).build()?;
    /// assert!(bridge.is_enabled());
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// The label text of the hidden activation button (default
    /// `"Enable accessibility"`).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_access::web::WebA11yBridge;
    ///
    /// # fn example() -> Result<(), martensite_access::web::WebA11yError> {
    /// let bridge = WebA11yBridge::builder()
    ///     .activation_label("Enable screen reader support")
    ///     .build()?;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn activation_label(mut self, label: impl Into<String>) -> Self {
        self.activation_label = label.into();
        self
    }

    /// Builds the bridge and appends the container, announcer, and
    /// activation button to `document.body`.
    ///
    /// # Errors
    ///
    /// Returns [`WebA11yError`] when there is no DOM document/body or
    /// element creation fails.
    pub fn build(self) -> Result<WebA11yBridge, WebA11yError> {
        let document = document()?;
        let body = document
            .body()
            .ok_or_else(|| WebA11yError::new("document has no <body>"))?;

        let container: HtmlElement = document
            .create_element("div")
            .map_err(|e| WebA11yError::from_js(&e))?
            .unchecked_into();
        // Zero-size positioned root at the viewport origin: every
        // mirror element resolves its absolute position against this
        // origin (or a positioned mirror ancestor).
        container
            .set_attribute(
                "style",
                "position:absolute;left:0;top:0;width:0;height:0;overflow:visible;",
            )
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

        // The Flutter-style activation affordance: a visually hidden
        // but AT-visible button a screen-reader user lands on to opt
        // the mirror in.
        let activation: HtmlElement = document
            .create_element("button")
            .map_err(|e| WebA11yError::from_js(&e))?
            .unchecked_into();
        activation
            .set_attribute("style", SR_ONLY)
            .map_err(|e| WebA11yError::from_js(&e))?;
        activation
            .set_attribute("type", "button")
            .map_err(|e| WebA11yError::from_js(&e))?;
        activation.set_text_content(Some(&self.activation_label));
        if self.enabled {
            activation
                .set_attribute("hidden", "")
                .map_err(|e| WebA11yError::from_js(&e))?;
        }

        let action_handler: ActionHandler = Rc::new(RefCell::new(None));
        let inner = Rc::new(RefCell::new(Inner {
            container: container.clone(),
            live: live.clone(),
            action_handler: Rc::clone(&action_handler),
            entries: HashMap::new(),
            focused: None,
            focused_element: None,
            root: None,
            dom_focus_enabled: true,
            enabled: self.enabled,
            pending: None,
            document,
        }));

        // The activation closure enables the mirror through a `Weak`
        // upgrade, so a dropped bridge cleanly deactivates the button.
        // `Inner::enable` returns the element to focus, applied after
        // the borrow is released (focus events dispatch synchronously).
        let weak = Rc::downgrade(&inner);
        let activation_for_closure = activation.clone();
        let activation_closure = Closure::wrap(Box::new(move |event: web_sys::Event| {
            if !event.is_trusted() {
                return;
            }
            let Some(inner) = weak.upgrade() else {
                return;
            };
            let focus = {
                let mut inner = inner.borrow_mut();
                match inner.enable() {
                    Ok(focus) => focus,
                    Err(e) => {
                        tracing::warn!("a11y mirror activation failed: {e}");
                        None
                    }
                }
            };
            let _ = activation_for_closure.set_attribute("hidden", "");
            if let Some(el) = focus {
                el.focus().ok();
            }
        }) as Box<dyn FnMut(web_sys::Event)>);
        activation
            .add_event_listener_with_callback("click", activation_closure.as_ref().unchecked_ref())
            .map_err(|e| WebA11yError::from_js(&e))?;

        body.append_child(&container)
            .map_err(|e| WebA11yError::from_js(&e))?;
        body.append_child(&live)
            .map_err(|e| WebA11yError::from_js(&e))?;
        body.append_child(&activation)
            .map_err(|e| WebA11yError::from_js(&e))?;

        Ok(WebA11yBridge {
            container,
            live,
            activation,
            inner,
            action_handler,
            _activation_closure: activation_closure,
        })
    }
}

impl Inner {
    /// Turns the mirror on: applies the merged pending tree (if any)
    /// and returns the element that should take DOM focus afterwards.
    fn enable(&mut self) -> Result<Option<HtmlElement>, WebA11yError> {
        if self.enabled {
            return Ok(None);
        }
        self.enabled = true;
        match self.pending.take() {
            Some(pending) => self.apply_update(&pending),
            None => Ok(None),
        }
    }

    /// Turns the mirror off: removes all per-node DOM and forgets
    /// pending state.
    fn disable(&mut self) {
        self.enabled = false;
        self.pending = None;
        self.clear();
    }

    /// Removes all mirrored elements without changing the enabled state.
    fn clear(&mut self) {
        for (_, entry) in self.entries.drain() {
            entry.element.remove();
        }
        self.focused = None;
        self.focused_element = None;
        self.root = None;
    }

    /// Routes an incoming [`TreeUpdate`]: merged into `pending` while
    /// disabled, applied immediately (after any pending tree) while
    /// enabled. Returns the element to focus afterwards.
    fn ingest(&mut self, update: &TreeUpdate) -> Result<Option<HtmlElement>, WebA11yError> {
        if !self.enabled {
            match &mut self.pending {
                Some(p) => {
                    p.nodes.extend(update.nodes.iter().cloned());
                    if update.tree.is_some() {
                        p.tree = update.tree.clone();
                    }
                    p.focus = update.focus;
                }
                None => self.pending = Some(update.clone()),
            }
            return Ok(None);
        }
        if let Some(pending) = self.pending.take() {
            self.apply_update(&pending)?;
        }
        self.apply_update(update)
    }

    /// Applies a [`TreeUpdate`] to the mirror DOM and returns the
    /// element to focus afterwards (if focus changed and DOM focus is
    /// enabled).
    fn apply_update(&mut self, update: &TreeUpdate) -> Result<Option<HtmlElement>, WebA11yError> {
        // Phase 1: create/update elements, apply the diffed spec, and
        // collect the new parent→child edges plus live-region
        // announcements for after the borrow on `entries` ends.
        let mut new_children: HashMap<u64, Vec<u64>> = HashMap::new();
        let mut announcements: Vec<(String, Live)> = Vec::new();
        for (node_id, node) in &update.nodes {
            let id = node_id.0;
            // `aria-setsize` is item-scoped in ARIA but container-scoped
            // in AccessKit — republish the stored parent value.
            let parent_setsize = self
                .entries
                .get(&id)
                .and_then(|e| e.parent)
                .and_then(|p| self.entries.get(&p))
                .and_then(|pe| pe.size_of_set);
            let proj = projection(node.role());
            let spec = node_spec(id, node, &proj, parent_setsize);
            let entry = self.entry_for(id, node, proj)?;
            Self::apply_spec(entry, &spec)?;
            entry.focusable = node.supports_action(Action::Focus);
            entry.role = node.role();
            entry.selected = node.is_selected();
            entry.active_descendant = node.active_descendant().map(|d| d.0);
            entry.size_of_set = node.size_of_set();
            entry.clips_children = node.clips_children();
            entry.bounds = node.bounds();
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
            announce_through(&self.live, &text);
        }
        // Restore the resting level so a finished assertive announcement
        // doesn't leave the region stuck on assertive for later
        // `announce` calls.
        let _ = self.live.set_attribute("aria-live", "polite");

        // Phase 2: rewire parent→child edges and record each child's
        // parent for roving/managed-group and positioning decisions.
        for (parent_id, children) in &new_children {
            for &child_id in children {
                if let Some(child_entry) = self.entries.get_mut(&child_id) {
                    child_entry.parent = Some(*parent_id);
                }
                let child_element = self.entries.get(&child_id).map(|e| e.element.clone());
                if let (Some(parent_entry), Some(child_element)) =
                    (self.entries.get(parent_id), child_element)
                {
                    parent_entry
                        .element
                        .append_child(&child_element)
                        .map_err(|e| WebA11yError::from_js(&e))?;
                }
            }
        }

        // The root node hangs directly off the container.
        if let Some(tree) = &update.tree {
            if let Some(root_entry) = self.entries.get_mut(&tree.root.0) {
                root_entry.parent = None;
                let element = root_entry.element.clone();
                self.container
                    .append_child(&element)
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
        let mut claimed: HashSet<u64> = HashSet::new();
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

        // Phase 4: bounds positioning and focus/tabindex. Both passes
        // write to the DOM only when the computed value differs.
        self.refresh_positions();
        Ok(self.set_focus(update.focus))
    }

    /// Mirrors `focus` as the DOM-focused element, maintaining roving
    /// tabindex. Returns the element to `focus()` (called by the outer
    /// bridge only after the inner borrow is released), or `None`.
    fn set_focus(&mut self, node_id: NodeId) -> Option<HtmlElement> {
        self.focused = Some(node_id.0);
        self.refresh_tabindex();
        if !self.dom_focus_enabled {
            self.focused_element = None;
            return None;
        }
        let target = self.dom_focus_target(node_id.0);
        let needs_focus = target.is_some() && self.focused_element != target;
        self.focused_element = target.clone();
        if needs_focus {
            target
        } else {
            None
        }
    }

    /// The element a tree-focus on `id` should land on: the node's own
    /// element, or — when the node is a managed member of an
    /// `aria-activedescendant` composite — the container element, which
    /// is the focusable surface in that ARIA pattern.
    fn dom_focus_target(&self, id: u64) -> Option<HtmlElement> {
        let entry = self.entries.get(&id)?;
        if let Some(parent) = entry.parent.and_then(|p| self.entries.get(&p)) {
            if parent.active_descendant.is_some() {
                return Some(parent.element.clone());
            }
        }
        Some(entry.element.clone())
    }

    /// Rewrites every element's `tabindex` per the composite rules:
    /// members of `aria-activedescendant` containers leave the tab
    /// order; members of roving containers carry `0` only when they are
    /// the group's active item; all other focusable elements carry `0`.
    fn refresh_tabindex(&mut self) {
        // Snapshot parent context (shared borrows) before the mutable
        // pass.
        let parents: HashMap<u64, (Role, bool)> = self
            .entries
            .iter()
            .map(|(id, e)| (*id, (e.role, e.active_descendant.is_some())))
            .collect();
        let parent_of: HashMap<u64, u64> = self
            .entries
            .iter()
            .filter_map(|(id, e)| e.parent.map(|p| (*id, p)))
            .collect();
        // The active item of each roving container.
        let mut active_of: HashMap<u64, u64> = HashMap::new();
        for (id, entry) in &self.entries {
            let (role, managed) = (entry.role, entry.active_descendant.is_some());
            if managed || !roving_container(role) {
                continue;
            }
            if let Some(active) = self.active_child(id) {
                active_of.insert(*id, active);
            }
        }
        for (id, entry) in &mut self.entries {
            let group = parent_of
                .get(id)
                .and_then(|p| parents.get(p))
                .map(|&(role, managed)| {
                    if managed {
                        GroupMembership::Managed
                    } else if roving_container(role) {
                        GroupMembership::Roving
                    } else {
                        GroupMembership::None
                    }
                })
                .unwrap_or(GroupMembership::None);
            let group_active = parent_of
                .get(id)
                .and_then(|p| active_of.get(p))
                .is_some_and(|a| *a == *id);
            let desired = desired_tabindex(
                entry.focusable,
                self.focused == Some(*id),
                group,
                group_active,
            );
            if entry.applied_tabindex != desired {
                match desired {
                    Some(v) => {
                        let _ = entry.element.set_attribute("tabindex", &v.to_string());
                    }
                    None => {
                        let _ = entry.element.remove_attribute("tabindex");
                    }
                }
                entry.applied_tabindex = desired;
            }
        }
    }

    /// The active item of a roving container: its `active_descendant`
    /// when it names a child, else the selected-and-focusable child,
    /// else the focused child, else the first focusable child.
    fn active_child(&self, parent_id: &u64) -> Option<u64> {
        let parent = self.entries.get(parent_id)?;
        let kids = &parent.children;
        if let Some(ad) = parent.active_descendant {
            if kids.contains(&ad) {
                return Some(ad);
            }
        }
        for &c in kids {
            if let Some(ce) = self.entries.get(&c) {
                if ce.selected == Some(true) && ce.focusable {
                    return Some(c);
                }
            }
        }
        if let Some(f) = self.focused {
            if kids.contains(&f) && self.entries.get(&f).is_some_and(|e| e.focusable) {
                return Some(f);
            }
        }
        kids.iter()
            .find(|&&c| self.entries.get(&c).is_some_and(|e| e.focusable))
            .copied()
    }

    /// Rewrites every element's `style` from its bounds. `position:
    /// absolute` resolves against the nearest positioned ancestor, so
    /// offsets are parent-relative: `left`/`top` are the node's bounds
    /// origin minus the parent's absolute origin. Style writes are
    /// diffed — an unchanged computed style never touches the DOM.
    fn refresh_positions(&mut self) {
        // Resolve each entry's absolute origin: its bounds origin, or
        // the parent's when it has no bounds (a zero-size element then
        // sits at its parent's origin).
        let mut origins: HashMap<u64, (f64, f64)> = HashMap::new();
        let ids: Vec<u64> = self.entries.keys().copied().collect();
        for id in ids {
            self.resolve_origin(id, &mut origins);
        }
        for entry in self.entries.values_mut() {
            let parent_origin = entry
                .parent
                .and_then(|p| origins.get(&p).copied())
                .unwrap_or((0.0, 0.0));
            let style = mirror_style(entry.bounds, parent_origin, entry.clips_children);
            if entry.applied_style.as_deref() != Some(style.as_str())
                && entry.element.set_attribute("style", &style).is_ok()
            {
                entry.applied_style = Some(style);
            }
        }
    }

    /// Resolves the absolute origin `(x, y)` of entry `id` in window
    /// coordinates: its own bounds origin, or its parent's resolved
    /// origin when it has none.
    fn resolve_origin(&self, id: u64, memo: &mut HashMap<u64, (f64, f64)>) -> (f64, f64) {
        if let Some(&o) = memo.get(&id) {
            return o;
        }
        // Cycle guard: a malformed parent cycle resolves to the root
        // origin rather than recursing forever.
        memo.insert(id, (0.0, 0.0));
        let origin = match self.entries.get(&id) {
            Some(entry) => {
                let parent_origin = entry
                    .parent
                    .map(|p| self.resolve_origin(p, memo))
                    .unwrap_or((0.0, 0.0));
                entry.bounds.map(|b| (b.x0, b.y0)).unwrap_or(parent_origin)
            }
            None => (0.0, 0.0),
        };
        memo.insert(id, origin);
        origin
    }

    /// Fetches (creating if needed) the mirror entry for `id`. An
    /// existing element whose projection or tag changed is rebuilt —
    /// `<div>`/`<input>`/`<textarea>` are not interchangeable.
    fn entry_for(
        &mut self,
        id: u64,
        node: &Node,
        proj: Projection,
    ) -> Result<&mut MirrorEntry, WebA11yError> {
        let tag = tag_name(node, &proj);
        let rebuild = self
            .entries
            .get(&id)
            .is_some_and(|e| e.projection != proj || e.tag != tag);
        if rebuild {
            if let Some(old) = self.entries.remove(&id) {
                old.element.remove();
            }
        }
        if !self.entries.contains_key(&id) {
            let element: HtmlElement = self
                .document
                .create_element(&tag)
                .map_err(|e| WebA11yError::from_js(&e))?
                .unchecked_into();
            let closures =
                attach_listeners(&element, id, proj.is_text_editable(), &self.action_handler)?;
            self.entries.insert(
                id,
                MirrorEntry {
                    element,
                    tag,
                    projection: proj,
                    children: Vec::new(),
                    parent: None,
                    live_text: None,
                    focusable: false,
                    role: node.role(),
                    selected: None,
                    active_descendant: None,
                    size_of_set: None,
                    clips_children: false,
                    bounds: None,
                    applied_attrs: Vec::new(),
                    applied_style: None,
                    applied_tabindex: None,
                    applied_text: None,
                    applied_value: None,
                    applied_selection: None,
                    _closures: closures,
                },
            );
        }
        Ok(self.entries.get_mut(&id).expect("just inserted"))
    }

    /// Writes a node's spec to `entry`'s element, diffed field by field
    /// so unchanged state produces no DOM writes.
    fn apply_spec(entry: &mut MirrorEntry, spec: &NodeSpec) -> Result<(), WebA11yError> {
        let (remove, set) = attr_ops(&entry.applied_attrs, &spec.attrs);
        for name in remove {
            entry
                .element
                .remove_attribute(name)
                .map_err(|e| WebA11yError::from_js(&e))?;
        }
        for (name, value) in set {
            entry
                .element
                .set_attribute(name, value)
                .map_err(|e| WebA11yError::from_js(&e))?;
        }
        entry.applied_attrs = spec.attrs.clone();
        if entry.applied_text != spec.text {
            entry.element.set_text_content(spec.text.as_deref());
            entry.applied_text = spec.text.clone();
        }
        if entry.applied_value != spec.value {
            if let Some(value) = &spec.value {
                set_input_value(&entry.element, value);
            }
            entry.applied_value = spec.value.clone();
        }
        if entry.applied_selection != spec.selection {
            if let Some((start, end)) = spec.selection {
                set_input_selection(&entry.element, start, end);
            }
            entry.applied_selection = spec.selection;
        }
        Ok(())
    }

    /// Recursively removes a subtree's mirror entries, skipping ids in
    /// `claimed` — children re-parented elsewhere in the same update have
    /// already been moved by Phase 2 and their subtrees must survive.
    fn purge_subtree(&mut self, id: u64, claimed: &HashSet<u64>) {
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
}

/// Writes `text` into an `aria-live` element, toggling a trailing
/// zero-width space on repeated identical text so *every* call produces
/// a DOM text mutation (a fixed suffix would defeat the screen reader's
/// text-no-change dedup only once).
fn announce_through(live: &HtmlElement, text: &str) {
    let current = live.text_content().unwrap_or_default();
    let next = if current.trim_end_matches('\u{200B}') == text {
        if current.ends_with('\u{200B}') {
            text.to_string()
        } else {
            format!("{text}\u{200B}")
        }
    } else {
        text.to_string()
    };
    live.set_text_content(Some(&next));
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
        self.activation.remove();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use accesskit::{Node, TreeInfo};

    fn node(role: Role) -> Node {
        Node::new(role)
    }

    #[test]
    fn projection_maps_input_roles() {
        assert_eq!(projection(Role::TextInput), Projection::Input("text"));
        assert_eq!(
            projection(Role::PasswordInput),
            Projection::Input("password")
        );
        assert_eq!(projection(Role::SearchInput), Projection::Input("search"));
        assert_eq!(projection(Role::EmailInput), Projection::Input("email"));
        assert_eq!(projection(Role::UrlInput), Projection::Input("url"));
        assert_eq!(projection(Role::PhoneNumberInput), Projection::Input("tel"));
        assert_eq!(projection(Role::NumberInput), Projection::Input("number"));
        assert_eq!(projection(Role::DateInput), Projection::Input("date"));
        assert_eq!(projection(Role::MultilineTextInput), Projection::TextArea);
        assert_eq!(projection(Role::Button), Projection::Generic);
        assert_eq!(projection(Role::ComboBox), Projection::Generic);
        assert_eq!(
            projection(Role::EditableComboBox),
            Projection::Input("text")
        );
    }

    #[test]
    fn password_spec_never_serializes_value() {
        let mut n = node(Role::PasswordInput);
        n.set_value("hunter2");
        let spec = node_spec(7, &n, &Projection::Input("password"), None);
        assert_eq!(spec.value.as_deref(), Some("hunter2"));
        // The value may only reach input.value — never an attribute or
        // text content.
        assert!(
            !spec
                .attrs
                .iter()
                .any(|(k, v)| k == "aria-valuetext" || v == "hunter2"),
            "password value must not appear in serialized DOM attributes"
        );
        assert!(spec.text.is_none());
        assert!(spec
            .attrs
            .contains(&("type".to_string(), "password".to_string())));
    }

    #[test]
    fn text_input_value_projects_to_value_not_valuetext() {
        let mut n = node(Role::TextInput);
        n.set_value("abc");
        let spec = node_spec(3, &n, &Projection::Input("text"), None);
        assert_eq!(spec.value.as_deref(), Some("abc"));
        assert!(!spec.attrs.iter().any(|(k, _)| k == "aria-valuetext"));
    }

    #[test]
    fn generic_value_projects_to_valuetext() {
        let mut n = node(Role::Slider);
        n.set_value("50%");
        let spec = node_spec(3, &n, &Projection::Generic, None);
        assert!(spec.value.is_none());
        assert!(spec
            .attrs
            .contains(&("aria-valuetext".to_string(), "50%".to_string())));
    }

    #[test]
    fn spec_maps_composite_relations() {
        let mut combo = node(Role::ComboBox);
        combo.set_active_descendant(NodeId(9));
        combo.set_controls([NodeId(9)]);
        combo.set_owns([NodeId(9)]);
        let spec = node_spec(4, &combo, &Projection::Generic, None);
        assert!(spec
            .attrs
            .contains(&("aria-activedescendant".to_string(), dom_id(9))));
        assert!(spec
            .attrs
            .contains(&("aria-controls".to_string(), dom_id(9))));
        assert!(spec.attrs.contains(&("aria-owns".to_string(), dom_id(9))));
    }

    #[test]
    fn spec_zero_based_indices_become_one_based() {
        let mut item = node(Role::ListBoxOption);
        item.set_position_in_set(0);
        item.set_level(0);
        let spec = node_spec(2, &item, &Projection::Generic, Some(5));
        assert!(spec
            .attrs
            .contains(&("aria-posinset".to_string(), "1".to_string())));
        assert!(spec
            .attrs
            .contains(&("aria-level".to_string(), "1".to_string())));
        assert!(spec
            .attrs
            .contains(&("aria-setsize".to_string(), "5".to_string())));
    }

    #[test]
    fn attr_ops_diffs_sorted_lists() {
        let old = vec![
            ("a".to_string(), "1".to_string()),
            ("b".to_string(), "2".to_string()),
            ("c".to_string(), "3".to_string()),
        ];
        let new = vec![
            ("b".to_string(), "2".to_string()),
            ("c".to_string(), "4".to_string()),
            ("d".to_string(), "5".to_string()),
        ];
        let (remove, set) = attr_ops(&old, &new);
        assert_eq!(remove, vec!["a"]);
        assert_eq!(set, vec![("c", "4"), ("d", "5")]);
        // Identical lists produce no work.
        let (remove, set) = attr_ops(&new, &new);
        assert!(remove.is_empty() && set.is_empty());
    }

    #[test]
    fn tabindex_matrix() {
        use GroupMembership::{Managed, None as NoGroup, Roving};
        // Non-focusable nodes never carry tabindex.
        assert_eq!(desired_tabindex(false, false, NoGroup, false), None);
        assert_eq!(desired_tabindex(false, true, NoGroup, false), None);
        // Plain focusable nodes are always tabbable.
        assert_eq!(desired_tabindex(true, false, NoGroup, false), Some(0));
        // Managed group members leave the tab order, focused or not.
        assert_eq!(desired_tabindex(true, true, Managed, false), Some(-1));
        assert_eq!(desired_tabindex(true, false, Managed, true), Some(-1));
        // Roving group: only the active (or focused) item is tabbable.
        assert_eq!(desired_tabindex(true, false, Roving, true), Some(0));
        assert_eq!(desired_tabindex(true, true, Roving, false), Some(0));
        assert_eq!(desired_tabindex(true, false, Roving, false), Some(-1));
    }

    #[test]
    fn roving_container_roles() {
        for role in [
            Role::TabList,
            Role::Menu,
            Role::MenuBar,
            Role::RadioGroup,
            Role::Toolbar,
            Role::ListBox,
            Role::Tree,
            Role::Grid,
        ] {
            assert!(roving_container(role), "{role:?} should be roving");
        }
        for role in [Role::Button, Role::Group, Role::ComboBox, Role::List] {
            assert!(!roving_container(role), "{role:?} should not be roving");
        }
    }

    #[test]
    fn mirror_style_positions_relative_to_parent_origin() {
        let bounds = Rect::new(30.0, 40.0, 130.0, 80.0);
        let style = mirror_style(Some(bounds), (10.0, 10.0), false);
        assert!(style.contains("left:20px"));
        assert!(style.contains("top:30px"));
        assert!(style.contains("width:100px"));
        assert!(style.contains("height:40px"));
        assert!(style.contains("opacity:0"));
        assert!(style.contains("overflow:visible"));
        let clipped = mirror_style(Some(bounds), (10.0, 10.0), true);
        assert!(clipped.contains("overflow:hidden"));
    }

    #[test]
    fn mirror_style_collapses_unbounded_nodes() {
        let style = mirror_style(None, (5.0, 5.0), false);
        assert!(style.contains("width:0"));
        assert!(style.contains("height:0"));
        let zero = mirror_style(Some(Rect::new(0.0, 0.0, 0.0, 0.0)), (0.0, 0.0), false);
        assert!(zero.contains("width:0"));
    }

    #[test]
    fn textarea_projection_adds_multiline() {
        let n = node(Role::MultilineTextInput);
        let spec = node_spec(1, &n, &Projection::TextArea, None);
        assert!(spec
            .attrs
            .contains(&("aria-multiline".to_string(), "true".to_string())));
        assert!(spec
            .attrs
            .contains(&("role".to_string(), "textbox".to_string())));
    }

    #[test]
    fn editable_combobox_keeps_combobox_role_on_input() {
        let n = node(Role::EditableComboBox);
        let spec = node_spec(1, &n, &Projection::Input("text"), None);
        assert!(spec
            .attrs
            .contains(&("role".to_string(), "combobox".to_string())));
    }

    #[test]
    fn pending_merge_preserves_latest_state() {
        // Disabled updates accumulate: last write per node wins, latest
        // tree/focus win.
        let mut u1 = TreeUpdate {
            nodes: vec![(NodeId(1), node(Role::Button))],
            tree: Some(TreeInfo::new(NodeId(1))),
            tree_id: TreeId::ROOT,
            focus: NodeId(1),
        };
        u1.nodes[0].1.set_label("first");
        let mut u2 = TreeUpdate {
            nodes: vec![(NodeId(2), node(Role::CheckBox))],
            tree: None,
            tree_id: TreeId::ROOT,
            focus: NodeId(2),
        };
        u2.nodes[0].1.set_label("second");
        let mut pending = Some(u1);
        let p = pending.as_mut().unwrap();
        p.nodes.extend(u2.nodes.iter().cloned());
        if u2.tree.is_some() {
            p.tree = u2.tree.clone();
        }
        p.focus = u2.focus;
        let p = pending.unwrap();
        assert_eq!(p.nodes.len(), 2);
        assert_eq!(p.focus, NodeId(2));
        assert_eq!(p.tree.unwrap().root, NodeId(1));
    }
}
