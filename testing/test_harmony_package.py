"""Offline Harmony portable-package validation regressions."""
import importlib.util
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[1]

class HarmonyPackageTests(unittest.TestCase):
    def test_source_snapshot_keeps_bytes_on_native_windows(self):
        import tempfile
        import zipfile
        from unittest.mock import patch
        spec = importlib.util.spec_from_file_location('release_package', ROOT/'scripts/release_package.py')
        assert spec is not None and spec.loader is not None
        mod = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(mod)
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)/'input'
            root.mkdir()
            (root/'Cargo.toml').write_bytes(b'fixture source bytes\n')
            temporary=Path(folder)/'stage'
            temporary.mkdir()
            output=Path(folder)/'source.zip'
            with patch.object(mod,'ROOT',root), patch.object(mod.subprocess,'check_output',side_effect=[b'Cargo.toml\0',b'1700000000\n']):
                mod.source_snapshot({'packages':[]},'Fixture',temporary,output)
            with zipfile.ZipFile(output) as archive:
                self.assertEqual(archive.read('Fixture-source/Cargo.toml'),b'fixture source bytes\n')

    def test_notice_hash_inputs_disable_checkout_newline_conversion(self):
        import subprocess
        value = subprocess.check_output(['git','check-attr','text','--','packaging/licenses/SPDX-MIT.txt'],cwd=ROOT,text=True)
        self.assertTrue(value.rstrip().endswith(': unset'), value)

    def test_corresponding_source_includes_compiled_theme_fixture_assets(self):
        spec = importlib.util.spec_from_file_location('release_package', ROOT/'scripts/release_package.py')
        assert spec is not None and spec.loader is not None
        mod = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(mod)
        self.assertIn('examples', mod.SOURCE_DIRECTORIES)
        for name in ('flake.nix', 'flake.lock', 'CONTRIBUTING.md'):
            self.assertIn(name, mod.SOURCE_FILES)

    def test_identity_must_match_clean_committed_running_build(self):
        spec = importlib.util.spec_from_file_location('harmony_package', ROOT/'scripts/package-harmony.py')
        assert spec is not None and spec.loader is not None
        mod = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(mod)
        stamp = {'version':'1.0.0','build_id':1,'commit':'a'*40,'dirty':False,'platform':'windows','architecture':'x86_64','upstream':'Yapfast 0.3.0','license':'GPL-3.0-or-later'}
        mod.validate_stamp(stamp, 'a'*40, '1.0.0')
        for change in ({'dirty':True},{'commit':'b'*40},{'build_id':0},{'platform':'linux'},{'license':'MIT'},{'build_id':True}):
            with self.subTest(change=change), self.assertRaises(ValueError):
                mod.validate_stamp(dict(stamp,**change),'a'*40,'1.0.0')

if __name__=='__main__':
    unittest.main()
