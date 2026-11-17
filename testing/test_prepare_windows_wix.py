"""Offline fixtures for the pinned build-tool downloader; no code is executed."""
import base64
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import zipfile

ROOT = Path(__file__).resolve().parents[1]


class PrepareWixTests(unittest.TestCase):
    def module(self):
        spec = importlib.util.spec_from_file_location('prepare_wix', ROOT/'scripts/prepare-windows-wix.py')
        assert spec is not None and spec.loader is not None
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        return module

    def archive(self, entries):
        output = io.BytesIO()
        with zipfile.ZipFile(output, 'w') as archive:
            for name, value in entries.items():
                # ZipInfo's Windows constructor normalizes backslashes; retain
                # the exact malicious member spelling this fixture must test.
                entry = zipfile.ZipInfo('placeholder')
                entry.filename = name
                entry.orig_filename = name
                archive.writestr(entry, value)
        return output.getvalue()

    def invoke(self, module, root, *, bad_path=None, bad_hash=False):
        packages = {
            'wix': self.archive({'tools/net6.0/any/wix.dll': b'explicit non-executable CLI fixture'}),
            'wixtoolset.ui.wixext': self.archive({'wixext4/WixToolset.UI.wixext.dll': b'explicit non-executable UI fixture'}),
        }
        if bad_path is not None:
            packages['wix'] = self.archive({bad_path: b'must never escape'})
        hashes = {name: base64.b64encode(hashlib.sha512(data).digest()).decode('ascii') for name, data in packages.items()}
        if bad_hash:
            hashes['wix'] = 'deliberately mismatched fixture hash'
        def fetch(url, timeout):
            self.assertEqual(timeout, 90)
            self.assertTrue(url.startswith('https://api.nuget.org/v3-flatcontainer/'))
            return io.BytesIO(packages['wix' if '/wix/' in url else 'wixtoolset.ui.wixext'])
        output = root/'tools'
        with patch.object(module, 'PACKAGES', hashes), patch.object(module.urllib.request, 'urlopen', side_effect=fetch), \
                patch.dict(os.environ, {'TMPDIR': str(root)}), patch('sys.argv', ['prepare-windows-wix.py', '--output', str(output)]):
            module.main()
        return output

    def test_verified_packages_are_staged_but_never_executed(self):
        module = self.module()
        with tempfile.TemporaryDirectory(dir=os.environ['TMPDIR']) as temporary:
            root = Path(temporary)
            output = self.invoke(module, root)
            info = json.loads((output/'tooling-provenance.json').read_text(encoding='utf-8'))
            self.assertEqual(info['version'], '4.0.6')
            self.assertFalse(info['executed'])
            self.assertTrue(info['antivirus_scan_required_before_execution'])
            self.assertEqual(len(info['packages']), 2)
            self.assertTrue(Path(info['wix_dll']).is_file())
            self.assertTrue(Path(info['ui_extension']).is_file())

    def test_bad_hash_never_creates_the_tool_destination(self):
        module = self.module()
        with tempfile.TemporaryDirectory(dir=os.environ['TMPDIR']) as temporary:
            root = Path(temporary)
            with self.assertRaisesRegex(ValueError, 'SHA-512 mismatch'):
                self.invoke(module, root, bad_hash=True)
            self.assertFalse((root/'tools').exists())

    def test_even_integrity_valid_archives_cannot_escape_or_introduce_windows_paths(self):
        module = self.module()
        with tempfile.TemporaryDirectory(dir=os.environ['TMPDIR']) as temporary:
            root = Path(temporary)
            for name in ('../escaped.txt', '/escaped.txt', 'C:/escaped.txt', 'nested\\escaped.txt'):
                with self.subTest(name=name), self.assertRaisesRegex(ValueError, 'Unsafe'):
                    self.invoke(module, root, bad_path=name)
                self.assertFalse((root/'tools').exists())
                self.assertFalse((root/'escaped.txt').exists())

    def test_existing_tool_directory_is_not_overwritten(self):
        module = self.module()
        with tempfile.TemporaryDirectory(dir=os.environ['TMPDIR']) as temporary:
            root = Path(temporary)
            output = root/'tools'
            output.mkdir()
            marker = output/'owned.txt'
            marker.write_text('keep existing', encoding='utf-8')
            with self.assertRaises(SystemExit):
                self.invoke(module, root)
            self.assertEqual(marker.read_text(encoding='utf-8'), 'keep existing')


if __name__ == '__main__':
    unittest.main()
