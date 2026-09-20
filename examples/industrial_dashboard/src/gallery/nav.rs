//! Navigation & layout category — one live card per assigned widget
//! module. Every construction is adapted from the widget's own `///`
//! doctest (the verified canonical example in its source file).

use martensite::core::Widget;
use martensite::widgets::about::About;
use martensite::widgets::accordion::Accordion;
use martensite::widgets::anchor::{Anchor, AnchorItem};
use martensite::widgets::app_grid::{AppEntry, AppGrid};
use martensite::widgets::aspect_frame::AspectFrame;
use martensite::widgets::breadcrumb::Breadcrumb;
use martensite::widgets::button::Button;
use martensite::widgets::card_deck::CardDeck;
use martensite::widgets::clamp::Clamp;
use martensite::widgets::container::Container;
use martensite::widgets::disclosure::Disclosure;
use martensite::widgets::dock::{Dock, DockItem};
use martensite::widgets::drawer::Drawer;
use martensite::widgets::expander_row::ExpanderRow;
use martensite::widgets::filmstrip::Thumbnail;
use martensite::widgets::flex::Flex;
use martensite::widgets::flow_box::{FlowBox, FlowSelection};
use martensite::widgets::grid::{Grid, GridCell};
use martensite::widgets::group_box::GroupBox;
use martensite::widgets::hero_header::HeroHeader;
use martensite::widgets::masonry::Masonry;
use martensite::widgets::nav_rail::NavRail;
use martensite::widgets::nav_stack::NavStack;
use martensite::widgets::page_header::PageHeader;
use martensite::widgets::pagination::Pagination;
use martensite::widgets::pips_pager::PipsPager;
use martensite::widgets::pull_to_refresh::PullToRefresh;
use martensite::widgets::resize_handle::ResizeHandle;
use martensite::widgets::scroll_indicator::ScrollIndicator;
use martensite::widgets::scrollview::ScrollView;
use martensite::widgets::separator::Separator;
use martensite::widgets::settings_row::SettingsRow;
use martensite::widgets::split_view::SplitView;
use martensite::widgets::stack::{Stack, StackAlignment};
use martensite::widgets::steps::Steps;
use martensite::widgets::tabs::Tabs;
use martensite::widgets::task_switcher::TaskSwitcher;
use martensite::widgets::text::Text;
use martensite::widgets::viewport::Viewport;
use martensite::widgets::wizard::Wizard;

/// Navigation & layout showcase entries — `(display name, live widget)`.
pub fn entries() -> Vec<(&'static str, Box<dyn Widget>)> {
    vec![
        (
            "About",
            Box::new(
                About::new("Martensite Studio")
                    .version("1.4.0")
                    .copyright("© 2025 Cognition")
                    .website("martensite.dev", "https://martensite.dev")
                    .credits("Written by", ["A. Dev", "B. Designer"]),
            ),
        ),
        (
            "Accordion",
            Box::new(
                Accordion::new()
                    .section("General", Text::new("general settings"))
                    .section("Advanced", Text::new("advanced settings")),
            ),
        ),
        ("Anchor", {
            let mut a = Anchor::new().items([
                AnchorItem::new("Overview", "overview"),
                AnchorItem::new("API", "api"),
            ]);
            a.set_active(1);
            Box::new(a)
        }),
        (
            "App Grid",
            Box::new(
                AppGrid::new()
                    .app(AppEntry::new("Files", [90, 140, 200, 255]))
                    .app(AppEntry::new("Mail", [200, 120, 90, 255]))
                    .app(AppEntry::new("Chat", [90, 190, 120, 255])),
            ),
        ),
        (
            "Aspect Frame",
            Box::new(AspectFrame::new(16.0 / 9.0).child(Text::new("16:9"))),
        ),
        (
            "Breadcrumb",
            Box::new(Breadcrumb::new().segments(["Home", "Docs", "API"])),
        ),
        (
            "Card Deck",
            Box::new(
                CardDeck::new()
                    .card(Text::new("Card 1"))
                    .card(Text::new("Card 2"))
                    .card(Text::new("Card 3")),
            ),
        ),
        (
            "Clamp",
            Box::new(
                Clamp::new()
                    .maximum(420.0)
                    .child(Text::new("clamped content")),
            ),
        ),
        (
            "Container",
            Box::new(
                Container::new()
                    .padding_uniform(10.0)
                    .child(Text::new("Inside container")),
            ),
        ),
        ("Disclosure", {
            let mut d = Disclosure::new("Advanced").child(Text::new("details"));
            d.set_open(true);
            Box::new(d)
        }),
        (
            "Dock",
            Box::new(
                Dock::new()
                    .item(DockItem::new("Mail", [80, 120, 220, 255]).running(true))
                    .item(DockItem::new("Chat", [90, 190, 120, 255])),
            ),
        ),
        (
            "Drawer",
            Box::new(
                Drawer::new("Inspector")
                    .width(280.0)
                    .content(Text::new("Properties")),
            ),
        ),
        (
            "Expander Row",
            Box::new(
                ExpanderRow::new("Network")
                    .subtitle("3 adapters")
                    .child(SettingsRow::new("Ethernet"))
                    .child(SettingsRow::new("Wi-Fi")),
            ),
        ),
        (
            "Flex",
            Box::new(
                Flex::row()
                    .gap(8.0)
                    .child(Text::new("First"))
                    .child(Text::new("Second"))
                    .child(Text::new("Third")),
            ),
        ),
        (
            "Flow Box",
            Box::new(
                FlowBox::new()
                    .gap(6.0)
                    .child(Text::new("one"))
                    .child(Text::new("two"))
                    .child(Text::new("three"))
                    .selection_mode(FlowSelection::Single),
            ),
        ),
        (
            "Grid",
            Box::new(
                Grid::new()
                    .columns(12)
                    .gap(6.0)
                    .cell(GridCell::new(Text::new("half")).col_span(6))
                    .cell(GridCell::new(Text::new("half")).col_span(6))
                    .cell(GridCell::new(Text::new("full")).col_span(12)),
            ),
        ),
        (
            "Group Box",
            Box::new(GroupBox::new("Network").child(Text::new("settings"))),
        ),
        (
            "Hero Header",
            Box::new(
                HeroHeader::new("Ship faster")
                    .eyebrow("Martensite")
                    .subtitle("The widget toolkit for Rust")
                    .primary("Get started"),
            ),
        ),
        (
            "Masonry",
            Box::new(
                Masonry::new()
                    .columns(3)
                    .gap(6.0)
                    .child(Text::new("a"))
                    .child(Text::new("b"))
                    .child(Text::new("c")),
            ),
        ),
        (
            "Nav Rail",
            Box::new(
                NavRail::new()
                    .destination("🏠", "Home")
                    .destination("📊", "Reports")
                    .destination("⚙", "Settings")
                    .selected(0),
            ),
        ),
        ("Nav Stack", {
            let mut nav = NavStack::new(Text::new("root")).title("Home");
            nav.push(Text::new("detail"), "Detail");
            Box::new(nav)
        }),
        (
            "Page Header",
            Box::new(
                PageHeader::new("Invoice #1042")
                    .back(true)
                    .subtitle("Draft")
                    .action(Button::new("Send")),
            ),
        ),
        (
            "Pagination",
            Box::new(Pagination::new().total_pages(20).current(7)),
        ),
        ("Pips Pager", Box::new(PipsPager::new(8).max_visible(5))),
        (
            "Pull to Refresh",
            Box::new(PullToRefresh::new(Text::new("feed"))),
        ),
        (
            "Resize Handle",
            Box::new(ResizeHandle::horizontal().label("Sidebar")),
        ),
        (
            "Scroll Indicator",
            Box::new(
                ScrollIndicator::vertical()
                    .scroll(0.35, 0.25)
                    .always_visible(true),
            ),
        ),
        (
            "Scroll View",
            Box::new(ScrollView::new(
                Flex::column()
                    .gap(4.0)
                    .child(Text::new("row 1"))
                    .child(Text::new("row 2"))
                    .child(Text::new("row 3"))
                    .child(Text::new("row 4"))
                    .child(Text::new("row 5")),
            )),
        ),
        ("Separator", Box::new(Separator::horizontal())),
        (
            "Settings Row",
            Box::new(SettingsRow::new("Wi-Fi").subtitle("Connected").icon("📶")),
        ),
        (
            "Split View",
            Box::new(
                SplitView::horizontal()
                    .first(Text::new("Sidebar"))
                    .second(Text::new("Content"))
                    .ratio(0.35),
            ),
        ),
        (
            "Stack",
            Box::new(
                Stack::new()
                    .alignment(StackAlignment::Center)
                    .child(Text::new("Layer 1")),
            ),
        ),
        (
            "Steps",
            Box::new(
                Steps::new()
                    .steps(["Account", "Profile", "Done"])
                    .current(1),
            ),
        ),
        (
            "Tabs",
            Box::new(
                Tabs::new()
                    .tab("General", Text::new("general panel"))
                    .tab("Advanced", Text::new("advanced panel"))
                    .closable(true),
            ),
        ),
        (
            "Task Switcher",
            Box::new(
                TaskSwitcher::new()
                    .item(Thumbnail::new("editor", [80, 120, 200, 255]))
                    .item(Thumbnail::new("terminal", [60, 60, 70, 255]))
                    .index(1),
            ),
        ),
        (
            "Viewport",
            Box::new(Viewport::new().child(Text::new("canvas")).zoom(1.25)),
        ),
        (
            "Wizard",
            Box::new(
                Wizard::new()
                    .step("Account", Text::new("page 1"))
                    .step("Confirm", Text::new("page 2"))
                    .cancelable(true),
            ),
        ),
    ]
}
