use rnix::{self, types::TypedNode};
use std::fmt::Write;

use crate::errors::{Error, EvalResult};

pub fn interpret(code: String) -> EvalResult<String> {
    let ast = rnix::parse(&code);

    let errors = ast.errors();
    if !errors.is_empty() {
        todo!()
    }

    let mut out = String::new();
    println!("{}", ast.root().dump());

    let code = crate::compiler::compile(ast)?;
    writeln!(out, "code: {:?}", code).ok();

    let value = crate::vm::run_chunk(code)?;
    writeln!(out, "value: {:?}", value).ok();

    Ok(out)
}
