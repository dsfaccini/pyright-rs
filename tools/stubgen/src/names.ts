// Rust names for TypeScript names, per docs/porting.md "names".

const RUST_KEYWORDS = new Set([
    'as', 'async', 'await', 'box', 'break', 'const', 'continue', 'crate', 'do', 'dyn', 'else', 'enum',
    'extern', 'false', 'final', 'fn', 'for', 'gen', 'if', 'impl', 'in', 'let', 'loop', 'macro', 'match',
    'mod', 'move', 'mut', 'override', 'priv', 'pub', 'ref', 'return', 'self', 'static', 'struct', 'super',
    'trait', 'true', 'try', 'type', 'typeof', 'unsafe', 'unsized', 'use', 'virtual', 'where', 'while', 'yield',
]);

// camelCase or PascalCase to snake_case. Acronym runs stay one word: `getJSXType` -> `get_jsx_type`.
// A leading underscore is kept: `_readIndent` -> `_read_indent`.
export function snake(name: string): string {
    const lead = name.match(/^_*/)?.[0] ?? '';
    const body = name.slice(lead.length);
    const out = body
        .replace(/([A-Z]+)([A-Z][a-z])/g, '$1_$2')
        .replace(/([a-z0-9])([A-Z])/g, '$1_$2')
        .replace(/\$/g, '_')
        .toLowerCase();
    return lead + out;
}

export function rustIdent(name: string): string {
    return RUST_KEYWORDS.has(name) ? `${name}_` : name;
}

// A method name (a function with a receiver). `clone` becomes `clone_` (docs/porting.md "names"):
// on a `P<T>`, `.clone()` resolves to `P`'s `Clone` and never reaches `T::clone`.
export function fnIdent(name: string): string {
    return name === 'clone' ? 'clone_' : rustIdent(name);
}

export function fieldName(name: string): string {
    return rustIdent(snake(name));
}

export function screaming(name: string): string {
    return snake(name).replace(/^_+/, '').toUpperCase();
}

// `analyzer/parseTreeWalker.ts` -> `analyzer/parse_tree_walker`.
export function modulePathOf(tsPath: string): string {
    return tsPath
        .replace(/\.ts$/, '')
        .split('/')
        .map((part) => snake(part))
        .join('/');
}

// `analyzer/parseTreeWalker.ts` -> `crate::analyzer::parse_tree_walker`.
export function crateModuleOf(tsPath: string): string {
    return 'crate::' + modulePathOf(tsPath).split('/').join('::');
}

export function pascal(text: string): string {
    const words = text.split(/[^A-Za-z0-9]+/).filter((w) => w.length > 0);
    const joined = words.map((w) => w[0].toUpperCase() + w.slice(1)).join('');
    if (joined.length === 0) {
        return 'Empty';
    }
    return /^[0-9]/.test(joined) ? `V${joined}` : joined;
}
