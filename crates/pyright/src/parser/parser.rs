//! Port of `packages/pyright-internal/src/parser/parser.ts`.
//!
//! Based on code from vscode-python repository:
//!  https://github.com/Microsoft/vscode-python
//!
//! Parser for the Python language. Converts a stream of tokens
//! into an abstract syntax tree (AST).
//!
//! `Parser` methods appear in the TypeScript order, split over several
//! `impl` blocks.

use jsrt::{FxHashMap, JsMap, JsSet, JsStr, JsString, P};

use super::parse_nodes::*;
use super::string_annotation_info::{
    StringAnnotationInfo, StringAnnotationInfoAndWriter, create_string_annotation_info,
};
use super::string_token_utils;
use super::tokenizer::{Tokenizer, TokenizerOutput};
use super::tokenizer_types::{
    DedentToken, FStringEndToken, FStringMiddleToken, FStringStartToken, IdentifierToken,
    IndentToken, KeywordToken, KeywordType, NumberToken, OperatorToken, OperatorType, StringToken,
    StringTokenFlags, Token, TokenDetails, TokenType,
};
use crate::common::timing::timing_stats;
use crate::common::char_codes::Char;
use crate::common::collection_utils::append_array;
use crate::common::diagnostic::{
    DEFAULT_MAX_DIAGNOSTIC_DEPTH, DEFAULT_MAX_DIAGNOSTIC_LINE_COUNT, Diagnostic, DiagnosticAddendum,
};
use crate::common::diagnostic_sink::DiagnosticSink;
use crate::common::position_utils::convert_offsets_to_range;
use crate::common::python_version::{
    LATEST_STABLE_PYTHON_VERSION, PYTHON_VERSION3_3, PYTHON_VERSION3_5, PYTHON_VERSION3_6,
    PYTHON_VERSION3_8, PYTHON_VERSION3_9, PYTHON_VERSION3_10, PYTHON_VERSION3_11,
    PYTHON_VERSION3_12, PYTHON_VERSION3_13, PYTHON_VERSION3_14, PYTHON_VERSION3_15, PythonVersion,
};
use crate::common::string_utils::{StringFingerprint, get_string_fingerprint};
use crate::common::text_range::{HasTextRange, TextRange};
use crate::common::text_range_collection::TextRangeCollection;
use crate::localization::localize::{LocAddendum, LocMessage};

// parser/parser.ts:157
struct ListResult {
    list: Vec<P<ParseNode>>,
    trailing_comma: bool,
    parse_error: Option<ErrorNode>,
}

// parser/parser.ts:163
struct SubscriptListResult {
    list: Vec<ArgumentNode>,
    trailing_comma: bool,
}

// parser/parser.ts:171
#[derive(Clone, Debug)]
pub struct ParseOptions {
    pub is_stub_file: bool,
    pub python_version: PythonVersion,
    pub report_invalid_string_escape_sequence: bool,
    pub skip_function_and_class_body: bool,
    pub use_notebook_mode: bool,
    pub report_errors_for_parsed_string_contents: bool,
}

impl Default for ParseOptions {
    fn default() -> Self {
        ParseOptions::new()
    }
}

impl ParseOptions {
    // parser/parser.ts:179
    pub fn new() -> ParseOptions {
        ParseOptions {
            is_stub_file: false,
            python_version: LATEST_STABLE_PYTHON_VERSION,
            report_invalid_string_escape_sequence: false,
            skip_function_and_class_body: false,
            use_notebook_mode: false,
            report_errors_for_parsed_string_contents: false,
        }
    }
}

// parser/parser.ts:189
#[derive(jsrt::Trace)]
pub struct ParserOutput {
    pub parse_tree: ModuleNode,
    pub string_annotations: StringAnnotationInfo,
    pub imported_modules: Vec<ModuleImport>,
    pub future_imports: JsSet<String>,
    pub contains_wildcard_import: bool,
    pub typing_symbol_aliases: JsMap<String, String>,
    pub has_type_annotations: bool,
    pub lines: &'static TextRangeCollection<TextRange>,
}

// parser/parser.ts:200
pub struct ParseFileResults {
    pub text: JsString,
    pub content_hash: StringFingerprint,
    pub parser_output: ParserOutput,
    pub tokenizer_output: P<TokenizerOutput>,
}

// parser/parser.ts:207
pub struct ParseExpressionTextResults {
    pub parse_tree: Option<P<ParseNode>>,
    pub string_annotations: StringAnnotationInfo,
    pub lines: &'static TextRangeCollection<TextRange>,
    pub diagnostics: Vec<P<Diagnostic>>,
}

// parser/parser.ts:214
#[derive(Clone, Debug)]
#[derive(jsrt::Trace)]
pub struct ModuleImport {
    pub name_node: ModuleNameNode,
    pub leading_dots: i32,
    pub name_parts: Vec<String>,

    // Used for "from X import Y" pattern. An empty
    // array implies "from X import *".
    pub imported_symbols: Option<JsSet<String>>,
}

// parser/parser.ts:224
pub struct ArgListResult {
    pub args: Vec<ArgumentNode>,
    pub trailing_comma: bool,
}

// parser/parser.ts:229
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParseTextMode {
    Expression,
    VariableAnnotation,
    FunctionAnnotation,
}

// Limit the max child node depth to prevent stack overflows.
// parser/parser.ts:236
const MAX_CHILD_NODE_DEPTH: i32 = 256;

/// `getErrorString` callbacks: a message accessor such as `LocMessage::expected_expr`.
type ErrorStringGetter = fn() -> String;

/// `(token as KeywordToken).keywordType` when `token.type === TokenType.Keyword`.
fn keyword_type_of(token: P<Token>) -> Option<KeywordType> {
    if token.type_ != TokenType::Keyword {
        return None;
    }
    KeywordToken::cast(token).map(|t| t.d().keyword_type)
}

/// `(token as OperatorToken).operatorType` when `token.type === TokenType.Operator`.
fn operator_type_of(token: P<Token>) -> Option<OperatorType> {
    if token.type_ != TokenType::Operator {
        return None;
    }
    OperatorToken::cast(token).map(|t| t.d().operator_type)
}

/// `LocMessage.x().format({ name })` with one string parameter.
fn format1(
    message: crate::localization::localize::ParameterizedString,
    key: &str,
    value: &str,
) -> String {
    message.format(&[(key, value)])
}

// parser/parser.ts:238
pub struct Parser {
    _file_contents: Option<JsString>,
    _tokenizer_output: Option<P<TokenizerOutput>>,
    _tokens: Option<&'static TextRangeCollection<P<Token>>>,
    _token_count: i32,
    _token_index: i32,
    _are_errors_suppressed: bool,
    _parse_options: ParseOptions,
    _diag_sink: P<DiagnosticSink>,
    _is_in_loop: bool,
    _is_in_function: bool,
    _is_in_exception_group: bool,
    _is_parsing_type_annotation: bool,
    _is_parsing_index_trailer: bool,
    _is_parsing_quoted_text: bool,
    _is_in_finally_block: bool,
    _is_in_finally_loop: bool,
    _future_imports: JsSet<String>,
    _imported_modules: Vec<ModuleImport>,
    _contains_wildcard_import: bool,
    _assignment_expressions_allowed: bool,
    _typing_import_aliases: Vec<String>,
    _typing_symbol_aliases: JsMap<String, String>,
    _max_child_depth_map: FxHashMap<u32, i32>,
    _has_type_annotations: bool,
    _owner_key: Option<ParseTreeKey>,
    _created_owner_key: bool,
    _string_annotations: StringAnnotationInfoAndWriter,
}

impl Default for Parser {
    fn default() -> Self {
        Parser::new()
    }
}

impl Parser {
    pub fn new() -> Parser {
        Parser {
            _file_contents: None,
            _tokenizer_output: None,
            _tokens: None,
            _token_count: 0,
            _token_index: 0,
            _are_errors_suppressed: false,
            _parse_options: ParseOptions::new(),
            _diag_sink: P::new(DiagnosticSink::new(None)),
            _is_in_loop: false,
            _is_in_function: false,
            _is_in_exception_group: false,
            _is_parsing_type_annotation: false,
            _is_parsing_index_trailer: false,
            _is_parsing_quoted_text: false,
            _is_in_finally_block: false,
            _is_in_finally_loop: false,
            _future_imports: JsSet::default(),
            _imported_modules: vec![],
            _contains_wildcard_import: false,
            _assignment_expressions_allowed: true,
            _typing_import_aliases: vec![],
            _typing_symbol_aliases: JsMap::default(),
            _max_child_depth_map: FxHashMap::default(),
            _has_type_annotations: false,
            _owner_key: None,
            _created_owner_key: false,
            _string_annotations: create_string_annotation_info(),
        }
    }

    // parser/parser.ts:267
    pub fn parse_source_file(
        &mut self,
        file_contents: JsString,
        parse_options: ParseOptions,
        diag_sink: P<DiagnosticSink>,
        content_hash: Option<StringFingerprint>,
    ) -> ParseFileResults {
        self._has_type_annotations = false;
        self._string_annotations = create_string_annotation_info();
        let length = file_contents.len() as i32;
        timing_stats().tokenize_file_time.time_operation(|| {
            self._start_new_parse(
                file_contents.clone(),
                0,
                length,
                parse_options,
                diag_sink,
                0,
            );
        });

        let module_node = ModuleNode::create(&TextRange {
            start: 0,
            length: file_contents.len() as i32,
        });
        self._owner_key = Some(module_node.a);
        self._created_owner_key = true;

        timing_stats().parse_file_time.time_operation(|| {
            while !self._at_eof() {
                if !self._consume_token_if_type(TokenType::NewLine) {
                    // Handle a common error case and try to recover.
                    let next_token = self._peek_token(0);
                    if next_token.type_ == TokenType::Indent {
                        self._get_next_token();
                        let indent_token = IndentToken::cast(next_token).unwrap();
                        if indent_token.d().is_indent_ambiguous {
                            self._add_syntax_error(LocMessage::inconsistent_tabs(), &indent_token);
                        } else {
                            self._add_syntax_error(LocMessage::unexpected_indent(), &next_token);
                        }
                    }

                    let statement = self._parse_statement();
                    match statement {
                        None => {
                            // Perform basic error recovery to get to the next line.
                            self._consume_tokens_until_type(&[TokenType::NewLine]);
                        }
                        Some(statement) => {
                            statement.parent.set(Some(module_node.0));
                            module_node.d().statements.borrow_mut().push(statement);
                        }
                    }
                }
            }
        });

        self._attach_parser_string_annotations();

        let tokenizer_output = self._tokenizer_output.expect("tokenizer output");
        let content_hash = content_hash
            .unwrap_or_else(|| get_string_fingerprint(&file_contents.to_string_lossy()));
        ParseFileResults {
            text: file_contents,
            content_hash,
            parser_output: ParserOutput {
                parse_tree: module_node,
                string_annotations: self._string_annotations.info,
                imported_modules: std::mem::take(&mut self._imported_modules),
                future_imports: std::mem::take(&mut self._future_imports),
                contains_wildcard_import: self._contains_wildcard_import,
                typing_symbol_aliases: std::mem::take(&mut self._typing_symbol_aliases),
                has_type_annotations: self._has_type_annotations,
                lines: &tokenizer_output.get().lines,
            },
            tokenizer_output,
        }
    }

    /// The four TypeScript overloads are one function; `parse_tree` is a
    /// `FunctionAnnotationNode` for `ParseTextMode::FunctionAnnotation`.
    // parser/parser.ts:360
    pub fn parse_text_expression(
        &mut self,
        file_contents: JsString,
        text_offset: i32,
        text_length: i32,
        parse_options: ParseOptions,
        parse_text_mode: ParseTextMode,
        initial_paren_depth: i32,
        typing_symbol_aliases: Option<&JsMap<String, String>>,
        owner_key: Option<ParseTreeKey>,
    ) -> ParseExpressionTextResults {
        self._string_annotations = create_string_annotation_info();
        self._created_owner_key = owner_key.is_none();
        self._owner_key = Some(owner_key.unwrap_or_else(create_parse_tree_key));
        let diag_sink = P::new(DiagnosticSink::new(None));
        self._start_new_parse(
            file_contents,
            text_offset,
            text_length,
            parse_options,
            diag_sink,
            initial_paren_depth,
        );

        if let Some(typing_symbol_aliases) = typing_symbol_aliases {
            self._typing_symbol_aliases = typing_symbol_aliases.clone();
        }

        let parse_tree: Option<P<ParseNode>>;
        if parse_text_mode == ParseTextMode::VariableAnnotation {
            self._is_parsing_quoted_text = true;
            parse_tree = Some(self._parse_type_annotation(false));
        } else if parse_text_mode == ParseTextMode::FunctionAnnotation {
            self._is_parsing_quoted_text = true;
            parse_tree = self._parse_function_type_annotation().map(|n| n.0);
        } else {
            let expr_list_result = self._parse_test_or_star_expression_list(
                /* allow_assignment_expression */ false, /* allow_multiple_unpack */ true,
            );
            if let Some(parse_error) = expr_list_result.parse_error {
                parse_tree = Some(parse_error.0);
            } else {
                if expr_list_result.list.is_empty() {
                    let token = self._peek_token(0);
                    self._add_syntax_error(LocMessage::expected_expr(), &token);
                }
                parse_tree = Some(self._make_expression_or_tuple(
                    expr_list_result,
                    /* enclosed_in_parens */ false,
                ));
            }
        }

        if self._peek_token_type() == TokenType::NewLine {
            self._get_next_token();
        }

        if !self._at_eof() {
            let token = self._peek_token(0);
            self._add_syntax_error(LocMessage::unexpected_expr_token(), &token);
        }

        self._attach_parser_string_annotations();

        ParseExpressionTextResults {
            parse_tree,
            string_annotations: self._string_annotations.info,
            lines: &self
                ._tokenizer_output
                .expect("tokenizer output")
                .get()
                .lines,
            diagnostics: diag_sink.fetch_and_clear(),
        }
    }

    // Attaches parser-derived string annotations onto the owner key, but only when
    // this parser instance created that key and only when the tree actually
    // contains at least one quoted annotation.
    // parser/parser.ts:426
    fn _attach_parser_string_annotations(&mut self) {
        if self._created_owner_key
            && let Some(owner_key) = self._owner_key
            && self._string_annotations.info.size() > 0
        {
            set_parser_string_annotation_info(owner_key, self._string_annotations.info);
        }
    }

    // parser/parser.ts:432
    fn _get_owner_key(&self) -> ParseTreeKey {
        self._owner_key.expect("owner key")
    }

    // parser/parser.ts:437
    fn _start_new_parse(
        &mut self,
        file_contents: JsString,
        text_offset: i32,
        text_length: i32,
        parse_options: ParseOptions,
        diag_sink: P<DiagnosticSink>,
        initial_paren_depth: i32,
    ) {
        self._file_contents = Some(file_contents.clone());
        self._parse_options = parse_options;
        self._diag_sink = diag_sink;

        // Tokenize the file contents.
        let mut tokenizer = Tokenizer::new();
        let tokenizer_output = P::new(tokenizer.tokenize(
            &file_contents,
            Some(text_offset),
            Some(text_length),
            initial_paren_depth,
            self._parse_options.use_notebook_mode,
        ));
        self._tokenizer_output = Some(tokenizer_output);
        self._tokens = Some(&tokenizer_output.get().tokens);
        self._token_count = tokenizer_output.tokens.count();
        self._token_index = 0;
    }

    // stmt: simple_stmt | compound_stmt
    // compound_stmt: if_stmt | while_stmt | for_stmt | try_stmt | with_stmt
    //   | funcdef | classdef | decorated | async_stmt
    // parser/parser.ts:466
    fn _parse_statement(&mut self) -> Option<StatementNode> {
        // Handle the errant condition of a dedent token here to provide
        // better recovery.
        if self._consume_token_if_type(TokenType::Dedent) {
            let token = self._peek_token(0);
            self._add_syntax_error(LocMessage::unexpected_unindent(), &token);
        }

        match self._peek_keyword_type() {
            Some(KeywordType::If) => return Some(self._parse_if_statement(KeywordType::If).0),

            Some(KeywordType::While) => return Some(self._parse_while_statement().0),

            Some(KeywordType::For) => return Some(self._parse_for_statement(None).0),

            Some(KeywordType::Try) => return Some(self._parse_try_statement().0),

            Some(KeywordType::With) => return Some(self._parse_with_statement(None).0),

            Some(KeywordType::Def) => return Some(self._parse_function_def(None, None)),

            Some(KeywordType::Class) => return Some(self._parse_class_def(None)),

            Some(KeywordType::Async) => return self._parse_async_statement(),

            Some(KeywordType::Match) => {
                // Match is considered a "soft" keyword, so we will treat
                // it as an identifier if it is followed by an unexpected
                // token.
                let peek_token = self._peek_token(1);
                let mut is_invalid_match_token = false;

                if peek_token.type_ == TokenType::Colon
                    || peek_token.type_ == TokenType::Semicolon
                    || peek_token.type_ == TokenType::Comma
                    || peek_token.type_ == TokenType::Dot
                    || peek_token.type_ == TokenType::NewLine
                    || peek_token.type_ == TokenType::EndOfStream
                {
                    is_invalid_match_token = true;
                } else if peek_token.type_ == TokenType::Operator {
                    let operator_type = operator_type_of(peek_token);
                    if operator_type != Some(OperatorType::Multiply)
                        && operator_type != Some(OperatorType::Add)
                        && operator_type != Some(OperatorType::BitwiseInvert)
                        && operator_type != Some(OperatorType::Subtract)
                    {
                        is_invalid_match_token = true;
                    }
                }

                if !is_invalid_match_token {
                    // Try to parse the match statement. If it doesn't appear to
                    // be a match statement, treat as a non-keyword and reparse.
                    if let Some(match_statement) = self._parse_match_statement() {
                        return Some(match_statement.0);
                    }
                }
            }

            _ => {}
        }

        if self._peek_operator_type() == Some(OperatorType::MatrixMultiply) {
            return Some(self._parse_decorated());
        }

        Some(self._parse_simple_statement().0)
    }

    // async_stmt: 'async' (funcdef | with_stmt | for_stmt)
    // parser/parser.ts:546
    fn _parse_async_statement(&mut self) -> Option<StatementNode> {
        let async_token = self._get_keyword_token(KeywordType::Async);

        match self._peek_keyword_type() {
            Some(KeywordType::Def) => {
                return Some(self._parse_function_def(Some(async_token), None));
            }

            Some(KeywordType::With) => {
                return Some(self._parse_with_statement(Some(async_token)).0);
            }

            Some(KeywordType::For) => return Some(self._parse_for_statement(Some(async_token)).0),

            _ => {}
        }

        self._add_syntax_error(LocMessage::unexpected_async_token(), &async_token);

        None
    }

    // lazy_import_stmt: "lazy" import_stmt | "lazy" from_import_stmt
    // parser/parser.ts:566
    fn _parse_lazy_import_statement(&mut self) -> P<ParseNode> {
        let lazy_token = self._get_keyword_token(KeywordType::Lazy);

        if !self._parse_options.is_stub_file
            && PythonVersion::is_less_than(&self._get_language_version(), &PYTHON_VERSION3_15)
        {
            self._add_syntax_error(LocMessage::lazy_import_illegal(), &lazy_token);
        }

        let next_token = self._peek_token(0);
        let node: P<ParseNode> = if next_token.type_ == TokenType::Keyword
            && keyword_type_of(next_token) == Some(KeywordType::From)
        {
            self._parse_from_statement().0
        } else {
            self._parse_import_statement().0
        };

        match node.d() {
            ParseNodeDetails::Import(d) => {
                d.is_lazy.set(Some(true));
                d.lazy_token.set(Some(lazy_token));
            }
            ParseNodeDetails::ImportFrom(d) => {
                d.is_lazy.set(Some(true));
                d.lazy_token.set(Some(lazy_token));
            }
            _ => unreachable!(),
        }
        extend_range(&node, &lazy_token);

        // PEP 810 disallows wildcard imports with 'lazy'.
        if let Some(import_from) = ImportFromNode::cast(node)
            && import_from.d().is_wildcard_import.get()
        {
            match import_from.d().wildcard_token.get() {
                Some(wildcard_token) => self
                    ._add_syntax_error(LocMessage::lazy_import_wildcard_illegal(), &wildcard_token),
                None => {
                    self._add_syntax_error(LocMessage::lazy_import_wildcard_illegal(), &lazy_token)
                }
            }
        }

        node
    }

    // type_alias_stmt: "type" name [type_param_seq] = expr
    // parser/parser.ts:595
    fn _parse_type_alias_statement(&mut self) -> TypeAliasNode {
        let type_token = self._get_keyword_token(KeywordType::Type);

        if !self._parse_options.is_stub_file
            && PythonVersion::is_less_than(&self._get_language_version(), &PYTHON_VERSION3_12)
        {
            self._add_syntax_error(LocMessage::type_alias_statement_illegal(), &type_token);
        }

        let name_token = self._get_token_if_identifier();
        assert!(name_token.is_some());
        let name = NameNode::create(self._get_owner_key(), name_token.unwrap());

        let mut type_parameters: Option<TypeParameterListNode> = None;
        if self._peek_token(0).type_ == TokenType::OpenBracket {
            type_parameters = Some(self._parse_type_parameter_list());
        }

        let assign_token = self._peek_token(0);
        if assign_token.type_ != TokenType::Operator
            || operator_type_of(assign_token) != Some(OperatorType::Assign)
        {
            self._add_syntax_error(LocMessage::expected_equals(), &assign_token);
        } else {
            self._get_next_token();
        }

        let was_parsing_type_annotation = self._is_parsing_type_annotation;
        self._is_parsing_type_annotation = true;
        let expression = self._parse_test_expression(/* allow_assignment_expression */ false);
        self._is_parsing_type_annotation = was_parsing_type_annotation;

        TypeAliasNode::create(
            self._get_owner_key(),
            type_token,
            name,
            expression,
            type_parameters,
        )
    }

    // type_param_seq: '[' (type_param ',')+ ']'
    // parser/parser.ts:630
    fn _parse_type_parameter_list(&mut self) -> TypeParameterListNode {
        let mut type_variable_nodes: Vec<TypeParameterNode> = vec![];

        let open_bracket_token = self._get_next_token();
        assert!(open_bracket_token.type_ == TokenType::OpenBracket);

        loop {
            let first_token = self._peek_token(0);

            if first_token.type_ == TokenType::CloseBracket {
                if type_variable_nodes.is_empty() {
                    let token = self._peek_token(0);
                    self._add_syntax_error(LocMessage::type_parameters_missing(), &token);
                }
                break;
            }

            let Some(type_var_node) = self._parse_type_parameter() else {
                break;
            };

            type_variable_nodes.push(type_var_node);

            if !self._consume_token_if_type(TokenType::Comma) {
                break;
            }
        }

        let closing_token = self._peek_token(0);
        if closing_token.type_ != TokenType::CloseBracket {
            let token = self._peek_token(0);
            self._add_syntax_error(LocMessage::expected_close_bracket(), &token);
            self._consume_tokens_until_type(&[
                TokenType::NewLine,
                TokenType::CloseBracket,
                TokenType::Colon,
            ]);
        } else {
            self._get_next_token();
        }

        TypeParameterListNode::create(
            self._get_owner_key(),
            open_bracket_token,
            closing_token,
            type_variable_nodes,
        )
    }

    // type_param: ['*' | '**'] NAME [':' bound_expr] ['=' default_expr]
    // parser/parser.ts:670
    fn _parse_type_parameter(&mut self) -> Option<TypeParameterNode> {
        let mut type_param_category = TypeParamKind::TypeVar;
        if self._consume_token_if_operator(OperatorType::Multiply) {
            type_param_category = TypeParamKind::TypeVarTuple;
        } else if self._consume_token_if_operator(OperatorType::Power) {
            type_param_category = TypeParamKind::ParamSpec;
        }

        let Some(name_token) = self._get_token_if_identifier() else {
            let token = self._peek_token(0);
            self._add_syntax_error(LocMessage::expected_type_parameter_name(), &token);
            return None;
        };

        let name = NameNode::create(self._get_owner_key(), name_token);

        let mut bound_expression: Option<ExpressionNode> = None;
        if self._consume_token_if_type(TokenType::Colon) {
            let expr = self._parse_expression(/* allow_unpack */ false);
            bound_expression = Some(expr);

            if type_param_category != TypeParamKind::TypeVar {
                self._add_syntax_error(LocMessage::type_parameter_bound_not_allowed(), &expr);
            }
        }

        let mut default_expression: Option<ExpressionNode> = None;
        if self._consume_token_if_operator(OperatorType::Assign) {
            let expr = self._parse_expression(
                /* allow_unpack */ type_param_category == TypeParamKind::TypeVarTuple,
            );
            default_expression = Some(expr);

            if !self._parse_options.is_stub_file
                && PythonVersion::is_less_than(&self._get_language_version(), &PYTHON_VERSION3_13)
            {
                self._add_syntax_error(LocMessage::type_var_default_illegal(), &expr);
            }
        }

        Some(TypeParameterNode::create(
            self._get_owner_key(),
            name,
            type_param_category,
            bound_expression,
            default_expression,
        ))
    }

    // match_stmt: "match" subject_expr ':' NEWLINE INDENT case_block+ DEDENT
    // subject_expr:
    //     | star_named_expression ',' star_named_expressions?
    //     | named_expression
    // parser/parser.ts:722
    fn _parse_match_statement(&mut self) -> Option<MatchNode> {
        // Parse the subject expression with errors suppressed. If it's not
        // followed by a colon, we'll assume this is not a match statement.
        let mut smells_like_match_statement = false;
        self._suppress_errors(|this| {
            let cur_token_index = this._token_index;

            this._get_keyword_token(KeywordType::Match);
            let expression = this._parse_test_or_star_list_as_expression(
                /* allow_assignment_expression */ true,
                /* allow_multiple_unpack */ true,
                ErrorExpressionCategory::MissingPatternSubject,
                LocMessage::expected_return_expr,
            );
            smells_like_match_statement = expression.node_type != ParseNodeType::Error
                && this._peek_token(0).type_ == TokenType::Colon;

            // Set the token index back to the start.
            this._token_index = cur_token_index;
        });

        if !smells_like_match_statement {
            return None;
        }

        let match_token = self._get_keyword_token(KeywordType::Match);

        let subject_expression = self._parse_test_or_star_list_as_expression(
            /* allow_assignment_expression */ true,
            /* allow_multiple_unpack */ true,
            ErrorExpressionCategory::MissingPatternSubject,
            LocMessage::expected_return_expr,
        );
        let match_node =
            MatchNode::create(self._get_owner_key(), match_token.0, subject_expression);

        let next_token = self._peek_token(0);

        if !self._consume_token_if_type(TokenType::Colon) {
            self._add_syntax_error(LocMessage::expected_colon(), &next_token);

            // Try to perform parse recovery by consuming tokens until
            // we find the end of the line.
            if self._consume_tokens_until_type(&[TokenType::NewLine, TokenType::Colon]) {
                self._get_next_token();
            }
        } else {
            extend_range(&match_node, &next_token);

            if !self._consume_token_if_type(TokenType::NewLine) {
                self._add_syntax_error(LocMessage::expected_newline(), &next_token);
            } else {
                let possible_indent = self._peek_token(0);
                if !self._consume_token_if_type(TokenType::Indent) {
                    let token = self._peek_token(0);
                    self._add_syntax_error(LocMessage::expected_indented_block(), &token);
                } else {
                    let indent_token = IndentToken::cast(possible_indent).unwrap();
                    if indent_token.d().is_indent_ambiguous {
                        self._add_syntax_error(LocMessage::inconsistent_tabs(), &indent_token);
                    }
                }

                loop {
                    // Handle a common error here and see if we can recover.
                    let possible_unexpected_indent = self._peek_token(0);
                    if possible_unexpected_indent.type_ == TokenType::Indent {
                        self._get_next_token();
                        let indent_token = IndentToken::cast(possible_unexpected_indent).unwrap();
                        if indent_token.d().is_indent_ambiguous {
                            self._add_syntax_error(LocMessage::inconsistent_tabs(), &indent_token);
                        } else {
                            self._add_syntax_error(
                                LocMessage::unexpected_indent(),
                                &possible_unexpected_indent,
                            );
                        }
                    }

                    match self._parse_case_statement() {
                        None => {
                            // Perform basic error recovery to get to the next line.
                            if self
                                ._consume_tokens_until_type(&[TokenType::NewLine, TokenType::Colon])
                            {
                                self._get_next_token();
                            }
                        }
                        Some(case_statement) => {
                            case_statement.parent.set(Some(match_node.0));
                            match_node.d().cases.borrow_mut().push(case_statement);
                        }
                    }

                    let dedent_token = self._peek_token(0);
                    if self._consume_token_if_type(TokenType::Dedent) {
                        let dedent_token = DedentToken::cast(dedent_token).unwrap();
                        if !dedent_token.d().matches_indent {
                            self._add_syntax_error(
                                LocMessage::inconsistent_indent(),
                                &dedent_token,
                            );
                        }
                        if dedent_token.d().is_dedent_ambiguous {
                            self._add_syntax_error(LocMessage::inconsistent_tabs(), &dedent_token);
                        }
                        break;
                    }

                    if self._peek_token_type() == TokenType::EndOfStream {
                        break;
                    }
                }
            }

            let last_case = match_node.d().cases.borrow().last().copied();
            if let Some(last_case) = last_case {
                extend_range(&match_node, &last_case);
            } else {
                self._add_syntax_error(LocMessage::zero_case_statements_found(), &match_token);
            }
        }

        // This feature requires Python 3.10.
        if PythonVersion::is_less_than(&self._get_language_version(), &PYTHON_VERSION3_10) {
            self._add_syntax_error(LocMessage::match_incompatible(), &match_token);
        }

        // Validate that only the last entry uses an irrefutable pattern.
        let cases = match_node.d().cases.borrow().clone();
        for i in 0..cases.len().saturating_sub(1) {
            let case_node = cases[i];
            if case_node.d().guard_expr.get().is_none() && case_node.d().is_irrefutable.get() {
                self._add_syntax_error(
                    LocMessage::case_pattern_is_irrefutable(),
                    &case_node.d().pattern.get(),
                );
            }
        }

        Some(match_node)
    }

    // case_block: "case" patterns [guard] ':' block
    // patterns: sequence_pattern | as_pattern
    // guard: 'if' named_expression
    // parser/parser.ts:853
    fn _parse_case_statement(&mut self) -> Option<CaseNode> {
        let case_token = self._peek_token(0);

        if !self._consume_token_if_keyword(KeywordType::Case) {
            self._add_syntax_error(LocMessage::expected_case(), &case_token);
            return None;
        }

        let pattern_list = self._parse_pattern_sequence();
        let case_pattern: PatternAtomNode;

        if let Some(parse_error) = pattern_list.parse_error {
            case_pattern = parse_error.0;
        } else if pattern_list.list.is_empty() {
            let token = self._peek_token(0);
            self._add_syntax_error(LocMessage::expected_pattern_expr(), &token);
            case_pattern = ErrorNode::create(
                self._get_owner_key(),
                &case_token,
                ErrorExpressionCategory::MissingPattern,
                None,
                None,
            )
            .0;
        } else if pattern_list.list.len() == 1 && !pattern_list.trailing_comma {
            let first = PatternAsNode::cast(pattern_list.list[0]).unwrap();
            let pattern = first.d().or_patterns.borrow()[0];

            if let Some(capture) = PatternCaptureNode::cast(pattern)
                && capture.d().is_star.get()
            {
                case_pattern = PatternSequenceNode::create(
                    self._get_owner_key(),
                    &first,
                    pattern_as_list(&pattern_list.list),
                )
                .0;
            } else {
                case_pattern = first.0;
            }
        } else {
            case_pattern = PatternSequenceNode::create(
                self._get_owner_key(),
                &pattern_list.list[0],
                pattern_as_list(&pattern_list.list),
            )
            .0;
        }

        if case_pattern.node_type != ParseNodeType::Error {
            let global_name_map: JsMap<String, NameNode> = JsMap::default();
            let mut local_name_map: JsMap<String, NameNode> = JsMap::default();
            self._report_duplicate_pattern_capture_targets(
                case_pattern,
                &global_name_map,
                &mut local_name_map,
            );
        }

        let mut guard_expression: Option<ExpressionNode> = None;
        if self._consume_token_if_keyword(KeywordType::If) {
            guard_expression =
                Some(self._parse_test_expression(/* allow_assignment_expression */ true));
        }

        let suite = self._parse_suite(self._is_in_function, false, None);
        let is_irrefutable = self._is_pattern_irrefutable(case_pattern);
        Some(CaseNode::create(
            self._get_owner_key(),
            &case_token,
            case_pattern,
            is_irrefutable,
            guard_expression,
            suite,
        ))
    }

    // PEP 634 defines the concept of an "irrefutable" pattern - a pattern that
    // will always be matched.
    // parser/parser.ts:905
    fn _is_pattern_irrefutable(&self, node: PatternAtomNode) -> bool {
        if node.node_type == ParseNodeType::PatternCapture {
            return true;
        }

        if let Some(pattern_as) = PatternAsNode::cast(node) {
            let or_patterns = pattern_as.d().or_patterns.borrow().clone();
            return or_patterns
                .iter()
                .any(|&pattern| self._is_pattern_irrefutable(pattern));
        }

        false
    }

    /// The `reportTargetIfDuplicate` closure of `_reportDuplicatePatternCaptureTargets`.
    fn _report_target_if_duplicate(
        &mut self,
        name_node: NameNode,
        global_name_map: &JsMap<String, NameNode>,
        local_name_map: &mut JsMap<String, NameNode>,
    ) {
        let value = &name_node.d().value;
        if global_name_map.contains_key(value) || local_name_map.contains_key(value) {
            self._add_syntax_error(
                format1(
                    LocMessage::duplicate_capture_pattern_target(),
                    "name",
                    value,
                ),
                &name_node,
            );
        } else {
            local_name_map.insert(value.clone(), name_node);
        }
    }

    // Reports any situations where a capture target (a variable that receives part of a pattern)
    // appears twice within the same pattern. This is complicated by the fact that duplicate targets
    // are allowed in separate "or" clauses, so we need to track the targets we've seen globally
    // as well as the targets we've seen locally within the current "or" clause.
    // parser/parser.ts:921
    fn _report_duplicate_pattern_capture_targets(
        &mut self,
        node: PatternAtomNode,
        global_name_map: &JsMap<String, NameNode>,
        local_name_map: &mut JsMap<String, NameNode>,
    ) {
        match node.d() {
            ParseNodeDetails::PatternSequence(d) => {
                let entries = d.entries.borrow().clone();
                for subpattern in entries {
                    self._report_duplicate_pattern_capture_targets(
                        subpattern.0,
                        global_name_map,
                        local_name_map,
                    );
                }
            }

            ParseNodeDetails::PatternClass(d) => {
                let args = d.args.borrow().clone();
                for arg in args {
                    self._report_duplicate_pattern_capture_targets(
                        arg.d().pattern.get().0,
                        global_name_map,
                        local_name_map,
                    );
                }
            }

            ParseNodeDetails::PatternAs(d) => {
                if let Some(target) = d.target.get() {
                    self._report_target_if_duplicate(target, global_name_map, local_name_map);
                }

                let or_patterns = d.or_patterns.borrow().clone();
                let or_local_name_maps: Vec<JsMap<String, NameNode>> = or_patterns
                    .iter()
                    .map(|&subpattern| {
                        let mut or_local_name_map: JsMap<String, NameNode> = JsMap::default();
                        self._report_duplicate_pattern_capture_targets(
                            subpattern,
                            local_name_map,
                            &mut or_local_name_map,
                        );
                        or_local_name_map
                    })
                    .collect();

                let mut combined_local_or_name_map: JsMap<String, NameNode> = JsMap::default();
                for or_local_name_map in &or_local_name_maps {
                    for &node in or_local_name_map.values() {
                        if !combined_local_or_name_map.contains_key(&node.d().value) {
                            combined_local_or_name_map.insert(node.d().value.clone(), node);
                            self._report_target_if_duplicate(node, global_name_map, local_name_map);
                        }
                    }
                }
            }

            ParseNodeDetails::PatternCapture(d) => {
                if !d.is_wildcard.get() {
                    self._report_target_if_duplicate(
                        d.target.get(),
                        global_name_map,
                        local_name_map,
                    );
                }
            }

            ParseNodeDetails::PatternMapping(d) => {
                let entries = d.entries.borrow().clone();
                for map_entry in entries {
                    if let Some(expand) = PatternMappingExpandEntryNode::cast(map_entry) {
                        self._report_target_if_duplicate(
                            expand.d().target.get(),
                            global_name_map,
                            local_name_map,
                        );
                    } else {
                        let key_entry = PatternMappingKeyEntryNode::cast(map_entry).unwrap();
                        self._report_duplicate_pattern_capture_targets(
                            key_entry.d().key_pattern.get(),
                            global_name_map,
                            local_name_map,
                        );
                        self._report_duplicate_pattern_capture_targets(
                            key_entry.d().value_pattern.get(),
                            global_name_map,
                            local_name_map,
                        );
                    }
                }
            }

            _ => {}
        }
    }

    // parser/parser.ts:1008
    fn _get_pattern_target_names(&self, node: PatternAtomNode, name_set: &mut JsSet<String>) {
        match node.d() {
            ParseNodeDetails::PatternSequence(d) => {
                for subpattern in d.entries.borrow().iter() {
                    self._get_pattern_target_names(subpattern.0, name_set);
                }
            }

            ParseNodeDetails::PatternClass(d) => {
                for arg in d.args.borrow().iter() {
                    self._get_pattern_target_names(arg.d().pattern.get().0, name_set);
                }
            }

            ParseNodeDetails::PatternAs(d) => {
                if let Some(target) = d.target.get() {
                    name_set.insert(target.d().value.clone());
                }
                for &subpattern in d.or_patterns.borrow().iter() {
                    self._get_pattern_target_names(subpattern, name_set);
                }
            }

            ParseNodeDetails::PatternCapture(d) => {
                if !d.is_wildcard.get() {
                    name_set.insert(d.target.get().d().value.clone());
                }
            }

            ParseNodeDetails::PatternMapping(d) => {
                for &map_entry in d.entries.borrow().iter() {
                    if let Some(expand) = PatternMappingExpandEntryNode::cast(map_entry) {
                        name_set.insert(expand.d().target.get().d().value.clone());
                    } else {
                        let key_entry = PatternMappingKeyEntryNode::cast(map_entry).unwrap();
                        self._get_pattern_target_names(key_entry.d().key_pattern.get(), name_set);
                        self._get_pattern_target_names(key_entry.d().value_pattern.get(), name_set);
                    }
                }
            }

            _ => {}
        }
    }

    // parser/parser.ts:1061
    fn _parse_pattern_sequence(&mut self) -> ListResult {
        let pattern_list =
            self._parse_expression_list_generic(&mut |this| this._parse_pattern_as().0, None, None);

        // Check for more than one star entry.
        let star_entries: Vec<PatternAsNode> = pattern_list
            .list
            .iter()
            .map(|&entry| PatternAsNode::cast(entry).unwrap())
            .filter(|entry| {
                let or_patterns = entry.d().or_patterns.borrow();
                or_patterns.len() == 1
                    && PatternCaptureNode::cast(or_patterns[0])
                        .is_some_and(|capture| capture.d().is_star.get())
            })
            .collect();

        if star_entries.len() > 1 {
            let target = star_entries[1].d().or_patterns.borrow()[0];
            self._add_syntax_error(LocMessage::duplicate_star_pattern(), &target);
        }

        pattern_list
    }

    // as_pattern: or_pattern ['as' NAME]
    // or_pattern: '|'.pattern_atom+
    // parser/parser.ts:1081
    fn _parse_pattern_as(&mut self) -> PatternAsNode {
        let mut or_patterns: Vec<PatternAtomNode> = vec![];

        loop {
            let pattern_atom = self._parse_pattern_atom();
            or_patterns.push(pattern_atom);

            if !self._consume_token_if_operator(OperatorType::BitwiseOr) {
                break;
            }
        }

        if or_patterns.len() > 1 {
            // Star patterns cannot be ORed with other patterns.
            for &pattern_atom in &or_patterns {
                if PatternCaptureNode::cast(pattern_atom)
                    .is_some_and(|capture| capture.d().is_star.get())
                {
                    self._add_syntax_error(LocMessage::star_pattern_in_or_pattern(), &pattern_atom);
                }
            }
        }

        let mut target: Option<NameNode> = None;
        if self._consume_token_if_keyword(KeywordType::As) {
            if let Some(name_token) = self._get_token_if_identifier() {
                target = Some(NameNode::create(self._get_owner_key(), name_token));
            } else {
                let token = self._peek_token(0);
                self._add_syntax_error(LocMessage::expected_name_after_as(), &token);
            }
        }

        // Star patterns cannot be used with AS pattern.
        if target.is_some()
            && or_patterns.len() == 1
            && PatternCaptureNode::cast(or_patterns[0])
                .is_some_and(|capture| capture.d().is_star.get())
        {
            self._add_syntax_error(LocMessage::star_pattern_in_as_pattern(), &or_patterns[0]);
        }

        // Validate that irrefutable patterns are not in any entries other than the last.
        for (index, &or_pattern) in or_patterns.iter().enumerate() {
            if index < or_patterns.len() - 1 && self._is_pattern_irrefutable(or_pattern) {
                self._add_syntax_error(LocMessage::or_pattern_irrefutable(), &or_pattern);
            }
        }

        // Validate that all bound variables are the same within all or patterns.
        let mut full_name_set: JsSet<String> = JsSet::default();
        for &or_pattern in &or_patterns {
            self._get_pattern_target_names(or_pattern, &mut full_name_set);
        }

        for &or_pattern in &or_patterns {
            let mut local_name_set: JsSet<String> = JsSet::default();
            self._get_pattern_target_names(or_pattern, &mut local_name_set);

            if local_name_set.len() < full_name_set.len() {
                let missing_names: Vec<&String> = full_name_set
                    .iter()
                    .filter(|name| !local_name_set.contains(*name))
                    .collect();
                let diag = DiagnosticAddendum::new();
                diag.add_message(format1(
                    LocAddendum::or_pattern_missing_name(),
                    "name",
                    &missing_names
                        .iter()
                        .map(|name| format!("\"{name}\""))
                        .collect::<Vec<_>>()
                        .join(", "),
                ));
                self._add_syntax_error(
                    LocMessage::or_pattern_missing_name()
                        + &diag.get_string(
                            DEFAULT_MAX_DIAGNOSTIC_DEPTH,
                            DEFAULT_MAX_DIAGNOSTIC_LINE_COUNT,
                        ),
                    &or_pattern,
                );
            }
        }

        PatternAsNode::create(self._get_owner_key(), or_patterns, target)
    }

    // pattern_atom:
    //     | literal_pattern
    //     | name_or_attr
    //     | '(' as_pattern ')'
    //     | '[' [sequence_pattern] ']'
    //     | '(' [sequence_pattern] ')'
    //     | '{' [items_pattern] '}'
    //     | name_or_attr '(' [pattern_arguments ','?] ')'
    // parser/parser.ts:1167
    fn _parse_pattern_atom(&mut self) -> PatternAtomNode {
        if let Some(pattern_literal) = self._parse_pattern_literal() {
            return pattern_literal.0;
        }

        if let Some(pattern_capture_or_value) = self._parse_pattern_capture_or_value() {
            let open_paren_token = self._peek_token(0);
            if pattern_capture_or_value.node_type == ParseNodeType::Error
                || !self._consume_token_if_type(TokenType::OpenParenthesis)
            {
                return pattern_capture_or_value;
            }

            let args = self._parse_class_pattern_arg_list();

            let class_name_expr = match pattern_capture_or_value.d() {
                ParseNodeDetails::PatternCapture(d) => d.target.get().0,
                ParseNodeDetails::PatternValue(d) => d.expr.get().0,
                _ => unreachable!(),
            };
            let class_pattern =
                PatternClassNode::create(self._get_owner_key(), class_name_expr, args);

            if !self._consume_token_if_type(TokenType::CloseParenthesis) {
                self._add_syntax_error(LocMessage::expected_close_paren(), &open_paren_token);

                // Consume the remainder of tokens on the line for error
                // recovery.
                self._consume_tokens_until_type(&[TokenType::NewLine]);

                // Extend the node's range to include the rest of the line.
                // This helps the signatureHelpProvider.
                let token = self._peek_token(0);
                extend_range(&class_pattern, &token);
            }

            return class_pattern.0;
        }

        let next_token = self._peek_token(0);
        let next_operator = self._peek_operator_type();

        if next_operator == Some(OperatorType::Multiply) {
            let star_token = self._get_next_token();
            let Some(identifier_token) = self._get_token_if_identifier() else {
                let token = self._peek_token(0);
                self._add_syntax_error(LocMessage::expected_identifier(), &token);
                return ErrorNode::create(
                    self._get_owner_key(),
                    &star_token,
                    ErrorExpressionCategory::MissingExpression,
                    None,
                    None,
                )
                .0;
            };
            return PatternCaptureNode::create(
                self._get_owner_key(),
                NameNode::create(self._get_owner_key(), identifier_token),
                Some(&star_token),
            )
            .0;
        }

        if next_token.type_ == TokenType::OpenParenthesis
            || next_token.type_ == TokenType::OpenBracket
        {
            let start_token = self._get_next_token();
            let pattern_list = self._parse_pattern_sequence();
            let case_pattern: PatternAtomNode;

            if let Some(parse_error) = pattern_list.parse_error {
                case_pattern = parse_error.0;
            } else if pattern_list.list.len() == 1
                && !pattern_list.trailing_comma
                && start_token.type_ == TokenType::OpenParenthesis
            {
                let first = PatternAsNode::cast(pattern_list.list[0]).unwrap();
                let pattern = first.d().or_patterns.borrow()[0];

                if PatternCaptureNode::cast(pattern)
                    .is_some_and(|capture| capture.d().is_star.get())
                {
                    case_pattern = PatternSequenceNode::create(
                        self._get_owner_key(),
                        &start_token,
                        pattern_as_list(&pattern_list.list),
                    )
                    .0;
                } else {
                    case_pattern = first.0;
                }

                extend_range(&case_pattern, &next_token);
            } else {
                case_pattern = PatternSequenceNode::create(
                    self._get_owner_key(),
                    &start_token,
                    pattern_as_list(&pattern_list.list),
                )
                .0;
            }

            let end_token = self._peek_token(0);
            if self._consume_token_if_type(if next_token.type_ == TokenType::OpenParenthesis {
                TokenType::CloseParenthesis
            } else {
                TokenType::CloseBracket
            }) {
                extend_range(&case_pattern, &end_token);
            } else {
                self._add_syntax_error(
                    if next_token.type_ == TokenType::OpenParenthesis {
                        LocMessage::expected_close_paren()
                    } else {
                        LocMessage::expected_close_bracket()
                    },
                    &next_token,
                );
                self._consume_tokens_until_type(&[
                    TokenType::Colon,
                    if next_token.type_ == TokenType::OpenParenthesis {
                        TokenType::CloseParenthesis
                    } else {
                        TokenType::CloseBracket
                    },
                ]);
            }

            return case_pattern;
        } else if next_token.type_ == TokenType::OpenCurlyBrace {
            let first_token = self._get_next_token();
            let mapping_pattern = self._parse_pattern_mapping(first_token);
            let last_token = self._peek_token(0);

            if self._consume_token_if_type(TokenType::CloseCurlyBrace) {
                extend_range(&mapping_pattern, &last_token);
            } else {
                self._add_syntax_error(LocMessage::expected_close_brace(), &next_token);
                self._consume_tokens_until_type(&[TokenType::Colon, TokenType::CloseCurlyBrace]);
            }

            return mapping_pattern;
        }

        self._handle_expression_parse_error(
            ErrorExpressionCategory::MissingPattern,
            LocMessage::expected_pattern_expr(),
            None,
            None,
            None,
        )
        .0
    }

    // pattern_arguments:
    //     | positional_patterns [',' keyword_patterns]
    //     | keyword_patterns
    // positional_patterns: ','.as_pattern+
    // keyword_patterns: ','.keyword_pattern+
    // parser/parser.ts:1296
    fn _parse_class_pattern_arg_list(&mut self) -> Vec<PatternClassArgumentNode> {
        let mut arg_list: Vec<PatternClassArgumentNode> = vec![];
        let mut saw_keyword_arg = false;

        loop {
            let next_token_type = self._peek_token_type();
            if next_token_type == TokenType::CloseParenthesis
                || next_token_type == TokenType::NewLine
                || next_token_type == TokenType::EndOfStream
            {
                break;
            }

            let arg = self._parse_class_pattern_argument();
            if arg.d().name.get().is_some() {
                saw_keyword_arg = true;
            } else if saw_keyword_arg && arg.d().name.get().is_none() {
                self._add_syntax_error(LocMessage::position_arg_after_named_arg(), &arg);
            }
            arg_list.push(arg);

            if !self._consume_token_if_type(TokenType::Comma) {
                break;
            }
        }

        arg_list
    }

    // keyword_pattern: NAME '=' as_pattern
    // parser/parser.ts:1327
    fn _parse_class_pattern_argument(&mut self) -> PatternClassArgumentNode {
        let first_token = self._peek_token(0);
        let second_token = self._peek_token(1);

        let mut keyword_name: Option<NameNode> = None;

        if (first_token.type_ == TokenType::Identifier || first_token.type_ == TokenType::Keyword)
            && second_token.type_ == TokenType::Operator
            && operator_type_of(second_token) == Some(OperatorType::Assign)
        {
            if let Some(class_name_token) = self._get_token_if_identifier() {
                keyword_name = Some(NameNode::create(self._get_owner_key(), class_name_token));
                self._get_next_token();
            }
        }

        let pattern = self._parse_pattern_as();

        PatternClassArgumentNode::create(self._get_owner_key(), pattern, keyword_name)
    }

    // literal_pattern:
    //     | signed_number
    //     | signed_number '+' NUMBER
    //     | signed_number '-' NUMBER
    //     | strings
    //     | 'None'
    //     | 'True'
    //     | 'False'
    // parser/parser.ts:1358
    fn _parse_pattern_literal(&mut self) -> Option<PatternLiteralNode> {
        let next_token = self._peek_token(0);
        let next_operator = self._peek_operator_type();

        if next_token.type_ == TokenType::Number || next_operator == Some(OperatorType::Subtract) {
            return Some(self._parse_pattern_literal_number());
        }

        if next_token.type_ == TokenType::String {
            let string_list = self._parse_atom();
            assert!(string_list.node_type == ParseNodeType::StringList);

            // Check for f-strings, which are not allowed.
            let strings = StringListNode(string_list).d().strings.borrow().clone();
            for string_atom in strings {
                if string_atom.node_type == ParseNodeType::FormatString {
                    self._add_syntax_error(LocMessage::format_string_in_pattern(), &string_atom);
                }
            }

            return Some(PatternLiteralNode::create(
                self._get_owner_key(),
                string_list,
            ));
        }

        if next_token.type_ == TokenType::Keyword {
            let keyword_type = keyword_type_of(next_token);
            if keyword_type == Some(KeywordType::False)
                || keyword_type == Some(KeywordType::True)
                || keyword_type == Some(KeywordType::None)
            {
                let atom = self._parse_atom();
                return Some(PatternLiteralNode::create(self._get_owner_key(), atom));
            }
        }

        None
    }

    // signed_number: NUMBER | '-' NUMBER
    // parser/parser.ts:1395
    fn _parse_pattern_literal_number(&mut self) -> PatternLiteralNode {
        let expression = self._parse_arithmetic_expression();
        let mut real_value: Option<ExpressionNode> = None;
        let mut imag_value: Option<ExpressionNode> = None;

        if let Some(binary) = BinaryOperationNode::cast(expression) {
            if binary.d().operator.get() == OperatorType::Subtract
                || binary.d().operator.get() == OperatorType::Add
            {
                real_value = Some(binary.d().left_expr.get());
                imag_value = Some(binary.d().right_expr.get());
            }
        } else {
            real_value = Some(expression);
        }

        if let Some(mut real) = real_value {
            if let Some(unary) = UnaryOperationNode::cast(real)
                && unary.d().operator.get() == OperatorType::Subtract
            {
                real = unary.d().expr.get();
            }

            let real_is_imaginary =
                NumberNode::cast(real).is_some_and(|n| n.d().is_imaginary.get());
            if real.node_type != ParseNodeType::Number
                || (imag_value.is_some() && real_is_imaginary)
            {
                self._add_syntax_error(LocMessage::expected_complex_number_literal(), &expression);
                imag_value = None;
            }
        }

        if let Some(mut imag) = imag_value {
            if let Some(unary) = UnaryOperationNode::cast(imag)
                && unary.d().operator.get() == OperatorType::Subtract
            {
                imag = unary.d().expr.get();
            }

            let imag_is_imaginary =
                NumberNode::cast(imag).is_some_and(|n| n.d().is_imaginary.get());
            if imag.node_type != ParseNodeType::Number || !imag_is_imaginary {
                self._add_syntax_error(LocMessage::expected_complex_number_literal(), &expression);
            }
        }

        PatternLiteralNode::create(self._get_owner_key(), expression)
    }

    // parser/parser.ts:1433
    fn _parse_pattern_mapping(&mut self, first_token: P<Token>) -> P<ParseNode> {
        let item_list = self._parse_expression_list_generic(
            &mut |this| this._parse_pattern_mapping_item(),
            None,
            None,
        );

        if !item_list.list.is_empty() {
            // Verify there's at most one ** entry.
            let star_star_entries: Vec<P<ParseNode>> = item_list
                .list
                .iter()
                .copied()
                .filter(|entry| entry.node_type == ParseNodeType::PatternMappingExpandEntry)
                .collect();
            if star_star_entries.len() > 1 {
                self._add_syntax_error(
                    LocMessage::duplicate_star_star_pattern(),
                    &star_star_entries[1],
                );
            }

            return PatternMappingNode::create(self._get_owner_key(), &first_token, item_list.list)
                .0;
        }

        match item_list.parse_error {
            Some(parse_error) => parse_error.0,
            None => {
                let token = self._peek_token(0);
                ErrorNode::create(
                    self._get_owner_key(),
                    &token,
                    ErrorExpressionCategory::MissingPattern,
                    None,
                    None,
                )
                .0
            }
        }
    }

    // key_value_pattern:
    //     | (literal_pattern | attr) ':' as_pattern
    //     | '**' NAME
    // parser/parser.ts:1457
    fn _parse_pattern_mapping_item(&mut self) -> P<ParseNode> {
        let mut key_expression: Option<P<ParseNode>> = None;
        let double_star = self._peek_token(0);

        if self._consume_token_if_operator(OperatorType::Power) {
            let Some(identifier_token) = self._get_token_if_identifier() else {
                let token = self._peek_token(0);
                self._add_syntax_error(LocMessage::expected_identifier(), &token);
                let token = self._peek_token(0);
                return ErrorNode::create(
                    self._get_owner_key(),
                    &token,
                    ErrorExpressionCategory::MissingPattern,
                    None,
                    None,
                )
                .0;
            };

            let name_node = NameNode::create(self._get_owner_key(), identifier_token);
            if identifier_token.d().value == "_" {
                self._add_syntax_error(LocMessage::star_star_wildcard_not_allowed(), &name_node);
            }

            return PatternMappingExpandEntryNode::create(
                self._get_owner_key(),
                &double_star,
                name_node,
            )
            .0;
        }

        if let Some(pattern_literal) = self._parse_pattern_literal() {
            key_expression = Some(pattern_literal.0);
        } else if let Some(pattern_capture_or_value) = self._parse_pattern_capture_or_value() {
            if pattern_capture_or_value.node_type == ParseNodeType::PatternValue {
                key_expression = Some(pattern_capture_or_value);
            } else {
                self._add_syntax_error(
                    LocMessage::expected_pattern_value(),
                    &pattern_capture_or_value,
                );
                let token = self._peek_token(0);
                key_expression = Some(
                    ErrorNode::create(
                        self._get_owner_key(),
                        &token,
                        ErrorExpressionCategory::MissingPattern,
                        None,
                        None,
                    )
                    .0,
                );
            }
        }

        let key_expression = match key_expression {
            Some(key_expression) => key_expression,
            None => {
                let token = self._peek_token(0);
                self._add_syntax_error(LocMessage::expected_pattern_expr(), &token);
                let token = self._peek_token(0);
                ErrorNode::create(
                    self._get_owner_key(),
                    &token,
                    ErrorExpressionCategory::MissingPattern,
                    None,
                    None,
                )
                .0
            }
        };

        let value_pattern: P<ParseNode> = if !self._consume_token_if_type(TokenType::Colon) {
            let token = self._peek_token(0);
            self._add_syntax_error(LocMessage::expected_colon(), &token);
            let token = self._peek_token(0);
            ErrorNode::create(
                self._get_owner_key(),
                &token,
                ErrorExpressionCategory::MissingPattern,
                None,
                None,
            )
            .0
        } else {
            self._parse_pattern_as().0
        };

        PatternMappingKeyEntryNode::create(self._get_owner_key(), key_expression, value_pattern).0
    }

    // parser/parser.ts:1519
    fn _parse_pattern_capture_or_value(&mut self) -> Option<P<ParseNode>> {
        let next_token = self._peek_token(0);

        if next_token.type_ == TokenType::Identifier || next_token.type_ == TokenType::Keyword {
            let mut name_or_member: Option<P<ParseNode>> = None;

            loop {
                if let Some(identifier_token) = self._get_token_if_identifier() {
                    let name_node = NameNode::create(self._get_owner_key(), identifier_token);
                    name_or_member = Some(match name_or_member {
                        Some(left) => {
                            MemberAccessNode::create(self._get_owner_key(), left, name_node).0
                        }
                        None => name_node.0,
                    });
                } else {
                    let token = self._peek_token(0);
                    self._add_syntax_error(LocMessage::expected_identifier(), &token);
                    break;
                }

                if !self._consume_token_if_type(TokenType::Dot) {
                    break;
                }
            }

            let Some(name_or_member) = name_or_member else {
                let token = self._peek_token(0);
                self._add_syntax_error(LocMessage::expected_identifier(), &token);
                let token = self._peek_token(0);
                return Some(
                    ErrorNode::create(
                        self._get_owner_key(),
                        &token,
                        ErrorExpressionCategory::MissingPattern,
                        None,
                        None,
                    )
                    .0,
                );
            };

            if let Some(member_access) = MemberAccessNode::cast(name_or_member) {
                return Some(PatternValueNode::create(self._get_owner_key(), member_access).0);
            }

            return Some(
                PatternCaptureNode::create(
                    self._get_owner_key(),
                    NameNode(name_or_member),
                    None::<&TextRange>,
                )
                .0,
            );
        }

        None
    }

    // if_stmt: 'if' test_suite ('elif' test_suite)* ['else' suite]
    // test_suite: test suite
    // test: or_test ['if' or_test 'else' test] | lambdef
    // parser/parser.ts:1560
    fn _parse_if_statement(&mut self, keyword_type: KeywordType) -> IfNode {
        let if_or_elif_token = self._get_keyword_token(keyword_type);

        let test = self._parse_test_expression(/* allow_assignment_expression */ true);
        let suite = self._parse_suite(self._is_in_function, false, None);
        let if_node = IfNode::create(self._get_owner_key(), if_or_elif_token.0, test, suite, None);

        if self._consume_token_if_keyword(KeywordType::Else) {
            let else_suite = self._parse_suite(self._is_in_function, false, None);
            if_node.d().else_suite.set(Some(else_suite.0));
            else_suite.parent.set(Some(if_node.0));
            extend_range(&if_node, &else_suite);
        } else if self._peek_keyword_type() == Some(KeywordType::Elif) {
            // Recursively handle an "elif" statement.
            let else_suite = self._parse_if_statement(KeywordType::Elif);
            if_node.d().else_suite.set(Some(else_suite.0));
            else_suite.parent.set(Some(if_node.0));
            extend_range(&if_node, &else_suite);
        }

        if_node
    }

    // parser/parser.ts:1581
    fn _parse_except_suite<T>(
        &mut self,
        is_exception_group: bool,
        callback: impl FnOnce(&mut Self) -> T,
    ) -> T {
        let was_in_exception_group = self._is_in_exception_group;

        if is_exception_group {
            self._is_in_exception_group = true;
        }
        let result = callback(self);

        self._is_in_exception_group = was_in_exception_group;

        result
    }

    // parser/parser.ts:1594
    fn _parse_loop_suite(&mut self) -> SuiteNode {
        let was_in_loop = self._is_in_loop;
        let was_in_exception_group = self._is_in_exception_group;
        self._is_in_exception_group = false;
        self._is_in_loop = true;

        // Record the fact that we are no longer in a finally block
        // that is contained within a loop. A loop within the finally
        // block resets this. See PEP 765 for details.
        let was_in_finally_loop = self._is_in_finally_loop;
        self._is_in_finally_loop = false;

        let mut type_comment: Option<StringToken> = None;
        let suite = self._parse_suite(
            self._is_in_function,
            /* skip_body */ false,
            Some(&mut |this: &mut Parser| {
                if let Some(comment) = this._get_type_annotation_comment_text() {
                    type_comment = Some(comment);
                }
            }),
        );

        self._is_in_loop = was_in_loop;
        self._is_in_finally_loop = was_in_finally_loop;
        self._is_in_exception_group = was_in_exception_group;

        if let Some(type_comment) = type_comment {
            suite.d().type_comment.set(Some(type_comment));
        }

        suite
    }

    // suite: ':' (simple_stmt | NEWLINE INDENT stmt+ DEDENT)
    // parser/parser.ts:1626
    fn _parse_suite(
        &mut self,
        is_function: bool,
        skip_body: bool,
        mut post_colon_callback: Option<&mut dyn FnMut(&mut Parser)>,
    ) -> SuiteNode {
        let next_token = self._peek_token(0);
        let suite = SuiteNode::create(self._get_owner_key(), &next_token);

        if !self._consume_token_if_type(TokenType::Colon) {
            self._add_syntax_error(LocMessage::expected_colon(), &next_token);

            // Try to perform parse recovery by consuming tokens.
            if self._consume_tokens_until_type(&[TokenType::NewLine, TokenType::Colon]) {
                if self._peek_token_type() == TokenType::Colon {
                    self._get_next_token();
                } else if self._peek_token(1).type_ != TokenType::Indent {
                    // Bail so we resume the at the next statement.
                    // We can't parse as a simple statement as we've skipped all but the newline.
                    self._get_next_token();
                    return suite;
                }
            }
        }

        if skip_body {
            if self._consume_token_if_type(TokenType::NewLine) {
                let mut indent = 0;
                loop {
                    let next_token = self._get_next_token();
                    if next_token.type_ == TokenType::Indent {
                        indent += 1;
                    }

                    if next_token.type_ == TokenType::Dedent {
                        if DedentToken::cast(next_token)
                            .unwrap()
                            .d()
                            .is_dedent_ambiguous
                        {
                            self._add_syntax_error(LocMessage::inconsistent_tabs(), &next_token);
                        }

                        indent -= 1;

                        if indent == 0 {
                            break;
                        }
                    }

                    if next_token.type_ == TokenType::EndOfStream {
                        break;
                    }
                }
            } else {
                // consume tokens
                self._parse_simple_statement();
            }

            if self._token_index > 0 {
                let token = self
                    ._tokenizer_output
                    .unwrap()
                    .tokens
                    .get_item_at(self._token_index - 1);
                extend_range(&suite, &token);
            }

            return suite;
        }

        if let Some(callback) = &mut post_colon_callback {
            callback(self);
        }

        let was_function = self._is_in_function;
        self._is_in_function = is_function;

        if self._consume_token_if_type(TokenType::NewLine) {
            if let Some(callback) = &mut post_colon_callback {
                callback(self);
            }

            let possible_indent = self._peek_token(0);
            if !self._consume_token_if_type(TokenType::Indent) {
                let token = self._peek_token(0);
                self._add_syntax_error(LocMessage::expected_indented_block(), &token);
                return suite;
            }

            let body_indent_token = IndentToken::cast(possible_indent).unwrap();
            if body_indent_token.d().is_indent_ambiguous {
                self._add_syntax_error(LocMessage::inconsistent_tabs(), &body_indent_token);
            }

            loop {
                // In notebook mode, an IPython magic or shell-escape line is consumed into a
                // comment, which can leave a bare NewLine token at the start of a suite body.
                // Skip such blank lines like the module-level loop does.
                if self._parse_options.use_notebook_mode
                    && self._peek_token_type() == TokenType::NewLine
                {
                    self._get_next_token();

                    // If the magic/blank line(s) were the entire suite body, this is a valid empty
                    // suite. Report any indentation inconsistency on the terminating dedent, then
                    // consume the dedent so it doesn't bubble up to the enclosing scope.
                    if self._peek_token_type() == TokenType::Dedent
                        && suite.d().statements.borrow().is_empty()
                    {
                        let dedent_token = DedentToken::cast(self._peek_token(0)).unwrap();
                        self._report_dedent_inconsistencies(dedent_token);
                        self._get_next_token();
                        extend_range(&suite, &dedent_token);
                        break;
                    }

                    if self._peek_token_type() == TokenType::EndOfStream {
                        break;
                    }
                    continue;
                }

                // Handle a common error here and see if we can recover.
                let next_token = self._peek_token(0);
                if next_token.type_ == TokenType::Indent {
                    self._get_next_token();
                    let indent_token = IndentToken::cast(next_token).unwrap();
                    if indent_token.d().is_indent_ambiguous {
                        self._add_syntax_error(LocMessage::inconsistent_tabs(), &indent_token);
                    } else {
                        self._add_syntax_error(LocMessage::unexpected_indent(), &next_token);
                    }
                } else if next_token.type_ == TokenType::Dedent {
                    // When we see a dedent, stop before parsing the dedented statement.
                    let dedent_token = DedentToken::cast(next_token).unwrap();
                    self._report_dedent_inconsistencies(dedent_token);

                    // When the suite is incomplete (no statements), leave the dedent token for
                    // recovery. This allows a single dedent token to cause us to break out of
                    // multiple levels of nested suites. Also extend the suite's range in this
                    // case so it is multi-line as this works better with indentationUtils.
                    if !suite.d().statements.borrow().is_empty() {
                        self._consume_token_if_type(TokenType::Dedent);
                    } else {
                        extend_range(&suite, &dedent_token);
                    }

                    // Did this dedent take us to an indent amount that is less than the
                    // initial indent of the suite body?
                    if dedent_token.d().indent_amount < body_indent_token.d().indent_amount {
                        break;
                    } else if dedent_token.d().indent_amount == body_indent_token.d().indent_amount
                    {
                        // If the next token is also a dedent that reduces the indent
                        // level to a less than the initial indent of the suite body, swallow
                        // the extra dedent to help recover the parse.
                        let next_token = self._peek_token(0);
                        if self._consume_token_if_type(TokenType::Dedent) {
                            extend_range(&suite, &next_token);
                            break;
                        }
                    }
                }

                match self._parse_statement() {
                    None => {
                        // Perform basic error recovery to get to the next line.
                        self._consume_tokens_until_type(&[TokenType::NewLine]);
                    }
                    Some(statement) => {
                        statement.parent.set(Some(suite.0));
                        suite.d().statements.borrow_mut().push(statement);
                    }
                }

                if self._peek_token_type() == TokenType::EndOfStream {
                    break;
                }
            }
        } else {
            let simple_statement = self._parse_simple_statement();
            suite.d().statements.borrow_mut().push(simple_statement.0);
            simple_statement.parent.set(Some(suite.0));
        }

        let last_statement = suite.d().statements.borrow().last().copied();
        if let Some(last_statement) = last_statement {
            extend_range(&suite, &last_statement);
        }

        self._is_in_function = was_function;

        suite
    }

    // Reports indentation inconsistencies (inconsistent indent / ambiguous tabs) for a terminating
    // dedent token.
    // parser/parser.ts:1807
    fn _report_dedent_inconsistencies(&mut self, dedent_token: DedentToken) {
        if !dedent_token.d().matches_indent {
            self._add_syntax_error(LocMessage::inconsistent_indent(), &dedent_token);
        }
        if dedent_token.d().is_dedent_ambiguous {
            self._add_syntax_error(LocMessage::inconsistent_tabs(), &dedent_token);
        }
    }

    // for_stmt: [async] 'for' exprlist 'in' testlist suite ['else' suite]
    // parser/parser.ts:1817
    fn _parse_for_statement(&mut self, async_token: Option<KeywordToken>) -> ForNode {
        let for_token = self._get_keyword_token(KeywordType::For);

        let target_expr = self._parse_expression_list_as_possible_tuple(
            ErrorExpressionCategory::MissingExpression,
            LocMessage::expected_expr,
            for_token.0,
        );

        let seq_expr: ExpressionNode;
        let for_suite: SuiteNode;
        let mut else_suite: Option<SuiteNode> = None;

        if !self._consume_token_if_keyword(KeywordType::In) {
            seq_expr = self
                ._handle_expression_parse_error(
                    ErrorExpressionCategory::MissingIn,
                    LocMessage::expected_in(),
                    None,
                    None,
                    None,
                )
                .0;
            let start = self._peek_token(0).start;
            for_suite = SuiteNode::create(
                self._get_owner_key(),
                &TextRange::create(start, /* length */ 0),
            );
        } else {
            seq_expr = self._parse_test_or_star_list_as_expression(
                /* allow_assignment_expression */ false,
                /* allow_multiple_unpack */ true,
                ErrorExpressionCategory::MissingExpression,
                LocMessage::expected_in_expr,
            );

            for_suite = self._parse_loop_suite();

            // Versions of Python earlier than 3.9 didn't allow unpack operators if the
            // tuple wasn't enclosed in parentheses.
            if PythonVersion::is_less_than(&self._get_language_version(), &PYTHON_VERSION3_9)
                && !self._parse_options.is_stub_file
            {
                if let Some(tuple) = TupleNode::cast(seq_expr)
                    && !tuple.d().has_parens.get()
                {
                    let mut saw_star = false;
                    let items = tuple.d().items.borrow().clone();
                    for expr in items {
                        if expr.node_type == ParseNodeType::Unpack && !saw_star {
                            self._add_syntax_error(
                                LocMessage::unpack_operator_not_allowed(),
                                &expr,
                            );
                            saw_star = true;
                        }
                    }
                }
            }

            if self._consume_token_if_keyword(KeywordType::Else) {
                else_suite = Some(self._parse_suite(self._is_in_function, false, None));
            }
        }

        let for_node = ForNode::create(
            self._get_owner_key(),
            for_token.0,
            target_expr,
            seq_expr,
            for_suite,
        );
        for_node.d().else_suite.set(else_suite);
        if let Some(else_suite) = else_suite {
            extend_range(&for_node, &else_suite);
            else_suite.parent.set(Some(for_node.0));
        }

        if let Some(async_token) = async_token {
            for_node.d().is_async.set(Some(true));
            for_node.d().async_token.set(Some(async_token.0));
            extend_range(&for_node, &async_token);
        }

        if let Some(type_comment) = for_suite.d().type_comment.get() {
            for_node.d().type_comment.set(Some(type_comment));
        }

        for_node
    }
}

/// `patternList.list` as `PatternAsNode[]`: `_parsePatternAs` builds every entry.
fn pattern_as_list(list: &[P<ParseNode>]) -> Vec<PatternAsNode> {
    list.iter()
        .map(|&entry| PatternAsNode::cast(entry).unwrap())
        .collect()
}

impl Parser {
    // comp_iter: comp_for | comp_if
    // parser/parser.ts:1886
    fn _try_parse_comprehension(
        &mut self,
        target: P<ParseNode>,
        is_generator: bool,
    ) -> Option<ComprehensionNode> {
        let comp_for = self._try_parse_comp_for_statement()?;

        if target.node_type == ParseNodeType::Unpack {
            self._add_syntax_error(LocMessage::unpack_illegal_in_comprehension(), &target);
        } else if target.node_type == ParseNodeType::DictionaryExpandEntry {
            self._add_syntax_error(LocMessage::dict_expand_illegal_in_comprehension(), &target);
        }

        let comp_node = ComprehensionNode::create(self._get_owner_key(), target, is_generator);

        let mut for_if_list: Vec<ComprehensionForIfNode> = vec![comp_for.0];
        loop {
            let comp_iter = match self._try_parse_comp_for_statement() {
                Some(comp_for) => Some(comp_for.0),
                None => self._try_parse_comp_if_statement().map(|comp_if| comp_if.0),
            };
            let Some(comp_iter) = comp_iter else {
                break;
            };
            comp_iter.parent.set(Some(comp_node.0));
            for_if_list.push(comp_iter);
        }

        *comp_node.d().for_if_nodes.borrow_mut() = for_if_list.clone();
        if !for_if_list.is_empty() {
            for comp in &for_if_list {
                comp.parent.set(Some(comp_node.0));
            }
            extend_range(&comp_node, &for_if_list[for_if_list.len() - 1]);
        }
        Some(comp_node)
    }

    // comp_for: ['async'] 'for' exprlist 'in' or_test [comp_iter]
    // parser/parser.ts:1922
    fn _try_parse_comp_for_statement(&mut self) -> Option<ComprehensionForNode> {
        let start_token_keyword_type = self._peek_keyword_type();

        if start_token_keyword_type == Some(KeywordType::Async) {
            let next_token = self._peek_token(1);
            if next_token.type_ != TokenType::Keyword
                || keyword_type_of(next_token) != Some(KeywordType::For)
            {
                return None;
            }
        } else if start_token_keyword_type != Some(KeywordType::For) {
            return None;
        }

        let mut async_token: Option<KeywordToken> = None;
        if self._peek_keyword_type() == Some(KeywordType::Async) {
            async_token = Some(self._get_keyword_token(KeywordType::Async));
        }

        let for_token = self._get_keyword_token(KeywordType::For);

        let target_expr = self._parse_expression_list_as_possible_tuple(
            ErrorExpressionCategory::MissingExpression,
            LocMessage::expected_expr,
            for_token.0,
        );
        let mut seq_expr: Option<ExpressionNode> = None;

        if !self._consume_token_if_keyword(KeywordType::In) {
            seq_expr = Some(
                self._handle_expression_parse_error(
                    ErrorExpressionCategory::MissingIn,
                    LocMessage::expected_in(),
                    None,
                    None,
                    None,
                )
                .0,
            );
        } else {
            self._disallow_assignment_expression(|this| {
                seq_expr = Some(this._parse_or_test());
            });
        }

        let comp_for_node = ComprehensionForNode::create(
            self._get_owner_key(),
            async_token.unwrap_or(for_token).0,
            target_expr,
            seq_expr.unwrap(),
        );

        if let Some(async_token) = async_token {
            comp_for_node.d().is_async.set(Some(true));
            comp_for_node.d().async_token.set(Some(async_token.0));
        }

        Some(comp_for_node)
    }

    // comp_if: 'if' test_nocond [comp_iter]
    // comp_iter: comp_for | comp_if
    // parser/parser.ts:1973
    fn _try_parse_comp_if_statement(&mut self) -> Option<ComprehensionIfNode> {
        if self._peek_keyword_type() != Some(KeywordType::If) {
            return None;
        }

        let if_token = self._get_keyword_token(KeywordType::If);
        let if_expr = match self._try_parse_lambda_expression(true) {
            Some(lambda) => lambda.0,
            None => {
                self._parse_assignment_expression(/* disallow_assignment_expression */ true)
            }
        };

        let comp_if_node = ComprehensionIfNode::create(self._get_owner_key(), if_token.0, if_expr);

        Some(comp_if_node)
    }

    // while_stmt: 'while' test suite ['else' suite]
    // parser/parser.ts:1989
    fn _parse_while_statement(&mut self) -> WhileNode {
        let while_token = self._get_keyword_token(KeywordType::While);

        let test_expr = self._parse_test_expression(/* allow_assignment_expression */ true);
        let while_suite = self._parse_loop_suite();
        let while_node =
            WhileNode::create(self._get_owner_key(), while_token.0, test_expr, while_suite);

        if self._consume_token_if_keyword(KeywordType::Else) {
            let else_suite = self._parse_suite(self._is_in_function, false, None);
            while_node.d().else_suite.set(Some(else_suite));
            else_suite.parent.set(Some(while_node.0));
            extend_range(&while_node, &else_suite);
        }

        while_node
    }

    // try_stmt: ('try' suite
    //         ((except_clause suite)+
    //             ['else' suite]
    //             ['finally' suite] |
    //         'finally' suite))
    // except_clause: 'except' [test ['as' NAME]]
    // parser/parser.ts:2014
    fn _parse_try_statement(&mut self) -> TryNode {
        let try_token = self._get_keyword_token(KeywordType::Try);
        let try_suite = self._parse_suite(self._is_in_function, false, None);
        let try_node = TryNode::create(self._get_owner_key(), try_token.0, try_suite);
        let mut saw_catch_all_except = false;
        let mut reported_except_group_mismatch = false;

        loop {
            let except_token = self._peek_token(0);
            if !self._consume_token_if_keyword(KeywordType::Except) {
                break;
            }

            // See if this is a Python 3.11 exception group.
            let possible_star_token = self._peek_token(0);
            let mut is_except_group = false;
            if self._consume_token_if_operator(OperatorType::Multiply) {
                if PythonVersion::is_less_than(&self._get_language_version(), &PYTHON_VERSION3_11)
                    && !self._parse_options.is_stub_file
                {
                    self._add_syntax_error(
                        LocMessage::exception_group_incompatible(),
                        &possible_star_token,
                    );
                }

                is_except_group = true;

                if !reported_except_group_mismatch
                    && try_node
                        .d()
                        .except_clauses
                        .borrow()
                        .iter()
                        .any(|clause| !clause.d().is_except_group.get())
                {
                    self._add_syntax_error(
                        LocMessage::except_group_mismatch(),
                        &possible_star_token,
                    );
                    reported_except_group_mismatch = true;
                }
            } else {
                if !reported_except_group_mismatch
                    && try_node
                        .d()
                        .except_clauses
                        .borrow()
                        .iter()
                        .any(|clause| clause.d().is_except_group.get())
                {
                    self._add_syntax_error(
                        LocMessage::except_group_mismatch(),
                        &possible_star_token,
                    );
                    reported_except_group_mismatch = true;
                }
            }

            let mut type_expr: Option<ExpressionNode> = None;
            let mut symbol_name: Option<IdentifierToken> = None;
            let mut is_as_keyword_allowed = true;

            if self._peek_token_type() != TokenType::Colon {
                let list_result = self._parse_expression_list_generic(
                    &mut |this| {
                        this._parse_test_expression(/* allow_assignment_expression */ true)
                    },
                    None,
                    None,
                );
                if let Some(parse_error) = list_result.parse_error {
                    type_expr = Some(parse_error.0);
                } else {
                    let list_len = list_result.list.len();
                    let expr = self._make_expression_or_tuple(
                        list_result,
                        /* enclosed_in_parens */ false,
                    );
                    type_expr = Some(expr);

                    // Python 3.14 allows more than one exception type to be provided in
                    // an except clause.
                    if list_len > 1 {
                        if PythonVersion::is_less_than(
                            &self._get_language_version(),
                            &PYTHON_VERSION3_14,
                        ) {
                            self._add_syntax_error(LocMessage::except_requires_parens(), &expr);
                        }

                        is_as_keyword_allowed = false;
                    }
                }

                if self._consume_token_if_keyword(KeywordType::As) {
                    if !is_as_keyword_allowed {
                        self._add_syntax_error(
                            LocMessage::except_with_as_requires_parens(),
                            &type_expr.unwrap(),
                        );
                    }

                    symbol_name = self._get_token_if_identifier();
                    if symbol_name.is_none() {
                        let token = self._peek_token(0);
                        self._add_syntax_error(LocMessage::expected_name_after_as(), &token);
                    }
                }
            } else if is_except_group {
                let token = self._peek_token(0);
                self._add_syntax_error(LocMessage::except_group_requires_type(), &token);
            }

            match type_expr {
                None => {
                    if saw_catch_all_except {
                        self._add_syntax_error(LocMessage::duplicate_catch_all(), &except_token);
                    }
                    saw_catch_all_except = true;
                }
                Some(type_expr) => {
                    if saw_catch_all_except {
                        self._add_syntax_error(
                            LocMessage::named_except_after_catch_all(),
                            &type_expr,
                        );
                    }
                }
            }

            let except_suite = self._parse_except_suite(is_except_group, |this| {
                this._parse_suite(this._is_in_function, false, None)
            });
            let except_node = ExceptNode::create(
                self._get_owner_key(),
                except_token,
                except_suite,
                is_except_group,
            );
            if let Some(type_expr) = type_expr {
                except_node.d().type_expr.set(Some(type_expr));
                type_expr.parent.set(Some(except_node.0));
            }

            if let Some(symbol_name) = symbol_name {
                let name = NameNode::create(self._get_owner_key(), symbol_name);
                except_node.d().name.set(Some(name));
                name.parent.set(Some(except_node.0));
            }

            try_node.d().except_clauses.borrow_mut().push(except_node);
            except_node.parent.set(Some(try_node.0));
        }

        let last_except_clause = try_node.d().except_clauses.borrow().last().copied();
        if let Some(last_except_clause) = last_except_clause {
            extend_range(&try_node, &last_except_clause);

            if self._consume_token_if_keyword(KeywordType::Else) {
                let else_suite = self._parse_suite(self._is_in_function, false, None);
                try_node.d().else_suite.set(Some(else_suite));
                else_suite.parent.set(Some(try_node.0));
                extend_range(&try_node, &else_suite);
            }
        }

        if self._consume_token_if_keyword(KeywordType::Finally) {
            let was_in_finally_block = self._is_in_finally_block;
            let was_in_finally_loop = self._is_in_finally_loop;
            self._is_in_finally_block = true;
            self._is_in_finally_loop = self._is_in_loop;

            let finally_suite = self._parse_suite(self._is_in_function, false, None);
            try_node.d().finally_suite.set(Some(finally_suite));

            self._is_in_finally_block = was_in_finally_block;
            self._is_in_finally_loop = was_in_finally_loop;

            finally_suite.parent.set(Some(try_node.0));
            extend_range(&try_node, &finally_suite);
        }

        if try_node.d().finally_suite.get().is_none()
            && try_node.d().except_clauses.borrow().is_empty()
        {
            self._add_syntax_error(LocMessage::try_without_except(), &try_token);
        }

        try_node
    }

    // funcdef: 'def' NAME parameters ['->' test] ':' suite
    // parameters: '(' [typedargslist] ')'
    // parser/parser.ts:2150
    fn _parse_function_def(
        &mut self,
        async_token: Option<KeywordToken>,
        decorators: Option<Vec<DecoratorNode>>,
    ) -> P<ParseNode> {
        let def_token = self._get_keyword_token(KeywordType::Def);

        let Some(name_token) = self._get_token_if_identifier() else {
            self._add_syntax_error(LocMessage::expected_function_name(), &def_token);
            return ErrorNode::create(
                self._get_owner_key(),
                &def_token,
                ErrorExpressionCategory::MissingFunctionParameterList,
                None,
                decorators,
            )
            .0;
        };

        let mut type_parameters: Option<TypeParameterListNode> = None;
        let possible_open_bracket = self._peek_token(0);
        if possible_open_bracket.type_ == TokenType::OpenBracket {
            let type_params = self._parse_type_parameter_list();
            type_parameters = Some(type_params);

            if !self._parse_options.is_stub_file
                && PythonVersion::is_less_than(&self._get_language_version(), &PYTHON_VERSION3_12)
            {
                self._add_syntax_error(
                    LocMessage::function_type_parameters_illegal(),
                    &type_params,
                );
            }
        }
        let open_paren_token = self._peek_token(0);
        if !self._consume_token_if_type(TokenType::OpenParenthesis) {
            let token = self._peek_token(0);
            self._add_syntax_error(LocMessage::expected_open_paren(), &token);
            return ErrorNode::create(
                self._get_owner_key(),
                &name_token,
                ErrorExpressionCategory::MissingFunctionParameterList,
                Some(NameNode::create(self._get_owner_key(), name_token).0),
                decorators,
            )
            .0;
        }

        let param_list = self._parse_var_args_list(
            TokenType::CloseParenthesis,
            /* allow_annotations */ true,
        );

        if !self._consume_token_if_type(TokenType::CloseParenthesis) {
            self._add_syntax_error(LocMessage::expected_close_paren(), &open_paren_token);
            self._consume_tokens_until_type(&[TokenType::Colon]);
        }

        let mut return_type: Option<ExpressionNode> = None;
        if self._consume_token_if_type(TokenType::Arrow) {
            return_type = Some(self._parse_type_annotation(false));
        }

        let mut function_type_annotation_token: Option<StringToken> = None;
        let was_in_exception_group = self._is_in_exception_group;
        self._is_in_exception_group = false;

        let was_in_finally_block = self._is_in_finally_block;
        let was_in_finally_loop = self._is_in_finally_loop;
        self._is_in_finally_block = false;
        self._is_in_finally_loop = false;

        let skip_body = self._parse_options.skip_function_and_class_body;
        let suite = self._parse_suite(
            /* is_function */ true,
            skip_body,
            Some(&mut |this: &mut Parser| {
                if function_type_annotation_token.is_none() {
                    function_type_annotation_token = this._get_type_annotation_comment_text();
                }
            }),
        );

        self._is_in_exception_group = was_in_exception_group;
        self._is_in_finally_block = was_in_finally_block;
        self._is_in_finally_loop = was_in_finally_loop;

        let function_node = FunctionNode::create(
            self._get_owner_key(),
            def_token.0,
            NameNode::create(self._get_owner_key(), name_token),
            suite,
            type_parameters,
        );
        if let Some(async_token) = async_token {
            function_node.d().is_async.set(true);
            extend_range(&function_node, &async_token);
        }

        *function_node.d().params.borrow_mut() = param_list.clone();
        for param in &param_list {
            param.parent.set(Some(function_node.0));
        }

        if let Some(decorators) = decorators {
            *function_node.d().decorators.borrow_mut() = decorators.clone();
            for decorator in &decorators {
                decorator.parent.set(Some(function_node.0));
            }

            if !decorators.is_empty() {
                extend_range(&function_node, &decorators[0]);
            }
        }

        if let Some(return_type) = return_type {
            function_node.d().return_annotation.set(Some(return_type));
            return_type.parent.set(Some(function_node.0));
            extend_range(&function_node, &return_type);
        }

        // If there was a type annotation comment for the function,
        // parse it now.
        if let Some(function_type_annotation_token) = function_type_annotation_token {
            self._parse_function_type_annotation_comment(
                function_type_annotation_token,
                function_node,
            );
        }

        function_node.0
    }

    // typedargslist: (
    //   tfpdef ['=' test] (',' tfpdef ['=' test])*
    //      [ ','
    //          [
    //              '*' [tfpdef] (',' tfpdef ['=' test])* [',' ['**' tfpdef [',']]]
    //              | '**' tfpdef [',']
    //          ]
    //      ]
    //   | '*' [tfpdef] (',' tfpdef ['=' test])* [',' ['**' tfpdef [',']]]
    //   | '**' tfpdef [','])
    // tfpdef: NAME [':' test]
    // vfpdef: NAME;
    // parser/parser.ts:2275
    fn _parse_var_args_list(
        &mut self,
        terminator: TokenType,
        allow_annotations: bool,
    ) -> Vec<ParameterNode> {
        let mut param_map: JsMap<String, String> = JsMap::default();
        let mut param_list: Vec<ParameterNode> = vec![];
        let mut saw_default_param = false;
        let mut reported_non_default_param_err = false;
        let mut saw_keyword_only_separator = false;
        let mut saw_position_only_separator = false;
        let mut saw_keyword_only_param_after_separator = false;
        let mut saw_args = false;
        let mut saw_kw_args = false;

        loop {
            if self._peek_token_type() == terminator {
                break;
            }

            let param = self._parse_parameter(allow_annotations);

            if let Some(param_name) = param.d().name.get() {
                let name = param_name.d().value.clone();
                if param_map.contains_key(&name) {
                    self._add_syntax_error(
                        format1(LocMessage::duplicate_param(), "name", &name),
                        &param_name,
                    );
                } else {
                    param_map.insert(name.clone(), name);
                }
            } else if param.d().category.get() == ParamCategory::Simple {
                if param_list.is_empty() {
                    self._add_syntax_error(LocMessage::position_only_first_param(), &param);
                }
            }

            if param.d().category.get() == ParamCategory::Simple {
                if param.d().name.get().is_none() {
                    if saw_position_only_separator {
                        self._add_syntax_error(LocMessage::duplicate_position_only(), &param);
                    } else if saw_keyword_only_separator {
                        self._add_syntax_error(
                            LocMessage::position_only_after_keyword_only(),
                            &param,
                        );
                    } else if saw_args {
                        self._add_syntax_error(LocMessage::position_only_after_args(), &param);
                    }
                    saw_position_only_separator = true;
                } else {
                    if saw_keyword_only_separator {
                        saw_keyword_only_param_after_separator = true;
                    }

                    if param.d().default_value.get().is_some() {
                        saw_default_param = true;
                    } else if saw_default_param && !saw_keyword_only_separator && !saw_args {
                        // Report this error only once.
                        if !reported_non_default_param_err {
                            self._add_syntax_error(LocMessage::non_default_after_default(), &param);
                            reported_non_default_param_err = true;
                        }
                    }
                }
            }

            param_list.push(param);

            if param.d().category.get() == ParamCategory::ArgsList {
                if param.d().name.get().is_none() {
                    if saw_keyword_only_separator {
                        self._add_syntax_error(LocMessage::duplicate_keyword_only(), &param);
                    } else if saw_args {
                        self._add_syntax_error(LocMessage::keyword_only_after_args(), &param);
                    }
                    saw_keyword_only_separator = true;
                } else {
                    if saw_keyword_only_separator || saw_args {
                        self._add_syntax_error(LocMessage::duplicate_args_param(), &param);
                    }
                    saw_args = true;
                }
            }

            if param.d().category.get() == ParamCategory::KwargsDict {
                if saw_kw_args {
                    self._add_syntax_error(LocMessage::duplicate_kwargs_param(), &param);
                }
                saw_kw_args = true;

                // A **kwargs cannot immediately follow a keyword-only separator ("*").
                if saw_keyword_only_separator && !saw_keyword_only_param_after_separator {
                    self._add_syntax_error(LocMessage::keyword_parameter_missing(), &param);
                }
            } else if saw_kw_args {
                self._add_syntax_error(LocMessage::param_after_kwargs_param(), &param);
            }

            let found_comma = self._consume_token_if_type(TokenType::Comma);

            if allow_annotations && param.d().annotation.get().is_none() {
                // Look for a type annotation comment at the end of the line.
                if let Some(type_annotation_comment) =
                    self._parse_variable_type_annotation_comment()
                {
                    param
                        .d()
                        .annotation_comment
                        .set(Some(type_annotation_comment));
                    type_annotation_comment.parent.set(Some(param.0));
                    extend_range(&param, &type_annotation_comment);
                }
            }

            if !found_comma {
                break;
            }
        }

        if let Some(&last_param) = param_list.last() {
            if last_param.d().category.get() == ParamCategory::ArgsList
                && last_param.d().name.get().is_none()
            {
                self._add_syntax_error(LocMessage::expected_named_parameter(), &last_param);
            }
        }

        param_list
    }

    // parser/parser.ts:2396
    fn _parse_parameter(&mut self, allow_annotations: bool) -> ParameterNode {
        let mut star_count = 0;
        let mut slash_count = 0;
        let first_token = self._peek_token(0);

        if self._consume_token_if_operator(OperatorType::Multiply) {
            star_count = 1;
        } else if self._consume_token_if_operator(OperatorType::Power) {
            star_count = 2;
        } else if self._consume_token_if_operator(OperatorType::Divide) {
            if PythonVersion::is_less_than(&self._get_language_version(), &PYTHON_VERSION3_8)
                && !self._parse_options.is_stub_file
            {
                self._add_syntax_error(LocMessage::position_only_incompatible(), &first_token);
            }
            slash_count = 1;
        }

        let param_name = self._get_token_if_identifier();
        if param_name.is_none() {
            if star_count == 1 {
                return ParameterNode::create(
                    self._get_owner_key(),
                    first_token,
                    ParamCategory::ArgsList,
                );
            } else if slash_count == 1 {
                return ParameterNode::create(
                    self._get_owner_key(),
                    first_token,
                    ParamCategory::Simple,
                );
            }

            // Check for the Python 2.x parameter sublist syntax and handle it gracefully.
            if self._peek_token_type() == TokenType::OpenParenthesis {
                let sublist_start = self._get_next_token();
                if self._consume_tokens_until_type(&[TokenType::CloseParenthesis]) {
                    self._get_next_token();
                }
                self._add_syntax_error(LocMessage::sublist_params_incompatible(), &sublist_start);
            } else {
                let token = self._peek_token(0);
                self._add_syntax_error(LocMessage::expected_param_name(), &token);
            }
        }

        let mut param_type = ParamCategory::Simple;
        if star_count == 1 {
            param_type = ParamCategory::ArgsList;
        } else if star_count == 2 {
            param_type = ParamCategory::KwargsDict;
        }
        let param_node = ParameterNode::create(self._get_owner_key(), first_token, param_type);
        if let Some(param_name) = param_name {
            let name = NameNode::create(self._get_owner_key(), param_name);
            param_node.d().name.set(Some(name));
            name.parent.set(Some(param_node.0));
            extend_range(&param_node, &param_name);
        }

        if allow_annotations && self._consume_token_if_type(TokenType::Colon) {
            let annotation = self._parse_type_annotation(param_type == ParamCategory::ArgsList);
            param_node.d().annotation.set(Some(annotation));
            annotation.parent.set(Some(param_node.0));
            extend_range(&param_node, &annotation);
        }

        if self._consume_token_if_operator(OperatorType::Assign) {
            let default_value =
                self._parse_test_expression(/* allow_assignment_expression */ false);
            param_node.d().default_value.set(Some(default_value));
            default_value.parent.set(Some(param_node.0));
            extend_range(&param_node, &default_value);

            if star_count > 0 {
                self._add_syntax_error(LocMessage::default_value_not_allowed(), &default_value);
            }
        }

        param_node
    }

    // with_stmt: 'with' with_item (',' with_item)*  ':' suite
    // Python 3.10 adds support for optional parentheses around
    // with_item list.
    // parser/parser.ts:2472
    fn _parse_with_statement(&mut self, async_token: Option<KeywordToken>) -> WithNode {
        let with_token = self._get_keyword_token(KeywordType::With);
        let mut with_item_list: Vec<WithItemNode> = vec![];

        let possible_paren = self._peek_token(0);

        // If the expression starts with a paren, parse it as though the
        // paren is enclosing the list of "with items". This is done as a
        // "dry run" to determine whether the entire list of "with items"
        // is enclosed in parentheses.
        let mut is_parenthesized_with_item_list = false;
        let mut is_parenthesized_disallowed = false;

        if possible_paren.type_ == TokenType::OpenParenthesis {
            let open_paren_token_index = self._token_index;

            self._suppress_errors(|this| {
                this._get_next_token();
                loop {
                    with_item_list.push(this._parse_with_item());
                    if !this._consume_token_if_type(TokenType::Comma) {
                        break;
                    }

                    if this._peek_token(0).type_ == TokenType::CloseParenthesis {
                        break;
                    }
                }

                if this._peek_token(0).type_ == TokenType::CloseParenthesis
                    && this._peek_token(1).type_ == TokenType::Colon
                {
                    is_parenthesized_with_item_list = true;

                    // Some forms of parenthesized context with statements were not
                    // allowed prior to Python 3.9. Is this such a form?
                    is_parenthesized_disallowed =
                        with_item_list.len() != 1 || with_item_list[0].d().target.get().is_some();
                }

                this._token_index = open_paren_token_index;
                with_item_list = vec![];
            });
        }

        if is_parenthesized_with_item_list {
            self._consume_token_if_type(TokenType::OpenParenthesis);
            if is_parenthesized_disallowed
                && PythonVersion::is_less_than(&self._get_language_version(), &PYTHON_VERSION3_9)
            {
                self._add_syntax_error(
                    LocMessage::parenthesized_context_manager_illegal(),
                    &possible_paren,
                );
            }
        }

        loop {
            with_item_list.push(self._parse_with_item());

            if !self._consume_token_if_type(TokenType::Comma) {
                break;
            }

            if self._peek_token(0).type_ == TokenType::CloseParenthesis {
                break;
            }
        }

        if is_parenthesized_with_item_list {
            if !self._consume_token_if_type(TokenType::CloseParenthesis) {
                self._add_syntax_error(LocMessage::expected_close_paren(), &possible_paren);
            }
        }

        let mut type_comment: Option<StringToken> = None;
        let with_suite = self._parse_suite(
            self._is_in_function,
            /* skip_body */ false,
            Some(&mut |this: &mut Parser| {
                if let Some(comment) = this._get_type_annotation_comment_text() {
                    type_comment = Some(comment);
                }
            }),
        );
        let with_node = WithNode::create(self._get_owner_key(), with_token.0, with_suite);
        if let Some(async_token) = async_token {
            with_node.d().is_async.set(Some(true));
            with_node.d().async_token.set(Some(async_token.0));
            extend_range(&with_node, &async_token);
        }

        if let Some(type_comment) = type_comment {
            with_node.d().type_comment.set(Some(type_comment));
        }

        *with_node.d().with_items.borrow_mut() = with_item_list.clone();
        for with_item in &with_item_list {
            with_item.parent.set(Some(with_node.0));
        }

        with_node
    }

    // with_item: test ['as' expr]
    // parser/parser.ts:2569
    fn _parse_with_item(&mut self) -> WithItemNode {
        let expr = self._parse_test_expression(/* allow_assignment_expression */ true);
        let item_node = WithItemNode::create(self._get_owner_key(), expr);

        if self._consume_token_if_keyword(KeywordType::As) {
            let target = self._parse_expression(/* allow_unpack */ false);
            item_node.d().target.set(Some(target));
            target.parent.set(Some(item_node.0));
            extend_range(&item_node, &target);
        }

        item_node
    }

    // decorators: decorator+
    // decorated: decorators (classdef | funcdef | async_funcdef)
    // parser/parser.ts:2584
    fn _parse_decorated(&mut self) -> StatementNode {
        let mut decorator_list: Vec<DecoratorNode> = vec![];

        loop {
            if self._peek_operator_type() == Some(OperatorType::MatrixMultiply) {
                decorator_list.push(self._parse_decorator());
            } else {
                break;
            }
        }

        let next_token = self._peek_token(0);
        if next_token.type_ == TokenType::Keyword {
            let keyword_type = keyword_type_of(next_token);
            if keyword_type == Some(KeywordType::Async) {
                self._get_next_token();

                if self._peek_keyword_type() != Some(KeywordType::Def) {
                    let token = self._peek_token(0);
                    self._add_syntax_error(LocMessage::expected_function_after_async(), &token);
                } else {
                    return self
                        ._parse_function_def(KeywordToken::cast(next_token), Some(decorator_list));
                }
            } else if keyword_type == Some(KeywordType::Def) {
                return self._parse_function_def(None, Some(decorator_list));
            } else if keyword_type == Some(KeywordType::Class) {
                return self._parse_class_def(Some(decorator_list));
            }
        }

        let token = self._peek_token(0);
        self._add_syntax_error(LocMessage::expected_after_decorator(), &token);

        // Return a dummy class declaration so the completion provider has
        // some parse nodes to work with.
        ClassNode::create_dummy_for_decorators(self._get_owner_key(), decorator_list).0
    }

    // decorator: '@' dotted_name [ '(' [arglist] ')' ] NEWLINE
    // parser/parser.ts:2620
    fn _parse_decorator(&mut self) -> DecoratorNode {
        let at_operator = self._get_next_token();
        assert!(operator_type_of(at_operator) == Some(OperatorType::MatrixMultiply));

        let expression = self._parse_test_expression(/* allow_assignment_expression */ true);

        // Versions of Python prior to 3.9 support a limited set of
        // expression forms.
        if PythonVersion::is_less_than(&self._get_language_version(), &PYTHON_VERSION3_9) {
            let mut is_supported_expression_form = false;
            if self._is_name_or_member_access_expression(expression) {
                is_supported_expression_form = true;
            } else if let Some(call) = CallNode::cast(expression)
                && self._is_name_or_member_access_expression(call.d().left_expr.get())
            {
                is_supported_expression_form = true;
            }

            if !is_supported_expression_form {
                self._add_syntax_error(LocMessage::expected_decorator_expr(), &expression);
            }
        }

        let decorator_node = DecoratorNode::create(self._get_owner_key(), at_operator, expression);

        if !self._consume_token_if_type(TokenType::NewLine) {
            let token = self._peek_token(0);
            self._add_syntax_error(LocMessage::expected_decorator_newline(), &token);
            self._consume_tokens_until_type(&[TokenType::NewLine]);
        }

        decorator_node
    }

    // parser/parser.ts:2654
    fn _is_name_or_member_access_expression(&self, expression: ExpressionNode) -> bool {
        if expression.node_type == ParseNodeType::Name {
            return true;
        } else if let Some(member_access) = MemberAccessNode::cast(expression) {
            return self._is_name_or_member_access_expression(member_access.d().left_expr.get());
        }

        false
    }

    // classdef: 'class' NAME ['(' [arglist] ')'] suite
    // parser/parser.ts:2665
    fn _parse_class_def(&mut self, decorators: Option<Vec<DecoratorNode>>) -> P<ParseNode> {
        let class_token = self._get_keyword_token(KeywordType::Class);

        let name_token = match self._get_token_if_identifier() {
            Some(name_token) => name_token,
            None => {
                let token = self._peek_token(0);
                self._add_syntax_error(LocMessage::expected_class_name(), &token);
                IdentifierToken::create(0, 0, String::new(), /* comments */ None)
            }
        };

        let mut type_parameters: Option<TypeParameterListNode> = None;
        let possible_open_bracket = self._peek_token(0);
        if possible_open_bracket.type_ == TokenType::OpenBracket {
            let type_params = self._parse_type_parameter_list();
            type_parameters = Some(type_params);

            if !self._parse_options.is_stub_file
                && PythonVersion::is_less_than(&self._get_language_version(), &PYTHON_VERSION3_12)
            {
                self._add_syntax_error(LocMessage::class_type_parameters_illegal(), &type_params);
            }
        }

        let mut arg_list: Vec<ArgumentNode> = vec![];
        let open_paren_token = self._peek_token(0);
        if self._consume_token_if_type(TokenType::OpenParenthesis) {
            arg_list = self._parse_arg_list().args;

            if !self._consume_token_if_type(TokenType::CloseParenthesis) {
                self._add_syntax_error(LocMessage::expected_close_paren(), &open_paren_token);
            }
        }

        let skip_body = self._parse_options.skip_function_and_class_body;
        let suite = self._parse_suite(/* is_function */ false, skip_body, None);

        let class_node = ClassNode::create(
            self._get_owner_key(),
            class_token.0,
            NameNode::create(self._get_owner_key(), name_token),
            suite,
            type_parameters,
        );
        *class_node.d().arguments.borrow_mut() = arg_list.clone();
        for arg in &arg_list {
            arg.parent.set(Some(class_node.0));
        }

        if let Some(decorators) = decorators {
            *class_node.d().decorators.borrow_mut() = decorators.clone();
            if !decorators.is_empty() {
                for decorator in &decorators {
                    decorator.parent.set(Some(class_node.0));
                }
                extend_range(&class_node, &decorators[0]);
            }
        }

        class_node.0
    }
}

impl Parser {
    // parser/parser.ts:2724
    fn _parse_pass_statement(&mut self) -> PassNode {
        let pass_token = self._get_keyword_token(KeywordType::Pass);
        PassNode::create(self._get_owner_key(), &pass_token)
    }

    // parser/parser.ts:2728
    fn _parse_break_statement(&mut self) -> BreakNode {
        let break_token = self._get_keyword_token(KeywordType::Break);

        if !self._is_in_loop {
            self._add_syntax_error(LocMessage::break_outside_loop(), &break_token);
        } else if self._is_in_exception_group {
            self._add_syntax_error(LocMessage::break_in_exception_group(), &break_token);
        }

        if self._is_in_finally_loop
            && PythonVersion::is_greater_or_equal_to(
                &self._get_language_version(),
                &PYTHON_VERSION3_14,
            )
        {
            self._add_syntax_error(LocMessage::finally_break(), &break_token);
        }

        BreakNode::create(self._get_owner_key(), &break_token)
    }

    // parser/parser.ts:2744
    fn _parse_continue_statement(&mut self) -> ContinueNode {
        let continue_token = self._get_keyword_token(KeywordType::Continue);

        if !self._is_in_loop {
            self._add_syntax_error(LocMessage::continue_outside_loop(), &continue_token);
        } else if self._is_in_exception_group {
            self._add_syntax_error(LocMessage::continue_in_exception_group(), &continue_token);
        }

        if self._is_in_finally_loop
            && PythonVersion::is_greater_or_equal_to(
                &self._get_language_version(),
                &PYTHON_VERSION3_14,
            )
        {
            self._add_syntax_error(LocMessage::finally_continue(), &continue_token);
        }

        ContinueNode::create(self._get_owner_key(), &continue_token)
    }

    // return_stmt: 'return' [testlist]
    // parser/parser.ts:2761
    fn _parse_return_statement(&mut self) -> ReturnNode {
        let return_token = self._get_keyword_token(KeywordType::Return);

        let return_node = ReturnNode::create(self._get_owner_key(), &return_token);

        if !self._is_in_function {
            self._add_syntax_error(LocMessage::return_outside_function(), &return_token);
        } else if self._is_in_exception_group {
            self._add_syntax_error(LocMessage::return_in_exception_group(), &return_token);
        }

        if self._is_in_finally_block
            && PythonVersion::is_greater_or_equal_to(
                &self._get_language_version(),
                &PYTHON_VERSION3_14,
            )
        {
            self._add_syntax_error(LocMessage::finally_return(), &return_token);
        }

        if !self._is_next_token_never_expression() {
            let return_expr = self._parse_test_or_star_list_as_expression(
                /* allow_assignment_expression */ true,
                /* allow_multiple_unpack */ true,
                ErrorExpressionCategory::MissingExpression,
                LocMessage::expected_return_expr,
            );
            self._report_conditional_error_for_star_tuple_element(return_expr, PYTHON_VERSION3_8);
            return_node.d().expr.set(Some(return_expr));
            return_expr.parent.set(Some(return_node.0));
            extend_range(&return_node, &return_expr);
        }

        return_node
    }

    // import_from: ('from' (('.' | '...')* dotted_name | ('.' | '...')+)
    //             'import' ('*' | '(' import_as_names ')' | import_as_names))
    // import_as_names: import_as_name (',' import_as_name)* [',']
    // import_as_name: NAME ['as' NAME]
    // parser/parser.ts:2796
    fn _parse_from_statement(&mut self) -> ImportFromNode {
        let from_token = self._get_keyword_token(KeywordType::From);

        let mod_name = self._parse_dotted_module_name(/* allow_just_dots */ true);
        let import_from_node =
            ImportFromNode::create(self._get_owner_key(), from_token.0, mod_name);

        // Handle imports from __future__ specially because they can
        // change the way we interpret the rest of the file.
        let is_future_import = {
            let name_parts = mod_name.d().name_parts.borrow();
            mod_name.d().leading_dots.get() == 0
                && name_parts.len() == 1
                && name_parts[0].d().value == "__future__"
        };

        let possible_input_token = self._peek_token(0);
        if !self._consume_token_if_keyword(KeywordType::Import) {
            let token = self._peek_token(0);
            self._add_syntax_error(LocMessage::expected_import(), &token);
            if mod_name.d().has_trailing_dot.get() != Some(true) {
                import_from_node.d().missing_import.set(Some(true));
            }
        } else {
            extend_range(&import_from_node, &possible_input_token);

            // Look for "*" token.
            let possible_star_token = self._peek_token(0);
            if self._consume_token_if_operator(OperatorType::Multiply) {
                extend_range(&import_from_node, &possible_star_token);
                import_from_node.d().is_wildcard_import.set(true);
                import_from_node
                    .d()
                    .wildcard_token
                    .set(Some(possible_star_token));
                self._contains_wildcard_import = true;
            } else {
                let open_paren_token = self._peek_token(0);
                let in_paren = self._consume_token_if_type(TokenType::OpenParenthesis);
                let mut trailing_comma_token: Option<P<Token>> = None;

                while let Some(import_name) = self._get_token_if_identifier() {
                    trailing_comma_token = None;

                    let import_from_as_node = ImportFromAsNode::create(
                        self._get_owner_key(),
                        NameNode::create(self._get_owner_key(), import_name),
                    );

                    if self._consume_token_if_keyword(KeywordType::As) {
                        match self._get_token_if_identifier() {
                            None => {
                                let token = self._peek_token(0);
                                self._add_syntax_error(LocMessage::expected_import_alias(), &token);
                            }
                            Some(alias_name) => {
                                let alias = NameNode::create(self._get_owner_key(), alias_name);
                                import_from_as_node.d().alias.set(Some(alias));
                                alias.parent.set(Some(import_from_as_node.0));
                                extend_range(&import_from_as_node, &alias_name);
                            }
                        }
                    }

                    import_from_node
                        .d()
                        .imports
                        .borrow_mut()
                        .push(import_from_as_node);
                    import_from_as_node.parent.set(Some(import_from_node.0));
                    extend_range(&import_from_node, &import_from_as_node);

                    if is_future_import {
                        // Add the future import by name.
                        self._future_imports.insert(import_name.d().value.clone());
                    }

                    let next_token = self._peek_token(0);
                    if !self._consume_token_if_type(TokenType::Comma) {
                        break;
                    }
                    trailing_comma_token = Some(next_token);
                }

                if import_from_node.d().imports.borrow().is_empty() {
                    let token = self._peek_token(0);
                    self._add_syntax_error(LocMessage::expected_import_symbols(), &token);
                }

                if in_paren {
                    import_from_node.d().uses_parens.set(true);

                    let next_token = self._peek_token(0);
                    if !self._consume_token_if_type(TokenType::CloseParenthesis) {
                        self._add_syntax_error(
                            LocMessage::expected_close_paren(),
                            &open_paren_token,
                        );
                    } else {
                        extend_range(&import_from_node, &next_token);
                    }
                } else if let Some(trailing_comma_token) = trailing_comma_token {
                    self._add_syntax_error(
                        LocMessage::trailing_comma_in_from_import(),
                        &trailing_comma_token,
                    );
                }
            }
        }

        let module = import_from_node.d().module.get();
        self._imported_modules.push(ModuleImport {
            name_node: module,
            leading_dots: module.d().leading_dots.get(),
            name_parts: module
                .d()
                .name_parts
                .borrow()
                .iter()
                .map(|p| p.d().value.clone())
                .collect(),
            imported_symbols: Some(
                import_from_node
                    .d()
                    .imports
                    .borrow()
                    .iter()
                    .map(|imp| imp.d().name.get().d().value.clone())
                    .collect(),
            ),
        });

        let mut is_typing_import = false;
        if module.d().name_parts.borrow().len() == 1 {
            let first_name_part_value = &module.d().name_parts.borrow()[0].d().value;
            if first_name_part_value == "typing" || first_name_part_value == "typing_extensions" {
                is_typing_import = true;
            }
        }

        if is_typing_import {
            let typing_symbols_of_interest = ["Literal", "TypeAlias", "Annotated"];

            if import_from_node.d().is_wildcard_import.get() {
                for s in typing_symbols_of_interest {
                    self._typing_symbol_aliases
                        .insert(s.to_string(), s.to_string());
                }
            } else {
                let imports = import_from_node.d().imports.borrow().clone();
                for imp in imports {
                    let name_value = &imp.d().name.get().d().value;
                    if typing_symbols_of_interest.iter().any(|s| s == name_value) {
                        // `imp.d.alias?.d.value || imp.d.name.d.value`: an empty alias falls back.
                        let key = match imp.d().alias.get() {
                            Some(alias) if !alias.d().value.is_empty() => alias.d().value.clone(),
                            _ => name_value.clone(),
                        };
                        self._typing_symbol_aliases.insert(key, name_value.clone());
                    }
                }
            }
        }

        import_from_node
    }

    // import_name: 'import' dotted_as_names
    // dotted_as_names: dotted_as_name (',' dotted_as_name)*
    // dotted_as_name: dotted_name ['as' NAME]
    // parser/parser.ts:2926
    fn _parse_import_statement(&mut self) -> ImportNode {
        let import_token = self._get_keyword_token(KeywordType::Import);

        let import_node = ImportNode::create(self._get_owner_key(), &import_token);

        loop {
            let mod_name = self._parse_dotted_module_name(false);

            let import_as_node = ImportAsNode::create(self._get_owner_key(), mod_name);

            if self._consume_token_if_keyword(KeywordType::As) {
                match self._get_token_if_identifier() {
                    Some(alias_token) => {
                        let alias = NameNode::create(self._get_owner_key(), alias_token);
                        import_as_node.d().alias.set(Some(alias));
                        alias.parent.set(Some(import_as_node.0));
                        extend_range(&import_as_node, &alias);
                    }
                    None => {
                        let token = self._peek_token(0);
                        self._add_syntax_error(LocMessage::expected_import_alias(), &token);
                    }
                }
            }

            let module = import_as_node.d().module.get();
            if module.d().leading_dots.get() > 0 {
                self._add_syntax_error(LocMessage::relative_import_not_allowed(), &module);
            }

            import_node.d().list.borrow_mut().push(import_as_node);
            import_as_node.parent.set(Some(import_node.0));

            let name_parts: Vec<String> = module
                .d()
                .name_parts
                .borrow()
                .iter()
                .map(|p| p.d().value.clone())
                .collect();

            if import_as_node.d().alias.get().is_some()
                || module.d().leading_dots.get() > 0
                || module.d().name_parts.borrow().is_empty()
            {
                self._imported_modules.push(ModuleImport {
                    name_node: module,
                    leading_dots: module.d().leading_dots.get(),
                    name_parts,
                    imported_symbols: None,
                });
            } else {
                // Implicitly import all modules in the multi-part name if we
                // are not assigning the final module to an alias.
                let count = module.d().name_parts.borrow().len();
                for index in 0..count {
                    self._imported_modules.push(ModuleImport {
                        name_node: module,
                        leading_dots: module.d().leading_dots.get(),
                        name_parts: name_parts[..index + 1].to_vec(),
                        imported_symbols: None,
                    });
                }
            }

            if mod_name.d().name_parts.borrow().len() == 1 {
                let first_name_part_value = mod_name.d().name_parts.borrow()[0].d().value.clone();
                if first_name_part_value == "typing" || first_name_part_value == "typing_extensions"
                {
                    // `importAsNode.d.alias?.d.value || firstNamePartValue`: an empty alias falls back.
                    let alias = match import_as_node.d().alias.get() {
                        Some(alias) if !alias.d().value.is_empty() => alias.d().value.clone(),
                        _ => first_name_part_value,
                    };
                    self._typing_import_aliases.push(alias);
                }
            }

            if !self._consume_token_if_type(TokenType::Comma) {
                break;
            }
        }

        let last = import_node.d().list.borrow().last().copied();
        if let Some(last) = last {
            extend_range(&import_node, &last);
        }

        import_node
    }

    // ('.' | '...')* dotted_name | ('.' | '...')+
    // dotted_name: NAME ('.' NAME)*
    // parser/parser.ts:3001
    fn _parse_dotted_module_name(&mut self, allow_just_dots: bool) -> ModuleNameNode {
        let token = self._peek_token(0);
        let module_name_node = ModuleNameNode::create(self._get_owner_key(), &token);

        loop {
            let token = match self._get_token_if_type(TokenType::Ellipsis) {
                Some(token) => Some(token),
                None => self._get_token_if_type(TokenType::Dot),
            };
            if let Some(token) = token {
                if token.type_ == TokenType::Ellipsis {
                    module_name_node
                        .d()
                        .leading_dots
                        .set(module_name_node.d().leading_dots.get() + 3);
                } else {
                    module_name_node
                        .d()
                        .leading_dots
                        .set(module_name_node.d().leading_dots.get() + 1);
                }

                extend_range(&module_name_node, &token);
            } else {
                break;
            }
        }

        loop {
            let Some(identifier) = self._get_token_if_identifier() else {
                if !allow_just_dots
                    || module_name_node.d().leading_dots.get() == 0
                    || !module_name_node.d().name_parts.borrow().is_empty()
                {
                    let token = self._peek_token(0);
                    self._add_syntax_error(LocMessage::expected_module_name(), &token);
                    module_name_node.d().has_trailing_dot.set(Some(true));
                }
                break;
            };

            let name_part = NameNode::create(self._get_owner_key(), identifier);
            module_name_node.d().name_parts.borrow_mut().push(name_part);
            name_part.parent.set(Some(module_name_node.0));
            extend_range(&module_name_node, &name_part);

            let next_token = self._peek_token(0);
            if !self._consume_token_if_type(TokenType::Dot) {
                break;
            }

            // Extend the module name to include the dot.
            extend_range(&module_name_node, &next_token);
        }

        module_name_node
    }

    // parser/parser.ts:3046
    fn _parse_global_statement(&mut self) -> GlobalNode {
        let global_token = self._get_keyword_token(KeywordType::Global);

        let global_node = GlobalNode::create(self._get_owner_key(), &global_token);
        let targets = self._parse_name_list();
        *global_node.d().targets.borrow_mut() = targets.clone();
        if !targets.is_empty() {
            for name in &targets {
                name.parent.set(Some(global_node.0));
            }
            extend_range(&global_node, &targets[targets.len() - 1]);
        }
        global_node
    }

    // parser/parser.ts:3060
    fn _parse_nonlocal_statement(&mut self) -> NonlocalNode {
        let nonlocal_token = self._get_keyword_token(KeywordType::Nonlocal);

        let nonlocal_node = NonlocalNode::create(self._get_owner_key(), &nonlocal_token);
        let targets = self._parse_name_list();
        *nonlocal_node.d().targets.borrow_mut() = targets.clone();
        if !targets.is_empty() {
            for name in &targets {
                name.parent.set(Some(nonlocal_node.0));
            }
            extend_range(&nonlocal_node, &targets[targets.len() - 1]);
        }
        nonlocal_node
    }

    // parser/parser.ts:3074
    fn _parse_name_list(&mut self) -> Vec<NameNode> {
        let mut name_list: Vec<NameNode> = vec![];

        loop {
            let Some(name) = self._get_token_if_identifier() else {
                let token = self._peek_token(0);
                self._add_syntax_error(LocMessage::expected_identifier(), &token);
                break;
            };

            name_list.push(NameNode::create(self._get_owner_key(), name));

            if !self._consume_token_if_type(TokenType::Comma) {
                break;
            }
        }

        name_list
    }

    // raise_stmt: 'raise' [test ['from' test]]
    // (old) raise_stmt: 'raise' [test [',' test [',' test]]]
    // parser/parser.ts:3096
    fn _parse_raise_statement(&mut self) -> RaiseNode {
        let raise_token = self._get_keyword_token(KeywordType::Raise);

        let raise_node = RaiseNode::create(self._get_owner_key(), &raise_token);
        if !self._is_next_token_never_expression() {
            let expr = self._parse_test_expression(/* allow_assignment_expression */ true);
            raise_node.d().expr.set(Some(expr));
            expr.parent.set(Some(raise_node.0));
            extend_range(&raise_node, &expr);

            if self._consume_token_if_keyword(KeywordType::From) {
                let from_expr =
                    self._parse_test_expression(/* allow_assignment_expression */ true);
                raise_node.d().from_expr.set(Some(from_expr));
                from_expr.parent.set(Some(raise_node.0));
                extend_range(&raise_node, &from_expr);
            }
        }

        raise_node
    }

    // assert_stmt: 'assert' test [',' test]
    // parser/parser.ts:3116
    fn _parse_assert_statement(&mut self) -> AssertNode {
        let assert_token = self._get_keyword_token(KeywordType::Assert);

        let expr = self._parse_test_expression(/* allow_assignment_expression */ false);
        let assert_node = AssertNode::create(self._get_owner_key(), assert_token.0, expr);

        if self._consume_token_if_type(TokenType::Comma) {
            let exception_expr =
                self._parse_test_expression(/* allow_assignment_expression */ false);
            assert_node.d().exception_expr.set(Some(exception_expr));
            exception_expr.parent.set(Some(assert_node.0));
            extend_range(&assert_node, &exception_expr);
        }

        assert_node
    }

    // del_stmt: 'del' exprlist
    // parser/parser.ts:3133
    fn _parse_del_statement(&mut self) -> DelNode {
        let del_token = self._get_keyword_token(KeywordType::Del);

        let expr_list_result = self._parse_expression_list(/* allow_star */ true);
        if expr_list_result.parse_error.is_none() && expr_list_result.list.is_empty() {
            let token = self._peek_token(0);
            self._add_syntax_error(LocMessage::expected_del_expr(), &token);
        }
        let del_node = DelNode::create(self._get_owner_key(), del_token.0);
        let targets = expr_list_result.list;
        *del_node.d().targets.borrow_mut() = targets.clone();
        if !targets.is_empty() {
            for expr in &targets {
                expr.parent.set(Some(del_node.0));
            }
            extend_range(&del_node, &targets[targets.len() - 1]);
        }
        del_node
    }

    // yield_expr: 'yield' [yield_arg]
    // yield_arg: 'from' test | testlist
    // parser/parser.ts:3153
    fn _parse_yield_expression(&mut self) -> P<ParseNode> {
        let yield_token = self._get_keyword_token(KeywordType::Yield);

        let next_token = self._peek_token(0);
        if self._consume_token_if_keyword(KeywordType::From) {
            if PythonVersion::is_less_than(&self._get_language_version(), &PYTHON_VERSION3_3) {
                self._add_syntax_error(LocMessage::yield_from_illegal(), &next_token);
            }
            let expr = self._parse_test_expression(/* allow_assignment_expression */ false);
            return YieldFromNode::create(self._get_owner_key(), yield_token.0, expr).0;
        }

        let mut expr_list: Option<ExpressionNode> = None;
        if !self._is_next_token_never_expression() {
            let expr = self._parse_test_or_star_list_as_expression(
                /* allow_assignment_expression */ false,
                /* allow_multiple_unpack */ true,
                ErrorExpressionCategory::MissingExpression,
                LocMessage::expected_yield_expr,
            );
            expr_list = Some(expr);
            self._report_conditional_error_for_star_tuple_element(expr, PYTHON_VERSION3_8);
        }

        YieldNode::create(self._get_owner_key(), yield_token.0, expr_list).0
    }

    // parser/parser.ts:3182
    fn _try_parse_yield_expression(&mut self) -> Option<P<ParseNode>> {
        if self._peek_keyword_type() != Some(KeywordType::Yield) {
            return None;
        }

        Some(self._parse_yield_expression())
    }
}

impl Parser {
    // simple_stmt: small_stmt (';' small_stmt)* [';'] NEWLINE
    // parser/parser.ts:3191
    fn _parse_simple_statement(&mut self) -> StatementListNode {
        let token = self._peek_token(0);
        let statement = StatementListNode::create(self._get_owner_key(), token);

        loop {
            // Swallow invalid tokens to make sure we make forward progress.
            if self._peek_token_type() == TokenType::Invalid {
                let invalid_token = self._get_next_token();
                let first_char_code =
                    self._file_contents.as_ref().unwrap()[invalid_token.start as usize];

                // If the invalid token is a line-continuation backslash at the end of the file,
                // report a clearer error message consistent with Python: "Unexpected EOF".
                let next_tok = self._peek_token(0);
                let next_next_tok = self._peek_token(1);
                let is_backslash = first_char_code == Char::Backslash;
                let is_at_eof_line_continuation = is_backslash
                    && next_tok.type_ == TokenType::NewLine
                    && next_next_tok.type_ == TokenType::EndOfStream;

                if is_at_eof_line_continuation {
                    self._add_syntax_error(LocMessage::unexpected_eof(), &invalid_token);
                } else {
                    // Remove any non-printable characters.
                    self._add_syntax_error(
                        format1(
                            LocMessage::invalid_token_chars(),
                            "text",
                            &format!("\\u{first_char_code:x}"),
                        ),
                        &invalid_token,
                    );
                }

                self._consume_tokens_until_type(&[TokenType::NewLine]);
                break;
            }

            let small_statement = self._parse_small_statement();
            statement.d().statements.borrow_mut().push(small_statement);
            small_statement.parent.set(Some(statement.0));
            extend_range(&statement, &small_statement);

            if small_statement.node_type == ParseNodeType::Error {
                // No need to log an error here. We assume that
                // it was already logged by _parseSmallStatement.
                break;
            }

            // Consume the semicolon if present.
            if !self._consume_token_if_type(TokenType::Semicolon) {
                break;
            }

            let next_token_type = self._peek_token_type();
            if next_token_type == TokenType::NewLine || next_token_type == TokenType::EndOfStream {
                break;
            }
        }

        if !self._consume_token_if_type(TokenType::NewLine) {
            let token = self._peek_token(0);
            self._add_syntax_error(LocMessage::expected_newline_or_semicolon(), &token);
        }

        statement
    }

    // small_stmt: (expr_stmt | del_stmt | pass_stmt | flow_stmt |
    //             import_stmt | global_stmt | nonlocal_stmt | assert_stmt)
    // flow_stmt: break_stmt | continue_stmt | return_stmt | raise_stmt | yield_stmt
    // import_stmt: import_name | import_from
    // parser/parser.ts:3257
    fn _parse_small_statement(&mut self) -> P<ParseNode> {
        match self._peek_keyword_type() {
            Some(KeywordType::Pass) => return self._parse_pass_statement().0,

            Some(KeywordType::Break) => return self._parse_break_statement().0,

            Some(KeywordType::Continue) => return self._parse_continue_statement().0,

            Some(KeywordType::Return) => return self._parse_return_statement().0,

            Some(KeywordType::From) => return self._parse_from_statement().0,

            Some(KeywordType::Import) => return self._parse_import_statement().0,

            Some(KeywordType::Global) => return self._parse_global_statement().0,

            Some(KeywordType::Nonlocal) => return self._parse_nonlocal_statement().0,

            Some(KeywordType::Raise) => return self._parse_raise_statement().0,

            Some(KeywordType::Assert) => return self._parse_assert_statement().0,

            Some(KeywordType::Del) => return self._parse_del_statement().0,

            Some(KeywordType::Yield) => return self._parse_yield_expression(),

            Some(KeywordType::Lazy) => {
                // Lazy is considered a "soft" keyword, so we will treat it
                // as an identifier if it is not followed by "import" or "from".
                let peek_token1 = self._peek_token(1);
                if peek_token1.type_ == TokenType::Keyword
                    && (keyword_type_of(peek_token1) == Some(KeywordType::Import)
                        || keyword_type_of(peek_token1) == Some(KeywordType::From))
                {
                    return self._parse_lazy_import_statement();
                }
            }

            Some(KeywordType::Type) => {
                // Type is considered a "soft" keyword, so we will treat it
                // as an identifier if it is followed by an unexpected token.

                let peek_token1 = self._peek_token(1);
                let peek_token2 = self._peek_token(2);
                let mut is_invalid_type_token = true;

                if peek_token1.type_ == TokenType::Identifier
                    || (peek_token1.type_ == TokenType::Keyword
                        && KeywordToken::cast(peek_token1)
                            .is_some_and(KeywordToken::is_soft_keyword))
                {
                    if peek_token2.type_ == TokenType::OpenBracket {
                        is_invalid_type_token = false;
                    } else if peek_token2.type_ == TokenType::Operator
                        && operator_type_of(peek_token2) == Some(OperatorType::Assign)
                    {
                        is_invalid_type_token = false;
                    }
                }

                if !is_invalid_type_token {
                    return self._parse_type_alias_statement().0;
                }
            }

            _ => {}
        }

        self._parse_expression_statement()
    }

    // parser/parser.ts:3341
    fn _make_expression_or_tuple(
        &mut self,
        expr_list_result: ListResult,
        enclosed_in_parens: bool,
    ) -> ExpressionNode {
        // A single-element tuple with no trailing comma is simply an expression
        // that's surrounded by parens.
        if expr_list_result.list.len() == 1 && !expr_list_result.trailing_comma {
            if expr_list_result.list[0].node_type == ParseNodeType::Unpack {
                self._add_syntax_error(
                    LocMessage::unpack_operator_not_allowed(),
                    &expr_list_result.list[0],
                );
            }
            return expr_list_result.list[0];
        }

        // To accommodate empty tuples ("()"), we will reach back to get
        // the opening parenthesis as the opening token.

        let tuple_start_range: TextRange = if !expr_list_result.list.is_empty() {
            expr_list_result.list[0].text_range()
        } else {
            self._peek_token(-1).text_range()
        };

        let tuple_node = TupleNode::create(
            self._get_owner_key(),
            &tuple_start_range,
            enclosed_in_parens,
        );
        *tuple_node.d().items.borrow_mut() = expr_list_result.list.clone();
        if !expr_list_result.list.is_empty() {
            for expr in &expr_list_result.list {
                expr.parent.set(Some(tuple_node.0));
            }
            extend_range(
                &tuple_node,
                &expr_list_result.list[expr_list_result.list.len() - 1],
            );
        }

        tuple_node.0
    }

    // parser/parser.ts:3372
    fn _parse_expression_list_as_possible_tuple(
        &mut self,
        error_category: ErrorExpressionCategory,
        get_error_string: ErrorStringGetter,
        error_token: P<Token>,
    ) -> ExpressionNode {
        if self._is_next_token_never_expression() {
            self._add_syntax_error(get_error_string(), &error_token);
            return ErrorNode::create(
                self._get_owner_key(),
                &error_token,
                error_category,
                None,
                None,
            )
            .0;
        }

        let expr_list_result = self._parse_expression_list(/* allow_star */ true);
        if let Some(parse_error) = expr_list_result.parse_error {
            return parse_error.0;
        }
        self._make_expression_or_tuple(expr_list_result, /* enclosed_in_parens */ false)
    }

    // parser/parser.ts:3389
    fn _parse_test_list_as_expression(
        &mut self,
        error_category: ErrorExpressionCategory,
        get_error_string: ErrorStringGetter,
    ) -> ExpressionNode {
        if self._is_next_token_never_expression() {
            return self
                ._handle_expression_parse_error(
                    error_category,
                    get_error_string(),
                    None,
                    None,
                    None,
                )
                .0;
        }

        let expr_list_result = self._parse_test_expression_list();
        if let Some(parse_error) = expr_list_result.parse_error {
            return parse_error.0;
        }
        self._make_expression_or_tuple(expr_list_result, /* enclosed_in_parens */ false)
    }

    // parser/parser.ts:3404
    fn _parse_test_or_star_list_as_expression(
        &mut self,
        allow_assignment_expression: bool,
        allow_multiple_unpack: bool,
        error_category: ErrorExpressionCategory,
        get_error_string: ErrorStringGetter,
    ) -> ExpressionNode {
        if self._is_next_token_never_expression() {
            return self
                ._handle_expression_parse_error(
                    error_category,
                    get_error_string(),
                    None,
                    None,
                    None,
                )
                .0;
        }

        let expr_list_result = self._parse_test_or_star_expression_list(
            allow_assignment_expression,
            allow_multiple_unpack,
        );
        if let Some(parse_error) = expr_list_result.parse_error {
            return parse_error.0;
        }
        self._make_expression_or_tuple(expr_list_result, /* enclosed_in_parens */ false)
    }

    // parser/parser.ts:3421
    fn _parse_expression_list(&mut self, allow_star: bool) -> ListResult {
        self._parse_expression_list_generic(
            &mut |this| this._parse_expression(allow_star),
            None,
            None,
        )
    }

    // testlist: test (',' test)* [',']
    // parser/parser.ts:3426
    fn _parse_test_expression_list(&mut self) -> ListResult {
        self._parse_expression_list_generic(
            &mut |this| this._parse_test_expression(/* allow_assignment_expression */ false),
            None,
            None,
        )
    }

    // parser/parser.ts:3430
    fn _parse_test_or_star_expression_list(
        &mut self,
        allow_assignment_expression: bool,
        allow_multiple_unpack: bool,
    ) -> ListResult {
        let expr_list_result = self._parse_expression_list_generic(
            &mut |this| this._parse_test_or_star_expression(allow_assignment_expression),
            None,
            None,
        );

        if !allow_multiple_unpack && expr_list_result.parse_error.is_none() {
            let mut saw_star = false;
            for &expr in &expr_list_result.list {
                if expr.node_type == ParseNodeType::Unpack {
                    if saw_star {
                        self._add_syntax_error(LocMessage::duplicate_unpack(), &expr);
                        break;
                    }
                    saw_star = true;
                }
            }
        }

        expr_list_result
    }

    // exp_or_star: expr | star_expr
    // expr: xor_expr ('|' xor_expr)*
    // star_expr: '*' expr
    // parser/parser.ts:3457
    fn _parse_expression(&mut self, allow_unpack: bool) -> ExpressionNode {
        let start_token = self._peek_token(0);

        if allow_unpack && self._consume_token_if_operator(OperatorType::Multiply) {
            let expr = self._parse_expression(/* allow_unpack */ false);
            return UnpackNode::create(self._get_owner_key(), start_token, expr).0;
        }

        self._parse_bitwise_or_expression()
    }

    // test_or_star: test | star_expr
    // parser/parser.ts:3468
    fn _parse_test_or_star_expression(
        &mut self,
        allow_assignment_expression: bool,
    ) -> ExpressionNode {
        if self._peek_operator_type() == Some(OperatorType::Multiply) {
            return self._parse_expression(/* allow_unpack */ true);
        }

        self._parse_test_expression(allow_assignment_expression)
    }

    // test: or_test ['if' or_test 'else' test] | lambdef
    // parser/parser.ts:3477
    fn _parse_test_expression(&mut self, allow_assignment_expression: bool) -> ExpressionNode {
        if self._peek_keyword_type() == Some(KeywordType::Lambda) {
            return self._parse_lambda_expression(true).0;
        }

        let if_expr = self._parse_assignment_expression(!allow_assignment_expression);
        if if_expr.node_type == ParseNodeType::Error {
            return if_expr;
        }

        if !self._consume_token_if_keyword(KeywordType::If) {
            return if_expr;
        }

        let test_expr = self._parse_or_test();
        if test_expr.node_type == ParseNodeType::Error {
            return test_expr;
        }

        if !self._consume_token_if_keyword(KeywordType::Else) {
            let else_expr = self._handle_expression_parse_error(
                ErrorExpressionCategory::MissingElse,
                LocMessage::expected_else(),
                None,
                None,
                None,
            );
            return TernaryNode::create(self._get_owner_key(), if_expr, test_expr, else_expr.0).0;
        }

        let else_expr = self._parse_test_expression(/* allow_assignment_expression */ true);

        TernaryNode::create(self._get_owner_key(), if_expr, test_expr, else_expr).0
    }

    // assign_expr: NAME := test
    // parser/parser.ts:3511
    fn _parse_assignment_expression(
        &mut self,
        disallow_assignment_expression: bool,
    ) -> ExpressionNode {
        let left_expr = self._parse_or_test();
        if left_expr.node_type == ParseNodeType::Error {
            return left_expr;
        }

        if left_expr.node_type != ParseNodeType::Name {
            return left_expr;
        }

        let walrus_token = self._peek_token(0);
        if !self._consume_token_if_operator(OperatorType::Walrus) {
            return left_expr;
        }

        if !self._assignment_expressions_allowed {
            // Assignment expressions are disallowed anywhere within the iterable
            // expression of a comprehension's "for" clause, even if parenthesized.
            self._add_syntax_error(
                LocMessage::walrus_not_allowed_in_comprehension(),
                &walrus_token,
            );
        } else if disallow_assignment_expression {
            self._add_syntax_error(LocMessage::walrus_not_allowed(), &walrus_token);
        }

        if PythonVersion::is_less_than(&self._get_language_version(), &PYTHON_VERSION3_8) {
            self._add_syntax_error(LocMessage::walrus_illegal(), &walrus_token);
        }

        let right_expr = self._parse_test_expression(/* allow_assignment_expression */ false);

        AssignmentExpressionNode::create(
            self._get_owner_key(),
            NameNode(left_expr),
            walrus_token,
            right_expr,
        )
        .0
    }

    // or_test: and_test ('or' and_test)*
    // parser/parser.ts:3545
    fn _parse_or_test(&mut self) -> ExpressionNode {
        let mut left_expr = self._parse_and_test();
        if left_expr.node_type == ParseNodeType::Error {
            return left_expr;
        }

        loop {
            let peek_token = self._peek_token(0);
            if !self._consume_token_if_keyword(KeywordType::Or) {
                break;
            }
            let right_expr = self._parse_and_test();
            left_expr = self._create_binary_operation_node(
                left_expr,
                right_expr,
                peek_token,
                OperatorType::Or,
            );
        }

        left_expr
    }

    // and_test: not_test ('and' not_test)*
    // parser/parser.ts:3564
    fn _parse_and_test(&mut self) -> ExpressionNode {
        let mut left_expr = self._parse_not_test();
        if left_expr.node_type == ParseNodeType::Error {
            return left_expr;
        }

        loop {
            let peek_token = self._peek_token(0);
            if !self._consume_token_if_keyword(KeywordType::And) {
                break;
            }
            let right_expr = self._parse_not_test();
            left_expr = self._create_binary_operation_node(
                left_expr,
                right_expr,
                peek_token,
                OperatorType::And,
            );
        }

        left_expr
    }

    // not_test: 'not' not_test | comparison
    // parser/parser.ts:3583
    fn _parse_not_test(&mut self) -> ExpressionNode {
        let not_token = self._peek_token(0);
        if self._consume_token_if_keyword(KeywordType::Not) {
            let not_expr = self._parse_not_test();
            return self._create_unary_operation_node(not_token, not_expr, OperatorType::Not);
        }

        self._parse_comparison()
    }

    // comparison: expr (comp_op expr)*
    // comp_op: '<'|'>'|'=='|'>='|'<='|'<>'|'!='|'in'|'not' 'in'|'is'|'is' 'not'
    // parser/parser.ts:3595
    fn _parse_comparison(&mut self) -> ExpressionNode {
        let mut left_expr = self._parse_bitwise_or_expression();
        if left_expr.node_type == ParseNodeType::Error {
            return left_expr;
        }

        loop {
            let mut comparison_operator: Option<OperatorType> = None;
            let peek_token = self._peek_token(0);

            if Tokenizer::is_operator_comparison(self._peek_operator_type()) {
                comparison_operator = self._peek_operator_type();
                if comparison_operator == Some(OperatorType::LessOrGreaterThan) {
                    self._add_syntax_error(
                        LocMessage::operator_less_or_greater_deprecated(),
                        &peek_token,
                    );
                    comparison_operator = Some(OperatorType::NotEquals);
                }
                self._get_next_token();
            } else if self._consume_token_if_keyword(KeywordType::In) {
                comparison_operator = Some(OperatorType::In);
            } else if self._consume_token_if_keyword(KeywordType::Is) {
                if self._consume_token_if_keyword(KeywordType::Not) {
                    comparison_operator = Some(OperatorType::IsNot);
                } else {
                    comparison_operator = Some(OperatorType::Is);
                }
            } else if self._peek_keyword_type() == Some(KeywordType::Not) {
                let token_after_not = self._peek_token(1);
                if token_after_not.type_ == TokenType::Keyword
                    && keyword_type_of(token_after_not) == Some(KeywordType::In)
                {
                    self._get_next_token();
                    self._get_next_token();
                    comparison_operator = Some(OperatorType::NotIn);
                }
            }

            let Some(comparison_operator) = comparison_operator else {
                break;
            };

            let right_expr = self._parse_comparison();
            left_expr = self._create_binary_operation_node(
                left_expr,
                right_expr,
                peek_token,
                comparison_operator,
            );
        }

        left_expr
    }

    // expr: xor_expr ('|' xor_expr)*
    // parser/parser.ts:3644
    fn _parse_bitwise_or_expression(&mut self) -> ExpressionNode {
        let mut left_expr = self._parse_bitwise_xor_expression();
        if left_expr.node_type == ParseNodeType::Error {
            return left_expr;
        }

        loop {
            let peek_token = self._peek_token(0);
            if !self._consume_token_if_operator(OperatorType::BitwiseOr) {
                break;
            }
            let right_expr = self._parse_bitwise_xor_expression();
            left_expr = self._create_binary_operation_node(
                left_expr,
                right_expr,
                peek_token,
                OperatorType::BitwiseOr,
            );
        }

        left_expr
    }

    // xor_expr: and_expr ('^' and_expr)*
    // parser/parser.ts:3663
    fn _parse_bitwise_xor_expression(&mut self) -> ExpressionNode {
        let mut left_expr = self._parse_bitwise_and_expression();
        if left_expr.node_type == ParseNodeType::Error {
            return left_expr;
        }

        loop {
            let peek_token = self._peek_token(0);
            if !self._consume_token_if_operator(OperatorType::BitwiseXor) {
                break;
            }
            let right_expr = self._parse_bitwise_and_expression();
            left_expr = self._create_binary_operation_node(
                left_expr,
                right_expr,
                peek_token,
                OperatorType::BitwiseXor,
            );
        }

        left_expr
    }

    // and_expr: shift_expr ('&' shift_expr)*
    // parser/parser.ts:3682
    fn _parse_bitwise_and_expression(&mut self) -> ExpressionNode {
        let mut left_expr = self._parse_shift_expression();
        if left_expr.node_type == ParseNodeType::Error {
            return left_expr;
        }

        loop {
            let peek_token = self._peek_token(0);
            if !self._consume_token_if_operator(OperatorType::BitwiseAnd) {
                break;
            }
            let right_expr = self._parse_shift_expression();
            left_expr = self._create_binary_operation_node(
                left_expr,
                right_expr,
                peek_token,
                OperatorType::BitwiseAnd,
            );
        }

        left_expr
    }

    // shift_expr: arith_expr (('<<'|'>>') arith_expr)*
    // parser/parser.ts:3701
    fn _parse_shift_expression(&mut self) -> ExpressionNode {
        let mut left_expr = self._parse_arithmetic_expression();
        if left_expr.node_type == ParseNodeType::Error {
            return left_expr;
        }

        let mut peek_token = self._peek_token(0);
        let mut next_operator = self._peek_operator_type();
        while next_operator == Some(OperatorType::LeftShift)
            || next_operator == Some(OperatorType::RightShift)
        {
            self._get_next_token();
            let right_expr = self._parse_arithmetic_expression();
            left_expr = self._create_binary_operation_node(
                left_expr,
                right_expr,
                peek_token,
                next_operator.unwrap(),
            );
            peek_token = self._peek_token(0);
            next_operator = self._peek_operator_type();
        }

        left_expr
    }

    // arith_expr: term (('+'|'-') term)*
    // parser/parser.ts:3721
    fn _parse_arithmetic_expression(&mut self) -> ExpressionNode {
        let mut left_expr = self._parse_arithmetic_term();
        if left_expr.node_type == ParseNodeType::Error {
            return left_expr;
        }

        let mut peek_token = self._peek_token(0);
        let mut next_operator = self._peek_operator_type();
        while next_operator == Some(OperatorType::Add)
            || next_operator == Some(OperatorType::Subtract)
        {
            self._get_next_token();
            let right_expr = self._parse_arithmetic_term();
            if right_expr.node_type == ParseNodeType::Error {
                return right_expr;
            }

            left_expr = self._create_binary_operation_node(
                left_expr,
                right_expr,
                peek_token,
                next_operator.unwrap(),
            );
            peek_token = self._peek_token(0);
            next_operator = self._peek_operator_type();
        }

        left_expr
    }

    // term: factor (('*'|'@'|'/'|'%'|'//') factor)*
    // parser/parser.ts:3745
    fn _parse_arithmetic_term(&mut self) -> ExpressionNode {
        let mut left_expr = self._parse_arithmetic_factor();
        if left_expr.node_type == ParseNodeType::Error {
            return left_expr;
        }

        let mut peek_token = self._peek_token(0);
        let mut next_operator = self._peek_operator_type();
        while next_operator == Some(OperatorType::Multiply)
            || next_operator == Some(OperatorType::MatrixMultiply)
            || next_operator == Some(OperatorType::Divide)
            || next_operator == Some(OperatorType::Mod)
            || next_operator == Some(OperatorType::FloorDivide)
        {
            self._get_next_token();
            let right_expr = self._parse_arithmetic_factor();
            left_expr = self._create_binary_operation_node(
                left_expr,
                right_expr,
                peek_token,
                next_operator.unwrap(),
            );
            peek_token = self._peek_token(0);
            next_operator = self._peek_operator_type();
        }

        left_expr
    }

    // factor: ('+'|'-'|'~') factor | power
    // power: atom_expr ['**' factor]
    // parser/parser.ts:3772
    fn _parse_arithmetic_factor(&mut self) -> ExpressionNode {
        let next_token = self._peek_token(0);
        let next_operator = self._peek_operator_type();
        if next_operator == Some(OperatorType::Add)
            || next_operator == Some(OperatorType::Subtract)
            || next_operator == Some(OperatorType::BitwiseInvert)
        {
            self._get_next_token();
            let expression = self._parse_arithmetic_factor();
            return self._create_unary_operation_node(
                next_token,
                expression,
                next_operator.unwrap(),
            );
        }

        let left_expr = self._parse_atom_expression();
        if left_expr.node_type == ParseNodeType::Error {
            return left_expr;
        }

        let peek_token = self._peek_token(0);
        if self._consume_token_if_operator(OperatorType::Power) {
            let right_expr = self._parse_arithmetic_factor();
            return self._create_binary_operation_node(
                left_expr,
                right_expr,
                peek_token,
                OperatorType::Power,
            );
        }

        left_expr
    }

    // Determines whether the expression refers to a type exported by the typing
    // or typing_extensions modules.
    // parser/parser.ts:3803
    fn _is_typing_annotation(&self, type_annotation: ExpressionNode, name: &str) -> bool {
        if let Some(name_node) = NameNode::cast(type_annotation) {
            let alias = self._typing_symbol_aliases.get(&name_node.d().value);
            if alias.map(String::as_str) == Some(name) {
                return true;
            }
        } else if let Some(member_access) = MemberAccessNode::cast(type_annotation) {
            if let Some(left_name) = NameNode::cast(member_access.d().left_expr.get())
                && member_access.d().member.get().d().value == name
            {
                let base_name = &left_name.d().value;
                return self
                    ._typing_import_aliases
                    .iter()
                    .any(|alias| alias == base_name);
            }
        }

        false
    }
}

impl Parser {
    // atom_expr: ['await'] atom trailer*
    // trailer: '(' [arglist] ')' | '[' subscriptlist ']' | '.' NAME
    // parser/parser.ts:3821
    fn _parse_atom_expression(&mut self) -> ExpressionNode {
        let mut await_token: Option<KeywordToken> = None;
        if self._peek_keyword_type() == Some(KeywordType::Await) {
            let token = self._get_keyword_token(KeywordType::Await);
            await_token = Some(token);
            if PythonVersion::is_less_than(&self._get_language_version(), &PYTHON_VERSION3_5) {
                self._add_syntax_error(LocMessage::await_illegal(), &token);
            }
        }

        let mut atom_expression = self._parse_atom();
        if atom_expression.node_type == ParseNodeType::Error {
            return atom_expression;
        }

        // Consume trailers.
        loop {
            // Is it a function call?
            let start_of_trailer_token = self._peek_token(0);
            if self._consume_token_if_type(TokenType::OpenParenthesis) {
                // Generally, function calls are not allowed within type annotations,
                // but they are permitted in "Annotated" annotations.
                let was_parsing_type_annotation = self._is_parsing_type_annotation;
                self._is_parsing_type_annotation = false;

                let arg_list_result = self._parse_arg_list();
                let call_node = CallNode::create(
                    self._get_owner_key(),
                    atom_expression,
                    arg_list_result.args.clone(),
                    arg_list_result.trailing_comma,
                );

                if arg_list_result.args.len() > 1 || arg_list_result.trailing_comma {
                    for arg in &arg_list_result.args {
                        let value_expr = arg.d().value_expr.get();
                        if let Some(comprehension) = ComprehensionNode::cast(value_expr) {
                            if !comprehension.d().has_parens.get() {
                                self._add_syntax_error(
                                    LocMessage::generator_not_parenthesized(),
                                    &value_expr,
                                );
                            }
                        }
                    }
                }

                let next_token = self._peek_token(0);
                let mut is_arg_list_terminated = false;
                if !self._consume_token_if_type(TokenType::CloseParenthesis) {
                    self._add_syntax_error(
                        LocMessage::expected_close_paren(),
                        &start_of_trailer_token,
                    );

                    // Consume the remainder of tokens on the line for error
                    // recovery.
                    self._consume_tokens_until_type(&[TokenType::NewLine]);

                    // Extend the node's range to include the rest of the line.
                    // This helps the signatureHelpProvider.
                    let token = self._peek_token(0);
                    extend_range(&call_node, &token);
                } else {
                    extend_range(&call_node, &next_token);
                    is_arg_list_terminated = true;
                }

                self._is_parsing_type_annotation = was_parsing_type_annotation;

                let max_depth = self
                    ._max_child_depth_map
                    .get(&atom_expression.id.get())
                    .copied()
                    .unwrap_or(0);
                if max_depth >= MAX_CHILD_NODE_DEPTH {
                    atom_expression = ErrorNode::create(
                        self._get_owner_key(),
                        &call_node,
                        ErrorExpressionCategory::MaxDepthExceeded,
                        None,
                        None,
                    )
                    .0;
                    self._add_syntax_error(
                        LocMessage::max_parse_depth_exceeded(),
                        &atom_expression,
                    );
                } else {
                    atom_expression = call_node.0;
                    self._max_child_depth_map
                        .insert(call_node.id.get(), max_depth + 1);
                }

                // If the argument list wasn't terminated, break out of the loop
                if !is_arg_list_terminated {
                    break;
                }
            } else if self._consume_token_if_type(TokenType::OpenBracket) {
                // Is it an index operator?

                // This is an unfortunate hack that's necessary to accommodate 'Literal'
                // and 'Annotated' type annotations properly. We need to suspend treating
                // strings as type annotations within a Literal or Annotated subscript.
                let was_parsing_index_trailer = self._is_parsing_index_trailer;
                let was_parsing_type_annotation = self._is_parsing_type_annotation;

                if self._is_typing_annotation(atom_expression, "Literal")
                    || self._is_typing_annotation(atom_expression, "Annotated")
                {
                    self._is_parsing_type_annotation = false;
                }

                self._is_parsing_index_trailer = true;
                let subscript_list = self._parse_subscript_list();
                self._is_parsing_type_annotation = was_parsing_type_annotation;
                self._is_parsing_index_trailer = was_parsing_index_trailer;

                let closing_token = self._peek_token(0);

                let index_node = IndexNode::create(
                    self._get_owner_key(),
                    atom_expression,
                    subscript_list.list,
                    subscript_list.trailing_comma,
                    closing_token,
                );
                extend_range(&index_node, &index_node);

                if !self._consume_token_if_type(TokenType::CloseBracket) {
                    // Handle the error case, but don't use the error node in this
                    // case because it creates problems for the completion provider.
                    self._handle_expression_parse_error(
                        ErrorExpressionCategory::MissingIndexCloseBracket,
                        LocMessage::expected_close_bracket(),
                        Some(start_of_trailer_token),
                        Some(index_node.0),
                        None,
                    );
                }

                let max_depth = self
                    ._max_child_depth_map
                    .get(&atom_expression.id.get())
                    .copied()
                    .unwrap_or(0);
                if max_depth >= MAX_CHILD_NODE_DEPTH {
                    atom_expression = ErrorNode::create(
                        self._get_owner_key(),
                        &index_node,
                        ErrorExpressionCategory::MaxDepthExceeded,
                        None,
                        None,
                    )
                    .0;
                    self._add_syntax_error(
                        LocMessage::max_parse_depth_exceeded(),
                        &atom_expression,
                    );
                } else {
                    atom_expression = index_node.0;
                    self._max_child_depth_map
                        .insert(index_node.id.get(), max_depth + 1);
                }
            } else if self._consume_token_if_type(TokenType::Dot) {
                // Is it a member access?
                let Some(member_name) = self._get_token_if_identifier() else {
                    return self
                        ._handle_expression_parse_error(
                            ErrorExpressionCategory::MissingMemberAccessName,
                            LocMessage::expected_member_name(),
                            Some(start_of_trailer_token),
                            Some(atom_expression),
                            Some(&[TokenType::Keyword]),
                        )
                        .0;
                };

                let member_access_node = MemberAccessNode::create(
                    self._get_owner_key(),
                    atom_expression,
                    NameNode::create(self._get_owner_key(), member_name),
                );

                let max_depth = self
                    ._max_child_depth_map
                    .get(&atom_expression.id.get())
                    .copied()
                    .unwrap_or(0);
                if max_depth >= MAX_CHILD_NODE_DEPTH {
                    atom_expression = ErrorNode::create(
                        self._get_owner_key(),
                        &member_access_node,
                        ErrorExpressionCategory::MaxDepthExceeded,
                        None,
                        None,
                    )
                    .0;
                    self._add_syntax_error(
                        LocMessage::max_parse_depth_exceeded(),
                        &atom_expression,
                    );
                } else {
                    atom_expression = member_access_node.0;
                    self._max_child_depth_map
                        .insert(member_access_node.id.get(), max_depth + 1);
                }
            } else {
                break;
            }
        }

        if let Some(await_token) = await_token {
            return AwaitNode::create(self._get_owner_key(), await_token.0, atom_expression).0;
        }

        atom_expression
    }

    // subscriptlist: subscript (',' subscript)* [',']
    // parser/parser.ts:3998
    fn _parse_subscript_list(&mut self) -> SubscriptListResult {
        let mut arg_list: Vec<ArgumentNode> = vec![];
        let mut saw_keyword_arg = false;
        let mut trailing_comma = false;

        loop {
            let first_token = self._peek_token(0);

            if first_token.type_ != TokenType::Colon && self._is_next_token_never_expression() {
                break;
            }

            let mut arg_type = ArgCategory::Simple;
            if self._consume_token_if_operator(OperatorType::Multiply) {
                arg_type = ArgCategory::UnpackedList;
            } else if self._consume_token_if_operator(OperatorType::Power) {
                arg_type = ArgCategory::UnpackedDictionary;
            }

            let start_of_subscript_index = self._token_index;
            let mut value_expr = self._parse_possible_slice();
            let mut name_identifier: Option<IdentifierToken> = None;

            // Is this a keyword argument?
            if arg_type == ArgCategory::Simple {
                if self._consume_token_if_operator(OperatorType::Assign) {
                    let name_expr = value_expr;
                    value_expr = self._parse_possible_slice();

                    if let Some(name_node) = NameNode::cast(name_expr) {
                        name_identifier = Some(name_node.d().token.get());
                    } else {
                        self._add_syntax_error(LocMessage::expected_param_name(), &name_expr);
                    }
                } else if value_expr.node_type == ParseNodeType::Name
                    && self._peek_operator_type() == Some(OperatorType::Walrus)
                {
                    self._token_index = start_of_subscript_index;
                    value_expr =
                        self._parse_test_expression(/* allow_assignment_expression */ true);

                    // Python 3.10 and newer allow assignment expressions to be used inside of a subscript.
                    if !self._parse_options.is_stub_file
                        && PythonVersion::is_less_than(
                            &self._get_language_version(),
                            &PYTHON_VERSION3_10,
                        )
                    {
                        self._add_syntax_error(
                            LocMessage::assignment_expr_in_subscript(),
                            &value_expr,
                        );
                    }
                }
            }

            let arg_node = ArgumentNode::create(
                self._get_owner_key(),
                Some(first_token),
                value_expr,
                arg_type,
            );
            if let Some(name_identifier) = name_identifier {
                let name = NameNode::create(self._get_owner_key(), name_identifier);
                arg_node.d().name.set(Some(name));
                name.parent.set(Some(arg_node.0));
            }

            if arg_node.d().name.get().is_some() {
                saw_keyword_arg = true;
            } else if saw_keyword_arg && arg_node.d().arg_category.get() == ArgCategory::Simple {
                self._add_syntax_error(LocMessage::position_arg_after_named_arg(), &arg_node);
            }
            arg_list.push(arg_node);

            if let Some(name) = arg_node.d().name.get() {
                self._add_syntax_error(LocMessage::keyword_subscript_illegal(), &name);
            }

            if arg_type != ArgCategory::Simple {
                let unpack_list_allowed = self._parse_options.is_stub_file
                    || self._is_parsing_quoted_text
                    || PythonVersion::is_greater_or_equal_to(
                        &self._get_language_version(),
                        &PYTHON_VERSION3_11,
                    );

                if arg_type == ArgCategory::UnpackedList && !unpack_list_allowed {
                    self._add_syntax_error(LocMessage::unpacked_subscript_illegal(), &arg_node);
                }

                if arg_type == ArgCategory::UnpackedDictionary {
                    self._add_syntax_error(
                        LocMessage::unpacked_dict_subscript_illegal(),
                        &arg_node,
                    );
                }
            }

            if !self._consume_token_if_type(TokenType::Comma) {
                trailing_comma = false;
                break;
            }

            trailing_comma = true;
        }

        // An empty subscript list is illegal.
        if arg_list.is_empty() {
            let error_node = self._handle_expression_parse_error(
                ErrorExpressionCategory::MissingIndexOrSlice,
                LocMessage::expected_slice_index(),
                /* target_token */ None,
                /* child_node */ None,
                Some(&[TokenType::CloseBracket]),
            );
            let token = self._peek_token(0);
            arg_list.push(ArgumentNode::create(
                self._get_owner_key(),
                Some(token),
                error_node.0,
                ArgCategory::Simple,
            ));
        }

        SubscriptListResult {
            list: arg_list,
            trailing_comma,
        }
    }

    // subscript: test | [test] ':' [test] [sliceop]
    // sliceop: ':' [test]
    // parser/parser.ts:4109
    fn _parse_possible_slice(&mut self) -> ExpressionNode {
        let first_token = self._peek_token(0);
        let mut slice_expressions: [Option<ExpressionNode>; 3] = [None, None, None];
        let mut slice_index = 0;
        let mut saw_colon = false;

        loop {
            let next_token_type = self._peek_token_type();
            if next_token_type == TokenType::CloseBracket || next_token_type == TokenType::Comma {
                break;
            }

            if next_token_type != TokenType::Colon {
                // Python 3.10 and newer allow assignment expressions to be used inside of a subscript.
                let allow_assignment_expression = self._parse_options.is_stub_file
                    || PythonVersion::is_greater_or_equal_to(
                        &self._get_language_version(),
                        &PYTHON_VERSION3_10,
                    );
                slice_expressions[slice_index] =
                    Some(self._parse_test_expression(allow_assignment_expression));
            }
            slice_index += 1;

            if slice_index >= 3 || !self._consume_token_if_type(TokenType::Colon) {
                break;
            }
            saw_colon = true;
        }

        // If this was a simple expression with no colons return it.
        if !saw_colon {
            if let Some(expr) = slice_expressions[0] {
                return expr;
            }

            let token = self._peek_token(0);
            return ErrorNode::create(
                self._get_owner_key(),
                &token,
                ErrorExpressionCategory::MissingIndexOrSlice,
                None,
                None,
            )
            .0;
        }

        let slice_node = SliceNode::create(self._get_owner_key(), &first_token);
        slice_node.d().start_value.set(slice_expressions[0]);
        if let Some(start_value) = slice_expressions[0] {
            start_value.parent.set(Some(slice_node.0));
        }
        slice_node.d().end_value.set(slice_expressions[1]);
        if let Some(end_value) = slice_expressions[1] {
            end_value.parent.set(Some(slice_node.0));
        }
        slice_node.d().step_value.set(slice_expressions[2]);
        if let Some(step_value) = slice_expressions[2] {
            step_value.parent.set(Some(slice_node.0));
        }
        let extension = slice_expressions[2]
            .or(slice_expressions[1])
            .or(slice_expressions[0]);
        if let Some(extension) = extension {
            extend_range(&slice_node, &extension);
        }

        slice_node.0
    }

    // arglist: argument (',' argument)*  [',']
    // parser/parser.ts:4171
    fn _parse_arg_list(&mut self) -> ArgListResult {
        let mut arg_list: Vec<ArgumentNode> = vec![];
        let mut saw_keyword_arg = false;
        let mut saw_unpacked_keyword_arg = false;
        let mut trailing_comma = false;

        loop {
            let next_token_type = self._peek_token_type();
            if next_token_type == TokenType::CloseParenthesis
                || next_token_type == TokenType::NewLine
                || next_token_type == TokenType::EndOfStream
            {
                break;
            }

            trailing_comma = false;
            let arg = self._parse_argument();
            if arg.d().name.get().is_some() {
                saw_keyword_arg = true;
            } else {
                if saw_keyword_arg && arg.d().arg_category.get() == ArgCategory::Simple {
                    self._add_syntax_error(LocMessage::position_arg_after_named_arg(), &arg);
                }

                if saw_unpacked_keyword_arg
                    && arg.d().arg_category.get() != ArgCategory::UnpackedDictionary
                {
                    self._add_syntax_error(
                        LocMessage::position_arg_after_unpacked_dict_arg(),
                        &arg,
                    );
                }
            }
            if arg.d().arg_category.get() == ArgCategory::UnpackedDictionary {
                saw_unpacked_keyword_arg = true;
            }
            arg_list.push(arg);

            if !self._consume_token_if_type(TokenType::Comma) {
                break;
            }

            trailing_comma = true;
        }

        ArgListResult {
            args: arg_list,
            trailing_comma,
        }
    }

    // argument: ( test [comp_for] |
    //             test '=' test |
    //             '**' test |
    //             '*' test )
    // parser/parser.ts:4219
    fn _parse_argument(&mut self) -> ArgumentNode {
        let first_token = self._peek_token(0);

        let mut arg_type = ArgCategory::Simple;
        if self._consume_token_if_operator(OperatorType::Multiply) {
            arg_type = ArgCategory::UnpackedList;
        } else if self._consume_token_if_operator(OperatorType::Power) {
            arg_type = ArgCategory::UnpackedDictionary;
        }

        let mut value_expr =
            self._parse_test_expression(/* allow_assignment_expression */ true);
        let mut name_identifier: Option<IdentifierToken> = None;

        if arg_type == ArgCategory::Simple {
            if self._consume_token_if_operator(OperatorType::Assign) {
                let name_expr = value_expr;
                value_expr =
                    self._parse_test_expression(/* allow_assignment_expression */ false);

                if let Some(name_node) = NameNode::cast(name_expr) {
                    name_identifier = Some(name_node.d().token.get());
                } else {
                    self._add_syntax_error(LocMessage::expected_param_name(), &name_expr);
                }
            } else if let Some(comprehension) =
                self._try_parse_comprehension(value_expr, /* is_generator */ true)
            {
                value_expr = comprehension.0;
            }
        }

        let arg_node = ArgumentNode::create(
            self._get_owner_key(),
            Some(first_token),
            value_expr,
            arg_type,
        );
        if let Some(name_identifier) = name_identifier {
            let name = NameNode::create(self._get_owner_key(), name_identifier);
            arg_node.d().name.set(Some(name));
            name.parent.set(Some(arg_node.0));
        }

        arg_node
    }
}

impl Parser {
    // atom: ('(' [yield_expr | testlist_comp] ')' |
    //     '[' [testlist_comp] ']' |
    //     '{' [dictorsetmaker] '}' |
    //     NAME | NUMBER | STRING+ | '...' | 'None' | 'True' | 'False' | '__debug__')
    // parser/parser.ts:4263
    fn _parse_atom(&mut self) -> ExpressionNode {
        let next_token = self._peek_token(0);

        if next_token.type_ == TokenType::Ellipsis {
            let token = self._get_next_token();
            return EllipsisNode::create(self._get_owner_key(), &token).0;
        }

        if next_token.type_ == TokenType::Number {
            let token = NumberToken::cast(self._get_next_token()).unwrap();
            return NumberNode::create(self._get_owner_key(), token).0;
        }

        if next_token.type_ == TokenType::Identifier {
            let token = IdentifierToken::cast(self._get_next_token()).unwrap();
            return NameNode::create(self._get_owner_key(), token).0;
        }

        if next_token.type_ == TokenType::String || next_token.type_ == TokenType::FStringStart {
            return self._parse_string_list().0;
        }

        if next_token.type_ == TokenType::Backtick {
            self._get_next_token();

            // Atoms with backticks are no longer allowed in Python 3.x, but they
            // were a thing in Python 2.x. We'll parse them to improve parse recovery
            // and emit an error.
            self._add_syntax_error(LocMessage::backticks_illegal(), &next_token);

            let expression_node = self._parse_test_list_as_expression(
                ErrorExpressionCategory::MissingExpression,
                LocMessage::expected_expr,
            );

            self._consume_token_if_type(TokenType::Backtick);
            return expression_node;
        }

        if next_token.type_ == TokenType::OpenParenthesis {
            let possible_tuple_node = self._parse_tuple_atom();

            match possible_tuple_node.d() {
                // Mark binary expressions as parenthesized so we don't attempt
                // to use comparison chaining, which isn't appropriate when the
                // expression is parenthesized. Unary and await expressions
                // are also marked to be able to display them unambiguously.
                ParseNodeDetails::UnaryOperation(d) => d.has_parens.set(true),
                ParseNodeDetails::Await(d) => d.has_parens.set(true),
                ParseNodeDetails::BinaryOperation(d) => d.has_parens.set(true),
                _ => {}
            }

            match possible_tuple_node.d() {
                ParseNodeDetails::StringList(d) => d.has_parens.set(true),
                ParseNodeDetails::Comprehension(d) => d.has_parens.set(true),
                ParseNodeDetails::AssignmentExpression(d) => d.has_parens.set(true),
                _ => {}
            }

            return possible_tuple_node;
        } else if next_token.type_ == TokenType::OpenBracket {
            return self._parse_list_atom();
        } else if next_token.type_ == TokenType::OpenCurlyBrace {
            return self._parse_dictionary_or_set_atom();
        }

        if next_token.type_ == TokenType::Keyword {
            let keyword_type = keyword_type_of(next_token);
            if keyword_type == Some(KeywordType::False)
                || keyword_type == Some(KeywordType::True)
                || keyword_type == Some(KeywordType::Debug)
                || keyword_type == Some(KeywordType::None)
            {
                let token = KeywordToken::cast(self._get_next_token()).unwrap();
                return ConstantNode::create(self._get_owner_key(), token).0;
            }

            // Make an identifier out of the keyword.
            if let Some(keyword_as_identifier) = self._get_token_if_identifier() {
                return NameNode::create(self._get_owner_key(), keyword_as_identifier).0;
            }
        }

        self._handle_expression_parse_error(
            ErrorExpressionCategory::MissingExpression,
            LocMessage::expected_expr(),
            None,
            None,
            None,
        )
        .0
    }

    // Allocates a dummy "error expression" and consumes the remainder
    // of the tokens on the line for error recovery. A partially-completed
    // child node can be passed to help the completion provider determine
    // what to do.
    // parser/parser.ts:4353
    fn _handle_expression_parse_error(
        &mut self,
        category: ErrorExpressionCategory,
        error_msg: String,
        target_token: Option<P<Token>>,
        child_node: Option<ExpressionNode>,
        additional_stop_tokens: Option<&[TokenType]>,
    ) -> ErrorNode {
        let error_range = match target_token {
            Some(target_token) => target_token.text_range(),
            None => self._peek_token(0).text_range(),
        };
        self._add_syntax_error(error_msg, &error_range);

        let mut stop_tokens = vec![TokenType::NewLine];
        if let Some(additional_stop_tokens) = additional_stop_tokens {
            append_array(&mut stop_tokens, additional_stop_tokens);
        }

        // Using a token that is not included in the error node creates problems.
        // Sibling nodes in parse tree shouldn't overlap each other.
        let next_token = self._peek_token(0);
        let initial_range: TextRange = if stop_tokens.iter().any(|&k| next_token.type_ == k) {
            match (target_token, child_node) {
                (Some(target_token), _) => target_token.text_range(),
                (None, Some(child_node)) => child_node.text_range(),
                (None, None) => TextRange::create(next_token.start, /* length */ 0),
            }
        } else {
            next_token.text_range()
        };
        let expr = ErrorNode::create(
            self._get_owner_key(),
            &initial_range,
            category,
            child_node,
            None,
        );
        self._consume_tokens_until_type(&stop_tokens);

        expr
    }

    // lambdef: 'lambda' [varargslist] ':' test
    // parser/parser.ts:4380
    fn _parse_lambda_expression(&mut self, allow_conditional: bool) -> LambdaNode {
        let lambda_token = self._get_keyword_token(KeywordType::Lambda);

        let arg_list =
            self._parse_var_args_list(TokenType::Colon, /* allow_annotations */ false);

        if !self._consume_token_if_type(TokenType::Colon) {
            let token = self._peek_token(0);
            self._add_syntax_error(LocMessage::expected_colon(), &token);
        }

        let test_expr: ExpressionNode = if allow_conditional {
            self._parse_test_expression(/* allow_assignment_expression */ false)
        } else {
            match self._try_parse_lambda_expression(/* allow_conditional */ false) {
                Some(lambda) => lambda.0,
                None => self._parse_or_test(),
            }
        };

        let lambda_node = LambdaNode::create(self._get_owner_key(), lambda_token.0, test_expr);
        *lambda_node.d().params.borrow_mut() = arg_list.clone();
        for arg in &arg_list {
            arg.parent.set(Some(lambda_node.0));
        }
        lambda_node
    }

    // parser/parser.ts:4404
    fn _try_parse_lambda_expression(&mut self, allow_conditional: bool) -> Option<LambdaNode> {
        if self._peek_keyword_type() != Some(KeywordType::Lambda) {
            return None;
        }

        Some(self._parse_lambda_expression(allow_conditional))
    }

    // ('(' [yield_expr | testlist_comp] ')'
    // testlist_comp: (test | star_expr) (comp_for | (',' (test | star_expr))* [','])
    // parser/parser.ts:4414
    fn _parse_tuple_atom(&mut self) -> ExpressionNode {
        let start_paren = self._get_next_token();
        assert!(start_paren.type_ == TokenType::OpenParenthesis);

        if let Some(yield_expr) = self._try_parse_yield_expression() {
            if self._peek_token_type() != TokenType::CloseParenthesis {
                return self
                    ._handle_expression_parse_error(
                        ErrorExpressionCategory::MissingTupleCloseParen,
                        LocMessage::expected_close_paren(),
                        Some(start_paren),
                        Some(yield_expr),
                        None,
                    )
                    .0;
            } else {
                let token = self._get_next_token();
                extend_range(&yield_expr, &token);
            }

            return yield_expr;
        }

        let expr_list_result =
            self._parse_test_list_with_comprehension(/* is_generator */ true);
        let parse_error = expr_list_result.parse_error;
        let tuple_or_expression =
            self._make_expression_or_tuple(expr_list_result, /* enclosed_in_parens */ true);

        extend_range(&tuple_or_expression, &start_paren);

        if self._peek_token_type() != TokenType::CloseParenthesis {
            return self
                ._handle_expression_parse_error(
                    ErrorExpressionCategory::MissingTupleCloseParen,
                    LocMessage::expected_close_paren(),
                    Some(start_paren),
                    Some(parse_error.map_or(tuple_or_expression, |e| e.0)),
                    None,
                )
                .0;
        } else {
            let token = self._get_next_token();
            extend_range(&tuple_or_expression, &token);
        }

        tuple_or_expression
    }

    // '[' [testlist_comp] ']'
    // testlist_comp: (test | star_expr) (comp_for | (',' (test | star_expr))* [','])
    // parser/parser.ts:4455
    fn _parse_list_atom(&mut self) -> P<ParseNode> {
        let start_bracket = self._get_next_token();
        assert!(start_bracket.type_ == TokenType::OpenBracket);
        let owner_key = self._get_owner_key();

        let expr_list_result =
            self._parse_test_list_with_comprehension(/* is_generator */ false);
        let close_bracket: Option<P<Token>> = Some(self._peek_token(0));

        let create_list = || {
            let list_atom = ListNode::create(owner_key, &start_bracket);

            if let Some(close_bracket) = close_bracket {
                extend_range(&list_atom, &close_bracket);
            }

            if !expr_list_result.list.is_empty() {
                for expr in &expr_list_result.list {
                    expr.parent.set(Some(list_atom.0));
                }
                extend_range(
                    &list_atom,
                    &expr_list_result.list[expr_list_result.list.len() - 1],
                );
            }

            *list_atom.d().items.borrow_mut() = expr_list_result.list.clone();
            list_atom
        };

        if !self._consume_token_if_type(TokenType::CloseBracket) {
            let child = match expr_list_result.parse_error {
                Some(parse_error) => parse_error.0,
                None => create_list().0,
            };
            return self
                ._handle_expression_parse_error(
                    ErrorExpressionCategory::MissingListCloseBracket,
                    LocMessage::expected_close_bracket(),
                    Some(start_bracket),
                    Some(child),
                    None,
                )
                .0;
        }

        create_list().0
    }

    // parser/parser.ts:4492
    fn _parse_test_list_with_comprehension(&mut self, is_generator: bool) -> ListResult {
        let saw_comprehension = jsrt::cell::Cell::new(false);

        self._parse_expression_list_generic(
            &mut |this| {
                let mut expr = this
                    ._parse_test_or_star_expression(/* allow_assignment_expression */ true);
                if let Some(comprehension) = this._try_parse_comprehension(expr, is_generator) {
                    expr = comprehension.0;
                    saw_comprehension.set(true);
                }
                expr
            },
            Some(&mut |this| this._is_next_token_never_expression()),
            Some(&mut |_this| saw_comprehension.get()),
        )
    }

    // '{' [dictorsetmaker] '}'
    // dictorsetmaker: (
    //    (dictentry (comp_for | (',' dictentry)* [',']))
    //    | (setentry (comp_for | (',' setentry)* [',']))
    // )
    // dictentry: (test ':' test | '**' expr)
    // setentry: test | star_expr
    // parser/parser.ts:4517
    fn _parse_dictionary_or_set_atom(&mut self) -> P<ParseNode> {
        let start_brace = self._get_next_token();
        assert!(start_brace.type_ == TokenType::OpenCurlyBrace);

        let mut dictionary_entries: Vec<DictionaryEntryNode> = vec![];
        let mut set_entries: Vec<ExpressionNode> = vec![];
        let mut is_dictionary = false;
        let mut is_set = false;
        let mut saw_comprehension = false;
        let mut is_first_entry = true;
        let mut trailing_comma_token: Option<P<Token>> = None;

        loop {
            if self._peek_token_type() == TokenType::CloseCurlyBrace {
                break;
            }

            trailing_comma_token = None;

            let mut double_star_expression: Option<ExpressionNode> = None;
            let mut key_expression: Option<ExpressionNode> = None;
            let mut value_expression: Option<ExpressionNode> = None;
            let double_star = self._peek_token(0);

            if self._consume_token_if_operator(OperatorType::Power) {
                double_star_expression = Some(self._parse_expression(/* allow_unpack */ false));
            } else {
                // A dictionary key is never a forward-reference type annotation, even
                // when the dictionary appears within a type annotation. Suspend
                // type-annotation parsing for the key so its string isn't parsed into a
                // forward-reference expression.
                let was_parsing_type_annotation = self._is_parsing_type_annotation;
                self._is_parsing_type_annotation = false;
                let key = self
                    ._parse_test_or_star_expression(/* allow_assignment_expression */ true);
                key_expression = Some(key);
                self._is_parsing_type_annotation = was_parsing_type_annotation;

                // Allow walrus operators in this context only for Python 3.10 and newer.
                // Older versions of Python generated a syntax error in this context.
                let mut is_walrus_allowed = PythonVersion::is_greater_or_equal_to(
                    &self._get_language_version(),
                    &PYTHON_VERSION3_10,
                );

                if self._consume_token_if_type(TokenType::Colon) {
                    value_expression = Some(
                        self._parse_test_expression(/* allow_assignment_expression */ false),
                    );
                    is_walrus_allowed = false;
                }

                if !is_walrus_allowed
                    && let Some(assignment) = AssignmentExpressionNode::cast(key)
                    && !assignment.d().has_parens.get()
                {
                    self._add_syntax_error(
                        LocMessage::walrus_not_allowed(),
                        &assignment.d().walrus_token.get(),
                    );
                }
            }

            if let (Some(key), Some(value)) = (key_expression, value_expression) {
                if key.node_type == ParseNodeType::Unpack {
                    self._add_syntax_error(LocMessage::unpack_in_dict(), &key);
                }

                if is_set {
                    self._add_syntax_error(LocMessage::key_value_in_set(), &value);
                } else {
                    let key_entry_node =
                        DictionaryKeyEntryNode::create(self._get_owner_key(), key, value);
                    let mut dict_entry: DictionaryEntryNode = key_entry_node.0;
                    if let Some(comprehension) = self
                        ._try_parse_comprehension(key_entry_node.0, /* is_generator */ false)
                    {
                        dict_entry = comprehension.0;
                        saw_comprehension = true;

                        if !is_first_entry {
                            self._add_syntax_error(
                                LocMessage::comprehension_in_dict(),
                                &dict_entry,
                            );
                        }
                    }
                    dictionary_entries.push(dict_entry);
                    is_dictionary = true;
                }
            } else if let Some(double_star_expression) = double_star_expression {
                if is_set {
                    self._add_syntax_error(LocMessage::unpack_in_set(), &double_star_expression);
                } else {
                    let list_entry_node = DictionaryExpandEntryNode::create(
                        self._get_owner_key(),
                        double_star_expression,
                    );
                    extend_range(&list_entry_node, &double_star);
                    let mut expand_entry_node: DictionaryEntryNode = list_entry_node.0;
                    if let Some(comprehension) = self
                        ._try_parse_comprehension(list_entry_node.0, /* is_generator */ false)
                    {
                        expand_entry_node = comprehension.0;
                        saw_comprehension = true;

                        if !is_first_entry {
                            self._add_syntax_error(
                                LocMessage::comprehension_in_dict(),
                                &double_star_expression,
                            );
                        }
                    }
                    dictionary_entries.push(expand_entry_node);
                    is_dictionary = true;
                }
            } else {
                assert!(key_expression.is_some());
                if let Some(mut key) = key_expression {
                    if is_dictionary {
                        let token = self._peek_token(0);
                        let missing_value_error_node = ErrorNode::create(
                            self._get_owner_key(),
                            &token,
                            ErrorExpressionCategory::MissingDictValue,
                            None,
                            None,
                        );
                        let key_entry_node = DictionaryKeyEntryNode::create(
                            self._get_owner_key(),
                            key,
                            missing_value_error_node.0,
                        );
                        dictionary_entries.push(key_entry_node.0);
                        self._add_syntax_error(LocMessage::dict_key_value_pairs(), &key);
                    } else {
                        if let Some(comprehension) =
                            self._try_parse_comprehension(key, /* is_generator */ false)
                        {
                            key = comprehension.0;
                            saw_comprehension = true;

                            if !is_first_entry {
                                self._add_syntax_error(LocMessage::comprehension_in_set(), &key);
                            }
                        }
                        set_entries.push(key);
                        is_set = true;
                    }
                }
            }

            // List comprehension statements always end the list.
            if saw_comprehension {
                break;
            }

            if self._peek_token_type() != TokenType::Comma {
                break;
            }

            trailing_comma_token = Some(self._get_next_token());

            is_first_entry = false;
        }

        let mut close_curly_brace: Option<P<Token>> = Some(self._peek_token(0));
        if !self._consume_token_if_type(TokenType::CloseCurlyBrace) {
            self._add_syntax_error(LocMessage::expected_close_brace(), &start_brace);
            close_curly_brace = None;
        }

        if is_set {
            let set_atom = SetNode::create(self._get_owner_key(), &start_brace);
            if let Some(close_curly_brace) = close_curly_brace {
                extend_range(&set_atom, &close_curly_brace);
            }

            if !set_entries.is_empty() {
                extend_range(&set_atom, &set_entries[set_entries.len() - 1]);
            }

            for entry in &set_entries {
                entry.parent.set(Some(set_atom.0));
            }

            *set_atom.d().items.borrow_mut() = set_entries;
            return set_atom.0;
        }

        let dictionary_atom = DictionaryNode::create(self._get_owner_key(), &start_brace);

        if let Some(trailing_comma_token) = trailing_comma_token {
            dictionary_atom
                .d()
                .trailing_comma_token
                .set(Some(trailing_comma_token));
            extend_range(&dictionary_atom, &trailing_comma_token);
        }

        if let Some(close_curly_brace) = close_curly_brace {
            extend_range(&dictionary_atom, &close_curly_brace);
        }

        if !dictionary_entries.is_empty() {
            for entry in &dictionary_entries {
                entry.parent.set(Some(dictionary_atom.0));
            }
            extend_range(
                &dictionary_atom,
                &dictionary_entries[dictionary_entries.len() - 1],
            );
        }
        *dictionary_atom.d().items.borrow_mut() = dictionary_entries;
        dictionary_atom.0
    }

    /// `None` checks are the TypeScript defaults: `_isNextTokenNeverExpression()`
    /// and `() => false`.
    // parser/parser.ts:4709
    fn _parse_expression_list_generic(
        &mut self,
        parser: &mut dyn FnMut(&mut Parser) -> P<ParseNode>,
        terminal_check: Option<&mut dyn FnMut(&mut Parser) -> bool>,
        final_entry_check: Option<&mut dyn FnMut(&mut Parser) -> bool>,
    ) -> ListResult {
        let mut terminal_check = terminal_check;
        let mut final_entry_check = final_entry_check;
        let mut trailing_comma = false;
        let mut list: Vec<P<ParseNode>> = vec![];
        let mut parse_error: Option<ErrorNode> = None;

        loop {
            let is_terminal = match &mut terminal_check {
                Some(check) => check(self),
                None => self._is_next_token_never_expression(),
            };
            if is_terminal {
                break;
            }

            let expr = parser(self);
            if let Some(error) = ErrorNode::cast(expr) {
                parse_error = Some(error);
                break;
            }
            list.push(expr);

            // Should we stop without checking for a trailing comma?
            let is_final = match &mut final_entry_check {
                Some(check) => check(self),
                None => false,
            };
            if is_final {
                break;
            }

            if !self._consume_token_if_type(TokenType::Comma) {
                trailing_comma = false;
                break;
            }

            trailing_comma = true;
        }

        ListResult {
            trailing_comma,
            list,
            parse_error,
        }
    }
}

impl Parser {
    // parser/parser.ts:4752
    fn _parse_expression_statement(&mut self) -> ExpressionNode {
        let mut left_expr = self._parse_test_or_star_list_as_expression(
            /* allow_assignment_expression */ false,
            /* allow_multiple_unpack */ false,
            ErrorExpressionCategory::MissingExpression,
            LocMessage::expected_expr,
        );
        let annotation_expr: ExpressionNode;

        if left_expr.node_type == ParseNodeType::Error {
            return left_expr;
        }

        // Is this a type annotation assignment?
        if self._consume_token_if_type(TokenType::Colon) {
            annotation_expr = self._parse_type_annotation(false);
            left_expr =
                TypeAnnotationNode::create(self._get_owner_key(), left_expr, annotation_expr).0;

            if !self._parse_options.is_stub_file
                && PythonVersion::is_less_than(&self._get_language_version(), &PYTHON_VERSION3_6)
            {
                self._add_syntax_error(LocMessage::var_annotation_illegal(), &annotation_expr);
            }

            if !self._consume_token_if_operator(OperatorType::Assign) {
                return left_expr;
            }

            // This is an unfortunate hack that's necessary to accommodate 'TypeAlias'
            // declarations properly. We need to treat this assignment differently than
            // most because the expression on the right side is treated like a type
            // annotation and therefore allows string-literal forward declarations.
            let is_type_alias_declaration =
                self._is_typing_annotation(annotation_expr, "TypeAlias");

            let was_parsing_type_annotation = self._is_parsing_type_annotation;
            if is_type_alias_declaration {
                self._is_parsing_type_annotation = true;
            }

            let right_expr = match self._try_parse_yield_expression() {
                Some(yield_expr) => yield_expr,
                None => self._parse_test_or_star_list_as_expression(
                    /* allow_assignment_expression */ false,
                    /* allow_multiple_unpack */ true,
                    ErrorExpressionCategory::MissingExpression,
                    LocMessage::expected_assign_right_hand_expr,
                ),
            };

            self._is_parsing_type_annotation = was_parsing_type_annotation;

            return AssignmentNode::create(self._get_owner_key(), left_expr, right_expr).0;
        }

        // Is this a simple assignment?
        if self._consume_token_if_operator(OperatorType::Assign) {
            return self._parse_chain_assignments(left_expr);
        }

        if Tokenizer::is_operator_assignment(self._peek_operator_type()) {
            let operator_token = OperatorToken::cast(self._get_next_token()).unwrap();

            let right_expr = match self._try_parse_yield_expression() {
                Some(yield_expr) => yield_expr,
                None => self._parse_test_or_star_list_as_expression(
                    /* allow_assignment_expression */ false,
                    /* allow_multiple_unpack */ true,
                    ErrorExpressionCategory::MissingExpression,
                    LocMessage::expected_binary_right_hand_expr,
                ),
            };
            self._report_conditional_error_for_star_tuple_element(right_expr, PYTHON_VERSION3_9);

            // Make a shallow copy of the dest expression but give it a new ID.
            // Nothing writes to either node's details after this.
            let dest_expr = left_expr.shallow_copy();
            dest_expr.id.set(get_next_node_id());

            return AugmentedAssignmentNode::create(
                self._get_owner_key(),
                left_expr,
                right_expr,
                operator_token.d().operator_type,
                dest_expr,
            )
            .0;
        }

        left_expr
    }

    // parser/parser.ts:4840
    fn _parse_chain_assignments(&mut self, left_expr: ExpressionNode) -> ExpressionNode {
        // Make a list of assignment targets.
        let mut assignment_targets = vec![left_expr];
        let mut right_expr: ExpressionNode;

        loop {
            right_expr = match self._try_parse_yield_expression() {
                Some(yield_expr) => yield_expr,
                None => self._parse_test_or_star_list_as_expression(
                    /* allow_assignment_expression */ false,
                    /* allow_multiple_unpack */ true,
                    ErrorExpressionCategory::MissingExpression,
                    LocMessage::expected_assign_right_hand_expr,
                ),
            };

            if right_expr.node_type == ParseNodeType::Error {
                break;
            }

            // Continue until we've consumed the entire chain.
            if !self._consume_token_if_operator(OperatorType::Assign) {
                break;
            }

            assignment_targets.push(right_expr);
        }

        // Create a tree of assignment expressions starting with the first one.
        // The final RHS value is assigned to the targets left to right in Python.
        let mut assignment_node =
            AssignmentNode::create(self._get_owner_key(), assignment_targets[0], right_expr);

        // Look for a type annotation comment at the end of the line.
        let type_annotation_comment = self._parse_variable_type_annotation_comment();
        if let Some(type_annotation_comment) = type_annotation_comment {
            if assignment_targets.len() > 1 {
                // Type comments are not allowed for chained assignments for the
                // same reason that variable type annotations don't support
                // chained assignments. Note that a type comment was used here
                // so it can be later reported as an error by the binder.
                assignment_node
                    .d()
                    .chained_annotation_comment
                    .set(Some(type_annotation_comment));
            } else {
                assignment_node
                    .d()
                    .annotation_comment
                    .set(Some(type_annotation_comment));
                type_annotation_comment.parent.set(Some(assignment_node.0));
                extend_range(&assignment_node, &type_annotation_comment);
            }
        }

        for (index, &target) in assignment_targets.iter().enumerate() {
            if index > 0 {
                assignment_node =
                    AssignmentNode::create(self._get_owner_key(), target, assignment_node.0);
            }
        }

        assignment_node.0
    }

    // parser/parser.ts:4896
    fn _parse_function_type_annotation(&mut self) -> Option<FunctionAnnotationNode> {
        let open_paren_token = self._peek_token(0);
        if !self._consume_token_if_type(TokenType::OpenParenthesis) {
            let token = self._peek_token(0);
            self._add_syntax_error(LocMessage::expected_open_paren(), &token);
            return None;
        }

        let mut param_annotations: Vec<ExpressionNode> = vec![];

        loop {
            let next_token_type = self._peek_token_type();
            if next_token_type == TokenType::CloseParenthesis
                || next_token_type == TokenType::NewLine
                || next_token_type == TokenType::EndOfStream
            {
                break;
            }

            // Consume "*" or "**" indicators but don't do anything with them.
            // (We don't enforce that these are present, absent, or match
            // the corresponding parameter types.)
            let _ = self._consume_token_if_operator(OperatorType::Multiply)
                || self._consume_token_if_operator(OperatorType::Power);

            let param_annotation = self._parse_type_annotation(false);
            param_annotations.push(param_annotation);

            if !self._consume_token_if_type(TokenType::Comma) {
                break;
            }
        }

        if !self._consume_token_if_type(TokenType::CloseParenthesis) {
            self._add_syntax_error(LocMessage::expected_close_paren(), &open_paren_token);
            self._consume_tokens_until_type(&[TokenType::Colon]);
        }

        if !self._consume_token_if_type(TokenType::Arrow) {
            let token = self._peek_token(0);
            self._add_syntax_error(LocMessage::expected_arrow(), &token);
            return None;
        }

        let return_type = self._parse_type_annotation(false);

        let mut is_param_list_ellipsis = false;
        if param_annotations.len() == 1 && param_annotations[0].node_type == ParseNodeType::Ellipsis
        {
            param_annotations = vec![];
            is_param_list_ellipsis = true;
        }

        Some(FunctionAnnotationNode::create(
            self._get_owner_key(),
            open_paren_token,
            is_param_list_ellipsis,
            param_annotations,
            return_type,
        ))
    }

    // parser/parser.ts:4955
    fn _parse_type_annotation(&mut self, allow_unpack: bool) -> ExpressionNode {
        // Temporary set a flag that indicates we're parsing a type annotation.
        let was_parsing_type_annotation = self._is_parsing_type_annotation;
        self._is_parsing_type_annotation = true;

        // Allow unpack operators.
        let start_token = self._peek_token(0);
        let is_unpack = self._consume_token_if_operator(OperatorType::Multiply);

        if is_unpack
            && allow_unpack
            && !self._parse_options.is_stub_file
            && !self._is_parsing_quoted_text
            && PythonVersion::is_less_than(&self._get_language_version(), &PYTHON_VERSION3_11)
        {
            self._add_syntax_error(LocMessage::unpacked_subscript_illegal(), &start_token);
        }

        let mut result = self._parse_test_expression(/* allow_assignment_expression */ false);
        if is_unpack {
            result = UnpackNode::create(self._get_owner_key(), start_token, result).0;
        }

        self._is_parsing_type_annotation = was_parsing_type_annotation;
        self._has_type_annotations = true;

        result
    }

    /// `string_token` is a `StringToken` or an `FStringStartToken`.
    // parser/parser.ts:4985
    fn _report_string_token_errors(
        &mut self,
        string_token: P<Token>,
        unescaped_result: Option<&string_token_utils::UnescapedString>,
    ) {
        let flags = match &string_token.d {
            TokenDetails::String(d) => d.flags,
            TokenDetails::FStringStart(d) => d.flags,
            _ => unreachable!("a StringToken or FStringStartToken"),
        };

        if flags.any(StringTokenFlags::Unterminated) {
            self._add_syntax_error(LocMessage::string_unterminated(), &string_token);
        }

        if unescaped_result.is_some_and(|r| r.non_ascii_in_bytes) {
            self._add_syntax_error(LocMessage::string_non_ascii_bytes(), &string_token);
        }

        if flags.any(StringTokenFlags::Format) {
            if PythonVersion::is_less_than(&self._get_language_version(), &PYTHON_VERSION3_6) {
                self._add_syntax_error(LocMessage::format_string_illegal(), &string_token);
            }

            if flags.any(StringTokenFlags::Bytes) {
                self._add_syntax_error(LocMessage::format_string_bytes(), &string_token);
            }

            if flags.any(StringTokenFlags::Unicode) {
                self._add_syntax_error(LocMessage::format_string_unicode(), &string_token);
            }

            if flags.any(StringTokenFlags::Template) {
                self._add_syntax_error(LocMessage::format_string_template(), &string_token);
            }
        }

        if flags.any(StringTokenFlags::Template) {
            if PythonVersion::is_less_than(&self._get_language_version(), &PYTHON_VERSION3_14) {
                self._add_syntax_error(LocMessage::template_string_illegal(), &string_token);
            }

            if flags.any(StringTokenFlags::Bytes) {
                self._add_syntax_error(LocMessage::template_string_bytes(), &string_token);
            }

            if flags.any(StringTokenFlags::Unicode) {
                self._add_syntax_error(LocMessage::template_string_unicode(), &string_token);
            }
        }
    }

    // parser/parser.ts:5030
    fn _make_string_node(&mut self, string_token: StringToken) -> StringNode {
        let unescaped_result =
            string_token_utils::get_unescaped_string(&string_token, /* elide_crlf */ true);
        self._report_string_token_errors(string_token.0, Some(&unescaped_result));
        StringNode::create(self._get_owner_key(), string_token, unescaped_result.value)
    }

    // parser/parser.ts:5036
    fn _get_type_annotation_comment_text(&mut self) -> Option<StringToken> {
        if self._token_index == 0 {
            return None;
        }

        let tokens = &self._tokenizer_output.unwrap().get().tokens;
        let cur_token = tokens.get_item_at(self._token_index - 1);
        let next_token = tokens.get_item_at(self._token_index);

        if cur_token.start + cur_token.length == next_token.start {
            return None;
        }

        let file_contents = self._file_contents.as_ref().unwrap();
        let inter_token_contents =
            file_contents.js_slice(cur_token.start + cur_token.length, next_token.start);
        let (prefix_length, type_string) = comment_reg_ex(inter_token_contents)?;

        // Ignore all "ignore" comments. Include "[" in the regular
        // expression because mypy supports ignore comments of the
        // form ignore[errorCode, ...]. We'll treat these as regular
        // ignore statements (as though no errorCodes were included).
        if ignore_comment_reg_ex(type_string.js_trim()) {
            return None;
        }

        let token_offset = cur_token.start + cur_token.length + prefix_length;
        Some(StringToken::create(
            token_offset,
            type_string.js_len(),
            StringTokenFlags::None,
            JsString::from(type_string),
            0,
            /* comments */ None,
        ))
    }

    // parser/parser.ts:5076
    fn _parse_variable_type_annotation_comment(&mut self) -> Option<ExpressionNode> {
        let string_token = self._get_type_annotation_comment_text()?;

        let string_node = self._make_string_node(string_token);
        let string_list_node = StringListNode::create(self._get_owner_key(), vec![string_node.0]);
        let mut parser = Parser::new();
        let parse_results = parser.parse_text_expression(
            self._file_contents.clone().unwrap(),
            string_token.start,
            string_token.length,
            self._parse_options.clone(),
            ParseTextMode::VariableAnnotation,
            /* initial_paren_depth */ 0,
            Some(&self._typing_symbol_aliases),
            Some(self._get_owner_key()),
        );

        for diag in &parse_results.diagnostics {
            self._add_syntax_error(diag.message.clone(), &string_list_node);
        }

        let parse_tree = parse_results.parse_tree?;

        self._string_annotations
            .writer
            .add_all(parse_results.string_annotations);
        Some(parse_tree)
    }

    // parser/parser.ts:5108
    fn _parse_function_type_annotation_comment(
        &mut self,
        string_token: StringToken,
        function_node: FunctionNode,
    ) {
        let string_node = self._make_string_node(string_token);
        let string_list_node = StringListNode::create(self._get_owner_key(), vec![string_node.0]);
        let mut parser = Parser::new();
        let parse_results = parser.parse_text_expression(
            self._file_contents.clone().unwrap(),
            string_token.start,
            string_token.length,
            self._parse_options.clone(),
            ParseTextMode::FunctionAnnotation,
            /* initial_paren_depth */ 0,
            Some(&self._typing_symbol_aliases),
            Some(self._get_owner_key()),
        );

        for diag in &parse_results.diagnostics {
            self._add_syntax_error(diag.message.clone(), &string_list_node);
        }

        let Some(parse_tree) = parse_results.parse_tree else {
            return;
        };

        self._string_annotations
            .writer
            .add_all(parse_results.string_annotations);
        let function_annotation = FunctionAnnotationNode(parse_tree);

        function_node
            .d()
            .func_annotation_comment
            .set(Some(function_annotation));
        function_annotation.parent.set(Some(function_node.0));
        extend_range(&function_node, &function_annotation);
    }
}

/// `text.match(commentRegEx)`, `/^(\s*#\s*type:\s*)([^\r\n]*)/`: the UTF-16
/// length of group 1 and group 2.
// parser/parser.ts:168
fn comment_reg_ex(text: &[u16]) -> Option<(i32, &[u16])> {
    let skip_whitespace = |mut i: usize| {
        while i < text.len() && jsrt::string::is_js_whitespace(text[i]) {
            i += 1;
        }
        i
    };

    let mut i = skip_whitespace(0);
    if text.get(i) != Some(&Char::Hash) {
        return None;
    }
    i = skip_whitespace(i + 1);
    let type_colon: Vec<u16> = "type:".encode_utf16().collect();
    if !text[i..].starts_with(&type_colon) {
        return None;
    }
    i = skip_whitespace(i + type_colon.len());

    let mut end = i;
    while end < text.len() && text[end] != Char::CarriageReturn && text[end] != Char::LineFeed {
        end += 1;
    }
    Some((i as i32, &text[i..end]))
}

/// `text.match(ignoreCommentRegEx)`, `/^ignore(\s|\[|$)/`.
// parser/parser.ts:169
fn ignore_comment_reg_ex(text: &[u16]) -> bool {
    let ignore: Vec<u16> = "ignore".encode_utf16().collect();
    if !text.starts_with(&ignore) {
        return false;
    }
    match text.get(ignore.len()) {
        None => true,
        Some(&c) => jsrt::string::is_js_whitespace(c) || c == Char::OpenBracket,
    }
}

impl Parser {
    // parser/parser.ts:5139
    fn _parse_f_string_replacement_field(
        &mut self,
        field_expressions: &mut Vec<ExpressionNode>,
        middle_tokens: &mut Vec<FStringMiddleToken>,
        format_expressions: &mut Vec<ExpressionNode>,
        nesting_depth: i32,
    ) -> bool {
        let mut next_token = self._get_next_token();

        // The caller should have already confirmed that the next token is an open brace.
        assert!(next_token.type_ == TokenType::OpenCurlyBrace);

        // Consume the expression.
        let expr = match self._try_parse_yield_expression() {
            Some(yield_expr) => yield_expr,
            None => self._parse_test_or_star_list_as_expression(
                /* allow_assignment_expression */ true,
                /* allow_multiple_unpack */ true,
                ErrorExpressionCategory::MissingExpression,
                LocMessage::expected_expr,
            ),
        };

        field_expressions.push(expr);

        if expr.node_type == ParseNodeType::Error {
            return false;
        }

        // Consume an optional "=" token after the expression.
        next_token = self._peek_token(0);
        if next_token.type_ == TokenType::Operator
            && operator_type_of(next_token) == Some(OperatorType::Assign)
        {
            // This feature requires Python 3.8 or newer.
            if PythonVersion::is_less_than(&self._parse_options.python_version, &PYTHON_VERSION3_8)
            {
                self._add_syntax_error(LocMessage::format_string_debugging_illegal(), &next_token);
            }

            self._get_next_token();
            next_token = self._peek_token(0);
        }

        // Consume an optional !r, !s, or !a token.
        if next_token.type_ == TokenType::ExclamationMark {
            self._get_next_token();
            next_token = self._peek_token(0);

            if next_token.type_ != TokenType::Identifier {
                self._add_syntax_error(
                    LocMessage::format_string_expected_conversion(),
                    &next_token,
                );
            } else {
                self._get_next_token();
                next_token = self._peek_token(0);
            }
        }

        if next_token.type_ == TokenType::Colon {
            self._get_next_token();
            self._parse_f_string_format_string(
                field_expressions,
                middle_tokens,
                format_expressions,
                nesting_depth,
            );
            next_token = self._peek_token(0);
        }

        if next_token.type_ != TokenType::CloseCurlyBrace {
            self._add_syntax_error(LocMessage::format_string_unterminated(), &next_token);
            return false;
        } else {
            self._get_next_token();
        }

        // Indicate success.
        true
    }

    // parser/parser.ts:5211
    fn _parse_f_string_format_string(
        &mut self,
        field_expressions: &mut Vec<ExpressionNode>,
        middle_tokens: &mut Vec<FStringMiddleToken>,
        format_expressions: &mut Vec<ExpressionNode>,
        nesting_depth: i32,
    ) {
        loop {
            let next_token = self._peek_token(0);

            if next_token.type_ == TokenType::CloseCurlyBrace
                || next_token.type_ == TokenType::FStringEnd
            {
                break;
            }

            if next_token.type_ == TokenType::FStringMiddle {
                self._get_next_token();
                continue;
            }

            if next_token.type_ == TokenType::OpenCurlyBrace {
                // The Python interpreter reports an error at the point where the
                // nesting level exceeds 1. Don't report the error again for deeper nestings.
                if nesting_depth == 2 {
                    self._add_syntax_error(
                        LocMessage::format_string_nested_format_specifier(),
                        &next_token,
                    );
                }

                self._parse_f_string_replacement_field(
                    field_expressions,
                    middle_tokens,
                    format_expressions,
                    nesting_depth + 1,
                );
                continue;
            }

            break;
        }
    }

    // parser/parser.ts:5244
    fn _parse_format_string(&mut self, start_token: FStringStartToken) -> FormatStringNode {
        let mut middle_tokens: Vec<FStringMiddleToken> = vec![];
        let mut field_expressions: Vec<ExpressionNode> = vec![];
        let mut format_expressions: Vec<ExpressionNode> = vec![];
        let mut end_token: Option<FStringEndToken> = None;

        // Consume middle tokens and expressions until we hit a "{" or "}" token.
        loop {
            let next_token = self._peek_token(0);

            if next_token.type_ == TokenType::FStringEnd {
                let token = FStringEndToken::cast(next_token).unwrap();
                end_token = Some(token);

                if token.d().flags.any(StringTokenFlags::Unterminated) {
                    self._add_syntax_error(LocMessage::string_unterminated(), &start_token);
                }
                self._get_next_token();
                break;
            }

            if next_token.type_ == TokenType::FStringMiddle {
                middle_tokens.push(FStringMiddleToken::cast(next_token).unwrap());
                self._get_next_token();
                continue;
            }

            if next_token.type_ == TokenType::OpenCurlyBrace {
                if !self._parse_f_string_replacement_field(
                    &mut field_expressions,
                    &mut middle_tokens,
                    &mut format_expressions,
                    0,
                ) {
                    // An error was reported. Try to recover the parse.
                    if self._consume_tokens_until_type(&[TokenType::FStringEnd, TokenType::NewLine])
                        && self._peek_token(0).type_ == TokenType::FStringEnd
                    {
                        self._get_next_token();
                    }
                    break;
                }
                continue;
            }

            // We've hit an error. Try to recover as gracefully as possible.
            if next_token.type_ != TokenType::NewLine {
                // Consume tokens until we find the end.
                if self._consume_tokens_until_type(&[TokenType::FStringEnd]) {
                    self._get_next_token();
                }
            }

            self._add_syntax_error(
                if next_token.type_ == TokenType::CloseCurlyBrace {
                    LocMessage::format_string_brace()
                } else {
                    LocMessage::string_unterminated()
                },
                &next_token,
            );
            break;
        }

        self._report_string_token_errors(start_token.0, None);

        FormatStringNode::create(
            self._get_owner_key(),
            start_token,
            end_token,
            middle_tokens,
            field_expressions,
            format_expressions,
        )
    }

    // parser/parser.ts:5312
    fn _create_binary_operation_node(
        &mut self,
        left_expression: ExpressionNode,
        right_expression: ExpressionNode,
        operator_token: P<Token>,
        operator: OperatorType,
    ) -> ExpressionNode {
        let binary_node = BinaryOperationNode::create(
            self._get_owner_key(),
            left_expression,
            right_expression,
            operator_token,
            operator,
        );

        // Determine if we're exceeding the max parse depth. If so, replace
        // the subnode with an error node. Otherwise we risk crashing in the binder
        // or type evaluator.
        let left_max_depth = self
            ._max_child_depth_map
            .get(&left_expression.id.get())
            .copied()
            .unwrap_or(0);
        let right_max_depth = self
            ._max_child_depth_map
            .get(&right_expression.id.get())
            .copied()
            .unwrap_or(0);

        if left_max_depth >= MAX_CHILD_NODE_DEPTH || right_max_depth >= MAX_CHILD_NODE_DEPTH {
            self._add_syntax_error(LocMessage::max_parse_depth_exceeded(), &binary_node);
            return ErrorNode::create(
                self._get_owner_key(),
                &binary_node,
                ErrorExpressionCategory::MaxDepthExceeded,
                None,
                None,
            )
            .0;
        }

        self._max_child_depth_map.insert(
            binary_node.id.get(),
            left_max_depth.max(right_max_depth) + 1,
        );
        binary_node.0
    }

    // parser/parser.ts:5341
    fn _create_unary_operation_node(
        &mut self,
        operator_token: P<Token>,
        expression: ExpressionNode,
        operator: OperatorType,
    ) -> ExpressionNode {
        let unary_node =
            UnaryOperationNode::create(self._get_owner_key(), operator_token, expression, operator);

        // Determine if we're exceeding the max parse depth. If so, replace
        // the subnode with an error node. Otherwise we risk crashing in the binder
        // or type evaluator.

        let max_depth = self
            ._max_child_depth_map
            .get(&expression.id.get())
            .copied()
            .unwrap_or(0);
        if max_depth >= MAX_CHILD_NODE_DEPTH {
            self._add_syntax_error(LocMessage::max_parse_depth_exceeded(), &unary_node);
            return ErrorNode::create(
                self._get_owner_key(),
                &unary_node,
                ErrorExpressionCategory::MaxDepthExceeded,
                None,
                None,
            )
            .0;
        }

        self._max_child_depth_map
            .insert(unary_node.id.get(), max_depth + 1);
        unary_node.0
    }

    // parser/parser.ts:5358
    fn _parse_string_list(&mut self) -> StringListNode {
        let mut string_list: Vec<P<ParseNode>> = vec![];

        loop {
            let next_token = self._peek_token(0);
            if next_token.type_ == TokenType::String {
                let token = StringToken::cast(self._get_next_token()).unwrap();
                string_list.push(self._make_string_node(token).0);
            } else if next_token.type_ == TokenType::FStringStart {
                let token = FStringStartToken::cast(self._get_next_token()).unwrap();
                string_list.push(self._parse_format_string(token).0);
            } else {
                break;
            }
        }

        let string_node = StringListNode::create(self._get_owner_key(), string_list);

        // If we're parsing a type annotation, parse the contents of the string.
        if self._is_parsing_type_annotation {
            let strings = string_node.d().strings.borrow().clone();
            // Don't allow multiple strings because we have no way of reporting
            // parse errors that span strings.
            if strings.len() > 1 {
                if self._is_parsing_quoted_text {
                    self._add_syntax_error(LocMessage::annotation_spans_strings(), &string_node);
                }
            } else if strings[0].node_type == ParseNodeType::FormatString {
                if self._is_parsing_quoted_text {
                    self._add_syntax_error(LocMessage::annotation_format_string(), &string_node);
                }
            } else {
                let string_token = StringNode::cast(strings[0]).unwrap().d().token.get();
                let string_value = string_token_utils::get_unescaped_string(
                    &string_token,
                    /* elide_crlf */ false,
                );
                let unescaped_string = string_value.value;
                let token_offset = string_token.start;
                let prefix_length =
                    string_token.d().prefix_length + string_token.d().quote_mark_length;

                // Don't allow escape characters because we have no way of mapping
                // error ranges back to the escaped text.
                if unescaped_string.js_len()
                    != string_token.length - prefix_length - string_token.d().quote_mark_length
                {
                    if self._is_parsing_quoted_text {
                        self._add_syntax_error(
                            LocMessage::annotation_string_escape(),
                            &string_node,
                        );
                    }
                } else if !string_token.d().flags.any(StringTokenFlags(
                    StringTokenFlags::Raw.0
                        | StringTokenFlags::Bytes.0
                        | StringTokenFlags::Format.0
                        | StringTokenFlags::Template.0,
                )) {
                    let mut parser = Parser::new();
                    let parse_results = parser.parse_text_expression(
                        self._file_contents.clone().unwrap(),
                        token_offset + prefix_length,
                        unescaped_string.js_len(),
                        self._parse_options.clone(),
                        ParseTextMode::VariableAnnotation,
                        if string_token.d().flags.any(StringTokenFlags::Triplicate) {
                            1
                        } else {
                            0
                        },
                        Some(&self._typing_symbol_aliases),
                        Some(self._get_owner_key()),
                    );

                    if parse_results.diagnostics.is_empty()
                        || self._parse_options.report_errors_for_parsed_string_contents
                    {
                        for diag in &parse_results.diagnostics {
                            self._add_syntax_error(diag.message.clone(), &string_node);
                        }

                        if let Some(parse_tree) = parse_results.parse_tree {
                            self._string_annotations.writer.set(string_node, parse_tree);
                            self._string_annotations
                                .writer
                                .add_all(parse_results.string_annotations);
                            parse_tree.parent.set(Some(string_node.0));
                        }
                    }
                }
            }
        }

        string_node
    }

    // Python 3.8 added support for star (unpack) expressions in tuples
    // following a return or yield statement in cases where the tuple
    // wasn't surrounded in parentheses.
    /// `python_version` is `pythonVersion3_8` where TypeScript omits it.
    // parser/parser.ts:5446
    fn _report_conditional_error_for_star_tuple_element(
        &mut self,
        possible_tuple_expr: ExpressionNode,
        python_version: PythonVersion,
    ) {
        let Some(tuple) = TupleNode::cast(possible_tuple_expr) else {
            return;
        };

        if tuple.d().has_parens.get() {
            return;
        }

        if PythonVersion::is_greater_or_equal_to(
            &self._parse_options.python_version,
            &python_version,
        ) {
            return;
        }

        let items = tuple.d().items.borrow().clone();
        for expr in items {
            if expr.node_type == ParseNodeType::Unpack {
                self._add_syntax_error(LocMessage::unpack_tuples_illegal(), &expr);
                return;
            }
        }
    }

    // Peeks at the next token and returns true if it can never
    // represent the start of an expression.
    // parser/parser.ts:5472
    fn _is_next_token_never_expression(&self) -> bool {
        let next_token = self._peek_token(0);
        match next_token.type_ {
            TokenType::Keyword => {
                if let Some(KeywordType::For | KeywordType::In | KeywordType::If) =
                    self._peek_keyword_type()
                {
                    return true;
                }
            }

            TokenType::Operator => {
                if let Some(
                    OperatorType::AddEqual
                    | OperatorType::SubtractEqual
                    | OperatorType::MultiplyEqual
                    | OperatorType::DivideEqual
                    | OperatorType::ModEqual
                    | OperatorType::BitwiseAndEqual
                    | OperatorType::BitwiseOrEqual
                    | OperatorType::BitwiseXorEqual
                    | OperatorType::LeftShiftEqual
                    | OperatorType::RightShiftEqual
                    | OperatorType::PowerEqual
                    | OperatorType::FloorDivideEqual
                    | OperatorType::Assign,
                ) = self._peek_operator_type()
                {
                    return true;
                }
            }

            TokenType::Indent
            | TokenType::Dedent
            | TokenType::NewLine
            | TokenType::EndOfStream
            | TokenType::Semicolon
            | TokenType::CloseParenthesis
            | TokenType::CloseBracket
            | TokenType::CloseCurlyBrace
            | TokenType::Comma
            | TokenType::Colon
            | TokenType::ExclamationMark
            | TokenType::FStringMiddle
            | TokenType::FStringEnd => return true,

            _ => {}
        }

        false
    }

    // parser/parser.ts:5524
    fn _disallow_assignment_expression(&mut self, callback: impl FnOnce(&mut Self)) {
        let was_allowed = self._assignment_expressions_allowed;
        self._assignment_expressions_allowed = false;

        callback(self);

        self._assignment_expressions_allowed = was_allowed;
    }

    // parser/parser.ts:5533
    fn _get_next_token(&mut self) -> P<Token> {
        let token = self._tokens.unwrap().get_item_at(self._token_index);
        if !self._at_eof() {
            self._token_index += 1;
        }

        token
    }

    // parser/parser.ts:5542
    fn _at_eof(&self) -> bool {
        // Are we pointing at the last token in the stream (which is
        // assumed to be an end-of-stream token)?
        self._token_index >= self._token_count - 1
    }

    // parser/parser.ts:5548
    fn _peek_token(&self, count: i32) -> P<Token> {
        let target_index = self._token_index + count;
        if target_index < 0 {
            return self._tokens.unwrap().get_item_at(0);
        }

        if target_index >= self._token_count {
            return self._tokens.unwrap().get_item_at(self._token_count - 1);
        }

        self._tokens.unwrap().get_item_at(target_index)
    }

    // parser/parser.ts:5561
    fn _peek_token_type(&self) -> TokenType {
        self._peek_token(0).type_
    }

    // parser/parser.ts:5565
    fn _peek_keyword_type(&self) -> Option<KeywordType> {
        let next_token = self._peek_token(0);
        if next_token.type_ != TokenType::Keyword {
            return None;
        }

        keyword_type_of(next_token)
    }

    // parser/parser.ts:5574
    fn _peek_operator_type(&self) -> Option<OperatorType> {
        let next_token = self._peek_token(0);
        if next_token.type_ != TokenType::Operator {
            return None;
        }

        operator_type_of(next_token)
    }

    // parser/parser.ts:5583
    fn _get_token_if_identifier(&mut self) -> Option<IdentifierToken> {
        let next_token = self._peek_token(0);
        if next_token.type_ == TokenType::Identifier {
            return IdentifierToken::cast(self._get_next_token());
        }

        // If the next token is invalid, treat it as an identifier.
        if next_token.type_ == TokenType::Invalid {
            self._get_next_token();
            self._add_syntax_error(LocMessage::invalid_identifier_char(), &next_token);
            return Some(IdentifierToken::create_with_shared_comments(
                next_token.start,
                next_token.length,
                String::new(),
                next_token.comments,
            ));
        }

        // If this is a "soft keyword", it can be converted into an identifier.
        if next_token.type_ == TokenType::Keyword {
            let keyword_token = KeywordToken::cast(next_token).unwrap();
            if KeywordToken::is_soft_keyword(keyword_token) {
                let keyword_text = String::from_utf16_lossy(
                    self._file_contents
                        .as_ref()
                        .unwrap()
                        .js_substr(next_token.start, next_token.length),
                );
                self._get_next_token();
                return Some(IdentifierToken::create_with_shared_comments(
                    next_token.start,
                    next_token.length,
                    keyword_text,
                    next_token.comments,
                ));
            }
        }

        None
    }

    // Consumes tokens until the next one in the stream is
    // either a specified terminator or the end-of-stream
    // token.
    // parser/parser.ts:5612
    fn _consume_tokens_until_type(&mut self, terminators: &[TokenType]) -> bool {
        loop {
            let token = self._peek_token(0);
            if terminators.iter().any(|&term| term == token.type_) {
                return true;
            }

            if token.type_ == TokenType::EndOfStream {
                return false;
            }

            self._get_next_token();
        }
    }

    // parser/parser.ts:5627
    fn _get_token_if_type(&mut self, token_type: TokenType) -> Option<P<Token>> {
        if self._peek_token_type() == token_type {
            return Some(self._get_next_token());
        }

        None
    }

    // parser/parser.ts:5635
    fn _consume_token_if_type(&mut self, token_type: TokenType) -> bool {
        self._get_token_if_type(token_type).is_some()
    }

    // parser/parser.ts:5639
    fn _consume_token_if_keyword(&mut self, keyword_type: KeywordType) -> bool {
        if self._peek_keyword_type() == Some(keyword_type) {
            self._get_next_token();
            return true;
        }

        false
    }

    // parser/parser.ts:5648
    fn _consume_token_if_operator(&mut self, operator_type: OperatorType) -> bool {
        if self._peek_operator_type() == Some(operator_type) {
            self._get_next_token();
            return true;
        }

        false
    }

    // parser/parser.ts:5657
    fn _get_keyword_token(&mut self, keyword_type: KeywordType) -> KeywordToken {
        let keyword_token = KeywordToken::cast(self._get_next_token());
        assert!(keyword_token.is_some_and(|t| t.type_ == TokenType::Keyword));
        let keyword_token = keyword_token.unwrap();
        assert!(keyword_token.d().keyword_type == keyword_type);
        keyword_token
    }

    // parser/parser.ts:5664
    fn _get_language_version(&self) -> PythonVersion {
        self._parse_options.python_version
    }

    // parser/parser.ts:5668
    fn _suppress_errors(&mut self, callback: impl FnOnce(&mut Self)) {
        let errors_were_suppressed = self._are_errors_suppressed;
        self._are_errors_suppressed = true;
        callback(self);
        self._are_errors_suppressed = errors_were_suppressed;
    }

    // parser/parser.ts:5678
    fn _add_syntax_error(&mut self, message: String, range: &(impl HasTextRange + ?Sized)) {
        let range = range.text_range();

        if !self._are_errors_suppressed {
            self._diag_sink.add_error(
                message,
                convert_offsets_to_range(
                    range.start,
                    range.start + range.length,
                    &self._tokenizer_output.unwrap().get().lines,
                ),
            );
        }
    }
}

#[cfg(test)]
#[path = "parser_tests.rs"]
mod tests;
