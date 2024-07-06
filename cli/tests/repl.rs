use std::ffi::OsString;

use clap::Parser;
use expect_test::expect;
use tvix_cli::init_io_handle;

macro_rules! test_repl {
    ($name:ident() {$($send:expr => $expect:expr;)*}) => {
        #[test]
        fn $name() {
            let tokio_runtime = tokio::runtime::Runtime::new().unwrap();
            let args = tvix_cli::Args::parse_from(Vec::<OsString>::new());
            let mut repl = tvix_cli::Repl::new(init_io_handle(&tokio_runtime, &args), &args);
            $({
                let result = repl.send($send.into());
                $expect.assert_eq(result.output())
                ;
            })*
        }
    }
}

test_repl!(simple_expr_eval() {
    "1" => expect![[r#"
        => 1 :: int
    "#]];
});
