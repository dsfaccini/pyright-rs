//! Interior-mutable cells for heap objects: `Cell`, `RefCell` and `OnceCell` with
//! the std API, plus a write barrier for the generational collector
//! (`docs/collector-design.md`, "write barrier"), and [`OptionRefCell`], sized for
//! objects created by the million.
//!
//! Every write that can store an evaluator handle into a cell calls
//! [`heap::remember`] with the cell's address first: `Cell::set` and its kin,
//! `RefCell::borrow_mut` and its kin, `OnceCell::set` and `get_or_init`. A cell whose
//! value type holds no evaluator handle (`Trace::HOLDS_EVAL` false) skips it at
//! compile time.

use std::cmp::Ordering;
use std::fmt;
use std::ops::Deref;
use std::ptr;

pub use std::cell::{Ref, RefMut};

use crate::heap::{self, Tracer};
use crate::trace::Trace;

#[inline(always)]
fn remember<T: ?Sized + Trace, C: ?Sized>(cell: &C) {
    if T::HOLDS_EVAL {
        heap::remember(ptr::from_ref(cell).cast::<u8>() as usize);
    }
}

/// `std::cell::Cell` with the write barrier.
#[repr(transparent)]
#[derive(Default)]
pub struct Cell<T: ?Sized>(std::cell::Cell<T>);

impl<T> Cell<T> {
    #[inline]
    pub const fn new(value: T) -> Cell<T> {
        Cell(std::cell::Cell::new(value))
    }

    #[inline]
    pub fn into_inner(self) -> T {
        self.0.into_inner()
    }
}

impl<T: ?Sized> Cell<T> {
    #[inline]
    pub fn get_mut(&mut self) -> &mut T {
        self.0.get_mut()
    }
}

impl<T: Copy> Cell<T> {
    #[inline]
    pub fn get(&self) -> T {
        self.0.get()
    }
}

impl<T: Trace> Cell<T> {
    #[inline]
    pub fn set(&self, value: T) {
        remember::<T, _>(self);
        self.0.set(value);
    }

    #[inline]
    pub fn replace(&self, value: T) -> T {
        remember::<T, _>(self);
        self.0.replace(value)
    }

    #[inline]
    pub fn swap(&self, other: &Cell<T>) {
        remember::<T, _>(self);
        remember::<T, _>(other);
        self.0.swap(&other.0);
    }
}

impl<T: Default + Trace> Cell<T> {
    #[inline]
    pub fn take(&self) -> T {
        self.0.take()
    }
}

impl<T: Copy> Clone for Cell<T> {
    #[inline]
    fn clone(&self) -> Cell<T> {
        Cell::new(self.get())
    }
}

impl<T: Copy + fmt::Debug> fmt::Debug for Cell<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl<T: Copy + PartialEq> PartialEq for Cell<T> {
    fn eq(&self, other: &Cell<T>) -> bool {
        self.get() == other.get()
    }
}

impl<T: Copy + Eq> Eq for Cell<T> {}

impl<T: Copy + PartialOrd> PartialOrd for Cell<T> {
    fn partial_cmp(&self, other: &Cell<T>) -> Option<Ordering> {
        self.get().partial_cmp(&other.get())
    }
}

impl<T: Copy + Ord> Ord for Cell<T> {
    fn cmp(&self, other: &Cell<T>) -> Ordering {
        self.get().cmp(&other.get())
    }
}

impl<T> From<T> for Cell<T> {
    fn from(value: T) -> Cell<T> {
        Cell::new(value)
    }
}

/// `std::cell::RefCell` with the write barrier on mutable borrows.
#[repr(transparent)]
#[derive(Default)]
pub struct RefCell<T: ?Sized>(std::cell::RefCell<T>);

impl<T> RefCell<T> {
    #[inline]
    pub const fn new(value: T) -> RefCell<T> {
        RefCell(std::cell::RefCell::new(value))
    }

    #[inline]
    pub fn into_inner(self) -> T {
        self.0.into_inner()
    }
}

impl<T: ?Sized> RefCell<T> {
    #[inline]
    #[track_caller]
    pub fn borrow(&self) -> Ref<'_, T> {
        self.0.borrow()
    }

    #[inline]
    pub fn try_borrow(&self) -> Result<Ref<'_, T>, std::cell::BorrowError> {
        self.0.try_borrow()
    }

    #[inline]
    pub fn get_mut(&mut self) -> &mut T {
        self.0.get_mut()
    }

    /// # Safety
    /// As `std::cell::RefCell::try_borrow_unguarded`.
    #[inline]
    pub unsafe fn try_borrow_unguarded(&self) -> Result<&T, std::cell::BorrowError> {
        unsafe { self.0.try_borrow_unguarded() }
    }
}

impl<T: ?Sized + Trace> RefCell<T> {
    #[inline]
    #[track_caller]
    pub fn borrow_mut(&self) -> RefMut<'_, T> {
        remember::<T, _>(self);
        self.0.borrow_mut()
    }

    #[inline]
    pub fn try_borrow_mut(&self) -> Result<RefMut<'_, T>, std::cell::BorrowMutError> {
        remember::<T, _>(self);
        self.0.try_borrow_mut()
    }
}

impl<T: Trace> RefCell<T> {
    #[inline]
    #[track_caller]
    pub fn replace(&self, value: T) -> T {
        remember::<T, _>(self);
        self.0.replace(value)
    }

    #[inline]
    #[track_caller]
    pub fn replace_with(&self, f: impl FnOnce(&mut T) -> T) -> T {
        remember::<T, _>(self);
        self.0.replace_with(f)
    }

    #[inline]
    pub fn swap(&self, other: &RefCell<T>) {
        remember::<T, _>(self);
        remember::<T, _>(other);
        self.0.swap(&other.0);
    }
}

impl<T: Default + Trace> RefCell<T> {
    #[inline]
    pub fn take(&self) -> T {
        remember::<T, _>(self);
        self.0.take()
    }
}

impl<T: Clone> Clone for RefCell<T> {
    #[track_caller]
    fn clone(&self) -> RefCell<T> {
        RefCell(self.0.clone())
    }
}

impl<T: ?Sized + fmt::Debug> fmt::Debug for RefCell<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl<T: ?Sized + PartialEq> PartialEq for RefCell<T> {
    fn eq(&self, other: &RefCell<T>) -> bool {
        self.0 == other.0
    }
}

impl<T: ?Sized + Eq> Eq for RefCell<T> {}

impl<T> From<T> for RefCell<T> {
    fn from(value: T) -> RefCell<T> {
        RefCell::new(value)
    }
}

/// `std::cell::OnceCell` with the write barrier on initialization.
#[repr(transparent)]
#[derive(Default, Clone, PartialEq, Eq)]
pub struct OnceCell<T>(std::cell::OnceCell<T>);

impl<T> OnceCell<T> {
    #[inline]
    pub const fn new() -> OnceCell<T> {
        OnceCell(std::cell::OnceCell::new())
    }

    #[inline]
    pub fn get(&self) -> Option<&T> {
        self.0.get()
    }

    #[inline]
    pub fn get_mut(&mut self) -> Option<&mut T> {
        self.0.get_mut()
    }

    #[inline]
    pub fn into_inner(self) -> Option<T> {
        self.0.into_inner()
    }

    #[inline]
    pub fn take(&mut self) -> Option<T> {
        self.0.take()
    }
}

impl<T: Trace> OnceCell<T> {
    #[inline]
    pub fn set(&self, value: T) -> Result<(), T> {
        remember::<T, _>(self);
        self.0.set(value)
    }

    #[inline]
    pub fn get_or_init(&self, f: impl FnOnce() -> T) -> &T {
        if let Some(value) = self.0.get() {
            return value;
        }
        remember::<T, _>(self);
        self.0.get_or_init(f)
    }
}

impl<T: fmt::Debug> fmt::Debug for OnceCell<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

// SAFETY: as the std cells' impls in `trace.rs`: a cell is traced through its current
// value, never written while tracing.
unsafe impl<T: Copy + Trace> Trace for Cell<T> {
    const HOLDS_EVAL: bool = T::HOLDS_EVAL;
    const HAS_CELL: bool = T::HOLDS_EVAL;
    #[inline]
    fn trace(&self, t: &mut Tracer) {
        self.get().trace(t);
    }
}

// SAFETY: no `RefCell` is mutably borrowed between files, where collection runs.
unsafe impl<T: ?Sized + Trace> Trace for RefCell<T> {
    const HOLDS_EVAL: bool = T::HOLDS_EVAL;
    const HAS_CELL: bool = T::HOLDS_EVAL;
    const CELL_OUT_OF_LINE: bool = T::CELL_OUT_OF_LINE;
    #[inline]
    fn prefetch_owned(&self) {
        // SAFETY: as in `trace`; a dead object's cell is never borrowed.
        if let Ok(v) = unsafe { self.try_borrow_unguarded() } {
            v.prefetch_owned();
        }
    }
    #[inline]
    fn trace(&self, t: &mut Tracer) {
        // SAFETY: the reference does not outlive this call, and tracing never writes.
        match unsafe { self.try_borrow_unguarded() } {
            Ok(v) => v.trace(t),
            Err(_) => panic!("collector: a RefCell<{}> is mutably borrowed during collection", std::any::type_name::<T>()),
        }
    }
}

// SAFETY: traces the value once set.
unsafe impl<T: Trace> Trace for OnceCell<T> {
    const HOLDS_EVAL: bool = T::HOLDS_EVAL;
    const HAS_CELL: bool = T::HOLDS_EVAL;
    const CELL_OUT_OF_LINE: bool = T::CELL_OUT_OF_LINE;
    #[inline]
    fn prefetch_owned(&self) {
        if let Some(v) = self.get() {
            v.prefetch_owned();
        }
    }
    #[inline]
    fn trace(&self, t: &mut Tracer) {
        if let Some(v) = self.get() {
            v.trace(t);
        }
    }
}

/// `RefCell<Option<T>>` in one pointer: the storage is allocated on the first `borrow_mut`, so a
/// cell that is never written costs 8 bytes. For optional records most objects never set, as
/// pyright leaves `type.props` and `type.cached` undefined until the first write.
/// The storage is a buffer, so `borrow_mut` runs the write barrier for the cell's own address.
pub struct OptionRefCell<T>(std::cell::OnceCell<Box<std::cell::RefCell<Option<T>>>>);

// SAFETY: as `RefCell`. The boxed `RefCell` sits outside the owner's slot, but every
// write to it goes through `OptionRefCell::borrow_mut`, which runs the barrier. A cell
// inside `T` sits in the box too, where a write through it marks no card of the owner.
unsafe impl<T: Trace> Trace for OptionRefCell<T> {
    const HOLDS_EVAL: bool = T::HOLDS_EVAL;
    const HAS_CELL: bool = T::HOLDS_EVAL;
    const CELL_OUT_OF_LINE: bool = T::HAS_CELL;
    #[inline]
    fn prefetch_owned(&self) {
        if let Some(cell) = self.0.get() {
            heap::prefetch(ptr::from_ref::<std::cell::RefCell<Option<T>>>(cell).cast::<u8>() as usize);
        }
    }
    #[inline]
    fn trace(&self, t: &mut Tracer) {
        if let Some(cell) = self.0.get() {
            // SAFETY: as in `RefCell`'s impl.
            match unsafe { cell.try_borrow_unguarded() } {
                Ok(v) => v.trace(t),
                Err(_) => panic!("collector: an OptionRefCell<{}> is mutably borrowed during collection", std::any::type_name::<T>()),
            }
        }
    }
}

/// A shared borrow of an [`OptionRefCell`]: `None` while the cell is unallocated.
pub enum OptionRef<'a, T> {
    Unallocated,
    Borrowed(Ref<'a, Option<T>>),
}

impl<T> Deref for OptionRef<'_, T> {
    type Target = Option<T>;

    fn deref(&self) -> &Option<T> {
        match self {
            OptionRef::Unallocated => &None,
            OptionRef::Borrowed(value) => value,
        }
    }
}

impl<T> OptionRefCell<T> {
    pub const fn new() -> Self {
        OptionRefCell(std::cell::OnceCell::new())
    }

    /// Panics while a `borrow_mut` of an allocated cell is live, as `RefCell::borrow` does.
    pub fn borrow(&self) -> OptionRef<'_, T> {
        match self.0.get() {
            None => OptionRef::Unallocated,
            Some(cell) => OptionRef::Borrowed(cell.borrow()),
        }
    }

    /// Allocates the storage on first use. Panics while another borrow of it is live.
    pub fn borrow_mut(&self) -> RefMut<'_, Option<T>>
    where
        T: Trace,
    {
        remember::<T, _>(self);
        self.0.get_or_init(|| Box::new(std::cell::RefCell::new(None))).borrow_mut()
    }
}

/// A cell holding `value`, allocated only when it is `Some`.
impl<T: Trace> From<Option<T>> for OptionRefCell<T> {
    fn from(value: Option<T>) -> Self {
        let cell = OptionRefCell::new();
        if value.is_some() {
            *cell.borrow_mut() = value;
        }
        cell
    }
}

impl<T> Default for OptionRefCell<T> {
    fn default() -> Self {
        OptionRefCell::new()
    }
}

/// A copy that stays unallocated while the value is `None`.
impl<T: Clone + Trace> Clone for OptionRefCell<T> {
    fn clone(&self) -> Self {
        let copy = OptionRefCell::new();
        if let Some(value) = self.borrow().as_ref() {
            *copy.borrow_mut() = Some(value.clone());
        }
        copy
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn option_ref_cell_allocates_on_first_write() {
        assert_eq!(size_of::<OptionRefCell<[u64; 8]>>(), size_of::<usize>());
        let cell: OptionRefCell<String> = OptionRefCell::new();
        assert!(cell.borrow().is_none());
        assert!(cell.clone().0.get().is_none());
        cell.borrow_mut().get_or_insert_with(String::new).push('a');
        assert_eq!(cell.borrow().as_deref(), Some("a"));
        assert_eq!(cell.clone().borrow().as_deref(), Some("a"));
        *cell.borrow_mut() = None;
        assert!(cell.borrow().is_none() && cell.clone().0.get().is_none());
    }

    #[test]
    #[should_panic]
    fn option_ref_cell_borrow_checks_once_allocated() {
        let cell: OptionRefCell<u32> = OptionRefCell::new();
        *cell.borrow_mut() = Some(1);
        let _read = cell.borrow();
        let _write = cell.borrow_mut();
    }
}
