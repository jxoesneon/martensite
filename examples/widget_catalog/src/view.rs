//! View constructors for the widget catalog presentation and interactive cards.

use martensite::widgets::{Container, Flex, ScrollView, Text};
use martensite_core::Widget;

use crate::model::{CatalogModel, WidgetEntry, WidgetStateKind};

/// Extension helper to add boxed widgets to `Flex`.
trait FlexBoxExt {
    fn child_box(self, child: Box<dyn Widget>) -> Self;
}

impl FlexBoxExt for Flex {
    fn child_box(mut self, child: Box<dyn Widget>) -> Self {
        self.children.push(child);
        self
    }
}

/// Helper to wrap a boxed widget inside a padded `Container`.
fn box_in_container(child: Box<dyn Widget>, padding: f32) -> Container {
    let mut c = Container::new().padding_uniform(padding);
    c.child = Some(child);
    c
}

/// Formats cross-framework aliases into a clean readable string.
fn format_aliases(aliases: &[(&'static str, &'static str)]) -> String {
    aliases
        .iter()
        .map(|(fw, name)| format!("{fw}: {name}"))
        .collect::<Vec<_>>()
        .join("  |  ")
}

/// Builds an individual widget presentation card showcasing metadata,
/// live standard states, and runnable code snippets.
pub fn build_widget_card(entry: &WidgetEntry) -> Box<dyn Widget> {
    let title_text = Text::new(entry.name).font_size(18.0);
    let role_text = Text::new(format!("Role: {:?}", entry.accesskit_role)).font_size(12.0);
    let aliases_str = format!("Aliases: {}", format_aliases(entry.aliases));
    let aliases_text = Text::new(aliases_str).font_size(12.0);
    let desc_text = Text::new(entry.description).font_size(13.0);

    // Standard states grid: Default, Hover, Disabled, Focused
    let default_widget = (entry.instantiate)(WidgetStateKind::Default);
    let hover_widget = (entry.instantiate)(WidgetStateKind::Hover);
    let disabled_widget = (entry.instantiate)(WidgetStateKind::Disabled);
    let focused_widget = (entry.instantiate)(WidgetStateKind::Focused);

    let states_row = Flex::row()
        .gap(16.0)
        .child(
            Flex::column()
                .gap(6.0)
                .child(Text::new("Default State").font_size(11.0))
                .child_box(default_widget),
        )
        .child(
            Flex::column()
                .gap(6.0)
                .child(Text::new("Hover State").font_size(11.0))
                .child_box(hover_widget),
        )
        .child(
            Flex::column()
                .gap(6.0)
                .child(Text::new("Disabled State").font_size(11.0))
                .child_box(disabled_widget),
        )
        .child(
            Flex::column()
                .gap(6.0)
                .child(Text::new("Focused State").font_size(11.0))
                .child_box(focused_widget),
        );

    // Code snippet section
    let snippet_label = Text::new("Runnable Code Snippet:").font_size(12.0);
    let snippet_box = Container::new()
        .padding_uniform(8.0)
        .child(Text::new(entry.code_snippet).font_size(12.0));

    let card_content = Flex::column()
        .gap(12.0)
        .child(Flex::row().gap(16.0).child(title_text).child(role_text))
        .child(aliases_text)
        .child(desc_text)
        .child(Text::new("Standard States:").font_size(12.0))
        .child(states_row)
        .child(snippet_label)
        .child(snippet_box);

    Box::new(box_in_container(Box::new(card_content), 16.0))
}

/// Builds the complete catalog user interface.
pub fn build_catalog_view(model: &CatalogModel) -> Box<dyn Widget> {
    let header_title = Text::new("Martensite Widget Catalog").font_size(24.0);
    let header_subtitle = Text::new(
        "Interactive reference and cross-framework migration map for Martensite widgets.",
    )
    .font_size(13.0);

    let query = model.search_query.get();
    let filter_info = if query.is_empty() {
        format!("Showing all {} catalog entries", model.entries.len())
    } else {
        format!(
            "Search filter \"{}\" ({} matches)",
            query,
            model.filtered_entries().len()
        )
    };
    let filter_text = Text::new(filter_info).font_size(12.0);

    let mut entries_col = Flex::column().gap(20.0);
    for entry in model.filtered_entries() {
        entries_col = entries_col.child_box(build_widget_card(entry));
    }

    let scrollable_content = ScrollView::new(entries_col);

    let root_column = Flex::column()
        .gap(16.0)
        .child(header_title)
        .child(header_subtitle)
        .child(filter_text)
        .child(scrollable_content);

    Box::new(Container::new().padding_uniform(16.0).child(root_column))
}
