// Stand-ins for jsrt items the stubs use before crates/jsrt defines them. The compile check
// appends a section to its scratch copy of jsrt only when crates/jsrt lacks the item. Each item
// is a request to the jsrt owner; the real definition may differ as long as the name and the
// shape the stubs rely on stay.

// @item flag_ops
/// Bit operators for a flags newtype `pub struct X(pub u32)`, as TypeScript uses flag enums. Same
/// body as the `flag_ops!` in crates/pyright/src/parser/tokenizer_types.rs.
#[macro_export]
macro_rules! flag_ops {
    ($name:ident) => {
        impl ::std::ops::BitOr for $name {
            type Output = $name;
            fn bitor(self, rhs: $name) -> $name {
                $name(self.0 | rhs.0)
            }
        }
        impl ::std::ops::BitOrAssign for $name {
            fn bitor_assign(&mut self, rhs: $name) {
                self.0 |= rhs.0;
            }
        }
        impl ::std::ops::BitAnd for $name {
            type Output = $name;
            fn bitand(self, rhs: $name) -> $name {
                $name(self.0 & rhs.0)
            }
        }
        impl ::std::ops::BitAndAssign for $name {
            fn bitand_assign(&mut self, rhs: $name) {
                self.0 &= rhs.0;
            }
        }
        impl ::std::ops::Not for $name {
            type Output = $name;
            fn not(self) -> $name {
                $name(!self.0)
            }
        }
        impl $name {
            /// `(flags & other) !== 0`.
            pub fn any(self, other: $name) -> bool {
                (self.0 & other.0) != 0
            }
        }
    };
}
