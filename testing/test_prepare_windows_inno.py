"""Offline Inno tool-preparer tests. Mock downloads/native commands; never execute fixtures."""
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]


@unittest.skipUnless(os.name == 'nt', 'Windows-only portable compiler preparation')
class PrepareInnoTests(unittest.TestCase):
    def setUp(self):
        spec = importlib.util.spec_from_file_location('prepare_inno_fixture',ROOT/'scripts/prepare-windows-inno.py')
        if spec is None or spec.loader is None:
            raise RuntimeError('Unable to load the real preparer')
        self.module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(self.module)
        self.temporary = tempfile.TemporaryDirectory(dir=os.environ['HARMONY_TEST_SCRATCH'])
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)

    def test_published_release_pin_and_version_are_explicit(self):
        self.assertEqual(self.module.VERSION,'7.1.1')
        self.assertEqual(self.module.SHA256,'8a5a6711c3766a6e5fb74f280ba0e3551e719ce94538d901fdbd1f8f9920b3f0')
        self.assertEqual(self.module.PUBLISHER,'Pyrsys B.V.')
        self.assertTrue(self.module.URL.startswith('https://github.com/jrsoftware/issrc/releases/download/is-7_1_1/'))

    def test_existing_tool_directory_is_never_overwritten(self):
        with patch.object(self.module.urllib.request,'urlopen') as network:
            with self.assertRaisesRegex(ValueError,'fresh tool directory'):
                self.module.prepare(self.root,self.root)
            network.assert_not_called()

    def test_hash_failure_occurs_before_any_native_command(self):
        body = b'explicit non-executable invalid download fixture'
        with patch.object(self.module,'SIZE',len(body)), patch.object(self.module.urllib.request,'urlopen',return_value=io.BytesIO(body)), \
                patch.object(self.module.subprocess,'run') as native:
            with self.assertRaisesRegex(ValueError,'integrity mismatch'):
                self.module.prepare(self.root/'tools',self.root)
            native.assert_not_called()

    def test_oversized_download_is_rejected_before_execution(self):
        with patch.object(self.module,'SIZE',5), patch.object(self.module.urllib.request,'urlopen',return_value=io.BytesIO(b'123456')), \
                patch.object(self.module.subprocess,'run') as native:
            with self.assertRaisesRegex(ValueError,'exceeds'):
                self.module.prepare(self.root/'tools',self.root)
            native.assert_not_called()

    def invoke_positive_fixture(self, *, publisher='Pyrsys B.V.', changed_integration=False):
        module = self.module
        body = b'explicit non-executable, mocked compiler-installer fixture'
        commands = []
        def native(command,**kwargs):
            commands.append(command)
            if command[0] == 'powershell.exe':
                return SimpleNamespace(returncode=0,stdout=json.dumps({'Status':'Valid','Publisher':publisher,'Thumbprint':'unit-only'}),stderr='')
            if command[0].endswith(module.ASSET):
                compiler = self.root/'tools/compiler'
                compiler.mkdir()
                (compiler/'ISCC.exe').write_bytes(b'not executable; mocked CLI only')
                (compiler/'ISCmplr.dll').write_bytes(b'not executable; mocked DLL only')
                return SimpleNamespace(returncode=0,stdout='',stderr='')
            self.assertEqual(command[-1],'--version')
            return SimpleNamespace(returncode=0,stdout='7.1.1\n',stderr='')
        fake_scan = {'exit_code':0,'unchanged':True,'fixture_only':True}
        integration = [{'unit':'before'},{'unit':'changed' if changed_integration else 'before'}]
        with patch.object(module,'SIZE',len(body)), patch.object(module,'SHA256',hashlib.sha256(body).hexdigest()), \
                patch.object(module.urllib.request,'urlopen',return_value=io.BytesIO(body)), \
                patch.object(module,'scan',return_value=fake_scan), patch.object(module,'integration_fingerprint',side_effect=integration), \
                patch.object(module.subprocess,'run',side_effect=native):
            report = module.prepare(self.root/'tools',self.root)
        return report,commands

    def test_verified_mock_uses_portable_mode_no_global_tasks_and_explicit_scratch(self):
        report,commands = self.invoke_positive_fixture()
        extraction = commands[1]
        for flag in ('/PORTABLE=1','/CURRENTUSER','/VERYSILENT','/NORESTART','/NOICONS','/TASKS='):
            self.assertIn(flag,extraction)
        self.assertTrue(report['integration_fingerprint_unchanged'])
        self.assertEqual(report['scratch'],str(self.root))
        self.assertEqual(commands[-1][-1],'--version')
        self.assertTrue((self.root/'tools/tooling-provenance.json').is_file())

    def test_wrong_publisher_cannot_reach_portable_extraction(self):
        with self.assertRaisesRegex(ValueError,'publisher'):
            self.invoke_positive_fixture(publisher='Explicitly wrong unit-test publisher')
        self.assertFalse((self.root/'tools/compiler').exists())

    def test_portable_registration_drift_invalidates_preparation(self):
        with self.assertRaisesRegex(ValueError,'registration/file associations'):
            self.invoke_positive_fixture(changed_integration=True)
        self.assertFalse((self.root/'tools/tooling-provenance.json').exists())

    def test_zero_exit_scanner_cannot_hide_changed_file(self):
        path = self.root/'scan-target.txt'
        path.write_bytes(b'before')
        def scanner(command,**kwargs):
            path.write_bytes(b'changed during mocked scan')
            return SimpleNamespace(returncode=0,stdout='unit fixture only',stderr='')
        with patch.object(self.module.subprocess,'run',side_effect=scanner):
            with self.assertRaisesRegex(ValueError,'scanned files changed'):
                self.module.scan(path,dict(os.environ))


if __name__ == '__main__':
    unittest.main()
