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

    constructor(private readonly checker: ts.TypeChecker) {}

    analyze(files: readonly ts.SourceFile[]) {
        // Two passes: the second sees which params the first found mutated, so passing a field to
        // such a param counts as mutating the field.
        for (let pass = 0; pass < 2; pass++) {
            for (const sf of files) {
                this.visit(sf);
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

    private onAssign(lhs: ts.Expression, at: ts.Node) {
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

    // Passing a field or param to a parameter that the callee changes in place changes it too.
    private onCallArgs(call: ts.CallExpression) {
        const decl = this.checker.getResolvedSignature(call)?.getDeclaration();
        if (!decl || !ts.isFunctionLike(decl)) {
            return;
        }
        call.arguments.forEach((arg, i) => {
            const param = decl.parameters[Math.min(i, decl.parameters.length - 1)];
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

export function skipParens(e: ts.Expression): ts.Expression {
    let x = e;
    while (ts.isParenthesizedExpression(x) || ts.isNonNullExpression(x) || ts.isAsExpression(x) || ts.isTypeAssertionExpression(x)) {
        x = x.expression;
    }
    return x;
}

function isAssignmentOperator(kind: ts.SyntaxKind): boolean {
    return kind >= ts.SyntaxKind.FirstAssignment && kind <= ts.SyntaxKind.LastAssignment;
}
