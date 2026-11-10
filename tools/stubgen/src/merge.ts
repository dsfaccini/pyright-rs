// Merges a regenerated stub into a crate file that someone has started porting, by marker.
//
// The crate file is the base: its header, imports, ported functions and every line outside an
// unported item stay byte for byte. Only unported items change: a `todo!()` function or static
// and a generated struct or enum take the stub's text, a new stub item is inserted after its
// predecessor, and an unported item the stub no longer has is removed. A file with no ported
// function is replaced by its stub. What the merge cannot place (stub lines outside items, a ported
// function whose marker the stub lost) is reported, never dropped or rewritten.

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

// Generated `struct` and `enum` items under a marker, with their line ranges, inclusive. Nobody ports
// them: the generator owns their fields.
const TYPE_HEAD = /^\s*(pub(\([^)]*\))?\s+)?(struct|enum)\s/;

function typeItems(text: string): { marker: string; start: number; end: number }[] {
    const lines = text.split('\n');
    const out: { marker: string; start: number; end: number }[] = [];
    for (let i = 0; i < lines.length; i++) {
        const m = lines[i].match(MARKER);
        if (!m) {
            continue;
        }
        let j = i + 1;
        while (j < lines.length && /^\s*(\/\/|#\[|$)/.test(lines[j])) {
            j++;
        }
        if (j >= lines.length || !TYPE_HEAD.test(lines[j])) {
            continue;
        }
        let end = j;
        if (!/;\s*$/.test(lines[j])) {
            while (end < lines.length && lines[end] !== lines[j].match(/^\s*/)?.[0] + '}') {
                end++;
            }
        }
        if (end < lines.length) {
            out.push({ marker: `${m[1]}:${m[2]}`, start: i, end });
        }
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
    // Markers of the ported functions kept as they are.
    kept: string[];
    // Ported functions whose marker the stub no longer has: kept as they are, reported.
    lost: string[];
    // Unported items the stub changed, added or no longer has.
    updated: string[];
    inserted: string[];
    removed: string[];
    // Stub items or lines the merge could not place: the file's owner adds them.
    pending: string[];
}

interface Item {
    kind: 'fn' | 'type';
    marker: string;
    key: string;
    start: number;
    end: number;
    ported: boolean;
}

// Top-level items under a marker: functions and statics (ported or not) and generated structs and
// enums. A marked item inside another item (a local function in a ported body) belongs to it.
function items(text: string): Item[] {
    const all: Item[] = [
        ...fnItems(text).map((f) => ({ kind: 'fn' as const, key: '', ...f })),
        ...typeItems(text).map((t) => ({ kind: 'type' as const, key: '', ported: false, ...t })),
    ].sort((a, b) => a.start - b.start);
    const top = all.filter((x) => !all.some((y) => y !== x && y.start <= x.start && x.end <= y.end && (y.start < x.start || x.end < y.end)));
    const seen = new Map<string, number>();
    for (const x of top) {
        const base = `${x.kind}:${x.marker}`;
        const n = seen.get(base) ?? 0;
        seen.set(base, n + 1);
        x.key = `${base}#${n}`;
    }
    return top;
}

// `modPath` is the crate path of the module (`crate::analyzer::declaration_utils`).
export function mergePorted(stub: string, crate: string, modPath: string): MergeResult {
    const res: MergeResult = { text: stub, kept: [], lost: [], updated: [], inserted: [], removed: [], pending: [] };
    const cItems = items(crate);
    if (!cItems.some((c) => c.kind === 'fn' && c.ported)) {
        return res;
    }
    const sItems = items(stub);
    const cl = crate.split('\n');
    const sl = stub.split('\n');
    const sByKey = new Map(sItems.map((x) => [x.key, x]));
    const cByKey = new Map(cItems.map((x) => [x.key, x]));
    // Edits to the crate lines: delete `del` lines at `at`, insert `ins`; `order` keeps insertions
    // at one line in stub order.
    const edits: { at: number; del: number; ins: string[]; order: number }[] = [];
    const changed: string[] = [];
    for (const c of cItems) {
        const s = sByKey.get(c.key);
        if (c.kind === 'fn' && c.ported) {
            (s ? res.kept : res.lost).push(c.marker);
            continue;
        }
        if (!s) {
            // With the blank line before it, or else the one after it, so no blank line is left over.
            const before = c.start > 0 && cl[c.start - 1].trim() === '';
            const after = !before && c.end + 1 < cl.length && cl[c.end + 1].trim() === '';
            const from = before ? c.start - 1 : c.start;
            edits.push({ at: from, del: c.end - from + 1 + (after ? 1 : 0), ins: [], order: 0 });
            res.removed.push(c.marker);
            continue;
        }
        const text = sl.slice(s.start, s.end + 1);
        if (text.join('\n') !== cl.slice(c.start, c.end + 1).join('\n')) {
            edits.push({ at: c.start, del: c.end - c.start + 1, ins: text, order: 0 });
            changed.push(text.join('\n'));
            res.updated.push(c.marker);
        }
    }
    // A new stub item goes after its predecessor when only blank lines separate them in the stub,
    // so it lands in the same block; a chain of new items follows the first one.
    const placedAfter = new Map<string, number>();
    sItems.forEach((s, i) => {
        if (cByKey.has(s.key)) {
            return;
        }
        const prev = i > 0 ? sItems[i - 1] : undefined;
        const anchor = prev ? (cByKey.get(prev.key)?.end ?? placedAfter.get(prev.key)) : undefined;
        if (prev === undefined || anchor === undefined || !sl.slice(prev.end + 1, s.start).every((l) => l.trim() === '')) {
            res.pending.push(`item at ${s.marker}`);
            return;
        }
        const text = sl.slice(s.start, s.end + 1);
        edits.push({ at: anchor + 1, del: 0, ins: ['', ...text], order: i });
        placedAfter.set(s.key, anchor);
        changed.push(text.join('\n'));
        res.inserted.push(s.marker);
    });
    // Stub lines outside items that appear nowhere in the crate file: generated code the merge
    // does not own (impl headers, accessors, [items] blocks). Reported for the owner.
    const inItem = new Set<number>();
    for (const s of sItems) {
        for (let i = s.start; i <= s.end; i++) {
            inItem.add(i);
        }
    }
    // The header and imports, everything before the first marker, are the file's own.
    const stubBody = sl.findIndex((l) => MARKER.test(l));
    const crateLines = new Set(cl.map((l) => l.trim()));
    const missing = sl.filter((l, i) => i >= stubBody && !inItem.has(i) && l.trim() !== '' && !crateLines.has(l.trim()));
    if (missing.length) {
        res.pending.push(`${missing.length} stub line(s) outside items, first: \`${missing[0].trim()}\``);
    }
    // Imports a changed or inserted item names that the crate file lacks, from the stub's imports.
    const have = new Set(importPaths(crate, modPath).map(lastName));
    const add: string[] = [];
    const words = new Set(changed.join('\n').match(/[A-Za-z_][A-Za-z0-9_]*/g) ?? []);
    for (const p of importPaths(stub, modPath)) {
        const n = lastName(p);
        if (n !== '*' && !have.has(n) && words.has(n)) {
            have.add(n);
            add.push(`use ${p};`);
        }
    }
    if (add.length) {
        // After the file's last import, which comes before its first marker.
        const firstItem = cl.findIndex((l) => MARKER.test(l));
        const lastUse = cl.reduce((at, l, i) => ((firstItem < 0 || i < firstItem) && (/^use\s[^;]*;\s*$/.test(l) || /^\};\s*$/.test(l)) ? i : at), -1);
        edits.push({ at: lastUse + 1, del: 0, ins: add, order: -1 });
    }
    edits.sort((a, b) => b.at - a.at || b.order - a.order);
    const out = [...cl];
    for (const e of edits) {
        out.splice(e.at, e.del, ...e.ins);
    }
    res.text = out.join('\n');
    return res;
}
