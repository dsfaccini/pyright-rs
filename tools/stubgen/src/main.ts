// tools/stubgen: Rust signature stubs from pyright's TypeScript. See tools/stubgen/README.md.

import { spawnSync } from 'node:child_process';
import { mkdirSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import ts from 'typescript';
import * as path from 'node:path';
import { parseArgs } from 'node:util';
import { FileEmitter, trace } from './emit.ts';
import { Gen } from './gen.ts';
import { modulePathOf } from './names.ts';
import { Loaded, loadProgram, relPath } from './program.ts';
import { NumberTable, Run, Table, loadRuns, loadTable } from './table.ts';
import { Writes } from './writes.ts';
import { checkRun, installRun, isSkeleton } from './check.ts';
import { coverage } from './coverage.ts';
import { ifaceCheck } from './ifacecheck.ts';

const here = path.dirname(path.dirname(new URL(import.meta.url).pathname));
const repoRoot = path.resolve(here, '../..');

function main() {
    const { values } = parseArgs({
        options: {
            pyright: { type: 'string' },
            run: { type: 'string', multiple: true },
            out: { type: 'string' },
            check: { type: 'boolean' },
            'check-only': { type: 'boolean' },
            install: { type: 'boolean' },
            trace: { type: 'boolean' },
        },
    });
    trace.on = values.trace ?? false;
    const pyright = values.pyright ?? path.join(process.env.HOME ?? '', 'pyright');
    const outRoot = path.resolve(values.out ?? path.join(here, 'out'));
    const runs = loadRuns(path.join(here, 'runs.toml')).filter((r) => !values.run || values.run.includes(r.name));
    if (runs.length === 0) {
        throw new Error(`no run named ${values.run?.join(', ')} in runs.toml`);
    }
    if (values['check-only']) {
        let ok = true;
        for (const run of runs) {
            ok = checkRun(repoRoot, path.join(outRoot, run.name), run.install) && ok;
        }
        process.exit(ok ? 0 : 1);
    }
    if (values.install) {
        let ok = true;
        for (const run of runs) {
            if (!run.install) {
                throw new Error(`run ${run.name} has no \`install = true\` in runs.toml`);
            }
            ok = installRun(repoRoot, path.join(outRoot, run.name)) && ok;
        }
        process.exit(ok ? 0 : 1);
    }

    const table = loadTable(path.join(here, 'typemap.toml'));
    const numbersPath = path.join(here, 'numbers.toml');
    const numbers = new NumberTable(numbersPath);
    const t0 = Date.now();
    const loaded = loadProgram(pyright);
    const writes = new Writes(loaded.checker);
    registerFactoryImpls(loaded, table, writes);
    writes.analyze(loaded.files);
    console.error(`loaded ${loaded.files.length} files and analyzed writes in ${Date.now() - t0} ms`);

    let ok = true;
    for (const run of runs) {
        const gen = new Gen(loaded, table, numbers, writes);
        gen.heap = run.heap;
        generate(gen, run, path.join(outRoot, run.name));
        writeFileSync(numbersPath, numbers.render());
        if (values.check) {
            ok = checkRun(repoRoot, path.join(outRoot, run.name), run.install) && ok;
        }
    }
    process.exit(ok ? 0 : 1);
}

// For each closure factory, map its interface's members to the nested functions behind them, so
// the write analysis follows calls made through the interface.
function registerFactoryImpls(loaded: Loaded, table: Table, writes: Writes) {
    const byPath = new Map(loaded.files.map((sf) => [relPath(loaded, sf), sf]));
    for (const f of table.factories) {
        const sf = byPath.get(f.file);
        const ifaceFile = f.iface ? byPath.get(f.iface.slice(0, f.iface.indexOf(':'))) : undefined;
        const ifaceName = f.iface ? f.iface.slice(f.iface.indexOf(':') + 1) : '';
        if (!sf || !ifaceFile) {
            continue;
        }
        let fn: ts.FunctionDeclaration | undefined;
        let stmts: readonly ts.Statement[] = sf.statements;
        for (const part of f.func.split('.')) {
            fn = stmts.find((s): s is ts.FunctionDeclaration => ts.isFunctionDeclaration(s) && s.name?.text === part);
            stmts = fn?.body?.statements ?? [];
        }
        const nested = new Map(
            stmts.filter((s): s is ts.FunctionDeclaration => ts.isFunctionDeclaration(s) && s.body !== undefined).map((s) => [s.name?.text ?? '', s])
        );
        const iface = ifaceFile.statements.find((s): s is ts.InterfaceDeclaration => ts.isInterfaceDeclaration(s) && s.name.text === ifaceName);
        for (const m of iface?.members ?? []) {
            const name = m.name && ts.isIdentifier(m.name) ? m.name.text : undefined;
            const impl = name !== undefined ? nested.get(name) : undefined;
            if (!impl) {
                continue;
            }
            if (ts.isMethodSignature(m)) {
                writes.implParams.set(m, impl.parameters);
            } else if (ts.isPropertySignature(m) && m.type && ts.isFunctionTypeNode(m.type)) {
                writes.implParams.set(m.type, impl.parameters);
            }
        }
    }
}

function generate(gen: Gen, run: Run, outDir: string) {
    rmSync(outDir, { recursive: true, force: true });
    const byPath = new Map(gen.loaded.files.map((sf) => [relPath(gen.loaded, sf), sf]));
    const notes: string[] = [];
    const write = (kind: 'stubs' | 'deps', file: string, text: string) => {
        const target = path.join(outDir, kind, modulePathOf(file) + '.rs');
        mkdirSync(path.dirname(target), { recursive: true });
        writeFileSync(target, text);
    };
    for (const file of run.files) {
        const sf = byPath.get(file);
        if (!sf) {
            throw new Error(`${file} is not on the check path`);
        }
        const fe = new FileEmitter(gen, file, sf, 'stubs', undefined);
        write('stubs', file, fe.run());
        notes.push(...fe.notes.map((n) => `${file}: ${n}`));
    }
    // Dependency placeholders, to a fixpoint: each one can name more types.
    const wanted = new Map<string, Set<string>>();
    const runFiles = new Set(run.files);
    for (let round = 0; round < 50; round++) {
        let grew = false;
        for (const key of [...gen.needed]) {
            const file = gen.fileOfKey(key);
            if (runFiles.has(file)) {
                continue;
            }
            const set = wanted.get(file) ?? new Set<string>();
            if (!set.has(gen.nameOfKey(key))) {
                set.add(gen.nameOfKey(key));
                wanted.set(file, set);
                grew = true;
            }
        }
        if (!grew) {
            break;
        }
        for (const [file, names] of wanted) {
            const sf = byPath.get(file);
            if (!sf) {
                continue;
            }
            for (const [k, a] of gen.anon) {
                if (a.file === file) {
                    names.add(gen.nameOfKey(k));
                }
            }
            const fe = new FileEmitter(gen, file, sf, 'deps', names);
            // A file ported in the crate serves the stubs itself: its placeholder names no further
            // placeholders.
            const ported = !isSkeleton(path.join(repoRoot, 'crates', 'pyright', 'src', modulePathOf(file) + '.rs'));
            write('deps', file, ported ? gen.quietly(() => fe.run()) : fe.run());
        }
    }
    const factoryFuncs = new Set(gen.table.factories.filter((f) => !f.func.includes('.')).map((f) => f.func));
    const cov = coverage(gen.loaded.srcRoot, outDir, run.files, factoryFuncs);
    if (cov.missing) {
        console.error(`coverage: ${cov.missing} declarations have no marker (see REPORT.md)`);
    }
    for (const f of gen.table.factories) {
        if (!run.files.includes(f.file) || !f.iface) {
            continue;
        }
        const r = ifaceCheck(path.join(gen.loaded.srcRoot, gen.fileOfKey(f.iface)), gen.nameOfKey(f.iface), path.join(outDir, 'stubs', modulePathOf(f.file) + '.rs'), f.name);
        cov.lines.push(r.line);
        cov.missing += r.missing.length;
    }
    writeFileSync(path.join(outDir, 'REPORT.md'), report(gen, run, wanted, notes, cov.lines));
    const rs: string[] = [];
    const collect = (dir: string) => {
        for (const e of readdirSync(dir, { withFileTypes: true })) {
            const p = path.join(dir, e.name);
            if (e.isDirectory()) {
                collect(p);
            } else if (p.endsWith('.rs')) {
                rs.push(p);
            }
        }
    };
    collect(outDir);
    const fmt = spawnSync('rustfmt', ['--edition', '2024', ...rs], { encoding: 'utf8' });
    if (fmt.status !== 0) {
        console.error(`rustfmt failed:\n${fmt.stderr}`);
    }
}

function report(gen: Gen, run: Run, deps: Map<string, Set<string>>, notes: string[], cov: string[]): string {
    const lines = [`# stubgen run \`${run.name}\``, '', 'Stubs (copy candidates for `crates/pyright/src`):', ''];
    for (const f of run.files) {
        lines.push(`- \`stubs/${modulePathOf(f)}.rs\` from \`${f}\``);
    }
    lines.push('', 'Coverage: markers for every TypeScript function, method and type of each stubbed file, and methods for every member of each factory interface:', '', ...cov);
    lines.push('', 'Dependency placeholders (compile check only):', '');
    for (const [f, names] of [...deps].sort()) {
        lines.push(`- \`deps/${modulePathOf(f)}.rs\`: ${[...names].sort().join(', ')}`);
    }
    const [stubSites, depSites] = gen.numbers.unreviewed(new Set(run.files));
    lines.push('', `Number sites without a reviewed type (numbers.toml, values ending in "?"): ${stubSites} in the stubbed files, ${depSites} in placeholders.`, '');
    const unmapped = [...gen.unmapped].sort((a, b) => b[1].length - a[1].length);
    lines.push(`Unmapped TypeScript types (rendered with a \`/*? ... */\` marker): ${unmapped.length} distinct.`, '');
    for (const [ts, sites] of unmapped) {
        const shown = [...new Set(sites)].slice(0, 4).join(', ');
        lines.push(`- \`${ts}\` x${sites.length}: ${shown}${sites.length > 4 ? ', ...' : ''}`);
    }
    lines.push('', 'How each class and interface of the stubbed files is held (docs/porting.md "memory"):', '');
    for (const f of run.files) {
        for (const [k, decls] of gen.decls) {
            if (!k.startsWith(`${f}:`) || !decls.some((d) => ts.isInterfaceDeclaration(d) || ts.isClassDeclaration(d))) {
                continue;
            }
            if (gen.familyMembers.has(k) || gen.familyByRootKey.has(k) || gen.traitClasses.has(k) || gen.factoryByIface.has(k) || gen.table.merge.has(k)) {
                continue;
            }
            lines.push(`- \`${k}\`: ${gen.modeOf(k)}; ${gen.modeReason(k)}`);
        }
    }
    const conflicts = [...gen.table.modes]
        .filter(([k, m]) => m === 'value' && gen.identityComparedAt(k) !== undefined)
        .map(([k]) => `- \`${k}\`: compared with \`===\` at ${gen.identityComparedAt(k)}`);
    lines.push('', `[modes] values that some code compares by identity (porting.md wants a handle): ${conflicts.length}.`, '', ...conflicts);
    const writtenThrough = [...gen.decls.keys()]
        .filter((k) => gen.decls.get(k)?.some((d) => ts.isInterfaceDeclaration(d) || ts.isClassDeclaration(d)) && gen.modeOf(k) === 'value')
        .filter((k) => !(gen.familyMembers.has(k) || gen.familyByRootKey.has(k) || gen.traitClasses.has(k) || gen.factoryByIface.has(k) || gen.table.merge.has(k)))
        .map((k) => [k, gen.writtenThroughParamSites(k)] as const)
        .filter(([, sites]) => sites.length > 0)
        .sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0))
        .map(([k, sites]) => `- \`${k}\`: ${sites.length} params, e.g. ${sites.slice(0, 3).join('; ')}`);
    lines.push('', `Value-mode types whose fields some code writes through a parameter, directly or through calls (the caller does not see a write to a copy): ${writtenThrough.length}.`, '', ...writtenThrough);
    if (notes.length) {
        lines.push('', 'Notes:', '', ...notes.map((n) => `- ${n}`));
    }
    return lines.join('\n') + '\n';
}

main();
