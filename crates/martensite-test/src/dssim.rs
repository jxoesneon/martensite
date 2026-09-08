//! Perceptual image difference metric based on a simplified DSSIM.
//!
//! This module implements a self-contained, dependency-free structural
//! similarity metric so that the headless test harness can compare rendered
//! frames against golden reference images without pulling in external image
//! processing crates.
//!
//! The metric is a simplified DSSIM (Dissimilarity = `1 - SSIM`) computed over
//! 8×8 pixel blocks. Two identical images yield a DSSIM of `0.0`; the more the
//! images differ structurally, the closer the value approaches `1.0`.

/// The side length (in pixels) of the local window used to compute SSIM
/// statistics.
pub const BLOCK_SIZE: u32 = 8;

/// Dynamic range of an 8-bit grayscale channel.
const L: f64 = 255.0;
/// SSIM stability constant for luminance, `C1 = (k1 * L)^2`.
const C1: f64 = (0.01 * L) * (0.01 * L);
/// SSIM stability constant for contrast/structure, `C2 = (k2 * L)^2`.
const C2: f64 = (0.03 * L) * (0.03 * L);

/// A grayscale image buffer for snapshot comparison.
///
/// The buffer stores one byte per pixel with values in the range `0..=255`.
/// Pixel `(x, y)` is stored at index `y * width + x`, i.e. row-major order.
///
/// # Examples
///
/// ```
/// use martensite_test::dssim::ImageBuffer;
///
/// let mut img = ImageBuffer::new(2, 2);
/// img.set(0, 0, 10);
/// img.set(1, 1, 20);
/// assert_eq!(img.get(0, 0), Some(10));
/// assert_eq!(img.get(1, 1), Some(20));
/// assert_eq!(img.get(2, 0), None); // out of bounds
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageBuffer {
    /// The width of the image in pixels.
    pub width: u32,
    /// The height of the image in pixels.
    pub height: u32,
    /// The grayscale pixel data, row-major, length `width * height`.
    pub pixels: Vec<u8>,
}

impl ImageBuffer {
    /// Creates a new, fully black image buffer of the given dimensions.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_test::dssim::ImageBuffer;
    ///
    /// let img = ImageBuffer::new(4, 3);
    /// assert_eq!(img.width, 4);
    /// assert_eq!(img.height, 3);
    /// assert_eq!(img.pixels.len(), 12);
    /// assert!(img.pixels.iter().all(|&p| p == 0));
    /// ```
    pub fn new(width: u32, height: u32) -> Self {
        let len = (width as usize) * (height as usize);
        Self {
            width,
            height,
            pixels: vec![0; len],
        }
    }

    /// Converts an RGBA byte buffer to grayscale using Rec. 601 luma weights.
    ///
    /// `rgba` must contain exactly `width * height * 4` bytes. The alpha
    /// channel is ignored for luminance purposes. Panics if the buffer
    /// size does not match the expected dimensions.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_test::dssim::ImageBuffer;
    ///
    /// // A single red pixel.
    /// let img = ImageBuffer::from_rgba(1, 1, &[255, 0, 0, 255]);
    /// // 0.299 * 255 ≈ 76.
    /// assert_eq!(img.get(0, 0), Some(76));
    /// ```
    ///
    /// # Panics
    ///
    /// Panics if `rgba.len() != width * height * 4`.
    pub fn from_rgba(width: u32, height: u32, rgba: &[u8]) -> Self {
        let mut img = Self::new(width, height);
        let expected = (width as usize) * (height as usize) * 4;
        assert_eq!(
            rgba.len(),
            expected,
            "RGBA buffer size mismatch: expected {expected} bytes, got {}",
            rgba.len()
        );
        let pixel_count = (width as usize) * (height as usize);
        for i in 0..pixel_count {
            let r = rgba[i * 4] as f64;
            let g = rgba[i * 4 + 1] as f64;
            let b = rgba[i * 4 + 2] as f64;
            let luma = 0.299 * r + 0.587 * g + 0.114 * b;
            img.pixels[i] = luma.round().clamp(0.0, 255.0) as u8;
        }
        img
    }

    /// Returns the grayscale value at `(x, y)`, or `None` if out of bounds.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_test::dssim::ImageBuffer;
    ///
    /// let img = ImageBuffer::new(1, 1);
    /// assert_eq!(img.get(0, 0), Some(0));
    /// assert_eq!(img.get(1, 0), None);
    /// ```
    pub fn get(&self, x: u32, y: u32) -> Option<u8> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let idx = (y as usize) * (self.width as usize) + (x as usize);
        self.pixels.get(idx).copied()
    }

    /// Sets the grayscale value at `(x, y)`. Out-of-bounds writes are ignored.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_test::dssim::ImageBuffer;
    ///
    /// let mut img = ImageBuffer::new(2, 1);
    /// img.set(0, 0, 128);
    /// assert_eq!(img.get(0, 0), Some(128));
    /// // Out-of-bounds write is a no-op.
    /// img.set(5, 5, 200);
    /// ```
    pub fn set(&mut self, x: u32, y: u32, value: u8) {
        if x >= self.width || y >= self.height {
            return;
        }
        let idx = (y as usize) * (self.width as usize) + (x as usize);
        self.pixels[idx] = value;
    }

    /// Fills the entire buffer with a single grayscale value.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_test::dssim::ImageBuffer;
    ///
    /// let mut img = ImageBuffer::new(3, 2);
    /// img.fill(200);
    /// assert!(img.pixels.iter().all(|&p| p == 200));
    /// ```
    pub fn fill(&mut self, value: u8) {
        self.pixels.fill(value);
    }

    /// Returns the raw grayscale pixel data as a byte slice.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_test::dssim::ImageBuffer;
    ///
    /// let img = ImageBuffer::new(2, 1);
    /// assert_eq!(img.as_slice().len(), 2);
    /// ```
    pub fn as_slice(&self) -> &[u8] {
        &self.pixels
    }

    /// Returns the number of pixels in the buffer.
    fn len(&self) -> usize {
        self.pixels.len()
    }
}

/// Computes the per-block SSIM contribution for a single window of pixels
/// drawn from `a` and `b`.
fn block_ssim(a: &[u8], b: &[u8]) -> f64 {
    let n = a.len() as f64;
    if n == 0.0 {
        return 1.0;
    }

    let mut sum_a = 0.0f64;
    let mut sum_b = 0.0f64;
    for (pa, pb) in a.iter().zip(b.iter()) {
        sum_a += *pa as f64;
        sum_b += *pb as f64;
    }
    let mu_a = sum_a / n;
    let mu_b = sum_b / n;

    let mut var_a = 0.0f64;
    let mut var_b = 0.0f64;
    let mut cov = 0.0f64;
    for (pa, pb) in a.iter().zip(b.iter()) {
        let da = *pa as f64 - mu_a;
        let db = *pb as f64 - mu_b;
        var_a += da * da;
        var_b += db * db;
        cov += da * db;
    }
    // Population variance/covariance (divide by n).
    let sigma_a2 = var_a / n;
    let sigma_b2 = var_b / n;
    let sigma_ab = cov / n;

    let numerator = (2.0 * mu_a * mu_b + C1) * (2.0 * sigma_ab + C2);
    let denominator = (mu_a * mu_a + mu_b * mu_b + C1) * (sigma_a2 + sigma_b2 + C2);
    numerator / denominator
}

/// Computes the DSSIM (`1 - SSIM`) between two image buffers.
///
/// Returns a value in `[0.0, 1.0]` where `0.0` means the images are identical
/// and `1.0` means they are completely different. Local statistics are
/// computed over [`BLOCK_SIZE`]×[`BLOCK_SIZE`] windows; images smaller than a
/// single block in either dimension are compared as one whole-image window.
///
/// Images of differing dimensions are treated as maximally different
/// (`1.0`).
///
/// # Examples
///
/// ```
/// use martensite_test::dssim::{dssim, ImageBuffer};
///
/// let a = ImageBuffer::new(8, 8);
/// let mut b = ImageBuffer::new(8, 8);
/// b.fill(255);
/// let score = dssim(&a, &b);
/// assert!(score > 0.0 && score <= 1.0);
/// ```
pub fn dssim(a: &ImageBuffer, b: &ImageBuffer) -> f64 {
    // Differing dimensions are a total mismatch.
    if a.width != b.width || a.height != b.height {
        return 1.0;
    }
    if a.len() == 0 {
        // Two empty images are identical.
        return 0.0;
    }

    // If the image is smaller than a single block in either dimension, fall
    // back to a single whole-image window.
    if a.width < BLOCK_SIZE || a.height < BLOCK_SIZE {
        let ssim = block_ssim(&a.pixels, &b.pixels);
        return (1.0 - ssim).clamp(0.0, 1.0);
    }

    let block = BLOCK_SIZE as usize;
    let w = a.width as usize;
    let h = a.height as usize;
    let mut ssim_sum = 0.0f64;
    let mut block_count = 0u64;

    let mut by = 0usize;
    while by < h {
        let bh = (h - by).min(block);
        let mut bx = 0usize;
        while bx < w {
            let bw = (w - bx).min(block);
            // Collect the window pixels for both images.
            let mut win_a = Vec::with_capacity(bw * bh);
            let mut win_b = Vec::with_capacity(bw * bh);
            for row in 0..bh {
                let start = (by + row) * w + bx;
                win_a.extend_from_slice(&a.pixels[start..start + bw]);
                win_b.extend_from_slice(&b.pixels[start..start + bw]);
            }
            ssim_sum += block_ssim(&win_a, &win_b);
            block_count += 1;
            bx += block;
        }
        by += block;
    }

    let mean_ssim = ssim_sum / block_count as f64;
    (1.0 - mean_ssim).clamp(0.0, 1.0)
}

/// Compares two images and returns whether they are perceptually equivalent.
///
/// `threshold` is the maximum acceptable DSSIM value. Typical values for
/// "pixel-perfect" matching are in the range `0.001..=0.01`.
///
/// # Examples
///
/// ```
/// use martensite_test::dssim::{images_match, ImageBuffer};
///
/// let a = ImageBuffer::new(8, 8);
/// let b = ImageBuffer::new(8, 8);
/// assert!(images_match(&a, &b, 0.001));
/// ```
pub fn images_match(a: &ImageBuffer, b: &ImageBuffer, threshold: f64) -> bool {
    dssim(a, b) <= threshold
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_buffer_is_black() {
        let img = ImageBuffer::new(4, 3);
        assert_eq!(img.width, 4);
        assert_eq!(img.height, 3);
        assert_eq!(img.pixels.len(), 12);
        assert!(img.pixels.iter().all(|&p| p == 0));
    }

    #[test]
    fn get_set_roundtrip() {
        let mut img = ImageBuffer::new(3, 2);
        img.set(0, 0, 11);
        img.set(2, 1, 22);
        assert_eq!(img.get(0, 0), Some(11));
        assert_eq!(img.get(2, 1), Some(22));
        assert_eq!(img.get(1, 0), Some(0));
    }

    #[test]
    fn get_out_of_bounds() {
        let img = ImageBuffer::new(2, 2);
        assert_eq!(img.get(2, 0), None);
        assert_eq!(img.get(0, 2), None);
    }

    #[test]
    fn set_out_of_bounds_ignored() {
        let mut img = ImageBuffer::new(2, 2);
        img.set(5, 5, 99);
        assert!(img.pixels.iter().all(|&p| p == 0));
    }

    #[test]
    fn fill_sets_all_pixels() {
        let mut img = ImageBuffer::new(3, 3);
        img.fill(123);
        assert!(img.pixels.iter().all(|&p| p == 123));
    }

    #[test]
    fn as_slice_returns_pixels() {
        let mut img = ImageBuffer::new(2, 1);
        img.set(0, 0, 10);
        img.set(1, 0, 20);
        assert_eq!(img.as_slice(), &[10, 20]);
    }

    #[test]
    fn from_rgba_grayscale_red() {
        let img = ImageBuffer::from_rgba(1, 1, &[255, 0, 0, 255]);
        // 0.299 * 255 = 76.245 -> 76
        assert_eq!(img.get(0, 0), Some(76));
    }

    #[test]
    fn from_rgba_grayscale_green() {
        let img = ImageBuffer::from_rgba(1, 1, &[0, 255, 0, 255]);
        // 0.587 * 255 = 149.685 -> 150
        assert_eq!(img.get(0, 0), Some(150));
    }

    #[test]
    fn from_rgba_grayscale_blue() {
        let img = ImageBuffer::from_rgba(1, 1, &[0, 0, 255, 255]);
        // 0.114 * 255 = 29.07 -> 29
        assert_eq!(img.get(0, 0), Some(29));
    }

    #[test]
    fn from_rgba_grayscale_white() {
        let img = ImageBuffer::from_rgba(1, 1, &[255, 255, 255, 255]);
        assert_eq!(img.get(0, 0), Some(255));
    }

    #[test]
    fn from_rgba_grayscale_black() {
        let img = ImageBuffer::from_rgba(1, 1, &[0, 0, 0, 255]);
        assert_eq!(img.get(0, 0), Some(0));
    }

    #[test]
    fn from_rgba_multiple_pixels() {
        let rgba = [255, 0, 0, 255, 0, 255, 0, 255];
        let img = ImageBuffer::from_rgba(2, 1, &rgba);
        assert_eq!(img.get(0, 0), Some(76));
        assert_eq!(img.get(1, 0), Some(150));
    }

    #[test]
    fn from_rgba_size_mismatch_panics() {
        // Only 2 bytes provided for a 1x1 image (needs 4). Should panic.
        let result = std::panic::catch_unwind(|| {
            ImageBuffer::from_rgba(1, 1, &[255, 0]);
        });
        assert!(result.is_err(), "should panic on size mismatch");
    }

    #[test]
    fn dssim_identical_images_is_zero() {
        let a = ImageBuffer::new(16, 16);
        let b = ImageBuffer::new(16, 16);
        assert_eq!(dssim(&a, &b), 0.0);
    }

    #[test]
    fn dssim_empty_images_is_zero() {
        let a = ImageBuffer::new(0, 0);
        let b = ImageBuffer::new(0, 0);
        assert_eq!(dssim(&a, &b), 0.0);
    }

    #[test]
    fn dssim_differing_dimensions_is_one() {
        let a = ImageBuffer::new(8, 8);
        let b = ImageBuffer::new(16, 16);
        assert_eq!(dssim(&a, &b), 1.0);
    }

    #[test]
    fn dssim_opposite_images_is_positive() {
        let a = ImageBuffer::new(16, 16);
        let mut b = ImageBuffer::new(16, 16);
        b.fill(255);
        let score = dssim(&a, &b);
        assert!(score > 0.0, "expected positive DSSIM, got {score}");
        assert!(score <= 1.0);
    }

    #[test]
    fn dssim_small_image_uses_whole_window() {
        let mut a = ImageBuffer::new(3, 3);
        a.fill(100);
        let mut b = ImageBuffer::new(3, 3);
        b.fill(100);
        assert_eq!(dssim(&a, &b), 0.0);
    }

    #[test]
    fn dssim_is_symmetric() {
        let mut a = ImageBuffer::new(16, 16);
        for y in 0..16 {
            for x in 0..16 {
                a.set(x, y, ((x + y) % 256) as u8);
            }
        }
        let mut b = ImageBuffer::new(16, 16);
        for y in 0..16 {
            for x in 0..16 {
                b.set(x, y, ((x * y) % 256) as u8);
            }
        }
        let ab = dssim(&a, &b);
        let ba = dssim(&b, &a);
        assert!((ab - ba).abs() < 1e-12);
    }

    #[test]
    fn dssim_in_range_for_partial_image() {
        // Non-multiple of block size in both dimensions.
        let mut a = ImageBuffer::new(10, 10);
        let mut b = ImageBuffer::new(10, 10);
        a.fill(50);
        b.fill(60);
        let score = dssim(&a, &b);
        assert!((0.0..=1.0).contains(&score));
    }

    #[test]
    fn dssim_single_pixel_difference_small() {
        let mut a = ImageBuffer::new(16, 16);
        a.fill(128);
        let mut b = ImageBuffer::new(16, 16);
        b.fill(128);
        b.set(0, 0, 129);
        let score = dssim(&a, &b);
        // A single-pixel change of 1 should be extremely small.
        assert!(score < 0.001, "expected tiny DSSIM, got {score}");
    }

    #[test]
    fn images_match_identical_passes() {
        let a = ImageBuffer::new(8, 8);
        let b = ImageBuffer::new(8, 8);
        assert!(images_match(&a, &b, 0.0));
    }

    #[test]
    fn images_match_differing_dimensions_fails() {
        let a = ImageBuffer::new(8, 8);
        let b = ImageBuffer::new(16, 16);
        assert!(!images_match(&a, &b, 0.01));
    }

    #[test]
    fn images_match_within_threshold() {
        let mut a = ImageBuffer::new(16, 16);
        let mut b = ImageBuffer::new(16, 16);
        a.fill(128);
        b.fill(130);
        // Small difference should be within a loose threshold.
        assert!(images_match(&a, &b, 0.05));
    }

    #[test]
    fn images_match_outside_threshold() {
        let a = ImageBuffer::new(16, 16);
        let mut b = ImageBuffer::new(16, 16);
        b.fill(255);
        assert!(!images_match(&a, &b, 0.001));
    }

    #[test]
    fn block_ssim_identical_is_one() {
        let a = [10u8, 20, 30, 40];
        let s = block_ssim(&a, &a);
        assert!((s - 1.0).abs() < 1e-9);
    }

    #[test]
    fn block_ssim_empty_is_one() {
        let s = block_ssim(&[], &[]);
        assert!((s - 1.0).abs() < 1e-9);
    }
}
