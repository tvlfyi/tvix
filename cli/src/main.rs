use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::{fs, path::PathBuf};
use tvix_glue::known_paths::KnownPaths;
use tvix_glue::{builtins::add_derivation_builtins, configure_nix_path};

use clap::Parser;
use rustyline::{error::ReadlineError, Editor};
use tvix_castore::blobservice::MemoryBlobService;
use tvix_castore::directoryservice::MemoryDirectoryService;
use tvix_eval::observer::{DisassemblingObserver, TracingObserver};
use tvix_eval::Value;
use tvix_glue::tvix_store_io::TvixStoreIO;
use tvix_store::pathinfoservice::MemoryPathInfoService;

#[derive(Parser)]
struct Args {
    /// Path to a script to evaluate
    script: Option<PathBuf>,

    #[clap(long, short = 'E')]
    expr: Option<String>,

    /// Dump the raw AST to stdout before interpreting
    #[clap(long, env = "TVIX_DISPLAY_AST")]
    display_ast: bool,

    /// Dump the bytecode to stdout before evaluating
    #[clap(long, env = "TVIX_DUMP_BYTECODE")]
    dump_bytecode: bool,

    /// Trace the runtime of the VM
    #[clap(long, env = "TVIX_TRACE_RUNTIME")]
    trace_runtime: bool,

    /// Only compile, but do not execute code. This will make Tvix act
    /// sort of like a linter.
    #[clap(long)]
    compile_only: bool,

    /// Don't print warnings.
    #[clap(long)]
    no_warnings: bool,

    /// A colon-separated list of directories to use to resolve `<...>`-style paths
    #[clap(long, short = 'I', env = "NIX_PATH")]
    nix_search_path: Option<String>,

    /// Print "raw" (unquoted) output.
    #[clap(long)]
    raw: bool,

    /// Strictly evaluate values, traversing them and forcing e.g.
    /// elements of lists and attribute sets before printing the
    /// return value.
    #[clap(long)]
    strict: bool,
}

/// Interprets the given code snippet, printing out warnings, errors
/// and the result itself. The return value indicates whether
/// evaluation succeeded.
fn interpret(code: &str, path: Option<PathBuf>, args: &Args, explain: bool) -> bool {
    let mut eval = tvix_eval::Evaluation::new_impure(code, path);

    eval.strict = args.strict;

    let blob_service = Arc::new(MemoryBlobService::default());
    let directory_service = Arc::new(MemoryDirectoryService::default());
    let path_info_service = Arc::new(MemoryPathInfoService::new(
        blob_service.clone(),
        directory_service.clone(),
    ));

    let known_paths: Rc<RefCell<KnownPaths>> = Default::default();
    add_derivation_builtins(&mut eval, known_paths.clone());
    configure_nix_path(&mut eval, &args.nix_search_path);

    let tokio_runtime = tokio::runtime::Runtime::new().unwrap();
    eval.io_handle = Box::new(tvix_glue::tvix_io::TvixIO::new(
        known_paths.clone(),
        TvixStoreIO::new(
            blob_service,
            directory_service,
            path_info_service,
            tokio_runtime.handle().clone(),
        ),
    ));

    let source_map = eval.source_map();
    let result = {
        let mut compiler_observer =
            DisassemblingObserver::new(source_map.clone(), std::io::stderr());
        if args.dump_bytecode {
            eval.compiler_observer = Some(&mut compiler_observer);
        }

        let mut runtime_observer = TracingObserver::new(std::io::stderr());
        if args.trace_runtime {
            eval.runtime_observer = Some(&mut runtime_observer);
        }

        eval.evaluate()
    };

    if args.display_ast {
        if let Some(ref expr) = result.expr {
            eprintln!("AST: {}", tvix_eval::pretty_print_expr(expr));
        }
    }

    for error in &result.errors {
        error.fancy_format_stderr(&source_map);
    }

    if !args.no_warnings {
        for warning in &result.warnings {
            warning.fancy_format_stderr(&source_map);
        }
    }

    if let Some(value) = result.value.as_ref() {
        if explain {
            println!("=> {}", value.explain());
        } else {
            println_result(value, args.raw);
        }
    }

    // inform the caller about any errors
    result.errors.is_empty()
}

/// Interpret the given code snippet, but only run the Tvix compiler
/// on it and return errors and warnings.
fn lint(code: &str, path: Option<PathBuf>, args: &Args) -> bool {
    let mut eval = tvix_eval::Evaluation::new_impure(code, path);
    eval.strict = args.strict;

    let source_map = eval.source_map();

    let mut compiler_observer = DisassemblingObserver::new(source_map.clone(), std::io::stderr());

    if args.dump_bytecode {
        eval.compiler_observer = Some(&mut compiler_observer);
    }

    if args.trace_runtime {
        eprintln!("warning: --trace-runtime has no effect with --compile-only!");
    }

    let result = eval.compile_only();

    if args.display_ast {
        if let Some(ref expr) = result.expr {
            eprintln!("AST: {}", tvix_eval::pretty_print_expr(expr));
        }
    }

    for error in &result.errors {
        error.fancy_format_stderr(&source_map);
    }

    for warning in &result.warnings {
        warning.fancy_format_stderr(&source_map);
    }

    // inform the caller about any errors
    result.errors.is_empty()
}

fn main() {
    let args = Args::parse();

    if let Some(file) = &args.script {
        run_file(file.clone(), &args)
    } else if let Some(expr) = &args.expr {
        if !interpret(expr, None, &args, false) {
            std::process::exit(1);
        }
    } else {
        run_prompt(&args)
    }
}

fn run_file(mut path: PathBuf, args: &Args) {
    if path.is_dir() {
        path.push("default.nix");
    }
    let contents = fs::read_to_string(&path).expect("failed to read the input file");

    let success = if args.compile_only {
        lint(&contents, Some(path), args)
    } else {
        interpret(&contents, Some(path), args, false)
    };

    if !success {
        std::process::exit(1);
    }
}

fn println_result(result: &Value, raw: bool) {
    if raw {
        println!("{}", result.to_str().unwrap().as_str())
    } else {
        println!("=> {} :: {}", result, result.type_of())
    }
}

fn state_dir() -> Option<PathBuf> {
    let mut path = dirs::data_dir();
    if let Some(p) = path.as_mut() {
        p.push("tvix")
    }
    path
}

fn run_prompt(args: &Args) {
    let mut rl = Editor::<()>::new().expect("should be able to launch rustyline");

    if args.compile_only {
        eprintln!("warning: `--compile-only` has no effect on REPL usage!");
    }

    let history_path = match state_dir() {
        // Attempt to set up these paths, but do not hard fail if it
        // doesn't work.
        Some(mut path) => {
            let _ = std::fs::create_dir_all(&path);
            path.push("history.txt");
            let _ = rl.load_history(&path);
            Some(path)
        }

        None => None,
    };

    loop {
        let readline = rl.readline("tvix-repl> ");
        match readline {
            Ok(line) => {
                if line.is_empty() {
                    continue;
                }

                rl.add_history_entry(&line);

                if let Some(without_prefix) = line.strip_prefix(":d ") {
                    interpret(without_prefix, None, args, true);
                } else {
                    interpret(&line, None, args, false);
                }
            }
            Err(ReadlineError::Interrupted) | Err(ReadlineError::Eof) => break,

            Err(err) => {
                eprintln!("error: {}", err);
                break;
            }
        }
    }

    if let Some(path) = history_path {
        rl.save_history(&path).unwrap();
    }
}
