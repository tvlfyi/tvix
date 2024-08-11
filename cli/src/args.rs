use std::path::PathBuf;

use clap::Parser;
use tracing::Level;
use tvix_store::utils::ServiceUrlsMemory;

/// Provides a CLI interface to trigger evaluation using tvix-eval.
///
/// Uses configured tvix-[ca]store and tvix-build components,
/// and by default a set of builtins similar to these present in Nix.
///
/// None of the stores available add to the local `/nix/store` location.
///
/// The CLI interface is not stable and subject to change.
#[derive(Parser, Clone)]
pub struct Args {
    /// A global log level to use when printing logs.
    /// It's also possible to set `RUST_LOG` according to
    /// `tracing_subscriber::filter::EnvFilter`, which will always have
    /// priority.
    #[arg(long, default_value_t=Level::INFO)]
    pub log_level: Level,

    /// Path to a script to evaluate
    pub script: Option<PathBuf>,

    #[clap(long, short = 'E')]
    pub expr: Option<String>,

    /// Dump the raw AST to stdout before interpreting
    #[clap(long, env = "TVIX_DISPLAY_AST")]
    pub display_ast: bool,

    /// Dump the bytecode to stdout before evaluating
    #[clap(long, env = "TVIX_DUMP_BYTECODE")]
    pub dump_bytecode: bool,

    /// Trace the runtime of the VM
    #[clap(long, env = "TVIX_TRACE_RUNTIME")]
    pub trace_runtime: bool,

    /// Capture the time (relative to the start time of evaluation) of all events traced with
    /// `--trace-runtime`
    #[clap(long, env = "TVIX_TRACE_RUNTIME_TIMING", requires("trace_runtime"))]
    pub trace_runtime_timing: bool,

    /// Only compile, but do not execute code. This will make Tvix act
    /// sort of like a linter.
    #[clap(long)]
    pub compile_only: bool,

    /// Don't print warnings.
    #[clap(long)]
    pub no_warnings: bool,

    /// A colon-separated list of directories to use to resolve `<...>`-style paths
    #[clap(long, short = 'I', env = "NIX_PATH")]
    pub nix_search_path: Option<String>,

    /// Print "raw" (unquoted) output.
    #[clap(long)]
    pub raw: bool,

    /// Strictly evaluate values, traversing them and forcing e.g.
    /// elements of lists and attribute sets before printing the
    /// return value.
    #[clap(long)]
    pub strict: bool,

    #[clap(flatten)]
    pub service_addrs: ServiceUrlsMemory,

    #[arg(long, env, default_value = "dummy://")]
    pub build_service_addr: String,

    /// An optional path in which Derivations encountered during evaluation
    /// are dumped into, after evaluation. If it doesn't exist, the directory is created.
    ///
    /// Files dumped there are named like they would show up in `/nix/store`,
    /// if produced by Nix. Existing files are not overwritten.
    ///
    /// This is only for debugging and diffing purposes for post-eval inspection;
    /// Tvix does not read from these.
    #[clap(long)]
    pub drv_dumpdir: Option<PathBuf>,
}
