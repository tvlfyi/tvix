use crate::value::Builtin;

/// Return all impure builtins, that is all builtins which may perform I/O outside of the VM and so
/// cannot be used in all contexts (e.g. WASM).
pub(super) fn builtins() -> Vec<Builtin> {
    vec![]
}
