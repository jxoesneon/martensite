//! Drop target registration, validation, and lifecycle.
//!
//! A [`DropTarget`] describes a single widget region willing to accept drops,
//! along with the MIME types and [`DropEffect`]s it supports. A
//! [`DropTargetRegistry`] tracks all registered targets and drives the
//! drag-enter / drag-leave / drop lifecycle against an active
//! [`DndSession`].

use crate::{DndSession, DropEffect};
use bitflags::bitflags;
use glam::Vec2;
use martensite_core::{Rect, WidgetId};
use std::collections::HashMap;

/// Unique identifier for a [`DropTarget`] within a [`DropTargetRegistry`].
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TargetId(pub u64);

bitflags! {
    /// A bitmask of [`DropEffect`]s that a drop target is willing to accept.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_dnd::{DropEffect, DropEffectMask};
    ///
    /// let mask = DropEffectMask::COPY | DropEffectMask::MOVE;
    /// assert!(mask.contains(DropEffectMask::COPY));
    /// assert!(mask.contains(DropEffectMask::MOVE));
    /// assert!(!mask.contains(DropEffectMask::LINK));
    /// ```
    #[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
    pub struct DropEffectMask: u8 {
        /// Accepts [`DropEffect::Copy`].
        const COPY = 0b0000_0001;
        /// Accepts [`DropEffect::Move`].
        const MOVE = 0b0000_0010;
        /// Accepts [`DropEffect::Link`].
        const LINK = 0b0000_0100;
        /// Accepts no effects.
        const NONE = 0;
    }
}

impl DropEffectMask {
    /// Returns `true` if this mask accepts the given `effect`.
    ///
    /// [`DropEffect::None`] is accepted only by the empty mask semantics of
    /// the caller; this method returns `false` for [`DropEffect::None`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_dnd::{DropEffect, DropEffectMask};
    ///
    /// let mask = DropEffectMask::COPY;
    /// assert!(mask.accepts(DropEffect::Copy));
    /// assert!(!mask.accepts(DropEffect::Move));
    /// ```
    pub fn accepts(self, effect: DropEffect) -> bool {
        match effect {
            DropEffect::None => false,
            DropEffect::Copy => self.contains(Self::COPY),
            DropEffect::Move => self.contains(Self::MOVE),
            DropEffect::Link => self.contains(Self::LINK),
        }
    }

    /// Converts a single [`DropEffect`] into its corresponding mask bit.
    ///
    /// [`DropEffect::None`] maps to [`DropEffectMask::empty`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_dnd::{DropEffect, DropEffectMask};
    ///
    /// assert_eq!(DropEffectMask::from_effect(DropEffect::Copy), DropEffectMask::COPY);
    /// assert_eq!(DropEffectMask::from_effect(DropEffect::None), DropEffectMask::empty());
    /// ```
    pub fn from_effect(effect: DropEffect) -> Self {
        match effect {
            DropEffect::None => Self::empty(),
            DropEffect::Copy => Self::COPY,
            DropEffect::Move => Self::MOVE,
            DropEffect::Link => Self::LINK,
        }
    }
}

/// Lifecycle state of a single [`DropTarget`] during a drag operation.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum DropTargetState {
    /// The target is not currently hovered by an active drag.
    #[default]
    Idle,
    /// An active drag is hovering over the target and the target accepts it.
    Hovered,
    /// An active drag is hovering over the target but the target rejects it
    /// (type or effect mismatch).
    Rejected,
}

/// A single widget region willing to accept drag-and-drop drops.
///
/// # Examples
///
/// ```
/// use martensite_dnd::{DropEffect, DropEffectMask, DropTarget};
/// use martensite_core::{Rect, WidgetId};
///
/// let target = DropTarget::new(
///     WidgetId::from_parts(1, 1),
///     vec!["text/plain".into()],
///     DropEffectMask::COPY | DropEffectMask::MOVE,
/// );
/// assert!(target.accepts_type("text/plain"));
/// assert!(target.accepts_effect(DropEffect::Copy));
/// ```
pub struct DropTarget {
    /// The widget that owns this drop target.
    pub widget_id: WidgetId,
    /// MIME types accepted by this target, stored in canonicalized form
    /// (lowercased type/subtype, surrounding whitespace trimmed) so that
    /// comparisons are case-insensitive.
    pub accepted_types: Vec<String>,
    /// Bitmask of [`DropEffect`]s accepted by this target.
    pub accepted_effects: DropEffectMask,
    /// Screen-space bounds of the target region.
    pub bounds: Rect,
    /// Preferred [`DropEffect`] to negotiate when multiple effects are
    /// mutually acceptable. When `None` (the default), negotiation falls
    /// back to the built-in Copy > Move > Link priority.
    pub preferred_effect: Option<DropEffect>,
    state: DropTargetState,
}

impl DropTarget {
    /// Creates a new drop target with zero-sized bounds and the
    /// [`DropTargetState::Idle`] state.
    ///
    /// Use [`DropTarget::with_bounds`] to assign screen-space bounds.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_dnd::{DropEffectMask, DropTarget};
    /// use martensite_core::WidgetId;
    ///
    /// let target = DropTarget::new(WidgetId::from_parts(1, 1), vec![], DropEffectMask::COPY);
    /// assert_eq!(target.widget_id, WidgetId::from_parts(1, 1));
    /// ```
    pub fn new(
        widget_id: WidgetId,
        accepted_types: Vec<String>,
        accepted_effects: DropEffectMask,
    ) -> Self {
        Self {
            widget_id,
            accepted_types: accepted_types
                .into_iter()
                .map(|t| canonicalize_mime(&t))
                .collect(),
            accepted_effects,
            bounds: Rect::default(),
            preferred_effect: None,
            state: DropTargetState::Idle,
        }
    }

    /// Builder-style setter for the target's screen-space bounds.
    ///
    /// # Panics
    ///
    /// Panics if the bounds' origin or size contains a non-finite value
    /// (NaN or infinity). Non-finite bounds would make hit-testing
    /// meaningless, so they are rejected eagerly with a clear message,
    /// mirroring the validation pattern used by `DpiScale::new` in the
    /// `martensite-window` crate.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_dnd::{DropEffectMask, DropTarget};
    /// use martensite_core::Rect;
    /// use martensite_core::WidgetId;
    ///
    /// let target = DropTarget::new(WidgetId::from_parts(1, 1), vec![], DropEffectMask::COPY)
    ///     .with_bounds(Rect::new(10.0, 20.0, 100.0, 50.0));
    /// assert_eq!(target.bounds.min_x(), 10.0);
    /// ```
    pub fn with_bounds(mut self, bounds: Rect) -> Self {
        assert!(
            bounds.origin.is_finite() && bounds.size.is_finite(),
            "DropTarget bounds must be finite (no NaN or infinity), got origin {:?} size {:?}",
            bounds.origin,
            bounds.size,
        );
        self.bounds = bounds;
        self
    }

    /// Builder-style setter for the target's preferred [`DropEffect`].
    ///
    /// When negotiating a drop, if the preferred effect is among the effects
    /// mutually accepted by the target and the source, it is chosen. Otherwise
    /// negotiation falls back to the built-in Copy > Move > Link priority.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_dnd::{DropEffect, DropEffectMask, DropTarget};
    /// use martensite_core::WidgetId;
    ///
    /// let target = DropTarget::new(
    ///     WidgetId::from_parts(1, 1),
    ///     vec!["text/plain".into()],
    ///     DropEffectMask::COPY | DropEffectMask::MOVE,
    /// )
    /// .with_preferred_effect(DropEffect::Move);
    /// assert_eq!(target.preferred_effect, Some(DropEffect::Move));
    /// ```
    pub fn with_preferred_effect(mut self, effect: DropEffect) -> Self {
        self.preferred_effect = Some(effect);
        self
    }

    /// Returns `true` if this target accepts the given MIME type.
    ///
    /// An empty `accepted_types` list is treated as "accepts everything".
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_dnd::{DropEffectMask, DropTarget};
    /// use martensite_core::WidgetId;
    ///
    /// let target = DropTarget::new(WidgetId::from_parts(1, 1), vec!["text/plain".into()], DropEffectMask::COPY);
    /// assert!(target.accepts_type("text/plain"));
    /// assert!(!target.accepts_type("image/png"));
    /// ```
    pub fn accepts_type(&self, mime: &str) -> bool {
        if self.accepted_types.is_empty() {
            return true;
        }
        let canonical = canonicalize_mime(mime);
        self.accepted_types.contains(&canonical)
    }

    /// Returns `true` if this target accepts the given [`DropEffect`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_dnd::{DropEffect, DropEffectMask, DropTarget};
    /// use martensite_core::WidgetId;
    ///
    /// let target = DropTarget::new(WidgetId::from_parts(1, 1), vec![], DropEffectMask::COPY | DropEffectMask::LINK);
    /// assert!(target.accepts_effect(DropEffect::Copy));
    /// assert!(!target.accepts_effect(DropEffect::Move));
    /// ```
    pub fn accepts_effect(&self, effect: DropEffect) -> bool {
        self.accepted_effects.accepts(effect)
    }

    /// Returns the current [`DropTargetState`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_dnd::{DropEffectMask, DropTarget, DropTargetState};
    /// use martensite_core::WidgetId;
    ///
    /// let target = DropTarget::new(WidgetId::from_parts(1, 1), vec![], DropEffectMask::COPY);
    /// assert_eq!(target.state(), DropTargetState::Idle);
    /// ```
    pub fn state(&self) -> DropTargetState {
        self.state
    }

    /// Returns `true` if `pos` lies within this target's bounds.
    ///
    /// Returns `false` if either `pos` or the stored bounds contain a NaN or
    /// infinity, since such a point can never meaningfully be "inside" a
    /// region.
    fn contains(&self, pos: Vec2) -> bool {
        if !pos.is_finite() || !self.bounds.origin.is_finite() || !self.bounds.size.is_finite() {
            return false;
        }
        let min = self.bounds.origin;
        let max = self.bounds.origin + self.bounds.size;
        pos.x >= min.x && pos.x < max.x && pos.y >= min.y && pos.y < max.y
    }

    /// Validates a [`DndSession`] against this target's accepted types and
    /// effects, returning the resulting [`DropTargetState`].
    ///
    /// The target is considered accepted if at least one of the session's
    /// `available_types` is accepted **and** at least one accepted effect is
    /// permitted by `allowed_effects`.
    fn validate(&self, session: &DndSession, allowed_effects: DropEffectMask) -> DropTargetState {
        let type_ok = session.available_types.iter().any(|t| self.accepts_type(t));
        let effect_ok = (self.accepted_effects & allowed_effects).bits() != 0;
        if type_ok && effect_ok {
            DropTargetState::Hovered
        } else {
            DropTargetState::Rejected
        }
    }
}

/// Canonicalizes a MIME type string for case-insensitive comparison.
///
/// Lowercases the entire `type/subtype` string and trims surrounding
/// whitespace. This ensures that `"TEXT/Plain"` and `" text/plain "` are
/// treated as equivalent to `"text/plain"`, matching the case-insensitivity
/// required by [RFC 2045](https://www.rfc-editor.org/rfc/rfc2045) for MIME
/// type and subtype tokens.
fn canonicalize_mime(mime: &str) -> String {
    mime.trim().to_ascii_lowercase()
}

/// Picks a single [`DropEffect`] from a `common` mask using the built-in
/// Copy > Move > Link priority.
///
/// Returns `None` when the mask contains none of the three effects.
fn pick_effect_by_priority(common: DropEffectMask) -> Option<DropEffect> {
    if common.contains(DropEffectMask::COPY) {
        Some(DropEffect::Copy)
    } else if common.contains(DropEffectMask::MOVE) {
        Some(DropEffect::Move)
    } else if common.contains(DropEffectMask::LINK) {
        Some(DropEffect::Link)
    } else {
        None
    }
}

/// Registry of all registered [`DropTarget`]s, keyed by [`TargetId`].
///
/// Targets are stored in insertion order. [`DropTargetRegistry::find_target_at`]
/// searches in **reverse insertion order** so that later-registered (i.e.
/// higher Z-order) targets win when bounds overlap — "first match wins" in
/// top-most Z-order.
///
/// # Examples
///
/// ```
/// use martensite_dnd::{DropEffectMask, DropTarget, DropTargetRegistry};
/// use martensite_core::{Rect, WidgetId};
///
/// let mut registry = DropTargetRegistry::new();
/// let id = registry.register(
///     DropTarget::new(WidgetId::from_parts(1, 1), vec![], DropEffectMask::COPY)
///         .with_bounds(Rect::new(0.0, 0.0, 100.0, 100.0)),
/// );
/// assert!(registry.find_target_at(glam::Vec2::new(50.0, 50.0)).is_some());
/// assert!(registry.find_target_at(glam::Vec2::new(200.0, 200.0)).is_none());
/// ```
pub struct DropTargetRegistry {
    targets: HashMap<TargetId, DropTarget>,
    order: Vec<TargetId>,
    next_id: u64,
}

impl Default for DropTargetRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl DropTargetRegistry {
    /// Creates a new, empty registry.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_dnd::DropTargetRegistry;
    ///
    /// let registry = DropTargetRegistry::new();
    /// assert_eq!(registry.len(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            targets: HashMap::new(),
            order: Vec::new(),
            next_id: 0,
        }
    }

    /// Registers a new drop target and returns its [`TargetId`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_dnd::{DropEffectMask, DropTarget, DropTargetRegistry};
    /// use martensite_core::WidgetId;
    ///
    /// let mut registry = DropTargetRegistry::new();
    /// let id = registry.register(DropTarget::new(WidgetId::from_parts(1, 1), vec![], DropEffectMask::COPY));
    /// assert!(registry.get(id).is_some());
    /// ```
    pub fn register(&mut self, target: DropTarget) -> TargetId {
        let id = TargetId(self.next_id);
        self.next_id += 1;
        self.order.push(id);
        self.targets.insert(id, target);
        id
    }

    /// Unregisters the drop target with the given `id`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_dnd::{DropEffectMask, DropTarget, DropTargetRegistry};
    /// use martensite_core::WidgetId;
    ///
    /// let mut registry = DropTargetRegistry::new();
    /// let id = registry.register(DropTarget::new(WidgetId::from_parts(1, 1), vec![], DropEffectMask::COPY));
    /// registry.unregister(id);
    /// assert_eq!(registry.len(), 0);
    /// ```
    pub fn unregister(&mut self, id: TargetId) {
        if self.targets.remove(&id).is_some() {
            self.order.retain(|t| *t != id);
        }
    }

    /// Returns a shared reference to the target with the given `id`, if any.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_dnd::{DropEffectMask, DropTarget, DropTargetRegistry};
    /// use martensite_core::WidgetId;
    ///
    /// let mut registry = DropTargetRegistry::new();
    /// let id = registry.register(DropTarget::new(WidgetId::from_parts(1, 1), vec![], DropEffectMask::COPY));
    /// assert!(registry.get(id).is_some());
    /// ```
    pub fn get(&self, id: TargetId) -> Option<&DropTarget> {
        self.targets.get(&id)
    }

    /// Returns the number of registered targets.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_dnd::{DropEffectMask, DropTarget, DropTargetRegistry};
    /// use martensite_core::WidgetId;
    ///
    /// let mut registry = DropTargetRegistry::new();
    /// registry.register(DropTarget::new(WidgetId::from_parts(1, 1), vec![], DropEffectMask::COPY));
    /// assert_eq!(registry.len(), 1);
    /// ```
    pub fn len(&self) -> usize {
        self.targets.len()
    }

    /// Returns `true` if the registry contains no targets.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_dnd::DropTargetRegistry;
    ///
    /// let registry = DropTargetRegistry::new();
    /// assert!(registry.is_empty());
    /// ```
    pub fn is_empty(&self) -> bool {
        self.targets.is_empty()
    }

    /// Finds the top-most target whose bounds contain `pos`.
    ///
    /// Searches in reverse insertion order so that later-registered (higher
    /// Z-order) targets take precedence when bounds overlap.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_dnd::{DropEffectMask, DropTarget, DropTargetRegistry};
    /// use martensite_core::{Rect, WidgetId};
    ///
    /// let mut registry = DropTargetRegistry::new();
    /// registry.register(
    ///     DropTarget::new(WidgetId::from_parts(1, 1), vec![], DropEffectMask::COPY)
    ///         .with_bounds(Rect::new(0.0, 0.0, 100.0, 100.0)),
    /// );
    /// assert!(registry.find_target_at(glam::Vec2::new(50.0, 50.0)).is_some());
    /// ```
    pub fn find_target_at(&self, pos: Vec2) -> Option<&DropTarget> {
        for id in self.order.iter().rev() {
            if let Some(target) = self.targets.get(id) {
                if target.contains(pos) {
                    return Some(target);
                }
            }
        }
        None
    }

    /// Finds the [`TargetId`] of the top-most target whose bounds contain
    /// `pos`.
    fn find_id_at(&self, pos: Vec2) -> Option<TargetId> {
        for id in self.order.iter().rev() {
            if let Some(target) = self.targets.get(id) {
                if target.contains(pos) {
                    return Some(*id);
                }
            }
        }
        None
    }

    /// Called when a drag enters the target with the given `id`.
    ///
    /// Validates the session's advertised types against the target's accepted
    /// types and effects, sets the target's state accordingly, and returns
    /// that state. Returns [`DropTargetState::Rejected`] if the target does
    /// not exist.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_dnd::{DndSession, DropEffectMask, DropTarget, DropTargetRegistry, DropTargetState};
    /// use martensite_core::WidgetId;
    /// use std::sync::Arc;
    ///
    /// let mut registry = DropTargetRegistry::new();
    /// let id = registry.register(
    ///     DropTarget::new(WidgetId::from_parts(1, 1), vec!["text/plain".into()], DropEffectMask::COPY),
    /// );
    /// let session = DndSession::new(Arc::new("hi"), vec!["text/plain".into()], None);
    /// assert_eq!(registry.enter_target(id, &session), DropTargetState::Hovered);
    /// ```
    pub fn enter_target(&mut self, id: TargetId, session: &DndSession) -> DropTargetState {
        let allowed = DropEffectMask::COPY | DropEffectMask::MOVE | DropEffectMask::LINK;
        self.enter_target_with_effects(id, session, allowed)
    }

    /// Like [`DropTargetRegistry::enter_target`] but restricts the negotiated
    /// effect to those in `allowed_effects`.
    pub fn enter_target_with_effects(
        &mut self,
        id: TargetId,
        session: &DndSession,
        allowed_effects: DropEffectMask,
    ) -> DropTargetState {
        let state = match self.targets.get(&id) {
            Some(target) => target.validate(session, allowed_effects),
            None => DropTargetState::Rejected,
        };
        if let Some(target) = self.targets.get_mut(&id) {
            target.state = state;
        }
        state
    }

    /// Called when a drag leaves the target with the given `id`.
    ///
    /// Resets the target's state to [`DropTargetState::Idle`]. No-op if the
    /// target does not exist.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_dnd::{DndSession, DropEffectMask, DropTarget, DropTargetRegistry, DropTargetState};
    /// use martensite_core::WidgetId;
    /// use std::sync::Arc;
    ///
    /// let mut registry = DropTargetRegistry::new();
    /// let id = registry.register(
    ///     DropTarget::new(WidgetId::from_parts(1, 1), vec!["text/plain".into()], DropEffectMask::COPY),
    /// );
    /// let session = DndSession::new(Arc::new("hi"), vec!["text/plain".into()], None);
    /// registry.enter_target(id, &session);
    /// registry.leave_target(id);
    /// assert_eq!(registry.get(id).unwrap().state(), DropTargetState::Idle);
    /// ```
    pub fn leave_target(&mut self, id: TargetId) {
        if let Some(target) = self.targets.get_mut(&id) {
            target.state = DropTargetState::Idle;
        }
    }

    /// Negotiates and commits a drop onto the target with the given `id`.
    ///
    /// On success, marks the session as completed with the negotiated
    /// [`DropEffect`] and returns `Some(effect)`. Returns `None` if the
    /// target does not exist or rejects the session (type/effect mismatch).
    /// The target's state is reset to [`DropTargetState::Idle`] afterwards.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_dnd::{DndSession, DropEffect, DropEffectMask, DropTarget, DropTargetRegistry};
    /// use martensite_core::WidgetId;
    /// use std::sync::Arc;
    ///
    /// let mut registry = DropTargetRegistry::new();
    /// let id = registry.register(
    ///     DropTarget::new(WidgetId::from_parts(1, 1), vec!["text/plain".into()], DropEffectMask::COPY),
    /// );
    /// let mut session = DndSession::new(Arc::new("hi"), vec!["text/plain".into()], None);
    /// assert_eq!(registry.drop_on_target(id, &mut session), Some(DropEffect::Copy));
    /// ```
    pub fn drop_on_target(&mut self, id: TargetId, session: &mut DndSession) -> Option<DropEffect> {
        self.drop_on_target_with_effects(
            id,
            session,
            DropEffectMask::COPY | DropEffectMask::MOVE | DropEffectMask::LINK,
        )
    }

    /// Like [`DropTargetRegistry::drop_on_target`] but restricts the
    /// negotiated effect to those in `allowed_effects`.
    ///
    /// On every rejection path (missing target, expired session, no common
    /// effect, or type/effect validation failure) the target's state is reset
    /// to [`DropTargetState::Idle`] before returning `None`, so a failed drop
    /// never leaves a target stuck in [`DropTargetState::Hovered`] or
    /// [`DropTargetState::Rejected`].
    pub fn drop_on_target_with_effects(
        &mut self,
        id: TargetId,
        session: &mut DndSession,
        allowed_effects: DropEffectMask,
    ) -> Option<DropEffect> {
        // A session that has already reached a terminal status (completed or
        // cancelled) cannot be dropped again. Reset the target and bail out.
        if session.is_expired() {
            if let Some(target) = self.targets.get_mut(&id) {
                target.state = DropTargetState::Idle;
            }
            return None;
        }
        let negotiated = {
            let target = self.targets.get(&id)?;
            let common = target.accepted_effects & allowed_effects;
            // If the target advertises a preferred effect that is among the
            // mutually-accepted effects, honor it. Otherwise fall back to the
            // built-in Copy > Move > Link priority for deterministic
            // negotiation.
            let effect = if let Some(preferred) = target.preferred_effect {
                if preferred == DropEffect::None {
                    // `None` means "no preference"; fall back to priority.
                    match pick_effect_by_priority(common) {
                        Some(e) => e,
                        None => {
                            if let Some(t) = self.targets.get_mut(&id) {
                                t.state = DropTargetState::Idle;
                            }
                            return None;
                        }
                    }
                } else {
                    let preferred_mask = DropEffectMask::from_effect(preferred);
                    if common.contains(preferred_mask) {
                        preferred
                    } else {
                        match pick_effect_by_priority(common) {
                            Some(e) => e,
                            None => {
                                if let Some(t) = self.targets.get_mut(&id) {
                                    t.state = DropTargetState::Idle;
                                }
                                return None;
                            }
                        }
                    }
                }
            } else {
                match pick_effect_by_priority(common) {
                    Some(e) => e,
                    None => {
                        if let Some(t) = self.targets.get_mut(&id) {
                            t.state = DropTargetState::Idle;
                        }
                        return None;
                    }
                }
            };
            if target.validate(session, allowed_effects) == DropTargetState::Rejected {
                if let Some(t) = self.targets.get_mut(&id) {
                    t.state = DropTargetState::Idle;
                }
                return None;
            }
            effect
        };
        session.complete(negotiated);
        if let Some(target) = self.targets.get_mut(&id) {
            target.state = DropTargetState::Idle;
        }
        Some(negotiated)
    }

    /// Finds the top-most target at `pos` and enters it, returning its
    /// [`TargetId`] and resulting [`DropTargetState`].
    ///
    /// This is a convenience combining [`DropTargetRegistry::find_target_at`]
    /// and [`DropTargetRegistry::enter_target`]. Returns `None` if no target
    /// contains `pos`.
    pub fn enter_at(
        &mut self,
        pos: Vec2,
        session: &DndSession,
    ) -> Option<(TargetId, DropTargetState)> {
        let id = self.find_id_at(pos)?;
        let state = self.enter_target(id, session);
        Some((id, state))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn target(types: &[&str], effects: DropEffectMask, bounds: Rect) -> DropTarget {
        DropTarget::new(
            WidgetId::from_parts(1, 1),
            types.iter().map(|s| s.to_string()).collect(),
            effects,
        )
        .with_bounds(bounds)
    }

    #[test]
    fn mask_accepts_effects() {
        let mask = DropEffectMask::COPY | DropEffectMask::MOVE;
        assert!(mask.accepts(DropEffect::Copy));
        assert!(mask.accepts(DropEffect::Move));
        assert!(!mask.accepts(DropEffect::Link));
        assert!(!mask.accepts(DropEffect::None));
    }

    #[test]
    fn mask_from_effect_roundtrips() {
        assert_eq!(
            DropEffectMask::from_effect(DropEffect::Copy),
            DropEffectMask::COPY
        );
        assert_eq!(
            DropEffectMask::from_effect(DropEffect::Move),
            DropEffectMask::MOVE
        );
        assert_eq!(
            DropEffectMask::from_effect(DropEffect::Link),
            DropEffectMask::LINK
        );
        assert_eq!(
            DropEffectMask::from_effect(DropEffect::None),
            DropEffectMask::empty()
        );
    }

    #[test]
    fn accepts_type_matches() {
        let t = target(
            &["text/plain", "text/html"],
            DropEffectMask::COPY,
            Rect::default(),
        );
        assert!(t.accepts_type("text/plain"));
        assert!(t.accepts_type("text/html"));
        assert!(!t.accepts_type("image/png"));
    }

    #[test]
    fn accepts_type_empty_accepts_all() {
        let t = target(&[], DropEffectMask::COPY, Rect::default());
        assert!(t.accepts_type("anything"));
        assert!(t.accepts_type("text/plain"));
    }

    #[test]
    fn accepts_effect_matches() {
        let t = target(
            &[],
            DropEffectMask::COPY | DropEffectMask::LINK,
            Rect::default(),
        );
        assert!(t.accepts_effect(DropEffect::Copy));
        assert!(t.accepts_effect(DropEffect::Link));
        assert!(!t.accepts_effect(DropEffect::Move));
    }

    #[test]
    fn registry_register_and_get() {
        let mut reg = DropTargetRegistry::new();
        let id = reg.register(target(
            &["text/plain"],
            DropEffectMask::COPY,
            Rect::default(),
        ));
        assert!(reg.get(id).is_some());
        assert_eq!(reg.len(), 1);
    }

    #[test]
    fn registry_unregister() {
        let mut reg = DropTargetRegistry::new();
        let id = reg.register(target(&[], DropEffectMask::COPY, Rect::default()));
        reg.unregister(id);
        assert!(reg.get(id).is_none());
        assert_eq!(reg.len(), 0);
        assert!(reg.is_empty());
    }

    #[test]
    fn registry_unregister_missing_is_noop() {
        let mut reg = DropTargetRegistry::new();
        reg.unregister(TargetId(999));
        assert!(reg.is_empty());
    }

    #[test]
    fn find_target_at_inside() {
        let mut reg = DropTargetRegistry::new();
        reg.register(target(
            &[],
            DropEffectMask::COPY,
            Rect::new(0.0, 0.0, 100.0, 100.0),
        ));
        assert!(reg.find_target_at(Vec2::new(50.0, 50.0)).is_some());
        assert!(reg.find_target_at(Vec2::new(0.0, 0.0)).is_some());
    }

    #[test]
    fn find_target_at_outside() {
        let mut reg = DropTargetRegistry::new();
        reg.register(target(
            &[],
            DropEffectMask::COPY,
            Rect::new(0.0, 0.0, 100.0, 100.0),
        ));
        assert!(reg.find_target_at(Vec2::new(100.0, 100.0)).is_none());
        assert!(reg.find_target_at(Vec2::new(-1.0, 0.0)).is_none());
    }

    #[test]
    fn overlapping_bounds_first_registered_wins_top_z() {
        // Later-registered target represents higher Z-order and should win.
        let mut reg = DropTargetRegistry::new();
        let bottom = reg.register(target(
            &[],
            DropEffectMask::COPY,
            Rect::new(0.0, 0.0, 100.0, 100.0),
        ));
        let top = reg.register(target(
            &[],
            DropEffectMask::MOVE,
            Rect::new(0.0, 0.0, 100.0, 100.0),
        ));
        let found = reg.find_target_at(Vec2::new(50.0, 50.0)).unwrap();
        assert_eq!(found.accepted_effects, DropEffectMask::MOVE);
        assert_ne!(
            found.accepted_effects,
            reg.get(bottom).unwrap().accepted_effects
        );
        assert_eq!(reg.get(top).unwrap().accepted_effects, DropEffectMask::MOVE);
    }

    #[test]
    fn enter_target_accepts_matching_session() {
        let mut reg = DropTargetRegistry::new();
        let id = reg.register(target(
            &["text/plain"],
            DropEffectMask::COPY,
            Rect::default(),
        ));
        let session = DndSession::new(Arc::new("hi"), vec!["text/plain".into()], None);
        assert_eq!(reg.enter_target(id, &session), DropTargetState::Hovered);
        assert_eq!(reg.get(id).unwrap().state(), DropTargetState::Hovered);
    }

    #[test]
    fn enter_target_rejects_type_mismatch() {
        let mut reg = DropTargetRegistry::new();
        let id = reg.register(target(
            &["text/plain"],
            DropEffectMask::COPY,
            Rect::default(),
        ));
        let session = DndSession::new(Arc::new("hi"), vec!["image/png".into()], None);
        assert_eq!(reg.enter_target(id, &session), DropTargetState::Rejected);
        assert_eq!(reg.get(id).unwrap().state(), DropTargetState::Rejected);
    }

    #[test]
    fn enter_target_rejects_effect_mismatch() {
        let mut reg = DropTargetRegistry::new();
        let id = reg.register(target(
            &["text/plain"],
            DropEffectMask::COPY,
            Rect::default(),
        ));
        let session = DndSession::new(Arc::new("hi"), vec!["text/plain".into()], None);
        assert_eq!(
            reg.enter_target_with_effects(id, &session, DropEffectMask::MOVE),
            DropTargetState::Rejected
        );
    }

    #[test]
    fn enter_target_missing_returns_rejected() {
        let mut reg = DropTargetRegistry::new();
        let session = DndSession::new(Arc::new("hi"), vec![], None);
        assert_eq!(
            reg.enter_target(TargetId(999), &session),
            DropTargetState::Rejected
        );
    }

    #[test]
    fn leave_target_resets_state() {
        let mut reg = DropTargetRegistry::new();
        let id = reg.register(target(
            &["text/plain"],
            DropEffectMask::COPY,
            Rect::default(),
        ));
        let session = DndSession::new(Arc::new("hi"), vec!["text/plain".into()], None);
        reg.enter_target(id, &session);
        reg.leave_target(id);
        assert_eq!(reg.get(id).unwrap().state(), DropTargetState::Idle);
    }

    #[test]
    fn leave_target_missing_is_noop() {
        let mut reg = DropTargetRegistry::new();
        reg.leave_target(TargetId(999));
    }

    #[test]
    fn drop_on_target_completes_session() {
        let mut reg = DropTargetRegistry::new();
        let id = reg.register(target(
            &["text/plain"],
            DropEffectMask::COPY,
            Rect::default(),
        ));
        let mut session = DndSession::new(Arc::new("hi"), vec!["text/plain".into()], None);
        let effect = reg.drop_on_target(id, &mut session);
        assert_eq!(effect, Some(DropEffect::Copy));
        assert!(session.is_expired());
        assert_eq!(reg.get(id).unwrap().state(), DropTargetState::Idle);
    }

    #[test]
    fn drop_on_target_rejects_type_mismatch() {
        let mut reg = DropTargetRegistry::new();
        let id = reg.register(target(
            &["text/plain"],
            DropEffectMask::COPY,
            Rect::default(),
        ));
        let mut session = DndSession::new(Arc::new("hi"), vec!["image/png".into()], None);
        assert_eq!(reg.drop_on_target(id, &mut session), None);
        assert!(!session.is_expired());
    }

    #[test]
    fn drop_on_target_rejects_effect_mismatch() {
        let mut reg = DropTargetRegistry::new();
        let id = reg.register(target(
            &["text/plain"],
            DropEffectMask::COPY,
            Rect::default(),
        ));
        let mut session = DndSession::new(Arc::new("hi"), vec!["text/plain".into()], None);
        assert_eq!(
            reg.drop_on_target_with_effects(id, &mut session, DropEffectMask::MOVE),
            None
        );
        assert!(!session.is_expired());
    }

    #[test]
    fn drop_on_target_missing_returns_none() {
        let mut reg = DropTargetRegistry::new();
        let mut session = DndSession::new(Arc::new("hi"), vec![], None);
        assert_eq!(reg.drop_on_target(TargetId(999), &mut session), None);
    }

    #[test]
    fn drop_negotiates_preferred_effect_order() {
        let mut reg = DropTargetRegistry::new();
        let id = reg.register(target(
            &["text/plain"],
            DropEffectMask::COPY | DropEffectMask::MOVE,
            Rect::default(),
        ));
        let mut session = DndSession::new(Arc::new("hi"), vec!["text/plain".into()], None);
        // Copy is preferred over Move.
        assert_eq!(reg.drop_on_target(id, &mut session), Some(DropEffect::Copy));
    }

    #[test]
    fn drop_negotiates_move_when_copy_disallowed() {
        let mut reg = DropTargetRegistry::new();
        let id = reg.register(target(
            &["text/plain"],
            DropEffectMask::COPY | DropEffectMask::MOVE,
            Rect::default(),
        ));
        let mut session = DndSession::new(Arc::new("hi"), vec!["text/plain".into()], None);
        assert_eq!(
            reg.drop_on_target_with_effects(id, &mut session, DropEffectMask::MOVE),
            Some(DropEffect::Move)
        );
    }

    #[test]
    fn enter_at_combines_find_and_enter() {
        let mut reg = DropTargetRegistry::new();
        reg.register(target(
            &["text/plain"],
            DropEffectMask::COPY,
            Rect::new(0.0, 0.0, 100.0, 100.0),
        ));
        let session = DndSession::new(Arc::new("hi"), vec!["text/plain".into()], None);
        let (id, state) = reg.enter_at(Vec2::new(50.0, 50.0), &session).unwrap();
        assert_eq!(state, DropTargetState::Hovered);
        assert_eq!(reg.get(id).unwrap().state(), DropTargetState::Hovered);
    }

    #[test]
    fn enter_at_misses_return_none() {
        let mut reg = DropTargetRegistry::new();
        reg.register(target(
            &[],
            DropEffectMask::COPY,
            Rect::new(0.0, 0.0, 100.0, 100.0),
        ));
        let session = DndSession::new(Arc::new("hi"), vec![], None);
        assert!(reg.enter_at(Vec2::new(200.0, 200.0), &session).is_none());
    }

    #[test]
    fn full_lifecycle_enter_leave_drop() {
        let mut reg = DropTargetRegistry::new();
        let id = reg.register(target(
            &["text/plain"],
            DropEffectMask::COPY,
            Rect::new(0.0, 0.0, 100.0, 100.0),
        ));
        let mut session = DndSession::new(Arc::new("hi"), vec!["text/plain".into()], None);

        // Enter
        assert_eq!(reg.enter_target(id, &session), DropTargetState::Hovered);
        // Leave without dropping
        reg.leave_target(id);
        assert_eq!(reg.get(id).unwrap().state(), DropTargetState::Idle);
        assert!(!session.is_expired());

        // Re-enter and drop
        reg.enter_target(id, &session);
        assert_eq!(reg.drop_on_target(id, &mut session), Some(DropEffect::Copy));
        assert!(session.is_expired());
    }

    #[test]
    fn accepts_type_is_case_insensitive() {
        let t = DropTarget::new(
            WidgetId::from_parts(1, 1),
            vec!["text/plain".into(), "TEXT/HTML".into()],
            DropEffectMask::COPY,
        );
        // Stored types are canonicalized to lowercase.
        assert_eq!(t.accepted_types, vec!["text/plain", "text/html"]);
        // Queries are case-insensitive.
        assert!(t.accepts_type("TEXT/PLAIN"));
        assert!(t.accepts_type("text/html"));
        assert!(t.accepts_type("Text/Plain"));
        assert!(!t.accepts_type("image/png"));
    }

    #[test]
    fn accepts_type_trims_whitespace() {
        let t = DropTarget::new(
            WidgetId::from_parts(1, 1),
            vec!["  text/plain  ".into()],
            DropEffectMask::COPY,
        );
        assert_eq!(t.accepted_types, vec!["text/plain"]);
        assert!(t.accepts_type(" text/plain "));
        assert!(t.accepts_type("text/plain"));
    }

    #[test]
    fn enter_target_matches_case_insensitive_session_types() {
        let mut reg = DropTargetRegistry::new();
        let id = reg.register(target(
            &["text/plain"],
            DropEffectMask::COPY,
            Rect::default(),
        ));
        // Session advertises an uppercased MIME; the target should still match.
        let session = DndSession::new(Arc::new("hi"), vec!["TEXT/PLAIN".into()], None);
        assert_eq!(reg.enter_target(id, &session), DropTargetState::Hovered);
        assert_eq!(reg.get(id).unwrap().state(), DropTargetState::Hovered);
    }

    #[test]
    fn with_preferred_effect_honored_when_available() {
        let mut reg = DropTargetRegistry::new();
        let id = reg.register(
            DropTarget::new(
                WidgetId::from_parts(1, 1),
                vec!["text/plain".into()],
                DropEffectMask::COPY | DropEffectMask::MOVE,
            )
            .with_preferred_effect(DropEffect::Move),
        );
        let mut session = DndSession::new(Arc::new("hi"), vec!["text/plain".into()], None);
        // Both Copy and Move are acceptable; preferred Move wins.
        assert_eq!(reg.drop_on_target(id, &mut session), Some(DropEffect::Move));
    }

    #[test]
    fn with_preferred_effect_falls_back_when_unavailable() {
        let mut reg = DropTargetRegistry::new();
        let id = reg.register(
            DropTarget::new(
                WidgetId::from_parts(1, 1),
                vec!["text/plain".into()],
                DropEffectMask::COPY | DropEffectMask::MOVE,
            )
            .with_preferred_effect(DropEffect::Link),
        );
        let mut session = DndSession::new(Arc::new("hi"), vec!["text/plain".into()], None);
        // Link is preferred but not in the common mask; falls back to Copy.
        assert_eq!(reg.drop_on_target(id, &mut session), Some(DropEffect::Copy));
    }

    #[test]
    fn with_preferred_effect_none_falls_back_to_priority() {
        // `DropEffect::None` must be treated as "no preference" — an empty
        // bitflags mask is contained by every mask, so without this guard the
        // negotiation would incorrectly select `None` even when real effects
        // are available.
        let mut reg = DropTargetRegistry::new();
        let id = reg.register(
            DropTarget::new(
                WidgetId::from_parts(1, 1),
                vec!["text/plain".into()],
                DropEffectMask::COPY | DropEffectMask::MOVE,
            )
            .with_preferred_effect(DropEffect::None),
        );
        let mut session = DndSession::new(Arc::new("hi"), vec!["text/plain".into()], None);
        // None is "preferred" but must not be selected; falls back to Copy.
        assert_eq!(reg.drop_on_target(id, &mut session), Some(DropEffect::Copy));
    }

    #[test]
    fn drop_on_expired_session_returns_none_and_resets_state() {
        let mut reg = DropTargetRegistry::new();
        let id = reg.register(target(
            &["text/plain"],
            DropEffectMask::COPY,
            Rect::default(),
        ));
        let mut session = DndSession::new(Arc::new("hi"), vec!["text/plain".into()], None);
        // Mark the session terminal before dropping.
        session.complete(DropEffect::Copy);
        reg.enter_target(id, &session);
        assert_eq!(reg.get(id).unwrap().state(), DropTargetState::Hovered);
        assert_eq!(reg.drop_on_target(id, &mut session), None);
        // State must be reset to Idle even on the expired-session path.
        assert_eq!(reg.get(id).unwrap().state(), DropTargetState::Idle);
    }

    #[test]
    fn drop_rejected_resets_state_to_idle() {
        let mut reg = DropTargetRegistry::new();
        let id = reg.register(target(
            &["text/plain"],
            DropEffectMask::COPY,
            Rect::default(),
        ));
        let mut session = DndSession::new(Arc::new("hi"), vec!["image/png".into()], None);
        reg.enter_target(id, &session);
        assert_eq!(reg.get(id).unwrap().state(), DropTargetState::Rejected);
        // Type mismatch rejects the drop; state must still reset to Idle.
        assert_eq!(reg.drop_on_target(id, &mut session), None);
        assert_eq!(reg.get(id).unwrap().state(), DropTargetState::Idle);
        assert!(!session.is_expired());
    }

    #[test]
    fn drop_effect_mismatch_resets_state_to_idle() {
        let mut reg = DropTargetRegistry::new();
        let id = reg.register(target(
            &["text/plain"],
            DropEffectMask::COPY,
            Rect::default(),
        ));
        let mut session = DndSession::new(Arc::new("hi"), vec!["text/plain".into()], None);
        reg.enter_target(id, &session);
        assert_eq!(
            reg.drop_on_target_with_effects(id, &mut session, DropEffectMask::MOVE),
            None
        );
        assert_eq!(reg.get(id).unwrap().state(), DropTargetState::Idle);
        assert!(!session.is_expired());
    }

    #[test]
    fn drop_no_common_effect_resets_state_to_idle() {
        let mut reg = DropTargetRegistry::new();
        let id = reg.register(target(
            &["text/plain"],
            DropEffectMask::COPY,
            Rect::default(),
        ));
        let mut session = DndSession::new(Arc::new("hi"), vec!["text/plain".into()], None);
        // Target accepts only Copy; allowed effects exclude Copy entirely.
        assert_eq!(
            reg.drop_on_target_with_effects(id, &mut session, DropEffectMask::LINK),
            None
        );
        assert_eq!(reg.get(id).unwrap().state(), DropTargetState::Idle);
        assert!(!session.is_expired());
    }

    #[test]
    #[should_panic(expected = "DropTarget bounds must be finite")]
    fn with_bounds_panics_on_nan() {
        let _ = DropTarget::new(WidgetId::from_parts(1, 1), vec![], DropEffectMask::COPY)
            .with_bounds(Rect::new(f32::NAN, 0.0, 100.0, 100.0));
    }

    #[test]
    #[should_panic(expected = "DropTarget bounds must be finite")]
    fn with_bounds_panics_on_infinity() {
        let _ = DropTarget::new(WidgetId::from_parts(1, 1), vec![], DropEffectMask::COPY)
            .with_bounds(Rect::new(0.0, 0.0, f32::INFINITY, 100.0));
    }

    #[test]
    fn find_target_at_nan_point_returns_none() {
        let mut reg = DropTargetRegistry::new();
        reg.register(target(
            &[],
            DropEffectMask::COPY,
            Rect::new(0.0, 0.0, 100.0, 100.0),
        ));
        assert!(reg.find_target_at(Vec2::new(f32::NAN, 50.0)).is_none());
        assert!(reg.find_target_at(Vec2::new(50.0, f32::INFINITY)).is_none());
    }
}
