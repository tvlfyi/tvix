tvix-eval VM loop
=================

This document describes the new tvix-eval VM execution loop implemented in the
chain focusing around cl/8104.

## Background

The VM loop implemented in Tvix prior to cl/8104 had several functions:

1. Advancing the instruction pointer for a chunk of Tvix bytecode and
   executing instructions in a loop until a result was yielded.

2. Tracking Nix call frames as functions/thunks were entered/exited.

3. Catching trampoline requests returned from instructions to force suspended
   thunks without increasing stack size *where possible*.

4. Handling trampolines through an inner trampoline loop, switching between a
   code execution mode and execution of subsequent trampolines.

This implementation of the trampoline logic was added on to the existing VM,
which previously always recursed for thunk forcing. There are some cases (for
example values that need to be forced *inside* of the execution of a builtin)
where trampolines could not previously be used, and the VM recursed anyways.

As a result of this trampoline logic being added "on top" of the existing VM
loop the code became quite difficult to understand. This led to several bugs,
for example: b/251, b/246, b/245, and b/238.

These bugs were tricky to deal with, as we had to try and make the VM do
things that are somewhat difficult to fit into its model. We could of course
keep extending the trampoline logic to accommodate all sorts of concepts (such
as finalisers), but that seems like it does not solve the root problem.

## New VM loop

In cl/8104, a unified new solution is implemented with which the VM is capable
of evaluating everything without increasing the call stack size.

This is done by introducing a new frame stack in the VM, on which execution
frames are enqueued that are either:

1. A bytecode frame, consisting of Tvix bytecode that evaluates compiled Nix
   code.
2. A generator frame, consisting of some VM logic implemented in pure Rust
   code that can be *suspended* when it hits a point where the VM would
   previously need to recurse.

We do this by making use of the `async` *keyword* in Rust, but notably
*without* introducing asynchronous I/O or concurrency in tvix-eval (the
complexity of which is currently undesirable for us).

Specifically, when writing a Rust function that uses the `async` keyword, such
as:

```rust
async fn some_builtin(input: Value) -> Result<Value, ErrorKind> {
  let mut out = NixList::new();

  for element in input.to_list()? {
    let result = do_something_that_requires_the_vm(element).await;
    out.push(result);
  }

  Ok(out)
}
```

The compiler actually generates a state-machine under-the-hood which allows
the execution of that function to be *suspended* whenever it hits an `await`.

We use the [`genawaiter`][] crate that gives us a data structure and simple
interface for getting instances of these state machines that can be stored in
a struct (in our case, a *generator frame*).

The execution of the VM then becomes the execution of an *outer loop*, which
is responsible for selecting the next generator frame to execute, and two
*inner loops*, which drive the execution of a bytecode frame or generator
frame forward until it either yields a value or asks to be suspended in favour
of another frame.

All "communication" between frames happens solely through values left on the
stack: Whenever a frame of either type runs to completion, it is expected to
leave a *single* value on the stack. It follows that the whole VM, upon
completion of the last (or initial, depending on your perspective) frame
yields its result as the return value.

The core of the VM restructuring is cl/8104, unfortunately one of the largest
single commit changes we've had to make yet, as it touches pretty much all
areas of tvix-eval. The introduction of the generators and the
message/response system we built to request something from the VM, suspend a
generator, and wait for the return is in cl/8148.

The next sections describe in detail how the three different loops work.

### Outer VM loop

The outer VM loop is responsible for selecting the next frame to run, and
dispatching it correctly to inner loops, as well as determining when to shut
down the VM and return the final result.

```
                          ╭──────────────────╮
                 ╭────────┤ match frame kind ├──────╮
                 │        ╰──────────────────╯      │
                 │                                  │
    ┏━━━━━━━━━━━━┷━━━━━┓                ╭───────────┴───────────╮
───►┃ frame_stack.pop()┃                ▼                       ▼
    ┗━━━━━━━━━━━━━━━━━━┛       ┏━━━━━━━━━━━━━━━━┓      ┏━━━━━━━━━━━━━━━━━┓
                 ▲             ┃ bytecode frame ┃      ┃ generator frame ┃
                 │             ┗━━━━━━━━┯━━━━━━━┛      ┗━━━━━━━━┯━━━━━━━━┛
                 │[yes, cont.]          │                       │
                 │                      ▼                       ▼
    ┏━━━━━━━━┓   │             ╔════════════════╗      ╔═════════════════╗
◄───┨ return ┃   │             ║ inner bytecode ║      ║ inner generator ║
    ┗━━━━━━━━┛   │             ║      loop      ║      ║      loop       ║
        ▲        │             ╚════════╤═══════╝      ╚════════╤════════╝
        │   ╭────┴─────╮                │                       │
        │   │ has next │                ╰───────────┬───────────╯
   [no] ╰───┤  frame?  │                            │
            ╰────┬─────╯                            ▼
                 │                         ┏━━━━━━━━━━━━━━━━━┓
                 │                         ┃ frame completed ┃
                 ╰─────────────────────────┨  or suspended   ┃
                                           ┗━━━━━━━━━━━━━━━━━┛
```

Initially, the VM always pops a frame from the frame stack and then inspects
the type of frame it found. As a consequence the next frame to execute is
always the frame at the top of the stack, and setting up a VM initially for
code execution is done by leaving a bytecode frame with the code to execute on
the stack and passing control to the outer loop.

Control is dispatched to either of the inner loops (depending on the type of
frame) and the cycle continues once they return.

When an inner loop returns, it has either finished its execution (and left its
result value on the *value stack*), or its frame has requested to be
suspended.

Frames request suspension by re-enqueueing *themselves* through VM helper
methods, and then leaving the frame they want to run *on top* of themselves in
the frame stack before yielding control back to the outer loop.

The inner control loops inform the outer loops about whether the frame has
been *completed* or *suspended* by returning a boolean.

### Inner bytecode loop

The inner bytecode loop drives the execution of some Tvix bytecode by
continously looking at the next instruction to execute, and dispatching to the
instruction handler.

```
   ┏━━━━━━━━━━━━━┓
◄──┨ return true ┃
   ┗━━━━━━━━━━━━━┛
          ▲
     ╔════╧═════╗
     ║ OpReturn ║
     ╚══════════╝
          ▲
          ╰──┬────────────────────────────╮
             │                            ▼
             │                 ╔═════════════════════╗
    ┏━━━━━━━━┷━━━━━┓           ║ execute instruction ║
───►┃ inspect next ┃           ╚══════════╤══════════╝
    ┃  instruction ┃                      │
    ┗━━━━━━━━━━━━━━┛                      │
             ▲                      ╭─────┴─────╮
             ╰──────────────────────┤ suspends? │
                       [no]         ╰─────┬─────╯
                                          │
                                          │
   ┏━━━━━━━━━━━━━━┓                       │
◄──┨ return false ┃───────────────────────╯
   ┗━━━━━━━━━━━━━━┛              [yes]
```

With this refactoring, the compiler now emits a special `OpReturn` instruction
at the end of bytecode chunks. This is a signal to the runtime that the chunk
has completed and that its current value should be returned, without having to
perform instruction pointer arithmetic.

When `OpReturn` is encountered, the inner bytecode loop returns control to the
outer loop and informs it (by returning `true`) that the bytecode frame has
completed.

Any other instruction may also request a suspension of the bytecode frame (for
example, instructions that need to force a value). In this case the inner loop
is responsible for setting up the frame stack correctly, and returning `false`
to inform the outer loop of the suspension

### Inner generator loop

The inner generator loop is responsible for driving the execution of a
generator frame by continously calling [`Gen::resume`][] until it requests a
suspension (as a result of which control is returned to the outer loop), or
until the generator is done and yields a value.

```
   ┏━━━━━━━━━━━━━┓
◄──┨ return true ┃ ◄───────────────────╮
   ┗━━━━━━━━━━━━━┛                     │
                                       │
                               [Done]  │
                    ╭──────────────────┴─────────╮
                    │ inspect generator response │◄────────────╮
                    ╰──────────────────┬─────────╯             │
                            [yielded]  │              ┏━━━━━━━━┷━━━━━━━━┓
                                       │              ┃ gen.resume(msg) ┃◄──
                                       ▼              ┗━━━━━━━━━━━━━━━━━┛
                                 ╭────────────╮                ▲
                                 │ same-frame │                │
                                 │  request?  ├────────────────╯
                                 ╰─────┬──────╯      [yes]
   ┏━━━━━━━━━━━━━━┓                    │
◄──┨ return false ┃ ◄──────────────────╯
   ┗━━━━━━━━━━━━━━┛                [no]
```

On each execution of a generator frame, `resume_with` is called with a
[`VMResponse`][] (i.e. a message *from* the VM *to* the generator). For a newly
created generator, the initial message is just `Empty`.

A generator may then respond by signaling that it has finished execution
(`Done`), in which case the inner generator loop returns control to the outer
loop and informs it that this generator is done (by returning `true`).

A generator may also respond by signaling that it needs some data from the VM.
This is implemented through a request-response pattern, in which the generator
returns a `Yielded` message containing a [`VMRequest`][]. These requests can be
very simple ("Tell me the current store path") or more complex ("Call this Nix
function with these values").

Requests are divided into two classes: Same-frame requests (requests that can be
responded to *without* returning control to the outer loop, i.e. without
executing a *different* frame), and multi-frame generator requests. Based on the
type of request, the inner generator loop will either handle it right away and
send the response in a new `resume_with` call, or return `false` to the outer
generator loop after setting up the frame stack.

Most of this logic is implemented in cl/8148.

[`Gen::resume`]: https://docs.rs/genawaiter/0.99.1/genawaiter/rc/struct.Gen.html#method.resume_with
[`VMRequest`]: https://cs.tvl.fyi/depot@2696839770c1ccb62929ff2575a633c07f5c9593/-/blob/tvix/eval/src/vm/generators.rs?L44
[`VMResponse`]: https://cs.tvl.fyi/depot@2696839770c1ccb62929ff2575a633c07f5c9593/-/blob/tvix/eval/src/vm/generators.rs?L169

## Advantages & Disadvantages of the approach

This approach has several advantages:

* The execution model is much simpler than before, making it fairly
  straightforward to build up a mental model of what the VM does.

* All "out of band requests" inside the VM are handled through the same
  abstraction (generators).

* Implementation is not difficult, albeit a little verbose in some cases (we
  can argue about whether or not to introduce macros for simplifying it).

* Several parts of the VM execution are now much easier to document,
  potentially letting us onboard tvix-eval contributors faster.

* The linear VM execution itself is much easier to trace now, with for example
  the `RuntimeObserver` (and by extension `tvixbolt`) giving much clearer
  output now.

But it also comes with some disadvantages:

* Even though we "only" use the `async` keyword without a full async-I/O
  runtime, we still encounter many of the drawbacks of the fragmented Rust
  async ecosystem.

  The biggest issue with this is that parts of the standard library become
  unavailable to us, for example the built-in `Vec::sort_by` can no longer be
  used for sorting in Nix because our comparators themselves are `async`.

  This led us to having to implement some logic on our own, as the design of
  `async` in Rust even makes it difficult to provide usecase-generic
  implementations of concepts like sorting.

* We need to allocate quite a few new structures on the heap in order to drive
  generators, as generators involve storing `Future` types (with unknown
  sizes) inside of structs.

  In initial testing this seems to make no significant difference in
  performance (our performance in an actual nixpkgs-eval is still bottlenecked
  by I/O concerns and reference scanning), but is something to keep in mind
  later on when we start optimising more after the low-hanging fruits have
  been reaped.

## Alternatives considered

1. Tacking on more functionality onto the existing VM loop
   implementation to accomodate problems as they show up. This is not
   preferred as the code is already getting messy.

2. Making tvix-eval a fully `async` project, pulling in something like Tokio
   or `async-std` as a runtime. This is not preferred due to the massively
   increased complexity of those solutions, and all the known issues of fully
   buying in to the async ecosystem.

   tvix-eval fundamentally should work for use-cases besides building Nix
   packages (e.g. for `//tvix/serde`), and its profile should be as slim as
   possible.

3. Convincing the Rust developers that Rust needs a way to guarantee
   constant-stack-depth tail calls through something like a `tailcall`
   keyword.

4. ... ?

[`genawaiter`]: https://docs.rs/genawaiter/
