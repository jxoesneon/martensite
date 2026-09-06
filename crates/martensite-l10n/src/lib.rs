//! Fluent natural language localization.
#![forbid(unsafe_code)]

pub use unic_langid::LanguageIdentifier;

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
