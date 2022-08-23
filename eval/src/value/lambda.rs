//! This module implements the runtime representation of functions.
use std::rc::Rc;

use crate::chunk::Chunk;

use super::NixString;

#[derive(Clone, Debug)]
pub struct Lambda {
    name: Option<NixString>,
    chunk: Rc<Chunk>,
}
