use std::{
    env, fs,
    path::{Path, PathBuf},
    process,
};

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
    let path = Path::new(file).to_owned();

    match tvix_eval::interpret(&contents, Some(path)) {
        Ok(result) => println!("=> {} :: {}", result, result.type_of()),
        Err(err) => eprintln!("{}", err),
    }
}

fn state_dir() -> Option<PathBuf> {
    let mut path = dirs::data_dir();
    if let Some(p) = path.as_mut() {
        p.push("tvix")
    }
    path
}

fn run_prompt() {
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
                match tvix_eval::interpret(&line, None) {
                    Ok(result) => {
                        println!("=> {} :: {}", result, result.type_of());
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
