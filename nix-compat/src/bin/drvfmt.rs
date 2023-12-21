use std::{collections::BTreeMap, io::Read};

use nix_compat::derivation::Derivation;
use serde_json::json;

/// construct a serde_json::Value from a Derivation.
/// Some environment values can be non-valid UTF-8 strings.
/// `serde_json` prints them out really unreadable.
/// This is a tool to print A-Terms in a more readable fashion, so we brutally
/// use the [std::string::ToString] implementation of [bstr::BString] to get
/// a UTF-8 string (replacing invalid characters with the Unicode replacement
/// codepoint).
fn build_serde_json_value(drv: Derivation) -> serde_json::Value {
    json!({
        "args": drv.arguments,
        "builder": drv.builder,
        "env":   drv.environment.into_iter().map(|(k,v)| (k, v.to_string())).collect::<BTreeMap<String, String>>(),
        "inputDrvs": drv.input_derivations,
        "inputSrcs": drv.input_sources,
        "outputs": drv.outputs,
        "system": drv.system,
    })
}

fn main() {
    // read A-Term from stdin
    let mut buf = Vec::new();
    std::io::stdin()
        .read_to_end(&mut buf)
        .expect("failed to read from stdin");

    match Derivation::from_aterm_bytes(&buf) {
        Ok(drv) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&build_serde_json_value(drv))
                    .expect("unable to serialize")
            );
        }
        Err(e) => eprintln!("unable to parse derivation: {:#?}", e),
    }
}
