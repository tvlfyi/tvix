use std::{fs, path::PathBuf};

use clap::Parser;
use rustyline::{error::ReadlineError, Editor};
use tvix_eval::Value;

#[derive(Parser)]
struct Args {
    /// Path to a script to evaluate
    script: Option<PathBuf>,

    #[clap(long, short = 'E')]
    expr: Option<String>,

    #[clap(flatten)]
    eval_options: tvix_eval::Options,
}

fn main() {
    let args = Args::parse();

    if let Some(file) = args.script {
        run_file(file, args.eval_options)
    } else if let Some(expr) = args.expr {
        let raw = args.eval_options.raw;
        if let Ok(result) = tvix_eval::interpret(&expr, None, args.eval_options) {
            println_result(&result, raw);
        }
    } else {
        run_prompt(args.eval_options)
    }
}

fn run_file(mut path: PathBuf, eval_options: tvix_eval::Options) {
    if path.is_dir() {
        path.push("default.nix");
    }
    let contents = fs::read_to_string(&path).expect("failed to read the input file");
    let raw = eval_options.raw;
    match tvix_eval::interpret(&contents, Some(path), eval_options) {
        Ok(result) => println_result(&result, raw),
        Err(err) => eprintln!("{}", err),
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

fn run_prompt(eval_options: tvix_eval::Options) {
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
                match tvix_eval::interpret(&line, None, eval_options.clone()) {
                    Ok(result) => {
                        println!("=> {} :: {}", result, result.type_of());
                    }
                    Err(_) => { /* interpret takes care of error formatting */ }
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
