//! Interior-mutable cells sized for objects created by the million.

use std::cell::{OnceCell, Ref, RefCell, RefMut};
use std::ops::Deref;

/// `RefCell<Option<T>>` in one pointer: the storage is allocated on the first `borrow_mut`, so a
/// cell that is never written costs 8 bytes. For optional records most objects never set, as
/// pyright leaves `type.props` and `type.cached` undefined until the first write.
pub struct OptionRefCell<T>(OnceCell<Box<RefCell<Option<T>>>>);

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
        OptionRefCell(OnceCell::new())
    }

    /// Panics while a `borrow_mut` of an allocated cell is live, as `RefCell::borrow` does.
    pub fn borrow(&self) -> OptionRef<'_, T> {
        match self.0.get() {
            None => OptionRef::Unallocated,
            Some(cell) => OptionRef::Borrowed(cell.borrow()),
        }
    }

    /// Allocates the storage on first use. Panics while another borrow of it is live.
    pub fn borrow_mut(&self) -> RefMut<'_, Option<T>> {
        self.0.get_or_init(|| Box::new(RefCell::new(None))).borrow_mut()
    }
}

impl<T> Default for OptionRefCell<T> {
    fn default() -> Self {
        OptionRefCell::new()
    }
}

/// A copy that stays unallocated while the value is `None`.
impl<T: Clone> Clone for OptionRefCell<T> {
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
