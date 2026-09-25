# Cookbook 11 — Localization, BiDi Mirroring & Font Cascades

Internationalizing modern desktop and embedded applications requires far more than substituting English
strings with translated text. Complete global readiness requires pluralization and parameter interpolation,
Bidirectional (BiDi) layout mirroring for Right-to-Left (RTL) scripts (Arabic, Hebrew, Persian), Unicode
vertical text orientation (UAX #50) for East Asian typography, and multi-tier platform font fallback
to guarantee that no character renders as an unreadable missing-glyph square ("tofu").

This recipe demonstrates how to integrate `martensite-l10n`, `martensite-text`, and native platform
font cascades (`martensite-font-fallback`) to build truly global, reactive user interfaces.

---

## 1. Goal

Build an internationalized application view that:
1. Translates text reactively using Project Fluent (`martensite-l10n::reactive::L10n`), supporting dynamic parameters and plural forms.
2. Inverts layout geometry automatically for RTL locales (converting `Start`/`End` margins, flex direction, and icon alignment) per Unicode BiDi (UAX #9).
3. Renders vertical text runs correctly using Unicode UAX #50 (`WritingMode::VerticalRl`) with OpenType vertical layout features (`vert`, `vkrn`).
4. Resolves missing glyphs across system font boundaries using native platform font cascades (`DirectWriteFontFallback`, `CoreTextFontFallback`, `FontconfigFontFallback`).
5. Switches active locales at runtime without rebuilding the widget tree or disturbing focus state.

---

## 2. Complete Runnable Pattern

The following pattern constructs an internationalized user profile card. It defines Fluent bundles in English (`en`), Arabic (`ar` for RTL), and Japanese (`ja` for vertical text demonstration). When the locale is switched, text values update reactively through signals, layout automatically mirrors for RTL, and font fallbacks ensure missing glyphs render cleanly.

```rust
use fluent_bundle::FluentArgs;
use glam::Vec2;
use kurbo::{Point, Rect as KurboRect, RoundedRect};
use martensite::prelude::*;
use martensite_l10n::direction::{direction_for_locale, ScriptDirection};
use martensite_l10n::reactive::L10n;
use martensite_l10n::LanguageIdentifier;
use martensite_reactive::{create_memo, flush, Memo, Signal};
use martensite_text::cascade::{
    classify_script, FallbackDecisionCache, FontFallbackChain, PlatformCascadeResolver, ScriptTag,
};
use martensite_text::vertical::{
    apply_vertical_features, classify_vertical_orientation, VerticalOrientation, WritingMode,
};
use martensite_text::{shape_text_with_attrs, Attrs, Family, FontSystem, Metrics, ShapingOptions};
use std::sync::Arc;

/// A responsive, BiDi-aware profile card supporting dynamic translation and layout mirroring.
pub struct InternationalizedProfileCard {
    l10n: Arc<L10n>,
    cached_bounds: Rect,
    user_name: String,
    unread_messages: Signal<usize>,
    // Localized reactive memos
    welcome_text: Memo<String>,
    messages_badge_text: Memo<String>,
}

impl InternationalizedProfileCard {
    pub fn new(l10n: Arc<L10n>, user_name: String, initial_messages: usize) -> Self {
        let unread_messages = Signal::new(initial_messages);

        // Memo 1: Localized welcome header with interpolated username parameter
        let welcome_text = {
            let l10n = l10n.clone();
            let name = user_name.clone();
            create_memo(move || {
                let mut args = FluentArgs::new();
                args.set("name", name.clone());
                l10n.get_with_args("welcome-user", &args)
                    .unwrap_or_else(|| format!("Welcome, {}", name))
            })
        };

        // Memo 2: Localized message count with pluralization rule
        let messages_badge_text = {
            let l10n = l10n.clone();
            let count_sig = unread_messages.clone();
            create_memo(move || {
                let count = count_sig.get();
                let mut args = FluentArgs::new();
                args.set("count", count);
                l10n.get_with_args("unread-messages", &args)
                    .unwrap_or_else(|| format!("{} messages", count))
            })
        };

        Self {
            l10n,
            cached_bounds: Rect::default(),
            user_name,
            unread_messages,
            welcome_text,
            messages_badge_text,
        }
    }

    /// Dynamically update message count, triggering reactive string updates
    pub fn set_messages(&self, count: usize) {
        self.unread_messages.set(count);
    }
}

impl Widget for InternationalizedProfileCard {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let desired = Vec2::new(cx.pt(300.0), cx.pt(80.0));
        Vec2::new(
            desired.x.clamp(constraints.min_size.x, constraints.max_size.x),
            desired.y.clamp(constraints.min_size.y, constraints.max_size.y),
        )
    }

    fn layout(&mut self, _cx: &mut LayoutContext, bounds: Rect) {
        self.cached_bounds = bounds;
    }

    fn paint(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        let k_rect = KurboRect::new(
            b.min_x() as f64,
            b.min_y() as f64,
            b.max_x() as f64,
            b.max_y() as f64,
        );

        // 1. Draw card background
        cx.list.push_fill_rounded_rect(k_rect, cx.pt(8.0), [32, 34, 40, 255]);
        cx.list.push_stroke_rect(k_rect, [60, 65, 78, 255], cx.pt(1.0));

        // 2. Query script direction for BiDi spatial positioning
        let is_rtl = self.l10n.direction().is_rtl();

        // 3. Compute mirrored horizontal coordinates
        let pad = cx.pt(16.0);
        let avatar_size = cx.pt(40.0);

        let (avatar_x, text_x) = if is_rtl {
            // RTL: Avatar is pinned to the RIGHT edge; text starts to the left of the avatar
            (
                b.origin.x + b.size.x - pad - avatar_size,
                b.origin.x + b.size.x - pad - avatar_size - cx.pt(12.0),
            )
        } else {
            // LTR: Avatar is pinned to the LEFT edge; text starts to the right of the avatar
            (
                b.origin.x + pad,
                b.origin.x + pad + avatar_size + cx.pt(12.0),
            )
        };

        // Draw Avatar circular placeholder
        let avatar_rect = KurboRect::new(
            avatar_x as f64,
            (b.origin.y + pad) as f64,
            (avatar_x + avatar_size) as f64,
            (b.origin.y + pad + avatar_size) as f64,
        );
        cx.list.push_fill_rounded_rect(avatar_rect, (avatar_size * 0.5) as f64, [70, 110, 190, 255]);

        // 4. Render localized text runs
        let welcome = self.welcome_text.get();
        let badge = self.messages_badge_text.get();

        cx.list.push_text(
            Point::new(text_x as f64, (b.origin.y + cx.pt(32.0)) as f64),
            welcome,
            cx.pt(14.0),
            [240, 245, 255, 255],
        );

        cx.list.push_text(
            Point::new(text_x as f64, (b.origin.y + cx.pt(54.0)) as f64),
            badge,
            cx.pt(11.0),
            [160, 175, 195, 255],
        );
    }
}

/// Helper function to initialize multi-locale Fluent resources
pub fn setup_localization_controller() -> Arc<L10n> {
    let l10n = Arc::new(L10n::new("en".parse().unwrap()));

    // 1. English Bundle (LTR)
    l10n.add_bundle(
        "en".parse().unwrap(),
        vec![
            r#"
welcome-user = Welcome back, {$name}!
unread-messages = {$count ->
    [0] No new messages
    [one] 1 unread message
   *[other] {$count} unread messages
}
"#
            .to_string(),
        ],
    )
    .expect("en bundle valid");

    // 2. Arabic Bundle (RTL, Arabic plural categories: zero, one, two, few, many, other)
    l10n.add_bundle(
        "ar".parse().unwrap(),
        vec![
            r#"
welcome-user = !مرحباً بعودتك، {$name}
unread-messages = {$count ->
    [0] لا توجد رسائل جديدة
    [one] رسالة واحدة غير مقروءة
    [two] رسالتان غير مقروءتين
    [few] {$count} رسائل غير مقروءة
    [many] {$count} رسالة غير مقروءة
   *[other] {$count} رسالة غير مقروءة
}
"#
            .to_string(),
        ],
    )
    .expect("ar bundle valid");

    // 3. Japanese Bundle (CJK, default plural category: other)
    l10n.add_bundle(
        "ja".parse().unwrap(),
        vec![
            r#"
welcome-user = おかえりなさい、{$name}さん！
unread-messages = {$count ->
    [0] 新着メッセージはありません
   *[other] {$count}件の未読メッセージ
}
"#
            .to_string(),
        ],
    )
    .expect("ja bundle valid");

    l10n
}
```

---

## 3. Key Architectural Invariants

### 1. Zero-Structural-Rebuild Reactive Locale Switching
Traditional desktop frameworks handle language changes by reloading views, destroying active windows, or reconstructing the entire widget tree. This erases focus state, resets scroll offsets, and causes visual flicker.

Martensite achieves **zero-rebuild locale transitions**:
```
[ User selects Arabic (ar) ]
              │
              ▼
   L10n::set_locale("ar")
              │
              ▼
 Root Locale Signal updates
              │
              ├──> welcome_text: Memo<String> re-evaluates
              │         └── Marks HotNode::flags |= DIRTY_PAINT
              │
              └──> messages_badge_text: Memo<String> re-evaluates
                        └── Marks HotNode::flags |= DIRTY_PAINT
              │
              ▼
   Frame Render: Only text strings and paint commands update!
   WidgetId and arena generational indices remain completely untouched.
```
- Only widgets actively observing localized `Memo`s or `l10n.direction()` set their dirty bits (`NodeFlags::DIRTY_PAINT` or `NodeFlags::DIRTY_LAYOUT`).
- Generational `WidgetId`s, event listeners, focus handles, and animation states survive completely uninterrupted.

### 2. Unicode Bidirectional Algorithm (UAX #9) & Spatial Mirroring
When rendering text and composing layouts across LTR and RTL scripts:
- **Text-Level BiDi**: Handled by `martensite-text::bidi` and `cosmic-text`. Even in an English sentence, an embedded Arabic word is shaped from right to left; in an Arabic paragraph, an English technical term or number runs left to right.
- **Layout-Level Mirroring**: Layout containers must flip their spatial axis:
  - `InlineStart` maps to **Left** in LTR, but **Right** in RTL.
  - `InlineEnd` maps to **Right** in LTR, but **Left** in RTL.
  - Direction arrows ($\rightarrow$ vs $\leftarrow$), chevron expanders, and split-pane ordering mirror horizontally.
  - **Exemptions from Mirroring**: Media playback scrub bars, clock dials, volume sliders, and physical hardware dials do *not* mirror in RTL.

### 3. Unicode Vertical Text Layout (UAX #50)
For traditional East Asian documents (Japanese *tate-chū-yoko*, Chinese, or Mongolian calligraphy), text advances vertically downwards along columns running right-to-left (`WritingMode::VerticalRl`):

| Property | Horizontal Text | Vertical Text (`VerticalRl`) |
|---|---|---|
| **Inline Advance Axis** | Horizontal ($+X$) | Vertical ($+Y$) |
| **Block Progression Axis** | Vertical ($+Y$) | Horizontal ($-X$) |
| **CJK Ideographs** | Upright orientation | Upright orientation (`VerticalOrientation::Upright`) |
| **Latin / Numerics** | Upright orientation | Rotated $90^\circ$ clockwise (`VerticalOrientation::Rotated90`) |
| **OpenType Features** | Standard horizontal metrics | Enables `vert`, `vrt2`, `vkrn`, `vhal` features |

`martensite-text::vertical` applies UAX #50 orientation classification to split vertical paragraphs into runs, applying $90^\circ$ affine rotation matrices to Western script runs while keeping Kanji and Kana upright.

### 4. Native OS Font Fallback Cascades
No single font file contains all 150,000+ Unicode characters. If an application uses *Inter* as its primary typeface, displaying a user name in Arabic or Japanese will result in empty boxes or question marks unless font fallback is configured.

Martensite provides a two-tier cascade architecture:
```
Primary Font: "Inter" (Latin only)
       │
       ▼ (Glyph missing for U+0627 Arabic Alef)
PlatformCascadeResolver
       │
       ├──> Windows: IDWriteFontFallback::MapCharacters (DirectWrite FFI) -> "Segoe UI"
       ├──> macOS:   CTFontCreateForStringWithLanguage (CoreText FFI)    -> "Geeza Pro"
       └──> Linux:   FcFontSort (Fontconfig FFI)                         -> "Noto Sans Arabic"
       │
       ▼
FallbackDecisionCache (LRU cache keyed by (script, locale, primary_family))
```
- **FallbackDecisionCache**: Resolving system fonts via platform FFI is expensive. Martensite caches the resolved `FontFallbackChain` keyed by `(ScriptTag, LanguageIdentifier, FontId)`.
- When the font system generation counter advances (e.g. user installs a font), the cache automatically invalidates.

---

## 4. Common Pitfalls & Antipatterns

### 1. Hardcoding Physical `Left` and `Right` in Layouts
**Wrong:**
```rust
// HARDCODED: Layout breaks when switched to RTL
let label_x = bounds.origin.x + 16.0;
let icon_x = bounds.origin.x + bounds.size.x - 32.0;
```
**Right:**
```rust
// BIDI-AWARE: Flip logical start/end according to script direction
let is_rtl = l10n.direction().is_rtl();
let (label_x, icon_x) = if is_rtl {
    (bounds.origin.x + bounds.size.x - 16.0 - label_width, bounds.origin.x + 16.0)
} else {
    (bounds.origin.x + 16.0, bounds.origin.x + bounds.size.x - 32.0)
};
```
Always use logical alignment concepts (`InlineStart`, `InlineEnd`) rather than assuming the origin is always the visual left.

### 2. Manual String Concatenation Instead of Fluent Selectors
**Wrong:**
```rust
// FRAGILE: Assumes English grammar, word order, and plural rules
let text = format!("{} items selected", count);
```
**Right:**
```fluent
# ROBUST: Handles language-specific pluralization and grammar
items-selected = {$count ->
    [0] No items selected
    [one] 1 item selected
    [two] Two items selected
    [few] {$count} items selected
   *[other] {$count} items selected
}
```
Languages like Arabic have 6 plural categories (`zero`, `one`, `two`, `few`, `many`, `other`); Russian has 3; Japanese has 1. String formatting in Rust code cannot represent these variations without writing bespoke language code.

### 3. Rebuilding the Whole `WidgetArena` on Locale Change
Calling `arena.clear()` and re-instantiating all widgets on a locale switch forces a full reallocation, resets scroll view positions, closes dropdown menus, and destroys caret positions in active text inputs. Always bind localized text properties to `Memo<String>` derived from `L10n`.

### 4. Overlooking Vertical Baseline Advances
When positioning vertical text, advancing the caret by `metrics.advance_width` will cause characters to collide horizontally. Vertical text runs must advance along the Y-axis by `metrics.advance_height` plus vertical line spacing.

---

## Next Steps

- [Cookbook 01 — Responsive Layout & Underflow Policies](01-responsive-layout.md)
- [Cookbook 10 — Headless Component Testing](10-headless-testing.md)
- [ADR-0023 — Fluent Localization Pipeline](../adr/ADR-0023-fluent-localization.md)
- [DDR-0016 — martensite-l10n Fluent Integration](../ddr/DDR-0016-martensite-l10n.md)
