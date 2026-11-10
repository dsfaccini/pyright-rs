// The compile check: overlay a run's stubs and dependency placeholders on a scratch copy of the
// workspace and run `cargo check`.
//
// - A stub replaces the crate's file.
// - A dependency placeholder fills a crate file only while that file is still an empty skeleton.
//   Once someone ports the file, the stubs compile against the real one.
// - `jsrt` items the stubs use but `crates/jsrt` lacks come from jsrt_shim.rs. The report lists
//   them: they are requests to the owner of `crates/jsrt`.

import { execFileSync, spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from 'node:fs';
import * as path from 'node:path';
import { mergePorted } from './merge.ts';

const here = path.dirname(path.dirname(new URL(import.meta.url).pathname));

function walk(dir: string): string[] {
    if (!existsSync(dir)) {
        return [];
    }
    const out: string[] = [];
    for (const e of readdirSync(dir)) {
        const p = path.join(dir, e);
        if (statSync(p).isDirectory()) {
            out.push(...walk(p));
        } else if (p.endsWith('.rs')) {
            out.push(p);
        }
    }
    return out;
}

// A skeleton file holds only `//!` and `//` comments.
export function isSkeleton(file: string): boolean {
    if (!existsSync(file)) {
        return true;
    }
    return readFileSync(file, 'utf8')
        .split('\n')
        .every((l) => l.trim() === '' || l.trim().startsWith('//'));
}

// A ported file holds at least one function whose body is not `todo!()`.
function isPorted(file: string): boolean {
    if (!existsSync(file)) {
        return false;
    }
    const text = readFileSync(file, 'utf8');
    const fns = (text.match(/^\s*(pub(\([^)]*\))?\s+)?fn\s/gm) ?? []).length;
    const todos = (text.match(/todo!\(\)/g) ?? []).length;
    return fns > todos;
}

// Names `text` imports from the module at `modPath` (`crate::analyzer::source_file`).
function importedFrom(text: string, modPath: string): string[] {
    const out: string[] = [];
    for (const m of text.matchAll(/^use\s+([^;]+);/gms)) {
        const spec = m[1].replace(/\s+/g, '');
        if (spec.startsWith(`${modPath}::{`)) {
            out.push(...spec.slice(modPath.length + 3, -1).split(',').filter((n) => n.length > 0));
        } else if (spec.startsWith(`${modPath}::`) && !spec.slice(modPath.length + 2).includes('::')) {
            out.push(spec.slice(modPath.length + 2));
        }
    }
    return out;
}

// Defined by an item, a `pub use`, or a macro call that takes the name first (`node_handle!(FunctionNode, ..)`).
function defines(text: string, name: string): boolean {
    return new RegExp(`\\b(struct|enum|type|trait|fn|const|static|mod)\\s+${name}\\b|pub use [^;]*\\b${name}\\b|!\\s*[({]\\s*${name}\\b`).test(text);
}

function ensureModDecl(srcRoot: string, rel: string) {
    const parts = rel.replace(/\.rs$/, '').split('/');
    for (let i = parts.length - 1; i >= 1; i--) {
        const dir = path.join(srcRoot, ...parts.slice(0, i));
        const modFile = path.join(dir, 'mod.rs');
        const decl = `pub mod ${parts[i]};`;
        const text = existsSync(modFile) ? readFileSync(modFile, 'utf8') : '';
        if (!text.split('\n').some((l) => l.trim() === decl)) {
            // Before the first `pub mod` that sorts after it, else at the end.
            const lines = text === '' ? [] : text.replace(/\n$/, '').split('\n');
            const at = lines.findIndex((l) => /^pub mod [a-z_0-9]+;$/.test(l) && l > decl);
            lines.splice(at < 0 ? lines.length : at, 0, decl);
            writeFileSync(modFile, lines.join('\n') + '\n');
        }
    }
    const lib = path.join(srcRoot, 'lib.rs');
    const top = `pub mod ${parts[0]};`;
    const libText = readFileSync(lib, 'utf8');
    if (!libText.split('\n').some((l) => l.trim() === top)) {
        writeFileSync(lib, libText + top + '\n');
    }
}

// Code lines of a crate file outside the given line ranges that the new stub does not hold: what a
// merge would drop. Imports, attributes and comments do not count.
export interface Placement {
    // Stubs written, relative to the source root.
    placed: string[];
    // Ported files that superseded their stub (runs that do not install).
    superseded: string[];
    // Ported functions carried into the stub, per file.
    merged: string[];
    // Merge failures: the install changes nothing when there is one.
    lost: string[];
    // What a merge kept as it was or could not place, per file: the owner's to resolve.
    warnings: string[];
}

function crateModPath(rel: string): string {
    return 'crate::' + rel.replace(/\.rs$/, '').split('/').join('::');
}

// Writes a run's stubs under `targetSrc`. A crate file (under `crateSrc`) with ported functions
// either supersedes its stub (a run that does not install) or is merged into it (an installed run).
export function placeStubs(runDir: string, crateSrc: string, targetSrc: string, install: boolean): Placement {
    const res: Placement = { placed: [], superseded: [], merged: [], lost: [], warnings: [] };
    for (const f of walk(path.join(runDir, 'stubs'))) {
        const rel = path.relative(path.join(runDir, 'stubs'), f);
        const target = path.join(targetSrc, rel);
        const real = path.join(crateSrc, rel);
        let text = readFileSync(f, 'utf8');
        if (!install && isPorted(real)) {
            // A file someone has ported supersedes its stub: placing the stub would break the code
            // that already calls the port.
            res.superseded.push(rel);
            continue;
        }
        const crateText = existsSync(real) ? readFileSync(real, 'utf8') : '';
        const m = mergePorted(text, crateText, crateModPath(rel));
        // A crate file with no ported function at a marker (a skeleton, a placeholder, an
        // unchanged stub) is replaced; a ported one changes only in its unported items.
        if (install && (m.kept.length > 0 || m.lost.length > 0)) {
            res.warnings.push(
                ...m.lost.map((l) => `${rel}: ported function at ${l} has no stub marker; kept as is`),
                ...m.pending.map((l) => `${rel}: not placed: ${l}`)
            );
            const changes = [
                m.updated.length ? `updated ${m.updated.join(', ')}` : '',
                m.inserted.length ? `inserted ${m.inserted.join(', ')}` : '',
                m.removed.length ? `removed ${m.removed.join(', ')}` : '',
            ].filter((x) => x);
            if (changes.length) {
                res.merged.push(`${rel}: ${changes.join('; ')}`);
            }
            text = m.text;
        }
        mkdirSync(path.dirname(target), { recursive: true });
        writeFileSync(target, text);
        // An install merges into an empty scratch root first; the crate gets the mod lines later.
        if (existsSync(path.join(targetSrc, 'lib.rs'))) {
            ensureModDecl(targetSrc, rel);
        }
        res.placed.push(rel);
    }
    // No rustfmt here: the generator formats the stubs, and a ported file keeps its own layout.
    return res;
}

function shimSections(): Map<string, string> {
    const text = readFileSync(path.join(here, 'jsrt_shim.rs'), 'utf8');
    const out = new Map<string, string>();
    for (const part of text.split(/^\/\/ @item /m).slice(1)) {
        const nl = part.indexOf('\n');
        out.set(part.slice(0, nl).trim(), part.slice(nl + 1));
    }
    return out;
}

export function checkRun(repoRoot: string, runDir: string, install: boolean): boolean {
    const name = path.basename(runDir);
    const scratch = path.join(repoRoot, 'target', 'stubgen-check', name);
    rmSync(scratch, { recursive: true, force: true });
    mkdirSync(scratch, { recursive: true });
    execFileSync('cp', ['-r', path.join(repoRoot, 'crates'), path.join(repoRoot, 'Cargo.toml'), path.join(repoRoot, 'Cargo.lock'), scratch]);
    const src = path.join(scratch, 'crates', 'pyright', 'src');

    const placement = placeStubs(runDir, path.join(repoRoot, 'crates', 'pyright', 'src'), src, install);
    const placed = [...placement.placed];
    const stubFiles = new Set(placement.placed);
    const keptReal: string[] = [];
    const filled: string[] = [];
    const superseded = placement.superseded;

    // Placeholders serve the placed stubs only.
    const deps = placed.length ? walk(path.join(runDir, 'deps')) : [];
    const depRels = deps.map((f) => path.relative(path.join(runDir, 'deps'), f));
    const placedTexts = [
        ...placed.map((r) => readFileSync(path.join(src, r), 'utf8')),
        ...depRels.filter((r) => isSkeleton(path.join(repoRoot, 'crates', 'pyright', 'src', r))).map((r) => readFileSync(path.join(runDir, 'deps', r), 'utf8')),
    ];
    for (const f of deps) {
        const rel = path.relative(path.join(runDir, 'deps'), f);
        const target = path.join(src, rel);
        const real = path.join(repoRoot, 'crates', 'pyright', 'src', rel);
        if (!isSkeleton(real) || stubFiles.has(rel)) {
            if (stubFiles.has(rel)) {
                continue;
            }
            keptReal.push(rel);
            // A partially ported file may lack items the stubs import from it: serve them from the
            // placeholder, in a side module the real file re-exports.
            const modPath = crateModPath(rel);
            const wanted = new Set(placedTexts.flatMap((t) => importedFrom(t, modPath)));
            const missing = [...wanted].filter((n) => !defines(readFileSync(real, 'utf8'), n));
            if (missing.length) {
                const stem = path.basename(rel, '.rs');
                const fillRel = path.join(path.dirname(rel), `${stem}_stubgen_fill.rs`);
                const fill = readFileSync(f, 'utf8').replace(/^(#!\[allow\([^)]*\)\])$/m, `$1\n\nuse super::${stem}::*;`);
                writeFileSync(path.join(src, fillRel), fill);
                ensureModDecl(src, fillRel);
                writeFileSync(target, readFileSync(target, 'utf8') + `\n#[allow(unused_imports)]\npub use super::${stem}_stubgen_fill::{${missing.join(', ')}};\n`);
                filled.push(`${rel}: ${missing.join(', ')}`);
            }
            continue;
        }
        mkdirSync(path.dirname(target), { recursive: true });
        writeFileSync(target, readFileSync(f));
        ensureModDecl(src, rel);
        placed.push(rel);
    }
    // jsrt items the stubs name that crates/jsrt does not define yet.
    const jsrtSrc = walk(path.join(repoRoot, 'crates', 'jsrt', 'src')).map((f) => readFileSync(f, 'utf8')).join('\n');
    const usedJsrt = new Set<string>();
    for (const f of placed) {
        const text = readFileSync(path.join(src, f), 'utf8');
        for (const m of text.matchAll(/\bjsrt::([A-Za-z_][A-Za-z0-9_]*)/g)) {
            usedJsrt.add(m[1]);
        }
        for (const m of text.matchAll(/\bjsrt::\{([^}]*)\}/g)) {
            for (const n of m[1].split(',')) {
                usedJsrt.add(n.trim());
            }
        }
    }
    const shims = shimSections();
    const shimmed: string[] = [];
    const missing: string[] = [];
    for (const item of [...usedJsrt].sort()) {
        const defined = new RegExp(`\\b(struct|enum|type|trait|fn|macro_rules!)\\s+${item}\\b|pub use [^;]*\\b${item}\\b`).test(jsrtSrc);
        if (defined) {
            continue;
        }
        const shim = shims.get(item);
        if (shim) {
            shimmed.push(item);
            const lib = path.join(scratch, 'crates', 'jsrt', 'src', 'lib.rs');
            writeFileSync(lib, readFileSync(lib, 'utf8') + `\n// stubgen shim: ${item}\n` + shim);
        } else {
            missing.push(item);
        }
    }

    const target = path.join(repoRoot, 'target', 'stubgen-check', 'target');
    const r = spawnSync('cargo', ['check', '-p', 'pyright', '--message-format', 'short'], {
        cwd: scratch,
        env: { ...process.env, CARGO_TARGET_DIR: target },
        encoding: 'utf8',
        maxBuffer: 1 << 28,
    });
    const output = (r.stderr ?? '') + (r.stdout ?? '');
    const errors = output.split('\n').filter((l) => /: error(\[E\d+\])?:/.test(l) || /^error(\[E\d+\])?:/.test(l));
    const warnings = output.split('\n').filter((l) => /: warning:/.test(l));
    const summary = [
        `# compile check: ${name}`,
        '',
        `cargo check -p pyright: ${r.status === 0 ? 'pass' : 'FAIL'} (${errors.length} error lines, ${warnings.length} warning lines)`,
        `files placed: ${placed.length}; ported crate files used instead of placeholders: ${keptReal.join(', ') || 'none'}`,
        `stubs not placed because the crate file is already ported: ${superseded.join(', ') || 'none'}`,
        `unported items a merge changed in ported files: ${placement.merged.join('; ') || 'none'}`,
        `ported code a merge would drop (the stub was not placed): ${placement.lost.join('; ') || 'none'}`,
        `merge warnings (kept as is or not placed, for the file's owner): ${placement.warnings.join('; ') || 'none'}`,
        `items missing from ported files, served from placeholders (needed from their owners): ${filled.join('; ') || 'none'}`,
        `jsrt items shimmed (needed from the jsrt owner): ${shimmed.join(', ') || 'none'}`,
        `jsrt items with no shim: ${missing.join(', ') || 'none'}`,
        '',
        ...errors.slice(0, 200),
    ].join('\n');
    writeFileSync(path.join(runDir, 'CHECK.md'), summary + '\n');
    writeFileSync(path.join(scratch, 'cargo-check.log'), output);
    console.log(summary.split('\n').slice(0, 8).join('\n'));
    if (errors.length) {
        console.log(errors.slice(0, 30).join('\n'));
    }
    return r.status === 0 && placement.lost.length === 0;
}

// Writes a run's stubs into crates/pyright/src, merging ported functions (see placeStubs).
export function installRun(repoRoot: string, runDir: string): boolean {
    const src = path.join(repoRoot, 'crates', 'pyright', 'src');
    // Merge into a scratch copy first, so a failed merge leaves the crate untouched.
    const scratch = path.join(repoRoot, 'target', 'stubgen-install', path.basename(runDir));
    rmSync(scratch, { recursive: true, force: true });
    mkdirSync(scratch, { recursive: true });
    const p = placeStubs(runDir, src, scratch, true);
    if (p.lost.length) {
        console.error(`install: not installed; ported code a merge would drop:\n${p.lost.join('\n')}`);
        return false;
    }
    let written = 0;
    for (const rel of p.placed) {
        const dest = path.join(src, rel);
        const text = readFileSync(path.join(scratch, rel));
        // An unchanged file is not rewritten.
        if (!existsSync(dest) || !readFileSync(dest).equals(text)) {
            mkdirSync(path.dirname(dest), { recursive: true });
            writeFileSync(dest, text);
            written++;
        }
        ensureModDecl(src, rel);
    }
    console.log(`installed ${p.placed.length} files into crates/pyright/src, ${written} changed`);
    for (const m of p.merged) {
        console.log(`  ${m}`);
    }
    if (p.warnings.length) {
        console.log(`merge warnings (kept as is or not placed, for the file's owner):\n  ${p.warnings.join('\n  ')}`);
    }
    return true;
}
