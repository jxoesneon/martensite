//! Data model, widget metadata, and cross-framework search indexing for the Martensite Widget Catalog.

use accesskit::Role;
use martensite_core::Widget;
use martensite_reactive::{create_signal, Signal};

/// Widget family groupings showcased in the catalog.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum WidgetFamily {
    /// Interactive input controls (Button, Slider, Toggle, TextInput, CheckBox).
    Controls,
    /// Spatial and layout containers (Flex, Stack, Container, ScrollView).
    Containers,
    /// Tabular and collection data viewers (Table, DataGrid, ListView).
    Data,
    /// Floating, anchored, and modal overlays (Tooltip, Popover, Popconfirm).
    Overlays,
    /// Application and page navigation controls (Tabs, NavRail).
    Navigation,
}

impl WidgetFamily {
    /// All widget families in display order.
    pub const ALL: [WidgetFamily; 5] = [
        WidgetFamily::Controls,
        WidgetFamily::Containers,
        WidgetFamily::Data,
        WidgetFamily::Overlays,
        WidgetFamily::Navigation,
    ];

    /// Human-readable display label.
    pub fn as_str(&self) -> &'static str {
        match self {
            WidgetFamily::Controls => "Controls",
            WidgetFamily::Containers => "Containers",
            WidgetFamily::Data => "Data Displays",
            WidgetFamily::Overlays => "Overlays & Popups",
            WidgetFamily::Navigation => "Navigation",
        }
    }

    /// Icon or symbol representing the family.
    pub fn icon(&self) -> &'static str {
        match self {
            WidgetFamily::Controls => "🎛",
            WidgetFamily::Containers => "📦",
            WidgetFamily::Data => "📊",
            WidgetFamily::Overlays => "💬",
            WidgetFamily::Navigation => "🧭",
        }
    }
}

/// Standard presentation states required for each widget showcase entry.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum WidgetStateKind {
    /// Normal, un-interacted baseline state.
    Default,
    /// Active pointer hover or highlighted state.
    Hover,
    /// Disabled / inert state.
    Disabled,
    /// Keyboard focused / active input state.
    Focused,
}

impl WidgetStateKind {
    /// All standard states in display order.
    pub const ALL: [WidgetStateKind; 4] = [
        WidgetStateKind::Default,
        WidgetStateKind::Hover,
        WidgetStateKind::Disabled,
        WidgetStateKind::Focused,
    ];

    /// Human-readable label for state badges.
    pub fn as_str(&self) -> &'static str {
        match self {
            WidgetStateKind::Default => "Default",
            WidgetStateKind::Hover => "Hover",
            WidgetStateKind::Disabled => "Disabled",
            WidgetStateKind::Focused => "Focused",
        }
    }
}

/// A catalog entry for a single widget type.
#[derive(Clone)]
pub struct WidgetEntry {
    /// Canonical Martensite widget name (e.g. "Button", "Slider").
    pub name: &'static str,
    /// Family category.
    pub family: WidgetFamily,
    /// Brief description of the widget's function and APG/a11y contract.
    pub description: &'static str,
    /// Native AccessKit accessibility role.
    pub accesskit_role: Role,
    /// Cross-framework equivalence aliases: `(framework, alias)`.
    pub aliases: &'static [(&'static str, &'static str)],
    /// Minimal runnable code snippet demonstrating instantiation.
    pub code_snippet: &'static str,
    /// Factory function producing a live widget in the requested state.
    pub instantiate: fn(WidgetStateKind) -> Box<dyn Widget>,
}

impl WidgetEntry {
    /// Checks whether this entry matches a search query string.
    ///
    /// Matches against:
    /// - Widget name (e.g. "Button")
    /// - Family name (e.g. "Controls")
    /// - Description keywords
    /// - Cross-framework aliases (e.g. "QPushButton", "GtkButton", "VStack", "<input type=\"text\">")
    pub fn matches_query(&self, query: &str) -> bool {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return true;
        }

        if self.name.to_lowercase().contains(&q) {
            return true;
        }

        if self.family.as_str().to_lowercase().contains(&q) {
            return true;
        }

        if self.description.to_lowercase().contains(&q) {
            return true;
        }

        // Search cross-framework aliases (framework name + alias symbol)
        for (framework, alias) in self.aliases {
            if framework.to_lowercase().contains(&q) || alias.to_lowercase().contains(&q) {
                return true;
            }
        }

        false
    }
}

/// Reactive catalog model managing entries and live search filters.
#[derive(Clone)]
pub struct CatalogModel {
    /// Registered catalog entries.
    pub entries: Vec<WidgetEntry>,
    /// Active search filter string.
    pub search_query: Signal<String>,
    /// Selected family filter (or `None` for all families).
    pub selected_family: Signal<Option<WidgetFamily>>,
}

impl CatalogModel {
    /// Creates a new catalog model initialized with the given entries.
    pub fn new(entries: Vec<WidgetEntry>) -> Self {
        Self {
            entries,
            search_query: create_signal(String::new()),
            selected_family: create_signal(None),
        }
    }

    /// Sets the search filter query.
    pub fn set_search_query(&self, query: impl Into<String>) {
        self.search_query.set(query.into());
    }

    /// Sets or clears the active family filter.
    pub fn set_family(&self, family: Option<WidgetFamily>) {
        self.selected_family.set(family);
    }

    /// Returns the filtered slice of entries matching current query and family.
    pub fn filtered_entries(&self) -> Vec<&WidgetEntry> {
        let query = self.search_query.get();
        let family_filter = self.selected_family.get();

        self.entries
            .iter()
            .filter(|e| {
                if let Some(family) = family_filter {
                    if e.family != family {
                        return false;
                    }
                }
                e.matches_query(&query)
            })
            .collect()
    }
}
