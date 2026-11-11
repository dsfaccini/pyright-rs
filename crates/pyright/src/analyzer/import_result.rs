//! Port of `packages/pyright-internal/src/analyzer/importResult.ts`.

use jsrt::{JsMap, P};

use crate::analyzer::py_typed_utils::PyTypedInfo;
use crate::common::uri::Uri;

// analyzer/importResult.ts:13
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[derive(jsrt::Trace)]
pub enum ImportType {
    BuiltIn,
    ThirdParty,
    Local,
}

// analyzer/importResult.ts:19
#[derive(Clone, Debug)]
#[derive(jsrt::Trace)]
pub struct ImplicitImport {
    pub is_stub_file: bool,
    pub is_native_lib: bool,
    pub name: String,
    pub uri: Uri,
    pub py_typed_info: Option<PyTypedInfo>,
}

// analyzer/importResult.ts:27
/// Shared: the resolver caches it and parse nodes point at it. Build one,
/// then wrap it in `P`.
#[derive(Clone, Debug)]
#[derive(jsrt::Trace)]
pub struct ImportResult {
    pub import_name: String,
    pub is_relative: bool,
    pub is_import_found: bool,
    pub is_partly_resolved: bool,
    pub is_namespace_package: bool,
    pub is_init_file_present: bool,
    pub is_stub_package: bool,
    pub import_failure_info: Option<Vec<String>>,
    pub import_type: ImportType,
    pub resolved_uris: Vec<Uri>,
    pub search_path: Option<Uri>,
    pub is_stub_file: bool,
    pub is_native_lib: bool,
    pub is_stdlib_typeshed_file: Option<bool>,
    pub is_third_party_typeshed_file: Option<bool>,
    pub is_local_typings_file: Option<bool>,
    pub implicit_imports: Option<JsMap<String, ImplicitImport>>,
    pub filtered_implicit_imports: Option<JsMap<String, ImplicitImport>>,
    pub non_stub_import_result: Option<P<ImportResult>>,
    pub py_typed_info: Option<PyTypedInfo>,
    pub package_directory: Option<Uri>,
}
