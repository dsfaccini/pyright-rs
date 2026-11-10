// Write analysis over the whole check path. It answers the questions the porting contract asks
// about mutability:
// - which fields some code writes after construction (Cell/RefCell),
// - which array/Map/Set fields have their contents changed in place (RefCell),
// - which fields hold a struct whose own fields get written (RefCell when the struct is a value),
// - which parameters a function changes in place (&mut),
// - which factory locals are reassigned or changed in place.
// Everything is keyed by declaration node, so a write through a union-typed receiver counts for
// every declaration it can reach.

import ts from 'typescript';
import { resolveSymbol } from './program.ts';

const MUTATING_METHODS = new Set([
    'push', 'pop', 'shift', 'unshift', 'splice', 'sort', 'reverse', 'fill', 'copyWithin',
    'set', 'delete', 'clear', 'add',
]);

export class Writes {
    // Fields assigned outside their own constructor.
    readonly written = new Set<ts.Node>();
    // Fields (or params/locals) whose array/Map/Set contents change in place.
    readonly mutated = new Set<ts.Node>();
    // Fields (or params/locals) through which some code writes a field: `x.f.g = v` marks `f`.
    readonly writtenThrough = new Set<ts.Node>();
    // Params and locals assigned after their declaration.
    readonly reassigned = new Set<ts.Node>();
    readonly where = new Map<ts.Node, string>();
    // Interface and class declarations whose objects some code compares with `===` or `!==`.
    readonly identityCompared = new Map<ts.Node, string>();
    // Parameters of an interface member that a closure factory implements, mapped to the parameters
    // of the nested function behind it: a call through `evaluator.getTypeOfExpression(..)` reaches
    // `getTypeOfExpression` in `createTypeEvaluator`.
    readonly implParams = new Map<ts.SignatureDeclaration, readonly ts.ParameterDeclaration[]>();
    // Interface and class declarations whose existing objects some code stores in a second place:
    // a field, a container, an object or array literal, or a parameter its callee stores. A fresh
    // object (`new`, a literal) placed straight into one place does not count. With a field write,
    // this makes the type a handle (docs/porting.md "memory").
    readonly stored = new Map<ts.Node, string>();
    // Every store site per declaration, for the report.
    readonly storeSites = new Map<ts.Node, Set<string>>();
    // Parameters whose value the function stores.
    readonly storingParams = new Set<ts.Node>();
    // Methods that change `this`: they write a field of it, change a field's collection in place,
    // or call another such method on it. Calling one changes the receiver in place, as `push` does.
    readonly mutatingMethods = new Set<ts.Node>();

    constructor(private readonly checker: ts.TypeChecker) {}

    analyze(files: readonly ts.SourceFile[]) {
        // Repeat until nothing grows: passing a field to a parameter that a callee mutates mutates
        // the field, and the callee's own parameters may only be known as mutated after a pass.
        for (let pass = 0; pass < 20; pass++) {
            const before = this.size();
            for (const sf of files) {
                this.visit(sf);
            }
            if (this.size() === before) {
                break;
            }
        }
    }

    private size(): number {
        return this.written.size + this.mutated.size + this.writtenThrough.size + this.reassigned.size + this.stored.size + this.storingParams.size + this.mutatingMethods.size;
    }

    private site(at: ts.Node): string {
        const sf = at.getSourceFile();
        return `${sf.fileName.replace(/.*\/src\//, '')}:${sf.getLineAndCharacterOfPosition(at.getStart(sf)).line + 1}`;
    }

    // The pyright interface and class declarations a type can be; a union gives each member, an
    // array or tuple its elements when `elements` is set.
    private objectDecls(t0: ts.Type, elements: boolean): ts.Node[] {
        const t = this.checker.getNonNullableType(t0);
        const parts = t.isUnion() ? t.types : [t];
        const out: ts.Node[] = [];
        for (const p of parts) {
            if (elements && (this.checker.isArrayType(p) || this.checker.isTupleType(p)) && isTypeReference(p)) {
                for (const a of this.checker.getTypeArguments(p)) {
                    out.push(...this.objectDecls(a, false));
                }
                continue;
            }
            const d = p.flags & ts.TypeFlags.Object ? p.getSymbol()?.declarations?.[0] : undefined;
            if (d && (ts.isInterfaceDeclaration(d) || ts.isClassDeclaration(d)) && !d.getSourceFile().isDeclarationFile) {
                out.push(d);
            }
        }
        return out;
    }

    // `e` is placed in a field, container or literal.
    private onStore(e0: ts.Expression, at: ts.Node) {
        const e = skipParens(e0);
        if (ts.isConditionalExpression(e)) {
            this.onStore(e.whenTrue, at);
            this.onStore(e.whenFalse, at);
            return;
        }
        if (ts.isBinaryExpression(e) && [ts.SyntaxKind.QuestionQuestionToken, ts.SyntaxKind.BarBarToken, ts.SyntaxKind.AmpersandAmpersandToken].includes(e.operatorToken.kind)) {
            this.onStore(e.left, at);
            this.onStore(e.right, at);
            return;
        }
        if (ts.isNewExpression(e) || ts.isObjectLiteralExpression(e) || ts.isArrayLiteralExpression(e) || ts.isFunctionLike(e)) {
            return;
        }
        if (ts.isIdentifier(e)) {
            const sym = resolveSymbol(this.checker, this.checker.getSymbolAtLocation(e));
            for (const d of sym?.declarations ?? []) {
                if (ts.isParameter(d)) {
                    this.storingParams.add(d);
                }
            }
        }
        for (const d of this.objectDecls(this.checker.getTypeAtLocation(e), false)) {
            this.addStored(d, at);
        }
    }

    private addStored(d: ts.Node, at: ts.Node) {
        if (!this.stored.has(d)) {
            this.stored.set(d, this.site(at));
        }
        const set = this.storeSites.get(d) ?? new Set<string>();
        set.add(this.site(at));
        this.storeSites.set(d, set);
    }

    // A container copy (`[...a]`, `a.slice()`, `Array.from(a)`) puts its elements in a second place.
    private onCopy(e: ts.Expression, at: ts.Node) {
        for (const d of this.objectDecls(this.checker.getTypeAtLocation(e), true)) {
            this.addStored(d, at);
        }
    }

    // An object used as a Map key or Set element is hashed by identity.
    private onKey(e: ts.Expression, at: ts.Node) {
        for (const d of this.objectDecls(this.checker.getTypeAtLocation(e), false)) {
            if (!this.identityCompared.has(d)) {
                this.identityCompared.set(d, `${this.site(at)} (Map key or Set element)`);
            }
        }
    }

    private onStoreSites(node: ts.Node) {
        if (ts.isBinaryExpression(node) && node.operatorToken.kind === ts.SyntaxKind.EqualsToken) {
            const l = skipParens(node.left);
            if (ts.isPropertyAccessExpression(l) || ts.isElementAccessExpression(l)) {
                this.onStore(node.right, node);
            }
        } else if (ts.isParameter(node) && ts.isConstructorDeclaration(node.parent) && ts.isParameterPropertyDeclaration(node, node.parent)) {
            this.storingParams.add(node);
        } else if (ts.isPropertyAssignment(node) && ts.isObjectLiteralExpression(node.parent)) {
            this.onStore(node.initializer, node);
        } else if (ts.isShorthandPropertyAssignment(node)) {
            this.onStore(node.name, node);
        } else if (ts.isArrayLiteralExpression(node)) {
            for (const el of node.elements) {
                if (ts.isSpreadElement(el)) {
                    this.onCopy(el.expression, node);
                } else if (!ts.isOmittedExpression(el)) {
                    this.onStore(el, node);
                }
            }
        } else if (ts.isCallExpression(node)) {
            const callee = node.expression;
            const name = ts.isPropertyAccessExpression(callee) ? callee.name.text : undefined;
            const recv = ts.isPropertyAccessExpression(callee) ? callee.expression : undefined;
            const t = recv ? this.checker.getNonNullableType(this.checker.getTypeAtLocation(recv)) : undefined;
            const recvName = t?.getSymbol()?.getName();
            const isArray = t !== undefined && (this.checker.isArrayType(t) || this.checker.isTupleType(t));
            const isMap = recvName === 'Map' || recvName === 'WeakMap';
            const isSet = recvName === 'Set' || recvName === 'WeakSet';
            if (isArray && (name === 'push' || name === 'unshift')) {
                node.arguments.forEach((a) => (ts.isSpreadElement(a) ? this.onCopy(a.expression, node) : this.onStore(a, node)));
            } else if (isArray && name === 'splice') {
                node.arguments.slice(2).forEach((a) => this.onStore(a, node));
            } else if (isArray && recv && (name === 'slice' || name === 'concat' || name === 'filter' || name === 'reverse')) {
                this.onCopy(recv, node);
                node.arguments.forEach((a) => this.onCopy(a, node));
            } else if (isMap && recv && name === 'set') {
                this.onKey(node.arguments[0], node);
                if (node.arguments[1]) {
                    this.onStore(node.arguments[1], node);
                }
            } else if ((isMap || isSet) && name !== undefined && ['get', 'has', 'delete', 'add'].includes(name) && node.arguments[0]) {
                this.onKey(node.arguments[0], node);
            } else if (ts.isPropertyAccessExpression(callee) && ts.isIdentifier(callee.expression) && callee.expression.text === 'Array' && name === 'from' && node.arguments[0]) {
                this.onCopy(node.arguments[0], node);
            }
        }
        if (ts.isCallExpression(node) || ts.isNewExpression(node)) {
            // An argument passed to a parameter the callee stores is stored.
            const decl = this.checker.getResolvedSignature(node)?.getDeclaration();
            if (decl && ts.isFunctionLike(decl)) {
                const params = this.implParams.get(decl) ?? decl.parameters;
                (node.arguments ?? []).forEach((arg, i) => {
                    const param = params[Math.min(i, params.length - 1)];
                    if (param && this.storingParams.has(param)) {
                        this.onStore(arg, node);
                    }
                });
            }
        }
    }

    isWritten(decl: ts.Node): boolean {
        return this.written.has(decl) || this.reassigned.has(decl);
    }

    private mark(set: Set<ts.Node>, decls: readonly ts.Node[], at: ts.Node) {
        for (const d of decls) {
            if (!set.has(d)) {
                set.add(d);
                const sf = at.getSourceFile();
                const line = sf.getLineAndCharacterOfPosition(at.getStart(sf)).line + 1;
                this.where.set(d, `${sf.fileName.replace(/.*\/src\//, '')}:${line}`);
            }
        }
    }

    // Declarations an expression names: a property access gives the property's declarations, an
    // identifier gives its variable or parameter declaration. Locals aliasing a field
    // (`const a = x.f`) also give the field.
    private targets(expr: ts.Expression): ts.Node[] {
        const e = skipParens(expr);
        if (ts.isPropertyAccessExpression(e)) {
            const sym = this.checker.getSymbolAtLocation(e.name);
            return [...(sym?.declarations ?? [])];
        }
        if (ts.isElementAccessExpression(e)) {
            // `x["name"]` names a property; `x[key]` names whatever property the key selects, never
            // the variable `key` itself.
            const arg = e.argumentExpression;
            const sym = ts.isStringLiteral(arg) || ts.isNumericLiteral(arg) ? this.checker.getSymbolAtLocation(arg) : this.checker.getSymbolAtLocation(e);
            return [...(sym?.declarations ?? [])];
        }
        if (ts.isIdentifier(e)) {
            const sym = resolveSymbol(this.checker, this.checker.getSymbolAtLocation(e));
            const decls: ts.Node[] = [...(sym?.declarations ?? [])];
            for (const d of [...decls]) {
                if (ts.isVariableDeclaration(d) && d.initializer) {
                    const init = skipParens(d.initializer);
                    if (ts.isPropertyAccessExpression(init)) {
                        decls.push(...this.targets(init));
                    }
                }
            }
            return decls;
        }
        return [];
    }

    private isCollection(expr: ts.Expression): boolean {
        const t = this.checker.getNonNullableType(this.checker.getTypeAtLocation(expr));
        if (this.checker.isArrayType(t) || this.checker.isTupleType(t)) {
            return true;
        }
        const name = t.getSymbol()?.getName();
        return name === 'Map' || name === 'Set' || name === 'WeakMap' || name === 'WeakSet';
    }

    private inOwnConstructor(target: ts.PropertyAccessExpression): boolean {
        if (target.expression.kind !== ts.SyntaxKind.ThisKeyword) {
            return false;
        }
        let fn: ts.Node | undefined = target.parent;
        while (fn && !ts.isFunctionLike(fn)) {
            fn = fn.parent;
        }
        return fn !== undefined && ts.isConstructorDeclaration(fn);
    }

    // The method whose `this` an expression `this` or `this.f` names, if any.
    private thisMethod(e0: ts.Expression): ts.Node | undefined {
        let e = skipParens(e0);
        while (ts.isPropertyAccessExpression(e) || ts.isElementAccessExpression(e)) {
            e = skipParens(e.expression);
        }
        if (e.kind !== ts.SyntaxKind.ThisKeyword) {
            return undefined;
        }
        let fn: ts.Node | undefined = e.parent;
        while (fn && !(ts.isMethodDeclaration(fn) || ts.isConstructorDeclaration(fn) || ts.isGetAccessorDeclaration(fn) || ts.isSetAccessorDeclaration(fn) || ts.isFunctionDeclaration(fn) || ts.isFunctionExpression(fn))) {
            fn = fn.parent;
        }
        return fn && (ts.isMethodDeclaration(fn) || ts.isSetAccessorDeclaration(fn)) ? fn : undefined;
    }

    private markMutatingMethod(target: ts.Expression) {
        const m = this.thisMethod(target);
        if (m) {
            this.mutatingMethods.add(m);
        }
    }

    private onAssign(lhs: ts.Expression, at: ts.Node) {
        this.markMutatingMethod(lhs);
        const e = skipParens(lhs);
        if (ts.isPropertyAccessExpression(e)) {
            if (e.name.text === 'length' && this.isCollection(e.expression)) {
                this.mark(this.mutated, this.targets(e.expression), at);
                return;
            }
            if (!this.inOwnConstructor(e)) {
                this.mark(this.written, this.targets(e), at);
            }
            this.mark(this.writtenThrough, this.targets(e.expression), at);
        } else if (ts.isElementAccessExpression(e)) {
            if (this.isCollection(e.expression)) {
                this.mark(this.mutated, this.targets(e.expression), at);
            } else {
                this.mark(this.written, this.targets(e), at);
                this.mark(this.writtenThrough, this.targets(e.expression), at);
            }
        } else if (ts.isIdentifier(e)) {
            const sym = resolveSymbol(this.checker, this.checker.getSymbolAtLocation(e));
            this.mark(this.reassigned, sym?.declarations ?? [], at);
        } else if (ts.isObjectLiteralExpression(e) || ts.isArrayLiteralExpression(e)) {
            // Destructuring assignment: each target is a write.
            for (const el of ts.isObjectLiteralExpression(e) ? e.properties : e.elements) {
                if (ts.isShorthandPropertyAssignment(el)) {
                    this.onAssign(el.name, at);
                } else if (ts.isPropertyAssignment(el)) {
                    this.onAssign(el.initializer, at);
                } else if (ts.isExpression(el) && !ts.isOmittedExpression(el)) {
                    this.onAssign(ts.isSpreadElement(el) ? el.expression : el, at);
                }
            }
        }
    }

    private visit(node: ts.Node) {
        this.onStoreSites(node);
        if (ts.isBinaryExpression(node) && isIdentityOperator(node.operatorToken.kind)) {
            this.onIdentity(node);
        }
        if (ts.isBinaryExpression(node) && isAssignmentOperator(node.operatorToken.kind)) {
            this.onAssign(node.left, node);
        } else if (
            (ts.isPrefixUnaryExpression(node) || ts.isPostfixUnaryExpression(node)) &&
            (node.operator === ts.SyntaxKind.PlusPlusToken || node.operator === ts.SyntaxKind.MinusMinusToken)
        ) {
            this.onAssign(node.operand, node);
        } else if (ts.isDeleteExpression(node)) {
            this.onAssign(node.expression, node);
        } else if (ts.isCallExpression(node)) {
            const callee = node.expression;
            if (ts.isPropertyAccessExpression(callee) && MUTATING_METHODS.has(callee.name.text)) {
                if (this.isCollection(callee.expression)) {
                    this.mark(this.mutated, this.targets(callee.expression), node);
                    this.markMutatingMethod(callee.expression);
                }
            }
            if (ts.isPropertyAccessExpression(callee)) {
                // A call to a method that changes `this` changes the receiver.
                const decl = this.checker.getResolvedSignature(node)?.getDeclaration();
                if (decl && this.mutatingMethods.has(decl)) {
                    this.mark(this.mutated, this.targets(callee.expression), node);
                    this.markMutatingMethod(callee.expression);
                }
            }
            this.onCallArgs(node);
        } else if (ts.isForOfStatement(node) || ts.isForInStatement(node)) {
            const init = node.initializer;
            if (!ts.isVariableDeclarationList(init)) {
                this.onAssign(init, node);
            }
        }
        ts.forEachChild(node, (child) => this.visit(child));
    }

    // `a === b` on two objects of one pyright interface or class: that type needs identity.
    private onIdentity(node: ts.BinaryExpression) {
        // The interface and class declarations an operand's type can be; a union gives each member.
        const declsOf = (e: ts.Expression): ts.Node[] => {
            const t = this.checker.getNonNullableType(this.checker.getTypeAtLocation(e));
            const parts = t.isUnion() ? t.types : [t];
            const out: ts.Node[] = [];
            for (const p of parts) {
                const d = p.flags & ts.TypeFlags.Object ? p.getSymbol()?.declarations?.[0] : undefined;
                if (d && (ts.isInterfaceDeclaration(d) || ts.isClassDeclaration(d)) && !d.getSourceFile().isDeclarationFile) {
                    out.push(d);
                }
            }
            return out;
        };
        const right = new Set(declsOf(node.right));
        for (const d of declsOf(node.left)) {
            if (right.has(d) && !this.identityCompared.has(d)) {
                const sf = node.getSourceFile();
                const line = sf.getLineAndCharacterOfPosition(node.getStart(sf)).line + 1;
                this.identityCompared.set(d, `${sf.fileName.replace(/.*\/src\//, '')}:${line}`);
            }
        }
    }

    // Passing a field or param to a parameter that the callee changes in place changes it too.
    private onCallArgs(call: ts.CallExpression) {
        const decl = this.checker.getResolvedSignature(call)?.getDeclaration();
        if (!decl || !ts.isFunctionLike(decl)) {
            return;
        }
        const params = this.implParams.get(decl) ?? decl.parameters;
        call.arguments.forEach((arg, i) => {
            const param = params[Math.min(i, params.length - 1)];
            if (!param) {
                return;
            }
            if (this.mutated.has(param)) {
                this.mark(this.mutated, this.targets(arg), call);
            }
            if (this.writtenThrough.has(param)) {
                this.mark(this.writtenThrough, this.targets(arg), call);
            }
        });
    }
}

function isTypeReference(t: ts.Type): t is ts.TypeReference {
    return (t.flags & ts.TypeFlags.Object) !== 0 && 'target' in t;
}

export function skipParens(e: ts.Expression): ts.Expression {
    let x = e;
    while (ts.isParenthesizedExpression(x) || ts.isNonNullExpression(x) || ts.isAsExpression(x) || ts.isTypeAssertionExpression(x)) {
        x = x.expression;
    }
    return x;
}

function isIdentityOperator(kind: ts.SyntaxKind): boolean {
    return (
        kind === ts.SyntaxKind.EqualsEqualsEqualsToken ||
        kind === ts.SyntaxKind.ExclamationEqualsEqualsToken ||
        kind === ts.SyntaxKind.EqualsEqualsToken ||
        kind === ts.SyntaxKind.ExclamationEqualsToken
    );
}

function isAssignmentOperator(kind: ts.SyntaxKind): boolean {
    return kind >= ts.SyntaxKind.FirstAssignment && kind <= ts.SyntaxKind.LastAssignment;
}
