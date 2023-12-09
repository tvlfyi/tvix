fn main() {
    main_args(std::env::args().collect()).unwrap_or_else(|e| e.exit());
}

pub fn main_args(args: Vec<String>) -> clap::error::Result<NixResult> {
    let matches = clap::Command::new("nix-store")
        .subcommand(clap::Command::new("--add").arg(clap::Arg::new("FILE").required(true).index(1)))
        .try_get_matches_from(args.iter())?;
    if let Some(add) = matches.subcommand_matches("--add") {
        let file = add.get_one::<String>("FILE").expect("--add needs a file");
        let file_contents = std::fs::read_to_string(file)
            .unwrap_or_else(|_| panic!("file {} does not exist", file));
        Ok(NixResult::FileAddedToStore {
            content: file_contents,
        })
    } else {
        panic!("read some arguments that we do not know: {:?}", args)
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum NixResult {
    FileAddedToStore { content: String },
}

#[cfg(test)]
mod integration_tests {
    use std::collections::VecDeque;
    use std::io::Write;

    use super::*;

    #[derive(Debug)]
    enum NixOutput {
        #[allow(dead_code)]
        Err {
            status: i32,
            stdout: String,
            stderr: String,
        },
        Ok {
            stdout: String,
            stderr: String,
        },
    }

    fn run_nix_command(cmd: &str, args: Vec<String>) -> NixOutput {
        let out = std::process::Command::new(cmd)
            .args(args)
            .output()
            .unwrap_or_else(|_| panic!("could not run {}", cmd));
        match out.status.code().expect("no status code!") {
            0 => NixOutput::Ok {
                stdout: String::from_utf8_lossy(&out.stdout).trim_end().to_string(),
                stderr: String::from_utf8_lossy(&out.stderr).trim_end().to_string(),
            },
            status => NixOutput::Err {
                status,
                stdout: String::from_utf8_lossy(&out.stdout).trim_end().to_string(),
                stderr: String::from_utf8_lossy(&out.stderr).trim_end().to_string(),
            },
        }
    }

    fn nix_nix_store<'a>(args: Vec<String>) -> NixResult {
        match run_nix_command("nix-store", args) {
            err @ NixOutput::Err { .. } => panic!("nix-store --add failed: {:#?}", err),
            NixOutput::Ok { stdout, .. } => NixResult::FileAddedToStore {
                content: std::fs::read_to_string(&stdout)
                    .unwrap_or_else(|_| panic!("cannot open {} as store file", stdout)),
            },
        }
    }

    fn tvix_nix_store<'a>(args: Vec<String>) -> NixResult {
        eprintln!("running tvix with arguments {:?}", args);
        let mut args = VecDeque::from(args);
        args.push_front("tvix-store".to_string());
        super::main_args(Vec::from(args))
            .unwrap_or_else(|e| panic!("clap command line parsing failed:\n{}", e))
    }

    #[test]
    #[cfg_attr(not(feature = "integration_tests"), ignore)]
    fn test_nix_store_add() {
        let file_content = "I am a copied file";
        let mut tempfile = tempfile::NamedTempFile::new().expect("cannot create temp file");
        tempfile
            .write_all(file_content.as_bytes())
            .expect("could not write to tempfile");
        assert_eq!(
            tvix_nix_store(vec![
                "--add".to_string(),
                tempfile.path().as_os_str().to_string_lossy().into_owned()
            ]),
            nix_nix_store(vec![
                "--add".to_string(),
                tempfile.path().as_os_str().to_string_lossy().into_owned()
            ]),
            "added file contents were not the same"
        );

        // make sure the tempfile lives till here
        drop(tempfile)
    }
}
