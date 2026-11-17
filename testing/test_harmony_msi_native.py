"""Real WiX compilation checks; never install or launch the live application."""
import ctypes as c
from ctypes import wintypes as w
import importlib.util
import os
from pathlib import Path
import tempfile
import unittest
import uuid

ROOT=Path(__file__).resolve().parents[1]

@unittest.skipUnless(os.name=='nt' and os.environ.get('HARMONY_WIX_DLL'),'Requires explicit private WiX tooling on Windows')
class NativeMsiBuildTests(unittest.TestCase):
    def test_real_wix_builds_a_readable_per_user_x64_msi(self):
        spec=importlib.util.spec_from_file_location('harmony_msi',ROOT/'scripts/package_harmony_msi.py')
        assert spec and spec.loader
        module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
        self.assertTrue(callable(getattr(module,'build_msi',None)),'Actual WiX/MSI compilation is not implemented')
        root=Path(tempfile.mkdtemp(prefix='harmony-msi-native-',dir=os.environ['TMPDIR']))
        payload=root/'payload';payload.mkdir()
        (payload/'Harmony.exe').write_bytes(b'fixture package content, never executed')
        (payload/'LICENSE.txt').write_bytes((ROOT/'LICENSE').read_bytes())
        output=root/'Fixture.msi'
        namespace=uuid.uuid4()
        result=module.build_msi(payload,{'version':'1.1.0','build_id':1},output,Path(os.environ['HARMONY_WIX_DLL']),Path(os.environ['HARMONY_WIX_UI']),test_namespace=namespace)
        self.assertTrue(output.is_file())
        self.assertEqual(output.read_bytes()[:8],b'\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1')
        self.assertEqual(result['msi_version'],'1.1.1')
        msi=c.WinDLL('msi',use_last_error=True)
        msi.MsiOpenDatabaseW.argtypes=[w.LPCWSTR,w.LPCWSTR,c.POINTER(w.UINT)];msi.MsiOpenDatabaseW.restype=w.UINT
        msi.MsiCloseHandle.argtypes=[w.UINT]
        db=w.UINT()
        self.assertEqual(msi.MsiOpenDatabaseW(str(output),None,c.byref(db)),0)
        msi.MsiCloseHandle(db)
        from testing.msi_database import Database
        with Database(output) as database:
            filenames = {row[0].split('|')[-1] for row in database.query('SELECT `FileName` FROM `File`')}
            self.assertIn('Harmony.exe', filenames)
            self.assertNotIn('yapfast.exe', filenames)
            properties = dict(database.query('SELECT `Property`, `Value` FROM `Property`'))
            self.assertEqual(properties['MSIDISABLERMRESTART'], '1')
            self.assertNotIn('ALLUSERS', properties)
            conditions = dict(database.query('SELECT `Condition`, `Description` FROM `LaunchCondition`'))
            self.assertIn(module.LEGACY_MIGRATION_CONDITION, conditions)
            self.assertIn('before removing the old product', conditions[module.LEGACY_MIGRATION_CONDITION])
            self.assertIn('HARMONY_LEGACY_MSI', properties['SecureCustomProperties'].split(';'))
            self.assertIn('HARMONY_LEGACY_COMPONENT', properties['SecureCustomProperties'].split(';'))
            upgrades = database.query('SELECT `UpgradeCode`, `VersionMax`, `Attributes`, `ActionProperty` FROM `Upgrade`')
            legacy = [row for row in upgrades if row[3] == 'HARMONY_LEGACY_MSI']
            self.assertEqual(len(legacy), 1)
            self.assertEqual(legacy[0][0].strip('{}').lower(), str(namespace))
            self.assertEqual(legacy[0][1], '1.1.1')
            self.assertEqual(int(legacy[0][2]) & 2, 2, 'OnlyDetect must not remove old products')
            self.assertEqual(int(legacy[0][2]) & 512, 0, 'First rebranded MSI must be excluded')
            locators = database.query('SELECT `Root`, `Key`, `Name`, `Type` FROM `RegLocator`')
            marker = [row for row in locators if row[2] == 'C_bb51d6588f1a73f021a9113e']
            self.assertEqual(len(marker), 1)
            self.assertEqual(int(marker[0][0]), 1, 'Search must use HKCU only')
            self.assertEqual(marker[0][1], rf'Software\Harmony\InstallerTests\{namespace}\Components')
            self.assertEqual(int(marker[0][3]), 18, 'Raw value in 64-bit registry view')
            for table in ('InstallUISequence', 'InstallExecuteSequence'):
                sequence = dict(database.query(f'SELECT `Action`, `Sequence` FROM `{table}`'))
                self.assertLess(int(sequence['FindRelatedProducts']), int(sequence['LaunchConditions']))
                self.assertLess(int(sequence['AppSearch']), int(sequence['LaunchConditions']))
                if table == 'InstallExecuteSequence':
                    self.assertLess(int(sequence['LaunchConditions']), int(sequence['InstallInitialize']))
                    self.assertLess(int(sequence['InstallInitialize']), int(sequence['RemoveExistingProducts']))

if __name__=='__main__':unittest.main()
