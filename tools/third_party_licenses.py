#!/usr/bin/env python3
"""Print the license texts of the crates linked into pyright-rs, for release tarballs.

Usage: tools/third_party_licenses.py > THIRD-PARTY-LICENSES.txt
Takes each crate's MIT text where the crate offers MIT, else every license file it ships.
Build scripts and proc macros are left out: they are not linked into the binary.
"""

import json
import os
import subprocess

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
LICENSE_PREFIXES = ('LICENSE', 'LICENCE', 'COPYING', 'UNLICENSE', 'NOTICE')


def cargo(*args: str) -> str:
    return subprocess.run(['cargo', *args], cwd=ROOT, check=True, capture_output=True, text=True).stdout


def main() -> None:
    linked = set(
        cargo('tree', '-p', 'pyright', '-e', 'normal,no-proc-macro', '--prefix', 'none', '-f', '{p}').split('\n')
    )
    metadata = json.loads(cargo('metadata', '--format-version', '1', '--filter-platform', 'x86_64-unknown-linux-gnu'))
    workspace = set(metadata['workspace_members'])
    for package in sorted(metadata['packages'], key=lambda p: p['name']):
        if package['id'] in workspace or f'{package["name"]} v{package["version"]}' not in linked:
            continue
        directory = os.path.dirname(package['manifest_path'])
        files = sorted(f for f in os.listdir(directory) if f.upper().startswith(LICENSE_PREFIXES))
        mit = [f for f in files if 'MIT' in f.upper() or 'UNICODE' in f.upper()]
        chosen = mit if any('MIT' in f.upper() for f in mit) else files
        if not chosen:
            raise SystemExit(f'{package["name"]}: no license file in {directory}')
        print('=' * 80)
        print(f'{package["name"]} {package["version"]} ({package["license"]})')
        print(package.get('repository') or '')
        for name in chosen:
            with open(os.path.join(directory, name), encoding='utf-8') as f:
                print(f'\n--- {name}\n')
                print(f.read().rstrip())
        print()


if __name__ == '__main__':
    main()
