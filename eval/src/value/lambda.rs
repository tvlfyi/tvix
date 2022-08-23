//! This module implements the runtime representation of functions.
use std::rc::Rc;

use crate::chunk::Chunk;

#[derive(Clone, Debug)]
pub struct Lambda {
    // name: Option<NixString>,
    pub(crate) chunk: Rc<Chunk>,
}

impl Lambda {
    pub fn new_anonymous() -> Self {
        Lambda {
            // name: None,
            chunk: Default::default(),
        }
    }

    pub fn chunk(&mut self) -> &mut Rc<Chunk> {
        &mut self.chunk
    }
}
