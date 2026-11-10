# Oracle dump format

Format of the text dumps that the pyright fork writes (`~/pyright-oracle`, `src/oracle/oracleDumper.ts`) and that `crates/pyright/src/oracle/` must reproduce byte for byte.
This file is the whole contract. A Rust dumper written from it alone must match the TypeScript output.

Format version: oracle-1.

## running

- `PYRIGHT_ORACLE_DUMP=<dir>` turns dumping on. Unset or empty, the fork behaves like upstream pyright.
- `PYRIGHT_ORACLE_STAGES=<list>` selects stages: a comma-separated list of `tokens`, `tree`, `bind`, `flow`, `types`, `diag`. Whitespace around each name is ignored. Unset or empty selects all six.
- An unknown stage name, or any error while dumping, prints `pyright oracle: <message>` to stderr and exits with code 70.

## files

Each stage writes one file per source file: `<dir>/<stage>/<mapped path>.txt`.

The mapped path of a file path `p`:

1. If `p` contains `/typeshed-fallback/`, it is `typeshed/` followed by the part of `p` after the first occurrence of that segment. Example: `.../dist/typeshed-fallback/stdlib/builtins.pyi` maps to `typeshed/stdlib/builtins.pyi`.
2. Otherwise, if `p` starts with `/`, it is `fs` followed by `p`. Example: `/home/a/x.py` maps to `fs/home/a/x.py`.
3. Otherwise it is `other/` followed by `p`.

The same mapping applies to every file `Uri` printed inside a dump.
It lets a checker with its typeshed copy elsewhere produce the same dumps.

When a stage file is written:

| stage | written | for which files |
|---|---|---|
| tokens, tree | each time `SourceFile.parse` gets a result from the parser, before import resolution | every file pyright parses |
| bind, flow | each time `SourceFile.bind` has run the binder and published its node info | every file pyright binds |
| types, diag | once, when analysis completes, before the CLI reports | every file in `Program.getSourceFileInfoList()` order whose parse results are current |

A second write to the same dump file replaces it and records an event in `oracle-run.txt`.

### `oracle-run.txt`

Written at the end of the run, after the types and diag files:

```
cacheEmptyCount: <n>
events: [<k>]
  [0]: rewritten "<stage>/<mapped path>.txt"
stages: <enabled stages in the order tokens,tree,bind,flow,types,diag, comma-separated>
```

`cacheEmptyCount` counts calls to `Program.emptyCache`. A run with a nonzero count is not a valid reference.
The compare tool ignores this file.

### encoding

- A dump file is UTF-8 text. Lines end with `\n`, including the last one. A dump with no lines is an empty file.
- Every string from pyright goes through the string escape below, so no lone surrogate reaches the file.

## values

### layout

- Each line is `<indent><key>: <value>` or `<indent><key>:`. The indent is two spaces per depth level.
- `<key>:` with nothing after the colon opens a nested object. Its fields follow at depth + 1.
- Lines carry no trailing whitespace.

### fields of an object

An object prints its own enumerable properties:

- sorted by property name, comparing UTF-16 code units (JavaScript's default `sort`, the same as byte order for ASCII names);
- without a property named `id` (parse node, flow node and symbol ids are global counters);
- without a property whose value is `undefined`. An absent property and a property set to `undefined` print the same: nothing. The Rust side models such fields as `Option` and omits `None`, even where it could store a default.

An object with no printed fields prints `<key>: {}`.

### scalars

| value | printed as |
|---|---|
| string | the string escape below |
| number | JavaScript `String(n)`: `0.1`, `1e+21`, `1e-7`, `NaN`, `Infinity`, `-0` prints `0` |
| bigint | its decimal digits followed by `n`: `9223372036854775808n` |
| boolean | `true`, `false` |
| null | `null` |
| undefined | `undefined` (only an array element or map value can print it) |
| function | `<function>` |

String escape, as JavaScript's `JSON.stringify` (ES2019 and later):

- wrap in `"`;
- `"` becomes `\"` and `\` becomes `\\`;
- U+0008, U+0009, U+000A, U+000C, U+000D become `\b`, `\t`, `\n`, `\f`, `\r`;
- any other code unit below U+0020 becomes `\u00xx`, lowercase hex;
- a lone surrogate (a high surrogate not followed by a low one, or a low surrogate not preceded by a high one) becomes `\udxxx`, lowercase hex;
- everything else is written as is, in UTF-8. U+007F, U+2028 and U+2029 are not escaped.

Enums print as their number, except parse node types and token types, which print by name (see below).

### containers

| value | header line | element lines at depth + 1 |
|---|---|---|
| array | `<key>: [<length>]` | `[<index>]: <element>` |
| `TextRangeCollection` | `<key>: [<count>]` | `[<index>]: <item>` |
| `Map` | `<key>: Map(<size>)` | `<map key>: <value>`, in insertion order |
| `Set` | `<key>: Set(<size>)` | `[<index>]: <element>`, in insertion order |

A map key prints like a scalar: a string key is escaped and quoted (`"name": ...`), a number key prints as `String(n)` (`12: ...`).

### one-line forms

These objects print on one line, `<key>: <form>`, checked in this order:

1. stage-specific forms: scope references in bind (`#3`, `external`), flow node references in flow (`F12`).
2. A parse node not printed as a tree child: `<NodeTypeName>@<start>+<length>`, for example `Name@120+3`.
3. A `Uri`: `Uri("<text>")`. For a `file` URI the text is the mapped path; for any other URI it is `toString()` (`""` for the empty URI, `*** unresolved module ***` for the unresolved-module marker).
4. A pyright type object: `<type>`. Types print only in the types stage, as described there.
5. An object whose printed fields are exactly `start` and `length` (a `TextRange`): `<start>+<length>`.
6. An object whose printed fields are exactly `start` and `end`, each with exactly `line` and `character` (a `Range`): `<start.line>:<start.character>-<end.line>:<end.character>`.

### tokens

A token prints on one line wherever it appears (tokens stage and tree stage):

```
<key>: <TokenTypeName> <start>+<length>[ <field>=<scalar>]...
```

The `<field>=<scalar>` pairs are the token's printed fields other than `type`, `start`, `length` and `comments`, in field order. Per token type:

| token type | fields after the range |
|---|---|
| Indent | `indentAmount`, `isIndentAmbiguous` |
| Dedent | `indentAmount`, `isDedentAmbiguous`, `matchesIndent` |
| NewLine | `newLineType` |
| Keyword | `keywordType` |
| Operator | `operatorType` |
| Identifier | `value` |
| Number | `isImaginary`, `isInteger`, `value` |
| String | `escapedValue`, `flags`, `prefixLength`, `quoteMarkLength` |
| FStringStart | `flags`, `prefixLength`, `quoteMarkLength` |
| FStringMiddle | `escapedValue`, `flags` |
| FStringEnd | `flags` |
| all others | none |

A Number token's `value` is a number or a bigint, printed as in the scalar table: `value=0.5`, `value=9223372036854775808n`.

When the token has `comments`, they nest below the token line:

```
  [17]: NewLine 74+1 newLineType=1
    comments: [1]
      [0]: 61+13 type=0 value=" comment here"
```

A comment line is `[<index>]: <start>+<length> type=<CommentType number> value=<escaped string>`.

Token type names, by `TokenType` value from 0: `Invalid`, `EndOfStream`, `NewLine`, `Indent`, `Dedent`, `String`, `Number`, `Identifier`, `Keyword`, `Operator`, `Colon`, `Semicolon`, `Comma`, `OpenParenthesis`, `CloseParenthesis`, `OpenBracket`, `CloseBracket`, `OpenCurlyBrace`, `CloseCurlyBrace`, `Ellipsis`, `Dot`, `Arrow`, `Backtick`, `ExclamationMark`, `FStringStart`, `FStringMiddle`, `FStringEnd`.

Parse node type names are the `ParseNodeType` member names in `parser/parseNodes.ts`, `Error` (0) through `TypeAlias` (77).

## tokens stage

The file prints the `TokenizerOutput` that the parser returned, as an object at depth 0. Its fields, in order:

```
hasPredominantTabSequence: <bool>
lines: [<n>]
  [<i>]: <start>+<length>
predominantEndOfLineSequence: <string>
predominantSingleQuoteCharacter: <string>
predominantTabSequence: <string>
pyrightIgnoreLines: Map(<n>)
  <line number>:
    range: <start>+<length>
    rulesList: [<k>]
      [<j>]:
        range: <start>+<length>
        text: <string>
tokens: [<n>]
  [<i>]: <token line>
typeIgnoreAll:
  range: <start>+<length>
  rulesList: ...
typeIgnoreLines: Map(<n>)
  <line number>: <IgnoreComment, as in pyrightIgnoreLines>
```

`rulesList` and `typeIgnoreAll` are omitted when undefined.

Example (start of a file `import os`):

```
hasPredominantTabSequence: true
lines: [19]
  [0]: 0+10
  [1]: 10+24
...
tokens: [105]
  [0]: Keyword 0+6 keywordType=21
  [1]: Identifier 7+2 value="os"
  [2]: NewLine 9+1 newLineType=1
```

## tree stage

The file prints the parser's `ParserOutput` as an object at depth 0, without its `lines` field (the tokens stage has it) and without its `stringAnnotations` field (annotations print inside the tree). Its fields, in order:

```
containsWildcardImport: <bool>
futureImports: Set(<n>)
  [<i>]: <string>
hasTypeAnnotations: <bool>
importedModules: [<n>]
  [<i>]:
    importedSymbols: Set(<k>)
      [<j>]: <string>
    leadingDots: <n>
    nameNode: ModuleName@<start>+<length>
    nameParts: [<k>]
      [<j>]: <string>
parseTree: <node>
typingSymbolAliases: Map(<n>)
  <string key>: <string>
```

`importedSymbols` is omitted when undefined.

### nodes

A node prints a header line, then its fields at depth + 1:

```
<key>: <NodeTypeName> <start>+<length>[ !parent=<parent>]
```

- The header uses a space between the name and the range. A reference uses `@`. That keeps a child with no fields distinct from a reference.
- `!parent=<parent>` appears when the node's `parent` is not the node that holds it. `<parent>` is the parent's reference form, or `undefined`. The root (`parseTree`) gets no check.

The node's fields are the printed fields of its `d` object, in field order. `id`, `parent`, `a`, `nodeType`, `start` and `length` are not fields; they appear only in the header.
Each `d` field prints by these rules:

- A parse node that is one of the node's children prints as a nested node. Children are the entries of `getChildNodes(node)` (`parser/parseTreeUtils.ts`, with the parser's string annotations), compared by identity.
- A parse node that is not a child prints as a reference, `<key>: <NodeTypeName>@<start>+<length>`. Example: `AssignmentNode.d.chainedAnnotationComment`.
- An array prints its header, then each element by the same two rules; an `undefined` element prints `[<i>]: undefined`.
- Anything else prints as a value: tokens as token lines, numbers, booleans and strings as scalars.

After the `d` fields, two more kinds of line can follow:

- `+<name>: <value>` for each own property of the node object outside `nodeType`, `start`, `length`, `id`, `parent`, `a` and `d`, sorted, except the `stringAnnotations` property that the parser attaches to the module node. None appear at the pinned commit; the Rust side prints none.
- `(annotation): <node>` for a `StringList` node that has a parser string annotation (`getParserStringAnnotation`). It prints as a child of the `StringList`, including the parent check.

Example (`x: "List[int]" = [1, 2]`):

```
    [2]: StatementList 35+23
      firstToken: Identifier 35+1 value="x"
      statements: [1]
        [0]: Assignment 35+23
          leftExpr: TypeAnnotation 35+14
            annotation: StringList 38+11
              hasParens: false
              strings: [1]
                [0]: String 38+11
                  token: String 38+11 escapedValue="List[int]" flags=2 prefixLength=0 quoteMarkLength=1
                  value: "List[int]"
              (annotation): Index 39+9
                items: [1]
                  [0]: Argument 44+3
                    argCategory: 0
                    valueExpr: Name 44+3
                      token: Identifier 44+3 value="int"
                      value: "int"
                leftExpr: Name 39+4
                  token: Identifier 39+4 value="List"
                  value: "List"
                trailingComma: false
            valueExpr: Name 35+1
              token: Identifier 35+1 value="x"
              value: "x"
          rightExpr: List 52+6
            items: [2]
              [0]: Number 53+1
                isImaginary: false
                isInteger: true
                value: 1
              [1]: Number 56+1
                isImaginary: false
                isInteger: true
                value: 2
```

A `StringNode`'s `value` holds the unescaped string, so a Python `"\ud800"` prints `value: "\ud800"` (one lone surrogate) while its token prints `escapedValue="\\ud800"`.

Sections for the bind, flow, diag and types stages follow in later commits.
