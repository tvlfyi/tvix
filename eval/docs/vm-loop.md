tvix-eval VM loop
=================

Date: 2023-02-14
Author: tazjin

## Background

The VM loop implemented in `src/vm.rs` currently has a couple of functions:

1. Advance the instruction pointer and execute instructions, in a loop
   (unsurprisingly).

2. Tracking Nix call frames as functions/thunks are entered/exited.

3. Catch trampoline requests returned from instruction executions, and
   resuming of executions afterwards.

4. Invoking the inner trampoline loop, handling actions and
   continuations from the trampoline.

The current implementation of the trampoline logic was added on to the
existing VM, which recursed for thunk forcing. As a result, it is
currently a little difficult to understand what exactly is going on in
the VM loop and how the trampoline logic works.

This has also led to several bugs, for example: b/251, b/246, b/245,
and b/238.

These bugs are very tricky to deal with, as we have to try and make
the VM do things that are somewhat difficult to fit into the current
model. We could of course keep extending the trampoline logic to
accomodate all sorts of concepts (such as finalisers), but that seems
difficult.

There are also additional problems, such as forcing inside of builtin
implementations, which leads to a situation where we would like to
suspend the builtin and return control flow to the VM until the value
is forced.

## Proposal

We rewrite parts of the VM loop to elevate trampolining and
potentially other modes of execution to a top-level concept of the VM.

We achieve this by replacing the current concept of call frames with a
"VM context" (naming tbd), which can represent multiple different
states of the VM:

1. Tvix code execution, equivalent to what is currently a call frame,
   executing bytecode until the instruction pointer reaches the end of
   a chunk, then returning one level up.

2. Trampolining the forcing of a thunk, equivalent to the current
   trampolining logic.

3. Waiting for the result of a trampoline, to ensure that in case of
   nested thunks all representations are correctly transformed.

4. Trampolining the execution of a builtin. This is not in scope for
   the initial implementation, but it should be conceptually possible.

It is *not* in scope for this proposal to enable parallel suspension
of thunks. This is a separate topic which is discussed in [waiting for
the store][wfs].

## Alternatives considered

1. Tacking on more functionality onto the existing VM loop
   implementation to accomodate problems as they show up. This is not
   preferred as the code is already getting messy.

2. ... ?


[wfs]: https://docs.google.com/document/d/1Zuw9UdMy95hcqsd-KudTw5yeeUkEXrqOi0rooGB7GWA/edit
