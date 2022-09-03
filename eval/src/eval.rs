use std::{path::PathBuf, rc::Rc};

use crate::{
    builtins::global_builtins,
    errors::{Error, ErrorKind, EvalResult},
    value::Value,
};

pub fn interpret(code: &str, location: Option<PathBuf>) -> EvalResult<Value> {
    let mut codemap = codemap::CodeMap::new();
    let file = codemap.add_file(
        location
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "<repl>".into()),
        code.into(),
    );

    let parsed = rnix::ast::Root::parse(code);
    let errors = parsed.errors();

    if !errors.is_empty() {
        for err in errors {
            eprintln!("parse error: {}", err);
        }
        return Err(Error {
            kind: ErrorKind::ParseErrors(errors.to_vec()),
            span: file.span,
        });
    }

    // If we've reached this point, there are no errors.
    let root_expr = parsed
        .tree()
        .expr()
        .expect("expression should exist if no errors occured");

    if std::env::var("TVIX_DISPLAY_AST").is_ok() {
        println!("{:?}", root_expr);
    }

    let result = crate::compiler::compile(
        root_expr,
        location,
        &file,
        global_builtins(),
        #[cfg(feature = "disassembler")]
        Rc::new(codemap),
    )?;
    let lambda = Rc::new(result.lambda);

    #[cfg(feature = "disassembler")]
    crate::disassembler::disassemble_lambda(lambda.clone());

    for warning in result.warnings {
        eprintln!(
            "warning: {:?} at `{}`[line {}]",
            warning.kind,
            file.source_slice(warning.span),
            file.find_line(warning.span.low()) + 1
        )
    }

    for error in &result.errors {
        eprintln!(
            "compiler error: {:?} at `{}`[line {}]",
            error.kind,
            file.source_slice(error.span),
            file.find_line(error.span.low()) + 1
        );
    }

    if let Some(err) = result.errors.last() {
        return Err(err.clone());
    }

    crate::vm::run_lambda(lambda)
}
