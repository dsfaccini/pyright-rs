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
