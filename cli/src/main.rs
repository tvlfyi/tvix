use clap::Parser;
use std::rc::Rc;
use std::{fs, path::PathBuf};
use tvix_cli::args::Args;
use tvix_cli::repl::Repl;
use tvix_cli::{init_io_handle, interpret, AllowIncomplete};
use tvix_eval::observer::DisassemblingObserver;
use tvix_glue::tvix_store_io::TvixStoreIO;

#[cfg(not(target_env = "msvc"))]
use tikv_jemallocator::Jemalloc;

#[cfg(not(target_env = "msvc"))]
#[global_allocator]
static GLOBAL: Jemalloc = Jemalloc;

/// Interpret the given code snippet, but only run the Tvix compiler
/// on it and return errors and warnings.
fn lint(code: &str, path: Option<PathBuf>, args: &Args) -> bool {
    let mut eval_builder = tvix_eval::Evaluation::builder_impure().with_strict(args.strict);

    let source_map = eval_builder.source_map().clone();

    let mut compiler_observer = DisassemblingObserver::new(source_map.clone(), std::io::stderr());

    if args.dump_bytecode {
        eval_builder.set_compiler_observer(Some(&mut compiler_observer));
    }

    if args.trace_runtime {
        eprintln!("warning: --trace-runtime has no effect with --compile-only!");
    }

    let eval = eval_builder.build();
    let result = eval.compile_only(code, path);

    if args.display_ast {
        if let Some(ref expr) = result.expr {
            eprintln!("AST: {}", tvix_eval::pretty_print_expr(expr));
        }
    }

    for error in &result.errors {
        error.fancy_format_stderr();
    }

    for warning in &result.warnings {
        warning.fancy_format_stderr(&source_map);
    }

    // inform the caller about any errors
    result.errors.is_empty()
}

fn main() {
    let args = Args::parse();

    let _ = tvix_tracing::TracingBuilder::default()
        .level(args.log_level)
        .enable_progressbar()
        .build()
        .expect("unable to set up tracing subscriber");
    let tokio_runtime = tokio::runtime::Runtime::new().expect("failed to setup tokio runtime");

    let io_handle = init_io_handle(&tokio_runtime, &args);

    if let Some(file) = &args.script {
        run_file(io_handle, file.clone(), &args)
    } else if let Some(expr) = &args.expr {
        if !interpret(
            io_handle,
            expr,
            None,
            &args,
            false,
            AllowIncomplete::RequireComplete,
            None, // TODO(aspen): Pass in --arg/--argstr here
            None,
            None,
        )
        .unwrap()
        .finalize()
        {
            std::process::exit(1);
        }
    } else {
        let mut repl = Repl::new(io_handle, &args);
        repl.run()
    }
}

fn run_file(io_handle: Rc<TvixStoreIO>, mut path: PathBuf, args: &Args) {
    if path.is_dir() {
        path.push("default.nix");
    }
    let contents = fs::read_to_string(&path).expect("failed to read the input file");

    let success = if args.compile_only {
        lint(&contents, Some(path), args)
    } else {
        interpret(
            io_handle,
            &contents,
            Some(path),
            args,
            false,
            AllowIncomplete::RequireComplete,
            None,
            None,
            None,
        )
        .unwrap()
        .finalize()
    };

    if !success {
        std::process::exit(1);
    }
}
