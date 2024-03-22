use bstr::ByteSlice;
use data_encoding::HEXLOWER;
use md5::Md5;
use sha1::Sha1;
use sha2::{digest::Output, Digest, Sha256, Sha512};

use crate::ErrorKind;

/// Reads through all data from the passed reader, and returns the resulting [Digest].
/// The exact hash function used is left generic over all [Digest].
fn hash<D: Digest + std::io::Write>(mut r: impl std::io::Read) -> Result<Output<D>, ErrorKind> {
    let mut hasher = D::new();
    std::io::copy(&mut r, &mut hasher)?;
    Ok(hasher.finalize())
}

/// For a given algo "string" and reader for data, calculate the digest
/// and return it as a hexlower encoded [String].
pub fn hash_nix_string(algo: impl AsRef<[u8]>, s: impl std::io::Read) -> Result<String, ErrorKind> {
    match algo.as_ref() {
        b"md5" => Ok(HEXLOWER.encode(hash::<Md5>(s)?.as_bstr())),
        b"sha1" => Ok(HEXLOWER.encode(hash::<Sha1>(s)?.as_bstr())),
        b"sha256" => Ok(HEXLOWER.encode(hash::<Sha256>(s)?.as_bstr())),
        b"sha512" => Ok(HEXLOWER.encode(hash::<Sha512>(s)?.as_bstr())),
        _ => Err(ErrorKind::UnknownHashType(
            algo.as_ref().as_bstr().to_string(),
        )),
    }
}
