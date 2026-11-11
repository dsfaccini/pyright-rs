//! Port of `packages/pyright-internal/src/common/configOptions.ts`.
//!
//! A config object is a `serde_json::Value` (`preserve_order`), standing in
//! for the JS object `JSONC.parse` or `smol-toml` returns. Key order is
//! insertion order: config keys are never integer-like.

use jsrt::{JsMap, JsSet, P};
use serde_json::Value;

use crate::analyzer::import_logger::ImportLogger;
use crate::analyzer::python_path_utils::get_paths_from_pth_files;
use crate::common::console::{ConsoleInterface, node_stderr, node_stdout};
use crate::common::diagnostic::TaskListToken;
use crate::common::diagnostic_rules::DiagnosticRule;
use crate::common::extra_path_glob::{contains_wildcard, expand_extra_paths};
use crate::common::file_system::FileSystem;
use crate::common::host::Host;
use crate::common::path_consts;
use crate::common::python_version::{LATEST_STABLE_PYTHON_VERSION, PythonVersion};
use crate::common::uri::Uri;
use crate::common::uri::uri_utils::{FileSpec, get_file_spec, is_directory};

// common/configOptions.ts:35
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PythonPlatform {
    Darwin,
    Windows,
    Linux,
    iOS,
    Android,
}

impl PythonPlatform {
    pub fn as_str(self) -> &'static str {
        match self {
            PythonPlatform::Darwin => "Darwin",
            PythonPlatform::Windows => "Windows",
            PythonPlatform::Linux => "Linux",
            PythonPlatform::iOS => "iOS",
            PythonPlatform::Android => "Android",
        }
    }
}

// common/configOptions.ts:43
#[derive(Clone, Debug)]
#[derive(jsrt::Trace)]
pub struct ExecutionEnvironment {
    pub root: Option<Uri>,
    pub name: String,
    pub python_version: PythonVersion,
    pub python_platform: Option<String>,
    pub extra_paths: Vec<Uri>,
    pub diagnostic_rule_set: DiagnosticRuleSet,
    pub skip_native_libraries: bool,
}

impl ExecutionEnvironment {
    // common/configOptions.ts:70
    pub fn new(
        name: String,
        root: Uri,
        default_diag_rule_set: &DiagnosticRuleSet,
        default_python_version: Option<PythonVersion>,
        default_python_platform: Option<String>,
        default_extra_paths: Option<&[Uri]>,
        skip_native_libraries: bool,
    ) -> ExecutionEnvironment {
        ExecutionEnvironment {
            name,
            root: Some(root),
            python_version: default_python_version.unwrap_or(LATEST_STABLE_PYTHON_VERSION),
            python_platform: default_python_platform,
            extra_paths: default_extra_paths.map(<[Uri]>::to_vec).unwrap_or_default(),
            diagnostic_rule_set: default_diag_rule_set.clone(),
            skip_native_libraries,
        }
    }
}

// common/configOptions.ts:89
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[derive(jsrt::Trace)]
pub enum DiagnosticLevel {
    None,
    Information,
    Warning,
    Error,
}

impl DiagnosticLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            DiagnosticLevel::None => "none",
            DiagnosticLevel::Information => "information",
            DiagnosticLevel::Warning => "warning",
            DiagnosticLevel::Error => "error",
        }
    }

    pub fn from_name(value: &str) -> Option<DiagnosticLevel> {
        match value {
            "none" => Some(DiagnosticLevel::None),
            "information" => Some(DiagnosticLevel::Information),
            "warning" => Some(DiagnosticLevel::Warning),
            "error" => Some(DiagnosticLevel::Error),
            _ => None,
        }
    }
}

// common/configOptions.ts:91
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[derive(jsrt::Trace)]
pub enum SignatureDisplayType {
    compact,
    formatted,
}

// common/configOptions.ts:96
/// Field names are the TypeScript names in snake_case. `get_*`/`set_*` reach a
/// field by its `DiagnosticRule`, as the TypeScript does with `ruleSet[rule]`.
#[derive(Clone, Debug, PartialEq)]
#[derive(jsrt::Trace)]
pub struct DiagnosticRuleSet {
    pub print_unknown_as_any: bool,
    pub omit_type_args_if_unknown: bool,
    pub omit_unannotated_param_type: bool,
    pub omit_conditional_constraint: bool,
    pub pep604_printing: bool,
    pub strict_list_inference: bool,
    pub strict_set_inference: bool,
    pub strict_dictionary_inference: bool,
    pub analyze_unannotated_functions: bool,
    pub strict_parameter_none_value: bool,
    pub enable_experimental_features: bool,
    pub enable_type_ignore_comments: bool,
    pub enable_reachability_analysis: bool,
    pub deprecate_typing_aliases: bool,
    pub disable_bytes_type_promotions: bool,
    pub report_general_type_issues: DiagnosticLevel,
    pub report_property_type_mismatch: DiagnosticLevel,
    pub report_function_member_access: DiagnosticLevel,
    pub report_missing_imports: DiagnosticLevel,
    pub report_missing_module_source: DiagnosticLevel,
    pub report_invalid_type_form: DiagnosticLevel,
    pub report_missing_type_stubs: DiagnosticLevel,
    pub report_import_cycles: DiagnosticLevel,
    pub report_unused_import: DiagnosticLevel,
    pub report_unused_class: DiagnosticLevel,
    pub report_unused_function: DiagnosticLevel,
    pub report_unused_variable: DiagnosticLevel,
    pub report_duplicate_import: DiagnosticLevel,
    pub report_wildcard_import_from_library: DiagnosticLevel,
    pub report_abstract_usage: DiagnosticLevel,
    pub report_argument_type: DiagnosticLevel,
    pub report_assert_type_failure: DiagnosticLevel,
    pub report_assignment_type: DiagnosticLevel,
    pub report_attribute_access_issue: DiagnosticLevel,
    pub report_call_issue: DiagnosticLevel,
    pub report_inconsistent_overload: DiagnosticLevel,
    pub report_index_issue: DiagnosticLevel,
    pub report_invalid_type_arguments: DiagnosticLevel,
    pub report_no_overload_implementation: DiagnosticLevel,
    pub report_operator_issue: DiagnosticLevel,
    pub report_optional_subscript: DiagnosticLevel,
    pub report_optional_member_access: DiagnosticLevel,
    pub report_optional_call: DiagnosticLevel,
    pub report_optional_iterable: DiagnosticLevel,
    pub report_optional_context_manager: DiagnosticLevel,
    pub report_optional_operand: DiagnosticLevel,
    pub report_redeclaration: DiagnosticLevel,
    pub report_return_type: DiagnosticLevel,
    pub report_typed_dict_not_required_access: DiagnosticLevel,
    pub report_untyped_function_decorator: DiagnosticLevel,
    pub report_untyped_class_decorator: DiagnosticLevel,
    pub report_untyped_base_class: DiagnosticLevel,
    pub report_untyped_named_tuple: DiagnosticLevel,
    pub report_private_usage: DiagnosticLevel,
    pub report_type_comment_usage: DiagnosticLevel,
    pub report_private_import_usage: DiagnosticLevel,
    pub report_constant_redefinition: DiagnosticLevel,
    pub report_deprecated: DiagnosticLevel,
    pub report_incompatible_method_override: DiagnosticLevel,
    pub report_incompatible_variable_override: DiagnosticLevel,
    pub report_inconsistent_constructor: DiagnosticLevel,
    pub report_overlapping_overload: DiagnosticLevel,
    pub report_possibly_unbound_variable: DiagnosticLevel,
    pub report_missing_super_call: DiagnosticLevel,
    pub report_uninitialized_instance_variable: DiagnosticLevel,
    pub report_invalid_string_escape_sequence: DiagnosticLevel,
    pub report_unknown_parameter_type: DiagnosticLevel,
    pub report_unknown_argument_type: DiagnosticLevel,
    pub report_unknown_lambda_type: DiagnosticLevel,
    pub report_unknown_variable_type: DiagnosticLevel,
    pub report_unknown_member_type: DiagnosticLevel,
    pub report_missing_parameter_type: DiagnosticLevel,
    pub report_missing_type_argument: DiagnosticLevel,
    pub report_invalid_type_var_use: DiagnosticLevel,
    pub report_call_in_default_initializer: DiagnosticLevel,
    pub report_unnecessary_is_instance: DiagnosticLevel,
    pub report_unnecessary_cast: DiagnosticLevel,
    pub report_unnecessary_comparison: DiagnosticLevel,
    pub report_unnecessary_contains: DiagnosticLevel,
    pub report_assert_always_true: DiagnosticLevel,
    pub report_self_cls_parameter_name: DiagnosticLevel,
    pub report_implicit_string_concatenation: DiagnosticLevel,
    pub report_undefined_variable: DiagnosticLevel,
    pub report_unbound_variable: DiagnosticLevel,
    pub report_unhashable: DiagnosticLevel,
    pub report_invalid_stub_statement: DiagnosticLevel,
    pub report_incomplete_stub: DiagnosticLevel,
    pub report_unsupported_dunder_all: DiagnosticLevel,
    pub report_unused_call_result: DiagnosticLevel,
    pub report_unused_coroutine: DiagnosticLevel,
    pub report_unused_except: DiagnosticLevel,
    pub report_unused_expression: DiagnosticLevel,
    pub report_unnecessary_type_ignore_comment: DiagnosticLevel,
    pub report_match_not_exhaustive: DiagnosticLevel,
    pub report_unreachable: DiagnosticLevel,
    pub report_implicit_override: DiagnosticLevel,
}

impl DiagnosticRuleSet {
    /// `ruleSet[rule]` for a rule whose value is a boolean.
    pub fn get_bool(&self, rule: DiagnosticRule) -> Option<bool> {
        Some(match rule {
            DiagnosticRule::strictListInference => self.strict_list_inference,
            DiagnosticRule::strictSetInference => self.strict_set_inference,
            DiagnosticRule::strictDictionaryInference => self.strict_dictionary_inference,
            DiagnosticRule::analyzeUnannotatedFunctions => self.analyze_unannotated_functions,
            DiagnosticRule::strictParameterNoneValue => self.strict_parameter_none_value,
            DiagnosticRule::enableExperimentalFeatures => self.enable_experimental_features,
            DiagnosticRule::enableTypeIgnoreComments => self.enable_type_ignore_comments,
            DiagnosticRule::enableReachabilityAnalysis => self.enable_reachability_analysis,
            DiagnosticRule::deprecateTypingAliases => self.deprecate_typing_aliases,
            DiagnosticRule::disableBytesTypePromotions => self.disable_bytes_type_promotions,
            _ => return None,
        })
    }

    /// `ruleSet[rule] = value` for a rule whose value is a boolean.
    pub fn set_bool(&mut self, rule: DiagnosticRule, value: bool) {
        match rule {
            DiagnosticRule::strictListInference => self.strict_list_inference = value,
            DiagnosticRule::strictSetInference => self.strict_set_inference = value,
            DiagnosticRule::strictDictionaryInference => self.strict_dictionary_inference = value,
            DiagnosticRule::analyzeUnannotatedFunctions => {
                self.analyze_unannotated_functions = value
            }
            DiagnosticRule::strictParameterNoneValue => self.strict_parameter_none_value = value,
            DiagnosticRule::enableExperimentalFeatures => self.enable_experimental_features = value,
            DiagnosticRule::enableTypeIgnoreComments => self.enable_type_ignore_comments = value,
            DiagnosticRule::enableReachabilityAnalysis => self.enable_reachability_analysis = value,
            DiagnosticRule::deprecateTypingAliases => self.deprecate_typing_aliases = value,
            DiagnosticRule::disableBytesTypePromotions => {
                self.disable_bytes_type_promotions = value
            }
            _ => panic!("{rule} is not a boolean rule"),
        }
    }

    /// `ruleSet[rule]` for a rule whose value is a `DiagnosticLevel`.
    pub fn get_level(&self, rule: DiagnosticRule) -> Option<DiagnosticLevel> {
        Some(match rule {
            DiagnosticRule::reportGeneralTypeIssues => self.report_general_type_issues,
            DiagnosticRule::reportPropertyTypeMismatch => self.report_property_type_mismatch,
            DiagnosticRule::reportFunctionMemberAccess => self.report_function_member_access,
            DiagnosticRule::reportMissingImports => self.report_missing_imports,
            DiagnosticRule::reportMissingModuleSource => self.report_missing_module_source,
            DiagnosticRule::reportInvalidTypeForm => self.report_invalid_type_form,
            DiagnosticRule::reportMissingTypeStubs => self.report_missing_type_stubs,
            DiagnosticRule::reportImportCycles => self.report_import_cycles,
            DiagnosticRule::reportUnusedImport => self.report_unused_import,
            DiagnosticRule::reportUnusedClass => self.report_unused_class,
            DiagnosticRule::reportUnusedFunction => self.report_unused_function,
            DiagnosticRule::reportUnusedVariable => self.report_unused_variable,
            DiagnosticRule::reportDuplicateImport => self.report_duplicate_import,
            DiagnosticRule::reportWildcardImportFromLibrary => {
                self.report_wildcard_import_from_library
            }
            DiagnosticRule::reportAbstractUsage => self.report_abstract_usage,
            DiagnosticRule::reportArgumentType => self.report_argument_type,
            DiagnosticRule::reportAssertTypeFailure => self.report_assert_type_failure,
            DiagnosticRule::reportAssignmentType => self.report_assignment_type,
            DiagnosticRule::reportAttributeAccessIssue => self.report_attribute_access_issue,
            DiagnosticRule::reportCallIssue => self.report_call_issue,
            DiagnosticRule::reportInconsistentOverload => self.report_inconsistent_overload,
            DiagnosticRule::reportIndexIssue => self.report_index_issue,
            DiagnosticRule::reportInvalidTypeArguments => self.report_invalid_type_arguments,
            DiagnosticRule::reportNoOverloadImplementation => {
                self.report_no_overload_implementation
            }
            DiagnosticRule::reportOperatorIssue => self.report_operator_issue,
            DiagnosticRule::reportOptionalSubscript => self.report_optional_subscript,
            DiagnosticRule::reportOptionalMemberAccess => self.report_optional_member_access,
            DiagnosticRule::reportOptionalCall => self.report_optional_call,
            DiagnosticRule::reportOptionalIterable => self.report_optional_iterable,
            DiagnosticRule::reportOptionalContextManager => self.report_optional_context_manager,
            DiagnosticRule::reportOptionalOperand => self.report_optional_operand,
            DiagnosticRule::reportRedeclaration => self.report_redeclaration,
            DiagnosticRule::reportReturnType => self.report_return_type,
            DiagnosticRule::reportTypedDictNotRequiredAccess => {
                self.report_typed_dict_not_required_access
            }
            DiagnosticRule::reportUntypedFunctionDecorator => {
                self.report_untyped_function_decorator
            }
            DiagnosticRule::reportUntypedClassDecorator => self.report_untyped_class_decorator,
            DiagnosticRule::reportUntypedBaseClass => self.report_untyped_base_class,
            DiagnosticRule::reportUntypedNamedTuple => self.report_untyped_named_tuple,
            DiagnosticRule::reportPrivateUsage => self.report_private_usage,
            DiagnosticRule::reportTypeCommentUsage => self.report_type_comment_usage,
            DiagnosticRule::reportPrivateImportUsage => self.report_private_import_usage,
            DiagnosticRule::reportConstantRedefinition => self.report_constant_redefinition,
            DiagnosticRule::reportDeprecated => self.report_deprecated,
            DiagnosticRule::reportIncompatibleMethodOverride => {
                self.report_incompatible_method_override
            }
            DiagnosticRule::reportIncompatibleVariableOverride => {
                self.report_incompatible_variable_override
            }
            DiagnosticRule::reportInconsistentConstructor => self.report_inconsistent_constructor,
            DiagnosticRule::reportOverlappingOverload => self.report_overlapping_overload,
            DiagnosticRule::reportPossiblyUnboundVariable => self.report_possibly_unbound_variable,
            DiagnosticRule::reportMissingSuperCall => self.report_missing_super_call,
            DiagnosticRule::reportUninitializedInstanceVariable => {
                self.report_uninitialized_instance_variable
            }
            DiagnosticRule::reportInvalidStringEscapeSequence => {
                self.report_invalid_string_escape_sequence
            }
            DiagnosticRule::reportUnknownParameterType => self.report_unknown_parameter_type,
            DiagnosticRule::reportUnknownArgumentType => self.report_unknown_argument_type,
            DiagnosticRule::reportUnknownLambdaType => self.report_unknown_lambda_type,
            DiagnosticRule::reportUnknownVariableType => self.report_unknown_variable_type,
            DiagnosticRule::reportUnknownMemberType => self.report_unknown_member_type,
            DiagnosticRule::reportMissingParameterType => self.report_missing_parameter_type,
            DiagnosticRule::reportMissingTypeArgument => self.report_missing_type_argument,
            DiagnosticRule::reportInvalidTypeVarUse => self.report_invalid_type_var_use,
            DiagnosticRule::reportCallInDefaultInitializer => {
                self.report_call_in_default_initializer
            }
            DiagnosticRule::reportUnnecessaryIsInstance => self.report_unnecessary_is_instance,
            DiagnosticRule::reportUnnecessaryCast => self.report_unnecessary_cast,
            DiagnosticRule::reportUnnecessaryComparison => self.report_unnecessary_comparison,
            DiagnosticRule::reportUnnecessaryContains => self.report_unnecessary_contains,
            DiagnosticRule::reportAssertAlwaysTrue => self.report_assert_always_true,
            DiagnosticRule::reportSelfClsParameterName => self.report_self_cls_parameter_name,
            DiagnosticRule::reportImplicitStringConcatenation => {
                self.report_implicit_string_concatenation
            }
            DiagnosticRule::reportUndefinedVariable => self.report_undefined_variable,
            DiagnosticRule::reportUnboundVariable => self.report_unbound_variable,
            DiagnosticRule::reportUnhashable => self.report_unhashable,
            DiagnosticRule::reportInvalidStubStatement => self.report_invalid_stub_statement,
            DiagnosticRule::reportIncompleteStub => self.report_incomplete_stub,
            DiagnosticRule::reportUnsupportedDunderAll => self.report_unsupported_dunder_all,
            DiagnosticRule::reportUnusedCallResult => self.report_unused_call_result,
            DiagnosticRule::reportUnusedCoroutine => self.report_unused_coroutine,
            DiagnosticRule::reportUnusedExcept => self.report_unused_except,
            DiagnosticRule::reportUnusedExpression => self.report_unused_expression,
            DiagnosticRule::reportUnnecessaryTypeIgnoreComment => {
                self.report_unnecessary_type_ignore_comment
            }
            DiagnosticRule::reportMatchNotExhaustive => self.report_match_not_exhaustive,
            DiagnosticRule::reportUnreachable => self.report_unreachable,
            DiagnosticRule::reportImplicitOverride => self.report_implicit_override,
            _ => return None,
        })
    }

    /// `ruleSet[rule] = value` for a rule whose value is a `DiagnosticLevel`.
    pub fn set_level(&mut self, rule: DiagnosticRule, value: DiagnosticLevel) {
        match rule {
            DiagnosticRule::reportGeneralTypeIssues => self.report_general_type_issues = value,
            DiagnosticRule::reportPropertyTypeMismatch => {
                self.report_property_type_mismatch = value
            }
            DiagnosticRule::reportFunctionMemberAccess => {
                self.report_function_member_access = value
            }
            DiagnosticRule::reportMissingImports => self.report_missing_imports = value,
            DiagnosticRule::reportMissingModuleSource => self.report_missing_module_source = value,
            DiagnosticRule::reportInvalidTypeForm => self.report_invalid_type_form = value,
            DiagnosticRule::reportMissingTypeStubs => self.report_missing_type_stubs = value,
            DiagnosticRule::reportImportCycles => self.report_import_cycles = value,
            DiagnosticRule::reportUnusedImport => self.report_unused_import = value,
            DiagnosticRule::reportUnusedClass => self.report_unused_class = value,
            DiagnosticRule::reportUnusedFunction => self.report_unused_function = value,
            DiagnosticRule::reportUnusedVariable => self.report_unused_variable = value,
            DiagnosticRule::reportDuplicateImport => self.report_duplicate_import = value,
            DiagnosticRule::reportWildcardImportFromLibrary => {
                self.report_wildcard_import_from_library = value
            }
            DiagnosticRule::reportAbstractUsage => self.report_abstract_usage = value,
            DiagnosticRule::reportArgumentType => self.report_argument_type = value,
            DiagnosticRule::reportAssertTypeFailure => self.report_assert_type_failure = value,
            DiagnosticRule::reportAssignmentType => self.report_assignment_type = value,
            DiagnosticRule::reportAttributeAccessIssue => {
                self.report_attribute_access_issue = value
            }
            DiagnosticRule::reportCallIssue => self.report_call_issue = value,
            DiagnosticRule::reportInconsistentOverload => self.report_inconsistent_overload = value,
            DiagnosticRule::reportIndexIssue => self.report_index_issue = value,
            DiagnosticRule::reportInvalidTypeArguments => {
                self.report_invalid_type_arguments = value
            }
            DiagnosticRule::reportNoOverloadImplementation => {
                self.report_no_overload_implementation = value
            }
            DiagnosticRule::reportOperatorIssue => self.report_operator_issue = value,
            DiagnosticRule::reportOptionalSubscript => self.report_optional_subscript = value,
            DiagnosticRule::reportOptionalMemberAccess => {
                self.report_optional_member_access = value
            }
            DiagnosticRule::reportOptionalCall => self.report_optional_call = value,
            DiagnosticRule::reportOptionalIterable => self.report_optional_iterable = value,
            DiagnosticRule::reportOptionalContextManager => {
                self.report_optional_context_manager = value
            }
            DiagnosticRule::reportOptionalOperand => self.report_optional_operand = value,
            DiagnosticRule::reportRedeclaration => self.report_redeclaration = value,
            DiagnosticRule::reportReturnType => self.report_return_type = value,
            DiagnosticRule::reportTypedDictNotRequiredAccess => {
                self.report_typed_dict_not_required_access = value
            }
            DiagnosticRule::reportUntypedFunctionDecorator => {
                self.report_untyped_function_decorator = value
            }
            DiagnosticRule::reportUntypedClassDecorator => {
                self.report_untyped_class_decorator = value
            }
            DiagnosticRule::reportUntypedBaseClass => self.report_untyped_base_class = value,
            DiagnosticRule::reportUntypedNamedTuple => self.report_untyped_named_tuple = value,
            DiagnosticRule::reportPrivateUsage => self.report_private_usage = value,
            DiagnosticRule::reportTypeCommentUsage => self.report_type_comment_usage = value,
            DiagnosticRule::reportPrivateImportUsage => self.report_private_import_usage = value,
            DiagnosticRule::reportConstantRedefinition => self.report_constant_redefinition = value,
            DiagnosticRule::reportDeprecated => self.report_deprecated = value,
            DiagnosticRule::reportIncompatibleMethodOverride => {
                self.report_incompatible_method_override = value
            }
            DiagnosticRule::reportIncompatibleVariableOverride => {
                self.report_incompatible_variable_override = value
            }
            DiagnosticRule::reportInconsistentConstructor => {
                self.report_inconsistent_constructor = value
            }
            DiagnosticRule::reportOverlappingOverload => self.report_overlapping_overload = value,
            DiagnosticRule::reportPossiblyUnboundVariable => {
                self.report_possibly_unbound_variable = value
            }
            DiagnosticRule::reportMissingSuperCall => self.report_missing_super_call = value,
            DiagnosticRule::reportUninitializedInstanceVariable => {
                self.report_uninitialized_instance_variable = value
            }
            DiagnosticRule::reportInvalidStringEscapeSequence => {
                self.report_invalid_string_escape_sequence = value
            }
            DiagnosticRule::reportUnknownParameterType => {
                self.report_unknown_parameter_type = value
            }
            DiagnosticRule::reportUnknownArgumentType => self.report_unknown_argument_type = value,
            DiagnosticRule::reportUnknownLambdaType => self.report_unknown_lambda_type = value,
            DiagnosticRule::reportUnknownVariableType => self.report_unknown_variable_type = value,
            DiagnosticRule::reportUnknownMemberType => self.report_unknown_member_type = value,
            DiagnosticRule::reportMissingParameterType => {
                self.report_missing_parameter_type = value
            }
            DiagnosticRule::reportMissingTypeArgument => self.report_missing_type_argument = value,
            DiagnosticRule::reportInvalidTypeVarUse => self.report_invalid_type_var_use = value,
            DiagnosticRule::reportCallInDefaultInitializer => {
                self.report_call_in_default_initializer = value
            }
            DiagnosticRule::reportUnnecessaryIsInstance => {
                self.report_unnecessary_is_instance = value
            }
            DiagnosticRule::reportUnnecessaryCast => self.report_unnecessary_cast = value,
            DiagnosticRule::reportUnnecessaryComparison => {
                self.report_unnecessary_comparison = value
            }
            DiagnosticRule::reportUnnecessaryContains => self.report_unnecessary_contains = value,
            DiagnosticRule::reportAssertAlwaysTrue => self.report_assert_always_true = value,
            DiagnosticRule::reportSelfClsParameterName => {
                self.report_self_cls_parameter_name = value
            }
            DiagnosticRule::reportImplicitStringConcatenation => {
                self.report_implicit_string_concatenation = value
            }
            DiagnosticRule::reportUndefinedVariable => self.report_undefined_variable = value,
            DiagnosticRule::reportUnboundVariable => self.report_unbound_variable = value,
            DiagnosticRule::reportUnhashable => self.report_unhashable = value,
            DiagnosticRule::reportInvalidStubStatement => {
                self.report_invalid_stub_statement = value
            }
            DiagnosticRule::reportIncompleteStub => self.report_incomplete_stub = value,
            DiagnosticRule::reportUnsupportedDunderAll => {
                self.report_unsupported_dunder_all = value
            }
            DiagnosticRule::reportUnusedCallResult => self.report_unused_call_result = value,
            DiagnosticRule::reportUnusedCoroutine => self.report_unused_coroutine = value,
            DiagnosticRule::reportUnusedExcept => self.report_unused_except = value,
            DiagnosticRule::reportUnusedExpression => self.report_unused_expression = value,
            DiagnosticRule::reportUnnecessaryTypeIgnoreComment => {
                self.report_unnecessary_type_ignore_comment = value
            }
            DiagnosticRule::reportMatchNotExhaustive => self.report_match_not_exhaustive = value,
            DiagnosticRule::reportUnreachable => self.report_unreachable = value,
            DiagnosticRule::reportImplicitOverride => self.report_implicit_override = value,
            _ => panic!("{rule} is not a level rule"),
        }
    }
}

// common/configOptions.ts:404
pub fn clone_diagnostic_rule_set(diag_settings: &DiagnosticRuleSet) -> DiagnosticRuleSet {
    diag_settings.clone()
}

// common/configOptions.ts:411
pub fn get_boolean_diagnostic_rules(include_non_overridable: bool) -> Vec<DiagnosticRule> {
    let mut bool_rules = vec![
        DiagnosticRule::strictListInference,
        DiagnosticRule::strictSetInference,
        DiagnosticRule::strictDictionaryInference,
        DiagnosticRule::analyzeUnannotatedFunctions,
        DiagnosticRule::strictParameterNoneValue,
        DiagnosticRule::enableExperimentalFeatures,
        DiagnosticRule::deprecateTypingAliases,
        DiagnosticRule::disableBytesTypePromotions,
    ];

    if include_non_overridable {
        bool_rules.push(DiagnosticRule::enableTypeIgnoreComments);
        bool_rules.push(DiagnosticRule::enableReachabilityAnalysis);
    }

    bool_rules
}

// common/configOptions.ts:436
pub fn get_diag_level_diagnostic_rules() -> Vec<DiagnosticRule> {
    vec![
        DiagnosticRule::reportGeneralTypeIssues,
        DiagnosticRule::reportPropertyTypeMismatch,
        DiagnosticRule::reportFunctionMemberAccess,
        DiagnosticRule::reportMissingImports,
        DiagnosticRule::reportMissingModuleSource,
        DiagnosticRule::reportInvalidTypeForm,
        DiagnosticRule::reportMissingTypeStubs,
        DiagnosticRule::reportImportCycles,
        DiagnosticRule::reportUnusedImport,
        DiagnosticRule::reportUnusedClass,
        DiagnosticRule::reportUnusedFunction,
        DiagnosticRule::reportUnusedVariable,
        DiagnosticRule::reportDuplicateImport,
        DiagnosticRule::reportWildcardImportFromLibrary,
        DiagnosticRule::reportAbstractUsage,
        DiagnosticRule::reportArgumentType,
        DiagnosticRule::reportAssertTypeFailure,
        DiagnosticRule::reportAssignmentType,
        DiagnosticRule::reportAttributeAccessIssue,
        DiagnosticRule::reportCallIssue,
        DiagnosticRule::reportInconsistentOverload,
        DiagnosticRule::reportIndexIssue,
        DiagnosticRule::reportInvalidTypeArguments,
        DiagnosticRule::reportNoOverloadImplementation,
        DiagnosticRule::reportOperatorIssue,
        DiagnosticRule::reportOptionalSubscript,
        DiagnosticRule::reportOptionalMemberAccess,
        DiagnosticRule::reportOptionalCall,
        DiagnosticRule::reportOptionalIterable,
        DiagnosticRule::reportOptionalContextManager,
        DiagnosticRule::reportOptionalOperand,
        DiagnosticRule::reportRedeclaration,
        DiagnosticRule::reportReturnType,
        DiagnosticRule::reportTypedDictNotRequiredAccess,
        DiagnosticRule::reportUntypedFunctionDecorator,
        DiagnosticRule::reportUntypedClassDecorator,
        DiagnosticRule::reportUntypedBaseClass,
        DiagnosticRule::reportUntypedNamedTuple,
        DiagnosticRule::reportPrivateUsage,
        DiagnosticRule::reportTypeCommentUsage,
        DiagnosticRule::reportPrivateImportUsage,
        DiagnosticRule::reportConstantRedefinition,
        DiagnosticRule::reportDeprecated,
        DiagnosticRule::reportIncompatibleMethodOverride,
        DiagnosticRule::reportIncompatibleVariableOverride,
        DiagnosticRule::reportInconsistentConstructor,
        DiagnosticRule::reportOverlappingOverload,
        DiagnosticRule::reportPossiblyUnboundVariable,
        DiagnosticRule::reportMissingSuperCall,
        DiagnosticRule::reportUninitializedInstanceVariable,
        DiagnosticRule::reportInvalidStringEscapeSequence,
        DiagnosticRule::reportUnknownParameterType,
        DiagnosticRule::reportUnknownArgumentType,
        DiagnosticRule::reportUnknownLambdaType,
        DiagnosticRule::reportUnknownVariableType,
        DiagnosticRule::reportUnknownMemberType,
        DiagnosticRule::reportMissingParameterType,
        DiagnosticRule::reportMissingTypeArgument,
        DiagnosticRule::reportInvalidTypeVarUse,
        DiagnosticRule::reportCallInDefaultInitializer,
        DiagnosticRule::reportUnnecessaryIsInstance,
        DiagnosticRule::reportUnnecessaryCast,
        DiagnosticRule::reportUnnecessaryComparison,
        DiagnosticRule::reportUnnecessaryContains,
        DiagnosticRule::reportAssertAlwaysTrue,
        DiagnosticRule::reportSelfClsParameterName,
        DiagnosticRule::reportImplicitStringConcatenation,
        DiagnosticRule::reportUndefinedVariable,
        DiagnosticRule::reportUnhashable,
        DiagnosticRule::reportUnboundVariable,
        DiagnosticRule::reportInvalidStubStatement,
        DiagnosticRule::reportIncompleteStub,
        DiagnosticRule::reportUnsupportedDunderAll,
        DiagnosticRule::reportUnusedCallResult,
        DiagnosticRule::reportUnusedCoroutine,
        DiagnosticRule::reportUnusedExcept,
        DiagnosticRule::reportUnusedExpression,
        DiagnosticRule::reportUnnecessaryTypeIgnoreComment,
        DiagnosticRule::reportMatchNotExhaustive,
        DiagnosticRule::reportUnreachable,
        DiagnosticRule::reportImplicitOverride,
    ]
}

// common/configOptions.ts:522
pub fn get_strict_mode_not_overridden_rules() -> Vec<DiagnosticRule> {
    vec![DiagnosticRule::reportMissingModuleSource]
}

// common/configOptions.ts:528
pub fn get_off_diagnostic_rule_set() -> DiagnosticRuleSet {
    DiagnosticRuleSet {
        print_unknown_as_any: true,
        omit_type_args_if_unknown: true,
        omit_unannotated_param_type: true,
        omit_conditional_constraint: true,
        pep604_printing: true,
        strict_list_inference: false,
        strict_set_inference: false,
        strict_dictionary_inference: false,
        analyze_unannotated_functions: true,
        strict_parameter_none_value: true,
        enable_experimental_features: false,
        enable_type_ignore_comments: true,
        enable_reachability_analysis: false,
        deprecate_typing_aliases: false,
        disable_bytes_type_promotions: true,
        report_general_type_issues: DiagnosticLevel::None,
        report_property_type_mismatch: DiagnosticLevel::None,
        report_function_member_access: DiagnosticLevel::None,
        report_missing_imports: DiagnosticLevel::Warning,
        report_missing_module_source: DiagnosticLevel::Warning,
        report_invalid_type_form: DiagnosticLevel::Warning,
        report_missing_type_stubs: DiagnosticLevel::None,
        report_import_cycles: DiagnosticLevel::None,
        report_unused_import: DiagnosticLevel::None,
        report_unused_class: DiagnosticLevel::None,
        report_unused_function: DiagnosticLevel::None,
        report_unused_variable: DiagnosticLevel::None,
        report_duplicate_import: DiagnosticLevel::None,
        report_wildcard_import_from_library: DiagnosticLevel::None,
        report_abstract_usage: DiagnosticLevel::None,
        report_argument_type: DiagnosticLevel::None,
        report_assert_type_failure: DiagnosticLevel::None,
        report_assignment_type: DiagnosticLevel::None,
        report_attribute_access_issue: DiagnosticLevel::None,
        report_call_issue: DiagnosticLevel::None,
        report_inconsistent_overload: DiagnosticLevel::None,
        report_index_issue: DiagnosticLevel::None,
        report_invalid_type_arguments: DiagnosticLevel::None,
        report_no_overload_implementation: DiagnosticLevel::None,
        report_operator_issue: DiagnosticLevel::None,
        report_optional_subscript: DiagnosticLevel::None,
        report_optional_member_access: DiagnosticLevel::None,
        report_optional_call: DiagnosticLevel::None,
        report_optional_iterable: DiagnosticLevel::None,
        report_optional_context_manager: DiagnosticLevel::None,
        report_optional_operand: DiagnosticLevel::None,
        report_redeclaration: DiagnosticLevel::None,
        report_return_type: DiagnosticLevel::None,
        report_typed_dict_not_required_access: DiagnosticLevel::None,
        report_untyped_function_decorator: DiagnosticLevel::None,
        report_untyped_class_decorator: DiagnosticLevel::None,
        report_untyped_base_class: DiagnosticLevel::None,
        report_untyped_named_tuple: DiagnosticLevel::None,
        report_private_usage: DiagnosticLevel::None,
        report_type_comment_usage: DiagnosticLevel::None,
        report_private_import_usage: DiagnosticLevel::None,
        report_constant_redefinition: DiagnosticLevel::None,
        report_deprecated: DiagnosticLevel::None,
        report_incompatible_method_override: DiagnosticLevel::None,
        report_incompatible_variable_override: DiagnosticLevel::None,
        report_inconsistent_constructor: DiagnosticLevel::None,
        report_overlapping_overload: DiagnosticLevel::None,
        report_possibly_unbound_variable: DiagnosticLevel::None,
        report_missing_super_call: DiagnosticLevel::None,
        report_uninitialized_instance_variable: DiagnosticLevel::None,
        report_invalid_string_escape_sequence: DiagnosticLevel::None,
        report_unknown_parameter_type: DiagnosticLevel::None,
        report_unknown_argument_type: DiagnosticLevel::None,
        report_unknown_lambda_type: DiagnosticLevel::None,
        report_unknown_variable_type: DiagnosticLevel::None,
        report_unknown_member_type: DiagnosticLevel::None,
        report_missing_parameter_type: DiagnosticLevel::None,
        report_missing_type_argument: DiagnosticLevel::None,
        report_invalid_type_var_use: DiagnosticLevel::None,
        report_call_in_default_initializer: DiagnosticLevel::None,
        report_unnecessary_is_instance: DiagnosticLevel::None,
        report_unnecessary_cast: DiagnosticLevel::None,
        report_unnecessary_comparison: DiagnosticLevel::None,
        report_unnecessary_contains: DiagnosticLevel::None,
        report_assert_always_true: DiagnosticLevel::None,
        report_self_cls_parameter_name: DiagnosticLevel::None,
        report_implicit_string_concatenation: DiagnosticLevel::None,
        report_undefined_variable: DiagnosticLevel::Warning,
        report_unbound_variable: DiagnosticLevel::None,
        report_unhashable: DiagnosticLevel::None,
        report_invalid_stub_statement: DiagnosticLevel::None,
        report_incomplete_stub: DiagnosticLevel::None,
        report_unsupported_dunder_all: DiagnosticLevel::None,
        report_unused_call_result: DiagnosticLevel::None,
        report_unused_coroutine: DiagnosticLevel::None,
        report_unused_except: DiagnosticLevel::None,
        report_unused_expression: DiagnosticLevel::None,
        report_unnecessary_type_ignore_comment: DiagnosticLevel::None,
        report_match_not_exhaustive: DiagnosticLevel::None,
        report_unreachable: DiagnosticLevel::None,
        report_implicit_override: DiagnosticLevel::None,
    }
}

// common/configOptions.ts:631
pub fn get_basic_diagnostic_rule_set() -> DiagnosticRuleSet {
    DiagnosticRuleSet {
        print_unknown_as_any: false,
        omit_type_args_if_unknown: false,
        omit_unannotated_param_type: true,
        omit_conditional_constraint: false,
        pep604_printing: true,
        strict_list_inference: false,
        strict_set_inference: false,
        strict_dictionary_inference: false,
        analyze_unannotated_functions: true,
        strict_parameter_none_value: true,
        enable_experimental_features: false,
        enable_type_ignore_comments: true,
        enable_reachability_analysis: true,
        deprecate_typing_aliases: false,
        disable_bytes_type_promotions: true,
        report_general_type_issues: DiagnosticLevel::Error,
        report_property_type_mismatch: DiagnosticLevel::None,
        report_function_member_access: DiagnosticLevel::None,
        report_missing_imports: DiagnosticLevel::Error,
        report_missing_module_source: DiagnosticLevel::Warning,
        report_invalid_type_form: DiagnosticLevel::Error,
        report_missing_type_stubs: DiagnosticLevel::None,
        report_import_cycles: DiagnosticLevel::None,
        report_unused_import: DiagnosticLevel::None,
        report_unused_class: DiagnosticLevel::None,
        report_unused_function: DiagnosticLevel::None,
        report_unused_variable: DiagnosticLevel::None,
        report_duplicate_import: DiagnosticLevel::None,
        report_wildcard_import_from_library: DiagnosticLevel::Warning,
        report_abstract_usage: DiagnosticLevel::Error,
        report_argument_type: DiagnosticLevel::Error,
        report_assert_type_failure: DiagnosticLevel::Error,
        report_assignment_type: DiagnosticLevel::Error,
        report_attribute_access_issue: DiagnosticLevel::Error,
        report_call_issue: DiagnosticLevel::Error,
        report_inconsistent_overload: DiagnosticLevel::Error,
        report_index_issue: DiagnosticLevel::Error,
        report_invalid_type_arguments: DiagnosticLevel::Error,
        report_no_overload_implementation: DiagnosticLevel::Error,
        report_operator_issue: DiagnosticLevel::Error,
        report_optional_subscript: DiagnosticLevel::Error,
        report_optional_member_access: DiagnosticLevel::Error,
        report_optional_call: DiagnosticLevel::Error,
        report_optional_iterable: DiagnosticLevel::Error,
        report_optional_context_manager: DiagnosticLevel::Error,
        report_optional_operand: DiagnosticLevel::Error,
        report_redeclaration: DiagnosticLevel::Error,
        report_return_type: DiagnosticLevel::Error,
        report_typed_dict_not_required_access: DiagnosticLevel::Error,
        report_untyped_function_decorator: DiagnosticLevel::None,
        report_untyped_class_decorator: DiagnosticLevel::None,
        report_untyped_base_class: DiagnosticLevel::None,
        report_untyped_named_tuple: DiagnosticLevel::None,
        report_private_usage: DiagnosticLevel::None,
        report_type_comment_usage: DiagnosticLevel::None,
        report_private_import_usage: DiagnosticLevel::Error,
        report_constant_redefinition: DiagnosticLevel::None,
        report_deprecated: DiagnosticLevel::None,
        report_incompatible_method_override: DiagnosticLevel::None,
        report_incompatible_variable_override: DiagnosticLevel::None,
        report_inconsistent_constructor: DiagnosticLevel::None,
        report_overlapping_overload: DiagnosticLevel::None,
        report_possibly_unbound_variable: DiagnosticLevel::None,
        report_missing_super_call: DiagnosticLevel::None,
        report_uninitialized_instance_variable: DiagnosticLevel::None,
        report_invalid_string_escape_sequence: DiagnosticLevel::Warning,
        report_unknown_parameter_type: DiagnosticLevel::None,
        report_unknown_argument_type: DiagnosticLevel::None,
        report_unknown_lambda_type: DiagnosticLevel::None,
        report_unknown_variable_type: DiagnosticLevel::None,
        report_unknown_member_type: DiagnosticLevel::None,
        report_missing_parameter_type: DiagnosticLevel::None,
        report_missing_type_argument: DiagnosticLevel::None,
        report_invalid_type_var_use: DiagnosticLevel::Warning,
        report_call_in_default_initializer: DiagnosticLevel::None,
        report_unnecessary_is_instance: DiagnosticLevel::None,
        report_unnecessary_cast: DiagnosticLevel::None,
        report_unnecessary_comparison: DiagnosticLevel::None,
        report_unnecessary_contains: DiagnosticLevel::None,
        report_assert_always_true: DiagnosticLevel::Warning,
        report_self_cls_parameter_name: DiagnosticLevel::Warning,
        report_implicit_string_concatenation: DiagnosticLevel::None,
        report_undefined_variable: DiagnosticLevel::Error,
        report_unbound_variable: DiagnosticLevel::Error,
        report_unhashable: DiagnosticLevel::Error,
        report_invalid_stub_statement: DiagnosticLevel::None,
        report_incomplete_stub: DiagnosticLevel::None,
        report_unsupported_dunder_all: DiagnosticLevel::Warning,
        report_unused_call_result: DiagnosticLevel::None,
        report_unused_coroutine: DiagnosticLevel::Error,
        report_unused_except: DiagnosticLevel::Error,
        report_unused_expression: DiagnosticLevel::Warning,
        report_unnecessary_type_ignore_comment: DiagnosticLevel::None,
        report_match_not_exhaustive: DiagnosticLevel::None,
        report_unreachable: DiagnosticLevel::None,
        report_implicit_override: DiagnosticLevel::None,
    }
}

// common/configOptions.ts:734
pub fn get_standard_diagnostic_rule_set() -> DiagnosticRuleSet {
    DiagnosticRuleSet {
        print_unknown_as_any: false,
        omit_type_args_if_unknown: false,
        omit_unannotated_param_type: true,
        omit_conditional_constraint: false,
        pep604_printing: true,
        strict_list_inference: false,
        strict_set_inference: false,
        strict_dictionary_inference: false,
        analyze_unannotated_functions: true,
        strict_parameter_none_value: true,
        enable_experimental_features: false,
        enable_type_ignore_comments: true,
        enable_reachability_analysis: true,
        deprecate_typing_aliases: false,
        disable_bytes_type_promotions: true,
        report_general_type_issues: DiagnosticLevel::Error,
        report_property_type_mismatch: DiagnosticLevel::None,
        report_function_member_access: DiagnosticLevel::Error,
        report_missing_imports: DiagnosticLevel::Error,
        report_missing_module_source: DiagnosticLevel::Warning,
        report_invalid_type_form: DiagnosticLevel::Error,
        report_missing_type_stubs: DiagnosticLevel::None,
        report_import_cycles: DiagnosticLevel::None,
        report_unused_import: DiagnosticLevel::None,
        report_unused_class: DiagnosticLevel::None,
        report_unused_function: DiagnosticLevel::None,
        report_unused_variable: DiagnosticLevel::None,
        report_duplicate_import: DiagnosticLevel::None,
        report_wildcard_import_from_library: DiagnosticLevel::Warning,
        report_abstract_usage: DiagnosticLevel::Error,
        report_argument_type: DiagnosticLevel::Error,
        report_assert_type_failure: DiagnosticLevel::Error,
        report_assignment_type: DiagnosticLevel::Error,
        report_attribute_access_issue: DiagnosticLevel::Error,
        report_call_issue: DiagnosticLevel::Error,
        report_inconsistent_overload: DiagnosticLevel::Error,
        report_index_issue: DiagnosticLevel::Error,
        report_invalid_type_arguments: DiagnosticLevel::Error,
        report_no_overload_implementation: DiagnosticLevel::Error,
        report_operator_issue: DiagnosticLevel::Error,
        report_optional_subscript: DiagnosticLevel::Error,
        report_optional_member_access: DiagnosticLevel::Error,
        report_optional_call: DiagnosticLevel::Error,
        report_optional_iterable: DiagnosticLevel::Error,
        report_optional_context_manager: DiagnosticLevel::Error,
        report_optional_operand: DiagnosticLevel::Error,
        report_redeclaration: DiagnosticLevel::Error,
        report_return_type: DiagnosticLevel::Error,
        report_typed_dict_not_required_access: DiagnosticLevel::Error,
        report_untyped_function_decorator: DiagnosticLevel::None,
        report_untyped_class_decorator: DiagnosticLevel::None,
        report_untyped_base_class: DiagnosticLevel::None,
        report_untyped_named_tuple: DiagnosticLevel::None,
        report_private_usage: DiagnosticLevel::None,
        report_type_comment_usage: DiagnosticLevel::None,
        report_private_import_usage: DiagnosticLevel::Error,
        report_constant_redefinition: DiagnosticLevel::None,
        report_deprecated: DiagnosticLevel::None,
        report_incompatible_method_override: DiagnosticLevel::Error,
        report_incompatible_variable_override: DiagnosticLevel::Error,
        report_inconsistent_constructor: DiagnosticLevel::None,
        report_overlapping_overload: DiagnosticLevel::Error,
        report_possibly_unbound_variable: DiagnosticLevel::Error,
        report_missing_super_call: DiagnosticLevel::None,
        report_uninitialized_instance_variable: DiagnosticLevel::None,
        report_invalid_string_escape_sequence: DiagnosticLevel::Warning,
        report_unknown_parameter_type: DiagnosticLevel::None,
        report_unknown_argument_type: DiagnosticLevel::None,
        report_unknown_lambda_type: DiagnosticLevel::None,
        report_unknown_variable_type: DiagnosticLevel::None,
        report_unknown_member_type: DiagnosticLevel::None,
        report_missing_parameter_type: DiagnosticLevel::None,
        report_missing_type_argument: DiagnosticLevel::None,
        report_invalid_type_var_use: DiagnosticLevel::Warning,
        report_call_in_default_initializer: DiagnosticLevel::None,
        report_unnecessary_is_instance: DiagnosticLevel::None,
        report_unnecessary_cast: DiagnosticLevel::None,
        report_unnecessary_comparison: DiagnosticLevel::None,
        report_unnecessary_contains: DiagnosticLevel::None,
        report_assert_always_true: DiagnosticLevel::Warning,
        report_self_cls_parameter_name: DiagnosticLevel::Warning,
        report_implicit_string_concatenation: DiagnosticLevel::None,
        report_undefined_variable: DiagnosticLevel::Error,
        report_unbound_variable: DiagnosticLevel::Error,
        report_unhashable: DiagnosticLevel::Error,
        report_invalid_stub_statement: DiagnosticLevel::None,
        report_incomplete_stub: DiagnosticLevel::None,
        report_unsupported_dunder_all: DiagnosticLevel::Warning,
        report_unused_call_result: DiagnosticLevel::None,
        report_unused_coroutine: DiagnosticLevel::Error,
        report_unused_except: DiagnosticLevel::Error,
        report_unused_expression: DiagnosticLevel::Warning,
        report_unnecessary_type_ignore_comment: DiagnosticLevel::None,
        report_match_not_exhaustive: DiagnosticLevel::None,
        report_unreachable: DiagnosticLevel::None,
        report_implicit_override: DiagnosticLevel::None,
    }
}

// common/configOptions.ts:837
pub fn get_strict_diagnostic_rule_set() -> DiagnosticRuleSet {
    DiagnosticRuleSet {
        print_unknown_as_any: false,
        omit_type_args_if_unknown: false,
        omit_unannotated_param_type: false,
        omit_conditional_constraint: false,
        pep604_printing: true,
        strict_list_inference: true,
        strict_set_inference: true,
        strict_dictionary_inference: true,
        analyze_unannotated_functions: true,
        strict_parameter_none_value: true,
        enable_experimental_features: false,
        enable_type_ignore_comments: true,
        enable_reachability_analysis: true,
        deprecate_typing_aliases: false,
        disable_bytes_type_promotions: true,
        report_general_type_issues: DiagnosticLevel::Error,
        report_property_type_mismatch: DiagnosticLevel::None,
        report_function_member_access: DiagnosticLevel::Error,
        report_missing_imports: DiagnosticLevel::Error,
        report_missing_module_source: DiagnosticLevel::Warning,
        report_invalid_type_form: DiagnosticLevel::Error,
        report_missing_type_stubs: DiagnosticLevel::Error,
        report_import_cycles: DiagnosticLevel::None,
        report_unused_import: DiagnosticLevel::Error,
        report_unused_class: DiagnosticLevel::Error,
        report_unused_function: DiagnosticLevel::Error,
        report_unused_variable: DiagnosticLevel::Error,
        report_duplicate_import: DiagnosticLevel::Error,
        report_wildcard_import_from_library: DiagnosticLevel::Error,
        report_abstract_usage: DiagnosticLevel::Error,
        report_argument_type: DiagnosticLevel::Error,
        report_assert_type_failure: DiagnosticLevel::Error,
        report_assignment_type: DiagnosticLevel::Error,
        report_attribute_access_issue: DiagnosticLevel::Error,
        report_call_issue: DiagnosticLevel::Error,
        report_inconsistent_overload: DiagnosticLevel::Error,
        report_index_issue: DiagnosticLevel::Error,
        report_invalid_type_arguments: DiagnosticLevel::Error,
        report_no_overload_implementation: DiagnosticLevel::Error,
        report_operator_issue: DiagnosticLevel::Error,
        report_optional_subscript: DiagnosticLevel::Error,
        report_optional_member_access: DiagnosticLevel::Error,
        report_optional_call: DiagnosticLevel::Error,
        report_optional_iterable: DiagnosticLevel::Error,
        report_optional_context_manager: DiagnosticLevel::Error,
        report_optional_operand: DiagnosticLevel::Error,
        report_redeclaration: DiagnosticLevel::Error,
        report_return_type: DiagnosticLevel::Error,
        report_typed_dict_not_required_access: DiagnosticLevel::Error,
        report_untyped_function_decorator: DiagnosticLevel::Error,
        report_untyped_class_decorator: DiagnosticLevel::Error,
        report_untyped_base_class: DiagnosticLevel::Error,
        report_untyped_named_tuple: DiagnosticLevel::Error,
        report_private_usage: DiagnosticLevel::Error,
        report_type_comment_usage: DiagnosticLevel::Error,
        report_private_import_usage: DiagnosticLevel::Error,
        report_constant_redefinition: DiagnosticLevel::Error,
        report_deprecated: DiagnosticLevel::Error,
        report_incompatible_method_override: DiagnosticLevel::Error,
        report_incompatible_variable_override: DiagnosticLevel::Error,
        report_inconsistent_constructor: DiagnosticLevel::Error,
        report_overlapping_overload: DiagnosticLevel::Error,
        report_possibly_unbound_variable: DiagnosticLevel::Error,
        report_missing_super_call: DiagnosticLevel::None,
        report_uninitialized_instance_variable: DiagnosticLevel::None,
        report_invalid_string_escape_sequence: DiagnosticLevel::Error,
        report_unknown_parameter_type: DiagnosticLevel::Error,
        report_unknown_argument_type: DiagnosticLevel::Error,
        report_unknown_lambda_type: DiagnosticLevel::Error,
        report_unknown_variable_type: DiagnosticLevel::Error,
        report_unknown_member_type: DiagnosticLevel::Error,
        report_missing_parameter_type: DiagnosticLevel::Error,
        report_missing_type_argument: DiagnosticLevel::Error,
        report_invalid_type_var_use: DiagnosticLevel::Error,
        report_call_in_default_initializer: DiagnosticLevel::None,
        report_unnecessary_is_instance: DiagnosticLevel::Error,
        report_unnecessary_cast: DiagnosticLevel::Error,
        report_unnecessary_comparison: DiagnosticLevel::Error,
        report_unnecessary_contains: DiagnosticLevel::Error,
        report_assert_always_true: DiagnosticLevel::Error,
        report_self_cls_parameter_name: DiagnosticLevel::Error,
        report_implicit_string_concatenation: DiagnosticLevel::None,
        report_undefined_variable: DiagnosticLevel::Error,
        report_unbound_variable: DiagnosticLevel::Error,
        report_unhashable: DiagnosticLevel::Error,
        report_invalid_stub_statement: DiagnosticLevel::Error,
        report_incomplete_stub: DiagnosticLevel::Error,
        report_unsupported_dunder_all: DiagnosticLevel::Error,
        report_unused_call_result: DiagnosticLevel::None,
        report_unused_coroutine: DiagnosticLevel::Error,
        report_unused_except: DiagnosticLevel::Error,
        report_unused_expression: DiagnosticLevel::Error,
        report_unnecessary_type_ignore_comment: DiagnosticLevel::None,
        report_match_not_exhaustive: DiagnosticLevel::Error,
        report_unreachable: DiagnosticLevel::None,
        report_implicit_override: DiagnosticLevel::None,
    }
}

// common/configOptions.ts:940
pub fn match_file_specs(config_options: &ConfigOptions, uri: Uri, is_file: bool) -> bool {
    for include_spec in &config_options.include {
        if FileSpec::match_include_file_spec(
            &include_spec.reg_exp,
            &config_options.exclude,
            uri,
            is_file,
        ) {
            return true;
        }
    }

    false
}

/// A `defineConstant` value.
#[derive(Clone, Debug, PartialEq, Eq)]
#[derive(jsrt::Trace)]
pub enum DefineConstantValue {
    Bool(bool),
    String(String),
}

/// `'strict' | 'basic' | 'off' | 'standard'`, held as the string pyright
/// stores (it casts any `typeCheckingMode` string without checking it).
pub type TypeCheckingMode = String;

// common/configOptions.ts:952
#[derive(Clone, Debug)]
#[derive(jsrt::Trace)]
pub struct ConfigOptions {
    pub project_root: Uri,
    pub python_path: Option<Uri>,
    pub python_environment_name: Option<String>,
    pub typeshed_path: Option<Uri>,
    pub stub_path: Option<Uri>,
    pub include: Vec<FileSpec>,
    pub exclude: Vec<FileSpec>,
    pub auto_exclude_venv: Option<bool>,
    pub user_specified_excludes: bool,
    pub ignore: Vec<FileSpec>,
    pub strict: Vec<FileSpec>,
    pub define_constant: JsMap<String, DefineConstantValue>,
    pub verbose_output: Option<bool>,
    pub check_only_open_files: Option<bool>,
    pub use_library_code_for_types: Option<bool>,
    pub auto_import_completions: bool,
    pub indexing: bool,
    pub log_type_evaluation_time: bool,
    pub type_evaluation_time_threshold: f64,
    pub initialized_from_json: bool,
    pub disable_tagged_hints: bool,
    pub diagnostic_rule_set: DiagnosticRuleSet,
    pub task_list_tokens: Option<Vec<TaskListToken>>,
    pub execution_environments: Vec<P<ExecutionEnvironment>>,
    pub venv_path: Option<Uri>,
    pub venv: Option<String>,
    pub default_python_version: Option<PythonVersion>,
    pub default_python_platform: Option<String>,
    pub default_extra_paths: Option<Vec<Uri>>,
    pub extra_path_glob_file_specs: Vec<String>,
    pub skip_native_libraries: Option<bool>,
    pub internal_test_mode: Option<bool>,
    pub index_generation_mode: Option<bool>,
    pub evaluate_unknown_imports_as_any: Option<bool>,
    pub function_signature_display: SignatureDisplayType,
    pub config_file_source: Option<Uri>,
    pub effective_type_checking_mode: Option<TypeCheckingMode>,
}

impl ConfigOptions {
    // common/configOptions.ts:1099
    pub fn new(project_root: Uri) -> ConfigOptions {
        ConfigOptions {
            project_root,
            python_path: None,
            python_environment_name: None,
            typeshed_path: None,
            stub_path: None,
            include: Vec::new(),
            exclude: Vec::new(),
            auto_exclude_venv: None,
            user_specified_excludes: false,
            ignore: Vec::new(),
            strict: Vec::new(),
            define_constant: JsMap::default(),
            verbose_output: None,
            check_only_open_files: None,
            use_library_code_for_types: None,
            auto_import_completions: true,
            indexing: false,
            log_type_evaluation_time: false,
            type_evaluation_time_threshold: 50.0,
            initialized_from_json: false,
            disable_tagged_hints: false,
            diagnostic_rule_set: ConfigOptions::get_diagnostic_rule_set(None),
            task_list_tokens: None,
            execution_environments: Vec::new(),
            venv_path: None,
            venv: None,
            default_python_version: None,
            default_python_platform: None,
            default_extra_paths: None,
            extra_path_glob_file_specs: Vec::new(),
            skip_native_libraries: None,
            internal_test_mode: None,
            index_generation_mode: None,
            evaluate_unknown_imports_as_any: None,
            function_signature_display: SignatureDisplayType::formatted,
            config_file_source: None,
            effective_type_checking_mode: Some("standard".to_string()),
        }
    }

    // common/configOptions.ts:1105
    pub fn get_diagnostic_rule_set(type_checking_mode: Option<&str>) -> DiagnosticRuleSet {
        match type_checking_mode {
            Some("strict") => get_strict_diagnostic_rule_set(),
            Some("basic") => get_basic_diagnostic_rule_set(),
            Some("off") => get_off_diagnostic_rule_set(),
            _ => get_standard_diagnostic_rule_set(),
        }
    }

    // common/configOptions.ts:1121
    pub fn get_default_exec_environment(&self) -> P<ExecutionEnvironment> {
        P::new(ExecutionEnvironment::new(
            self.get_environment_name(),
            self.project_root,
            &self.diagnostic_rule_set,
            self.default_python_version,
            self.default_python_platform.clone(),
            self.default_extra_paths.as_deref(),
            self.skip_native_libraries.unwrap_or(false),
        ))
    }

    // common/configOptions.ts:1137
    pub fn find_exec_environment(&self, file: Uri) -> P<ExecutionEnvironment> {
        self.execution_environments
            .iter()
            .copied()
            .find(|env| {
                let env_root = match env.root {
                    Some(root) => root,
                    None => self.project_root.resolve_paths(&[""]),
                };
                file.starts_with(Some(env_root))
            })
            .unwrap_or_else(|| self.get_default_exec_environment())
    }

    // common/configOptions.ts:1146
    pub fn get_execution_environments(&self) -> Vec<P<ExecutionEnvironment>> {
        if !self.execution_environments.is_empty() {
            return self.execution_environments.clone();
        }

        vec![self.get_default_exec_environment()]
    }

    // common/configOptions.ts:1154
    pub fn initialize_type_checking_mode(
        &mut self,
        type_checking_mode: Option<&str>,
        severity_overrides: Option<&JsMap<String, DiagnosticOverride>>,
    ) {
        self.diagnostic_rule_set = ConfigOptions::get_diagnostic_rule_set(type_checking_mode);
        self.effective_type_checking_mode = type_checking_mode.map(str::to_string);

        if let Some(severity_overrides) = severity_overrides {
            self.apply_diagnostic_overrides(Some(severity_overrides));
        }
    }

    // common/configOptions.ts:1167
    pub fn initialize_from_json(
        &mut self,
        config_obj: &Value,
        config_dir_uri: Uri,
        console: &dyn ConsoleInterface,
        fs: &dyn FileSystem,
        _host: &dyn Host,
    ) {
        self.initialized_from_json = true;
        let config_obj_keys: Vec<String> = match config_obj {
            Value::Object(map) => map.keys().cloned().collect(),
            // `Object.getOwnPropertyNames` of an array lists its indices and `length`.
            Value::Array(items) => (0..items.len())
                .map(|i| i.to_string())
                .chain(["length".to_string()])
                .collect(),
            _ => Vec::new(),
        };
        let mut unused_config_keys: JsSet<String> = config_obj_keys.into_iter().collect();
        let get = |key: &str| config_obj.get(key);

        // Read the "include", "exclude", "ignore" and "strict" entries.
        for (key, allow_absolute) in [
            ("include", false),
            ("exclude", false),
            ("ignore", true),
            ("strict", false),
        ] {
            let Some(value) = get(key) else {
                continue;
            };
            unused_config_keys.shift_remove(key);
            let Value::Array(files_list) = value else {
                console.error(&format!("Config \"{key}\" entry must contain an array."));
                continue;
            };
            let mut specs = Vec::new();
            for (index, file_spec) in files_list.iter().enumerate() {
                match file_spec {
                    Value::String(file_spec) => {
                        if !allow_absolute && jsrt::node_path::is_absolute(file_spec) {
                            console.error(&format!(
                                "Ignoring path \"{file_spec}\" in \"{key}\" array because it is not relative."
                            ));
                        } else {
                            specs.push(get_file_spec(config_dir_uri, file_spec));
                        }
                    }
                    _ => console.error(&format!(
                        "Index {index} of \"{key}\" array should be a string."
                    )),
                }
            }
            match key {
                "include" => self.include = specs,
                "exclude" => self.exclude = specs,
                "ignore" => self.ignore = specs,
                _ => self.strict = specs,
            }
        }

        // If there is a "typeCheckingMode", it can override the provided setting.
        if let Some(mode) = get("typeCheckingMode") {
            unused_config_keys.shift_remove("typeCheckingMode");
            match mode.as_str() {
                Some(mode @ ("off" | "basic" | "standard" | "strict")) => {
                    self.initialize_type_checking_mode(Some(mode), None);
                }
                _ => console.error("Config \"typeCheckingMode\" entry must contain \"off\", \"basic\", \"standard\", or \"strict\"."),
            }
        }

        if let Some(value) = get("useLibraryCodeForTypes") {
            unused_config_keys.shift_remove("useLibraryCodeForTypes");
            match value {
                Value::Bool(b) => self.use_library_code_for_types = Some(*b),
                _ => {
                    console.error("Config \"useLibraryCodeForTypes\" entry must be true or false.")
                }
            }
        }

        // Apply overrides from the config file for the boolean rules.
        let mut config_rule_set = self.diagnostic_rule_set.clone();
        for rule_name in get_boolean_diagnostic_rules(true) {
            unused_config_keys.shift_remove(rule_name.as_str());
            let default_value = config_rule_set.get_bool(rule_name).unwrap();
            config_rule_set.set_bool(
                rule_name,
                convert_boolean(get(rule_name.as_str()), rule_name.as_str(), default_value),
            );
        }

        // Apply overrides from the config file for the diagnostic level rules.
        for rule_name in get_diag_level_diagnostic_rules() {
            unused_config_keys.shift_remove(rule_name.as_str());
            let default_value = config_rule_set.get_level(rule_name).unwrap();
            config_rule_set.set_level(
                rule_name,
                convert_diagnostic_level(
                    get(rule_name.as_str()),
                    rule_name.as_str(),
                    default_value,
                ),
            );
        }
        self.diagnostic_rule_set = config_rule_set;

        // Read the "venvPath".
        if let Some(value) = get("venvPath") {
            unused_config_keys.shift_remove("venvPath");
            match value {
                Value::String(s) => self.venv_path = Some(config_dir_uri.resolve_paths(&[s])),
                _ => console.error("Config \"venvPath\" field must contain a string."),
            }
        }

        // Read the "venv" name.
        if let Some(value) = get("venv") {
            unused_config_keys.shift_remove("venv");
            match value {
                Value::String(s) => self.venv = Some(s.clone()),
                _ => console.error("Config \"venv\" field must contain a string."),
            }
        }

        // Read the config "extraPaths".
        if let Some(value) = get("extraPaths") {
            unused_config_keys.shift_remove("extraPaths");
            match value {
                Value::Array(path_list) => {
                    let mut valid_paths: Vec<String> = Vec::new();
                    for (path_index, path) in path_list.iter().enumerate() {
                        match path {
                            Value::String(s) => valid_paths.push(s.clone()),
                            _ => console.error(&format!(
                                "Config \"extraPaths\" field {path_index} must be a string."
                            )),
                        }
                    }

                    self.default_extra_paths = Some(expand_extra_paths(
                        fs,
                        config_dir_uri,
                        &valid_paths,
                        Some(console),
                    ));
                    self.record_extra_path_glob_file_specs(config_dir_uri, &valid_paths);
                }
                _ => console.error("Config \"extraPaths\" field must contain an array."),
            }
        }

        // Read the default "pythonVersion".
        if let Some(value) = get("pythonVersion") {
            unused_config_keys.shift_remove("pythonVersion");
            match value {
                Value::String(s) => match PythonVersion::from_string(s) {
                    Some(version) => self.default_python_version = Some(version),
                    None => console
                        .error("Config \"pythonVersion\" field contains unsupported version."),
                },
                _ => console.error("Config \"pythonVersion\" field must contain a string."),
            }
        }

        // Read the default "pythonPlatform".
        if let Some(value) = get("pythonPlatform") {
            unused_config_keys.shift_remove("pythonPlatform");
            match value {
                Value::String(s) => self.default_python_platform = Some(s.clone()),
                _ => console.error("Config \"pythonPlatform\" field must contain a string."),
            }
        }

        // Read the skipNativeLibraries flag.
        if let Some(value) = get("skipNativeLibraries") {
            unused_config_keys.shift_remove("skipNativeLibraries");
            match value {
                Value::Bool(b) => self.skip_native_libraries = Some(*b),
                _ => console.error("Config \"skipNativeLibraries\" field must contain a boolean."),
            }
        }

        // Read the "typeshedPath" setting.
        if let Some(value) = get("typeshedPath") {
            unused_config_keys.shift_remove("typeshedPath");
            match value {
                Value::String(s) => {
                    self.typeshed_path = if !s.is_empty() {
                        Some(config_dir_uri.resolve_paths(&[s]))
                    } else {
                        None
                    };
                }
                _ => console.error("Config \"typeshedPath\" field must contain a string."),
            }
        }

        // Keep this for backward compatibility
        if let Some(value) = get("typingsPath") {
            unused_config_keys.shift_remove("typingsPath");
            match value {
                Value::String(s) => {
                    console.error(
                        "Config \"typingsPath\" is now deprecated. Please, use stubPath instead.",
                    );
                    self.stub_path = Some(config_dir_uri.resolve_paths(&[s]));
                }
                _ => console.error("Config \"typingsPath\" field must contain a string."),
            }
        }

        if let Some(value) = get("stubPath") {
            unused_config_keys.shift_remove("stubPath");
            match value {
                Value::String(s) => self.stub_path = Some(config_dir_uri.resolve_paths(&[s])),
                _ => console.error("Config \"stubPath\" field must contain a string."),
            }
        }

        // Read the "verboseOutput" setting.
        if let Some(value) = get("verboseOutput") {
            unused_config_keys.shift_remove("verboseOutput");
            match value {
                Value::Bool(b) => self.verbose_output = Some(*b),
                _ => console.error("Config \"verboseOutput\" field must be true or false."),
            }
        }

        // Read the "defineConstant" setting.
        if let Some(value) = get("defineConstant") {
            unused_config_keys.shift_remove("defineConstant");
            match value {
                // `typeof null === 'object'`, and `Object.getOwnPropertyNames(null)` throws:
                // pyright crashes on `"defineConstant": null`.
                Value::Object(map) => {
                    for (key, value) in map {
                        match value {
                            Value::Bool(b) => {
                                self.define_constant.insert(key.clone(), DefineConstantValue::Bool(*b));
                            }
                            Value::String(s) => {
                                self.define_constant.insert(key.clone(), DefineConstantValue::String(s.clone()));
                            }
                            _ => console.error(&format!(
                                "Defined constant \"{key}\" must be associated with a boolean or string value."
                            )),
                        }
                    }
                }
                _ => console.error(
                    "Config \"defineConstant\" field must contain a map indexed by constant names.",
                ),
            }
        }

        // Read the "useLibraryCodeForTypes" setting.
        if let Some(value) = get("useLibraryCodeForTypes") {
            unused_config_keys.shift_remove("useLibraryCodeForTypes");
            match value {
                Value::Bool(b) => self.use_library_code_for_types = Some(*b),
                _ => {
                    console.error("Config \"useLibraryCodeForTypes\" field must be true or false.")
                }
            }
        }

        for (key, field) in [
            ("autoImportCompletions", 0),
            ("indexing", 1),
            ("logTypeEvaluationTime", 2),
        ] {
            if let Some(value) = get(key) {
                unused_config_keys.shift_remove(key);
                match value {
                    Value::Bool(b) => match field {
                        0 => self.auto_import_completions = *b,
                        1 => self.indexing = *b,
                        _ => self.log_type_evaluation_time = *b,
                    },
                    _ => console.error(&format!("Config \"{key}\" field must be true or false.")),
                }
            }
        }

        // Read the "typeEvaluationTimeThreshold" setting.
        if let Some(value) = get("typeEvaluationTimeThreshold") {
            unused_config_keys.shift_remove("typeEvaluationTimeThreshold");
            match value.as_f64() {
                Some(n) => self.type_evaluation_time_threshold = n,
                None => {
                    console.error("Config \"typeEvaluationTimeThreshold\" field must be a number.")
                }
            }
        }

        // Read the "functionSignatureDisplay" setting.
        if let Some(value) = get("functionSignatureDisplay") {
            unused_config_keys.shift_remove("functionSignatureDisplay");
            match value.as_str() {
                Some("compact") => self.function_signature_display = SignatureDisplayType::compact,
                Some("formatted") => {
                    self.function_signature_display = SignatureDisplayType::formatted
                }
                Some(_) => {}
                None => console
                    .error("Config \"functionSignatureDisplay\" field must be true or false."),
            }
        }

        unused_config_keys.shift_remove("executionEnvironments");
        unused_config_keys.shift_remove("extends");

        for unknown_key in unused_config_keys {
            console.error(&format!(
                "Config contains unrecognized setting \"{unknown_key}\"."
            ));
        }
    }

    // common/configOptions.ts:1527
    /// Reports through Node's global `console.error`, not the service console.
    pub fn resolve_extends(config_obj: &Value, config_dir_uri: Uri) -> Option<Uri> {
        if let Some(extends) = config_obj.get("extends") {
            match extends {
                Value::String(s) => return Some(config_dir_uri.resolve_paths(&[s])),
                _ => node_stderr("Config \"extends\" field must contain a string."),
            }
        }

        None
    }

    // common/configOptions.ts:1539
    pub fn ensure_default_python_platform(
        &mut self,
        host: &dyn Host,
        console: &dyn ConsoleInterface,
    ) {
        if self.default_python_platform.is_some() {
            return;
        }

        self.default_python_platform = host
            .get_python_platform(None)
            .map(|p| p.as_str().to_string());
        if let Some(platform) = &self.default_python_platform {
            console.log(&format!("Assuming Python platform {platform}"));
        }
    }

    // common/configOptions.ts:1552
    pub fn ensure_default_python_version(
        &mut self,
        host: &dyn Host,
        console: &dyn ConsoleInterface,
    ) {
        if self.default_python_version.is_some() {
            return;
        }

        let import_logger = ImportLogger::new();
        self.default_python_version =
            host.get_python_version(self.python_path, Some(&import_logger));
        if let Some(version) = &self.default_python_version {
            console.info(&format!(
                "Assuming Python version {}",
                PythonVersion::to_string(version)
            ));
        }

        for log in import_logger.get_logs() {
            console.info(&log);
        }
    }

    // common/configOptions.ts:1570
    pub fn ensure_default_extra_paths(
        &mut self,
        fs: &dyn FileSystem,
        auto_search_paths: bool,
        extra_paths: Option<&[String]>,
    ) {
        let mut paths: Vec<Uri> = Vec::new();

        if auto_search_paths {
            let src_path = self.project_root.resolve_paths(&[path_consts::SRC]);
            if fs.exists_sync(src_path) && !fs.exists_sync(src_path.resolve_paths(&["__init__.py"]))
            {
                paths.push(fs.real_case_path(src_path));
            }
        }

        if let Some(extra_paths) = extra_paths.filter(|p| !p.is_empty()) {
            self.record_extra_path_glob_file_specs(self.project_root, extra_paths);

            let mut seen: JsSet<String> = JsSet::default();
            for expanded_path in expand_extra_paths(fs, self.project_root, extra_paths, None) {
                let real_cased_path = fs.real_case_path(expanded_path);
                if seen.contains(real_cased_path.key()) {
                    continue;
                }
                seen.insert(real_cased_path.key().to_string());

                paths.push(real_cased_path);
                if is_directory(fs, expanded_path) {
                    paths.extend(get_paths_from_pth_files(fs, expanded_path));
                }
            }
        }

        if !paths.is_empty() {
            self.default_extra_paths = Some(paths);
        }
    }

    // common/configOptions.ts:1608
    pub fn apply_diagnostic_overrides(
        &mut self,
        diagnostic_overrides: Option<&JsMap<String, DiagnosticOverride>>,
    ) {
        let Some(diagnostic_overrides) = diagnostic_overrides else {
            return;
        };

        for rule_name in get_diag_level_diagnostic_rules() {
            if let Some(DiagnosticOverride::Severity(severity)) =
                diagnostic_overrides.get(rule_name.as_str())
            {
                self.diagnostic_rule_set.set_level(rule_name, *severity);
            }
        }

        for rule_name in get_boolean_diagnostic_rules(true) {
            if let Some(DiagnosticOverride::Bool(value)) =
                diagnostic_overrides.get(rule_name.as_str())
            {
                self.diagnostic_rule_set.set_bool(rule_name, *value);
            }
        }
    }

    // common/configOptions.ts:1630
    pub fn setup_execution_environments(
        &mut self,
        config_obj: &Value,
        config_dir_uri: Uri,
        console: &dyn ConsoleInterface,
        fs: &dyn FileSystem,
    ) {
        if let Some(envs) = config_obj.get("executionEnvironments") {
            let Value::Array(exec_environments) = envs else {
                console.error("Config \"executionEnvironments\" field must contain an array.");
                return;
            };
            self.execution_environments = Vec::new();

            for (index, env) in exec_environments.iter().enumerate() {
                let rule_set = self.diagnostic_rule_set.clone();
                let default_extra_paths = self.default_extra_paths.clone().unwrap_or_default();
                let exec_env = self.init_execution_environment_from_json(
                    env,
                    config_dir_uri,
                    index,
                    console,
                    &rule_set,
                    self.default_python_version,
                    self.default_python_platform.clone(),
                    &default_extra_paths,
                    fs,
                );

                if let Some(exec_env) = exec_env {
                    self.execution_environments.push(P::new(exec_env));
                }
            }
        }
    }

    // common/configOptions.ts:1662
    fn get_environment_name(&self) -> String {
        if let Some(name) = self
            .python_environment_name
            .as_ref()
            .filter(|n| !n.is_empty())
        {
            return name.clone();
        }
        if let Some(path) = self.python_path {
            let s = path.to_uri_string();
            if !s.is_empty() {
                return s;
            }
        }
        "python".to_string()
    }

    // common/configOptions.ts:1684
    fn record_extra_path_glob_file_specs(&mut self, base_uri: Uri, entries: &[String]) {
        for entry in entries {
            if !contains_wildcard(entry) {
                continue;
            }
            let spec = base_uri.resolve_paths(&[entry]).get_file_path().to_string();
            if !self.extra_path_glob_file_specs.contains(&spec) {
                self.extra_path_glob_file_specs.push(spec);
            }
        }
    }

    // common/configOptions.ts:1711
    fn init_execution_environment_from_json(
        &mut self,
        env_obj: &Value,
        config_dir_uri: Uri,
        index: usize,
        console: &dyn ConsoleInterface,
        config_diagnostic_rule_set: &DiagnosticRuleSet,
        config_python_version: Option<PythonVersion>,
        config_python_platform: Option<String>,
        config_extra_paths: &[Uri],
        fs: &dyn FileSystem,
    ) -> Option<ExecutionEnvironment> {
        // `envObj.root` on `null` or `undefined` throws; pyright catches it.
        if env_obj.is_null() {
            console.error(&format!(
                "Config executionEnvironments index {index} is not accessible."
            ));
            return None;
        }
        let env_obj_keys: Vec<String> = match env_obj {
            Value::Object(map) => map.keys().cloned().collect(),
            Value::Array(items) => (0..items.len())
                .map(|i| i.to_string())
                .chain(["length".to_string()])
                .collect(),
            _ => Vec::new(),
        };
        let mut unused_env_keys: JsSet<String> = env_obj_keys.into_iter().collect();
        let get = |key: &str| env_obj.get(key);
        let truthy = |v: Option<&Value>| v.is_some_and(is_truthy);

        let mut new_exec_env = ExecutionEnvironment::new(
            self.get_environment_name(),
            config_dir_uri,
            config_diagnostic_rule_set,
            config_python_version,
            config_python_platform,
            Some(config_extra_paths),
            false,
        );

        // Validate the root.
        unused_env_keys.shift_remove("root");
        match get("root") {
            Some(Value::String(root)) if !root.is_empty() => {
                new_exec_env.root = Some(config_dir_uri.resolve_paths(&[root]))
            }
            _ => console.error(&format!(
                "Config executionEnvironments index {index}: missing root value."
            )),
        }

        // Validate the extraPaths.
        unused_env_keys.shift_remove("extraPaths");
        if truthy(get("extraPaths")) {
            match get("extraPaths") {
                Some(Value::Array(path_list)) => {
                    let mut valid_paths: Vec<String> = Vec::new();
                    for (path_index, path) in path_list.iter().enumerate() {
                        match path {
                            Value::String(s) => valid_paths.push(s.clone()),
                            _ => console.error(&format!(
                                "Config executionEnvironments index {index}: extraPaths field {path_index} must be a string."
                            )),
                        }
                    }

                    new_exec_env.extra_paths = expand_extra_paths(fs, config_dir_uri, &valid_paths, Some(console));
                    self.record_extra_path_glob_file_specs(config_dir_uri, &valid_paths);
                }
                _ => console.error(&format!(
                    "Config executionEnvironments index {index}: extraPaths field must contain an array."
                )),
            }
        }

        // Validate the pythonVersion.
        unused_env_keys.shift_remove("pythonVersion");
        if truthy(get("pythonVersion")) {
            match get("pythonVersion") {
                Some(Value::String(s)) => match PythonVersion::from_string(s) {
                    Some(version) => new_exec_env.python_version = version,
                    None => console.warn(&format!(
                        "Config executionEnvironments index {index} contains unsupported pythonVersion."
                    )),
                },
                _ => console.error(&format!("Config executionEnvironments index {index} pythonVersion must be a string.")),
            }
        }

        // Validate the pythonPlatform.
        unused_env_keys.shift_remove("pythonPlatform");
        if truthy(get("pythonPlatform")) {
            match get("pythonPlatform") {
                Some(Value::String(s)) => new_exec_env.python_platform = Some(s.clone()),
                _ => console.error(&format!(
                    "Config executionEnvironments index {index} pythonPlatform must be a string."
                )),
            }
        }

        // Validate the name.
        unused_env_keys.shift_remove("name");
        if truthy(get("name")) {
            match get("name") {
                Some(Value::String(s)) => new_exec_env.name = s.clone(),
                _ => console.error(&format!(
                    "Config executionEnvironments index {index} name must be a string."
                )),
            }
        }

        for rule_name in get_boolean_diagnostic_rules(true) {
            unused_env_keys.shift_remove(rule_name.as_str());
            let default_value = new_exec_env
                .diagnostic_rule_set
                .get_bool(rule_name)
                .unwrap();
            new_exec_env.diagnostic_rule_set.set_bool(
                rule_name,
                convert_boolean(get(rule_name.as_str()), rule_name.as_str(), default_value),
            );
        }

        for rule_name in get_diag_level_diagnostic_rules() {
            unused_env_keys.shift_remove(rule_name.as_str());
            let default_value = new_exec_env
                .diagnostic_rule_set
                .get_level(rule_name)
                .unwrap();
            new_exec_env.diagnostic_rule_set.set_level(
                rule_name,
                convert_diagnostic_level(
                    get(rule_name.as_str()),
                    rule_name.as_str(),
                    default_value,
                ),
            );
        }

        for unknown_key in unused_env_keys {
            console.error(&format!("Config executionEnvironments index {index}: unrecognized setting \"{unknown_key}\"."));
        }

        Some(new_exec_env)
    }
}

/// JS truthiness of a JSON value.
pub fn is_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0 && !f.is_nan()),
        Value::String(s) => !s.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

// common/configOptions.ts:1666
/// Reports through Node's global `console.log`, which writes to stdout even
/// under `--outputjson`.
fn convert_boolean(value: Option<&Value>, field_name: &str, default_value: bool) -> bool {
    match value {
        None => default_value,
        Some(Value::Bool(b)) => *b,
        Some(_) => {
            node_stdout(&format!(
                "Config \"{field_name}\" entry must be true or false."
            ));
            default_value
        }
    }
}

// common/configOptions.ts:1696
/// Reports through Node's global `console.log`, which writes to stdout.
fn convert_diagnostic_level(
    value: Option<&Value>,
    field_name: &str,
    default_value: DiagnosticLevel,
) -> DiagnosticLevel {
    match value {
        None => default_value,
        Some(Value::Bool(b)) => {
            if *b {
                DiagnosticLevel::Error
            } else {
                DiagnosticLevel::None
            }
        }
        Some(Value::String(s)) if DiagnosticLevel::from_name(s).is_some() => {
            DiagnosticLevel::from_name(s).unwrap()
        }
        Some(_) => {
            node_stdout(&format!(
                "Config \"{field_name}\" entry must be true, false, \"error\", \"warning\", \"information\" or \"none\"."
            ));
            default_value
        }
    }
}

/// A value of `DiagnosticSeverityOverridesMap` or `DiagnosticBooleanOverridesMap`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiagnosticOverride {
    Severity(DiagnosticLevel),
    Bool(bool),
}

// common/configOptions.ts:1849
/// `DiagnosticSeverityOverrides` values are the same strings as `DiagnosticLevel`.
pub fn parse_diag_level(value: &Value) -> Option<DiagnosticLevel> {
    match value {
        Value::Bool(false) => Some(DiagnosticLevel::None),
        Value::Bool(true) => Some(DiagnosticLevel::Error),
        Value::String(s) => match s.as_str() {
            "none" => Some(DiagnosticLevel::None),
            "error" => Some(DiagnosticLevel::Error),
            "warning" => Some(DiagnosticLevel::Warning),
            "information" => Some(DiagnosticLevel::Information),
            _ => None,
        },
        _ => None,
    }
}
