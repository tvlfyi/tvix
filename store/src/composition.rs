use std::sync::LazyLock;

pub use tvix_castore::composition::*;

/// The provided registry of tvix_store, which has all the builtin
/// tvix_castore (BlobStore/DirectoryStore) and tvix_store
/// (PathInfoService) implementations.
pub static REG: LazyLock<&'static Registry> = LazyLock::new(|| {
    let mut reg = Default::default();
    add_default_services(&mut reg);
    // explicitly leak to get an &'static, so that we gain `&Registry: Send` from `Registry: Sync`
    Box::leak(Box::new(reg))
});

/// Register the builtin services of tvix_castore and tvix_store with the given
/// registry. This is useful for creating your own registry with the builtin
/// types _and_ extra third party types.
pub fn add_default_services(reg: &mut Registry) {
    tvix_castore::composition::add_default_services(reg);
    crate::pathinfoservice::register_pathinfo_services(reg);
}
