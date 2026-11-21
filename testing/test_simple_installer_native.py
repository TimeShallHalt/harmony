"""Short opt-in native smoke test of the simple installer; never runs Harmony."""
from pathlib import Path
import argparse
import importlib.util
import json
import os
import subprocess
import time
import uuid
import winreg

ROOT = Path(__file__).resolve().parents[1]
SCRATCH = Path('C:/Users/ren/AppData/Local/hermes/profiles/hex/cache/scratch')

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--allow-private-install', action='store_true', required=True)
    parser.parse_args()
    spec = importlib.util.spec_from_file_location('simple_builder', ROOT / 'scripts/build-harmony-installer.py')
    if spec is None or spec.loader is None:
        raise RuntimeError('Builder unavailable')
    builder = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(builder)
    identity = uuid.uuid4()
    fixture = SCRATCH / ('simple-installer-' + identity.hex[:12])
    fixture.mkdir()
    evidence = ROOT / '.local/evidence/simple-installer' / identity.hex[:12]
    evidence.mkdir(parents=True)
    manifest = ROOT / '.local/evidence/upstream-041-branding/release/release-artifacts.json'
    package = builder.build(manifest, evidence / 'compiled', fixture, str(identity))
    key = 'Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{' + str(identity).upper() + '}_is1'
    env = dict(os.environ)
    for name in ('TMP', 'TEMP', 'TMPDIR', 'HARMONY_TEST_SCRATCH'):
        env[name] = str(SCRATCH)
    canary = fixture / 'synthetic-user-data.bin'
    canary.write_bytes(b'SYNTHETIC DATA ONLY; NO ACCOUNT OR CREDENTIALS\n' + identity.bytes)
    canary_hash = builder.digest(canary)
    result = {'status': 'IN_PROGRESS', 'evidence': str(evidence), 'fixture': str(fixture),
              'app_id': str(identity), 'application_executed': False, 'machine_scope_exercised': False, 'cases': []}
    def save():
        (evidence / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
    def record(name):
        result['cases'].append({'case': name, 'passed': True})
        save()
        print('PASS ' + name, flush=True)
    def registration():
        try:
            with winreg.OpenKey(winreg.HKEY_CURRENT_USER, key, 0, winreg.KEY_READ) as handle:
                return {winreg.EnumValue(handle, i)[0]: winreg.EnumValue(handle, i)[1] for i in range(winreg.QueryInfoKey(handle)[1])}
        except FileNotFoundError:
            return None
    def run(command, label):
        completed = subprocess.run(command, env=env, cwd=fixture, capture_output=True, text=True,
                                   encoding='utf-8', errors='replace', timeout=150, creationflags=subprocess.CREATE_NO_WINDOW)
        (evidence / (label + '-exit.json')).write_text(json.dumps({'exit_code': completed.returncode, 'stdout': completed.stdout, 'stderr': completed.stderr}, indent=2))
        builder.require(completed.returncode == 0, label + ' failed; inspect native log')
    def install(target, label):
        run([package['installer'], '/CURRENTUSER', '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART',
             '/DIR=' + str(target), '/LOG=' + str(evidence / (label + '.log'))], label)
        for name, checksum in package['payload_files'].items():
            builder.require(builder.digest(target / name) == checksum, 'Payload differs: ' + name)
        info = registration()
        builder.require(info is not None and Path(info['InstallLocation']) == target and info.get('HarmonySimpleInstaller') == '1', 'Registration differs')
        builder.require(builder.digest(canary) == canary_hash, 'Synthetic user data changed')
    try:
        builder.require(registration() is None, 'Private namespace already registered')
        target = fixture / 'Custom Program 日本'
        install(target, 'fresh')
        record('fresh current-user install, custom Unicode folder, every payload hash and registration')
        foreign = target / 'foreign.keep'
        foreign.write_bytes(b'FOREIGN PROGRAM CANARY; KEEP')
        install(target, 'update')
        builder.require(foreign.read_bytes() == b'FOREIGN PROGRAM CANARY; KEEP', 'Foreign file changed')
        record('same-location update detects previous installation and preserves foreign/data canaries')
        relocated = fixture / 'Moved Program'
        install(relocated, 'relocate')
        builder.require(not (target / 'Harmony.exe').exists() and foreign.read_bytes() == b'FOREIGN PROGRAM CANARY; KEEP', 'Old program removal or canary preservation failed')
        record('previous-program uninstall and custom-folder relocation')
        info = registration()
        if info is None:
            raise RuntimeError('Private registration disappeared before uninstall')
        command = info['UninstallString']
        builder.require(command.startswith('"') and command.endswith('"') and command.count('"') == 2, 'Unexpected uninstall metadata')
        uninstaller = Path(command[1:-1])
        builder.require(uninstaller.parent == relocated and uninstaller.name.startswith('unins'), 'Uninstaller escaped private folder')
        run([str(uninstaller), '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/LOG=' + str(evidence / 'uninstall.log')], 'uninstall')
        deadline = time.monotonic() + 15
        while (relocated / 'Harmony.exe').exists() and time.monotonic() < deadline:
            time.sleep(.1)
        builder.require(registration() is None, 'Registration remains')
        builder.require(all(not (relocated / name).exists() for name in package['payload_files']), 'Owned payload remains')
        builder.require(builder.digest(canary) == canary_hash and foreign.read_bytes() == b'FOREIGN PROGRAM CANARY; KEEP', 'Canary changed')
        record('uninstall removes registered program/payload and keeps data plus foreign file')
        install(relocated, 'reinstall')
        record('clean reinstall after uninstall')
        info = registration()
        if info is None:
            raise RuntimeError('Private registration disappeared before final cleanup')
        run([info['UninstallString'][1:-1], '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/LOG=' + str(evidence / 'final-uninstall.log')], 'final-uninstall')
        builder.require(registration() is None and not (relocated / 'Harmony.exe').exists(), 'Final private cleanup failed')
        record('final private registration/program cleanup')
        result['status'] = 'PASS_NATIVE_CURRENT_USER_INSTALLER_SMOKE'
    except Exception as error:
        result['status'] = 'FAIL_NATIVE_CURRENT_USER_INSTALLER_SMOKE'
        result['error'] = repr(error)
        raise
    finally:
        save()
        print(json.dumps(result, indent=2), flush=True)

if __name__ == '__main__':
    main()
