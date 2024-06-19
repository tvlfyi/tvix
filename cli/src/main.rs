mod repl;

use clap::Parser;
use repl::Repl;
use std::rc::Rc;
use std::{fs, path::PathBuf};
use tracing::{instrument, Level, Span};
use tracing_indicatif::span_ext::IndicatifSpanExt;
use tvix_build::buildservice;
use tvix_eval::builtins::impure_builtins;
use tvix_eval::observer::{DisassemblingObserver, TracingObserver};
use tvix_eval::{ErrorKind, EvalIO, Value};
use tvix_glue::builtins::add_fetcher_builtins;
use tvix_glue::builtins::add_import_builtins;
use tvix_glue::tvix_io::TvixIO;
use tvix_glue::tvix_store_io::TvixStoreIO;
use tvix_glue::{builtins::add_derivation_builtins, configure_nix_path};

#[cfg(not(target_env = "msvc"))]
use tikv_jemallocator::Jemalloc;

#[cfg(not(target_env = "msvc"))]
#[global_allocator]
static GLOBAL: Jemalloc = Jemalloc;

#[derive(Parser, Clone)]
struct Args {
    /// A global log level to use when printing logs.
    /// It's also possible to set `RUST_LOG` according to
    /// `tracing_subscriber::filter::EnvFilter`, which will always have
    /// priority.
    #[arg(long, default_value_t=Level::INFO)]
    log_level: Level,

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

    /// Capture the time (relative to the start time of evaluation) of all events traced with
    /// `--trace-runtime`
    #[clap(long, env = "TVIX_TRACE_RUNTIME_TIMING", requires("trace_runtime"))]
    trace_runtime_timing: bool,

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

    #[arg(long, env, default_value = "dummy://")]
    build_service_addr: String,
}

fn init_io_handle(tokio_runtime: &tokio::runtime::Runtime, args: &Args) -> Rc<TvixStoreIO> {
    let (blob_service, directory_service, path_info_service, nar_calculation_service) =
        tokio_runtime
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

    let build_service = tokio_runtime
        .block_on({
            let blob_service = blob_service.clone();
            let directory_service = directory_service.clone();
            async move {
                buildservice::from_addr(
                    &args.build_service_addr,
                    blob_service.clone(),
                    directory_service.clone(),
                )
                .await
            }
        })
        .expect("unable to setup buildservice before interpreter setup");

    Rc::new(TvixStoreIO::new(
        blob_service.clone(),
        directory_service.clone(),
        path_info_service.into(),
        nar_calculation_service.into(),
        build_service.into(),
        tokio_runtime.handle().clone(),
    ))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum AllowIncomplete {
    Allow,
    #[default]
    RequireComplete,
}

impl AllowIncomplete {
    fn allow(&self) -> bool {
        matches!(self, Self::Allow)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct IncompleteInput;

/// Interprets the given code snippet, printing out warnings, errors
/// and the result itself. The return value indicates whether
/// evaluation succeeded.
#[instrument(skip_all, fields(indicatif.pb_show=1))]
fn interpret(
    tvix_store_io: Rc<TvixStoreIO>,
    code: &str,
    path: Option<PathBuf>,
    args: &Args,
    explain: bool,
    allow_incomplete: AllowIncomplete,
) -> Result<bool, IncompleteInput> {
    let span = Span::current();
    span.pb_start();
    span.pb_set_style(&tvix_tracing::PB_SPINNER_STYLE);
    span.pb_set_message("Setting up evaluator…");

    let mut eval = tvix_eval::Evaluation::new(
        Box::new(TvixIO::new(tvix_store_io.clone() as Rc<dyn EvalIO>)) as Box<dyn EvalIO>,
        true,
    );
    eval.strict = args.strict;
    eval.builtins.extend(impure_builtins());
    add_derivation_builtins(&mut eval, Rc::clone(&tvix_store_io));
    add_fetcher_builtins(&mut eval, Rc::clone(&tvix_store_io));
    add_import_builtins(&mut eval, tvix_store_io);
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
            if args.trace_runtime_timing {
                runtime_observer.enable_timing()
            }
            eval.runtime_observer = Some(&mut runtime_observer);
        }

        span.pb_set_message("Evaluating…");
        eval.evaluate(code, path)
    };

    if allow_incomplete.allow()
        && result.errors.iter().any(|err| {
            matches!(
                &err.kind,
                ErrorKind::ParseErrors(pes)
                    if pes.iter().any(|pe| matches!(pe, rnix::parser::ParseError::UnexpectedEOF))
            )
        })
    {
        return Err(IncompleteInput);
    }

    if args.display_ast {
        if let Some(ref expr) = result.expr {
            eprintln!("AST: {}", tvix_eval::pretty_print_expr(expr));
        }
    }

    for error in &result.errors {
        error.fancy_format_stderr();
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
    Ok(result.errors.is_empty())
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
        )
        .unwrap()
        {
            std::process::exit(1);
        }
    } else {
        let mut repl = Repl::new();
        repl.run(io_handle, &args)
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
        )
        .unwrap()
    };

    if !success {
        std::process::exit(1);
    }
}

fn println_result(result: &Value, raw: bool) {
    if raw {
        println!("{}", result.to_contextful_str().unwrap())
    } else {
        println!("=> {} :: {}", result, result.type_of())
    }
}
