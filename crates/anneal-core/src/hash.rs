/// Stable FNV-1a 64-bit hash used for deterministic local identifiers.
pub fn fnv1a_64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::fnv1a_64;

    #[test]
    fn hash_matches_known_vectors() {
        assert_eq!(fnv1a_64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a_64(b"a"), 0xaf63_dc4c_8601_ec8c);
    }

    #[test]
    fn hash_changes_when_payload_changes() {
        assert_ne!(fnv1a_64(b"anneal"), fnv1a_64(b"anneal!"));
    }
}
