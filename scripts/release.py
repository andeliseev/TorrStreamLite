#!/usr/bin/env python3
"""Release helper for the Release workflow (.github/workflows/release.yml).

  python3 scripts/release.py next --bump patch    last release and next version, as last=/version= lines
  python3 scripts/release.py next --version 0.3.0 an exact version wins over the bump
  python3 scripts/release.py set 0.3.0            set the version in Cargo.toml, Cargo.lock and Info.plist
  python3 scripts/release.py changelog 0.3.0      move CHANGELOG.md's Unreleased notes under 0.3.0 and print them

The last release is the highest vX.Y.Z tag on GitHub. Release tags only appear once a
release is published, so a run that failed is simply retried and gets the same version.
"""
import argparse
import datetime
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


def latest_release(tags):
    published = [parse(tag[1:]) for tag in tags if tag.startswith('v') and VERSION.fullmatch(tag[1:])]
    return max(published, default=None)


def next_version(current, tags, bump='patch', exact=''):
    current_version = parse(current)
    latest = latest_release(tags)
    if exact:
        chosen = parse(exact)
    elif latest is None:
        chosen = current_version
    else:
        parts = list(latest)
        position = {'major': 0, 'minor': 1, 'patch': 2}[bump]
        parts[position] += 1
        parts[position + 1:] = [0] * (2 - position)
        # After a failed run Cargo.toml may already be ahead of the last release.
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


def release_notes(text, version, date):
    """Move the Unreleased notes under "## version (date)" and return (new text, notes).

    A version that already has a section (a retried release) is left as it is, and
    its notes are returned again. A new, empty Unreleased section stays on top.
    """
    parse(version)
    lines = text.split('\n')
    headings = [i for i, line in enumerate(lines) if line.startswith('## ')]

    def notes_of(heading):
        end = next((i for i in headings if i > heading), len(lines))
        return '\n'.join(lines[heading + 1:end]).strip()

    existing = re.compile(r'## ' + re.escape(version) + r'( |$)')
    for heading in headings:
        if existing.match(lines[heading]):
            return text, notes_of(heading)
    if '## Unreleased' not in lines:
        raise ValueError('CHANGELOG.md has no "## Unreleased" section')
    unreleased = lines.index('## Unreleased')
    notes = notes_of(unreleased)
    lines[unreleased:unreleased + 1] = ['## Unreleased', '', f'## {version} ({date})']
    return '\n'.join(lines), notes


def remote_tags():
    output = subprocess.check_output(['git', 'ls-remote', '--tags', 'origin'], cwd=ROOT, text=True)
    return [line.split('refs/tags/', 1)[1] for line in output.splitlines()
            if 'refs/tags/' in line and not line.endswith('^{}')]


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = parser.add_subparsers(dest='command', required=True)
    choose = commands.add_parser('next', help='print the last release and the next version')
    choose.add_argument('--bump', choices=['patch', 'minor', 'major'], default='patch')
    choose.add_argument('--version', default='')
    update = commands.add_parser('set', help='set the app version')
    update.add_argument('version')
    notes = commands.add_parser('changelog', help='move the Unreleased notes under a version and print them')
    notes.add_argument('version')
    args = parser.parse_args()
    if args.command == 'set':
        set_version(ROOT, args.version)
    elif args.command == 'changelog':
        path = ROOT / 'CHANGELOG.md'
        today = datetime.datetime.now(datetime.timezone.utc).date().isoformat()
        text, release = release_notes(path.read_text(), args.version, today)
        path.write_text(text)
        print(release)
    else:
        tags = remote_tags()
        current = tomllib.loads((ROOT / 'Cargo.toml').read_text())['package']['version']
        latest = latest_release(tags)
        print('last=' + ('.'.join(map(str, latest)) if latest else 'none'))
        print('version=' + next_version(current, tags, args.bump, args.version))


if __name__ == '__main__':
    main()
