"""MSI recipe unit contracts, separate from real Windows Installer lifecycle evidence."""
import importlib.util
from pathlib import Path
import unittest

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
            (payload/'yapfast.exe').write_bytes(b'fixture, never executed')
            (payload/'licenses').mkdir()
            (payload/'licenses/LICENSE.txt').write_text('fixture notice')
            xml=ET.fromstring(module.generate_wxs(payload,{'version':'1.0.0','build_id':1}))
        ns={'w':'http://wixtoolset.org/schemas/v4/wxs'}
        package=xml.find('w:Package',ns)
        assert package is not None
        self.assertEqual(package.get('Scope'),'perUser')
        self.assertEqual(package.get('Version'),'1.0.1')
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
        self.assertEqual(names,{'yapfast.exe','LICENSE.txt'})
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
            (payload/'yapfast.exe').write_bytes(b'fixture, never executed')
            prod=ET.fromstring(module.generate_wxs(payload,{'version':'1.0.0','build_id':1}))
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
            info={'version':'1.0.0','build_id':1,'commit':'a'*40,'dirty':False,'platform':'windows','architecture':'x86_64','license':'GPL-3.0-or-later'}
            exe_hash=hashlib.sha256(raw).hexdigest()
            build=dict(info,executable_sha256=exe_hash,source_archive_sha256=sha(source))
            bundle=root/'Fixture-windows-x64.zip'
            def write_bundle(extra=None):
                files={'yapfast.exe':raw,'BUILD.json':json.dumps(build),'harmony-build-info.json':json.dumps(info),'LICENSE.txt':(ROOT/'LICENSE').read_bytes(),'THIRD-PARTY.json':json.dumps([{'name':'fixture-only','license_files':['licenses/fixture.txt']}]),'licenses/fixture.txt':'fixture notice'}
                if extra:files.update(extra)
                with zipfile.ZipFile(bundle,'w') as archive:
                    for name,data in files.items():archive.writestr('Fixture/'+name,data)
            write_bundle()
            manifest={'commit':info['commit'],'version':'1.0.0','build_id':1,'executable':{'sha256':exe_hash},'artifacts':[{'path':str(bundle),'sha256':sha(bundle)},{'path':str(source),'sha256':sha(source)}]}
            path=root/'manifest.json';path.write_text(json.dumps(manifest))
            staged_info,files=module.stage_release(path,root/'positive')
            self.assertEqual(staged_info,info)
            self.assertEqual(files['yapfast.exe'],exe_hash)
            self.assertEqual(sha(root/'positive/corresponding-source'/source.name),sha(source))
            bundle.write_bytes(bundle.read_bytes()+b'changed after manifest')
            with self.assertRaises(ValueError):module.stage_release(path,root/'bad-hash')
            self.assertFalse((root/'bad-hash').exists())
            write_bundle({'../escaped.txt':'must never escape'})
            manifest['artifacts'][0]['sha256']=sha(bundle);path.write_text(json.dumps(manifest))
            with self.assertRaises(ValueError):module.stage_release(path,root/'bad-path')
            self.assertFalse((root/'escaped.txt').exists())
            self.assertFalse((root/'bad-path').exists())

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
            (payload/'licenses/nested/deep/LICENSE.txt').write_text('fixture notice')
            xml=ET.fromstring(module.generate_wxs(payload,{'version':'1.0.0','build_id':1}))
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

if __name__=='__main__':unittest.main()
