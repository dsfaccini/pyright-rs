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
mod vm;
pub mod vscode_uri;

pub use arena::{EvalShared, Header, P};
pub use atom::{Atom, AtomKey};
pub use cell::{EvalOptionRefCell, OptionRefCell};
pub use heap::{ClassSlot, GcMode, Tracer};
pub use jsrt_derive::Trace;
pub use trace::Trace;
pub use collections::{FxHashMap, FxHashSet, IdMap, JsMap, JsSet, LazyJsMap, LogKeyed, LoggedMap, NonEmptyList, SmallJsSet, VecMap};
pub use number::{JsBigInt, NumberValue};
pub use string::{JsStr, JsString, NormalizationForm};
pub use switch::SwitchMode;
