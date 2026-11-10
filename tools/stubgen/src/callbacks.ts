// Stored callbacks (docs/porting.md "memory"): a closure that a struct stores becomes a struct with
// one named field per captured variable and a `call` method, so a collector can see every `P` it
// holds. A closure that captures nothing is a plain `fn` pointer.
//
// A stored callback slot is a field (interface or class property, closure-factory local) whose type
// holds a function type, or a function type alias some field names. Its creation sites are the
// arrow functions and function expressions assigned to it anywhere on the check path.

import ts from 'typescript';
import type { Gen } from './gen.ts';
import { IR, render, Uses } from './ir.ts';
import { crateModuleOf, fieldName, pascal, rustIdent, snake } from './names.ts';
import { lineOf, relPath, resolveSymbol } from './program.ts';
import { skipParens } from './writes.ts';

export interface Capture {
    name: string;
    ir: IR;
    note: string | undefined;
}

export interface CallbackSite {
    // The closure, or the named function a slot is given.
    fn: FnExpr | ts.FunctionDeclaration;
    // Where the closure is made or the function named: the marker of `call`.
    at: ts.Node;
    // The struct for this site; the slot's own name when it has one site.
    structName: string;
    variant: string;
    captures: Capture[];
    notes: string[];
}

export interface CallbackDef {
    slot: ts.Node;
    name: string;
    file: string;
    fnIR: Extract<IR, { k: 'fn' }>;
    paramNames: string[];
    sites: CallbackSite[];
}

type FnExpr = ts.ArrowFunction | ts.FunctionExpression;

interface Site {
    fn: FnExpr | ts.FunctionDeclaration;
    at: ts.Node;
}

export class Callbacks {
    private sitesBySlot: Map<ts.Node, Site[]> | undefined;
    private storedAliases: Set<ts.Node> | undefined;
    readonly defs = new Map<string, CallbackDef>();
    private readonly bySlot = new Map<ts.Node, string>();

    constructor(private readonly gen: Gen) {}

    private get checker(): ts.TypeChecker {
        return this.gen.checker;
    }

    // Function type aliases that a field or parameter property names: those are stored.
    isStoredAlias(alias: ts.TypeAliasDeclaration): boolean {
        if (!this.storedAliases) {
            this.storedAliases = new Set();
            for (const sf of this.gen.loaded.files) {
                const visit = (n: ts.Node) => {
                    const typeNode = (ts.isPropertySignature(n) || ts.isPropertyDeclaration(n) || (ts.isParameter(n) && ts.isConstructorDeclaration(n.parent) && ts.isParameterPropertyDeclaration(n, n.parent))) ? n.type : undefined;
                    if (typeNode) {
                        const refs = (t: ts.Node) => {
                            if (ts.isTypeReferenceNode(t)) {
                                const d = resolveSymbol(this.checker, this.checker.getSymbolAtLocation(t.typeName))?.declarations?.[0];
                                if (d && ts.isTypeAliasDeclaration(d) && ts.isFunctionTypeNode(d.type)) {
                                    this.storedAliases?.add(d);
                                }
                            }
                            ts.forEachChild(t, refs);
                        };
                        refs(typeNode);
                    }
                    ts.forEachChild(n, visit);
                };
                visit(sf);
            }
        }
        return this.storedAliases.has(alias);
    }

    private index(): Map<ts.Node, Site[]> {
        if (this.sitesBySlot) {
            return this.sitesBySlot;
        }
        const out = new Map<ts.Node, Site[]>();
        const add = (slot: ts.Node, fn: FnExpr | ts.FunctionDeclaration, at: ts.Node) => {
            const list = out.get(slot) ?? [];
            if (!list.some((x) => x.fn === fn)) {
                list.push({ fn, at });
            }
            out.set(slot, list);
        };
        for (const sf of this.gen.loaded.files) {
            const visit = (n: ts.Node) => {
                if (ts.isArrowFunction(n) || ts.isFunctionExpression(n)) {
                    for (const slot of this.slotsOf(n)) {
                        add(slot, n, n);
                    }
                } else if (ts.isIdentifier(n) && !(ts.isCallExpression(n.parent) && n.parent.expression === n)) {
                    // A named function given to a slot: `printType(type, flags, getEffectiveReturnType)`.
                    const d = resolveSymbol(this.checker, this.checker.getSymbolAtLocation(n))?.declarations?.[0];
                    if (d && ts.isFunctionDeclaration(d) && d.body && d.name !== n) {
                        for (const slot of this.slotsOf(n)) {
                            add(slot, d, n);
                        }
                    }
                }
                ts.forEachChild(n, visit);
            };
            visit(sf);
        }
        this.sitesBySlot = out;
        return out;
    }

    // The slots a closure expression, or a function named by an identifier, is stored in.
    private slotsOf(fn: FnExpr | ts.Identifier): ts.Node[] {
        const out: ts.Node[] = [];
        let up: ts.Node = fn;
        while (
            ts.isParenthesizedExpression(up.parent) ||
            (ts.isConditionalExpression(up.parent) && up !== up.parent.condition) ||
            (ts.isBinaryExpression(up.parent) && [ts.SyntaxKind.QuestionQuestionToken, ts.SyntaxKind.BarBarToken].includes(up.parent.operatorToken.kind))
        ) {
            up = up.parent;
        }
        const parent = up.parent;
        if (ts.isBinaryExpression(parent) && parent.operatorToken.kind === ts.SyntaxKind.EqualsToken && parent.right === up) {
            const l = skipParens(parent.left);
            if (ts.isPropertyAccessExpression(l)) {
                out.push(...(this.checker.getSymbolAtLocation(l.name)?.declarations ?? []));
            }
        } else if (ts.isPropertyAssignment(parent) && parent.initializer === up && ts.isObjectLiteralExpression(parent.parent)) {
            const ct = this.checker.getContextualType(parent.parent);
            const name = ts.isIdentifier(parent.name) ? parent.name.text : undefined;
            const prop = ct && name ? this.checker.getNonNullableType(ct).getProperty(name) : undefined;
            out.push(...(prop?.declarations ?? []));
        }
        const ct = this.checker.getContextualType(fn);
        const alias = ct?.aliasSymbol?.declarations?.[0];
        if (alias && ts.isTypeAliasDeclaration(alias)) {
            out.push(alias);
        }
        // A closure inside the initializer of a variable or property: the variable is its slot.
        for (let a: ts.Node | undefined = ts.isIdentifier(fn) ? undefined : fn.parent; a && !ts.isSourceFile(a); a = a.parent) {
            if ((ts.isVariableDeclaration(a) || ts.isPropertyDeclaration(a)) && a.initializer && a.type) {
                out.push(a);
                break;
            }
            if (ts.isFunctionLike(a) || ts.isBlock(a)) {
                break;
            }
        }
        return out;
    }

    // The Rust type for a function type stored in `slot` (a field, factory local or alias).
    callbackIR(slot: ts.Node, fnIR: Extract<IR, { k: 'fn' }>, file: string, slotName: string): IR {
        const known = this.bySlot.get(slot);
        const sites = this.index().get(slot) ?? [];
        const def = known ? this.defs.get(known) : this.define(slot, fnIR, file, slotName, sites);
        if (!def) {
            return fnIR;
        }
        if (def.sites.length > 0 && def.sites.every((s) => s.captures.length === 0)) {
            const scratch = new Uses();
            const params = fnIR.params.map((p) => render(p, 'param', scratch)).join(', ');
            const ret = fnIR.ret.k === 'unit' ? '' : ` -> ${render(fnIR.ret, 'ret', scratch)}`;
            const text = `fn(${params})${ret}`;
            return { k: 'raw', text, param: text, copy: true, uses: [...scratch.set] };
        }
        return { k: 'named', text: def.name, copy: false, uses: [`${crateModuleOf(def.file)}::${def.name}`] };
    }

    private define(slot: ts.Node, fnIR: Extract<IR, { k: 'fn' }>, file: string, slotName: string, sites: Site[]): CallbackDef {
        let name = pascal(slotName);
        if (this.gen.decls.has(`${file}:${name}`) && !ts.isTypeAliasDeclaration(slot)) {
            name = `${name}Callback`;
        }
        const def: CallbackDef = { slot, name, file, fnIR, paramNames: paramNamesOf(slot, fnIR), sites: [] };
        this.bySlot.set(slot, `${file}:${name}`);
        this.defs.set(`${file}:${name}`, def);
        const usedVariants = new Set<string>();
        for (const { fn, at } of sites) {
            let variant = pascal(ts.isFunctionDeclaration(fn) ? fn.name?.text ?? 'Fn' : enclosingName(fn));
            if (usedVariants.has(variant)) {
                variant = `${variant}${lineOf(at)}`;
            }
            usedVariants.add(variant);
            const { captures, notes } = this.captures(fn, `${name}.${variant}`, file);
            def.sites.push({ fn, at, structName: sites.length === 1 ? name : `${name}${variant}`, variant, captures, notes });
        }
        return def;
    }

    // Variables of enclosing functions that `fn` reads, by name, in first-use order. A local function
    // it calls contributes its own captures; a closure factory's locals are reached through the
    // factory's struct.
    private captures(fn: FnExpr | ts.FunctionDeclaration, siteKey: string, file: string): { captures: Capture[]; notes: string[] } {
        const captures: Capture[] = [];
        const notes: string[] = [];
        const seen = new Set<string>();
        const visitedFns = new Set<ts.Node>();
        const factoryFns = new Map<ts.Node, string>();
        for (const f of this.gen.table.factories) {
            const d = this.gen.factoryNode(f);
            if (d) {
                factoryFns.set(d, f.name);
            }
        }
        const add = (name: string, ir: IR, note?: string) => {
            if (!seen.has(name)) {
                seen.add(name);
                captures.push({ name, ir, note });
            }
        };
        const enclosingFn = (n: ts.Node): ts.Node | undefined => {
            for (let a = n.parent; a && !ts.isSourceFile(a); a = a.parent) {
                if (ts.isFunctionLike(a)) {
                    return a;
                }
            }
            return undefined;
        };
        const scan = (root: ts.Node, owner: ts.Node) => {
            const visit = (n: ts.Node) => {
                if (n.kind === ts.SyntaxKind.ThisKeyword) {
                    const cls = classOf(n);
                    if (cls?.name) {
                        const key = `${relPath(this.gen.loaded, cls.getSourceFile())}:${cls.name.text}`;
                        add(snake(cls.name.text), this.gen.mapKey(key, [], undefined, { file, key: `${siteKey}.this`, count: 0 }));
                    }
                }
                if (ts.isIdentifier(n) && !isNameOnly(n)) {
                    const sym = ts.isShorthandPropertyAssignment(n.parent) && n.parent.name === n ? this.checker.getShorthandAssignmentValueSymbol(n.parent) : this.checker.getSymbolAtLocation(n);
                    const decl = resolveSymbol(this.checker, sym)?.declarations?.[0];
                    const declFn = decl ? enclosingFn(decl) : undefined;
                    if (decl && declFn && !isWithin(decl, owner) && decl.getSourceFile() === fn.getSourceFile()) {
                        const factory = factoryFns.get(declFn) ?? (ts.isFunctionDeclaration(decl) && factoryFns.has(decl) ? factoryFns.get(decl) : undefined);
                        if (factory !== undefined) {
                            add(snake(factory), this.gen.factoryIR(factory));
                        } else if (ts.isFunctionDeclaration(decl) && decl.body) {
                            if (!visitedFns.has(decl)) {
                                visitedFns.add(decl);
                                notes.push(`calls the local function \`${decl.name?.text}\` (:${lineOf(decl)})`);
                                scan(decl.body, decl);
                            }
                        } else if (ts.isVariableDeclaration(decl) && decl.initializer && isClosure(skipParens(decl.initializer)) && !this.gen.writes.reassigned.has(decl)) {
                            // A local closure (`const synthesizeMethods = () => ..`): its captures.
                            const init = skipParens(decl.initializer);
                            if (!visitedFns.has(init)) {
                                visitedFns.add(init);
                                notes.push(`calls the local closure \`${n.text}\` (:${lineOf(decl)})`);
                                scan(init, init);
                            }
                        } else if (sym && (ts.isParameter(decl) || ts.isVariableDeclaration(decl) || ts.isBindingElement(decl))) {
                            const t = this.checker.getTypeOfSymbolAtLocation(sym, n);
                            const ir = this.gen.mapType(t, { file, key: `${siteKey}.${n.text}`, count: 0 }, 0);
                            const reassigned = this.gen.writes.reassigned.has(decl);
                            add(fieldName(n.text), ir, reassigned ? `TS reassigns \`${n.text}\`: check whether a write follows the closure, which reads the latest value` : undefined);
                        }
                    }
                }
                ts.forEachChild(n, visit);
            };
            visit(root);
        };
        // A named function given to a slot: a closure factory's method captures the factory, a
        // local function its own captures, a module function nothing.
        if (ts.isFunctionDeclaration(fn)) {
            const outer = enclosingFn(fn);
            const factory = outer ? factoryFns.get(outer) : undefined;
            if (factory !== undefined) {
                add(snake(factory), this.gen.factoryIR(factory));
            } else if (outer && fn.body) {
                scan(fn.body, fn);
            }
            return { captures, notes };
        }
        scan(fn.body, fn);
        return { captures, notes };
    }

    // Lines of the callback types whose home is `file`.
    emit(file: string, uses: Uses): string[] {
        const marker = (n: ts.Node) => `// ${relPath(this.gen.loaded, n.getSourceFile())}:${lineOf(n)}`;
        const out: string[] = [];
        for (const def of this.defs.values()) {
            if (def.file !== file || (def.sites.length > 0 && def.sites.every((s) => s.captures.length === 0))) {
                continue;
            }
            const params = def.fnIR.params.map((p, i) => `${def.paramNames[i]}: ${render(p, 'param', uses)}`);
            const ret = def.fnIR.ret.k === 'unit' ? '' : ` -> ${render(def.fnIR.ret, 'ret', uses)}`;
            const sig = `pub fn call(${['&self', ...params].join(', ')})${ret}`;
            const slotMarker = ts.isTypeAliasDeclaration(def.slot) ? [marker(def.slot)] : [];
            if (def.sites.length === 0) {
                out.push(...slotMarker, `// A stored callback with no closure creation site on the check path.`, '#[derive(Clone)]', `pub struct ${def.name} {}`, '', `impl ${def.name} {`, `    ${sig} {`, '        todo!()', '    }', '}', '');
                continue;
            }
            for (const s of def.sites) {
                const fields = s.captures.map((c) => `    pub ${rustIdent(c.name)}: ${render(c.ir, 'field', uses)},${c.note ? ` // ${c.note}` : ''}`);
                const doc = s.notes.length ? [`    /// The closure ${s.notes.join('; ')}.`] : [];
                out.push(
                    ...(def.sites.length === 1 ? slotMarker : []),
                    `// A stored callback (docs/porting.md "memory"): the closure's captures, by name.`,
                    '#[derive(Clone)]',
                    `pub struct ${s.structName} {`,
                    ...fields,
                    '}',
                    '',
                    `impl ${s.structName} {`,
                    `    ${marker(s.at)}`,
                    ...doc,
                    `    ${sig} {`,
                    '        todo!()',
                    '    }',
                    '}',
                    ''
                );
            }
            if (def.sites.length > 1) {
                const args = def.paramNames.slice(0, def.fnIR.params.length).join(', ');
                out.push(
                    ...slotMarker,
                    `// One variant per closure stored in this slot.`,
                    '#[derive(Clone)]',
                    `pub enum ${def.name} {`,
                    ...def.sites.map((s) => `    ${s.variant}(${s.structName}),`),
                    '}',
                    '',
                    `impl ${def.name} {`,
                    `    ${sig} {`,
                    '        match self {',
                    ...def.sites.map((s) => `            ${def.name}::${s.variant}(c) => c.call(${args}),`),
                    '        }',
                    '    }',
                    '}',
                    ''
                );
            }
        }
        return out;
    }
}

function paramNamesOf(slot: ts.Node, fnIR: Extract<IR, { k: 'fn' }>): string[] {
    let fnType: ts.FunctionTypeNode | undefined;
    const find = (n: ts.Node) => {
        if (!fnType && ts.isFunctionTypeNode(n)) {
            fnType = n;
            return;
        }
        ts.forEachChild(n, find);
    };
    const typeNode =
        ts.isTypeAliasDeclaration(slot) || ts.isPropertySignature(slot) || ts.isPropertyDeclaration(slot) || ts.isVariableDeclaration(slot) || ts.isParameter(slot) ? slot.type : undefined;
    if (typeNode) {
        find(typeNode);
    }
    return fnIR.params.map((_, i) => {
        const p = fnType?.parameters[i];
        return p && ts.isIdentifier(p.name) ? rustIdent(snake(p.name.text)) : `a${i}`;
    });
}

function isClosure(e: ts.Node): e is FnExpr {
    return ts.isArrowFunction(e) || ts.isFunctionExpression(e);
}

function enclosingName(fn: ts.Node): string {
    for (let a = fn.parent; a && !ts.isSourceFile(a); a = a.parent) {
        if ((ts.isFunctionDeclaration(a) || ts.isMethodDeclaration(a)) && a.name && ts.isIdentifier(a.name)) {
            return a.name.text;
        }
    }
    return relPath0(fn);
}

function relPath0(n: ts.Node): string {
    return n.getSourceFile().fileName.replace(/.*\//, '').replace(/\.ts$/, '');
}

function classOf(n: ts.Node): ts.ClassDeclaration | undefined {
    for (let a = n.parent; a && !ts.isSourceFile(a); a = a.parent) {
        if (ts.isClassDeclaration(a)) {
            return a;
        }
        if (ts.isFunctionDeclaration(a) || ts.isFunctionExpression(a)) {
            return undefined;
        }
    }
    return undefined;
}

function isWithin(n: ts.Node, root: ts.Node): boolean {
    for (let a: ts.Node | undefined = n; a; a = a.parent) {
        if (a === root) {
            return true;
        }
    }
    return false;
}

// An identifier that names a property, not a variable.
function isNameOnly(n: ts.Identifier): boolean {
    const p = n.parent;
    return (
        (ts.isPropertyAccessExpression(p) && p.name === n) ||
        (ts.isPropertyAssignment(p) && p.name === n) ||
        (ts.isQualifiedName(p) && p.right === n) ||
        ts.isTypeReferenceNode(p) ||
        (ts.isMethodDeclaration(p) && p.name === n) ||
        (ts.isPropertyDeclaration(p) && p.name === n) ||
        (ts.isPropertySignature(p) && p.name === n) ||
        (ts.isBindingElement(p) && p.propertyName === n)
    );
}

