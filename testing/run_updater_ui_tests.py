"""Windows cached-dependency check: compile ALL production Slint, then execute
production updater callbacks in a small native testing-backend window. The
adapter fixture exposes only update-related layout/runtime fields; it is not a
full desktop build, network test or test of OS windows/input. No Cargo locks.
"""
from pathlib import Path
import argparse
import json
import os
import subprocess
import sys
import tomllib

def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--deps-root',required=True,type=Path)
    p.add_argument('--output-dir',type=Path)
    args=p.parse_args()
    if sys.platform!='win32': p.error('cached native UI runner is Windows-only')
    root=Path(__file__).resolve().parents[1]
    out=args.output_dir or Path(os.environ['LOCALAPPDATA'])/'hermes/cache/scratch/harmony-updater-ui-tests'
    out.mkdir(parents=True,exist_ok=True)
    env=dict(os.environ)
    env['CARGO_BUILD_JOBS']='2'
    env['RAYON_NUM_THREADS']='2'
    priority=subprocess.BELOW_NORMAL_PRIORITY_CLASS
    vswhere=Path(os.environ.get('ProgramFiles(x86)','C:/Program Files (x86)'))/'Microsoft Visual Studio/Installer/vswhere.exe'
    vs=subprocess.check_output([str(vswhere),'-latest','-products','*','-requires','Microsoft.VisualStudio.Component.VC.Tools.x86.x64','-property','installationPath'],text=True).strip()
    vc=subprocess.run(f'call "{vs}/VC/Auxiliary/Build/vcvars64.bat" >nul && set',shell=True,capture_output=True,text=True,encoding='mbcs',errors='replace',check=True)
    for line in vc.stdout.splitlines():
        if '=' in line and not line.startswith('='):
            key,value=line.split('=',1);env[key]=value
    for key in ['TEMP','TMP','TMPDIR']:env[key]=str(out)
    env['CARGO_BUILD_JOBS']='2'
    env['CARGO_MANIFEST_DIR']=str(root)
    env['CARGO_PKG_VERSION']=tomllib.loads((root/'Cargo.toml').read_text())['package']['version']
    env['HARMONY_BUILD_COMMIT']=subprocess.check_output(['git','-C',str(root),'rev-parse','HEAD'],text=True).strip()
    env['HARMONY_BUILD_ID']='0';env['HARMONY_BUILD_DIRTY']='true'
    env['CARGO_CFG_TARGET_OS']='windows';env['CARGO_CFG_TARGET_ARCH']='x86_64';env['TARGET']='x86_64-pc-windows-msvc'
    env['OPT_LEVEL']='0';env.pop('SLINT_LIVE_PREVIEW',None)
    deps=args.deps_root/'deps'
    from updater_cached_deps import Cache
    cache=Cache(args.deps_root)
    def lib(name):
        return cache.lib(name, '0.10.' if name=='sha2' else '1.' if name=='semver' else None)
    compiler=out/'compile_ui.rs'
    compiler.write_text('fn main() { slint_build::compile_with_config(std::env::args().nth(1).unwrap(), slint_build::CompilerConfiguration::new().with_style("fluent".into())).unwrap(); }')
    compiler_exe=out/'compile-ui.exe'
    subprocess.run(['rustc','--edition=2024',str(compiler),'-L',f'dependency={deps}','--extern',f'slint_build={lib("slint_build")}','-C','target-feature=+crt-static','-C','codegen-units=1','-o',str(compiler_exe)],env=env,check=True,creationflags=priority)
    def compile_ui(source,folder):
        folder.mkdir(exist_ok=True); env['OUT_DIR']=str(folder)
        result=subprocess.run([str(compiler_exe),str(source)],env=env,cwd=root,capture_output=True,text=True,creationflags=priority)
        (folder/'compile.log').write_text(result.stdout+'\n'+result.stderr)
        if result.returncode:print(result.stderr);raise SystemExit(result.returncode)
        generated=next(line.split('=',2)[2] for line in result.stdout.splitlines() if line.startswith('cargo:rustc-env=SLINT_INCLUDE_GENERATED='))
        print(f'Production Slint compile: {source.name}: OK')
        return generated
    compile_ui(root/'ui/yapfast.slint',out/'full-ui')
    slint_file=out/'updater_info_window.slint'
    slint_file.write_text('''import { AboutUpdatesSettings } from "'''+(root/'ui/about_updates.slint').as_posix()+'''";
import { SettingsScreen } from "'''+(root/'ui/settings.slint').as_posix()+'''";
export { HarmonyUpdates } from "'''+(root/'ui/about_updates.slint').as_posix()+'''";
export component UpdaterInfoWindow inherits Window {
    preferred-width: 1000px; preferred-height: 720px;
    in-out property <int> settings-section: 9;
    in-out property <bool> settings-open: true;
    in-out property <bool> preview;
    public function copy-text(text: string) { clipboard.text=text; clipboard.select-all(); clipboard.copy(); clipboard.text=""; }
    clipboard := TextInput { visible: false; width: 0px; height: 0px; }
    SettingsScreen { settings-section <=> root.settings-section; AboutUpdatesSettings { } }
}
''')
    env['SLINT_INCLUDE_GENERATED']=compile_ui(slint_file,out/'focused-ui')
    harness=out/'harness.rs'
    harness.write_text('''#![allow(dead_code, unused_imports)]
use std::{cell::RefCell,rc::Rc,sync::Arc,path::{Path,PathBuf}};
use slint::ComponentHandle;
use yapfast_core::runtime::BackgroundRuntime;
slint::include_modules!();
type YapfastWindow=UpdaterInfoWindow;
const LAYOUT:&str="layout.json";
#[path="'''+(root/'src/harmony_update/mod.rs').as_posix()+'''"] mod harmony_update;
#[path="'''+(root/'src/ui/theme_pack.rs').as_posix()+'''"] mod theme_pack;
#[derive(Clone,Default,serde::Serialize,serde::Deserialize)]
#[serde(default)]
struct Layout { harmony_updates:harmony_update::controller::Preferences }
impl Layout {
    fn try_write(&self,root:&Path)->Result<(),String> { theme_pack::write_atomic(&root.join(LAYOUT),&serde_json::to_vec(self).map_err(|e|e.to_string())?) }
}
// Deliberately minimal adapter fixture; actual production binding and atomic
// persistence primitive are imported, not copied or replaced with success.
struct Adapter { root:PathBuf,layout:Layout,runtime:Arc<BackgroundRuntime>,harmony_updates:Arc<harmony_update::controller::Controller> }
impl Adapter {
    fn new(runtime:Arc<BackgroundRuntime>,root:PathBuf,_window:&YapfastWindow)->anyhow::Result<Rc<RefCell<Self>>> {
        let layout=std::fs::read(root.join(LAYOUT)).ok().and_then(|v|serde_json::from_slice(&v).ok()).unwrap_or_default();
        Ok(Rc::new(RefCell::new(Self {root,layout,runtime,harmony_updates:Arc::new(harmony_update::controller::Controller::new(Err(harmony_update::Error::Unsupported)))})))
    }
}
#[path="'''+(root/'src/ui/updater_info.rs').as_posix()+'''"] mod updater_info;
fn bind_callbacks(window:&YapfastWindow,adapter:&Rc<RefCell<Adapter>>) { updater_info::bind(window,adapter); }
#[path="'''+(root/'src/ui/updater_info_tests.rs').as_posix()+'''"] mod ui_callbacks;
''')
    exe=out/'ui-tests.exe'
    command=['rustc','--edition=2024','--test',str(harness),'-L',f'dependency={deps}','-C','target-feature=+crt-static','-C','codegen-units=1','-o',str(exe)]
    for name in ['slint','i_slint_backend_testing','i_slint_core','yapfast_core','anyhow','tempfile','serde','serde_json','sha2','semver','image']:
        command+=['--extern',f'{name}={lib(name)}']
    for folder in sorted({f.parent for f in (args.deps_root/'build').rglob('*.lib')}):command+=['-L',f'native={folder}']
    for folder in (Path.home()/'.cargo/registry/src').glob('*/windows_x86_64_msvc-*/lib'):command+=['-L',f'native={folder}']
    subprocess.run(command,env=env,cwd=root,check=True,creationflags=priority)
    # Compile an owned gh stand-in. Only the isolated startup test has this PATH;
    # even its RED execution cannot reach the user's official gh/account.
    gh_fixture=out/'gh.exe'
    fixture_command=['rustc','--edition=2024',str(root/'testing/updater_process_fixture.rs'),'-L',f'dependency={deps}','-C','target-feature=+crt-static','-C','codegen-units=1','-o',str(gh_fixture)]
    for name in ['serde','serde_json','sha2','semver']: fixture_command+=['--extern',f'{name}={lib(name)}']
    subprocess.run(fixture_command,env=env,cwd=root,check=True,creationflags=priority)
    bootstrap_fixture=out/'harmony-bootstrap.exe'
    bootstrap_command=list(fixture_command)
    bootstrap_command[bootstrap_command.index(str(root/'testing/updater_process_fixture.rs'))]=str(root/'scripts/harmony_bootstrap.rs')
    bootstrap_command[bootstrap_command.index(str(gh_fixture))]=str(bootstrap_fixture)
    subprocess.run(bootstrap_command,env=env,cwd=root,check=True,creationflags=priority)
    env['HARMONY_TEST_PROCESS_FIXTURE']=str(gh_fixture)
    env['HARMONY_TEST_BOOTSTRAP']=str(bootstrap_fixture)
    result=subprocess.run([str(exe),'ui_callbacks::','--test-threads=1','--nocapture'],env=env,cwd=root,timeout=60,creationflags=priority)
    if result.returncode:return result.returncode
    startup_env=dict(env);startup_env['PATH']=str(out)
    startup_env['HARMONY_FIXTURE_GH_LOG']=str(out/'gh-invocations.log')
    Path(startup_env['HARMONY_FIXTURE_GH_LOG']).unlink(missing_ok=True)
    for name in ['harmony_normal_startup_requires_fresh_and_migrated_consent_before_gh','harmony_normal_startup_exposes_safe_applying_recovery_in_native_ui','harmony_stable_layout_cut_recovery_uses_real_native_startup_and_callbacks']:
        result=subprocess.run([str(exe),'--exact',f'ui_callbacks::{name}','--ignored','--nocapture'],env=startup_env,cwd=root,timeout=60,creationflags=priority,capture_output=True,text=True,encoding='utf-8',errors='replace')
        print(result.stdout);print(result.stderr,file=sys.stderr)
        (out/(name+'.log')).write_text(result.stdout+'\n'+result.stderr,encoding='utf-8')
        if result.returncode:return result.returncode
        if '1 passed; 0 failed;' not in result.stdout:raise RuntimeError('isolated UI test did not execute')
    return 0
if __name__=='__main__':raise SystemExit(main())
