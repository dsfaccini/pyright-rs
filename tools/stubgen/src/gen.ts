// The generator core: classifies TypeScript declarations and maps TypeScript types to the IR.

import ts from 'typescript';
import { BOOL, IR, STR, UNIT, isCopy, opt, render as renderIR } from './ir.ts';
import { crateModuleOf, pascal, rustIdent } from './names.ts';
import { Loaded, lineOf, propName, relPath, resolveSymbol } from './program.ts';
import { Accessor, Factory, Family, Mode, NumberTable, Table } from './table.ts';
import { Writes } from './writes.ts';
import { Callbacks } from './callbacks.ts';

export interface FamilyMember {
    fam: Family;
    name: string;
    decl: ts.InterfaceDeclaration;
    // Enum member name of the kind field, or the interface name for families without a kind.
    variant: string;
    // The member whose variant this one shares (a subkind such as ParamSpecType), if any.
    sharesVariantOf: string | undefined;
    // The member whose details this one embeds as `base` (families without a kind field).
    parent: string | undefined;
}

export interface Prop {
    node: ts.Node;
    subst: Map<string, IR>;
}

export interface Site {
    file: string;
    key: string;
    count: number;
}

// An inline object type that needs a named struct.
export interface AnonStruct {
    name: string;
    file: string;
    node: ts.TypeLiteralNode | undefined;
    type: ts.Type | undefined;
    site: string;
}

export class Gen {
    readonly checker: ts.TypeChecker;
    readonly familyMembers = new Map<string, FamilyMember>();
    readonly familyByRootKey = new Map<string, Family>();
    readonly familyAliases = new Map<string, Family>();
    readonly factoryByIface = new Map<string, Factory>();
    readonly factoryByFunc = new Map<string, Factory>();
    readonly traitClasses = new Set<string>();
    readonly subclassesOf = new Map<string, string[]>();
    readonly unmapped = new Map<string, string[]>();
    readonly anon = new Map<string, AnonStruct>();
    // Declarations in other files that emitted code refers to: "<file>:<Name>".
    readonly needed = new Set<string>();
    // Pyright declarations by key.
    readonly decls = new Map<string, ts.Declaration[]>();
    // While probing a type's shape, map without recording sites, needs or anonymous structs.
    private quiet = 0;
    // Aliases being inlined, to stop a recursive alias.
    private readonly inlining = new Set<string>();
    // Type-parameter values for inferred types (`mapType`), set while mapping one signature.
    typeSubst: Map<string, IR> | undefined;
    // The heap the run's code allocates in.
    heap: 'program' | 'eval' = 'program';
    readonly callbacks: Callbacks;

    constructor(
        readonly loaded: Loaded,
        readonly table: Table,
        readonly numbers: NumberTable,
        readonly writes: Writes
    ) {
        this.checker = loaded.checker;
        this.callbacks = new Callbacks(this);
        this.indexDecls();
        this.indexFamilies();
        for (const f of table.factories) {
            if (f.iface) {
                this.factoryByIface.set(f.iface, f);
            }
            this.factoryByFunc.set(`${f.file}:${f.func}`, f);
        }
        for (const key of table.traits.keys()) {
            this.traitClasses.add(key);
        }
    }

    private indexDecls() {
        for (const sf of this.loaded.files) {
            const file = relPath(this.loaded, sf);
            const add = (name: string, d: ts.Declaration) => {
                const key = `${file}:${name}`;
                const list = this.decls.get(key) ?? [];
                list.push(d);
                this.decls.set(key, list);
            };
            for (const st of sf.statements) {
                if (
                    (ts.isInterfaceDeclaration(st) ||
                        ts.isClassDeclaration(st) ||
                        ts.isEnumDeclaration(st) ||
                        ts.isTypeAliasDeclaration(st) ||
                        ts.isFunctionDeclaration(st) ||
                        ts.isModuleDeclaration(st)) &&
                    st.name &&
                    ts.isIdentifier(st.name)
                ) {
                    add(st.name.text, st);
                }
                if (ts.isClassDeclaration(st) && st.name) {
                    for (const h of st.heritageClauses ?? []) {
                        if (h.token !== ts.SyntaxKind.ExtendsKeyword) {
                            continue;
                        }
                        for (const t of h.types) {
                            const base = this.keyOfSymbol(this.checker.getSymbolAtLocation(t.expression));
                            if (base) {
                                const list = this.subclassesOf.get(base) ?? [];
                                list.push(`${file}:${st.name.text}`);
                                this.subclassesOf.set(base, list);
                            }
                        }
                    }
                }
            }
        }
    }

    keyOfSymbol(sym: ts.Symbol | undefined): string | undefined {
        const s = resolveSymbol(this.checker, sym);
        const d = s?.declarations?.[0];
        if (!s || !d) {
            return undefined;
        }
        const sf = d.getSourceFile();
        if (sf.isDeclarationFile || !sf.fileName.startsWith(this.loaded.srcRoot)) {
            return undefined;
        }
        return `${relPath(this.loaded, sf)}:${s.getName()}`;
    }

    fileOfKey(key: string): string {
        return key.slice(0, key.indexOf(':'));
    }

    nameOfKey(key: string): string {
        return key.slice(key.indexOf(':') + 1);
    }

    // ---- families -------------------------------------------------------------------------

    private indexFamilies() {
        for (const fam of this.table.families) {
            this.familyByRootKey.set(`${fam.file}:${fam.base}`, fam);
            if (fam.union) {
                this.familyByRootKey.set(`${fam.file}:${fam.union}`, fam);
            }
        }
        // Members: interfaces whose heritage reaches the family base.
        for (const [key, decls] of this.decls) {
            const decl = decls.find(ts.isInterfaceDeclaration);
            if (!decl) {
                continue;
            }
            for (const fam of this.table.families) {
                const baseKey = `${fam.file}:${fam.base}`;
                if (key === baseKey || key === `${fam.file}:${fam.union}`) {
                    continue;
                }
                const chain = this.heritageChain(decl, baseKey);
                if (chain === undefined) {
                    continue;
                }
                const name = this.nameOfKey(key);
                let variant = name;
                let sharesVariantOf: string | undefined;
                const parentKey = chain[0];
                const parent = parentKey === baseKey || parentKey === `${fam.file}:${fam.union}` ? undefined : this.nameOfKey(parentKey);
                if (fam.kind) {
                    const kind = this.kindOf(decl, fam.kind);
                    if (kind === undefined) {
                        continue;
                    }
                    variant = kind;
                    if (parent) {
                        const parentDecl = this.decls.get(parentKey)?.find(ts.isInterfaceDeclaration);
                        if (parentDecl && this.kindOf(parentDecl, fam.kind) === kind) {
                            sharesVariantOf = parent;
                        }
                    }
                }
                this.familyMembers.set(key, {
                    fam,
                    name,
                    decl,
                    variant,
                    sharesVariantOf,
                    parent: fam.kind ? undefined : parent,
                });
            }
        }
        // Aliases of unions of members (ExpressionNode).
        for (const [key, decls] of this.decls) {
            const alias = decls.find(ts.isTypeAliasDeclaration);
            if (!alias || this.familyByRootKey.has(key)) {
                continue;
            }
            const fam = this.familyOfTypeNode(alias.type);
            if (fam) {
                this.familyAliases.set(key, fam);
            }
        }
    }

    // Keys of the interfaces between `decl` and the family base, nearest first.
    private heritageChain(decl: ts.InterfaceDeclaration, baseKey: string, depth = 0): string[] | undefined {
        if (depth > 10) {
            return undefined;
        }
        for (const h of decl.heritageClauses ?? []) {
            for (const t of h.types) {
                const k = this.keyOfSymbol(this.checker.getSymbolAtLocation(t.expression));
                if (!k) {
                    continue;
                }
                if (k === baseKey) {
                    return [k];
                }
                const fam = this.table.families.find((f) => `${f.file}:${f.base}` === baseKey);
                if (fam && k === `${fam.file}:${fam.union}`) {
                    return [k];
                }
                const next = this.decls.get(k)?.find(ts.isInterfaceDeclaration);
                if (next) {
                    const rest = this.heritageChain(next, baseKey, depth + 1);
                    if (rest) {
                        return [k, ...rest];
                    }
                }
            }
        }
        return undefined;
    }

    // The enum member name of a member's kind field, from its literal type.
    private kindOf(decl: ts.InterfaceDeclaration, field: string): string | undefined {
        const sym = this.checker.getSymbolAtLocation(decl.name);
        if (!sym) {
            return undefined;
        }
        const t = this.checker.getDeclaredTypeOfSymbol(sym);
        const prop = t.getProperty(field);
        if (!prop) {
            return undefined;
        }
        const pt = this.checker.getTypeOfSymbol(prop);
        if (pt.flags & ts.TypeFlags.EnumLiteral && !(pt.flags & ts.TypeFlags.Union)) {
            return pt.getSymbol()?.getName();
        }
        return undefined;
    }

    familyOfTypeNode(node: ts.TypeNode): Family | undefined {
        return this.familyOfParts(ts.isUnionTypeNode(node) ? node.types : [node]);
    }

    familyOfParts(parts: readonly ts.TypeNode[]): Family | undefined {
        let fam: Family | undefined;
        for (const p of parts) {
            if (isNullish(p)) {
                continue;
            }
            if (!ts.isTypeReferenceNode(p)) {
                return undefined;
            }
            const key = this.keyOfSymbol(this.checker.getSymbolAtLocation(p.typeName));
            if (!key) {
                return undefined;
            }
            const f = this.familyMembers.get(key)?.fam ?? this.familyByRootKey.get(key) ?? this.familyAliases.get(key);
            if (!f || (fam && fam !== f)) {
                return undefined;
            }
            fam = f;
        }
        return fam;
    }

    familyRootIR(fam: Family): IR {
        return { k: 'named', text: `P<${fam.name}>`, copy: true, uses: ['jsrt::P', `${crateModuleOf(fam.file)}::${fam.name}`] };
    }

    // ---- modes ----------------------------------------------------------------------------

    // docs/porting.md "memory": an object is a `P` only when pyright compares it with `===` (or
    // hashes it as a Map key or Set element), or writes it after storing it in a second place.
    // Otherwise a class is owned (an `Rc` when stored but never written) and an interface is a value.
    modeOf(key: string): Mode {
        const m = this.table.modes.get(key);
        if (m) {
            return m;
        }
        const decls = this.decls.get(key) ?? [];
        const cls = decls.find(ts.isClassDeclaration);
        if (cls && this.traitAncestors(key).length > 0) {
            return 'owned';
        }
        if (!cls && !decls.some(ts.isInterfaceDeclaration)) {
            return 'value';
        }
        const written = this.firstFieldWrite(key) !== undefined;
        const stored = this.storedAt(key) !== undefined;
        if (this.identityComparedAt(key) !== undefined || (written && stored)) {
            return 'handle';
        }
        if (cls) {
            return !written && stored ? 'rc' : 'owned';
        }
        return 'value';
    }

    // Where code stores an existing object of this type in a second place, if anywhere.
    storedAt(key: string): string | undefined {
        for (const d of this.decls.get(key) ?? []) {
            const at = this.writes.stored.get(d);
            if (at) {
                return at;
            }
        }
        return undefined;
    }

    // Why `modeOf` chose the mode it did, for the report.
    modeReason(key: string): string {
        if (this.table.modes.has(key)) {
            return '[modes]';
        }
        const parts: string[] = [];
        const id = this.identityComparedAt(key);
        if (id) {
            parts.push(`compared or hashed by identity at ${id}`);
        }
        const w = this.firstFieldWrite(key);
        parts.push(w ? `written: ${w}` : 'never written after construction');
        const st = this.storedAt(key);
        parts.push(st ? `stored at ${st}` : 'never stored in a second place');
        return parts.join('; ');
    }

    // Where code compares two objects of this type with `===`, if anywhere.
    identityComparedAt(key: string): string | undefined {
        for (const d of this.decls.get(key) ?? []) {
            const at = this.writes.identityCompared.get(d);
            if (at) {
                return at;
            }
        }
        return undefined;
    }

    // "<field> at <file>:<line>" for the first field of an interface written after construction.
    firstFieldWrite(key: string): string | undefined {
        for (const d of this.decls.get(key) ?? []) {
            if (!ts.isInterfaceDeclaration(d) && !ts.isClassDeclaration(d)) {
                continue;
            }
            const fields: ts.Node[] = [];
            for (const m of d.members) {
                if (ts.isPropertySignature(m) || (ts.isPropertyDeclaration(m) && !m.modifiers?.some((x) => x.kind === ts.SyntaxKind.StaticKeyword))) {
                    fields.push(m);
                } else if (ts.isConstructorDeclaration(m)) {
                    fields.push(...m.parameters.filter((p) => ts.isParameterPropertyDeclaration(p, m)));
                }
            }
            for (const m of fields) {
                if (this.writes.written.has(m) || this.writes.mutated.has(m)) {
                    return `${memberName(m)} at ${this.writes.where.get(m)}`;
                }
            }
        }
        return undefined;
    }

    // Trait classes this class extends, nearest first.
    traitAncestors(key: string): string[] {
        const out: string[] = [];
        let cur: string | undefined = key;
        for (let i = 0; i < 10 && cur; i++) {
            const cls: ts.ClassDeclaration | undefined = this.decls.get(cur)?.find(ts.isClassDeclaration);
            const ext = cls?.heritageClauses?.find((h) => h.token === ts.SyntaxKind.ExtendsKeyword)?.types[0];
            if (!ext) {
                break;
            }
            const baseKey = this.keyOfSymbol(this.checker.getSymbolAtLocation(ext.expression));
            if (!baseKey) {
                break;
            }
            if (this.traitClasses.has(baseKey)) {
                out.push(baseKey);
            }
            cur = baseKey;
        }
        return out;
    }

    // A [types] entry as IR. A template of exactly "$0" is its first argument (Promise<T> is T).
    private rawEntry(entry: { rust: string; param: string | undefined; copy: boolean; uses: string[] }, args: IR[]): IR {
        if (entry.rust === '$0' && args[0]) {
            return args[0];
        }
        return {
            k: 'raw',
            text: shortenPaths(subst(entry.rust, args)),
            param: entry.param ? shortenPaths(subst(entry.param, args)) : undefined,
            copy: entry.copy,
            uses: [...entry.uses, ...usesOf(entry.rust), ...(entry.param ? usesOf(entry.param) : []), ...args.flatMap(usesOfIR)],
        };
    }

    // ---- generics of structs ------------------------------------------------------------
    //
    // A type parameter with a mappable constraint is erased to that constraint, as in functions:
    // `interface TypeResult<T extends Type = Type>` is `struct TypeResult` with `type_: P<Type>`.
    // The rest stay Rust generics.

    structParams(decl: { typeParameters?: ts.NodeArray<ts.TypeParameterDeclaration> }): { kept: string[]; erased: Map<string, IR>; keptIndex: number[]; defaults: (ts.TypeNode | undefined)[] } {
        const kept: string[] = [];
        const keptIndex: number[] = [];
        const defaults: (ts.TypeNode | undefined)[] = [];
        const erased = new Map<string, IR>();
        (decl.typeParameters ?? []).forEach((tp, i) => {
            if (tp.constraint) {
                this.quiet++;
                try {
                    const c = this.mapNode(tp.constraint, { file: '', key: '', count: 0 }, new Map());
                    if (c.k !== 'unmapped' && isCopy(c)) {
                        erased.set(tp.name.text, c);
                        return;
                    }
                } finally {
                    this.quiet--;
                }
            }
            kept.push(tp.name.text);
            keptIndex.push(i);
            defaults.push(tp.default);
        });
        return { kept, erased, keptIndex, defaults };
    }

    // ---- sites and numbers ----------------------------------------------------------------

    // `role` is 'key' for the key of a Map or the element of a Set: pyright keys those by node,
    // flow node or type ids.
    numberAt(site: Site, role: 'value' | 'key' = 'value'): IR {
        if (this.quiet > 0) {
            return { k: 'num', rust: 'f64' };
        }
        const key = site.count === 0 ? site.key : `${site.key}#${site.count}`;
        site.count++;
        const override = this.table.sites.get(`${site.file}:${key}`);
        if (override) {
            return { k: 'num', rust: override };
        }
        const rust = this.numbers.lookup(site.file, key, () => (role === 'key' ? ['u32', 'Map or Set key: an id'] : guessNumber(key)));
        return { k: 'num', rust };
    }

    // ---- mapping --------------------------------------------------------------------------

    // Maps a declared type, or the checker's inferred type when there is no annotation.
    // A [sites] entry: the Rust type, verbatim in every position. Qualified paths in it
    // (`jsrt::P`, `std::cell::RefCell`) become imports.
    siteOverride(site: Site): IR | undefined {
        const override = this.table.sites.get(`${site.file}:${site.key}`);
        if (override === undefined) {
            return undefined;
        }
        // `()` drops a return value no caller reads.
        if (override === '()') {
            return { k: 'unit' };
        }
        const text = shortenPaths(override);
        // Handles and options of handles are Copy, so a written field holding one is a Cell.
        const copy = /^(Option<)?P</.test(text);
        return { k: 'raw', text, param: text, copy, uses: usesOf(override) };
    }

    mapDecl(typeNode: ts.TypeNode | undefined, node: ts.Node, site: Site, subst?: Map<string, IR>): IR {
        const override = this.siteOverride(site);
        if (override) {
            return override;
        }
        if (typeNode) {
            return this.mapNode(typeNode, site, subst ?? new Map());
        }
        return this.mapType(this.checker.getTypeAtLocation(node), site, 0);
    }

    mapNode(node: ts.TypeNode, site: Site, subst: Map<string, IR>): IR {
        switch (node.kind) {
            case ts.SyntaxKind.StringKeyword:
                return STR;
            case ts.SyntaxKind.NumberKeyword:
                return this.numberAt(site);
            case ts.SyntaxKind.BooleanKeyword:
                return BOOL;
            case ts.SyntaxKind.VoidKeyword:
            case ts.SyntaxKind.UndefinedKeyword:
            case ts.SyntaxKind.NullKeyword:
                return UNIT;
            case ts.SyntaxKind.NeverKeyword:
                return { k: 'never' };
            case ts.SyntaxKind.BigIntKeyword:
                return this.builtin('bigint', [], node);
            case ts.SyntaxKind.AnyKeyword:
            case ts.SyntaxKind.UnknownKeyword:
            case ts.SyntaxKind.ObjectKeyword:
            case ts.SyntaxKind.SymbolKeyword:
                return this.unmappedAt(node.getText(), undefined, site);
        }
        if (ts.isLiteralTypeNode(node)) {
            const lit = node.literal;
            if (lit.kind === ts.SyntaxKind.NullKeyword) {
                return UNIT;
            }
            if (lit.kind === ts.SyntaxKind.TrueKeyword || lit.kind === ts.SyntaxKind.FalseKeyword) {
                return BOOL;
            }
            if (ts.isStringLiteral(lit)) {
                return STR;
            }
            return this.numberAt(site);
        }
        if (ts.isParenthesizedTypeNode(node)) {
            return this.mapNode(node.type, site, subst);
        }
        if (ts.isTypeOperatorNode(node)) {
            if (node.operator === ts.SyntaxKind.ReadonlyKeyword) {
                return this.mapNode(node.type, site, subst);
            }
            return this.mapType(this.checker.getTypeFromTypeNode(node), site, 0);
        }
        if (ts.isArrayTypeNode(node)) {
            return { k: 'vec', elem: this.mapNode(node.elementType, site, subst) };
        }
        if (ts.isTupleTypeNode(node)) {
            return {
                k: 'tuple',
                elems: node.elements.map((e) => {
                    if (ts.isNamedTupleMember(e)) {
                        const inner = this.mapNode(e.type, site, subst);
                        return e.questionToken ? opt(inner) : inner;
                    }
                    if (ts.isOptionalTypeNode(e)) {
                        return opt(this.mapNode(e.type, site, subst));
                    }
                    if (ts.isRestTypeNode(e)) {
                        return this.mapNode(e.type, site, subst);
                    }
                    return this.mapNode(e, site, subst);
                }),
            };
        }
        if (ts.isUnionTypeNode(node)) {
            return this.mapUnion(node, site, subst);
        }
        if (ts.isFunctionTypeNode(node)) {
            return this.mapSignatureNode(node.parameters, node.type, site, subst);
        }
        if (ts.isTypeLiteralNode(node)) {
            const index = node.members.find(ts.isIndexSignatureDeclaration);
            if (index && node.members.length === 1) {
                return { k: 'map', key: this.mapNode(index.parameters[0].type ?? node, site, subst), val: this.mapNode(index.type, site, subst) };
            }
            const call = node.members.find(ts.isCallSignatureDeclaration);
            if (call && node.members.length === 1) {
                return this.mapSignatureNode(call.parameters, call.type, site, subst);
            }
            return this.anonStruct(site, node, undefined);
        }
        if (ts.isTypePredicateNode(node)) {
            return BOOL;
        }
        if (ts.isThisTypeNode(node)) {
            return this.mapType(this.checker.getTypeAtLocation(node), site, 0);
        }
        if (ts.isTypeReferenceNode(node)) {
            return this.mapReference(node, site, subst);
        }
        if (ts.isConditionalTypeNode(node)) {
            // `T extends X ? T : never` narrows T: map the true branch.
            return this.unmappedAt(node.getText(), this.mapNode(node.trueType, site, subst), site);
        }
        if (ts.isIntersectionTypeNode(node)) {
            // `A & { kind: X }` narrows A: map A.
            return this.mapNode(node.types[0], site, subst);
        }
        return this.mapType(this.checker.getTypeFromTypeNode(node), site, 0);
    }

    private mapSignatureNode(
        params: readonly ts.ParameterDeclaration[],
        ret: ts.TypeNode | undefined,
        site: Site,
        subst: Map<string, IR>
    ): IR {
        return {
            k: 'fn',
            params: params.map((p) => {
                const t = p.type ? this.mapNode(p.type, site, subst) : this.mapType(this.checker.getTypeAtLocation(p), site, 0);
                if (p.dotDotDotToken) {
                    return { k: 'vec', elem: t.k === 'vec' ? t.elem : t };
                }
                return p.questionToken ? opt(t) : t;
            }),
            ret: ret ? this.mapNode(ret, site, subst) : UNIT,
        };
    }

    private mapUnion(node: ts.UnionTypeNode, site: Site, subst: Map<string, IR>): IR {
        const parts = node.types.filter((t) => !isNullish(t));
        const optional = parts.length !== node.types.length;
        const wrap = (t: IR) => (optional ? opt(t) : t);
        // A union named in [types] by its members, sorted: "bigint | number".
        const unionKey = parts.map((p) => p.getText().replace(/\s+/g, ' ')).sort().join(' | ');
        const entry = this.table.types.get(unionKey);
        if (entry) {
            return wrap(this.rawEntry(entry, []));
        }
        if (parts.length === 0) {
            return UNIT;
        }
        if (parts.length === 1) {
            return wrap(this.mapNode(parts[0], site, subst));
        }
        if (parts.every((p) => ts.isLiteralTypeNode(p) && (p.literal.kind === ts.SyntaxKind.TrueKeyword || p.literal.kind === ts.SyntaxKind.FalseKeyword))) {
            return wrap(BOOL);
        }
        if (parts.every((p) => ts.isLiteralTypeNode(p) && ts.isStringLiteral(p.literal))) {
            return wrap(STR);
        }
        // String literals mixed with string-literal aliases (`StructElementType | 'pad'`): a String.
        const t0 = this.checker.getTypeFromTypeNode(node);
        if (t0.isUnion() && t0.types.every((m) => m.flags & (ts.TypeFlags.StringLiteral | ts.TypeFlags.Undefined | ts.TypeFlags.Null))) {
            return wrap(STR);
        }
        // `-1 | 0 | 1`: one number site.
        if (parts.every((p) => ts.isLiteralTypeNode(p) && (ts.isNumericLiteral(p.literal) || ts.isPrefixUnaryExpression(p.literal)))) {
            return wrap(this.numberAt(site));
        }
        const fam = this.familyOfParts(parts);
        if (fam) {
            return wrap(this.familyRootIR(fam));
        }
        // A union of members of one enum is the enum.
        const t = this.checker.getTypeFromTypeNode(node);
        const enumSym = this.enumOfUnion(t);
        if (enumSym) {
            return wrap(this.mapType(t, site, 0));
        }
        const mapped = parts.map((p) => this.mapNode(p, site, subst));
        if (mapped.every((m) => sameIR(m, mapped[0]))) {
            return wrap(mapped[0]);
        }
        return wrap(this.unmappedAt(node.getText(), mapped[0], site));
    }

    private enumOfUnion(t: ts.Type): ts.Symbol | undefined {
        if (!t.isUnion()) {
            return undefined;
        }
        let sym: ts.Symbol | undefined;
        for (const m of t.types) {
            if (m.flags & (ts.TypeFlags.Undefined | ts.TypeFlags.Null)) {
                continue;
            }
            if (!(m.flags & ts.TypeFlags.EnumLiteral)) {
                return undefined;
            }
            const parent = enumParent(m);
            if (!parent || (sym && sym !== parent)) {
                return undefined;
            }
            sym = parent;
        }
        return sym;
    }

    private mapReference(node: ts.TypeReferenceNode, site: Site, subst: Map<string, IR>): IR {
        const raw = this.checker.getSymbolAtLocation(node.typeName);
        const sym = resolveSymbol(this.checker, raw);
        const targetKey = this.keyOfSymbol(sym);
        const ignoresArgs = targetKey !== undefined && (this.familyByRootKey.has(targetKey) || this.familyMembers.has(targetKey));
        if (ignoresArgs) {
            this.quiet++;
        }
        let args: IR[];
        const keyed = sym !== undefined && ['Map', 'ReadonlyMap', 'Set', 'ReadonlySet', 'WeakMap', 'WeakSet'].includes(sym.getName());
        try {
            args = (node.typeArguments ?? []).map((a, i) =>
                keyed && i === 0 && a.kind === ts.SyntaxKind.NumberKeyword ? this.numberAt(site, 'key') : this.mapNode(a, site, subst)
            );
        } finally {
            if (ignoresArgs) {
                this.quiet--;
            }
        }
        if (!sym) {
            return this.unmappedAt(node.getText(), undefined, site);
        }
        if (sym.flags & ts.SymbolFlags.TypeParameter) {
            const name = sym.getName();
            const s = subst.get(name);
            if (s) {
                return s;
            }
            return this.mapTypeParam(sym, site);
        }
        if (sym.flags & ts.SymbolFlags.EnumMember) {
            const parent = enumParent(this.checker.getTypeFromTypeNode(node));
            if (parent) {
                return this.mapSymbol(parent, [], node, site);
            }
        }
        return this.mapSymbol(sym, args, node, site);
    }

    // Generic parameters are replaced by their constraint when the constraint is a pyright type
    // (`T extends Type` becomes `P<Type>`), and kept as Rust generics otherwise.
    private mapTypeParam(sym: ts.Symbol, site: Site): IR {
        const decl = sym.declarations?.find(ts.isTypeParameterDeclaration);
        // A type parameter of a function type (`<T>(func: T) => ..`) has no Rust generic to bind
        // to: use its constraint.
        const bindable = decl !== undefined && (ts.isFunctionLike(decl.parent) ? !ts.isFunctionTypeNode(decl.parent) && !ts.isCallSignatureDeclaration(decl.parent) : true);
        if (decl !== undefined && bindable && this.boundOf(decl) !== undefined) {
            return { k: 'generic', name: sym.getName() };
        }
        if (decl?.constraint) {
            // Only a handle constraint can stand for its subtypes: a caller casts the handle back.
            const c = this.mapNode(decl.constraint, site, new Map());
            if (c.k !== 'unmapped' && (isCopy(c) || !bindable)) {
                return c;
            }
        }
        if (!bindable) {
            return this.unmappedAt(`unbound type parameter ${sym.getName()}`, undefined, site);
        }
        return { k: 'generic', name: sym.getName() };
    }

    // The `[bounds]` entry of a function's type parameter: `<file>:<function>.<T>`.
    boundOf(tp: ts.TypeParameterDeclaration): string | undefined {
        const fn = tp.parent;
        if (!ts.isFunctionDeclaration(fn) || !fn.name) {
            return undefined;
        }
        return this.table.bounds.get(`${relPath(this.loaded, fn.getSourceFile())}:${fn.name.text}.${tp.name.text}`);
    }

    builtin(name: string, args: IR[], node: ts.Node | undefined, site?: Site): IR {
        const entry = this.table.types.get(name);
        if (entry) {
            return this.rawEntry(entry, args);
        }
        switch (name) {
            case 'Generator':
            case 'IterableIterator': {
                // A TS generator runs lazily, interleaved with its caller: an iterator, never a Vec.
                const scratch = { set: new Set<string>(), add(p: readonly string[]) { for (const x of p) { this.set.add(x); } } };
                const elem = renderIR(args[0] ?? UNIT, 'elem', scratch);
                return { k: 'raw', text: `Box<dyn Iterator<Item = ${elem}>>`, param: `&mut dyn Iterator<Item = ${elem}>`, copy: false, uses: [...scratch.set] };
            }
            case 'Array':
            case 'ReadonlyArray':
            case 'Iterable':
            case 'Iterator':
                return { k: 'vec', elem: args[0] ?? UNIT };
            case 'Map':
            case 'ReadonlyMap':
            case 'WeakMap':
                return { k: 'map', key: args[0] ?? UNIT, val: args[1] ?? UNIT };
            case 'Record':
                return { k: 'map', key: args[0] ?? STR, val: args[1] ?? UNIT };
            case 'Set':
            case 'ReadonlySet':
            case 'WeakSet':
                return { k: 'set', elem: args[0] ?? UNIT };
            case 'Readonly':
            case 'Required':
            case 'NonNullable':
                return args[0] ?? UNIT;
            case 'Partial': {
                // `Partial<X>`: a struct `PartialX` beside X with every field optional.
                const arg = node && ts.isTypeReferenceNode(node) ? node.typeArguments?.[0] : undefined;
                const argKey = arg && ts.isTypeReferenceNode(arg) ? this.keyOfSymbol(this.checker.getSymbolAtLocation(arg.typeName)) : undefined;
                if (node && ts.isTypeNode(node) && argKey) {
                    const file = this.fileOfKey(argKey);
                    return this.anonStruct({ file, key: `Partial${this.nameOfKey(argKey)}`, count: 0 }, undefined, this.checker.getTypeFromTypeNode(node));
                }
                break;
            }
        }
        if (site && node && ts.isTypeNode(node) && ['ReturnType', 'Parameters', 'Awaited', 'InstanceType', 'Exclude', 'Extract', 'Omit', 'Pick'].includes(name)) {
            return this.mapType(this.checker.getTypeFromTypeNode(node), site, 1);
        }
        return this.unmappedAt(node ? node.getText() : name, undefined, site);
    }

    unmappedAt(text: string, fallback: IR | undefined, site: Site | undefined): IR {
        const label = text.replace(/\s+/g, ' ').slice(0, 80);
        if (this.quiet === 0) {
            const at = site ? `${site.file}:${site.key}` : '';
            const list = this.unmapped.get(label) ?? [];
            list.push(at);
            this.unmapped.set(label, list);
        }
        return { k: 'unmapped', ts: label, fallback };
    }

    // A named declaration: pyright's own, or a library or package type.
    mapSymbol(sym: ts.Symbol, args: IR[], node: ts.Node | undefined, site: Site): IR {
        const decl = sym.declarations?.[0];
        const sf = decl?.getSourceFile();
        const name = sym.getName();
        if (!decl || !sf || sf.isDeclarationFile || !sf.fileName.startsWith(this.loaded.srcRoot)) {
            // Library or package type: the table, then the built-in rules.
            return this.builtin(name, args, node, site);
        }
        if (ts.isTypeAliasDeclaration(decl) && !ts.isSourceFile(decl.parent)) {
            // A type alias declared inside a function body: inline it.
            return this.mapNode(decl.type, site, new Map());
        }
        const key = `${relPath(this.loaded, sf)}:${name}`;
        return this.mapKey(key, args, node, site);
    }

    mapKey(key: string, argsIn: IR[], node: ts.Node | undefined, site: Site): IR {
        let args = argsIn;
        const file = this.fileOfKey(key);
        const name = this.nameOfKey(key);
        const path = `${crateModuleOf(file)}::${name}`;
        const entry = this.table.types.get(key);
        if (entry) {
            if (entry.uses.some((u) => u.startsWith('crate::'))) {
                this.need(key);
            }
            return this.rawEntry(entry, args);
        }
        const merged = this.table.merge.get(key);
        if (merged) {
            return this.mapKey(merged.includes(':') ? merged : `${file}:${merged}`, args, node, site);
        }
        const factory = this.factoryByIface.get(key);
        if (factory) {
            this.need(`${factory.file}:${factory.name}`);
            return { k: 'factory', name: factory.name, uses: [`${crateModuleOf(factory.file)}::${factory.name}`] };
        }
        const famRoot = this.familyByRootKey.get(key);
        if (famRoot) {
            this.need(`${famRoot.file}:${famRoot.base}`);
            return this.familyRootIR(famRoot);
        }
        const member = this.familyMembers.get(key);
        if (member) {
            if (!member.fam.typedHandles) {
                this.need(`${member.fam.file}:${member.fam.base}`);
                return this.familyRootIR(member.fam);
            }
            this.need(key);
            return { k: 'named', text: name, copy: true, uses: [path] };
        }
        if (this.familyAliases.has(key)) {
            this.need(key);
            return { k: 'named', text: name, copy: true, uses: [path] };
        }
        const decls = this.decls.get(key) ?? [];
        // Erased type parameters take no argument. An inlined alias keeps all of them: they are
        // substituted into its body.
        const shaped = decls.find((d) => ts.isInterfaceDeclaration(d) || ts.isClassDeclaration(d) || ts.isTypeAliasDeclaration(d));
        const inlined = shaped !== undefined && ts.isTypeAliasDeclaration(shaped) && this.aliasShape(shaped) === 'inline';
        if (shaped && !inlined && (ts.isInterfaceDeclaration(shaped) || ts.isClassDeclaration(shaped) || ts.isTypeAliasDeclaration(shaped))) {
            const sp = this.structParams(shaped);
            args = sp.keptIndex.map((i, j) => {
                const a = args[i];
                if (a) {
                    return a;
                }
                const dflt = sp.defaults[j];
                return dflt ? this.mapNode(dflt, site, new Map()) : UNIT;
            });
        }
        const generic = args.length > 0 ? `<${args.map((a) => renderElem(a)).join(', ')}>` : '';
        const argUses = args.flatMap(usesOfIR);
        if (decls.some(ts.isEnumDeclaration)) {
            this.need(key);
            return { k: 'named', text: name, copy: true, uses: [path] };
        }
        if (this.traitClasses.has(key)) {
            this.need(key);
            const t = this.table.traits.get(key);
            return { k: 'trait', text: name + generic, mutRecv: t?.receiver === '&mut self', uses: [path, ...argUses] };
        }
        const alias = decls.find(ts.isTypeAliasDeclaration);
        if (alias) {
            const aliasShape = this.aliasShape(alias);
            if (aliasShape === 'inline') {
                if (this.inlining.has(key)) {
                    return this.unmappedAt(`recursive alias ${name}`, undefined, site);
                }
                const sub = new Map<string, IR>();
                (alias.typeParameters ?? []).forEach((tp, i) => sub.set(tp.name.text, args[i] ?? UNIT));
                // An alias is mapped at its own site, so one numbers.toml entry covers every use.
                this.inlining.add(key);
                try {
                    return this.mapNode(alias.type, { file, key: name, count: 0 }, sub);
                } finally {
                    this.inlining.delete(key);
                }
            }
            this.need(key);
            const copy = aliasShape === 'enum' ? this.aliasEnumCopy(alias) : false;
            return { k: 'named', text: name + generic, copy, uses: [path, ...argUses] };
        }
        if (decls.some((d) => ts.isInterfaceDeclaration(d) || ts.isClassDeclaration(d))) {
            this.need(key);
            const mode = this.modeOf(key);
            if (mode === 'handle') {
                return { k: 'named', text: `P<${name}${generic}>`, copy: true, uses: ['jsrt::P', path, ...argUses] };
            }
            if (mode === 'rc') {
                // Shared and never written: an `Rc`, read through `&T`.
                return { k: 'raw', text: `Rc<${name}${generic}>`, param: `&${name}${generic}`, copy: false, uses: ['std::rc::Rc', path, ...argUses] };
            }
            return { k: 'named', text: name + generic, copy: false, uses: [path, ...argUses] };
        }
        return this.unmappedAt(node ? node.getText() : key, undefined, site);
    }

    // How a type alias is emitted and referenced:
    // - 'family': a union of family members, `pub type X = P<Root>`;
    // - 'enum': a union of unrelated types or string literals, a Rust enum;
    // - 'fn': a function type, `pub type X = Rc<dyn Fn(..)>`;
    // - 'struct': an object type, a struct;
    // - 'inline': anything else; uses are replaced by the aliased type.
    aliasShape(alias: ts.TypeAliasDeclaration): 'family' | 'enum' | 'fn' | 'struct' | 'inline' {
        const key = this.keyOfSymbol(this.checker.getSymbolAtLocation(alias.name));
        if (key && (this.familyAliases.has(key) || this.familyByRootKey.has(key))) {
            return 'family';
        }
        const t = alias.type;
        if (ts.isFunctionTypeNode(t)) {
            return 'fn';
        }
        if (ts.isTypeLiteralNode(t) && !t.members.some(ts.isIndexSignatureDeclaration) && !t.members.some(ts.isCallSignatureDeclaration)) {
            return 'struct';
        }
        if (ts.isUnionTypeNode(t)) {
            const parts = t.types.filter((p) => !isNullish(p));
            if (parts.length < 2) {
                return 'inline';
            }
            if (parts.every((p) => ts.isLiteralTypeNode(p) && ts.isStringLiteral(p.literal))) {
                return 'enum';
            }
            if (this.enumOfUnion(this.checker.getTypeFromTypeNode(t))) {
                return 'inline';
            }
            const site: Site = { file: '', key: '', count: 0 };
            const mapped = parts.map((p) => this.mapNodeQuiet(p, site));
            if (mapped.every((m) => sameIR(m, mapped[0]))) {
                return 'inline';
            }
            return 'enum';
        }
        return 'inline';
    }

    private mapNodeQuiet(node: ts.TypeNode, site: Site): IR {
        this.quiet++;
        try {
            return this.mapNode(node, site, new Map());
        } finally {
            this.quiet--;
        }
    }

    private aliasEnumCopy(alias: ts.TypeAliasDeclaration): boolean {
        const t = alias.type;
        if (!ts.isUnionTypeNode(t)) {
            return false;
        }
        const parts = t.types.filter((p) => !isNullish(p));
        if (parts.every((p) => ts.isLiteralTypeNode(p) && ts.isStringLiteral(p.literal))) {
            return true;
        }
        return false;
    }

    // ---- inferred types -------------------------------------------------------------------

    mapType(t: ts.Type, site: Site, depth: number): IR {
        if (depth > 8) {
            return this.unmappedAt(this.checker.typeToString(t), undefined, site);
        }
        const f = t.flags;
        if (t.aliasSymbol && !(f & ts.TypeFlags.Boolean)) {
            const key = this.keyOfSymbol(t.aliasSymbol);
            if (key) {
                const args = (t.aliasTypeArguments ?? []).map((a) => this.mapType(a, site, depth + 1));
                return this.mapKey(key, args, undefined, site);
            }
        }
        if (f & (ts.TypeFlags.String | ts.TypeFlags.StringLiteral | ts.TypeFlags.TemplateLiteral)) {
            return STR;
        }
        if (f & (ts.TypeFlags.Boolean | ts.TypeFlags.BooleanLiteral)) {
            return BOOL;
        }
        if (f & ts.TypeFlags.EnumLiteral || f & ts.TypeFlags.Enum) {
            const parent = enumParent(t);
            if (parent) {
                return this.mapSymbol(parent, [], undefined, site);
            }
        }
        if (f & (ts.TypeFlags.Number | ts.TypeFlags.NumberLiteral)) {
            return this.numberAt(site);
        }
        if (f & (ts.TypeFlags.BigInt | ts.TypeFlags.BigIntLiteral)) {
            return this.builtin('bigint', [], undefined);
        }
        if (f & (ts.TypeFlags.Void | ts.TypeFlags.Undefined | ts.TypeFlags.Null)) {
            return UNIT;
        }
        if (f & ts.TypeFlags.Never) {
            return { k: 'never' };
        }
        if (f & (ts.TypeFlags.Any | ts.TypeFlags.Unknown | ts.TypeFlags.NonPrimitive | ts.TypeFlags.ESSymbol)) {
            return this.unmappedAt(this.checker.typeToString(t), undefined, site);
        }
        if (t.isUnion()) {
            const parts = t.types.filter((m) => !(m.flags & (ts.TypeFlags.Undefined | ts.TypeFlags.Null | ts.TypeFlags.Void)));
            const optional = parts.length !== t.types.length;
            const wrap = (x: IR) => (optional ? opt(x) : x);
            if (parts.length === 0) {
                return UNIT;
            }
            if (parts.every((m) => m.flags & ts.TypeFlags.BooleanLiteral)) {
                return wrap(BOOL);
            }
            // `-1 | 0 | 1`: one number site.
            if (parts.every((m) => m.flags & ts.TypeFlags.NumberLiteral && !(m.flags & ts.TypeFlags.EnumLiteral))) {
                return wrap(this.numberAt(site));
            }
            const enumSym = this.enumOfUnion(t);
            if (enumSym) {
                return wrap(this.mapSymbol(enumSym, [], undefined, site));
            }
            const mapped = parts.map((m) => this.mapType(m, site, depth + 1));
            // Union of family members: the family root.
            const fams = new Set(parts.map((m) => this.familyOfType(m)));
            if (fams.size === 1) {
                const fam = [...fams][0];
                if (fam) {
                    return wrap(this.familyRootIR(fam));
                }
            }
            if (mapped.every((m) => sameIR(m, mapped[0]))) {
                return wrap(mapped[0]);
            }
            return wrap(this.unmappedAt(this.checker.typeToString(t), mapped[0], site));
        }
        if (t.flags & ts.TypeFlags.TypeParameter) {
            const sym = t.getSymbol();
            const bound = sym ? this.typeSubst?.get(sym.getName()) : undefined;
            if (bound) {
                return bound;
            }
            return sym ? this.mapTypeParam(sym, site) : { k: 'generic', name: 'T' };
        }
        if (t.isIntersection()) {
            return this.mapType(t.types[0], site, depth + 1);
        }
        if (this.checker.isArrayType(t)) {
            const [elem] = this.checker.getTypeArguments(asRef(t));
            return { k: 'vec', elem: elem ? this.mapType(elem, site, depth + 1) : UNIT };
        }
        if (this.checker.isTupleType(t)) {
            return { k: 'tuple', elems: this.checker.getTypeArguments(asRef(t)).map((e) => this.mapType(e, site, depth + 1)) };
        }
        const sym = t.getSymbol();
        if (sym) {
            const decl = sym.declarations?.[0];
            const named = decl && (ts.isInterfaceDeclaration(decl) || ts.isClassDeclaration(decl) || ts.isEnumDeclaration(decl));
            if (named) {
                const keyed = ['Map', 'ReadonlyMap', 'Set', 'ReadonlySet', 'WeakMap', 'WeakSet'].includes(sym.getName());
                // A generator's return and next types are not mapped: only what it yields.
                const used = sym.getName() === 'Generator' ? 1 : undefined;
                const args = isRef(t)
                    ? this.checker.getTypeArguments(t).slice(0, used).map((a, i) =>
                          keyed && i === 0 && a.flags & ts.TypeFlags.Number ? this.numberAt(site, 'key') : this.mapType(a, site, depth + 1)
                      )
                    : [];
                return this.mapSymbol(sym, args, undefined, site);
            }
        }
        const sigs = t.getCallSignatures();
        if (sigs.length > 0 && t.getProperties().length === 0) {
            const sig = sigs[0];
            return {
                k: 'fn',
                params: sig.getParameters().map((p) => this.mapType(this.checker.getTypeOfSymbol(p), site, depth + 1)),
                ret: this.mapType(sig.getReturnType(), site, depth + 1),
            };
        }
        if (t.flags & ts.TypeFlags.Object) {
            return this.anonStruct(site, undefined, t);
        }
        return this.unmappedAt(this.checker.typeToString(t), undefined, site);
    }

    private familyOfType(t: ts.Type): Family | undefined {
        const key = this.keyOfSymbol(t.aliasSymbol ?? t.getSymbol());
        if (!key) {
            return undefined;
        }
        return this.familyMembers.get(key)?.fam ?? this.familyByRootKey.get(key) ?? this.familyAliases.get(key);
    }

    // Inline object types get a struct named after their site: `Binder._bindNameToScope.result`
    // becomes `BinderBindNameToScopeResult`.
    anonStruct(site: Site, node: ts.TypeLiteralNode | undefined, type: ts.Type | undefined): IR {
        const name = pascal(site.key.replace(/#.*/, '')) || 'Anon';
        const key = `${site.file}:${name}`;
        if (this.quiet === 0 && !this.anon.has(key)) {
            this.anon.set(key, { name, file: site.file, node, type, site: site.key });
        }
        this.need(key);
        return { k: 'named', text: name, copy: false, uses: [`${crateModuleOf(site.file)}::${name}`] };
    }

    // Runs `f` without recording sites, needs or anonymous structs.
    quietly<T>(f: () => T): T {
        this.quiet++;
        try {
            return f();
        } finally {
            this.quiet--;
        }
    }

    need(key: string) {
        if (this.quiet === 0) {
            this.needed.add(key);
        }
    }

    // The interface (or class) properties of a declaration in source order, bases first. Each
    // comes with the substitution for the base's type parameters: in `class ParseTreeWalker
    // extends ParseTreeVisitor<boolean>`, the visitor's `_default: T` is a `bool`.
    collectProps(decl: ts.InterfaceDeclaration | ts.ClassDeclaration, subst: Map<string, IR> = new Map()): Prop[] {
        const out: Prop[] = [];
        for (const h of decl.heritageClauses ?? []) {
            if (h.token !== ts.SyntaxKind.ExtendsKeyword) {
                continue;
            }
            for (const t of h.types) {
                const key = this.keyOfSymbol(this.checker.getSymbolAtLocation(t.expression));
                const base = key ? this.decls.get(key)?.find((d) => ts.isInterfaceDeclaration(d) || ts.isClassDeclaration(d)) : undefined;
                if (base && (ts.isInterfaceDeclaration(base) || ts.isClassDeclaration(base))) {
                    const sub = new Map<string, IR>();
                    const args = t.typeArguments ?? [];
                    (base.typeParameters ?? []).forEach((tp, i) => {
                        const a = args[i];
                        if (a) {
                            this.quiet++;
                            try {
                                sub.set(tp.name.text, this.mapNode(a, { file: '', key: '', count: 0 }, subst));
                            } finally {
                                this.quiet--;
                            }
                        }
                    });
                    for (const p of this.collectProps(base, sub)) {
                        out.push(p);
                    }
                }
            }
        }
        const own: Prop[] = [];
        for (const m of decl.members) {
            if (ts.isPropertySignature(m) || ts.isPropertyDeclaration(m)) {
                if (ts.isPropertyDeclaration(m) && m.modifiers?.some((x) => x.kind === ts.SyntaxKind.StaticKeyword)) {
                    continue;
                }
                own.push({ node: m, subst });
            } else if (ts.isConstructorDeclaration(m)) {
                for (const p of m.parameters) {
                    if (ts.isParameterPropertyDeclaration(p, m)) {
                        own.push({ node: p, subst });
                    }
                }
            }
        }
        for (const m of own) {
            const n = memberName(m.node);
            if (n === undefined) {
                continue;
            }
            const idx = out.findIndex((p) => memberName(p.node) === n);
            if (idx >= 0) {
                out[idx] = m;
            } else {
                out.push(m);
            }
        }
        return out;
    }

    // Type-parameter substitutions for every class or interface `decl` extends or implements,
    // keyed by the ancestor: `Binder` gets `ParseTreeVisitor -> {T: bool}`.
    ancestorSubsts(decl: ts.ClassDeclaration | ts.InterfaceDeclaration, subst: Map<string, IR> = new Map(), out = new Map<string, Map<string, IR>>()): Map<string, Map<string, IR>> {
        for (const h of decl.heritageClauses ?? []) {
            for (const t of h.types) {
                const key = this.keyOfSymbol(this.checker.getSymbolAtLocation(t.expression));
                const base = key ? this.decls.get(key)?.find((d) => ts.isInterfaceDeclaration(d) || ts.isClassDeclaration(d)) : undefined;
                if (!key || !base || !(ts.isInterfaceDeclaration(base) || ts.isClassDeclaration(base)) || out.has(key)) {
                    continue;
                }
                const sub = new Map<string, IR>();
                const args = t.typeArguments ?? [];
                (base.typeParameters ?? []).forEach((tp, i) => {
                    const a = args[i];
                    if (a) {
                        this.quiet++;
                        try {
                            sub.set(tp.name.text, this.mapNode(a, { file: '', key: '', count: 0 }, subst));
                        } finally {
                            this.quiet--;
                        }
                    }
                });
                out.set(key, sub);
                this.ancestorSubsts(base, sub, out);
            }
        }
        return out;
    }

    // The function declaration a closure factory entry names (`getCodeFlowEngine.createCodeFlowAnalyzer`).
    factoryNode(f: Factory): ts.FunctionDeclaration | undefined {
        const sf = this.loaded.files.find((x) => relPath(this.loaded, x) === f.file);
        let fn: ts.FunctionDeclaration | undefined;
        let stmts: readonly ts.Statement[] = sf?.statements ?? [];
        for (const part of f.func.split('.')) {
            fn = stmts.find((x): x is ts.FunctionDeclaration => ts.isFunctionDeclaration(x) && x.name?.text === part);
            stmts = fn?.body?.statements ?? [];
        }
        return fn;
    }

    // A stored closure factory struct: `P<TypeEvaluator>`.
    factoryIR(name: string): IR {
        const f = this.table.factories.find((x) => x.name === name);
        return { k: 'named', text: `P<${name}>`, copy: true, uses: ['jsrt::P', `${crateModuleOf(f?.file ?? '')}::${name}`] };
    }

    // A stored function type becomes a callback struct or a `fn` pointer (callbacks.ts). Function
    // types inside options, vectors and tuples count; a [sites] entry wins.
    storedCallbacks(t: IR, slot: ts.Node, file: string, slotName: string): IR {
        switch (t.k) {
            case 'fn':
                return this.callbacks.callbackIR(slot, t, file, slotName);
            case 'opt':
                return { k: 'opt', inner: this.storedCallbacks(t.inner, slot, file, slotName) };
            case 'vec':
                return { k: 'vec', elem: this.storedCallbacks(t.elem, slot, file, slotName) };
            case 'tuple':
                return { k: 'tuple', elems: t.elems.map((e) => this.storedCallbacks(e, slot, file, slotName)) };
            case 'map':
                return { k: 'map', key: t.key, val: this.storedCallbacks(t.val, slot, file, slotName) };
            default:
                return t;
        }
    }

    // The [traits] accessor entry for a field of a trait class, if the table has one.
    accessorOf(p: ts.Node): Accessor | undefined {
        const owner = ts.isParameter(p) ? p.parent.parent : p.parent;
        if (!owner || !(ts.isClassDeclaration(owner) || ts.isInterfaceDeclaration(owner)) || !owner.name) {
            return undefined;
        }
        const name = memberName(p);
        const key = `${relPath(this.loaded, owner.getSourceFile())}:${owner.name.text}`;
        return name === undefined ? undefined : this.table.traits.get(key)?.accessors.get(name);
    }

    lineKey(node: ts.Node): string {
        return `${relPath(this.loaded, node.getSourceFile())}:${lineOf(node)}`;
    }
}

export function memberName(m: ts.Node): string | undefined {
    if (
        (ts.isPropertySignature(m) ||
            ts.isPropertyDeclaration(m) ||
            ts.isMethodSignature(m) ||
            ts.isMethodDeclaration(m) ||
            ts.isGetAccessorDeclaration(m) ||
            ts.isSetAccessorDeclaration(m) ||
            ts.isParameter(m)) &&
        m.name
    ) {
        return propName(m.name);
    }
    return undefined;
}

export function isNullish(t: ts.TypeNode): boolean {
    if (t.kind === ts.SyntaxKind.UndefinedKeyword || t.kind === ts.SyntaxKind.VoidKeyword || t.kind === ts.SyntaxKind.NullKeyword) {
        return true;
    }
    return ts.isLiteralTypeNode(t) && t.literal.kind === ts.SyntaxKind.NullKeyword;
}

function enumParent(t: ts.Type): ts.Symbol | undefined {
    const sym = t.getSymbol();
    if (!sym) {
        return undefined;
    }
    if (sym.flags & ts.SymbolFlags.EnumMember) {
        const parent: unknown = Reflect.get(sym, 'parent');
        return isSymbol(parent) ? parent : undefined;
    }
    if (sym.flags & ts.SymbolFlags.Enum) {
        return sym;
    }
    return undefined;
}

function isRef(t: ts.Type): t is ts.TypeReference {
    return (t.flags & ts.TypeFlags.Object) !== 0 && 'target' in t;
}

function isSymbol(x: unknown): x is ts.Symbol {
    return typeof x === 'object' && x !== null && 'flags' in x && 'escapedName' in x;
}

function asRef(t: ts.Type): ts.TypeReference {
    if (!isRef(t)) {
        throw new Error('expected a type reference');
    }
    return t;
}

export function sameIR(a: IR, b: IR): boolean {
    return JSON.stringify(a) === JSON.stringify(b);
}

function subst(template: string, args: IR[]): string {
    return template.replace(/\$(\d)/g, (_, i: string) => {
        const a = args[Number(i)];
        return a ? renderElem(a) : '()';
    });
}

// Elem rendering without collecting uses; generic arguments add their uses separately.
function renderElem(a: IR): string {
    const scratch = { set: new Set<string>(), add(p: readonly string[]) { for (const x of p) { this.set.add(x); } } };
    return renderLazy(a, scratch);
}

function renderLazy(a: IR, u: { set: Set<string>; add(p: readonly string[]): void }): string {
    return renderIR(a, 'elem', u);
}

export function usesOfIR(a: IR): string[] {
    const u = { set: new Set<string>(), add(p: readonly string[]) { for (const x of p) { this.set.add(x); } } };
    renderIR(a, 'elem', u);
    return [...u.set];
}

// `jsrt::P<std::cell::RefCell<T>>` -> `P<RefCell<T>>`.
export function shortenPaths(rust: string): string {
    return rust.replace(/\b(?:crate|jsrt|std)(?:::[A-Za-z_][A-Za-z0-9_]*)*::([A-Za-z_][A-Za-z0-9_]*)/g, '$1');
}

// `jsrt::JsString` -> uses `jsrt::JsString`. Bare names are assumed to be in scope.
export function usesOfText(rust: string): string[] {
    return usesOf(rust);
}

function usesOf(rust: string): string[] {
    const out: string[] = [];
    for (const m of rust.matchAll(/\b((?:crate|jsrt|std)(?:::[A-Za-z_][A-Za-z0-9_]*)+)/g)) {
        out.push(m[1]);
    }
    return out;
}

// porting.md "numbers": offsets, lengths, lines and columns are i32; ids and counters are u32;
// computed or fractional values are f64. The guess reads the site's last name.
export function guessNumber(siteKey: string): [string, string] {
    const last = siteKey.replace(/#\d+$/, '').split('.').filter((p) => p !== 'return').pop() ?? '';
    const n = last.toLowerCase();
    if (/(^id$|id$|ids$|^next.*id|^unique)/.test(n) && !/valid|paid/.test(n)) {
        return ['u32', 'id'];
    }
    if (/(count|generation|gen$|depth|recursion|attempt|level|^max|limit|size$|nesting|stack)/.test(n)) {
        return ['u32', 'counter'];
    }
    if (/(start|end$|offset|length|^len$|line|column|^col$|pos$|position|index|indent|width|tab|^i$|^j$|^k$|^n$|^char)/.test(n)) {
        return ['i32', 'text position or index'];
    }
    if (/(complexity|ratio|threshold|time|ms$|duration|weight|factor|contribution|score|percent)/.test(n)) {
        return ['f64', 'fractional'];
    }
    if (/flags$/.test(n)) {
        return ['u32', 'flags'];
    }
    return ['f64', 'no rule matched'];
}

export { isCopy, rustIdent };
