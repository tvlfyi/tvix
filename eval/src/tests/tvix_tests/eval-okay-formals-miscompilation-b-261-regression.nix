# This is a regression test for https://b.tvl.fyi/261.
#
# The bug occurred when Tvix would unconditionally finalise the stack slot of
# `finalise` (as its default expression needs a finaliser): Finalising an
# manually provided, already forced thunk would cause the VM to crash.
let
  thunk = x: x;
  bomb = thunk true;
  f =
    { finalise ? later == null
    , later ? null
    }:
    [ finalise later ];
in

# Note that the crash did not occur if the offending expression was the rhs
# argument to `builtins.seq`, hence we need to put the assert in between.
assert builtins.seq bomb true;

f { finalise = bomb; }
