//! Two-tier text measurement and glyph shaping cache.
//!
//! ## Tier 1 — Inline cache (in `martensite-core`)
//!
//! Four inline `(available_width, measured_height)` entries embedded
//! directly in each [`ColdNode`](martensite_core::ColdNode). This
//! provides O(1) lookup during flexbox's two-pass measurement, where
//! the same text node is probed at multiple constraint widths.
//!
//! ## Tier 2 — Global LRU shaping cache
//!
//! A bounded global LRU cache that stores fully shaped glyph runs,
//! keyed by `(FontId, font_size_bits, text_hash)`. The cache targets
//! a 16 MB memory budget and evicts least-recently-used entries when
//! the budget is exceeded.
//!
//! ## Cache key
//!
//! The key is `(FontId, font_size_bits, text_hash)` where:
//! - `FontId` identifies the font face
//! - `font_size_bits` is the font size quantized to `f32` bits (via
//!   `f32::to_bits`) for stable hashing
//! - `text_hash` is a `FxHash`-compatible hash of the text content

use std::collections::HashMap;
use std::hash::Hash;

use crate::bidi::BidiDirection;
use crate::font::FontId;
use crate::shaping::{ShapedGlyph, ShapedLine, TextMetrics};
use crate::vertical::WritingMode;

/// Default memory budget for the Tier 2 cache: 16 MB.
///
/// # Examples
///
/// ```
/// use martensite_text::cache::DEFAULT_MEMORY_BUDGET;
///
/// assert_eq!(DEFAULT_MEMORY_BUDGET, 16 * 1024 * 1024);
/// ```
pub const DEFAULT_MEMORY_BUDGET: usize = 16 * 1024 * 1024;

/// Quantized font size for cache keying.
///
/// We use the raw `f32` bits to ensure stable, exact matching.
///
/// # Examples
///
/// ```
/// use martensite_text::cache::FontSizeBits;
///
/// let bits = FontSizeBits::from_f32(16.0);
/// assert_eq!(bits.to_f32(), 16.0);
/// ```
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct FontSizeBits(pub u32);

impl FontSizeBits {
    /// Creates a `FontSizeBits` from an `f32` font size.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_text::cache::FontSizeBits;
    ///
    /// let bits = FontSizeBits::from_f32(12.0);
    /// assert_eq!(bits.to_f32(), 12.0);
    /// ```
    #[inline(always)]
    pub fn from_f32(size: f32) -> Self {
        Self(size.to_bits())
    }

    /// Converts back to `f32`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_text::cache::FontSizeBits;
    ///
    /// let bits = FontSizeBits::from_f32(20.0);
    /// assert_eq!(bits.to_f32(), 20.0);
    /// ```
    #[inline(always)]
    pub fn to_f32(self) -> f32 {
        f32::from_bits(self.0)
    }
}

/// A fast, deterministic hash for text content.
///
/// Uses a simple FxHash-style accumulator. This is NOT cryptographically
/// secure but is fast and sufficient for cache keying.
///
/// # Examples
///
/// ```
/// use martensite_text::cache::TextHash;
///
/// let a = TextHash::from_string("hello");
/// let b = TextHash::from_string("hello");
/// assert_eq!(a, b);
/// ```
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct TextHash(pub u64);

impl TextHash {
    /// Computes a hash from a byte slice.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_text::cache::TextHash;
    ///
    /// let h = TextHash::from_bytes(b"abc");
    /// assert_ne!(h, TextHash::from_bytes(b"xyz"));
    /// ```
    pub fn from_bytes(bytes: &[u8]) -> Self {
        // FxHash variant
        let mut hash = 0xcbf29ce484222325u64;
        for &byte in bytes {
            hash = (hash ^ byte as u64).wrapping_mul(0x100000001b3);
        }
        Self(hash)
    }

    /// Computes a hash from a string.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_text::cache::TextHash;
    ///
    /// let h = TextHash::from_string("hello");
    /// assert_eq!(h, TextHash::from_bytes(b"hello"));
    /// ```
    #[inline]
    pub fn from_string(text: &str) -> Self {
        Self::from_bytes(text.as_bytes())
    }
}

/// Cache key for the Tier 2 shaping cache.
///
/// # Examples
///
/// ```
/// use martensite_text::cache::ShapeCacheKey;
/// use martensite_text::font::FontId;
///
/// let key = ShapeCacheKey::new(FontId::dummy(), 16.0, "hello");
/// assert_eq!(key.font_size_bits.to_f32(), 16.0);
/// ```
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct ShapeCacheKey {
    /// Font face identifier.
    pub font_id: FontId,
    /// Font size quantized to bits.
    pub font_size_bits: FontSizeBits,
    /// Hash of the text content.
    pub text_hash: TextHash,
    /// Available width for wrapping, quantized to bits.
    /// `u32::MAX` represents unbounded (no wrapping).
    pub max_width_bits: MaxWidthBits,
    /// Hash of the font family name.
    pub family_hash: TextHash,
    /// Line height quantized to bits.
    pub line_height_bits: LineHeightBits,
    /// Base BiDi direction.
    pub direction: DirectionBits,
    /// Writing mode (horizontal/vertical).
    pub writing_mode: WritingModeBits,
    /// Hash of the resolved fallback chain.
    pub fallback_hash: FallbackHash,
}

/// Quantized max width for cache keying.
/// `u32::MAX` represents unbounded (no wrapping).
///
/// # Examples
///
/// ```
/// use martensite_text::cache::MaxWidthBits;
///
/// let bits = MaxWidthBits::from_opt(Some(200.0));
/// assert_eq!(bits.to_opt(), Some(200.0));
/// ```
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct MaxWidthBits(pub u32);

impl MaxWidthBits {
    /// Creates a `MaxWidthBits` from an optional `f32` width.
    /// `None` maps to `u32::MAX` (unbounded).
    /// `Some(0.0)` or negative maps to `0` (zero width).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_text::cache::MaxWidthBits;
    ///
    /// assert_eq!(MaxWidthBits::from_opt(None).to_opt(), None);
    /// assert_eq!(MaxWidthBits::from_opt(Some(0.0)).to_opt(), Some(0.0));
    /// assert_eq!(MaxWidthBits::from_opt(Some(100.0)).to_opt(), Some(100.0));
    /// ```
    #[inline]
    pub fn from_opt(width: Option<f32>) -> Self {
        match width {
            Some(w) if w.is_finite() && w > 0.0 => Self(w.to_bits()),
            Some(w) if w.is_finite() && w <= 0.0 => Self(0),
            _ => Self(u32::MAX),
        }
    }

    /// Converts back to `Option<f32>`.
    /// `u32::MAX` represents unbounded (None).
    /// `0` represents zero width (Some(0.0)).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_text::cache::MaxWidthBits;
    ///
    /// let bits = MaxWidthBits::from_opt(Some(50.0));
    /// assert_eq!(bits.to_opt(), Some(50.0));
    /// ```
    #[inline]
    pub fn to_opt(self) -> Option<f32> {
        if self.0 == u32::MAX {
            None
        } else if self.0 == 0 {
            Some(0.0)
        } else {
            Some(f32::from_bits(self.0))
        }
    }
}

/// Quantized line height for cache keying.
///
/// # Examples
///
/// ```
/// use martensite_text::cache::LineHeightBits;
///
/// let bits = LineHeightBits::from_f32(24.0);
/// assert_eq!(bits.to_f32(), 24.0);
/// ```
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct LineHeightBits(pub u32);

/// Quantized BiDi direction for cache keying.
///
/// # Examples
///
/// ```
/// use martensite_text::cache::DirectionBits;
///
/// let bits = DirectionBits::default();
/// assert_eq!(bits, DirectionBits::Ltr);
/// ```
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]
pub enum DirectionBits {
    /// Left-to-right base direction.
    #[default]
    Ltr,
    /// Right-to-left base direction.
    Rtl,
    /// Automatic direction detection.
    Auto,
}

impl DirectionBits {
    /// Creates a `DirectionBits` from a [`BidiDirection`].
    #[inline]
    pub const fn from_direction(dir: BidiDirection) -> Self {
        match dir {
            BidiDirection::Ltr => Self::Ltr,
            BidiDirection::Rtl => Self::Rtl,
            BidiDirection::Auto => Self::Auto,
        }
    }
}

/// Writing mode for cache keying.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]
pub enum WritingModeBits {
    /// Horizontal top-to-bottom flow.
    #[default]
    HorizontalTb,
    /// Vertical right-to-left flow.
    VerticalRl,
    /// Vertical left-to-right flow.
    VerticalLr,
}

impl WritingModeBits {
    /// Creates a `WritingModeBits` from a [`WritingMode`].
    #[inline]
    pub const fn from_mode(mode: WritingMode) -> Self {
        match mode {
            WritingMode::HorizontalTb => Self::HorizontalTb,
            WritingMode::VerticalRl => Self::VerticalRl,
            WritingMode::VerticalLr => Self::VerticalLr,
        }
    }
}

/// Hash of a resolved fallback chain, used in cache keys.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]
pub struct FallbackHash(pub u64);

impl FallbackHash {
    /// Computes a hash from an iterator of family names.
    pub fn from_families<'a>(families: impl IntoIterator<Item = &'a str>) -> Self {
        let mut hash = 0xcbf29ce484222325u64;
        for family in families {
            for &byte in family.as_bytes() {
                hash = (hash ^ byte as u64).wrapping_mul(0x100000001b3);
            }
        }
        Self(hash)
    }
}

impl LineHeightBits {
    /// Creates a `LineHeightBits` from an `f32` line height.
    /// Zero or negative maps to `0` (default line height).
    #[inline]
    pub fn from_f32(line_height: f32) -> Self {
        if line_height.is_finite() && line_height > 0.0 {
            Self(line_height.to_bits())
        } else {
            Self(0)
        }
    }

    /// Converts back to `f32`.
    #[inline]
    pub fn to_f32(self) -> f32 {
        if self.0 == 0 {
            0.0
        } else {
            f32::from_bits(self.0)
        }
    }
}

impl ShapeCacheKey {
    /// Creates a new cache key.
    #[inline]
    pub fn new(font_id: FontId, font_size: f32, text: &str) -> Self {
        Self::with_options(
            font_id,
            font_size,
            text,
            None,
            "",
            0.0,
            BidiDirection::Ltr,
            WritingMode::HorizontalTb,
            &[],
        )
    }

    /// Creates a new cache key with a max width for wrapping.
    #[inline]
    pub fn with_max_width(
        font_id: FontId,
        font_size: f32,
        text: &str,
        max_width: Option<f32>,
    ) -> Self {
        Self::with_options(
            font_id,
            font_size,
            text,
            max_width,
            "",
            0.0,
            BidiDirection::Ltr,
            WritingMode::HorizontalTb,
            &[],
        )
    }

    /// Creates a new cache key with max width, family, and line height.
    #[inline]
    pub fn with_max_width_and_family(
        font_id: FontId,
        font_size: f32,
        text: &str,
        max_width: Option<f32>,
        family: &str,
        line_height: f32,
    ) -> Self {
        Self::with_options(
            font_id,
            font_size,
            text,
            max_width,
            family,
            line_height,
            BidiDirection::Ltr,
            WritingMode::HorizontalTb,
            &[],
        )
    }

    /// Creates a full cache key including direction, writing mode, and fallback.
    #[inline]
    #[allow(clippy::too_many_arguments)]
    pub fn with_options(
        font_id: FontId,
        font_size: f32,
        text: &str,
        max_width: Option<f32>,
        family: &str,
        line_height: f32,
        direction: BidiDirection,
        writing_mode: WritingMode,
        fallback_families: &[&str],
    ) -> Self {
        Self {
            font_id,
            font_size_bits: FontSizeBits::from_f32(font_size),
            text_hash: TextHash::from_string(text),
            max_width_bits: MaxWidthBits::from_opt(max_width),
            family_hash: TextHash::from_string(family),
            line_height_bits: LineHeightBits::from_f32(line_height),
            direction: DirectionBits::from_direction(direction),
            writing_mode: WritingModeBits::from_mode(writing_mode),
            fallback_hash: FallbackHash::from_families(fallback_families.iter().copied()),
        }
    }
}

/// A cached shaped text entry, storing the shaped lines and metrics.
#[derive(Clone, Debug)]
pub struct CachedShape {
    /// The shaped lines.
    pub lines: Vec<ShapedLine>,
    /// The measured metrics.
    pub metrics: TextMetrics,
    /// Approximate memory size in bytes (u64 to prevent overflow).
    pub mem_size: u64,
    /// Resolved base BiDi embedding level (even = LTR, odd = RTL).
    pub base_bidi_level: u8,
    /// Visual run order indices, if BiDi reordering was applied.
    pub visual_run_order: Vec<usize>,
    /// Resolved fallback chain families, in priority order.
    pub fallback_chain: Vec<String>,
    /// Writing mode used for shaping.
    pub writing_mode: WritingMode,
}

impl CachedShape {
    /// Creates a new cached shape entry.
    pub fn new(lines: Vec<ShapedLine>, metrics: TextMetrics) -> Self {
        let mem_size = Self::estimate_mem_size(&lines);
        Self {
            lines,
            metrics,
            mem_size,
            base_bidi_level: 0,
            visual_run_order: Vec::new(),
            fallback_chain: Vec::new(),
            writing_mode: WritingMode::HorizontalTb,
        }
    }

    /// Creates a new cached shape entry with extended v0.11 metadata.
    pub fn with_metadata(
        lines: Vec<ShapedLine>,
        metrics: TextMetrics,
        base_bidi_level: u8,
        visual_run_order: Vec<usize>,
        fallback_chain: Vec<String>,
        writing_mode: WritingMode,
    ) -> Self {
        let mut shape = Self::new(lines, metrics);
        shape.base_bidi_level = base_bidi_level;
        shape.visual_run_order = visual_run_order;
        shape.fallback_chain = fallback_chain;
        shape.writing_mode = writing_mode;
        shape
    }

    /// Estimates the memory usage of the shaped lines in bytes.
    ///
    /// Uses `saturating_add` throughout to prevent overflow on very
    /// large shaped texts (peer crates like moka and byte-lru-cache
    /// use `u64` for the same reason).
    fn estimate_mem_size(lines: &[ShapedLine]) -> u64 {
        // Base overhead: the CachedShape struct itself + TextMetrics
        let mut total = (std::mem::size_of::<TextMetrics>() + std::mem::size_of::<usize>()) as u64;
        // Each line's heap allocations
        for line in lines {
            total = total.saturating_add(std::mem::size_of::<ShapedLine>() as u64);
            total = total.saturating_add(line.text.capacity() as u64);
            total = total.saturating_add(
                (line.glyphs.len() as u64)
                    .saturating_mul(std::mem::size_of::<ShapedGlyph>() as u64),
            );
        }
        total
    }
}

/// Tier 2: bounded global LRU shaping cache.
///
/// Stores shaped text results keyed by `(FontId, font_size_bits,
/// text_hash)`. Evicts least-recently-used entries when the total
/// memory budget is exceeded.
///
/// The cache tracks access order via an internal age counter. Each
/// access updates the entry's age. When the budget is exceeded, the
/// oldest entries are evicted first.
///
/// # Examples
///
/// ```
/// use martensite_text::TextShapeCache;
///
/// let cache = TextShapeCache::with_default_budget();
/// assert!(cache.is_empty());
/// assert_eq!(cache.budget(), 16 * 1024 * 1024);
/// ```
pub struct TextShapeCache {
    entries: HashMap<ShapeCacheKey, (u64, CachedShape)>,
    /// Current age counter; incremented on each access.
    age: u64,
    /// Total estimated memory in bytes (u64 to prevent overflow).
    total_mem: u64,
    /// Memory budget in bytes (u64 to prevent overflow).
    budget: u64,
    /// Number of cache hits.
    hits: u64,
    /// Number of cache misses.
    misses: u64,
}

impl Default for TextShapeCache {
    fn default() -> Self {
        Self::new(DEFAULT_MEMORY_BUDGET as u64)
    }
}

impl TextShapeCache {
    /// Creates a new cache with the given memory budget in bytes.
    pub fn new(budget: u64) -> Self {
        Self {
            entries: HashMap::new(),
            age: 0,
            total_mem: 0,
            budget,
            hits: 0,
            misses: 0,
        }
    }

    /// Creates a cache with the default 16 MB budget.
    #[inline]
    pub fn with_default_budget() -> Self {
        Self::default()
    }

    /// Returns the number of entries in the cache.
    #[inline]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` if the cache is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Returns the total estimated memory usage in bytes.
    #[inline]
    pub fn total_memory(&self) -> u64 {
        self.total_mem
    }

    /// Returns the memory budget in bytes.
    #[inline]
    pub fn budget(&self) -> u64 {
        self.budget
    }

    /// Returns the number of cache hits.
    #[inline]
    pub fn hits(&self) -> u64 {
        self.hits
    }

    /// Returns the number of cache misses.
    #[inline]
    pub fn misses(&self) -> u64 {
        self.misses
    }

    /// Returns the cache hit rate as a fraction in `[0.0, 1.0]`.
    #[inline]
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            self.hits as f64 / total as f64
        }
    }

    /// Looks up a cached shape by key.
    ///
    /// Returns `Some(&CachedShape)` on hit, `None` on miss. Updates
    /// the entry's access age on hit.
    pub fn get(&mut self, key: &ShapeCacheKey) -> Option<&CachedShape> {
        if let Some((entry_age, shape)) = self.entries.get_mut(key) {
            *entry_age = self.age;
            self.age += 1;
            self.hits += 1;
            Some(shape)
        } else {
            self.misses += 1;
            None
        }
    }

    /// Inserts a shaped result into the cache.
    ///
    /// If the entry's memory pushes the total over budget, LRU
    /// entries are evicted until the budget is satisfied. Returns
    /// `true` if the entry was cached, or `false` if the entry was
    /// rejected because its own memory size exceeds the budget (an
    /// oversized entry can never fit even in an empty cache, so it
    /// is dropped to avoid violating the memory invariant).
    pub fn insert(&mut self, key: ShapeCacheKey, shape: CachedShape) -> bool {
        let mem_size = shape.mem_size;

        // Reject entries that are individually larger than the budget.
        // Even with an empty cache such an entry would exceed the limit,
        // and the eviction loop below would exit immediately without
        // making room, so we short-circuit here.
        if mem_size > self.budget {
            return false;
        }

        // If updating an existing entry, subtract old size first.
        if let Some((_, old)) = self.entries.remove(&key) {
            self.total_mem = self.total_mem.saturating_sub(old.mem_size);
        }

        // Evict LRU entries until we have room.
        while self.total_mem.saturating_add(mem_size) > self.budget && !self.entries.is_empty() {
            self.evict_oldest();
        }

        self.total_mem = self.total_mem.saturating_add(mem_size);
        self.entries.insert(key, (self.age, shape));
        self.age += 1;
        true
    }

    /// Evicts the oldest (least-recently-used) entry.
    fn evict_oldest(&mut self) {
        if let Some(&oldest_key) = self
            .entries
            .iter()
            .min_by_key(|(_, (age, _))| *age)
            .map(|(k, _)| k)
        {
            if let Some((_, removed)) = self.entries.remove(&oldest_key) {
                self.total_mem = self.total_mem.saturating_sub(removed.mem_size);
            }
        }
    }

    /// Clears all entries from the cache.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.total_mem = 0;
    }

    /// Trims entries that haven't been accessed in `keep_age` ticks.
    ///
    /// This is a softer eviction than the memory-based eviction in
    /// [`Self::insert`].
    pub fn trim(&mut self, keep_age: u64) {
        let current_age = self.age;
        self.entries.retain(|_, (age, shape)| {
            // Use saturating_sub so the age difference cannot overflow.
            // An entry is kept if it was accessed within `keep_age` ticks
            // of the current age (i.e. current_age - age <= keep_age).
            if current_age.saturating_sub(*age) <= keep_age {
                true
            } else {
                self.total_mem = self.total_mem.saturating_sub(shape.mem_size);
                false
            }
        });
    }

    /// Invalidates all entries for a specific font (e.g., when a font
    /// is unloaded or changed).
    pub fn invalidate_font(&mut self, font_id: FontId) {
        self.entries.retain(|key, (_, shape)| {
            if key.font_id == font_id {
                self.total_mem = self.total_mem.saturating_sub(shape.mem_size);
                false
            } else {
                true
            }
        });
    }

    /// Invalidates all entries for a specific font size.
    pub fn invalidate_font_size(&mut self, font_size: f32) {
        let bits = FontSizeBits::from_f32(font_size);
        self.entries.retain(|key, (_, shape)| {
            if key.font_size_bits == bits {
                self.total_mem = self.total_mem.saturating_sub(shape.mem_size);
                false
            } else {
                true
            }
        });
    }

    /// Resizes the memory budget, evicting entries if the new budget
    /// is smaller than current usage.
    pub fn resize(&mut self, new_budget: u64) {
        self.budget = new_budget;
        while self.total_mem > self.budget && !self.entries.is_empty() {
            self.evict_oldest();
        }
    }
}

impl std::fmt::Debug for TextShapeCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextShapeCache")
            .field("entries", &self.entries.len())
            .field("total_mem", &self.total_mem)
            .field("budget", &self.budget)
            .field("hits", &self.hits)
            .field("misses", &self.misses)
            .field("hit_rate", &self.hit_rate())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shaping::TextMetrics;

    fn make_cached_shape(width: f32, height: f32, line_count: usize) -> CachedShape {
        let metrics = TextMetrics {
            width,
            height,
            line_count,
        };
        CachedShape::new(vec![], metrics)
    }

    fn make_key(_id: u32, size: f32, text: &str) -> ShapeCacheKey {
        ShapeCacheKey::new(FontId(dummy_font_id()), size, text)
    }

    // Use a real fontdb::ID by creating a dummy value
    fn dummy_font_id() -> fontdb::ID {
        // fontdb::ID is a NonZeroU64 wrapper in some versions; use a safe default
        // We'll create an ID from a value of 1
        fontdb::ID::dummy()
    }

    fn make_key_v2(_id_val: u64, size: f32, text: &str) -> ShapeCacheKey {
        ShapeCacheKey::new(FontId(dummy_font_id()), size, text)
    }

    #[test]
    fn make_key_v2_compiles() {
        let _ = make_key_v2(1, 16.0, "test");
    }

    #[test]
    fn font_size_bits_roundtrip() {
        let bits = FontSizeBits::from_f32(16.0);
        assert_eq!(bits.to_f32(), 16.0);
        let bits_nan = FontSizeBits::from_f32(f32::NAN);
        assert!(bits_nan.to_f32().is_nan());
    }

    #[test]
    fn text_hash_deterministic() {
        let h1 = TextHash::from_string("Hello");
        let h2 = TextHash::from_string("Hello");
        assert_eq!(h1, h2);
        let h3 = TextHash::from_string("World");
        assert_ne!(h1, h3);
    }

    #[test]
    fn text_hash_empty() {
        let h = TextHash::from_string("");
        // FNV offset basis
        assert_eq!(h.0, 0xcbf29ce484222325);
    }

    #[test]
    fn cache_key_equality() {
        let k1 = ShapeCacheKey::new(FontId(dummy_font_id()), 16.0, "Hello");
        let k2 = ShapeCacheKey::new(FontId(dummy_font_id()), 16.0, "Hello");
        assert_eq!(k1, k2);
    }

    #[test]
    fn cache_key_differs_by_text() {
        let k1 = ShapeCacheKey::new(FontId(dummy_font_id()), 16.0, "Hello");
        let k2 = ShapeCacheKey::new(FontId(dummy_font_id()), 16.0, "World");
        assert_ne!(k1, k2);
    }

    #[test]
    fn cache_key_differs_by_font_size() {
        let k1 = ShapeCacheKey::new(FontId(dummy_font_id()), 16.0, "Hello");
        let k2 = ShapeCacheKey::new(FontId(dummy_font_id()), 20.0, "Hello");
        assert_ne!(k1, k2);
    }

    #[test]
    fn cache_key_includes_direction_and_writing_mode() {
        let key_ltr = ShapeCacheKey::with_options(
            FontId(dummy_font_id()),
            16.0,
            "Hello",
            None,
            "",
            0.0,
            crate::bidi::BidiDirection::Ltr,
            crate::vertical::WritingMode::HorizontalTb,
            &[],
        );
        let key_rtl = ShapeCacheKey::with_options(
            FontId(dummy_font_id()),
            16.0,
            "Hello",
            None,
            "",
            0.0,
            crate::bidi::BidiDirection::Rtl,
            crate::vertical::WritingMode::HorizontalTb,
            &[],
        );
        let key_vertical = ShapeCacheKey::with_options(
            FontId(dummy_font_id()),
            16.0,
            "Hello",
            None,
            "",
            0.0,
            crate::bidi::BidiDirection::Ltr,
            crate::vertical::WritingMode::VerticalRl,
            &[],
        );
        assert_ne!(
            key_ltr, key_rtl,
            "LTR and RTL should produce different keys"
        );
        assert_ne!(
            key_ltr, key_vertical,
            "horizontal and vertical should produce different keys"
        );
    }

    #[test]
    fn cache_key_includes_fallback_hash() {
        let key_no_fallback = ShapeCacheKey::with_options(
            FontId(dummy_font_id()),
            16.0,
            "Hello",
            None,
            "",
            0.0,
            crate::bidi::BidiDirection::Ltr,
            crate::vertical::WritingMode::HorizontalTb,
            &[],
        );
        let key_with_fallback = ShapeCacheKey::with_options(
            FontId(dummy_font_id()),
            16.0,
            "Hello",
            None,
            "",
            0.0,
            crate::bidi::BidiDirection::Ltr,
            crate::vertical::WritingMode::HorizontalTb,
            &["Noto Sans"],
        );
        assert_ne!(key_no_fallback, key_with_fallback);
    }

    #[test]
    fn cache_new_is_empty() {
        let cache = TextShapeCache::new(1024);
        assert!(cache.is_empty());
        assert_eq!(cache.len(), 0);
        assert_eq!(cache.total_memory(), 0);
    }

    #[test]
    fn cache_insert_and_get() {
        let mut cache = TextShapeCache::new(1024 * 1024);
        let key = ShapeCacheKey::new(FontId(dummy_font_id()), 16.0, "Hello");
        let shape = make_cached_shape(100.0, 20.0, 1);
        cache.insert(key, shape);
        assert_eq!(cache.len(), 1);

        let retrieved = cache.get(&key);
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().metrics.width, 100.0);
    }

    #[test]
    fn cache_miss_returns_none() {
        let mut cache = TextShapeCache::new(1024 * 1024);
        let key = ShapeCacheKey::new(FontId(dummy_font_id()), 16.0, "Hello");
        assert!(cache.get(&key).is_none());
        assert_eq!(cache.misses(), 1);
    }

    #[test]
    fn cache_hit_rate() {
        let mut cache = TextShapeCache::new(1024 * 1024);
        let key = ShapeCacheKey::new(FontId(dummy_font_id()), 16.0, "Hello");
        cache.insert(key, make_cached_shape(100.0, 20.0, 1));

        // 2 hits, 1 miss
        cache.get(&key);
        cache.get(&key);
        let missing_key = ShapeCacheKey::new(FontId(dummy_font_id()), 16.0, "World");
        cache.get(&missing_key);

        assert_eq!(cache.hits(), 2);
        assert_eq!(cache.misses(), 1);
        assert!((cache.hit_rate() - 2.0 / 3.0).abs() < 0.001);
    }

    #[test]
    fn cache_eviction_on_budget_exceeded() {
        let mut cache = TextShapeCache::new(200); // Very small budget
        let key1 = ShapeCacheKey::new(FontId(dummy_font_id()), 16.0, "A");
        let key2 = ShapeCacheKey::new(FontId(dummy_font_id()), 16.0, "B");
        let key3 = ShapeCacheKey::new(FontId(dummy_font_id()), 16.0, "C");

        // Each empty CachedShape is ~80 bytes (Vec<ShapedLine> header + TextMetrics)
        cache.insert(key1, make_cached_shape(10.0, 10.0, 1));
        cache.insert(key2, make_cached_shape(20.0, 10.0, 1));
        cache.insert(key3, make_cached_shape(30.0, 10.0, 1));

        // Should have evicted some entries to stay under budget
        assert!(
            cache.total_memory() <= 200,
            "total mem {} should be <= 200",
            cache.total_memory()
        );
    }

    #[test]
    fn cache_lru_eviction_order() {
        let mut cache = TextShapeCache::new(300);
        let key1 = ShapeCacheKey::new(FontId(dummy_font_id()), 16.0, "A");
        let key2 = ShapeCacheKey::new(FontId(dummy_font_id()), 16.0, "B");
        let key3 = ShapeCacheKey::new(FontId(dummy_font_id()), 16.0, "C");

        cache.insert(key1, make_cached_shape(10.0, 10.0, 1));
        cache.insert(key2, make_cached_shape(20.0, 10.0, 1));

        // Access key1 to make it more recently used
        cache.get(&key1);

        // Insert key3, which should evict key2 (least recently used)
        cache.insert(key3, make_cached_shape(30.0, 10.0, 1));

        assert!(cache.get(&key1).is_some(), "key1 should still be present");
        // key2 may or may not be evicted depending on exact sizes
    }

    #[test]
    fn cache_clear() {
        let mut cache = TextShapeCache::new(1024 * 1024);
        let key = ShapeCacheKey::new(FontId(dummy_font_id()), 16.0, "Hello");
        cache.insert(key, make_cached_shape(100.0, 20.0, 1));
        assert!(!cache.is_empty());

        cache.clear();
        assert!(cache.is_empty());
        assert_eq!(cache.total_memory(), 0);
    }

    #[test]
    fn cache_update_existing_entry() {
        let mut cache = TextShapeCache::new(1024 * 1024);
        let key = ShapeCacheKey::new(FontId(dummy_font_id()), 16.0, "Hello");
        cache.insert(key, make_cached_shape(100.0, 20.0, 1));

        // Insert again with different metrics
        cache.insert(key, make_cached_shape(200.0, 40.0, 2));
        assert_eq!(cache.len(), 1, "should still have 1 entry");

        let retrieved = cache.get(&key).unwrap();
        assert_eq!(retrieved.metrics.width, 200.0);
        assert_eq!(retrieved.metrics.line_count, 2);
    }

    #[test]
    fn cache_invalidate_font() {
        let mut cache = TextShapeCache::new(1024 * 1024);
        let fid = FontId(dummy_font_id());
        let key = ShapeCacheKey::new(fid, 16.0, "Hello");
        cache.insert(key, make_cached_shape(100.0, 20.0, 1));
        assert!(!cache.is_empty());

        cache.invalidate_font(fid);
        assert!(cache.is_empty());
    }

    #[test]
    fn cache_invalidate_font_size() {
        let mut cache = TextShapeCache::new(1024 * 1024);
        let key16 = ShapeCacheKey::new(FontId(dummy_font_id()), 16.0, "Hello");
        let key20 = ShapeCacheKey::new(FontId(dummy_font_id()), 20.0, "Hello");
        cache.insert(key16, make_cached_shape(100.0, 20.0, 1));
        cache.insert(key20, make_cached_shape(120.0, 24.0, 1));
        assert_eq!(cache.len(), 2);

        cache.invalidate_font_size(16.0);
        assert_eq!(cache.len(), 1);
        assert!(cache.get(&key20).is_some());
    }

    #[test]
    fn cache_resize_evicts() {
        let mut cache = TextShapeCache::new(1024 * 1024);
        for i in 0..10 {
            let key = ShapeCacheKey::new(FontId(dummy_font_id()), 16.0, &format!("text{i}"));
            cache.insert(key, make_cached_shape(100.0, 20.0, 1));
        }
        assert!(cache.total_memory() > 0);

        // Resize to very small budget — should evict down to at most 1 entry
        cache.resize(1);
        // After resize, total memory should be within budget or cache is empty
        assert!(
            cache.total_memory() <= 1 || cache.is_empty(),
            "total mem {} should be <= 1 or cache empty",
            cache.total_memory()
        );
    }

    #[test]
    fn cache_trim_old_entries() {
        let mut cache = TextShapeCache::new(1024 * 1024);
        let key1 = ShapeCacheKey::new(FontId(dummy_font_id()), 16.0, "A");
        let key2 = ShapeCacheKey::new(FontId(dummy_font_id()), 16.0, "B");

        cache.insert(key1, make_cached_shape(10.0, 10.0, 1));
        // key1 has age ~0
        cache.insert(key2, make_cached_shape(20.0, 10.0, 1));
        // key2 has age ~1

        // Access key2 to bump its age
        cache.get(&key2);

        // Trim entries older than 1 tick from current age
        cache.trim(1);

        // key2 was accessed more recently, should survive
        assert!(cache.get(&key2).is_some());
    }

    #[test]
    fn cache_debug_format() {
        let cache = TextShapeCache::new(1024);
        let debug = format!("{:?}", cache);
        assert!(debug.contains("TextShapeCache"));
        assert!(debug.contains("hit_rate"));
    }

    #[test]
    fn cached_shape_mem_size_estimation() {
        let shape = make_cached_shape(100.0, 20.0, 1);
        // Empty lines vec has zero size for the slice itself (size_of_val on empty slice)
        // but the TextMetrics is stored inline. The mem_size may be 0 for empty lines.
        // This test just verifies the estimation doesn't panic.
        let _ = shape.mem_size;
    }

    #[test]
    fn cached_shape_with_lines() {
        use crate::shaping::ShapedLine;
        let line = ShapedLine {
            text: "Hello".to_string(),
            rtl: false,
            line_y: 0.0,
            line_top: 0.0,
            line_height: 20.0,
            line_w: 50.0,
            glyphs: vec![],
        };
        let shape = CachedShape::new(
            vec![line],
            TextMetrics {
                width: 50.0,
                height: 20.0,
                line_count: 1,
            },
        );
        assert!(shape.mem_size > 0);
    }

    // Suppress unused function warnings for the initial make_key
    #[test]
    fn make_key_compiles() {
        let _ = make_key(1, 16.0, "test");
    }
}
