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

/// JS `Map` that allocates its table on the first insert, so an empty one is one pointer.
/// For maps most of their owners never fill (`ConstraintSet._typeVarMap`). Iteration order and
/// lookups are those of [`JsMap`].
#[derive(Clone, Debug, jsrt::Trace)]
pub struct LazyJsMap<K, V>(Option<Box<JsMap<K, V>>>);

impl<K, V> Default for LazyJsMap<K, V> {
    fn default() -> Self {
        LazyJsMap(None)
    }
}

impl<K: std::hash::Hash + Eq, V> LazyJsMap<K, V> {
    pub fn len(&self) -> usize {
        self.0.as_ref().map_or(0, |map| map.len())
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn get<Q>(&self, key: &Q) -> Option<&V>
    where
        Q: ?Sized + std::hash::Hash + indexmap::Equivalent<K>,
    {
        self.0.as_ref()?.get(key)
    }

    pub fn contains_key<Q>(&self, key: &Q) -> bool
    where
        Q: ?Sized + std::hash::Hash + indexmap::Equivalent<K>,
    {
        self.0.as_ref().is_some_and(|map| map.contains_key(key))
    }

    pub fn get_index(&self, index: usize) -> Option<(&K, &V)> {
        self.0.as_ref()?.get_index(index)
    }

    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        self.0.get_or_insert_with(Default::default).insert(key, value)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        self.0.iter().flat_map(|map| map.iter())
    }

    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.0.iter().flat_map(|map| map.values())
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
}
