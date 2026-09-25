//! {{project_name}} — Martensite application.

use martensite::prelude::*;

/// Top-level application state and UI definition for `{{project_name}}`.
pub struct MartensiteApp {
    /// Reactive counter signal.
    pub count: Signal<i32>,
}

impl Default for MartensiteApp {
    fn default() -> Self {
        Self::new()
    }
}

impl MartensiteApp {
    /// Creates a new application instance with initial reactive state.
    pub fn new() -> Self {
        Self {
            count: create_signal(0),
        }
    }

    /// Increments the counter.
    pub fn increment(&self) {
        self.count.set(self.count.get() + 1);
    }

    /// Decrements the counter.
    pub fn decrement(&self) {
        self.count.set(self.count.get() - 1);
    }

    /// Builds the primary widget hierarchy.
    pub fn view(&self) -> Flex {
        let mut root = Flex::column();
        root.add_child(Box::new(Text::new("{{project_name}}").font_size(24.0)));
        root.add_child(Box::new(
            Text::new(format!("Count: {}", self.count.get())).font_size(16.0),
        ));
        root.add_child(Box::new(Button::new("Increment")));
        root
    }
}

fn main() {
    let _config = App::build().build();
    let app = MartensiteApp::new();
    println!("{{project_name}} started; initial count: {}", app.count.get());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoke_test_counter() {
        let app = MartensiteApp::new();
        assert_eq!(app.count.get(), 0);
        app.increment();
        assert_eq!(app.count.get(), 1);
        app.decrement();
        assert_eq!(app.count.get(), 0);
        let _view = app.view();
    }
}
