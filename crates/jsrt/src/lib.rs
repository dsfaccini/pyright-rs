//! What pyright gets from JavaScript and V8 for free: garbage-collected object
//! references, insertion-ordered collections, and JS string, number and sort
//! semantics. Nothing here ports a pyright file.

pub mod arena;
pub mod collections;
pub mod string;

pub use arena::P;
pub use collections::{FxHashMap, FxHashSet, JsMap, JsSet};
pub use string::{JsStr, JsString, NormalizationForm};
