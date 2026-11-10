//! Port of `packages/pyright-internal/src/analyzer/deprecatedSymbols.ts`.
//! A list of implicitly-deprecated symbols as defined in PEP 585, etc.

use crate::common::python_version::{PYTHON_VERSION3_10, PYTHON_VERSION3_9, PythonVersion};
use jsrt::JsMap;

// analyzer/deprecatedSymbols.ts:12
#[derive(Clone)]
pub struct DeprecatedForm {
    pub version: PythonVersion,
    pub full_name: String,
    pub replacement_text: String,
    pub typing_import_only: Option<bool>,
}

thread_local! {
    // analyzer/deprecatedSymbols.ts:26
    pub static DEPRECATED_ALIASES: JsMap<String, DeprecatedForm> = {
        let mut map = JsMap::default();
        map.insert(
            "Tuple".to_string(),
            DeprecatedForm {
                version: PYTHON_VERSION3_9,
                full_name: "builtins.tuple".to_string(),
                replacement_text: "tuple".to_string(),
                typing_import_only: None,
            },
        );
        map.insert(
            "List".to_string(),
            DeprecatedForm {
                version: PYTHON_VERSION3_9,
                full_name: "builtins.list".to_string(),
                replacement_text: "list".to_string(),
                typing_import_only: None,
            },
        );
        map.insert(
            "Dict".to_string(),
            DeprecatedForm {
                version: PYTHON_VERSION3_9,
                full_name: "builtins.dict".to_string(),
                replacement_text: "dict".to_string(),
                typing_import_only: None,
            },
        );
        map.insert(
            "Set".to_string(),
            DeprecatedForm {
                version: PYTHON_VERSION3_9,
                full_name: "builtins.set".to_string(),
                replacement_text: "set".to_string(),
                typing_import_only: None,
            },
        );
        map.insert(
            "FrozenSet".to_string(),
            DeprecatedForm {
                version: PYTHON_VERSION3_9,
                full_name: "builtins.frozenset".to_string(),
                replacement_text: "frozenset".to_string(),
                typing_import_only: None,
            },
        );
        map.insert(
            "Type".to_string(),
            DeprecatedForm {
                version: PYTHON_VERSION3_9,
                full_name: "builtins.type".to_string(),
                replacement_text: "type".to_string(),
                typing_import_only: None,
            },
        );
        map.insert(
            "Deque".to_string(),
            DeprecatedForm {
                version: PYTHON_VERSION3_9,
                full_name: "collections.deque".to_string(),
                replacement_text: "collections.deque".to_string(),
                typing_import_only: None,
            },
        );
        map.insert(
            "DefaultDict".to_string(),
            DeprecatedForm {
                version: PYTHON_VERSION3_9,
                full_name: "collections.defaultdict".to_string(),
                replacement_text: "collections.defaultdict".to_string(),
                typing_import_only: None,
            },
        );
        map.insert(
            "OrderedDict".to_string(),
            DeprecatedForm {
                version: PYTHON_VERSION3_9,
                full_name: "collections.OrderedDict".to_string(),
                replacement_text: "collections.OrderedDict".to_string(),
                typing_import_only: Some(true),
            },
        );
        map.insert(
            "Counter".to_string(),
            DeprecatedForm {
                version: PYTHON_VERSION3_9,
                full_name: "collections.Counter".to_string(),
                replacement_text: "collections.Counter".to_string(),
                typing_import_only: Some(true),
            },
        );
        map.insert(
            "ChainMap".to_string(),
            DeprecatedForm {
                version: PYTHON_VERSION3_9,
                full_name: "collections.ChainMap".to_string(),
                replacement_text: "collections.ChainMap".to_string(),
                typing_import_only: Some(true),
            },
        );
        map.insert(
            "Awaitable".to_string(),
            DeprecatedForm {
                version: PYTHON_VERSION3_9,
                full_name: "typing.Awaitable".to_string(),
                replacement_text: "collections.abc.Awaitable".to_string(),
                typing_import_only: Some(true),
            },
        );
        map.insert(
            "Coroutine".to_string(),
            DeprecatedForm {
                version: PYTHON_VERSION3_9,
                full_name: "typing.Coroutine".to_string(),
                replacement_text: "collections.abc.Coroutine".to_string(),
                typing_import_only: Some(true),
            },
        );
        map.insert(
            "AsyncIterable".to_string(),
            DeprecatedForm {
                version: PYTHON_VERSION3_9,
                full_name: "typing.AsyncIterable".to_string(),
                replacement_text: "collections.abc.AsyncIterable".to_string(),
                typing_import_only: Some(true),
            },
        );
        map.insert(
            "AsyncIterator".to_string(),
            DeprecatedForm {
                version: PYTHON_VERSION3_9,
                full_name: "typing.AsyncIterator".to_string(),
                replacement_text: "collections.abc.AsyncIterator".to_string(),
                typing_import_only: Some(true),
            },
        );
        map.insert(
            "AsyncGenerator".to_string(),
            DeprecatedForm {
                version: PYTHON_VERSION3_9,
                full_name: "typing.AsyncGenerator".to_string(),
                replacement_text: "collections.abc.AsyncGenerator".to_string(),
                typing_import_only: Some(true),
            },
        );
        map.insert(
            "Iterable".to_string(),
            DeprecatedForm {
                version: PYTHON_VERSION3_9,
                full_name: "typing.Iterable".to_string(),
                replacement_text: "collections.abc.Iterable".to_string(),
                typing_import_only: Some(true),
            },
        );
        map.insert(
            "Iterator".to_string(),
            DeprecatedForm {
                version: PYTHON_VERSION3_9,
                full_name: "typing.Iterator".to_string(),
                replacement_text: "collections.abc.Iterator".to_string(),
                typing_import_only: Some(true),
            },
        );
        map.insert(
            "Generator".to_string(),
            DeprecatedForm {
                version: PYTHON_VERSION3_9,
                full_name: "typing.Generator".to_string(),
                replacement_text: "collections.abc.Generator".to_string(),
                typing_import_only: Some(true),
            },
        );
        map.insert(
            "Reversible".to_string(),
            DeprecatedForm {
                version: PYTHON_VERSION3_9,
                full_name: "typing.Reversible".to_string(),
                replacement_text: "collections.abc.Reversible".to_string(),
                typing_import_only: Some(true),
            },
        );
        map.insert(
            "Container".to_string(),
            DeprecatedForm {
                version: PYTHON_VERSION3_9,
                full_name: "typing.Container".to_string(),
                replacement_text: "collections.abc.Container".to_string(),
                typing_import_only: Some(true),
            },
        );
        map.insert(
            "Collection".to_string(),
            DeprecatedForm {
                version: PYTHON_VERSION3_9,
                full_name: "typing.Collection".to_string(),
                replacement_text: "collections.abc.Collection".to_string(),
                typing_import_only: Some(true),
            },
        );
        map.insert(
            "AbstractSet".to_string(),
            DeprecatedForm {
                version: PYTHON_VERSION3_9,
                full_name: "typing.AbstractSet".to_string(),
                replacement_text: "collections.abc.Set".to_string(),
                typing_import_only: Some(true),
            },
        );
        map.insert(
            "MutableSet".to_string(),
            DeprecatedForm {
                version: PYTHON_VERSION3_9,
                full_name: "typing.MutableSet".to_string(),
                replacement_text: "collections.abc.MutableSet".to_string(),
                typing_import_only: Some(true),
            },
        );
        map.insert(
            "Mapping".to_string(),
            DeprecatedForm {
                version: PYTHON_VERSION3_9,
                full_name: "typing.Mapping".to_string(),
                replacement_text: "collections.abc.Mapping".to_string(),
                typing_import_only: Some(true),
            },
        );
        map.insert(
            "MutableMapping".to_string(),
            DeprecatedForm {
                version: PYTHON_VERSION3_9,
                full_name: "typing.MutableMapping".to_string(),
                replacement_text: "collections.abc.MutableMapping".to_string(),
                typing_import_only: Some(true),
            },
        );
        map.insert(
            "Sequence".to_string(),
            DeprecatedForm {
                version: PYTHON_VERSION3_9,
                full_name: "typing.Sequence".to_string(),
                replacement_text: "collections.abc.Sequence".to_string(),
                typing_import_only: Some(true),
            },
        );
        map.insert(
            "MutableSequence".to_string(),
            DeprecatedForm {
                version: PYTHON_VERSION3_9,
                full_name: "typing.MutableSequence".to_string(),
                replacement_text: "collections.abc.MutableSequence".to_string(),
                typing_import_only: Some(true),
            },
        );
        map.insert(
            "ByteString".to_string(),
            DeprecatedForm {
                version: PYTHON_VERSION3_9,
                full_name: "typing.ByteString".to_string(),
                replacement_text: "collections.abc.ByteString".to_string(),
                typing_import_only: Some(true),
            },
        );
        map.insert(
            "MappingView".to_string(),
            DeprecatedForm {
                version: PYTHON_VERSION3_9,
                full_name: "typing.MappingView".to_string(),
                replacement_text: "collections.abc.MappingView".to_string(),
                typing_import_only: Some(true),
            },
        );
        map.insert(
            "KeysView".to_string(),
            DeprecatedForm {
                version: PYTHON_VERSION3_9,
                full_name: "typing.KeysView".to_string(),
                replacement_text: "collections.abc.KeysView".to_string(),
                typing_import_only: Some(true),
            },
        );
        map.insert(
            "ItemsView".to_string(),
            DeprecatedForm {
                version: PYTHON_VERSION3_9,
                full_name: "typing.ItemsView".to_string(),
                replacement_text: "collections.abc.ItemsView".to_string(),
                typing_import_only: Some(true),
            },
        );
        map.insert(
            "ValuesView".to_string(),
            DeprecatedForm {
                version: PYTHON_VERSION3_9,
                full_name: "typing.ValuesView".to_string(),
                replacement_text: "collections.abc.ValuesView".to_string(),
                typing_import_only: Some(true),
            },
        );
        map.insert(
            "ContextManager".to_string(),
            DeprecatedForm {
                version: PYTHON_VERSION3_9,
                full_name: "typing.ContextManager".to_string(),
                replacement_text: "contextlib.AbstractContextManager".to_string(),
                typing_import_only: None,
            },
        );
        map.insert(
            "AsyncContextManager".to_string(),
            DeprecatedForm {
                version: PYTHON_VERSION3_9,
                full_name: "typing.AsyncContextManager".to_string(),
                replacement_text: "contextlib.AbstractAsyncContextManager".to_string(),
                typing_import_only: None,
            },
        );
        map.insert(
            "Pattern".to_string(),
            DeprecatedForm {
                version: PYTHON_VERSION3_9,
                full_name: "re.Pattern".to_string(),
                replacement_text: "re.Pattern".to_string(),
                typing_import_only: Some(true),
            },
        );
        map.insert(
            "Match".to_string(),
            DeprecatedForm {
                version: PYTHON_VERSION3_9,
                full_name: "re.Match".to_string(),
                replacement_text: "re.Match".to_string(),
                typing_import_only: Some(true),
            },
        );
        map
    };
}

thread_local! {
    // analyzer/deprecatedSymbols.ts:303
    pub static DEPRECATED_SPECIAL_FORMS: JsMap<String, DeprecatedForm> = {
        let mut map = JsMap::default();
        map.insert(
            "Optional".to_string(),
            DeprecatedForm {
                version: PYTHON_VERSION3_10,
                full_name: "typing.Optional".to_string(),
                replacement_text: "| None".to_string(),
                typing_import_only: None,
            },
        );
        map.insert(
            "Union".to_string(),
            DeprecatedForm {
                version: PYTHON_VERSION3_10,
                full_name: "typing.Union".to_string(),
                replacement_text: "|".to_string(),
                typing_import_only: None,
            },
        );
        map.insert(
            "Callable".to_string(),
            DeprecatedForm {
                version: PYTHON_VERSION3_9,
                full_name: "typing.Callable".to_string(),
                replacement_text: "collections.abc.Callable".to_string(),
                typing_import_only: Some(true),
            },
        );
        map
    };
}
