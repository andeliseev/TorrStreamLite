#!/usr/bin/env python3
"""Write THIRD_PARTY_NOTICES.txt: the licenses and copyright notices of the crates in the server.

  python3 scripts/notices.py          write THIRD_PARTY_NOTICES.txt
  python3 scripts/notices.py --check  fail if THIRD_PARTY_NOTICES.txt isn't what it would write

cargo-about, with about.toml, finds each crate's license files. Of the crates it lists, this
keeps the ones linked into the server for the targets about.toml names, not the procedural
macros and build scripts that only run while it's built, and writes each license text once,
with the crates it covers. It fails when a linked crate has no license text of its own.
Needs cargo-about (see CARGO_ABOUT below) and network access for the crates' sources.
"""
import argparse
import difflib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import textwrap
import tomllib

ROOT = Path(__file__).resolve().parents[1]
OUTPUT = ROOT / 'THIRD_PARTY_NOTICES.txt'
# Other versions may read license files differently. Keep in step with the Release workflow.
CARGO_ABOUT = '0.9.3'
INSTALL = f'sh scripts/cargo.sh install cargo-about --locked --version {CARGO_ABOUT} --features cli'

# Licenses for crates that ship no license file of their own: (SPDX id, files in this repository).
# The vendored engine's crates have upstream's notice at its workspace root, which refers to
# the Apache-2.0 text. The other two files are from the crates' repositories, at the commits
# the published versions were made from.
VENDORED = ROOT / 'vendor/rqbit'
VENDORED_LICENSES = ('Apache-2.0', ['vendor/rqbit/LICENSE', 'licenses/Apache-2.0.txt'])
MISSING_LICENSES = {
    # https://github.com/boinkor-net/governor/blob/9f3a79dd47dd32acd589c562b8d4fefe99b93372/LICENSE
    'governor': ('MIT', ['licenses/governor.txt']),
    # https://github.com/rodrimati1992/assert_cfg/blob/4799cdc4e5e1a94ef6bdb24d8c5e7a71c72e2601/LICENSE-ZLIB.md
    'assert_cfg': ('Zlib', ['licenses/assert_cfg.txt']),
}

INTRO = [
    "TorrStream Lite's own code is under the MIT license, copyright (c) 2026 Andrei Eliseev; "
    "see LICENSE. It's built with the open-source Rust crates listed below, which keep their "
    "own licenses: the {count} crates compiled into its server, in the Mac app and in the "
    "Keenetic router build.",
    "The torrent engine is librqbit (https://github.com/ikatson/rqbit), copyright 2021 Igor "
    "Katson, under the Apache License, Version 2.0. TorrStream Lite includes its crates from "
    "commit f23bacdddcc5318caa813418b8eb1b1d7307faaa, with changes for streaming from a fixed "
    "RAM cache. They're marked \"in vendor/rqbit\" below: that folder of TorrStream Lite's "
    "source code (https://github.com/andeliseev/TorrStreamLite) holds them, and "
    "vendor/UPSTREAM.md lists the changes. The source of every other crate is at "
    "https://crates.io/crates/<name>/<version>.",
    "TorrStream Lite is inspired by TorrServer (https://github.com/YouROK/TorrServer) and "
    "contains none of its code.",
    "After the list, each license text appears once, under the names of the crates it covers.",
]
WIDTH = 76
RULE = '=' * 60


def fail(message):
    if os.environ.get('GITHUB_ACTIONS') == 'true':
        print('::error::' + message.replace('\n', '%0A'), flush=True)
    sys.exit(message)


def cargo(*args, check=True):
    """Runs cargo through scripts/cargo.sh, which picks the project's toolchain, from the root."""
    result = subprocess.run(['sh', str(ROOT / 'scripts/cargo.sh'), *args],
                            stdout=subprocess.PIPE, text=True)
    if result.returncode and check:
        fail(f'cargo {" ".join(args[:2])} failed (exit code {result.returncode}).')
    return result.stdout if result.returncode == 0 else ''


def version_key(version):
    return [(0, int(part)) if part.isdigit() else (1, part) for part in re.split(r'[.+-]', version)]


def crate_key(crate):
    return crate[0], version_key(crate[1])


def normalize(text):
    text = text.lstrip('\ufeff').replace('\r\n', '\n').replace('\r', '\n')
    return '\n'.join(line.rstrip() for line in text.split('\n')).strip('\n')


def wrap(text):
    return textwrap.fill(text, WIDTH, break_long_words=False, break_on_hyphens=False)


def crate_names(crates):
    # A no-break space keeps each name on the line with its version.
    return wrap('Used by: ' + ', '.join(f'{name}\xa0{version}' for name, version in crates)
                ).replace('\xa0', ' ')


def vendored(package):
    return Path(package['manifest_path']).resolve().is_relative_to(VENDORED)


def linked_crates(targets, root_name):
    """Every crate compiled into the server for any of the targets, as (name, version)."""
    crates = set()
    for target in targets:
        tree = cargo('tree', '--locked', '--edges', 'normal,no-proc-macro', '--target', target,
                     '--prefix', 'none', '--format', '{p}')
        for line in filter(None, tree.splitlines()):
            name, version = line.split()[:2]
            if name != root_name:
                crates.add((name, version.removeprefix('v')))
    return crates


def missing_license(package):
    """The (SPDX id, files) to use for a crate that ships no license file, if there are any."""
    if vendored(package):
        return VENDORED_LICENSES
    return MISSING_LICENSES.get(package['name'])


def collect(about, linked):
    """Groups the linked crates by license text. Returns (sections, errors)."""
    sections = {}  # text with whitespace collapsed -> {'names', 'texts': {text: crates}}
    errors = []
    reported = set()
    used = set()

    def add(text, license_name, crate):
        text = normalize(text)
        section = sections.setdefault(' '.join(text.split()), {'names': set(), 'texts': {}})
        section['names'].add(license_name)
        section['texts'].setdefault(text, set()).add(crate)

    for license in about['licenses']:
        for user in license['used_by']:
            package = user['crate']
            crate = (package['name'], package['version'])
            if crate not in linked:
                continue
            if license['source_path'] is not None:
                add(license['text'], license['name'], crate)
                continue
            # cargo-about found no file and used the license's standard text, without the
            # crate's copyright notice.
            entry = missing_license(package)
            if entry is None:
                errors.append(
                    f'{crate[0]} {crate[1]} has no {license["id"]} license file that cargo-about '
                    'can read. Add a clarification to about.toml for a file it ships, or its '
                    'license from its source repository to MISSING_LICENSES in scripts/notices.py.')
                reported.add(crate)
                continue
            used.add(package['name'])
            spdx, files = entry
            if spdx != license['id']:
                errors.append(f'{crate[0]} {crate[1]} is under {license["id"]}, but '
                              f'scripts/notices.py has {spdx} files for it.')
                reported.add(crate)
                continue
            for file in files:
                add((ROOT / file).read_text(encoding='utf-8'), license['name'], crate)

    covered = {crate for section in sections.values() for crates in section['texts'].values()
               for crate in crates}
    for name, version in sorted(linked - covered - reported, key=crate_key):
        errors.append(f'{name} {version} has no license text.')
    for name in sorted(set(MISSING_LICENSES) - used):
        errors.append(f'{name} no longer needs its entry in MISSING_LICENSES in scripts/notices.py, '
                      'and its file in licenses/ can go.')
    return sections, errors


def render(about, linked, sections):
    packages = {(c['package']['name'], c['package']['version']): c for c in about['crates']}
    title = 'TorrStream Lite third-party notices'
    lines = [title, '=' * len(title), '']
    for paragraph in INTRO:
        lines += [wrap(paragraph.format(count=len(linked))), '']
    lines += ['Crates', '------', '']
    for crate in sorted(linked, key=crate_key):
        package = packages[crate]
        where = ', in vendor/rqbit' if vendored(package['package']) else ''
        lines.append(f'{crate[0]} {crate[1]}: {package["license"]}{where}')
    lines += ['', '', 'License texts', '-------------']

    for section in sections.values():
        section['crates'] = sorted({c for crates in section['texts'].values() for c in crates},
                                   key=crate_key)
        # Texts that differ only in spacing are one text; show the first crate's.
        section['text'] = min(section['texts'],
                              key=lambda t: (min(crate_key(c) for c in section['texts'][t]), t))

    def order(section):
        # By license, then crate; a crate's shorter texts, like a notice, before longer ones.
        text = section['text']
        return sorted(section['names']), crate_key(section['crates'][0]), len(text), text

    for section in sorted(sections.values(), key=order):
        lines += ['', RULE, ', '.join(sorted(section['names'])), crate_names(section['crates']),
                  '', section['text']]
    return '\n'.join(lines) + '\n'


def generate():
    version = (cargo('about', '--version', check=False).split() or [''])[-1]
    if version != CARGO_ABOUT:
        found = f', not {version}' if version else ''
        fail(f'This needs cargo-about {CARGO_ABOUT}{found}. Install it with: {INSTALL}')
    with tempfile.TemporaryDirectory(prefix='torrstream-notices-') as tmp:
        output = Path(tmp) / 'about.json'
        cargo('about', 'generate', '--locked', '--fail', '--format', 'json', '-o', str(output))
        about = json.loads(output.read_text(encoding='utf-8'))
    config = tomllib.loads((ROOT / 'about.toml').read_text(encoding='utf-8'))
    root_name = tomllib.loads((ROOT / 'Cargo.toml').read_text(encoding='utf-8'))['package']['name']
    linked = linked_crates(config['targets'], root_name)
    listed = {(c['package']['name'], c['package']['version']) for c in about['crates']}
    unlisted = sorted(linked - listed, key=crate_key)
    if unlisted:
        fail('cargo-about missed crates the server links: '
             + ', '.join(f'{name} {version}' for name, version in unlisted))
    sections, errors = collect(about, linked)
    if errors:
        fail('Can\'t write complete notices:\n' + '\n'.join('- ' + error for error in errors))
    return render(about, linked, sections)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--check', action='store_true',
                        help='fail if THIRD_PARTY_NOTICES.txt is out of date instead of writing it')
    args = parser.parse_args()
    text = generate()
    if not args.check:
        OUTPUT.write_bytes(text.encode('utf-8'))
        print(f'Wrote {OUTPUT.relative_to(ROOT)}')
        return
    current = OUTPUT.read_bytes().decode('utf-8') if OUTPUT.exists() else ''
    if current != text:
        diff = difflib.unified_diff(current.splitlines(), text.splitlines(),
                                    'THIRD_PARTY_NOTICES.txt', 'generated', lineterm='', n=1)
        print('\n'.join(list(diff)[:80]))
        fail('THIRD_PARTY_NOTICES.txt is out of date: run python3 scripts/notices.py and commit it.')
    print('THIRD_PARTY_NOTICES.txt is up to date')


if __name__ == '__main__':
    main()
