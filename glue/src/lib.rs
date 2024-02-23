pub mod builtins;
pub mod known_paths;
pub mod refscan;
pub mod tvix_build;
pub mod tvix_io;
pub mod tvix_store_io;

mod decompression;
#[cfg(test)]
mod tests;

/// Tell the Evaluator to resolve `<nix>` to the path `/__corepkgs__`,
/// which has special handling in [tvix_io::TvixIO].
/// This is used in nixpkgs to import `fetchurl.nix` from `<nix>`.
pub fn configure_nix_path<IO>(
    eval: &mut tvix_eval::Evaluation<IO>,
    nix_search_path: &Option<String>,
) {
    eval.nix_path = nix_search_path
        .as_ref()
        .map(|p| format!("nix=/__corepkgs__:{}", p))
        .or_else(|| Some("nix=/__corepkgs__".to_string()));
}
