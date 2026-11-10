// Checks that every member of a factory's TypeScript interface is a `pub fn` of its Rust struct.

import { readFileSync } from 'node:fs';
import ts from 'typescript';
import { fieldName } from './names.ts';

export function ifaceCheck(tsFile: string, iface: string, rustFile: string, struct: string): { line: string; missing: string[] } {
    const sf = ts.createSourceFile(tsFile, readFileSync(tsFile, 'utf8'), ts.ScriptTarget.Latest, true);
    const decl = sf.statements.find((s): s is ts.InterfaceDeclaration => ts.isInterfaceDeclaration(s) && s.name.text === iface);
    if (!decl) {
        return { line: `- interface ${iface} not found in ${tsFile}`, missing: [iface] };
    }
    const rust = readFileSync(rustFile, 'utf8');
    const impl = rust.slice(Math.max(0, rust.indexOf(`impl ${struct} {`)));
    const names = decl.members.map((m) => (m.name && ts.isIdentifier(m.name) ? m.name.text : undefined)).filter((n): n is string => n !== undefined);
    const missing = names.filter((n) => !new RegExp(`\\bpub fn ${fieldName(n)}\\b`).test(impl));
    return { line: `- \`${iface}\`: ${names.length - missing.length}/${names.length} members are \`pub\` methods of \`${struct}\`${missing.length ? `; missing ${missing.join(', ')}` : ''}`, missing };
}
