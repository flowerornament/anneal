//! Typed numeric identifiers for the planned relational runtime.
#![allow(dead_code)]

macro_rules! index_id {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub(crate) struct $name(u32);

        impl $name {
            pub(crate) const fn from_raw(raw: u32) -> Self {
                Self(raw)
            }

            pub(crate) fn from_index(index: usize) -> Self {
                let raw = u32::try_from(index)
                    .expect(concat!(stringify!($name), " index exceeds u32 range"));
                Self(raw)
            }

            pub(crate) const fn raw(self) -> u32 {
                self.0
            }

            pub(crate) const fn index(self) -> usize {
                self.0 as usize
            }
        }
    };
}

index_id!(SymbolId);
index_id!(VarId);
index_id!(RelationId);
index_id!(FieldId);
index_id!(SlotId);
index_id!(RowId);
index_id!(ListId);

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
