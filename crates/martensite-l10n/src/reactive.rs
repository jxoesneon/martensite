//! Reactive locale signal integration binding Fluent resources into the signal graph.
//!
//! [`L10n`][crate::reactive::L10n] ties a [`FluentCatalog`][crate::fluent::FluentCatalog]
//! to a root `Signal` holding the active [`LanguageIdentifier`][crate::LanguageIdentifier].
//! A locale switch via [`L10n::set_locale`][crate::reactive::L10n::set_locale] updates the
//! catalog *and* the root locale signal, so only text leaf nodes bound to
//! localized strings (via `L10n::localized` / `L10n::localized_with_args`)
//! invalidate their dirty bitmasks. The structural layout hierarchy is never
//! disturbed.
//!
//! The catalog is shared through an `Arc<RwLock<FluentCatalog>>` so
//! that the `Memo` evaluation closures — which must be `Send + Sync` — can
//! resolve messages without capturing any non-thread-safe state. This is what
//! makes the concurrent Fluent bundle specialization (see
//! [`FluentBundle`][crate::fluent::FluentBundle]) essential.

use std::sync::{Arc, RwLock};

use martensite_reactive::{Memo, Signal};

use crate::direction::ScriptDirection;
use crate::fluent::{FluentCatalog, L10nError};
use crate::LanguageIdentifier;

/// Reactive localization controller binding a root locale signal to a
/// [`FluentCatalog`].
///
/// `L10n` is the integration seam between Project Fluent and the Martensite
/// reactive signal graph. Construct it inside a reactive runtime scope (or rely
/// on the ambient global runtime), register bundles, and then create localized
/// text memos that automatically invalidate when the locale changes.
///
/// # Examples
///
/// ```
/// use martensite_l10n::reactive::L10n;
/// use martensite_reactive::{create_effect, flush};
///
/// let l10n = L10n::new("en".parse().unwrap());
/// l10n.add_bundle(
///     "en".parse().unwrap(),
///     vec!["greeting = Hello!".to_string()],
/// ).unwrap();
/// l10n.add_bundle(
///     "es".parse().unwrap(),
///     vec!["greeting = ¡Hola!".to_string()],
/// ).unwrap();
///
/// let text = l10n.localized("greeting");
/// assert_eq!(text.get(), "Hello!");
///
/// l10n.set_locale("es".parse().unwrap()).unwrap();
/// flush();
/// assert_eq!(text.get(), "¡Hola!");
/// ```
pub struct L10n {
    /// Root locale signal; the single source of truth for the active locale.
    locale: Signal<LanguageIdentifier>,
    /// Shared, thread-safe catalog of Fluent bundles.
    catalog: Arc<RwLock<FluentCatalog>>,
}

impl L10n {
    /// Creates a new `L10n` anchored to `default_locale` and bound to the
    /// ambient reactive runtime.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_l10n::reactive::L10n;
    /// use martensite_l10n::direction::ScriptDirection;
    ///
    /// let l10n = L10n::new("ar".parse().unwrap());
    /// assert_eq!(l10n.direction(), ScriptDirection::Rtl);
    /// ```
    pub fn new(default_locale: LanguageIdentifier) -> Self {
        let catalog = FluentCatalog::new(default_locale.clone());
        Self {
            locale: Signal::new(default_locale),
            catalog: Arc::new(RwLock::new(catalog)),
        }
    }

    /// Registers a Fluent bundle for `locale` built from the supplied FTL
    /// resource strings.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_l10n::reactive::L10n;
    ///
    /// let l10n = L10n::new("en".parse().unwrap());
    /// l10n.add_bundle(
    ///     "en".parse().unwrap(),
    ///     vec!["hi = Hello!".to_string()],
    /// ).unwrap();
    /// assert_eq!(l10n.get("hi"), Some("Hello!".to_string()));
    /// ```
    pub fn add_bundle(
        &self,
        locale: LanguageIdentifier,
        resources: Vec<String>,
    ) -> Result<(), L10nError> {
        self.catalog
            .write()
            .expect("l10n catalog lock poisoned")
            .add_bundle(locale, resources)
    }

    /// Switches the active locale, updating the root locale signal and the
    /// underlying catalog.
    ///
    /// Downstream localized memos are marked dirty by the reactive runtime and
    /// re-resolve on the next pull. Returns [`L10nError::UndefinedLocale`] if
    /// `locale` is the undefined (`und`) identifier.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_l10n::reactive::L10n;
    /// use martensite_l10n::direction::ScriptDirection;
    /// use martensite_reactive::flush;
    ///
    /// let l10n = L10n::new("en".parse().unwrap());
    /// l10n.add_bundle("en".parse().unwrap(), vec!["k = Hi".to_string()]).unwrap();
    /// l10n.add_bundle("he".parse().unwrap(), vec!["k = שלום".to_string()]).unwrap();
    ///
    /// let text = l10n.localized("k");
    /// assert_eq!(text.get(), "Hi");
    ///
    /// l10n.set_locale("he".parse().unwrap()).unwrap();
    /// flush();
    /// assert_eq!(text.get(), "שלום");
    /// assert_eq!(l10n.direction(), ScriptDirection::Rtl);
    /// ```
    pub fn set_locale(&self, locale: LanguageIdentifier) -> Result<(), L10nError> {
        {
            let mut catalog = self.catalog.write().expect("l10n catalog lock poisoned");
            catalog.set_locale(locale.clone())?;
        }
        self.locale.set(locale);
        Ok(())
    }

    /// Returns a clone of the active locale without registering a reactive
    /// dependency.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_l10n::reactive::L10n;
    /// use martensite_l10n::LanguageIdentifier;
    ///
    /// let l10n = L10n::new("en-US".parse().unwrap());
    /// assert_eq!(l10n.locale(), "en-US".parse::<LanguageIdentifier>().unwrap());
    /// ```
    pub fn locale(&self) -> LanguageIdentifier {
        self.locale.get_untracked()
    }

    /// Returns a reference to the root locale [`Signal`].
    ///
    /// Reading this signal inside a reactive context establishes a dependency
    /// on the active locale.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_l10n::reactive::L10n;
    ///
    /// let l10n = L10n::new("en".parse().unwrap());
    /// let signal = l10n.locale_signal();
    /// assert_eq!(signal.get_untracked().to_string(), "en");
    /// ```
    pub fn locale_signal(&self) -> &Signal<LanguageIdentifier> {
        &self.locale
    }

    /// Returns the [`ScriptDirection`] of the active locale.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_l10n::reactive::L10n;
    /// use martensite_l10n::direction::ScriptDirection;
    ///
    /// let l10n = L10n::new("en".parse().unwrap());
    /// assert_eq!(l10n.direction(), ScriptDirection::Ltr);
    /// ```
    pub fn direction(&self) -> ScriptDirection {
        self.catalog
            .read()
            .expect("l10n catalog lock poisoned")
            .direction()
    }

    /// Resolves a localized message for the active locale with no arguments,
    /// without establishing a reactive dependency.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_l10n::reactive::L10n;
    ///
    /// let l10n = L10n::new("en".parse().unwrap());
    /// l10n.add_bundle("en".parse().unwrap(), vec!["k = Hi".to_string()]).unwrap();
    /// assert_eq!(l10n.get("k"), Some("Hi".to_string()));
    /// ```
    pub fn get(&self, key: &str) -> Option<String> {
        self.catalog
            .read()
            .expect("l10n catalog lock poisoned")
            .get(key)
    }

    /// Resolves a localized message with arguments, without establishing a
    /// reactive dependency.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_l10n::reactive::L10n;
    ///
    /// let l10n = L10n::new("en".parse().unwrap());
    /// l10n.add_bundle("en".parse().unwrap(), vec!["k = Hi, { $n }!".to_string()]).unwrap();
    /// assert_eq!(l10n.get_with_args("k", &[("n", "Ada")]), Some("Hi, Ada!".to_string()));
    /// ```
    pub fn get_with_args(&self, key: &str, args: &[(&str, &str)]) -> Option<String> {
        self.catalog
            .read()
            .expect("l10n catalog lock poisoned")
            .get_with_args(key, args)
    }

    /// Returns the locales of all registered bundles, sorted lexicographically.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_l10n::reactive::L10n;
    ///
    /// let l10n = L10n::new("en".parse().unwrap());
    /// l10n.add_bundle("en".parse().unwrap(), vec!["a = A".to_string()]).unwrap();
    /// l10n.add_bundle("de".parse().unwrap(), vec!["a = A".to_string()]).unwrap();
    ///
    /// let locales: Vec<String> = l10n.available_locales().into_iter().map(|l| l.to_string()).collect();
    /// assert_eq!(locales, vec!["de", "en"]);
    /// ```
    pub fn available_locales(&self) -> Vec<LanguageIdentifier> {
        self.catalog
            .read()
            .expect("l10n catalog lock poisoned")
            .available_locales()
            .into_iter()
            .cloned()
            .collect()
    }

    /// Negotiates the best available locale for `requested` against the
    /// registered bundles.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_l10n::reactive::L10n;
    ///
    /// let l10n = L10n::new("en".parse().unwrap());
    /// l10n.add_bundle("en".parse().unwrap(), vec!["a = A".to_string()]).unwrap();
    /// l10n.add_bundle("es".parse().unwrap(), vec!["a = A".to_string()]).unwrap();
    ///
    /// let requested = vec!["es-AR".parse().unwrap()];
    /// assert_eq!(l10n.negotiate(&requested), Some("es".parse().unwrap()));
    /// ```
    pub fn negotiate(&self, requested: &[LanguageIdentifier]) -> Option<LanguageIdentifier> {
        self.catalog
            .read()
            .expect("l10n catalog lock poisoned")
            .negotiate(requested)
    }

    /// Creates a reactive [`Memo`] that resolves `key` for the active locale.
    ///
    /// The memo reads the root locale signal, so it invalidates — and only it
    /// invalidates — when the locale changes. Missing messages resolve to the
    /// empty string so text leaves always have a well-defined value.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_l10n::reactive::L10n;
    /// use martensite_reactive::flush;
    ///
    /// let l10n = L10n::new("en".parse().unwrap());
    /// l10n.add_bundle("en".parse().unwrap(), vec!["k = Hello!".to_string()]).unwrap();
    /// l10n.add_bundle("es".parse().unwrap(), vec!["k = ¡Hola!".to_string()]).unwrap();
    ///
    /// let text = l10n.localized("k");
    /// assert_eq!(text.get(), "Hello!");
    ///
    /// l10n.set_locale("es".parse().unwrap()).unwrap();
    /// flush();
    /// assert_eq!(text.get(), "¡Hola!");
    /// ```
    pub fn localized(&self, key: impl Into<String>) -> Memo<String> {
        let catalog = Arc::clone(&self.catalog);
        let locale = self.locale.clone();
        let key = key.into();
        Memo::new(move || {
            // Read the locale signal to register the reactive dependency.
            let _active = locale.get();
            let catalog = catalog.read().expect("l10n catalog lock poisoned");
            catalog.get(&key).unwrap_or_default()
        })
    }

    /// Creates a reactive [`Memo`] that resolves `key` with named string
    /// arguments for the active locale.
    ///
    /// The memo invalidates when the locale signal changes. `args` is captured
    /// by value as owned strings so the resulting closure is `Send + Sync`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_l10n::reactive::L10n;
    /// use martensite_reactive::flush;
    ///
    /// let l10n = L10n::new("en".parse().unwrap());
    /// l10n.add_bundle("en".parse().unwrap(), vec!["k = Hi, { $name }!".to_string()]).unwrap();
    /// l10n.add_bundle("es".parse().unwrap(), vec!["k = ¡Hola, { $name }!".to_string()]).unwrap();
    ///
    /// let text = l10n.localized_with_args("k", vec![("name".to_string(), "Ada".to_string())]);
    /// assert_eq!(text.get(), "Hi, Ada!");
    ///
    /// l10n.set_locale("es".parse().unwrap()).unwrap();
    /// flush();
    /// assert_eq!(text.get(), "¡Hola, Ada!");
    /// ```
    pub fn localized_with_args(
        &self,
        key: impl Into<String>,
        args: Vec<(String, String)>,
    ) -> Memo<String> {
        let catalog = Arc::clone(&self.catalog);
        let locale = self.locale.clone();
        let key = key.into();
        Memo::new(move || {
            // Read the locale signal to register the reactive dependency.
            let _active = locale.get();
            let catalog = catalog.read().expect("l10n catalog lock poisoned");
            let args_ref: Vec<(&str, &str)> =
                args.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
            catalog.get_with_args(&key, &args_ref).unwrap_or_default()
        })
    }
}

impl std::fmt::Debug for L10n {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let catalog = self.catalog.read().expect("l10n catalog lock poisoned");
        f.debug_struct("L10n")
            .field("locale", &self.locale.get_untracked())
            .field("direction", &catalog.direction())
            .field("available_locales", &catalog.available_locales())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_reactive::{create_effect, flush};
    use std::str::FromStr;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn build_bilingual() -> L10n {
        let l10n = L10n::new(LanguageIdentifier::from_str("en").unwrap());
        l10n.add_bundle(
            LanguageIdentifier::from_str("en").unwrap(),
            vec![
                "hello = Hello!".to_string(),
                "greet = Hello, { $name }!".to_string(),
            ],
        )
        .unwrap();
        l10n.add_bundle(
            LanguageIdentifier::from_str("es").unwrap(),
            vec![
                "hello = ¡Hola!".to_string(),
                "greet = ¡Hola, { $name }!".to_string(),
            ],
        )
        .unwrap();
        l10n
    }

    #[test]
    fn localized_memo_tracks_locale_signal() {
        let l10n = build_bilingual();
        let text = l10n.localized("hello");

        assert_eq!(text.get(), "Hello!");

        l10n.set_locale(LanguageIdentifier::from_str("es").unwrap())
            .unwrap();
        flush();
        assert_eq!(text.get(), "¡Hola!");
    }

    #[test]
    fn localized_with_args_tracks_locale_signal() {
        let l10n = build_bilingual();
        let text = l10n.localized_with_args("greet", vec![("name".to_string(), "Ada".to_string())]);

        assert_eq!(text.get(), "Hello, Ada!");

        l10n.set_locale(LanguageIdentifier::from_str("es").unwrap())
            .unwrap();
        flush();
        assert_eq!(text.get(), "¡Hola, Ada!");
    }

    #[test]
    fn missing_message_resolves_to_empty_string() {
        let l10n = build_bilingual();
        let text = l10n.localized("missing-key");
        assert_eq!(text.get(), "");
    }

    #[test]
    fn set_locale_rejects_undefined_locale() {
        let l10n = build_bilingual();
        let result = l10n.set_locale(LanguageIdentifier::default());
        assert_eq!(result, Err(L10nError::UndefinedLocale));
        // Locale signal must be unchanged.
        assert_eq!(l10n.locale().to_string(), "en");
    }

    #[test]
    fn only_localized_memos_invalidate_on_switch() {
        let l10n = build_bilingual();

        let recompute_count = Arc::new(AtomicUsize::new(0));
        let recompute_count_clone = Arc::clone(&recompute_count);
        let text = l10n.localized("hello");

        // An effect that reads the localized memo. It should re-run when the
        // locale changes (because its dependency, the memo, invalidates).
        create_effect(move || {
            recompute_count_clone.fetch_add(1, Ordering::SeqCst);
            let _ = text.get();
        });
        flush();

        let initial = recompute_count.load(Ordering::SeqCst);
        l10n.set_locale(LanguageIdentifier::from_str("es").unwrap())
            .unwrap();
        flush();

        assert!(
            recompute_count.load(Ordering::SeqCst) > initial,
            "effect bound to localized memo must re-run after locale switch"
        );
    }

    #[test]
    fn unrelated_signal_does_not_invalidate_localized_memo() {
        let other = Signal::new(0i32);
        let l10n = build_bilingual();
        let text = l10n.localized("hello");

        // Establish the memo's initial dependency on the locale signal only.
        let initial = text.get();
        assert_eq!(initial, "Hello!");

        // Mutating an unrelated signal must not change the localized value.
        other.set(42);
        flush();
        assert_eq!(text.get_untracked(), "Hello!");
    }

    #[test]
    fn locale_signal_get_reflects_changes() {
        let l10n = build_bilingual();
        assert_eq!(l10n.locale_signal().get_untracked().to_string(), "en");

        l10n.set_locale(LanguageIdentifier::from_str("es").unwrap())
            .unwrap();
        assert_eq!(l10n.locale_signal().get_untracked().to_string(), "es");
    }

    #[test]
    fn available_locales_and_negotiate_delegate_to_catalog() {
        let l10n = build_bilingual();
        let locales: Vec<String> = l10n
            .available_locales()
            .into_iter()
            .map(|l| l.to_string())
            .collect();
        assert_eq!(locales, vec!["en", "es"]);

        let requested = vec![LanguageIdentifier::from_str("es-AR").unwrap()];
        assert_eq!(
            l10n.negotiate(&requested),
            Some(LanguageIdentifier::from_str("es").unwrap())
        );
    }

    #[test]
    fn direction_reflects_active_locale() {
        let l10n = L10n::new(LanguageIdentifier::from_str("en").unwrap());
        assert_eq!(l10n.direction(), ScriptDirection::Ltr);

        l10n.set_locale(LanguageIdentifier::from_str("ar").unwrap())
            .unwrap();
        assert_eq!(l10n.direction(), ScriptDirection::Rtl);
    }

    #[test]
    fn thousand_localized_nodes_settle_after_locale_switch() {
        // Exit criteria: switching locale across 1,000 active text nodes
        // settles within a single flush (one frame).
        let l10n = L10n::new(LanguageIdentifier::from_str("en").unwrap());
        l10n.add_bundle(
            LanguageIdentifier::from_str("en").unwrap(),
            vec!["node = English".to_string()],
        )
        .unwrap();
        l10n.add_bundle(
            LanguageIdentifier::from_str("es").unwrap(),
            vec!["node = Spanish".to_string()],
        )
        .unwrap();

        let memos: Vec<Memo<String>> = (0..1_000).map(|_| l10n.localized("node")).collect();

        assert!(memos.iter().all(|m| m.get() == "English"));

        l10n.set_locale(LanguageIdentifier::from_str("es").unwrap())
            .unwrap();
        // A single flush corresponds to one frame's Phase 2 topological pass.
        flush();

        assert!(
            memos.iter().all(|m| m.get_untracked() == "Spanish"),
            "all 1000 localized memos must settle to the new locale after one flush"
        );
    }

    #[test]
    fn debug_repr_compiles() {
        let l10n = build_bilingual();
        let s = format!("{l10n:?}");
        assert!(s.contains("L10n"));
    }
}
