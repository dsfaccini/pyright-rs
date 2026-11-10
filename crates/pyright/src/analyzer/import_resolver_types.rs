//! Port of `packages/pyright-internal/src/analyzer/importResolverTypes.ts`.
//!
//! Each interface here has one implementation, so it is a type alias for that
//! struct. pyright also looks the implementations up through
//! `ServiceKeys.importResolverFileSystem` and `ServiceKeys.typeshedInfoProvider`;
//! only tests register those, and that hook is not ported.

use jsrt::{JsMap, P};

use crate::analyzer::import_resolver_file_system::ImportResolverFileSystemImpl;
use crate::analyzer::typeshed_info_provider::DefaultTypeshedInfoProvider;
use crate::common::python_version::PythonVersion;
use crate::common::uri::Uri;

// analyzer/importResolverTypes.ts:12
#[derive(Clone, Debug)]
pub struct SupportedVersionInfo {
    pub min: PythonVersion,
    pub max: Option<PythonVersion>,
    pub unsupported_platforms: Option<Vec<String>>,
    pub supported_platforms: Option<Vec<String>>,
}

// analyzer/importResolverTypes.ts:19
/// Memoized and returned as the same object each time.
pub type TypeshedThirdPartyPackageMapResult = P<(JsMap<String, Vec<Uri>>, Vec<Uri>)>;

// analyzer/importResolverTypes.ts:26
pub type TypeshedInfoProvider = DefaultTypeshedInfoProvider;

// analyzer/importResolverTypes.ts:50
pub type ImportResolverFileSystem = ImportResolverFileSystemImpl;
