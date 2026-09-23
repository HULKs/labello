"""Regression checks for wiki boundaries, navigation, and repeatable publication."""

import contextlib
import io
import json
import subprocess
import tempfile
import unittest
from pathlib import Path

import docs


class WikiTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name) / 'repo'
        self.root.mkdir()
        subprocess.run(['git', 'init', '-q', str(self.root)], check=True)
        (self.root / 'docs/archive').mkdir(parents=True)
        (self.root / 'docs/README.md').write_text('# Home\n\n[Guide](guide.md#details)\n')
        (self.root / 'docs/guide.md').write_text('# Guide\n\n## Details\n\n## Details\n')
        (self.root / 'docs/archive/draft.md').write_text('# Unimplemented draft\n')
        (self.root / 'code.rs').write_text('// code\n')
        (self.root / 'icon.svg').write_text('<svg/>')
        self.config = {
            'repository': 'HULKs/labello', 'ref': 'main',
            'groups': [{'title': 'Guides', 'pages': [
                {'source': 'docs/README.md', 'page': 'Home'},
                {'source': 'docs/guide.md', 'page': 'Guide'},
            ]}],
        }
        self.save_config()

    def save_config(self):
        (self.root / 'docs/wiki.json').write_text(json.dumps(self.config))

    def run_build(self, output):
        with contextlib.redirect_stdout(io.StringIO()):
            docs.build(output, self.root)

    def test_wiki_links_anchors_repo_files_and_assets(self):
        _, mapping = docs.load_manifest(self.root)
        text = ('[Guide](guide.md#details-1) [Archive](archive/draft.md) '
                '[Code](../code.rs) [Local](#home) [External](https://example.com)\n'
                '<img src="../icon.svg" />\n[ref]: guide.md#details\n')
        result = docs.rewrite(self.root, self.root / 'docs/README.md', text, self.config, mapping)
        self.assertIn('(https://github.com/HULKs/labello/wiki/Guide#details-1)', result)
        self.assertIn('/blob/main/docs/archive/draft.md)', result)
        self.assertIn('/blob/main/code.rs)', result)
        self.assertIn('[Local](#home)', result)
        self.assertIn('https://raw.githubusercontent.com/HULKs/labello/main/icon.svg', result)
        self.assertIn('[ref]: https://github.com/HULKs/labello/wiki/Guide#details', result)
        self.assertIn('[External](https://example.com)', result)

    def test_code_examples_are_untouched(self):
        _, mapping = docs.load_manifest(self.root)
        text = ('```markdown\n[Example](not-a-file.md)\n```\n'
                '`[Inline](not-a-file.md)`\n~~~\n[Also code](missing.md)\n~~~\n')
        self.assertEqual(text, docs.rewrite(
            self.root, self.root / 'docs/README.md', text, self.config, mapping))

    def test_archive_and_agent_pages_cannot_be_published(self):
        for source in ('docs/archive/draft.md', 'AGENTS.md'):
            with self.subTest(source=source):
                self.config['groups'][0]['pages'].append({'source': source, 'page': 'Draft'})
                self.save_config()
                with self.assertRaises(ValueError):
                    docs.load_manifest(self.root)
                self.config['groups'][0]['pages'].pop()

    def test_missing_current_page_and_duplicate_name_fail(self):
        self.config['groups'][0]['pages'].pop()
        self.save_config()
        with self.assertRaisesRegex(ValueError, 'missing from wiki'):
            docs.load_manifest(self.root)
        self.config['groups'][0]['pages'].append({'source': 'docs/guide.md', 'page': 'home'})
        self.save_config()
        with self.assertRaisesRegex(ValueError, 'duplicate'):
            docs.load_manifest(self.root)

    def test_broken_link_anchor_and_metadata_fail(self):
        for line, expected in (
            ('[Broken](missing.md)', 'missing link target'),
            ('[Broken](guide.md#missing)', 'missing anchor'),
            ('> **Owner:** Someone', 'metadata header'),
            ('> **Last verified:** a revision', 'metadata header'),
            ('[Escape](../../outside.md)', 'escapes repository'),
        ):
            with self.subTest(line=line):
                (self.root / 'docs/README.md').write_text('# Home\n\n' + line + '\n')
                with self.assertRaisesRegex(ValueError, expected):
                    docs.check(self.root)

    def test_build_is_repeatable_preserves_unmanaged_and_removes_retired_pages(self):
        output = Path(self.temp.name) / 'wiki'
        output.mkdir()
        (output / 'Unmanaged.md').write_text('Keep this page.\n')
        (output / 'Retired.md').write_text('Previously generated.\n')
        (output / docs.GENERATED).write_text('["Retired.md"]')
        self.run_build(output)
        self.assertFalse((output / 'Retired.md').exists())
        self.assertEqual((output / 'Unmanaged.md').read_text(), 'Keep this page.\n')
        self.assertFalse((output / 'Draft.md').exists())
        self.assertIn('/wiki/Guide', (output / '_Sidebar.md').read_text())
        first = {p.name: p.read_bytes() for p in output.iterdir()}
        self.run_build(output)
        self.assertEqual(first, {p.name: p.read_bytes() for p in output.iterdir()})

    def test_output_cannot_overwrite_source_or_follow_symlinks(self):
        with self.assertRaisesRegex(ValueError, 'outside the source'):
            self.run_build(self.root / 'docs/output')
        outside = Path(self.temp.name) / 'outside'
        outside.mkdir()
        link = Path(self.temp.name) / 'linked'
        link.symlink_to(outside, target_is_directory=True)
        with self.assertRaisesRegex(ValueError, 'symlink'):
            self.run_build(link)
        (outside / 'Home.md').symlink_to(self.root / 'docs/README.md')
        with self.assertRaisesRegex(ValueError, 'unsafe wiki output'):
            self.run_build(outside)

    def test_inventory_cannot_delete_outside_output(self):
        output = Path(self.temp.name) / 'wiki'
        output.mkdir()
        (output / docs.GENERATED).write_text('["../repo/code.rs"]')
        with self.assertRaisesRegex(ValueError, 'invalid generated-page inventory'):
            self.run_build(output)
        self.assertTrue((self.root / 'code.rs').is_file())

    def test_heading_anchors_ignore_code_and_preserve_repeated_headings(self):
        self.assertEqual(
            {'guide', 'details', 'details-1', 'details-1-1', 'named'},
            docs.anchors('# Guide\n## Details\n## Details\n## Details-1\n'
                         '```\n## Ignored\n```\n<a id="named"></a>\n'),
        )


if __name__ == '__main__':
    unittest.main()
