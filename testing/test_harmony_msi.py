"""MSI recipe unit contracts, separate from real Windows Installer lifecycle evidence."""
import importlib.util
from pathlib import Path
import tempfile
import unittest
import uuid
import xml.etree.ElementTree as ET

ROOT=Path(__file__).resolve().parents[1]

class HarmonyMsiTests(unittest.TestCase):
    def module(self):
        path=ROOT/'scripts/package_harmony_msi.py'
        if not path.is_file():
            self.fail('Harmony MSI packaging/version encoding is not implemented')
        spec=importlib.util.spec_from_file_location('harmony_msi',path)
        assert spec is not None and spec.loader is not None
        module=importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        return module

    def test_build_is_encoded_in_a_significant_msi_version_field(self):
        self.assertEqual(self.module().msi_version('1.0.0',1),'1.0.1')

    def test_add_remove_programs_uses_the_original_harmony_icon(self):
        module=self.module()
        ns={'w':module.WIX_NS}
        with tempfile.TemporaryDirectory() as folder:
            payload=Path(folder)
            (payload/'Harmony.exe').write_bytes(b'fixture, never executed')
            xml=ET.fromstring(module.generate_wxs(payload,{'version':'1.1.0','build_id':1}))
        icon=xml.find('.//w:Icon',ns)
        assert icon is not None
        self.assertEqual(icon.get('Id'),'Harmony.ico')
        self.assertEqual(Path(icon.attrib['SourceFile']),ROOT/'packaging/windows/harmony.ico')
        self.assertIsNotNone(xml.find('.//w:Property[@Id="ARPPRODUCTICON"][@Value="Harmony.ico"]',ns))

    def test_fault_injection_is_isolated_and_never_authored_for_production(self):
        module=self.module()
        ns={'w':module.WIX_NS}
        with tempfile.TemporaryDirectory() as folder:
            payload=Path(folder)
            (payload/'Harmony.exe').write_bytes(b'fixture, never executed')
            info={'version':'1.1.0','build_id':1}
            with self.assertRaisesRegex(ValueError,'isolated lifecycle fixtures'):
                module.generate_wxs(payload,info,test_fail_after_install_files=True)
            xml=ET.fromstring(module.generate_wxs(payload,info,test_namespace=uuid.uuid4(),
                                                 test_version='1.1.3',test_fail_after_install_files=True))
            ordinary=ET.fromstring(module.generate_wxs(payload,info))
        self.assertEqual(ordinary.findall('.//w:CustomAction',ns),[])
        action=xml.find('.//w:CustomAction',ns)
        assert action is not None
        self.assertEqual(action.attrib,{'Id':'FixtureAbortAfterInstallFiles',
                                       'Error':'Harmony isolated MSI rollback fixture failure'})
        sequenced=xml.find('.//w:InstallExecuteSequence/w:Custom',ns)
        assert sequenced is not None
        self.assertEqual(sequenced.attrib,{'Action':'FixtureAbortAfterInstallFiles',
                                          'After':'InstallExecute','Condition':'NOT REMOVE'})

    def test_fixture_ancestor_is_secure_and_remembered_for_nested_uninstall(self):
        module=self.module()
        ns={'w':module.WIX_NS}
        namespace=uuid.uuid4()
        with tempfile.TemporaryDirectory() as folder:
            payload=Path(folder)
            (payload/'Harmony.exe').write_bytes(b'fixture, never executed')
            fixture=ET.fromstring(module.generate_wxs(payload,{'version':'1.1.0','build_id':1},test_namespace=namespace))
            production=ET.fromstring(module.generate_wxs(payload,{'version':'1.1.0','build_id':1}))
        ancestor=fixture.find('.//w:Property[@Id="HARMONYFIXTUREPROGRAMS"]',ns)
        assert ancestor is not None
        self.assertEqual(ancestor.get('Secure'),'yes')
        search=ancestor.find('w:RegistrySearch',ns)
        assert search is not None
        self.assertEqual(search.get('Key'),rf'Software\Harmony\InstallerTests\{namespace}')
        self.assertEqual(search.get('Name'),'FixtureProgramsRoot')
        self.assertEqual(search.get('Bitness'),'always64')
        remembered=fixture.find('.//w:RegistryValue[@Name="FixtureProgramsRoot"]',ns)
        assert remembered is not None
        self.assertEqual(remembered.get('Value'),'[HARMONYFIXTUREPROGRAMS]')
        self.assertIsNotNone(fixture.find('.//w:RemoveFolder[@Directory="HARMONYFIXTUREPROGRAMS"]',ns))
        self.assertIsNone(fixture.find('.//w:Directory[@Id="HarmonyPrograms"]',ns))
        self.assertIsNotNone(production.find('.//w:Directory[@Id="HarmonyPrograms"]',ns))
        self.assertIsNone(production.find('.//w:Property[@Id="HARMONYFIXTUREPROGRAMS"]',ns))

    def test_msi_version_encoding_rejects_ambiguous_or_unrepresentable_identity(self):
        module=self.module()
        for version,build in [('01.0.0',1),('1.0.0.1',1),('1.0.0-beta',1),('1.0.0',True),('1.0.0',0),('1.0.0',1000),('256.0.0',1),('1.256.0',1),('1.0.65',536)]:
            with self.subTest(version=version,build=build),self.assertRaises(ValueError):
                module.msi_version(version,build)
        self.assertEqual(module.msi_version('1.0.0',2),'1.0.2')
        self.assertEqual(module.msi_version('1.0.1',1),'1.0.1001')
        self.assertEqual(module.msi_version('255.255.65',535),'255.255.65535')

    def test_recipe_updates_only_per_user_program_files_and_keeps_existing_launchers(self):
        import tempfile
        import xml.etree.ElementTree as ET
        module=self.module()
        self.assertTrue(callable(getattr(module,'generate_wxs',None)),'The per-user MSI recipe is not implemented')
        with tempfile.TemporaryDirectory() as folder:
            payload=Path(folder)
            (payload/'Harmony.exe').write_bytes(b'fixture, never executed')
            (payload/'licenses').mkdir()
            (payload/'licenses/LICENSE.txt').write_text('fixture notice')
            xml=ET.fromstring(module.generate_wxs(payload,{'version':'1.1.0','build_id':1}))
        ns={'w':'http://wixtoolset.org/schemas/v4/wxs'}
        package=xml.find('w:Package',ns)
        assert package is not None
        self.assertEqual(package.get('Scope'),'perUser')
        self.assertEqual(package.get('Version'),'1.1.1')
        upgrade=package.find('w:MajorUpgrade',ns)
        assert upgrade is not None
        self.assertEqual(upgrade.get('Schedule'),'afterInstallInitialize')
        self.assertNotEqual(upgrade.get('AllowDowngrades'),'yes')
        props={node.get('Id'):node.get('Value') for node in package.findall('w:Property',ns)}
        self.assertEqual(props['MSIRESTARTMANAGERCONTROL'],'DisableShutdown')
        self.assertEqual(props.get('MSIDISABLERMRESTART'),'1')
        self.assertEqual(props['REBOOT'],'ReallySuppress')
        self.assertEqual(props['REINSTALLMODE'],'amus')
        self.assertIsNotNone(package.find('.//w:StandardDirectory[@Id="LocalAppDataFolder"]/w:Directory[@Name="Programs"]/w:Directory[@Id="INSTALLFOLDER"]',ns))
        self.assertEqual(package.findall('.//w:Shortcut',ns),[])
        self.assertEqual(package.findall('.//w:RemoveFile',ns),[])
        self.assertEqual(package.findall('.//w:RemoveFolderEx',ns),[])
        self.assertEqual(package.findall('.//w:ServiceControl',ns),[])
        # WiX v4 Launch elements are launch *conditions*, not app launch actions.
        self.assertTrue(all(node.get('ExeCommand') is None for node in package.findall('.//w:CustomAction',ns)))
        self.assertTrue(all(node.get('Root')=='HKCU' for node in package.findall('.//w:RegistryValue',ns)))
        components=package.findall('.//w:Component',ns)
        self.assertTrue(components)
        self.assertTrue(all(node.find('w:RegistryValue[@KeyPath="yes"]',ns) is not None for node in components))
        names={node.get('Name') for node in package.findall('.//w:File',ns)}
        self.assertEqual(names,{'Harmony.exe','LICENSE.txt'})
        self.assertNotIn('Harmony/data',ET.tostring(xml,encoding='unicode'))

    def test_lifecycle_fixture_uses_disjoint_product_component_and_registry_identity(self):
        import inspect
        import tempfile
        import uuid
        import xml.etree.ElementTree as ET
        module=self.module()
        self.assertIn('test_namespace',inspect.signature(module.generate_wxs).parameters,'Real MSI lifecycle tests need isolated identities first')
        ns={'w':module.WIX_NS}
        with tempfile.TemporaryDirectory() as folder:
            payload=Path(folder)
            (payload/'Harmony.exe').write_bytes(b'fixture, never executed')
            prod=ET.fromstring(module.generate_wxs(payload,{'version':'1.1.0','build_id':1}))
            fixture=uuid.uuid4()
            old=ET.fromstring(module.generate_wxs(payload,{'version':'1.0.0','build_id':1},test_namespace=fixture,test_version='1.0.1'))
            new=ET.fromstring(module.generate_wxs(payload,{'version':'1.0.0','build_id':1},test_namespace=fixture,test_version='1.0.2'))
        def package(xml):
            value=xml.find('w:Package',ns)
            assert value is not None
            return value
        self.assertNotEqual(package(prod).get('UpgradeCode'),package(old).get('UpgradeCode'))
        self.assertEqual(package(old).get('UpgradeCode'),package(new).get('UpgradeCode'))
        self.assertNotEqual(package(old).get('ProductCode'),package(new).get('ProductCode'))
        prod_components={x.get('Guid') for x in prod.findall('.//w:Component',ns)}
        old_components={x.get('Guid') for x in old.findall('.//w:Component',ns)}
        self.assertFalse(prod_components & old_components)
        self.assertEqual(old_components,{x.get('Guid') for x in new.findall('.//w:Component',ns)})
        self.assertTrue(all('InstallerTests' in x.get('Key','') for x in old.findall('.//w:RegistryValue',ns)))
        self.assertIn('Fixture',package(old).get('Name',''))
        self.assertIsNotNone(package(old).find('w:Property[@Id="ARPSYSTEMCOMPONENT"]',ns))
        self.assertIsNone(package(prod).find('w:Property[@Id="ARPSYSTEMCOMPONENT"]',ns))

    def test_staging_requires_a_matching_bundle_executable_stamp_and_source(self):
        import hashlib,json,struct,tempfile,zipfile
        module=self.module()
        self.assertTrue(callable(getattr(module,'stage_release',None)),'Verified release staging is not implemented')
        with tempfile.TemporaryDirectory() as folder:
            root=Path(folder)
            source=root/'Fixture-source.zip'
            with zipfile.ZipFile(source,'w') as archive:archive.writestr('Fixture-source/Cargo.toml','fixture source, never built')
            sha=lambda path:hashlib.sha256(path.read_bytes()).hexdigest()
            raw=bytearray(80);raw[:2]=b'MZ';struct.pack_into('<I',raw,60,64);raw[64:70]=b'PE\0\0\x64\x86'
            info={'version':'1.1.0','build_id':1,'commit':'a'*40,'dirty':False,'platform':'windows','architecture':'x86_64','upstream':'Yapfast 0.4.1','license':'GPL-3.0-or-later'}
            exe_hash=hashlib.sha256(raw).hexdigest()
            build=dict(info,executable_sha256=exe_hash,source_archive_sha256=sha(source),updater_included=False)
            bundle=root/'Fixture-windows-x64.zip'
            def write_bundle(extra=None):
                files={'Harmony.exe':raw,'BUILD.json':json.dumps(build),'harmony-build-info.json':json.dumps(info),'LICENSE.txt':(ROOT/'LICENSE').read_bytes(),'SOURCE.txt':'fixture source accompanying this package','THIRD-PARTY.json':json.dumps([{'name':'fixture-only','license_files':['licenses/fixture.txt']}]),'licenses/fixture.txt':'fixture notice'}
                if extra:files.update(extra)
                with zipfile.ZipFile(bundle,'w') as archive:
                    for name,data in files.items():
                        entry=zipfile.ZipInfo('placeholder')
                        entry.filename='Fixture/'+name
                        entry.orig_filename=entry.filename
                        archive.writestr(entry,data)
            write_bundle()
            manifest={'commit':info['commit'],'version':info['version'],'build_id':1,'executable':{'sha256':exe_hash},'artifacts':[{'path':str(bundle),'sha256':sha(bundle)},{'path':str(source),'sha256':sha(source)}]}
            path=root/'manifest.json';path.write_text(json.dumps(manifest))
            staged_info,files=module.stage_release(path,root/'positive')
            self.assertEqual(staged_info,info)
            self.assertEqual(files['Harmony.exe'],exe_hash)
            self.assertEqual(sha(root/'positive/corresponding-source'/source.name),sha(source))
            instructions=(root/'positive/MSI-INSTALL.txt').read_text(encoding='utf-8')
            for required in ('before removing any old product', 'wizard and silent mode',
                             'separate, unowned folder', '--data-dir', 'taskbar pins',
                             'Do not claim new handlers or enable startup', 'packaging/windows-msi.md'):
                self.assertIn(required, instructions)
            bundle.write_bytes(bundle.read_bytes()+b'changed after manifest')
            with self.assertRaises(ValueError):module.stage_release(path,root/'bad-hash')
            self.assertFalse((root/'bad-hash').exists())
            write_bundle({'../escaped.txt':'must never escape'})
            manifest['artifacts'][0]['sha256']=sha(bundle);path.write_text(json.dumps(manifest))
            with self.assertRaises(ValueError):module.stage_release(path,root/'bad-path')
            self.assertFalse((root/'escaped.txt').exists())
            self.assertFalse((root/'bad-path').exists())
            for index,name in enumerate(('nested\\file.txt','cut\0suffix.txt','a/./file.txt','a//file.txt')):
                with self.subTest(raw_archive_name=repr(name)):
                    write_bundle({name:'must be rejected without normalization'})
                    manifest['artifacts'][0]['sha256']=sha(bundle);path.write_text(json.dumps(manifest))
                    bad=root/f'bad-raw-name-{index}'
                    with self.assertRaisesRegex(ValueError,'noncanonical archive member'):
                        module.stage_release(path,bad)
                    self.assertFalse(bad.exists())
            for index, extra in enumerate(({'yapfast.exe':raw},{'harmony.exe':raw},{'BUILD.json':json.dumps(dict(build,updater_included=True))},{'BUILD.json':json.dumps(dict(build,version='9.9.9'))})):
                with self.subTest(extra=list(extra)):
                    write_bundle(extra)
                    manifest['artifacts'][0]['sha256']=sha(bundle);path.write_text(json.dumps(manifest))
                    bad=root/f'bad-identity-{index}'
                    with self.assertRaises(ValueError):module.stage_release(path,bad)
                    self.assertFalse(bad.exists())
            # Even a self-consistent archive claiming the updater is disabled
            # must use the 1.2+ installer path for an app at that version.
            info['version'] = build['version'] = manifest['version'] = '1.2.0'
            write_bundle()
            manifest['artifacts'][0]['sha256'] = sha(bundle)
            path.write_text(json.dumps(manifest))
            with self.assertRaisesRegex(ValueError, 'Inno Setup'):
                module.stage_release(path, root / 'modern-msi')
            self.assertFalse((root / 'modern-msi').exists())

    def test_command_line_exposes_verified_input_and_explicit_tool_paths(self):
        import os,subprocess,sys
        result=subprocess.run([sys.executable,str(ROOT/'scripts/package_harmony_msi.py'),'--help'],capture_output=True,text=True,timeout=20,creationflags=subprocess.BELOW_NORMAL_PRIORITY_CLASS if os.name=='nt' else 0)
        self.assertEqual(result.returncode,0)
        for argument in ('--manifest','--wix-dll','--ui-extension','--output'):
            self.assertIn(argument,result.stdout)

    def test_all_profile_program_directory_ancestors_have_uninstall_only_empty_cleanup(self):
        import tempfile
        import xml.etree.ElementTree as ET
        module=self.module()
        with tempfile.TemporaryDirectory() as folder:
            payload=Path(folder)
            (payload/'licenses/nested/deep').mkdir(parents=True)
            (payload/'Harmony.exe').write_bytes(b'fixture, never executed')
            (payload/'licenses/nested/deep/LICENSE.txt').write_text('fixture notice')
            xml=ET.fromstring(module.generate_wxs(payload,{'version':'1.1.0','build_id':1}))
        ns={'w':module.WIX_NS}
        directories=xml.findall('.//w:Directory',ns)
        by_id={node.get('Id'):node for node in directories}
        parents={child:parent for parent in xml.iter() for child in parent}
        cleaned=set()
        for node in xml.findall('.//w:RemoveFolder',ns):
            self.assertEqual(node.get('On'),'uninstall')
            if node.get('Directory'):cleaned.add(node.get('Directory'))
            else:cleaned.add(parents[parents[node]].get('Id'))
        self.assertEqual(set(by_id)-cleaned,set(),'ICE64: profile ancestor cleanup missing; use empty-directory removal only, never recursive deletion')

    def test_legacy_guard_uses_only_the_stable_owned_upgrade_family_and_exact_component_marker(self):
        module=self.module()
        ns={'w':module.WIX_NS}
        with tempfile.TemporaryDirectory() as folder:
            payload=Path(folder)
            (payload/'Harmony.exe').write_bytes(b'fixture, never executed')
            xml=ET.fromstring(module.generate_wxs(payload,{'version':'1.1.0','build_id':1}))
        package=xml.find('w:Package',ns)
        self.assertIsNotNone(package)
        assert package is not None
        # Pin the actual pre-rebrand identity, not just a new derivation of it.
        self.assertEqual(package.get('UpgradeCode'),'B85CFFDA-896F-5BBC-8863-BC5F4DD0E810')
        upgrade=package.find('w:Upgrade',ns)
        assert upgrade is not None
        self.assertEqual(upgrade.get('Id'),package.get('UpgradeCode'))
        detect=upgrade.find('w:UpgradeVersion',ns)
        assert detect is not None
        self.assertEqual(detect.attrib,{'Maximum':'1.1.1','IncludeMaximum':'no',
                                      'OnlyDetect':'yes','Property':'HARMONY_LEGACY_MSI'})
        marker=package.find('w:Property[@Id="HARMONY_LEGACY_COMPONENT"]',ns)
        assert marker is not None
        self.assertEqual(marker.get('Secure'),'yes')
        search=marker.find('w:RegistrySearch',ns)
        assert search is not None
        self.assertEqual(search.attrib,{'Id':'LegacyExecutableComponent','Root':'HKCU',
                                      'Key':r'Software\Harmony\Installer\Components',
                                      'Name':'C_bb51d6588f1a73f021a9113e','Type':'raw','Bitness':'always64'})
        self.assertIsNone(marker.get('Value'),'Fresh installs must not default to legacy-found')
        launches=package.findall('w:Launch',ns)
        guard=[node for node in launches if 'HARMONY_LEGACY_' in node.get('Condition','')]
        self.assertEqual(len(guard),1)
        # Either detection is sufficient; deleting the old exe must not bypass the guard.
        self.assertEqual(guard[0].get('Condition'),
                         'Installed OR NOT (HARMONY_LEGACY_MSI OR HARMONY_LEGACY_COMPONENT)')
        for required in ('before removing the old product','portable Harmony.exe',
                         'separate folder','preserving arguments and data folders','packaging/windows-msi.md'):
            self.assertIn(required,guard[0].get('Message',''))
        self.assertEqual(package.findall('.//w:CustomAction',ns),[])
        self.assertEqual(package.findall('.//w:FileSearch',ns),[])
        self.assertTrue(all(node.get('Key','').startswith(r'Software\Harmony\Installer')
                            for node in package.findall('.//w:RegistryValue',ns)))

    def test_legacy_detection_and_launch_conditions_precede_old_product_removal_in_both_modes(self):
        module=self.module()
        ns={'w':module.WIX_NS}
        with tempfile.TemporaryDirectory() as folder:
            payload=Path(folder)
            (payload/'Harmony.exe').write_bytes(b'fixture, never executed')
            xml=ET.fromstring(module.generate_wxs(payload,{'version':'1.1.0','build_id':1}))
        package=xml.find('w:Package',ns)
        assert package is not None
        for table in ('InstallUISequence','InstallExecuteSequence'):
            with self.subTest(table=table):
                sequence=package.find('w:'+table,ns)
                assert sequence is not None
                positions={node.tag.rsplit('}',1)[-1]:int(node.attrib['Sequence']) for node in sequence}
                self.assertLess(positions['FindRelatedProducts'],positions['LaunchConditions'])
                self.assertLess(positions['AppSearch'],positions['LaunchConditions'])
                for node in sequence:
                    self.assertFalse(node.get('Condition'), 'Never skip detection/guard in silent mode')
                if table=='InstallExecuteSequence':
                    self.assertLess(positions['LaunchConditions'],positions['InstallInitialize'])
        upgrade=package.find('w:MajorUpgrade',ns)
        assert upgrade is not None
        self.assertEqual(upgrade.get('Schedule'),'afterInstallInitialize')

    def test_fresh_and_future_harmony_payloads_remain_valid_without_claiming_any_launchers(self):
        module=self.module()
        ns={'w':module.WIX_NS}
        with tempfile.TemporaryDirectory() as folder:
            payload=Path(folder)
            (payload/'Harmony.exe').write_bytes(b'fixture, never executed')
            for app_version,build in (('1.1.0',1),('1.1.0',2),('1.1.1',1),('2.0.0',1)):
                with self.subTest(app_version=app_version,build=build):
                    xml=ET.fromstring(module.generate_wxs(payload,{'version':app_version,'build_id':build}))
                    package=xml.find('w:Package',ns)
                    assert package is not None
                    version=module.msi_version(app_version,build)
                    self.assertEqual(package.get('Version'),version)
                    self.assertEqual(package.get('ProductCode'),str(uuid.uuid5(module.UPGRADE_CODE,'product:'+version)).upper())
                    for tag in ('Shortcut','RemoveFile','RemoveFolderEx','CustomAction','ServiceControl'):
                        self.assertEqual(package.findall('.//w:'+tag,ns),[])
                    self.assertEqual({node.get('Name') for node in package.findall('.//w:File',ns)},{'Harmony.exe'})
                    self.assertFalse(any(node.get('Id')=='HARMONY_LEGACY_MSI' and node.get('Value')
                                         for node in package.findall('w:Property',ns)))

    def test_rebranded_production_never_reuses_legacy_product_codes(self):
        module=self.module()
        with tempfile.TemporaryDirectory() as folder:
            payload=Path(folder)
            (payload/'Harmony.exe').write_bytes(b'fixture, never executed')
            for version,build in (('1.0.0',1),('1.0.0',999),('1.0.65',535)):
                with self.subTest(version=version,build=build),self.assertRaisesRegex(ValueError,'never reuse a legacy ProductCode'):
                    module.generate_wxs(payload,{'version':version,'build_id':build})

    def test_manual_migration_guide_requires_verified_owned_entries_before_explicit_uninstall(self):
        guide=(ROOT/'packaging/windows-msi.md').read_text(encoding='utf-8')
        section=guide.split('## Manual legacy MSI migration\n',1)[1].split('\n## ',1)[0]
        for required in ('separate, unowned program folder', '--data-dir', 'working directory',
                         'Harmony/Harmony Preview', 'taskbar pin', 'Discord link handler',
                         'exact old executable path', 'enabled/disabled state',
                         'Do not register a new protocol', 'Read back every changed target/command',
                         'Do not uninstall the old MSI until', 'do not delete registry keys to bypass'):
            self.assertIn(required,section)
        self.assertLess(section.index('Read back every changed target/command'),
                        section.index('explicitly uninstall the **old Harmony MSI**'))

    def test_fixture_legacy_marker_matches_guard_without_reading_production_identity(self):
        module=self.module()
        ns={'w':module.WIX_NS}
        namespace=uuid.uuid4()
        with tempfile.TemporaryDirectory() as folder:
            payload=Path(folder)
            legacy=payload/'yapfast.exe'
            legacy.write_bytes(b'legacy fixture, never executed')
            old=ET.fromstring(module.generate_wxs(payload,{'version':'1.0.0','build_id':1},
                                                test_namespace=namespace,test_version='1.0.1'))
            legacy.unlink()
            (payload/'Harmony.exe').write_bytes(b'new fixture, never executed')
            new=ET.fromstring(module.generate_wxs(payload,{'version':'1.1.0','build_id':1},
                                                test_namespace=namespace))
        marker=new.find('.//w:RegistrySearch[@Id="LegacyExecutableComponent"]',ns)
        assert marker is not None
        registration=old.find('.//w:RegistryValue[@Name="'+marker.attrib['Name']+'"]',ns)
        self.assertIsNotNone(registration)
        assert registration is not None
        self.assertEqual(registration.get('Key'),marker.get('Key'))
        self.assertIn('InstallerTests',marker.attrib['Key'])
        upgrade=new.find('.//w:Upgrade',ns)
        assert upgrade is not None
        self.assertEqual(upgrade.get('Id'),str(namespace).upper())
        self.assertNotEqual(upgrade.get('Id'),str(module.UPGRADE_CODE).upper())

if __name__=='__main__':unittest.main()
