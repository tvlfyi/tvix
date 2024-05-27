use std::{path::PathBuf, rc::Rc};

use rustyline::{error::ReadlineError, Editor};
use tvix_glue::tvix_store_io::TvixStoreIO;

use crate::{interpret, AllowIncomplete, Args, IncompleteInput};

fn state_dir() -> Option<PathBuf> {
    let mut path = dirs::data_dir();
    if let Some(p) = path.as_mut() {
        p.push("tvix")
    }
    path
}

#[derive(Debug)]
pub struct Repl {
    /// In-progress multiline input, when the input so far doesn't parse as a complete expression
    multiline_input: Option<String>,
    rl: Editor<()>,
}

impl Repl {
    pub fn new() -> Self {
        let rl = Editor::<()>::new().expect("should be able to launch rustyline");
        Self {
            multiline_input: None,
            rl,
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

                    let res = if let Some(without_prefix) = input.strip_prefix(":d ") {
                        interpret(
                            Rc::clone(&io_handle),
                            without_prefix,
                            None,
                            args,
                            true,
                            AllowIncomplete::Allow,
                        )
                    } else {
                        interpret(
                            Rc::clone(&io_handle),
                            input,
                            None,
                            args,
                            false,
                            AllowIncomplete::Allow,
                        )
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
