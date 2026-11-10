//! Bit operators for flag enums: TypeScript `const enum`s whose members are `1 << n`.

/// Implements `|`, `|=`, `&`, `&=`, `!` and `any` for a flags newtype `pub struct X(pub u32)`.
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
