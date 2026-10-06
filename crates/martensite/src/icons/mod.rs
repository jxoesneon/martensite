//! A native stroke-icon vocabulary and the name-resolution chain for
//! icon packs.
//!
//! Martensite ships [`BUILTIN`] — a hand-authored set of stroke-path
//! icons on the conventional 24px grid, the same `d`-string idiom
//! [`MorphIcon`](crate::widgets::MorphIcon) renders. Names are
//! qualified and kebab-cased — `"nav.menu"`, `"media.play"`,
//! `"status.bell-off"` — so application and pack authors share one
//! vocabulary without depending on a downloaded icon pack.
//!
//! External packs (generated or fetched lucide, tabler, …) plug in as
//! an *overlay*: an [`IconSet`] searches its registered [`IconPack`]s
//! in order and always falls back to [`BUILTIN`]. The stroke-path
//! engine is the universal renderer — nothing in the framework
//! requires external icon data, and unknown names resolve to `None`
//! rather than panicking.
//!
//! Icons are decorative geometry: no accessibility semantics are
//! attached — the surrounding control (or `MorphIcon::label`) owns
//! the name.
//!
//! # Examples
//!
//! ```
//! use martensite::icons::{builtin, IconSet};
//!
//! // The default set resolves against the native pack.
//! let set = IconSet::new();
//! assert_eq!(set.resolve("nav.menu"), Some(builtin::nav::NAV_MENU));
//! assert!(set.resolve("not.an.icon").is_none());
//!
//! // Declared morph pairs resolve both endpoints.
//! let (a, b) = set.resolve_pair("status.lock").unwrap();
//! assert_eq!(a, builtin::status::STATUS_LOCK);
//! assert_eq!(b, builtin::status::STATUS_LOCK_OPEN);
//! ```

use std::borrow::Cow;
use std::sync::LazyLock;

use crate::widgets::morph_icon::MorphError;

/// The native pack data — per-icon `d` constants, qualified-name
/// constants, and the entry/pair tables backing [`BUILTIN`].
pub mod builtin;

/// One icon in a pack: a qualified name plus its stroke `d` string.
///
/// `Cow` fields let the same type back a zero-alloc `&'static` table
/// (generated packs, [`builtin`]) and runtime-owned data (icons
/// parsed from disk at startup).
///
/// # Examples
///
/// ```
/// use martensite::icons::IconEntry;
///
/// let e = IconEntry::new("status.check", "M5 12l5 5 9-10");
/// assert_eq!(e.name.as_ref(), "status.check");
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct IconEntry {
    /// Qualified icon name (`"status.check"`).
    pub name: Cow<'static, str>,
    /// SVG path data — multi-subpath stroke geometry on the 24px grid.
    pub d: Cow<'static, str>,
}

impl IconEntry {
    /// A borrowed (zero-alloc) entry — usable in `const`/`static`
    /// contexts, which is how [`builtin::nav::ENTRIES`] is built.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::icons::IconEntry;
    ///
    /// static ENTRIES: &[IconEntry] = &[IconEntry::new("a.b", "M0 0L1 1")];
    /// assert_eq!(ENTRIES[0].d.as_ref(), "M0 0L1 1");
    /// ```
    pub const fn new(name: &'static str, d: &'static str) -> Self {
        Self {
            name: Cow::Borrowed(name),
            d: Cow::Borrowed(d),
        }
    }
}

impl From<(&'static str, &'static str)> for IconEntry {
    fn from((name, d): (&'static str, &'static str)) -> Self {
        Self::new(name, d)
    }
}

impl From<(String, String)> for IconEntry {
    fn from((name, d): (String, String)) -> Self {
        Self {
            name: Cow::Owned(name),
            d: Cow::Owned(d),
        }
    }
}

/// A declared morph pair: `name` and `alternate` are two icon names
/// whose shapes are meaningful states of one control (locked ↔
/// unlocked, playing ↔ paused). The relation is symmetric — resolving
/// either endpoint yields the other.
///
/// # Examples
///
/// ```
/// use martensite::icons::IconPair;
///
/// let p = IconPair::new("media.play", "media.pause");
/// assert_eq!(p.alternate.as_ref(), "media.pause");
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct IconPair {
    /// One endpoint's qualified name.
    pub name: Cow<'static, str>,
    /// The other endpoint's qualified name.
    pub alternate: Cow<'static, str>,
}

impl IconPair {
    /// A borrowed (zero-alloc) pair declaration — usable in `const`/
    /// `static` contexts.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::icons::IconPair;
    ///
    /// static PAIRS: &[IconPair] = &[IconPair::new("a.x", "a.y")];
    /// assert_eq!(PAIRS[0].name.as_ref(), "a.x");
    /// ```
    pub const fn new(name: &'static str, alternate: &'static str) -> Self {
        Self {
            name: Cow::Borrowed(name),
            alternate: Cow::Borrowed(alternate),
        }
    }
}

impl From<(&'static str, &'static str)> for IconPair {
    fn from((name, alternate): (&'static str, &'static str)) -> Self {
        Self::new(name, alternate)
    }
}

/// A named, ordered registry of `(name, d)` stroke-path icons plus
/// its declared morph pairs.
///
/// Entries keep insertion order — that order is both the lookup order
/// (an earlier duplicate name wins) and a pack's natural catalog order
/// for picker UIs. A pack borrowed from `&'static` tables costs
/// nothing to construct or clone; [`from_entries`](Self::from_entries)
/// builds an owned pack from runtime data.
///
/// # Examples
///
/// ```
/// use martensite::icons::IconPack;
///
/// static ENTRIES: &[martensite::icons::IconEntry] =
///     &[martensite::icons::IconEntry::new("app.dot", "M4 4h16v16H4z")];
/// let pack = IconPack::new("app", ENTRIES);
/// assert_eq!(pack.lookup("app.dot"), Some("M4 4h16v16H4z"));
/// assert!(pack.lookup("app.miss").is_none());
/// ```
#[derive(Debug, Clone)]
pub struct IconPack {
    /// Display/registry name (`"martensite"`, `"lucide"`, `"app"`).
    name: Cow<'static, str>,
    /// Ordered entries — lookup scans in order.
    entries: Cow<'static, [IconEntry]>,
    /// Declared morph pairs (symmetric alternates).
    pairs: Cow<'static, [IconPair]>,
}

impl IconPack {
    /// A borrowed pack over `&'static` entry tables — `const`, so
    /// generated packs can be `static`s with zero construction cost.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::icons::{IconEntry, IconPack};
    ///
    /// static ENTRIES: &[IconEntry] = &[IconEntry::new("x.y", "M0 0L1 1")];
    /// static PACK: IconPack = IconPack::new("x", ENTRIES);
    /// assert_eq!(PACK.name(), "x");
    /// ```
    pub const fn new(name: &'static str, entries: &'static [IconEntry]) -> Self {
        Self {
            name: Cow::Borrowed(name),
            entries: Cow::Borrowed(entries),
            pairs: Cow::Borrowed(&[]),
        }
    }

    /// An owned pack built from any entry source — `(name, d)` tuples
    /// (`&'static` or `String`) or `IconEntry`s. Use this for packs
    /// materialized at runtime.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::icons::IconPack;
    ///
    /// let parsed: Vec<(String, String)> =
    ///     vec![("app.star".into(), "M12 2l3 7 7 1-5 5 1 7-6-3-6 3 1-7-5-5 7-1z".into())];
    /// let pack = IconPack::from_entries("app", parsed);
    /// assert!(pack.contains("app.star"));
    /// ```
    pub fn from_entries<N, I, E>(name: N, entries: I) -> Self
    where
        N: Into<Cow<'static, str>>,
        I: IntoIterator<Item = E>,
        E: Into<IconEntry>,
    {
        Self {
            name: name.into(),
            entries: Cow::Owned(entries.into_iter().map(Into::into).collect()),
            pairs: Cow::Borrowed(&[]),
        }
    }

    /// Declares the pack's morph pairs (state alternates). Overrides
    /// any pairs already set.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::icons::{IconEntry, IconPack};
    ///
    /// static ENTRIES: &[IconEntry] = &[
    ///     IconEntry::new("app.on", "M5 12h14"),
    ///     IconEntry::new("app.off", "M5 5v14"),
    /// ];
    /// let pack = IconPack::new("app", ENTRIES).with_pairs([("app.on", "app.off")]);
    /// assert_eq!(pack.paired("app.off"), Some("app.on"));
    /// ```
    #[must_use]
    pub fn with_pairs<I, P>(mut self, pairs: I) -> Self
    where
        I: IntoIterator<Item = P>,
        P: Into<IconPair>,
    {
        self.pairs = Cow::Owned(pairs.into_iter().map(Into::into).collect());
        self
    }

    /// The pack's registry name.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::icons::BUILTIN;
    ///
    /// assert_eq!(BUILTIN.name(), "martensite");
    /// ```
    pub fn name(&self) -> &str {
        self.name.as_ref()
    }

    /// The ordered entries.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::icons::BUILTIN;
    ///
    /// assert!(BUILTIN.entries().len() >= 40);
    /// ```
    pub fn entries(&self) -> &[IconEntry] {
        &self.entries
    }

    /// Number of icons in the pack.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::icons::BUILTIN;
    ///
    /// assert!(BUILTIN.len() >= 40);
    /// assert!(!BUILTIN.is_empty());
    /// ```
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// `true` when the pack carries no icons.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::icons::IconPack;
    ///
    /// assert!(IconPack::new("empty", &[]).is_empty());
    /// ```
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// `true` when `name` is an entry of this pack (exact match).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::icons::BUILTIN;
    ///
    /// assert!(BUILTIN.contains("media.play"));
    /// assert!(!BUILTIN.contains("play"));
    /// ```
    pub fn contains(&self, name: &str) -> bool {
        self.lookup(name).is_some()
    }

    /// The `d` string registered under `name` (exact match — lookups
    /// never panic and never guess at partial names).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::icons::{builtin, BUILTIN};
    ///
    /// assert_eq!(BUILTIN.lookup("media.play"), Some(builtin::media::MEDIA_PLAY));
    /// assert_eq!(BUILTIN.lookup("media.play-2"), None);
    /// ```
    pub fn lookup(&self, name: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|e| e.name.as_ref() == name)
            .map(|e| e.d.as_ref())
    }

    /// Entry names in pack order.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::icons::BUILTIN;
    ///
    /// assert_eq!(BUILTIN.names().next(), Some("nav.menu"));
    /// ```
    pub fn names(&self) -> impl Iterator<Item = &str> + '_ {
        self.entries.iter().map(|e| e.name.as_ref())
    }

    /// The morph-partner *name* declared for `name`, from either
    /// endpoint of a pair (`paired` is symmetric). `None` when no
    /// declared pair mentions `name`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::icons::{builtin::names, BUILTIN};
    ///
    /// assert_eq!(BUILTIN.paired("status.lock"), Some(names::LOCK_OPEN));
    /// assert_eq!(BUILTIN.paired("status.lock-open"), Some(names::LOCK));
    /// assert_eq!(BUILTIN.paired("nav.home"), None);
    /// ```
    pub fn paired(&self, name: &str) -> Option<&str> {
        self.pairs.iter().find_map(|p| {
            if p.name.as_ref() == name {
                Some(p.alternate.as_ref())
            } else if p.alternate.as_ref() == name {
                Some(p.name.as_ref())
            } else {
                None
            }
        })
    }

    /// The pack's declared morph pairs.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::icons::BUILTIN;
    ///
    /// assert!(BUILTIN.pairs().iter().any(|p| p.name.as_ref() == "media.play"));
    /// ```
    pub fn pairs(&self) -> &[IconPair] {
        &self.pairs
    }
}

/// The native Martensite icon pack — every [`IconSet`] resolution
/// chain ends here, so builtin names always resolve even when an
/// overlay pack lacks them.
///
/// # Examples
///
/// ```
/// use martensite::icons::{builtin, BUILTIN};
///
/// assert_eq!(BUILTIN.lookup("status.check"), Some(builtin::status::STATUS_CHECK));
/// ```
///
/// Assembled from every `builtin::*` namespace module on first
/// access — `LazyLock` rather than `const` because `&[T]` slices
/// cannot be concatenated in const context.
pub static BUILTIN: LazyLock<IconPack> = LazyLock::new(|| builtin::builtin().clone());

/// The native icon pack — identical to [`BUILTIN`], as an `&'static`
/// for APIs that want a reference.
///
/// # Examples
///
/// ```
/// use martensite::icons::builtin;
///
/// assert!(builtin().contains("nav.search"));
/// ```
pub fn builtin() -> &'static IconPack {
    &BUILTIN
}

thread_local! {
    /// The ambient icon-resolution chain for the current pass — see
    /// [`install_ambient_icons`].
    static AMBIENT_ICONS: std::cell::RefCell<Option<IconSet>> =
        const { std::cell::RefCell::new(None) };
}

/// RAII guard restoring the previous ambient [`IconSet`] on drop —
/// see [`install_ambient_icons`].
#[must_use = "the ambient icon set is uninstalled when the guard drops"]
pub struct AmbientIconsGuard(Option<IconSet>);

impl std::fmt::Debug for AmbientIconsGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("AmbientIconsGuard").field(&self.0).finish()
    }
}

impl Drop for AmbientIconsGuard {
    fn drop(&mut self) {
        let prior = self.0.take();
        AMBIENT_ICONS.with(|s| *s.borrow_mut() = prior);
    }
}

/// Installs `set` as this thread's ambient icon-resolution chain until
/// the returned guard drops — thread-local and re-entrant, the same
/// contract as [`install_ambient_intl`](martensite_core::intl::install_ambient_intl).
///
/// This is the default icon-family setting: widgets resolving an icon
/// *name* consult the ambient set first — overlay packs (a morph pack,
/// an app-private family, a themed variant set) shadow the builtin
/// lucide-style pack, which remains the fallback tail. Code that
/// paints icons from raw `d` data is unaffected.
///
/// # Examples
///
/// ```
/// use martensite::icons::{ambient_icons, install_ambient_icons, IconEntry, IconPack, IconSet};
///
/// assert!(ambient_icons().packs().is_empty());
/// {
///     let set = IconSet::new()
///         .with_pack(IconPack::new("app", &[IconEntry::new("app.logo", "M4 4l8 8-8 8")]));
///     let _guard = install_ambient_icons(set);
///     assert_eq!(ambient_icons().resolve("app.logo"), Some("M4 4l8 8-8 8"));
/// }
/// ```
pub fn install_ambient_icons(set: IconSet) -> AmbientIconsGuard {
    let prior = AMBIENT_ICONS.with(|s| s.replace(Some(set)));
    AmbientIconsGuard(prior)
}

/// The ambient [`IconSet`] for this pass — an empty overlay chain
/// (builtin pack only) when nothing is installed. Cloned out of the
/// thread-local so callers hold no borrow across widget calls.
pub fn ambient_icons() -> IconSet {
    AMBIENT_ICONS
        .with(|s| s.borrow().clone())
        .unwrap_or_default()
}

/// Resolves `name` through the ambient [`IconSet`] — the single seam
/// name-based icon consumers share so an installed family shadows the
/// builtin pack everywhere at once.
///
/// # Examples
///
/// ```
/// use martensite::icons::resolve_icon;
///
/// assert!(resolve_icon("nav.search").is_some());
/// assert!(resolve_icon("bogus.name").is_none());
/// ```
pub fn resolve_icon(name: &str) -> Option<String> {
    ambient_icons().resolve(name).map(str::to_string)
}

/// An ordered resolution chain over [`IconPack`]s ending at
/// [`BUILTIN`].
///
/// Overlay packs (external pack data, app-private icons, theme
/// overrides) are searched in registration order; the builtin pack is
/// always the fallback tail. Martensite keeps no global registry —
/// the app builds the set it wants and passes it to
/// [`MorphIcon::named_in`](crate::widgets::MorphIcon::named_in) /
/// [`morph_to_named`](crate::widgets::MorphIcon::morph_to_named) /
/// [`set_named`](crate::widgets::MorphIcon::set_named).
///
/// # Examples
///
/// ```
/// use martensite::icons::{IconEntry, IconPack, IconSet};
///
/// // Overlay shadows one builtin name, adds one private icon.
/// static ENTRIES: &[IconEntry] = &[
///     IconEntry::new("nav.menu", "M2 5h20M2 19h20"),
///     IconEntry::new("app.logo", "M4 4l8 8-8 8M12 4l8 8-8 8"),
/// ];
/// let set = IconSet::new().with_pack(IconPack::new("app", ENTRIES));
/// assert_eq!(set.resolve("nav.menu"), Some("M2 5h20M2 19h20"));
/// assert_eq!(set.resolve("app.logo"), Some("M4 4l8 8-8 8M12 4l8 8-8 8"));
/// // Untouched names still fall through to the builtin pack.
/// assert!(set.resolve("status.bell").is_some());
/// ```
#[derive(Debug, Default, Clone)]
pub struct IconSet {
    /// Overlay packs in search order — the builtin pack is implicit.
    packs: Vec<IconPack>,
}

impl IconSet {
    /// An empty overlay chain — resolution is the builtin pack alone.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::icons::IconSet;
    ///
    /// let set = IconSet::new();
    /// assert!(set.packs().is_empty());
    /// ```
    pub fn new() -> Self {
        Self::default()
    }

    /// Builder form of [`push`](Self::push) — appends `pack` to the
    /// overlay chain (earlier packs outrank later ones).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::icons::{IconPack, IconSet};
    ///
    /// let set = IconSet::new()
    ///     .with_pack(IconPack::new("a", &[]))
    ///     .with_pack(IconPack::new("b", &[]));
    /// assert_eq!(set.packs().len(), 2);
    /// ```
    #[must_use]
    pub fn with_pack(mut self, pack: IconPack) -> Self {
        self.push(pack);
        self
    }

    /// Appends `pack` to the overlay chain — it sits *below* every
    /// pack already registered and above [`BUILTIN`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::icons::{IconPack, IconSet};
    ///
    /// let mut set = IconSet::new();
    /// set.push(IconPack::new("app", &[]));
    /// assert_eq!(set.packs()[0].name(), "app");
    /// ```
    pub fn push(&mut self, pack: IconPack) {
        self.packs.push(pack);
    }

    /// The registered overlay packs in search order (the builtin
    /// fallback is not listed).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::icons::IconSet;
    ///
    /// assert!(IconSet::new().packs().is_empty());
    /// ```
    pub fn packs(&self) -> &[IconPack] {
        &self.packs
    }

    /// The `d` string for `name`: the first overlay pack carrying it
    /// wins; [`BUILTIN`] answers when none does. `None` for unknown
    /// names — never a panic.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::icons::{builtin, IconSet};
    ///
    /// let set = IconSet::new();
    /// assert_eq!(set.resolve("media.pause"), Some(builtin::media::MEDIA_PAUSE));
    /// assert_eq!(set.resolve(""), None);
    /// ```
    pub fn resolve(&self, name: &str) -> Option<&str> {
        self.packs
            .iter()
            .find_map(|p| p.lookup(name))
            .or_else(|| BUILTIN.lookup(name))
    }

    /// `true` when `name` resolves through this chain.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::icons::IconSet;
    ///
    /// let set = IconSet::new();
    /// assert!(set.contains("edit.trash"));
    /// assert!(!set.contains("edit.broom"));
    /// ```
    pub fn contains(&self, name: &str) -> bool {
        self.resolve(name).is_some()
    }

    /// The morph-partner name declared for `name`: pair declarations
    /// are searched over the same chain as [`resolve`](Self::resolve)
    /// — overlays first, then builtin — and are symmetric, so
    /// `paired` on either endpoint yields the other.
    ///
    /// Resolve the result through this set to get its `d`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::icons::{builtin::names, IconSet};
    ///
    /// let set = IconSet::new();
    /// assert_eq!(set.paired("nav.menu"), Some(names::CLOSE));
    /// assert_eq!(set.paired("status.close"), Some(names::MENU));
    /// assert_eq!(set.paired("nav.search"), None);
    /// ```
    pub fn paired(&self, name: &str) -> Option<&str> {
        self.packs
            .iter()
            .chain(std::iter::once(&*BUILTIN))
            .find_map(|p| p.paired(name))
    }

    /// Both `d` strings of the declared pair for `name` — `Some((a,
    /// b))` where `a` is `name`'s own shape and `b` its partner's —
    /// or `None` if `name` is unpaired or either endpoint fails to
    /// resolve.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::icons::{builtin, IconSet};
    ///
    /// let set = IconSet::new();
    /// let (on, off) = set.resolve_pair("media.volume-on").unwrap();
    /// assert_eq!((on, off), (builtin::media::MEDIA_VOLUME_ON, builtin::media::MEDIA_VOLUME_OFF));
    /// ```
    pub fn resolve_pair(&self, name: &str) -> Option<(&str, &str)> {
        let a = self.resolve(name)?;
        let partner = self.paired(name)?;
        let b = self.resolve(partner)?;
        Some((a, b))
    }

    /// Every resolvable name: overlay entries first (search order),
    /// then builtin entries, first occurrence of shadowed names only.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::icons::IconSet;
    ///
    /// let set = IconSet::new();
    /// let names = set.names();
    /// assert!(names.contains(&"nav.menu"));
    /// assert!(names.contains(&"misc.gauge"));
    /// ```
    pub fn names(&self) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::new();
        for pack in self.packs.iter().chain(std::iter::once(&*BUILTIN)) {
            for entry in pack.entries.iter() {
                if !out.contains(&entry.name.as_ref()) {
                    out.push(entry.name.as_ref());
                }
            }
        }
        out
    }
}

/// Why [`MorphIcon::named`](crate::widgets::MorphIcon::named) (and
/// its `*_named` siblings) failed.
///
/// # Examples
///
/// ```
/// use martensite::icons::IconError;
///
/// let e = IconError::Unknown {
///     name: "bogus".into(),
/// };
/// assert!(e.to_string().contains("bogus"));
/// ```
#[derive(Debug, Clone, PartialEq)]
pub enum IconError {
    /// No pack in the chain carried the name.
    Unknown {
        /// The unresolvable name.
        name: String,
    },
    /// The name resolved but its `d` failed the [`MorphIcon`]
    /// parse contract (malformed, oversized, degenerate).
    ///
    /// [`MorphIcon`]: crate::widgets::MorphIcon
    Invalid {
        /// The resolved name whose data was rejected.
        name: String,
        /// The underlying parse/cap failure.
        source: MorphError,
    },
}

impl core::fmt::Display for IconError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Unknown { name } => write!(f, "unknown icon name: {name}"),
            Self::Invalid { name, source } => {
                write!(f, "icon {name:?} has invalid path data: {source}")
            }
        }
    }
}

impl std::error::Error for IconError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Unknown { .. } => None,
            Self::Invalid { source, .. } => Some(source),
        }
    }
}

#[cfg(test)]
mod tests;
