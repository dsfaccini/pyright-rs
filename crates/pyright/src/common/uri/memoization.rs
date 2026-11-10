//! Port of `packages/pyright-internal/src/common/uri/memoization.ts`.
//!
//! `cacheProperty` and `cacheMethodWithNoArgs` memoize per object; the Uri
//! types port them as `OnceCell` fields. `cacheStaticFunc` is one global LRU
//! shared by every decorated static function, and it decides which calls
//! return the same object, so it is ported entry for entry.

use std::cell::RefCell;
use std::collections::BTreeMap;

use jsrt::FxHashMap;

use crate::common::uri::Uri;

// common/uri/memoization.ts:17
const MAX_STATIC_CACHE_ENTRIES: usize = 8192;

/// `staticCache`: a JS `Map` used as an LRU. Insertion order is kept as a
/// use counter, so promotion and eviction need no list walk.
#[derive(Default)]
struct StaticCache {
    entries: FxHashMap<String, (Uri, u64)>,
    order: BTreeMap<u64, String>,
    next_use: u64,
}

thread_local! {
    // common/uri/memoization.ts:10
    static STATIC_CACHE: RefCell<StaticCache> = RefCell::default();
}

// common/uri/memoization.ts:61
/// The wrapper `cacheStaticFunc` puts around a static method. `key` is
/// `` `${functionName}+${args?.map((a) => a?.toString()).join(',')}` ``;
/// `compute` is the original method.
pub fn cache_static_func(key: String, compute: impl FnOnce() -> Uri) -> Uri {
    let hit = STATIC_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        let next_use = cache.next_use;
        let (value, last_use) = cache.entries.get_mut(&key).map(|entry| {
            let last_use = entry.1;
            entry.1 = next_use;
            (entry.0, last_use)
        })?;

        // Promote to most-recently used by re-inserting.
        let key = cache.order.remove(&last_use).unwrap();
        cache.order.insert(next_use, key);
        cache.next_use += 1;
        Some(value)
    });
    if let Some(value) = hit {
        return value;
    }

    // Miss: compute and insert, evict LRU if over capacity.
    let result = compute();

    STATIC_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.entries.len() >= MAX_STATIC_CACHE_ENTRIES {
            // Remove least-recently used (the first key in insertion order).
            if let Some((_, lru_key)) = cache.order.pop_first() {
                cache.entries.remove(&lru_key);
            }
        }

        let next_use = cache.next_use;
        cache.next_use += 1;
        cache.order.insert(next_use, key.clone());
        cache.entries.insert(key, (result, next_use));
    });
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::uri::file_uri::create_file_uri;

    #[test]
    fn evicts_least_recently_used_and_promotes_on_hit() {
        let first = create_file_uri("/lru/first", "", "", None, true);
        // A hit returns the same object and makes it most recently used.
        for i in 0..MAX_STATIC_CACHE_ENTRIES - 1 {
            create_file_uri(&format!("/lru/a{i}"), "", "", None, true);
            assert!(Uri::ptr_eq(create_file_uri("/lru/first", "", "", None, true), first));
        }

        // 8191 more entries push out the `a` entries, not `first`.
        for i in 0..MAX_STATIC_CACHE_ENTRIES - 1 {
            create_file_uri(&format!("/lru/b{i}"), "", "", None, true);
        }
        assert!(Uri::ptr_eq(create_file_uri("/lru/first", "", "", None, true), first));

        // Without a hit, 8192 new entries evict it: the next call builds a new object.
        for i in 0..MAX_STATIC_CACHE_ENTRIES {
            create_file_uri(&format!("/lru/c{i}"), "", "", None, true);
        }
        let again = create_file_uri("/lru/first", "", "", None, true);
        assert!(!Uri::ptr_eq(again, first));
        assert!(again == first);
    }

    #[test]
    fn the_key_includes_every_argument() {
        let plain = create_file_uri("/key/x", "", "", None, true);
        assert!(Uri::ptr_eq(create_file_uri("/key/x", "", "", Some(""), true), plain));
        assert!(!Uri::ptr_eq(create_file_uri("/key/x", "", "", Some("file:///key/x"), true), plain));
        assert!(!Uri::ptr_eq(create_file_uri("/key/x", "", "", None, false), plain));
    }
}
