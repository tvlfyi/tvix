use std::{
    env, fs,
    io::{self, Write},
    mem, process,
};

mod chunk;
mod compiler;
mod errors;
mod eval;
mod opcode;
mod value;
mod vm;

#[cfg(test)]
mod tests;

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

    run(contents);
}

fn run_prompt() {
    let mut line = String::new();

    loop {
        print!("> ");
        io::stdout().flush().unwrap();
        io::stdin()
            .read_line(&mut line)
            .expect("failed to read user input");
        run(mem::take(&mut line));
        line.clear();
    }
}

fn run(code: String) {
    match eval::interpret(&code) {
        Ok(result) => println!("=> {} :: {}", result, result.type_of()),
        Err(err) => eprintln!("{}", err),
    }
}
