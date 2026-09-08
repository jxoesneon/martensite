//! Headless test harness with pixel-perfect perceptual snapshot diffing.
//!
//! [`HeadlessHarness`] drives a deterministic render loop using a
//! [`VirtualClock`], captures rendered frames into offscreen grayscale
//! buffers, and compares them against golden reference images using the
//! perceptual DSSIM metric from the [`mod@crate::dssim`] module.

use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;

use crate::dssim::{images_match, ImageBuffer};
use crate::virtual_clock::{VirtualClock, FRAME_60FPS};

/// Magic bytes identifying the on-disk golden image format (`MTI1`).
const GOLDEN_MAGIC: &[u8; 4] = b"MTI1";

/// A headless test harness for deterministic UI testing.
///
/// The harness owns a [`VirtualClock`] so that time only advances when the
/// test explicitly steps a frame. Each captured frame is stored as a
/// grayscale [`ImageBuffer`] and can be compared against a golden reference
/// using the perceptual DSSIM metric.
///
/// # Examples
///
/// ```
/// use martensite_test::HeadlessHarness;
///
/// let mut harness = HeadlessHarness::new(8, 8);
/// // Render a solid black frame.
/// let rgba = vec![0u8; 8 * 8 * 4];
/// harness.capture_frame(&rgba);
/// assert_eq!(harness.frame_count(), 1);
/// ```
pub struct HeadlessHarness {
    clock: VirtualClock,
    width: u32,
    height: u32,
    frame_count: u64,
    snapshots: Vec<ImageBuffer>,
}

impl HeadlessHarness {
    /// Creates a new harness for an offscreen buffer of `width`×`height`
    /// pixels.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_test::HeadlessHarness;
    ///
    /// let harness = HeadlessHarness::new(64, 32);
    /// assert_eq!(harness.clock().now(), std::time::Duration::ZERO);
    /// ```
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            clock: VirtualClock::new(),
            width,
            height,
            frame_count: 0,
            snapshots: Vec::new(),
        }
    }

    /// Returns a shared reference to the harness's virtual clock.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_test::HeadlessHarness;
    ///
    /// let harness = HeadlessHarness::new(4, 4);
    /// assert_eq!(harness.clock().elapsed_millis(), 0);
    /// ```
    pub fn clock(&self) -> &VirtualClock {
        &self.clock
    }

    /// Returns a mutable reference to the harness's virtual clock.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_test::HeadlessHarness;
    /// use std::time::Duration;
    ///
    /// let mut harness = HeadlessHarness::new(4, 4);
    /// harness.clock_mut().advance(Duration::from_millis(16));
    /// assert_eq!(harness.clock().elapsed_millis(), 16);
    /// ```
    pub fn clock_mut(&mut self) -> &mut VirtualClock {
        &mut self.clock
    }

    /// Returns the number of frames captured so far.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_test::HeadlessHarness;
    ///
    /// let mut harness = HeadlessHarness::new(2, 2);
    /// assert_eq!(harness.frame_count(), 0);
    /// harness.capture_frame(&[0; 2 * 2 * 4]);
    /// assert_eq!(harness.frame_count(), 1);
    /// ```
    pub fn frame_count(&self) -> u64 {
        self.frame_count
    }

    /// Captures a single RGBA frame into the snapshot history.
    ///
    /// `rgba` is converted to grayscale via [`ImageBuffer::from_rgba`]. The
    /// returned reference points to the newly stored snapshot.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_test::HeadlessHarness;
    ///
    /// let mut harness = HeadlessHarness::new(1, 1);
    /// let snap = harness.capture_frame(&[255, 255, 255, 255]);
    /// assert_eq!(snap.get(0, 0), Some(255));
    /// ```
    pub fn capture_frame(&mut self, rgba: &[u8]) -> &ImageBuffer {
        let image = ImageBuffer::from_rgba(self.width, self.height, rgba);
        self.snapshots.push(image);
        self.frame_count += 1;
        // Return the last stored snapshot. Indexing is safe because we just
        // pushed exactly one element.
        self.snapshots.last().expect("snapshot just pushed")
    }

    /// Returns the entire captured snapshot history, oldest first.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_test::HeadlessHarness;
    ///
    /// let mut harness = HeadlessHarness::new(1, 1);
    /// harness.capture_frame(&[0, 0, 0, 255]);
    /// harness.capture_frame(&[255, 255, 255, 255]);
    /// assert_eq!(harness.snapshots().len(), 2);
    /// ```
    pub fn snapshots(&self) -> &[ImageBuffer] {
        &self.snapshots
    }

    /// Compares the most recently captured frame against a golden reference.
    ///
    /// Returns `true` when the DSSIM distance between the last captured frame
    /// and `golden` is within `threshold`. If no frame has been captured yet,
    /// this returns `false`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_test::dssim::ImageBuffer;
    /// use martensite_test::HeadlessHarness;
    ///
    /// let mut harness = HeadlessHarness::new(8, 8);
    /// harness.capture_frame(&[0; 8 * 8 * 4]);
    /// let golden = ImageBuffer::new(8, 8);
    /// assert!(harness.compare_to_golden(&golden, 0.0));
    /// ```
    pub fn compare_to_golden(&self, golden: &ImageBuffer, threshold: f64) -> bool {
        match self.snapshots.last() {
            Some(frame) => images_match(frame, golden, threshold),
            None => false,
        }
    }

    /// Advances the harness clock by a single 60 FPS frame.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_test::HeadlessHarness;
    ///
    /// let mut harness = HeadlessHarness::new(4, 4);
    /// harness.step_frame();
    /// assert!(harness.clock().elapsed_millis() >= 16);
    /// ```
    pub fn step_frame(&mut self) {
        self.clock.advance(FRAME_60FPS);
    }

    /// Runs `count` frames, invoking `render_fn` for each frame.
    ///
    /// For every frame index `i` (starting at the current `frame_count`), the
    /// closure is called with `i` and must return the RGBA bytes for that
    /// frame. The clock is advanced by one 60 FPS step before each render so
    /// that time-driven logic inside the closure observes a deterministic
    /// progression.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_test::HeadlessHarness;
    ///
    /// let mut harness = HeadlessHarness::new(1, 1);
    /// harness.run_frames(3, |_| vec![0u8; 4]);
    /// assert_eq!(harness.frame_count(), 3);
    /// assert_eq!(harness.snapshots().len(), 3);
    /// ```
    ///
    /// The closure may capture mutable state:
    ///
    /// ```
    /// use martensite_test::HeadlessHarness;
    ///
    /// let mut harness = HeadlessHarness::new(1, 1);
    /// let mut count = 0u32;
    /// harness.run_frames(2, |_| {
    ///     count += 1;
    ///     vec![0u8; 4]
    /// });
    /// assert_eq!(count, 2);
    /// ```
    pub fn run_frames(&mut self, count: u32, mut render_fn: impl FnMut(u64) -> Vec<u8>) {
        for _ in 0..count {
            self.step_frame();
            let rgba = render_fn(self.frame_count);
            self.capture_frame(&rgba);
        }
    }
}

/// Errors that can occur while loading or saving golden images.
///
/// # Examples
///
/// ```
/// use martensite_test::GoldenError;
///
/// let err = GoldenError::InvalidFormat("bad magic".to_string());
/// assert!(matches!(err, GoldenError::InvalidFormat(_)));
/// ```
#[derive(Debug)]
pub enum GoldenError {
    /// An underlying I/O error occurred while reading or writing a file.
    Io(io::Error),
    /// The file content was not a valid golden image (bad magic, truncated
    /// header, or mismatched pixel count).
    InvalidFormat(String),
}

impl std::fmt::Display for GoldenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GoldenError::Io(err) => write!(f, "golden image I/O error: {err}"),
            GoldenError::InvalidFormat(msg) => write!(f, "invalid golden image format: {msg}"),
        }
    }
}

impl std::error::Error for GoldenError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            GoldenError::Io(err) => Some(err),
            GoldenError::InvalidFormat(_) => None,
        }
    }
}

impl From<io::Error> for GoldenError {
    fn from(err: io::Error) -> Self {
        GoldenError::Io(err)
    }
}

/// Golden image storage for snapshot tests.
///
/// Golden images are persisted as a small binary container: a 4-byte magic
/// (`MTI1`), the width and height as little-endian `u32`, followed by the raw
/// grayscale pixel bytes. The storage directory is created on demand.
///
/// # Examples
///
/// ```
/// use martensite_test::dssim::ImageBuffer;
/// use martensite_test::GoldenImages;
///
/// let dir = std::env::temp_dir().join("martensite_test_golden_doctest");
/// let goldens = GoldenImages::new(&dir);
/// let mut img = ImageBuffer::new(2, 2);
/// img.fill(128);
/// goldens.save("example", &img).unwrap();
/// assert!(goldens.exists("example"));
/// let loaded = goldens.load("example").unwrap();
/// assert_eq!(loaded.pixels, img.pixels);
/// # std::fs::remove_dir_all(&dir).ok();
/// ```
pub struct GoldenImages {
    /// The directory in which golden image files are stored.
    base_dir: PathBuf,
}

impl GoldenImages {
    /// Creates a new golden image store rooted at `base_dir`.
    ///
    /// The directory itself is not created until [`GoldenImages::save`] is
    /// called.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_test::GoldenImages;
    ///
    /// let goldens = GoldenImages::new(std::env::temp_dir());
    /// ```
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        Self {
            base_dir: base_dir.into(),
        }
    }

    /// Returns the path to the file backing the named golden image.
    fn path_for(&self, name: &str) -> PathBuf {
        self.base_dir.join(format!("{name}.mti"))
    }

    /// Loads a golden image previously written with [`Self::save`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_test::dssim::ImageBuffer;
    /// use martensite_test::GoldenImages;
    ///
    /// let dir = std::env::temp_dir().join("martensite_test_golden_load_doctest");
    /// let goldens = GoldenImages::new(&dir);
    /// let img = ImageBuffer::new(1, 1);
    /// goldens.save("g", &img).unwrap();
    /// let loaded = goldens.load("g").unwrap();
    /// assert_eq!(loaded.width, 1);
    /// # std::fs::remove_dir_all(&dir).ok();
    /// ```
    pub fn load(&self, name: &str) -> Result<ImageBuffer, GoldenError> {
        let path = self.path_for(name);
        let bytes = fs::read(&path)?;

        if bytes.len() < 12 {
            return Err(GoldenError::InvalidFormat(format!(
                "file too short: {} bytes (need at least 12)",
                bytes.len()
            )));
        }
        if &bytes[0..4] != GOLDEN_MAGIC {
            return Err(GoldenError::InvalidFormat(
                "bad magic bytes; expected `MTI1`".to_string(),
            ));
        }

        let width = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        let height = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);

        // Guard against OOM from malformed files: reject images larger
        // than 8192×8192 (64 MiB of grayscale pixels).
        const MAX_DIM: u32 = 8192;
        if width > MAX_DIM || height > MAX_DIM {
            return Err(GoldenError::InvalidFormat(format!(
                "image dimensions too large: {width}x{height} (max {MAX_DIM}x{MAX_DIM})"
            )));
        }

        let expected_pixels = (width as usize)
            .checked_mul(height as usize)
            .ok_or_else(|| GoldenError::InvalidFormat("image dimensions overflow".to_string()))?;
        let pixel_bytes = &bytes[12..];
        if pixel_bytes.len() != expected_pixels {
            return Err(GoldenError::InvalidFormat(format!(
                "pixel count mismatch: header implies {expected_pixels} bytes, file has {}",
                pixel_bytes.len()
            )));
        }

        Ok(ImageBuffer {
            width,
            height,
            pixels: pixel_bytes.to_vec(),
        })
    }

    /// Saves a golden image to disk, creating the base directory if needed.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_test::dssim::ImageBuffer;
    /// use martensite_test::GoldenImages;
    ///
    /// let dir = std::env::temp_dir().join("martensite_test_golden_save_doctest");
    /// let goldens = GoldenImages::new(&dir);
    /// let img = ImageBuffer::new(4, 4);
    /// goldens.save("frame_0", &img).unwrap();
    /// assert!(goldens.exists("frame_0"));
    /// # std::fs::remove_dir_all(&dir).ok();
    /// ```
    pub fn save(&self, name: &str, image: &ImageBuffer) -> Result<(), GoldenError> {
        fs::create_dir_all(&self.base_dir)?;
        let path = self.path_for(name);

        let mut file = fs::File::create(&path)?;
        file.write_all(GOLDEN_MAGIC)?;
        file.write_all(&image.width.to_le_bytes())?;
        file.write_all(&image.height.to_le_bytes())?;
        file.write_all(image.as_slice())?;
        file.flush()?;
        Ok(())
    }

    /// Returns `true` if a golden image with the given name exists on disk.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_test::GoldenImages;
    ///
    /// let dir = std::env::temp_dir().join("martensite_test_golden_exists_doctest");
    /// let goldens = GoldenImages::new(&dir);
    /// assert!(!goldens.exists("missing"));
    /// ```
    pub fn exists(&self, name: &str) -> bool {
        self.path_for(name).exists()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dssim::{dssim, ImageBuffer};
    use std::error::Error;
    use std::time::Duration;

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("martensite_test_harness_{label}"));
        // Start clean.
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn new_harness_is_empty() {
        let harness = HeadlessHarness::new(8, 8);
        assert_eq!(harness.clock().now(), Duration::ZERO);
        assert_eq!(harness.frame_count(), 0);
        assert!(harness.snapshots().is_empty());
    }

    #[test]
    fn clock_mut_allows_advance() {
        let mut harness = HeadlessHarness::new(4, 4);
        harness.clock_mut().advance(Duration::from_millis(16));
        assert_eq!(harness.clock().elapsed_millis(), 16);
    }

    #[test]
    fn capture_frame_stores_snapshot() {
        let mut harness = HeadlessHarness::new(2, 2);
        let rgba = vec![255; 2 * 2 * 4];
        let snap = harness.capture_frame(&rgba);
        assert_eq!(snap.width, 2);
        assert_eq!(snap.height, 2);
        assert!(snap.pixels.iter().all(|&p| p == 255));
        assert_eq!(harness.frame_count(), 1);
        assert_eq!(harness.snapshots().len(), 1);
    }

    #[test]
    fn capture_multiple_frames() {
        let mut harness = HeadlessHarness::new(1, 1);
        harness.capture_frame(&[0, 0, 0, 255]);
        harness.capture_frame(&[255, 255, 255, 255]);
        assert_eq!(harness.frame_count(), 2);
        assert_eq!(harness.snapshots().len(), 2);
        assert_eq!(harness.snapshots()[0].get(0, 0), Some(0));
        assert_eq!(harness.snapshots()[1].get(0, 0), Some(255));
    }

    #[test]
    fn compare_to_golden_identical_passes() {
        let mut harness = HeadlessHarness::new(8, 8);
        harness.capture_frame(&[0; 8 * 8 * 4]);
        let golden = ImageBuffer::new(8, 8);
        assert!(harness.compare_to_golden(&golden, 0.0));
    }

    #[test]
    fn compare_to_golden_different_fails() {
        let mut harness = HeadlessHarness::new(8, 8);
        harness.capture_frame(&[255; 8 * 8 * 4]);
        let golden = ImageBuffer::new(8, 8);
        assert!(!harness.compare_to_golden(&golden, 0.001));
    }

    #[test]
    fn compare_to_golden_no_frames_returns_false() {
        let harness = HeadlessHarness::new(8, 8);
        let golden = ImageBuffer::new(8, 8);
        assert!(!harness.compare_to_golden(&golden, 1.0));
    }

    #[test]
    fn step_frame_advances_clock() {
        let mut harness = HeadlessHarness::new(4, 4);
        let before = harness.clock().now();
        harness.step_frame();
        let after = harness.clock().now();
        assert!(after > before);
        assert!(harness.clock().elapsed_millis() >= 16);
    }

    #[test]
    fn run_frames_captures_and_steps() {
        let mut harness = HeadlessHarness::new(1, 1);
        let start = harness.clock().elapsed_millis();
        harness.run_frames(5, |_| vec![0u8; 4]);
        assert_eq!(harness.frame_count(), 5);
        assert_eq!(harness.snapshots().len(), 5);
        // 5 frames at ~16.666ms each ≈ 83ms.
        let elapsed = harness.clock().elapsed_millis() - start;
        assert!((80..=90).contains(&elapsed), "elapsed={elapsed}");
    }

    #[test]
    fn run_frames_passes_frame_index() {
        let mut harness = HeadlessHarness::new(1, 1);
        let mut seen = Vec::new();
        harness.run_frames(3, |i| {
            seen.push(i);
            vec![0u8; 4]
        });
        // Frame indices start at the count before each capture (0, 1, 2).
        assert_eq!(seen, vec![0, 1, 2]);
    }

    #[test]
    fn run_frames_zero_is_noop() {
        let mut harness = HeadlessHarness::new(1, 1);
        harness.run_frames(0, |_| vec![0u8; 4]);
        assert_eq!(harness.frame_count(), 0);
        assert_eq!(harness.clock().now(), Duration::ZERO);
    }

    #[test]
    fn golden_save_load_roundtrip() {
        let dir = temp_dir("roundtrip");
        let goldens = GoldenImages::new(&dir);

        let mut img = ImageBuffer::new(4, 3);
        for i in 0..img.pixels.len() {
            img.pixels[i] = (i % 256) as u8;
        }
        goldens.save("frame", &img).unwrap();
        assert!(goldens.exists("frame"));

        let loaded = goldens.load("frame").unwrap();
        assert_eq!(loaded.width, 4);
        assert_eq!(loaded.height, 3);
        assert_eq!(loaded.pixels, img.pixels);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn golden_load_missing_file_errors() {
        let dir = temp_dir("missing");
        let goldens = GoldenImages::new(&dir);
        let err = goldens.load("nope").unwrap_err();
        assert!(matches!(err, GoldenError::Io(_)));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn golden_load_bad_magic_errors() {
        let dir = temp_dir("badmagic");
        let goldens = GoldenImages::new(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("bad.mti"),
            b"XXXX\x01\x00\x00\x00\x01\x00\x00\x00\x00",
        )
        .unwrap();
        let err = goldens.load("bad").unwrap_err();
        assert!(matches!(err, GoldenError::InvalidFormat(_)));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn golden_load_truncated_header_errors() {
        let dir = temp_dir("truncated");
        let goldens = GoldenImages::new(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("short.mti"), b"MTI1\x01").unwrap();
        let err = goldens.load("short").unwrap_err();
        assert!(matches!(err, GoldenError::InvalidFormat(_)));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn golden_load_pixel_count_mismatch_errors() {
        let dir = temp_dir("mismatch");
        let goldens = GoldenImages::new(&dir);
        fs::create_dir_all(&dir).unwrap();
        // Header says 2x2 = 4 pixels, but only 1 pixel byte follows.
        let mut data = Vec::new();
        data.extend_from_slice(GOLDEN_MAGIC);
        data.extend_from_slice(&2u32.to_le_bytes());
        data.extend_from_slice(&2u32.to_le_bytes());
        data.push(0);
        fs::write(dir.join("m.mti"), &data).unwrap();
        let err = goldens.load("m").unwrap_err();
        assert!(matches!(err, GoldenError::InvalidFormat(_)));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn golden_save_creates_directory() {
        let dir = temp_dir("mkdir");
        let nested = dir.join("nested");
        let goldens = GoldenImages::new(&nested);
        let img = ImageBuffer::new(1, 1);
        goldens.save("g", &img).unwrap();
        assert!(nested.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn golden_exists_false_for_missing() {
        let dir = temp_dir("exists_false");
        let goldens = GoldenImages::new(&dir);
        assert!(!goldens.exists("absent"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn golden_error_display_and_source() {
        let io_err = GoldenError::Io(io::Error::new(io::ErrorKind::NotFound, "missing"));
        assert!(format!("{io_err}").contains("I/O error"));
        assert!(io_err.source().is_some());

        let fmt_err = GoldenError::InvalidFormat("bad".to_string());
        assert!(format!("{fmt_err}").contains("invalid golden image format"));
        assert!(fmt_err.source().is_none());
    }

    #[test]
    fn golden_error_from_io() {
        let err: GoldenError = io::Error::other("x").into();
        assert!(matches!(err, GoldenError::Io(_)));
    }

    #[test]
    fn golden_save_overwrites_existing() {
        let dir = temp_dir("overwrite");
        let goldens = GoldenImages::new(&dir);
        let mut a = ImageBuffer::new(1, 1);
        a.fill(10);
        goldens.save("g", &a).unwrap();
        let mut b = ImageBuffer::new(1, 1);
        b.fill(200);
        goldens.save("g", &b).unwrap();
        let loaded = goldens.load("g").unwrap();
        assert_eq!(loaded.pixels, vec![200]);
        let _ = fs::remove_dir_all(&dir);
    }

    /// Verifies the deterministic CI test gate: 100 consecutive runs of the
    /// same snapshot comparison yield 100% identical pass results with zero
    /// timing jitter.
    #[test]
    fn deterministic_ci_gate_100_runs() {
        // Build a non-trivial golden image and a near-matching frame.
        let mut golden = ImageBuffer::new(16, 16);
        for y in 0..16 {
            for x in 0..16 {
                golden.set(x, y, ((x * 17 + y * 3) % 256) as u8);
            }
        }

        // Render the same frame 100 times and confirm every comparison is
        // identical (both the boolean pass result and the underlying DSSIM
        // value).
        let mut last_pass: Option<bool> = None;
        let mut last_score: Option<f64> = None;
        for _ in 0..100 {
            let mut harness = HeadlessHarness::new(16, 16);
            let mut rgba = Vec::with_capacity(16 * 16 * 4);
            for y in 0..16 {
                for x in 0..16 {
                    let g = ((x * 17 + y * 3) % 256) as u8;
                    rgba.extend_from_slice(&[g, g, g, 255]);
                }
            }
            harness.capture_frame(&rgba);
            let pass = harness.compare_to_golden(&golden, 0.001);
            let score = dssim(harness.snapshots().last().unwrap(), &golden);

            if let Some(prev) = last_pass {
                assert_eq!(prev, pass, "pass result jittered across runs");
            }
            if let Some(prev) = last_score {
                assert!(
                    (prev - score).abs() < 1e-12,
                    "DSSIM score jittered: {prev} vs {score}"
                );
            }
            last_pass = Some(pass);
            last_score = Some(score);
        }

        // The images are identical, so the gate must pass every run.
        assert_eq!(last_pass, Some(true));
        assert_eq!(last_score, Some(0.0));
    }
}
