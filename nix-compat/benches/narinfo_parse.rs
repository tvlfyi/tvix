use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use lazy_static::lazy_static;
use nix_compat::narinfo::NarInfo;
use std::{io, str};

const SAMPLE: &str = r#"StorePath: /nix/store/1pajsq519irjy86vli20bgq1wr1q3pny-banking-0.3.0
URL: nar/0rdn027rxqbl42bv9jxhsipgq2hwqdapvwmdzligmzdmz2p9vybs.nar.xz
Compression: xz
FileHash: sha256:0rdn027rxqbl42bv9jxhsipgq2hwqdapvwmdzligmzdmz2p9vybs
FileSize: 92828
NarHash: sha256:0cfnydzp132y69bh20dj76yfd6hc3qdyblbwr9hwn59vfmnb09m0
NarSize: 173352
References: 03d4ncyfh76mgs6sfayl8l6zzdhm219w-python3.9-mt-940-4.23.0 0rhbw783qcjxv3cqln1760i1lmz2yb67-gsettings-desktop-schemas-41.0 1dm9ndgg56ylawpcbdzkhl03fg6777rr-python3.9-six-1.16.0 1pajsq519irjy86vli20bgq1wr1q3pny-banking-0.3.0 2ccy5zc89zpc2aznqxgvzp4wm1bwj05n-bzip2-1.0.6.0.2-bin 32gy3pqk4n725lscdm622yzsg9np3xvs-python3.9-cryptography-36.0.0-dev 35chvqbr7vp9icdki0132fc6np09vrx5-python3.9-bleach-4.1.0 53abh5cz9zi4yh75lfzg99xqy0fdgj4i-python3.9-xmlschema-1.9.2 5p96sifyavb407mnharhyzlw6pn6km1b-glib-2.70.2-bin 6hil8z0zkqcgvaw1qwjyqa8qyaa1lm3k-python3.9-pycairo-1.20.1 803ffb21rv4af521pplb72zjm1ygm9kk-python3.9-pyparsing-2.4.7 al95l8psvmq5di3vdwa75n8w2m0sj2sy-gdk-pixbuf-2.42.6 b09371lq1jjrv43h8jpp82v23igndsn2-python3.9-fints-3.0.1 b53hk557pdk5mq4lv1zrh71a54qazbsm-python3.9-certifi-2021.10.08 bl0cwvwgch92cfsnli4dsah2gxgdickp-gtk+3-3.24.30 cfkq9wi7ypqk26c75dzic5v3nxlzyi58-python3.9-cryptography-36.0.0 cyhg57whqvrx7xf7fvn70dr5836y7zak-python3.9-sepaxml-2.4.1 d810g729g1c4lvp3nv1n3ah6cvpwg7by-cairo-1.16.0-dev dn4fwp0yx6nsa85cr20cwvdmg64xwmcy-python3-3.9.9 dzsj2n0nmq8nv6w0hvy5vb61kim3rzmd-pango-1.50.0 fs6rcnhbjvpxsyw5qiq0q7jx378fjrq7-python3.9-webencodings-0.5.1 g08sxarx191yh2dh0yk2j8icja54aksf-harfbuzz-3.1.2 glanz2lv7m6ak8pql0jcpr3izyp5cxm5-python3.9-pycparser-2.21 gpzx6h0dp5yhcvkfj68zs444ghll7dzm-python3.9-html5lib-1.1 gxyhqkpahahn4h8wbanzfhr1zkxbysid-expat-2.4.2-dev gy3pnc7bpff1h4ylhrivs4cjlvmxl0dk-python3.9-packaging-20.9 hhpqldw0552mf4mjdm2q7zqwy9hpfchd-libpng-apng-1.6.37-dev ig2bdwmplvs6dyg07fdyh006ha768jh1-python3.9-cffi-1.15.0 ij5rm5y6lmqzrwqd1zxckhbii3dg2nq5-glib-2.70.2-dev j5raylzz6fsafbgayyfaydadjl0x22s0-freetype-2.11.1-dev j6w2fbsl49jska4scyr860gz4df9biha-gobject-introspection-1.70.0 jfc99f1hrca6ih6h0n4ax431hjlx96j0-python3.9-brotli-1.0.9 kbazcxnki2qz514rl1plhsj3587hl8bb-python3.9-pysocks-1.7.1 kkljrrrj80fnz59qyfgnv6wvv0cbmpql-libhandy-1.5.0 l82il2lbp757c0smi81qmj4crlcmdz9s-python3.9-pygobject-3.42.0-dev m4zflhr10wz4frhgxqfi43rwvapki1pi-fontconfig-2.13.94-bin mbsc1c7mq15vgfzcdma9fglczih9ncfy-python3.9-chardet-4.0.0 mfvaaf4illpwrflg30cij5x4rncp9jin-python3.9-text-unidecode-1.3 msiv2nkdcaf4gvaf2cfnxcjm66j8mjxz-python3.9-elementpath-2.4.0 nmwapds8fcx22vd30d81va7a7a51ywwx-gettext-0.21 pbfraw351mksnkp2ni9c4rkc9cpp89iv-bash-5.1-p12 r8cbf18vrd54rb4psf3m4zlk5sd2jsv3-python3.9-pygobject-3.42.0 rig6npd9sd45ashf6fxcwgxzm7m4p0l3-python3.9-requests-2.26.0 ryj72ashr27gf4kh0ssgi3zpiv8fxw53-librsvg-2.52.4 s2jjq7rk5yrzlv9lyralzvpixg4p6jh3-atk-2.36.0 w1lsr2i37fr0mp1jya04nwa5nf5dxm2n-python3.9-setuptools-57.2.0 whfykra99ahs814l5hp3q5ps8rwzsf3s-python3.9-brotlicffi-1.0.9.2 wqdmghdvc4s95jgpp13fj5v3xar8mlks-python3.9-charset-normalizer-2.0.8 x1ha2nyji1px0iqknbyhdnvw4icw5h3i-python3.9-idna-3.3 z9560qb4ygbi0352m9pglwhi332cxb1f-python3.9-urllib3-1.26.7
Deriver: 2ch8jx910qk6721mp4yqsmvdfgj5c8ir-banking-0.3.0.drv
Sig: cache.nixos.org-1:xcL67rBZPcdVZudDLpLeddkBa0KaFTw5A0udnaa0axysjrQ6Nvd9p3BLZ4rhKgl52/cKiU3c6aq60L8+IcE5Dw==
"#;

lazy_static! {
    static ref CASES: &'static [&'static str] = {
        let data =
            zstd::decode_all(io::Cursor::new(include_bytes!("../testdata/narinfo.zst"))).unwrap();
        let data = str::from_utf8(Vec::leak(data)).unwrap();
        Vec::leak(
            data.split_inclusive("\n\n")
                .map(|s| s.strip_suffix('\n').unwrap())
                .collect::<Vec<_>>(),
        )
    };
}

pub fn parse(c: &mut Criterion) {
    let mut g = c.benchmark_group("parse");

    {
        g.throughput(Throughput::Bytes(SAMPLE.len() as u64));
        g.bench_with_input("single", SAMPLE, |b, data| {
            b.iter(|| {
                black_box(NarInfo::parse(black_box(data)));
            });
        });
    }

    {
        for &case in *CASES {
            NarInfo::parse(case).expect("should parse");
        }

        g.throughput(Throughput::Bytes(
            CASES.iter().map(|s| s.len() as u64).sum(),
        ));
        g.bench_with_input("many", &*CASES, |b, data| {
            let mut vec = vec![];
            b.iter(|| {
                vec.clear();
                vec.extend(black_box(data).iter().map(|s| NarInfo::parse(s)));
                black_box(&vec);
            });
        });
    }

    g.finish();
}

criterion_group!(benches, parse);
criterion_main!(benches);
