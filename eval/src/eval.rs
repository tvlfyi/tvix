use std::path::PathBuf;

use rnix::{self, types::TypedNode};

use crate::{errors::EvalResult, value::Value};

pub fn interpret(code: &str, location: Option<PathBuf>) -> EvalResult<Value> {
    let ast = rnix::parse(code);

    let errors = ast.errors();
    if !errors.is_empty() {
        todo!()
    }

    if let Ok(_) = std::env::var("TVIX_DISPLAY_AST") {
        println!("{}", ast.root().dump());
    }

    let result = crate::compiler::compile(ast, location)?;
    println!("code: {:?}", result.chunk);

    for warning in result.warnings {
        eprintln!(
            "warning: {:?} at {:?}",
            warning.kind,
            warning.node.text_range().start()
        )
    }

    crate::vm::run_chunk(result.chunk)
}
