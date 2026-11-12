//! Port of `packages/pyright-internal/src/analyzer/parseTreeWalker.ts`.
//!
//! `ParseTreeVisitor<T>` and `ParseTreeWalker` are traits: a TypeScript
//! subclass becomes a type that implements them and overrides the
//! `visit_*` methods it needs. The constructor's `_default` is
//! `default_visit_result`.

use jsrt::P;

use super::analyzer_node_info::{self, AnalyzerNodeInfoReader};
use crate::parser::parse_nodes::{
    ArgumentNode, AssertNode, AssignmentExpressionNode, AssignmentNode, AugmentedAssignmentNode, AwaitNode, BinaryOperationNode, BreakNode, CallNode, CaseNode, ClassNode, ComprehensionForNode, ComprehensionIfNode, ComprehensionNode, ConstantNode, ContinueNode, DecoratorNode, DelNode, DictionaryExpandEntryNode, DictionaryKeyEntryNode, DictionaryNode, EllipsisNode, ErrorNode, ExceptNode, ForNode, FormatStringNode, FunctionAnnotationNode, FunctionNode, GlobalNode, IfNode, ImportAsNode, ImportFromAsNode, ImportFromNode, ImportNode, IndexNode, LambdaNode, ListNode, MatchNode, MemberAccessNode, ModuleNameNode, ModuleNode, NameNode, NonlocalNode, NumberNode, ParameterNode, PassNode, PatternAsNode, PatternCaptureNode, PatternClassArgumentNode, PatternClassNode, PatternLiteralNode, PatternMappingExpandEntryNode, PatternMappingKeyEntryNode, PatternMappingNode, PatternSequenceNode, PatternValueNode, RaiseNode, ReturnNode, SetNode, SliceNode, StatementListNode, StringListNode, StringNode, SuiteNode, TernaryNode, TryNode, TupleNode, TypeAliasNode, TypeAnnotationNode, TypeParameterListNode, TypeParameterNode, UnaryOperationNode, UnpackNode, WhileNode, WithItemNode, WithNode, YieldFromNode, YieldNode,
    ExpressionNode, ParseNode, ParseNodeType, get_parser_string_annotation,
};
pub use crate::parser::parse_tree_utils::{ChildNodes, get_child_nodes};

// analyzer/parseTreeWalker.ts:102
// To use this class, create a subclass and override the
// visitXXX methods that you want to handle.
pub trait ParseTreeVisitor<T> {
    /// The `_default` passed to the constructor.
    fn default_visit_result(&self) -> T;

    // analyzer/parseTreeWalker.ts:107
    fn visit(&mut self, node: P<ParseNode>) -> T {
        match node.node_type {
            ParseNodeType::Error => self.visit_error(ErrorNode(node)),
            ParseNodeType::Argument => self.visit_argument(ArgumentNode(node)),
            ParseNodeType::Assert => self.visit_assert(AssertNode(node)),
            ParseNodeType::AssignmentExpression => self.visit_assignment_expression(AssignmentExpressionNode(node)),
            ParseNodeType::Assignment => self.visit_assignment(AssignmentNode(node)),
            ParseNodeType::AugmentedAssignment => self.visit_augmented_assignment(AugmentedAssignmentNode(node)),
            ParseNodeType::Await => self.visit_await(AwaitNode(node)),
            ParseNodeType::BinaryOperation => self.visit_binary_operation(BinaryOperationNode(node)),
            ParseNodeType::Break => self.visit_break(BreakNode(node)),
            ParseNodeType::Call => self.visit_call(CallNode(node)),
            ParseNodeType::Case => self.visit_case(CaseNode(node)),
            ParseNodeType::Class => self.visit_class(ClassNode(node)),
            ParseNodeType::Comprehension => self.visit_comprehension(ComprehensionNode(node)),
            ParseNodeType::ComprehensionFor => self.visit_comprehension_for(ComprehensionForNode(node)),
            ParseNodeType::ComprehensionIf => self.visit_comprehension_if(ComprehensionIfNode(node)),
            ParseNodeType::Constant => self.visit_constant(ConstantNode(node)),
            ParseNodeType::Continue => self.visit_continue(ContinueNode(node)),
            ParseNodeType::Decorator => self.visit_decorator(DecoratorNode(node)),
            ParseNodeType::Del => self.visit_del(DelNode(node)),
            ParseNodeType::Dictionary => self.visit_dictionary(DictionaryNode(node)),
            ParseNodeType::DictionaryExpandEntry => self.visit_dictionary_expand_entry(DictionaryExpandEntryNode(node)),
            ParseNodeType::DictionaryKeyEntry => self.visit_dictionary_key_entry(DictionaryKeyEntryNode(node)),
            ParseNodeType::Ellipsis => self.visit_ellipsis(EllipsisNode(node)),
            ParseNodeType::If => self.visit_if(IfNode(node)),
            ParseNodeType::Import => self.visit_import(ImportNode(node)),
            ParseNodeType::ImportAs => self.visit_import_as(ImportAsNode(node)),
            ParseNodeType::ImportFrom => self.visit_import_from(ImportFromNode(node)),
            ParseNodeType::ImportFromAs => self.visit_import_from_as(ImportFromAsNode(node)),
            ParseNodeType::Index => self.visit_index(IndexNode(node)),
            ParseNodeType::Except => self.visit_except(ExceptNode(node)),
            ParseNodeType::For => self.visit_for(ForNode(node)),
            ParseNodeType::FormatString => self.visit_format_string(FormatStringNode(node)),
            ParseNodeType::Function => self.visit_function(FunctionNode(node)),
            ParseNodeType::FunctionAnnotation => self.visit_function_annotation(FunctionAnnotationNode(node)),
            ParseNodeType::Global => self.visit_global(GlobalNode(node)),
            ParseNodeType::Lambda => self.visit_lambda(LambdaNode(node)),
            ParseNodeType::List => self.visit_list(ListNode(node)),
            ParseNodeType::Match => self.visit_match(MatchNode(node)),
            ParseNodeType::MemberAccess => self.visit_member_access(MemberAccessNode(node)),
            ParseNodeType::ModuleName => self.visit_module_name(ModuleNameNode(node)),
            ParseNodeType::Module => self.visit_module(ModuleNode(node)),
            ParseNodeType::Name => self.visit_name(NameNode(node)),
            ParseNodeType::Nonlocal => self.visit_nonlocal(NonlocalNode(node)),
            ParseNodeType::Number => self.visit_number(NumberNode(node)),
            ParseNodeType::Parameter => self.visit_parameter(ParameterNode(node)),
            ParseNodeType::Pass => self.visit_pass(PassNode(node)),
            ParseNodeType::PatternAs => self.visit_pattern_as(PatternAsNode(node)),
            ParseNodeType::PatternClass => self.visit_pattern_class(PatternClassNode(node)),
            ParseNodeType::PatternClassArgument => self.visit_pattern_class_argument(PatternClassArgumentNode(node)),
            ParseNodeType::PatternCapture => self.visit_pattern_capture(PatternCaptureNode(node)),
            ParseNodeType::PatternLiteral => self.visit_pattern_literal(PatternLiteralNode(node)),
            ParseNodeType::PatternMappingExpandEntry => self.visit_pattern_mapping_expand_entry(PatternMappingExpandEntryNode(node)),
            ParseNodeType::PatternMappingKeyEntry => self.visit_pattern_mapping_key_entry(PatternMappingKeyEntryNode(node)),
            ParseNodeType::PatternMapping => self.visit_pattern_mapping(PatternMappingNode(node)),
            ParseNodeType::PatternSequence => self.visit_pattern_sequence(PatternSequenceNode(node)),
            ParseNodeType::PatternValue => self.visit_pattern_value(PatternValueNode(node)),
            ParseNodeType::Raise => self.visit_raise(RaiseNode(node)),
            ParseNodeType::Return => self.visit_return(ReturnNode(node)),
            ParseNodeType::Set => self.visit_set(SetNode(node)),
            ParseNodeType::Slice => self.visit_slice(SliceNode(node)),
            ParseNodeType::StatementList => self.visit_statement_list(StatementListNode(node)),
            ParseNodeType::StringList => self.visit_string_list(StringListNode(node)),
            ParseNodeType::String => self.visit_string(StringNode(node)),
            ParseNodeType::Suite => self.visit_suite(SuiteNode(node)),
            ParseNodeType::Ternary => self.visit_ternary(TernaryNode(node)),
            ParseNodeType::Tuple => self.visit_tuple(TupleNode(node)),
            ParseNodeType::Try => self.visit_try(TryNode(node)),
            ParseNodeType::TypeAlias => self.visit_type_alias(TypeAliasNode(node)),
            ParseNodeType::TypeAnnotation => self.visit_type_annotation(TypeAnnotationNode(node)),
            ParseNodeType::TypeParameter => self.visit_type_parameter(TypeParameterNode(node)),
            ParseNodeType::TypeParameterList => self.visit_type_parameter_list(TypeParameterListNode(node)),
            ParseNodeType::UnaryOperation => self.visit_unary_operation(UnaryOperationNode(node)),
            ParseNodeType::Unpack => self.visit_unpack(UnpackNode(node)),
            ParseNodeType::While => self.visit_while(WhileNode(node)),
            ParseNodeType::With => self.visit_with(WithNode(node)),
            ParseNodeType::WithItem => self.visit_with_item(WithItemNode(node)),
            ParseNodeType::Yield => self.visit_yield(YieldNode(node)),
            ParseNodeType::YieldFrom => self.visit_yield_from(YieldFromNode(node)),
        }
    }

    // Override these methods as necessary.
    // analyzer/parseTreeWalker.ts:349
    fn visit_argument(&mut self, node: ArgumentNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:353
    fn visit_assert(&mut self, node: AssertNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:357
    fn visit_assignment(&mut self, node: AssignmentNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:361
    fn visit_assignment_expression(&mut self, node: AssignmentExpressionNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:365
    fn visit_augmented_assignment(&mut self, node: AugmentedAssignmentNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:369
    fn visit_await(&mut self, node: AwaitNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:373
    fn visit_binary_operation(&mut self, node: BinaryOperationNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:377
    fn visit_break(&mut self, node: BreakNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:381
    fn visit_call(&mut self, node: CallNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:385
    fn visit_case(&mut self, node: CaseNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:389
    fn visit_class(&mut self, node: ClassNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:393
    fn visit_comprehension(&mut self, node: ComprehensionNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:397
    fn visit_comprehension_for(&mut self, node: ComprehensionForNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:401
    fn visit_comprehension_if(&mut self, node: ComprehensionIfNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:405
    fn visit_continue(&mut self, node: ContinueNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:409
    fn visit_constant(&mut self, node: ConstantNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:413
    fn visit_decorator(&mut self, node: DecoratorNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:417
    fn visit_del(&mut self, node: DelNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:421
    fn visit_dictionary(&mut self, node: DictionaryNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:425
    fn visit_dictionary_key_entry(&mut self, node: DictionaryKeyEntryNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:429
    fn visit_dictionary_expand_entry(&mut self, node: DictionaryExpandEntryNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:433
    fn visit_error(&mut self, node: ErrorNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:437
    fn visit_ellipsis(&mut self, node: EllipsisNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:441
    fn visit_if(&mut self, node: IfNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:445
    fn visit_import(&mut self, node: ImportNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:449
    fn visit_import_as(&mut self, node: ImportAsNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:453
    fn visit_import_from(&mut self, node: ImportFromNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:457
    fn visit_import_from_as(&mut self, node: ImportFromAsNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:461
    fn visit_index(&mut self, node: IndexNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:465
    fn visit_except(&mut self, node: ExceptNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:469
    fn visit_for(&mut self, node: ForNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:473
    fn visit_format_string(&mut self, node: FormatStringNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:477
    fn visit_function(&mut self, node: FunctionNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:481
    fn visit_function_annotation(&mut self, node: FunctionAnnotationNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:485
    fn visit_global(&mut self, node: GlobalNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:489
    fn visit_lambda(&mut self, node: LambdaNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:493
    fn visit_list(&mut self, node: ListNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:497
    fn visit_match(&mut self, node: MatchNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:501
    fn visit_member_access(&mut self, node: MemberAccessNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:505
    fn visit_module(&mut self, node: ModuleNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:509
    fn visit_module_name(&mut self, node: ModuleNameNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:513
    fn visit_name(&mut self, node: NameNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:517
    fn visit_nonlocal(&mut self, node: NonlocalNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:521
    fn visit_number(&mut self, node: NumberNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:525
    fn visit_parameter(&mut self, node: ParameterNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:529
    fn visit_pass(&mut self, node: PassNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:533
    fn visit_pattern_capture(&mut self, node: PatternCaptureNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:537
    fn visit_pattern_class(&mut self, node: PatternClassNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:541
    fn visit_pattern_class_argument(&mut self, node: PatternClassArgumentNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:545
    fn visit_pattern_as(&mut self, node: PatternAsNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:549
    fn visit_pattern_literal(&mut self, node: PatternLiteralNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:553
    fn visit_pattern_mapping(&mut self, node: PatternMappingNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:557
    fn visit_pattern_mapping_expand_entry(&mut self, node: PatternMappingExpandEntryNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:561
    fn visit_pattern_mapping_key_entry(&mut self, node: PatternMappingKeyEntryNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:565
    fn visit_pattern_sequence(&mut self, node: PatternSequenceNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:569
    fn visit_pattern_value(&mut self, node: PatternValueNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:573
    fn visit_raise(&mut self, node: RaiseNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:577
    fn visit_return(&mut self, node: ReturnNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:581
    fn visit_set(&mut self, node: SetNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:585
    fn visit_slice(&mut self, node: SliceNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:589
    fn visit_statement_list(&mut self, node: StatementListNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:593
    fn visit_string(&mut self, node: StringNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:597
    fn visit_string_list(&mut self, node: StringListNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:601
    fn visit_suite(&mut self, node: SuiteNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:605
    fn visit_ternary(&mut self, node: TernaryNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:609
    fn visit_tuple(&mut self, node: TupleNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:613
    fn visit_try(&mut self, node: TryNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:617
    fn visit_type_alias(&mut self, node: TypeAliasNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:621
    fn visit_type_annotation(&mut self, node: TypeAnnotationNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:625
    fn visit_type_parameter(&mut self, node: TypeParameterNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:629
    fn visit_type_parameter_list(&mut self, node: TypeParameterListNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:633
    fn visit_unary_operation(&mut self, node: UnaryOperationNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:637
    fn visit_unpack(&mut self, node: UnpackNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:641
    fn visit_while(&mut self, node: WhileNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:645
    fn visit_with(&mut self, node: WithNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:649
    fn visit_with_item(&mut self, node: WithItemNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:653
    fn visit_yield(&mut self, node: YieldNode) -> T {
        let _ = node;
        self.default_visit_result()
    }

    // analyzer/parseTreeWalker.ts:657
    fn visit_yield_from(&mut self, node: YieldFromNode) -> T {
        let _ = node;
        self.default_visit_result()
    }
}

// analyzer/parseTreeWalker.ts:664
// To use this class, create a subclass and override the
// visitXXX methods that you want to handle.
pub trait ParseTreeWalker: ParseTreeVisitor<bool> {
    /// The `_annotationNodeInfo` passed to the constructor.
    fn annotation_node_info(&self) -> Option<P<dyn AnalyzerNodeInfoReader>> {
        None
    }

    // analyzer/parseTreeWalker.ts:675
    fn walk(&mut self, node: P<ParseNode>) {
        let children_to_walk = self.visit_node(node);
        if !children_to_walk.is_empty() {
            self.walk_multiple(&children_to_walk);
        }
    }

    // analyzer/parseTreeWalker.ts:682
    fn walk_multiple(&mut self, nodes: &[Option<P<ParseNode>>]) {
        for node in nodes.iter().flatten() {
            self.walk(*node);
        }
    }

    // analyzer/parseTreeWalker.ts:693
    // If this.visit(node) returns true, all child nodes for the node are returned.
    // If the method returns false, we assume that the handler has already handled the
    // child nodes, so an empty list is returned.
    fn visit_node(&mut self, node: P<ParseNode>) -> ChildNodes {
        if self.visit(node) {
            get_child_nodes(node, Some(&|n| self.get_string_annotation(n)))
        } else {
            ChildNodes::default()
        }
    }

    // Resolves the effective string annotation used to expand child nodes during traversal.
    //
    // When an `AnalyzerNodeInfoReader` was supplied (Program-aware walkers that must observe
    // evaluator-discovered annotations, such as the checker and the semantic-token/rename/
    // references walkers), the combined accessor is used so both tier-1 parser annotations and
    // tier-2 evaluator-discovered annotations (e.g. `cast("Data", v)`) are visited. When no reader
    // was supplied, the walker is deliberately syntax-only and sees tier-1 parser annotations
    // exclusively. The binder intentionally stays syntax-only: tier-2 annotations do not exist yet
    // at bind time, so tier-1 descent is both sufficient and correct there.
    // analyzer/parseTreeWalker.ts:712
    fn get_string_annotation(&self, node: StringListNode) -> Option<ExpressionNode> {
        match self.annotation_node_info() {
            Some(reader) => analyzer_node_info::get_string_annotation(node, reader.get()),
            None => get_parser_string_annotation(node),
        }
    }
}
