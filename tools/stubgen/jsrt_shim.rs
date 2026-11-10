// Stand-ins for jsrt items the stubs use before crates/jsrt defines them. The compile check
// appends a section to its scratch copy of jsrt only when crates/jsrt lacks the item. Each item
// is a request to the jsrt owner; the real definition may differ as long as the name and the
// shape the stubs rely on stay.

// @item js_flags
/// Bit operators for a flags newtype `pub struct X(pub u32)`, as TypeScript uses flag enums.
#[macro_export]
macro_rules! js_flags {
    ($name:ident) => {
        impl ::std::ops::BitOr for $name {
            type Output = $name;
            fn bitor(self, o: $name) -> $name {
                $name(self.0 | o.0)
            }
        }
        impl ::std::ops::BitAnd for $name {
            type Output = $name;
            fn bitand(self, o: $name) -> $name {
                $name(self.0 & o.0)
            }
        }
        impl ::std::ops::Not for $name {
            type Output = $name;
            fn not(self) -> $name {
                $name(!self.0)
            }
        }
        impl ::std::ops::BitOrAssign for $name {
            fn bitor_assign(&mut self, o: $name) {
                self.0 |= o.0;
            }
        }
        impl ::std::ops::BitAndAssign for $name {
            fn bitand_assign(&mut self, o: $name) {
                self.0 &= o.0;
            }
        }
    };
}

// @item JsString
/// A JavaScript string value: UTF-16 code units, lone surrogates allowed.
#[derive(Clone, PartialEq, Eq, Hash, Debug, Default)]
pub struct JsString(pub Vec<u16>);

// @item JsBigInt
/// A JavaScript `bigint`.
#[derive(Clone, PartialEq, Eq, Hash, Debug, Default)]
pub struct JsBigInt(pub Vec<u64>, pub bool);

// @item JsRegExp
/// A JavaScript `RegExp` with its flags.
#[derive(Clone, Debug)]
pub struct JsRegExp(pub String, pub String);

// @item NumberValue
/// porting.md "numbers": a Python number literal, a JS number or a JS bigint.
#[derive(Clone, PartialEq, Debug)]
pub enum NumberValue {
    Number(f64),
    BigInt(JsBigInt),
}
