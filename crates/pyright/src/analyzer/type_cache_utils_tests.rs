//! Port of `packages/pyright-internal/src/tests/typeCacheUtils.test.ts`.

use crate::analyzer::type_cache_utils::{
    ContextualTypeCacheEntry, add_contextual_type_cache_entry, contextual_type_cache_entry_matches,
};
use crate::analyzer::types::{Type, TypeVarKind, TypeVarType};
use jsrt::P;

#[derive(Clone)]
struct TestCacheEntry {
    expected_type: Option<P<Type>>,
    value: i32,
}

impl ContextualTypeCacheEntry for TestCacheEntry {
    fn expected_type(&self) -> Option<P<Type>> {
        self.expected_type
    }
}

fn values(entries: &[TestCacheEntry]) -> Vec<i32> {
    entries.iter().map(|entry| entry.value).collect()
}

#[test]
fn contextual_type_cache_entry_matching() {
    let expected_type = TypeVarType::create_instance("T", TypeVarKind::TypeVar).0;
    let other_expected_type = TypeVarType::create_instance("U", TypeVarKind::TypeVar).0;
    let entry = TestCacheEntry {
        expected_type: Some(expected_type),
        value: 1,
    };
    let no_expected_type_entry = TestCacheEntry {
        expected_type: None,
        value: 2,
    };

    assert!(contextual_type_cache_entry_matches(
        &entry,
        Some(expected_type)
    ));
    assert!(!contextual_type_cache_entry_matches(
        &entry,
        Some(other_expected_type)
    ));
    assert!(!contextual_type_cache_entry_matches(&entry, None));
    assert!(contextual_type_cache_entry_matches(
        &no_expected_type_entry,
        None
    ));
}

#[test]
fn contextual_type_cache_entry_replacement_and_eviction() {
    let expected_types: Vec<P<Type>> = (0..9)
        .map(|index| TypeVarType::create_instance(&format!("T{index}"), TypeVarKind::TypeVar).0)
        .collect();
    let mut entries: Vec<TestCacheEntry> = Vec::new();

    for (index, expected_type) in expected_types.iter().enumerate() {
        entries = add_contextual_type_cache_entry(
            &entries,
            TestCacheEntry {
                expected_type: Some(*expected_type),
                value: index as i32,
            },
            None,
        );
    }

    assert_eq!(values(&entries), [1, 2, 3, 4, 5, 6, 7, 8]);

    entries = add_contextual_type_cache_entry(
        &entries,
        TestCacheEntry {
            expected_type: Some(expected_types[4]),
            value: 9,
        },
        None,
    );
    assert_eq!(values(&entries), [1, 2, 3, 5, 6, 7, 8, 9]);

    entries = add_contextual_type_cache_entry(
        &entries,
        TestCacheEntry {
            expected_type: None,
            value: 10,
        },
        Some(&mut |entry: TestCacheEntry| entry.value != 2),
    );
    assert_eq!(values(&entries), [1, 3, 5, 6, 7, 8, 9, 10]);
}
