#!/usr/bin/env python3
"""Select a release version and keep the Rust/Mac version metadata in sync."""
import argparse
from pathlib import Path
import re
import subprocess
import tomllib

ROOT = Path(__file__).resolve().parents[1]
VERSION = re.compile(r'(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)')


def parse(value):
    if not VERSION.fullmatch(value):
        raise ValueError('Use a version like 0.2.1 (no v prefix or prerelease suffix)')
    return tuple(map(int, value.split('.')))


def next_version(current, tags, bump='patch', exact=''):
    current_version = parse(current)
    published = [parse(tag[1:]) for tag in tags if tag.startswith('v') and VERSION.fullmatch(tag[1:])]
    latest = max(published, default=None)
    if exact:
        chosen = parse(exact)
    elif latest is None:
        chosen = current_version
    else:
        parts = list(latest)
        position = {'major': 0, 'minor': 1, 'patch': 2}[bump]
        parts[position] += 1
        parts[position + 1:] = [0] * (2 - position)
        chosen = max(tuple(parts), current_version)
    if chosen < current_version or (latest is not None and chosen <= latest):
        raise ValueError('Version must be at least the source version and newer than all release tags')
    return '.'.join(map(str, chosen))


def set_version(root, version):
    parse(version)
    # Edit only this package; never bump versions in the vendored engine.
    manifest = root / 'Cargo.toml'
    text = manifest.read_text()
    old = tomllib.loads(text)['package']['version']
    text, count = re.subn(r'(?m)^version = "' + re.escape(old) + '"$',
                          f'version = "{version}"', text, count=1)
    if count != 1:
        raise ValueError('Cannot locate package version')
    lock = root / 'Cargo.lock'
    lock_text, count = re.subn(r'(\[\[package\]\]\nname = "torrstream-lite"\nversion = ")[^"]+("\n)',
                               lambda m: m[1] + version + m[2], lock.read_text())
    if count != 1:
        raise ValueError('Cannot locate torrstream-lite in Cargo.lock')
    plist = root / 'macos/Info.plist'
    plist_text = plist.read_text()
    for key in ('CFBundleShortVersionString', 'CFBundleVersion'):
        plist_text, count = re.subn(r'(<key>' + key + r'</key>\s*<string>)[^<]+(</string>)',
                                    lambda m: m[1] + version + m[2], plist_text)
        if count != 1:
            raise ValueError(f'Cannot locate {key}')
    manifest.write_text(text)
    lock.write_text(lock_text)
    plist.write_text(plist_text)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest='command', required=True)
    choose = commands.add_parser('next')
    choose.add_argument('--bump', choices=['patch', 'minor', 'major'], default='patch')
    choose.add_argument('--version', default='')
    update = commands.add_parser('set')
    update.add_argument('version')
    args = parser.parse_args()
    if args.command == 'set':
        set_version(ROOT, args.version)
    else:
        tags = subprocess.check_output(['git', 'tag', '--list', 'v*'], cwd=ROOT, text=True).splitlines()
        current = tomllib.loads((ROOT / 'Cargo.toml').read_text())['package']['version']
        print(next_version(current, tags, args.bump, args.version))


if __name__ == '__main__':
    main()
