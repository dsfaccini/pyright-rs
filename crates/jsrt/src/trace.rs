//! `Trace`: how the evaluator heap collector finds the handles a value holds
//! (`docs/collector-design.md`, "tracing"). `#[derive(jsrt::Trace)]` writes
//! it for port types; this module writes it for std and dependency types.

use std::cell::{Cell, OnceCell, RefCell};
use std::collections::{HashMap, HashSet, VecDeque};
use std::marker::PhantomData;
use std::path::PathBuf;
use std::rc::Rc;

use indexmap::{IndexMap, IndexSet};

use crate::heap::{ClassSlot, Tracer};

/// Visits every handle a value holds.
///
/// # Safety
/// `trace` must visit every `P` the value can reach without going through
/// another `P`, and `HOLDS_EVAL` must be true when one of them can point into
/// the evaluator heap. A missed handle is a use-after-free once its target is
/// swept. Check mode verifies both on the corpora.
pub unsafe trait Trace {
    /// A value of this type can hold a handle to an evaluator-heap object.
    const HOLDS_EVAL: bool;
    /// A value of this type can live on the evaluator heap. `P::new_eval`
    /// requires it, so a `P<T>` holds an evaluator handle only when `T::EVAL`.
    /// A std or dependency type can live there when it can hold an evaluator
    /// handle: the evaluator allocates `RefCell<Vec<P<Type>>>` directly.
    const EVAL: bool = Self::HOLDS_EVAL;
    /// A value of this type holds an interior-mutable cell that can hold an
    /// evaluator handle.
    const HAS_CELL: bool = false;
    /// Such a cell sits behind an indirection (a `Vec`, `Box`, `Rc` or map buffer)
    /// rather than in the value's own bytes. The write barrier finds a written
    /// object by the cell's address, so a heap object's type must not have it
    /// (`P::new_eval` and typed `P::new` check at compile time).
    const CELL_OUT_OF_LINE: bool = false;

    fn trace(&self, t: &mut Tracer);

    /// Prefetches the buffers this value owns directly, which dropping it frees.
    /// The sweep calls it on dead objects a few slots ahead of the one it drops.
    #[inline]
    fn prefetch_owned(&self) {}

    /// The type's allocator id, cached in a static the derive emits for a
    /// non-generic type. `None` makes the heap look the type up by `TypeId`.
    #[doc(hidden)]
    #[inline]
    fn class_slot() -> Option<&'static ClassSlot>
    where
        Self: Sized,
    {
        None
    }
}

/// Types that hold no handle.
macro_rules! leaf {
    ($($t:ty),* $(,)?) => {
        $(unsafe impl Trace for $t {
            const HOLDS_EVAL: bool = false;
            #[inline]
            fn trace(&self, _: &mut Tracer) {}
        })*
    };
}

leaf!(
    (), bool, char, u8, u16, u32, u64, u128, usize, i8, i16, i32, i64, i128, isize, f32, f64, str,
    PathBuf, regex::Regex, num_bigint::BigInt, serde_json::Value, std::time::Instant, std::time::Duration,
);

unsafe impl Trace for String {
    const HOLDS_EVAL: bool = false;
    #[inline]
    fn trace(&self, _: &mut Tracer) {}
    #[inline]
    fn prefetch_owned(&self) {
        if self.capacity() != 0 {
            crate::heap::prefetch(self.as_ptr() as usize);
        }
    }
}

macro_rules! fn_ptr {
    ($($a:ident),*) => {
        unsafe impl<R $(, $a)*> Trace for fn($($a),*) -> R {
            const HOLDS_EVAL: bool = false;
            #[inline]
            fn trace(&self, _: &mut Tracer) {}
        }
    };
}

fn_ptr!();
fn_ptr!(A);
fn_ptr!(A, B);
fn_ptr!(A, B, C);
fn_ptr!(A, B, C, D);

/// A `&'static` points into a heap that is never collected or into leaked
/// memory, never into the evaluator heap (`docs/porting.md`, "memory").
unsafe impl<T: ?Sized> Trace for &'static T {
    const HOLDS_EVAL: bool = false;
    #[inline]
    fn trace(&self, t: &mut Tracer) {
        if t.checking() {
            t.expect_program(std::ptr::from_ref::<T>(*self).cast::<u8>() as usize, std::any::type_name::<T>());
        }
    }
}

unsafe impl<T> Trace for PhantomData<T> {
    const HOLDS_EVAL: bool = false;
    #[inline]
    fn trace(&self, _: &mut Tracer) {}
}

unsafe impl<T: Trace> Trace for Option<T> {
    #[inline]
    fn prefetch_owned(&self) {
        if let Some(v) = self {
            v.prefetch_owned();
        }
    }
    const HOLDS_EVAL: bool = T::HOLDS_EVAL;
    const HAS_CELL: bool = T::HAS_CELL;
    const CELL_OUT_OF_LINE: bool = T::CELL_OUT_OF_LINE;
    #[inline]
    fn trace(&self, t: &mut Tracer) {
        if let Some(v) = self {
            v.trace(t);
        }
    }
}

unsafe impl<T: Trace> Trace for [T] {
    const HOLDS_EVAL: bool = T::HOLDS_EVAL;
    const HAS_CELL: bool = T::HAS_CELL;
    const CELL_OUT_OF_LINE: bool = T::CELL_OUT_OF_LINE;
    #[inline]
    fn trace(&self, t: &mut Tracer) {
        for v in self {
            v.trace(t);
        }
    }
}

unsafe impl<T: Trace, const N: usize> Trace for [T; N] {
    const HOLDS_EVAL: bool = T::HOLDS_EVAL;
    const HAS_CELL: bool = T::HAS_CELL;
    const CELL_OUT_OF_LINE: bool = T::CELL_OUT_OF_LINE;
    #[inline]
    fn trace(&self, t: &mut Tracer) {
        self.as_slice().trace(t);
    }
}

unsafe impl<T: Trace> Trace for Vec<T> {
    #[inline]
    fn prefetch_owned(&self) {
        if self.capacity() != 0 {
            crate::heap::prefetch(self.as_ptr() as usize);
        }
    }
    const HOLDS_EVAL: bool = T::HOLDS_EVAL;
    const HAS_CELL: bool = T::HAS_CELL;
    const CELL_OUT_OF_LINE: bool = T::HAS_CELL;
    #[inline]
    fn trace(&self, t: &mut Tracer) {
        self.as_slice().trace(t);
    }
}

unsafe impl<T: Trace> Trace for VecDeque<T> {
    const HOLDS_EVAL: bool = T::HOLDS_EVAL;
    const HAS_CELL: bool = T::HAS_CELL;
    const CELL_OUT_OF_LINE: bool = T::HAS_CELL;
    #[inline]
    fn trace(&self, t: &mut Tracer) {
        for v in self {
            v.trace(t);
        }
    }
}

unsafe impl<T: ?Sized + Trace> Trace for Box<T> {
    #[inline]
    fn prefetch_owned(&self) {
        crate::heap::prefetch(std::ptr::from_ref::<T>(self).cast::<u8>() as usize);
    }
    const HOLDS_EVAL: bool = T::HOLDS_EVAL;
    const HAS_CELL: bool = T::HAS_CELL;
    const CELL_OUT_OF_LINE: bool = T::HAS_CELL;
    #[inline]
    fn trace(&self, t: &mut Tracer) {
        (**self).trace(t);
    }
}

/// An `Rc` is acyclic (`docs/porting.md`, "object references"), so tracing
/// through it ends. A value shared by several holders is traced once per holder.
unsafe impl<T: ?Sized + Trace> Trace for Rc<T> {
    /// Dropping an `Rc` writes its count, next to the value.
    #[inline]
    fn prefetch_owned(&self) {
        crate::heap::prefetch(std::ptr::from_ref::<T>(self).cast::<u8>() as usize);
    }
    const HOLDS_EVAL: bool = T::HOLDS_EVAL;
    const HAS_CELL: bool = T::HAS_CELL;
    const CELL_OUT_OF_LINE: bool = T::HAS_CELL;
    #[inline]
    fn trace(&self, t: &mut Tracer) {
        (**self).trace(t);
    }
}

/// The std cells have no write barrier: one that can hold an evaluator handle is a
/// compile error. Use `jsrt::cell`.
unsafe impl<T: Copy + Trace> Trace for Cell<T> {
    const HOLDS_EVAL: bool = T::HOLDS_EVAL;
    const HAS_CELL: bool = T::HOLDS_EVAL;
    #[inline]
    fn trace(&self, t: &mut Tracer) {
        const { assert!(!T::HOLDS_EVAL, "a std::cell::Cell holds evaluator handles: use jsrt::cell::Cell") };
        self.get().trace(t);
    }
}

/// No `RefCell` is mutably borrowed between files, where collection runs.
unsafe impl<T: ?Sized + Trace> Trace for RefCell<T> {
    const HOLDS_EVAL: bool = T::HOLDS_EVAL;
    const HAS_CELL: bool = T::HOLDS_EVAL;
    const CELL_OUT_OF_LINE: bool = T::CELL_OUT_OF_LINE;
    #[inline]
    fn trace(&self, t: &mut Tracer) {
        const { assert!(!T::HOLDS_EVAL, "a std::cell::RefCell holds evaluator handles: use jsrt::cell::RefCell") };
        // SAFETY: the reference does not outlive this call, and tracing never writes.
        match unsafe { self.try_borrow_unguarded() } {
            Ok(v) => v.trace(t),
            Err(_) => panic!("collector: a RefCell<{}> is mutably borrowed during collection", std::any::type_name::<T>()),
        }
    }
}

unsafe impl<T: Trace> Trace for OnceCell<T> {
    const HOLDS_EVAL: bool = T::HOLDS_EVAL;
    const HAS_CELL: bool = T::HOLDS_EVAL;
    const CELL_OUT_OF_LINE: bool = T::CELL_OUT_OF_LINE;
    #[inline]
    fn trace(&self, t: &mut Tracer) {
        const { assert!(!T::HOLDS_EVAL, "a std::cell::OnceCell holds evaluator handles: use jsrt::cell::OnceCell") };
        if let Some(v) = self.get() {
            v.trace(t);
        }
    }
}

unsafe impl<K: Trace, V: Trace, S> Trace for IndexMap<K, V, S> {
    #[inline]
    fn prefetch_owned(&self) {
        if self.capacity() != 0 {
            crate::heap::prefetch(self.as_slice() as *const _ as *const u8 as usize);
        }
    }
    const HOLDS_EVAL: bool = K::HOLDS_EVAL || V::HOLDS_EVAL;
    const HAS_CELL: bool = K::HAS_CELL || V::HAS_CELL;
    const CELL_OUT_OF_LINE: bool = K::HAS_CELL || V::HAS_CELL;
    #[inline]
    fn trace(&self, t: &mut Tracer) {
        for (k, v) in self {
            if K::HOLDS_EVAL || t.checking() {
                k.trace(t);
            }
            v.trace(t);
        }
    }
}

unsafe impl<T: Trace, S> Trace for IndexSet<T, S> {
    #[inline]
    fn prefetch_owned(&self) {
        if self.capacity() != 0 {
            crate::heap::prefetch(self.as_slice() as *const _ as *const u8 as usize);
        }
    }
    const HOLDS_EVAL: bool = T::HOLDS_EVAL;
    const HAS_CELL: bool = T::HAS_CELL;
    const CELL_OUT_OF_LINE: bool = T::HAS_CELL;
    #[inline]
    fn trace(&self, t: &mut Tracer) {
        for v in self {
            v.trace(t);
        }
    }
}

unsafe impl<K: Trace, V: Trace, S> Trace for HashMap<K, V, S> {
    const HOLDS_EVAL: bool = K::HOLDS_EVAL || V::HOLDS_EVAL;
    const HAS_CELL: bool = K::HAS_CELL || V::HAS_CELL;
    const CELL_OUT_OF_LINE: bool = K::HAS_CELL || V::HAS_CELL;
    #[inline]
    fn trace(&self, t: &mut Tracer) {
        for (k, v) in self {
            if K::HOLDS_EVAL || t.checking() {
                k.trace(t);
            }
            v.trace(t);
        }
    }
}

unsafe impl<T: Trace, S> Trace for HashSet<T, S> {
    const HOLDS_EVAL: bool = T::HOLDS_EVAL;
    const HAS_CELL: bool = T::HAS_CELL;
    const CELL_OUT_OF_LINE: bool = T::HAS_CELL;
    #[inline]
    fn trace(&self, t: &mut Tracer) {
        for v in self {
            v.trace(t);
        }
    }
}

macro_rules! tuple {
    ($($a:ident $i:tt),*) => {
        unsafe impl<$($a: Trace),*> Trace for ($($a,)*) {
            const HOLDS_EVAL: bool = false $(|| $a::HOLDS_EVAL)*;
            const HAS_CELL: bool = false $(|| $a::HAS_CELL)*;
            const CELL_OUT_OF_LINE: bool = false $(|| $a::CELL_OUT_OF_LINE)*;
            #[inline]
            fn trace(&self, t: &mut Tracer) {
                $(if $a::HOLDS_EVAL || t.checking() { self.$i.trace(t); })*
            }
        }
    };
}

tuple!(A 0);
tuple!(A 0, B 1);
tuple!(A 0, B 1, C 2);
tuple!(A 0, B 1, C 2, D 3);
tuple!(A 0, B 1, C 2, D 3, E 4);
tuple!(A 0, B 1, C 2, D 3, E 4, F 5);
