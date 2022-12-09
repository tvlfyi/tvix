use std::{fs, path::PathBuf};

use clap::Parser;
use rustyline::{error::ReadlineError, Editor};
use tvix_eval::Value; //{Error, EvalWarning, Evaluation, Value};

#[derive(Parser)]
struct Args {
    /// Path to a script to evaluate
    script: Option<PathBuf>,

    #[clap(long, short = 'E')]
    expr: Option<String>,

    /// A colon-separated list of directories to use to resolve `<...>`-style paths
    #[clap(long, short = 'I', env = "NIX_PATH")]
    nix_search_path: Option<String>,

    /// Print "raw" (unquoted) output.
    #[clap(long)]
    raw: bool,
}

/// Interprets the given code snippet, printing out warnings, errors
/// and the result itself. The return value indicates whether
/// evaluation succeeded.
fn interpret(code: &str, path: Option<PathBuf>, args: &Args) -> bool {
    let mut eval = tvix_eval::Evaluation::new(code, path);
    eval.nix_path = args.nix_search_path.clone();

    let source_map = eval.source_map();
    let result = eval.evaluate();

    for error in &result.errors {
        error.fancy_format_stderr(&source_map);
    }

    for warning in &result.warnings {
        warning.fancy_format_stderr(&source_map);
    }

    if let Some(value) = result.value.as_ref() {
        println_result(value, args.raw);
    }

    // inform the caller about any errors
    result.errors.is_empty()
}

fn main() {
    let args = Args::parse();

    if let Some(file) = &args.script {
        run_file(file.clone(), &args)
    } else if let Some(expr) = &args.expr {
        if !interpret(expr, None, &args) {
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

    if !interpret(&contents, Some(path), args) {
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
                interpret(&line, None, args);
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
