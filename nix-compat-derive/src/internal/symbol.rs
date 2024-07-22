use std::fmt;

use syn::Path;

#[derive(Copy, Clone)]
pub struct Symbol(&'static str);

pub const NIX: Symbol = Symbol("nix");
pub const VERSION: Symbol = Symbol("version");
pub const DEFAULT: Symbol = Symbol("default");
pub const FROM: Symbol = Symbol("from");
pub const TRY_FROM: Symbol = Symbol("try_from");
pub const FROM_STR: Symbol = Symbol("from_str");
pub const CRATE: Symbol = Symbol("crate");

impl PartialEq<Symbol> for Path {
    fn eq(&self, word: &Symbol) -> bool {
        self.is_ident(word.0)
    }
}

impl<'a> PartialEq<Symbol> for &'a Path {
    fn eq(&self, word: &Symbol) -> bool {
        self.is_ident(word.0)
    }
}

impl fmt::Display for Symbol {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str(self.0)
    }
}
