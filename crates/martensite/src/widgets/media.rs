//! `MediaView` widget presenting hardware video surfaces with aspect ratio preservation.
//!
//! This module provides the [`MediaView`] widget, enabling zero-copy video playback
//! directly within the Martensite layout and widget tree hierarchy.

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::node::Rect;
use martensite_core::widget::{LayoutConstraints, LayoutContext, PaintContext, Widget};
use martensite_media::decoder::{EncodedPacket, VideoDecoder};
use martensite_media::queue::{FrameQueue, QueueAction};
use martensite_media::surface::VideoSurface;

/// Scaling fit mode for video surfaces within their allocated layout bounds.
///
/// # Examples
///
/// ```
/// use martensite::widgets::media::VideoFit;
///
/// assert_eq!(VideoFit::default(), VideoFit::Contain);
/// ```
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum VideoFit {
    /// Preserves aspect ratio, scaling to fit entirely within bounds (letterbox or pillarbox).
    #[default]
    Contain,
    /// Preserves aspect ratio, scaling to fill bounds completely (cropping overflow).
    Cover,
    /// Stretches content to completely fill bounds, ignoring native aspect ratio.
    Fill,
    /// Preserves native video resolution centered in bounds without scaling.
    Fixed,
}

/// A retained-mode widget presenting hardware video surfaces.
///
/// `MediaView` integrates with the two-level layout engine, preserving aspect ratios,
/// computing letterbox/pillarbox viewports, and providing AccessKit video semantics.
///
/// # Examples
///
/// ```
/// use martensite::widgets::media::{MediaView, VideoFit};
/// use martensite_media::surface::{VideoPixelFormat, VideoSurface};
///
/// let surface = VideoSurface::new_mock(1920, 1080, VideoPixelFormat::Nv12);
/// let view = MediaView::new().with_surface(surface).with_fit(VideoFit::Contain);
/// assert_eq!(view.fit(), VideoFit::Contain);
/// ```
pub struct MediaView {
    surface: Option<VideoSurface>,
    fit: VideoFit,
    explicit_aspect_ratio: Option<f32>,
    cached_bounds: Rect,
    cached_video_rect: Rect,
    /// Decode producer feeding [`VideoSurface`] handles through `queue`.
    decoder: Option<Box<dyn VideoDecoder>>,
    /// Decode-ahead pacing ring; frames leave in PTS order.
    queue: FrameQueue,
}

impl std::fmt::Debug for MediaView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MediaView")
            .field("surface", &self.surface)
            .field("fit", &self.fit)
            .field("explicit_aspect_ratio", &self.explicit_aspect_ratio)
            .field("cached_bounds", &self.cached_bounds)
            .field("cached_video_rect", &self.cached_video_rect)
            .field("decoder", &self.decoder.is_some())
            .field("queue_len", &self.queue.len())
            .finish()
    }
}

impl Default for MediaView {
    fn default() -> Self {
        Self::new()
    }
}

impl MediaView {
    /// Creates a new, empty `MediaView` widget.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::media::MediaView;
    ///
    /// let view = MediaView::new();
    /// assert!(view.surface().is_none());
    /// ```
    #[inline]
    #[must_use]
    pub fn new() -> Self {
        Self {
            surface: None,
            fit: VideoFit::default(),
            explicit_aspect_ratio: None,
            cached_bounds: Rect::default(),
            cached_video_rect: Rect::default(),
            decoder: None,
            queue: FrameQueue::new(3),
        }
    }

    /// Attaches a decode producer and creates the presentation surface from
    /// its negotiated format and configured size.
    ///
    /// Packets are fed with [`feed_packet`](Self::feed_packet); each
    /// [`advance`](Self::advance) call drains decoded frames into the pacing
    /// queue and presents the frame whose PTS has been reached.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::media::MediaView;
    /// use martensite_media::decoder::{DecoderConfig, MockDecoder, VideoCodec, VideoDecoder};
    ///
    /// let dec = MockDecoder::init(DecoderConfig::new(VideoCodec::H264, 640, 360)).unwrap();
    /// let view = MediaView::new().with_decoder(Box::new(dec));
    /// assert!(view.decoder().is_some());
    /// ```
    #[must_use]
    pub fn with_decoder(mut self, decoder: Box<dyn VideoDecoder>) -> Self {
        self.set_decoder(decoder);
        self
    }

    /// Replaces the decode producer, recreating the presentation surface
    /// from the decoder's negotiated format and configured size.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::media::MediaView;
    /// use martensite_media::decoder::{DecoderConfig, MockDecoder, VideoCodec, VideoDecoder};
    ///
    /// let dec = MockDecoder::init(DecoderConfig::new(VideoCodec::H264, 320, 240)).unwrap();
    /// let mut view = MediaView::new();
    /// view.set_decoder(Box::new(dec));
    /// assert!(view.decoder().is_some());
    /// ```
    pub fn set_decoder(&mut self, decoder: Box<dyn VideoDecoder>) {
        let format = decoder.negotiated_format();
        // The surface starts as a zero-sized mock; the first decoded frame
        // re-dimensions it via `update_handle`'s metadata.
        self.surface
            .get_or_insert_with(|| VideoSurface::new_mock(2, 2, format));
        self.queue.clear();
        self.decoder = Some(decoder);
    }

    /// Returns a shared reference to the decode producer, if attached.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::media::MediaView;
    ///
    /// assert!(MediaView::new().decoder().is_none());
    /// ```
    #[must_use]
    pub fn decoder(&self) -> Option<&dyn VideoDecoder> {
        self.decoder.as_deref()
    }

    /// Returns an exclusive reference to the decode producer, if attached.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::media::MediaView;
    ///
    /// assert!(MediaView::new().decoder_mut().is_none());
    /// ```
    #[must_use]
    pub fn decoder_mut(&mut self) -> Option<&mut (dyn VideoDecoder + 'static)> {
        self.decoder.as_deref_mut()
    }

    /// Feeds one compressed packet to the attached decoder.
    ///
    /// Returns `Ok(false)` when no decoder is attached; `Ok(true)` when the
    /// packet was accepted.
    ///
    /// # Errors
    ///
    /// Propagates decoder errors (corrupt stream, keyframe violations,
    /// fatal backend faults).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::media::MediaView;
    /// use martensite_media::decoder::{DecoderConfig, EncodedPacket, MockDecoder, VideoCodec, VideoDecoder};
    ///
    /// let dec = MockDecoder::init(DecoderConfig::new(VideoCodec::H264, 64, 64)).unwrap();
    /// let mut view = MediaView::new().with_decoder(Box::new(dec));
    /// assert!(view.feed_packet(&EncodedPacket::new(vec![0x67], 0, 16_666_667)).unwrap());
    /// ```
    pub fn feed_packet(
        &mut self,
        packet: &EncodedPacket,
    ) -> Result<bool, martensite_media::surface::MediaError> {
        match self.decoder.as_mut() {
            None => Ok(false),
            Some(dec) => {
                dec.send_packet(packet)?;
                Ok(true)
            }
        }
    }

    /// Signals the attached decoder that no further packets will arrive,
    /// releasing every frame still held in its reorder buffer.
    ///
    /// Returns `Ok(false)` when no decoder is attached. Call this once the
    /// producer reaches end-of-stream, then keep pumping
    /// [`advance`](Self::advance) until the queue is empty.
    ///
    /// # Errors
    ///
    /// Propagates backend drain errors.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::media::MediaView;
    ///
    /// assert_eq!(MediaView::new().end_of_stream().unwrap(), false);
    /// ```
    pub fn end_of_stream(&mut self) -> Result<bool, martensite_media::surface::MediaError> {
        match self.decoder.as_mut() {
            None => Ok(false),
            Some(dec) => {
                dec.end_of_stream()?;
                Ok(true)
            }
        }
    }

    /// Advances the pipeline at `now_nanos`: drains decoded frames into the
    /// pacing queue, then presents the frame whose PTS has been reached by
    /// updating the [`VideoSurface`] handle.
    ///
    /// Returns `true` when a new frame was presented and the widget needs a
    /// repaint. Returns `false` when waiting (queue early or decoder starved).
    /// The returned [`Option<u64>`] via [`next_wait_nanos`](Self::next_wait_nanos)
    /// tells the event loop how long it can sleep.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::media::MediaView;
    /// use martensite_media::decoder::{DecoderConfig, EncodedPacket, MockDecoder, VideoCodec, VideoDecoder};
    ///
    /// let dec = MockDecoder::init(DecoderConfig::new(VideoCodec::H264, 64, 64)).unwrap();
    /// let mut view = MediaView::new().with_decoder(Box::new(dec));
    /// view.feed_packet(&EncodedPacket::new(vec![0x67], 0, 16_666_667)).unwrap();
    /// assert!(view.advance(0));
    /// ```
    pub fn advance(&mut self, now_nanos: u64) -> bool {
        // Drain the decoder into the pacing queue.
        if let Some(dec) = self.decoder.as_mut() {
            while let Ok(Some(frame)) = dec.try_recv_frame() {
                self.queue.push(frame);
            }
        }

        match self.queue.pop_present(now_nanos) {
            QueueAction::Present(frame) => {
                let surface = self
                    .surface
                    .get_or_insert_with(|| VideoSurface::new_mock(2, 2, frame.metadata.format));
                surface.update_handle(frame.handle, frame.metadata.pts_nanos);
                surface.metadata_mut().width = frame.metadata.width;
                surface.metadata_mut().height = frame.metadata.height;
                surface.metadata_mut().format = frame.metadata.format;
                surface.metadata_mut().range = frame.metadata.range;
                surface.metadata_mut().duration_nanos = frame.metadata.duration_nanos;
                true
            }
            QueueAction::WaitFor { .. } | QueueAction::Empty => false,
        }
    }

    /// Nanoseconds until the next queued frame's PTS, if the head frame is
    /// early. Returns `None` when the queue is empty or a frame is due.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::media::MediaView;
    ///
    /// assert_eq!(MediaView::new().next_wait_nanos(0), None);
    /// ```
    #[must_use]
    pub fn next_wait_nanos(&self, now_nanos: u64) -> Option<u64> {
        let pts = self.queue.next_pts_nanos()?;
        (pts > now_nanos).then_some(pts - now_nanos)
    }

    /// Percentage of frames dropped by the pacing queue (the `< 0.1%`
    /// milestone gate reads this).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::media::MediaView;
    ///
    /// assert_eq!(MediaView::new().drop_rate_pct(), 0.0);
    /// ```
    #[must_use]
    pub fn drop_rate_pct(&self) -> f64 {
        self.queue.drop_rate_pct()
    }

    /// Number of decoded frames waiting in the pacing queue.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::media::MediaView;
    ///
    /// assert_eq!(MediaView::new().queued_frames(), 0);
    /// ```
    #[must_use]
    pub fn queued_frames(&self) -> usize {
        self.queue.len()
    }

    /// Attaches an active hardware [`VideoSurface`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::media::MediaView;
    /// use martensite_media::surface::{VideoPixelFormat, VideoSurface};
    ///
    /// let surface = VideoSurface::new_mock(3840, 2160, VideoPixelFormat::P010);
    /// let view = MediaView::new().with_surface(surface);
    /// assert!(view.surface().is_some());
    /// ```
    #[inline]
    #[must_use]
    pub fn with_surface(mut self, surface: VideoSurface) -> Self {
        self.surface = Some(surface);
        self
    }

    /// Configures the video scaling fit mode.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::media::{MediaView, VideoFit};
    ///
    /// let view = MediaView::new().with_fit(VideoFit::Cover);
    /// assert_eq!(view.fit(), VideoFit::Cover);
    /// ```
    #[inline]
    #[must_use]
    pub fn with_fit(mut self, fit: VideoFit) -> Self {
        self.fit = fit;
        self
    }

    /// Sets an explicit aspect ratio (width / height) overriding native frame dimensions.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::media::MediaView;
    ///
    /// let view = MediaView::new().with_aspect_ratio(16.0 / 9.0);
    /// assert_eq!(view.explicit_aspect_ratio(), Some(16.0 / 9.0));
    /// ```
    #[inline]
    #[must_use]
    pub fn with_aspect_ratio(mut self, ratio: f32) -> Self {
        self.explicit_aspect_ratio = if ratio.is_finite() && ratio > 0.0 {
            Some(ratio.clamp(0.001, 1000.0))
        } else {
            None
        };
        self
    }

    /// Returns the currently active [`VideoFit`] mode.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::media::{MediaView, VideoFit};
    ///
    /// let view = MediaView::new().with_fit(VideoFit::Cover);
    /// assert_eq!(view.fit(), VideoFit::Cover);
    /// ```
    #[inline]
    #[must_use]
    pub fn fit(&self) -> VideoFit {
        self.fit
    }

    /// Returns the explicit aspect ratio override, if configured.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::media::MediaView;
    ///
    /// let view = MediaView::new().with_aspect_ratio(4.0 / 3.0);
    /// assert_eq!(view.explicit_aspect_ratio(), Some(4.0 / 3.0));
    /// ```
    #[inline]
    #[must_use]
    pub fn explicit_aspect_ratio(&self) -> Option<f32> {
        self.explicit_aspect_ratio
    }

    /// Returns a shared reference to the attached [`VideoSurface`], if any.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::media::MediaView;
    /// use martensite_media::surface::{VideoPixelFormat, VideoSurface};
    ///
    /// let surface = VideoSurface::new_mock(1920, 1080, VideoPixelFormat::Nv12);
    /// let view = MediaView::new().with_surface(surface);
    /// assert!(view.surface().is_some());
    /// ```
    #[inline]
    #[must_use]
    pub fn surface(&self) -> Option<&VideoSurface> {
        self.surface.as_ref()
    }

    /// Returns an exclusive reference to the attached [`VideoSurface`], if any.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::media::MediaView;
    /// use martensite_media::surface::{VideoPixelFormat, VideoSurface};
    ///
    /// let surface = VideoSurface::new_mock(1920, 1080, VideoPixelFormat::Nv12);
    /// let mut view = MediaView::new().with_surface(surface);
    /// assert!(view.surface_mut().is_some());
    /// ```
    #[inline]
    #[must_use]
    pub fn surface_mut(&mut self) -> Option<&mut VideoSurface> {
        self.surface.as_mut()
    }

    /// Replaces or updates the attached [`VideoSurface`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::media::MediaView;
    /// use martensite_media::surface::{VideoPixelFormat, VideoSurface};
    ///
    /// let mut view = MediaView::new();
    /// let surface = VideoSurface::new_mock(1280, 720, VideoPixelFormat::Nv12);
    /// view.set_surface(surface);
    /// assert!(view.surface().is_some());
    /// ```
    #[inline]
    pub fn set_surface(&mut self, surface: VideoSurface) {
        self.surface = Some(surface);
    }

    /// Returns the cached outer layout bounds from the last layout pass.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::media::MediaView;
    ///
    /// let view = MediaView::new();
    /// assert_eq!(view.cached_bounds().size.x, 0.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn cached_bounds(&self) -> Rect {
        self.cached_bounds
    }

    /// Returns the destination rectangle of the video stream within the outer bounds,
    /// taking letterboxing/pillarboxing into account.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::media::MediaView;
    ///
    /// let view = MediaView::new();
    /// assert_eq!(view.cached_video_rect().size.x, 0.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn cached_video_rect(&self) -> Rect {
        self.cached_video_rect
    }

    /// Resolves the effective aspect ratio (width / height).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::media::MediaView;
    /// use martensite_media::surface::{VideoPixelFormat, VideoSurface};
    ///
    /// let surface = VideoSurface::new_mock(1920, 1080, VideoPixelFormat::Nv12);
    /// let view = MediaView::new().with_surface(surface);
    /// assert!((view.effective_aspect_ratio() - 16.0 / 9.0).abs() < 1e-4);
    /// ```
    #[must_use]
    pub fn effective_aspect_ratio(&self) -> f32 {
        if let Some(explicit) = self.explicit_aspect_ratio {
            return explicit;
        }
        if let Some(ref s) = self.surface {
            let (w, h) = s.dimensions();
            if w > 0 && h > 0 {
                return w as f32 / h as f32;
            }
        }
        16.0 / 9.0 // default widescreen aspect ratio
    }

    /// Calculates the video destination rectangle within layout bounds for a given fit mode.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::media::{MediaView, VideoFit};
    /// use martensite_core::node::Rect;
    ///
    /// let view = MediaView::new().with_aspect_ratio(16.0 / 9.0);
    /// let bounds = Rect::new(0.0, 0.0, 1000.0, 1000.0);
    /// let rect = view.compute_dest_rect(bounds, VideoFit::Contain);
    /// assert_eq!(rect.origin.x, 0.0);
    /// ```
    #[must_use]
    pub fn compute_dest_rect(&self, bounds: Rect, fit: VideoFit) -> Rect {
        let aspect = self.effective_aspect_ratio();
        let b_w = bounds.size.x.max(0.0);
        let b_h = bounds.size.y.max(0.0);

        if b_w <= 0.0 || b_h <= 0.0 {
            return bounds;
        }

        let bounds_aspect = b_w / b_h;

        match fit {
            VideoFit::Contain => {
                let (v_w, v_h) = if bounds_aspect > aspect {
                    (b_h * aspect, b_h)
                } else {
                    (b_w, b_w / aspect)
                };
                let offset_x = bounds.origin.x + (b_w - v_w) * 0.5;
                let offset_y = bounds.origin.y + (b_h - v_h) * 0.5;
                Rect::new(offset_x, offset_y, v_w, v_h)
            }
            VideoFit::Cover => {
                let (v_w, v_h) = if bounds_aspect > aspect {
                    (b_w, b_w / aspect)
                } else {
                    (b_h * aspect, b_h)
                };
                let offset_x = bounds.origin.x + (b_w - v_w) * 0.5;
                let offset_y = bounds.origin.y + (b_h - v_h) * 0.5;
                Rect::new(offset_x, offset_y, v_w, v_h)
            }
            VideoFit::Fill => bounds,
            VideoFit::Fixed => {
                if let Some(ref s) = self.surface {
                    let (w, h) = s.dimensions();
                    let v_w = w as f32;
                    let v_h = h as f32;
                    let offset_x = bounds.origin.x + (b_w - v_w) * 0.5;
                    let offset_y = bounds.origin.y + (b_h - v_h) * 0.5;
                    Rect::new(offset_x, offset_y, v_w, v_h)
                } else {
                    bounds
                }
            }
        }
    }
}

impl Widget for MediaView {
    fn measure(&mut self, _cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let aspect = self.effective_aspect_ratio();
        let max_w = constraints.max_size.x;
        let max_h = constraints.max_size.y;

        let measured = if max_w.is_finite() && max_h.is_finite() {
            if max_w / max_h > aspect {
                Vec2::new(max_h * aspect, max_h)
            } else {
                Vec2::new(max_w, max_w / aspect)
            }
        } else if max_w.is_finite() {
            Vec2::new(max_w, max_w / aspect)
        } else if max_h.is_finite() {
            Vec2::new(max_h * aspect, max_h)
        } else {
            Vec2::new(640.0, 640.0 / aspect)
        };

        measured.clamp(constraints.min_size, constraints.max_size)
    }

    fn layout(&mut self, _cx: &mut LayoutContext, bounds: Rect) {
        self.cached_bounds = bounds;
        self.cached_video_rect = self.compute_dest_rect(bounds, self.fit);
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Video);
    }

    fn paint(&self, cx: &mut PaintContext) {
        // Video frames present via the hardware overlay / `VideoProcessor`
        // compute path, keyed off the attached `VideoSurface` handle. Paint
        // a black backdrop under the video rect so the letterbox region and
        // any pre-first-frame state are not transparent.
        if self.surface.is_some() {
            let r = self.cached_video_rect;
            if r.size.x > 0.0 && r.size.y > 0.0 {
                cx.list.push_fill_rect(
                    kurbo::Rect::new(
                        f64::from(r.origin.x),
                        f64::from(r.origin.y),
                        f64::from(r.max_x()),
                        f64::from(r.max_y()),
                    ),
                    [0, 0, 0, 255],
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;
    use martensite_media::surface::VideoPixelFormat;

    #[test]
    fn media_view_aspect_ratio_contain() {
        let surface = VideoSurface::new_mock(1920, 1080, VideoPixelFormat::Nv12);
        let mut view = MediaView::new()
            .with_surface(surface)
            .with_fit(VideoFit::Contain);

        let mut hot = HotNode::new(taffy::NodeId::new(1));
        let mut cx = LayoutContext { hot: &mut hot };

        // Measure in a 1000x1000 square container
        let constraints = LayoutConstraints {
            min_size: Vec2::ZERO,
            max_size: Vec2::new(1000.0, 1000.0),
        };
        let size = view.measure(&mut cx, constraints);
        // 16:9 in 1000x1000 -> 1000 x 562.5
        assert_eq!(size.x, 1000.0);
        assert!((size.y - 562.5).abs() < 1e-3);

        // Layout in 1000x1000 bounds
        view.layout(&mut cx, Rect::new(0.0, 0.0, 1000.0, 1000.0));
        let video_rect = view.cached_video_rect();
        assert_eq!(video_rect.size.x, 1000.0);
        assert!((video_rect.size.y - 562.5).abs() < 1e-3);
        // Vertically centered: (1000 - 562.5) / 2 = 218.75
        assert!((video_rect.origin.y - 218.75).abs() < 1e-3);
    }

    #[test]
    fn media_view_fill() {
        let surface = VideoSurface::new_mock(1920, 1080, VideoPixelFormat::Nv12);
        let mut view = MediaView::new()
            .with_surface(surface)
            .with_fit(VideoFit::Fill);

        let mut hot = HotNode::new(taffy::NodeId::new(1));
        let mut cx = LayoutContext { hot: &mut hot };

        view.layout(&mut cx, Rect::new(10.0, 20.0, 800.0, 600.0));
        assert_eq!(
            view.cached_video_rect(),
            Rect::new(10.0, 20.0, 800.0, 600.0)
        );
    }
}
