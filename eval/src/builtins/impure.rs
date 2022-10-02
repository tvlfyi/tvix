use std::{
    collections::BTreeMap,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{
    value::{Builtin, NixString},
    Value,
};

fn impure_builtins() -> Vec<Builtin> {
    vec![]
}

/// Return all impure builtins, that is all builtins which may perform I/O outside of the VM and so
/// cannot be used in all contexts (e.g. WASM).
pub(super) fn builtins() -> BTreeMap<NixString, Value> {
    let mut map: BTreeMap<NixString, Value> = impure_builtins()
        .into_iter()
        .map(|b| (b.name().into(), Value::Builtin(b)))
        .collect();

    // currentTime pins the time at which evaluation was started
    {
        let seconds = match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(dur) => dur.as_secs() as i64,

            // This case is hit if the system time is *before* epoch.
            Err(err) => -(err.duration().as_secs() as i64),
        };

        map.insert(NixString::from("currentTime"), Value::Integer(seconds));
    }

    map
}
