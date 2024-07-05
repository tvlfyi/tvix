use std::rc::Rc;
use std::{collections::HashMap, path::PathBuf};

use rustyline::{error::ReadlineError, Editor};
use smol_str::SmolStr;
use tvix_eval::Value;
use tvix_glue::tvix_store_io::TvixStoreIO;

use crate::evaluate;
use crate::{assignment::Assignment, interpret, AllowIncomplete, Args, IncompleteInput};

fn state_dir() -> Option<PathBuf> {
    let mut path = dirs::data_dir();
    if let Some(p) = path.as_mut() {
        p.push("tvix")
    }
    path
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplCommand<'a> {
    Expr(&'a str),
    Assign(Assignment<'a>),
    Explain(&'a str),
    Print(&'a str),
    Quit,
    Help,
}

impl<'a> ReplCommand<'a> {
    const HELP: &'static str = "
Welcome to the Tvix REPL!

The following commands are supported:

  <expr>       Evaluate a Nix language expression and print the result, along with its inferred type
  <x> = <expr> Bind the result of an expression to a variable
  :d <expr>    Evaluate a Nix language expression and print a detailed description of the result
  :p <expr>    Evaluate a Nix language expression and print the result recursively
  :q           Exit the REPL
  :?, :h       Display this help text
";

    pub fn parse(input: &'a str) -> Self {
        if input.starts_with(':') {
            if let Some(without_prefix) = input.strip_prefix(":d ") {
                return Self::Explain(without_prefix);
            } else if let Some(without_prefix) = input.strip_prefix(":p ") {
                return Self::Print(without_prefix);
            }

            let input = input.trim_end();
            match input {
                ":q" => return Self::Quit,
                ":h" | ":?" => return Self::Help,
                _ => {}
            }
        }

        if let Some(assignment) = Assignment::parse(input) {
            return Self::Assign(assignment);
        }

        Self::Expr(input)
    }
}

#[derive(Debug)]
pub struct Repl {
    /// In-progress multiline input, when the input so far doesn't parse as a complete expression
    multiline_input: Option<String>,
    rl: Editor<()>,
    /// Local variables defined at the top-level in the repl
    env: HashMap<SmolStr, Value>,
}

impl Repl {
    pub fn new() -> Self {
        let rl = Editor::<()>::new().expect("should be able to launch rustyline");
        Self {
            multiline_input: None,
            rl,
            env: HashMap::new(),
        }
    }

    pub fn run(&mut self, io_handle: Rc<TvixStoreIO>, args: &Args) {
        if args.compile_only {
            eprintln!("warning: `--compile-only` has no effect on REPL usage!");
        }

        let history_path = match state_dir() {
            // Attempt to set up these paths, but do not hard fail if it
            // doesn't work.
            Some(mut path) => {
                let _ = std::fs::create_dir_all(&path);
                path.push("history.txt");
                let _ = self.rl.load_history(&path);
                Some(path)
            }

            None => None,
        };

        loop {
            let prompt = if self.multiline_input.is_some() {
                "         > "
            } else {
                "tvix-repl> "
            };

            let readline = self.rl.readline(prompt);
            match readline {
                Ok(line) => {
                    if line.is_empty() {
                        continue;
                    }

                    let input = if let Some(mi) = &mut self.multiline_input {
                        mi.push('\n');
                        mi.push_str(&line);
                        mi
                    } else {
                        &line
                    };

                    let res = match ReplCommand::parse(input) {
                        ReplCommand::Quit => break,
                        ReplCommand::Help => {
                            println!("{}", ReplCommand::HELP);
                            Ok(false)
                        }
                        ReplCommand::Expr(input) => interpret(
                            Rc::clone(&io_handle),
                            input,
                            None,
                            args,
                            false,
                            AllowIncomplete::Allow,
                            Some(&self.env),
                        ),
                        ReplCommand::Assign(Assignment { ident, value }) => {
                            match evaluate(
                                Rc::clone(&io_handle),
                                &value.to_string(), /* FIXME: don't re-parse */
                                None,
                                args,
                                AllowIncomplete::Allow,
                                Some(&self.env),
                            ) {
                                Ok(Some(value)) => {
                                    self.env.insert(ident.into(), value);
                                    Ok(true)
                                }
                                Ok(None) => Ok(true),
                                Err(incomplete) => Err(incomplete),
                            }
                        }
                        ReplCommand::Explain(input) => interpret(
                            Rc::clone(&io_handle),
                            input,
                            None,
                            args,
                            true,
                            AllowIncomplete::Allow,
                            Some(&self.env),
                        ),
                        ReplCommand::Print(input) => interpret(
                            Rc::clone(&io_handle),
                            input,
                            None,
                            &Args {
                                strict: true,
                                ..(args.clone())
                            },
                            false,
                            AllowIncomplete::Allow,
                            Some(&self.env),
                        ),
                    };

                    match res {
                        Ok(_) => {
                            self.rl.add_history_entry(input);
                            self.multiline_input = None;
                        }
                        Err(IncompleteInput) => {
                            if self.multiline_input.is_none() {
                                self.multiline_input = Some(line);
                            }
                        }
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
            self.rl.save_history(&path).unwrap();
        }
    }
}
