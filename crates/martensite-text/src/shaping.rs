//! Complex text shaping: BiDi, line breaking, font fallback chaining,
//! and text measurement via cosmic-text.
//!
//! The [`Shaper`] wraps a cosmic-text [`Buffer`] and provides high-level
//! methods for shaping text runs, measuring text, and extracting glyph
//! data for rendering. It handles Unicode Bidirectional Algorithm
//! reordering, line breaking, and font fallback automatically through
//! cosmic-text's `Shaping::Advanced` mode.

use cosmic_text::{Attrs, Buffer, Family, FontSystem, Metrics, Shaping};
use std::panic::{catch_unwind, AssertUnwindSafe};

use crate::bidi::{BidiDirection, BidiMirrorMap, BidiResolved};
use crate::cache::{CachedShape, ShapeCacheKey};
use crate::cascade::{
    classify_script, FallbackDecisionCache, FallbackKey, FontFallbackProvider,
    InstalledFontFallbackResolver, ScriptTag,
};
use crate::font::{FontId, FontManager};
use crate::vertical::{apply_vertical_features, WritingMode};

/// The result of measuring shaped text.
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct TextMetrics {
    /// The total width of the shaped text in logical pixels.
    pub width: f32,
    /// The total height of the shaped text in logical pixels.
    pub height: f32,
    /// The number of lines after wrapping.
    pub line_count: usize,
}

/// Direction and writing-mode options that affect shaping results.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct ShapingOptions {
    /// Base paragraph direction for BiDi resolution.
    pub direction: BidiDirection,
    /// Vertical or horizontal writing mode.
    pub writing_mode: WritingMode,
    /// Whether to enable vertical OpenType features when the writing mode is vertical.
    pub enable_vertical_features: bool,
    /// Optional culture key used for fallback chain resolution.
    pub fallback_key: Option<FallbackKey>,
}

impl ShapingOptions {
    /// Default horizontal LTR shaping options.
    #[inline]
    pub const fn default() -> Self {
        Self {
            direction: BidiDirection::Ltr,
            writing_mode: WritingMode::HorizontalTb,
            enable_vertical_features: true,
            fallback_key: None,
        }
    }

    /// Shaping options for a vertical right-to-left flow.
    #[inline]
    pub const fn vertical_rl() -> Self {
        Self {
            direction: BidiDirection::Ltr,
            writing_mode: WritingMode::VerticalRl,
            enable_vertical_features: true,
            fallback_key: None,
        }
    }

    /// Returns a display name for a [`Family`] selector suitable for the
    /// fallback resolver.
    fn family_display_name<'a>(family: &'a Family<'a>) -> &'a str {
        match family {
            Family::Name(name) => name,
            Family::Serif => "serif",
            Family::SansSerif => "sans-serif",
            Family::Cursive => "cursive",
            Family::Fantasy => "fantasy",
            Family::Monospace => "monospace",
        }
    }

    /// Resolves the fallback chain for `text` and `attrs` against the
    /// installed font database.
    ///
    /// The script used for the platform cascade is taken from
    /// [`Self::fallback_key`] when set, otherwise it is detected from the
    /// dominant script of `text`. When [`Self::fallback_key`] includes a
    /// locale, it is passed to the [`crate::cascade::FontFallbackProvider`] for
    /// locale-sensitive CJK and Indic variant selection.
    pub fn resolve_fallback_chain(
        &self,
        font_system: &FontSystem,
        text: &str,
        attrs: &Attrs,
    ) -> Vec<String> {
        self.resolve_fallback_chain_with_provider(font_system, text, attrs, None)
    }

    /// Like [`resolve_fallback_chain`](Self::resolve_fallback_chain) but
    /// uses the supplied [`FontFallbackProvider`] when `provider` is
    /// `Some`, falling back to the default [`PlatformCascadeResolver`](crate::cascade::PlatformCascadeResolver)
    /// when `None`.
    ///
    /// This is the entry point used by [`Shaper::shape_with_options`]
    /// when an OS-native provider has been injected via
    /// [`Shaper::set_fallback_provider`].
    pub fn resolve_fallback_chain_with_provider(
        &self,
        font_system: &FontSystem,
        text: &str,
        attrs: &Attrs,
        provider: Option<&dyn FontFallbackProvider>,
    ) -> Vec<String> {
        let resolver = match provider {
            Some(p) => InstalledFontFallbackResolver::with_provider(font_system, p),
            None => InstalledFontFallbackResolver::new(font_system),
        };
        if let Some(key) = &self.fallback_key {
            // Resolve against the culture-specific script from the key,
            // passing the locale for locale-sensitive fallback ordering.
            let locale = &key.locale;
            let mut chain = vec![Self::family_display_name(&attrs.family).to_string()];
            for family in resolver.installed_fallbacks_for_script_with_locale(key.script, locale) {
                if !chain.iter().any(|f| f.eq_ignore_ascii_case(&family)) {
                    chain.push(family);
                }
            }
            return chain;
        }
        resolver.resolve_for_text(text, Self::family_display_name(&attrs.family))
    }

    /// Builds a [`ShapeCacheKey`] for `text` under these options.
    ///
    /// The key incorporates the base BiDi direction, writing mode, and a
    /// hash of the resolved fallback chain so that shaped results are
    /// never conflated across typographic configurations.
    #[allow(clippy::too_many_arguments)]
    pub fn cache_key(
        &self,
        font_system: &FontSystem,
        font_id: FontId,
        font_size: f32,
        text: &str,
        max_width: Option<f32>,
        family: &str,
        line_height: f32,
        attrs: &Attrs,
    ) -> ShapeCacheKey {
        let chain = self.resolve_fallback_chain(font_system, text, attrs);
        let families: Vec<&str> = chain.iter().map(String::as_str).collect();
        ShapeCacheKey::with_options(
            font_id,
            font_size,
            text,
            max_width,
            family,
            line_height,
            self.direction,
            self.writing_mode,
            &families,
        )
    }
}

impl TextMetrics {
    /// Creates zero metrics.
    #[inline(always)]
    pub fn zero() -> Self {
        Self::default()
    }

    /// Returns `true` if either dimension is zero or negative.
    ///
    /// This is consistent with `martensite_layout::geometry::Size::is_empty`.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.width <= 0.0 || self.height <= 0.0
    }
}

/// A shaped glyph ready for rendering.
#[derive(Copy, Clone, Debug)]
pub struct ShapedGlyph {
    /// Start index in the original text.
    pub start: usize,
    /// End index in the original text.
    pub end: usize,
    /// Font face ID.
    pub font_id: FontId,
    /// Glyph ID within the font.
    pub glyph_id: u16,
    /// X position of the glyph's hitbox.
    pub x: f32,
    /// Y position of the glyph's hitbox.
    pub y: f32,
    /// Width of the glyph's hitbox.
    pub w: f32,
    /// Font size used for this glyph.
    pub font_size: f32,
    /// Unicode BiDi embedding level (even = LTR, odd = RTL).
    pub bidi_level: u8,
}

/// A single line of shaped text.
#[derive(Clone, Debug)]
pub struct ShapedLine {
    /// The original text of this line.
    pub text: String,
    /// Whether the paragraph direction is RTL.
    pub rtl: bool,
    /// Y offset to the baseline of this line.
    pub line_y: f32,
    /// Y offset to the top of this line.
    pub line_top: f32,
    /// The line height.
    pub line_height: f32,
    /// The width of this line.
    pub line_w: f32,
    /// The glyphs in this line.
    pub glyphs: Vec<ShapedGlyph>,
}

/// The text shaper, wrapping a cosmic-text [`Buffer`].
///
/// The shaper is reusable: call [`Shaper::set_text`] to change the
/// text, then [`Shaper::shape`] to perform shaping and line breaking,
/// and [`Shaper::measure`] or [`Shaper::lines`] to extract results.
///
/// # Examples
///
/// ```
/// use martensite_text::{Attrs, Family, FontManager, Metrics, Shaper};
///
/// let mut mgr = FontManager::new();
/// let mut shaper = Shaper::new(mgr.system_mut(), Metrics::new(16.0, 20.0));
/// let attrs = Attrs::new().family(Family::SansSerif);
/// shaper.set_text("Hello", &attrs);
/// shaper.shape(mgr.system_mut());
/// let metrics = shaper.measure();
/// assert!(metrics.width >= 0.0);
/// ```
pub struct Shaper {
    buffer: Buffer,
    /// Resolved base BiDi embedding level of the current text, if set.
    base_bidi_level: Option<u8>,
    /// Visual order indices of the resolved BiDi runs.
    visual_run_order: Vec<usize>,
    /// Mirrored-glyph positions for RTL runs of the current text.
    mirror_map: Option<BidiMirrorMap>,
    /// Resolved fallback chain for the current text, in priority order.
    fallback_chain: Vec<String>,
    /// Family names actually applied to buffer spans, in text order.
    ///
    /// Populated by [`Self::shape_with_options`] when the resolved
    /// fallback chain supplies per-script families; cleared otherwise.
    applied_families: Vec<String>,
    /// Writing mode used for the current text.
    writing_mode: WritingMode,
    /// Cache for resolved fallback chains, keyed by
    /// `(FallbackKey, primary_family)` and invalidated by the
    /// font-system generation counter. This avoids re-scanning the
    /// font database on every shaping call for the same text and
    /// family.
    fallback_cache: FallbackDecisionCache,
    /// The font-system generation counter last seen by the shaper.
    /// When this differs from the value passed to
    /// [`Self::set_font_generation`], the [`fallback_cache`](Self::fallback_cache)
    /// is invalidated.
    font_generation: u64,
    /// Optional injected font fallback provider. When set, this is
    /// used instead of the default [`PlatformCascadeResolver`](crate::cascade::PlatformCascadeResolver) for
    /// fallback chain resolution. This is the seam through which
    /// native OS providers (DirectWrite, CoreText, Fontconfig) are
    /// plugged in by the `martensite` crate when the `native-fallback`
    /// feature is enabled.
    fallback_provider: Option<Box<dyn FontFallbackProvider>>,
}

impl Shaper {
    /// Creates a new `Shaper` with the given font system and font metrics.
    pub fn new(font_system: &mut FontSystem, metrics: Metrics) -> Self {
        Self {
            buffer: Buffer::new(font_system, metrics),
            base_bidi_level: None,
            visual_run_order: Vec::new(),
            mirror_map: None,
            fallback_chain: Vec::new(),
            applied_families: Vec::new(),
            writing_mode: WritingMode::HorizontalTb,
            fallback_cache: FallbackDecisionCache::new(),
            font_generation: 0,
            fallback_provider: None,
        }
    }

    /// Creates a new `Shaper` with empty metrics (zero font size).
    /// Call [`Shaper::set_metrics`] before shaping.
    pub fn new_empty(metrics: Metrics) -> Self {
        Self {
            buffer: Buffer::new_empty(metrics),
            base_bidi_level: None,
            visual_run_order: Vec::new(),
            mirror_map: None,
            fallback_chain: Vec::new(),
            applied_families: Vec::new(),
            writing_mode: WritingMode::HorizontalTb,
            fallback_cache: FallbackDecisionCache::new(),
            font_generation: 0,
            fallback_provider: None,
        }
    }

    /// Records resolved BiDi metadata for `text` under `direction`.
    fn resolve_bidi(&mut self, text: &str, direction: BidiDirection) {
        if text.is_empty() {
            self.base_bidi_level = Some(match direction {
                BidiDirection::Rtl => 1,
                _ => 0,
            });
            self.visual_run_order.clear();
            self.mirror_map = Some(BidiMirrorMap::new(text));
            return;
        }
        let resolved = BidiResolved::new(text, direction);
        self.base_bidi_level = Some(resolved.base_level());
        self.visual_run_order = resolved
            .visual_runs()
            .iter()
            .map(|run| run.visual_order)
            .collect();
        self.mirror_map = Some(resolved.mirror_map());
    }

    /// Sets the font size and line height.
    pub fn set_metrics(&mut self, font_size: f32, line_height: f32) {
        let metrics = Metrics::new(font_size, line_height);
        self.buffer.set_metrics(metrics);
    }

    /// Sets the available width and height for wrapping.
    ///
    /// `None` means unbounded (no wrapping on that axis).
    pub fn set_size(&mut self, width: Option<f32>, height: Option<f32>) {
        self.buffer.set_size(width, height);
    }

    /// Sets the font-system generation counter used to invalidate the
    /// internal [`FallbackDecisionCache`].
    ///
    /// Obtain this value from [`FontManager::generation`]. When the
    /// generation changes (e.g. after loading a new font), all cached
    /// fallback decisions are invalidated so the next shaping call
    /// re-resolves against the updated font database.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_text::{FontManager, Metrics, Shaper};
    ///
    /// let mut mgr = FontManager::new();
    /// let mut shaper = Shaper::new_empty(Metrics::new(16.0, 20.0));
    /// shaper.set_font_generation(mgr.generation());
    /// ```
    #[inline]
    pub fn set_font_generation(&mut self, generation: u64) {
        self.font_generation = generation;
    }

    /// Injects a custom [`FontFallbackProvider`] to use for fallback
    /// chain resolution.
    ///
    /// When set, this provider is used instead of the default
    /// [`PlatformCascadeResolver`](crate::cascade::PlatformCascadeResolver). This is the seam through which
    /// native OS providers (DirectWrite, CoreText, Fontconfig) are
    /// plugged in by the `martensite` crate when the `native-fallback`
    /// feature is enabled.
    ///
    /// Pass `None` to revert to the default platform resolver.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_text::{Metrics, Shaper};
    /// use martensite_text::cascade::{FontFallbackProvider, PlatformCascadeResolver};
    ///
    /// let mut shaper = Shaper::new_empty(Metrics::new(16.0, 20.0));
    /// shaper.set_fallback_provider(Some(Box::new(PlatformCascadeResolver)));
    /// ```
    #[inline]
    pub fn set_fallback_provider(&mut self, provider: Option<Box<dyn FontFallbackProvider>>) {
        self.fallback_provider = provider;
        // Clear the cache so stale decisions from the previous provider
        // are not reused.
        self.fallback_cache.clear();
    }

    /// Resolves the fallback chain for `text` and `attrs` under
    /// `options`, using the internal [`FallbackDecisionCache`] to
    /// avoid re-scanning the font database on repeated calls for the
    /// same `(FallbackKey, primary_family)` combination.
    ///
    /// The cache is keyed by `(FallbackKey, primary_family)` and
    /// invalidated by the font-system generation counter set via
    /// [`Self::set_font_generation`]. When a provider has been
    /// injected via [`Self::set_fallback_provider`], it is used
    /// instead of the default [`PlatformCascadeResolver`](crate::cascade::PlatformCascadeResolver).
    ///
    /// On a cache miss, the chain is resolved (delegating to
    /// [`ShapingOptions::resolve_fallback_chain_with_provider`]) and
    /// inserted into the cache. On a cache hit, the cached chain is
    /// returned directly.
    fn resolve_fallback_chain_cached(
        &mut self,
        font_system: &FontSystem,
        text: &str,
        attrs: &Attrs,
        options: &ShapingOptions,
    ) -> Vec<String> {
        let primary_family = ShapingOptions::family_display_name(&attrs.family).to_string();

        // Determine the FallbackKey for this resolution.
        let key = match &options.fallback_key {
            Some(k) => k.clone(),
            None => {
                let script = if text.is_empty() {
                    ScriptTag::Latin
                } else {
                    crate::cascade::dominant_script(text)
                };
                FallbackKey::new(script, "")
            }
        };

        // Check the cache first.
        if let Some(cached) = self
            .fallback_cache
            .get(&key, &primary_family, self.font_generation)
        {
            return cached.clone();
        }

        // Cache miss: resolve the chain using the injected provider
        // (if any) or the default platform resolver.
        let chain = options.resolve_fallback_chain_with_provider(
            font_system,
            text,
            attrs,
            self.fallback_provider.as_deref(),
        );

        self.fallback_cache
            .insert(&key, &primary_family, chain.clone(), self.font_generation);
        chain
    }

    /// Sets the text to be shaped with the given attributes.
    ///
    /// Uses `Shaping::Advanced` for full BiDi, fallback, and complex
    /// script support. BiDi metadata is resolved with automatic base
    /// direction detection.
    pub fn set_text(&mut self, text: &str, attrs: &Attrs) {
        self.resolve_bidi(text, BidiDirection::Auto);
        self.fallback_chain.clear();
        self.applied_families.clear();
        self.writing_mode = WritingMode::HorizontalTb;
        self.buffer.set_text(text, attrs, Shaping::Advanced, None);
    }

    /// Performs shaping and line breaking up to the current cursor
    /// or the entire buffer.
    ///
    /// After calling this, [`Shaper::measure`] and [`Shaper::lines`]
    /// return the final results.
    ///
    /// The underlying cosmic-text shaping engine delegates to `swash`
    /// for glyph parsing, which has known panic paths on malformed font
    /// data (swash issues #123–#126). This call is wrapped in
    /// [`catch_unwind`] so a corrupt or adversarial font cannot abort
    /// the calling thread; on panic the buffer is left unshaped
    /// (yielding zero metrics) and a warning is logged.
    pub fn shape(&mut self, font_system: &mut FontSystem) {
        let result = catch_unwind(AssertUnwindSafe(|| {
            self.buffer.shape_until_scroll(font_system, false);
        }));
        if result.is_err() {
            tracing::warn!(
                "swash shaping panicked on potentially malformed font data; \
                 returning unshaped buffer"
            );
        }
    }

    /// Measures the shaped text, returning the total width, height,
    /// and line count.
    ///
    /// Must be called after [`Shaper::shape`].
    pub fn measure(&self) -> TextMetrics {
        let mut max_width = 0.0f32;
        let mut total_height = 0.0f32;
        let mut line_count = 0usize;

        for run in self.buffer.layout_runs() {
            let run_w = run.line_w.max(
                run.glyphs
                    .iter()
                    .map(|g| g.x + g.w)
                    .max_by(f32::total_cmp)
                    .unwrap_or(0.0),
            );
            max_width = max_width.max(run_w);
            total_height += run.line_height;
            line_count += 1;
        }

        TextMetrics {
            width: max_width,
            height: total_height,
            line_count,
        }
    }

    /// Returns the shaped lines as [`ShapedLine`]s.
    ///
    /// Must be called after [`Shaper::shape`].
    pub fn lines(&self) -> Vec<ShapedLine> {
        self.buffer
            .layout_runs()
            .map(|run| ShapedLine {
                text: run.text.to_string(),
                rtl: run.rtl,
                line_y: run.line_y,
                line_top: run.line_top,
                line_height: run.line_height,
                line_w: run.line_w,
                glyphs: run
                    .glyphs
                    .iter()
                    .map(|g| ShapedGlyph {
                        start: g.start,
                        end: g.end,
                        font_id: FontId(g.font_id),
                        glyph_id: g.glyph_id,
                        x: g.x,
                        y: g.y,
                        w: g.w,
                        font_size: g.font_size,
                        bidi_level: g.level.number(),
                    })
                    .collect(),
            })
            .collect()
    }

    /// Borrows the underlying cosmic-text [`Buffer`].
    #[inline(always)]
    pub fn buffer(&self) -> &Buffer {
        &self.buffer
    }

    /// Mutably borrows the underlying cosmic-text [`Buffer`].
    #[inline(always)]
    pub fn buffer_mut(&mut self) -> &mut Buffer {
        &mut self.buffer
    }

    /// Convenience: shape text and return metrics in one call.
    pub fn measure_text(
        &mut self,
        font_system: &mut FontSystem,
        text: &str,
        attrs: &Attrs,
        font_size: f32,
        line_height: f32,
        max_width: Option<f32>,
    ) -> TextMetrics {
        self.set_metrics(font_size, line_height);
        self.set_size(max_width, None);
        self.set_text(text, attrs);
        self.shape(font_system);
        self.measure()
    }

    /// Computes the attributes passed to the buffer for `options`.
    ///
    /// When `options.writing_mode` is vertical and
    /// `options.enable_vertical_features` is true, the vertical OpenType
    /// feature tags (`vert`, `vrt2`, `vkrn`) are enabled on the
    /// attributes.
    fn effective_attrs<'a>(attrs: &Attrs<'a>, options: &ShapingOptions) -> Attrs<'a> {
        if options.writing_mode.is_vertical() && options.enable_vertical_features {
            apply_vertical_features(attrs.clone())
        } else {
            attrs.clone()
        }
    }

    /// Sets the text to be shaped, applying direction and vertical-feature options.
    ///
    /// When `options.writing_mode` is vertical and `options.enable_vertical_features`
    /// is true, the vertical OpenType feature tags (`vert`, `vrt2`, `vkrn`) are
    /// enabled on the attributes before passing them to the buffer.
    ///
    /// Note: this entry point does not apply the resolved installed-font
    /// fallback chain because it has no access to the [`FontSystem`].
    /// Prefer [`Self::shape_with_options`], which wires the resolved
    /// per-script fallback families into the buffer's attribute spans.
    pub fn set_text_with_options(&mut self, text: &str, attrs: &Attrs, options: &ShapingOptions) {
        self.resolve_bidi(text, options.direction);
        self.writing_mode = options.writing_mode;
        self.applied_families.clear();
        let effective_attrs = Self::effective_attrs(attrs, options);
        self.buffer
            .set_text(text, &effective_attrs, Shaping::Advanced, None);
    }

    /// Splits `text` into contiguous script runs.
    ///
    /// Returns `(start, end, script)` byte-range triples. Neutral
    /// characters (whitespace, controls, and [`ScriptTag::Other`]) are
    /// absorbed into the surrounding run so that e.g. spaces between
    /// words do not create single-character spans. A leading run of
    /// neutral characters is assigned the script of the first
    /// non-neutral run that follows it (falling back to `Latin` when
    /// the entire text is neutral).
    fn script_runs(text: &str) -> Vec<(usize, usize, ScriptTag)> {
        let mut runs: Vec<(usize, usize, ScriptTag)> = Vec::new();
        for (idx, ch) in text.char_indices() {
            let tag = classify_script(ch);
            let neutral = ch.is_whitespace() || ch.is_control() || tag == ScriptTag::Other;
            match runs.last_mut() {
                Some((_, end, last)) if *last == tag || neutral => *end = idx + ch.len_utf8(),
                _ => {
                    let effective = if neutral {
                        // Look ahead past neutrals so a leading neutral run
                        // adopts the following script rather than Latin.
                        text[idx..]
                            .chars()
                            .find(|c| {
                                !c.is_whitespace()
                                    && !c.is_control()
                                    && classify_script(*c) != ScriptTag::Other
                            })
                            .map(classify_script)
                            .unwrap_or(ScriptTag::Latin)
                    } else {
                        tag
                    };
                    runs.push((idx, idx + ch.len_utf8(), effective));
                }
            }
        }
        runs
    }

    /// Performs shaping and line breaking with direction/writing-mode aware options.
    ///
    /// In addition to BiDi-aware shaping, this resolves the installed-font
    /// fallback chain for the text (see
    /// [`ShapingOptions::resolve_fallback_chain`]) and stores it for
    /// retrieval via [`Self::fallback_chain`] and [`Self::cached_shape`].
    ///
    /// # How the resolved chain reaches the shaper
    ///
    /// cosmic-text's [`Attrs`] supports exactly one [`Family`] selector,
    /// and its internal [`FontFallbackIter`] is driven by the platform
    /// fallback tables inside the vendored `FontSystem`, not by
    /// user-supplied chains. The supported mechanism for steering font
    /// selection per script is therefore [`Buffer::set_rich_text`] with
    /// an [`cosmic_text::AttrsList`]-style per-span `family`: this method
    /// splits the text into contiguous script runs (via
    /// [`classify_script`]) and assigns each run the first family in the
    /// resolved chain whose installed faces cover that run's
    /// codepoints. Runs without a covering candidate keep `attrs.family`
    /// so cosmic-text's internal per-word fallback still applies. The
    /// families actually applied are recorded in
    /// [`Self::applied_families`].
    ///
    /// [`FontFallbackIter`]: cosmic_text::FontSystem
    pub fn shape_with_options(
        &mut self,
        font_system: &mut FontSystem,
        text: &str,
        attrs: &Attrs,
        options: &ShapingOptions,
    ) {
        self.fallback_chain = self.resolve_fallback_chain_cached(font_system, text, attrs, options);
        self.resolve_bidi(text, options.direction);
        self.writing_mode = options.writing_mode;
        self.applied_families.clear();

        let effective_attrs = Self::effective_attrs(attrs, options);
        let default_family =
            ShapingOptions::family_display_name(&effective_attrs.family).to_string();

        if text.is_empty() || self.fallback_chain.len() <= 1 {
            // No usable fallback chain: plain set_text path. cosmic-text
            // still performs its own internal per-word fallback.
            self.buffer
                .set_text(text, &effective_attrs, Shaping::Advanced, None);
            self.shape(font_system);
            return;
        }

        let resolver = InstalledFontFallbackResolver::new(font_system);
        // Group contiguous script runs by the resolved family that should
        // shape them. `None` means "keep the requested family and let
        // cosmic-text's internal fallback handle it".
        let mut run_family: Vec<(usize, usize, Option<usize>)> = Vec::new();
        for (start, end, _script) in Self::script_runs(text) {
            let run_text = &text[start..end];
            // First family in the resolved chain (which begins with the
            // requested primary) whose installed faces cover this run.
            let chosen = self
                .fallback_chain
                .iter()
                .position(|family| resolver.family_covers_text(family, run_text));
            match run_family.last_mut() {
                Some((_, prev_end, prev)) if *prev == chosen => *prev_end = end,
                _ => run_family.push((start, end, chosen)),
            }
        }

        let chain = self.fallback_chain.clone();
        let mut spans: Vec<(&str, Attrs<'_>)> = Vec::new();
        let mut applied: Vec<String> = Vec::new();
        for (start, end, family_idx) in run_family {
            let (name, span_attrs) = match family_idx {
                Some(i) if !chain[i].eq_ignore_ascii_case(&default_family) => (
                    chain[i].clone(),
                    effective_attrs
                        .clone()
                        .family(Family::Name(chain[i].as_str())),
                ),
                _ => (default_family.clone(), effective_attrs.clone()),
            };
            spans.push((&text[start..end], span_attrs));
            applied.push(name);
        }

        self.applied_families = applied;
        self.buffer
            .set_rich_text(spans, &effective_attrs, Shaping::Advanced, None);
        self.shape(font_system);
    }

    /// Returns the resolved base BiDi embedding level of the current text
    /// (even = LTR, odd = RTL), if text has been set.
    pub fn base_bidi_level(&self) -> Option<u8> {
        self.base_bidi_level
    }

    /// Returns the visual order indices of the resolved BiDi runs of the
    /// current text.
    ///
    /// The index at position `i` is the visual slot of the `i`-th logical
    /// run. Empty when no text has been set.
    pub fn visual_run_order(&self) -> &[usize] {
        &self.visual_run_order
    }

    /// Returns the mirrored-glyph map for the current text, if any.
    ///
    /// Apply these substitutions at glyph rasterization time; the source
    /// text is never mutated.
    pub fn mirror_map(&self) -> Option<&BidiMirrorMap> {
        self.mirror_map.as_ref()
    }

    /// Returns the fallback chain resolved for the current text.
    ///
    /// Populated by [`Self::shape_with_options`]; empty otherwise.
    pub fn fallback_chain(&self) -> &[String] {
        &self.fallback_chain
    }

    /// Returns the family names actually applied to buffer spans during
    /// the last [`Self::shape_with_options`] call, in text order.
    ///
    /// Each entry is the [`Family::Name`] assigned to that span (or the
    /// requested family when no resolved fallback covered the run).
    /// Empty when the plain [`Self::set_text`] path was used.
    pub fn applied_families(&self) -> &[String] {
        &self.applied_families
    }

    /// Builds a [`ShapeCacheKey`] for `text` under `options`.
    ///
    /// See [`ShapingOptions::cache_key`].
    #[allow(clippy::too_many_arguments)]
    pub fn cache_key(
        &self,
        font_system: &FontSystem,
        font_id: FontId,
        font_size: f32,
        text: &str,
        max_width: Option<f32>,
        family: &str,
        line_height: f32,
        attrs: &Attrs,
        options: &ShapingOptions,
    ) -> ShapeCacheKey {
        options.cache_key(
            font_system,
            font_id,
            font_size,
            text,
            max_width,
            family,
            line_height,
            attrs,
        )
    }

    /// Builds a [`CachedShape`] from the current shaped buffer and the
    /// BiDi/fallback metadata recorded during shaping.
    ///
    /// Must be called after [`Shaper::shape`].
    pub fn cached_shape(&self) -> CachedShape {
        CachedShape::with_metadata(
            self.lines(),
            self.measure(),
            self.base_bidi_level.unwrap_or(0),
            self.visual_run_order.clone(),
            self.fallback_chain.clone(),
            self.writing_mode,
        )
    }
}

/// Convenience function to measure text using a [`FontManager`].
///
/// Creates a temporary [`Shaper`], shapes the text, and returns the
/// metrics. For repeated measurements, reuse a [`Shaper`] instance.
pub fn measure_text(
    manager: &mut FontManager,
    text: &str,
    font_size: f32,
    line_height: f32,
    max_width: Option<f32>,
) -> TextMetrics {
    measure_text_with_attrs(
        manager,
        text,
        &Attrs::new(),
        font_size,
        line_height,
        max_width,
    )
}

/// Convenience function to measure text with explicit attributes
/// (font family, direction, weight, etc.).
pub fn measure_text_with_attrs(
    manager: &mut FontManager,
    text: &str,
    attrs: &Attrs,
    font_size: f32,
    line_height: f32,
    max_width: Option<f32>,
) -> TextMetrics {
    let mut shaper = Shaper::new_empty(Metrics::new(font_size, line_height));
    shaper.set_size(max_width, None);
    shaper.set_text(text, attrs);
    shaper.shape(manager.system_mut());
    shaper.measure()
}

/// Convenience function to shape text and extract lines using a
/// [`FontManager`].
pub fn shape_text(
    manager: &mut FontManager,
    text: &str,
    font_size: f32,
    line_height: f32,
    max_width: Option<f32>,
) -> Vec<ShapedLine> {
    let mut shaper = Shaper::new_empty(Metrics::new(font_size, line_height));
    shaper.set_size(max_width, None);
    shaper.set_text(text, &Attrs::new());
    shaper.shape(manager.system_mut());
    shaper.lines()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_metrics_zero() {
        let m = TextMetrics::zero();
        assert_eq!(m.width, 0.0);
        assert_eq!(m.height, 0.0);
        assert_eq!(m.line_count, 0);
        assert!(m.is_empty());
    }

    #[test]
    fn text_metrics_non_empty() {
        let m = TextMetrics {
            width: 100.0,
            height: 20.0,
            line_count: 1,
        };
        assert!(!m.is_empty());
    }

    #[test]
    fn shaper_new_empty() {
        let shaper = Shaper::new_empty(Metrics::new(16.0, 20.0));
        // Empty shaper should measure zero
        let m = shaper.measure();
        assert_eq!(m.line_count, 0);
    }

    #[test]
    fn shaper_measure_empty_text() {
        let mut manager = FontManager::with_fonts(std::iter::empty());
        let metrics = measure_text(&mut manager, "", 16.0, 20.0, None);
        // Empty text may still produce one empty line in cosmic-text
        assert!(
            metrics.line_count <= 1,
            "empty text should have 0 or 1 lines"
        );
        // Width should be zero for empty text
        assert_eq!(metrics.width, 0.0);
    }

    #[test]
    fn shaper_measure_simple_text() {
        let mut manager = FontManager::new();
        let metrics = measure_text(&mut manager, "Hello", 16.0, 20.0, None);
        // Should have at least one line
        assert!(
            metrics.line_count >= 1,
            "should have at least 1 line, got {}",
            metrics.line_count
        );
        // Width should be positive (if a font was found)
        if metrics.width > 0.0 {
            assert!(metrics.height > 0.0);
        }
    }

    #[test]
    fn shaper_measure_with_wrapping() {
        let mut manager = FontManager::new();
        // Long text with narrow width should wrap to multiple lines
        let metrics = measure_text(
            &mut manager,
            "The quick brown fox jumps over the lazy dog repeatedly",
            16.0,
            20.0,
            Some(50.0),
        );
        // With a 50px width, this should wrap to multiple lines (if fonts available)
        if metrics.width > 0.0 {
            assert!(
                metrics.line_count > 1,
                "expected wrapping, got {} lines",
                metrics.line_count
            );
        }
    }

    #[test]
    fn shaper_measure_multilingual() {
        let mut manager = FontManager::new();
        // Mix of LTR and RTL text
        let texts = ["Hello", "مرحبا", "你好", "Привет", "こんにちは"];
        for text in &texts {
            let metrics = measure_text(&mut manager, text, 16.0, 20.0, None);
            // Should not panic; may have zero width if no font covers the script
            let _ = metrics;
        }
    }

    #[test]
    fn shape_text_returns_lines() {
        let mut manager = FontManager::new();
        let lines = shape_text(&mut manager, "Hello World", 16.0, 20.0, None);
        if !lines.is_empty() {
            let first = &lines[0];
            assert!(!first.text.is_empty());
        }
    }

    #[test]
    fn shaped_glyph_bidi_level() {
        let mut manager = FontManager::new();
        let lines = shape_text(&mut manager, "Hello", 16.0, 20.0, None);
        for line in &lines {
            for glyph in &line.glyphs {
                // LTR text should have even bidi level
                assert_eq!(
                    glyph.bidi_level % 2,
                    0,
                    "LTR text should have even bidi level"
                );
            }
        }
    }

    #[test]
    fn shaping_options_default_horizontal_ltr() {
        let opts = ShapingOptions::default();
        assert_eq!(opts.direction, BidiDirection::Ltr);
        assert!(opts.writing_mode.is_horizontal());
        assert!(!opts.writing_mode.is_vertical());
    }

    #[test]
    fn shaper_applies_vertical_features() {
        let mut manager = FontManager::with_fonts(std::iter::empty());
        let mut shaper = Shaper::new_empty(Metrics::new(16.0, 20.0));
        let attrs = Attrs::new();
        let mut options = ShapingOptions::vertical_rl();
        options.enable_vertical_features = true;
        // This should not panic even with no installed fonts.
        shaper.shape_with_options(manager.system_mut(), "漢字A", &attrs, &options);
    }

    #[test]
    fn shaper_base_bidi_level_rtl() {
        let mut manager = FontManager::with_fonts(std::iter::empty());
        let mut shaper = Shaper::new_empty(Metrics::new(16.0, 20.0));
        let attrs = Attrs::new();
        let options = ShapingOptions {
            direction: BidiDirection::Rtl,
            writing_mode: WritingMode::HorizontalTb,
            enable_vertical_features: false,
            fallback_key: None,
        };
        shaper.shape_with_options(manager.system_mut(), "مرحبا", &attrs, &options);
        // The base level should be odd (RTL) when shaping explicit RTL text.
        assert_eq!(shaper.base_bidi_level(), Some(1));
    }

    #[test]
    fn shaper_reuse() {
        let mut manager = FontManager::new();
        let mut shaper = Shaper::new(manager.system_mut(), Metrics::new(16.0, 20.0));

        // First text
        shaper.set_text("First", &Attrs::new());
        shaper.shape(manager.system_mut());
        let m1 = shaper.measure();

        // Second text - shaper should be reusable
        shaper.set_text("Second text that is longer", &Attrs::new());
        shaper.shape(manager.system_mut());
        let m2 = shaper.measure();

        // Both should produce valid results
        let _ = (m1, m2);
    }

    #[test]
    fn shaper_set_size_affects_wrapping() {
        let mut manager = FontManager::new();
        let mut shaper = Shaper::new(manager.system_mut(), Metrics::new(16.0, 20.0));

        // Unbounded width
        shaper.set_size(None, None);
        shaper.set_text("The quick brown fox", &Attrs::new());
        shaper.shape(manager.system_mut());
        let unbounded = shaper.measure();

        // Bounded width
        shaper.set_size(Some(30.0), None);
        shaper.set_text("The quick brown fox", &Attrs::new());
        shaper.shape(manager.system_mut());
        let bounded = shaper.measure();

        // Bounded should have more lines or equal (if no font found)
        if unbounded.line_count > 0 && bounded.line_count > 0 {
            assert!(
                bounded.line_count >= unbounded.line_count,
                "bounded width should have >= lines than unbounded"
            );
        }
    }
}
