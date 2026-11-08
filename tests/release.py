#!/usr/bin/env python3
import importlib.util
from pathlib import Path
import plistlib
import shutil
import tempfile
import tomllib
import unittest

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('release_helper', ROOT / 'scripts/release.py')
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)


class ReleaseTests(unittest.TestCase):
    def test_selects_first_release_and_numeric_tag_order(self):
        self.assertEqual(release.next_version('0.2.1', []), '0.2.1')
        self.assertEqual(release.next_version('0.2.1', ['v0.2.9', 'v0.2.10', 'test']), '0.2.11')
        self.assertEqual(release.next_version('0.3.0', ['v0.2.9']), '0.3.0')
        self.assertEqual(release.next_version('0.2.1', ['v0.2.1'], 'minor'), '0.3.0')
        self.assertEqual(release.next_version('0.2.1', ['v0.2.1'], 'major'), '1.0.0')
        self.assertEqual(release.next_version('0.2.1', [], exact='0.3.2'), '0.3.2')

    def test_rejects_replacement_downgrade_and_invalid_versions(self):
        for exact in ['0.1.9', 'v0.3.0', '01.2.0', '1.2', '1.2.3\n', '1.2.3; echo unsafe']:
            with self.assertRaises(ValueError):
                release.next_version('0.2.1', [], exact=exact)
        with self.assertRaises(ValueError):
            release.next_version('0.2.1', ['v0.3.0'], exact='0.3.0')

    def test_updates_only_app_versions_and_is_idempotent(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / 'macos').mkdir()
            for name in ('Cargo.toml', 'Cargo.lock', 'macos/Info.plist'):
                shutil.copyfile(ROOT / name, root / name)
            before = tomllib.loads((root / 'Cargo.lock').read_text())['package']
            release.set_version(root, '0.3.1')
            release.set_version(root, '0.3.1')
            after = tomllib.loads((root / 'Cargo.lock').read_text())['package']
            self.assertEqual([p for p in before if p['name'] != 'torrstream-lite'],
                             [p for p in after if p['name'] != 'torrstream-lite'])
            self.assertEqual(next(p['version'] for p in after if p['name'] == 'torrstream-lite'), '0.3.1')
            self.assertEqual(tomllib.loads((root / 'Cargo.toml').read_text())['package']['version'], '0.3.1')
            plist = plistlib.loads((root / 'macos/Info.plist').read_bytes())
            self.assertEqual(plist['CFBundleShortVersionString'], '0.3.1')
            self.assertEqual(plist['CFBundleVersion'], '0.3.1')

    def test_changelog_moves_unreleased_notes_once(self):
        text = ('# Changelog\n\nIntro.\n\n## Unreleased\n\n- Added a thing.\n- Fixed a thing.\n\n'
                '## 0.2.2 (2026-11-08)\n\n- Older.\n')
        updated, notes = release.release_notes(text, '0.3.0', '2026-11-10')
        self.assertEqual(notes, '- Added a thing.\n- Fixed a thing.')
        self.assertIn('## Unreleased\n\n## 0.3.0 (2026-11-10)\n\n- Added a thing.', updated)
        self.assertIn('## 0.2.2 (2026-11-08)\n\n- Older.', updated)
        # A retried release reuses its section and changes nothing.
        self.assertEqual(release.release_notes(updated, '0.3.0', '2026-11-11'), (updated, notes))
        # The next release starts from the empty Unreleased section.
        self.assertEqual(release.release_notes(updated, '0.3.1', '2026-11-12')[1], '')
        with self.assertRaises(ValueError):
            release.release_notes('# Changelog\n', '0.3.0', '2026-11-10')
        with self.assertRaises(ValueError):
            release.release_notes(text, 'v0.3.0', '2026-11-10')

    def test_repository_changelog_has_unreleased_section(self):
        self.assertIn('\n## Unreleased\n', (ROOT / 'CHANGELOG.md').read_text())


if __name__ == '__main__':
    unittest.main()
