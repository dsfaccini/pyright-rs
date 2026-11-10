// Carries ported functions and module-variable initializers from a crate file into its regenerated
// stub, by marker.
//
// A crate file that someone has started porting holds functions with real bodies, each under the
// `// <file>.ts:<line>` marker its stub had. Merging replaces the stub's `todo!()` function at that
// marker with the ported one, and adds the crate file's imports the stub lacks. Anything ported that
// sits at no stub marker is reported as lost, so a rerun never drops work silently.

const MARKER = /^\s*\/\/ ([A-Za-z0-9_/.]+\.ts):(\d+)\s*$/;
const FN_HEAD = /^\s*(pub(\([^)]*\))?\s+)?(const\s+)?(unsafe\s+)?fn\s/;
// A module variable's `static` inside `thread_local!`; its marker sits above it.
const STATIC_HEAD = /^\s*(pub(\([^)]*\))?\s+)?static\s/;

interface FnItem {
    marker: string;
    start: number;
    // Line of the closing brace, inclusive.
    end: number;
    ported: boolean;
}

// Index just past the brace that closes the one at `open`, skipping strings, chars and comments.
// With `stmt`, index just past the first `;` outside brackets from `open` on.
function closeBrace(text: string, open: number, stmt = false): number {
    let depth = 0;
    let i = open;
    while (i < text.length) {
        const c = text[i];
        if (c === '/' && text[i + 1] === '/') {
            i = text.indexOf('\n', i);
            if (i < 0) {
                return -1;
            }
            continue;
        }
        if (c === '/' && text[i + 1] === '*') {
            i = text.indexOf('*/', i + 2) + 2;
            continue;
        }
        if (c === '"') {
            i++;
            while (i < text.length && text[i] !== '"') {
                i += text[i] === '\\' ? 2 : 1;
            }
            i++;
            continue;
        }
        if (c === 'r' && /^r#*"/.test(text.slice(i, i + 8)) && !/[A-Za-z0-9_]/.test(text[i - 1] ?? '')) {
            const hashes = text.slice(i + 1).match(/^#*/)?.[0] ?? '';
            const endQuote = '"' + hashes;
            i = text.indexOf(endQuote, i + 2 + hashes.length) + endQuote.length;
            continue;
        }
        if (c === "'") {
            // A char literal ('x', '\n', '\u{..}'), not a lifetime ('a).
            const m = text.slice(i).match(/^'(\\u\{[0-9a-fA-F]+\}|\\.|[^\\'])'/);
            if (m) {
                i += m[0].length;
                continue;
            }
        }
        if (stmt) {
            if (c === '{' || c === '(' || c === '[') {
                depth++;
            } else if (c === '}' || c === ')' || c === ']') {
                depth--;
            } else if (c === ';' && depth === 0) {
                return i + 1;
            }
        } else if (c === '{') {
            depth++;
        } else if (c === '}') {
            depth--;
            if (depth === 0) {
                return i + 1;
            }
        }
        i++;
    }
    return -1;
}

function fnItems(text: string): FnItem[] {
    const lines = text.split('\n');
    const lineStarts: number[] = [];
    let pos = 0;
    for (const l of lines) {
        lineStarts.push(pos);
        pos += l.length + 1;
    }
    const lineAt = (offset: number) => {
        let lo = 0;
        let hi = lineStarts.length - 1;
        while (lo < hi) {
            const mid = (lo + hi + 1) >> 1;
            if (lineStarts[mid] <= offset) {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        return lo;
    };
    const out: FnItem[] = [];
    for (let i = 0; i < lines.length; i++) {
        const m = lines[i].match(MARKER);
        if (!m) {
            continue;
        }
        let j = i + 1;
        while (j < lines.length && /^\s*(\/\/|#\[|$)/.test(lines[j])) {
            j++;
        }
        if (j < lines.length && STATIC_HEAD.test(lines[j])) {
            // `static NAME: T = init;`: ported once the initializer is no longer `todo!()`.
            const eq = text.indexOf('=', lineStarts[j]);
            const close = eq < 0 ? -1 : closeBrace(text, eq + 1, true);
            if (close >= 0) {
                const init = text.slice(eq + 1, close - 1).trim();
                out.push({ marker: `${m[1]}:${m[2]}`, start: i, end: lineAt(close - 1), ported: init !== 'todo!()' });
            }
            continue;
        }
        if (j >= lines.length || !FN_HEAD.test(lines[j])) {
            continue;
        }
        // The body opens at the first brace after the signature; a trait declaration ends in `;`.
        const from = lineStarts[j];
        const open = text.indexOf('{', from);
        if (open < 0 || text.slice(from, open).replace(/\[[^\]]*\]/g, '').includes(';')) {
            continue;
        }
        const close = closeBrace(text, open);
        if (close < 0) {
            continue;
        }
        const body = text.slice(open + 1, close - 1).trim();
        out.push({ marker: `${m[1]}:${m[2]}`, start: i, end: lineAt(close - 1), ported: body !== 'todo!()' });
    }
    return out;
}

// `use a::b::{C, D};` -> ["a::b::C", "a::b::D"]; `super::` resolved against `modPath`.
function importPaths(text: string, modPath: string): string[] {
    const parent = modPath.split('::').slice(0, -1).join('::');
    const out: string[] = [];
    for (const m of text.matchAll(/^(?:pub\s+)?use\s+([^;]+);/gms)) {
        const spec = m[1].replace(/\s+/g, '').replace(/^super::/, `${parent}::`).replace(/^self::/, `${modPath}::`);
        const brace = spec.match(/^(.*)::\{([^{}]*)\}$/);
        if (brace) {
            for (const n of brace[2].split(',').filter((x) => x.length > 0)) {
                out.push(n === 'self' ? brace[1] : `${brace[1]}::${n}`);
            }
        } else {
            out.push(spec);
        }
    }
    return out;
}

function lastName(p: string): string {
    const m = p.match(/(?:^|::)([A-Za-z_][A-Za-z0-9_]*)(?:as([A-Za-z_][A-Za-z0-9_]*))?$/);
    return m ? (m[2] ?? m[1]) : p;
}

export interface MergeResult {
    text: string;
    kept: string[];
    // Lines of the kept functions in the crate file, inclusive.
    keptRanges: [number, number][];
    lost: string[];
}

// `modPath` is the crate path of the module (`crate::analyzer::declaration_utils`).
export function mergePorted(stub: string, crate: string, modPath: string): MergeResult {
    const ported = fnItems(crate).filter((f) => f.ported);
    const crateLines = crate.split('\n');
    const stubItems = new Map(fnItems(stub).map((f) => [f.marker, f]));
    const lines = stub.split('\n');
    const kept: string[] = [];
    const keptRanges: [number, number][] = [];
    const lost: string[] = [];
    const replacements: { item: FnItem; with: string[] }[] = [];
    for (const p of ported) {
        const target = stubItems.get(p.marker);
        if (!target) {
            lost.push(p.marker);
            continue;
        }
        kept.push(p.marker);
        keptRanges.push([p.start, p.end]);
        replacements.push({ item: target, with: crateLines.slice(p.start, p.end + 1) });
    }
    replacements.sort((a, b) => b.item.start - a.item.start);
    for (const r of replacements) {
        lines.splice(r.item.start, r.item.end - r.item.start + 1, ...r.with);
    }
    // Imports the ported functions need that the stub lacks, by imported name.
    const have = new Set(importPaths(stub, modPath).map(lastName));
    const add: string[] = [];
    for (const p of importPaths(crate, modPath)) {
        const n = lastName(p);
        if (!have.has(n) && n !== '*') {
            have.add(n);
            add.push(`use ${p};`);
        }
    }
    if (add.length) {
        let at = lines.findIndex((l) => /^use /.test(l));
        if (at < 0) {
            at = lines.findIndex((l) => !/^(\/\/!|#!\[|\s|\)\]|$)/.test(l));
        }
        lines.splice(Math.max(at, 0), 0, ...add);
    }
    // A ported file that dropped the stub's blanket `#![allow(..)]` keeps it dropped, so clippy sees the port.
    let text = lines.join('\n');
    if (!crate.includes('#![allow(')) {
        text = text.replace(/^#!\[allow\([^\]]*\)\]\n/m, '');
    }
    return { text, kept, keptRanges, lost };
}
