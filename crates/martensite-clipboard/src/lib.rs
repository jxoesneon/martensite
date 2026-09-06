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
