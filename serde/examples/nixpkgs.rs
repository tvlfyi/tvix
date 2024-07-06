//! This program demonstrates deserialising some configuration
//! structure from Nix code that makes use of nixpkgs.lib
//!
//! This example does not add the full set of Nix features (i.e.
//! builds & derivations).

use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Config {
    host: String,
    port: usize,
}

fn main() {
    let code = r#"
    let
       lib = import <nixpkgs/lib>;
       host = lib.strings.concatStringsSep "." ["foo" "example" "com"];
    in {
      inherit host;
      port = 4242;
    }
    "#;

    let result = tvix_serde::from_str_with_config::<Config, _>(code, |eval_builder| {
        eval_builder.enable_impure(None)
    });

    match result {
        Ok(cfg) => println!("Config says: {}:{}", cfg.host, cfg.port),
        Err(e) => eprintln!("{:?} / {}", e, e),
    }
}
