//! Multi-MIME delayed rendering clipboard engine.
#![forbid(unsafe_code)]

#[derive(Default)]
pub struct ClipboardItem {
    pub text: Option<String>,
    pub html: Option<String>,
}

impl ClipboardItem {
    pub fn new() -> Self { Self::default() }
    pub fn offer_text(mut self, text: impl Into<String>) -> Self {
        self.text = Some(text.into());
        self
    }
}
