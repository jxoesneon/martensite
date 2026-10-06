//! Catalog page model: metadata, prop specs, and the [`Page`] trait
//! every showcased widget implements.
//!
//! A page owns three things: the [`PageMeta`] reference data shown
//! beside the stage (description, AccessKit role, cross-framework
//! aliases), a declarative [`PropSpec`] list that drives the generated
//! props panel, and two functions — [`Page::build`] for the live
//! widget on stage and [`Page::snippet`] for the Rust expression
//! producing exactly that widget — so the snippet always matches what
//! is on screen.

use std::collections::BTreeMap;

use martensite::core::Widget;

/// Reference metadata shown beside the stage.
#[derive(Clone, Debug)]
pub struct PageMeta {
    /// Canonical widget name (`"Button"`).
    pub name: &'static str,
    /// Family label used for rail grouping and search (`"Controls"`).
    pub family: &'static str,
    /// What the widget is for, in one or two sentences.
    pub description: &'static str,
    /// AccessKit role name as a string (`"Button"`, `"Slider"`).
    pub role: &'static str,
    /// Cross-framework equivalents: `(framework, widget)` pairs.
    pub aliases: &'static [(&'static str, &'static str)],
    /// True when the demo opens real popups through the overlay layer.
    pub needs_overlay: bool,
}

/// Declarative description of one prop control in the props panel.
#[derive(Clone, Debug)]
pub enum PropSpec {
    /// Boolean toggle (`Toggle`).
    Bool {
        /// Stable key used by [`PropValues`] and snippets.
        key: &'static str,
        /// Control label.
        label: &'static str,
        /// Default value when a page is opened.
        default: bool,
    },
    /// Continuous scalar (`Slider`).
    Float {
        /// Stable key.
        key: &'static str,
        /// Control label.
        label: &'static str,
        /// Inclusive minimum.
        min: f64,
        /// Inclusive maximum.
        max: f64,
        /// Step quantum.
        step: f64,
        /// Default value.
        default: f64,
    },
    /// Integer (`Stepper`).
    Int {
        /// Stable key.
        key: &'static str,
        /// Control label.
        label: &'static str,
        /// Inclusive minimum.
        min: i64,
        /// Inclusive maximum.
        max: i64,
        /// Default value.
        default: i64,
    },
    /// Free-form text (`TextInput`).
    Text {
        /// Stable key.
        key: &'static str,
        /// Control label.
        label: &'static str,
        /// Default text.
        default: &'static str,
    },
    /// Named variant (`Segmented` for ≤4 options, `Dropdown` above).
    Choice {
        /// Stable key.
        key: &'static str,
        /// Control label.
        label: &'static str,
        /// Option labels in order.
        options: &'static [&'static str],
        /// Default option index.
        default: usize,
    },
    /// Non-interactive section heading — groups the interactive
    /// specs that follow it (e.g. `"State & Accessibility"`).
    Header {
        /// Heading text.
        label: &'static str,
    },
    /// Audit annotation, not a panel control: a semantically-valid
    /// value for the prop `key` that the shape-generic prop audit
    /// can't express (ISO dates, `"r,g,b,a"` colors, csv rows, icon
    /// names, in-range indices). Place it immediately after the spec
    /// it annotates. `--audit-props`/`--audit-gate` parse `value`
    /// through that spec's [`PropSpec::parse_value`] and require the
    /// render to change — so a wired prop proves itself rather than
    /// living in the baseline file.
    Probe {
        /// Key of the prop spec this value targets.
        key: &'static str,
        /// A valid, non-default value in that prop's input syntax.
        value: &'static str,
    },
}

impl PropSpec {
    /// The spec's stable key — `""` for headers and probes, which
    /// carry no value of their own.
    pub fn key(&self) -> &'static str {
        match self {
            Self::Header { .. } | Self::Probe { .. } => "",
            Self::Bool { key, .. }
            | Self::Float { key, .. }
            | Self::Int { key, .. }
            | Self::Text { key, .. }
            | Self::Choice { key, .. } => key,
        }
    }

    /// The spec's display label.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Header { label } => label,
            Self::Probe { .. } => "Audit Probe",
            Self::Bool { label, .. }
            | Self::Float { label, .. }
            | Self::Int { label, .. }
            | Self::Text { label, .. }
            | Self::Choice { label, .. } => label,
        }
    }

    /// Parses a text value into the spec's [`PropValue`] type — the
    /// same coercion the props panel applies to dev-channel `prop`
    /// signals. `Choice` accepts an index or an option label. Returns
    /// `None` for unparseable values and headers.
    pub fn parse_value(&self, value: &str) -> Option<PropValue> {
        match self {
            Self::Bool { .. } => value.parse::<bool>().ok().map(PropValue::Bool),
            Self::Float { .. } => value.parse::<f64>().ok().map(PropValue::Float),
            Self::Int { .. } => value.parse::<i64>().ok().map(PropValue::Int),
            Self::Text { .. } => Some(PropValue::Text(value.to_string())),
            Self::Choice { options, .. } => value
                .parse::<usize>()
                .ok()
                .or_else(|| options.iter().position(|o| *o == value))
                .map(PropValue::Choice),
            Self::Header { .. } | Self::Probe { .. } => None,
        }
    }

    /// The spec's default value — headers and probes carry
    /// `Bool(false)` as a placeholder and are skipped by
    /// [`PropValues::from_specs`].
    pub fn default_value(&self) -> PropValue {
        match *self {
            Self::Header { .. } | Self::Probe { .. } => PropValue::Bool(false),
            Self::Bool { default, .. } => PropValue::Bool(default),
            Self::Float { default, .. } => PropValue::Float(default),
            Self::Int { default, .. } => PropValue::Int(default),
            Self::Text { default, .. } => PropValue::Text(default.to_string()),
            Self::Choice { default, .. } => PropValue::Choice(default),
        }
    }
}

/// One prop's current value.
#[derive(Clone, Debug, PartialEq)]
pub enum PropValue {
    /// Boolean.
    Bool(bool),
    /// Continuous scalar.
    Float(f64),
    /// Integer.
    Int(i64),
    /// Free-form text.
    Text(String),
    /// Named variant index.
    Choice(usize),
}

/// Current values for a page's prop set, keyed by [`PropSpec::key`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PropValues {
    map: BTreeMap<&'static str, PropValue>,
}

impl PropValues {
    /// All-default values for `specs`.
    pub fn from_specs(specs: &[PropSpec]) -> Self {
        let mut map = BTreeMap::new();
        for spec in specs {
            if matches!(spec, PropSpec::Header { .. } | PropSpec::Probe { .. }) {
                continue;
            }
            map.insert(spec.key(), spec.default_value());
        }
        Self { map }
    }

    /// Current value for `key` — `None` for unknown keys.
    pub fn get(&self, key: &str) -> Option<&PropValue> {
        self.map.get(key)
    }

    /// Sets `key` to `value` (replacing any previous value).
    pub fn set(&mut self, key: &'static str, value: PropValue) {
        self.map.insert(key, value);
    }

    /// `true` when any value differs from its spec default — used to
    /// render a "reset props" affordance.
    pub fn is_defaulted(&self, specs: &[PropSpec]) -> bool {
        specs.iter().all(|s| {
            self.map
                .get(s.key())
                .is_some_and(|v| *v == s.default_value())
        })
    }

    /// Bool accessor — panics on a spec mismatch, which is a page bug,
    /// not a user error.
    pub fn bool(&self, key: &str) -> bool {
        match self.get(key) {
            Some(PropValue::Bool(v)) => *v,
            other => panic!("prop {key:?}: expected Bool, got {other:?}"),
        }
    }

    /// Float accessor; also accepts `Int` for convenience.
    pub fn f64(&self, key: &str) -> f64 {
        match self.get(key) {
            Some(PropValue::Float(v)) => *v,
            Some(PropValue::Int(v)) => *v as f64,
            other => panic!("prop {key:?}: expected Float, got {other:?}"),
        }
    }

    /// Int accessor.
    pub fn i64(&self, key: &str) -> i64 {
        match self.get(key) {
            Some(PropValue::Int(v)) => *v,
            other => panic!("prop {key:?}: expected Int, got {other:?}"),
        }
    }

    /// Text accessor.
    pub fn str(&self, key: &str) -> &str {
        match self.get(key) {
            Some(PropValue::Text(v)) => v,
            other => panic!("prop {key:?}: expected Text, got {other:?}"),
        }
    }

    /// Choice accessor — returns the selected option index.
    pub fn choice(&self, key: &str) -> usize {
        match self.get(key) {
            Some(PropValue::Choice(v)) => *v,
            other => panic!("prop {key:?}: expected Choice, got {other:?}"),
        }
    }
}

/// Rust expression fragment used by the live snippet: the prop
/// formatting helpers make page snippets read like builder calls.
pub fn fmt_lit(v: &PropValue) -> String {
    match v {
        PropValue::Bool(b) => b.to_string(),
        PropValue::Float(f) => format!("{f:?}"),
        PropValue::Int(i) => i.to_string(),
        PropValue::Text(t) => format!("{t:?}"),
        PropValue::Choice(_) => String::new(),
    }
}

/// One showcaseable widget: its reference metadata, declarative props,
/// live builder, snippet generator, and per-frame event drain.
pub trait Page: Send + Sync + 'static {
    /// Reference metadata shown beside the stage.
    fn meta(&self) -> PageMeta;
    /// The page's prop controls, in panel order. Pages without
    /// meaningful props return `&[]`.
    fn props(&self) -> &'static [PropSpec] {
        &[]
    }
    /// Builds the stage content for `props` — rebuilt on every change.
    fn build(&self, props: &PropValues) -> Box<dyn Widget>;
    /// Rust snippet constructing exactly what [`Page::build`] returns
    /// for `props` — the copyable "how do I write this" answer.
    fn snippet(&self, props: &PropValues) -> String;
    /// Drains the staged widget's event channels into `out` as
    /// human-readable log lines — `take_*` queues, parked flags, etc.
    /// Called once per frame while the page is on stage.
    fn poll_events(&self, _widget: &mut dyn Widget, _out: &mut Vec<String>) {}

    /// The staged widget's observable state as `(key, value)` pairs —
    /// the event log diffs successive snapshots and logs changes.
    /// Called once per frame while the page is on stage.
    fn describe_state(&self, _widget: &mut dyn Widget) -> Vec<(String, String)> {
        Vec::new()
    }

    /// The snippet for default props — used by the compile test.
    fn default_snippet(&self) -> String {
        self.snippet(&PropValues::from_specs(self.props()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prop_values_defaults_and_accessors() {
        static SPECS: &[PropSpec] = &[
            PropSpec::Bool {
                key: "enabled",
                label: "Enabled",
                default: true,
            },
            PropSpec::Float {
                key: "alpha",
                label: "Alpha",
                min: 0.0,
                max: 1.0,
                step: 0.05,
                default: 0.5,
            },
            PropSpec::Int {
                key: "count",
                label: "Count",
                min: 0,
                max: 9,
                default: 3,
            },
            PropSpec::Text {
                key: "label",
                label: "Label",
                default: "Hi",
            },
            PropSpec::Choice {
                key: "variant",
                label: "Variant",
                options: &["A", "B"],
                default: 1,
            },
        ];
        let mut v = PropValues::from_specs(SPECS);
        assert!(v.is_defaulted(SPECS));
        assert!(v.bool("enabled"));
        assert_eq!(v.f64("alpha"), 0.5);
        assert_eq!(v.i64("count"), 3);
        assert_eq!(v.str("label"), "Hi");
        assert_eq!(v.choice("variant"), 1);
        v.set("enabled", PropValue::Bool(false));
        assert!(!v.bool("enabled"));
        assert!(!v.is_defaulted(SPECS));
    }
}
