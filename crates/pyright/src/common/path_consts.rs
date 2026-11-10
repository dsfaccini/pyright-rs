//! Port of `packages/pyright-internal/src/common/pathConsts.ts`.

// common/pathConsts.ts:9
pub const TYPESHED_FALLBACK: &str = "typeshed-fallback";
pub const LIB: &str = "lib";
pub const LIB_ALTERNATE: &str = "Lib";
pub const LIB64: &str = "lib64";
pub const SITE_PACKAGES: &str = "site-packages";
pub const DIST_PACKAGES: &str = "dist-packages";
pub const SRC: &str = "src";
pub const STUBS_SUFFIX: &str = "-stubs";
pub const DEFAULT_STUBS_DIRECTORY: &str = "typings";
pub const REQUIREMENTS_FILE_NAME: &str = "requirements.txt";
pub const PYPROJECT_TOML_NAME: &str = "pyproject.toml";
pub const DOT_PYTHON_VERSION_NAME: &str = ".python-version";
pub const CONFIG_FILE_NAME: &str = "pyrightconfig.json";

// common/pathConsts.ts:27
pub const DEFAULT_EXCLUDES: [&str; 4] = [
    "**/node_modules",
    "**/__pycache__",
    "**/.*",
    "**/__editable__.*",
];
