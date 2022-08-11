use std::{env, fs, path::PathBuf, process};

use rustyline::{error::ReadlineError, Editor};

fn main() {
    let mut args = env::args();
    if args.len() > 2 {
        println!("Usage: tvix-eval [script]");
        process::exit(1);
    }

    if let Some(file) = args.nth(1) {
        run_file(&file);
    } else {
        run_prompt();
    }
}

fn run_file(file: &str) {
    let contents = fs::read_to_string(file).expect("failed to read the input file");

    match tvix_eval::interpret(&contents) {
        Ok(result) => println!("=> {} :: {}", result, result.type_of()),
        Err(err) => eprintln!("{}", err),
    }
}

fn state_dir() -> Option<PathBuf> {
    let mut path = dirs::data_dir();
    path.as_mut().map(|p| p.push("tvix"));
    path
}

fn run_prompt() {
    let mut rl = Editor::<()>::new().expect("should be able to launch rustyline");

    let history_path = match state_dir() {
        Some(mut path) => {
            path.push("history.txt");
            rl.load_history(&path).ok();

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

                match tvix_eval::interpret(&line) {
                    Ok(result) => {
                        println!("=> {} :: {}", result, result.type_of());
                        rl.add_history_entry(line);
                    }
                    Err(err) => println!("{}", err),
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
