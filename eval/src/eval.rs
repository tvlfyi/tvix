use std::{path::PathBuf, rc::Rc};

use crate::{
    builtins::global_builtins,
    errors::{Error, ErrorKind, EvalResult},
    observer::{DisassemblingObserver, NoOpObserver, TracingObserver},
    value::Value,
};

pub fn interpret(code: &str, location: Option<PathBuf>) -> EvalResult<Value> {
    let mut codemap = codemap::CodeMap::new();
    let file = codemap.add_file(
        location
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "[tvix-repl]".into()),
        code.into(),
    );
    let codemap = Rc::new(codemap);

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

    let result = if std::env::var("TVIX_DUMP_BYTECODE").is_ok() {
        crate::compiler::compile(
            root_expr,
            location,
            file.clone(),
            global_builtins(),
            &mut DisassemblingObserver::new(codemap.clone(), std::io::stderr()),
        )
    } else {
        crate::compiler::compile(
            root_expr,
            location,
            file.clone(),
            global_builtins(),
            &mut NoOpObserver::default(),
        )
    }?;

    for warning in result.warnings {
        warning.fancy_format_stderr(&codemap);
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

    if std::env::var("TVIX_TRACE_RUNTIME").is_ok() {
        crate::vm::run_lambda(&mut TracingObserver::new(std::io::stderr()), result.lambda)
    } else {
        crate::vm::run_lambda(&mut NoOpObserver::default(), result.lambda)
    }
}
