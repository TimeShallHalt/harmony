"""Build Harmony's single-file Inno Setup installer from verified release bytes."""
from pathlib import Path, PurePosixPath
import argparse
import hashlib
import json
import os
import shutil
import subprocess
import uuid
import zipfile

ROOT = Path(__file__).resolve().parents[1]
PUBLIC_ID = '46969CAC-C472-58C4-89BD-069E35950175'
SCRATCH = Path(os.environ.get('HARMONY_TEST_SCRATCH', str(Path.home() / 'AppData/Local/hermes/profiles/hex/cache/scratch')))

def digest(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()

def require(condition, message):
    if not condition:
        raise ValueError(message)

def stage(manifest, payload):
    info = json.loads(manifest.read_text(encoding='utf-8'))
    artifacts = {Path(row['path']).name: row for row in info['artifacts']}
    portable = next(row for name, row in artifacts.items() if name.endswith('-windows-x64.zip'))
    source = next(row for name, row in artifacts.items() if name.endswith('-source.zip'))
    for artifact in (portable, source):
        require(digest(artifact['path']) == artifact['sha256'], 'Release archive hash mismatch')
    payload.mkdir(parents=True)
    seen = set()
    with zipfile.ZipFile(portable['path']) as archive:
        prefix = f"Harmony-{info['version']}-b{info['build_id']}-windows-x64"
        for item in archive.infolist():
            require(item.orig_filename == item.filename and '\\' not in item.filename, 'Noncanonical ZIP name')
            path = PurePosixPath(item.filename)
            require(path.parts and path.parts[0] == prefix, 'Unexpected archive root')
            relative = PurePosixPath(*path.parts[1:])
            require(all(part not in ('.', '..') and not any(c in part for c in ':"{}') and not part.endswith((' ', '.')) for part in relative.parts), 'Unsafe archive member')
            if item.is_dir():
                continue
            require(relative.parts and str(relative).casefold() not in seen, 'Duplicate/empty member')
            require((item.external_attr >> 16) & 0o170000 != 0o120000, 'Archive links are not allowed')
            seen.add(str(relative).casefold())
            destination = payload.joinpath(*relative.parts)
            destination.parent.mkdir(parents=True, exist_ok=True)
            with archive.open(item) as src, destination.open('xb') as dst:
                shutil.copyfileobj(src, dst)
    require(digest(payload / 'Harmony.exe') == info['executable']['sha256'], 'Application hash mismatch')
    corresponding = payload / 'corresponding-source' / Path(source['path']).name
    corresponding.parent.mkdir(parents=True, exist_ok=True)
    if corresponding.exists():
        require(digest(corresponding) == source['sha256'], 'Conflicting corresponding source')
    else:
        shutil.copyfile(source['path'], corresponding)
    for name in ('packaging/windows/harmony-simple.iss', 'scripts/build-harmony-installer.py'):
        target = payload / 'installer-source' / Path(name).name
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(ROOT / name, target)
    (payload / 'INSTALLER.txt').write_text(
        'Harmony installer\n\nChoose current-user or all-users installation, then a program folder.\n'
        'Existing supported installations are detected. Profiles, credentials, settings and custom data folders are not copied or deleted.\n'
        'Quit Harmony before updating. Setup never launches the application or enables startup/protocol integrations.\n'
        'Moving a shared install to current-user removes shared app availability, not other users\' data.\n'
        'Uninstall removes installer-owned program files only. Unknown older installers require removal through Windows Apps.\n'
        'External previous-program removal is not atomic with the new install. A failed new install may require rerunning Setup.\n'
        'Application GPL-3.0-or-later; original attribution, notices and corresponding source are included.\n', encoding='utf-8')
    files = {path.relative_to(payload).as_posix(): digest(path) for path in sorted(payload.rglob('*')) if path.is_file()}
    return info, files

def build(manifest, output_dir, fixture_root=None, fixture_id=None, tooling_receipt=None):
    output_dir = output_dir.resolve()
    output_dir.mkdir(parents=True, exist_ok=True)
    work = output_dir / ('build-' + uuid.uuid4().hex[:12])
    work.mkdir()
    info, files = stage(manifest.resolve(), work / 'payload')
    payload = work / 'payload'
    if tooling_receipt is None:
        tooling_receipt = ROOT / '.local/tools/inno-7.1.1-x64/tooling-provenance.json'
    provenance = json.loads(tooling_receipt.read_text())
    compiler = Path(provenance['compiler'])
    for name, checksum in provenance['compiler_scan']['file_sha256'].items():
        require(digest(compiler.parent / name) == checksum, 'Pinned compiler file changed: ' + name)
    include = work / 'files.iss'
    rows = []
    for name, checksum in files.items():
        member = PurePosixPath(name)
        destination = '{app}' + ('\\' + str(member.parent).replace('/', '\\') if str(member.parent) != '.' else '')
        rows.append(f'Source: "{payload.joinpath(*member.parts)}"; DestDir: "{destination}"; DestName: "{member.name}"; Flags: ignoreversion; Hash: "{checksum}"')
    include.write_text('\n'.join(rows) + '\n', encoding='utf-8')
    defines = {'PayloadDir': str(payload), 'PayloadFiles': str(include), 'AppVersion': info['version'], 'AppBuild': str(info['build_id'])}
    if fixture_root is not None:
        fixture_root = fixture_root.resolve()
        require(fixture_root.is_relative_to(SCRATCH.resolve()) and fixture_root != SCRATCH.resolve(), 'Fixture must be beneath approved scratch')
        identity = uuid.UUID(fixture_id) if fixture_id else uuid.uuid4()
        require(identity.version == 4 and str(identity).upper() != PUBLIC_ID, 'Private UUID4 required')
        defines.update(FixtureRoot=str(fixture_root), HarmonyId=str(identity).upper())
    output = output_dir / ('Harmony-' + info['version'] + '-Setup.exe')
    require(not output.exists(), 'Choose a fresh output location')
    command = [str(compiler), '--quiet-progress', '--no-ide-signtools', '--output-dir=' + str(output_dir)]
    command += ['--define=' + key + '=' + value for key, value in defines.items()]
    command += [str(ROOT / 'packaging/windows/harmony-simple.iss')]
    env = dict(os.environ)
    for name in ('TMP', 'TEMP', 'TMPDIR', 'HARMONY_TEST_SCRATCH'):
        env[name] = str(SCRATCH)
    with (work / 'compiler.log').open('w', encoding='utf-8') as log:
        run = subprocess.run(command, env=env, stdout=log, stderr=subprocess.STDOUT, timeout=300)
    require(run.returncode == 0 and output.exists(), 'Inno compile failed; see ' + str(work / 'compiler.log'))
    require({name: digest(payload / name) for name in files} == files, 'Payload changed while building')
    result = {'installer': str(output), 'sha256': digest(output), 'bytes': output.stat().st_size,
              'application_version': info['version'], 'application_commit': info['commit'], 'application_sha256': files['Harmony.exe'],
              'payload': str(payload), 'payload_files': files, 'compiler_version': provenance['version'],
              'fixture_root': str(fixture_root) if fixture_root else None, 'app_id': defines.get('HarmonyId', PUBLIC_ID),
              'unsigned': True, 'application_rebuilt': False, 'command': command, 'compiler_log': str(work / 'compiler.log')}
    output.with_suffix('.json').write_text(json.dumps(result, indent=2) + '\n', encoding='utf-8')
    return result

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--manifest', type=Path, default=ROOT / '.local/evidence/upstream-041-branding/release/release-artifacts.json')
    parser.add_argument('--output-dir', type=Path, required=True)
    parser.add_argument('--tooling-receipt', type=Path, help='Verified Inno compiler receipt from prepare-windows-inno.py')
    parser.add_argument('--fixture-root', type=Path)
    parser.add_argument('--fixture-id')
    args = parser.parse_args()
    result = build(args.manifest, args.output_dir, args.fixture_root, args.fixture_id, args.tooling_receipt)
    print(json.dumps({key: value for key, value in result.items() if key not in ('payload_files', 'command')}, indent=2))
