//! Fluent natural language localization.
//!
//! `martensite-l10n` provides Project Fluent integration, script directionality
//! resolution for BiDi mirroring, and a reactive locale signal binding for the
//! Martensite signal graph. The crate is fully `#![forbid(unsafe_code)]` and
//! every public item is documented with examples.
//!
//! # Modules
//!
//! - [`direction`]: [`ScriptDirection`][direction::ScriptDirection] resolution
//!   from a [`LanguageIdentifier`].
//! - [`fluent`]: [`FluentCatalog`][fluent::FluentCatalog] bundle management,
//!   message resolution, and locale negotiation.
//! - [`reactive`]: [`L10n`][reactive::L10n] reactive locale signal integration.
//!
//! # Examples
//!
//! ```
//! use martensite_l10n::reactive::L10n;
//! use martensite_reactive::flush;
//!
//! let l10n = L10n::new("en".parse().unwrap());
//! l10n.add_bundle(
//!     "en".parse().unwrap(),
//!     vec!["greeting = Hello, world!".to_string()],
//! ).unwrap();
//! l10n.add_bundle(
//!     "es".parse().unwrap(),
//!     vec!["greeting = ¡Hola, mundo!".to_string()],
//! ).unwrap();
//!
//! let text = l10n.localized("greeting");
//! assert_eq!(text.get(), "Hello, world!");
//!
//! l10n.set_locale("es".parse().unwrap()).unwrap();
//! flush();
//! assert_eq!(text.get(), "¡Hola, mundo!");
//! ```
#![forbid(unsafe_code)]
#![deny(missing_docs)]

/// A Unicode BCP47 language identifier (e.g. `en`, `en-US`, `zh-Hans`).
///
/// This is a re-export of [`unic_langid::LanguageIdentifier`], the locale key
/// used throughout the crate to register and select Fluent bundles.
///
/// # Examples
///
/// ```
/// use martensite_l10n::LanguageIdentifier;
/// use std::str::FromStr;
///
/// let en: LanguageIdentifier = LanguageIdentifier::from_str("en-US").unwrap();
/// assert_eq!(en.to_string(), "en-US");
/// ```
pub use unic_langid::LanguageIdentifier;

/// Script directionality resolution (LTR/RTL) for BiDi layout mirroring.
pub mod direction;
/// Project Fluent bundle integration, message resolution, and locale negotiation.
pub mod fluent;
/// Reactive locale signal integration binding Fluent resources into the signal graph.
pub mod reactive;

#[cfg(test)]
mod tests {
    use super::LanguageIdentifier;
    use std::str::FromStr;

    #[test]
    fn parse_en() {
        let li = LanguageIdentifier::from_str("en").expect("en should parse");
        assert_eq!(li.to_string(), "en");
    }

    #[test]
    fn parse_en_us() {
        let li = LanguageIdentifier::from_str("en-US").expect("en-US should parse");
        assert_eq!(li.to_string(), "en-US");
    }

    #[test]
    fn invalid_identifier_returns_error() {
        let result = LanguageIdentifier::from_str("not-a-language!!!");
        assert!(
            result.is_err(),
            "invalid language identifier must return an error"
        );
    }

    #[test]
    fn default_language_identifier() {
        let li = LanguageIdentifier::default();
        assert_eq!(li.to_string(), "und", "default language identifier is und");
    }
}
