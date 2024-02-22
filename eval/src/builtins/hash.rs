use bstr::ByteSlice;
use data_encoding::HEXLOWER;
use md5::Md5;
use sha1::Sha1;
use sha2::{digest::Output, Digest, Sha256, Sha512};

use crate::ErrorKind;

fn hash<D: Digest>(b: &[u8]) -> Output<D> {
    let mut hasher = D::new();
    hasher.update(b);
    hasher.finalize()
}

pub fn hash_nix_string(algo: impl AsRef<[u8]>, s: impl AsRef<[u8]>) -> Result<String, ErrorKind> {
    match algo.as_ref() {
        b"md5" => Ok(HEXLOWER.encode(hash::<Md5>(s.as_ref()).as_bstr())),
        b"sha1" => Ok(HEXLOWER.encode(hash::<Sha1>(s.as_ref()).as_bstr())),
        b"sha256" => Ok(HEXLOWER.encode(hash::<Sha256>(s.as_ref()).as_bstr())),
        b"sha512" => Ok(HEXLOWER.encode(hash::<Sha512>(s.as_ref()).as_bstr())),
        _ => Err(ErrorKind::UnknownHashType(
            algo.as_ref().as_bstr().to_string(),
        )),
    }
}
