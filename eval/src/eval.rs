use std::path::PathBuf;

use rnix;

use crate::{
    errors::{ErrorKind, EvalResult},
    value::Value,
};

pub fn interpret(code: &str, location: Option<PathBuf>) -> EvalResult<Value> {
    let parsed = rnix::ast::Root::parse(code);
    let errors = parsed.errors();

    if !errors.is_empty() {
        for err in errors {
            eprintln!("parse error: {}", err);
        }
        return Err(ErrorKind::ParseErrors(errors.to_vec()).into());
    }

    // If we've reached this point, there are no errors.
    let root_expr = parsed
        .tree()
        .expr()
        .expect("expression should exist if no errors occured");

    if std::env::var("TVIX_DISPLAY_AST").is_ok() {
        println!("{:?}", root_expr);
    }

    let result = crate::compiler::compile(root_expr, location)?;

    #[cfg(feature = "disassembler")]
    crate::disassembler::disassemble_chunk(&result.chunk);

    for warning in result.warnings {
        eprintln!(
            "warning: {:?} at {:?}",
            warning.kind,
            warning.node.text_range().start()
        )
    }

    crate::vm::run_chunk(result.chunk)
}
