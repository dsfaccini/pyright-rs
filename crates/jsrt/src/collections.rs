//! Insertion-ordered collections: the replacements for JS `Map` and `Set`.

use rustc_hash::FxBuildHasher;

/// JS `Map`. Remove entries with `shift_remove`: `swap_remove` reorders the map.
pub type JsMap<K, V> = indexmap::IndexMap<K, V, FxBuildHasher>;

/// JS `Set`. Remove entries with `shift_remove`: `swap_remove` reorders the set.
pub type JsSet<T> = indexmap::IndexSet<T, FxBuildHasher>;

/// Lookup-only hash map. Never iterate one: its order is not JS order.
pub type FxHashMap<K, V> = rustc_hash::FxHashMap<K, V>;

/// Lookup-only hash set. Never iterate one: its order is not JS order.
pub type FxHashSet<T> = rustc_hash::FxHashSet<T>;

/// JS `Map` for few entries: searched linearly, and allocated on the first insert, so an
/// empty one is one pointer. For maps most of their owners never fill, and fill with a handful
/// of entries (`ConstraintSet._typeVarMap`). Iteration order and lookups are those of [`JsMap`].
#[derive(Clone, Debug, jsrt::Trace)]
// The box keeps an empty map one pointer wide; a bare `Vec` is three.
#[allow(clippy::box_collection)]
pub struct LazyJsMap<K, V>(Option<Box<Vec<(K, V)>>>);

impl<K, V> Default for LazyJsMap<K, V> {
    fn default() -> Self {
        LazyJsMap(None)
    }
}

impl<K: Eq, V> LazyJsMap<K, V> {
    pub fn len(&self) -> usize {
        self.0.as_ref().map_or(0, |map| map.len())
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn position<Q>(&self, key: &Q) -> Option<usize>
    where
        K: std::borrow::Borrow<Q>,
        Q: ?Sized + Eq,
    {
        self.0.as_ref()?.iter().position(|(k, _)| k.borrow() == key)
    }

    pub fn get<Q>(&self, key: &Q) -> Option<&V>
    where
        K: std::borrow::Borrow<Q>,
        Q: ?Sized + Eq,
    {
        let index = self.position(key)?;
        self.0.as_ref().map(|map| &map[index].1)
    }

    pub fn contains_key<Q>(&self, key: &Q) -> bool
    where
        K: std::borrow::Borrow<Q>,
        Q: ?Sized + Eq,
    {
        self.position(key).is_some()
    }

    pub fn get_mut<Q>(&mut self, key: &Q) -> Option<&mut V>
    where
        K: std::borrow::Borrow<Q>,
        Q: ?Sized + Eq,
    {
        let index = self.position(key)?;
        self.0.as_mut().map(|map| &mut map[index].1)
    }

    pub fn get_index(&self, index: usize) -> Option<(&K, &V)> {
        self.0.as_ref()?.get(index).map(|(k, v)| (k, v))
    }

    /// `map.set(key, value)`: a new key goes last; an existing key keeps its place.
    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        match self.position(&key) {
            Some(index) => Some(std::mem::replace(&mut self.0.as_mut().unwrap()[index].1, value)),
            None => {
                self.0.get_or_insert_with(|| Box::new(Vec::with_capacity(4))).push((key, value));
                None
            }
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        self.0.iter().flat_map(|map| map.iter().map(|(k, v)| (k, v)))
    }

    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.0.iter().flat_map(|map| map.iter().map(|(_, v)| v))
    }
}

/// JS `Set` for few elements: a vector searched linearly. Iteration order and membership
/// are those of [`JsSet`].
#[derive(Clone, Debug, jsrt::Trace)]
pub struct SmallJsSet<T>(Vec<T>);

impl<T> Default for SmallJsSet<T> {
    fn default() -> Self {
        SmallJsSet(Vec::new())
    }
}

impl<T: Eq> SmallJsSet<T> {
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn contains<Q>(&self, value: &Q) -> bool
    where
        T: std::borrow::Borrow<Q>,
        Q: ?Sized + Eq,
    {
        self.0.iter().any(|v| v.borrow() == value)
    }

    /// `set.add(value)`: returns whether `value` was new.
    pub fn insert(&mut self, value: T) -> bool {
        if self.contains(&value) {
            return false;
        }
        self.0.push(value);
        true
    }

    /// `set.delete(value)`: returns whether `value` was present.
    pub fn shift_remove<Q>(&mut self, value: &Q) -> bool
    where
        T: std::borrow::Borrow<Q>,
        Q: ?Sized + Eq,
    {
        match self.0.iter().position(|v| v.borrow() == value) {
            Some(index) => {
                self.0.remove(index);
                true
            }
            None => false,
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.0.iter()
    }
}

/// An array that always holds at least one `Copy` value and is replaced whole, never
/// edited: the first value sits inline, so a one-value array needs no allocation, and a
/// copy is a value copy or a reference count increment.
#[derive(Clone, Debug, jsrt::Trace)]
pub struct NonEmptyList<T: Copy> {
    first: T,
    rest: Option<std::rc::Rc<Vec<T>>>,
}

#[allow(clippy::len_without_is_empty, reason = "never empty")]
impl<T: Copy> NonEmptyList<T> {
    pub fn one(first: T) -> Self {
        NonEmptyList { first, rest: None }
    }

    /// `None` for an empty slice.
    pub fn from_slice(items: &[T]) -> Option<Self> {
        let (&first, rest) = items.split_first()?;
        Some(NonEmptyList { first, rest: (!rest.is_empty()).then(|| std::rc::Rc::new(rest.to_vec())) })
    }

    /// A list of `f` applied to each value, in order.
    pub fn map(&self, mut f: impl FnMut(T) -> T) -> Self {
        let first = f(self.first);
        NonEmptyList { first, rest: self.rest.as_ref().map(|rest| std::rc::Rc::new(rest.iter().map(|&v| f(v)).collect())) }
    }

    pub fn first(&self) -> T {
        self.first
    }

    pub fn len(&self) -> usize {
        1 + self.rest.as_ref().map_or(0, |rest| rest.len())
    }

    pub fn get(&self, index: usize) -> Option<T> {
        match index {
            0 => Some(self.first),
            _ => self.rest.as_ref().and_then(|rest| rest.get(index - 1).copied()),
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = T> + '_ {
        std::iter::once(self.first).chain(self.rest.iter().flat_map(|rest| rest.iter().copied()))
    }

    pub fn to_vec(&self) -> Vec<T> {
        self.iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lazy_map_allocates_on_first_insert() {
        let mut map: LazyJsMap<String, u32> = LazyJsMap::default();
        assert_eq!(size_of::<LazyJsMap<String, u32>>(), size_of::<usize>());
        assert!(map.is_empty() && map.get("a").is_none() && map.iter().next().is_none());
        map.insert("b".to_string(), 1);
        map.insert("a".to_string(), 2);
        assert_eq!(map.insert("b".to_string(), 3), Some(1));
        assert_eq!(map.iter().map(|(k, v)| (k.as_str(), *v)).collect::<Vec<_>>(), [("b", 3), ("a", 2)]);
        assert_eq!(map.get_index(1), Some((&"a".to_string(), &2)));
        assert!(map.contains_key("a") && map.len() == 2);
    }

    #[test]
    fn non_empty_list_keeps_one_value_inline() {
        assert!(NonEmptyList::<u32>::from_slice(&[]).is_none());
        let one = NonEmptyList::one(7u32);
        assert!(one.rest.is_none() && one.len() == 1 && one.get(1).is_none());
        let three = NonEmptyList::from_slice(&[1u32, 2, 3]).unwrap();
        assert_eq!((three.first(), three.len(), three.get(2)), (1, 3, Some(3)));
        assert_eq!(three.map(|v| v * 10).to_vec(), [10, 20, 30]);
        assert!(NonEmptyList::from_slice(&[4u32]).unwrap().rest.is_none());
    }

    #[test]
    fn small_set_keeps_insertion_order() {
        let mut set: SmallJsSet<String> = SmallJsSet::default();
        assert!(set.insert("b".to_string()) && set.insert("a".to_string()));
        assert!(!set.insert("b".to_string()));
        assert!(set.contains("a") && set.len() == 2);
        assert!(set.shift_remove("b") && !set.shift_remove("b"));
        assert!(set.insert("b".to_string()));
        assert_eq!(set.iter().map(String::as_str).collect::<Vec<_>>(), ["a", "b"]);
    }
}
