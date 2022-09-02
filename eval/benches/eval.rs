use criterion::{black_box, criterion_group, criterion_main, Criterion};
use tvix_eval::interpret;

fn eval_literals(c: &mut Criterion) {
    c.bench_function("int", |b| b.iter(|| black_box(interpret("42", None))));
}

criterion_group!(benches, eval_literals);
criterion_main!(benches);
