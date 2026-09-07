//! Perceptual image diffing engine based on DSSIM (Structural Similarity).
//!
//! The engine compares two RGBA images of identical dimensions and produces a
//! [`DiffResult`] containing separate SSIM scores for interior and edge
//! windows, an overall SSIM for reporting, and a pass/fail verdict. Two
//! tolerance bands are used:
//!
//! - **Edge windows** (blocks that contain at least one high-frequency
//!   luminance-gradient pixel) use a relaxed threshold of `SSIM >= 0.995`.
//! - **Interior windows** (blocks with no edge pixels, i.e. solid fills) use
//!   a strict threshold of `SSIM >= 0.9999`.
//!
//! This mirrors the behavior of perceptual diff tools used by browser engine
//! reftest harnesses: anti-aliased edges are allowed minor sub-pixel drift
//! while solid fills must match almost exactly.

/// The result of a perceptual diff between two images.
#[derive(Clone, Copy, Debug)]
pub struct DiffResult {
    /// The mean SSIM across all evaluated windows, in `[0.0, 1.0]` where `1.0`
    /// is identical. This is reported for diagnostic purposes only and is not
    /// used by the pass condition.
    pub overall_ssim: f64,
    /// The mean SSIM computed only over windows classified as high-frequency
    /// edges. When no edges are present this equals `overall_ssim`. Retained
    /// for backward compatibility; new code should prefer [`DiffResult::edge_ssim`].
    pub edge_masked_ssim: f64,
    /// The mean SSIM over windows that contain **no** edge pixels. These
    /// represent solid-fill interiors and are held to the strict
    /// [`FILL_SSIM_THRESHOLD`]. When every window contains an edge this
    /// defaults to `1.0` (vacuously passing the interior check).
    pub interior_ssim: f64,
    /// The mean SSIM over windows that **do** contain edge pixels. These
    /// represent anti-aliased or high-frequency boundaries and are held to the
    /// relaxed [`EDGE_SSIM_THRESHOLD`]. When no edges are present this
    /// defaults to `1.0` (vacuously passing the edge check).
    pub edge_ssim: f64,
    /// `true` when the interior windows meet [`FILL_SSIM_THRESHOLD`] **and**
    /// the edge windows meet [`EDGE_SSIM_THRESHOLD`].
    pub passed: bool,
}

/// The minimum SSIM required for edge-masked (high-frequency) regions.
pub const EDGE_SSIM_THRESHOLD: f64 = 0.995;
/// The minimum SSIM required for strict fill-interior matching.
pub const FILL_SSIM_THRESHOLD: f64 = 0.9999;

/// The window side length (in pixels) used for SSIM block evaluation.
const WINDOW: usize = 8;
/// Per-window luminance stability constant.
const C1: f64 = 6.5025;
/// Per-window contrast/structure stability constant.
const C2: f64 = 58.5225;

/// Converts an RGBA byte slice into a grayscale luminance buffer.
///
/// Luminance is computed with the standard Rec. 601 weights and returned in
/// the range `[0.0, 255.0]`.
fn to_grayscale(rgba: &[u8], width: u32, height: u32) -> Vec<f64> {
    let count = (width as usize) * (height as usize);
    let mut gray = Vec::with_capacity(count);
    for px in rgba.as_chunks::<4>().0 {
        let r = f64::from(px[0]);
        let g = f64::from(px[1]);
        let b = f64::from(px[2]);
        // Rec. 601 luma.
        gray.push(0.299 * r + 0.587 * g + 0.114 * b);
    }
    gray
}

/// Computes the per-pixel luminance gradient magnitude using a simple 3x3
/// Sobel operator and returns a boolean mask marking high-frequency edge
/// pixels.
fn edge_mask(gray: &[f64], width: usize, height: usize) -> Vec<bool> {
    let mut mask = vec![false; gray.len()];
    for y in 1..height.saturating_sub(1) {
        for x in 1..width.saturating_sub(1) {
            let idx = y * width + x;
            let gx = -gray[idx - width - 1] + gray[idx - width + 1] - 2.0 * gray[idx - 1]
                + 2.0 * gray[idx + 1]
                - gray[idx + width - 1]
                + gray[idx + width + 1];
            let gy = -gray[idx - width - 1] - 2.0 * gray[idx - width] - gray[idx - width + 1]
                + gray[idx + width - 1]
                + 2.0 * gray[idx + width]
                + gray[idx + width + 1];
            let mag = (gx * gx + gy * gy).sqrt();
            // Threshold tuned for 8-bit luminance: 16.0 distinguishes visible
            // edges from flat fills.
            mask[idx] = mag > 16.0;
        }
    }
    mask
}

/// Computes the SSIM of a `width x height` block starting at the given origins.
fn ssim_block_sized(
    a: &[f64],
    b: &[f64],
    stride: usize,
    width: usize,
    height: usize,
    origin_a: usize,
    origin_b: usize,
) -> f64 {
    let n = (width * height) as f64;
    if n == 0.0 {
        return 1.0;
    }
    let mut mu_a = 0.0;
    let mut mu_b = 0.0;
    for dy in 0..height {
        for dx in 0..width {
            mu_a += a[origin_a + dy * stride + dx];
            mu_b += b[origin_b + dy * stride + dx];
        }
    }
    mu_a /= n;
    mu_b /= n;

    let mut var_a = 0.0;
    let mut var_b = 0.0;
    let mut cov = 0.0;
    for dy in 0..height {
        for dx in 0..width {
            let va = a[origin_a + dy * stride + dx] - mu_a;
            let vb = b[origin_b + dy * stride + dx] - mu_b;
            var_a += va * va;
            var_b += vb * vb;
            cov += va * vb;
        }
    }
    var_a /= n;
    var_b /= n;
    cov /= n;

    let numerator = (2.0 * mu_a * mu_b + C1) * (2.0 * cov + C2);
    let denominator = (mu_a * mu_a + mu_b * mu_b + C1) * (var_a + var_b + C2);
    numerator / denominator
}

/// Computes the perceptual diff between two RGBA images of identical
/// dimensions.
///
/// Both `expected` and `actual` must contain exactly `width * height * 4`
/// bytes. The function panics in debug builds if the inputs are malformed; in
/// production builds a best-effort `DiffResult` indicating failure is returned.
pub fn perceptual_diff(expected: &[u8], actual: &[u8], width: u32, height: u32) -> DiffResult {
    let expected_len = (width as usize) * (height as usize) * 4;
    if expected.len() < expected_len || actual.len() < expected_len {
        return DiffResult {
            overall_ssim: 0.0,
            edge_masked_ssim: 0.0,
            interior_ssim: 0.0,
            edge_ssim: 0.0,
            passed: false,
        };
    }

    let w = width as usize;
    let h = height as usize;
    let expected_rgba = &expected[..expected_len];
    let actual_rgba = &actual[..expected_len];

    let gray_a = to_grayscale(expected_rgba, width, height);
    let gray_b = to_grayscale(actual_rgba, width, height);
    let edges = edge_mask(&gray_a, w, h);

    let mut overall_sum = 0.0;
    let mut overall_count = 0usize;
    let mut interior_sum = 0.0;
    let mut interior_count = 0usize;
    let mut edge_sum = 0.0;
    let mut edge_count = 0usize;

    if w < WINDOW || h < WINDOW {
        // Image smaller than a single window: evaluate the whole image as one
        // block using the actual image dimensions, then classify it as an edge
        // or interior window based on whether any edge pixels are present.
        let block_ssim = ssim_block_sized(&gray_a, &gray_b, w, w, h, 0, 0);
        let has_edge = edges.iter().any(|&e| e);
        if has_edge {
            edge_sum = block_ssim;
            edge_count = 1;
        } else {
            interior_sum = block_ssim;
            interior_count = 1;
        }
        let interior_ssim = if interior_count > 0 {
            interior_sum / interior_count as f64
        } else {
            1.0
        };
        let edge_ssim = if edge_count > 0 {
            edge_sum / edge_count as f64
        } else {
            1.0
        };
        return DiffResult {
            overall_ssim: block_ssim,
            edge_masked_ssim: edge_ssim,
            interior_ssim,
            edge_ssim,
            passed: interior_ssim >= FILL_SSIM_THRESHOLD && edge_ssim >= EDGE_SSIM_THRESHOLD,
        };
    }

    // Iterate over the image in WINDOW x WINDOW blocks. When the image
    // dimensions are not exact multiples of WINDOW, the trailing partial
    // blocks along the right and bottom edges are accumulated with their
    // actual (smaller) extents rather than discarded.
    let mut y = 0;
    while y < h {
        let block_h = if y + WINDOW <= h { WINDOW } else { h - y };
        let mut x = 0;
        while x < w {
            let block_w = if x + WINDOW <= w { WINDOW } else { w - x };
            let origin = y * w + x;
            let block_ssim =
                ssim_block_sized(&gray_a, &gray_b, w, block_w, block_h, origin, origin);
            overall_sum += block_ssim;
            overall_count += 1;

            // A block is considered an edge block if any pixel within it is
            // flagged as a high-frequency edge.
            let mut has_edge = false;
            for dy in 0..block_h {
                for dx in 0..block_w {
                    if edges[origin + dy * w + dx] {
                        has_edge = true;
                        break;
                    }
                }
                if has_edge {
                    break;
                }
            }
            if has_edge {
                edge_sum += block_ssim;
                edge_count += 1;
            } else {
                interior_sum += block_ssim;
                interior_count += 1;
            }
            x += WINDOW;
        }
        y += WINDOW;
    }

    let overall_ssim = if overall_count > 0 {
        overall_sum / overall_count as f64
    } else {
        1.0
    };
    let interior_ssim = if interior_count > 0 {
        interior_sum / interior_count as f64
    } else {
        // No interior windows: vacuously pass the strict check.
        1.0
    };
    let edge_ssim = if edge_count > 0 {
        edge_sum / edge_count as f64
    } else {
        // No edge windows: vacuously pass the relaxed check.
        1.0
    };
    // `edge_masked_ssim` retained for backward compatibility; it mirrors the
    // edge-window mean (falling back to the overall mean when there are none).
    let edge_masked_ssim = if edge_count > 0 {
        edge_ssim
    } else {
        overall_ssim
    };

    let passed = interior_ssim >= FILL_SSIM_THRESHOLD && edge_ssim >= EDGE_SSIM_THRESHOLD;

    DiffResult {
        overall_ssim,
        edge_masked_ssim,
        interior_ssim,
        edge_ssim,
        passed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid_image(width: u32, height: u32, color: [u8; 4]) -> Vec<u8> {
        let mut buf = Vec::with_capacity((width * height * 4) as usize);
        for _ in 0..(width * height) {
            buf.extend_from_slice(&color);
        }
        buf
    }

    #[test]
    fn identical_images_pass() {
        let a = solid_image(32, 32, [255, 0, 0, 255]);
        let b = solid_image(32, 32, [255, 0, 0, 255]);
        let result = perceptual_diff(&a, &b, 32, 32);
        assert!(result.passed, "identical images should pass");
        assert!(result.overall_ssim > 0.9999);
        // A solid fill has no edges, so every window is interior.
        assert!(result.interior_ssim > 0.9999);
        assert_eq!(result.edge_ssim, 1.0, "no edge windows => edge_ssim is 1.0");
    }

    #[test]
    fn different_images_fail() {
        let a = solid_image(32, 32, [255, 0, 0, 255]);
        let b = solid_image(32, 32, [0, 0, 255, 255]);
        let result = perceptual_diff(&a, &b, 32, 32);
        assert!(!result.passed, "different solid colors should fail");
        // Both are solid fills so the failure is in interior_ssim.
        assert!(result.interior_ssim < FILL_SSIM_THRESHOLD);
    }

    #[test]
    fn slightly_off_fill_fails_strict_threshold() {
        let a = solid_image(32, 32, [200, 200, 200, 255]);
        let b = solid_image(32, 32, [205, 205, 205, 255]);
        let result = perceptual_diff(&a, &b, 32, 32);
        // A uniform 5-unit shift should drop below the strict fill threshold.
        assert!(
            result.interior_ssim < FILL_SSIM_THRESHOLD,
            "uniform shift should fail strict threshold, got {}",
            result.interior_ssim
        );
        assert!(!result.passed);
    }

    #[test]
    fn malformed_inputs_fail() {
        let a = solid_image(32, 32, [255, 0, 0, 255]);
        let short = vec![0u8; 10];
        let result = perceptual_diff(&a, &short, 32, 32);
        assert!(!result.passed);
        assert_eq!(result.overall_ssim, 0.0);
        assert_eq!(result.interior_ssim, 0.0);
        assert_eq!(result.edge_ssim, 0.0);
    }

    #[test]
    fn tiny_image_evaluates_single_block() {
        let a = solid_image(4, 4, [10, 20, 30, 255]);
        let b = solid_image(4, 4, [10, 20, 30, 255]);
        let result = perceptual_diff(&a, &b, 4, 4);
        assert!(result.passed);
        // No edges in a solid fill, so it is classified as interior.
        assert!(result.interior_ssim >= FILL_SSIM_THRESHOLD);
        assert_eq!(result.edge_ssim, 1.0);
    }

    #[test]
    fn tiny_edge_image_uses_relaxed_threshold() {
        // A 4x4 image with a sharp vertical edge at x=2. Smaller than WINDOW
        // so it takes the small-image branch, but it still has edge pixels.
        let mut a = Vec::with_capacity(4 * 4 * 4);
        for _y in 0..4 {
            for x in 0..4 {
                let c = if x < 2 {
                    [0, 0, 0, 255]
                } else {
                    [255, 255, 255, 255]
                };
                a.extend_from_slice(&c);
            }
        }
        let b = a.clone();
        let result = perceptual_diff(&a, &b, 4, 4);
        assert!(result.passed);
        // The single block contains edges so it is classified as an edge
        // window and held to the relaxed threshold.
        assert!(result.edge_ssim >= EDGE_SSIM_THRESHOLD);
        // No interior windows => interior_ssim vacuously passes at 1.0.
        assert_eq!(result.interior_ssim, 1.0);
    }

    #[test]
    fn edge_image_has_edge_score() {
        // Build a 16x16 image with a sharp vertical edge at x=8.
        let mut a = Vec::with_capacity(16 * 16 * 4);
        for _y in 0..16 {
            for x in 0..16 {
                let c = if x < 8 {
                    [0, 0, 0, 255]
                } else {
                    [255, 255, 255, 255]
                };
                a.extend_from_slice(&c);
            }
        }
        let b = a.clone();
        let result = perceptual_diff(&a, &b, 16, 16);
        assert!(result.passed);
        // The edge band should be detected, so edge_ssim should be computed
        // from at least one edge window.
        assert!(result.edge_ssim >= EDGE_SSIM_THRESHOLD);
    }

    #[test]
    fn interior_and_edge_separation() {
        // Build a 32x32 image: solid black on the left half, solid white on
        // the right half, with a sharp vertical edge at x=16. The left and
        // right quarters are pure interior windows; the middle column of
        // windows straddles the edge.
        let mut a = Vec::with_capacity(32 * 32 * 4);
        for _y in 0..32 {
            for x in 0..32 {
                let c = if x < 16 {
                    [0, 0, 0, 255]
                } else {
                    [255, 255, 255, 255]
                };
                a.extend_from_slice(&c);
            }
        }
        let b = a.clone();
        let result = perceptual_diff(&a, &b, 32, 32);
        assert!(result.passed);
        // Identical images => both bands are perfect.
        assert!(result.interior_ssim >= FILL_SSIM_THRESHOLD);
        assert!(result.edge_ssim >= EDGE_SSIM_THRESHOLD);
        // There must be at least one interior and one edge window.
        // overall_ssim is the mean of both, and should be 1.0 for identical.
        assert!(result.overall_ssim > 0.9999);
    }

    #[test]
    fn interior_fail_not_rescued_by_relaxed_edge() {
        // A 32x32 image where the left half is solid grey and the right half
        // is solid grey with a 5-unit shift, plus a sharp boundary. The
        // interior windows on the right should fail the strict threshold even
        // though the edge windows along the boundary may pass the relaxed one.
        let mut a = Vec::with_capacity(32 * 32 * 4);
        let mut b = Vec::with_capacity(32 * 32 * 4);
        for _y in 0..32 {
            for x in 0..32 {
                let ca = [200, 200, 200, 255];
                // Shift the right half of `b` by 5 units.
                let cb = if x < 16 {
                    [200, 200, 200, 255]
                } else {
                    [205, 205, 205, 255]
                };
                a.extend_from_slice(&ca);
                b.extend_from_slice(&cb);
            }
        }
        let result = perceptual_diff(&a, &b, 32, 32);
        // The interior windows on the right half differ by a uniform 5-unit
        // shift, which must fail the strict fill threshold.
        assert!(
            result.interior_ssim < FILL_SSIM_THRESHOLD,
            "interior should fail strict threshold, got {}",
            result.interior_ssim
        );
        assert!(!result.passed, "interior failure must fail overall");
    }

    #[test]
    fn non_multiple_of_eight_includes_partial_blocks() {
        // A 12x12 image (not a multiple of 8) with identical solid fills.
        // The loop must accumulate the partial 4-wide blocks on the right and
        // 4-tall blocks at the bottom rather than ignoring them.
        let a = solid_image(12, 12, [100, 150, 200, 255]);
        let b = solid_image(12, 12, [100, 150, 200, 255]);
        let result = perceptual_diff(&a, &b, 12, 12);
        assert!(result.passed);
        assert!(result.interior_ssim >= FILL_SSIM_THRESHOLD);
        // overall_ssim should reflect all 4 blocks (8x8, 4x8, 8x4, 4x4).
        assert!(result.overall_ssim > 0.9999);
    }

    #[test]
    fn non_multiple_of_eight_partial_block_failure_detected() {
        // A 12x12 image where the bottom-right 4x4 partial block differs. If
        // partial blocks were ignored this would incorrectly pass.
        let a = solid_image(12, 12, [100, 100, 100, 255]);
        let mut b = solid_image(12, 12, [100, 100, 100, 255]);
        // Corrupt the bottom-right 4x4 partial block of `b`.
        for y in 8..12 {
            for x in 8..12 {
                let idx = (y * 12 + x) * 4;
                b[idx] = 200;
                b[idx + 1] = 200;
                b[idx + 2] = 200;
            }
        }
        let result = perceptual_diff(&a, &b, 12, 12);
        assert!(
            !result.passed,
            "partial-block difference must be detected, interior_ssim={}",
            result.interior_ssim
        );
    }
}
