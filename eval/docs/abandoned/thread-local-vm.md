# We can't have nice things because IFD

The thread-local VM work below was ultimately not merged because it
was decided that it would be harmful for `tvix::eval::Value` to
implement `Eq`, `Hash`, or any of the other `std` traits.

Implementing `std` traits on `Value` was deemed harmful because IFD
can cause arbitrary amounts of compilation to occur, including
network transactions with builders.  Obviously it would be
unexpected and error-prone to have a `PartialEq::eq()` which does
something like this.  This problem does not manifest within the
"nixpkgs compatibility only" scope, or in any undeprecated language
feature other than IFD.  Although IFD is outside the "nixpkgs
compatibility scope", it [has been added to the TVL compatibility
scope](https://cl.tvl.fyi/c/depot/+/7193/comment/3418997b_0dbd0b65/).

This was the sole reason for not merging.

The explanation below may be useful in case future circumstances
affect the relevance of the reasoning above.

The implementation can be found in these CLs:

- [refactor(tvix/eval): remove lifetime parameter from VM<'o>](https://cl.tvl.fyi/c/depot/+/7194)
- [feat(tvix/eval): [FOUNDLING] thread-local VM](https://cl.tvl.fyi/c/depot/+/7195)
- [feat(tvix/eval): [FOUNDLING] VM::vm_xxx convenience methods](https://cl.tvl.fyi/c/depot/+/7196)
- [refactor(tvix/eval): [FOUNDLING]: drop explicit `&mut vm` parameter](https://cl.tvl.fyi/c/depot/+/7197)

# Thread-local storage for tvix::eval::vm::VM

## The problem

`Value::force()` takes a `&mut VM` argument, since forcing a value
requires executing opcodes.  This means that `Value::nix_eq()` too
must take a `&mut VM`, since any sensible definition of equality
will have to force thunks.

Unfortunately Rust's `PartialEq::eq()` function does not accept any
additional arguments like this, so `Value` cannot implement
`PartialEq`.  Worse, structs which *contain* `Value`s can't
implement `PartialEq` either.  This means `Value`, and anything
containing it, cannot be the key for a `BTreeMap` or `HashMap`.  We
can't even insert `Value`s into a `HashSet`!

There are other situations like this that don't involve `PartialEq`,
but it's the most glaring one.  The main problem is that you need a
`VM` in order to force thunks, and thunks can be anywhere in a
`Value`.

## Solving the problem with thread-locals

We could avoid threading the `&mut VM` through the entire codebase
by making it a thread-local.

To do this without a performance hit, we need to use LLVM
thread-locals, which are the same cost as references to `static`s
but load relative to
[`llvm.threadlocal.address`][threadlocal-intrinsic] instead of
relative to the data segment.  Unfortunately `#[thread_local]` [is
unstable][thread-local-unstable] and [unsafe in
general][thread-local-unsafe] for most of the cases where we would
want to use it.  There is one [exception][tls-const-init], however:
if a `!thread_local()` has a `const` initializer, the compiler will
insert a `#[thread_local]`; this special case is both safe and
stable.

The difficult decision is what the type of the thread-local should
be.  Since you can't get a mutable reference to a `thread_local!()`
it will have to be some interior-mutability-bestowing wrapper around
our current `struct VM`.  Here are the choices:

### `RefCell<VM>`

This is the obvious first choice, since it lets you borrow a
`RefMut<Target=VM>`.  The problem here is that we want to keep the
codebase written such that all the functions in `impl VM` still take
a `&mut self`.  This means that there will be an active mutable
borrow for the duration of `VM::call_builtin()`.  So if we implement
`PartialEq` by having `eq()` attempt a second mutable borrow from
the thread-local storage, it will fail since there is already an
active borrow.

The problem here is that you can't "unborrow" a `RefMut` except by
dropping it.  There's no way around this.

#### Problem: Uglification

The only solution here is to rewrite all the functions in `impl VM`
so they don't take any kind of `self` argument, and then have them
do a short-lived `.borrow_mut()` from the thread-local `RefCell`
*separately, each time* they want to modify one of the fields of
`VM` (currently `frames`, `stack`, `with_stack`, `warnings`).  This
means that if you had a code sequence like this:

```
impl VM {
  fn foo(&mut self, ...) {
    ...
    self.frame().ip += 1;
    self.some_other_method();
    self.frame().ip += 1;
```

You would need to add *two separate `borrow_mut()`s*, one for each
of the `self.frame().ip+=1` statements.  You can't just do one big
`borrow_mut()` because `some_other_method()` will call
`borrow_mut()` and panic.

#### Problem: Performance

The `RefCell<VM>` approach also has a fairly huge performance hit,
because every single modification to any part of `VM` will require a
reference count increment/decrement, and a conditional branch based
on the check (which will never fail) that the `RefCell` isn't
already mutably borrowed.  It will also impede a lot of rustc's
optimizations.

### `Cell<VM>`

This is a non-starter because it means that in order to mutate any
field of `VM`, you have to move the entire `struct VM` out of the
`Cell`, mutate it, and move it back in.

### `Cell<Box<VM>>`

Now we're getting warmer.  Here, we can move the `Box<VM>` out of
the cell with a single pointer-sized memory access.

We don't want to do the "uglification" described in the previous
section.  We are very fortunate that, sometime in mid-2019, the Rust
dieties [decreed by fiat][fiat-decree] that `&Cell<T>` and `&mut T`
are bit-for-bit identical, and even gave us mortals safe wrappers
[`from_mut()`][from_mut] and [`get_mut()`][get_mut] around
`mem::transmute()`.

So now, when a `VM` method (which takes `&mut self`) calls out to
some external code (like a builtin), instead of passing the `&mut
self` to the external code it can call `Cell::from_mut(&mut self)`,
and then `Cell::swap()` that into the thread-local storage cell for
the duration of the external code.  After the external code returns,
it can `Cell::swap()` it back.  This whole dance gets wrapped in a
lexical block, and the borrow checker sees that the `&Cell<Box<VM>>`
returned by `Cell::from_mut()` lives only until the end of the
lexical block, *so we get the `&mut self` back after the close-brace
for that block*.  NLL FTW.  This sounds like a lot of work, but it
should compile down to two pointer-sized loads and two pointer-sized
stores, and it is incurred basically only for `OpBuiltin`.

This all works, with only two issues:

1. `vm.rs` needs to be very careful to do the thread-local cell swap
   dance before calling anything that might call `PartialEq::eq()`
   (or any other method that expects to be able to pull the `VM` out
   of thread-local storage).  There is no compile-time check that we
   did the dance in all the right places.  If we forget to do the
   dance somewhere we'll get a runtime panic from `Option::expect()`
   (see next section).

2. Since we need to call `Cell::from_mut()` on a `Box<VM>` rather
   than a bare `VM`, we still need to rewrite all of `vm.rs` so that
   every function takes a `&mut Box<VM>` instead of a `&mut self`.
   This creates a huge amount of "noise" in the code.

Fortunately, it turns out that nearly all the "noise" that arises
from the second point can be eliminated by taking advantage of
[deref coercions][deref-coercions]!  This was the last "shoe to
drop".

There is still the issue of having to be careful about calls from
`vm.rs` to things outside that file, but it's manageable.

### `Cell<Option<Box<VM>>>`

In order to get the "safe and stable `#[thread_local]`"
[exception][tls-const-init] we need a `const` initializer, which
means we need to be able to put something into the `Cell` that isn't
a `VM`.  So the type needs to be `Cell<Option<Box<VM>>>`.

Recall that you can't turn an `Option<&T>` into an `&Option<T>`.
The latter type has the "is this a `Some` or `None`" bit immediately
adjacent to the bits representing `T`.  So if I hand you a `t:&T`
and you wrap it as `Some(t)`, those bits aren't adjacent in memory.
This means that all the VM methods need to operate on an
`Option<Box<VM>>` -- we can't just wrap a `Some()` around `&mut
self` "at the last minute" before inserting it into the thread-local
storage cell.  Fortunately deref coercions save the day here too --
the coercion is inferred through both layers (`Box` and `Option`) of
wrapper, so there is no additional noise in the code.

Note that Rust is clever and can find some sequence of bits that
aren't a valid `T`, so `sizeof(Option<T>)==sizeof(T)`.  And in fact,
`Box<T>` is one of these cases (and this is guaranteed).  So the
`Option` has no overhead.

# Closing thoughts, language-level support

This would have been easier with language-level support.

## What wouldn't help

Although it [it was decreed][fiat-decree] that `Cell<T>` and `&mut
T` are interchangeable, a `LocalKey<Cell<T>>` isn't quite the same
thing as a `Cell<T>`, so it wouldn't be safe for the standard
library to contain something like this:

```
impl<T> LocalKey<Cell<T>> {
  fn get_mut(&self) -> &mut T {
    unsafe {
      // ... mem::transmute() voodoo goes here ...
```

The problem here is that you can call `LocalKey<Cell<T>>::get_mut()` twice and
end up with two `&mut T`s that point to the same thing (mutable aliasing) which
results in undefined behavior.

## What would help

The ideal solution is for Rust to let you call arbitrary methods
`T::foo(&mut self...)` on a `LocalKey<Cell<T>>`.  This way you can
have one (and only one) `&mut T` at any syntactical point in your
program -- the `&mut self`.


[tls-const-init]: https://github.com/rust-lang/rust/pull/90774
[thread-local-unstable]: https://github.com/rust-lang/rust/issues/29594
[thread-local-unsafe-generally]: https://github.com/rust-lang/rust/issues/54366
[fiat-decree]: https://github.com/rust-lang/rust/issues/43038
[from_mut]: https://doc.rust-lang.org/stable/std/cell/struct.Cell.html#method.from_mut
[get_mut]: https://doc.rust-lang.org/stable/std/cell/struct.Cell.html#method.get_mut
[thread-local-unsafe]: [https://github.com/rust-lang/rust/issues/54366]
[deref-coercions]: https://doc.rust-lang.org/book/ch15-02-deref.html#implicit-deref-coercions-with-functions-and-methods
[threadlocal-intrinsic]: https://llvm.org/docs/LangRef.html#llvm-threadlocal-address-intrinsic
