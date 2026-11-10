// Loads pyright's check path as a TypeScript program and indexes its declarations.

import * as path from 'node:path';
import ts from 'typescript';

export interface Loaded {
    program: ts.Program;
    checker: ts.TypeChecker;
    srcRoot: string;
    files: ts.SourceFile[];
}

// The check path is everything `pyright.ts` imports, transitively (docs/research/pyright-map.md).
export function loadProgram(pyrightRoot: string): Loaded {
    const pkg = path.join(pyrightRoot, 'packages/pyright-internal');
    const configPath = path.join(pkg, 'tsconfig.json');
    const config = ts.readConfigFile(configPath, ts.sys.readFile);
    if (config.error) {
        throw new Error(ts.flattenDiagnosticMessageText(config.error.messageText, '\n'));
    }
    const parsed = ts.parseJsonConfigFileContent(config.config, ts.sys, pkg);
    const srcRoot = path.join(pkg, 'src');
    const program = ts.createProgram({
        rootNames: [path.join(srcRoot, 'pyright.ts')],
        options: { ...parsed.options, noEmit: true },
    });
    const checker = program.getTypeChecker();
    const files = program
        .getSourceFiles()
        .filter((sf) => !sf.isDeclarationFile && sf.fileName.startsWith(srcRoot + path.sep));
    return { program, checker, srcRoot, files };
}

export function relPath(loaded: Loaded, sf: ts.SourceFile): string {
    return path.relative(loaded.srcRoot, sf.fileName).split(path.sep).join('/');
}

export function lineOf(node: ts.Node): number {
    const sf = node.getSourceFile();
    return sf.getLineAndCharacterOfPosition(node.getStart(sf)).line + 1;
}

export function declName(node: ts.Node): string | undefined {
    if (
        (ts.isInterfaceDeclaration(node) ||
            ts.isClassDeclaration(node) ||
            ts.isEnumDeclaration(node) ||
            ts.isTypeAliasDeclaration(node) ||
            ts.isFunctionDeclaration(node) ||
            ts.isModuleDeclaration(node)) &&
        node.name &&
        ts.isIdentifier(node.name)
    ) {
        return node.name.text;
    }
    return undefined;
}

// Follows import aliases to the symbol that declares the thing.
export function resolveSymbol(checker: ts.TypeChecker, sym: ts.Symbol | undefined): ts.Symbol | undefined {
    let s = sym;
    while (s && s.flags & ts.SymbolFlags.Alias) {
        const next = checker.getAliasedSymbol(s);
        if (next === s) {
            break;
        }
        s = next;
    }
    return s;
}

// Top-level declarations of a file, in source order, with namespace members flattened under their
// namespace.
export function topLevelStatements(sf: ts.SourceFile): readonly ts.Statement[] {
    return sf.statements;
}

export function hasModifier(node: ts.Node, kind: ts.SyntaxKind): boolean {
    return ts.canHaveModifiers(node) ? (ts.getModifiers(node) ?? []).some((m) => m.kind === kind) : false;
}

export function isExported(node: ts.Node): boolean {
    return hasModifier(node, ts.SyntaxKind.ExportKeyword);
}

export function propName(name: ts.PropertyName | ts.BindingName): string | undefined {
    if (ts.isIdentifier(name) || ts.isPrivateIdentifier(name)) {
        return name.text;
    }
    if (ts.isStringLiteral(name) || ts.isNumericLiteral(name)) {
        return name.text;
    }
    return undefined;
}
