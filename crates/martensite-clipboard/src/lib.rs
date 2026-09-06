//! Multi-MIME delayed rendering clipboard engine.
#![forbid(unsafe_code)]

/// A multi-MIME clipboard payload carrying optional plain-text and HTML representations.
#[derive(Default)]
pub struct ClipboardItem {
    /// Optional plain-text representation of the clipboard contents.
    pub text: Option<String>,
    /// Optional HTML representation of the clipboard contents.
    pub html: Option<String>,
}

impl ClipboardItem {
    /// Creates a new empty [`ClipboardItem`] with no text or HTML payloads.
    pub fn new() -> Self {
        Self::default()
    }
    /// Sets the plain-text payload and returns `self` for chaining.
    pub fn offer_text(mut self, text: impl Into<String>) -> Self {
        self.text = Some(text.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::ClipboardItem;

    #[test]
    fn new_creates_empty_item() {
        let item = ClipboardItem::new();
        assert!(item.text.is_none());
        assert!(item.html.is_none());
    }

    #[test]
    fn default_matches_new() {
        let new_item = ClipboardItem::new();
        let default_item = ClipboardItem::default();
        assert!(new_item.text.is_none());
        assert!(default_item.text.is_none());
        assert!(new_item.html.is_none());
        assert!(default_item.html.is_none());
    }

    #[test]
    fn offer_text_sets_text_and_returns_self() {
        let item = ClipboardItem::new().offer_text("hello");
        assert_eq!(item.text, Some("hello".to_string()));
        assert!(item.html.is_none());
    }

    #[test]
    fn offer_text_accepts_str_and_string() {
        let from_str = ClipboardItem::new().offer_text("from &str");
        assert_eq!(from_str.text, Some("from &str".to_string()));

        let from_string = ClipboardItem::new().offer_text(String::from("from String"));
        assert_eq!(from_string.text, Some("from String".to_string()));
    }
}
