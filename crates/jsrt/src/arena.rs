//! `P`: the replacement for references to garbage-collected JS objects.

use std::cell::{Cell, RefCell};
use std::fmt;
use std::hash::{Hash, Hasher};
use std::ops::Deref;
use std::ptr::{self, NonNull};

use bumpalo::Bump;

use crate::collections::FxHashMap;
use crate::heap;
use crate::trace::Trace;

thread_local! {
    /// Program objects that can hold no evaluator handle: the collector never visits them.
    static ARENA: &'static Bump = Box::leak(Box::new(Bump::new()));
    static ALLOCATED: Cell<usize> = const { Cell::new(0) };
    static EVAL_ALLOCATED: Cell<usize> = const { Cell::new(0) };
    static STATS: RefCell<Option<FxHashMap<&'static str, AllocStat>>> =
        RefCell::new(std::env::var_os("PYRIGHT_RS_ALLOC_STATS").map(|_| FxHashMap::default()));
    /// `PYRIGHT_RS_ALLOC_STATS=<n>` also prints the stats each time the
    /// evaluator heap grows by another n GiB, so a run killed for memory still reports.
    static REPORT_STEP: usize = std::env::var("PYRIGHT_RS_ALLOC_STATS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .map_or(0, |gib| gib << 30);
    static NEXT_REPORT: Cell<usize> = REPORT_STEP.with(|step| Cell::new(*step));
}

/// Objects and bytes allocated for one Rust type, per heap.
#[derive(Clone, Copy, Default, Debug)]
pub struct AllocStat {
    pub program_count: u64,
    pub program_bytes: u64,
    pub eval_count: u64,
    pub eval_bytes: u64,
}

/// Whether `PYRIGHT_RS_ALLOC_STATS` is set, read once per process: `record` runs on every
/// allocation and checks this before it touches the per-thread table.
fn stats_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("PYRIGHT_RS_ALLOC_STATS").is_some())
}

fn record<T>(eval: bool) {
    if !stats_enabled() {
        return;
    }
    let enabled = STATS.with(|stats| {
        if let Some(stats) = stats.borrow_mut().as_mut() {
            let stat = stats.entry(std::any::type_name::<T>()).or_default();
            let bytes = size_of::<T>() as u64;
            if eval {
                stat.eval_count += 1;
                stat.eval_bytes += bytes;
            } else {
                stat.program_count += 1;
                stat.program_bytes += bytes;
            }
            true
        } else {
            false
        }
    });
    if enabled && eval {
        report_if_due();
    }
}

fn report_if_due() {
    let step = REPORT_STEP.with(|step| *step);
    if step == 0 || eval_arena_allocated_bytes() < NEXT_REPORT.with(Cell::get) {
        return;
    }
    NEXT_REPORT.with(|next| next.set(next.get() + step));
    eprintln!("alloc stats at {} GiB of evaluator heap:", eval_arena_allocated_bytes() >> 30);
    print_alloc_stats();
}

/// A handle to a heap object.
///
/// Copy, compared and hashed by identity (JS `===` on objects). Never sort or
/// iterate by the address: it differs between runs.
///
/// Two heaps (see `docs/porting.md`, "memory"): the program heap holds what
/// parsing, binding and configuration create, and is never collected; the
/// evaluator heap holds what type evaluation and checking create, and is
/// collected between files when `PYRIGHT_RS_GC` is on
/// (`docs/collector-design.md`).
///
/// A trait object is a `P<dyn Trait>`, made from a `P` of the concrete type
/// with [`P::to_dyn`].
///
/// Not `Send` or `Sync`: an evaluator object belongs to the thread whose
/// collector frees it.
///
/// A raw pointer, not a `&'static T`: it keeps the provenance of the whole object
/// as allocated, so a header handle reaches the details behind its header
/// ([`P::header`], [`Header`]), and a handle to a swept object is never a dangling
/// reference (`docs/porting.md`, "unsafe code").
pub struct P<T: ?Sized + 'static>(NonNull<T>);

impl<T: Trace + 'static> P<T> {
    /// Moves `value` into the current thread's program heap, which is never
    /// collected. Drop never runs for it.
    pub fn new(value: T) -> P<T> {
        const { assert!(!T::CELL_OUT_OF_LINE, "the write barrier cannot find this type's objects: a cell holding evaluator handles sits in a buffer") };
        ALLOCATED.with(|n| n.set(n.get() + size_of::<T>()));
        record::<T>(false);
        if heap::typed_program::<T>() {
            let slot = heap::alloc::<T>(false);
            // SAFETY: a fresh slot sized and aligned for `T`, never freed.
            unsafe {
                slot.write(value);
                P(NonNull::new_unchecked(slot))
            }
        } else {
            ARENA.with(|arena| P(NonNull::from(arena.alloc(value))))
        }
    }

    /// Moves `value` into the current thread's evaluator heap. The collector
    /// drops it once no root reaches it. `T` needs `#[trace(eval)]`.
    pub fn new_eval(value: T) -> P<T> {
        const { assert!(T::EVAL, "P::new_eval needs #[trace(eval)] on the type") };
        const { assert!(!T::CELL_OUT_OF_LINE, "the write barrier cannot find this type's objects: a cell holding evaluator handles sits in a buffer") };
        EVAL_ALLOCATED.with(|n| n.set(n.get() + size_of::<T>()));
        record::<T>(true);
        let slot = heap::alloc::<T>(true);
        // SAFETY: a fresh slot sized and aligned for `T`; the collector frees it
        // only when no traced handle reaches it.
        unsafe {
            slot.write(value);
            P(NonNull::new_unchecked(slot))
        }
    }
}

impl<T: 'static> P<T> {
    /// The handle of the object's first field, for a `#[repr(C)]` type that
    /// starts with a header: both handles hold the same address, and the header
    /// handle keeps the provenance of the whole object, so code that knows the
    /// full type reads past the header through it ([`Header`]).
    pub fn header<U: Trace>(self, first_field: fn(&'static T) -> &'static U) -> P<U>
    where
        T: Trace,
    {
        const { assert!(T::EVAL == U::EVAL, "P::header across heaps") };
        let header = first_field(self.get());
        assert!(ptr::addr_eq(header, self.0.as_ptr()), "P::header of a field that is not first");
        P(self.0.cast())
    }

    /// A handle to an object outside the heaps, such as in a `jsrt::region::Region`.
    ///
    /// # Safety
    /// `value` points at a live `T` that stays valid for as long as this handle,
    /// or a copy of it, is used.
    pub unsafe fn from_raw(value: *const T) -> P<T>
    where
        T: Trace,
    {
        const { assert!(!T::EVAL, "P::from_raw of an evaluator type") };
        // SAFETY: the caller guarantees `value` points at a live `T`, so it is not null.
        P(unsafe { NonNull::new_unchecked(value.cast_mut()) })
    }

    /// The same object as a trait object: `p.to_dyn::<dyn Trait>(|x| x)`. An
    /// evaluator object needs a trait whose `P` the collector traces (`U::EVAL`).
    pub fn to_dyn<U: ?Sized + Trace>(self, upcast: fn(*const T) -> *const U) -> P<U>
    where
        T: Trace,
    {
        const { assert!(!T::EVAL || U::EVAL, "P::to_dyn of an evaluator type to a trait without EVAL") };
        let object = upcast(self.0.as_ptr());
        assert!(ptr::addr_eq(object, self.0.as_ptr()), "P::to_dyn to another object");
        // SAFETY: the same address as `self`, which is not null.
        P(unsafe { NonNull::new_unchecked(object.cast_mut()) })
    }
}

impl<T: ?Sized + 'static> P<T> {
    pub fn ptr_eq(a: P<T>, b: P<T>) -> bool {
        a == b
    }

    /// The referenced value. Never store the returned reference in a struct:
    /// store the `P`, so the collector can see it. The reference covers `T`
    /// only: read a header's details through the handle ([`Header`]).
    #[inline]
    pub fn get(self) -> &'static T {
        heap::check_handle(self.addr());
        // SAFETY: a handle points at a live object: the program heap is never freed,
        // and the collector frees an evaluator object only once no handle reaches it.
        unsafe { self.0.as_ref() }
    }

    /// The object's address, with the provenance of the whole object as allocated.
    #[inline]
    pub fn as_ptr(self) -> *const T {
        heap::check_handle(self.addr());
        self.0.as_ptr()
    }

    #[cfg(test)]
    pub(crate) fn addr_for_test(self) -> usize {
        self.addr()
    }

    #[inline]
    fn addr(self) -> usize {
        self.0.as_ptr().cast::<u8>().addr()
    }
}

/// A `#[repr(C)]` header type allocated with details behind it, as the first
/// field of a larger object ([`P::header`]): `ParseNode`, `Token`, `FlowNode`,
/// `Declaration`. A `&` to the header covers the header's bytes only, so the
/// details are read through the handle: `p.d()`.
pub trait Header: Sized + 'static {
    type Details;

    /// The details of the object `header` starts.
    fn details(header: P<Self>) -> Self::Details;
}

impl<T: Header> P<T> {
    /// The details behind the header.
    #[inline]
    pub fn d(self) -> T::Details {
        T::details(self)
    }
}

/// Marks the target; `HOLDS_EVAL` follows whether `T` can live on the evaluator heap.
unsafe impl<T: ?Sized + Trace + 'static> Trace for P<T> {
    const HOLDS_EVAL: bool = T::EVAL;

    #[inline]
    fn trace(&self, t: &mut heap::Tracer) {
        if T::EVAL {
            t.mark(self.addr());
        } else if t.checking() {
            t.expect_program(self.addr(), std::any::type_name::<T>());
        }
    }
}

/// Bytes moved into the current thread's program heap so far.
pub fn arena_allocated_bytes() -> usize {
    ALLOCATED.with(Cell::get)
}

/// Bytes of the chunks the current thread's program arena holds, used or not.
pub fn arena_chunk_bytes() -> usize {
    ARENA.with(|arena| arena.allocated_bytes())
}

/// Census: bytes used in the current thread's arena chunks, and how many of those are resident.
pub(crate) fn arena_chunks_resident() -> (usize, usize) {
    ARENA.with(|arena| {
        let (mut used, mut resident) = (0, 0);
        // SAFETY: only reads the chunk bounds; nothing allocates in the arena meanwhile.
        for (ptr, len) in unsafe { arena.iter_allocated_chunks_raw() } {
            used += len;
            resident += crate::vm::resident(ptr as usize, len);
        }
        (used, resident)
    })
}

/// Bytes moved into the current thread's evaluator heap so far.
pub fn eval_arena_allocated_bytes() -> usize {
    EVAL_ALLOCATED.with(Cell::get)
}

/// Per-type allocation counts, largest byte total first, when the
/// `PYRIGHT_RS_ALLOC_STATS` environment variable was set at process start.
pub fn alloc_stats() -> Option<Vec<(&'static str, AllocStat)>> {
    STATS.with(|stats| {
        let stats = stats.borrow();
        let mut rows: Vec<_> = stats.as_ref()?.iter().map(|(k, v)| (*k, *v)).collect();
        rows.sort_by(|a, b| {
            (b.1.program_bytes + b.1.eval_bytes)
                .cmp(&(a.1.program_bytes + a.1.eval_bytes))
                .then(a.0.cmp(b.0))
        });
        Some(rows)
    })
}

/// Prints [`alloc_stats`] to stderr, when they are on.
pub fn print_alloc_stats() {
    let Some(rows) = alloc_stats() else { return };
    eprintln!("{:>14} {:>14} {:>14} {:>14}  type", "program count", "program bytes", "eval count", "eval bytes");
    for (name, s) in rows {
        eprintln!("{:>14} {:>14} {:>14} {:>14}  {name}", s.program_count, s.program_bytes, s.eval_count, s.eval_bytes);
    }
}

impl<T: ?Sized> Clone for P<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: ?Sized> Copy for P<T> {}

/// Identity by address: two trait-object handles to one object are equal.
impl<T: ?Sized + 'static> PartialEq for P<T> {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        heap::check_handle(self.addr());
        heap::check_handle(other.addr());
        ptr::addr_eq(self.0.as_ptr(), other.0.as_ptr())
    }
}

impl<T: ?Sized + 'static> Eq for P<T> {}

impl<T: ?Sized + 'static> Hash for P<T> {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        heap::check_handle(self.addr());
        ptr::hash(self.0.as_ptr().cast::<()>(), state);
    }
}

impl<T: ?Sized + 'static> Deref for P<T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &T {
        self.get()
    }
}

/// Prints the address only: arena values form cycles.
impl<T: ?Sized> fmt::Debug for P<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "P({:p})", self.0.as_ptr())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_not_value() {
        let a = P::new(1);
        let b = P::new(1);
        assert_ne!(a, b);
        assert_eq!(a, a);
        assert_eq!(*a, *b);
        assert_eq!(size_of::<Option<P<u64>>>(), size_of::<usize>());
    }

    trait Named {
        fn name(&self) -> &'static str;
    }

    #[derive(jsrt::Trace)]
    struct Thing(u32);

    impl Named for Thing {
        fn name(&self) -> &'static str {
            if self.0 == 1 { "thing" } else { "other" }
        }
    }

    unsafe impl Trace for dyn Named {
        const HOLDS_EVAL: bool = false;
        const EVAL: bool = false;
        fn trace(&self, _: &mut heap::Tracer) {}
    }

    /// Compiles only while `P` is neither `Send` nor `Sync` (the ambiguity trick of
    /// `static_assertions::assert_not_impl_any!`).
    #[test]
    fn handles_stay_on_their_thread() {
        trait AmbiguousIfImpl<A> {
            fn some_item() {}
        }
        impl<T: ?Sized> AmbiguousIfImpl<()> for T {}
        struct IfSend;
        struct IfSync;
        impl<T: ?Sized + Send> AmbiguousIfImpl<IfSend> for T {}
        impl<T: ?Sized + Sync> AmbiguousIfImpl<IfSync> for T {}
        let _ = <P<u64> as AmbiguousIfImpl<_>>::some_item;
    }

    #[test]
    fn trait_object_keeps_identity() {
        let a = P::new(Thing(1));
        let d: P<dyn Named> = a.to_dyn(|x| x);
        assert_eq!(d.name(), "thing");
        assert_eq!(d, a.to_dyn::<dyn Named>(|x| x));
        assert_ne!(d, P::new(Thing(1)).to_dyn::<dyn Named>(|x| x));
    }
}
