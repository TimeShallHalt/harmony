"""Offline packaging helper tests: synthetic headers, real files and SHA-256."""
from pathlib import Path
import hashlib
import json
import os
import subprocess
import sys
import tempfile
import unittest
ROOT = Path(__file__).resolve().parents[1]
SCRATCH = (Path(os.environ['LOCALAPPDATA']) / 'hermes/cache/scratch' if sys.platform == 'win32' else Path.home() / '.cache/hermes-scratch') / 'harmony-release-helper-tests'
SCRATCH.mkdir(parents=True, exist_ok=True)
COMMIT = '0123456789abcdef0123456789abcdef01234567'
def pe():
    b = bytearray(128); b[:2] = b'MZ'; b[60:64] = (64).to_bytes(4,'little'); b[64:68] = b'PE\0\0'; b[68:70] = (0x8664).to_bytes(2,'little'); return b
def appimage():
    b = bytearray(128); b[:4] = b'\x7fELF'; b[4:7] = bytes([2,1,1]); b[8:11] = b'AI\2'; b[18:20] = (62).to_bytes(2,'little'); return b
class ReleaseHelper(unittest.TestCase):
    def invoke(self, folder, *args):
        script = ROOT / 'scripts/harmony-release-manifest.py'
        self.assertTrue(script.is_file(), 'a real offline release-generation helper must exist')
        stamps=[]
        for i,arg in enumerate(args):
            if arg=='--artifact':
                platform=args[i+1].split('=',1)[0]
                os_name,arch=platform.split('-',1)
                stamp=folder/(platform+'.json')
                stamp.write_text(json.dumps({'version':'1.0.1','build_id':1,'commit':COMMIT,'dirty':False,'platform':os_name,'architecture':arch,'upstream':'Yapfast 0.3.0','license':'GPL-3.0-or-later'}))
                stamps+=['--build-info',f'{platform}={stamp}']
        return subprocess.run([sys.executable, str(script), '--version', '1.0.1', '--build-id', '1', '--commit', COMMIT, '--output-dir', str(folder / 'out'), *args, *stamps], capture_output=True, text=True)
    def test_generates_publishable_artifacts_manifest_and_real_checksums(self):
        with tempfile.TemporaryDirectory(dir=SCRATCH) as name:
            p=Path(name); win=p/'input.exe'; linux=p/'input.AppImage'; win.write_bytes(pe()); linux.write_bytes(appimage())
            result=self.invoke(p, '--artifact', f'windows-x86_64={win}', '--artifact', f'linux-x86_64={linux}')
            self.assertEqual(result.returncode,0,result.stderr)
            m=json.loads((p/'out/harmony-update-v1.json').read_text())
            self.assertEqual(m['repository'],'TimeShallHalt/harmony'); self.assertEqual(m['tag'],'harmony-v1.0.1-b1'); self.assertEqual(m['commit'],COMMIT)
            self.assertEqual(len(m['artifacts']),2)
            for a in m['artifacts']:
                data=(p/'out'/a['name']).read_bytes()
                self.assertEqual(a['size'],len(data)); self.assertEqual(a['sha256'],hashlib.sha256(data).hexdigest())
    def test_wrong_architecture_and_archives_cannot_generate_manifest(self):
        with tempfile.TemporaryDirectory(dir=SCRATCH) as name:
            p=Path(name); wrong=p/'wrong.exe'; wrong.write_bytes(b'PK archive fixture')
            result=self.invoke(p,'--artifact',f'windows-x86_64={wrong}')
            self.assertNotEqual(result.returncode,0); self.assertFalse((p/'out/harmony-update-v1.json').exists())
    def test_no_overwrite_and_no_duplicate_platform(self):
        with tempfile.TemporaryDirectory(dir=SCRATCH) as name:
            p=Path(name); win=p/'input.exe'; win.write_bytes(pe())
            result=self.invoke(p,'--artifact',f'windows-x86_64={win}','--artifact',f'windows-x86_64={win}')
            self.assertNotEqual(result.returncode,0)
            out=p/'out'; out.mkdir(exist_ok=True); old=out/'harmony-update-v1.json'; old.write_text('old release')
            result=self.invoke(p,'--artifact',f'windows-x86_64={win}')
            self.assertNotEqual(result.returncode,0); self.assertEqual(old.read_text(),'old release')
    def test_dirty_mismatched_or_unstamped_artifacts_cannot_be_released(self):
        import runpy
        assemble=runpy.run_path(str(ROOT/'scripts/harmony-release-manifest.py'))['assemble']
        with tempfile.TemporaryDirectory(dir=SCRATCH) as name:
            p=Path(name); win=p/'input.exe'; win.write_bytes(pe()); stamp=p/'build.json'
            base={'version':'1.0.1','build_id':1,'commit':COMMIT,'dirty':False,'platform':'windows','architecture':'x86_64','upstream':'Yapfast 0.3.0','license':'GPL-3.0-or-later'}
            for change in [{'dirty':True},{'commit':'f'*40},{'architecture':'aarch64'},{'build_id':True}]:
                stamp.write_text(json.dumps(base|change))
                with self.assertRaises(ValueError): assemble('1.0.1',1,COMMIT,[f'windows-x86_64={win}'],p/'out',[f'windows-x86_64={stamp}'])
                self.assertFalse((p/'out/harmony-update-v1.json').exists())
            with self.assertRaises(ValueError): assemble('1.0.1',1,COMMIT,[f'windows-x86_64={win}'],p/'out',[])
    def test_second_bad_artifact_rolls_back_only_new_assembly_output(self):
        with tempfile.TemporaryDirectory(dir=SCRATCH) as name:
            p=Path(name); win=p/'input.exe'; linux=p/'bad.AppImage'; win.write_bytes(pe()); linux.write_bytes(b'not an AppImage')
            result=self.invoke(p,'--artifact',f'windows-x86_64={win}','--artifact',f'linux-x86_64={linux}')
            self.assertNotEqual(result.returncode,0); self.assertEqual(list((p/'out').iterdir()),[])
    def test_cli_build_id_rejects_raw_noncanonical_tokens_before_conversion(self):
        for token in ['01', '+1', '١', '１', ' 1', '1 ', '-1', '0', '18446744073709551616']:
            with self.subTest(token=token), tempfile.TemporaryDirectory(dir=SCRATCH) as name:
                p=Path(name); win=p/'input.exe'; win.write_bytes(pe())
                result=self.invoke(p,'--build-id',token,'--artifact',f'windows-x86_64={win}')
                self.assertNotEqual(result.returncode,0,repr(token))
                self.assertFalse((p/'out/harmony-update-v1.json').exists())
    def test_cli_canonical_positive_build_id_bounds_are_preserved(self):
        for token in ['1','18446744073709551615']:
            with self.subTest(token=token), tempfile.TemporaryDirectory(dir=SCRATCH) as name:
                p=Path(name); win=p/'input.exe'; win.write_bytes(pe()); stamp=p/'stamp.json'
                stamp.write_text(json.dumps({'version':'1.0.1','build_id':int(token),'commit':COMMIT,'dirty':False,'platform':'windows','architecture':'x86_64','upstream':'Yapfast 0.3.0','license':'GPL-3.0-or-later'}))
                result=subprocess.run([sys.executable,str(ROOT/'scripts/harmony-release-manifest.py'),'--version','1.0.1','--build-id',token,'--commit',COMMIT,'--artifact',f'windows-x86_64={win}','--build-info',f'windows-x86_64={stamp}','--output-dir',str(p/'out')],capture_output=True,text=True)
                self.assertEqual(result.returncode,0,result.stderr)
                self.assertEqual(json.loads((p/'out/harmony-update-v1.json').read_text())['build_id'],int(token))
    def test_version_tokens_are_rejected_without_repair(self):
        import runpy
        assemble=runpy.run_path(str(ROOT/'scripts/harmony-release-manifest.py'))['assemble']
        with tempfile.TemporaryDirectory(dir=SCRATCH) as name:
            p=Path(name)
            for v in ['01.0.0','v1.0.0','1.0','1.0.0+hash','1.0.0-rc.1']:
                with self.assertRaises(ValueError): assemble(v,1,COMMIT,[],p/'out',[])
if __name__ == '__main__': unittest.main()
