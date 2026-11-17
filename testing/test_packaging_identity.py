"""Offline product-identity contracts; all package bytes here are test fixtures."""
import configparser
import contextlib
import fnmatch
import importlib.util
import io
import json
import os
from pathlib import Path
import re
import shutil
import struct
import subprocess
import sys
import tempfile
import tomllib
import unittest
from unittest.mock import patch
import xml.etree.ElementTree as ET
import zipfile

ROOT = Path(__file__).resolve().parents[1]
APP_ID = 'io.github.TimeShallHalt.Harmony'


def load_script(name):
    spec = importlib.util.spec_from_file_location(name.replace('-', '_'), ROOT / 'scripts' / (name + '.py'))
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class PackagingIdentityTests(unittest.TestCase):
    def test_linux_desktop_and_appstream_agree_on_fork_identity(self):
        desktop = configparser.ConfigParser(interpolation=None)
        desktop.read(ROOT / 'packaging/linux' / (APP_ID + '.desktop'), encoding='utf-8')
        entry = desktop['Desktop Entry']
        self.assertEqual(entry['Name'], 'Harmony')
        self.assertEqual(entry['Exec'], 'harmony --open-link %u')
        self.assertEqual(entry['Icon'], APP_ID)
        self.assertEqual(entry['StartupWMClass'], APP_ID)
        self.assertEqual(entry['MimeType'], 'x-scheme-handler/harmony;')
        component = ET.parse(ROOT / 'packaging/linux' / (APP_ID + '.metainfo.xml')).getroot()
        self.assertEqual(component.findtext('id'), APP_ID)
        self.assertEqual(component.findtext('name'), 'Harmony')
        self.assertEqual(component.findtext('launchable'), APP_ID + '.desktop')
        self.assertEqual(component.findtext('project_license'), 'GPL-3.0-or-later')
        self.assertIn('wiredtenshi', ET.tostring(component, encoding='unicode'))
        self.assertFalse((ROOT / 'packaging/linux/io.github.yapfast.Yapfast.desktop').exists())
        self.assertFalse((ROOT / 'packaging/linux/io.github.yapfast.Yapfast.metainfo.xml').exists())

    def test_windows_resources_and_optional_installer_target_exact_binary_name(self):
        rc = (ROOT / 'packaging/windows/harmony.rc').read_text(encoding='utf-8')
        self.assertIn('1 ICON "harmony.ico"', rc)
        for field, value in (('ProductName', 'Harmony'), ('FileDescription', 'Harmony'), ('OriginalFilename', 'Harmony.exe')):
            self.assertIn(f'VALUE "{field}", "{value}\\0"', rc)
        self.assertIn('wiredtenshi', rc)
        version = tomllib.loads((ROOT / 'Cargo.toml').read_text(encoding='utf-8'))['package']['version']
        self.assertIn('FILEVERSION ' + version.replace('.', ',') + ',0', rc)
        self.assertIn('PRODUCTVERSION ' + version.replace('.', ',') + ',0', rc)
        self.assertIn('VALUE "FileVersion", "' + version + '\\0"', rc)
        recipe = (ROOT / 'packaging/windows/harmony.iss').read_text(encoding='utf-8')
        self.assertIn('AppName=Harmony', recipe)
        self.assertIn('UninstallDisplayIcon={app}\\Harmony.exe', recipe)
        self.assertIn('SetupIconFile=harmony.ico', recipe)
        self.assertIn('CloseApplications=no', recipe)
        self.assertIn('RestartApplications=no', recipe)
        self.assertNotIn('19FF8D53-8D7E-4E66-91BB-E2F5BE307E9A', recipe)
        for section in ('[Registry]', '[Run]', '[UninstallRun]'):
            self.assertNotIn(section, '\n'.join(line for line in recipe.splitlines() if not line.startswith(';')))
        self.assertFalse((ROOT / 'packaging/windows/yapfast.rc').exists())
        self.assertFalse((ROOT / 'packaging/windows/yapfast.iss').exists())

    def test_offline_inno_tool_discovery_never_downloads_or_installs(self):
        common = load_script('release_package')
        with patch.dict(sys.modules, release_package=common):
            module = load_script('package-windows')
        with tempfile.TemporaryDirectory() as folder:
            prefix = Path(folder) / 'offline-tool'
            with patch.dict(os.environ, HARMONY_INNO_PREFIX=str(prefix)), patch.object(module.subprocess, 'run') as run:
                with self.assertRaisesRegex(SystemExit, 'Offline Inno Setup'):
                    module.inno_setup()
                run.assert_not_called()
                compiler = prefix / 'drive_c/InnoSetup/ISCC.exe'
                compiler.parent.mkdir(parents=True)
                compiler.write_bytes(b'offline compiler discovery fixture, never executed')
                self.assertEqual(module.inno_setup(), prefix)
                run.assert_not_called()

    def test_arch_and_appimage_keep_harmony_paths_and_disable_update_feeds(self):
        arch = (ROOT / 'packaging/arch/PKGBUILD').read_text(encoding='utf-8')
        self.assertIn('pkgname=harmony', arch)
        self.assertIn('_id=' + APP_ID, arch)
        self.assertIn('target/release/Harmony "$pkgdir/usr/bin/harmony"', arch)
        self.assertNotIn('git+https://github.com/yapfast/yapfast.git', arch)
        self.assertIn('HARMONY_SOURCE_SHA256', arch)
        self.assertIn('corresponding-source/', arch)
        linux = (ROOT / 'scripts/package-linux.py').read_text(encoding='utf-8')
        self.assertIn('base = f"Harmony-{version}"', linux)
        self.assertIn('"usr/bin/harmony"', linux)
        self.assertIn('"usr/share/doc/harmony"', linux)
        self.assertNotIn('zsyncmake', linux)
        self.assertNotIn('github.com/yapfast/yapfast/releases', linux)
        self.assertIn('if update:', linux)
        windows = (ROOT / 'scripts/package-windows.py').read_text(encoding='utf-8')
        self.assertIn('release/Harmony.exe', windows)
        self.assertIn('"run-harmony.cmd"', windows)
        self.assertNotIn('github.com/yapfast/yapfast/releases', windows)
        build = (ROOT / 'scripts/build-windows').read_text(encoding='utf-8')
        self.assertIn('--bin Harmony', build)

    def test_appimage_packager_fails_closed_on_upstream_update_feed(self):
        common = load_script('release_package')
        with patch.dict(sys.modules, release_package=common):
            module = load_script('package-linux')
        with tempfile.TemporaryDirectory(prefix='harmony-appimage-fixture-') as folder:
            root = Path(folder)
            parts = root / 'nix-output'
            parts.mkdir()
            (parts / 'build.json').write_text(json.dumps({'update_information':'gh-releases-zsync|yapfast|yapfast|latest|upstream.zsync'}))
            metadata = json.dumps({'packages':[{'name':'yapfast','version':'1.1.0'}]}).encode()
            argv = ['package-linux.py', '--output', str(root / 'artifacts')]
            with patch.object(module, 'nix_build', side_effect=[parts, root / 'skia-source']), \
                 patch.object(module.subprocess, 'check_output', return_value=metadata), \
                 patch.object(module.subprocess, 'run') as run, patch.object(sys, 'argv', argv), \
                 patch.dict(os.environ, SKIA_BINARIES_URL=(root / 'skia.tar.gz').as_uri(), LK_CUSTOM_WEBRTC=str(root / 'webrtc')):
                with self.assertRaisesRegex(SystemExit, 'must not contain upstream update information'):
                    module.main()
                run.assert_not_called()
            self.assertEqual(list((root / 'artifacts').iterdir()), [])

    def test_nix_aliases_install_case_mapping_and_update_free_appdir(self):
        root_flake = (ROOT / 'flake.nix').read_text(encoding='utf-8')
        self.assertIn('harmony = build', root_flake)
        self.assertIn('yapfast = harmony;', root_flake)
        self.assertIn('default = harmony;', root_flake)
        package = (ROOT / 'nix/package.nix').read_text(encoding='utf-8')
        self.assertIn('pname = "harmony";', package)
        self.assertIn('mainProgram = "harmony";', package)
        self.assertIn('id = "' + APP_ID + '";', package)
        self.assertLess(package.index('mv "$out/bin/Harmony" "$out/bin/harmony"'), package.index('wrapProgram "$out/bin/harmony"'))
        self.assertIn('../scripts/harmony_build_stamp.rs', package)
        self.assertIn('../examples', package)
        self.assertIn('share/licenses/harmony/LICENSE', package)
        appimage = (ROOT / 'nix/appimage.nix').read_text(encoding='utf-8')
        self.assertIn('/bin/.harmony-wrapped', appimage)
        self.assertIn('ln -s usr/bin/harmony', appimage)
        self.assertIn('updateInformation = "";', appimage)
        self.assertIn('dd if=/dev/zero', appimage)
        self.assertNotIn('gh-releases-zsync', appimage)
        self.assertNotIn('nixpkgs#zsync', (ROOT / 'scripts/build-appimage').read_text())

    @unittest.skipUnless(shutil.which('bash'), 'Requires bash for Nix install-phase fixture')
    def test_nix_install_shell_renames_before_wrapping_in_scratch(self):
        # Exercise the shell body, not Nix evaluation; the host need not have Nix.
        package = (ROOT / 'nix/package.nix').read_text(encoding='utf-8')
        body = package.split("  postInstall = ''\n", 1)[1].split("\n  '';", 1)[0]
        body = body.replace('${id}', APP_ID).replace('${lib.makeLibraryPath native.runtimeLibraries}', '/fixture/lib')
        with tempfile.TemporaryDirectory(prefix='harmony-nix-install-fixture-') as folder:
            output = Path(folder)
            (output / 'bin').mkdir()
            (output / 'bin/Harmony').write_bytes(b'fixture program, never executed')
            script = '''set -euo pipefail
wrapProgram() {
  mv "$1" "${1%/*}/.${1##*/}-wrapped"
  printf '# wrapper fixture, never executed\\n' > "$1"
}
''' + body
            result = subprocess.run(['bash'], input=script, text=True, capture_output=True, timeout=30,
                                    cwd=ROOT, env=dict(os.environ, out=str(output).replace('\\', '/')))
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertNotIn('Harmony', {path.name for path in (output / 'bin').iterdir()})
            self.assertTrue((output / 'bin/harmony').is_file())
            self.assertEqual((output / 'bin/.harmony-wrapped').read_bytes(), b'fixture program, never executed')
            self.assertTrue((output / 'share/applications' / (APP_ID + '.desktop')).is_file())
            self.assertEqual((output / 'share/licenses/harmony/LICENSE').read_bytes(), (ROOT / 'LICENSE').read_bytes())

    def test_desktop_installer_exec_quoting_preserves_special_paths_without_launch(self):
        script = (ROOT / 'scripts/install-desktop-entry').read_text(encoding='utf-8')
        # Execute only its Python entry-rendering block against a scratch XDG target,
        # never the actual shell installation or a user launcher directory.
        block = script.split("<<'PY'\n", 1)[1].split('\nPY\n', 1)[0]
        with tempfile.TemporaryDirectory(prefix='harmony-desktop-fixture-') as folder:
            root = Path(folder)
            destination = root / 'entry.desktop'
            executable = str(root / 'Harmony with spaces & 50% $cash.exe')
            argv = ['entry-renderer', executable, str(ROOT / 'packaging/linux' / (APP_ID + '.desktop')), str(destination)]
            with patch.object(sys, 'argv', argv):
                exec(compile(block, 'desktop-entry-renderer', 'exec'), {})
            text = destination.read_text(encoding='utf-8')
            self.assertIn('Name=Harmony', text)
            self.assertIn('50%%', text)
            self.assertIn('\\\\$cash', text)
            self.assertIn(' --open-link %u\n', text)
            self.assertNotIn('Exec=yapfast', text)
            self.assertFalse(Path(executable).exists(), 'The renderer must never create or run an executable')

    def test_portable_producer_manifest_round_trips_into_msi_stager(self):
        common = load_script('release_package')
        module = load_script('package-harmony')
        msi = load_script('package_harmony_msi')
        with tempfile.TemporaryDirectory(prefix='harmony-package-fixture-') as folder:
            root = Path(folder)
            (root / 'Cargo.toml').write_text('[package]\nname="yapfast"\nversion="1.1.0"\n', encoding='utf-8')
            (root / 'LICENSE').write_bytes((ROOT / 'LICENSE').read_bytes())
            raw = bytearray(80)
            raw[:2] = b'MZ'
            struct.pack_into('<I', raw, 60, 64)
            raw[64:70] = b'PE\0\0\x64\x86'
            (root / 'fixture.exe').write_bytes(raw)  # PE-header fixture, not a runnable production build.
            stamp = dict(version='1.1.0', build_id=1, commit='a' * 40, dirty=False, platform='windows',
                         architecture='x86_64', upstream='Yapfast 0.4.1', license='GPL-3.0-or-later')
            (root / 'stamp.json').write_text(json.dumps(stamp), encoding='utf-8')
            metadata = {'packages': [{'name': 'yapfast', 'version': '1.1.0', 'manifest_path': str(root / 'Cargo.toml')}]}
            (root / 'metadata.json').write_text(json.dumps(metadata), encoding='utf-8')
            def source_fixture(metadata, base, temporary, destination):
                with zipfile.ZipFile(destination, 'w') as archive:
                    archive.write(root / 'Cargo.toml', base + '-source/Cargo.toml')
                    archive.write(root / 'LICENSE', base + '-source/LICENSE')
            def notice_fixture(metadata, package, licenses, *native):
                licenses.mkdir()
                (licenses / 'fixture.txt').write_text('Unit-test notice fixture', encoding='utf-8')
                return [{'name': 'fixture-only', 'license_files': ['licenses/fixture.txt']}]
            output = root / 'artifacts'
            argv = ['package-harmony.py', '--exe', str(root / 'fixture.exe'), '--stamp', str(root / 'stamp.json'),
                    '--metadata', str(root / 'metadata.json'), '--skia-archive', str(root / 'unused-skia'),
                    '--skia-source', str(root / 'unused-skia-source'), '--webrtc', str(root / 'unused-webrtc'), '--output', str(output)]
            with patch.dict(sys.modules, release_package=common), patch.object(module, 'ROOT', root), \
                 patch.object(common, 'source_snapshot', side_effect=source_fixture), \
                 patch.object(common, 'dependency_notices', side_effect=notice_fixture), \
                 patch.object(module.subprocess, 'check_output', side_effect=['a' * 40, '']), \
                 patch.object(sys, 'argv', argv), contextlib.redirect_stdout(io.StringIO()):
                module.main()
            manifest = output / 'release-artifacts.json'
            receipt = json.loads(manifest.read_text(encoding='utf-8'))
            self.assertEqual(receipt['executable']['name'], 'Harmony.exe')
            self.assertFalse(receipt['uploaded'])
            self.assertFalse(receipt['installed'])
            self.assertEqual(len(receipt['artifacts']), 2)
            info, files = msi.stage_release(manifest, root / 'msi-stage')
            self.assertEqual(info, stamp)
            self.assertIn('Harmony.exe', files)
            self.assertNotIn('yapfast.exe', files)
            self.assertEqual((root / 'msi-stage/Harmony.exe').read_bytes(), raw)
            self.assertIn('corresponding-source/Harmony-1.1.0-b1-source.zip', files)
            self.assertIn('Upstream: Yapfast 0.4.1', (root / 'msi-stage/SOURCE.txt').read_text())
            with zipfile.ZipFile(output / 'Harmony-1.1.0-b1-windows-x64.zip') as archive:
                self.assertIsNone(archive.testzip())
                names = archive.namelist()
                self.assertIn('Harmony-1.1.0-b1-windows-x64/Harmony.exe', names)
                self.assertFalse(any(name.endswith('/yapfast.exe') for name in names))

    def test_dormant_nightly_uses_current_artifact_names_and_cannot_use_upstream_signing_keys(self):
        workflow=(ROOT / '.github/workflows/nightly.yml').read_text(encoding='utf-8')
        jobs=re.findall(r'^  ([a-z][a-z0-9_-]*):\n(.*?)(?=^  [a-z][a-z0-9_-]*:|\Z)',
                        workflow.split('jobs:\n',1)[1],flags=re.M | re.S)
        self.assertEqual({name for name,_ in jobs},{'version','linux','windows','release'})
        for name,body in jobs:
            with self.subTest(job=name):
                self.assertRegex(body,r'^    if: \$\{\{ false \}\}\n')
        for forbidden in ('Yapfast-', 'yapfast-*.pkg.tar.zst', '${{ secrets.', 'NIGHTLY_SIGNING_KEY',
                          'packaging/keys', 'minisign', 'environment: nightly', 'AppImage updaters take'):
            self.assertNotIn(forbidden,workflow)
        for required in ('target/dist/linux/Harmony-*','target/dist/linux/harmony-*.pkg.tar.zst',
                         'target/dist/discord/Harmony-*', 'dist/linux/Harmony-$VERSION-source.zip',
                         'dist/windows/Harmony-$VERSION-source.zip',
                         'mv dist/linux/Harmony-* dist/linux/harmony-*.pkg.tar.zst release/',
                         'mv dist/windows/Harmony-*-windows-x64* release/',
                         'sha256sum Harmony-* harmony-*.pkg.tar.zst',
                         '--title "Harmony $VERSION', 'no automatic update feed'):
            self.assertIn(required,workflow)
        # Exercise the upload globs against producer-named fixture members, not
        # made-up CI results. Publication remains disabled; nothing is uploaded.
        examples={
            'target/dist/linux/Harmony-*': ['Harmony-1.2.0-nightly.20261116.1-source.zip',
                                          'Harmony-1.2.0-nightly.20261116.1-x86_64.AppImage'],
            'target/dist/linux/harmony-*.pkg.tar.zst': ['harmony-1.2.0nightly.20261116.1-1-x86_64.pkg.tar.zst'],
            'target/dist/discord/Harmony-*': ['Harmony-1.2.0-nightly.20261116.1-source.zip',
                                            'Harmony-1.2.0-nightly.20261116.1-windows-x64.zip'],
        }
        for glob,names in examples.items():
            for name in names:
                with self.subTest(glob=glob,name=name):
                    self.assertTrue(fnmatch.fnmatchcase(str(Path(glob).parent / name).replace('\\','/'),glob))

    @unittest.skipUnless(shutil.which('bash'), 'Requires bash for shell syntax checks')
    def test_owned_shell_scripts_have_valid_bash_syntax(self):
        for name in ('build-windows', 'build-appimage', 'make-windows-icon', 'install-desktop-entry', 'package-arch'):
            with self.subTest(script=name):
                # Decode checkout newlines just as Git's Unix checkout does.
                result = subprocess.run(['bash', '-n'], input=(ROOT / 'scripts' / name).read_text(),
                                        text=True, capture_output=True, timeout=20)
                self.assertEqual(result.returncode, 0, result.stderr)

    @unittest.skipUnless(os.name == 'nt' and os.environ.get('HARMONY_RC_EXE'), 'Requires explicit Windows SDK resource compiler')
    def test_native_resource_compiler_embeds_harmony_version_fields(self):
        with tempfile.TemporaryDirectory(prefix='harmony-resource-fixture-') as folder:
            output = Path(folder) / 'harmony.res'
            result = subprocess.run([os.environ['HARMONY_RC_EXE'], '/nologo', '/fo', str(output),
                                     str(ROOT / 'packaging/windows/harmony.rc')],
                                    cwd=ROOT / 'packaging/windows', text=True, capture_output=True, timeout=30)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            raw = output.read_bytes()
            for value in ('Harmony', 'Harmony.exe', 'ProductName', 'OriginalFilename', 'wiredtenshi'):
                self.assertIn(value.encode('utf-16-le'), raw)
            offset = raw.index(b'\xbd\x04\xef\xfe')
            fixed = struct.unpack_from('<6I', raw, offset)
            self.assertEqual(fixed[2:6], (0x00010001, 0, 0x00010001, 0))


if __name__ == '__main__':
    unittest.main()
