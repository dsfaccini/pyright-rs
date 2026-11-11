//! Port of `packages/pyright-internal/src/parser/tokenizerTypes.ts`.
//!
//! `Token` is the `TokenBase` header. A token is allocated as its header followed
//! by the fields of its specific interface (`KeywordToken`, `StringToken`, ...);
//! `d()` returns them as a `TokenDetails` variant.
//! Each specific interface also gets a typed handle, for example
//! `StringToken(P<Token>)`, with `cast`, `d()` and `Deref` to `Token`, as parse
//! nodes do (see `docs/porting.md`).

use jsrt::flag_ops;
use std::cell::Cell;
use std::ops::Deref;
use std::ptr;

use jsrt::region::Region;

use jsrt::{Atom, JsStr, JsString, NormalizationForm, P};

use jsrt::NumberValue;

use crate::common::text_range::{HasTextRange, TextRange};

// parser/tokenizerTypes.ts:16
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[derive(jsrt::Trace)]
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
#[derive(jsrt::Trace)]
pub enum NewLineType {
    CarriageReturn,
    LineFeed,
    CarriageReturnLineFeed,
    Implied,
}

// parser/tokenizerTypes.ts:53
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[derive(jsrt::Trace)]
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
#[derive(jsrt::Trace)]
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
#[derive(jsrt::Trace)]
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


flag_ops!(OperatorFlags);
flag_ops!(StringTokenFlags);

// parser/tokenizerTypes.ts:187
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[derive(jsrt::Trace)]
pub enum CommentType {
    Regular,
    IPythonMagic,
    IPythonShellEscape,
    IPythonCellMagic,
    IPythonCellShellEscape,
}

// parser/tokenizerTypes.ts:195
#[derive(Clone, Debug, PartialEq)]
#[derive(jsrt::Trace)]
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
/// The `TokenBase` fields. A token is allocated as this header followed by the
/// fields of its specific interface (`TokenAlloc`), so it takes the size of its
/// own interface; `d()` returns them.
#[repr(C)]
#[derive(jsrt::Trace)]
pub struct Token {
    pub type_: TokenType,
    pub start: i32,
    pub length: i32,

    /// The specific interface the token was built as, `d()`'s variant. A plain
    /// `Token.create` can carry any type, `Keyword` included.
    kind: TokenKind,

    /// The token lives in its tokenizer's scratch region (`TokenScratch`).
    scratch: bool,

    /// The token has comments prior to it, after its interface fields (`TokenAlloc`).
    has_comments: bool,
}
const _: () = assert!(size_of::<Token>() <= 16);

#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
#[derive(jsrt::Trace)]
enum TokenKind {
    None,
    Indent,
    Dedent,
    NewLine,
    Keyword,
    String,
    FStringStart,
    FStringMiddle,
    FStringEnd,
    Number,
    Operator,
    Identifier,
}

/// A token as allocated: the header, the fields of its interface, then the
/// comments prior to it: `C` is `P<Vec<Comment>>` for a token that has them, else `()`.
#[repr(C)]
#[derive(jsrt::Trace)]
struct TokenAlloc<D, C = ()> {
    token: Token,
    d: D,
    comments: C,
}

/// The comments of a token that has them.
type Comments = P<Vec<Comment>>;

/// `$body` with `$D` naming the interface type of token kind `$kind`.
macro_rules! with_interface {
    ($kind:expr, $D:ident => $body:expr) => {
        match $kind {
            TokenKind::None => { type $D = NoInterface; $body }
            TokenKind::Indent => { type $D = IndentTokenDetails; $body }
            TokenKind::Dedent => { type $D = DedentTokenDetails; $body }
            TokenKind::NewLine => { type $D = NewLineTokenDetails; $body }
            TokenKind::Keyword => { type $D = KeywordTokenDetails; $body }
            TokenKind::String => { type $D = StringTokenDetails; $body }
            TokenKind::FStringStart => { type $D = FStringStartTokenDetails; $body }
            TokenKind::FStringMiddle => { type $D = FStringMiddleTokenDetails; $body }
            TokenKind::FStringEnd => { type $D = FStringEndTokenDetails; $body }
            TokenKind::Number => { type $D = NumberTokenDetails; $body }
            TokenKind::Operator => { type $D = OperatorTokenDetails; $body }
            TokenKind::Identifier => { type $D = IdentifierTokenDetails; $body }
        }
    };
}

/// The fields one specific token interface adds to `Token`.
trait TokenInterface: jsrt::Trace + 'static {
    const KIND: TokenKind;

    /// Parse nodes hold nearly every token of this interface (names, strings and
    /// f-string parts), so the tokenizer puts it on the program heap directly
    /// rather than copying it out of the scratch region later.
    const HELD_BY_NODES: bool = matches!(
        Self::KIND,
        TokenKind::Identifier | TokenKind::String | TokenKind::FStringStart | TokenKind::FStringMiddle
    );
}

/// The fields of a plain `Token`: none.
#[derive(jsrt::Trace)]
struct NoInterface;

impl TokenInterface for NoInterface {
    const KIND: TokenKind = TokenKind::None;
}

/// A token of a specific interface, in the scratch region of the tokenizer run in
/// progress, or on the program heap outside a run.
fn alloc_token<D: TokenInterface>(
    type_: TokenType,
    start: i32,
    length: i32,
    comments: Option<Comments>,
    d: D,
) -> P<Token> {
    let scratch = if D::HELD_BY_NODES { ptr::null() } else { SCRATCH.with(Cell::get) };
    let token = Token {
        type_,
        start,
        length,
        kind: D::KIND,
        scratch: !scratch.is_null(),
        has_comments: comments.is_some(),
    };
    match comments {
        None => place_token(scratch, TokenAlloc { token, d, comments: () }),
        Some(comments) => place_token(scratch, TokenAlloc { token, d, comments }),
    }
}

/// `alloc` in the scratch region `scratch`, or on the program heap when it is null.
fn place_token<D: TokenInterface, C: jsrt::Trace + 'static>(
    scratch: *const Region,
    alloc: TokenAlloc<D, C>,
) -> P<Token> {
    if scratch.is_null() {
        return P::new(alloc).header(|alloc| &alloc.token);
    }
    // SAFETY: `SCRATCH` points at the region of the tokenizer run in progress.
    let slot = unsafe { &*scratch }.alloc(ScratchToken { kept: Cell::new(None), alloc });
    // SAFETY: the region lives as long as the tokenizer output. Parse nodes hold
    // kept copies (`TokenCell`), and nothing else holds a token past the output.
    unsafe { P::from_raw(ptr::addr_of!((*slot).alloc.token)) }
}

/// A plain `Token`, with no interface fields.
fn alloc_plain_token(type_: TokenType, start: i32, length: i32, comments: Option<Comments>) -> P<Token> {
    alloc_token(type_, start, length, comments, NoInterface)
}

thread_local! {
    /// The scratch region of the tokenizer run in progress, or null.
    static SCRATCH: Cell<*const Region> = const { Cell::new(ptr::null()) };

    /// `PYRIGHT_RS_TOKENS`: `keep` puts every token on the program heap; `check`
    /// makes a freed scratch region inaccessible, so a later use faults. Anything
    /// else frees the tokens no parse node holds with their tokenizer output.
    static TOKEN_MODE: TokenMode = match std::env::var("PYRIGHT_RS_TOKENS").as_deref() {
        Ok("keep") => TokenMode::Keep,
        Ok("check") => TokenMode::Check,
        _ => TokenMode::Free,
    };
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TokenMode {
    Free,
    Keep,
    Check,
}

/// A token in a scratch region: the copy a parse node keeps (`Token::keep`), then the token.
#[repr(C)]
struct ScratchToken<D, C = ()> {
    kept: Cell<Option<P<Token>>>,
    alloc: TokenAlloc<D, C>,
}

/// The region one tokenizer run puts its tokens in, owned by its `TokenizerOutput`.
/// pyright drops the tokenizer output once `SourceFile.parse` is done with it, so
/// the tokens no parse node holds become garbage; here they are dropped with the
/// output. A parse node holds a copy on the program heap (`TokenCell`).
pub struct TokenScratch {
    region: Box<Region>,
}

/// Restores the previous scratch region when dropped.
pub struct TokenScratchGuard(*const Region);

impl Drop for TokenScratchGuard {
    fn drop(&mut self) {
        SCRATCH.with(|scratch| scratch.set(self.0));
    }
}

impl TokenScratch {
    /// A region for one tokenizer run, or `None` under `PYRIGHT_RS_TOKENS=keep`.
    pub fn new() -> Option<TokenScratch> {
        let mode = TOKEN_MODE.with(|mode| *mode);
        (mode != TokenMode::Keep).then(|| TokenScratch { region: Box::new(Region::new(mode == TokenMode::Check)) })
    }

    /// New tokens go to this region until the guard drops.
    pub fn enter(&self) -> TokenScratchGuard {
        TokenScratchGuard(SCRATCH.with(|scratch| scratch.replace(&*self.region)))
    }
}

/// Drops the tokens no parse node kept. A kept token's buffers belong to its copy.
impl Drop for TokenScratch {
    fn drop(&mut self) {
        /// Drops the token at `at` unless it was kept; returns its size.
        unsafe fn drop_unkept<D, C>(at: *mut u8) -> usize {
            let token = at.cast::<ScratchToken<D, C>>();
            // SAFETY: the caller passes the address of a live `ScratchToken<D>`.
            unsafe {
                if (*token).kept.get().is_none() {
                    ptr::drop_in_place(ptr::addr_of_mut!((*token).alloc.d));
                }
            }
            size_of::<ScratchToken<D, C>>()
        }

        self.region.for_each_used(|start, end| {
            let mut at = start;
            while at < end {
                // SAFETY: the region holds only `ScratchToken`s, back to back: each
                // has a size that is a multiple of its alignment, 8.
                let token = unsafe { &(*at.cast::<ScratchToken<NoInterface>>()).alloc.token };
                let size = unsafe {
                    with_interface!(token.kind, D => if token.has_comments {
                        drop_unkept::<D, Comments>(at)
                    } else {
                        drop_unkept::<D, ()>(at)
                    })
                };
                at = at.wrapping_add(size);
            }
        });
    }
}

/// A token handle that a parse node can keep (`Token::keep`).
pub trait KeepToken: Copy + jsrt::Trace {
    fn keep(self) -> Self;
}

impl KeepToken for P<Token> {
    fn keep(self) -> Self {
        Token::keep(self)
    }
}

impl<T: KeepToken> KeepToken for Option<T> {
    fn keep(self) -> Self {
        self.map(T::keep)
    }
}

/// A token field of a parse node: it holds the kept copy of each token stored in it.
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct TokenCell<T: Copy>(jsrt::cell::Cell<T>);

impl<T: KeepToken> TokenCell<T> {
    pub fn new(token: T) -> TokenCell<T> {
        TokenCell(jsrt::cell::Cell::new(token.keep()))
    }

    pub fn get(&self) -> T {
        self.0.get()
    }

    pub fn set(&self, token: T) {
        self.0.set(token.keep());
    }
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
fn alloc_comments(comments: Option<Vec<Comment>>) -> Option<Comments> {
    comments.map(P::new)
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
        comments: Option<Comments>,
    ) -> P<Token> {
        alloc_plain_token(type_, start, length, comments)
    }

    // Comments prior to the token.
    pub fn comments(token: P<Token>) -> Option<&'static [Comment]> {
        Token::shared_comments(token).map(|comments| comments.get().as_slice())
    }

    /// The comment list itself, for a token that shares it.
    pub fn shared_comments(token: P<Token>) -> Option<Comments> {
        if !token.has_comments {
            return None;
        }
        // SAFETY: `alloc_token` allocates a token that has comments as a
        // `TokenAlloc<D, Comments>` whose `D` matches `kind`. The handle's pointer
        // covers the whole `TokenAlloc`.
        Some(with_interface!(token.kind, D => unsafe {
            (*token.as_ptr().cast::<TokenAlloc<D, Comments>>()).comments
        }))
    }

    /// `token` as a parse node holds it. A token in a scratch region is copied once
    /// to the program heap, where it outlives the region; later calls return the
    /// same copy. Any other token is itself.
    pub fn keep(token: P<Token>) -> P<Token> {
        if !token.scratch {
            return token;
        }
        // SAFETY: a scratch token is the `alloc.token` of a `ScratchToken`, whose
        // `kept` slot is the word before it. The handle's pointer covers the region's chunk.
        let kept = unsafe { &*token.as_ptr().cast::<Cell<Option<P<Token>>>>().sub(1) };
        if let Some(copy) = kept.get() {
            return copy;
        }
        // SAFETY: each arm copies the interface of its kind. Once `kept` is set, the
        // scratch token is never dropped (`TokenScratch::drop`): its buffers move to the copy.
        let copy = unsafe { with_interface!(token.kind, D => Token::copy_out::<D>(token)) };
        kept.set(Some(copy));
        copy
    }

    /// # Safety
    /// `scratch.kind` is `D::KIND`, and `scratch` is never dropped afterwards.
    unsafe fn copy_out<D: TokenInterface>(scratch: P<Token>) -> P<Token> {
        let token = Token {
            type_: scratch.type_,
            start: scratch.start,
            length: scratch.length,
            kind: scratch.kind,
            scratch: false,
            has_comments: scratch.has_comments,
        };
        // SAFETY: the caller guarantees the kind, and that the buffers move.
        let d = unsafe { ptr::read(Token::details_unchecked::<D>(scratch)) };
        match Token::shared_comments(scratch) {
            None => place_token(ptr::null(), TokenAlloc { token, d, comments: () }),
            Some(comments) => place_token(ptr::null(), TokenAlloc { token, d, comments }),
        }
    }

    /// The fields of interface `D`.
    fn details<D: TokenInterface>(token: P<Token>) -> &'static D {
        assert!(token.kind == D::KIND);
        // SAFETY: checked above.
        unsafe { Token::details_unchecked(token) }
    }

    /// # Safety
    /// `token.kind` is `D::KIND`.
    unsafe fn details_unchecked<D: TokenInterface>(token: P<Token>) -> &'static D {
        // SAFETY: `alloc_token` makes every header the first field of a `#[repr(C)]`
        // `TokenAlloc` whose fields match `kind`, on the program heap, which is never
        // freed, or in a scratch region, which outlives every use of its tokens. The
        // handle's pointer covers the whole `TokenAlloc`.
        unsafe { &(*token.as_ptr().cast::<TokenAlloc<D>>()).d }
    }
}

/// `token.d()`: the fields of the specific token interface.
impl jsrt::Header for Token {
    type Details = TokenDetails;

    fn details(token: P<Token>) -> TokenDetails {
        // SAFETY: each arm reads the interface of its kind.
        unsafe {
            match token.kind {
                TokenKind::None => TokenDetails::None,
                TokenKind::Indent => TokenDetails::Indent(Token::details_unchecked(token)),
                TokenKind::Dedent => TokenDetails::Dedent(Token::details_unchecked(token)),
                TokenKind::NewLine => TokenDetails::NewLine(Token::details_unchecked(token)),
                TokenKind::Keyword => TokenDetails::Keyword(Token::details_unchecked(token)),
                TokenKind::String => TokenDetails::String(Token::details_unchecked(token)),
                TokenKind::FStringStart => TokenDetails::FStringStart(Token::details_unchecked(token)),
                TokenKind::FStringMiddle => TokenDetails::FStringMiddle(Token::details_unchecked(token)),
                TokenKind::FStringEnd => TokenDetails::FStringEnd(Token::details_unchecked(token)),
                TokenKind::Number => TokenDetails::Number(Token::details_unchecked(token)),
                TokenKind::Operator => TokenDetails::Operator(Token::details_unchecked(token)),
                TokenKind::Identifier => TokenDetails::Identifier(Token::details_unchecked(token)),
            }
        }
    }
}

/// The fields that each specific token interface adds to `Token`: `token.d()`.
/// `None` for a plain `Token`.
#[derive(Clone, Copy)]
pub enum TokenDetails {
    None,
    Indent(&'static IndentTokenDetails),
    Dedent(&'static DedentTokenDetails),
    NewLine(&'static NewLineTokenDetails),
    Keyword(&'static KeywordTokenDetails),
    String(&'static StringTokenDetails),
    FStringStart(&'static FStringStartTokenDetails),
    FStringMiddle(&'static FStringMiddleTokenDetails),
    FStringEnd(&'static FStringEndTokenDetails),
    Number(&'static NumberTokenDetails),
    Operator(&'static OperatorTokenDetails),
    Identifier(&'static IdentifierTokenDetails),
}

macro_rules! token_handle {
    ($handle:ident, $variant:ident, $details:ident) => {
        impl TokenInterface for $details {
            const KIND: TokenKind = TokenKind::$variant;
        }

        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, jsrt::Trace)]
        pub struct $handle(pub P<Token>);

        impl $handle {
            pub fn cast(token: P<Token>) -> Option<$handle> {
                if token.kind == TokenKind::$variant {
                    Some($handle(token))
                } else {
                    None
                }
            }

            pub fn d(&self) -> &'static $details {
                Token::details(self.0)
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

        impl KeepToken for $handle {
            fn keep(self) -> Self {
                $handle(Token::keep(self.0))
            }
        }
    };
}

// parser/tokenizerTypes.ts:232
#[derive(jsrt::Trace)]
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
        IndentToken(alloc_token(
            TokenType::Indent,
            start,
            length,
            alloc_comments(comments),
            IndentTokenDetails {
                is_indent_ambiguous,
                indent_amount,
            },
        ))
    }
}

// parser/tokenizerTypes.ts:269
#[derive(jsrt::Trace)]
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
        DedentToken(alloc_token(
            TokenType::Dedent,
            start,
            length,
            alloc_comments(comments),
            DedentTokenDetails {
                indent_amount,
                matches_indent,
                is_dedent_ambiguous,
            },
        ))
    }
}

// parser/tokenizerTypes.ts:308
#[derive(jsrt::Trace)]
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
        NewLineToken(alloc_token(
            TokenType::NewLine,
            start,
            length,
            alloc_comments(comments),
            NewLineTokenDetails { new_line_type },
        ))
    }
}

// parser/tokenizerTypes.ts:339
#[derive(jsrt::Trace)]
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
        KeywordToken(alloc_token(
            TokenType::Keyword,
            start,
            length,
            alloc_comments(comments),
            KeywordTokenDetails { keyword_type },
        ))
    }

    // parser/tokenizerTypes.ts:369
    pub fn is_soft_keyword(token: KeywordToken) -> bool {
        SOFT_KEYWORDS.iter().any(|&t| token.d().keyword_type == t)
    }
}

// parser/tokenizerTypes.ts:374
#[derive(jsrt::Trace)]
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
        StringToken(alloc_token(
            TokenType::String,
            start,
            length,
            alloc_comments(comments),
            StringTokenDetails {
                flags,
                escaped_value,
                prefix_length,
                quote_mark_length,
            },
        ))
    }
}

// parser/tokenizerTypes.ts:425
#[derive(jsrt::Trace)]
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
        FStringStartToken(alloc_token(
            TokenType::FStringStart,
            start,
            length,
            alloc_comments(comments),
            FStringStartTokenDetails {
                flags,
                prefix_length,
                quote_mark_length,
            },
        ))
    }
}

// parser/tokenizerTypes.ts:470
#[derive(jsrt::Trace)]
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
        FStringMiddleToken(alloc_token(
            TokenType::FStringMiddle,
            start,
            length,
            None,
            FStringMiddleTokenDetails {
                flags,
                escaped_value,
            },
        ))
    }
}

// parser/tokenizerTypes.ts:492
#[derive(jsrt::Trace)]
pub struct FStringEndTokenDetails {
    pub flags: StringTokenFlags,
}
token_handle!(FStringEndToken, FStringEnd, FStringEndTokenDetails);

impl FStringEndToken {
    // parser/tokenizerTypes.ts:498
    pub fn create(start: i32, length: i32, flags: StringTokenFlags) -> FStringEndToken {
        FStringEndToken(alloc_token(
            TokenType::FStringEnd,
            start,
            length,
            None,
            FStringEndTokenDetails { flags },
        ))
    }
}

// parser/tokenizerTypes.ts:510
#[derive(jsrt::Trace)]
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
        NumberToken(alloc_token(
            TokenType::Number,
            start,
            length,
            alloc_comments(comments),
            NumberTokenDetails {
                is_integer,
                is_imaginary,
                value,
            },
        ))
    }
}

// parser/tokenizerTypes.ts:549
#[derive(jsrt::Trace)]
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
        OperatorToken(alloc_token(
            TokenType::Operator,
            start,
            length,
            alloc_comments(comments),
            OperatorTokenDetails { operator_type },
        ))
    }
}

// parser/tokenizerTypes.ts:580
#[derive(jsrt::Trace)]
pub struct IdentifierTokenDetails {
    /// Interned (`docs/porting.md`, "strings").
    pub value: Atom,
}
token_handle!(IdentifierToken, Identifier, IdentifierTokenDetails);

impl IdentifierToken {
    // parser/tokenizerTypes.ts:586
    pub fn create(
        start: i32,
        length: i32,
        value: impl Into<Atom>,
        comments: Option<Vec<Comment>>,
    ) -> IdentifierToken {
        IdentifierToken::create_with_shared_comments(start, length, value, alloc_comments(comments))
    }

    /// `IdentifierToken.create` with a comment list that another token already holds.
    pub fn create_with_shared_comments(
        start: i32,
        length: i32,
        value: impl Into<Atom>,
        comments: Option<Comments>,
    ) -> IdentifierToken {
        let value: Atom = value.into();
        // Perform "NFKC normalization", as per the Python lexical spec.
        let normalized_value = if value.bytes().any(|b| b > 0x7f) {
            Atom::new(&value.js_normalize(NormalizationForm::Nfkc))
        } else {
            value
        };

        IdentifierToken(alloc_token(
            TokenType::Identifier,
            start,
            length,
            comments,
            IdentifierTokenDetails {
                value: normalized_value,
            },
        ))
    }
}
