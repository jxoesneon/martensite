use std::num::NonZeroU64;

/// A 64-bit copyable generational handle to a widget in the arena.
/// Guaranteed 8-byte layout with niche optimization (Option<WidgetId> is 8 bytes).
#[repr(transparent)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct WidgetId(NonZeroU64);

impl WidgetId {
    /// Create a new WidgetId from slot index and generation.
    /// Generation 0 is automatically adjusted to 1 to guarantee non-zero invariant.
    #[inline(always)]
    pub const fn new(slot_idx: u32, generation: u32) -> Self {
        let gen = if generation == 0 { 1 } else { generation };
        let val = ((gen as u64) << 32) | (slot_idx as u64);
        match NonZeroU64::new(val) {
            Some(nz) => Self(nz),
            None => panic!("WidgetId invariant violated: value cannot be zero"),
        }
    }


    /// Retrieve the dense/sparse slot index.
    #[inline(always)]
    pub const fn slot_idx(self) -> u32 {
        (self.0.get() & 0xFFFF_FFFF) as u32
    }

    /// Retrieve the generational counter.
    #[inline(always)]
    pub const fn generation(self) -> u32 {
        (self.0.get() >> 32) as u32
    }

    /// Convert to a raw 64-bit integer.
    #[inline(always)]
    pub const fn to_u64(self) -> u64 {
        self.0.get()
    }

    /// Construct from a raw 64-bit integer, returning None if 0.
    #[inline(always)]
    pub fn from_u64(val: u64) -> Option<Self> {
        NonZeroU64::new(val).map(Self)
    }

    /// Convert to little-endian byte array for wire/shader/storage serialization.
    #[inline(always)]
    pub const fn to_le_bytes(self) -> [u8; 8] {
        self.0.get().to_le_bytes()
    }

    /// Construct from little-endian byte array, returning None if 0.
    #[inline(always)]
    pub fn from_le_bytes(bytes: [u8; 8]) -> Option<Self> {
        Self::from_u64(u64::from_le_bytes(bytes))
    }
}


