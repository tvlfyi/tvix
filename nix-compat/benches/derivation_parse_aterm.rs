use std::path::Path;

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use nix_compat::derivation::Derivation;

const RESOURCES_PATHS: &str = "src/derivation/tests/derivation_tests/ok";

fn bench_aterm_parser(c: &mut Criterion) {
    for drv in [
        "0hm2f1psjpcwg8fijsmr4wwxrx59s092-bar.drv",
        "292w8yzv5nn7nhdpxcs8b7vby2p27s09-nested-json.drv",
        "4wvvbi4jwn0prsdxb7vs673qa5h9gr7x-foo.drv",
        "52a9id8hx688hvlnz4d1n25ml1jdykz0-unicode.drv",
        "9lj1lkjm2ag622mh4h9rpy6j607an8g2-structured-attrs.drv",
        "ch49594n9avinrf8ip0aslidkc4lxkqv-foo.drv",
        "h32dahq0bx5rp1krcdx3a53asj21jvhk-has-multi-out.drv",
        "m1vfixn8iprlf0v9abmlrz7mjw1xj8kp-cp1252.drv",
        "ss2p4wmxijn652haqyd7dckxwl4c7hxx-bar.drv",
        "x6p0hg79i3wg0kkv7699935f7rrj9jf3-latin1.drv",
    ] {
        let drv_path = Path::new(RESOURCES_PATHS).join(drv);
        let drv_bytes = &std::fs::read(drv_path).unwrap();

        c.bench_function(drv, |b| {
            b.iter(|| Derivation::from_aterm_bytes(black_box(drv_bytes)))
        });
    }
}

criterion_group!(benches, bench_aterm_parser);
criterion_main!(benches);
