/// Stable FNV-1a 64-bit hash used for deterministic local identifiers.
pub fn fnv1a_64(bytes: &[u8]) -> u64 {
    Fnv1a64::new().write(bytes).finish()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fnv1a64 {
    state: u64,
}

impl Fnv1a64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0100_0000_01b3;

    pub fn new() -> Self {
        Self {
            state: Self::OFFSET,
        }
    }

    pub fn write(mut self, bytes: &[u8]) -> Self {
        for byte in bytes {
            self.state ^= u64::from(*byte);
            self.state = self.state.wrapping_mul(Self::PRIME);
        }
        self
    }

    pub fn finish(self) -> u64 {
        self.state
    }
}

impl Default for Fnv1a64 {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{Fnv1a64, fnv1a_64};

    #[test]
    fn hash_matches_known_vectors() {
        assert_eq!(fnv1a_64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a_64(b"a"), 0xaf63_dc4c_8601_ec8c);
    }

    #[test]
    fn hash_changes_when_payload_changes() {
        assert_ne!(fnv1a_64(b"anneal"), fnv1a_64(b"anneal!"));
    }

    #[test]
    fn incremental_hash_matches_single_write() {
        let incremental = Fnv1a64::new().write(b"ann").write(b"eal").finish();
        assert_eq!(incremental, fnv1a_64(b"anneal"));
    }
}
