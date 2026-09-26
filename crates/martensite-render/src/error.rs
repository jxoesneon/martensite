//! Structured rendering error definitions and diagnostic conversion.

use crate::presentation::PresentationError;

/// Structured rendering error produced by rendering backends or presentation layers.
///
/// # Examples
///
/// ```
/// use martensite_render::PaintError;
///
/// let err = PaintError::DeviceLost("GPU hung".into());
/// assert!(err.is_fatal());
/// assert_eq!(format!("{err}"), "GPU device lost: GPU hung");
/// ```
#[derive(Debug, Clone, PartialEq)]
pub enum PaintError {
    /// Texture, buffer, or memory allocation failed.
    AllocationFailed {
        /// Width in texels/pixels, if known.
        width: u32,
        /// Height in texels/pixels, if known.
        height: u32,
        /// Specific reason or backend message.
        details: String,
    },
    /// The GPU device was lost or disconnected.
    DeviceLost(String),
    /// Shader compilation, pipeline creation, or resource binding error.
    ShaderCompilation(String),
    /// Vello bump buffer overflow (e.g. blend stack or binning spill).
    VelloBumpOverflow(String),
    /// Softbuffer or window presentation failure.
    Presentation(String),
    /// Backend-specific unrecoverable or recoverable error.
    Backend(String),
}

impl PaintError {
    /// Returns `true` if this error indicates an unrecoverable device loss requiring re-initialization.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_render::PaintError;
    ///
    /// assert!(PaintError::DeviceLost("driver crash".into()).is_fatal());
    /// assert!(!PaintError::VelloBumpOverflow("blend spill".into()).is_fatal());
    /// ```
    #[inline]
    pub const fn is_fatal(&self) -> bool {
        matches!(self, Self::DeviceLost(_))
    }

    /// Converts this paint error into a structured diagnostic with scope path and optional bounds.
    ///
    /// # Examples
    ///
    /// ```
    /// use kurbo::Rect;
    /// use martensite_render::PaintError;
    ///
    /// let err = PaintError::VelloBumpOverflow("bump.blend overflow".into());
    /// let diag = err.to_diagnostic("App/Viewport/Canvas", Some(Rect::new(0.0, 0.0, 800.0, 600.0)));
    /// assert_eq!(diag.node_path, "App/Viewport/Canvas");
    /// assert!(diag.message.contains("Vello bump"));
    /// assert_eq!(diag.doc_link, "https://martensite.dev/docs/errors/paint");
    /// ```
    pub fn to_diagnostic(
        &self,
        node_path: impl Into<String>,
        bounds: Option<kurbo::Rect>,
    ) -> PaintDiagnosticInfo {
        PaintDiagnosticInfo {
            node_path: node_path.into(),
            message: self.to_string(),
            bounds,
            doc_link: "https://martensite.dev/docs/errors/paint".to_string(),
        }
    }
}

impl std::fmt::Display for PaintError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AllocationFailed {
                width,
                height,
                details,
            } => {
                write!(f, "allocation failed for {width}x{height}: {details}")
            }
            Self::DeviceLost(msg) => write!(f, "GPU device lost: {msg}"),
            Self::ShaderCompilation(msg) => write!(f, "shader compilation failed: {msg}"),
            Self::VelloBumpOverflow(msg) => write!(f, "Vello bump buffer overflow: {msg}"),
            Self::Presentation(msg) => write!(f, "presentation failed: {msg}"),
            Self::Backend(msg) => write!(f, "backend error: {msg}"),
        }
    }
}

impl std::error::Error for PaintError {}

impl From<PresentationError> for PaintError {
    fn from(err: PresentationError) -> Self {
        Self::Presentation(err.to_string())
    }
}

/// Diagnostic information converted from a [`PaintError`].
///
/// Designed to attach directly to DevTools' `PaintErrorDiagnostic`.
///
/// # Examples
///
/// ```
/// use kurbo::Rect;
/// use martensite_render::PaintDiagnosticInfo;
///
/// let info = PaintDiagnosticInfo::new(
///     "Root/Panel",
///     "Out of memory",
///     Some(Rect::new(0.0, 0.0, 100.0, 100.0)),
/// );
/// assert_eq!(info.node_path, "Root/Panel");
/// assert_eq!(info.doc_link, "https://martensite.dev/docs/errors/paint");
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct PaintDiagnosticInfo {
    /// Node or scope path where the error occurred.
    pub node_path: String,
    /// Human-readable error message.
    pub message: String,
    /// Bounding rectangle where the error occurred, if known.
    pub bounds: Option<kurbo::Rect>,
    /// Link to documentation for resolving paint errors.
    pub doc_link: String,
}

impl PaintDiagnosticInfo {
    /// Creates a new `PaintDiagnosticInfo` with default documentation link.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_render::PaintDiagnosticInfo;
    ///
    /// let info = PaintDiagnosticInfo::new("Canvas", "Buffer allocation failed", None);
    /// assert_eq!(info.node_path, "Canvas");
    /// ```
    pub fn new(
        node_path: impl Into<String>,
        message: impl Into<String>,
        bounds: Option<kurbo::Rect>,
    ) -> Self {
        Self {
            node_path: node_path.into(),
            message: message.into(),
            bounds,
            doc_link: "https://martensite.dev/docs/errors/paint".to_string(),
        }
    }

    /// Overrides the documentation link.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_render::PaintDiagnosticInfo;
    ///
    /// let info = PaintDiagnosticInfo::new("Canvas", "err", None)
    ///     .with_doc_link("https://custom.link/docs");
    /// assert_eq!(info.doc_link, "https://custom.link/docs");
    /// ```
    #[must_use]
    pub fn with_doc_link(mut self, link: impl Into<String>) -> Self {
        self.doc_link = link.into();
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kurbo::Rect;

    #[test]
    fn test_paint_error_formatting_and_diagnostic() {
        let err = PaintError::AllocationFailed {
            width: 1920,
            height: 1080,
            details: "out of VRAM".into(),
        };
        assert!(!err.is_fatal());
        assert!(format!("{err}").contains("1920x1080"));

        let bounds = Some(Rect::new(0.0, 0.0, 1920.0, 1080.0));
        let diag = err.to_diagnostic("App/Viewport", bounds);
        assert_eq!(diag.node_path, "App/Viewport");
        assert!(diag.message.contains("out of VRAM"));
        assert_eq!(diag.bounds, bounds);
    }

    #[test]
    fn test_presentation_error_conversion() {
        let pres_err = PresentationError::EmptyBuffer;
        let paint_err: PaintError = pres_err.into();
        assert!(matches!(paint_err, PaintError::Presentation(_)));
        assert!(format!("{paint_err}").contains("pixel buffer is empty"));
    }
}
