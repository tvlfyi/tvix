use clap::Parser;
use rustyline::{error::ReadlineError, Editor};
use std::rc::Rc;
use std::{fs, path::PathBuf};
use tvix_eval::builtins::impure_builtins;
use tvix_eval::observer::{DisassemblingObserver, TracingObserver};
use tvix_eval::{EvalIO, Value};
use tvix_glue::tvix_io::TvixIO;
use tvix_glue::tvix_store_io::TvixStoreIO;
use tvix_glue::{builtins::add_derivation_builtins, configure_nix_path};

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

    #[arg(long, env, default_value = "memory://")]
    blob_service_addr: String,

    #[arg(long, env, default_value = "memory://")]
    directory_service_addr: String,

    #[arg(long, env, default_value = "memory://")]
    path_info_service_addr: String,
}

/// Interprets the given code snippet, printing out warnings, errors
/// and the result itself. The return value indicates whether
/// evaluation succeeded.
fn interpret(code: &str, path: Option<PathBuf>, args: &Args, explain: bool) -> bool {
    let tokio_runtime = tokio::runtime::Runtime::new().expect("failed to setup tokio runtime");

    let (blob_service, directory_service, path_info_service) = tokio_runtime
        .block_on({
            let blob_service_addr = args.blob_service_addr.clone();
            let directory_service_addr = args.directory_service_addr.clone();
            let path_info_service_addr = args.path_info_service_addr.clone();
            async move {
                tvix_store::utils::construct_services(
                    blob_service_addr,
                    directory_service_addr,
                    path_info_service_addr,
                )
                .await
            }
        })
        .expect("unable to setup {blob|directory|pathinfo}service before interpreter setup");

    let tvix_store_io = Rc::new(TvixStoreIO::new(
        blob_service.clone(),
        directory_service.clone(),
        path_info_service.into(),
        tokio_runtime.handle().clone(),
    ));

    let mut eval = tvix_eval::Evaluation::new(
        Box::new(TvixIO::new(tvix_store_io.clone() as Rc<dyn EvalIO>)) as Box<dyn EvalIO>,
        true,
    );
    eval.strict = args.strict;
    eval.builtins.extend(impure_builtins());
    add_derivation_builtins(&mut eval, tvix_store_io);
    configure_nix_path(&mut eval, &args.nix_search_path);

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

        eval.evaluate(code, path)
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
    let mut eval = tvix_eval::Evaluation::new_impure();
    eval.strict = args.strict;

    let source_map = eval.source_map();

    let mut compiler_observer = DisassemblingObserver::new(source_map.clone(), std::io::stderr());

    if args.dump_bytecode {
        eval.compiler_observer = Some(&mut compiler_observer);
    }

    if args.trace_runtime {
        eprintln!("warning: --trace-runtime has no effect with --compile-only!");
    }

    let result = eval.compile_only(code, path);

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
        println!("{}", result.to_contextful_str().unwrap().as_str())
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
