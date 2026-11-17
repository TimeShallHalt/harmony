"""Opt-in, scratch-only native MSI fixtures. Never install a production MSI or launch an app.

Build/inspect only: --release-receipt RELEASE.msi.json --wix-dll ... --ui-extension ...
                   --prepare-only [--include-fault-rollback]
Run those exact inspected fixtures: --run-prepared WORK/plan.json
                                   --allow-isolated-msi-registration
A combined invocation without --prepare-only also requires the registration opt-in.

Fixture builder contract: secure HARMONYFIXTUREPROGRAMS directory, its
FixtureProgramsRoot value/search in the fixture registry, and optional
test_fail_after_install_files=True fault after InstallExecute(6500).
"""
from __future__ import annotations

import argparse
from dataclasses import dataclass
import hashlib
import importlib.util
import inspect as python_inspect
import json
import os
from pathlib import Path, PurePosixPath, PureWindowsPath
import re
import shutil
import subprocess
import time
import uuid

try:
    from .msi_database import guid_text, inspect, product_state, related_products, validate_table_policy
except ImportError:  # Direct script invocation.
    from msi_database import guid_text, inspect, product_state, related_products, validate_table_policy

ROOT = Path(__file__).resolve().parents[1]
PROFILE_SCRATCH = Path.home() / 'AppData/Local/hermes/profiles/hex/cache/scratch'
PRODUCTION_UPGRADE_CODE = uuid.UUID('B85CFFDA-896F-5BBC-8863-BC5F4DD0E810')
FIRST_RENAMED = '1.1.1'
FIXTURE_PROGRAMS = 'HARMONYFIXTUREPROGRAMS'
LEGACY_CONDITION = 'Installed OR NOT (HARMONY_LEGACY_MSI OR HARMONY_LEGACY_COMPONENT)'
LEGACY_MARKER = 'C_bb51d6588f1a73f021a9113e'
FAULT_ACTION = 'FixtureAbortAfterInstallFiles'
FAULT_MESSAGE = 'Harmony isolated MSI rollback fixture failure'
FIXTURE_NOTE = 'harmony-msi-fixture.txt'
NEW_ONLY = 'harmony-msi-rollback-new-only.txt'
UNOWNED = 'unowned-fixture-canary.txt'
GROUPS = ('legacy-version', 'legacy-marker', 'renamed', 'rollback')
# Version-only has no yapfast marker; marker-only is outside the legacy version range.
SPECS = (
    ('legacy-version-old', 'legacy-version', '1.1.0', False, False),
    ('legacy-version-candidate', 'legacy-version', '1.1.2', False, False),
    ('legacy-marker-old', 'legacy-marker', '1.1.1', True, False),
    ('legacy-marker-candidate', 'legacy-marker', '1.1.2', False, False),
    ('renamed-base', 'renamed', '1.1.1', False, False),
    ('renamed-upgrade', 'renamed', '1.1.2', False, False),
    ('rollback-base', 'rollback', '1.1.1', False, False),
    ('rollback-fault', 'rollback', '1.1.2', False, True),
)


def require(condition, message):
    # Safety must still hold with python -O.
    if not condition:
        raise ValueError(message)


def digest(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def write_json(path, value, *, exclusive=False):
    with Path(path).open('x' if exclusive else 'w', encoding='utf-8') as stream:
        json.dump(value, stream, indent=2)
        stream.write('\n')


def no_reparse(path):
    path = Path(path).absolute()
    for ancestor in (path, *path.parents):
        if ancestor.exists() or ancestor.is_symlink():
            attributes = getattr(ancestor.lstat(), 'st_file_attributes', 0)
            require(not ancestor.is_symlink() and not attributes & 0x400,
                    f'Reparse/symlink boundary refused: {ancestor}')
    return path


def scratch_path(path, *, parent=PROFILE_SCRATCH):
    raw = Path(path)
    require(raw.is_absolute() and '..' not in raw.parts, 'An absolute, canonical scratch path is required')
    no_reparse(parent)
    no_reparse(raw)
    resolved = raw.resolve()
    require(resolved.is_relative_to(Path(parent).resolve()), 'Fixtures must remain in the active hex profile scratch')
    return resolved


def relative_name(name):
    require(isinstance(name, str), 'Payload name must be text')
    pure = PurePosixPath(name)
    require(bool(pure.parts) and pure.as_posix() == name and not pure.is_absolute()
            and not PureWindowsPath(name).drive and '\\' not in name and ':' not in name,
            'Noncanonical payload member')
    for part in pure.parts:
        require(part not in ('.', '..') and part.rstrip(' .') == part
                and not any(ord(char) < 32 or char in '<>"|?*' for char in part)
                and not re.fullmatch(r'(CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])(?:\..*)?', part, re.I),
                'Unsafe Windows payload member')
    return pure


def payload_hashes(root):
    no_reparse(root)
    result = {}
    seen = set()
    for path in sorted(Path(root).rglob('*')):
        no_reparse(path)
        if path.is_file():
            name = path.relative_to(root).as_posix()
            relative_name(name)
            require(name.casefold() not in seen, 'Case-colliding payload names')
            seen.add(name.casefold())
            result[name] = digest(path)
    return result


def validate_hash_map(files):
    require(isinstance(files, dict) and bool(files), 'Empty or invalid payload manifest')
    for name, value in files.items():
        relative_name(name)
        require(isinstance(value, str) and re.fullmatch('[0-9a-f]{64}', value), 'Invalid payload SHA-256')
    require(len({name.casefold() for name in files}) == len(files), 'Case-colliding payload manifest')


def load_release(receipt_path):
    no_reparse(receipt_path)
    source = json.loads(Path(receipt_path).read_text(encoding='utf-8'))
    payload = Path(source['payload_root'])
    require(payload.is_absolute(), 'Release payload root must be absolute')
    files = source['payload_files']
    validate_hash_map(files)
    require('Harmony.exe' in files and not any(PurePosixPath(name).name.casefold() == 'yapfast.exe' for name in files),
            'Release must contain exact Harmony.exe and no legacy executable')
    require(not {FIXTURE_NOTE.casefold(), NEW_ONLY.casefold(), UNOWNED.casefold()} & {name.casefold() for name in files},
            'Release collides with fixture canaries')
    require(payload_hashes(payload) == files, 'Release receipt does not describe exact current payload bytes/names')
    info = json.loads((payload / 'harmony-build-info.json').read_text(encoding='utf-8'))
    require(source['executable_sha256'] == files['Harmony.exe'], 'Release executable SHA does not match its payload')
    require(source['application_commit'] == info['commit'] and info.get('dirty') is False
            and source['app_version'] == info['version'] and source['app_build_id'] == info['build_id'],
            'Release build identity mismatch')
    return payload, files, info


def builder_module():
    spec = importlib.util.spec_from_file_location('harmony_msi_fixture_builder', ROOT / 'scripts/package_harmony_msi.py')
    if spec is None or spec.loader is None:
        raise ValueError('MSI builder not found')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    require(module.UPGRADE_CODE == PRODUCTION_UPGRADE_CODE and module.FIRST_REBRANDED_MSI_VERSION == FIRST_RENAMED,
            'Packaging policy changed; re-review lifecycle fixture boundaries')
    return module


def identifier(prefix, name):
    return prefix + hashlib.sha256(name.casefold().encode('utf-8')).hexdigest()[:24]


def fixture_registry_absent(namespace):
    """Read only the exact generated fixture key, never production/user-data keys."""
    require(isinstance(namespace, uuid.UUID) and namespace.version == 4
            and namespace != PRODUCTION_UPGRADE_CODE, 'Not a private fixture registry namespace')
    import winreg
    key = rf'Software\Harmony\InstallerTests\{namespace}'
    try:
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, key, 0, winreg.KEY_READ | winreg.KEY_WOW64_64KEY):
            return False
    except FileNotFoundError:
        return True


def fixture_registry_value(namespace, name):
    """Read only the generated namespace's exact remembered program paths."""
    require(isinstance(namespace, uuid.UUID) and namespace.version == 4
            and namespace != PRODUCTION_UPGRADE_CODE, 'Not a private fixture registry namespace')
    require(name in ('InstallFolder', 'FixtureProgramsRoot'), 'Unexpected fixture registry value')
    import winreg
    try:
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, rf'Software\Harmony\InstallerTests\{namespace}',
                            0, winreg.KEY_READ | winreg.KEY_WOW64_64KEY) as key:
            value, kind = winreg.QueryValueEx(key, name)
            require(kind == winreg.REG_SZ and isinstance(value, str), 'Unexpected fixture program-path registry type')
            return value
    except FileNotFoundError:
        return None


def fixture_install_folder(namespace):
    return fixture_registry_value(namespace, 'InstallFolder')


def fixture_programs_root(namespace):
    return fixture_registry_value(namespace, 'FixtureProgramsRoot')


@dataclass
class Package:
    label: str
    group: str
    namespace: uuid.UUID
    version: str
    legacy_alias: bool
    fault: bool
    path: Path
    payload: Path
    target: Path
    files: dict
    sha256: str

    @property
    def code(self):
        return guid_text(uuid.uuid5(self.namespace, 'product:' + self.version))

    @property
    def registry(self):
        return rf'Software\Harmony\InstallerTests\{self.namespace}'

    def record(self):
        result = dict(vars(self))
        for key in ('namespace', 'path', 'payload', 'target'):
            result[key] = str(result[key])
        return result


def validate_database(db, package, legacy_message):
    """Fail-closed compiled-table preflight; no recipe-only proof, no host writes."""
    ns = package.namespace
    require(ns.version == 4 and ns != PRODUCTION_UPGRADE_CODE, 'Only fresh UUID4 fixture namespaces are allowed')
    props, tables = db['properties'], db['raw_tables']
    validate_table_policy(db['table_names'], tables)
    require(guid_text(props['UpgradeCode']) == guid_text(ns) and guid_text(props['ProductCode']) == package.code,
            'MSI fixture product/upgrade identity mismatch')
    require(props.get('ProductVersion') == package.version and props.get('ProductName') == 'Harmony MSI Lifecycle Fixture'
            and props.get('ARPSYSTEMCOMPONENT') == '1', 'Not the exact hidden lifecycle fixture')
    require(not props.get('ALLUSERS') and not props.get('MSIINSTALLPERUSER'), 'Unexpected install context override')
    for name, expected in {'REINSTALLMODE': 'amus', 'REBOOT': 'ReallySuppress',
                           'MSIRESTARTMANAGERCONTROL': 'DisableShutdown', 'MSIDISABLERMRESTART': '1'}.items():
        require(props.get(name) == expected, 'MSI restart/overwrite policy changed: ' + name)
    require(db['summary_template'].split(';')[0] == 'x64', 'Expected x64 MSI template')
    require(not {'INSTALLFOLDER', FIXTURE_PROGRAMS, 'HARMONY_LEGACY_MSI', 'HARMONY_LEGACY_COMPONENT'} & props.keys(),
            'Search/target properties must not have author-controlled defaults')
    require({'INSTALLFOLDER', FIXTURE_PROGRAMS, 'HARMONY_LEGACY_MSI', 'HARMONY_LEGACY_COMPONENT'}
            <= set(props.get('SecureCustomProperties', '').split(';')), 'Detection/target properties must be secure')

    directories = {row[0]: [row[1], row[2].split('|')[-1]] for row in tables['Directory']}
    expected_dirs = {'TARGETDIR': ['', 'SourceDir'], 'LocalAppDataFolder': ['TARGETDIR', 'LocalApp'],
                     FIXTURE_PROGRAMS: ['LocalAppDataFolder', 'Programs'],
                     'INSTALLFOLDER': [FIXTURE_PROGRAMS, 'Harmony-MSI-Fixture-' + ns.hex]}
    file_directories = {}
    for name in package.files:
        relative_name(name)
        current, parent = '', 'INSTALLFOLDER'
        for part in PurePosixPath(name).parent.parts:
            current = current + '/' + part if current else part
            key = identifier('D_', current)
            expected_dirs[key] = [parent, part]
            parent = key
        file_directories[name] = parent
    require(directories == expected_dirs, 'Unexpected directory ownership/default production destination')
    components = {row[0]: row for row in tables['Component']}
    registry = {row[0]: row for row in tables['Registry']}
    file_rows = {row[0]: row for row in tables['File']}
    require(len(components) == len(package.files) + 1 and len(registry) == len(components) + 1
            and len(file_rows) == len(package.files), 'Unexpected file/component/registry ownership count')
    expected_components = {
        'ProgramFolderRegistration': (uuid.uuid5(ns, 'install-folder'), 'INSTALLFOLDER',
                                      package.registry, 'InstallFolder', '[INSTALLFOLDER]')}
    for name in package.files:
        key = identifier('C_', name)
        expected_components[key] = (uuid.uuid5(ns, 'file:' + name.casefold()), file_directories[name],
                                    package.registry + r'\Components', key, '#1')
        row = file_rows.get(identifier('F_', name))
        require(row is not None and row[1] == key and row[2].split('|')[-1] == PurePosixPath(name).name
                and int(row[3]) == (package.payload / name).stat().st_size, 'MSI file table differs from fixture payload')
    production_guids = {guid_text(uuid.uuid5(PRODUCTION_UPGRADE_CODE, 'install-folder'))}
    production_guids.update(guid_text(uuid.uuid5(PRODUCTION_UPGRADE_CODE, 'file:' + name.casefold())) for name in package.files)
    require(set(components) == set(expected_components), 'Unexpected component IDs')
    for key, (component_guid, directory, regkey, regname, value) in expected_components.items():
        row = components[key]
        require(guid_text(row[1]) == guid_text(component_guid) and guid_text(row[1]) not in production_guids
                and row[2] == directory and row[3] == '260' and row[4] == '',
                'Component escaped namespace, directory or private registry-key-path policy')
        reg = registry.get(row[5])
        require(reg is not None and reg[1:] == ['1', regkey, regname, value, key],
                'Unexpected registry write/key path outside exact fixture namespace')
    extra_registry = [row for key, row in registry.items() if key not in {row[5] for row in components.values()}]
    require(len(extra_registry) == 1 and extra_registry[0][1:] ==
            ['1', package.registry, 'FixtureProgramsRoot', '[' + FIXTURE_PROGRAMS + ']', 'ProgramFolderRegistration'],
            'Nested predecessor uninstall must remember the exact isolated Programs ancestor')
    removals = tables.get('RemoveFile', [])
    require(len(removals) == len(expected_dirs) - 2, 'Unexpected empty-directory removal set')
    require({row[3] for row in removals} == set(expected_dirs) - {'TARGETDIR', 'LocalAppDataFolder'},
            'Folder removal escaped declared fixture directories')
    require(all(row[1] == 'ProgramFolderRegistration' and row[2] == '' and row[4] == '2' for row in removals),
            'Only nonrecursive uninstall-time empty-directory removal is allowed')

    root_search = dict(tables['AppSearch']).get(FIXTURE_PROGRAMS)
    require(root_search is not None, 'Nested uninstall needs an isolated ancestor AppSearch')
    require(sorted(tables['RegLocator']) == sorted([
        ['LegacyExecutableComponent', '1', package.registry + r'\Components', LEGACY_MARKER, '18'],
        ['RememberedProgramFolder', '1', package.registry, 'InstallFolder', '18'],
        [root_search, '1', package.registry, 'FixtureProgramsRoot', '18']]),
        'Registry searches escaped exact 64-bit HKCU fixture namespace')
    require(sorted(tables['AppSearch']) == sorted([
        ['HARMONY_LEGACY_COMPONENT', 'LegacyExecutableComponent'], ['INSTALLFOLDER', 'RememberedProgramFolder'],
        [FIXTURE_PROGRAMS, root_search]]),
        'Unexpected AppSearch ownership')
    conditions = dict(tables['LaunchCondition'])
    require(conditions.get(LEGACY_CONDITION) == legacy_message and 'VersionNT64' in conditions
            and 'NOT ALLUSERS' in conditions and any('WIX_DOWNGRADE_DETECTED' in key for key in conditions),
            'Legacy/downgrade/context launch guards missing')
    upgrades = tables['Upgrade']
    require(not {row[6] for row in upgrades} & props.keys(),
            'Upgrade action properties must not be pre-authored; product removal lists come only from FindRelatedProducts')
    require(all(guid_text(row[0]) == guid_text(ns) and row[6] in
                {'HARMONY_LEGACY_MSI', 'WIX_UPGRADE_DETECTED', 'WIX_DOWNGRADE_DETECTED'} for row in upgrades),
            'Upgrade table can discover/remove a non-fixture identity')
    legacy = [row for row in upgrades if row[6] == 'HARMONY_LEGACY_MSI']
    require(len(legacy) == 1 and legacy[0][1:4] == ['', FIRST_RENAMED, '']
            and int(legacy[0][4]) & 2 and not int(legacy[0][4]) & 512, 'Legacy detection must be below floor and detect-only')
    require({row[6] for row in upgrades} == {'HARMONY_LEGACY_MSI', 'WIX_UPGRADE_DETECTED', 'WIX_DOWNGRADE_DETECTED'},
            'Normal significant-version upgrade/downgrade detection missing')
    for table in ('InstallUISequence', 'InstallExecuteSequence'):
        actions = {row[0]: (row[1], int(row[2])) for row in tables[table]}
        require(all(actions.get(name) == ('', sequence) for name, sequence in
                    (('FindRelatedProducts', 25), ('AppSearch', 50), ('LaunchConditions', 100))),
                'Pre-removal guard sequencing changed in ' + table)
        if table == 'InstallExecuteSequence':
            require(actions['InstallInitialize'][1] == 1500
                    and 1500 < actions['RemoveExistingProducts'][1] < actions['InstallFiles'][1],
                    'Pre-removal guard/transactional upgrade order changed')
    execute = {row[0]: (row[1], int(row[2])) for row in tables['InstallExecuteSequence']}
    faults = []
    for action, kind, source, target in tables.get('CustomAction', []):
        if kind == '51' and source == 'ARPINSTALLLOCATION' and target == '[INSTALLFOLDER]':
            require(action in execute and execute[action][1] > execute['CostFinalize'][1], 'Unexpected ARP setter schedule')
        elif action == FAULT_ACTION:
            require(package.fault and kind == '19' and source == '' and target == FAULT_MESSAGE,
                    'Unexpected rollback fault action')
            require(execute.get(action, ('', 0))[0] == 'NOT REMOVE'
                    and execute.get('InstallExecute', ('', 0))[1] == 6500
                    and execute['InstallFiles'][1] < execute['InstallExecute'][1]
                    < execute[action][1] < execute['InstallFinalize'][1],
                    'Fault must follow flushed InstallExecute inside the upgrade transaction')
            faults.append(action)
        else:
            # WiX Minimal includes a dormant, UI-only EULA printing DLL action. It
            # is not an application launch and cannot execute in these /qn runs.
            require(re.fullmatch(r'WixUIPrintEula(?:_X64)?', action) is not None
                    and kind == '65' and target == 'PrintEula' and action not in execute,
                    'Unreviewed executable/script/custom action: ' + action)
    require(bool(faults) == package.fault, 'Missing/unrequested fault injection')
    for forbidden in ('Shortcut', 'ServiceInstall', 'ServiceControl', 'Environment', 'IniFile', 'RemoveIniFile',
                      'RemoveRegistry', 'MoveFiles', 'DuplicateFile', 'Class', 'ProgId', 'Extension', 'Verb',
                      'MIME', 'TypeLib', 'SelfReg', 'BindImage', 'IsolatedComponent', 'CreateFolder',
                      'ODBCDataSource', 'ODBCDriver', 'ODBCTranslator', 'Complus', 'MsiLockPermissionsEx', 'LockPermissions'):
        require(forbidden not in db['table_names'], 'Unexpected integration table: ' + forbidden)
    return {'product_code': package.code, 'namespace': str(ns), 'version': package.version,
            'msi_sha256': package.sha256, 'file_count': len(package.files), 'compiled_tables_checked': True}


def note_bytes(label, namespace):
    return (f'Isolated Harmony MSI fixture: {label}\nNamespace: {namespace}\n'
            'Fixture MSI version changes are NOT application builds. Never execute the payload.\n').encode('utf-8')


def expected_files(source_files, label, ns, alias, fault):
    files = dict(source_files)
    if alias:
        files['yapfast.exe'] = files.pop('Harmony.exe')
    files[FIXTURE_NOTE] = hashlib.sha256(note_bytes(label, ns)).hexdigest()
    if fault:
        files[NEW_ONLY] = hashlib.sha256(b'New-only rollback canary; must not survive failed upgrade.\n').hexdigest()
    return files


def validate_native_paths(payload, target, files):
    # WiX 4's native cabinet backend still has MAX_PATH-sensitive inputs.
    # Keep the complete notice payload; shorten the scratch work name instead.
    for root in (payload, target):
        for name in files:
            units = len(str(root / name).encode('utf-16-le')) // 2
            require(units < 260, 'Native MSI fixture path exceeds MAX_PATH; use the profile scratch root directly: ' + str(root / name))


def prepare(args):
    builder = builder_module()
    payload, source_files, info = load_release(args.release_receipt)
    require(tuple(map(int, builder.msi_version(info['version'], info['build_id']).split('.'))) >= (1, 1, 1),
            'Release predates the Harmony.exe packaging floor')
    if args.include_fault_rollback:
        require(all('test_fail_after_install_files' in python_inspect.signature(function).parameters
                    for function in (builder.generate_wxs, builder.build_msi)),
                'Builder needs test-namespace-only test_fail_after_install_files=False in generate_wxs/build_msi')
    scratch = scratch_path(args.scratch_root)
    scratch.mkdir(parents=True, exist_ok=True)
    # The directory token is not an MSI identity. All product/component/upgrade
    # namespaces below remain complete independent UUID4 values; mkdir is exclusive.
    work = scratch / ('msi-lifecycle-' + uuid.uuid4().hex[:12])
    work.mkdir()
    namespaces = {group: uuid.uuid4() for group in GROUPS if group != 'rollback' or args.include_fault_rollback}
    packages = []
    for label, group, version, alias, fault in SPECS:
        if group not in namespaces:
            continue
        ns = namespaces[group]
        destination = work / 'payloads' / label
        shutil.copytree(payload, destination)
        if alias:
            (destination / 'Harmony.exe').rename(destination / 'yapfast.exe')
        (destination / FIXTURE_NOTE).write_bytes(note_bytes(label, ns))
        if fault:
            (destination / NEW_ONLY).write_bytes(b'New-only rollback canary; must not survive failed upgrade.\n')
        output = work / 'packages' / (label + '.msi')
        target = work / group / 'program'
        files = expected_files(source_files, label, ns, alias, fault)
        validate_native_paths(destination, target, files)
        kwargs = {'test_namespace': ns, 'test_version': version}
        if fault:
            kwargs['test_fail_after_install_files'] = True
        builder.build_msi(destination, info, output, args.wix_dll, args.ui_extension, **kwargs)
        package = Package(label, group, ns, version, alias, fault, output, destination,
                          target, files, digest(output))
        require(payload_hashes(destination) == package.files, 'Prepared payload identity mismatch')
        db = inspect(output)
        validate_database(db, package, builder.LEGACY_MIGRATION_MESSAGE)
        write_json(work / 'packages' / (label + '-tables.json'), db, exclusive=True)
        packages.append(package)
    plan = {'schema': 1, 'work': str(work), 'release_receipt': str(args.release_receipt.resolve()),
            'release_receipt_sha256': digest(args.release_receipt), 'application_info': info,
            'application_executable_sha256': source_files['Harmony.exe'], 'include_fault_rollback': args.include_fault_rollback,
            'legacy_alias_is_verified_harmony_bytes_not_an_older_real_application': True,
            'packages': [package.record() for package in packages]}
    write_json(work / 'plan.json', plan, exclusive=True)
    return work / 'plan.json'


def load_prepared(path):
    path = scratch_path(path)
    require(path.name == 'plan.json', 'Expected an exact prepared plan.json')
    plan = json.loads(path.read_text(encoding='utf-8'))
    work = scratch_path(plan['work'])
    require(work == path.parent and work.name.startswith('msi-lifecycle-') and plan['schema'] == 1,
            'Prepared plan location/schema mismatch')
    require(type(plan['include_fault_rollback']) is bool, 'Prepared rollback choice must be an explicit boolean')
    require(not (work / 'execution-started.json').exists(), 'Fixture plans are single-use; prepare fresh UUID namespaces')
    require(digest(plan['release_receipt']) == plan['release_receipt_sha256'], 'Producer receipt changed since preparation')
    _, source_files, info = load_release(plan['release_receipt'])
    require(info == plan['application_info'] and source_files['Harmony.exe'] == plan['application_executable_sha256'],
            'Prepared plan no longer matches release')
    specs = [spec for spec in SPECS if spec[1] != 'rollback' or plan['include_fault_rollback']]
    require(len(plan['packages']) == len(specs), 'Prepared fixture set is incomplete')
    packages, namespaces = [], {}
    builder = builder_module()
    for row, (label, group, version, alias, fault) in zip(plan['packages'], specs):
        ns = uuid.UUID(row['namespace'])
        require(str(ns) == row['namespace'] and ns.version == 4 and ns != PRODUCTION_UPGRADE_CODE,
                'Invalid/non-fixture namespace')
        require((row['label'], row['group'], row['version'], row['legacy_alias'], row['fault'])
                == (label, group, version, alias, fault), 'Prepared lifecycle phases changed')
        require(group not in namespaces or namespaces[group] == ns, 'A phase escaped its group namespace')
        namespaces[group] = ns
        package = Package(label, group, ns, version, alias, fault,
                          scratch_path(row['path'], parent=work), scratch_path(row['payload'], parent=work),
                          scratch_path(row['target'], parent=work), row['files'], row['sha256'])
        require(package.path == work / 'packages' / (label + '.msi') and package.payload == work / 'payloads' / label
                and package.target == work / group / 'program', 'Prepared paths changed')
        require(package.files == expected_files(source_files, label, ns, alias, fault)
                and payload_hashes(package.payload) == package.files and digest(package.path) == package.sha256,
                'Prepared package/payload bytes changed')
        validate_native_paths(package.payload, package.target, package.files)
        db = inspect(package.path)
        validate_database(db, package, builder.LEGACY_MIGRATION_MESSAGE)
        write_json(work / 'packages' / (label + '-run-preflight.json'), db)
        packages.append(package)
    require(len(set(namespaces.values())) == len(namespaces), 'Independent scenarios must have disjoint namespaces')
    codes = [package.code for package in packages]
    require(len(codes) == len(set(codes)), 'Duplicate fixture ProductCodes')
    # All tables and all namespaces are checked before the first msiexec call.
    for ns in namespaces.values():
        require(not related_products(ns), 'A supposedly fresh fixture namespace is already registered')
        require(fixture_registry_absent(ns), 'A supposedly fresh fixture registry namespace already exists')
    for package in packages:
        require(product_state(package.code) == -1, 'A supposedly fresh fixture ProductCode is already registered')
    return plan, packages


def read_log(path):
    raw = Path(path).read_bytes()
    return raw.decode('utf-16' if raw.startswith((b'\xff\xfe', b'\xfe\xff')) else 'utf-8', errors='replace')


def action_started(text, action):
    return re.search(r'Action start [^\r\n]*: ' + re.escape(action) + r'\.', text, re.I) is not None


def action_ended(text, action, result):
    return re.search(r'Action ended [^\r\n]*: ' + re.escape(action) + r'\. Return value ' + str(result) + r'\.', text, re.I) is not None


def property_was_set(text, name):
    key = re.escape(name)
    return re.search(r'PROPERTY CHANGE: (?:Adding|Modifying) ' + key + r' property\.[^\r\n]*value is \'[^\'\r\n]+\'', text, re.I) is not None or re.search(
        r'^Property\([SC]\): ' + key + r' = \S+', text, re.M) is not None


def refusal_evidence(text, *, message, legacy_signal=None):
    require(' '.join(message.split()) in ' '.join(text.split()), 'Refusal log does not contain the exact policy message')
    require(action_started(text, 'LaunchConditions') and action_ended(text, 'LaunchConditions', 3),
            'Refusal must be an actual failed LaunchConditions action, not an unrelated nonzero exit')
    require(not action_started(text, 'InstallInitialize') and not action_started(text, 'RemoveExistingProducts'),
            'Refusal occurred after transaction/removal started')
    if legacy_signal:
        require(property_was_set(text, legacy_signal), 'Refusal did not detect the intended isolated legacy signal')
        other = 'HARMONY_LEGACY_COMPONENT' if legacy_signal == 'HARMONY_LEGACY_MSI' else 'HARMONY_LEGACY_MSI'
        require(not property_was_set(text, other), 'Legacy signals were not independently exercised')
    else:
        require(property_was_set(text, 'WIX_DOWNGRADE_DETECTED'), 'Downgrade guard did not detect the newer fixture')
    return {'guard': legacy_signal or 'WIX_DOWNGRADE_DETECTED', 'launch_conditions_failed': True,
            'install_initialize_started': False, 'remove_existing_products_started': False}


def rollback_evidence(text, old_code):
    require(old_code.casefold() in text.casefold(), 'Exact predecessor ProductCode is absent from the rollback log')
    require(re.search(r'Removal success or error status: 0\.', text, re.I), 'Predecessor removal was not logged as successful')
    require(FAULT_MESSAGE in text and action_started(text, FAULT_ACTION) and action_ended(text, FAULT_ACTION, 3),
            'Failure was not the explicit fixture rollback fault')
    failure = re.search(r'Action start .*: ' + FAULT_ACTION + r'\.', text, re.I)
    if failure is None:
        raise ValueError('Fixture fault action did not start')
    removals = [match for match in re.finditer(r'Action ended .*: RemoveExistingProducts\. Return value 1\.', text, re.I)
                if match.end() < failure.start()]
    require(bool(removals), 'No completed predecessor removal before the fault')
    removal = removals[-1]
    flushes = [match for match in re.finditer(r'Action ended .*: InstallExecute\. Return value 1\.', text, re.I)
               if removal.end() < match.start() < failure.start()]
    require(bool(flushes), 'Deferred payload writes did not finish after predecessor removal and before the fixture fault')
    forward_writes = text[removal.end():flushes[-1].start()]
    for name in (FIXTURE_NOTE, NEW_ONLY):
        require(re.search(r'Executing op: FileCopy\(.*DestName=' + re.escape(name) + r'(?:,|\))', forward_writes, re.I),
                'No forward physical file-copy evidence for rollback marker: ' + name)
    require(re.search(r'Rolling back action:|Rollback:|Executing rollback|RollbackInfo', text[failure.end():], re.I),
            'No native rollback evidence after the fixture fault')
    return {'predecessor_removal_completed': True, 'install_execute_completed': True,
            'overwritten_and_new_only_marker_copies_logged': True, 'fault_action_failed': True, 'rollback_logged': True,
            'scope': 'Type 19 after InstallExecute flushed deferred writes; physical payload/predecessor rollback, not crash recovery'}


class Lifecycle:
    """Executor accepts only a fully inspected, single-use prepared fixture plan."""
    def __init__(self, plan, packages):
        self.plan, self.packages = plan, packages
        self.work = scratch_path(plan['work'])
        self.by_label = {package.label: package for package in packages}
        self.steps, self.evidence, self.cleanup = [], [], []
        self.canaries = {}
        self.env = dict(os.environ, TMPDIR=str(self.work), TMP=str(self.work), TEMP=str(self.work),
                        HARMONY_TEST_SCRATCH=str(self.work))
        self.msiexec = str(Path(os.environ['SYSTEMROOT']) / 'System32/msiexec.exe')
        self.flags = getattr(subprocess, 'BELOW_NORMAL_PRIORITY_CLASS', 0) | getattr(subprocess, 'CREATE_NO_WINDOW', 0)

    def initialize(self):
        for group in dict.fromkeys(package.group for package in self.packages):
            target = self.work / group / 'program'
            data = self.work / group / 'fake-user-data'
            target.mkdir(parents=True)
            data.mkdir()
            for path, content in ((target / UNOWNED, b'Unowned file; MSI must leave these exact bytes alone.\n'),
                                  (data / 'settings.json', b'{"fixture_only":true,"preserve":"all phases"}\n')):
                path.write_bytes(content)
                self.canaries[path] = digest(path)
        self.invariants()

    def invariants(self):
        for path, value in self.canaries.items():
            scratch_path(path, parent=self.work)
            require(path.is_file() and digest(path) == value, 'Unowned/fake-user-data canary changed: ' + str(path))
        return {str(path.relative_to(self.work)): digest(path) for path in self.canaries}

    def verify(self, label, group, installed):
        candidates = [package for package in self.packages if package.group == group]
        states = {package.code: product_state(package.code) for package in candidates}
        require(all(state == (5 if installed is not None and code == installed.code else -1)
                    for code, state in states.items()), 'Exact fixture registration state mismatch: ' + label)
        target = self.work / group / 'program'
        actual = payload_hashes(target)
        expected = dict(installed.files) if installed else {}
        expected[UNOWNED] = self.canaries[target / UNOWNED]
        require(actual == expected, 'Exact installed payload bytes/file set mismatch: ' + label)
        remembered = {'InstallFolder': fixture_install_folder(candidates[0].namespace),
                      'FixtureProgramsRoot': fixture_programs_root(candidates[0].namespace)}
        if installed is not None:
            require(all(value is not None for value in remembered.values())
                    and scratch_path(remembered['InstallFolder'], parent=self.work) == target
                    and scratch_path(remembered['FixtureProgramsRoot'], parent=self.work) == target.parent,
                    'Read-back fixture program-folder registration mismatch: ' + label)
        else:
            require(all(value is None for value in remembered.values()), 'Fixture program-folder values survived uninstall')
        row = {'label': label, 'group': group, 'product_states': states,
               'installed_payload_sha256': actual, 'fixture_program_path_registry': remembered, 'canaries': self.invariants()}
        self.evidence.append(row)
        write_json(self.work / 'evidence.json', self.evidence)
        return row

    def operate(self, label, package, mode, *, allowed=(0,)):
        require(package in self.packages, 'Operation on an uninspected fixture refused')
        scratch_path(package.target, parent=self.work)
        scratch_path(package.path, parent=self.work)
        require(digest(package.path) == package.sha256, 'MSI bytes changed after inspection')
        require(package.namespace != PRODUCTION_UPGRADE_CODE and package.code == guid_text(uuid.uuid5(package.namespace, 'product:' + package.version)),
                'Operation identity escaped fixture namespace')
        remembered = fixture_install_folder(package.namespace)
        if remembered is not None:
            require(scratch_path(remembered, parent=self.work) == package.target, 'Fixture remembered folder escaped its exact target')
        ancestor = fixture_programs_root(package.namespace)
        require((remembered is None) == (ancestor is None), 'Incomplete fixture program-folder registration')
        if ancestor is not None:
            require(scratch_path(ancestor, parent=self.work) == package.target.parent,
                    'Fixture remembered ancestor escaped its exact scratch target')
        log = self.work / (label + '.log')
        require(not log.exists(), 'Native operation logs are never reused')
        if mode == 'install':
            operation = ['/i', str(package.path), 'INSTALLFOLDER=' + str(package.target)]
        elif mode == 'repair':
            operation = ['/fa', str(package.path)]
        elif mode == 'uninstall':
            operation = ['/x', package.code]
        else:
            raise ValueError('Unrecognized MSI operation')
        command = [self.msiexec, *operation, '/qn', '/norestart', 'REBOOT=ReallySuppress',
                   FIXTURE_PROGRAMS + '=' + str(package.target.parent),
                   'MSIRESTARTMANAGERCONTROL=DisableShutdown', 'MSIDISABLERMRESTART=1', '/l*v', str(log)]
        start = time.monotonic()
        row = {'label': label, 'command': command, 'product_code': package.code,
               'msi_sha256': package.sha256, 'log': str(log), 'exit': None}
        self.steps.append(row)
        try:
            result = subprocess.run(command, env=self.env, capture_output=True, text=True, encoding='utf-8',
                                    errors='replace', timeout=600, creationflags=self.flags)
            row.update(exit=result.returncode, stdout=result.stdout, stderr=result.stderr)
            require(result.returncode in allowed, f'{label} exited {result.returncode}; retain native logs under {self.work}')
            require(log.is_file() and log.stat().st_size > 0, 'Native MSI evidence log is missing')
            row['log_sha256'] = digest(log)
            row['canaries'] = self.invariants()
            return read_log(log)
        except BaseException as error:
            row['error'] = repr(error)
            raise
        finally:
            row['seconds'] = time.monotonic() - start
            write_json(self.work / 'steps.json', self.steps)

    def legacy(self, group, signal, message):
        old, candidate = self.by_label[group + '-old'], self.by_label[group + '-candidate']
        self.operate(group + '-install', old, 'install')
        self.verify(group + '-installed', group, old)
        text = self.operate(group + '-refused', candidate, 'install', allowed=(1603,))
        proof = refusal_evidence(text, message=message, legacy_signal=signal)
        self.verify(group + '-refusal-preserved-old', group, old)['refusal'] = proof
        # This explicit uninstall is a distinct step, never an automatic migration.
        self.operate(group + '-explicit-uninstall', old, 'uninstall')
        self.verify(group + '-explicitly-uninstalled', group, None)

    def renamed(self):
        old, new = self.by_label['renamed-base'], self.by_label['renamed-upgrade']
        self.operate('renamed-fresh-install', old, 'install')
        self.verify('renamed-fresh-installed', 'renamed', old)
        self.operate('renamed-significant-version-upgrade', new, 'install')
        self.verify('renamed-upgraded-old-unregistered', 'renamed', new)
        (new.target / 'Harmony.exe').unlink()
        require(not (new.target / 'Harmony.exe').exists(), 'Missing-file repair precondition failed')
        self.operate('renamed-missing-Harmony-exe-repair', new, 'repair')
        self.verify('renamed-repaired', 'renamed', new)
        message = 'A newer Harmony MSI is already installed. Use that version or a newer installer.'
        text = self.operate('renamed-downgrade-refused', old, 'install', allowed=(1603,))
        proof = refusal_evidence(text, message=message)
        self.verify('renamed-downgrade-preserved-new', 'renamed', new)['refusal'] = proof
        self.operate('renamed-uninstall', new, 'uninstall')
        self.verify('renamed-uninstalled', 'renamed', None)

    def rollback(self):
        old, fault = self.by_label['rollback-base'], self.by_label['rollback-fault']
        require(old.files[FIXTURE_NOTE] != fault.files[FIXTURE_NOTE] and NEW_ONLY not in old.files and NEW_ONLY in fault.files,
                'Rollback must exercise distinguishable overwritten and new-only owned bytes')
        self.operate('rollback-base-install', old, 'install')
        self.verify('rollback-base-installed', 'rollback', old)
        text = self.operate('rollback-injected-upgrade-failure', fault, 'install', allowed=(1603,))
        proof = rollback_evidence(text, old.code)
        self.verify('rollback-predecessor-registration-and-bytes-restored', 'rollback', old)['rollback'] = proof
        require(not (old.target / NEW_ONLY).exists(), 'New-only failed-upgrade file survived rollback')
        self.operate('rollback-explicit-uninstall', old, 'uninstall')
        self.verify('rollback-uninstalled', 'rollback', None)

    def cleanup_all(self):
        # Independent best effort for EVERY exact preflighted ProductCode; an
        # error cleaning one product must not skip other namespaces/products.
        errors = []
        for package in reversed(self.packages):
            row = {'product_code': package.code, 'label': package.label}
            self.cleanup.append(row)
            try:
                state = product_state(package.code)
                row['before'] = state
                if state != -1:
                    self.operate('cleanup-' + package.label, package, 'uninstall')
                row['after'] = product_state(package.code)
                require(row['after'] == -1, 'Fixture remains registered after cleanup')
            except BaseException as error:
                row['error'] = repr(error)
                errors.append(row)
        for group in dict.fromkeys(package.group for package in self.packages):
            try:
                self.verify('finally-cleanup-' + group, group, None)
                namespace = next(package.namespace for package in self.packages if package.group == group)
                require(not related_products(namespace), 'Related fixture product remains after cleanup')
                require(fixture_registry_absent(namespace), 'Fixture registry namespace remains after cleanup')
            except BaseException as error:
                errors.append({'group': group, 'error': repr(error)})
        write_json(self.work / 'cleanup.json', {'products': self.cleanup, 'errors': errors})
        return errors

    def execute(self):
        error, cleanup_errors = None, []
        write_json(self.work / 'execution-started.json', {'single_use': True, 'namespaces': sorted({str(p.namespace) for p in self.packages})}, exclusive=True)
        try:
            self.initialize()
            message = builder_module().LEGACY_MIGRATION_MESSAGE
            self.legacy('legacy-version', 'HARMONY_LEGACY_MSI', message)
            self.legacy('legacy-marker', 'HARMONY_LEGACY_COMPONENT', message)
            self.renamed()
            if self.plan['include_fault_rollback']:
                self.rollback()
        except BaseException as caught:
            error = repr(caught)
        finally:
            cleanup_errors = self.cleanup_all()
            write_json(self.work / 'evidence.json', self.evidence)
            report = {
                'status': 'PASS_REAL_ISOLATED_MSI_LIFECYCLE' if error is None and not cleanup_errors else 'FAIL_REAL_ISOLATED_MSI_LIFECYCLE',
                'work': str(self.work), 'error': error, 'cleanup_errors': cleanup_errors,
                'application_info': self.plan['application_info'],
                'application_executable_sha256': self.plan['application_executable_sha256'],
                'fixture_packages': [p.record() for p in self.packages], 'steps': self.steps,
                'evidence': self.evidence, 'cleanup': self.cleanup,
                'fixture_versions_are_not_new_application_builds': True,
                'legacy_alias_is_verified_harmony_bytes_not_an_older_real_application': True,
                'production_product_installed': False, 'application_launched_or_processes_inspected_or_controlled': False,
                'live_program_or_user_data_read_or_modified': False,
                'interactive_wizard_execution': 'not exercised; both compiled sequence tables checked; execution is /qn only',
                'fault_rollback': 'physical deferred payload/predecessor-removal rollback exercised' if self.plan['include_fault_rollback'] and error is None and not cleanup_errors
                                  else 'not claimed; see error/evidence' if self.plan['include_fault_rollback'] else 'not requested; no rollback claim',
                'crash_recovery': 'not exercised; controlled Type 19 failure after InstallExecute',
            }
            write_json(self.work / 'result.json', report, exclusive=True)
        print(json.dumps({'status': report['status'], 'work': str(self.work), 'result': str(self.work / 'result.json'),
                          'error': error, 'cleanup_errors': cleanup_errors}, indent=2))
        return 0 if report['status'].startswith('PASS_') else 1


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    inputs = parser.add_mutually_exclusive_group(required=True)
    inputs.add_argument('--release-receipt', type=Path, help='Verified current builder .msi.json (never installed)')
    inputs.add_argument('--run-prepared', type=Path, help='Single-use scratch plan.json; reinspects every MSI before msiexec')
    parser.add_argument('--wix-dll', type=Path)
    parser.add_argument('--ui-extension', type=Path)
    parser.add_argument('--scratch-root', type=Path, default=PROFILE_SCRATCH)
    parser.add_argument('--prepare-only', action='store_true', help='Build/inspect fixtures only, never run msiexec')
    parser.add_argument('--include-fault-rollback', action='store_true', help='Requires the explicit test-only builder fault API')
    parser.add_argument('--allow-isolated-msi-registration', action='store_true')
    args = parser.parse_args(argv)
    if not args.prepare_only and not args.allow_isolated_msi_registration:
        parser.error('Native registration requires --allow-isolated-msi-registration')
    if args.run_prepared and (args.prepare_only or args.include_fault_rollback or args.wix_dll or args.ui_extension):
        parser.error('--run-prepared accepts no build/prepare options')
    if args.release_receipt and (not args.wix_dll or not args.ui_extension):
        parser.error('Preparation requires both --wix-dll and --ui-extension')
    require(os.name == 'nt', 'Native lifecycle preparation/execution requires Windows')
    plan_path = args.run_prepared or prepare(args)
    if args.prepare_only:
        print(json.dumps({'status': 'PREPARED_ONLY_NOT_NATIVE_LIFECYCLE_EVIDENCE', 'plan': str(plan_path)}, indent=2))
        return 0
    plan, packages = load_prepared(plan_path)
    return Lifecycle(plan, packages).execute()


if __name__ == '__main__':
    raise SystemExit(main())
