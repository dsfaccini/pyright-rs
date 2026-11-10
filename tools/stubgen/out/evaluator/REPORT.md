# stubgen run `evaluator`

Stubs (copy candidates for `crates/pyright/src`):

- `stubs/analyzer/type_evaluator.rs` from `analyzer/typeEvaluator.ts`
- `stubs/analyzer/types.rs` from `analyzer/types.ts`

Coverage: markers for every TypeScript function, method and type of each stubbed file, and methods for every member of each factory interface:

- `analyzer/typeEvaluator.ts`: 416/416 declarations have a marker
- `analyzer/types.ts`: 289/289 declarations have a marker
- `TypeEvaluator`: 112/112 members are `pub` methods of `TypeEvaluator`

Dependency placeholders (compile check only):

- `deps/analyzer/analyzer_file_info.rs`: AbsoluteModuleDescriptor, AnalyzerFileInfo, ImportLookup, ImportLookupResult, LookupImportOptions
- `deps/analyzer/analyzer_node_info.rs`: AnalyzerNodeInfo, AnalyzerNodeInfoAccessor, AnalyzerNodeInfoReader, AnalyzerNodeInfoWriter, DunderAllInfo
- `deps/analyzer/code_flow_engine.rs`: CachedType, CodeFlowAnalyzer, CodeFlowEngine, CodeFlowTypeCache, FlowNodeTypeOptions, FlowNodeTypeResult, IncompleteSubtypeInfo, IncompleteType, ReachabilityCacheEntry
- `deps/analyzer/code_flow_types.rs`: CodeFlowReferenceExpressionNode, FlowFlags, FlowNode, FlowPreFinallyGate
- `deps/analyzer/constraint_tracker.rs`: ConstraintSet, ConstraintTracker, TypeVarConstraints
- `deps/analyzer/declaration.rs`: DeclarationBase, DeclarationType, IntrinsicType, ModuleLoaderActions
- `deps/analyzer/declaration_utils.rs`: ResolvedAliasInfo
- `deps/analyzer/import_result.rs`: ImplicitImport, ImportResult, ImportType
- `deps/analyzer/parameter_utils.rs`: ParamKind, ParamListDetails, VirtualParamDetails
- `deps/analyzer/py_typed_utils.rs`: PyTypedInfo
- `deps/analyzer/scope.rs`: ChainedModuleLevelLookupContext, NameBindingType, Scope, ScopeChainedModuleLevelLookup, ScopeType, SymbolWithScope
- `deps/analyzer/source_file.rs`: IPythonMode
- `deps/analyzer/symbol.rs`: Symbol, SymbolFlags, SynthesizedTypeInfo
- `deps/analyzer/type_cache_utils.rs`: DependentType, SpeculativeContext, SpeculativeEntry, SpeculativeModeOptions, SpeculativeTypeEntry, SpeculativeTypeTracker, TypeResult
- `deps/analyzer/type_evaluator_types.rs`: AbstractSymbol, Arg, ArgResult, ArgWithExpression, ArgWithType, AssignTypeFlags, CallResult, CallSignature, CallSignatureInfo, CallSiteEvaluationInfo, ClassMemberLookup, ClassTypeResult, DeclaredSymbolTypeInfo, EffectiveTypeResult, EvalFlags, EvaluatorUsage, ExpectedTypeOptions, ExpectedTypeResult, FunctionTypeResult, MagicMethodDeprecationInfo, MapSubtypesOptions, MemberAccessDeprecationInfo, PartialPrefetchedTypes, PrefetchedTypes, PrintTypeOptions, Reachability, ResolveAliasOptions, SolveConstraintsOptions, SymbolDeclInfo, TypeResult, TypeResultWithNode, ValidateArgTypeParams, ValidateTypeArgsOptions
- `deps/analyzer/type_printer.rs`: PrintTypeFlags
- `deps/analyzer/type_utils.rs`: ApplyTypeVarOptions, ApplyTypeVarOptionsReplaceUnsolved, ClassMember, InferenceContext, MemberAccessFlags, UniqueSignatureTracker
- `deps/common/config_options.rs`: DiagnosticLevel, DiagnosticRuleSet, ExecutionEnvironment
- `deps/common/console.rs`: ConsoleInterface
- `deps/common/diagnostic.rs`: Diagnostic, DiagnosticAction, DiagnosticAddendum, DiagnosticCategory, DiagnosticRelatedInfo, TaskListPriority
- `deps/common/diagnostic_rules.rs`: DiagnosticRule
- `deps/common/diagnostic_sink.rs`: TextRangeDiagnosticSink
- `deps/common/python_version.rs`: PythonReleaseLevel, PythonVersion
- `deps/common/text_range.rs`: Position, Range, TextRange
- `deps/common/text_range_collection.rs`: TextRangeCollection
- `deps/common/uri/uri.rs`: Uri
- `deps/parser/parse_nodes.rs`: ArgCategory, ArgumentNode, AssertNode, AssignmentExpressionNode, AssignmentNode, AugmentedAssignmentNode, AwaitNode, BinaryOperationNode, BreakNode, CallNode, CaseNode, ClassNode, ComprehensionForIfNode, ComprehensionForNode, ComprehensionIfNode, ComprehensionNode, ConstantNode, ContinueNode, DecoratorNode, DelNode, DictionaryEntryNode, DictionaryExpandEntryNode, DictionaryKeyEntryNode, DictionaryNode, EllipsisNode, ErrorExpressionCategory, ErrorNode, ExceptNode, ExecutionScopeNode, ExpressionNode, ForNode, FormatStringNode, FunctionAnnotationNode, FunctionNode, GlobalNode, IfNode, ImportAsNode, ImportFromAsNode, ImportFromNode, ImportNode, IndexNode, LambdaNode, ListNode, MatchNode, MemberAccessNode, ModuleNameNode, ModuleNode, NameNode, NonlocalNode, NumberNode, ParamCategory, ParameterNode, ParseNodeBase, ParseNodeType, PassNode, PatternAsNode, PatternAtomNode, PatternCaptureNode, PatternClassArgumentNode, PatternClassNode, PatternLiteralNode, PatternMappingEntryNode, PatternMappingExpandEntryNode, PatternMappingKeyEntryNode, PatternMappingNode, PatternSequenceNode, PatternValueNode, RaiseNode, ReturnNode, SetNode, SliceNode, StatementListNode, StatementNode, StringListNode, StringNode, SuiteNode, TernaryNode, TryNode, TupleNode, TypeAliasNode, TypeAnnotationNode, TypeParamKind, TypeParameterListNode, TypeParameterNode, UnaryOperationNode, UnpackNode, WhileNode, WithItemNode, WithNode, YieldFromNode, YieldNode
- `deps/parser/tokenizer_types.rs`: Comment, CommentType, FStringMiddleToken, FStringStartToken, IdentifierToken, KeywordToken, KeywordType, NewLineType, OperatorType, StringToken, StringTokenFlags, TokenBase, TokenType

Number sites without a reviewed type (numbers.toml, values ending in "?"): 0 in the stubbed files, 53 in placeholders.

Unmapped TypeScript types (rendered with a `/*? ... */` marker): 7 distinct.

- `any` x12: analyzer/typeEvaluator.ts:LogWrapper, common/diagnostic.ts:Diagnostic._data, analyzer/typeCacheUtils.ts:SpeculativeEntry.cache, ...
- `Uri | AbsoluteModuleDescriptor` x4: analyzer/analyzerFileInfo.ts:ImportLookup
- `boolean | string` x4: analyzer/analyzerFileInfo.ts:AnalyzerFileInfo.definedConstants
- `Parameters<T>` x1: analyzer/typeEvaluator.ts:LogWrapper
- `ReturnType<T>` x1: analyzer/typeEvaluator.ts:LogWrapper
- `object` x1: analyzer/types.ts:ClassDetailsShared.protocolCompatibility
- `0 | 1 | -1` x1: analyzer/types.ts:TypeCondition._compare.return

Interfaces held as `P<T>` by the default rule (a field is written after construction); set [modes] to override:

- `analyzer/analyzerFileInfo.ts:AnalyzerFileInfo`: accessedSymbolSet at analyzer/typeEvaluator.ts:5123
- `analyzer/analyzerNodeInfo.ts:AnalyzerNodeInfo`: importInfo at analyzer/analyzerNodeInfo.ts:554
- `analyzer/codeFlowEngine.ts:CodeFlowTypeCache`: cache at analyzer/codeFlowEngine.ts:313
- `analyzer/codeFlowEngine.ts:FlowNodeTypeResult`: type at analyzer/codeFlowEngine.ts:600
- `analyzer/codeFlowEngine.ts:IncompleteType`: type at analyzer/codeFlowEngine.ts:373
- `analyzer/codeFlowEngine.ts:ReachabilityCacheEntry`: reachability at analyzer/codeFlowEngine.ts:1277
- `analyzer/declaration.ts:ModuleLoaderActions`: uri at analyzer/binder.ts:3019
- `analyzer/importResult.ts:ImplicitImport`: isNativeLib at analyzer/importResolver.ts:812
- `analyzer/importResult.ts:ImportResult`: isRelative at analyzer/importResolver.ts:925
- `analyzer/parameterUtils.ts:ParamListDetails`: params at analyzer/typeEvaluator.ts:28147
- `analyzer/typeCacheUtils.ts:SpeculativeContext`: entriesToUndo at analyzer/typeCacheUtils.ts:162
- `analyzer/typeCacheUtils.ts:SpeculativeEntry`: cache at analyzer/typeCacheUtils.ts:134
- `analyzer/typeCacheUtils.ts:SpeculativeTypeEntry`: dependentTypes at analyzer/typeCacheUtils.ts:198
- `analyzer/typeEvaluator.ts:DeferredClassCompletion`: classesToComplete at analyzer/typeEvaluator.ts:19914
- `analyzer/typeEvaluator.ts:ExpectedTypeCacheEntry`: type at analyzer/typeEvaluator.ts:1159
- `analyzer/typeEvaluatorTypes.ts:CallResult`: returnType at analyzer/typeEvaluator.ts:11896
- `analyzer/typeEvaluatorTypes.ts:EffectiveTypeResult`: type at analyzer/typeEvaluator.ts:25503
- `analyzer/typeEvaluatorTypes.ts:ExpectedTypeOptions`: notParsed at analyzer/typeEvaluator.ts:2132
- `analyzer/typeEvaluatorTypes.ts:PrefetchedTypes`: noneTypeClass at analyzer/typeEvaluator.ts:27032
- `analyzer/typeEvaluatorTypes.ts:TypeResult`: type at analyzer/tuples.ts:146
- `analyzer/typeUtils.ts:ClassMember`: isClassMember at analyzer/typeUtils.ts:1784
- `analyzer/typeUtils.ts:InferenceContext`: expectedType at analyzer/typeEvaluator.ts:1369
- `analyzer/types.ts:ClassDetailsShared`: flags at analyzer/types.ts:1063
- `analyzer/types.ts:DataClassBehaviors`: skipGenerateInit at analyzer/dataClasses.ts:1643
- `analyzer/types.ts:DataClassEntry`: hasDefault at analyzer/dataClasses.ts:591
- `analyzer/types.ts:FunctionDetailsShared`: flags at analyzer/types.ts:2002
- `analyzer/types.ts:LiteralTypes`: literalStrMap at analyzer/types.ts:2734
- `analyzer/types.ts:ParamSpecDetailsPriv`: paramSpecAccess at analyzer/types.ts:3081
- `analyzer/types.ts:SignatureWithOffsets`: expressionOffsets at analyzer/typeUtils.ts:287
- `analyzer/types.ts:SpecializedFunctionTypes`: parameterTypes at analyzer/types.ts:2143
- `analyzer/types.ts:TypeAliasSharedInfo`: typeParams at analyzer/typeEvaluator.ts:18021
- `analyzer/types.ts:TypeVarDetailsShared`: name at analyzer/types.ts:2998
- `analyzer/types.ts:TypeVarTupleDetailsPriv`: isInUnion at analyzer/types.ts:3030
- `analyzer/types.ts:TypedDictEntries`: knownItems at analyzer/typedDicts.ts:1138
- `common/configOptions.ts:DiagnosticRuleSet`: analyzeUnannotatedFunctions at analyzer/service.ts:1160

[modes] values that some code compares by identity (porting.md wants a handle): 0.

