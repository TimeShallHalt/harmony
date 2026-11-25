"""Create a legacy per-user MSI from verified, updater-free Harmony artifacts before 1.2.0."""

import re
import os
import subprocess
import json
import shutil
import struct
import zipfile
from pathlib import PurePosixPath, PureWindowsPath
import hashlib
from pathlib import Path
import uuid
import xml.etree.ElementTree as ET

WIX_NS = 'http://wixtoolset.org/schemas/v4/wxs'
UI_NS = 'http://wixtoolset.org/schemas/v4/wxs/ui'
ROOT = Path(__file__).resolve().parents[1]
UPGRADE_CODE = uuid.uuid5(uuid.NAMESPACE_URL, 'https://github.com/TimeShallHalt/harmony/windows-msi/per-user/x64')
# First production MSI whose executable is Harmony.exe: app 1.1.0 build 1.
# Quarantine older products even if their component key path needs repair.
FIRST_REBRANDED_MSI_VERSION = '1.1.1'
LEGACY_MIGRATION_CONDITION = 'Installed OR NOT (HARMONY_LEGACY_MSI OR HARMONY_LEGACY_COMPONENT)'
LEGACY_MIGRATION_MESSAGE = (
    'A pre-rebrand Harmony MSI or its yapfast.exe installer component was found. '
    'Setup stopped before removing the old product. Keep it installed and keep your data. '
    'Use the portable Harmony.exe in a separate folder; migrate only your existing owned '
    'shortcuts/handlers, preserving arguments and data folders, before explicitly uninstalling '
    'the old MSI. See packaging/windows-msi.md: Manual legacy MSI migration.'
)
ET.register_namespace('', WIX_NS)
ET.register_namespace('ui', UI_NS)


def msi_version(version, build_id):
    """Encode the app patch/build in MSI's significant third field, never a fourth."""
    if not isinstance(version, str) or not re.fullmatch(r'(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)', version):
        raise ValueError('A canonical three-field app version is required')
    if type(build_id) is not int or not 1 <= build_id <= 999:
        raise ValueError('The MSI version scheme supports build IDs 1 through 999')
    major, minor, patch = map(int, version.split('.'))
    encoded = patch * 1000 + build_id
    if major > 255 or minor > 255 or encoded > 65535:
        raise ValueError('App version/build exceeds Windows Installer version limits')
    return f'{major}.{minor}.{encoded}'


def generate_wxs(payload, info, *, test_namespace=None, test_version=None, test_fail_after_install_files=False):
    """Author only private program files; never own user data, launchers or processes."""
    payload = Path(payload)
    if type(test_fail_after_install_files) is not bool or (test_fail_after_install_files and test_namespace is None):
        raise ValueError('Fault injection is allowed only for explicitly isolated lifecycle fixtures')
    payload_names = {path.name.casefold(): path.name for path in payload.iterdir()}
    if test_namespace is None and (payload_names.get('harmony.exe') != 'Harmony.exe' or not (payload / 'Harmony.exe').is_file() or 'yapfast.exe' in payload_names):
        raise ValueError('Production MSI requires Harmony.exe, not a legacy yapfast.exe payload')
    version = msi_version(info['version'], info['build_id'])
    if test_namespace is None and tuple(map(int, version.split('.'))) < tuple(map(int, FIRST_REBRANDED_MSI_VERSION.split('.'))):
        raise ValueError('Harmony.exe MSI requires app 1.1.0 build 1 or later; never reuse a legacy ProductCode')
    registry = r'Software\Harmony\Installer'
    upgrade_code = UPGRADE_CODE
    name = 'Harmony'
    folder_name = 'Harmony'
    if test_namespace is not None:
        if not isinstance(test_namespace, uuid.UUID) or test_namespace == UPGRADE_CODE:
            raise ValueError('Lifecycle tests require a distinct UUID namespace')
        upgrade_code = test_namespace
        registry = rf'Software\Harmony\InstallerTests\{test_namespace}'
        name = 'Harmony MSI Lifecycle Fixture'
        folder_name = 'Harmony-MSI-Fixture-' + test_namespace.hex
        if test_version is not None:
            if not re.fullmatch(r'(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)', test_version):
                raise ValueError('Invalid fixture MSI version')
            fields = [int(value) for value in test_version.split('.')]
            if any(value > bound for value, bound in zip(fields, (255, 255, 65535))):
                raise ValueError('Fixture MSI version exceeds MSI limits')
            version = test_version
    elif test_version is not None:
        raise ValueError('Never override a production release MSI version')
    def element(parent, tag, **attributes):
        return ET.SubElement(parent, f'{{{WIX_NS}}}{tag}', attributes)
    def identifier(prefix, name):
        return prefix + hashlib.sha256(name.casefold().encode('utf-8')).hexdigest()[:24]
    root = ET.Element(f'{{{WIX_NS}}}Wix')
    package = element(root, 'Package', Name=name, Manufacturer='Harmony contributors', Version=version,
                      ProductCode=str(uuid.uuid5(upgrade_code, 'product:' + version)).upper(),
                      UpgradeCode=str(upgrade_code).upper(), Language='1033', Scope='perUser', InstallerVersion='500')
    if test_namespace is not None:
        element(package, 'Property', Id='ARPSYSTEMCOMPONENT', Value='1')
    element(package, 'Icon', Id='Harmony.ico', SourceFile=str(ROOT/'packaging/windows/harmony.ico'))
    element(package, 'Property', Id='ARPPRODUCTICON', Value='Harmony.ico')
    if test_fail_after_install_files:
        element(package, 'CustomAction', Id='FixtureAbortAfterInstallFiles', Error='Harmony isolated MSI rollback fixture failure')
    element(package, 'SummaryInformation', Description=f"Harmony {info['version']} (build {info['build_id']}) manual update")
    element(package, 'MajorUpgrade', Schedule='afterInstallInitialize', DowngradeErrorMessage='A newer Harmony MSI is already installed. Use that version or a newer installer.')
    # Detection only: never uninstall a legacy product just to rename its binary.
    # The version range catches damaged old registrations; the exact HKCU component
    # marker catches a legacy payload even if it used an unexpected product version.
    legacy_upgrade = element(package, 'Upgrade', Id=str(upgrade_code).upper())
    element(legacy_upgrade, 'UpgradeVersion', Maximum=FIRST_REBRANDED_MSI_VERSION,
            IncludeMaximum='no', OnlyDetect='yes', Property='HARMONY_LEGACY_MSI')
    legacy_component = element(package, 'Property', Id='HARMONY_LEGACY_COMPONENT', Secure='yes')
    element(legacy_component, 'RegistrySearch', Id='LegacyExecutableComponent', Root='HKCU',
            Key=registry + r'\Components', Name=identifier('C_', 'yapfast.exe'), Type='raw', Bitness='always64')
    element(package, 'Launch', Condition=LEGACY_MIGRATION_CONDITION, Message=LEGACY_MIGRATION_MESSAGE)
    # Make the pre-transaction guard explicit for both interactive and /qn setup.
    # These are the standard MSI sequence positions; no executable custom action.
    for sequence in ('InstallUISequence', 'InstallExecuteSequence'):
        actions = element(package, sequence)
        element(actions, 'FindRelatedProducts', Sequence='25')
        element(actions, 'AppSearch', Sequence='50')
        element(actions, 'LaunchConditions', Sequence='100')
        if sequence == 'InstallExecuteSequence':
            element(actions, 'InstallInitialize', Sequence='1500')
            if test_fail_after_install_files:
                # InstallFiles schedules deferred writes. Flush that script
                # before failing, so the native test exercises real restoration.
                element(actions, 'InstallExecute', Sequence='6500')
                element(actions, 'Custom', Action='FixtureAbortAfterInstallFiles', After='InstallExecute', Condition='NOT REMOVE')
    element(package, 'MediaTemplate', EmbedCab='yes', CompressionLevel='high')
    for name, value in {'REINSTALLMODE':'amus','REBOOT':'ReallySuppress','MSIRESTARTMANAGERCONTROL':'DisableShutdown','MSIDISABLERMRESTART':'1',
                        'WIXUI_EXITDIALOGOPTIONALTEXT':'Open Harmony.exe using your chosen data folder. Setup does not change shortcuts, protocol handlers or startup entries, or start the app.'}.items():
        element(package, 'Property', Id=name, Value=value)
    element(package, 'SetProperty', Id='ARPINSTALLLOCATION', Value='[INSTALLFOLDER]', After='CostFinalize', Sequence='execute')
    existing = element(package, 'Property', Id='INSTALLFOLDER', Secure='yes')
    element(existing, 'RegistrySearch', Id='RememberedProgramFolder', Root='HKCU', Key=registry, Name='InstallFolder', Type='raw', Bitness='always64')
    programs_id = 'HarmonyPrograms'
    if test_namespace is not None:
        # A fixture's inherited uninstall must never fall back to the real
        # LocalAppData Programs ancestor when RemoveExistingProducts runs it.
        programs_id = 'HARMONYFIXTUREPROGRAMS'
        ancestor = element(package, 'Property', Id=programs_id, Secure='yes')
        element(ancestor, 'RegistrySearch', Id='RememberedFixtureProgramsRoot', Root='HKCU',
                Key=registry, Name='FixtureProgramsRoot', Type='raw', Bitness='always64')
    element(package, 'Launch', Condition='VersionNT64', Message='This Harmony package requires 64-bit Windows.')
    element(package, 'Launch', Condition='NOT ALLUSERS', Message='Harmony installs only for the current user; do not request an all-users installation.')
    standard = element(package, 'StandardDirectory', Id='LocalAppDataFolder')
    programs = element(standard, 'Directory', Id=programs_id, Name='Programs')
    install = element(programs, 'Directory', Id='INSTALLFOLDER', Name=folder_name)
    feature = element(package, 'Feature', Id='Application', Title='Harmony program and notices', Level='1', AllowAdvertise='no')
    location = element(install, 'Component', Id='ProgramFolderRegistration', Guid=str(uuid.uuid5(upgrade_code, 'install-folder')), Bitness='always64')
    element(location, 'RegistryValue', Root='HKCU', Key=registry, Name='InstallFolder', Value='[INSTALLFOLDER]', Type='string', KeyPath='yes')
    if test_namespace is not None:
        element(location, 'RegistryValue', Root='HKCU', Key=registry, Name='FixtureProgramsRoot',
                Value='[HARMONYFIXTUREPROGRAMS]', Type='string', KeyPath='no')
    element(feature, 'ComponentRef', Id='ProgramFolderRegistration')
    directories = {'': install}

    seen = set()
    for source in sorted(payload.rglob('*')):
        if source.is_symlink() or source.is_junction():
            raise ValueError('MSI payload cannot contain symbolic links or junctions')
        if not source.is_file():
            continue
        relative = source.relative_to(payload).as_posix()
        if relative.casefold() in seen:
            raise ValueError('MSI payload paths collide on Windows')
        seen.add(relative.casefold())
        parent = source.relative_to(payload).parent
        current = ''
        directory = install
        for part in parent.parts:
            current = (current + '/' if current else '') + part
            if current not in directories:
                directories[current] = element(directory, 'Directory', Id=identifier('D_', current), Name=part)
            directory = directories[current]
        component_id = identifier('C_', relative)
        component = element(directory, 'Component', Id=component_id,
                            Guid=str(uuid.uuid5(upgrade_code, 'file:' + relative.casefold())), Bitness='always64')
        element(component, 'RegistryValue', Root='HKCU', Key=registry + r'\Components', Name=component_id, Value='1', Type='integer', KeyPath='yes')
        element(component, 'File', Id=identifier('F_', relative), Name=source.name, Source=str(source.resolve()), KeyPath='no')
        element(feature, 'ComponentRef', Id=component_id)
    # Standard RemoveFolder removes only an empty directory. Include intermediate
    # profile ancestors for ICE64, never recursive deletion or wildcard files.
    for relative, directory in directories.items():
        element(location, 'RemoveFolder', Id=identifier('Remove_', relative), Directory=directory.attrib['Id'], On='uninstall')
    element(location, 'RemoveFolder', Id='RemoveEmptyProgramsParent', Directory=programs_id, On='uninstall')
    ET.SubElement(package, f'{{{UI_NS}}}WixUI', {'Id':'WixUI_Minimal'})
    element(package, 'UIRef', Id='WixUI_ErrorProgressText')
    element(package, 'WixVariable', Id='WixUILicenseRtf', Value='$(var.LicenseRtf)')
    ET.indent(root)
    return ET.tostring(root, encoding='utf-8', xml_declaration=True)


def stage_release(manifest_file, destination):
    """Accept trusted producer inputs only, rehash before copying; never execute payload."""
    manifest = json.loads(Path(manifest_file).read_text(encoding='utf-8'))
    destination = Path(destination)
    if destination.exists():
        raise ValueError('Use a fresh staging directory; partial evidence is never overwritten')
    artifacts = manifest['artifacts']
    bundle = next(Path(item['path']) for item in artifacts if item['path'].endswith('-windows-x64.zip'))
    source = next(Path(item['path']) for item in artifacts if item['path'].endswith('-source.zip'))
    def sha(path):
        with path.open('rb') as stream:
            return hashlib.file_digest(stream, 'sha256').hexdigest()
    for item in artifacts:
        if not re.fullmatch('[0-9a-f]{64}', item['sha256']) or sha(Path(item['path'])) != item['sha256']:
            raise ValueError('Release artifact no longer matches its producer manifest')
    with zipfile.ZipFile(source) as archive:
        if archive.testzip() is not None:
            raise ValueError('Corrupted corresponding-source archive')
    with zipfile.ZipFile(bundle) as archive:
        if archive.testzip() is not None:
            raise ValueError('Corrupted application archive')
        entries = {}
        roots = set()
        total = 0
        forbidden = {'layout.json','credentials.json','session.json','discord-account.json','discord-cookies.json','yapfast.log','harmony.log','.env','yapfast.exe'}
        for item in archive.infolist():
            # Windows ZipInfo normalizes separators/truncates NULs before
            # exposing filename; never silently accept repaired member tokens.
            raw_name = item.orig_filename
            if raw_name != item.filename:
                raise ValueError('Unsafe or noncanonical archive member')
            if item.is_dir():
                continue
            path = PurePosixPath(raw_name)
            if path.as_posix() != raw_name or len(path.parts) < 2 or path.is_absolute() or any(part in ('.','..') or part.rstrip(' .') != part for part in path.parts) or '\\' in raw_name or ':' in raw_name or PureWindowsPath(raw_name).drive:
                raise ValueError('Unsafe or noncanonical archive member')
            roots.add(path.parts[0]); relative = PurePosixPath(*path.parts[1:]).as_posix()
            if relative.casefold() in entries or path.name.lower() in forbidden or (item.external_attr >> 16) & 0o170000 == 0o120000:
                raise ValueError('Duplicate, link, or private runtime member in application archive')
            if any(re.fullmatch(r'(CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])(?:\..*)?', part, flags=re.I) for part in path.parts):
                raise ValueError('Reserved Windows path in archive')
            entries[relative.casefold()] = (relative, item)
            total += item.file_size
            if total > 512 * 1024 * 1024 or item.file_size > 256 * 1024 * 1024:
                raise ValueError('Application archive exceeds bounded staging limits')
        if len(roots) != 1 or not {'harmony.exe','build.json','harmony-build-info.json','license.txt','third-party.json','source.txt'} <= entries.keys():
            raise ValueError('Application archive lacks its unique root or required metadata')
        if entries['harmony.exe'][0] != 'Harmony.exe':
            raise ValueError('Windows executable must have the exact name Harmony.exe')
        read = lambda name: archive.read(entries[name][1])
        info = json.loads(read('harmony-build-info.json'))
        build = json.loads(read('build.json'))
        msi_version(info['version'], info['build_id'])
        if tuple(map(int, info['version'].split('.'))) >= (1, 2, 0):
            raise ValueError('Harmony 1.2.0 and later use the Inno Setup installer; run scripts/build-harmony-installer.py')
        if info.get('dirty') is not False or info.get('commit') != manifest['commit'] or info['version'] != manifest['version'] or info['build_id'] != manifest['build_id'] or info.get('platform') != 'windows' or info.get('architecture') != 'x86_64' or info.get('license') != 'GPL-3.0-or-later':
            raise ValueError('Clean Windows build identity does not match the release manifest')
        if info.get('upstream') != 'Yapfast 0.4.1' or any(build.get(key) != value for key, value in info.items()) or build.get('updater_included') is not False:
            raise ValueError('MSI requires the matching updater-free Harmony build and upstream attribution')
        raw = read('harmony.exe')
        expected_exe = manifest['executable']['sha256']
        if hashlib.sha256(raw).hexdigest() != expected_exe or build.get('executable_sha256') != expected_exe or build.get('source_archive_sha256') != sha(source) or build.get('commit') != info['commit']:
            raise ValueError('Executable, build metadata and corresponding source are not paired')
        if len(raw) < 64 or raw[:2] != b'MZ':
            raise ValueError('Not a Windows executable')
        offset = struct.unpack_from('<I', raw, 60)[0]
        if raw[offset:offset + 6] != b'PE\0\0\x64\x86':
            raise ValueError('MSI requires an AMD64 executable')
        notices = json.loads(read('third-party.json'))
        if not notices or not all(item.get('license_files') and all(name.casefold() in entries for name in item['license_files']) for item in notices):
            raise ValueError('Dependency notice files are missing')
        destination.mkdir(parents=True)
        for relative, item in entries.values():
            target = destination / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            with archive.open(item) as src, target.open('xb') as out:
                shutil.copyfileobj(src, out, 1024 * 1024)
    source_folder = destination / 'corresponding-source'
    source_folder.mkdir()
    shutil.copyfile(source, source_folder / source.name)
    instructions = (
        f"Harmony {info['version']} (build {info['build_id']}) MSI\n\n"
        'Quit Harmony from its tray menu before updating. Closing its window may only hide it.\n'
        'Run this MSI as your normal user. No administrator account is required.\n'
        'Program location: %LOCALAPPDATA%\\Programs\\Harmony. Existing shortcuts are not changed.\n'
        'LEGACY MSI MIGRATION: setup refuses a pre-rebrand Harmony MSI or its exact yapfast.exe\n'
        'installer component marker before removing any old product, in both wizard and silent mode.\n'
        'Keep the old MSI installed; first stage portable Harmony.exe in a separate, unowned folder.\n'
        'Preserve data and all launcher arguments (including --data-dir); manually migrate only\n'
        'existing owned shortcuts, taskbar pins and already-selected handlers/startup entries.\n'
        'Do not claim new handlers or enable startup. Verify each existing entry before explicitly\n'
        'uninstalling the old MSI. See packaging/windows-msi.md: Manual legacy MSI migration.\n'
        'After setup, open Harmony.exe or your existing Harmony.exe shortcut, preserving custom --data-dir arguments.\n'
        'Setup never changes shortcuts, protocol handlers or startup entries; never starts the app,\n'
        'joins calls, resumes capture, or force-closes applications.\n'
        'Settings/account data are outside the installer payload and are not removed on uninstall.\n'
        'After legacy migration, standard MSI upgrades replace older Harmony.exe products;\n'
        'registered newer versions block downgrades.\n'
        'A newer manually copied, unregistered executable is not checked by MSI product registration.\n'
        'Do not use an older MSI over a newer portable/manual build.\n'
        'The installer owns only its enumerated program/notice files. It force-replaces them to update\n'
        'unversioned legacy binaries; it does not delete unrelated files or rollback copies.\n'
        'The corresponding GPL source archive and third-party notices are included next to the program.\n'
        f"MSI upgrade version: {msi_version(info['version'], info['build_id'])}; app version remains {info['version']} build {info['build_id']}.\n"
        'MSI uses major.minor.(patch*1000+build), with build IDs 1..999, within MSI limits.\n'
        'This is a manual offline installer, not an automatic update service.\n'
        'This package is unsigned. Windows may show Unknown Publisher; do not disable security tools.\n'
    )
    (destination / 'MSI-INSTALL.txt').write_text(instructions, encoding='utf-8')
    return info, {path.relative_to(destination).as_posix(): sha(path) for path in sorted(destination.rglob('*')) if path.is_file()}


def build_msi(payload, info, output, wix_dll, ui_extension, *, test_namespace=None, test_version=None, test_fail_after_install_files=False):
    """Compile real WiX below normal priority; no installation, UI input or app launch."""
    if os.name != 'nt':
        raise ValueError('This MSI build/validation entry point requires Windows')
    output = Path(output).resolve()
    if output.exists():
        raise ValueError('Refusing to overwrite a previously built MSI')
    output.parent.mkdir(parents=True, exist_ok=True)
    work = output.parent / (output.stem + '-build')
    work.mkdir()
    recipe = work / 'Harmony.wxs'
    recipe.write_bytes(generate_wxs(payload, info, test_namespace=test_namespace, test_version=test_version,
                                    test_fail_after_install_files=test_fail_after_install_files))
    license_rtf = work / 'license.rtf'
    text = (
        f"Harmony {info['version']} (build {info['build_id']})\n\n"
        'Quit Harmony using its tray Quit menu before continuing. This installer updates program files only; '
        'it does not remove settings/accounts, change shortcuts, protocol handlers or startup entries, '
        'automatically start the app, or stop other programs.\n'
        'Legacy MSI upgrades are refused before removing the old product; see '
        'packaging/windows-msi.md: Manual legacy MSI migration. Keep the old installation '
        'until its exact owned launchers have been migrated to a separate portable folder.\n'
        'After setup, use Harmony.exe or an existing Harmony.exe shortcut, preserving its custom data folder.\n\n'
        + (Path(payload) / 'LICENSE.txt').read_text(encoding='utf-8')
    )
    escaped = text.replace('\\', r'\\').replace('{', r'\{').replace('}', r'\}').replace('\n', '\\par\n')
    license_rtf.write_text('{\\rtf1\\ansi\\deff0{\\fonttbl{\\f0 Segoe UI;}}\\f0\\fs18\n' + escaped + '}', encoding='ascii')
    command = ['dotnet', str(Path(wix_dll).resolve()), 'build', str(recipe), '-arch', 'x64', '-ext', str(Path(ui_extension).resolve()),
               '-culture', 'en-US', '-d', 'LicenseRtf=' + str(license_rtf), '-pdbtype', 'none', '-ct', '2',
               '-intermediatefolder', str(work / 'intermediate'), '-out', str(output)]
    env = dict(os.environ, DOTNET_ROLL_FORWARD='Major', DOTNET_CLI_TELEMETRY_OPTOUT='1', DOTNET_NOLOGO='1')
    for key in ('TMPDIR', 'TMP', 'TEMP'):
        env[key] = str(work)
    result = subprocess.run(command, cwd=work, env=env, capture_output=True, text=True, encoding='utf-8', errors='replace',
                            timeout=300, creationflags=subprocess.BELOW_NORMAL_PRIORITY_CLASS | subprocess.CREATE_NO_WINDOW)
    log = work / 'wix-build.log'
    log.write_text(result.stdout + result.stderr, encoding='utf-8')
    receipt = {'command':command,'exit_code':result.returncode,'log':str(log),'msi_version':test_version or msi_version(info['version'],info['build_id']),
               'app_version':info['version'],'app_build_id':info['build_id'],'fixture_namespace':str(test_namespace) if test_namespace else None,
               'source_recipe':str(recipe),'output':str(output),'installed':False,'priority':'below-normal','cabinet_threads':2,
               'fixture_fault_after_install_files':test_fail_after_install_files,
               'builder_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
               'wix_cli_sha256':hashlib.sha256(Path(wix_dll).read_bytes()).hexdigest(),
               'wix_ui_sha256':hashlib.sha256(Path(ui_extension).read_bytes()).hexdigest()}
    (work / 'build-receipt.json').write_text(json.dumps(receipt,indent=2)+'\n',encoding='utf-8')
    if result.returncode != 0:
        raise RuntimeError(f'WiX exited {result.returncode}; see {log}:\n{result.stdout}{result.stderr}')
    with output.open('rb') as stream:
        if stream.read(8) != b'\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1':
            raise ValueError('WiX did not produce a structured-storage MSI file')
    return receipt


def main():
    import argparse
    parser = argparse.ArgumentParser(description='Build an offline per-user Harmony MSI; never install or launch it.')
    parser.add_argument('--manifest',type=Path,required=True,help='Trusted verified producer release-artifacts.json')
    parser.add_argument('--wix-dll',type=Path,required=True,help='Pinned portable WiX v4 build-only CLI DLL')
    parser.add_argument('--ui-extension',type=Path,required=True,help='Matching pinned WixToolset.UI.wixext DLL')
    parser.add_argument('--output',type=Path,required=True,help='New .msi destination; existing artifacts are not overwritten')
    args = parser.parse_args()
    if args.output.suffix.lower() != '.msi' or args.output.exists():
        parser.error('Choose a new .msi output file')
    packaging_commit = subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip()
    if subprocess.check_output(['git','status','--porcelain'],cwd=ROOT,text=True).strip():
        parser.error('Commit the exact clean installer source before building a production MSI')
    payload = args.output.parent / (args.output.stem + '-payload')
    info, files = stage_release(args.manifest, payload)
    result = build_msi(payload,info,args.output,args.wix_dll,args.ui_extension)
    with args.output.open('rb') as stream:
        digest = hashlib.file_digest(stream,'sha256').hexdigest()
    result.update({'application_commit':info['commit'],'packaging_commit':packaging_commit,
                   'executable_sha256':files['Harmony.exe'],'msi_sha256':digest,
                   'msi_bytes':args.output.stat().st_size,'payload_root':str(payload),'payload_files':files,'unsigned':True,
                   'scope':'per-user x64; manual MSI only; no accounts/user-data/shortcut changes, no in-app updater',
                   'producer_manifest_sha256':hashlib.sha256(args.manifest.read_bytes()).hexdigest()})
    receipt = args.output.with_suffix('.msi.json')
    with receipt.open('x',encoding='utf-8') as stream:
        json.dump(result,stream,indent=2);stream.write('\n')
    print(json.dumps({key:value for key,value in result.items() if key!='payload_files'},indent=2))


if __name__ == '__main__':
    main()
