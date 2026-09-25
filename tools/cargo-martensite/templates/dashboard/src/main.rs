//! {{project_name}} — Martensite industrial dashboard skeleton.

use martensite::blessed::docking::{DockPanel, DockTree};
use martensite::prelude::*;

/// Primary dashboard state containing dock layout and zone data.
pub struct DashboardApp {
    /// Docking layout tree for multi-panel arrangement.
    pub dock: DockTree,
    /// Instrument telemetry reading signal.
    pub reading: Signal<f64>,
}

impl Default for DashboardApp {
    fn default() -> Self {
        Self::new()
    }
}

impl DashboardApp {
    /// Creates a new dashboard skeleton with a single instrument zone.
    pub fn new() -> Self {
        let mut dock = DockTree::new();
        let _root = dock.insert_root(DockPanel::new(1, "Telemetry Zone"));

        Self {
            dock,
            reading: create_signal(42.0),
        }
    }

    /// Updates the instrument telemetry reading.
    pub fn set_reading(&self, val: f64) {
        self.reading.set(val);
    }

    /// Builds the primary zone widget hierarchy.
    pub fn build_zone_view(&self) -> Flex {
        let mut root = Flex::column();
        root.add_child(Box::new(
            Text::new("Zone 1: Primary Telemetry").font_size(20.0),
        ));
        root.add_child(Box::new(
            Text::new(format!("Gauge: {:.1} units", self.reading.get())).font_size(16.0),
        ));
        root
    }
}

fn main() {
    let _app = App::build().build();
    let dashboard = DashboardApp::new();
    println!(
        "{{project_name}} dashboard initialized. Active panels: {}, initial reading: {:.1}",
        dashboard.dock.panel_count(),
        dashboard.reading.get()
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoke_test_dashboard() {
        let dashboard = DashboardApp::new();
        assert_eq!(dashboard.reading.get(), 42.0);
        dashboard.set_reading(100.5);
        assert_eq!(dashboard.reading.get(), 100.5);
        assert_eq!(dashboard.dock.panel_count(), 1);
        let _zone = dashboard.build_zone_view();
    }
}
