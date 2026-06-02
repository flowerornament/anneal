//! Per-session symbol interner for physical runtime strings.
#![allow(dead_code)]

use std::collections::BTreeMap;

use super::ids::SymbolId;

#[derive(Clone, Debug, Default)]
pub(crate) struct Interner {
    by_text: BTreeMap<Box<str>, SymbolId>,
    texts: Vec<Box<str>>,
}

impl Interner {
    pub(crate) fn intern(&mut self, text: impl AsRef<str>) -> SymbolId {
        let text = text.as_ref();
        if let Some(symbol) = self.by_text.get(text) {
            return *symbol;
        }

        let symbol = SymbolId::from_index(self.texts.len());
        let stored: Box<str> = text.into();
        self.texts.push(stored.clone());
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

    pub(crate) fn is_empty(&self) -> bool {
        self.texts.is_empty()
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
}
