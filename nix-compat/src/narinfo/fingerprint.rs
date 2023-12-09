use crate::{nixbase32, store_path::StorePathRef};

/// Computes the fingerprint string for certain fields in a [NarInfo].
/// This fingerprint is signed by an ed25519 key, and in the case of a Nix HTTP
/// Binary cache, included in the NARInfo files served from there.
pub fn fingerprint<'a, R: Iterator<Item = &'a StorePathRef<'a>>>(
    store_path: &StorePathRef,
    nar_sha256: &[u8; 32],
    nar_size: u64,
    references: R,
) -> String {
    format!(
        "1;{};sha256:{};{};{}",
        store_path.to_owned().to_absolute_path(), // TODO: move to StorePathRef
        nixbase32::encode(nar_sha256),
        nar_size,
        // references are absolute paths, joined with `,`.
        references
            .map(|r| r.to_owned().to_absolute_path())
            .collect::<Vec<String>>()
            .join(",")
    )
}

#[cfg(test)]
mod tests {
    use crate::narinfo::NarInfo;

    const NARINFO_STR: &str = r#"StorePath: /nix/store/syd87l2rxw8cbsxmxl853h0r6pdwhwjr-curl-7.82.0-bin
URL: nar/05ra3y72i3qjri7xskf9qj8kb29r6naqy1sqpbs3azi3xcigmj56.nar.xz
Compression: xz
FileHash: sha256:05ra3y72i3qjri7xskf9qj8kb29r6naqy1sqpbs3azi3xcigmj56
FileSize: 68852
NarHash: sha256:1b4sb93wp679q4zx9k1ignby1yna3z7c4c2ri3wphylbc2dwsys0
NarSize: 196040
References: 0jqd0rlxzra1rs38rdxl43yh6rxchgc6-curl-7.82.0 6w8g7njm4mck5dmjxws0z1xnrxvl81xa-glibc-2.34-115 j5jxw3iy7bbz4a57fh9g2xm2gxmyal8h-zlib-1.2.12 yxvjs9drzsphm9pcf42a4byzj1kb9m7k-openssl-1.1.1n
Deriver: 5rwxzi7pal3qhpsyfc16gzkh939q1np6-curl-7.82.0.drv
Sig: cache.nixos.org-1:TsTTb3WGTZKphvYdBHXwo6weVILmTytUjLB+vcX89fOjjRicCHmKA4RCPMVLkj6TMJ4GMX3HPVWRdD1hkeKZBQ==
Sig: test1:519iiVLx/c4Rdt5DNt6Y2Jm6hcWE9+XY69ygiWSZCNGVcmOcyL64uVAJ3cV8vaTusIZdbTnYo9Y7vDNeTmmMBQ==
"#;

    #[test]
    fn fingerprint() {
        let parsed = NarInfo::parse(NARINFO_STR).expect("must parse");
        assert_eq!(
            "1;/nix/store/syd87l2rxw8cbsxmxl853h0r6pdwhwjr-curl-7.82.0-bin;sha256:1b4sb93wp679q4zx9k1ignby1yna3z7c4c2ri3wphylbc2dwsys0;196040;/nix/store/0jqd0rlxzra1rs38rdxl43yh6rxchgc6-curl-7.82.0,/nix/store/6w8g7njm4mck5dmjxws0z1xnrxvl81xa-glibc-2.34-115,/nix/store/j5jxw3iy7bbz4a57fh9g2xm2gxmyal8h-zlib-1.2.12,/nix/store/yxvjs9drzsphm9pcf42a4byzj1kb9m7k-openssl-1.1.1n",
            parsed.fingerprint()
        );
    }
}
