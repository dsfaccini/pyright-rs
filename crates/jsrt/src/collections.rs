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
