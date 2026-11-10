//! What pyright gets from JavaScript and V8 for free: garbage-collected object
//! references, insertion-ordered collections, and JS string, number and sort
//! semantics. Nothing here ports a pyright file.

pub mod arena;
pub mod array;
pub mod cell;
pub mod collections;
pub mod flags;
pub mod command_line_args;
pub mod js_regex;
pub mod jsonc;
pub mod node_path;
pub mod number;
pub mod smol_toml;
pub mod sort;
pub mod string;
pub mod vscode_uri;

pub use arena::P;
pub use cell::OptionRefCell;
pub use collections::{FxHashMap, FxHashSet, JsMap, JsSet, LazyJsMap};
pub use number::{JsBigInt, NumberValue};
pub use string::{JsStr, JsString, NormalizationForm};
