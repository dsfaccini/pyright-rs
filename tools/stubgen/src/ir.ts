// Rust type IR and its rendering per position.
//
// One TypeScript type renders differently by position (docs/evaluator-design.md, "type mapping"):
// a `string` field is `String`, a `string` parameter is `&str`; an array field is `Vec<T>`, an array
// parameter is `&[T]`, or `&mut Vec<T>` when the callee changes it in place.

export type IR =
    | { k: 'bool' }
    | { k: 'str' }
    | { k: 'num'; rust: string }
    | { k: 'unit' }
    | { k: 'never' }
    | { k: 'opt'; inner: IR }
    | { k: 'vec'; elem: IR }
    | { k: 'map'; key: IR; val: IR }
    | { k: 'set'; elem: IR }
    | { k: 'tuple'; elems: IR[] }
    | { k: 'fn'; params: IR[]; ret: IR }
    // A named Rust type. `copy` types pass by value; others pass as `&T`.
    | { k: 'named'; text: string; copy: boolean; uses: string[] }
    // The struct a closure factory builds (TypeEvaluator): `&T` as a parameter, `P<T>` when stored.
    | { k: 'factory'; name: string; uses: string[] }
    // A trait object. `mutRecv` traits are passed as `&mut dyn T` and stored as `Box<dyn T>`.
    | { k: 'trait'; text: string; mutRecv: boolean; uses: string[] }
    | { k: 'generic'; name: string }
    | { k: 'raw'; text: string; param: string | undefined; copy: boolean; uses: string[] }
    | { k: 'unmapped'; ts: string; fallback: IR | undefined };

export type Pos = 'field' | 'param' | 'ret' | 'elem';

export const UNIT: IR = { k: 'unit' };
export const BOOL: IR = { k: 'bool' };
export const STR: IR = { k: 'str' };

export function opt(inner: IR): IR {
    if (inner.k === 'opt' || inner.k === 'unit') {
        return inner;
    }
    return { k: 'opt', inner };
}

export function isCopy(t: IR): boolean {
    switch (t.k) {
        case 'bool':
        case 'num':
        case 'unit':
        case 'never':
        case 'factory':
            return true;
        case 'opt':
            return isCopy(t.inner);
        case 'tuple':
            return t.elems.every(isCopy);
        case 'named':
        case 'raw':
            return t.copy;
        case 'trait':
            return !t.mutRecv;
        case 'unmapped':
            return t.fallback ? isCopy(t.fallback) : true;
        default:
            return false;
    }
}

export class Uses {
    readonly set = new Set<string>();
    add(paths: readonly string[]) {
        for (const p of paths) {
            this.set.add(p);
        }
    }
}

// `mutated`: the parameter is changed in place by the callee.
export function render(t: IR, pos: Pos, uses: Uses, mutated = false): string {
    switch (t.k) {
        case 'bool':
            return 'bool';
        case 'str':
            return pos === 'param' ? (mutated ? '&mut String' : '&str') : 'String';
        case 'num':
            return t.rust;
        case 'unit':
            return '()';
        case 'never':
            return pos === 'ret' ? '!' : '()';
        case 'opt':
            return renderOpt(t.inner, pos, uses, mutated);
        case 'vec': {
            const e = render(t.elem, 'elem', uses);
            if (pos === 'param') {
                return mutated ? `&mut Vec<${e}>` : `&[${e}]`;
            }
            return `Vec<${e}>`;
        }
        case 'map': {
            uses.add(['jsrt::JsMap']);
            const m = `JsMap<${render(t.key, 'elem', uses)}, ${render(t.val, 'elem', uses)}>`;
            return pos === 'param' ? (mutated ? `&mut ${m}` : `&${m}`) : m;
        }
        case 'set': {
            uses.add(['jsrt::JsSet']);
            const s = `JsSet<${render(t.elem, 'elem', uses)}>`;
            return pos === 'param' ? (mutated ? `&mut ${s}` : `&${s}`) : s;
        }
        case 'tuple':
            return `(${t.elems.map((e) => render(e, 'elem', uses)).join(', ')}${t.elems.length === 1 ? ',' : ''})`;
        case 'fn': {
            const params = t.params.map((p) => render(p, 'param', uses)).join(', ');
            const ret = t.ret.k === 'unit' ? '' : ` -> ${render(t.ret, 'ret', uses)}`;
            if (pos === 'param') {
                return `&mut dyn FnMut(${params})${ret}`;
            }
            if (pos === 'ret') {
                return `Box<dyn FnMut(${params})${ret}>`;
            }
            uses.add(['std::rc::Rc']);
            return `Rc<dyn Fn(${params})${ret}>`;
        }
        case 'named':
            uses.add(t.uses);
            if (pos === 'param' && !t.copy) {
                return mutated ? `&mut ${t.text}` : `&${t.text}`;
            }
            return t.text;
        case 'factory':
            uses.add(t.uses);
            if (pos === 'param') {
                return `&${t.name}`;
            }
            uses.add(['jsrt::P']);
            return `P<${t.name}>`;
        case 'trait':
            uses.add(t.uses);
            if (pos === 'param') {
                return t.mutRecv ? `&mut dyn ${t.text}` : `&dyn ${t.text}`;
            }
            return t.mutRecv ? `Box<dyn ${t.text}>` : `&'static dyn ${t.text}`;
        case 'generic':
            return t.name;
        case 'raw':
            uses.add(t.uses);
            if (pos === 'param') {
                return t.param ?? (t.copy ? t.text : mutated ? `&mut ${t.text}` : `&${t.text}`);
            }
            return t.text;
        case 'unmapped': {
            const note = ` /*? ${t.ts.replace(/\*\//g, '* /').replace(/\s+/g, ' ')} */`;
            return t.fallback ? render(t.fallback, pos, uses, mutated) + note : `()${note}`;
        }
    }
}

function renderOpt(inner: IR, pos: Pos, uses: Uses, mutated: boolean): string {
    if (pos === 'param') {
        // Borrowed forms inside Option, so callers pass `x.as_deref()` and the like.
        switch (inner.k) {
            case 'str':
            case 'vec':
            case 'map':
            case 'set':
            case 'named':
            case 'factory':
            case 'trait':
            case 'raw':
            case 'fn':
                return `Option<${render(inner, 'param', uses, mutated)}>`;
            default:
                return `Option<${render(inner, 'elem', uses)}>`;
        }
    }
    return `Option<${render(inner, pos === 'ret' ? 'ret' : 'elem', uses)}>`;
}

// The Rust type of a field. Fields written after construction get interior mutability.
export function renderField(t: IR, uses: Uses, interior: boolean): string {
    const base = render(t, 'field', uses);
    if (!interior) {
        return base;
    }
    if (isCopy(t)) {
        uses.add(['std::cell::Cell']);
        return `Cell<${base}>`;
    }
    uses.add(['std::cell::RefCell']);
    return `RefCell<${base}>`;
}
