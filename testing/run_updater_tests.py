"""Compile only Harmony's real updater modules, never the desktop or a live app.
Windows uses the integration owner's cached libraries; Linux uses an isolated
small Cargo harness (serde/serde_json/sha2/semver/libc only).
"""
from pathlib import Path
import argparse
import json
import os
import subprocess
import sys
import tomllib
import re
import time


def checked_tests(command, env, scratch, **kwargs):
    result=subprocess.run(command,env=env,capture_output=True,text=True,encoding='utf-8',errors='replace',**kwargs)
    (scratch/f'test-{time.time_ns()}.log').write_text(result.stdout+'\n'+result.stderr,encoding='utf-8')
    print(result.stdout,end='');print(result.stderr,end='',file=sys.stderr)
    if result.returncode:return result.returncode
    summaries=re.findall(r'test result: .*? (\d+) passed; (\d+) failed;',result.stdout)
    if not summaries or sum(map(int,summaries[-1]))==0:
        print('No tests executed; refusing false GREEN.',file=sys.stderr);return 2
    return 0


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--deps-root", type=Path)
    parser.add_argument("--output-dir", type=Path)
    args, rest = parser.parse_known_args()
    root = Path(__file__).resolve().parents[1]
    scratch = args.output_dir or ((Path(os.environ["LOCALAPPDATA"]) / "hermes/cache/scratch") if sys.platform == "win32" else Path.home() / ".cache/hermes-scratch") / "harmony-updater-tests"
    scratch.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ)
    env["CARGO_BUILD_JOBS"] = "2"
    env["RAYON_NUM_THREADS"] = "2"
    env["RUSTFLAGS"] = "-C codegen-units=2"
    priority = subprocess.BELOW_NORMAL_PRIORITY_CLASS if sys.platform == "win32" else 0
    if sys.platform != "win32":
        os.nice(10)  # All isolated Linux compiler/test children inherit nice.
    env["CARGO_PKG_VERSION"] = tomllib.loads((root / "Cargo.toml").read_text())["package"]["version"]
    env["HARMONY_BUILD_COMMIT"] = "79ccfcd252cc69e56fc8082fe960f85cf5b806af"
    env["HARMONY_BUILD_ID"] = "0"
    env["HARMONY_BUILD_DIRTY"] = "true"
    env["TMPDIR"] = env["TEMP"] = env["TMP"] = str(scratch)
    source = '#![allow(dead_code, unused_imports)]\n#[path = "' + (root / "src/harmony_update/mod.rs").as_posix() + '"]\nmod harmony_update;\n'
    if sys.platform != "win32":
        (scratch / "src").mkdir(exist_ok=True)
        (scratch / "src/lib.rs").write_text(source)
        packages=tomllib.loads((root / "Cargo.lock").read_text())["package"]
        def version(name,prefix=""):
            return next(p["version"] for p in packages if p["name"]==name and p["version"].startswith(prefix))
        (scratch / "Cargo.toml").write_text('[package]\nname="harmony-updater-tests"\nversion="1.0.0"\nedition="2024"\n[dependencies]\n' +
            'serde={version="=' + version('serde') + '",features=["derive"]}\n' +
            '\n'.join(name+'="='+version(name,'0.10.' if name=='sha2' else '')+'"' for name in ['serde_json','sha2','semver','libc'])+'\n')
        with (scratch / "Cargo.toml").open("a") as manifest:
            manifest.write('\n[[bin]]\nname="updater-process-fixture"\npath="' + (root / 'testing/updater_process_fixture.rs').as_posix() + '"\n')
        subprocess.run(["cargo", "build", "--jobs", "2", "--manifest-path", str(scratch / "Cargo.toml"), "--bin", "updater-process-fixture"], env=env, check=True)
        env["HARMONY_TEST_PROCESS_FIXTURE"] = str(scratch / "target/debug/updater-process-fixture")
        return checked_tests(["cargo", "test", "--jobs", "2", "--lib", "--manifest-path", str(scratch / "Cargo.toml"), "--", "--test-threads=1", "--include-ignored", *rest], env, scratch,timeout=120)
    if not env.get("VCINSTALLDIR"):
        vswhere = Path(os.environ.get("ProgramFiles(x86)", "C:/Program Files (x86)")) / "Microsoft Visual Studio/Installer/vswhere.exe"
        vs = subprocess.check_output([str(vswhere), "-latest", "-products", "*", "-requires", "Microsoft.VisualStudio.Component.VC.Tools.x86.x64", "-property", "installationPath"], text=True).strip()
        result = subprocess.run(f'call "{vs}/VC/Auxiliary/Build/vcvars64.bat" >nul && set', shell=True, capture_output=True, text=True, encoding="mbcs", errors="replace", check=True)
        for line in result.stdout.splitlines():
            if "=" in line and not line.startswith("="):
                key, value = line.split("=", 1)
                env[key] = value
    env["TMPDIR"] = env["TEMP"] = env["TMP"] = str(scratch)
    env["CARGO_BUILD_JOBS"] = "2"
    if not args.deps_root:
        parser.error("--deps-root must name the owner's existing target/fast directory")
    deps = args.deps_root / "deps"
    harness = scratch / "harness.rs"
    harness.write_text(source)
    exe = scratch / "harmony-updater-tests.exe"
    command = ["rustc", "--edition=2024", "--test", str(harness), "-L", f"dependency={deps}", "-C", "target-feature=+crt-static", "-C", "codegen-units=1", "-o", str(exe)]
    from updater_cached_deps import Cache
    cache=Cache(args.deps_root)
    for name in ["serde_json", "serde", "sha2", "semver"]:
        library=cache.lib(name, "0.10." if name=="sha2" else "1." if name=="semver" else None)
        command += ["--extern", f"{name}={library}"]
    fixture = scratch / "updater-process-fixture.exe"
    fixture_command = [arg for arg in command if arg != "--test"]
    fixture_command[fixture_command.index(str(harness))] = str(root / "testing/updater_process_fixture.rs")
    fixture_command[fixture_command.index(str(exe))] = str(fixture)
    fixture_command += ["-A", "dead_code", "-A", "unused_imports"]
    subprocess.run(fixture_command, env=env, cwd=root, check=True, creationflags=priority)
    env["HARMONY_TEST_PROCESS_FIXTURE"] = str(fixture)
    subprocess.run(command, env=env, cwd=root, check=True, creationflags=priority)
    return checked_tests([str(exe), "--test-threads=1", "--include-ignored", *rest], env, scratch, cwd=root, timeout=120, creationflags=priority)


if __name__ == "__main__":
    raise SystemExit(main())
