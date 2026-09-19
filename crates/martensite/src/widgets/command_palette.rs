//! `CommandPalette` widget: a fuzzy action launcher — the KDE
//! `KCommandBar` / cmdk `Command` / VS Code `Ctrl+Shift+P` equivalent.
//!
//! Implements the [APG editable-combobox pattern](https://www.w3.org/WAI/ARIA/apg/patterns/combobox/)
//! with the same architecture [`AutoComplete`](crate::widgets::AutoComplete)
//! uses:
//!
//! - The widget embeds a [`TextInput`] internal child as its face — a
//!   search-field-looking trigger showing the placeholder and a `⌘K`
//!   shortcut-hint suffix — and emits `Role::ComboBox` with
//!   `aria-haspopup="listbox"`, `aria-expanded`, `aria-controls`, and
//!   `aria-activedescendant` wired to the popup (via
//!   [`Widget::a11y_fixup`](martensite_core::Widget::a11y_fixup)
//!   against overlay node ids). The inner field keeps its own
//!   `Role::TextInput` node as a virtual child.
//! - The result list is a `Role::ListBox` of `Role::ListBoxOption`
//!   children living in the
//!   [`OverlayLayer`](martensite_core::overlay::OverlayLayer), placed
//!   below the field (flipping above near the bottom edge), with
//!   `Arc<Mutex<_>>`-shared state so hover/click inside the popup
//!   reaches the owner.
//! - The query field stays in the owner rather than inside the popup
//!   surface because the production router delivers `ImeCommitted`
//!   text input only to the focused arena widget — overlay content
//!   receives `KeyPressed`/`KeyReleased` but never IME text. Embedding
//!   the field here keeps typing working end-to-end while the popup
//!   below shows the filtered results.
//! - Typing re-filters by fuzzy subsequence match (see
//!   [`CommandPalette::filtered`] for the scoring contract) and resets
//!   the highlight to the top match. `ArrowDown`/`ArrowUp` move the
//!   highlight, `Home`/`End` jump, `Enter` activates the highlighted
//!   action (closing the palette and reporting its id through
//!   [`take_activated`](CommandPalette::take_activated)), and `Escape`
//!   or an outside press dismisses without activating — the overlay
//!   handles both and the widget reconciles in
//!   [`CommandPalette::sync_overlay`].
//! - Out-seam: [`take_activated`](CommandPalette::take_activated)
//!   yields the activated [`CommandAction::id`].
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::{CommandAction, CommandPalette};
//!
//! let mut pal = CommandPalette::new().actions([
//!     CommandAction::new("file.open", "Open File…"),
//!     CommandAction::new("file.save", "Save File"),
//! ]);
//! pal.open();
//! assert!(pal.is_open());
//! assert_eq!(pal.filtered().len(), 2);
//! ```

use std::sync::Arc;

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::overlay::{OverlayAnchor, OverlayLayer};
use martensite_core::shape::Shape;
use martensite_core::widget::{
    A11yEmittedNode, EventContext, EventResponse, LayoutConstraints, LayoutContext, OverlayA11yRef,
    PaintContext, PointerButton, SemanticAction, Widget, WidgetEvent,
};
use martensite_core::{NodeFlags, Rect, RenderMinimum, TokenKey, UnderflowPolicy};
use parking_lot::Mutex;

use crate::widgets::scrollview::ScrollView;
use crate::widgets::text_input::TextInput;

/// Result row height in the popup, in logical pixels.
const ROW_H: f32 = 30.0;
/// Default maximum results listed in the popup.
const DEFAULT_MAX_RESULTS: usize = 8;
/// Face measurement floor in logical points — a launcher wants a
/// wider field than a plain autocomplete.
const FIELD_W: f32 = 280.0;
const FIELD_H: f32 = 28.0;
/// Title ink.
const INK: [u8; 4] = [30, 30, 36, 255];
/// Subtitle ink.
const INK_MUTED: [u8; 4] = [120, 124, 134, 255];
/// Popup background.
const POPUP_BG: [u8; 4] = [252, 252, 254, 255];
/// Popup border.
const POPUP_BORDER: [u8; 4] = [140, 145, 155, 255];
/// Highlighted result background.
const HIGHLIGHT_BG: [u8; 4] = [60, 110, 220, 255];
/// Highlighted result ink.
const HIGHLIGHT_INK: [u8; 4] = [255, 255, 255, 255];
/// Fallback accessible name when no [`CommandPalette::label`] is set.
const DEFAULT_LABEL: &str = "Command palette";
/// Default placeholder shown while the query is empty.
const DEFAULT_PLACEHOLDER: &str = "Search commands…";
/// Shortcut hint painted as the field's right-edge suffix.
const SHORTCUT_HINT: &str = "⌘K";

/// Base score awarded for every matched query character.
const SCORE_CHAR: i64 = 1;
/// Bonus for a matched character immediately following the previous
/// matched character (consecutive-run bonus).
const SCORE_CONSECUTIVE: i64 = 10;
/// Bonus for a matched character sitting on a word boundary — the
/// start of the candidate, after a non-alphanumeric separator, or a
/// camelCase hump.
const SCORE_BOUNDARY: i64 = 8;
/// Bonus when the candidate literally starts with the query
/// (exact-prefix bonus).
const SCORE_PREFIX: i64 = 50;

/// One launchable command in a [`CommandPalette`].
///
/// Carries the stable [`id`](CommandAction::id) reported through
/// [`CommandPalette::take_activated`], the display
/// [`title`](CommandAction::title), an optional dimmed
/// [`subtitle`](CommandAction::subtitle) (a category path, shortcut,
/// or description), and extra [`keywords`](CommandAction::keywords)
/// the fuzzy matcher scores alongside the title.
///
/// # Examples
///
/// ```
/// use martensite::widgets::CommandAction;
///
/// let action = CommandAction::new("app.quit", "Quit")
///     .subtitle("Application")
///     .keywords(["exit", "close"]);
/// assert_eq!(action.id, "app.quit");
/// assert_eq!(action.subtitle.as_deref(), Some("Application"));
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommandAction {
    /// Stable identifier reported by
    /// [`CommandPalette::take_activated`].
    pub id: String,
    /// Display title — the primary fuzzy-match candidate.
    pub title: String,
    /// Optional secondary line painted dimmed beside the title.
    pub subtitle: Option<String>,
    /// Extra match terms scored alongside the title (e.g. synonyms
    /// like `"exit"` for a `"Quit"` command).
    pub keywords: Vec<String>,
}

impl CommandAction {
    /// Creates an action with the given stable id and display title.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::CommandAction;
    ///
    /// let action = CommandAction::new("edit.copy", "Copy");
    /// assert_eq!(action.title, "Copy");
    /// assert!(action.keywords.is_empty());
    /// ```
    pub fn new(id: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            subtitle: None,
            keywords: Vec::new(),
        }
    }

    /// Sets the secondary line painted dimmed beside the title.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::CommandAction;
    ///
    /// let action = CommandAction::new("file.save", "Save").subtitle("File · Ctrl+S");
    /// assert_eq!(action.subtitle.as_deref(), Some("File · Ctrl+S"));
    /// ```
    #[inline]
    #[must_use]
    pub fn subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    /// Sets extra match terms the fuzzy matcher scores alongside the
    /// title.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::CommandAction;
    ///
    /// let action = CommandAction::new("app.quit", "Quit").keywords(["exit", "close"]);
    /// assert_eq!(action.keywords.len(), 2);
    /// ```
    #[inline]
    #[must_use]
    pub fn keywords(mut self, keywords: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.keywords = keywords.into_iter().map(Into::into).collect();
        self
    }
}

/// `true` when `hay[i]` starts a new word: index 0, following a
/// non-alphanumeric separator, or a camelCase hump (`lowercase` →
/// `UPPERCASE`). `hay` is the candidate's original-case characters.
fn is_word_boundary(hay: &[char], i: usize) -> bool {
    if i == 0 {
        return true;
    }
    let prev = hay[i - 1];
    !prev.is_alphanumeric() || (prev.is_lowercase() && hay[i].is_uppercase())
}

/// Scores one candidate string against a lowercased query — a
/// leftmost-greedy subsequence match.
///
/// Returns `None` when `query` is not a subsequence of `candidate`.
/// Otherwise the score is:
///
/// - [`SCORE_CHAR`] per matched character;
/// - [`SCORE_CONSECUTIVE`] per matched character consecutive to the
///   previous match (run bonus — `"sav"` in `"Save"` beats `s…a…v`
///   scattered across a long title);
/// - [`SCORE_BOUNDARY`] per matched character on a word boundary;
/// - [`SCORE_PREFIX`] when the candidate starts with the whole query.
///
/// The lowercase fold keeps per-character index alignment with the
/// original (first char of each `to_lowercase` expansion), so
/// camelCase boundaries stay detectable on `orig`.
fn fuzzy_score(query: &[char], candidate: &str) -> Option<i64> {
    let orig: Vec<char> = candidate.chars().collect();
    let lower: Vec<char> = orig
        .iter()
        .map(|c| c.to_lowercase().next().unwrap_or(*c))
        .collect();
    // Leftmost-greedy subsequence scan.
    let mut positions = Vec::with_capacity(query.len());
    let mut qi = 0usize;
    for (i, &h) in lower.iter().enumerate() {
        if qi == query.len() {
            break;
        }
        if h == query[qi] {
            positions.push(i);
            qi += 1;
        }
    }
    if qi != query.len() {
        return None;
    }
    let mut score = query.len() as i64 * SCORE_CHAR;
    for (k, &p) in positions.iter().enumerate() {
        if k > 0 && p == positions[k - 1] + 1 {
            score += SCORE_CONSECUTIVE;
        }
        if is_word_boundary(&orig, p) {
            score += SCORE_BOUNDARY;
        }
    }
    if lower.starts_with(query) {
        score += SCORE_PREFIX;
    }
    Some(score)
}

/// The best fuzzy score for an action — the maximum over its title
/// and each keyword.
fn action_score(query: &[char], action: &CommandAction) -> Option<i64> {
    let mut best = fuzzy_score(query, &action.title);
    for keyword in &action.keywords {
        if let Some(s) = fuzzy_score(query, keyword) {
            best = Some(best.map_or(s, |b| b.max(s)));
        }
    }
    best
}

/// One display row mirrored into the popup's shared state.
#[derive(Clone, Debug)]
struct PopupRow {
    /// Action title.
    title: String,
    /// Optional dimmed subtitle.
    subtitle: Option<String>,
}

/// State shared between a [`CommandPalette`] and its popup widgets.
///
/// The popup content reads `items`/`highlighted` to paint and emit
/// accessibility; option hover and activation write back so the owner
/// can apply them on the next `sync_overlay`.
#[derive(Debug)]
struct PopupState {
    /// Filtered rows in display order.
    items: Vec<PopupRow>,
    /// Row with the visual highlight (the `aria-activedescendant`
    /// target); `None` before keyboard navigation begins.
    highlighted: Option<usize>,
    /// Set by a popup row when it is activated (click or AT Click).
    activated: Option<usize>,
}

/// One result inside the popup listbox — a stateless view over the
/// shared [`PopupState`], emitted as `Role::ListBoxOption`.
struct CommandItem {
    /// Index into `shared.items`.
    index: usize,
    /// Shared state with the owning palette.
    shared: Arc<Mutex<PopupState>>,
    /// Shared shaped-text painter from the owning `CommandPalette`.
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl Widget for CommandItem {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        // Content width (title + subtitle approx + gutters) —
        // reporting `max_size.x` here makes the ScrollView think the
        // column overflows horizontally and shows a phantom hbar.
        let label_w = self
            .shared
            .lock()
            .items
            .get(self.index)
            .map(|row| {
                let chars = row.title.chars().count()
                    + row.subtitle.as_deref().map_or(0, |s| s.chars().count());
                chars as f32 * cx.pt(7.0) + cx.pt(40.0)
            })
            .unwrap_or(0.0);
        Vec2::new(
            label_w.min(constraints.max_size.x.max(0.0)),
            cx.pt(ROW_H).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn layout(&mut self, _cx: &mut LayoutContext, _bounds: Rect) {}

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::ListBoxOption);
        let state = self.shared.lock();
        if let Some(row) = state.items.get(self.index) {
            node.set_label(row.title.as_str());
            if let Some(ref subtitle) = row.subtitle {
                node.set_description(subtitle.as_str());
            }
        }
        node.set_position_in_set(self.index + 1);
        node.set_size_of_set(state.items.len());
        node.add_action(accesskit::Action::Click);
        // No `Action::Focus`: options are not focusable — the palette
        // field owns focus and tracks the highlight via
        // `aria-activedescendant` (the editable APG combobox pattern).
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerMoved { .. } | WidgetEvent::PointerEnter => {
                let mut state = self.shared.lock();
                if state.highlighted != Some(self.index) {
                    state.highlighted = Some(self.index);
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Handled
            }
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                ..
            } => {
                self.shared.lock().activated = Some(self.index);
                EventResponse::Handled
            }
            WidgetEvent::SemanticAction(SemanticAction::Click) => {
                self.shared.lock().activated = Some(self.index);
                EventResponse::Handled
            }
            WidgetEvent::SemanticAction(SemanticAction::Focus) => {
                self.shared.lock().highlighted = Some(self.index);
                EventResponse::Handled
            }
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let state = self.shared.lock();
        let b = cx.bounds;
        let rect = kurbo::Rect::new(
            f64::from(b.min_x()),
            f64::from(b.min_y()),
            f64::from(b.max_x()),
            f64::from(b.max_y()),
        );
        let highlighted = state.highlighted == Some(self.index);
        if highlighted {
            cx.list
                .push_fill_rect(rect, cx.color(TokenKey::AccentColor, HIGHLIGHT_BG));
        }
        // `DrawText` positions by the run's top edge — centre the
        // font box inside the row.
        let font_px = cx.pt(14.0);
        let sub_px = cx.pt(12.0);
        let text_y = b.min_y() + (b.height() - font_px) / 2.0;
        let ink = if highlighted {
            cx.color(TokenKey::TextInverseColor, HIGHLIGHT_INK)
        } else {
            cx.color(TokenKey::TextColor, INK)
        };
        let muted = if highlighted {
            cx.color(TokenKey::TextInverseColor, HIGHLIGHT_INK)
        } else {
            cx.color(TokenKey::TextMutedColor, INK_MUTED)
        };
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        if let Some(row) = state.items.get(self.index) {
            // Reserve the subtitle's right-aligned lane so a long
            // title can't paint beneath it.
            let sub_w = row.subtitle.as_deref().map_or(0.0, |s| {
                painter
                    .and_then(|p| p.measure_text(s, sub_px))
                    .unwrap_or_else(|| s.chars().count() as f32 * cx.pt(6.0))
                    + cx.pt(16.0)
            });
            // Clip the title to the row minus the subtitle lane — a
            // long command can't spill past the popup's right edge.
            let text_x = b.min_x() + cx.pt(10.0);
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                kurbo::Rect::new(
                    f64::from(text_x),
                    f64::from(b.min_y()),
                    f64::from(b.max_x() - cx.pt(6.0) - sub_w),
                    f64::from(b.max_y()),
                ),
                kurbo::Point::new(f64::from(text_x), f64::from(text_y)),
                row.title.as_str(),
                font_px,
                ink,
            );
            if let Some(ref subtitle) = row.subtitle {
                let sub_w = painter
                    .and_then(|p| p.measure_text(subtitle, sub_px))
                    .unwrap_or_else(|| subtitle.chars().count() as f32 * cx.pt(6.0));
                let sub_y = b.min_y() + (b.height() - sub_px) / 2.0;
                let sx = (b.max_x() - cx.pt(10.0) - sub_w).max(text_x);
                crate::text_paint::paint_label_clipped(
                    painter,
                    cx.list,
                    rect,
                    kurbo::Point::new(f64::from(sx), f64::from(sub_y)),
                    subtitle.as_str(),
                    sub_px,
                    muted,
                );
            }
        }
    }
}

/// Vertical column of popup results — the scroll view's content.
struct CommandColumn {
    /// Results in order.
    items: Vec<CommandItem>,
    /// Row bounds from the last layout pass.
    row_bounds: Vec<Rect>,
}

impl Widget for CommandColumn {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        // Widest item — `CommandItem::measure` reports content width
        // so the ScrollView does not see a phantom horizontal
        // overflow.
        let w = self
            .items
            .iter_mut()
            .map(|i| i.measure(cx, constraints).x)
            .fold(0.0f32, f32::max);
        Vec2::new(
            w.min(constraints.max_size.x.max(0.0)),
            (self.items.len() as f32 * cx.pt(ROW_H)).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.row_bounds.clear();
        for (i, item) in self.items.iter_mut().enumerate() {
            let rect = Rect::new(
                bounds.min_x(),
                bounds.min_y() + i as f32 * cx.pt(ROW_H),
                bounds.width(),
                cx.pt(ROW_H),
            );
            self.row_bounds.push(rect);
            cx.layout_child(item, rect);
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::GenericContainer);
    }

    fn child_count(&self) -> usize {
        self.items.len()
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        self.items.get(index).map(|i| i as &dyn Widget)
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        self.items.get_mut(index).map(|i| i as &mut dyn Widget)
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        self.row_bounds.get(index).copied()
    }
}

/// The popup surface — a `Role::ListBox` wrapping a [`ScrollView`] of
/// [`CommandItem`]s. Opened in the overlay by
/// [`CommandPalette::sync_overlay`].
struct PalettePopup {
    /// Scrolling result list (internal child 0).
    scroll: ScrollView,
    /// Shared state with the owning palette.
    shared: Arc<Mutex<PopupState>>,
    /// Maximum rows the popup measures before scrolling.
    max_rows: usize,
    /// Popup bounds from the last layout pass (the scroll view fills
    /// the whole popup inside its 1px border).
    bounds: Option<Rect>,
    /// The silhouette painted last frame — the single source of truth
    /// for `clip_shape`/`hit_shape` so clipping and hit-testing can
    /// never diverge from the visible outline.
    painted_shape: Mutex<Shape>,
}

impl PalettePopup {
    fn new(
        shared: Arc<Mutex<PopupState>>,
        text_painter: Option<crate::text_paint::SharedTextPainter>,
        max_rows: usize,
    ) -> Self {
        let items = {
            let state = shared.lock();
            (0..state.items.len())
                .map(|index| CommandItem {
                    index,
                    shared: Arc::clone(&shared),
                    text_painter: text_painter.clone(),
                })
                .collect()
        };
        Self {
            scroll: ScrollView::new(CommandColumn {
                items,
                row_bounds: Vec::new(),
            }),
            shared,
            max_rows,
            bounds: None,
            painted_shape: Mutex::new(Shape::RECT),
        }
    }
}

impl Widget for PalettePopup {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let mut s = self.scroll.measure(cx, constraints);
        s.y = s.y.min(self.max_rows as f32 * cx.pt(ROW_H) + cx.pt(2.0));
        // Items measure as fill-width (`max_size.x`), so `s.x` would
        // report the whole viewport — size to the widest row's text
        // plus gutters instead.
        let widest = self
            .shared
            .lock()
            .items
            .iter()
            .map(|row| {
                row.title.chars().count() + row.subtitle.as_deref().map_or(0, |s| s.chars().count())
            })
            .max()
            .unwrap_or(0) as f32;
        s.x = (widest * cx.pt(7.0) + cx.pt(40.0)).clamp(
            cx.pt(160.0).min(constraints.max_size.x.max(0.0)),
            constraints.max_size.x.max(0.0),
        );
        s
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = Some(bounds);
        cx.layout_child(&mut self.scroll, bounds);
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::ListBox);
        node.set_orientation(accesskit::Orientation::Vertical);
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        // Keyboard on the popup itself: move the highlight / activate.
        // Production keyboard input stays with the owning field — this
        // path covers ownerless-embedded popups, mirroring
        // `AutoComplete`'s popup.
        if let WidgetEvent::KeyPressed { key, .. } = cx.event {
            let mut state = self.shared.lock();
            let n = state.items.len();
            if n > 0 {
                match key.as_str() {
                    "ArrowDown" => {
                        state.highlighted =
                            Some(state.highlighted.map_or(0, |h| (h + 1).min(n - 1)));
                        return EventResponse::RequestRepaint;
                    }
                    "ArrowUp" => {
                        state.highlighted =
                            Some(state.highlighted.map_or(n - 1, |h| h.saturating_sub(1)));
                        return EventResponse::RequestRepaint;
                    }
                    "Home" => {
                        state.highlighted = Some(0);
                        return EventResponse::RequestRepaint;
                    }
                    "End" => {
                        state.highlighted = Some(n - 1);
                        return EventResponse::RequestRepaint;
                    }
                    "Enter" | " " | "Space" => {
                        if let Some(h) = state.highlighted {
                            state.activated = Some(h);
                        }
                        return EventResponse::Handled;
                    }
                    _ => {}
                }
            }
            drop(state);
        }
        // Forward to the scroll view / results.
        self.scroll.event(cx)
    }

    fn paint(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        let rect = kurbo::Rect::new(
            f64::from(b.min_x()),
            f64::from(b.min_y()),
            f64::from(b.max_x()),
            f64::from(b.max_y()),
        );
        let popup_shape = Shape::rounded(cx.dim(TokenKey::BorderRadius, 6.0));
        *self.painted_shape.lock() = popup_shape.clone();
        cx.list.push_fill_shape(
            rect,
            &popup_shape,
            cx.color(TokenKey::SurfaceColor, POPUP_BG),
        );
        cx.list.push_stroke_shape(
            rect,
            &popup_shape,
            cx.pt(1.0),
            cx.color(TokenKey::BorderColor, POPUP_BORDER),
        );
    }

    fn clips_children(&self) -> bool {
        true
    }

    fn clip_shape(&self) -> Option<Shape> {
        Some(self.painted_shape.lock().clone())
    }

    fn hit_shape(&self) -> Option<Shape> {
        Some(self.painted_shape.lock().clone())
    }

    fn child_count(&self) -> usize {
        1
    }

    fn child(&self, _index: usize) -> Option<&dyn Widget> {
        Some(&self.scroll)
    }

    fn child_mut(&mut self, _index: usize) -> Option<&mut dyn Widget> {
        Some(&mut self.scroll)
    }

    fn child_bounds(&self, _index: usize) -> Option<Rect> {
        // The scroll view fills the whole popup inside its 1px border.
        self.bounds
    }
}

/// A fuzzy action launcher — a search field whose filtered command
/// list hangs in an overlay popup.
///
/// Owns no popup widget itself — [`sync_overlay`](Self::sync_overlay)
/// reconciles an overlay entry each frame so the result list paints
/// above window content and is emitted into the accessibility tree.
///
/// # Fuzzy scoring
///
/// A query matches an action when it is a *subsequence* of the title
/// or of any keyword (case-insensitive, leftmost-greedy). Matches
/// score by:
///
/// 1. **Exact-prefix bonus** — a candidate starting with the whole
///    query earns `+50`, so `"sav"` ranks `"Save"` above a scattered
///    match;
/// 2. **Consecutive-run bonus** — each matched character adjacent to
///    the previous one earns `+10`, favouring contiguous runs;
/// 3. **Word-boundary bonus** — each matched character at a word
///    start (index 0, after a non-alphanumeric separator, or a
///    camelCase hump) earns `+8`;
/// 4. **Shorter-title tiebreak** — equal scores order by title
///    length, then declaration order.
///
/// An empty query lists every action in declaration order. The list
/// is capped at [`max_results`](Self::max_results) entries (default
/// 8).
///
/// # Examples
///
/// ```
/// use martensite::widgets::{CommandAction, CommandPalette};
///
/// let pal = CommandPalette::new()
///     .actions([
///         CommandAction::new("file.open", "Open File…"),
///         CommandAction::new("app.quit", "Quit").keywords(["exit"]),
///     ])
///     .placeholder("Type a command…");
/// assert_eq!(pal.get_actions().len(), 2);
/// assert!(!pal.is_open());
/// ```
pub struct CommandPalette {
    /// Optional accessible label for the combobox.
    pub label: Option<String>,
    /// Whether the widget accepts input.
    pub enabled: bool,
    /// Placeholder text shown while the query is empty.
    pub placeholder: String,
    /// The embedded query field (internal child).
    field: TextInput,
    /// Field bounds assigned in `layout` (fills the widget).
    field_rect: Rect,
    /// Full action list in declaration order.
    actions: Vec<CommandAction>,
    /// Indices into `actions` matching the current query, in
    /// score-sorted display order and capped at `max_results`.
    filtered: Vec<usize>,
    /// Maximum results the popup lists.
    max_results: usize,
    /// Whether the popup is logically open.
    open: bool,
    /// Highlighted filtered index (the `aria-activedescendant`
    /// target while open).
    highlighted: Option<usize>,
    /// Overlay entry id of the open popup.
    popup_id: Option<u64>,
    /// State shared with popup widgets.
    shared: Arc<Mutex<PopupState>>,
    /// `take_activated` out-seam — the activated action's id.
    activated: Option<String>,
    /// Whether the widget holds keyboard focus.
    focused: bool,
    /// Cached bounds from the last layout pass.
    cached_bounds: Rect,
    /// The bounds the live popup was last anchored to — `sync_overlay`
    /// re-anchors when `cached_bounds` moves so an open listbox tracks
    /// its field instead of detaching.
    last_anchor: Option<Rect>,
    /// Bumped every [`refilter`](Self::refilter) — the live popup is
    /// rebuilt via `OverlayLayer::replace_content` when its baked
    /// items go stale.
    generation: u64,
    /// The `generation` the live popup's content was built from.
    built_generation: u64,
    /// Shared shaped-text painter — `paint` emits real `GlyphRun`s
    /// when set. Propagated to the field and popup rows.
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl CommandPalette {
    /// Creates a command palette with an empty action list.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::CommandPalette;
    ///
    /// let pal = CommandPalette::new();
    /// assert_eq!(pal.action_count(), 0);
    /// assert_eq!(pal.query(), "");
    /// assert!(!pal.is_open());
    /// ```
    pub fn new() -> Self {
        Self {
            label: None,
            enabled: true,
            placeholder: DEFAULT_PLACEHOLDER.to_string(),
            field: TextInput::new(DEFAULT_LABEL)
                .clearable(true)
                .suffix(SHORTCUT_HINT),
            field_rect: Rect::default(),
            actions: Vec::new(),
            filtered: Vec::new(),
            max_results: DEFAULT_MAX_RESULTS,
            open: false,
            highlighted: None,
            popup_id: None,
            shared: Arc::new(Mutex::new(PopupState {
                items: Vec::new(),
                highlighted: None,
                activated: None,
            })),
            activated: None,
            focused: false,
            cached_bounds: Rect::default(),
            last_anchor: None,
            generation: 0,
            built_generation: 0,
            text_painter: None,
        }
    }

    /// Sets the full action list (builder version).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{CommandAction, CommandPalette};
    ///
    /// let pal = CommandPalette::new().actions(vec![
    ///     CommandAction::new("a", "Alpha"),
    ///     CommandAction::new("b", "Beta"),
    /// ]);
    /// assert_eq!(pal.action_count(), 2);
    /// ```
    #[must_use]
    pub fn actions(mut self, actions: impl IntoIterator<Item = CommandAction>) -> Self {
        self.set_actions(actions);
        self
    }

    /// Replaces the action list and refilters against the current
    /// query (mutable version).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{CommandAction, CommandPalette};
    ///
    /// let mut pal = CommandPalette::new();
    /// pal.set_actions([CommandAction::new("a", "Alpha")]);
    /// assert_eq!(pal.action_count(), 1);
    /// ```
    pub fn set_actions(&mut self, actions: impl IntoIterator<Item = CommandAction>) {
        self.actions = actions.into_iter().collect();
        self.refilter();
        self.highlighted = if self.open { self.top_match() } else { None };
        self.push_shared();
    }

    /// Appends one action (builder version).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{CommandAction, CommandPalette};
    ///
    /// let pal = CommandPalette::new().action(CommandAction::new("a", "Alpha"));
    /// assert_eq!(pal.action_count(), 1);
    /// ```
    #[must_use]
    pub fn action(mut self, action: CommandAction) -> Self {
        self.add_action(action);
        self
    }

    /// Appends one action and refilters (mutable version).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{CommandAction, CommandPalette};
    ///
    /// let mut pal = CommandPalette::new();
    /// pal.add_action(CommandAction::new("a", "Alpha"));
    /// assert_eq!(pal.action_count(), 1);
    /// ```
    pub fn add_action(&mut self, action: CommandAction) {
        self.actions.push(action);
        self.refilter();
        self.push_shared();
    }

    /// The full action list in declaration order.
    ///
    /// Named `get_actions` rather than `actions` because the
    /// consuming builder [`actions`](Self::actions) already occupies
    /// that method name (Rust E0592 forbids the pair).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{CommandAction, CommandPalette};
    ///
    /// let pal = CommandPalette::new().actions([CommandAction::new("a", "Alpha")]);
    /// assert_eq!(pal.get_actions()[0].id, "a");
    /// ```
    #[inline]
    pub fn get_actions(&self) -> &[CommandAction] {
        &self.actions
    }

    /// Number of actions in the full list.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{CommandAction, CommandPalette};
    ///
    /// assert_eq!(CommandPalette::new().actions([CommandAction::new("a", "A")]).action_count(), 1);
    /// ```
    #[inline]
    pub fn action_count(&self) -> usize {
        self.actions.len()
    }

    /// Sets the placeholder text shown while the query is empty
    /// (default `"Search commands…"`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::CommandPalette;
    ///
    /// let pal = CommandPalette::new().placeholder("Type a command…");
    /// assert_eq!(pal.placeholder, "Type a command…");
    /// ```
    #[inline]
    #[must_use]
    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    /// Sets the maximum results the popup lists (default 8).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::CommandPalette;
    ///
    /// let pal = CommandPalette::new().max_results(5);
    /// assert_eq!(pal.maximum_results(), 5);
    /// ```
    #[must_use]
    pub fn max_results(mut self, max_results: usize) -> Self {
        self.max_results = max_results.max(1);
        self.refilter();
        self
    }

    /// The maximum results the popup lists.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::CommandPalette;
    ///
    /// assert_eq!(CommandPalette::new().maximum_results(), 8);
    /// ```
    #[inline]
    pub fn maximum_results(&self) -> usize {
        self.max_results
    }

    /// Sets the accessible label.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::CommandPalette;
    ///
    /// let pal = CommandPalette::new().label("Commands");
    /// assert_eq!(pal.label.as_deref(), Some("Commands"));
    /// ```
    #[inline]
    #[must_use]
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Sets whether the widget is enabled.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::CommandPalette;
    ///
    /// let pal = CommandPalette::new().enabled(false);
    /// assert!(!pal.enabled);
    /// ```
    #[inline]
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Shares a [`crate::text_paint::TextPainter`] so the field and
    /// popup rows emit real glyph runs instead of `DrawText`
    /// placeholder boxes.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::text_paint::shared_painter;
    /// use martensite::widgets::CommandPalette;
    ///
    /// let pal = CommandPalette::new().with_text_painter(shared_painter());
    /// ```
    #[must_use]
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.field = self.field.clone().with_text_painter(painter.clone());
        self.text_painter = Some(painter);
        self
    }

    /// The actions matching the current query, in score-sorted
    /// display order — the rows the popup shows. An empty query
    /// returns every action in declaration order.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{CommandAction, CommandPalette};
    ///
    /// let pal = CommandPalette::new()
    ///     .actions([
    ///         CommandAction::new("a", "Save As"),
    ///         CommandAction::new("b", "Save"),
    ///         CommandAction::new("c", "Close"),
    ///     ])
    ///     .with_query("sav");
    /// let ids: Vec<&str> = pal.filtered().iter().map(|a| a.id.as_str()).collect();
    /// assert_eq!(ids, ["b", "a"]);
    /// ```
    #[inline]
    pub fn filtered(&self) -> Vec<&CommandAction> {
        self.filtered.iter().map(|&i| &self.actions[i]).collect()
    }

    /// Sets the current query text (builder version).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::CommandPalette;
    ///
    /// let pal = CommandPalette::new().with_query("op");
    /// assert_eq!(pal.query(), "op");
    /// ```
    #[inline]
    #[must_use]
    pub fn with_query(mut self, query: impl Into<String>) -> Self {
        self.set_query(query);
        self
    }

    /// Sets the query text and refilters (mutable version — does not
    /// mark the field edited, matching `TextInput::set_value`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::CommandPalette;
    ///
    /// let mut pal = CommandPalette::new();
    /// pal.set_query("sav");
    /// assert_eq!(pal.query(), "sav");
    /// ```
    pub fn set_query(&mut self, query: impl Into<String>) {
        self.field.set_value(query);
        self.refilter();
        self.highlighted = if self.open { self.top_match() } else { None };
        self.refresh_open();
        self.push_shared();
    }

    /// The current query text.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::CommandPalette;
    ///
    /// assert_eq!(CommandPalette::new().with_query("x").query(), "x");
    /// ```
    #[inline]
    pub fn query(&self) -> &str {
        &self.field.value
    }

    /// Whether the result popup is logically open.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::CommandPalette;
    ///
    /// assert!(!CommandPalette::new().is_open());
    /// ```
    #[inline]
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// The highlighted filtered index (the `aria-activedescendant`
    /// target while open), or `None` before keyboard navigation
    /// begins.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::CommandPalette;
    ///
    /// assert_eq!(CommandPalette::new().highlighted(), None);
    /// ```
    #[inline]
    pub fn highlighted(&self) -> Option<usize> {
        self.highlighted
    }

    /// The overlay entry id of the open popup, if any.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::CommandPalette;
    ///
    /// assert_eq!(CommandPalette::new().popup_id(), None);
    /// ```
    #[inline]
    pub fn popup_id(&self) -> Option<u64> {
        self.popup_id
    }

    /// The activated action's id — an explicit result pick (popup
    /// click, `Enter` on a highlight) — or `None` when nothing has
    /// activated since the last call. The widget's activation
    /// out-seam (mirrors `AutoComplete::take_committed`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::CommandPalette;
    ///
    /// let mut pal = CommandPalette::new();
    /// assert_eq!(pal.take_activated(), None);
    /// ```
    #[inline]
    pub fn take_activated(&mut self) -> Option<String> {
        self.activated.take()
    }

    /// Opens the result popup with the top match highlighted —
    /// regardless of focus, the explicit `open()`/AT `Expand` path.
    /// No-op while disabled.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{CommandAction, CommandPalette};
    ///
    /// let mut pal = CommandPalette::new().actions([CommandAction::new("a", "Alpha")]);
    /// pal.open();
    /// assert!(pal.is_open());
    /// assert_eq!(pal.highlighted(), Some(0));
    /// ```
    pub fn open(&mut self) {
        if !self.enabled {
            return;
        }
        self.refilter();
        self.open = true;
        self.highlighted = self.top_match();
        self.push_shared();
    }

    /// Closes the popup, keeping the query text.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{CommandAction, CommandPalette};
    ///
    /// let mut pal = CommandPalette::new().actions([CommandAction::new("a", "Alpha")]);
    /// pal.open();
    /// pal.close();
    /// assert!(!pal.is_open());
    /// ```
    pub fn close(&mut self) {
        self.open = false;
        self.highlighted = None;
    }

    /// The top filtered index — `Some(0)` while results exist.
    #[inline]
    fn top_match(&self) -> Option<usize> {
        (!self.filtered.is_empty()).then_some(0)
    }

    /// Recomputes `filtered` from `actions` and the current query:
    /// an empty query lists every action in declaration order;
    /// otherwise each action keeps the best
    /// [`fuzzy_score`]/[`action_score`] over its title and keywords,
    /// sorted by score descending, then shorter title, then
    /// declaration order — and capped at `max_results`. Bumps
    /// `generation` so a live popup notices its baked items went
    /// stale.
    fn refilter(&mut self) {
        let query: Vec<char> = self
            .field
            .value
            .chars()
            .flat_map(|c| c.to_lowercase())
            .collect();
        if query.is_empty() {
            self.filtered = (0..self.actions.len()).collect();
        } else {
            let mut scored: Vec<(i64, usize)> = self
                .actions
                .iter()
                .enumerate()
                .filter_map(|(i, a)| action_score(&query, a).map(|s| (s, i)))
                .collect();
            scored.sort_by(|&(sa, ia), &(sb, ib)| {
                sb.cmp(&sa)
                    .then_with(|| {
                        self.actions[ia]
                            .title
                            .chars()
                            .count()
                            .cmp(&self.actions[ib].title.chars().count())
                    })
                    .then_with(|| ia.cmp(&ib))
            });
            self.filtered = scored.into_iter().map(|(_, i)| i).collect();
        }
        self.filtered.truncate(self.max_results);
        self.generation = self.generation.wrapping_add(1);
    }

    /// Reconciles `open` with focus — the automatic open paths
    /// (focus, edits, programmatic query writes) require focus: an
    /// unfocused write must not pop a result list over unrelated
    /// content. Explicit requests use [`open`](Self::open) instead.
    fn refresh_open(&mut self) {
        self.open = self.enabled && self.focused;
        if !self.open {
            self.highlighted = None;
        }
    }

    /// The user edited the query text: refilter, reset the highlight
    /// to the top match, and reconcile the popup's open state.
    fn on_edit(&mut self) {
        self.refilter();
        self.refresh_open();
        if self.open {
            self.highlighted = self.top_match();
        }
        self.push_shared();
    }

    /// Moves the highlight by `delta` with clamping (APG keeps the
    /// highlight inside the list).
    fn move_highlight(&mut self, delta: i64) {
        let n = self.filtered.len() as i64;
        if n == 0 {
            return;
        }
        let next = match self.highlighted {
            Some(h) => (h as i64 + delta).clamp(0, n - 1) as usize,
            None if delta >= 0 => 0,
            None => (n - 1) as usize,
        };
        self.highlighted = Some(next);
        self.push_shared();
    }

    /// Highlights `index` directly (`Home`/`End`) with the same
    /// clamping as [`move_highlight`](Self::move_highlight).
    fn highlight_index(&mut self, index: usize) {
        if self.filtered.is_empty() {
            return;
        }
        self.highlighted = Some(index.min(self.filtered.len() - 1));
        self.push_shared();
    }

    /// Activates filtered index `index`: reports the action's id
    /// through [`take_activated`](Self::take_activated) and closes.
    /// Returns whether an action was activated.
    fn activate_index(&mut self, index: usize) -> bool {
        if let Some(&action_idx) = self.filtered.get(index) {
            self.activated = Some(self.actions[action_idx].id.clone());
            self.close();
            return true;
        }
        false
    }

    /// Activates the highlighted result, if any.
    fn activate_highlighted(&mut self) -> bool {
        self.highlighted.is_some_and(|h| self.activate_index(h))
    }

    /// Mirrors widget state into the shared popup state.
    fn push_shared(&self) {
        let mut state = self.shared.lock();
        state.items = self
            .filtered
            .iter()
            .map(|&i| PopupRow {
                title: self.actions[i].title.clone(),
                subtitle: self.actions[i].subtitle.clone(),
            })
            .collect();
        state.highlighted = self.highlighted;
    }

    /// Applies state the popup wrote into the shared slot: an
    /// activated pick (click or AT `Click` on an option) is reported
    /// here, and popup-side highlight changes are mirrored so
    /// `aria-activedescendant` follows the pointer.
    fn drain_shared_state(&mut self) {
        let committed = self.shared.lock().activated.take();
        if let Some(index) = committed {
            self.activate_index(index);
        }
        // Popup-side hover moved the highlight — mirror it so
        // aria-activedescendant follows the pointer.
        let popup_highlight = self.shared.lock().highlighted;
        self.highlighted = popup_highlight
            .map(|h| h.min(self.filtered.len().saturating_sub(1)))
            .filter(|_| self.open);
    }

    /// Mirrors widget state onto the embedded field.
    fn sync_field(&mut self) {
        self.field.enabled = self.enabled;
        self.field.label = self
            .label
            .clone()
            .unwrap_or_else(|| DEFAULT_LABEL.to_string());
        self.field.placeholder.clone_from(&self.placeholder);
    }

    /// Forwards an event to the embedded field the way the default
    /// `Widget::event` child walk would: positional events only inside
    /// the field's bounds, except moves and releases which always
    /// reach it so a captured drag keeps tracking outside.
    fn forward(&mut self, cx: &mut EventContext) -> EventResponse {
        if let Some(pos) = cx.event.position() {
            let drag = matches!(
                cx.event,
                WidgetEvent::PointerMoved { .. } | WidgetEvent::PointerReleased { .. }
            );
            if !drag && !self.field_rect.contains(pos) {
                return EventResponse::Ignored;
            }
        }
        let mut child_cx = EventContext {
            event: cx.event,
            bounds: self.field_rect,
            scale: cx.scale,
        };
        self.field.event(&mut child_cx)
    }

    /// Forwards an event to the field, then applies the edit protocol:
    /// a user-driven mutation refilters the popup and resets the
    /// highlight to the top match.
    fn forward_then_edit(&mut self, cx: &mut EventContext) -> EventResponse {
        let response = self.forward(cx);
        if self.field.take_edited() {
            self.on_edit();
            EventResponse::RequestRepaint
        } else {
            response
        }
    }

    /// Reconciles the overlay with the palette's open state.
    ///
    /// Call once per frame before `OverlayLayer::layout_pass`:
    ///
    /// - applies a result activation made inside the popup;
    /// - opens/closes the popup entry to match
    ///   [`is_open`](Self::is_open);
    /// - rebuilds the live popup's content in place when the filtered
    ///   list changed under it (`OverlayLayer::replace_content`);
    /// - notices overlay-level dismissal (outside press, `Escape`) and
    ///   closes.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{CommandAction, CommandPalette};
    /// use martensite_core::overlay::OverlayLayer;
    /// use martensite_core::widget::Widget;
    /// use martensite_core::{EventContext, HotNode, LayoutContext, Rect, WidgetEvent};
    ///
    /// let mut pal = CommandPalette::new().actions([CommandAction::new("a", "Alpha")]);
    /// let mut hot = HotNode::default();
    /// let mut cx = LayoutContext {
    ///     hot: &mut hot,
    ///     scale: 1.0,
    /// };
    /// pal.layout(&mut cx, Rect::new(10.0, 10.0, 280.0, 28.0));
    /// pal.open();
    ///
    /// let mut overlay = OverlayLayer::new();
    /// overlay.set_viewport(Rect::new(0.0, 0.0, 800.0, 600.0));
    /// pal.sync_overlay(&mut overlay);
    /// overlay.layout_pass();
    /// assert_eq!(overlay.len(), 1);
    /// ```
    pub fn sync_overlay(&mut self, overlay: &mut OverlayLayer) {
        self.drain_shared_state();
        // The layer dismissed our popup (outside press / Escape).
        if let Some(id) = self.popup_id {
            if !overlay.is_open(id) {
                self.popup_id = None;
                self.last_anchor = None;
                self.close();
            }
        }
        if self.open && self.popup_id.is_none() {
            self.push_shared();
            let popup = PalettePopup::new(
                Arc::clone(&self.shared),
                self.text_painter.clone(),
                self.max_results,
            );
            self.popup_id =
                Some(overlay.open(Box::new(popup), OverlayAnchor::Bounds(self.cached_bounds)));
            self.built_generation = self.generation;
            self.last_anchor = Some(self.cached_bounds);
        } else if !self.open {
            if let Some(id) = self.popup_id.take() {
                overlay.close(id);
            }
            self.last_anchor = None;
        } else if let Some(id) = self.popup_id {
            // The filtered list changed under the live popup — rebuild
            // its content in place so the entry keeps its id and
            // z-position.
            if self.built_generation != self.generation {
                self.push_shared();
                let popup = PalettePopup::new(
                    Arc::clone(&self.shared),
                    self.text_painter.clone(),
                    self.max_results,
                );
                overlay.replace_content(id, Box::new(popup));
                self.built_generation = self.generation;
            }
            // The field moved while open (resize, scale change,
            // relayout) — re-anchor so the listbox tracks it. Guarded
            // on change so a settled popup doesn't re-mark layout
            // every tick.
            if self.last_anchor != Some(self.cached_bounds) {
                overlay.set_anchor(id, OverlayAnchor::Bounds(self.cached_bounds));
                self.last_anchor = Some(self.cached_bounds);
            }
        }
    }
}

impl Default for CommandPalette {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for CommandPalette {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        // A one-line editable field — wider than a plain autocomplete
        // because a launcher is the window's primary surface.
        Vec2::new(
            cx.pt(FIELD_W).min(constraints.max_size.x.max(0.0)),
            cx.pt(FIELD_H).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        // A narrower or shorter face cannot render the text lane
        // legibly.
        RenderMinimum::new(Vec2::new(200.0, FIELD_H)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.cached_bounds = bounds;
        // Declare keyboard focusability on the arena node so
        // press-to-focus applies; key/IME input then forwards into
        // the field child.
        if self.enabled {
            cx.hot.flags |= NodeFlags::FOCUSABLE;
        } else {
            cx.hot.flags.remove(NodeFlags::FOCUSABLE);
        }
        self.sync_field();
        self.field_rect = bounds;
        cx.layout_child(&mut self.field, self.field_rect);
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::ComboBox);
        node.set_label(self.label.as_deref().unwrap_or(DEFAULT_LABEL));
        node.set_value(self.field.value.as_str());
        node.set_has_popup(accesskit::HasPopup::Listbox);
        node.set_auto_complete(accesskit::AutoComplete::List);
        node.set_expanded(self.open);
        node.add_action(accesskit::Action::SetValue);
        node.add_action(accesskit::Action::Expand);
        node.add_action(accesskit::Action::Collapse);
        if self.enabled {
            node.add_action(accesskit::Action::Focus);
        }
        if !self.enabled {
            node.set_disabled();
        }
    }

    fn a11y_prepare(&mut self) {
        // AT activations delivered to popup options (an `Action::Click`
        // on a ListBoxOption virtual node) write into the shared slot;
        // drain them here so the emitted tree reflects the activation
        // even when the action bypassed `sync_overlay`.
        self.drain_shared_state();
    }

    fn a11y_fixup(
        &self,
        _emitted: &mut Vec<A11yEmittedNode>,
        overlay_nodes: &[OverlayA11yRef],
        this_node: &mut AccessKitNode,
    ) {
        let Some(popup) = self.popup_id else {
            return;
        };
        // aria-controls → the popup's ListBox root.
        if let Some(listbox_id) = overlay_nodes
            .iter()
            .find(|r| r.entry == popup && r.path.is_empty())
            .map(|r| r.id)
        {
            this_node.set_controls(vec![listbox_id]);
        }
        // aria-activedescendant → the highlighted option. Options live
        // at path [0, 0, i]: PalettePopup → ScrollView → CommandColumn.
        if let Some(highlighted) = self.highlighted {
            let option_path = [0u32, 0, highlighted as u32];
            if let Some(option_id) = overlay_nodes
                .iter()
                .find(|r| r.entry == popup && r.path.as_slice() == option_path)
                .map(|r| r.id)
            {
                this_node.set_active_descendant(option_id);
            }
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.enabled {
            return EventResponse::Ignored;
        }
        match cx.event {
            WidgetEvent::KeyPressed { key, .. } => match key.as_str() {
                // While the popup is open this branch is normally
                // unreachable for `Escape`: the arena-owned
                // OverlayLayer consumes it first (dismissing the
                // topmost popup), and `sync_overlay` reconciles via
                // `close`. It's kept for ownerless-embedded use — a
                // CommandPalette driven without arena overlay routing
                // — where no layer intercepts the key.
                "Escape" => {
                    if self.open {
                        self.close();
                        EventResponse::RequestRepaint
                    } else {
                        // Let the field collapse a selection.
                        self.forward(cx)
                    }
                }
                "ArrowDown" => {
                    if self.open {
                        self.move_highlight(1);
                        EventResponse::RequestRepaint
                    } else {
                        self.forward_then_edit(cx)
                    }
                }
                "ArrowUp" => {
                    if self.open {
                        self.move_highlight(-1);
                        EventResponse::RequestRepaint
                    } else {
                        self.forward_then_edit(cx)
                    }
                }
                "Home" if self.open => {
                    self.highlight_index(0);
                    EventResponse::RequestRepaint
                }
                "End" if self.open => {
                    self.highlight_index(self.filtered.len().saturating_sub(1));
                    EventResponse::RequestRepaint
                }
                "Enter" => {
                    // Activate the highlighted result — the palette's
                    // whole job. Closed fields keep their own Enter
                    // semantics (none for a single-line input).
                    if self.open && self.activate_highlighted() {
                        EventResponse::RequestRepaint
                    } else {
                        self.forward_then_edit(cx)
                    }
                }
                _ => self.forward_then_edit(cx),
            },
            WidgetEvent::FocusGained => {
                self.focused = true;
                let response = self.forward(cx);
                self.refilter();
                self.refresh_open();
                if self.open {
                    self.highlighted = self.top_match();
                }
                self.push_shared();
                response
            }
            WidgetEvent::FocusLost => {
                self.focused = false;
                // Blur dismisses the popup, keeping the query.
                self.close();
                self.forward(cx)
            }
            WidgetEvent::SemanticAction(action) => match action {
                SemanticAction::SetValue(text) => {
                    self.set_query(text.clone());
                    EventResponse::RequestRepaint
                }
                SemanticAction::Expand => {
                    self.open();
                    EventResponse::RequestRepaint
                }
                SemanticAction::Collapse => {
                    self.close();
                    EventResponse::RequestRepaint
                }
                SemanticAction::Click => {
                    // Trigger-face activation toggles the palette.
                    if self.open {
                        self.close();
                    } else {
                        self.open();
                    }
                    EventResponse::RequestRepaint
                }
                SemanticAction::Focus => EventResponse::CaptureFocus,
                _ => EventResponse::Ignored,
            },
            _ => self.forward_then_edit(cx),
        }
    }

    fn sync_overlay(&mut self, overlay: &mut OverlayLayer) {
        // Delegate to the inherent method so
        // `CommandPalette::sync_overlay` and the `Widget` trait seam
        // stay in lock-step.
        CommandPalette::sync_overlay(self, overlay);
    }

    fn child_count(&self) -> usize {
        1
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        (index == 0).then_some(&self.field as &dyn Widget)
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        (index == 0).then_some(&mut self.field as &mut dyn Widget)
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        (index == 0).then_some(self.field_rect)
    }
}

impl std::fmt::Debug for CommandPalette {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CommandPalette")
            .field("label", &self.label)
            .field("query", &self.field.value)
            .field("enabled", &self.enabled)
            .field("open", &self.open)
            .field("filtered", &self.filtered.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn act(id: &str, title: &str) -> CommandAction {
        CommandAction::new(id, title)
    }

    fn palette(actions: &[(&str, &str)]) -> CommandPalette {
        CommandPalette::new().actions(
            actions
                .iter()
                .map(|(id, title)| act(id, title))
                .collect::<Vec<_>>(),
        )
    }

    fn laid_out(pal: &mut CommandPalette) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        pal.layout(&mut cx, Rect::new(10.0, 10.0, 280.0, 28.0));
    }

    fn overlay() -> OverlayLayer {
        let mut o = OverlayLayer::new();
        o.set_viewport(Rect::new(0.0, 0.0, 800.0, 600.0));
        o
    }

    fn key(k: &str) -> WidgetEvent {
        WidgetEvent::KeyPressed {
            key: k.to_string(),
            repeat: false,
        }
    }

    fn event(pal: &mut CommandPalette, ev: &WidgetEvent) -> EventResponse {
        let mut cx = EventContext {
            event: ev,
            bounds: pal.cached_bounds,
            scale: 1.0,
        };
        pal.event(&mut cx)
    }

    fn focus(pal: &mut CommandPalette) {
        event(pal, &WidgetEvent::FocusGained);
    }

    fn type_text(pal: &mut CommandPalette, text: &str) {
        event(
            pal,
            &WidgetEvent::ImeCommitted {
                text: text.to_string(),
            },
        );
    }

    fn filtered_ids(pal: &CommandPalette) -> Vec<String> {
        pal.filtered().iter().map(|a| a.id.clone()).collect()
    }

    #[test]
    fn empty_query_lists_all_in_declaration_order() {
        let pal = palette(&[("a", "Alpha"), ("b", "Beta"), ("c", "Gamma")]);
        assert_eq!(filtered_ids(&pal), ["a", "b", "c"]);
    }

    #[test]
    fn fuzzy_prefix_beats_scattered() {
        // "sav" prefix-matches both Save variants; the shorter title
        // wins the tiebreak. "Autosave" only matches scattered (no
        // prefix, no boundary hit) and ranks last.
        let pal = palette(&[
            ("autosave", "Autosave"),
            ("save_as", "Save As"),
            ("save", "Save"),
        ])
        .with_query("sav");
        assert_eq!(filtered_ids(&pal), ["save", "save_as", "autosave"]);
    }

    #[test]
    fn fuzzy_subsequence_word_boundaries() {
        // "sf" matches inside both — word-boundary hits rank the
        // shorter title first, and non-matches drop out.
        let pal = palette(&[
            ("search", "Search Files"),
            ("profile", "Profile"),
            ("save", "Save File"),
        ])
        .with_query("sf");
        assert_eq!(filtered_ids(&pal), ["save", "search"]);
    }

    #[test]
    fn fuzzy_case_insensitive() {
        let pal = palette(&[("open", "Open File")]).with_query("OPEN");
        assert_eq!(filtered_ids(&pal), ["open"]);
    }

    #[test]
    fn fuzzy_keywords_match() {
        let pal = CommandPalette::new()
            .actions([
                CommandAction::new("quit", "Quit").keywords(["exit", "shutdown"]),
                act("open", "Open"),
            ])
            .with_query("exit");
        assert_eq!(filtered_ids(&pal), ["quit"]);
    }

    #[test]
    fn fuzzy_no_subsequence_no_match() {
        let pal = palette(&[("open", "Open File")]).with_query("xyz");
        assert!(pal.filtered().is_empty());
    }

    #[test]
    fn max_results_caps_list() {
        let pal = CommandPalette::new()
            .actions(
                (0..10)
                    .map(|i| act(&format!("a{i}"), "Save"))
                    .collect::<Vec<_>>(),
            )
            .max_results(3);
        // Even the empty-query listing honours the cap.
        assert_eq!(pal.filtered().len(), 3);
        let pal = pal.with_query("sav");
        assert_eq!(pal.filtered().len(), 3);
    }

    #[test]
    fn focus_opens_popup() {
        let mut pal = palette(&[("a", "Alpha")]);
        laid_out(&mut pal);
        assert!(!pal.is_open());
        focus(&mut pal);
        assert!(pal.is_open());
        assert_eq!(pal.highlighted(), Some(0));
    }

    #[test]
    fn typing_refilters_and_resets_highlight() {
        let mut pal = palette(&[("a", "Alpha"), ("b", "Beta"), ("c", "Gamma")]);
        laid_out(&mut pal);
        focus(&mut pal);
        event(&mut pal, &key("ArrowDown"));
        assert_eq!(pal.highlighted(), Some(1));
        type_text(&mut pal, "a");
        // "a" is a subsequence of all three titles — but the highlight
        // snapped back to the top match.
        assert_eq!(pal.filtered().len(), 3);
        assert_eq!(pal.highlighted(), Some(0));
        type_text(&mut pal, "l");
        assert_eq!(filtered_ids(&pal), ["a"]);
        assert_eq!(pal.highlighted(), Some(0));
    }

    #[test]
    fn arrows_move_highlight_clamped() {
        let mut pal = palette(&[("a", "A"), ("b", "B"), ("c", "C")]);
        laid_out(&mut pal);
        pal.open();
        event(&mut pal, &key("ArrowDown"));
        event(&mut pal, &key("ArrowDown"));
        event(&mut pal, &key("ArrowDown")); // clamps at last
        assert_eq!(pal.highlighted(), Some(2));
        event(&mut pal, &key("ArrowUp"));
        assert_eq!(pal.highlighted(), Some(1));
        event(&mut pal, &key("Home"));
        assert_eq!(pal.highlighted(), Some(0));
        event(&mut pal, &key("End"));
        assert_eq!(pal.highlighted(), Some(2));
    }

    #[test]
    fn enter_activates_highlighted_id() {
        let mut pal = palette(&[("a", "Alpha"), ("b", "Beta")]);
        laid_out(&mut pal);
        pal.open();
        event(&mut pal, &key("ArrowDown"));
        event(&mut pal, &key("Enter"));
        assert_eq!(pal.take_activated(), Some("b".to_string()));
        assert!(!pal.is_open());
        // The seam drains once.
        assert_eq!(pal.take_activated(), None);
    }

    #[test]
    fn enter_on_top_match_without_navigation() {
        let mut pal = palette(&[("a", "Alpha"), ("b", "Beta")]);
        laid_out(&mut pal);
        pal.open();
        event(&mut pal, &key("Enter"));
        assert_eq!(pal.take_activated(), Some("a".to_string()));
    }

    #[test]
    fn enter_with_no_results_does_nothing() {
        let mut pal = palette(&[("a", "Alpha")]).with_query("zzz");
        laid_out(&mut pal);
        pal.open();
        event(&mut pal, &key("Enter"));
        assert_eq!(pal.take_activated(), None);
    }

    #[test]
    fn escape_closes_keeping_query() {
        let mut pal = palette(&[("a", "Alpha")]).with_query("a");
        laid_out(&mut pal);
        pal.open();
        event(&mut pal, &key("Escape"));
        assert!(!pal.is_open());
        assert_eq!(pal.query(), "a");
        assert_eq!(pal.take_activated(), None);
    }

    #[test]
    fn blur_closes_popup() {
        let mut pal = palette(&[("a", "Alpha")]);
        laid_out(&mut pal);
        focus(&mut pal);
        assert!(pal.is_open());
        event(&mut pal, &WidgetEvent::FocusLost);
        assert!(!pal.is_open());
    }

    #[test]
    fn set_query_refilters() {
        let mut pal = palette(&[("a", "Alpha"), ("b", "Beta")]);
        pal.set_query("bet");
        assert_eq!(filtered_ids(&pal), ["b"]);
        assert_eq!(pal.query(), "bet");
    }

    #[test]
    fn overlay_opens_listbox_below() {
        let mut pal = palette(&[("a", "Alpha")]);
        laid_out(&mut pal);
        pal.open();
        let mut o = overlay();
        pal.sync_overlay(&mut o);
        o.layout_pass();
        assert_eq!(o.len(), 1);
        let b = o.entry_bounds(pal.popup_id.unwrap()).unwrap();
        // Below the field face.
        assert!(b.min_y() >= 38.0);
    }

    #[test]
    fn popup_option_click_activates() {
        let mut pal = palette(&[("a", "Alpha"), ("b", "Beta")]);
        laid_out(&mut pal);
        pal.open();
        let mut o = overlay();
        pal.sync_overlay(&mut o);
        o.layout_pass();
        let id = pal.popup_id.unwrap();
        // Reach the option at path [0, 0, 1] inside the popup and
        // press it.
        let option = o.widget_at_mut(id, &[0, 0, 1]).expect("option widget");
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(5.0, 5.0),
            button: PointerButton::Primary,
            count: 1,
        };
        let mut cx = EventContext {
            event: &press,
            bounds: Rect::default(),
            scale: 1.0,
        };
        assert_eq!(option.event(&mut cx), EventResponse::Handled);
        pal.sync_overlay(&mut o);
        assert_eq!(pal.take_activated(), Some("b".to_string()));
        assert!(!pal.is_open());
    }

    #[test]
    fn outside_press_dismisses_and_reconciles() {
        let mut pal = palette(&[("a", "Alpha")]);
        laid_out(&mut pal);
        pal.open();
        let mut o = overlay();
        pal.sync_overlay(&mut o);
        o.layout_pass();
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(700.0, 500.0),
            button: PointerButton::Primary,
            count: 1,
        };
        assert_eq!(o.dispatch_event(&press), EventResponse::Ignored);
        pal.sync_overlay(&mut o);
        assert!(!pal.is_open());
        assert_eq!(pal.popup_id, None);
    }

    #[test]
    fn refilter_rebuilds_live_popup() {
        let mut pal = palette(&[("a", "Alpha"), ("b", "Alps"), ("c", "Beta")]).with_query("a");
        laid_out(&mut pal);
        focus(&mut pal);
        let mut o = overlay();
        pal.sync_overlay(&mut o);
        o.layout_pass();
        let id = pal.popup_id.unwrap();
        // Narrow the filter — the same entry id serves the shorter
        // list (replace_content, not close/reopen).
        type_text(&mut pal, "l");
        pal.sync_overlay(&mut o);
        assert_eq!(pal.popup_id, Some(id));
        o.layout_pass();
        // The rebuilt popup exposes exactly the filtered rows.
        let column = o.widget_at_mut(id, &[0, 0]).expect("column");
        assert_eq!(column.child_count(), 2);
    }

    #[test]
    fn layer_escape_dismissal_reconciles() {
        let mut pal = palette(&[("a", "Alpha")]);
        laid_out(&mut pal);
        pal.open();
        let mut o = overlay();
        pal.sync_overlay(&mut o);
        o.layout_pass();
        // The layer consumes Escape and drops the entry; the next
        // sync reconciles.
        assert_eq!(o.dispatch_event(&key("Escape")), EventResponse::Handled);
        pal.sync_overlay(&mut o);
        assert!(!pal.is_open());
        assert_eq!(pal.popup_id, None);
    }

    #[test]
    fn popup_keyboard_navigation_mirrors_highlight() {
        let mut pal = palette(&[("a", "A"), ("b", "B")]);
        laid_out(&mut pal);
        pal.open();
        let mut o = overlay();
        pal.sync_overlay(&mut o);
        o.layout_pass();
        let id = pal.popup_id.unwrap();
        // The layer offers non-Escape keys to the topmost popup first.
        assert_eq!(
            o.dispatch_event(&key("ArrowDown")),
            EventResponse::RequestRepaint
        );
        pal.sync_overlay(&mut o);
        assert_eq!(pal.highlighted(), Some(1));
        let _ = id;
    }

    #[test]
    fn combobox_accessibility() {
        let mut pal = palette(&[("a", "A")]).label("Commands").with_query("a");
        laid_out(&mut pal);
        pal.open();
        let mut node = AccessKitNode::new(accesskit::Role::Unknown);
        pal.accessibility(&mut node);
        assert_eq!(node.role(), accesskit::Role::ComboBox);
        assert_eq!(node.label(), Some("Commands"));
        assert_eq!(node.value(), Some("a"));
        assert_eq!(node.has_popup(), Some(accesskit::HasPopup::Listbox));
        assert_eq!(node.is_expanded(), Some(true));
        assert!(node.supports_action(accesskit::Action::Expand));
        assert!(node.supports_action(accesskit::Action::Collapse));
        assert_eq!(pal.child_count(), 1);
    }

    #[test]
    fn semantic_expand_collapse_set_value() {
        let mut pal = palette(&[("a", "Alpha")]);
        laid_out(&mut pal);
        event(
            &mut pal,
            &WidgetEvent::SemanticAction(SemanticAction::Expand),
        );
        assert!(pal.is_open());
        event(
            &mut pal,
            &WidgetEvent::SemanticAction(SemanticAction::Collapse),
        );
        assert!(!pal.is_open());
        focus(&mut pal);
        event(
            &mut pal,
            &WidgetEvent::SemanticAction(SemanticAction::SetValue("alp".to_string())),
        );
        assert_eq!(pal.query(), "alp");
        assert_eq!(filtered_ids(&pal), ["a"]);
        assert!(pal.is_open());
    }

    #[test]
    fn disabled_ignores_events() {
        let mut pal = palette(&[("a", "Alpha")]).enabled(false);
        laid_out(&mut pal);
        pal.open();
        assert!(!pal.is_open());
        assert_eq!(event(&mut pal, &key("Enter")), EventResponse::Ignored);
        assert_eq!(
            event(&mut pal, &WidgetEvent::FocusGained),
            EventResponse::Ignored
        );
        let mut node = AccessKitNode::new(accesskit::Role::Unknown);
        pal.accessibility(&mut node);
        assert!(node.is_disabled());
    }
}
