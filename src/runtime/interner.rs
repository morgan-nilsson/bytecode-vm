use std::collections::HashMap;

#[derive(Copy, Clone, PartialEq, Eq, Hash)]
pub struct Symbol(u32);

pub struct Interner {
    map: HashMap<Box<str>, Symbol>,
    strings: Vec<Box<str>>,
}

impl Interner {
    pub fn new() -> Self {
        Interner {
            map: HashMap::new(),
            strings: Vec::new(),
        }
    }

    pub fn intern(&mut self, s: &str) -> Symbol {
        if let Some(&symbol) = self.map.get(s) {
            return symbol;
        }

        let symbol = Symbol(self.strings.len() as u32);
        self.strings.push(s.to_owned().into_boxed_str());
        self.map.insert(s.to_owned().into_boxed_str(), symbol);
        symbol
    }

    pub fn resolve(&self, symbol: Symbol) -> Option<&str> {
        self.strings.get(symbol.0 as usize).map(|s| s.as_ref())
    }
}