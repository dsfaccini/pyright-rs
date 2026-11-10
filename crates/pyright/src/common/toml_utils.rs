//! Port of `packages/pyright-internal/src/common/tomlUtils.ts`.

use serde_json::Value;

// common/tomlUtils.ts:31
/// `smol-toml`'s `parse`, through `jsrt::smol_toml`. The error is the
/// message of the `TomlError` it throws.
pub fn parse(toml: &str) -> Result<Value, String> {
    jsrt::smol_toml::parse(toml)
}
