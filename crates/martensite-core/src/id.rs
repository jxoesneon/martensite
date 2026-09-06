#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct WidgetId {
    pub slot_idx: u32,
    pub generation: u32,
}

impl WidgetId {
    #[inline(always)]
    pub const fn new(slot_idx: u32, generation: u32) -> Self {
        Self { slot_idx, generation }
    }

    #[inline(always)]
    pub fn to_u64(self) -> u64 {
        ((self.generation as u64) << 32) | (self.slot_idx as u64)
    }

    #[inline(always)]
    pub fn from_u64(val: u64) -> Self {
        Self {
            slot_idx: val as u32,
            generation: (val >> 32) as u32,
        }
    }
}
