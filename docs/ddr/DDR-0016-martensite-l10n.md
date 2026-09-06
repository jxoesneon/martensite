# Detailed Design Record: DDR-0016
## Title: `martensite-l10n` Fluent Integration & Global Typography

### 1. Architectural Role & Invariants
`martensite-l10n` integrates Project Fluent (`fluent-rs`), Unicode Bidi, and CLDR rules.
* **Invariant 1.1**: All user-visible strings must be accessed via strongly-typed keys into Fluent bundles. Hardcoded string literals in UI are an anti-pattern.
* **Invariant 1.2**: Locale is an environment-level Reactive Signal (`Signal<Locale>`). When changed, UI strictly re-evaluates dirty nodes.
* **Invariant 1.3**: BiDi directionality translates automatically to Taffy's logical properties (`Start`/`End` vs `Left`/`Right`).

### 2. Fluent Bundle Loading & Reactive Signal
```rust
use fluent::{FluentBundle, FluentResource};
use unic_langid::LanguageIdentifier;

pub struct L10nEnv {
    current_locale: Signal<LanguageIdentifier>,
    bundles: HashMap<LanguageIdentifier, FluentBundle<FluentResource>>,
}

impl L10nEnv {
    pub fn format(&self, key: &str, args: Option<&fluent::FluentArgs>) -> String {
        let locale = self.current_locale.get();
        let bundle = self.bundles.get(&locale).unwrap();
        let msg = bundle.get_message(key).unwrap();
        let pattern = msg.value().unwrap();
        let mut errors = vec![];
        bundle.format_pattern(pattern, args, &mut errors).into_owned()
    }
}
```

### 3. BiDi Mirroring via Taffy
If `Locale` implies Right-To-Left (e.g., Arabic, Hebrew), the root container's Taffy `Style` is mutated:
```rust
style.direction = taffy::style::Direction::RTL;
```
Padding and margins authored as `padding_start` automatically map to right-padding.

### 4. CLDR Plural Rules
Rely on `fluent`'s internal CLDR data to resolve `$count` arguments for pluralization (`zero`, `one`, `two`, `few`, `many`, `other`).

### 5. Error Conditions
- **Missing Keys**: Returns a fallback string formatted as `"!{key}!"` and logs a developer warning. Never crashes the UI.
