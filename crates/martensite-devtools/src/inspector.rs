//! In-app widget inspector and DevTools overlay.
//!
//! Provides the Chrome DevTools-style inspection workflow inside a running
//! Martensite application: click a pixel, inspect the widget, view its
//! layout constraint chain, examine properties/markers, and browse the widget tree.
//!
//! # Activation
//!
//! ```
//! use martensite_devtools::inspector::{DevToolsOptions, InspectorState, KeyCombo};
//!
//! let options = DevToolsOptions {
//!     inspector_key: KeyCombo::F12,
//!     select_mode_key: KeyCombo::CTRL_SHIFT_C,
//!     ..Default::default()
//! };
//!
//! let mut state = InspectorState::with_options(options);
//! assert!(!state.is_active());
//! state.toggle_active();
//! assert!(state.is_active());
//! ```

use std::collections::{HashMap, HashSet};

use glam::Vec2;
use martensite_core::{
    ColdNode, HotNode, LayoutConstraints, NodeFlags, Rect, RenderMinimum, UnderflowPolicy,
    WidgetArena, WidgetId,
};
pub use martensite_design_lint::NodeKind;

bitflags::bitflags! {
    /// Modifier keys active for shortcut matching.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::inspector::Modifiers;
    ///
    /// let mods = Modifiers::CONTROL | Modifiers::SHIFT;
    /// assert!(mods.contains(Modifiers::CONTROL));
    /// assert!(mods.contains(Modifiers::SHIFT));
    /// assert!(!mods.contains(Modifiers::ALT));
    /// ```
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct Modifiers: u8 {
        /// No modifier keys pressed.
        const NONE = 0;
        /// The Shift key.
        const SHIFT = 1 << 0;
        /// The Control key.
        const CONTROL = 1 << 1;
        /// The Alt / Option key.
        const ALT = 1 << 2;
        /// The Meta / Command / Super key.
        const META = 1 << 3;
    }
}

/// Key code representations for DevTools shortcut keys.
///
/// # Examples
///
/// ```
/// use martensite_devtools::inspector::KeyCode;
///
/// assert_eq!(KeyCode::from_name("F12"), Some(KeyCode::F12));
/// assert_eq!(KeyCode::from_name("c"), Some(KeyCode::Char('C')));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyCode {
    /// Function key F1.
    F1,
    /// Function key F2.
    F2,
    /// Function key F3.
    F3,
    /// Function key F4.
    F4,
    /// Function key F5.
    F5,
    /// Function key F6.
    F6,
    /// Function key F7.
    F7,
    /// Function key F8.
    F8,
    /// Function key F9.
    F9,
    /// Function key F10.
    F10,
    /// Function key F11.
    F11,
    /// Function key F12.
    F12,
    /// Character key (stored uppercased for ASCII letters).
    Char(char),
    /// Escape key.
    Escape,
    /// Tab key.
    Tab,
    /// Enter key.
    Enter,
    /// Space key.
    Space,
    /// Unclassified platform key code.
    Other(u32),
}

impl KeyCode {
    /// Parses a key code from its case-insensitive name.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::inspector::KeyCode;
    ///
    /// assert_eq!(KeyCode::from_name("F12"), Some(KeyCode::F12));
    /// assert_eq!(KeyCode::from_name("Escape"), Some(KeyCode::Escape));
    /// assert_eq!(KeyCode::from_name("a"), Some(KeyCode::Char('A')));
    /// ```
    pub fn from_name(name: &str) -> Option<Self> {
        let trimmed = name.trim();
        if trimmed.eq_ignore_ascii_case("F1") {
            return Some(Self::F1);
        }
        if trimmed.eq_ignore_ascii_case("F2") {
            return Some(Self::F2);
        }
        if trimmed.eq_ignore_ascii_case("F3") {
            return Some(Self::F3);
        }
        if trimmed.eq_ignore_ascii_case("F4") {
            return Some(Self::F4);
        }
        if trimmed.eq_ignore_ascii_case("F5") {
            return Some(Self::F5);
        }
        if trimmed.eq_ignore_ascii_case("F6") {
            return Some(Self::F6);
        }
        if trimmed.eq_ignore_ascii_case("F7") {
            return Some(Self::F7);
        }
        if trimmed.eq_ignore_ascii_case("F8") {
            return Some(Self::F8);
        }
        if trimmed.eq_ignore_ascii_case("F9") {
            return Some(Self::F9);
        }
        if trimmed.eq_ignore_ascii_case("F10") {
            return Some(Self::F10);
        }
        if trimmed.eq_ignore_ascii_case("F11") {
            return Some(Self::F11);
        }
        if trimmed.eq_ignore_ascii_case("F12") {
            return Some(Self::F12);
        }
        if trimmed.eq_ignore_ascii_case("Escape") || trimmed.eq_ignore_ascii_case("Esc") {
            return Some(Self::Escape);
        }
        if trimmed.eq_ignore_ascii_case("Tab") {
            return Some(Self::Tab);
        }
        if trimmed.eq_ignore_ascii_case("Enter") || trimmed.eq_ignore_ascii_case("Return") {
            return Some(Self::Enter);
        }
        if trimmed.eq_ignore_ascii_case("Space") || trimmed == " " {
            return Some(Self::Space);
        }
        if trimmed.chars().count() == 1 {
            let c = trimmed.chars().next()?.to_ascii_uppercase();
            return Some(Self::Char(c));
        }
        None
    }
}

/// Keyboard shortcut combination of key code and modifier keys.
///
/// # Examples
///
/// ```
/// use martensite_devtools::inspector::{KeyCombo, KeyCode, Modifiers};
///
/// let f12 = KeyCombo::F12;
/// assert!(f12.matches(KeyCode::F12, Modifiers::NONE));
///
/// let select_mode = KeyCombo::CTRL_SHIFT_C;
/// assert!(select_mode.matches(KeyCode::Char('C'), Modifiers::CONTROL | Modifiers::SHIFT));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyCombo {
    /// Key code for the combo.
    pub key: KeyCode,
    /// Active modifiers required.
    pub modifiers: Modifiers,
}

impl KeyCombo {
    /// F12 default toggle shortcut.
    pub const F12: Self = Self {
        key: KeyCode::F12,
        modifiers: Modifiers::NONE,
    };

    /// Ctrl+Shift+C default select mode shortcut (matching Chrome DevTools).
    pub const CTRL_SHIFT_C: Self = Self {
        key: KeyCode::Char('C'),
        modifiers: Modifiers::CONTROL.union(Modifiers::SHIFT),
    };

    /// Creates a key combo with key code and modifiers.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::inspector::{KeyCombo, KeyCode, Modifiers};
    ///
    /// let combo = KeyCombo::new(KeyCode::F1, Modifiers::CONTROL);
    /// assert_eq!(combo.key, KeyCode::F1);
    /// ```
    pub const fn new(key: KeyCode, modifiers: Modifiers) -> Self {
        Self { key, modifiers }
    }

    /// Creates a key combo with no modifiers.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::inspector::{KeyCombo, KeyCode};
    ///
    /// let combo = KeyCombo::key(KeyCode::F12);
    /// assert_eq!(combo.key, KeyCode::F12);
    /// ```
    pub const fn key(key: KeyCode) -> Self {
        Self {
            key,
            modifiers: Modifiers::NONE,
        }
    }

    /// Checks if a key code and modifier set matches this combination.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::inspector::{KeyCombo, KeyCode, Modifiers};
    ///
    /// let combo = KeyCombo::F12;
    /// assert!(combo.matches(KeyCode::F12, Modifiers::NONE));
    /// assert!(!combo.matches(KeyCode::F11, Modifiers::NONE));
    /// ```
    pub fn matches(&self, key: KeyCode, modifiers: Modifiers) -> bool {
        self.key == key && self.modifiers == modifiers
    }

    /// Checks if a key name and modifier set matches this combination.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::inspector::{KeyCombo, Modifiers};
    ///
    /// let combo = KeyCombo::F12;
    /// assert!(combo.matches_event("F12", Modifiers::NONE));
    /// assert!(!combo.matches_event("F11", Modifiers::NONE));
    /// ```
    pub fn matches_event(&self, key_name: &str, modifiers: Modifiers) -> bool {
        if let Some(code) = KeyCode::from_name(key_name) {
            self.matches(code, modifiers)
        } else {
            false
        }
    }
}

/// Configuration options for the in-app DevTools inspector.
///
/// # Examples
///
/// ```
/// use martensite_devtools::inspector::{DevToolsOptions, KeyCombo};
///
/// let options = DevToolsOptions::default()
///     .with_inspector_key(KeyCombo::F12)
///     .with_select_mode_key(KeyCombo::CTRL_SHIFT_C);
/// assert!(options.enabled);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DevToolsOptions {
    /// Shortcut key to toggle the inspector overlay (defaults to F12).
    pub inspector_key: KeyCombo,
    /// Shortcut key to arm select mode (defaults to Ctrl+Shift+C).
    pub select_mode_key: KeyCombo,
    /// Whether DevTools is enabled initially.
    pub enabled: bool,
    /// Maximum children to materialize before creating a virtual placeholder in tree view.
    pub virtual_child_threshold: usize,
    /// Maximum execution time budget per collection frame (nanoseconds, default 100_000 ns = 0.1 ms).
    pub frame_budget_ns: u64,
}

impl Default for DevToolsOptions {
    fn default() -> Self {
        Self {
            inspector_key: KeyCombo::F12,
            select_mode_key: KeyCombo::CTRL_SHIFT_C,
            enabled: true,
            virtual_child_threshold: 50,
            frame_budget_ns: 100_000,
        }
    }
}

impl DevToolsOptions {
    /// Creates a default `DevToolsOptions`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::inspector::DevToolsOptions;
    ///
    /// let options = DevToolsOptions::new();
    /// assert!(options.enabled);
    /// ```
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the inspector toggle shortcut key.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::inspector::{DevToolsOptions, KeyCombo};
    ///
    /// let options = DevToolsOptions::new().with_inspector_key(KeyCombo::F12);
    /// assert_eq!(options.inspector_key, KeyCombo::F12);
    /// ```
    pub fn with_inspector_key(mut self, key: KeyCombo) -> Self {
        self.inspector_key = key;
        self
    }

    /// Sets the select mode shortcut key.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::inspector::{DevToolsOptions, KeyCombo};
    ///
    /// let options = DevToolsOptions::new().with_select_mode_key(KeyCombo::CTRL_SHIFT_C);
    /// assert_eq!(options.select_mode_key, KeyCombo::CTRL_SHIFT_C);
    /// ```
    pub fn with_select_mode_key(mut self, key: KeyCombo) -> Self {
        self.select_mode_key = key;
        self
    }

    /// Sets whether DevTools is enabled.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::inspector::DevToolsOptions;
    ///
    /// let options = DevToolsOptions::new().with_enabled(false);
    /// assert!(!options.enabled);
    /// ```
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Sets the child count threshold for virtualizing tree nodes.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::inspector::DevToolsOptions;
    ///
    /// let options = DevToolsOptions::new().with_virtual_child_threshold(100);
    /// assert_eq!(options.virtual_child_threshold, 100);
    /// ```
    pub fn with_virtual_child_threshold(mut self, threshold: usize) -> Self {
        self.virtual_child_threshold = threshold;
        self
    }

    /// Sets the maximum per-frame collection budget in nanoseconds.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::inspector::DevToolsOptions;
    ///
    /// let options = DevToolsOptions::new().with_frame_budget_ns(50_000);
    /// assert_eq!(options.frame_budget_ns, 50_000);
    /// ```
    pub fn with_frame_budget_ns(mut self, budget: u64) -> Self {
        self.frame_budget_ns = budget;
        self
    }
}

/// Active inspection panel in the DevTools overlay.
///
/// # Examples
///
/// ```
/// use martensite_devtools::inspector::InspectionMode;
///
/// let mode = InspectionMode::Tree;
/// assert_eq!(mode.title(), "Elements");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum InspectionMode {
    /// Widget hierarchy tree panel (indented tree with lazy expansion).
    #[default]
    Tree,
    /// Layout constraint and sizing panel.
    Layout,
    /// Properties, semantic markers, and reactive bindings panel.
    Properties,
    /// Design-lint findings and rule diagnostics panel.
    Lint,
    /// Accessibility tree and live region panel.
    Accessibility,
    /// Event dispatch ring buffer panel.
    Events,
}

impl InspectionMode {
    /// Returns the human-readable display title for the panel tab.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::inspector::InspectionMode;
    ///
    /// assert_eq!(InspectionMode::Tree.title(), "Elements");
    /// assert_eq!(InspectionMode::Layout.title(), "Layout");
    /// ```
    pub const fn title(&self) -> &'static str {
        match self {
            Self::Tree => "Elements",
            Self::Layout => "Layout",
            Self::Properties => "Properties",
            Self::Lint => "Design Lint",
            Self::Accessibility => "Accessibility",
            Self::Events => "Events",
        }
    }
}

/// In-app DevTools inspector state machine.
///
/// Controls overlay visibility, select-mode arming, widget selection,
/// and inspection modes.
///
/// # Examples
///
/// ```
/// use martensite_devtools::inspector::InspectorState;
///
/// let mut state = InspectorState::new();
/// assert!(!state.is_active());
///
/// state.toggle_active();
/// assert!(state.is_active());
///
/// state.arm_select_mode();
/// assert!(state.is_select_mode_armed());
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct InspectorState {
    options: DevToolsOptions,
    is_active: bool,
    select_mode: bool,
    selected_widget: Option<WidgetId>,
    hovered_widget: Option<WidgetId>,
    mode: InspectionMode,
    ancestry_chain: Vec<WidgetId>,
    last_collection_ns: u64,
}

impl Default for InspectorState {
    fn default() -> Self {
        Self::new()
    }
}

impl InspectorState {
    /// Creates a new `InspectorState` with default options.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::inspector::InspectorState;
    ///
    /// let state = InspectorState::new();
    /// assert!(!state.is_active());
    /// ```
    pub fn new() -> Self {
        Self::with_options(DevToolsOptions::default())
    }

    /// Creates a new `InspectorState` with custom options.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::inspector::{DevToolsOptions, InspectorState};
    ///
    /// let state = InspectorState::with_options(DevToolsOptions::default());
    /// assert_eq!(state.mode(), martensite_devtools::inspector::InspectionMode::Tree);
    /// ```
    pub fn with_options(options: DevToolsOptions) -> Self {
        Self {
            options,
            is_active: false,
            select_mode: false,
            selected_widget: None,
            hovered_widget: None,
            mode: InspectionMode::Tree,
            ancestry_chain: Vec::new(),
            last_collection_ns: 0,
        }
    }

    /// Returns `true` if the inspector overlay is active/open.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::inspector::InspectorState;
    ///
    /// let state = InspectorState::new();
    /// assert!(!state.is_active());
    /// ```
    #[inline]
    pub fn is_active(&self) -> bool {
        self.is_active
    }

    /// Sets whether the inspector overlay is active.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::inspector::InspectorState;
    ///
    /// let mut state = InspectorState::new();
    /// state.set_active(true);
    /// assert!(state.is_active());
    /// ```
    pub fn set_active(&mut self, active: bool) {
        self.is_active = active;
        if !active {
            self.select_mode = false;
        }
    }

    /// Toggles the active state of the inspector overlay.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::inspector::InspectorState;
    ///
    /// let mut state = InspectorState::new();
    /// assert!(state.toggle_active());
    /// assert!(state.is_active());
    /// assert!(!state.toggle_active());
    /// assert!(!state.is_active());
    /// ```
    pub fn toggle_active(&mut self) -> bool {
        self.set_active(!self.is_active);
        self.is_active
    }

    /// Returns `true` if select mode is armed.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::inspector::InspectorState;
    ///
    /// let mut state = InspectorState::new();
    /// assert!(!state.is_select_mode_armed());
    /// state.arm_select_mode();
    /// assert!(state.is_select_mode_armed());
    /// ```
    #[inline]
    pub fn is_select_mode_armed(&self) -> bool {
        self.select_mode
    }

    /// Arms select mode so the next click selects the widget under the pointer.
    ///
    /// Also opens the inspector overlay if closed.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::inspector::InspectorState;
    ///
    /// let mut state = InspectorState::new();
    /// state.arm_select_mode();
    /// assert!(state.is_select_mode_armed());
    /// assert!(state.is_active());
    /// ```
    pub fn arm_select_mode(&mut self) {
        self.select_mode = true;
        self.is_active = true;
    }

    /// Disarms select mode.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::inspector::InspectorState;
    ///
    /// let mut state = InspectorState::new();
    /// state.arm_select_mode();
    /// state.disarm_select_mode();
    /// assert!(!state.is_select_mode_armed());
    /// ```
    pub fn disarm_select_mode(&mut self) {
        self.select_mode = false;
    }

    /// Toggles select mode state.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::inspector::InspectorState;
    ///
    /// let mut state = InspectorState::new();
    /// assert!(state.toggle_select_mode());
    /// assert!(state.is_select_mode_armed());
    /// ```
    pub fn toggle_select_mode(&mut self) -> bool {
        if self.select_mode {
            self.disarm_select_mode();
            false
        } else {
            self.arm_select_mode();
            true
        }
    }

    /// Returns the currently selected widget handle, if any.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::WidgetId;
    /// use martensite_devtools::inspector::InspectorState;
    ///
    /// let mut state = InspectorState::new();
    /// assert_eq!(state.selected(), None);
    /// state.select(WidgetId::from_parts(10, 1), None);
    /// assert_eq!(state.selected(), Some(WidgetId::from_parts(10, 1)));
    /// ```
    #[inline]
    pub fn selected(&self) -> Option<WidgetId> {
        self.selected_widget
    }

    /// Selects a widget and updates its ancestry chain if an arena is provided.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::{ColdNode, DummyWidget, HotNode, WidgetArena};
    /// use martensite_devtools::inspector::InspectorState;
    ///
    /// let mut arena = WidgetArena::new();
    /// let root = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
    /// let mut state = InspectorState::new();
    /// state.select(root, Some(&arena));
    /// assert_eq!(state.selected(), Some(root));
    /// assert_eq!(state.ancestry(), &[root]);
    /// ```
    pub fn select(&mut self, id: WidgetId, arena: Option<&WidgetArena>) {
        self.selected_widget = Some(id);
        if let Some(arena) = arena {
            let mut chain = Vec::new();
            let mut curr = Some(id);
            while let Some(node) = curr {
                chain.push(node);
                curr = arena.parent(node);
            }
            chain.reverse();
            self.ancestry_chain = chain;
        } else {
            self.ancestry_chain = vec![id];
        }
    }

    /// Clears the current widget selection.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::WidgetId;
    /// use martensite_devtools::inspector::InspectorState;
    ///
    /// let mut state = InspectorState::new();
    /// state.select(WidgetId::from_parts(1, 1), None);
    /// state.clear_selection();
    /// assert_eq!(state.selected(), None);
    /// ```
    pub fn clear_selection(&mut self) {
        self.selected_widget = None;
        self.ancestry_chain.clear();
    }

    /// Returns the currently hovered widget handle, if any.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::WidgetId;
    /// use martensite_devtools::inspector::InspectorState;
    ///
    /// let mut state = InspectorState::new();
    /// assert_eq!(state.hovered(), None);
    /// state.set_hovered(Some(WidgetId::from_parts(2, 1)));
    /// assert_eq!(state.hovered(), Some(WidgetId::from_parts(2, 1)));
    /// ```
    #[inline]
    pub fn hovered(&self) -> Option<WidgetId> {
        self.hovered_widget
    }

    /// Sets the currently hovered widget handle.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::WidgetId;
    /// use martensite_devtools::inspector::InspectorState;
    ///
    /// let mut state = InspectorState::new();
    /// state.set_hovered(Some(WidgetId::from_parts(3, 1)));
    /// assert_eq!(state.hovered(), Some(WidgetId::from_parts(3, 1)));
    /// ```
    pub fn set_hovered(&mut self, hovered: Option<WidgetId>) {
        self.hovered_widget = hovered;
    }

    /// Returns the active inspection mode / panel.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::inspector::{InspectionMode, InspectorState};
    ///
    /// let state = InspectorState::new();
    /// assert_eq!(state.mode(), InspectionMode::Tree);
    /// ```
    #[inline]
    pub fn mode(&self) -> InspectionMode {
        self.mode
    }

    /// Sets the active inspection mode / panel.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::inspector::{InspectionMode, InspectorState};
    ///
    /// let mut state = InspectorState::new();
    /// state.set_mode(InspectionMode::Layout);
    /// assert_eq!(state.mode(), InspectionMode::Layout);
    /// ```
    pub fn set_mode(&mut self, mode: InspectionMode) {
        self.mode = mode;
    }

    /// Returns the ancestry chain for the currently selected widget (`[root, ..., selected]`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::WidgetId;
    /// use martensite_devtools::inspector::InspectorState;
    ///
    /// let mut state = InspectorState::new();
    /// state.select(WidgetId::from_parts(5, 1), None);
    /// assert_eq!(state.ancestry(), &[WidgetId::from_parts(5, 1)]);
    /// ```
    #[inline]
    pub fn ancestry(&self) -> &[WidgetId] {
        &self.ancestry_chain
    }

    /// Returns a reference to the active DevTools options.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::inspector::InspectorState;
    ///
    /// let state = InspectorState::new();
    /// assert!(state.options().enabled);
    /// ```
    #[inline]
    pub fn options(&self) -> &DevToolsOptions {
        &self.options
    }

    /// Returns a mutable reference to the active DevTools options.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::inspector::InspectorState;
    ///
    /// let mut state = InspectorState::new();
    /// state.options_mut().enabled = false;
    /// assert!(!state.options().enabled);
    /// ```
    #[inline]
    pub fn options_mut(&mut self) -> &mut DevToolsOptions {
        &mut self.options
    }

    /// Processes a key event against the configured shortcut keys.
    ///
    /// Returns `true` if the event was handled by DevTools (e.g. toggled active
    /// or armed select mode).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::inspector::{InspectorState, Modifiers};
    ///
    /// let mut state = InspectorState::new();
    /// assert!(state.handle_key("F12", Modifiers::NONE));
    /// assert!(state.is_active());
    /// ```
    pub fn handle_key(&mut self, key_name: &str, modifiers: Modifiers) -> bool {
        if !self.options.enabled {
            return false;
        }

        if self
            .options
            .inspector_key
            .matches_event(key_name, modifiers)
        {
            self.toggle_active();
            return true;
        }

        if self
            .options
            .select_mode_key
            .matches_event(key_name, modifiers)
        {
            self.toggle_select_mode();
            return true;
        }

        false
    }

    /// Handles a pointer click event when select mode is armed.
    ///
    /// If select mode is active, tests the click location, selects the hit
    /// widget, disarms select mode, and returns `Some(WidgetId)`. If select mode
    /// is not active or no widget was hit, returns `None`.
    ///
    /// # Examples
    ///
    /// ```
    /// use glam::Vec2;
    /// use martensite_core::{ColdNode, DummyWidget, HotNode, NodeFlags, Rect, WidgetArena};
    /// use martensite_devtools::inspector::InspectorState;
    ///
    /// let mut arena = WidgetArena::new();
    /// let root = arena.insert(
    ///     HotNode {
    ///         bounds: Rect::new(0.0, 0.0, 100.0, 100.0),
    ///         flags: NodeFlags::VISIBLE | NodeFlags::HIT_TEST_ENABLED,
    ///         ..HotNode::default()
    ///     },
    ///     ColdNode::default(),
    /// );
    ///
    /// let mut state = InspectorState::new();
    /// state.arm_select_mode();
    /// let hit = state.handle_pointer_click(&arena, root, Vec2::new(50.0, 50.0));
    /// assert_eq!(hit, Some(root));
    /// assert!(!state.is_select_mode_armed());
    /// ```
    pub fn handle_pointer_click(
        &mut self,
        arena: &WidgetArena,
        root: WidgetId,
        point: Vec2,
    ) -> Option<WidgetId> {
        if !self.select_mode {
            return None;
        }

        if let Some(hit) = hit_test_select(arena, root, point) {
            self.select(hit.widget_id, Some(arena));
            self.disarm_select_mode();
            Some(hit.widget_id)
        } else {
            None
        }
    }

    /// Records the time spent on collection in nanoseconds for overhead budgeting.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::inspector::InspectorState;
    ///
    /// let mut state = InspectorState::new();
    /// state.record_collection_duration(50_000);
    /// assert!(state.overhead_under_budget());
    /// ```
    pub fn record_collection_duration(&mut self, nanos: u64) {
        self.last_collection_ns = nanos;
    }

    /// Returns `true` if the last data collection duration was within the configured budget (< 0.1 ms).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::inspector::InspectorState;
    ///
    /// let mut state = InspectorState::new();
    /// state.record_collection_duration(50_000);
    /// assert!(state.overhead_under_budget());
    /// ```
    pub fn overhead_under_budget(&self) -> bool {
        self.last_collection_ns <= self.options.frame_budget_ns
    }
}

/// The outcome of an inspector select-mode hit-test.
///
/// Contains the targeted widget handle, coordinates, and the complete
/// ancestry chain from root down to the target widget.
///
/// # Examples
///
/// ```
/// use glam::Vec2;
/// use martensite_core::{ColdNode, HotNode, NodeFlags, Rect, WidgetArena};
/// use martensite_devtools::inspector::hit_test_select;
///
/// let mut arena = WidgetArena::new();
/// let root = arena.insert(
///     HotNode {
///         bounds: Rect::new(0.0, 0.0, 100.0, 100.0),
///         flags: NodeFlags::VISIBLE | NodeFlags::HIT_TEST_ENABLED,
///         ..HotNode::default()
///     },
///     ColdNode::default(),
/// );
///
/// let hit = hit_test_select(&arena, root, Vec2::new(50.0, 50.0)).expect("hit root");
/// assert_eq!(hit.widget_id, root);
/// assert_eq!(hit.ancestry, vec![root]);
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct HitTestResult {
    /// The hit widget handle.
    pub widget_id: WidgetId,
    /// Pointer location in the hit widget's local coordinates.
    pub local_point: Vec2,
    /// Pointer location in screen coordinates.
    pub screen_point: Vec2,
    /// Complete ancestry chain from root to the hit widget (`[root, ..., target]`).
    pub ancestry: Vec<WidgetId>,
}

/// Hit-tests the arena for select-mode, resolving the exact same [`WidgetId`]
/// as the production router and capturing the complete ancestry chain.
///
/// # Examples
///
/// ```
/// use glam::Vec2;
/// use martensite_core::{ColdNode, DummyWidget, HotNode, NodeFlags, Rect, WidgetArena};
/// use martensite_devtools::inspector::hit_test_select;
///
/// let mut arena = WidgetArena::new();
/// let root = arena.insert(
///     HotNode {
///         bounds: Rect::new(0.0, 0.0, 200.0, 200.0),
///         flags: NodeFlags::VISIBLE | NodeFlags::HIT_TEST_ENABLED,
///         ..HotNode::default()
///     },
///     ColdNode::default(),
/// );
/// let child = arena.insert(
///     HotNode {
///         bounds: Rect::new(10.0, 10.0, 50.0, 50.0),
///         flags: NodeFlags::VISIBLE | NodeFlags::HIT_TEST_ENABLED,
///         ..HotNode::default()
///     },
///     ColdNode::default(),
/// );
/// arena.append_child(root, child).unwrap();
///
/// let hit = hit_test_select(&arena, root, Vec2::new(20.0, 20.0)).unwrap();
/// assert_eq!(hit.widget_id, child);
/// assert_eq!(hit.ancestry, vec![root, child]);
/// ```
pub fn hit_test_select(
    arena: &WidgetArena,
    root: WidgetId,
    screen_point: Vec2,
) -> Option<HitTestResult> {
    if !screen_point.is_finite() || !arena.is_alive(root) {
        return None;
    }

    fn hit_test_node(
        arena: &WidgetArena,
        node: WidgetId,
        screen_point: Vec2,
    ) -> Option<(WidgetId, Vec2)> {
        let hot: &HotNode = arena.get_hot(node)?;

        // Invisible widgets hide their entire subtree.
        if !hot.flags.contains(NodeFlags::VISIBLE) {
            return None;
        }

        // Underflow-covered subtrees (Hide/Collapse/Scrim) are unhittable.
        if arena
            .get_cold(node)
            .and_then(|c| c.underflow_policy())
            .is_some_and(|p| p.covers_input())
        {
            return None;
        }

        // Recurse into children in reverse Z-order (topmost first).
        let mut child = arena.last_child(node);
        while let Some(child_id) = child {
            if let Some(hit) = hit_test_node(arena, child_id, screen_point) {
                return Some(hit);
            }
            child = arena.prev_sibling(child_id);
        }

        // Test the node itself if hit-test is enabled and not inert.
        let flags = hot.flags;
        if !flags.contains(NodeFlags::HIT_TEST_ENABLED) || flags.contains(NodeFlags::INERT) {
            return None;
        }

        // Point inside bounds test
        if !hot.bounds.contains(screen_point) {
            return None;
        }

        let local_point = screen_point - hot.bounds.origin;
        Some((node, local_point))
    }

    let (target, local_point) = hit_test_node(arena, root, screen_point)?;

    // Build the complete ancestry chain from root to target.
    let mut ancestry = Vec::new();
    let mut curr = Some(target);
    while let Some(id) = curr {
        ancestry.push(id);
        curr = arena.parent(id);
    }
    ancestry.reverse();

    Some(HitTestResult {
        widget_id: target,
        local_point,
        screen_point,
        ancestry,
    })
}

/// Diagnostic badges displayed alongside tree items.
///
/// # Examples
///
/// ```
/// use martensite_devtools::inspector::NodeBadges;
///
/// let mut badges = NodeBadges::default();
/// badges.has_active_lints = true;
/// assert_eq!(badges.format_badges(), "⚠");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NodeBadges {
    /// `⚠` Active design lint findings or layout violations.
    pub has_active_lints: bool,
    /// `↻` Signals fired or updated during the current frame.
    pub signal_fired: bool,
    /// `⛔` Suppressed lint findings (`@lint:allow` / `@lint:...`).
    pub has_suppressed_lints: bool,
}

impl NodeBadges {
    /// Formats the active badges into an indicator string (e.g. `"⚠ ↻ ⛔"`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::inspector::NodeBadges;
    ///
    /// let badges = NodeBadges {
    ///     has_active_lints: true,
    ///     signal_fired: true,
    ///     has_suppressed_lints: false,
    /// };
    /// assert_eq!(badges.format_badges(), "⚠ ↻");
    /// ```
    pub fn format_badges(&self) -> String {
        let mut parts = Vec::new();
        if self.has_active_lints {
            parts.push("⚠");
        }
        if self.signal_fired {
            parts.push("↻");
        }
        if self.has_suppressed_lints {
            parts.push("⛔");
        }
        parts.join(" ")
    }

    /// Returns `true` if no badges are active.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::inspector::NodeBadges;
    ///
    /// let badges = NodeBadges::default();
    /// assert!(badges.is_empty());
    /// ```
    pub const fn is_empty(&self) -> bool {
        !self.has_active_lints && !self.signal_fired && !self.has_suppressed_lints
    }
}

/// A node in the lazy inspector tree model.
///
/// # Examples
///
/// ```
/// use glam::Vec2;
/// use martensite_core::{NodeFlags, Rect, WidgetId};
/// use martensite_devtools::inspector::{InspectorTreeNode, NodeBadges, NodeKind};
///
/// let node = InspectorTreeNode {
///     id: WidgetId::from_parts(1, 1),
///     debug_name: Some("Button".to_string()),
///     kind: NodeKind::Interactive,
///     screen_bounds: Rect::new(0.0, 0.0, 100.0, 30.0),
///     local_bounds: Rect::new(0.0, 0.0, 100.0, 30.0),
///     depth: 0,
///     child_count: 0,
///     badges: NodeBadges::default(),
///     active_signal_count: 0,
///     is_expanded: false,
///     is_virtual_placeholder: false,
///     virtual_placeholder_label: None,
///     children: None,
/// };
/// assert_eq!(node.display_label(), "Button");
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct InspectorTreeNode {
    /// Widget arena identifier.
    pub id: WidgetId,
    /// Debug name from [`ColdNode::debug_name`].
    pub debug_name: Option<String>,
    /// Classified node kind.
    pub kind: NodeKind,
    /// Screen-space bounding rectangle.
    pub screen_bounds: Rect,
    /// Local-space bounding rectangle relative to parent.
    pub local_bounds: Rect,
    /// Depth rank in hierarchy (0 = root).
    pub depth: usize,
    /// Total number of children in arena.
    pub child_count: usize,
    /// Active badges (⚠, ↻, ⛔).
    pub badges: NodeBadges,
    /// Number of active reactive signals bound to this widget.
    pub active_signal_count: usize,
    /// Whether this tree node is currently expanded.
    pub is_expanded: bool,
    /// Whether this node is a virtualized placeholder (`+N rows`).
    pub is_virtual_placeholder: bool,
    /// Text label for virtual placeholder (e.g. `"+1000000 rows"`).
    pub virtual_placeholder_label: Option<String>,
    /// Children materialized on demand when expanded.
    pub children: Option<Vec<InspectorTreeNode>>,
}

impl InspectorTreeNode {
    /// Returns the user-facing display label for the tree node.
    ///
    /// # Examples
    ///
    /// ```
    /// use glam::Vec2;
    /// use martensite_core::{NodeFlags, Rect, WidgetId};
    /// use martensite_devtools::inspector::{InspectorTreeNode, NodeBadges, NodeKind};
    ///
    /// let node = InspectorTreeNode {
    ///     id: WidgetId::from_parts(1, 1),
    ///     debug_name: Some("Label".to_string()),
    ///     kind: NodeKind::Content,
    ///     screen_bounds: Rect::default(),
    ///     local_bounds: Rect::default(),
    ///     depth: 0,
    ///     child_count: 0,
    ///     badges: NodeBadges::default(),
    ///     active_signal_count: 0,
    ///     is_expanded: false,
    ///     is_virtual_placeholder: false,
    ///     virtual_placeholder_label: None,
    ///     children: None,
    /// };
    /// assert_eq!(node.display_label(), "Label");
    /// ```
    pub fn display_label(&self) -> &str {
        if let Some(label) = &self.virtual_placeholder_label {
            label.as_str()
        } else if let Some(name) = &self.debug_name {
            name.as_str()
        } else {
            "Widget"
        }
    }
}

/// Lazy tree model for inspecting the [`WidgetArena`].
///
/// Materializes tree nodes on demand, keeping memory and traversal
/// $O(\text{visible rows})$ rather than $O(\text{arena})$, even when
/// inspecting virtualized lists with millions of rows.
///
/// # Examples
///
/// ```
/// use martensite_core::{ColdNode, DummyWidget, HotNode, WidgetArena};
/// use martensite_devtools::inspector::InspectorTreeModel;
///
/// let mut arena = WidgetArena::new();
/// let root = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
///
/// let mut model = InspectorTreeModel::new(root);
/// let root_node = model.resolve_node(&arena, root, 0).expect("resolved root");
/// assert_eq!(root_node.id, root);
/// ```
#[derive(Debug, Clone)]
pub struct InspectorTreeModel {
    root_id: WidgetId,
    expanded: HashSet<WidgetId>,
    virtual_threshold: usize,
    active_lints: HashMap<WidgetId, usize>,
    suppressed_lints: HashMap<WidgetId, usize>,
    active_signals: HashMap<WidgetId, usize>,
    fired_signals: HashSet<WidgetId>,
}

impl InspectorTreeModel {
    /// Creates a new lazy tree model rooted at `root_id`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::WidgetId;
    /// use martensite_devtools::inspector::InspectorTreeModel;
    ///
    /// let model = InspectorTreeModel::new(WidgetId::from_parts(1, 1));
    /// assert!(!model.is_expanded(WidgetId::from_parts(1, 1)));
    /// ```
    pub fn new(root_id: WidgetId) -> Self {
        Self {
            root_id,
            expanded: HashSet::new(),
            virtual_threshold: 50,
            active_lints: HashMap::new(),
            suppressed_lints: HashMap::new(),
            active_signals: HashMap::new(),
            fired_signals: HashSet::new(),
        }
    }

    /// Sets the child count threshold above which children are collapsed into a virtual placeholder.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::WidgetId;
    /// use martensite_devtools::inspector::InspectorTreeModel;
    ///
    /// let model = InspectorTreeModel::new(WidgetId::from_parts(1, 1)).with_virtual_threshold(20);
    /// assert_eq!(model.virtual_threshold(), 20);
    /// ```
    pub fn with_virtual_threshold(mut self, threshold: usize) -> Self {
        self.virtual_threshold = threshold;
        self
    }

    /// Returns the virtualized child count threshold.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::WidgetId;
    /// use martensite_devtools::inspector::InspectorTreeModel;
    ///
    /// let model = InspectorTreeModel::new(WidgetId::from_parts(1, 1));
    /// assert_eq!(model.virtual_threshold(), 50);
    /// ```
    #[inline]
    pub fn virtual_threshold(&self) -> usize {
        self.virtual_threshold
    }

    /// Returns `true` if the given widget is expanded in the tree view.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::WidgetId;
    /// use martensite_devtools::inspector::InspectorTreeModel;
    ///
    /// let model = InspectorTreeModel::new(WidgetId::from_parts(1, 1));
    /// assert!(!model.is_expanded(WidgetId::from_parts(1, 1)));
    /// ```
    #[inline]
    pub fn is_expanded(&self, id: WidgetId) -> bool {
        self.expanded.contains(&id)
    }

    /// Expands the node with identifier `id`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::WidgetId;
    /// use martensite_devtools::inspector::InspectorTreeModel;
    ///
    /// let mut model = InspectorTreeModel::new(WidgetId::from_parts(1, 1));
    /// model.expand(WidgetId::from_parts(1, 1));
    /// assert!(model.is_expanded(WidgetId::from_parts(1, 1)));
    /// ```
    pub fn expand(&mut self, id: WidgetId) {
        self.expanded.insert(id);
    }

    /// Collapses the node with identifier `id`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::WidgetId;
    /// use martensite_devtools::inspector::InspectorTreeModel;
    ///
    /// let mut model = InspectorTreeModel::new(WidgetId::from_parts(1, 1));
    /// model.expand(WidgetId::from_parts(1, 1));
    /// model.collapse(WidgetId::from_parts(1, 1));
    /// assert!(!model.is_expanded(WidgetId::from_parts(1, 1)));
    /// ```
    pub fn collapse(&mut self, id: WidgetId) {
        self.expanded.remove(&id);
    }

    /// Toggles the expansion state of `id`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::WidgetId;
    /// use martensite_devtools::inspector::InspectorTreeModel;
    ///
    /// let mut model = InspectorTreeModel::new(WidgetId::from_parts(1, 1));
    /// assert!(model.toggle_expanded(WidgetId::from_parts(1, 1)));
    /// assert!(model.is_expanded(WidgetId::from_parts(1, 1)));
    /// ```
    pub fn toggle_expanded(&mut self, id: WidgetId) -> bool {
        if self.is_expanded(id) {
            self.collapse(id);
            false
        } else {
            self.expand(id);
            true
        }
    }

    /// Expands all ancestors of `target` in `arena` so that `target` becomes visible in the tree.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::{ColdNode, DummyWidget, HotNode, WidgetArena};
    /// use martensite_devtools::inspector::InspectorTreeModel;
    ///
    /// let mut arena = WidgetArena::new();
    /// let root = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
    /// let child = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
    /// arena.append_child(root, child).unwrap();
    ///
    /// let mut model = InspectorTreeModel::new(root);
    /// model.expand_to_widget(&arena, child);
    /// assert!(model.is_expanded(root));
    /// ```
    pub fn expand_to_widget(&mut self, arena: &WidgetArena, target: WidgetId) {
        let mut curr = arena.parent(target);
        while let Some(parent) = curr {
            self.expand(parent);
            curr = arena.parent(parent);
        }
    }

    /// Sets the design lint finding counts for a widget.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::WidgetId;
    /// use martensite_devtools::inspector::InspectorTreeModel;
    ///
    /// let mut model = InspectorTreeModel::new(WidgetId::from_parts(1, 1));
    /// model.set_lint_stats(WidgetId::from_parts(1, 1), 2, 1);
    /// ```
    pub fn set_lint_stats(&mut self, id: WidgetId, active_count: usize, suppressed_count: usize) {
        self.active_lints.insert(id, active_count);
        self.suppressed_lints.insert(id, suppressed_count);
    }

    /// Sets reactive signal activity for a widget.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::WidgetId;
    /// use martensite_devtools::inspector::InspectorTreeModel;
    ///
    /// let mut model = InspectorTreeModel::new(WidgetId::from_parts(1, 1));
    /// model.set_signal_stats(WidgetId::from_parts(1, 1), 3, true);
    /// ```
    pub fn set_signal_stats(&mut self, id: WidgetId, count: usize, fired: bool) {
        self.active_signals.insert(id, count);
        if fired {
            self.fired_signals.insert(id);
        } else {
            self.fired_signals.remove(&id);
        }
    }

    /// Clears frame signal fired markers.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::WidgetId;
    /// use martensite_devtools::inspector::InspectorTreeModel;
    ///
    /// let mut model = InspectorTreeModel::new(WidgetId::from_parts(1, 1));
    /// model.set_signal_stats(WidgetId::from_parts(1, 1), 1, true);
    /// model.clear_signal_fired();
    /// ```
    pub fn clear_signal_fired(&mut self) {
        self.fired_signals.clear();
    }

    /// Lazily resolves a tree node and its children (if expanded) on demand.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::{ColdNode, DummyWidget, HotNode, WidgetArena};
    /// use martensite_devtools::inspector::InspectorTreeModel;
    ///
    /// let mut arena = WidgetArena::new();
    /// let root = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
    /// let model = InspectorTreeModel::new(root);
    /// let node = model.resolve_node(&arena, root, 0).expect("resolved root");
    /// assert_eq!(node.id, root);
    /// ```
    pub fn resolve_node(
        &self,
        arena: &WidgetArena,
        id: WidgetId,
        depth: usize,
    ) -> Option<InspectorTreeNode> {
        let hot: &HotNode = arena.get_hot(id)?;
        let cold: &ColdNode = arena.get_cold(id)?;

        let raw_debug = cold.debug_name;
        let (display_name, markers) = if let Some(raw) = raw_debug {
            let (name, m) = InlineMarkers::parse(raw);
            (Some(name), m)
        } else {
            (None, InlineMarkers::default())
        };

        let kind = NodeKind::from_widget_name(display_name.as_deref().unwrap_or("Widget"));

        let local_bounds = if let Some(parent) = arena.parent(id) {
            if let Some(parent_hot) = arena.get_hot(parent) {
                Rect::new(
                    hot.bounds.origin.x - parent_hot.bounds.origin.x,
                    hot.bounds.origin.y - parent_hot.bounds.origin.y,
                    hot.bounds.size.x,
                    hot.bounds.size.y,
                )
            } else {
                hot.bounds
            }
        } else {
            hot.bounds
        };

        let active_lint_count = self.active_lints.get(&id).copied().unwrap_or(0);
        let suppressed_lint_count = self.suppressed_lints.get(&id).copied().unwrap_or(0);
        let signal_count = self.active_signals.get(&id).copied().unwrap_or(0);
        let signal_fired = self.fired_signals.contains(&id);

        let badges = NodeBadges {
            has_active_lints: active_lint_count > 0,
            signal_fired,
            has_suppressed_lints: suppressed_lint_count > 0
                || !markers.lint_suppressions.is_empty(),
        };

        let child_count = arena.children(id).count();
        let is_expanded = self.is_expanded(id);

        let children = if is_expanded {
            let mut resolved_children = Vec::new();
            let mut count = 0;
            for child_id in arena.children(id) {
                if count < self.virtual_threshold {
                    if let Some(child_node) = self.resolve_node(arena, child_id, depth + 1) {
                        resolved_children.push(child_node);
                    }
                    count += 1;
                } else {
                    let remaining = child_count - count;
                    resolved_children.push(InspectorTreeNode {
                        id: WidgetId::from_parts(u32::MAX, 1),
                        debug_name: None,
                        kind: NodeKind::Content,
                        screen_bounds: Rect::default(),
                        local_bounds: Rect::default(),
                        depth: depth + 1,
                        child_count: 0,
                        badges: NodeBadges::default(),
                        active_signal_count: 0,
                        is_expanded: false,
                        is_virtual_placeholder: true,
                        virtual_placeholder_label: Some(format!("+{remaining} rows")),
                        children: None,
                    });
                    break;
                }
            }
            Some(resolved_children)
        } else {
            None
        };

        Some(InspectorTreeNode {
            id,
            debug_name: display_name,
            kind,
            screen_bounds: hot.bounds,
            local_bounds,
            depth,
            child_count,
            badges,
            active_signal_count: signal_count,
            is_expanded,
            is_virtual_placeholder: false,
            virtual_placeholder_label: None,
            children,
        })
    }

    /// Flattens currently expanded visible nodes into a linear sequence of rows
    /// suitable for a virtualized list tree view.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::{ColdNode, DummyWidget, HotNode, WidgetArena};
    /// use martensite_devtools::inspector::InspectorTreeModel;
    ///
    /// let mut arena = WidgetArena::new();
    /// let root = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
    /// let model = InspectorTreeModel::new(root);
    /// let rows = model.flatten_visible(&arena);
    /// assert_eq!(rows.len(), 1);
    /// assert_eq!(rows[0].id, root);
    /// ```
    pub fn flatten_visible(&self, arena: &WidgetArena) -> Vec<InspectorTreeNode> {
        let mut rows = Vec::new();
        if let Some(root_node) = self.resolve_node(arena, self.root_id, 0) {
            fn flatten_rec(node: InspectorTreeNode, rows: &mut Vec<InspectorTreeNode>) {
                let children = node.children.clone();
                rows.push(node);
                if let Some(children) = children {
                    for child in children {
                        flatten_rec(child, rows);
                    }
                }
            }
            flatten_rec(root_node, &mut rows);
        }
        rows
    }
}

/// Axis classification for layout constraint analysis.
///
/// # Examples
///
/// ```
/// use martensite_devtools::inspector::Axis;
///
/// let axis = Axis::Horizontal;
/// assert_eq!(axis, Axis::Horizontal);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Axis {
    /// Horizontal (X) axis.
    Horizontal,
    /// Vertical (Y) axis.
    Vertical,
    /// Both axes simultaneously.
    Both,
}

/// Overflow details for a widget.
///
/// # Examples
///
/// ```
/// use martensite_devtools::inspector::OverflowInfo;
///
/// let info = OverflowInfo::new(10.0, 0.0);
/// assert!(info.has_horizontal_overflow);
/// assert!(!info.has_vertical_overflow);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OverflowInfo {
    /// Overflow amount along horizontal axis in logical pixels.
    pub overflow_x: f32,
    /// Overflow amount along vertical axis in logical pixels.
    pub overflow_y: f32,
    /// Whether horizontal overflow is present (> 0.01 px).
    pub has_horizontal_overflow: bool,
    /// Whether vertical overflow is present (> 0.01 px).
    pub has_vertical_overflow: bool,
}

impl OverflowInfo {
    /// Creates a new `OverflowInfo` from raw overflow deltas.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::inspector::OverflowInfo;
    ///
    /// let info = OverflowInfo::new(5.0, 0.0);
    /// assert!(info.has_overflow());
    /// ```
    pub fn new(overflow_x: f32, overflow_y: f32) -> Self {
        Self {
            overflow_x: overflow_x.max(0.0),
            overflow_y: overflow_y.max(0.0),
            has_horizontal_overflow: overflow_x > 0.01,
            has_vertical_overflow: overflow_y > 0.01,
        }
    }

    /// Returns `true` if any axis has overflow.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::inspector::OverflowInfo;
    ///
    /// let info = OverflowInfo::new(0.0, 0.0);
    /// assert!(!info.has_overflow());
    /// ```
    pub fn has_overflow(&self) -> bool {
        self.has_horizontal_overflow || self.has_vertical_overflow
    }
}

/// A constraint violation detected during layout inspection.
///
/// # Examples
///
/// ```
/// use martensite_core::UnderflowPolicy;
/// use martensite_devtools::inspector::{Axis, ConstraintViolation};
///
/// let violation = ConstraintViolation::Underflow {
///     axis: Axis::Horizontal,
///     deficit: 20.0,
///     minimum: 100.0,
///     policy: UnderflowPolicy::Clip,
/// };
/// assert!(matches!(violation, ConstraintViolation::Underflow { .. }));
/// ```
#[derive(Debug, Clone, PartialEq)]
pub enum ConstraintViolation {
    /// Allocated size is smaller than the declared render minimum (underflow).
    Underflow {
        /// Axis along which underflow occurred.
        axis: Axis,
        /// Shortfall in logical pixels.
        deficit: f32,
        /// Required minimum size.
        minimum: f32,
        /// Active underflow policy.
        policy: UnderflowPolicy,
    },
    /// Content or child bounds exceed parent allocated bounds (overflow).
    Overflow {
        /// Axis along which overflow occurred.
        axis: Axis,
        /// Excess pixels beyond parent boundary.
        excess: f32,
        /// Size of parent along this axis.
        parent_size: f32,
        /// Size of child along this axis.
        child_size: f32,
    },
    /// Allocated size violates offered min/max layout constraints.
    ConstraintUnsatisfied {
        /// Minimum offered size.
        min: Vec2,
        /// Maximum offered size.
        max: Vec2,
        /// Actual allocated size.
        actual: Vec2,
    },
}

/// A single step in the constraint resolution chain.
///
/// # Examples
///
/// ```
/// use glam::Vec2;
/// use martensite_core::{LayoutConstraints, Rect, WidgetId};
/// use martensite_devtools::inspector::ConstraintStep;
///
/// let step = ConstraintStep {
///     widget_id: WidgetId::from_parts(1, 1),
///     debug_name: Some("Container".to_string()),
///     offered_constraints: LayoutConstraints { min_size: Vec2::ZERO, max_size: Vec2::splat(500.0) },
///     resolved_size: Vec2::new(100.0, 50.0),
///     allocated_bounds: Rect::new(0.0, 0.0, 100.0, 50.0),
///     violations: Vec::new(),
/// };
/// assert!(step.violations.is_empty());
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct ConstraintStep {
    /// Widget identifier.
    pub widget_id: WidgetId,
    /// Debug name of the widget at this step.
    pub debug_name: Option<String>,
    /// Constraints offered by the parent (`min_size` and `max_size`).
    pub offered_constraints: LayoutConstraints,
    /// Resolved / measured size.
    pub resolved_size: Vec2,
    /// Final allocated bounding rectangle.
    pub allocated_bounds: Rect,
    /// Violations detected at this step.
    pub violations: Vec<ConstraintViolation>,
}

/// Summary of layout parameters for the inspected widget.
///
/// # Examples
///
/// ```
/// use glam::Vec2;
/// use martensite_devtools::inspector::LayoutStyleSummary;
///
/// let summary = LayoutStyleSummary {
///     allocated_size: Vec2::new(100.0, 50.0),
///     render_minimum: Vec2::new(20.0, 20.0),
///     is_underflowed: false,
///     is_overflowed: false,
/// };
/// assert!(!summary.is_underflowed);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayoutStyleSummary {
    /// Allocated width and height.
    pub allocated_size: Vec2,
    /// Declared render minimum.
    pub render_minimum: Vec2,
    /// Whether underflow is engaged.
    pub is_underflowed: bool,
    /// Whether overflow is detected.
    pub is_overflowed: bool,
}

/// Layout inspection report for a selected widget.
///
/// # Examples
///
/// ```
/// use glam::Vec2;
/// use martensite_core::{LayoutConstraints, Rect, RenderMinimum, UnderflowPolicy, WidgetId};
/// use martensite_devtools::inspector::{LayoutInspection, LayoutStyleSummary, OverflowInfo};
///
/// let inspection = LayoutInspection {
///     widget_id: WidgetId::from_parts(1, 1),
///     allocated_bounds: Rect::new(0.0, 0.0, 100.0, 50.0),
///     intrinsic_measure: Some(Vec2::new(90.0, 45.0)),
///     render_minimum: RenderMinimum::ZERO,
///     underflow_engaged: false,
///     overflow: None,
///     constraint_chain: Vec::new(),
///     style_summary: LayoutStyleSummary {
///         allocated_size: Vec2::new(100.0, 50.0),
///         render_minimum: Vec2::ZERO,
///         is_underflowed: false,
///         is_overflowed: false,
///     },
/// };
/// assert_eq!(inspection.allocated_bounds.width(), 100.0);
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutInspection {
    /// Inspected widget identifier.
    pub widget_id: WidgetId,
    /// Allocated screen-space bounds.
    pub allocated_bounds: Rect,
    /// Intrinsic measure if cached or available.
    pub intrinsic_measure: Option<Vec2>,
    /// Effective render minimum area and policy.
    pub render_minimum: RenderMinimum,
    /// Whether underflow is currently engaged.
    pub underflow_engaged: bool,
    /// Overflow details against parent bounds if applicable.
    pub overflow: Option<OverflowInfo>,
    /// Constraint resolution chain from root down to this widget.
    pub constraint_chain: Vec<ConstraintStep>,
    /// Layout style summary.
    pub style_summary: LayoutStyleSummary,
}

/// Layout inspector engine for analyzing constraints, underflow, and overflow.
///
/// # Examples
///
/// ```
/// use martensite_core::{ColdNode, DummyWidget, HotNode, Rect, WidgetArena};
/// use martensite_devtools::inspector::LayoutInspector;
///
/// let mut arena = WidgetArena::new();
/// let root = arena.insert_with_widget(
///     HotNode { bounds: Rect::new(0.0, 0.0, 500.0, 500.0), ..HotNode::default() },
///     Box::new(DummyWidget),
/// );
///
/// let inspection = LayoutInspector::inspect(&arena, root).expect("inspected root");
/// assert_eq!(inspection.allocated_bounds.width(), 500.0);
/// ```
pub struct LayoutInspector;

impl LayoutInspector {
    /// Inspects the layout of `target` and computes the complete constraint chain.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::{ColdNode, DummyWidget, HotNode, Rect, WidgetArena};
    /// use martensite_devtools::inspector::LayoutInspector;
    ///
    /// let mut arena = WidgetArena::new();
    /// let root = arena.insert_with_widget(
    ///     HotNode { bounds: Rect::new(0.0, 0.0, 300.0, 300.0), ..HotNode::default() },
    ///     Box::new(DummyWidget),
    /// );
    ///
    /// let report = LayoutInspector::inspect(&arena, root).unwrap();
    /// assert_eq!(report.constraint_chain.len(), 1);
    /// ```
    pub fn inspect(arena: &WidgetArena, target: WidgetId) -> Option<LayoutInspection> {
        let hot = arena.get_hot(target)?;
        let cold = arena.get_cold(target)?;

        let mut path = Vec::new();
        let mut curr = Some(target);
        while let Some(id) = curr {
            path.push(id);
            curr = arena.parent(id);
        }
        path.reverse();

        let mut constraint_chain = Vec::with_capacity(path.len());
        for &node_id in &path {
            let n_hot = arena.get_hot(node_id)?;
            let n_cold = arena.get_cold(node_id)?;

            let (offered_constraints, parent_bounds) = if let Some(parent) = arena.parent(node_id) {
                if let Some(p_hot) = arena.get_hot(parent) {
                    (
                        LayoutConstraints {
                            min_size: Vec2::ZERO,
                            max_size: p_hot.bounds.size,
                        },
                        Some(p_hot.bounds),
                    )
                } else {
                    (
                        LayoutConstraints {
                            min_size: Vec2::ZERO,
                            max_size: Vec2::new(f32::INFINITY, f32::INFINITY),
                        },
                        None,
                    )
                }
            } else {
                (
                    LayoutConstraints {
                        min_size: Vec2::ZERO,
                        max_size: n_hot.bounds.size,
                    },
                    None,
                )
            };

            let mut violations = Vec::new();
            let min_render = n_cold.effective_render_minimum();

            // Check underflow
            if min_render.size.x > 0.0 && n_hot.bounds.width() + 0.01 < min_render.size.x {
                violations.push(ConstraintViolation::Underflow {
                    axis: Axis::Horizontal,
                    deficit: min_render.size.x - n_hot.bounds.width(),
                    minimum: min_render.size.x,
                    policy: min_render.policy,
                });
            }
            if min_render.size.y > 0.0 && n_hot.bounds.height() + 0.01 < min_render.size.y {
                violations.push(ConstraintViolation::Underflow {
                    axis: Axis::Vertical,
                    deficit: min_render.size.y - n_hot.bounds.height(),
                    minimum: min_render.size.y,
                    policy: min_render.policy,
                });
            }

            // Check overflow against parent
            if let Some(pb) = parent_bounds {
                let excess_x = n_hot.bounds.max_x() - pb.max_x();
                let excess_y = n_hot.bounds.max_y() - pb.max_y();
                if excess_x > 0.01 {
                    violations.push(ConstraintViolation::Overflow {
                        axis: Axis::Horizontal,
                        excess: excess_x,
                        parent_size: pb.width(),
                        child_size: n_hot.bounds.width(),
                    });
                }
                if excess_y > 0.01 {
                    violations.push(ConstraintViolation::Overflow {
                        axis: Axis::Vertical,
                        excess: excess_y,
                        parent_size: pb.height(),
                        child_size: n_hot.bounds.height(),
                    });
                }
            }

            let debug_name = n_cold.debug_name.map(|d| {
                let (name, _) = InlineMarkers::parse(d);
                name
            });

            constraint_chain.push(ConstraintStep {
                widget_id: node_id,
                debug_name,
                offered_constraints,
                resolved_size: n_hot.bounds.size,
                allocated_bounds: n_hot.bounds,
                violations,
            });
        }

        let overflow = if let Some(parent) = arena.parent(target) {
            let p_hot = arena.get_hot(parent)?;
            let excess_x = (hot.bounds.max_x() - p_hot.bounds.max_x()).max(0.0);
            let excess_y = (hot.bounds.max_y() - p_hot.bounds.max_y()).max(0.0);
            Some(OverflowInfo::new(excess_x, excess_y))
        } else {
            None
        };

        let intrinsic_measure = cold
            .text_cache
            .entries()
            .iter()
            .find(|e| !e.0.is_nan())
            .map(|e| Vec2::new(e.1, e.2));

        let render_minimum = cold.effective_render_minimum();
        let is_underflowed = cold.underflow_engaged
            || (render_minimum.size.x > 0.0 && hot.bounds.width() + 0.01 < render_minimum.size.x)
            || (render_minimum.size.y > 0.0 && hot.bounds.height() + 0.01 < render_minimum.size.y);

        let is_overflowed = overflow.is_some_and(|o| o.has_overflow());

        let style_summary = LayoutStyleSummary {
            allocated_size: hot.bounds.size,
            render_minimum: render_minimum.size,
            is_underflowed,
            is_overflowed,
        };

        Some(LayoutInspection {
            widget_id: target,
            allocated_bounds: hot.bounds,
            intrinsic_measure,
            render_minimum,
            underflow_engaged: cold.underflow_engaged,
            overflow,
            constraint_chain,
            style_summary,
        })
    }
}

/// Parsed inline semantic markers from widget `debug_name`.
///
/// Handles `@level:1..4`, `@lint:rule1,rule2`, `@alarm`, `@priority:N`,
/// `@kpi`, `@destructive`, and custom key-value tags.
///
/// # Examples
///
/// ```
/// use martensite_devtools::inspector::InlineMarkers;
///
/// let (display_name, markers) = InlineMarkers::parse("Overview@level:2@alarm@lint:contrast");
/// assert_eq!(display_name, "Overview");
/// assert_eq!(markers.isa_level, Some(2));
/// assert!(markers.is_alarm);
/// assert_eq!(markers.lint_suppressions, vec!["contrast"]);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InlineMarkers {
    /// ISA-101 level (1..=4).
    pub isa_level: Option<u8>,
    /// Suppressed design-lint rules from `@lint:...`.
    pub lint_suppressions: Vec<String>,
    /// Whether marked as an alarm device (`@alarm`).
    pub is_alarm: bool,
    /// Alarm or widget priority (`@priority:N`).
    pub priority: Option<u8>,
    /// Key performance indicator marker (`@kpi`).
    pub is_kpi: bool,
    /// Destructive action marker (`@destructive`).
    pub is_destructive: bool,
    /// Custom key-value or flag markers.
    pub custom_markers: Vec<(String, Option<String>)>,
}

impl InlineMarkers {
    /// Parses markers from a debug name string (e.g. `"Button@alarm@priority:1@lint:contrast"`).
    ///
    /// Returns the cleaned display name and the parsed markers struct.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::inspector::InlineMarkers;
    ///
    /// let (name, markers) = InlineMarkers::parse("MyWidget@level:1@kpi");
    /// assert_eq!(name, "MyWidget");
    /// assert_eq!(markers.isa_level, Some(1));
    /// assert!(markers.is_kpi);
    /// ```
    pub fn parse(raw_name: &str) -> (String, Self) {
        let mut markers = Self::default();
        let (base_part, marker_part) = if let Some(idx) = raw_name.find('@') {
            (&raw_name[..idx], &raw_name[idx..])
        } else {
            (raw_name, "")
        };

        // Also strip any `#source/path:line` fragment from base name
        let clean_base = if let Some(hash_idx) = base_part.find('#') {
            &base_part[..hash_idx]
        } else {
            base_part
        };

        for section in marker_part.split('@').filter(|s| !s.is_empty()) {
            let section_trimmed = section.trim();
            if let Some(spec) = section_trimmed.strip_prefix("lint:") {
                for item in spec.split(',') {
                    let s = item.trim().to_ascii_lowercase();
                    if !s.is_empty() && !markers.lint_suppressions.contains(&s) {
                        markers.lint_suppressions.push(s);
                    }
                }
            } else if let Some(lvl_str) = section_trimmed.strip_prefix("level:") {
                if let Ok(lvl) = lvl_str.trim().parse::<u8>() {
                    if (1..=4).contains(&lvl) {
                        markers.isa_level = Some(lvl);
                    }
                }
            } else if let Some(prio_str) = section_trimmed.strip_prefix("priority:") {
                if let Ok(prio) = prio_str.trim().parse::<u8>() {
                    markers.priority = Some(prio);
                }
            } else if section_trimmed.eq_ignore_ascii_case("alarm") {
                markers.is_alarm = true;
            } else if section_trimmed.eq_ignore_ascii_case("kpi") {
                markers.is_kpi = true;
            } else if section_trimmed.eq_ignore_ascii_case("destructive") {
                markers.is_destructive = true;
            } else {
                let lower = section_trimmed.to_ascii_lowercase();
                if let Some((k, v)) = lower.split_once(':') {
                    markers
                        .custom_markers
                        .push((k.trim().to_string(), Some(v.trim().to_string())));
                } else {
                    markers.custom_markers.push((lower, None));
                }
            }
        }

        (clean_base.trim().to_string(), markers)
    }
}

/// Information about a reactive signal dependency tracked on a widget.
///
/// # Examples
///
/// ```
/// use martensite_devtools::inspector::TrackedSignalInfo;
///
/// let sig = TrackedSignalInfo::new("counter", "42", true);
/// assert_eq!(sig.name, "counter");
/// assert_eq!(sig.value_repr, "42");
/// assert!(sig.updated_this_frame);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackedSignalInfo {
    /// Signal identifier or name.
    pub name: String,
    /// String representation of current signal value.
    pub value_repr: String,
    /// Whether this signal updated during the last frame.
    pub updated_this_frame: bool,
}

impl TrackedSignalInfo {
    /// Creates a new `TrackedSignalInfo`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::inspector::TrackedSignalInfo;
    ///
    /// let info = TrackedSignalInfo::new("temp_c", "21.5", false);
    /// assert_eq!(info.name, "temp_c");
    /// ```
    pub fn new(
        name: impl Into<String>,
        value_repr: impl Into<String>,
        updated_this_frame: bool,
    ) -> Self {
        Self {
            name: name.into(),
            value_repr: value_repr.into(),
            updated_this_frame,
        }
    }
}

/// Extracted properties, metadata, and reactive bindings for a widget.
///
/// # Examples
///
/// ```
/// use accesskit::Role;
/// use martensite_core::{ColdNode, DummyWidget, HotNode, WidgetArena};
/// use martensite_devtools::inspector::WidgetProperties;
///
/// let mut arena = WidgetArena::new();
/// let id = arena.insert(
///     HotNode::default(),
///     ColdNode::new(Box::new(DummyWidget))
///         .with_name("SensorCard@level:1@alarm")
///         .with_role(Role::Application),
/// );
///
/// let props = WidgetProperties::from_arena(&arena, id).expect("extracted properties");
/// assert_eq!(props.display_name, "SensorCard");
/// assert!(props.markers.is_alarm);
/// assert_eq!(props.markers.isa_level, Some(1));
/// assert_eq!(props.a11y_role, Role::Application);
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct WidgetProperties {
    /// Inspected widget identifier.
    pub widget_id: WidgetId,
    /// Raw unparsed debug name from [`ColdNode::debug_name`].
    pub raw_debug_name: Option<String>,
    /// Base display name stripped of `@` markers and source spans.
    pub display_name: String,
    /// Clickable source location path (e.g. `"src/pages/overview.rs:142"`).
    pub source_location: Option<String>,
    /// Parsed semantic markers (`@level`, `@lint`, `@alarm`, etc.).
    pub markers: InlineMarkers,
    /// Accessibility role from [`ColdNode::a11y_role`].
    pub a11y_role: accesskit::Role,
    /// Accessibility name / label from [`ColdNode::a11y_name`].
    pub a11y_name: Option<String>,
    /// Tooltip text from [`ColdNode::tooltip`].
    pub tooltip: Option<String>,
    /// Classified [`NodeKind`].
    pub node_kind: NodeKind,
    /// Interaction and visibility flags from [`HotNode::flags`].
    pub flags: NodeFlags,
    /// Effective underflow policy.
    pub underflow_policy: UnderflowPolicy,
    /// Whether underflow is currently engaged.
    pub underflow_engaged: bool,
    /// Tracked reactive signal dependencies.
    pub tracked_signals: Vec<TrackedSignalInfo>,
}

impl WidgetProperties {
    /// Extracts widget properties and metadata from the arena.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::{ColdNode, DummyWidget, HotNode, WidgetArena};
    /// use martensite_devtools::inspector::WidgetProperties;
    ///
    /// let mut arena = WidgetArena::new();
    /// let id = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
    /// let props = WidgetProperties::from_arena(&arena, id).unwrap();
    /// assert_eq!(props.widget_id, id);
    /// ```
    pub fn from_arena(arena: &WidgetArena, id: WidgetId) -> Option<Self> {
        let hot: &HotNode = arena.get_hot(id)?;
        let cold: &ColdNode = arena.get_cold(id)?;

        let raw = cold.debug_name;
        let source_location = raw.and_then(Self::extract_source_location);
        let (display_name, markers) = if let Some(raw_str) = raw {
            let (name, m) = InlineMarkers::parse(raw_str);
            (name, m)
        } else {
            ("Widget".to_string(), InlineMarkers::default())
        };

        let node_kind = NodeKind::from_widget_name(&display_name);

        Some(Self {
            widget_id: id,
            raw_debug_name: raw.map(ToString::to_string),
            display_name,
            source_location,
            markers,
            a11y_role: cold.a11y_role,
            a11y_name: cold.a11y_name.clone(),
            tooltip: cold.tooltip.clone(),
            node_kind,
            flags: hot.flags,
            underflow_policy: cold.effective_render_minimum().policy,
            underflow_engaged: cold.underflow_engaged,
            tracked_signals: Vec::new(),
        })
    }

    /// Attaches tracked signal dependency information to this properties view.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::{ColdNode, DummyWidget, HotNode, WidgetArena};
    /// use martensite_devtools::inspector::{TrackedSignalInfo, WidgetProperties};
    ///
    /// let mut arena = WidgetArena::new();
    /// let id = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
    /// let props = WidgetProperties::from_arena(&arena, id).unwrap()
    ///     .with_signals(vec![TrackedSignalInfo::new("count", "10", false)]);
    /// assert_eq!(props.tracked_signals.len(), 1);
    /// ```
    pub fn with_signals(mut self, signals: Vec<TrackedSignalInfo>) -> Self {
        self.tracked_signals = signals;
        self
    }

    /// Extracts source code location from debug name string if present.
    ///
    /// Matches format `"WidgetName#src/pages/overview.rs:142"` or `"src/overview.rs:142"`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::inspector::WidgetProperties;
    ///
    /// assert_eq!(
    ///     WidgetProperties::extract_source_location("Button#src/pages/overview.rs:142"),
    ///     Some("src/pages/overview.rs:142".to_string())
    /// );
    /// ```
    pub fn extract_source_location(debug_name: &str) -> Option<String> {
        if let Some(hash_idx) = debug_name.find('#') {
            let path_part = &debug_name[hash_idx + 1..];
            let clean_path = path_part.split('@').next()?.trim();
            if !clean_path.is_empty() {
                return Some(clean_path.to_string());
            }
        }

        // Check if debug name itself looks like a source span: `path/file.rs:123`
        if debug_name.ends_with(".rs") || debug_name.contains(".rs:") {
            let candidate = debug_name.split('@').next()?.trim();
            if candidate.contains(".rs") {
                return Some(candidate.to_string());
            }
        }

        None
    }
}
