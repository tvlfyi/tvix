use std::path::PathBuf;

use crate::{
    builtins::global_builtins,
    errors::{Error, ErrorKind, EvalResult},
    nix_search_path::NixSearchPath,
    observer::{DisassemblingObserver, NoOpObserver, TracingObserver},
    pretty_ast::pretty_print_expr,
    value::Value,
    SourceCode,
};

/// Runtime options for the Tvix interpreter
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "repl", derive(clap::Parser))]
pub struct Options {
    /// Dump the raw AST to stdout before interpreting
    #[cfg_attr(feature = "repl", clap(long, env = "TVIX_DISPLAY_AST"))]
    display_ast: bool,

    /// Dump the bytecode to stdout before evaluating
    #[cfg_attr(feature = "repl", clap(long, env = "TVIX_DUMP_BYTECODE"))]
    dump_bytecode: bool,

    /// Trace the runtime of the VM
    #[cfg_attr(feature = "repl", clap(long, env = "TVIX_TRACE_RUNTIME"))]
    trace_runtime: bool,

    /// Print warnings
    #[cfg_attr(
        feature = "repl",
        clap(long, env = "TVIX_WARNINGS", default_value = "true")
    )]
    warnings: bool,

    /// A colon-separated list of directories to use to resolve `<...>`-style paths
    #[cfg_attr(feature = "repl", clap(long, short = 'I', env = "NIX_PATH"))]
    nix_search_path: Option<NixSearchPath>,

    #[cfg_attr(feature = "repl", clap(long))]
    pub raw: bool,
}

impl Options {
    #[cfg(test)]
    pub(crate) fn test_options() -> Options {
        Options {
            warnings: false,
            ..Options::default()
        }
    }
}

pub fn interpret(code: &str, location: Option<PathBuf>, options: Options) -> EvalResult<Value> {
    let source = SourceCode::new();
    let file = source.add_file(
        location
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "[tvix-repl]".into()),
        code.into(),
    );

    let parsed = rnix::ast::Root::parse(code);
    let errors = parsed.errors();

    if !errors.is_empty() {
        let err = Error {
            kind: ErrorKind::ParseErrors(errors.to_vec()),
            span: file.span,
        };
        err.fancy_format_stderr(&source);
        return Err(err);
    }

    // If we've reached this point, there are no errors.
    let root_expr = parsed
        .tree()
        .expr()
        .expect("expression should exist if no errors occured");

    if options.display_ast {
        println!("{}", pretty_print_expr(&root_expr));
    }

    let builtins = crate::compiler::prepare_globals(Box::new(global_builtins(source.clone())));
    let result = if options.dump_bytecode {
        crate::compiler::compile(
            &root_expr,
            location,
            file.clone(),
            builtins,
            &mut DisassemblingObserver::new(source.clone(), std::io::stderr()),
        )
    } else {
        crate::compiler::compile(
            &root_expr,
            location,
            file.clone(),
            builtins,
            &mut NoOpObserver::default(),
        )
    }?;

    if options.warnings {
        for warning in result.warnings {
            warning.fancy_format_stderr(&source);
        }
    }

    for error in &result.errors {
        error.fancy_format_stderr(&source);
    }

    if let Some(err) = result.errors.last() {
        return Err(err.clone());
    }

    let result = if options.trace_runtime {
        crate::vm::run_lambda(
            options.nix_search_path.unwrap_or_default(),
            &mut TracingObserver::new(std::io::stderr()),
            result.lambda,
        )
    } else {
        crate::vm::run_lambda(
            options.nix_search_path.unwrap_or_default(),
            &mut NoOpObserver::default(),
            result.lambda,
        )
    };

    if let Err(err) = &result {
        err.fancy_format_stderr(&source);
    }

    result.map(|r| {
        if options.warnings {
            for warning in r.warnings {
                warning.fancy_format_stderr(&source);
            }
        }
        r.value
    })
}
