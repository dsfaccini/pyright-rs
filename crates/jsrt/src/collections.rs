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

/// Slots per [`IdMap`] page.
const ID_MAP_PAGE: usize = 32;

/// A map keyed by a dense `u32` id, such as a parse node id: lookup-only, like
/// [`FxHashMap`], for maps that are read and written by id and never iterated. Ids are
/// grouped in pages of `ID_MAP_PAGE` slots, each allocated on the first insert into its
/// range; the page table spans only the pages between the lowest and the highest id
/// inserted. A lookup is two loads, and a page half full holds an entry in twice its size,
/// where a hash map holds it in its size plus key, padding and spare buckets.
#[derive(jsrt::Trace)]
pub struct IdMap<V> {
    /// The page number of `pages[0]`.
    first_page: usize,
    pages: Vec<Option<Box<[Option<V>; ID_MAP_PAGE]>>>,
    len: usize,
}

impl<V> Default for IdMap<V> {
    fn default() -> Self {
        IdMap { first_page: 0, pages: Vec::new(), len: 0 }
    }
}

impl<V> IdMap<V> {
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[inline]
    pub fn get(&self, id: u32) -> Option<&V> {
        let id = id as usize;
        let page = (id / ID_MAP_PAGE).wrapping_sub(self.first_page);
        self.pages.get(page)?.as_ref()?[id % ID_MAP_PAGE].as_ref()
    }

    pub fn contains_key(&self, id: u32) -> bool {
        self.get(id).is_some()
    }

    /// The previous value of `id`, as `HashMap::insert` returns it.
    pub fn insert(&mut self, id: u32, value: V) -> Option<V> {
        let id = id as usize;
        let page = id / ID_MAP_PAGE;
        if self.pages.is_empty() {
            self.first_page = page;
        } else if page < self.first_page {
            self.pages.splice(0..0, std::iter::repeat_with(|| None).take(self.first_page - page));
            self.first_page = page;
        }
        let index = page - self.first_page;
        if index >= self.pages.len() {
            self.pages.resize_with(index + 1, || None);
        }
        let slots = self.pages[index].get_or_insert_with(|| Box::new([const { None }; ID_MAP_PAGE]));
        let old = slots[id % ID_MAP_PAGE].replace(value);
        if old.is_none() {
            self.len += 1;
        }
        old
    }

    /// Removes `id`'s value and returns it. Its page stays allocated.
    pub fn remove(&mut self, id: u32) -> Option<V> {
        let id = id as usize;
        let page = (id / ID_MAP_PAGE).wrapping_sub(self.first_page);
        let old = self.pages.get_mut(page)?.as_mut()?[id % ID_MAP_PAGE].take();
        if old.is_some() {
            self.len -= 1;
        }
        old
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_map_reads_what_a_hash_map_reads() {
        let mut map: IdMap<u32> = IdMap::default();
        let mut reference: FxHashMap<u32, u32> = FxHashMap::default();
        assert!(map.is_empty() && map.get(5).is_none() && map.remove(5).is_none());
        // Ids above, below and inside the pages already allocated, repeated and removed.
        let ids = [5000u32, 5001, 12, 70000, 255, 256, 0, 5000, 69999, 200_000, 12];
        for (value, &id) in ids.iter().enumerate() {
            assert_eq!(map.insert(id, value as u32), reference.insert(id, value as u32));
            assert_eq!(map.len(), reference.len());
        }
        for id in [0u32, 12, 255, 256, 257, 4999, 5000, 5001, 69999, 70000, 70001, 200_000, u32::MAX] {
            assert_eq!(map.get(id), reference.get(&id));
        }
        for id in [12u32, 12, 70000, 3, u32::MAX] {
            assert_eq!(map.remove(id), reference.remove(&id));
            assert_eq!((map.len(), map.contains_key(id)), (reference.len(), false));
        }
        assert_eq!(map.get(5000), Some(&7));
    }

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
