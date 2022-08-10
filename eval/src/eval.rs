use rnix::{self, types::TypedNode};

use crate::{errors::EvalResult, value::Value};

pub fn interpret(code: &str) -> EvalResult<Value> {
    let ast = rnix::parse(code);

    let errors = ast.errors();
    if !errors.is_empty() {
        todo!()
    }

    println!("{}", ast.root().dump());

    let code = crate::compiler::compile(ast)?;
    println!("code: {:?}", code);

    crate::vm::run_chunk(code)
}
