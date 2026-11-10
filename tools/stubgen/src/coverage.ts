// Coverage check: every TypeScript function, method and type declaration of a stubbed file has a
// `// <file>:<line>` marker in its Rust stub. Nested functions of a closure factory count as methods.

import { readFileSync } from 'node:fs';
import * as path from 'node:path';
import ts from 'typescript';
import { modulePathOf } from './names.ts';

function expected(sf: ts.SourceFile, factoryNames: Set<string>): Map<number, string> {
    const out = new Map<number, string>();
    const line = (n: ts.Node) => sf.getLineAndCharacterOfPosition(n.getStart(sf)).line + 1;
    const add = (n: ts.Node, what: string) => out.set(line(n), what);
    const visitBody = (stmts: readonly ts.Statement[], inFactory: boolean) => {
        for (const st of stmts) {
            if (ts.isFunctionDeclaration(st) && st.body && st.name) {
                add(st, `function ${st.name.text}`);
                if (factoryNames.has(st.name.text) || inFactory) {
                    // Nested functions of a factory are methods.
                    if (factoryNames.has(st.name.text)) {
                        visitBody(st.body.statements, true);
                    }
                }
            } else if (!inFactory && (ts.isInterfaceDeclaration(st) || ts.isEnumDeclaration(st) || ts.isTypeAliasDeclaration(st))) {
                add(st, `type ${st.name.text}`);
            } else if (!inFactory && ts.isClassDeclaration(st) && st.name) {
                add(st, `class ${st.name.text}`);
                for (const m of st.members) {
                    if ((ts.isMethodDeclaration(m) || ts.isConstructorDeclaration(m) || ts.isGetAccessorDeclaration(m) || ts.isSetAccessorDeclaration(m)) && m.body) {
                        add(m, `method ${st.name.text}.${m.name?.getText(sf) ?? 'constructor'}`);
                    }
                }
            } else if (!inFactory && ts.isModuleDeclaration(st) && st.body && ts.isModuleBlock(st.body)) {
                visitBody(st.body.statements, false);
            }
        }
    };
    visitBody(sf.statements, false);
    return out;
}

// Lines of the report; `missing` counts declarations without a marker.
export function coverage(srcRoot: string, runDir: string, files: string[], factoryFuncs: Set<string>): { lines: string[]; missing: number } {
    const lines: string[] = [];
    let missing = 0;
    for (const file of files) {
        const full = path.join(srcRoot, file);
        const sf = ts.createSourceFile(full, readFileSync(full, 'utf8'), ts.ScriptTarget.Latest, true);
        const rust = readFileSync(path.join(runDir, 'stubs', modulePathOf(file) + '.rs'), 'utf8');
        const markers = new Set([...rust.matchAll(new RegExp(`// ${file.replace(/[.]/g, '\\.')}:(\\d+)`, 'g'))].map((m) => Number(m[1])));
        const exp = expected(sf, factoryFuncs);
        const miss = [...exp].filter(([l]) => !markers.has(l));
        missing += miss.length;
        lines.push(`- \`${file}\`: ${exp.size - miss.length}/${exp.size} declarations have a marker`);
        for (const [l, what] of miss) {
            lines.push(`  - missing ${file}:${l} ${what}`);
        }
    }
    return { lines, missing };
}
