//! Leak arena: the replacement for references to garbage-collected JS objects.

use std::cell::Cell;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::ops::Deref;
use std::ptr;

use bumpalo::Bump;

thread_local! {
    static ARENA: &'static Bump = Box::leak(Box::new(Bump::new()));
    static ALLOCATED: Cell<usize> = const { Cell::new(0) };
}

/// A handle to a value that lives until the thread exits.
///
/// Copy, compared and hashed by identity (JS `===` on objects). Never sort or
/// iterate by the address: it differs between runs.
pub struct P<T: 'static>(&'static T);

impl<T: 'static> P<T> {
    /// Moves `value` into the current thread's arena. Drop never runs for it.
    pub fn new(value: T) -> P<T> {
        ALLOCATED.with(|n| n.set(n.get() + size_of::<T>()));
        ARENA.with(|arena| P(arena.alloc(value)))
    }

    pub fn ptr_eq(a: P<T>, b: P<T>) -> bool {
        ptr::eq(a.0, b.0)
    }

    pub fn get(self) -> &'static T {
        self.0
    }
}

/// Bytes moved into the current thread's arena so far.
pub fn arena_allocated_bytes() -> usize {
    ALLOCATED.with(Cell::get)
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
