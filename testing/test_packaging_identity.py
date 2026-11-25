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
VERSION = tomllib.loads((ROOT / 'Cargo.toml').read_text(encoding='utf-8'))['package']['version']


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
        if '-' not in VERSION:
            self.assertEqual(component.find('releases/release').get('version'), VERSION)
        self.assertFalse((ROOT / 'packaging/linux/io.github.yapfast.Yapfast.desktop').exists())
        self.assertFalse((ROOT / 'packaging/linux/io.github.yapfast.Yapfast.metainfo.xml').exists())

    def test_windows_resources_and_setup_target_exact_binary_name(self):
        rc = (ROOT / 'packaging/windows/harmony.rc').read_text(encoding='utf-8')
        self.assertIn('1 ICON "harmony.ico"', rc)
        for field, value in (('ProductName', 'Harmony'), ('FileDescription', 'Harmony'), ('OriginalFilename', 'Harmony.exe')):
            self.assertIn(f'VALUE "{field}", "{value}\\0"', rc)
        self.assertIn('wiredtenshi', rc)
        fixed_version = VERSION.split('-', 1)[0].replace('.', ',') + ',0'
        self.assertIn('FILEVERSION ' + fixed_version, rc)
        self.assertIn('PRODUCTVERSION ' + fixed_version, rc)
        self.assertIn('VALUE "FileVersion", "' + VERSION + '\\0"', rc)
        self.assertIn('VALUE "ProductVersion", "' + VERSION + '\\0"', rc)
        recipe = (ROOT / 'packaging/windows/harmony-simple.iss').read_text(encoding='utf-8')
        self.assertIn('AppName=Harmony', recipe)
        self.assertIn('OutputBaseFilename=Harmony-{#AppVersion}-Setup', recipe)
        self.assertIn('LicenseFile={#PayloadDir}\\LICENSE.txt', recipe)
        self.assertIn('UninstallDisplayIcon={app}\\Harmony.exe', recipe)
        self.assertIn('SetupIconFile=harmony.ico', recipe)
        self.assertIn('CloseApplications=no', recipe)
        self.assertIn('RestartApplications=no', recipe)
        self.assertNotIn('19FF8D53-8D7E-4E66-91BB-E2F5BE307E9A', recipe)
        sections = {line.strip() for line in recipe.splitlines() if line.startswith('[')}
        self.assertFalse(sections & {'[Registry]', '[Run]', '[UninstallRun]'})
        self.assertFalse((ROOT / 'packaging/windows/yapfast.rc').exists())
        self.assertFalse((ROOT / 'packaging/windows/yapfast.iss').exists())

    def test_default_setup_build_uses_the_pinned_local_compiler(self):
        module = load_script('build-harmony-installer')
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            compiler = root / '.local/tools/inno-7.1.1-x64/compiler/ISCC.exe'
            compiler.parent.mkdir(parents=True)
            compiler.write_bytes(b'fixture compiler, never executed')
            receipt = compiler.parent.parent / 'tooling-provenance.json'
            output = root / 'compiled'
            def stage(manifest, payload):
                payload.mkdir()
                executable = payload / 'Harmony.exe'
                executable.write_bytes(b'fixture application, never executed')
                return {'version': VERSION, 'build_id': 1, 'commit': 'a' * 40}, {'Harmony.exe': module.digest(executable)}
            def compile_fixture(command, **kwargs):
                self.assertEqual(command[0], str(compiler))
                self.assertEqual(command[-1], str(root / 'packaging/windows/harmony-simple.iss'))
                self.assertIn('--define=AppVersion=' + VERSION, command)
                (output / f'Harmony-{VERSION}-Setup.exe').write_bytes(b'fixture installer')
                return subprocess.CompletedProcess(command, 0)
            with patch.object(module, 'ROOT', root), patch.object(module, 'stage', side_effect=stage), \
                 patch.object(module, 'build_repair', return_value=root / 'HarmonyRepair.exe'), \
                 patch.object(module.subprocess, 'run', side_effect=compile_fixture) as run:
                with self.assertRaises(FileNotFoundError):
                    module.build(root / 'release.json', output)
                provenance = {'version': '7.1.1', 'compiler': str(compiler),
                              'compiler_scan': {'file_sha256': {'ISCC.exe': '0' * 64}}}
                receipt.write_text(json.dumps(provenance), encoding='utf-8')
                with self.assertRaisesRegex(ValueError, 'Pinned compiler file changed'):
                    module.build(root / 'release.json', output)
                run.assert_not_called()
                provenance['compiler_scan']['file_sha256']['ISCC.exe'] = module.digest(compiler)
                receipt.write_text(json.dumps(provenance), encoding='utf-8')
                result = module.build(root / 'release.json', output)
                self.assertEqual(result['app_id'], module.PUBLIC_ID)
                self.assertIsNone(result['fixture_root'])
                self.assertEqual(Path(result['installer']).name, f'Harmony-{VERSION}-Setup.exe')
                run.assert_called_once()

    def test_arch_and_appimage_keep_harmony_paths_and_disable_update_feeds(self):
        arch = (ROOT / 'packaging/arch/PKGBUILD').read_text(encoding='utf-8')
        self.assertIn('pkgname=harmony', arch)
        if '-' not in VERSION:
            self.assertIn(f'pkgver={VERSION}\n', arch)
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
                 patch.object(module.os, 'environ', dict(os.environ, SKIA_BINARIES_URL=(root / 'skia.tar.gz').as_uri(),
                                                        LK_CUSTOM_WEBRTC=str(root / 'webrtc'))):
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
        # Let the shell allocate the scratch directory on its own filesystem.
        # WSL can read the checkout from NTFS but needs its Linux /tmp for a case-only rename.
        script = '''set -euo pipefail
out=$(mktemp -d "${TMPDIR:-/tmp}/harmony-nix-install.XXXXXXXX")
trap 'rm -r -- "$out"' EXIT
mkdir "$out/bin"
printf 'fixture binary' > "$out/bin/Harmony"
if [[ -e "$out/bin/harmony" ]]; then exit 77; fi
wrapProgram() {
  mv "$1" "${1%/*}/.${1##*/}-wrapped"
  printf '# wrapper fixture, never executed\\n' > "$1"
}
''' + body + f'''
test ! -e "$out/bin/Harmony"
test -f "$out/bin/harmony"
test "$(cat "$out/bin/.harmony-wrapped")" = 'fixture binary'
cmp "packaging/linux/{APP_ID}.desktop" "$out/share/applications/{APP_ID}.desktop"
cmp LICENSE "$out/share/licenses/harmony/LICENSE"
'''
        result = subprocess.run(['bash'], input=script.encode(), capture_output=True, timeout=30, cwd=ROOT)
        if result.returncode == 77:
            self.skipTest('Linux install fixture requires a case-sensitive shell scratch directory')
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

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

    def test_current_portable_manifest_feeds_the_simple_installer(self):
        common = load_script('release_package')
        module = load_script('package-harmony')
        setup = load_script('build-harmony-installer')
        msi = load_script('package_harmony_msi')
        with tempfile.TemporaryDirectory(prefix='harmony-package-fixture-') as folder:
            root = Path(folder)
            (root / 'Cargo.toml').write_text(f'[package]\nname="yapfast"\nversion="{VERSION}"\n', encoding='utf-8')
            (root / 'LICENSE').write_bytes((ROOT / 'LICENSE').read_bytes())
            raw = bytearray(80)
            raw[:2] = b'MZ'
            struct.pack_into('<I', raw, 60, 64)
            raw[64:70] = b'PE\0\0\x64\x86'
            (root / 'fixture.exe').write_bytes(raw)  # PE-header fixture, not a runnable production build.
            stamp = dict(version=VERSION, build_id=1, commit='a' * 40, dirty=False, platform='windows',
                         architecture='x86_64', upstream='Yapfast 0.4.1', license='GPL-3.0-or-later')
            (root / 'stamp.json').write_text(json.dumps(stamp), encoding='utf-8')
            metadata = {'packages': [{'name': 'yapfast', 'version': VERSION, 'manifest_path': str(root / 'Cargo.toml')}]}
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
            staged = root / 'setup-stage'
            info, files = setup.stage(manifest, staged)
            self.assertEqual(info, receipt)
            self.assertIn('Harmony.exe', files)
            self.assertNotIn('yapfast.exe', files)
            self.assertEqual((staged / 'Harmony.exe').read_bytes(), raw)
            source_name = f'Harmony-{VERSION}-b1-source.zip'
            self.assertEqual(files['corresponding-source/' + source_name], module.digest(output / source_name))
            self.assertEqual((staged / 'LICENSE.txt').read_bytes(), (ROOT / 'LICENSE').read_bytes())
            self.assertIn('Upstream: Yapfast 0.4.1', (staged / 'SOURCE.txt').read_text())
            self.assertIn('licenses/fixture.txt', files)
            self.assertIn('installer-source/harmony-simple.iss', files)
            self.assertIs(json.loads((staged / 'BUILD.json').read_text())['updater_included'], True)
            with self.assertRaisesRegex(ValueError, 'Inno Setup'):
                msi.stage_release(manifest, root / 'msi-stage')
            self.assertFalse((root / 'msi-stage').exists())
            with zipfile.ZipFile(output / f'Harmony-{VERSION}-b1-windows-x64.zip') as archive:
                self.assertIsNone(archive.testzip())
                names = archive.namelist()
                self.assertIn(f'Harmony-{VERSION}-b1-windows-x64/Harmony.exe', names)
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
                result = subprocess.run(['bash', '-n'], input=(ROOT / 'scripts' / name).read_text().encode(),
                                        capture_output=True, timeout=20)
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
            major, minor, patch = map(int, VERSION.split('-', 1)[0].split('.'))
            product = (major << 16 | minor, patch << 16)
            self.assertEqual(fixed[2:6], product + product)


if __name__ == '__main__':
    unittest.main()
