//! Port of `packages/pyright-internal/src/parser/tokenizerTypes.ts`.
//!
//! A token is one `Token` struct: the `TokenBase` fields plus `d`, an enum with
//! the fields of the specific token interface (`KeywordToken`, `StringToken`, ...).
//! Each specific interface also gets a typed handle, for example
//! `StringToken(P<Token>)`, with `cast`, `d()` and `Deref` to `Token`, as parse
//! nodes do (see `docs/porting.md`).

use std::ops::{BitAnd, BitAndAssign, BitOr, BitOrAssign, Deref, Not};

use jsrt::{JsStr, JsString, NormalizationForm, P};

use jsrt::NumberValue;

use crate::common::text_range::{HasTextRange, TextRange};

// parser/tokenizerTypes.ts:16
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TokenType {
    Invalid,
    EndOfStream,
    NewLine,
    Indent,
    Dedent,
    String,
    Number,
    Identifier,
    Keyword,
    Operator,
    Colon,
    Semicolon,
    Comma,
    OpenParenthesis,
    CloseParenthesis,
    OpenBracket,
    CloseBracket,
    OpenCurlyBrace,
    CloseCurlyBrace,
    Ellipsis,
    Dot,
    Arrow,
    Backtick,
    ExclamationMark,
    FStringStart,
    FStringMiddle,
    FStringEnd,
}

// parser/tokenizerTypes.ts:46
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NewLineType {
    CarriageReturn,
    LineFeed,
    CarriageReturnLineFeed,
    Implied,
}

// parser/tokenizerTypes.ts:53
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum OperatorType {
    // These operators are used with tokens
    // of type TokenType.Operator.
    Add,
    AddEqual,
    Assign,
    BitwiseAnd,
    BitwiseAndEqual,
    BitwiseInvert,
    BitwiseOr,
    BitwiseOrEqual,
    BitwiseXor,
    BitwiseXorEqual,
    Divide,
    DivideEqual,
    Equals,
    FloorDivide,
    FloorDivideEqual,
    GreaterThan,
    GreaterThanOrEqual,
    LeftShift,
    LeftShiftEqual,
    LessOrGreaterThan,
    LessThan,
    LessThanOrEqual,
    MatrixMultiply,
    MatrixMultiplyEqual,
    Mod,
    ModEqual,
    Multiply,
    MultiplyEqual,
    NotEquals,
    Power,
    PowerEqual,
    RightShift,
    RightShiftEqual,
    Subtract,
    SubtractEqual,
    Walrus,

    // These operators are used with tokens
    // of type TokenType.Keyword.
    And,
    Or,
    Not,
    Is,
    IsNot,
    In,
    NotIn,
}

impl OperatorType {
    /// Every member, in declaration order (`OperatorType[i]` is `ALL[i]`).
    pub const ALL: [OperatorType; 43] = [
        OperatorType::Add,
        OperatorType::AddEqual,
        OperatorType::Assign,
        OperatorType::BitwiseAnd,
        OperatorType::BitwiseAndEqual,
        OperatorType::BitwiseInvert,
        OperatorType::BitwiseOr,
        OperatorType::BitwiseOrEqual,
        OperatorType::BitwiseXor,
        OperatorType::BitwiseXorEqual,
        OperatorType::Divide,
        OperatorType::DivideEqual,
        OperatorType::Equals,
        OperatorType::FloorDivide,
        OperatorType::FloorDivideEqual,
        OperatorType::GreaterThan,
        OperatorType::GreaterThanOrEqual,
        OperatorType::LeftShift,
        OperatorType::LeftShiftEqual,
        OperatorType::LessOrGreaterThan,
        OperatorType::LessThan,
        OperatorType::LessThanOrEqual,
        OperatorType::MatrixMultiply,
        OperatorType::MatrixMultiplyEqual,
        OperatorType::Mod,
        OperatorType::ModEqual,
        OperatorType::Multiply,
        OperatorType::MultiplyEqual,
        OperatorType::NotEquals,
        OperatorType::Power,
        OperatorType::PowerEqual,
        OperatorType::RightShift,
        OperatorType::RightShiftEqual,
        OperatorType::Subtract,
        OperatorType::SubtractEqual,
        OperatorType::Walrus,
        OperatorType::And,
        OperatorType::Or,
        OperatorType::Not,
        OperatorType::Is,
        OperatorType::IsNot,
        OperatorType::In,
        OperatorType::NotIn,
    ];
}

// parser/tokenizerTypes.ts:104
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct OperatorFlags(pub u32);

impl OperatorFlags {
    pub const Unary: OperatorFlags = OperatorFlags(1 << 0);
    pub const Binary: OperatorFlags = OperatorFlags(1 << 1);
    pub const Assignment: OperatorFlags = OperatorFlags(1 << 2);
    pub const Comparison: OperatorFlags = OperatorFlags(1 << 3);
    pub const Deprecated: OperatorFlags = OperatorFlags(1 << 4);
}

// parser/tokenizerTypes.ts:112
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum KeywordType {
    And,
    As,
    Assert,
    Async,
    Await,
    Break,
    Case,
    Class,
    Continue,
    Debug,
    Def,
    Del,
    Elif,
    Else,
    Except,
    False,
    Finally,
    For,
    From,
    Global,
    If,
    Import,
    In,
    Is,
    Lambda,
    Lazy,
    Match,
    None,
    Nonlocal,
    Not,
    Or,
    Pass,
    Raise,
    Return,
    True,
    Try,
    Type,
    While,
    With,
    Yield,
}

// parser/tokenizerTypes.ts:155
pub const SOFT_KEYWORDS: [KeywordType; 5] = [
    KeywordType::Debug,
    KeywordType::Match,
    KeywordType::Case,
    KeywordType::Type,
    KeywordType::Lazy,
];

// parser/tokenizerTypes.ts:163
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct StringTokenFlags(pub u32);

impl StringTokenFlags {
    pub const None: StringTokenFlags = StringTokenFlags(0);

    // Quote types
    pub const SingleQuote: StringTokenFlags = StringTokenFlags(1 << 0);
    pub const DoubleQuote: StringTokenFlags = StringTokenFlags(1 << 1);
    pub const Triplicate: StringTokenFlags = StringTokenFlags(1 << 2);

    // String content format
    pub const Raw: StringTokenFlags = StringTokenFlags(1 << 3);
    pub const Unicode: StringTokenFlags = StringTokenFlags(1 << 4);
    pub const Bytes: StringTokenFlags = StringTokenFlags(1 << 5);
    pub const Format: StringTokenFlags = StringTokenFlags(1 << 6);
    pub const Template: StringTokenFlags = StringTokenFlags(1 << 7);

    // Other conditions
    pub const ReplacementFieldStart: StringTokenFlags = StringTokenFlags(1 << 8);
    pub const ReplacementFieldEnd: StringTokenFlags = StringTokenFlags(1 << 9);
    pub const NamedUnicodeEscape: StringTokenFlags = StringTokenFlags(1 << 10);

    // Error conditions
    pub const Unterminated: StringTokenFlags = StringTokenFlags(1 << 16);
}

macro_rules! flag_ops {
    ($name:ident) => {
        impl BitOr for $name {
            type Output = $name;
            fn bitor(self, rhs: $name) -> $name {
                $name(self.0 | rhs.0)
            }
        }
        impl BitOrAssign for $name {
            fn bitor_assign(&mut self, rhs: $name) {
                self.0 |= rhs.0;
            }
        }
        impl BitAnd for $name {
            type Output = $name;
            fn bitand(self, rhs: $name) -> $name {
                $name(self.0 & rhs.0)
            }
        }
        impl BitAndAssign for $name {
            fn bitand_assign(&mut self, rhs: $name) {
                self.0 &= rhs.0;
            }
        }
        impl Not for $name {
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

flag_ops!(OperatorFlags);
flag_ops!(StringTokenFlags);

// parser/tokenizerTypes.ts:187
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CommentType {
    Regular,
    IPythonMagic,
    IPythonShellEscape,
    IPythonCellMagic,
    IPythonCellShellEscape,
}

// parser/tokenizerTypes.ts:195
#[derive(Clone, Debug, PartialEq)]
pub struct Comment {
    pub type_: CommentType,
    pub value: String,
    pub start: i32,
    pub length: i32,
}

impl Comment {
    // parser/tokenizerTypes.ts:203
    pub fn create(start: i32, length: i32, value: String, type_: Option<CommentType>) -> Comment {
        Comment {
            type_: type_.unwrap_or(CommentType::Regular),
            start,
            length,
            value,
        }
    }
}

impl HasTextRange for Comment {
    fn text_range(&self) -> TextRange {
        TextRange {
            start: self.start,
            length: self.length,
        }
    }
}

// parser/tokenizerTypes.ts:208
pub struct Token {
    pub type_: TokenType,
    pub start: i32,
    pub length: i32,

    // Comments prior to the token.
    pub comments: Option<&'static [Comment]>,

    /// The fields of the specific token interface. `None` for a plain `Token`.
    pub d: TokenDetails,
}

/// The fields that each specific token interface adds to `Token`.
pub enum TokenDetails {
    None,
    Indent(IndentTokenDetails),
    Dedent(DedentTokenDetails),
    NewLine(NewLineTokenDetails),
    Keyword(KeywordTokenDetails),
    String(StringTokenDetails),
    FStringStart(FStringStartTokenDetails),
    FStringMiddle(FStringMiddleTokenDetails),
    FStringEnd(FStringEndTokenDetails),
    Number(NumberTokenDetails),
    Operator(OperatorTokenDetails),
    Identifier(IdentifierTokenDetails),
}

impl HasTextRange for Token {
    fn text_range(&self) -> TextRange {
        TextRange {
            start: self.start,
            length: self.length,
        }
    }
}

/// Moves a comment list into the arena, where tokens can share it.
fn alloc_comments(comments: Option<Vec<Comment>>) -> Option<&'static [Comment]> {
    comments.map(|c| P::new(c).get().as_slice())
}

impl Token {
    // parser/tokenizerTypes.ts:222
    pub fn create(
        type_: TokenType,
        start: i32,
        length: i32,
        comments: Option<Vec<Comment>>,
    ) -> P<Token> {
        Token::create_with_shared_comments(type_, start, length, alloc_comments(comments))
    }

    /// `Token.create` with a comment list that another token already holds.
    pub fn create_with_shared_comments(
        type_: TokenType,
        start: i32,
        length: i32,
        comments: Option<&'static [Comment]>,
    ) -> P<Token> {
        P::new(Token {
            start,
            length,
            type_,
            comments,
            d: TokenDetails::None,
        })
    }
}

macro_rules! token_handle {
    ($handle:ident, $variant:ident, $details:ident) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub struct $handle(pub P<Token>);

        impl $handle {
            pub fn cast(token: P<Token>) -> Option<$handle> {
                match token.d {
                    TokenDetails::$variant(_) => Some($handle(token)),
                    _ => None,
                }
            }

            pub fn d(&self) -> &'static $details {
                match &self.0.get().d {
                    TokenDetails::$variant(d) => d,
                    _ => unreachable!(),
                }
            }

            pub fn token(self) -> P<Token> {
                self.0
            }
        }

        impl Deref for $handle {
            type Target = Token;
            fn deref(&self) -> &Token {
                self.0.get()
            }
        }

        impl HasTextRange for $handle {
            fn text_range(&self) -> TextRange {
                self.0.text_range()
            }
        }

        impl From<$handle> for P<Token> {
            fn from(handle: $handle) -> P<Token> {
                handle.0
            }
        }
    };
}

// parser/tokenizerTypes.ts:232
pub struct IndentTokenDetails {
    pub indent_amount: i32,
    pub is_indent_ambiguous: bool,
}
token_handle!(IndentToken, Indent, IndentTokenDetails);

impl IndentToken {
    // parser/tokenizerTypes.ts:239
    pub fn create(
        start: i32,
        length: i32,
        indent_amount: i32,
        is_indent_ambiguous: bool,
        comments: Option<Vec<Comment>>,
    ) -> IndentToken {
        IndentToken(P::new(Token {
            start,
            length,
            type_: TokenType::Indent,
            comments: alloc_comments(comments),
            d: TokenDetails::Indent(IndentTokenDetails {
                is_indent_ambiguous,
                indent_amount,
            }),
        }))
    }
}

// parser/tokenizerTypes.ts:269
pub struct DedentTokenDetails {
    pub indent_amount: i32,
    pub matches_indent: bool,
    pub is_dedent_ambiguous: bool,
}
token_handle!(DedentToken, Dedent, DedentTokenDetails);

impl DedentToken {
    // parser/tokenizerTypes.ts:277
    pub fn create(
        start: i32,
        length: i32,
        indent_amount: i32,
        matches_indent: bool,
        is_dedent_ambiguous: bool,
        comments: Option<Vec<Comment>>,
    ) -> DedentToken {
        DedentToken(P::new(Token {
            start,
            length,
            type_: TokenType::Dedent,
            comments: alloc_comments(comments),
            d: TokenDetails::Dedent(DedentTokenDetails {
                indent_amount,
                matches_indent,
                is_dedent_ambiguous,
            }),
        }))
    }
}

// parser/tokenizerTypes.ts:308
pub struct NewLineTokenDetails {
    pub new_line_type: NewLineType,
}
token_handle!(NewLineToken, NewLine, NewLineTokenDetails);

impl NewLineToken {
    // parser/tokenizerTypes.ts:314
    pub fn create(
        start: i32,
        length: i32,
        new_line_type: NewLineType,
        comments: Option<Vec<Comment>>,
    ) -> NewLineToken {
        NewLineToken(P::new(Token {
            start,
            length,
            type_: TokenType::NewLine,
            comments: alloc_comments(comments),
            d: TokenDetails::NewLine(NewLineTokenDetails { new_line_type }),
        }))
    }
}

// parser/tokenizerTypes.ts:339
pub struct KeywordTokenDetails {
    pub keyword_type: KeywordType,
}
token_handle!(KeywordToken, Keyword, KeywordTokenDetails);

impl KeywordToken {
    // parser/tokenizerTypes.ts:345
    pub fn create(
        start: i32,
        length: i32,
        keyword_type: KeywordType,
        comments: Option<Vec<Comment>>,
    ) -> KeywordToken {
        KeywordToken(P::new(Token {
            start,
            length,
            type_: TokenType::Keyword,
            comments: alloc_comments(comments),
            d: TokenDetails::Keyword(KeywordTokenDetails { keyword_type }),
        }))
    }

    // parser/tokenizerTypes.ts:369
    pub fn is_soft_keyword(token: KeywordToken) -> bool {
        SOFT_KEYWORDS.iter().any(|&t| token.d().keyword_type == t)
    }
}

// parser/tokenizerTypes.ts:374
pub struct StringTokenDetails {
    pub flags: StringTokenFlags,

    // Use StringTokenUtils to convert escaped value to unescaped value.
    pub escaped_value: JsString,

    // Number of characters in token that appear before
    // the quote marks (e.g. "r" or "UR").
    pub prefix_length: i32,

    // Number of characters in token that make up the quote
    // (either 1 or 3).
    pub quote_mark_length: i32,
}
token_handle!(StringToken, String, StringTokenDetails);

impl StringToken {
    // parser/tokenizerTypes.ts:391
    pub fn create(
        start: i32,
        length: i32,
        flags: StringTokenFlags,
        escaped_value: JsString,
        prefix_length: i32,
        comments: Option<Vec<Comment>>,
    ) -> StringToken {
        let quote_mark_length = if flags.any(StringTokenFlags::Triplicate) {
            3
        } else {
            1
        };
        StringToken(P::new(Token {
            start,
            length,
            type_: TokenType::String,
            comments: alloc_comments(comments),
            d: TokenDetails::String(StringTokenDetails {
                flags,
                escaped_value,
                prefix_length,
                quote_mark_length,
            }),
        }))
    }
}

// parser/tokenizerTypes.ts:425
pub struct FStringStartTokenDetails {
    pub flags: StringTokenFlags,

    // Number of characters in token that appear before
    // the quote marks (e.g. "r" or "UR").
    pub prefix_length: i32,

    // Number of characters in token that make up the quote
    // (either 1 or 3).
    pub quote_mark_length: i32,
}
token_handle!(FStringStartToken, FStringStart, FStringStartTokenDetails);

impl FStringStartToken {
    // parser/tokenizerTypes.ts:439
    pub fn create(
        start: i32,
        length: i32,
        flags: StringTokenFlags,
        prefix_length: i32,
        comments: Option<Vec<Comment>>,
    ) -> FStringStartToken {
        let quote_mark_length = if flags.any(StringTokenFlags::Triplicate) {
            3
        } else {
            1
        };
        FStringStartToken(P::new(Token {
            start,
            length,
            type_: TokenType::FStringStart,
            comments: alloc_comments(comments),
            d: TokenDetails::FStringStart(FStringStartTokenDetails {
                flags,
                prefix_length,
                quote_mark_length,
            }),
        }))
    }
}

// parser/tokenizerTypes.ts:470
pub struct FStringMiddleTokenDetails {
    pub flags: StringTokenFlags,

    // Use StringTokenUtils to convert escaped value to unescaped value.
    pub escaped_value: JsString,
}
token_handle!(FStringMiddleToken, FStringMiddle, FStringMiddleTokenDetails);

impl FStringMiddleToken {
    // parser/tokenizerTypes.ts:479
    pub fn create(
        start: i32,
        length: i32,
        flags: StringTokenFlags,
        escaped_value: JsString,
    ) -> FStringMiddleToken {
        FStringMiddleToken(P::new(Token {
            start,
            length,
            type_: TokenType::FStringMiddle,
            comments: None,
            d: TokenDetails::FStringMiddle(FStringMiddleTokenDetails {
                flags,
                escaped_value,
            }),
        }))
    }
}

// parser/tokenizerTypes.ts:492
pub struct FStringEndTokenDetails {
    pub flags: StringTokenFlags,
}
token_handle!(FStringEndToken, FStringEnd, FStringEndTokenDetails);

impl FStringEndToken {
    // parser/tokenizerTypes.ts:498
    pub fn create(start: i32, length: i32, flags: StringTokenFlags) -> FStringEndToken {
        FStringEndToken(P::new(Token {
            start,
            length,
            type_: TokenType::FStringEnd,
            comments: None,
            d: TokenDetails::FStringEnd(FStringEndTokenDetails { flags }),
        }))
    }
}

// parser/tokenizerTypes.ts:510
pub struct NumberTokenDetails {
    pub value: NumberValue,
    pub is_integer: bool,
    pub is_imaginary: bool,
}
token_handle!(NumberToken, Number, NumberTokenDetails);

impl NumberToken {
    // parser/tokenizerTypes.ts:518
    pub fn create(
        start: i32,
        length: i32,
        value: NumberValue,
        is_integer: bool,
        is_imaginary: bool,
        comments: Option<Vec<Comment>>,
    ) -> NumberToken {
        NumberToken(P::new(Token {
            start,
            length,
            type_: TokenType::Number,
            comments: alloc_comments(comments),
            d: TokenDetails::Number(NumberTokenDetails {
                is_integer,
                is_imaginary,
                value,
            }),
        }))
    }
}

// parser/tokenizerTypes.ts:549
pub struct OperatorTokenDetails {
    pub operator_type: OperatorType,
}
token_handle!(OperatorToken, Operator, OperatorTokenDetails);

impl OperatorToken {
    // parser/tokenizerTypes.ts:555
    pub fn create(
        start: i32,
        length: i32,
        operator_type: OperatorType,
        comments: Option<Vec<Comment>>,
    ) -> OperatorToken {
        OperatorToken(P::new(Token {
            start,
            length,
            type_: TokenType::Operator,
            comments: alloc_comments(comments),
            d: TokenDetails::Operator(OperatorTokenDetails { operator_type }),
        }))
    }
}

// parser/tokenizerTypes.ts:580
pub struct IdentifierTokenDetails {
    pub value: String,
}
token_handle!(IdentifierToken, Identifier, IdentifierTokenDetails);

impl IdentifierToken {
    // parser/tokenizerTypes.ts:586
    pub fn create(
        start: i32,
        length: i32,
        value: String,
        comments: Option<Vec<Comment>>,
    ) -> IdentifierToken {
        IdentifierToken::create_with_shared_comments(start, length, value, alloc_comments(comments))
    }

    /// `IdentifierToken.create` with a comment list that another token already holds.
    pub fn create_with_shared_comments(
        start: i32,
        length: i32,
        value: String,
        comments: Option<&'static [Comment]>,
    ) -> IdentifierToken {
        // Perform "NFKC normalization", as per the Python lexical spec.
        let normalized_value = if value.bytes().any(|b| b > 0x7f) {
            value.js_normalize(NormalizationForm::Nfkc)
        } else {
            value
        };

        IdentifierToken(P::new(Token {
            start,
            length,
            type_: TokenType::Identifier,
            comments,
            d: TokenDetails::Identifier(IdentifierTokenDetails {
                value: normalized_value,
            }),
        }))
    }
}
