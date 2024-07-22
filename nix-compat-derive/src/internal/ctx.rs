use std::cell::RefCell;
use std::fmt;
use std::thread::panicking;

use quote::ToTokens;

pub struct Context {
    errors: RefCell<Option<Vec<syn::Error>>>,
}

impl Context {
    pub fn new() -> Context {
        Context {
            errors: RefCell::new(Some(Vec::new())),
        }
    }

    pub fn syn_error(&self, error: syn::Error) {
        self.errors
            .borrow_mut()
            .as_mut()
            .take()
            .unwrap()
            .push(error);
    }

    pub fn error_spanned<T: ToTokens, D: fmt::Display>(&self, tokens: T, message: D) {
        self.syn_error(syn::Error::new_spanned(tokens, message));
    }

    pub fn check(&self) -> syn::Result<()> {
        let mut iter = self.errors.borrow_mut().take().unwrap().into_iter();
        let mut err = match iter.next() {
            None => return Ok(()),
            Some(err) => err,
        };
        for next_err in iter {
            err.combine(next_err);
        }
        Err(err)
    }
}

impl Drop for Context {
    fn drop(&mut self) {
        if self.errors.borrow().is_some() && !panicking() {
            panic!("Context dropped without checking errors");
        }
    }
}
