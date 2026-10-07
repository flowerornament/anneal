//! Per-session symbol interner for physical runtime strings.

use std::collections::BTreeMap;
use std::sync::Arc;

use super::ids::SymbolId;

#[derive(Clone, Debug, Default)]
pub(crate) struct Interner {
    by_text: BTreeMap<Arc<str>, SymbolId>,
    texts: Vec<Arc<str>>,
}

impl Interner {
    pub(crate) fn intern(&mut self, text: impl AsRef<str>) -> SymbolId {
        let text = text.as_ref();
        if let Some(symbol) = self.by_text.get(text) {
            return *symbol;
        }

        let symbol = SymbolId::from_index(self.texts.len());
        let stored: Arc<str> = text.into();
        self.texts.push(Arc::clone(&stored));
        self.by_text.insert(stored, symbol);
        symbol
    }

    pub(crate) fn lookup(&self, text: &str) -> Option<SymbolId> {
        self.by_text.get(text).copied()
    }

    pub(crate) fn resolve(&self, symbol: SymbolId) -> Option<&str> {
        self.texts.get(symbol.index()).map(AsRef::as_ref)
    }

    pub(crate) fn len(&self) -> usize {
        self.texts.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interner_reuses_symbols_and_resolves_text() {
        let mut interner = Interner::default();

        let first = interner.intern("stable");
        let second = interner.intern("stable");
        let third = interner.intern("draft");

        assert_eq!(first, second);
        assert_ne!(first, third);
        assert_eq!(interner.resolve(first), Some("stable"));
        assert_eq!(interner.resolve(third), Some("draft"));
        assert_eq!(interner.len(), 2);
    }

    #[test]
    fn interner_shares_text_between_lookup_and_resolution() {
        let mut interner = Interner::default();
        let symbol = interner.intern("one immutable allocation");
        let (key, stored_symbol) = interner
            .by_text
            .get_key_value("one immutable allocation")
            .expect("interned text has a lookup entry");

        assert_eq!(*stored_symbol, symbol);
        assert!(std::ptr::eq(
            key.as_ref(),
            interner.resolve(symbol).expect("interned symbol resolves")
        ));
    }

    #[test]
    fn interner_preserves_exact_text_and_insertion_order() {
        let mut interner = Interner::default();
        let texts = ["", "a\0b", "a", "é", "e\u{301}"];
        for (index, text) in texts.iter().enumerate() {
            let symbol = interner.intern(text);
            assert_eq!(symbol.index(), index);
            assert_eq!(interner.lookup(text), Some(symbol));
            assert_eq!(interner.resolve(symbol), Some(*text));
            assert_eq!(interner.intern(text), symbol);
        }
        assert_eq!(interner.lookup("missing"), None);
        assert_eq!(interner.len(), texts.len());
    }

    #[test]
    fn cloned_interner_keeps_symbols_and_independent_membership() {
        let mut original = Interner::default();
        let shared = original.intern("shared");
        let mut cloned = original.clone();
        let added = cloned.intern("clone only");

        assert_eq!(cloned.lookup("shared"), Some(shared));
        assert_eq!(cloned.resolve(shared), original.resolve(shared));
        assert_eq!(added.index(), 1);
        assert_eq!(original.lookup("clone only"), None);
        assert_eq!(original.len(), 1);
        drop(original);
        assert_eq!(cloned.resolve(shared), Some("shared"));
        assert_eq!(cloned.resolve(added), Some("clone only"));
    }

    #[test]
    fn interner_keeps_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Interner>();
    }
}
