//! `PdfView` — a paged PDF viewer. The widget owns a
//! [`PdfDocument`](martensite_pdf::PdfDocument)
//! (`martensite-pdf`'s raster contract) and paints the current page
//! via `PaintList::push_image` — the same call a pdfium/mupdf backend
//! will make once one lands. It defaults to
//! [`BlankPdfDocument`](martensite_pdf::BlankPdfDocument), a
//! procedural document that produces real pixels, so the scaffold
//! shows a genuine page raster immediately rather than a stub.
//!
//! Keyboard: `PageDown`/`→`/`↓` next page, `PageUp`/`←`/`↑` previous,
//! `Home`/`End` first/last, `+`/`-` zoom step, `0` fit-page.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::pdf_view::PdfView;
//!
//! let mut v = PdfView::new();
//! assert_eq!(v.page(), 0);
//! assert!(v.page_count() > 0);
//! v.next_page();
//! assert_eq!(v.page(), 1);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, TokenKey, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_pdf::{BlankPdfDocument, PageSize, PdfDocInfo, PdfDocument};

use crate::text_paint::SharedTextPainter;

const PAD_PT: f32 = 12.0;
const LABEL_PT: f32 = 10.0;
const RENDER_MAX_PX: u32 = 1024;
const ZOOM_STEP: f32 = 0.25;
const ZOOM_MIN: f32 = 0.5;
const ZOOM_MAX: f32 = 4.0;

const CANVAS: [u8; 4] = [96, 100, 110, 255];
const EDGE: [u8; 4] = [60, 64, 74, 255];
const MUTED: [u8; 4] = [215, 218, 226, 255];

/// How the page scales to the widget bounds.
///
/// # Examples
///
/// ```
/// use martensite::widgets::pdf_view::ZoomMode;
///
/// assert_eq!(ZoomMode::default(), ZoomMode::FitPage);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum ZoomMode {
    /// Scale so the whole page fits inside the widget.
    #[default]
    FitPage,
    /// Scale so the page width matches the widget width.
    FitWidth,
    /// Explicit factor relative to fit-page scale (`1.0` = fit).
    Zoom(f32),
}

/// The paged PDF viewer — see the module docs.
///
/// ```
/// use martensite::widgets::pdf_view::PdfView;
///
/// let v = PdfView::new();
/// assert_eq!(v.page_count(), 3);
/// ```
pub struct PdfView {
    /// Accessibility label (overrides the document title).
    pub label: String,
    doc: Box<dyn PdfDocument>,
    /// Current 0-based page.
    page: u32,
    /// Active zoom mode.
    zoom: ZoomMode,
    text_painter: Option<SharedTextPainter>,
    bounds: Rect,
    scale: f32,
}

impl std::fmt::Debug for PdfView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PdfView")
            .field("page", &self.page)
            .field("pages", &self.doc.info().page_count)
            .finish()
    }
}

impl PdfView {
    /// A viewer over a 3-page [`BlankPdfDocument`].
    ///
    /// ```
    /// use martensite::widgets::pdf_view::PdfView;
    ///
    /// assert_eq!(PdfView::new().page_count(), 3);
    /// ```
    pub fn new() -> Self {
        Self::with_document(Box::new(BlankPdfDocument::new(3).with_title("Document")))
    }

    /// Open a real document through the build's
    /// [`default_pdf_provider`](martensite_pdf::default_pdf_provider) —
    /// the CLI rasterizer (`pdftoppm`/`mutool`) when `martensite-pdf`'s
    /// `platform` feature is enabled and a toolset is installed.
    /// Returns the provider's error (`Unsupported` without a backend,
    /// `OpenFailed` on a bad file) rather than falling back silently.
    ///
    /// ```no_run
    /// use martensite::widgets::pdf_view::PdfView;
    /// use martensite_pdf::PdfSource;
    /// use std::path::PathBuf;
    ///
    /// let v = PdfView::open(&PdfSource::File(PathBuf::from("spec.pdf")))?;
    /// assert!(v.page_count() > 0);
    /// # Ok::<(), martensite_pdf::PdfError>(())
    /// ```
    pub fn open(source: &martensite_pdf::PdfSource) -> Result<Self, martensite_pdf::PdfError> {
        let doc = martensite_pdf::default_pdf_provider().open(source)?;
        Ok(Self::with_document(doc))
    }

    /// A viewer over an explicit [`PdfDocument`] — the seam a real
    /// rasterizer backend (pdfium, mupdf, Quartz) plugs into.
    ///
    /// ```
    /// use martensite::widgets::pdf_view::PdfView;
    /// use martensite_pdf::BlankPdfDocument;
    ///
    /// let v = PdfView::with_document(Box::new(BlankPdfDocument::new(10)));
    /// assert_eq!(v.page_count(), 10);
    /// ```
    pub fn with_document(doc: Box<dyn PdfDocument>) -> Self {
        Self {
            label: String::new(),
            doc,
            page: 0,
            zoom: ZoomMode::FitPage,
            text_painter: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Accessibility label override.
    ///
    /// ```
    /// use martensite::widgets::pdf_view::PdfView;
    ///
    /// assert_eq!(PdfView::new().label("Spec").label, "Spec");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Shared text painter.
    ///
    /// ```no_run
    /// use martensite::widgets::pdf_view::PdfView;
    /// # fn example(p: martensite::text_paint::SharedTextPainter) {
    /// let _w = PdfView::new().with_text_painter(p);
    /// # }
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    // ---- document accessors -------------------------------------------

    /// The backing document's metadata.
    ///
    /// ```
    /// use martensite::widgets::pdf_view::PdfView;
    ///
    /// assert_eq!(PdfView::new().doc_info().title.as_deref(), Some("Document"));
    /// ```
    pub fn doc_info(&self) -> PdfDocInfo {
        self.doc.info()
    }

    /// Number of pages.
    ///
    /// ```
    /// use martensite::widgets::pdf_view::PdfView;
    ///
    /// assert_eq!(PdfView::new().page_count(), 3);
    /// ```
    pub fn page_count(&self) -> u32 {
        self.doc.info().page_count
    }

    /// Current 0-based page index.
    ///
    /// ```
    /// use martensite::widgets::pdf_view::PdfView;
    ///
    /// assert_eq!(PdfView::new().page(), 0);
    /// ```
    pub fn page(&self) -> u32 {
        self.page
    }

    /// Current page's media-box size in points, `None` when the
    /// document is empty.
    ///
    /// ```
    /// use martensite::widgets::pdf_view::PdfView;
    ///
    /// assert!(PdfView::new().page_size().is_some());
    /// ```
    pub fn page_size(&self) -> Option<PageSize> {
        self.doc.page_size(self.page)
    }

    // ---- navigation ----------------------------------------------------

    /// Jump to `page` (clamped to the last page).
    ///
    /// ```
    /// use martensite::widgets::pdf_view::PdfView;
    ///
    /// let mut v = PdfView::new();
    /// v.set_page(99);
    /// assert_eq!(v.page(), 2); // clamped
    /// ```
    pub fn set_page(&mut self, page: u32) {
        let last = self.page_count().saturating_sub(1);
        self.page = page.min(last);
    }

    /// Next page (no-op on the last).
    ///
    /// ```
    /// use martensite::widgets::pdf_view::PdfView;
    ///
    /// let mut v = PdfView::new();
    /// v.next_page();
    /// assert_eq!(v.page(), 1);
    /// ```
    pub fn next_page(&mut self) {
        self.set_page(self.page + 1);
    }

    /// Previous page (no-op on the first).
    ///
    /// ```
    /// use martensite::widgets::pdf_view::PdfView;
    ///
    /// let mut v = PdfView::new();
    /// v.prev_page();
    /// assert_eq!(v.page(), 0);
    /// ```
    pub fn prev_page(&mut self) {
        self.set_page(self.page.saturating_sub(1));
    }

    /// First page.
    ///
    /// ```
    /// use martensite::widgets::pdf_view::PdfView;
    ///
    /// let mut v = PdfView::new();
    /// v.set_page(2);
    /// v.first_page();
    /// assert_eq!(v.page(), 0);
    /// ```
    pub fn first_page(&mut self) {
        self.set_page(0);
    }

    /// Last page.
    ///
    /// ```
    /// use martensite::widgets::pdf_view::PdfView;
    ///
    /// let mut v = PdfView::new();
    /// v.last_page();
    /// assert_eq!(v.page(), v.page_count() - 1);
    /// ```
    pub fn last_page(&mut self) {
        self.set_page(u32::MAX);
    }

    // ---- zoom ----------------------------------------------------------

    /// Active zoom mode.
    ///
    /// ```
    /// use martensite::widgets::pdf_view::{PdfView, ZoomMode};
    ///
    /// assert_eq!(PdfView::new().zoom_mode(), ZoomMode::FitPage);
    /// ```
    pub fn zoom_mode(&self) -> ZoomMode {
        self.zoom
    }

    /// Set the zoom mode.
    ///
    /// ```
    /// use martensite::widgets::pdf_view::{PdfView, ZoomMode};
    ///
    /// let mut v = PdfView::new();
    /// v.set_zoom(ZoomMode::FitWidth);
    /// assert_eq!(v.zoom_mode(), ZoomMode::FitWidth);
    /// ```
    pub fn set_zoom(&mut self, zoom: ZoomMode) {
        self.zoom = zoom;
    }

    /// Step explicit zoom up (clamped to 4×); leaves `Fit*` modes,
    /// switching to `Zoom`.
    ///
    /// ```
    /// use martensite::widgets::pdf_view::{PdfView, ZoomMode};
    ///
    /// let mut v = PdfView::new();
    /// v.zoom_in();
    /// assert_eq!(v.zoom_mode(), ZoomMode::Zoom(1.25));
    /// ```
    pub fn zoom_in(&mut self) {
        self.zoom = ZoomMode::Zoom((self.zoom_factor() + ZOOM_STEP).min(ZOOM_MAX));
    }

    /// Step explicit zoom down (clamped to 0.5×).
    ///
    /// ```
    /// use martensite::widgets::pdf_view::{PdfView, ZoomMode};
    ///
    /// let mut v = PdfView::new();
    /// v.zoom_out();
    /// assert_eq!(v.zoom_mode(), ZoomMode::Zoom(0.75));
    /// ```
    pub fn zoom_out(&mut self) {
        self.zoom = ZoomMode::Zoom((self.zoom_factor() - ZOOM_STEP).max(ZOOM_MIN));
    }

    /// The explicit zoom factor — `1.0` for `Fit*` modes, the `Zoom`
    /// value otherwise.
    ///
    /// ```
    /// use martensite::widgets::pdf_view::PdfView;
    ///
    /// assert_eq!(PdfView::new().zoom_factor(), 1.0);
    /// ```
    pub fn zoom_factor(&self) -> f32 {
        match self.zoom {
            ZoomMode::Zoom(z) => z,
            ZoomMode::FitPage | ZoomMode::FitWidth => 1.0,
        }
    }

    /// Page rect inside `bounds` under the current zoom mode.
    fn page_rect(&self, b: Rect, size: PageSize) -> Rect {
        let fit_scale = (b.width() / size.width.max(1.0)).min(b.height() / size.height.max(1.0));
        let s = match self.zoom {
            ZoomMode::FitPage => fit_scale,
            ZoomMode::FitWidth => b.width() / size.width.max(1.0),
            ZoomMode::Zoom(z) => fit_scale * z,
        };
        let w = size.width * s;
        let h = size.height * s;
        // Centered horizontally, vertically centered when it fits,
        // top-aligned when it overflows (scrollback is a future layer).
        let x = b.min_x() + (b.width() - w).max(0.0) * 0.5;
        let y = if h <= b.height() {
            b.min_y() + (b.height() - h) * 0.5
        } else {
            b.min_y()
        };
        Rect::new(x, y, w, h)
    }
}

impl Default for PdfView {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for PdfView {
    fn measure(&mut self, _cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            constraints.max_size.x.max(0.0),
            constraints.max_size.y.max(0.0),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(120.0, 160.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Document);
        let info = self.doc.info();
        node.set_label(if !self.label.is_empty() {
            self.label.clone()
        } else {
            info.title.unwrap_or_else(|| "PDF document".to_string())
        });
        if info.page_count > 0 {
            node.set_value(format!("Page {} of {}", self.page + 1, info.page_count));
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        let WidgetEvent::KeyPressed { key, .. } = cx.event else {
            return EventResponse::Ignored;
        };
        match key.as_str() {
            "PageDown" | "ArrowRight" | "ArrowDown" => self.next_page(),
            "PageUp" | "ArrowLeft" | "ArrowUp" => self.prev_page(),
            "Home" => self.first_page(),
            "End" => self.last_page(),
            "+" | "=" => self.zoom_in(),
            "-" => self.zoom_out(),
            "0" => self.set_zoom(ZoomMode::FitPage),
            _ => return EventResponse::Ignored,
        }
        EventResponse::RequestRepaint
    }

    fn paint(&self, cx: &mut PaintContext) {
        let s = cx.scale;
        let b = self.bounds;
        // Dark canvas behind the page (reader chrome).
        cx.list.push_fill_rect(
            kurbo::Rect::new(
                f64::from(b.min_x()),
                f64::from(b.min_y()),
                f64::from(b.max_x()),
                f64::from(b.max_y()),
            ),
            cx.color(TokenKey::SurfaceColor, CANVAS),
        );

        if let Some(size) = self.page_size() {
            let pr = self.page_rect(b, size);
            let krect = kurbo::Rect::new(
                f64::from(pr.min_x()),
                f64::from(pr.min_y()),
                f64::from(pr.max_x()),
                f64::from(pr.max_y()),
            );
            if let Some(bmp) = self.doc.render_page(self.page, RENDER_MAX_PX) {
                if let Some(img) =
                    martensite_core::ImageData::from_rgba(bmp.width, bmp.height, bmp.pixels)
                {
                    cx.list.push_image(krect, img);
                }
            } else {
                // No raster — placeholder page outline.
                cx.list
                    .push_stroke_rect(krect, cx.pt(1.0), cx.color(TokenKey::BorderColor, EDGE));
            }
        }

        // Page indicator, bottom-right.
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let count = self.page_count();
        if count > 0 {
            let label = format!("{} / {}", self.page + 1, count);
            let fs = LABEL_PT * s;
            let pad = PAD_PT * s;
            crate::text_paint::paint_label(
                painter,
                cx.list,
                kurbo::Point::new(
                    f64::from(b.max_x() - pad - label.len() as f32 * fs * 0.55),
                    f64::from(b.max_y() - pad),
                ),
                &label,
                fs,
                cx.color(TokenKey::TextMutedColor, MUTED),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(v: &mut PdfView, k: &str) -> EventResponse {
        let ev = WidgetEvent::KeyPressed {
            key: k.to_string(),
            repeat: false,
        };
        let mut cx = EventContext {
            event: &ev,
            bounds: Rect::new(0.0, 0.0, 400.0, 300.0),
            scale: 1.0,
        };
        v.event(&mut cx)
    }

    #[test]
    fn keyboard_navigation() {
        let mut v = PdfView::with_document(Box::new(BlankPdfDocument::new(5)));
        assert_eq!(key(&mut v, "PageDown"), EventResponse::RequestRepaint);
        assert_eq!(v.page(), 1);
        key(&mut v, "End");
        assert_eq!(v.page(), 4);
        key(&mut v, "PageDown");
        assert_eq!(v.page(), 4); // clamped
        key(&mut v, "Home");
        assert_eq!(v.page(), 0);
        assert_eq!(key(&mut v, "x"), EventResponse::Ignored);
    }

    #[test]
    fn zoom_steps() {
        let mut v = PdfView::new();
        key(&mut v, "+");
        assert_eq!(v.zoom_mode(), ZoomMode::Zoom(1.25));
        key(&mut v, "-");
        key(&mut v, "-");
        assert_eq!(v.zoom_mode(), ZoomMode::Zoom(0.75));
        key(&mut v, "0");
        assert_eq!(v.zoom_mode(), ZoomMode::FitPage);
    }

    #[test]
    fn page_rect_fit_page_centers() {
        let v = PdfView::new();
        // Widget wider than the page aspect → centered horizontally.
        let b = Rect::new(0.0, 0.0, 1000.0, 792.0);
        let pr = v.page_rect(b, PageSize::LETTER);
        assert!((pr.height() - 792.0).abs() < 1.0);
        assert!((pr.width() - 612.0).abs() < 1.0);
        assert!(pr.min_x() > 0.0);
    }

    #[test]
    fn page_rect_fit_width_top_aligns() {
        let mut v = PdfView::new();
        v.set_zoom(ZoomMode::FitWidth);
        let b = Rect::new(0.0, 0.0, 612.0, 300.0);
        let pr = v.page_rect(b, PageSize::LETTER);
        assert!((pr.width() - 612.0).abs() < 1.0);
        assert!((pr.min_y() - b.min_y()).abs() < f32::EPSILON);
    }
}
