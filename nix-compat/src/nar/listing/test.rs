use std::{collections::HashMap, path::PathBuf, str::FromStr};

use crate::nar;

#[test]
fn weird_paths() {
    let root = nar::listing::ListingEntry::Directory {
        entries: HashMap::new(),
    };

    root.locate("../../../../etc/passwd")
        .expect_err("Failed to reject `../` fragment in a path during traversal");

    // Gated on Windows as C:\\ is parsed as `Component::Normal(_)` on Linux.
    #[cfg(target_os = "windows")]
    root.locate("C:\\\\Windows\\System32")
        .expect_err("Failed to reject Windows-style prefixes");

    root.locate("/etc/passwd")
        .expect_err("Failed to reject absolute UNIX paths");
}

#[test]
fn nixos_release() {
    let listing_bytes = include_bytes!("../tests/nixos-release.ls");
    let listing: nar::listing::Listing = serde_json::from_slice(listing_bytes).unwrap();

    let nar::listing::Listing::V1 { root, .. } = listing;
    assert!(matches!(root, nar::listing::ListingEntry::Directory { .. }));

    let build_products = root
        .locate(PathBuf::from_str("nix-support/hydra-build-products").unwrap())
        .expect("Failed to locate a known file in a directory")
        .expect("File was unexpectedly not found in the listing");

    assert!(matches!(
        build_products,
        nar::listing::ListingEntry::Regular { .. }
    ));

    let nonexisting_file = root
        .locate(PathBuf::from_str("nix-support/does-not-exist").unwrap())
        .expect("Failed to locate an unknown file in a directory");

    assert!(
        nonexisting_file.is_none(),
        "Non-existing file was unexpectedly found in the listing"
    );

    let existing_dir = root
        .locate(PathBuf::from_str("nix-support").unwrap())
        .expect("Failed to locate a known directory in a directory")
        .expect("Directory was expectedly found in the listing");

    assert!(matches!(
        existing_dir,
        nar::listing::ListingEntry::Directory { .. }
    ));
}
