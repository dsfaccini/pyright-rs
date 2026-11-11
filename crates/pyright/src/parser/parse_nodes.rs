//! Port of `packages/pyright-internal/src/parser/parseNodes.ts`.
//!
//! Layout (see the parse-node section of `docs/porting.md`):
//! - `ParseNode` is the header every node shares. A node is allocated as its
//!   header followed by the details struct of its type (`NodeAlloc`); `d()`
//!   returns them as a `ParseNodeDetails` variant.
//! - Each node type has a typed handle, for example `FunctionNode(P<ParseNode>)`,
//!   with `cast`, `d()`, `Deref` to `ParseNode`, and its TypeScript `create`
//!   functions.
//! - A `d` field is a `Cell` (Copy values) or `RefCell<Vec<_>>` (arrays). A
//!   non-Copy value that TypeScript never writes after creation (`StringNode.value`,
//!   `NumberNode.value`, `ErrorNode.decorators`) is a plain field. `NameNode.value`,
//!   always its token's value, is read from the token: `value()`.
//! - A field typed as one node type holds that type's handle; a field typed as a
//!   union (`ExpressionNode`, `SuiteNode | IfNode`, ...) holds `P<ParseNode>`.

use jsrt::cell::{Cell, RefCell};
use std::ops::Deref;
use std::ptr;

use jsrt::{JsString, P};

use jsrt::NumberValue;

use super::string_annotation_info::{StringAnnotationInfo, empty_string_annotation_info};
use super::tokenizer_types::{
    FStringEndToken, FStringMiddleToken, FStringStartToken, IdentifierToken, KeywordToken,
    KeywordType, NumberToken, OperatorType, StringToken, Token, TokenType,
};
use crate::common::text_range::{HasTextRange, TextRange};

// parser/parseNodes.ts:27
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[derive(jsrt::Trace)]
pub enum ParseNodeType {
    Error, // 0

    Argument,
    Assert,
    Assignment,
    AssignmentExpression,
    AugmentedAssignment,
    Await,
    BinaryOperation,
    Break,
    Call,

    Class, // 10
    Comprehension,
    ComprehensionFor,
    ComprehensionIf,
    Constant,
    Continue,
    Decorator,
    Del,
    Dictionary,
    DictionaryExpandEntry,

    DictionaryKeyEntry, // 20
    Ellipsis,
    If,
    Import,
    ImportAs,
    ImportFrom,
    ImportFromAs,
    Index,
    Except,
    For,

    FormatString, // 30
    Function,
    Global,
    Lambda,
    List,
    MemberAccess,
    Module,
    ModuleName,
    Name,
    Nonlocal,

    Number, // 40
    Parameter,
    Pass,
    Raise,
    Return,
    Set,
    Slice,
    StatementList,
    StringList,
    String,

    Suite, // 50
    Ternary,
    Tuple,
    Try,
    TypeAnnotation,
    UnaryOperation,
    Unpack,
    While,
    With,
    WithItem,

    Yield, // 60
    YieldFrom,
    FunctionAnnotation,
    Match,
    Case,
    PatternSequence,
    PatternAs,
    PatternLiteral,
    PatternClass,
    PatternCapture,

    PatternMapping, // 70
    PatternMappingKeyEntry,
    PatternMappingExpandEntry,
    PatternValue,
    PatternClassArgument,
    TypeParameter,
    TypeParameterList,
    TypeAlias,
}

// parser/parseNodes.ts:116
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[derive(jsrt::Trace)]
pub enum ErrorExpressionCategory {
    MissingIn,
    MissingElse,
    MissingExpression,
    MissingIndexOrSlice,
    MissingDecoratorCallName,
    MissingCallCloseParen,
    MissingIndexCloseBracket,
    MissingMemberAccessName,
    MissingTupleCloseParen,
    MissingListCloseBracket,
    MissingFunctionParameterList,
    MissingPattern,
    MissingPatternSubject,
    MissingDictValue,
    MissingKeywordArgValue,
    MaxDepthExceeded,
}

/// The object behind a `ParseTreeKey`. In TypeScript the key of a file parse is
/// the module node itself, and the key of a detached expression parse is a
/// plain `{}`. Here both are this object: `module` is set for a file parse.
/// The string-annotation map that `setParserStringAnnotationInfo` stashes on
/// the key in TypeScript is the `string_annotations` field.
#[derive(jsrt::Trace)]
pub struct ParseTreeKeyObject {
    module: Cell<Option<ModuleNode>>,
    string_annotations: Cell<Option<StringAnnotationInfo>>,
}

// Opaque key used to retrieve binding information for a parse tree. Compared
// by identity.
// parser/parseNodes.ts:135
pub type ParseTreeKey = P<ParseTreeKeyObject>;

/// A standalone key, the `{}` that `Parser.parseTextExpression` creates.
pub fn create_parse_tree_key() -> ParseTreeKey {
    P::new(ParseTreeKeyObject {
        module: Cell::new(None),
        string_annotations: Cell::new(None),
    })
}

// parser/parseNodes.ts:147
pub fn set_parser_string_annotation_info(
    key: ParseTreeKey,
    string_annotations: StringAnnotationInfo,
) {
    key.string_annotations.set(Some(string_annotations));
}

// parser/parseNodes.ts:151
pub fn get_parse_tree_root(node: &ParseNode) -> Option<ModuleNode> {
    // The owner key is the module node itself for file parses, or a plain
    // standalone key object for detached expression parses. Only the former
    // has a real parse-tree root.
    node.a.module.get()
}

// parser/parseNodes.ts:159
pub fn get_parser_string_annotation(node: StringListNode) -> Option<ExpressionNode> {
    node.a
        .string_annotations
        .get()
        .and_then(|info| info.get(node))
}

// parser/parseNodes.ts:163
pub fn get_parser_string_annotation_info(node: &ParseNode) -> StringAnnotationInfo {
    node.a
        .string_annotations
        .get()
        .unwrap_or_else(empty_string_annotation_info)
}

// parser/parseNodes.ts:167
/// The fields every node shares. A node is allocated as a `NodeAlloc<D>`: this
/// header, then the details `D` of its type, so it takes the size of its own
/// details rather than of the largest. `d()` reads the details.
#[repr(C)]
#[derive(jsrt::Trace)]
// `_details` keeps construction inside this module; `#[non_exhaustive]` allows the crate.
#[allow(clippy::manual_non_exhaustive)]
pub struct ParseNode {
    pub node_type: ParseNodeType,
    pub start: Cell<i32>,
    pub length: Cell<i32>,

    // A unique ID given to each parse node.
    pub id: Cell<u32>,

    pub parent: Cell<Option<P<ParseNode>>>,

    // Opaque key used to retrieve binding information for this parse tree.
    pub a: ParseTreeKey,

    /// Private, so only `alloc_node` makes a header, always followed by its details.
    _details: (),
}
const _: () = assert!(size_of::<ParseNode>() <= 32);

impl HasTextRange for ParseNode {
    fn text_range(&self) -> TextRange {
        TextRange {
            start: self.start.get(),
            length: self.length.get(),
        }
    }
}

thread_local! {
    // parser/parseNodes.ts:184
    static _NEXT_NODE_ID: Cell<u32> = const { Cell::new(1) };
}

// parser/parseNodes.ts:185
pub fn get_next_node_id() -> u32 {
    _NEXT_NODE_ID.with(|n| {
        let id = n.get();
        n.set(id + 1);
        id
    })
}

// parser/parseNodes.ts:189
pub fn extend_range(node: &ParseNode, new_range: &(impl HasTextRange + ?Sized)) {
    let extended_range = TextRange::extend(node, new_range);

    // Temporarily allow writes to the range fields.
    node.start.set(extended_range.start);
    node.length.set(extended_range.length);
}

// parser/parseNodes.ts:197
pub type ParseNodeArray = Vec<Option<P<ParseNode>>>;

/// A node as allocated: the header, then the details of its type.
#[repr(C)]
#[derive(jsrt::Trace)]
struct NodeAlloc<D> {
    node: ParseNode,
    d: D,
}

/// The details struct of one node type.
pub trait NodeDetails: jsrt::Trace + Clone + 'static {
    const NODE_TYPE: ParseNodeType;
}

impl ParseNode {
    /// The details of a node of type `D::NODE_TYPE`.
    fn details<D: NodeDetails>(&self) -> &'static D {
        assert!(self.node_type == D::NODE_TYPE);
        // SAFETY: checked above.
        unsafe { self.details_unchecked() }
    }

    /// # Safety
    /// `self.node_type` is `D::NODE_TYPE`.
    unsafe fn details_unchecked<D: NodeDetails>(&self) -> &'static D {
        // SAFETY: `alloc_node` makes every header the first field of a `#[repr(C)]`
        // `NodeAlloc` whose details type matches `node_type`, on the program heap,
        // which is never freed.
        unsafe { &(*ptr::from_ref(self).cast::<NodeAlloc<D>>()).d }
    }

    /// `Object.assign({}, node)`: a new node with this node's fields, id included.
    /// The details are cloned: the copy holds the same child handles, as the shared
    /// TypeScript `d` object does.
    pub fn shallow_copy(&self) -> P<ParseNode> {
        let copy = self.clone_details_into_node();
        copy.parent.set(self.parent.get());
        copy
    }
}

fn alloc_node<D: NodeDetails>(start: i32, length: i32, id: u32, a: ParseTreeKey, d: D) -> P<ParseNode> {
    let node = ParseNode {
        node_type: D::NODE_TYPE,
        start: Cell::new(start),
        length: Cell::new(length),
        id: Cell::new(id),
        parent: Cell::new(None),
        a,
        _details: (),
    };
    P::new(NodeAlloc { node, d }).header(|alloc| &alloc.node)
}

/// The object-literal part of every `create`: a new node with the next id.
fn create_node<D: NodeDetails>(start: i32, length: i32, a: ParseTreeKey, d: D) -> P<ParseNode> {
    alloc_node(start, length, get_next_node_id(), a, d)
}

/// A detached error node with id 0, which takes nothing from the node counter.
pub fn create_placeholder_node() -> P<ParseNode> {
    alloc_node(
        0,
        0,
        0,
        create_parse_tree_key(),
        ErrorNodeDetails {
            category: Cell::new(ErrorExpressionCategory::MissingExpression),
            child: Cell::new(None),
            decorators: None,
        },
    )
}

fn set_parent(child: &ParseNode, parent: P<ParseNode>) {
    child.parent.set(Some(parent));
}

macro_rules! node_handle {
    ($handle:ident, $variant:ident, $details:ident) => {
        impl NodeDetails for $details {
            const NODE_TYPE: ParseNodeType = ParseNodeType::$variant;
        }

        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, jsrt::Trace)]
        pub struct $handle(pub P<ParseNode>);

        impl $handle {
            pub fn cast(node: P<ParseNode>) -> Option<$handle> {
                if node.node_type == ParseNodeType::$variant {
                    Some($handle(node))
                } else {
                    None
                }
            }

            pub fn d(&self) -> &'static $details {
                self.0.get().details()
            }

            pub fn node(self) -> P<ParseNode> {
                self.0
            }
        }

        impl Deref for $handle {
            type Target = ParseNode;
            fn deref(&self) -> &ParseNode {
                self.0.get()
            }
        }

        impl HasTextRange for $handle {
            fn text_range(&self) -> TextRange {
                self.0.text_range()
            }
        }

        impl From<$handle> for P<ParseNode> {
            fn from(handle: $handle) -> P<ParseNode> {
                handle.0
            }
        }

        impl PartialEq<P<ParseNode>> for $handle {
            fn eq(&self, other: &P<ParseNode>) -> bool {
                self.0 == *other
            }
        }

        impl PartialEq<$handle> for P<ParseNode> {
            fn eq(&self, other: &$handle) -> bool {
                *self == other.0
            }
        }
    };
}

macro_rules! parse_node_details {
    ($($variant:ident($details:ident),)*) => {
        /// The per-type details of a node, one variant per `ParseNodeType`: `node.d()`.
        #[derive(Clone, Copy)]
        pub enum ParseNodeDetails {
            $($variant(&'static $details),)*
        }

        impl ParseNode {
            /// The details of this node, as the variant of its type.
            pub fn d(&self) -> ParseNodeDetails {
                // SAFETY: each arm reads the details type of its node type.
                unsafe {
                    match self.node_type {
                        $(ParseNodeType::$variant => ParseNodeDetails::$variant(self.details_unchecked()),)*
                    }
                }
            }

            fn clone_details_into_node(&self) -> P<ParseNode> {
                let (start, length, id) = (self.start.get(), self.length.get(), self.id.get());
                // SAFETY: each arm reads the details type of its node type.
                unsafe {
                    match self.node_type {
                        $(ParseNodeType::$variant => {
                            alloc_node(start, length, id, self.a, self.details_unchecked::<$details>().clone())
                        })*
                    }
                }
            }
        }
    };
}

parse_node_details! {
    Error(ErrorNodeDetails),
    Argument(ArgumentNodeDetails),
    Assert(AssertNodeDetails),
    Assignment(AssignmentNodeDetails),
    AssignmentExpression(AssignmentExpressionNodeDetails),
    AugmentedAssignment(AugmentedAssignmentNodeDetails),
    Await(AwaitNodeDetails),
    BinaryOperation(BinaryOperationNodeDetails),
    Break(BreakNodeDetails),
    Call(CallNodeDetails),
    Class(ClassNodeDetails),
    Comprehension(ComprehensionNodeDetails),
    ComprehensionFor(ComprehensionForNodeDetails),
    ComprehensionIf(ComprehensionIfNodeDetails),
    Constant(ConstantNodeDetails),
    Continue(ContinueNodeDetails),
    Decorator(DecoratorNodeDetails),
    Del(DelNodeDetails),
    Dictionary(DictionaryNodeDetails),
    DictionaryExpandEntry(DictionaryExpandEntryNodeDetails),
    DictionaryKeyEntry(DictionaryKeyEntryNodeDetails),
    Ellipsis(EllipsisNodeDetails),
    If(IfNodeDetails),
    Import(ImportNodeDetails),
    ImportAs(ImportAsNodeDetails),
    ImportFrom(ImportFromNodeDetails),
    ImportFromAs(ImportFromAsNodeDetails),
    Index(IndexNodeDetails),
    Except(ExceptNodeDetails),
    For(ForNodeDetails),
    FormatString(FormatStringNodeDetails),
    Function(FunctionNodeDetails),
    Global(GlobalNodeDetails),
    Lambda(LambdaNodeDetails),
    List(ListNodeDetails),
    MemberAccess(MemberAccessNodeDetails),
    Module(ModuleNodeDetails),
    ModuleName(ModuleNameNodeDetails),
    Name(NameNodeDetails),
    Nonlocal(NonlocalNodeDetails),
    Number(NumberNodeDetails),
    Parameter(ParameterNodeDetails),
    Pass(PassNodeDetails),
    Raise(RaiseNodeDetails),
    Return(ReturnNodeDetails),
    Set(SetNodeDetails),
    Slice(SliceNodeDetails),
    StatementList(StatementListNodeDetails),
    StringList(StringListNodeDetails),
    String(StringNodeDetails),
    Suite(SuiteNodeDetails),
    Ternary(TernaryNodeDetails),
    Tuple(TupleNodeDetails),
    Try(TryNodeDetails),
    TypeAnnotation(TypeAnnotationNodeDetails),
    UnaryOperation(UnaryOperationNodeDetails),
    Unpack(UnpackNodeDetails),
    While(WhileNodeDetails),
    With(WithNodeDetails),
    WithItem(WithItemNodeDetails),
    Yield(YieldNodeDetails),
    YieldFrom(YieldFromNodeDetails),
    FunctionAnnotation(FunctionAnnotationNodeDetails),
    Match(MatchNodeDetails),
    Case(CaseNodeDetails),
    PatternSequence(PatternSequenceNodeDetails),
    PatternAs(PatternAsNodeDetails),
    PatternLiteral(PatternLiteralNodeDetails),
    PatternClass(PatternClassNodeDetails),
    PatternCapture(PatternCaptureNodeDetails),
    PatternMapping(PatternMappingNodeDetails),
    PatternMappingKeyEntry(PatternMappingKeyEntryNodeDetails),
    PatternMappingExpandEntry(PatternMappingExpandEntryNodeDetails),
    PatternValue(PatternValueNodeDetails),
    PatternClassArgument(PatternClassArgumentNodeDetails),
    TypeParameter(TypeParameterNodeDetails),
    TypeParameterList(TypeParameterListNodeDetails),
    TypeAlias(TypeAliasNodeDetails),
}

// ---------------------------------------------------------------------------
// Module

// parser/parseNodes.ts:199
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct ModuleNodeDetails {
    pub statements: RefCell<Vec<StatementNode>>,
}
node_handle!(ModuleNode, Module, ModuleNodeDetails);

impl ModuleNode {
    // parser/parseNodes.ts:206
    pub fn create(range: &(impl HasTextRange + ?Sized)) -> ModuleNode {
        let range = range.text_range();
        let key = create_parse_tree_key();
        let node = ModuleNode(create_node(
            range.start,
            range.length,
            key,
            ModuleNodeDetails {
                statements: RefCell::new(vec![]),
            },
        ));

        // The module node is its own owner key; every structural node in a file
        // parse shares this reference via its `a` field.
        key.module.set(Some(node));
        node
    }
}

// parser/parseNodes.ts:224
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct SuiteNodeDetails {
    pub statements: RefCell<Vec<StatementNode>>,
    pub type_comment: Cell<Option<StringToken>>,
}
node_handle!(SuiteNode, Suite, SuiteNodeDetails);

impl SuiteNode {
    // parser/parseNodes.ts:232
    pub fn create(key: ParseTreeKey, range: &(impl HasTextRange + ?Sized)) -> SuiteNode {
        let range = range.text_range();
        SuiteNode(create_node(
            range.start,
            range.length,
            key,
            SuiteNodeDetails {
                statements: RefCell::new(vec![]),
                type_comment: Cell::new(None),
            },
        ))
    }
}

// parser/parseNodes.ts:250
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct IfNodeDetails {
    pub first_token: Cell<P<Token>>,
    pub test_expr: Cell<ExpressionNode>,
    pub if_suite: Cell<SuiteNode>,
    /// `SuiteNode | IfNode`.
    pub else_suite: Cell<Option<P<ParseNode>>>,
}
node_handle!(IfNode, If, IfNodeDetails);

impl IfNode {
    // parser/parseNodes.ts:260
    pub fn create(
        key: ParseTreeKey,
        if_or_elif_token: P<Token>,
        test_expr: ExpressionNode,
        if_suite: SuiteNode,
        else_suite: Option<SuiteNode>,
    ) -> IfNode {
        let node = create_node(
            if_or_elif_token.start,
            if_or_elif_token.length,
            key,
            IfNodeDetails {
                first_token: Cell::new(if_or_elif_token),
                test_expr: Cell::new(test_expr),
                if_suite: Cell::new(if_suite),
                else_suite: Cell::new(else_suite.map(|s| s.0)),
            },
        );

        set_parent(&test_expr, node);
        set_parent(&if_suite, node);

        extend_range(&node, &test_expr);
        extend_range(&node, &if_suite);
        if let Some(else_suite) = else_suite {
            extend_range(&node, &else_suite);
            set_parent(&else_suite, node);
        }

        IfNode(node)
    }
}

// parser/parseNodes.ts:296
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct WhileNodeDetails {
    pub first_token: Cell<P<Token>>,
    pub test_expr: Cell<ExpressionNode>,
    pub while_suite: Cell<SuiteNode>,
    pub else_suite: Cell<Option<SuiteNode>>,
}
node_handle!(WhileNode, While, WhileNodeDetails);

impl WhileNode {
    // parser/parseNodes.ts:306
    pub fn create(
        key: ParseTreeKey,
        while_token: P<Token>,
        test_expr: ExpressionNode,
        while_suite: SuiteNode,
    ) -> WhileNode {
        let node = create_node(
            while_token.start,
            while_token.length,
            key,
            WhileNodeDetails {
                first_token: Cell::new(while_token),
                test_expr: Cell::new(test_expr),
                while_suite: Cell::new(while_suite),
                else_suite: Cell::new(None),
            },
        );

        set_parent(&test_expr, node);
        set_parent(&while_suite, node);

        extend_range(&node, &while_suite);

        WhileNode(node)
    }
}

// parser/parseNodes.ts:330
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct ForNodeDetails {
    pub first_token: Cell<P<Token>>,
    pub is_async: Cell<Option<bool>>,
    pub async_token: Cell<Option<P<Token>>>,
    pub target_expr: Cell<ExpressionNode>,
    pub iterable_expr: Cell<ExpressionNode>,
    pub for_suite: Cell<SuiteNode>,
    pub else_suite: Cell<Option<SuiteNode>>,
    pub type_comment: Cell<Option<StringToken>>,
}
node_handle!(ForNode, For, ForNodeDetails);

impl ForNode {
    // parser/parseNodes.ts:344
    pub fn create(
        key: ParseTreeKey,
        for_token: P<Token>,
        target_expr: ExpressionNode,
        iterable_expr: ExpressionNode,
        for_suite: SuiteNode,
    ) -> ForNode {
        let node = create_node(
            for_token.start,
            for_token.length,
            key,
            ForNodeDetails {
                first_token: Cell::new(for_token),
                is_async: Cell::new(None),
                async_token: Cell::new(None),
                target_expr: Cell::new(target_expr),
                iterable_expr: Cell::new(iterable_expr),
                for_suite: Cell::new(for_suite),
                else_suite: Cell::new(None),
                type_comment: Cell::new(None),
            },
        );

        set_parent(&target_expr, node);
        set_parent(&iterable_expr, node);
        set_parent(&for_suite, node);

        extend_range(&node, &for_suite);

        ForNode(node)
    }
}

// parser/parseNodes.ts:376
pub type ComprehensionForIfNode = P<ParseNode>;

// parser/parseNodes.ts:378
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct ComprehensionForNodeDetails {
    pub is_async: Cell<Option<bool>>,
    pub async_token: Cell<Option<P<Token>>>,
    pub target_expr: Cell<ExpressionNode>,
    pub iterable_expr: Cell<ExpressionNode>,
}
node_handle!(
    ComprehensionForNode,
    ComprehensionFor,
    ComprehensionForNodeDetails
);

impl ComprehensionForNode {
    // parser/parseNodes.ts:388
    pub fn create(
        key: ParseTreeKey,
        start_token: P<Token>,
        target_expr: ExpressionNode,
        iterable_expr: ExpressionNode,
    ) -> ComprehensionForNode {
        let node = create_node(
            start_token.start,
            start_token.length,
            key,
            ComprehensionForNodeDetails {
                is_async: Cell::new(None),
                async_token: Cell::new(None),
                target_expr: Cell::new(target_expr),
                iterable_expr: Cell::new(iterable_expr),
            },
        );

        set_parent(&target_expr, node);
        set_parent(&iterable_expr, node);

        extend_range(&node, &target_expr);
        extend_range(&node, &iterable_expr);

        ComprehensionForNode(node)
    }
}

// parser/parseNodes.ts:417
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct ComprehensionIfNodeDetails {
    pub test_expr: Cell<ExpressionNode>,
}
node_handle!(
    ComprehensionIfNode,
    ComprehensionIf,
    ComprehensionIfNodeDetails
);

impl ComprehensionIfNode {
    // parser/parseNodes.ts:424
    pub fn create(
        key: ParseTreeKey,
        if_token: P<Token>,
        test_expr: ExpressionNode,
    ) -> ComprehensionIfNode {
        let node = create_node(
            if_token.start,
            if_token.length,
            key,
            ComprehensionIfNodeDetails {
                test_expr: Cell::new(test_expr),
            },
        );

        set_parent(&test_expr, node);

        extend_range(&node, &test_expr);

        ComprehensionIfNode(node)
    }
}

// parser/parseNodes.ts:443
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct TryNodeDetails {
    pub first_token: Cell<P<Token>>,
    pub try_suite: Cell<SuiteNode>,
    pub except_clauses: RefCell<Vec<ExceptNode>>,
    pub else_suite: Cell<Option<SuiteNode>>,
    pub finally_suite: Cell<Option<SuiteNode>>,
}
node_handle!(TryNode, Try, TryNodeDetails);

impl TryNode {
    // parser/parseNodes.ts:454
    pub fn create(key: ParseTreeKey, try_token: P<Token>, try_suite: SuiteNode) -> TryNode {
        let node = create_node(
            try_token.start,
            try_token.length,
            key,
            TryNodeDetails {
                first_token: Cell::new(try_token),
                try_suite: Cell::new(try_suite),
                except_clauses: RefCell::new(vec![]),
                else_suite: Cell::new(None),
                finally_suite: Cell::new(None),
            },
        );

        set_parent(&try_suite, node);

        extend_range(&node, &try_suite);

        TryNode(node)
    }
}

// parser/parseNodes.ts:477
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct ExceptNodeDetails {
    pub type_expr: Cell<Option<ExpressionNode>>,
    pub name: Cell<Option<NameNode>>,
    pub except_suite: Cell<SuiteNode>,
    pub is_except_group: Cell<bool>,
    pub except_token: Cell<P<Token>>,
}
node_handle!(ExceptNode, Except, ExceptNodeDetails);

impl ExceptNode {
    // parser/parseNodes.ts:488
    pub fn create(
        key: ParseTreeKey,
        except_token: P<Token>,
        except_suite: SuiteNode,
        is_except_group: bool,
    ) -> ExceptNode {
        let node = create_node(
            except_token.start,
            except_token.length,
            key,
            ExceptNodeDetails {
                type_expr: Cell::new(None),
                name: Cell::new(None),
                except_suite: Cell::new(except_suite),
                is_except_group: Cell::new(is_except_group),
                except_token: Cell::new(except_token),
            },
        );

        set_parent(&except_suite, node);

        extend_range(&node, &except_suite);

        ExceptNode(node)
    }
}

// parser/parseNodes.ts:511
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct FunctionNodeDetails {
    pub first_token: Cell<P<Token>>,
    pub decorators: RefCell<Vec<DecoratorNode>>,
    pub is_async: Cell<bool>,
    pub name: Cell<NameNode>,
    pub type_params: Cell<Option<TypeParameterListNode>>,
    pub params: RefCell<Vec<ParameterNode>>,
    pub return_annotation: Cell<Option<ExpressionNode>>,
    pub func_annotation_comment: Cell<Option<FunctionAnnotationNode>>,
    pub suite: Cell<SuiteNode>,
}
node_handle!(FunctionNode, Function, FunctionNodeDetails);

impl FunctionNode {
    // parser/parseNodes.ts:526
    pub fn create(
        key: ParseTreeKey,
        def_token: P<Token>,
        name: NameNode,
        suite: SuiteNode,
        type_params: Option<TypeParameterListNode>,
    ) -> FunctionNode {
        let node = create_node(
            def_token.start,
            def_token.length,
            key,
            FunctionNodeDetails {
                first_token: Cell::new(def_token),
                decorators: RefCell::new(vec![]),
                is_async: Cell::new(false),
                name: Cell::new(name),
                type_params: Cell::new(type_params),
                params: RefCell::new(vec![]),
                return_annotation: Cell::new(None),
                func_annotation_comment: Cell::new(None),
                suite: Cell::new(suite),
            },
        );

        set_parent(&name, node);
        set_parent(&suite, node);

        if let Some(type_params) = type_params {
            set_parent(&type_params, node);
        }

        extend_range(&node, &suite);

        FunctionNode(node)
    }
}

// parser/parseNodes.ts:566
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[derive(jsrt::Trace)]
pub enum ParamCategory {
    Simple,
    ArgsList,
    KwargsDict,
}

// parser/parseNodes.ts:572
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct ParameterNodeDetails {
    pub category: Cell<ParamCategory>,
    pub name: Cell<Option<NameNode>>,
    pub annotation: Cell<Option<ExpressionNode>>,
    pub annotation_comment: Cell<Option<ExpressionNode>>,
    pub default_value: Cell<Option<ExpressionNode>>,
}
node_handle!(ParameterNode, Parameter, ParameterNodeDetails);

impl ParameterNode {
    // parser/parseNodes.ts:583
    pub fn create(
        key: ParseTreeKey,
        start_token: P<Token>,
        param_category: ParamCategory,
    ) -> ParameterNode {
        ParameterNode(create_node(
            start_token.start,
            start_token.length,
            key,
            ParameterNodeDetails {
                category: Cell::new(param_category),
                name: Cell::new(None),
                annotation: Cell::new(None),
                annotation_comment: Cell::new(None),
                default_value: Cell::new(None),
            },
        ))
    }
}

// parser/parseNodes.ts:604
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct ClassNodeDetails {
    pub first_token: Cell<P<Token>>,
    pub decorators: RefCell<Vec<DecoratorNode>>,
    pub name: Cell<NameNode>,
    pub type_params: Cell<Option<TypeParameterListNode>>,
    pub arguments: RefCell<Vec<ArgumentNode>>,
    pub suite: Cell<SuiteNode>,
}
node_handle!(ClassNode, Class, ClassNodeDetails);

impl ClassNode {
    // parser/parseNodes.ts:616
    pub fn create(
        key: ParseTreeKey,
        class_token: P<Token>,
        name: NameNode,
        suite: SuiteNode,
        type_params: Option<TypeParameterListNode>,
    ) -> ClassNode {
        let node = create_node(
            class_token.start,
            class_token.length,
            key,
            ClassNodeDetails {
                first_token: Cell::new(class_token),
                decorators: RefCell::new(vec![]),
                name: Cell::new(name),
                type_params: Cell::new(type_params),
                arguments: RefCell::new(vec![]),
                suite: Cell::new(suite),
            },
        );

        set_parent(&name, node);
        set_parent(&suite, node);

        if let Some(type_params) = type_params {
            set_parent(&type_params, node);
        }

        extend_range(&node, &suite);

        ClassNode(node)
    }

    // This variant is used to create a dummy class
    // when the parser encounters decorators with no
    // function or class declaration.
    // parser/parseNodes.ts:655
    pub fn create_dummy_for_decorators(
        key: ParseTreeKey,
        decorators: Vec<DecoratorNode>,
    ) -> ClassNode {
        let decorators_start = decorators[0].start.get();

        // The node's own id comes first; the nested name and suite literals have id 0.
        let id = get_next_node_id();

        let first_token = Token::create(TokenType::Keyword, 0, 0, Some(vec![]));
        let name_token = IdentifierToken::create(0, 0, String::new(), Some(vec![]));
        let name = NameNode(alloc_node(
            decorators_start,
            0,
            0,
            key,
            NameNodeDetails {
                token: Cell::new(name_token),
            },
        ));
        let suite = SuiteNode(alloc_node(
            decorators_start,
            0,
            0,
            key,
            SuiteNodeDetails {
                statements: RefCell::new(vec![]),
                type_comment: Cell::new(None),
            },
        ));

        let node = alloc_node(
            decorators_start,
            0,
            id,
            key,
            ClassNodeDetails {
                first_token: Cell::new(first_token),
                decorators: RefCell::new(decorators.clone()),
                name: Cell::new(name),
                type_params: Cell::new(None),
                arguments: RefCell::new(vec![]),
                suite: Cell::new(suite),
            },
        );

        for decorator in &decorators {
            set_parent(decorator, node);
            extend_range(&node, decorator);
        }

        set_parent(&name, node);
        set_parent(&suite, node);

        ClassNode(node)
    }
}

// parser/parseNodes.ts:715
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct WithNodeDetails {
    pub first_token: Cell<P<Token>>,
    pub is_async: Cell<Option<bool>>,
    pub async_token: Cell<Option<P<Token>>>,
    pub with_items: RefCell<Vec<WithItemNode>>,
    pub suite: Cell<SuiteNode>,
    pub type_comment: Cell<Option<StringToken>>,
}
node_handle!(WithNode, With, WithNodeDetails);

impl WithNode {
    // parser/parseNodes.ts:727
    pub fn create(key: ParseTreeKey, with_token: P<Token>, suite: SuiteNode) -> WithNode {
        let node = create_node(
            with_token.start,
            with_token.length,
            key,
            WithNodeDetails {
                first_token: Cell::new(with_token),
                is_async: Cell::new(None),
                async_token: Cell::new(None),
                with_items: RefCell::new(vec![]),
                suite: Cell::new(suite),
                type_comment: Cell::new(None),
            },
        );

        set_parent(&suite, node);

        extend_range(&node, &suite);

        WithNode(node)
    }
}

// parser/parseNodes.ts:750
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct WithItemNodeDetails {
    pub expr: Cell<ExpressionNode>,
    pub target: Cell<Option<ExpressionNode>>,
}
node_handle!(WithItemNode, WithItem, WithItemNodeDetails);

impl WithItemNode {
    // parser/parseNodes.ts:758
    pub fn create(key: ParseTreeKey, expr: ExpressionNode) -> WithItemNode {
        let node = create_node(
            expr.start.get(),
            expr.length.get(),
            key,
            WithItemNodeDetails {
                expr: Cell::new(expr),
                target: Cell::new(None),
            },
        );

        set_parent(&expr, node);

        WithItemNode(node)
    }
}

// parser/parseNodes.ts:775
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct DecoratorNodeDetails {
    pub expr: Cell<ExpressionNode>,
}
node_handle!(DecoratorNode, Decorator, DecoratorNodeDetails);

impl DecoratorNode {
    // parser/parseNodes.ts:782
    pub fn create(key: ParseTreeKey, at_token: P<Token>, expr: ExpressionNode) -> DecoratorNode {
        let node = create_node(
            at_token.start,
            at_token.length,
            key,
            DecoratorNodeDetails {
                expr: Cell::new(expr),
            },
        );

        set_parent(&expr, node);

        extend_range(&node, &expr);

        DecoratorNode(node)
    }
}

// parser/parseNodes.ts:801
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct StatementListNodeDetails {
    pub first_token: Cell<P<Token>>,
    pub statements: RefCell<Vec<P<ParseNode>>>,
}
node_handle!(StatementListNode, StatementList, StatementListNodeDetails);

impl StatementListNode {
    // parser/parseNodes.ts:809
    pub fn create(key: ParseTreeKey, at_token: P<Token>) -> StatementListNode {
        StatementListNode(create_node(
            at_token.start,
            at_token.length,
            key,
            StatementListNodeDetails {
                first_token: Cell::new(at_token),
                statements: RefCell::new(vec![]),
            },
        ))
    }
}

// IfNode | WhileNode | ForNode | TryNode | FunctionNode | ClassNode | WithNode
// | StatementListNode | MatchNode | TypeAliasNode | ErrorNode
// parser/parseNodes.ts:824
pub type StatementNode = P<ParseNode>;

// ExpressionNode | DelNode | PassNode | ImportNode | GlobalNode | NonlocalNode | AssertNode
// parser/parseNodes.ts:837
pub type SmallStatementNode = P<ParseNode>;

// parser/parseNodes.ts:846
pub type ExpressionNode = P<ParseNode>;

// parser/parseNodes.ts:877
pub fn is_expression_node(node: &ParseNode) -> bool {
    matches!(
        node.node_type,
        ParseNodeType::Error
            | ParseNodeType::UnaryOperation
            | ParseNodeType::BinaryOperation
            | ParseNodeType::AssignmentExpression
            | ParseNodeType::TypeAnnotation
            | ParseNodeType::Await
            | ParseNodeType::Ternary
            | ParseNodeType::Unpack
            | ParseNodeType::Tuple
            | ParseNodeType::Call
            | ParseNodeType::Comprehension
            | ParseNodeType::Index
            | ParseNodeType::Slice
            | ParseNodeType::Yield
            | ParseNodeType::YieldFrom
            | ParseNodeType::MemberAccess
            | ParseNodeType::Lambda
            | ParseNodeType::Name
            | ParseNodeType::Constant
            | ParseNodeType::Ellipsis
            | ParseNodeType::Number
            | ParseNodeType::String
            | ParseNodeType::FormatString
            | ParseNodeType::StringList
            | ParseNodeType::Dictionary
            | ParseNodeType::List
            | ParseNodeType::Set
    )
}

// parser/parseNodes.ts:913
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct ErrorNodeDetails {
    pub category: Cell<ErrorExpressionCategory>,
    pub child: Cell<Option<ExpressionNode>>,
    pub decorators: Option<Vec<DecoratorNode>>,
}
node_handle!(ErrorNode, Error, ErrorNodeDetails);

impl ErrorNode {
    // parser/parseNodes.ts:922
    pub fn create(
        key: ParseTreeKey,
        initial_range: &(impl HasTextRange + ?Sized),
        category: ErrorExpressionCategory,
        child: Option<ExpressionNode>,
        decorators: Option<Vec<DecoratorNode>>,
    ) -> ErrorNode {
        let initial_range = initial_range.text_range();
        let decorator_list = decorators.clone();
        let node = create_node(
            initial_range.start,
            initial_range.length,
            key,
            ErrorNodeDetails {
                category: Cell::new(category),
                child: Cell::new(child),
                decorators,
            },
        );

        if let Some(child) = child {
            set_parent(&child, node);
            extend_range(&node, &child);
        }

        if let Some(decorators) = decorator_list {
            for decorator in &decorators {
                set_parent(decorator, node);
            }

            if !decorators.is_empty() {
                extend_range(&node, &decorators[0]);
            }
        }

        ErrorNode(node)
    }
}

// parser/parseNodes.ts:962
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct UnaryOperationNodeDetails {
    pub expr: Cell<ExpressionNode>,
    pub operator_token: Cell<P<Token>>,
    pub operator: Cell<OperatorType>,
    pub has_parens: Cell<bool>,
}
node_handle!(
    UnaryOperationNode,
    UnaryOperation,
    UnaryOperationNodeDetails
);

impl UnaryOperationNode {
    // parser/parseNodes.ts:972
    pub fn create(
        key: ParseTreeKey,
        operator_token: P<Token>,
        expr: ExpressionNode,
        operator: OperatorType,
    ) -> UnaryOperationNode {
        let node = create_node(
            operator_token.start,
            operator_token.length,
            key,
            UnaryOperationNodeDetails {
                operator: Cell::new(operator),
                operator_token: Cell::new(operator_token),
                expr: Cell::new(expr),
                has_parens: Cell::new(false),
            },
        );

        set_parent(&expr, node);

        extend_range(&node, &expr);

        UnaryOperationNode(node)
    }
}

// parser/parseNodes.ts:996
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct BinaryOperationNodeDetails {
    pub left_expr: Cell<ExpressionNode>,
    pub operator_token: Cell<P<Token>>,
    pub operator: Cell<OperatorType>,
    pub right_expr: Cell<ExpressionNode>,
    pub has_parens: Cell<bool>,
}
node_handle!(
    BinaryOperationNode,
    BinaryOperation,
    BinaryOperationNodeDetails
);

impl BinaryOperationNode {
    // parser/parseNodes.ts:1007
    pub fn create(
        key: ParseTreeKey,
        left_expr: ExpressionNode,
        right_expr: ExpressionNode,
        operator_token: P<Token>,
        operator: OperatorType,
    ) -> BinaryOperationNode {
        let node = create_node(
            left_expr.start.get(),
            left_expr.length.get(),
            key,
            BinaryOperationNodeDetails {
                left_expr: Cell::new(left_expr),
                operator_token: Cell::new(operator_token),
                operator: Cell::new(operator),
                right_expr: Cell::new(right_expr),
                has_parens: Cell::new(false),
            },
        );

        set_parent(&left_expr, node);
        set_parent(&right_expr, node);

        extend_range(&node, &right_expr);

        BinaryOperationNode(node)
    }
}

// parser/parseNodes.ts:1039
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct AssignmentExpressionNodeDetails {
    pub name: Cell<NameNode>,
    pub walrus_token: Cell<P<Token>>,
    pub right_expr: Cell<ExpressionNode>,
    pub has_parens: Cell<bool>,
}
node_handle!(
    AssignmentExpressionNode,
    AssignmentExpression,
    AssignmentExpressionNodeDetails
);

impl AssignmentExpressionNode {
    // parser/parseNodes.ts:1049
    pub fn create(
        key: ParseTreeKey,
        name: NameNode,
        walrus_token: P<Token>,
        right_expr: ExpressionNode,
    ) -> AssignmentExpressionNode {
        let node = create_node(
            name.start.get(),
            name.length.get(),
            key,
            AssignmentExpressionNodeDetails {
                name: Cell::new(name),
                walrus_token: Cell::new(walrus_token),
                right_expr: Cell::new(right_expr),
                has_parens: Cell::new(false),
            },
        );

        set_parent(&name, node);
        set_parent(&right_expr, node);

        extend_range(&node, &right_expr);

        AssignmentExpressionNode(node)
    }
}

// parser/parseNodes.ts:1074
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct AssignmentNodeDetails {
    pub left_expr: Cell<ExpressionNode>,
    pub right_expr: Cell<ExpressionNode>,
    pub annotation_comment: Cell<Option<ExpressionNode>>,
    pub chained_annotation_comment: Cell<Option<ExpressionNode>>,
}
node_handle!(AssignmentNode, Assignment, AssignmentNodeDetails);

impl AssignmentNode {
    // parser/parseNodes.ts:1084
    pub fn create(
        key: ParseTreeKey,
        left_expr: ExpressionNode,
        right_expr: ExpressionNode,
    ) -> AssignmentNode {
        let node = create_node(
            left_expr.start.get(),
            left_expr.length.get(),
            key,
            AssignmentNodeDetails {
                left_expr: Cell::new(left_expr),
                right_expr: Cell::new(right_expr),
                annotation_comment: Cell::new(None),
                chained_annotation_comment: Cell::new(None),
            },
        );

        set_parent(&left_expr, node);
        set_parent(&right_expr, node);

        extend_range(&node, &right_expr);

        AssignmentNode(node)
    }
}

// parser/parseNodes.ts:1107
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[derive(jsrt::Trace)]
pub enum TypeParamKind {
    TypeVar,
    TypeVarTuple,
    ParamSpec,
}

// parser/parseNodes.ts:1113
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct TypeParameterNodeDetails {
    pub name: Cell<NameNode>,
    pub type_param_kind: Cell<TypeParamKind>,
    pub bound_expr: Cell<Option<ExpressionNode>>,
    pub default_expr: Cell<Option<ExpressionNode>>,
}
node_handle!(TypeParameterNode, TypeParameter, TypeParameterNodeDetails);

impl TypeParameterNode {
    // parser/parseNodes.ts:1123
    pub fn create(
        key: ParseTreeKey,
        name: NameNode,
        type_param_kind: TypeParamKind,
        bound_expr: Option<ExpressionNode>,
        default_expr: Option<ExpressionNode>,
    ) -> TypeParameterNode {
        let node = create_node(
            name.start.get(),
            name.length.get(),
            key,
            TypeParameterNodeDetails {
                name: Cell::new(name),
                type_param_kind: Cell::new(type_param_kind),
                bound_expr: Cell::new(bound_expr),
                default_expr: Cell::new(default_expr),
            },
        );

        set_parent(&name, node);

        if let Some(bound_expr) = bound_expr {
            set_parent(&bound_expr, node);
            extend_range(&node, &bound_expr);
        }

        if let Some(default_expr) = default_expr {
            set_parent(&default_expr, node);
            extend_range(&node, &default_expr);
        }

        TypeParameterNode(node)
    }
}

// parser/parseNodes.ts:1161
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct TypeParameterListNodeDetails {
    pub params: RefCell<Vec<TypeParameterNode>>,
}
node_handle!(
    TypeParameterListNode,
    TypeParameterList,
    TypeParameterListNodeDetails
);

impl TypeParameterListNode {
    // parser/parseNodes.ts:1168
    pub fn create(
        key: ParseTreeKey,
        start_token: P<Token>,
        end_token: P<Token>,
        params: Vec<TypeParameterNode>,
    ) -> TypeParameterListNode {
        let node = create_node(
            start_token.start,
            start_token.length,
            key,
            TypeParameterListNodeDetails {
                params: RefCell::new(params.clone()),
            },
        );

        extend_range(&node, &end_token);

        for param in &params {
            extend_range(&node, param);
            set_parent(param, node);
        }

        TypeParameterListNode(node)
    }
}

// parser/parseNodes.ts:1190
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct TypeAliasNodeDetails {
    pub first_token: Cell<P<Token>>,
    pub name: Cell<NameNode>,
    pub type_params: Cell<Option<TypeParameterListNode>>,
    pub expr: Cell<ExpressionNode>,
}
node_handle!(TypeAliasNode, TypeAlias, TypeAliasNodeDetails);

impl TypeAliasNode {
    // parser/parseNodes.ts:1200
    pub fn create(
        key: ParseTreeKey,
        type_token: KeywordToken,
        name: NameNode,
        expr: ExpressionNode,
        type_params: Option<TypeParameterListNode>,
    ) -> TypeAliasNode {
        let node = create_node(
            type_token.start,
            type_token.length,
            key,
            TypeAliasNodeDetails {
                first_token: Cell::new(type_token.0),
                name: Cell::new(name),
                type_params: Cell::new(type_params),
                expr: Cell::new(expr),
            },
        );

        set_parent(&name, node);
        set_parent(&expr, node);

        if let Some(type_params) = type_params {
            set_parent(&type_params, node);
        }

        extend_range(&node, &expr);

        TypeAliasNode(node)
    }
}

// parser/parseNodes.ts:1235
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct TypeAnnotationNodeDetails {
    pub value_expr: Cell<ExpressionNode>,
    pub annotation: Cell<ExpressionNode>,
}
node_handle!(
    TypeAnnotationNode,
    TypeAnnotation,
    TypeAnnotationNodeDetails
);

impl TypeAnnotationNode {
    // parser/parseNodes.ts:1243
    pub fn create(
        key: ParseTreeKey,
        value_expr: ExpressionNode,
        annotation: ExpressionNode,
    ) -> TypeAnnotationNode {
        let node = create_node(
            value_expr.start.get(),
            value_expr.length.get(),
            key,
            TypeAnnotationNodeDetails {
                value_expr: Cell::new(value_expr),
                annotation: Cell::new(annotation),
            },
        );

        set_parent(&value_expr, node);
        set_parent(&annotation, node);

        extend_range(&node, &annotation);

        TypeAnnotationNode(node)
    }
}

// parser/parseNodes.ts:1266
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct FunctionAnnotationNodeDetails {
    pub is_ellipsis: Cell<bool>,
    pub param_annotations: RefCell<Vec<ExpressionNode>>,
    pub return_annotation: Cell<ExpressionNode>,
}
node_handle!(
    FunctionAnnotationNode,
    FunctionAnnotation,
    FunctionAnnotationNodeDetails
);

impl FunctionAnnotationNode {
    // parser/parseNodes.ts:1275
    pub fn create(
        key: ParseTreeKey,
        open_paren_token: P<Token>,
        is_ellipsis: bool,
        param_annotations: Vec<ExpressionNode>,
        return_annotation: ExpressionNode,
    ) -> FunctionAnnotationNode {
        let node = create_node(
            open_paren_token.start,
            open_paren_token.length,
            key,
            FunctionAnnotationNodeDetails {
                is_ellipsis: Cell::new(is_ellipsis),
                param_annotations: RefCell::new(param_annotations.clone()),
                return_annotation: Cell::new(return_annotation),
            },
        );

        for p in &param_annotations {
            set_parent(p, node);
        }
        set_parent(&return_annotation, node);

        extend_range(&node, &return_annotation);

        FunctionAnnotationNode(node)
    }
}

// parser/parseNodes.ts:1307
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct AugmentedAssignmentNodeDetails {
    pub left_expr: Cell<ExpressionNode>,
    pub operator: Cell<OperatorType>,
    pub right_expr: Cell<ExpressionNode>,

    // The destExpression is a copy of the leftExpression
    // node. We use it as a place to hang the result type,
    // as opposed to the source type.
    pub dest_expr: Cell<ExpressionNode>,
}
node_handle!(
    AugmentedAssignmentNode,
    AugmentedAssignment,
    AugmentedAssignmentNodeDetails
);

impl AugmentedAssignmentNode {
    // parser/parseNodes.ts:1321
    pub fn create(
        key: ParseTreeKey,
        left_expr: ExpressionNode,
        right_expr: ExpressionNode,
        operator: OperatorType,
        dest_expr: ExpressionNode,
    ) -> AugmentedAssignmentNode {
        let node = create_node(
            left_expr.start.get(),
            left_expr.length.get(),
            key,
            AugmentedAssignmentNodeDetails {
                left_expr: Cell::new(left_expr),
                operator: Cell::new(operator),
                right_expr: Cell::new(right_expr),
                dest_expr: Cell::new(dest_expr),
            },
        );

        set_parent(&left_expr, node);
        set_parent(&right_expr, node);
        set_parent(&dest_expr, node);

        extend_range(&node, &right_expr);

        AugmentedAssignmentNode(node)
    }
}

// parser/parseNodes.ts:1353
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct AwaitNodeDetails {
    pub expr: Cell<ExpressionNode>,
    pub await_token: Cell<P<Token>>,
    pub has_parens: Cell<bool>,
}
node_handle!(AwaitNode, Await, AwaitNodeDetails);

impl AwaitNode {
    // parser/parseNodes.ts:1362
    pub fn create(key: ParseTreeKey, await_token: P<Token>, expr: ExpressionNode) -> AwaitNode {
        let node = create_node(
            await_token.start,
            await_token.length,
            key,
            AwaitNodeDetails {
                expr: Cell::new(expr),
                await_token: Cell::new(await_token),
                has_parens: Cell::new(false),
            },
        );

        set_parent(&expr, node);

        extend_range(&node, &expr);

        AwaitNode(node)
    }
}

// parser/parseNodes.ts:1381
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct TernaryNodeDetails {
    pub if_expr: Cell<ExpressionNode>,
    pub test_expr: Cell<ExpressionNode>,
    pub else_expr: Cell<ExpressionNode>,
}
node_handle!(TernaryNode, Ternary, TernaryNodeDetails);

impl TernaryNode {
    // parser/parseNodes.ts:1390
    pub fn create(
        key: ParseTreeKey,
        if_expr: ExpressionNode,
        test_expr: ExpressionNode,
        else_expr: ExpressionNode,
    ) -> TernaryNode {
        let node = create_node(
            if_expr.start.get(),
            if_expr.length.get(),
            key,
            TernaryNodeDetails {
                if_expr: Cell::new(if_expr),
                test_expr: Cell::new(test_expr),
                else_expr: Cell::new(else_expr),
            },
        );

        set_parent(&if_expr, node);
        set_parent(&test_expr, node);
        set_parent(&else_expr, node);

        extend_range(&node, &else_expr);

        TernaryNode(node)
    }
}

// parser/parseNodes.ts:1420
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct UnpackNodeDetails {
    pub expr: Cell<ExpressionNode>,
    pub star_token: Cell<P<Token>>,
}
node_handle!(UnpackNode, Unpack, UnpackNodeDetails);

impl UnpackNode {
    // parser/parseNodes.ts:1428
    pub fn create(key: ParseTreeKey, star_token: P<Token>, expr: ExpressionNode) -> UnpackNode {
        let node = create_node(
            star_token.start,
            star_token.length,
            key,
            UnpackNodeDetails {
                expr: Cell::new(expr),
                star_token: Cell::new(star_token),
            },
        );

        set_parent(&expr, node);

        extend_range(&node, &expr);

        UnpackNode(node)
    }
}

// parser/parseNodes.ts:1450
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct TupleNodeDetails {
    pub items: RefCell<Vec<ExpressionNode>>,
    pub has_parens: Cell<bool>,
}
node_handle!(TupleNode, Tuple, TupleNodeDetails);

impl TupleNode {
    // parser/parseNodes.ts:1458
    pub fn create(
        key: ParseTreeKey,
        range: &(impl HasTextRange + ?Sized),
        has_parens: bool,
    ) -> TupleNode {
        let range = range.text_range();
        TupleNode(create_node(
            range.start,
            range.length,
            key,
            TupleNodeDetails {
                items: RefCell::new(vec![]),
                has_parens: Cell::new(has_parens),
            },
        ))
    }
}

// parser/parseNodes.ts:1476
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct CallNodeDetails {
    pub left_expr: Cell<ExpressionNode>,
    pub args: RefCell<Vec<ArgumentNode>>,
    pub trailing_comma: Cell<bool>,
}
node_handle!(CallNode, Call, CallNodeDetails);

impl CallNode {
    // parser/parseNodes.ts:1485
    pub fn create(
        key: ParseTreeKey,
        left_expr: ExpressionNode,
        args: Vec<ArgumentNode>,
        trailing_comma: bool,
    ) -> CallNode {
        let node = create_node(
            left_expr.start.get(),
            left_expr.length.get(),
            key,
            CallNodeDetails {
                left_expr: Cell::new(left_expr),
                args: RefCell::new(args.clone()),
                trailing_comma: Cell::new(trailing_comma),
            },
        );

        set_parent(&left_expr, node);

        if !args.is_empty() {
            for arg in &args {
                set_parent(arg, node);
            }
            extend_range(&node, &args[args.len() - 1]);
        }

        CallNode(node)
    }
}

// parser/parseNodes.ts:1513
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct ComprehensionNodeDetails {
    pub expr: Cell<P<ParseNode>>,
    pub for_if_nodes: RefCell<Vec<ComprehensionForIfNode>>,
    pub is_generator: Cell<bool>,
    pub has_parens: Cell<bool>,
}
node_handle!(ComprehensionNode, Comprehension, ComprehensionNodeDetails);

impl ComprehensionNode {
    // parser/parseNodes.ts:1523
    pub fn create(key: ParseTreeKey, expr: P<ParseNode>, is_generator: bool) -> ComprehensionNode {
        let node = create_node(
            expr.start.get(),
            expr.length.get(),
            key,
            ComprehensionNodeDetails {
                expr: Cell::new(expr),
                for_if_nodes: RefCell::new(vec![]),
                is_generator: Cell::new(is_generator),
                has_parens: Cell::new(false),
            },
        );

        set_parent(&expr, node);

        ComprehensionNode(node)
    }
}

// parser/parseNodes.ts:1545
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct IndexNodeDetails {
    pub left_expr: Cell<ExpressionNode>,
    pub items: RefCell<Vec<ArgumentNode>>,
    pub trailing_comma: Cell<bool>,
}
node_handle!(IndexNode, Index, IndexNodeDetails);

impl IndexNode {
    // parser/parseNodes.ts:1554
    pub fn create(
        key: ParseTreeKey,
        left_expr: ExpressionNode,
        items: Vec<ArgumentNode>,
        trailing_comma: bool,
        close_bracket_token: P<Token>,
    ) -> IndexNode {
        let node = create_node(
            left_expr.start.get(),
            left_expr.length.get(),
            key,
            IndexNodeDetails {
                left_expr: Cell::new(left_expr),
                items: RefCell::new(items.clone()),
                trailing_comma: Cell::new(trailing_comma),
            },
        );

        set_parent(&left_expr, node);
        for item in &items {
            set_parent(item, node);
        }

        extend_range(&node, &close_bracket_token);

        IndexNode(node)
    }
}

// parser/parseNodes.ts:1586
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct SliceNodeDetails {
    pub start_value: Cell<Option<ExpressionNode>>,
    pub end_value: Cell<Option<ExpressionNode>>,
    pub step_value: Cell<Option<ExpressionNode>>,
}
node_handle!(SliceNode, Slice, SliceNodeDetails);

impl SliceNode {
    // parser/parseNodes.ts:1595
    pub fn create(key: ParseTreeKey, range: &(impl HasTextRange + ?Sized)) -> SliceNode {
        let range = range.text_range();
        SliceNode(create_node(
            range.start,
            range.length,
            key,
            SliceNodeDetails {
                start_value: Cell::new(None),
                end_value: Cell::new(None),
                step_value: Cell::new(None),
            },
        ))
    }
}

// parser/parseNodes.ts:1610
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct YieldNodeDetails {
    pub expr: Cell<Option<ExpressionNode>>,
}
node_handle!(YieldNode, Yield, YieldNodeDetails);

impl YieldNode {
    // parser/parseNodes.ts:1617
    pub fn create(
        key: ParseTreeKey,
        yield_token: P<Token>,
        expr: Option<ExpressionNode>,
    ) -> YieldNode {
        let node = create_node(
            yield_token.start,
            yield_token.length,
            key,
            YieldNodeDetails {
                expr: Cell::new(expr),
            },
        );

        if let Some(expr) = expr {
            set_parent(&expr, node);
            extend_range(&node, &expr);
        }

        YieldNode(node)
    }
}

// parser/parseNodes.ts:1637
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct YieldFromNodeDetails {
    pub expr: Cell<ExpressionNode>,
}
node_handle!(YieldFromNode, YieldFrom, YieldFromNodeDetails);

impl YieldFromNode {
    // parser/parseNodes.ts:1644
    pub fn create(key: ParseTreeKey, yield_token: P<Token>, expr: ExpressionNode) -> YieldFromNode {
        let node = create_node(
            yield_token.start,
            yield_token.length,
            key,
            YieldFromNodeDetails {
                expr: Cell::new(expr),
            },
        );

        set_parent(&expr, node);

        extend_range(&node, &expr);

        YieldFromNode(node)
    }
}

// parser/parseNodes.ts:1663
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct MemberAccessNodeDetails {
    pub left_expr: Cell<ExpressionNode>,
    pub member: Cell<NameNode>,
}
node_handle!(MemberAccessNode, MemberAccess, MemberAccessNodeDetails);

impl MemberAccessNode {
    // parser/parseNodes.ts:1671
    pub fn create(
        key: ParseTreeKey,
        left_expr: ExpressionNode,
        member: NameNode,
    ) -> MemberAccessNode {
        let node = create_node(
            left_expr.start.get(),
            left_expr.length.get(),
            key,
            MemberAccessNodeDetails {
                left_expr: Cell::new(left_expr),
                member: Cell::new(member),
            },
        );

        set_parent(&left_expr, node);
        set_parent(&member, node);

        extend_range(&node, &member);

        MemberAccessNode(node)
    }
}

// parser/parseNodes.ts:1694
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct LambdaNodeDetails {
    pub params: RefCell<Vec<ParameterNode>>,
    pub expr: Cell<ExpressionNode>,
}
node_handle!(LambdaNode, Lambda, LambdaNodeDetails);

impl LambdaNode {
    // parser/parseNodes.ts:1702
    pub fn create(key: ParseTreeKey, lambda_token: P<Token>, expr: ExpressionNode) -> LambdaNode {
        let node = create_node(
            lambda_token.start,
            lambda_token.length,
            key,
            LambdaNodeDetails {
                params: RefCell::new(vec![]),
                expr: Cell::new(expr),
            },
        );

        set_parent(&expr, node);

        extend_range(&node, &expr);

        LambdaNode(node)
    }
}

// parser/parseNodes.ts:1724
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct NameNodeDetails {
    pub token: Cell<IdentifierToken>,
}
node_handle!(NameNode, Name, NameNodeDetails);

impl NameNodeDetails {
    /// `value`, which TypeScript sets to `token.value` and never writes again,
    /// read from the token.
    pub fn value(&self) -> &'static str {
        &self.token.get().d().value
    }
}

impl NameNode {
    // parser/parseNodes.ts:1732
    pub fn create(key: ParseTreeKey, name_token: IdentifierToken) -> NameNode {
        NameNode(create_node(
            name_token.start,
            name_token.length,
            key,
            NameNodeDetails {
                token: Cell::new(name_token),
            },
        ))
    }
}

// parser/parseNodes.ts:1750
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct ConstantNodeDetails {
    pub const_type: Cell<KeywordType>,
}
node_handle!(ConstantNode, Constant, ConstantNodeDetails);

impl ConstantNode {
    // parser/parseNodes.ts:1757
    pub fn create(key: ParseTreeKey, token: KeywordToken) -> ConstantNode {
        ConstantNode(create_node(
            token.start,
            token.length,
            key,
            ConstantNodeDetails {
                const_type: Cell::new(token.d().keyword_type),
            },
        ))
    }
}

// parser/parseNodes.ts:1772
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct EllipsisNodeDetails {}
node_handle!(EllipsisNode, Ellipsis, EllipsisNodeDetails);

impl EllipsisNode {
    // parser/parseNodes.ts:1775
    pub fn create(key: ParseTreeKey, range: &(impl HasTextRange + ?Sized)) -> EllipsisNode {
        let range = range.text_range();
        EllipsisNode(create_node(
            range.start,
            range.length,
            key,
            EllipsisNodeDetails {},
        ))
    }
}

// parser/parseNodes.ts:1790
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct NumberNodeDetails {
    pub value: NumberValue,
    pub is_integer: Cell<bool>,
    pub is_imaginary: Cell<bool>,
}
node_handle!(NumberNode, Number, NumberNodeDetails);

impl NumberNode {
    // parser/parseNodes.ts:1799
    pub fn create(key: ParseTreeKey, token: NumberToken) -> NumberNode {
        NumberNode(create_node(
            token.start,
            token.length,
            key,
            NumberNodeDetails {
                value: token.d().value.clone(),
                is_integer: Cell::new(token.d().is_integer),
                is_imaginary: Cell::new(token.d().is_imaginary),
            },
        ))
    }
}

// parser/parseNodes.ts:1818
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct StringNodeDetails {
    pub token: Cell<StringToken>,
    pub value: JsString,
}
node_handle!(StringNode, String, StringNodeDetails);

impl StringNode {
    // parser/parseNodes.ts:1826
    pub fn create(key: ParseTreeKey, token: StringToken, value: JsString) -> StringNode {
        StringNode(create_node(
            token.start,
            token.length,
            key,
            StringNodeDetails {
                token: Cell::new(token),
                value,
            },
        ))
    }
}

// Used for both f-strings and t-strings
// parser/parseNodes.ts:1845
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct FormatStringNodeDetails {
    pub token: Cell<FStringStartToken>,
    pub middle_tokens: RefCell<Vec<FStringMiddleToken>>,
    pub field_exprs: RefCell<Vec<ExpressionNode>>,
    pub format_exprs: RefCell<Vec<ExpressionNode>>,

    // Include a dummy "value" to simplify other code.
    pub value: JsString,
}
node_handle!(FormatStringNode, FormatString, FormatStringNodeDetails);

impl FormatStringNode {
    // parser/parseNodes.ts:1858
    pub fn create(
        key: ParseTreeKey,
        start_token: FStringStartToken,
        end_token: Option<FStringEndToken>,
        middle_tokens: Vec<FStringMiddleToken>,
        field_exprs: Vec<ExpressionNode>,
        format_exprs: Vec<ExpressionNode>,
    ) -> FormatStringNode {
        let node = create_node(
            start_token.start,
            start_token.length,
            key,
            FormatStringNodeDetails {
                token: Cell::new(start_token),
                middle_tokens: RefCell::new(middle_tokens),
                field_exprs: RefCell::new(field_exprs.clone()),
                format_exprs: RefCell::new(format_exprs.clone()),
                value: JsString::from(""),
            },
        );

        for expr in &field_exprs {
            set_parent(expr, node);
            extend_range(&node, expr);
        }

        for expr in &format_exprs {
            set_parent(expr, node);
            extend_range(&node, expr);
        }

        if let Some(end_token) = end_token {
            extend_range(&node, &end_token);
        }

        FormatStringNode(node)
    }
}

// parser/parseNodes.ts:1902
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct StringListNodeDetails {
    /// `StringNode | FormatStringNode`.
    pub strings: RefCell<Vec<P<ParseNode>>>,

    // Indicates that the string list is enclosed in parens.
    pub has_parens: Cell<bool>,
}
node_handle!(StringListNode, StringList, StringListNodeDetails);

impl StringListNode {
    // parser/parseNodes.ts:1912
    pub fn create(key: ParseTreeKey, strings: Vec<P<ParseNode>>) -> StringListNode {
        let node = create_node(
            strings[0].start.get(),
            strings[0].length.get(),
            key,
            StringListNodeDetails {
                strings: RefCell::new(strings.clone()),
                has_parens: Cell::new(false),
            },
        );

        if !strings.is_empty() {
            for str in &strings {
                set_parent(str, node);
            }
            extend_range(&node, &strings[strings.len() - 1]);
        }

        StringListNode(node)
    }
}

// parser/parseNodes.ts:1937
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct DictionaryNodeDetails {
    pub items: RefCell<Vec<DictionaryEntryNode>>,
    pub trailing_comma_token: Cell<Option<P<Token>>>,
}
node_handle!(DictionaryNode, Dictionary, DictionaryNodeDetails);

impl DictionaryNode {
    // parser/parseNodes.ts:1945
    pub fn create(key: ParseTreeKey, range: &(impl HasTextRange + ?Sized)) -> DictionaryNode {
        let range = range.text_range();
        DictionaryNode(create_node(
            range.start,
            range.length,
            key,
            DictionaryNodeDetails {
                items: RefCell::new(vec![]),
                trailing_comma_token: Cell::new(None),
            },
        ))
    }
}

// parser/parseNodes.ts:1963
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct DictionaryKeyEntryNodeDetails {
    pub key_expr: Cell<ExpressionNode>,
    pub value_expr: Cell<ExpressionNode>,
}
node_handle!(
    DictionaryKeyEntryNode,
    DictionaryKeyEntry,
    DictionaryKeyEntryNodeDetails
);

impl DictionaryKeyEntryNode {
    // parser/parseNodes.ts:1971
    pub fn create(
        key: ParseTreeKey,
        key_expr: ExpressionNode,
        value_expr: ExpressionNode,
    ) -> DictionaryKeyEntryNode {
        let node = create_node(
            key_expr.start.get(),
            key_expr.length.get(),
            key,
            DictionaryKeyEntryNodeDetails {
                key_expr: Cell::new(key_expr),
                value_expr: Cell::new(value_expr),
            },
        );

        set_parent(&key_expr, node);
        set_parent(&value_expr, node);

        extend_range(&node, &value_expr);

        DictionaryKeyEntryNode(node)
    }
}

// parser/parseNodes.ts:1994
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct DictionaryExpandEntryNodeDetails {
    pub expr: Cell<ExpressionNode>,
}
node_handle!(
    DictionaryExpandEntryNode,
    DictionaryExpandEntry,
    DictionaryExpandEntryNodeDetails
);

impl DictionaryExpandEntryNode {
    // parser/parseNodes.ts:2001
    pub fn create(key: ParseTreeKey, expr: ExpressionNode) -> DictionaryExpandEntryNode {
        let node = create_node(
            expr.start.get(),
            expr.length.get(),
            key,
            DictionaryExpandEntryNodeDetails {
                expr: Cell::new(expr),
            },
        );

        set_parent(&expr, node);

        DictionaryExpandEntryNode(node)
    }
}

// DictionaryKeyEntryNode | DictionaryExpandEntryNode | ComprehensionNode
// parser/parseNodes.ts:2018
pub type DictionaryEntryNode = P<ParseNode>;

// parser/parseNodes.ts:2020
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct SetNodeDetails {
    pub items: RefCell<Vec<ExpressionNode>>,
}
node_handle!(SetNode, Set, SetNodeDetails);

impl SetNode {
    // parser/parseNodes.ts:2025
    pub fn create(key: ParseTreeKey, range: &(impl HasTextRange + ?Sized)) -> SetNode {
        let range = range.text_range();
        SetNode(create_node(
            range.start,
            range.length,
            key,
            SetNodeDetails {
                items: RefCell::new(vec![]),
            },
        ))
    }
}

// parser/parseNodes.ts:2040
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct ListNodeDetails {
    pub items: RefCell<Vec<ExpressionNode>>,
}
node_handle!(ListNode, List, ListNodeDetails);

impl ListNode {
    // parser/parseNodes.ts:2047
    pub fn create(key: ParseTreeKey, range: &(impl HasTextRange + ?Sized)) -> ListNode {
        let range = range.text_range();
        ListNode(create_node(
            range.start,
            range.length,
            key,
            ListNodeDetails {
                items: RefCell::new(vec![]),
            },
        ))
    }
}

// parser/parseNodes.ts:2062
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[derive(jsrt::Trace)]
pub enum ArgCategory {
    Simple,
    UnpackedList,
    UnpackedDictionary,
}

// parser/parseNodes.ts:2068
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct ArgumentNodeDetails {
    pub arg_category: Cell<ArgCategory>,
    pub name: Cell<Option<NameNode>>,
    pub value_expr: Cell<ExpressionNode>,
}
node_handle!(ArgumentNode, Argument, ArgumentNodeDetails);

impl ArgumentNode {
    // parser/parseNodes.ts:2077
    pub fn create(
        key: ParseTreeKey,
        start_token: Option<P<Token>>,
        value_expr: ExpressionNode,
        arg_category: ArgCategory,
    ) -> ArgumentNode {
        let node = create_node(
            match start_token {
                Some(t) => t.start,
                None => value_expr.start.get(),
            },
            match start_token {
                Some(t) => t.length,
                None => value_expr.length.get(),
            },
            key,
            ArgumentNodeDetails {
                arg_category: Cell::new(arg_category),
                name: Cell::new(None),
                value_expr: Cell::new(value_expr),
            },
        );

        set_parent(&value_expr, node);

        extend_range(&node, &value_expr);

        ArgumentNode(node)
    }
}

// parser/parseNodes.ts:2105
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct DelNodeDetails {
    pub targets: RefCell<Vec<ExpressionNode>>,
}
node_handle!(DelNode, Del, DelNodeDetails);

impl DelNode {
    // parser/parseNodes.ts:2112
    pub fn create(key: ParseTreeKey, del_token: P<Token>) -> DelNode {
        DelNode(create_node(
            del_token.start,
            del_token.length,
            key,
            DelNodeDetails {
                targets: RefCell::new(vec![]),
            },
        ))
    }
}

// parser/parseNodes.ts:2127
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct PassNodeDetails {}
node_handle!(PassNode, Pass, PassNodeDetails);

impl PassNode {
    // parser/parseNodes.ts:2130
    pub fn create(key: ParseTreeKey, pass_token: &(impl HasTextRange + ?Sized)) -> PassNode {
        let range = pass_token.text_range();
        PassNode(create_node(
            range.start,
            range.length,
            key,
            PassNodeDetails {},
        ))
    }
}

// parser/parseNodes.ts:2145
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct ImportNodeDetails {
    pub list: RefCell<Vec<ImportAsNode>>,
    pub is_lazy: Cell<Option<bool>>,
    pub lazy_token: Cell<Option<KeywordToken>>,
}
node_handle!(ImportNode, Import, ImportNodeDetails);

impl ImportNode {
    // parser/parseNodes.ts:2154
    pub fn create(key: ParseTreeKey, import_token: &(impl HasTextRange + ?Sized)) -> ImportNode {
        let range = import_token.text_range();
        ImportNode(create_node(
            range.start,
            range.length,
            key,
            ImportNodeDetails {
                list: RefCell::new(vec![]),
                is_lazy: Cell::new(None),
                lazy_token: Cell::new(None),
            },
        ))
    }
}

// parser/parseNodes.ts:2169
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct ModuleNameNodeDetails {
    pub leading_dots: Cell<i32>,
    pub name_parts: RefCell<Vec<NameNode>>,

    // This is an error condition used only for type completion.
    pub has_trailing_dot: Cell<Option<bool>>,
}
node_handle!(ModuleNameNode, ModuleName, ModuleNameNodeDetails);

impl ModuleNameNode {
    // parser/parseNodes.ts:2180
    pub fn create(key: ParseTreeKey, range: &(impl HasTextRange + ?Sized)) -> ModuleNameNode {
        let range = range.text_range();
        ModuleNameNode(create_node(
            range.start,
            range.length,
            key,
            ModuleNameNodeDetails {
                leading_dots: Cell::new(0),
                name_parts: RefCell::new(vec![]),
                has_trailing_dot: Cell::new(None),
            },
        ))
    }
}

// parser/parseNodes.ts:2198
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct ImportAsNodeDetails {
    pub module: Cell<ModuleNameNode>,
    pub alias: Cell<Option<NameNode>>,
}
node_handle!(ImportAsNode, ImportAs, ImportAsNodeDetails);

impl ImportAsNode {
    // parser/parseNodes.ts:2206
    pub fn create(key: ParseTreeKey, module: ModuleNameNode) -> ImportAsNode {
        let node = create_node(
            module.start.get(),
            module.length.get(),
            key,
            ImportAsNodeDetails {
                module: Cell::new(module),
                alias: Cell::new(None),
            },
        );

        set_parent(&module, node);

        ImportAsNode(node)
    }
}

// parser/parseNodes.ts:2223
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct ImportFromNodeDetails {
    pub module: Cell<ModuleNameNode>,
    pub imports: RefCell<Vec<ImportFromAsNode>>,
    pub is_wildcard_import: Cell<bool>,
    pub uses_parens: Cell<bool>,
    pub wildcard_token: Cell<Option<P<Token>>>,
    pub missing_import: Cell<Option<bool>>,
    pub is_lazy: Cell<Option<bool>>,
    pub lazy_token: Cell<Option<KeywordToken>>,
}
node_handle!(ImportFromNode, ImportFrom, ImportFromNodeDetails);

impl ImportFromNode {
    // parser/parseNodes.ts:2237
    pub fn create(
        key: ParseTreeKey,
        from_token: P<Token>,
        module: ModuleNameNode,
    ) -> ImportFromNode {
        let node = create_node(
            from_token.start,
            from_token.length,
            key,
            ImportFromNodeDetails {
                module: Cell::new(module),
                imports: RefCell::new(vec![]),
                is_wildcard_import: Cell::new(false),
                uses_parens: Cell::new(false),
                wildcard_token: Cell::new(None),
                missing_import: Cell::new(None),
                is_lazy: Cell::new(None),
                lazy_token: Cell::new(None),
            },
        );

        set_parent(&module, node);

        extend_range(&node, &module);

        ImportFromNode(node)
    }
}

// parser/parseNodes.ts:2261
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct ImportFromAsNodeDetails {
    pub name: Cell<NameNode>,
    pub alias: Cell<Option<NameNode>>,
}
node_handle!(ImportFromAsNode, ImportFromAs, ImportFromAsNodeDetails);

impl ImportFromAsNode {
    // parser/parseNodes.ts:2269
    pub fn create(key: ParseTreeKey, name: NameNode) -> ImportFromAsNode {
        let node = create_node(
            name.start.get(),
            name.length.get(),
            key,
            ImportFromAsNodeDetails {
                name: Cell::new(name),
                alias: Cell::new(None),
            },
        );

        set_parent(&name, node);

        ImportFromAsNode(node)
    }
}

// parser/parseNodes.ts:2286
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct GlobalNodeDetails {
    pub targets: RefCell<Vec<NameNode>>,
}
node_handle!(GlobalNode, Global, GlobalNodeDetails);

impl GlobalNode {
    // parser/parseNodes.ts:2293
    pub fn create(key: ParseTreeKey, range: &(impl HasTextRange + ?Sized)) -> GlobalNode {
        let range = range.text_range();
        GlobalNode(create_node(
            range.start,
            range.length,
            key,
            GlobalNodeDetails {
                targets: RefCell::new(vec![]),
            },
        ))
    }
}

// parser/parseNodes.ts:2308
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct NonlocalNodeDetails {
    pub targets: RefCell<Vec<NameNode>>,
}
node_handle!(NonlocalNode, Nonlocal, NonlocalNodeDetails);

impl NonlocalNode {
    // parser/parseNodes.ts:2315
    pub fn create(key: ParseTreeKey, range: &(impl HasTextRange + ?Sized)) -> NonlocalNode {
        let range = range.text_range();
        NonlocalNode(create_node(
            range.start,
            range.length,
            key,
            NonlocalNodeDetails {
                targets: RefCell::new(vec![]),
            },
        ))
    }
}

// parser/parseNodes.ts:2330
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct AssertNodeDetails {
    pub test_expr: Cell<ExpressionNode>,
    pub exception_expr: Cell<Option<ExpressionNode>>,
}
node_handle!(AssertNode, Assert, AssertNodeDetails);

impl AssertNode {
    // parser/parseNodes.ts:2338
    pub fn create(
        key: ParseTreeKey,
        assert_token: P<Token>,
        test_expr: ExpressionNode,
    ) -> AssertNode {
        let node = create_node(
            assert_token.start,
            assert_token.length,
            key,
            AssertNodeDetails {
                test_expr: Cell::new(test_expr),
                exception_expr: Cell::new(None),
            },
        );

        set_parent(&test_expr, node);

        extend_range(&node, &test_expr);

        AssertNode(node)
    }
}

// parser/parseNodes.ts:2357
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct BreakNodeDetails {}
node_handle!(BreakNode, Break, BreakNodeDetails);

impl BreakNode {
    // parser/parseNodes.ts:2360
    pub fn create(key: ParseTreeKey, range: &(impl HasTextRange + ?Sized)) -> BreakNode {
        let range = range.text_range();
        BreakNode(create_node(
            range.start,
            range.length,
            key,
            BreakNodeDetails {},
        ))
    }
}

// parser/parseNodes.ts:2375
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct ContinueNodeDetails {}
node_handle!(ContinueNode, Continue, ContinueNodeDetails);

impl ContinueNode {
    // parser/parseNodes.ts:2378
    pub fn create(key: ParseTreeKey, range: &(impl HasTextRange + ?Sized)) -> ContinueNode {
        let range = range.text_range();
        ContinueNode(create_node(
            range.start,
            range.length,
            key,
            ContinueNodeDetails {},
        ))
    }
}

// parser/parseNodes.ts:2393
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct ReturnNodeDetails {
    pub expr: Cell<Option<ExpressionNode>>,
}
node_handle!(ReturnNode, Return, ReturnNodeDetails);

impl ReturnNode {
    // parser/parseNodes.ts:2400
    pub fn create(key: ParseTreeKey, range: &(impl HasTextRange + ?Sized)) -> ReturnNode {
        let range = range.text_range();
        ReturnNode(create_node(
            range.start,
            range.length,
            key,
            ReturnNodeDetails {
                expr: Cell::new(None),
            },
        ))
    }
}

// parser/parseNodes.ts:2415
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct RaiseNodeDetails {
    pub expr: Cell<Option<ExpressionNode>>,
    pub from_expr: Cell<Option<ExpressionNode>>,
}
node_handle!(RaiseNode, Raise, RaiseNodeDetails);

impl RaiseNode {
    // parser/parseNodes.ts:2423
    pub fn create(key: ParseTreeKey, range: &(impl HasTextRange + ?Sized)) -> RaiseNode {
        let range = range.text_range();
        RaiseNode(create_node(
            range.start,
            range.length,
            key,
            RaiseNodeDetails {
                expr: Cell::new(None),
                from_expr: Cell::new(None),
            },
        ))
    }
}

// parser/parseNodes.ts:2438
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct MatchNodeDetails {
    pub first_token: Cell<P<Token>>,
    pub expr: Cell<ExpressionNode>,
    pub cases: RefCell<Vec<CaseNode>>,
}
node_handle!(MatchNode, Match, MatchNodeDetails);

impl MatchNode {
    // parser/parseNodes.ts:2447
    pub fn create(key: ParseTreeKey, match_token: P<Token>, expr: ExpressionNode) -> MatchNode {
        let node = create_node(
            match_token.start,
            match_token.length,
            key,
            MatchNodeDetails {
                first_token: Cell::new(match_token),
                expr: Cell::new(expr),
                cases: RefCell::new(vec![]),
            },
        );

        set_parent(&expr, node);

        extend_range(&node, &expr);

        MatchNode(node)
    }
}

// parser/parseNodes.ts:2470
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct CaseNodeDetails {
    pub pattern: Cell<PatternAtomNode>,
    pub is_irrefutable: Cell<bool>,
    pub guard_expr: Cell<Option<ExpressionNode>>,
    pub suite: Cell<SuiteNode>,
}
node_handle!(CaseNode, Case, CaseNodeDetails);

impl CaseNode {
    // parser/parseNodes.ts:2480
    pub fn create(
        key: ParseTreeKey,
        case_token: &(impl HasTextRange + ?Sized),
        pattern: PatternAtomNode,
        is_irrefutable: bool,
        guard_expr: Option<ExpressionNode>,
        suite: SuiteNode,
    ) -> CaseNode {
        let range = case_token.text_range();
        let node = create_node(
            range.start,
            range.length,
            key,
            CaseNodeDetails {
                pattern: Cell::new(pattern),
                is_irrefutable: Cell::new(is_irrefutable),
                guard_expr: Cell::new(guard_expr),
                suite: Cell::new(suite),
            },
        );

        extend_range(&node, &suite);

        set_parent(&pattern, node);
        set_parent(&suite, node);

        if let Some(guard_expr) = guard_expr {
            set_parent(&guard_expr, node);
        }

        CaseNode(node)
    }
}

// parser/parseNodes.ts:2516
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct PatternSequenceNodeDetails {
    pub entries: RefCell<Vec<PatternAsNode>>,
    pub star_entry_index: Cell<Option<i32>>,
}
node_handle!(
    PatternSequenceNode,
    PatternSequence,
    PatternSequenceNodeDetails
);

impl PatternSequenceNode {
    // parser/parseNodes.ts:2524
    pub fn create(
        key: ParseTreeKey,
        first_token: &(impl HasTextRange + ?Sized),
        entries: Vec<PatternAsNode>,
    ) -> PatternSequenceNode {
        let star_entry_index = entries
            .iter()
            .position(|entry| {
                let or_patterns = entry.d().or_patterns.borrow();
                or_patterns.len() == 1
                    && or_patterns[0].node_type == ParseNodeType::PatternCapture
                    && PatternCaptureNode(or_patterns[0]).d().is_star.get()
            })
            .map_or(-1, |i| i as i32);

        let range = first_token.text_range();
        let node = create_node(
            range.start,
            range.length,
            key,
            PatternSequenceNodeDetails {
                entries: RefCell::new(entries.clone()),
                star_entry_index: Cell::new(if star_entry_index >= 0 {
                    Some(star_entry_index)
                } else {
                    None
                }),
            },
        );

        if !entries.is_empty() {
            extend_range(&node, &entries[entries.len() - 1]);
        }

        for entry in &entries {
            set_parent(entry, node);
        }

        PatternSequenceNode(node)
    }
}

// parser/parseNodes.ts:2557
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct PatternAsNodeDetails {
    pub or_patterns: RefCell<Vec<PatternAtomNode>>,
    pub target: Cell<Option<NameNode>>,
}
node_handle!(PatternAsNode, PatternAs, PatternAsNodeDetails);

impl PatternAsNode {
    // parser/parseNodes.ts:2565
    pub fn create(
        key: ParseTreeKey,
        or_patterns: Vec<PatternAtomNode>,
        target: Option<NameNode>,
    ) -> PatternAsNode {
        let node = create_node(
            or_patterns[0].start.get(),
            or_patterns[0].length.get(),
            key,
            PatternAsNodeDetails {
                or_patterns: RefCell::new(or_patterns.clone()),
                target: Cell::new(target),
            },
        );

        if or_patterns.len() > 1 {
            extend_range(&node, &or_patterns[or_patterns.len() - 1]);
        }

        for pattern in &or_patterns {
            set_parent(pattern, node);
        }

        if let Some(target) = target {
            extend_range(&node, &target);
            set_parent(&target, node);
        }

        PatternAsNode(node)
    }
}

// parser/parseNodes.ts:2596
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct PatternLiteralNodeDetails {
    pub expr: Cell<ExpressionNode>,
}
node_handle!(
    PatternLiteralNode,
    PatternLiteral,
    PatternLiteralNodeDetails
);

impl PatternLiteralNode {
    // parser/parseNodes.ts:2603
    pub fn create(key: ParseTreeKey, expr: ExpressionNode) -> PatternLiteralNode {
        let node = create_node(
            expr.start.get(),
            expr.length.get(),
            key,
            PatternLiteralNodeDetails {
                expr: Cell::new(expr),
            },
        );

        set_parent(&expr, node);

        PatternLiteralNode(node)
    }
}

// parser/parseNodes.ts:2620
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct PatternClassNodeDetails {
    /// `NameNode | MemberAccessNode`.
    pub class_name: Cell<P<ParseNode>>,
    pub args: RefCell<Vec<PatternClassArgumentNode>>,
}
node_handle!(PatternClassNode, PatternClass, PatternClassNodeDetails);

impl PatternClassNode {
    // parser/parseNodes.ts:2628
    pub fn create(
        key: ParseTreeKey,
        class_name: P<ParseNode>,
        args: Vec<PatternClassArgumentNode>,
    ) -> PatternClassNode {
        let node = create_node(
            class_name.start.get(),
            class_name.length.get(),
            key,
            PatternClassNodeDetails {
                class_name: Cell::new(class_name),
                args: RefCell::new(args.clone()),
            },
        );

        set_parent(&class_name, node);
        for arg in &args {
            set_parent(arg, node);
        }

        if !args.is_empty() {
            extend_range(&node, &args[args.len() - 1]);
        }

        PatternClassNode(node)
    }
}

// parser/parseNodes.ts:2659
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct PatternClassArgumentNodeDetails {
    pub name: Cell<Option<NameNode>>,
    pub pattern: Cell<PatternAsNode>,
}
node_handle!(
    PatternClassArgumentNode,
    PatternClassArgument,
    PatternClassArgumentNodeDetails
);

impl PatternClassArgumentNode {
    // parser/parseNodes.ts:2667
    pub fn create(
        key: ParseTreeKey,
        pattern: PatternAsNode,
        name: Option<NameNode>,
    ) -> PatternClassArgumentNode {
        let node = create_node(
            pattern.start.get(),
            pattern.length.get(),
            key,
            PatternClassArgumentNodeDetails {
                pattern: Cell::new(pattern),
                name: Cell::new(name),
            },
        );

        set_parent(&pattern, node);

        if let Some(name) = name {
            extend_range(&node, &name);
            set_parent(&name, node);
        }

        PatternClassArgumentNode(node)
    }
}

// parser/parseNodes.ts:2692
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct PatternCaptureNodeDetails {
    pub target: Cell<NameNode>,
    pub is_star: Cell<bool>,
    pub is_wildcard: Cell<bool>,
}
node_handle!(
    PatternCaptureNode,
    PatternCapture,
    PatternCaptureNodeDetails
);

impl PatternCaptureNode {
    // parser/parseNodes.ts:2701
    pub fn create(
        key: ParseTreeKey,
        target: NameNode,
        star_token: Option<&(impl HasTextRange + ?Sized)>,
    ) -> PatternCaptureNode {
        let node = create_node(
            target.start.get(),
            target.length.get(),
            key,
            PatternCaptureNodeDetails {
                target: Cell::new(target),
                is_star: Cell::new(star_token.is_some()),
                is_wildcard: Cell::new(target.d().value() == "_"),
            },
        );

        set_parent(&target, node);

        if let Some(star_token) = star_token {
            extend_range(&node, star_token);
        }

        PatternCaptureNode(node)
    }
}

// parser/parseNodes.ts:2726
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct PatternMappingNodeDetails {
    pub entries: RefCell<Vec<PatternMappingEntryNode>>,
}
node_handle!(
    PatternMappingNode,
    PatternMapping,
    PatternMappingNodeDetails
);

impl PatternMappingNode {
    // parser/parseNodes.ts:2733
    pub fn create(
        key: ParseTreeKey,
        start_token: &(impl HasTextRange + ?Sized),
        entries: Vec<PatternMappingEntryNode>,
    ) -> PatternMappingNode {
        let range = start_token.text_range();
        let node = create_node(
            range.start,
            range.length,
            key,
            PatternMappingNodeDetails {
                entries: RefCell::new(entries.clone()),
            },
        );

        if !entries.is_empty() {
            extend_range(&node, &entries[entries.len() - 1]);
        }

        for entry in &entries {
            set_parent(entry, node);
        }

        PatternMappingNode(node)
    }
}

// PatternMappingKeyEntryNode | PatternMappingExpandEntryNode
// parser/parseNodes.ts:2756
pub type PatternMappingEntryNode = P<ParseNode>;

// parser/parseNodes.ts:2758
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct PatternMappingKeyEntryNodeDetails {
    /// `PatternLiteralNode | PatternValueNode | ErrorNode`.
    pub key_pattern: Cell<P<ParseNode>>,
    /// `PatternAsNode | ErrorNode`.
    pub value_pattern: Cell<P<ParseNode>>,
}
node_handle!(
    PatternMappingKeyEntryNode,
    PatternMappingKeyEntry,
    PatternMappingKeyEntryNodeDetails
);

impl PatternMappingKeyEntryNode {
    // parser/parseNodes.ts:2766
    pub fn create(
        key: ParseTreeKey,
        key_pattern: P<ParseNode>,
        value_pattern: P<ParseNode>,
    ) -> PatternMappingKeyEntryNode {
        let node = create_node(
            key_pattern.start.get(),
            key_pattern.length.get(),
            key,
            PatternMappingKeyEntryNodeDetails {
                key_pattern: Cell::new(key_pattern),
                value_pattern: Cell::new(value_pattern),
            },
        );

        set_parent(&key_pattern, node);
        set_parent(&value_pattern, node);

        extend_range(&node, &value_pattern);

        PatternMappingKeyEntryNode(node)
    }
}

// parser/parseNodes.ts:2793
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct PatternMappingExpandEntryNodeDetails {
    pub target: Cell<NameNode>,
}
node_handle!(
    PatternMappingExpandEntryNode,
    PatternMappingExpandEntry,
    PatternMappingExpandEntryNodeDetails
);

impl PatternMappingExpandEntryNode {
    // parser/parseNodes.ts:2800
    pub fn create(
        key: ParseTreeKey,
        star_star_token: &(impl HasTextRange + ?Sized),
        target: NameNode,
    ) -> PatternMappingExpandEntryNode {
        let range = star_star_token.text_range();
        let node = create_node(
            range.start,
            range.length,
            key,
            PatternMappingExpandEntryNodeDetails {
                target: Cell::new(target),
            },
        );

        set_parent(&target, node);

        extend_range(&node, &target);

        PatternMappingExpandEntryNode(node)
    }
}

// parser/parseNodes.ts:2819
#[derive(Clone)]
#[derive(jsrt::Trace)]
pub struct PatternValueNodeDetails {
    pub expr: Cell<MemberAccessNode>,
}
node_handle!(PatternValueNode, PatternValue, PatternValueNodeDetails);

impl PatternValueNode {
    // parser/parseNodes.ts:2826
    pub fn create(key: ParseTreeKey, expr: MemberAccessNode) -> PatternValueNode {
        let node = create_node(
            expr.start.get(),
            expr.length.get(),
            key,
            PatternValueNodeDetails {
                expr: Cell::new(expr),
            },
        );

        set_parent(&expr, node);

        PatternValueNode(node)
    }
}

// PatternSequenceNode | PatternLiteralNode | PatternClassNode | PatternAsNode
// | PatternCaptureNode | PatternMappingNode | PatternValueNode | ErrorNode
// parser/parseNodes.ts:2843
pub type PatternAtomNode = P<ParseNode>;

// LambdaNode | FunctionNode | ModuleNode | ClassNode | ComprehensionNode | TypeParameterListNode
// parser/parseNodes.ts:2933
pub type EvaluationScopeNode = P<ParseNode>;

// LambdaNode | FunctionNode | ModuleNode | TypeParameterListNode
// parser/parseNodes.ts:2940
pub type ExecutionScopeNode = P<ParseNode>;

// FunctionNode | ClassNode | TypeAliasNode
// parser/parseNodes.ts:2941
pub type TypeParameterScopeNode = P<ParseNode>;
