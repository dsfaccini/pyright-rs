//! Interned strings. An `Atom` is a handle to text its thread interned once:
//! two atoms of one thread are equal exactly when their texts are, so hashing
//! and comparing one is integer work (`docs/porting.md`, "strings").
//!
//! Each thread keeps its own table, so an atom is neither `Send` nor `Sync`,
//! like `P`. The text is never freed.

use std::borrow::Borrow;
use std::cell::RefCell;
use std::collections::HashSet;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::ops::Deref;
use std::ptr::NonNull;

use rustc_hash::FxBuildHasher;

use crate::heap::Tracer;
use crate::trace::Trace;

/// The length of an atom's text; the UTF-8 bytes follow it.
#[repr(C)]
struct Header {
    len: u32,
}

/// An interned string. Equality and hashing use its identity; order and
/// printing use its text ([`Atom::as_str`]).
#[derive(Clone, Copy)]
pub struct Atom(NonNull<Header>, PhantomData<*const ()>);

impl Atom {
    /// The atom of `text`, interning it on first use.
    pub fn new(text: &str) -> Atom {
        INTERNER.with(|interner| interner.borrow_mut().intern(text))
    }

    /// The atom of `text` if this thread interned it: a key no atom has
    /// cannot be in a table keyed by atoms.
    pub fn lookup(text: &str) -> Option<Atom> {
        INTERNER.with(|interner| interner.borrow().set.get(text).map(|entry| entry.0))
    }

    #[inline]
    pub fn as_str(self) -> &'static str {
        // SAFETY: an atom points to a header of its interner's leaked chunks,
        // followed by `len` bytes copied from a `str`.
        unsafe {
            let len = (*self.0.as_ptr()).len as usize;
            let bytes = self.0.as_ptr().cast::<u8>().add(size_of::<Header>());
            std::str::from_utf8_unchecked(std::slice::from_raw_parts(bytes, len))
        }
    }
}

impl PartialEq for Atom {
    #[inline]
    fn eq(&self, other: &Atom) -> bool {
        self.0 == other.0
    }
}

impl Eq for Atom {}

impl Hash for Atom {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_usize(self.0.as_ptr() as usize);
    }
}

impl PartialEq<str> for Atom {
    #[inline]
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl PartialEq<&str> for Atom {
    #[inline]
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl PartialEq<Atom> for str {
    #[inline]
    fn eq(&self, other: &Atom) -> bool {
        self == other.as_str()
    }
}

impl PartialEq<Atom> for &str {
    #[inline]
    fn eq(&self, other: &Atom) -> bool {
        *self == other.as_str()
    }
}

impl Deref for Atom {
    type Target = str;
    #[inline]
    fn deref(&self) -> &str {
        self.as_str()
    }
}

impl AsRef<str> for Atom {
    #[inline]
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl From<&str> for Atom {
    fn from(text: &str) -> Atom {
        Atom::new(text)
    }
}

impl From<&String> for Atom {
    fn from(text: &String) -> Atom {
        Atom::new(text)
    }
}

impl From<String> for Atom {
    fn from(text: String) -> Atom {
        Atom::new(&text)
    }
}

impl From<Atom> for String {
    fn from(atom: Atom) -> String {
        atom.as_str().to_string()
    }
}

impl fmt::Display for Atom {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self.as_str(), f)
    }
}

impl fmt::Debug for Atom {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_str(), f)
    }
}

impl Default for Atom {
    fn default() -> Atom {
        crate::atom!("")
    }
}

unsafe impl Trace for Atom {
    const HOLDS_EVAL: bool = false;
    #[inline]
    fn trace(&self, _: &mut Tracer) {}
}

/// The atom of a string literal, interned once per call site and thread.
#[macro_export]
macro_rules! atom {
    ($text:literal) => {{
        ::std::thread_local! {
            static ATOM: ::std::cell::Cell<::std::option::Option<$crate::atom::Atom>> =
                const { ::std::cell::Cell::new(None) };
        }
        ATOM.with(|cell| match cell.get() {
            Some(atom) => atom,
            None => {
                let atom = $crate::atom::Atom::new($text);
                cell.set(Some(atom));
                atom
            }
        })
    }};
}

/// A table entry: hashed and compared by text, so the table finds a `&str`.
struct ByText(Atom);

impl PartialEq for ByText {
    fn eq(&self, other: &ByText) -> bool {
        self.0 == other.0
    }
}

impl Eq for ByText {}

impl Hash for ByText {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.as_str().hash(state);
    }
}

impl Borrow<str> for ByText {
    fn borrow(&self) -> &str {
        self.0.as_str()
    }
}

const CHUNK_SIZE: usize = 64 * 1024;

struct Interner {
    set: HashSet<ByText, FxBuildHasher>,
    /// The free part of the current chunk.
    next: *mut u8,
    end: *mut u8,
}

impl Interner {
    fn intern(&mut self, text: &str) -> Atom {
        if let Some(entry) = self.set.get(text) {
            return entry.0;
        }
        let atom = self.alloc(text);
        self.set.insert(ByText(atom));
        atom
    }

    fn alloc(&mut self, text: &str) -> Atom {
        let len = u32::try_from(text.len()).expect("an interned string is shorter than 4 GiB");
        let size = (size_of::<Header>() + text.len()).next_multiple_of(align_of::<Header>());
        if self.end as usize - (self.next as usize) < size {
            let chunk = Box::leak(vec![0u32; size.max(CHUNK_SIZE) / size_of::<u32>()].into_boxed_slice());
            self.next = chunk.as_mut_ptr().cast::<u8>();
            // SAFETY: one past the end of the chunk.
            self.end = unsafe { self.next.add(size_of_val(chunk)) };
        }
        let header = self.next.cast::<Header>();
        // SAFETY: `size` bytes from `next` are free and aligned for `Header`; the
        // chunk is leaked, so the text lives as long as the thread's atoms.
        unsafe {
            header.write(Header { len });
            std::ptr::copy_nonoverlapping(text.as_ptr(), self.next.add(size_of::<Header>()), text.len());
            self.next = self.next.add(size);
            Atom(NonNull::new_unchecked(header), PhantomData)
        }
    }
}

thread_local! {
    static INTERNER: RefCell<Interner> = const {
        RefCell::new(Interner {
            set: HashSet::with_hasher(FxBuildHasher),
            next: std::ptr::null_mut(),
            end: std::ptr::null_mut(),
        })
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_text_same_atom() {
        let a = Atom::new("name");
        let b = Atom::new(&String::from("name"));
        assert_eq!(a, b);
        assert_eq!(a.as_str(), "name");
        assert_ne!(a, Atom::new("other"));
        assert_eq!(atom!("name"), a);
        assert_eq!(Atom::lookup("name"), Some(a));
        assert_eq!(Atom::lookup("never interned here"), None);
        assert_eq!(Atom::new("").as_str(), "");
        assert_eq!(Atom::default(), Atom::new(""));
    }

    #[test]
    fn long_and_many_atoms() {
        let long = "x".repeat(3 * CHUNK_SIZE);
        let atom = Atom::new(&long);
        let many: Vec<Atom> = (0..20_000).map(|i| Atom::new(&format!("n{i}"))).collect();
        assert_eq!(atom.as_str(), long);
        for (i, atom) in many.iter().enumerate() {
            assert_eq!(atom.as_str(), format!("n{i}"));
            assert_eq!(*atom, Atom::new(&format!("n{i}")));
        }
        assert_eq!(Atom::new("é\u{10000}").as_str(), "é\u{10000}");
    }

    #[test]
    fn threads_have_their_own_tables() {
        let here = Atom::new("shared text").as_str().as_ptr() as usize;
        let there = std::thread::spawn(|| Atom::new("shared text").as_str().as_ptr() as usize).join().unwrap();
        assert_ne!(here, there);
    }
}
