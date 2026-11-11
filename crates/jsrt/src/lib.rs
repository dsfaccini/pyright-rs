//! What pyright gets from JavaScript and V8 for free: garbage-collected object
//! references, insertion-ordered collections, and JS string, number and sort
//! semantics. Nothing here ports a pyright file.

extern crate self as jsrt;

pub mod arena;
pub mod atom;
pub mod array;
pub mod cell;
pub mod collections;
pub mod flags;
pub mod heap;
pub mod command_line_args;
pub mod js_regex;
pub mod jsonc;
pub mod node_path;
pub mod number;
pub mod region;
pub mod smol_toml;
pub mod sort;
pub mod string;
pub mod switch;
pub mod trace;
pub mod vscode_uri;

pub use arena::P;
pub use atom::{Atom, AtomKey};
pub use cell::OptionRefCell;
pub use heap::{ClassSlot, GcMode, Tracer};
pub use jsrt_derive::Trace;
pub use trace::Trace;
pub use collections::{FxHashMap, FxHashSet, JsMap, JsSet, LazyJsMap, NonEmptyList, SmallJsSet};
pub use number::{JsBigInt, NumberValue};
pub use string::{JsStr, JsString, NormalizationForm};
pub use switch::SwitchMode;
