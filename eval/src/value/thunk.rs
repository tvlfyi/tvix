//! This module implements the runtime representation of Thunks.
//!
//! Thunks are a special kind of Nix value, similar to a 0-argument
//! closure that yields some value. Thunks are used to implement the
//! lazy evaluation behaviour of Nix:
//!
//! Whenever the compiler determines that an expression should be
//! evaluated lazily, it creates a thunk instead of compiling the
//! expression value directly. At any point in the runtime where the
//! actual value of a thunk is required, it is "forced", meaning that
//! the encompassing computation takes place and the thunk takes on
//! its new value.
//!
//! Thunks have interior mutability to be able to memoise their
//! computation. Once a thunk is evaluated, its internal
//! representation becomes the result of the expression. It is legal
//! for the runtime to replace a thunk object directly with its value
//! object, but when forcing a thunk, the runtime *must* mutate the
//! memoisable slot.

use std::{
    cell::{Ref, RefCell, RefMut},
    collections::HashSet,
    fmt::Debug,
    rc::Rc,
};

use serde::Serialize;

use crate::{
    errors::{Error, ErrorKind},
    spans::LightSpan,
    upvalues::Upvalues,
    value::Closure,
    vm::{Trampoline, TrampolineAction, VM},
    Value,
};

use super::{Lambda, TotalDisplay};

/// Internal representation of a suspended native thunk.
struct SuspendedNative(Box<dyn Fn(&mut VM) -> Result<Value, ErrorKind>>);

impl Debug for SuspendedNative {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SuspendedNative({:p})", self.0)
    }
}

/// Internal representation of the different states of a thunk.
///
/// Upvalues must be finalised before leaving the initial state
/// (Suspended or RecursiveClosure).  The [`value()`] function may
/// not be called until the thunk is in the final state (Evaluated).
#[derive(Debug)]
enum ThunkRepr {
    /// Thunk is closed over some values, suspended and awaiting
    /// execution.
    Suspended {
        lambda: Rc<Lambda>,
        upvalues: Rc<Upvalues>,
        light_span: LightSpan,
    },

    /// Thunk is a suspended native computation.
    Native(SuspendedNative),

    /// Thunk currently under-evaluation; encountering a blackhole
    /// value means that infinite recursion has occured.
    Blackhole,

    /// Fully evaluated thunk.
    Evaluated(Value),
}

impl ThunkRepr {
    fn debug_repr(&self) -> String {
        match self {
            ThunkRepr::Evaluated(v) => format!("thunk(val|{})", v),
            ThunkRepr::Blackhole => "thunk(blackhole)".to_string(),
            ThunkRepr::Native(_) => "thunk(native)".to_string(),
            ThunkRepr::Suspended { lambda, .. } => format!("thunk({:p})", *lambda),
        }
    }
}

/// A thunk is created for any value which requires non-strict
/// evaluation due to self-reference or lazy semantics (or both).
/// Every reference cycle involving `Value`s will contain at least
/// one `Thunk`.
#[derive(Clone, Debug)]
pub struct Thunk(Rc<RefCell<ThunkRepr>>);

impl Thunk {
    pub fn new_closure(lambda: Rc<Lambda>) -> Self {
        Thunk(Rc::new(RefCell::new(ThunkRepr::Evaluated(Value::Closure(
            Rc::new(Closure {
                upvalues: Rc::new(Upvalues::with_capacity(lambda.upvalue_count)),
                lambda: lambda.clone(),
            }),
        )))))
    }

    pub fn new_suspended(lambda: Rc<Lambda>, light_span: LightSpan) -> Self {
        Thunk(Rc::new(RefCell::new(ThunkRepr::Suspended {
            upvalues: Rc::new(Upvalues::with_capacity(lambda.upvalue_count)),
            lambda: lambda.clone(),
            light_span,
        })))
    }

    pub fn new_suspended_native(native: Box<dyn Fn(&mut VM) -> Result<Value, ErrorKind>>) -> Self {
        Thunk(Rc::new(RefCell::new(ThunkRepr::Native(SuspendedNative(
            native,
        )))))
    }

    /// Force a thunk from a context that can't handle trampoline
    /// continuations, eg outside the VM's normal execution loop.  Calling
    /// `force_trampoline()` instead should be preferred whenever possible.
    pub fn force(&self, vm: &mut VM) -> Result<(), ErrorKind> {
        if self.is_forced() {
            return Ok(());
        }

        let mut trampoline = Self::force_trampoline(vm, Value::Thunk(self.clone()))?;
        loop {
            match trampoline.action {
                None => (),
                Some(TrampolineAction::EnterFrame {
                    lambda,
                    upvalues,
                    arg_count,
                    light_span: _,
                }) => vm.enter_frame(lambda, upvalues, arg_count)?,
            }
            match trampoline.continuation {
                None => break,
                Some(cont) => {
                    trampoline = cont(vm)?;
                    continue;
                }
            }
        }
        vm.pop();
        Ok(())
    }

    /// Evaluate the content of a thunk, potentially repeatedly, until a
    /// non-thunk value is returned.
    ///
    /// When this function returns, the result of one "round" of forcing is left
    /// at the top of the stack. This may still be a partially evaluated thunk
    /// which must be further run through the trampoline.
    pub fn force_trampoline(vm: &mut VM, outer: Value) -> Result<Trampoline, ErrorKind> {
        match outer {
            Value::Thunk(thunk) => thunk.force_trampoline_self(vm),
            v => {
                vm.push(v);
                Ok(Trampoline::default())
            }
        }
    }

    /// Analyses `self` and, upon finding a suspended thunk, requests evaluation
    /// of the contained code from the VM. Control flow may pass back and forth
    /// between this function and the VM multiple times through continuations
    /// that call `force_trampoline` again if nested thunks are encountered.
    ///
    /// This function is entered again by returning a continuation that calls
    /// [force_trampoline].
    // When working on this function, care should be taken to ensure that each
    // evaluated thunk's *own copy* of its inner representation is replaced by
    // evaluated results and blackholes, as appropriate. It is a critical error
    // to move the representation of one thunk into another and can lead to
    // hard-to-debug performance issues.
    // TODO: check Rc count when replacing inner repr, to skip it optionally
    fn force_trampoline_self(&self, vm: &mut VM) -> Result<Trampoline, ErrorKind> {
        // If the current thunk is already fully evaluated, leave its evaluated
        // value on the stack and return an empty trampoline. The VM will
        // continue running the code that landed us here.
        if self.is_forced() {
            vm.push(self.value().clone());
            return Ok(Trampoline::default());
        }

        // Begin evaluation of this thunk by marking it as a blackhole, meaning
        // that any other trampoline loop round encountering this thunk before
        // its evaluation is completed detected an evaluation cycle.
        let inner = self.0.replace(ThunkRepr::Blackhole);

        match inner {
            // If there was already a blackhole in the thunk, this is an
            // evaluation cycle.
            ThunkRepr::Blackhole => return Err(ErrorKind::InfiniteRecursion),

            // If there is a native function stored in the thunk, evaluate it
            // and replace this thunk's representation with it. Then bounces off
            // the trampoline, to handle the case of the native function
            // returning another thunk.
            ThunkRepr::Native(native) => {
                let value = native.0(vm)?;
                self.0.replace(ThunkRepr::Evaluated(value));
                let self_clone = self.clone();

                return Ok(Trampoline {
                    action: None,
                    continuation: Some(Box::new(move |vm| {
                        Thunk::force_trampoline(vm, Value::Thunk(self_clone))
                            .map_err(|kind| Error::new(kind, todo!("BUG: b/238")))
                    })),
                });
            }

            // When encountering a suspended thunk, construct a trampoline that
            // enters the thunk's code in the VM and replaces the thunks
            // representation with the evaluated one upon return.
            //
            // Thunks may be nested, so this case initiates another round of
            // trampolining to ensure that the returned value is forced.
            ThunkRepr::Suspended {
                lambda,
                upvalues,
                light_span,
            } => {
                // Clone self to move an Rc pointing to *this* thunk instance
                // into the continuation closure.
                let self_clone = self.clone();

                return Ok(Trampoline {
                    // Ask VM to enter frame of this thunk ...
                    action: Some(TrampolineAction::EnterFrame {
                        lambda,
                        upvalues,
                        arg_count: 0,
                        light_span: light_span.clone(),
                    }),

                    // ... and replace the inner representation once that is done,
                    // looping back around to here.
                    continuation: Some(Box::new(move |vm: &mut VM| {
                        let should_be_blackhole =
                            self_clone.0.replace(ThunkRepr::Evaluated(vm.pop()));
                        debug_assert!(matches!(should_be_blackhole, ThunkRepr::Blackhole));

                        Thunk::force_trampoline(vm, Value::Thunk(self_clone))
                            .map_err(|kind| Error::new(kind, light_span.span()))
                    })),
                });
            }

            // Note by tazjin: I have decided at this point to fully unroll the inner thunk handling
            // here, leaving no room for confusion about how inner thunks are handled. This *could*
            // be written in a shorter way (for example by using a helper function that handles all
            // cases in which inner thunks can trivially be turned into a value), but given that we
            // have been bitten by this logic repeatedly, I think it is better to let it be slightly
            // verbose for now.

            // If an inner thunk is found and already fully-forced, we can
            // short-circuit and replace the representation of self with it.
            ThunkRepr::Evaluated(Value::Thunk(ref inner)) if inner.is_forced() => {
                self.0.replace(ThunkRepr::Evaluated(inner.value().clone()));
                vm.push(inner.value().clone());
                return Ok(Trampoline::default());
            }

            // Otherwise we handle inner thunks mostly as above, with the
            // primary difference that we set the representations of *both*
            // thunks in this case.
            ThunkRepr::Evaluated(Value::Thunk(ref inner)) => {
                // The inner thunk is now under evaluation, mark it as such.
                let inner_repr = inner.0.replace(ThunkRepr::Blackhole);

                match inner_repr {
                    ThunkRepr::Blackhole => return Err(ErrorKind::InfiniteRecursion),

                    // Same as for the native case above, but results are placed
                    // in *both* thunks.
                    ThunkRepr::Native(native) => {
                        let value = native.0(vm)?;
                        self.0.replace(ThunkRepr::Evaluated(value.clone()));
                        inner.0.replace(ThunkRepr::Evaluated(value));
                        let self_clone = self.clone();

                        return Ok(Trampoline {
                            action: None,
                            continuation: Some(Box::new(move |vm| {
                                Thunk::force_trampoline(vm, Value::Thunk(self_clone))
                                    .map_err(|kind| Error::new(kind, todo!("BUG: b/238")))
                            })),
                        });
                    }

                    // Inner suspended thunks are trampolined to the VM, and
                    // their results written to both thunks in the continuation.
                    ThunkRepr::Suspended {
                        lambda,
                        upvalues,
                        light_span,
                    } => {
                        let self_clone = self.clone();
                        let inner_clone = inner.clone();

                        return Ok(Trampoline {
                            // Ask VM to enter frame of this thunk ...
                            action: Some(TrampolineAction::EnterFrame {
                                lambda,
                                upvalues,
                                arg_count: 0,
                                light_span: light_span.clone(),
                            }),

                            // ... and replace the inner representations.
                            continuation: Some(Box::new(move |vm: &mut VM| {
                                let result = vm.pop();

                                let self_blackhole =
                                    self_clone.0.replace(ThunkRepr::Evaluated(result.clone()));
                                debug_assert!(matches!(self_blackhole, ThunkRepr::Blackhole));

                                let inner_blackhole =
                                    inner_clone.0.replace(ThunkRepr::Evaluated(result));
                                debug_assert!(matches!(inner_blackhole, ThunkRepr::Blackhole));

                                Thunk::force_trampoline(vm, Value::Thunk(self_clone))
                                    .map_err(|kind| Error::new(kind, light_span.span()))
                            })),
                        });
                    }

                    // If the inner thunk is some arbitrary other value (this is
                    // almost guaranteed to be another thunk), change our
                    // representation to the same inner thunk and bounce off the
                    // trampoline. The inner thunk is changed *back* to the same
                    // state.
                    //
                    // This is safe because we are not cloning the innermost
                    // thunk's representation, so while the inner thunk will not
                    // eventually have its representation replaced by _this_
                    // trampoline run, we will return the correct representation
                    // out of here and memoize the innermost thunk.
                    ThunkRepr::Evaluated(v) => {
                        self.0.replace(ThunkRepr::Evaluated(v.clone()));
                        inner.0.replace(ThunkRepr::Evaluated(v));
                        let self_clone = self.clone();

                        return Ok(Trampoline {
                            action: None,
                            continuation: Some(Box::new(move |vm: &mut VM| {
                                // TODO(tazjin): not sure about this span ...
                                // let span = vm.current_span();
                                Thunk::force_trampoline(vm, Value::Thunk(self_clone))
                                    .map_err(|kind| Error::new(kind, todo!("BUG: b/238")))
                            })),
                        });
                    }
                }
            }

            // This branch can not occur here, it would have been caught by our
            // `self.is_forced()` check above.
            ThunkRepr::Evaluated(_) => unreachable!("BUG: definition of Thunk::is_forced changed"),
        }
    }

    pub fn finalise(&self, stack: &[Value]) {
        self.upvalues_mut().resolve_deferred_upvalues(stack);
    }

    pub fn is_evaluated(&self) -> bool {
        matches!(*self.0.borrow(), ThunkRepr::Evaluated(_))
    }

    pub fn is_suspended(&self) -> bool {
        matches!(
            *self.0.borrow(),
            ThunkRepr::Suspended { .. } | ThunkRepr::Native(_)
        )
    }

    /// Returns true if forcing this thunk will not change it.
    pub fn is_forced(&self) -> bool {
        match *self.0.borrow() {
            ThunkRepr::Blackhole => panic!("is_forced() called on a blackholed thunk"),
            ThunkRepr::Evaluated(Value::Thunk(_)) => false,
            ThunkRepr::Evaluated(_) => true,
            _ => false,
        }
    }

    /// Returns a reference to the inner evaluated value of a thunk.
    /// It is an error to call this on a thunk that has not been
    /// forced, or is not otherwise known to be fully evaluated.
    // Note: Due to the interior mutability of thunks this is
    // difficult to represent in the type system without impacting the
    // API too much.
    pub fn value(&self) -> Ref<Value> {
        Ref::map(self.0.borrow(), |thunk| match thunk {
            ThunkRepr::Evaluated(value) => value,
            ThunkRepr::Blackhole => panic!("Thunk::value called on a black-holed thunk"),
            ThunkRepr::Suspended { .. } | ThunkRepr::Native(_) => {
                panic!("Thunk::value called on a suspended thunk")
            }
        })
    }

    pub fn upvalues(&self) -> Ref<'_, Upvalues> {
        Ref::map(self.0.borrow(), |thunk| match thunk {
            ThunkRepr::Suspended { upvalues, .. } => upvalues.as_ref(),
            ThunkRepr::Evaluated(Value::Closure(c)) => &c.upvalues,
            _ => panic!("upvalues() on non-suspended thunk"),
        })
    }

    pub fn upvalues_mut(&self) -> RefMut<'_, Upvalues> {
        RefMut::map(self.0.borrow_mut(), |thunk| match thunk {
            ThunkRepr::Suspended { upvalues, .. } => Rc::get_mut(upvalues).unwrap(),
            ThunkRepr::Evaluated(Value::Closure(c)) => Rc::get_mut(
                &mut Rc::get_mut(c).unwrap().upvalues,
            )
            .expect(
                "upvalues_mut() was called on a thunk which already had multiple references to it",
            ),
            thunk => panic!("upvalues() on non-suspended thunk: {thunk:?}"),
        })
    }

    /// Do not use this without first reading and understanding
    /// `tvix/docs/value-pointer-equality.md`.
    pub(crate) fn ptr_eq(&self, other: &Self) -> bool {
        if Rc::ptr_eq(&self.0, &other.0) {
            return true;
        }
        match &*self.0.borrow() {
            ThunkRepr::Evaluated(Value::Closure(c1)) => match &*other.0.borrow() {
                ThunkRepr::Evaluated(Value::Closure(c2)) => Rc::ptr_eq(c1, c2),
                _ => false,
            },
            _ => false,
        }
    }

    /// Helper function to format thunks in observer output.
    pub(crate) fn debug_repr(&self) -> String {
        self.0.borrow().debug_repr()
    }
}

impl TotalDisplay for Thunk {
    fn total_fmt(&self, f: &mut std::fmt::Formatter<'_>, set: &mut ThunkSet) -> std::fmt::Result {
        if !set.insert(self) {
            return f.write_str("<CYCLE>");
        }

        match self.0.try_borrow() {
            Ok(repr) => match &*repr {
                ThunkRepr::Evaluated(v) => v.total_fmt(f, set),
                _ => f.write_str("internal[thunk]"),
            },

            _ => f.write_str("internal[thunk]"),
        }
    }
}

impl Serialize for Thunk {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.value().serialize(serializer)
    }
}

/// A wrapper type for tracking which thunks have already been seen in a
/// context. This is necessary for cycle detection.
///
/// The inner `HashSet` is not available on the outside, as it would be
/// potentially unsafe to interact with the pointers in the set.
#[derive(Default)]
pub struct ThunkSet(HashSet<*mut ThunkRepr>);

impl ThunkSet {
    /// Check whether the given thunk has already been seen. Will mark the thunk
    /// as seen otherwise.
    pub fn insert(&mut self, thunk: &Thunk) -> bool {
        let ptr: *mut ThunkRepr = thunk.0.as_ptr();
        self.0.insert(ptr)
    }
}
