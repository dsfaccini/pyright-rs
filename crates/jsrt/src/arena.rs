//! Leak arena: the replacement for references to garbage-collected JS objects.

use std::cell::{Cell, RefCell};
use std::fmt;
use std::hash::{Hash, Hasher};
use std::ops::Deref;
use std::ptr;

use bumpalo::Bump;

use crate::collections::FxHashMap;

thread_local! {
    static ARENA: &'static Bump = Box::leak(Box::new(Bump::new()));
    static EVAL_ARENA: &'static Bump = Box::leak(Box::new(Bump::new()));
    static ALLOCATED: Cell<usize> = const { Cell::new(0) };
    static EVAL_ALLOCATED: Cell<usize> = const { Cell::new(0) };
    static STATS: RefCell<Option<FxHashMap<&'static str, AllocStat>>> =
        RefCell::new(std::env::var_os("PYRIGHT_RS_ALLOC_STATS").map(|_| FxHashMap::default()));
}

/// Objects and bytes allocated for one Rust type, per heap.
#[derive(Clone, Copy, Default, Debug)]
pub struct AllocStat {
    pub program_count: u64,
    pub program_bytes: u64,
    pub eval_count: u64,
    pub eval_bytes: u64,
}

fn record<T>(eval: bool) {
    STATS.with(|stats| {
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
        }
    });
}

/// A handle to a value that lives until the thread exits.
///
/// Copy, compared and hashed by identity (JS `===` on objects). Never sort or
/// iterate by the address: it differs between runs.
///
/// Two heaps (see `docs/porting.md`, "memory"): the program heap holds what
/// parsing, binding and configuration create, and is never collected; the
/// evaluator heap holds what type evaluation and checking create, and is the
/// heap a later collector reclaims.
pub struct P<T: 'static>(&'static T);

impl<T: 'static> P<T> {
    /// Moves `value` into the current thread's program heap. Drop never runs for it.
    pub fn new(value: T) -> P<T> {
        ALLOCATED.with(|n| n.set(n.get() + size_of::<T>()));
        record::<T>(false);
        ARENA.with(|arena| P(arena.alloc(value)))
    }

    /// Moves `value` into the current thread's evaluator heap. Drop never runs for it.
    pub fn new_eval(value: T) -> P<T> {
        EVAL_ALLOCATED.with(|n| n.set(n.get() + size_of::<T>()));
        record::<T>(true);
        EVAL_ARENA.with(|arena| P(arena.alloc(value)))
    }

    pub fn ptr_eq(a: P<T>, b: P<T>) -> bool {
        ptr::eq(a.0, b.0)
    }

    /// The referenced value. Never store the returned reference in a struct:
    /// store the `P`, so a collector can see it.
    pub fn get(self) -> &'static T {
        self.0
    }
}

/// Bytes moved into the current thread's program heap so far.
pub fn arena_allocated_bytes() -> usize {
    ALLOCATED.with(Cell::get)
}

/// Bytes moved into the current thread's evaluator heap so far.
pub fn eval_arena_allocated_bytes() -> usize {
    EVAL_ALLOCATED.with(Cell::get)
}

/// Per-type allocation counts, largest byte total first, when the
/// `PYRIGHT_RS_ALLOC_STATS` environment variable was set at thread start.
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

impl<T> Clone for P<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for P<T> {}

impl<T> PartialEq for P<T> {
    fn eq(&self, other: &Self) -> bool {
        ptr::eq(self.0, other.0)
    }
}

impl<T> Eq for P<T> {}

impl<T> Hash for P<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        ptr::hash(self.0, state);
    }
}

impl<T> Deref for P<T> {
    type Target = T;

    fn deref(&self) -> &T {
        self.0
    }
}

/// Prints the address only: arena values form cycles.
impl<T> fmt::Debug for P<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "P({:p})", self.0)
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
}
