use criterion::{black_box, criterion_group, criterion_main, Criterion};
use lazy_static::lazy_static;
use std::{env, rc::Rc, sync::Arc, time::Duration};
#[cfg(not(target_env = "msvc"))]
use tikv_jemallocator::Jemalloc;
use tvix_build::buildservice::DummyBuildService;
use tvix_eval::{builtins::impure_builtins, EvalIO};
use tvix_glue::{
    builtins::{add_derivation_builtins, add_fetcher_builtins, add_import_builtins},
    configure_nix_path,
    tvix_io::TvixIO,
    tvix_store_io::TvixStoreIO,
};
use tvix_store::utils::construct_services;

#[cfg(not(target_env = "msvc"))]
#[global_allocator]
static GLOBAL: Jemalloc = Jemalloc;

lazy_static! {
    static ref TOKIO_RUNTIME: tokio::runtime::Runtime = tokio::runtime::Runtime::new().unwrap();
}

fn interpret(code: &str) {
    // TODO: this is a bit annoying.
    // It'd be nice if we could set this up once and then run evaluate() with a
    // piece of code. b/262
    let (blob_service, directory_service, path_info_service, nar_calculation_service) =
        TOKIO_RUNTIME
            .block_on(async { construct_services("memory://", "memory://", "memory://").await })
            .unwrap();

    // We assemble a complete store in memory.
    let tvix_store_io = Rc::new(TvixStoreIO::new(
        blob_service,
        directory_service,
        path_info_service.into(),
        nar_calculation_service.into(),
        Arc::<DummyBuildService>::default(),
        TOKIO_RUNTIME.handle().clone(),
    ));

    let mut eval_builder = tvix_eval::Evaluation::builder(Box::new(TvixIO::new(
        tvix_store_io.clone() as Rc<dyn EvalIO>,
    )) as Box<dyn EvalIO>)
    .enable_import()
    .add_builtins(impure_builtins());

    eval_builder = add_derivation_builtins(eval_builder, Rc::clone(&tvix_store_io));
    eval_builder = add_fetcher_builtins(eval_builder, Rc::clone(&tvix_store_io));
    eval_builder = add_import_builtins(eval_builder, tvix_store_io);
    eval_builder = configure_nix_path(
        eval_builder,
        // The benchmark requires TVIX_BENCH_NIX_PATH to be set, so barf out
        // early, rather than benchmarking tvix returning an error.
        &Some(env::var("TVIX_BENCH_NIX_PATH").expect("TVIX_BENCH_NIX_PATH must be set")),
    );

    let eval = eval_builder.build();
    let result = eval.evaluate(code, None);

    assert!(result.errors.is_empty());
}

fn eval_nixpkgs(c: &mut Criterion) {
    c.bench_function("hello outpath", |b| {
        b.iter(|| {
            interpret(black_box("(import <nixpkgs> {}).hello.outPath"));
        })
    });
}

criterion_group!(
    name = benches;
    config = Criterion::default().measurement_time(Duration::from_secs(30)).sample_size(10);
    targets = eval_nixpkgs
);
criterion_main!(benches);
