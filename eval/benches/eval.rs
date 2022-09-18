use criterion::{black_box, criterion_group, criterion_main, Criterion};
use itertools::Itertools;
use tvix_eval::interpret;

fn eval_literals(c: &mut Criterion) {
    c.bench_function("int", |b| {
        b.iter(|| black_box(interpret("42", None, Default::default())))
    });
}

fn eval_merge_attrs(c: &mut Criterion) {
    c.bench_function("merge small attrs", |b| {
        b.iter(|| {
            black_box(interpret(
                "{ a = 1; b = 2; } // { c = 3; }",
                None,
                Default::default(),
            ))
        })
    });

    c.bench_function("merge large attrs with small attrs", |b| {
        let large_attrs = format!(
            "{{{}}}",
            (0..10000).map(|n| format!("a{n} = {n};")).join(" ")
        );
        let expr = format!("{large_attrs} // {{ c = 3; }}");
        b.iter(move || black_box(interpret(&expr, None, Default::default())))
    });
}

criterion_group!(benches, eval_literals, eval_merge_attrs);
criterion_main!(benches);
