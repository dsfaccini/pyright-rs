//! Port of `packages/pyright-internal/src/localization/localize.ts`.
//!
//! Ships the English strings only (`package.nls.en-us.json`, copied verbatim
//! from the spec). A locale that would select another language fails loudly:
//! see `load_strings_for_locale`.

use jsrt::{FxHashMap, JsStr};
use std::cell::RefCell;

use serde_json::Value;

// localization/localize.ts:28
/// `format` takes the parameters in the order the TypeScript object literal
/// lists them: `Object.keys` order decides which `{key}` is replaced first.
#[derive(Clone, Debug)]
#[derive(jsrt::Trace)]
pub struct ParameterizedString {
    format_string: String,
}

impl ParameterizedString {
    pub fn new(format_string: String) -> ParameterizedString {
        ParameterizedString { format_string }
    }

    // localization/localize.ts:31
    /// `str.replace(new RegExp(`{${key}}`, 'g'), value.toString())` per key:
    /// `$&`, `$$`, `` $` `` and `$'` in a value are replacement patterns.
    /// Reported upstream as https://github.com/microsoft/pyright/issues/12006;
    /// follow its fix when the pin moves.
    /// Callers format numbers with JS `String(n)` before passing them.
    pub fn format(&self, params: &[(&str, &str)]) -> String {
        let mut str = self.format_string.clone();
        for (key, value) in params {
            str = str.js_replace_all(&format!("{{{key}}}"), value);
        }
        str
    }

    // localization/localize.ts:39
    pub fn get_format_string(&self) -> &str {
        &self.format_string
    }
}

// localization/localize.ts:44
const DEFAULT_LOCALE: &str = "en-us";

/// Keys of pyright's `stringMapsByLocale`.
const KNOWN_LOCALES: [&str; 16] = [
    "cs", "de", "en-us", "en", "es", "fr", "it", "ja", "ko", "pl", "pt-br", "qps-ploc", "ru", "tr",
    "zh-cn", "zh-tw",
];

const EN_US_STRINGS: &str = include_str!("package.nls.en-us.json");

struct LocalizeState {
    default_strings: Option<Value>,
    /// `default_strings` indexed by every key `getRawStringFromMap` finds a string for,
    /// which pyright lacks (`PYRIGHT_RS_LOCALIZE_INDEX`).
    default_index: Option<FxHashMap<String, String>>,
    locale_override: Option<String>,
    initialized: bool,
}

thread_local! {
    static STATE: RefCell<LocalizeState> = const {
        RefCell::new(LocalizeState { default_strings: None, default_index: None, locale_override: None, initialized: false })
    };
}

// localization/localize.ts:73
fn get_raw_string_default(key: &str) -> String {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        if !state.initialized {
            initialize(&mut state);
        }

        // The localized map is always empty or English: see `initialize`.
        let found = match localize_index_mode() {
            LocalizeIndexMode::Off => raw_string_from_key(state.default_strings.as_ref().unwrap(), key),
            mode => {
                let indexed = state.default_index.as_ref().unwrap().get(key).cloned();
                if mode == LocalizeIndexMode::Check {
                    let walked = raw_string_from_key(state.default_strings.as_ref().unwrap(), key);
                    assert!(indexed == walked, "PYRIGHT_RS_LOCALIZE_INDEX=check: {key:?} indexed {indexed:?}, walked {walked:?}");
                }
                indexed
            }
        };
        match found {
            Some(str) => str,
            None => crate::common::debug::fail(Some(&format!(
                "Missing localized string for key \"{key}\""
            ))),
        }
    })
}

fn raw_string_from_key(map: &Value, key: &str) -> Option<String> {
    let key_parts: Vec<&str> = key.split('.').collect();
    get_raw_string_from_map(map, &key_parts)
}

/// `PYRIGHT_RS_LOCALIZE_INDEX`: `on` (the default) looks a message up by its whole key in an
/// index built once from the strings, `off` walks the key's parts through the JSON maps as
/// pyright does, and `check` does both and panics when they differ.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum LocalizeIndexMode {
    Off,
    On,
    Check,
}

fn localize_index_mode() -> LocalizeIndexMode {
    static MODE: std::sync::OnceLock<LocalizeIndexMode> = std::sync::OnceLock::new();
    *MODE.get_or_init(|| match std::env::var("PYRIGHT_RS_LOCALIZE_INDEX").as_deref() {
        Err(_) | Ok("") | Ok("on") => LocalizeIndexMode::On,
        Ok("off") => LocalizeIndexMode::Off,
        Ok("check") => LocalizeIndexMode::Check,
        Ok(other) => panic!("PYRIGHT_RS_LOCALIZE_INDEX must be on, off or check, not {other:?}"),
    })
}

/// Adds to `index` the string `getRawStringFromMap` returns for each key below `prefix`. A
/// lookup steps only into truthy values, and splits its key at dots, so a name with a dot is
/// out of its reach.
fn index_strings(map: &Value, prefix: &mut String, index: &mut FxHashMap<String, String>) {
    let Value::Object(entries) = map else {
        return;
    };
    for (name, value) in entries {
        if !is_truthy(value) || name.contains('.') {
            continue;
        }
        let len = prefix.len();
        if len != 0 {
            prefix.push('.');
        }
        prefix.push_str(name);
        if let Some(found) = get_raw_string_from_map(value, &[]) {
            index.insert(prefix.clone(), found);
        }
        index_strings(value, prefix, index);
        prefix.truncate(len);
    }
}

pub fn get_raw_string(key: &str) -> String {
    get_raw_string_default(key)
}

// localization/localize.ts:103
pub fn get_raw_string_from_map(map: &Value, key_parts: &[&str]) -> Option<String> {
    let mut cur_obj = map;

    for key_part in key_parts {
        match cur_obj.get(key_part) {
            Some(next) if is_truthy(next) => cur_obj = next,
            _ => return None,
        }
    }

    match cur_obj {
        Value::String(s) => Some(s.clone()),
        other => other
            .get("message")
            .and_then(Value::as_str)
            .map(str::to_string),
    }
}

fn is_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}

// localization/localize.ts:117
fn initialize(state: &mut LocalizeState) {
    state.default_strings =
        Some(serde_json::from_str(EN_US_STRINGS).expect("package.nls.en-us.json"));
    if localize_index_mode() != LocalizeIndexMode::Off {
        let mut index = FxHashMap::default();
        index_strings(state.default_strings.as_ref().unwrap(), &mut String::new(), &mut index);
        state.default_index = Some(index);
    }
    let current_locale = get_locale_from_env(state.locale_override.as_deref());
    load_strings_for_locale(&current_locale);
    state.initialized = true;
}

// localization/localize.ts:126
pub fn set_locale_override(locale: &str) {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.initialized = false;
        state.locale_override = Some(locale.js_to_lower_case());
    });
}

// localization/localize.ts:140
pub fn get_locale_from_env(locale_override: Option<&str>) -> String {
    if let Some(locale_override) = locale_override.filter(|s| !s.is_empty()) {
        return locale_override.to_string();
    }

    if let Ok(vscode_config_string) = std::env::var("VSCODE_NLS_CONFIG") {
        if !vscode_config_string.is_empty() {
            if let Ok(config) = serde_json::from_str::<Value>(&vscode_config_string) {
                return match config.get("locale") {
                    Some(locale) if is_truthy(locale) => {
                        locale.as_str().unwrap_or(DEFAULT_LOCALE).to_string()
                    }
                    _ => DEFAULT_LOCALE.to_string(),
                };
            }
        }
    }

    let env = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty());
    let locale_string = env("LC_ALL")
        .or_else(|| env("LC_MESSAGES"))
        .or_else(|| env("LANG"))
        .or_else(|| env("LANGUAGE"));
    if let Some(locale_string) = locale_string {
        let first = locale_string.split('.').next().unwrap_or("");
        if !first.is_empty() {
            return first.to_string();
        }
    }

    DEFAULT_LOCALE.to_string()
}

// localization/localize.ts:184
/// Returns the locale key pyright would load strings for, or `None` for the
/// English default. Panics for a non-English locale: only English ships.
pub fn load_strings_for_locale(locale: &str) -> Option<&'static str> {
    if locale == DEFAULT_LOCALE {
        return None;
    }

    let found = KNOWN_LOCALES.iter().find(|l| **l == locale).or_else(|| {
        let first = locale.split('-').next().unwrap_or("");
        if first.is_empty() {
            None
        } else {
            KNOWN_LOCALES.iter().find(|l| **l == first)
        }
    });

    match found {
        None => None,
        Some(&"en") | Some(&"en-us") => None,
        Some(other) => panic!(
            "pyright-rs ships English messages only, and locale \"{locale}\" selects \"{other}\""
        ),
    }
}

// localization/localize.ts:208
#[allow(non_snake_case)]
pub mod Localizer {
    #[allow(non_snake_case)]
    pub mod Diagnostic {
        #[allow(unused_imports)]
        use super::super::{ParameterizedString, get_raw_string};

        // localization/localize.ts:210
        /// Params: type, metadataType.
        pub fn annotated_metadata_inconsistent() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.annotatedMetadataInconsistent"))
        }

        // localization/localize.ts:214
        /// Params: method.
        pub fn abstract_method_invocation() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.abstractMethodInvocation"))
        }

        // localization/localize.ts:216
        /// Params: expected, received.
        pub fn annotated_param_count_mismatch() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.annotatedParamCountMismatch"))
        }

        // localization/localize.ts:220
        pub fn annotated_type_arg_missing() -> String {
            get_raw_string("Diagnostic.annotatedTypeArgMissing")
        }

        // localization/localize.ts:221
        pub fn annotation_bytes_string() -> String {
            get_raw_string("Diagnostic.annotationBytesString")
        }

        // localization/localize.ts:222
        pub fn annotation_format_string() -> String {
            get_raw_string("Diagnostic.annotationFormatString")
        }

        // localization/localize.ts:223
        pub fn annotation_not_supported() -> String {
            get_raw_string("Diagnostic.annotationNotSupported")
        }

        // localization/localize.ts:224
        pub fn annotation_raw_string() -> String {
            get_raw_string("Diagnostic.annotationRawString")
        }

        // localization/localize.ts:225
        pub fn annotation_spans_strings() -> String {
            get_raw_string("Diagnostic.annotationSpansStrings")
        }

        // localization/localize.ts:226
        pub fn annotation_string_escape() -> String {
            get_raw_string("Diagnostic.annotationStringEscape")
        }

        // localization/localize.ts:227
        pub fn annotation_template_string() -> String {
            get_raw_string("Diagnostic.annotationTemplateString")
        }

        // localization/localize.ts:228
        /// Params: argType, paramType.
        pub fn arg_assignment() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.argAssignment"))
        }

        // localization/localize.ts:230
        /// Params: argType, paramType, functionName.
        pub fn arg_assignment_function() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.argAssignmentFunction"))
        }

        // localization/localize.ts:234
        /// Params: argType, paramType, paramName.
        pub fn arg_assignment_param() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.argAssignmentParam"))
        }

        // localization/localize.ts:238
        /// Params: argType, paramType, paramName, functionName.
        pub fn arg_assignment_param_function() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.argAssignmentParamFunction"))
        }

        // localization/localize.ts:242
        /// Params: name.
        pub fn arg_missing_for_param() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.argMissingForParam"))
        }

        // localization/localize.ts:244
        /// Params: names.
        pub fn arg_missing_for_params() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.argMissingForParams"))
        }

        // localization/localize.ts:246
        /// Params: expected.
        pub fn arg_more_positional_expected_count() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.argMorePositionalExpectedCount"))
        }

        // localization/localize.ts:248
        pub fn arg_more_positional_expected_one() -> String {
            get_raw_string("Diagnostic.argMorePositionalExpectedOne")
        }

        // localization/localize.ts:249
        pub fn arg_positional() -> String {
            get_raw_string("Diagnostic.argPositional")
        }

        // localization/localize.ts:250
        /// Params: expected.
        pub fn arg_positional_expected_count() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.argPositionalExpectedCount"))
        }

        // localization/localize.ts:252
        pub fn arg_positional_expected_one() -> String {
            get_raw_string("Diagnostic.argPositionalExpectedOne")
        }

        // localization/localize.ts:253
        pub fn arg_type_partially_unknown() -> String {
            get_raw_string("Diagnostic.argTypePartiallyUnknown")
        }

        // localization/localize.ts:254
        pub fn arg_type_unknown() -> String {
            get_raw_string("Diagnostic.argTypeUnknown")
        }

        // localization/localize.ts:255
        pub fn assert_always_true() -> String {
            get_raw_string("Diagnostic.assertAlwaysTrue")
        }

        // localization/localize.ts:256
        pub fn assert_type_args() -> String {
            get_raw_string("Diagnostic.assertTypeArgs")
        }

        // localization/localize.ts:257
        /// Params: expected, received.
        pub fn assert_type_type_mismatch() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.assertTypeTypeMismatch"))
        }

        // localization/localize.ts:261
        pub fn assignment_expr_context() -> String {
            get_raw_string("Diagnostic.assignmentExprContext")
        }

        // localization/localize.ts:262
        /// Params: name.
        pub fn assignment_expr_comprehension() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.assignmentExprComprehension"))
        }

        // localization/localize.ts:264
        pub fn assignment_expr_in_subscript() -> String {
            get_raw_string("Diagnostic.assignmentExprInSubscript")
        }

        // localization/localize.ts:265
        pub fn assignment_in_protocol() -> String {
            get_raw_string("Diagnostic.assignmentInProtocol")
        }

        // localization/localize.ts:266
        pub fn assignment_target_expr() -> String {
            get_raw_string("Diagnostic.assignmentTargetExpr")
        }

        // localization/localize.ts:267
        pub fn async_not_in_async_function() -> String {
            get_raw_string("Diagnostic.asyncNotInAsyncFunction")
        }

        // localization/localize.ts:268
        pub fn await_illegal() -> String {
            get_raw_string("Diagnostic.awaitIllegal")
        }

        // localization/localize.ts:269
        pub fn await_not_allowed() -> String {
            get_raw_string("Diagnostic.awaitNotAllowed")
        }

        // localization/localize.ts:270
        pub fn await_not_in_async() -> String {
            get_raw_string("Diagnostic.awaitNotInAsync")
        }

        // localization/localize.ts:271
        pub fn backticks_illegal() -> String {
            get_raw_string("Diagnostic.backticksIllegal")
        }

        // localization/localize.ts:272
        pub fn base_class_circular() -> String {
            get_raw_string("Diagnostic.baseClassCircular")
        }

        // localization/localize.ts:273
        /// Params: type.
        pub fn base_class_final() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.baseClassFinal"))
        }

        // localization/localize.ts:275
        /// Params: type.
        pub fn base_class_incompatible() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.baseClassIncompatible"))
        }

        // localization/localize.ts:277
        pub fn base_class_invalid() -> String {
            get_raw_string("Diagnostic.baseClassInvalid")
        }

        // localization/localize.ts:278
        /// Params: classType, name.
        pub fn base_class_method_type_incompatible() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.baseClassMethodTypeIncompatible"))
        }

        // localization/localize.ts:282
        /// Params: classType, name.
        pub fn base_class_variable_type_incompatible() -> ParameterizedString {
            ParameterizedString::new(get_raw_string(
                "Diagnostic.baseClassVariableTypeIncompatible",
            ))
        }

        // localization/localize.ts:286
        pub fn base_class_unknown() -> String {
            get_raw_string("Diagnostic.baseClassUnknown")
        }

        // localization/localize.ts:287
        pub fn binary_operation_not_allowed() -> String {
            get_raw_string("Diagnostic.binaryOperationNotAllowed")
        }

        // localization/localize.ts:288
        /// Params: methodName.
        pub fn bind_param_missing() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.bindParamMissing"))
        }

        // localization/localize.ts:290
        /// Params: type, methodName, paramName.
        pub fn bind_type_mismatch() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.bindTypeMismatch"))
        }

        // localization/localize.ts:294
        pub fn break_in_exception_group() -> String {
            get_raw_string("Diagnostic.breakInExceptionGroup")
        }

        // localization/localize.ts:295
        pub fn break_outside_loop() -> String {
            get_raw_string("Diagnostic.breakOutsideLoop")
        }

        // localization/localize.ts:296
        pub fn bytes_unsupported_escape() -> String {
            get_raw_string("Diagnostic.bytesUnsupportedEscape")
        }

        // localization/localize.ts:297
        pub fn callable_extra_args() -> String {
            get_raw_string("Diagnostic.callableExtraArgs")
        }

        // localization/localize.ts:298
        pub fn callable_first_arg() -> String {
            get_raw_string("Diagnostic.callableFirstArg")
        }

        // localization/localize.ts:299
        /// Params: type.
        pub fn callable_not_instantiable() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.callableNotInstantiable"))
        }

        // localization/localize.ts:301
        pub fn callable_second_arg() -> String {
            get_raw_string("Diagnostic.callableSecondArg")
        }

        // localization/localize.ts:302
        pub fn case_pattern_is_irrefutable() -> String {
            get_raw_string("Diagnostic.casePatternIsIrrefutable")
        }

        // localization/localize.ts:303
        /// Params: type.
        pub fn class_already_specialized() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.classAlreadySpecialized"))
        }

        // localization/localize.ts:305
        pub fn class_decorator_type_unknown() -> String {
            get_raw_string("Diagnostic.classDecoratorTypeUnknown")
        }

        // localization/localize.ts:306
        /// Params: name.
        pub fn class_definition_cycle() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.classDefinitionCycle"))
        }

        // localization/localize.ts:308
        pub fn class_get_item_cls_param() -> String {
            get_raw_string("Diagnostic.classGetItemClsParam")
        }

        // localization/localize.ts:309
        pub fn class_method_cls_param() -> String {
            get_raw_string("Diagnostic.classMethodClsParam")
        }

        // localization/localize.ts:310
        /// Params: name.
        pub fn class_not_runtime_subscriptable() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.classNotRuntimeSubscriptable"))
        }

        // localization/localize.ts:312
        pub fn class_pattern_built_in_arg_positional() -> String {
            get_raw_string("Diagnostic.classPatternBuiltInArgPositional")
        }

        // localization/localize.ts:314
        /// Params: type.
        pub fn class_pattern_new_type() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.classPatternNewType"))
        }

        // localization/localize.ts:316
        /// Params: type, expected, received.
        pub fn class_pattern_positional_arg_count() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.classPatternPositionalArgCount"))
        }

        // localization/localize.ts:320
        /// Params: type.
        pub fn class_pattern_type_alias() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.classPatternTypeAlias"))
        }

        // localization/localize.ts:322
        pub fn class_property_deprecated() -> String {
            get_raw_string("Diagnostic.classPropertyDeprecated")
        }

        // localization/localize.ts:323
        pub fn class_type_parameters_illegal() -> String {
            get_raw_string("Diagnostic.classTypeParametersIllegal")
        }

        // localization/localize.ts:324
        pub fn class_var_not_allowed() -> String {
            get_raw_string("Diagnostic.classVarNotAllowed")
        }

        // localization/localize.ts:325
        pub fn class_var_first_arg_missing() -> String {
            get_raw_string("Diagnostic.classVarFirstArgMissing")
        }

        // localization/localize.ts:326
        /// Params: name, className.
        pub fn class_var_overrides_instance_var() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.classVarOverridesInstanceVar"))
        }

        // localization/localize.ts:330
        pub fn class_var_too_many_args() -> String {
            get_raw_string("Diagnostic.classVarTooManyArgs")
        }

        // localization/localize.ts:331
        pub fn class_var_with_type_var() -> String {
            get_raw_string("Diagnostic.classVarWithTypeVar")
        }

        // localization/localize.ts:332
        /// Params: name, classType.
        pub fn cls_self_param_type_mismatch() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.clsSelfParamTypeMismatch"))
        }

        // localization/localize.ts:336
        pub fn code_too_complex_to_analyze() -> String {
            get_raw_string("Diagnostic.codeTooComplexToAnalyze")
        }

        // localization/localize.ts:337
        /// Params: type, alias.
        pub fn collection_alias_instantiation() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.collectionAliasInstantiation"))
        }

        // localization/localize.ts:341
        /// Params: leftType, rightType.
        pub fn comparison_always_false() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.comparisonAlwaysFalse"))
        }

        // localization/localize.ts:345
        /// Params: leftType, rightType.
        pub fn comparison_always_true() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.comparisonAlwaysTrue"))
        }

        // localization/localize.ts:349
        pub fn comprehension_in_dict() -> String {
            get_raw_string("Diagnostic.comprehensionInDict")
        }

        // localization/localize.ts:350
        pub fn comprehension_in_set() -> String {
            get_raw_string("Diagnostic.comprehensionInSet")
        }

        // localization/localize.ts:351
        pub fn concatenate_context() -> String {
            get_raw_string("Diagnostic.concatenateContext")
        }

        // localization/localize.ts:352
        pub fn concatenate_param_spec_missing() -> String {
            get_raw_string("Diagnostic.concatenateParamSpecMissing")
        }

        // localization/localize.ts:353
        pub fn concatenate_type_args_missing() -> String {
            get_raw_string("Diagnostic.concatenateTypeArgsMissing")
        }

        // localization/localize.ts:354
        /// Params: type.
        pub fn conditional_operand_invalid() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.conditionalOperandInvalid"))
        }

        // localization/localize.ts:356
        /// Params: name.
        pub fn constant_redefinition() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.constantRedefinition"))
        }

        // localization/localize.ts:358
        pub fn coroutine_in_conditional_expression() -> String {
            get_raw_string("Diagnostic.coroutineInConditionalExpression")
        }

        // localization/localize.ts:360
        /// Params: classType.
        pub fn constructor_parameters_mismatch() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.constructorParametersMismatch"))
        }

        // localization/localize.ts:362
        /// Params: leftType, rightType.
        pub fn containment_always_false() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.containmentAlwaysFalse"))
        }

        // localization/localize.ts:366
        /// Params: leftType, rightType.
        pub fn containment_always_true() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.containmentAlwaysTrue"))
        }

        // localization/localize.ts:370
        pub fn continue_in_exception_group() -> String {
            get_raw_string("Diagnostic.continueInExceptionGroup")
        }

        // localization/localize.ts:371
        pub fn continue_outside_loop() -> String {
            get_raw_string("Diagnostic.continueOutsideLoop")
        }

        // localization/localize.ts:372
        pub fn data_class_base_class_frozen() -> String {
            get_raw_string("Diagnostic.dataClassBaseClassFrozen")
        }

        // localization/localize.ts:373
        pub fn data_class_base_class_not_frozen() -> String {
            get_raw_string("Diagnostic.dataClassBaseClassNotFrozen")
        }

        // localization/localize.ts:374
        /// Params: argType, fieldType, fieldName.
        pub fn data_class_converter_function() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.dataClassConverterFunction"))
        }

        // localization/localize.ts:378
        /// Params: funcName, fieldType, fieldName.
        pub fn data_class_converter_overloads() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.dataClassConverterOverloads"))
        }

        // localization/localize.ts:382
        /// Params: name.
        pub fn data_class_duplicate_kw_only() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.dataClassDuplicateKwOnly"))
        }

        // localization/localize.ts:384
        /// Params: fieldName.
        pub fn data_class_field_inherited_default() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.dataClassFieldInheritedDefault"))
        }

        // localization/localize.ts:386
        pub fn data_class_field_with_default() -> String {
            get_raw_string("Diagnostic.dataClassFieldWithDefault")
        }

        // localization/localize.ts:387
        pub fn data_class_field_without_annotation() -> String {
            get_raw_string("Diagnostic.dataClassFieldWithoutAnnotation")
        }

        // localization/localize.ts:388
        pub fn data_class_field_with_private_name() -> String {
            get_raw_string("Diagnostic.dataClassFieldWithPrivateName")
        }

        // localization/localize.ts:389
        /// Params: expected.
        pub fn data_class_post_init_param_count() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.dataClassPostInitParamCount"))
        }

        // localization/localize.ts:391
        /// Params: fieldName.
        pub fn data_class_post_init_type() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.dataClassPostInitType"))
        }

        // localization/localize.ts:393
        pub fn data_class_slots_overwrite() -> String {
            get_raw_string("Diagnostic.dataClassSlotsOverwrite")
        }

        // localization/localize.ts:394
        pub fn data_class_transform_expected_bool_literal() -> String {
            get_raw_string("Diagnostic.dataClassTransformExpectedBoolLiteral")
        }

        // localization/localize.ts:396
        /// Params: type.
        pub fn data_class_transform_field_specifier() -> ParameterizedString {
            ParameterizedString::new(get_raw_string(
                "Diagnostic.dataClassTransformFieldSpecifier",
            ))
        }

        // localization/localize.ts:398
        pub fn data_class_transform_positional_param() -> String {
            get_raw_string("Diagnostic.dataClassTransformPositionalParam")
        }

        // localization/localize.ts:400
        /// Params: name.
        pub fn data_class_transform_unknown_argument() -> ParameterizedString {
            ParameterizedString::new(get_raw_string(
                "Diagnostic.dataClassTransformUnknownArgument",
            ))
        }

        // localization/localize.ts:402
        pub fn data_protocol_in_subclass_check() -> String {
            get_raw_string("Diagnostic.dataProtocolInSubclassCheck")
        }

        // localization/localize.ts:403
        /// Params: returnType.
        pub fn declared_return_type_partially_unknown() -> ParameterizedString {
            ParameterizedString::new(get_raw_string(
                "Diagnostic.declaredReturnTypePartiallyUnknown",
            ))
        }

        // localization/localize.ts:407
        pub fn declared_return_type_unknown() -> String {
            get_raw_string("Diagnostic.declaredReturnTypeUnknown")
        }

        // localization/localize.ts:408
        pub fn default_value_contains_call() -> String {
            get_raw_string("Diagnostic.defaultValueContainsCall")
        }

        // localization/localize.ts:409
        pub fn default_value_not_allowed() -> String {
            get_raw_string("Diagnostic.defaultValueNotAllowed")
        }

        // localization/localize.ts:410
        /// Params: name.
        pub fn deprecated_class() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.deprecatedClass"))
        }

        // localization/localize.ts:412
        /// Params: name.
        pub fn deprecated_constructor() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.deprecatedConstructor"))
        }

        // localization/localize.ts:414
        /// Params: name.
        pub fn deprecated_descriptor_deleter() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.deprecatedDescriptorDeleter"))
        }

        // localization/localize.ts:416
        /// Params: name.
        pub fn deprecated_descriptor_getter() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.deprecatedDescriptorGetter"))
        }

        // localization/localize.ts:418
        /// Params: name.
        pub fn deprecated_descriptor_setter() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.deprecatedDescriptorSetter"))
        }

        // localization/localize.ts:420
        /// Params: name.
        pub fn deprecated_function() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.deprecatedFunction"))
        }

        // localization/localize.ts:422
        /// Params: name, className.
        pub fn deprecated_method() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.deprecatedMethod"))
        }

        // localization/localize.ts:424
        /// Params: name.
        pub fn deprecated_property_deleter() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.deprecatedPropertyDeleter"))
        }

        // localization/localize.ts:426
        /// Params: name.
        pub fn deprecated_property_getter() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.deprecatedPropertyGetter"))
        }

        // localization/localize.ts:428
        /// Params: name.
        pub fn deprecated_property_setter() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.deprecatedPropertySetter"))
        }

        // localization/localize.ts:430
        /// Params: version, replacement.
        pub fn deprecated_type() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.deprecatedType"))
        }

        // localization/localize.ts:434
        pub fn dict_expand_illegal_in_comprehension() -> String {
            get_raw_string("Diagnostic.dictExpandIllegalInComprehension")
        }

        // localization/localize.ts:436
        pub fn dict_in_annotation() -> String {
            get_raw_string("Diagnostic.dictInAnnotation")
        }

        // localization/localize.ts:437
        pub fn dict_key_value_pairs() -> String {
            get_raw_string("Diagnostic.dictKeyValuePairs")
        }

        // localization/localize.ts:438
        pub fn dict_unpack_is_not_mapping() -> String {
            get_raw_string("Diagnostic.dictUnpackIsNotMapping")
        }

        // localization/localize.ts:439
        pub fn disjoint_base_function() -> String {
            get_raw_string("Diagnostic.disjointBaseFunction")
        }

        // localization/localize.ts:440
        /// Params: bases.
        pub fn disjoint_base_incompatible() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.disjointBaseIncompatible"))
        }

        // localization/localize.ts:442
        pub fn disjoint_base_protocol() -> String {
            get_raw_string("Diagnostic.disjointBaseProtocol")
        }

        // localization/localize.ts:443
        pub fn disjoint_base_typed_dict() -> String {
            get_raw_string("Diagnostic.disjointBaseTypedDict")
        }

        // localization/localize.ts:444
        pub fn del_target_expr() -> String {
            get_raw_string("Diagnostic.delTargetExpr")
        }

        // localization/localize.ts:445
        /// Params: name.
        pub fn dunder_all_symbol_not_present() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.dunderAllSymbolNotPresent"))
        }

        // localization/localize.ts:447
        pub fn duplicate_args_param() -> String {
            get_raw_string("Diagnostic.duplicateArgsParam")
        }

        // localization/localize.ts:448
        pub fn duplicate_base_class() -> String {
            get_raw_string("Diagnostic.duplicateBaseClass")
        }

        // localization/localize.ts:449
        pub fn duplicate_catch_all() -> String {
            get_raw_string("Diagnostic.duplicateCatchAll")
        }

        // localization/localize.ts:450
        /// Params: name.
        pub fn duplicate_enum_member() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.duplicateEnumMember"))
        }

        // localization/localize.ts:452
        pub fn duplicate_generic_and_protocol_base() -> String {
            get_raw_string("Diagnostic.duplicateGenericAndProtocolBase")
        }

        // localization/localize.ts:453
        /// Params: importName.
        pub fn duplicate_import() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.duplicateImport"))
        }

        // localization/localize.ts:455
        pub fn duplicate_kwargs_param() -> String {
            get_raw_string("Diagnostic.duplicateKwargsParam")
        }

        // localization/localize.ts:456
        pub fn duplicate_keyword_only() -> String {
            get_raw_string("Diagnostic.duplicateKeywordOnly")
        }

        // localization/localize.ts:457
        /// Params: name.
        pub fn duplicate_param() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.duplicateParam"))
        }

        // localization/localize.ts:459
        /// Params: name.
        pub fn duplicate_capture_pattern_target() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.duplicateCapturePatternTarget"))
        }

        // localization/localize.ts:461
        pub fn duplicate_star_pattern() -> String {
            get_raw_string("Diagnostic.duplicateStarPattern")
        }

        // localization/localize.ts:462
        pub fn duplicate_star_star_pattern() -> String {
            get_raw_string("Diagnostic.duplicateStarStarPattern")
        }

        // localization/localize.ts:463
        pub fn duplicate_position_only() -> String {
            get_raw_string("Diagnostic.duplicatePositionOnly")
        }

        // localization/localize.ts:464
        pub fn duplicate_unpack() -> String {
            get_raw_string("Diagnostic.duplicateUnpack")
        }

        // localization/localize.ts:465
        pub fn ellipsis_after_unpacked() -> String {
            get_raw_string("Diagnostic.ellipsisAfterUnpacked")
        }

        // localization/localize.ts:466
        pub fn ellipsis_context() -> String {
            get_raw_string("Diagnostic.ellipsisContext")
        }

        // localization/localize.ts:467
        pub fn ellipsis_second_arg() -> String {
            get_raw_string("Diagnostic.ellipsisSecondArg")
        }

        // localization/localize.ts:468
        /// Params: name.
        pub fn enum_class_override() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.enumClassOverride"))
        }

        // localization/localize.ts:470
        /// Params: name.
        pub fn enum_member_delete() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.enumMemberDelete"))
        }

        // localization/localize.ts:472
        /// Params: name.
        pub fn enum_member_set() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.enumMemberSet"))
        }

        // localization/localize.ts:474
        pub fn enum_member_type_annotation() -> String {
            get_raw_string("Diagnostic.enumMemberTypeAnnotation")
        }

        // localization/localize.ts:475
        pub fn exception_group_incompatible() -> String {
            get_raw_string("Diagnostic.exceptionGroupIncompatible")
        }

        // localization/localize.ts:476
        pub fn except_group_mismatch() -> String {
            get_raw_string("Diagnostic.exceptGroupMismatch")
        }

        // localization/localize.ts:477
        pub fn except_group_requires_type() -> String {
            get_raw_string("Diagnostic.exceptGroupRequiresType")
        }

        // localization/localize.ts:478
        pub fn exception_group_type_incorrect() -> String {
            get_raw_string("Diagnostic.exceptionGroupTypeIncorrect")
        }

        // localization/localize.ts:479
        /// Params: type.
        pub fn exception_type_incorrect() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.exceptionTypeIncorrect"))
        }

        // localization/localize.ts:481
        /// Params: type.
        pub fn exception_type_not_class() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.exceptionTypeNotClass"))
        }

        // localization/localize.ts:483
        /// Params: type.
        pub fn exception_type_not_instantiable() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.exceptionTypeNotInstantiable"))
        }

        // localization/localize.ts:485
        pub fn except_requires_parens() -> String {
            get_raw_string("Diagnostic.exceptRequiresParens")
        }

        // localization/localize.ts:486
        pub fn except_with_as_requires_parens() -> String {
            get_raw_string("Diagnostic.exceptWithAsRequiresParens")
        }

        // localization/localize.ts:487
        pub fn expected_after_decorator() -> String {
            get_raw_string("Diagnostic.expectedAfterDecorator")
        }

        // localization/localize.ts:488
        pub fn expected_arrow() -> String {
            get_raw_string("Diagnostic.expectedArrow")
        }

        // localization/localize.ts:489
        pub fn expected_as_after_exception() -> String {
            get_raw_string("Diagnostic.expectedAsAfterException")
        }

        // localization/localize.ts:490
        pub fn expected_assign_right_hand_expr() -> String {
            get_raw_string("Diagnostic.expectedAssignRightHandExpr")
        }

        // localization/localize.ts:491
        pub fn expected_binary_right_hand_expr() -> String {
            get_raw_string("Diagnostic.expectedBinaryRightHandExpr")
        }

        // localization/localize.ts:492
        pub fn expected_bool_literal() -> String {
            get_raw_string("Diagnostic.expectedBoolLiteral")
        }

        // localization/localize.ts:493
        pub fn expected_case() -> String {
            get_raw_string("Diagnostic.expectedCase")
        }

        // localization/localize.ts:494
        pub fn expected_class_name() -> String {
            get_raw_string("Diagnostic.expectedClassName")
        }

        // localization/localize.ts:495
        pub fn expected_close_brace() -> String {
            get_raw_string("Diagnostic.expectedCloseBrace")
        }

        // localization/localize.ts:496
        pub fn expected_close_bracket() -> String {
            get_raw_string("Diagnostic.expectedCloseBracket")
        }

        // localization/localize.ts:497
        pub fn expected_close_paren() -> String {
            get_raw_string("Diagnostic.expectedCloseParen")
        }

        // localization/localize.ts:498
        pub fn expected_colon() -> String {
            get_raw_string("Diagnostic.expectedColon")
        }

        // localization/localize.ts:499
        pub fn expected_complex_number_literal() -> String {
            get_raw_string("Diagnostic.expectedComplexNumberLiteral")
        }

        // localization/localize.ts:500
        pub fn expected_decorator_expr() -> String {
            get_raw_string("Diagnostic.expectedDecoratorExpr")
        }

        // localization/localize.ts:501
        pub fn expected_decorator_name() -> String {
            get_raw_string("Diagnostic.expectedDecoratorName")
        }

        // localization/localize.ts:502
        pub fn expected_decorator_newline() -> String {
            get_raw_string("Diagnostic.expectedDecoratorNewline")
        }

        // localization/localize.ts:503
        pub fn expected_del_expr() -> String {
            get_raw_string("Diagnostic.expectedDelExpr")
        }

        // localization/localize.ts:504
        pub fn expected_else() -> String {
            get_raw_string("Diagnostic.expectedElse")
        }

        // localization/localize.ts:505
        pub fn expected_equals() -> String {
            get_raw_string("Diagnostic.expectedEquals")
        }

        // localization/localize.ts:506
        pub fn expected_exception_class() -> String {
            get_raw_string("Diagnostic.expectedExceptionClass")
        }

        // localization/localize.ts:507
        pub fn expected_exception_obj() -> String {
            get_raw_string("Diagnostic.expectedExceptionObj")
        }

        // localization/localize.ts:508
        pub fn expected_expr() -> String {
            get_raw_string("Diagnostic.expectedExpr")
        }

        // localization/localize.ts:509
        pub fn expected_identifier() -> String {
            get_raw_string("Diagnostic.expectedIdentifier")
        }

        // localization/localize.ts:510
        pub fn expected_import() -> String {
            get_raw_string("Diagnostic.expectedImport")
        }

        // localization/localize.ts:511
        pub fn expected_import_alias() -> String {
            get_raw_string("Diagnostic.expectedImportAlias")
        }

        // localization/localize.ts:512
        pub fn expected_import_symbols() -> String {
            get_raw_string("Diagnostic.expectedImportSymbols")
        }

        // localization/localize.ts:513
        pub fn expected_indented_block() -> String {
            get_raw_string("Diagnostic.expectedIndentedBlock")
        }

        // localization/localize.ts:514
        pub fn expected_in() -> String {
            get_raw_string("Diagnostic.expectedIn")
        }

        // localization/localize.ts:515
        pub fn expected_in_expr() -> String {
            get_raw_string("Diagnostic.expectedInExpr")
        }

        // localization/localize.ts:516
        pub fn expected_function_after_async() -> String {
            get_raw_string("Diagnostic.expectedFunctionAfterAsync")
        }

        // localization/localize.ts:517
        pub fn expected_function_name() -> String {
            get_raw_string("Diagnostic.expectedFunctionName")
        }

        // localization/localize.ts:518
        pub fn expected_member_name() -> String {
            get_raw_string("Diagnostic.expectedMemberName")
        }

        // localization/localize.ts:519
        pub fn expected_module_name() -> String {
            get_raw_string("Diagnostic.expectedModuleName")
        }

        // localization/localize.ts:520
        pub fn expected_name_after_as() -> String {
            get_raw_string("Diagnostic.expectedNameAfterAs")
        }

        // localization/localize.ts:521
        pub fn expected_named_parameter() -> String {
            get_raw_string("Diagnostic.expectedNamedParameter")
        }

        // localization/localize.ts:522
        pub fn expected_newline() -> String {
            get_raw_string("Diagnostic.expectedNewline")
        }

        // localization/localize.ts:523
        pub fn expected_newline_or_semicolon() -> String {
            get_raw_string("Diagnostic.expectedNewlineOrSemicolon")
        }

        // localization/localize.ts:524
        pub fn expected_open_paren() -> String {
            get_raw_string("Diagnostic.expectedOpenParen")
        }

        // localization/localize.ts:525
        pub fn expected_param_name() -> String {
            get_raw_string("Diagnostic.expectedParamName")
        }

        // localization/localize.ts:526
        pub fn expected_pattern_expr() -> String {
            get_raw_string("Diagnostic.expectedPatternExpr")
        }

        // localization/localize.ts:527
        pub fn expected_pattern_subject_expr() -> String {
            get_raw_string("Diagnostic.expectedPatternSubjectExpr")
        }

        // localization/localize.ts:528
        pub fn expected_pattern_value() -> String {
            get_raw_string("Diagnostic.expectedPatternValue")
        }

        // localization/localize.ts:529
        pub fn expected_return_expr() -> String {
            get_raw_string("Diagnostic.expectedReturnExpr")
        }

        // localization/localize.ts:530
        pub fn expected_slice_index() -> String {
            get_raw_string("Diagnostic.expectedSliceIndex")
        }

        // localization/localize.ts:531
        pub fn expected_type_not_string() -> String {
            get_raw_string("Diagnostic.expectedTypeNotString")
        }

        // localization/localize.ts:532
        pub fn expected_type_parameter_name() -> String {
            get_raw_string("Diagnostic.expectedTypeParameterName")
        }

        // localization/localize.ts:533
        pub fn expected_yield_expr() -> String {
            get_raw_string("Diagnostic.expectedYieldExpr")
        }

        // localization/localize.ts:534
        /// Params: type.
        pub fn final_class_is_abstract() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.finalClassIsAbstract"))
        }

        // localization/localize.ts:536
        pub fn final_context() -> String {
            get_raw_string("Diagnostic.finalContext")
        }

        // localization/localize.ts:537
        pub fn final_in_loop() -> String {
            get_raw_string("Diagnostic.finalInLoop")
        }

        // localization/localize.ts:538
        pub fn finally_break() -> String {
            get_raw_string("Diagnostic.finallyBreak")
        }

        // localization/localize.ts:539
        pub fn finally_continue() -> String {
            get_raw_string("Diagnostic.finallyContinue")
        }

        // localization/localize.ts:540
        pub fn finally_return() -> String {
            get_raw_string("Diagnostic.finallyReturn")
        }

        // localization/localize.ts:541
        /// Params: name, className.
        pub fn final_method_override() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.finalMethodOverride"))
        }

        // localization/localize.ts:545
        /// Params: name.
        pub fn final_non_method() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.finalNonMethod"))
        }

        // localization/localize.ts:547
        /// Params: name.
        pub fn final_reassigned() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.finalReassigned"))
        }

        // localization/localize.ts:549
        /// Params: name.
        pub fn final_redeclaration() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.finalRedeclaration"))
        }

        // localization/localize.ts:551
        /// Params: name, className.
        pub fn final_redeclaration_by_subclass() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.finalRedeclarationBySubclass"))
        }

        // localization/localize.ts:555
        pub fn final_too_many_args() -> String {
            get_raw_string("Diagnostic.finalTooManyArgs")
        }

        // localization/localize.ts:556
        /// Params: name.
        pub fn final_unassigned() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.finalUnassigned"))
        }

        // localization/localize.ts:558
        pub fn format_string_brace() -> String {
            get_raw_string("Diagnostic.formatStringBrace")
        }

        // localization/localize.ts:559
        pub fn format_string_bytes() -> String {
            get_raw_string("Diagnostic.formatStringBytes")
        }

        // localization/localize.ts:560
        pub fn format_string_debugging_illegal() -> String {
            get_raw_string("Diagnostic.formatStringDebuggingIllegal")
        }

        // localization/localize.ts:561
        pub fn format_string_escape() -> String {
            get_raw_string("Diagnostic.formatStringEscape")
        }

        // localization/localize.ts:562
        pub fn format_string_expected_conversion() -> String {
            get_raw_string("Diagnostic.formatStringExpectedConversion")
        }

        // localization/localize.ts:563
        pub fn format_string_in_pattern() -> String {
            get_raw_string("Diagnostic.formatStringInPattern")
        }

        // localization/localize.ts:564
        pub fn format_string_illegal() -> String {
            get_raw_string("Diagnostic.formatStringIllegal")
        }

        // localization/localize.ts:565
        pub fn format_string_nested_format_specifier() -> String {
            get_raw_string("Diagnostic.formatStringNestedFormatSpecifier")
        }

        // localization/localize.ts:567
        pub fn format_string_nested_quote() -> String {
            get_raw_string("Diagnostic.formatStringNestedQuote")
        }

        // localization/localize.ts:568
        pub fn format_string_template() -> String {
            get_raw_string("Diagnostic.formatStringTemplate")
        }

        // localization/localize.ts:569
        pub fn format_string_unicode() -> String {
            get_raw_string("Diagnostic.formatStringUnicode")
        }

        // localization/localize.ts:570
        pub fn format_string_unterminated() -> String {
            get_raw_string("Diagnostic.formatStringUnterminated")
        }

        // localization/localize.ts:571
        pub fn function_decorator_type_unknown() -> String {
            get_raw_string("Diagnostic.functionDecoratorTypeUnknown")
        }

        // localization/localize.ts:572
        pub fn function_in_conditional_expression() -> String {
            get_raw_string("Diagnostic.functionInConditionalExpression")
        }

        // localization/localize.ts:573
        pub fn function_type_parameters_illegal() -> String {
            get_raw_string("Diagnostic.functionTypeParametersIllegal")
        }

        // localization/localize.ts:574
        pub fn future_import_location_not_allowed() -> String {
            get_raw_string("Diagnostic.futureImportLocationNotAllowed")
        }

        // localization/localize.ts:575
        /// Params: yieldType.
        pub fn generator_async_return_type() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.generatorAsyncReturnType"))
        }

        // localization/localize.ts:577
        pub fn generator_not_parenthesized() -> String {
            get_raw_string("Diagnostic.generatorNotParenthesized")
        }

        // localization/localize.ts:578
        /// Params: yieldType.
        pub fn generator_sync_return_type() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.generatorSyncReturnType"))
        }

        // localization/localize.ts:580
        pub fn generic_base_class_not_allowed() -> String {
            get_raw_string("Diagnostic.genericBaseClassNotAllowed")
        }

        // localization/localize.ts:581
        pub fn generic_class_assigned() -> String {
            get_raw_string("Diagnostic.genericClassAssigned")
        }

        // localization/localize.ts:582
        pub fn generic_class_deleted() -> String {
            get_raw_string("Diagnostic.genericClassDeleted")
        }

        // localization/localize.ts:583
        pub fn generic_instance_variable_access() -> String {
            get_raw_string("Diagnostic.genericInstanceVariableAccess")
        }

        // localization/localize.ts:584
        pub fn generic_not_allowed() -> String {
            get_raw_string("Diagnostic.genericNotAllowed")
        }

        // localization/localize.ts:585
        /// Params: names.
        pub fn generic_type_alias_bound_type_var() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.genericTypeAliasBoundTypeVar"))
        }

        // localization/localize.ts:587
        pub fn generic_type_arg_missing() -> String {
            get_raw_string("Diagnostic.genericTypeArgMissing")
        }

        // localization/localize.ts:588
        pub fn generic_type_arg_type_var() -> String {
            get_raw_string("Diagnostic.genericTypeArgTypeVar")
        }

        // localization/localize.ts:589
        pub fn generic_type_arg_unique() -> String {
            get_raw_string("Diagnostic.genericTypeArgUnique")
        }

        // localization/localize.ts:590
        /// Params: name.
        pub fn global_reassignment() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.globalReassignment"))
        }

        // localization/localize.ts:592
        /// Params: name.
        pub fn global_redefinition() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.globalRedefinition"))
        }

        // localization/localize.ts:594
        pub fn implicit_string_concat() -> String {
            get_raw_string("Diagnostic.implicitStringConcat")
        }

        // localization/localize.ts:595
        pub fn import_cycle_detected() -> String {
            get_raw_string("Diagnostic.importCycleDetected")
        }

        // localization/localize.ts:596
        /// Params: depth.
        pub fn import_depth_exceeded() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.importDepthExceeded"))
        }

        // localization/localize.ts:598
        /// Params: importName, venv.
        pub fn import_resolve_failure() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.importResolveFailure"))
        }

        // localization/localize.ts:602
        /// Params: importName, venv.
        pub fn import_source_resolve_failure() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.importSourceResolveFailure"))
        }

        // localization/localize.ts:606
        /// Params: name.
        pub fn import_symbol_unknown() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.importSymbolUnknown"))
        }

        // localization/localize.ts:608
        /// Params: name, className.
        pub fn incompatible_method_override() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.incompatibleMethodOverride"))
        }

        // localization/localize.ts:612
        pub fn inconsistent_indent() -> String {
            get_raw_string("Diagnostic.inconsistentIndent")
        }

        // localization/localize.ts:613
        pub fn inconsistent_tabs() -> String {
            get_raw_string("Diagnostic.inconsistentTabs")
        }

        // localization/localize.ts:614
        pub fn init_method_self_param_type_var() -> String {
            get_raw_string("Diagnostic.initMethodSelfParamTypeVar")
        }

        // localization/localize.ts:615
        pub fn init_must_return_none() -> String {
            get_raw_string("Diagnostic.initMustReturnNone")
        }

        // localization/localize.ts:616
        pub fn init_subclass_cls_param() -> String {
            get_raw_string("Diagnostic.initSubclassClsParam")
        }

        // localization/localize.ts:617
        pub fn init_subclass_call_failed() -> String {
            get_raw_string("Diagnostic.initSubclassCallFailed")
        }

        // localization/localize.ts:618
        pub fn init_var_not_allowed() -> String {
            get_raw_string("Diagnostic.initVarNotAllowed")
        }

        // localization/localize.ts:619
        pub fn instance_method_self_param() -> String {
            get_raw_string("Diagnostic.instanceMethodSelfParam")
        }

        // localization/localize.ts:620
        /// Params: name, className.
        pub fn instance_var_overrides_class_var() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.instanceVarOverridesClassVar"))
        }

        // localization/localize.ts:624
        /// Params: type.
        pub fn instantiate_abstract() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.instantiateAbstract"))
        }

        // localization/localize.ts:626
        /// Params: type.
        pub fn instantiate_protocol() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.instantiateProtocol"))
        }

        // localization/localize.ts:628
        /// Params: file, message.
        pub fn internal_bind_error() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.internalBindError"))
        }

        // localization/localize.ts:630
        /// Params: file, message.
        pub fn internal_parse_error() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.internalParseError"))
        }

        // localization/localize.ts:632
        /// Params: file, message.
        pub fn internal_type_checking_error() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.internalTypeCheckingError"))
        }

        // localization/localize.ts:636
        pub fn invalid_identifier_char() -> String {
            get_raw_string("Diagnostic.invalidIdentifierChar")
        }

        // localization/localize.ts:637
        pub fn invalid_stub_statement() -> String {
            get_raw_string("Diagnostic.invalidStubStatement")
        }

        // localization/localize.ts:638
        /// Params: text.
        pub fn invalid_token_chars() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.invalidTokenChars"))
        }

        // localization/localize.ts:640
        /// Params: type.
        pub fn is_instance_invalid_type() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.isInstanceInvalidType"))
        }

        // localization/localize.ts:642
        /// Params: type.
        pub fn is_subclass_invalid_type() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.isSubclassInvalidType"))
        }

        // localization/localize.ts:644
        pub fn key_value_in_set() -> String {
            get_raw_string("Diagnostic.keyValueInSet")
        }

        // localization/localize.ts:645
        pub fn keyword_arg_in_type_argument() -> String {
            get_raw_string("Diagnostic.keywordArgInTypeArgument")
        }

        // localization/localize.ts:646
        pub fn keyword_only_after_args() -> String {
            get_raw_string("Diagnostic.keywordOnlyAfterArgs")
        }

        // localization/localize.ts:647
        pub fn keyword_parameter_missing() -> String {
            get_raw_string("Diagnostic.keywordParameterMissing")
        }

        // localization/localize.ts:648
        pub fn keyword_subscript_illegal() -> String {
            get_raw_string("Diagnostic.keywordSubscriptIllegal")
        }

        // localization/localize.ts:649
        pub fn lambda_return_type_unknown() -> String {
            get_raw_string("Diagnostic.lambdaReturnTypeUnknown")
        }

        // localization/localize.ts:650
        /// Params: returnType.
        pub fn lambda_return_type_partially_unknown() -> ParameterizedString {
            ParameterizedString::new(get_raw_string(
                "Diagnostic.lambdaReturnTypePartiallyUnknown",
            ))
        }

        // localization/localize.ts:654
        pub fn lazy_import_illegal() -> String {
            get_raw_string("Diagnostic.lazyImportIllegal")
        }

        // localization/localize.ts:655
        pub fn lazy_import_wildcard_illegal() -> String {
            get_raw_string("Diagnostic.lazyImportWildcardIllegal")
        }

        // localization/localize.ts:656
        /// Params: type.
        pub fn list_assignment_mismatch() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.listAssignmentMismatch"))
        }

        // localization/localize.ts:658
        pub fn list_in_annotation() -> String {
            get_raw_string("Diagnostic.listInAnnotation")
        }

        // localization/localize.ts:659
        pub fn literal_named_unicode_escape() -> String {
            get_raw_string("Diagnostic.literalNamedUnicodeEscape")
        }

        // localization/localize.ts:660
        pub fn literal_unsupported_type() -> String {
            get_raw_string("Diagnostic.literalUnsupportedType")
        }

        // localization/localize.ts:661
        pub fn literal_empty_args() -> String {
            get_raw_string("Diagnostic.literalEmptyArgs")
        }

        // localization/localize.ts:662
        pub fn literal_not_allowed() -> String {
            get_raw_string("Diagnostic.literalNotAllowed")
        }

        // localization/localize.ts:663
        pub fn literal_not_callable() -> String {
            get_raw_string("Diagnostic.literalNotCallable")
        }

        // localization/localize.ts:664
        pub fn match_incompatible() -> String {
            get_raw_string("Diagnostic.matchIncompatible")
        }

        // localization/localize.ts:665
        pub fn match_is_not_exhaustive() -> String {
            get_raw_string("Diagnostic.matchIsNotExhaustive")
        }

        // localization/localize.ts:666
        pub fn max_parse_depth_exceeded() -> String {
            get_raw_string("Diagnostic.maxParseDepthExceeded")
        }

        // localization/localize.ts:667
        /// Params: name, type.
        pub fn member_access() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.memberAccess"))
        }

        // localization/localize.ts:669
        /// Params: name, type.
        pub fn member_delete() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.memberDelete"))
        }

        // localization/localize.ts:671
        /// Params: name, type.
        pub fn member_set() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.memberSet"))
        }

        // localization/localize.ts:673
        pub fn metaclass_conflict() -> String {
            get_raw_string("Diagnostic.metaclassConflict")
        }

        // localization/localize.ts:674
        pub fn metaclass_duplicate() -> String {
            get_raw_string("Diagnostic.metaclassDuplicate")
        }

        // localization/localize.ts:675
        pub fn metaclass_is_generic() -> String {
            get_raw_string("Diagnostic.metaclassIsGeneric")
        }

        // localization/localize.ts:676
        /// Params: name.
        pub fn method_not_defined() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.methodNotDefined"))
        }

        // localization/localize.ts:678
        /// Params: name, type.
        pub fn method_not_defined_on_type() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.methodNotDefinedOnType"))
        }

        // localization/localize.ts:680
        pub fn method_ordering() -> String {
            get_raw_string("Diagnostic.methodOrdering")
        }

        // localization/localize.ts:681
        /// Params: name, className, type.
        pub fn method_overridden() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.methodOverridden"))
        }

        // localization/localize.ts:685
        /// Params: name.
        pub fn method_returns_non_object() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.methodReturnsNonObject"))
        }

        // localization/localize.ts:687
        /// Params: methodName.
        pub fn missing_super_call() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.missingSuperCall"))
        }

        // localization/localize.ts:689
        pub fn mixing_bytes_and_str() -> String {
            get_raw_string("Diagnostic.mixingBytesAndStr")
        }

        // localization/localize.ts:690
        pub fn module_as_type() -> String {
            get_raw_string("Diagnostic.moduleAsType")
        }

        // localization/localize.ts:691
        pub fn module_not_callable() -> String {
            get_raw_string("Diagnostic.moduleNotCallable")
        }

        // localization/localize.ts:692
        /// Params: memberName, moduleName.
        pub fn module_unknown_member() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.moduleUnknownMember"))
        }

        // localization/localize.ts:696
        pub fn named_except_after_catch_all() -> String {
            get_raw_string("Diagnostic.namedExceptAfterCatchAll")
        }

        // localization/localize.ts:697
        /// Params: name.
        pub fn named_param_after_param_spec_args() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.namedParamAfterParamSpecArgs"))
        }

        // localization/localize.ts:699
        pub fn named_tuple_empty_name() -> String {
            get_raw_string("Diagnostic.namedTupleEmptyName")
        }

        // localization/localize.ts:700
        /// Params: name, className.
        pub fn named_tuple_entry_redeclared() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.namedTupleEntryRedeclared"))
        }

        // localization/localize.ts:704
        pub fn named_tuple_field_underscore() -> String {
            get_raw_string("Diagnostic.namedTupleFieldUnderscore")
        }

        // localization/localize.ts:705
        pub fn named_tuple_first_arg() -> String {
            get_raw_string("Diagnostic.namedTupleFirstArg")
        }

        // localization/localize.ts:706
        pub fn named_tuple_multiple_inheritance() -> String {
            get_raw_string("Diagnostic.namedTupleMultipleInheritance")
        }

        // localization/localize.ts:707
        pub fn named_tuple_name_keyword() -> String {
            get_raw_string("Diagnostic.namedTupleNameKeyword")
        }

        // localization/localize.ts:708
        pub fn named_tuple_name_type() -> String {
            get_raw_string("Diagnostic.namedTupleNameType")
        }

        // localization/localize.ts:709
        pub fn named_tuple_name_unique() -> String {
            get_raw_string("Diagnostic.namedTupleNameUnique")
        }

        // localization/localize.ts:710
        pub fn named_tuple_no_types() -> String {
            get_raw_string("Diagnostic.namedTupleNoTypes")
        }

        // localization/localize.ts:711
        pub fn named_tuple_second_arg() -> String {
            get_raw_string("Diagnostic.namedTupleSecondArg")
        }

        // localization/localize.ts:712
        pub fn new_cls_param() -> String {
            get_raw_string("Diagnostic.newClsParam")
        }

        // localization/localize.ts:713
        pub fn new_type_any_or_unknown() -> String {
            get_raw_string("Diagnostic.newTypeAnyOrUnknown")
        }

        // localization/localize.ts:714
        pub fn new_type_bad_name() -> String {
            get_raw_string("Diagnostic.newTypeBadName")
        }

        // localization/localize.ts:715
        pub fn new_type_literal() -> String {
            get_raw_string("Diagnostic.newTypeLiteral")
        }

        // localization/localize.ts:716
        pub fn new_type_name_mismatch() -> String {
            get_raw_string("Diagnostic.newTypeNameMismatch")
        }

        // localization/localize.ts:717
        pub fn new_type_not_a_class() -> String {
            get_raw_string("Diagnostic.newTypeNotAClass")
        }

        // localization/localize.ts:718
        pub fn new_type_param_count() -> String {
            get_raw_string("Diagnostic.newTypeParamCount")
        }

        // localization/localize.ts:719
        pub fn new_type_protocol_class() -> String {
            get_raw_string("Diagnostic.newTypeProtocolClass")
        }

        // localization/localize.ts:720
        pub fn non_default_after_default() -> String {
            get_raw_string("Diagnostic.nonDefaultAfterDefault")
        }

        // localization/localize.ts:721
        pub fn none_not_callable() -> String {
            get_raw_string("Diagnostic.noneNotCallable")
        }

        // localization/localize.ts:722
        pub fn none_not_iterable() -> String {
            get_raw_string("Diagnostic.noneNotIterable")
        }

        // localization/localize.ts:723
        pub fn none_not_subscriptable() -> String {
            get_raw_string("Diagnostic.noneNotSubscriptable")
        }

        // localization/localize.ts:724
        pub fn none_not_usable_with() -> String {
            get_raw_string("Diagnostic.noneNotUsableWith")
        }

        // localization/localize.ts:725
        pub fn none_not_usable_with_async() -> String {
            get_raw_string("Diagnostic.noneNotUsableWithAsync")
        }

        // localization/localize.ts:726
        /// Params: operator.
        pub fn none_operator() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.noneOperator"))
        }

        // localization/localize.ts:728
        /// Params: name.
        pub fn none_unknown_member() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.noneUnknownMember"))
        }

        // localization/localize.ts:730
        pub fn non_local_in_module() -> String {
            get_raw_string("Diagnostic.nonLocalInModule")
        }

        // localization/localize.ts:731
        /// Params: name.
        pub fn non_local_no_binding() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.nonLocalNoBinding"))
        }

        // localization/localize.ts:733
        /// Params: name.
        pub fn non_local_reassignment() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.nonLocalReassignment"))
        }

        // localization/localize.ts:735
        /// Params: name.
        pub fn non_local_redefinition() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.nonLocalRedefinition"))
        }

        // localization/localize.ts:737
        /// Params: name.
        pub fn no_overload() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.noOverload"))
        }

        // localization/localize.ts:739
        /// Params: name.
        pub fn nonlocal_type_param() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.nonlocalTypeParam"))
        }

        // localization/localize.ts:741
        pub fn no_return_contains_return() -> String {
            get_raw_string("Diagnostic.noReturnContainsReturn")
        }

        // localization/localize.ts:742
        pub fn no_return_contains_yield() -> String {
            get_raw_string("Diagnostic.noReturnContainsYield")
        }

        // localization/localize.ts:743
        pub fn no_return_returns_none() -> String {
            get_raw_string("Diagnostic.noReturnReturnsNone")
        }

        // localization/localize.ts:744
        pub fn not_required_arg_count() -> String {
            get_raw_string("Diagnostic.notRequiredArgCount")
        }

        // localization/localize.ts:745
        pub fn not_required_not_in_typed_dict() -> String {
            get_raw_string("Diagnostic.notRequiredNotInTypedDict")
        }

        // localization/localize.ts:746
        /// Params: type.
        pub fn object_not_callable() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.objectNotCallable"))
        }

        // localization/localize.ts:748
        /// Params: name.
        pub fn obscured_class_declaration() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.obscuredClassDeclaration"))
        }

        // localization/localize.ts:750
        /// Params: name.
        pub fn obscured_function_declaration() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.obscuredFunctionDeclaration"))
        }

        // localization/localize.ts:752
        /// Params: name.
        pub fn obscured_method_declaration() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.obscuredMethodDeclaration"))
        }

        // localization/localize.ts:754
        /// Params: name.
        pub fn obscured_parameter_declaration() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.obscuredParameterDeclaration"))
        }

        // localization/localize.ts:756
        /// Params: name.
        pub fn obscured_type_alias_declaration() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.obscuredTypeAliasDeclaration"))
        }

        // localization/localize.ts:758
        /// Params: name.
        pub fn obscured_variable_declaration() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.obscuredVariableDeclaration"))
        }

        // localization/localize.ts:760
        pub fn operator_less_or_greater_deprecated() -> String {
            get_raw_string("Diagnostic.operatorLessOrGreaterDeprecated")
        }

        // localization/localize.ts:761
        pub fn optional_extra_args() -> String {
            get_raw_string("Diagnostic.optionalExtraArgs")
        }

        // localization/localize.ts:762
        pub fn or_pattern_irrefutable() -> String {
            get_raw_string("Diagnostic.orPatternIrrefutable")
        }

        // localization/localize.ts:763
        pub fn or_pattern_missing_name() -> String {
            get_raw_string("Diagnostic.orPatternMissingName")
        }

        // localization/localize.ts:764
        /// Params: names.
        pub fn overlapping_keyword_args() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.overlappingKeywordArgs"))
        }

        // localization/localize.ts:766
        /// Params: name, obscured, obscuredBy.
        pub fn overlapping_overload() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.overlappingOverload"))
        }

        // localization/localize.ts:770
        /// Params: name.
        pub fn overload_abstract_mismatch() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.overloadAbstractMismatch"))
        }

        // localization/localize.ts:772
        /// Params: name.
        pub fn overload_abstract_impl_mismatch() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.overloadAbstractImplMismatch"))
        }

        // localization/localize.ts:774
        /// Params: name.
        pub fn overload_class_method_inconsistent() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.overloadClassMethodInconsistent"))
        }

        // localization/localize.ts:776
        pub fn overload_final_impl() -> String {
            get_raw_string("Diagnostic.overloadFinalImpl")
        }

        // localization/localize.ts:777
        pub fn overload_final_no_impl() -> String {
            get_raw_string("Diagnostic.overloadFinalNoImpl")
        }

        // localization/localize.ts:778
        /// Params: name, index.
        pub fn overload_implementation_mismatch() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.overloadImplementationMismatch"))
        }

        // localization/localize.ts:782
        pub fn overload_override_impl() -> String {
            get_raw_string("Diagnostic.overloadOverrideImpl")
        }

        // localization/localize.ts:783
        pub fn overload_override_no_impl() -> String {
            get_raw_string("Diagnostic.overloadOverrideNoImpl")
        }

        // localization/localize.ts:784
        /// Params: name, newIndex, prevIndex.
        pub fn overload_return_type_mismatch() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.overloadReturnTypeMismatch"))
        }

        // localization/localize.ts:788
        /// Params: name.
        pub fn overload_static_method_inconsistent() -> ParameterizedString {
            ParameterizedString::new(get_raw_string(
                "Diagnostic.overloadStaticMethodInconsistent",
            ))
        }

        // localization/localize.ts:790
        /// Params: name.
        pub fn overload_without_implementation() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.overloadWithoutImplementation"))
        }

        // localization/localize.ts:792
        /// Params: name.
        pub fn overridden_method_not_found() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.overriddenMethodNotFound"))
        }

        // localization/localize.ts:794
        /// Params: name, className.
        pub fn override_decorator_missing() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.overrideDecoratorMissing"))
        }

        // localization/localize.ts:798
        pub fn param_after_kwargs_param() -> String {
            get_raw_string("Diagnostic.paramAfterKwargsParam")
        }

        // localization/localize.ts:799
        /// Params: name.
        pub fn param_already_assigned() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.paramAlreadyAssigned"))
        }

        // localization/localize.ts:801
        /// Params: name.
        pub fn param_annotation_missing() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.paramAnnotationMissing"))
        }

        // localization/localize.ts:803
        /// Params: name.
        pub fn param_name_missing() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.paramNameMissing"))
        }

        // localization/localize.ts:805
        /// Params: type.
        pub fn param_spec_args_kwargs_duplicate() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.paramSpecArgsKwargsDuplicate"))
        }

        // localization/localize.ts:807
        pub fn param_spec_args_kwargs_usage() -> String {
            get_raw_string("Diagnostic.paramSpecArgsKwargsUsage")
        }

        // localization/localize.ts:808
        /// Params: type.
        pub fn param_spec_args_missing() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.paramSpecArgsMissing"))
        }

        // localization/localize.ts:810
        pub fn param_spec_args_usage() -> String {
            get_raw_string("Diagnostic.paramSpecArgsUsage")
        }

        // localization/localize.ts:811
        /// Params: name.
        pub fn param_spec_assigned_name() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.paramSpecAssignedName"))
        }

        // localization/localize.ts:813
        pub fn param_spec_context() -> String {
            get_raw_string("Diagnostic.paramSpecContext")
        }

        // localization/localize.ts:814
        pub fn param_spec_default_not_tuple() -> String {
            get_raw_string("Diagnostic.paramSpecDefaultNotTuple")
        }

        // localization/localize.ts:815
        pub fn param_spec_first_arg() -> String {
            get_raw_string("Diagnostic.paramSpecFirstArg")
        }

        // localization/localize.ts:816
        pub fn param_spec_kwargs_usage() -> String {
            get_raw_string("Diagnostic.paramSpecKwargsUsage")
        }

        // localization/localize.ts:817
        /// Params: name.
        pub fn param_spec_not_used_by_outer_scope() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.paramSpecNotUsedByOuterScope"))
        }

        // localization/localize.ts:819
        pub fn param_spec_unknown_arg() -> String {
            get_raw_string("Diagnostic.paramSpecUnknownArg")
        }

        // localization/localize.ts:820
        /// Params: name.
        pub fn param_spec_unknown_member() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.paramSpecUnknownMember"))
        }

        // localization/localize.ts:822
        /// Params: name.
        pub fn param_spec_unknown_param() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.paramSpecUnknownParam"))
        }

        // localization/localize.ts:824
        pub fn param_type_covariant() -> String {
            get_raw_string("Diagnostic.paramTypeCovariant")
        }

        // localization/localize.ts:825
        /// Params: paramName.
        pub fn param_type_unknown() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.paramTypeUnknown"))
        }

        // localization/localize.ts:827
        /// Params: sourceType, paramType.
        pub fn param_assignment_mismatch() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.paramAssignmentMismatch"))
        }

        // localization/localize.ts:831
        /// Params: paramName.
        pub fn param_type_partially_unknown() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.paramTypePartiallyUnknown"))
        }

        // localization/localize.ts:833
        pub fn parenthesized_context_manager_illegal() -> String {
            get_raw_string("Diagnostic.parenthesizedContextManagerIllegal")
        }

        // localization/localize.ts:835
        /// Params: type.
        pub fn pattern_never_matches() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.patternNeverMatches"))
        }

        // localization/localize.ts:837
        pub fn position_arg_after_named_arg() -> String {
            get_raw_string("Diagnostic.positionArgAfterNamedArg")
        }

        // localization/localize.ts:838
        pub fn position_arg_after_unpacked_dict_arg() -> String {
            get_raw_string("Diagnostic.positionArgAfterUnpackedDictArg")
        }

        // localization/localize.ts:839
        /// Params: name, module.
        pub fn private_import_from_py_typed_module() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.privateImportFromPyTypedModule"))
        }

        // localization/localize.ts:843
        pub fn position_only_after_args() -> String {
            get_raw_string("Diagnostic.positionOnlyAfterArgs")
        }

        // localization/localize.ts:844
        pub fn position_only_after_keyword_only() -> String {
            get_raw_string("Diagnostic.positionOnlyAfterKeywordOnly")
        }

        // localization/localize.ts:845
        pub fn position_only_after_non() -> String {
            get_raw_string("Diagnostic.positionOnlyAfterNon")
        }

        // localization/localize.ts:846
        pub fn position_only_incompatible() -> String {
            get_raw_string("Diagnostic.positionOnlyIncompatible")
        }

        // localization/localize.ts:847
        pub fn position_only_first_param() -> String {
            get_raw_string("Diagnostic.positionOnlyFirstParam")
        }

        // localization/localize.ts:848
        /// Params: name.
        pub fn private_used_outside_of_class() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.privateUsedOutsideOfClass"))
        }

        // localization/localize.ts:850
        /// Params: name.
        pub fn private_used_outside_of_module() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.privateUsedOutsideOfModule"))
        }

        // localization/localize.ts:852
        /// Params: name, className.
        pub fn property_overridden() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.propertyOverridden"))
        }

        // localization/localize.ts:854
        pub fn property_static_method() -> String {
            get_raw_string("Diagnostic.propertyStaticMethod")
        }

        // localization/localize.ts:855
        /// Params: name.
        pub fn protected_used_outside_of_class() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.protectedUsedOutsideOfClass"))
        }

        // localization/localize.ts:857
        /// Params: classType, baseType.
        pub fn protocol_base_class() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.protocolBaseClass"))
        }

        // localization/localize.ts:861
        pub fn protocol_base_class_with_type_args() -> String {
            get_raw_string("Diagnostic.protocolBaseClassWithTypeArgs")
        }

        // localization/localize.ts:862
        pub fn protocol_illegal() -> String {
            get_raw_string("Diagnostic.protocolIllegal")
        }

        // localization/localize.ts:863
        pub fn protocol_not_allowed() -> String {
            get_raw_string("Diagnostic.protocolNotAllowed")
        }

        // localization/localize.ts:864
        pub fn protocol_type_arg_must_be_type_param() -> String {
            get_raw_string("Diagnostic.protocolTypeArgMustBeTypeParam")
        }

        // localization/localize.ts:865
        /// Params: name.
        pub fn protocol_unsafe_overlap() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.protocolUnsafeOverlap"))
        }

        // localization/localize.ts:867
        /// Params: variable, class.
        pub fn protocol_variance_contravariant() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.protocolVarianceContravariant"))
        }

        // localization/localize.ts:871
        /// Params: variable, class.
        pub fn protocol_variance_covariant() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.protocolVarianceCovariant"))
        }

        // localization/localize.ts:875
        /// Params: variable, class.
        pub fn protocol_variance_invariant() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.protocolVarianceInvariant"))
        }

        // localization/localize.ts:879
        pub fn pyright_comment_invalid_diagnostic_bool_value() -> String {
            get_raw_string("Diagnostic.pyrightCommentInvalidDiagnosticBoolValue")
        }

        // localization/localize.ts:881
        pub fn pyright_comment_invalid_diagnostic_severity_value() -> String {
            get_raw_string("Diagnostic.pyrightCommentInvalidDiagnosticSeverityValue")
        }

        // localization/localize.ts:883
        pub fn pyright_comment_missing_directive() -> String {
            get_raw_string("Diagnostic.pyrightCommentMissingDirective")
        }

        // localization/localize.ts:884
        pub fn pyright_comment_not_on_own_line() -> String {
            get_raw_string("Diagnostic.pyrightCommentNotOnOwnLine")
        }

        // localization/localize.ts:885
        /// Params: directive.
        pub fn pyright_comment_unknown_directive() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.pyrightCommentUnknownDirective"))
        }

        // localization/localize.ts:887
        /// Params: rule.
        pub fn pyright_comment_unknown_diagnostic_rule() -> ParameterizedString {
            ParameterizedString::new(get_raw_string(
                "Diagnostic.pyrightCommentUnknownDiagnosticRule",
            ))
        }

        // localization/localize.ts:889
        pub fn read_only_arg_count() -> String {
            get_raw_string("Diagnostic.readOnlyArgCount")
        }

        // localization/localize.ts:890
        pub fn read_only_not_in_typed_dict() -> String {
            get_raw_string("Diagnostic.readOnlyNotInTypedDict")
        }

        // localization/localize.ts:891
        /// Params: name.
        pub fn recursive_definition() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.recursiveDefinition"))
        }

        // localization/localize.ts:893
        pub fn relative_import_not_allowed() -> String {
            get_raw_string("Diagnostic.relativeImportNotAllowed")
        }

        // localization/localize.ts:894
        pub fn required_arg_count() -> String {
            get_raw_string("Diagnostic.requiredArgCount")
        }

        // localization/localize.ts:895
        pub fn required_not_in_typed_dict() -> String {
            get_raw_string("Diagnostic.requiredNotInTypedDict")
        }

        // localization/localize.ts:896
        pub fn return_in_async_generator() -> String {
            get_raw_string("Diagnostic.returnInAsyncGenerator")
        }

        // localization/localize.ts:897
        /// Params: returnType.
        pub fn return_missing() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.returnMissing"))
        }

        // localization/localize.ts:899
        pub fn return_in_exception_group() -> String {
            get_raw_string("Diagnostic.returnInExceptionGroup")
        }

        // localization/localize.ts:900
        pub fn return_outside_function() -> String {
            get_raw_string("Diagnostic.returnOutsideFunction")
        }

        // localization/localize.ts:901
        pub fn return_type_contravariant() -> String {
            get_raw_string("Diagnostic.returnTypeContravariant")
        }

        // localization/localize.ts:902
        /// Params: exprType, returnType.
        pub fn return_type_mismatch() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.returnTypeMismatch"))
        }

        // localization/localize.ts:906
        pub fn return_type_unknown() -> String {
            get_raw_string("Diagnostic.returnTypeUnknown")
        }

        // localization/localize.ts:907
        /// Params: returnType.
        pub fn return_type_partially_unknown() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.returnTypePartiallyUnknown"))
        }

        // localization/localize.ts:909
        pub fn reveal_locals_args() -> String {
            get_raw_string("Diagnostic.revealLocalsArgs")
        }

        // localization/localize.ts:910
        pub fn reveal_locals_none() -> String {
            get_raw_string("Diagnostic.revealLocalsNone")
        }

        // localization/localize.ts:911
        pub fn reveal_type_args() -> String {
            get_raw_string("Diagnostic.revealTypeArgs")
        }

        // localization/localize.ts:912
        pub fn reveal_type_expected_text_arg() -> String {
            get_raw_string("Diagnostic.revealTypeExpectedTextArg")
        }

        // localization/localize.ts:913
        /// Params: expected, received.
        pub fn reveal_type_expected_text_mismatch() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.revealTypeExpectedTextMismatch"))
        }

        // localization/localize.ts:917
        /// Params: expected, received.
        pub fn reveal_type_expected_type_mismatch() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.revealTypeExpectedTypeMismatch"))
        }

        // localization/localize.ts:921
        pub fn self_type_context() -> String {
            get_raw_string("Diagnostic.selfTypeContext")
        }

        // localization/localize.ts:922
        pub fn self_type_metaclass() -> String {
            get_raw_string("Diagnostic.selfTypeMetaclass")
        }

        // localization/localize.ts:923
        pub fn self_type_with_typed_self_or_cls() -> String {
            get_raw_string("Diagnostic.selfTypeWithTypedSelfOrCls")
        }

        // localization/localize.ts:924
        pub fn sentinel_bad_name() -> String {
            get_raw_string("Diagnostic.sentinelBadName")
        }

        // localization/localize.ts:925
        pub fn sentinel_name_mismatch() -> String {
            get_raw_string("Diagnostic.sentinelNameMismatch")
        }

        // localization/localize.ts:926
        pub fn sentinel_param_count() -> String {
            get_raw_string("Diagnostic.sentinelParamCount")
        }

        // localization/localize.ts:927
        pub fn setter_getter_type_mismatch() -> String {
            get_raw_string("Diagnostic.setterGetterTypeMismatch")
        }

        // localization/localize.ts:928
        pub fn star_pattern_in_as_pattern() -> String {
            get_raw_string("Diagnostic.starPatternInAsPattern")
        }

        // localization/localize.ts:929
        pub fn star_pattern_in_or_pattern() -> String {
            get_raw_string("Diagnostic.starPatternInOrPattern")
        }

        // localization/localize.ts:930
        /// Params: name.
        pub fn single_overload() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.singleOverload"))
        }

        // localization/localize.ts:932
        /// Params: name.
        pub fn slots_attribute_error() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.slotsAttributeError"))
        }

        // localization/localize.ts:934
        /// Params: name.
        pub fn slots_class_var_conflict() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.slotsClassVarConflict"))
        }

        // localization/localize.ts:936
        pub fn star_star_wildcard_not_allowed() -> String {
            get_raw_string("Diagnostic.starStarWildcardNotAllowed")
        }

        // localization/localize.ts:937
        pub fn static_cls_self_param() -> String {
            get_raw_string("Diagnostic.staticClsSelfParam")
        }

        // localization/localize.ts:938
        pub fn string_non_ascii_bytes() -> String {
            get_raw_string("Diagnostic.stringNonAsciiBytes")
        }

        // localization/localize.ts:939
        pub fn string_not_subscriptable() -> String {
            get_raw_string("Diagnostic.stringNotSubscriptable")
        }

        // localization/localize.ts:940
        pub fn string_unsupported_escape() -> String {
            get_raw_string("Diagnostic.stringUnsupportedEscape")
        }

        // localization/localize.ts:941
        pub fn string_unterminated() -> String {
            get_raw_string("Diagnostic.stringUnterminated")
        }

        // localization/localize.ts:942
        /// Params: importName.
        pub fn stub_file_missing() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.stubFileMissing"))
        }

        // localization/localize.ts:944
        pub fn stub_uses_get_attr() -> String {
            get_raw_string("Diagnostic.stubUsesGetAttr")
        }

        // localization/localize.ts:945
        pub fn sublist_params_incompatible() -> String {
            get_raw_string("Diagnostic.sublistParamsIncompatible")
        }

        // localization/localize.ts:946
        pub fn super_call_arg_count() -> String {
            get_raw_string("Diagnostic.superCallArgCount")
        }

        // localization/localize.ts:947
        /// Params: type.
        pub fn super_call_first_arg() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.superCallFirstArg"))
        }

        // localization/localize.ts:949
        /// Params: type.
        pub fn super_call_second_arg() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.superCallSecondArg"))
        }

        // localization/localize.ts:951
        pub fn super_call_zero_arg_form() -> String {
            get_raw_string("Diagnostic.superCallZeroArgForm")
        }

        // localization/localize.ts:952
        pub fn super_call_zero_arg_form_static_method() -> String {
            get_raw_string("Diagnostic.superCallZeroArgFormStaticMethod")
        }

        // localization/localize.ts:954
        /// Params: name.
        pub fn symbol_is_unbound() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.symbolIsUnbound"))
        }

        // localization/localize.ts:956
        /// Params: name.
        pub fn symbol_is_undefined() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.symbolIsUndefined"))
        }

        // localization/localize.ts:958
        /// Params: name.
        pub fn symbol_is_possibly_unbound() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.symbolIsPossiblyUnbound"))
        }

        // localization/localize.ts:960
        /// Params: name, className.
        pub fn symbol_overridden() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.symbolOverridden"))
        }

        // localization/localize.ts:962
        pub fn template_string_bytes() -> String {
            get_raw_string("Diagnostic.templateStringBytes")
        }

        // localization/localize.ts:963
        pub fn template_string_illegal() -> String {
            get_raw_string("Diagnostic.templateStringIllegal")
        }

        // localization/localize.ts:964
        pub fn template_string_unicode() -> String {
            get_raw_string("Diagnostic.templateStringUnicode")
        }

        // localization/localize.ts:965
        pub fn ternary_not_allowed() -> String {
            get_raw_string("Diagnostic.ternaryNotAllowed")
        }

        // localization/localize.ts:966
        pub fn total_ordering_missing_method() -> String {
            get_raw_string("Diagnostic.totalOrderingMissingMethod")
        }

        // localization/localize.ts:967
        pub fn trailing_comma_in_from_import() -> String {
            get_raw_string("Diagnostic.trailingCommaInFromImport")
        }

        // localization/localize.ts:968
        pub fn try_without_except() -> String {
            get_raw_string("Diagnostic.tryWithoutExcept")
        }

        // localization/localize.ts:969
        /// Params: type.
        pub fn tuple_assignment_mismatch() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.tupleAssignmentMismatch"))
        }

        // localization/localize.ts:971
        pub fn tuple_in_annotation() -> String {
            get_raw_string("Diagnostic.tupleInAnnotation")
        }

        // localization/localize.ts:972
        /// Params: type, index.
        pub fn tuple_index_out_of_range() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.tupleIndexOutOfRange"))
        }

        // localization/localize.ts:974
        pub fn type_alias_illegal_expression_form() -> String {
            get_raw_string("Diagnostic.typeAliasIllegalExpressionForm")
        }

        // localization/localize.ts:975
        /// Params: name.
        pub fn type_alias_is_recursive_direct() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.typeAliasIsRecursiveDirect"))
        }

        // localization/localize.ts:977
        pub fn type_alias_not_in_module_or_class() -> String {
            get_raw_string("Diagnostic.typeAliasNotInModuleOrClass")
        }

        // localization/localize.ts:978
        /// Params: name.
        pub fn type_alias_redeclared() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.typeAliasRedeclared"))
        }

        // localization/localize.ts:980
        pub fn type_alias_statement_illegal() -> String {
            get_raw_string("Diagnostic.typeAliasStatementIllegal")
        }

        // localization/localize.ts:981
        pub fn type_alias_statement_bad_scope() -> String {
            get_raw_string("Diagnostic.typeAliasStatementBadScope")
        }

        // localization/localize.ts:982
        pub fn type_alias_type_bad_scope() -> String {
            get_raw_string("Diagnostic.typeAliasTypeBadScope")
        }

        // localization/localize.ts:983
        pub fn type_alias_type_base_class() -> String {
            get_raw_string("Diagnostic.typeAliasTypeBaseClass")
        }

        // localization/localize.ts:984
        pub fn type_alias_type_must_be_assigned() -> String {
            get_raw_string("Diagnostic.typeAliasTypeMustBeAssigned")
        }

        // localization/localize.ts:985
        pub fn type_alias_type_name_arg() -> String {
            get_raw_string("Diagnostic.typeAliasTypeNameArg")
        }

        // localization/localize.ts:986
        pub fn type_alias_type_name_mismatch() -> String {
            get_raw_string("Diagnostic.typeAliasTypeNameMismatch")
        }

        // localization/localize.ts:987
        pub fn type_alias_type_param_invalid() -> String {
            get_raw_string("Diagnostic.typeAliasTypeParamInvalid")
        }

        // localization/localize.ts:988
        pub fn type_annotation_call() -> String {
            get_raw_string("Diagnostic.typeAnnotationCall")
        }

        // localization/localize.ts:989
        pub fn type_annotation_variable() -> String {
            get_raw_string("Diagnostic.typeAnnotationVariable")
        }

        // localization/localize.ts:990
        pub fn type_annotation_with_callable() -> String {
            get_raw_string("Diagnostic.typeAnnotationWithCallable")
        }

        // localization/localize.ts:991
        pub fn type_arg_list_expected() -> String {
            get_raw_string("Diagnostic.typeArgListExpected")
        }

        // localization/localize.ts:992
        pub fn type_arg_list_not_allowed() -> String {
            get_raw_string("Diagnostic.typeArgListNotAllowed")
        }

        // localization/localize.ts:993
        /// Params: name.
        pub fn type_args_expecting_none() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.typeArgsExpectingNone"))
        }

        // localization/localize.ts:995
        /// Params: received.
        pub fn type_args_mismatch_one() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.typeArgsMismatchOne"))
        }

        // localization/localize.ts:997
        /// Params: name.
        pub fn type_args_missing_for_alias() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.typeArgsMissingForAlias"))
        }

        // localization/localize.ts:999
        /// Params: name.
        pub fn type_args_missing_for_class() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.typeArgsMissingForClass"))
        }

        // localization/localize.ts:1001
        /// Params: name, expected, received.
        pub fn type_args_too_few() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.typeArgsTooFew"))
        }

        // localization/localize.ts:1005
        /// Params: name, expected, received.
        pub fn type_args_too_many() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.typeArgsTooMany"))
        }

        // localization/localize.ts:1009
        /// Params: sourceType, destType.
        pub fn type_assignment_mismatch() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.typeAssignmentMismatch"))
        }

        // localization/localize.ts:1013
        /// Params: name, sourceType, destType.
        pub fn type_assignment_mismatch_wildcard() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.typeAssignmentMismatchWildcard"))
        }

        // localization/localize.ts:1017
        pub fn type_call_not_allowed() -> String {
            get_raw_string("Diagnostic.typeCallNotAllowed")
        }

        // localization/localize.ts:1018
        /// Params: name.
        pub fn type_check_only() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.typeCheckOnly"))
        }

        // localization/localize.ts:1020
        pub fn type_comment_deprecated() -> String {
            get_raw_string("Diagnostic.typeCommentDeprecated")
        }

        // localization/localize.ts:1021
        pub fn typed_dict_access() -> String {
            get_raw_string("Diagnostic.typedDictAccess")
        }

        // localization/localize.ts:1022
        /// Params: name.
        pub fn typed_dict_assigned_name() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.typedDictAssignedName"))
        }

        // localization/localize.ts:1024
        pub fn typed_dict_bad_var() -> String {
            get_raw_string("Diagnostic.typedDictBadVar")
        }

        // localization/localize.ts:1025
        pub fn typed_dict_base_class() -> String {
            get_raw_string("Diagnostic.typedDictBaseClass")
        }

        // localization/localize.ts:1026
        /// Params: name.
        pub fn typed_dict_bool_param() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.typedDictBoolParam"))
        }

        // localization/localize.ts:1028
        /// Params: name, type.
        pub fn typed_dict_closed_extras() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.typedDictClosedExtras"))
        }

        // localization/localize.ts:1030
        /// Params: name.
        pub fn typed_dict_closed_no_extras() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.typedDictClosedNoExtras"))
        }

        // localization/localize.ts:1032
        /// Params: name.
        pub fn typed_dict_closed_false_non_open_base() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.typedDictClosedFalseNonOpenBase"))
        }

        // localization/localize.ts:1034
        pub fn typed_dict_delete() -> String {
            get_raw_string("Diagnostic.typedDictDelete")
        }

        // localization/localize.ts:1035
        pub fn typed_dict_empty_name() -> String {
            get_raw_string("Diagnostic.typedDictEmptyName")
        }

        // localization/localize.ts:1036
        pub fn typed_dict_entry_name() -> String {
            get_raw_string("Diagnostic.typedDictEntryName")
        }

        // localization/localize.ts:1037
        pub fn typed_dict_entry_unique() -> String {
            get_raw_string("Diagnostic.typedDictEntryUnique")
        }

        // localization/localize.ts:1038
        pub fn typed_dict_extra_args() -> String {
            get_raw_string("Diagnostic.typedDictExtraArgs")
        }

        // localization/localize.ts:1039
        pub fn typed_dict_extra_items_closed() -> String {
            get_raw_string("Diagnostic.typedDictExtraItemsClosed")
        }

        // localization/localize.ts:1040
        /// Params: name.
        pub fn typed_dict_field_not_required_redefinition() -> ParameterizedString {
            ParameterizedString::new(get_raw_string(
                "Diagnostic.typedDictFieldNotRequiredRedefinition",
            ))
        }

        // localization/localize.ts:1042
        /// Params: name.
        pub fn typed_dict_field_read_only_redefinition() -> ParameterizedString {
            ParameterizedString::new(get_raw_string(
                "Diagnostic.typedDictFieldReadOnlyRedefinition",
            ))
        }

        // localization/localize.ts:1044
        /// Params: name.
        pub fn typed_dict_field_required_redefinition() -> ParameterizedString {
            ParameterizedString::new(get_raw_string(
                "Diagnostic.typedDictFieldRequiredRedefinition",
            ))
        }

        // localization/localize.ts:1046
        pub fn typed_dict_first_arg() -> String {
            get_raw_string("Diagnostic.typedDictFirstArg")
        }

        // localization/localize.ts:1047
        pub fn typed_dict_in_class_pattern() -> String {
            get_raw_string("Diagnostic.typedDictInClassPattern")
        }

        // localization/localize.ts:1048
        /// Params: name.
        pub fn typed_dict_initsubclass_parameter() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.typedDictInitsubclassParameter"))
        }

        // localization/localize.ts:1050
        pub fn typed_dict_not_allowed() -> String {
            get_raw_string("Diagnostic.typedDictNotAllowed")
        }

        // localization/localize.ts:1051
        pub fn typed_dict_second_arg_dict() -> String {
            get_raw_string("Diagnostic.typedDictSecondArgDict")
        }

        // localization/localize.ts:1052
        pub fn typed_dict_second_arg_dict_entry() -> String {
            get_raw_string("Diagnostic.typedDictSecondArgDictEntry")
        }

        // localization/localize.ts:1053
        pub fn typed_dict_set() -> String {
            get_raw_string("Diagnostic.typedDictSet")
        }

        // localization/localize.ts:1054
        /// Params: type.
        pub fn type_expected_class() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.typeExpectedClass"))
        }

        // localization/localize.ts:1056
        pub fn type_form_args() -> String {
            get_raw_string("Diagnostic.typeFormArgs")
        }

        // localization/localize.ts:1057
        pub fn type_guard_arg_count() -> String {
            get_raw_string("Diagnostic.typeGuardArgCount")
        }

        // localization/localize.ts:1058
        pub fn type_guard_param_count() -> String {
            get_raw_string("Diagnostic.typeGuardParamCount")
        }

        // localization/localize.ts:1059
        /// Params: type, returnType.
        pub fn type_is_return_type() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.typeIsReturnType"))
        }

        // localization/localize.ts:1061
        /// Params: type.
        pub fn type_not_awaitable() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.typeNotAwaitable"))
        }

        // localization/localize.ts:1063
        /// Params: type.
        pub fn type_not_intantiable() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.typeNotIntantiable"))
        }

        // localization/localize.ts:1065
        /// Params: type.
        pub fn type_not_iterable() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.typeNotIterable"))
        }

        // localization/localize.ts:1067
        /// Params: type.
        pub fn type_not_specializable() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.typeNotSpecializable"))
        }

        // localization/localize.ts:1069
        /// Params: type.
        pub fn type_not_subscriptable() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.typeNotSubscriptable"))
        }

        // localization/localize.ts:1071
        /// Params: type, method.
        pub fn type_not_usable_with() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.typeNotUsableWith"))
        }

        // localization/localize.ts:1073
        /// Params: type, method.
        pub fn type_not_usable_with_async() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.typeNotUsableWithAsync"))
        }

        // localization/localize.ts:1077
        /// Params: leftType, rightType, operator.
        pub fn type_not_support_binary_operator() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.typeNotSupportBinaryOperator"))
        }

        // localization/localize.ts:1081
        /// Params: leftType, rightType, expectedType, operator.
        pub fn type_not_support_binary_operator_bidirectional() -> ParameterizedString {
            ParameterizedString::new(get_raw_string(
                "Diagnostic.typeNotSupportBinaryOperatorBidirectional",
            ))
        }

        // localization/localize.ts:1085
        /// Params: type, operator.
        pub fn type_not_support_unary_operator() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.typeNotSupportUnaryOperator"))
        }

        // localization/localize.ts:1089
        /// Params: type, expectedType, operator.
        pub fn type_not_support_unary_operator_bidirectional() -> ParameterizedString {
            ParameterizedString::new(get_raw_string(
                "Diagnostic.typeNotSupportUnaryOperatorBidirectional",
            ))
        }

        // localization/localize.ts:1093
        pub fn type_parameter_bound_not_allowed() -> String {
            get_raw_string("Diagnostic.typeParameterBoundNotAllowed")
        }

        // localization/localize.ts:1094
        pub fn type_parameter_constraint_tuple() -> String {
            get_raw_string("Diagnostic.typeParameterConstraintTuple")
        }

        // localization/localize.ts:1095
        /// Params: name.
        pub fn type_parameter_existing_type_parameter() -> ParameterizedString {
            ParameterizedString::new(get_raw_string(
                "Diagnostic.typeParameterExistingTypeParameter",
            ))
        }

        // localization/localize.ts:1097
        pub fn type_parameters_missing() -> String {
            get_raw_string("Diagnostic.typeParametersMissing")
        }

        // localization/localize.ts:1098
        /// Params: name, container.
        pub fn type_parameter_not_declared() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.typeParameterNotDeclared"))
        }

        // localization/localize.ts:1102
        /// Params: name.
        pub fn type_partially_unknown() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.typePartiallyUnknown"))
        }

        // localization/localize.ts:1104
        /// Params: name.
        pub fn type_unknown() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.typeUnknown"))
        }

        // localization/localize.ts:1106
        /// Params: name.
        pub fn type_var_assigned_name() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.typeVarAssignedName"))
        }

        // localization/localize.ts:1108
        /// Params: type, name.
        pub fn type_var_assignment_mismatch() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.typeVarAssignmentMismatch"))
        }

        // localization/localize.ts:1112
        pub fn type_var_bound_and_constrained() -> String {
            get_raw_string("Diagnostic.typeVarBoundAndConstrained")
        }

        // localization/localize.ts:1113
        pub fn type_var_bound_generic() -> String {
            get_raw_string("Diagnostic.typeVarBoundGeneric")
        }

        // localization/localize.ts:1114
        pub fn type_var_constraint_generic() -> String {
            get_raw_string("Diagnostic.typeVarConstraintGeneric")
        }

        // localization/localize.ts:1115
        pub fn type_var_default_bound_mismatch() -> String {
            get_raw_string("Diagnostic.typeVarDefaultBoundMismatch")
        }

        // localization/localize.ts:1116
        pub fn type_var_default_constraint_mismatch() -> String {
            get_raw_string("Diagnostic.typeVarDefaultConstraintMismatch")
        }

        // localization/localize.ts:1118
        pub fn type_var_default_illegal() -> String {
            get_raw_string("Diagnostic.typeVarDefaultIllegal")
        }

        // localization/localize.ts:1119
        /// Params: name.
        pub fn type_var_default_invalid_type_var() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.typeVarDefaultInvalidTypeVar"))
        }

        // localization/localize.ts:1121
        pub fn type_var_first_arg() -> String {
            get_raw_string("Diagnostic.typeVarFirstArg")
        }

        // localization/localize.ts:1122
        /// Params: name.
        pub fn type_var_invalid_for_member_variable() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.typeVarInvalidForMemberVariable"))
        }

        // localization/localize.ts:1124
        /// Params: type, name.
        pub fn type_var_no_member() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.typeVarNoMember"))
        }

        // localization/localize.ts:1126
        /// Params: type.
        pub fn type_var_not_subscriptable() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.typeVarNotSubscriptable"))
        }

        // localization/localize.ts:1128
        /// Params: name.
        pub fn type_var_not_used_by_outer_scope() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.typeVarNotUsedByOuterScope"))
        }

        // localization/localize.ts:1130
        /// Params: name, param.
        pub fn type_var_possibly_unsolvable() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.typeVarPossiblyUnsolvable"))
        }

        // localization/localize.ts:1134
        pub fn type_var_single_constraint() -> String {
            get_raw_string("Diagnostic.typeVarSingleConstraint")
        }

        // localization/localize.ts:1135
        pub fn type_vars_not_in_generic_or_protocol() -> String {
            get_raw_string("Diagnostic.typeVarsNotInGenericOrProtocol")
        }

        // localization/localize.ts:1136
        pub fn type_var_tuple_context() -> String {
            get_raw_string("Diagnostic.typeVarTupleContext")
        }

        // localization/localize.ts:1137
        pub fn type_var_tuple_default_not_unpacked() -> String {
            get_raw_string("Diagnostic.typeVarTupleDefaultNotUnpacked")
        }

        // localization/localize.ts:1138
        pub fn type_var_tuple_must_be_unpacked() -> String {
            get_raw_string("Diagnostic.typeVarTupleMustBeUnpacked")
        }

        // localization/localize.ts:1139
        pub fn type_var_tuple_constraints() -> String {
            get_raw_string("Diagnostic.typeVarTupleConstraints")
        }

        // localization/localize.ts:1140
        /// Params: name.
        pub fn type_var_tuple_unknown_param() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.typeVarTupleUnknownParam"))
        }

        // localization/localize.ts:1142
        /// Params: name.
        pub fn type_var_unknown_param() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.typeVarUnknownParam"))
        }

        // localization/localize.ts:1144
        /// Params: name.
        pub fn type_var_used_by_outer_scope() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.typeVarUsedByOuterScope"))
        }

        // localization/localize.ts:1146
        /// Params: name.
        pub fn type_var_used_only_once() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.typeVarUsedOnlyOnce"))
        }

        // localization/localize.ts:1148
        pub fn type_var_variance() -> String {
            get_raw_string("Diagnostic.typeVarVariance")
        }

        // localization/localize.ts:1149
        /// Params: variadicName, typeVarName.
        pub fn type_var_with_default_follows_variadic() -> ParameterizedString {
            ParameterizedString::new(get_raw_string(
                "Diagnostic.typeVarWithDefaultFollowsVariadic",
            ))
        }

        // localization/localize.ts:1153
        /// Params: name, other.
        pub fn type_var_without_default() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.typeVarWithoutDefault"))
        }

        // localization/localize.ts:1155
        /// Params: name.
        pub fn unaccessed_class() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.unaccessedClass"))
        }

        // localization/localize.ts:1157
        /// Params: name.
        pub fn unaccessed_function() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.unaccessedFunction"))
        }

        // localization/localize.ts:1159
        /// Params: name.
        pub fn unaccessed_import() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.unaccessedImport"))
        }

        // localization/localize.ts:1161
        /// Params: name.
        pub fn unaccessed_symbol() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.unaccessedSymbol"))
        }

        // localization/localize.ts:1163
        /// Params: name.
        pub fn unaccessed_variable() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.unaccessedVariable"))
        }

        // localization/localize.ts:1165
        /// Params: name.
        pub fn unannotated_function_skipped() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.unannotatedFunctionSkipped"))
        }

        // localization/localize.ts:1167
        pub fn unary_operation_not_allowed() -> String {
            get_raw_string("Diagnostic.unaryOperationNotAllowed")
        }

        // localization/localize.ts:1168
        pub fn unexpected_async_token() -> String {
            get_raw_string("Diagnostic.unexpectedAsyncToken")
        }

        // localization/localize.ts:1169
        pub fn unexpected_eof() -> String {
            get_raw_string("Diagnostic.unexpectedEof")
        }

        // localization/localize.ts:1170
        pub fn unexpected_expr_token() -> String {
            get_raw_string("Diagnostic.unexpectedExprToken")
        }

        // localization/localize.ts:1171
        pub fn unexpected_indent() -> String {
            get_raw_string("Diagnostic.unexpectedIndent")
        }

        // localization/localize.ts:1172
        pub fn unexpected_unindent() -> String {
            get_raw_string("Diagnostic.unexpectedUnindent")
        }

        // localization/localize.ts:1173
        pub fn unhashable_dict_key() -> String {
            get_raw_string("Diagnostic.unhashableDictKey")
        }

        // localization/localize.ts:1174
        pub fn unhashable_set_entry() -> String {
            get_raw_string("Diagnostic.unhashableSetEntry")
        }

        // localization/localize.ts:1175
        pub fn union_forward_reference_not_allowed() -> String {
            get_raw_string("Diagnostic.unionForwardReferenceNotAllowed")
        }

        // localization/localize.ts:1176
        pub fn union_syntax_illegal() -> String {
            get_raw_string("Diagnostic.unionSyntaxIllegal")
        }

        // localization/localize.ts:1177
        pub fn union_type_arg_count() -> String {
            get_raw_string("Diagnostic.unionTypeArgCount")
        }

        // localization/localize.ts:1178
        /// Params: classType.
        pub fn uninitialized_abstract_variables() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.uninitializedAbstractVariables"))
        }

        // localization/localize.ts:1180
        /// Params: name.
        pub fn uninitialized_instance_variable() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.uninitializedInstanceVariable"))
        }

        // localization/localize.ts:1182
        pub fn union_unpacked_tuple() -> String {
            get_raw_string("Diagnostic.unionUnpackedTuple")
        }

        // localization/localize.ts:1183
        pub fn union_unpacked_type_var_tuple() -> String {
            get_raw_string("Diagnostic.unionUnpackedTypeVarTuple")
        }

        // localization/localize.ts:1184
        /// Params: type.
        pub fn unnecessary_cast() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.unnecessaryCast"))
        }

        // localization/localize.ts:1186
        /// Params: testType, classType.
        pub fn unnecessary_is_instance_always() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.unnecessaryIsInstanceAlways"))
        }

        // localization/localize.ts:1190
        /// Params: testType, classType.
        pub fn unnecessary_is_subclass_always() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.unnecessaryIsSubclassAlways"))
        }

        // localization/localize.ts:1194
        /// Params: testType, classType.
        pub fn unnecessary_is_instance_never() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.unnecessaryIsInstanceNever"))
        }

        // localization/localize.ts:1198
        /// Params: testType, classType.
        pub fn unnecessary_is_subclass_never() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.unnecessaryIsSubclassNever"))
        }

        // localization/localize.ts:1202
        pub fn unnecessary_pyright_ignore() -> String {
            get_raw_string("Diagnostic.unnecessaryPyrightIgnore")
        }

        // localization/localize.ts:1203
        /// Params: name.
        pub fn unnecessary_pyright_ignore_rule() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.unnecessaryPyrightIgnoreRule"))
        }

        // localization/localize.ts:1205
        pub fn unnecessary_type_ignore() -> String {
            get_raw_string("Diagnostic.unnecessaryTypeIgnore")
        }

        // localization/localize.ts:1206
        pub fn unpack_arg_count() -> String {
            get_raw_string("Diagnostic.unpackArgCount")
        }

        // localization/localize.ts:1207
        pub fn unpacked_arg_in_type_argument() -> String {
            get_raw_string("Diagnostic.unpackedArgInTypeArgument")
        }

        // localization/localize.ts:1208
        pub fn unpacked_arg_with_variadic_param() -> String {
            get_raw_string("Diagnostic.unpackedArgWithVariadicParam")
        }

        // localization/localize.ts:1209
        pub fn unpacked_dict_argument_not_mapping() -> String {
            get_raw_string("Diagnostic.unpackedDictArgumentNotMapping")
        }

        // localization/localize.ts:1210
        pub fn unpacked_dict_subscript_illegal() -> String {
            get_raw_string("Diagnostic.unpackedDictSubscriptIllegal")
        }

        // localization/localize.ts:1211
        pub fn unpacked_subscript_illegal() -> String {
            get_raw_string("Diagnostic.unpackedSubscriptIllegal")
        }

        // localization/localize.ts:1212
        pub fn unpacked_typed_dict_argument() -> String {
            get_raw_string("Diagnostic.unpackedTypedDictArgument")
        }

        // localization/localize.ts:1213
        /// Params: name1, name2.
        pub fn unpacked_type_var_tuple_expected() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.unpackedTypeVarTupleExpected"))
        }

        // localization/localize.ts:1217
        pub fn unpack_expected_typed_dict() -> String {
            get_raw_string("Diagnostic.unpackExpectedTypedDict")
        }

        // localization/localize.ts:1218
        pub fn unpack_expected_type_var_tuple() -> String {
            get_raw_string("Diagnostic.unpackExpectedTypeVarTuple")
        }

        // localization/localize.ts:1219
        pub fn unpack_illegal_in_comprehension() -> String {
            get_raw_string("Diagnostic.unpackIllegalInComprehension")
        }

        // localization/localize.ts:1220
        pub fn unpack_in_annotation() -> String {
            get_raw_string("Diagnostic.unpackInAnnotation")
        }

        // localization/localize.ts:1221
        pub fn unpack_in_dict() -> String {
            get_raw_string("Diagnostic.unpackInDict")
        }

        // localization/localize.ts:1222
        pub fn unpack_in_set() -> String {
            get_raw_string("Diagnostic.unpackInSet")
        }

        // localization/localize.ts:1223
        pub fn unpack_not_allowed() -> String {
            get_raw_string("Diagnostic.unpackNotAllowed")
        }

        // localization/localize.ts:1224
        pub fn unpack_operator_not_allowed() -> String {
            get_raw_string("Diagnostic.unpackOperatorNotAllowed")
        }

        // localization/localize.ts:1225
        pub fn unpack_tuples_illegal() -> String {
            get_raw_string("Diagnostic.unpackTuplesIllegal")
        }

        // localization/localize.ts:1226
        pub fn unreachable_code_condition() -> String {
            get_raw_string("Diagnostic.unreachableCodeCondition")
        }

        // localization/localize.ts:1227
        pub fn unreachable_code_structure() -> String {
            get_raw_string("Diagnostic.unreachableCodeStructure")
        }

        // localization/localize.ts:1228
        pub fn unreachable_code_type() -> String {
            get_raw_string("Diagnostic.unreachableCodeType")
        }

        // localization/localize.ts:1229
        pub fn unreachable_except() -> String {
            get_raw_string("Diagnostic.unreachableExcept")
        }

        // localization/localize.ts:1230
        pub fn unsupported_dunder_all_operation() -> String {
            get_raw_string("Diagnostic.unsupportedDunderAllOperation")
        }

        // localization/localize.ts:1231
        /// Params: type.
        pub fn unused_call_result() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.unusedCallResult"))
        }

        // localization/localize.ts:1233
        pub fn unused_coroutine() -> String {
            get_raw_string("Diagnostic.unusedCoroutine")
        }

        // localization/localize.ts:1234
        pub fn unused_expression() -> String {
            get_raw_string("Diagnostic.unusedExpression")
        }

        // localization/localize.ts:1235
        pub fn var_annotation_illegal() -> String {
            get_raw_string("Diagnostic.varAnnotationIllegal")
        }

        // localization/localize.ts:1236
        /// Params: className, name.
        pub fn variable_final_override() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.variableFinalOverride"))
        }

        // localization/localize.ts:1240
        pub fn variadic_type_args_too_many() -> String {
            get_raw_string("Diagnostic.variadicTypeArgsTooMany")
        }

        // localization/localize.ts:1241
        /// Params: names.
        pub fn variadic_type_param_too_many_alias() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.variadicTypeParamTooManyAlias"))
        }

        // localization/localize.ts:1243
        /// Params: names.
        pub fn variadic_type_param_too_many_class() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Diagnostic.variadicTypeParamTooManyClass"))
        }

        // localization/localize.ts:1245
        pub fn walrus_illegal() -> String {
            get_raw_string("Diagnostic.walrusIllegal")
        }

        // localization/localize.ts:1246
        pub fn walrus_not_allowed() -> String {
            get_raw_string("Diagnostic.walrusNotAllowed")
        }

        // localization/localize.ts:1247
        pub fn walrus_not_allowed_in_comprehension() -> String {
            get_raw_string("Diagnostic.walrusNotAllowedInComprehension")
        }

        // localization/localize.ts:1248
        pub fn wildcard_in_function() -> String {
            get_raw_string("Diagnostic.wildcardInFunction")
        }

        // localization/localize.ts:1249
        pub fn wildcard_pattern_type_unknown() -> String {
            get_raw_string("Diagnostic.wildcardPatternTypeUnknown")
        }

        // localization/localize.ts:1250
        pub fn wildcard_pattern_type_partially_unknown() -> String {
            get_raw_string("Diagnostic.wildcardPatternTypePartiallyUnknown")
        }

        // localization/localize.ts:1252
        pub fn wildcard_library_import() -> String {
            get_raw_string("Diagnostic.wildcardLibraryImport")
        }

        // localization/localize.ts:1253
        pub fn yield_from_illegal() -> String {
            get_raw_string("Diagnostic.yieldFromIllegal")
        }

        // localization/localize.ts:1254
        pub fn yield_from_outside_async() -> String {
            get_raw_string("Diagnostic.yieldFromOutsideAsync")
        }

        // localization/localize.ts:1255
        pub fn yield_outside_function() -> String {
            get_raw_string("Diagnostic.yieldOutsideFunction")
        }

        // localization/localize.ts:1256
        pub fn yield_within_comprehension() -> String {
            get_raw_string("Diagnostic.yieldWithinComprehension")
        }

        // localization/localize.ts:1257
        pub fn zero_case_statements_found() -> String {
            get_raw_string("Diagnostic.zeroCaseStatementsFound")
        }

        // localization/localize.ts:1258
        pub fn zero_length_tuple_not_allowed() -> String {
            get_raw_string("Diagnostic.zeroLengthTupleNotAllowed")
        }
    }

    #[allow(non_snake_case)]
    pub mod DiagnosticAddendum {
        #[allow(unused_imports)]
        use super::super::{ParameterizedString, get_raw_string};

        // localization/localize.ts:1262
        pub fn annotated_not_allowed() -> String {
            get_raw_string("DiagnosticAddendum.annotatedNotAllowed")
        }

        // localization/localize.ts:1263
        /// Params: paramName.
        pub fn arg_param() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.argParam"))
        }

        // localization/localize.ts:1265
        /// Params: paramName, functionName.
        pub fn arg_param_function() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.argParamFunction"))
        }

        // localization/localize.ts:1269
        /// Params: paramName.
        pub fn args_param_missing() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.argsParamMissing"))
        }

        // localization/localize.ts:1271
        /// Params: expected, received.
        pub fn args_position_only() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.argsPositionOnly"))
        }

        // localization/localize.ts:1275
        /// Params: type.
        pub fn argument_type() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.argumentType"))
        }

        // localization/localize.ts:1277
        /// Params: types.
        pub fn argument_types() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.argumentTypes"))
        }

        // localization/localize.ts:1279
        pub fn assign_to_none() -> String {
            get_raw_string("DiagnosticAddendum.assignToNone")
        }

        // localization/localize.ts:1280
        pub fn async_help() -> String {
            get_raw_string("DiagnosticAddendum.asyncHelp")
        }

        // localization/localize.ts:1281
        /// Params: baseClass, type.
        pub fn base_class_incompatible() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.baseClassIncompatible"))
        }

        // localization/localize.ts:1285
        /// Params: baseClass, subclass, type.
        pub fn base_class_incompatible_subclass() -> ParameterizedString {
            ParameterizedString::new(get_raw_string(
                "DiagnosticAddendum.baseClassIncompatibleSubclass",
            ))
        }

        // localization/localize.ts:1289
        /// Params: baseClass, type.
        pub fn base_class_overridden_type() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.baseClassOverriddenType"))
        }

        // localization/localize.ts:1293
        /// Params: baseClass, type.
        pub fn base_class_overrides_type() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.baseClassOverridesType"))
        }

        // localization/localize.ts:1297
        pub fn bytes_type_promotions() -> String {
            get_raw_string("DiagnosticAddendum.bytesTypePromotions")
        }

        // localization/localize.ts:1298
        /// Params: operandType, boolReturnType.
        pub fn conditional_requires_bool() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.conditionalRequiresBool"))
        }

        // localization/localize.ts:1302
        /// Params: name.
        pub fn data_class_frozen() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.dataClassFrozen"))
        }

        // localization/localize.ts:1304
        pub fn data_class_field_location() -> String {
            get_raw_string("DiagnosticAddendum.dataClassFieldLocation")
        }

        // localization/localize.ts:1305
        /// Params: name.
        pub fn data_protocol_unsupported() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.dataProtocolUnsupported"))
        }

        // localization/localize.ts:1307
        /// Params: name, className.
        pub fn descriptor_access_binding_failed() -> ParameterizedString {
            ParameterizedString::new(get_raw_string(
                "DiagnosticAddendum.descriptorAccessBindingFailed",
            ))
        }

        // localization/localize.ts:1311
        /// Params: name, className.
        pub fn descriptor_access_call_failed() -> ParameterizedString {
            ParameterizedString::new(get_raw_string(
                "DiagnosticAddendum.descriptorAccessCallFailed",
            ))
        }

        // localization/localize.ts:1315
        pub fn final_method() -> String {
            get_raw_string("DiagnosticAddendum.finalMethod")
        }

        // localization/localize.ts:1316
        /// Params: name.
        pub fn function_param_default_missing() -> ParameterizedString {
            ParameterizedString::new(get_raw_string(
                "DiagnosticAddendum.functionParamDefaultMissing",
            ))
        }

        // localization/localize.ts:1318
        /// Params: destName, srcName.
        pub fn function_param_name() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.functionParamName"))
        }

        // localization/localize.ts:1322
        /// Params: name.
        pub fn function_param_position_only() -> ParameterizedString {
            ParameterizedString::new(get_raw_string(
                "DiagnosticAddendum.functionParamPositionOnly",
            ))
        }

        // localization/localize.ts:1324
        /// Params: sourceType, destType.
        pub fn function_return_type_mismatch() -> ParameterizedString {
            ParameterizedString::new(get_raw_string(
                "DiagnosticAddendum.functionReturnTypeMismatch",
            ))
        }

        // localization/localize.ts:1328
        /// Params: expected, received.
        pub fn function_too_few_params() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.functionTooFewParams"))
        }

        // localization/localize.ts:1332
        pub fn generic_class_not_allowed() -> String {
            get_raw_string("DiagnosticAddendum.genericClassNotAllowed")
        }

        // localization/localize.ts:1333
        pub fn incompatible_getter() -> String {
            get_raw_string("DiagnosticAddendum.incompatibleGetter")
        }

        // localization/localize.ts:1334
        pub fn incompatible_setter() -> String {
            get_raw_string("DiagnosticAddendum.incompatibleSetter")
        }

        // localization/localize.ts:1335
        pub fn incompatible_deleter() -> String {
            get_raw_string("DiagnosticAddendum.incompatibleDeleter")
        }

        // localization/localize.ts:1336
        /// Params: type.
        pub fn init_method_location() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.initMethodLocation"))
        }

        // localization/localize.ts:1338
        /// Params: type.
        pub fn init_method_signature() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.initMethodSignature"))
        }

        // localization/localize.ts:1340
        /// Params: name.
        pub fn init_subclass_location() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.initSubclassLocation"))
        }

        // localization/localize.ts:1342
        pub fn invariant_suggestion_dict() -> String {
            get_raw_string("DiagnosticAddendum.invariantSuggestionDict")
        }

        // localization/localize.ts:1343
        pub fn invariant_suggestion_list() -> String {
            get_raw_string("DiagnosticAddendum.invariantSuggestionList")
        }

        // localization/localize.ts:1344
        pub fn invariant_suggestion_set() -> String {
            get_raw_string("DiagnosticAddendum.invariantSuggestionSet")
        }

        // localization/localize.ts:1345
        /// Params: type.
        pub fn isinstance_class_not_supported() -> ParameterizedString {
            ParameterizedString::new(get_raw_string(
                "DiagnosticAddendum.isinstanceClassNotSupported",
            ))
        }

        // localization/localize.ts:1347
        /// Params: expected, received.
        pub fn function_too_many_params() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.functionTooManyParams"))
        }

        // localization/localize.ts:1351
        /// Params: name, type.
        pub fn key_not_required() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.keyNotRequired"))
        }

        // localization/localize.ts:1353
        /// Params: name, type.
        pub fn key_read_only() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.keyReadOnly"))
        }

        // localization/localize.ts:1355
        /// Params: name.
        pub fn key_required_deleted() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.keyRequiredDeleted"))
        }

        // localization/localize.ts:1357
        /// Params: name, type.
        pub fn key_undefined() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.keyUndefined"))
        }

        // localization/localize.ts:1359
        /// Params: paramName.
        pub fn kwargs_param_missing() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.kwargsParamMissing"))
        }

        // localization/localize.ts:1361
        /// Params: type.
        pub fn list_assignment_mismatch() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.listAssignmentMismatch"))
        }

        // localization/localize.ts:1363
        /// Params: sourceType, destType.
        pub fn literal_assignment_mismatch() -> ParameterizedString {
            ParameterizedString::new(get_raw_string(
                "DiagnosticAddendum.literalAssignmentMismatch",
            ))
        }

        // localization/localize.ts:1367
        pub fn literal_not_allowed() -> String {
            get_raw_string("DiagnosticAddendum.literalNotAllowed")
        }

        // localization/localize.ts:1368
        /// Params: type.
        pub fn match_is_not_exhaustive_type() -> ParameterizedString {
            ParameterizedString::new(get_raw_string(
                "DiagnosticAddendum.matchIsNotExhaustiveType",
            ))
        }

        // localization/localize.ts:1370
        pub fn match_is_not_exhaustive_hint() -> String {
            get_raw_string("DiagnosticAddendum.matchIsNotExhaustiveHint")
        }

        // localization/localize.ts:1371
        /// Params: type, name, classType.
        pub fn member_assignment() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.memberAssignment"))
        }

        // localization/localize.ts:1375
        /// Params: type, name.
        pub fn member_is_abstract() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.memberIsAbstract"))
        }

        // localization/localize.ts:1379
        /// Params: count.
        pub fn member_is_abstract_more() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.memberIsAbstractMore"))
        }

        // localization/localize.ts:1381
        /// Params: name.
        pub fn member_is_class_var_in_protocol() -> ParameterizedString {
            ParameterizedString::new(get_raw_string(
                "DiagnosticAddendum.memberIsClassVarInProtocol",
            ))
        }

        // localization/localize.ts:1383
        /// Params: name.
        pub fn member_is_init_var() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.memberIsInitVar"))
        }

        // localization/localize.ts:1385
        /// Params: name.
        pub fn member_is_invariant() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.memberIsInvariant"))
        }

        // localization/localize.ts:1387
        /// Params: name.
        pub fn member_is_not_class_var_in_class() -> ParameterizedString {
            ParameterizedString::new(get_raw_string(
                "DiagnosticAddendum.memberIsNotClassVarInClass",
            ))
        }

        // localization/localize.ts:1389
        /// Params: name.
        pub fn member_is_not_class_var_in_protocol() -> ParameterizedString {
            ParameterizedString::new(get_raw_string(
                "DiagnosticAddendum.memberIsNotClassVarInProtocol",
            ))
        }

        // localization/localize.ts:1391
        /// Params: name.
        pub fn member_is_not_read_only_in_protocol() -> ParameterizedString {
            ParameterizedString::new(get_raw_string(
                "DiagnosticAddendum.memberIsNotReadOnlyInProtocol",
            ))
        }

        // localization/localize.ts:1393
        /// Params: name.
        pub fn member_is_read_only_in_protocol() -> ParameterizedString {
            ParameterizedString::new(get_raw_string(
                "DiagnosticAddendum.memberIsReadOnlyInProtocol",
            ))
        }

        // localization/localize.ts:1395
        /// Params: name.
        pub fn member_is_writable_in_protocol() -> ParameterizedString {
            ParameterizedString::new(get_raw_string(
                "DiagnosticAddendum.memberIsWritableInProtocol",
            ))
        }

        // localization/localize.ts:1397
        /// Params: name.
        pub fn member_set_class_var() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.memberSetClassVar"))
        }

        // localization/localize.ts:1399
        /// Params: name.
        pub fn member_type_mismatch() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.memberTypeMismatch"))
        }

        // localization/localize.ts:1401
        /// Params: name.
        pub fn member_unknown() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.memberUnknown"))
        }

        // localization/localize.ts:1403
        /// Params: metaclass1, metaclass2.
        pub fn metaclass_conflict() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.metaclassConflict"))
        }

        // localization/localize.ts:1407
        pub fn missing_getter() -> String {
            get_raw_string("DiagnosticAddendum.missingGetter")
        }

        // localization/localize.ts:1408
        pub fn missing_setter() -> String {
            get_raw_string("DiagnosticAddendum.missingSetter")
        }

        // localization/localize.ts:1409
        pub fn missing_deleter() -> String {
            get_raw_string("DiagnosticAddendum.missingDeleter")
        }

        // localization/localize.ts:1410
        /// Params: name.
        pub fn named_param_missing_in_dest() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.namedParamMissingInDest"))
        }

        // localization/localize.ts:1412
        /// Params: name.
        pub fn named_param_missing_in_source() -> ParameterizedString {
            ParameterizedString::new(get_raw_string(
                "DiagnosticAddendum.namedParamMissingInSource",
            ))
        }

        // localization/localize.ts:1414
        /// Params: name, sourceType, destType.
        pub fn named_param_type_mismatch() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.namedParamTypeMismatch"))
        }

        // localization/localize.ts:1418
        pub fn named_tuple_not_allowed() -> String {
            get_raw_string("DiagnosticAddendum.namedTupleNotAllowed")
        }

        // localization/localize.ts:1419
        /// Params: type.
        pub fn new_method_location() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.newMethodLocation"))
        }

        // localization/localize.ts:1421
        /// Params: type.
        pub fn new_method_signature() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.newMethodSignature"))
        }

        // localization/localize.ts:1423
        pub fn none_not_allowed() -> String {
            get_raw_string("DiagnosticAddendum.noneNotAllowed")
        }

        // localization/localize.ts:1424
        pub fn new_type_class_not_allowed() -> String {
            get_raw_string("DiagnosticAddendum.newTypeClassNotAllowed")
        }

        // localization/localize.ts:1425
        /// Params: type.
        pub fn no_overload_assignable() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.noOverloadAssignable"))
        }

        // localization/localize.ts:1427
        /// Params: name.
        pub fn or_pattern_missing_name() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.orPatternMissingName"))
        }

        // localization/localize.ts:1429
        /// Params: index.
        pub fn overload_index() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.overloadIndex"))
        }

        // localization/localize.ts:1431
        pub fn overload_signature() -> String {
            get_raw_string("DiagnosticAddendum.overloadSignature")
        }

        // localization/localize.ts:1432
        /// Params: name.
        pub fn overload_not_assignable() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.overloadNotAssignable"))
        }

        // localization/localize.ts:1434
        pub fn overridden_method() -> String {
            get_raw_string("DiagnosticAddendum.overriddenMethod")
        }

        // localization/localize.ts:1435
        pub fn overridden_symbol() -> String {
            get_raw_string("DiagnosticAddendum.overriddenSymbol")
        }

        // localization/localize.ts:1436
        pub fn override_is_invariant() -> String {
            get_raw_string("DiagnosticAddendum.overrideIsInvariant")
        }

        // localization/localize.ts:1437
        /// Params: overrideType, baseType.
        pub fn override_invariant_mismatch() -> ParameterizedString {
            ParameterizedString::new(get_raw_string(
                "DiagnosticAddendum.overrideInvariantMismatch",
            ))
        }

        // localization/localize.ts:1441
        pub fn override_no_overload_matches() -> String {
            get_raw_string("DiagnosticAddendum.overrideNoOverloadMatches")
        }

        // localization/localize.ts:1442
        pub fn override_not_class_method() -> String {
            get_raw_string("DiagnosticAddendum.overrideNotClassMethod")
        }

        // localization/localize.ts:1443
        pub fn override_not_instance_method() -> String {
            get_raw_string("DiagnosticAddendum.overrideNotInstanceMethod")
        }

        // localization/localize.ts:1444
        pub fn override_not_static_method() -> String {
            get_raw_string("DiagnosticAddendum.overrideNotStaticMethod")
        }

        // localization/localize.ts:1445
        pub fn override_overload_no_match() -> String {
            get_raw_string("DiagnosticAddendum.overrideOverloadNoMatch")
        }

        // localization/localize.ts:1446
        pub fn override_overload_order() -> String {
            get_raw_string("DiagnosticAddendum.overrideOverloadOrder")
        }

        // localization/localize.ts:1447
        /// Params: name.
        pub fn override_param_keyword_no_default() -> ParameterizedString {
            ParameterizedString::new(get_raw_string(
                "DiagnosticAddendum.overrideParamKeywordNoDefault",
            ))
        }

        // localization/localize.ts:1449
        /// Params: name, baseType, overrideType.
        pub fn override_param_keyword_type() -> ParameterizedString {
            ParameterizedString::new(get_raw_string(
                "DiagnosticAddendum.overrideParamKeywordType",
            ))
        }

        // localization/localize.ts:1453
        /// Params: index, baseName, overrideName.
        pub fn override_param_name() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.overrideParamName"))
        }

        // localization/localize.ts:1457
        /// Params: name.
        pub fn override_param_name_extra() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.overrideParamNameExtra"))
        }

        // localization/localize.ts:1459
        /// Params: name.
        pub fn override_param_name_missing() -> ParameterizedString {
            ParameterizedString::new(get_raw_string(
                "DiagnosticAddendum.overrideParamNameMissing",
            ))
        }

        // localization/localize.ts:1461
        /// Params: index, baseName.
        pub fn override_param_name_position_only() -> ParameterizedString {
            ParameterizedString::new(get_raw_string(
                "DiagnosticAddendum.overrideParamNamePositionOnly",
            ))
        }

        // localization/localize.ts:1465
        /// Params: index.
        pub fn override_param_no_default() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.overrideParamNoDefault"))
        }

        // localization/localize.ts:1467
        /// Params: index, baseType, overrideType.
        pub fn override_param_type() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.overrideParamType"))
        }

        // localization/localize.ts:1471
        /// Params: baseCount, overrideCount.
        pub fn override_positional_param_count() -> ParameterizedString {
            ParameterizedString::new(get_raw_string(
                "DiagnosticAddendum.overridePositionalParamCount",
            ))
        }

        // localization/localize.ts:1475
        /// Params: baseType, overrideType.
        pub fn override_return_type() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.overrideReturnType"))
        }

        // localization/localize.ts:1479
        /// Params: type.
        pub fn override_type() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.overrideType"))
        }

        // localization/localize.ts:1481
        /// Params: index, sourceType, destType.
        pub fn param_assignment() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.paramAssignment"))
        }

        // localization/localize.ts:1485
        pub fn param_spec_missing_in_override() -> String {
            get_raw_string("DiagnosticAddendum.paramSpecMissingInOverride")
        }

        // localization/localize.ts:1486
        /// Params: paramType.
        pub fn param_type() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.paramType"))
        }

        // localization/localize.ts:1488
        /// Params: module.
        pub fn private_import_from_py_typed_source() -> ParameterizedString {
            ParameterizedString::new(get_raw_string(
                "DiagnosticAddendum.privateImportFromPyTypedSource",
            ))
        }

        // localization/localize.ts:1492
        pub fn property_access_from_protocol_class() -> String {
            get_raw_string("DiagnosticAddendum.propertyAccessFromProtocolClass")
        }

        // localization/localize.ts:1494
        /// Params: name.
        pub fn property_method_incompatible() -> ParameterizedString {
            ParameterizedString::new(get_raw_string(
                "DiagnosticAddendum.propertyMethodIncompatible",
            ))
        }

        // localization/localize.ts:1496
        /// Params: name.
        pub fn property_method_missing() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.propertyMethodMissing"))
        }

        // localization/localize.ts:1498
        /// Params: name.
        pub fn property_missing_deleter() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.propertyMissingDeleter"))
        }

        // localization/localize.ts:1500
        /// Params: name.
        pub fn property_missing_setter() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.propertyMissingSetter"))
        }

        // localization/localize.ts:1502
        /// Params: sourceType, destType.
        pub fn protocol_incompatible() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.protocolIncompatible"))
        }

        // localization/localize.ts:1506
        /// Params: name.
        pub fn protocol_member_missing() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.protocolMemberMissing"))
        }

        // localization/localize.ts:1508
        pub fn protocol_requires_runtime_checkable() -> String {
            get_raw_string("DiagnosticAddendum.protocolRequiresRuntimeCheckable")
        }

        // localization/localize.ts:1510
        /// Params: sourceType, destType.
        pub fn protocol_source_is_not_concrete() -> ParameterizedString {
            ParameterizedString::new(get_raw_string(
                "DiagnosticAddendum.protocolSourceIsNotConcrete",
            ))
        }

        // localization/localize.ts:1514
        /// Params: name.
        pub fn protocol_unsafe_overlap() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.protocolUnsafeOverlap"))
        }

        // localization/localize.ts:1516
        pub fn pyright_comment_ignore_tip() -> String {
            get_raw_string("DiagnosticAddendum.pyrightCommentIgnoreTip")
        }

        // localization/localize.ts:1517
        /// Params: name.
        pub fn read_only_attribute() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.readOnlyAttribute"))
        }

        // localization/localize.ts:1519
        pub fn see_declaration() -> String {
            get_raw_string("DiagnosticAddendum.seeDeclaration")
        }

        // localization/localize.ts:1520
        pub fn see_class_declaration() -> String {
            get_raw_string("DiagnosticAddendum.seeClassDeclaration")
        }

        // localization/localize.ts:1521
        pub fn see_function_declaration() -> String {
            get_raw_string("DiagnosticAddendum.seeFunctionDeclaration")
        }

        // localization/localize.ts:1522
        pub fn see_method_declaration() -> String {
            get_raw_string("DiagnosticAddendum.seeMethodDeclaration")
        }

        // localization/localize.ts:1523
        pub fn see_parameter_declaration() -> String {
            get_raw_string("DiagnosticAddendum.seeParameterDeclaration")
        }

        // localization/localize.ts:1524
        pub fn see_type_alias_declaration() -> String {
            get_raw_string("DiagnosticAddendum.seeTypeAliasDeclaration")
        }

        // localization/localize.ts:1525
        pub fn see_variable_declaration() -> String {
            get_raw_string("DiagnosticAddendum.seeVariableDeclaration")
        }

        // localization/localize.ts:1526
        /// Params: entry.
        pub fn tuple_entry_type_mismatch() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.tupleEntryTypeMismatch"))
        }

        // localization/localize.ts:1528
        /// Params: type.
        pub fn tuple_assignment_mismatch() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.tupleAssignmentMismatch"))
        }

        // localization/localize.ts:1530
        /// Params: expected.
        pub fn tuple_size_indeterminate_src() -> ParameterizedString {
            ParameterizedString::new(get_raw_string(
                "DiagnosticAddendum.tupleSizeIndeterminateSrc",
            ))
        }

        // localization/localize.ts:1532
        /// Params: expected.
        pub fn tuple_size_indeterminate_src_dest() -> ParameterizedString {
            ParameterizedString::new(get_raw_string(
                "DiagnosticAddendum.tupleSizeIndeterminateSrcDest",
            ))
        }

        // localization/localize.ts:1536
        /// Params: expected, received.
        pub fn tuple_size_mismatch() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.tupleSizeMismatch"))
        }

        // localization/localize.ts:1540
        /// Params: expected, received.
        pub fn tuple_size_mismatch_indeterminate_dest() -> ParameterizedString {
            ParameterizedString::new(get_raw_string(
                "DiagnosticAddendum.tupleSizeMismatchIndeterminateDest",
            ))
        }

        // localization/localize.ts:1544
        pub fn type_alias_instance_check() -> String {
            get_raw_string("DiagnosticAddendum.typeAliasInstanceCheck")
        }

        // localization/localize.ts:1545
        /// Params: sourceType, destType.
        pub fn type_assignment_mismatch() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.typeAssignmentMismatch"))
        }

        // localization/localize.ts:1549
        /// Params: sourceType, destType, name.
        pub fn type_bound() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.typeBound"))
        }

        // localization/localize.ts:1553
        /// Params: type, name.
        pub fn type_constrained_type_var() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.typeConstrainedTypeVar"))
        }

        // localization/localize.ts:1557
        /// Params: type.
        pub fn typed_dict_base_class() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.typedDictBaseClass"))
        }

        // localization/localize.ts:1559
        pub fn typed_dict_class_not_allowed() -> String {
            get_raw_string("DiagnosticAddendum.typedDictClassNotAllowed")
        }

        // localization/localize.ts:1560
        /// Params: name, type.
        pub fn typed_dict_extra_field_not_allowed() -> ParameterizedString {
            ParameterizedString::new(get_raw_string(
                "DiagnosticAddendum.typedDictExtraFieldNotAllowed",
            ))
        }

        // localization/localize.ts:1564
        /// Params: name, type.
        pub fn typed_dict_extra_field_type_mismatch() -> ParameterizedString {
            ParameterizedString::new(get_raw_string(
                "DiagnosticAddendum.typedDictExtraFieldTypeMismatch",
            ))
        }

        // localization/localize.ts:1568
        /// Params: name, type.
        pub fn typed_dict_field_missing() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.typedDictFieldMissing"))
        }

        // localization/localize.ts:1572
        /// Params: name.
        pub fn typed_dict_closed_extra_not_allowed() -> ParameterizedString {
            ParameterizedString::new(get_raw_string(
                "DiagnosticAddendum.typedDictClosedExtraNotAllowed",
            ))
        }

        // localization/localize.ts:1576
        /// Params: name, type.
        pub fn typed_dict_closed_extra_type_mismatch() -> ParameterizedString {
            ParameterizedString::new(get_raw_string(
                "DiagnosticAddendum.typedDictClosedExtraTypeMismatch",
            ))
        }

        // localization/localize.ts:1580
        /// Params: name.
        pub fn typed_dict_closed_field_not_read_only() -> ParameterizedString {
            ParameterizedString::new(get_raw_string(
                "DiagnosticAddendum.typedDictClosedFieldNotReadOnly",
            ))
        }

        // localization/localize.ts:1584
        /// Params: name.
        pub fn typed_dict_closed_field_not_required() -> ParameterizedString {
            ParameterizedString::new(get_raw_string(
                "DiagnosticAddendum.typedDictClosedFieldNotRequired",
            ))
        }

        // localization/localize.ts:1588
        /// Params: name, type.
        pub fn typed_dict_field_not_read_only() -> ParameterizedString {
            ParameterizedString::new(get_raw_string(
                "DiagnosticAddendum.typedDictFieldNotReadOnly",
            ))
        }

        // localization/localize.ts:1592
        /// Params: name, type.
        pub fn typed_dict_field_not_required() -> ParameterizedString {
            ParameterizedString::new(get_raw_string(
                "DiagnosticAddendum.typedDictFieldNotRequired",
            ))
        }

        // localization/localize.ts:1596
        /// Params: name, type.
        pub fn typed_dict_field_required() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.typedDictFieldRequired"))
        }

        // localization/localize.ts:1600
        /// Params: name, type.
        pub fn typed_dict_field_type_mismatch() -> ParameterizedString {
            ParameterizedString::new(get_raw_string(
                "DiagnosticAddendum.typedDictFieldTypeMismatch",
            ))
        }

        // localization/localize.ts:1604
        /// Params: name, type.
        pub fn typed_dict_field_undefined() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.typedDictFieldUndefined"))
        }

        // localization/localize.ts:1608
        /// Params: name.
        pub fn typed_dict_key_access() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.typedDictKeyAccess"))
        }

        // localization/localize.ts:1610
        pub fn typed_dict_not_allowed() -> String {
            get_raw_string("DiagnosticAddendum.typedDictNotAllowed")
        }

        // localization/localize.ts:1611
        /// Params: sourceType, destType.
        pub fn type_incompatible() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.typeIncompatible"))
        }

        // localization/localize.ts:1615
        /// Params: type.
        pub fn type_not_class() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.typeNotClass"))
        }

        // localization/localize.ts:1617
        /// Params: type, name.
        pub fn type_param_spec() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.typeParamSpec"))
        }

        // localization/localize.ts:1619
        /// Params: type.
        pub fn type_not_string_literal() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.typeNotStringLiteral"))
        }

        // localization/localize.ts:1621
        /// Params: name, type.
        pub fn type_of_symbol() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.typeOfSymbol"))
        }

        // localization/localize.ts:1623
        /// Params: type.
        pub fn type_unsupported() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.typeUnsupported"))
        }

        // localization/localize.ts:1625
        /// Params: name.
        pub fn type_var_default_out_of_scope() -> ParameterizedString {
            ParameterizedString::new(get_raw_string(
                "DiagnosticAddendum.typeVarDefaultOutOfScope",
            ))
        }

        // localization/localize.ts:1627
        /// Params: name, sourceType, destType.
        pub fn type_var_is_contravariant() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.typeVarIsContravariant"))
        }

        // localization/localize.ts:1631
        /// Params: name, sourceType, destType.
        pub fn type_var_is_covariant() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.typeVarIsCovariant"))
        }

        // localization/localize.ts:1635
        /// Params: name, sourceType, destType.
        pub fn type_var_is_invariant() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.typeVarIsInvariant"))
        }

        // localization/localize.ts:1639
        /// Params: names.
        pub fn type_vars_missing() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.typeVarsMissing"))
        }

        // localization/localize.ts:1641
        pub fn type_var_not_allowed() -> String {
            get_raw_string("DiagnosticAddendum.typeVarNotAllowed")
        }

        // localization/localize.ts:1642
        pub fn type_var_tuple_requires_known_length() -> String {
            get_raw_string("DiagnosticAddendum.typeVarTupleRequiresKnownLength")
        }

        // localization/localize.ts:1644
        /// Params: type.
        pub fn type_var_unnecessary_suggestion() -> ParameterizedString {
            ParameterizedString::new(get_raw_string(
                "DiagnosticAddendum.typeVarUnnecessarySuggestion",
            ))
        }

        // localization/localize.ts:1646
        pub fn type_var_unsolvable_remedy() -> String {
            get_raw_string("DiagnosticAddendum.typeVarUnsolvableRemedy")
        }

        // localization/localize.ts:1647
        /// Params: type.
        pub fn unhashable_type() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.unhashableType"))
        }

        // localization/localize.ts:1649
        /// Params: name, classType.
        pub fn uninitialized_abstract_variable() -> ParameterizedString {
            ParameterizedString::new(get_raw_string(
                "DiagnosticAddendum.uninitializedAbstractVariable",
            ))
        }

        // localization/localize.ts:1653
        /// Params: exceptionType, parentType.
        pub fn unreachable_except() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("DiagnosticAddendum.unreachableExcept"))
        }

        // localization/localize.ts:1657
        pub fn use_dict_instead() -> String {
            get_raw_string("DiagnosticAddendum.useDictInstead")
        }

        // localization/localize.ts:1658
        pub fn use_list_instead() -> String {
            get_raw_string("DiagnosticAddendum.useListInstead")
        }

        // localization/localize.ts:1659
        pub fn use_tuple_instead() -> String {
            get_raw_string("DiagnosticAddendum.useTupleInstead")
        }

        // localization/localize.ts:1660
        pub fn use_type_instead() -> String {
            get_raw_string("DiagnosticAddendum.useTypeInstead")
        }

        // localization/localize.ts:1661
        /// Params: typeVarName, className.
        pub fn variance_mismatch_for_class() -> ParameterizedString {
            ParameterizedString::new(get_raw_string(
                "DiagnosticAddendum.varianceMismatchForClass",
            ))
        }

        // localization/localize.ts:1665
        /// Params: typeVarName, typeAliasParam.
        pub fn variance_mismatch_for_type_alias() -> ParameterizedString {
            ParameterizedString::new(get_raw_string(
                "DiagnosticAddendum.varianceMismatchForTypeAlias",
            ))
        }
    }

    #[allow(non_snake_case)]
    pub mod CallHierarchy {
        #[allow(unused_imports)]
        use super::super::{ParameterizedString, get_raw_string};

        // localization/localize.ts:1672
        pub fn library() -> String {
            get_raw_string("CallHierarchy.library")
        }

        // localization/localize.ts:1673
        pub fn standard_library() -> String {
            get_raw_string("CallHierarchy.standardLibrary")
        }

        // localization/localize.ts:1674
        pub fn workspace() -> String {
            get_raw_string("CallHierarchy.workspace")
        }
    }

    #[allow(non_snake_case)]
    pub mod CodeAction {
        #[allow(unused_imports)]
        use super::super::{ParameterizedString, get_raw_string};

        // localization/localize.ts:1678
        pub fn create_type_stub() -> String {
            get_raw_string("CodeAction.createTypeStub")
        }

        // localization/localize.ts:1679
        /// Params: moduleName.
        pub fn create_type_stub_for() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("CodeAction.createTypeStubFor"))
        }

        // localization/localize.ts:1681
        pub fn executing_command() -> String {
            get_raw_string("CodeAction.executingCommand")
        }

        // localization/localize.ts:1682
        pub fn files_to_analyze_one() -> String {
            get_raw_string("CodeAction.filesToAnalyzeOne")
        }

        // localization/localize.ts:1683
        /// Params: count.
        pub fn files_to_analyze_count() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("CodeAction.filesToAnalyzeCount"))
        }

        // localization/localize.ts:1685
        pub fn finding_references() -> String {
            get_raw_string("CodeAction.findingReferences")
        }

        // localization/localize.ts:1686
        pub fn organize_imports() -> String {
            get_raw_string("CodeAction.organizeImports")
        }
    }

    #[allow(non_snake_case)]
    pub mod Completion {
        #[allow(unused_imports)]
        use super::super::{ParameterizedString, get_raw_string};

        // localization/localize.ts:1690
        pub fn auto_import_detail() -> String {
            get_raw_string("Completion.autoImportDetail")
        }

        // localization/localize.ts:1691
        pub fn index_value_detail() -> String {
            get_raw_string("Completion.indexValueDetail")
        }
    }

    #[allow(non_snake_case)]
    pub mod Rename {
        #[allow(unused_imports)]
        use super::super::{ParameterizedString, get_raw_string};

        // localization/localize.ts:1695
        /// Params: symbolName, locations.
        pub fn cannot_rename_non_user_code() -> ParameterizedString {
            ParameterizedString::new(get_raw_string("Rename.cannotRenameNonUserCode"))
        }
    }

    #[allow(non_snake_case)]
    pub mod Service {
        #[allow(unused_imports)]
        use super::super::{ParameterizedString, get_raw_string};

        // localization/localize.ts:1702
        pub fn long_operation() -> String {
            get_raw_string("Service.longOperation")
        }
    }
}

// localization/localize.ts:1706
pub use Localizer::Diagnostic as LocMessage;
// localization/localize.ts:1707
pub use Localizer::DiagnosticAddendum as LocAddendum;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_matches_the_walk() {
        let strings: Value = serde_json::from_str(EN_US_STRINGS).unwrap();
        let mut index = FxHashMap::default();
        index_strings(&strings, &mut String::new(), &mut index);
        assert!(index.len() > 1000);
        for (key, message) in &index {
            assert_eq!(raw_string_from_key(&strings, key).as_ref(), Some(message), "{key}");
        }
        for key in ["Diagnostic", "Diagnostic.", "", "Diagnostic.noSuchMessage", "Localization.Diagnostic"] {
            assert_eq!(index.get(key).cloned(), raw_string_from_key(&strings, key), "{key:?}");
        }
    }

    #[test]
    fn simple_and_parameterized_messages() {
        assert_eq!(LocMessage::expected_expr(), "Expected expression");
        assert_eq!(
            LocMessage::import_resolve_failure().format(&[("importName", "foo"), ("venv", "x")]),
            "Import \"foo\" could not be resolved"
        );
    }

    #[test]
    fn format_applies_js_replacement_patterns() {
        let s = ParameterizedString::new("a {type} b".to_string());
        assert_eq!(s.format(&[("type", "$&")]), "a {type} b");
        assert_eq!(s.format(&[("type", "$$x")]), "a $x b");
    }

    #[test]
    fn locale_selection() {
        assert_eq!(load_strings_for_locale("en_US"), None);
        assert_eq!(load_strings_for_locale("de_DE"), None);
        assert_eq!(load_strings_for_locale("en"), None);
        assert!(std::panic::catch_unwind(|| load_strings_for_locale("de-DE")).is_err());
    }

    #[test]
    fn every_accessor_has_a_string() {
        let en: Value = serde_json::from_str(EN_US_STRINGS).unwrap();
        for key in KEYS {
            let parts: Vec<&str> = key.split('.').collect();
            assert!(get_raw_string_from_map(&en, &parts).is_some(), "{key}");
        }
    }

    const KEYS: &[&str] = &[
        "Diagnostic.annotatedMetadataInconsistent",
        "Diagnostic.abstractMethodInvocation",
        "Diagnostic.annotatedParamCountMismatch",
        "Diagnostic.annotatedTypeArgMissing",
        "Diagnostic.annotationBytesString",
        "Diagnostic.annotationFormatString",
        "Diagnostic.annotationNotSupported",
        "Diagnostic.annotationRawString",
        "Diagnostic.annotationSpansStrings",
        "Diagnostic.annotationStringEscape",
        "Diagnostic.annotationTemplateString",
        "Diagnostic.argAssignment",
        "Diagnostic.argAssignmentFunction",
        "Diagnostic.argAssignmentParam",
        "Diagnostic.argAssignmentParamFunction",
        "Diagnostic.argMissingForParam",
        "Diagnostic.argMissingForParams",
        "Diagnostic.argMorePositionalExpectedCount",
        "Diagnostic.argMorePositionalExpectedOne",
        "Diagnostic.argPositional",
        "Diagnostic.argPositionalExpectedCount",
        "Diagnostic.argPositionalExpectedOne",
        "Diagnostic.argTypePartiallyUnknown",
        "Diagnostic.argTypeUnknown",
        "Diagnostic.assertAlwaysTrue",
        "Diagnostic.assertTypeArgs",
        "Diagnostic.assertTypeTypeMismatch",
        "Diagnostic.assignmentExprContext",
        "Diagnostic.assignmentExprComprehension",
        "Diagnostic.assignmentExprInSubscript",
        "Diagnostic.assignmentInProtocol",
        "Diagnostic.assignmentTargetExpr",
        "Diagnostic.asyncNotInAsyncFunction",
        "Diagnostic.awaitIllegal",
        "Diagnostic.awaitNotAllowed",
        "Diagnostic.awaitNotInAsync",
        "Diagnostic.backticksIllegal",
        "Diagnostic.baseClassCircular",
        "Diagnostic.baseClassFinal",
        "Diagnostic.baseClassIncompatible",
        "Diagnostic.baseClassInvalid",
        "Diagnostic.baseClassMethodTypeIncompatible",
        "Diagnostic.baseClassVariableTypeIncompatible",
        "Diagnostic.baseClassUnknown",
        "Diagnostic.binaryOperationNotAllowed",
        "Diagnostic.bindParamMissing",
        "Diagnostic.bindTypeMismatch",
        "Diagnostic.breakInExceptionGroup",
        "Diagnostic.breakOutsideLoop",
        "Diagnostic.bytesUnsupportedEscape",
        "Diagnostic.callableExtraArgs",
        "Diagnostic.callableFirstArg",
        "Diagnostic.callableNotInstantiable",
        "Diagnostic.callableSecondArg",
        "Diagnostic.casePatternIsIrrefutable",
        "Diagnostic.classAlreadySpecialized",
        "Diagnostic.classDecoratorTypeUnknown",
        "Diagnostic.classDefinitionCycle",
        "Diagnostic.classGetItemClsParam",
        "Diagnostic.classMethodClsParam",
        "Diagnostic.classNotRuntimeSubscriptable",
        "Diagnostic.classPatternBuiltInArgPositional",
        "Diagnostic.classPatternNewType",
        "Diagnostic.classPatternPositionalArgCount",
        "Diagnostic.classPatternTypeAlias",
        "Diagnostic.classPropertyDeprecated",
        "Diagnostic.classTypeParametersIllegal",
        "Diagnostic.classVarNotAllowed",
        "Diagnostic.classVarFirstArgMissing",
        "Diagnostic.classVarOverridesInstanceVar",
        "Diagnostic.classVarTooManyArgs",
        "Diagnostic.classVarWithTypeVar",
        "Diagnostic.clsSelfParamTypeMismatch",
        "Diagnostic.codeTooComplexToAnalyze",
        "Diagnostic.collectionAliasInstantiation",
        "Diagnostic.comparisonAlwaysFalse",
        "Diagnostic.comparisonAlwaysTrue",
        "Diagnostic.comprehensionInDict",
        "Diagnostic.comprehensionInSet",
        "Diagnostic.concatenateContext",
        "Diagnostic.concatenateParamSpecMissing",
        "Diagnostic.concatenateTypeArgsMissing",
        "Diagnostic.conditionalOperandInvalid",
        "Diagnostic.constantRedefinition",
        "Diagnostic.coroutineInConditionalExpression",
        "Diagnostic.constructorParametersMismatch",
        "Diagnostic.containmentAlwaysFalse",
        "Diagnostic.containmentAlwaysTrue",
        "Diagnostic.continueInExceptionGroup",
        "Diagnostic.continueOutsideLoop",
        "Diagnostic.dataClassBaseClassFrozen",
        "Diagnostic.dataClassBaseClassNotFrozen",
        "Diagnostic.dataClassConverterFunction",
        "Diagnostic.dataClassConverterOverloads",
        "Diagnostic.dataClassDuplicateKwOnly",
        "Diagnostic.dataClassFieldInheritedDefault",
        "Diagnostic.dataClassFieldWithDefault",
        "Diagnostic.dataClassFieldWithoutAnnotation",
        "Diagnostic.dataClassFieldWithPrivateName",
        "Diagnostic.dataClassPostInitParamCount",
        "Diagnostic.dataClassPostInitType",
        "Diagnostic.dataClassSlotsOverwrite",
        "Diagnostic.dataClassTransformExpectedBoolLiteral",
        "Diagnostic.dataClassTransformFieldSpecifier",
        "Diagnostic.dataClassTransformPositionalParam",
        "Diagnostic.dataClassTransformUnknownArgument",
        "Diagnostic.dataProtocolInSubclassCheck",
        "Diagnostic.declaredReturnTypePartiallyUnknown",
        "Diagnostic.declaredReturnTypeUnknown",
        "Diagnostic.defaultValueContainsCall",
        "Diagnostic.defaultValueNotAllowed",
        "Diagnostic.deprecatedClass",
        "Diagnostic.deprecatedConstructor",
        "Diagnostic.deprecatedDescriptorDeleter",
        "Diagnostic.deprecatedDescriptorGetter",
        "Diagnostic.deprecatedDescriptorSetter",
        "Diagnostic.deprecatedFunction",
        "Diagnostic.deprecatedMethod",
        "Diagnostic.deprecatedPropertyDeleter",
        "Diagnostic.deprecatedPropertyGetter",
        "Diagnostic.deprecatedPropertySetter",
        "Diagnostic.deprecatedType",
        "Diagnostic.dictExpandIllegalInComprehension",
        "Diagnostic.dictInAnnotation",
        "Diagnostic.dictKeyValuePairs",
        "Diagnostic.dictUnpackIsNotMapping",
        "Diagnostic.disjointBaseFunction",
        "Diagnostic.disjointBaseIncompatible",
        "Diagnostic.disjointBaseProtocol",
        "Diagnostic.disjointBaseTypedDict",
        "Diagnostic.delTargetExpr",
        "Diagnostic.dunderAllSymbolNotPresent",
        "Diagnostic.duplicateArgsParam",
        "Diagnostic.duplicateBaseClass",
        "Diagnostic.duplicateCatchAll",
        "Diagnostic.duplicateEnumMember",
        "Diagnostic.duplicateGenericAndProtocolBase",
        "Diagnostic.duplicateImport",
        "Diagnostic.duplicateKwargsParam",
        "Diagnostic.duplicateKeywordOnly",
        "Diagnostic.duplicateParam",
        "Diagnostic.duplicateCapturePatternTarget",
        "Diagnostic.duplicateStarPattern",
        "Diagnostic.duplicateStarStarPattern",
        "Diagnostic.duplicatePositionOnly",
        "Diagnostic.duplicateUnpack",
        "Diagnostic.ellipsisAfterUnpacked",
        "Diagnostic.ellipsisContext",
        "Diagnostic.ellipsisSecondArg",
        "Diagnostic.enumClassOverride",
        "Diagnostic.enumMemberDelete",
        "Diagnostic.enumMemberSet",
        "Diagnostic.enumMemberTypeAnnotation",
        "Diagnostic.exceptionGroupIncompatible",
        "Diagnostic.exceptGroupMismatch",
        "Diagnostic.exceptGroupRequiresType",
        "Diagnostic.exceptionGroupTypeIncorrect",
        "Diagnostic.exceptionTypeIncorrect",
        "Diagnostic.exceptionTypeNotClass",
        "Diagnostic.exceptionTypeNotInstantiable",
        "Diagnostic.exceptRequiresParens",
        "Diagnostic.exceptWithAsRequiresParens",
        "Diagnostic.expectedAfterDecorator",
        "Diagnostic.expectedArrow",
        "Diagnostic.expectedAsAfterException",
        "Diagnostic.expectedAssignRightHandExpr",
        "Diagnostic.expectedBinaryRightHandExpr",
        "Diagnostic.expectedBoolLiteral",
        "Diagnostic.expectedCase",
        "Diagnostic.expectedClassName",
        "Diagnostic.expectedCloseBrace",
        "Diagnostic.expectedCloseBracket",
        "Diagnostic.expectedCloseParen",
        "Diagnostic.expectedColon",
        "Diagnostic.expectedComplexNumberLiteral",
        "Diagnostic.expectedDecoratorExpr",
        "Diagnostic.expectedDecoratorName",
        "Diagnostic.expectedDecoratorNewline",
        "Diagnostic.expectedDelExpr",
        "Diagnostic.expectedElse",
        "Diagnostic.expectedEquals",
        "Diagnostic.expectedExceptionClass",
        "Diagnostic.expectedExceptionObj",
        "Diagnostic.expectedExpr",
        "Diagnostic.expectedIdentifier",
        "Diagnostic.expectedImport",
        "Diagnostic.expectedImportAlias",
        "Diagnostic.expectedImportSymbols",
        "Diagnostic.expectedIndentedBlock",
        "Diagnostic.expectedIn",
        "Diagnostic.expectedInExpr",
        "Diagnostic.expectedFunctionAfterAsync",
        "Diagnostic.expectedFunctionName",
        "Diagnostic.expectedMemberName",
        "Diagnostic.expectedModuleName",
        "Diagnostic.expectedNameAfterAs",
        "Diagnostic.expectedNamedParameter",
        "Diagnostic.expectedNewline",
        "Diagnostic.expectedNewlineOrSemicolon",
        "Diagnostic.expectedOpenParen",
        "Diagnostic.expectedParamName",
        "Diagnostic.expectedPatternExpr",
        "Diagnostic.expectedPatternSubjectExpr",
        "Diagnostic.expectedPatternValue",
        "Diagnostic.expectedReturnExpr",
        "Diagnostic.expectedSliceIndex",
        "Diagnostic.expectedTypeNotString",
        "Diagnostic.expectedTypeParameterName",
        "Diagnostic.expectedYieldExpr",
        "Diagnostic.finalClassIsAbstract",
        "Diagnostic.finalContext",
        "Diagnostic.finalInLoop",
        "Diagnostic.finallyBreak",
        "Diagnostic.finallyContinue",
        "Diagnostic.finallyReturn",
        "Diagnostic.finalMethodOverride",
        "Diagnostic.finalNonMethod",
        "Diagnostic.finalReassigned",
        "Diagnostic.finalRedeclaration",
        "Diagnostic.finalRedeclarationBySubclass",
        "Diagnostic.finalTooManyArgs",
        "Diagnostic.finalUnassigned",
        "Diagnostic.formatStringBrace",
        "Diagnostic.formatStringBytes",
        "Diagnostic.formatStringDebuggingIllegal",
        "Diagnostic.formatStringEscape",
        "Diagnostic.formatStringExpectedConversion",
        "Diagnostic.formatStringInPattern",
        "Diagnostic.formatStringIllegal",
        "Diagnostic.formatStringNestedFormatSpecifier",
        "Diagnostic.formatStringNestedQuote",
        "Diagnostic.formatStringTemplate",
        "Diagnostic.formatStringUnicode",
        "Diagnostic.formatStringUnterminated",
        "Diagnostic.functionDecoratorTypeUnknown",
        "Diagnostic.functionInConditionalExpression",
        "Diagnostic.functionTypeParametersIllegal",
        "Diagnostic.futureImportLocationNotAllowed",
        "Diagnostic.generatorAsyncReturnType",
        "Diagnostic.generatorNotParenthesized",
        "Diagnostic.generatorSyncReturnType",
        "Diagnostic.genericBaseClassNotAllowed",
        "Diagnostic.genericClassAssigned",
        "Diagnostic.genericClassDeleted",
        "Diagnostic.genericInstanceVariableAccess",
        "Diagnostic.genericNotAllowed",
        "Diagnostic.genericTypeAliasBoundTypeVar",
        "Diagnostic.genericTypeArgMissing",
        "Diagnostic.genericTypeArgTypeVar",
        "Diagnostic.genericTypeArgUnique",
        "Diagnostic.globalReassignment",
        "Diagnostic.globalRedefinition",
        "Diagnostic.implicitStringConcat",
        "Diagnostic.importCycleDetected",
        "Diagnostic.importDepthExceeded",
        "Diagnostic.importResolveFailure",
        "Diagnostic.importSourceResolveFailure",
        "Diagnostic.importSymbolUnknown",
        "Diagnostic.incompatibleMethodOverride",
        "Diagnostic.inconsistentIndent",
        "Diagnostic.inconsistentTabs",
        "Diagnostic.initMethodSelfParamTypeVar",
        "Diagnostic.initMustReturnNone",
        "Diagnostic.initSubclassClsParam",
        "Diagnostic.initSubclassCallFailed",
        "Diagnostic.initVarNotAllowed",
        "Diagnostic.instanceMethodSelfParam",
        "Diagnostic.instanceVarOverridesClassVar",
        "Diagnostic.instantiateAbstract",
        "Diagnostic.instantiateProtocol",
        "Diagnostic.internalBindError",
        "Diagnostic.internalParseError",
        "Diagnostic.internalTypeCheckingError",
        "Diagnostic.invalidIdentifierChar",
        "Diagnostic.invalidStubStatement",
        "Diagnostic.invalidTokenChars",
        "Diagnostic.isInstanceInvalidType",
        "Diagnostic.isSubclassInvalidType",
        "Diagnostic.keyValueInSet",
        "Diagnostic.keywordArgInTypeArgument",
        "Diagnostic.keywordOnlyAfterArgs",
        "Diagnostic.keywordParameterMissing",
        "Diagnostic.keywordSubscriptIllegal",
        "Diagnostic.lambdaReturnTypeUnknown",
        "Diagnostic.lambdaReturnTypePartiallyUnknown",
        "Diagnostic.lazyImportIllegal",
        "Diagnostic.lazyImportWildcardIllegal",
        "Diagnostic.listAssignmentMismatch",
        "Diagnostic.listInAnnotation",
        "Diagnostic.literalNamedUnicodeEscape",
        "Diagnostic.literalUnsupportedType",
        "Diagnostic.literalEmptyArgs",
        "Diagnostic.literalNotAllowed",
        "Diagnostic.literalNotCallable",
        "Diagnostic.matchIncompatible",
        "Diagnostic.matchIsNotExhaustive",
        "Diagnostic.maxParseDepthExceeded",
        "Diagnostic.memberAccess",
        "Diagnostic.memberDelete",
        "Diagnostic.memberSet",
        "Diagnostic.metaclassConflict",
        "Diagnostic.metaclassDuplicate",
        "Diagnostic.metaclassIsGeneric",
        "Diagnostic.methodNotDefined",
        "Diagnostic.methodNotDefinedOnType",
        "Diagnostic.methodOrdering",
        "Diagnostic.methodOverridden",
        "Diagnostic.methodReturnsNonObject",
        "Diagnostic.missingSuperCall",
        "Diagnostic.mixingBytesAndStr",
        "Diagnostic.moduleAsType",
        "Diagnostic.moduleNotCallable",
        "Diagnostic.moduleUnknownMember",
        "Diagnostic.namedExceptAfterCatchAll",
        "Diagnostic.namedParamAfterParamSpecArgs",
        "Diagnostic.namedTupleEmptyName",
        "Diagnostic.namedTupleEntryRedeclared",
        "Diagnostic.namedTupleFieldUnderscore",
        "Diagnostic.namedTupleFirstArg",
        "Diagnostic.namedTupleMultipleInheritance",
        "Diagnostic.namedTupleNameKeyword",
        "Diagnostic.namedTupleNameType",
        "Diagnostic.namedTupleNameUnique",
        "Diagnostic.namedTupleNoTypes",
        "Diagnostic.namedTupleSecondArg",
        "Diagnostic.newClsParam",
        "Diagnostic.newTypeAnyOrUnknown",
        "Diagnostic.newTypeBadName",
        "Diagnostic.newTypeLiteral",
        "Diagnostic.newTypeNameMismatch",
        "Diagnostic.newTypeNotAClass",
        "Diagnostic.newTypeParamCount",
        "Diagnostic.newTypeProtocolClass",
        "Diagnostic.nonDefaultAfterDefault",
        "Diagnostic.noneNotCallable",
        "Diagnostic.noneNotIterable",
        "Diagnostic.noneNotSubscriptable",
        "Diagnostic.noneNotUsableWith",
        "Diagnostic.noneNotUsableWithAsync",
        "Diagnostic.noneOperator",
        "Diagnostic.noneUnknownMember",
        "Diagnostic.nonLocalInModule",
        "Diagnostic.nonLocalNoBinding",
        "Diagnostic.nonLocalReassignment",
        "Diagnostic.nonLocalRedefinition",
        "Diagnostic.noOverload",
        "Diagnostic.nonlocalTypeParam",
        "Diagnostic.noReturnContainsReturn",
        "Diagnostic.noReturnContainsYield",
        "Diagnostic.noReturnReturnsNone",
        "Diagnostic.notRequiredArgCount",
        "Diagnostic.notRequiredNotInTypedDict",
        "Diagnostic.objectNotCallable",
        "Diagnostic.obscuredClassDeclaration",
        "Diagnostic.obscuredFunctionDeclaration",
        "Diagnostic.obscuredMethodDeclaration",
        "Diagnostic.obscuredParameterDeclaration",
        "Diagnostic.obscuredTypeAliasDeclaration",
        "Diagnostic.obscuredVariableDeclaration",
        "Diagnostic.operatorLessOrGreaterDeprecated",
        "Diagnostic.optionalExtraArgs",
        "Diagnostic.orPatternIrrefutable",
        "Diagnostic.orPatternMissingName",
        "Diagnostic.overlappingKeywordArgs",
        "Diagnostic.overlappingOverload",
        "Diagnostic.overloadAbstractMismatch",
        "Diagnostic.overloadAbstractImplMismatch",
        "Diagnostic.overloadClassMethodInconsistent",
        "Diagnostic.overloadFinalImpl",
        "Diagnostic.overloadFinalNoImpl",
        "Diagnostic.overloadImplementationMismatch",
        "Diagnostic.overloadOverrideImpl",
        "Diagnostic.overloadOverrideNoImpl",
        "Diagnostic.overloadReturnTypeMismatch",
        "Diagnostic.overloadStaticMethodInconsistent",
        "Diagnostic.overloadWithoutImplementation",
        "Diagnostic.overriddenMethodNotFound",
        "Diagnostic.overrideDecoratorMissing",
        "Diagnostic.paramAfterKwargsParam",
        "Diagnostic.paramAlreadyAssigned",
        "Diagnostic.paramAnnotationMissing",
        "Diagnostic.paramNameMissing",
        "Diagnostic.paramSpecArgsKwargsDuplicate",
        "Diagnostic.paramSpecArgsKwargsUsage",
        "Diagnostic.paramSpecArgsMissing",
        "Diagnostic.paramSpecArgsUsage",
        "Diagnostic.paramSpecAssignedName",
        "Diagnostic.paramSpecContext",
        "Diagnostic.paramSpecDefaultNotTuple",
        "Diagnostic.paramSpecFirstArg",
        "Diagnostic.paramSpecKwargsUsage",
        "Diagnostic.paramSpecNotUsedByOuterScope",
        "Diagnostic.paramSpecUnknownArg",
        "Diagnostic.paramSpecUnknownMember",
        "Diagnostic.paramSpecUnknownParam",
        "Diagnostic.paramTypeCovariant",
        "Diagnostic.paramTypeUnknown",
        "Diagnostic.paramAssignmentMismatch",
        "Diagnostic.paramTypePartiallyUnknown",
        "Diagnostic.parenthesizedContextManagerIllegal",
        "Diagnostic.patternNeverMatches",
        "Diagnostic.positionArgAfterNamedArg",
        "Diagnostic.positionArgAfterUnpackedDictArg",
        "Diagnostic.privateImportFromPyTypedModule",
        "Diagnostic.positionOnlyAfterArgs",
        "Diagnostic.positionOnlyAfterKeywordOnly",
        "Diagnostic.positionOnlyAfterNon",
        "Diagnostic.positionOnlyIncompatible",
        "Diagnostic.positionOnlyFirstParam",
        "Diagnostic.privateUsedOutsideOfClass",
        "Diagnostic.privateUsedOutsideOfModule",
        "Diagnostic.propertyOverridden",
        "Diagnostic.propertyStaticMethod",
        "Diagnostic.protectedUsedOutsideOfClass",
        "Diagnostic.protocolBaseClass",
        "Diagnostic.protocolBaseClassWithTypeArgs",
        "Diagnostic.protocolIllegal",
        "Diagnostic.protocolNotAllowed",
        "Diagnostic.protocolTypeArgMustBeTypeParam",
        "Diagnostic.protocolUnsafeOverlap",
        "Diagnostic.protocolVarianceContravariant",
        "Diagnostic.protocolVarianceCovariant",
        "Diagnostic.protocolVarianceInvariant",
        "Diagnostic.pyrightCommentInvalidDiagnosticBoolValue",
        "Diagnostic.pyrightCommentInvalidDiagnosticSeverityValue",
        "Diagnostic.pyrightCommentMissingDirective",
        "Diagnostic.pyrightCommentNotOnOwnLine",
        "Diagnostic.pyrightCommentUnknownDirective",
        "Diagnostic.pyrightCommentUnknownDiagnosticRule",
        "Diagnostic.readOnlyArgCount",
        "Diagnostic.readOnlyNotInTypedDict",
        "Diagnostic.recursiveDefinition",
        "Diagnostic.relativeImportNotAllowed",
        "Diagnostic.requiredArgCount",
        "Diagnostic.requiredNotInTypedDict",
        "Diagnostic.returnInAsyncGenerator",
        "Diagnostic.returnMissing",
        "Diagnostic.returnInExceptionGroup",
        "Diagnostic.returnOutsideFunction",
        "Diagnostic.returnTypeContravariant",
        "Diagnostic.returnTypeMismatch",
        "Diagnostic.returnTypeUnknown",
        "Diagnostic.returnTypePartiallyUnknown",
        "Diagnostic.revealLocalsArgs",
        "Diagnostic.revealLocalsNone",
        "Diagnostic.revealTypeArgs",
        "Diagnostic.revealTypeExpectedTextArg",
        "Diagnostic.revealTypeExpectedTextMismatch",
        "Diagnostic.revealTypeExpectedTypeMismatch",
        "Diagnostic.selfTypeContext",
        "Diagnostic.selfTypeMetaclass",
        "Diagnostic.selfTypeWithTypedSelfOrCls",
        "Diagnostic.sentinelBadName",
        "Diagnostic.sentinelNameMismatch",
        "Diagnostic.sentinelParamCount",
        "Diagnostic.setterGetterTypeMismatch",
        "Diagnostic.starPatternInAsPattern",
        "Diagnostic.starPatternInOrPattern",
        "Diagnostic.singleOverload",
        "Diagnostic.slotsAttributeError",
        "Diagnostic.slotsClassVarConflict",
        "Diagnostic.starStarWildcardNotAllowed",
        "Diagnostic.staticClsSelfParam",
        "Diagnostic.stringNonAsciiBytes",
        "Diagnostic.stringNotSubscriptable",
        "Diagnostic.stringUnsupportedEscape",
        "Diagnostic.stringUnterminated",
        "Diagnostic.stubFileMissing",
        "Diagnostic.stubUsesGetAttr",
        "Diagnostic.sublistParamsIncompatible",
        "Diagnostic.superCallArgCount",
        "Diagnostic.superCallFirstArg",
        "Diagnostic.superCallSecondArg",
        "Diagnostic.superCallZeroArgForm",
        "Diagnostic.superCallZeroArgFormStaticMethod",
        "Diagnostic.symbolIsUnbound",
        "Diagnostic.symbolIsUndefined",
        "Diagnostic.symbolIsPossiblyUnbound",
        "Diagnostic.symbolOverridden",
        "Diagnostic.templateStringBytes",
        "Diagnostic.templateStringIllegal",
        "Diagnostic.templateStringUnicode",
        "Diagnostic.ternaryNotAllowed",
        "Diagnostic.totalOrderingMissingMethod",
        "Diagnostic.trailingCommaInFromImport",
        "Diagnostic.tryWithoutExcept",
        "Diagnostic.tupleAssignmentMismatch",
        "Diagnostic.tupleInAnnotation",
        "Diagnostic.tupleIndexOutOfRange",
        "Diagnostic.typeAliasIllegalExpressionForm",
        "Diagnostic.typeAliasIsRecursiveDirect",
        "Diagnostic.typeAliasNotInModuleOrClass",
        "Diagnostic.typeAliasRedeclared",
        "Diagnostic.typeAliasStatementIllegal",
        "Diagnostic.typeAliasStatementBadScope",
        "Diagnostic.typeAliasTypeBadScope",
        "Diagnostic.typeAliasTypeBaseClass",
        "Diagnostic.typeAliasTypeMustBeAssigned",
        "Diagnostic.typeAliasTypeNameArg",
        "Diagnostic.typeAliasTypeNameMismatch",
        "Diagnostic.typeAliasTypeParamInvalid",
        "Diagnostic.typeAnnotationCall",
        "Diagnostic.typeAnnotationVariable",
        "Diagnostic.typeAnnotationWithCallable",
        "Diagnostic.typeArgListExpected",
        "Diagnostic.typeArgListNotAllowed",
        "Diagnostic.typeArgsExpectingNone",
        "Diagnostic.typeArgsMismatchOne",
        "Diagnostic.typeArgsMissingForAlias",
        "Diagnostic.typeArgsMissingForClass",
        "Diagnostic.typeArgsTooFew",
        "Diagnostic.typeArgsTooMany",
        "Diagnostic.typeAssignmentMismatch",
        "Diagnostic.typeAssignmentMismatchWildcard",
        "Diagnostic.typeCallNotAllowed",
        "Diagnostic.typeCheckOnly",
        "Diagnostic.typeCommentDeprecated",
        "Diagnostic.typedDictAccess",
        "Diagnostic.typedDictAssignedName",
        "Diagnostic.typedDictBadVar",
        "Diagnostic.typedDictBaseClass",
        "Diagnostic.typedDictBoolParam",
        "Diagnostic.typedDictClosedExtras",
        "Diagnostic.typedDictClosedNoExtras",
        "Diagnostic.typedDictClosedFalseNonOpenBase",
        "Diagnostic.typedDictDelete",
        "Diagnostic.typedDictEmptyName",
        "Diagnostic.typedDictEntryName",
        "Diagnostic.typedDictEntryUnique",
        "Diagnostic.typedDictExtraArgs",
        "Diagnostic.typedDictExtraItemsClosed",
        "Diagnostic.typedDictFieldNotRequiredRedefinition",
        "Diagnostic.typedDictFieldReadOnlyRedefinition",
        "Diagnostic.typedDictFieldRequiredRedefinition",
        "Diagnostic.typedDictFirstArg",
        "Diagnostic.typedDictInClassPattern",
        "Diagnostic.typedDictInitsubclassParameter",
        "Diagnostic.typedDictNotAllowed",
        "Diagnostic.typedDictSecondArgDict",
        "Diagnostic.typedDictSecondArgDictEntry",
        "Diagnostic.typedDictSet",
        "Diagnostic.typeExpectedClass",
        "Diagnostic.typeFormArgs",
        "Diagnostic.typeGuardArgCount",
        "Diagnostic.typeGuardParamCount",
        "Diagnostic.typeIsReturnType",
        "Diagnostic.typeNotAwaitable",
        "Diagnostic.typeNotIntantiable",
        "Diagnostic.typeNotIterable",
        "Diagnostic.typeNotSpecializable",
        "Diagnostic.typeNotSubscriptable",
        "Diagnostic.typeNotUsableWith",
        "Diagnostic.typeNotUsableWithAsync",
        "Diagnostic.typeNotSupportBinaryOperator",
        "Diagnostic.typeNotSupportBinaryOperatorBidirectional",
        "Diagnostic.typeNotSupportUnaryOperator",
        "Diagnostic.typeNotSupportUnaryOperatorBidirectional",
        "Diagnostic.typeParameterBoundNotAllowed",
        "Diagnostic.typeParameterConstraintTuple",
        "Diagnostic.typeParameterExistingTypeParameter",
        "Diagnostic.typeParametersMissing",
        "Diagnostic.typeParameterNotDeclared",
        "Diagnostic.typePartiallyUnknown",
        "Diagnostic.typeUnknown",
        "Diagnostic.typeVarAssignedName",
        "Diagnostic.typeVarAssignmentMismatch",
        "Diagnostic.typeVarBoundAndConstrained",
        "Diagnostic.typeVarBoundGeneric",
        "Diagnostic.typeVarConstraintGeneric",
        "Diagnostic.typeVarDefaultBoundMismatch",
        "Diagnostic.typeVarDefaultConstraintMismatch",
        "Diagnostic.typeVarDefaultIllegal",
        "Diagnostic.typeVarDefaultInvalidTypeVar",
        "Diagnostic.typeVarFirstArg",
        "Diagnostic.typeVarInvalidForMemberVariable",
        "Diagnostic.typeVarNoMember",
        "Diagnostic.typeVarNotSubscriptable",
        "Diagnostic.typeVarNotUsedByOuterScope",
        "Diagnostic.typeVarPossiblyUnsolvable",
        "Diagnostic.typeVarSingleConstraint",
        "Diagnostic.typeVarsNotInGenericOrProtocol",
        "Diagnostic.typeVarTupleContext",
        "Diagnostic.typeVarTupleDefaultNotUnpacked",
        "Diagnostic.typeVarTupleMustBeUnpacked",
        "Diagnostic.typeVarTupleConstraints",
        "Diagnostic.typeVarTupleUnknownParam",
        "Diagnostic.typeVarUnknownParam",
        "Diagnostic.typeVarUsedByOuterScope",
        "Diagnostic.typeVarUsedOnlyOnce",
        "Diagnostic.typeVarVariance",
        "Diagnostic.typeVarWithDefaultFollowsVariadic",
        "Diagnostic.typeVarWithoutDefault",
        "Diagnostic.unaccessedClass",
        "Diagnostic.unaccessedFunction",
        "Diagnostic.unaccessedImport",
        "Diagnostic.unaccessedSymbol",
        "Diagnostic.unaccessedVariable",
        "Diagnostic.unannotatedFunctionSkipped",
        "Diagnostic.unaryOperationNotAllowed",
        "Diagnostic.unexpectedAsyncToken",
        "Diagnostic.unexpectedEof",
        "Diagnostic.unexpectedExprToken",
        "Diagnostic.unexpectedIndent",
        "Diagnostic.unexpectedUnindent",
        "Diagnostic.unhashableDictKey",
        "Diagnostic.unhashableSetEntry",
        "Diagnostic.unionForwardReferenceNotAllowed",
        "Diagnostic.unionSyntaxIllegal",
        "Diagnostic.unionTypeArgCount",
        "Diagnostic.uninitializedAbstractVariables",
        "Diagnostic.uninitializedInstanceVariable",
        "Diagnostic.unionUnpackedTuple",
        "Diagnostic.unionUnpackedTypeVarTuple",
        "Diagnostic.unnecessaryCast",
        "Diagnostic.unnecessaryIsInstanceAlways",
        "Diagnostic.unnecessaryIsSubclassAlways",
        "Diagnostic.unnecessaryIsInstanceNever",
        "Diagnostic.unnecessaryIsSubclassNever",
        "Diagnostic.unnecessaryPyrightIgnore",
        "Diagnostic.unnecessaryPyrightIgnoreRule",
        "Diagnostic.unnecessaryTypeIgnore",
        "Diagnostic.unpackArgCount",
        "Diagnostic.unpackedArgInTypeArgument",
        "Diagnostic.unpackedArgWithVariadicParam",
        "Diagnostic.unpackedDictArgumentNotMapping",
        "Diagnostic.unpackedDictSubscriptIllegal",
        "Diagnostic.unpackedSubscriptIllegal",
        "Diagnostic.unpackedTypedDictArgument",
        "Diagnostic.unpackedTypeVarTupleExpected",
        "Diagnostic.unpackExpectedTypedDict",
        "Diagnostic.unpackExpectedTypeVarTuple",
        "Diagnostic.unpackIllegalInComprehension",
        "Diagnostic.unpackInAnnotation",
        "Diagnostic.unpackInDict",
        "Diagnostic.unpackInSet",
        "Diagnostic.unpackNotAllowed",
        "Diagnostic.unpackOperatorNotAllowed",
        "Diagnostic.unpackTuplesIllegal",
        "Diagnostic.unreachableCodeCondition",
        "Diagnostic.unreachableCodeStructure",
        "Diagnostic.unreachableCodeType",
        "Diagnostic.unreachableExcept",
        "Diagnostic.unsupportedDunderAllOperation",
        "Diagnostic.unusedCallResult",
        "Diagnostic.unusedCoroutine",
        "Diagnostic.unusedExpression",
        "Diagnostic.varAnnotationIllegal",
        "Diagnostic.variableFinalOverride",
        "Diagnostic.variadicTypeArgsTooMany",
        "Diagnostic.variadicTypeParamTooManyAlias",
        "Diagnostic.variadicTypeParamTooManyClass",
        "Diagnostic.walrusIllegal",
        "Diagnostic.walrusNotAllowed",
        "Diagnostic.walrusNotAllowedInComprehension",
        "Diagnostic.wildcardInFunction",
        "Diagnostic.wildcardPatternTypeUnknown",
        "Diagnostic.wildcardPatternTypePartiallyUnknown",
        "Diagnostic.wildcardLibraryImport",
        "Diagnostic.yieldFromIllegal",
        "Diagnostic.yieldFromOutsideAsync",
        "Diagnostic.yieldOutsideFunction",
        "Diagnostic.yieldWithinComprehension",
        "Diagnostic.zeroCaseStatementsFound",
        "Diagnostic.zeroLengthTupleNotAllowed",
        "DiagnosticAddendum.annotatedNotAllowed",
        "DiagnosticAddendum.argParam",
        "DiagnosticAddendum.argParamFunction",
        "DiagnosticAddendum.argsParamMissing",
        "DiagnosticAddendum.argsPositionOnly",
        "DiagnosticAddendum.argumentType",
        "DiagnosticAddendum.argumentTypes",
        "DiagnosticAddendum.assignToNone",
        "DiagnosticAddendum.asyncHelp",
        "DiagnosticAddendum.baseClassIncompatible",
        "DiagnosticAddendum.baseClassIncompatibleSubclass",
        "DiagnosticAddendum.baseClassOverriddenType",
        "DiagnosticAddendum.baseClassOverridesType",
        "DiagnosticAddendum.bytesTypePromotions",
        "DiagnosticAddendum.conditionalRequiresBool",
        "DiagnosticAddendum.dataClassFrozen",
        "DiagnosticAddendum.dataClassFieldLocation",
        "DiagnosticAddendum.dataProtocolUnsupported",
        "DiagnosticAddendum.descriptorAccessBindingFailed",
        "DiagnosticAddendum.descriptorAccessCallFailed",
        "DiagnosticAddendum.finalMethod",
        "DiagnosticAddendum.functionParamDefaultMissing",
        "DiagnosticAddendum.functionParamName",
        "DiagnosticAddendum.functionParamPositionOnly",
        "DiagnosticAddendum.functionReturnTypeMismatch",
        "DiagnosticAddendum.functionTooFewParams",
        "DiagnosticAddendum.genericClassNotAllowed",
        "DiagnosticAddendum.incompatibleGetter",
        "DiagnosticAddendum.incompatibleSetter",
        "DiagnosticAddendum.incompatibleDeleter",
        "DiagnosticAddendum.initMethodLocation",
        "DiagnosticAddendum.initMethodSignature",
        "DiagnosticAddendum.initSubclassLocation",
        "DiagnosticAddendum.invariantSuggestionDict",
        "DiagnosticAddendum.invariantSuggestionList",
        "DiagnosticAddendum.invariantSuggestionSet",
        "DiagnosticAddendum.isinstanceClassNotSupported",
        "DiagnosticAddendum.functionTooManyParams",
        "DiagnosticAddendum.keyNotRequired",
        "DiagnosticAddendum.keyReadOnly",
        "DiagnosticAddendum.keyRequiredDeleted",
        "DiagnosticAddendum.keyUndefined",
        "DiagnosticAddendum.kwargsParamMissing",
        "DiagnosticAddendum.listAssignmentMismatch",
        "DiagnosticAddendum.literalAssignmentMismatch",
        "DiagnosticAddendum.literalNotAllowed",
        "DiagnosticAddendum.matchIsNotExhaustiveType",
        "DiagnosticAddendum.matchIsNotExhaustiveHint",
        "DiagnosticAddendum.memberAssignment",
        "DiagnosticAddendum.memberIsAbstract",
        "DiagnosticAddendum.memberIsAbstractMore",
        "DiagnosticAddendum.memberIsClassVarInProtocol",
        "DiagnosticAddendum.memberIsInitVar",
        "DiagnosticAddendum.memberIsInvariant",
        "DiagnosticAddendum.memberIsNotClassVarInClass",
        "DiagnosticAddendum.memberIsNotClassVarInProtocol",
        "DiagnosticAddendum.memberIsNotReadOnlyInProtocol",
        "DiagnosticAddendum.memberIsReadOnlyInProtocol",
        "DiagnosticAddendum.memberIsWritableInProtocol",
        "DiagnosticAddendum.memberSetClassVar",
        "DiagnosticAddendum.memberTypeMismatch",
        "DiagnosticAddendum.memberUnknown",
        "DiagnosticAddendum.metaclassConflict",
        "DiagnosticAddendum.missingGetter",
        "DiagnosticAddendum.missingSetter",
        "DiagnosticAddendum.missingDeleter",
        "DiagnosticAddendum.namedParamMissingInDest",
        "DiagnosticAddendum.namedParamMissingInSource",
        "DiagnosticAddendum.namedParamTypeMismatch",
        "DiagnosticAddendum.namedTupleNotAllowed",
        "DiagnosticAddendum.newMethodLocation",
        "DiagnosticAddendum.newMethodSignature",
        "DiagnosticAddendum.noneNotAllowed",
        "DiagnosticAddendum.newTypeClassNotAllowed",
        "DiagnosticAddendum.noOverloadAssignable",
        "DiagnosticAddendum.orPatternMissingName",
        "DiagnosticAddendum.overloadIndex",
        "DiagnosticAddendum.overloadSignature",
        "DiagnosticAddendum.overloadNotAssignable",
        "DiagnosticAddendum.overriddenMethod",
        "DiagnosticAddendum.overriddenSymbol",
        "DiagnosticAddendum.overrideIsInvariant",
        "DiagnosticAddendum.overrideInvariantMismatch",
        "DiagnosticAddendum.overrideNoOverloadMatches",
        "DiagnosticAddendum.overrideNotClassMethod",
        "DiagnosticAddendum.overrideNotInstanceMethod",
        "DiagnosticAddendum.overrideNotStaticMethod",
        "DiagnosticAddendum.overrideOverloadNoMatch",
        "DiagnosticAddendum.overrideOverloadOrder",
        "DiagnosticAddendum.overrideParamKeywordNoDefault",
        "DiagnosticAddendum.overrideParamKeywordType",
        "DiagnosticAddendum.overrideParamName",
        "DiagnosticAddendum.overrideParamNameExtra",
        "DiagnosticAddendum.overrideParamNameMissing",
        "DiagnosticAddendum.overrideParamNamePositionOnly",
        "DiagnosticAddendum.overrideParamNoDefault",
        "DiagnosticAddendum.overrideParamType",
        "DiagnosticAddendum.overridePositionalParamCount",
        "DiagnosticAddendum.overrideReturnType",
        "DiagnosticAddendum.overrideType",
        "DiagnosticAddendum.paramAssignment",
        "DiagnosticAddendum.paramSpecMissingInOverride",
        "DiagnosticAddendum.paramType",
        "DiagnosticAddendum.privateImportFromPyTypedSource",
        "DiagnosticAddendum.propertyAccessFromProtocolClass",
        "DiagnosticAddendum.propertyMethodIncompatible",
        "DiagnosticAddendum.propertyMethodMissing",
        "DiagnosticAddendum.propertyMissingDeleter",
        "DiagnosticAddendum.propertyMissingSetter",
        "DiagnosticAddendum.protocolIncompatible",
        "DiagnosticAddendum.protocolMemberMissing",
        "DiagnosticAddendum.protocolRequiresRuntimeCheckable",
        "DiagnosticAddendum.protocolSourceIsNotConcrete",
        "DiagnosticAddendum.protocolUnsafeOverlap",
        "DiagnosticAddendum.pyrightCommentIgnoreTip",
        "DiagnosticAddendum.readOnlyAttribute",
        "DiagnosticAddendum.seeDeclaration",
        "DiagnosticAddendum.seeClassDeclaration",
        "DiagnosticAddendum.seeFunctionDeclaration",
        "DiagnosticAddendum.seeMethodDeclaration",
        "DiagnosticAddendum.seeParameterDeclaration",
        "DiagnosticAddendum.seeTypeAliasDeclaration",
        "DiagnosticAddendum.seeVariableDeclaration",
        "DiagnosticAddendum.tupleEntryTypeMismatch",
        "DiagnosticAddendum.tupleAssignmentMismatch",
        "DiagnosticAddendum.tupleSizeIndeterminateSrc",
        "DiagnosticAddendum.tupleSizeIndeterminateSrcDest",
        "DiagnosticAddendum.tupleSizeMismatch",
        "DiagnosticAddendum.tupleSizeMismatchIndeterminateDest",
        "DiagnosticAddendum.typeAliasInstanceCheck",
        "DiagnosticAddendum.typeAssignmentMismatch",
        "DiagnosticAddendum.typeBound",
        "DiagnosticAddendum.typeConstrainedTypeVar",
        "DiagnosticAddendum.typedDictBaseClass",
        "DiagnosticAddendum.typedDictClassNotAllowed",
        "DiagnosticAddendum.typedDictExtraFieldNotAllowed",
        "DiagnosticAddendum.typedDictExtraFieldTypeMismatch",
        "DiagnosticAddendum.typedDictFieldMissing",
        "DiagnosticAddendum.typedDictClosedExtraNotAllowed",
        "DiagnosticAddendum.typedDictClosedExtraTypeMismatch",
        "DiagnosticAddendum.typedDictClosedFieldNotReadOnly",
        "DiagnosticAddendum.typedDictClosedFieldNotRequired",
        "DiagnosticAddendum.typedDictFieldNotReadOnly",
        "DiagnosticAddendum.typedDictFieldNotRequired",
        "DiagnosticAddendum.typedDictFieldRequired",
        "DiagnosticAddendum.typedDictFieldTypeMismatch",
        "DiagnosticAddendum.typedDictFieldUndefined",
        "DiagnosticAddendum.typedDictKeyAccess",
        "DiagnosticAddendum.typedDictNotAllowed",
        "DiagnosticAddendum.typeIncompatible",
        "DiagnosticAddendum.typeNotClass",
        "DiagnosticAddendum.typeParamSpec",
        "DiagnosticAddendum.typeNotStringLiteral",
        "DiagnosticAddendum.typeOfSymbol",
        "DiagnosticAddendum.typeUnsupported",
        "DiagnosticAddendum.typeVarDefaultOutOfScope",
        "DiagnosticAddendum.typeVarIsContravariant",
        "DiagnosticAddendum.typeVarIsCovariant",
        "DiagnosticAddendum.typeVarIsInvariant",
        "DiagnosticAddendum.typeVarsMissing",
        "DiagnosticAddendum.typeVarNotAllowed",
        "DiagnosticAddendum.typeVarTupleRequiresKnownLength",
        "DiagnosticAddendum.typeVarUnnecessarySuggestion",
        "DiagnosticAddendum.typeVarUnsolvableRemedy",
        "DiagnosticAddendum.unhashableType",
        "DiagnosticAddendum.uninitializedAbstractVariable",
        "DiagnosticAddendum.unreachableExcept",
        "DiagnosticAddendum.useDictInstead",
        "DiagnosticAddendum.useListInstead",
        "DiagnosticAddendum.useTupleInstead",
        "DiagnosticAddendum.useTypeInstead",
        "DiagnosticAddendum.varianceMismatchForClass",
        "DiagnosticAddendum.varianceMismatchForTypeAlias",
        "CallHierarchy.library",
        "CallHierarchy.standardLibrary",
        "CallHierarchy.workspace",
        "CodeAction.createTypeStub",
        "CodeAction.createTypeStubFor",
        "CodeAction.executingCommand",
        "CodeAction.filesToAnalyzeOne",
        "CodeAction.filesToAnalyzeCount",
        "CodeAction.findingReferences",
        "CodeAction.organizeImports",
        "Completion.autoImportDetail",
        "Completion.indexValueDetail",
        "Rename.cannotRenameNonUserCode",
        "Service.longOperation",
    ];
}
