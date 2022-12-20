/// true iff the argument is recognized by cppnix as the second
/// coordinate of a "nix double"
fn is_second_coordinate(x: &str) -> bool {
    matches!(x, "linux" | "darwin" | "netbsd" | "openbsd" | "freebsd")
}

/// This function takes an llvm triple (which may have three or four
/// components, separated by dashes) and returns the "best"
/// approximation as a nix double, where "best" is currently defined
/// as "however cppnix handles it".
pub fn llvm_triple_to_nix_double(llvm_triple: &str) -> String {
    let parts: Vec<&str> = llvm_triple.split('-').collect();
    let cpu = match parts[0] {
        "armv6" => "armv6l", // cppnix appends an "l" to armv6
        "armv7" => "armv7l", // cppnix appends an "l" to armv7
        x => match x.as_bytes() {
            [b'i', _, b'8', b'6'] => "i686", // cppnix glob-matches against i*86
            _ => x,
        },
    };
    let os = match parts[1..] {
        [_vendor, kernel, _environment] if is_second_coordinate(kernel) => kernel,
        [_vendor, kernel] if is_second_coordinate(kernel) => kernel,
        [kernel, _environment] if is_second_coordinate(kernel) => kernel,

        // Rustc uses wasm32-unknown-unknown, which is rejected by
        // config.sub, for wasm-in-the-browser environments.  Rustc
        // should be using wasm32-unknown-none, which config.sub
        // accepts.  Hopefully the rustc people will change their
        // triple before stabilising this triple.  In the meantime,
        // we fix it here in order to unbreak tvixbolt.
        //
        // https://doc.rust-lang.org/beta/nightly-rustc/rustc_target/spec/wasm32_unknown_unknown/index.html
        ["unknown", "unknown"] if cpu == "wasm32" => "none",

        _ => panic!("unrecognized triple {llvm_triple}"),
    };
    format!("{cpu}-{os}")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_systems() {
        assert_eq!(
            llvm_triple_to_nix_double("aarch64-unknown-linux-gnu"),
            "aarch64-linux"
        );
        assert_eq!(
            llvm_triple_to_nix_double("i686-unknown-linux-gnu"),
            "i686-linux"
        );
        assert_eq!(
            llvm_triple_to_nix_double("x86_64-apple-darwin"),
            "x86_64-darwin"
        );
        assert_eq!(
            llvm_triple_to_nix_double("x86_64-unknown-linux-gnu"),
            "x86_64-linux"
        );
        assert_eq!(
            llvm_triple_to_nix_double("aarch64-apple-darwin"),
            "aarch64-darwin"
        );
        assert_eq!(
            llvm_triple_to_nix_double("aarch64-unknown-linux-musl"),
            "aarch64-linux"
        );
        assert_eq!(
            llvm_triple_to_nix_double("arm-unknown-linux-gnueabi"),
            "arm-linux"
        );
        assert_eq!(
            llvm_triple_to_nix_double("arm-unknown-linux-gnueabihf"),
            "arm-linux"
        );
        assert_eq!(
            llvm_triple_to_nix_double("armv7-unknown-linux-gnueabihf"),
            "armv7l-linux"
        );
        assert_eq!(
            llvm_triple_to_nix_double("mips-unknown-linux-gnu"),
            "mips-linux"
        );
        assert_eq!(
            llvm_triple_to_nix_double("mips64-unknown-linux-gnuabi64"),
            "mips64-linux"
        );
        assert_eq!(
            llvm_triple_to_nix_double("mips64-unknown-linux-gnuabin32"),
            "mips64-linux"
        );
        assert_eq!(
            llvm_triple_to_nix_double("mips64el-unknown-linux-gnuabi64"),
            "mips64el-linux"
        );
        assert_eq!(
            llvm_triple_to_nix_double("mips64el-unknown-linux-gnuabin32"),
            "mips64el-linux"
        );
        assert_eq!(
            llvm_triple_to_nix_double("mipsel-unknown-linux-gnu"),
            "mipsel-linux"
        );
        assert_eq!(
            llvm_triple_to_nix_double("powerpc-unknown-linux-gnu"),
            "powerpc-linux"
        );
        assert_eq!(
            llvm_triple_to_nix_double("powerpc64-unknown-linux-gnu"),
            "powerpc64-linux"
        );
        assert_eq!(
            llvm_triple_to_nix_double("powerpc64le-unknown-linux-gnu"),
            "powerpc64le-linux"
        );
        assert_eq!(
            llvm_triple_to_nix_double("s390x-unknown-linux-gnu"),
            "s390x-linux"
        );
        assert_eq!(
            llvm_triple_to_nix_double("x86_64-unknown-linux-musl"),
            "x86_64-linux"
        );
        assert_eq!(
            llvm_triple_to_nix_double("x86_64-unknown-netbsd"),
            "x86_64-netbsd"
        );
        assert_eq!(
            llvm_triple_to_nix_double("aarch64-linux-android"),
            "aarch64-linux"
        );
        assert_eq!(
            llvm_triple_to_nix_double("arm-linux-androideabi"),
            "arm-linux"
        );
        assert_eq!(
            llvm_triple_to_nix_double("arm-unknown-linux-musleabi"),
            "arm-linux"
        );
        assert_eq!(
            llvm_triple_to_nix_double("arm-unknown-linux-musleabihf"),
            "arm-linux"
        );
        assert_eq!(
            llvm_triple_to_nix_double("armv5te-unknown-linux-gnueabi"),
            "armv5te-linux"
        );
        assert_eq!(
            llvm_triple_to_nix_double("armv5te-unknown-linux-musleabi"),
            "armv5te-linux"
        );
        assert_eq!(
            llvm_triple_to_nix_double("armv7-linux-androideabi"),
            "armv7l-linux"
        );
        assert_eq!(
            llvm_triple_to_nix_double("armv7-unknown-linux-gnueabi"),
            "armv7l-linux"
        );
        assert_eq!(
            llvm_triple_to_nix_double("armv7-unknown-linux-musleabi"),
            "armv7l-linux"
        );
        assert_eq!(
            llvm_triple_to_nix_double("armv7-unknown-linux-musleabihf"),
            "armv7l-linux"
        );
        assert_eq!(
            llvm_triple_to_nix_double("i586-unknown-linux-gnu"),
            "i686-linux"
        );
        assert_eq!(
            llvm_triple_to_nix_double("i586-unknown-linux-musl"),
            "i686-linux"
        );
        assert_eq!(
            llvm_triple_to_nix_double("i686-linux-android"),
            "i686-linux"
        );
        assert_eq!(
            llvm_triple_to_nix_double("i686-unknown-linux-musl"),
            "i686-linux"
        );
        assert_eq!(
            llvm_triple_to_nix_double("mips-unknown-linux-musl"),
            "mips-linux"
        );
        assert_eq!(
            llvm_triple_to_nix_double("mips64-unknown-linux-muslabi64"),
            "mips64-linux"
        );
        assert_eq!(
            llvm_triple_to_nix_double("mips64el-unknown-linux-muslabi64"),
            "mips64el-linux"
        );
        assert_eq!(
            llvm_triple_to_nix_double("mipsel-unknown-linux-musl"),
            "mipsel-linux"
        );
        assert_eq!(
            llvm_triple_to_nix_double("sparc64-unknown-linux-gnu"),
            "sparc64-linux"
        );
        assert_eq!(
            llvm_triple_to_nix_double("thumbv7neon-linux-androideabi"),
            "thumbv7neon-linux"
        );
        assert_eq!(
            llvm_triple_to_nix_double("thumbv7neon-unknown-linux-gnueabihf"),
            "thumbv7neon-linux"
        );
        assert_eq!(
            llvm_triple_to_nix_double("x86_64-linux-android"),
            "x86_64-linux"
        );
        assert_eq!(
            llvm_triple_to_nix_double("x86_64-unknown-linux-gnux32"),
            "x86_64-linux"
        );
        assert_eq!(
            llvm_triple_to_nix_double("aarch64-unknown-linux-gnu_ilp32"),
            "aarch64-linux"
        );
        assert_eq!(
            llvm_triple_to_nix_double("aarch64-unknown-netbsd"),
            "aarch64-netbsd"
        );
        assert_eq!(
            llvm_triple_to_nix_double("aarch64_be-unknown-linux-gnu_ilp32"),
            "aarch64_be-linux"
        );
        assert_eq!(
            llvm_triple_to_nix_double("aarch64_be-unknown-linux-gnu"),
            "aarch64_be-linux"
        );
        assert_eq!(
            llvm_triple_to_nix_double("armeb-unknown-linux-gnueabi"),
            "armeb-linux"
        );
        assert_eq!(
            llvm_triple_to_nix_double("armv4t-unknown-linux-gnueabi"),
            "armv4t-linux"
        );
        assert_eq!(
            llvm_triple_to_nix_double("armv6-unknown-netbsd-eabihf"),
            "armv6l-netbsd"
        );
        assert_eq!(
            llvm_triple_to_nix_double("armv7-unknown-linux-uclibceabi"),
            "armv7l-linux"
        );
        assert_eq!(
            llvm_triple_to_nix_double("armv7-unknown-linux-uclibceabihf"),
            "armv7l-linux"
        );
        assert_eq!(
            llvm_triple_to_nix_double("armv7-unknown-netbsd-eabihf"),
            "armv7l-netbsd"
        );
        assert_eq!(
            llvm_triple_to_nix_double("hexagon-unknown-linux-musl"),
            "hexagon-linux"
        );
        assert_eq!(
            llvm_triple_to_nix_double("i686-unknown-netbsd"),
            "i686-netbsd"
        );
        assert_eq!(
            llvm_triple_to_nix_double("m68k-unknown-linux-gnu"),
            "m68k-linux"
        );
        assert_eq!(
            llvm_triple_to_nix_double("mips-unknown-linux-uclibc"),
            "mips-linux"
        );
        assert_eq!(
            llvm_triple_to_nix_double("mips64-openwrt-linux-musl"),
            "mips64-linux"
        );
        assert_eq!(
            llvm_triple_to_nix_double("mipsel-unknown-linux-uclibc"),
            "mipsel-linux"
        );
        assert_eq!(
            llvm_triple_to_nix_double("mipsisa32r6-unknown-linux-gnu"),
            "mipsisa32r6-linux"
        );
        assert_eq!(
            llvm_triple_to_nix_double("mipsisa32r6el-unknown-linux-gnu"),
            "mipsisa32r6el-linux"
        );
        assert_eq!(
            llvm_triple_to_nix_double("mipsisa64r6-unknown-linux-gnuabi64"),
            "mipsisa64r6-linux"
        );
        assert_eq!(
            llvm_triple_to_nix_double("mipsisa64r6el-unknown-linux-gnuabi64"),
            "mipsisa64r6el-linux"
        );
        assert_eq!(
            llvm_triple_to_nix_double("powerpc-unknown-linux-gnuspe"),
            "powerpc-linux"
        );
        assert_eq!(
            llvm_triple_to_nix_double("powerpc-unknown-linux-musl"),
            "powerpc-linux"
        );
        assert_eq!(
            llvm_triple_to_nix_double("powerpc-unknown-netbsd"),
            "powerpc-netbsd"
        );
        assert_eq!(
            llvm_triple_to_nix_double("powerpc64-unknown-linux-musl"),
            "powerpc64-linux"
        );
        assert_eq!(
            llvm_triple_to_nix_double("powerpc64le-unknown-linux-musl"),
            "powerpc64le-linux"
        );
        assert_eq!(
            llvm_triple_to_nix_double("riscv32gc-unknown-linux-gnu"),
            "riscv32gc-linux"
        );
        assert_eq!(
            llvm_triple_to_nix_double("riscv32gc-unknown-linux-musl"),
            "riscv32gc-linux"
        );
        assert_eq!(
            llvm_triple_to_nix_double("riscv64gc-unknown-linux-musl"),
            "riscv64gc-linux"
        );
        assert_eq!(
            llvm_triple_to_nix_double("s390x-unknown-linux-musl"),
            "s390x-linux"
        );
        assert_eq!(
            llvm_triple_to_nix_double("sparc-unknown-linux-gnu"),
            "sparc-linux"
        );
        assert_eq!(
            llvm_triple_to_nix_double("sparc64-unknown-netbsd"),
            "sparc64-netbsd"
        );
        assert_eq!(
            llvm_triple_to_nix_double("thumbv7neon-unknown-linux-musleabihf"),
            "thumbv7neon-linux"
        );
    }
}
