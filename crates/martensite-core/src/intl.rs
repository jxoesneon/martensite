//! Ambient layout direction and locale.
//!
//! Direction and locale are framework-level state owned by
//! [`WidgetArena`](crate::WidgetArena) and published to widgets through
//! a thread-local ambient channel for the duration of each layout,
//! paint, and event pass — the same pattern
//! [`install_ambient_measurer`](crate::paint::install_ambient_measurer)
//! uses for text measurement. Widgets read the values through
//! [`LayoutContext::direction`](crate::LayoutContext::direction),
//! [`PaintContext::direction`](crate::PaintContext::direction), and
//! [`EventContext::direction`](crate::EventContext::direction) (plus the
//! matching `locale` accessors).
//!
//! # Examples
//!
//! ```
//! use martensite_core::intl::{ambient_direction, install_ambient_intl};
//! use martensite_core::{LayoutDirection, Locale};
//!
//! assert_eq!(ambient_direction(), LayoutDirection::Ltr);
//! {
//!     let _guard = install_ambient_intl(LayoutDirection::Rtl, Locale::new("ar-EG"));
//!     assert!(ambient_direction().is_rtl());
//! }
//! assert_eq!(ambient_direction(), LayoutDirection::Ltr);
//! ```

use std::sync::Arc;

use crate::node::Rect;

/// Primary language subtags whose scripts are written right-to-left.
const RTL_LANGUAGES: &[&str] = &[
    "ar", "he", "iw", "fa", "ur", "ps", "sd", "yi", "dv", "ug", "ckb", "syr",
];

/// Horizontal flow direction for layout and text.
///
/// # Examples
///
/// ```
/// use martensite_core::LayoutDirection;
///
/// assert_eq!(LayoutDirection::default(), LayoutDirection::Ltr);
/// assert!(LayoutDirection::Rtl.is_rtl());
/// ```
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum LayoutDirection {
    /// Left-to-right (Latin, Cyrillic, CJK, ...).
    #[default]
    Ltr,
    /// Right-to-left (Arabic, Hebrew, Persian, ...).
    Rtl,
}

impl LayoutDirection {
    /// `true` for [`LayoutDirection::Rtl`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::LayoutDirection;
    ///
    /// assert!(!LayoutDirection::Ltr.is_rtl());
    /// assert!(LayoutDirection::Rtl.is_rtl());
    /// ```
    pub fn is_rtl(self) -> bool {
        matches!(self, Self::Rtl)
    }

    /// The conventional direction for `locale`: [`LayoutDirection::Rtl`]
    /// when the locale's primary language subtag is one of `ar`, `he`,
    /// `iw`, `fa`, `ur`, `ps`, `sd`, `yi`, `dv`, `ug`, `ckb`, `syr`;
    /// otherwise [`LayoutDirection::Ltr`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::{LayoutDirection, Locale};
    ///
    /// assert_eq!(LayoutDirection::for_locale(&Locale::new("ar-EG")), LayoutDirection::Rtl);
    /// assert_eq!(LayoutDirection::for_locale(&Locale::new("en-US")), LayoutDirection::Ltr);
    /// ```
    pub fn for_locale(locale: &Locale) -> Self {
        let lang = locale.language();
        if RTL_LANGUAGES.contains(&lang.as_str()) {
            Self::Rtl
        } else {
            Self::Ltr
        }
    }
}

/// A BCP-47 language tag such as `"en-US"` or `"ar-EG"`.
///
/// The tag is stored verbatim; only [`Locale::language`] interprets it.
/// Cloning is cheap (shared `Arc<str>`).
///
/// # Examples
///
/// ```
/// use martensite_core::Locale;
///
/// let locale = Locale::new("pt-BR");
/// assert_eq!(locale.as_str(), "pt-BR");
/// assert_eq!(locale.language(), "pt");
/// assert_eq!(Locale::default().as_str(), "en-US");
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Locale(Arc<str>);

impl Locale {
    /// Wraps a BCP-47 tag.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::Locale;
    ///
    /// let locale = Locale::new("he-IL");
    /// assert_eq!(locale.as_str(), "he-IL");
    /// ```
    pub fn new(tag: impl Into<Arc<str>>) -> Self {
        Self(tag.into())
    }

    /// The tag exactly as supplied.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::Locale;
    ///
    /// assert_eq!(Locale::new("de_DE").as_str(), "de_DE");
    /// ```
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The lowercased primary language subtag — `"ar"` for `"ar-EG"`.
    /// Both `-` and `_` are accepted as subtag separators.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::Locale;
    ///
    /// assert_eq!(Locale::new("ar-EG").language(), "ar");
    /// assert_eq!(Locale::new("DE_de").language(), "de");
    /// ```
    pub fn language(&self) -> String {
        self.0
            .split(['-', '_'])
            .next()
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase()
    }
}

impl Default for Locale {
    fn default() -> Self {
        Self::new("en-US")
    }
}

thread_local! {
    /// The ambient direction and locale for the current pass — see
    /// [`install_ambient_intl`].
    static AMBIENT_INTL: std::cell::RefCell<Option<(LayoutDirection, Locale)>> =
        const { std::cell::RefCell::new(None) };
}

/// RAII guard that restores the previous ambient direction and locale
/// on drop — see [`install_ambient_intl`].
///
/// # Examples
///
/// ```
/// use martensite_core::intl::{ambient_direction, install_ambient_intl};
/// use martensite_core::{LayoutDirection, Locale};
///
/// let guard = install_ambient_intl(LayoutDirection::Rtl, Locale::new("fa-IR"));
/// assert!(ambient_direction().is_rtl());
/// drop(guard);
/// assert!(!ambient_direction().is_rtl());
/// ```
#[must_use = "the ambient direction and locale are uninstalled when the guard drops"]
pub struct AmbientIntlGuard(Option<(LayoutDirection, Locale)>);

impl std::fmt::Debug for AmbientIntlGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("AmbientIntlGuard").field(&self.0).finish()
    }
}

impl Drop for AmbientIntlGuard {
    fn drop(&mut self) {
        let prior = self.0.take();
        AMBIENT_INTL.with(|s| *s.borrow_mut() = prior);
    }
}

/// Installs `direction` and `locale` as this thread's ambient values
/// until the returned [`AmbientIntlGuard`] drops.
///
/// `WidgetArena` installs its own values automatically around
/// `build_paint_list`, `dispatch_event`, and overlay passes, and
/// `LayoutEngine::compute_with_widgets` installs them for layout;
/// manual layout passes (tests, harness sweeps) install them via
/// [`WidgetArena::install_ambient_intl`](crate::WidgetArena::install_ambient_intl).
///
/// The guard is thread-local and re-entrant: nested installs restore
/// in LIFO order.
///
/// # Examples
///
/// ```
/// use martensite_core::intl::{ambient_direction, ambient_locale, install_ambient_intl};
/// use martensite_core::{LayoutDirection, Locale};
///
/// let outer = install_ambient_intl(LayoutDirection::Rtl, Locale::new("ar"));
/// {
///     let _inner = install_ambient_intl(LayoutDirection::Ltr, Locale::new("fr-FR"));
///     assert_eq!(ambient_locale().as_str(), "fr-FR");
/// }
/// assert_eq!(ambient_direction(), LayoutDirection::Rtl);
/// drop(outer);
/// assert_eq!(ambient_locale(), Locale::default());
/// ```
pub fn install_ambient_intl(direction: LayoutDirection, locale: Locale) -> AmbientIntlGuard {
    let prior = AMBIENT_INTL.with(|s| s.replace(Some((direction, locale))));
    AmbientIntlGuard(prior)
}

/// The ambient layout direction — [`LayoutDirection::Ltr`] when nothing
/// is installed.
///
/// # Examples
///
/// ```
/// use martensite_core::intl::ambient_direction;
/// use martensite_core::LayoutDirection;
///
/// assert_eq!(ambient_direction(), LayoutDirection::Ltr);
/// ```
pub fn ambient_direction() -> LayoutDirection {
    AMBIENT_INTL.with(|s| {
        s.borrow()
            .as_ref()
            .map_or_else(LayoutDirection::default, |v| v.0)
    })
}

/// The ambient locale — [`Locale::default`] (`en-US`) when nothing is
/// installed.
///
/// # Examples
///
/// ```
/// use martensite_core::intl::ambient_locale;
///
/// assert_eq!(ambient_locale().as_str(), "en-US");
/// ```
pub fn ambient_locale() -> Locale {
    AMBIENT_INTL.with(|s| {
        s.borrow()
            .as_ref()
            .map_or_else(Locale::default, |v| v.1.clone())
    })
}

/// Reflects `inner` horizontally within `outer` when `dir` is
/// [`LayoutDirection::Rtl`]; identity for [`LayoutDirection::Ltr`].
///
/// `x' = outer.x + (outer.x + outer.w) - (inner.x + inner.w)`; `y` and
/// size are unchanged.
///
/// # Examples
///
/// ```
/// use martensite_core::intl::mirror_x;
/// use martensite_core::{LayoutDirection, Rect};
///
/// let outer = Rect::new(100.0, 0.0, 200.0, 50.0);
/// let inner = Rect::new(110.0, 5.0, 40.0, 20.0);
/// assert_eq!(mirror_x(outer, inner, LayoutDirection::Ltr), inner);
/// let m = mirror_x(outer, inner, LayoutDirection::Rtl);
/// assert_eq!(m, Rect::new(250.0, 5.0, 40.0, 20.0));
/// ```
pub fn mirror_x(outer: Rect, inner: Rect, dir: LayoutDirection) -> Rect {
    if !dir.is_rtl() {
        return inner;
    }
    let x = outer.min_x() + outer.max_x() - inner.max_x();
    Rect::new(x, inner.min_y(), inner.width(), inner.height())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn for_locale_detects_rtl_languages() {
        let rtl = |t: &str| LayoutDirection::for_locale(&Locale::new(t));
        assert_eq!(rtl("ar-EG"), LayoutDirection::Rtl);
        assert_eq!(rtl("he"), LayoutDirection::Rtl);
        assert_eq!(rtl("fa-IR"), LayoutDirection::Rtl);
        assert_eq!(rtl("AR"), LayoutDirection::Rtl);
        assert_eq!(rtl("ckb-IQ"), LayoutDirection::Rtl);
        assert_eq!(rtl("syr"), LayoutDirection::Rtl);
        assert_eq!(rtl("en-US"), LayoutDirection::Ltr);
        assert_eq!(rtl("de_DE"), LayoutDirection::Ltr);
        assert_eq!(rtl(""), LayoutDirection::Ltr);
        // Prefix match must not leak: "arn" (Mapudungun) is LTR.
        assert_eq!(rtl("arn-CL"), LayoutDirection::Ltr);
    }

    #[test]
    fn language_parsing() {
        assert_eq!(Locale::new("ar-EG").language(), "ar");
        assert_eq!(Locale::new("de_DE").language(), "de");
        assert_eq!(Locale::new("AR").language(), "ar");
        assert_eq!(Locale::new("zh-Hant-TW").language(), "zh");
        assert_eq!(Locale::new("").language(), "");
        assert_eq!(Locale::new(String::from("he")).as_str(), "he");
    }

    #[test]
    fn defaults() {
        assert_eq!(LayoutDirection::default(), LayoutDirection::Ltr);
        assert_eq!(Locale::default().as_str(), "en-US");
        assert_eq!(ambient_direction(), LayoutDirection::Ltr);
        assert_eq!(ambient_locale(), Locale::default());
    }

    #[test]
    fn guard_restores_lifo_nested() {
        let a = install_ambient_intl(LayoutDirection::Rtl, Locale::new("ar"));
        assert_eq!(ambient_direction(), LayoutDirection::Rtl);
        assert_eq!(ambient_locale().as_str(), "ar");
        {
            let _b = install_ambient_intl(LayoutDirection::Ltr, Locale::new("de"));
            assert_eq!(ambient_direction(), LayoutDirection::Ltr);
            assert_eq!(ambient_locale().as_str(), "de");
            {
                let _c = install_ambient_intl(LayoutDirection::Rtl, Locale::new("he"));
                assert_eq!(ambient_locale().as_str(), "he");
            }
            assert_eq!(ambient_locale().as_str(), "de");
        }
        assert_eq!(ambient_direction(), LayoutDirection::Rtl);
        assert_eq!(ambient_locale().as_str(), "ar");
        assert!(format!("{a:?}").contains("AmbientIntlGuard"));
        drop(a);
        assert_eq!(ambient_direction(), LayoutDirection::Ltr);
        assert_eq!(ambient_locale(), Locale::default());
    }

    #[test]
    fn mirror_x_ltr_identity_rtl_reflection() {
        let outer = Rect::new(50.0, 10.0, 300.0, 100.0);
        let inner = Rect::new(60.0, 20.0, 80.0, 30.0);
        assert_eq!(mirror_x(outer, inner, LayoutDirection::Ltr), inner);
        let m = mirror_x(outer, inner, LayoutDirection::Rtl);
        // Left gap 10 becomes right gap 10: x = 50 + 300 - 10 - 80.
        assert_eq!(m, Rect::new(260.0, 20.0, 80.0, 30.0));
        // Involution.
        assert_eq!(mirror_x(outer, m, LayoutDirection::Rtl), inner);
    }
}
