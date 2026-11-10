# stubgen run `binder`

Stubs (copy candidates for `crates/pyright/src`):

- `stubs/analyzer/binder.rs` from `analyzer/binder.ts`
- `stubs/analyzer/scope.rs` from `analyzer/scope.ts`
- `stubs/analyzer/symbol.rs` from `analyzer/symbol.ts`
- `stubs/analyzer/declaration.rs` from `analyzer/declaration.ts`
- `stubs/analyzer/code_flow_types.rs` from `analyzer/codeFlowTypes.ts`
- `stubs/analyzer/analyzer_node_info.rs` from `analyzer/analyzerNodeInfo.ts`
- `stubs/analyzer/parse_tree_walker.rs` from `analyzer/parseTreeWalker.ts`
- `stubs/analyzer/parse_tree_utils.rs` from `analyzer/parseTreeUtils.ts`
- `stubs/analyzer/static_expressions.rs` from `analyzer/staticExpressions.ts`

Coverage: markers for every TypeScript function, method and type of each stubbed file, and methods for every member of each factory interface:

- `analyzer/binder.ts`: 132/132 declarations have a marker
- `analyzer/scope.ts`: 19/19 declarations have a marker
- `analyzer/symbol.ts`: 42/42 declarations have a marker
- `analyzer/declaration.ts`: 24/24 declarations have a marker
- `analyzer/codeFlowTypes.ts`: 19/19 declarations have a marker
- `analyzer/analyzerNodeInfo.ts`: 98/98 declarations have a marker
- `analyzer/parseTreeWalker.ts`: 87/87 declarations have a marker
- `analyzer/parseTreeUtils.ts`: 106/106 declarations have a marker
- `analyzer/staticExpressions.ts`: 17/17 declarations have a marker

Dependency placeholders (compile check only):

- `deps/analyzer/analyzer_file_info.rs`: AnalyzerFileInfo
- `deps/analyzer/cell_chain_index.rs`: CellChainIndexProvider
- `deps/analyzer/import_result.rs`: ImportResult
- `deps/analyzer/types.rs`: TypeBase
- `deps/common/config_options.rs`: ExecutionEnvironment
- `deps/common/diagnostic.rs`: Diagnostic
- `deps/common/diagnostic_rules.rs`: DiagnosticRule
- `deps/common/python_version.rs`: PythonVersion
- `deps/common/text_range.rs`: Position, Range, TextRange
- `deps/common/text_range_collection.rs`: TextRangeCollection
- `deps/common/uri/uri.rs`: Uri
- `deps/parser/parse_nodes.rs`: ArgumentNode, AssertNode, AssignmentExpressionNode, AssignmentNode, AugmentedAssignmentNode, AwaitNode, BinaryOperationNode, BreakNode, CallNode, CaseNode, ClassNode, ComprehensionForNode, ComprehensionIfNode, ComprehensionNode, ConstantNode, ContinueNode, DecoratorNode, DelNode, DictionaryEntryNode, DictionaryExpandEntryNode, DictionaryKeyEntryNode, DictionaryNode, EllipsisNode, ErrorNode, EvaluationScopeNode, ExceptNode, ExecutionScopeNode, ExpressionNode, ForNode, FormatStringNode, FunctionAnnotationNode, FunctionNode, GlobalNode, IfNode, ImportAsNode, ImportFromAsNode, ImportFromNode, ImportNode, IndexNode, LambdaNode, ListNode, MatchNode, MemberAccessNode, ModuleNameNode, ModuleNode, NameNode, NonlocalNode, NumberNode, ParameterNode, ParseNodeBase, ParseNodeType, PassNode, PatternAsNode, PatternCaptureNode, PatternClassArgumentNode, PatternClassNode, PatternLiteralNode, PatternMappingExpandEntryNode, PatternMappingKeyEntryNode, PatternMappingNode, PatternSequenceNode, PatternValueNode, RaiseNode, ReturnNode, SetNode, SliceNode, StatementListNode, StatementNode, StringListNode, StringNode, SuiteNode, TernaryNode, TryNode, TupleNode, TypeAliasNode, TypeAnnotationNode, TypeParameterListNode, TypeParameterNode, TypeParameterScopeNode, UnaryOperationNode, UnpackNode, WhileNode, WithItemNode, WithNode, YieldFromNode, YieldNode
- `deps/parser/parser.rs`: ParseFileResults
- `deps/parser/string_annotation_info.rs`: StringAnnotationInfo
- `deps/parser/tokenizer.rs`: TokenizerOutput
- `deps/parser/tokenizer_types.rs`: Comment, OperatorType, StringToken, TokenBase

Number sites without a reviewed type (numbers.toml, values ending in "?"): 0 in the stubbed files, 0 in placeholders.

Unmapped TypeScript types (rendered with a `/*? ... */` marker): 5 distinct.

- `boolean | string` x3: analyzer/staticExpressions.ts:evaluateStaticBoolExpression.definedConstants, analyzer/staticExpressions.ts:evaluateStaticBoolLikeExpression.definedConstants, analyzer/staticExpressions.ts:_evaluateStaticBoolOrBoolLikeExpression.definedConstants
- `T extends ParseNode & { nodeType: NT } ? T : never` x2: analyzer/parseTreeUtils.ts:NodeForType
- `any` x2: analyzer/staticExpressions.ts:_evaluateVersionBinaryOperation.return, analyzer/staticExpressions.ts:_evaluateStringBinaryOperation.return
- `AnalyzerNodeInfoReaderPropertyProvider | AnalyzerNodeInfoReaderMethodProvider` x1: analyzer/analyzerNodeInfo.ts:getInfoReader.provider
- `unique symbol` x1: analyzer/analyzerNodeInfo.ts:_retiredStores

How each class and interface of the stubbed files is held (docs/porting.md "memory"):

- `analyzer/binder.ts:MemberAccessInfo`: value; written: classScope at analyzer/binder.ts:4036; never stored in a second place
- `analyzer/binder.ts:DeferredBindingTask`: owned; [modes]
- `analyzer/binder.ts:FinalInfo`: value; never written after construction; never stored in a second place
- `analyzer/binder.ts:ClassVarInfo`: value; never written after construction; never stored in a second place
- `analyzer/binder.ts:NarrowExprOptions`: value; never written after construction; never stored in a second place
- `analyzer/binder.ts:Binder`: owned; written: _deferredBindingTasks at analyzer/binder.ts:4753; never stored in a second place
- `analyzer/binder.ts:YieldFinder`: owned; written: _containsYield at analyzer/binder.ts:4844; never stored in a second place
- `analyzer/binder.ts:ReturnFinder`: owned; written: _containsReturn at analyzer/binder.ts:4863; never stored in a second place
- `analyzer/binder.ts:DummyScopeGenerator`: owned; written: _currentScope at analyzer/binder.ts:4911; never stored in a second place
- `analyzer/scope.ts:SymbolWithScope`: value; written: symbol at analyzer/binder.ts:4024; never stored in a second place
- `analyzer/scope.ts:GlobalScopeResult`: value; written: scope at analyzer/binder.ts:1918; never stored in a second place
- `analyzer/scope.ts:LookupSymbolOptions`: value; never written after construction; never stored in a second place
- `analyzer/scope.ts:ChainedModuleLevelLookupContext`: value; never written after construction; never stored in a second place
- `analyzer/scope.ts:Scope`: handle; compared or hashed by identity at analyzer/scopeUtils.ts:53; written: symbolTable at analyzer/scope.ts:257; stored at analyzer/scope.ts:140
- `analyzer/symbol.ts:SynthesizedTypeInfo`: value; never written after construction; stored at analyzer/typeEvaluator.ts:23617
- `analyzer/symbol.ts:Symbol`: handle; compared or hashed by identity at analyzer/binder.ts:194 (Map key or Set element); written: _declarations at analyzer/symbol.ts:291; stored at analyzer/scope.ts:200
- `analyzer/declaration.ts:ModuleLoaderActions`: handle; written: uri at analyzer/binder.ts:3019; stored at analyzer/binder.ts:3012
- `analyzer/analyzerNodeInfo.ts:DunderAllInfo`: value; never written after construction; stored at analyzer/analyzerNodeInfo.ts:703
- `analyzer/analyzerNodeInfo.ts:AnalyzerNodeInfo`: handle; written: importInfo at analyzer/analyzerNodeInfo.ts:554; stored at analyzer/analyzerNodeInfo.ts:134
- `analyzer/analyzerNodeInfo.ts:AnalyzerNodeInfoReaderPropertyProvider`: value; never written after construction; never stored in a second place
- `analyzer/analyzerNodeInfo.ts:AnalyzerNodeInfoReaderMethodProvider`: value; never written after construction; never stored in a second place
- `analyzer/analyzerNodeInfo.ts:AnalyzerNodeInfoStore`: handle; written: _infoByTree at analyzer/analyzerNodeInfo.ts:128; stored at analyzer/analyzerNodeInfo.ts:315
- `analyzer/analyzerNodeInfo.ts:AnalyzerNodeInfoContextLayer`: handle; written: stores at analyzer/analyzerNodeInfo.ts:315; stored at analyzer/analyzerNodeInfo.ts:230
- `analyzer/analyzerNodeInfo.ts:RetiredAnalyzerNodeInfoHost`: value; never written after construction; never stored in a second place
- `analyzer/analyzerNodeInfo.ts:AnalyzerNodeInfoBindingSessionImpl`: owned; written: _store at analyzer/analyzerNodeInfo.ts:197; never stored in a second place
- `analyzer/analyzerNodeInfo.ts:AnalyzerNodeInfoContextImpl`: handle; compared or hashed by identity at analyzer/analyzerNodeInfo.ts:206; written: _layers at analyzer/analyzerNodeInfo.ts:373; stored at analyzer/program.ts:1434
- `analyzer/analyzerNodeInfo.ts:AnalyzerNodeInfoAccessor`: rc; never written after construction; stored at analyzer/binder.ts:372
- `analyzer/parseTreeUtils.ts:EvaluationScopeInfo`: value; never written after construction; never stored in a second place
- `analyzer/parseTreeUtils.ts:NameNodeWalker`: owned; written: _subscriptIndex at analyzer/parseTreeUtils.ts:1733; never stored in a second place
- `analyzer/parseTreeUtils.ts:CallNodeWalker`: owned; never written after construction; never stored in a second place

[modes] values that some code compares by identity (porting.md wants a handle): 0.

