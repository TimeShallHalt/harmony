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
        (payload/'yapfast.exe').write_bytes(b'fixture package content, never executed')
        (payload/'LICENSE.txt').write_bytes((ROOT/'LICENSE').read_bytes())
        output=root/'Fixture.msi'
        result=module.build_msi(payload,{'version':'1.0.0','build_id':1},output,Path(os.environ['HARMONY_WIX_DLL']),Path(os.environ['HARMONY_WIX_UI']),test_namespace=uuid.uuid4())
        self.assertTrue(output.is_file())
        self.assertEqual(output.read_bytes()[:8],b'\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1')
        self.assertEqual(result['msi_version'],'1.0.1')
        msi=c.WinDLL('msi',use_last_error=True)
        msi.MsiOpenDatabaseW.argtypes=[w.LPCWSTR,w.LPCWSTR,c.POINTER(w.UINT)];msi.MsiOpenDatabaseW.restype=w.UINT
        msi.MsiCloseHandle.argtypes=[w.UINT]
        db=w.UINT()
        self.assertEqual(msi.MsiOpenDatabaseW(str(output),None,c.byref(db)),0)
        msi.MsiCloseHandle(db)

if __name__=='__main__':unittest.main()
