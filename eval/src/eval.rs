use rnix::{self, types::TypedNode};

use crate::errors::EvalResult;

pub fn interpret(code: String) -> EvalResult<String> {
    let ast = rnix::parse(&code);

    let errors = ast.errors();
    if !errors.is_empty() {
        todo!()
    }

    println!("{}", ast.root().dump());

    let code = crate::compiler::compile(ast)?;
    println!("code: {:?}", code);

    let value = crate::vm::run_chunk(code)?;
    Ok(format!("value: {} :: {}", value, value.type_of()))
}
