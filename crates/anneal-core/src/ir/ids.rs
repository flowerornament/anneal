//! Typed numeric identifiers for the planned relational runtime.

macro_rules! index_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub(crate) struct $name(u32);

        $(#[$meta])*
        impl $name {
            pub(crate) fn from_index(index: usize) -> Self {
                let raw = u32::try_from(index)
                    .expect(concat!(stringify!($name), " index exceeds u32 range"));
                Self(raw)
            }

            pub(crate) const fn index(self) -> usize {
                self.0 as usize
            }
        }
    };
}

index_id!(SymbolId);
// Reserved for the Plan/IR middle-end, where variables become typed slots.
index_id!(
    #[allow(dead_code)]
    VarId
);
index_id!(RelationId);
index_id!(FieldId);
// Reserved for the true slot-frame evaluator planned after query planning lands.
index_id!(
    #[allow(dead_code)]
    SlotId
);
index_id!(RowId);
// Reserved for eval-scoped aggregate lists once Plan/IR owns list lifetimes.
index_id!(
    #[allow(dead_code)]
    ListId
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_four_bytes() {
        assert_eq!(std::mem::size_of::<SymbolId>(), 4);
        assert_eq!(std::mem::size_of::<VarId>(), 4);
        assert_eq!(std::mem::size_of::<RelationId>(), 4);
        assert_eq!(std::mem::size_of::<FieldId>(), 4);
        assert_eq!(std::mem::size_of::<SlotId>(), 4);
        assert_eq!(std::mem::size_of::<RowId>(), 4);
        assert_eq!(std::mem::size_of::<ListId>(), 4);
    }
}
