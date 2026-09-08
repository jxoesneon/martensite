//! Project Fluent bundle integration, message resolution, and locale negotiation.
//!
//! [`FluentCatalog`][crate::fluent::FluentCatalog] owns a set of thread-safe
//! [`FluentBundle`][crate::fluent::FluentBundle]s keyed by
//! [`LanguageIdentifier`][crate::LanguageIdentifier] and provides message
//! resolution with arguments, locale negotiation via `fluent-langneg`, and
//! tracks the current locale and its
//! [`ScriptDirection`][crate::direction::ScriptDirection].
//!
//! The bundles use the concurrent `IntlLangMemoizer` (backed by
//! [`std::sync::Mutex`]) so that the catalog is `Send + Sync` and can be shared
//! across the reactive signal graph without `unsafe` code.

use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;

use fluent_bundle::concurrent::FluentBundle as ConcurrentFluentBundle;
use fluent_bundle::{FluentArgs, FluentError, FluentResource, FluentValue};
use fluent_langneg::{convert_vec_str_to_langids_lossy, negotiate_languages, NegotiationStrategy};

use crate::direction::{direction_for_locale, ScriptDirection};
use crate::LanguageIdentifier;

/// Thread-safe Fluent bundle specialization used by the catalog.
///
/// This is `fluent_bundle::concurrent::FluentBundle<FluentResource>`, which
/// uses a `Mutex`-backed memoizer so that bundles (and therefore
/// [`FluentCatalog`]) are `Send + Sync`.
pub type FluentBundle = ConcurrentFluentBundle<FluentResource>;

/// Errors raised while constructing localization bundles or switching locales.
///
/// The [`L10nError::Fluent`] variant wraps the accumulated
/// [`FluentError`] list so callers can inspect the
/// individual parse or overlay failures via pattern matching.
///
/// # Examples
///
/// ```
/// use martensite_l10n::fluent::{FluentCatalog, L10nError};
/// use martensite_l10n::LanguageIdentifier;
///
/// let mut catalog = FluentCatalog::new("en".parse().unwrap());
/// // A malformed FTL resource produces a fluent parse error.
/// let result = catalog.add_bundle(
///     "en".parse().unwrap(),
///     vec!["= missing identifier".to_string()],
/// );
/// // The inner `Vec<FluentError>` is accessible via pattern matching.
/// match result {
///     Err(L10nError::Fluent(errors)) => assert!(!errors.is_empty()),
///     other => panic!("expected a Fluent error, got {other:?}"),
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum L10nError {
    /// One or more Fluent parse or resource-overlay errors were encountered
    /// while adding resources to a bundle.
    ///
    /// The wrapped [`Vec<FluentError>`] is
    /// accessible via pattern matching so callers can react to individual
    /// failures (e.g. distinguish parser errors from overlay errors).
    Fluent(Vec<FluentError>),
    /// The caller attempted to activate the undefined (`und`) locale, which
    /// has no meaningful translation direction.
    UndefinedLocale,
}

impl fmt::Display for L10nError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fluent(errors) => {
                f.write_str("fluent bundle errors: ")?;
                let mut first = true;
                for err in errors {
                    if !first {
                        f.write_str(", ")?;
                    }
                    first = false;
                    write!(f, "{err}")?;
                }
                Ok(())
            }
            Self::UndefinedLocale => f.write_str("undefined locale `und` cannot be activated"),
        }
    }
}

impl std::error::Error for L10nError {}

/// A catalog of Project Fluent translation bundles keyed by locale.
///
/// The catalog resolves localized strings for the currently active locale,
/// falling back to negotiated available locales when the exact locale has no
/// bundle registered. It also tracks the active [`ScriptDirection`] so the
/// layout engine can mirror the widget tree on locale switches without
/// rebuilding structure.
///
/// # Examples
///
/// ```
/// use martensite_l10n::fluent::FluentCatalog;
/// use martensite_l10n::LanguageIdentifier;
///
/// let mut catalog = FluentCatalog::new("en".parse().unwrap());
/// catalog.add_bundle(
///     "en".parse().unwrap(),
///     vec!["greeting = Hello, world!".to_string()],
/// ).unwrap();
/// catalog.add_bundle(
///     "es".parse().unwrap(),
///     vec!["greeting = ¡Hola, mundo!".to_string()],
/// ).unwrap();
///
/// assert_eq!(catalog.get("greeting"), Some("Hello, world!".to_string()));
///
/// catalog.set_locale("es".parse().unwrap()).unwrap();
/// assert_eq!(catalog.get("greeting"), Some("¡Hola, mundo!".to_string()));
/// ```
pub struct FluentCatalog {
    /// Registered bundles keyed by their primary locale.
    bundles: HashMap<LanguageIdentifier, FluentBundle>,
    /// The anchor locale used as the negotiation fallback.
    default_locale: LanguageIdentifier,
    /// The currently active locale.
    current_locale: LanguageIdentifier,
    /// Direction of the currently active locale.
    direction: ScriptDirection,
}

impl FluentCatalog {
    /// Creates a new, empty catalog anchored to `default_locale`.
    ///
    /// The default locale is used as the negotiation fallback and becomes the
    /// active locale until [`FluentCatalog::set_locale`] is called. The
    /// initial [`ScriptDirection`] is resolved from `default_locale`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_l10n::fluent::FluentCatalog;
    /// use martensite_l10n::direction::ScriptDirection;
    /// use martensite_l10n::LanguageIdentifier;
    ///
    /// let catalog = FluentCatalog::new("ar".parse().unwrap());
    /// assert_eq!(catalog.locale(), &"ar".parse::<LanguageIdentifier>().unwrap());
    /// assert_eq!(catalog.direction(), ScriptDirection::Rtl);
    /// assert!(catalog.available_locales().is_empty());
    /// ```
    pub fn new(default_locale: LanguageIdentifier) -> Self {
        let direction = direction_for_locale(&default_locale);
        Self {
            bundles: HashMap::new(),
            default_locale: default_locale.clone(),
            current_locale: default_locale,
            direction,
        }
    }

    /// Registers a Fluent bundle for `locale` built from the supplied FTL
    /// resource strings.
    ///
    /// All resources are parsed and added atomically: if any resource fails to
    /// parse or overlays an existing entry, no bundle is inserted and the
    /// accumulated errors are returned.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_l10n::fluent::FluentCatalog;
    /// use martensite_l10n::LanguageIdentifier;
    ///
    /// let mut catalog = FluentCatalog::new("en".parse().unwrap());
    /// catalog.add_bundle(
    ///     "en".parse().unwrap(),
    ///     vec![
    ///         "hello = Hello!".to_string(),
    ///         "bye = Goodbye!".to_string(),
    ///     ],
    /// ).unwrap();
    ///
    /// assert_eq!(catalog.get("hello"), Some("Hello!".to_string()));
    /// assert_eq!(catalog.get("bye"), Some("Goodbye!".to_string()));
    /// ```
    pub fn add_bundle(
        &mut self,
        locale: LanguageIdentifier,
        resources: Vec<String>,
    ) -> Result<(), L10nError> {
        let mut bundle = FluentBundle::new_concurrent(vec![locale.clone()]);
        // Martensite resolves BiDi itself via `ScriptDirection`, so disable
        // Fluent's FSI/PDI isolating marks to keep resolved strings clean.
        bundle.set_use_isolating(false);
        let mut errors: Vec<FluentError> = Vec::new();

        for source in resources {
            match FluentResource::try_new(source) {
                Ok(resource) => {
                    if let Err(errs) = bundle.add_resource(resource) {
                        errors.extend(errs);
                    }
                }
                Err((_resource, parse_errors)) => {
                    errors.extend(parse_errors.into_iter().map(FluentError::ParserError));
                }
            }
        }

        if !errors.is_empty() {
            return Err(L10nError::Fluent(errors));
        }

        self.bundles.insert(locale, bundle);
        Ok(())
    }

    /// Activates `locale`, updating the current locale and its
    /// [`ScriptDirection`].
    ///
    /// Returns [`L10nError::UndefinedLocale`] if `locale` is the undefined
    /// (`und`) identifier. The active locale need not have a registered bundle:
    /// if no bundle is registered for `locale`, subsequent calls to
    /// [`FluentCatalog::get`] / [`FluentCatalog::get_with_args`] fall back via
    /// locale negotiation (see [`FluentCatalog::negotiate`]) to the best
    /// available bundle, ultimately anchoring on the default locale.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_l10n::fluent::FluentCatalog;
    /// use martensite_l10n::direction::ScriptDirection;
    /// use martensite_l10n::LanguageIdentifier;
    ///
    /// let mut catalog = FluentCatalog::new("en".parse().unwrap());
    /// catalog.add_bundle(
    ///     "ar".parse().unwrap(),
    ///     vec!["hi = مرحبا".to_string()],
    /// ).unwrap();
    ///
    /// catalog.set_locale("ar".parse().unwrap()).unwrap();
    /// assert_eq!(catalog.direction(), ScriptDirection::Rtl);
    /// assert_eq!(catalog.get("hi"), Some("مرحبا".to_string()));
    /// ```
    pub fn set_locale(&mut self, locale: LanguageIdentifier) -> Result<(), L10nError> {
        if locale == LanguageIdentifier::default() {
            return Err(L10nError::UndefinedLocale);
        }
        self.direction = direction_for_locale(&locale);
        self.current_locale = locale;
        Ok(())
    }

    /// Returns the currently active locale.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_l10n::fluent::FluentCatalog;
    /// use martensite_l10n::LanguageIdentifier;
    ///
    /// let catalog = FluentCatalog::new("en-US".parse().unwrap());
    /// assert_eq!(
    ///     catalog.locale(),
    ///     &"en-US".parse::<LanguageIdentifier>().unwrap(),
    /// );
    /// ```
    pub fn locale(&self) -> &LanguageIdentifier {
        &self.current_locale
    }

    /// Returns the [`ScriptDirection`] of the currently active locale.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_l10n::fluent::FluentCatalog;
    /// use martensite_l10n::direction::ScriptDirection;
    ///
    /// let mut catalog = FluentCatalog::new("en".parse().unwrap());
    /// assert_eq!(catalog.direction(), ScriptDirection::Ltr);
    /// catalog.set_locale("he".parse().unwrap()).unwrap();
    /// assert_eq!(catalog.direction(), ScriptDirection::Rtl);
    /// ```
    pub fn direction(&self) -> ScriptDirection {
        self.direction
    }

    /// Resolves a localized message for the active locale with no arguments.
    ///
    /// Returns `None` if no bundle or message is available. Falls back to a
    /// negotiated available locale when the active locale has no registered
    /// bundle.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_l10n::fluent::FluentCatalog;
    ///
    /// let mut catalog = FluentCatalog::new("en".parse().unwrap());
    /// catalog.add_bundle(
    ///     "en".parse().unwrap(),
    ///     vec!["welcome = Welcome!".to_string()],
    /// ).unwrap();
    /// assert_eq!(catalog.get("welcome"), Some("Welcome!".to_string()));
    /// assert_eq!(catalog.get("missing"), None);
    /// ```
    pub fn get(&self, key: &str) -> Option<String> {
        self.get_with_args(key, &[])
    }

    /// Resolves a localized message for the active locale with named arguments.
    ///
    /// `args` is a slice of `(name, value)` string pairs. Each value is parsed
    /// as an `f64` first: values that parse successfully are passed to Fluent
    /// as `FluentValue::Number` (so plural selectors like
    /// `{ $n -> [one] ... *[other] ... }` pick the correct branch), while
    /// non-numeric values are passed as `FluentValue::String`. Falls back to a
    /// negotiated available locale when the active locale has no registered
    /// bundle. Returns `None` if no bundle or message is available.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_l10n::fluent::FluentCatalog;
    ///
    /// let mut catalog = FluentCatalog::new("en".parse().unwrap());
    /// catalog.add_bundle(
    ///     "en".parse().unwrap(),
    ///     vec!["greet = Hello, { $name }!".to_string()],
    /// ).unwrap();
    ///
    /// let value = catalog.get_with_args("greet", &[("name", "Ada")]);
    /// assert_eq!(value, Some("Hello, Ada!".to_string()));
    /// ```
    pub fn get_with_args(&self, key: &str, args: &[(&str, &str)]) -> Option<String> {
        let bundle = self.current_bundle()?;
        let message = bundle.get_message(key)?;
        let pattern = message.value()?;

        let mut errors = Vec::new();
        if args.is_empty() {
            Some(
                bundle
                    .format_pattern(pattern, None, &mut errors)
                    .into_owned(),
            )
        } else {
            let mut fluent_args = FluentArgs::with_capacity(args.len());
            for (name, value) in args {
                // Try to parse the argument as a number first so that Fluent
                // plural selectors (`{ $n -> [one] ... *[other] ... }`) receive
                // a `FluentValue::Number` and can pick the correct plural form.
                // Anything that fails to parse as an `f64` falls back to a
                // plain string argument.
                let fluent_value = match value.parse::<f64>() {
                    Ok(n) => FluentValue::from(n),
                    Err(_) => FluentValue::String((*value).to_string().into()),
                };
                fluent_args.set(*name, fluent_value);
            }
            Some(
                bundle
                    .format_pattern(pattern, Some(&fluent_args), &mut errors)
                    .into_owned(),
            )
        }
    }

    /// Returns the locales of all registered bundles, sorted lexicographically
    /// for deterministic ordering.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_l10n::fluent::FluentCatalog;
    /// use martensite_l10n::LanguageIdentifier;
    ///
    /// let mut catalog = FluentCatalog::new("en".parse().unwrap());
    /// catalog.add_bundle("en".parse().unwrap(), vec!["a = A".to_string()]).unwrap();
    /// catalog.add_bundle("es".parse().unwrap(), vec!["a = A".to_string()]).unwrap();
    ///
    /// let locales: Vec<String> = catalog
    ///     .available_locales()
    ///     .into_iter()
    ///     .map(|l| l.to_string())
    ///     .collect();
    /// assert_eq!(locales, vec!["en", "es"]);
    /// ```
    pub fn available_locales(&self) -> Vec<&LanguageIdentifier> {
        let mut locales: Vec<&LanguageIdentifier> = self.bundles.keys().collect();
        locales.sort();
        locales
    }

    /// Negotiates the best available locale for `requested` against the
    /// registered bundles.
    ///
    /// Uses `fluent-langneg` with the [`NegotiationStrategy::Filtering`] strategy
    /// and the current locale as the default fallback. Returns `None` if no
    /// bundles are registered.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_l10n::fluent::FluentCatalog;
    /// use martensite_l10n::LanguageIdentifier;
    ///
    /// let mut catalog = FluentCatalog::new("en".parse().unwrap());
    /// catalog.add_bundle("en".parse().unwrap(), vec!["a = A".to_string()]).unwrap();
    /// catalog.add_bundle("es".parse().unwrap(), vec!["a = A".to_string()]).unwrap();
    ///
    /// let requested: Vec<LanguageIdentifier> =
    ///     ["es-AR", "pt-BR"].iter().map(|s| s.parse().unwrap()).collect();
    /// let best = catalog.negotiate(&requested);
    /// assert_eq!(best, Some("es".parse().unwrap()));
    /// ```
    pub fn negotiate(&self, requested: &[LanguageIdentifier]) -> Option<LanguageIdentifier> {
        if self.bundles.is_empty() {
            return None;
        }

        let available: Vec<String> = self.bundles.keys().map(|l| l.to_string()).collect();
        let available_icu = convert_vec_str_to_langids_lossy(&available);

        let requested: Vec<String> = requested.iter().map(|l| l.to_string()).collect();
        let requested_icu = convert_vec_str_to_langids_lossy(&requested);

        let default_str = self.default_locale.to_string();
        let default_icu = convert_vec_str_to_langids_lossy(&[default_str]);
        let default_icu = default_icu.into_iter().next()?;

        let supported = negotiate_languages(
            &requested_icu,
            &available_icu,
            Some(&default_icu),
            NegotiationStrategy::Filtering,
        );

        // `negotiate_languages` appends the default locale to the result even
        // when it is not among the available bundles. Filter to locales that
        // are actually registered so callers always receive a usable bundle
        // locale (or `None`).
        supported
            .into_iter()
            .map(|langid| langid.to_string())
            .find_map(|s| {
                let parsed = LanguageIdentifier::from_str(&s).ok()?;
                if self.bundles.contains_key(&parsed) {
                    Some(parsed)
                } else {
                    None
                }
            })
    }

    /// Returns the bundle for the active locale, negotiating a fallback when
    /// the exact locale has no registered bundle.
    fn current_bundle(&self) -> Option<&FluentBundle> {
        if let Some(bundle) = self.bundles.get(&self.current_locale) {
            return Some(bundle);
        }
        let negotiated = self.negotiate(std::slice::from_ref(&self.current_locale))?;
        self.bundles.get(&negotiated)
    }
}

impl fmt::Debug for FluentCatalog {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FluentCatalog")
            .field("current_locale", &self.current_locale)
            .field("direction", &self.direction)
            .field("available_locales", &self.available_locales())
            .finish()
    }
}

impl Default for FluentCatalog {
    /// Defaults to an empty catalog anchored to the undefined (`und`) locale.
    fn default() -> Self {
        Self::new(LanguageIdentifier::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn en_resource() -> Vec<String> {
        vec![
            "hello = Hello, world!".to_string(),
            "greet = Hello, { $name }!".to_string(),
            "count = You have { $n } messages.".to_string(),
        ]
    }

    fn es_resource() -> Vec<String> {
        vec![
            "hello = ¡Hola, mundo!".to_string(),
            "greet = ¡Hola, { $name }!".to_string(),
        ]
    }

    #[test]
    fn new_catalog_has_no_bundles() {
        let catalog = FluentCatalog::new(LanguageIdentifier::from_str("en").unwrap());
        assert!(catalog.available_locales().is_empty());
        assert_eq!(catalog.direction(), ScriptDirection::Ltr);
        assert_eq!(
            catalog.locale(),
            &LanguageIdentifier::from_str("en").unwrap()
        );
    }

    #[test]
    fn add_and_resolve_simple_message() {
        let mut catalog = FluentCatalog::new(LanguageIdentifier::from_str("en").unwrap());
        catalog
            .add_bundle(LanguageIdentifier::from_str("en").unwrap(), en_resource())
            .unwrap();

        assert_eq!(catalog.get("hello"), Some("Hello, world!".to_string()));
    }

    #[test]
    fn resolve_message_with_args() {
        let mut catalog = FluentCatalog::new(LanguageIdentifier::from_str("en").unwrap());
        catalog
            .add_bundle(LanguageIdentifier::from_str("en").unwrap(), en_resource())
            .unwrap();

        assert_eq!(
            catalog.get_with_args("greet", &[("name", "Ada")]),
            Some("Hello, Ada!".to_string())
        );
        assert_eq!(
            catalog.get_with_args("count", &[("n", "3")]),
            Some("You have 3 messages.".to_string())
        );
    }

    #[test]
    fn get_returns_none_for_missing_key() {
        let mut catalog = FluentCatalog::new(LanguageIdentifier::from_str("en").unwrap());
        catalog
            .add_bundle(LanguageIdentifier::from_str("en").unwrap(), en_resource())
            .unwrap();

        assert_eq!(catalog.get("does-not-exist"), None);
    }

    #[test]
    fn get_returns_none_when_no_bundles() {
        let catalog = FluentCatalog::new(LanguageIdentifier::from_str("en").unwrap());
        assert_eq!(catalog.get("hello"), None);
        assert_eq!(catalog.get_with_args("hello", &[("name", "x")]), None);
    }

    #[test]
    fn set_locale_switches_active_bundle() {
        let mut catalog = FluentCatalog::new(LanguageIdentifier::from_str("en").unwrap());
        catalog
            .add_bundle(LanguageIdentifier::from_str("en").unwrap(), en_resource())
            .unwrap();
        catalog
            .add_bundle(LanguageIdentifier::from_str("es").unwrap(), es_resource())
            .unwrap();

        assert_eq!(catalog.get("hello"), Some("Hello, world!".to_string()));

        catalog
            .set_locale(LanguageIdentifier::from_str("es").unwrap())
            .unwrap();
        assert_eq!(catalog.get("hello"), Some("¡Hola, mundo!".to_string()));
        assert_eq!(
            catalog.get_with_args("greet", &[("name", "Ada")]),
            Some("¡Hola, Ada!".to_string())
        );
    }

    #[test]
    fn set_locale_updates_direction() {
        let mut catalog = FluentCatalog::new(LanguageIdentifier::from_str("en").unwrap());
        assert_eq!(catalog.direction(), ScriptDirection::Ltr);

        catalog
            .set_locale(LanguageIdentifier::from_str("ar").unwrap())
            .unwrap();
        assert_eq!(catalog.direction(), ScriptDirection::Rtl);

        catalog
            .set_locale(LanguageIdentifier::from_str("en-US").unwrap())
            .unwrap();
        assert_eq!(catalog.direction(), ScriptDirection::Ltr);
    }

    #[test]
    fn set_locale_rejects_undefined_locale() {
        let mut catalog = FluentCatalog::new(LanguageIdentifier::from_str("en").unwrap());
        let result = catalog.set_locale(LanguageIdentifier::default());
        assert_eq!(result, Err(L10nError::UndefinedLocale));
    }

    #[test]
    fn available_locales_sorted_deterministically() {
        let mut catalog = FluentCatalog::new(LanguageIdentifier::from_str("en").unwrap());
        catalog
            .add_bundle(
                LanguageIdentifier::from_str("es").unwrap(),
                vec!["a = A".to_string()],
            )
            .unwrap();
        catalog
            .add_bundle(
                LanguageIdentifier::from_str("en").unwrap(),
                vec!["a = A".to_string()],
            )
            .unwrap();
        catalog
            .add_bundle(
                LanguageIdentifier::from_str("de").unwrap(),
                vec!["a = A".to_string()],
            )
            .unwrap();

        let locales: Vec<String> = catalog
            .available_locales()
            .into_iter()
            .map(|l| l.to_string())
            .collect();
        assert_eq!(locales, vec!["de", "en", "es"]);
    }

    #[test]
    fn negotiate_picks_exact_match() {
        let mut catalog = FluentCatalog::new(LanguageIdentifier::from_str("en").unwrap());
        catalog
            .add_bundle(
                LanguageIdentifier::from_str("en").unwrap(),
                vec!["a = A".to_string()],
            )
            .unwrap();
        catalog
            .add_bundle(
                LanguageIdentifier::from_str("es").unwrap(),
                vec!["a = A".to_string()],
            )
            .unwrap();

        let requested = vec![
            LanguageIdentifier::from_str("es").unwrap(),
            LanguageIdentifier::from_str("en").unwrap(),
        ];
        assert_eq!(
            catalog.negotiate(&requested),
            Some(LanguageIdentifier::from_str("es").unwrap())
        );
    }

    #[test]
    fn negotiate_falls_back_to_regional_match() {
        let mut catalog = FluentCatalog::new(LanguageIdentifier::from_str("en").unwrap());
        catalog
            .add_bundle(
                LanguageIdentifier::from_str("en").unwrap(),
                vec!["a = A".to_string()],
            )
            .unwrap();
        catalog
            .add_bundle(
                LanguageIdentifier::from_str("es").unwrap(),
                vec!["a = A".to_string()],
            )
            .unwrap();

        let requested = vec![LanguageIdentifier::from_str("es-AR").unwrap()];
        assert_eq!(
            catalog.negotiate(&requested),
            Some(LanguageIdentifier::from_str("es").unwrap())
        );
    }

    #[test]
    fn negotiate_falls_back_to_default_when_no_match() {
        let mut catalog = FluentCatalog::new(LanguageIdentifier::from_str("en").unwrap());
        catalog
            .add_bundle(
                LanguageIdentifier::from_str("en").unwrap(),
                vec!["a = A".to_string()],
            )
            .unwrap();

        let requested = vec![LanguageIdentifier::from_str("zh").unwrap()];
        assert_eq!(
            catalog.negotiate(&requested),
            Some(LanguageIdentifier::from_str("en").unwrap())
        );
    }

    #[test]
    fn negotiate_returns_none_with_no_bundles() {
        let catalog = FluentCatalog::new(LanguageIdentifier::from_str("en").unwrap());
        let requested = vec![LanguageIdentifier::from_str("en").unwrap()];
        assert_eq!(catalog.negotiate(&requested), None);
    }

    #[test]
    fn current_bundle_falls_back_via_negotiation() {
        let mut catalog = FluentCatalog::new(LanguageIdentifier::from_str("en").unwrap());
        catalog
            .add_bundle(LanguageIdentifier::from_str("en").unwrap(), en_resource())
            .unwrap();

        // es-AR has no bundle; resolution should fall back to en.
        catalog
            .set_locale(LanguageIdentifier::from_str("es-AR").unwrap())
            .unwrap();
        assert_eq!(catalog.get("hello"), Some("Hello, world!".to_string()));
    }

    #[test]
    fn malformed_resource_returns_fluent_error() {
        let mut catalog = FluentCatalog::new(LanguageIdentifier::from_str("en").unwrap());
        let result = catalog.add_bundle(
            LanguageIdentifier::from_str("en").unwrap(),
            vec!["= no key here".to_string()],
        );
        assert!(matches!(result, Err(L10nError::Fluent(_))));
        // The failed bundle must not have been registered.
        assert!(catalog.available_locales().is_empty());
    }

    #[test]
    fn multiple_resources_in_one_bundle() {
        let mut catalog = FluentCatalog::new(LanguageIdentifier::from_str("en").unwrap());
        catalog
            .add_bundle(
                LanguageIdentifier::from_str("en").unwrap(),
                vec!["a = Alpha".to_string(), "b = Beta".to_string()],
            )
            .unwrap();
        assert_eq!(catalog.get("a"), Some("Alpha".to_string()));
        assert_eq!(catalog.get("b"), Some("Beta".to_string()));
    }

    #[test]
    fn overlapping_messages_report_overlay_error() {
        let mut catalog = FluentCatalog::new(LanguageIdentifier::from_str("en").unwrap());
        let result = catalog.add_bundle(
            LanguageIdentifier::from_str("en").unwrap(),
            vec!["dup = First".to_string(), "dup = Second".to_string()],
        );
        assert!(matches!(result, Err(L10nError::Fluent(_))));
    }

    #[test]
    fn empty_resource_list_yields_empty_bundle() {
        let mut catalog = FluentCatalog::new(LanguageIdentifier::from_str("en").unwrap());
        catalog
            .add_bundle(LanguageIdentifier::from_str("en").unwrap(), Vec::new())
            .unwrap();
        assert_eq!(catalog.available_locales().len(), 1);
        assert_eq!(catalog.get("anything"), None);
    }

    #[test]
    fn default_catalog_is_und_and_ltr() {
        let catalog = FluentCatalog::default();
        assert_eq!(catalog.locale(), &LanguageIdentifier::default());
        assert_eq!(catalog.direction(), ScriptDirection::Ltr);
    }

    #[test]
    fn debug_repr_compiles_and_contains_locale() {
        let catalog = FluentCatalog::new(LanguageIdentifier::from_str("en").unwrap());
        let s = format!("{catalog:?}");
        assert!(s.contains("FluentCatalog"));
        assert!(s.contains("en"));
    }

    #[test]
    fn l10n_error_display_is_informative() {
        let err = L10nError::UndefinedLocale;
        assert!(err.to_string().contains("und"));

        // Trigger a real FluentError::Overriding by adding two resources with
        // the same message id to a single bundle.
        let mut catalog = FluentCatalog::new(LanguageIdentifier::from_str("en").unwrap());
        let result = catalog.add_bundle(
            LanguageIdentifier::from_str("en").unwrap(),
            vec!["dup = First".to_string(), "dup = Second".to_string()],
        );
        match result {
            Err(L10nError::Fluent(errors)) => {
                let s = L10nError::Fluent(errors).to_string();
                assert!(s.contains("fluent bundle errors"));
            }
            other => panic!("expected Fluent overlay error, got {other:?}"),
        }
    }

    #[test]
    fn selectors_resolve_via_fluent_engine() {
        let mut catalog = FluentCatalog::new(LanguageIdentifier::from_str("en").unwrap());
        catalog
            .add_bundle(
                LanguageIdentifier::from_str("en").unwrap(),
                vec![
                    "emails = { $n ->\n    [one] You have one message.\n   *[other] You have { $n } messages.\n}"
                        .to_string(),
                ],
            )
            .unwrap();

        // Numeric args are parsed into `FluentValue::Number`, so the plural
        // selector picks the matching branch instead of always falling back to
        // the `*other` default.
        let one = catalog.get_with_args("emails", &[("n", "1")]);
        assert_eq!(
            one.as_deref(),
            Some("You have one message."),
            "n=1 must select the [one] plural branch"
        );

        let many = catalog.get_with_args("emails", &[("n", "5")]);
        assert_eq!(
            many.as_deref(),
            Some("You have 5 messages."),
            "n=5 must select the *[other] plural branch"
        );
    }

    #[test]
    fn plural_selection_works_with_numeric_args() {
        let mut catalog = FluentCatalog::new(LanguageIdentifier::from_str("en").unwrap());
        catalog
            .add_bundle(
                LanguageIdentifier::from_str("en").unwrap(),
                vec![
                    "apples = { $count ->\n    [one] You have one apple.\n   *[other] You have { $count } apples.\n}"
                        .to_string(),
                ],
            )
            .unwrap();

        // The singular `[one]` branch is only reachable when the argument is
        // supplied as a number; a string argument would always hit `*other`.
        assert_eq!(
            catalog.get_with_args("apples", &[("count", "1")]),
            Some("You have one apple.".to_string())
        );
        assert_eq!(
            catalog.get_with_args("apples", &[("count", "2")]),
            Some("You have 2 apples.".to_string())
        );
        assert_eq!(
            catalog.get_with_args("apples", &[("count", "0")]),
            Some("You have 0 apples.".to_string())
        );
    }

    #[test]
    fn non_numeric_args_still_format_as_strings() {
        let mut catalog = FluentCatalog::new(LanguageIdentifier::from_str("en").unwrap());
        catalog
            .add_bundle(
                LanguageIdentifier::from_str("en").unwrap(),
                vec!["greet = Hello, { $name }!".to_string()],
            )
            .unwrap();

        // A non-numeric value must not be misinterpreted as a number.
        assert_eq!(
            catalog.get_with_args("greet", &[("name", "Ada Lovelace")]),
            Some("Hello, Ada Lovelace!".to_string())
        );
    }

    #[test]
    fn fluent_catalog_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<FluentCatalog>();
    }
}
