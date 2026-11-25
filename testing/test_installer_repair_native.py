"""Exercise the actual Windows Setup /REPAIR entry point with isolated, synthetic programs."""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import uuid
import winreg
import zipfile

ROOT = Path(__file__).resolve().parents[1]


def fake_release(folder, version, build):
    folder.mkdir(parents=True)
    source = folder / 'fixture.cs'
    source.write_text(
        'using System.Reflection;\n'
        '[assembly: AssemblyProduct("Harmony")]\n'
        f'[assembly: AssemblyInformationalVersion("{version}")]\n'
        f'[assembly: AssemblyFileVersion("{version}.0")]\n'
        'class Stub { static void Main() { System.Threading.Thread.Sleep(120000); } }\n', encoding='utf-8')
    program = folder / 'Harmony.exe'
    csc = Path(os.environ['SystemRoot']) / 'Microsoft.NET/Framework64/v4.0.30319/csc.exe'
    subprocess.run([str(csc), '/nologo', '/target:winexe', '/out:' + str(program), str(source)], check=True)
    sha = lambda p: hashlib.sha256(p.read_bytes()).hexdigest()
    prefix = f'Harmony-{version}-b{build}'
    stamp = {'version': version, 'build_id': build, 'executable_sha256': sha(program)}
    (folder / 'BUILD.json').write_text(json.dumps(stamp), encoding='utf-8')
    (folder / 'LICENSE.txt').write_text('Synthetic fixture only.\n', encoding='utf-8')
    portable = folder / (prefix + '-windows-x64.zip')
    with zipfile.ZipFile(portable, 'w') as archive:
        for name in ['Harmony.exe', 'BUILD.json', 'LICENSE.txt']:
            archive.write(folder / name, portable.stem + '/' + name)
    sources = folder / (prefix + '-source.zip')
    with zipfile.ZipFile(sources, 'w') as archive:
        archive.write(source, 'fixture.cs')
    manifest = folder / 'release-artifacts.json'
    manifest.write_text(json.dumps(dict(stamp, commit='a'*40, executable={'sha256': sha(program)},
                                        artifacts=[{'path':str(p), 'sha256':sha(p)} for p in (portable, sources)])))
    return manifest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--allow-private-install', action='store_true', required=True)
    parser.add_argument('--tooling-receipt', type=Path)
    args = parser.parse_args()
    spec = importlib.util.spec_from_file_location('builder', ROOT / 'scripts/build-harmony-installer.py')
    builder = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(builder)
    uid = str(uuid.uuid4()).upper()
    fixture = builder.SCRATCH / ('hr-' + uid[:8])
    fixture.mkdir()
    print('Private fixture ' + str(fixture), flush=True)
    manifest = fake_release(fixture / 'bundle', '1.2.0', 2)
    package = builder.build(manifest, fixture / 'compiled', fixture, uid, args.tooling_receipt)
    installer = str(package['installer'])
    target = fixture / 'program'
    releases = fixture / 'legacy/releases'
    user_key = r'Software\Harmony\InstallerTests' + '\\' + uid
    discord = user_key + r'\Software\Classes\discord\shell\open\command'
    app_key = r'Software\Microsoft\Windows\CurrentVersion\Uninstall\{' + uid + '}_is1'
    pin_args = '--data-dir "' + str(fixture / 'personal data 日本') + '"'
    run_env = dict(os.environ, HARMONY_FIXTURE_ROOT=str(fixture), HARMONY_FIXTURE_ARGS=pin_args,
                   HARMONY_LINK_SOURCE=str(ROOT / 'packaging/windows/repair/ShellLink.cs'))
    def shell(code):
        return subprocess.check_output(['powershell.exe', '-NoProfile', '-NonInteractive', '-Command',
            "$ErrorActionPreference='Stop'; $f=$env:HARMONY_FIXTURE_ROOT; Add-Type -Path $env:HARMONY_LINK_SOURCE; " + code],
            env=run_env, encoding='utf-8', errors='replace', creationflags=subprocess.CREATE_NO_WINDOW).strip()
    def setup(label, repair=False, success=True):
        log = fixture / (label + '.log')
        command = [installer, '/CURRENTUSER', '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/DIR=' + str(target), '/LOG=' + str(log)]
        if repair:
            command.append('/REPAIR')
        result = subprocess.run(command, timeout=90)
        assert (result.returncode == 0) == success, (label, result.returncode, log.read_text(encoding='utf-8-sig',errors='replace')[-2800:])
        return log.read_text(encoding='utf-8-sig', errors='replace')
    def registration():
        try:
            with winreg.OpenKey(winreg.HKEY_CURRENT_USER, app_key) as key:
                return Path(winreg.QueryValueEx(key, 'InstallLocation')[0])
        except FileNotFoundError:
            return None
    def legacy(source_dir, version, build):
        dest = releases / f'{version}-b{build}'
        dest.mkdir(parents=True)
        for name in ['Harmony.exe','BUILD.json','LICENSE.txt']:
            shutil.copyfile(source_dir / name, dest / name)
        info = json.loads((dest / 'BUILD.json').read_text())
        info['build_id'] = build
        (dest / 'BUILD.json').write_text(json.dumps(info))
        return dest
    def read_handler():
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, discord) as key:
            return winreg.QueryValueEx(key, '')[0]
    for name in ['shortcuts','common-shortcuts','pins','desktop']:
        (fixture / name).mkdir()
    setup('fresh')
    assert registration() == target
    old = legacy(fixture / 'bundle', '1.2.0', 1)
    foreign = old / 'notes.keep'
    foreign.write_text('keep')
    run_env['HARMONY_FIXTURE_OLD'] = str(old / 'Harmony.exe')
    old_exe = str(old / 'Harmony.exe')
    suffix = '  ' + pin_args + ' --open-link "%1"'
    with winreg.CreateKey(winreg.HKEY_CURRENT_USER, discord) as key:
        winreg.SetValueEx(key, '', 0, winreg.REG_SZ, '"' + old_exe + '"' + suffix)
    shell(r'''
function Link($rel,$exe,$argsText) {
    $ln=[Harmony.Setup.ShellLink]::new((Join-Path $f $rel)); $ln.TargetPath=$exe; $ln.Arguments=$argsText
    $ln.WorkingDirectory=Split-Path $exe; $ln.Save()
}
Link 'shortcuts\Harmony.lnk' $env:HARMONY_FIXTURE_OLD ''
Link 'shortcuts\Harmony Preview.lnk' $env:HARMONY_FIXTURE_OLD '--preview'
Link 'pins\Harmony.lnk' $env:HARMONY_FIXTURE_OLD $env:HARMONY_FIXTURE_ARGS
Link 'desktop\Harmony Preview.lnk' "$env:SystemRoot\explorer.exe" ''
Link 'common-shortcuts\Harmony.lnk' (Join-Path $f 'program\Harmony.exe') ''
''')
    pin = r"[Harmony.Setup.ShellLink]::new((Join-Path $f 'pins\Harmony.lnk'))"
    with subprocess.Popen([old_exe]) as running_old:
        try:
            assert 'Quit the older or affected' in setup('running-old', True, False)
            assert Path(shell('(' + pin + ').TargetPath')) == Path(old_exe)
        finally:
            running_old.terminate()
    def repaired():
        assert not (old / 'Harmony.exe').exists() and foreign.read_text() == 'keep'
        assert not (fixture / 'shortcuts/Harmony.lnk').exists()
        assert not (fixture / 'shortcuts/Harmony Preview.lnk').exists()
        assert (fixture / 'desktop/Harmony Preview.lnk').is_file()
        assert Path(shell('(' + pin + ').TargetPath')) == target / 'Harmony.exe'
        assert shell('(' + pin + ').Arguments -eq $env:HARMONY_FIXTURE_ARGS') == 'True'
        assert read_handler() == '"' + str(target / 'Harmony.exe') + '"' + suffix
    with subprocess.Popen([str(target / 'Harmony.exe')]) as running_keeper:
        try:
            assert 'removed=1' in setup('repair-duplicates', True)
            repaired()
            assert 'launchers=0' in setup('repair-again', True)
            assert running_keeper.poll() is None and registration() == target
        finally:
            running_keeper.terminate()
    print('PASS older running copy blocked, running keeper survived, duplicates repaired, foreign files and arguments preserved, second Repair made no changes', flush=True)
    # A fresh normal update must also retire older releases automatically.
    shutil.copyfile(fixture / 'bundle/Harmony.exe', old / 'Harmony.exe')
    info = json.loads((fixture / 'bundle/BUILD.json').read_text())
    info['build_id'] = 1
    (old / 'BUILD.json').write_text(json.dumps(info))
    assert 'removed=1' in setup('normal-update')
    assert not (old / 'Harmony.exe').exists() and registration() == target
    print('PASS normal update consolidated an older deployment', flush=True)
    # Version 1.10.0 must outrank 1.2.0, even when only the older release is registered.
    newer_manifest = fake_release(fixture / 'future', '1.10.0', 10)
    future = legacy(newer_manifest.parent, '1.10.0', 10)
    assert 'newer Harmony' in setup('downgrade-blocked', success=False)
    assert registration() == target and (future / 'Harmony.exe').is_file()
    assert 'removed=1' in setup('keep-newer', True)
    assert registration() is None and not (target / 'Harmony.exe').exists() and (future / 'Harmony.exe').is_file()
    assert read_handler() == '"' + str(future / 'Harmony.exe') + '"' + suffix
    print('PASS older installer refused downgrade and Repair kept the newest numeric version', flush=True)
    # The last keeper is an isolated portable. Its dev directory contains test evidence.
    with winreg.OpenKey(winreg.HKEY_CURRENT_USER, user_key) as key:
        assert winreg.QueryInfoKey(key)[0] == 1
    for path in (discord, discord.rsplit('\\',1)[0], discord.rsplit('\\',2)[0], discord.rsplit('\\',3)[0],
                 discord.rsplit('\\',4)[0], discord.rsplit('\\',5)[0], user_key):
        winreg.DeleteKey(winreg.HKEY_CURRENT_USER, path)


if __name__ == '__main__':
    main()
