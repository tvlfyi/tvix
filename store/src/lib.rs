pub mod import;
pub mod nar;
pub mod pathinfoservice;
pub mod proto;
pub mod utils;

#[cfg(test)]
mod tests;

// That's what the rstest_reuse README asks us do, and fails about being unable
// to find rstest_reuse in crate root.
#[cfg(test)]
#[allow(clippy::single_component_path_imports)]
use rstest_reuse;
