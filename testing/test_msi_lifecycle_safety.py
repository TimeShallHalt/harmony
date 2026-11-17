"""Offline lifecycle safety tests: synthetic tables/mocks, NEVER native MSI evidence.

Run with python3 -B -m unittest testing.test_msi_lifecycle_safety -v.
No WiX compilation, msiexec, application execution, or production-registry access.
"""
import argparse
import ast
import copy
import ctypes as c
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from typing import Any
from unittest.mock import patch
import uuid

from testing import msi_database as database
from testing import run_msi_lifecycle as runner

MESSAGE = 'Unit fixture legacy refusal message; not an observed MSI log.'


def fake_database(package):
    """Explicit synthetic table model, used only to attack the pure validator."""
    ns = package.namespace
    raw = {name: [] for name in database.TABLE_COLUMNS}
    raw['Directory'] = [['TARGETDIR', '', 'SourceDir'], ['LocalAppDataFolder', 'TARGETDIR', '.'],
                        [runner.FIXTURE_PROGRAMS, 'LocalAppDataFolder', 'Programs'],
                        ['INSTALLFOLDER', runner.FIXTURE_PROGRAMS, 'short|Harmony-MSI-Fixture-' + ns.hex]]
    raw['Component'] = [['ProgramFolderRegistration', database.guid_text(uuid.uuid5(ns, 'install-folder')),
                         'INSTALLFOLDER', '260', '', 'RegFolder']]
    raw['Registry'] = [['RegFolder', '1', package.registry, 'InstallFolder', '[INSTALLFOLDER]', 'ProgramFolderRegistration']]
    raw['Registry'].append(['FixtureRootValue', '1', package.registry, 'FixtureProgramsRoot',
                            '[' + runner.FIXTURE_PROGRAMS + ']', 'ProgramFolderRegistration'])
    for index, name in enumerate(package.files, 1):
        component = runner.identifier('C_', name)
        reg = 'Reg' + str(index)
        raw['Component'].append([component, database.guid_text(uuid.uuid5(ns, 'file:' + name.casefold())),
                                 'INSTALLFOLDER', '260', '', reg])
        raw['Registry'].append([reg, '1', package.registry + r'\Components', component, '#1', component])
        raw['File'].append([runner.identifier('F_', name), component, 'short|' + name,
                            str((package.payload / name).stat().st_size), '', '', '512', str(index)])
    raw['RemoveFile'] = [[name, 'ProgramFolderRegistration', '', directory, '2']
                         for name, directory in [('RemoveRoot', 'INSTALLFOLDER'), ('RemoveParent', runner.FIXTURE_PROGRAMS)]]
    raw['Upgrade'] = [
        [database.guid_text(ns), '', '1.1.1', '', '2', '', 'HARMONY_LEGACY_MSI'],
        [database.guid_text(ns), '', package.version, '', '1', '', 'WIX_UPGRADE_DETECTED'],
        [database.guid_text(ns), package.version, '', '', '2', '', 'WIX_DOWNGRADE_DETECTED']]
    raw['LaunchCondition'] = [[runner.LEGACY_CONDITION, MESSAGE], ['VersionNT64', 'x64 only'],
                              ['NOT ALLUSERS', 'per-user only'], ['NOT WIX_DOWNGRADE_DETECTED', 'newer product']]
    raw['RegLocator'] = [['LegacyExecutableComponent', '1', package.registry + r'\Components', runner.LEGACY_MARKER, '18'],
                         ['RememberedProgramFolder', '1', package.registry, 'InstallFolder', '18'],
                         ['RememberedFixtureProgramsRoot', '1', package.registry, 'FixtureProgramsRoot', '18']]
    raw['AppSearch'] = [['HARMONY_LEGACY_COMPONENT', 'LegacyExecutableComponent'], ['INSTALLFOLDER', 'RememberedProgramFolder'],
                        [runner.FIXTURE_PROGRAMS, 'RememberedFixtureProgramsRoot']]
    raw['InstallUISequence'] = [[name, '', value] for name, value in
                                [('FindRelatedProducts', '25'), ('AppSearch', '50'), ('LaunchConditions', '100')]]
    raw['InstallExecuteSequence'] = copy.deepcopy(raw['InstallUISequence']) + [
        ['CostFinalize', '', '1000'], ['SetARPINSTALLLOCATION', '', '1001'], ['InstallInitialize', '', '1500'],
        ['RemoveExistingProducts', '', '1501'], ['InstallFiles', '', '4000'], ['InstallFinalize', '', '6600']]
    raw['CustomAction'] = [['SetARPINSTALLLOCATION', '51', 'ARPINSTALLLOCATION', '[INSTALLFOLDER]'],
                           ['WixUIPrintEula', '65', 'WixUI_CA', 'PrintEula']]
    if package.fault:
        raw['CustomAction'].append([runner.FAULT_ACTION, '19', '', runner.FAULT_MESSAGE])
        raw['InstallExecuteSequence'].append(['InstallExecute', '', '6500'])
        raw['InstallExecuteSequence'].append([runner.FAULT_ACTION, 'NOT REMOVE', '6501'])
    properties = {'UpgradeCode': database.guid_text(ns), 'ProductCode': package.code, 'ProductVersion': package.version,
                  'ProductName': 'Harmony MSI Lifecycle Fixture', 'ARPSYSTEMCOMPONENT': '1', 'REINSTALLMODE': 'amus',
                  'REBOOT': 'ReallySuppress', 'MSIRESTARTMANAGERCONTROL': 'DisableShutdown', 'MSIDISABLERMRESTART': '1',
                  'SecureCustomProperties': 'INSTALLFOLDER;' + runner.FIXTURE_PROGRAMS + ';HARMONY_LEGACY_MSI;HARMONY_LEGACY_COMPONENT'}
    raw['Property'] = [[key, value] for key, value in properties.items()]
    return {'properties': properties, 'raw_tables': raw, 'table_names': sorted(raw), 'summary_template': 'x64;1033'}


def refusal_log(signal, message=MESSAGE):
    # These are deliberately synthetic parser inputs, not retained/native receipts.
    return (f"PROPERTY CHANGE: Adding {signal} property. Its value is '{{unit-only}}'.\n"
            f'Action start 12:00:00: LaunchConditions.\n{message}\n'
            'Action ended 12:00:00: LaunchConditions. Return value 3.\n')


class ScratchTest(unittest.TestCase):
    def setUp(self):
        scratch = Path(os.environ.get('HARMONY_TEST_SCRATCH', runner.PROFILE_SCRATCH))
        runner.scratch_path(scratch)
        scratch.mkdir(parents=True, exist_ok=True)
        self.folder = tempfile.TemporaryDirectory(prefix='msi-safety-unit-', dir=scratch)
        self.addCleanup(self.folder.cleanup)
        self.root = Path(self.folder.name)
        # A test can never accidentally become a native build/install/API test.
        self.native = patch.object(database, 'msi_api', side_effect=AssertionError('Native MSI API forbidden in unit tests')).start()
        self.addCleanup(patch.stopall)
        self.process = patch.object(runner.subprocess, 'run', side_effect=AssertionError('No subprocess in safety unit tests')).start()
        patch.object(runner, 'fixture_registry_absent', side_effect=AssertionError('No native registry reads in unit tests')).start()
        patch.object(runner, 'fixture_install_folder', side_effect=AssertionError('No native registry reads in unit tests')).start()
        patch.object(runner, 'fixture_programs_root', side_effect=AssertionError('No native registry reads in unit tests')).start()

    def package(self, label='renamed-base', group='renamed', namespace=None, version='1.1.1', fault=False, alias=False):
        ns = namespace or uuid.uuid4()
        payload = self.root / 'payloads' / label
        payload.mkdir(parents=True)
        filename = 'yapfast.exe' if alias else 'Harmony.exe'
        (payload / filename).write_bytes(b'SYNTHETIC UNIT DATA, NOT A REAL APPLICATION')
        (payload / runner.FIXTURE_NOTE).write_bytes(runner.note_bytes(label, ns))
        if fault:
            (payload / runner.NEW_ONLY).write_bytes(b'unit new-only bytes')
        package_path = self.root / (label + '.msi')
        package_path.write_bytes(b'NOT AN MSI; UNIT SAFETY MODEL ONLY')
        return runner.Package(label, group, ns, version, alias, fault, package_path, payload,
                              self.root / group / 'program', runner.payload_hashes(payload), runner.digest(package_path))


class BoundaryTests(ScratchTest):
    def test_active_profile_scratch_and_descendants_only(self):
        self.assertEqual(runner.scratch_path(self.root), self.root.resolve())
        for invalid in (Path.home() / 'AppData/Local/hermes/cache/scratch',
                        Path.home() / 'AppData/Local/Programs/Harmony', self.root / '..' / 'escape', Path('relative')):
            with self.subTest(path=invalid), self.assertRaises(ValueError):
                runner.scratch_path(invalid)

    def test_reparse_ancestors_fail_closed(self):
        class Stat:
            st_file_attributes = 0x400
        with patch.object(Path, 'lstat', return_value=Stat()):
            with self.assertRaisesRegex(ValueError, 'Reparse'):
                runner.no_reparse(self.root)

    def test_windows_payload_tokens_are_validated_without_repair(self):
        for name in ('../escape', 'a/../b', '/absolute', 'C:/absolute', 'a\\b', 'a//b', './b',
                     'a:stream', 'CON.txt', 'a/NUL', 'trailing.', 'trailing ', 'wild*.txt', ''):
            with self.subTest(name=name), self.assertRaises(ValueError):
                runner.relative_name(name)
        self.assertEqual(runner.relative_name('notices/license.txt').as_posix(), 'notices/license.txt')

    def test_guid_tokens_are_not_repaired(self):
        value = uuid.uuid4()
        self.assertEqual(database.guid_text(str(value)), '{' + str(value).upper() + '}')
        for token in (value.hex, ' ' + str(value), '{' + str(value), str(value) + '}', 'not-a-guid'):
            with self.subTest(token=token), self.assertRaises(ValueError):
                database.guid_text(token)

    def test_alias_is_the_same_verified_bytes_not_a_historical_app(self):
        files = {'Harmony.exe': 'a' * 64, 'LICENSE.txt': 'b' * 64}
        transformed = runner.expected_files(files, 'legacy-marker-old', uuid.uuid4(), True, False)
        self.assertEqual(transformed['yapfast.exe'], files['Harmony.exe'])
        self.assertNotIn('Harmony.exe', transformed)
        self.assertEqual(files['Harmony.exe'], 'a' * 64)

    def test_namespaces_and_significant_versions_are_independent(self):
        self.assertEqual([(spec[2], spec[3]) for spec in runner.SPECS[:4]],
                         [('1.1.0', False), ('1.1.2', False), ('1.1.1', True), ('1.1.2', False)])
        self.assertEqual([spec[2] for spec in runner.SPECS[4:]], ['1.1.1', '1.1.2', '1.1.1', '1.1.2'])
        a, b = self.package(), self.package('renamed-upgrade', version='1.1.2')
        self.assertNotEqual(a.code, b.code)
        self.assertNotEqual(a.namespace, runner.PRODUCTION_UPGRADE_CODE)

    def test_registration_opt_in_is_checked_before_preparation(self):
        with patch.object(runner, 'prepare') as prepare:
            with patch('sys.stderr', new=io.StringIO()), self.assertRaises(SystemExit) as raised:
                runner.main(['--release-receipt', 'unused.json', '--wix-dll', 'unused.dll', '--ui-extension', 'unused.dll'])
            self.assertEqual(raised.exception.code, 2)
            prepare.assert_not_called()
            self.process.assert_not_called()

    def test_unit_suite_never_calls_native_tools(self):
        tree = ast.parse(Path(runner.__file__).read_text(encoding='utf-8'))
        calls = [node for node in ast.walk(tree) if isinstance(node, ast.Call)]
        disallowed = {'OpenProcess', 'WaitForSingleObject', 'Popen', 'startfile', 'TerminateProcess', 'kill'}
        self.assertFalse(any(isinstance(node.func, ast.Attribute) and node.func.attr in disallowed for node in calls))
        methods = {node.name: node for node in ast.walk(tree) if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef))}
        process_callers = [name for name, node in methods.items() if any(
            isinstance(child, ast.Call) and isinstance(child.func, ast.Attribute)
            and isinstance(child.func.value, ast.Name) and child.func.value.id == 'subprocess'
            for child in ast.walk(node))]
        self.assertEqual(process_callers, ['operate'])
        self.assertFalse(any(isinstance(node, ast.Assert) for node in ast.walk(tree)), 'Guards must survive -O')


class TableSafetyTests(ScratchTest):
    def setUp(self):
        super().setUp()
        self.pkg = self.package()
        self.db = fake_database(self.pkg)

    def validate(self):
        return runner.validate_database(self.db, self.pkg, MESSAGE)

    def test_exact_synthetic_fixture_tables_pass_pure_validator(self):
        self.assertTrue(self.validate()['compiled_tables_checked'])
        self.process.assert_not_called()

    def test_production_upgrade_identity_is_refused(self):
        self.db['properties']['UpgradeCode'] = database.guid_text(runner.PRODUCTION_UPGRADE_CODE)
        with self.assertRaisesRegex(ValueError, 'identity'):
            self.validate()

    def test_production_component_identity_is_refused(self):
        row = self.db['raw_tables']['Component'][1]
        row[1] = database.guid_text(uuid.uuid5(runner.PRODUCTION_UPGRADE_CODE, 'file:harmony.exe'))
        with self.assertRaisesRegex(ValueError, 'Component'):
            self.validate()

    def test_registry_write_and_search_are_exactly_namespace_scoped(self):
        for table in ('Registry', 'RegLocator'):
            with self.subTest(table=table):
                db = copy.deepcopy(self.db)
                db['raw_tables'][table][0][2] = r'Software\Harmony\Installer'
                with self.assertRaises(ValueError):
                    runner.validate_database(db, self.pkg, MESSAGE)

    def test_target_defaults_cannot_escape_to_production(self):
        self.db['raw_tables']['Directory'][3][2] = 'Harmony'
        with self.assertRaisesRegex(ValueError, 'directory'):
            self.validate()

    def test_real_programs_ancestor_cannot_be_an_uninstall_target(self):
        self.db['raw_tables']['Directory'][2][0] = 'HarmonyPrograms'
        self.db['raw_tables']['Directory'][3][1] = 'HarmonyPrograms'
        self.db['raw_tables']['RemoveFile'][1][3] = 'HarmonyPrograms'
        with self.assertRaisesRegex(ValueError, 'directory'):
            self.validate()

    def test_payload_file_names_and_counts_are_exact(self):
        for field, value in ((2, 'yapfast.exe'), (3, '999')):
            db = copy.deepcopy(self.db)
            db['raw_tables']['File'][0][field] = value
            with self.subTest(field=field), self.assertRaisesRegex(ValueError, 'file table'):
                runner.validate_database(db, self.pkg, MESSAGE)

    def test_both_ui_and_silent_guard_sequences_are_checked(self):
        for table in ('InstallUISequence', 'InstallExecuteSequence'):
            db = copy.deepcopy(self.db)
            db['raw_tables'][table][2][2] = '1600'
            with self.subTest(table=table), self.assertRaisesRegex(ValueError, 'sequencing'):
                runner.validate_database(db, self.pkg, MESSAGE)

    def test_removal_must_be_inside_transaction_after_legacy_guard(self):
        self.db['raw_tables']['InstallExecuteSequence'][6][2] = '1400'
        with self.assertRaisesRegex(ValueError, 'transactional'):
            self.validate()

    def test_only_detect_legacy_range_cannot_remove_old_product(self):
        for attributes in ('0', '514'):
            db = copy.deepcopy(self.db)
            db['raw_tables']['Upgrade'][0][4] = attributes
            with self.subTest(attributes=attributes), self.assertRaisesRegex(ValueError, 'detect-only'):
                runner.validate_database(db, self.pkg, MESSAGE)

    def test_unsafe_integration_tables_are_refused(self):
        for table in ('Shortcut', 'ServiceControl', 'Environment', 'RemoveRegistry', 'MoveFiles', 'CreateFolder'):
            db = copy.deepcopy(self.db)
            db['table_names'].append(table)
            with self.subTest(table=table), self.assertRaisesRegex(ValueError, 'integration table'):
                runner.validate_database(db, self.pkg, MESSAGE)

    def test_recursive_or_file_removal_is_refused(self):
        self.db['raw_tables']['RemoveFile'][0][2] = '*'
        with self.assertRaisesRegex(ValueError, 'nonrecursive'):
            self.validate()

    def test_restart_and_relaunch_policy_cannot_be_relaxed(self):
        for key in ('REBOOT', 'MSIRESTARTMANAGERCONTROL', 'MSIDISABLERMRESTART'):
            db = copy.deepcopy(self.db)
            db['properties'][key] = '0'
            with self.subTest(key=key), self.assertRaisesRegex(ValueError, 'policy'):
                runner.validate_database(db, self.pkg, MESSAGE)

    def test_no_unreviewed_executable_custom_action(self):
        self.db['raw_tables']['CustomAction'].append(['LaunchApp', '18', 'HarmonyExe', ''])
        with self.assertRaisesRegex(ValueError, 'Unreviewed'):
            self.validate()

    def test_fault_api_is_exact_and_never_accepted_on_normal_package(self):
        fault = self.package('rollback-fault', group='rollback', fault=True, version='1.1.2')
        db = fake_database(fault)
        runner.validate_database(db, fault, MESSAGE)
        for index, value in ((1, 'NOT Installed'), (2, '1499'), (2, '4001')):
            bad = copy.deepcopy(db)
            bad['raw_tables']['InstallExecuteSequence'][-1][index] = value
            with self.subTest(index=index), self.assertRaisesRegex(ValueError, 'Fault'):
                runner.validate_database(bad, fault, MESSAGE)
        fault.fault = False
        with self.assertRaisesRegex(ValueError, 'fault'):
            runner.validate_database(db, fault, MESSAGE)


class EvidenceTests(ScratchTest):
    def test_legacy_refusal_requires_actual_guard_and_independent_signal(self):
        for signal in ('HARMONY_LEGACY_MSI', 'HARMONY_LEGACY_COMPONENT'):
            text = refusal_log(signal)
            self.assertEqual(runner.refusal_evidence(text, message=MESSAGE, legacy_signal=signal)['guard'], signal)
            for bad in (text.replace('Return value 3.', 'Return value 1.'),
                        text + 'Action start 12:00:01: RemoveExistingProducts.\n',
                        text + 'Action start 12:00:01: InstallInitialize.\n',
                        text.replace(MESSAGE, 'Unrelated install failure'),
                        text.replace(signal, 'OTHER_PROPERTY')):
                with self.subTest(signal=signal, bad=bad), self.assertRaises(ValueError):
                    runner.refusal_evidence(bad, message=MESSAGE, legacy_signal=signal)
        with self.assertRaisesRegex(ValueError, 'independently'):
            runner.refusal_evidence(refusal_log('HARMONY_LEGACY_MSI') + refusal_log('HARMONY_LEGACY_COMPONENT'),
                                    message=MESSAGE, legacy_signal='HARMONY_LEGACY_MSI')

    def test_downgrade_nonzero_without_newer_version_signal_is_not_proof(self):
        self.assertEqual(runner.refusal_evidence(refusal_log('WIX_DOWNGRADE_DETECTED'), message=MESSAGE)['guard'],
                         'WIX_DOWNGRADE_DETECTED')
        with self.assertRaisesRegex(ValueError, 'newer fixture'):
            runner.refusal_evidence(refusal_log('WRONG_SIGNAL'), message=MESSAGE)

    def test_native_log_encoding_both_utf16_and_utf8(self):
        for encoding in ('utf-16', 'utf-8'):
            path = self.root / ('log-' + encoding)
            path.write_bytes('MSI fixture evidence text'.encode(encoding))
            self.assertEqual(runner.read_log(path), 'MSI fixture evidence text')

    def test_rollback_requires_ordered_old_removal_exact_fault_and_native_rollback(self):
        code = database.guid_text(uuid.uuid4())
        text = (f'Old product: {code}\nRemoval success or error status: 0.\n'
                'Action ended 12:00:00: RemoveExistingProducts. Return value 1.\n'
                f'Executing op: FileCopy(SourceName=unit,DestName={runner.FIXTURE_NOTE},Attributes=512)\n'
                f'Executing op: FileCopy(SourceName=unit,DestName={runner.NEW_ONLY},Attributes=512)\n'
                'Action ended 12:00:01: InstallExecute. Return value 1.\n'
                f'Action start 12:00:01: {runner.FAULT_ACTION}.\n{runner.FAULT_MESSAGE}\n'
                f'Action ended 12:00:01: {runner.FAULT_ACTION}. Return value 3.\nRolling back action:\n')
        self.assertTrue(runner.rollback_evidence(text, code)['rollback_logged'])
        self.assertTrue(runner.rollback_evidence('Action ended 11:59:00: InstallExecute. Return value 1.\n' + text, code)['rollback_logged'])
        copy_lines = [line for line in text.splitlines() if 'Executing op: FileCopy' in line]
        misplaced = '\n'.join(line for line in text.splitlines() if line not in copy_lines) + '\n' + '\n'.join(copy_lines)
        with self.assertRaisesRegex(ValueError, 'forward physical'):
            runner.rollback_evidence(misplaced, code)
        for bad in (text.replace(code, '{NOT-THE-OLD-PRODUCT}'), text.replace('status: 0.', 'status: 1.'),
                    text.replace(runner.FAULT_MESSAGE, 'unrelated failure'), text.replace('Rolling back action:', 'No rollback'),
                    text.replace('InstallExecute. Return value 1.', 'InstallExecute. Return value 3.'),
                    text.replace('DestName=' + runner.FIXTURE_NOTE, 'DestName=wrong'),
                    text.replace('RemoveExistingProducts. Return value 1.', 'RemoveExistingProducts. Return value 3.')):
            with self.subTest(bad=bad), self.assertRaises(ValueError):
                runner.rollback_evidence(bad, code)

    def test_product_state_and_all_payload_bytes_are_both_required(self):
        package = self.package()
        engine = runner.Lifecycle({'work': str(self.root)}, [package])
        engine.initialize()
        for name in package.files:
            shutil.copy2(package.payload / name, package.target / name)
        with patch.object(runner, 'product_state', return_value=5), \
                patch.object(runner, 'fixture_install_folder', return_value=str(package.target)), \
                patch.object(runner, 'fixture_programs_root', return_value=str(package.target.parent)):
            engine.verify('unit-mock-only', 'renamed', package)
            (package.target / 'Harmony.exe').write_bytes(b'changed')
            with self.assertRaisesRegex(ValueError, 'payload bytes'):
                engine.verify('unit-mock-only-bad-bytes', 'renamed', package)
        shutil.copy2(package.payload / 'Harmony.exe', package.target / 'Harmony.exe')
        with patch.object(runner, 'product_state', return_value=-1):
            with self.assertRaisesRegex(ValueError, 'registration'):
                engine.verify('unit-mock-only-wrong-state', 'renamed', package)

    def test_unowned_canary_and_fake_user_data_are_invariants(self):
        package = self.package()
        engine = runner.Lifecycle({'work': str(self.root)}, [package])
        engine.initialize()
        for path in engine.canaries:
            original = path.read_bytes()
            path.write_bytes(b'changed')
            with self.subTest(path=path), self.assertRaisesRegex(ValueError, 'canary'):
                engine.invariants()
            path.write_bytes(original)

    def test_cleanup_continues_after_one_product_failure(self):
        a, b = self.package(), self.package('renamed-upgrade', version='1.1.2')
        engine = runner.Lifecycle({'work': str(self.root)}, [a, b])
        engine.initialize()
        with patch.object(runner, 'product_state', return_value=5), \
                patch.object(runner, 'related_products', return_value=[]), \
                patch.object(runner, 'fixture_registry_absent', return_value=True), \
                patch.object(engine, 'verify'), \
                patch.object(engine, 'operate', side_effect=RuntimeError('unit-only cleanup failure')) as operate:
            errors = engine.cleanup_all()
        self.assertEqual(operate.call_count, 2)
        self.assertEqual(len(errors), 2)
        self.assertEqual([row['product_code'] for row in engine.cleanup], [b.code, a.code])

    def test_all_native_command_modes_pin_both_program_directories_and_disable_ui_restart(self):
        package = self.package()
        engine = runner.Lifecycle({'work': str(self.root)}, [package])
        engine.initialize()
        commands = []

        def simulated(command, **kwargs):
            commands.append(command)
            Path(command[-1]).write_text('UNIT MOCK ONLY; NO NATIVE EXECUTION', encoding='utf-8')
            return subprocess.CompletedProcess(command, 0, '', '')

        with patch.object(runner, 'fixture_install_folder', return_value=None), \
                patch.object(runner, 'fixture_programs_root', return_value=None), \
                patch.object(runner.subprocess, 'run', side_effect=simulated):
            for mode in ('install', 'repair', 'uninstall'):
                engine.operate('unit-command-' + mode, package, mode)
        for command in commands:
            self.assertEqual(Path(command[0]).name.lower(), 'msiexec.exe')
            self.assertIn(runner.FIXTURE_PROGRAMS + '=' + str(package.target.parent), command)
            self.assertIn('/qn', command)
            self.assertIn('/norestart', command)
            self.assertIn('REBOOT=ReallySuppress', command)
            self.assertIn('MSIRESTARTMANAGERCONTROL=DisableShutdown', command)
            self.assertIn('MSIDISABLERMRESTART=1', command)
            self.assertEqual(Path(command[-1]).parent, self.root)
        self.assertIn('INSTALLFOLDER=' + str(package.target), commands[0])
        self.assertEqual(commands[1][1:3], ['/fa', str(package.path)])
        self.assertEqual(commands[2][1:3], ['/x', package.code])

    def test_legacy_refusal_phases_precede_independent_renamed_and_rollback(self):
        package = self.package()
        engine = runner.Lifecycle({'work': str(self.root), 'include_fault_rollback': True,
                                   'application_info': {}, 'application_executable_sha256': 'unit-only'}, [package])
        order = []
        with patch.object(engine, 'initialize'), patch.object(runner, 'builder_module', return_value=argparse.Namespace(LEGACY_MIGRATION_MESSAGE=MESSAGE)), \
                patch.object(engine, 'legacy', side_effect=lambda group, *_: order.append(group)), \
                patch.object(engine, 'renamed', side_effect=lambda: order.append('renamed')), \
                patch.object(engine, 'rollback', side_effect=lambda: order.append('rollback')), \
                patch.object(engine, 'cleanup_all', side_effect=lambda: order.append('cleanup') or []), \
                patch.object(runner, 'write_json'), patch('builtins.print'):
            self.assertEqual(engine.execute(), 0)
        self.assertEqual(order, ['legacy-version', 'legacy-marker', 'renamed', 'rollback', 'cleanup'])
        self.process.assert_not_called()

    def test_failure_cannot_skip_cleanup_or_claim_native_pass(self):
        package = self.package()
        engine = runner.Lifecycle({'work': str(self.root), 'include_fault_rollback': False,
                                   'application_info': {}, 'application_executable_sha256': 'unit-only'}, [package])
        with patch.object(engine, 'initialize', side_effect=RuntimeError('unit-only failure')), \
                patch.object(engine, 'cleanup_all', return_value=[]) as cleanup, \
                patch.object(runner, 'write_json') as writes, patch('builtins.print'):
            self.assertEqual(engine.execute(), 1)
        cleanup.assert_called_once()
        report = next(call.args[1] for call in writes.call_args_list if Path(call.args[0]).name == 'result.json')
        self.assertEqual(report['status'], 'FAIL_REAL_ISOLATED_MSI_LIFECYCLE')
        self.assertIn('unit-only failure', report['error'])


class PreparedPlanTests(ScratchTest):
    def setUp(self):
        super().setUp()
        self.root = self.root / ('msi-lifecycle-' + uuid.uuid4().hex)
        self.root.mkdir()
        source = self.root / 'verified-unit-source'
        source.mkdir()
        (source / 'Harmony.exe').write_bytes(b'SYNTHETIC UNIT SOURCE, NEVER EXECUTED')
        info = {'version': '1.1.0', 'build_id': 1, 'commit': 'd8e33cb2461b8efc903ff2907059d979cd9e153a', 'dirty': False}
        runner.write_json(source / 'harmony-build-info.json', info)
        files = runner.payload_hashes(source)
        receipt = self.root / 'unit-release.json'
        runner.write_json(receipt, {'payload_root': str(source), 'payload_files': files,
                                    'executable_sha256': files['Harmony.exe'], 'application_commit': info['commit'],
                                    'app_version': info['version'], 'app_build_id': info['build_id']})
        namespaces = {group: uuid.uuid4() for group in runner.GROUPS}
        self.packages = []
        for label, group, version, alias, fault in runner.SPECS:
            payload = self.root / 'payloads' / label
            shutil.copytree(source, payload)
            ns = namespaces[group]
            if alias:
                (payload / 'Harmony.exe').rename(payload / 'yapfast.exe')
            (payload / runner.FIXTURE_NOTE).write_bytes(runner.note_bytes(label, ns))
            if fault:
                (payload / runner.NEW_ONLY).write_bytes(b'New-only rollback canary; must not survive failed upgrade.\n')
            msi = self.root / 'packages' / (label + '.msi')
            msi.parent.mkdir(exist_ok=True)
            msi.write_bytes(b'NOT AN MSI: UNIT PLAN MODEL ' + label.encode())
            self.packages.append(runner.Package(label, group, ns, version, alias, fault, msi, payload,
                                                self.root / group / 'program', runner.payload_hashes(payload), runner.digest(msi)))
        self.plan = {'schema': 1, 'work': str(self.root), 'release_receipt': str(receipt),
                     'release_receipt_sha256': runner.digest(receipt), 'application_info': info,
                     'application_executable_sha256': files['Harmony.exe'], 'include_fault_rollback': True,
                     'packages': [package.record() for package in self.packages]}
        self.path = self.root / 'plan.json'
        runner.write_json(self.path, self.plan)
        self.tables = {package.path: fake_database(package) for package in self.packages}
        self.inspect = patch.object(runner, 'inspect', side_effect=lambda path: self.tables[path]).start()
        self.builder = patch.object(runner, 'builder_module', return_value=argparse.Namespace(LEGACY_MIGRATION_MESSAGE=MESSAGE)).start()
        self.state = patch.object(runner, 'product_state', return_value=-1).start()
        self.related = patch.object(runner, 'related_products', return_value=[]).start()
        self.registry = patch.object(runner, 'fixture_registry_absent', return_value=True).start()

    def test_all_eight_models_are_inspected_before_any_registration_queries(self):
        order = []
        self.inspect.side_effect = lambda path: order.append('inspect') or self.tables[path]
        self.related.side_effect = lambda _: order.append('related') or []
        self.state.side_effect = lambda _: order.append('state') or -1
        _, packages = runner.load_prepared(self.path)
        self.assertEqual(len(packages), 8)
        self.assertEqual(order[:8], ['inspect'] * 8)
        self.assertEqual(self.state.call_count, 8)
        self.process.assert_not_called()

    def test_last_package_unsafe_table_blocks_the_whole_run_before_native_queries(self):
        self.tables[self.packages[-1].path]['raw_tables']['RegLocator'][0][2] = r'Software\Harmony\Installer'
        with self.assertRaisesRegex(ValueError, 'Registry searches'):
            runner.load_prepared(self.path)
        self.assertEqual(self.inspect.call_count, 8)
        self.state.assert_not_called()
        self.related.assert_not_called()
        self.process.assert_not_called()

    def test_tampered_msi_bytes_are_rejected_before_native_inspection(self):
        self.packages[0].path.write_bytes(b'changed')
        with self.assertRaisesRegex(ValueError, 'bytes changed'):
            runner.load_prepared(self.path)
        self.inspect.assert_not_called()
        self.state.assert_not_called()

    def test_plan_target_cannot_be_changed_to_live_program(self):
        self.plan['packages'][0]['target'] = str(Path.home() / 'AppData/Local/Programs/Harmony')
        runner.write_json(self.path, self.plan)
        with self.assertRaisesRegex(ValueError, 'profile scratch'):
            runner.load_prepared(self.path)
        self.inspect.assert_not_called()
        self.process.assert_not_called()

    def test_single_use_plan_cannot_be_replayed(self):
        runner.write_json(self.root / 'execution-started.json', {'unit_test_only': True})
        with self.assertRaisesRegex(ValueError, 'single-use'):
            runner.load_prepared(self.path)
        self.inspect.assert_not_called()

    def test_registered_or_stale_fixture_namespace_is_not_reused(self):
        for tool, value in ((self.state, 5), (self.related, ['unit-existing-product']), (self.registry, False)):
            with self.subTest(tool=tool):
                tool.return_value = value
                with self.assertRaisesRegex(ValueError, 'already'):
                    runner.load_prepared(self.path)
                tool.return_value = -1 if tool is self.state else [] if tool is self.related else True
        self.process.assert_not_called()

    def test_remembered_folder_must_be_exactly_the_scratch_target(self):
        engine = runner.Lifecycle(self.plan, self.packages)
        package = self.packages[0]
        with patch.object(runner, 'fixture_install_folder', return_value=str(Path.home() / 'AppData/Local/Programs/Harmony')):
            with self.assertRaisesRegex(ValueError, 'profile scratch'):
                engine.operate('unit-blocked', package, 'uninstall')
        self.process.assert_not_called()


class StreamApi:
    """ctypes-shaped fake for handle/bound tests; never loads msi.dll."""
    def __init__(self, content=b'abc\x00def', rows=1, fields=1, fail_read=False):
        self.content, self.rows, self.fields = content, rows, fields
        self.fail_read = fail_read
        self.offset, self.fetches = 0, 0
        self.closed = []

    def MsiDatabaseOpenViewW(self, handle, sql, output):
        output._obj.value = 10
        return 0

    def MsiViewExecute(self, *_):
        return 0

    def MsiViewFetch(self, view, output):
        self.fetches += 1
        if self.fetches > self.rows:
            return 259
        output._obj.value = 20 + self.fetches
        return 0

    def MsiRecordGetFieldCount(self, *_):
        return self.fields

    def MsiRecordReadStream(self, record, index, output, length):
        if self.fail_read:
            return 13
        size = length._obj.value
        chunk = self.content[self.offset:self.offset + size]
        c.memmove(output, chunk, len(chunk))
        self.offset += len(chunk)
        length._obj.value = len(chunk)
        return 0

    def MsiCloseHandle(self, handle):
        self.closed.append(handle.value)
        return 0


class BinaryStreamTests(unittest.TestCase):
    def instance(self, **kwargs) -> Any:
        db: Any = database.Database.__new__(database.Database)
        db.api = StreamApi(**kwargs)
        db.handle = c.c_uint(1)
        return db

    def test_binary_stream_preserves_bytes_and_closes_record_and_view(self):
        data = bytes(range(256)) * 513
        db = self.instance(content=data)
        self.assertEqual(db.read_stream('SELECT `Data` FROM `Icon`', max_bytes=len(data)), data)
        self.assertEqual(db.api.closed, [21, 10])

    def test_binary_stream_limit_is_enforced_without_unbounded_allocation(self):
        db = self.instance(content=b'abcd')
        with self.assertRaisesRegex(ValueError, 'bound'):
            db.read_stream('SELECT `Data` FROM `Icon`', max_bytes=3)
        self.assertEqual(db.api.closed, [21, 10])
        self.assertLessEqual(db.api.offset, 4)

    def test_empty_stream_can_use_zero_byte_limit(self):
        db = self.instance(content=b'')
        self.assertEqual(db.read_stream('SELECT `Data` FROM `Icon`', max_bytes=0), b'')

    def test_binary_select_requires_exactly_one_record_and_field(self):
        for kwargs in ({'rows': 0}, {'rows': 2}, {'fields': 2}):
            db = self.instance(**kwargs)
            with self.subTest(kwargs=kwargs), self.assertRaises(ValueError):
                db.read_stream('SELECT `Data` FROM `Icon`')
            self.assertIn(10, db.api.closed)
            if kwargs.get('rows') != 0:
                self.assertIn(21, db.api.closed)
            if kwargs.get('rows') == 2:
                self.assertIn(22, db.api.closed)

    def test_read_error_closes_handles(self):
        db = self.instance(fail_read=True)
        with self.assertRaises(OSError):
            db.read_stream('SELECT `Data` FROM `Icon`')
        self.assertEqual(db.api.closed, [21, 10])

    def test_queries_are_read_only_and_limits_are_validated_before_api(self):
        for sql in ('UPDATE `Icon` SET `Data`=1', 'DELETE FROM `Icon`', 'SELECT `Data` FROM `Icon`; DELETE FROM `Icon`'):
            with self.subTest(sql=sql), self.assertRaises(ValueError):
                self.instance().read_stream(sql)
        for limit in (-1, True, 512 * 1024 * 1024 + 1):
            with self.subTest(limit=limit), self.assertRaises(ValueError):
                self.instance().read_stream('SELECT `Data` FROM `Icon`', max_bytes=limit)


if __name__ == '__main__':
    unittest.main()
