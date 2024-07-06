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

test_repl!(multiline_input() {
    "{ x = 1; " => expect![[""]];
    "y = 2; }" => expect![[r#"
        => { x = 1; y = 2; } :: set
    "#]];
});

test_repl!(bind_literal() {
    "x = 1" => expect![[""]];
    "x" => expect![[r#"
        => 1 :: int
    "#]];
});

test_repl!(bind_lazy() {
    "x = { z = 1; }" => expect![[""]];
    "x" => expect![[r#"
        => { z = 1; } :: set
    "#]];
    "x.z" => expect![[r#"
        => 1 :: int
    "#]];
    "x.z" => expect![[r#"
        => 1 :: int
    "#]];
});

test_repl!(deep_print() {
    "builtins.map (x: x + 1) [ 1 2 3 ]" => expect![[r#"
        => [ <CODE> <CODE> <CODE> ] :: list
    "#]];
    ":p builtins.map (x: x + 1) [ 1 2 3 ]" => expect![[r#"
        => [ 2 3 4 ] :: list
    "#]];
});

test_repl!(explain() {
    ":d { x = 1; y = [ 2 3 4 ]; }" => expect![[r#"
        => a 2-item attribute set
    "#]];
});
