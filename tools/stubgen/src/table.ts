// Loads typemap.toml (hand-written rules) and numbers.toml (one Rust type per TypeScript `number` site).

import { readFileSync, existsSync } from 'node:fs';
import { parse } from 'smol-toml';

export type Mode = 'handle' | 'value' | 'owned' | 'opaque';

export interface TypeEntry {
    rust: string;
    // Rust text in parameter position. Defaults to `&<rust>` unless `copy`.
    param: string | undefined;
    copy: boolean;
    // Paths to import where the type is used.
    uses: string[];
}

export interface Family {
    name: string;
    file: string;
    base: string;
    union: string | undefined;
    kind: string | undefined;
    flatten: string | undefined;
    detailsField: string;
    allFieldsMutable: boolean;
    rootVariant: string | undefined;
}

export interface Factory {
    name: string;
    file: string;
    func: string;
    iface: string | undefined;
    dropParams: string[];
    selfNames: string[];
    wrappers: string[];
}

export interface TraitEntry {
    receiver: string;
}

export interface Table {
    types: Map<string, TypeEntry>;
    modes: Map<string, Mode>;
    traits: Map<string, TraitEntry>;
    families: Family[];
    merge: Map<string, string>;
    factories: Factory[];
    sites: Map<string, string>;
    mutable: Map<string, boolean>;
    flagEnums: Map<string, boolean>;
    skip: Set<string>;
}

export interface Run {
    name: string;
    files: string[];
}

type Obj = Record<string, unknown>;

function isObj(v: unknown): v is Obj {
    return typeof v === 'object' && v !== null && !Array.isArray(v);
}

function fail(where: string, what: string): never {
    throw new Error(`${where}: ${what}`);
}

function str(v: unknown, where: string): string {
    return typeof v === 'string' ? v : fail(where, 'expected a string');
}

function optStr(v: unknown, where: string): string | undefined {
    return v === undefined ? undefined : str(v, where);
}

function bool(v: unknown, where: string, dflt: boolean): boolean {
    if (v === undefined) {
        return dflt;
    }
    return typeof v === 'boolean' ? v : fail(where, 'expected a boolean');
}

function strList(v: unknown, where: string): string[] {
    if (v === undefined) {
        return [];
    }
    if (!Array.isArray(v)) {
        fail(where, 'expected a list of strings');
    }
    return v.map((x, i) => str(x, `${where}[${i}]`));
}

function table(v: unknown, where: string): Obj {
    if (v === undefined) {
        return {};
    }
    return isObj(v) ? v : fail(where, 'expected a table');
}

function checkKeys(o: Obj, where: string, allowed: string[]) {
    for (const k of Object.keys(o)) {
        if (!allowed.includes(k)) {
            fail(where, `unknown key "${k}"`);
        }
    }
}

function readToml(path: string): Obj {
    const parsed: unknown = parse(readFileSync(path, 'utf8'));
    return isObj(parsed) ? parsed : fail(path, 'expected a table');
}

export function loadTable(path: string): Table {
    const root = readToml(path);
    checkKeys(root, path, ['types', 'modes', 'traits', 'families', 'merge', 'factories', 'sites', 'mutable', 'enums', 'skip']);

    const types = new Map<string, TypeEntry>();
    for (const [k, v] of Object.entries(table(root.types, 'types'))) {
        const where = `types."${k}"`;
        if (typeof v === 'string') {
            types.set(k, { rust: v, param: undefined, copy: false, uses: [] });
        } else {
            const o = table(v, where);
            checkKeys(o, where, ['rust', 'param', 'copy', 'use']);
            const u = o.use;
            types.set(k, {
                rust: str(o.rust, where),
                param: optStr(o.param, where),
                copy: bool(o.copy, where, false),
                uses: typeof u === 'string' ? [u] : strList(u, where),
            });
        }
    }

    const modes = new Map<string, Mode>();
    for (const [k, v] of Object.entries(table(root.modes, 'modes'))) {
        const m = str(v, `modes."${k}"`);
        if (m !== 'handle' && m !== 'value' && m !== 'owned' && m !== 'opaque') {
            fail(`modes."${k}"`, 'expected "handle", "value", "owned" or "opaque"');
        }
        modes.set(k, m);
    }

    const traits = new Map<string, TraitEntry>();
    for (const [k, v] of Object.entries(table(root.traits, 'traits'))) {
        const o = table(v, `traits."${k}"`);
        checkKeys(o, `traits."${k}"`, ['receiver']);
        traits.set(k, { receiver: optStr(o.receiver, `traits."${k}"`) ?? '&self' });
    }

    const families: Family[] = [];
    for (const [name, v] of Object.entries(table(root.families, 'families'))) {
        const where = `families.${name}`;
        const o = table(v, where);
        checkKeys(o, where, ['file', 'base', 'union', 'kind', 'flatten', 'details_field', 'all_fields_mutable', 'root_variant']);
        families.push({
            name,
            file: str(o.file, where),
            base: str(o.base, where),
            union: optStr(o.union, where),
            kind: optStr(o.kind, where),
            flatten: optStr(o.flatten, where),
            detailsField: optStr(o.details_field, where) ?? 'd',
            allFieldsMutable: bool(o.all_fields_mutable, where, false),
            rootVariant: optStr(o.root_variant, where),
        });
    }

    const merge = new Map<string, string>();
    for (const [k, v] of Object.entries(table(root.merge, 'merge'))) {
        merge.set(k, str(v, `merge."${k}"`));
    }

    const factories: Factory[] = [];
    for (const [name, v] of Object.entries(table(root.factories, 'factories'))) {
        const where = `factories.${name}`;
        const o = table(v, where);
        checkKeys(o, where, ['file', 'function', 'interface', 'drop_params', 'self_names', 'wrappers']);
        factories.push({
            name,
            file: str(o.file, where),
            func: str(o.function, where),
            iface: optStr(o.interface, where),
            dropParams: strList(o.drop_params, where),
            selfNames: strList(o.self_names, where),
            wrappers: strList(o.wrappers, where),
        });
    }

    const sites = new Map<string, string>();
    for (const [k, v] of Object.entries(table(root.sites, 'sites'))) {
        sites.set(k, str(v, `sites."${k}"`));
    }

    const mutable = new Map<string, boolean>();
    for (const [k, v] of Object.entries(table(root.mutable, 'mutable'))) {
        mutable.set(k, bool(v, `mutable."${k}"`, false));
    }

    const flagEnums = new Map<string, boolean>();
    const enums = table(root.enums, 'enums');
    checkKeys(enums, 'enums', ['flags', 'plain']);
    for (const k of strList(enums.flags, 'enums.flags')) {
        flagEnums.set(k, true);
    }
    for (const k of strList(enums.plain, 'enums.plain')) {
        flagEnums.set(k, false);
    }

    const skip = new Set(strList(root.skip, 'skip'));

    return { types, modes, traits, families, merge, factories, sites, mutable, flagEnums, skip };
}

export function loadRuns(path: string): Run[] {
    const root = readToml(path);
    const runs: Run[] = [];
    for (const [name, v] of Object.entries(table(root.runs, 'runs'))) {
        const o = table(v, `runs.${name}`);
        checkKeys(o, `runs.${name}`, ['files']);
        runs.push({ name, files: strList(o.files, `runs.${name}.files`) });
    }
    return runs;
}

// numbers.toml: `["<file>"]` tables of `"<site>" = "<rust type>"`. A trailing "?" marks a guess that
// nobody has reviewed yet.
export interface NumberEntry {
    rust: string;
    reviewed: boolean;
    note: string;
}

export class NumberTable {
    readonly entries = new Map<string, Map<string, NumberEntry>>();
    readonly seen = new Set<string>();
    added = 0;

    constructor(readonly path: string) {
        if (!existsSync(path)) {
            return;
        }
        const root = readToml(path);
        for (const [file, v] of Object.entries(root)) {
            const sites = new Map<string, NumberEntry>();
            for (const [site, t] of Object.entries(table(v, file))) {
                const text = str(t, `${file}."${site}"`);
                const reviewed = !text.endsWith('?');
                sites.set(site, { rust: reviewed ? text : text.slice(0, -1), reviewed, note: '' });
            }
            this.entries.set(file, sites);
        }
    }

    lookup(file: string, site: string, guess: () => [string, string]): string {
        let sites = this.entries.get(file);
        if (!sites) {
            sites = new Map();
            this.entries.set(file, sites);
        }
        this.seen.add(`${file}\0${site}`);
        const found = sites.get(site);
        if (found) {
            if (!found.reviewed) {
                found.note = guess()[1];
            }
            return found.rust;
        }
        const [rust, note] = guess();
        sites.set(site, { rust, reviewed: false, note });
        this.added++;
        return rust;
    }

    render(): string {
        const lines = [
            '# Rust type of every TypeScript `number` site the stub generator has mapped.',
            '# Tables are TypeScript files; keys are "<Container>.<member>[.<param>|.return][#n]",',
            '# where #n counts the n-th number inside one site, from 0, and #0 is left out.',
            '# A value ending in "?" is the generator\'s guess. Drop the "?" once you have checked it.',
            '# The generator rewrites this file: it keeps every value and regenerates the comments.',
            '',
        ];
        for (const file of [...this.entries.keys()].sort()) {
            const sites = this.entries.get(file);
            if (!sites || sites.size === 0) {
                continue;
            }
            lines.push(`["${file}"]`);
            for (const [site, e] of sites) {
                const note = e.note ? `  # ${e.note}` : '';
                lines.push(`"${site}" = "${e.rust}${e.reviewed ? '' : '?'}"${note}`);
            }
            lines.push('');
        }
        return lines.join('\n');
    }

    // Unreviewed sites this run mapped, in the given files and in all others.
    unreviewed(files: Set<string>): [number, number] {
        let inFiles = 0;
        let other = 0;
        for (const [file, sites] of this.entries) {
            for (const [site, e] of sites) {
                if (!e.reviewed && this.seen.has(`${file}\0${site}`)) {
                    if (files.has(file)) {
                        inFiles++;
                    } else {
                        other++;
                    }
                }
            }
        }
        return [inFiles, other];
    }
}
