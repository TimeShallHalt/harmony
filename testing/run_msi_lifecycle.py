"""Opt-in real MSI lifecycle, isolated GUIDs/registry/program root; never install the production MSI."""
from pathlib import Path
import argparse
import ctypes as c
from ctypes import wintypes as w
import hashlib
import importlib.util
import json
import os
import shutil
import subprocess
import time
import uuid
from msi_database import inspect

ROOT=Path(__file__).resolve().parents[1]
FLAGS=subprocess.BELOW_NORMAL_PRIORITY_CLASS|subprocess.CREATE_NO_WINDOW

def digest(path):
    with path.open('rb') as stream:return hashlib.file_digest(stream,'sha256').hexdigest()

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--release-receipt',type=Path,required=True)
    parser.add_argument('--wix-dll',type=Path,required=True)
    parser.add_argument('--ui-extension',type=Path,required=True)
    parser.add_argument('--scratch-root',type=Path,required=True)
    parser.add_argument('--allow-isolated-msi-registration',action='store_true',required=True)
    args=parser.parse_args()
    scratch=args.scratch_root.resolve()
    expected=Path.home()/'AppData/Local/hermes/cache/scratch'
    if not scratch.is_relative_to(expected.resolve()):raise ValueError('Lifecycle fixtures must remain under the Hermes scratch directory')
    source=json.loads(args.release_receipt.read_text())
    payload=Path(source['payload_root']);files=source['payload_files']
    for name,value in files.items():assert digest(payload/name)==value
    spec=importlib.util.spec_from_file_location('harmony_msi',ROOT/'scripts/package_harmony_msi.py')
    assert spec and spec.loader
    mod=importlib.util.module_from_spec(spec);spec.loader.exec_module(mod)
    info=json.loads((payload/'harmony-build-info.json').read_text())
    live=Path.home()/'AppData/Local/Programs/Harmony/yapfast.exe'
    before=digest(live)
    assert before==source['executable_sha256']
    # Acquire only SYNCHRONIZE handles to existing exact-path app processes.
    ps='Get-CimInstance Win32_Process | Where-Object { $_.Name -eq "yapfast.exe" -and $_.ExecutablePath -eq ([IO.Path]::GetFullPath("'+str(live).replace('\\','/')+'")) } | Select-Object -ExpandProperty ProcessId | ConvertTo-Json -Compress'
    result=subprocess.run(['powershell.exe','-NoProfile','-NonInteractive','-Command',ps],capture_output=True,text=True,timeout=20,creationflags=FLAGS);assert result.returncode==0
    pids=json.loads(result.stdout) if result.stdout.strip() else []
    if isinstance(pids,int):pids=[pids]
    kernel=c.WinDLL('kernel32',use_last_error=True)
    kernel.OpenProcess.argtypes=[w.DWORD,w.BOOL,w.DWORD];kernel.OpenProcess.restype=w.HANDLE
    kernel.WaitForSingleObject.argtypes=[w.HANDLE,w.DWORD];kernel.WaitForSingleObject.restype=w.DWORD
    kernel.CloseHandle.argtypes=[w.HANDLE]
    handles=[]
    for pid in pids:
        handle=kernel.OpenProcess(0x100000,False,pid)
        if not handle:raise c.WinError(c.get_last_error())
        handles.append(handle)
    api=c.WinDLL('msi',use_last_error=True)
    api.MsiQueryProductStateW.argtypes=[w.LPCWSTR];api.MsiQueryProductStateW.restype=c.c_int
    namespace=uuid.uuid4();work=scratch/('lifecycle-'+namespace.hex);work.mkdir(parents=True)
    target=work/'program';target.mkdir()
    data=work/'user-data';data.mkdir()
    canary=data/'settings.json';canary.write_text('{"fixture":"must survive install/upgrade/repair/uninstall"}')
    canary_sha=digest(canary)
    extra=target/'user-created-fixture.txt';extra.write_text('not owned by Windows Installer')
    extra_sha=digest(extra)
    # Real previously installed production binary, not a synthetic update result.
    old=live.with_name('yapfast.rollback-pre-update1-3f6c512cf860.exe')
    assert digest(old)=='3f6c512cf86088d452f3d3f11a4dac49a11c99254ccf9429ac0db652d0adf96e'
    shutil.copy2(old,target/'yapfast.exe')
    steps=[];packages=[]
    env=dict(os.environ,TMPDIR=str(work),TMP=str(work),TEMP=str(work))
    def invariants():
        assert digest(live)==before,'Live application bytes changed'
        assert all(kernel.WaitForSingleObject(handle,0)==258 for handle in handles),'A baseline live process exited during fixture testing'
        assert digest(canary)==canary_sha and digest(extra)==extra_sha,'Unowned fixture/user-data file changed'
    def run(label,command,allowed=(0,)):
        start=time.monotonic()
        proc=subprocess.run(command,env=env,capture_output=True,text=True,encoding='utf-8',errors='replace',timeout=240,creationflags=FLAGS)
        row={'label':label,'command':command,'exit':proc.returncode,'seconds':time.monotonic()-start,'stdout':proc.stdout,'stderr':proc.stderr}
        steps.append(row);(work/'steps.json').write_text(json.dumps(steps,indent=2)+'\n')
        invariants()
        if proc.returncode not in allowed:raise RuntimeError(f'{label} failed with {proc.returncode}; full MSI log retained under {work}')
    def verify_payload():
        for name,value in files.items():assert digest(target/name)==value,name
    for version in ('1.0.1','1.0.2'):
        output=work/('Fixture-'+version+'.msi')
        mod.build_msi(payload,info,output,args.wix_dll,args.ui_extension,test_namespace=namespace,test_version=version)
        db=inspect(output);props=db['properties']
        assert props['UpgradeCode'].strip('{}').lower()==str(namespace) and props['UpgradeCode'].strip('{}').lower()!=str(mod.UPGRADE_CODE)
        assert props['ProductCode'].strip('{}').lower()==str(uuid.uuid5(namespace,'product:'+version))
        assert props['ProductName']=='Harmony MSI Lifecycle Fixture' and props['ARPSYSTEMCOMPONENT']=='1'
        assert all('InstallerTests' in row[2] for row in db['raw_tables']['Registry'])
        production_guids={str(uuid.uuid5(mod.UPGRADE_CODE,'install-folder'))}|{str(uuid.uuid5(mod.UPGRADE_CODE,'file:'+name.casefold())) for name in files}
        assert not production_guids & {row[1].strip('{}').lower() for row in db['raw_tables']['Component']}
        assert api.MsiQueryProductStateW(props['ProductCode'])==-1,'Fixture product identity is already registered'
        packages.append((output,props['ProductCode']))
    msiexec=str(Path(os.environ['SYSTEMROOT'])/'System32/msiexec.exe')
    installed=[]
    try:
        for index,(package,code) in enumerate(packages):
            label='legacy-upgrade-install' if index==0 else 'major-upgrade'
            run(label,[msiexec,'/i',str(package),'/qn','/norestart','INSTALLFOLDER='+str(target),'REBOOT=ReallySuppress','MSIRESTARTMANAGERCONTROL=DisableShutdown','/l*v',str(work/(label+'.log'))])
            installed.append(code)
            assert api.MsiQueryProductStateW(code)==5,'MSI success did not register the exact fixture product'
            if index:assert api.MsiQueryProductStateW(packages[0][1])==-1,'Old fixture product was not removed by major upgrade'
            verify_payload()
        # Force a real missing-file repair, using the exact MSI engine and payload.
        (target/'yapfast.exe').unlink()
        run('repair',[msiexec,'/fa',str(packages[1][0]),'/qn','/norestart','REBOOT=ReallySuppress','MSIRESTARTMANAGERCONTROL=DisableShutdown','/l*v',str(work/'repair.log')])
        verify_payload()
        run('downgrade-refused',[msiexec,'/i',str(packages[0][0]),'/qn','/norestart','INSTALLFOLDER='+str(target),'REBOOT=ReallySuppress','MSIRESTARTMANAGERCONTROL=DisableShutdown','/l*v',str(work/'downgrade.log')],allowed=(1603,1638))
        downgrade_bytes=(work/'downgrade.log').read_bytes()
        downgrade_text=downgrade_bytes.decode('utf-16' if downgrade_bytes.startswith(b'\xff\xfe') else 'utf-8',errors='replace')
        assert 'A newer Harmony MSI is already installed' in downgrade_text,'Refusal must be the actual newer-version guard, not an unrelated install failure'
        assert api.MsiQueryProductStateW(packages[1][1])==5 and api.MsiQueryProductStateW(packages[0][1])==-1
        verify_payload()
        run('uninstall',[msiexec,'/x',packages[1][1],'/qn','/norestart','REBOOT=ReallySuppress','MSIRESTARTMANAGERCONTROL=DisableShutdown','/l*v',str(work/'uninstall.log')])
        assert api.MsiQueryProductStateW(packages[1][1])==-1
        assert all(not (target/name).exists() for name in files)
        invariants()
        report={'status':'PASS_REAL_ISOLATED_MSI_LIFECYCLE','namespace':str(namespace),'work':str(work),'steps':steps,
                'application_payload_version':info['version'],'application_payload_build':info['build_id'],
                'fixture_msi_versions':['1.0.1','1.0.2'],'fixture_versions_are_not_new_application_builds':True,
                'real_legacy_binary_sha256':'3f6c512cf86088d452f3d3f11a4dac49a11c99254ccf9429ac0db652d0adf96e',
                'expected_current_binary_sha256':before,'payload_files_verified_each_success':len(files),
                'live_program_unchanged':True,'baseline_live_pids_still_running':pids,'fixture_data_and_unowned_file_preserved':True,
                'fixture_products_unregistered':True,'production_product_installed':False,'normal_app_launched_or_closed':False,
                'upgrade_fault_rollback_injection':'not executed; only transaction scheduling statically checked'}
        with (work/'result.json').open('x') as stream:json.dump(report,stream,indent=2);stream.write('\n')
        print(json.dumps({key:value for key,value in report.items() if key!='steps'},indent=2))
    finally:
        # Cleanup only exact generated fixture product IDs, never production IDs.
        for _,code in reversed(packages):
            if api.MsiQueryProductStateW(code)!=-1:
                cleanup=subprocess.run([msiexec,'/x',code,'/qn','/norestart','REBOOT=ReallySuppress','MSIRESTARTMANAGERCONTROL=DisableShutdown','/l*v',str(work/'failure-fixture-cleanup.log')],env=env,capture_output=True,text=True,timeout=180,creationflags=FLAGS)
                assert cleanup.returncode==0 and api.MsiQueryProductStateW(code)==-1,'Fixture cleanup failed; preserve state and escalate'
        for handle in handles:kernel.CloseHandle(handle)

if __name__=='__main__':main()
