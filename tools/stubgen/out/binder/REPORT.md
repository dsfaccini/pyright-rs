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

- `deps/analyzer/analyzer_file_info.rs`: AbsoluteModuleDescriptor, AnalyzerFileInfo, ImportLookup, ImportLookupResult, LookupImportOptions
- `deps/analyzer/cell_chain_index.rs`: CellChainIndexProvider
- `deps/analyzer/import_result.rs`: ImplicitImport, ImportResult, ImportType
- `deps/analyzer/py_typed_utils.rs`: PyTypedInfo
- `deps/analyzer/source_file.rs`: IPythonMode
- `deps/analyzer/types.rs`: AnyDetailsPriv, CachedTypeInfo, CallSiteInferenceTypeCacheEntry, ClassDetailsPriv, ClassDetailsShared, ClassType, ClassTypeFlags, DataClassBehaviors, DataClassEntry, EnumLiteral, FunctionDetailsPriv, FunctionDetailsShared, FunctionDetailsSharedInferredReturnType, FunctionParam, FunctionParamFlags, FunctionType, FunctionTypeFlags, LiteralTypes, LiteralValue, ModuleDetailsPriv, NeverDetailsPriv, OverloadedDetailsPriv, OverloadedType, ParamSpecAccess, PropertyMethodInfo, SentinelLiteral, SpecializedFunctionTypes, TupleTypeArg, TypeAliasInfo, TypeAliasSharedInfo, TypeBase, TypeBaseProps, TypeCategory, TypeCondition, TypeFlags, TypeVarDetailsPriv, TypeVarDetailsShared, TypeVarKind, TypeVarScopeType, TypeVarType, TypedDictEntries, TypedDictEntry, UnionDetailsPriv, UnionType, UnionableType, UnknownDetailsPriv, Variance
- `deps/common/config_options.rs`: DiagnosticLevel, DiagnosticRuleSet, ExecutionEnvironment
- `deps/common/diagnostic.rs`: Diagnostic, DiagnosticAction, DiagnosticCategory, DiagnosticRelatedInfo, TaskListPriority
- `deps/common/diagnostic_rules.rs`: DiagnosticRule
- `deps/common/diagnostic_sink.rs`: TextRangeDiagnosticSink
- `deps/common/python_version.rs`: PythonReleaseLevel, PythonVersion
- `deps/common/string_utils.rs`: StringFingerprint
- `deps/common/text_range.rs`: Position, Range, TextRange
- `deps/common/text_range_collection.rs`: TextRangeCollection
- `deps/common/uri/uri.rs`: Uri
- `deps/parser/parse_nodes.rs`: ArgCategory, ArgumentNode, AssertNode, AssignmentExpressionNode, AssignmentNode, AugmentedAssignmentNode, AwaitNode, BinaryOperationNode, BreakNode, CallNode, CaseNode, ClassNode, ComprehensionForIfNode, ComprehensionForNode, ComprehensionIfNode, ComprehensionNode, ConstantNode, ContinueNode, DecoratorNode, DelNode, DictionaryEntryNode, DictionaryExpandEntryNode, DictionaryKeyEntryNode, DictionaryNode, EllipsisNode, ErrorExpressionCategory, ErrorNode, EvaluationScopeNode, ExceptNode, ExecutionScopeNode, ExpressionNode, ForNode, FormatStringNode, FunctionAnnotationNode, FunctionNode, GlobalNode, IfNode, ImportAsNode, ImportFromAsNode, ImportFromNode, ImportNode, IndexNode, LambdaNode, ListNode, MatchNode, MemberAccessNode, ModuleNameNode, ModuleNode, NameNode, NonlocalNode, NumberNode, ParamCategory, ParameterNode, ParseNodeBase, ParseNodeType, PassNode, PatternAsNode, PatternAtomNode, PatternCaptureNode, PatternClassArgumentNode, PatternClassNode, PatternLiteralNode, PatternMappingEntryNode, PatternMappingExpandEntryNode, PatternMappingKeyEntryNode, PatternMappingNode, PatternSequenceNode, PatternValueNode, RaiseNode, ReturnNode, SetNode, SliceNode, StatementListNode, StatementNode, StringListNode, StringNode, SuiteNode, TernaryNode, TryNode, TupleNode, TypeAliasNode, TypeAnnotationNode, TypeParamKind, TypeParameterListNode, TypeParameterNode, TypeParameterScopeNode, UnaryOperationNode, UnpackNode, WhileNode, WithItemNode, WithNode, YieldFromNode, YieldNode
- `deps/parser/parser.rs`: ModuleImport, ParseFileResults, ParserOutput
- `deps/parser/string_annotation_info.rs`: StringAnnotationInfo
- `deps/parser/tokenizer.rs`: IgnoreComment, IgnoreCommentRule, TokenizerOutput
- `deps/parser/tokenizer_types.rs`: Comment, CommentType, FStringMiddleToken, FStringStartToken, IdentifierToken, KeywordToken, KeywordType, NewLineType, OperatorType, StringToken, StringTokenFlags, TokenBase, TokenType

Number sites without a reviewed type (numbers.toml, values ending in "?"): 0 in the stubbed files, 31 in placeholders.

Unmapped TypeScript types (rendered with a `/*? ... */` marker): 8 distinct.

- `boolean | string` x7: analyzer/staticExpressions.ts:evaluateStaticBoolExpression.definedConstants, analyzer/staticExpressions.ts:evaluateStaticBoolLikeExpression.definedConstants, analyzer/staticExpressions.ts:_evaluateStaticBoolOrBoolLikeExpression.definedConstants, analyzer/analyzerFileInfo.ts:AnalyzerFileInfo.definedConstants, ...
- `any` x6: analyzer/staticExpressions.ts:_evaluateVersionBinaryOperation.return, analyzer/staticExpressions.ts:_evaluateStringBinaryOperation.return, common/diagnostic.ts:Diagnostic._data, ...
- `Uri | AbsoluteModuleDescriptor` x3: analyzer/analyzerFileInfo.ts:ImportLookup
- `object` x3: analyzer/types.ts:ClassDetailsShared.protocolCompatibility
- `T extends ParseNode & { nodeType: NT } ? T : never` x2: analyzer/parseTreeUtils.ts:NodeForType
- `AnalyzerNodeInfoReaderPropertyProvider | AnalyzerNodeInfoReaderMethodProvider` x1: analyzer/analyzerNodeInfo.ts:getInfoReader.provider
- `unique symbol` x1: analyzer/analyzerNodeInfo.ts:_retiredStores
- `unknown` x1: analyzer/parseTreeUtils.ts:getAncestorsIncludingSelf.return

Interfaces held as `P<T>` by the default rule (a field is written after construction); set [modes] to override:

- `analyzer/analyzerFileInfo.ts:AnalyzerFileInfo`: accessedSymbolSet at analyzer/typeEvaluator.ts:5123
- `analyzer/analyzerNodeInfo.ts:AnalyzerNodeInfo`: importInfo at analyzer/analyzerNodeInfo.ts:554
- `analyzer/analyzerNodeInfo.ts:AnalyzerNodeInfoContextLayer`: stores at analyzer/analyzerNodeInfo.ts:315
- `analyzer/declaration.ts:ModuleLoaderActions`: uri at analyzer/binder.ts:3019
- `analyzer/importResult.ts:ImplicitImport`: isNativeLib at analyzer/importResolver.ts:812
- `analyzer/importResult.ts:ImportResult`: isRelative at analyzer/importResolver.ts:925
- `analyzer/types.ts:ClassDetailsShared`: flags at analyzer/types.ts:1063
- `analyzer/types.ts:DataClassBehaviors`: skipGenerateInit at analyzer/dataClasses.ts:1643
- `analyzer/types.ts:DataClassEntry`: hasDefault at analyzer/dataClasses.ts:591
- `analyzer/types.ts:FunctionDetailsShared`: flags at analyzer/types.ts:2002
- `analyzer/types.ts:LiteralTypes`: literalStrMap at analyzer/types.ts:2734
- `analyzer/types.ts:SpecializedFunctionTypes`: parameterTypes at analyzer/types.ts:2143
- `analyzer/types.ts:TypeAliasSharedInfo`: typeParams at analyzer/typeEvaluator.ts:18021
- `analyzer/types.ts:TypeVarDetailsShared`: name at analyzer/types.ts:2998
- `analyzer/types.ts:TypedDictEntries`: knownItems at analyzer/typedDicts.ts:1138
- `common/configOptions.ts:DiagnosticRuleSet`: analyzeUnannotatedFunctions at analyzer/service.ts:1160

[modes] values that some code compares by identity (porting.md wants a handle): 0.

