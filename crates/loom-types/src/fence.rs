use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Fence {
    authority: u32,
    epoch: u32,
    sequence: u64,
}

impl Fence {
    pub const EMBEDDED_AUTHORITY: u32 = 0;
    pub const EMBEDDED_EPOCH: u32 = 0;

    pub const fn new(authority: u32, epoch: u32, sequence: u64) -> Self {
        Self {
            authority,
            epoch,
            sequence,
        }
    }

    pub const fn embedded(sequence: u64) -> Self {
        Self::new(Self::EMBEDDED_AUTHORITY, Self::EMBEDDED_EPOCH, sequence)
    }

    pub const fn authority(self) -> u32 {
        self.authority
    }

    pub const fn epoch(self) -> u32 {
        self.epoch
    }

    pub const fn sequence(self) -> u64 {
        self.sequence
    }

    pub const fn is_embedded(self) -> bool {
        self.authority == Self::EMBEDDED_AUTHORITY && self.epoch == Self::EMBEDDED_EPOCH
    }

    pub const fn embedded_sequence(self) -> Option<u64> {
        if self.is_embedded() {
            Some(self.sequence)
        } else {
            None
        }
    }

    pub const fn to_u128(self) -> u128 {
        ((self.authority as u128) << 96) | ((self.epoch as u128) << 64) | self.sequence as u128
    }

    pub const fn from_u128(value: u128) -> Self {
        Self::new((value >> 96) as u32, (value >> 64) as u32, value as u64)
    }

    pub const fn to_limbs(self) -> (u64, u64) {
        (self.to_u128() as u64, (self.to_u128() >> 64) as u64)
    }

    pub const fn from_limbs(low: u64, high: u64) -> Self {
        Self::from_u128(((high as u128) << 64) | low as u128)
    }
}

impl fmt::Display for Fence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}:{}", self.authority, self.epoch, self.sequence)
    }
}

#[cfg(test)]
mod tests {
    use super::Fence;

    #[test]
    fn canonical_packing_round_trips() {
        let fence = Fence::new(7, 3, 42);
        assert_eq!(fence.to_u128(), (7u128 << 96) | (3u128 << 64) | 42);
        assert_eq!(Fence::from_u128(fence.to_u128()), fence);
        assert_eq!(Fence::from_limbs(42, (7u64 << 32) | 3), fence);
    }

    #[test]
    fn embedded_fence_uses_the_low_sequence_bits() {
        let fence = Fence::embedded(42);
        assert_eq!(fence.to_limbs(), (42, 0));
        assert_eq!(fence.embedded_sequence(), Some(42));
        assert_eq!(Fence::new(1, 0, 42).embedded_sequence(), None);
    }
}
